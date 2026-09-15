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
//! **Bilinmeyen anahtar ve bölüm sessizce yoksayılır:** sonraki setlerin
//! anahtarı (`[motion]`) bugünkü sürümde tanı üretmemeli.
//!
//! Ayrıştırıcı önceki ayarları yalnız kabul edilmeyen değerin yerine geçecek
//! değer olarak görür ([`Settings::parse_keeping`], kayıt anı); fark almak
//! çağıranın işi ([`Settings::changes`]).
//!
//! Tanı tipi ve TOML yardımcıları tema dosyasının ayrıştırıcısıyla (`theme`)
//! ortak: iki dosyanın hata dili aynı olsun.

use std::fmt;

use toml_edit::{Document, Item, TableLike};

use crate::session::TerminalOptions;

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
/// `pub(crate)`: bugün dışarıda soran yok, `bt-shell`
/// [`Settings::follows_system`]'e bakıyor; menüden yazan phase-7 açar.
pub(crate) const SYSTEM_THEME: &str = "system";

/// Kullanıcının değiştirebildiği her şey — ayrıştırılmış ve doğrulanmış.
///
/// Alanlar `pub`: tip bir kayıt, davranış taşımıyor. Değerin geçerliliğini
/// kuran yol [`Settings::parse`]; elle kurulan bir `Settings` bu kuralları
/// atlayabilir ve bu bilerek serbest (sınamalar böyle kuruyor).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    /// `[terminal] scrollback`: geçmişte tutulan satır, `0..=SCROLLBACK_MAX`.
    pub scrollback: usize,
    /// `[appearance] theme`: [`SYSTEM_THEME`] ya da tema **adı** —
    /// `themes/{ad}.toml` ya da gömülü bir tema. Ad biçim olarak geçerli (boş
    /// değil, `/` yok); var olup olmadığı dosya sistemi ister ve `bt-shell`'in
    /// ad çözümünde.
    pub theme: String,
    /// `[appearance] light_theme`: `theme = "system"` iken açık görünümün
    /// teması. `theme`'den **ayrı** anahtar: menüden sabit bir tema seçmek
    /// yalnız `theme`'i yazar (007 phase-7) ve kullanıcının açık/koyu çifti
    /// yerinde kalır.
    pub light_theme: String,
    /// `[appearance] dark_theme`: `theme = "system"` iken koyu görünümün
    /// teması.
    pub dark_theme: String,
}

impl Default for Settings {
    /// Dosya yokken ve anahtar eksikken geçerli olan değerler.
    ///
    /// `scrollback` 006'ya kadar `bt-shell`'in `SCROLLBACK` sabitiydi; değer
    /// aynı kaldı, sahibi buraya taşındı. Tema sistemin görünümünü izler:
    /// açıkta gömülü `bateri-light`, koyuda gömülü `bateri`.
    fn default() -> Self {
        Self {
            scrollback: 10_000,
            theme: SYSTEM_THEME.to_owned(),
            light_theme: "bateri-light".to_owned(),
            dark_theme: "bateri".to_owned(),
        }
    }
}

/// Ayrıştırılabilen bir dosyanın sonucu: değerler ve kabul edilmeyenler.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    /// `settings.toml`'un metni → değerler + tanılar, ya da ayrıştırılamadı.
    ///
    /// `Err` **yalnız** geçersiz TOML'da; anahtar düzeyindeki her sorun
    /// `Ok`'un tanı listesine düşer ve o anahtar varsayılanını alır.
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
            }
            None if root.contains_key("terminal") => {
                parsed.settings.scrollback = fallback.scrollback;
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
        }
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
        assert_eq!(before.changes(&after), Changes { terminal: true });
        assert_eq!(after.terminal(), TerminalOptions { scrollback: 20 });
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
            theme: "paper".to_owned(),
            light_theme: "chalk".to_owned(),
            dark_theme: "ink".to_owned(),
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
        let text = "\
future = true
[terminal]
scrollback = 42
shape = \"block\"
[motion]
cursor = \"spring\"
";
        assert_eq!(clean(text).scrollback, 42);
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
