//! Bir karenin çizim listesi.
//!
//! `bt-core`'un `frame()` sink'i burayı doğrudan doldurur: grid koordinatı
//! burada piksele çevrilir ve GPU'nun göreceği düzene girer. Renderer "ne
//! çizileceğini" buradan okur, "ne anlama geldiğini" bilmez.
//!
//! Dört liste, iki pipeline: komut bloğu şeritleri, arka planlar (ve imleç)
//! `cell_bg`'nin, glyph'ler ve kural çizgileri `cell`'in. Ayrı durmalarının
//! sebebi çizim sırası — glyph'ler arka planların, kurallar da glyph'lerin
//! **üstüne** gelmek zorunda ve tek listede sıra hücre hücre karışırdı. Glyph
//! ile kuralın ayrı listede olması da aynı cümlenin devamı: ikisi aynı
//! pipeline'dan geçiyor ama üstü çizili, altındaki harften sonra çizilmeli.
//! Şerit `cell_bg`'yi arka planlarla paylaşıyor ama listesi ayrı ve gerekçesi
//! sıra değil **ömür**: [`Frame::move_caret`] arka plan listesini kırpıyor,
//! şerit ise hareket karesinde olduğu gibi kalmalı (bkz. [`Frame::stripes`]).

use std::mem::offset_of;

use bt_atlas::{Face, RuleKind};
use bt_core::{Block, CaretShape, Cell, LinearRgba, UnderlineStyle};

use crate::renderer::CellMetrics;

/// `shaders/cell_bg.metal` → `Instance` ile alan alan aynı.
///
/// Crate dışına açılmaz: bu bir GPU bayt düzeni, `bt-core`'un `Cell`'i ise
/// anlam taşıyan grid koordinatı. İkisini aynı tip yapmak hücre modelini
/// shader düzenine çivilemek olurdu.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    /// **Lineer** RGBA. Hedef `BGRA8Unorm_sRGB` ve kodlamayı ROP yapıyor:
    /// shader tarafında ikinci bir gamma düzeltmesi paleti iki kez kodlar.
    /// Kaynağı `bt_core::color::linear_rgba` (`CLAUDE.md` → renk uzayı).
    rgba: [f32; 4],
}

// MSL tarafında float2 8, float4 16 hizalı; Rust'ta hepsi 4 hizalı ama alan
// ofsetleri ve stride örtüşüyor. Bağlanan üç sayı bunlar — biri kayarsa GPU
// baştan sona yanlış renk/konum okur ve belirti sessizdir. (`pos`'un 0'da
// olması `repr(C)`'nin tanımı, assert edilecek bir şey değil.) Bunlar yalnız
// BU tarafı çiviler; MSL tarafının kendi `static_assert`'leri var.
const _: () = assert!(size_of::<Instance>() == 32);
const _: () = assert!(offset_of!(Instance, size) == 8);
const _: () = assert!(offset_of!(Instance, rgba) == 16);

/// `shaders/cell.metal` → `GlyphInstance` ile alan alan aynı.
///
/// **`size` yok, uv boyutu yok**: bu sette her glyph tam bir hücre boyunda
/// (`plan.md` → R1.4, sabit yuva ızgarası) ve ikisi de kare boyunca sabit,
/// yani instance başına değil uniform olarak geçiyorlar. Kazanç yalnız bant
/// genişliği değil: `{pos, size, uv0, rgba}` düzeni Rust'ta 40, MSL'de 48
/// bayt eder (`float4` 16 hizalı, `[f32; 4]` 4) ve ancak sırf hizalama için
/// var olan bir dolgu alanıyla eşlenirdi. Bu hâlde iki taraf dolgusuz
/// örtüşüyor ve stride `Instance`'la aynı 32.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphInstance {
    /// Hücrenin sol üst köşesi, piksel.
    pub(crate) pos: [f32; 2],
    /// Atlastaki yuvanın sol üst köşesi, **normalize** doku koordinatı.
    pub(crate) uv0: [f32; 2],
    /// Ön plan, **lineer** RGBA; `Instance.rgba` ile aynı uzay ve aynı uyarı.
    pub(crate) rgba: [f32; 4],
}

// `Instance` ile aynı gerekçe, aynı ikili bağ.
const _: () = assert!(size_of::<GlyphInstance>() == 32);
const _: () = assert!(offset_of!(GlyphInstance, uv0) == 8);
const _: () = assert!(offset_of!(GlyphInstance, rgba) == 16);

/// `shaders/cell.metal` → `CursorBlock` ile alan alan aynı: imlecin **piksel**
/// dikdörtgeni ve bloğun altında kalan metnin rengi.
///
/// Instance değil **uniform**: kare boyunca tek imleç var ve `cell`
/// pipeline'ından geçen her fragment ona bakıyor. İki değer tek `#[repr(C)]`
/// yapıda çünkü ikisi tek soruyu yanıtlıyor ("bu fragment bloğun altında mı,
/// altındaysa ne renk") ve tek binding tek düzen sözleşmesi demek — ayrı
/// bağlansalardı çivilenecek bir ofset de kalmazdı.
///
/// Dikdörtgen min/max (`x0, y0, x1, y1`), köşe+boyut değil: fragment testi
/// toplama yapmadan iki karşılaştırmaya iniyor. **Görünmez imleç dejenere bir
/// dikdörtgendir** (hepsi sıfır: `x >= 0 && x < 0` hiçbir fragment için doğru
/// değil) — shader'da ikinci bir bayrak yok, çünkü bayrak ile dikdörtgen
/// ayrışabilen iki gerçek olurdu.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct CursorBlock {
    /// `[x0, y0, x1, y1]`, piksel; sol üst başlangıçlı — fragment'in
    /// `[[position]]`'ı ile aynı uzay.
    rect: [f32; 4],
    /// Bloğun altında kalan metnin **lineer** RGBA'sı; `Instance.rgba` ile
    /// aynı uzay ve aynı uyarı. Kaynağı `bt_core::Cursor::text`, yani karar
    /// `bt-core`'un.
    rgba: [f32; 4],
}

// `Instance` ile aynı gerekçe, aynı ikili bağ; MSL tarafının kendi
// `static_assert`'leri var.
const _: () = assert!(size_of::<CursorBlock>() == 32);
const _: () = assert!(offset_of!(CursorBlock, rgba) == 16);

/// Caret'in bu karedeki instance'ı: yeri ve rengi.
///
/// Ayrı bir tip, çünkü caret'in **iki yuvası** var (ızgara ve dock) ve ikisi
/// de aynı veriyi taşıyor; iki `Option<Instance>` tutmak `size`'ı iki kez
/// yazmak olurdu. Hücre boyu `Frame`'in kendi alanı, o yüzden burada yok.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Caret {
    /// Sol üst köşe, **pencere uzayında** piksel.
    at: [f32; 2],
    /// Bloğun rengi; alfa belirme kipinde pişmiş olarak geliyor.
    rgba: [f32; 4],
}

impl Caret {
    /// Instance'a çevirir; hücre boyunu, şekli ve kural kalınlığını çağıran
    /// veriyor ([`Frame`]'in alanları).
    fn instance(self, cell_px: (f32, f32), shape: CaretShape, rule: f32) -> Instance {
        let (pos, size) = caret_rect(self.at, cell_px, shape, rule);
        Instance {
            pos,
            size,
            rgba: self.rgba,
        }
    }
}

/// Caret'in dikdörtgeni: sol üst köşe ve ölçü, **pencere uzayında** piksel.
///
/// **Tek yer, iki tüketici:** boyanan dörtlü ([`Caret::instance`]) ve ters
/// çevirme dikdörtgeni ([`CursorBlock`]). Ayrı yazılsalardı biri daralıp
/// öteki hücrenin tamamında kalırdı ve belirti sessiz olurdu — ince bir
/// çubuğun altındaki harf, hücre boyunca ters çevrilmiş görünürdü.
///
/// **Kalınlık uydurulmuyor:** fontun kendi alt çizgi metriğinden geliyor
/// (`CellMetrics::rule_px`), chevron emsali. Hücreyi aşamaz — küçük puntoda
/// metrik hücreden büyük çıkabilir ve caret komşu hücreye taşardı.
fn caret_rect(
    at: [f32; 2],
    cell_px: (f32, f32),
    shape: CaretShape,
    rule: f32,
) -> ([f32; 2], [f32; 2]) {
    let thick = rule.clamp(1.0, cell_px.0.min(cell_px.1));
    match shape {
        CaretShape::Block => (at, [cell_px.0, cell_px.1]),
        // Hücrenin **dibinde**, fontun alt çizgi konumunda değil: o konum
        // taban çizgisinin hemen altı ve caret orada `g`'nin kuyruğunu
        // keserdi. Metrikten alınan şey konum değil **kalınlık**.
        CaretShape::Underline => ([at[0], at[1] + cell_px.1 - thick], [cell_px.0, thick]),
        CaretShape::Beam => (at, [thick, cell_px.1]),
    }
}

/// Çizilecek bir glyph — **uv'siz**.
///
/// Karakterin hangi yuvaya düştüğü burada bilinmiyor ve bilinmemeli: yuva
/// çözümü atlası `&mut` ödünç alır ve bu liste `Session::frame`'in sink'inde,
/// yani `Renderer`'a hiç dokunmadan doluyor. Çözümü sink'e hoist etmek
/// atlas ödüncünü `draw` boyunca canlı tutar ve ilk glyph'li karede
/// `BorrowMutError` verirdi (`link.rs` `frame`'i tam olarak öyle tutuyor).
/// Bu ayrım bedava bir garanti de veriyor: bütün kare **tek** atlas
/// kuşağıyla çizilir, kuşak sayacı gerekmeden.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GlyphCell {
    pub(crate) pos: [f32; 2],
    pub(crate) ch: char,
    /// Hangi font yüzünden rasterize edileceği; `(bold, italic)`'in [`face`]
    /// çevirisi. [`GlyphInstance`] bunu **taşımıyor**: uv0 yuvayı, yuva da
    /// yüzü zaten kodluyor.
    pub(crate) face: Face,
    pub(crate) rgba: [f32; 4],
}

/// Çizilecek bir kural çizgisi — [`GlyphCell`]'in kardeşi ve aynı gerekçeyle
/// uv'siz: yuva çözümü atlas ödüncünün yaşadığı yerde (`encode_glyphs`).
///
/// Yüz taşımıyor çünkü kurallar yüzden bağımsız (kalın metnin altındaki çizgi
/// kalın değildir); çağıran onları her zaman [`Face::Regular`] ile soruyor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RuleCell {
    pub(crate) pos: [f32; 2],
    pub(crate) kind: RuleKind,
    pub(crate) rgba: [f32; 4],
}

/// `bt_core`'un SGR bayrakları → `bt_atlas`'ın font yüzü.
///
/// **Çeviri burada, çünkü tek yer.** `bt-atlas` `bt-core`'u görmüyor ve
/// görmemeli: o kenar `alacritty_terminal`'i saf-CoreText crate'ine çekerdi
/// (`CLAUDE.md` → "bağımlılık mimari karardır"); `bt-gpu` ikisini birden gören
/// tek katman. Dört varyantları aynı, **sebepleri ayrı** — biri SGR 1/3
/// semantiği, öteki bir CoreText trait'i. "Aynı görünüyorlar" diye
/// birleştirilirse katman yönü ters döner: birleşik tip ya `bt-core`'a girer
/// (`bt-atlas` onu göremez) ya `bt-atlas`'a (`bt-core` göremez).
fn face(bold: bool, italic: bool) -> Face {
    match (bold, italic) {
        (false, false) => Face::Regular,
        (true, false) => Face::Bold,
        (false, true) => Face::Italic,
        (true, true) => Face::BoldItalic,
    }
}

/// Paletin rengi + bu karenin opaklığı.
///
/// Alfa [`LinearRgba`]'ya **girmiyor** ve bu katman kuralının sonucu: o tip
/// paletin uzayını taşıyor (`bt-core`), opaklık ise bu karenin çizim durumu.
/// Temaya bir alfa alanı açmak "yarı saydam accent" diye ayrıştırılabilir
/// ikinci bir gerçek doğururdu; burada yalnız son bileşen değişiyor.
fn with_alpha(rgba: LinearRgba, alpha: f32) -> [f32; 4] {
    let [r, g, b, _] = rgba.to_array();
    [r, g, b, alpha]
}

/// Alt çizgi çeşidi → kural sprite'ı; [`UnderlineStyle::None`] çizgi istemiyor.
///
/// Beş varyantın beşi de birebir karşılığını buluyor ve `Option` yalnız
/// "çizgi yok"u taşıyor — [`RuleKind`]'ın altıncısı ([`RuleKind::Strike`]) bu
/// çeviriden geçmez, çünkü SGR'de üstü çizili alt çizginin bir çeşidi değil
/// ayrı bir bayrak.
fn rule_kind(underline: UnderlineStyle) -> Option<RuleKind> {
    match underline {
        UnderlineStyle::None => None,
        UnderlineStyle::Single => Some(RuleKind::Single),
        UnderlineStyle::Double => Some(RuleKind::Double),
        UnderlineStyle::Curl => Some(RuleKind::Curl),
        UnderlineStyle::Dotted => Some(RuleKind::Dotted),
        UnderlineStyle::Dashed => Some(RuleKind::Dashed),
    }
}

