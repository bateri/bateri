# Phase 3 — bt-shell çizim: DisplayLink, Waker, asenkron kare

## Özet

`CAMetalDisplayLink` paused başlar, shell çıktısı geldiğinde `Waker` onu açar,
callback'te `Session::frame` doluysa çizilir yoksa link durur; commit asenkron,
sayaç tamamlanma handler'ında. Duman sözleşmesi `kare=N hucre=K pipeline=ok`
ve jeton listesi bu commit'te değişir; `draw_surface` yolu silinir.

_Requirements: R3, R4, R6 (resize kısmı), R7_

---

## 1. Bağımlılık ve feature

- `crates/bt-gpu/Cargo.toml`: `objc2-quartz-core` feature listesine
  `"CAMetalDisplayLink"`; `block2 = { workspace = true }` (workspace'e
  `block2 = "0.6"`; kullanıcı onaylı, `addCompletedHandler` için `RcBlock`).
- `crates/bt-shell/Cargo.toml`: `bt-core = { workspace = true }` (doğrudan kenar;
  `CLAUDE.md` zincir çizimine `bt-shell → bt-core` eklenir).

## 2. DisplayLink ve Waker (`bt-gpu`)

`crates/bt-gpu/src/link.rs`

```rust
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriLinkDelegate"]
    #[ivars = LinkIvars]
    struct LinkDelegate;
    unsafe impl NSObjectProtocol for LinkDelegate {}
    unsafe impl CAMetalDisplayLinkDelegate for LinkDelegate {
        #[unsafe(method(metalDisplayLink:needsUpdate:))]
        fn needs_update(&self, link: &CAMetalDisplayLink, update: &CAMetalDisplayLinkUpdate) {
            let iv = self.ivars();
            let mut frame = iv.frame.borrow_mut(); frame.clear();
            let cursor = iv.session.frame(&mut |c| frame.push_bg(c.col, c.row, c.rgba));
            match cursor {
                None => link.setPaused(true),                    // hasar yok → uyu
                Some(cur) => {
                    if cur.visible { frame.push_cursor(cur.col, cur.row, CURSOR_RGBA); }
                    iv.last_bg_count.set(frame.bg_count);
                    if let Err(e) = iv.renderer.draw(&update.drawable(), DEFAULT_BG, &frame) {
                        eprintln!("bateri: kare çizilemedi: {e}");
                    }
                }
            }
        }
    }
);

pub struct DisplayLink { link: Retained<CAMetalDisplayLink>, delegate: Retained<LinkDelegate> }
impl DisplayLink {
    /// Ana thread'de: run loop'a ekleme sözleşmeyi (callback ana thread'de) kurar.
    pub fn new(mtm, surface: &Surface, renderer: Arc<Renderer>, session: Arc<Session>) -> Self {
        let link = CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), surface.layer());
        // SAFETY: mainRunLoop + common modes; delegate zayıf property, sahibi bu yapı.
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        link.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        link.setPaused(true);
        …
    }
    pub fn waker(&self, mtm: MainThreadMarker) -> Arc<Waker> { … }
    pub fn last_bg_count(&self) -> usize { … }
}

/// Okuyucu thread'den ana thread'e tek iş: link'i aç. Bayrak yok — Wakeup
/// okuma başına en fazla bir kez gelir, setPaused idempotent.
pub struct Waker { link: dispatch2::MainThreadBound<Retained<CAMetalDisplayLink>> }
impl Waker {
    pub fn wake(self: &Arc<Self>) {
        let me = Arc::clone(self);
        DispatchQueue::main().exec_async(move || {
            // SAFETY: ana kuyruk = ana thread.
            let mtm = MainThreadMarker::new().expect("ana kuyruk");   // audit: dispatch main
            me.link.get(mtm).setPaused(false);
        });
    }
}
```

