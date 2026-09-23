//! Uygulama delegate'i: **uygulama geneli** — ayarları okur ve izler, temayı
//! ve görünümü çözer, alt başlığın yuvalarını tutar, pencereleri açar ve
//! listeler, kayıt anı yollarını her pencereye dağıtır ve kapanış sırasını
//! yürütür. Pencere başına olan her şey (yüzey, renderer, oturum, display
//! link, dock payı, geçici punto) `window`'da. Çizim çağrısı burada **yok**,
//! bu dosyanın işi bağlamak.

use std::cell::{Cell, Ref, RefCell};
use std::ffi::{OsString, c_void};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bt_core::{
    CursorMotion, ReduceMotion, SHUTDOWN_GRACE, SYSTEM_THEME, Settings, SettingsEdit,
    ShellIntegration, SmoothScroll, Teardown, Theme,
};
use bt_gpu::{CellMetrics, DOCK_ROWS, DisplayLink, MIN_SAMPLES, Renderer, Stats};
use dispatch2::DispatchQueue;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlertFirstButtonReturn, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication,
    NSApplicationDelegate, NSApplicationTerminateReply, NSEvent, NSMenu, NSMenuDelegate,
    NSMenuItem, NSWindow, NSWorkspace, NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification,
};
use objc2_foundation::{
    NSArray, NSDictionary, NSKeyValueObservingOptions, NSNotification, NSNumber, NSObject,
    NSObjectNSDelayedPerforming, NSObjectNSKeyValueObserverRegistration, NSObjectProtocol,
    NSRunLoopCommonModes, NSString, NSURL, NSUserDefaults, ns_string,
};

use crate::notices::{Notices, Source};
use crate::settings_window::SettingsWindow;
use crate::watch::{Notify, Watch};
use crate::window::{self, CloseScope, TerminalWindow};
use crate::{Options, Run, Workload};
use crate::{child, settings};

/// Boşta sıfır karenin bekçisi: [`Workload::Smoke`] yükünde pencere ilk
/// çizimden sonra ~`run_seconds` saniye boşta duruyor.
///
/// **Sınırın operandı `kare` değil [`Counters::content`]** (`icerik=`
/// jetonu): GPU'nun bitirdiği kare değil, çizilmeye **karar verilen** içerik
/// karesi. Sebep bu sınırın kendi doc'unda yıllanmış borç — "durma koşulu
/// unutulmuş bir animasyon bugünkü kapıdan yeşil geçer". Çare sınırı
/// oynatmak değil, hareket karelerini kapının dışında tutmak oldu: bir imleç
/// kayması `kare`'yi meşru olarak ~24'e çıkarır, `icerik`'i hiç artırmaz
/// (`.tasks/008-hareket-ve-imlec/discussion.md` → Karar 2).
///
/// **İki sayacın ilişkisi hareketle birlikte koptu** (`/code-review`
/// bulgusu): operand değiştiğinde "hatasız biten her kare bir içerik
/// karesiydi" diye yazılmıştı ve o cümle phase-1'de doğruydu, phase-3'ten
/// sonra **değil** — hareket karesi de bir komut tamponu commit ediyor, yani
/// `kare`'yi artırıp `icerik`'i artırmıyor. Yön bugün tersine bile dönmüş
/// durumda: ölçülen sağlıklı duman koşusu `kare` 27–30 iken `icerik` 2–3
/// (aşağıdaki 008 satırı). Yani sınır **daha gevşek** bir sayacın üstünde
/// duruyor, daha sıkı değil — ve bu yüzden taşınan değil **yeniden ölçülen**
/// bir sayı gerekiyordu; phase-6 onu ölçtü.
///
/// **Sayı iki kez ölçüldü; ikincisi görünür pencerede ve onu değiştirmedi.**
/// Koşu tabloları, ortam ve yöntem `docs/OLCUMLER.md` → `## Boşta kare`'de;
/// burada yalnız sınırı doğuran kutuplar ve türetme duruyor.
///
/// - **005 phase-3 (2026-09-12, debug, bundle'sız süreç):** `2` → `8`. Eski
///   `2`'nin dayanağı ("sistem display link'i askıya alıyor, tavan ~3 kare")
///   005 phase-2b'de çürüdü: [`Workload::Load`] aynı pencere durumunda beş
///   saniyede `kare=594` üretti, yani ölçülen şey tavan değil kapanış
///   kilitlenmesiyle bozulmuş bir koşuydu. Üstelik `2` **doğru bir build'de
///   kırmızı düştü** (beş saniyelik sağlıklı koşu `kare=4`). Kutuplar:
///   sağlıklı en çok `4`, bozuk en az `49`.
/// - **006 phase-5 (2026-09-15, debug + release paket; yoklanan iki koşuda
///   pencere ekranda ve önde):** sağlıklı elli bir koşunun en yükseği `2`, bozuk altı koşunun en
///   düşüğü `353`. Görünür pencere meşru kare sayısını **artırmadı**; bozuk
///   koşuyu ise tam tazeleme hızına taşıdı.
///
/// `8` iki ölçümün kutuplarının arasında: en yüksek sağlıklı gözlemin (`4`)
/// iki katı, en düşük bozuk gözlemin (`49`) altıda biri. 006 boşluğu yalnız
/// genişletti; sınırı oynatacak bir gözlem yok — düşürmek 005'in sağlıklı
/// `4`'ünü yeniden ölçmeden geçersiz saymak olurdu.
///
/// - **008 phase-6 (2026-09-16, debug + release paket):** operand `kare`'den
///   `icerik`'e geçtikten sonraki **ilk** ölçüm, yani üstteki iki satırın
///   sayıları artık başka bir sayacın. Otuz sağlıklı koşuda `icerik` en çok
///   `3`, bozuk kolda (koşulsuz `wake()`) en az `357`. Kural iki uçta da
///   sağlanıyor ve sınırı **oynatan bir gözlem yok**: `8`, `3`'ün iki
///   katından (`6`) büyük ve `357`'nin çok altında.
///
/// **Kapıya bağlanan profil debug**, çünkü gözetimsiz koşan tek bağlam
/// `make duman` ve o debug derliyor. Release paketi de aynı sınıra tabi (kapı
/// profilden bağımsız) ve dağılımı ayrı ölçüldü; aynı sayı ikisini de taşıyor.
///
/// Sınır, bu makinenin iki rejiminde de güvenli ama pay rejime bağlı.
/// Kısılmış rejimde (005: ölçüm yükü 5 sn'de `kare=21`, yani ~4 Hz) bozuk bir
/// üç saniyelik duman ~12 kare eder, `8`'in **1,5 katı** — sınırı buradan
/// yükseltmemenin sebebi bu. 006'nın görünür penceresinde kısılma görülmedi.
/// Aynı binary'nin ölçüm yükünde iki rejim vermesinin (bir koşuda `kare=21`,
/// bir koşuda `kare=597`) en olası değişkeni pencere görünürlüğü ama bu
/// **doğrulanmadı**: 006 duman yükünde pencereyi ekranda gördü, ölçüm yükünü
/// koşmadı.
///
/// **Sınır büyürken kapının algılama tabanı da yükseldi** ve bunun bedeli
/// bugün değil sonra ödenecek. Kapı `n > limit`'te ateşliyor, yani yakalamak
/// için `limit + 1` kare gerekiyor: üç saniyelik bir koşuda eski `2` **1
/// Hz**'lik bir sızıntıyı yakalardı, bugünkü `8` ancak **3 Hz**'i yakalıyor.
/// (İkisi de `make duman`'ın 3 saniyesinden türüyor; süre değişirse eşik de
/// değişir.) Durma koşulu unutulmuş 2 Hz'lik bir blink üç saniyede ~6 kare
/// eder — sınırın altında, yani bu sayı onu tek başına **göremez**.
///
/// **Bu yüzden sınır kapının tamamı değil, bir katı.** 008 kapıyı iki katlı
/// kuruyor ve ikisi de buradan bağımsız: (a) deadline'da **yerleşmemiş**
/// animasyon varsa koşu kırmızı — hızdan bağımsız, ölçüm istemez, ama yalnız
/// hareket altyapısından geçen animasyonları görür; (b) son kareyle deadline
/// arasındaki sessizlik ([`QUIET_FLOOR`]) — altyapıyı atlayan sızıntıyı da
/// görür ve **ölçüldü** (phase-6): kapının en duyarlı katı artık o, çünkü
/// periyodu 868 ms'den kısa her sızıntıyı yakalıyor, bu sayı ise ancak
/// 3 Hz'in üstünü.
///
/// **Sağlıklı koşudaki oynamanın mekanizması ölçülmedi.** Kare talebi
/// (`istek=`) üç ölçümde de **sabit** kaldı (006'da 2–3, 008'de 4), yani
/// fazladan kare fazladan **talepten** gelmiyor — geometri/örtülme kancaları
/// olsaydı `istek` de artardı. Oynama üç ölçümde de profile göre ayrıştı ama
/// yönü 008'de **döndü**: 006'da `kare` debug'da çoğunlukla `1`, release
/// pakette `2` idi; 008'de `icerik` debug'da çoğunlukla `3`, release pakette
/// `2`. Talep yine ayrışmadı. Geriye
/// taleplerin birleşip birleşmemesi kalıyor (açılış karesi shell'in ilk
/// baytlarından önce çizildiyse ikinci bir kare gerekir; profil farkı onu
/// `acilis=` ile sınayabilir, sınanmadı) ama bu bir **hipotez**, ölçüm değil.
///
/// **Kapı yalnız `BT_RUN_SECONDS` yolunda değerlendiriliyor**
/// ([`AppDelegate::report_and_exit`]). Gözetimsiz koşan tek bağlamı
/// `make duman`; paketten aynı ortamla açılan koşu da aynı sınıra tabi (006'nın
/// bozuk paket koşuları onu ateşledi). Etkileşimli koşu bu sınırı hiç
/// değerlendirmiyor.
///
/// **Ne zaman yeniden ölçülür:** kare yolunu ya da pencerenin görünürlüğünü
/// değiştiren bir set geldiğinde (hareket/motion, sekme). Tarif
/// `docs/OLCUMLER.md` → `## Nasıl yeniden ölçülür`.
///
/// **Bilinen yanlış pozitif (duruyor):** `DisplayLink::resize` koşulsuz kare
/// istiyor, yani koşu sırasında pencereyi sürüklemek meşru kareler üretir ve
/// sekiz kareyi de aşabilir. `make duman` gözetimsiz koşuyor, bedel kabul
/// edildi; kalıcı çözüm geometri yolundan gelen kareleri sayaç dışında
/// tutmak.
///
/// Sınırın **çizilen** kare üstünde durmasının sebebi adı: "boşta sıfır kare"
/// çizim hakkında bir söz. `istek=` daha erken bir yerde sayıyor ama kapı
/// değil — eşiği ölçülmedi.
///
/// **Ölçülen `istek ≈ kare + 2` ilişkisi 008'de geçersizleşti** ve cümlenin
/// düzelttiği şey bir sayı değil bir mekanizma: hareket kareleri `Waker`'a
/// hiç dokunmuyor (`bt_gpu::link` modül başlığı), yani `kare`'yi şişirirken
/// `istek`'i şişirmiyorlar. İlişkinin yeni hâli **ölçüldü** (phase-6, otuz
/// sağlıklı koşu): `istek` otuz koşunun hepsinde `4`, `icerik` `2`–`3`, yani
/// `istek ≈ icerik + 1..2` — `kare` ise 27–30, ondan tamamen kopmuş durumda.
/// (Sonraki bir koşuda `istek=3` görüldü ve nedeni ölçülmedi; kayıt
/// `docs/OLCUMLER.md`'de.)
/// Ölçüm yükünde `istek` ile `kare` üç mertebe ayrışıyor (bkz. `bt_gpu`'nun
/// `requests` sayacı); oran olarak bir kapı kurulabilir ama o ölçülmedi.
///
/// [`Workload::Load`] yükünde üst sınır **yok** — orada kare akışı işin
/// kendisi.
const IDLE_FRAME_LIMIT: u64 = 8;

/// Duman koşusunun sonunda beklenen **en az** sessizlik: son çizilen kareyle
/// deadline arası (`sessiz=` jetonu). Altı kırmızı, `sessiz=none` de kırmızı.
///
/// [`IDLE_FRAME_LIMIT`]'in **tamamlayıcısı, kopyası değil.** O, üç saniyede
/// sekizden fazla içerik karesi çizen bir sızıntıyı görüyor, yani ancak ~3
/// Hz'in üstünü; bu ise **periyodu** bu değerden kısa olan her sızıntıyı
/// görüyor (~1,15 Hz'in üstü). Ölçülen boşluk tam da buydu: yarım saniyelik
/// bir sızıntı `icerik=8` ile sınırı **aşmadan** geçiyor ve o koşu bugün
/// yeşil düşüyordu (`docs/OLCUMLER.md` → `## Boşta kare`, "yavaş sızıntı").
///
/// **Kuralın yönü bu jetonda ters:** sağlıklı koşuda `sessiz` büyük, bozuk
/// koşuda küçük. Taban bu yüzden "en düşük sağlıklı gözlemin en çok yarısı
/// **ve** en yüksek bozuk gözlemin üstünde" ve aralığın **en büyük** ucundan
/// seçiliyor — ortadan seçilen bir sayı kapıyı yavaş sızıntıya körleştirirdi.
/// Türetme (2026-09-17, yirmi sağlıklı koşu): en düşük sağlıklı
/// `1737,12 ms` → tavan `868,56 ms`; en yüksek bozuk `129,25 ms` (2026-09-16).
/// `868` o aralığın en büyük tam milisaniyesi.
///
/// **Bir kez zaten tetiklendi ve bu sabitin asıl dersi o.** 2026-09-16'nın
/// türetmesi `1742,29 ms`'lik bir uçtan `870`'i vermişti; 011'in duman
/// reçetesini değiştirmesinden sonra yirmi koşuluk yeniden gözlem bandın
/// alt ucunu `1737,12`'ye indirdi ve `870` kuralın tavanını **1,44 ms**
/// aştı. Kapı o koşularda yeşildi — aşım payda saklanıyordu, sayıda değil.
/// Ders: bu sabitin tetiği dar ve **sessiz**; üç saniyelik sağlıklı bir koşu
/// `1737 ms`'nin altına inerse bozulan şey kapı değil **kuralın kendisi**
/// olur ve sayı `/measure` ile yeniden türetilmelidir.
///
/// **Dört sayı birbirine bağlı ve gerekçeleri aynı blokta**
/// (`docs/OLCUMLER.md` → `## Boşta kare`): `BT_RUN_SECONDS`'ın 3'ü,
/// [`bt_core::smoke_shell`]'in 1 saniyelik uykusu, aynı reçetenin imleç
/// sıçrama **mesafesi** (011) ve bu taban. Sessizlik
/// `koşu süresi − (uyku + yerleşme)` kadar, yani **ikisinden biri oynarsa bu
/// sayı da oynamak zorunda**: `BT_RUN_SECONDS=2` ile kuyruk ~0,75 saniyeye
/// iner ve kapı kod doğruyken düşer. Üçü üç dosyaya dağılırsa biri
/// oynadığında kapı sessizce kırılganlaşır.
///
/// **Bilinen yanlış pozitif** ([`IDLE_FRAME_LIMIT`]'inkiyle aynı kök):
/// koşunun son `QUIET_FLOOR`'unda pencereyi sürüklemek, örtüp açmak ya da
/// ekranı uyandırmak meşru bir kare doğurur ve kuyruğu sıfırlar. Kalıcı çare
/// aynı: geometri kaynaklı kareleri sayaç dışında tutmak (kayıtlı borç,
/// `docs/YOL-HARITASI.md`).
///
/// Yalnız [`Workload::Smoke`]'ta soruluyor: ölçüm yükü deadline'a kadar çıktı
/// akıtıyor, yani orada sessizlik sıfıra yakın olmak **zorunda**
/// ([`Verdict::MotionUnsettled`]'ın aynı kolda muaf olmasının gerekçesiyle).
const QUIET_FLOOR: Duration = Duration::from_millis(868);

/// Kullanıcının dünyasına açılan girişlerin **tek** dalı.
///
/// Süreli koşu (`make duman`, ölçüm) ayar dosyasını, dosya izlemeyi, sistemin
/// açık/koyu görünümünü, Hareketi Azalt ayarını ve Tema menüsünün
/// `themes/`'ten dolmasını görmez:
/// kapının sonucu o makinenin `~/.config/bateri/`'sine bağlı olmasın: dosyayı
/// okuyup izlemeyi kuran [`AppDelegate::load_settings`] ve
/// [`AppDelegate::reload_settings`], görünümü okuyan
/// [`AppDelegate::apply_appearance`] ve Theme ▸'yi dolduran
/// `menuNeedsUpdate:`. Dosyayı yaratan "Settings…"
/// ([`AppDelegate::edit_settings`]) ve yazan tema seçimi
/// ([`AppDelegate::save_theme`]) de bu değere bakar, kendi `run.is_some()`
/// koşulunu yazmaz — beş ayrı koşuldan birinin unutulduğu gün kapı sessizce
/// kullanıcının dosyasına bağlanırdı
/// (`.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 1).
///
/// **Beşincisi Hareketi Azalt** ([`resolve_reduce_motion`], 008 phase-5) ve
/// dalı ayar dosyasında değil sistemde: `NSWorkspace`'in erişilebilirlik
/// ayarı okunsaydı `make duman`'ın `hareket=` jetonu ölçen makinenin
/// erişilebilirlik tercihine bağlanır, yani kapı bir makinede yeşil bir
/// makinede kırmızı düşerdi. Gözlemciyi kuran
/// [`AppDelegate::observe_reduce_motion`] de aynı değere bakıyor.
///
/// Bedeli: dosyadan ekrana giden kabloyu hiçbir kapı görmüyor; onu geçici
/// dizindeki sınamalar (`settings`) ve göz kontrolü taşıyor.
///
/// **Saklanmıyor**, her soruşta [`AppDelegate::inputs`] ile `Ivars.run`'dan
/// türüyor: ayrı bir ivar aynı kararın ikinci kopyası olurdu ve ikisinin
/// ayrıştığı gün süreli bir koşu kullanıcının dosyasını okurdu.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Inputs {
    /// Süreli koşu: gömülü varsayılanlar, dışarıdan hiçbir şey.
    Hermetic,
    /// Kullanıcının oturumu. `config_root` `None` → ev dizini çözülemedi ve
    /// ayar dosyası aranmıyor.
    User { config_root: Option<PathBuf> },
}

/// [`Inputs`]'un kararı — saf, sınanıyor.
fn decide_inputs(run: Option<Run>, home: Option<PathBuf>) -> Inputs {
    match run {
        Some(_) => Inputs::Hermetic,
        None => Inputs::User {
            config_root: home.as_deref().map(settings::config_root),
        },
    }
}

/// Üç değerli `[motion] reduce_motion` + sistemin cevabı → tek `bool`.
///
/// **Birleştirme burada, çünkü sistemi gören katman burası:** `bt-gpu` AppKit
/// görmüyor (`CLAUDE.md` → katman tablosu) ve `bt-core`'un ayar modeli zaten
/// bir dosyanın karşılığı, bir erişilebilirlik ayarının değil. Aşağıya
/// **çözülmüş** bir `bool` iniyor (`Renderer::set_font` emsali).
///
/// `system` bir **closure**, `bool` değil: `"on"`/`"off"` diyen kullanıcının
/// oturumunda `NSWorkspace`'e hiç gidilmiyor. Süreli koşu da hiç gitmiyor ve
/// bu bir tembellik değil kapı — [`Inputs::Hermetic`]'te `make duman`'ın
/// satırı ölçen makinenin erişilebilirlik ayarına bağlanırdı.
///
/// Saf ve bu yüzden sınanabilir: gerçek bir `AppDelegate` gerekmiyor
/// (`hermetic_run_does_not_read_reduce_motion`).
fn resolve_reduce_motion(
    inputs: &Inputs,
    setting: ReduceMotion,
    system: impl FnOnce() -> bool,
) -> bool {
    if let Inputs::Hermetic = inputs {
        return false;
    }
    match setting {
        ReduceMotion::On => true,
        ReduceMotion::Off => false,
        ReduceMotion::System => system(),
    }
}

/// `[motion] smooth_scroll` + Hareketi Azalt + `cursor_motion` → tek `bool`:
/// tekerlek pürüzsüz mü gidiyor.
///
/// Üçünden biri hareketi kapatıyorsa satır adımı — hareketi kapatmış olana
/// kaydırma animasyon *eklemez* (`cursor_motion = "snap"`'in Hareketi
/// Azalt'la ilişkisinin aynısı). Nicemleme **kaynakta**, `bt-gpu`'nun
/// `Motion`'ında değil: `false` kolu bugünkü satır yolu olarak kalıyor
/// (`.tasks/027-yumusak-kaydirma/discussion.md` → Karar 5).
///
/// `reduce` [`resolve_reduce_motion`]'ın çözülmüş cevabı, yani süreli koşu
/// burada da sistemi okumuyor. Saf, sınanıyor.
fn resolve_smooth_scroll(settings: &Settings, reduce: bool) -> bool {
    settings.smooth_scroll == SmoothScroll::On
        && !reduce
        && settings.cursor_motion != CursorMotion::Snap
}

