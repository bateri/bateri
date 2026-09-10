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

**`Waker::wake(self: &Arc<Self>)` taslağı derlenmiyor.** `&Arc<Self>` stabil
Rust'ta geçerli bir alıcı değil (`arbitrary_self_types` nightly). `Waker`
bunun yerine `Clone` bir değer oldu ve `Arc`'ı **içeride** taşıyor
(`Arc<MainThreadBound<Retained<CAMetalDisplayLink>>>`). Kazanç yalnız derleme
değil: `Wake` uygulayıcısı artık `Arc<Waker>` yerine `Waker` tutuyor, tip
imzası "bu ucuz kopyalanır" diyor.

**Kareyi yeniden isteme tek mekanizmada toplandı.** Taslakta `setPaused(false)`
ile `mark_dirty()` ayrı çağrılardı; checklist'in (c) maddesi de tam olarak
ikisinin ayrı düşmesinden doğan hatayı tarif ediyordu. `DisplayLink::request_frame()`
artık **ikisini birden** yapıyor ve tek bir cümleyle tanımlı: "çizilmiş olan
artık geçerli değil". Üç çağıranı da (açılış, resize, görünürlük) aynı yol
besliyor, `resize` de onu çağırıyor. Ayrı bir "yalnız link'i aç" yolu bilerek
**yok**: bayrağı dikmeden açılan link "hasar yok" deyip anında geri uyur ve
sessizce hiçbir şey yapmaz.

**`GpuError::CommandFailed` silinmedi, yer değiştirdi.** Checklist onu
`NoDrawable` ile birlikte siliyordu; taslakta asenkron hata yalnız
`eprintln!` olacaktı. Ama phase-2 kalite kapısının (b) devri "handler
`mark_dirty()` **ve** `Waker::wake()` çağırmalı" diyor — yani hatayı **çağıran**
öğrenmek zorunda. `Renderer::draw` bu yüzden bir `on_complete: impl
Fn(Result<(), GpuError>) + Send + 'static` alıyor: renderer sonucu yorumlamaz,
loglamaz; yeniden deneme ve durma koşulu kareyi isteyenin (`link.rs`) işi.
`CommandFailed` o sonucun taşıyıcısı olarak kaldı, `NoDrawable` `draw_surface`
ile birlikte gerçekten silindi.

**Görünürlük bildirimleri eklendi, göz kontrolünün yerine geçmedi.**
Checklist "`app.rs` bunları dinlemiyor, göz kontrolü yap" diyordu.
`windowDidDeminiaturize:` ve `windowDidChangeOcclusionState:` artık dinleniyor
(ikincisi yalnız **görünür olmaya** geçişte kare istiyor; örtülmeye giderken
kare çizmek kimsenin görmeyeceği bir kare olurdu). Göz kontrolünün kendisi
ajanın elinden gelmiyor — pencereyle etkileşemez — ve `[~]` işaretlendi.

**`DisplayLink` `Drop`'ta `invalidate()` çağırıyor** (planda yoktu). Run loop
link'i kendi tutuyor: `invalidate` olmadan düşen bir `DisplayLink`'in
callback'i tazeleme hızında atmaya devam eder ve delegate zayıf olduğu için
sessizce hiçbir şey çizmez. Belirtisi olmayan, pil giden döngünün ta kendisi.

**`Session`'ın tek sahibi şimdilik link.** `bt-shell` `Arc<Session>`'ı ivar'da
tutmuyor; `DisplayLink`'in delegate'i tutuyor ve resize onun üzerinden
geçiyor. Klavye (`write`) ve kapanış (`shutdown`) phase-4'te doğrudan
erişim isteyecek, ivar o zaman ekleniyor — bugün eklenseydi kullanılmayan
alan olurdu.

**`ShellWake::child_exit` yalnız kare istiyor.** Uygulamayı sonlandırmak R6,
yani phase-4. Bugünkü davranış shell'in son çıktısının ekrana gelmesini
sağlıyor; süreç ayakta kalıyor.

