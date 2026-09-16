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
//! Hangi kabuğun koşacağı ([`shell`]) ve shell entegrasyonunun betiğinin
//! nerede durduğu ([`zsh_wrapper_dir`]) da burada: ikisi de kabuğun doğuşuna
//! ait ve ikisi de **spawn'dan önce** cevaplanmak zorunda.
//!
//! Kararlar saf fonksiyonlarda ([`home_directory`], [`decide_locale`],
//! [`is_zsh`]): sistemin okunduğu tek yer birkaç ince sarmalayıcı, geri kalanı
//! sınanıyor.

use std::ffi::{CStr, OsString};
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
    home()
}

/// Kullanıcının ev dizini, [`working_directory`] ile **aynı çözümle**: ayar
/// dizini (`~/.config/bateri/`) de buradan türüyor ve kabuğun `$HOME`'u ile
/// ayarın okunduğu ev iki ayrı kuralla ayrışmasın.
pub(crate) fn home() -> Option<PathBuf> {
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
/// Türk kullanıcı `tr_TR.UTF-8` yerine düşüş yerelini, `fr-CA` kullanıcı
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
///    `en_TR.UTF-8` yok) → `LANG=en_US.UTF-8`: mesajlar İngilizce; tarih,
///    sayı biçimi ve sıralama `en_US`'ninki, ama UTF-8 girişi çalışır.
///
/// **`LC_ALL` değil `LANG`**, yazan iki kolda da: en zayıf değişken
/// (Terminal.app'in yaptığı), kabuğun rc dosyası kendi `LC_*`'ını üstüne
/// yazabilsin. alacritty `LC_ALL` yazıyor ve o, rc'deki her `LC_*`'ı ezer.
///
/// **Düşüş `LC_CTYPE=UTF-8` değil** — alacritty ve iTerm2'nin düşüşünden
/// bilerek ayrılıyoruz. `UTF-8` macOS'ta geçerli bir `LC_CTYPE` ama Linux'ta
/// yerel adı değil, ve macOS'un `ssh_config`'i `LANG` ile `LC_*`'ı uzak
/// makineye taşıyor (`SendEnv LANG LC_*`): oradaki araçlar `setlocale`
/// uyarısı basıp `C`'ye düşerdi. `en_US.UTF-8` Linux'ta da bir yerel adı ve
/// sunucuların çoğunda kurulu; kurulu olmayanda (ör. yalnız `C.UTF-8`
/// taşıyan bir imaj) uyarı yine çıkar — `LANG=en_US.UTF-8` veren her
/// terminalin bedeli. Üstelik `LC_CTYPE` `LANG`'dan güçlü: rc'de yalnız
/// `LANG` değiştiren kullanıcının karakter sınıfını da kilitlerdi.
/// Kullanıcı kararı (`discussion.md` → Karar 6 eki, son madde).
///
/// `en_US.UTF-8`'in kurulu olup olmadığı **sorulmuyor**: `bt-shell` yalnız
/// macOS'ta derleniyor ve o yerel sistemle geliyor (`/usr/share/locale`
/// salt okunur sistem biriminde). "O da yoksa `LC_CTYPE=UTF-8`" diye bir
/// son çare hiç koşmayacak bir dal olurdu. İkisinin karakter sınıfı zaten
/// aynı dosya: `en_US.UTF-8/LC_CTYPE` → `../C.UTF-8/LC_CTYPE`, o da
/// `UTF-8/LC_CTYPE` ile aynı inode.
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
    let name = system
        .map(|(language, region)| format!("{language}_{region}.UTF-8"))
        .filter(|name| installed(name))
        .unwrap_or_else(|| "en_US.UTF-8".to_owned());
    Some(("LANG".to_owned(), name))
}

/// Yerel kurulu mu: `/usr/share/locale/{ad}` dizini var mı.
///
/// `setlocale` ile **sınanmıyor**: o, kendi sürecimizin global yerelini
/// değiştirir. `/` taşıyan ad reddediliyor, dizinin dışına çıkamasın.
fn locale_installed(name: &str) -> bool {
    !name.contains('/') && Path::new("/usr/share/locale").join(name).is_dir()
}

