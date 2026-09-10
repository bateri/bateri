# Phase 1 — bt-core: Session, Wake, frame

## Özet

`alacritty_terminal` `bt-core`'a kapsüllü girer: `Session` PTY'yi ve okuyucu
thread'i kurar, `Wake` dış dünyayı uyandırır, `frame()` tek kilitte hasarı
okur; pencere ve Metal yok, sınama sabit `/bin/sh` ile.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4, R1.5, R1.6, R7_

---

## 1. Bağımlılık

`Cargo.toml` (workspace) `[workspace.dependencies]`:

```toml
# Kullanıcı onaylı (002 discussion.md → Karar). serde kapalı: ağaç ~22 crate.
alacritty_terminal = { version = "0.26", default-features = false }
```

`crates/bt-core/Cargo.toml`: `alacritty_terminal = { workspace = true }`.
`libc`/`rustix` girer; `CLAUDE.md` `bt-core` platform sütunu buna göre yazılır
(bölüm 7). `cargo tree -p bt-core -e normal | grep -E "objc2|core-text|metal"`
boş kalmalı.

## 2. Dış tipler

`crates/bt-core/src/lib.rs`

```rust
//! bt-core — terminal modelinin platformsuz çekirdeği.
//! (başlık: mevcut metin + "alacritty_terminal burada kapsüllüdür; pub API'de
//! alacritty tipi görünmez, kendi grid'e geçiş bu sınırın arkasında yapılır")

mod session;
mod wake;

pub use session::{CellBg, Cursor, Session, SessionOptions};
pub use wake::Wake;

/// alacritty `Cell`: c 4 + fg 4 + bg 4 + flags 2 + dolgu + Option<Arc<CellExtra>> 8.
/// Seyrek veri zaten yan tabloda (`CellExtra`). Sabit değişirse bu satır ve
/// `CLAUDE.md` birlikte değişir.
const _: () = assert!(size_of::<alacritty_terminal::term::cell::Cell>() == 24);
```

`crates/bt-core/src/wake.rs`

```rust
/// Okuyucu thread'den dış dünyaya sinyal. Çağrılar okuyucu thread'de gelir ve
/// `Term` kilidi TUTULURKEN gelebilir: uygulayan asla `Session`'a geri girmez,
/// asla bloklamaz; yalnız bir kuyruğa iş atar.
pub trait Wake: Send + Sync + 'static {
    /// Grid değişti; bir kare gerekebilir.
    fn wake(&self);
    /// Shell çocuğu bitti.
    fn child_exit(&self, code: Option<i32>);
}
```

`crates/bt-core/src/session.rs`

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellBg { pub col: u16, pub row: u16, pub rgba: [f32; 4] }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor { pub col: u16, pub row: u16, pub visible: bool }

pub struct SessionOptions {
    /// `None` → `$SHELL` login; `Some((program, args))` → o komut (duman).
    pub command: Option<(String, Vec<String>)>,
    pub cols: u16,
    pub rows: u16,
    pub cell_px: (u16, u16),
    pub scrollback: usize,
}
```

## 3. Session

```rust
pub struct Session {
    term: Arc<FairMutex<Term<Adapter>>>,
    sender: EventLoopSender,
    reader: Option<JoinHandle<(EventLoop<Pty, Adapter>, State)>>,
}

struct Adapter { wake: Arc<dyn Wake>, sender: EventLoopSender }

