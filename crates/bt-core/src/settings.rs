//! Ayar modeli: `settings.toml`'un metninden [`Settings`]'e giden saf yol.
//!
//! Dosya sistemi **görmez**: metni okuyan, izleyen ve hatayı pencerede
//! gösteren `bt-shell`. Burada yalnız karar var — `child.rs`'in "saf karar +
//! ince sistem sarmalayıcısı" örüntüsü — ve varsayılanların tek sahibi
//! burası. Kararın kaydı `.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 1.
//!
//! **Hata kuralı tek:** metin TOML olarak ayrıştırılamıyorsa sonuç ayrı bir
//! değer ([`Settings::parse`]'ın `Err`'i) ve hiçbir alan uydurulmaz — ne
//! yapılacağı çağıranın kararı (açılışta varsayılanlar, canlı yenilemede
//! hiçbir şey). Ayrıştırılıyorsa her anahtar ya geçerli değerini ya
//! varsayılanını alır; kabul edilmeyen değer bir [`Diagnostic`] bırakır.
//! **Tek istisna `clipboard.osc52`:** kabul edilmeyen değeri varsayılanı
//! (açık) değil kapalıyı alır ([`Settings::parse_keeping`]'in doc'u).
//! **Bilinmeyen anahtar ve bölüm sessizce yoksayılır:** sonraki setlerin
//! anahtarı (`[motion] keypress`) bugünkü sürümde tanı üretmemeli.
//!
//! Ayrıştırıcı önceki ayarları yalnız kabul edilmeyen değerin yerine geçecek
//! değer olarak görür ([`Settings::parse_keeping`], kayıt anı); fark almak
//! çağıranın işi ([`Settings::changes`]).
//!
//! Tanı tipi ve TOML yardımcıları tema dosyasının ayrıştırıcısıyla (`theme`)
//! ortak: iki dosyanın hata dili aynı olsun.

use std::fmt;

use toml_edit::{Document, Item, TableLike};

use crate::session::{Osc52, TerminalOptions};

/// Kaydırma geçmişinin tavanı: **alacritty uygulamasının** sınırı.
///
/// Kaynak `alacritty/src/config/scrolling.rs` → `MAX_SCROLLBACK_LINES =
/// 100_000`; aşan değeri ayar okurken reddediyor. Sınır uygulamada,
/// `alacritty_terminal`'da **değil** — `Term` `scrolling_history`'yi
/// kırpmadan alıyor, yani tavanı koymak bizim işimiz. İçe aktarılamaz (o
/// crate bağımlılığımız değil), sayı kaynağıyla birlikte buraya kopyalandı;
/// ölçülmüş bir bellek bütçesi değil.
///
/// `pub(crate)`: tavan kullanıcı girdisinin kuralı, `Session`'ın değişmezi
/// değil — `SessionOptions.scrollback`'i kırpan başka bir kapı yok ve olması
/// da gerekmiyor, oraya giden tek değer bu ayrıştırıcıdan geçiyor.
pub(crate) const SCROLLBACK_MAX: usize = 100_000;

/// `[appearance] theme`'in ayrılmış değeri: temayı sistemin açık/koyu
/// görünümü seçer ([`Settings::theme_for`]).
///
/// Bir tema adı **değil** — `themes/system.toml` bu yüzden seçilemez ve
/// `light_theme`/`dark_theme` bu değeri kabul etmez (kendi kendine dönen bir
/// seçim olurdu).
///
/// `pub`: View ▸ Theme ▸ Match System bu değeri yazıyor
/// ([`Settings::with_theme`]); okuyan taraf [`Settings::follows_system`]'e
/// bakar, değeri karşılaştırmaz.
pub const SYSTEM_THEME: &str = "system";

/// `[font]`: hücre ölçüsünü ve glyph'leri belirleyen iki değer.
///
/// Ayrı bir tip, çünkü renderer onu **bütün olarak** tutuyor ve açılış
/// değerini buradan alıyor: varsayılan puntonun tek sahibi bu tipin
/// `Default`'u. Renderer'ın kendi sabiti olsaydı süreli koşunun (ayar hiç
/// okunmuyor) fontu ile dosyasız kullanıcınınki iki ayrı sayıya bağlanırdı.
///
/// `Eq` yok: punto `f64`. Ayrıştırıcı yalnız sonlu ve pozitif değer
/// bırakıyor, yani karşılaştırmaya NaN girmiyor.
#[derive(Clone, Debug, PartialEq)]
pub struct FontOptions {
    /// Aile adı; `None` → zincir (SF Mono, yoksa Menlo). Makinede olup
    /// olmadığı `bt-atlas`'ın sorusu — burada yalnız metin.
    pub family: Option<String>,
    /// Mantıksal punto. Ölçekle çarpılmış hâlinin kırpması `bt-atlas`'ta ve
    /// **sessiz**; buradaki kural yalnız "sonlu ve sıfırdan büyük".
    pub size: f64,
    /// Satır yüksekliği çarpanı: hücre, fontun kendi
    /// `ascent + descent + leading`'inin bu katı olur ve fazlalık glyph'in
    /// **altına ve üstüne eşit** dağılır (taban çizgisi de o kadar iniyor).
    ///
    /// Taban `1.0` ve **altına inilmiyor**: fontun istediğinden kısa bir hücre
    /// `g j p q y` altındaki kapsamayı kırpardı ve bunun kendi bekçisi var
    /// (`bt-atlas`'ta `descender_fits_in_the_cell`). Ayarın bir bekçiyi
    /// delmesi, ayarın kendisinden önemli.
    pub line_height: f64,
}

impl Default for FontOptions {
    /// 13 punto 006'ya kadar `bt-gpu`'nun `POINT_SIZE` sabitiydi: seçilmiş
    /// bir varsayılan, ölçülmüş bir sayı değil.
    fn default() -> Self {
        Self {
            family: None,
            size: 13.0,
            line_height: 1.0,
        }
    }
}

/// `[motion] cursor_motion`: imlecin hücreler arasında nasıl gittiği.
///
/// Ayar modelinde yaşıyor ([`FontOptions`] ve `Osc52` emsali) ama tüketicisi
/// `bt-gpu`: `bt-shell` çözülmüş değeri renderer'ın ritmine veriyor. Buradaki
/// tek bilgi **hangi stil**; sürelerin ve yay katsayılarının sahibi
/// `bt_gpu::motion` — sayıların ayar modelinde durması onları iki yerden
/// değiştirilebilir kılardı.
///
/// `Default` **`Spring`** (008 Karar 6): set'in ürün gerekçesi "the reference'i
/// ekranda tanıtan üç şeyden biri" ve varsayılanı `Snap` yapmak özelliği
/// kapalı sevk etmek olurdu. Varsayılanın tek sahibi burası olduğu için
/// hermetik süreli koşu da (ayar dosyası okumuyor) bu değeri alıyor —
/// `make duman`'ın `hareket > 0` gerekliliği tam buna yaslanıyor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorMotion {
    /// Anında: imleç hedef hücrede doğar, animasyon hiç başlamaz.
    Snap,
    /// Sabit süre, taşma yok — mesafeden bağımsız.
    Ease,
    /// Kritik sönümlü yay; süre mesafeyle büyür.
    #[default]
    Spring,
}

impl CursorMotion {
    /// Ayar dosyasındaki yazılışı — tanı metninin "using …" yarısı buradan.
    ///
    /// Ayrıştırıcının kabul ettiği dizgilerle **aynı** olmak zorunda
    /// ([`cursor_motion`]): tanı kullanıcıya geçerli bir değer göstermeli.
    fn name(self) -> &'static str {
        match self {
            Self::Snap => "snap",
            Self::Ease => "ease",
            Self::Spring => "spring",
        }
    }
}

/// `[terminal] cursor_blink`: imleç yanıp söner mi.
///
/// **Üç değerli, çünkü iki soru var:** uygulamanın isteği dinlensin mi
/// ([`Self::Auto`]) yoksa kullanıcının dediği her şeyi ezsin mi
/// ([`Self::On`]/[`Self::Off`]). İki değerli olsaydı `false` "uygulama da
/// söndüremesin" mi yoksa "varsayılan kapalı" mı demek olduğu belirsiz kalırdı
/// — ve vi modunda `\e[5 q` gönderen bir zsh kurulumu, kullanıcı kapalı
/// yazmışken imleci yakardı.
///
/// **Varsayılan [`Self::Off`]** ve bu bir ürün kararı: yanıp sönen imleç
/// pencereyi **kalıcı olarak boşta-değil** yapıyor (saniyede iki kare) ve bu
/// depo on üç set boyunca "boşta sıfır kare"yi savundu. Bedeli kullanıcının
/// **seçtiği** bir şey olmalı, sessizce gelen bir varsayılan değil. Hermetik
/// süreli koşu da (ayar dosyası okumuyor) bu değeri alıyor, yani `make
/// duman`'ın sessizlik kapısı yapısal olarak bağışık.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorBlink {
    /// Uygulamanın dediği: DECSCUSR'ın tek sayıları ve DECSET 12 açar.
    Auto,
    /// Her zaman söner; uygulamanın `\e[2 q`'su bile durduramaz.
    On,
    /// Hiç sönmez; uygulamanın `\e[5 q`'su bile başlatamaz.
    #[default]
    Off,
}

impl CursorBlink {
    /// Ayar dosyasındaki yazılışı; ayrıştırıcının kabul ettikleriyle **aynı**
    /// olmak zorunda ([`cursor_blink`]).
    fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::On => "on",
            Self::Off => "off",
        }
    }

    /// Uygulamanın söylediğiyle kullanıcının dediğini birleştirir — **tek
    /// yer**, `Session::frame` onu buradan soruyor.
    pub(crate) fn resolve(self, requested: bool) -> bool {
        match self {
            Self::Auto => requested,
            Self::On => true,
            Self::Off => false,
        }
    }
}

/// `[terminal] cursor`: imlecin **şekli** — DECSCUSR'ın üç biçimi.
///
/// Bölüm `[motion]` **değil**: şekil hareket değil, terminalin durum
/// makinesinin bir parçası. `[terminal]`'da duruyor ve aynı bölümde değer
/// [`TerminalOptions`] ile `Session`'a iniyor, orada alacritty'nin
/// `default_cursor_style`'ı oluyor.
///
/// **O ayna betimleyici, buyurucu değil** (016): `[clipboard] osc52` de
/// `TerminalOptions`'a giriyor, `[terminal] cursor_radius` ise girmiyor.
/// Bölüm kullanıcının **neyi ayarladığını** adlandırıyor, hangi struct'ın
/// taşıdığını değil. Referans anahtarı kendi
/// `[typography]`'sinde tutuyor (`docs/ARASTIRMA.md` → İmleç); adını aldık,
/// yerini değil.
///
/// **Bu ayar yalnız varsayılanı söyler.** Uygulama DECSCUSR (`\e[5 q`) ya da
/// OSC 50 ile şekli değiştirebilir ve o sözü dinleniyor: vim insert modda
/// çubuk isterse çubuk olur. Kullanıcının burada yazdığı şey, kimse bir şey
/// istemediğindeki hâl.
///
/// `Hidden` ve `HollowBlock` **temsil edilmiyor**: ilki bir şekil değil
/// görünürlük (`\e[?25l`) ve `Cursor::visible` onu zaten taşıyor; ikincisi
/// odak kaybının hâli ve odak bugün sınırdan geçmiyor
/// (`.tasks/014-imlec-stilleri/plan.md` → Kapsam Dışı).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaretShape {
    /// Hücreyi dolduran blok — alacritty'nin de varsayılanı.
    #[default]
    Block,
    /// Hücrenin altında ince bir çizgi.
    Underline,
    /// Hücrenin solunda ince bir dikey çubuk.
    Beam,
}

/// İmlecin **köşe yarıçapı** varsayılanı, hücre yüksekliğinin oranı.
///
/// **Varsayılanların tek sahibi burası** ve bu bir tesisat kararı: `bt-gpu`
/// aynı sabiti **import ediyor** (`Frame::default()` ve piksel bekçileri).
/// İki literal olsaydı bekçiler kendi tutarlılığını sınar, sevk edilen imleç
/// başka ölçüde olsa da yeşil geçerdi — sessiz kırılmanın tarifi
/// (`/plan-review`, 016). Emsal [`FontOptions`]'ın doc'u: *"renderer'ın kendi
/// sabiti olsaydı süreli koşunun fontu ile dosyasız kullanıcınınki iki ayrı
/// sayıya bağlanırdı"*.
///
/// Değer **seçilmiş, ölçülmemiş** ve 015'te iki tur gözle indi (0.18 → 0.10):
/// blok caret hücre genişliğinden kısa ve daha büyük bir yarıçap onu
/// dikdörtgen olmaktan çıkarıp hapa çeviriyordu.
pub const CURSOR_RADIUS: f64 = 0.10;

/// İmlecin **gölge gücü** varsayılanı; `1.0` = tasarımın kendi ölçüsü.
///
/// **Tek sayı, iki değil** (`/plan-review`, 016): hale payı ile alfası 015'te
/// aynı iki göz turunda **aynı yönde** indi (pay 1.0 → 0.5 → 0.4, alfa
/// 0.35 → 0.14 → 0.10), yani kullanıcı iki eksende değil tek histe gezindi.
/// Ayrı anahtarlar ayrıca anlamsız hâl üretirdi: `pay = 2, alfa = 0` hiçbir
/// şeyin halesini boyayan bir dörtlü demek.
///
/// `bt-gpu`'daki iki sabit **taban olarak yerinde kalıyor**; bu yalnız onların
/// çarpanı, yani "ikinci bir tasarım sabiti yok" kuralı korunuyor.
pub const CURSOR_GLOW: f64 = 1.0;

/// Blink'in **yarım periyodu** varsayılanı, saniye.
///
/// **Varsayılanın tek sahibi burası** ([`CURSOR_RADIUS`] ile aynı gerekçe):
/// `bt-gpu` bunu import ediyor. Değer **seçilmiş, ölçülmemiş** — hedefi
/// "yanıp söndüğü fark edilsin ama göz yormasın" ve bedeli doğrusal: 250 ms
/// saniyede dört kare eder.
pub const CURSOR_BLINK_INTERVAL: f64 = 0.5;

/// Blink periyodunun kabul aralığı, saniye — **seçilmiş, ölçülmemiş**.
///
/// Alt uç tavanı durduruyor: 50 ms'lik bir yarım periyot saniyede 20 kare
/// eder ve altına inmek terminali stroboskopa çevirirdi. **Kapı bunu
/// göremiyor** ve bu yazılı olsun: süreli koşu ayar dosyasını hiç okumuyor,
/// blink varsayılanı da kapalı, yani bozuk bir periyot `make duman`'ın
/// `sessiz=` katını **hiçbir koşulda** kızartmaz (014 `teslim.md`: "koruma
/// bir jeton değil varsayılanın kendisi"). Tek koruma bu aralık.
const CURSOR_BLINK_RANGE: std::ops::RangeInclusive<f64> = 0.05..=5.0;

/// Yarıçabın kabul aralığı; yarım = hücrenin yarısı, ötesi anlamsız.
const CURSOR_RADIUS_RANGE: std::ops::RangeInclusive<f64> = 0.0..=0.5;

/// Gölge çarpanının kabul aralığı — **seçilmiş, ölçülmemiş**.
///
/// Çarpan **iki ekseni birden** ölçekliyor ve tavanın gerekçesi ikisini de
/// saymalı (`/code-review`): 3.0'da alfa 0.30 (015'te reddedilen 0.35'in
/// altında) ama yayılma `1.2 × gutter_px`, yani `CARET_GLOW_RATIO`'nun
/// doc'unda "gölge değil neon" diye kaydedilen "sol payın tamamı"nın **üstü**.
///
/// Tavan yine de orada, çünkü **varsayılanın zevki ile tavanın işi ayrı**:
/// reddedilen şey o görüntünün *varsayılan* olmasıydı. Tavan kullanıcının
/// açıkça seçtiği uca yer bırakıyor ve tek görevi sınırsızlığı kesmek.
const CURSOR_GLOW_RANGE: std::ops::RangeInclusive<f64> = 0.0..=3.0;

