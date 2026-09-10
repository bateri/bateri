# Phase 3 — bt-shell: pencere ve ilk kare

## Özet

AppKit penceresi, `CAMetalLayer`'ın view'a takılması, ilk kare, boyut
değişiminde yeniden çizim, `BT_RUN_SECONDS` ile `make duman`; WindowServer
ilk kez burada gerekir.

_Requirements: R6, R7_

---

## 1. Bağımlılıklar

`[workspace.dependencies]`'e `objc2-app-kit = "0.3"`. `crates/bt-shell/Cargo.toml`:

```toml
[dependencies]
bt-gpu = { workspace = true }
objc2 = { workspace = true }
objc2-foundation = { workspace = true }
objc2-app-kit = { workspace = true }
# Yalnız CALayer tipi için: NSView::setLayer(Option<&CALayer>) ister.
# Katman tablosu (CLAUDE.md) bunu "yalnız CALayer takma" diye kayda aldı.
objc2-quartz-core = { workspace = true, default-features = false, features = ["std", "CALayer"] }
```

`objc2-metal` **yok**: device'ı `bt_gpu::Renderer::system_default()` kurar.
`objc2-app-kit`'in `NSView::setLayer`'ı kendi `objc2-quartz-core` feature'ını
ister; varsayılanda açık, uygulamada `cargo tree -e features` ile doğrula.

## 2. Kabuk

`crates/bt-shell/src/lib.rs`

```rust
//! bt-shell — AppKit kabuğu: pencere, sekme, bölme, menü, klavye, servisler.
//! Bu phase'de tek pencere ve tek kare; sekme/menü/klavye sonraki setlerde.

pub struct Options {
    /// `BT_RUN_SECONDS`: dolunca kare sayısına bakıp çıkılır (`make duman`).
    pub run_seconds: Option<u64>,
}

/// Uygulamayı kurar ve `NSApplication::run` ile ana döngüye girer; dönmez
/// (son pencere kapanınca AppKit süreci bitirir).
pub fn run(opts: Options) -> Result<(), bt_gpu::GpuError> {
    let mtm = MainThreadMarker::new().expect("bt-shell::run ana thread'de çağrılır"); // audit: giriş noktası
    let renderer = bt_gpu::Renderer::system_default()?;     // Err → main stderr + exit 1
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let delegate = AppDelegate::new(mtm, renderer, opts);
    app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    app.run();
    Ok(())
}
```

`crates/bt-shell/src/app.rs` — **tek** `define_class!`:

```rust
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct AppDelegate {
        renderer: Renderer,
        surface: Surface,
        window: OnceCell<Retained<NSWindow>>,
        view: OnceCell<Retained<NSView>>,
        run_seconds: Option<u64>,
    }

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn did_finish_launching(&self, _n: &NSNotification) {
            let mtm = self.mtm();
            let rect = NSRect::new(NSPoint::new(0., 0.), NSSize::new(900., 600.));
            let style = NSWindowStyleMask::Titled | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable | NSWindowStyleMask::Resizable;
            let window = unsafe { NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm), rect, style, NSBackingStoreType::Buffered, false) };
            let view = NSView::initWithFrame(NSView::alloc(mtm), rect);
            // Sıra önemli: önce layer, sonra wantsLayer — tersi AppKit'e kendi
            // layer'ını kurdurur ve CAMetalLayer düşer (plan-review, Codebase-fit).
            view.setLayer(Some(self.ivars().surface.ca_layer()));
            view.setWantsLayer(true);
            window.setContentView(Some(&view));
            window.setTitle(ns_string!("bateri"));
            window.setDelegate(Some(ProtocolObject::from_ref(self)));
            window.center();
            window.makeKeyAndOrderFront(None);
            NSApplication::sharedApplication(mtm).activate();   // macOS 14+: activateIgnoringOtherApps yerine
            self.ivars().view.set(view).ok();
            self.ivars().window.set(window).ok();
            self.resize_and_draw();
            if let Some(s) = self.ivars().run_seconds {
                // block2 yok: performSelector ile zamanlayıcı.
                unsafe { self.performSelector_withObject_afterDelay(sel!(runDeadline:), None, s as f64) };
            }
        }

        #[unsafe(method(applicationShouldTerminateAfterLastWindowClosed:))]
        fn should_terminate_after_last_window(&self, _app: &NSApplication) -> bool { true }
    }

    unsafe impl NSWindowDelegate for AppDelegate {
        #[unsafe(method(windowDidResize:))]
        fn window_did_resize(&self, _n: &NSNotification) { self.resize_and_draw(); }
    }

    impl AppDelegate {
        #[unsafe(method(runDeadline:))]
        fn run_deadline(&self, _: Option<&AnyObject>) {
            let n = self.ivars().renderer.frames();
            if n > 0 { println!("kare={n} pipeline=ok"); std::process::exit(0); }
            eprintln!("bateri: {} saniyede hiç kare çizilmedi", self.ivars().run_seconds.unwrap_or(0));
            std::process::exit(1);
        }
    }
);

impl AppDelegate {
    fn new(mtm: MainThreadMarker, renderer: Renderer, opts: Options) -> Retained<Self> {
        let surface = renderer.surface();
        let this = Self::alloc(mtm).set_ivars(Ivars::<Self> {
            renderer, surface, window: OnceCell::new(), view: OnceCell::new(),
            run_seconds: opts.run_seconds,
        });
        unsafe { msg_send![super(this), init] }
    }

    /// Backing boyutunu layer'a yaz ve bir kare çiz. Hata → stderr; iskelette
    /// kare düşer, süreç düşmez (kare sayacı artmaz, duman kırmızı çıkar).
    fn resize_and_draw(&self) {
        let (Some(view), Some(window)) = (self.ivars().view.get(), self.ivars().window.get()) else { return };
        let bounds = view.bounds();
        let px = view.convertSizeToBacking(bounds.size);
        self.ivars().surface.set_size(px.width, px.height, window.backingScaleFactor());
        if let Err(e) = self.ivars().renderer.draw_surface(&self.ivars().surface, [0.10, 0.11, 0.13, 1.0]) {
            eprintln!("bateri: kare çizilemedi: {e}");
        }
    }
}
```

Renk `[0.10, 0.11, 0.13]` geçici; tema modeli 00X. Menü yok: ⌘Q çalışmaz,
kırmızı düğme çalışır — menü çubuğu kapsam dışı, `## Kapsam Dışı`'na bağlı.

## 3. Giriş noktası

`crates/bateri/src/main.rs`

```rust
//! bateri — uygulama girişi.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    // Başsız ortam (SSH, CI): AppKit WindowServer'a bağlanamaz ve belirsiz
    // hata verir. Atlama ≠ geçme: 78 (EX_CONFIG) ile açıkça çık.
    if !aqua_oturumu() {
        eprintln!("ATLANDI: Aqua oturumu yok");
        return ExitCode::from(78);
    }
    let run_seconds = std::env::var("BT_RUN_SECONDS").ok().and_then(|s| s.parse().ok());
    match bt_shell::run(bt_shell::Options { run_seconds }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { eprintln!("bateri: {e}"); ExitCode::FAILURE }
    }
}

fn aqua_oturumu() -> bool {
    Command::new("launchctl").arg("managername").output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "Aqua")
        .unwrap_or(false)
}
```

## 4. Makefile

Phase-1'deki `duman` **stub'ını yerinde değiştir** (ikinci tanım ekleme):

```make
# Pencereyi açar, BT_RUN_SECONDS dolunca kare sayısına bakar: 0 → exit 1.
# Başsız ortamda binary "ATLANDI" basıp 78 ile çıkar; make bunu 2 olarak
# döndürür — ayırt edici sinyal stdout metnidir, çıkış kodu değil.
duman:
	$(CARGO) build -p bateri
	BT_RUN_SECONDS=3 ./target/debug/bateri
```