impl EventListener for Adapter {
    fn send_event(&self, ev: Event) {
        match ev {
            Event::Wakeup => self.wake.wake(),
            Event::ChildExit(status) => self.wake.child_exit(status.code()),
            // Term kilidi tutulurken gelir; kanala yazmak kilitsizdir.
            Event::PtyWrite(s) => { let _ = self.sender.send(Msg::Input(s.into_bytes().into())); }
            Event::ColorRequest(i, fmt) => { let _ = self.sender.send(Msg::Input(fmt(palette(i)).into_bytes().into())); }
            Event::TextAreaSizeRequest(fmt) => { /* aynı biçimde, WindowSize'dan */ }
            Event::Title(_) | Event::ResetTitle | Event::Bell | Event::ClipboardStore(..)
            | Event::ClipboardLoad(..) | Event::MouseCursorDirty | Event::CursorBlinkingChange
            | Event::Exit => {}
        }
    }
}
```

`Session::spawn(opts, wake) -> io::Result<Session>`:

1. `tty::Options { shell: opts.command.map(|(p,a)| Shell::new(p,a)), working_directory: None,
   drain_on_exit: false, env: [("TERM","xterm-256color"),("COLORTERM","truecolor")] }`.
   **`tty::setup_env()` çağrılmaz** (bizim sürecin env'ini `unsafe set_var` ile
   değiştirir ve makinede alacritty kuruluysa `TERM=alacritty` yazar).
2. `WindowSize { num_cols, num_lines, cell_width, cell_height }`; `tty::new(&opts, size, 0)`.
3. `Term::new(Config { scrolling_history, ..Default::default() }, &size, adapter.clone())`
   — `EventLoop::new` `U: EventListener + Clone` isteyebilir; uygulamada bak.
4. `EventLoop::new(term.clone(), adapter, pty, drain_on_exit, ref_test=false)`;
   `sender = loop.channel()` **spawn'dan önce** (spawn `self`'i tüketir);
   adapter'ın `sender`'ı bu — kanal `channel()` sonrası klonlanabilir, sırayı
   uygulamada çöz (adapter'a `OnceLock<EventLoopSender>`).
5. `reader = Some(loop.spawn())`.

`frame`:

```rust
/// Tek kilit tutuşu: hasar yoksa None ve sıfır iterasyon. Hasar "çizilsin mi"
/// der, "ne çizileceğine" değil — drawable içeriği korunmaz, her kare tam grid.
pub fn frame(&self, sink: &mut dyn FnMut(CellBg)) -> Option<Cursor> {
    let mut term = self.term.lock();            // adil kilit; lock_unfair okuyucunun
    if matches!(term.damage(), TermDamage::Partial(mut it) if it.next().is_none()) { return None; }
    let content = term.renderable_content();
    for cell in content.display_iter {          // Indexed<&Cell>
        let bg = resolve(cell.bg, cell.flags);  // INVERSE → fg; Named/Indexed → palet; Spec → rgb
        if bg != DEFAULT_BG { sink(CellBg { col, row, rgba: bg }); }
    }
    let cursor = Cursor { col, row, visible: content.cursor.shape != Hidden };
    term.reset_damage();
    Some(cursor)
}
```

`TermDamage::Full` → hasar var. Palet: 16 renk sabit (Metalterm "Graphite"
benzeri nötr; tema 00X), `DEFAULT_BG` = `[0.10, 0.11, 0.13, 1.0]` (001'in rengi;
`bt-shell`'deki `ARKA_PLAN` buradan okunur, iki yerde durmaz).

`write(&self, bytes)` → `Msg::Input(bytes.to_vec().into())`.
`resize(cols, rows, cell_px)` → `term.lock().resize(size)` + `Msg::Resize(size)`.

`shutdown`:

```rust
pub fn shutdown(&mut self) {
    let _ = self.sender.send(Msg::Shutdown);
    if let Some(h) = self.reader.take() {
        // join → (EventLoop, State) döner; drop'u Pty'yi düşürür: kill(SIGHUP) + wait.
        // Çocuk SIGHUP'ı yutarsa wait bloklar — bateri'deki bekçi thread bunu keser.
        let _ = h.join();
    }
}
impl Drop for Session { fn drop(&mut self) { self.shutdown(); } }
pub fn reader_alive(&self) -> bool { self.reader.as_ref().is_some_and(|h| !h.is_finished()) }
```

## 4. Sınamalar

`crates/bt-core/src/session.rs` → `#[cfg(test)]`