/// `[terminal] cursor_unfocused`: odakta olmayan pencerede imleç ne olsun.
///
/// 015 odak kaybında imlecin içini boşaltıyor; bu anahtar onu kapatıyor.
/// **Blink'e dokunmuyor** — odakta blink'in durması ayrı bir sinyal ve ayrı
/// bir karar (015 R7.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnfocusedCaret {
    /// İçi boşalır: çerçeve kalır, dolgu gider. Bugünkü davranış.
    #[default]
    Hollow,
    /// Hiç değişmez; odaksızlığın tek işareti blink'in durması.
    Solid,
}

impl UnfocusedCaret {
    /// Ayar dosyasındaki yazılışların **tek listesi**: ayrıştırıcı da
    /// [`Self::name`] de buradan okuyor.
    ///
    /// İki yerde yazılsaydı bir varyantın yazılışını değiştirmek, kullanıcıya
    /// **ayrıştırıcının reddettiği** bir değer öneren bir tanı üretirdi —
    /// `docs/YOL-HARITASI.md`'nin borcu bu kusuru adıyla sayıyor ("her
    /// enum'un `name()`'i de ayrıştırıcının kollarıyla elle eşleşiyor") ve
    /// yeni bir enum'da onu tekrarlamanın gerekçesi yok.
    const NAMES: &'static [(&'static str, Self)] =
        &[("hollow", Self::Hollow), ("solid", Self::Solid)];

    /// Ayar dosyasındaki yazılışı.
    fn name(self) -> &'static str {
        Self::NAMES
            .iter()
            .find(|(_, value)| *value == self)
            .map_or("hollow", |(name, _)| *name)
    }
}

/// `[terminal] cursor_radius` ve `cursor_glow`: imlecin **çizim** sayıları.
///
/// `TerminalOptions`'a **girmiyor** ve `Session` görmüyor: ikisi de terminalin
/// durumu değil, saf boyama. Yol `cursor_motion` emsali —
/// `Settings::changes` farkı buluyor, `bt-shell` `bt_gpu::DisplayLink`'e
/// iletiyor, kayıt anında uygulanıyor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretStyle {
    /// Köşe yarıçapı, hücre **yüksekliğinin** oranı.
    ///
    /// `f64`, `f32` değil ve sebebi **tanı metni**: `ranged_float` geri
    /// düşülen değeri mesaja basıyor ve `f64::from(0.10f32)`
    /// `0.10000000149011612` ediyor — kullanıcı yazdığı sayıyı değil float
    /// gürültüsünü görürdü (`/code-review`). Daraltma `bt-gpu` sınırında,
    /// kare başına değil bir kez.
    pub radius_ratio: f64,
    /// Gölgenin gücü; `0.0` kapalı, `1.0` tasarımın kendi ölçüsü.
    pub glow: f64,
    /// Odakta olmayan pencerede imlecin hâli.
    pub unfocused: UnfocusedCaret,
}

impl Default for CaretStyle {
    fn default() -> Self {
        Self {
            radius_ratio: CURSOR_RADIUS,
            glow: CURSOR_GLOW,
            unfocused: UnfocusedCaret::default(),
        }
    }
}

impl CaretShape {
    /// Ayar dosyasındaki yazılışı; ayrıştırıcının kabul ettikleriyle **aynı**
    /// olmak zorunda ([`caret_shape`]).
    fn name(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Underline => "underline",
            Self::Beam => "beam",
        }
    }
}

/// `[motion] reduce_motion`: animasyonların kısılıp kısılmayacağı.
///
/// **`bool` değil** ve sebebi bu dosyanın kendi kuralı: en olası seçim
/// "sistemi izle" ve `bool`'da onu ifade etmenin tek yolu anahtarı **silmek**
/// olurdu — burada anahtar silinmiyor, bilinmeyen anahtar bile korunuyor.
/// Üç değerli dizgi üçünü de yazılı tutuyor.
///
/// Sistemin cevabını okumak `bt-shell`'in işi (`NSWorkspace`); buradaki tek
/// bilgi kullanıcının **hangisini** istediği. Üçünün tek `bool`'a indiği yer
/// de orası, çünkü `bt-gpu` AppKit görmüyor (`CLAUDE.md` → katman tablosu).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReduceMotion {
    /// macOS'un Hareketi Azalt ayarını izle.
    #[default]
    System,
    /// Sistem ne derse desin kıs.
    On,
    /// Sistem ne derse desin kısma.
    Off,
}

impl ReduceMotion {
    /// Ayar dosyasındaki yazılışı; [`CursorMotion::name`] ile aynı gerekçe.
    fn name(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

/// `[shell] integration`: kabuğa sarmalayıcımız kurulsun mu.
///
/// Anahtarın anlamı dar ve bilerek öyle: **"sarmalayıcıyı kurma"**. İşaretleri
/// ayrıştırmak her hâlde serbest kalıyor — başka bir aracın (ya da SSH'ın öte
/// tarafındaki bir kurulumun) bastığı gerçek OSC 133'ü görmek zarar değil
/// kazanç, ve kapatmanın gerekçesi de o değil.
///
/// **Kayıt anında uygulanmayan tek ayar** ve bu, "ayar kayıt anında
/// uygulanır" sözleşmesinin ilk istisnası: sarmalayıcı kabuğun **doğuşunda**
/// kuruluyor, dosya kaydedildiğinde kabuk çoktan doğmuş oluyor. Bu yüzden
/// [`Changes`]'e kol takılmıyor ve `docs/AYARLAR.md` anahtarın **sonraki
/// oturumda** geçerli olduğunu kendi satırında söylüyor (009 Karar 5).
///
/// Tüketicisi `bt-shell` ([`CursorMotion`] emsali): kararı o veriyor, çünkü
/// hangi kabuğun koştuğunu ve betiğin nerede olduğunu gören taraf o.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShellIntegration {
    /// Tanıdığımız bir kabuksa sarmalayıcı kurulur; değilse hiçbir şey olmaz.
    ///
    /// Aynalayabilen kabukta (bugün zsh) dock da açılır ve prompt terminalin
    /// olur.
    #[default]
    Auto,
    /// Sarmalayıcı kurulur ama **giriş satırı alınmaz**: komut blokları ve
    /// işaretler çalışır, dock açılmaz, prompt kullanıcınındır.
    ///
    /// **Uydurulmuş bir kademe değil, yapının kendisi.** Dock ZLE'nin aynasına
    /// bağlı; bash (`--rcfile`) ve fish (`vendor_conf.d`) betikleri
    /// doğduğunda o kabuklarda işaretler olacak ama dock olmayacak. Bu değer
    /// yalnız zsh kullanıcısına aynı hâli **seçme** hakkı veriyor.
    ///
    /// 012 phase-10'da `[shell] prompt`'un yerine geldi: ayrı anahtar ekranda
    /// **iki prompt** üretiyordu (kullanıcınınki ızgarada, dock'unki altta) ve
    /// caret ikisi arasında sıçrıyordu. "Prompt kullanıcının" demek zaten
    /// "satır ızgarada" demek.
    Blocks,
    /// Sarmalayıcı hiç kurulmaz.
    Off,
}

impl ShellIntegration {
    /// Ayar dosyasındaki yazılışı; tanı metni bunu basıyor.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Blocks => "blocks",
            Self::Off => "off",
        }
    }

    /// Sarmalayıcı kurulacak mı — `auto` ve `blocks` için evet.
    pub fn installs_wrapper(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Dock açılacak mı. Kabuğun aynalayıp aynalayamadığı **ayrı** bir soru ve
    /// onu `bt-shell` yanıtlıyor; bu yalnız kullanıcının seçimi.
    pub fn wants_dock(self) -> bool {
        matches!(self, Self::Auto)
    }
}

/// Emekli anahtarlar: dosyada durmaya devam eder, **okunmaz**, ve görülünce
/// tanı bırakır.
///
/// Deponun kuralı "bilinmeyen anahtar korunur, anahtar silinmez"; emeklilik o
/// kuralın üçüncü hâli. Sessizce yok saymak yanlış olurdu — kullanıcı yazdığı
/// satırın bir işe yaradığını sanır; silmek de yanlış, çünkü dosyaya
/// dokunmuyoruz. Tanı ikisinin arası: satır yerinde kalıyor ve alt başlık
/// nereye bakılacağını söylüyor.
const RETIRED: &[(&str, &str)] = &[(
    "prompt",
    // 012 phase-10: `[shell] prompt` ayrı anahtar olarak ekranda iki prompt
    // üretiyordu; seçim `integration`'ın üçüncü değerine taşındı.
    "`shell.prompt` is no longer read; use `shell.integration = \"blocks\"` \
     to keep your own prompt",
)];

/// Kullanıcının değiştirebildiği her şey — ayrıştırılmış ve doğrulanmış.
///
/// Alanlar `pub`: tip bir kayıt, davranış taşımıyor. Değerin geçerliliğini
/// kuran yol [`Settings::parse`]; elle kurulan bir `Settings` bu kuralları
/// atlayabilir ve bu bilerek serbest (sınamalar böyle kuruyor).
///
/// `Eq` yok: [`FontOptions::size`] `f64`.
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// `[terminal] scrollback`: geçmişte tutulan satır, `0..=SCROLLBACK_MAX`.
    pub scrollback: usize,
    /// `[terminal] cursor`: imlecin **varsayılan** şekli; uygulama DECSCUSR
    /// ile üstüne yazabilir ([`CaretShape`]).
    pub cursor: CaretShape,
    /// `[terminal] cursor_blink`: imleç yanıp söner mi ([`CursorBlink`]).
    pub cursor_blink: CursorBlink,
    /// `[terminal] cursor_radius` + `cursor_glow` + `cursor_unfocused`:
    /// imlecin çizim sayıları ([`CaretStyle`]). `TerminalOptions`'a girmiyor.
    pub caret: CaretStyle,
    /// `[terminal] cursor_blink_interval`: blink'in **yarım periyodu**, saniye.
    ///
    /// [`Self::caret`]'ten ayrı alan, çünkü varış yeri ayrı: çizim sayıları
    /// `Frame`'e, bu `bt_gpu::blink`'e gidiyor. `Changes::caret` ikisini
    /// birden taşıyor — emsal `Changes::motion`'ın iki anahtarı.
    pub blink_interval: f64,
    /// `[appearance] theme`: [`SYSTEM_THEME`] ya da tema **adı** —
    /// `themes/{ad}.toml` ya da gömülü bir tema. Ad biçim olarak geçerli (boş
    /// değil, `/` yok); var olup olmadığı dosya sistemi ister ve `bt-shell`'in
    /// ad çözümünde.
    pub theme: String,
    /// `[appearance] light_theme`: `theme = "system"` iken açık görünümün
    /// teması. `theme`'den **ayrı** anahtar: menüden sabit bir tema seçmek
    /// yalnız `theme`'i yazar ([`Settings::with_theme`]) ve kullanıcının
    /// açık/koyu çifti yerinde kalır.
    pub light_theme: String,
    /// `[appearance] dark_theme`: `theme = "system"` iken koyu görünümün
    /// teması.
    pub dark_theme: String,
    /// `[font] family` ve `size`.
    pub font: FontOptions,
    /// `[clipboard] osc52`: `"copy"` ya da `"off"`.
    pub osc52: Osc52,
    /// `[motion] cursor_motion`: imlecin kayma stili.
    pub cursor_motion: CursorMotion,
    /// `[motion] reduce_motion`: animasyonlar kısılsın mı.
    pub reduce_motion: ReduceMotion,
    /// `[shell] integration`: kabuk sarmalayıcısı kurulsun mu. **Sonraki
    /// oturumda** geçerli ([`ShellIntegration`]).
    pub shell_integration: ShellIntegration,
}

impl Default for Settings {
    /// Dosya yokken ve anahtar eksikken geçerli olan değerler.
    ///
    /// `scrollback` 006'ya kadar `bt-shell`'in `SCROLLBACK` sabitiydi; değer
    /// aynı kaldı, sahibi buraya taşındı. Tema sistemin görünümünü izler:
    /// açıkta gömülü `bateri-light`, koyuda gömülü `bateri`.
    ///
    /// OSC 52 **açık** (`copy`): ssh'taki vim'in kopyasının yerel panoya
    /// gelmesi terminalden beklenen davranış ve alacritty'nin de varsayılanı.
    /// Bedeli: arka planda koşan uzak bir program da panoya yazabilir; okuyamaz.
    /// Kullanılamayan bir dosyada açılıştaki değer bu değil
    /// ([`Settings::for_unusable_file`]).
    fn default() -> Self {
        Self {
            scrollback: 10_000,
            cursor: CaretShape::default(),
            cursor_blink: CursorBlink::default(),
            caret: CaretStyle::default(),
            blink_interval: CURSOR_BLINK_INTERVAL,
            theme: SYSTEM_THEME.to_owned(),
            light_theme: "bateri-light".to_owned(),
            dark_theme: "bateri".to_owned(),
            font: FontOptions::default(),
            osc52: Osc52::Copy,
            cursor_motion: CursorMotion::default(),
            reduce_motion: ReduceMotion::default(),
            shell_integration: ShellIntegration::default(),
        }
    }
}

/// Ayrıştırılabilen bir dosyanın sonucu: değerler ve kabul edilmeyenler.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub settings: Settings,
    /// Dosyadaki sırayla değil **anahtar okuma sırasıyla**; boşsa dosya
    /// temiz.
    pub diagnostics: Vec<Diagnostic>,
}

/// Bir ayarın neden kabul edilmediği.
///
/// Metin **İngilizce**: pencerenin alt başlığında görünüyor, yani bir UI
/// dizgisi (`CLAUDE.md` → Dil); stderr aynı metnin kopyasını basıyor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// Noktalı anahtar yolu (`terminal.scrollback`); sözdizimi hatasında
    /// `None`.
    pub key: Option<&'static str>,
    /// 1'den başlayan satır; ayrıştırıcı konum vermediyse `None`.
    pub line: Option<usize>,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    /// Tek satır: pencerenin alt başlığı başlıkla **aynı satırda** çiziliyor
    /// (araç çubuksuz pencere, 007 phase-1 göz kontrolü), uzun ve çok
    /// satırlı bir metin orada kesilirdi.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "line {line}: ")?;
        }
        f.write_str(&self.message)
    }
}