**Sahiplik çemberi kapanmıyor, çünkü `delegate` zayıf.** Zincir
`AppDelegate → DisplayLink → LinkDelegate → Arc<Session> → Arc<dyn Wake> →
Waker → CAMetalDisplayLink`. Son halka link'e **güçlü** dönüyor ama link
delegate'i zayıf tutuyor (objc2 `setDelegate` belgesi bunu yazıyor), yani
`wake.rs`'in uyardığı sızıntı oluşmuyor.

**`Frame` crate dışına hiç çıkmıyor.** Checklist yalnız kurucuların
`pub(crate)` olmasını istiyordu; `Frame`'in kendisi ve `Renderer::draw` de
`pub(crate)` oldu ve `lib.rs`'in `pub use`'undan düştü. Kare listesini
dolduran tek yer `link.rs`; dışarıya açık bırakmak "yanlış hücre boyutuyla
doldur" kapısını açık tutmaktı.

**Hücre metriği `bt-shell`'de bir sabit.** `CELL_PX = (9.0, 18.0)` @1x,
ölçekle çarpılıp yuvarlanıyor; `SCROLLBACK = 10_000`. İkisi de yer tutucu
(font metriği 003, ayar dosyası 00X) ve `Metrics` yapısı pencere
geometrisinden grid ölçüsüne çeviriyi tek yerde topluyor.

**`Cargo.lock` yalnız bir satır oynadı.** `block2` grafta zaten vardı
(`objc2-metal`'in varsayılan `block2` feature'ı çekiyor); doğrudan bağımlılık
yapmak yeni crate ya da sürüm getirmedi, `bt-gpu`'nun bağımlılık listesine bir
ad ekledi.

**`CLAUDE.md`'nin iskelet paragrafı da düzeldi.** "`bt-core` ve `bt-atlas`
boştur" cümlesi phase-1'den beri kodla çelişiyordu (R7'nin yedinci cümlesi,
phase-1 ve phase-2'de gözden kaçmış). Bölüm 6'nın tablosunda yoktu, aynı
commit'te düzeltildi: sözleşmenin kodla çelişen cümlesi bırakılmaz.

### `/simplify` (dört mercek) — uygulananlar

Üç mercek bağımsız olarak **aynı köke** işaret etti: hata politikası iki yerde
yazılmıştı. İrtifa merceği orada teorik olmayan bir kayıp gösterdi — senkron
hata kolunun kendi icat ettiği durak (`setPaused(true)`) bayrağa bakmıyordu,
yani okuyucunun o sırada diktiği bir hasarın üstüne link'i uyutup kareyi
yutabiliyordu. Politika tek yere indi (`Retry`), senkron kolun ayrı durağı
silindi; durma koşulu artık genel yol: "hasar yok → uyu".

Uygulanan diğerleri:

- **`Waker` dispatch'leri birleştiriyor.** alacritty `Wakeup`'ı her ≤64 KiB'de
  ve her okuma turunda yolluyor; sürekli çıktıda her biri bir kapanış
  kutulaması + ana thread uyandırması demekti. Kareler birleşiyordu ama
  dispatch'ler birleşmiyordu — `AtomicBool` ile uçuşta tek iş kalıyor.
  Kayıpsızlığın gerekçesi `wake()`'in doküman cümlesinde.
- **Tamamlanma bloğu kare başına değil, kurulumda bir kez ayrılıyor**
  (`Renderer::completion` → `Completion`). Blokta kareden kareye değişen
  hiçbir şey yoktu; her kare bir heap ayırması ve dört `Arc` sayaç hareketi
  ödüyordu.
- **`hucre=K` sayacı `Renderer`'a taşındı.** İki tanı sayacı (`frames`,
  `last_bg_count`) aynı evde ve tek yerden okunuyor; `DisplayLink`'in pub
  yüzeyinden bir metot, `LinkIvars`'tan bir alan düştü.