/// Çocuk olarak koşacak kabuğun yolu — alacritty'nin `tty::new` içinde
/// yaptığı çözümün **aynısı**, ama spawn'dan **önce**.
///
/// İkinci bir çözüm doğuyor ve bu bilinçli: shell entegrasyonu "bu zsh mi"
/// sorusunun cevabını `SessionOptions` kurulurken istiyor, alacritty ise aynı
/// soruyu `tty::new`'un içinde, biz artık karışamazken cevaplıyor. Emsali
/// aynı dosyadaki [`home`]: orada da politika bizde, çözüm alacritty'nin
/// sırasıyla **parite** hâlinde.
///
/// **Parite:** `$SHELL`, yoksa kullanıcının passwd kaydındaki `pw_shell`
/// (`ShellUser::from_env`). İki kenarda alacritty ile **aynı** davranıyoruz,
/// çünkü ikisi de aynı `env::var` çağrısından doğuyor: UTF-8 olmayan bir
/// `$SHELL` passwd'ye düşürüyor, boş bir `SHELL=""` ise olduğu gibi
/// alınıyor — o da bizde "zsh değil", alacritty'de çalıştırılamayan bir
/// program demek. Ayrıştığımız tek yer passwd'nin **okunamaması**: alacritty
/// orada oturumu hiç açmıyor, biz yalnız entegrasyonu kurmuyoruz. Kabuğu
/// seçen yine o, biz yalnız aynı cevabı önceden hesaplıyoruz.
///
/// Dock'tan açılışta bu yolun ikinci yarısı **zorunlu**: launchd'nin
/// ortamında `SHELL` yok (`launchctl getenv SHELL` boş), yani yalnız `$SHELL`
/// bakan bir çözüm entegrasyonu tam da sevk edilen pakette kapatırdı. Aynı
/// tuzağın `LANG` hâli [`decide_locale`]'in doc'unda.
pub(crate) fn shell() -> Option<PathBuf> {
    std::env::var("SHELL")
        .ok()
        .or_else(passwd_shell)
        .map(PathBuf::from)
}

/// Kullanıcının passwd kaydındaki kabuk (`pw_shell`); okunamazsa `None`.
///
/// `getpwuid_r`, `getpwuid` değil: ikincisi süreç genelinde paylaşılan statik
/// bir tampon döndürüyor ve başka bir thread'in çağrısı onu tazeliyor.
/// Tampon alacritty'nin `ShellUser::from_env`'iyle aynı 1024 bayt; sığmayan
/// kayıt `ERANGE` ile düşer ve entegrasyon kurulmaz.
fn passwd_shell() -> Option<String> {
    let mut buf = [0; 1024];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: `entry` ve `found` bu çerçevede yaşayan geçerli yazılabilir
    // yuvalar; `buf` de `buf.len()` bayt. `getpwuid_r` kaydı `entry`'ye,
    // dizgileri `buf`'a yazıyor ve `found`'u `entry`'ye ya da null'a çeviriyor.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
        )
    };
    if status != 0 || found.is_null() || entry.pw_shell.is_null() {
        return None;
    }
    // SAFETY: `found` null değil, yani `entry` dolduruldu ve `pw_shell`
    // `buf`'un içinde NUL ile biten bir dizgiyi gösteriyor. Dilim `buf`
    // yaşarken okunuyor ve hemen sahipli bir `String`'e kopyalanıyor.
    let shell = unsafe { CStr::from_ptr(entry.pw_shell) };
    // UTF-8 olmayan kabuk yolu `None`: `SessionOptions.env` `String` istiyor
    // ve entegrasyonun kurulmaması doğru geri düşüş.
    shell.to_str().ok().map(str::to_owned)
}

/// Kabuk zsh mi: yolun son parçası tam olarak `zsh`.
///
/// Yol değil **ad** soruluyor: Homebrew'un `/opt/homebrew/bin/zsh`'i de
/// sistemin `/bin/zsh`'i de aynı kabuk. `zsh-5.9` gibi bir ad tanınmıyor —
/// zsh o adla kurulmuyor ve tanımadığımız bir kabuğa sarmalayıcı kurmak,
/// yükleyemeyeceği dosyalarla açılan bir oturum demek olurdu.
pub(crate) fn is_zsh(shell: &Path) -> bool {
    shell.file_name().is_some_and(|name| name == "zsh")
}