impl Settings {
    /// "Settings…"ın dosya yokken yarattığı `settings.toml`: her anahtar
    /// açıklamasıyla ve varsayılan değeriyle.
    ///
    /// Sahibi varsayılanların sahibi, yani burası: ayrıştırılınca tanısız
    /// [`Settings::default`] verdiği sınamayla bağlı. Anahtarlar **yazılı**,
    /// yorumda değil — kullanıcı değeri yerinde değiştiriyor, menüden tema
    /// seçimi de satırı yerinde yazıyor ([`Settings::with_theme`]). Bedeli:
    /// varsayılan bir gün değişirse şablonu açmış kullanıcı eskisinde kalır.
    /// Yalnız varsayılanı olmayan `family` yorumda bir örnek.
    ///
    /// Bölüm başlıkları yorumda değil: yorumu kaldırılan bir anahtar başlıksız
    /// kalsaydı kök anahtar olur ve tanınmayan anahtar diye **sessizce**
    /// yoksayılırdı.
    ///
    /// Metin İngilizce: kullanıcının açtığı dosya bir UI dizgisi
    /// (`CLAUDE.md` → Dil).
    pub const TEMPLATE: &str = r#"# bateri settings. Changes apply as soon as you save this file.
# A key you delete goes back to its default. Values are case-sensitive; one that
# is not understood leaves its key alone and says so under the title — except
# clipboard.osc52, which turns off instead.

[terminal]
# 0 to 100000. Lines of history kept above the screen.
scrollback = 10000
# "block" | "underline" | "beam". The cursor's default shape: block fills the
# cell, underline sits below it, beam stands at its left edge. Programs such as
# vim may ask for a different shape while they run; this is the shape when none
# is asked for.
cursor = "block"
# "auto" | "on" | "off". Whether the cursor blinks: auto blinks until a program
# asks it to stop (vim in normal mode does), on blinks whatever the program
# says, off never blinks. Blinking asks for two frames a second, so it is off
# unless you choose it; with it on, it stops on its own 15 seconds after the
# window last drew anything and comes back with the next output or keystroke.
cursor_blink = "off"
# 0.0 to 0.5. How round the cursor's corners are, as a fraction of the cell's
# height: 0 is a sharp rectangle, 0.5 rounds a block into a stadium. It scales
# with the font size, so a larger point size keeps the same look.
cursor_radius = 0.10
# 0.0 to 3.0. How strong the soft shadow around the cursor is: 0 turns it off,
# 1 is the designed amount. It scales both how far the shadow reaches and how
# dark it is, because those two are one feeling, not two.
cursor_glow = 1.0
# "hollow" | "solid". What the cursor does while the window is not focused:
# hollow empties it to an outline, solid leaves it as it is. Either way a
# blinking cursor stops blinking until the window is focused again.
cursor_unfocused = "hollow"
# 0.05 to 5.0. Half the blink period in seconds: the cursor stays lit this
# long, then dark this long. Shorter costs more frames — 0.25 asks for four a
# second — and 0.5 is a blink you notice without it tiring the eye.
cursor_blink_interval = 0.5

[appearance]
# "system" or a theme name. "system" follows the macOS light/dark appearance;
# any other value is a theme used in both — a file themes/NAME.toml next to
# this one, or a built-in theme, "bateri" (dark) or "bateri-light" (light).
theme = "system"
# Theme names, used while theme = "system".
light_theme = "bateri-light"
dark_theme = "bateri"

[font]
# A family name as shown in Font Book. Without it bateri uses SF Mono, or
# Menlo when SF Mono is not installed — SF Mono ships with Xcode, so it is not
# on every machine. A character the family lacks is drawn from the system font
# chain when it fits one cell; emoji, CJK and other wide glyphs stay as boxes.
# family = "Menlo"
# Greater than 0. Size in points.
size = 13
# 1 to 2. Line spacing as a multiple of the font's own: 1 is the font's own
# spacing, 1.4 is airy. Below 1 is refused — it would clip the tails of g and y.
line_height = 1.0

[clipboard]
# "copy" | "off". Lets programs in the terminal, also over ssh, copy text to
# the clipboard (OSC 52): copy allows it, off does not. They can never read it.
osc52 = "copy"

[motion]
# "snap" | "ease" | "spring". How the cursor travels between cells: spring
# glides and eases into place, ease glides for a fixed time, snap jumps there
# at once.
cursor_motion = "spring"
# "system" | "on" | "off". Whether to tone animations down to a short fade:
# system follows the macOS Reduce Motion setting, on and off decide it here.
reduce_motion = "system"

[shell]
# "auto" | "blocks" | "off". Whether bateri sets up the shell so it can report
# where prompts and commands begin and end. auto does it for shells bateri
# knows, and on those shells it also moves the line you type into the dock at
# the bottom of the window and draws the prompt itself. blocks keeps command
# blocks and marks but leaves the line and the prompt to your shell, the way a
# terminal normally works. off never sets anything up.
# Unlike every other key here, this one only takes effect in shells started
# after the change; shells already open keep what they were started with.
integration = "auto"
"#;

    /// Dosya **var ama kullanılamıyor** (okunamıyor ya da geçersiz TOML)
    /// iken açılışın ayarları: varsayılanlar, yalnız OSC 52 **kapalı**.
    ///
    /// Dosyada kullanıcının `osc52 = "off"`'u olabilir ve okunamayan bir
    /// dosya bunu söyleyemiyor: pano uzaktaki bir programa kapalıya düşer,
    /// açığa değil (`discussion.md` → Karar 5). Geri kalan her anahtarın
    /// yanlış tahmini zararsız ve görünür (tema, punto); OSC 52'ninki sessiz.
    /// Dosya düzelip kaydedilince canlı yenileme dosyadaki değeri uygular.
    ///
    /// Dosya **yoksa** bu değil, [`Settings::default`]: kullanıcı hiçbir şey
    /// söylememiş. Kararı veren (`bt-shell`'in yükleyicisi) ikisini ayırıyor;
    /// değerin sahibi burası, varsayılanların sahibi olduğu için.
    pub fn for_unusable_file() -> Self {
        Self {
            osc52: Osc52::Off,
            ..Self::default()
        }
    }

    /// `settings.toml`'un metni → değerler + tanılar, ya da ayrıştırılamadı.
    ///
    /// `Err` **yalnız** geçersiz TOML'da; anahtar düzeyindeki her sorun
    /// `Ok`'un tanı listesine düşer ve o anahtar varsayılanını alır
    /// (`osc52` kapalıyı, [`Settings::parse_keeping`]).
    ///
    /// Geçersiz TOML sözdiziminden geniş: yinelenen anahtar ve TOML'un
    /// tam sayı sınırını (`i64`) aşan sayı da belgenin tamamını düşürüyor —
    /// `scrollback = 99999999999999999999` tavana kırpılamaz, çünkü değer
    /// hiç okunamıyor. `docs/AYARLAR.md` bunu söylüyor.
    pub fn parse(text: &str) -> Result<Parsed, Diagnostic> {
        Self::parse_keeping(text, &Settings::default())
    }

