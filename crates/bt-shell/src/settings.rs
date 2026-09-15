//! Ayar ve tema dosyalarının yükleyicisi: kök dizindeki `settings.toml`'u ve
//! `themes/{ad}.toml`'u okur, `bt-core`'un saf ayrıştırıcılarına verir; tema
//! **adını** bir temaya çözen de burası ([`load_theme`]).
//!
//! Kök **parametre**: üretimde `$HOME/.config/bateri/` ([`config_root`]),
//! sınamada geçici bir dizin — hiçbir sınama gerçek `HOME`'u okumaz. Süreli
//! koşu yükleyiciyi hiç çağırmaz; o dalın tek yeri `app::Inputs`.
//!
//! Sonuç dört ayrı hâl taşır ve [`Settings::default`]'a **çökertilmez**:
//! "dosya yok" sessiz ve doğru bir hâl, "ayrıştırılamadı" ise canlı
//! yenilemede hiçbir şey uygulamamayı ve açılışta OSC 52'yi kapatmayı
//! (phase-8) gerektiriyor. Hangi hâlde ne yapılacağı çağıranın kuralı;
//! açılışınki [`Loaded::at_launch`], kayıt anınınki [`Loaded::live`]. Tema
//! adının iki kuralı da aynı ikilik: [`ThemeLoaded::or_embedded`],
//! [`ThemeLoaded::or_current`]. İzlenen yollar da buradan
//! ([`watched_paths`], [`theme_path`]): okunan yolla izlenen yol ayrışmasın.
//!
//! **Bilinen sınır — okuma ana thread'de ve sınırsız bekler.** Düz dosya
//! olmayan yol (FIFO, `/dev/zero`'ya bağ, dizin) okunmadan elenir; ama iCloud
//! Drive'dan tahliye edilmiş bir dosyaya bağ ya da takılmış bir ağ ev dizini
//! açılışı durdurabilir. Ayar dosyası küçük ve yereldir; bunu ayrı bir
//! thread'e taşımak açılış sırasını (ayar → geometri → oturum) eşzamansız
//! yapardı ve bu sette bedeline değmedi.

use std::io;
use std::path::{Path, PathBuf};

use bt_core::{Diagnostic, Parsed, Settings, Theme};

/// Ayar dosyasının adı; tanı metinleri de kullanıcıya bu adla söylüyor.
pub(crate) const FILE_NAME: &str = "settings.toml";

/// Kullanıcı temalarının dizini, kökün altında.
const THEMES_DIR: &str = "themes";

/// Üretimdeki kök: `{ev}/.config/bateri/`.
///
/// macOS'un `~/Library/Application Support`'u değil: dosya elle düzenleniyor
/// ve `CLAUDE.md`'nin sözleşmesi bu yolu adıyla veriyor.
pub(crate) fn config_root(home: &Path) -> PathBuf {
    home.join(".config").join("bateri")
}

/// Kökün izlenen yolları (`watch`): kökün kendisi, `themes/` ve
/// `settings.toml`. Etkin tema dosyası ayrı ([`theme_path`]): adı ayarlardan
/// ve görünümden türüyor, ayrı kurulup ayrı yenileniyor.
///
/// Kök ve `themes/` dizin olarak dosya doğumunu, silinmesini ve üstüne
/// taşınmasını; `settings.toml` dosya olarak yerinde yazmayı ve bağın
/// hedefindeki kaydı veriyor. Olmayan yol kaynak doğurmaz.
pub(crate) fn watched_paths(root: &Path) -> [PathBuf; 3] {
    [
        root.to_path_buf(),
        root.join(THEMES_DIR),
        root.join(FILE_NAME),
    ]
}

/// Kullanıcı temasının kökten göreli yolu — tanı metni de dosyayı bu adla
/// söylüyor.
fn theme_file(name: &str) -> String {
    format!("{THEMES_DIR}/{name}.toml")
}

/// `{root}/themes/{ad}.toml`: [`load_theme`]'in okuduğu ve izleyicinin
/// etkin tema için kurduğu dosya. Gömülü tema seçiliyse dosya yoktur ve
/// kaynak kurulmaz.
pub(crate) fn theme_path(root: &Path, name: &str) -> PathBuf {
    root.join(theme_file(name))
}

/// "Settings…"ın ilk yarısı: kök dizini ve `settings.toml`'u yoksa
/// [`Settings::TEMPLATE`] ile yaratır, dosyanın yolunu döner.
///
/// **Var olanı asla ezmez** — bozuk dosya da, sembolik bağ da, hedefi olmayan
/// bağ da (`create_new`, `O_EXCL`: bağı izlemiyor). Bozuk dosyanın içeriği
/// kullanıcının yarım işi; hedefsiz bağın hedefini yaratmak dotfile deposunun
/// taşındığı yerde başıboş bir dosya bırakırdı, ayar yuvası o bağı zaten
/// söylüyor.
pub(crate) fn create_if_missing(root: &Path) -> io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    let path = root.join(FILE_NAME);
    let created = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path);
    match created {
        Ok(mut file) => {
            use std::io::Write as _;
            file.write_all(Settings::TEMPLATE.as_bytes())?;
        }
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
        Err(err) => return Err(err),
    }
    Ok(path)
}

