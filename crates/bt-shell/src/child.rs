//! Kabuğun başlangıç koşulları: hangi dizinde ve hangi yerelle açılır.
//!
//! Dock'tan açılan paket LaunchServices'ten `cwd=/` ve launchd'nin ortamını
//! alıyor; launchd'nin kullanıcı alanında `LANG` yok (`launchctl getenv
//! LANG` boş). Kabuk olduğu gibi miras alsaydı `/`'da ve UTF-8'siz başlardı.
//! `cargo run` ikisini de göstermez: çağıranın dizinini ve ortamını miras
//! alır.
//!
//! Politika **burada**, `bt-core`'da değil: "ev dizini" ve "hangi yerel"
//! uygulamanın kararı, `bt-core` yalnız verileni çocuğa geçirir
//! (`SessionOptions`). İkisi de **yalnız çocuğa** gider — kendi sürecimizin
//! dizini ve ortamı hiçbir hâlde değişmez (`set_current_dir`, `set_var`,
//! `setlocale` yok; `CLAUDE.md` → `tty::setup_env()` çağrılmaz). alacritty
//! aynı iki işi kendi sürecinde yapıyor; izlenmemesinin sebebi bu.
//!
//! Kararlar saf fonksiyonlarda ([`home_directory`], [`decide_locale`]):
//! sistemin okunduğu tek yer iki ince sarmalayıcı, geri kalanı sınanıyor.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use objc2_foundation::NSLocale;

/// Kabuğun başlangıç dizini: kullanıcının ev dizini — **her** açılışta,
/// `cargo run` dahil (alacritty ve Terminal.app ile aynı). "Yalnız `/`
/// gelirse ev dizini" reddedildi: kural iki kollu olurdu
/// (`discussion.md` → Karar 6 eki).
///
/// Kaynak `std::env::home_dir`: `HOME`, o yoksa kullanıcının passwd kaydı
/// (`getpwuid_r`) — alacritty'nin çocuğa `HOME` yazarken izlediği sıra
/// (`ShellUser::from_env`), yani olağan hâlde kabuğun `pwd`'si ile `$HOME`'u
/// aynı dizin. Yeni bağımlılık istemiyor. İkisinin ayrışabildiği tek kenar
/// UTF-8 olmayan bir `HOME`: `std` onu olduğu gibi alıyor, alacritty
/// (`env::var`) passwd'ye düşüyor.
pub(crate) fn working_directory() -> Option<PathBuf> {
    home_directory(std::env::home_dir())
}

/// Ev dizini → başlangıç dizini; **mutlak değilse** `None`, yani çocuk bizim
/// dizinimizi miras alır.
///
/// İki kenar tek koşulda: `HOME=""` passwd'ye düşürmüyor, `std` `Some("")`
/// veriyor; göreli bir `HOME` ise `chdir`'de **bizim** dizinimize (Dock'ta
/// `/`) göre çözülürdü. İkisini de `chdir`'e vermenin anlamı yok; miras
/// dürüst olan.
fn home_directory(home: Option<PathBuf>) -> Option<PathBuf> {
    home.filter(|home| home.is_absolute())
}

/// Kabuğa eklenecek yerel değişkeni, gerekiyorsa: ortamda yerel yoksa
/// macOS'un dil/bölge ayarından.
///
/// **Dil `preferredLanguages`'ın ilkinden**, `currentLocale().languageCode`
/// değil: paketin içinde o, **kullanıcının** dilini değil paketin
/// yerelleştirmesinden seçilen dili veriyor — `bateri.app` `.lproj`
/// taşımıyor ve `CFBundleDevelopmentRegion` `en`, yani her Dock açılışında
/// `en`. Paketle yoklandı (`-AppleLanguages (tr-TR) -AppleLocale tr_TR`):
/// Türk kullanıcı `tr_TR.UTF-8` yerine `LC_CTYPE=UTF-8`, `fr-CA` kullanıcı
/// `fr_CA.UTF-8` yerine `en_CA.UTF-8` alıyordu. `cargo run` (paketsiz)
/// doğru dili verdiği için hata orada görünmüyordu. alacritty
/// `currentLocale`'i kullanıyor; izlenmedi.
///
/// **Bölge `currentLocale().regionCode`'dan** (paket onu etkilemiyor),
/// `countryCode`'dan değil: SDK ikincisini `regionCode` lehine kalkacak diye
/// işaretliyor (objc2'de `#[deprecated]`, `-D warnings` altında hata).
/// `regionCode` macOS 14'te geldi; taban zaten 14. Farkı `@rg=` alt
/// etiketi: kullanıcı bölge biçimini ayrıca seçtiyse (`en_US@rg=gbzzzz`) o
/// bölgeyi veriyor.
pub(crate) fn locale_env() -> Option<(String, String)> {
    let language = NSLocale::preferredLanguages().firstObject();
    let region = NSLocale::currentLocale().regionCode();
    let system = language.zip(region).and_then(|(tag, region)| {
        let language = primary_language(&tag.to_string())?.to_owned();
        Some((language, region.to_string()))
    });
    decide_locale(|name| std::env::var_os(name), system, locale_installed)
}