- **Duman betiğinin tek sahibi `bt-core::smoke_shell()`.** Aynı betik
  `app.rs`'te ve `bt-core` sınamasında ayrı ayrı yazılıydı; duman K'yı yalnız
  "> 0" diye sorduğu için biri değişse fark edilmezdi. Artık
  `sabit_shell_arka_plan_hucreleri_verir` **uygulamanın koştuğu** betiği
  sınıyor, yani `hucre=8` bir belge cümlesi değil sınanmış bir iddia.
- **`windowDidDeminiaturize:` silindi.** Simge durumu da `occlusionState`'i
  düşürüyor, yani genel sinyalin altkümesiydi. Genel sinyalin üstüne özel
  durum dizmek listenin hiç kapanmaması demek (tam ekran, Space, `unhide`...).
- **Ölü fallback ve `CELL_PX`'in ikinci kopyası gitti** (`expect` +
  `// audit:` gerekçesi); `sync_size` → `geometriyi_esitle` (hem yazıyor hem
  döndürüyor, ad ikisini de söylesin); drawable yorumunun fazla iddiası
  düzeltildi (`CAMetalDisplayLink` drawable'ı callback'ten önce alıyor,
  çağırmamak alımı iptal etmiyor).

**Uygulanmayanlar, gerekçeleriyle:**

- *`Frame`'in `cell_px`'i `clear`'ın parametresi olmaktan çıkıp alan olsun.*
  Kopya sayısı aynı kalıyor (bugün `LinkIvars`, o hâlde `Frame`), kazanç yok;
  buna karşılık phase-2'nin gerekçeli ve **sınanmış** kararı ("boyut vermeden
  temizleyemezsin") geri alınırdı.
- *`DisplayLink` `Session`'ı kendi spawn etsin, `OnceLock<Waker>` düşsün.*
  Düğüm kaybolmuyor, yer değiştiriyor: `Session` link'ten önce `Wake` ister,
  link `Session`'dan önce doğamaz. İki ajanın önerdiği biçim `OnceLock`'u
  `bt-shell`'den `bt-gpu`'ya taşırdı — üstelik PTY oturumunun ömrünü GPU
  crate'ine verirdi ki `CLAUDE.md`'nin katman cümlesi tam tersini söylüyor.
- *`draw` `()` dönsün, senkron hata da kapanıştan geçsin (tek kanal).* Politika
  zaten tek yerde; `Result` çağırana "komut yola çıktı mı" sorusunun senkron
  cevabını veriyor ve tek satırlık çağrı yerini karmaşıklaştırmıyor.
- *Kalıcı `MTLRenderPassDescriptor`.* Kare başına bir ObjC nesnesi kazandırırdı
  ama `Renderer`'ı paylaşılan değişken ObjC durumu tutar hâle getirip
  `Sync`'liğini bozardı.
- *`setVertexBytes` ile ≤4 KiB'lik kareyi tamponsuz geçirmek* ve *pipeline
  derlemesini açılışta asenkronlaştırmak*: ikisi de ölçüm işi, Muhakeme
  bunları `/measure`'a bağlamıştı — Yayın Etkisi'nde "ölçüm bekliyor" olarak
  duruyor.
- *`run_seconds`'ın shell'i sabitlemesi ayrı bir `BT_COMMAND` anahtarına
  bölünsün.* Doğru bir gözlem (bir anahtar bir iş) ama `plan.md` R4 ikisini
  bilerek bağlıyor; kapsam değişikliği phase'in içinde yapılmaz.

### `/code-review` — giderilenler

İki bulgu gerçek hataydı ve ikisi de sessizdi:

- **`child_exit`'in istediği kare hiç çizilmiyordu.** `Waker::wake` yalnız
  `setPaused(false)` yapıyordu; `Event::ChildExit` hasar bayrağı dikmiyor
  (onu yalnız `Wakeup` diker). Yani callback "hasar yok" bulup anında geri
  uyuyordu: bir dispatch, bir callback, alınıp atılan bir drawable ve sıfır
  piksel — üstelik yorumu "son çıktı ekrana gelsin" diyordu. Kök neden
  "kare iste"nin iki yarısının (bayrak + link) ayrı ayrı çağrılabilmesiydi;
  `Waker::wake` artık ikisini birden yapıyor ve "kare iste"nin tek tanımı o.
  `Retry` ile `request_frame` de aynı tanımı çağırıyor.