/// View ▸ Theme ▸'nin yazması: `{root}/settings.toml`'da `[appearance]
/// theme`'i `name` yapar ([`Settings::with_theme`]); hata yazma yuvasının
/// iletisi.
///
/// **Yalnız yazar, uygulamaz** — uygulayan dosyayı okuyan yol, izleyicinin
/// yolu (`app`).
///
/// - Dosya **o anda** okunuyor: elde tutulan bir kopya editörde yapılmış
///   kaydı ezerdi.
/// - Dosya yoksa "Settings…"ın yolu ([`create_if_missing`]): şablon, üstüne
///   anahtar. Hedefi olmayan bağın hedefi yaratılmıyor; okuma onu söylüyor.
/// - **Yerinde** yazılıyor (`O_TRUNC`): sembolik bağ izleniyor, hedef
///   güncelleniyor. Geçici dosya + yeniden adlandırma bağı düz dosyaya
///   çevirirdi; korunduğu bir yarış da yok, okuma ve yazma aynı ana kuyrukta.
///   Boşaltmayla yazma arasındaki boş dosyayı izleyici dosyasızlık sayıyor
///   ([`load_keeping`]).
/// - Okunamayan ya da ayrıştırılamayan dosyaya **yazılmaz**: içerik
///   kullanıcının yarım işi.
///
/// **Bilinen sınır — boşaltmayla yazma arası.** `write` boşaltılmış dosyaya
/// tek bir (1 KB'ın altında) yazma yapıyor; o yazma hata verirse ya da süreç
/// tam o anda ölürse dosya boş ya da yarım kalır. Çözülmüş hedefin yanına
/// geçici dosya + yeniden adlandırma bu pencereyi kapatırdı ama sabit bağı
/// koparır, izinleri ve genişletilmiş öznitelikleri düşürür ve yazılamayan
/// dizinde başarısız olur; yerinde yazma planın kararı (`/code-review` bulgusu,
/// waive: `.tasks/007-ayarlar-ve-tema/phase-7.md`).
pub(crate) fn write_theme(root: &Path, name: &str) -> Result<(), String> {
    let not_saved = |reason: String| format!("{reason}; the theme was not saved");
    let path = create_if_missing(root)
        .map_err(|err| format!("{FILE_NAME} could not be created: {err}"))?;
    let text = match read_text(&path) {
        Text::Read(text) => text,
        Text::Unreadable(err) => {
            return Err(not_saved(format!("{FILE_NAME} could not be read: {err}")));
        }
        // Yaratmayla okuma arasında silindi.
        Text::Missing => return Err(not_saved(format!("{FILE_NAME} was removed"))),
    };
    let written = Settings::with_theme(&text, name).map_err(|d| not_saved(notice(&d)))?;
    std::fs::write(&path, written).map_err(|err| format!("{FILE_NAME} could not be written: {err}"))
}

/// View ▸ Theme ▸'deki kullanıcı temaları: `{root}/themes/*.toml`'un adları,
/// büyük/küçük harf duyarsız sırayla.
///
/// Seçilemeyecek olan listelenmez: düz dosya olmayan (dizin, hedefi olmayan
/// bağ), UTF-8 olmayan ve nokta ile başlayan ad (gizli dosya, başka dosya
/// sisteminden kopyalanan AppleDouble `._x.toml`), ayrılmış
/// [`SYSTEM_THEME`](bt_core::SYSTEM_THEME) ve gömülü bir temanın adı — o
/// dosya gömülüyü gölgeliyor ve seçimi gömülü adın öğesiyle aynı.
///
/// Dizin yoksa ya da okunamıyorsa liste boş: menünün hata gösterecek yeri yok
/// ve tema dizini olmayan kullanıcı olağan hâl.
pub(crate) fn user_theme_names(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join(THEMES_DIR)) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".toml")?;
            // Ad sınanıyor, gövde değil: tam `.toml` adlı dosyanın gövdesi boş.
            let selectable = !name.starts_with('.')
                && stem != bt_core::SYSTEM_THEME
                && Theme::embedded(stem).is_none()
                // Bağı izliyor: bağlı tema dosyası da seçilebilir.
                && std::fs::metadata(entry.path()).is_ok_and(|meta| meta.is_file());
            selectable.then(|| stem.to_owned())
        })
        .collect();
    names.sort_by_cached_key(|name| name.to_lowercase());
    names
}

/// Bir okumanın sonucu.
#[derive(Debug)]
pub(crate) enum Loaded {
    /// Dosya yok ya da boş: kullanıcı hiç ayar yazmamış. Tanı **yok**.
    Missing,
    /// Dosya var ama okunamadı: izin, düz dosya değil (dizin, FIFO), kırık
    /// sembolik bağ, UTF-8 olmayan içerik.
    Unreadable(io::Error),
    /// Okundu, TOML olarak ayrıştırılamadı.
    Unparseable(Diagnostic),
    /// Ayrıştırıldı; tanılar kabul edilmeyen anahtarlar.
    Parsed(Parsed),
}

/// Bir metin dosyasının okunuşu — ayar ve tema dosyasının ortak kapısı.
enum Text {
    Missing,
    Unreadable(io::Error),
    Read(String),
}

/// `path`'i okur; "dosya yok" ile "okunamadı"yı ayırır.
fn read_text(path: &Path) -> Text {
    // `metadata` bağı izliyor: hedefin türü soruluyor, bağın değil.
    match std::fs::metadata(path) {
        // Kırık bağ da `NotFound` veriyor; ama dosya `ls`'te görünüyor ve
        // "hiç ayar yok" diye susmak kullanıcıyı neden işlemediğini aramaya
        // bırakırdı (dotfile yöneticisinin taşınmış deposu).
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return match std::fs::symlink_metadata(path) {
                Ok(_) => {
                    Text::Unreadable(io::Error::other("symbolic link points to a missing file"))
                }
                Err(_) => Text::Missing,
            };
        }
        Err(err) => return Text::Unreadable(err),
        // FIFO okumayı sonsuza dek bekletir, `/dev/zero` belleği bitirir.
        Ok(meta) if !meta.is_file() => {
            return Text::Unreadable(io::Error::other("not a regular file"));
        }
        Ok(_) => {}
    }
    match std::fs::read_to_string(path) {
        // Denetimle okuma arasında silindi: dosya yok hâli.
        Err(err) if err.kind() == io::ErrorKind::NotFound => Text::Missing,
        Err(err) => Text::Unreadable(err),
        Ok(text) => Text::Read(text),
    }
}

/// `{root}/settings.toml`'u okur — açılışın okuması: kabul edilmeyen değer
/// varsayılanını alır (`osc52` hariç, o kapalıya düşer).
pub(crate) fn load(root: &Path) -> Loaded {
    load_keeping(root, &Settings::default())
}

