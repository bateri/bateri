//! Paket girdilerinin içerik denetimi (006 phase-4).
//!
//! Ayrı bir `tests/` sınaması değil, bin'in birim test demetinde: o demet
//! `make hepsi`'de zaten bağlanıyor. `tests/` altında dursaydı cargo her
//! koşuda uygulama binary'sini de ayrıca bağlardı (`CARGO_BIN_EXE_*`).
//!
//! Neden var: `alacritty_terminal` Apache-2.0 ve lisans metni `.app` ile
//! birlikte gitmek zorunda. Metin ya da atıf silinirse hiçbir derleme,
//! clippy ya da duman koşusu kızarmaz — ihlal **sessiz** olur. Bu sınama
//! `make hepsi`'de koşar ve girdileri (`assets/bundle/`, `assets/shell/`)
//! denetler.
//!
//! Kapsamadığı: girdilerin pakete **kopyalanıp kopyalanmadığı**. Ürün yalnız
//! `make kur`'da doğuyor ve onu `kur`'un kendi denetimi görüyor; burada
//! tekrarlanmıyor, çünkü sınamanın ürünü kurması için release derlemesi
//! gerekirdi.
//!
//! `plutil` bir macOS aracı; bu crate de zaten yalnız macOS'ta derleniyor
//! (`bt-shell` → AppKit).

use std::path::{Path, PathBuf};
use std::process::Command;

fn asset(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/bundle")
        .join(name)
}

/// zsh sarmalayıcısının girdi dizini (`assets/shell/zsh`).
fn shell_asset_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell/zsh")
}

fn read_asset(name: &str) -> String {
    let path = asset(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} okunamadı: {e}", path.display()))
}