`CAMetalDisplayLink` `AnyThread` — `alloc()` için `use objc2::AnyThread`.
`Renderer` `Send + Sync` (001'de doğrulandı) → `Arc<Renderer>`; `Session`
`Send + Sync` olmalı (`Arc<FairMutex>` + `EventLoopSender`; uygulamada doğrula).

## 3. Renderer asenkron

`draw`: `waitUntilCompleted` ve senkron `status` kontrolü kalkar; yerine

```rust
let frames = Arc::clone(&self.frames);            // AtomicU64 artık Arc'ta
let handler = RcBlock::new(move |cmd: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
    // SAFETY: Metal handler'ı tamamlanmış tamponla çağırır.
    let cmd = unsafe { cmd.as_ref() };
    if cmd.status() != MTLCommandBufferStatus::Error { frames.fetch_add(1, Relaxed); }
    else { eprintln!("bateri: komut tamponu hatayla bitti: {:?}", cmd.error()); }
});
unsafe { cmd.addCompletedHandler(&handler) };
cmd.presentDrawable(drawable.as_ref()); cmd.commit();
```

Handler thread'i Metal'in; `frames` atomik, `eprintln` thread-safe. Sayaç
"GPU bitirdi ve hata yok" anlamını korur. `draw_surface`, `Surface::layer()`
(pub(crate) kalır, `DisplayLink` kullanır), `GpuError::NoDrawable` silinir;
`CommandFailed` handler'da yalnız loglanır — varyant da silinir.

## 4. bt-shell bağlama

`app.rs`: `Ivars` → `session: Arc<Session>`, `link: OnceCell<DisplayLink>`,
`renderer: Arc<Renderer>`. `did_finish_launching`: pencere (001) → `Session::spawn`
(duman modunda sabit komut, değilse `$SHELL`) → `DisplayLink::new` → `waker` →
`Session`'ın `Wake` impl'i `ShellWake(Arc<Waker>)`:

```rust
struct ShellWake { waker: Arc<Waker> }
impl Wake for ShellWake {
    fn wake(&self) { self.waker.wake(); }
    fn child_exit(&self, _code: Option<i32>) { self.waker.wake_exit(); }   // phase-4 terminate
}
```

`Wake`'in `Session::spawn`'dan önce var olması ama `Waker`'ın link'ten sonra
doğması: `ShellWake { waker: OnceLock<Arc<Waker>> }` — `wake()` link henüz
yoksa hiçbir şey yapmaz (ilk kare `setPaused(false)` ile açılışta zorlanır).

`windowDidResize`/`windowDidChangeBackingProperties`: `sync_size()` +
`session.resize(cols, rows, cell_px)` + `link.setPaused(false)` — çizim
çağrısı **yok**, kirli işaretleme link'i açar (resize hasarı alacritty'den).

Hücre piksel boyutu: `const CELL_PX: (f32, f32) = (9.0, 18.0)` @1x, ölçekle
çarpılır; `cols = floor(w / cw)`, `rows = floor(h / ch)`.

## 5. Duman sözleşmesi (R4)