```rust
struct TestWake(Arc<(Mutex<u32>, Condvar)>);   // wake sayar, sınama bekler

#[test]
fn sabit_shell_arka_plan_hucreleri_verir() {
    let s = Session::spawn(SessionOptions { command: Some(("/bin/sh".into(),
        vec!["-c".into(), "printf '\\033[41m bateri \\033[0m\\n'; sleep 5".into()])),
        cols: 40, rows: 10, cell_px: (9, 18), scrollback: 100 }, wake.clone()).unwrap();
    wake.bekle(Duration::from_secs(3));           // en az bir Wakeup
    let mut cells = vec![]; let cur = s.frame(&mut |c| cells.push(c));
    assert!(cur.is_some());
    assert_eq!(cells.len(), 8);                   // " bateri " kırmızı
    assert!(cells.iter().all(|c| c.row == 0 && c.rgba[0] > 0.5));
    assert!(s.frame(&mut |_| ()).is_none());       // hasar sıfırlandı
}

#[test]
fn shutdown_okuyucuyu_bitirir() { /* spawn, shutdown, reader_alive() == false */ }

#[test] #[ignore = "make test-yaris ile koşar"]
fn yaris_wake_ve_frame() { /* 4 thread write + 1 thread frame, 2 s; panik yok */ }
```

## 5. Makefile

```make
# ThreadSanitizer nightly ister ve bu makinede yok; yerine belirlenimci stres:
# #[ignore] işaretli yaris_* sınamaları + tek thread karşılaştırma koşusu.
test-yaris:
	$(CARGO) test --workspace -- --ignored yaris_
	$(CARGO) test --workspace -- --test-threads=1
```

`proje.md` doğrulama tablosundaki `test-yaris` satırı ve başındaki "henüz yok"
listesi buna göre düzeltilir (nightly notu "TSan nightly gelince eklenir"e iner).

## 6. `TERM` ve env

