//! Bir karenin çizim listesi.
//!
//! `bt-core`'un `frame()` sink'i burayı doğrudan doldurur: grid koordinatı
//! burada piksele çevrilir ve GPU'nun göreceği düzene girer. Renderer "ne
//! çizileceğini" buradan okur, "ne anlama geldiğini" bilmez.
//!
//! **On liste (artı dock'un iki efekt, seçimin iki ve aramanın dört listesi), üç yüzey, üç pipeline**
//! (buradaki listelerin; efektler beşincisinden, `glyph_fx`). Listeler yüzey başına dörtlü/üçlü
//! gruplar hâlinde: ızgaranın dördü (komut bloğu şeritleri, arka planlar,
//! glyph'ler, kural çizgileri), dock'un üçü (`dock_bg`/`dock_glyphs`/
//! `dock_rules`) ve doldurma bandının üçü (`fill_bg`/`fill_glyphs`/
//! `fill_rules`). Grupların ayrılığı sıra değil **koordinat uzayı**: her
//! yüzeyin kendi `setViewport`'u var ve satır numaraları yüzey-yerel, yani
//! tek listede ayırt edilemezlerdi ([`crate::Renderer`]). Pipeline ise üç ve
//! yüzeyden bağımsız: şeritler ve arka planlar `cell_bg`'nin, glyph'ler ve
//! kural çizgileri `cell`'in, caret de `cell_bg`'nin vertex'ini paylaşan
//! kardeş fragment'in (`caret`). Seçimin listesi (031) altıncı pipeline'ın
//! (`selection`): aynı `Instance`, kendi vertex'i ve köşe maskeli fragment'i;
//! aramanın dört listesi (033; ızgarada ve bantta ikişer, rol başına bir)
//! aynı pipeline'ı paylaşıyor. Grubun içindeki listelerin ayrı durmasının
//! sebebi çizim sırası — glyph'ler arka planların, kurallar da glyph'lerin
//! **üstüne** gelmek zorunda ve tek listede sıra hücre hücre karışırdı. Glyph
//! ile kuralın ayrı listede olması da aynı cümlenin devamı: ikisi aynı
//! pipeline'dan geçiyor ama üstü çizili, altındaki harften sonra çizilmeli.
//! Şerit `cell_bg`'yi arka planlarla paylaşıyor ama listesi ayrı ve gerekçesi
//! sıra değil **ömür**: [`Frame::move_caret`] arka plan listesini kırpıyor,
//! şerit ise hareket karesinde olduğu gibi kalmalı (bkz. [`Frame::stripes`]).

use std::mem::offset_of;

use bt_atlas::{Face, RuleKind, SizeClass};
use bt_core::{
    Block, ButtonState, CaretShape, CaretStyle, Cell, ClusterId, Clusters, DockButton, LinearRgba,
    SearchRun, SelectionRun, UnderlineStyle, UnfocusedCaret,
};

use crate::glyph_fx::{Fx, GlyphFx, Kind};
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

/// `shaders/glyph_fx.metal` → `FxInstance` ile alan alan aynı: dock'un yazım
/// efektlerinin instance'ı (030).
///
/// **[`GlyphInstance`]'ın genişletilmişi değil kardeşi**: bütün glyph
/// listelerinin stride'ı animasyonlu bir avuç glyph için büyürdü
/// (`.tasks/030-dock-yazim-animasyonlari/discussion.md` → Karar 5). İlk üç
/// alan onunkiyle aynı yerde, dördüncüsü efektin parametreleri:
///
/// - `fx[0]` — ilerleme `t`, `0..=1`; eğri shader'da.
/// - `fx[1]` — efekt kimliği, düzlem ve yarı **tek küçük tam sayıda**
///   (`kimlik | düzlem << 5 | yarı << 6`), `f32` olarak: tam sayı `f32`'de
///   birebir temsil ediliyor ve bit kalıbı (`from_bits`) olarak taşınsaydı
///   küçük bir kalıp denormal sayılıp `flat` aktarımda sıfırlanabilirdi.
/// - `fx[2]` — tohum ([`crate::glyph_fx::Fx::seed`]).
/// - `fx[3]` — yedek, sıfır.
///
/// Düzen dolgusuz: `float2` 8, `float4` 16 hizalı → 0/8/16/32, stride 48.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FxInstance {
    pub(crate) pos: [f32; 2],
    pub(crate) uv0: [f32; 2],
    pub(crate) rgba: [f32; 4],
    pub(crate) fx: [f32; 4],
}

const _: () = assert!(size_of::<FxInstance>() == 48);
const _: () = assert!(offset_of!(FxInstance, uv0) == 8);
const _: () = assert!(offset_of!(FxInstance, rgba) == 16);
const _: () = assert!(offset_of!(FxInstance, fx) == 32);

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
    /// Instance'a çevirir; hücre boyunu, şekli, kural kalınlığını ve hale
    /// payını çağıran veriyor ([`Frame`]'in alanları).
    ///
    /// **Dörtlü hale payı kadar şişiyor**, boyanan dikdörtgen şişmiyor: hale
    /// boyanın dışında yaşıyor ve fragment onu dikdörtgenin **dışındaki**
    /// mesafeden çiziyor. Şişme yalnız burada, çünkü yuva seçimi
    /// ([`Frame::push_caret`]) şişmemiş dikdörtgene bakmak **zorunda** — hale
    /// ayak izini büyütüp caret'i dock yuvasına kaydırsaydı caret ızgaranın
    /// glyph'lerinden sonra çizilir ve altındaki harfi boyardı (014 phase-1'de
    /// aynı tuzağa düşülmüştü).
    fn instance(self, cell_px: (f32, f32), shape: CaretShape, rule: f32, glow: f32) -> Instance {
        let (pos, size) = caret_painted_rect(self.at, cell_px, shape, rule);
        Instance {
            pos: [pos[0] - glow, pos[1] - glow],
            size: [size[0] + glow * 2.0, size[1] + glow * 2.0],
            rgba: self.rgba,
        }
    }
}

/// Halenin payı, **sol payın oranı** olarak.
///
/// **Seçilmiş, ölçülmemiş** ve bir kez gözle düzeltildi: pay başta sol payın
/// tamamıydı (varsayılan puntoda ~8 px) ve hale caret'in kendisi kadar
/// genişleyince ortaya "box shadow" değil neon çıktı — kullanıcının ilk
/// bakışta söylediği şey buydu. Yarısı gölge ölçeğinde kalıyor.
///
/// Oran, çünkü payın kaynağı hâlâ tek: `CellMetrics::gutter_px`. İkinci bir
/// tasarım sabiti yok ve punto büyüyünce hale de büyüyor.
///
/// İki turda indi: sol payın tamamı → yarısı → **beşte ikisi**; ikisi de gözle.
///
/// **Bu bir taban**, ayarın kendisi değil: `[terminal] cursor_glow` bunun
/// **çarpanı** ([`bt_core::CaretStyle`]). Sabitin burada kalması "ikinci bir
/// tasarım sabiti yok" kuralını koruyor — kullanıcı tasarımın ölçüsünü
/// ölçekliyor, yeni bir ölçü uydurmuyor.
pub(crate) const CARET_GLOW_RATIO: f32 = 0.4;

/// Halenin tepe alfası — dikdörtgenin kenarında bu, hale payının ucunda sıfır.
///
/// **Seçilmiş, ölçülmemiş** ve yine gözle düzeltildi: 0.35 altın bir bloğun
/// çevresinde parlıyordu. İstenen "box shadow gibi temiz bir hafif tasarım
/// dokunuşu", yani gölge ölçeğinde bir alfa. Caret'in kendi alfasıyla
/// **çarpılıyor**, yani blink sönerken hale de sönüyor (R6) ve ikinci bir yol
/// yazılmıyor.
///
/// İki turda indi: 0.35 → 0.14 → **0.10**; ikisi de gözle. [`CARET_GLOW_RATIO`]
/// gibi **taban**: `cursor_glow` onu da aynı çarpanla ölçekliyor, çünkü pay ve
/// alfa tek his (016 `discussion.md` → Muhakeme).
const CARET_GLOW_ALPHA: f32 = 0.10;

/// Caret'in köşe yarıçapı, piksel — **kırpması dahil**.
///
/// Kırpma burada, shader'da değil: yarıçapın değerine karar veren taraf tek
/// olmalı ve o taraf hücre ölçüsünü bilen taraf. `caret_fragment` kendi
/// `min`'ini koruyor ama o bir **politika değil matematik ön koşulu** (SDF
/// yarıçapın yarım ölçüyü aşmamasını istiyor); değeri burası seçiyor.
/// Sınamalar da buradan okuyor, yoksa formülün üçüncü bir yazarı olurdu.
///
/// Oran **argüman**, sabit değil: 016'dan beri ayardan geliyor
/// ([`bt_core::CaretStyle`]) ve varsayılanının tek sahibi `bt-core`
/// ([`bt_core::CURSOR_RADIUS`]).
pub(crate) fn caret_radius_px(cell_px: (f32, f32), ratio: f32) -> f32 {
    (cell_px.1 * ratio)
        .min(cell_px.0 / 2.0)
        .min(cell_px.1 / 2.0)
}

/// Seçim şeklinin köşe yarıçapı, hücre **yüksekliğinin** oranı — tasarım
/// sabiti, ölçülmüş bir sayı değil.
///
/// Caret'in oranından ([`bt_core::CURSOR_RADIUS`]) **ayrı** ve iki katından
/// büyük: seçim bir metin bloğunu saran yüzey, caret ise hücre boyunda bir
/// blok — aynı piksel yarıçapı seçimde köşeyi görünmez kılıyordu (031
/// phase-3'ün gözle kontrolü, kullanıcı: "radius değerini biraz daha
/// arttırabilirsin"; 13pt@2x'te ≈3 px → ≈7 px). Oran, piksel değil: Cmd +/−
/// ile köşe de büyüyor. Kırpma [`caret_radius_px`]'ten: tek hücrelik seçimde
/// yarıçap hücrenin kısa kenarının yarısını aşmıyor, yani şekil hap olsa da
/// bozulmuyor. Kullanıcının `cursor_radius`'u buna dokunmuyor (Karar 10:
/// anahtar imlecin).
pub(crate) const SELECTION_RADIUS: f32 = 0.22;

/// Seçim şeklinin bir köşesi (031 phase-3); sırası [`selection_corners`]'ın
/// dizisinde TL, TR, BR, BL — `selection_fragment`'in maske sırası.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Corner {
    /// Açıkta kalan köşe: o kenardaki komşu satırın koşusu köşenin
    /// sütununu örtmüyor (ya komşu yok, ya sütun onun dışında).
    Convex,
    /// Komşu koşu köşeyi örtüyor ve kenar ikisinde hizalı: şekil düz devam
    /// ediyor.
    Square,
    /// Komşu köşeyi örtüyor **ve** bu koşunun kenarını aşıyor: köşe kare,
    /// basamağın dışına bir içbükey dolgu parçası düşüyor. Dolguyu her zaman
    /// **dar** koşu doğuruyor — geniş olanın o kenarı komşusunca örtülmüyor,
    /// yani her basamak tam bir kez dolduruluyor.
    Concave,
}

/// `runs[index]`'in dört köşesi (TL, TR, BR, BL).
///
/// Komşu yalnız **bitişik** satırın koşusu: araya koşusuz bir satır girerse
/// (Karar 4 — çizilir hücresi olmayan satır koşu üretmez) şekil orada
/// bölünüyor ve iki parça kendi köşelerini alıyor. `runs` satır sırasıyla ve
/// satır başına en çok bir koşu (`bt_core::SelectionRuns::as_slice`).
///
/// Karar sütunda, pikselde değil: köşe o kenardaki komşunun o **sütunu**
/// kapsayıp kapsamadığına bakıyor. Yalnız çaprazdan değen iki koşu (üstteki
/// 5'ten başlıyor, alttaki 4'te bitiyor) birbirini örtmüyor ve iki dışbükey
/// köşe veriyor — satır akışındaki seçimin olağan hâli.
pub(crate) fn selection_corners(runs: &[SelectionRun], index: usize) -> [Corner; 4] {
    let this = runs[index];
    let adjacent = |other: Option<&SelectionRun>, row: Option<u16>| {
        other.copied().filter(|o| Some(o.row) == row)
    };
    let above = index
        .checked_sub(1)
        .and_then(|i| adjacent(runs.get(i), this.row.checked_sub(1)));
    let below = adjacent(runs.get(index + 1), this.row.checked_add(1));
    // Sol köşe `first` sütununa, sağ köşe `last` sütununa bakıyor; komşu o
    // sütunu kapsıyorsa kare, üstelik o yönde taşıyorsa içbükey.
    let left = |n: Option<SelectionRun>| corner(n, this.first, |n| n.first < this.first);
    let right = |n: Option<SelectionRun>| corner(n, this.last, |n| n.last > this.last);
    [left(above), right(above), right(below), left(below)]
}

/// [`selection_corners`]'ın tek köşesi: komşu `col` sütununu örtüyor mu, ve
/// örtüyorsa bu kenarı aşıyor mu.
fn corner(
    neighbour: Option<SelectionRun>,
    col: u16,
    extends: impl Fn(SelectionRun) -> bool,
) -> Corner {
    match neighbour {
        Some(n) if (n.first..=n.last).contains(&col) => {
            if extends(n) {
                Corner::Concave
            } else {
                Corner::Square
            }
        }
        _ => Corner::Convex,
    }
}

/// Caret'in **iki** dikdörtgeni; ikisi de sol üst köşe + ölçü, pencere uzayı.
///
/// Değişmez ("tek yer, iki tüketici") kırılmıyor, **eksik tanımlıydı**: bir
/// caret'in boyandığı alan ile altındaki metni ters çevirdiği alan aynı şey
/// değil. Dolu caret'te ikisi eşit; **içi boş caret'te boyanan var, opak iç
/// yok** — ters çevirme boyanan zemine dayanıyor.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CaretRects {
    /// Fragment'in gövdeyi çizdiği alan — hale bunun **dışında** yaşıyor.
    painted: ([f32; 2], [f32; 2]),
    /// Ters çevirmenin alanı ([`CursorBlock`]). Boş dikdörtgen = ters çevirme
    /// yok, ayrı bir bayrak değil.
    opaque: ([f32; 2], [f32; 2]),
}

/// Caret'in dikdörtgenleri: sol üst köşe ve ölçü, **pencere uzayında** piksel.
///
/// **Tek yer, iki tüketici:** boyanan dörtlü ([`Caret::instance`]) ve ters
/// çevirme dikdörtgeni ([`CursorBlock`]). Ayrı yazılsalardı biri daralıp
/// öteki hücrenin tamamında kalırdı ve belirti sessiz olurdu — ince bir
/// çubuğun altındaki harf, hücre boyunca ters çevrilmiş görünürdü.
///
/// **Kalınlık uydurulmuyor:** fontun kendi alt çizgi metriğinden geliyor
/// (`CellMetrics::rule_px`), chevron emsali. Hücreyi aşamaz — küçük puntoda
/// metrik hücreden büyük çıkabilir ve caret komşu hücreye taşardı.
fn caret_painted_rect(
    at: [f32; 2],
    cell_px: (f32, f32),
    shape: CaretShape,
    rule: f32,
) -> ([f32; 2], [f32; 2]) {
    // **`clamp` değil `min`+`max`** (`/code-review`, 014 kapı): `f32::clamp`
    // `min <= max` istiyor ve `Frame::default()`'ın hücresi `(0.0, 0.0)` —
    // `clear` çağrılmadan gelen bir `push_caret` display link callback'inin
    // içinde **panik** ederdi. Panik yolu değil ama bir pencereyi öldürürdü;
    // sıfır hücrede kalınlık da sıfır kalıyor, yani çizilmeyen bir caret.
    let limit = cell_px.0.min(cell_px.1);
    let thick = rule.max(1.0).min(limit);
    match shape {
        CaretShape::Block => (at, [cell_px.0, cell_px.1]),
        // Hücrenin **dibinde**, fontun alt çizgi konumunda değil: o konum
        // taban çizgisinin hemen altı ve caret orada `g`'nin kuyruğunu
        // keserdi. Metrikten alınan şey konum değil **kalınlık**.
        CaretShape::Underline => ([at[0], at[1] + cell_px.1 - thick], [cell_px.0, thick]),
        CaretShape::Beam => (at, [thick, cell_px.1]),
    }
}

/// [`caret_painted_rect`]'in üstüne ters çevirme alanını ekler.
///
/// **Boyanan alan odağı bilmiyor** ve bilmemeli: içi boş caret aynı yeri
/// kaplıyor, yalnız içini boyamıyor. Odağa bağlı olan tek şey opak iç.
fn caret_rect(
    at: [f32; 2],
    cell_px: (f32, f32),
    shape: CaretShape,
    rule: f32,
    hollow: bool,
) -> CaretRects {
    let rect = caret_painted_rect(at, cell_px, shape, rule);
    CaretRects {
        painted: rect,
        // **İçi boş caret'te opak iç yok.** Ters çevirme boyanan zemine
        // dayanıyor: boyanmayan bir pikselin altındaki harf kendi rengiyle
        // kalmalı, yoksa çerçevenin ortasındaki metin zemin renginde çizilir
        // ve **görünmez** olurdu. Boş dikdörtgen ayrı bir bayrak değil,
        // `CursorBlock`'un kendi sözleşmesi ("görünmez imleç dejenere bir
        // dikdörtgendir").
        opaque: if hollow {
            ([0.0, 0.0], [0.0, 0.0])
        } else {
            rect
        },
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
    /// Hangi punto sınıfından rasterize edileceği; yüze **dik** bir eksen.
    ///
    /// Tek üreticisi [`Frame::push_dock`] ve tek değeri dock'un bağlam
    /// satırında `Small`. Ayrı bir liste açılmadı ve sebebi encode:
    /// `GlyphInstance` bundan da etkilenmiyor (uv0 yuvayı kodluyor), yani
    /// küçük glyph aynı listede, aynı draw call'da ve aynı `cell_px`
    /// uniform'uyla çiziliyor. Dörtlü büyük kalıyor, küçük harf onun sol
    /// kenarında duruyor; komşu dörtlüler örtüşüyor ama örtüşen pikseller
    /// saydam ve blend `SourceAlpha`, yani altındaki harf bozulmuyor.
    pub(crate) size: SizeClass,
    pub(crate) rgba: [f32; 4],
    /// Sınırın `Cell::wide`'ı: hücre **iki sütun** genişliğinde bir
    /// karakterin baş hücresi mi.
    ///
    /// Alan `half` **değil** ve bu bilinçli: "hangi yarı" sorusunun cevabı
    /// mürekkep kapısında, yani `Atlas::slot`'ta doğuyor ve bu liste atlası
    /// hiç görmüyor (tipin uv'siz olmasının gerekçesi hemen yukarıda).
    /// Yelpazeleme bu yüzden `AtlasTexture::prepare`'de: orada atlas zaten
    /// ödünç alınmış ve hücre ölçüsü elde.
    pub(crate) wide: bool,
    /// Sınırın `Cell::cluster`'ı: emoji dizisinin **listenin kendi**
    /// tablosundaki kimliği (035 Karar 4B) — ızgara ve doldurma bandı
    /// [`Frame::clusters`], dock [`Frame::dock_clusters`], hayaletler
    /// [`Frame::fx_clusters`]. Dizgi atlasa `prepare` anında iniyor
    /// (`Atlas::intern`), `ch` gibi: bu liste atlası görmüyor.
    pub(crate) cluster: Option<ClusterId>,
}

/// Bir küme kimliğini `from` tablosundan `to` tablosuna taşır (035): ömrü
/// kaynağınkini aşan listeler (yazım efektleri) dizgiyi kendi tablolarına
/// kopyalıyor. Kaynakta bulunamayan kimlik `None` — glyph taban karakterle
/// çiziliyor, yanlış bir dizgiyle değil.
pub(crate) fn copy_cluster(
    id: Option<ClusterId>,
    from: &Clusters,
    to: &mut Clusters,
) -> Option<ClusterId> {
    id.and_then(|id| from.get(id))
        .and_then(|text| to.push(text))
}

/// Çizilecek bir yazım efekti: glyph'i ve efektin parametreleri — uv'siz,
/// [`GlyphCell`] ile aynı gerekçe (yuva çözümü `encode` anında).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FxCell {
    pub(crate) glyph: GlyphCell,
    /// İlerleme, `0..=1`.
    pub(crate) t: f32,
    /// Shader'ın efekt kimliği ([`crate::glyph_fx`]).
    pub(crate) effect: u32,
    pub(crate) seed: f32,
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

/// Dock'un kare başına tek olan iki rengi.
///
/// Satır sayısı burada **değil** (032): hücrelerin yerleşimi ona bağlı ve
/// hücreler yüzeyden **önce** basılıyor ([`Frame::open_dock`]), yani sayı
/// hücrelerden önce yazılan ayrı bir alanda ([`Frame::set_dock_rows`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DockSurface {
    /// Yüzeyin zemini; **opak** (`bt_core::Dock::ground`). Kayma boyunca
    /// ızgaranın taşan alt satırı bunun altında kalıyor.
    ground: [f32; 4],
    /// Dock'u ızgaradan ayıran **üst** saç çizgisi (`bt_core::Dock::edge`):
    /// uzak oturumda ayrı bir renk (036), yani ikinci çizgiyle aynı alan
    /// olamaz.
    edge: [f32; 4],
    /// Giriş bloğunu bağlam satırından ayıran saç çizgisi.
    separator: [f32; 4],
    /// Üst çizginin dolan payı, `0..=1` ([`Frame::set_dock_progress`]);
    /// `None` → çizgi bütünüyle `edge`.
    progress: Option<f32>,
    /// Yükleme satırının düğmeleri ([`Frame::set_dock_buttons`]); açılış
    /// her karede siliyor, yani düğme yalnız söylendiği karede var.
    buttons: [Option<DockButton>; 2],
}