/// Dock'un kare başına tek olan değerleri: kaç satır ve iki rengi.
///
/// Satır sayısı **oturumun sabiti** ama burada kare başına yeniden yazılıyor
/// ([`Frame::clear`] onu da sıfırlıyor): tek alternatifi `Frame`'e kurucuda
/// girmesiydi ve o, hücre ölçüsünün `clear`'ın parametresi olmasıyla aynı
/// gerekçeyle reddedildi — kurucuda donan bir geometri ekran ölçeği ya da
/// oturum değişince sessizce bayatlar.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DockSurface {
    /// Dock'un yüksekliği, **satır**. Piksel karşılığı `px`.
    rows: u16,
    /// Yüzeyin zemini; **opak** (`bt_core::Dock::ground`). Kayma boyunca
    /// ızgaranın taşan alt satırı bunun altında kalıyor.
    ground: [f32; 4],
    /// Dock'u ızgaradan ayıran saç çizgisi.
    separator: [f32; 4],
}

/// Dock'un yüksekliği, **satır**: giriş satırı + bağlam satırı.
///
/// İki, çünkü dock'un tasarımı iki satır (`plan.md` → Hedef): üstte
/// `>` + ZLE'nin görüntüsü, altta `[klasör] | [dal]`. Pay ızgaranın
/// yüksekliğinden düşülüyor, yani sayıyı sonradan büyütmek kullanıcının
/// penceresini bir satır kısaltan ikinci bir `TIOCSWINSZ` demek.
///
/// Bu crate'in sabiti çünkü çizen bu crate; `bt-shell` onu ızgara
/// aritmetiğinde (`split_into_grid`) **tüketiyor** ve ikinci bir kopya
/// tutmuyor — payın `CellMetrics` ile taşınmasıyla aynı disiplin.
pub const DOCK_ROWS: u16 = 2;

/// Dock'un kapladığı yükseklik, **piksel**; `dock_rows == 0` ise sıfır.
///
/// Formülün **tek** kopyası burası ve iki tüketicisi var: ızgaranın satır
/// aritmetiği (`bt_shell`'in `split_into_grid`'i) ve ikinci viewport'un
/// orijini ([`Frame::dock_px`]). Ayrı ayrı yazılsalardı yeniden boyutlandırmada
/// bir kare boyunca ayrışırlardı — `DOCK_ROWS`'un `bt-shell` tarafından
/// tüketilmesiyle aynı disiplin.
///
/// **Nefes payı satırların üstünde ve altında** (`2 *`): iki satır saç
/// çizgisine yapışınca dock "çirkin" duruyordu (kullanıcı, 012 phase-9).
/// Kullanıcı bitişik + nefes paylı görünümü seçti, ayrık yüzeyi değil.
///
/// Payın kaynağı **sol payın ta kendisi** ([`CellMetrics::gutter_px`]): ikinci
/// bir tasarım sabiti uydurulmadı, aynı içi girinti iki eksende kullanılıyor.
/// Sabit bir piksel sayısı da olmazdı — Cmd +/− ile punto büyüyünce pay aynı
/// kalır ve oran bozulurdu; `gutter_px` ölçekle zaten çarpılıyor.
pub fn dock_px(dock_rows: u16, cell: CellMetrics) -> f32 {
    dock_height(
        dock_rows,
        f32::from(cell.cell_px().1),
        f32::from(cell.gutter_px()),
    )
}

/// Formülün gövdesi, ham sayılarla: [`dock_px`] ile [`Frame`] aynı aritmetiği
/// paylaşsın diye ayrı. `Frame` [`CellMetrics`]'i alan olarak tutamıyor
/// (kurucusu sıfırı eliyor, yani `Default`'u yok), ama iki bileşeni zaten
/// elinde.
fn dock_height(rows: u16, cell_h: f32, pad: f32) -> f32 {
    if rows == 0 {
        return 0.0;
    }
    f32::from(rows) * cell_h + 2.0 * pad + f32::from(rows - 1) * dock_row_gap(pad)
}

/// Dock satırlarının **arasındaki** boşluk, piksel.
///
/// Dış payın **iki katı** ve bu sayı bir zevk değil, tek bir kuralın sonucu:
/// araya bir saç çizgisi girdiği için her satır kendi **bandı** oldu ve bandın
/// içi simetrik olmalı. Çizginin iki yanına birer `pad` düşünce dock'un dört
/// boşluğu da eşitleniyor:
///
/// ```text
///   ─────────── üst saç çizgisi
///        pad
///   giriş satırı
///        pad
///   ─────────── satır arası çizgi
///        pad
///   bağlam satırı
///        pad
///   ─────────── dock'un dibi
/// ```
///
/// phase-9'da `pad / 2`'ydi ve gerekçesi "dış boşluk içtekinden büyük"tü. O
/// kural **gruplar** için doğru ama burada grup yok: çizgi iki satırı iki ayrı
/// şeye çeviriyor ve o hâlde giriş satırının üstünde `pad`, altında `pad / 2`
/// kalıyordu — kullanıcı gördü ("alttan ve üstten çizgiler aynı uzaklıkta
/// olmalı"), üstelik görmesi gerekmeyen bir şeydi.
///
/// Yuvarlanıyor, çünkü aygıt ızgarasına oturmayan bir kayma bütün dock
/// metnini bulanıklaştırırdı — `Frame::set_origin_rows`'un aynı gerekçesi.
fn dock_row_gap(pad: f32) -> f32 {
    (pad * 2.0).round()
}

/// Ayracın kalınlığı, **piksel**.
///
/// Ölçekle çarpılmıyor ve bu bilinçli: saç çizgisi bir çizgidir, @2x'te iki
/// piksel olması onu kalınlaştırırdı — retina ekranın kazandırdığı incelik tam
/// da bu. Ölçülmüş bir sayı değil, bir tasarım sabiti (`CellMetrics::GUTTER_PT`
/// emsali).
const SEPARATOR_PX: f32 = 1.0;

/// Tek karede çizilecekler.
///
/// Hücre arka planları ve imleç aynı listede yaşar: ikisi de aynı pipeline'la
/// çizilir, sıra çizim sırasıdır (imleç arka planların üstüne gelsin diye
/// sona eklenir). Glyph'ler ayrı listede ve ikinci pipeline'la, imlecin de
/// üstüne çizilir — imleç opak ve altındaki harfi örterdi.
///
/// Uzun ömürlüdür: display link onu ivar'da tutar ve **içerik** karesinde
/// `clear` ile yeniden doldurur. Bu yüzden ızgara geometrisi (hücre ölçüsü ve
/// sol pay) **alan değil `clear`'ın parametresidir** — kurucuda
/// dondurulsaydı ekran ölçeği değiştiğinde
/// (`windowDidChangeBackingProperties:`) sessizce bayatlardı.
///
/// **Her kare `clear` görmüyor ve bu 008'in getirdiği ayrım:** hareket karesi
/// grid'i kirli bulmadan çiziyor, yani listeyi temizleyemez —
/// [`Frame::move_caret`] onu koruyarak yalnız imleci taşıyor. `clear`'ın
/// çağrıldığı tek yer içerik karesi.
#[derive(Default)]
pub(crate) struct Frame {
    /// Komut bloklarının sol paydaki şeritleri; arka planlarla **aynı**
    /// pipeline'dan ama ayrı listede.
    ///
    /// Ayrılığın sebebi çizim sırası değil ömür (010 → R4.1): `bg`'ye
    /// girseydi ya sayılmadan girerdi — [`Frame::move_caret`]'ın
    /// `truncate(bg_count)`'u onu her hareket karesinde siler ve şerit imleç
    /// kaydıkça **titrerdi** — ya da sayılarak girer ve `hucre=` jetonunun
    /// anlamı kayardı ("çizilen hücre" artık hücre olmayan bir şeyi de
    /// sayardı). Üçüncü bir liste ikisini de temsil edilemez kılıyor.
    stripes: Vec<RuleCell>,
    bg: Vec<Instance>,
    glyphs: Vec<GlyphCell>,
    /// Kural çizgileri; glyph'lerle **aynı** pipeline'dan ama onlardan sonra
    /// çizilir (üstü çizili, altındaki harfin üstünden geçmeli).
    rules: Vec<RuleCell>,
    cell_px: (f32, f32),
    /// Kural çizgisinin kalınlığı, piksel — ince caret'lerin genişliği
    /// ([`caret_rect`]). `cell_px` ile aynı gerekçeyle alan: hareket karesi
    /// `clear` çağırmıyor ve değeri **koruyor**.
    rule_px: f32,
    /// Caret'in şekli; [`Frame::push_caret`] yazıyor, [`Frame::move_caret`]
    /// koruyor — o yol `bt-core`'a hiç gitmiyor.
    caret_shape: CaretShape,
    /// Izgaranın sol payı: her hücrenin x'i buradan **sonra** başlar.
    ///
    /// `cell_px` ile aynı gerekçeyle alan değil [`Frame::clear`]'ın taşıdığı
    /// bir değer (ikisi de tek [`CellMetrics`] ile geliyor): ölçek değişince
    /// ikisi birlikte tazelenir. Ayrı bir sabitten okunsaydı `cols` hesabıyla
    /// ayrışabilirdi — üçünün tek kaynağı olması 010 Karar 3'ün şartı.
    gutter_px: f32,
    /// Dock bandının tepesi, **pencere uzayında piksel**; dock yoksa
    /// sonsuz (caret hiçbir zaman dock yuvasına düşmez).
    ///
    /// Çağıran yazıyor ([`Frame::set_dock_top`]), çünkü dokunun boyunu bilen
    /// tek yer kare yolu; `Frame` listelerin uzayını biliyor, pencereninkini
    /// değil ([`Frame::dock_ground`]'un genişliği argüman almasıyla aynı
    /// gerekçe).
    dock_top_px: f32,
    /// Caret'in ızgara yuvası: ızgaranın arka planlarından sonra, glyph'lerinden
    /// önce çizilir.
    grid_caret: Option<Caret>,
    /// Caret'in dock yuvası: dock'un opak zemininden sonra çizilir, yani her
    /// şeyin üstünde.
    dock_caret: Option<Caret>,
    /// Izgaranın dikey orijini, piksel: içerik bu kadar **aşağıdan** başlar.
    ///
    /// `gutter_px`'in dikey ikizi ama **listelere işlenmiyor**:
    /// [`Frame::pos_at`] onu görmüyor ve görmemeli. Sebep sıra — sink döngünün
    /// içinde koşuyor ve hücreyi basma anında `Instance`'a pişiriyor, oysa
    /// doluluk sayısı ancak döngü bitince doğuyor
    /// (`bt_core::Cursor::content_rows`). Öteleme bu yüzden **çizim zamanı**
    /// uygulanıyor: `setViewport` dört listeyi birden kaydırıyor
    /// ([`crate::Renderer`]) ve instance başına maliyeti sıfır.
    ///
    /// Tek istisna imleç: hedefi **ekran** satırı ve ötelemeden muaf, yani
    /// onun instance'ı ötelemeyi CPU'da **geri veriyor**, dikdörtgeni ise
    /// hiç almıyor ([`Frame::push_caret`]).
    ///
    /// Satır değil **piksel** ve `f32`: kayma iki satır arasında duruyor.
    /// Değerin kendisi yine de **aygıt pikseline yuvarlı**
    /// ([`Frame::set_origin_rows`]) — kesirli olan **satır**, piksel değil.
    ///
    /// [`Frame::clear`] sıfırlıyor, yani içerik karesi değeri her karede
    /// yeniden söylemek zorunda; hareket karesi `clear` çağırmıyor ama
    /// ötelemeyi **yine de yazıyor** (`link.rs`'in ikinci yazma noktası):
    /// kayma iki içerik karesi arasında ilerliyor ve korunan bir değer
    /// ilerleyemez.
    origin_px: f32,
    /// İmlecin piksel dikdörtgeni ve blok altındaki metin rengi; `cell`
    /// pipeline'ının uniform'u.
    ///
    /// Liste değil **alan**: kare başına tek imleç var ve [`Frame::clear`] onu
    /// dejenereye döndürüyor. Alan olması hareket karesinin de şartı
    /// (`plan.md` → Karar 4): o yol `bg`'yi `bg_count`'a kırpıp
    /// [`Frame::push_caret`]'ı yeni konumla yeniden çağırıyor, yani ikinci
    /// çağrı birincinin üstüne yazmak zorunda.
    cursor: CursorBlock,
    /// Dock yüzeyi: pencerenin altındaki **ikinci koordinat uzayı**.
    ///
    /// `Option`, çünkü dock oturum doğarken kararlaşıyor (entegrasyonlu zsh mi)
    /// ve `None` "bu pencerede dock yok" demek — `bt-shell` ızgara
    /// yüksekliğini de ona göre hesaplıyor. Boş bir dock'la `None` arasındaki
    /// fark görünür: boş dock zeminini çiziyor, `None` hiçbir şey çizmiyor.
    dock: Option<DockSurface>,
    /// Dock'un kendi arka planları **ve caret'i**; ızgaranın `bg`'sinin ikizi.
    ///
    /// Ayrı liste olması `stripes` ile **aynı** gerekçe ve bir derece daha
    /// zorunlu: [`Frame::move_caret`]'ın `truncate(bg_count)`'u `bg`'ye giren
    /// her şeyi her hareket karesinde siler, yani dock ızgaranın imleci
    /// kaydıkça **titrerdi**. Sayaçlara da girmiyor (`bg_count`,
    /// `glyph_count`, `rule_count`): `hucre=8 glif=6 kural=15` duman
    /// sözleşmesi dock'suz bir kabukta ölçülüyor ve anlamı bit bit korunmalı.
    dock_bg: Vec<Instance>,
    dock_glyphs: Vec<GlyphCell>,
    dock_rules: Vec<RuleCell>,
    /// Çizilen **arka plan** instance'ı sayısı; imleç sayılmaz.
    ///
    /// `make duman`'ın `hucre=K` jetonu bunu okur: sink'in hücre ürettiğinin
    /// kanıtı. İmleç sayıya girseydi K boş bir grid'de bile 1 olur ve iddiayı
    /// boşa çıkarırdı. Dikkat: bu bir **CPU** sayacıdır, GPU'nun o hücreleri
    /// boyadığını kanıtlamaz — onu `renderer`'ın offscreen okuma sınaması yapar.
    bg_count: usize,
}