/// Shell entegrasyonunun çocuğa eklediği ortam — kurulmuyorsa boş.
///
/// **Kararın tamamı burada ve saf**: hangi kabuk, hangi ayar, betik nerede.
/// Yeri `child` değil `app`, çünkü kapının ilk katı [`Inputs`] ve o bu modüle
/// özel ([`resolve_reduce_motion`] emsali; orada da sistemi okuyan taraf
/// `bt-shell` ama kararı `Inputs` kapılıyor).
///
/// `shell` ve `script_dir` birer **closure**: süreli koşuda ve `"off"` diyen
/// kullanıcının oturumunda ikisine de hiç gidilmiyor. Süreli koşununki bir
/// tembellik değil **kapı** — `make duman`'ın sonucu ölçen makinenin kabuk
/// yapılandırmasına bağlanırdı ve kapıyı closure'ı panikleyen bir sınama
/// tutuyor (`hermetic_run_does_not_set_up_shell_integration`).
///
/// `zdotdir` **eager**: kendi sürecimizin ortamı, kullanıcının dünyasına
/// açılan bir giriş değil ve hermetik kolda değeri çocuğa zaten hiç ulaşmıyor.
///
/// Dönüş `Vec`, `Option` değil: kurulan ortam bir çift değil **iki** çift
/// olabiliyor (kullanıcının özgün `ZDOTDIR`'ı varsa ikincisi de gider) ve
/// çağıran onu `locale_env()`'in yanına zincirliyor.
fn shell_integration_env(
    inputs: &Inputs,
    setting: ShellIntegration,
    shell: impl FnOnce() -> Option<PathBuf>,
    script_dir: impl FnOnce() -> Option<PathBuf>,
    zdotdir: Option<OsString>,
) -> Vec<(String, String)> {
    if matches!(inputs, Inputs::Hermetic) || !setting.installs_wrapper() {
        return Vec::new();
    }
    // Tanımadığımız kabuk sessizce geri düşüyor: terminal bugünkü gibi
    // çalışıyor, yalnız işaret gelmiyor.
    if !shell().is_some_and(|shell| child::is_zsh(&shell)) {
        return Vec::new();
    }
    // UTF-8 olmayan yol da aynı sessiz geri düşüş: `SessionOptions.env`
    // `String` istiyor ve entegrasyonsuz bir oturum, yarım kurulmuş bir
    // `ZDOTDIR`'dan iyi.
    let Some(dir) = script_dir().and_then(|dir| dir.into_os_string().into_string().ok()) else {
        return Vec::new();
    };
    // Kullanıcının özgün `ZDOTDIR`'ı: betik onu geri koyacak. Üç kol da
    // "ikinci çift gitmesin" diyor ama gerekçeleri ayrı:
    let original = match zdotdir {
        // Boş değer tanımsız sayılıyor (`decide_locale`'in kuralı) — boş bir
        // `ZDOTDIR`'ı "geri koymak" `$HOME`'u işaret eden bir değişken
        // yaratmak olurdu.
        None => None,
        Some(value) if value.is_empty() => None,
        Some(value) => match value.into_string() {
            // **Kendine dönük değer** (`/code-review`, 009 kapısı): ortamdaki
            // `ZDOTDIR` zaten betiğin dizinini gösteriyorsa (elle kurulmuş ya
            // da sızmış) onu "kullanıcının özgün değeri" diye geri vermek,
            // betiğe kendi `.zshenv`'ini yeniden yükletir ve zsh'in `FUNCNEST`
            // sınırına kadar özyineler; oturum `ZDOTDIR`'sız kalır. Betikte de
            // bir kat var, bu ilk kat.
            Ok(value) if value == dir => None,
            Ok(value) => Some(value),
            // **UTF-8 olmayan değer entegrasyonu tümden reddediyor** ve bu
            // kol `var` yerine `var_os` istemesinin sebebi (`/code-review`,
            // 009 kapısı): `var().ok()` onu `None`'a düşürüyordu, yani
            // "kullanıcının `ZDOTDIR`'ı yoktu" sayılıyor ve betik oturumun
            // sonunda değişkeni **siliyordu** — kullanıcının bütün
            // yapılandırması tanısız kaybolurdu. Komşu her kenar (UTF-8
            // olmayan betik yolu, tanınmayan `$SHELL`) entegrasyonu
            // reddederek geri düşüyor; `decide_locale` de "yok" ile
            // "kullanılamaz"ı bilerek ayırıyor.
            Err(_) => return Vec::new(),
        },
    };
    let mut env = vec![("ZDOTDIR".to_owned(), dir)];
    if let Some(original) = original {
        env.push(("BATERI_ZDOTDIR".to_owned(), original));
    }
    // **Yalnız `blocks` kademesinde gönderiliyor** (`BATERI_ZDOTDIR`'ın
    // koşullu olmasıyla aynı biçim): varsayılan kolda ortama tek bayt
    // eklemiyoruz ve betiğin "değişken yok → prompt terminalin" kuralı
    // varsayılanın **tek** kaydı oluyor. İki yerde yazılsaydı biri
    // değiştiğinde öteki sessizce eskirdi.
    //
    // Değişkenin adı kararı söylüyor, sonucunu değil: betik ondan **üç** şey
    // türetiyor (prompt sıfırlansın mı, ayna kurulsun mu, dal basılsın mı) ve
    // üçü de "bu oturumda dock var mı"nın cevabı. Kararı terminal veriyor,
    // kabuğa sorulmuyor (`ShellIntegration::wants_dock`).
    if !setting.wants_dock() {
        env.push(("BATERI_DOCK".to_owned(), "off".to_owned()));
    }
    env
}

/// Pencere geometrisi + hücre ölçüsünden türeyen grid.
///
/// Adı `Metrics` değil: hücre metriğinin sahibi artık `bt-gpu`
/// ([`CellMetrics`]) ve iki tip bu dosyada yan yana okunuyor. Buradaki
/// "kaç sütun kaç satır **ve** hangi hücreyle", oradaki yalnız hücre.
///
/// `view` modülüne de açık: fare çevirisi aynı üçlüyü ister ve ayrıştırma
/// iki çağrı yerinde tekrarlanacağına burada bir kez durur.
#[derive(Clone, Copy)]
pub(crate) struct Grid {
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    /// Demet değil `CellMetrics`: ölçü buradan `DisplayLink::resize`'a
    /// olduğu gibi geçiyor. `Grid`'de saklanan bu değer yalnız
    /// `SessionOptions`'a girerken demete iniyor — `split_into_grid`'un
    /// bölmeye soktuğu demet başka bir değer: oraya **gelen** ölçü girer,
    /// `Grid` ondan sonra doğar.
    pub(crate) cell: CellMetrics,
}

/// Piksel geometrisi + ızgara ölçüsü → grid.
///
/// `TerminalWindow::sync_geometry`'den ayrı duruyor çünkü saf olan tek parça
/// bu; geri kalanı pencere ve layer, yani sınanamaz. Ölçü **argüman**: bu gövdeye
/// gizlenmiş bir sabit `cell_metrics_come_from_outside`'i düşürür.
///
/// Kapsamı bu kadar, daha fazlası değil: `CELL_PX`'in asıl durduğu satır
/// `sync_geometry`'deki `cell_metrics(scale)` çağrısıydı ve orası bir
/// pencere ile Metal device istediği için sınanmıyor. `CellMetrics::new`
/// bilerek `pub`, yani oraya yazılacak bir `CellMetrics::new(9, 18, 7, 8, 1)` yer
/// tutucuyu diriltir ve buradaki iki sınama yeşil kalır.
///
/// **Sol pay sütunlardan düşülür** (010 Karar 3): şerit metnin üstüne
/// binmesin. Pay her zaman ayrılıyor — entegrasyonsuz oturumda (bash/fish,
/// `shell.integration = false`, SSH) boş kalması kabul edilen bedel;
/// alternatifi ilk prompt'ta bir SIGWINCH ve üç tüketicinin aynı anda
/// güncellenmesiydi.
///
/// **Dock payı satırlardan düşülür** (012) ve sol payın tersine **koşullu**:
/// dock yalnız entegrasyonlu zsh oturumunda var ve karar oturum doğarken
/// veriliyor (`TerminalWindow::start`). Payı koşulsuz ayırmak dock'u
/// olmayan pencereden sebepsiz iki satır götürürdü — sol payın sekiz
/// noktasıyla kıyaslanmayacak bir bedel.
///
/// **Pay koşu boyunca oynuyor** (R5.2): alternatif ekranda sıfıra iniyor,
/// çıkışta doğum değerine dönüyor (`dock_rows_for`,
/// `TerminalWindow::alt_screen_did_change`).
/// Oynamanın bedeli bir `TIOCSWINSZ` ve o bedel **komut başına değil geçiş
/// başına** ödeniyor — `git log` gibi alternatif ekrana girmeyen komutlar
/// bayrağı hiç oynatmıyor, yani bu fonksiyon da yeniden çağrılmıyor.
pub(crate) fn split_into_grid(
    width_px: f64,
    height_px: f64,
    cell: CellMetrics,
    dock_rows: u16,
) -> Grid {
    let (cell_w, cell_h) = cell.cell_px();
    // `as u16` f64'te doygundur (NaN ve negatif → 0, büyük → 65535) ve kesme
    // tam olarak istediğimiz taban yuvarlama; sıfır sütun/satırı
    // `Session::resize` zaten yoksayar (simge durumundaki pencere). Bölen
    // sıfır olamaz ve bunu tip taşıyor: `CellMetrics`'in alanı private ve
    // kurucusu (`CellMetrics::new`) sıfırı eliyor; üretimdeki kaynağı
    // `Renderer::cell_metrics`, oranın garantisi de `bt-atlas`'ın ≥ 1
    // kırpması.
    //
    // Çıkarma **`f64`'te** ve bu bir tercih değil şart: paydan dar bir
    // pencerede fark negatife iner, bölme negatif kalır ve `as u16` onu
    // sıfıra doyurur — yani mevcut davranış (sıfır sütun, `Session::resize`
    // yoksayar) korunur. Aynı çıkarma `u16`'da yapılsaydı **taşar** ve
    // 65535'e yakın bir sütun sayısı, o boyda bir `TIOCSWINSZ` üretirdi.
    // Yeni bir alt sınır bilerek getirilmiyor: zincirin sonu zaten doğru.
    let usable_width = width_px - f64::from(cell.gutter_px());
    // Dock payı da **`f64`'te** ve aynı gerekçeyle: dock'tan alçak bir
    // pencerede fark negatife iner, bölme negatif kalır ve `as u16` onu sıfıra
    // doyurur — `Session::resize` o boyutu zaten yoksayıyor. `u16`'da
    // yapılsaydı taşar ve 65535 satırlık bir `TIOCSWINSZ` üretirdi.
    // Formül **bt-gpu'nun** ([`bt_gpu::dock_px`]): dock'un payı satırların
    // yanında iki nefes payı da taşıyor ve burada yeniden yazılsaydı yeniden
    // boyutlandırmada bir kare boyunca ayrışırdı — `DOCK_ROWS`'u tüketmekle
    // aynı disiplin, ikinci bir kopya tutulmuyor.
    let usable_height = height_px - f64::from(bt_gpu::dock_px(dock_rows, cell));
    Grid {
        cols: (usable_width / f64::from(cell_w)) as u16,
        rows: (usable_height / f64::from(cell_h)) as u16,
        cell,
    }
}

/// Uygulamanın delegate'i — pencereden uygulama geneline dönen yol.
///
/// Pencere uygulama delegate'ine referans **tutmuyor**: delegate süreç boyunca
/// yaşıyor ve `NSApp`'in `delegate` özelliğinden her seferinde bulunabiliyor,
/// yani saklanan bir referans yalnız bir çember ya da bir sarkma ihtimali
/// eklerdi. Çağıranları: ana kuyruk işleri (alternatif ekran habercisi, başlık
/// haberi, kabuğun çıkışı, kapanan pencerenin listeden çıkışı — kimlikten
/// pencereye), punto eylemleri (ayarın fontu) ve geometri (font tanısının alt
/// başlığı). `None` → delegate henüz bağlanmadı; çağıran sessizce düşüyor.
pub(crate) fn delegate(mtm: MainThreadMarker) -> Option<Retained<AppDelegate>> {
    let delegate = NSApplication::sharedApplication(mtm).delegate()?;
    let object: &AnyObject = (*delegate).as_ref();
    object.downcast_ref::<AppDelegate>().map(Message::retain)
}

/// İzleme kaynaklarının bildirimi ([`notify_settings_changed`]).
fn watch_notify() -> Notify {
    Arc::new(notify_settings_changed)
}

/// Bir izleme olayını uygulayıcıya taşır: **hedefsiz eylemle**
/// `settingsDidChange:`'e, görünüm değişiminin (`view.rs`) yolundan.
///
/// Hiçbir şey yakalamıyor: kaynağın context'inde bir delegate referansı
/// olsaydı iptal işleyicisi onu düşürür ve ömrünü libdispatch'in iptal
/// zamanlamasına bağlardı. Responder zinciri pencere key olmasa da (kullanıcı
/// editörde) `NSApp`'e ve onun delegate'ine varıyor.
fn notify_settings_changed() {
    // audit: kaynaklar yalnız `DispatchQueue::main()`'e kuruluyor
    // (`AppDelegate::watch_config`, `watch_theme`) ve ana kuyrukta koşan iş
    // tanımı gereği ana thread'dedir.
    let mtm = MainThreadMarker::new().expect("izleme kaynakları ana kuyrukta");
    let app = NSApplication::sharedApplication(mtm);
    // SAFETY: seçici geçerli; hedef `None` → responder zinciri. Alıcısı
    // `AppDelegate::settings_did_change`, tek `Option<&AnyObject>` argüman
    // alıyor ve gönderene bakmıyor. Alıcı yoksa (delegate henüz bağlanmadı)
    // `false` döner ve olay düşer; sonraki kayıt yine gelir.
    let _ = unsafe { app.sendAction_to_from(sel!(settingsDidChange:), None, None) };
}

/// Oturum doğarken ayrılacak dock payı (R5.1).
///
/// **İki koşul da gerekli ve ayrı sorular.** `integration` boşsa sarmalayıcı
/// hiç kurulmadı — hermetik koşu, `"off"`, tanımadığımız kabuk, UTF-8 olmayan
/// betik yolu — yani dock'u dolduracak ayna yok. `wants_dock` ise
/// **kullanıcının seçimi**: `"blocks"` kademesinde sarmalayıcı kuruluyor
/// (bloklar ve işaretler onun bütün gerekçesi) ama giriş satırı ızgarada
/// kalıyor, yani pay ayrılmıyor.
///
/// Birini ötekinden türetmek 012 phase-10'un kapattığı kusuru geri getirirdi:
/// ekranda **iki prompt** (kullanıcınınki ızgarada, dock'unki altta) ve
/// ikisi arasında sıçrayan bir caret.
fn dock_rows_at_birth(integration: &[(String, String)], setting: ShellIntegration) -> u16 {
    if integration.is_empty() || !setting.wants_dock() {
        0
    } else {
        DOCK_ROWS
    }
}

/// Bu anın dock payı: alternatif ekranda **sıfır**, değilse doğum değeri.
///
/// Doğum değeri ayrı bir girdi ve bu zorunlu: entegrasyonsuz bir oturumda
/// (`birth == 0`) alternatif ekrandan çıkmak dock **doğurmamalı**. Tek bir
/// `dock_rows` alanı üstüne yazılsaydı `DOCK_ROWS` sabitinden geri kurmak
/// gerekirdi ve o, olmayan bir dock'u var etmenin tam yolu.
///
/// Saf: `bt-shell`'in AppKit'siz sınanabilen tek yarısı burası.
pub(crate) fn dock_rows_for(alt_screen: bool, birth: u16) -> u16 {
    if alt_screen { 0 } else { birth }
}

/// Dosyayı kullanıcının editöründe açar; hiçbir yol açamadıysa `false`.
///
/// Önce dosya türünün varsayılan uygulaması (`NSWorkspace`, Finder'da çift
/// tıklamanın yolu). `.toml`'u sahiplenen uygulama her makinede yok — sistem
/// türü tanımasa da o türü kimse açmayabilir; o zaman varsayılan **metin**
/// editörü (`open -t`, çoğu makinede TextEdit). İkincisi bir alt süreç ve
/// dönüşü bekleniyor: `open` işi LaunchServices'e verip hemen çıkıyor.
///
/// **Bilinen sınır — ana thread bekler.** `open` editörü soğuk açarken
/// dönmüyor ve display link ana thread'de: o arada pencere kare çizmez, tuş
/// işlenmez (`/code-review` bulgusu, 007 kapıda waive). Yalnız `.toml`'u
/// sahiplenen uygulama yokken ve kullanıcının kendi tıklamasında; odak zaten
/// editöre geçiyor. Beklememek hatanın alt başlığa yolunu keserdi.
fn open_in_editor(path: &Path) -> bool {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        return true;
    }
    std::process::Command::new("/usr/bin/open")
        .arg("-t")
        .arg(path)
        .status()
        .is_ok_and(|status| status.success())
}

/// Basılı tutulan harfin **aksan popover'ını** kapatır: terminalde basılı
/// tuş **yineleme** demektir (vim'de `j`, kabukta `u`), popover onu çalar.
///
/// Yan etki `NSTextInputClient`'ın kendisiyle geliyor (018 phase-1): protokolü
/// uygulamayan bir view'da popover zaten çıkmıyordu.
///
/// Yazılan yer uygulamanın **kendi `registerDefaults`'ı**, yani bellekteki
/// registration domain: kullanıcının plist'i el değmeden kalıyor ve ayar
/// koşudan koşuya taşınmıyor. Gerekçe kabuğun rc dosyasına dokunmama
/// kuralının aynısı — kullanıcının dosyasına biz yazmayız. Kurulu bir ürünün
/// kanıtı aynı anahtarı gösteriyor: iTerm2 kendi domain'inde
/// `ApplePressAndHoldEnabled = 0` tutuyor.
///
/// **Yarısı ölçüldü** (2026-09-20): bu makinenin `NSGlobalDomain`'inde
/// `ApplePressAndHoldEnabled` **yok** (`defaults read -g`), yani arama
/// sırasında registration domain'in üstünde onu ezecek bir halka bulunmuyor
/// — set kapısının "NSGlobalDomain ya da MDM eziyor olabilir" itirazı bu
/// kurulumda konusuz.
///
/// **İkinci yarı da ölçüldü** (2026-09-20, kullanıcı gerçek pencerede):
/// harf basılı tutulunca popover **çıkmıyor**, yani AppKit kararı
/// `NSUserDefaults` üzerinden okuyor ve registration domain'i görüyor.
/// Kuşkunun kaynağı `CFPreferences`'a doğrudan bakma ihtimaliydi (iTerm2'nin
/// *kalıcı* domain değeri tutması o ihtimalin ipucuydu; ghostty aynı
/// `registerDefaults` yolunu kullanıyor) ve düştü. Hermetik sınama makineye
/// bağlı olurdu; kapatan şey `e`'yi basılı tutmaktı.
///
/// **Tutmasaydı belirti iki yarılı olurdu** ve ikincisi sessiz: gürültülü yarısı
/// basılı tuşun yinelememesi, sessiz yarısı popover'dan seçilen harfin
/// kabuğa **çift** gitmesi — o çağrı `insertText:"é" replacementRange:{n-1,1}`
/// oluyor ve `view::BateriView` aralığı atladığı için `eé` yazılıyor.
/// Popover'ı açan şey `NSTextInputClient`'ın kendisi, yani bu iki belirtiyi
/// de doğuran ve tek çaresi burada olan aynı değişiklik (018 phase-1).
fn disable_press_and_hold() {
    let key = ns_string!("ApplePressAndHoldEnabled");
    let off = NSNumber::numberWithBool(false);
    let defaults = NSDictionary::from_slices::<NSString>(&[key], &[off.as_ref()]);
    // SAFETY: sözlüğün anahtarı `NSString`, değeri property-list'e girebilen
    // bir `NSNumber` — `registerDefaults`'ın istediği tipler.
    unsafe { NSUserDefaults::standardUserDefaults().registerDefaults(&defaults) };
}

/// Delegate'in durumu — **uygulama geneli**. Pencere başına olan her şey
/// (pencere, view, yüzey, renderer, oturum, link, dock payı, geçici punto)
/// [`TerminalWindow`]'da; burada ayarlar, izleme kaynakları, alt başlık
/// yuvaları, ölçüm defteri, süreli koşunun tarifi ve pencere listesi.
pub(crate) struct Ivars {
    /// Süreli koşunun tarifi; `None` → kullanıcının kendi oturumu. Deadline,
    /// bekçi, sabit shell ve rapor **hep birlikte** buna bağlı.
    run: Option<Run>,
    /// Alt başlığın yuvaları; yazanı yalnız [`AppDelegate::post_notices`].
    /// Uygulama genelinde, çünkü kaynakları (ayar, tema, font, yazma) da öyle:
    /// her pencerenin alt başlığı aynı metni gösteriyor.
    notices: RefCell<Notices>,
    /// Geçerli ayarlar: açılışta [`AppDelegate::load_settings`], kayıtta
    /// [`AppDelegate::reload_settings`] yazar; görünüm uygulayıcısı
    /// `theme_for` için, Theme ▸ işaretli öğe için okur. Süreli koşuda varsayılanlar ve görünüm
    /// uygulayıcısı onları hiç okumaz (`Inputs::Hermetic`).
    ///
    /// Saklanıyor, çünkü görünüm değişimi dosyayı yeniden okumadan hangi
    /// temanın seçileceğini bilmeli ve canlı yenileme farkı buna karşı alıyor.
    /// Kullanılamayan bir kayıt onu **değiştirmez**: sonraki görünüm değişimi
    /// son iyi ayarlarla seçer.
    settings: RefCell<Settings>,
    /// Ayar dizininin kaynakları: kök, `themes/`, `settings.toml`
    /// ([`settings::watched_paths`]). Süreli koşuda ve ev dizini
    /// çözülemeyince hiç kurulmuyor.
    config_watch: RefCell<Option<Watch>>,
    /// Etkin kullanıcı temasının dosyası. Ayrı yuva, çünkü adı görünümle de
    /// değişiyor ve görünüm değişimi ayar dosyasını yeniden okumuyor; gömülü
    /// tema seçiliyse dosya yok ve yuva kaynaksız.
    theme_watch: RefCell<Option<Watch>>,
    /// Ölçüm defteri — kapı kapalıyken `None` ve hiç ayrılmamış.
    ///
    /// `bt-gpu`'nun tipi ama sahibi burası: `DisplayLink` ile tamamlanma bloğu
    /// birer kopyasını yazıyor, kapanışta okuyan (rapor) bu kopya.
    stats: Option<Arc<Stats>>,
    /// Açık pencereler (her sekme bir pencere). **Sahibi burası**: pencerenin
    /// delegate özelliği zayıf ve `TerminalWindow` başka hiçbir yerde
    /// tutulmuyor. Doğuran tek yol [`AppDelegate::open_window`]; kapanan
    /// pencere bir tur sonra çıkıyor ([`AppDelegate::forget_window`]).
    ///
    /// Kayıt anı yolları bu listeyi dolaşıyor ve dolaşırken **kopyasını**
    /// ([`AppDelegate::windows`]) alıyor: pencereye giden çağrı geri dönüp
    /// buraya uzanabiliyor (`sync_geometry` → [`AppDelegate::post_notices`]).
    windows: RefCell<Vec<Retained<TerminalWindow>>>,
    /// Pencere kimliklerinin sayacı ([`TerminalWindow::id`]); kimlik yeniden
    /// kullanılmıyor, yani kapanmış bir pencereye giden bayat bir haber başka
    /// bir pencereyi bulamaz.
    next_window_id: Cell<u64>,
    /// Son görülen sistem görünümü koyu muydu — [`AppDelegate::apply_appearance`]'ın
    /// kapısı. `None`: henüz hiç değişim gelmedi (ilk haber her zaman geçer).
    ///
    /// Kapı bir **tasarruf**, doğruluk şartı değil: KVO haberi görünümün adı
    /// değişince de geliyor (vurgu rengi, yüksek kontrast) ve tema yalnız
    /// açık/koyu bitine bağlı; bit aynıysa tema dosyasını yeniden okumanın ve
    /// bütün pencereleri yeniden boyamanın sebebi yok.
    appearance_dark: Cell<Option<bool>>,
    /// Ayar penceresi (bateri ▸ Settings…): ilk açılışta doğuyor, kapatınca
    /// gizleniyor ve süreç boyunca yaşıyor (029 Karar 4). Terminal penceresi
    /// **değil** — [`Ivars::windows`]'a girmiyor, yani ⌘Q'nun onayı, ayar
    /// yayılımı ve sekme işleri onu görmüyor. Süreli koşuda hiç doğmuyor.
    settings_window: RefCell<Option<Retained<SettingsWindow>>>,
    /// Ayar dosyasının son okunuşundaki hâli (029 Karar 7): ayar penceresinin
    /// kilidi ve satır tanıları buradan. Açılışta ve her canlı okumada
    /// yazılıyor, yani pencere sonradan açılsa da dosyanın hâlini görüyor.
    settings_state: RefCell<settings::FileState>,
}

