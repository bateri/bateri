//! bt-shell — AppKit kabuğu: pencere, sekme, bölme, menü, klavye, servisler.
//!
//! `objc2-app-kit` üzerinden doğrudan AppKit; Metal'i görmez, çizimi
//! `bt-gpu`'ya bırakır ve device'ı `Renderer::system_default` kurar. Kareyi
//! de sürmez: pencereyi, oturumu ve display link'i birbirine bağlar, gerisi
//! `bt-gpu`'nun ritmidir. Klavye buradan PTY'ye akar (`keys`, `view`);
//! kapanış sırasının sahibi de bu crate. Tek pencere; sekme, bölme, menü ve
//! IME sonraki setlerde.

mod app;
mod keys;
mod view;

use std::rc::Rc;
use std::time::{Duration, Instant};

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

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
    /// sayılarının kaynağı ve boşta sıfır karenin bekçisi — kare sayısı
    /// burada **üst sınırlıdır** (`app::IDLE_FRAME_LIMIT`).
    Smoke,
    /// `BT_SCROLL_TEST`: koşu boyunca akan çıktı. Kare akışı işin kendisi,
    /// üst sınır yok.
    Load,
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
    /// `BT_FRAME_STATS`: `Some` ise ölçüm açık **ve** damga süreç başında
    /// alınmış. `bool` olsaydı damgayı [`run`] içinde almak gerekirdi — yani
    /// `Renderer::system_default()`'tan sonra, açılışın en pahalı parçasını
    /// (Metal device kurulumu, metallib yüklemesi) kaçırarak.
    pub stats_since: Option<Instant>,
}

pub struct Options {
    pub run: Option<Run>,
}

/// Uygulamayı kurar ve `NSApplication::run` ile ana döngüye girer. **Dönmez:**
/// son pencere kapanınca ve shell çıkınca (`child_exit` → `terminate:`) AppKit
/// yoluyla, `BT_RUN_SECONDS` yolu `process::exit` ile süreçten çıkar; `Ok(())`
/// yalnız kurulum hatası yoksa ve AppKit'in `run`'ı bir gün dönerse görülür.
///
/// Kapanış işi (PTY, ayar yazımı) buradan sonraya değil, **her iki çıkış
/// yolunun da geçtiği** `app::AppDelegate::shutdown`'a konur —
/// `applicationWillTerminate:`'a değil: duman deadline'ı ona bilerek uğramıyor
/// ve oraya konan bir adım o yolda sessizce atlanır.
pub fn run(opts: Options) -> Result<(), GpuError> {
    // audit: giriş noktası; ana thread dışından çağrılması programlama hatasıdır.
    let mtm = MainThreadMarker::new().expect("bt_shell::run ana thread'de çağrılır");
    // `Rc`: renderer'ı hem delegate hem display link tutar, ama ikisi de ana
    // thread'de. `Arc` yanlış bir söz verirdi — `Renderer` glyph atlasını
    // taşıyor ve atlasın `CTFont`'u `Send` değil.
    // Açılış damgası bu satırdan **önce** alınmış olmalı ve tipi bunu zorluyor:
    // `Options` bir `Instant` taşıyor, bir bayrak değil.
    let renderer = Rc::new(bt_gpu::Renderer::system_default()?);
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // `delegate` bu kapsamda `app.run()`'ı aşar: AppKit'in ve pencerenin
    // delegate özellikleri zayıftır, sahip bu Retained'dır.
    let delegate = app::AppDelegate::new(mtm, renderer, opts);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    Ok(())
}

/// Kapanışın asılmasını kesen son çare — **yalnız `BT_RUN_SECONDS`
/// yolunda** ve kapanış başlarken kurulur (`AppDelegate::shutdown`).
///
/// Kapanış ana thread'de koşuyor ve oradan `Session::shutdown()`'a giriyor;
/// `Pty::drop` `SIGHUP`'tan sonra `child.wait()` çağırdığı için sinyali yutan
/// bir çocuk (`trap '' HUP`) ana thread'i süresiz bekletir. O noktada kesecek
/// kimse kalmıyor: bekçi bu yüzden ayrı bir thread.
///
/// Etkileşimli kullanımda bekçi **yoktur** ve böyle bir çocuk uygulamayı
/// gerçekten asar; bilinen sınır, `Session::shutdown`'ın kendi belgesinde de
/// yazılı. Kalıcı çözüm sınırlı bekleme (`SIGHUP` → süre → `SIGKILL`) ve yeri
/// `bt-core`.
pub(crate) fn watchdog(run_seconds: u64) {
    // Koşu süresinin üç katı. Sağlıklı bir kapanış `SIGHUP` ile hemen biter;
    // bu süreye ancak gerçekten asılmış bir çocuk varır. `max(1)`:
    // `BT_RUN_SECONDS=0` bekçiyi doğar doğmaz ateşlemesin.
    let budget = Duration::from_secs(run_seconds.saturating_mul(3).max(1));
    std::thread::spawn(move || {
        std::thread::sleep(budget);
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
}