// Tümü `pub(crate)`: `Frame`'i dolduran tek yer `link.rs`, yani bu crate.
// Kare listesi bir GPU ayrıntısıdır; `bt-shell`'in onu görmesi için bir sebep
// yok ve görmezse yanlış hücre boyutuyla dolduramaz.
impl Frame {
    /// Tamponları boşaltır ve bu karenin ızgara geometrisini kurar. Ayrılan
    /// yer korunur: kare başına yeniden ayırma yok.
    ///
    /// Demet değil [`CellMetrics`]: hücre ölçüsü ile sol pay aynı çağrıdan
    /// geliyor ve burada da birlikte yazılıyorlar. Ayrı iki parametre olsaydı
    /// biri tazelenip öteki unutulabilirdi ve belirti "glyph'ler pay kadar
    /// kaymış" olurdu.
    pub(crate) fn clear(&mut self, metrics: CellMetrics) {
        let cell_px = metrics.cell_px();
        self.stripes.clear();
        self.bg.clear();
        self.glyphs.clear();
        self.rules.clear();
        self.bg_count = 0;
        // Dock da içerik karesinin sözleşmesinde: yüzeyi **her karede**
        // yeniden açılıyor (`Frame::open_dock`). Korunsaydı dock'u olmayan bir
        // oturumda son karenin yüzeyi ekranda asılı kalırdı.
        self.dock = None;
        self.dock_bg.clear();
        self.dock_glyphs.clear();
        self.dock_rules.clear();
        // Dikdörtgen de sıfırlanıyor ([`Frame::clear_caret`]): kalsaydı
        // imlecin sönmesi (`\e[?25l`) ya da geçmişe kayması bloğu ekrandan
        // kaldırır ama **altındaki metnin rengini** eski yerinde bırakırdı —
        // zemin renginde bir harf, yani görünmez bir hücre.
        self.clear_caret();
        self.cell_px = (f32::from(cell_px.0), f32::from(cell_px.1));
        self.rule_px = f32::from(metrics.rule_px());
        self.gutter_px = f32::from(metrics.gutter_px());
        // **Sonsuz**, sıfır değil: sıfır "dock bandı pencerenin tepesinde"
        // demek olurdu ve her caret dock yuvasına düşerdi. Çağıran her içerik
        // karesinde üstüne yazıyor ([`Frame::set_dock_top`]); hareket karesi
        // `clear` çağırmadığı için değeri **koruyor** ve caret'in yuva kararı
        // animasyon boyunca aynı bandı görüyor.
        self.dock_top_px = f32::INFINITY;
        // Orijin **sıfırlanıyor**, geometriden gelmiyor: kaynağı bu karenin
        // doluluk sayısı ve o ancak sink döngüsü bitince biliniyor. Sıfırda
        // bırakmak "bu kare daha söylemedi" demek ve söylemeyen bir kare
        // bugünkü (tavana yapışık) yerleşimi çiziyor — sessiz bir yanlış
        // ötelemeden iyi.
        self.origin_px = 0.0;
    }

    /// Bu karenin dikey orijini, **satır** cinsinden: içerik bu kadar aşağıdan
    /// başlar.
    ///
    /// Satır alıyor piksel saklıyor, çünkü çeviri hücre boyunu ister ve o
    /// yalnız burada (`clear`'ın yazdığı `cell_px`). Çağıranın (`link.rs`)
    /// pikselle uğraşması hücre ölçüsünün ikinci bir okuyucusu demekti.
    ///
    /// **Kesirli satır meşru, kesirli piksel değil.** Kayma iki satır arasında
    /// duruyor ama piksel **aygıt ızgarasına yuvarlanıyor**, çünkü kaymanın
    /// durduğu yer ekranda kalıcı: link'in "hasar yok" dalı animasyonun
    /// *yerleştiği* kareyi hiç çizmeden uyuyor, yani ekranda kalan son kare
    /// yerleşmeden bir adım öncesi. İmleç için bu yarım pikselin altında bir
    /// fark (`crate::motion::POS_EPSILON`), **bütün metin** için yarım piksel
    /// kaymış bir ızgara — yani her Enter'dan sonra bulanıklaşan bir ekran.
    /// Yuvarlama onu kapatıyor ve kaymanın kendisini de keskinleştiriyor:
    /// metin tam piksel adımlarıyla ilerliyor.
    ///
    /// Satır alıyor piksel saklıyor demenin ikinci sonucu bu: yuvarlama ancak
    /// hücre boyunun bilindiği yerde yapılabilir.
    pub(crate) fn set_origin_rows(&mut self, rows: f32) {
        debug_assert!(self.cell_px.1 > 0.0, "clear(metrics) çağrılmadı");
        self.origin_px = (rows * self.cell_px.1).round();
    }

    /// Bu karenin dikey orijini, piksel; `setViewport`'un `originY`'si.
    pub(crate) fn origin_px(&self) -> f32 {
        self.origin_px
    }

    /// Sink'in tek girişi: hücrenin arka planı varsa boyanır, mürekkebi varsa
    /// çizilir, kuralı varsa çizilir — üçü de varsa üçü de.
    ///
    /// Bir hücre **ikiye kadar** kural üretir: alt çizgi ve üstü çizili. İkisi
    /// `bt-core`'da ayrı alanlar çünkü SGR'de de ayrılar; aynı hücrede
    /// buluştuklarında ikisi de çizilir.
    pub(crate) fn push(&mut self, cell: Cell) {
        // Dört dalın (arka plan, glyph, alt çizgi, üstü çizili) ortak
        // aritmetiği bir kez: hücre başına dört kez `pos()` çağırmanın kazancı
        // yok ve ayrışabilen dört kopya demek.
        let pos = self.pos(cell.col, cell.row);
        if let Some(bg) = cell.bg {
            // `bg.len() > bg_count` tam olarak "imleç eklendi" demektir.
            debug_assert_eq!(
                self.bg.len(),
                self.bg_count,
                "arka plan imleçten sonra eklendi: imleç gömülür"
            );
            self.bg.push(Instance {
                pos,
                size: [self.cell_px.0, self.cell_px.1],
                rgba: bg.to_array(),
            });
            self.bg_count += 1;
        }
        // Mürekkebi olmayan hücre glyph üretmez: atlasta yuva, tamponda
        // instance ve GPU'da tamamen şeffaf bir dörtlü harcardı. Ayrımı
        // `bt-core` yapıyor (boşluk, gizli metin, geniş karakterin ikinci
        // hücresi hepsi `None`), burada sorulacak bir bayrak yok. Kural
        // dalları buna **bağlı değil**: altı çizili bir boşluk mürekkepsizdir
        // ama çizgisini alır (duman reçetesinde yedi tane var).
        if let Some(ch) = cell.ch {
            self.glyphs.push(GlyphCell {
                pos,
                ch,
                face: face(cell.bold, cell.italic),
                rgba: cell.fg.to_array(),
            });
        }
        if let Some(kind) = rule_kind(cell.underline) {
            self.rules.push(RuleCell {
                pos,
                kind,
                // SGR 58 varsa o, yoksa ön plan (`bt-core` → R3.5).
                rgba: cell.underline_color.unwrap_or(cell.fg).to_array(),
            });
        }
        if cell.strikeout {
            self.rules.push(RuleCell {
                pos,
                kind: RuleKind::Strike,
                // Üstü çizili SGR 58'i **kullanmaz**: SGR'de üstü çizilinin
                // ayrı bir rengi yok ve `underline_color` adıyla alt çizginin.
                rgba: cell.fg.to_array(),
            });
        }
    }

    /// Bir komut bloğunun işareti: **0. sütuna**, komutun kendi satırına
    /// çizilen chevron.
    ///
    /// **Renk üretilmiyor, taşınıyor.** `bt-core` "hangi satırlar, hangi renk"
    /// sorusunu çözülmüş veriyor ([`Block`]); burada çıkış kodu tanıyan bir dal
    /// yanlış yerde olurdu (`CLAUDE.md` → karar burada, boyama orada).
    ///
    /// [`Frame::pos`]'tan **geçiyor** ve geçmesi şart: dock'un prompt işareti
    /// de aynı satırdan geçiyor (`dock::render`, sütun 0), yani iki işaretin
    /// hizası hesaplanan bir şey değil, tek formülün sonucu. Ayrı bir
    /// aritmetikle yerleştirildiği sürece — payın ortasında — dock'unkinden
    /// yarım pay kadar solda duruyordu.
    ///
    /// İşaretin oturduğu sütun **boş**, çünkü prompt gerçekten iki sütun geniş
    /// (`assets/shell/zsh/bateri.zsh` → `__bateri_ps1`, `dock::TEXT_COL` ile
    /// aynı sayı). İşaret komutun harfini örtmüyor ve çizim terminale yalan
    /// söylemiyor — alternatifi komut satırını çizerken kaydırmaktı ve fare
    /// eşlemesini de satır sarmayı da bozardı.
    ///
    /// Sprite tam bir hücre boyunda (`cell` pipeline'ının sabit yuvası) ama
    /// ink'i hücrenin ortasına toplu (`bt_atlas::raster::chevron`).
    pub(crate) fn push_block(&mut self, block: Block) {
        let h = self.cell_px.1;
        debug_assert!(h > 0.0, "clear(metrics) çağrılmadı");
        // **İşaret artık bir dikdörtgen değil, dock'un chevron'unun ta
        // kendisi** (012 phase-9, kullanıcı: "ızgara kısmında sonuç renk
        // kutuları da bu yeni > olacak, renkleri aynı kalacak"). İkisi zaten
        // aynı şeyi söylüyordu — safha renginde bir prompt işareti — ve iki
        // ayrı şekille çizilmeleri bir tasarım kararı değil, bir kalıntıydı.
        //
        // Sprite tam bir hücre boyunda (`cell` pipeline'ının sabit yuvası) ama
        // ink'i hücrenin ortasına toplu (`bt_atlas::raster::chevron`), yani
        // sol payın içinde kalıyor.
        //
        // **0. SÜTUNDA, payın içinde değil.** Dock'un prompt işareti de orada
        // (`dock::render`, sütun 0), yani iki işaretin hizası hesaplanmıyor —
        // ikisi de `Frame::pos`'tan geçiyor ve aynı formülden doğuyor. Payın
        // ortasına konduğu sürece dock'un işaretinden yarım pay kadar solda
        // duruyordu ve kullanıcı bunu gördü.
        //
        // Sütunu komuta çarpmıyor, çünkü prompt artık **gerçekten** iki sütun
        // geniş (`assets/shell/zsh/bateri.zsh` → `__bateri_ps1`): komut
        // metni 2. sütundan başlıyor ve işaret 0.'daki boşluğun üstüne
        // düşüyor. Çizim terminale yalan söylemiyor, o yüzden fare eşlemesi
        // ve satır sarma dokunulmadan kalıyor.
        //
        // Yükseklik tam bir hücre: işaret komutun satırını gösteriyor, bir
        // aralığı değil (`bt_core::Block`). Satır aralığı sınırı geçmiyor,
        // yani burada doğrulanacak bir "ters aralık" da kalmadı.
        self.stripes.push(RuleCell {
            pos: self.pos(0, block.row),
            kind: RuleKind::Chevron,
            rgba: block.stripe.to_array(),
        });
    }