define_class!(
    // SAFETY: NSObject alt sınıflama şartı taşımaz; AppDelegate Drop uygulamaz.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriAppDelegate"]
    #[ivars = Ivars]
    pub(crate) struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            let mtm = self.mtm();
            disable_press_and_hold();
            // Native sekmeler açık (026 → Karar 1): `setAllowsAutomaticWindowTabbing`
            // varsayılanında, pencereler ortak `tabbingIdentifier` taşıyor
            // (`TerminalWindow::new`).
            crate::menu::install(mtm, ProtocolObject::from_ref(self));
            // Ayarlar ilk pencereden **önce** okunuyor: `scrollback` ve tema
            // `SessionOptions`'a giriyor, font ayarı da hücre ölçüsünü, yani
            // ilk grid'i ve kabuğun gördüğü ilk `TIOCSWINSZ`'yi belirliyor.
            // Tanıların alt başlığa ulaşması için pencerenin önce doğması
            // artık gerekmiyor: yeni pencere alt başlığını yuvalardan
            // devralıyor (`open_window`).
            self.load_settings();
            NSApplication::sharedApplication(mtm).activate();
            // Renderer pencereyle birlikte doğuyor (026 → Karar 2a) ve hatası
            // buraya düşüyor. `didFinishLaunching` hata döndüremez; Metal'siz
            // ya da kabuksuz bir terminal penceresi boş bir kutudur ve eskiden
            // `run`'ın döndürdüğü hata `main`'de aynı satırla ve aynı çıkış
            // koduyla basılıyordu. **Yalnız ilk pencerede**: ⌘T/⌘N'nin hatası
            // süreci bitirmiyor ([`AppDelegate::open_window_or_report`]).
            if let Err(e) = self.open_window(None, false) {
                eprintln!("bateri: {e}");
                std::process::exit(1);
            }
            // Sistemin Hareketi Azalt bildirimi uygulama genelinde ve bir kez;
            // pencerenin ilk değeri `start`'ta kendi link'ine indi.
            self.observe_reduce_motion();
            // Açık/koyu görünüm de uygulama genelinde ve bir kez; ilk pencerenin
            // teması zaten görünümden seçildi (`open_window` → `resolve_theme`).
            self.observe_appearance();

            if let Some(run) = self.ivars().run {
                // Zamanlayıcı bir blok değil `performSelector`: seçici bu
                // sınıfta ve iptal edilmesi gerekmiyor.
                // SAFETY: `runDeadline:` bu sınıfta tanımlı ve tek
                // Option<&AnyObject> argüman alıyor. Delegate özellikleri zayıf
                // referanstır; self'i yaşatan `run()`'daki `Retained`, o da
                // `app.run()`'ı aşar. Zamanlayıcı ayrıca hedefini kendi tutar.
                // Common modes: canlı boyutlandırma run loop'u tracking moduna
                // sokar, varsayılan modda kurulan zamanlayıcı orada ertelenirdi.
                unsafe {
                    self.performSelector_withObject_afterDelay_inModes(
                        sel!(runDeadline:),
                        None,
                        run.seconds as f64,
                        &NSArray::from_slice(&[NSRunLoopCommonModes]),
                    );
                }
            }
        }

        /// Son pencere kapanınca uygulama **açık kalır** (026 → Karar 5):
        /// macOS'un çok pencereli uygulama geleneği; Dock ikonu ve ⌘N yeni
        /// pencere açıyor.
        ///
        /// **Süreli koşuda** `true` ve bu bir sözleşme: duman reçetesi
        /// deadline'dan kısa biterse rapor `child_exit` → `terminate:` yolundan
        /// basılıyor (`ShellWake::child_exit`) ve pencere o yolda listeden hiç
        /// düşmüyor; kapanan tek pencerede uygulama yine bitmeli.
        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window(&self, _app: &NSApplication) -> bool {
            self.ivars().run.is_some()
        }

        /// Dock ikonuna tıklandı. **Hiç pencere yoksa** yeni pencere açılır ve
        /// AppKit'in varsayılanı atlanır; pencere varsa (simge durumunda da)
        /// varsayılan kalır — AppKit simge durumundakini geri getiriyor, ve
        /// yenisini açmak kullanıcının küçülttüğü oturumu gizlemek olurdu.
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_handle_reopen(&self, _app: &NSApplication, _has_visible_windows: bool) -> bool {
            // `return` yok: `define_class!` gövdenin son ifadesini `Bool`'a
            // çeviriyor, erken `return`'ün `bool`'unu çevirmiyor.
            //
            // Ölçüt yalnız terminal penceresi listesi: `has_visible_windows`
            // About paneli gibi terminal olmayan pencereyi de sayıyor ve açık
            // bir panel yeni pencereyi engellerdi (`/code-review`); liste simge
            // durumundakileri zaten kapsıyor.
            let default = !self.ivars().windows.borrow().is_empty();
            if !default {
                self.open_window_or_report(None, false);
            }
            default
        }

        /// ⌘Q, Dock ▸ Quit, oturum kapatma ve yeniden başlatma: çıkmadan önce
        /// sorulsun mu (028 → Karar 3, 5). Soru bütün pencereler için **tek**
        /// uyarı; `runModal` eşzamanlı, yani cevap doğrudan dönüyor ve
        /// `NSTerminateLater` gerekmiyor.
        ///
        /// Süreli koşu **ilk satırda** ve süreç tablosuna dokunmadan geçiyor:
        /// kabuğun `exit`'i orada `child_exit` → `terminate:` ile buraya varıyor
        /// ve başsız bir `runModal` bekçi kurulmadan asılırdı.
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            self.terminate_reply()
        }

        /// AppKit'in kapanış yolu: bateri ▸ Quit (Cmd-Q, menüden `terminate:`)
        /// ve süreli koşuda `exit` yazan shell (`child_exit` → `terminate:`)
        /// buraya varır; etkileşimli oturumda kırmızı düğme ve `exit` yalnız o
        /// pencereyi kapatıyor (`TerminalWindow`'un `windowWillClose:`'u).
        /// Koşan iş varken Cmd-Q önce sorar (`applicationShouldTerminate:`);
        /// buraya varıldıysa karar verilmiştir.
        /// Duman deadline'ı buraya uğramaz, `terminate:` her zaman 0 ile
        /// çıkar ve `runDeadline:` kırmızı düşebilmek zorunda. Ortak olan
        /// bildirim değil sıra: iki yol da [`AppDelegate::shutdown`] çağırır ve
        /// kapanışa eklenecek her adım oraya eklenir.
        #[unsafe(method(applicationWillTerminate:))]
        fn will_terminate(&self, _n: &NSNotification) {
            // Damga kapanıştan **önce**, `runDeadline:`'daki gerekçeyle:
            // `shutdown()` yarım saniyeye kadar bekleyebiliyor ve o bekleme
            // sessizliğe yazılırsa jeton ölçtüğünü sandığı şeyi ölçmez.
            //
            // Kapı **okumadan önce** soruluyor: `quiet_since` bir saat okuması
            // (`CACurrentMediaTime`) ve süresiz koşuda bu değer atılacak.
            // "Kapı kapalıyken tek bir saat okuması bile yok" (`CLAUDE.md`)
            // Cmd-Q yolunda da geçerli; aşağıdaki `if let` tek başına değeri
            // atıyordu ama okumayı engellemiyordu.
            let quiet = self
                .ivars()
                .run
                .is_some()
                .then(|| self.quiet_since())
                .flatten();
            let teardown = self.shutdown();
            // Duman koşusu deadline'a varmadan da bitebilir: shell kendi
            // çıkarsa (`BT_RUN_SECONDS` betiğin uykusundan uzunsa, ya da
            // gerçek bir shell hemen ölürse) `ChildExit` buraya getirir.
            // Rapor basılmadan çıkmak `make duman`'a hiçbir şey ölçmemiş bir
            // koşuyu exit 0 ile yeşil gösterirdi — kapının sahte yeşil verdiği
            // tek yol buydu.
            if let Some(run) = self.ivars().run {
                self.report_and_exit(run, teardown, quiet);
            }
        }
    }

    unsafe impl NSMenuDelegate for AppDelegate {
        /// Theme ▸ açılıyor — delegate yalnız ona bağlı (`menu::install`).
        /// Liste o anda kuruluyor: `themes/`'e konan dosya bir sonraki
        /// açılışta görünür, dizin liste için izlenmiyor. İşaretli öğe
        /// geçerli ayardaki `theme`.
        ///
        /// Süreli koşuda ve ev dizini çözülemeyince doldurulmaz
        /// ([`Inputs`]): seçimin yazacağı bir dosya yok.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            let Inputs::User {
                config_root: Some(root),
            } = self.inputs()
            else {
                return;
            };
            let embedded: Vec<&str> = Theme::embedded_names().collect();
            let user = settings::user_theme_names(&root);
            let settings = self.ivars().settings.borrow();
            crate::menu::fill_themes(self.mtm(), menu, &settings.theme, &embedded, &user);
        }

        /// "Bu menüde şu tuşun karşılığı var mı": hayır, tema öğelerinin
        /// kısayolu yok.
        ///
        /// Tanımlanmasının tek sebebi maliyet: delegate bunu tanımlamazsa
        /// AppKit her Command'lı tuşta (Cmd-C dahil) karşılığı aramak için
        /// menüyü `menuNeedsUpdate:` ile doldurur — her tuşta `themes/`
        /// okunurdu. `objc2-app-kit` bu yöntemi üretmiyor (dönüş işaretçili
        /// argümanlar); imza elle. İki çıkış argümanı (`id *`, `SEL *`) opak
        /// işaretçi: `Sel` işaretçi kodlaması taşımıyor ve `false` dönen yöntem
        /// onlara hiç yazmıyor.
        #[unsafe(method(menuHasKeyEquivalent:forEvent:target:action:))]
        fn menu_has_key_equivalent(
            &self,
            _menu: &NSMenu,
            _event: &NSEvent,
            _target: *mut c_void,
            _action: *mut c_void,
        ) -> bool {
            false
        }
    }

    impl AppDelegate {
        /// KVO: `NSApp.effectiveAppearance` değişti — sistemin açık/koyu
        /// görünümü ([`AppDelegate::observe_appearance`]). Bu sınıfın izlediği
        /// tek anahtar yolu bu, yani yol ve nesne sorulmuyor.
        #[unsafe(method(observeValueForKeyPath:ofObject:change:context:))]
        fn observe_value(
            &self,
            _key_path: Option<&NSString>,
            _object: Option<&AnyObject>,
            _change: Option<&AnyObject>,
            _context: *mut c_void,
        ) {
            self.apply_appearance();
        }

        /// Bir izleme kaynağı haber verdi (`notify_settings_changed`,
        /// hedefsiz eylem): ayar ya da tema dosyası kaydedildi.
        #[unsafe(method(settingsDidChange:))]
        fn settings_did_change(&self, _sender: Option<&AnyObject>) {
            self.reload_settings();
        }

        /// macOS'un erişilebilirlik görüntü ayarları değişti; gönderen
        /// `NSWorkspace`'in **kendi** bildirim merkezi
        /// ([`AppDelegate::observe_reduce_motion`]).
        ///
        /// Bildirim Hareketi Azalt'a özel değil — kontrast, saydamlık ve renk
        /// ayrımı da buradan geliyor. Ayırt etmeye gerek yok: aşağıdaki yol
        /// değeri yeniden okuyor ve değişmediyse link'e giden çağrı zaten
        /// no-op (`Motion::set_reduce`).
        #[unsafe(method(accessibilityDisplayDidChange:))]
        fn accessibility_display_did_change(&self, _note: Option<&AnyObject>) {
            // audit: bu yol ana thread'i **yapısal olarak** garanti etmiyor —
            // `NSNotificationCenter` gözlemcisi yayınlayan thread'de senkron
            // ateşliyor ve `NSWorkspace`'in merkezi bunu sözleşmeye bağlamıyor
            // (`/audit` bulgusu). Altındaki iş ise ana thread varsayıyor:
            // `settings`'in `RefCell`'i ve link'in `Cell<Motion>`'ı. İddia bu
            // yüzden kodda duruyor — yanlışsa belirti sessiz bir veri yarışı
            // değil, burada patlayan bir panik olur.
            let _mtm = MainThreadMarker::new()
                .expect("erişilebilirlik bildirimi ana thread'de bekleniyor");
            self.apply_reduce_motion();
        }

        /// Shell ▸ New Window (⌘N): etkin pencerenin dizininde ve punto
        /// farkıyla yeni bir pencere (026 → Karar 3, 4). Burada, pencerede
        /// değil: pencere yokken de çalışmalı.
        #[unsafe(method(newWindow:))]
        fn new_window(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(false);
        }

        /// Shell ▸ Close Tab (⌘W) terminal olmayan bir pencere key iken (About
        /// paneli): responder zinciri onu buraya getiriyor ve o pencere
        /// AppKit'in kendi yolundan kapanıyor. Terminal penceresinde eylemi
        /// pencerenin delegate'i önce karşılıyor (`TerminalWindow`'un
        /// `closeTab:`'ı) — menü `performClose:`'dan ayrılınca ⌘W panellerde
        /// sessizce ölmesin diye (`/code-review`).
        #[unsafe(method(closeTab:))]
        fn close_tab(&self, _sender: Option<&AnyObject>) {
            if let Some(key) = NSApplication::sharedApplication(self.mtm()).keyWindow() {
                key.performClose(None);
            }
        }

        /// Shell ▸ New Tab (⌘T): etkin pencerenin grubuna yeni sekme; pencere
        /// yoksa yeni pencere.
        #[unsafe(method(newTab:))]
        fn new_tab(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(true);
        }

        /// Sekme çubuğunun `+` düğmesi. AppKit düğmeyi yalnız responder
        /// zincirinde bu seçiciyi tanıyan biri varsa gösteriyor; iş ⌘T'ninki.
        #[unsafe(method(newWindowForTab:))]
        fn new_window_for_tab(&self, _sender: Option<&AnyObject>) {
            self.open_from_key_window(true);
        }

        /// bateri ▸ Settings… (Cmd-,), hedefsiz menü öğesinden (`menu`):
        /// ayar penceresini açar ya da öne getirir.
        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: Option<&AnyObject>) {
            self.show_settings_window();
        }

        /// View ▸ Theme ▸ {ad}: öğenin başlığı temanın adı
        /// (`menu::fill_themes`).
        #[unsafe(method(selectTheme:))]
        fn select_theme(&self, sender: Option<&AnyObject>) {
            let Some(item) = sender.and_then(|sender| sender.downcast_ref::<NSMenuItem>()) else {
                return;
            };
            self.save_theme(&item.title().to_string());
        }

        /// View ▸ Theme ▸ Match System.
        #[unsafe(method(matchSystemTheme:))]
        fn match_system_theme(&self, _sender: Option<&AnyObject>) {
            self.save_theme(SYSTEM_THEME);
        }

        #[unsafe(method(runDeadline:))]
        fn run_deadline(&self, _arg: Option<&AnyObject>) {
            // Sessizlik damgası kapanıştan **önce** okunuyor ve sıra
            // bilinçli: `shutdown()` en çok `SHUTDOWN_GRACE` (yarım saniye)
            // bekliyor ve ölçüm koşularının dörtte birinde gerçekten
            // bekliyor (`kapanis=abandoned`). Sonra okunsaydı `sessiz=`
            // "son kare → deadline" değil "son kare → kapanışın sonu" olurdu
            // ve kapının tabanına kapanış değişkenliği karışırdı
            // ([`QUIET_FLOOR`] ölçülen dağılımın yarısında duruyor; yarım
            // saniyelik bir kapanış beklemesi o payı tek başına yerdi).
            let quiet = self.quiet_since();
            let teardown = self.shutdown();
            // Zamanlayıcı yalnız `run` doluyken kuruldu; `if let` burada bir
            // dal değil o değişmezin okunması. `expect` olmadı, çünkü burası
            // rapor yolu ve kapanışta bir panik raporun kendisini yutardı.
            if let Some(run) = self.ivars().run {
                self.report_and_exit(run, teardown, quiet);
            }
        }
    }
);

/// Duman **kapısının** sayaçları — satırın hepsi değil, [`verdict`]'in gördüğü
/// kadarı.
///
/// Yapı, çünkü hepsi sayı: konumsal geçirilseler `hucre` ile `glif` yer
/// değiştirdiğinde **derleme geçerdi** ve sınama da aynı sırayı kullandığı
/// için ikisi birlikte yanılırdı (`/code-review` bulgusu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counters {
    /// GPU'nun hatasız bitirdiği kare.
    frames: u64,
    /// Çizilmeye **karar verilen** içerik karesi: boşta sıfır kare kapısının
    /// operandı ([`IDLE_FRAME_LIMIT`]). `frames`'in yerine geçmiyor, yanına
    /// geliyor — ikisi ayrı soru yanıtlıyor ve jeton ikisini de basıyor.
    content: u64,
    /// Son karede sink'in ürettiği arka plan hücresi (imleç hariç).
    cells: usize,
    /// Son karede çizilen glyph.
    glyphs: usize,
    /// Son karede çizilen alt çizgi / üstü çizili.
    rules: usize,
    /// Yerleşmemiş **imleç** animasyonu yüzünden çizilen kare — `slide`'ın
    /// ikizi değil tamamlayıcısı: ötekini yalnız öteleme, bunu yalnız imleç
    /// artırıyor ve bir karede ikisi birden artabilir.
    ///
    /// `content`'in kardeşi ve kapıda **ters yönde**: `content`'in bir üst
    /// sınırı var, bunun bir **alt** sınırı (`> 0`). Duman reçetesi bir imleç
    /// hareketi içeriyor (`bt_core::smoke_shell`), yani sıfır "animasyon hiç
    /// koşmadı" demek — tıpkı `hucre=0`'ın "shell çıktısı yok" demesi gibi.
    ///
    /// **Gizli bağ, artık adıyla:** bu gereklilik hermetik koşunun imleç
    /// stilinin **animasyonlu** olmasına dayanıyor ve o stil
    /// `bt_core::Settings::default().cursor_motion`, yani varsayılanların tek
    /// sahibinden geliyor (süreli koşu ayar dosyasını okumuyor,
    /// [`Inputs::Hermetic`]). Varsayılan bir gün `CursorMotion::Snap` olursa
    /// bu kapı sessizce düşer — o değişiklik ya hermetik koşunun stilini
    /// koşuda açıkça sabitlemek zorunda ya bu cümleyi karşısında bulacak.
    ///
    /// **Blink'i saymıyor** (014 phase-2): imlecin yanıp sönmesi
    /// `bt_gpu::motion`'ın dışında yaşıyor, yani `cursor_settled()` onu hiç
    /// görmüyor ve bu sayaç artmıyor. Blink karesinin **hiçbir CPU tanığı
    /// yok** — `istek=` de artmıyor (`Waker::resume` sayaca dokunmuyor),
    /// `icerik=` de (tasarımın amacı bu). Jeton **bilerek eklenmedi**
    /// (`cpu_elenen=` emsali, aşağıda): varsayılan kapalı olduğu için
    /// gözlenebilir her koşuda sıfır basardı ve jeton silinmiyor, ekleniyor.
    /// Bozuk bir blink'i kapının hiçbir katı görmez; **koruma bir jeton değil
    /// varsayılanın kendisi** ve bu, setin `teslim.md`'sinde yazılı.
    motion: u64,
    /// Yerleşmemiş **kayma** (içeriğin ötelemesi) yüzünden çizilen kare.
    ///
    /// `motion`'ın kardeşi ve **kapıda yok**: reçetenin imleç hareketi
    /// garantili (`bt_core::smoke_shell`) ama kaymanın orada doğup doğmayacağı
    /// ölçülmedi ve ölçülmemiş sayı kapıya yazılmaz. Satırda olmasının sebebi
    /// tanı: kırmızı bir koşuda `hareket` ile birlikte okunduğunda hangi
    /// animatörün yerleşmediği ayırt edilebiliyor.
    ///
    /// İkisi toplanıp çizilen kareyi **vermiyor**: aynı karede ikisi birden
    /// artabilir.
    slide: u64,
}

/// Deadline'da animasyonun hâli — kapının **ölçüm istemeyen** yarısı.
///
/// `bool` değil ve sebebi çağrı yeri: [`verdict`] zaten beş sayı alıyor ve
/// çıplak bir `true` orada hangi soruyu yanıtladığını söylemezdi. [`Counters`]
/// da değil, çünkü bu bir sayı değil bir **durum**: yerleşmemiş animasyon
/// koşuyu kırmızı düşürüyor, sayısı değil varlığı önemli.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MotionState {
    Settled,
    Unsettled,
}

/// Ölçüm defterinin kapanıştaki özeti: halkadan okunmuş, henüz biçimlenmemiş.
///
/// Sayaç yarısı (`ornek`, `dusen`, `elenen`) p95'ten **önce** okunuyor, çünkü
/// [`bt_gpu::Samples::p95_and_worst`] kendini tüketiyor — sıra tipin
/// zorladığı bir şey, yorumun değil.
///
/// # Ölçümün dürüst sınırları
///
/// **Liste buradan taşındı.** Sahibi 2026-09-21'den beri
/// `docs/OLCUMLER.md` → `## Yöntem` → "Kare süresi ve açılış"; o türün ilk
/// `/measure`'ı taşımayı yaptı ve aynı koşu yedinci bir kalem ekledi (GPU
/// sütununun taban olacak kadar kararlı olmaması). Burada **kopyası tutulmuyor**: iki yerde
/// duran bir liste sessizce ayrışır, ki bu maddenin kendi uyarısıydı.
///
/// Aşağıdaki alan doc'ları o listenin **kapsam** kalemlerinden yalnız kendi
/// alanına düşeni tekrar ediyor; tamamı ve **açık kalemler** o dosyada.
///
struct Measured {
    /// `main()`'in ilk satırından ilk **tamamlanan** kareye. `None` → hiç kare
    /// bitmedi; iki ucun sınırı [`bt_gpu::Stats::startup`]'ta.
    startup: Option<Duration>,
    /// CPU sütunlarının uzunluğu. Tek sayı, çünkü iki CPU sütunu birlikte
    /// yazılıyor (`Stats::record_cpu`) ve hizaları tipte bağlı.
    cpu_samples: usize,
    /// Halkaya sığmayıp düşen örnek — üç sütunun en yükseği.
    ///
    /// Kuralı ve gerekçesi halkanın yanında ([`bt_gpu::Samples::dropped`]);
    /// burada yalnız uygulanıyor ve **elde duran anlık görüntülerden**:
    /// taze bir okuma, `ornek=` ile `dusen=`'i aynı andan almazdı.
    dropped: u64,
    /// GPU sütununun uzunluğu — CPU'dan **kısa olabilir**.
    gpu_samples: usize,
    /// Metal'in sıfır/NaN damgası yüzünden hiç yazılamayan kare. Bu sayı
    /// olmadan boş bir GPU sütunu "donanım damga vermiyor" ile "hiç kare
    /// çizilmedi"den ayırt edilemezdi.
    gpu_rejected: u64,
    cpu_frame: Option<(Duration, Duration)>,
    cpu_encode: Option<(Duration, Duration)>,
    gpu: Option<(Duration, Duration)>,
}