`run_deadline`: `let n = renderer.frames(); let k = link.last_bg_count();`
→ `n > 0 && k > 0` ise `println!("kare={n} hucre={k} pipeline=ok")`, çıkış 0;
değilse stderr + çıkış 1 (kapanış phase-4'te `shutdown()`'a bağlanır; bu
phase'de `process::exit` kalır). Duman komutu:
`("/bin/sh", ["-c", "printf '\\033[41m bateri \\033[0m\\n'; sleep 30"])` → K = 8.

## 6. Belgeler (R7, bu commit)

| dosya | değişiklik |
|---|---|
| `CLAUDE.md` Komutlar | `make duman` satırı: "kare ve çizilen arka plan hücresi sayar; `kare=N hucre=K pipeline=ok`" |
| `CLAUDE.md` Dil maddesi | jeton listesine `hucre=` eklenir; "değişmez" → "silinmez, eklenir" |
| `CLAUDE.md` katman tablosu | `bt-gpu` sorumluluk: "display link ve `Waker`"; zincir çizimi `bt-shell → bt-core` |
| `proje.md` `duman` satırı | yeni sözleşme; K=0 kırmızı |

---

## Uygulama Notları

## Yayın Etkisi

- **yeni bağımlılık:** `block2` (`bt-gpu`); feature `CAMetalDisplayLink`.
- **duman sözleşmesi değişir** (jeton eklenir); 001'in doğrulaması bu commit'te
  yeni sözleşmeye geçer — geri alma tek commit.
- Belgeler (bölüm 6). Ölçüm bekleyen iddia: yok (kare süresi ölçümü `BT_FRAME_LOG`
  kancasıyla gelecek; bu sette yok).

---

## Checklist

- [ ] `DisplayLink`, `LinkDelegate`, `Waker` (`bt-gpu`); `dispatch2` `bt-shell`'e girmedi
- [ ] `draw` asenkron, sayaç `addCompletedHandler`'da; `draw_surface`/`NoDrawable`/`CommandFailed` silindi
- [ ] `bt-shell`: `Session` bağlandı, `ShellWake`, resize → `session.resize` + link aç
- [ ] `session.mark_dirty()` üç yerde gerekiyor (002 phase-2 kalite kapısı devri): **(a)** senkron çizim hatasında — bayrak kimseyi uyandırmaz, link bir kare daha açık tutulur; **(b)** asenkron tamamlanma handler'ında komut tamponu `Error` dönerse — `mark_dirty()` **ve** `Waker::wake()` birlikte, yoksa durmuş link açılmaz (bölüm 2 taslağının yalnız `eprintln!` yapan hata dalı da düzeltilir); **(c)** piksel boyutu değişip grid boyutu değişmediğinde — `sync_size` `drawableSize`'ı koşulsuz yazıyor ama `session.resize` aynı boyutta erken dönüyor, kare hiç gelmez ve layer eski drawable'ı gerdirir. **Durma koşulu zorunlu:** hata başına tek yeniden deneme, art arda ikinci hatada `setPaused(true)`
- [ ] Göz kontrolü: pencereyi simge durumuna indirip geri al ve başka pencerenin arkasına al — geri dönüşte kare geliyor mu? `app.rs` görünürlük bildirimi (`windowDidDeminiaturize:`, `windowDidChangeOcclusionState:`) dinlemiyor; compositor layer içeriğini düşürürse hiçbir yol bayrağı dikmez (002 phase-2 mercek 8 devri)
- [ ] `frame()` kilidi: `FairMutex::lock()` okuyucunun 64 KiB ayrıştırma lease'i arkasında bekliyor (002 phase-1 `/code-review` devri). Display link callback'inde ölç; gerekirse `try_lock_unfair` + bayrağı geri dikip kareyi atla. **Tampon stratejisini de ölç:** `setVertexBytes` ≤4 KiB'lik kareyi (eşik 128 instance; tipik kare 9) tampon ayırmadan geçirebiliyor — 002 phase-2 `/simplify` devri, Muhakeme'nin `/measure`'a bağladığı üçlü tamponlama kararıyla birlikte bakılacak. **İkinci yön de ölç:** `frame()` veri kilidini tarama+sink boyunca tutarken okuyucu thread `try_lock_unfair` → `read()` → `EAGAIN` sıkı döngüsüne giriyor (alacritty `event_loop.rs:140`), yani pil sözleşmesine iki yönden de dokunuyor
- [ ] `Frame`'in kurucuları (`clear`, `push_bg`, `push_cursor`, `bg_count`) `pub` → `pub(crate)`: sahiplik `link.rs`'e geçtiğinde crate dışı tüketici kalmıyor (002 phase-2 devri)
- [ ] `proje.md`'nin `duman` satırındaki phase-2'ye ait geçici "bugünkü kapsam" paragrafı kalkar, yerine yeni sözleşme gelir (002 phase-2 devri)
- [ ] Test: `make duman` → `kare=N hucre=8 pipeline=ok` (N ≥ 1), çıkış 0
- [ ] Test: `BT_RUN_SECONDS=10` ile `kare=N`: N küçük kalır (boşta link duruyor; N > 5 ise bulgu)
- [ ] Test: `cargo run -q -p bateri` gerçek `$SHELL` ile prompt arka planı görünmez ama `ls --color` benzeri renkli çıktı hücre arka planı verir (göz) — `printf '\e[44m  \e[0m'` yazılamaz (klavye phase-4), `BT_STARTUP`? yok: göz kontrolü phase-4'e devredilir
- [ ] Belgeler (bölüm 6) aynı commit'te
- [ ] Doğrulama geçti (`make hepsi`; koşullu: `make shader`, `make duman`, `make test-yaris`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi (mercek 7: callback ana thread, `Session` kilidi kısa, handler thread'i; mercek 8: boşta N küçük)
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