/// Kayıt anının okuması: kabul edilmeyen değer `current`'inkini alır
/// ([`Settings::parse_keeping`]; `osc52` hariç, o kapalıya düşer). Yanlış türde kaydedilmiş bir `scrollback`
/// varsayılana düşseydi geçmişi geri dönülmez kırpardı.
///
/// Boş dosya (yalnız boşluk) dosyasızlık sayılır. Yerinde kaydeden editör
/// önce boşaltıyor (`O_TRUNC`) sonra yazıyor, boşaltma da olay veriyor: arada
/// okunan boş dosya varsayılan `scrollback`'i uygulasaydı geçmiş geri dönülmez
/// kırpılırdı. Açılışta ikisi zaten aynıydı (varsayılanlar, tanısız).
pub(crate) fn load_keeping(root: &Path, current: &Settings) -> Loaded {
    match read_text(&root.join(FILE_NAME)) {
        Text::Missing => Loaded::Missing,
        Text::Read(text) if text.trim().is_empty() => Loaded::Missing,
        Text::Unreadable(err) => Loaded::Unreadable(err),
        Text::Read(text) => match Settings::parse_keeping(&text, current) {
            Ok(parsed) => Loaded::Parsed(parsed),
            Err(diagnostic) => Loaded::Unparseable(diagnostic),
        },
    }
}

/// Bir tema adının çözümü.
#[derive(Debug)]
pub(crate) enum ThemeLoaded {
    /// Kullanıcı dosyasından ya da gömülü temalardan bulundu; iletiler
    /// dosyada kabul edilmeyen renkler (alt başlığa hazır biçimde).
    Found(Theme, Vec<String>),
    /// Kullanılamadı: dosya okunamadı ya da ayrıştırılamadı, ya da ad hiçbir
    /// yerde yok. Tek ileti; hangi temanın geçerli kalacağı çağıranın kuralı.
    Failed(String),
}

/// Tema adını çözer: önce `{root}/themes/{ad}.toml`, sonra gömülü temalar.
///
/// `root` `None` → ev dizini çözülemedi, yalnız gömülüler. Dosyanın eksik
/// anahtarı **tabandan** gelir: gömülü bir temayı gölgeleyen dosyada o
/// temanın kendisi, başka adda `Theme::BATERI` (`docs/AYARLAR.md` → Temalar).
/// Gölgeleyen `themes/bateri-light.toml`'a yalnız `accent` yazan kullanıcı
/// açık temayı kırmızı imleçle bekliyor; koyu tabandan okunsaydı açık modda
/// koyu zemin alırdı (`/code-review` bulgusu).
///
/// **Dosya var ama kullanılamıyorsa gömülüye düşülmez**: bozuk bir
/// `themes/bateri.toml` sessizce gömülü `bateri`'yi açsaydı kullanıcı
/// dosyasının neden işlemediğini göremezdi. Dosya **yoksa** gömülü aranır —
/// gölgelenmemiş ad budur.
///
/// **Boş dosya kullanılamaz sayılır**, `settings.toml`'un kuralıyla
/// ([`load_keeping`]): yerinde kaydeden editör dosyayı önce boşaltıyor ve
/// kaydın ortasında okunan boş tema tabanın kendisi olurdu — canlı yenileme
/// pencereyi o anda koyu tabana çakardı. Kayıt anında ekrandaki tema kalır
/// ([`ThemeLoaded::or_current`]), açılışta görünüme uyan gömülü tema gelir.
///
/// Adın biçimi (`/` yok, boş değil) `bt-core`'da zaten sınandı; burada yeniden
/// sınanmıyor, `Settings`'ten gelmeyen bir ad bu fonksiyona hiç verilmiyor.
pub(crate) fn load_theme(root: Option<&Path>, name: &str) -> ThemeLoaded {
    if let Some(root) = root {
        let file = theme_file(name);
        match read_text(&root.join(&file)) {
            Text::Missing => {}
            Text::Unreadable(err) => {
                return ThemeLoaded::Failed(format!("{file} could not be read: {err}"));
            }
            Text::Read(text) if text.trim().is_empty() => {
                return ThemeLoaded::Failed(format!("{file} is empty"));
            }
            Text::Read(text) => {
                let base = Theme::embedded(name).unwrap_or(Theme::BATERI);
                return match Theme::parse(&text, &base) {
                    Ok((theme, diagnostics)) => ThemeLoaded::Found(
                        theme,
                        diagnostics.iter().map(|d| format!("{file}: {d}")).collect(),
                    ),
                    Err(diagnostic) => ThemeLoaded::Failed(format!("{file}: {diagnostic}")),
                };
            }
        }
    }
    match Theme::embedded(name) {
        Some(theme) => ThemeLoaded::Found(theme, Vec::new()),
        None => ThemeLoaded::Failed(format!("theme \"{name}\" not found")),
    }
}

impl ThemeLoaded {
    /// Temanın **bir görünüm için** seçildiği anın kuralı — açılış ve görünüm
    /// değişimi: kullanılamayan temanın yerine görünüme uyan gömülü tema
    /// (koyuda `bateri`, açıkta `bateri-light`) ve bunu söyleyen ileti.
    ///
    /// Yedek, dosyasız bir kullanıcının o görünümde göreceği tema
    /// (`Settings::default().theme_for(dark)`): açık modda `light_theme`'ini
    /// yanlış yazan kullanıcıya koyu bir pencere açmak hatayı ikinci bir
    /// sürprizle büyütürdü. Görünüm değişiminde "ekrandaki tema kalır"
    /// denemez — ekrandaki tema öteki görünümün teması (`dark_theme` bozukken
    /// açıktan koyuya dönen pencere açık kalırdı). "Ekrandaki tema kalır"
    /// kuralı görünüm aynıyken dosyanın kaydedildiği canlı yenilemenin
    /// ([`ThemeLoaded::or_current`]).
    pub(crate) fn or_embedded(self, dark: bool) -> (Theme, Vec<String>) {
        match self {
            ThemeLoaded::Found(theme, messages) => (theme, messages),
            ThemeLoaded::Failed(message) => {
                let defaults = Settings::default();
                let name = defaults.theme_for(dark);
                // Varsayılan adların ikisi de gömülü (`Theme::embedded`'in
                // sınaması); `BATERI` yalnız o tablo bozulursa.
                let theme = Theme::embedded(name).unwrap_or(Theme::BATERI);
                (theme, vec![format!("{message}; using {name}")])
            }
        }
    }