    /// İmleç bloğu; `bg_count`'a **girmez** ve görünmez imleç çizilmez.
    ///
    /// İki şey birden yazıyor ve bilerek tek çağrıda: bloğun kendisi arka plan
    /// listesine bir dikdörtgen olarak (`rgba` — temanın vurgusu), bloğun
    /// **altında kalan metnin** rengi ise `cell` pipeline'ının uniform'una
    /// (`cursor.text` — `bt-core`'un kararı). İkisi ayrı çağrı olsaydı biri
    /// çağrılıp öteki unutulabilirdi ve belirti sessiz olurdu: imleç doğru
    /// yerde, altındaki harf okunmaz.
    ///
    /// **Konum `cursor`'dan gelmiyor, `at`'ten** ve hücre biriminde `f32`:
    /// kayan imleç iki hücre arasındayken tam sayı değil. `cursor` yine de
    /// gerekli, çünkü ötekiler (`visible`, `text`) `bt-core`'un kararı ve
    /// konuma bağlı değil — ara konum çizenin, hedef sınırın.
    ///
    /// **`at` bir ekran satırı, grid satırı değil** (`crate::motion`, R2.1):
    /// imleç ötelemeden muaf, çünkü Enter'da grid satırı bir artarken öteleme
    /// bir azalıyor ve imlecin ekrandaki yeri hiç değişmiyor. İçerik arkasından
    /// yukarı akarken imleç dipteki satırında duruyor.
    ///
    /// **`alpha` ikisine birden yazılıyor** (`crate::motion::Motion::alpha`):
    /// Hareketi Azalt açıkken imleç yeni hücresinde belirir ve blok ile
    /// altındaki metnin rengi **birlikte** belirmek zorunda. Ayrılsalardı harf
    /// henüz görünmeyen bir bloğun rengine boyanırdı — zeminin üstünde zemin
    /// renginde bir harf, yani okunmayan bir hücre. Belirme dışında `1.0`,
    /// yani bu yol her kare aynı iki değeri taşıyor.
    pub(crate) fn push_caret(
        &mut self,
        at: [f32; 2],
        text: LinearRgba,
        rgba: LinearRgba,
        alpha: f32,
        shape: CaretShape,
    ) {
        let pos = self.pos_at(at);
        let top = pos[1];
        // **Şekil `Frame`'de yaşıyor**, imzada taşınıp unutulmuyor: hareket
        // karesi ([`Frame::move_caret`]) `bt-core`'a hiç gitmiyor ve şekli
        // bilmiyor. Alan olmasaydı ilk hareket karesinde beam bloğa dönerdi
        // ve belirti "imleç bazen şekil değiştiriyor" olurdu.
        self.caret_shape = shape;
        // **Dikdörtgen pencere uzayında ve iki glyph encode'una da gidiyor.**
        // Fragment'in `[[position]]`'ı ile karşılaştırılıyor ve o koordinat
        // viewport dönüşümünden **sonraki**, yani hem ızgaranın hem dock'un
        // glyph'leri için aynı uzay. `at` ekran satırı olduğu için çevirme
        // gerekmiyor — eskiden dock'un kendi dikdörtgeni vardı ve encode onu
        // `shifted_y` ile taşıyordu; tek caret o çeviriyi büsbütün kaldırdı.
        let (rect_pos, rect_size) = caret_rect(pos, self.cell_px, shape, self.rule_px);
        self.cursor = CursorBlock {
            rect: [
                rect_pos[0],
                rect_pos[1],
                rect_pos[0] + rect_size[0],
                rect_pos[1] + rect_size[1],
            ],
            rgba: with_alpha(text, alpha),
        };
        // **Yuva seçimi boyacı algoritmasının zorunluluğu.** Blok, üstünde
        // duracağı yüzeyin zemininden **sonra** ama glyph'lerinden **önce**
        // çizilmek zorunda: ızgaranın yuvasında kalsaydı dock'un opak zemini
        // onu örterdi, dock'un yuvasında kalsaydı ızgaranın harfini boyardı.
        // Ölçüt örtüşme: caret dock bandına bir piksel bile girdiyse dock'un
        // yuvasına geçiyor ve orada **en üstte** kalıyor — devir karelerinde
        // yarısı kırpılmış bir blok görünmesin diye.
        //
        // Tek caret, tek dikdörtgen, tek instance; değişen yalnız hangi
        // encode'a girdiği.
        let caret = Caret {
            at: pos,
            rgba: with_alpha(rgba, alpha),
        };
        if top + self.cell_px.1 > self.dock_top_px {
            self.dock_caret = Some(caret);
        } else {
            self.grid_caret = Some(caret);
        }
    }

    /// **Hareket karesinin tek ucu**: listeyi koruyarak imleci taşır.
    ///
    /// Grid kirli değil, yani glyph ve kural listeleri hâlâ geçerli — onları
    /// yeniden kurmak `Term` kilidine saniyede 120 kez girmek olurdu ve
    /// "render yolu bloklanmaz" ile tam orada kavga ederdi (008 Karar 4).
    /// Arka plan listesi `bg_count`'a kırpılıyor: kırpılan tek şey önceki
    /// karenin imleci, çünkü [`Frame::push`] arka planları imleçten **önce**
    /// eklemek zorunda ve bunu bir `debug_assert` tutuyor. Kırpma o bekçiyi
    /// geçerli bırakıyor — liste her hâlükârda "önce arka planlar, sonra
    /// imleç" düzeninde kalıyor.
    ///
    /// **Şerit listesine dokunmuyor** ve bu da aynı cümlenin parçası: ızgara
    /// değişmediyse blokların satır aralığı da değişmedi, yani şerit de
    /// değişmemeli. Şerit ayrı listede olmasaydı bu kırpma onu silerdi.
    pub(crate) fn move_caret(
        &mut self,
        at: [f32; 2],
        text: LinearRgba,
        rgba: LinearRgba,
        alpha: f32,
    ) {
        // Şekil **korunuyor**: bu yolun `bt-core`'a erişimi yok ve
        // `clear_caret` yalnız yuvaları boşaltıyor.
        let shape = self.caret_shape;
        self.clear_caret();
        self.push_caret(at, text, rgba, alpha, shape);
    }

    /// Caret'in üç yuvasını da boşaltır: iki instance ve dikdörtgen.
    ///
    /// Ayrı fonksiyon, çünkü iki çağıranı var ve ikisi de **hepsini** boşaltmak
    /// zorunda: [`Frame::clear`] (yeni kare) ve [`Frame::move_caret`] (hareket
    /// karesi). Biri unutulsaydı devir karesinde caret iki yerde birden
    /// çizilirdi — yuva değişiyor ama eskisi temizlenmiyor.
    fn clear_caret(&mut self) {
        self.grid_caret = None;
        self.dock_caret = None;
        self.cursor = CursorBlock::default();
    }

    /// Dock'un bir hücresi; [`Frame::push`]'un ikizi ama **dock-yerel**
    /// koordinatta ve sayaçlara girmeden.
    ///
    /// Satır `bt-core`'dan dock-yerel geliyor (0 = giriş satırı) ve burada da
    /// öyle kalıyor: dock'u ekrana taşıyan şey ikinci `setViewport`
    /// ([`crate::Renderer`]), aritmetik değil. Ötelemeyi (`origin_px`) hiç
    /// görmemesi de bundan — muafiyet **yapısal**, çıkarmayla değil.
    ///
    /// Sol payı ızgarayla paylaşıyor ([`Frame::pos_at`]): dock'un sütunları
    /// ızgaranınkilerle hizalı ve şeridin ayrıldığı pay dock'ta da boş kalıyor.
    pub(crate) fn push_dock(&mut self, cell: Cell) {
        let pos = self.dock_pos(cell.col, cell.row);
        if let Some(bg) = cell.bg {
            // Caret artık bu listede **değil**: kendi yuvası var ve encode onu
            // dock'un arka planlarından sonra çiziyor ([`Frame::push_caret`]),
            // yani "caret'ten sonra arka plan eklenmesin" bekçisine de gerek
            // kalmadı — sıra listede değil encode'da.
            self.dock_bg.push(Instance {
                pos,
                size: [self.cell_px.0, self.cell_px.1],
                rgba: bg.to_array(),
            });
        }
        if let Some(ch) = cell.ch {
            self.dock_glyphs.push(GlyphCell {
                pos,
                ch,
                face: face(cell.bold, cell.italic),
                rgba: cell.fg.to_array(),
            });
        }
        if let Some(kind) = rule_kind(cell.underline) {
            self.dock_rules.push(RuleCell {
                pos,
                kind,
                rgba: cell.underline_color.unwrap_or(cell.fg).to_array(),
            });
        }
        if cell.strikeout {
            self.dock_rules.push(RuleCell {
                pos,
                kind: RuleKind::Strike,
                rgba: cell.fg.to_array(),
            });
        }
    }

    /// Dock'un prompt işareti: ilk satırın ilk sütununda, safha renginde.
    ///
    /// Ayrı çağrı, çünkü işaret bir **hücre değil** — `bt-core` sınırdan yalnız
    /// rengini veriyor (`bt_core::Dock::sigil`) ve şekli bu katmanın kararı.
    /// Karakter olarak geçseydi kullanıcının fontunun `>`'ü çizilirdi; oysa
    /// işaret terminalin kendisi ve ızgaranın blok işaretiyle **aynı** sprite
    /// ([`Frame::push_block`]).
    pub(crate) fn push_dock_sigil(&mut self, rgba: LinearRgba) {
        self.dock_rules.push(RuleCell {
            pos: self.dock_pos(0, 0),
            kind: RuleKind::Chevron,
            rgba: rgba.to_array(),
        });
    }

    /// Dock yüzeyini bu kare için açar: kaç satır ve iki rengi.
    ///
    /// Hücrelerden **sonra** çağrılıyor ve bu bir sıra tercihi değil zorunluk:
    /// renkler `bt-core`'un dock çağrısından dönüyor ve o çağrı hücreleri
    /// sink'e basarken doğuruyor onları. `Frame` bu yüzden yüzeyi hücrelerden
    /// bağımsız tutuyor — listeler doluyken `dock` hâlâ `None` olabilir ve o
    /// hâlde hiçbir şey çizilmez, yani "yarım açılmış dock" temsil edilemez.
    pub(crate) fn open_dock(&mut self, rows: u16, ground: LinearRgba, separator: LinearRgba) {
        self.dock = Some(DockSurface {
            rows,
            ground: ground.to_array(),
            separator: separator.to_array(),
        });
    }

    /// Bu karenin dock yüzeyi; `None` → dock yok, ikinci viewport kurulmaz.
    pub(crate) fn dock(&self) -> Option<DockSurface> {
        self.dock
    }

    /// Dock'un yüksekliği, piksel; ikinci viewport'un orijinini ve caret'in
    /// kaymasını veren tek sayı. Dock yoksa sıfır.
    pub(crate) fn dock_px(&self) -> f32 {
        self.dock.map_or(0.0, |dock| {
            dock_height(dock.rows, self.cell_px.1, self.gutter_px)
        })
    }

    /// Dock içeriğinin dock-yerel dikey kayması: **nefes payı**.
    ///
    /// Saç çizgisi viewport'un tepesinde (y = 0) kalıyor ve pay onun
    /// **altında** başlıyor; üstüne pay koymak çizgiyi ızgaranın içine
    /// sokardı.
    fn dock_pad(&self) -> f32 {
        self.gutter_px
    }

    /// Dock'un zemini ve ayracı, **verilen genişlikte**.
    ///
    /// Genişlik argüman, çünkü `Frame` dokunun boyunu bilmiyor ve bilmemeli:
    /// listeler hücre ızgarasından doğuyor, yüzey ise pencerenin **tamamını**
    /// kaplamak zorunda. Aritmetiğin burada durması dikdörtgenin
    /// `renderer.rs`'te elle kurulmasını önlüyor — orada kurulsaydı `Instance`
    /// düzeninin ikinci bir yazarı olurdu.
    ///
    /// Zemin **opak** ve tam genişlik: kayma boyunca ızgaranın taşan alt
    /// satırı dock'un üstüne düşüyor (`LinkDelegate::set_origin`'in yazdığı
    /// taşma) ve onu örten tek şey bu dikdörtgen.
    pub(crate) fn dock_ground(&self, width_px: f32) -> [Instance; 3] {
        let dock = self.dock.unwrap_or(DockSurface {
            rows: 0,
            ground: [0.0; 4],
            separator: [0.0; 4],
        });
        [
            Instance {
                // Zemin **paylar dahil** bütün yüzeyi kaplıyor: pay kadar
                // eksik bir dikdörtgen, kayma boyunca taşan ızgara satırını
                // tam da nefes payının olduğu yerde gösterirdi.
                pos: [0.0, 0.0],
                size: [
                    width_px,
                    dock_height(dock.rows, self.cell_px.1, self.gutter_px),
                ],
                rgba: dock.ground,
            },
            // Ayraç zeminin **üstünde** ve dock'un en üst pikselinde: ızgara
            // ile dock arasındaki sınır orası.
            Instance {
                pos: [0.0, 0.0],
                size: [width_px, SEPARATOR_PX],
                rgba: dock.separator,
            },
            // **İkinci ayraç: giriş satırı ile bağlam satırı arasında.** Aynı
            // renk ve aynı kalınlık, çünkü aynı şeyi söylüyor — "bunlar ayrı
            // iki yüzey". phase-9 araya boşluk koymuştu; boşluk ayrımı
            // *önerir*, çizgi **söyler** (kullanıcı istedi).
            //
            // Yeri boşluğun **ortası**, üst ya da alt kenarı değil: kenara
            // konsaydı bir satıra yapışır ve ona ait görünürdü. Ortada
            // durunca iki satır da ondan eşit uzaklıkta.
            //
            // `rows < 2` iken yüksekliği **sıfır**: ayrılacak iki satır yok.
            // Dizinin boyu sabit kalıyor ki çağıran kolu dallanmasın; sıfır
            // yükseklikli dikdörtgen hiç fragment üretmiyor.
            Instance {
                pos: [0.0, self.dock_row_divider_y(dock.rows)],
                size: [width_px, if dock.rows < 2 { 0.0 } else { SEPARATOR_PX }],
                rgba: dock.separator,
            },
        ]
    }

