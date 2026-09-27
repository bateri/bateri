//! bt-shell — AppKit kabuğu: pencere, sekme, bölme, menü, klavye, servisler.
//!
//! `objc2-app-kit` üzerinden doğrudan AppKit; Metal'i görmez, çizimi
//! `bt-gpu`'ya bırakır ve device'ı `Renderer::system_default` kurar — pane
//! başına bir renderer (`pane`). Kareyi de sürmez: pane'i, oturumu ve
//! display link'i birbirine bağlar, gerisi `bt-gpu`'nun ritmidir. Uygulama
//! geneli (`app`), pencere başına olan (`window`: krom, sekme, kapatma
//! sorusu) ve oturum başına olan (`pane`: `NSView` alt sınıfı, oturumun
//! çekirdeği) ayrı nesnelerde. Klavye buradan PTY'ye akar (`keys`, `view`,
//! `clipboard`), fare de buradan oturuma (seçim ve kaydırma, `view`);
//! Finder'dan bırakılan dosyanın yolu da buradan giriş satırına düşer
//! (`view`'ın sürükleme hedefi + `quote`'un kabuk kaçışı);
//! kabuğun hangi dizinde ve hangi yerelle açılacağına (`child`) ve kapanış
//! sırasına da bu crate karar verir; kabuğun ön planında koşan işi süreç
//! tablosundan okuyan da (`jobs`). Ayar dosyasını okuyan (`settings`),
//! kayıt anında yeniden okuyabilsin diye izleyen (`watch`) ve tanısını
//! pencere alt başlığında gösteren (`notices`) de burası; ayrıştırma ve fark
//! `bt-core`'da; ayar penceresi (`settings_window`, Settings…) dosyaya o
//! yazma yolundan yazar. Sistemin açık/koyu görünümünü okuyup temayı seçen de
//! (`app`, `NSApp.effectiveAppearance`'ın KVO'suyla) ve pencere kromunu
//! temaya boyayan (`window`). Ana menü (`menu`) uygulama, Shell,
//! Edit, View ve Window menüsü; öğeleri hedefsiz eylem. View'da Theme ▸ seçimi
//! ayar dosyasına yazar (`settings`), Cmd +/−/0 dosyaya dokunmayan ve pencereye
//! ait geçici punto (`zoom`).
//! **Çok pencere ve macOS'un kendi sekmeleri** (`.tasks/026-sekmeler`): her
//! sekme bir `NSWindow` ve tek pane'inde kendi oturumu (`window`, `pane`); pencereleri açan,
//! listeleyen ve kapanışı paralel yürüten `app`. Bölme ve IME sonraki setlerde.

pub(crate) mod app;
mod child;
mod clipboard;
mod gesture;
mod jobs;
mod keys;
mod menu;
mod notices;
mod pane;
mod quote;
mod search_bar;
mod settings;
mod settings_window;
mod upload;
mod uploader;
mod view;
mod watch;
mod window;
mod zoom;

use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

use bt_core::SHUTDOWN_GRACE;

pub use bt_gpu::GpuError;

/// Duman ve ölçüm koşularının shell'i. Kullanıcının `$SHELL`'i **değil**:
/// sonuç rc dosyasına bağlı olmasın.
///
/// Ayrı bir tip olmasının sebebi `run_seconds.is_some()`'ın taşıdığı üç ayrı
/// anlam: sabit shell seç, deadline kur, bekçiyi kur. Yük seçimi yalnız
/// birincisini ilgilendiriyor; ayrılmazsa ölçüm koşusu ya deadline'ı ya
/// bekçiyi kaybeder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workload {
    /// `make duman`: tek atış, sonra boşta. `hucre`/`glif`/`kural`
    /// sayılarının kaynağı ve boşta sıfır karenin bekçisi — içerik karesi
    /// burada **üst sınırlı** (`app::IDLE_FRAME_LIMIT`), koşunun sonundaki
    /// sessizlik ise **alt sınırlı** (`app::QUIET_FLOOR`).
    Smoke,
    /// `BT_SCROLL_TEST`: koşu boyunca akan çıktı. Kare akışı işin kendisi,
    /// üst sınır yok.
    Load,
}

impl Workload {
    /// `yuk=` jetonunun değeri.
    ///
    /// Dizgi tipin **yanında**, çağrı yerinde değil: jeton bir makine
    /// sözleşmesi ve sözleşmenin metni tanımın yanında yaşar. Çağrı yerinde
    /// dursaydı üçüncü bir yük eklendiğinde eşlemeyi derleyici değil okuyan
    /// hatırlamak zorunda kalırdı.
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Smoke => "smoke",
        }
    }
}