/// zsh sarmalayıcısının dizini: pakette `Contents/Resources/shell/zsh`,
/// debug derlemede deponun `assets/shell/zsh`'i.
///
/// İki kol da **gövdenin varlığıyla** doğrulanıyor (`bateri.zsh`): dizin adı
/// tek başına bir kanıt değil ve eksik betikle kurulan bir `ZDOTDIR`,
/// kullanıcının bütün yapılandırmasını yüklenmemiş bırakırdı.
///
/// Depo kolu **yalnız debug'da** ve `cargo run` yüzünden: geliştirme
/// paketsiz koşuyor, yalnız pakete bakan bir çözüm özelliği en çok
/// koştuğumuz yolda kapatırdı (009 Karar 4). Release'te o kol hiç
/// derlenmiyor — sevk edilen binary'nin bir geliştirme makinesindeki yola
/// düşmesi, ürünü o makineye bağlamak olurdu.
pub(crate) fn zsh_wrapper_dir() -> Option<PathBuf> {
    bundle_shell_dir().and_then(wrapper_dir).or_else(|| {
        cfg!(debug_assertions)
            .then(repo_shell_dir)
            .and_then(wrapper_dir)
    })
}

/// `{shell}/zsh`, yalnız gövde okunabilir bir dosyaysa.
fn wrapper_dir(shell: PathBuf) -> Option<PathBuf> {
    let dir = shell.join("zsh");
    dir.join("bateri.zsh").is_file().then_some(dir)
}

/// Paketin `Contents/Resources/shell`'i: `…/bateri.app/Contents/MacOS/bateri`
/// → iki üst dizin → `Resources/shell`.
///
/// Paket olup olmadığı **sorulmuyor**; çağıranın gövde denetimi zaten
/// cevabı veriyor ve `target/debug/bateri`'nin iki üstünde öyle bir dosya yok.
fn bundle_shell_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?;
    Some(contents.join("Resources/shell"))
}