`Options.env` yalnız `TERM`, `COLORTERM`. alacritty `ALACRITTY_WINDOW_ID` ve
`WINDOWID`'yi koşulsuz yazar (`tty/unix.rs`), kapatılamaz — `## Uygulama
Notları`'na düş, `CLAUDE.md` tuzaklarına tek satır.

## 7. Belgeler (R7, bu commit)

| dosya | değişiklik |
|---|---|
| `CLAUDE.md` bağımlılık maddesi | `portable-pty` düşer, `alacritty_terminal` (`serde` kapalı, kapsüllü) girer |
| `CLAUDE.md` hücre maddesi | "hedefimiz 16, 002 ölçer" → "sabit 24 (alacritty `Cell`), `const` assert `bt-core`'da; seyrek veri `CellExtra`'da; kendi hücre `frame()` sınırının arkasında" |
| `CLAUDE.md` katman tablosu `bt-core` | sorumluluk: "PTY ve okuyucu thread (alacritty), hasar, `Wake`"; platform: "macOS'a özgü hiçbiri (`objc2*`, `core-text`, `metal`); Unix PTY (`libc`/`rustix`) serbest — kapı Linux hedefiyle derlemedir"; OSC 133 notu: "alacritty'de yok; komut blokları `frame()` sınırına kanca ister (00X)" |
| `CLAUDE.md` tuzaklar | `ALACRITTY_WINDOW_ID`/`WINDOWID` env sızıntısı; `setup_env()` yasağı |
| `proje.md` | `test-yaris` satırı ve liste |

---

## Uygulama Notları

**`Term::damage()` kapı olamıyor — kapı `Event::Wakeup`'ın diktiği bayrak.**
Plan `frame()`'in hasarı sorup boşsa `None` dönmesini istiyordu. alacritty
`damage()` her çağrıda imleci koşulsuz kirletiyor (`Term::damage` →
`damage_cursor`, kendi sınamalarının notu da bunu söylüyor), yani "hasar yok"
cevabı **hiçbir zaman gelmiyor**: link hiç durmaz, boşta 120 Hz kare çıkardı.
Kapı bu yüzden `AdapterInner::dirty` (`AtomicBool`) oldu; bayrağı
`Event::Wakeup` diker — alacritty'nin "yeni içerik var" sinyali odur — ve
`frame()` `Term` kilidi altında tüketir. `Term::damage()`/`reset_damage()`
hiç çağrılmıyor: iki ayrı "kirli" kavramı yan yana durursa biri sessizce
eskir.

Bu, Muhakeme'nin reddettiği "`AtomicBool` birleştirme" **değildir**. Reddedilen
mekanizma *uyandırmayı atlamak* içindi (bayrak setse `exec_async` çağırma) ve
kayıp uyanma riski taşıyordu. Buradaki bayrak uyandırmayı hiç atlamaz; sıra
`store(true)` → `wake()` olduğu için bayrağı kaçıran bir uyanma yok:
`frame()` bayrağı `false`'a çekip kilidi bırakırken gelen içerik bayrağı
yeniden diker ve arkasından `wake()` gelir. Açılış karesi için bayrak `true`
başlar (pencere ilk kez boyansın diye); `resize()` de diker, çünkü boyut
değişimi `Wakeup` üretmez.

**Sapmalar ve karşılaştıkları sebep:**

- `Session::shutdown(&self)` — plan `&mut self` yazıyordu. Phase-3 `Arc<Session>`
  paylaşıyor, phase-4 `applicationWillTerminate:`'ten çağırıyor; ikisi de
  `&mut` veremez. Tutamak `Mutex<Option<Reader>>`'da, `Drop` aynı yolu
  çağırıyor. (Phase-4 bu seçimi zaten uygulamaya bırakmıştı.)
- `EventLoop::new` `U: Clone` **istemiyor**, ama `Term::new` ve `EventLoop::new`
  ayrı ayrı birer `U` alıyor. Adapter bu yüzden `Adapter(Arc<AdapterInner>)`:
  iki kopya tek gövdeyi paylaşır, `channel()` sonrası tek `OnceLock::set`
  ikisini birden bağlar.
- `Dimensions` yalnız `Grid` için (ve `#[cfg(test)]` `(usize, usize)` için)
  uygulanmış; `TermSize` `term::test` altında bir sınama yardımcısı. Ürün
  yolunda kendi `GridSize`'ımız duruyor, üç satır.