    /// [`Settings::parse`], ama **kabul edilmeyen** değer varsayılanı değil
    /// `fallback`'inkini alır — kayıt anının kuralı: çağıran geçerli ayarları
    /// verir (`bt-shell`'in canlı yenilemesi).
    ///
    /// Sebep geri alınamayan uygulama: `scrollback = 100000` iken yanlışlıkla
    /// kaydedilen `scrollback = "100000"` varsayılana (on bin) düşseydi
    /// geçmişin doksan bin satırı o anda silinir, dosyayı düzeltmek onları
    /// geri getirmezdi. Tanı da düşülen değeri söylüyor ("using 100000").
    ///
    /// Yalnız kabul edilmeyen değer: dosyada **olmayan** anahtar varsayılanını
    /// alır (dosya bir şey söylemiyor, anahtarı silen kullanıcı varsayılanı
    /// istiyor) ve tavanı aşan değer tavana kırpılır (niyet belli). Bölüm
    /// yanlış türdeyse (`terminal = 5`) bölümün bütün anahtarları kabul
    /// edilmemiş sayılır.
    ///
    /// **Tek istisna `clipboard.osc52`:** kabul edilmeyen değeri `fallback`'i
    /// değil `"off"`'u alır, bölüm yanlış türdeyse de. Kuralın sebebi geri
    /// alınamayan uygulamaydı ve OSC 52'yi kapatmak geri alınabilir; tersi,
    /// `"of"` diye yanlış yazılmış bir kapatmanın panoyu sessizce açık
    /// tutması, değil (`discussion.md` → Karar 5).
    pub fn parse_keeping(text: &str, fallback: &Settings) -> Result<Parsed, Diagnostic> {
        let doc = document(text)?;
        let mut parsed = Parsed {
            settings: Settings::default(),
            diagnostics: Vec::new(),
        };
        let root = doc.as_table();
        match section(text, root, "terminal", &mut parsed.diagnostics) {
            Some(terminal) => {
                if let Some(item) = terminal.get("scrollback") {
                    parsed.settings.scrollback =
                        scrollback(text, item, fallback.scrollback, &mut parsed.diagnostics);
                }
                if let Some(item) = terminal.get("cursor") {
                    parsed.settings.cursor =
                        caret_shape(text, item, fallback.cursor, &mut parsed.diagnostics);
                }
                if let Some(item) = terminal.get("cursor_blink") {
                    parsed.settings.cursor_blink =
                        cursor_blink(text, item, fallback.cursor_blink, &mut parsed.diagnostics);
                }
                if let Some(item) = terminal.get("cursor_radius") {
                    parsed.settings.caret.radius_ratio = ranged_float(
                        text,
                        item,
                        "terminal.cursor_radius",
                        CURSOR_RADIUS_RANGE,
                        fallback.caret.radius_ratio,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_blink_interval") {
                    parsed.settings.blink_interval = ranged_float(
                        text,
                        item,
                        "terminal.cursor_blink_interval",
                        CURSOR_BLINK_RANGE,
                        fallback.blink_interval,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_unfocused") {
                    parsed.settings.caret.unfocused = named_enum(
                        text,
                        item,
                        "terminal.cursor_unfocused",
                        UnfocusedCaret::NAMES,
                        fallback.caret.unfocused,
                        fallback.caret.unfocused.name(),
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_glow") {
                    parsed.settings.caret.glow = ranged_float(
                        text,
                        item,
                        "terminal.cursor_glow",
                        CURSOR_GLOW_RANGE,
                        fallback.caret.glow,
                        &mut parsed.diagnostics,
                    );
                }
            }
            None if root.contains_key("terminal") => {
                parsed.settings.scrollback = fallback.scrollback;
                parsed.settings.cursor = fallback.cursor;
                parsed.settings.cursor_blink = fallback.cursor_blink;
                parsed.settings.caret = fallback.caret;
                parsed.settings.blink_interval = fallback.blink_interval;
            }
            None => {}
        }
        // İkincisi tanıdaki noktalı yol (`Diagnostic::key` `'static` ister),
        // `theme.rs`'in `ANSI_KEYS`'iyle aynı deyiş; sonuncusu kabul
        // edilmeyen değerin yerine geçen.
        let names = [
            (
                "theme",
                "appearance.theme",
                &mut parsed.settings.theme,
                &fallback.theme,
            ),
            (
                "light_theme",
                "appearance.light_theme",
                &mut parsed.settings.light_theme,
                &fallback.light_theme,
            ),
            (
                "dark_theme",
                "appearance.dark_theme",
                &mut parsed.settings.dark_theme,
                &fallback.dark_theme,
            ),
        ];
        match section(text, root, "appearance", &mut parsed.diagnostics) {
            Some(appearance) => {
                for (key, path, slot, kept) in names {
                    if let Some(item) = appearance.get(key) {
                        let accepts_system = key == "theme";
                        let diagnostics = &mut parsed.diagnostics;
                        *slot = theme_name(text, item, path, kept, accepts_system, diagnostics)
                            .unwrap_or_else(|| kept.clone());
                    }
                }
            }
            None if root.contains_key("appearance") => {
                for (_, _, slot, kept) in names {
                    slot.clone_from(kept);
                }
            }
            None => {}
        }
        match section(text, root, "font", &mut parsed.diagnostics) {
            Some(font) => {
                let diagnostics = &mut parsed.diagnostics;
                if let Some(item) = font.get("family") {
                    parsed.settings.font.family =
                        font_family(text, item, &fallback.font.family, diagnostics);
                }
                if let Some(item) = font.get("size") {
                    parsed.settings.font.size =
                        font_size(text, item, fallback.font.size, diagnostics);
                }
                if let Some(item) = font.get("line_height") {
                    parsed.settings.font.line_height =
                        line_height(text, item, fallback.font.line_height, diagnostics);
                }
            }
            None if root.contains_key("font") => {
                parsed.settings.font.clone_from(&fallback.font);
            }
            None => {}
        }
        // `fallback` bilerek okunmuyor: kabul edilmeyen değer kapalıya düşüyor
        // (yukarıdaki doc'un istisnası).
        match section(text, root, "clipboard", &mut parsed.diagnostics) {
            Some(clipboard) => {
                if let Some(item) = clipboard.get("osc52") {
                    parsed.settings.osc52 = osc52(text, item, &mut parsed.diagnostics);
                }
            }
            None if root.contains_key("clipboard") => {
                parsed.settings.osc52 = Osc52::Off;
            }
            None => {}
        }
        match section(text, root, "motion", &mut parsed.diagnostics) {
            Some(motion) => {
                if let Some(item) = motion.get("cursor_motion") {
                    parsed.settings.cursor_motion =
                        cursor_motion(text, item, fallback.cursor_motion, &mut parsed.diagnostics);
                }
                if let Some(item) = motion.get("reduce_motion") {
                    parsed.settings.reduce_motion =
                        reduce_motion(text, item, fallback.reduce_motion, &mut parsed.diagnostics);
                }
            }
            None if root.contains_key("motion") => {
                parsed.settings.cursor_motion = fallback.cursor_motion;
                parsed.settings.reduce_motion = fallback.reduce_motion;
            }
            None => {}
        }
        match section(text, root, "shell", &mut parsed.diagnostics) {
            Some(shell) => {
                if let Some(item) = shell.get("integration") {
                    parsed.settings.shell_integration = shell_integration(
                        text,
                        item,
                        fallback.shell_integration,
                        &mut parsed.diagnostics,
                    );
                }
                // Emekli anahtarlar: değeri okunmuyor, varlığı söyleniyor.
                for (key, message) in RETIRED {
                    if let Some(item) = shell.get(key) {
                        parsed.diagnostics.push(Diagnostic {
                            key: None,
                            line: item.span().and_then(|span| line_of(text, span.start)),
                            message: (*message).to_owned(),
                        });
                    }
                }
            }
            None if root.contains_key("shell") => {
                parsed.settings.shell_integration = fallback.shell_integration;
            }
            None => {}
        }
        Ok(parsed)
    }

    /// Kullanılacak temanın **adı**: `theme = "system"` ise görünüme göre
    /// `light_theme` ya da `dark_theme`, değilse `theme`'in kendisi —
    /// görünümden bağımsız.
    ///
    /// Saf: görünümü okuyan `bt-shell`, ad çözümü de orada.
    pub fn theme_for(&self, dark: bool) -> &str {
        match (self.follows_system(), dark) {
            (false, _) => &self.theme,
            (true, true) => &self.dark_theme,
            (true, false) => &self.light_theme,
        }
    }

    /// Tema sistemin görünümüne mi bağlı. Değilse görünüm değişimi temaya
    /// dokunmaz ve çağıranın dosyayı yeniden okumasına gerek yok.
    pub fn follows_system(&self) -> bool {
        self.theme == SYSTEM_THEME
    }

    /// Oturumun terminal seçenekleri — `Session`'a açılışta da canlı
    /// değişimde de **tamamı** bununla gider ([`TerminalOptions`]'ın doc'u).
    pub fn terminal(&self) -> TerminalOptions {
        TerminalOptions {
            scrollback: self.scrollback,
            osc52: self.osc52,
            cursor: self.cursor,
            blink: self.cursor_blink,
        }
    }

    /// `self`'ten (önceki) `new`'e neyin değiştiği — canlı yenilemenin
    /// kapısı: değişmeyen parça uygulanmaz.
    ///
    /// Saf; önceki değeri tutan çağıran (`bt-shell`). Ayrı bir birleştirme
    /// mekanizması yok: bir kayıt birden çok olay doğurursa ikincisi boş fark
    /// verir.
    pub fn changes(&self, new: &Settings) -> Changes {
        Changes {
            terminal: self.terminal() != new.terminal(),
            font: self.font != new.font,
            motion: self.cursor_motion != new.cursor_motion
                || self.reduce_motion != new.reduce_motion,
            caret: self.caret != new.caret || self.blink_interval != new.blink_interval,
        }
    }

    /// Menünün tema seçimi (View ▸ Theme ▸): `settings.toml`'un metninde
    /// `[appearance] theme`'i `name` yapar ve **geri kalan her baytı** yerinde
    /// bırakır — yorumlar, boş satırlar, anahtar sırası, tanımadığımız
    /// anahtarlar, değerin yanındaki yorum. Dosyayı okuyup yazan `bt-shell`.
    ///
    /// - Bölüm yoksa sona, anahtar yoksa bölümün içine eklenir; bölümün
    ///   yazılışı (başlık, satır içi tablo, noktalı anahtar) korunur.
    /// - `light_theme` ve `dark_theme`'e dokunmaz: sabit bir tema seçen
    ///   kullanıcı `"system"`'e dönünce çiftini geri bulur.
    /// - **Ayrıştırılamayan metin `Err`**, yeni metin üretilmez: dosya
    ///   kullanıcının yarım işi ve üstüne yazmak onu silerdi. Aynı sebeple
    ///   bölüm olmayan bir `appearance` (`appearance = 1`, `[[appearance]]`)
    ///   ve bölüm olan bir `theme` (`[appearance.theme]`, `theme = { … }`) de
    ///   `Err`: yerlerine
    ///   yazmak içeriklerini silerdi. Kabul edilmeyen türdeki bir değer
    ///   (`theme = 3`) ise değişir — kullanıcı bir tema seçti.
    ///
    /// Adın biçimi sınanmıyor: çağıran (menü) yalnız gömülü temaların ve
    /// `themes/`'teki dosyaların adlarını veriyor; öyle olmasa da ayrıştırıcı
    /// adı okurken reddeder.
    pub fn with_theme(text: &str, name: &str) -> Result<String, Diagnostic> {
        const SECTION: &str = "appearance";
        const KEY: &str = "appearance.theme";
        let parsed = document(text)?;
        // Ret konumlu belgede: `into_mut` konumları düşürüyor, tanının satırı
        // onlardan geliyor.
        let mut refused = Vec::new();
        // Satır içi tablo (`theme = { … }`) da bir bölüm: `is_value` onu
        // geçirirdi ve yerine yazmak `[appearance.theme]`'in reddedildiği
        // içeriği bu yazılışta sessizce silerdi (`/code-review` bulgusu).
        if let Some(appearance) = section(text, parsed.as_table(), SECTION, &mut refused)
            && let Some(item) = appearance
                .get("theme")
                .filter(|item| !item.is_value() || item.is_inline_table())
        {
            refused.push(Diagnostic {
                key: Some(KEY),
                line: item.span().and_then(|span| line_of(text, span.start)),
                message: format!("`{KEY}` must be a string, found {}", kind(item)),
            });
        }
        if let Some(diagnostic) = refused.pop() {
            return Err(diagnostic);
        }
        let mut doc = parsed.into_mut();
        if !doc.contains_key(SECTION) {
            let mut table = toml_edit::Table::new();
            // Belge sonundaki yorum `toml_edit`'te belgenin kuyruğu ve yeni
            // bölüm onun önüne yazılırdı: son bölümün altındaki
            // `# family = "Menlo"` `[appearance]`'a geçer, yorumu kaldıran
            // kullanıcının satırı sessizce yoksayılırdı. Kuyruk yeni başlığın
            // önüne alınıyor, yani yazıldığı bölümde kalıyor.
            let trailing = doc.trailing().as_str().unwrap_or_default().to_owned();
            if !trailing.trim().is_empty() {
                table.decor_mut().set_prefix(format!("{trailing}\n"));
                doc.set_trailing("");
            }
            doc.insert(SECTION, Item::Table(table));
        }
        // `else` dalı yok: bölüm olmayan bir `appearance` yukarıda reddedildi,
        // eksik olan da az önce tablo olarak eklendi.
        if let Some(appearance) = doc.get_mut(SECTION).and_then(Item::as_table_like_mut) {
            match appearance.get_mut("theme").and_then(Item::as_value_mut) {
                // Süs (`=`'den sonraki boşluk, satır sonundaki yorum) değerin
                // üstünde duruyor; yeni değer onu devralmazsa yorum düşerdi.
                Some(value) => {
                    let decor = value.decor().clone();
                    *value = name.into();
                    *value.decor_mut() = decor;
                }
                None => {
                    appearance.insert("theme", toml_edit::value(name));
                }
            }
        }
        // `toml_edit` satır sonlarını LF yazıyor. İlk satırı CRLF olan dosya
        // CRLF kalıyor; yoksa tek bir seçim dotfile deposunda bütün dosyayı
        // değişmiş gösterirdi. Karışık satır sonlu dosya ilk satırınkini alır.
        //
        // Önce LF'ye indirilip sonra çevriliyor (`/code-review` bulgusu):
        // `toml_edit` çok satırlı metnin **içindeki** `\r\n`'i olduğu gibi
        // bırakıyor ve doğrudan çeviri onu `\r\r\n` yapardı — geçersiz TOML,
        // yani dosyaya bir daha yazılamaz ve hiçbir kayıt uygulanmazdı.
        let crlf = text
            .find('\n')
            .is_some_and(|end| text.as_bytes()[..end].ends_with(b"\r"));
        let written = doc.to_string();
        Ok(if crlf {
            written.replace("\r\n", "\n").replace('\n', "\r\n")
        } else {
            written
        })
    }
}

/// İki [`Settings`] arasındaki fark ([`Settings::changes`]).
///
/// **Tema burada yok**, bilerek: canlı yenilemede tema her olayda yeniden
/// çözülüyor, çünkü etkin tema dosyasının kendisi de bir kaynak ve onun
/// değişimi ayar metninin farkında görünmez. Tema adı için bir alan ikinci,
/// yarım bir kapı olurdu; aynı temanın takası zaten no-op
/// (`Session::set_theme`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Changes {
    /// [`Settings::terminal`] değişti: seçenekler `Session`'a **tamamıyla**
    /// gider.
    pub terminal: bool,
    /// [`Settings::font`] değişti: renderer'a gider, hücre ölçüsü ve grid
    /// yeniden hesaplanır.
    pub font: bool,
    /// `[motion]` bölümü değişti: kareyi süren ritme gider
    /// (`bt_gpu::DisplayLink::set_cursor_motion`,
    /// `bt_gpu::DisplayLink::set_reduce_motion`). Terminalden ve fonttan ayrı
    /// bir alan, çünkü hareket ne oturumu ne hücre ölçüsünü ilgilendiriyor —
    /// ikisine de bağlansaydı bir stil değişimi grid'i yeniden kurdururdu.
    ///
    /// İki anahtar **tek** alanda: ikisi de aynı yere, aynı çağrı yerinde
    /// gidiyor ve ayrı alanlar çağıranda tek bir `if` yerine iki tane
    /// yazdırırdı. [`Settings::reduce_motion`] üç değerli olduğu için
    /// `bt-shell` onu yine de çözmek zorunda; fark yalnız "bir şey değişti"
    /// diyor.
    pub motion: bool,
    /// [`Settings::caret`] değişti: imlecin çizim sayıları `bt-gpu`'ya gider
    /// (`bt_gpu::DisplayLink::set_caret_style`).
    ///
    /// **`terminal`'dan ayrı alan** ve bu şart: `changes.terminal` bugün
    /// `self.terminal() != new.terminal()`'in ta kendisi, yani
    /// `TerminalOptions`'ın farkı. Bu iki anahtar oraya **girmiyor**; aynı
    /// alana binselerdi bir yarıçap değişimi `TerminalOptions`'ı baştan
    /// `Session`'a gönderirdi.
    pub caret: bool,
}

/// Metni TOML belgesine ayrıştırır; ayrıştırılamıyorsa tek satırlık tanı.
///
/// `Document` (değişmez belge) `DocumentMut` değil: konumlar yalnız
/// ayrıştırılmış belgede duruyor ve tanının satırı onlardan geliyor.
pub(crate) fn document(text: &str) -> Result<Document<&str>, Diagnostic> {
    Document::parse(text).map_err(|err| Diagnostic {
        key: None,
        line: err.span().and_then(|span| line_of(text, span.start)),
        message: format!("invalid TOML: {}", parser_reason(err.message())),
    })
}

/// Bir bölümü okur; bölüm değilse (`terminal = 5`) tanı bırakır ve `None`.
///
/// `TableLike`: `[terminal]` başlığı da `terminal = { scrollback = 1 }`
/// satır içi tablosu da aynı bölümdür.
pub(crate) fn section<'a>(
    text: &str,
    root: &'a toml_edit::Table,
    name: &'static str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a dyn TableLike> {
    let item = root.get(name)?;
    let table = item.as_table_like();
    if table.is_none() {
        diagnostics.push(Diagnostic {
            key: Some(name),
            line: item.span().and_then(|span| line_of(text, span.start)),
            message: format!("`{name}` must be a section, found {}", kind(item)),
        });
    }
    table
}

/// `terminal.scrollback`: tam sayı, negatif değil, tavanı aşarsa tavan.
///
/// İki kabul edilmeyen hâl iki ayrı sonuç veriyor ve ikisi de tanı bırakıyor:
///
/// - **Tavanı aşan → tavan.** "Çok geçmiş" isteyen kullanıcının niyeti
///   belli; `fallback`'e (açılışta on bin) düşürmek istediğinin tersini
///   verirdi. Tanı sessiz değil: istediği sayı uygulanmadı ve bunu bilmeli.
///   (Punto kırpması sessiz — orada sınır Cmd +/−'nin olağan ucu, bir hata
///   değil.)
/// - **Negatif ya da tam sayı değil → `fallback`.** Niyet okunamıyor.
fn scrollback(
    text: &str,
    item: &Item,
    fallback: usize,
    diagnostics: &mut Vec<Diagnostic>,
) -> usize {
    const KEY: &str = "terminal.scrollback";
    let line = item.span().and_then(|span| line_of(text, span.start));
    let reject = |message: String| Diagnostic {
        key: Some(KEY),
        line,
        message,
    };
    let Some(value) = item.as_integer() else {
        diagnostics.push(reject(format!(
            "`{KEY}` must be an integer, found {}; using {fallback}",
            kind(item)
        )));
        return fallback;
    };
    let Ok(value) = usize::try_from(value) else {
        diagnostics.push(reject(format!(
            "`{KEY}` cannot be negative; using {fallback}"
        )));
        return fallback;
    };
    if value > SCROLLBACK_MAX {
        diagnostics.push(reject(format!(
            "`{KEY}` is at most {SCROLLBACK_MAX}; using {SCROLLBACK_MAX}"
        )));
        return SCROLLBACK_MAX;
    }
    value
}

/// `appearance.theme`, `.light_theme`, `.dark_theme`: bir tema adı
/// (`theme` için [`SYSTEM_THEME`] de).
///
/// Adın yalnız **biçimi** sınanıyor: boş ad ve `/` içeren ad varsayılana
/// döner. `/` adı `themes/` dizininin dışına taşırdı — `"../settings"`
/// ayar dosyasının kendisini tema diye okuturdu. NUL da dosya yolu olamaz.
/// Adın bir temaya çözülüp çözülmediği `bt-shell`'in işi.
fn theme_name(
    text: &str,
    item: &Item,
    path: &'static str,
    default: &str,
    accepts_system: bool,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let line = item.span().and_then(|span| line_of(text, span.start));
    let reject = |message: String| Diagnostic {
        key: Some(path),
        line,
        message,
    };
    let Some(name) = item.as_str() else {
        diagnostics.push(reject(format!(
            "`{path}` must be a string, found {}; using \"{default}\"",
            kind(item)
        )));
        return None;
    };
    if name.is_empty() || name.contains(['/', '\0']) {
        diagnostics.push(reject(format!(
            "`{path}` must be a theme name without `/`, found {name:?}; using \"{default}\""
        )));
        return None;
    }
    if name == SYSTEM_THEME && !accepts_system {
        diagnostics.push(reject(format!(
            "`{path}` must name a theme, not \"{SYSTEM_THEME}\"; using \"{default}\""
        )));
        return None;
    }
    Some(name.to_owned())
}

/// `font.family`: metin; kırpılmış hâli boşsa `None` (zincir).
///
/// Boş ad bir hata değil: "aileyi sen seç" demenin yazılabilir yolu, anahtarı
/// silmeden. Adın makinede olup olmadığı burada sorulmuyor — dosya sistemi
/// değil CoreText ister, `bt-atlas` söylüyor.
fn font_family(
    text: &str,
    item: &Item,
    fallback: &Option<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    const KEY: &str = "font.family";
    let Some(name) = item.as_str() else {
        let using = match fallback {
            Some(name) => format!("\"{name}\""),
            None => "the default font".to_owned(),
        };
        diagnostics.push(Diagnostic {
            key: Some(KEY),
            line: item.span().and_then(|span| line_of(text, span.start)),
            message: format!(
                "`{KEY}` must be a string, found {}; using {using}",
                kind(item)
            ),
        });
        return fallback.clone();
    };
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

/// `font.size`: tam sayı ya da ondalıklı, sonlu ve sıfırdan büyük.
///
/// Üst sınır **yok**: kırpma `punto × ölçek`'e bağlı ve `bt-atlas`'ta sessiz
/// (`discussion.md` → Karar 4). Burada bir tavan olsaydı iki sahibi olurdu
/// ve pencere ekran değiştirdikçe tanı gelip giderdi.
/// `font.line_height`: `1.0` ile [`MAX_LINE_HEIGHT`] arasında bir çarpan.
///
/// [`font_size`]'ın aksine **iki uçlu**. Alt uç fontun kendi metriğinin
/// altına inmeyi yasaklıyor (bkz. [`FontOptions::line_height`]); üst uç keyfi
/// değil bir bütçe: her yuva `cell_w × cell_h` bayt ve atlas sabit boyutlu,
/// yani çarpan büyüdükçe atlasa sığan glyph sayısı düşüyor. Sınırsız bırakmak
/// tofu'ya düşen bir terminal demekti ve belirti ancak uzun bir oturumdan
/// sonra görünürdü.
fn line_height(text: &str, item: &Item, fallback: f64, diagnostics: &mut Vec<Diagnostic>) -> f64 {
    ranged_float(
        text,
        item,
        "font.line_height",
        1.0..=MAX_LINE_HEIGHT,
        fallback,
        diagnostics,
    )
}

/// Aralıklı ondalık anahtarın **ortak gövdesi**: sayı değilse ya da aralık
/// dışındaysa anahtar kendi değerinde kalır ve tanı bırakılır.
///
/// Ayrı fonksiyon, çünkü aynı ~35 satır depoda **iki kez elle** yazılmıştı
/// (`line_height`, `font_size`) ve 016 üç anahtar daha getiriyordu — yardımcı
/// adlandırılmasa üç kopya daha doğardı (`/plan-review`).
///
/// **Kırpma yok, red var:** aralık dışı değer sessizce uca çekilseydi
/// kullanıcı yanlış yazdığını hiç görmezdi. Deponun kuralı bu
/// ([`Settings::parse_keeping`]).
///
/// `font_size` **taşınmadı**: tek uçlu (`> 0`) ve tanı metni bu kalıba
/// girmiyor; zorlamak mesajı bozardı.
fn ranged_float(
    text: &str,
    item: &Item,
    key: &'static str,
    range: std::ops::RangeInclusive<f64>,
    fallback: f64,
    diagnostics: &mut Vec<Diagnostic>,
) -> f64 {
    let line = item.span().and_then(|span| line_of(text, span.start));
    let reject = |message: String| Diagnostic {
        key: Some(key),
        line,
        message,
    };
    let value = match (item.as_float(), item.as_integer()) {
        (Some(value), _) => value,
        (None, Some(value)) => value as f64,
        (None, None) => {
            diagnostics.push(reject(format!(
                "`{key}` must be a number, found {}; using {fallback}",
                kind(item)
            )));
            return fallback;
        }
    };
    if !(value.is_finite() && range.contains(&value)) {
        diagnostics.push(reject(format!(
            "`{key}` must be a number between {} and {}, found {value}; using {fallback}",
            range.start(),
            range.end()
        )));
        return fallback;
    }
    value
}

/// Satır yüksekliği çarpanının tavanı — atlas bütçesi (bkz. [`line_height`]).
pub const MAX_LINE_HEIGHT: f64 = 2.0;

fn font_size(text: &str, item: &Item, fallback: f64, diagnostics: &mut Vec<Diagnostic>) -> f64 {
    const KEY: &str = "font.size";
    let line = item.span().and_then(|span| line_of(text, span.start));
    let reject = |message: String| Diagnostic {
        key: Some(KEY),
        line,
        message,
    };
    // `as f64` tam sayıda kayıpsız değil ama anlamlı her puntoda kayıpsız;
    // kayıp başladığı büyüklük zaten kırpmanın çok ötesinde.
    let value = match (item.as_float(), item.as_integer()) {
        (Some(value), _) => value,
        (None, Some(value)) => value as f64,
        (None, None) => {
            diagnostics.push(reject(format!(
                "`{KEY}` must be a number, found {}; using {fallback}",
                kind(item)
            )));
            return fallback;
        }
    };
    if !(value.is_finite() && value > 0.0) {
        diagnostics.push(reject(format!(
            "`{KEY}` must be a number greater than 0, found {value}; using {fallback}"
        )));
        return fallback;
    }
    value
}

/// `clipboard.osc52`: tam olarak `"copy"` ya da `"off"`; başka her şey
/// `"off"` ve tanı.
///
/// Büyük/küçük harf duyarlı, tema adları gibi: `"Copy"` bir yazım hatası ve
/// hata kapalıya düşüyor. Okuma yönünün değerleri (`"paste"`, alacritty'nin
/// `"copy_paste"`'i) de tanınmıyor — okuma yönü yok (006 Karar 5).
fn osc52(text: &str, item: &Item, diagnostics: &mut Vec<Diagnostic>) -> Osc52 {
    const KEY: &str = "clipboard.osc52";
    let found = match item.as_str() {
        Some("copy") => return Osc52::Copy,
        Some("off") => return Osc52::Off,
        Some(value) => format!("{value:?}"),
        None => kind(item).to_owned(),
    };
    diagnostics.push(Diagnostic {
        key: Some(KEY),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!("`{KEY}` must be \"copy\" or \"off\", found {found}; using \"off\""),
    });
    Osc52::Off
}

/// `motion.cursor_motion`: tam olarak `"snap"`, `"ease"` ya da `"spring"`.
///
/// Kabul edilmeyen değer `fallback`'i alır ve tanı bırakır, yani **öteki
/// anahtarların kuralı**. `clipboard.osc52`'nin "kapalıya düş" istisnası
/// buraya geçmiyor (008 Karar 6): o istisnanın gerekçesi yanlış tahminin
/// **sessiz** olmasıydı — burada yanlış tahminin belirtisi ekranda kayan (ya
/// da kaymayan) bir imleç, yani kullanıcı ne olduğunu görüyor.
///
/// Büyük/küçük harf duyarlı, tema adları ve `osc52` gibi: `"Spring"` bir
/// yazım hatası.
fn cursor_motion(
    text: &str,
    item: &Item,
    fallback: CursorMotion,
    diagnostics: &mut Vec<Diagnostic>,
) -> CursorMotion {
    const KEY: &str = "motion.cursor_motion";
    let found = match item.as_str() {
        Some("snap") => return CursorMotion::Snap,
        Some("ease") => return CursorMotion::Ease,
        Some("spring") => return CursorMotion::Spring,
        Some(value) => format!("{value:?}"),
        None => kind(item).to_owned(),
    };
    diagnostics.push(Diagnostic {
        key: Some(KEY),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{KEY}` must be \"snap\", \"ease\" or \"spring\", found {found}; using \"{}\"",
            fallback.name()
        ),
    });
    fallback
}

/// `terminal.cursor`: tam olarak `"block"`, `"underline"` ya da `"beam"`.
///
/// [`cursor_motion`] ile aynı kural ve aynı gerekçe: kabul edilmeyen değer
/// `fallback`'i alır ve tanı bırakır. Yanlış tahminin belirtisi ekrandaki
/// imlecin şekli, yani kullanıcı ne olduğunu görüyor — `clipboard.osc52`'nin
/// "kapalıya düş" istisnası buraya geçmiyor.
fn caret_shape(
    text: &str,
    item: &Item,
    fallback: CaretShape,
    diagnostics: &mut Vec<Diagnostic>,
) -> CaretShape {
    const KEY: &str = "terminal.cursor";
    let found = match item.as_str() {
        Some("block") => return CaretShape::Block,
        Some("underline") => return CaretShape::Underline,
        Some("beam") => return CaretShape::Beam,
        Some(value) => format!("{value:?}"),
        None => kind(item).to_owned(),
    };
    diagnostics.push(Diagnostic {
        key: Some(KEY),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{KEY}` must be \"block\", \"underline\" or \"beam\", found {found}; using \"{}\"",
            fallback.name()
        ),
    });
    fallback
}

/// Adlandırılmış seçenek anahtarının **ortak gövdesi**: listedeki adlardan
/// biri değilse anahtar kendi değerinde kalır ve tanı bırakılır.
///
/// Ayrı fonksiyon, çünkü aynı kalıp depoda **beş kez** elle yazılmış
/// (`osc52`, `cursor_motion`, `reduce_motion`, `cursor_blink`, `caret_shape`)
/// ve `docs/YOL-HARITASI.md`'nin kayıtlı borcu altıncı kopyayı adıyla
/// öngörüyor: *"Dördüncü anahtar altıncı kopyayı doğurur."*
///
/// **Beş kopya bu sette taşınmadı** — her birinin tanı cümlesi kendi
/// sözcükleriyle yazılı ve taşımak mesajları bir turda değiştirirdi; emsal
/// `ranged_float`'ın `font_size`'ı bırakması.
///
/// Büyük/küçük harf **duyarlı**: `"Hollow"` bir yazım hatası ve sessizce
/// kabul edilmesi kullanıcıyı yanıltırdı.
fn named_enum<T: Copy>(
    text: &str,
    item: &Item,
    key: &'static str,
    names: &[(&str, T)],
    fallback: T,
    fallback_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> T {
    let found = match item.as_str() {
        Some(value) => {
            if let Some((_, picked)) = names.iter().find(|(name, _)| *name == value) {
                return *picked;
            }
            format!("{value:?}")
        }
        None => kind(item).to_owned(),
    };
    let expected = match names {
        [] => String::new(),
        [(one, _)] => format!("{one:?}"),
        [rest @ .., (last, _)] => format!(
            "{} or {last:?}",
            rest.iter()
                .map(|(name, _)| format!("{name:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    diagnostics.push(Diagnostic {
        key: Some(key),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!("`{key}` must be {expected}, found {found}; using \"{fallback_name}\""),
    });
    fallback
}

/// `terminal.cursor_blink`: tam olarak `"auto"`, `"on"` ya da `"off"`.
///
/// [`caret_shape`] ile aynı kural: kabul edilmeyen değer `fallback`'i alır ve
/// tanı bırakır.
fn cursor_blink(
    text: &str,
    item: &Item,
    fallback: CursorBlink,
    diagnostics: &mut Vec<Diagnostic>,
) -> CursorBlink {
    const KEY: &str = "terminal.cursor_blink";
    let found = match item.as_str() {
        Some("auto") => return CursorBlink::Auto,
        Some("on") => return CursorBlink::On,
        Some("off") => return CursorBlink::Off,
        Some(value) => format!("{value:?}"),
        None => kind(item).to_owned(),
    };
    diagnostics.push(Diagnostic {
        key: Some(KEY),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{KEY}` must be \"auto\", \"on\" or \"off\", found {found}; using \"{}\"",
            fallback.name()
        ),
    });
    fallback
}

/// `motion.reduce_motion`: tam olarak `"system"`, `"on"` ya da `"off"`.
///
/// [`cursor_motion`] ile aynı kural ve aynı gerekçe: kabul edilmeyen değer
/// `fallback`'i alır ve tanı bırakır. Burada da yanlış tahminin belirtisi
/// görünür (imleç kayar ya da kaymaz), yani `clipboard.osc52`'nin "kapalıya
/// düş" istisnası buraya da geçmiyor.
fn reduce_motion(
    text: &str,
    item: &Item,
    fallback: ReduceMotion,
    diagnostics: &mut Vec<Diagnostic>,
) -> ReduceMotion {
    const KEY: &str = "motion.reduce_motion";
    let found = match item.as_str() {
        Some("system") => return ReduceMotion::System,
        Some("on") => return ReduceMotion::On,
        Some("off") => return ReduceMotion::Off,
        Some(value) => format!("{value:?}"),
        None => kind(item).to_owned(),
    };
    diagnostics.push(Diagnostic {
        key: Some(KEY),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{KEY}` must be \"system\", \"on\" or \"off\", found {found}; using \"{}\"",
            fallback.name()
        ),
    });
    fallback
}

/// `shell.integration`: tam olarak `"auto"` ya da `"off"`.
///
/// [`cursor_motion`] ile aynı kural: kabul edilmeyen değer `fallback`'i alır
/// ve tanı bırakır. `clipboard.osc52`'nin "kapalıya düş" istisnası buraya
/// geçmiyor ve gerekçe bu anahtarda daha da net — kapalıya düşmek, yanlış
/// yazımın bedelini **özelliği kaybetmek** yaparken güvenlik adına hiçbir şey
/// kazandırmazdı: sarmalayıcı kullanıcının kendi dosyalarını yüklüyor,
/// kurulması bir risk değil.
fn shell_integration(
    text: &str,
    item: &Item,
    fallback: ShellIntegration,
    diagnostics: &mut Vec<Diagnostic>,
) -> ShellIntegration {
    const KEY: &str = "shell.integration";
    let found = match item.as_str() {
        Some("auto") => return ShellIntegration::Auto,
        Some("blocks") => return ShellIntegration::Blocks,
        Some("off") => return ShellIntegration::Off,
        Some(value) => format!("{value:?}"),
        None => kind(item).to_owned(),
    };
    diagnostics.push(Diagnostic {
        key: Some(KEY),
        line: item.span().and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{KEY}` must be \"auto\", \"blocks\" or \"off\", found {found}; using \"{}\"",
            fallback.name()
        ),
    });
    fallback
}

/// Bayt konumunun 1'den başlayan satırı.
///
/// `toml_edit`'in kendi çevirisi (`translate_position`) crate'e özel;
/// ayrıştırıcının verdiği konum her zaman metnin içinde ama `get` yine de
/// sınırın dışını `None`'a çeviriyor, dilimleme paniği yok.
pub(crate) fn line_of(text: &str, offset: usize) -> Option<usize> {
    let before = text.as_bytes().get(..offset)?;
    Some(before.iter().filter(|&&byte| byte == b'\n').count() + 1)
}

/// Ayrıştırıcının iletisinden alt başlığa sığan kısmı: nedeni, "beklenen"
/// listesi olmadan.
///
/// `toml_edit` iletiyi "neden, expected a, b, …" diye kuruyor ve liste on
/// kaleme çıkabiliyor (`a = "\q"`); tek satırlık alt başlıkta kesilirdi ve
/// kullanıcıya satırı göstermek zaten yetiyor.
fn parser_reason(message: &str) -> &str {
    message
        .split_once(", expected")
        .map_or(message, |(reason, _)| reason)
}

/// Tanı metninde bulunan değerin türü.
pub(crate) fn kind(item: &Item) -> &'static str {
    match item {
        Item::None => "nothing",
        Item::Table(_) => "a section",
        // `[[terminal]]`: bölüm **dizisi**. "Bölüm olmalı, bölüm bulundu"
        // demek kullanıcıya `[[…]]`'ı `[…]` yapmasını söylemezdi.
        Item::ArrayOfTables(_) => "an array of sections (`[[…]]`)",
        Item::Value(value) => match value {
            toml_edit::Value::String(_) => "a string",
            toml_edit::Value::Integer(_) => "an integer",
            toml_edit::Value::Float(_) => "a float",
            toml_edit::Value::Boolean(_) => "a boolean",
            toml_edit::Value::Datetime(_) => "a date",
            toml_edit::Value::Array(_) => "an array",
            toml_edit::Value::InlineTable(_) => "a section",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(text: &str) -> Settings {
        let parsed = Settings::parse(text).expect("ayrıştırılabilir metin");
        assert_eq!(parsed.diagnostics, Vec::new(), "tanı beklenmiyordu: {text}");
        parsed.settings
    }

    fn rejected(text: &str) -> (Settings, Diagnostic) {
        let parsed = Settings::parse(text).expect("ayrıştırılabilir metin");
        let [diagnostic] = <[Diagnostic; 1]>::try_from(parsed.diagnostics)
            .unwrap_or_else(|got| panic!("tek tanı beklendi: {got:?}"));
        (parsed.settings, diagnostic)
    }

    #[test]
    fn empty_file_is_default() {
        assert_eq!(clean(""), Settings::default());
        assert_eq!(clean("# yalnız yorum\n\n"), Settings::default());
    }

    #[test]
    fn template_is_the_defaults() {
        // "Settings…"ın yarattığı dosya bugünkü davranışı değiştirmemeli:
        // tanısız ve varsayılanların ta kendisi. Varsayılan değişip şablon
        // değişmezse burada düşer.
        assert_eq!(clean(Settings::TEMPLATE), Settings::default());

        // Varsayılanı olan her anahtar **yazılı**, yorumda değil: kullanıcı
        // değeri yerinde değiştiriyor ve menüden tema seçimi satırı yerinde
        // yazıyor. Boş bir şablon yukarıdaki eşitliği de geçerdi.
        let doc = document(Settings::TEMPLATE).expect("şablon TOML");
        for (section, key) in [
            ("terminal", "scrollback"),
            ("terminal", "cursor"),
            ("terminal", "cursor_blink"),
            ("terminal", "cursor_radius"),
            ("terminal", "cursor_glow"),
            ("terminal", "cursor_unfocused"),
            ("terminal", "cursor_blink_interval"),
            ("appearance", "theme"),
            ("appearance", "light_theme"),
            ("appearance", "dark_theme"),
            ("font", "size"),
            ("font", "line_height"),
            ("clipboard", "osc52"),
            ("motion", "cursor_motion"),
            ("motion", "reduce_motion"),
            ("shell", "integration"),
        ] {
            assert!(
                doc.get(section).and_then(|s| s.get(key)).is_some(),
                "şablonda {section}.{key} yok"
            );
        }

        // Varsayılanı olmayan `family` yorumda bir örnek; yorumu kaldıran
        // kullanıcı geçerli bir değer bulmalı.
        let uncommented = Settings::TEMPLATE.replace("# family = ", "family = ");
        assert_ne!(
            uncommented,
            Settings::TEMPLATE,
            "şablonda family örneği yok"
        );
        assert!(clean(&uncommented).font.family.is_some());
    }

    #[test]
    fn documented_template_is_the_template() {
        // `docs/AYARLAR.md` şablonu olduğu gibi gösteriyor; kopya drift eder.
        let doc = include_str!("../../../docs/AYARLAR.md");
        let (_, after) = doc
            .split_once("### Şablon\n")
            .expect("AYARLAR.md'de şablon başlığı yok");
        let (_, block) = after
            .split_once("```toml\n")
            .expect("başlığın altında toml bloğu yok");
        let (block, _) = block.split_once("```").expect("toml bloğu kapanmıyor");
        assert_eq!(block, Settings::TEMPLATE);
    }

    #[test]
    fn scrollback_is_read() {
        assert_eq!(clean("[terminal]\nscrollback = 500\n").scrollback, 500);
        // Satır içi tablo aynı bölüm.
        assert_eq!(clean("terminal = { scrollback = 0 }").scrollback, 0);
        assert_eq!(
            clean(&format!("[terminal]\nscrollback = {SCROLLBACK_MAX}")).scrollback,
            SCROLLBACK_MAX
        );
    }

    #[test]
    fn wrong_type_falls_back_to_default_with_diagnostic() {
        let (settings, diagnostic) = rejected("[terminal]\n\nscrollback = \"çok\"\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostic.key, Some("terminal.scrollback"));
        assert_eq!(diagnostic.line, Some(3));
        assert_eq!(
            diagnostic.to_string(),
            "line 3: `terminal.scrollback` must be an integer, found a string; using 10000"
        );

        let (settings, diagnostic) = rejected("[terminal]\nscrollback = 1.5\n");
        assert_eq!(settings, Settings::default());
        assert!(diagnostic.message.contains("a float"), "{diagnostic}");

        let (settings, diagnostic) = rejected("[terminal]\nscrollback = -1\n");
        assert_eq!(settings, Settings::default());
        assert!(diagnostic.message.contains("negative"), "{diagnostic}");
    }

    #[test]
    fn scrollback_over_the_ceiling_is_clamped_with_diagnostic() {
        let (settings, diagnostic) = rejected("[terminal]\nscrollback = 1000000\n");
        assert_eq!(settings.scrollback, SCROLLBACK_MAX);
        assert_eq!(diagnostic.key, Some("terminal.scrollback"));
        assert_eq!(diagnostic.line, Some(2));
    }

    #[test]
    fn theme_name_is_read() {
        assert_eq!(clean("[appearance]\ntheme = \"paper\"\n").theme, "paper");
        // Varsayılan sistemi izlemek, çift gömülü temalar.
        let defaults = clean("");
        assert_eq!(
            (
                defaults.theme.as_str(),
                defaults.light_theme.as_str(),
                defaults.dark_theme.as_str()
            ),
            ("system", "bateri-light", "bateri")
        );
        // Bölüm satır içi de yazılabilir; komşu bölüm okumayı bozmaz.
        let settings = clean("appearance = { theme = \"a b.c\" }\n[terminal]\nscrollback = 3\n");
        assert_eq!((settings.theme.as_str(), settings.scrollback), ("a b.c", 3));

        let settings = clean("[appearance]\nlight_theme = \"paper\"\ndark_theme = \"ink\"\n");
        assert_eq!(
            (
                settings.theme.as_str(),
                settings.light_theme.as_str(),
                settings.dark_theme.as_str()
            ),
            ("system", "paper", "ink")
        );
    }

    #[test]
    fn theme_follows_the_appearance_only_when_system() {
        let pair = Settings {
            light_theme: "paper".to_owned(),
            dark_theme: "ink".to_owned(),
            ..Settings::default()
        };
        assert_eq!(pair.theme_for(true), "ink");
        assert_eq!(pair.theme_for(false), "paper");
        assert!(pair.follows_system());
        // Sabit ad görünümden bağımsız; çift yerinde kalsa da okunmaz.
        let fixed = Settings {
            theme: "bateri".to_owned(),
            ..pair
        };
        assert_eq!(fixed.theme_for(true), "bateri");
        assert_eq!(fixed.theme_for(false), "bateri");
        assert!(!fixed.follows_system());
    }

    #[test]
    fn unchanged_settings_have_no_changes() {
        // Her kayıtta dosyanın tamamı yeniden okunuyor; aynı metin boş fark
        // vermeli, yoksa her kayıt geçmişi yeniden kurar ve kare ister.
        let text = "[terminal]\nscrollback = 500\n[appearance]\ntheme = \"paper\"\n";
        assert_eq!(clean(text).changes(&clean(text)), Changes::default());
        assert_eq!(
            Settings::default().changes(&Settings::default()),
            Changes::default()
        );
    }

    #[test]
    fn scrollback_change_is_a_terminal_change() {
        let before = clean("[terminal]\nscrollback = 500\n");
        let after = clean("[terminal]\nscrollback = 20\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: true,
                font: false,
                motion: false,
                caret: false
            }
        );
        assert_eq!(
            after.terminal(),
            TerminalOptions {
                scrollback: 20,
                osc52: Osc52::Copy,
                cursor: CaretShape::default(),
                blink: CursorBlink::default(),
            }
        );
        // Tema adları terminal seçeneği değil: tema her kayıtta yeniden
        // çözülüyor (`bt-shell`), fark onu kapılamıyor.
        let themed = clean("[terminal]\nscrollback = 500\n[appearance]\ntheme = \"paper\"\n");
        assert_eq!(before.changes(&themed), Changes::default());
    }

    #[test]
    fn rejected_values_keep_the_given_settings() {
        // Kayıt anının kuralı (`/code-review` bulgusu): `scrollback`'in
        // yanlış türde kaydı varsayılana (on bin) düşseydi yüz binlik geçmiş
        // o anda geri dönülmez kırpılırdı. Kabul edilmeyen değer verilen
        // ayarlarınkini alıyor ve tanı **onu** söylüyor.
        let current = Settings {
            scrollback: 100_000,
            cursor: CaretShape::default(),
            cursor_blink: CursorBlink::default(),
            caret: CaretStyle::default(),
            blink_interval: CURSOR_BLINK_INTERVAL,
            theme: "paper".to_owned(),
            light_theme: "chalk".to_owned(),
            dark_theme: "ink".to_owned(),
            font: FontOptions::default(),
            osc52: Osc52::Copy,
            cursor_motion: CursorMotion::Spring,
            reduce_motion: ReduceMotion::System,
            shell_integration: ShellIntegration::Auto,
        };
        let parsed = Settings::parse_keeping(
            "[terminal]\nscrollback = \"100000\"\n[appearance]\ntheme = 3\n",
            &current,
        )
        .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.scrollback, 100_000);
        assert_eq!(parsed.settings.theme, "paper");
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>(),
            [
                "`terminal.scrollback` must be an integer, found a string; using 100000",
                "`appearance.theme` must be a string, found an integer; using \"paper\"",
            ]
        );
        // Dosyada **olmayan** anahtar yine varsayılan: dosya bir şey
        // söylemiyor, kabul edilmeyen bir değer de yok.
        assert_eq!(parsed.settings.light_theme, "bateri-light");
        assert_eq!(parsed.settings.dark_theme, "bateri");

        // Tavanı aşan değer tavana kırpılıyor, verilene değil: niyet belli.
        let parsed = Settings::parse_keeping("[terminal]\nscrollback = 1000000\n", &current)
            .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.scrollback, SCROLLBACK_MAX);

        // Bölüm yanlış türde: bölümün bütün anahtarları kabul edilmemiş sayılır.
        let parsed = Settings::parse_keeping("terminal = 5\nappearance = 1\n", &current)
            .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings, current);
        assert_eq!(parsed.diagnostics.len(), 2);

        // `parse` açılışın kuralı: aynı metin varsayılana düşüyor.
        assert_eq!(
            Settings::parse("terminal = 5\nappearance = 1\n")
                .expect("ayrıştırılabilir metin")
                .settings,
            Settings::default()
        );
    }

