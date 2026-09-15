//! Ayar dosyasının yükleyicisi: kök dizindeki `settings.toml`'u okur ve
//! `bt-core`'un saf ayrıştırıcısına verir.
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

use bt_core::{Diagnostic, Parsed, Settings};

/// Ayar dosyasının adı; tanı metinleri de kullanıcıya bu adla söylüyor.
pub(crate) const FILE_NAME: &str = "settings.toml";

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

/// `{root}/settings.toml`'u okur.
pub(crate) fn load(root: &Path) -> Loaded {
    let path = root.join(FILE_NAME);
    // `metadata` bağı izliyor: hedefin türü soruluyor, bağın değil.
    match std::fs::metadata(&path) {
        // Kırık bağ da `NotFound` veriyor; ama dosya `ls`'te görünüyor ve
        // "hiç ayar yok" diye susmak kullanıcıyı neden işlemediğini aramaya
        // bırakırdı (dotfile yöneticisinin taşınmış deposu).
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return match std::fs::symlink_metadata(&path) {
                Ok(_) => {
                    Loaded::Unreadable(io::Error::other("symbolic link points to a missing file"))
                }
                Err(_) => Loaded::Missing,
            };
        }
        Err(err) => return Loaded::Unreadable(err),
        // FIFO okumayı sonsuza dek bekletir, `/dev/zero` belleği bitirir.
        Ok(meta) if !meta.is_file() => {
            return Loaded::Unreadable(io::Error::other("not a regular file"));
        }
        Ok(_) => {}
    }
    match std::fs::read_to_string(&path) {
        // Denetimle okuma arasında silindi: dosya yok hâli.
        Err(err) if err.kind() == io::ErrorKind::NotFound => Loaded::Missing,
        Err(err) => Loaded::Unreadable(err),
        Ok(text) => match Settings::parse(&text) {
            Ok(parsed) => Loaded::Parsed(parsed),
            Err(diagnostic) => Loaded::Unparseable(diagnostic),
        },
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
        assert_eq!(load(&root.0).at_launch().0, Settings { scrollback: 7 });
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
            (Settings { scrollback: 2500 }, Vec::new())
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
}
