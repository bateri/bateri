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
//! yenilemede hiçbir şey uygulamamayı (007 phase-4) ve açılışta OSC 52'yi
//! kapatmayı (phase-8) gerektiriyor. Hangi hâlde ne yapılacağı çağıranın
//! kuralı; açılışın kuralı [`Loaded::at_launch`].
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

/// Bir okumanın sonucu.
#[derive(Debug)]
pub(crate) enum Loaded {
    /// Dosya yok: kullanıcı hiç ayar yazmamış. Tanı **yok**.
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

/// `{root}/settings.toml`'u okur.
pub(crate) fn load(root: &Path) -> Loaded {
    match read_text(&root.join(FILE_NAME)) {
        Text::Missing => Loaded::Missing,
        Text::Unreadable(err) => Loaded::Unreadable(err),
        Text::Read(text) => match Settings::parse(&text) {
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
/// `root` `None` → ev dizini çözülemedi, yalnız gömülüler. Dosya
/// `Theme::BATERI`'nin üstüne okunur: eksik anahtar oradan gelir
/// (`docs/AYARLAR.md` → Temalar).
///
/// **Dosya var ama kullanılamıyorsa gömülüye düşülmez**: bozuk bir
/// `themes/bateri.toml` sessizce gömülü `bateri`'yi açsaydı kullanıcı
/// dosyasının neden işlemediğini göremezdi. Dosya **yoksa** gömülü aranır —
/// gölgelenmemiş ad budur.
///
/// Adın biçimi (`/` yok, boş değil) `bt-core`'da zaten sınandı; burada yeniden
/// sınanmıyor, `Settings`'ten gelmeyen bir ad bu fonksiyona hiç verilmiyor.
pub(crate) fn load_theme(root: Option<&Path>, name: &str) -> ThemeLoaded {
    if let Some(root) = root {
        let file = format!("{THEMES_DIR}/{name}.toml");
        match read_text(&root.join(&file)) {
            Text::Missing => {}
            Text::Unreadable(err) => {
                return ThemeLoaded::Failed(format!("{file} could not be read: {err}"));
            }
            Text::Read(text) => {
                return match Theme::parse(&text, &Theme::BATERI) {
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
    /// açıktan koyuya dönen pencere açık kalırdı). "Önceki tema kalır" kuralı
    /// görünüm aynıyken tema dosyasının bozulduğu canlı yenilemenin (phase-4).
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
}

/// Bir tanının alt başlıktaki ve stderr'deki biçimi; iki kolun ve canlı
/// yenilemenin (phase-4) aynı biçimi kullanması için tek yerde.
fn notice(diagnostic: &Diagnostic) -> String {
    format!("{FILE_NAME}: {diagnostic}")
}

impl Loaded {
    /// Açılışın kuralı: kullanılacak ayarlar ve alt başlığa gidecek tanılar.
    ///
    /// Okunamayan ya da ayrıştırılamayan dosyada **varsayılanlar**: pencere
    /// yine açılmalı, bozuk bir dosya terminali kilitlememeli. Ayrıştırılan
    /// dosyada her anahtar kendi değerini ya da varsayılanını zaten aldı.
    pub(crate) fn at_launch(self) -> (Settings, Vec<String>) {
        match self {
            Loaded::Missing => (Settings::default(), Vec::new()),
            Loaded::Unreadable(err) => (
                Settings::default(),
                vec![format!("{FILE_NAME} could not be read: {err}")],
            ),
            Loaded::Unparseable(diagnostic) => (Settings::default(), vec![notice(&diagnostic)]),
            Loaded::Parsed(parsed) => (
                parsed.settings,
                parsed.diagnostics.iter().map(notice).collect(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sınamaya özel geçici kök; süreç kimliği paralel koşan iki `cargo
    /// test`'i ayırıyor. `tempfile` bir bağımlılık kararı olurdu.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("bateri-settings-{}-{name}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("geçici kök kurulamadı");
            Self(path)
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn config_root_is_under_dot_config() {
        assert_eq!(
            config_root(Path::new("/Users/someone")),
            PathBuf::from("/Users/someone/.config/bateri")
        );
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
        assert_eq!(settings, Settings::default());
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
        assert_eq!(settings, Settings::default());
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
        assert_eq!(settings, Settings::default());
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
    fn write_theme(root: &TempRoot, name: &str, text: &str) {
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
        write_theme(&root, "bateri", "background = \"#ffffff\"\n");
        let (theme, notices) = load_theme(Some(&root.0), "bateri").or_embedded(DARK);
        assert_eq!(notices, Vec::<String>::new());
        assert_eq!(
            theme,
            Theme {
                background: 0xffffff,
                ..Theme::BATERI
            }
        );
    }

    #[test]
    fn user_theme_reports_its_bad_colors_with_the_file_name() {
        let root = TempRoot::new("theme-diagnostics");
        write_theme(
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
        write_theme(&root, "bateri", "background = \"#ffffff\n");
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
}