    #[test]
    fn font_is_read() {
        // Dosyada yoksa zincir ve 13 punto.
        assert_eq!(
            clean("").font,
            FontOptions {
                family: None,
                size: 13.0,
                line_height: 1.0
            }
        );
        assert_eq!(
            clean("[font]\nfamily = \"Monaco\"\nsize = 14.5\n").font,
            FontOptions {
                family: Some("Monaco".to_owned()),
                size: 14.5,
                line_height: 1.0
            }
        );
        // Tam sayı da punto: kullanıcının ilk yazacağı `size = 14`.
        assert_eq!(clean("[font]\nsize = 14\n").font.size, 14.0);
        // Boş aile (yalnız boşluk da) zincir demek, hata değil; ad kırpılıyor.
        assert_eq!(clean("[font]\nfamily = \"\"\n").font.family, None);
        assert_eq!(clean("[font]\nfamily = \"  \"\n").font.family, None);
        assert_eq!(
            clean("font = { family = \" Menlo \" }\n").font.family,
            Some("Menlo".to_owned())
        );
    }

    #[test]
    fn broken_font_size_falls_back_with_diagnostic() {
        // TOML `nan` ve `inf`'i sayı olarak kabul ediyor; tip denetimi onları
        // geçirir, kural geçirmemeli. Negatif ve sıfır punto da.
        for (value, found) in [
            ("-1", "-1"),
            ("0", "0"),
            ("0.0", "0"),
            ("-2.5", "-2.5"),
            ("nan", "NaN"),
            ("inf", "inf"),
        ] {
            let (settings, diagnostic) = rejected(&format!("[font]\nsize = {value}\n"));
            assert_eq!(settings, Settings::default(), "{value}");
            assert_eq!(diagnostic.key, Some("font.size"), "{value}");
            assert_eq!(diagnostic.line, Some(2), "{value}");
            assert_eq!(
                diagnostic.message,
                format!("`font.size` must be a number greater than 0, found {found}; using 13")
            );
        }
        let (settings, diagnostic) = rejected("[font]\nsize = \"14\"\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(
            diagnostic.message,
            "`font.size` must be a number, found a string; using 13"
        );

        let (settings, diagnostic) = rejected("[font]\nfamily = 3\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostic.key, Some("font.family"));
        assert_eq!(
            diagnostic.message,
            "`font.family` must be a string, found an integer; using the default font"
        );
    }

    #[test]
    fn rejected_font_values_keep_the_given_settings() {
        // Kayıt anının kuralı fontta da: yarım kaydedilmiş bir punto pencereyi
        // varsayılana çakıp geri döndürmesin.
        let current = Settings {
            font: FontOptions {
                family: Some("Monaco".to_owned()),
                size: 18.0,
                line_height: 1.0,
            },
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[font]\nfamily = false\nsize = 0\n", &current)
            .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.font, current.font);
        assert_eq!(
            parsed
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>(),
            [
                "`font.family` must be a string, found a boolean; using \"Monaco\"",
                "`font.size` must be a number greater than 0, found 0; using 18",
            ]
        );
        let parsed =
            Settings::parse_keeping("font = 5\n", &current).expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings, current);
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn font_change_is_a_font_change() {
        let before = clean("[font]\nsize = 13\n");
        let font_only = Changes {
            terminal: false,
            font: true,
            motion: false,
            caret: false,
        };
        assert_eq!(before.changes(&clean("[font]\nsize = 14\n")), font_only);
        assert_eq!(
            before.changes(&clean("[font]\nfamily = \"Monaco\"\nsize = 13\n")),
            font_only
        );
        // Açıkça yazılan varsayılan bir fark değil: kayıt atlası yeniden
        // kurdurmaz.
        assert_eq!(Settings::default().changes(&before), Changes::default());
    }

    #[test]
    fn osc52_is_read() {
        // Dosyada yoksa açık: ssh'taki vim'in kopyası kutudan çıktığı gibi
        // çalışsın.
        assert_eq!(clean("").osc52, Osc52::Copy);
        assert_eq!(clean("[clipboard]\nosc52 = \"copy\"\n").osc52, Osc52::Copy);
        assert_eq!(clean("[clipboard]\nosc52 = \"off\"\n").osc52, Osc52::Off);
        assert_eq!(clean("clipboard = { osc52 = \"off\" }\n").osc52, Osc52::Off);
    }

    #[test]
    fn unrecognized_osc52_is_off_with_diagnostic() {
        // Kapalıya düşer: okuma yönünün adı, büyük harfli yazım, `false`,
        // sayı ve bölüm — hiçbiri varsayılana (açık) dönmüyor.
        for (value, found) in [
            ("\"paste\"", "\"paste\""),
            ("\"Copy\"", "\"Copy\""),
            ("false", "a boolean"),
            ("1", "an integer"),
            ("{ mode = \"copy\" }", "a section"),
        ] {
            let (settings, diagnostic) = rejected(&format!("[clipboard]\nosc52 = {value}\n"));
            assert_eq!(
                settings,
                Settings {
                    osc52: Osc52::Off,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("clipboard.osc52"), "{value}");
            assert_eq!(diagnostic.line, Some(2), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`clipboard.osc52` must be \"copy\" or \"off\", found {found}; using \"off\""
                )
            );
        }
        // Bölüm yanlış türde: anahtarı kabul edilmemiş sayılıyor, yine kapalı.
        let (settings, diagnostic) = rejected("clipboard = \"copy\"\n");
        assert_eq!(settings.osc52, Osc52::Off);
        assert_eq!(diagnostic.key, Some("clipboard"));
    }

    #[test]
    fn rejected_osc52_is_off_even_when_the_given_settings_copy() {
        // Kayıt anının "geçerli değeri tut" kuralının tek istisnası:
        // `"of"` diye yanlış yazılmış bir kapatma panoyu açık tutmamalı.
        let current = Settings::default();
        assert_eq!(current.osc52, Osc52::Copy);
        for text in ["[clipboard]\nosc52 = \"of\"\n", "clipboard = 5\n"] {
            let parsed = Settings::parse_keeping(text, &current).expect("ayrıştırılabilir metin");
            assert_eq!(parsed.settings.osc52, Osc52::Off, "{text}");
            assert_eq!(parsed.diagnostics.len(), 1, "{text}");
        }
        // Dosyadan silinen anahtar ise varsayılana döner: kabul edilmeyen
        // bir değer yok, kullanıcı varsayılanı istiyor.
        let off = Settings {
            osc52: Osc52::Off,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("", &off).expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.osc52, Osc52::Copy);
    }

    #[test]
    fn osc52_change_is_a_terminal_change() {
        // Canlı değişim terminal seçeneklerinin **tamamıyla** gidiyor:
        // `scrollback` yanında taşınıyor, onu varsayılana çekmiyor.
        let before = clean("[terminal]\nscrollback = 500\n");
        let after = clean("[terminal]\nscrollback = 500\n[clipboard]\nosc52 = \"off\"\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: true,
                font: false,
                motion: false,
                caret: false
            }
        );
        assert_eq!(
            after.terminal(),
            TerminalOptions {
                scrollback: 500,
                osc52: Osc52::Off,
                cursor: CaretShape::default(),
                blink: CursorBlink::default(),
            }
        );
    }

