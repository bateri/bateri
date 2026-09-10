//! Uygulama delegate'i: pencereyi açar, CAMetalLayer'ı view'a takar, ilk
//! kareyi çizer, boyut değişince yeniden çizer. Tek ObjC sınıfı.

use std::cell::OnceCell;

use bt_core::DEFAULT_BG;
use bt_gpu::{Renderer, Surface};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSApplicationDelegate, NSBackingStoreType, NSView, NSWindow, NSWindowDelegate,
    NSWindowStyleMask,
};
use objc2_foundation::{
    NSArray, NSNotification, NSObject, NSObjectNSDelayedPerforming, NSObjectProtocol, NSPoint,
    NSRect, NSRunLoopCommonModes, NSSize, ns_string,
};

use crate::Options;

/// Delegate'in durumu. `OnceCell`: pencere `applicationDidFinishLaunching`
/// içinde bir kez doğar, sonra yalnız okunur; view `contentView()` ile türetilir.
pub(crate) struct Ivars {
    renderer: Renderer,
    surface: Surface,
    window: OnceCell<Retained<NSWindow>>,
    run_seconds: Option<u64>,
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
            let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(900.0, 600.0));
            let style = NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable;
            // SAFETY: defer=false ile pencere hemen yaratılır. Kurucunun unsafe
            // olma sebebi `releasedWhenClosed`: pencere kontrolcüsü olmadan
            // AppKit kapanışta pencereyi serbest bırakır ve `Ivars.window`'daki
            // Retained sarkar; hemen altında kapatıyoruz.
            let window = unsafe {
                NSWindow::initWithContentRect_styleMask_backing_defer(
                    NSWindow::alloc(mtm),
                    rect,
                    style,
                    NSBackingStoreType::Buffered,
                    false,
                )
            };
            // SAFETY: yalnız sahiplik semantiğini değiştirir; Retained sahibi biziz.
            unsafe { window.setReleasedWhenClosed(false) };
            let view = NSView::initWithFrame(NSView::alloc(mtm), rect);
            // Sıra önemli: önce layer, sonra wantsLayer — tersi AppKit'e kendi
            // layer'ını kurdurur ve CAMetalLayer düşer.
            view.setLayer(Some(self.ivars().surface.ca_layer()));
            view.setWantsLayer(true);
            window.setContentView(Some(&view));
            window.setTitle(ns_string!("bateri"));
            // Delegate bağlanmadan önce ivar dolu olsun: arada düşen bir
            // pencere bildirimi `sync_size`'ı boş bulup bayat boyutla çizmesin.
            // OnceCell doluysa didFinishLaunching ikinci kez geldi demek; AppKit
            // bunu yapmaz, yapsaydı ilk pencere kalırdı.
            let _ = self.ivars().window.set(window.clone());
            window.setDelegate(Some(ProtocolObject::from_ref(self)));
            window.center();
            window.makeKeyAndOrderFront(None);
            NSApplication::sharedApplication(mtm).activate();
            self.sync_size();
            self.draw();
            if let Some(s) = self.ivars().run_seconds {
                // block2 yok: zamanlayıcı performSelector ile.
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
                        s as f64,
                        &NSArray::from_slice(&[NSRunLoopCommonModes]),
                    );
                }
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window(&self, _app: &NSApplication) -> bool {
            true
        }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) {
            // 002'de burası yalnız kirli işaretler; kareyi display link sürer.
            self.sync_size();
            self.draw();
        }

        // Ekranlar arası taşımada boyut (nokta) değişmez ama ölçek değişir;
        // layer-hosting view'da bunu bizden başka kimse yazmaz.
        #[unsafe(method(windowDidChangeBackingProperties:))]
        fn window_did_change_backing(&self, _n: &NSNotification) {
            self.sync_size();
            self.draw();
        }
    }

    impl AppDelegate {
        #[unsafe(method(runDeadline:))]
        fn run_deadline(&self, _arg: Option<&AnyObject>) {
            let n = self.ivars().renderer.frames();
            if n > 0 {
                println!("kare={n} pipeline=ok");
                std::process::exit(0);
            }
            eprintln!(
                "bateri: {} saniyede hiç kare çizilmedi",
                self.ivars().run_seconds.unwrap_or(0)
            );
            std::process::exit(1);
        }
    }
);

impl AppDelegate {
    pub(crate) fn new(mtm: MainThreadMarker, renderer: Renderer, opts: Options) -> Retained<Self> {
        let surface = renderer.surface();
        let this = Self::alloc(mtm).set_ivars(Ivars {
            renderer,
            surface,
            window: OnceCell::new(),
            run_seconds: opts.run_seconds,
        });
        // SAFETY: NSObject'in init'i argümansızdır ve ivar'lar set edildi.
        unsafe { msg_send![super(this), init] }
    }

    /// Layer'ın drawable boyutunu view'ın backing geometrisiyle eşle. Ölçek tek
    /// kaynaktan okunur ve piksel boyutu ondan çarpılır; `drawableSize` ile
    /// `contentsScale` ayrışırsa bulanıklık olur.
    fn sync_size(&self) {
        let Some(window) = self.ivars().window.get() else {
            return;
        };
        let Some(view) = window.contentView() else {
            return;
        };
        let scale = window.backingScaleFactor();
        let bounds = view.bounds().size;
        self.ivars()
            .surface
            .set_size(bounds.width * scale, bounds.height * scale, scale);
    }

    /// Bir kare çiz. Hata stderr'e; iskelette kare düşer, süreç düşmez
    /// (sayaç artmaz, duman kırmızı çıkar).
    fn draw(&self) {
        if let Err(e) = self
            .ivars()
            .renderer
            .draw_surface(&self.ivars().surface, DEFAULT_BG)
        {
            eprintln!("bateri: kare çizilemedi: {e}");
        }
    }
}