/// Deponun `assets/shell`'i, crate'in kendi yolundan.
///
/// `CARGO_MANIFEST_DIR` derleme zamanında gömülüyor, yani bu yol derlendiği
/// makinenin yolu — [`zsh_wrapper_dir`]'in onu yalnız debug'da sormasının
/// sebebi bu.
fn repo_shell_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell")
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
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use bt_core::{
        Osc52, Session, SessionOptions, ShellPhase, ShellState, TerminalOptions, Theme, Wake,
    };

    use super::*;
    use crate::settings::TempRoot;

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
    fn missing_system_locale_falls_back_to_en_us_lang() {
        // Bu makinenin hâli: İngilizce dil + Türkiye bölgesi, `en_TR.UTF-8`
        // yok (`ls /usr/share/locale`).
        let decided = decide_locale(env_of(&[]), system("en", "TR"), |name| {
            name == "tr_TR.UTF-8"
        });
        assert_eq!(decided, pair("LANG", "en_US.UTF-8"));
    }

    #[test]
    fn locale_without_region_falls_back_to_en_us_lang() {
        // Bölgesiz yerelden (`en`) `{dil}_{ülke}` kurulamaz. Her ad "kurulu"
        // diyen sorgu bu dalın varlık sorgusundan geçmediğini gösteriyor.
        let decided = decide_locale(env_of(&[]), None, |_| true);
        assert_eq!(decided, pair("LANG", "en_US.UTF-8"));
    }

    #[test]
    fn zsh_is_recognized_by_name_not_by_path() {
        // Homebrew'un zsh'i de sistemin zsh'i de aynı kabuk: yol değil ad
        // soruluyor.
        assert!(is_zsh(Path::new("/bin/zsh")));
        assert!(is_zsh(Path::new("/opt/homebrew/bin/zsh")));
        assert!(is_zsh(Path::new("zsh")));
        // Tanımadığımız kabuğa sarmalayıcı kurmak, yükleyemeyeceği dosyalarla
        // açılan bir oturum demek olurdu.
        assert!(!is_zsh(Path::new("/bin/bash")));
        assert!(!is_zsh(Path::new("/usr/local/bin/fish")));
        assert!(!is_zsh(Path::new("/usr/local/bin/zsh-5.9")));
        // `SHELL=""`: alacritty onu olduğu gibi alıp çalıştıramaz, bizde
        // "zsh değil" demek (parite, [`shell`]'in doc'u).
        assert!(!is_zsh(Path::new("")));
        // Sondaki eğik çizgi adı değiştirmiyor (`Path::file_name`) ve bu
        // sorulmuyor: `/bin/zsh/` bir dizin, yani exec düşer ve oturum hiç
        // açılmaz — entegrasyonun kurulup kurulmadığının bir anlamı kalmaz.
        assert!(is_zsh(Path::new("/bin/zsh/")));
    }

    #[test]
    fn this_user_has_a_resolvable_shell() {
        // Çözümün iki yarısı da gerçek: `$SHELL` sınama sürecinde tanımlı,
        // passwd kaydı da okunabilir olmalı. İkincisi Dock'tan açılışın tek
        // yolu (`launchctl getenv SHELL` boş) ve onu **yalnız** bu sınama
        // koruyor — `$SHELL` her zaman öne geçtiği için kusur sessiz kalırdı.
        assert!(passwd_shell().is_some_and(|shell| shell.starts_with('/')));
        assert!(shell().is_some_and(|shell| shell.is_absolute()));
    }

    #[test]
    fn the_zsh_wrapper_ships_with_the_crate() {
        // Sınamalar debug derlemede koşuyor, yani bu depo kolunu sınıyor:
        // `assets/shell/zsh` yerinde ve gövdesi okunabilir mi. Dizin adı tek
        // başına yetmiyor — gövdesiz bir `ZDOTDIR` kullanıcının bütün
        // yapılandırmasını yüklenmemiş bırakırdı.
        let dir = zsh_wrapper_dir().expect("depo kolunda sarmalayıcı bulunamadı");
        assert!(dir.ends_with("zsh"));
        // zsh'in başlangıç dosyalarının dördü de yerinde. `.zlogout` bilerek
        // yok: `ZDOTDIR` en geç `.zlogin`'de kullanıcıya geri konuyor, yani
        // çıkışta zsh zaten kullanıcının kendi `.zlogout`'unu okuyor
        // (`bateri.zsh`'in başlığı).
        for file in [".zshenv", ".zprofile", ".zshrc", ".zlogin", "bateri.zsh"] {
            assert!(dir.join(file).is_file(), "sarmalayıcıda {file} yok");
        }
        assert!(!dir.join(".zlogout").exists(), ".zlogout beklenmiyordu");
    }

    /// Sınamanın `Wake`'i: hiçbir şey yapmıyor.
    ///
    /// Kare istemeye gerek yok — sorulan tek şey `shell_state()` ve o, `Term`
    /// kilidine dokunmayan ayrı bir sorgu (`Session::shell_state`'in doc'u).
    #[derive(Debug, Default)]
    struct SilentWake;

    impl Wake for SilentWake {
        fn wake(&self) {}
        fn child_exit(&self, _code: Option<i32>) {}
        fn copy_to_clipboard(&self, _text: String) {}
    }

    /// `ready` doğru diyene kadar bekler; süre dolarsa `message` ile düşer.
    fn wait_until(message: &str, ready: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if ready() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{message}");
    }

    #[test]
    fn the_zsh_wrapper_loads_the_users_files_and_reports_marks() {
        // Setin **asıl** sınaması: gerçek bir zsh, gerçek bir PTY ve gerçek
        // bir kullanıcı yapılandırması. Tek turda dört iddia birden:
        //
        // 1. Kullanıcının `.zshenv`'i okundu **ve** oradaki `ZDOTDIR` ataması
        //    geri okundu — bir kullanıcının `ZDOTDIR`'ı olmasının en yaygın
        //    yolu bu ve okumasaydık kalan dosyaları eski dizinden arardık.
        // 2. Kullanıcının `.zprofile`'ı okundu: login kabuğun PATH'i orada
        //    doğuyor ve yalnız `.zshrc`'yi devreden bir sarmalayıcı onu
        //    sessizce düşürürdü.
        // 3. **Bozuk** bir `.zshrc` kabuğu düşürmedi ve işaretler yine geldi.
        // 4. `ZDOTDIR` kullanıcıya geri kondu: `.zlogin` artık bizim
        //    dizinimizden değil onun dizininden okunuyor ve içeride gördüğü
        //    değer kendi dizini.
        // 5. `HISTFILE` kullanıcının dizinini gösteriyor. Sistemin
        //    `/etc/zshrc`'si bizim dosyalarımızdan **önce** okunuyor ve onu
        //    `${ZDOTDIR:-$HOME}/.zsh_history` diye kuruyor: düzeltilmezse
        //    kullanıcının geçmişi uygulamanın paketine yazılır, kendi dosyası
        //    donar ve hiçbir yerde uyarı çıkmaz. Sınamanın gördüğü tek yer
        //    burası — kusur ilk yazımda gerçekten oluştu ve izini
        //    `assets/shell/zsh/.zsh_history` olarak bıraktı.
        let root = TempRoot::new("shell-wrapper");
        let home = root.0.join("home");
        let cfg = home.join("cfg");
        std::fs::create_dir_all(&cfg).expect("sahte ev dizini kurulamadı");
        let write = |path: PathBuf, text: &str| {
            std::fs::write(&path, text)
                .unwrap_or_else(|e| panic!("{} yazılamadı: {e}", path.display()))
        };
        write(
            home.join(".zshenv"),
            "export ZDOTDIR=$HOME/cfg\nexport SEEN_ZSHENV=1\n",
        );
        // R3.5'in pini. `typeset` fonksiyon içinde **yereldir**: kullanıcının
        // dosyası fonksiyondan `source` edilirse bu iki satır dönüşte silinir
        // ve belirti sessizdir. Seçilen deyim uydurma değil — Homebrew, asdf,
        // pyenv ve nvm PATH'i tam böyle kuruyor, yani kusur o araçların
        // bateri'de kaybolması demekti (009 phase-5, ölçüm o dosyada).
        write(
            cfg.join(".zprofile"),
            "export SEEN_ZPROFILE=1\n\
             typeset -U path\n\
             path+=(/opt/probe)\n\
             typeset -A probe_map=(k v)\n\
             export SEEN_ARGC=$#\n",
        );
        // Kasıtlı bozuk: olmayan bir komut **ve** bir sözdizimi hatası. İkisi
        // de `source`'u yarıda bırakır; kabuğu bırakmamalı.
        write(
            cfg.join(".zshrc"),
            "PS1='$ '\nbateri_bozuk_komut_yok\nif then fi\n",
        );
        // `path`'teki indeks makineye bağlı (kalıtılan PATH'in uzunluğu), o
        // yüzden **varlık** basılıyor: `> 0` deterministik.
        write(
            cfg.join(".zlogin"),
            "print -r -- \"$ZDOTDIR $SEEN_ZSHENV $SEEN_ZPROFILE $HISTFILE \
             $((${path[(I)/opt/probe]} > 0)) ${${(t)probe_map}:-yok} $SEEN_ARGC\" \
             >| $HOME/zlogin\n",
        );

        // Sarmalayıcı depodan **kopyalanıyor**, depo dizini `ZDOTDIR` olarak
        // verilmiyor (`/code-review`, 009 kapısı): `ZDOTDIR` oturum boyunca
        // bir süre bizi gösteriyor ve `HISTFILE` düzeltmesi gerilerse zsh
        // oraya `.zsh_history` bırakır. Depo yolunda bu, çalışma kopyasını
        // kirletmenin ötesinde **başka bir crate'in** sınamasını
        // (`zsh_wrapper_inventory_is_exactly_what_the_bundle_copies`, `bateri`)
        // kalıcı kırmızıya çevirirdi — üstelik ayrı bir test binary'sinde,
        // yani belirti rastgele bir koşuda görünürdü.
        let source = zsh_wrapper_dir().expect("sarmalayıcı bulunamadı");
        let wrapper = root.0.join("wrapper");
        std::fs::create_dir_all(&wrapper).expect("sarmalayıcı kopyası kurulamadı");
        for file in [".zshenv", ".zprofile", ".zshrc", ".zlogin", "bateri.zsh"] {
            std::fs::copy(source.join(file), wrapper.join(file))
                .unwrap_or_else(|e| panic!("{file} kopyalanamadı: {e}"));
        }
        let session = Session::spawn(
            SessionOptions {
                // `-l -i`: bizim oturumumuzun hâli (alacritty `login` ile
                // argv[0]'ı `-zsh` yapıyor). Beş dosyanın hangisinin okunacağı
                // buna bağlı, yani sınamanın sınadığı zincir bu iki bayrak.
                command: Some((
                    "/bin/zsh".to_owned(),
                    vec!["-l".to_owned(), "-i".to_owned()],
                )),
                working_directory: Some(home.clone()),
                env: HashMap::from([
                    ("HOME".to_owned(), home.display().to_string()),
                    ("ZDOTDIR".to_owned(), wrapper.display().to_string()),
                ]),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                },
                theme: Theme::BATERI,
            },
            Arc::new(SilentWake),
        )
        .expect("oturum açılamadı");

        // `A` sonra `B`: prompt çizildi ve bitti. Buraya gelmek 1–3'ü birden
        // kanıtlıyor — kanca yüklenmiş, PS1'e ek girmiş ve bozuk dosya
        // kabuğu düşürmemiş.
        wait_until("prompt işaretleri gelmedi", || {
            session.shell_state()
                == Some(ShellState {
                    phase: ShellPhase::Input,
                    last_exit: None,
                })
        });

        // Bir komut koştur: `C` çalışmayı, sonraki prompt'un `D`'si çıkış
        // kodunu getirir. `false` seçildi ki kod sıfırdan farklı olsun —
        // `0` "kodu okuyamadım"la karışırdı.
        session.write(b"false\n");
        wait_until("komutun çıkış kodu duruma düşmedi", || {
            session
                .shell_state()
                .is_some_and(|state| state.last_exit == Some(1))
        });

        session.write(b"exit\n");
        // 4 ve 5: `.zlogin` çıkışta değil **açılışta** okundu (login kabuk),
        // ama dosyayı yazan satır kabuk çıkana kadar diske inmiş olmayabilir;
        // beklemek yerine dosyanın varlığını bekliyoruz. `.zlogin` geri
        // koymadan **sonra** koştuğu için gördüğü `HISTFILE` düzeltilmiş olan.
        let seen = home.join("zlogin");
        wait_until("kullanıcının .zlogin'i okunmadı", || seen.is_file());
        let seen = std::fs::read_to_string(&seen).expect("zlogin izi okunamadı");
        // Son üç alan R3.5: `typeset -U path` ile eklenen dizin `path`'te
        // duruyor, `typeset -A` dizisi hâlâ bir association ve kullanıcının
        // dosyası konumsal parametre görmüyor. Üçü de fonksiyon içinden
        // `source` edilen bir dosyada başarısız olur.
        assert_eq!(
            seen.trim(),
            format!("{0} 1 1 {0}/.zsh_history 1 association 0", cfg.display()),
            "ZDOTDIR/HISTFILE geri konmadı, kullanıcının dosyaları yüklenmedi \
             ya da dosya zsh'in okuduğu bağlamda okunmadı (`typeset` yerel \
             kaldı / konumsal parametre sızdı)"
        );
        // Kusurun kendi izi: geçmiş **bizim** dizinimize yazılmış olmamalı.
        //
        // Kapanışı BEKLEMEK zorunlu (`/code-review`, 009 kapısı): zsh
        // `$HISTFILE`'ı **çıkışta** yazıyor (zincirde `inc_append_history` ya
        // da `share_history` kuran yok). `exit`ten mikrosaniyeler sonra
        // bakan bir iddia yarışı her seferinde kazanır ve kusur gerilese de
        // yeşil kalırdı — tuzağın kendisi tuzağa düşerdi.
        //
        // Senkron noktası **olayın kendisi**: geçmiş dosyasının kullanıcının
        // dizininde belirmesi. İki aday reddedildi — `reader_alive()`
        // `shutdown()` okuyucuyu `take` ettiği için dönüşte zaten `false`,
        // `Teardown::Clean` ise burada garanti değil (ölçüldü: `Abandoned`
        // geliyor; çıkışın içinde takılan çocuk kapanışı asamıyor, kayıtlı
        // borç — `CLAUDE.md` → Kapanış). Beklenen olay aynı zamanda
        // **pozitif** iddia: negatif iddia tek başına "doğru yere yazıldı"
        // ile "hiç yazılmadı"yı ayırt edemezdi, ikisi de sarmalayıcının
        // dizinini boş bırakır.
        wait_until(
            "komut geçmişi kullanıcının dizinine yazılmadı",
            || cfg.join(".zsh_history").is_file(),
        );
        assert!(
            !wrapper.join(".zsh_history").exists(),
            "komut geçmişi sarmalayıcının dizinine yazıldı"
        );
        session.shutdown();
    }

    #[test]
    fn locale_name_with_slash_is_not_installed() {
        // `/usr/share/locale/../../../usr` gerçek bir dizin (`/usr`): denetim olmasa
        // "kurulu" derdi.
        assert!(!locale_installed("../../../usr"));
    }
}