    #[test]
    fn cursor_shape_is_read() {
        // Dosyada yoksa `block`: alacritty'nin varsayılanıyla aynı, yani
        // ayar gelmeden önceki davranış birebir korunuyor.
        assert_eq!(clean("").cursor, CaretShape::Block);
        assert_eq!(
            clean("[terminal]\ncursor = \"underline\"\n").cursor,
            CaretShape::Underline
        );
        // Satır içi tablo aynı bölüm.
        assert_eq!(
            clean("terminal = { cursor = \"beam\" }").cursor,
            CaretShape::Beam
        );
        // Tanınmayan değer anahtarı **değiştirmiyor** ve tanı bırakıyor;
        // büyük/küçük harf duyarlı.
        for text in [
            "[terminal]\ncursor = \"bar\"\n",
            "[terminal]\ncursor = \"Block\"\n",
        ] {
            let (settings, diagnostic) = rejected(text);
            assert_eq!(settings.cursor, CaretShape::Block, "{text}");
            assert_eq!(diagnostic.key, Some("terminal.cursor"), "{text}");
        }
        // Bölüm **bölüm değilse** (ör. `terminal = 1`) anahtar fallback'e
        // düşüyor — `scrollback`'in ikinci kolunun aynısı. Tanı bölümün
        // kendisine ait, anahtara değil.
        let (settings, diagnostic) = rejected("terminal = 1");
        assert_eq!(settings.cursor, CaretShape::Block);
        assert_eq!(diagnostic.key, Some("terminal"));
    }

    #[test]
    fn cursor_blink_is_read() {
        // Dosyada yoksa `off`: yanıp sönen imleç pencereyi kalıcı olarak
        // meşgul tutuyor ve bu kullanıcının **seçtiği** bir şey olmalı.
        assert_eq!(clean("").cursor_blink, CursorBlink::Off);
        assert_eq!(
            clean("[terminal]\ncursor_blink = \"auto\"\n").cursor_blink,
            CursorBlink::Auto
        );
        assert_eq!(
            clean("terminal = { cursor_blink = \"on\" }").cursor_blink,
            CursorBlink::On
        );
        let (settings, diagnostic) = rejected("[terminal]\ncursor_blink = \"yes\"\n");
        assert_eq!(settings.cursor_blink, CursorBlink::Off);
        assert_eq!(diagnostic.key, Some("terminal.cursor_blink"));
    }