- `ColorRequest` `Term` kilidi tutulurken geliyor; renk tablosunu okumak için
  kilidi geri istemek kilitlenme olurdu. Yanıt varsayılan paletten veriliyor —
  uygulamanın OSC 4 ile değiştirdiği renk bu yanıtta eski kalır (kimse
  değiştirmiyor; tema seti 00X'in işi).
- `tracing` **eklenmedi** (yeni bağımlılık mimari karardır ve bu sette onaylı
  değil); iz `eprintln!` ile, 001'in `bt-gpu`'da kurduğu örüntü.
- `clippy::type_complexity`: `Mutex<Option<JoinHandle<(EventLoop<Pty, Adapter>,
  State)>>>` → `type Reader`.
- `DEFAULT_FG` ve `DEFAULT_CURSOR` de dışa veriliyor. İmleç rengi phase-3'te
  `bt-gpu`'da sabit duracaktı; renk kararı paletin, çizim kararı renderer'ın —
  ikisi de `bt-core::color`'dan okunur.
- Palet `0xRRGGBB` yazılıyor: `Rgb { r, g, b }` üçlüsü `cargo fmt` altında
  16 satırlık tabloyu 80 satıra çıkarıyordu.

**Bulunan ve düzeltilen hata:** ilk `dim()` kanalları `u8` üstünde `* 2 / 3`
hesaplıyordu; `0xd1 * 2` bir `u8`'e sığmaz ve `Named(DimRed)` arka planlı bir
hücre debug derlemesinde **taşma paniği** verirdi — üstelik tam da "PTY
yolunda panik yok" kuralının yasakladığı yerde. Nihai hâlde kendi `dim`'imiz
hiç yok: vte'nin `impl Mul<f32> for Rgb`'si (`vte/src/ansi.rs`, yorumu birebir
"the default dim is just *2/3") `f32`'de çarpıp `clamp(0.0, 255.0)` uyguluyor,
yani taşma sınıfı ortadan kalkıyor. Kendi yazdığımız için kendi bulduğumuz bir
hataydı.

**Yarış sınaması:** ilk hâli 4 thread'den kısıtsız `write` yapıyordu; kanal
sınırsız olduğu için sınama yarışı değil bellek tüketimini ölçüyor ve
asılıyordu. Yazanlar 1 ms uykuyla, çocuk `sleep 0.01`'lık döngüyle
kısıtlandı; 2 saniyede geçiyor.

**`/simplify` bulguları (dört mercek, paralel).** Uygulananlar:

- **`ARKA_PLAN` iki sahipliydi ve değerleri bile farklıydı** (`bt-shell`
  `[0.10, 0.11, 0.13]`, `bt-core` `0x1a1c21` = `[0.1020, 0.1098, 0.1294]`);
  `color.rs`'in "tek sahibi burasıdır" yorumu yalan söylüyordu. Phase-3'ün
  `bt-shell → bt-core` kenarı öne alındı (aşağı yönlü, katman düzenine uygun),
  `ARKA_PLAN` silindi, `CLAUDE.md` zincir çizimine o kenar eklendi. Phase-3'ün
  bağımlılık adımı bu commit'te kapandı.
- **`dim`/`ton` yerine vte'nin `Mul<f32> for Rgb`'si** (yukarıda).
- **Kirlilik bayrağı artık kilitten ÖNCE tüketiliyor.** `FairMutex::lock()` iki
  muteks alıyor ve okuyucu thread PTY'den okumaya başlamadan önce aynı sıraya
  giriyor; boştaki kare o sıraya hiç girmemeli. Swap ile lock arasına düşen bir
  `Wakeup` bayrağı yeniden diker — en kötüsü fazladan bir kare, kaçan kare değil.
- **Satır sınırı dalı ölüydü** (`display_iter` aralığı zaten kırpıyor,
  `grid/mod.rs:422`): hücre başına dal yerine `debug_assert!`.
- **`make test-yaris`'in ikinci satırı `make test`'in kopyasıydı** ve
  `--ignored` taşımadığı için yarış sınamasını hiç koşmuyordu.
  `--include-ignored --test-threads=1` oldu: ikinci satır artık gerçek bir
  ikinci zamanlama profili. `proje.md` satırı da düzeltildi.
- `pub const DEFAULT_FG` tüketicisiz dışa açmaydı, düştü (`INVERSE` kolu
  `resolve(cell.fg)` çağırıyor, sabite uğramıyor); glyph 003'te geri gelir.
- `TestWake`'e `bekle_cikis`; `bekle(u32::MAX, …)`'ı uyku yerine kullanan
  yoklama döngüsü ve `.max(hedef) + 1` bilmecesi kalktı.
- `GridSize`'ın gerekçesi ve `total_lines` varsayımı yazıya döküldü.

Uygulanmayanlar ve nedenleri:

- **`frame()` kilidi `sink` boyunca da tutuyor** → tamponu `Session` içinde
  toplayıp kilidi erken bırakmak önerildi. Uygulanmadı: Muhakeme maddi
  `Snapshot`'ı açıkça reddetti, sınır "tek fonksiyon + sink" olarak seçildi ve
  sink'in yapacağı iş phase-2'de `Vec::push`. Ölçüm gelirse yeniden bakılır.
- **OSC 11 varsayılan arka planı değiştirirse** `resolve` her hücre için
  `BG_RGB`'den farklı bir renk döndürür ve tam grid çizilir. Bugünkü davranış
  **doğrudur, yalnız pahalıdır**; ucuz yolu (kare başına bir kez çözüp ona
  karşı karşılaştırmak) tek başına uygularsak pencerenin clear rengi eski
  kalır ve ekran yanlış renge boyanır. Doğru derinlikteki düzeltme
  `frame()`'in etkin arka planı dışarı vermesi, yani R1.2 imzasının
  değişmesi — tema seti 00X'in işi.
- **`write(impl Into<Cow<'static, [u8]>>)`** tuş başına bir kopyayı kaldırırdı;
  plan imzayı `&[u8]` olarak sabitledi ve kopyanın görünür olacağı yer
  phase-4'ün `kod_cevir`'i (o zaten `Cow<'static, [u8]>` döndürüyor).
  **Phase-4'e devredildi.**
- `sink` için `&mut dyn FnMut` yerine jenerik `impl FnMut`: R1.2 imzayı
  böyle yazıyor ve çağrı sayısı hücre değil "varsayılan olmayan hücre"
  kadar — dinamik çağrı sıcak nokta değil.
- `GridSize` yerine `alacritty_terminal::term::test::TermSize`: erişilebilir
  ve birebir aynı, ama adı `test` olan bir modülü ürün yolunda taşımıyoruz.

**`/code-review` bulguları.** İki gerçek hata — ikisi de sınamayla bağlandı
(koruma kaldırılınca sınamanın düştüğü doğrulandı):

- **Boş yazma PTY yazıcısını kalıcı kilitliyordu.** `write(b"")` →
  `Msg::Input(&[])` → `EventLoop::pty_write`'ta `write` `Ok(0)` döner, öge
  kuyruğun başına geri konur ve bir daha emilmez; poller seviye tetiklemeli
  olduğu için okuyucu thread %100'de döner ve **sonraki her tuş kuyrukta
  kalır**. alacritty kendi `Notifier`'ında tam bu korumayı taşıyor
  ("Terminal hangs if we send 0 bytes through"), bizim kapsül onu atlamıştı.
  `Session::write` ve `Adapter::reply` korundu; sınama
  `bos_yazma_pty_yazicisini_kilitlemez` (korumasız hâlde zaman aşımı).
- **Sıfır sütun/satır panik ettiriyordu.** `bt-shell` boyutu `bounds()`'tan
  hesaplıyor, simge durumuna inen pencere 0 verebilir; alacritty'de
  `last_column()` = `Column(columns() - 1)` ve `usize`'ta `0 - 1` taşar
  (ölçüldü: panik `grid/resize.rs:276`'da, tahmin edilenden de erken).
  `GridSize::new` tabanı 1×1'de kesiyor; sınama `sifir_boyut_panik_etmez`.

Ayrıca giderilenler:

- **Ters video + `DIM` tam güçte boyanıyordu.** `\e[2;7;31m` hücresinde ön
  plan arka plan olur ve `DIM` artık ona uygulanmalıdır; adlı rengi sönük
  eşine çeviren kod alacritty'nin **ikili** tarafında, kitaplıkta değil.
  `color::dim` eklendi, sınama `ters_videoda_sonuk_bayragi_arka_plani_koyultur`.
- **`resize` üç adımı ayrı ayrı yapıyordu** (grid, adapter boyutu, `Msg::Resize`);
  eşzamanlı iki resize grid'i bir sayıda, `TIOCSWINSZ`'i başkasında bırakabilirdi
  — `yaris_wake_ve_frame` tam bunu yapıyor. Üçü tek kilit tutuşuna alındı.
- **Yarış sınaması okuyucunun paniğini yutuyordu:** `shutdown()` `join()`
  hatasında yalnız stderr'e yazıyor, sınama yeşil kalıyordu — yani kapı, var
  olma sebebi olan hata sınıfında yeşil düşerdi. `reader_alive()` iddiası eklendi.
- **`row as u16`** sürüm derlemesinde negatif satırı 65535'e sarardı;
  `u16::try_from` ile korundu (`debug_assert` sözleşmeyi söylemeye devam ediyor).
- Sözleşme yorumları: `sink` `Term` kilidi altında çağrılır ve `Session`'a geri
  giremez; `shutdown()` `SIGHUP`'ı yutan çocukta **bloklar** (bekçi phase-4'te);
  `send()` shell kendi çıktıktan sonra sessizce başarılı olur, "gitti mi"nin
  cevabı `reader_alive()`; `Cursor::row` pencereye kırpılıdır.
- Yoksayılan olayların "loglanır" yarısı **borç olarak kayda geçti**: workspace'te
  hiçbir logger yok, alacritty'nin kendi `log::error!` satırları da yere düşüyor.
  `tracing` bağımlılık kararı istiyor.

Uygulanmayanlar:

- **`frame()` adil `FairMutex::lock()` kullanıyor**, okuyucunun 64 KiB'lik
  ayrıştırma lease'i arkasında bekleyebilir — "render yolu bloklanmaz"la
  gerçek bir gerilim. Muhakeme kilidi açıkça `lock()` olarak seçti; `try_lock`
  geri düşüşü bayrağı geri dikmeyi de gerektiriyor. **Phase-3'e devredildi**
  (display link callback'i orada doğuyor, ölçüm de orada anlamlı).
- `Session.sender` ile `adapter.0.sender` aynı kanalın iki tutamağı: `Session`'ınki
  `Option`suz, adapter'ınki `OnceLock` olmak zorunda (kanal adapter'dan sonra
  doğuyor). `send()`'i `OnceLock`'a bağlamak asla olmayacak bir `None` dalı ekler.
- `&mut dyn FnMut` yerine jenerik `sink`: `/simplify`'da da geçti, aynı gerekçe.

**`/audit` sonucu.** Koşan mercekler: 1 katman/platformsuzluk, 2 yeni
bağımlılık, 3 panik yolu, 6 ölçüm sahipliği (mekanik, inline); 7 thread ve
blokaj, 8 boşta sıfır kare, 9 hücre boyutu, 10 belge ve üslup (yargı, paralel
ajan). **İlgisiz:** 4 (ayar/tema şeması — `settings.rs` ve tema modeli henüz
yok), 5 (shell üçlüsü — `assets/shell/` el değmedi), 9'un shader yarısı
(`.metal` değişmedi).

Temiz çıkanlar: katman yönü ve `bt-core`'un platformsuzluğu (`cargo tree` +
kaynak grep, ikisi de boş); üretim yolunda korumasız panik kaynağı yok;
belgelere ölçüm sayısı ya da ölçülmemiş iddia girmedi; **kilit sırası tablosu
çıkarıldı, ters sıra yok** (`Mutex<WindowSize>` her zaman `Term`'ün altında ve
en içte; `shutdown` `join` boyunca `reader` kilidini tutmuyor); bayrağın
çivilenebileceği yol yok ve alacritty'de kendiliğinden `Wakeup` üreten kaynak
yok (üç `send_event(Wakeup)` çağrısının üçü de dış olaya bağlı ve sonlu);
`Cell` 24 elle ve derleyiciyle doğrulandı; `pub` API'nin tamamı İngilizce,
Türkçe adların hiçbiri dışa bakmıyor, diakritikler eksiksiz.