impl Measured {
    /// Defteri okur. Kapanışta bir kez koşuyor.
    ///
    /// `link.stop()` çoktan çağrıldı, ama **halkaların hepsi durağan değil**:
    /// iki CPU sütununu bu thread yazıyor (yani onlar durağan), GPU sütununu
    /// Metal'in tamamlanma thread'i yazıyor ve uçuşta kalan bir kare rapor
    /// okunurken hâlâ düşebilir. Sonucu bir örneklik kayma: `kare` ile
    /// `gpu_ornek + gpu_elenen` bu yüzden bire kadar ayrışabilir. `ornek=`
    /// jetonu bunu görünür kılıyor; sayıyı yorumlayan taraf eşitlik
    /// beklememeli.
    fn read(stats: &Stats) -> Self {
        let cpu_frame = stats.cpu_frame();
        let cpu_encode = stats.cpu_encode();
        let gpu = stats.gpu();
        Self {
            startup: stats.startup(),
            cpu_samples: cpu_frame.nanos.len(),
            dropped: cpu_frame.dropped.max(cpu_encode.dropped).max(gpu.dropped),
            gpu_samples: gpu.nanos.len(),
            gpu_rejected: gpu.rejected,
            cpu_frame: cpu_frame.p95_and_worst(),
            cpu_encode: cpu_encode.p95_and_worst(),
            gpu: gpu.p95_and_worst(),
        }
    }
}

/// Başarı satırının **bütün** girdisi.
///
/// Satırı kuran fonksiyonun saf olması gerekiyordu: gerçek bir pencere ve
/// display link olmadan sınanabilsin diye. Altı ayrı argüman olarak
/// geçirilseydi `Counters`'ın kaçtığı hatayı bir üst katmanda tekrarlardı.
struct Report {
    counters: Counters,
    /// Atlasın **maske** düzleminin dolu/toplam yuvası. Bir kapı **değil**,
    /// sayaç.
    atlas: (usize, usize),
    /// Atlasın **renk** düzleminin dolu/toplam yuvası; ikinci bir jeton
    /// (`yuva2=`) olmasının gerekçesi `Atlas::color_occupancy`'nin doc'unda.
    /// Jeton **eklendi, silinmedi**: `yuva=` yerinde ve anlamı değişmedi.
    color_atlas: (usize, usize),
    workload: Workload,
    /// Koşu boyunca istenen kare — çizilen değil.
    requests: u64,
    /// Son çizilen kareyle deadline arasındaki süre; `None` → hiç kare
    /// çizilmedi (`sessiz=none`). **Kapı** ([`QUIET_FLOOR`]): duman yükünde
    /// tabanın altı da `None` de kırmızı.
    quiet: Option<Duration>,
    /// Kapanışın sonucu; `None` → oturum hiç doğmamıştı.
    teardown: Option<Teardown>,
    /// Ölçüm defteri; `None` → kapı kapalıydı (`BT_FRAME_STATS` verilmedi).
    measured: Option<Measured>,
}

impl Report {
    /// Başarı satırı — **saf**, yani gerçek bir pencere olmadan sınanabilir.
    ///
    /// Jeton sözleşmesi: **silinmez, eklenir.** Eski beşli (`kare`, `hucre`,
    /// `glif`, `kural`, `yuva`) ve `yuk` ile `pipeline=ok` yerinde; yenisi
    /// aralarına giriyor.
    ///
    /// **Dil kuralı, tek yerde:** *anahtarlar* Türkçe ve **donmuş** — sözleşme
    /// "silinmez" diyor, yani bugün `kare=`'yi İngilizceleştirmek onu okuyan
    /// her tarafı kırar. *Değerler* İngilizce, çünkü onları okuyan şey bir tanı
    /// metni değil bir `match` kolu ya da CI grep'i (`yuk=smoke|load` ve
    /// `pipeline=ok` bu deseni bu satır doğmadan önce kurmuştu). Türkçe kalan
    /// tek yer **tanı metni**: stderr satırları ve `assert!` gerekçeleri.
    ///
    /// Satır **açıkça** basılıyor (`report_and_exit`'te bir
    /// `println!`); `Drop`'ta boşalan bir tampona bırakılan hiçbir yol yok —
    /// `process::exit` `Drop` koşturmuyor, bekçinin `_exit(70)`'i atexit'i
    /// bile atlıyor (R5.5).
    fn token_line(&self) -> String {
        let Counters {
            frames,
            content,
            cells,
            glyphs,
            rules,
            motion,
            slide,
        } = self.counters;
        let (used, total) = self.atlas;
        let (color_used, color_total) = self.color_atlas;
        // `profil=` kapı kapalıyken de basılıyor: `make duman` **debug**
        // koşuyor, `/measure` **release** şart koşuyor ve bir debug sayısını
        // taban sanmak ancak satırın kendisi profilini söylerse imkânsız olur
        // (R5.3).
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        // `icerik`/`hareket`/`kayma`/`sessiz` dörtlüsü `istek=`'in yanına
        // giriyor: hepsi kare **muhasebesi** ve satırı okuyan taraf onları bir
        // arada istiyor. `kayma` `hareket`'in yanında, çünkü ikisi aynı soruyu
        // iki animatör için yanıtlıyor. Baştaki dört sayaç yerinde kalmak
        // **zorunda** (`smoke_counts_unchanged`).
        let mut line = format!(
            "kare={frames} hucre={cells} glif={glyphs} kural={rules} \
yuva={used}/{total} yuva2={color_used}/{color_total} yuk={workload} \
istek={requests} icerik={content} \
hareket={motion} kayma={slide} sessiz={quiet} kapanis={teardown} \
profil={profile}",
            workload = self.workload.token(),
            requests = self.requests,
            // **`sessiz=0` değil:** sıfır, "deadline anında kare akıyordu"
            // demek ve hiç kare çizilmemiş bir koşuyla karışırdı — `ornek=off`
            // ile aynı kural, uydurulmuş bir sayı yerine yokluğun kendi
            // kelimesi.
            quiet = self.quiet.map_or_else(|| "none".to_owned(), ms),
            teardown = teardown_token(self.teardown),
        );
        match &self.measured {
            // Kapı kapalıydı. **`ornek=0` değil:** sıfır, "kapı açıktı ama hiç
            // örnek toplanmadı" ile aynı görünürdü ve R5.2'nin kapatmak
            // istediği körlük tam olarak o. Ölçüm jetonları da hiç basılmıyor;
            // sözleşme jetonun yokluğunu okumaya izin veriyor, yalan bir
            // değeri değil.
            None => line.push_str(" ornek=off"),
            Some(m) => {
                // `write!` bir `String`'e hata döndüremez; `let _` onu
                // görünür kılıyor ve rapor yolunda `unwrap` bırakmıyor.
                let _ = write!(
                    line,
                    " ornek={} dusen={} gpu_ornek={} gpu_elenen={} taban={MIN_SAMPLES}",
                    m.cpu_samples, m.dropped, m.gpu_samples, m.gpu_rejected
                );
                push_span(&mut line, "cpu_kare", m.cpu_frame);
                push_span(&mut line, "cpu_encode", m.cpu_encode);
                push_span(&mut line, "gpu", m.gpu);
                // Açılış tek sayı, dağılım değil: koşu başına bir kez olur.
                let _ = match m.startup {
                    Some(startup) => write!(line, " acilis={}", ms(startup)),
                    None => write!(line, " acilis=none"),
                };
            }
        }
        line.push_str(" pipeline=ok");
        line
    }
}

/// Bir sütunun iki jetonu.
///
/// Taban altında sayı **yok** (R5.6): `insufficient` basılır ve sebebi aynı
/// satırdaki `ornek=`/`gpu_ornek=` ile `taban=` çiftinde okunur. İkisi
/// **birlikte** susuyor, çünkü ikisi de aynı `Option`'dan geliyor: taban
/// altında p95 zaten en kötünün kopyasıdır, yani basılacak iki sayı değil bir
/// sayı ve iki ad olurdu.
fn push_span(line: &mut String, name: &str, span: Option<(Duration, Duration)>) {
    let _ = match span {
        Some((p95, worst)) => write!(line, " {name}_p95={} {name}_max={}", ms(p95), ms(worst)),
        None => write!(line, " {name}_p95=insufficient {name}_max=insufficient"),
    };
}

/// Süreyi jeton değerine çevirir: iki ondalıklı milisaniye.
///
/// Tek biçim, `acilis=` dâhil. İki ayrı hassasiyet okuyanı jeton başına kural
/// ezberlemeye zorlardı; makine sözleşmesinin istediği tam tersi.
fn ms(value: Duration) -> String {
    format!("{:.2}ms", value.as_secs_f64() * 1e3)
}

/// Sessizliğin **tanı** hâli: jeton değil, cümlenin içinde okunan bir öbek.
///
/// Jeton satırı yalnız yeşil koşuda basılıyor ([`Report::token_line`]), yani
/// düşen bir koşunun `sessiz`i hiçbir yerde görünmüyordu. `sessiz ≥ T` kapısı
/// ise **iki** dağılımdan türüyor ve ikincisi tam olarak düşen koşuların:
/// kasıtlı bozulmuş bir kol `sessiz`ini basmasaydı `T` tek yandan türetilir,
/// yani alt sınırı ölçülmemiş bir sayı olurdu (008 phase-6).
///
/// Ayrı fonksiyon olmasının sebebi jeton sözleşmesi: satırın `sessiz=`'i
/// makine tarafından okunuyor ve bu öbek ona **benzememeli** — `sessiz=`
/// arayan bir CI adımı düşen koşudan sayı okumasın.
fn quiet_phrase(quiet: Option<Duration>) -> String {
    quiet.map_or_else(
        || "hiç çizilen kare yok".to_owned(),
        |q| format!("son kareden sonra {} sessizlik", ms(q)),
    )
}

/// `kapanis=` jetonunun değeri.
///
/// Sınır dolan koşu ve panikle biten okuyucu bugüne kadar **yeşil bir
/// satırla** geçiyordu: stderr'de bir satır vardı, jetonda iz yoktu. Her
/// sonuç ayrı bir kelime, çünkü ayrı arıza — `bool` olsaydı okuyan taraf
/// hangisi olduğunu satırın dışında aramak zorunda kalırdı.
///
/// Değerler İngilizce ve varyant adının `kebab-case` hâli; kuralın gerekçesi
/// tek yerde, [`Report::token_line`]'ın doc'unda.
fn teardown_token(teardown: Option<Teardown>) -> &'static str {
    match teardown {
        // Oturum hiç doğmadı: kapanacak bir şey de yoktu.
        None => "none",
        Some(Teardown::Clean) => "clean",
        Some(Teardown::ReaderPanicked) => "reader-panicked",
        Some(Teardown::Abandoned) => "abandoned",
        Some(Teardown::Panicked) => "panicked",
        Some(Teardown::Unbounded) => "unbounded",
        Some(Teardown::AlreadyDone) => "already-done",
    }
}

/// Duman kapısının kararı.
///
/// `bool` **değil**: hata yolunun iki ayrı iletisi var ve `bool` onları
/// kapının dışında yeniden türetmeye zorlardı. Politika o zaman üç yere
/// dağılırdı (başarı satırı, "sıfır" iletisi, "fazla" iletisi) ve yalnız biri
/// sınanmış olurdu — kapı düşerken yanlış arızayı tarif eden bir koşu tam da
/// böyle doğar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    /// Sayaçlardan biri sıfır: pipeline'ın bir halkası hiç çalışmamış.
    /// `required` iletinin gereklilik yarısı ve yüke göre değişiyor — `Load`
    /// düz metin akıtıyor, orada `hucre` ile `kural` yapısal olarak sıfır.
    MissingCounter {
        required: &'static str,
    },
    /// Kare sayısı üst sınırı aştı: boşta sıfır kare bozulmuş.
    ExcessFrames {
        limit: u64,
    },
    /// Deadline'da yerleşmemiş bir animasyon vardı: durma koşulu bozulmuş.
    ///
    /// [`ExcessFrames`](Verdict::ExcessFrames)'in **tamamlayıcısı**, kopyası
    /// değil: o, sınırı aşacak kadar hızlı akan kareyi görüyor ve üç saniyelik
    /// bir koşuda ancak ~3 Hz'in üstünü yakalıyor; bu ise hızdan **bağımsız**.
    /// Durma koşulu unutulmuş 0,2 Hz'lik bir animasyon hiçbir kare sınırını
    /// aşmaz ama deadline'da hâlâ yerleşmemiş olur ve pil sözleşmesini tam da
    /// o ihlal eder.
    ///
    /// Yalnız [`Workload::Smoke`]'ta soruluyor: ölçüm yükü deadline'a kadar
    /// çıktı akıtıyor, yani son satırla birlikte imleç hedef değiştiriyor ve
    /// deadline yayın ortasına düşüyor. O kola bağlansaydı her ölçüm koşusu
    /// kod doğruyken kırmızı düşerdi — `ExcessFrames`'in aynı kolda muaf
    /// olmasının gerekçesiyle aynı.
    MotionUnsettled,
    /// Son kareyle deadline arasındaki sessizlik ölçülmüş tabanın altında —
    /// ya da hiç kare çizilmedi ([`QUIET_FLOOR`]).
    ///
    /// Sızıntı kollarının **sonuncusu** (arkasında yalnız
    /// [`ShutdownPanicked`](Verdict::ShutdownPanicked) var, o da koşunun
    /// ölçtüğü şeyi değil kapanış yolunu anlatıyor): yukarıdaki iki kol
    /// sızıntıyı ya hızından (`ExcessFrames`) ya da hareket altyapısından
    /// (`MotionUnsettled`) tanıyor; bu ise ikisini de atlayan bir yolu —
    /// altyapıya uğramadan, sınırı aşmayacak kadar seyrek kare isteyen kodu —
    /// yalnız bıraktığı izden tanıyor. Sıranın gerekçesi `verdict`'in
    /// gövdesinde, kolları `a_short_tail_fails_the_gate` çiviliyor.
    QuietTooShort {
        floor: Duration,
    },
    /// Kapanış yolunda **panik** oldu. Sayaçlar yerinde olabilir ama koşu
    /// yeşil geçemez: projenin "PTY ve ayrıştırma yolunda panik yok" kuralı
    /// ihlal edilmiş demektir ve `kapanis=` jetonunu **hiç kimse okumasa**
    /// bile kapı bunu görmek zorunda.
    ///
    /// [`Teardown::Abandoned`] ve [`Teardown::Unbounded`] buraya **girmez**:
    /// ikisi de kayıtlı borç (çocuk çıkışın içinde takılıyor; OS thread
    /// sınırı) ve ilki ölçüm yükünün dört koşusundan birinde oluyor — kapıya
    /// bağlansaydı `make duman` bilinen bir borç yüzünden kırmızı düşerdi.
    ShutdownPanicked {
        which: &'static str,
    },
}

/// Kapının saf hâli — gerçek bir display link ve pencere istemeden sınanır.
///
/// Karar [`AppDelegate::report_and_exit`]'in gövdesinde kalsaydı sınırın
/// yönünü (8 mi 180 mi, `Load` muaf mı) yalnız `make duman` bilirdi ve hiçbir
/// sınamada yazılı olmazdı.
fn verdict(
    counters: Counters,
    workload: Workload,
    teardown: Option<Teardown>,
    motion: MotionState,
    quiet: Option<Duration>,
) -> Verdict {
    let Counters {
        frames: n,
        content: c,
        cells: k,
        glyphs: g,
        rules: r,
        motion: m,
        // Kapıda **yok** ve bu bilinçli: reçetenin kayma üretip üretmediği
        // ölçülmedi ([`Counters::slide`]). Jeton yine de basılıyor — tanı için.
        slide: _,
    } = counters;
    // Panik **en sonda** soruluyor ve bu kolların sırası bir tanı tercihi,
    // kapı kararı değil: hangi kol seçilirse seçilsin koşu kırmızı ve çıkış 1.
    // Sıra "daha temel arıza önce" diye kuruldu — eksik sayaç (bir halka hiç
    // çalışmadı) > akan kare > yerleşmeyen animasyon > kısa kuyruk > kapanış
    // paniği. Sızıntının üç kolu kendi aralarında **tanıma gücüne** göre
    // sıralı: `icerik` onu sayısından, yerleşme sorusu altyapısından tanıyor;
    // kuyruk ise yalnız bıraktığı izden, yani en az şey söyleyen o. Panik
    // en sonda, çünkü ötekiler koşunun **ölçtüğü** şeyin bozulduğunu söylüyor;
    // panik koşu bittikten sonraki yolu. İkisi birden olduğunda satır yalnız
    // ilkini yazıyor, ama `kapanis=` jetonu zaten ikincisini taşıyor —
    // `motion_and_panic_report_the_more_fundamental_fault` ve
    // `a_short_tail_fails_the_gate` bu sırayı çiviliyor.
    // Ters çevirmek `ExcessFrames`'in bugünkü sırasını da bozardı.
    let panicked = match teardown {
        Some(Teardown::ReaderPanicked) => Some("okuyucu thread"),
        Some(Teardown::Panicked) => Some("kapanış thread'i"),
        _ => None,
    };
    match workload {
        // Ölçüm yükü düz metin akıtıyor: arka plan da kural da **yok** ve
        // olmayacak. İkisini sormak, duman reçetesini hiç koşmayan bir koşuya
        // o reçetenin sayılarını sormak olurdu — kapı her ölçüm koşusunda
        // düşerdi. Kare akışı burada işin kendisi: üst sınır da yok.
        Workload::Load => {
            if n == 0 || g == 0 {
                Verdict::MissingCounter {
                    required: "kare ve glif >0 olmalı",
                }
            } else if let Some(which) = panicked {
                Verdict::ShutdownPanicked { which }
            } else {
                Verdict::Pass
            }
        }
        // Duman reçetesi: dördü de > 0 **ve** içerik karesi üst sınırlı.
        //
        // Alt sınır `kare`'de, üst sınır `icerik`'te ve bu bilinçli: "pipeline
        // çalıştı mı" sorusunu GPU'nun bitirdiği kare yanıtlıyor, "boşta kare
        // akıyor mu" sorusunu ise çizilmeye karar verilen kare — sonraki
        // phase'in hareket kareleri `kare`'yi meşru olarak şişirecek.
        Workload::Smoke => {
            // `hareket` beşinci gereklilik ve ötekilerle aynı sınıfta: duman
            // reçetesinde bir imleç hareketi var (`bt_core::smoke_shell`),
            // yani sıfır "animasyon yolu hiç koşmadı" demek. Yerleşme sorusu
            // ondan **sonra**: hiç koşmamış bir animasyon zaten yerleşiktir
            // ve okuyanı yanlış arızaya göndermemek gerek.
            if n == 0 || k == 0 || g == 0 || r == 0 || m == 0 {
                Verdict::MissingCounter {
                    required: "beşi de >0 olmalı",
                }
            } else if c > IDLE_FRAME_LIMIT {
                Verdict::ExcessFrames {
                    limit: IDLE_FRAME_LIMIT,
                }
            } else if motion == MotionState::Unsettled {
                Verdict::MotionUnsettled
            } else if quiet.is_none_or(|q| q < QUIET_FLOOR) {
                // `None` de buraya düşüyor ve ayrı bir kol **değil**: ikisi de
                // "koşunun sonunda sessizlik yoktu" diyor ve ileti hangisi
                // olduğunu `quiet_phrase` ile zaten söylüyor. Ayrı bir varyant
                // kapıya ikinci bir karar eklemeden yalnız ikinci bir ad
                // eklerdi.
                Verdict::QuietTooShort { floor: QUIET_FLOOR }
            } else if let Some(which) = panicked {
                Verdict::ShutdownPanicked { which }
            } else {
                Verdict::Pass
            }
        }
    }
}

impl AppDelegate {
    pub(crate) fn new(mtm: MainThreadMarker, opts: Options) -> Retained<Self> {
        // Halka **yalnız** kapı açıkken ayrılıyor: kapalı kapının bedeli bir
        // `Option` dallanması olmalı, bir ayırma değil (R4.1). Kapasitenin
        // koşu süresinden türemesi de `bt-gpu`'nun işi — tazeleme hızını bilen
        // taraf o.
        let stats = opts
            .run
            .and_then(|run| run.stats_since.map(|since| Stats::new(since, run.seconds)))
            .map(Arc::new);
        let this = Self::alloc(mtm).set_ivars(Ivars {
            run: opts.run,
            notices: RefCell::new(Notices::default()),
            settings: RefCell::new(Settings::default()),
            config_watch: RefCell::new(None),
            theme_watch: RefCell::new(None),
            stats,
            windows: RefCell::new(Vec::new()),
            next_window_id: Cell::new(0),
            appearance_dark: Cell::new(None),
            settings_window: RefCell::new(None),
            settings_state: RefCell::new(settings::FileState::Missing),
        });
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        unsafe { msg_send![super(this), init] }
    }

    /// Yeni pencerenin kimliği; sayaç yalnız artıyor.
    fn next_window_id(&self) -> u64 {
        let id = self.ivars().next_window_id.get();
        self.ivars().next_window_id.set(id + 1);
        id
    }

    /// Pencere listesinin **kopyası** — dolaşan her yol bunu kullanıyor.
    ///
    /// Kopya, çünkü pencereye giden çağrı geri dönüp buraya uzanabiliyor
    /// (`sync_geometry` → [`AppDelegate::post_notices`]) ve listeyi ödünç
    /// tutarak dolaşmak ileride listeyi değiştiren bir yolda (`borrow_mut`)
    /// panikle biterdi. Bedeli birkaç `Retained` kopyası.
    pub(crate) fn windows(&self) -> Vec<Retained<TerminalWindow>> {
        self.ivars().windows.borrow().clone()
    }

    /// Kimliği `id` olan pencere; kapanmışsa `None` (alternatif ekran
    /// habercisi, `window::alt_screen_notifier`).
    pub(crate) fn window(&self, id: u64) -> Option<Retained<TerminalWindow>> {
        self.ivars()
            .windows
            .borrow()
            .iter()
            .find(|window| window.id() == id)
            .cloned()
    }

    /// Etkin pencere: `NSApp.keyWindow` listede aranıyor. Ayar penceresi ya
    /// da bir panel key ise `None` ve yeni pencere evde doğuyor.
    fn key_window(&self) -> Option<Retained<TerminalWindow>> {
        let key = NSApplication::sharedApplication(self.mtm()).keyWindow()?;
        self.window_owning(&key)
    }

    /// `NSWindow`'u `window` olan terminal penceresi; listede yoksa `None`
    /// (panel, ayar penceresi, kapanmış pencere).
    pub(crate) fn window_owning(&self, window: &NSWindow) -> Option<Retained<TerminalWindow>> {
        self.ivars()
            .windows
            .borrow()
            .iter()
            .find(|candidate| candidate.owns(window))
            .cloned()
    }

