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
//! anahtarı (`[motion] intensity`) bugünkü sürümde tanı üretmemeli.
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
/// Tavan kullanıcı girdisinin kuralı, `Session`'ın değişmezi değil —
/// `SessionOptions.scrollback`'i kırpan başka bir kapı yok ve olması da
/// gerekmiyor, oraya giden tek değer bu ayrıştırıcıdan geçiyor. `pub`, çünkü
/// ayar penceresinin alanı da aynı tavanı soruyor: ikinci bir kopya pencereye
/// ayrıştırıcının reddettiği bir sayı yazdırabilirdi (029).
pub const SCROLLBACK_MAX: usize = 100_000;

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
    /// Ayar dosyasındaki yazılışların **tek listesi**
    /// ([`UnfocusedCaret::NAMES`]'in gerekçesi).
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("snap", Self::Snap),
        ("ease", Self::Ease),
        ("spring", Self::Spring),
    ];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
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
    /// Ayar dosyasındaki yazılışların tek listesi.
    pub const NAMES: &'static [(&'static str, Self)] =
        &[("auto", Self::Auto), ("on", Self::On), ("off", Self::Off)];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
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
pub const CURSOR_BLINK_RANGE: std::ops::RangeInclusive<f64> = 0.05..=5.0;

/// Yarıçabın kabul aralığı; yarım = hücrenin yarısı, ötesi anlamsız.
///
/// Aralıklar `pub`: ayar penceresinin kontrolleri de bu uçlarla kuruluyor, yani
/// ayrıştırıcının kabul ettiği ile pencerenin sunduğu tek yerden (029).
pub const CURSOR_RADIUS_RANGE: std::ops::RangeInclusive<f64> = 0.0..=0.5;

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
pub const CURSOR_GLOW_RANGE: std::ops::RangeInclusive<f64> = 0.0..=3.0;

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
    /// [`Self::name`] de ayar penceresinin seçenekleri de buradan okuyor.
    ///
    /// İki yerde yazılsaydı bir varyantın yazılışını değiştirmek, kullanıcıya
    /// **ayrıştırıcının reddettiği** bir değer öneren bir tanı üretirdi. Sıra
    /// tanı metninin sırası (`"hollow" or "solid"`).
    pub const NAMES: &'static [(&'static str, Self)] =
        &[("hollow", Self::Hollow), ("solid", Self::Solid)];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[terminal] confirm_close`: pencere, sekme ya da uygulama kapanırken ne
/// zaman sorulsun (`.tasks/028-kapatma-onayi/discussion.md` → Karar 6).
///
/// "Koşan" kabuğun **dışında** ön planda bir program demek (vim, `ssh`,
/// Claude Code); arka plan işi ve kabuğun kendi döngüsü sayılmıyor. Tespit
/// `bt-shell`'de, süreç tablosundan — burada yalnız kullanıcının seçimi.
///
/// `TerminalOptions`'a ve [`Changes`]'e **girmiyor** (emsal [`CaretStyle`]):
/// değer kapanış anında güncel ayardan okunuyor, yani kayıt anında geçerli
/// olması bedava ve oturumlara giden bir yol gerekmiyor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConfirmClose {
    /// Hiç sorma.
    Never,
    /// Ön planda kabuğun dışında bir program koşuyorsa sor.
    #[default]
    Running,
    /// Kabuk boştayken de sor.
    Always,
}

impl ConfirmClose {
    /// Ayar dosyasındaki yazılışların tek listesi ([`UnfocusedCaret::NAMES`]
    /// ile aynı gerekçe).
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("never", Self::Never),
        ("running", Self::Running),
        ("always", Self::Always),
    ];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
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
    /// Ayar dosyasındaki yazılışların tek listesi.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("block", Self::Block),
        ("underline", Self::Underline),
        ("beam", Self::Beam),
    ];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
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
    /// Ayar dosyasındaki yazılışların tek listesi.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("system", Self::System),
        ("on", Self::On),
        ("off", Self::Off),
    ];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[motion] smooth_scroll`: geçmişte kaydırmak pürüzsüz mü, satır adımıyla
/// mı.
///
/// **`bool` değil**, dosyanın dizge-enum geleneği ([`ReduceMotion`],
/// `Osc52`): değer referansın anahtarının (`scroll.smooth`) anlamı, türü
/// bizim.
///
/// Tüketicisi `bt-shell` ([`CursorMotion`] emsali) ve orada Hareketi Azalt
/// ile `cursor_motion = "snap"`'le **tek `bool`'a** iniyor: üçünden biri
/// hareketi kapatıyorsa tekerlek bugünkü satır adımıyla gidiyor
/// (`.tasks/027-yumusak-kaydirma/discussion.md` → Karar 5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SmoothScroll {
    /// Trackpad parmağı izler, çentik süzülür, jest sonunda satıra oturur.
    #[default]
    On,
    /// Satır adımı — `"on"`'dan önceki davranışın ta kendisi.
    Off,
}