Aynı commit'te `proje.md` başındaki "henüz yok" listesinden `duman`'ı ve
tablodaki *henüz yok* notunu çıkar.

## 5. Belge

`CLAUDE.md` → "Depo **iskelet aşamasındadır**: ... ilk iş setinin işidir"
paragrafı → "İskelet 001 ile kuruldu; `bt-core` ve `bt-atlas` boştur, VT
motoru 002'de gelir." `proje.md` başındaki "> Depo iskelet aşamasındadır ...
hedef adları o setin sözleşmesidir" notu kaldırılır; ertelenen üç hedefin notu
kalır.

---

## Uygulama Notları

- **`define_class!` sözdizimi (objc2 0.6.4):** kılavuz depodaki güncel
  örneklerin sözdizimini (`struct X { ivar... }`, `Ivars::<Self>`) taşıyordu;
  0.6.4'te ivar'lar ayrı `pub(crate) struct Ivars` + `#[ivars = Ivars]` ile
  bildirilir, sınıf `struct AppDelegate;` olur, `DefinedClass` trait'i
  `ivars()` için import edilir. `Ivars` `pub(crate)` olmak zorunda (makro onu
  `DefinedClass::Ivars` olarak dışa verir).
- **Feature kırpma workspace düzeyinde:** üye mirası `default-features = false`'u
  ezemediği için `objc2-quartz-core` ve `objc2-app-kit` workspace tablosunda
  kırpıldı, üyeler yalnız feature ekler (`bt-gpu`: CALayer, CAMetalLayer,
  objc2-metal, objc2-core-foundation; `bt-shell`: CALayer). quartz-core 39 →
  31 birleşik feature, app-kit 305 → 17. app-kit kırpması dört alt crate'in
  (`objc2-cloud-kit`, `core-data`, `core-image`, `core-text`) lock'a girmesini
  engelledi; quartz-core kırpması üçünü (`objc2-core-graphics`, `core-video`,
  `io-surface`) düşürdü. Lock 16 crate.
- `NSObjectNSDelayedPerforming` trait'i `performSelector_withObject_afterDelay`
  için gerekir (`objc2-foundation`, `NSRunLoop` + `NSDate` feature'ları;
  `bt-shell` bunları açıkça sayar).
- `setReleasedWhenClosed(false)`: kılavuzda yoktu; pencere kontrolcüsü yok,
  kapanışta AppKit pencereyi serbest bırakırdı ve `Ivars.window` sarkardı.
  `windowDidChangeBackingProperties:` de eklendi (ekranlar arası ölçek).