    /// İki dock satırını ayıran çizginin **üst** kenarı, dock-yerel piksel.
    ///
    /// Satırların yerleşimi [`Frame::dock_pos`]'ta: `r`. satır
    /// `pad + r·(cell_h + gap)` yüksekliğinde başlıyor, yani boşluk
    /// `pad + cell_h` ile `pad + cell_h + gap` arasında. Çizgi o aralığın
    /// ortasına oturuyor.
    fn dock_row_divider_y(&self, rows: u16) -> f32 {
        if rows < 2 {
            return 0.0;
        }
        let pad = self.dock_pad();
        let gap = dock_row_gap(pad);
        // Yuvarlanıyor: aygıt ızgarasına oturmayan bir saç çizgisi iki piksele
        // yayılıp soluklaşırdı — `SEPARATOR_PX`'in ölçekle çarpılmama
        // gerekçesiyle aynı yerden.
        (pad + self.cell_px.1 + (gap - SEPARATOR_PX) * 0.5).round()
    }

    pub(crate) fn dock_bg(&self) -> &[Instance] {
        &self.dock_bg
    }

    pub(crate) fn dock_glyphs(&self) -> &[GlyphCell] {
        &self.dock_glyphs
    }

    pub(crate) fn dock_rules(&self) -> &[RuleCell] {
        &self.dock_rules
    }

    /// Caret'in ızgara yuvası; `None` → caret bu karede ızgarada değil.
    pub(crate) fn grid_caret(&self) -> Option<Instance> {
        self.grid_caret.map(|caret| {
            let mut instance = caret.instance(self.cell_px, self.caret_shape, self.rule_px);
            // **Ötelemeyi geri veriyor** ve bu bir asimetri değil: bu yuva
            // ızgaranın viewport'undan geçiyor, o da `+ origin_px` uyguluyor.
            // Dikdörtgen ise fragment'in `[[position]]`'ı ile karşılaştırılıyor
            // ve o koordinat dönüşümden **sonraki**, yani pencere uzayı — orada
            // çıkarma yok. Dock yuvasının ikizi aynı işi `origin_y` ile yapıyor
            // ([`Frame::dock_caret`]).
            //
            // Çıkarma unutulursa caret öteleme kadar aşağıda çizilir; ekleme
            // unutulursa (dikdörtgene öteleme konursa) altındaki harfin rengi
            // başka bir satıra düşer — zemin renginde bir harf, yani görünmez
            // bir hücre. Okuma anında yapılıyor, çünkü `origin_px` `push_caret`
            // ile encode arasında hâlâ değişebilir (`set_origin_rows` sink'ten
            // sonra çağrılıyor).
            instance.pos[1] -= self.origin_px;
            instance
        })
    }

    /// Caret'in dock yuvası, **dock-yerel** koordinatta; `None` → caret bu
    /// karede dock bandında değil.
    ///
    /// Instance pencere uzayında doğuyor ([`Frame::push_caret`]) ve dock
    /// viewport'u `origin_y` kadar aşağıdan başlıyor: farkı burada geri
    /// veriyoruz. Çeviriyi `Frame`'in yapması `dock_ground`'un genişliği
    /// argüman almasıyla aynı disiplin — `Instance` düzeninin ikinci bir
    /// yazarı doğmasın.
    pub(crate) fn dock_caret(&self, origin_y: f32) -> Option<Instance> {
        self.dock_caret.map(|caret| {
            let mut instance = caret.instance(self.cell_px, self.caret_shape, self.rule_px);
            instance.pos[1] -= origin_y;
            instance
        })
    }

    /// Dock bandının tepesini bu kare için yazar; caret'in yuvasını o belirliyor.
    pub(crate) fn set_dock_top(&mut self, top_px: f32) {
        self.dock_top_px = top_px;
    }

    pub(crate) fn bg_count(&self) -> usize {
        self.bg_count
    }

    /// Bu karenin imleç uniform'u; görünmez imleçte dejenere dikdörtgen.
    pub(crate) fn cursor_block(&self) -> &CursorBlock {
        &self.cursor
    }

    /// Bu karede çizilecek glyph sayısı; `make duman`'ın `glif=G` jetonu.
    /// `bg_count` gibi bir **CPU** sayacı: atlasın o glyph'leri gerçekten
    /// rasterize ettiğini kanıtlamaz, onu offscreen sınaması yapar.
    pub(crate) fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    /// Bu karede çizilecek kural çizgisi sayısı; `make duman`'ın `kural=R`
    /// jetonu.
    ///
    /// **Jetonun sınırının tek sahibi burası** — okuyan üç yer (`app.rs`'in
    /// duman kapısı, `Renderer::last_rule_count`, offscreen sınamalar) buraya
    /// işaret ediyor; dört kopya olsaydı bekçi yeniden adlandırıldığında üçü
    /// sessizce bayatlardı. Sınır iki katlı: (1) `bg_count` ve `glyph_count`
    /// gibi bir **CPU** sayacı, GPU'nun o çizgileri boyadığını kanıtlamaz;
    /// (2) **stil ayrımını göremez** — beş çeşidi de düz çizgi olarak çizen
    /// bir kod da aynı R'yi basar. Birincisini `rule_band_is_not_uniform_along_x`
    /// ve `sgr58_color_differs_from_foreground` kapatıyor, ikincisini o kıvrım
    /// sınaması ile `bt-core`'un `smoke_shell_distinguishes_five_styles`'ı.
    pub(crate) fn rule_count(&self) -> usize {
        self.rules.len()
    }

    /// Bu karenin hücre piksel boyutu; glyph dörtlüsünün boyu.
    ///
    /// Instance başına taşınmıyor (bkz. [`GlyphInstance`]), uniform olarak
    /// gidiyor — kare boyunca tek değer. Dönüş `[f32; 2]`: tek tüketicisi
    /// onu shader'a öyle geçiriyor.
    pub(crate) fn cell_px(&self) -> [f32; 2] {
        [self.cell_px.0, self.cell_px.1]
    }

    pub(crate) fn bg_instances(&self) -> &[Instance] {
        &self.bg
    }

    /// Bu karenin komut bloğu şeritleri.
    ///
    /// Sayacı **yok** ve bilerek: üç kardeşi (`bg_count`, `glyph_count`,
    /// `rule_count`) `make duman`'ın jetonları ve jeton satırı bir makine
    /// sözleşmesi. Şerit oraya girseydi ya yeni bir jeton açardı — duman
    /// reçetesi OSC 133 basmadığı için değeri hep sıfır olurdu, yani hiçbir
    /// şey söylemeyen bir kapı — ya da var olan bir jetonun anlamını
    /// kaydırırdı. Şeridin kanıtı sayaç değil, `renderer.rs`'in offscreen
    /// piksel okuması.
    pub(crate) fn stripes(&self) -> &[RuleCell] {
        &self.stripes
    }

    pub(crate) fn glyphs(&self) -> &[GlyphCell] {
        &self.glyphs
    }

    pub(crate) fn rules(&self) -> &[RuleCell] {
        &self.rules
    }

    /// Grid koordinatının sol üst köşesi, piksel — **üç listenin ortak
    /// aritmetiği**.
    ///
    /// [`Frame::push`] onu hücre başına bir kez çağırıyor, yani bekçi artık
    /// push edilen **her** hücrede koşuyor: `clear` çağrılmadan push edilen
    /// hücre sıfır boyutlu doğar ve ekranda sessizce kaybolur. Tek çağrı
    /// olduğu için formülün dallara kopyalanma ihtimali de kalmadı; imleç
    /// yolu ([`Frame::push_caret`]) aynı fonksiyondan geçen ikinci çağıran.
    fn pos(&self, col: u16, row: u16) -> [f32; 2] {
        self.pos_at([f32::from(col), f32::from(row)])
    }

    /// [`Frame::pos`]'un dock hâli: aynı sütun aritmetiği, **artı nefes payı**.
    ///
    /// Payın eklendiği **tek** yer burası ve gerekçesi sol payınkiyle aynı
    /// ([`Frame::pos_at`]): arka plan, glyph, kural ve caret dördü de bu
    /// satırdan geçiyor, ikinci bir yerde eklenseydi pay iki kez uygulanırdı.
    fn dock_pos(&self, col: u16, row: u16) -> [f32; 2] {
        let [x, y] = self.pos(col, row);
        let pad = self.dock_pad();
        [x, y + pad + f32::from(row) * dock_row_gap(pad)]
    }

    /// [`Frame::pos`]'un kesirli hâli — imleç iki hücre arasındayken.
    ///
    /// Formülün **tek** kopyası burası; tam sayı yolu buradan geçiyor ki
    /// kayan imleç ile duran hücre aynı aritmetiği paylaşsın. Ayrışsalardı
    /// yerleşmiş imleç altındaki harften yarım piksel kayabilirdi.
    ///
    /// Sol pay da **yalnız burada** ekleniyor: 0. sütun payın bittiği yerde
    /// başlıyor ve arka plan, glyph, kural, imleç dördü de bu satırdan
    /// geçiyor. İkinci bir yerde eklenseydi pay iki kez uygulanırdı.
    fn pos_at(&self, at: [f32; 2]) -> [f32; 2] {
        let (w, h) = self.cell_px;
        debug_assert!(w > 0.0 && h > 0.0, "clear(metrics) çağrılmadı");
        [self.gutter_px + at[0] * w, at[1] * h]
    }
}

#[cfg(test)]
mod tests {
    use bt_core::{CaretShape, Cursor};

    use super::*;

    // Uçlar bilerek: `0.0`/`1.0` sRGB transfer fonksiyonunun sabit noktaları,
    // yani bu sınamalar renk uzayından bağımsız. Uzayı sınayan yer
    // `renderer.rs` → `cell_bg_paints_pixels_on_the_gpu`.
    //
    // Gömülü temanın iki ayrık rengi; adları rolleri değil kaynakları
    // söylüyor. Burada bakılan şey renk değil düzen, o yüzden renk uydurmaya
    // (`LinearRgba::from_srgb`) gerek yok — `renderer.rs`'in offscreen
    // sınamaları onu üç ayrık ton gerektirdikleri için kullanıyor.
    const BG: LinearRgba = bt_core::Theme::BATERI.background_linear();
    const CURSOR: LinearRgba = bt_core::Theme::BATERI.accent_linear();
    /// Blok altındaki metnin rengi; üretimde temanın zemini (`bt-core` →
    /// `Cursor::text`). Burada bloğun renginden **ayrık** olması yetiyor.
    const TEXT: LinearRgba = BG;
    /// Yerleşmiş imlecin opaklığı — belirme dışında her kare bu
    /// (`crate::motion::Motion::alpha`). Belirmeyi sınayan tek yer
    /// `cursor_alpha_reaches_the_block_and_the_text`.
    const OPAQUE: f32 = 1.0;

    /// Sol payı **sıfır** olan ızgara ölçüsü: bu modüldeki sınamaların çoğu
    /// listelerin düzenini soruyor, orijini değil, ve sıfır pay onların
    /// beklenen piksellerini hücre aritmetiğinde tutuyor. Payın kendi
    /// sınamaları [`GUTTER`]'ı kullanıyor ve adıyla anıyor.
    fn grid(width: u16, height: u16) -> CellMetrics {
        CellMetrics::new(width, height, 0, 1).expect("sıfır olmayan hücre")
    }

    /// Payı sorgulayan sınamaların ölçüsü. Değer üretimdekiyle aynı olmak
    /// zorunda değil — sorulan şey "pay orijine ekleniyor mu", genişliği
    /// değil — ve hücre genişliğinden **ayrık** seçildi ki iki çarpanın
    /// yanlışlıkla örtüşmesi sınamayı sessizce geçirmesin.
    const GUTTER: u16 = 7;

    /// Eski `Frame::push_cursor` imzasının sınama kabuğu: tek caret API'sine
    /// çeviriyor. Sınamaların çoğu `Cursor` ile konuşuyor ve o kayıt hâlâ
    /// `bt-core`'un sınır tipi; değişen yalnız `Frame`'in iç yuvası.
    fn push_cursor(frame: &mut Frame, cursor: Cursor, at: [f32; 2], rgba: LinearRgba, alpha: f32) {
        if cursor.visible {
            frame.push_caret(at, cursor.text, rgba, alpha, cursor.shape);
        }
    }