    /// `applicationShouldTerminate:`'in gövdesi.
    ///
    /// Ayar ödüncü `runModal`'dan **önce** bırakılıyor: modal döngü run
    /// loop'u döndürüyor ve o arada gelen bir kayıt (`reload_settings`)
    /// açık bir ödünçle `replace` edemezdi. Uygulama önce öne alınıyor: arka
    /// plandaki uygulamanın modali pencerelerin arkasında kalabilir ve Dock ▸
    /// Quit tam o yol.
    fn terminate_reply(&self) -> NSApplicationTerminateReply {
        let timed = self.ivars().run.is_some();
        if timed {
            return NSApplicationTerminateReply::TerminateNow;
        }
        let windows = self.windows();
        if windows.is_empty() {
            return NSApplicationTerminateReply::TerminateNow;
        }
        let confirm = self.settings().confirm_close;
        let tabs: Vec<&TerminalWindow> = windows.iter().map(|window| &**window).collect();
        let Some(foregrounds) = window::foregrounds_to_ask(timed, confirm, &tabs) else {
            return NSApplicationTerminateReply::TerminateNow;
        };
        let mtm = self.mtm();
        NSApplication::sharedApplication(mtm).activate();
        let alert = window::alert(mtm, &window::prompt(CloseScope::Quit, &foregrounds));
        if alert.runModal() == NSAlertFirstButtonReturn {
            NSApplicationTerminateReply::TerminateNow
        } else {
            NSApplicationTerminateReply::TerminateCancel
        }
    }

    /// Kapanan pencereyi listeden çıkarır — `windowWillClose:`'un bir tur
    /// sonraki işi. Nesne burada, ana thread'de ve ödünç bırakıldıktan sonra
    /// düşüyor: düşen pencerenin `Drop`'u geri dönüp listeye uzanırsa
    /// `borrow_mut` açık kalmasın.
    pub(crate) fn forget_window(&self, id: u64) {
        let removed = {
            let mut windows = self.ivars().windows.borrow_mut();
            windows
                .iter()
                .position(|window| window.id() == id)
                .map(|index| windows.remove(index))
        };
        drop(removed);
    }

    /// Yeni pencere (ya da `from`'un grubunda yeni sekme) açar — pencere
    /// doğuran **tek** yol: açılışın ilk penceresi (`from = None`), ⌘N, ⌘T,
    /// sekme çubuğunun `+`'sı ve Dock ikonu.
    ///
    /// `from` etkin pencere; yeni kabuk onun OSC 7 dizininde (yoksa evde,
    /// 026 → Karar 4), geçici punto farkı ondan (Karar 3) ve tema onun
    /// oturumundan — bütün pencereler aynı temada; `from` yoksa tema
    /// ayarlardan çözülüyor. `as_tab` ama `from` yoksa ayrı pencere.
    ///
    /// Sıra: punto, alt başlık ve krom pencere görünmeden, liste yerleşimden önce
    /// (geometri olayları pencereyi listede bulsun), oturum yerleşimden
    /// **sonra** — sekmeye eklenen pencere grubun boyutunu alıyor ve kabuk ilk
    /// `TIOCSWINSZ`'yi o boyutla görmeli.
    ///
    /// Hata çağırana dönüyor; oturum doğamadıysa pencere kapatılmış olarak.
    fn open_window(
        &self,
        from: Option<&TerminalWindow>,
        as_tab: bool,
    ) -> Result<Retained<TerminalWindow>, String> {
        let mtm = self.mtm();
        let window = TerminalWindow::new(mtm, self.next_window_id(), self.ivars().run)
            .map_err(|e| e.to_string())?;
        if let Some(from) = from {
            window.set_zoom(from.zoom());
        }
        window.request_font(&self.ivars().settings.borrow().font);
        window.set_subtitle(&NSString::from_str(
            &self.ivars().notices.borrow().subtitle(),
        ));
        self.ivars().windows.borrow_mut().push(window.clone());
        let session = from.and_then(TerminalWindow::session);
        let theme = session.map_or_else(|| self.resolve_theme(), |session| session.theme());
        // Krom pencere **görünmeden**: sonra boyansaydı her ⌘T bir kare
        // sistemin gri başlık çubuğunu gösterirdi.
        window.apply_chrome(&theme);
        match from {
            Some(from) if as_tab => window.show_as_tab_of(from),
            _ => window.show_after(from),
        }
        let dir = session
            .and_then(|session| session.working_directory())
            .or_else(child::working_directory);
        if let Err(e) = window.start(self, mtm, theme, dir) {
            window.close();
            return Err(format!("shell başlatılamadı: {e}"));
        }
        Ok(window)
    }

    /// Etkin pencereden türeyen yeni pencere ya da sekme (⌘N, ⌘T, `+`).
    fn open_from_key_window(&self, as_tab: bool) {
        let from = self.key_window();
        self.open_window_or_report(from.as_deref(), as_tab);
    }

    /// [`AppDelegate::open_window`], hatası stderr'e — ⌘N/⌘T/`+`/Dock'un
    /// yolu. Süreç **çıkmıyor**: öteki pencerelerin kabukları bir yenisinin
    /// doğamamasıyla ölmemeli (yalnız ilk pencere çıkar, `didFinishLaunching`).
    fn open_window_or_report(&self, from: Option<&TerminalWindow>, as_tab: bool) {
        if let Err(e) = self.open_window(from, as_tab) {
            eprintln!("bateri: {e}");
        }
    }

    /// Süreli koşunun tek penceresinin sessizlik damgası (`sessiz=`).
    ///
    /// Süreli koşuda tek pencere var ve rapor onu okuyor; listenin ilki o.
    fn quiet_since(&self) -> Option<Duration> {
        self.windows()
            .first()
            .and_then(|window| window.link().and_then(DisplayLink::quiet_since))
    }