/// Yükleme düğmesinin dolgusunun ve çerçevesinin alfası, durum başına (037
/// phase-6) — **tasarım sabiti**, onaylanan tasarımın değerleri: dinlenen
/// düğme sönük bir dolgu ve belirgin bir çerçeve, fare üstündeyken ikisi de
/// koyulaşıyor, basılıyken dolgu bir ton daha.
///
/// Alfa burada, `bt-core`'da değil: renk paletin (işaretin rengi), opaklık bu
/// karenin çizim durumu ([`with_alpha`]'nın gerekçesi).
const fn button_alpha(state: ButtonState) -> (f32, f32) {
    match state {
        ButtonState::Idle => (0.16, 0.38),
        ButtonState::Hover => (0.34, 0.7),
        ButtonState::Pressed => (0.5, 0.7),
    }
}

/// Yuvarlak dikdörtgenin bir çizimi: dörtlü (dock-yerel), `caret_fragment`'in
/// pencere uzayındaki çekirdeği ve şekil uniform'u.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RoundedDraw {
    pub(crate) instance: Instance,
    pub(crate) core: [f32; 4],
    pub(crate) shape: [f32; 4],
}

/// PTY'nin dock'a **ayırdığı** pay, satır: bir giriş satırı + bağlam satırı.
///
/// İki, çünkü dock'un tasarımı iki satır (`plan.md` → Hedef): üstte
/// `>` + ZLE'nin görüntüsü, altta `[klasör] | [dal]`. Pay ızgaranın
/// yüksekliğinden düşülüyor, yani sayıyı sonradan büyütmek kullanıcının
/// penceresini bir satır kısaltan ikinci bir `TIOCSWINSZ` demek.
///
/// **Ayrılan, çizilen değil** (032): dock'un çizilen bandı giriş satırı
/// sayısıyla büyüyor ([`band_px`]) ama bu pay **hiç değişmiyor** — kabuk
/// SIGWINCH görmüyor, ızgara çizimde yukarı ötelenerek yer açıyor
/// (`.tasks/032-cok-satirli-dock/discussion.md` → Karar 1).
///
/// Bu crate'in sabiti çünkü çizen bu crate; `bt-shell` onu ızgara
/// aritmetiğinde (`split_into_grid`) **tüketiyor** ve ikinci bir kopya
/// tutmuyor — payın `CellMetrics` ile taşınmasıyla aynı disiplin.
pub const DOCK_ROWS: u16 = 2;

/// PTY'nin dock'a **ayırdığı** yükseklik, **piksel**; `dock_rows == 0` ise
/// sıfır. Çizilen bandın boyu bu değil: o [`band_px`].
///
/// Formülün **tek** kopyası burası ve iki tüketicisi var: ızgaranın satır
/// aritmetiği (`bt_shell`'in `split_into_grid`'i) ve ikinci viewport'un
/// orijini ([`Frame::dock_layout_px`]). Ayrı ayrı yazılsalardı yeniden boyutlandırmada
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

/// Dock'un **çizilen** bandının yüksekliği, piksel: `input_rows` giriş satırı
/// artı bağlam satırı (032).
///
/// [`dock_px`]'in (PTY'nin **ayırdığı** pay) kardeşi ve ondan ayrı bir ad,
/// çünkü iki sayı artık ayrışıyor: pay [`DOCK_ROWS`]'la sabit, bant giriş
/// satırlarıyla büyüyor ve aradaki fark ızgaranın çizimde yukarı
/// ötelenmesiyle kapanıyor. `input_rows == 1`'de ikisi **aynı** piksel.
///
/// Giriş satırları arasında boşluk yok — tek bir editör yüzeyi; boşluk ve
/// ikinci saç çizgisi yalnız giriş bloğu ile bağlam satırı arasında
/// (`discussion.md` → Karar 9). Formülün gövdesi yine [`dock_height`]: bant
/// `input_rows + 1` satırlık bir dock.
pub(crate) fn band_px(input_rows: u16, cell: CellMetrics) -> f32 {
    dock_px(input_rows.saturating_add(1), cell)
}

/// Dock'un giriş satırlarının tavanı: ızgaranın satırlarının **yarısı**
/// (032 Karar 4) — **tasarım sabiti**, ölçülmüş bir sayı değil
/// ([`bt_atlas::CONTEXT_SCALE`]'in emsali).
///
/// Gerekçe: komutun yazıldığı yüzey ile onun bağlamı olan çıktı eşit kalsın,
/// editör pencereyi yutmasın. **Oran**, mutlak sayı değil: pencereyle ve
/// puntoyla ölçekleniyor. Aşan girişte dock kendi içinde caret'i izleyen
/// dikey bir pencere açıyor (`bt_core`'un `dock::window_top`'u).
///
/// Yerleşim kararı çizenin, yani sabit burada; `bt-core` onu bütçe olarak
/// alıyor ([`bt_core::DockBudget::share`]) ve ızgaranın satır sayısının tek
/// okumasına uyguluyor — bu katman satır sayısının ikinci bir kopyasını
/// tutmuyor (`Layout`'un doc'u).
pub(crate) const DOCK_MAX_SHARE: f32 = 0.5;

/// Bağlam satırının sütun bütçesi: **aynı piksel şeridi, küçük adım**.
///
/// Dock sol payı ızgarayla paylaşıyor ([`Frame::dock_pos`]), yani iki satırın
/// kapladığı yatay şerit birebir aynı; ayrışan tek şey bir harfin kaç piksel
/// ilerlettiği. Bütçe bu yüzden bir oran: `cols * hücre / bağlam hücresi`.
///
/// Hesabın burada olması şart — `bt-core` piksel görmüyor ve görmemeli
/// (`dock::render`'ın `context_cols`'u bir **bütçe**, punto kararı değil).
/// `u32`'de çarpılıyor: 65535 sütun × 65535 piksel `u16`'yı taşırdı, oysa
/// ara değer yalnız bir orana giriyor.
///
/// Bölen ≥ 1 ve bu **yapısal**: [`CellMetrics::new`] sıfır bağlam genişliğini
/// eliyor, yani burada ikinci bir kapı yok.
pub fn context_cols(cols: u16, cell: CellMetrics) -> u16 {
    let span = u32::from(cols) * u32::from(cell.cell_px().0);
    u16::try_from(span / u32::from(cell.context_cell_px())).unwrap_or(u16::MAX)
}

/// Bağlam satırının hücre bandının tepesi, giriş bloğunun **dibinden**
/// ölçülen piksel: giriş satırı varsa satır arası boşluk, yoksa (uzak oturum,
/// 036) sıfır — [`Frame::dock_pos`]'un kuralı. Fare yükleme düğmesinin
/// dikey aralığını bundan okuyor (037 phase-6): dolgu tam o bantta
/// ([`Frame::dock_button_draws`]).
pub fn context_row_offset(input_rows: u16, cell: CellMetrics) -> f32 {
    if input_rows == 0 {
        0.0
    } else {
        dock_row_gap(f32::from(cell.gutter_px()))
    }
}