    /// `Frame::move_cursor`'ın sınama kabuğu; görünmez imleç caret'i siliyor.
    fn move_cursor(frame: &mut Frame, cursor: Cursor, at: [f32; 2], rgba: LinearRgba, alpha: f32) {
        frame.move_caret(at, cursor.text, rgba, alpha);
        if !cursor.visible {
            frame.clear_caret();
        }
    }

    fn cursor(col: u16, row: u16, visible: bool) -> Cursor {
        Cursor {
            next_tick: None,
            col,
            row,
            visible,
            // Bu modül ızgaranın listelerini sınıyor; devir `link`'in sorusu.
            caret_in_dock: false,
            shape: CaretShape::Block,
            text: TEXT,
            // Kaydırma kararı hareketin işi (`motion.rs`); bu listeyi
            // ilgilendirmiyor, çünkü konum zaten dışarıdan geliyor.
            display_offset: 0,
            // Doluluk sayısı da bu listeyi ilgilendirmiyor: ötelemeyi
            // `set_origin_rows` söylüyor ve bu iki alan onun **girdisi**,
            // yani `link.rs`'in okuduğu yer. Dolu ızgara, yani öteleme sıfır.
            content_rows: 1,
            rows: 1,
        }
    }

    /// İmleci **kendi** hücresine çizer: yerleşmiş (animasyonsuz) hâl.
    /// Ara konumu sınayan tek yer `cursor_slides_between_cells`.
    fn push_settled(frame: &mut Frame, cursor: Cursor) {
        push_cursor(
            frame,
            cursor,
            [f32::from(cursor.col), f32::from(cursor.row)],
            CURSOR,
            OPAQUE,
        );
    }

    fn bg_cell(col: u16, row: u16) -> Cell {
        Cell {
            col,
            row,
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        }
    }