    #[test]
    fn the_blink_setting_overrides_what_the_program_asks() {
        // `auto` uygulamayı izliyor; ötekiler **eziyor** ve ezmenin iki yönü
        // de şart: `\e[5 q` gönderen vim `off`'u delememeli, `\e[2 q`
        // gönderen bir program da `on`'u susturmamalı.
        assert!(CursorBlink::Auto.resolve(true));
        assert!(!CursorBlink::Auto.resolve(false));
        assert!(CursorBlink::On.resolve(false), "on ezmedi");
        assert!(!CursorBlink::Off.resolve(true), "off ezmedi");
    }

    #[test]
    fn cursor_motion_is_read() {
        // Dosyada yoksa `spring`: özelliği kapalı sevk etmemek kararın kendisi
        // (008 Karar 6).
        assert_eq!(clean("").cursor_motion, CursorMotion::Spring);
        assert_eq!(
            clean("[motion]\ncursor_motion = \"snap\"\n").cursor_motion,
            CursorMotion::Snap
        );
        assert_eq!(
            clean("[motion]\ncursor_motion = \"ease\"\n").cursor_motion,
            CursorMotion::Ease
        );
        assert_eq!(
            clean("motion = { cursor_motion = \"spring\" }\n").cursor_motion,
            CursorMotion::Spring
        );
    }