Giderilen bulgular:

- **`resize` aynı boyutta da bayrağı dikiyordu.** `windowDidChangeBackingProperties:`
  boyut değişmeden ateşlenebilir; koşulsuz bayrak "boşta sıfır kare"yi sessizce
  delerdi. Erken dönüş eklendi (`Term` kilidi altında, sıra bozulmadan).
- **`hasarsiz_frame_sink_cagirmaz` özelliği bağlamıyordu:** boş grid'de her
  hücre zaten `continue` ile düşüyor, yani kirli kapısı **tamamen silinse bile**
  sayaç 0 kalır ve sınama yeşil geçerdi. Sınama artık önce dolu bir kare
  tüketiyor, sonra ikincisinin hem `None` olduğunu hem sink'i çağırmadığını
  iddia ediyor.
- **`Wake` sahiplik sözleşmesi yazılmamıştı:** uygulayan `Arc<Session>` tutarsa
  çember kapanır, `Drop for Session` hiç koşmaz ve sekme başına bir PTY + bir
  thread sızar; `Weak` varyantında son referans okuyucu thread'in içinde
  düşerse `join` kendi kendini bekletir (`EDEADLK`) ve `Drop` içinde panik olur.
  `wake.rs`'e sahiplik paragrafı eklendi — phase-3/4 buna dayanacak.