impl SmoothScroll {
    /// Ayar dosyasındaki yazılışların tek listesi.
    pub const NAMES: &'static [(&'static str, Self)] = &[("on", Self::On), ("off", Self::Off)];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[motion] keypress`: dock'ta yazılan glyph'in nasıl geldiği (030).
///
/// **Yalnız çizilebilen adlar** ([`Self::NAMES`]) ve bugün referansın
/// listesinin tamamı: adlar çizildikçe girdi — popup'ta ya da dosyada
/// çizilmeyen bir ad kabul edilseydi seçmek hiçbir şey yapmazdı
/// (`.tasks/030-dock-yazim-animasyonlari/discussion.md` → Karar 7).
/// Görünüşlerin tanımı Karar 6'nın tablosu; genlikler `bt-gpu`'nun
/// `shaders/glyph_fx.metal`'inde.
///
/// Tüketicisi `bt-gpu` ([`CursorMotion`] emsali) ve değer **ham** gidiyor:
/// `cursor_motion = "snap"` ile Hareketi Azalt'ın indirgemesi orada, imlecin
/// kipiyle aynı yerde. Sürelerin ve eğrinin sahibi de orası.
///
/// Varsayılan **[`Self::Fade`]**: kullanıcı animasyonu açıkça istedi ve kutudan
/// çıkınca görmeli; listenin en az yer değiştiren efekti — glyph yerinden
/// oynamıyor, yalnız beliriyor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Keypress {
    /// Glyph anında belirir.
    Off,
    /// Glyph yerinde saydamdan tam renge belirir.
    #[default]
    Fade,
    /// Glyph hücrenin biraz altından yukarı kayarak yerine oturur, kayarken
    /// belirir.
    Rise,
    /// Glyph küçük doğar, bir an yerinden biraz büyür ve yerine oturur.
    Pop,
    /// Glyph sol kenarından sağa doğru uzayarak çıkar.
    Extrude,
    /// Glyph temanın `cursor` renginde doğar ve kendi rengine soğur.
    Heat,
    /// Glyph yerinde belirir, üstünden büyüyerek sönen soluk bir kopyası
    /// dağılır.
    Echo,
    /// Glyph hücrenin üstünden düşer, hafifçe sekip yerine oturur.
    Drop,
    /// Önce çizgilerin çekirdeği görünür, mürekkep kenarlara yayılır.
    Ink,
    /// Glyph yatayda sıkışmış ve dikeyde uzamış doğar, kendi oranına açılır.
    Squeeze,
}

impl Keypress {
    /// Ayar dosyasındaki yazılışların tek listesi.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("off", Self::Off),
        ("fade", Self::Fade),
        ("rise", Self::Rise),
        ("pop", Self::Pop),
        ("extrude", Self::Extrude),
        ("heat", Self::Heat),
        ("echo", Self::Echo),
        ("drop", Self::Drop),
        ("ink", Self::Ink),
        ("squeeze", Self::Squeeze),
    ];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
    }
}

/// `[motion] erase`: dock'ta silinen glyph'in nasıl gittiği (030).
///
/// [`Keypress`]'in kardeşi, aynı kurallarla: yalnız çizilebilen adlar, ham
/// değer `bt-gpu`'ya. Varsayılan **[`Self::Recede`]** — gidişlerin en az yer
/// değiştireni, glyph yerinde küçülüp söner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Erase {
    /// Glyph anında kaybolur.
    Off,
    /// Glyph'in üstünde dairesel bir diyafram merkezine doğru kapanır.
    Iris,
    /// Glyph aşağı ve caret'e doğru çekilerek söner.
    Undertow,
    /// Glyph büyüyerek dışa doğru bir halka gibi dağılır ve söner.
    Echo,
    /// Glyph'in mürekkebi dağılır: kenarlar yayılıp incelirken söner.
    Bleed,
    /// Glyph yatay şeritlere ayrılır, şeritler sırayla yana kayıp çözülür.
    Unravel,
    /// Glyph merkezine doğru küçülerek geri çekilir ve söner.
    #[default]
    Recede,
    /// Glyph buharlaşır gibi yukarı süzülür, açılarak söner.
    Sublime,
    /// Glyph parçalara kırılır, parçalar hafif dönerek dağılıp düşer ve
    /// söner.
    Shatter,
}

impl Erase {
    /// Ayar dosyasındaki yazılışların tek listesi.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("off", Self::Off),
        ("iris", Self::Iris),
        ("undertow", Self::Undertow),
        ("echo", Self::Echo),
        ("bleed", Self::Bleed),
        ("unravel", Self::Unravel),
        ("recede", Self::Recede),
        ("sublime", Self::Sublime),
        ("shatter", Self::Shatter),
    ];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
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
    /// Ayar dosyasındaki yazılışların tek listesi.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("auto", Self::Auto),
        ("blocks", Self::Blocks),
        ("off", Self::Off),
    ];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        name_in(Self::NAMES, self)
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

/// Bir uzak host'un işareti (037 Karar 2, 3): anlam, renk değil — rengi
/// temanın rolünden ([`crate::Theme::mark_linear`]), yani açık/koyu geçişi
/// işareti kendiliğinden taşıyor.
///
/// `None` "işaretsiz" demek (uzak oturumun bugünkü `info`'su) ve desen
/// listesinde eşleşmeyi **bitiriyor** ([`host_mark`]): bir globun yakaladığı
/// tek bir host'u işaretsiz bırakmanın tek yolu o.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HostMark {
    /// Temanın `error`'u.
    Production,
    /// Temanın `warning`'i.
    Staging,
    /// Temanın `success`'i.
    Development,
    /// İşaretsiz: temanın `info`'su.
    #[default]
    None,
    /// Doğrudan renk, `0xRRGGBB` (`"#rrggbb"`). Temayla değişmiyor ve açık
    /// temada okunur olacağını kimse denetlemiyor — bedeli Karar 2'de adıyla;
    /// menü onu hiç yazmıyor.
    Rgb(u32),
}

impl HostMark {
    /// Adlı işaretlerin ayar dosyasındaki yazılışları; `Rgb` bir ad değil,
    /// `"#rrggbb"` biçimi.
    pub const NAMES: &'static [(&'static str, Self)] = &[
        ("production", Self::Production),
        ("staging", Self::Staging),
        ("development", Self::Development),
        ("none", Self::None),
    ];

    /// Ayar dosyasındaki yazılışı: adlı işaretin adı ([`Self::NAMES`]),
    /// doğrudan rengin `"#rrggbb"`'si — ayrıştırıcının okuduğunun tersi.
    pub fn written(self) -> String {
        match self {
            Self::Rgb(hex) => format!("#{hex:06x}"),
            named => Self::NAMES
                .iter()
                .find(|(_, mark)| *mark == named)
                .map_or_else(String::new, |(name, _)| (*name).to_owned()),
        }
    }
}

/// `[remote] hosts` dizisinin bir girdisi: desen ve işareti (037 Karar 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRule {
    /// `*` (boş dahil herhangi bir dizi) ve `?` (tek karakter), harf
    /// duyarsız; `@` taşıyorsa host'un tamamıyla, taşımıyorsa son `@`'ten
    /// sonrasıyla eşleşiyor ([`host_mark`]).
    pub pattern: String,
    pub mark: HostMark,
}

/// Host'un işareti: `rules`'un **ilk** eşleşen girdisininki, eşleşme yoksa
/// [`HostMark::None`] (037 Karar 2).
///
/// Girdi uzak oturumun gösterdiği host (036: kullanıcının yazdığı gibi,
/// şema ve port atılmış). Desende `@` yoksa girdinin son `@`'ten sonraki
/// kısmı eşleşiyor — `deploy@prod` ile `prod` aynı makine; desende `@` varsa
/// girdinin tamamı, yani `root@*` yazılabiliyor. Sıra dizide, çünkü TOML
/// tablosunun anahtarları anlamca sırasız. `None` girdisi eşleşmeyi orada
/// bitiriyor ve sonucu da `None`.
///
/// Saf ve kare yolunda değil: `Session` onu yalnız uzak durumun ve listenin
/// değiştiği iki kenarda çağırıyor.
pub fn host_mark(rules: &[HostRule], host: &str) -> HostMark {
    let bare = bare_host(host);
    rules
        .iter()
        .find(|rule| {
            let subject = if rule.pattern.contains('@') {
                host
            } else {
                bare
            };
            glob_matches(&rule.pattern, subject)
        })
        .map_or(HostMark::None, |rule| rule.mark)
}

/// `*` ve `?`'li desen, harf duyarsız; sınıf (`[a-z]`) ve küme (`{a,b}`)
/// yok — onlar ayrı bir glob kütüphanesi demek (Karar 2).
///
/// İki taraf da **bir kez** küçük harfe iniyor ve karşılaştırma karakter
/// dizileri üstünde: harf katlaması bir karakteri birden çoğuna açabiliyor
/// (`İ`), yani karakter karakter katlamak `?`'in saydığını kaydırırdı.
/// Geri izleme yalnız son `*`'a — klasik doğrusal eşleştirici.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let (mut p, mut t) = (0, 0);
    // Son `*`'ın desendeki yeri ve o an metinde nereye kadar yuttuğu.
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == '?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((at, swallowed)) => {
                    p = at + 1;
                    t = swallowed + 1;
                    star = Some((at, swallowed + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
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
    /// `[motion] smooth_scroll`: geçmişte kaydırmak pürüzsüz mü.
    pub smooth_scroll: SmoothScroll,
    /// `[motion] keypress`: dock'ta yazılan glyph'in efekti.
    pub keypress: Keypress,
    /// `[motion] erase`: dock'ta silinen glyph'in efekti.
    pub erase: Erase,
    /// `[shell] integration`: kabuk sarmalayıcısı kurulsun mu. **Sonraki
    /// oturumda** geçerli ([`ShellIntegration`]).
    pub shell_integration: ShellIntegration,
    /// `[terminal] confirm_close`: kapanışta ne zaman sorulsun
    /// ([`ConfirmClose`]). `TerminalOptions`'a girmiyor.
    pub confirm_close: ConfirmClose,
    /// `[remote] hosts`: uzak host'ların işaret desenleri, dosyadaki
    /// sırasıyla (037 Karar 2; eşleşme [`host_mark`]). Varsayılan boş.
    pub remote_hosts: Vec<HostRule>,
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
            smooth_scroll: SmoothScroll::default(),
            keypress: Keypress::default(),
            erase: Erase::default(),
            shell_integration: ShellIntegration::default(),
            confirm_close: ConfirmClose::default(),
            remote_hosts: Vec::new(),
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

/// Tek bir anahtarın yeni değeri — ayar penceresinin dosyaya yazdığı şey
/// ([`Settings::with_edit`]).
///
/// Tipli, dizge değil: bölüm, anahtar ve TOML türü varyanttan türüyor, yani
/// pencere yanlış bölüme ya da yanlış türde yazamaz. Değerin **aralığı**
/// sınanmıyor — pencerenin kontrolleri aralıkları buradan alıyor
/// ([`CURSOR_RADIUS_RANGE`] …); öyle olmasa da ayrıştırıcı okurken reddeder.
#[derive(Clone, Debug, PartialEq)]
pub enum SettingsEdit {
    Scrollback(usize),
    Cursor(CaretShape),
    CursorBlink(CursorBlink),
    CursorRadius(f64),
    CursorGlow(f64),
    CursorUnfocused(UnfocusedCaret),
    BlinkInterval(f64),
    ConfirmClose(ConfirmClose),
    Theme(String),
    LightTheme(String),
    DarkTheme(String),
    /// Boş dizge varsayılan aile (zincir): ayrıştırıcı `family = ""`'yi öyle
    /// okuyor ve anahtar silinmiyor.
    FontFamily(String),
    FontSize(f64),
    LineHeight(f64),
    Osc52(Osc52),
    CursorMotion(CursorMotion),
    ReduceMotion(ReduceMotion),
    SmoothScroll(SmoothScroll),
    Keypress(Keypress),
    Erase(Erase),
    ShellIntegration(ShellIntegration),
    /// Shell ▸ Mark … as ▸ (037 Karar 5): `host`'un işaretini `mark` yapan
    /// `[remote] hosts` düzenlemesi. Tek bir anahtarın değeri değil, dizinin
    /// girdileri — kuralı [`Settings::with_edit`]'in bu kolunda. `host` uzak
    /// oturumun gösterdiği hâli (`user@` dahil, eşleşmenin girdisi); yazılan
    /// desen onun `user@`'siz kısmı. [`HostMark::None`] menünün "None"u.
    RemoteHostMark {
        host: String,
        mark: HostMark,
    },
}

impl SettingsEdit {
    /// Düzenlenen anahtarın noktalı yolu (`terminal.cursor`) — ayrıştırıcının
    /// tanısında geçen [`Diagnostic::key`]'in aynısı; ayar penceresi satırını
    /// bununla buluyor.
    pub fn path(&self) -> &'static str {
        self.place().2
    }

    /// Bölüm, anahtar ve tanının taşıdığı noktalı yol (`Diagnostic::key`
    /// `'static` istiyor, o yüzden üçü de sabit).
    fn place(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Scrollback(_) => ("terminal", "scrollback", "terminal.scrollback"),
            Self::Cursor(_) => ("terminal", "cursor", "terminal.cursor"),
            Self::CursorBlink(_) => ("terminal", "cursor_blink", "terminal.cursor_blink"),
            Self::CursorRadius(_) => ("terminal", "cursor_radius", "terminal.cursor_radius"),
            Self::CursorGlow(_) => ("terminal", "cursor_glow", "terminal.cursor_glow"),
            Self::CursorUnfocused(_) => {
                ("terminal", "cursor_unfocused", "terminal.cursor_unfocused")
            }
            Self::BlinkInterval(_) => (
                "terminal",
                "cursor_blink_interval",
                "terminal.cursor_blink_interval",
            ),
            Self::ConfirmClose(_) => ("terminal", "confirm_close", "terminal.confirm_close"),
            Self::Theme(_) => ("appearance", "theme", "appearance.theme"),
            Self::LightTheme(_) => ("appearance", "light_theme", "appearance.light_theme"),
            Self::DarkTheme(_) => ("appearance", "dark_theme", "appearance.dark_theme"),
            Self::FontFamily(_) => ("font", "family", "font.family"),
            Self::FontSize(_) => ("font", "size", "font.size"),
            Self::LineHeight(_) => ("font", "line_height", "font.line_height"),
            Self::Osc52(_) => ("clipboard", "osc52", "clipboard.osc52"),
            Self::CursorMotion(_) => ("motion", "cursor_motion", "motion.cursor_motion"),
            Self::ReduceMotion(_) => ("motion", "reduce_motion", "motion.reduce_motion"),
            Self::SmoothScroll(_) => ("motion", "smooth_scroll", "motion.smooth_scroll"),
            Self::Keypress(_) => ("motion", "keypress", "motion.keypress"),
            Self::Erase(_) => ("motion", "erase", "motion.erase"),
            Self::ShellIntegration(_) => ("shell", "integration", "shell.integration"),
            Self::RemoteHostMark { .. } => ("remote", "hosts", "remote.hosts"),
        }
    }

    /// Dosyaya yazılacak değer. Ondalık iki basamağa yuvarlanıyor: pencerenin
    /// kaydırıcısı `0.30000000000000004` yazmasın, ve okunan değer yazılanın
    /// ta kendisi olsun.
    fn value(&self) -> toml_edit::Value {
        let decimal = |value: f64| toml_edit::Value::from((value * 100.0).round() / 100.0);
        match self {
            // `i64`'e sığmayan satır sayısı zaten tavanın çok ötesinde.
            Self::Scrollback(lines) => i64::try_from(*lines).unwrap_or(i64::MAX).into(),
            Self::Cursor(shape) => shape.name().into(),
            Self::CursorBlink(blink) => blink.name().into(),
            Self::CursorUnfocused(unfocused) => unfocused.name().into(),
            Self::ConfirmClose(confirm) => confirm.name().into(),
            Self::Osc52(mode) => mode.name().into(),
            Self::CursorMotion(motion) => motion.name().into(),
            Self::ReduceMotion(reduce) => reduce.name().into(),
            Self::SmoothScroll(smooth) => smooth.name().into(),
            Self::Keypress(keypress) => keypress.name().into(),
            Self::Erase(erase) => erase.name().into(),
            Self::ShellIntegration(integration) => integration.name().into(),
            // Dizinin kendisi değil, yazılan girdinin `mark`'ı.
            Self::RemoteHostMark { mark, .. } => mark.written().into(),
            Self::CursorRadius(value)
            | Self::CursorGlow(value)
            | Self::BlinkInterval(value)
            | Self::FontSize(value)
            | Self::LineHeight(value) => decimal(*value),
            Self::Theme(name)
            | Self::LightTheme(name)
            | Self::DarkTheme(name)
            | Self::FontFamily(name) => name.as_str().into(),
        }
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
    pub const TEMPLATE: &str = r##"# bateri settings. Changes apply as soon as you save this file.
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
# "never" | "running" | "always". When closing a tab or window, or quitting,
# asks first: running asks only while a program other than the shell is in
# the foreground (vim, ssh, a build) and names it, always asks even at an idle
# prompt, never closes without asking. Typing exit never asks, and neither do
# programs left running in the background.
confirm_close = "running"

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
# "on" | "off". How scrolling back through history moves: on follows your
# fingers on a trackpad pixel by pixel, lets a flick coast to a stop, glides a
# mouse wheel notch and settles on a whole line when you let go; off moves
# line by line. Reduce Motion and cursor_motion = "snap" also move line by
# line.
smooth_scroll = "on"
# "off" | "fade" | "rise" | "pop" | "extrude" | "heat" | "echo" | "drop" |
# "ink" | "squeeze". How a letter you type in the dock at the bottom of the
# window appears: fade brings it in from clear, rise slides it up into place,
# pop springs it out from small, extrude stretches it out from its left edge,
# heat starts it in the cursor color and cools it to its own, echo sends a
# faint copy of it rippling outward, drop lets it fall into place with a small
# bounce, ink fills it from the middle of its strokes outward, squeeze starts
# it narrow and tall and lets it spring into shape. off shows it at once.
keypress = "fade"
# "off" | "iris" | "undertow" | "echo" | "bleed" | "unravel" | "recede" |
# "sublime" | "shatter". How a letter you delete in the dock goes: iris closes
# a round shutter over it, undertow pulls it down toward the cursor, echo
# swells it outward like a ripple, bleed lets its ink spread thin, unravel
# slides it apart in strips, recede shrinks it away, sublime lets it drift up
# like vapor, shatter breaks it into falling pieces. off removes it at once.
# Pasting, history and deleting a whole word or line are instant. cursor_motion = "snap" turns both off; Reduce Motion
# keeps only a fade for typing.
erase = "recede"

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

[remote]
# Colors the dock of an ssh or mosh session by the host it is on, so a
# production machine is never mistaken for another. Each entry names a host
# pattern and a mark: "production" (red), "staging" (yellow), "development"
# (green), "none" (no mark), or a color like "#c678dd". In a pattern * stands
# for any run of characters and ? for one, ignoring case; a pattern without @
# matches the host after any user@. The first entry that matches wins, so put
# exact names before wide patterns; "none" stops the search. Shell > Mark
# "host" as writes the entry for the host of the ssh tab you are in.
# hosts = [
#   { host = "prod-*", mark = "production" },
#   { host = "*.staging.example.com", mark = "staging" },
# ]
hosts = []
"##;

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
                    parsed.settings.cursor = named_enum(
                        text,
                        item,
                        "terminal.cursor",
                        CaretShape::NAMES,
                        fallback.cursor,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = terminal.get("cursor_blink") {
                    parsed.settings.cursor_blink = named_enum(
                        text,
                        item,
                        "terminal.cursor_blink",
                        CursorBlink::NAMES,
                        fallback.cursor_blink,
                        &mut parsed.diagnostics,
                    );
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
                if let Some(item) = terminal.get("confirm_close") {
                    parsed.settings.confirm_close = named_enum(
                        text,
                        item,
                        "terminal.confirm_close",
                        ConfirmClose::NAMES,
                        fallback.confirm_close,
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
                parsed.settings.confirm_close = fallback.confirm_close;
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
                    parsed.settings.osc52 = named_enum(
                        text,
                        item,
                        "clipboard.osc52",
                        Osc52::NAMES,
                        Osc52::Off,
                        &mut parsed.diagnostics,
                    );
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
                    parsed.settings.cursor_motion = named_enum(
                        text,
                        item,
                        "motion.cursor_motion",
                        CursorMotion::NAMES,
                        fallback.cursor_motion,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = motion.get("reduce_motion") {
                    parsed.settings.reduce_motion = named_enum(
                        text,
                        item,
                        "motion.reduce_motion",
                        ReduceMotion::NAMES,
                        fallback.reduce_motion,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = motion.get("smooth_scroll") {
                    parsed.settings.smooth_scroll = named_enum(
                        text,
                        item,
                        "motion.smooth_scroll",
                        SmoothScroll::NAMES,
                        fallback.smooth_scroll,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = motion.get("keypress") {
                    parsed.settings.keypress = named_enum(
                        text,
                        item,
                        "motion.keypress",
                        Keypress::NAMES,
                        fallback.keypress,
                        &mut parsed.diagnostics,
                    );
                }
                if let Some(item) = motion.get("erase") {
                    parsed.settings.erase = named_enum(
                        text,
                        item,
                        "motion.erase",
                        Erase::NAMES,
                        fallback.erase,
                        &mut parsed.diagnostics,
                    );
                }
            }
            None if root.contains_key("motion") => {
                parsed.settings.cursor_motion = fallback.cursor_motion;
                parsed.settings.reduce_motion = fallback.reduce_motion;
                parsed.settings.smooth_scroll = fallback.smooth_scroll;
                parsed.settings.keypress = fallback.keypress;
                parsed.settings.erase = fallback.erase;
            }
            None => {}
        }
        match section(text, root, "shell", &mut parsed.diagnostics) {
            Some(shell) => {
                if let Some(item) = shell.get("integration") {
                    parsed.settings.shell_integration = named_enum(
                        text,
                        item,
                        "shell.integration",
                        ShellIntegration::NAMES,
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
        match section(text, root, "remote", &mut parsed.diagnostics) {
            Some(remote) => {
                if let Some(item) = remote.get("hosts") {
                    parsed.settings.remote_hosts =
                        host_rules(text, item, &fallback.remote_hosts, &mut parsed.diagnostics);
                }
            }
            None if root.contains_key("remote") => {
                parsed
                    .settings
                    .remote_hosts
                    .clone_from(&fallback.remote_hosts);
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
                || self.reduce_motion != new.reduce_motion
                || self.smooth_scroll != new.smooth_scroll
                || self.keypress != new.keypress
                || self.erase != new.erase,
            caret: self.caret != new.caret || self.blink_interval != new.blink_interval,
            remote: self.remote_hosts != new.remote_hosts,
        }
    }

    /// Menünün tema seçimi (View ▸ Theme ▸): `[appearance] theme`'i `name`
    /// yapar — [`Settings::with_edit`]'in tema hâli.
    ///
    /// `light_theme` ve `dark_theme`'e dokunmaz: sabit bir tema seçen
    /// kullanıcı `"system"`'e dönünce çiftini geri bulur. Adın biçimi
    /// sınanmıyor: menü yalnız gömülü temaların ve `themes/`'teki dosyaların
    /// adlarını veriyor; öyle olmasa da ayrıştırıcı adı okurken reddeder.
    pub fn with_theme(text: &str, name: &str) -> Result<String, Diagnostic> {
        Self::with_edit(text, &SettingsEdit::Theme(name.to_owned()))
    }

    /// `settings.toml`'un metninde tek bir anahtarı `edit`'in değeri yapar ve
    /// **geri kalan her baytı** yerinde bırakır — yorumlar, boş satırlar,
    /// anahtar sırası, tanımadığımız anahtarlar, değerin yanındaki yorum.
    /// Dosyayı okuyup yazan `bt-shell`; yazanlar menü ve ayar penceresi.
    ///
    /// - Bölüm yoksa sona, anahtar yoksa bölümün içine eklenir; bölümün
    ///   yazılışı (başlık, satır içi tablo, noktalı anahtar) korunur.
    /// - **Ayrıştırılamayan metin `Err`**, yeni metin üretilmez: dosya
    ///   kullanıcının yarım işi ve üstüne yazmak onu silerdi. Aynı sebeple
    ///   bölüm olmayan bir bölüm (`appearance = 1`, `[[appearance]]`) ve bölüm
    ///   olan bir anahtar (`[appearance.theme]`, `theme = { … }`) de `Err`:
    ///   yerlerine yazmak içeriklerini silerdi. Kabul edilmeyen türdeki bir
    ///   değer (`theme = 3`) ise değişir — kullanıcı bir değer seçti.
    ///
    /// [`SettingsEdit::RemoteHostMark`] tek bir değer değil dizinin
    /// girdilerini yazıyor; kuralı [`with_host_mark`]'ta, aynı sözleşmeyle
    /// (ayrıştırılamayan metin ve bozuk dizi `Err`, geri kalan her bayt
    /// yerinde).
    pub fn with_edit(text: &str, edit: &SettingsEdit) -> Result<String, Diagnostic> {
        if let SettingsEdit::RemoteHostMark { host, mark } = edit {
            return with_host_mark(text, host, *mark);
        }
        let (section_name, key, path) = edit.place();
        let value = edit.value();
        let parsed = document(text)?;
        // Ret konumlu belgede: `into_mut` konumları düşürüyor, tanının satırı
        // onlardan geliyor.
        let mut refused = Vec::new();
        // Satır içi tablo (`theme = { … }`) da bir bölüm: `is_value` onu
        // geçirirdi ve yerine yazmak `[appearance.theme]`'in reddedildiği
        // içeriği bu yazılışta sessizce silerdi (`/code-review` bulgusu).
        if let Some(table) = section(text, parsed.as_table(), section_name, &mut refused)
            && let Some(item) = table
                .get(key)
                .filter(|item| !item.is_value() || item.is_inline_table())
        {
            let expected = match value {
                toml_edit::Value::String(_) => "a string",
                toml_edit::Value::Integer(_) => "an integer",
                _ => "a number",
            };
            refused.push(Diagnostic {
                key: Some(path),
                line: item.span().and_then(|span| line_of(text, span.start)),
                message: format!("`{path}` must be {expected}, found {}", kind(item)),
            });
        }
        if let Some(diagnostic) = refused.pop() {
            return Err(diagnostic);
        }
        let mut doc = parsed.into_mut();
        ensure_section(&mut doc, section_name);
        // `else` dalı yok: bölüm olmayan bir bölüm yukarıda reddedildi, eksik
        // olan da az önce tablo olarak eklendi.
        if let Some(table) = doc.get_mut(section_name).and_then(Item::as_table_like_mut) {
            match table.get_mut(key).and_then(Item::as_value_mut) {
                // Süs (`=`'den sonraki boşluk, satır sonundaki yorum) değerin
                // üstünde duruyor; yeni değer onu devralmazsa yorum düşerdi.
                Some(old) => {
                    let decor = old.decor().clone();
                    *old = value;
                    *old.decor_mut() = decor;
                }
                None => {
                    table.insert(key, Item::Value(value));
                }
            }
        }
        Ok(rendered(text, &doc))
    }
}

/// Bölüm yoksa belgeye boş bir `[name]` ekler; varsa dokunmaz.
///
/// Belge sonundaki yorum `toml_edit`'te belgenin kuyruğu ve yeni bölüm onun
/// önüne yazılırdı: son bölümün altındaki `# family = "Menlo"`
/// `[appearance]`'a geçer, yorumu kaldıran kullanıcının satırı sessizce
/// yoksayılırdı. Kuyruk yeni başlığın önüne alınıyor, yani yazıldığı bölümde
/// kalıyor.
fn ensure_section(doc: &mut toml_edit::DocumentMut, name: &str) {
    if doc.contains_key(name) {
        return;
    }
    let mut table = toml_edit::Table::new();
    let trailing = doc.trailing().as_str().unwrap_or_default().to_owned();
    if !trailing.trim().is_empty() {
        table.decor_mut().set_prefix(format!("{trailing}\n"));
        doc.set_trailing("");
    }
    doc.insert(name, Item::Table(table));
}

/// Düzenlenmiş belgenin metni, `text`'in satır sonlarıyla.
///
/// `toml_edit` satır sonlarını LF yazıyor. İlk satırı CRLF olan dosya CRLF
/// kalıyor; yoksa tek bir seçim dotfile deposunda bütün dosyayı değişmiş
/// gösterirdi. Karışık satır sonlu dosya ilk satırınkini alır.
///
/// Önce LF'ye indirilip sonra çevriliyor (`/code-review` bulgusu):
/// `toml_edit` çok satırlı metnin **içindeki** `\r\n`'i olduğu gibi
/// bırakıyor ve doğrudan çeviri onu `\r\r\n` yapardı — geçersiz TOML, yani
/// dosyaya bir daha yazılamaz ve hiçbir kayıt uygulanmazdı.
fn rendered(text: &str, doc: &toml_edit::DocumentMut) -> String {
    let crlf = text
        .find('\n')
        .is_some_and(|end| text.as_bytes()[..end].ends_with(b"\r"));
    let written = doc.to_string();
    if crlf {
        written.replace("\r\n", "\n").replace('\n', "\r\n")
    } else {
        written
    }
}

/// [`with_host_mark`]'ın dizide yapacakları ([`host_mark_plan`]).
#[derive(Debug, PartialEq, Eq)]
struct MarkPlan {
    /// Bu indeksteki girdinin `mark`'ı yerinde değişiyor.
    in_place: Option<usize>,
    /// Silinen girdiler, artan sırayla.
    remove: Vec<usize>,
    /// Dizinin başına `{ host = <user@'siz host>, mark }` giriyor.
    prepend: bool,
}

/// Menünün yazım kuralı (037 Karar 5), saf: `rules`'u `host`'un çözümü
/// `mark` olacak biçimde en az bozan düzenleme; çözüm zaten `mark`'sa `None`
/// (no-op).
///
/// - Tam bu host'u yazan (desen, harf duyarsız, `user@`'siz ya da tam
///   host'a eşit) ilk girdinin işareti **yerinde** değişiyor — kullanıcının
///   koyduğu sıra bozulmuyor.
/// - Yerinde değişim sonucu vermiyorsa (girdi yok ya da önünde başka bir
///   işaret veren bir glob var) tam girdiler siliniyor ve yenisi **başa**
///   yazılıyor: kullanıcı "bu makine prod" dedi, o cümle bir globun arkasında
///   kalıp etkisiz görünmemeli. Karar "yoksa başa" diyor; önünde glob olan
///   tam girdi aynı gerekçeyle başa taşınıyor.
/// - **None** tam girdileri siliyor; ardından bir glob hâlâ işaret
///   veriyorsa başa `mark = "none"` yazılıyor.
fn host_mark_plan(rules: &[HostRule], host: &str, mark: HostMark) -> Option<MarkPlan> {
    if host_mark(rules, host) == mark {
        return None;
    }
    let bare = bare_host(host).to_lowercase();
    let full = host.to_lowercase();
    let exact: Vec<usize> = rules
        .iter()
        .enumerate()
        .filter(|(_, rule)| {
            let pattern = rule.pattern.to_lowercase();
            pattern == bare || pattern == full
        })
        .map(|(index, _)| index)
        .collect();
    if mark != HostMark::None
        && let Some(&first) = exact.first()
    {
        let mut edited = rules.to_vec();
        edited[first].mark = mark;
        if host_mark(&edited, host) == mark {
            return Some(MarkPlan {
                in_place: Some(first),
                remove: Vec::new(),
                prepend: false,
            });
        }
    }
    let kept: Vec<HostRule> = rules
        .iter()
        .enumerate()
        .filter(|(index, _)| !exact.contains(index))
        .map(|(_, rule)| rule.clone())
        .collect();
    Some(MarkPlan {
        in_place: None,
        prepend: host_mark(&kept, host) != mark,
        remove: exact,
    })
}

/// Host'un `user@`'siz kısmı: desende `@` yoksa eşleşmenin girdisi
/// ([`host_mark`]) ve menünün başlığındaki ad.
pub fn bare_host(host: &str) -> &str {
    host.rsplit('@').next().unwrap_or(host)
}

/// [`SettingsEdit::RemoteHostMark`]'ın yazımı: [`host_mark_plan`]'ı
/// `[remote] hosts`'a uygular. İki yazılış da (satır içi dizi ve
/// `[[remote.hosts]]`) kendi biçiminde kalıyor; bölüm ya da anahtar yoksa
/// satır içi dizi olarak doğuyor. Ayrıştırılamayan metin ve bozuk dizi
/// `Err` — bozuk bir girdiyi yerinde bırakıp önüne yazmak listenin anlamını
/// tahmin etmek olurdu.
fn with_host_mark(text: &str, host: &str, mark: HostMark) -> Result<String, Diagnostic> {
    let parsed = document(text)?;
    let mut refused = Vec::new();
    let rules = match section(text, parsed.as_table(), "remote", &mut refused)
        .and_then(|remote| remote.get("hosts"))
    {
        Some(item) => host_rules(text, item, &[], &mut refused),
        None => Vec::new(),
    };
    if let Some(diagnostic) = refused.pop() {
        return Err(diagnostic);
    }
    let Some(plan) = host_mark_plan(&rules, host, mark) else {
        return Ok(text.to_owned());
    };
    let pattern = bare_host(host);
    let written = mark.written();
    let mut doc = parsed.into_mut();
    ensure_section(&mut doc, "remote");
    // `else` dalı yok: bölüm olmayan bir bölüm yukarıda reddedildi, eksik
    // olan az önce eklendi.
    if let Some(remote) = doc.get_mut("remote").and_then(Item::as_table_like_mut) {
        match remote.get_mut("hosts") {
            Some(Item::ArrayOfTables(tables)) => {
                if let Some(index) = plan.in_place
                    && let Some(table) = tables.get_mut(index)
                {
                    set_keeping_decor(table.get_mut("mark"), &written);
                }
                for &index in plan.remove.iter().rev() {
                    tables.remove(index);
                }
                if plan.prepend {
                    let mut table = toml_edit::Table::new();
                    table.insert("host", toml_edit::value(pattern));
                    table.insert("mark", toml_edit::value(written.as_str()));
                    // Eski ilk bölümün yeri ve üstündeki yorum yeniye geçiyor
                    // (yazılış sırası konumdan; eşit konumda dizinin sırası),
                    // eskisi bir boş satırla ayrılıyor.
                    if let Some(first) = tables.get_mut(0) {
                        table.set_position(first.position());
                        *table.decor_mut() = first.decor().clone();
                        first.decor_mut().set_prefix("\n");
                    }
                    tables.insert(0, table);
                }
            }
            Some(Item::Value(toml_edit::Value::Array(array))) => {
                if let Some(index) = plan.in_place
                    && let Some(entry) = array
                        .get_mut(index)
                        .and_then(toml_edit::Value::as_inline_table_mut)
                    && let Some(old) = entry.get_mut("mark")
                {
                    let decor = old.decor().clone();
                    *old = written.as_str().into();
                    *old.decor_mut() = decor;
                }
                for &index in plan.remove.iter().rev() {
                    array.remove(index);
                }
                if plan.prepend {
                    prepend_entry(array, pattern, &written);
                }
            }
            // Anahtar yok (dizi boşmuş gibi): yalnız başa yazma olabilir.
            _ => {
                let mut array = toml_edit::Array::new();
                if plan.prepend {
                    prepend_entry(&mut array, pattern, &written);
                }
                remote.insert("hosts", Item::Value(array.into()));
            }
        }
    }
    Ok(rendered(text, &doc))
}

/// `item` bir değerse onu `written` yapar, süsünü (yanındaki yorum)
/// koruyarak — `[[remote.hosts]]` girdisinin `mark = "…"` satırı.
fn set_keeping_decor(item: Option<&mut Item>, written: &str) {
    if let Some(old) = item.and_then(Item::as_value_mut) {
        let decor = old.decor().clone();
        *old = written.into();
        *old.decor_mut() = decor;
    }
}

/// Satır içi dizinin başına `{ host, mark }` yazar ve dizinin yazılışını
/// sürdürür: yeni girdi eski ilk girdinin süsünü (çok satırlı dizide
/// `\n  ` girintisi) alıyor; eski ilk girdi tek satırlı dizide virgülden
/// sonra bir boşluk kazanıyor, yoksa `{…},{…}` yapışırdı.
fn prepend_entry(array: &mut toml_edit::Array, pattern: &str, written: &str) {
    let mut entry = toml_edit::InlineTable::new();
    entry.insert("host", pattern.into());
    entry.insert("mark", written.into());
    entry.fmt();
    let mut value = toml_edit::Value::InlineTable(entry);
    if let Some(first) = array.get_mut(0) {
        *value.decor_mut() = first.decor().clone();
        let multiline = first
            .decor()
            .prefix()
            .and_then(|prefix| prefix.as_str())
            .is_some_and(|prefix| prefix.contains('\n'));
        if !multiline {
            first.decor_mut().set_prefix(" ");
        }
    }
    array.insert_formatted(0, value);
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
    /// Üç anahtar **tek** alanda: üçü de aynı çağrı yerinde çözülüyor ve ayrı
    /// alanlar çağıranda tek bir `if` yerine üç tane yazdırırdı.
    /// [`Settings::reduce_motion`] üç değerli olduğu için `bt-shell` onu yine
    /// de çözmek zorunda, [`Settings::smooth_scroll`] de öteki ikisiyle tek
    /// `bool`'a iniyor (view'ın tekerleğine); fark yalnız "bir şey değişti"
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
    /// [`Settings::remote_hosts`] değişti: desen listesi her oturuma gider
    /// (`Session::set_host_marks`) ve etkin uzak host'un işareti yeniden
    /// çözülür (037 Karar 2).
    pub remote: bool,
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
        LINE_HEIGHT_RANGE,
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

/// Satır yüksekliği çarpanının kabul aralığı: alt ucu fontun kendi metriği
/// ([`FontOptions::line_height`]), üst ucu [`MAX_LINE_HEIGHT`].
pub const LINE_HEIGHT_RANGE: std::ops::RangeInclusive<f64> = 1.0..=MAX_LINE_HEIGHT;

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

/// Adlandırılmış seçenek anahtarının **tek gövdesi**: listedeki adlardan
/// biri değilse anahtar `fallback`'te kalır ve tanı bırakılır. Her dizge
/// enum'u (`clipboard.osc52` dahil) buradan okunuyor, tanının "must be …"
/// listesi ve "using …" değeri de tipin `NAMES` tablosundan — yazılış tek
/// yerde, yani tanı ayrıştırıcının reddettiği bir değeri öneremez.
///
/// Kabul edilmeyen değerin `fallback`'e düşmesi ([`Settings::parse_keeping`])
/// bu anahtarların hepsinde doğru, çünkü yanlış tahminin belirtisi görünür:
/// imlecin şekli, kayması, kaydırmanın adımı. Tek istisna `osc52` ve çağıran
/// onu `Osc52::Off` vererek kuruyor — orada yanlış tahmin sessiz.
///
/// Büyük/küçük harf **duyarlı**: `"Hollow"` bir yazım hatası ve sessizce
/// kabul edilmesi kullanıcıyı yanıltırdı.
fn named_enum<T: Copy + PartialEq>(
    text: &str,
    item: &Item,
    key: &'static str,
    names: &'static [(&'static str, T)],
    fallback: T,
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
        message: format!(
            "`{key}` must be {expected}, found {found}; using \"{}\"",
            name_in(names, fallback)
        ),
    });
    fallback
}

/// `remote.hosts`: `{ host, mark }` girdilerinin dizisi (037 Karar 2).
///
/// **Bozuk tek bir girdi anahtarın tamamını reddediyor** ve `fallback`'in
/// listesi kalıyor — `parse_keeping`'in kuralı, istisnasız: listeden yalnız
/// bozuk girdiyi atmak sırayı değiştirir ve bir globun arkasındaki tam adı
/// öne çıkarıp işareti sessizce değiştirebilirdi. Tanı ilk bozuk girdinin
/// satırını söylüyor.
///
/// Satır içi dizi de (`hosts = [{ … }]`) bölüm dizisi de (`[[remote.hosts]]`)
/// kabul: ikisi de aynı listeyi yazıyor ve TOML'u elle yazan kullanıcı
/// ikincisini de seçebilir.
fn host_rules(
    text: &str,
    item: &Item,
    fallback: &[HostRule],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<HostRule> {
    const KEY: &str = "remote.hosts";
    let reject = |span: Option<std::ops::Range<usize>>, found: String| Diagnostic {
        key: Some(KEY),
        line: span.and_then(|span| line_of(text, span.start)),
        message: format!(
            "`{KEY}` must be a list of {{ host = \"pattern\", mark = \"production\", \
             \"staging\", \"development\", \"none\" or \"#rrggbb\" }}, found {found}; \
             keeping the previous list"
        ),
    };
    let mut entries: Vec<(&dyn TableLike, Option<std::ops::Range<usize>>)> = Vec::new();
    if let Some(array) = item.as_array() {
        for value in array {
            let Some(table) = value.as_inline_table() else {
                let found = "an entry that is not a { … } table".to_owned();
                diagnostics.push(reject(value.span(), found));
                return fallback.to_vec();
            };
            entries.push((table, value.span()));
        }
    } else if let Some(tables) = item.as_array_of_tables() {
        entries.extend(
            tables
                .iter()
                .map(|table| (table as &dyn TableLike, table.span())),
        );
    } else {
        diagnostics.push(reject(item.span(), kind(item).to_owned()));
        return fallback.to_vec();
    }
    let mut rules = Vec::with_capacity(entries.len());
    for (table, span) in entries {
        let pattern = table
            .get("host")
            .and_then(Item::as_str)
            .filter(|pattern| !pattern.is_empty());
        let mark = table.get("mark").and_then(Item::as_str).and_then(|mark| {
            HostMark::NAMES
                .iter()
                .find(|(name, _)| *name == mark)
                .map(|(_, named)| *named)
                .or_else(|| crate::theme::hex_color(mark).map(HostMark::Rgb))
        });
        match (pattern, mark) {
            (Some(pattern), Some(mark)) => rules.push(HostRule {
                pattern: pattern.to_owned(),
                mark,
            }),
            (None, _) => {
                diagnostics.push(reject(span, "an entry without a host".to_owned()));
                return fallback.to_vec();
            }
            (Some(pattern), None) => {
                let found = match table.get("mark").and_then(Item::as_str) {
                    Some(mark) => format!("mark {mark:?} for {pattern:?}"),
                    None => format!("no mark for {pattern:?}"),
                };
                diagnostics.push(reject(span, found));
                return fallback.to_vec();
            }
        }
    }
    rules
}

/// Değerin yazılışı `names` tablosunda. Tablolar her varyantı taşıyor
/// (`every_name_reads_back_as_its_value` bekçi), boş dize yalnız eksik bir
/// tablonun belirtisi olurdu.
pub(crate) fn name_in<T: PartialEq>(names: &'static [(&'static str, T)], value: T) -> &'static str {
    names
        .iter()
        .find(|(_, named)| *named == value)
        .map_or("", |(name, _)| *name)
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
            ("terminal", "confirm_close"),
            ("appearance", "theme"),
            ("appearance", "light_theme"),
            ("appearance", "dark_theme"),
            ("font", "size"),
            ("font", "line_height"),
            ("clipboard", "osc52"),
            ("motion", "cursor_motion"),
            ("motion", "reduce_motion"),
            ("motion", "smooth_scroll"),
            ("motion", "keypress"),
            ("motion", "erase"),
            ("shell", "integration"),
            ("remote", "hosts"),
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
                caret: false,
                remote: false,
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
            smooth_scroll: SmoothScroll::On,
            keypress: Keypress::Fade,
            erase: Erase::Recede,
            shell_integration: ShellIntegration::Auto,
            confirm_close: ConfirmClose::Always,
            remote_hosts: Vec::new(),
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
            remote: false,
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
                caret: false,
                remote: false,
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
                caret: false,
                remote: false,
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
                caret: false,
                remote: false,
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
    fn smooth_scroll_is_read() {
        // Dosyada yoksa `on`: özellik kapalı sevk edilmiyor (027 Karar 4).
        assert_eq!(clean("").smooth_scroll, SmoothScroll::On);
        assert_eq!(
            clean("[motion]\nsmooth_scroll = \"off\"\n").smooth_scroll,
            SmoothScroll::Off
        );
        assert_eq!(
            clean("motion = { smooth_scroll = \"on\" }\n").smooth_scroll,
            SmoothScroll::On
        );
        // Üç anahtar birbirini ezmiyor.
        let all = clean(
            "[motion]\ncursor_motion = \"ease\"\nreduce_motion = \"on\"\nsmooth_scroll = \"off\"\n",
        );
        assert_eq!(all.cursor_motion, CursorMotion::Ease);
        assert_eq!(all.reduce_motion, ReduceMotion::On);
        assert_eq!(all.smooth_scroll, SmoothScroll::Off);
    }

    #[test]
    fn keypress_and_erase_are_read() {
        // Dosyada yoksa `fade` / `recede`: animasyon kutudan çıkınca
        // görünmeli (030 Karar 7).
        let empty = clean("");
        assert_eq!(
            (empty.keypress, empty.erase),
            (Keypress::Fade, Erase::Recede)
        );
        for &(name, keypress) in Keypress::NAMES {
            let settings = clean(&format!("[motion]\nkeypress = \"{name}\"\n"));
            assert_eq!(settings.keypress, keypress, "{name}");
            assert_eq!(keypress.name(), name);
        }
        for &(name, erase) in Erase::NAMES {
            let settings = clean(&format!("motion = {{ erase = \"{name}\" }}\n"));
            assert_eq!(settings.erase, erase, "{name}");
            assert_eq!(erase.name(), name);
        }
        // Komşular birbirini ezmiyor.
        let all = clean(
            "[motion]\ncursor_motion = \"ease\"\nkeypress = \"off\"\nerase = \"off\"\n\
             smooth_scroll = \"off\"\n",
        );
        assert_eq!(all.cursor_motion, CursorMotion::Ease);
        assert_eq!(all.smooth_scroll, SmoothScroll::Off);
        assert_eq!((all.keypress, all.erase), (Keypress::Off, Erase::Off));
    }

    #[test]
    fn unrecognized_keypress_and_erase_keep_their_own_keys() {
        // `bounce` hiçbir listede yok: tanınmıyor, yani seçmek hiçbir şey
        // yapmayan bir ad kabul edilmiyor (030 Karar 7).
        for (value, found) in [
            ("\"bounce\"", "\"bounce\""),
            ("\"Fade\"", "\"Fade\""),
            ("1", "an integer"),
        ] {
            let text = format!("[motion]\ncursor_motion = \"snap\"\nkeypress = {value}\n");
            let (settings, diagnostic) = rejected(&text);
            assert_eq!(
                settings,
                Settings {
                    cursor_motion: CursorMotion::Snap,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("motion.keypress"), "{value}");
            assert_eq!(diagnostic.line, Some(3), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`motion.keypress` must be \"off\", \"fade\", \"rise\", \"pop\", \
                     \"extrude\", \"heat\", \"echo\", \"drop\", \"ink\" or \"squeeze\", \
                     found {found}; using \"fade\""
                )
            );
        }
        let (settings, diagnostic) =
            rejected("[motion]\nkeypress = \"off\"\nerase = \"dissolve\"\n");
        assert_eq!(settings.keypress, Keypress::Off);
        assert_eq!(settings.erase, Erase::Recede);
        assert_eq!(diagnostic.key, Some("motion.erase"));
        assert_eq!(
            diagnostic.message,
            "`motion.erase` must be \"off\", \"iris\", \"undertow\", \"echo\", \"bleed\", \
             \"unravel\", \"recede\", \"sublime\" or \"shatter\", found \"dissolve\"; \
             using \"recede\""
        );
        // Kayıt anında yerine geçen değer varsayılan değil **geçerli** ayar.
        let current = Settings {
            keypress: Keypress::Off,
            erase: Erase::Off,
            ..Settings::default()
        };
        let parsed =
            Settings::parse_keeping("[motion]\nkeypress = \"x\"\nerase = \"y\"\n", &current)
                .expect("ayrıştırılabilir metin");
        assert_eq!(
            (parsed.settings.keypress, parsed.settings.erase),
            (Keypress::Off, Erase::Off)
        );
        assert_eq!(parsed.diagnostics.len(), 2);
        let parsed =
            Settings::parse_keeping("motion = 5\n", &current).expect("ayrıştırılabilir metin");
        assert_eq!(
            (parsed.settings.keypress, parsed.settings.erase),
            (Keypress::Off, Erase::Off)
        );
    }

    #[test]
    fn keypress_and_erase_changes_are_motion_changes() {
        // İkisi de link'e gidiyor (`bt_gpu::DisplayLink::set_glyph_fx`), yani
        // `motion` kolunda; oturumu ve fontu kıpırdatmamalı.
        let before = clean("");
        for text in [
            "[motion]\nkeypress = \"off\"\n",
            "[motion]\nerase = \"off\"\n",
        ] {
            assert_eq!(
                before.changes(&clean(text)),
                Changes {
                    terminal: false,
                    font: false,
                    motion: true,
                    caret: false,
                    remote: false,
                },
                "{text}"
            );
        }
    }

    #[test]
    fn unrecognized_smooth_scroll_keeps_its_own_key() {
        // `cursor_motion` ile aynı kural: yalnız kendi anahtarı etkilenir,
        // yanında tanı.
        for (value, found) in [
            ("\"yes\"", "\"yes\""),
            ("\"On\"", "\"On\""),
            ("true", "a boolean"),
        ] {
            let text = format!("[motion]\ncursor_motion = \"snap\"\nsmooth_scroll = {value}\n");
            let (settings, diagnostic) = rejected(&text);
            assert_eq!(
                settings,
                Settings {
                    cursor_motion: CursorMotion::Snap,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("motion.smooth_scroll"), "{value}");
            assert_eq!(diagnostic.line, Some(3), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`motion.smooth_scroll` must be \"on\" or \"off\", found {found}; using \"on\""
                )
            );
        }
        // Kayıt anında yerine geçen değer varsayılan değil **geçerli** ayar.
        let current = Settings {
            smooth_scroll: SmoothScroll::Off,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[motion]\nsmooth_scroll = \"yes\"\n", &current)
            .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.smooth_scroll, SmoothScroll::Off);
        assert!(parsed.diagnostics[0].message.ends_with("using \"off\""));
        // Bölüm yanlış türde: anahtar kabul edilmemiş sayılıyor.
        let parsed =
            Settings::parse_keeping("motion = 5\n", &current).expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.smooth_scroll, SmoothScroll::Off);
    }

    #[test]
    fn smooth_scroll_change_is_a_motion_change() {
        // `bt-shell` onu Hareketi Azalt'ın yolunda çözüyor, yani fark
        // `motion`'da; oturumu ve fontu kıpırdatmamalı.
        let before = clean("");
        let after = clean("[motion]\nsmooth_scroll = \"off\"\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                terminal: false,
                font: false,
                motion: true,
                caret: false,
                remote: false,
            }
        );
        assert_eq!(after.changes(&after), Changes::default());
    }

    #[test]
    fn theme_write_keeps_smooth_scroll_and_unknown_keys() {
        // Menünün yazma yolu yeni anahtarı ve tanımadığı komşusunu yerinde
        // bırakıyor; yazılan metin aynı değeri geri okuyor.
        let text = "[motion]\nsmooth_scroll = \"off\" # satır satır\nglide = 3\n";
        let written = Settings::with_theme(text, "paper").expect("yazılabilir metin");
        assert!(written.starts_with(text), "{written}");
        let settings = clean(&written);
        assert_eq!(settings.smooth_scroll, SmoothScroll::Off);
        assert_eq!(settings.theme, "paper");
    }

    #[test]
    fn confirm_close_is_read() {
        // Dosyada yoksa `running`: soru yalnız koşan bir iş varken.
        assert_eq!(clean("").confirm_close, ConfirmClose::Running);
        for (value, expected) in [
            ("never", ConfirmClose::Never),
            ("running", ConfirmClose::Running),
            ("always", ConfirmClose::Always),
        ] {
            let text = format!("[terminal]\nconfirm_close = \"{value}\"\n");
            assert_eq!(clean(&text).confirm_close, expected, "{value}");
        }
        // Komşu anahtarlar birbirini ezmiyor.
        let both = clean("[terminal]\nscrollback = 42\nconfirm_close = \"never\"\n");
        assert_eq!(both.scrollback, 42);
        assert_eq!(both.confirm_close, ConfirmClose::Never);
    }

    #[test]
    fn unrecognized_confirm_close_keeps_its_own_key() {
        for (value, found) in [
            ("\"Always\"", "\"Always\""),
            ("\"ask\"", "\"ask\""),
            ("true", "a boolean"),
        ] {
            let text = format!("[terminal]\nscrollback = 42\nconfirm_close = {value}\n");
            let (settings, diagnostic) = rejected(&text);
            assert_eq!(
                settings,
                Settings {
                    scrollback: 42,
                    ..Settings::default()
                },
                "{value}"
            );
            assert_eq!(diagnostic.key, Some("terminal.confirm_close"), "{value}");
            assert_eq!(diagnostic.line, Some(3), "{value}");
            assert_eq!(
                diagnostic.message,
                format!(
                    "`terminal.confirm_close` must be \"never\", \"running\" or \"always\", \
                     found {found}; using \"running\""
                )
            );
        }
        // Kayıt anında yerine geçen değer geçerli ayar; bölüm yanlış türdeyse
        // de.
        let current = Settings {
            confirm_close: ConfirmClose::Always,
            ..Settings::default()
        };
        let parsed = Settings::parse_keeping("[terminal]\nconfirm_close = \"no\"\n", &current)
            .expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.confirm_close, ConfirmClose::Always);
        assert!(parsed.diagnostics[0].message.ends_with("using \"always\""));
        let parsed =
            Settings::parse_keeping("terminal = 5\n", &current).expect("ayrıştırılabilir metin");
        assert_eq!(parsed.settings.confirm_close, ConfirmClose::Always);
    }

    #[test]
    fn confirm_close_change_reaches_no_session() {
        // Kapanış anında güncel ayardan okunuyor: fark oturumlara, fonta ya da
        // imlece hiçbir şey göndermemeli (028 → Karar 6, emsal `caret`).
        let before = clean("");
        let after = clean("[terminal]\nconfirm_close = \"always\"\n");
        assert_eq!(before.terminal(), after.terminal());
        assert_eq!(before.changes(&after), Changes::default());
    }

    #[test]
    fn theme_write_keeps_confirm_close_and_unknown_keys() {
        let text = "[terminal]\nconfirm_close = \"never\" # sormadan\nask_twice = true\n";
        let written = Settings::with_theme(text, "paper").expect("yazılabilir metin");
        assert!(written.starts_with(text), "{written}");
        let settings = clean(&written);
        assert_eq!(settings.confirm_close, ConfirmClose::Never);
        assert_eq!(settings.theme, "paper");
    }

    fn host_rule(pattern: &str, mark: HostMark) -> HostRule {
        HostRule {
            pattern: pattern.to_owned(),
            mark,
        }
    }

    #[test]
    fn a_host_pattern_matches_with_star_and_question_ignoring_case() {
        // 037 Karar 2: `*` boş dahil herhangi bir dizi, `?` tek karakter.
        let prod = [host_rule("prod-*", HostMark::Production)];
        assert_eq!(host_mark(&prod, "prod-web-1"), HostMark::Production);
        assert_eq!(host_mark(&prod, "PROD-WEB-1"), HostMark::Production);
        assert_eq!(host_mark(&prod, "prod-"), HostMark::Production);
        assert_eq!(host_mark(&prod, "preprod-web"), HostMark::None);
        let one = [host_rule("prod-?", HostMark::Staging)];
        assert_eq!(host_mark(&one, "prod-1"), HostMark::Staging);
        assert_eq!(host_mark(&one, "prod-10"), HostMark::None);
        assert_eq!(host_mark(&one, "prod-"), HostMark::None);
        // Nokta sıradan bir karakter; `*` onu da yutuyor.
        let domain = [host_rule("*.staging.example.com", HostMark::Staging)];
        assert_eq!(
            host_mark(&domain, "a.b.staging.example.com"),
            HostMark::Staging
        );
        assert_eq!(host_mark(&domain, "staging.example.com"), HostMark::None);
        // Geri izleme: `*`'ın ilk denemesi yanlış yerde bitiyor.
        let tricky = [host_rule("*a*b?", HostMark::Development)];
        assert_eq!(host_mark(&tricky, "xaxbxbz"), HostMark::Development);
        assert_eq!(host_mark(&tricky, "xaxb"), HostMark::None);
    }

    #[test]
    fn a_pattern_without_at_matches_the_host_after_the_user() {
        // `deploy@prod` ile `prod` aynı makine; `@`'li desen kullanıcıyı da
        // soruyor.
        let bare = [host_rule("prod", HostMark::Production)];
        assert_eq!(host_mark(&bare, "deploy@prod"), HostMark::Production);
        assert_eq!(host_mark(&bare, "prod"), HostMark::Production);
        let root = [host_rule("root@*", HostMark::Staging)];
        assert_eq!(host_mark(&root, "root@db"), HostMark::Staging);
        assert_eq!(host_mark(&root, "deploy@db"), HostMark::None);
        assert_eq!(host_mark(&root, "db"), HostMark::None);
    }

    #[test]
    fn the_first_matching_host_rule_wins_and_none_stops_the_search() {
        let rules = [
            host_rule("prod-canary", HostMark::None),
            host_rule("prod-db", HostMark::Rgb(0xc678dd)),
            host_rule("prod-*", HostMark::Production),
            host_rule("*", HostMark::Development),
        ];
        assert_eq!(host_mark(&rules, "prod-canary"), HostMark::None);
        assert_eq!(host_mark(&rules, "prod-db"), HostMark::Rgb(0xc678dd));
        assert_eq!(host_mark(&rules, "prod-web"), HostMark::Production);
        assert_eq!(host_mark(&rules, "vm"), HostMark::Development);
        assert_eq!(host_mark(&[], "vm"), HostMark::None);
    }

    #[test]
    fn remote_hosts_are_read_in_order() {
        let settings = clean(
            "[remote]\nhosts = [\n  { host = \"prod-*\", mark = \"production\" },\n  \
             { host = \"stage\", mark = \"staging\" },\n  { host = \"dev\", mark = \"development\" },\n  \
             { host = \"vm\", mark = \"#C678dd\" },\n  { host = \"x\", mark = \"none\" },\n]\n",
        );
        assert_eq!(
            settings.remote_hosts,
            [
                host_rule("prod-*", HostMark::Production),
                host_rule("stage", HostMark::Staging),
                host_rule("dev", HostMark::Development),
                host_rule("vm", HostMark::Rgb(0xc678dd)),
                host_rule("x", HostMark::None),
            ]
        );
        // Bölüm dizisi aynı listeyi yazıyor.
        let tables = clean(
            "[[remote.hosts]]\nhost = \"prod-*\"\nmark = \"production\"\n\
             [[remote.hosts]]\nhost = \"vm\"\nmark = \"#c678dd\"\n",
        );
        assert_eq!(
            tables.remote_hosts,
            [
                host_rule("prod-*", HostMark::Production),
                host_rule("vm", HostMark::Rgb(0xc678dd)),
            ]
        );
        assert_eq!(clean("[remote]\nhosts = []\n").remote_hosts, []);
    }

    #[test]
    fn a_broken_host_entry_rejects_the_whole_list() {
        // Kayıt anının kuralı: bozuk tek girdi anahtarın tamamını reddediyor
        // ve verilen liste kalıyor; tanı ilk bozuk girdinin satırını söylüyor.
        let current = Settings {
            remote_hosts: vec![host_rule("old", HostMark::Staging)],
            ..Settings::default()
        };
        for (text, found) in [
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 { host = \"b\", mark = \"prod\" },\n]\n",
                "found mark \"prod\" for \"b\"",
            ),
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 { mark = \"staging\" },\n]\n",
                "found an entry without a host",
            ),
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 { host = \"b\" },\n]\n",
                "found no mark for \"b\"",
            ),
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 { host = \"b\", mark = \"#12345\" },\n]\n",
                "found mark \"#12345\" for \"b\"",
            ),
            (
                "[remote]\nhosts = [\n  { host = \"a\", mark = \"production\" },\n  \
                 \"b\",\n]\n",
                "found an entry that is not a { … } table",
            ),
            ("[remote]\n\nhosts = \"prod\"\n", "found a string"),
        ] {
            let parsed = Settings::parse_keeping(text, &current).expect("ayrıştırılabilir metin");
            assert_eq!(parsed.settings.remote_hosts, current.remote_hosts, "{text}");
            let [diagnostic] = <[Diagnostic; 1]>::try_from(parsed.diagnostics)
                .unwrap_or_else(|got| panic!("tek tanı beklendi: {got:?}"));
            assert_eq!(diagnostic.key, Some("remote.hosts"));
            assert!(diagnostic.message.contains(found), "{diagnostic}");
            assert!(diagnostic.message.ends_with("keeping the previous list"));
            // Bozuk girdi dördüncü satırda; `hosts`'un kendisi üçüncüde.
            let line = if found == "found a string" { 3 } else { 4 };
            assert_eq!(diagnostic.line, Some(line), "{text}");
        }
        // Açılışta aynı metin boş listeye düşüyor; yanlış türde bölüm de
        // verilen listeyi tutuyor.
        let (settings, _) = rejected("[remote]\nhosts = [{ host = \"b\", mark = \"x\" }]\n");
        assert_eq!(settings.remote_hosts, []);
        let parsed = Settings::parse_keeping("remote = 1\n", &current).expect("ayrıştırılabilir");
        assert_eq!(parsed.settings.remote_hosts, current.remote_hosts);
    }

    #[test]
    fn a_remote_hosts_change_is_its_own_field() {
        let before = clean("");
        let after = clean("[remote]\nhosts = [{ host = \"prod\", mark = \"production\" }]\n");
        assert_eq!(
            before.changes(&after),
            Changes {
                remote: true,
                ..Changes::default()
            }
        );
        assert_eq!(after.changes(&after), Changes::default());
    }

    #[test]
    fn theme_write_keeps_remote_hosts_comments_and_unknown_keys() {
        // R2.4: dosyaya yazan her yol `[remote]`'ı, yorumunu ve tanımadığımız
        // anahtarı yerinde bırakıyor.
        let text = "[remote]\n# prod kırmızı\nhosts = [\n  { host = \"prod\", mark = \"production\" }, # canlı\n]\nfuture = 1\n";
        let written = Settings::with_theme(text, "paper").expect("yazılabilir metin");
        assert!(written.starts_with(text), "{written}");
        let settings = clean(&written);
        assert_eq!(
            settings.remote_hosts,
            [host_rule("prod", HostMark::Production)]
        );
        assert_eq!(settings.theme, "paper");
    }

    #[test]
    fn unknown_keys_and_sections_are_silent() {
        // Sonraki setlerin anahtarları bugün tanı üretmemeli: `intensity`
        // ve `speed` referansın `[motion]` bölümünde var, bizde yok
        // (008 → Kapsam dışı; `keypress` 030'da tanındı ve tanık oldu).
        let text = "\
future = true
[terminal]
scrollback = 42
shape = \"block\"
[motion]
speed = \"brisk\"
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

    /// Sınamanın kâhini: düzenlemeyi `Settings`'e **elle** uygular, yazma
    /// yolundan bağımsız. Ondalıklar iki basamakta (yazma yolunun sözü).
    fn applied(mut settings: Settings, edit: &SettingsEdit) -> Settings {
        let two = |value: f64| (value * 100.0).round() / 100.0;
        match edit.clone() {
            SettingsEdit::Scrollback(lines) => settings.scrollback = lines,
            SettingsEdit::Cursor(shape) => settings.cursor = shape,
            SettingsEdit::CursorBlink(blink) => settings.cursor_blink = blink,
            SettingsEdit::CursorRadius(ratio) => settings.caret.radius_ratio = two(ratio),
            SettingsEdit::CursorGlow(glow) => settings.caret.glow = two(glow),
            SettingsEdit::CursorUnfocused(unfocused) => settings.caret.unfocused = unfocused,
            SettingsEdit::BlinkInterval(seconds) => settings.blink_interval = two(seconds),
            SettingsEdit::ConfirmClose(confirm) => settings.confirm_close = confirm,
            SettingsEdit::Theme(name) => settings.theme = name,
            SettingsEdit::LightTheme(name) => settings.light_theme = name,
            SettingsEdit::DarkTheme(name) => settings.dark_theme = name,
            SettingsEdit::FontFamily(name) => {
                settings.font.family = (!name.is_empty()).then_some(name);
            }
            SettingsEdit::FontSize(size) => settings.font.size = two(size),
            SettingsEdit::LineHeight(height) => settings.font.line_height = two(height),
            SettingsEdit::Osc52(mode) => settings.osc52 = mode,
            SettingsEdit::CursorMotion(motion) => settings.cursor_motion = motion,
            SettingsEdit::ReduceMotion(reduce) => settings.reduce_motion = reduce,
            SettingsEdit::SmoothScroll(smooth) => settings.smooth_scroll = smooth,
            SettingsEdit::Keypress(keypress) => settings.keypress = keypress,
            SettingsEdit::Erase(erase) => settings.erase = erase,
            SettingsEdit::ShellIntegration(integration) => {
                settings.shell_integration = integration;
            }
            // Kâhin yalnız boş listeden doğru: `every_edit`'in iki metni de
            // `hosts = []` taşıyor. Dolu listenin kuralı kendi sınamasında.
            SettingsEdit::RemoteHostMark { host, mark } => settings.remote_hosts.insert(
                0,
                HostRule {
                    pattern: bare_host(&host).to_owned(),
                    mark,
                },
            ),
        }
        settings
    }

    /// Her anahtardan varsayılan olmayan bir değer — her varyant en az bir
    /// kez; `FontFamily` iki kez, çünkü boş dizge "varsayılan aile" demek.
    fn every_edit() -> Vec<SettingsEdit> {
        vec![
            SettingsEdit::Scrollback(2500),
            SettingsEdit::Cursor(CaretShape::Beam),
            SettingsEdit::CursorBlink(CursorBlink::Auto),
            // İki basamağa yuvarlanıyor: 0.123 → 0.12.
            SettingsEdit::CursorRadius(0.123),
            SettingsEdit::CursorGlow(2.5),
            SettingsEdit::CursorUnfocused(UnfocusedCaret::Solid),
            SettingsEdit::BlinkInterval(0.75),
            SettingsEdit::ConfirmClose(ConfirmClose::Always),
            SettingsEdit::Theme("paper".to_owned()),
            SettingsEdit::LightTheme("paper".to_owned()),
            SettingsEdit::DarkTheme("ink".to_owned()),
            SettingsEdit::FontFamily("Menlo".to_owned()),
            SettingsEdit::FontFamily(String::new()),
            SettingsEdit::FontSize(14.5),
            SettingsEdit::LineHeight(1.25),
            SettingsEdit::Osc52(Osc52::Off),
            SettingsEdit::CursorMotion(CursorMotion::Ease),
            SettingsEdit::ReduceMotion(ReduceMotion::On),
            SettingsEdit::SmoothScroll(SmoothScroll::Off),
            SettingsEdit::Keypress(Keypress::Off),
            SettingsEdit::Erase(Erase::Off),
            SettingsEdit::ShellIntegration(ShellIntegration::Blocks),
            SettingsEdit::RemoteHostMark {
                host: "deploy@prod".to_owned(),
                mark: HostMark::Production,
            },
        ]
    }

    fn marked(text: &str, host: &str, mark: HostMark) -> String {
        let edit = SettingsEdit::RemoteHostMark {
            host: host.to_owned(),
            mark,
        };
        Settings::with_edit(text, &edit).expect("yazılabilir metin")
    }

    #[test]
    fn marking_a_host_creates_the_list() {
        // Boş dosya: bölüm ve anahtar doğuyor; desen `user@`'siz.
        assert_eq!(
            marked("", "deploy@prod", HostMark::Production),
            "[remote]\nhosts = [{ host = \"prod\", mark = \"production\" }]\n"
        );
        // Bölüm var, anahtar yok.
        assert_eq!(
            marked("[remote]\nfuture = 1\n", "vm", HostMark::Staging),
            "[remote]\nfuture = 1\nhosts = [{ host = \"vm\", mark = \"staging\" }]\n"
        );
        // Tek satırlı dizinin başına, virgülden sonra boşlukla.
        assert_eq!(
            marked(
                "[remote]\nhosts = [{ host = \"a\", mark = \"staging\" }]\n",
                "b",
                HostMark::Development
            ),
            "[remote]\nhosts = [{ host = \"b\", mark = \"development\" }, \
             { host = \"a\", mark = \"staging\" }]\n"
        );
    }

    #[test]
    fn marking_a_host_keeps_the_list_as_written() {
        // Yorumlar, bilinmeyen anahtar ve kullanıcının sırası yerinde.
        let text = "# üst\n[remote]\n# prod kırmızı\nhosts = [\n  \
                    { host = \"db\", mark = \"staging\" }, # veri\n  \
                    { host = \"prod-*\", mark = \"production\" },\n]\nfuture = 1\n";
        // Eşit desen (harf duyarsız) yerinde değişiyor, sıra korunuyor.
        assert_eq!(
            marked(text, "root@DB", HostMark::Development),
            text.replace("mark = \"staging\"", "mark = \"development\"")
        );
        // Yeni host başa, dizinin girintisiyle.
        assert_eq!(
            marked(text, "cache", HostMark::Production),
            text.replace(
                "hosts = [\n",
                "hosts = [\n  { host = \"cache\", mark = \"production\" },\n"
            )
        );
        // None tam girdiyi siliyor; glob eşleşmiyorsa başka bir şey yazılmıyor.
        let removed = marked(text, "db", HostMark::None);
        assert_eq!(clean(&removed).remote_hosts.len(), 1, "{removed}");
        assert!(removed.contains("# prod kırmızı") && removed.contains("future = 1"));
        assert!(!removed.contains("\"db\""), "{removed}");
        // Seçilen zaten geçerli çözüm (glob'dan gelse de): metin aynen.
        assert_eq!(marked(text, "db", HostMark::Staging), text);
        assert_eq!(marked(text, "prod-web", HostMark::Production), text);
        assert_eq!(marked(text, "vm", HostMark::None), text);
    }

    #[test]
    fn marking_a_host_beats_the_globs_in_front() {
        let resolved = |text: &str, host: &str| host_mark(&clean(text).remote_hosts, host);
        let globs = "[remote]\nhosts = [\n  { host = \"prod-*\", mark = \"production\" },\n]\n";
        // None bir globun yakaladığı host'u işaretsiz bırakıyor: başa `none`.
        let none = marked(globs, "prod-canary", HostMark::None);
        assert_eq!(
            none,
            "[remote]\nhosts = [\n  { host = \"prod-canary\", mark = \"none\" },\n  \
             { host = \"prod-*\", mark = \"production\" },\n]\n"
        );
        assert_eq!(resolved(&none, "prod-canary"), HostMark::None);
        assert_eq!(resolved(&none, "prod-web"), HostMark::Production);
        // Önünde glob olan tam girdi yerinde değişse etkisiz kalırdı: başa
        // taşınıyor.
        let behind = "[remote]\nhosts = [\n  { host = \"*\", mark = \"development\" },\n  \
                      { host = \"db\", mark = \"production\" },\n]\n";
        let moved = marked(behind, "db", HostMark::Staging);
        assert_eq!(
            moved,
            "[remote]\nhosts = [\n  { host = \"db\", mark = \"staging\" },\n  \
             { host = \"*\", mark = \"development\" },\n]\n"
        );
        // `user@`'li glob: None'ın yazdığı `user@`'siz desen onu geçiyor.
        let user = "[remote]\nhosts = [{ host = \"root@*\", mark = \"staging\" }]\n";
        let none = marked(user, "root@db", HostMark::None);
        assert_eq!(resolved(&none, "root@db"), HostMark::None);
        assert_eq!(resolved(&none, "root@web"), HostMark::Staging);
    }

    #[test]
    fn marking_a_host_in_an_array_of_sections() {
        let text = "[[remote.hosts]]\nhost = \"db\"\nmark = \"staging\" # veri\n\n\
                    [[remote.hosts]]\nhost = \"prod-*\"\nmark = \"production\"\n";
        let in_place = marked(text, "db", HostMark::Development);
        assert_eq!(
            in_place,
            text.replace("mark = \"staging\"", "mark = \"development\"")
        );
        let prepended = marked(text, "cache", HostMark::Production);
        assert_eq!(
            clean(&prepended).remote_hosts,
            [
                HostRule {
                    pattern: "cache".to_owned(),
                    mark: HostMark::Production
                },
                HostRule {
                    pattern: "db".to_owned(),
                    mark: HostMark::Staging
                },
                HostRule {
                    pattern: "prod-*".to_owned(),
                    mark: HostMark::Production
                },
            ],
            "{prepended}"
        );
        assert!(prepended.contains("# veri"), "{prepended}");
        // Önde ve arkada başka bölümler: yeni girdi dizinin yerinde doğuyor.
        let around = format!("[font]\nsize = 13\n\n# liste\n{text}\n[notes]\nx = 1\n");
        assert_eq!(
            marked(&around, "cache", HostMark::Production),
            around.replace(
                "# liste\n",
                "# liste\n[[remote.hosts]]\nhost = \"cache\"\nmark = \"production\"\n\n"
            )
        );
        let removed = marked(text, "db", HostMark::None);
        assert_eq!(clean(&removed).remote_hosts.len(), 1, "{removed}");
    }

    #[test]
    fn marking_a_host_refuses_a_broken_list() {
        // Ayrıştırılamayan metin ve bozuk dizi yazılmıyor: kullanıcının yarım
        // işi.
        for text in [
            "[remote\n",
            "[remote]\nhosts = [{ host = \"a\", mark = \"prod\" }]\n",
            "[remote]\nhosts = \"a\"\n",
            "remote = 1\n",
        ] {
            let edit = SettingsEdit::RemoteHostMark {
                host: "b".to_owned(),
                mark: HostMark::Production,
            };
            assert!(Settings::with_edit(text, &edit).is_err(), "{text}");
        }
    }

    #[test]
    fn a_mark_is_written_the_way_it_is_read() {
        for (_, mark) in HostMark::NAMES {
            let text = marked("", "a", HostMark::Production);
            let text = marked(&text, "a", *mark);
            assert_eq!(host_mark(&clean(&text).remote_hosts, "a"), *mark, "{text}");
        }
        assert_eq!(HostMark::Rgb(0x0a0b0c).written(), "#0a0b0c");
    }

    /// `written`, `text`'ten tek satırın değişmesiyle ya da tek satırın
    /// eklenmesiyle mi doğmuş — satır sonu `eol`.
    fn one_line_apart(text: &str, written: &str, eol: &str) -> bool {
        let before: Vec<&str> = text.split(eol).collect();
        let after: Vec<&str> = written.split(eol).collect();
        if before.len() == after.len() {
            return before.iter().zip(&after).filter(|(a, b)| a != b).count() == 1;
        }
        after.len() == before.len() + 1
            && (0..after.len()).any(|skip| {
                let mut rest = after.clone();
                rest.remove(skip);
                rest == before
            })
    }

    #[test]
    fn every_edit_reads_back_and_touches_one_line() {
        // Kullanıcının dosyası: şablonun yorumları ve sırası, üstüne
        // tanımadığımız bir anahtar ve bölüm, değerin yanında yorum.
        let rich = format!(
            "{}future = true\n\n[notes]\nx = 1\n",
            Settings::TEMPLATE.replace("scrollback = 10000", "scrollback = 10000  # plenty")
        );
        let crlf = rich.replace('\n', "\r\n");
        for edit in every_edit() {
            for (text, eol) in [(rich.as_str(), "\n"), (crlf.as_str(), "\r\n")] {
                let written = Settings::with_edit(text, &edit).expect("yazılabilir metin");
                assert!(one_line_apart(text, &written, eol), "{edit:?}\n{written}");
                assert_eq!(
                    clean(&written),
                    applied(clean(text), &edit),
                    "{edit:?}\n{written}"
                );
            }
            // Boş metin: bölüm ve anahtar eklenir.
            let written = Settings::with_edit("", &edit).expect("yazılabilir metin");
            assert_eq!(
                clean(&written),
                applied(Settings::default(), &edit),
                "{edit:?}"
            );
        }
    }

    #[test]
    fn edits_keep_the_way_the_section_is_written() {
        // `with_theme`'in sınaması her türden bir anahtar için: satır içi tablo
        // ve noktalı anahtar yazılışlarını korur, kabul edilmeyen değerin
        // yerine yazar (kullanıcı pencerede bir değer seçti).
        for (text, edit, expected) in [
            (
                "font = { size = 13, family = \"Menlo\" }\n",
                SettingsEdit::FontSize(14.5),
                "font = { size = 14.5, family = \"Menlo\" }\n",
            ),
            (
                "terminal.scrollback = 5\n",
                SettingsEdit::Scrollback(2500),
                "terminal.scrollback = 2500\n",
            ),
            (
                "[motion]\ncursor_motion = 3 # oops\n",
                SettingsEdit::CursorMotion(CursorMotion::Snap),
                "[motion]\ncursor_motion = \"snap\" # oops\n",
            ),
        ] {
            assert_eq!(
                Settings::with_edit(text, &edit).expect("yazılabilir metin"),
                expected
            );
        }
        // Bölüm olan anahtar: yerine değer yazmak alt tabloyu silerdi.
        for text in ["[font.size]\nx = 1\n", "[font]\nsize = { x = 1 }\n"] {
            let err = Settings::with_edit(text, &SettingsEdit::FontSize(14.0)).expect_err("bölüm");
            assert_eq!(err.key, Some("font.size"), "{text}");
            assert_eq!(err.message, "`font.size` must be a number, found a section");
        }
        let err = Settings::with_edit(
            "motion = 1\n",
            &SettingsEdit::SmoothScroll(SmoothScroll::Off),
        )
        .expect_err("bölüm değil");
        assert_eq!(err.key, Some("motion"));
    }

    #[test]
    fn every_name_reads_back_as_its_value() {
        // Tablo ↔ ayrıştırıcı bekçisi: her yazılış ayrıştırıcıdan kendi
        // varyantını veriyor ve `name()` onu geri yazıyor.
        fn check<T: Copy + PartialEq + std::fmt::Debug>(
            names: &[(&str, T)],
            name: fn(T) -> &'static str,
            section: &str,
            key: &str,
            read: fn(&Settings) -> T,
        ) {
            for &(written, value) in names {
                assert_eq!(name(value), written);
                let text = format!("[{section}]\n{key} = {written:?}\n");
                assert_eq!(read(&clean(&text)), value, "{text}");
            }
        }
        check(
            CaretShape::NAMES,
            CaretShape::name,
            "terminal",
            "cursor",
            |s| s.cursor,
        );
        check(
            CursorBlink::NAMES,
            CursorBlink::name,
            "terminal",
            "cursor_blink",
            |s| s.cursor_blink,
        );
        check(
            UnfocusedCaret::NAMES,
            UnfocusedCaret::name,
            "terminal",
            "cursor_unfocused",
            |s| s.caret.unfocused,
        );
        check(
            ConfirmClose::NAMES,
            ConfirmClose::name,
            "terminal",
            "confirm_close",
            |s| s.confirm_close,
        );
        check(Osc52::NAMES, Osc52::name, "clipboard", "osc52", |s| s.osc52);
        check(
            CursorMotion::NAMES,
            CursorMotion::name,
            "motion",
            "cursor_motion",
            |s| s.cursor_motion,
        );
        check(
            ReduceMotion::NAMES,
            ReduceMotion::name,
            "motion",
            "reduce_motion",
            |s| s.reduce_motion,
        );
        check(
            SmoothScroll::NAMES,
            SmoothScroll::name,
            "motion",
            "smooth_scroll",
            |s| s.smooth_scroll,
        );
        check(
            ShellIntegration::NAMES,
            ShellIntegration::name,
            "shell",
            "integration",
            |s| s.shell_integration,
        );
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