- **Görünmeyen pencereye çizim sürüyordu.** Occlusion kancası link'i yalnız
  *açıyordu*, çizim yolunda hiçbir görünürlük kapısı yoktu: örtülü ya da
  simge durumundaki pencerede konuşkan bir shell (`tail -f`) her tazelemede
  tam bir kare çizdirirdi — `CLAUDE.md`'nin pil sözleşmesinin tam ihlali,
  belirtisi de yok. `DisplayLink::set_visible` geldi: görünmezken link
  uyutuluyor ve callback erken dönüyor, bayrak **tüketilmiyor** (görünürlük
  dönünce biriken hasar olduğu gibi çiziliyor).

Diğer giderilenler:

- `resize`, hücre piksel boyutunu artık **yalnız oturum kabul ederse**
  uyguluyor. `Session::resize` bu yüzden `bool` döndürüyor: dejenere boyut
  yoksayılırken hücre ölçüsünü yine de yazmak, grid'i eski ölçüde bırakıp
  çizimi yeni ölçüye kaydırırdı (simge durumundaki pencerede olur) ve PTY'nin
  bildiği `TIOCSWINSZ` ile ayrışırdı.
- `Waker` `Session`'ı **`Weak`** ile tutuyor. `wake.rs`'in kendi kuralı bu
  ("`Session`'a bakmak gerekiyorsa `Weak` ile bakılır") ve ikinci bir tehlikeyi
  daha kapatıyor: tamamlanma bloğu Metal'in thread'inde serbest bırakılırken
  elinde `Arc<Session>` kalmıyor, yani orada `Drop` → `shutdown()` → `join()`
  zinciri çalışamıyor.
- **Durma koşulu artık sınanıyor.** Politika `Ardisik` tipine çıktı (ObjC'siz,
  kilitsiz) ve `durma_kosulu_art_arda_ikinci_hatada_devreye_girer` onu
  çalıştırıyor; koşulu bozup sınamanın düştüğü doğrulandı. `frames()`'in yeni
  anlamı için de `tamamlanma_blogu_kareyi_sayar_ve_sonucu_iletir` eklendi —
  adı bilerek dar: `Error` durumunu isteyerek üretmenin güvenilir yolu yok,
  o dal koşmuyor.
- **Jetonlar yalnız başarı satırında.** Hata satırı `kare=`/`hucre=` yazıyordu;
  `kare=` arayan bir CI adımı düşen koşudan kare sayısı okurdu. Sayılar
  duruyor, jeton biçimi kalktı.
- `waker.set(...)`'in sessizce yutulan `Err`'i `assert!` oldu: ikinci kez
  kurulsa eski link'in `Waker`'ı kalır ve pencere shell çıktısına bir daha hiç
  uyanmazdı — izsiz.
- `smoke_shell`'in uykusu 30 → 10 sn: `run_deadline` `process::exit` ile
  çıkıyor, `Drop` koşmuyor, `SIGHUP` gitmiyor; artakalan çocuk uyku bitene
  kadar yaşıyordu.

**Giderilmeyenler, gerekçeleriyle:**

- *`update.drawable()` `Option` değil, nil gelirse panik.* Başlıkta `nonnull`
  ve objc2 onu `Option`suz üretiyor; `nextDrawable`'ın `Option`'ı bu yüzden
  kalktı. Yapılabilecek tek şey yorumla işaretlemekti, yapıldı.
- *Tamamlanma bloğu Metal'in thread'inde düşerken `Waker`'ı da düşürebilir;
  `MainThreadBound::drop` ana kuyruğa **senkron** iş atar.* `Weak` değişikliği
  `Session` yarısını kapattı; kalan yarı ancak uçuşta kare varken
  `DisplayLink` düşerse ortaya çıkar, o da phase-4'ün kapanış yolunda —
  phase-4 checklist'ine yazıldı.