- `resize`'ın yorumu "bayrağı dikeriz ki uyansınlar" diyordu; bayrak kimseyi
  uyandırmıyor, uyandırma çağıranın işi (phase-3'te `link.setPaused(false)`).
- `const` assert'in kapsamı dürüstçe yazıldı: `Cell` bizim tipimiz değil, bu
  bir **sürüm kanaryası**; kendi hücremiz gelince assert ona taşınır.
- `discussion.md` K5 instance tamponunu `16 bayt` diye bağlıyordu, `phase-2`
  ise `Instance`'ı 32 bayta çiviliyor — karar kaydına köşeli parantezli
  düzeltme düşüldü (tarihli kayıt yeniden yazılmadı).
- `session.rs` başlığı `lib.rs`'in kapsül sözleşmesini tekrar ediyordu (aynı
  cümlenin iki sahibi); kendi bildiğini söyleyecek şekilde daraltıldı.
- `#[rustfmt::skip]` gerekçesi yazıldı; `Makefile` yorumu "bu makinede rustup
  yok" yerine deponun kuralına bağlandı (araç zinciri pin'li değil).

Devredilen: `frame()` kilidinin **iki yönü** de phase-3'ün ölçüm maddesine
işlendi — hem `frame()`'in okuyucunun lease'i arkasında beklemesi, hem
okuyucunun `frame()`'in veri kilidi arkasında `try_lock` → `read()` → `EAGAIN`
sıkı döngüsüne girmesi (alacritty `event_loop.rs:140`).