/// Şablondan tek anahtarın ham değeri; anahtar yoksa `None`.
fn plist_value(key: &str) -> Option<String> {
    let out = Command::new("plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(asset("Info.plist.in"))
        .output()
        .expect("plutil çalıştırılamadı");
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

#[test]
fn info_plist_template_launches_the_binary() {
    let lint = Command::new("plutil")
        .arg("-lint")
        .arg(asset("Info.plist.in"))
        .output()
        .expect("plutil çalıştırılamadı");
    assert!(
        lint.status.success(),
        "Info.plist.in geçerli bir plist değil: {}{}",
        String::from_utf8_lossy(&lint.stdout),
        String::from_utf8_lossy(&lint.stderr)
    );
    // Çalıştırılabilir adı bin hedefinden okunuyor, elle yazılmıyor: ikisi
    // ayrışırsa LaunchServices paketi açamaz ve belirti Finder'da "uygulama
    // açılamıyor" iletisidir, derleme değil.
    assert_eq!(
        plist_value("CFBundleExecutable").as_deref(),
        Some(env!("CARGO_BIN_NAME"))
    );
    assert_eq!(plist_value("CFBundlePackageType").as_deref(), Some("APPL"));
    assert!(
        plist_value("CFBundleIdentifier").is_some_and(|id| !id.is_empty()),
        "CFBundleIdentifier yok"
    );
    // GPU'nun çizdiği bir terminal Retina'da bulanık açılmasın.
    assert_eq!(
        plist_value("NSHighResolutionCapable").as_deref(),
        Some("true")
    );
    let icon = plist_value("CFBundleIconFile").expect("CFBundleIconFile yok");
    assert!(
        asset(&format!("{icon}.png")).is_file(),
        "ikon kaynağı yok: assets/bundle/{icon}.png"
    );
}

/// Sürüm ve taban macOS şablona **yazılmaz**: `make kur` sürümü `Cargo.toml`'dan,
/// tabanı binary'nin `minos`'undan (o da `.cargo/config.toml`'dan) doldurur.
/// Şablonda düz bir `14.0` görmek, taban yükseldiğinde paketin eski sayıyla
/// kalacağı demek. Yer tutucuların şablon içinde bir açıklaması yok, çünkü
/// `sed` açıklamayı da doldurup ürüne sızdırırdı; açıklama `Makefile`'ın
/// `kur` yorumunda.
#[test]
fn info_plist_template_derives_version_and_minimum_os() {
    assert_eq!(
        plist_value("LSMinimumSystemVersion").as_deref(),
        Some("@MACOS_MIN@")
    );
    assert_eq!(
        plist_value("CFBundleShortVersionString").as_deref(),
        Some("@VERSION@")
    );
    assert_eq!(plist_value("CFBundleVersion").as_deref(), Some("@VERSION@"));
}

/// Sparkle'ın üç anahtarı. Besleme bir yer tutucu, çünkü `make kur` onu
/// `FEED_URL`'den dolduruyor (denemede başka bir adrese ezilebilsin); açık
/// anahtar ise sabit — değişirse kurulu kopyalar yeni sürümlerin imzasını
/// reddeder, yani onu değiştiren bir diff bu sınamayı da değiştirmek
/// zorunda kalsın. Paket kimliği de Sparkle'ın ölçüsü: güncelleme ancak aynı
/// `CFBundleIdentifier`'a kuruluyor.
#[test]
fn info_plist_template_carries_the_updater_keys() {
    assert_eq!(plist_value("SUFeedURL").as_deref(), Some("@FEED_URL@"));
    assert_eq!(
        plist_value("SUPublicEDKey").as_deref(),
        Some("WC9PPr7SL5v2LvmQShrOYVEawDoB5wWngrinpVZE6Tw=")
    );
    assert_eq!(
        plist_value("SUEnableAutomaticChecks").as_deref(),
        Some("true")
    );
    assert_eq!(
        plist_value("CFBundleIdentifier").as_deref(),
        Some("dev.bateri.bateri")
    );
}

/// zsh sarmalayıcısının envanteri **tam olarak** bu beş dosya.
///
/// "Eksiği yok" yarısını `bt-shell` de soruyor (`child::zsh_wrapper_dir`'in
/// sınaması); buranın tek başına gördüğü yarı **fazlası**. İki türü var ve
/// ikisi de sessiz:
///
/// - `make kur` betikleri elle yazılmış iki listeden geçiriyor (kopya ve
///   `cmp`). Listelere düşmemiş yeni bir girdi pakete hiç girmez, ürün
///   denetimi de onu aramaz — kapı yeşil kalır, sarmalayıcı eksik kurulur.
/// - Dizine **yazan** bir kol: ZDOTDIR oturum boyunca bir süre burayı
///   gösteriyor ve 009 phase-3'te `/etc/zshrc` bir kez gerçekten
///   `.zsh_history` doğurdu. Kullanıcının verisi ürüne girecek yoldu.
///
/// `.DS_Store` sayılmıyor: Finder üretiyor, depoya girmiyor (`.gitignore`) ve
/// kopya satırı adları tek tek saydığı için pakete sızamıyor. Kapının kod
/// doğruyken düşmesi, gördüğü kusurdan pahalı olurdu.
#[test]
fn zsh_wrapper_inventory_is_exactly_what_the_bundle_copies() {
    let dir = shell_asset_dir();
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{} okunamadı: {e}", dir.display()))
        .map(|entry| {
            entry
                .expect("dizin girdisi okunamadı")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name != ".DS_Store")
        .collect();
    found.sort();
    assert_eq!(
        found,
        [".zlogin", ".zprofile", ".zshenv", ".zshrc", "bateri.zsh"],
        "assets/shell/zsh envanteri değişti; `make kur`'un kopya ve cmp \
         listeleri de güncellenmeli"
    );
}

/// GPL-3.0 §4: binary'yi alan herkese lisansın bir kopyası verilir. Metin
/// depo kökündeki `LICENSE` (gnu.org'un metni, `make kur` onu pakete
/// kopyalayıp `cmp`'liyor), About paneli (`Credits.html`) lisansı ve
/// kaynağın yerini söylüyor ve manifestin SPDX'i aynı lisans — üçünden biri
/// ayrışırsa hiçbir derleme kızarmaz, ihlal sessiz olur.
#[test]
fn own_license_ships_with_notice() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let license = std::fs::read_to_string(root.join("LICENSE")).expect("LICENSE okunamadı");
    assert!(
        license.starts_with("                    GNU GENERAL PUBLIC LICENSE\n                       Version 3, 29 June 2007"),
        "LICENSE GPL-3.0'ın gnu.org metni değil"
    );
    let credits = read_asset("Credits.html");
    for needle in [
        "GNU General Public License, version 3",
        "or any later version",
        "github.com/bateri/bateri",
        "Contents/Resources/LICENSE",
    ] {
        assert!(
            credits.contains(needle),
            "Credits.html içinde {needle:?} yok"
        );
    }
    assert_eq!(env!("CARGO_PKG_LICENSE"), "GPL-3.0-or-later");
}

/// Apache-2.0 §4(a) ve MIT: alıcıya lisansın bir kopyası verilir (dosya
/// `tools/third_party_notices.py`'nin çıktısı). Atıf metni
/// (`Credits.html`) AppKit'in standart About panelinin okuduğu dosya.
#[test]
fn third_party_license_ships_with_attribution() {
    let licenses = read_asset("THIRD-PARTY-LICENSES.txt");
    for needle in [
        "alacritty_terminal",
        "Apache License",
        "Version 2.0",
        "MIT License",
        "Sparkle",
    ] {
        assert!(
            licenses.contains(needle),
            "THIRD-PARTY-LICENSES.txt içinde {needle:?} yok"
        );
    }
    let credits = read_asset("Credits.html");
    for needle in [
        "alacritty_terminal",
        "Apache License",
        "Sparkle",
        "MIT License",
        "THIRD-PARTY-LICENSES.txt",
    ] {
        assert!(
            credits.contains(needle),
            "Credits.html içinde {needle:?} yok"
        );
    }
}
