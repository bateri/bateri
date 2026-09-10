//! bt-shell — AppKit kabuğu: pencere, sekme, bölme, menü, klavye, servisler.
//!
//! `objc2-app-kit` üzerinden doğrudan AppKit; Metal'i görmez, çizimi
//! `bt-gpu`'ya bırakır ve device'ı `Renderer::system_default` kurar. Kareyi
//! de sürmez: pencereyi, oturumu ve display link'i birbirine bağlar, gerisi
//! `bt-gpu`'nun ritmidir. Tek pencere; sekme, menü ve klavye sonraki setlerde.

mod app;

use std::sync::Arc;

use objc2::MainThreadMarker;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

pub use bt_gpu::GpuError;

pub struct Options {
    /// `BT_RUN_SECONDS`: dolunca kare sayısına bakıp çıkılır (`make duman`).
    pub run_seconds: Option<u64>,
}

/// Uygulamayı kurar ve `NSApplication::run` ile ana döngüye girer. **Dönmez:**
/// son pencere kapanınca AppKit `terminate:` ile, `BT_RUN_SECONDS` yolu
/// `process::exit` ile süreçten çıkar; `Ok(())` yalnız kurulum hatası yoksa ve
/// AppKit'in `run`'ı bir gün dönerse görülür. Kapanış işi (PTY, ayar yazımı)
/// buradan sonraya değil, AppKit'in `applicationWillTerminate:`'ına konur.
pub fn run(opts: Options) -> Result<(), GpuError> {
    // audit: giriş noktası; ana thread dışından çağrılması programlama hatasıdır.
    let mtm = MainThreadMarker::new().expect("bt_shell::run ana thread'de çağrılır");
    // `Arc`: renderer'ı hem delegate hem display link tutar.
    let renderer = Arc::new(bt_gpu::Renderer::system_default()?);
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    // `delegate` bu kapsamda `app.run()`'ı aşar: AppKit'in ve pencerenin
    // delegate özellikleri zayıftır, sahip bu Retained'dır.
    let delegate = app::AppDelegate::new(mtm, renderer, opts);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    Ok(())
}