/// Formülün gövdesi, ham sayılarla: [`dock_px`] ile [`Frame`] aynı aritmetiği
/// paylaşsın diye ayrı. `Frame` [`CellMetrics`]'i alan olarak tutamıyor
/// (kurucusu sıfırı eliyor, yani `Default`'u yok), ama iki bileşeni zaten
/// elinde.
///
/// **Tek satır arası boşluk** (032): bağlam satırı ile üstündeki giriş bloğu
/// arasında. Giriş satırları kendi aralarında bitişik, yani `rows` satırlık
/// bir dock `rows · cell_h + 2 · pad + gap`; tek satırlık dock (yalnız
/// sınamalarda) boşluksuz. 032'den önce her satır arasında bir boşluk vardı
/// ve iki satırlık dock'ta iki formül aynı sayıyı veriyor.
fn dock_height(rows: u16, cell_h: f32, pad: f32) -> f32 {
    if rows == 0 {
        return 0.0;
    }
    let gap = if rows >= 2 { dock_row_gap(pad) } else { 0.0 };
    f32::from(rows) * cell_h + 2.0 * pad + gap
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
    /// girseydi ya sayılmadan girerdi — o dönem hareket karesi `bg`'yi
    /// `bg_count`'a kırpıyordu ve şerit imleç kaydıkça **titrerdi**; caret
    /// 012'den beri kendi yuvasında ([`Frame::grid_caret`]), 015'ten beri de
    /// kendi pipeline'ında — arka plan listesine hiç girmiyor ve kırpma kalktı,
    /// ama ayrılığın gerekçesi duruyor — ya da sayılarak girer ve `hucre=` jetonunun
    /// anlamı kayardı ("çizilen hücre" artık hücre olmayan bir şeyi de
    /// sayardı). Üçüncü bir liste ikisini de temsil edilemez kılıyor.
    stripes: Vec<RuleCell>,
    bg: Vec<Instance>,
    /// Fareyle seçimin satır koşuları ve içbükey dolguları (031); kendi
    /// pipeline'ından (`selection`, köşe maskeli SDF), arka planlardan
    /// **sonra**, caret'ten ve glyph'lerden önce çiziliyor
    /// ([`Renderer::encode_pass`](crate::renderer::Renderer)).
    ///
    /// Listenin ayrılığının pipeline dışındaki sebebi şeridinkiyle aynı: `bg`'ye girseydi ya sayılarak
    /// girer ve `hucre=` jetonunun anlamı kayardı ("çizilen hücre" bir satır
    /// koşusunu da sayardı), ya da sayılmadan girer ve `bg_count`'un
    /// bekçisini delerdi. Sayacı **yok**: duman reçetesinde seçim yok, yani
    /// jeton hiçbir şey söylemezdi; kanıtı `renderer.rs`'in offscreen okuması.
    selection: Vec<Instance>,
    /// Dock'un seçim koşusu (031 phase-4): `selection`'ın dock yüzeyindeki
    /// ikizi — aynı pipeline, aynı renk ve yarıçap uniform'u, dock'un kendi
    /// viewport'unda ([`Frame::push_dock_selection`]). Ayrı liste, çünkü
    /// ızgaranın listesi ötelemeyle kayıyor, dock ise ondan muaf.
    dock_selection: Vec<Instance>,
    /// Seçimin bu karedeki rengi ([`Frame::push_selection`] yazıyor);
    /// hareket karesi listeyi koruduğu gibi onu da koruyor.
    selection_rgba: [f32; 4],
    /// Arama vurgusunun parçaları (033), `selection`'ın pipeline'ından ve
    /// şeklinden: bütün eşleşmeler (`search_match`) ve geçerli eşleşme
    /// (`search_current`). **Rol başına bir liste**, çünkü renk çağrı başına
    /// uniform — iki rol iki encode ([`Frame::push_search`]). Seçimden ayrı,
    /// çünkü seçim aramanın **üstünde** çiziliyor (Karar 7) ve kendi rengini
    /// taşıyor. Sayacı yok, seçiminkiyle aynı gerekçe.
    search_match: Vec<Instance>,
    search_current: Vec<Instance>,
    /// İki rolün bu karedeki renkleri; `selection_rgba` gibi hareket
    /// karesinde korunuyor.
    search_match_rgba: [f32; 4],
    search_current_rgba: [f32; 4],
    /// Eşleşme başına köşe kararının geçici tamponu
    /// ([`Frame::search_parts`]): kare başına ayırma yok.
    search_scratch: Vec<SelectionRun>,
    glyphs: Vec<GlyphCell>,
    /// Kural çizgileri; glyph'lerle **aynı** pipeline'dan ama onlardan sonra
    /// çizilir (üstü çizili, altındaki harfin üstünden geçmeli).
    rules: Vec<RuleCell>,
    cell_px: (f32, f32),
    /// Dock'un bağlam satırındaki sütun adımı, piksel
    /// ([`CellMetrics::context_cell_px`]). `cell_px` ile aynı gerekçeyle
    /// alan: hareket karesi `clear` çağırmıyor ve değeri **koruyor**.
    context_cell_px: f32,
    /// Kural çizgisinin kalınlığı, piksel — ince caret'lerin genişliği
    /// ([`caret_rect`]). `cell_px` ile aynı gerekçeyle alan: hareket karesi
    /// `clear` çağırmıyor ve değeri **koruyor**.
    rule_px: f32,
    /// Caret'in şekli; [`Frame::push_caret`] yazıyor, [`Frame::move_caret`]
    /// koruyor — o yol `bt-core`'a hiç gitmiyor.
    caret_shape: CaretShape,
    /// Caret'in içi boş mu — odaksız pencerenin işareti.
    ///
    /// [`Frame`]'de yaşıyor, imzada taşınıp unutulmuyor: hareket karesi
    /// ([`Frame::move_caret`]) `bt-core`'a hiç gitmiyor ve odağı bilmiyor.
    /// Alan olmasaydı odaksız pencerede ilk hareket karesinde caret dolardı.
    ///
    /// `CaretShape`'e **eklenmedi** (R7.3): o enum ayar dosyasının sözlüğü
    /// (`"block" | "underline" | "beam"`) ve odak şekle **dik** bir eksen.
    caret_hollow: bool,
    /// Pencere odakta mı — [`Frame::push_caret`] yazıyor,
    /// [`Frame::move_caret`] **koruyor**.
    ///
    /// İmlecin **ayardan gelen** çizim sayıları; `clear`'ın ikinci argümanı
    /// yazıyor, hareket karesi koruyor (o `clear` çağırmıyor).
    ///
    /// `CellMetrics`'e **binmedi** (016 R3.1): o font geometrisi ve 32 çağrı
    /// yeri var; `GUTTER_PT`'nin doc'u zaten "ayar değil sabit" diyor.
    caret_style: CaretStyle,
    /// Izgaranın sol payı: her hücrenin x'i buradan **sonra** başlar.
    ///
    /// `cell_px` ile aynı gerekçeyle alan değil [`Frame::clear`]'ın taşıdığı
    /// bir değer (ikisi de tek [`CellMetrics`] ile geliyor): ölçek değişince
    /// ikisi birlikte tazelenir. Ayrı bir sabitten okunsaydı `cols` hesabıyla
    /// ayrışabilirdi — üçünün tek kaynağı olması 010 Karar 3'ün şartı.
    gutter_px: f32,
    /// Dock bandının **çizilen** tepesi, **pencere uzayında piksel**; dock
    /// yoksa sonsuz (caret hiçbir zaman dock yuvasına düşmez).
    ///
    /// Çağıran yazıyor ([`Frame::set_dock_band`], bandın boyuyla aynı
    /// çağrıda), çünkü dokunun boyunu bilen
    /// tek yer kare yolu; `Frame` listelerin uzayını biliyor, pencereninkini
    /// değil ([`Frame::dock_ground`]'un genişliği argüman almasıyla aynı
    /// gerekçe).
    dock_top_px: f32,
    /// Dock'un **yerleşim** satırları: giriş satırları + bağlam satırı
    /// ([`Frame::set_dock_rows`]). Hücrelerin yeri ([`Frame::dock_pos`]) ve
    /// bağlam satırının hangisi olduğu buradan; bandın **çizilen** boyu
    /// ([`Frame::dock_band`]) ise animasyonun o anki değeri ve ondan ayrı.
    ///
    /// [`Frame::clear`] [`DOCK_ROWS`]'a döndürüyor; hareket karesi `clear`
    /// çağırmadığı için değeri koruyor — hücreler de korunuyor.
    dock_rows: u16,
    /// Yerleşimin **son satırı bağlam satırı** mı ([`Frame::is_context_row`]).
    ///
    /// Satır sayısından türemiyor, çünkü tek satırlık yerleşimin iki anlamı
    /// var: üretimde uzak oturumun yalnız bağlam satırından ibaret bandı
    /// (036, [`Frame::set_dock_input_rows`] sıfırla) ve sınamaların bağlamsız
    /// tek giriş satırı ([`Frame::set_dock_rows`]). İkincisi "iki ve fazlası
    /// → son satır bağlam" kuralıyla kalıyor.
    dock_context: bool,
    /// Çizilen bandın PTY payından **fazlası**, piksel (aygıt ızgarasına
    /// yuvarlı); `None` → bu kare söylemedi ve band yerleşimin boyunda
    /// ([`Frame::set_dock_band`]).
    ///
    /// İki tüketicisi aynı sayıyı okuyor ve ayrışamıyorlar: bandın boyu
    /// ([`Frame::dock_band_px`]) ve ızgaranın çizilen orijini
    /// ([`Frame::origin_px`], `− fazla`). Izgaranın alt kenarı ile bandın üst
    /// kenarı bu yüzden **yapısal olarak** birlikte kayıyor — iki ayrı
    /// yuvarlama bir piksel ayrışabilirdi.
    ///
    /// Encode anında okunuyor (`fill_origin_px`'in emsali) ve **iki** kare yolu
    /// da yazıyor: hareket karesi `clear` çağırmıyor ama bant onun karesinde de
    /// ilerliyor.
    dock_band: Option<f32>,
    /// Pencerenin dibi, piksel — bandın dibe yaslı tepesinin ve fareye
    /// yayınlanan dock geometrisinin tabanı ([`Frame::set_dock_band`]).
    dock_bottom_px: f32,
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
    /// uygulanıyor: `setViewport` ızgaranın dört listesini birden kaydırıyor
    /// ([`crate::Renderer`]) ve instance başına maliyeti sıfır.
    ///
    /// **İkinci okuyucusu doldurma bandı** ([`Frame::fill_origin_px`]): kendi
    /// viewport'u var ama orijini buradan türüyor (`origin_px − fill_px`),
    /// yani bant ızgarayla **birlikte** kayıyor. Türetme de okuma anında,
    /// yoksa hareket karesinde bayatlardı.
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
    ///
    /// **Kaydırmanın kesri burada değil** ([`Frame::frac_px`]): viewport'un
    /// orijini ikisinin toplamı ve toplamı yalnız [`Frame::origin_px`]
    /// veriyor, yani iki setter'ın çağrı sırası sonucu değiştirmiyor.
    origin_px: f32,
    /// Kaydırmanın kesri (`bt_core::Cursor::scroll_frac`), **aygıt pikseline
    /// yuvarlı** — [`Frame::origin_px`]'in ikinci terimi.
    ///
    /// Ötelemeden **ayrı** tutuluyor ve ayrı yuvarlanıyor, çünkü iki
    /// tüketicisi ayrışıyor: ızgaranın viewport'u ikisinin toplamını, caret
    /// ise **yalnız bunu** istiyor ([`Frame::push_caret`]) — caret ötelemenin
    /// kaymasından muaf (hedefi ekran satırı) ama kesirden değil, kesir
    /// ızgaranın bütün dünyasını, altındaki imleç dahil kaydırıyor. Toplam
    /// tek yuvarlamayla yazılsaydı caret'in payı ondan geri çıkarılamazdı ve
    /// yerleşmiş caret harfinden bir piksel ayrışabilirdi.
    ///
    /// **Bir hücreden kısa** ([`Frame::set_scroll_frac`]): yuvarlama
    /// `1 − ½/h`'nin üstündeki kesri tam hücreye çıkarır ve ızgara ofset
    /// değişmeden bir satır aşağı çizilirdi.
    ///
    /// [`Frame::clear`] sıfırlıyor (içerik karesi her karede yeniden
    /// söylüyor); hareket karesi onu **koruyor**, çünkü `clear` çağırmıyor ve
    /// kesir yalnız içerik karesinde değişebiliyor — süzülme uçuştayken link
    /// içerik yoluna düşüyor.
    frac_px: f32,
    /// İmlecin piksel dikdörtgeni ve blok altındaki metin rengi; `cell`
    /// pipeline'ının uniform'u.
    ///
    /// Liste değil **alan**: kare başına tek imleç var ve [`Frame::clear`] onu
    /// dejenereye döndürüyor. Alan olması hareket karesinin de şartı: o yol
    /// [`Frame::move_caret`] ile yuvaları boşaltıp [`Frame::push_caret`]'ı
    /// yeni konumla yeniden çağırıyor, yani ikinci çağrı birincinin üstüne
    /// yazmak zorunda.
    cursor: CursorBlock,
    /// Caret'in **boyanan** dikdörtgeni (x0, y0, x1, y1), pencere uzayı.
    ///
    /// [`CursorBlock::rect`]'ten ayrı bir alan ve ayrılığı phase-3'ün şartı:
    /// içi boş caret'te boyanan var, opak iç yok. Dejenere (hepsi sıfır) =
    /// çizilecek caret yok.
    caret_core: [f32; 4],
    /// SDF uniform'unun **sınama ezmesi**; üretimde hep `None`.
    ///
    /// **Yarım bir ezme** (`/code-review`): yalnız fragment uniform'unu
    /// çeviriyor, dörtlünün hale payı kadar şişmesini ([`Frame::glow_px`])
    /// çevirmiyor. Yani hale payını burada büyütmek dörtlüyü büyütmez ve
    /// hale çekirdeğin dışına çıkamaz. Sınamalar bu yüzden haleyi ezmeyle
    /// değil **paylı bir ızgarayla** açıyor; ezme yalnız yarıçapı ve kenarı
    /// sürmek için.
    ///
    /// Geri alma yolunun (R8) tek bekçisi buradan geçiyor: "yarıçap 0, hale 0"
    /// kolunun çıktısı 014'ün düz dörtgeniyle **bit bit** aynı olmak zorunda
    /// ve bunu yalnız GPU söyleyebilir — dejenere kolda fragment `step`,
    /// açık kolda `smoothstep` kullanıyor ve ikisinin kenar pikselleri
    /// ayrışır. Üretimde bir kurucusu olsaydı ölü kod olurdu.
    #[cfg(test)]
    caret_sdf_override: Option<[f32; 4]>,
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
    /// zorunlu: dock ızgaranın listesine girseydi ızgaranın kare ömrüne
    /// bağlanırdı ve kendi viewport'undan kopardı. Sayaçlara da girmiyor (`bg_count`,
    /// `glyph_count`, `rule_count`): `hucre=8 glif=6 kural=15` duman
    /// sözleşmesi dock'suz bir kabukta ölçülüyor ve anlamı bit bit korunmalı.
    dock_bg: Vec<Instance>,
    dock_glyphs: Vec<GlyphCell>,
    dock_rules: Vec<RuleCell>,
    /// Kümelerin tabloları (035 Karar 4B): listelerle **birlikte** yaşıyor ve
    /// temizleniyor, yani hareket karesi (listeleri koruyan) aynı kimlikleri
    /// aynı dizgilerle çiziyor. Üç tablo, çünkü üç yazar var: `frame()`
    /// ızgara ile doldurma bandını tek çağrıda dolduruyor (`clusters`),
    /// `dock()` ayrı bir çağrı (`dock_clusters`), hayaletler ise dock
    /// tablosunu aşan ömürlü efektlerden her karede yeniden kuruluyor
    /// (`fx_clusters`, [`Frame::set_dock_fx`]). Gelişler statik glyph'in
    /// kopyası, yani dock tablosunu okuyor.
    ///
    /// Doldurulurken tablo `Frame`'in dışında ([`Frame::take_clusters`]):
    /// sink'ler `Frame`'i ödünç aldığı için aynı çağrıya ikinci bir `&mut`
    /// veremezler (`fill`'in gerekçesi).
    clusters: Clusters,
    dock_clusters: Clusters,
    fx_clusters: Clusters,
    /// Dock'un yazım efektleri (030): silinen glyph'lerin hayaletleri ve
    /// gelen glyph'ler. İki liste, çünkü çizim sıraları ayrı — hayaletler
    /// dock glyph'lerinden **önce**, gelişler **sonra**
    /// ([`crate::Renderer`]'ın `encode_dock`'u).
    ///
    /// **İki yazarı var** ([`Frame::set_dock_fx`]): içerik karesi ve hareket
    /// karesi. Hareket karesi `clear` çağırmıyor ve dock'un statik listeleri
    /// korunuyor; değişen yalnız bu ikisi. Sayaçlara girmiyorlar.
    dock_ghosts: Vec<FxCell>,
    dock_arrivals: Vec<FxCell>,
    /// `heat`'in kızgın rengi: temanın `cursor` rolü, lineer. Efekt başına
    /// değil kare başına tek değer ve fragment'e uniform olarak gidiyor
    /// (`glyph_fx.metal` → `heat`); instance'ta yeri yok (`FxInstance`'ın
    /// `fx`'inde tek bir yedek `f32` var). Yazarı listelerle aynı
    /// ([`Frame::set_dock_fx`]), yani ikisi ayrışamıyor.
    dock_fx_heat: [f32; 4],
    /// Uçuştaki gelişlerin statik glyph'i **çıkarılmış** dock glyph'leri.
    ///
    /// `dock_glyphs`'in kendisi değişmiyor ve bu şart: hareket karesi dock'u
    /// yeniden basmıyor, yani efekti biten gelişin statik glyph'i geri
    /// gelebilmek için hâlâ orada olmalı. Uçuşta geliş yoksa bu liste
    /// okunmuyor ([`Frame::dock_glyphs`]).
    dock_shown: Vec<GlyphCell>,
    /// Üstteki boşluğu dolduran geçmiş satırları; ızgaranın `bg`'sinin
    /// **üçüncü** ikizi (`stripes` ve `dock_bg`'den sonra).
    ///
    /// Ayrılığın gerekçesi dock'unkiyle aynı ve bir ucu daha var: bandın
    /// kendi viewport'u var ([`Frame::fill_origin_px`]) ve satır numaraları
    /// **fill-yerel**, yani ızgaranınkilerle çakışıyor — tek listede ayırt
    /// edilemezlerdi ve ızgaranın uzayından çizilip bandın yerine içeriğin
    /// üstüne düşerlerdi. Sayaçlara da girmiyorlar (`bg_count`,
    /// `glyph_count`, `rule_count`): `hucre=8 glif=6 kural=15` duman
    /// sözleşmesi doldurması olmayan bir kabukta ölçülüyor ve anlamı bit bit
    /// korunmalı.
    fill_bg: Vec<Instance>,
    fill_glyphs: Vec<GlyphCell>,
    fill_rules: Vec<RuleCell>,
    /// Doldurma bandının arama vurgusu (033 Karar 8): bandın satırları
    /// gerçek geçmiş ve eşleşmeleri ızgaradakiler gibi vurgulanıyor —
    /// satırlar fill-yerel, bandın viewport'unda çiziliyor. Renkler
    /// ızgaranınkiyle aynı uniform. Bantta seçim çizimi yok (Karar 12).
    fill_search_match: Vec<Instance>,
    fill_search_current: Vec<Instance>,
    /// Doldurma bandının yüksekliği, **satır** (`bt_core::Cursor::fill`);
    /// sıfır → bant yok ve üçüncü viewport hiç kurulmuyor.
    ///
    /// Listelerden ayrı bir alan, `DockSurface` emsali: sıfırken listeler
    /// dolu olsa bile hiçbir şey çizilmiyor, yani "yarım açılmış doldurma"
    /// temsil edilemez. Bandın boyu sınırın kendi sayısı ve hücrelerden
    /// türetilmiyor — hücresi olmayan (bütünüyle boş) bir doldurma satırı da
    /// bandın içinde yer tutmak zorunda.
    fill_rows: u16,
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
    pub(crate) fn clear(&mut self, metrics: CellMetrics, caret: CaretStyle) {
        self.caret_style = caret;
        let cell_px = metrics.cell_px();
        self.stripes.clear();
        self.bg.clear();
        self.selection.clear();
        self.dock_selection.clear();
        self.search_match.clear();
        self.search_current.clear();
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
        self.clusters.clear();
        self.dock_clusters.clear();
        // Efektler de: dock'u olmayan bir karede (alternatif ekran) önceki
        // karenin hayaleti asılı kalmasın.
        self.dock_ghosts.clear();
        self.dock_arrivals.clear();
        self.dock_shown.clear();
        self.fill_bg.clear();
        self.fill_glyphs.clear();
        self.fill_rules.clear();
        self.fill_search_match.clear();
        self.fill_search_current.clear();
        // **Bant da her karede yeniden söyleniyor** ve sıfırlanması dock'un
        // yüzeyiyle aynı gerekçeyi taşıyor: korunsaydı doldurmayı kapatan ilk
        // karede (Ctrl-L, alternatif ekrana giriş, dock'u olmayan pencere)
        // önceki karenin bandı ızgaranın üstünde asılı kalırdı. Sıfır aynı
        // zamanda geri alma şeridinin kapısı — [`Renderer::encode_pass`]
        // üçüncü viewport'u hiç kurmuyor.
        self.fill_rows = 0;
        // Dikdörtgen de sıfırlanıyor ([`Frame::clear_caret`]): kalsaydı
        // imlecin sönmesi (`\e[?25l`) ya da geçmişe kayması bloğu ekrandan
        // kaldırır ama **altındaki metnin rengini** eski yerinde bırakırdı —
        // zemin renginde bir harf, yani görünmez bir hücre.
        self.clear_caret();
        self.cell_px = (f32::from(cell_px.0), f32::from(cell_px.1));
        self.context_cell_px = f32::from(metrics.context_cell_px());
        self.rule_px = f32::from(metrics.rule_px());
        self.gutter_px = f32::from(metrics.gutter_px());
        // **Sonsuz**, sıfır değil: sıfır "dock bandı pencerenin tepesinde"
        // demek olurdu ve her caret dock yuvasına düşerdi. Çağıran dock'lu
        // her karede üstüne yazıyor ([`Frame::set_dock_band`]) — hareket
        // karesi de, çünkü bant onun karesinde de ilerliyor ve caret'in yuva
        // kararı bandın o anki tepesine bakmalı.
        self.dock_top_px = f32::INFINITY;
        // Yerleşim tek giriş satırına, bant "söylenmedi"ye dönüyor: içerik
        // karesi ikisini de yeniden söylüyor, söylemeyen (dock'suz) kare
        // bandı ızgaraya hiç katmıyor.
        self.dock_rows = DOCK_ROWS;
        self.dock_context = true;
        self.dock_band = None;
        self.dock_bottom_px = 0.0;
        // Orijin **sıfırlanıyor**, geometriden gelmiyor: kaynağı bu karenin
        // doluluk sayısı ve o ancak sink döngüsü bitince biliniyor. Sıfırda
        // bırakmak "bu kare daha söylemedi" demek ve söylemeyen bir kare
        // bugünkü (tavana yapışık) yerleşimi çiziyor — sessiz bir yanlış
        // ötelemeden iyi.
        self.origin_px = 0.0;
        // Kesir de aynı sözleşmede: söylemeyen kare tam satırda çiziyor.
        self.frac_px = 0.0;
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
    ///
    /// **Kaydırmanın kesri ayrı yuvarlanıyor** ([`Frame::set_scroll_frac`]) ve
    /// [`Frame::origin_px`] ikisini topluyor: toplam tek yuvarlamadan en çok
    /// bir piksel ayrışıyor — o da yalnız öteleme kayarken kesir de sıfırdan
    /// büyükse, yani iki hareketin üst üste bindiği bir karede. Kazanç
    /// yerleşmiş hâlde: caret'in payı toplamdan değil kesrin kendisinden
    /// geliyor ve harfiyle bit bit aynı pikselde duruyor.
    pub(crate) fn set_origin_rows(&mut self, rows: f32) {
        debug_assert!(self.cell_px.1 > 0.0, "clear(metrics) çağrılmadı");
        self.origin_px = (rows * self.cell_px.1).round();
    }

    /// Bu karenin kaydırma kesri, **satır** (`[0, 1)`,
    /// `bt_core::Cursor::scroll_frac`): ızgara bu kadar aşağı çizilecek.
    ///
    /// Caret'ten **önce** çağrılıyor ([`Frame::push_caret`] kesri ızgaradaki
    /// caret'e ekliyor); ötelemeyle sırası serbest. Sıfır kesir kareyi
    /// bugünküyle bit bit aynı bırakıyor: yuvarlanmış sıfır, eklenen sıfır.
    ///
    /// Piksel **bir hücreden kısa** kırpılıyor: `[0, 1)` satır sözleşmesi
    /// yuvarlamadan sonra da geçerli kalmalı — tam hücre, ofseti değişmemiş
    /// bir ızgarayı bir satır aşağı çizer, tepe satırını tümden seçilemez
    /// kılar ve son satırın caret'ini dock'un altına gömerdi.
    pub(crate) fn set_scroll_frac(&mut self, frac: f32) {
        debug_assert!(self.cell_px.1 > 0.0, "clear(metrics) çağrılmadı");
        self.frac_px = (frac * self.cell_px.1)
            .round()
            .clamp(0.0, (self.cell_px.1 - 1.0).max(0.0));
    }

    /// Bu karenin dikey orijini, piksel; `setViewport`'un `originY`'si —
    /// öteleme artı kaydırmanın kesri, **eksi bandın fazlası** (032).
    ///
    /// Bandın çizilen boyu PTY payını aştıkça ızgara o kadar yukarı
    /// çiziliyor: dolu ızgaranın tepesi kırpılıyor, doldurma bandı ve fare
    /// eşlemesi de aynı değeri okuyor ([`Frame::dock_band`]). Bant yoksa ya da
    /// payın boyundaysa terim sıfır ve kare bugünküyle bit bit aynı.
    pub(crate) fn origin_px(&self) -> f32 {
        self.origin_px + self.frac_px - self.dock_band.unwrap_or(0.0)
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
            // Sayaç ile listenin **aynı** uzunlukta olması `bg`'ye imleç
            // gibi sayılmayan bir şeyin sızmadığının kanıtı; caret 012'den
            // beri kendi yuvasında ([`Frame::grid_caret`]) ve buraya hiç
            // girmiyor.
            debug_assert_eq!(
                self.bg.len(),
                self.bg_count,
                "`bg`'ye sayılmayan bir instance sızdı"
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
                // Izgara **her zaman** gösterim fontunda: küçük sınıfın tek
                // yeri dock'un bağlam satırı ([`Frame::push_dock`]).
                size: SizeClass::Normal,
                rgba: cell.fg.to_array(),
                wide: cell.wide,
                cluster: cell.cluster,
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

    /// Seçimin satır koşuları: koşu başına **tek** dörtgen, yuvarlak köşeli
    /// tek parça şeklin parçaları olarak (031 phase-3).
    ///
    /// Koşu başına bir instance, hücre başına değil: köprülenen boşlukların
    /// kendi hücresi yok (sink'e hiç uğramıyorlar) ve komşu dörtgenlerin
    /// dikişi kesirli hücre genişliğinde yarı saydam bir çizgi bırakabilirdi.
    ///
    /// **Dilimin tamamı birden**, koşu koşu değil: bir köşenin kararı komşu
    /// satırın koşusuna bağlı ([`selection_corners`]). Instance'ın `rgba`
    /// yuvası burada renk değil **köşe maskesi** — köşe başına `1` dışbükey,
    /// `0` kare; içbükey dolgu ayrı bir `r×r` instance ve maskesinde dairenin
    /// merkezi olan köşe `-1` (`selection_fragment`). Renk ile yarıçap kare
    /// başına tek ve uniform ([`Frame::selection_rgba`],
    /// [`Frame::selection_radius`]), caret'in hizalama kaçışıyla aynı yol.
    ///
    /// Renk odağa göre çağıranın seçimi (`bt_core::SelectionRuns::color`):
    /// odak `bt-core`'a girmiyor, burada da sorulmuyor.
    pub(crate) fn push_selection(&mut self, runs: &[SelectionRun], rgba: LinearRgba) {
        debug_assert!(self.cell_px.0 > 0.0, "clear(metrics) çağrılmadı");
        self.selection_rgba = rgba.to_array();
        let mut out = std::mem::take(&mut self.selection);
        self.selection_parts(runs, |frame, col, row| frame.pos(col, row), &mut out);
        self.selection = out;
    }

    /// Dock'un seçim koşuları: giriş bloğunun görsel satırı başına bir koşu
    /// (`bt_core::Session::dock`'un `runs`'ı; uzun satır sarılıyor, 032).
    /// Izgaranın şekliyle **aynı** yoldan ([`Frame::selection_parts`]) — köşe
    /// kararı komşu satırın koşusuna bakıyor, yani satırlar arası seçim tek
    /// parça bir şekil; konum dock-yerel ([`Frame::dock_pos`]). Renk ile
    /// yarıçap ızgaranınkiyle aynı uniform — pencerede tek seçim var ve rengi
    /// [`Frame::push_selection`] her içerik karesinde yazıyor.
    pub(crate) fn push_dock_selection(&mut self, runs: &[SelectionRun]) {
        debug_assert!(self.cell_px.0 > 0.0, "clear(metrics) çağrılmadı");
        let mut out = std::mem::take(&mut self.dock_selection);
        self.selection_parts(runs, |frame, col, row| frame.dock_pos(col, row), &mut out);
        self.dock_selection = out;
    }

    /// Seçim koşularının dörtgenleri ve içbükey dolguları, `pos`'un
    /// koordinat uzayında — iki yüzeyin (ızgara, dock) **tek** şekil
    /// kararı; ayrışan yalnız hücrenin yeri.
    fn selection_parts(
        &self,
        runs: &[SelectionRun],
        pos: impl Fn(&Self, u16, u16) -> [f32; 2],
        out: &mut Vec<Instance>,
    ) {
        let r = self.selection_radius();
        let (cw, ch) = self.cell_px;
        for (index, run) in runs.iter().enumerate() {
            debug_assert!(run.first <= run.last, "ters koşu: {run:?}");
            let corners = selection_corners(runs, index);
            let pos = pos(self, run.first, run.row);
            let width = (f32::from(run.last.saturating_sub(run.first)) + 1.0) * cw;
            out.push(Instance {
                pos,
                size: [width, ch],
                rgba: corners.map(|c| if c == Corner::Convex { 1.0 } else { 0.0 }),
            });
            if r <= 0.0 {
                continue;
            }
            let (x0, x1) = (pos[0], pos[0] + width);
            let (y0, y1) = (pos[1], pos[1] + ch);
            // Dolgu basamağın **dışında**, bu koşunun kendi satır bandında:
            // köşenin yanındaki `r×r` kare, dairenin merkezi o karenin
            // köşeden en uzak ucu. Sıra [`Corner`]'ınki (TL, TR, BR, BL);
            // ikinci sayı dolgu dörtgeninin merkezi taşıyan köşesi.
            let fills = [
                ([x0 - r, y0], 3),
                ([x1, y0], 2),
                ([x1, y1 - r], 1),
                ([x0 - r, y1 - r], 0),
            ];
            for (corner, (fill_pos, centre)) in corners.into_iter().zip(fills) {
                if corner != Corner::Concave {
                    continue;
                }
                let mut mask = [0.0; 4];
                mask[centre] = -1.0;
                out.push(Instance {
                    pos: fill_pos,
                    size: [r, r],
                    rgba: mask,
                });
            }
        }
    }

    /// Arama vurgusunun ızgaradaki koşuları (033): seçimin şekli
    /// ([`Frame::selection_parts`], `SELECTION_RADIUS`) ama köşeler
    /// **eşleşme başına** — [`selection_corners`] satır başına tek koşu ve
    /// dizi komşuluğu varsayıyor, arama ise bir satıra birden çok koşu
    /// koyuyor ve ardışık satırlardaki iki ayrı eşleşme tek şekle
    /// kaynamamalı (Karar 7). Sarılan tek eşleşme kaynıyor: onu
    /// `SearchRun::continues` söylüyor.
    ///
    /// Renkler odağa göre çağıranın seçimi (`bt_core::SearchRuns`), seçimin
    /// kuralı.
    pub(crate) fn push_search(
        &mut self,
        runs: &[SearchRun],
        matched: LinearRgba,
        current: LinearRgba,
    ) {
        debug_assert!(self.cell_px.0 > 0.0, "clear(metrics) çağrılmadı");
        self.search_match_rgba = matched.to_array();
        self.search_current_rgba = current.to_array();
        let mut lists = (
            std::mem::take(&mut self.search_match),
            std::mem::take(&mut self.search_current),
        );
        self.search_parts(runs, &mut lists);
        (self.search_match, self.search_current) = lists;
    }

    /// Doldurma bandının arama koşuları; satırlar fill-yerel ve bandın
    /// kendi viewport'unda çiziliyor ([`Frame::push_fill_block`] emsali —
    /// konum yine [`Frame::pos`], uzay viewport'un). Renkler
    /// [`Frame::push_search`]'ün yazdığı uniform: önce o çağrılmalı.
    pub(crate) fn push_fill_search(&mut self, runs: &[SearchRun]) {
        debug_assert!(
            runs.iter().all(|run| run.row < self.fill_rows),
            "doldurma araması bandın dışında: {runs:?} / {}",
            self.fill_rows
        );
        let mut lists = (
            std::mem::take(&mut self.fill_search_match),
            std::mem::take(&mut self.fill_search_current),
        );
        self.search_parts(runs, &mut lists);
        (self.fill_search_match, self.fill_search_current) = lists;
    }

    /// Koşuları eşleşmelere böler ve her eşleşmeyi kendi diliminde
    /// [`Frame::selection_parts`]'tan geçirir; geçerli eşleşme ikinci
    /// listeye. Eşleşme `continues` bitiyle bitişik koşular: `bt-core` bir
    /// eşleşmenin koşularını art arda veriyor ama sonraki eşleşme daha üst
    /// bir satırda başlayabiliyor (5–6'ya sarılan eşleşmeden sonra 5'teki
    /// ikincisi), yani liste **sıralanmıyor**, yalnız bölünüyor. Listenin ilk
    /// koşusu `continues` olsa da yeni bir eşleşme sayılıyor — başı öteki
    /// yüzeyde (bant) ya da ekranın dışında.
    fn search_parts(&mut self, runs: &[SearchRun], lists: &mut (Vec<Instance>, Vec<Instance>)) {
        let mut scratch = std::mem::take(&mut self.search_scratch);
        let mut start = 0;
        while start < runs.len() {
            let head = runs[start];
            // Devam koşusu bir önceki koşunun **hemen altındaki** satırda;
            // satırı tutmayan devam (yalnız bozuk girdide) şekli bölüyor,
            // kaynatmıyor.
            let end = runs
                .windows(2)
                .skip(start)
                .position(|pair| {
                    !pair[1].continues || Some(pair[1].row) != pair[0].row.checked_add(1)
                })
                .map_or(runs.len(), |i| start + 1 + i);
            scratch.clear();
            scratch.extend(runs[start..end].iter().map(|run| SelectionRun {
                row: run.row,
                first: run.first,
                last: run.last,
            }));
            let out = if head.current {
                &mut lists.1
            } else {
                &mut lists.0
            };
            self.selection_parts(&scratch, |frame, col, row| frame.pos(col, row), out);
            start = end;
        }
        self.search_scratch = scratch;
    }

    /// Bu karenin eşleşme vurgusu parçaları ([`Frame::push_search`]).
    pub(crate) fn search_match_instances(&self) -> &[Instance] {
        &self.search_match
    }

    /// Bu karenin geçerli eşleşme parçaları ([`Frame::push_search`]).
    pub(crate) fn search_current_instances(&self) -> &[Instance] {
        &self.search_current
    }

    /// Bandın eşleşme vurgusu parçaları, fill-yerel.
    pub(crate) fn fill_search_match_instances(&self) -> &[Instance] {
        &self.fill_search_match
    }

    /// Bandın geçerli eşleşme parçaları, fill-yerel.
    pub(crate) fn fill_search_current_instances(&self) -> &[Instance] {
        &self.fill_search_current
    }

    /// `search_match` rengi, lineer — `selection_fragment`'in renk uniform'u.
    pub(crate) fn search_match_rgba(&self) -> [f32; 4] {
        self.search_match_rgba
    }

    /// `search_current` rengi, lineer.
    pub(crate) fn search_current_rgba(&self) -> [f32; 4] {
        self.search_current_rgba
    }

    /// Bu karenin seçim parçaları; [`Frame::push_selection`]'ın dörtgenleri.
    pub(crate) fn selection_instances(&self) -> &[Instance] {
        &self.selection
    }

    /// Dock'un seçim parçaları, dock-yerel ([`Frame::push_dock_selection`]).
    pub(crate) fn dock_selection_instances(&self) -> &[Instance] {
        &self.dock_selection
    }

    /// Seçimin rengi, lineer — `selection_fragment`'in renk uniform'u.
    pub(crate) fn selection_rgba(&self) -> [f32; 4] {
        self.selection_rgba
    }

    /// Seçimin köşe yarıçapı, piksel: seçimin kendi oranı
    /// ([`SELECTION_RADIUS`]), kullanıcının `cursor_radius`'u değil (Karar
    /// 10 — anahtar imlecin). Kırpması [`caret_radius_px`]'ten, yani tek
    /// hücrelik koşuda yarım genişlik.
    pub(crate) fn selection_radius(&self) -> f32 {
        caret_radius_px(self.cell_px, SELECTION_RADIUS)
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

    /// Doldurma bandının blok işareti — [`Frame::push_block`]'un band ikizi.
    ///
    /// Ayrı bir çağrı, çünkü ayrı bir **koordinat uzayı**: satır fill-yerel
    /// (`0..fill`) ve liste bandın kendi `setViewport`'unda çiziliyor
    /// (`fill_rules`, üçüncü yüzey). Izgaranın `stripes`'ına yazsaydı işaret
    /// bandın gösterdiği satırın değil, aynı numaralı ızgara satırının
    /// yanında belirirdi.
    ///
    /// Şekil, sütun ve renk kaynağı ızgarayla **aynı**: aynı chevron sprite'ı,
    /// 0. sütun, `bt-core`'un verdiği safha rengi. İkinci bir tasarım kararı
    /// yok — bandın kazandığı şey yalnız listeye erişim.
    pub(crate) fn push_fill_block(&mut self, block: Block) {
        debug_assert!(
            block.row < self.fill_rows,
            "doldurma işareti bandın dışında: {} / {}",
            block.row,
            self.fill_rows
        );
        self.fill_rules.push(RuleCell {
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
        focused: bool,
    ) {
        // **İçi boşalma yalnız bloğa.** Alt çizgi ve dikey çubuk zaten birer
        // ince şerit; onların "içi boş" hâli bir pikselin çerçevesi, yani
        // hiçbir şey — üstelik kenar kalınlığı `rule_px` ve şeridin kendi
        // kalınlığı da o, yani çıkarma gövdeyi tümden yutardı. O şekillerde
        // odaksızlığın sinyali blink'in durması.
        //
        // **Üçüncü terim kullanıcının** (`[terminal] cursor_unfocused`):
        // `"solid"` içi boşalmayı kapatıyor ve blink'e **dokunmuyor** — odakta
        // blink'in durması 015'in ayrı kararı, ikisi ayrı sinyal.
        let hollow = !focused
            && matches!(shape, CaretShape::Block)
            && self.caret_style.unfocused == UnfocusedCaret::Hollow;
        self.caret_hollow = hollow;
        let mut pos = self.pos_at(at);
        // **Yuva kaydırmadan önce seçiliyor** ve kesir ızgaranın yuvasındaki
        // caret'e ekleniyor ([`Frame::frac_px`]): ızgara kesir kadar aşağıda,
        // caret onun harfinin üstünde durmak zorunda. Dock yuvası kaydırmadan
        // muaf — dock ayrı bir yüzey ve orada duran caret kaydırılan ekranın
        // parçası değil. Seçim kaydırılmamış konuma bakıyor, çünkü kesir bir
        // devir değil: ızgaranın son satırındaki caret'i dock'un yuvasına
        // itseydi dock'un zemininin **altında** değil üstünde, harfinden
        // kopuk çizilirdi — oysa ızgaranın taşan harfini de dock örtüyor.
        //
        // **Bilinen sınır:** ızgaradan dock'a devir kaymasında eşiği geçen
        // karede kesir düşüyor, yani caret o karede kesir kadar sıçrıyor.
        // Kesir yalnız süren bir jestte sıfırdan büyük ve devir komutun
        // bitişinde, yani ikisinin aynı kareye denk gelmesi dar; kapatmak
        // kesri caret'in animatörüne taşımak demek.
        //
        // **Yarım pikselin altındaki örtüşme devir değil** (036): uzak
        // oturumda bandın fazlası kesirli (`(band_px − dock_px) / cell_h`) ve
        // `f32`'de tam temsil edilmiyor; son satırdaki caret'in alt kenarı
        // bandın tepesini bir epsilon aşıp dock yuvasına geçer, harfini
        // boyar ve kesri kaybederdi. Gerçek örtüşme en az bir piksel.
        let in_dock = pos[1] + self.cell_px.1 > self.dock_top_px + 0.5;
        if !in_dock {
            pos[1] += self.frac_px;
        }
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
        let rects = caret_rect(pos, self.cell_px, shape, self.rule_px, hollow);
        let (opaque_pos, opaque_size) = rects.opaque;
        let mut bottom = opaque_pos[1] + opaque_size[1];
        // **Izgaranın caret'i dock bandında ters çevirmiyor.** Kesirle kayan
        // son satırın caret'i banda girebiliyor ve orada harfiyle birlikte
        // dock'un opak zemininin altında kalıyor; dikdörtgen ise iki glyph
        // encode'una da gidiyor ve kırpılmasaydı dock'un o sütundaki harfleri
        // zemin renginde, yani görünmez çizilirdi. Dock'suz karede sınır
        // sonsuz ve kırpma kimlik.
        if !in_dock {
            bottom = bottom.min(self.dock_top_px);
        }
        self.cursor = CursorBlock {
            rect: [
                opaque_pos[0],
                opaque_pos[1],
                opaque_pos[0] + opaque_size[0],
                bottom.max(opaque_pos[1]),
            ],
            rgba: with_alpha(text, alpha),
        };
        // **Boyanan dikdörtgen ayrı tutuluyor**, `CursorBlock`'unkinden
        // türetilmiyor: bugün eşitler ama içi boş caret'te (phase-3, R5) ters
        // çevirme alanı boşalırken boyanan alan duruyor. Fragment SDF'inin
        // çekirdeği bu; ekran uzayında, yani `[[position]]` ile aynı uzayda.
        let (painted_pos, painted_size) = rects.painted;
        self.caret_core = [
            painted_pos[0],
            painted_pos[1],
            painted_pos[0] + painted_size[0],
            painted_pos[1] + painted_size[1],
        ];
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
        if in_dock {
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
        focused: bool,
    ) {
        // **Şekil korunuyor, odak korunmuyor** ve ayrım kaynakta: şekil
        // `bt-core`'dan geliyor ve bu yolun ona erişimi yok, odak ise
        // `bt-gpu`'nun kendi biti (`DisplayLink::set_focused`) ve her karede
        // okunabiliyor. Saklanmış bir kopyayı korumak onu **bayatlatırdı**:
        // odak dönerken uçuşta bir animasyon varsa caret içi boş çizilmeye
        // devam eder, ancak bir sonraki **içerik** karesinde dolardı —
        // kullanıcı bunu "çerçeve duruyor, içi sonradan doluyor" diye gördü
        // (2026-09-20).
        let shape = self.caret_shape;
        self.clear_caret();
        self.push_caret(at, text, rgba, alpha, shape, focused);
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
        self.caret_core = [0.0; 4];
    }

    /// SDF uniform'unu ezer — yalnız sınama; sınırı alanın doc'unda.
    #[cfg(test)]
    pub(crate) fn force_caret_sdf(&mut self, shape: [f32; 4]) {
        self.caret_sdf_override = Some(shape);
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
        if let Some(glyph) = self.dock_glyph(cell) {
            self.dock_glyphs.push(glyph);
        }
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

    /// Dock hücresinin glyph'i — [`Frame::push_dock`] ile yazım efektlerinin
    /// ([`Frame::set_dock_fx`]) **ortak** çevirisi.
    ///
    /// Tek yer, çünkü efektin `t = 1`'deki çizimi statik glyph'le piksel
    /// piksel aynı olmak zorunda (`plan.md` → R5): konum, yüz, boy sınıfı ya
    /// da renk iki yerde hesaplansaydı devir karesinde harf bir an sıçrardı.
    fn dock_glyph(&self, cell: Cell) -> Option<GlyphCell> {
        cell.ch.map(|ch| GlyphCell {
            pos: self.dock_pos(cell.col, cell.row),
            ch,
            face: face(cell.bold, cell.italic),
            // Konumla **aynı eşik** ([`Frame::column_px`]): ayrışsalardı
            // harf bir ölçüde, adımı başka ölçüde olurdu.
            size: if self.is_context_row(cell.row) {
                SizeClass::Small
            } else {
                SizeClass::Normal
            },
            // **Sınır burada artık `true` de verebiliyor** (024): dock'un
            // sütunu karakter indeksinden değil genişlikten birikiyor,
            // yani geniş karakterin baş hücresi işaretli geliyor ve
            // spacer sütununa glyph'siz bir zemin hücresi düşüyor.
            // 023'te bu satırın yorumu "sınır her zaman `false` veriyor"
            // diyordu ve alanın hücreden okunmasının gerekçesi de tam
            // buydu — "`bt-core` bir gün onu kaldırsa bu satır sessizce
            // eski kalırdı". Kaldırdı; satır sessizce eski kalmadı,
            // çünkü sabit yazılmamıştı.
            wide: cell.wide,
            rgba: cell.fg.to_array(),
            // Bağlam satırı (küçük sınıf) küme taşımıyor (Karar 6): sınır
            // oraya kümeli hücre basmıyor.
            cluster: cell.cluster,
        })
    }

    /// Uçuştaki gelişlerden **statik glyph'i bulunamayanları** bitirir —
    /// içerik karesinde, dock basıldıktan sonra.
    ///
    /// Eşleşme konum (satır ve sütun; aynı [`Frame::dock_pos`]'tan geçen
    /// piksel, yani tam eşitlik) ve karakter. Bulunamayan geliş ya pencerenin
    /// dışına düştü ya da o karakter artık orada değil — sarılan girişte
    /// düzenlemenin arkasında kayan harf de buradan bitiyor (032 phase-6);
    /// çizilseydi satırda olmayan bir harf belirirdi. Hayaletler sorulmuyor: onların statik glyph'i zaten yok.
    pub(crate) fn suppress_dock(&self, fx: &mut GlyphFx) {
        fx.retain(|fx| fx.kind == Kind::Ghost || self.static_arrival(fx).is_some());
    }

    /// Gelişin statik glyph'i, `dock_glyphs`'te.
    fn static_arrival(&self, fx: &Fx) -> Option<usize> {
        let pos = self.dock_pos(fx.cell.col, fx.cell.row);
        self.dock_glyphs
            .iter()
            .position(|glyph| glyph.pos == pos && Some(glyph.ch) == fx.cell.ch)
    }

    /// Yazım efektlerini bu kare için yazar; içerik karesi de hareket karesi
    /// de buradan geçiyor (dock'un statik listeleri hareket karesinde
    /// korunuyor, yalnız efektler ilerliyor).
    ///
    /// Uçuştaki gelişin statik glyph'i çizilecek listeden **çıkarılıyor**
    /// (`dock_shown`), yoksa `fade` statik glyph'in üstünde belirir ve hiçbir
    /// şey görünmezdi. `dock_glyphs`'e dokunulmuyor: efekt bitince statik
    /// glyph, dock yeniden basılmadan geri gelmeli.
    ///
    /// `heat` temanın `cursor` rengi (`heat` efektinin kızgın rengi); iki
    /// yazar da temayı elinde tutuyor.
    ///
    /// `table` efektlerin küme tablosu ([`GlyphFx::clusters`]); hayaletlerin
    /// kümeleri [`Frame::fx_clusters`]'a kopyalanıyor, gelişler statik
    /// glyph'in kopyası olduğu için dock'un tablosunda.
    pub(crate) fn set_dock_fx(
        &mut self,
        fx: impl IntoIterator<Item = Fx>,
        table: &Clusters,
        heat: LinearRgba,
    ) {
        self.dock_fx_heat = heat.to_array();
        self.dock_ghosts.clear();
        self.fx_clusters.clear();
        self.dock_arrivals.clear();
        self.dock_shown.clear();
        let mut hidden = [usize::MAX; crate::glyph_fx::FX_MAX];
        let mut hidden_len = 0;
        for fx in fx {
            let fx_cell = |glyph| FxCell {
                glyph,
                t: fx.t,
                effect: fx.effect,
                seed: fx.seed,
            };
            match fx.kind {
                Kind::Ghost => {
                    if let Some(glyph) = self.dock_glyph(fx.cell) {
                        let cluster = copy_cluster(glyph.cluster, table, &mut self.fx_clusters);
                        self.dock_ghosts
                            .push(fx_cell(GlyphCell { cluster, ..glyph }));
                    }
                }
                Kind::Arrival => {
                    // Statik glyph'i yoksa geliş çizilmiyor (bkz.
                    // [`Frame::suppress_dock`]; içerik karesi onu zaten
                    // bitirdi, burası hareket karesinin savunması).
                    let Some(index) = self.static_arrival(&fx) else {
                        continue;
                    };
                    if let Some(slot) = hidden.get_mut(hidden_len) {
                        *slot = index;
                        hidden_len += 1;
                    }
                    // **Glyph gizlenen statik glyph'in kendisi**, yazıldığı
                    // anın hücresi değil: vurgu sonradan değişebiliyor
                    // (`zsh-syntax-highlighting` `l`'yi kırmızı, `ls`'i yeşil
                    // boyuyor) ve efekt eski renkle bitip yeniye sıçrardı —
                    // `t = 1` eşitliği bozulurdu.
                    self.dock_arrivals.push(fx_cell(self.dock_glyphs[index]));
                }
            }
        }
        if hidden_len > 0 {
            let hidden = &hidden[..hidden_len];
            self.dock_shown.extend(
                self.dock_glyphs
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| !hidden.contains(index))
                    .map(|(_, glyph)| *glyph),
            );
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

    /// Dock'un bu karedeki **yerleşim** satırları, bağlam satırı dahil
    /// ([`Frame::dock_rows`]); hücrelerden **önce** çağrılıyor, çünkü
    /// hücrenin yeri ona bağlı.
    ///
    /// Bağlam satırının varlığı satır sayısından: iki ve fazlası → son satır
    /// bağlam, tek satır → bağlamsız bir giriş satırı. Üretim yolu
    /// [`Frame::set_dock_input_rows`]; bu hâl sınamaların tek satırlık
    /// dock'u için duruyor — yalnız sınamada derleniyor.
    #[cfg(test)]
    pub(crate) fn set_dock_rows(&mut self, rows: u16) {
        self.dock_rows = rows;
        self.dock_context = rows >= 2;
    }

    /// Kare yolunun yerleşimi: `input_rows` giriş satırı (`bt_core::Cursor::input_rows`)
    /// ve altında **her zaman** bağlam satırı.
    ///
    /// Sıfır giriş satırı meşru (036 Karar 8, uzak oturum): yerleşim yalnız
    /// bağlam satırı, küçük yüzle ve üstünde satır arası boşluk olmadan —
    /// ayrılacak bir giriş satırı yok.
    pub(crate) fn set_dock_input_rows(&mut self, input_rows: u16) {
        self.dock_rows = input_rows.saturating_add(1);
        self.dock_context = true;
    }

    /// Bandın bu karede **çizilen** boyu: pencerenin dibi ve PTY payından
    /// fazlası, satır (`crate::motion::Motion::band`, kesirli).
    ///
    /// İki kare yolu da çağırıyor ([`Frame::dock_band`]); caret'in yuvasını
    /// seçen bandın tepesi de ([`Frame::dock_top_px`]) aynı çağrıda yazılıyor —
    /// ayrı yazılsalardı caret bir yüzeyin tepesine, bant başka bir tepeye
    /// bakabilirdi.
    ///
    /// Fazlanın pikseli **aygıt ızgarasına yuvarlanıyor** (`set_origin_rows`'un
    /// gerekçesi): ızgaranın orijini de aynı yuvarlanmış sayıyı çıkarıyor.
    pub(crate) fn set_dock_band(&mut self, bottom_px: f32, extra_rows: f32) {
        debug_assert!(self.cell_px.1 > 0.0, "clear(metrics) çağrılmadı");
        self.dock_band = Some((extra_rows * self.cell_px.1).round());
        self.dock_bottom_px = bottom_px;
        self.dock_top_px = bottom_px - self.band_height();
    }

    /// Dock yüzeyini bu kare için açar: zemini ve iki saç çizgisinin rengi.
    ///
    /// Hücrelerden **sonra** çağrılıyor ve bu bir sıra tercihi değil zorunluk:
    /// renkler `bt-core`'un dock çağrısından dönüyor ve o çağrı hücreleri
    /// sink'e basarken doğuruyor onları. `Frame` bu yüzden yüzeyi hücrelerden
    /// bağımsız tutuyor — listeler doluyken `dock` hâlâ `None` olabilir ve o
    /// hâlde hiçbir şey çizilmez, yani "yarım açılmış dock" temsil edilemez.
    pub(crate) fn open_dock(
        &mut self,
        ground: LinearRgba,
        edge: LinearRgba,
        separator: LinearRgba,
    ) {
        self.dock = Some(DockSurface {
            ground: ground.to_array(),
            edge: edge.to_array(),
            separator: separator.to_array(),
            progress: None,
            buttons: [None; 2],
        });
    }

    /// Yükleme satırının düğmeleri (`bt_core::Dock::buttons`, 037 phase-6):
    /// [`Frame::open_dock`]'tan sonra; dock açık değilse no-op.
    pub(crate) fn set_dock_buttons(&mut self, buttons: [Option<DockButton>; 2]) {
        if let Some(dock) = &mut self.dock {
            dock.buttons = buttons;
        }
    }

    /// Yükleme düğmelerinin çizimleri: düğme başına **dolgu, sonra çerçeve**
    /// — ikisi de `caret_fragment`'ten (yuvarlak dikdörtgenin SDF'i ve kenar
    /// bandı zaten orada; yeni bir pipeline ya da shader yok). Dörtlü
    /// dock-yerel, çekirdek pencere uzayında (`origin_y` kadar aşağıda):
    /// fragment onu `[[position]]` ile karşılaştırıyor ([`Frame::dock_caret`]'in
    /// tersi yönde aynı çeviri).
    ///
    /// Dikdörtgen bağlam satırının hücre bandı: yatayda düğmenin sütun
    /// aralığı ([`Frame::dock_pos`], küçük sınıfın adımı), dikeyde satırın
    /// yüksekliği. **Tıklama alanının ta kendisi** — fare aynı sütun
    /// aralığını `bt_core::transfer_button_at`'ten okuyor. Kenarlar aygıt
    /// pikseline yuvarlanıyor: çerçeve saç çizgisi kalınlığında ve
    /// oturmayan kenar iki piksele yayılıp soluklaşırdı.
    ///
    /// Yarıçap seçiminki ([`Frame::selection_radius`]), kalınlık fontun kendi
    /// kural metriği — ikinci bir sayı uydurulmadı.
    pub(crate) fn dock_button_draws(
        &self,
        origin_y: f32,
    ) -> impl Iterator<Item = RoundedDraw> + '_ {
        let buttons = self.dock.map_or([None; 2], |dock| dock.buttons);
        let row = self.dock_rows.saturating_sub(1);
        let radius = self.selection_radius();
        let stroke = self.rule_px.max(1.0);
        buttons.into_iter().flatten().flat_map(move |button| {
            let [x0, y0] = self.dock_pos(button.start, row);
            let [x1, _] = self.dock_pos(button.end, row);
            let (x0, x1) = (x0.round(), x1.round());
            let (y0, y1) = (y0.round(), (y0 + self.cell_px.1).round());
            let (fill, edge) = button_alpha(button.state);
            let instance = |alpha: f32| Instance {
                pos: [x0, y0],
                size: [x1 - x0, y1 - y0],
                rgba: with_alpha(button.color, alpha),
            };
            let core = [x0, y0 + origin_y, x1, y1 + origin_y];
            [
                RoundedDraw {
                    instance: instance(fill),
                    core,
                    shape: [radius, 0.0, 0.0, 0.0],
                },
                RoundedDraw {
                    instance: instance(edge),
                    core,
                    shape: [radius, stroke, 0.0, 0.0],
                },
            ]
        })
    }

    /// Üst saç çizgisini bu kare için bir **ilerleme çubuğuna** çevirir
    /// (`bt_core::Dock::progress`, onbinde; 037 Karar 7): dolan kısım
    /// `edge`'in, kalanı `separator`'ın renginde. [`Frame::open_dock`]'tan
    /// sonra; açılış her karede `None`'a sıfırlıyor, yani çubuk yalnız
    /// söylendiği karede var. Dock açık değilse no-op.
    pub(crate) fn set_dock_progress(&mut self, progress: Option<u16>) {
        if let Some(dock) = &mut self.dock {
            dock.progress = progress.map(|p| f32::from(p.min(10_000)) / 10_000.0);
        }
    }

    /// Bu karenin dock yüzeyi; `None` → dock yok, ikinci viewport kurulmaz.
    pub(crate) fn dock(&self) -> Option<DockSurface> {
        self.dock
    }

    /// Dock'un **yerleşiminin** yüksekliği, piksel: hücrelerin viewport'unun
    /// orijinini ve caret'in kaymasını veren sayı. Dock yoksa sıfır.
    ///
    /// Bandın çizilen boyu ([`Frame::dock_band_px`]) animasyon boyunca bundan
    /// ayrışıyor; hücreler dibe yaslı olduğu için (bağlam satırı dipte)
    /// ayrışma yalnız zeminin ve üst saç çizgisinin yerini oynatıyor.
    pub(crate) fn dock_layout_px(&self) -> f32 {
        if self.dock.is_none() {
            return 0.0;
        }
        dock_height(self.dock_rows, self.cell_px.1, self.gutter_px)
    }

    /// Bandın bu karede **çizilen** yüksekliği, piksel: zeminin ve üst saç
    /// çizgisinin viewport'unu veren sayı. Dock yoksa sıfır; bant
    /// söylenmediyse yerleşimin boyu.
    pub(crate) fn dock_band_px(&self) -> f32 {
        if self.dock.is_none() {
            return 0.0;
        }
        self.band_height()
    }

    /// [`Frame::dock_band_px`]'in dock'tan bağımsız gövdesi: bant söylendiyse
    /// PTY payı artı fazlası, söylenmediyse yerleşimin boyu.
    fn band_height(&self) -> f32 {
        match self.dock_band {
            Some(extra) => dock_height(DOCK_ROWS, self.cell_px.1, self.gutter_px) + extra,
            None => dock_height(self.dock_rows, self.cell_px.1, self.gutter_px),
        }
    }

    /// Fare eşlemesinin dock geometrisi: giriş bloğunun tepesi (pencere
    /// uzayında piksel, üstten) ve giriş satırı sayısı. Dock yoksa ya da bu
    /// kare pencerenin dibini söylemediyse `None`.
    ///
    /// Giriş satırı yoksa (uzak oturum, 036) satır sayısı **sıfır**, `None`
    /// değil: `None` fare tarafında "henüz kare yok"un tek satırlık geri
    /// düşüşüne gider ve bağlam satırında hayalet bir giriş bloğu doğardı.
    ///
    /// **Yerleşimden**, çizilen banttan değil: metin dibe yaslı ve animasyon
    /// boyunca yerinde duruyor, yani tıklanan harf yerleşimin harfi.
    pub(crate) fn dock_hit(&self) -> Option<(f32, u16)> {
        self.dock_band?;
        self.dock?;
        Some((
            self.dock_bottom_px - self.dock_layout_px() + self.dock_pad(),
            self.dock_rows - u16::from(self.dock_context),
        ))
    }

    /// Bu satır bağlam satırı mı: yerleşimin **son** satırı, iki ve fazla
    /// satırlık dock'ta. Punto sınıfının, sütun adımının ve satır arası
    /// boşluğun tek karar noktası — ayrı eşiklere baksalardı harf bir ölçüde,
    /// adımı başka ölçüde olurdu.
    ///
    /// Satır numarasına bakılıyor, sınırdan bir bayrak geçmiyor: "hangi satır
    /// küçük" çizimin kararı, `bt-core` hücreyi verir (bağlam satırını giriş
    /// bloğunun altına, `input_rows`. satıra koyarak), punto sınıfını bu
    /// katman seçer.
    fn is_context_row(&self, row: u16) -> bool {
        self.dock_context && row.saturating_add(1) == self.dock_rows
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
    ///
    /// **Bandın uzayında** (032): üçü de çizilen bandın viewport'undan
    /// çiziliyor (`Renderer::encode_dock`, `yükseklik − bant`), yani zemin ve
    /// üst saç çizgisi animasyonla birlikte yükselip iniyor; ikinci saç
    /// çizgisi ise **dibe yaslı** — bağlam satırının üstündeki boşlukta
    /// kalıyor ve yeri bandın boyu ile yerleşiminki arasındaki farktan.
    pub(crate) fn dock_ground(&self, width_px: f32) -> [Instance; 4] {
        let dock = self.dock.unwrap_or(DockSurface {
            ground: [0.0; 4],
            edge: [0.0; 4],
            separator: [0.0; 4],
            progress: None,
            buttons: [None; 2],
        });
        // İlerleme varken çizginin zemini ayracın rengi ve üstüne dolan kısım
        // kenarın renginde; yokken zemin kenarın kendisi ve dolgu sıfır
        // genişlik (dizinin boyu sabit, çağıran dallanmasın).
        let (edge_base, fill) = match dock.progress {
            Some(p) => (dock.separator, width_px * p.clamp(0.0, 1.0)),
            None => (dock.edge, 0.0),
        };
        let band = self.dock_band_px();
        let rows = if self.dock.is_some() {
            self.dock_rows
        } else {
            0
        };
        [
            Instance {
                // Zemin **paylar dahil** bütün yüzeyi kaplıyor: pay kadar
                // eksik bir dikdörtgen, kayma boyunca taşan ızgara satırını
                // tam da nefes payının olduğu yerde gösterirdi.
                pos: [0.0, 0.0],
                size: [width_px, band],
                rgba: dock.ground,
            },
            // Ayraç zeminin **üstünde** ve dock'un en üst pikselinde: ızgara
            // ile dock arasındaki sınır orası. Rengi kendi alanından: uzak
            // oturumda yüzeyin kenarı uzaklığı söylüyor (036), bölme değil.
            Instance {
                pos: [0.0, 0.0],
                size: [width_px, SEPARATOR_PX],
                rgba: edge_base,
            },
            // Yükleme sürerken (037 Karar 7) çizginin soldan dolan kısmı:
            // bütün kuyruğun baytlarına göre, host'un renginde. Yuvarlanıyor
            // (saç çizgisinin gerekçesi: aygıt ızgarasına oturmayan kenar
            // soluklaşırdı).
            Instance {
                pos: [0.0, 0.0],
                size: [fill.round(), SEPARATOR_PX],
                rgba: dock.edge,
            },
            // **İkinci ayraç: giriş satırı ile bağlam satırı arasında.** Yerelde
            // üsttekiyle aynı renk ve aynı kalınlık, çünkü aynı şeyi söylüyor —
            // "bunlar ayrı iki yüzey"; uzak oturumun rengini almıyor. phase-9 araya boşluk koymuştu; boşluk ayrımı
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
                pos: [
                    0.0,
                    band - self.dock_layout_px() + self.dock_row_divider_y(rows),
                ],
                size: [width_px, if rows < 2 { 0.0 } else { SEPARATOR_PX }],
                rgba: dock.separator,
            },
        ]
    }

    /// Giriş bloğunu bağlam satırından ayıran çizginin **üst** kenarı,
    /// yerleşimin uzayında piksel.
    ///
    /// Satırların yerleşimi [`Frame::dock_pos`]'ta: giriş satırları
    /// `pad + r·cell_h`'de bitişik, bağlam satırı boşluğun altında — yani
    /// boşluk `pad + (rows − 1)·cell_h` ile onun `gap` fazlası arasında. Çizgi
    /// o aralığın ortasına oturuyor.
    fn dock_row_divider_y(&self, rows: u16) -> f32 {
        if rows < 2 {
            return 0.0;
        }
        let pad = self.dock_pad();
        let gap = dock_row_gap(pad);
        // Yuvarlanıyor: aygıt ızgarasına oturmayan bir saç çizgisi iki piksele
        // yayılıp soluklaşırdı — `SEPARATOR_PX`'in ölçekle çarpılmama
        // gerekçesiyle aynı yerden.
        (pad + f32::from(rows - 1) * self.cell_px.1 + (gap - SEPARATOR_PX) * 0.5).round()
    }

    pub(crate) fn dock_bg(&self) -> &[Instance] {
        &self.dock_bg
    }

    /// Dock'un çizilecek glyph'leri: uçuşta geliş varsa onların statik
    /// glyph'i çıkarılmış hâli ([`Frame::set_dock_fx`]).
    pub(crate) fn dock_glyphs(&self) -> &[GlyphCell] {
        if self.dock_arrivals.is_empty() {
            &self.dock_glyphs
        } else {
            &self.dock_shown
        }
    }

    /// Silinen glyph'lerin hayaletleri; dock glyph'lerinden **önce** çiziliyor.
    pub(crate) fn dock_ghosts(&self) -> &[FxCell] {
        &self.dock_ghosts
    }

    /// Gelen glyph'ler; dock glyph'lerinden **sonra** çiziliyor.
    pub(crate) fn dock_arrivals(&self) -> &[FxCell] {
        &self.dock_arrivals
    }

    /// `heat` efektinin kızgın rengi ([`Frame::set_dock_fx`]).
    pub(crate) fn dock_fx_heat(&self) -> &[f32; 4] {
        &self.dock_fx_heat
    }

    pub(crate) fn dock_rules(&self) -> &[RuleCell] {
        &self.dock_rules
    }

    /// Caret'in ızgara yuvası; `None` → caret bu karede ızgarada değil.
    pub(crate) fn grid_caret(&self) -> Option<Instance> {
        self.grid_caret.map(|caret| {
            let mut instance =
                caret.instance(self.cell_px, self.caret_shape, self.rule_px, self.glow_px());
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
            instance.pos[1] -= self.origin_px();
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
            let mut instance =
                caret.instance(self.cell_px, self.caret_shape, self.rule_px, self.glow_px());
            instance.pos[1] -= origin_y;
            instance
        })
    }

    /// Dock bandının tepesini doğrudan yazar — **sınamaların kısayolu**.
    /// Üretimde tepe bandın boyuyla aynı çağrıda yazılıyor
    /// ([`Frame::set_dock_band`]) ve ikisi ayrışamıyor.
    #[cfg(test)]
    pub(crate) fn set_dock_top(&mut self, top_px: f32) {
        self.dock_top_px = top_px;
    }

    /// Doldurma **kanalının** bu karedeki yüksekliği, satır: bandın
    /// (`bt_core::Cursor::fill`) artı kesrin tepe satırı
    /// (`bt_core::Cursor::top_row`), yani `top_row + fill`.
    ///
    /// Tepe satırı ayrı bir yüzey değil, kanalın en üst satırı: fill-yerel
    /// `0`, bandın satırları onun altında. Bandın orijini
    /// ([`Frame::fill_origin_px`]) boydan türüyor, yani tepe satırı bandla ve
    /// ızgarayla birlikte kayıyor ve ikinci bir aritmetik doğmuyor.
    ///
    /// Hücrelerden **önce** çağrılıyor ve bu dock'un tersi bir sıra: dock'un
    /// renkleri hücreleri basan çağrıdan dönüyor ([`Frame::open_dock`]), bandın
    /// boyu ise `frame()`'in dönüşünde hazır ve [`Frame::push_fill`]'in
    /// bekçisi onu okuyor. Ters sırada bekçi her karede kendi sıfırına bakardı.
    pub(crate) fn set_fill_rows(&mut self, rows: u16) {
        self.fill_rows = rows;
    }

    /// Doldurma kanalının yüksekliği, satır (tepe satırı dahil); sıfır →
    /// kanal yok.
    pub(crate) fn fill_rows(&self) -> u16 {
        self.fill_rows
    }

    /// Doldurulan bir satırın hücresi; [`Frame::push`]'un ikizi ama
    /// **fill-yerel** satırda ve sayaçlara girmeden.
    ///
    /// **Geometri ızgaranın ta kendisi** ([`Frame::pos`]), dock'unki gibi kendi
    /// payı olan ayrı bir yüzey değil: doldurulan satırlar ızgaranın
    /// satırları, yalnız ötelemenin **üstünde** duruyorlar. Ayrı bir aritmetik
    /// yazılsaydı bant ile içeriğin sütunları ayrışabilirdi.
    ///
    /// **Konum push anında pişmiyor** (R3.1) ve pişemezdi: hareket karesi
    /// listeleri koruyup yalnız `origin_px`'i yeniden yazıyor
    /// (`LinkDelegate::set_origin`), yani pişmiş bir konum kaymanın her
    /// karesinde bayatlardı — ızgara süzülürken doldurma yerinde donardı.
    /// Bandı ekrana taşıyan şey üçüncü `setViewport`
    /// ([`Frame::fill_origin_px`]) ve o, orijini **encode anında** okuyor.
    pub(crate) fn push_fill(&mut self, cell: Cell) {
        // Satır fill-yerel (`0..fill`): sınır "hangi satırlar" der, "nereye"
        // demez. Bandın boyu bu çağrıdan önce yazılmak zorunda, yoksa
        // ekrana çıkmayacak bir hücre sessizce listeye girerdi.
        debug_assert!(
            cell.row < self.fill_rows,
            "doldurma satırı bandın dışında: {} / {}",
            cell.row,
            self.fill_rows
        );
        let pos = self.pos(cell.col, cell.row);
        if let Some(bg) = cell.bg {
            self.fill_bg.push(Instance {
                pos,
                size: [self.cell_px.0, self.cell_px.1],
                rgba: bg.to_array(),
            });
        }
        if let Some(ch) = cell.ch {
            self.fill_glyphs.push(GlyphCell {
                pos,
                ch,
                face: face(cell.bold, cell.italic),
                // Izgaranın ölçüsü: bant ızgaranın geçmişi, dock'un bağlam
                // satırı gibi ayrı bir sınıf değil.
                size: SizeClass::Normal,
                rgba: cell.fg.to_array(),
                wide: cell.wide,
                cluster: cell.cluster,
            });
        }
        if let Some(kind) = rule_kind(cell.underline) {
            self.fill_rules.push(RuleCell {
                pos,
                kind,
                rgba: cell.underline_color.unwrap_or(cell.fg).to_array(),
            });
        }
        if cell.strikeout {
            self.fill_rules.push(RuleCell {
                pos,
                kind: RuleKind::Strike,
                rgba: cell.fg.to_array(),
            });
        }
    }

    pub(crate) fn fill_bg(&self) -> &[Instance] {
        &self.fill_bg
    }

    pub(crate) fn fill_glyphs(&self) -> &[GlyphCell] {
        &self.fill_glyphs
    }

    pub(crate) fn fill_rules(&self) -> &[RuleCell] {
        &self.fill_rules
    }

    /// Doldurma bandının viewport orijini, piksel: `origin_px − fill_px`.
    ///
    /// **Formülün tek kopyası** ve `Frame`'de duruyor, çünkü iki terimi de
    /// burada yaşıyor (`origin_px` ile bandın satır sayısı × hücre boyu);
    /// `renderer.rs`'te kurulsaydı hücre ölçüsünün ikinci bir okuyucusu
    /// olurdu — [`Frame::dock_ground`]'un aritmetiği içeride tutmasıyla aynı
    /// disiplin.
    ///
    /// **Negatif meşru** ve üretimde oluyor: `origin_px` bandın boyundan
    /// küçükken bandın en eski satırları pencerenin tepesinden taşıyor ve
    /// Metal onları kırpıyor. Ölçüldü (017 phase-0, Apple M1 Pro /
    /// macOS 26.4.1, API doğrulama katmanı açık); tanık
    /// `Renderer::tests::a_negative_viewport_origin_draws_and_clips_from_the_top`.
    ///
    /// **Okuma anında**, push anında değil: ikisi arasında `set_origin_rows`
    /// bir kez daha koşuyor (hareket karesi) ve bant onunla **birlikte**
    /// kaymak zorunda (R3.1).
    pub(crate) fn fill_origin_px(&self) -> f32 {
        self.origin_px() - f32::from(self.fill_rows) * self.cell_px.1
    }

    /// Halenin payı, piksel — sol paydan türüyor ([`CARET_GLOW_RATIO`]),
    /// ikinci bir tasarım sabiti değil (R3).
    ///
    /// Aynı içi girinti üçüncü kez kullanılıyor: sol pay, dock'un nefes payı
    /// ve şimdi hale — üçü de **tek kaynaktan**, punto büyüyünce üçü birden
    /// büyüyor. Hale o kaynağın kendisi değil [`CARET_GLOW_RATIO`] kadarı;
    /// tamamı olduğunda ortaya gölge değil neon çıkıyordu.
    fn glow_px(&self) -> f32 {
        self.gutter_px * CARET_GLOW_RATIO * self.caret_style.glow as f32
    }

    /// Caret fragment'inin **boyanan** çekirdeği (x0, y0, x1, y1), pencere
    /// uzayı; fragment onu `[[position]]` ile karşılaştırıyor.
    pub(crate) fn caret_core(&self) -> [f32; 4] {
        self.caret_core
    }

    /// Caret fragment'inin şekil uniform'u: yarıçap, kenar, hale payı, hale
    /// alfası — hepsi piksel, sonuncusu 0..1.
    ///
    /// **Çıplak `[f32; 4]`, struct değil** (R2.1): Rust'ta `[f32; 4]` 4,
    /// MSL'de `float4` 16 hizalı ve ikisi bir struct'ın içinde buluşunca
    /// stride sessizce ayrışır. Tek başına argüman olarak ikisi de 16 bayt ve
    /// ofset 0, yani tuzak hiç doğmuyor.
    ///
    /// **Kenar dolu caret'te sıfır, içi boş caret'te `rule_px`**: sıfır
    /// shader'a "dolgu" demek. İçi boşalma odaktan geliyor
    /// ([`Frame::push_caret`]) ve yalnız bloğa uygulanıyor.
    pub(crate) fn caret_sdf(&self) -> [f32; 4] {
        #[cfg(test)]
        if let Some(shape) = self.caret_sdf_override {
            return shape;
        }
        [
            caret_radius_px(self.cell_px, self.caret_style.radius_ratio as f32),
            // Kenar yalnız içi boş caret'te; dolu caret'te 0 = dolgu.
            // Kalınlık yine fontun kendi metriğinden (`rule_px`), ikinci bir
            // tasarım sabiti yok.
            //
            // **Taban `caret_painted_rect`'inkiyle aynı** (`rule.max(1.0)`) ve
            // şart: `CellMetrics::new` sıfır kuralı kabul ediyor, sıfır kenar
            // ise shader'a "dolu" demek. O hâlde ters çevirme çoktan
            // kalkmışken caret **opak** çizilirdi ve altındaki harf kendi
            // rengiyle kalırdı — okunmayan kombinasyonun ta kendisi
            // (`/code-review`). Kararın iki yarısı aynı tabanı görmek zorunda.
            if self.caret_hollow {
                self.rule_px.max(1.0)
            } else {
                0.0
            },
            self.glow_px(),
            CARET_GLOW_ALPHA * self.caret_style.glow as f32,
        ]
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

    /// Izgara ile doldurma bandının küme tablosu ([`GlyphCell::cluster`]).
    pub(crate) fn clusters(&self) -> &Clusters {
        &self.clusters
    }

    /// Dock'un (ve gelişlerin) küme tablosu.
    pub(crate) fn dock_clusters(&self) -> &Clusters {
        &self.dock_clusters
    }

    /// Hayaletlerin küme tablosu ([`Frame::set_dock_fx`]).
    pub(crate) fn fx_clusters(&self) -> &Clusters {
        &self.fx_clusters
    }

    /// Izgaranın tablosunu `frame()`'in doldurması için **dışarı** alır;
    /// çağrıdan sonra [`Frame::put_clusters`] geri koyar. Taşıma, kopya değil:
    /// sink'ler `Frame`'i ödünç alırken tablo aynı çağrının ikinci `&mut`'u
    /// olamaz. Temizleme [`Frame::clear`]'da, yani alınan tablo boş.
    pub(crate) fn take_clusters(&mut self) -> Clusters {
        std::mem::take(&mut self.clusters)
    }

    pub(crate) fn put_clusters(&mut self, clusters: Clusters) {
        self.clusters = clusters;
    }

    /// Dock'un tablosu için [`Frame::take_clusters`]'ın ikizi.
    pub(crate) fn take_dock_clusters(&mut self) -> Clusters {
        std::mem::take(&mut self.dock_clusters)
    }

    pub(crate) fn put_dock_clusters(&mut self, clusters: Clusters) {
        self.dock_clusters = clusters;
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
        // Sütun **adımı** satıra göre: bağlam satırında küçük yüzün
        // ilerlemesi. Büyük adımla çizilseydi küçük harfler büyük hücrelerin
        // sol kenarlarına dağılır, aralarında sebepsiz boşluk kalırdı — göz
        // bunu "harf harf yazılmış" diye okur.
        //
        // Dikey aritmetik **değişmiyor**: satırın yüksekliği de payı da
        // ortak, küçük glyph büyük hücrenin taban çizgisinde duruyor. Band
        // ([`dock_px`]) bu yüzden hiç kısalmıyor.
        //
        // **Satır arası boşluk yalnız bağlam satırının üstünde** (032): giriş
        // satırları tek bir editör yüzeyi, bitişik. Yerleşim tepeden sayılıyor
        // ama viewport'u dibe yaslı (`yükseklik − yerleşim`,
        // `Renderer::encode_dock`), yani bağlam satırı her zaman bandın dibinde
        // ve giriş satırları onun üstüne diziliyor.
        let [_, y] = self.pos(0, row);
        let w = self.column_px(row);
        let pad = self.dock_pad();
        // Boşluk bağlam satırını **üstündeki giriş satırından** ayırıyor:
        // yalnız bağlam satırından ibaret yerleşimde (uzak oturum) ayrılacak
        // bir şey yok ve bandın boyu da onu saymıyor ([`dock_height`]).
        let gap = if self.is_context_row(row) && self.dock_rows >= 2 {
            dock_row_gap(pad)
        } else {
            0.0
        };
        [self.gutter_px + f32::from(col) * w, y + pad + gap]
    }

    /// Dock'un `row` satırındaki sütun adımı, piksel.
    ///
    /// Tek karar noktası: hangi satırın küçük olduğu **yalnız** burada ve
    /// [`Frame::push_dock`]'ta sorulur, ikisi de aynı sabite bakar. Ayrı
    /// eşiklere baksalardı konum küçük, glyph büyük (ya da tersi) olurdu.
    fn column_px(&self, row: u16) -> f32 {
        if self.is_context_row(row) {
            self.context_cell_px
        } else {
            self.cell_px.0
        }
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
    use bt_core::{CaretShape, CaretStyle, Cursor};

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

    #[test]
    fn a_lone_run_rounds_all_four_corners() {
        let runs = [run(3, 2, 9)];
        assert_eq!(selection_corners(&runs, 0), [Corner::Convex; 4]);
    }

    #[test]
    fn two_equal_runs_square_their_inner_corners() {
        use Corner::{Convex, Square};
        let runs = [run(3, 2, 9), run(4, 2, 9)];
        assert_eq!(
            selection_corners(&runs, 0),
            [Convex, Convex, Square, Square]
        );
        assert_eq!(
            selection_corners(&runs, 1),
            [Square, Square, Convex, Convex]
        );
    }

    #[test]
    fn a_step_gets_one_concave_fill_from_the_narrower_run() {
        use Corner::{Concave, Convex, Square};
        // Akış seçiminin tipik hâli: üstte 4'ten satır sonuna, altta satır
        // başından sona. Sol kenarda alt satır uzuyor → üst koşunun BL'si
        // içbükey; sağ kenar hizalı → kare.
        let runs = [run(3, 4, 9), run(4, 0, 9)];
        assert_eq!(
            selection_corners(&runs, 0),
            [Convex, Convex, Square, Concave]
        );
        assert_eq!(
            selection_corners(&runs, 1),
            [Convex, Square, Convex, Convex]
        );
        // Ters basamak: alt koşu dar ve sağda kalıyor → onun TR'si içbükey.
        let runs = [run(3, 0, 9), run(4, 0, 5)];
        assert_eq!(
            selection_corners(&runs, 0),
            [Convex, Convex, Convex, Square]
        );
        assert_eq!(
            selection_corners(&runs, 1),
            [Square, Concave, Convex, Convex]
        );
        // Dolgu yalnız dar koşuda: iki satırın içbükey köşe toplamı tek.
        let concave = |runs: &[SelectionRun]| {
            (0..runs.len())
                .flat_map(|i| selection_corners(runs, i))
                .filter(|&c| c == Concave)
                .count()
        };
        assert_eq!(concave(&[run(3, 4, 9), run(4, 0, 9)]), 1);
        assert_eq!(
            concave(&[run(3, 3, 6), run(4, 0, 9)]),
            2,
            "iki yanda basamak"
        );
    }

    #[test]
    fn diagonal_runs_touch_only_at_a_point_and_stay_convex() {
        // Üstteki 5'ten başlıyor, alttaki 4'te bitiyor: aynı sütunu hiç
        // paylaşmıyorlar, yani iki ayrı yuvarlak parça.
        let runs = [run(3, 5, 9), run(4, 0, 4)];
        assert_eq!(selection_corners(&runs, 0), [Corner::Convex; 4]);
        assert_eq!(selection_corners(&runs, 1), [Corner::Convex; 4]);
    }

    #[test]
    fn a_blank_row_between_runs_splits_the_shape() {
        let runs = [run(3, 0, 9), run(5, 0, 9)];
        assert_eq!(selection_corners(&runs, 0), [Corner::Convex; 4]);
        assert_eq!(selection_corners(&runs, 1), [Corner::Convex; 4]);
    }

    /// Sol payı **sıfır** olan ızgara ölçüsü: bu modüldeki sınamaların çoğu
    /// listelerin düzenini soruyor, orijini değil, ve sıfır pay onların
    /// beklenen piksellerini hücre aritmetiğinde tutuyor. Payın kendi
    /// sınamaları [`GUTTER`]'ı kullanıyor ve adıyla anıyor.
    fn grid(width: u16, height: u16) -> CellMetrics {
        CellMetrics::new(width, height, width, 0, 1).expect("sıfır olmayan hücre")
    }

    /// Seçimin satır koşusu; köşe kararı sınamalarının kısaltması.
    fn run(row: u16, first: u16, last: u16) -> SelectionRun {
        SelectionRun { row, first, last }
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
            frame.push_caret(at, cursor.text, rgba, alpha, cursor.shape, true);
        }
    }

    /// `Frame::move_cursor`'ın sınama kabuğu; görünmez imleç caret'i siliyor.
    fn move_cursor(
        frame: &mut Frame,
        cursor: Cursor,
        at: [f32; 2],
        rgba: LinearRgba,
        alpha: f32,
        focused: bool,
    ) {
        frame.move_caret(at, cursor.text, rgba, alpha, focused);
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
            input_rows: 1,
            shape: CaretShape::Block,
            blink: false,
            text: TEXT,
            // Kaydırma kararı hareketin işi (`motion.rs`); bu listeyi
            // ilgilendirmiyor, çünkü konum zaten dışarıdan geliyor.
            display_offset: 0,
            // Doluluk sayısı da bu listeyi ilgilendirmiyor: ötelemeyi
            // `set_origin_rows` söylüyor ve bu iki alan onun **girdisi**,
            // yani `link.rs`'in okuduğu yer. Dolu ızgara, yani öteleme sıfır.
            content_rows: 1,
            // Doldurmanın çizimi phase-3'ün işi (017); bu modül onu henüz
            // tüketmiyor.
            fill: 0,
            // Kesirli kaydırma (027) de bu listeyi ilgilendirmiyor: tam
            // satırda, tepe satırı yok.
            top_row: 0,
            scrolled: 0,
            scroll_frac: 0.0,
            scroll_generation: 0,
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
        frame.clear(grid(9, 18), CaretStyle::default());

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

        frame.clear(grid(9, 18), CaretStyle::default());
        assert_eq!(frame.bg_count(), 0);
        assert!(frame.bg_instances().is_empty());
    }

    #[test]
    fn invisible_cursor_is_not_drawn() {
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        push_settled(&mut frame, cursor(0, 0, false));
        assert!(frame.bg_instances().is_empty());
        // Uniform da dokunulmadan kalır: dejenere dikdörtgen "blok yok"
        // demenin tek yolu, shader'da ikinci bir bayrak yok.
        assert_eq!(frame.cursor_block(), &CursorBlock::default());
    }

    #[test]
    fn an_unfocused_block_is_hollow_and_stops_inverting() {
        // **Odaksız pencerenin iki işareti** ve ikisi de tek karardan
        // (`push_caret`'in `focused`'ı): caret'in içi boşalıyor (kenar
        // kalınlığı `rule_px`) ve ters çevirme **kalkıyor**.
        //
        // İkincisi şart: ters çevirme boyanan zemine dayanıyor. Çerçevenin
        // ortasındaki harf zemin renginde çizilseydi altında boyanmış bir şey
        // olmadığı için **görünmez** olurdu — içi boş caret metni yutardı.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);

        assert_eq!(
            frame.cursor_block().rect,
            [0.0; 4],
            "odaksız caret hâlâ ters çeviriyor: altındaki harf kaybolur"
        );
        assert!(
            frame.caret_sdf()[1] > 0.0,
            "içi boş caret'in kenarı çizilmiyor"
        );

        // Boyanan alan **aynı yerde**: içi boş caret aynı hücreyi kaplıyor.
        let hollow = frame.grid_caret().expect("caret");
        let mut lit = Frame::default();
        lit.clear(grid(9, 18), CaretStyle::default());
        lit.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        let solid = lit.grid_caret().expect("caret");
        assert_eq!(hollow.pos, solid.pos, "içi boşalınca ayak izi kaydı");
        assert_eq!(hollow.size, solid.size);
        assert_eq!(lit.caret_sdf()[1], 0.0, "odaklı caret'in içi boş");
    }

    #[test]
    fn a_hollow_caret_survives_a_zero_rule_metric() {
        // **Kararın iki yarısı aynı tabanı görmek zorunda** (`/code-review`).
        // `CellMetrics::new` sıfır kuralı kabul ediyor; kenar tabansız
        // bırakılsaydı içi boş caret'in `stroke`'u 0 olur, shader onu "dolu"
        // okur ve caret **opak** çizilirdi — üstelik ters çevirme çoktan
        // kalkmış olduğu için altındaki harf kendi rengiyle kalır ve okunmayan
        // kombinasyon ortaya çıkardı.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(10, 20, 10, 0, 0).expect("sıfır olmayan hücre"),
            CaretStyle::default(),
        );
        frame.push_caret([0.0, 0.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);
        assert!(
            frame.caret_sdf()[1] > 0.0,
            "sıfır kural metriğinde içi boş caret doldu"
        );
        assert_eq!(
            frame.cursor_block().rect,
            [0.0; 4],
            "ters çevirme kalkmalıydı"
        );
    }

    #[test]
    fn the_unfocused_setting_keeps_the_caret_solid() {
        // `[terminal] cursor_unfocused = "solid"`: odak gitse de içi boşalmıyor
        // ve **ters çevirme de duruyor** — ikisi tek karardan.
        let mut frame = Frame::default();
        frame.clear(
            grid(9, 18),
            CaretStyle {
                unfocused: UnfocusedCaret::Solid,
                ..CaretStyle::default()
            },
        );
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);
        assert_eq!(frame.caret_sdf()[1], 0.0, "solid iken içi boşaldı");
        assert_ne!(
            frame.cursor_block().rect,
            [0.0; 4],
            "solid iken ters çevirme kalktı"
        );

        // Varsayılan (`hollow`) aynı girdide boşaltıyor: ayrım gerçekten
        // anahtardan geliyor, başka bir şeyden değil.
        let mut lit = Frame::default();
        lit.clear(grid(9, 18), CaretStyle::default());
        lit.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);
        assert!(lit.caret_sdf()[1] > 0.0, "hollow iken içi dolu kaldı");
    }

    #[test]
    fn thin_carets_never_go_hollow() {
        // **İçi boşalma yalnız bloğa.** Alt çizgi ve dikey çubuk zaten birer
        // ince şerit ve kenar kalınlığı da `rule_px`, yani çıkarma gövdeyi
        // tümden yutar ve caret görünmez olurdu. O şekillerde odaksızlığın
        // sinyali blink'in durması.
        for shape in [CaretShape::Underline, CaretShape::Beam] {
            let mut frame = Frame::default();
            frame.clear(grid(9, 18), CaretStyle::default());
            frame.push_caret([0.0, 0.0], TEXT, CURSOR, OPAQUE, shape, false);
            assert_eq!(frame.caret_sdf()[1], 0.0, "{shape:?} içi boşaldı");
            assert_ne!(
                frame.cursor_block().rect,
                [0.0; 4],
                "{shape:?} ters çevirmeyi bıraktı"
            );
        }
    }

    #[test]
    fn a_motion_frame_follows_the_live_focus() {
        // **Odak hareket karesinde de taze** ve bu bir kullanıcı bildirimiyle
        // geldi (2026-09-20): "pencereye geri dönünce çerçeve duruyor ama içi
        // boş, sonradan doluyor". Sebep saklanmış bir kopyanın korunmasıydı —
        // odak `bt-gpu`'nun kendi biti ve hareket karesi ona **erişiyor**,
        // yani korumak onu bayatlatmaktı. Şekil için koruma doğru: o
        // `bt-core`'dan geliyor ve bu yolun ona erişimi yok.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());

        // Odaksız basıldı, hareket karesi **odaklı** geldi: caret dolmalı.
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, false);
        assert!(frame.caret_sdf()[1] > 0.0, "içi boş başlamalıydı");
        frame.move_caret([2.0, 1.0], TEXT, CURSOR, OPAQUE, true);
        assert_eq!(frame.caret_sdf()[1], 0.0, "hareket karesi odağı görmedi");
        assert_ne!(frame.cursor_block().rect, [0.0; 4], "ters çevirme dönmedi");

        // Ters yön: odak giderken de hareket karesi anında boşaltıyor.
        frame.move_caret([3.0, 1.0], TEXT, CURSOR, OPAQUE, false);
        assert!(frame.caret_sdf()[1] > 0.0);
        assert_eq!(frame.cursor_block().rect, [0.0; 4]);
    }

    #[test]
    fn cursor_block_covers_its_cell_and_clears_with_the_frame() {
        // Dikdörtgen bloğun **kendi** instance'ıyla aynı hücreye oturmalı:
        // ayrışsalardı blok bir yerde, altındaki metnin rengi başka bir yerde
        // olurdu ve ikisi de sessizce yanlış çizerdi.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
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
        frame.clear(grid(9, 18), CaretStyle::default());
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
        frame.clear(grid(9, 18), CaretStyle::default());
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
        frame.clear(grid(9, 18), CaretStyle::default());
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
        frame.clear(grid(10, 20), CaretStyle::default());
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
        frame.clear(grid(8, 16), CaretStyle::default());
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
            move_cursor(
                &mut frame,
                cursor(5, 0, true),
                [4.5, 0.0],
                CURSOR,
                OPAQUE,
                true,
            );
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
        move_cursor(
            &mut frame,
            cursor(5, 0, false),
            [4.5, 0.0],
            CURSOR,
            OPAQUE,
            true,
        );
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
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.push(bg_cell(1, 1));
        assert_eq!(frame.bg_instances()[0].pos, [9.0, 18.0]);

        frame.clear(grid(18, 36), CaretStyle::default());
        frame.push(bg_cell(1, 1));
        assert_eq!(frame.bg_instances()[0].pos, [18.0, 36.0]);
        assert_eq!(frame.bg_instances()[0].size, [18.0, 36.0]);
    }

    #[test]
    fn grid_coords_convert_to_pixels() {
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
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
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
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
        frame.clear(grid(9, 18), CaretStyle::default());
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
        move_cursor(
            &mut frame,
            cursor(0, 2, true),
            [0.0, 4.0],
            CURSOR,
            OPAQUE,
            true,
        );
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
        frame.clear(grid(9, 18), CaretStyle::default());
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
        frame.clear(grid(9, 18), CaretStyle::default());
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
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        frame.push_block(block(2));

        let mark = frame.stripes()[0];
        assert_eq!(mark.kind, RuleKind::Chevron, "işaret hâlâ dikdörtgen");
        assert_eq!(mark.pos[1], 36.0, "işaret kendi satırında başlamalı");
        assert_eq!(mark.rgba, SUCCESS.to_array(), "renk sınırdan gelir");
        // **Hiza hesaplanmıyor, tek formülden doğuyor.** Dock'un işareti de
        // 0. sütunda ve o da `Frame::pos`'tan geçiyor; ikisi ayrı aritmetikle
        // yerleştirildiği sürece yarım pay kadar ayrı duruyorlardı.
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
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
        frame.clear(
            CellMetrics::new(4, 18, 4, 12, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('>'),
            ..Cell::default()
        });
        frame.push_block(block(0));
        assert_eq!(frame.stripes()[0].pos[0], 12.0, "işaret paydan geçmedi");
        assert_eq!(frame.stripes()[0].pos[0], frame.dock_glyphs()[0].pos[0]);

        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        assert!(
            frame.stripes().is_empty(),
            "`clear` işaretleri de boşaltmalı"
        );
    }

    #[test]
    fn the_context_budget_is_the_same_strip_in_small_steps() {
        // Bütçe bir **oran**: iki satır aynı yatay şeridi kaplıyor (sol pay
        // ortak), ayrışan tek şey bir harfin kaç piksel ilerlettiği. Sayı
        // burada doğuyor çünkü `bt-core` piksel görmüyor.
        let m = |w, cw| CellMetrics::new(w, 20, cw, GUTTER, 1).expect("ölçü");
        // 80 × 10 piksel = 800; 8 piksellik adımda 100 sütun.
        assert_eq!(context_cols(80, m(10, 8)), 100);
        // Oran 1 ise bütçe de aynı: "küçültme yok" kolu **desteklenen** bir
        // hâl ve çıktısı bugünküyle bit bit aynı.
        assert_eq!(context_cols(80, m(10, 10)), 80);
        // Tam bölünmeyen oran **aşağı** yuvarlanıyor: bir sütun fazla vermek
        // satırı payın dışına taşırırdı.
        assert_eq!(context_cols(10, m(10, 3)), 33);
        // Dejenere uçta taşma yok: çarpım `u32`'de yaşıyor ve sonuç kırpılıyor.
        assert_eq!(context_cols(u16::MAX, m(u16::MAX, 1)), u16::MAX);
    }

    #[test]
    fn the_context_row_steps_by_the_small_advance() {
        // **Dock'un iki satırı iki ayrı sütun adımında.** Giriş satırı
        // gösterim fontunda, bağlam satırı küçük yüzde: aynı piksel şeridine
        // daha çok harf sığıyor. Küçük glyph büyük yuvanın sol kenarında
        // durduğu için dörtlü büyük kalabiliyor ve ayrı bir draw call
        // doğmuyor (`GlyphCell::size`).
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(10, 20, 8, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        let at = |col, row| Cell {
            col,
            row,
            ch: Some('x'),
            ..Cell::default()
        };
        frame.push_dock(at(0, 0));
        frame.push_dock(at(3, 0));
        frame.push_dock(at(0, 1));
        frame.push_dock(at(3, 1));

        let g = frame.dock_glyphs();
        // İki satır da sol paydan başlıyor: adım ayrı, **başlangıç ortak**.
        assert_eq!(g[0].pos[0], g[2].pos[0], "satırlar aynı sütunda başlamadı");
        // Giriş satırı gösterim ölçüsünde ilerliyor.
        assert_eq!(g[1].pos[0] - g[0].pos[0], 30.0, "giriş satırının adımı");
        // Bağlam satırı küçük ölçüde.
        assert_eq!(g[3].pos[0] - g[2].pos[0], 24.0, "bağlam satırının adımı");
        // Punto sınıfı konumla **aynı eşikten**: harf bir ölçüde, adımı başka
        // ölçüde olamaz.
        assert_eq!(g[1].size, SizeClass::Normal);
        assert_eq!(g[3].size, SizeClass::Small);

        // **Dikey aritmetik dokunulmamış.** Bandın yüksekliği de satırların
        // y'si de büyük hücreden: küçük harf büyük satırın taban çizgisinde
        // duruyor, satır kendi bandını küçültmüyor. Bu yüzden `dock_px`'in
        // tüketicileri (`split_into_grid`, ikinci viewport) hiç değişmedi.
        assert_eq!(
            g[2].pos[1] - g[0].pos[1],
            20.0 + dock_row_gap(GUTTER as f32)
        );
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(
            frame.dock_layout_px(),
            dock_px(
                DOCK_ROWS,
                CellMetrics::new(10, 20, 8, GUTTER, 1).expect("ölçü")
            ),
            "küçük punto bandı kısaltmamalı"
        );

        // Izgara küçük sınıfa **hiç** girmiyor: tek tüketici dock'un alt satırı.
        frame.push(at(0, 1));
        assert_eq!(frame.glyphs()[0].size, SizeClass::Normal);
    }

    #[test]
    fn an_upload_button_fills_its_columns_on_the_context_row() {
        // 037 phase-6: dolgunun kenarı düğmenin sütun sınırı — farenin
        // isabet aralığı (`bt_core::transfer_button_at`) ile aynı sütunlar,
        // bağlam satırının küçük adımında. Etiketin glyph'i o aralığın içinde.
        let metrics = CellMetrics::new(10, 20, 8, GUTTER, 1).expect("ölçü");
        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.push_dock(Cell {
            col: 3,
            row: 1,
            ch: Some('C'),
            ..Cell::default()
        });
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(frame.dock_button_draws(0.0).count(), 0, "düğme söylenmedi");
        let button = DockButton {
            start: 2,
            end: 5,
            color: CURSOR,
            state: ButtonState::Hover,
        };
        frame.set_dock_buttons([None, Some(button)]);
        let draws: Vec<_> = frame.dock_button_draws(100.0).collect();
        assert_eq!(draws.len(), 2, "dolgu + çerçeve");
        let [fill, edge] = [draws[0], draws[1]];
        let glyph = frame.dock_glyphs()[0].pos;
        assert_eq!(
            fill.instance.pos,
            [glyph[0] - 8.0, glyph[1]],
            "sol kenar etiketten bir sütun önce"
        );
        assert_eq!(
            fill.instance.size,
            [24.0, 20.0],
            "üç küçük sütun, bir satır"
        );
        // Farenin dikey aralığı aynı bant: giriş bloğunun tepesi (pay) + bir
        // giriş satırı + `context_row_offset` (`BateriView::context_column`).
        assert_eq!(
            fill.instance.pos[1],
            (GUTTER as f32 + 20.0 + context_row_offset(1, metrics)).round(),
            "fare ile dolgu aynı satır bandını okumuyor"
        );
        assert_eq!(fill.instance.rgba[3], 0.34);
        assert_eq!(edge.instance.rgba[3], 0.7);
        assert_eq!(fill.shape[1], 0.0, "dolgu");
        assert_eq!(edge.shape[1], 1.0, "çerçeve kural kalınlığında");
        assert_eq!(fill.shape[0], frame.selection_radius());
        // Çekirdek pencere uzayında: dörtlü dock-yerel, viewport `origin_y`
        // kadar aşağıda.
        assert_eq!(fill.core[1], fill.instance.pos[1] + 100.0);
        assert_eq!(fill.core[2] - fill.core[0], 24.0);

        frame.set_dock_buttons([
            None,
            Some(DockButton {
                state: ButtonState::Idle,
                ..button
            }),
        ]);
        let idle: Vec<_> = frame.dock_button_draws(0.0).collect();
        assert!(
            idle[0].instance.rgba[3] < fill.instance.rgba[3],
            "fare üstünde koyulaşıyor"
        );
        // Açılış her karede siliyor.
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(frame.dock_button_draws(0.0).count(), 0);
    }

    #[test]
    fn stripes_stay_out_of_the_cell_count_and_survive_motion_frames() {
        // Phase'in asıl sözleşmesi (010 → R4.1): şerit `bg`'ye **girmiyor**. Girip
        // sayılmasaydı `move_cursor`'ın kırpması onu her hareket karesinde
        // siler ve şerit imleç kaydıkça titrerdi; sayılsaydı `hucre=` jetonu
        // hücre olmayan bir şeyi de sayar ve duman kapısının anlamı kayardı.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(8, 16, 8, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        frame.push(bg_cell(0, 0));
        frame.push_block(block(0));
        push_settled(&mut frame, cursor(0, 0, true));

        assert_eq!(frame.bg_count(), 1, "şerit hücre sayılmamalı");
        assert_eq!(frame.bg_instances().len(), 1, "caret arka plana sızdı");
        assert_eq!(frame.stripes().len(), 1);

        let stripes = frame.stripes().to_vec();
        for _ in 0..3 {
            move_cursor(
                &mut frame,
                cursor(5, 0, true),
                [4.5, 0.0],
                CURSOR,
                OPAQUE,
                true,
            );
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
        // Girselerdi ızgaranın kare ömrüne bağlanır ve kendi viewport'undan
        // koparlardı — şeridin ayrı liste olma gerekçesinin aynısı, bir derece
        // daha görünür belirtiyle.
        // Sayaçlara girmemesi ikinci sözleşme: `hucre=8 glif=6 kural=15` duman
        // koşusunda ölçülüyor ve anlamı bit bit korunmalı.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push(bg_cell(0, 0));
        push_settled(&mut frame, cursor(0, 0, true));
        frame.push_dock(dock_cell(0));
        frame.push_dock(Cell {
            underline: UnderlineStyle::Single,
            ..dock_cell(1)
        });
        frame.open_dock(BG, CURSOR, CURSOR);

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
            move_cursor(
                &mut frame,
                cursor(5, 0, true),
                [4.5, 0.0],
                CURSOR,
                OPAQUE,
                true,
            );
            assert_eq!(frame.dock_bg(), dock_bg, "hareket karesi dock'u oynattı");
            assert_eq!(frame.dock_glyphs(), dock_glyphs);
            assert_eq!(frame.dock_rules(), dock_rules);
            assert!(frame.dock().is_some(), "hareket karesi yüzeyi kapattı");
        }

        // `clear` **hepsini** boşaltıyor: korunan bir yüzey, dock'u olmayan
        // bir oturumun penceresinde asılı kalırdı.
        frame.clear(grid(8, 16), CaretStyle::default());
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
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push_dock(dock_cell(1));
        let settled_bg = frame.dock_bg()[0];

        // Aynı kare, iki satır ötelenmiş: ızgaranın hücresi kayar, dock'unki
        // kaymaz.
        frame.clear(grid(8, 16), CaretStyle::default());
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

    /// Doldurulan bir satırın hücresi; ızgaranın [`bg_cell`]'inin band ikizi.
    fn fill_cell(col: u16, row: u16) -> Cell {
        Cell {
            col,
            row,
            ch: Some('x'),
            fg: CURSOR,
            bg: Some(BG),
            ..Default::default()
        }
    }

    #[test]
    fn the_fill_keeps_its_own_lists_and_stays_out_of_the_counters() {
        // Dock bekçisinin kardeşi (R3.2) ve aynı iki sözleşme: doldurma
        // listeleri ızgaranınkilere **girmiyor** — girselerdi ızgaranın
        // uzayından, yani bandın yerine içeriğin üstüne çizilirlerdi — ve
        // sayaçlara da girmiyorlar: `hucre=8 glif=6 kural=15` duman
        // koşusunda ölçülüyor ve anlamı bit bit korunmalı.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push(bg_cell(0, 0));
        push_settled(&mut frame, cursor(0, 0, true));
        frame.set_fill_rows(2);
        frame.push_fill(fill_cell(0, 0));
        frame.push_fill(Cell {
            underline: UnderlineStyle::Single,
            ..fill_cell(1, 1)
        });

        assert_eq!(frame.bg_count(), 1, "doldurma hücre sayıldı");
        assert_eq!(frame.glyph_count(), 0, "doldurma glyph sayıldı");
        assert_eq!(frame.rule_count(), 0, "doldurma kural sayıldı");
        assert_eq!(frame.bg_instances().len(), 1, "doldurma `bg`'ye sızdı");

        let (fill_bg, fill_glyphs, fill_rules) = (
            frame.fill_bg().to_vec(),
            frame.fill_glyphs().to_vec(),
            frame.fill_rules().to_vec(),
        );
        assert_eq!(fill_bg.len(), 2);
        assert_eq!(fill_glyphs.len(), 2);
        assert_eq!(fill_rules.len(), 1);

        // Hareket karesi listeleri **koruyor**: grid kirli değil, yani bandın
        // hücreleri de hâlâ geçerli (dock emsali).
        for _ in 0..3 {
            move_cursor(
                &mut frame,
                cursor(5, 0, true),
                [4.5, 0.0],
                CURSOR,
                OPAQUE,
                true,
            );
            assert_eq!(frame.fill_bg(), fill_bg, "hareket karesi bandı oynattı");
            assert_eq!(frame.fill_glyphs(), fill_glyphs);
            assert_eq!(frame.fill_rules(), fill_rules);
            assert_eq!(frame.fill_rows(), 2, "hareket karesi bandı kapattı");
        }

        // `clear` **hepsini** boşaltıyor, bandın boyu dahil: korunan bir boy
        // doldurmayı kapatan ilk karede (Ctrl-L, dock'u olmayan pencere)
        // ekranda asılı kalırdı — ve `fill_rows == 0` üçüncü viewport'un
        // kurulmadığı hâlin ta kendisi (geri alma şeridi).
        frame.clear(grid(8, 16), CaretStyle::default());
        assert_eq!(frame.fill_rows(), 0, "clear bandı bırakmadı");
        assert!(frame.fill_bg().is_empty());
        assert!(frame.fill_glyphs().is_empty());
        assert!(frame.fill_rules().is_empty());
    }

    #[test]
    fn the_fill_band_rides_the_origin() {
        // **R3.1'in CPU yarısı.** Bant ötelemenin üstünde duruyor ve
        // ötelemeyle **birlikte** kayıyor: hücreler fill-yerel doğuyor,
        // ekrana taşıyan şey `origin_px − fill_px` ve o, okuma anında
        // türüyor. Push anında pişirilseydi hareket karesi — listeler
        // korunur, yalnız `origin_px` değişir — bandı yerinde dondururdu.
        // Pikselin tanığı `renderer.rs`'te.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_fill_rows(2);
        frame.push_fill(fill_cell(1, 0));
        let pushed = frame.fill_bg()[0];

        // Öteleme yokken bandın orijini negatif: iki satırı da pencerenin
        // tepesinden taşıyor ve Metal kırpıyor (017 phase-0'ın ölçümü).
        assert_eq!(frame.fill_origin_px(), -32.0, "bant ötelemesiz kaymadı");
        for rows in [3.0, 2.5, 1.0] {
            frame.set_origin_rows(rows);
            assert_eq!(
                frame.fill_origin_px(),
                rows * 16.0 - 32.0,
                "bandın orijini ötelemeyi izlemedi"
            );
            // Hücrenin kendisi kıpırdamıyor: kayan şey uzayın ta kendisi.
            assert_eq!(frame.fill_bg()[0], pushed, "bant ötelemeyi yedi");
        }
    }

    #[test]
    fn the_scroll_fraction_lowers_the_grid_by_whole_device_pixels() {
        // **R2.1:** kaydırmanın kesri ızgarayı o kadar aşağı çiziyor ve
        // piksel ötelemeninki gibi **aygıt ızgarasına** oturuyor — yarım
        // pikselde dinlenen bir kaydırma bütün metni bulanıklaştırırdı.
        // Kesir ötelemeden **ayrı** yuvarlanıyor, çünkü caret ötelemeden muaf
        // ama kesirden değil ve kendi payını tek başına istiyor
        // (`the_grid_caret_rides_the_fraction_and_the_dock_caret_does_not`).
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.set_scroll_frac(0.3);
        frame.set_origin_rows(0.0);
        assert_eq!(frame.origin_px(), 5.0, "kesir piksele yuvarlanmadı");
        frame.set_origin_rows(2.0);
        assert_eq!(frame.origin_px(), 41.0, "kesir ötelemeye eklenmedi");

        // Hareket karesi `clear` çağırmıyor: kesir korunuyor ve öteleme her
        // yazıldığında yeniden ekleniyor.
        frame.set_origin_rows(1.0);
        assert_eq!(frame.origin_px(), 23.0, "hareket karesi kesri düşürdü");

        // `clear` kesri bırakıyor: içerik karesi onu her karede yeniden
        // söylüyor, söylemeyen kare tam satırda çiziyor.
        frame.clear(grid(9, 18), CaretStyle::default());
        frame.set_origin_rows(2.0);
        assert_eq!(frame.origin_px(), 36.0, "clear kesri bırakmadı");

        // Sıra serbest: kesir ötelemeden sonra yazılsa da toplamda.
        frame.set_scroll_frac(0.3);
        assert_eq!(frame.origin_px(), 41.0, "kesir sıraya bağlı");

        // Yuvarlama tam hücreye çıkmıyor: ofseti değişmemiş ızgara bir satır
        // aşağı çizilemez.
        frame.set_scroll_frac(0.99);
        assert_eq!(
            frame.origin_px(),
            36.0 + 17.0,
            "kesir bir hücreye yuvarlandı"
        );
    }

    #[test]
    fn a_grid_caret_pushed_into_the_dock_band_does_not_invert_the_dock() {
        // Kesirle kayan son satırın caret'i dock bandına girebiliyor ve orada
        // harfiyle birlikte dock'un zemininin altında. Ters çevirme dikdörtgeni
        // iki glyph encode'una da gidiyor: kırpılmasaydı dock'un o sütundaki
        // harfleri zemin renginde, görünmez çizilirdi.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_dock_top(64.0);
        frame.set_scroll_frac(0.5);
        frame.push_caret([0.0, 3.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        assert!(
            frame.grid_caret().is_some(),
            "kesir caret'i dock'a devretti"
        );
        let rect = frame.cursor_block().rect;
        assert_eq!(rect[1], 56.0);
        assert_eq!(rect[3], 64.0, "ters çevirme dock bandına taştı");
    }

    #[test]
    fn the_top_row_sits_above_the_band_and_rides_with_it() {
        // **R2.2:** kesrin açtığı şeridi kapatan satır doldurma kanalının en
        // üstünde (fill-yerel `0`), bandın satırları onun altında. Kanalın
        // boyu `top_row + fill` ve bandın orijini de ondan türüyor, yani
        // tepe satırı bandla ve ızgarayla **birlikte** kayıyor. Sayaçlara da
        // girmiyor: `hucre=`/`glif=`/`kural=` ızgaranın tanıkları.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_scroll_frac(0.25);
        // Bir tepe satırı, iki bant satırı.
        frame.set_fill_rows(3);
        frame.push_fill(fill_cell(0, 0));
        frame.push_fill(fill_cell(0, 1));
        frame.push_fill(fill_cell(0, 2));
        frame.push_fill_block(block(0));

        assert_eq!(frame.bg_count(), 0, "tepe satırı hücre sayıldı");
        assert_eq!(frame.glyph_count(), 0, "tepe satırı glyph sayıldı");
        assert_eq!(frame.rule_count(), 0, "tepe satırı kural sayıldı");

        for rows in [2.0, 1.5, 0.0] {
            frame.set_origin_rows(rows);
            let origin = frame.origin_px();
            assert_eq!(origin, (rows * 16.0).round() + 4.0);
            let band = frame.fill_origin_px();
            assert_eq!(band, origin - 48.0, "kanal tepe satırını saymadı");
            // Ekrandaki yerler: tepe satırı ızgaranın üç satır, bandın son
            // satırı bir satır üstünde — aradaki boşluk sıfır.
            let top = band + frame.fill_bg()[0].pos[1];
            let last = band + frame.fill_bg()[2].pos[1];
            assert_eq!(top, origin - 48.0);
            assert_eq!(last + 16.0, origin, "bant ızgaraya değmiyor");
        }
        // Tepe satırının işareti de bandın listesinde, ızgaranınkinde değil.
        assert_eq!(frame.fill_rules().len(), 1);
        assert!(
            frame.stripes().is_empty(),
            "tepe satırının işareti ızgaraya düştü"
        );
    }

    #[test]
    fn the_grid_caret_rides_the_fraction_and_the_dock_caret_does_not() {
        // **Kesir ızgaranın bütün dünyasını kaydırıyor, caret dahil.** Caret
        // ötelemeden muaf (hedefi ekran satırı), kesirden değil: ızgara kesir
        // kadar aşağı çizilirken caret yerinde kalsaydı blok harfinin bir
        // kısmını ve üstteki satırın bir şeridini örter, ters çevirme de
        // yanlış pikselleri çevirirdi — `sleep 10`'un imleci ile trackpad'de
        // yarım satır yukarı. Dock caret'i ise kaydırmadan muaf: dock ayrı bir
        // yüzey.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_dock_top(64.0);
        frame.set_scroll_frac(0.5);
        frame.set_origin_rows(0.0);
        frame.push(bg_cell(0, 2));
        frame.push_caret([0.0, 2.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        // Hücrenin ekrandaki yeri: listedeki konum + viewport'un orijini.
        let cell_on_screen = frame.bg_instances()[0].pos[1] + frame.origin_px();
        assert_eq!(cell_on_screen, 40.0);
        assert_eq!(
            frame.grid_caret().expect("caret").pos[1] + frame.origin_px(),
            cell_on_screen,
            "caret harfinden ayrıldı"
        );
        assert_eq!(
            frame.cursor_block().rect[1],
            cell_on_screen,
            "ters çevirme harfin üstünde değil"
        );
        assert_eq!(frame.caret_core()[1], cell_on_screen);

        // Hareket karesi de aynı kaydırmayı uyguluyor: kesir `Frame`'de duruyor.
        frame.move_caret([0.0, 2.0], TEXT, CURSOR, OPAQUE, true);
        assert_eq!(frame.cursor_block().rect[1], cell_on_screen);

        // Yuva seçimi **kaydırılmamış** konuma bakıyor: kesir bir devir değil.
        // Dock bandındaki caret kıpırdamıyor.
        frame.move_caret([0.0, 4.0], TEXT, CURSOR, OPAQUE, true);
        let caret = frame.dock_caret(64.0).expect("caret dock yuvasında değil");
        assert_eq!(caret.pos[1], 0.0, "dock caret'i kesirle kaydı");
        assert_eq!(frame.cursor_block().rect[1], 64.0);
    }

    /// Dock'un seçimi ızgaranın şekliyle: tek satır, dört köşe yuvarlak,
    /// dock-yerel konum (sol pay + nefes payı) ve ızgaranın listesine
    /// **girmiyor** — dock ötelemeden muaf, kendi viewport'unda çiziliyor.
    #[test]
    fn a_dock_selection_is_one_rounded_run_in_dock_space() {
        let mut frame = Frame::default();
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("sıfır olmayan hücre");
        frame.clear(metrics, CaretStyle::default());
        frame.push_selection(&[], BG);
        frame.push_dock_selection(&[SelectionRun {
            row: 0,
            first: 3,
            last: 5,
        }]);
        assert!(frame.selection_instances().is_empty(), "ızgaraya sızdı");
        let [run] = frame.dock_selection_instances() else {
            panic!(
                "tek dörtgen beklendi: {:?}",
                frame.dock_selection_instances()
            );
        };
        let pad = f32::from(GUTTER);
        assert_eq!(run.pos, [pad + 3.0 * 9.0, pad]);
        assert_eq!(run.size, [27.0, 18.0]);
        assert_eq!(run.rgba, [1.0; 4], "köşeler yuvarlak değil");
        // Renk ızgaranınkiyle aynı uniform.
        assert_eq!(frame.selection_rgba(), BG.to_array());
        // İçerik karesi listeyi boşaltıyor.
        frame.clear(metrics, CaretStyle::default());
        assert!(frame.dock_selection_instances().is_empty());
    }

    fn search_run(row: u16, first: u16, last: u16, current: bool, continues: bool) -> SearchRun {
        SearchRun {
            row,
            first,
            last,
            current,
            continues,
        }
    }

    /// Arama çizimi için hazır bir kare: 9×18 hücre, renkler iki ayrık ton.
    fn search_frame() -> Frame {
        let mut frame = Frame::default();
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("sıfır olmayan hücre");
        frame.clear(metrics, CaretStyle::default());
        frame
    }

    /// 033 Karar 7: köşeler **eşleşme başına**. Ardışık satırlardaki iki ayrı
    /// eşleşme iki ayrı şekil — dört köşesi de yuvarlak, içbükey dolgu yok —;
    /// aynı iki koşu tek eşleşmenin sarılması olunca tek şekle kaynıyor ve
    /// basamaklar dolguyla kapanıyor.
    #[test]
    fn search_corners_are_per_match() {
        let mut frame = search_frame();
        let red = LinearRgba::from_srgb(0xff, 0, 0);
        let green = LinearRgba::from_srgb(0, 0xff, 0);
        frame.push_search(
            &[
                search_run(0, 2, 4, false, false),
                search_run(1, 0, 6, false, false),
            ],
            red,
            green,
        );
        let separate = frame.search_match_instances();
        assert_eq!(separate.len(), 2, "içbükey dolgu doğdu: {separate:?}");
        assert!(
            separate.iter().all(|part| part.rgba == [1.0; 4]),
            "iki ayrı eşleşme kaynadı: {separate:?}"
        );
        assert_eq!(frame.search_match_rgba(), red.to_array());
        assert_eq!(frame.search_current_rgba(), green.to_array());

        let mut frame = search_frame();
        frame.push_search(
            &[
                search_run(0, 2, 4, false, false),
                search_run(1, 0, 6, false, true),
            ],
            red,
            green,
        );
        let wrapped = frame.search_match_instances();
        // Üst koşunun iki alt köşesi içbükey: iki dolgu parçası.
        assert_eq!(wrapped.len(), 4, "sarılan eşleşme kaynamadı: {wrapped:?}");
        assert_eq!(
            wrapped[0].rgba,
            [1.0, 1.0, 0.0, 0.0],
            "üst koşunun köşeleri"
        );
        // Alt koşu üstünü aşıyor: köşeleri basamağın dışında, yani açıkta.
        assert_eq!(wrapped[3].rgba, [1.0; 4], "alt koşunun köşeleri");
    }

    /// Sonraki eşleşme daha üst bir satırda başlayabiliyor (5–6'ya sarılan
    /// eşleşmenin ardından 5. satırdaki ikincisi): liste sıralanmıyor,
    /// `continues`'la bölünüyor. Geçerli eşleşme kendi listesine gidiyor ve
    /// iki liste de içerik karesinde boşalıyor.
    #[test]
    fn search_runs_split_by_match_and_role() {
        let mut frame = search_frame();
        let color = LinearRgba::from_srgb(0x80, 0x80, 0x80);
        frame.push_search(
            &[
                search_run(5, 3, 6, false, false),
                search_run(6, 0, 1, false, true),
                search_run(5, 8, 9, true, false),
            ],
            color,
            color,
        );
        let [upper, lower] = frame.search_match_instances() else {
            panic!("iki parça beklendi: {:?}", frame.search_match_instances());
        };
        // Sarılan eşleşmenin iki koşusu çaprazdan değiyor: kaynıyor ama
        // örtüşmüyor, dört köşe de açıkta.
        assert_eq!((upper.rgba, lower.rgba), ([1.0; 4], [1.0; 4]));
        let [current] = frame.search_current_instances() else {
            panic!("tek geçerli parça: {:?}", frame.search_current_instances());
        };
        assert_eq!(current.pos, frame.pos(8, 5));
        assert_eq!(current.size, [18.0, 18.0]);
        assert_eq!(current.rgba, [1.0; 4]);
        assert!(frame.selection_instances().is_empty(), "seçime sızdı");
        assert!(
            frame.fill_search_match_instances().is_empty(),
            "banda sızdı"
        );

        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("sıfır olmayan hücre");
        frame.clear(metrics, CaretStyle::default());
        assert!(frame.search_match_instances().is_empty());
        assert!(frame.search_current_instances().is_empty());
    }

    /// Bandın koşuları bandın listelerine, fill-yerel satırla; ızgaranın
    /// listeleri boş kalıyor.
    #[test]
    fn fill_search_runs_stay_in_the_band() {
        let mut frame = search_frame();
        let color = LinearRgba::from_srgb(0x80, 0x80, 0x80);
        frame.set_fill_rows(2);
        frame.push_search(&[], color, color);
        frame.push_fill_search(&[
            search_run(0, 1, 2, false, false),
            search_run(1, 0, 0, true, false),
        ]);
        assert!(frame.search_match_instances().is_empty(), "ızgaraya sızdı");
        assert!(
            frame.search_current_instances().is_empty(),
            "ızgaraya sızdı"
        );
        let [matched] = frame.fill_search_match_instances() else {
            panic!("{:?}", frame.fill_search_match_instances());
        };
        assert_eq!(matched.pos, frame.pos(1, 0));
        let [current] = frame.fill_search_current_instances() else {
            panic!("{:?}", frame.fill_search_current_instances());
        };
        assert_eq!(current.pos, frame.pos(0, 1));
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("sıfır olmayan hücre");
        frame.clear(metrics, CaretStyle::default());
        assert!(frame.fill_search_match_instances().is_empty());
        assert!(frame.fill_search_current_instances().is_empty());
    }

    /// Sarılan girişte seçim satır başına bir koşu (032): ikinci koşu bir
    /// hücre aşağıda — giriş satırları bitişik, aralarında boşluk yok — ve
    /// köşe kararı ızgaranınki gibi komşu satıra bakıyor.
    #[test]
    fn a_dock_selection_across_rows_stacks_its_runs() {
        let mut frame = Frame::default();
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("sıfır olmayan hücre");
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_rows(3);
        frame.push_dock_selection(&[
            SelectionRun {
                row: 0,
                first: 5,
                last: 9,
            },
            SelectionRun {
                row: 1,
                first: 2,
                last: 7,
            },
        ]);
        let runs: Vec<&Instance> = frame
            .dock_selection_instances()
            .iter()
            .filter(|part| part.size[1] == 18.0)
            .collect();
        let [top, bottom] = runs[..] else {
            panic!("iki koşu beklendi: {runs:?}");
        };
        assert_eq!(bottom.pos[1] - top.pos[1], 18.0, "satırlar bitişik değil");
        // Tek parça şekil: iki koşunun örtüşen kenarındaki köşeler kare
        // (içbükey basamak), açıkta kalanlar yuvarlak.
        assert_eq!(top.rgba, [1.0, 1.0, 1.0, 0.0], "üst koşu");
        assert_eq!(bottom.rgba, [1.0, 0.0, 1.0, 1.0], "alt koşu");
    }

    #[test]
    fn the_dock_ground_spans_the_given_width() {
        // Genişlik argüman, çünkü `Frame` dokunun boyunu bilmiyor: listeler
        // hücre ızgarasından doğuyor, yüzey ise pencerenin **tamamını**
        // kaplamak zorunda. Zemin opak olmalı — kayma boyunca ızgaranın taşan
        // alt satırı onun altında kalıyor.
        let mut frame = Frame::default();
        frame.clear(grid(9, 18), CaretStyle::default());
        // Üst çizginin rengi ayrı bir alan (036): iki çizgi iki ayrı renkle
        // açılıyor ki biri ötekinin rengini alsa görünsün.
        frame.open_dock(BG, SUCCESS, CURSOR);
        assert_eq!(frame.dock_layout_px(), 36.0, "iki satır piksele çevrilmedi");

        let [ground, separator, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.pos, [0.0, 0.0], "zemin sol paydan başlamamalı");
        assert_eq!(ground.size, [500.0, 36.0]);
        assert_eq!(ground.rgba, BG.to_array());
        assert_eq!(ground.rgba[3], 1.0, "zemin saydam: taşan satır görünür");
        // Ayraç dock'un **en üst** pikselinde: ızgarayla sınır orası.
        assert_eq!(separator.pos, [0.0, 0.0]);
        assert_eq!(separator.size, [500.0, SEPARATOR_PX]);
        assert_eq!(
            separator.rgba,
            SUCCESS.to_array(),
            "üst çizgi kenarın rengi"
        );
        // İkinci ayraç iki satırın **arasında** ve ayracın renginde — uzak
        // oturumun kenar rengini almıyor. Paysız bu ölçüde satır arası boşluk
        // sıfır, yani çizgi tam satır sınırında.
        assert_eq!(divider.pos, [0.0, 18.0]);
        assert_eq!(divider.size, [500.0, SEPARATOR_PX]);
        assert_eq!(divider.rgba, CURSOR.to_array());

        // Yükleme sürerken (037 Karar 7) üst çizgi bir çubuk: zemini ayracın
        // renginde, soldan dolan kısmı kenarın renginde.
        frame.set_dock_progress(Some(2_500));
        let [_, base, fill, _] = frame.dock_ground(500.0);
        assert_eq!(base.size, [500.0, SEPARATOR_PX]);
        assert_eq!(base.rgba, CURSOR.to_array(), "çubuğun zemini ayraç");
        assert_eq!(fill.pos, [0.0, 0.0]);
        assert_eq!(fill.size, [125.0, SEPARATOR_PX]);
        assert_eq!(fill.rgba, SUCCESS.to_array(), "dolan kısım kenarın rengi");
        // Açılış her karede sıfırlıyor: çubuk yalnız söylendiği karede.
        frame.open_dock(BG, SUCCESS, CURSOR);
        let [_, base, fill, _] = frame.dock_ground(500.0);
        assert_eq!(base.rgba, SUCCESS.to_array());
        assert_eq!(fill.size[0], 0.0);

        // Dock'suz kare hiçbir yükseklik vermiyor: ikinci viewport kurulmaz.
        frame.clear(grid(9, 18), CaretStyle::default());
        assert_eq!(frame.dock_layout_px(), 0.0);
    }

    #[test]
    fn the_dock_breathes_above_and_below_its_rows() {
        // **Nefes payı** (012 phase-9, kullanıcı: "padding top yok resmen").
        // İki satırın üstünde ve altında pay var, kaynağı da sol payın ta
        // kendisi — ikinci bir tasarım sabiti uydurulmadı.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        frame.open_dock(BG, CURSOR, CURSOR);
        // 2×18 + 2×GUTTER + 1×(2×GUTTER) = 36 + 14 + 14 = 64. Satır arası
        // boşluk dış payın **iki katı**, çünkü ortasından bir çizgi geçiyor:
        // çizginin iki yanına birer pay düşünce dört boşluk da eşitleniyor.
        assert_eq!(frame.dock_layout_px(), 64.0, "pay yüksekliğe girmedi");

        // **Zemin payları da kaplıyor**: pay kadar eksik bir dikdörtgen,
        // kayma boyunca taşan ızgara satırını tam da nefes payında gösterirdi.
        let [ground, separator, _, divider] = frame.dock_ground(500.0);
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
        assert_eq!(frame.dock_layout_px() - (row1_top + 18.0), 7.0);
        assert_eq!(divider.size, [500.0, SEPARATOR_PX]);
        // Saç çizgisi payın **üstünde**, viewport'un tepesinde: ızgarayla
        // sınır orası ve payı onun üstüne koymak çizgiyi ızgaraya sokardı.
        assert_eq!(separator.pos, [0.0, 0.0]);

        // İçerik payın altından başlıyor: ilk satır y = pay.
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('x'),
            ..Cell::default()
        });
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(
            frame.dock_glyphs()[0].pos[1],
            f32::from(GUTTER),
            "içerik paya inmedi"
        );
        // İkinci satır bir hücre aşağıda, yani pay **bir kez** uygulanıyor.
        frame.clear(
            CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü"),
            CaretStyle::default(),
        );
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
    fn a_remote_dock_is_only_its_context_row() {
        // **Sıfır giriş satırı** (036 Karar 8): yerleşim yalnız bağlam
        // satırı — küçük yüz, küçük adım, üstünde satır arası boşluk yok
        // (ayrılacak bir giriş satırı yok) ve bant bir satır artı iki dış pay.
        // Üst çizgi bandın tepesinde ve kenarın renginde; ikinci ayraç sıfır
        // yükseklikli.
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü");
        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_input_rows(0);
        for col in 0..2 {
            frame.push_dock(Cell {
                col,
                row: 0,
                ch: Some('x'),
                ..Cell::default()
            });
        }
        let edge = bt_core::Theme::BATERI.info_linear();
        frame.open_dock(BG, edge, CURSOR);
        let band = 18.0 + 2.0 * f32::from(GUTTER);
        assert_eq!(frame.dock_layout_px(), band);
        assert_eq!(band_px(0, metrics), band);
        let glyphs = frame.dock_glyphs();
        assert_eq!(glyphs[0].pos[1], f32::from(GUTTER), "boşluk uygulandı");
        assert_eq!(
            glyphs[0].size,
            SizeClass::Small,
            "bağlam satırı büyük çizildi"
        );
        assert_eq!(
            glyphs[1].pos[0] - glyphs[0].pos[0],
            f32::from(metrics.context_cell_px()),
            "bağlam satırının adımı küçük yüzün ilerlemesi"
        );
        let [ground, separator, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, band]);
        assert_eq!(separator.pos, [0.0, 0.0]);
        assert_eq!(separator.rgba, edge.to_array());
        assert_eq!(divider.size[1], 0.0, "ayrılacak iki satır yok");
        // Sınamaların tek satırlık bağlamsız dock'u eski kuralıyla kalıyor.
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_rows(1);
        frame.push_dock(Cell {
            col: 0,
            row: 0,
            ch: Some('x'),
            ..Cell::default()
        });
        assert_eq!(frame.dock_glyphs()[0].size, SizeClass::Normal);
    }

    /// Üç giriş satırlık bir dock'un hücresi: `row` 0..3 giriş, 3 bağlam.
    fn dock_row(row: u16) -> Cell {
        Cell {
            col: 0,
            row,
            ch: Some('x'),
            ..Cell::default()
        }
    }

    #[test]
    fn a_three_row_band_stacks_its_input_above_the_context_row() {
        // **032 phase-2'nin sınama kancası** (`n = 3`): bant `n` giriş satırı
        // artı bağlam satırı; giriş satırları bitişik, boşluk ve ikinci saç
        // çizgisi yalnız giriş bloğu ile bağlam satırı arasında. @1x, 9×18
        // hücre, pay 7: `4·18 + 2·7 + 14 = 100` px.
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü");
        assert_eq!(band_px(3, metrics), 100.0);
        // Tek satırda PTY payının ta kendisi — ekran bit bit aynı.
        assert_eq!(band_px(1, metrics), dock_px(DOCK_ROWS, metrics));

        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_rows(4);
        for row in 0..4 {
            frame.push_dock(dock_row(row));
        }
        frame.set_dock_band(600.0, 2.0);
        frame.open_dock(BG, CURSOR, CURSOR);
        assert_eq!(frame.dock_layout_px(), 100.0);
        assert_eq!(
            frame.dock_band_px(),
            100.0,
            "dinlenen bant yerleşimden ayrıştı"
        );

        // Giriş satırları payın altından bitişik; bağlam satırı boşluğun
        // altında ve küçük sınıfta yalnız o.
        let ys: Vec<f32> = frame.dock_glyphs().iter().map(|g| g.pos[1]).collect();
        assert_eq!(ys, [7.0, 25.0, 43.0, 75.0]);
        let small: Vec<bool> = frame
            .dock_glyphs()
            .iter()
            .map(|g| g.size == SizeClass::Small)
            .collect();
        assert_eq!(small, [false, false, false, true]);
        // Bağlam satırı bandın **dibinde**: altında yalnız dış pay.
        assert_eq!(frame.dock_layout_px() - (ys[3] + 18.0), 7.0);

        // Tek saç çizgisi giriş bloğunun altında, boşluğun ortasında:
        // 7 + 3·18 + (14 − 1)/2 = 67,5 → 68. Giriş satırları arasında çizgi yok.
        let [ground, separator, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, 100.0]);
        assert_eq!(separator.pos, [0.0, 0.0]);
        assert_eq!(divider.pos, [0.0, 68.0]);
        assert!(divider.pos[1] > ys[2] + 18.0 && divider.pos[1] < ys[3]);

        // Fareye giden geometri: giriş bloğunun tepesi (600 − 100 + 7) ve
        // üç satır.
        assert_eq!(frame.dock_hit(), Some((507.0, 3)));
    }

    #[test]
    fn a_growing_band_keeps_its_rows_and_divider_on_the_bottom() {
        // **Dibe yaslı** (032): bant yarı yoldayken (fazla 2 hedefli, şu an
        // 0,5) zemin ve üst saç çizgisi animasyonun boyunda, hücreler ve ikinci
        // saç çizgisi yerleşimde — metin yerinde duruyor, yalnız bandın tepesi
        // yükseliyor. Izgaranın orijini bandın fazlası kadar yukarıda.
        let metrics = CellMetrics::new(9, 18, 9, GUTTER, 1).expect("ölçü");
        let mut frame = Frame::default();
        frame.clear(metrics, CaretStyle::default());
        frame.set_dock_rows(4);
        frame.set_origin_rows(5.0);
        frame.set_dock_band(600.0, 0.5);
        frame.open_dock(BG, CURSOR, CURSOR);
        // PTY payı 64 + yarım satırın yuvarlanmış pikseli 9.
        assert_eq!(frame.dock_band_px(), 73.0);
        assert_eq!(frame.origin_px(), 5.0 * 18.0 - 9.0);
        let [ground, _, _, divider] = frame.dock_ground(500.0);
        assert_eq!(ground.size, [500.0, 73.0]);
        // Çizgi pencere uzayında yerleşimdekiyle aynı pikselde: bandın
        // viewport'u `600 − 73`'te, yerleşiminki `600 − 100`'de.
        assert_eq!(600.0 - 73.0 + divider.pos[1], 600.0 - 100.0 + 68.0);
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
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_dock_top(64.0);

        // Bandın tamamen üstünde: ızgaranın yuvası.
        frame.push_caret([3.0, 2.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        assert!(
            frame.grid_caret().is_some(),
            "caret ızgara yuvasına düşmedi"
        );
        assert!(frame.dock_caret(64.0).is_none(), "caret iki yuvada birden");

        // Banda **değdiği** anda dock'un yuvası — henüz yarısı ızgarada olsa da.
        frame.move_caret([3.0, 3.5], TEXT, CURSOR, OPAQUE, true);
        assert!(frame.grid_caret().is_none(), "eski yuva temizlenmedi");
        let caret = frame.dock_caret(64.0).expect("caret dock yuvasında değil");
        // Instance pencere uzayında doğuyor (y = 3.5 × 16 = 56) ve dock
        // viewport'u 64'ten başlıyor: fark **negatif**, yani caret bandın
        // üstünde çiziliyor. Devrin ortasındaki kare tam olarak bu.
        assert_eq!(caret.pos[1], -8.0, "dock-yerel çeviri yanlış");

        // **Kesirli bant fazlasının `f32` hatası devir değil** (036): son
        // satırın alt kenarı bandın tepesini bir epsilon aşsa da ızgarada.
        frame.move_caret([3.0, 3.000_001], TEXT, CURSOR, OPAQUE, true);
        assert!(frame.grid_caret().is_some(), "epsilon caret'i dock'a itti");

        // `clear` bandı da sıfırlıyor: dock'u olmayan bir sonraki karede caret
        // yine ızgaranın yuvasına düşmeli.
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push_caret([3.0, 40.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
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
        let metrics = CellMetrics::new(10, 20, 10, 0, 2).expect("ölçü");

        frame.clear(metrics, CaretStyle::default());
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Block, true);
        let block = frame.grid_caret().expect("caret yok");
        assert_eq!((block.pos, block.size), ([10.0, 20.0], [10.0, 20.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 20.0, 20.0, 40.0]);

        // Alt çizgi hücrenin **dibinde**: 20 + 20 − 2.
        frame.clear(metrics, CaretStyle::default());
        frame.push_caret(
            [1.0, 1.0],
            TEXT,
            CURSOR,
            OPAQUE,
            CaretShape::Underline,
            true,
        );
        let under = frame.grid_caret().expect("caret yok");
        assert_eq!((under.pos, under.size), ([10.0, 38.0], [10.0, 2.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 38.0, 20.0, 40.0]);

        // Dikey çubuk hücrenin solunda ve tam boy.
        frame.clear(metrics, CaretStyle::default());
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Beam, true);
        let beam = frame.grid_caret().expect("caret yok");
        assert_eq!((beam.pos, beam.size), ([10.0, 20.0], [2.0, 20.0]));
        assert_eq!(frame.cursor_block().rect, [10.0, 20.0, 12.0, 40.0]);
    }

    #[test]
    fn caret_geometry_survives_a_zero_cell() {
        // `Frame::default()`'ın hücresi `(0.0, 0.0)` ve eski
        // `rule.clamp(1.0, 0.0)` `min > max` diye **panik** ediyordu
        // (`/code-review`, 014 kapı). Debug'da `push_caret`'in kendi
        // `debug_assert`'i önce düşüyor, ama `f32::clamp`'ın assert'i
        // **release'de de** var — yani sürüm derlemesinde bir pencereyi
        // öldürürdü. Geometri doğrudan sınanıyor, çünkü `push_caret`'e o
        // hâlde ulaşmanın yolu debug'da kapalı.
        let (pos, size) = caret_painted_rect([0.0, 0.0], (0.0, 0.0), CaretShape::Beam, 1.0);
        assert_eq!((pos, size), ([0.0, 0.0], [0.0, 0.0]));
        let (_, size) = caret_painted_rect([0.0, 0.0], (0.0, 0.0), CaretShape::Underline, 1.0);
        assert_eq!(size, [0.0, 0.0]);
    }

    #[test]
    fn the_caret_is_at_least_one_pixel_thick() {
        // Kural metriği sıfır gelse de ince caret görünür kalıyor.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(10, 20, 10, 0, 0).expect("ölçü"),
            CaretStyle::default(),
        );
        frame.push_caret([0.0, 0.0], TEXT, CURSOR, OPAQUE, CaretShape::Beam, true);
        assert_eq!(frame.grid_caret().expect("caret").size, [1.0, 20.0]);
    }

    #[test]
    fn a_motion_frame_keeps_the_caret_shape() {
        // Hareket karesi `bt-core`'a hiç gitmiyor, yani şekli bilmiyor. Alan
        // `Frame`'de olmasaydı beam ilk sönüp yanışta bloğa dönerdi — blink
        // (phase-2) tam bu yoldan geçecek.
        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(10, 20, 10, 0, 2).expect("ölçü"),
            CaretStyle::default(),
        );
        frame.push_caret([1.0, 1.0], TEXT, CURSOR, OPAQUE, CaretShape::Beam, true);
        frame.move_caret([2.0, 1.0], TEXT, CURSOR, OPAQUE, true);
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
        frame.clear(
            CellMetrics::new(8, 16, 8, 0, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        frame.set_dock_top(64.0);
        frame.push_caret(
            [3.0, 3.5],
            TEXT,
            CURSOR,
            OPAQUE,
            CaretShape::Underline,
            true,
        );
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
        frame.clear(grid(8, 16), CaretStyle::default());
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
                size: SizeClass::Normal,
                rgba: CURSOR.to_array(),
                wide: false,
                cluster: None,
            }
        );

        frame.clear(grid(8, 16), CaretStyle::default());
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
        frame.clear(grid(8, 16), CaretStyle::default());

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

        frame.clear(grid(8, 16), CaretStyle::default());
        assert_eq!(frame.rule_count(), 0);
    }

    /// Dock'un giriş satırında bir glyph'li hücre.
    fn typed_cell(col: u16, ch: char) -> Cell {
        Cell {
            col,
            row: 0,
            ch: Some(ch),
            fg: CURSOR,
            ..Default::default()
        }
    }

    fn fx(cell: Cell, kind: Kind) -> Fx {
        Fx {
            cell,
            kind,
            effect: 1,
            t: 0.5,
            seed: 0.0,
        }
    }

    #[test]
    fn an_arriving_glyph_is_drawn_by_its_effect_not_twice() {
        // `fade` statik glyph'in üstünde belirseydi hiçbir şey görünmezdi:
        // uçuştaki gelişin statik glyph'i çizilecek listeden çıkıyor, ama
        // `dock_glyphs`'in kendisinden değil — hareket karesi dock'u yeniden
        // basmıyor ve efekt bitince statik glyph geri gelmeli.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push_dock(typed_cell(2, 'l'));
        frame.push_dock(typed_cell(3, 's'));
        frame.set_dock_fx(
            [fx(typed_cell(3, 's'), Kind::Arrival)],
            &Clusters::default(),
            CURSOR,
        );
        let shown: Vec<char> = frame.dock_glyphs().iter().map(|g| g.ch).collect();
        assert_eq!(shown, ['l']);
        assert_eq!(frame.dock_arrivals().len(), 1);
        assert_eq!(frame.dock_arrivals()[0].glyph, frame.dock_glyphs[1]);
        // Efekt bitti (hareket karesi, liste boş): statik glyph dock yeniden
        // basılmadan geri geliyor.
        frame.set_dock_fx([], &Clusters::default(), CURSOR);
        let shown: Vec<char> = frame.dock_glyphs().iter().map(|g| g.ch).collect();
        assert_eq!(shown, ['l', 's']);
    }

    #[test]
    fn an_arrival_wears_the_static_glyphs_current_color() {
        // Vurgu uçuşta değişebiliyor (`l` kırmızı yazıldı, `s` gelince `ls`
        // yeşil oldu): geliş, yazıldığı anın rengini değil gizlediği statik
        // glyph'inkini taşımalı, yoksa efekt eski renkle bitip yeniye sıçrar.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        let recolored = LinearRgba::from_srgb(0x20, 0xc0, 0x40);
        let now = Cell {
            fg: recolored,
            ..typed_cell(2, 'l')
        };
        frame.push_dock(now);
        frame.set_dock_fx(
            [fx(typed_cell(2, 'l'), Kind::Arrival)],
            &Clusters::default(),
            CURSOR,
        );
        assert_eq!(frame.dock_arrivals().len(), 1);
        assert_eq!(frame.dock_arrivals()[0].glyph.rgba, recolored.to_array());
    }

    #[test]
    fn a_clustered_ghost_outlives_the_dock_table() {
        // 035 R4.1: hayaletin hücresi dock'un kare tablosunu gösteriyor ve o
        // tablo sonraki içerik karesinde temizleniyor; efekt dizgiyi kendi
        // tablosuna, `Frame` hayalet listesinin tablosuna kopyalıyor.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        let mut dock = Clusters::default();
        let ghost = Cell {
            wide: true,
            cluster: dock.push("🇹🇷"),
            ..typed_cell(4, '🇹')
        };
        let mut glyph_fx = GlyphFx::default();
        glyph_fx.apply(
            bt_core::DockEdit::Erase {
                row: 0,
                col: 4,
                ghosts: [ghost].into_iter().collect(),
                shift: 0,
            },
            crate::motion::Motion::default(),
            1,
            &dock,
        );
        dock.clear();
        dock.push("başka");
        for _ in 0..2 {
            // İkinci tur hareket karesi: tablo her yazımda yeniden kuruluyor.
            frame.set_dock_fx(glyph_fx.iter(), glyph_fx.clusters(), CURSOR);
            let ghosts = frame.dock_ghosts();
            assert_eq!(ghosts.len(), 1);
            let text = ghosts[0]
                .glyph
                .cluster
                .and_then(|id| frame.fx_clusters().get(id));
            assert_eq!(text, Some("🇹🇷"));
        }
    }

    #[test]
    fn an_arrival_without_its_static_glyph_finishes() {
        // Statik glyph'i bulunamayan geliş biter: satırda olmayan bir harf
        // belirmesin. Hayalet sorulmuyor — onun statik glyph'i zaten yok.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push_dock(typed_cell(2, 'l'));
        let mut glyph_fx = GlyphFx::default();
        let motion = crate::motion::Motion::default();
        let rows = 1;
        for edit in [
            bt_core::DockEdit::Arrive {
                row: 0,
                col: 2,
                cells: [typed_cell(2, 'l')].into_iter().collect(),
                shift: 0,
            },
            bt_core::DockEdit::Erase {
                row: 0,
                col: 5,
                ghosts: [typed_cell(5, 'q')].into_iter().collect(),
                shift: 0,
            },
            // Yanlış karakter: sütun tutuyor ama `x` orada değil.
            bt_core::DockEdit::Arrive {
                row: 0,
                col: 6,
                cells: [typed_cell(6, 'x')].into_iter().collect(),
                shift: 0,
            },
        ] {
            glyph_fx.apply(edit, motion, rows, &Clusters::default());
        }
        frame.suppress_dock(&mut glyph_fx);
        let left: Vec<(u16, Kind)> = glyph_fx.iter().map(|fx| (fx.cell.col, fx.kind)).collect();
        assert_eq!(left, [(2, Kind::Arrival), (5, Kind::Ghost)]);
        frame.set_dock_fx(glyph_fx.iter(), glyph_fx.clusters(), CURSOR);
        assert_eq!(frame.dock_ghosts().len(), 1);
        assert_eq!(frame.dock_arrivals().len(), 1);
        assert!(
            frame.dock_glyphs().is_empty(),
            "gelişin statik glyph'i çift çizildi"
        );
    }

    #[test]
    fn the_effects_live_and_die_with_the_content_frame() {
        // Dock'u olmayan karede (alternatif ekran) önceki karenin hayaleti
        // asılı kalmamalı.
        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.set_dock_fx(
            [fx(typed_cell(3, 's'), Kind::Ghost)],
            &Clusters::default(),
            CURSOR,
        );
        assert_eq!(frame.dock_ghosts().len(), 1);
        frame.clear(grid(8, 16), CaretStyle::default());
        assert!(frame.dock_ghosts().is_empty() && frame.dock_arrivals().is_empty());
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