    #[test]
    fn frame_bg_count_excludes_cursor() {
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));

        frame.push(bg_cell(0, 0));
        frame.push(bg_cell(1, 0));
        push_settled(&mut frame, cursor(5, 2, true));

        // Caret artık arka plan listesine **hiç girmiyor**: kendi yuvası var
        // ve encode onu ayrı çiziyor. `hucre=K` ile listenin boyu bu yüzden
        // artık aynı sayı — sayacın imleci dışlaması eskiden bir çıkarmaydı,
        // şimdi yapısal.
        assert_eq!(frame.bg_instances().len(), 2);
        assert_eq!(frame.bg_count(), 2);
        assert!(frame.grid_caret().is_some(), "caret çizilmiyor");

        frame.clear(grid(9, 18));
        assert_eq!(frame.bg_count(), 0);
        assert!(frame.bg_instances().is_empty());
    }

    #[test]
    fn invisible_cursor_is_not_drawn() {
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        push_settled(&mut frame, cursor(0, 0, false));
        assert!(frame.bg_instances().is_empty());
        // Uniform da dokunulmadan kalır: dejenere dikdörtgen "blok yok"
        // demenin tek yolu, shader'da ikinci bir bayrak yok.
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn cursor_block_covers_its_cell_and_clears_with_the_frame() {
        // Dikdörtgen bloğun **kendi** instance'ıyla aynı hücreye oturmalı:
        // ayrışsalardı blok bir yerde, altındaki metnin rengi başka bir yerde
        // olurdu ve ikisi de sessizce yanlış çizerdi.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        push_settled(&mut frame, cursor(3, 2, true));

        let block = *frame.cursor_block();
        assert_eq!(block.rect, [27.0, 36.0, 36.0, 54.0]);
        assert_eq!(
            block.rgba,
            TEXT.to_array(),
            "metin rengi `Cursor`'dan gelir"
        );
        let instance = frame.grid_caret().expect("blok instance'ı");
        assert_eq!(
            [
                instance.pos[0],
                instance.pos[1],
                instance.pos[0] + instance.size[0],
                instance.pos[1] + instance.size[1],
            ],
            block.rect,
            "dikdörtgen bloğun instance'ıyla ayrıştı"
        );

        // `clear` uniform'u da sıfırlar: sönen imleç (`\e[?25l`) bloğu
        // kaldırır ama rengi eski hücrede bırakırsa orası görünmez olur.
        frame.clear(grid(9, 18));
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn cursor_alpha_reaches_the_block_and_the_text() {
        // Hareketi Azalt'ın belirmesi **iki** yere birden yazılmak zorunda:
        // bloğun instance'ına ve `cell` pipeline'ının uniform'una. Yalnız
        // bloğa yazılsaydı harf, henüz görünmeyen bir bloğun rengine
        // boyanırdı — zeminin üstünde zemin renginde bir harf, yani okunmayan
        // bir hücre; belirti de tam olarak o hücreyle sınırlı, hiçbir sayaç
        // görmez.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        push_cursor(&mut frame, cursor(3, 2, true), [3.0, 2.0], CURSOR, 0.25);

        let instance = frame.grid_caret().expect("blok instance'ı");
        assert_eq!(instance.rgba[3], 0.25, "blok opaklığı taşınmadı");
        assert_eq!(
            frame.cursor_block().rgba[3],
            0.25,
            "metin opaklığı bloğunkinden ayrıştı"
        );
        // Renk bileşenleri opaklıktan **etkilenmiyor**: harmanlamayı GPU
        // yapıyor, burada ön çarpım yok (`Renderer::pipeline`).
        assert_eq!(instance.rgba[..3], CURSOR.to_array()[..3]);
        assert_eq!(frame.cursor_block().rgba[..3], TEXT.to_array()[..3]);

        // Yerleşmiş imleç opak ve o hâlde iki dizi de temanın kendisi.
        frame.clear(grid(9, 18));
        push_settled(&mut frame, cursor(3, 2, true));
        assert_eq!(frame.grid_caret().expect("blok").rgba, CURSOR.to_array());
        assert_eq!(frame.cursor_block().rgba, TEXT.to_array());
    }

    #[test]
    fn cursor_slides_between_cells() {
        // Ara konum: blok iki hücre arasındayken dikdörtgen de kesirli
        // piksele oturmalı. Tam sayıya yuvarlansaydı kayma hücre hücre
        // zıplar ve animasyonun tamamı görünmez olurdu.
        let mut frame = Frame::default();
        frame.clear(grid(10, 20));
        push_cursor(&mut frame, cursor(3, 2, true), [2.5, 1.25], CURSOR, OPAQUE);
        assert_eq!(frame.cursor_block().rect, [25.0, 25.0, 35.0, 45.0]);
        assert_eq!(frame.grid_caret().expect("caret").pos, [25.0, 25.0]);
    }

    #[test]
    fn a_motion_frame_keeps_the_lists_and_moves_only_the_cursor() {
        // Hareket karesinin sözleşmesi: grid kirli değil, yani glyph ve kural
        // listeleri geçerli kalmalı; kırpılan tek şey önceki karenin imleci.
        // Kırpma olmasaydı her hareket karesi listeye bir dikdörtgen daha
        // eklerdi — 200 ms'lik bir kaymada yirmi dört hayalet imleç.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16));
        frame.push(bg_cell(0, 0));
        frame.push(Cell {
            col: 1,
            row: 0,
            ch: Some('b'),
            fg: CURSOR,
            bg: Some(BG),
            underline: UnderlineStyle::Single,
            ..Default::default()
        });
        push_settled(&mut frame, cursor(0, 0, true));
        let (cells, glyphs, rules) = (frame.bg_count(), frame.glyph_count(), frame.rule_count());
        assert_eq!((cells, glyphs, rules), (2, 1, 1));

        for _ in 0..3 {
            move_cursor(&mut frame, cursor(5, 0, true), [4.5, 0.0], CURSOR, OPAQUE);
            // Üç sayacın üçü de oynamadı: `hucre=8 glif=6 kural=15` duman
            // koşusunda hareket karesiyle bitse bile aynı kalmalı.
            assert_eq!(frame.bg_count(), cells);
            assert_eq!(frame.glyph_count(), glyphs);
            assert_eq!(frame.rule_count(), rules);
            // Liste imleçten **bağımsız**: caret kendi yuvasında ve hareket
            // karesi onu oraya yazıyor, arka planlara dokunmadan.
            assert_eq!(frame.bg_instances().len(), cells);
            assert!(frame.grid_caret().is_some(), "hareket karesi caret'i sildi");
        }
        assert_eq!(frame.cursor_block().rect, [36.0, 0.0, 44.0, 16.0]);

        // Görünmez imleçle gelen hareket karesi bloğu **kaldırır**: uniform
        // eski yerinde kalsaydı orada zemin renginde bir harf dururdu.
        move_cursor(&mut frame, cursor(5, 0, false), [4.5, 0.0], CURSOR, OPAQUE);
        assert_eq!(frame.bg_instances().len(), cells);
        assert!(frame.grid_caret().is_none(), "görünmez imleç blok bıraktı");
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn clear_updates_cell_size() {
        // Izgara ölçüsünün `clear`'ın parametresi olmasının tek sebebi bu:
        // alan olsaydı ekran ölçeği değişince bayatlardı ve hiçbir sınama
        // görmezdi. Pay da aynı çağrıdan geliyor, yani aynı bekçinin altında.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        frame.push(bg_cell(1, 1));
        assert_eq!(frame.bg_instances()[0].pos, [9.0, 18.0]);

        frame.clear(grid(18, 36));
        frame.push(bg_cell(1, 1));
        assert_eq!(frame.bg_instances()[0].pos, [18.0, 36.0]);
        assert_eq!(frame.bg_instances()[0].size, [18.0, 36.0]);
    }

    #[test]
    fn grid_coords_convert_to_pixels() {
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        frame.push(bg_cell(3, 2));
        assert_eq!(
            frame.bg_instances()[0],
            Instance {
                pos: [27.0, 36.0],
                size: [9.0, 18.0],
                rgba: BG.to_array(),
            }
        );
    }

    #[test]
    fn the_gutter_offsets_every_pixel_position() {
        // Sol pay (010 Karar 3) çizim orijinine `pos_at`'te **bir kez**
        // ekleniyor; dört tüketicinin (arka plan, glyph, kural, imleç) hepsi
        // o satırdan geçtiği için dördü de aynı kadar kayıyor. İki yerde
        // eklenseydi biri payı iki kez uygular ve belirti "glyph arka
        // planından kaymış" olurdu.
        let mut frame = Frame::default();
        frame.clear(CellMetrics::new(9, 18, GUTTER, 1).expect("ölçü"));
        frame.push(Cell {
            col: 3,
            row: 2,
            ch: Some('x'),
            fg: CURSOR,
            bg: Some(BG),
            underline: UnderlineStyle::Single,
            ..Default::default()
        });
        push_settled(&mut frame, cursor(1, 0, true));

        let shifted = [f32::from(GUTTER) + 27.0, 36.0];
        assert_eq!(frame.bg_instances()[0].pos, shifted, "arka plan");
        assert_eq!(frame.glyphs()[0].pos, shifted, "glyph");
        assert_eq!(frame.rules()[0].pos, shifted, "kural");
        // İmleç de aynı satırdan geçiyor: 1. sütun payın 9 piksel sağında.
        assert_eq!(
            frame.cursor_block().rect[0],
            f32::from(GUTTER) + 9.0,
            "imleç"
        );

        // **Boyut kaymıyor, yalnız konum**: pay ızgarayı iteliyor, hücreyi
        // büyütmüyor.
        assert_eq!(frame.bg_instances()[0].size, [9.0, 18.0]);
    }

    #[test]
    fn the_cursor_rect_keeps_the_screen_row_and_the_instance_gives_the_origin_back() {
        // **Karar 7'nin bekçisiz kalan üçüncü belirtisi.** İmlecin
        // dikdörtgeni ile onun `bg` instance'ı iki ayrı uzayda yaşıyor:
        // instance vertex aşamasından, yani `setViewport`'tan geçiyor ve
        // ötelemeyi GPU'da **geri alıyor**; dikdörtgen fragment'in
        // `[[position]]`'ı ile karşılaştırılıyor ve o koordinat dönüşümden
        // **sonraki**, yani zaten ekran koordinatı. İkisi ayrışırsa imleç
        // doğru yerde görünür ama altındaki metnin rengi başka bir satıra
        // düşer — `make hepsi`'yi yeşil bırakan, gözle "bir hücre görünmez
        // oldu" diye fark edilen bir kusur.
        //
        // İki öteleme sınanıyor ve sıfır olmayanı asıl olan: sıfırda iki taraf
        // eşit ve `origin_px`'i tümden silen bir kod da geçer.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        frame.push(bg_cell(0, 2));
        push_settled(&mut frame, cursor(0, 2, true));
        let cell_y = frame.bg_instances()[0].pos[1];
        assert_eq!(cell_y, 36.0);
        assert_eq!(
            frame.cursor_block().rect[1],
            cell_y,
            "ötelemesiz karede dikdörtgen hücreyle aynı satırda"
        );

        // Aynı kare, iki satır ötelenmiş. İmlecin hedefi **ekran** satırı, yani
        // grid satırı 2 + öteleme 2 = 4; ekranda göründüğü yer `cell_y + 36`,
        // çünkü içerik de o kadar aşağı kaydı. İmleci yeniden basmak şart:
        // dikdörtgen `push_cursor` anındaki ötelemeyi pişiriyor ve üretimde de
        // sıra öyle (`link.rs` orijini imleçten **önce** yazıyor).
        frame.set_origin_rows(2.0);
        move_cursor(&mut frame, cursor(0, 2, true), [0.0, 4.0], CURSOR, OPAQUE);
        assert_eq!(
            frame.grid_caret().expect("caret").pos[1],
            cell_y,
            "instance ötelemeyi geri vermedi: viewport onu bir kez daha ekler"
        );
        assert_eq!(
            frame.cursor_block().rect[1],
            cell_y + 36.0,
            "dikdörtgen ekran satırında değil: blok altındaki metin başka satırda kalır"
        );
        assert_eq!(
            frame.cursor_block().rect[3],
            cell_y + 36.0 + 18.0,
            "dikdörtgenin altı da aynı kadar kaymalı: yoksa boyu değişir"
        );
    }

    #[test]
    fn a_sliding_origin_lands_on_whole_device_pixels() {
        // **Kaymanın durduğu yer ekranda kalıcı:** link'in "hasar yok" dalı
        // animasyonun *yerleştiği* kareyi hiç çizmeden uyuyor, yani ekranda
        // kalan son kare yerleşmeden bir adım öncesi. Kesirli bir piksel orada
        // donsaydı bütün metin yarım piksele kadar kaymış, yani her Enter'dan
        // sonra bulanık olurdu — hiçbir sayaç görmez, göz görür.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        frame.set_origin_rows(1.51);
        assert_eq!(frame.origin_px(), 27.0, "öteleme piksele yuvarlanmadı");

        // Yerleşmeye bir adım kala (~0,009 satır ≈ 0,16 piksel) ötelemenin
        // pikseli **tam** hedefte: kayma bitmeden de ekran keskin.
        frame.set_origin_rows(2.0 - 0.009);
        assert_eq!(frame.origin_px(), 36.0, "yerleşmeden önceki kare kaymış");

        // Tam satırda yuvarlama kimliktir: phase-1'in üç bekçisi ve üretimdeki
        // yerleşmiş hâl buradan geçiyor.
        frame.set_origin_rows(3.0);
        assert_eq!(frame.origin_px(), 54.0);
    }

    #[test]
    fn a_zero_gutter_leaves_the_origin_at_the_edge() {
        // Payın sıfırı meşru bir cevap (entegrasyonsuz bir gelecekte ya da
        // dejenere ölçekte): ızgara kenardan başlar ve aritmetik payın
        // eklenmediği hâline birebir döner. Bu modüldeki öteki sınamaların
        // `grid()` üzerinden dayandığı sözleşme de bu.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        frame.push(bg_cell(3, 2));
        assert_eq!(frame.bg_instances()[0].pos, [27.0, 36.0]);
    }

    /// Şeridin rengi: temanın durum rolü, `frame()` sınırının çözüp verdiği
    /// değerle aynı kaynak (`bt-core` → `Block::stripe`).
    const SUCCESS: LinearRgba = bt_core::Theme::BATERI.success_linear();

    fn block(row: u16) -> Block {
        Block {
            row,
            stripe: SUCCESS,
        }
    }

    #[test]
    fn a_mark_covers_one_row_and_lines_up_with_the_dock_sigil() {
        // İşaretin üç iddiası da sessizce bozulabilir: (1) **dock'un
        // chevron'uyla aynı sprite** — ikisi de safha renginde prompt işareti
        // ve ayrı şekillerle çizilmeleri bir kalıntıydı; (2) kendi satırında
        // başlar — komutun satırını gösteriyor, bir aralığı değil; (3) **0.
        // sütunda**, yani dock'un prompt işaretiyle aynı x'te (012 phase-11).
        let mut frame = Frame::default();
        frame.clear(CellMetrics::new(9, 18, GUTTER, 1).expect("ölçü"));
        frame.push_block(block(2));

        let mark = frame.stripes()[0];
        assert_eq!(mark.kind, RuleKind::Chevron, "işaret hâlâ dikdörtgen");
        assert_eq!(mark.pos[1], 36.0, "işaret kendi satırında başlamalı");
        assert_eq!(mark.rgba, SUCCESS.to_array(), "renk sınırdan gelir");
        // **Hiza hesaplanmıyor, tek formülden doğuyor.** Dock'un işareti de
        // 0. sütunda ve o da `Frame::pos`'tan geçiyor; ikisi ayrı aritmetikle
        // yerleştirildiği sürece yarım pay kadar ayrı duruyorlardı.
        frame.clear(CellMetrics::new(9, 18, GUTTER, 1).expect("ölçü"));
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('>'),
            ..Cell::default()
        });
        frame.push_block(block(0));
        assert_eq!(
            frame.stripes()[0].pos[0],
            frame.dock_glyphs()[0].pos[0],
            "ızgaranın işareti dock'unkiyle aynı sütunda değil"
        );
        // Pay değişince de aynı: ikisi de aynı paydan geçiyor.
        frame.clear(CellMetrics::new(4, 18, 12, 1).expect("ölçü"));
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('>'),
            ..Cell::default()
        });
        frame.push_block(block(0));
        assert_eq!(frame.stripes()[0].pos[0], 12.0, "işaret paydan geçmedi");
        assert_eq!(frame.stripes()[0].pos[0], frame.dock_glyphs()[0].pos[0]);

        frame.clear(CellMetrics::new(9, 18, GUTTER, 1).expect("ölçü"));
        assert!(
            frame.stripes().is_empty(),
            "`clear` işaretleri de boşaltmalı"
        );
    }

    #[test]
    fn stripes_stay_out_of_the_cell_count_and_survive_motion_frames() {
        // Phase'in asıl sözleşmesi (010 → R4.1): şerit `bg`'ye **girmiyor**. Girip
        // sayılmasaydı `move_cursor`'ın kırpması onu her hareket karesinde
        // siler ve şerit imleç kaydıkça titrerdi; sayılsaydı `hucre=` jetonu
        // hücre olmayan bir şeyi de sayar ve duman kapısının anlamı kayardı.
        let mut frame = Frame::default();
        frame.clear(CellMetrics::new(8, 16, GUTTER, 1).expect("ölçü"));
        frame.push(bg_cell(0, 0));
        frame.push_block(block(0));
        push_settled(&mut frame, cursor(0, 0, true));

        assert_eq!(frame.bg_count(), 1, "şerit hücre sayılmamalı");
        assert_eq!(frame.bg_instances().len(), 1, "caret arka plana sızdı");
        assert_eq!(frame.stripes().len(), 1);

        let stripes = frame.stripes().to_vec();
        for _ in 0..3 {
            move_cursor(&mut frame, cursor(5, 0, true), [4.5, 0.0], CURSOR, OPAQUE);
            // Izgara değişmedi, yani blokların satır aralığı da değişmedi:
            // şerit hareket karesinde olduğu gibi kalmalı.
            assert_eq!(frame.stripes(), stripes, "hareket karesi şeridi oynattı");
            assert_eq!(frame.bg_count(), 1);
        }
    }

    /// Dock'un bir hücresi; ızgaranın [`bg_cell`]'inin dock ikizi.
    fn dock_cell(col: u16) -> Cell {
        Cell {
            col,
            row: 0,
            ch: Some('x'),
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        }
    }

    #[test]
    fn the_dock_keeps_its_own_lists_and_stays_out_of_the_counters() {
        // Phase'in birinci sözleşmesi: dock listeleri `bg`'ye **girmiyor**.
        // Girselerdi `move_cursor`'ın `truncate(bg_count)`'u onları her hareket
        // karesinde siler ve dock, imleç kaydıkça **titrerdi** — şeridin ayrı
        // liste olma gerekçesinin aynısı, bir derece daha görünür belirtiyle.
        // Sayaçlara girmemesi ikinci sözleşme: `hucre=8 glif=6 kural=15` duman
        // koşusunda ölçülüyor ve anlamı bit bit korunmalı.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16));
        frame.push(bg_cell(0, 0));
        push_settled(&mut frame, cursor(0, 0, true));
        frame.push_dock(dock_cell(0));
        frame.push_dock(Cell {
            underline: UnderlineStyle::Single,
            ..dock_cell(1)
        });
        frame.open_dock(2, BG, CURSOR);

        assert_eq!(frame.bg_count(), 1, "dock hücre sayıldı");
        assert_eq!(frame.glyph_count(), 0, "dock glyph sayıldı");
        assert_eq!(frame.rule_count(), 0, "dock kural sayıldı");
        // Caret artık `bg`'de **değil**: kendi yuvası var ve encode onu ayrı
        // çiziyor. Arka plan listesinde yalnız hücrenin kendisi kalıyor.
        assert_eq!(frame.bg_instances().len(), 1, "caret arka plana sızdı");
        assert!(frame.grid_caret().is_some(), "caret ızgara yuvasında değil");

        let (dock_bg, dock_glyphs, dock_rules) = (
            frame.dock_bg().to_vec(),
            frame.dock_glyphs().to_vec(),
            frame.dock_rules().to_vec(),
        );
        assert_eq!(dock_bg.len(), 2, "caret dock arka planına sızdı");
        assert_eq!(dock_glyphs.len(), 2);
        assert_eq!(dock_rules.len(), 1);

        for _ in 0..3 {
            move_cursor(&mut frame, cursor(5, 0, true), [4.5, 0.0], CURSOR, OPAQUE);
            assert_eq!(frame.dock_bg(), dock_bg, "hareket karesi dock'u oynattı");
            assert_eq!(frame.dock_glyphs(), dock_glyphs);
            assert_eq!(frame.dock_rules(), dock_rules);
            assert!(frame.dock().is_some(), "hareket karesi yüzeyi kapattı");
        }

        // `clear` **hepsini** boşaltıyor: korunan bir yüzey, dock'u olmayan
        // bir oturumun penceresinde asılı kalırdı.
        frame.clear(grid(8, 16));
        assert!(frame.dock().is_none());
        assert!(frame.dock_bg().is_empty());
        assert!(frame.dock_glyphs().is_empty());
        assert!(frame.dock_rules().is_empty());
        assert!(frame.grid_caret().is_none(), "clear caret'i bırakmadı");
        assert!(frame.dock_caret(0.0).is_none());
    }

    #[test]
    fn the_dock_never_reads_the_origin() {
        // Muafiyet **yapısal**: dock listeleri dock-yerel doğuyor ve ötelemeyi
        // hiç görmüyor; ekrana taşıyan şey ikinci `setViewport`. Aritmetikle
        // kurulamazdı — `clear` ötelemeyi sıfırlıyor ve `set_origin_rows`
        // sink'ten sonra çağrılıyor, yani dock hücreleri basılırken değer
        // henüz bilinmiyor. Bu sınama o bağımsızlığı CPU tarafında çiviliyor;
        // pikselin tanığı `renderer.rs`'te.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16));
        frame.push_dock(dock_cell(1));
        let settled_bg = frame.dock_bg()[0];

        // Aynı kare, iki satır ötelenmiş: ızgaranın hücresi kayar, dock'unki
        // kaymaz.
        frame.clear(grid(8, 16));
        frame.set_origin_rows(2.0);
        frame.push_dock(dock_cell(1));
        assert_eq!(frame.dock_bg()[0], settled_bg, "dock ötelemeyi yedi");
        // Izgaranın imleci **aynı karede** ötelemeyi görüyor: ikisinin ayrı
        // uzaylarda olduğu iddiası ancak ikisi birden sorulunca kanıtlanır.
        push_cursor(&mut frame, cursor(0, 0, true), [0.0, 2.0], CURSOR, OPAQUE);
        assert_eq!(
            frame.cursor_block().rect[1],
            2.0 * 16.0,
            "ızgaranın imleci ekran satırında değil"
        );
    }

    #[test]
    fn the_dock_ground_spans_the_given_width() {
        // Genişlik argüman, çünkü `Frame` dokunun boyunu bilmiyor: listeler
        // hücre ızgarasından doğuyor, yüzey ise pencerenin **tamamını**
        // kaplamak zorunda. Zemin opak olmalı — kayma boyunca ızgaranın taşan
        // alt satırı onun altında kalıyor.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18));
        frame.open_dock(2, BG, CURSOR);
        assert_eq!(frame.dock_px(), 36.0, "iki satır piksele çevrilmedi");

        let [ground, separator, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.pos, [0.0, 0.0], "zemin sol paydan başlamamalı");
        assert_eq!(ground.size, [500.0, 36.0]);
        assert_eq!(ground.rgba, BG.to_array());
        assert_eq!(ground.rgba[3], 1.0, "zemin saydam: taşan satır görünür");
        // Ayraç dock'un **en üst** pikselinde: ızgarayla sınır orası.
        assert_eq!(separator.pos, [0.0, 0.0]);
        assert_eq!(separator.size, [500.0, SEPARATOR_PX]);
        assert_eq!(separator.rgba, CURSOR.to_array());
        // İkinci ayraç iki satırın **arasında** ve aynı renkte. Paysız bu
        // ölçüde satır arası boşluk sıfır, yani çizgi tam satır sınırında.
        assert_eq!(divider.pos, [0.0, 18.0]);
        assert_eq!(divider.size, [500.0, SEPARATOR_PX]);
        assert_eq!(divider.rgba, CURSOR.to_array());

        // Dock'suz kare hiçbir yükseklik vermiyor: ikinci viewport kurulmaz.
        frame.clear(grid(9, 18));
        assert_eq!(frame.dock_px(), 0.0);
    }

    #[test]
    fn the_dock_breathes_above_and_below_its_rows() {
        // **Nefes payı** (012 phase-9, kullanıcı: "padding top yok resmen").
        // İki satırın üstünde ve altında pay var, kaynağı da sol payın ta
        // kendisi — ikinci bir tasarım sabiti uydurulmadı.
        let mut frame = Frame::default();
        frame.clear(CellMetrics::new(9, 18, GUTTER, 1).expect("ölçü"));
        frame.open_dock(2, BG, CURSOR);
        // 2×18 + 2×GUTTER + 1×(2×GUTTER) = 36 + 14 + 14 = 64. Satır arası
        // boşluk dış payın **iki katı**, çünkü ortasından bir çizgi geçiyor:
        // çizginin iki yanına birer pay düşünce dört boşluk da eşitleniyor.
        assert_eq!(frame.dock_px(), 64.0, "pay yüksekliğe girmedi");

        // **Zemin payları da kaplıyor**: pay kadar eksik bir dikdörtgen,
        // kayma boyunca taşan ızgara satırını tam da nefes payında gösterirdi.
        let [ground, separator, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, 64.0]);
        // Satır arası çizgi boşluğun **ortasında**: pay 7, hücre 18, boşluk 14
        // → 7 + 18 + (14 − 1)/2 = 31,5 → 32. Kenara konsaydı bir satıra
        // yapışır ve ona ait görünürdü.
        assert_eq!(divider.pos, [0.0, 32.0]);
        // **Ritim eşit — piksel piksel yazılı.** `pad` 7, hücre 18, boşluk 14,
        // çizgi 1 px. Kutular: üst çizgi [0,1], giriş [7,25], ara çizgi
        // [32,33], bağlam [39,57], dip 64. Aradaki dört boşluk sırayla
        // 6, 7, 6, 7 — fark çizgilerin **kendi** pikselinden ve 31,5'in
        // yuvarlanmasından geliyor, formülden değil. Bir piksel eşitlenemez:
        // boşluk çift (14), çizgi tek (1), yani merkez hep yarım piksele
        // düşüyor. Eski hâlde bu boşluklar 6, 1, 1, 7 idi.
        assert_eq!(GUTTER, 7, "üstteki piksel tablosu bu paya bağlı");
        let row1_top = f32::from(GUTTER) + 18.0 + dock_row_gap(f32::from(GUTTER));
        assert_eq!(row1_top, 39.0);
        assert_eq!(divider.pos[1] - (f32::from(GUTTER) + 18.0), 7.0);
        assert_eq!(row1_top - (divider.pos[1] + SEPARATOR_PX), 6.0);
        assert_eq!(frame.dock_px() - (row1_top + 18.0), 7.0);
        assert_eq!(divider.size, [500.0, SEPARATOR_PX]);
        // Saç çizgisi payın **üstünde**, viewport'un tepesinde: ızgarayla
        // sınır orası ve payı onun üstüne koymak çizgiyi ızgaraya sokardı.
        assert_eq!(separator.pos, [0.0, 0.0]);

        // İçerik payın altından başlıyor: ilk satır y = pay.
        frame.clear(CellMetrics::new(9, 18, GUTTER, 1).expect("ölçü"));
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('x'),
            ..Cell::default()
        });
        frame.open_dock(2, BG, CURSOR);
        assert_eq!(
            frame.dock_glyphs()[0].pos[1],
            f32::from(GUTTER),
            "içerik paya inmedi"
        );
        // İkinci satır bir hücre aşağıda, yani pay **bir kez** uygulanıyor.
        frame.clear(CellMetrics::new(9, 18, GUTTER, 1).expect("ölçü"));
        frame.push_dock(Cell {
            col: 0,
            row: 1,
            ch: Some('x'),
            ..Cell::default()
        });
        // İkinci satır bir hücre **artı satır arası boşluk** aşağıda; dış pay
        // ona bir kez daha uygulanmıyor.
        assert_eq!(
            frame.dock_glyphs()[0].pos[1],
            f32::from(GUTTER) + 18.0 + 2.0 * f32::from(GUTTER),
            "satır arası boşluk ya da dış pay yanlış uygulandı"
        );
    }

    #[test]
    fn the_caret_picks_its_slot_from_the_dock_band() {
        // **Tek caret, iki yuva.** Blok, üstünde duracağı yüzeyin zemininden
        // sonra ama glyph'lerinden önce çizilmek zorunda; ızgarada kalsaydı
        // dock'un opak zemini onu örter, dock'ta kalsaydı ızgaranın harfini
        // boyardı. Ölçüt **örtüşme**, merkez değil: devir karelerinde yarısı
        // kırpılmış bir blok görünmesin diye banda değen caret yukarı değil
        // aşağı yuvarlanıyor.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16));
        frame.set_dock_top(64.0);

        // Bandın tamamen üstünde: ızgaranın yuvası.
        frame.push_caret([3.0, 2.0], TEXT, CURSOR, OPAQUE, CaretShape::Block);
        assert!(
            frame.grid_caret().is_some(),
            "caret ızgara yuvasına düşmedi"
        );
        assert!(frame.dock_caret(64.0).is_none(), "caret iki yuvada birden");

        // Banda **değdiği** anda dock'un yuvası — henüz yarısı ızgarada olsa da.
        frame.move_caret([3.0, 3.5], TEXT, CURSOR, OPAQUE);
        assert!(frame.grid_caret().is_none(), "eski yuva temizlenmedi");
        let caret = frame.dock_caret(64.0).expect("caret dock yuvasında değil");
        // Instance pencere uzayında doğuyor (y = 3.5 × 16 = 56) ve dock
        // viewport'u 64'ten başlıyor: fark **negatif**, yani caret bandın
        // üstünde çiziliyor. Devrin ortasındaki kare tam olarak bu.
        assert_eq!(caret.pos[1], -8.0, "dock-yerel çeviri yanlış");

        // `clear` bandı da sıfırlıyor: dock'u olmayan bir sonraki karede caret
        // yine ızgaranın yuvasına düşmeli.
        frame.clear(grid(8, 16));
        frame.push_caret([3.0, 40.0], TEXT, CURSOR, OPAQUE, CaretShape::Block);
        assert!(
            frame.grid_caret().is_some(),
            "dock'suz karede caret dock yuvasına düştü"
        );
    }

    #[test]
    fn caret_shapes_narrow_both_rectangles() {
        // **İkisi birlikte daralır** ve bu şart: boyanan dörtlü ile ters
        // çevirme dikdörtgeni ayrışsaydı ince bir çubuğun altındaki harf
        // hücre boyunca çevrilirdi — ilk yazımda tam bu olacaktı.
        //
        // Kalınlık `CellMetrics::rule_px`'ten; burada 2.
        let mut frame = Frame::default();
        let metrics = CellMetrics::new(10, 20, 0, 2).expect("ölçü");

        frame.clear(metrics);
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block);
        let block = frame.grid_caret().expect("caret yok");
        assert_eq!((block.pos, block.size), ([10.0, 20.0], [10.0, 20.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 20.0, 20.0, 40.0]);

        // Alt çizgi hücrenin **dibinde**: 20 + 20 − 2.
        frame.clear(metrics);
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Underline);
        let under = frame.grid_caret().expect("caret yok");
        assert_eq!((under.pos, under.size), ([10.0, 38.0], [10.0, 2.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 38.0, 20.0, 40.0]);

        // Dikey çubuk hücrenin solunda ve tam boy.
        frame.clear(metrics);
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Beam);
        let beam = frame.grid_caret().expect("caret yok");
        assert_eq!((beam.pos, beam.size), ([10.0, 20.0], [2.0, 20.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 20.0, 12.0, 40.0]);
    }

    #[test]
    fn a_motion_frame_keeps_the_caret_shape() {
        // Hareket karesi `bt-core`'a hiç gitmiyor, yani şekli bilmiyor. Alan
        // `Frame`'de olmasaydı beam ilk sönüp yanışta bloğa dönerdi — blink
        // (phase-2) tam bu yoldan geçecek.
        let mut frame = Frame::default();
        frame.clear(CellMetrics::new(10, 20, 0, 2).expect("ölçü"));
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Beam);
        frame.move_caret([2.0, 1.0], TEXT, CURSOR, OPAQUE);
        let moved = frame.grid_caret().expect("caret yok");
        assert_eq!(moved.size, [2.0, 20.0], "hareket karesi şekli yuttu");
    }

    #[test]
    fn an_underline_caret_still_moves_to_the_dock_slot() {
        // **Daraltmanın yeri.** Yuva seçimi hücrenin **ayak izine** bakıyor;
        // daraltma ondan önce yapılsaydı alt çizgi caret'i bandın üstünde
        // kalır (yüksekliği 2 piksel) ve ızgara yuvasına düşerdi — dock'un
        // opak zemini onu örterdi.
        let mut frame = Frame::default();
        frame.clear(CellMetrics::new(8, 16, 0, 1).expect("ölçü"));
        frame.set_dock_top(64.0);
        frame.push_caret([3.0, 3.5], TEXT, CURSOR, OPAQUE, CaretShape::Underline);
        assert!(
            frame.grid_caret().is_none(),
            "alt çizgi caret'i ızgara yuvasında kaldı"
        );
        assert!(
            frame.dock_caret(64.0).is_some(),
            "alt çizgi caret'i dock yuvasına geçmedi"
        );
    }

    #[test]
    fn inkless_cell_yields_background_without_glyph() {
        // `hucre=K` ile `glif=G`'yi ayıran satır bu: `" bateri "` sekiz arka
        // planlı hücredir ama altı glyph'tir. İkisi tek sayaçtan okunsaydı
        // duman kapısı ikisinden birini hiç sormamış olurdu.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16));
        frame.push(bg_cell(0, 0)); // mürekkepsiz
        frame.push(Cell {
            col: 1,
            row: 0,
            ch: Some('b'),
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        });
        // Arka planı olmayan ama mürekkebi olan hücre: yalnız glyph listesine.
        frame.push(Cell {
            col: 2,
            row: 0,
            ch: Some('a'),
            fg: CURSOR,
            bg: None,
            ..Default::default()
        });

        assert_eq!(frame.bg_count(), 2);
        assert_eq!(frame.glyph_count(), 2);
        assert_eq!(
            frame.glyphs()[1],
            GlyphCell {
                pos: [16.0, 0.0],
                ch: 'a',
                face: Face::Regular,
                rgba: CURSOR.to_array(),
            }
        );

        frame.clear(grid(8, 16));
        assert_eq!(frame.glyph_count(), 0);
    }

    #[test]
    fn cell_yields_up_to_two_rules() {
        // Beş iddia, beşi de sessizce bozulabilir: kuralsız hücre kural
        // üretmez, altı çizili **mürekkepsiz** hücre üretir (`ch: None` kuralı
        // düşürmez), alt çizgi rengi SGR 58'den gelir, üstü çizili hep ön
        // plandan, ikisi aynı hücrede buluşabilir — ve `clear` kural listesini
        // de boşaltır (üç listenin üçü de aynı çağrıda sıfırlanmalı).
        let mut frame = Frame::default();
        frame.clear(grid(8, 16));

        frame.push(bg_cell(0, 0));
        assert_eq!(frame.rule_count(), 0, "kuralsız hücre kural üretti");

        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        frame.push(Cell {
            col: 1,
            row: 0,
            fg: CURSOR,
            underline: UnderlineStyle::Curl,
            underline_color: Some(red),
            strikeout: true,
            ..Default::default()
        });

        assert_eq!(frame.rule_count(), 2);
        assert_eq!(frame.glyph_count(), 0, "kural hücresi mürekkep üretmedi");
        assert_eq!(
            frame.rules(),
            [
                RuleCell {
                    pos: [8.0, 0.0],
                    kind: RuleKind::Curl,
                    rgba: red.to_array(),
                },
                RuleCell {
                    pos: [8.0, 0.0],
                    kind: RuleKind::Strike,
                    rgba: CURSOR.to_array(),
                },
            ]
        );

        frame.clear(grid(8, 16));
        assert_eq!(frame.rule_count(), 0);
    }

    #[test]
    fn sgr_flags_translate_to_four_faces() {
        // Çevirinin tek yeri burası ve dört kolun ikisi karıştığında belirti
        // "eğik metin kalın çiziliyor" olur — hiçbir sayaç görmez.
        assert_eq!(face(false, false), Face::Regular);
        assert_eq!(face(true, false), Face::Bold);
        assert_eq!(face(false, true), Face::Italic);
        assert_eq!(face(true, true), Face::BoldItalic);
    }
}