/// Süreli koşu: `make duman` ve ölçüm. `None` → kullanıcının kendi oturumu.
///
/// Üç alan **birlikte** doğuyor ve tek bir `Option`'ın altında duruyor, çünkü
/// üçü de süreye bağlı: süresiz bir yük hiç bitmez, süresiz bir ölçüm de hiç
/// raporlanmaz (rapor `report_and_exit`'te ve oraya yalnız deadline varır).
/// Ayrı `Option`'lar olsaydı tip bu imkânsız durumlara izin verir ve bedeli
/// `unwrap_or(0)` ile "ulaşılmaz dal" yorumlarına çıkardı.
#[derive(Clone, Copy, Debug)]
pub struct Run {
    /// `BT_RUN_SECONDS`: dolunca kare sayısına bakıp çıkılır (`make duman`).
    pub seconds: u64,
    /// Hangi sabit shell.
    pub workload: Workload,
    /// `BT_FRAME_STATS`: `Some` ise ölçüm açık **ve** damga `main()`'in ilk
    /// satırında alınmış (süreç başlangıcı **değil**; damganın iki ucu da
    /// [`bt_gpu::Stats::startup`]'ta). `bool` olsaydı damgayı bayrağın
    /// okunduğu bir yerde almak gerekirdi — [`run`]'da ya da daha geç, yani
    /// en geç ilk pencerenin `Renderer::system_default()`'undan hemen önce ve
    /// sıranın doğruluğu bir yoruma kalırdı. İlk renderer artık ana döngü
    /// başladıktan sonra, ilk pencereyle doğuyor; damga yine de ondan
    /// **önce**, açılışın en pahalı parçası (Metal device kurulumu, metallib
    /// yüklemesi) ölçünün içinde.
    pub stats_since: Option<Instant>,
}

pub struct Options {
    pub run: Option<Run>,
}

/// Uygulamayı kurar ve `NSApplication::run` ile ana döngüye girer. **Dönmez:**
/// etkileşimli oturum yalnız Quit (Cmd-Q, `terminate:`) ile çıkar — son
/// pencere kapanınca da, kabuk çıkınca da (o pencere kapanır) uygulama açık
/// kalır. `BT_RUN_SECONDS` yolu `process::exit` ile çıkar; orada kabuğun
/// çıkışı (`child_exit` → `terminate:`) ve son pencerenin kapanması da süreci
/// bitiriyor. `Ok(())` yalnız AppKit'in `run`'ı bir gün dönerse görülür.
///
/// **`Err` bugün dönmüyor.** Renderer'ı artık ilk pencere kuruyor (pencere
/// başına renderer, `.tasks/026-sekmeler/discussion.md` → Karar 2a) ve o an
/// ana döngünün içindeyiz: kurulum hatası `applicationDidFinishLaunching:`'te
/// aynı satırla (`bateri: {hata}`) ve aynı çıkış koduyla (1) basılıyor. İmza
/// bin crate'inin çağrı yerini değiştirmemek için duruyor.
///
/// Kapanış işi (PTY, ayar yazımı) buradan sonraya değil, **her iki çıkış
/// yolunun da geçtiği** `app::AppDelegate::shutdown`'a konur —
/// `applicationWillTerminate:`'a değil: duman deadline'ı ona bilerek uğramıyor
/// ve oraya konan bir adım o yolda sessizce atlanır.
pub fn run(opts: Options) -> Result<(), GpuError> {
    // audit: giriş noktası; ana thread dışından çağrılması programlama hatasıdır.
    let mtm = MainThreadMarker::new().expect("bt_shell::run ana thread'de çağrılır");
    // Açılış damgası ilk renderer'dan (ilk pencere) **önce** alınmış olmalı ve
    // tipi bunu zorluyor: `Options` bir `Instant` taşıyor, bir bayrak değil.
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // `delegate` bu kapsamda `app.run()`'ı aşar: AppKit'in ve pencerenin
    // delegate özellikleri zayıftır, sahip bu Retained'dır.
    let delegate = app::AppDelegate::new(mtm, opts);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    Ok(())
}

/// Bekçinin bütçesi: `bt-core`'un kapanış sınırı artı sabit bir pay.
///
/// **Koşu süresinden türemiyor artık ve sebebi kapsamın daralması.** Eski
/// ölçü `run_seconds × 3`'tü; o, bekçi kapanışın *tamamının* tek keseni iken
/// doğruydu. Kapanış [`bt_core::SHUTDOWN_GRACE`] ile sınırlandıktan sonra
/// bekçinin kapsamı "kapanış yolunun **başka** asılmaları"na daraldı ve
/// bunların hiçbiri koşu süresiyle ölçeklenmiyor: eski ölçü üç saniyelik
/// dumana 9 saniye, altmış saniyelik bir ölçüm koşusuna **3 dakika**
/// veriyordu.
///
/// Sayı **ölçüldü** (2026-09-12, debug, bu makine, altı koşu): kapanışın
/// **beşi** temiz bitti ve `SHUTDOWN_GRACE`'in çok altında kaldı (toplam süre
/// koşu süresini ~0,18 sn aşıyor ve o payın içinde açılış da var); **biri**
/// sınırı doldurdu (`kapanis=abandoned`) ve tam **+0,49 sn** sürdü. Yani ölçülen
/// tavan `SHUTDOWN_GRACE`'in kendisi. İki saniyelik pay bunun **beş katı**.
///
/// Yeni bütçeyle on ölçüm koşusu koşuldu ve **hiçbiri** bekçiye düşmedi
/// (`exit 70` yok) — eski bütçenin bu koşularda verdiği 6 saniyeye karşılık.
///
/// Kısa olsa ne kaybolur: sağlıklı ama yavaş bir kapanış `_exit(70)` ile
/// kesilir ve `make duman` yanlış arızayı gösterir. Uzun olsa ne kaybolur:
/// gerçekten asılan bir koşu o kadar bekletir — ve bu bir insanın önünde
/// değil, bir kapının içinde geçiyor.
const WATCHDOG_BUDGET: Duration = SHUTDOWN_GRACE.saturating_add(Duration::from_secs(2));