    #[test]
    fn unrecognized_cursor_motion_keeps_its_own_key() {
        // `osc52`'nin "kabul edilmeyen değer kapalıya düşer" istisnası buraya
        // **geçmiyor**: yanlış tahminin bedeli görünür bir animasyon, sessiz
        // bir pano sızıntısı değil (008 Karar 6). Yani kural öteki
        // anahtarlarınki — anahtar kendi değerinde kalır, yanında tanı.
        for (value, found) in [
            ("\"sprong\"", "\"sprong\""),
            ("\"Spring\"", "\"Spring\""),
            ("true", "a boolean"),
            ("{ style = \"snap\" }", "a section"),
        ] {
            let (settings, diagnostic) = rejected(&format!("[motion]\ncursor_motion = {value}\n"));
            assert_eq!(settings, Settings::default(), "{value}");
            assert_eq!(diagnostic.key, Some("motion.cursor_motion"), "{value}");
            assert_eq!(diagnostic.line, Some(2), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`motion.cursor_motion` must be \"snap\", \"ease\" or \"spring\", \
found {found}; using \"spring\""
                )
            );
        }
        // Kayıt anında yerine geçen değer varsayılan değil **geçerli** ayar:
        // ekrandaki stil bir yazım hatasıyla değişmemeli.
        let current = Settings {
            cursor_motion: CursorMotion::Ease,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[motion]\ncursor_motion = \"sprong\"\n", &current)
            .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.cursor_motion, CursorMotion::Ease);
        assert!(parsed.diagnostics[0].message.ends_with("using \"ease\""));
        // Bölüm yanlış türde: anahtarı kabul edilmemiş sayılıyor.
        let parsed =
            Settings::parse_keeping("motion = 5\n", &current).expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.cursor_motion, CursorMotion::Ease);
        assert_eq!(parsed.diagnostics[0].key, Some("motion"));
    }

    #[test]
    fn cursor_motion_change_is_a_motion_change() {
        // Kendi farkı: stil renderer'ın ritmine gidiyor, oturuma ya da fonta
        // değil — ikisini de kıpırdatmamalı.
        let before = clean("");
        let after = clean("[motion]\ncursor_motion = \"snap\"\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: false,
                font: false,
                motion: true,
                caret: false
            }
        );
        assert_eq!(after.changes(&after), Changes::default());
    }

    #[test]
    fn reduce_motion_is_read() {
        // Dosyada yoksa `system`: en olası seçim "sistemi izle" ve anahtarın
        // üç değerli olmasının sebebi de bu (`ReduceMotion`).
        assert_eq!(clean("").reduce_motion, ReduceMotion::System);
        assert_eq!(
            clean("[motion]\nreduce_motion = \"on\"\n").reduce_motion,
            ReduceMotion::On
        );
        assert_eq!(
            clean("[motion]\nreduce_motion = \"off\"\n").reduce_motion,
            ReduceMotion::Off
        );
        assert_eq!(
            clean("motion = { reduce_motion = \"system\" }\n").reduce_motion,
            ReduceMotion::System
        );
        // İki anahtar birbirini ezmiyor: aynı bölümde ikisi de okunuyor.
        let both = clean("[motion]\ncursor_motion = \"ease\"\nreduce_motion = \"on\"\n");
        assert_eq!(both.cursor_motion, CursorMotion::Ease);
        assert_eq!(both.reduce_motion, ReduceMotion::On);
    }

    #[test]
    fn unrecognized_reduce_motion_keeps_its_own_key() {
        // `cursor_motion` ile aynı kural: anahtar kendi değerinde kalır,
        // yanında tanı — ve **yalnız kendi** anahtarı etkilenir.
        for (value, found) in [
            ("\"yes\"", "\"yes\""),
            ("\"System\"", "\"System\""),
            ("true", "a boolean"),
        ] {
            let text = format!("[motion]\ncursor_motion = \"snap\"\nreduce_motion = {value}\n");
            let (settings, diagnostic) = rejected(&text);
            assert_eq!(
                settings,
                Settings {
                    cursor_motion: CursorMotion::Snap,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("motion.reduce_motion"), "{value}");
            assert_eq!(diagnostic.line, Some(3), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`motion.reduce_motion` must be \"system\", \"on\" or \"off\", \
found {found}; using \"system\""
                )
            );
        }
        // Kayıt anında yerine geçen değer varsayılan değil **geçerli** ayar.
        let current = Settings {
            reduce_motion: ReduceMotion::Off,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[motion]\nreduce_motion = \"yes\"\n", &current)
            .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.reduce_motion, ReduceMotion::Off);
        assert!(parsed.diagnostics[0].message.ends_with("using \"off\""));
        // Bölüm yanlış türde: iki anahtar da kabul edilmemiş sayılıyor.
        let parsed =
            Settings::parse_keeping("motion = 5\n", &current).expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.reduce_motion, ReduceMotion::Off);
    }

    #[test]
    fn reduce_motion_change_is_a_motion_change() {
        // `cursor_motion` ile **aynı** farka düşüyor: ikisi de aynı yere,
        // aynı çağrı yerinde gidiyor (`Changes::motion`).
        let before = clean("");
        let after = clean("[motion]\nreduce_motion = \"on\"\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: false,
                font: false,
                motion: true,
                caret: false
            }
        );
        assert_eq!(after.changes(&after), Changes::default());
    }

    #[test]
    fn shell_integration_is_read() {
        assert_eq!(clean("").shell_integration, ShellIntegration::Auto);
        assert_eq!(
            clean("[shell]\nintegration = \"auto\"\n").shell_integration,
            ShellIntegration::Auto
        );
        assert_eq!(
            clean("[shell]\nintegration = \"off\"\n").shell_integration,
            ShellIntegration::Off
        );
        assert_eq!(
            clean("shell = { integration = \"off\" }\n").shell_integration,
            ShellIntegration::Off
        );
    }

    #[test]
    fn unrecognized_shell_integration_keeps_its_own_key() {
        // `cursor_motion` ile aynı kural: anahtar kendi değerinde kalır,
        // yanında tanı. `osc52`'nin "kapalıya düş" istisnası buraya geçmiyor.
        for (value, found) in [
            ("\"on\"", "\"on\""),
            ("\"Auto\"", "\"Auto\""),
            ("false", "a boolean"),
        ] {
            let (settings, diagnostic) = rejected(&format!("[shell]\nintegration = {value}\n"));
            assert_eq!(settings, Settings::default(), "{value}");
            assert_eq!(diagnostic.key, Some("shell.integration"), "{value}");
            assert_eq!(diagnostic.line, Some(2), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`shell.integration` must be \"auto\", \"blocks\" or \"off\", \
found {found}; using \"auto\""
                )
            );
        }
        // Kayıt anında yerine geçen değer varsayılan değil **geçerli** ayar.
        let current = Settings {
            shell_integration: ShellIntegration::Off,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[shell]\nintegration = \"on\"\n", &current)
            .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.shell_integration, ShellIntegration::Off);
        assert!(parsed.diagnostics[0].message.ends_with("using \"off\""));
        // Bölüm yanlış türde: anahtar da kabul edilmemiş sayılıyor.
        let parsed =
            Settings::parse_keeping("shell = 5\n", &current).expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.shell_integration, ShellIntegration::Off);
    }

    #[test]
    fn shell_integration_is_not_a_live_change() {
        // Sözleşmenin tek istisnası ve sınaması burada: anahtar değişse bile
        // `Changes` boş kalıyor, çünkü kabuk çoktan doğmuş ve uygulanacak bir
        // şey yok. Bir gün `Changes`'e kol takılırsa burası kızarır ve
        // `docs/AYARLAR.md`'nin "sonraki oturumda geçerli" cümlesi de
        // düzeltilmek zorunda kalır.
        let before = clean("");
        let after = clean("[shell]\nintegration = \"off\"\n");
        assert_ne!(before.shell_integration, after.shell_integration);
        assert_eq!(before.changes(&after), Changes::default());
    }

    #[test]
    fn caret_style_is_read_and_bounded() {
        // **Varsayılan bugünkü görüntü:** dosyası olmayan kullanıcı 015'in
        // sevk ettiği imleci görüyor ve `bt-gpu` aynı sabitleri import ediyor
        // (016 R2) — iki literal olsaydı piksel bekçileri kör kalırdı.
        assert_eq!(clean("").caret, CaretStyle::default());
        assert_eq!(
            (
                CaretStyle::default().radius_ratio,
                CaretStyle::default().glow
            ),
            (CURSOR_RADIUS, CURSOR_GLOW)
        );

        let read = clean("[terminal]\ncursor_radius = 0.3\ncursor_glow = 0\n").caret;
        assert_eq!((read.radius_ratio, read.glow), (0.3, 0.0));
        // Tam sayı da geçerli: kullanıcının yazacağı `cursor_glow = 2`.
        assert_eq!(clean("[terminal]\ncursor_glow = 2\n").caret.glow, 2.0);
    }

    #[test]
    fn a_rejected_caret_number_keeps_its_own_key() {
        // **Kırpma yok, red var** (016 R1.3): aralık dışı değer sessizce uca
        // çekilseydi kullanıcı yanlış yazdığını hiç görmezdi. Reddedilen
        // anahtar kendi varsayılanında kalıyor, **komşusu okunuyor**.
        for value in ["1.5", "-0.1", "\"big\""] {
            let (settings, diagnostic) = rejected(&format!(
                "[terminal]\ncursor_radius = {value}\ncursor_glow = 2.0\n"
            ));
            assert_eq!(
                settings.caret.radius_ratio, CURSOR_RADIUS,
                "{value} kendi anahtarını değiştirdi"
            );
            assert_eq!(
                settings.caret.glow, 2.0,
                "{value} komşu anahtarı da düşürdü"
            );
            assert_eq!(diagnostic.key, Some("terminal.cursor_radius"), "{value}");
        }
    }

    #[test]
    fn the_unfocused_caret_key_is_read_and_diagnosed() {
        assert_eq!(clean("").caret.unfocused, UnfocusedCaret::Hollow);
        assert_eq!(
            clean("[terminal]\ncursor_unfocused = \"solid\"\n")
                .caret
                .unfocused,
            UnfocusedCaret::Solid
        );

        // Büyük/küçük harf duyarlı ve kabul edilmeyen değer kendi anahtarını
        // değiştirmiyor; beklenen liste **tanıda** yazılı olmalı, yoksa
        // kullanıcı doğru yazılışı dosyadan aramak zorunda kalır.
        let (settings, diagnostic) = rejected("[terminal]\ncursor_unfocused = \"Solid\"\n");
        assert_eq!(settings.caret.unfocused, UnfocusedCaret::Hollow);
        assert_eq!(diagnostic.key, Some("terminal.cursor_unfocused"));
        assert!(
            diagnostic.message.contains("\"hollow\" or \"solid\""),
            "tanı beklenen listeyi saymıyor: {}",
            diagnostic.message
        );
    }

    #[test]
    fn a_rejected_caret_number_names_the_value_the_user_kept() {
        // **Tanı metni kullanıcının yazdığı sayıyı göstermeli**, float
        // gürültüsünü değil (`/code-review`): `CaretStyle` `f32` iken
        // `f64::from(0.10f32)` `0.10000000149011612` ediyordu ve mesaj
        // "using 0.10000000149011612" diyordu. Kardeş sınamalar (font, tema)
        // mesajın tamamını sınıyor; bu anahtar yalnız `key`'e bakıyordu ve
        // kusur oradan sızdı.
        let (_, diagnostic) = rejected("[terminal]\ncursor_radius = 1.5\n");
        assert_eq!(
            diagnostic.message,
            "`terminal.cursor_radius` must be a number between 0 and 0.5, \
found 1.5; using 0.1"
        );
        let (_, diagnostic) = rejected("[terminal]\ncursor_glow = 9\n");
        assert_eq!(
            diagnostic.message,
            "`terminal.cursor_glow` must be a number between 0 and 3, found 9; using 1"
        );
    }

    #[test]
    fn a_caret_change_is_its_own_field() {
        // `changes.terminal` bugün `terminal() != terminal()`'in ta kendisi ve
        // bu iki anahtar `TerminalOptions`'a **girmiyor**; aynı alana
        // binselerdi bir yarıçap değişimi oturumu baştan kurdururdu.
        let before = clean("");
        let after = clean("[terminal]\ncursor_radius = 0.2\n");
        let changes = before.changes(&after);
        assert!(changes.caret, "imleç farkı görülmedi");
        assert!(!changes.terminal, "yarıçap oturumu yeniden kurduruyor");
        assert!(!changes.font && !changes.motion);

        // Ters yön: terminal anahtarı değişince imleç alanı kımıldamıyor.
        let scrolled = clean("[terminal]\nscrollback = 50\n");
        let changes = before.changes(&scrolled);
        assert!(changes.terminal && !changes.caret, "{changes:?}");
    }

    #[test]
    fn line_height_is_read_and_bounded() {
        assert_eq!(clean("").font.line_height, 1.0, "varsayılan fontun kendi");
        assert_eq!(clean("[font]\nline_height = 1.4\n").font.line_height, 1.4);
        // Tam sayı da çarpan: kullanıcının yazacağı `line_height = 2`.
        assert_eq!(clean("[font]\nline_height = 2\n").font.line_height, 2.0);

        // **İki uçlu ve iki ucun gerekçesi ayrı.** Alt uç bir bekçiyi
        // koruyor: fontun istediğinden kısa hücre `g` ve `y` kuyruklarını
        // kırpardı (`bt-atlas`'ta `descender_fits_in_the_cell`). Üst uç bir
        // bütçe: yuva `cell_w × cell_h` bayt ve atlas sabit boyutlu, yani
        // çarpan büyüdükçe sığan glyph sayısı düşüyor.
        for value in ["0.9", "0", "-1", "2.5", "1e9"] {
            let (settings, diagnostic) = rejected(&format!("[font]\nline_height = {value}\n"));
            assert_eq!(settings, Settings::default(), "{value}");
            assert_eq!(diagnostic.key, Some("font.line_height"), "{value}");
            assert!(
                diagnostic.message.contains("between 1 and 2"),
                "{value}: {}",
                diagnostic.message
            );
        }
        // Sayı olmayan değer de kendi anahtarında kalıyor.
        let (_, diagnostic) = rejected("[font]\nline_height = \"big\"\n");
        assert!(diagnostic.message.contains("must be a number"));

        // Komşu anahtar düşmüyor: reddedilen çarpan puntoyu etkilemez.
        let parsed = Settings::parse("[font]\nline_height = 9\nsize = 18\n").expect("ayrıştırılır");
        assert_eq!(parsed.settings.font.size, 18.0);
        assert_eq!(parsed.settings.font.line_height, 1.0);
    }

    #[test]
    fn the_retired_prompt_key_is_kept_but_not_read() {
        // **012 phase-10: `shell.prompt` emekli.** Ayrı anahtar ekranda iki
        // prompt üretiyordu (kullanıcınınki ızgarada, dock'unki altta) ve
        // caret ikisi arasında sıçrıyordu; seçim `integration`'ın üçüncü
        // değerine taşındı.
        //
        // Emeklilik "bilinmeyen anahtar korunur, anahtar silinmez" kuralının
        // üçüncü hâli: satır dosyada duruyor, davranışa hiç karışmıyor, ama
        // **sessiz de değil**. Sessiz olsaydı kullanıcı yazdığı satırın bir
        // işe yaradığını sanırdı.
        for text in [
            "[shell]\nprompt = \"shell\"\n",
            "[shell]\nprompt = \"terminal\"\n",
            // Değeri hiç okunmadığı için tanınmayan değer de aynı yola düşüyor:
            // artık "kabul edilmedi" diye bir şey yok, anahtarın kendisi yok.
            "[shell]\nprompt = false\n",
            "shell = { prompt = \"shell\" }\n",
        ] {
            let parsed = Settings::parse(text).expect("ayrıştırılır");
            assert_eq!(parsed.settings, Settings::default(), "{text:?}");
            assert_eq!(parsed.diagnostics.len(), 1, "{text:?}");
            assert_eq!(
                parsed.diagnostics[0].message,
                "`shell.prompt` is no longer read; use `shell.integration = \"blocks\"` \
                 to keep your own prompt",
                "{text:?}"
            );
        }
    }

    #[test]
    fn the_retired_key_leaves_integration_alone() {
        // Aynı bölümde iki anahtar: emeklinin varlığı ötekini düşürmemeli.
        let parsed = Settings::parse("[shell]\nprompt = \"zsh\"\nintegration = \"blocks\"\n")
            .expect("ayrıştırılır");
        assert_eq!(parsed.settings.shell_integration, ShellIntegration::Blocks);
        assert_eq!(parsed.diagnostics.len(), 1);
    }

    #[test]
    fn integration_has_three_rungs() {
        assert_eq!(clean("").shell_integration, ShellIntegration::Auto);
        for (value, expected) in [
            ("auto", ShellIntegration::Auto),
            ("blocks", ShellIntegration::Blocks),
            ("off", ShellIntegration::Off),
        ] {
            assert_eq!(
                clean(&format!("[shell]\nintegration = \"{value}\"\n")).shell_integration,
                expected,
                "{value}"
            );
        }
        // İki türetilmiş soru ve **ayrı** cevaplar: `blocks` sarmalayıcıyı
        // kuruyor (bloklar ve işaretler için) ama dock istemiyor. Birini
        // ötekinden türetmek phase-10'un kapattığı hatayı geri getirirdi.
        assert!(ShellIntegration::Auto.installs_wrapper());
        assert!(ShellIntegration::Blocks.installs_wrapper());
        assert!(!ShellIntegration::Off.installs_wrapper());
        assert!(ShellIntegration::Auto.wants_dock());
        assert!(!ShellIntegration::Blocks.wants_dock());
        assert!(!ShellIntegration::Off.wants_dock());
    }

    #[test]
    fn unusable_file_closes_osc52_only() {
        assert_eq!(
            Settings::for_unusable_file(),
            Settings {
                osc52: Osc52::Off,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn theme_name_outside_themes_dir_falls_back() {
        let (settings, diagnostic) = rejected("[appearance]\ntheme = \"../settings\"\n");
        assert_eq!(settings.theme, "system");
        assert_eq!(diagnostic.key, Some("appearance.theme"));
        assert_eq!(diagnostic.line, Some(2));
        assert_eq!(
            diagnostic.message,
            "`appearance.theme` must be a theme name without `/`, found \"../settings\"; using \"system\""
        );
        assert_eq!(rejected("[appearance]\ntheme = \"\"\n").0.theme, "system");
        assert_eq!(
            rejected("[appearance]\ntheme = \"a\\u0000\"\n").0.theme,
            "system"
        );

        let (settings, diagnostic) = rejected("[appearance]\ntheme = 3\n");
        assert_eq!(settings.theme, "system");
        assert_eq!(
            diagnostic.message,
            "`appearance.theme` must be a string, found an integer; using \"system\""
        );

        // Çiftin anahtarları da aynı kuraldan, kendi varsayılanlarına.
        let (settings, diagnostic) = rejected("[appearance]\ndark_theme = \"a/b\"\n");
        assert_eq!(settings.dark_theme, "bateri");
        assert_eq!(diagnostic.key, Some("appearance.dark_theme"));
        let (settings, diagnostic) = rejected("[appearance]\nlight_theme = false\n");
        assert_eq!(settings.light_theme, "bateri-light");
        assert_eq!(
            diagnostic.message,
            "`appearance.light_theme` must be a string, found a boolean; using \"bateri-light\""
        );
    }

    #[test]
    fn system_is_not_a_name_for_the_pair() {
        // `light_theme = "system"` kendi kendine dönen bir seçim olurdu.
        let (settings, diagnostic) = rejected("[appearance]\nlight_theme = \"system\"\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostic.key, Some("appearance.light_theme"));
        assert_eq!(
            diagnostic.message,
            "`appearance.light_theme` must name a theme, not \"system\"; using \"bateri-light\""
        );
        assert_eq!(
            rejected("[appearance]\ndark_theme = \"system\"\n")
                .0
                .dark_theme,
            "bateri"
        );
        // `theme` için ayrılmış değer geçerli.
        assert!(clean("[appearance]\ntheme = \"system\"\n").follows_system());
    }

    #[test]
    fn section_of_wrong_type_is_diagnosed() {
        let (settings, diagnostic) = rejected("terminal = 5\n");
        assert_eq!(settings, Settings::default());
        assert_eq!(diagnostic.key, Some("terminal"));
        assert_eq!(diagnostic.line, Some(1));

        // Bölüm dizisi kendi adıyla söyleniyor: "section … found a section"
        // kendiyle çelişirdi.
        let (_, diagnostic) = rejected("[[terminal]]\nscrollback = 5\n");
        assert_eq!(
            diagnostic.message,
            "`terminal` must be a section, found an array of sections (`[[…]]`)"
        );
    }

    #[test]
    fn unknown_keys_and_sections_are_silent() {
        // Sonraki setlerin anahtarları bugün tanı üretmemeli: `keypress` ve
        // `intensity` referansın `[motion]` bölümünde var, bizde yok
        // (008 → Kapsam dışı).
        let text = "\
future = true
[terminal]
scrollback = 42
shape = \"block\"
[motion]
keypress = \"pop\"
intensity = 0.5
[font]
line_height = 1.2
";
        let settings = clean(text);
        assert_eq!(settings.scrollback, 42);
        assert_eq!(settings.cursor_motion, CursorMotion::Spring);
    }

    #[test]
    fn unparseable_text_is_a_separate_result() {
        let err = Settings::parse("[terminal]\nscrollback = \n").expect_err("geçersiz TOML");
        assert_eq!(err.key, None);
        assert_eq!(err.line, Some(2));
        assert!(err.message.starts_with("invalid TOML: "), "{err}");
        // Tanı tek satır ve "beklenen" listesi yok: alt başlık başlıkla aynı
        // satırda çiziliyor.
        assert!(!err.to_string().contains('\n'), "{err}");
        assert!(!err.message.contains("expected"), "{err}");

        assert!(Settings::parse("[terminal").is_err());
        // TOML'un tam sayı sınırını aşan sayı tavana kırpılamaz: değer hiç
        // okunamıyor ve belge düşüyor (`docs/AYARLAR.md`).
        assert!(Settings::parse("[terminal]\nscrollback = 99999999999999999999\n").is_err());
    }

    #[test]
    fn theme_write_keeps_every_other_byte() {
        // Kullanıcının dosyası: yorumlar, boş satırlar, anahtar sırası,
        // tanımadığımız anahtar ve bölüm, değerin yanındaki yorum. Menüden
        // tema seçmek yalnız değeri değiştirir.
        let text = "\
# my settings

[terminal]
scrollback = 500  # plenty
shape = \"block\"

[appearance]
# picked by hand
theme = \"system\"   # follows macOS
light_theme = \"paper\"
dark_theme = \"ink\"
future = true

[motion]
cursor = \"spring\"
";
        let written = Settings::with_theme(text, "bateri").expect("yazılabilir metin");
        assert_eq!(
            written,
            text.replace("theme = \"system\"", "theme = \"bateri\"")
        );
        // Çift yerinde: `"system"`'e dönmek onu geri getirir.
        let settings = clean(&written);
        assert_eq!(
            (
                settings.theme.as_str(),
                settings.light_theme.as_str(),
                settings.dark_theme.as_str(),
                settings.scrollback
            ),
            ("bateri", "paper", "ink", 500)
        );
        assert_eq!(
            Settings::with_theme(&written, SYSTEM_THEME).expect("yazılabilir metin"),
            text
        );
    }

    #[test]
    fn theme_write_adds_what_is_missing() {
        // Bölüm yok: sona eklenir, öncesi aynı kalır.
        let text = "[terminal]\nscrollback = 5\n";
        let written = Settings::with_theme(text, "paper").expect("yazılabilir metin");
        assert_eq!(
            written,
            format!("{text}\n[appearance]\ntheme = \"paper\"\n")
        );
        assert_eq!(clean(&written).theme, "paper");

        // Boş metin.
        assert_eq!(
            Settings::with_theme("", "paper").expect("yazılabilir metin"),
            "[appearance]\ntheme = \"paper\"\n"
        );

        // Bölüm var, anahtar yok: bölümün içine, sonraki bölümden önce.
        let text = "[appearance]\ndark_theme = \"ink\"\n\n[font]\nsize = 14\n";
        let written = Settings::with_theme(text, "paper").expect("yazılabilir metin");
        assert_eq!(
            written,
            "[appearance]\ndark_theme = \"ink\"\ntheme = \"paper\"\n\n[font]\nsize = 14\n"
        );
    }

    #[test]
    fn theme_write_leaves_trailing_comments_where_they_were() {
        // `/code-review` bulgusu: belge sonundaki yorum `toml_edit`'te belgenin
        // kuyruğu, yeni bölüm onun **önüne** ekleniyordu. Yorumu kaldıran
        // kullanıcının satırı `appearance.family` olur ve sessizce
        // yoksayılırdı.
        let text = "[font]\nsize = 14\n# family = \"Menlo\"\n";
        let written = Settings::with_theme(text, "paper").expect("yazılabilir metin");
        assert_eq!(
            written,
            format!("{text}\n[appearance]\ntheme = \"paper\"\n")
        );
        let uncommented = written.replace("# family", "family");
        assert_eq!(clean(&uncommented).font.family.as_deref(), Some("Menlo"));
    }

    #[test]
    fn theme_write_keeps_crlf_line_endings() {
        // `/code-review` bulgusu: `toml_edit` satır sonlarını LF yazıyor; tek
        // bir tema seçimi dotfile deposunda bütün dosyayı değişmiş gösterirdi.
        let text = "[appearance]\r\ntheme = \"a\"  # c\r\n\r\n[font]\r\nsize = 14\r\n";
        assert_eq!(
            Settings::with_theme(text, "paper").expect("yazılabilir metin"),
            text.replace("\"a\"", "\"paper\"")
        );
        // Eklenen bölüm de dosyanın satır sonuyla.
        assert_eq!(
            Settings::with_theme("[font]\r\nsize = 14\r\n", "paper").expect("yazılabilir metin"),
            "[font]\r\nsize = 14\r\n\r\n[appearance]\r\ntheme = \"paper\"\r\n"
        );
        // `/code-review` bulgusu: çok satırlı metnin içindeki `\r\n`'i
        // `toml_edit` ham bırakıyor; çeviri onu `\r\r\n` yapıp dosyayı
        // geçersiz bırakıyordu. Sonuç ayrıştırılabilir ve metin aynı.
        let text = "[appearance]\r\ntheme = \"a\"\r\n[notes]\r\nnote = \"\"\"x\r\ny\"\"\"\r\n";
        let written = Settings::with_theme(text, "paper").expect("yazılabilir metin");
        assert_eq!(written, text.replace("\"a\"", "\"paper\""));
        assert_eq!(clean(&written).theme, "paper");
    }

    #[test]
    fn theme_write_keeps_the_way_the_section_is_written() {
        for (text, expected) in [
            // Satır içi tablo satır içi kalır.
            (
                "appearance = { theme = \"a\", dark_theme = \"ink\" }\n",
                "appearance = { theme = \"paper\", dark_theme = \"ink\" }\n",
            ),
            // Noktalı anahtar noktalı kalır.
            (
                "appearance.theme = \"a\"\n",
                "appearance.theme = \"paper\"\n",
            ),
            // Kabul edilmeyen türdeki değerin yerine: kullanıcı bir tema seçti.
            (
                "[appearance]\ntheme = 3 # oops\n",
                "[appearance]\ntheme = \"paper\" # oops\n",
            ),
        ] {
            let written = Settings::with_theme(text, "paper").expect("yazılabilir metin");
            assert_eq!(written, expected);
            assert_eq!(clean(&written).theme, "paper", "{text}");
        }
        // Yalnız alt bölümü yazılmış `[appearance]`: sonuç yine okunuyor.
        let written = Settings::with_theme("[appearance.extra]\nx = 1\n", "paper")
            .expect("yazılabilir metin");
        assert_eq!(clean(&written).theme, "paper", "{written}");
    }

    #[test]
    fn theme_write_refuses_what_it_would_destroy() {
        // Ayrıştırılamayan metin: metin **üretilmez**, kullanıcının yarım işi
        // ezilmez.
        let err =
            Settings::with_theme("[appearance]\ntheme = \"a\n", "paper").expect_err("geçersiz");
        assert_eq!((err.key, err.line), (None, Some(2)));
        assert!(err.message.starts_with("invalid TOML: "), "{err}");

        // Bölüm olmayan `appearance`: yerine tablo yazmak değeri silerdi.
        for text in ["appearance = 1\n", "[[appearance]]\ntheme = \"a\"\n"] {
            let err = Settings::with_theme(text, "paper").expect_err("bölüm değil");
            assert_eq!(err.key, Some("appearance"), "{text}");
            assert!(err.message.contains("must be a section"), "{err}");
        }
        // Tablo olan `theme`: yerine değer yazmak alt tabloyu silerdi — iki
        // yazılışı da (`/code-review` bulgusu: satır içi hâl geçiyordu).
        for text in [
            "[appearance.theme]\nx = 1\n",
            "[appearance]\ntheme = { light = \"paper\" }\n",
        ] {
            let err = Settings::with_theme(text, "paper").expect_err("tema bir bölüm");
            assert_eq!(err.key, Some("appearance.theme"), "{text}");
            assert!(err.message.contains("found a section"), "{err}");
        }
    }

    #[test]
    fn parser_reason_drops_the_expected_list() {
        assert_eq!(
            parser_reason("invalid escape, expected `b`, `e`"),
            "invalid escape"
        );
        assert_eq!(parser_reason("duplicate key"), "duplicate key");
    }

    #[test]
    fn line_of_counts_from_one_and_rejects_out_of_range() {
        assert_eq!(line_of("a\nb\nc", 0), Some(1));
        assert_eq!(line_of("a\nb\nc", 2), Some(2));
        assert_eq!(line_of("a\nb\nc", 4), Some(3));
        assert_eq!(line_of("a", 99), None);
    }
}