    /// Canlı yenilemenin kuralı — görünüm aynı, bir dosya kaydedildi:
    /// kullanılamayan tema **takas edilmez** (`None`), ekrandaki tema kalır ve
    /// ileti bunu söyler.
    ///
    /// Görünüme uyan gömülü temaya düşmek ([`ThemeLoaded::or_embedded`])
    /// burada düzenlemeyi cezalandırırdı: yarım kaydedilmiş bir tema dosyası
    /// ya da yazılırken eksik kalan bir ad pencereyi her kayıtta gömülü temaya
    /// çakıp geri döndürürdü. Kural adın ayar dosyasında değişmesinde de aynı
    /// — ikisi de düzenleme anı. Yeniden açılışta görünümün kuralı geçerli.
    pub(crate) fn or_current(self) -> (Option<Theme>, Vec<String>) {
        match self {
            ThemeLoaded::Found(theme, messages) => (Some(theme), messages),
            ThemeLoaded::Failed(message) => {
                (None, vec![format!("{message}; keeping the current theme")])
            }
        }
    }
}

/// Bir tanının alt başlıktaki ve stderr'deki biçimi; iki kolun ve canlı
/// yenilemenin (phase-4) aynı biçimi kullanması için tek yerde.
fn notice(diagnostic: &Diagnostic) -> String {
    format!("{FILE_NAME}: {diagnostic}")
}

impl Loaded {
    /// Açılışın kuralı: kullanılacak ayarlar ve alt başlığa gidecek tanılar.
    ///
    /// Okunamayan ya da ayrıştırılamayan dosyada **varsayılanlar**, OSC 52
    /// kapalı ([`Settings::for_unusable_file`]): pencere yine açılmalı, bozuk
    /// bir dosya terminali kilitlememeli, ama dosyadaki `osc52 = "off"`
    /// okunamadı diye pano açığa düşmemeli. Dosya yoksa düz varsayılanlar.
    /// Ayrıştırılan dosyada her anahtar kendi değerini ya da varsayılanını
    /// zaten aldı.
    pub(crate) fn at_launch(self) -> (Settings, Vec<String>) {
        match self {
            Loaded::Missing => (Settings::default(), Vec::new()),
            Loaded::Unreadable(err) => (
                Settings::for_unusable_file(),
                vec![format!("{FILE_NAME} could not be read: {err}")],
            ),
            Loaded::Unparseable(diagnostic) => {
                (Settings::for_unusable_file(), vec![notice(&diagnostic)])
            }
            Loaded::Parsed(parsed) => (
                parsed.settings,
                parsed.diagnostics.iter().map(notice).collect(),
            ),
        }
    }

    /// Canlı yenilemenin kuralı: uygulanacak ayarlar (`None` → **hiçbir
    /// şey** uygulanmaz, geçerli ayarlar kalır) ve ayar yuvasının tanıları.
    ///
    /// - **Ayrıştırılamayan ya da okunamayan dosya** → `None` + tanı. Yarım
    ///   kayıt (eksik tırnak) ekranı bozmaz; düzeltilen kayıt uygulanır.
    ///   Açılıştan farkı bu: orada varsayılanlarla açmak zorunlu.
    /// - **Dosya yok** → `None`, tanısız. Editörler kaydı çoğu zaman "eskiyi
    ///   kenara taşı, yenisini yaz" diye yapıyor (vim'in yedeği) ve arada yol
    ///   bir an yok; varsayılanları uygulamak her kayıtta pencereyi çakardı.
    ///   Boş dosya da bu kol ([`load_keeping`]). Bedeli: dosyayı gerçekten
    ///   silen ya da boşaltan kullanıcı varsayılanları yeniden açılışta görür
    ///   (`docs/AYARLAR.md`).
    pub(crate) fn live(self) -> (Option<Settings>, Vec<String>) {
        match self {
            Loaded::Missing => (None, Vec::new()),
            Loaded::Unreadable(err) => {
                (None, vec![format!("{FILE_NAME} could not be read: {err}")])
            }
            Loaded::Unparseable(diagnostic) => (None, vec![notice(&diagnostic)]),
            Loaded::Parsed(parsed) => (
                Some(parsed.settings),
                parsed.diagnostics.iter().map(notice).collect(),
            ),
        }
    }
}

/// Sınamaya özel geçici kök; süreç kimliği paralel koşan iki `cargo test`'i
/// ayırıyor. `tempfile` bir bağımlılık kararı olurdu. İzleme sınamaları
/// (`watch`) da kullanıyor; ad önekleri çakışmasın.
#[cfg(test)]
pub(crate) struct TempRoot(pub(crate) PathBuf);

#[cfg(test)]
impl TempRoot {
    pub(crate) fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("bateri-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("geçici kök kurulamadı");
        Self(path)
    }
}