/// Kapanışın asılmasını kesen son çare — **yalnız `BT_RUN_SECONDS`
/// yolunda** ve kapanış başlarken kurulur (`AppDelegate::shutdown`).
///
/// Kapanış ana thread'de koşuyor ve oradan `Session::shutdown()`'a giriyor.
/// O çağrı artık **sınırlı bekliyor** (`bt-core`'un `SHUTDOWN_GRACE`'i), yani
/// bekçinin doğduğu asılma — ölmeyen ya da çıkışın içinde takılan bir çocuk —
/// ana thread'i artık tutamıyor ve bekçinin kapsamı **daraldı**: kapanış
/// yolundaki *başka* bir asılma (ana kuyruğa senkron iş atan bir `Drop`, kilit
/// sırasını bozan bir değişiklik) duman koşusunu süresiz bekletmesin diye
/// duruyor. Bu yüzden hâlâ ayrı bir thread: kesmesi gereken şey ana thread'in
/// kendisi.
///
/// Sınırın tek istisnası kapanış thread'inin kurulamaması (OS thread
/// sınırı). O dalda **kesen de kalmayabilir** ve bu vaat edilmiyor: thread
/// kurulamayan bir makinede bu bekçinin kendi thread'i de kurulamaz, yani
/// `Teardown::Unbounded` ile bekçisizlik aynı koşulda buluşur. İkisi de
/// stderr'e bir satır bırakıyor; sessiz kalan bir yol yok.
///
/// Bütçesi artık koşu süresinden değil [`WATCHDOG_BUDGET`]'ten geliyor, yani
/// argümansız: kestiği şeylerin hiçbiri koşu süresiyle ölçeklenmiyor.
///
/// Etkileşimli kullanımda bekçi **yoktur**; oradaki güvence `bt-core`'un
/// sınırı ve `Session::shutdown`'ın doc'u onun ne kapattığını, çocuğun
/// arkada kalmasının neden sürdüğünü anlatıyor.
pub(crate) fn watchdog() {
    // `thread::spawn` **değil**: o, thread kurulamayınca panikler ve buranın
    // çağrı yeri bir ObjC callback'i (`applicationWillTerminate:` /
    // `runDeadline:`). Panik `extern "C"` sınırından geçemez, yani süreç
    // **abort** eder: ne jeton satırı basılır ne `_exit(70)`. Üstelik bu tam
    // olarak `bt-core`'un `Teardown::Unbounded` ile hayatta kalmayı seçtiği
    // senaryo (OS thread sınırı) — ve o koşulda bu thread de kurulamaz, yani
    // bekçi **kesemez**. Hata yutulmuyor, söyleniyor: bekçisiz kalan bir
    // kapanış sessiz kalmamalı.
    let spawned = std::thread::Builder::new()
        .name("watchdog".to_owned())
        .spawn(|| {
            std::thread::sleep(WATCHDOG_BUDGET);
            // `eprintln!` DEĞİL: Rust'ın stderr'i kilitli ve ana thread o kilidi
            // tutarken asılmış olabilir (`shutdown`'ın kendi `eprintln!`'i,
            // `Retry::draw_failed`, ileride logger). Bekçi tam da onu kesmek için
            // var; aynı kilide girip beklemesi kendini iptal etmek olurdu. Sabit
            // metin, `format!` bile yok — `malloc` da bir kilit.
            //
            // `process::exit` de değil: o atexit zincirini ve stdio flush'ını
            // koşturur. 70 = EX_SOFTWARE; `make` bunu "Error 70" diye gösterir.
            //
            // SAFETY: `write` ve `_exit` async-signal-safe; ikisi de kilit almaz
            // ve süreci hiçbir şey koşturmadan bitirir.
            const MESSAGE: &str = "bateri: kapanış bekçinin bütçesinde bitmedi, süreç kesiliyor\n";
            unsafe {
                libc::write(2, MESSAGE.as_ptr().cast(), MESSAGE.len());
                libc::_exit(70)
            };
        });
    if let Err(err) = spawned {
        eprintln!("bateri: bekçi thread'i kurulamadı ({err}), kapanışı kesen yok");
    }
}