- *`hucre=K` süreç geneli `Renderer`'da; sekmelerle "en son commit eden yüzey"
  anlamına gelir.* Doğru, ama `frames` de aynı ölçüde global ve sekme bu sette
  kapsam dışı; `make duman` sekmeler gelince zaten yeniden düşünülecek. İki
  ajan bu noktada ters yönde önerdi, karar tek yerde tutmaktan yana ve geri
  alması tek alan.
- *Duman artık zamanlamaya bağlı (senkron ilk kare yok).* Asenkron commit
  phase'in kendisi (R3); `BT_RUN_SECONDS=3` bol geliyor, 0/1 gibi bir değerin
  "kısa deadline"ı "bozuk pipeline"dan ayırmaması bilinen sınır.

### `/audit` (10 mercek) — giderilenler

Mekanik mercekler temiz: katman yönü ve `bt-core`'un platformsuzluğu
(`cargo tree` + kaynak grep'i, ikisi de boş), panik yolu (`bt-core`'a giren tek
`unwrap` `#[cfg(test)]` içinde), ölçüm sahipliği (hiçbir belgeye sayı
girmedi), düzen sözleşmesi (`.metal`, `Instance` ve buffer indeksleri bu
phase'de hiç değişmedi). Yeni bağımlılık `block2` `discussion.md → Karar`'da
onaylı ve `Cargo.lock`'ta yalnız `bt-gpu`'nun bağımlılık listesi oynadı.
İlgisiz: ayar/tema şeması (4) ve shell entegrasyon üçlüsü (5) — dosyalarına
dokunulmadı.

Yargı mercekleri üç şey buldu:

- **Mercek 8 — çizim durmuştu, ritim durmamıştı.** Görünürlük kapısı yalnız
  çizim tarafındaydı: `Waker::wake` bayrağı görmüyor, koşulsuz
  `setPaused(false)` ediyordu. Örtülü pencerede konuşkan bir shell link'i
  tazeleme hızında kaldırıp yatırırdı — kare çizilmez ama her vsync'te bir ana
  thread callback'i ve `CAMetalDisplayLink`'in callback'ten **önce** aldığı bir
  drawable ödenir. Sözleşmenin harfi kalır, ruhu giderdi. Bayrak `WakerInner`'a
  taşındı ve kapı iki tarafta da duruyor; hasar yine her zaman dikiliyor, yalnız
  link açılmıyor.
- **Mercek 7 — `wake()` `wake.rs`'in kendi kuralını çiğniyordu.** `Weak<Session>`
  yetmiyor: `upgrade()` referansı çağrı süresince **maddileştiriyor** ve o
  geçici `Arc` okuyucu ya da Metal thread'inde son referans olabilir → `Drop` →
  `shutdown()` → `join()` orada koşar, thread kendi kendini bekler (`EDEADLK`)
  ve PTY yolunda panik olur. `bt-core` artık oturumdan bağımsız bir tutamak
  veriyor (`Session::dirty_flag() -> DirtyFlag`); `Waker` `Session`'a hiçbir
  biçimde referans tutmuyor ve kural tip düzeyinde tutuluyor.
- **Mercek 10 — koda göre yanlış yorum.** `resize`'ın doküman cümlesi "ekran
  ölçeği değişiminde `Session::resize` erken döner" diyordu; oysa `ayni_boyut`
  hücre piksel boyutunu da karşılaştırıyor, yani ölçek değişimi tam olarak
  erken dönmeyen hâl (aynı diff'in sınaması bunu iddia ediyor). Cümle
  gerçekten erken dönen hâllerle değiştirildi. Ayrıca `renderer.rs`'in modül
  başlığı hâlâ "sayacı yalnız sunulan kare artırır" diyordu — anlam bu phase'de
  `addCompletedHandler`'a taşındı; `CompletionBlock` takma adının gerekçesi
  kodda karşılığı olmayan bir iddia taşıyordu; `set_visible`'ın parametresi
  dışa bakan imzada Türkçeydi (`gorunur` → `visible`).

Küçükler: bozuk cümle düzeltildi, sarılmamış yorum satırları toparlandı, iki
`impl Waker` bloğu birleşti, `frame.borrow_mut()`'a `// audit:` gerekçesi
eklendi (dosyadaki diğer panik noktaları işaretliydi).

**Giderilmeyen:** mercek 7'nin ikinci bulgusu —
`MainThreadBound<Retained<CAMetalDisplayLink>>`'in `Drop`'u ana thread dışında
ana kuyruğa **senkron** iş atıp bekler ve o gövdeyi Metal'in tamamlanma bloğu
da tutuyor. Bugün ulaşılamaz (`process::exit` kapanışı atlıyor); kapanış
sırasının kendisiyle çözülür ve phase-4 checklist'ine yazıldı. Yorumu da
düzeltildi: eski hâli okuyucuyu tersine ikna ediyordu.

## Yayın Etkisi

- **yeni bağımlılık:** `block2` (`bt-gpu`, kullanıcı onaylı: `discussion.md` →
  Karar); `objc2-quartz-core` feature'ı `CAMetalDisplayLink`. `Cargo.lock`'ta
  yeni crate ya da sürüm hareketi **yok** — `block2` grafta zaten vardı
  (`objc2-metal`'in varsayılan feature'ı), oynayan tek satır `bt-gpu`'nun
  bağımlılık listesi.
- **duman sözleşmesi değişti:** `kare=N pipeline=ok` → `kare=N hucre=K
  pipeline=ok`, ikisi de > 0 olmalı. 001'in doğrulaması bu commit'te yeni
  sözleşmeye geçti — geri alma tek commit. Jeton **eklendi, silinmedi**.
- **shader:** `.metal` değişmedi; `make shader` yine de kanarya olarak koştu
  (yeşil). Rust ↔ MSL düzen sözleşmesine dokunulmadı.
- Belgeler (bölüm 6) + `CLAUDE.md` iskelet paragrafı, `Makefile`'ın `duman`
  yorumu.
- **ölçüm bekliyor:** (1) `Session::frame()`'in `FairMutex::lock()` beklemesi
  display link callback'inde — okuyucunun 64 KiB ayrıştırma lease'i arkasında
  bekliyor; (2) kare başına instance tamponu ayırmanın maliyeti ve
  `setVertexBytes` eşiği (≤4 KiB), Muhakeme'nin `/measure`'a bağladığı üçlü
  tamponlama kararıyla birlikte. Sayı **uydurulmadı**; `/measure` koşunca
  `docs/OLCUMLER.md`'ye girer.

---

## Checklist

- [x] `DisplayLink`, `LinkDelegate`, `Waker` (`bt-gpu`); `dispatch2` `bt-shell`'e girmedi
- [x] `draw` asenkron, sayaç `addCompletedHandler`'da; `draw_surface` ve `NoDrawable` silindi — `CommandFailed` **kaldı**, asenkron sonucun taşıyıcısı oldu (gerekçe: Uygulama Notları)
- [x] `bt-shell`: `Session` bağlandı, `ShellWake`, resize → `session.resize` + link aç
- [x] `session.mark_dirty()` üç yerde gerekiyor (002 phase-2 kalite kapısı devri): **(a)** senkron çizim hatasında — bayrak kimseyi uyandırmaz, link bir kare daha açık tutulur; **(b)** asenkron tamamlanma handler'ında komut tamponu `Error` dönerse — `mark_dirty()` **ve** `Waker::wake()` birlikte, yoksa durmuş link açılmaz (bölüm 2 taslağının yalnız `eprintln!` yapan hata dalı da düzeltilir); **(c)** piksel boyutu değişip grid boyutu değişmediğinde — `sync_size` `drawableSize`'ı koşulsuz yazıyor ama `session.resize` aynı boyutta erken dönüyor, kare hiç gelmez ve layer eski drawable'ı gerdirir. **Durma koşulu zorunlu:** hata başına tek yeniden deneme, art arda ikinci hatada `setPaused(true)` — (a) ve (b) `AtomicU32` sayacıyla, (c) `request_frame`'in bayrağı koşulsuz dikmesiyle çözüldü
- [x] **Göz kontrolü kullanıcı tarafından koşuldu ve geçti** (002 kapanışında): pencereyi simge durumuna indirip geri al ve başka pencerenin arkasına al — geri dönüşte kare geliyor mu? `app.rs` görünürlük bildirimi (`windowDidDeminiaturize:`, `windowDidChangeOcclusionState:`) dinlemiyor; compositor layer içeriğini düşürürse hiçbir yol bayrağı dikmez (002 phase-2 mercek 8 devri). Kod tarafı kapatıldı — iki bildirim de artık dinleniyor ve kare istiyor; kontrol artık keşif değil doğrulama
- [~] **Ölçüm, kapı değil** (`proje.md`: ölçümü kullanıcı `/measure` ile ister; Yayın Etkisi'ne "ölçüm bekliyor" olarak düştü). `frame()` kilidi: `FairMutex::lock()` okuyucunun 64 KiB ayrıştırma lease'i arkasında bekliyor (002 phase-1 `/code-review` devri). Display link callback'inde ölç; gerekirse `try_lock_unfair` + bayrağı geri dikip kareyi atla. **Tampon stratejisini de ölç:** `setVertexBytes` ≤4 KiB'lik kareyi (eşik 128 instance; tipik kare 9) tampon ayırmadan geçirebiliyor — 002 phase-2 `/simplify` devri, Muhakeme'nin `/measure`'a bağladığı üçlü tamponlama kararıyla birlikte bakılacak. **İkinci yön de ölç:** `frame()` veri kilidini tarama+sink boyunca tutarken okuyucu thread `try_lock_unfair` → `read()` → `EAGAIN` sıkı döngüsüne giriyor (alacritty `event_loop.rs:140`), yani pil sözleşmesine iki yönden de dokunuyor
- [x] `Frame`'in kurucuları (`clear`, `push_bg`, `push_cursor`, `bg_count`) `pub` → `pub(crate)`: sahiplik `link.rs`'e geçtiğinde crate dışı tüketici kalmıyor (002 phase-2 devri)
- [x] `proje.md`'nin `duman` satırındaki phase-2'ye ait geçici "bugünkü kapsam" paragrafı kalkar, yerine yeni sözleşme gelir (002 phase-2 devri) — `Frame`'in kendisi ve `Renderer::draw` de `pub(crate)` oldu
- [x] Test: `make duman` → `kare=N hucre=8 pipeline=ok` (N ≥ 1), çıkış 0
- [x] Test: `BT_RUN_SECONDS=10` ile `kare=N`: N küçük kalır (boşta link duruyor; N > 5 ise bulgu)
- [x] **phase-4'te koşuldu ve geçti** (maddenin kendi gerekçesi: yazmadan renkli çıktı üretilemiyor, klavye phase-4). Test: `cargo run -q -p bateri` gerçek `$SHELL` ile prompt arka planı görünmez ama `ls --color` benzeri renkli çıktı hücre arka planı verir (göz) — `printf '\e[44m  \e[0m'` yazılamaz (klavye phase-4), `BT_STARTUP`? yok: göz kontrolü phase-4'e devredilir
- [x] Belgeler (bölüm 6) aynı commit'te + `CLAUDE.md` iskelet paragrafı (R7'nin gözden kaçmış yedinci cümlesi)
- [x] Doğrulama geçti (`make hepsi`; koşullu: `make shader`, `make duman`, `make test-yaris`)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (dört mercek; uygulananlar ve gerekçeli redler Uygulama Notları'nda)
- [x] `/code-review` çalıştırıldı, bulgular giderildi (13 bulgu; ikisi gerçek hata, giderilmeyenler gerekçeli)
- [x] `/audit` çalıştırıldı, bulgular giderildi (mercek 7: callback ana thread, `Session` kilidi kısa, handler thread'i; mercek 8: boşta N küçük)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: `98baf87`