#[cfg(test)]
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_root_is_under_dot_config() {
        assert_eq!(
            config_root(Path::new("/Users/someone")),
            PathBuf::from("/Users/someone/.config/bateri")
        );
    }

    #[test]
    fn settings_command_creates_the_template_once() {
        // Dizin de dosya da yok: ikisi yaratılır, şablon açılışta tanısız
        // varsayılanları verir — "Settings…" davranışı değiştirmez.
        let root = TempRoot::new("create");
        let config = root.0.join("nested").join("bateri");
        let path = create_if_missing(&config).expect("şablon yaratılamadı");
        assert_eq!(path, config.join(FILE_NAME));
        assert_eq!(
            std::fs::read_to_string(&path).expect("okunamadı"),
            Settings::TEMPLATE
        );
        assert_eq!(load(&config).at_launch(), (Settings::default(), Vec::new()));

        // Var olan dosya ezilmez, bozuk olsa da: yarım kalmış bir düzenleme.
        std::fs::write(&path, "[terminal\n").expect("yazılamadı");
        assert_eq!(create_if_missing(&config).expect("ikinci çağrı"), path);
        assert_eq!(
            std::fs::read_to_string(&path).expect("okunamadı"),
            "[terminal\n"
        );
    }

    #[test]
    fn settings_command_does_not_write_through_a_dangling_link() {
        // Taşınmış dotfile deposuna bakan bağ: hedef yaratılmaz, bağ kalır ve
        // ayar yuvası onu söylemeye devam eder.
        let root = TempRoot::new("create-dangling");
        let target = root.0.join("moved-away.toml");
        std::os::unix::fs::symlink(&target, root.0.join(FILE_NAME)).expect("bağ kurulamadı");
        assert!(create_if_missing(&root.0).is_ok());
        assert!(!target.exists(), "bağın hedefi yaratıldı");
        assert!(matches!(load(&root.0), Loaded::Unreadable(_)));
    }

    #[test]
    fn settings_command_reports_an_uncreatable_root() {
        // Kökün yerinde bir dosya: dizin yaratılamaz, hata çağırana döner.
        let root = TempRoot::new("create-blocked");
        let config = root.0.join("bateri");
        std::fs::write(&config, "").expect("yazılamadı");
        assert!(create_if_missing(&config).is_err());
    }

    #[test]
    fn theme_write_updates_the_file_in_place() {
        // Yorum ve tanınmayan anahtar kalır, çift yerinde; okuma yeni temayı
        // görür.
        let root = TempRoot::new("write");
        let text =
            "# mine\n[appearance]\ntheme = \"system\" # os\ndark_theme = \"ink\"\n[x]\ny = 1\n";
        std::fs::write(root.0.join(FILE_NAME), text).expect("yazılamadı");
        assert_eq!(write_theme(&root.0, "paper"), Ok(()));
        assert_eq!(
            std::fs::read_to_string(root.0.join(FILE_NAME)).expect("okunamadı"),
            text.replace("\"system\"", "\"paper\"")
        );
        let (settings, notices) = load(&root.0).at_launch();
        assert_eq!(
            (settings.theme.as_str(), settings.dark_theme.as_str()),
            ("paper", "ink")
        );
        assert!(notices.is_empty(), "{notices:?}");
    }

    #[test]
    fn theme_write_goes_through_a_symlink() {
        // Dotfile deposu: hedef güncellenir, bağ bağ olarak kalır — geçici
        // dosya + yeniden adlandırma onu düz dosyaya çevirirdi.
        let root = TempRoot::new("write-symlink");
        let target = root.0.join("dotfiles.toml");
        std::fs::write(&target, "[terminal]\nscrollback = 7\n").expect("yazılamadı");
        let link = root.0.join(FILE_NAME);
        std::os::unix::fs::symlink(&target, &link).expect("bağ kurulamadı");
        assert_eq!(write_theme(&root.0, "paper"), Ok(()));
        assert!(
            std::fs::symlink_metadata(&link)
                .expect("bağ yok")
                .file_type()
                .is_symlink(),
            "bağ düz dosyaya döndü"
        );
        assert_eq!(
            std::fs::read_to_string(&target).expect("okunamadı"),
            "[terminal]\nscrollback = 7\n\n[appearance]\ntheme = \"paper\"\n"
        );
    }

    #[test]
    fn theme_write_refuses_a_file_it_cannot_parse() {
        // Kullanıcının yarım işi ezilmez; ileti yazma yuvasına gider.
        let root = TempRoot::new("write-unparseable");
        let text = "[appearance\ntheme = \"ink\"\n";
        std::fs::write(root.0.join(FILE_NAME), text).expect("yazılamadı");
        let err = write_theme(&root.0, "paper").expect_err("yazılmamalı");
        assert!(
            err.starts_with("settings.toml: line 1: invalid TOML: ")
                && err.ends_with("; the theme was not saved"),
            "{err}"
        );
        assert_eq!(
            std::fs::read_to_string(root.0.join(FILE_NAME)).expect("okunamadı"),
            text
        );

        // Bölüm olmayan `appearance` da ezilmez.
        std::fs::write(root.0.join(FILE_NAME), "appearance = 1\n").expect("yazılamadı");
        assert_eq!(
            write_theme(&root.0, "paper"),
            Err("settings.toml: line 1: `appearance` must be a section, found an integer; the theme was not saved".to_owned())
        );

        // Hedefi olmayan bağ: hedef yaratılmaz.
        std::fs::remove_file(root.0.join(FILE_NAME)).expect("silinemedi");
        let moved = root.0.join("moved.toml");
        std::os::unix::fs::symlink(&moved, root.0.join(FILE_NAME)).expect("bağ kurulamadı");
        assert_eq!(
            write_theme(&root.0, "paper"),
            Err("settings.toml could not be read: symbolic link points to a missing file; the theme was not saved".to_owned())
        );
        assert!(!moved.exists(), "bağın hedefi yaratıldı");
    }

    #[test]
    fn theme_write_without_a_file_starts_from_the_template() {
        // Dizin de dosya da yok: "Settings…"ın yolu, üstüne anahtar.
        let root = TempRoot::new("write-missing");
        let config = root.0.join("nested").join("bateri");
        assert_eq!(write_theme(&config, "paper"), Ok(()));
        assert_eq!(
            std::fs::read_to_string(config.join(FILE_NAME)).expect("okunamadı"),
            // Satır başıyla: şablonun yorumu da `theme = "system"` diyor ve
            // yerinde kalıyor.
            Settings::TEMPLATE.replace("\ntheme = \"system\"\n", "\ntheme = \"paper\"\n")
        );
        assert_eq!(
            load(&config).at_launch(),
            (
                Settings {
                    theme: "paper".to_owned(),
                    ..Settings::default()
                },
                Vec::new()
            )
        );
    }

    #[test]
    fn user_theme_names_are_the_selectable_files() {
        let root = TempRoot::new("theme-names");
        // Tema dizini yoksa liste boş, hata değil.
        assert_eq!(user_theme_names(&root.0), Vec::<String>::new());
        // `""`: adı tam `.toml` olan dosya — `/code-review` bulgusu, boş
        // başlıklı bir öğe `theme = ""` yazardı.
        for name in ["paper", "Ink", "bateri", "system", ".hidden", "._paper", ""] {
            write_theme_file(&root, name, "");
        }
        let dir = root.0.join(THEMES_DIR);
        std::fs::write(dir.join("notes.txt"), "").expect("yazılamadı");
        std::fs::create_dir(dir.join("folder.toml")).expect("dizin kurulamadı");
        std::os::unix::fs::symlink(dir.join("paper.toml"), dir.join("linked.toml"))
            .expect("bağ kurulamadı");
        std::os::unix::fs::symlink(dir.join("gone.toml"), dir.join("dangling.toml"))
            .expect("bağ kurulamadı");
        // Gömülü adı gölgeleyen dosya (`bateri`) gömülü adın öğesiyle aynı
        // seçim, ayrıca listelenmez; `system` bir tema adı değil; nokta ile
        // başlayan (AppleDouble `._x`, gizli) ve düz dosya olmayan girdiler
        // seçilemez. Sıra büyük/küçük harf duyarsız.
        assert_eq!(user_theme_names(&root.0), ["Ink", "linked", "paper"]);
    }

    #[test]
    fn missing_file_is_silent_default() {
        let root = TempRoot::new("missing");
        let loaded = load(&root.0);
        assert!(matches!(loaded, Loaded::Missing), "{loaded:?}");
        assert_eq!(loaded.at_launch(), (Settings::default(), Vec::new()));

        // Kökün kendisi de yoksa aynı hâl: kullanıcı dizini hiç kurmamış.
        let loaded = load(&root.0.join("absent"));
        assert!(matches!(loaded, Loaded::Missing), "{loaded:?}");
    }

    #[test]
    fn unreadable_file_is_reported_and_defaults_at_launch() {
        // İzinle değil dizinle: root olarak koşan bir sınama `chmod 000`'ı
        // yine okurdu.
        let root = TempRoot::new("unreadable");
        std::fs::create_dir(root.0.join(FILE_NAME)).expect("dizin kurulamadı");
        let loaded = load(&root.0);
        assert!(matches!(loaded, Loaded::Unreadable(_)), "{loaded:?}");
        let (settings, notices) = loaded.at_launch();
        // Okunamayan dosyanın `osc52 = "off"`'u olabilir: pano kapalıya düşer.
        assert_eq!(settings, Settings::for_unusable_file());
        assert_eq!(notices.len(), 1);
        assert_eq!(
            notices,
            ["settings.toml could not be read: not a regular file"]
        );
    }

    #[test]
    fn dangling_symlink_is_not_missing() {
        let root = TempRoot::new("dangling");
        std::os::unix::fs::symlink(root.0.join("moved-away.toml"), root.0.join(FILE_NAME))
            .expect("bağ kurulamadı");
        let loaded = load(&root.0);
        assert!(matches!(loaded, Loaded::Unreadable(_)), "{loaded:?}");
        let (settings, notices) = loaded.at_launch();
        assert_eq!(settings, Settings::for_unusable_file());
        assert_eq!(
            notices,
            ["settings.toml could not be read: symbolic link points to a missing file"]
        );
    }

    #[test]
    fn symlink_to_a_file_is_read() {
        // Dotfile yöneticisinin olağan hâli: bağ, depodaki dosyaya.
        let root = TempRoot::new("symlink");
        std::fs::write(root.0.join("real.toml"), "[terminal]\nscrollback = 7\n")
            .expect("yazılamadı");
        std::os::unix::fs::symlink(root.0.join("real.toml"), root.0.join(FILE_NAME))
            .expect("bağ kurulamadı");
        assert_eq!(load(&root.0).at_launch().0.scrollback, 7);
    }

    #[test]
    fn unparseable_file_is_its_own_result() {
        let root = TempRoot::new("unparseable");
        std::fs::write(root.0.join(FILE_NAME), "[terminal\n").expect("yazılamadı");
        let loaded = load(&root.0);
        assert!(matches!(loaded, Loaded::Unparseable(_)), "{loaded:?}");
        let (settings, notices) = loaded.at_launch();
        // Bütün ayarlar varsayılan, OSC 52 hariç: kapalıya düşer.
        assert_eq!(settings, Settings::for_unusable_file());
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("settings.toml: line 1: invalid TOML: "),
            "{notices:?}"
        );
    }

    #[test]
    fn valid_file_is_read_with_its_diagnostics() {
        let root = TempRoot::new("valid");
        std::fs::write(root.0.join(FILE_NAME), "[terminal]\nscrollback = 2500\n")
            .expect("yazılamadı");
        assert_eq!(
            load(&root.0).at_launch(),
            (
                Settings {
                    scrollback: 2500,
                    ..Settings::default()
                },
                Vec::new()
            )
        );

        std::fs::write(root.0.join(FILE_NAME), "[terminal]\nscrollback = true\n")
            .expect("yazılamadı");
        let (settings, notices) = load(&root.0).at_launch();
        assert_eq!(settings, Settings::default());
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("settings.toml: line 2: `terminal.scrollback`"),
            "{notices:?}"
        );
    }

    /// Tema sınamalarının görünümü: koyu. Yedeği görünüme bağlı olan sınama
    /// ikisini de açıkça veriyor.
    const DARK: bool = true;

    /// Kökün altına `themes/{name}.toml` yazar.
    fn write_theme_file(root: &TempRoot, name: &str, text: &str) {
        let dir = root.0.join(THEMES_DIR);
        std::fs::create_dir_all(&dir).expect("tema dizini kurulamadı");
        std::fs::write(dir.join(format!("{name}.toml")), text).expect("yazılamadı");
    }

    #[test]
    fn embedded_theme_without_user_file() {
        let root = TempRoot::new("theme-embedded");
        let (theme, notices) = load_theme(Some(&root.0), "bateri").or_embedded(DARK);
        assert_eq!((theme, notices), (Theme::BATERI, Vec::new()));
        // Ev dizini yoksa da gömülüler çözülür.
        assert_eq!(
            load_theme(None, "bateri").or_embedded(DARK),
            (Theme::BATERI, Vec::new())
        );
    }

    #[test]
    fn user_theme_shadows_the_embedded_one() {
        let root = TempRoot::new("theme-shadow");
        write_theme_file(&root, "bateri", "background = \"#ffffff\"\n");
        let (theme, notices) = load_theme(Some(&root.0), "bateri").or_embedded(DARK);
        assert_eq!(notices, Vec::<String>::new());
        assert_eq!(
            theme,
            Theme {
                background: 0xffffff,
                ..Theme::BATERI
            }
        );

        // Açık gömülü temayı gölgeleyen dosyanın tabanı açık tema
        // (`/code-review` bulgusu): yalnız imleci değiştiren kullanıcı açık
        // modda koyu zemin almamalı. Gölgelemeyen adın tabanı `bateri` kalır.
        write_theme_file(&root, "bateri-light", "accent = \"#ff0000\"\n");
        write_theme_file(&root, "paper", "accent = \"#ff0000\"\n");
        let light = load_theme(Some(&root.0), "bateri-light").or_embedded(false);
        assert_eq!(
            light,
            (
                Theme {
                    accent: 0xff0000,
                    ..Theme::BATERI_LIGHT
                },
                Vec::new()
            )
        );
        let paper = load_theme(Some(&root.0), "paper").or_embedded(false);
        assert_eq!(
            paper.0,
            Theme {
                accent: 0xff0000,
                ..Theme::BATERI
            }
        );
    }

    #[test]
    fn empty_user_theme_is_unusable() {
        // Yerinde kaydeden editör dosyayı önce boşaltıyor: kaydın ortasında
        // okunan boş tema koyu tabanın kendisi olur ve canlı yenileme
        // pencereyi ona çakardı (`/code-review` bulgusu). `settings.toml`'un
        // kuralı: boş dosya kullanılamaz.
        let root = TempRoot::new("theme-empty");
        write_theme_file(&root, "paper", " \n");
        assert_eq!(
            load_theme(Some(&root.0), "paper").or_current(),
            (
                None,
                vec!["themes/paper.toml is empty; keeping the current theme".to_owned()]
            )
        );
        // Açılışta görünüme uyan gömülü tema; gölgelenen adın gömülüsüne
        // sessizce düşülmez (bozuk dosyanın kuralı).
        write_theme_file(&root, "bateri", "");
        let (theme, notices) = load_theme(Some(&root.0), "bateri").or_embedded(false);
        assert_eq!(theme, Theme::BATERI_LIGHT);
        assert_eq!(notices, ["themes/bateri.toml is empty; using bateri-light"]);
    }

    #[test]
    fn user_theme_reports_its_bad_colors_with_the_file_name() {
        let root = TempRoot::new("theme-diagnostics");
        write_theme_file(
            &root,
            "paper",
            "[ansi]\nred = \"red\"\nblue = \"#0000ff\"\n",
        );
        let (theme, notices) = load_theme(Some(&root.0), "paper").or_embedded(DARK);
        assert_eq!(theme.ansi[4], 0x0000ff);
        assert_eq!(theme.ansi[1], Theme::BATERI.ansi[1]);
        assert_eq!(
            notices,
            [
                "themes/paper.toml: line 2: `ansi.red` must be a color like \"#rrggbb\", found \"red\"; using #d16d6a"
            ]
        );
    }

    #[test]
    fn missing_theme_falls_back_to_bateri_with_notice() {
        let root = TempRoot::new("theme-missing");
        let (theme, notices) = load_theme(Some(&root.0), "paper").or_embedded(DARK);
        assert_eq!(theme, Theme::BATERI);
        assert_eq!(notices, ["theme \"paper\" not found; using bateri"]);
    }

    #[test]
    fn fallback_follows_the_appearance() {
        // Açık modda bulunamayan tema koyu bir pencere açmıyor: yedek,
        // dosyasız kullanıcının o görünümde göreceği gömülü tema.
        let root = TempRoot::new("theme-fallback-light");
        let (theme, notices) = load_theme(Some(&root.0), "paper").or_embedded(false);
        assert_eq!(theme, Theme::BATERI_LIGHT);
        assert_eq!(notices, ["theme \"paper\" not found; using bateri-light"]);
        assert_eq!(
            load_theme(Some(&root.0), "bateri-light").or_embedded(false),
            (Theme::BATERI_LIGHT, Vec::new())
        );
    }

    #[test]
    fn appearance_switches_never_leave_the_other_appearances_theme() {
        // `/code-review`'un senaryosu: `dark_theme` bulunamıyor, pencere
        // koyu → açık → koyu gidip geliyor. Her geçiş temayı o görünüm için
        // yeniden seçiyor; bozuk koyu tema açık temayı ekranda **bırakmıyor**,
        // gömülü koyuya düşüyor. Açık geçiş tema yuvasını boşaltıyor.
        let root = TempRoot::new("theme-switches");
        let settings = Settings {
            dark_theme: "ink".to_owned(),
            ..Settings::default()
        };
        let pick = |dark| load_theme(Some(&root.0), settings.theme_for(dark)).or_embedded(dark);
        for _ in 0..2 {
            assert_eq!(
                pick(true),
                (
                    Theme::BATERI,
                    vec!["theme \"ink\" not found; using bateri".to_owned()]
                )
            );
            assert_eq!(pick(false), (Theme::BATERI_LIGHT, Vec::new()));
        }
    }

    #[test]
    fn broken_user_theme_does_not_fall_to_the_embedded_one() {
        // Aynı adlı gömülü tema var, ama bozuk dosya onu açmıyor: sonuç yine
        // `bateri` olsa da **ileti** dosyayı söylüyor. Gömülü adı olmayan bir
        // temayla aynı kural (ikinci yarı).
        let root = TempRoot::new("theme-broken");
        write_theme_file(&root, "bateri", "background = \"#ffffff\n");
        let loaded = load_theme(Some(&root.0), "bateri");
        assert!(matches!(loaded, ThemeLoaded::Failed(_)), "{loaded:?}");
        let (theme, notices) = loaded.or_embedded(DARK);
        assert_eq!(theme, Theme::BATERI);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("themes/bateri.toml: line 1: invalid TOML: ")
                && notices[0].ends_with("; using bateri"),
            "{notices:?}"
        );

        std::fs::create_dir_all(root.0.join(THEMES_DIR).join("paper.toml"))
            .expect("dizin kurulamadı");
        assert_eq!(
            load_theme(Some(&root.0), "paper").or_embedded(DARK).1,
            ["themes/paper.toml could not be read: not a regular file; using bateri"]
        );
    }

    #[test]
    fn live_reload_applies_nothing_from_a_broken_or_missing_file() {
        let root = TempRoot::new("live");
        // Dosya yok (editörün kaydı arasında da): uygulanacak bir şey yok ve
        // bu bir hata değil.
        assert_eq!(load(&root.0).live(), (None, Vec::new()));

        // Yarım kayıt: hiçbir şey uygulanmıyor, yuva satırı söylüyor.
        std::fs::write(root.0.join(FILE_NAME), "[appearance]\ntheme = \"paper\n")
            .expect("yazılamadı");
        let (settings, notices) = load(&root.0).live();
        assert_eq!(settings, None);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("settings.toml: line 2: invalid TOML: "),
            "{notices:?}"
        );

        // Okunamayan dosya da (bağın hedefi taşındı) hiçbir şey uygulamıyor.
        std::fs::remove_file(root.0.join(FILE_NAME)).expect("silinemedi");
        std::os::unix::fs::symlink(root.0.join("moved.toml"), root.0.join(FILE_NAME))
            .expect("bağ kurulamadı");
        assert_eq!(
            load(&root.0).live(),
            (
                None,
                vec![
                    "settings.toml could not be read: symbolic link points to a missing file"
                        .to_owned()
                ]
            )
        );

        // Düzeltilen kayıt tanılarıyla uygulanıyor.
        std::fs::remove_file(root.0.join(FILE_NAME)).expect("silinemedi");
        std::fs::write(
            root.0.join(FILE_NAME),
            "[terminal]\nscrollback = 5\n[appearance]\ntheme = 3\n",
        )
        .expect("yazılamadı");
        let (settings, notices) = load(&root.0).live();
        assert_eq!(settings.map(|s| s.scrollback), Some(5));
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("settings.toml: line 4: `appearance.theme`"),
            "{notices:?}"
        );
    }

    #[test]
    fn live_reload_keeps_current_values_for_rejected_keys() {
        // Yüz binlik geçmiş açıkken `scrollback` yanlış türde kaydedildi:
        // kayıt anında değer geçerli ayardan geliyor, fark boş kalıyor ve
        // geçmiş kırpılmıyor. Açılıştaki okuma aynı dosyada varsayılana düşer.
        let root = TempRoot::new("live-rejected");
        std::fs::write(
            root.0.join(FILE_NAME),
            "[terminal]\nscrollback = \"100000\"\n",
        )
        .expect("yazılamadı");
        let current = Settings {
            scrollback: 100_000,
            ..Settings::default()
        };
        let (settings, notices) = load_keeping(&root.0, &current).live();
        let settings = settings.expect("ayrıştırılan dosya uygulanır");
        assert_eq!(current.changes(&settings), bt_core::Changes::default());
        assert_eq!(
            notices,
            [
                "settings.toml: line 2: `terminal.scrollback` must be an integer, found a string; using 100000"
            ]
        );
        assert_eq!(load(&root.0).at_launch().0.scrollback, 10_000);
    }

    #[test]
    fn live_reload_applies_nothing_from_an_empty_file() {
        // Yerinde kaydeden editör dosyayı önce boşaltıyor (`O_TRUNC`), sonra
        // yazıyor; boşaltma olay veriyor. Arada okunan boş dosya varsayılan
        // `scrollback`'i uygulasaydı geçmişin fazlası geri dönülmez silinirdi.
        let root = TempRoot::new("live-empty");
        for text in ["", "\n  \n"] {
            std::fs::write(root.0.join(FILE_NAME), text).expect("yazılamadı");
            let loaded = load_keeping(&root.0, &Settings::default());
            assert!(matches!(loaded, Loaded::Missing), "{text:?}: {loaded:?}");
            assert_eq!(loaded.live(), (None, Vec::new()), "{text:?}");
            // Açılışta boş dosya dosyasızlıkla zaten aynıydı.
            assert_eq!(
                load(&root.0).at_launch(),
                (Settings::default(), Vec::new()),
                "{text:?}"
            );
        }
    }

    #[test]
    fn live_reload_keeps_the_current_theme_when_unusable() {
        // Canlı yenilemede kullanılamayan tema takas edilmiyor: yarım
        // kaydedilmiş tema dosyası gömülü temaya çakmıyor.
        let root = TempRoot::new("live-theme");
        write_theme_file(&root, "paper", "background = \"#ffffff\n");
        let (theme, notices) = load_theme(Some(&root.0), "paper").or_current();
        assert_eq!(theme, None);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].starts_with("themes/paper.toml: line 1: invalid TOML: ")
                && notices[0].ends_with("; keeping the current theme"),
            "{notices:?}"
        );
        assert_eq!(
            load_theme(Some(&root.0), "ink").or_current(),
            (
                None,
                vec!["theme \"ink\" not found; keeping the current theme".to_owned()]
            )
        );

        // Düzeltilen dosya takas ediliyor, yuva boşalıyor.
        write_theme_file(&root, "paper", "background = \"#ffffff\"\n");
        assert_eq!(
            load_theme(Some(&root.0), "paper").or_current(),
            (
                Some(Theme {
                    background: 0xffffff,
                    ..Theme::BATERI
                }),
                Vec::new()
            )
        );
    }

    #[test]
    fn watched_paths_follow_the_layout() {
        // İzleyicinin yolları okuyucunun okuduğu yollarla aynı: biri
        // değişip öteki kalırsa kaydı hiçbir kaynak görmez ve belirti
        // sessizdir.
        let root = Path::new("/r");
        assert_eq!(
            watched_paths(root),
            [
                PathBuf::from("/r"),
                PathBuf::from("/r/themes"),
                PathBuf::from("/r/settings.toml")
            ]
        );
        assert_eq!(
            theme_path(root, "paper"),
            PathBuf::from("/r/themes/paper.toml")
        );
    }
}