/// Yerel kararı: `env` ortamı okur, `system` sistemin `(dil, bölge)` kodu,
/// `installed` "bu adda yerel kurulu mu" sorusu.
///
/// 1. `LC_ALL`, `LC_CTYPE` ya da `LANG`'dan **biri** boş olmayan bir değerle
///    tanımlıysa → hiçbir şey; kullanıcının ortamına dokunulmaz (`cargo run`
///    bu yoldan geçer). Boş değer tanımsız sayılır: POSIX'te de öyle.
///    Değerin geçerliliği sorulmuyor — `LANG=C` de dokunulmaz.
/// 2. Değilse ve `{dil}_{bölge}.UTF-8` kuruluysa → `LANG` o ad.
/// 3. Değilse (bölge yok, ya da ör. İngilizce dil + Türkiye bölgesi →
///    `en_TR.UTF-8` yok) → `LC_CTYPE=UTF-8`: mesajlar İngilizce kalır ama
///    UTF-8 girişi çalışır. alacritty'nin düşüşüyle aynı.
///
/// **`LC_ALL` değil `LANG`**, en zayıf değişken (Terminal.app'in yaptığı):
/// kabuğun rc dosyası kendi `LC_*`'ını üstüne yazabilsin. alacritty
/// `LC_ALL` yazıyor ve o, rc'deki her `LC_*`'ı ezer. Düşüş kolunda bu söz
/// **daralıyor**: `LC_CTYPE`, `LANG`'dan güçlü, yani rc'de yalnız `LANG`
/// değiştiren kullanıcının karakter sınıfı `UTF-8` kalır — değiştirmek için
/// `LC_CTYPE` ya da `LC_ALL` gerekir.
///
/// **Bilinen bedel (düşüş kolu):** `LC_CTYPE=UTF-8` macOS'ta geçerli ama
/// Linux'ta yerel adı değil, ve macOS'un `ssh_config`'i `LC_*`'ı uzak
/// makineye taşıyor (`SendEnv LANG LC_*`): oradaki araçlar `setlocale`
/// uyarısı basıp `C`'ye düşer. Düşüşün kendisi kullanıcı kararı
/// (`discussion.md` → Karar 6 eki).
fn decide_locale(
    env: impl Fn(&str) -> Option<OsString>,
    system: Option<(String, String)>,
    installed: impl Fn(&str) -> bool,
) -> Option<(String, String)> {
    let defined = ["LC_ALL", "LC_CTYPE", "LANG"]
        .into_iter()
        .any(|name| env(name).is_some_and(|value| !value.is_empty()));
    if defined {
        return None;
    }
    Some(
        match system.map(|(language, region)| format!("{language}_{region}.UTF-8")) {
            Some(name) if installed(&name) => ("LANG".to_owned(), name),
            _ => ("LC_CTYPE".to_owned(), "UTF-8".to_owned()),
        },
    )
}

/// Yerel kurulu mu: `/usr/share/locale/{ad}` dizini var mı.
///
/// `setlocale` ile **sınanmıyor**: o, kendi sürecimizin global yerelini
/// değiştirir. `/` taşıyan ad reddediliyor, dizinin dışına çıkamasın.
fn locale_installed(name: &str) -> bool {
    !name.contains('/') && Path::new("/usr/share/locale").join(name).is_dir()
}