    /// Geçerli ayarlar — pencerelerin okuduğu yol. Ödünç kısa tutulmalı:
    /// kayıt anı yolu yazarken (`replace`) açık bir ödünç panikle biter.
    pub(crate) fn settings(&self) -> Ref<'_, Settings> {
        self.ivars().settings.borrow()
    }

    /// Ölçüm defterinin pencereye (link'e) giden kopyası.
    pub(crate) fn stats(&self) -> Option<Arc<Stats>> {
        self.ivars().stats.clone()
    }

    /// Yeni oturumun shell entegrasyonu: çocuğa eklenecek ortam **ve** dock
    /// payı, tek sorudan ([`shell_integration_env`], [`dock_rows_at_birth`]).
    ///
    /// İki anahtar **tek ödünçten**: ayrı `borrow()`'lar arasına düşen bir
    /// yeniden yükleme ikisini farklı dosyadan okuyabilirdi.
    pub(crate) fn shell_integration(&self) -> (Vec<(String, String)>, u16) {
        let setting = self.ivars().settings.borrow().shell_integration;
        let integration = shell_integration_env(
            &self.inputs(),
            setting,
            child::shell,
            child::zsh_wrapper_dir,
            std::env::var_os("ZDOTDIR"),
        );
        let birth = dock_rows_at_birth(&integration, setting);
        (integration, birth)
    }

    /// Açılışta ayarları okur, [`Ivars::settings`]'e yazar ve tanıları alt
    /// başlığa kaynak kaynak verir. Temayı ilk pencere çözüyor
    /// ([`AppDelegate::resolve_theme`]).
    ///
    /// Süreli koşuda yükleyici **hiç çağrılmaz** ([`Inputs::Hermetic`]) ve
    /// tema gömülü `bateri`, görünüm okunmadan: `Settings::default()` artık
    /// `"system"` ve ona çözülseydi duman makinenin açık modundan etkilenirdi.
    /// Font da renderer'ın açılış değerinde, yani `FontOptions::default()`'ta
    /// kalır — `hucre=8 glif=6` makinenin ayar dosyasına bağlanmaz.
    /// Bozuk dosya pencereyi açık bırakır, varsayılanlarla
    /// ([`settings::Loaded::at_launch`], [`AppDelegate::choose_theme`]).
    ///
    /// Font pencerenin renderer'ına pencere doğarken **yalnız istek** olarak
    /// gidiyor ([`AppDelegate::open_window`] → [`TerminalWindow::request_font`]):
    /// atlas hemen ardından gelen `sync_geometry`'de açılıyor ve font yuvasını
    /// da o yazıyor.
    ///
    /// İzleme de burada kuruluyor, okumadan **önce** (`watch` → kurulum tek
    /// atımlık): açılışla ilk olay arasına düşen bir kayıt kaybolmasın.
    fn load_settings(&self) {
        let Inputs::User { config_root } = self.inputs() else {
            return;
        };
        // Ev dizini çözülemedi: dosya aranamıyor ve bu da görünür olmalı —
        // Dock'tan açılışta stderr'i kimse görmez, kullanıcının ayarları
        // sessizce yok sayılmış olurdu. Okunamayan dosyanın kuralı
        // (`Loaded::at_launch`): dosyada `osc52 = "off"` olabilir, pano
        // kapalıya düşer.
        let (settings, messages) = match &config_root {
            Some(root) => {
                self.watch_config(root);
                let loaded = settings::load(root);
                self.ivars().settings_state.replace(loaded.state());
                loaded.at_launch()
            }
            None => (
                Settings::for_unusable_file(),
                vec![format!(
                    "home directory not found; {} is not read",
                    settings::FILE_NAME
                )],
            ),
        };
        self.post_notices(Source::Settings, messages);
        self.ivars().settings.replace(settings);
    }

    /// Pencere yokken doğan pencerenin teması: ayarların seçtiği, görünüme
    /// göre çözülmüş tema ([`AppDelegate::choose_theme`]). Süreli koşuda
    /// gömülü `bateri` ([`Inputs::Hermetic`]).
    fn resolve_theme(&self) -> Theme {
        let Inputs::User { config_root } = self.inputs() else {
            return Theme::BATERI;
        };
        let settings = self.ivars().settings.borrow();
        self.choose_theme(config_root.as_deref(), &settings)
    }

    /// Ayarların o anki görünüm için seçtiği temayı çözer ve tema yuvasını
    /// yeniler — açılışın ve görünüm değişiminin **ortak** yolu. Kullanılamayan
    /// temanın yerine görünüme uyan gömülü tema gelir
    /// ([`settings::ThemeLoaded::or_embedded`]); ekrandaki temayı tutmak
    /// görünüm değişiminde öteki görünümün temasını bırakırdı.
    ///
    /// Etkin tema dosyasının kaynağı da burada, okumadan önce yenileniyor:
    /// görünüm değişince ad değişiyor ve eski adın kaynağı yeni dosyadaki
    /// yerinde yazmayı görmezdi.
    fn choose_theme(&self, config_root: Option<&Path>, settings: &Settings) -> Theme {
        let dark = self.dark_appearance();
        let name = settings.theme_for(dark);
        self.watch_theme(config_root, name);
        let (theme, messages) = settings::load_theme(config_root, name).or_embedded(dark);
        self.post_notices(Source::Theme, messages);
        theme
    }

    /// Canlı yenileme: bir izleme kaynağı haber verdi. "Settings…" da dizini
    /// yarattıktan sonra buraya geliyor ([`AppDelegate::edit_settings`]) —
    /// sonradan yaratılan dizini hiçbir kaynak görmüyor (`watch`).
    ///
    /// Sıra, üç kural:
    /// - **Önce kur, sonra oku** ([`AppDelegate::watch_config`],
    ///   [`AppDelegate::watch_theme`]); her olayda hepsi yeniden kuruluyor,
    ///   üstüne taşınmış dosyanın ya da silinmiş dizinin bayat tanıtıcısı
    ///   böyle düşüyor.
    /// - **Ayar dosyası** ([`settings::Loaded::live`]): kullanılamayan ya da
    ///   bir an yok olan dosyadan hiçbir şey uygulanmaz ve [`Ivars::settings`]
    ///   değişmez; yuva kendi kaynağına göre dolar ya da boşalır. Değilse
    ///   kabul edilmeyen anahtar geçerli değerini tutar
    ///   ([`settings::load_keeping`]) ve fark alınır: terminal seçenekleri
    ///   **tamamıyla** oturuma gider; font pencerenin geçici punto farkıyla
    ///   renderer'a gider ([`TerminalWindow::apply_font`]) — `size`
    ///   değiştiyse fark sıfırlanarak ([`Zoom::after_reload`]); imleç stili link'e gider
    ///   ([`bt_gpu::DisplayLink::set_cursor_motion`]). Dosya okunup uygulanınca yazma
    ///   yuvası da boşalır: Theme ▸'nin reddettiği dosya düzeltildiyse ret
    ///   artık doğru değil.
    /// - **Tema her olayda yeniden çözülüyor**, ayar dosyası bozuk olsa da
    ///   (son iyi ayarların adıyla): etkin tema dosyası ayrı bir kaynak ve
    ///   hangi dosyanın haber verdiği bilinmiyor. Kullanılamayan tema takas
    ///   edilmez, ekrandaki kalır ([`settings::ThemeLoaded::or_current`]);
    ///   aynı tema takası no-op, kare istenmez.
    ///
    /// Birleştirme yok: bir kayıt birden çok olay doğurur (dizin + dosya) ve
    /// sonrakiler boş fark verir.
    fn reload_settings(&self) {
        let Inputs::User {
            config_root: Some(root),
        } = self.inputs()
        else {
            return;
        };
        // Her uygulama **her pencereye**: ayar dosyası tek, pencereler onu
        // birlikte izliyor. Pencere henüz oturumsuzsa (kaynaklar
        // `didFinishLaunching` içinde kuruluyor, olay ana kuyruğa ancak o
        // dönünce düşebiliyor) yöntemleri kendi yuvalarına bakıp sessizce
        // dönüyor; bu bir sıra değişikliğine karşı.
        let windows = self.windows();
        self.watch_config(&root);
        // Kabul edilmeyen değer geçerli ayardan (`load_keeping`): yanlış
        // türde kaydedilen `scrollback` geçmişi kırpmasın.
        let loaded = settings::load_keeping(&root, &self.ivars().settings.borrow());
        self.ivars().settings_state.replace(loaded.state());
        let (loaded, messages) = loaded.live();
        self.post_notices(Source::Settings, messages);
        if let Some(new) = loaded {
            let changes = {
                let old = self.ivars().settings.borrow();
                // Punto farkı **pencere başına** (026 → Karar 3) ve her
                // pencerede aynı kuralla sıfırlanıyor.
                for window in &windows {
                    window.zoom_after_reload(&old.font, &new.font);
                }
                old.changes(&new)
            };
            if changes.terminal {
                for window in &windows {
                    window.set_terminal_options(&new);
                }
            }
            // Stil link'e gidiyor, oturuma değil: hangi kareyi çizeceğimizi
            // değil **nasıl** çizeceğimizi değiştiriyor. Link `start_session`
            // içinde doğuyor ve bu yol ondan sonra koşuyor, ama sıra bir
            // sözleşme değil: yuva boşsa açılış çağrısı zaten aynı değeri
            // verecek.
            let motion_changed = changes.motion;
            if motion_changed {
                for window in &windows {
                    window.set_cursor_motion(&new);
                }
            }
            // İmlecin çizim sayıları da link'e, aynı gerekçeyle: **nasıl**
            // çizdiğimizi değiştiriyorlar, hangi kareyi çizdiğimizi değil.
            // `Changes::caret` ayrı bir alan, çünkü bunlar `TerminalOptions`'a
            // girmiyor ve `changes.terminal`'a binselerdi bir yarıçap
            // değişimi oturumu baştan kurdururdu.
            if changes.caret {
                for window in &windows {
                    window.apply_caret(&new);
                }
            }
            self.ivars().settings.replace(new);
            // Ayarlar yazıldıktan **sonra**: `apply_reduce_motion` üç
            // çağıranın ortak yolu ve değeri yuvadan okuyor, elindeki `new`'den
            // değil. Stilin yolu ayrı kaldı çünkü o link'i doğrudan alıyor;
            // ikisini birleştirmek bu yolu `new`'e bağlar ve sistem
            // bildiriminden çağrılamaz hâle getirirdi.
            if motion_changed {
                self.apply_reduce_motion();
            }
            // Ayarlar ve fark yazıldıktan **sonra**: `apply_font` ikisini de
            // okuyor.
            if changes.font {
                for window in &windows {
                    window.apply_font(self);
                }
            }
            self.post_notices(Source::Write, Vec::new());
        }
        // Ödünç `set_theme`'den önce düşüyor; içerideki çağrılar `settings`'e
        // dokunmuyor (`apply_appearance`'ın deseni).
        let theme = {
            let settings = self.ivars().settings.borrow();
            let name = settings.theme_for(self.dark_appearance());
            self.watch_theme(Some(&root), name);
            let (theme, messages) = settings::load_theme(Some(&root), name).or_current();
            self.post_notices(Source::Theme, messages);
            theme
        };
        if let Some(theme) = theme {
            for window in &windows {
                window.set_theme(theme);
            }
        }
        // Dosya dışarıdan da değişmiş olabilir (vnode): açık ayar penceresi
        // her koşuda dosyanın hâlini gösterir.
        self.refresh_settings_window();
    }

    /// Ayar penceresinin "Open settings.toml" düğmesi (029 Karar 8; 029'a
    /// kadar bateri ▸ Settings…'ın kendisiydi): dosya yoksa şablonla yaratır
    /// ([`settings::create_if_missing`]), izlemeyi yeniden kurup okur ve
    /// dosyayı editörde açar ([`open_in_editor`]).
    ///
    /// - **Süreli koşu** dosya yaratmaz ([`Inputs::Hermetic`]); ev dizini
    ///   çözülemediyse ayar yuvası bunu açılıştan beri söylüyor.
    /// - **Yeniden okuma yaratmadan sonra** ([`AppDelegate::reload_settings`]):
    ///   dizin yeni doğduysa onu hiçbir kaynak görmüyordu. Şablon
    ///   varsayılanları söylüyor; dosyasız kullanıcıda fark boş, ekran
    ///   değişmez.
    /// - **Hata yazma yuvasına, okumadan sonra**: okuma o yuvayı dosya
    ///   uygulanınca boşaltıyor, önce yazılsaydı hemen silinirdi. Yuva ayar
    ///   penceresinin şeridinde de görünüyor (düğme orada); sonraki başarılı
    ///   okuma ya da yazma onu boşaltır.
    pub(crate) fn edit_settings(&self) {
        let Inputs::User {
            config_root: Some(root),
        } = self.inputs()
        else {
            return;
        };
        let created = settings::create_if_missing(&root);
        self.reload_settings();
        let problem = match created {
            Err(err) => Some(format!(
                "{} could not be created: {err}",
                settings::FILE_NAME
            )),
            Ok(path) if !open_in_editor(&path) => Some(format!(
                "no editor could open {}; it is at {}",
                settings::FILE_NAME,
                path.display()
            )),
            Ok(_) => None,
        };
        // Yazma yuvasına: düğmeye basılan ayar penceresi o yuvayı şeridinde
        // gösteriyor — terminal penceresi hiç yokken alt başlık da yok.
        // Okuma yuvayı boşalttıktan sonra yazılıyor, yani görünür kalıyor.
        if let Some(problem) = problem {
            self.post_notices(Source::Write, vec![problem]);
            self.refresh_settings_window();
        }
    }

    /// View ▸ Theme ▸'nin seçimi: `theme`'i dosyaya yazar
    /// ([`settings::write_edit`]) ve **dosyayı okuyan yoldan** uygular
    /// ([`AppDelegate::reload_settings`]) — menünün kendi uygulama yolu yok,
    /// ekrana giden tek zincir dosyadan geçiyor.
    ///
    /// Okuma yazmadan hemen sonra, izleyicinin olayını beklemeden: dizin az
    /// önce yaratıldıysa onu gören bir kaynak yok ("Settings…"ın gerekçesi).
    /// Ardından gelen olay boş fark ve aynı temanın takası, yani no-op.
    ///
    /// Hata **yazma yuvasına**; başarılı yazma yuvayı boşaltır. Süreli koşu
    /// yazmaz ([`Inputs::Hermetic`]); menü o dalda zaten dolmuyor.
    fn save_theme(&self, name: &str) {
        self.save_edit(&SettingsEdit::Theme(name.to_owned()));
    }

    /// Tek anahtarın yeni değerini dosyaya yazar ve dosyayı okuyan yoldan
    /// uygular — View ▸ Theme ▸'nin ve ayar penceresinin **ortak** yolu
    /// ([`AppDelegate::save_theme`]'in gerekçeleri aynen). Yazma hatasında
    /// pencere dosyanın değerine döner: kontrolün gösterdiği yazılamayan bir
    /// değer olmasın.
    pub(crate) fn save_edit(&self, edit: &SettingsEdit) {
        let Inputs::User {
            config_root: Some(root),
        } = self.inputs()
        else {
            return;
        };
        match settings::write_edit(&root, edit) {
            Ok(()) => {
                self.post_notices(Source::Write, Vec::new());
                self.reload_settings();
            }
            Err(message) => {
                self.post_notices(Source::Write, vec![message]);
                self.refresh_settings_window();
            }
        }
    }

    /// bateri ▸ Settings…: ayar penceresini doğurur (ilk seferde), etkin
    /// ayarla doldurur ve öne getirir. Süreli koşuda ve ev dizini
    /// çözülemeyince **hiçbir şey** yapmaz ([`Inputs::Hermetic`]): yazacağı
    /// bir dosya yok, `make duman` pencereyi hiç görmüyor.
    fn show_settings_window(&self) {
        let Inputs::User {
            config_root: Some(_),
        } = self.inputs()
        else {
            return;
        };
        let window = self
            .ivars()
            .settings_window
            .borrow_mut()
            .get_or_insert_with(|| SettingsWindow::new(self.mtm()))
            .clone();
        // Önce göster: tazeleme kapalı pencereyi atlıyor. İkisi aynı ana
        // kuyruk turunda, arada bir kare çizilmiyor.
        window.show();
        self.refresh_settings_window();
    }

    /// Açık (ya da gizli) ayar penceresini etkin ayarla doldurur; pencere
    /// hiç doğmadıysa no-op. Ayar ödüncü pencereye girmeden **kopyalanıyor**:
    /// bu yol bir kontrolün eyleminden (yaz → `reload_settings` → buraya)
    /// koşuyor ve pencere geri dönüp delegate'e uzanabiliyor.
    ///
    /// Dosyanın hâli [`Ivars::settings_state`]'ten, yazma hatası alt başlığın
    /// yazma yuvasından (029 Karar 7): ikisi de tek kaynak, pencere kendi
    /// kopyasını tutmuyor. Yazma yuvası başarılı yazmada ve dosya okunup
    /// uygulanınca boşalıyor, yani şerit de o an kalkıyor.
    ///
    /// Kapalı pencere tazelenmez: her kayıtta tema dizinini okumak, dört
    /// popup'ı yeniden kurmak ve eksik font için CoreText açmak kimsenin
    /// görmediği bir iş olurdu; yeniden açılış tazeliyor
    /// ([`AppDelegate::show_settings_window`]).
    pub(crate) fn refresh_settings_window(&self) {
        let Some(window) = self.ivars().settings_window.borrow().clone() else {
            return;
        };
        if !window.is_open() {
            return;
        }
        let Inputs::User {
            config_root: Some(root),
        } = self.inputs()
        else {
            return;
        };
        let settings = self.ivars().settings.borrow().clone();
        let state = self.ivars().settings_state.borrow().clone();
        let write = self.ivars().notices.borrow().get(Source::Write).to_vec();
        let embedded: Vec<&str> = Theme::embedded_names().collect();
        let user = settings::user_theme_names(&root);
        window.refresh(&settings, &state, &write, &embedded, &user);
    }

    /// Ayar dizininin kaynaklarını yeniden kurar. Yenisi eskisi düşmeden
    /// kuruluyor (`replace`): iki kurulum arasında boşluk yok.
    fn watch_config(&self, root: &Path) {
        let watch = Watch::install(
            &settings::watched_paths(root),
            DispatchQueue::main(),
            &watch_notify(),
        );
        self.ivars().config_watch.replace(Some(watch));
    }

    /// Etkin tema dosyasının kaynağını yeniden kurar; ev dizini yoksa yuva
    /// boşalır.
    fn watch_theme(&self, config_root: Option<&Path>, name: &str) {
        let watch = config_root.map(|root| {
            Watch::install(
                &[settings::theme_path(root, name)],
                DispatchQueue::main(),
                &watch_notify(),
            )
        });
        self.ivars().theme_watch.replace(watch);
    }

    /// Sistemin açık/koyu görünümünü izlemeye başlar — **yalnız kullanıcının
    /// oturumunda** ([`Inputs`]).
    ///
    /// Kaynak `NSApp.effectiveAppearance`'ın KVO'su, view'ın
    /// `viewDidChangeEffectiveAppearance`'ı **değil**: pencerenin kromu temanın
    /// görünümünü taşıyor ([`TerminalWindow::apply_chrome`]) ve görünümü
    /// kurulmuş pencere sistemden miras almayı bırakıyor — view o andan sonra
    /// sistemin değişimini hiç görmüyor (ölçüldü, 026 phase-4 Uygulama
    /// Notları), yalnız bizim kendi kurduğumuzu görüyordu.
    ///
    /// Gözlemci **sökülmüyor**: `AppDelegate` de `NSApp` de sürecin ömrü
    /// boyunca yaşıyor ([`AppDelegate::observe_reduce_motion`]'ın emsali).
    fn observe_appearance(&self) {
        let Inputs::User { .. } = self.inputs() else {
            return;
        };
        let app = NSApplication::sharedApplication(self.mtm());
        // SAFETY: gözlemci bu sınıf ve `observeValueForKeyPath:…`'u
        // uyguluyor; bağlam boş, çünkü izlenen tek yol bu. İki nesne de süreç
        // boyunca yaşıyor, yani kayıt sarkan bir gözlemci bırakmıyor.
        unsafe {
            app.addObserver_forKeyPath_options_context(
                self,
                ns_string!("effectiveAppearance"),
                NSKeyValueObservingOptions::empty(),
                std::ptr::null_mut(),
            );
        }
    }

    /// Görünüm değişiminin uygulayıcısı: tema sistemi izliyorsa görünüme uyan
    /// temayı açılışla aynı yoldan ([`AppDelegate::choose_theme`]) seçer ve
    /// oturuma ve kroma takas eder ([`TerminalWindow::set_theme`]).
    ///
    /// Dört kapı, sırayla:
    /// - **Süreli koşu** ([`Inputs::Hermetic`]): görünüm yok sayılır, tema
    ///   `bateri` kalır — `make duman` makinenin açık modundan etkilenmez.
    /// - **Açık/koyu biti değişmedi** ([`Ivars::appearance_dark`]).
    /// - **Hiçbir pencerede oturum yok:** son pencere kapanmış (uygulama açık
    ///   kalıyor) ya da tek pencerenin oturumu henüz doğmamış; doğan pencere
    ///   temayı görünümden kendisi seçiyor ([`AppDelegate::open_window`]).
    /// - **Sabit tema** (`theme = "{ad}"`): görünüm temaya dokunmaz.
    ///
    /// Tema aynı çıkarsa (`light_theme` ile `dark_theme` aynı ad) takas no-op
    /// ve kare istenmez
    /// (`Session::set_theme`). Tema yuvası yine yeniden yazılır: bu okuma o
    /// kaynağın güncel hâli.
    fn apply_appearance(&self) {
        let Inputs::User { config_root } = self.inputs() else {
            return;
        };
        let dark = self.dark_appearance();
        if self.ivars().appearance_dark.replace(Some(dark)) == Some(dark) {
            return;
        }
        // Bit kapıdan **önce** yazıldı: pencere yokken gelen değişim de
        // görülmüş sayılıyor, sonra doğan pencere temayı zaten görünümden
        // seçiyor.
        let windows = self.windows();
        if windows.iter().all(|window| window.session().is_none()) {
            return;
        }
        // Ödünç `choose_theme`'in sonunda düşüyor; oradaki `post_notices`
        // yalnız `notices`'i ödünç alıyor, `settings`'e dokunmuyor.
        let theme = {
            let settings = self.ivars().settings.borrow();
            if !settings.follows_system() {
                return;
            }
            self.choose_theme(config_root.as_deref(), &settings)
        };
        for window in &windows {
            window.set_theme(theme);
        }
    }

    /// Sistemin Hareketi Azalt ayarını izlemeye başlar — **yalnız kullanıcının
    /// oturumunda** ([`Inputs`]).
    ///
    /// Bildirim `NSWorkspace`'in **kendi** merkezinden geliyor, varsayılan
    /// `NSNotificationCenter`'dan değil; Apple bunu böyle yayınlıyor ve yanlış
    /// merkeze abone olmak sessizce hiç haber almamak olurdu.
    ///
    /// Gözlemci **sökülmüyor**: `AppDelegate` sürecin ömrü boyunca yaşıyor
    /// (`run()`'daki `Retained`) ve merkez onu zaten sahiplenmeden tutuyor.
    /// Açık/koyu görünümün KVO'su ([`AppDelegate::observe_appearance`]) de
    /// aynı biçimde sökülmüyor.
    fn observe_reduce_motion(&self) {
        let Inputs::User { .. } = self.inputs() else {
            return;
        };
        let center = NSWorkspace::sharedWorkspace().notificationCenter();
        // SAFETY: `accessibilityDisplayDidChange:` bu sınıfta tanımlı ve tek
        // `Option<&AnyObject>` argüman alıyor; `self` sürecin ömrü boyunca
        // yaşıyor, yani merkezin sahiplenmeyen referansı asarak kalmıyor.
        // Sabit `NSString` AppKit'in dışa açtığı ad (`NSRunLoopCommonModes`
        // emsali).
        unsafe {
            center.addObserver_selector_name_object(
                self,
                sel!(accessibilityDisplayDidChange:),
                Some(NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification),
                None,
            );
        }
        self.apply_reduce_motion();
    }

    /// Hareketi Azalt'ın **çözülmüş** değerini bütün pencerelerin link'ine
    /// verir ([`AppDelegate::reduce_motion`]).
    ///
    /// Üç çağıranı var ve üçü de aynı soruyu yeniden soruyor: açılış
    /// ([`AppDelegate::observe_reduce_motion`]), sistem bildirimi ve ayar
    /// dosyasının kaydı ([`AppDelegate::reload_settings`]). Değer
    /// değişmediyse çağrı no-op (`bt_gpu::DisplayLink::set_reduce_motion`),
    /// yani üç yolu birleştirmeye gerek yok. Pencerenin ilk değeri kendi
    /// `start`'ında iniyor; link'i olmayan pencere sessizce atlanıyor.
    ///
    /// Tekerleğin kipi de burada iniyor ([`AppDelegate::smooth_scroll`]):
    /// Hareketi Azalt onun girdisi, yani üç tetikleyicinin üçü de onu da
    /// değiştirebiliyor — ikinci bir yol yazılsaydı sistem bildirimi onu
    /// atlardı.
    fn apply_reduce_motion(&self) {
        let reduce = self.reduce_motion();
        let smooth = resolve_smooth_scroll(&self.ivars().settings.borrow(), reduce);
        for window in self.windows() {
            window.set_reduce_motion(reduce);
            window.set_smooth_scroll(smooth);
        }
    }

    /// Tekerlek pürüzsüz mü ([`resolve_smooth_scroll`]).
    pub(crate) fn smooth_scroll(&self) -> bool {
        let reduce = self.reduce_motion();
        resolve_smooth_scroll(&self.ivars().settings.borrow(), reduce)
    }

    /// Ayarın üç değeri ile sistemin cevabı, [`resolve_reduce_motion`]'da
    /// birleşmiş hâliyle.
    pub(crate) fn reduce_motion(&self) -> bool {
        let setting = self.ivars().settings.borrow().reduce_motion;
        resolve_reduce_motion(&self.inputs(), setting, || {
            NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
        })
    }

    /// Uygulamanın etkin görünümü, yani sistemin açık/koyu ayarı koyu mu.
    ///
    /// `NSApp`'ten okunması **zorunlu**: pencere ve view'ın görünümü artık
    /// temayı yansıtıyor, sistemi değil ([`TerminalWindow::apply_chrome`]) —
    /// view'dan okumak sabit açık temalı bir kullanıcıda sistem koyuyken
    /// "açık" derdi ve temayı seçen soru kendi cevabını okurdu.
    /// `bestMatchFromAppearancesWithNames` "koyu mu" sorusunun
    /// AppKit'teki yolu — ad karşılaştırması yüksek kontrastlı koyu
    /// görünümü (`NSAppearanceNameAccessibilityHighContrastDarkAqua`) açık
    /// sayardı.
    fn dark_appearance(&self) -> bool {
        let appearance = NSApplication::sharedApplication(self.mtm()).effectiveAppearance();
        // SAFETY: AppKit'in dışa açtığı iki sabit `NSString`; süreç boyunca
        // yaşıyorlar ve yalnız okunuyorlar (`NSRunLoopCommonModes` emsali).
        let (aqua, dark_aqua) = unsafe { (NSAppearanceNameAqua, NSAppearanceNameDarkAqua) };
        appearance
            .bestMatchFromAppearancesWithNames(&NSArray::from_slice(&[aqua, dark_aqua]))
            .is_some_and(|best| &*best == dark_aqua)
    }

    /// Kullanıcının dünyasına açılan girişlerin kararı ([`Inputs`]).
    fn inputs(&self) -> Inputs {
        decide_inputs(self.ivars().run, child::home())
    }

    /// Pencere alt başlığının **tek** yazanı: kaynağın yuvasını yeniler,
    /// tanıları `bateri:` önekiyle stderr'e basar ve alt başlığı kurar.
    ///
    /// Alt başlığa yazan ikinci bir yol olursa yuvalar anlamını yitirir: biri
    /// ötekinin tanısını sessizce ezer.
    ///
    /// **Yuva aynı kalıyorsa hiçbir şey yapmaz** — ne stderr ne alt başlık.
    /// Font yuvası `sync_geometry`'nin sonunda yazılıyor ve o yol canlı
    /// boyutlandırmada her olayda koşuyor: bulunamayan bir aile stderr'e olay
    /// başına bir satır basar, alt başlık da boşuna yeniden kurulurdu. Aynı
    /// hatalı ayar dosyasını ikinci kez kaydetmek de artık satırı tekrar
    /// basmıyor; alt başlıkta zaten duruyor.
    pub(crate) fn post_notices(&self, source: Source, messages: Vec<String>) {
        let subtitle = {
            let mut notices = self.ivars().notices.borrow_mut();
            if notices.get(source) == messages.as_slice() {
                return;
            }
            for message in &messages {
                eprintln!("bateri: {message}");
            }
            notices.replace(source, messages);
            notices.subtitle()
        };
        let subtitle = NSString::from_str(&subtitle);
        for window in self.windows() {
            window.set_subtitle(&subtitle);
        }
    }

    /// Uygulamanın kapanış sırasının **tek** yeri; her çıkış yolu buradan
    /// geçer (`applicationWillTerminate:` ve `runDeadline:`). Tek bir
    /// pencerenin kapanışı buraya uğramıyor, beklemiyor da (`TerminalWindow`'un
    /// `windowWillClose:`'u).
    ///
    /// Bugün çağrı tek: süreli koşu `process::exit`'e, etkileşimli ⌘Q
    /// AppKit'in çıkışına varıyor ve ana thread beklerken zamanlayıcı
    /// ateşleyemiyor. Adımlar idempotent
    /// ([`TerminalWindow::begin_close`]); bekçi değil — ikinci bir çağrı
    /// ikinci bir bekleme doğurmaz ama sonuç `AlreadyDone` olur.
    ///
    /// **Paralel, tek son tarih** (026 → Karar 5): önce her pencerenin
    /// kapanışı başlıyor (ritim durur, `Waker` sökülür, `SIGHUP` gider), sonra
    /// hepsi **aynı** `now + SHUTDOWN_GRACE`'e kadar bekleniyor — N sekmenin
    /// toplam beklemesi N × `SHUTDOWN_GRACE` değil bir `SHUTDOWN_GRACE`.
    /// Ölmeyen çocuk arkada bırakılıyor. Tek istisna kapanış thread'inin
    /// kurulamaması (OS thread sınırı): o dalda sınır yok ve kesecek olan
    /// süreli koşuda bekçi.
    ///
    /// **Pencereler bekleme bitene kadar listede** (ve buradaki kopyada)
    /// kalıyor: `DisplayLink`'ler ana thread'de yaşıyor, yani Metal'in
    /// tamamlanma bloğunun tuttuğu `Waker` kopyası o arada son referans olup
    /// ana kuyruğa senkron iş atamaz — ana thread beklemedeyken ikisi
    /// birbirini kilitlerdi. `ShellWake`'lerin `Waker`'ı zaten sökülmüş,
    /// yani sınır dolduğunda `"PTY teardown"` thread'inde kalan kopyalar
    /// `Waker` taşımıyor (`wake.rs` → Sahiplik).
    ///
    /// Dönen sonuç **ilk** pencerenin: raporu isteyen tek yol süreli koşu ve
    /// orada tek pencere var (026 → Karar 9). Etkileşimli kapanışta sonuç
    /// atılıyor — toplanmıyor, çünkü okuyan yok.
    fn shutdown(&self) -> Option<Teardown> {
        // Bekçinin bütçesi **kapanıştan** başlıyor, süreç başından değil:
        // açılış (Metal device, metallib yükleme, ilk pencere) soğuk bir
        // makinede saniyeler sürebilir ve o süre bütçeden düşseydi sağlıklı
        // bir koşu `_exit(70)` ile kırmızı düşerdi.
        if self.ivars().run.is_some() {
            crate::watchdog();
        }
        let windows = self.windows();
        let closing: Vec<_> = windows.iter().map(|window| window.begin_close()).collect();
        let deadline = Instant::now() + SHUTDOWN_GRACE;
        // Sonuç raporu besliyor (`kapanis=`): oturum hiç doğmadıysa `None` ve
        // o da bir cevap — kapanacak bir şey yoktu.
        let mut first = None;
        for (index, closing) in closing.into_iter().enumerate() {
            let teardown = closing.map(|closing| closing.wait_until(deadline));
            if index == 0 {
                first = teardown;
            }
        }
        first
    }

    /// Duman koşusunun raporu ve çıkışı — **kapanıştan sonra** çağrılır.
    ///
    /// Sıra bilinçli: `shutdown()` sınırlı da olsa bekler ve o sınırı da aşan
    /// bir kapanışta bekçi süreci 70 ile keser, yani öyle bir kapanışta
    /// `kare=` satırı hiç çıkmaz. Ters sırada `make duman` yeşil bir satırla
    /// kırmızı bir çıkış kodunu birlikte verirdi.
    ///
    /// Satır burada **açıkça** yazılıyor; `Drop`'a güvenen hiçbir yol yok.
    /// `process::exit` `Drop` koşturmaz ve bekçinin `_exit(70)`'i atexit'i
    /// bile atlar.
    ///
    /// `teardown` argüman, ivar değil: kapanışın sonucunu **çağıran** biliyor
    /// ve bir ivar'a saklamak onu ikinci bir kez okunabilir kılardı. `quiet`
    /// de argüman ama başka bir sebeple: değeri kapanıştan **önce** okunmak
    /// zorunda (iki çağıranın da doc'unda) ve burada okunsaydı `shutdown()`'ın
    /// beklemesi sessizliğe yazılırdı.
    ///
    /// Sayaçlar süreli koşunun **tek** penceresinden okunuyor (listenin ilki).
    /// Pencere yoksa (açılış hiç pencere kuramadıysa süreç zaten çıkmıştı)
    /// sayaçlar sıfır ve kapı `MissingCounter` diyor.
    fn report_and_exit(&self, run: Run, teardown: Option<Teardown>, quiet: Option<Duration>) -> ! {
        let windows = self.windows();
        let window = windows.first();
        let renderer = window.map(|window| window.renderer());
        // Dört sayaç dört ayrı şey söyler: `kare` GPU'nun hatasız bitirdiği
        // kare sayısı, `hucre` sink'in ürettiği arka plan hücresi, `glif`
        // çizilen glyph, `kural` çizilen alt çizgi/üstü çizili. Biri sıfırken
        // diğerleri yeşil geçemez — kare>0 & hucre=0 "pencere var, shell
        // çıktısı yok" demek; hucre>0 & glif=0 ise "hücreler boyanıyor ama
        // harf yok", yani 002'nin körlemesine yazma dönemine sessizce geri
        // düşmek: `glif` kapısı olmasaydı `frame()` sınırı karakteri hiç
        // geçirmese bile `kare=1 hucre=8 pipeline=ok` basılırdı. `kural`ın
        // kapattığı yarı da aynı biçimde ayrı: sınır beş alt çizgi çeşidini,
        // üstü çiziliyi ve SGR 58'i taşıyor ve o yolun tamamı `glif`'ten
        // bağımsız — duman reçetesinin yedi kural hücresi mürekkepsiz.
        //
        // Kapsamadığı — dördü de birer **CPU** sayacı ve `kural` bunun üstüne
        // stil ayrımını da göremez; sınırın tamamı `Frame::rule_count`'ta
        // yazılı ve tek yerde duruyor. Buraya yalnız duman kapısına özgü olan
        // yarı düşüyor: jeton setin **yüz yarısını hiç sormuyor** — `Face`
        // çevirisini hep `Regular` döndüren bir yapı da aynı dört sayıyı
        // basar, çünkü kalın bir glyph de bir glyph'tir. O yarının kapısı
        // `bt-gpu`'nun `sgr_flags_translate_to_four_faces` ve
        // `bold_and_regular_draw_differently` sınamaları.
        //
        // Dördü bir **yapıda** taşınıyor, konumsal argüman olarak değil: üçü
        // aynı tip ve yer değiştirseler derleme geçerdi.
        let (n, k, g, r) = (
            renderer.map_or(0, Renderer::frames),
            renderer.map_or(0, Renderer::last_bg_count),
            renderer.map_or(0, Renderer::last_glyph_count),
            renderer.map_or(0, Renderer::last_rule_count),
        );
        // Beşinci sayaç `icerik` aynı yapıda ama başka bir yerden: `kare` GPU
        // tarafında (tamamlanma bloğu), `icerik` ana thread'de
        // (`needs_update`). Kapının üst sınırı buna bağlı ve alt sınır hâlâ
        // `kare`'de — hangi sorunun hangi sayacı sorduğu [`verdict`]'te.
        let link = window.and_then(|window| window.link());
        let counters = Counters {
            frames: n,
            content: link.map_or(0, DisplayLink::content_frames),
            cells: k,
            glyphs: g,
            rules: r,
            motion: link.map_or(0, DisplayLink::motion_frames),
            slide: link.map_or(0, DisplayLink::slide_frames),
        };
        // Yerleşme bir **sayı değil durum**, o yüzden `Counters`'ın dışında.
        // Link yoksa (oturum hiç doğmadı) bekleyen animasyon da yok: sayaç
        // yarısı (`hareket=0`) zaten `MissingCounter` veriyor ve okuyanı
        // "animasyon durmadı" diye yanlış arızaya göndermemek gerek.
        let motion = if link.is_none_or(DisplayLink::motion_settled) {
            MotionState::Settled
        } else {
            MotionState::Unsettled
        };
        // Beşinci jeton `yuva=U/T` bir kapı değil, bir **sayaç**: atlasın kaç
        // yuvasının dolduğunu söylüyor ve `/measure` doluluk oranını ondan
        // okuyacak. Kapıya girmemesinin sebebi anlamı: boş bir atlas da
        // meşrudur (glyph'siz bir kare) ve dolu bir atlas da — arıza eşiği
        // ölçülmeden bilinmiyor, ölçülmemiş sayı da kapıya yazılmaz. `istek=`
        // de öyle: kare **talebi** `kare`'nin göremediği yeri görüyor ama
        // eşiği ölçülmedi.
        let report = Report {
            counters,
            atlas: renderer.map_or((0, 0), Renderer::atlas_occupancy),
            color_atlas: renderer.map_or((0, 0), Renderer::color_atlas_occupancy),
            workload: run.workload,
            requests: link.map_or(0, DisplayLink::requests),
            quiet,
            teardown,
            // Kapı kapalıysa defter hiç doğmadı; `Option` bunu taşıyor ve
            // rapor `ornek=off` diyor — uydurulmuş bir sıfır değil.
            measured: self.ivars().stats.as_deref().map(Measured::read),
        };
        // Jetonlar **yalnız** başarı satırında ve yalnız stdout'ta: makine
        // sözleşmesi o. Hata satırları aynı sayıları taşıyor ama jeton
        // biçiminde değil, yoksa `kare=` arayan bir CI adımı düşen koşudan
        // kare sayısı okurdu.
        let secs = run.seconds;
        match verdict(counters, run.workload, teardown, motion, quiet) {
            Verdict::Pass => {
                println!("{}", report.token_line());
                std::process::exit(0);
            }
            // Ayrı ileti, çünkü ayrı arıza: burada beş sayacın beşi de
            // yerinde ve okuyanı sıfır aramaya göndermek zaman kaybettirirdi.
            // Sınırı aşan sayı `icerik`, ama satır `kare` ile `istek`'i de
            // söylüyor: üçü birlikte okunduğunda arıza "hasar akıyor" mu
            // (üçü de yüksek) yoksa "hareket yerleşmiyor" mu (`kare` yüksek,
            // `icerik` değil) ayırt edilebiliyor.
            Verdict::ExcessFrames { limit } => eprintln!(
                "bateri: boşta sıfır kare bozuldu — {secs} saniyelik koşuda {c} içerik karesi çizildi (toplam kare {n}, kare talebi {}, {}), üst sınır {limit}",
                report.requests,
                quiet_phrase(report.quiet),
                c = counters.content,
            ),
            Verdict::MissingCounter { required } => eprintln!(
                "bateri: {secs} saniyelik koşuda çizilen kare {n}, içerik karesi {c}, üretilen hücre {k}, çizilen glif {g}, çizilen kural {r}, hareket karesi {m}, {} ({required})",
                quiet_phrase(report.quiet),
                c = counters.content,
                m = counters.motion,
            ),
            // Durma koşulu bozuldu. Sayaçlar yerinde ve kare sınırı aşılmamış
            // olabilir — yavaş bir animasyon ikisini de geçer; kırmızıyı
            // düşüren şey deadline'da hâlâ uçuşta olması.
            Verdict::MotionUnsettled => eprintln!(
                "bateri: {secs} saniyelik koşunun sonunda animasyon hâlâ yerleşmemişti — bir durma koşulu bozuk (çizilen hareket karesi {m}, {})",
                quiet_phrase(report.quiet),
                m = counters.motion,
            ),
            // Sızıntı ne sınırı aştı ne de hareket altyapısından geçti: geriye
            // bıraktığı iz kaldı. İleti tabanı **ve** ölçüleni birlikte
            // söylüyor, çünkü ikisi arasındaki fark sızıntının periyodunu
            // veriyor — okuyan taraf "ne kadar sık kare istiyor" sorusunu
            // satırdan yanıtlayabilsin.
            Verdict::QuietTooShort { floor } => eprintln!(
                "bateri: {secs} saniyelik koşunun sonunda kare akıyordu — {} (en az {} beklenir; içerik karesi {c}, hareket karesi {m}, kare talebi {})",
                quiet_phrase(report.quiet),
                ms(floor),
                report.requests,
                c = counters.content,
                m = counters.motion,
            ),
            // Sayaçlar yerinde ama kapanış yolunda panik var: jeton satırı
            // basılmıyor ki `kare=` arayan bir CI adımı bu koşuyu ölçüm
            // sanmasın.
            Verdict::ShutdownPanicked { which } => eprintln!(
                "bateri: kapanış yolunda panik ({which}) — sayaçlar yerinde ama koşu geçerli değil"
            ),
        }
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sağlıklı bir duman koşusunun **ölçülen** kuyruğu (2026-09-16, otuz
    /// yedi koşunun en düşüğü: `1742,29 ms`; sahibi `docs/OLCUMLER.md`).
    /// Kapıyı sormayan sınamalar bunu veriyor ki `sessiz` kolu onların
    /// sorduğu şeyi gölgelemesin; kolun kendi sınamaları aşağıda ve tabanı
    /// adıyla anıyor.
    const HEALTHY_QUIET: Option<Duration> = Some(Duration::from_millis(1742));

    /// Izgara ölçüsü; pay **argüman**, çünkü `split_into_grid`'un sorduğu iki
    /// ayrı şey var: hücre bölmesi (pay sıfır) ve payın sütunlardan düşülmesi.
    fn metrics(w: u16, h: u16, gutter: u16) -> CellMetrics {
        CellMetrics::new(w, h, w, gutter, 1).expect("sıfır olmayan hücre")
    }

    /// Dock'suz pencere: entegrasyonsuz oturumun (ve duman reçetesinin) hâli.
    /// Sütun ve satır aritmetiğini sorgulayan sınamalar bunu veriyor ki dock
    /// payı onların beklediği sayılara karışmasın; payın kendi sınaması
    /// aşağıda ve `DOCK_ROWS`'u adıyla anıyor.
    const NO_DOCK: u16 = 0;

    fn report(counters: Counters, workload: Workload) -> Report {
        Report {
            counters,
            atlas: (13, 2048),
            // Renk düzlemi **boş**: duman reçetesi `/bin/sh` koşuyor ve
            // emoji basmıyor, yani sağlıklı koşunun beklediği sayı bu.
            color_atlas: (0, 2048),
            workload,
            requests: 4,
            quiet: Some(Duration::from_millis(2950)),
            teardown: Some(Teardown::Clean),
            measured: None,
        }
    }

    fn smoke_counters() -> Counters {
        Counters {
            frames: 1,
            content: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
            // Reçetedeki imleç hareketinin izi; sıfır olsaydı kapı
            // `MissingCounter` derdi (bkz. `Counters::motion`).
            motion: 3,
            // Kaymanın izi. `motion`'dan farklı bir sayı **bilerek**: ikisi
            // aynı karelerin sayısı değil, iki ayrı animatörün tanığı.
            slide: 2,
        }
    }

    #[test]
    fn smoke_counts_unchanged() {
        // Duman sözleşmesinin **bit bit** aynı kalması gereken yarısı. Jeton
        // eklemek serbest; bu dördü bu sırayla, bu değerlerle ve satırın
        // **başında** durur — `make duman`'ı okuyan taraf (ve `proje.md`'nin
        // doğrulama tablosu) onları metin olarak arıyor.
        let line = report(smoke_counters(), Workload::Smoke).token_line();
        assert!(
            line.starts_with("kare=1 hucre=8 glif=6 kural=15 "),
            "duman sayaçları oynadı: {line}"
        );
    }

    #[test]
    fn token_line_preserves_old_tokens() {
        // Sözleşme: **silinmez, eklenir.** Rapor genişlerken düşen bir jeton
        // sessizdir — okuyan taraf tanımadığını atlayabilir, kaybolanı
        // arayamaz.
        let line = report(smoke_counters(), Workload::Smoke).token_line();
        for token in [
            "kare=1",
            "hucre=8",
            "glif=6",
            "kural=15",
            "yuva=13/2048",
            "yuk=smoke",
            "istek=4",
            "kapanis=clean",
        ] {
            assert!(line.contains(token), "{token} düştü: {line}");
        }
        assert!(line.ends_with(" pipeline=ok"), "{line}");

        // Dört yeni anahtar da **kalıcı**: sözleşme bugünden sonra onları da
        // "silinmez" tarafına alıyor. `kayma=` 011 ile geldi ve `hareket=`'in
        // yanına girdi — jeton **silinmez, eklenir**.
        for token in [
            "icerik=1",
            "hareket=3",
            "kayma=2",
            "sessiz=2950.00ms",
            // 023 ile geldi ve `yuva=`'nin **hemen yanına** girdi: ikisi
            // atlasın iki düzlemi ve yan yana okunuyorlar. Listeye aynı gün
            // yazıldı, çünkü "silinmez" sözü ancak bir bekçisi varsa söz —
            // yukarıdaki liste yalnız **eski** jetonları koruyor.
            "yuva2=0/2048",
        ] {
            assert!(line.contains(token), "{token} yok: {line}");
        }
        // Yeri de sözleşme: `yuva=` ile `yuva2=` yan yana. Ayrılsalardı satırı
        // gözle okuyan taraf iki düzlemi birbirine bağlayamazdı.
        assert!(
            line.contains("yuva=13/2048 yuva2=0/2048 "),
            "iki düzlemin jetonu yan yana durmalı: {line}"
        );

        // Kapı kapalıyken ölçüm jetonları **yok** ve `ornek=0` da yok: sıfır,
        // "kapı açıktı ama hiç örnek toplanmadı" ile karışırdı ve R5.2'nin
        // kapatmak istediği körlük tam olarak o.
        assert!(line.contains(" ornek=off"), "{line}");
        assert!(!line.contains("cpu_kare_p95"), "{line}");
        assert!(!line.contains("acilis="), "{line}");

        // `yuk=` yükle değişiyor ve dizgi tipin yanında duruyor.
        let load = report(
            Counters {
                frames: 9,
                content: 9,
                cells: 0,
                glyphs: 12,
                rules: 0,
                motion: 0,
                slide: 0,
            },
            Workload::Load,
        )
        .token_line();
        assert!(load.contains("yuk=load"), "{load}");
    }

    #[test]
    fn quiet_token_says_none_when_nothing_was_drawn() {
        // `sessiz=` uydurulmuş bir sıfır basmıyor: sıfır, "deadline anında
        // kare akıyordu" demek ve hiç kare çizilmemiş bir koşuyla karışırdı
        // (`ornek=off` ile aynı kural).
        //
        // Bu kol jeton satırında **erişilemez** — kare çizilmemişse `kare=0`
        // ve kapı `MissingCounter` diyor, yani satır hiç basılmıyor. Yine de
        // sınanıyor: `Report` onu temsil edebiliyor ve kapının `sessiz` kolu
        // `None`'ı tabanın altıyla aynı sepete koyuyor
        // (`a_short_tail_fails_the_gate`).
        let mut r = report(smoke_counters(), Workload::Smoke);
        r.quiet = None;
        let line = r.token_line();
        assert!(line.contains(" sessiz=none "), "{line}");
        assert!(!line.contains("sessiz=0"), "{line}");
    }

    #[test]
    fn quiet_phrase_stays_out_of_the_token_contract() {
        // Tanı öbeği düşen koşunun **tek** `sessiz` kaydı (jeton satırı yalnız
        // yeşilde basılıyor), ama jeton gibi görünmemeli: `sessiz=` arayan bir
        // okuyucu düşen koşudan sayı okumamalı.
        let phrase = quiet_phrase(Some(Duration::from_millis(1745)));
        assert!(phrase.contains("1745.00ms"), "{phrase}");
        assert!(!phrase.contains("sessiz="), "{phrase}");
        assert_eq!(quiet_phrase(None), "hiç çizilen kare yok");
    }

    #[test]
    fn measured_tokens_report_every_column() {
        // Kapı açıkken üç sütunun **her biri** kendi jetonunu alıyor ve GPU'nun
        // kısa kalması görünür oluyor: `ornek=` ile `gpu_ornek=` ayrı sayılar,
        // çünkü Metal'in sıfır damgası bir kareyi CPU'ya yazdırıp GPU'ya
        // yazdırmayabiliyor.
        let mut r = report(smoke_counters(), Workload::Load);
        r.measured = Some(Measured {
            startup: Some(Duration::from_millis(284)),
            cpu_samples: 594,
            dropped: 2,
            gpu_samples: 591,
            gpu_rejected: 3,
            cpu_frame: Some((Duration::from_micros(1800), Duration::from_micros(4100))),
            cpu_encode: None,
            gpu: Some((Duration::from_micros(2200), Duration::from_micros(5000))),
        });
        let line = r.token_line();
        for token in [
            "ornek=594",
            "dusen=2",
            "gpu_ornek=591",
            "gpu_elenen=3",
            &format!("taban={MIN_SAMPLES}"),
            "cpu_kare_p95=1.80ms",
            "cpu_kare_max=4.10ms",
            "gpu_p95=2.20ms",
            "gpu_max=5.00ms",
            "acilis=284.00ms",
        ] {
            assert!(line.contains(token), "{token} yok: {line}");
        }
        // Taban altındaki sütun sayı **basmıyor** (R5.6) ve sebebi aynı
        // satırdaki `ornek=`/`taban=` çiftinde okunuyor. p95 ile en kötü
        // birlikte susuyor: az örnekte ikisi zaten aynı elemandır.
        assert!(line.contains("cpu_encode_p95=insufficient"), "{line}");
        assert!(line.contains("cpu_encode_max=insufficient"), "{line}");
    }

    #[test]
    fn cell_metrics_come_from_outside() {
        // Yer tutucunun ölmüş olmasının sınanabilir hâli: aynı pencere, iki
        // farklı hücre ölçüsü, iki farklı grid. Gövdeye geri sızan bir sabit
        // ikisini eşitler ve bu sınama düşer.
        // Pay sıfır: sorulan şey hücre ölçüsünün grid'i belirlediği, payın
        // etkisi değil. Payın kendi sınaması `the_gutter_costs_columns`.
        let narrow = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK);
        let wide = split_into_grid(900.0, 600.0, metrics(18, 36, 0), NO_DOCK);
        assert_eq!((narrow.cols, narrow.rows), (100, 33));
        assert_eq!((wide.cols, wide.rows), (50, 16));
    }

    #[test]
    fn the_gutter_costs_columns() {
        // Sol pay sütunlardan düşülür (010 Karar 3): şerit metnin üstüne
        // binmesin. 900 piksel, 9 piksel hücre → paysız 100 sütun; 8 piksel
        // pay bir sütun götürür, 9 piksel (tam bir hücre) de bir.
        let plain = split_into_grid(900.0, 600.0, metrics(9, 18, 0), NO_DOCK);
        let gutter = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK);
        assert_eq!(plain.cols, 100);
        assert_eq!(gutter.cols, 99, "pay bir sütun götürür");
        // Satırlar payı **görmez**: pay yalnız solda ve dikey geometriye
        // dokunmuyor.
        assert_eq!(gutter.rows, plain.rows);
        // Pay ölçüyle birlikte taşınıyor: grid'i kuran değer onu geri veriyor
        // ve çizim orijini ile fare eşlemesi aynı değeri okuyor.
        assert_eq!(gutter.cell.gutter_px(), 8);
    }

    #[test]
    fn the_dock_costs_rows_and_only_when_there_is_one() {
        // Dock payı **satırlardan** düşülür ve sol payın tersine koşullu:
        // dock'u olmayan pencereden (entegrasyonsuz kabuk, duman reçetesi)
        // tek satır bile gitmemeli — `smoke_shell`'in `hucre=8 glif=6`
        // sözleşmesi o pencerede ölçülüyor.
        let without = split_into_grid(900.0, 600.0, metrics(9, 18, 8), NO_DOCK);
        let with = split_into_grid(900.0, 600.0, metrics(9, 18, 8), DOCK_ROWS);
        // 600 / 18 = 33.3 → 33.
        assert_eq!(without.rows, 33);
        // Dock **iki satır, iki nefes payı ve bir satır arası** götürüyor:
        // 2×18 + 2×8 + 16 = 68 px, yani 532 / 18 = 29.5 → 29. Satır arası
        // (`dock_row_gap`) dış payın **iki katı**, çünkü ortasından bir çizgi
        // geçiyor ve çizginin iki yanına birer pay düşüyor; hesaba girmezse
        // 52 px çıkar, o da 30 satır verir ve fark **görünür** olur.
        // Sayının kaynağı `DOCK_ROWS` değil `bt_gpu::dock_px`; ikisi
        // ayrışırsa burası kızarır.
        assert_eq!(with.rows, 29, "dock payı satırlardan düşmedi");
        // Sütunlar dock'u **görmez**: dock ızgarayla aynı sütunları kullanıyor
        // ve payı yalnız dikeyde.
        assert_eq!(with.cols, without.cols);
    }

    #[test]
    fn the_dock_breathing_room_scales_with_the_gutter() {
        // Nefes payı **türetilmiş**, seçilmiş değil: kaynağı sol payın ta
        // kendisi. Sabit bir piksel sayısı olsaydı Cmd +/− ile punto büyürken
        // pay aynı kalır ve oran bozulurdu; bu sınama tam da o bağı tutuyor.
        let tight = split_into_grid(900.0, 600.0, metrics(9, 18, 0), DOCK_ROWS);
        let loose = split_into_grid(900.0, 600.0, metrics(9, 18, 8), DOCK_ROWS);
        // Paysız dock yalnız satırlarını götürür: 600 − 36 = 564 → 31.
        assert_eq!(tight.rows, 31);
        assert!(
            loose.rows < tight.rows,
            "pay büyüdü ama dock aynı yeri kapladı: {} / {}",
            loose.rows,
            tight.rows
        );
    }

    #[test]
    fn the_alternate_screen_takes_the_dock_and_gives_it_back() {
        // Alternatif ekranda pay sıfır, çıkışta **doğum değeri** geri geliyor.
        assert_eq!(dock_rows_for(true, DOCK_ROWS), 0);
        assert_eq!(dock_rows_for(false, DOCK_ROWS), DOCK_ROWS);
        // **Doğum değeri ayrı bir girdi olmasının sebebi bu satır:**
        // dock'u hiç olmayan bir pencerede (entegrasyonsuz kabuk, duman
        // reçetesi) alternatif ekrandan çıkmak dock **doğurmamalı**. Tek bir
        // alanın üstüne yazılsaydı geri getirilecek değer `DOCK_ROWS`
        // sabitinden kurulur ve tam da bu pencere dock kazanırdı.
        assert_eq!(dock_rows_for(true, NO_DOCK), 0);
        assert_eq!(dock_rows_for(false, NO_DOCK), 0);
    }

    #[test]
    fn a_window_shorter_than_the_dock_yields_no_rows() {
        // `a_window_narrower_than_the_gutter_yields_no_columns`'ın dikey
        // ikizi ve aynı kırılmaya bekçi: çıkarma `f64`'te negatife iniyor ve
        // `as u16` sıfıra doyuruyor. `u16`'da yapılsaydı taşar ve 65535
        // satırlık bir `TIOCSWINSZ` üretirdi. Sıfır satırı `Session::resize`
        // zaten yoksayıyor.
        let g = split_into_grid(900.0, 20.0, metrics(9, 18, 8), DOCK_ROWS);
        assert_eq!(g.rows, 0);
        // Sütunlar ayakta: alçak pencere yalnız satırları eliyor.
        assert_eq!(g.cols, 99);
    }

    #[test]
    fn a_window_narrower_than_the_gutter_yields_no_columns() {
        // Kabul: yeni bir alt sınır **getirilmiyor**, mevcut zincir doğru
        // cevabı veriyor. Çıkarma `f64`'te negatife iniyor, bölme negatif
        // kalıyor ve `as u16` sıfıra doyuruyor; sıfır sütunu `Session::resize`
        // zaten yoksayıyor. Aynı çıkarma `u16`'da yapılsaydı **taşar** ve
        // 65535'e yakın bir sütunla o boyda bir `TIOCSWINSZ` üretirdi — bu
        // sınamanın bekçilik ettiği kırılma o.
        let g = split_into_grid(4.0, 600.0, metrics(9, 18, 8), NO_DOCK);
        assert_eq!(g.cols, 0);
        // Satırlar ayakta: dar pencere yalnız sütunları eliyor.
        assert_eq!(g.rows, 33);
    }

    #[test]
    fn idle_limit_catches_excess_frames() {
        // Üst sınırın operandı `icerik`, alt sınırınki `kare`. Sağlıklı bir
        // koşuda ikisi eşit olduğu için yardımcılar `kare = icerik` kuruyor;
        // ikisinin ayrıştığı durumun kendi sınaması aşağıda.
        let counters = |content, cells, glyphs, rules| Counters {
            frames: content,
            content,
            cells,
            glyphs,
            rules,
            // Sağlıklı bir duman koşusunun izi; bu sınamanın sorduğu şey
            // `icerik` sınırı, hareketin kendi kapısı aşağıda.
            motion: 3,
            slide: 2,
        };
        let clean = Some(Teardown::Clean);
        let settled = MotionState::Settled;
        let smoke = |n, k, g, r| {
            verdict(
                counters(n, k, g, r),
                Workload::Smoke,
                clean,
                settled,
                HEALTHY_QUIET,
            )
        };
        let load = |n, k, g, r| {
            verdict(
                counters(n, k, g, r),
                Workload::Load,
                clean,
                settled,
                HEALTHY_QUIET,
            )
        };
        let excess = Verdict::ExcessFrames {
            limit: IDLE_FRAME_LIMIT,
        };

        // **Bu değişikliğin tamamı bu iki satırda.** Kapı `kare`'ye bakmayı
        // bıraktı: hareket kareleri `kare`'yi meşru olarak şişirecek ve sınır
        // onları görmemeli. Ters yön de bağlı — `icerik` taşarsa `kare`'nin
        // düşük olması kurtarmıyor.
        let mixed = |frames, content| {
            verdict(
                Counters {
                    frames,
                    content,
                    cells: 8,
                    glyphs: 6,
                    rules: 15,
                    motion: 3,
                    slide: 2,
                },
                Workload::Smoke,
                clean,
                settled,
                HEALTHY_QUIET,
            )
        };
        assert_eq!(mixed(200, 1), Verdict::Pass, "hareket karesi kapıya girmez");
        assert_eq!(mixed(1, 200), excess, "içerik karesi kapıdan kaçamaz");

        // Bugünkü duman koşusunun ta kendisi: bir kare, sekiz hücre, altı
        // glyph, on beş kural.
        assert_eq!(smoke(1, 8, 6, 15), Verdict::Pass);
        // Sınırın kendisi geçer, bir fazlası düşer. Eski kapı (`n > 0`) sıfırı
        // görüyordu ama fazlayı görmüyordu ve boşta sıfır kareyi bozan bir
        // değişikliğin belirtisi tam olarak fazla kare.
        assert_eq!(smoke(IDLE_FRAME_LIMIT, 8, 6, 15), Verdict::Pass);
        assert_eq!(smoke(IDLE_FRAME_LIMIT + 1, 8, 6, 15), excess);
        // Sağlıklı koşunun **ölçülen** tavanı (2026-09-12, otuz bir koşuda
        // bir kez): dört kare meşru ve geçmeli. Eski sınır (`2`) tam burada
        // doğru bir build'i kırmızıya düşürüyordu.
        assert_eq!(smoke(4, 8, 6, 15), Verdict::Pass);
        // Bozuk koşunun **ölçülen** alt ucu: boşta sıfır kare bilerek
        // bozulduğunda dokuz koşuda en düşük sayı 49'du. Sınır bunu yakalamak
        // zorunda.
        assert_eq!(smoke(49, 8, 6, 15), excess);
        assert_eq!(smoke(354, 8, 6, 15), excess);
        // `Load` yükünde akış işin kendisi: aynı sayı geçmeli. Sınırın yüke
        // bağlı olduğu tek yerde yazılı ve burada sınanıyor.
        assert_eq!(load(354, 8, 6, 15), Verdict::Pass);
        // Ölçüm yükünün **gerçek** sayıları: düz metin akıyor, arka plan ve
        // kural yapısal olarak sıfır. Dört sayaç da sorulsaydı her ölçüm
        // koşusu kırmızı düşerdi. `glif=1836` üç ölçümde de aynı çıktı
        // (2026-09-12); `kare` ise ortama bağlı ve tam bu yüzden `Load`
        // yükünde **kapı yok**: aynı makinede aynı komut bir rejimde 9 (2 sn)
        // ile 21 (5 sn), ötekinde 49–234 (2 sn) ile 597 (5 sn) verdi.
        assert_eq!(load(21, 0, 1836, 0), Verdict::Pass);
        assert_eq!(load(594, 0, 1836, 0), Verdict::Pass);

        // Sıfır, fazladan **önce** gelir: ikisi birden bozuksa okuyan taraf
        // önce eksik halkayı arasın. Kolların sırasını ters çeviren bir
        // değişiklik burada kırmızı düşer.
        assert_eq!(
            smoke(200, 0, 6, 15),
            Verdict::MissingCounter {
                required: "beşi de >0 olmalı"
            }
        );

        // Alt sınır iki yükte de duruyor ve her sayaç ayrı bir kapı. Karar
        // `MissingCounter` olmalı, `ExcessFrames` değil: düşen koşu okuyanı
        // doğru arızaya göndermeli.
        for (got, required) in [
            (smoke(0, 8, 6, 15), "beşi de >0 olmalı"),
            (smoke(1, 0, 6, 15), "beşi de >0 olmalı"),
            (smoke(1, 8, 0, 15), "beşi de >0 olmalı"),
            (smoke(1, 8, 6, 0), "beşi de >0 olmalı"),
            (load(0, 0, 1836, 0), "kare ve glif >0 olmalı"),
            (load(3, 0, 0, 0), "kare ve glif >0 olmalı"),
        ] {
            assert_eq!(got, Verdict::MissingCounter { required });
        }
    }

    #[test]
    fn an_unsettled_animation_fails_the_gate() {
        // Kapının **ölçüm istemeyen** yarısı ve `IDLE_FRAME_LIMIT`'in
        // göremediği sızıntı sınıfı: sayaçların hepsi yerinde, kare sınırı
        // aşılmamış — yavaş bir animasyon ikisini de geçer — ama deadline'da
        // hâlâ uçuşta. Phase-3'ün kabulündeki "geçici mutasyon: `settled` hep
        // `false`" senaryosunun saf hâli.
        let good = Counters {
            frames: 1,
            content: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 3,
            slide: 2,
        };
        let clean = Some(Teardown::Clean);
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                clean,
                MotionState::Unsettled,
                HEALTHY_QUIET
            ),
            Verdict::MotionUnsettled
        );
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                clean,
                MotionState::Settled,
                HEALTHY_QUIET
            ),
            Verdict::Pass
        );

        // **Ölçüm yükü muaf**: `Load` deadline'a kadar çıktı akıtıyor, yani
        // son satırla birlikte imleç hedef değiştiriyor ve deadline yayın
        // ortasına düşüyor. Bağlansaydı her ölçüm koşusu kod doğruyken
        // kırmızı düşerdi.
        assert_eq!(
            verdict(
                Counters {
                    cells: 0,
                    rules: 0,
                    motion: 0,
                    slide: 0,
                    ..good
                },
                Workload::Load,
                clean,
                MotionState::Unsettled,
                HEALTHY_QUIET,
            ),
            Verdict::Pass
        );

        // Hiç hareket karesi çizilmemişse arıza **yerleşmeme değil eksik
        // sayaç**: duman reçetesinde bir imleç hareketi var, yani sıfır
        // "animasyon yolu hiç koşmadı" demek ve okuyanı oraya göndermeli.
        assert_eq!(
            verdict(
                Counters { motion: 0, ..good },
                Workload::Smoke,
                clean,
                MotionState::Settled,
                HEALTHY_QUIET,
            ),
            Verdict::MissingCounter {
                required: "beşi de >0 olmalı"
            }
        );

        // Kare sınırı yerleşmeden **önce** geliyor: ikisi birden bozuksa
        // okuyan taraf önce akan kareyi görsün.
        assert_eq!(
            verdict(
                Counters {
                    content: IDLE_FRAME_LIMIT + 1,
                    ..good
                },
                Workload::Smoke,
                clean,
                MotionState::Unsettled,
                HEALTHY_QUIET,
            ),
            Verdict::ExcessFrames {
                limit: IDLE_FRAME_LIMIT
            }
        );
    }

    #[test]
    fn a_short_tail_fails_the_gate() {
        // Kapının üçüncü katı ve tek ölçülmüş eşiği: sayaçlar yerinde, içerik
        // karesi sınırın **altında**, animasyon yerleşmiş — ama son kareyle
        // deadline arası kısa, yani koşunun sonunda hâlâ kare akıyordu.
        // Ölçülen senaryosu yarım saniyelik sızıntı: `icerik=8` ile sınırı
        // aşmıyor ve bu kol olmadan **yeşil** düşüyordu
        // (`docs/OLCUMLER.md` → `## Boşta kare`).
        let good = Counters {
            frames: 30,
            content: 3,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 27,
            slide: 2,
        };
        let clean = Some(Teardown::Clean);
        let settled = MotionState::Settled;
        let smoke = |quiet| verdict(good, Workload::Smoke, clean, settled, quiet);
        let short = Verdict::QuietTooShort { floor: QUIET_FLOOR };

        // Ölçülen yavaş sızıntının kuyruğu (en yükseği `129,25 ms`) ve
        // ölçülen sağlıklı kuyruğun en düşüğü (`1742,29 ms`): kapı ikisinin
        // arasından geçiyor ve iki dağılım da kendi tarafında kalıyor.
        assert_eq!(smoke(Some(Duration::from_millis(130))), short);
        assert_eq!(smoke(HEALTHY_QUIET), Verdict::Pass);
        // Tabanın kendisi geçer, bir milisaniye altı düşer.
        assert_eq!(smoke(Some(QUIET_FLOOR)), Verdict::Pass);
        assert_eq!(smoke(Some(QUIET_FLOOR - Duration::from_millis(1))), short);
        // `sessiz=none` uydurulmuş bir sıfır değil ama kapı için aynı yanıt:
        // hiç kare çizilmemiş bir koşunun sessizliği de ölçülemez.
        assert_eq!(smoke(None), short);

        // **Ölçüm yükü muaf** ve gerekçesi `MotionUnsettled`'ınkiyle aynı:
        // `Load` deadline'a kadar çıktı akıtıyor, yani orada sessizlik sıfıra
        // yakın olmak zorunda. Bağlansaydı her ölçüm koşusu kırmızı düşerdi.
        assert_eq!(
            verdict(
                Counters {
                    cells: 0,
                    rules: 0,
                    motion: 0,
                    slide: 0,
                    ..good
                },
                Workload::Load,
                clean,
                settled,
                Some(Duration::ZERO),
            ),
            Verdict::Pass
        );

        // Sıra: üçü de bozuksa satır en temel arızayı yazar. Sessizlik en
        // sonda, çünkü ötekiler sızıntıyı **adıyla** tanıyor.
        assert_eq!(
            verdict(
                Counters {
                    content: IDLE_FRAME_LIMIT + 1,
                    ..good
                },
                Workload::Smoke,
                clean,
                settled,
                Some(Duration::ZERO),
            ),
            Verdict::ExcessFrames {
                limit: IDLE_FRAME_LIMIT
            }
        );
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                clean,
                MotionState::Unsettled,
                Some(Duration::ZERO),
            ),
            Verdict::MotionUnsettled
        );
        // Panik **kuyruktan da sonra**: ötekiler koşunun ölçtüğü şeyin
        // bozulduğunu söylüyor, panik koşu bittikten sonraki yolu ve
        // `kapanis=` jetonu onu zaten taşıyor. Kombinasyonun kendi sınaması
        // olmadan "en sonda" iddiası yalnız bir yorum cümlesi olurdu.
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                Some(Teardown::ReaderPanicked),
                settled,
                Some(Duration::ZERO),
            ),
            short
        );
    }

    #[test]
    fn motion_and_panic_report_the_more_fundamental_fault() {
        // Kolların sırası bir **tanı** tercihi: iki arıza birdenken koşu her
        // hâlükârda kırmızı, ama satır hangisini yazacak? `/code-review`
        // bulgusu bu kombinasyonun hiç sınanmamış olmasıydı.
        //
        // Yerleşmeme koşunun **ölçtüğü** şeyin bozulduğunu söylüyor, panik
        // koşu bittikten sonraki yolu; okuyanı önce ilkine göndermek doğru ve
        // `kapanis=` jetonu ikincisini zaten taşıyor. Ters çevirmek
        // `ExcessFrames`'in bugünkü sırasını da bozardı.
        let good = Counters {
            frames: 1,
            content: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 3,
            slide: 2,
        };
        assert_eq!(
            verdict(
                good,
                Workload::Smoke,
                Some(Teardown::Panicked),
                MotionState::Unsettled,
                HEALTHY_QUIET,
            ),
            Verdict::MotionUnsettled
        );
        // Panik tek başınayken yine görülüyor: sıra onu **yutmuyor**.
        assert!(matches!(
            verdict(
                good,
                Workload::Smoke,
                Some(Teardown::Panicked),
                MotionState::Settled,
                HEALTHY_QUIET,
            ),
            Verdict::ShutdownPanicked { .. }
        ));
    }

    #[test]
    fn shutdown_panic_cannot_pass_the_gate() {
        // `/code-review` bulgusu: `kapanis=` jetonu görünür oldu ama kapı onu
        // okumuyordu, yani kapanış yolunda panikleyen bir koşu hâlâ
        // `pipeline=ok` basıp 0 ile çıkıyordu — jetonun eklenme gerekçesinin
        // tam tersi.
        let good = Counters {
            frames: 1,
            content: 1,
            cells: 8,
            glyphs: 6,
            rules: 15,
            motion: 3,
            slide: 2,
        };
        let settled = MotionState::Settled;
        for teardown in [Teardown::ReaderPanicked, Teardown::Panicked] {
            assert!(
                matches!(
                    verdict(
                        good,
                        Workload::Smoke,
                        Some(teardown),
                        settled,
                        HEALTHY_QUIET
                    ),
                    Verdict::ShutdownPanicked { .. }
                ),
                "{teardown:?} yeşil geçemez"
            );
            assert!(
                matches!(
                    verdict(good, Workload::Load, Some(teardown), settled, HEALTHY_QUIET),
                    Verdict::ShutdownPanicked { .. }
                ),
                "{teardown:?} ölçüm yükünde de yeşil geçemez"
            );
        }

        // Kayıtlı borçlar kapıya **bağlanmadı**: `Abandoned` ölçüm yükünün
        // dört koşusundan birinde oluyor (ölçüldü) ve `make duman`'ı bilinen
        // bir borç yüzünden kırmızıya düşürmek kapıyı işe yaramaz kılardı.
        for teardown in [
            Some(Teardown::Clean),
            Some(Teardown::Abandoned),
            Some(Teardown::Unbounded),
            Some(Teardown::AlreadyDone),
            None,
        ] {
            assert_eq!(
                verdict(good, Workload::Smoke, teardown, settled, HEALTHY_QUIET),
                Verdict::Pass
            );
        }

        // Eksik sayaç panikten **önce** geliyor: okuyanı önce daha temel
        // arızaya göndermek doğru.
        assert_eq!(
            verdict(
                Counters { frames: 0, ..good },
                Workload::Smoke,
                Some(Teardown::Panicked),
                settled,
                HEALTHY_QUIET,
            ),
            Verdict::MissingCounter {
                required: "beşi de >0 olmalı"
            }
        );
    }

    #[test]
    fn timed_run_does_not_see_the_user() {
        // Süreli koşu yükleyiciyi çağırmaz: ev dizini çözülse bile karar
        // `Hermetic`. `make duman` jetonları bu satıra yaslanıyor — `hucre=`
        // ve `glif=` makinenin fontuna, `hareket=` de makinenin
        // `[motion] cursor_motion`'ına bağlanmıyor. Hermetik koşuda stil
        // `Settings::default()`'tan geliyor (`start_session`), yani
        // varsayılanların tek sahibinden.
        let home = Some(PathBuf::from("/Users/someone"));
        for workload in [Workload::Smoke, Workload::Load] {
            let run = Run {
                seconds: 3,
                workload,
                stats_since: None,
            };
            assert_eq!(decide_inputs(Some(run), home.clone()), Inputs::Hermetic);
        }
        assert_eq!(
            decide_inputs(None, home),
            Inputs::User {
                config_root: Some(PathBuf::from("/Users/someone/.config/bateri"))
            }
        );
        assert_eq!(
            decide_inputs(None, None),
            Inputs::User { config_root: None }
        );
    }

    #[test]
    fn smooth_scroll_is_off_when_any_input_turns_motion_off() {
        // Üç girdiden biri hareketi kapatıyorsa satır adımı (027 Karar 5).
        let on = Settings::default();
        assert!(
            resolve_smooth_scroll(&on, false),
            "varsayılan pürüzsüz değil"
        );
        assert!(
            !resolve_smooth_scroll(&on, true),
            "Hareketi Azalt'ta süzüldü"
        );
        let off = Settings {
            smooth_scroll: SmoothScroll::Off,
            ..Settings::default()
        };
        assert!(!resolve_smooth_scroll(&off, false));
        let snap = Settings {
            cursor_motion: CursorMotion::Snap,
            ..Settings::default()
        };
        assert!(!resolve_smooth_scroll(&snap, false), "snap'te süzüldü");
        // Öteki iki stil kaymayı açık bırakıyor.
        for style in [CursorMotion::Ease, CursorMotion::Spring] {
            let settings = Settings {
                cursor_motion: style,
                ..Settings::default()
            };
            assert!(resolve_smooth_scroll(&settings, false), "{style:?}");
        }
        // Süreli koşu: ayar okunmuyor, Hareketi Azalt `false` çözülüyor, yani
        // pürüzsüz — sistem ayarına bağlanmadan.
        let reduce = resolve_reduce_motion(&Inputs::Hermetic, ReduceMotion::System, || {
            panic!("süreli koşu sistem ayarını okudu")
        });
        assert!(resolve_smooth_scroll(&Settings::default(), reduce));
    }

    #[test]
    fn hermetic_run_does_not_read_reduce_motion() {
        // `Inputs`'un beşinci koşulu (008 phase-5): süreli koşu sistemin
        // Hareketi Azalt ayarını **okumaz**. Okusaydı `make duman`'ın
        // `hareket=` jetonu ölçen makinenin erişilebilirlik tercihine
        // bağlanırdı — bir makinede yeşil, bir makinede kırmızı düşen bir kapı.
        // Closure'ın paniği bunu "okumadı" iddiasından daha keskin sınıyor:
        // dönüşü `false` sabitlemek, okuyup yok sayan bir kodu da geçirirdi.
        for setting in [ReduceMotion::System, ReduceMotion::On, ReduceMotion::Off] {
            assert!(
                !resolve_reduce_motion(&Inputs::Hermetic, setting, || panic!(
                    "süreli koşu sistem ayarını okudu"
                )),
                "{setting:?} hermetik koşuda hareketi kıstı"
            );
        }

        let user = Inputs::User { config_root: None };
        // `"on"` ve `"off"` kendileri karar veriyor: sisteme hiç gidilmiyor.
        assert!(resolve_reduce_motion(&user, ReduceMotion::On, || panic!(
            "\"on\" sistem ayarını okudu"
        )));
        assert!(!resolve_reduce_motion(&user, ReduceMotion::Off, || panic!(
            "\"off\" sistem ayarını okudu"
        )));
        // `"system"` yalnız sistemin dediğini yapar.
        assert!(resolve_reduce_motion(&user, ReduceMotion::System, || true));
        assert!(!resolve_reduce_motion(&user, ReduceMotion::System, || {
            false
        }));
    }

    /// Entegrasyonun kurulduğu kolun sabit girdisi: zsh + gövdeli bir dizin.
    fn zsh_and_dir() -> (
        impl FnOnce() -> Option<PathBuf>,
        impl FnOnce() -> Option<PathBuf>,
    ) {
        (
            || Some(PathBuf::from("/bin/zsh")),
            || Some(PathBuf::from("/opt/bateri/shell/zsh")),
        )
    }

    #[test]
    fn blocks_keeps_the_wrapper_and_drops_the_dock() {
        // **012 phase-10'un kabul kriteri.** `"blocks"` kademesinde sarmalayıcı
        // kuruluyor — `ZDOTDIR` gidiyor, yani bloklar ve işaretler çalışıyor —
        // ama pencere **dock'suz** doğuyor: giriş satırı da prompt da
        // ızgarada kalıyor.
        let user = Inputs::User { config_root: None };
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Blocks, shell, dir, None);
        assert!(
            env.iter().any(|(key, _)| key == "ZDOTDIR"),
            "blocks sarmalayıcıyı kurmadı: bloklar da ölürdü"
        );
        assert_eq!(
            dock_rows_at_birth(&env, ShellIntegration::Blocks),
            0,
            "blocks kademesinde dock payı ayrıldı: ekranda iki prompt olurdu"
        );

        // `"auto"` aynı ortamı kuruyor ve payı **ayırıyor**: iki kademeyi
        // ayıran şey ortam değil, bu karar.
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Auto, shell, dir, None);
        assert_eq!(dock_rows_at_birth(&env, ShellIntegration::Auto), DOCK_ROWS);

        // Sarmalayıcı hiç kurulmadıysa kademe ne olursa olsun pay yok:
        // dolduracak ayna yok.
        for setting in [
            ShellIntegration::Auto,
            ShellIntegration::Blocks,
            ShellIntegration::Off,
        ] {
            assert_eq!(dock_rows_at_birth(&[], setting), 0, "{setting:?}");
        }
    }

    #[test]
    fn hermetic_run_does_not_set_up_shell_integration() {
        // `Inputs`'un altıncı koşulu (009 phase-3): süreli koşu entegrasyonu
        // **hiç kurmaz**. Kursaydı `make duman`'ın sonucu ölçen makinenin
        // kabuk yapılandırmasına bağlanırdı — kullanıcının `.zshrc`'si
        // pencereye tek bir bayt bassa `hucre=8` düşerdi. Closure'ın paniği
        // "kurmadı" iddiasından keskin: boş dönüşü sabitlemek, kabuğu çözüp
        // sonucu atan bir kodu da geçirirdi.
        for setting in [ShellIntegration::Auto, ShellIntegration::Off] {
            let env = shell_integration_env(
                &Inputs::Hermetic,
                setting,
                || panic!("süreli koşu kabuğu çözdü"),
                || panic!("süreli koşu betiği aradı"),
                Some("/home/someone/zsh".into()),
            );
            assert!(env.is_empty(), "{setting:?} hermetik koşuda ortam ekledi");
        }
    }

    #[test]
    fn shell_integration_off_asks_nothing() {
        // `"off"` kendi başına karar veriyor: ne kabuk çözülüyor ne betik
        // aranıyor. Anahtarın anlamı "sarmalayıcıyı kurma" ve o iş burada
        // bitiyor — işaretleri ayrıştıran yol (`bt-core`) bu koldan geçmiyor,
        // yani başka bir aracın bastığı gerçek OSC 133 yine okunuyor.
        let env = shell_integration_env(
            &Inputs::User { config_root: None },
            ShellIntegration::Off,
            || panic!("\"off\" kabuğu çözdü"),
            || panic!("\"off\" betiği aradı"),
            None,
        );
        assert!(env.is_empty());
    }

    #[test]
    fn shell_integration_needs_zsh_and_a_script() {
        let user = Inputs::User { config_root: None };
        // Tanımadığımız kabuk: betik bile aranmıyor, çünkü kuracak bir şey yok.
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            || Some(PathBuf::from("/bin/bash")),
            || panic!("zsh olmayan kabukta betik arandı"),
            None,
        );
        assert!(env.is_empty(), "bash'e sarmalayıcı kuruldu");
        // Kabuk hiç çözülemedi (passwd okunamadı, `$SHELL` yok): aynı sessiz
        // geri düşüş.
        let env = shell_integration_env(&user, ShellIntegration::Auto, || None, || None, None);
        assert!(env.is_empty(), "kabuksuz oturuma sarmalayıcı kuruldu");
        // Kabuk zsh ama betik yok (eksik paket): entegrasyonsuz bir oturum,
        // yarım kurulmuş bir `ZDOTDIR`'dan iyi — kullanıcının yapılandırması
        // hiç yüklenmemiş olurdu.
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            || Some(PathBuf::from("/bin/zsh")),
            || None,
            None,
        );
        assert!(env.is_empty(), "betiksiz ZDOTDIR kuruldu");
    }

    #[test]
    fn shell_integration_hands_the_original_zdotdir_to_the_script() {
        let user = Inputs::User { config_root: None };
        // Kullanıcının `ZDOTDIR`'ı yok: betiğe yalnız kendi dizinimiz gidiyor
        // ve `BATERI_ZDOTDIR`'ın **yokluğu** "kullanıcının da yoktu" demek.
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Auto, shell, dir, None);
        assert_eq!(
            env,
            vec![("ZDOTDIR".to_owned(), "/opt/bateri/shell/zsh".to_owned())]
        );
        // Boş değer tanımsız sayılıyor (`decide_locale`'in kuralı): "geri
        // koymak" `$HOME`'u gösteren bir değişken yaratmak olurdu.
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            shell,
            dir,
            Some(OsString::new()),
        );
        assert_eq!(env.len(), 1, "boş ZDOTDIR geri konacak değer sayıldı");
        // Kullanıcının `ZDOTDIR`'ı var: betik onu geri koyabilsin diye ikinci
        // çift de gidiyor.
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            shell,
            dir,
            Some("/home/someone/zsh".into()),
        );
        assert_eq!(
            env,
            vec![
                ("ZDOTDIR".to_owned(), "/opt/bateri/shell/zsh".to_owned()),
                ("BATERI_ZDOTDIR".to_owned(), "/home/someone/zsh".to_owned()),
            ]
        );
    }

    #[test]
    fn only_a_shell_prompt_is_announced_to_the_script() {
        // **Varsayılanın tek kaydı betikte.** `"terminal"` kolunda ortama tek
        // bayt eklenmiyor: betiğin "değişken yok → prompt terminalin" kuralı
        // varsayılanı tek başına taşıyor. Burada da bir değer gönderilseydi
        // varsayılan iki yerde yazılı olur ve biri değişince öteki sessizce
        // eskirdi.
        let user = Inputs::User { config_root: None };
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Auto, shell, dir, None);
        assert!(
            !env.iter().any(|(key, _)| key == "BATERI_DOCK"),
            "varsayılan kademe ortama bir şey yazdı"
        );

        // `"blocks"`: sarmalayıcı **kuruluyor** (bloklar ve işaretler için) ama
        // giriş satırı ile prompt kabuğun kalıyor. Betiğe giden tek fark bu
        // değişken; dock payının ayrılmaması ayrı bir karar ve bu tarafta
        // (`ShellIntegration::wants_dock`, `birth`).
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(&user, ShellIntegration::Blocks, shell, dir, None);
        assert_eq!(
            env,
            vec![
                ("ZDOTDIR".to_owned(), "/opt/bateri/shell/zsh".to_owned()),
                ("BATERI_DOCK".to_owned(), "off".to_owned()),
            ]
        );

        // `"off"` üç kademenin dışında: sarmalayıcı kurulmadan prompt'u kim
        // çizdiğinin bir anlamı yok ve "hiçbir şey kurulmaz" sözü mutlak.
        let env = shell_integration_env(
            &user,
            ShellIntegration::Off,
            || panic!("\"off\" kabuğu çözdü"),
            || panic!("\"off\" betiği aradı"),
            None,
        );
        assert!(env.is_empty());
    }

    #[test]
    fn a_self_referential_zdotdir_is_not_handed_back() {
        // Ortamdaki `ZDOTDIR` zaten **betiğin kendi dizini**: geri konacak bir
        // "kullanıcı değeri" yok. Verilseydi betik kendi `.zshenv`'ini yeniden
        // yükler ve zsh'in `FUNCNEST` sınırına kadar özyinelerdi; ölçülen
        // sonuç 336 satır hata ve `ZDOTDIR`'sız kalan bir oturumdu
        // (`/code-review`, 009 kapısı).
        let user = Inputs::User { config_root: None };
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            shell,
            dir,
            Some("/opt/bateri/shell/zsh".into()),
        );
        assert_eq!(
            env,
            vec![("ZDOTDIR".to_owned(), "/opt/bateri/shell/zsh".to_owned())],
            "kendine dönük ZDOTDIR betiğe geri verildi"
        );
    }

    #[test]
    fn a_non_utf8_zdotdir_refuses_the_integration() {
        // "Yok" ile "kullanılamaz" ayrı: `var().ok()` ikisini birleştiriyordu
        // ve sonuç sessiz bir veri kaybıydı — betik "kullanıcının yoktu"
        // sanıp oturum sonunda `ZDOTDIR`'ı **siler**, yani kullanıcının bütün
        // yapılandırması tanısız kaybolurdu. Entegrasyonu hiç kurmamak,
        // komşu kenarların (UTF-8 olmayan betik yolu, tanınmayan `$SHELL`)
        // zaten seçtiği geri düşüş.
        use std::os::unix::ffi::OsStringExt as _;
        let user = Inputs::User { config_root: None };
        let (shell, dir) = zsh_and_dir();
        let env = shell_integration_env(
            &user,
            ShellIntegration::Auto,
            shell,
            dir,
            Some(OsString::from_vec(vec![0x2f, 0xff, 0xfe])),
        );
        assert!(
            env.is_empty(),
            "UTF-8 olmayan ZDOTDIR ile sarmalayıcı kuruldu"
        );
    }

    #[test]
    fn zero_window_does_not_panic() {
        // Simge durumuna alınan pencere 0×0 bounds verir; `Session::resize`
        // sıfır grid'i yoksayıyor ama buraya gelen yolun panik etmemesi
        // gerekiyor — bölme değil, `as u16` doygunluğu taşıyor.
        let g = split_into_grid(0.0, 0.0, metrics(9, 18, 8), NO_DOCK);
        assert_eq!((g.cols, g.rows), (0, 0));
    }
}