- `run()` dönmez (kılavuz doğruydu, ilk yazım yanlıştı); `ATLANDI` stdout'a;
  `make duman` `cargo run` ile (`CARGO_TARGET_DIR`'a saygı, çıkış kodu geçer).
- `sync_size` ölçeği tek kaynaktan çarpar (`convertSizeToBacking` yerine);
  `Ivars.view` yok, `contentView()` türetir; `resize_and_draw` → `sync_size` +
  `draw` (002 resize'da yalnız kirli işaretleyecek).
- Deadline `NSRunLoopCommonModes`'ta (`/audit` mercek 7: canlı boyutlandırma
  varsayılan mod zamanlayıcısını erteler); `window.set` `setDelegate`'ten önce
  (mercek 8: arada düşen bildirim bayat boyutla çizmesin).
- 002'ye şart (`/audit` mercek 7): duman çıkışı da `stop:`/`terminate:`
  üzerinden `applicationWillTerminate:`'a uğramalı; PTY çocuğu gelince
  `process::exit` `Drop`'ları atlar.
- Reddedilen sadeleştirme önerileri: çıkış politikasını `run()`'a taşımak
  (`NSApplication::terminate` kod taşımaz, `stop:` + sahte olay gerekir; 002),
  Aqua kontrolünü `bt-shell`'e `CGSessionCopyCurrentDictionary` ile taşımak
  (yeni bağımlılık `objc2-core-graphics`, ayrı karar), ikinci `BateriView`
  sınıfı (R6 tek sınıf; 002'nin klavye/display link işiyle gelir), ilk kareyi
  `makeKeyAndOrderFront`'tan önce çizmek (doğrulanmamış kazanç).
- **Görsel kontrol otomatik yapılamadı:** `activate()` macOS 14'te işbirlikçi;
  pencere öndeki terminalin arkasında açılıyor, `screencapture` onu yakalamadı.
  Menü çubuğu olmadığı için ⌘Q da çalışmıyor (kapsam dışı, bundle/menü seti).
  Renk, boyutlandırma ve kapatma kullanıcı gözü bekliyor.
- Doğrulama: `make duman` → `kare=1 pipeline=ok`, çıkış 0; eksik fonksiyon
  deneyi → kırmızı (`MissingFunction`); sahte `launchctl` → "ATLANDI", 78;
  bozuk `BT_RUN_SECONDS` → stderr + 1;
  `cargo tree -p bt-shell --depth 1` `objc2-metal` içermiyor; `make hepsi` 0.
- sadakat: makas yok.

## Yayın Etkisi

- **app bundle**: yok — çıplak binary; `Info.plist` ve `make kur` bundle setinde.
- **yeni bağımlılık**: `objc2-app-kit` (`discussion.md` → Karar K1).
- `CLAUDE.md` ve `proje.md` "iskelet aşaması" paragrafları bu commit'te.
- Ölçüm bekleyen iddia: yok. (Kare süresi ölçümü `BT_FRAME_LOG` kancasıyla
  gelir; bu sette kanca yok, `/measure` "ölçüm aracı yok" der.)

---

## Checklist

- [x] `bt-shell`: `run`, `Options`, `AppDelegate` (tek `define_class!`, iki protokol + `runDeadline:`)
- [x] `setLayer` → `setWantsLayer(true)` sırası
- [x] `bateri` main: Aqua kontrolü, `BT_RUN_SECONDS`, hata → stderr + 1
- [x] Test: `make duman` → stdout `kare=1 pipeline=ok`, çıkış 0
- [~] Test: pencere koyu gri, siyah değil (göz) — ekran görüntüsü pencereyi yakalamadı, kullanıcı gözü bekliyor
- [x] Test: `quad_fragment` adını `quad.metal`'de geçici olarak değiştir → `make duman` kırmızı; geri al
- [~] Test: pencereyi boyutlandır → bulanıklık yok; kapat → süreç 0 ile biter — etkileşimli, kullanıcı gözü bekliyor
- [x] Test: `launchctl managername` ≠ Aqua ortamı taklidi (`PATH`'e sahte `launchctl` koyarak) → çıkış 78
- [x] `make duman` stub'ı gerçek reçeteyle değiştirildi; `proje.md` listesinden çıkarıldı
- [x] Belge paragrafları (bölüm 5) aynı commit'te
- [x] Doğrulama geçti (`make hepsi`; koşullu: `make duman`)
- [~] `make test-yaris` — nightly yok ve paylaşılan durum yok (tek thread); hedef "henüz yok" deyip kırmızı düşer, koşu `[~]`
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (4 mercek; 6 uygulandı, 4 gerekçeyle reddedildi)
- [x] `/code-review` çalıştırıldı, 8 bulgu giderildi
- [x] `/audit` çalıştırıldı — mercek 1, 2, 6, 8 temiz; 7 (run loop modu), 10 (dil kuralı, notlar) giderildi; 3, 4, 5, 9 ilgisiz (mercek 1: `cargo tree -p bt-shell --depth 1` `objc2-metal` içermez — geçişli olarak `bt-gpu` üzerinden gelir, doğrudan bağımlılık olmamalı; mercek 8: `waitUntilCompleted` sonrası talep yok, boşta sıfır kare)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