/// BCP 47 dil etiketinin dil alt etiketi: `tr-TR` → `tr`, `zh-Hans-CN` →
/// `zh`. `preferredLanguages` `-` ile veriyor; `_` eski yerel adları için.
fn primary_language(tag: &str) -> Option<&str> {
    tag.split(['-', '_'])
        .next()
        .filter(|language| !language.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    /// Ortamı sabit bir listeden okuyan okuyucu; listede olmayan tanımsız.
    fn env_of(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let vars: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| ((*k).to_owned(), OsString::from(v)))
            .collect();
        move |name| vars.get(name).cloned()
    }

    fn system(language: &str, region: &str) -> Option<(String, String)> {
        Some((language.to_owned(), region.to_owned()))
    }

    fn pair(key: &str, value: &str) -> Option<(String, String)> {
        Some((key.to_owned(), value.to_owned()))
    }

    #[test]
    fn home_directory_is_home_unless_missing_or_empty() {
        assert_eq!(
            home_directory(Some("/Users/someone".into())),
            Some(PathBuf::from("/Users/someone"))
        );
        assert_eq!(home_directory(Some(PathBuf::new())), None);
        assert_eq!(home_directory(Some("relative/home".into())), None);
        assert_eq!(home_directory(None), None);
    }

    #[test]
    fn primary_language_is_the_first_subtag() {
        assert_eq!(primary_language("tr-TR"), Some("tr"));
        assert_eq!(primary_language("zh-Hans-CN"), Some("zh"));
        assert_eq!(primary_language("en"), Some("en"));
        assert_eq!(primary_language("pt_BR"), Some("pt"));
        assert_eq!(primary_language(""), None);
        assert_eq!(primary_language("-TR"), None);
    }

    #[test]
    fn locale_in_env_is_left_alone() {
        // Üçünden **biri** yeter; hangisi olduğu fark etmez. Kurulu bir
        // sistem yereli bile verildi: karar ona hiç bakmamalı.
        for name in ["LC_ALL", "LC_CTYPE", "LANG"] {
            let decided = decide_locale(env_of(&[(name, "C")]), system("tr", "TR"), |_| true);
            assert_eq!(decided, None, "{name} tanımlıyken yerel eklendi");
        }
    }

    #[test]
    fn empty_locale_var_counts_as_unset() {
        // POSIX'te boş `LC_ALL`/`LC_CTYPE`/`LANG` tanımsızla aynı: kabuk
        // onları yoksayıp `C`'ye düşer.
        let env = env_of(&[("LC_ALL", ""), ("LC_CTYPE", ""), ("LANG", "")]);
        let decided = decide_locale(env, system("tr", "TR"), |_| true);
        assert_eq!(decided, pair("LANG", "tr_TR.UTF-8"));
    }

    #[test]
    fn installed_system_locale_becomes_lang() {
        let decided = decide_locale(env_of(&[]), system("tr", "TR"), |name| {
            name == "tr_TR.UTF-8"
        });
        assert_eq!(decided, pair("LANG", "tr_TR.UTF-8"));
    }

    #[test]
    fn missing_system_locale_falls_back_to_utf8_ctype() {
        // Bu makinenin hâli: İngilizce dil + Türkiye bölgesi, `en_TR.UTF-8`
        // yok (`ls /usr/share/locale`).
        let decided = decide_locale(env_of(&[]), system("en", "TR"), |name| {
            name == "tr_TR.UTF-8"
        });
        assert_eq!(decided, pair("LC_CTYPE", "UTF-8"));
    }

    #[test]
    fn locale_without_region_falls_back_to_utf8_ctype() {
        // Bölgesiz yerelden (`en`) `{dil}_{ülke}` kurulamaz. Her ad "kurulu"
        // diyen sorgu bu dalın varlık sorgusundan geçmediğini gösteriyor.
        let decided = decide_locale(env_of(&[]), None, |_| true);
        assert_eq!(decided, pair("LC_CTYPE", "UTF-8"));
    }

    #[test]
    fn locale_name_with_slash_is_not_installed() {
        // `/usr/share/locale/../../../usr` gerçek bir dizin (`/usr`): denetim olmasa
        // "kurulu" derdi.
        assert!(!locale_installed("../../../usr"));
    }
}