**Ölçülen:** `Cargo.lock` +47 paket (çoğu Windows hedefi; macOS'ta derlenen
`bt-core` ağacı 27 satır). Sınamalar `/bin/sh` ile gerçek PTY açar ve
0,12 saniyede biter.

## Yayın Etkisi

- **yeni bağımlılık:** `alacritty_terminal` (Apache-2.0; attribution borcu
  bundle setine — `teslim.md`'ye `[elle]` not). `Cargo.lock` **+47 paket**
  (ölçüldü; çoğu Windows hedefi, macOS'ta derlenen `bt-core` ağacı 27 satır).
- `CLAUDE.md`, `proje.md` (bölüm 7).
- `make test-yaris` gerçek hedef oldu; "henüz yok" listesinden çıkar.
- Ölçüm bekleyen iddia: yok.

---

## Checklist

- [x] Bağımlılık; `cargo tree -p bt-core` `objc2`/`core-text`/`metal` içermez
- [x] `Wake`, `CellBg`, `Cursor`, `SessionOptions`, `Session` (`spawn`, `frame`, `write`, `resize`, `shutdown`, `reader_alive`, `Drop`)
- [x] Adapter: `PtyWrite`/`ColorRequest`/`TextAreaSizeRequest` kanala; `setup_env()` çağrılmıyor
- [x] `const` assert 24
- [x] Test: `sabit_shell_arka_plan_hucreleri_verir`, `shutdown_okuyucuyu_bitirir` geçer; `yaris_*` `--ignored` ile geçer
- [x] Test: `frame()` hasarsızken sink'i hiç çağırmıyor (sayaçla)
- [x] `make test-yaris` gerçek reçete; `proje.md` düzeltildi
- [x] Belgeler (bölüm 7) aynı commit'te
- [x] Doğrulama geçti (`make hepsi`; koşullu: `make test-yaris` — bu phase paylaşılan durum ekler)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi
- [x] `/audit` çalıştırıldı, bulgular giderildi (mercek 1 kapsül; 3 panik yolu — `unwrap` yalnız `// audit:` ile)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: ed1c5a3
