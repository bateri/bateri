//! Bir terminal oturumu: PTY, okuyucu thread ve grid.
//!
//! Crate'in kapsül sözleşmesi `lib.rs`'te; burada onun iki pratik sonucu
//! yaşıyor: alacritty'nin `EventListener`'ı `Adapter`'da bizim `Wake`'imize
//! çevrilir, ve `Term` kilidi yalnız `frame()` ile `resize()`'da alınır —
//! üçüncü bir alan yeri yoktur, kilit sırası oradan okunur.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, RenderableContent, Term};
use alacritty_terminal::tty::{self, Pty, Shell};
use alacritty_terminal::vte::ansi::CursorShape;

use crate::color;
use crate::wake::Wake;

/// Varsayılan olmayan arka planıyla çizilecek tek hücre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CellBg {
    pub col: u16,
    pub row: u16,
    pub rgba: [f32; 4],
}

/// İmlecin karedeki yeri.
///
/// `row` her zaman görünür pencereye kırpılıdır. Kaydırma geçmişine bakarken
/// imleç ekranın dışına çıkar; o durumda `visible` kapanır ve `row` gerçek
/// satırı değil kırpılmış değeri taşır — konuma güvenen bir tüketici
/// (kaydırma, IME) çıkmadan önce buranın sözleşmesini genişletmeli.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

/// Oturumun açılış ayarları.
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// `None` → kullanıcının `$SHELL`'i login kabuk olarak; `Some((program,
    /// args))` → tam olarak o komut (duman ve sınamalar bunu kullanır ki
    /// sonuç kullanıcının rc dosyasına bağlı olmasın).
    pub command: Option<(String, Vec<String>)>,
    pub cols: u16,
    pub rows: u16,
    /// Bir hücrenin piksel boyutu; PTY'ye `TIOCSWINSZ` ile gider, grafik
    /// uygulamaları (sixel, kitty) bunu okur.
    pub cell_px: (u16, u16),
    pub scrollback: usize,
}

/// Hasarı işaretlemenin oturumdan bağımsız yolu; [`Session::dirty_flag`] verir.
///
/// `mark()` çağrılınca sıradaki `frame()` çizilecek bir kare döndürür.
/// **Kimseyi uyandırmaz:** uyandırmak çağıranın işi (kareyi isteyecek olan o).
///
/// **Durma koşulu çağıranındır ve zorunludur.** Kalıcı bir çizim hatası
/// "başarısız → bayrağı dik → yeniden dene" döngüsünü ekran tazeleme hızında
/// sonsuza çevirir; hata başına **tek** yeniden deneme, art arda ikinci hatada
/// kare talebi kesilir ve sıradaki `Wakeup` beklenir.
#[derive(Clone)]
pub struct DirtyFlag(Arc<AtomicBool>);

impl DirtyFlag {
    pub fn mark(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Duman koşusunun ve sınamaların ortak sabit shell'i.
///
/// Kullanıcının `$SHELL`'ine ve rc dosyasına bağlı olmayan bir komut: ilk
/// satıra **sekiz** kırmızı arka planlı hücre (`" bateri "`) basar, sonra uyur.
///
/// Uyku süresi duman koşusunun süresini (`BT_RUN_SECONDS`, varsayılan 3)
/// rahatça aşmalı: shell deadline'dan önce kendi kendine çıkarsa `ChildExit`
/// uygulamayı erken sonlandırır ve koşu ölçtüğü şeyi ölçmemiş olur. Üst sınır
/// artık yok — `run_deadline` çıkmadan önce `shutdown()` çağırıyor, yani
/// `SIGHUP` gidiyor ve artakalan çocuk uyku bitene kadar yaşamıyor.
///
/// Tek sahip olmasının sebebi sayının kendisi: `make duman`'ın `hucre=8`
/// beklentisi ile `sabit_shell_arka_plan_hucreleri_verir` sınamasının 8'i aynı
/// betiğe bağlı. İki yerde ayrı yazılsalardı biri değişip diğeri sessizce eski
/// kalırdı — ve duman K'yı yalnız "> 0" diye sorduğu için kimse fark etmezdi.
/// Bu hâliyle sınama, uygulamanın gerçekten koştuğu betiği doğruluyor.
pub fn smoke_shell() -> (String, Vec<String>) {
    (
        "/bin/sh".to_owned(),
        // Kaçışları printf çözer: Rust dizgisinde `\033` ilk baytı NUL yapardı.
        vec![
            "-c".to_owned(),
            "printf '\\033[41m bateri \\033[0m\\n'; sleep 10".to_owned(),
        ],
    )
}

/// `Term` boyutu `Dimensions` ister.
///
/// alacritty'nin `TermSize`'ı bu işi görürdü ve `#[cfg(test)]` ile kapalı da
/// değil — ama `term::test` modülünde, yani adı "sınama yardımcısı" diyor.
/// Ürün yolunda o adı taşımak yerine üç satırı kendimiz yazıyoruz.
///
/// `total_lines` sözleşme gereği "scrollback dahil toplam satır"dır, burada
/// `rows` dönüyor: alacritty dışarıdan geçirilen `Dimensions`'tan yalnız
/// `screen_lines` ve `columns` okuyor (scrollback'i `Config` veriyor).
/// Okumaya başlarsa scrollback sessizce sıfırlanır — o gün burası da değişir.
#[derive(Clone, Copy)]
struct GridSize {
    cols: usize,
    rows: usize,
}

impl GridSize {
    /// Sıfır sütun ya da satır alacritty'de taşmadır:
    /// `Dimensions::last_column()` = `Column(columns() - 1)`, `usize`'ta
    /// `0 - 1`. Açılışta grid'in var olması gerektiği için taban 1×1'de
    /// kesilir; henüz kaybedilecek geçmiş yok.
    ///
    /// **`resize` bu kurucuyu kullanmaz.** Var olan bir grid'i 1 sütuna
    /// çekmek yıkıcıdır: alacritty her sarmalı satırı tek sütuna açar ve
    /// `reversed.truncate(max_scroll_limit + lines)` ile geçmişin neredeyse
    /// tamamını atar; 80 sütuna dönmek onu geri getirmez. Dejenere boyut
    /// kırpılmaz, yoksayılır.
    fn spawn_tabani(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(1) as usize,
            rows: rows.max(1) as usize,
        }
    }

    /// Kırpmadan. Çağıran dejenere boyutu zaten elemiş olmalı;
    /// `spawn_tabani`'nin karşılığıdır ve `resize` bunu kullanır.
    fn tam(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols as usize,
            rows: rows as usize,
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// alacritty'nin `EventListener`'ı ile bizim `Wake`'imiz arasındaki adapter.
///
/// `Term` ve `EventLoop` ayrı ayrı birer kopya ister; ikisi de aynı `Arc`
/// gövdesini paylaşsın diye tip `Clone`'dur.
#[derive(Clone)]
struct Adapter(Arc<AdapterInner>);

struct AdapterInner {
    wake: Arc<dyn Wake>,
    /// `EventLoop`'un kanalı ancak `EventLoop::new`'dan sonra doğar, oysa
    /// adapter `Term::new`'dan önce gerekir; kanal bu yüzden sonradan takılır.
    sender: OnceLock<EventLoopSender>,
    /// Grid'de tüketilmemiş yeni içerik var mı.
    ///
    /// Kapı neden `Term::damage()` değil: alacritty her `damage()` çağrısında
    /// imleci koşulsuz kirletir (`damage_cursor`), yani "hasar yok" cevabı
    /// hiçbir zaman gelmez. Boşta sıfır kare ondan okunamaz. Bayrağı
    /// `Event::Wakeup` diker — alacritty'nin "yeni içerik var" sinyali odur.
    dirty: Arc<AtomicBool>,
    /// PTY'nin bildiği son boyut; `TextAreaSizeRequest` bunu yanıtlar.
    size: Mutex<WindowSize>,
}

impl Adapter {
    fn new(wake: Arc<dyn Wake>, size: WindowSize) -> Self {
        Self(Arc::new(AdapterInner {
            wake,
            sender: OnceLock::new(),
            // Açılış karesi: pencere ilk kez boyansın diye kirli başlar.
            dirty: Arc::new(AtomicBool::new(true)),
            size: Mutex::new(size),
        }))
    }

    /// Uygulamanın sorduğuna PTY'den yanıt verir. Kanala yazmak kilitsizdir;
    /// `Term` kilidi tutulurken çağrılmak serbesttir.
    fn reply(&self, text: String) {
        // Sıfır baytlık yazma `EventLoop`'un yazıcısını kilitler: `write`
        // `Ok(0)` döner, öge kuyruğun başına geri konur ve bir daha hiç
        // emilmez — poller seviye tetiklemeli olduğu için thread de %100'de
        // döner. alacritty kendi `Notifier`'ında aynı korumayı taşıyor.
        if text.is_empty() {
            return;
        }
        if let Some(sender) = self.0.sender.get() {
            // Kanal yalnız kapanışta ölür; o yolda sessiz kalmak doğrudur.
            let _ = sender.send(Msg::Input(text.into_bytes().into()));
        }
    }
}

impl EventListener for Adapter {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                // Sıra önemli: bayrak uyandırmadan ÖNCE dikilir. Ters sırada
                // uyanan taraf bayrağı henüz görmeden bakar ve kare kaçar.
                self.0.dirty.store(true, Ordering::Release);
                self.0.wake.wake();
            }
            Event::ChildExit(status) => self.0.wake.child_exit(status.code()),
            Event::PtyWrite(text) => self.reply(text),
            // Renk sorusu `Term` kilidi tutulurken gelir; tabloyu okumak için
            // kilidi geri istemek kilitlenme olurdu (kilit yeniden girilebilir
            // değil, `try_lock` da aynı thread'de hep düşer). Paletin
            // varsayılanıyla yanıtlıyoruz.
            //
            // **Bilinen sınır:** uygulama OSC 4/10/11 ile bir rengi
            // değiştirip sonra sorarsa eski değeri alır — "önce ata, sonra
            // sor" yaygın bir örüntüdür (arka planı okuyup açık/koyu tema
            // seçen editörler). Çizim yolu tabloyu doğru okuyor, yalnız
            // yanıt yolu okumuyor; ikisi ayrışıyor. Gerçek çözüm paletin
            // sahipliğinin alacritty'den bize geçmesi, yani 00X tema seti.
            Event::ColorRequest(index, format) => self.reply(format(color::default(index))),
            Event::TextAreaSizeRequest(format) => {
                let size = *kilit(&self.0.size);
                self.reply(format(size));
            }
            // Başlık, zil ve pano bu sette yok; bilinmeyen dizi gibi sessizce
            // düşerler (`CLAUDE.md` → PTY yolunda panik yok). "Yoksayılır ve
            // LOGLANIR" kuralının ikinci yarısı borç: `tracing` henüz
            // bağımlılık değil, workspace'te hiçbir logger yok — alacritty'nin
            // kendi `log::error!` satırları da bu yüzden yere düşüyor.
            Event::Title(_)
            | Event::ResetTitle
            | Event::Bell
            | Event::ClipboardStore(..)
            | Event::ClipboardLoad(..)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Exit => {}
        }
    }
}

/// Okuyucu thread'in tutamağı. `join()` döngüyü ve PTY'yi geri verir;
/// `SIGHUP` bu ikilinin düşmesiyle gider.
type Reader = JoinHandle<(EventLoop<Pty, Adapter>, State)>;

/// PTY'si, okuyucu thread'i ve grid'i olan bir terminal oturumu.
pub struct Session {
    term: Arc<FairMutex<Term<Adapter>>>,
    sender: EventLoopSender,
    adapter: Adapter,
    /// Okuyucu thread; `shutdown()` alır, `Drop` de çağırır. `Option`
    /// "kapandı" demenin ve iki kez join etmemenin yoludur.
    reader: Mutex<Option<Reader>>,
}

impl Session {
    /// PTY'yi açar, shell'i başlatır ve okuyucu thread'i kurar.
    pub fn spawn(options: SessionOptions, wake: Arc<dyn Wake>) -> io::Result<Self> {
        let grid = GridSize::spawn_tabani(options.cols, options.rows);
        let size = window_size(grid, options.cell_px);

        let pty_options = tty::Options {
            shell: options
                .command
                .map(|(program, args)| Shell::new(program, args)),
            // `tty::setup_env()` ÇAĞRILMAZ: o, kendi sürecimizin ortamını
            // `set_var` ile değiştirir ve makinede alacritty kuruluysa
            // `TERM=alacritty` yazar. Ortamı çocuğa doğrudan veriyoruz.
            env: HashMap::from([
                ("TERM".to_owned(), "xterm-256color".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ]),
            ..Default::default()
        };
        let pty = tty::new(&pty_options, size, 0)?;

        let adapter = Adapter::new(wake, size);
        let config = Config {
            scrolling_history: options.scrollback,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &grid, adapter.clone())));

        let event_loop = EventLoop::new(
            Arc::clone(&term),
            adapter.clone(),
            pty,
            pty_options.drain_on_exit,
            false,
        )?;
        let sender = event_loop.channel();
        // Kanal ancak burada doğar; adapter'ın kopyaları aynı gövdeyi
        // paylaştığı için tek `set` hepsini bağlar.
        let _ = adapter.0.sender.set(sender.clone());

        Ok(Self {
            term,
            sender,
            adapter,
            reader: Mutex::new(Some(event_loop.spawn())),
        })
    }

    /// Çizilecek kareyi verir: yeni içerik yoksa `None` ve **hiç iterasyon**.
    ///
    /// Kilit bir kez alınır. Hasar "çizilsin mi"ye karar verir, "ne
    /// çizileceğine" değil: drawable içeriği korunmadığı için her karede tam
    /// grid taranır.
    ///
    /// `sink` jeneriktir: hücre başına dinamik çağrı yerine satır içine
    /// alınır. **`Term` kilidi tutulurken** çağrılır ve kilit yeniden girilebilir
    /// değildir: `Session`'a geri giren bir sink (`resize`, `frame`) kendi
    /// kendini kilitler. Sink'in işi tamponu doldurmaktır, başka bir şey değil —
    /// `Wake` ile aynı sözleşme.
    pub fn frame(&self, mut sink: impl FnMut(CellBg)) -> Option<Cursor> {
        // Bayrak kilit istemez, kilit ise ucuz değil: `FairMutex::lock()` iki
        // muteks alır ve okuyucu thread PTY'den okumaya başlamadan önce
        // aynı sıraya giriyor. Boştaki kare o sıraya hiç girmesin.
        // Swap ile lock arasına düşen bir `Wakeup` bayrağı yeniden diker;
        // en kötüsü fazladan bir kare, kaçan kare değil.
        if !self.adapter.0.dirty.swap(false, Ordering::AcqRel) {
            return None;
        }
        let term = self.term.lock();

        let rows = term.screen_lines() as i32;
        let RenderableContent {
            display_iter,
            cursor,
            display_offset,
            colors,
            ..
        } = term.renderable_content();
        let offset = display_offset as i32;

        for indexed in display_iter {
            // Ters video hücrenin iki rengini takas eder; arka plan ön plandır.
            let cell = indexed.cell;
            let inverse = cell.flags.contains(Flags::INVERSE);
            let source = if inverse { cell.fg } else { cell.bg };
            let mut rgb = color::resolve(source, colors);
            // `DIM` ön plana uygulanır — ters videoda ön plan artık bu renk.
            // Adlı rengi sönük eşine çeviren kod alacritty'nin ikili
            // tarafında, kitaplıkta değil; çeviri bize düşüyor.
            if inverse && cell.flags.contains(Flags::DIM) {
                rgb = color::dim(rgb);
            }
            if rgb == color::BG_RGB {
                continue;
            }
            // `display_iter` yalnız görünür pencereyi verir: aralığı
            // `-offset ..= -offset + screen_lines - 1`'e kırpar, yani satır
            // zaten 0..rows. Sözleşme değişirse debug derlemesi haber verir;
            // sürüm derlemesinde hücre başına dal kalmaz.
            let row = indexed.point.line.0 + offset;
            debug_assert!((0..rows).contains(&row), "display_iter pencere dışı: {row}");
            // `try_from` sürüm derlemesinde de tutar: negatif bir satır
            // sessizce 65535'e sarsaydı renderer tampon dışına yazardı.
            let Ok(row) = u16::try_from(row) else {
                continue;
            };
            sink(CellBg {
                col: indexed.point.column.0 as u16,
                row,
                rgba: color::rgba(rgb),
            });
        }

        let cursor_row = cursor.point.line.0 + offset;
        Some(Cursor {
            col: cursor.point.column.0 as u16,
            row: cursor_row.clamp(0, rows.saturating_sub(1)) as u16,
            // Kaydırma geçmişine bakarken imleç ekranın dışına çıkar.
            visible: cursor.shape != CursorShape::Hidden && (0..rows).contains(&cursor_row),
        })
    }

    /// Hasarı uzaktan işaretleyebilen tutamak.
    ///
    /// Oturumun kendisine referans **vermez** ve bu kasıtlı: tutamağı tutan
    /// taraf (`bt-gpu`'nun `Waker`'ı, oradan da Metal'in tamamlanma bloğu)
    /// hiçbir koşulda `Arc<Session>` maddileştirmemeli. Maddileştirseydi son
    /// güçlü referans okuyucu ya da GPU thread'inde düşebilir, `Drop` →
    /// `shutdown()` → `join()` zinciri orada koşar ve thread kendi kendini
    /// beklerdi (`EDEADLK`) — `Wake` sözleşmesinin (`wake.rs`) yasakladığı
    /// tam olarak bu. `Weak<Session>` bile yetmez: `upgrade()` o referansı
    /// çağrı süresince maddileştirir.
    pub fn dirty_flag(&self) -> DirtyFlag {
        DirtyFlag(Arc::clone(&self.adapter.0.dirty))
    }

    /// Klavyeden ya da başka bir kaynaktan PTY'ye bayt akıtır.
    ///
    /// Boş dilim sessizce düşer: sıfır baytlık bir `Msg::Input`
    /// `EventLoop`'un yazıcısını kalıcı olarak kilitler (bkz. `Adapter::reply`).
    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.send(Msg::Input(bytes.to_vec().into()));
    }

    /// Grid'i ve PTY'yi yeni boyuta getirir. Reflow alacritty'nindir.
    ///
    /// `true` → boyut gerçekten değişti ve uygulandı. `false` iki durumda
    /// döner: boyut dejenere (yoksayıldı) ya da zaten aynı. Çağıranın buna
    /// ihtiyacı var çünkü hücre piksel boyutunu **kendi** tarafında da
    /// tutuyor: yoksayılan bir boyutu orada uygulamak grid'i eski ölçüde
    /// bırakıp çizimi yeni ölçüye kaydırırdı.
    #[must_use]
    pub fn resize(&self, cols: u16, rows: u16, cell_px: (u16, u16)) -> bool {
        // Simge durumuna inen ya da sıfır yükseklikli pencere 0 hesaplatabilir.
        // Bu boyut kırpılmaz, YOKSAYILIR: 1 sütuna reflow geçmişi kalıcı
        // olarak yok eder ve PTY'ye 1×1 winsize gitmesi tam ekran uygulamaları
        // bozar. Görünmeyen pencerede çizecek bir şey de yok.
        if cols == 0 || rows == 0 {
            return false;
        }
        let grid = GridSize::tam(cols, rows);
        let size = window_size(grid, cell_px);

        // Ucuz kapı önce. Canlı boyutlandırmada `windowDidResize:`
        // çağrılarının çoğu hücre sınırını geçmez ve hiçbir şey yapmaz;
        // `Term`'ün kilidi ise okuyucunun ayrıştırma lease'inin arkasında
        // bekleyebilir. Küçük kilitle eleyip oraya hiç girmiyoruz. Guard
        // `term`'den ÖNCE düşüyor, kilit sırası (term → size) bozulmuyor.
        let degisti = !ayni_boyut(*kilit(&self.adapter.0.size), size);
        if !degisti {
            return false;
        }

        // Üç adım tek kilit tutuşunda: grid, adapter'ın bildiği boyut ve
        // PTY'ye giden mesaj. Ayrı ayrı yapılsalardı eşzamanlı iki resize
        // grid'i bir sayıda, `TIOCSWINSZ`'i başkasında bırakabilirdi.
        // Okuyucu thread de aynı sırayla (term → size) kilit alıyor,
        // kilitlenme yok; `send` kilitsizdir.
        let mut term = self.term.lock();
        let mut onceki = kilit(&self.adapter.0.size);
        // Ön kapıdan iki eşzamanlı resize birlikte geçebilir; ikincisi burada
        // yakalanır. Koşulsuz dikilen bayrak "boşta sıfır kare"yi delerdi.
        if ayni_boyut(*onceki, size) {
            return false;
        }
        term.resize(grid);
        *onceki = size;
        self.send(Msg::Resize(size));
        // Boyut değişimi grid'i değiştiren ama `Wakeup` üretmeyen tek yol,
        // bayrağı bu yüzden elle dikiyoruz. Uyandırmak çağıranın işi
        // (`bt-shell` resize'dan sonra link'i açar); bayrak kimseyi uyandırmaz.
        self.adapter.0.dirty.store(true, Ordering::Release);
        true
    }

    /// Okuyucu thread'i durdurur ve shell çocuğunu bitirir.
    ///
    /// `Msg::Shutdown` → `join` → dönen `(EventLoop, State)`'in düşmesi.
    /// `SIGHUP` o son adımda, `Pty`'nin `Drop`'unda gider. İkinci çağrı
    /// sessizce döner.
    ///
    /// **Bloklar.** `Pty::drop` `SIGHUP`'tan sonra `child.wait()` çağırıyor;
    /// sinyali yutan bir çocuk (`trap '' HUP`) bu çağrıyı süresiz bekletir.
    /// Kesecek olan `bt-shell`'in bekçi thread'i ve o yalnız `BT_RUN_SECONDS`
    /// yolunda kurulur: etkileşimli kullanımda böyle bir çocuk uygulamayı
    /// gerçekten asar (bilinen sınır, `.tasks/002-vt-motoru/phase-4.md`).
    pub fn shutdown(&self) {
        let Some(reader) = kilit(&self.reader).take() else {
            return;
        };
        self.send(Msg::Shutdown);
        if reader.join().is_err() {
            eprintln!("bateri: okuyucu thread panikle bitti");
        }
    }

    /// Okuyucu thread hâlâ çalışıyor mu. `false` ya kapandığımız ya da
    /// shell'in öldüğü anlamına gelir.
    pub fn reader_alive(&self) -> bool {
        kilit(&self.reader)
            .as_ref()
            .is_some_and(|reader| !reader.is_finished())
    }

    fn send(&self, msg: Msg) {
        // Kanal alıcısı okuyucu thread'in dönüş değerinde yaşıyor, yani
        // `join` edilene kadar `send` başarılı olur: shell kendi kendine
        // çıktıktan sonra yazılan baytlar kimsenin emmeyeceği bir kuyruğa
        // gider. "Gitti mi" sorusunun cevabı `reader_alive()`. Bu dal
        // yalnız `shutdown` sonrası yazmayı yakalar; giriş yolunda panik
        // yasak, bir satır iz yeter.
        if self.sender.send(msg).is_err() {
            eprintln!("bateri: okuyucu thread kapalı, PTY'ye yazılamadı");
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // `shutdown()`suz düşen bir oturumda alacritty'nin okuyucu thread'i
        // "event loop channel closed" diye panikler.
        self.shutdown();
    }
}

/// `WindowSize` `PartialEq` türetmiyor; dört alanı elle karşılaştırıyoruz.
fn ayni_boyut(a: WindowSize, b: WindowSize) -> bool {
    a.num_cols == b.num_cols
        && a.num_lines == b.num_lines
        && a.cell_width == b.cell_width
        && a.cell_height == b.cell_height
}

fn window_size(grid: GridSize, cell_px: (u16, u16)) -> WindowSize {
    WindowSize {
        num_cols: grid.cols as u16,
        num_lines: grid.rows as u16,
        cell_width: cell_px.0,
        cell_height: cell_px.1,
    }
}

/// Zehirlenmiş kilitten içeriği geri alır. Bu kilitlerin altında `Option`
/// ve dört sayı var; panik anında da tutarlılar. Kapanış yolunda ikinci bir
/// panik üretmenin kimseye faydası yok.
fn kilit<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::*;

    /// Uyandırmaları sayar ve sınamanın beklemesine izin verir.
    #[derive(Default)]
    struct TestWake {
        durum: Mutex<(u32, Option<Option<i32>>)>,
        kosul: Condvar,
    }

    impl TestWake {
        /// En az `hedef` uyandırma gelene kadar bekler.
        fn bekle(&self, hedef: u32, sure: Duration) -> u32 {
            let durum = self.durum.lock().unwrap();
            let (durum, _) = self
                .kosul
                .wait_timeout_while(durum, sure, |(sayac, _)| *sayac < hedef)
                .unwrap();
            durum.0
        }

        /// Çocuk ölene kadar bekler; zaman aşımında `None`.
        fn bekle_cikis(&self, sure: Duration) -> Option<Option<i32>> {
            let durum = self.durum.lock().unwrap();
            let (durum, _) = self
                .kosul
                .wait_timeout_while(durum, sure, |(_, kod)| kod.is_none())
                .unwrap();
            durum.1
        }
    }

    impl Wake for TestWake {
        fn wake(&self) {
            self.durum.lock().unwrap().0 += 1;
            self.kosul.notify_all();
        }

        fn child_exit(&self, code: Option<i32>) {
            self.durum.lock().unwrap().1 = Some(code);
            self.kosul.notify_all();
        }
    }

    fn oturum(betik: &str, wake: Arc<TestWake>) -> Session {
        Session::spawn(
            SessionOptions {
                command: Some(("/bin/sh".into(), vec!["-c".into(), betik.into()])),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                scrollback: 100,
            },
            wake,
        )
        .unwrap()
    }

    #[test]
    fn session_send_ve_sync() {
        // Renderer `Arc<Session>`'ı ana thread'de, okuyucu thread'i PTY'de
        // kullanır; bu iki sınır derleme zamanında bağlanmalı.
        fn kontrol<T: Send + Sync>() {}
        kontrol::<Session>();
    }

    /// `adet` arka plan hücresi çizilen ilk kareyi bekler.
    fn hucreleri_bekle(session: &Session, wake: &TestWake, adet: usize) -> Vec<CellBg> {
        let bitis = Instant::now() + Duration::from_secs(5);
        let mut hucreler = Vec::new();
        let mut gorulen = 0;
        loop {
            assert!(
                Instant::now() < bitis,
                "beklenen çıktı gelmedi: {hucreler:?}"
            );
            gorulen = wake.bekle(gorulen + 1, Duration::from_millis(500));
            hucreler.clear();
            if session.frame(|c| hucreler.push(c)).is_some() && hucreler.len() == adet {
                return hucreler;
            }
        }
    }

    #[test]
    fn sabit_shell_arka_plan_hucreleri_verir() {
        let wake = Arc::new(TestWake::default());
        // `smoke_shell`'in ta kendisi: `make duman`'ın koştuğu betiğin sekiz
        // hücre verdiğini doğrulayan yer burası. Betiğin `sleep`'i uzun ama
        // önemsiz — oturum düşerken `SIGHUP` çocuğu keser.
        let (program, args) = smoke_shell();
        let session = Session::spawn(
            SessionOptions {
                command: Some((program, args)),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                scrollback: 100,
            },
            Arc::clone(&wake) as Arc<dyn Wake>,
        )
        .unwrap();

        let hucreler = hucreleri_bekle(&session, &wake, 8);

        // " bateri " → sekiz hücre, hepsi ilk satırda ve kırmızı.
        assert!(hucreler.iter().all(|c| c.row == 0), "{hucreler:?}");
        assert_eq!(
            hucreler.iter().map(|c| c.col).collect::<Vec<_>>(),
            (0..8).collect::<Vec<_>>()
        );
        let kirmizi = color::rgba(color::default(1));
        assert!(hucreler.iter().all(|c| c.rgba == kirmizi), "{hucreler:?}");
    }

    #[test]
    fn hasarsiz_frame_sink_cagirmaz() {
        let wake = Arc::new(TestWake::default());
        // Shell ÇIKTI üretmeli: boş grid'de her hücre varsayılan arka planlı
        // olduğu için sink zaten çağrılmazdı, yani kirli kapısı tamamen
        // silinse bile sayaç 0 kalır ve sınama hiçbir şey bağlamazdı.
        let session = oturum("printf '\\033[41m x \\033[0m'; sleep 5", Arc::clone(&wake));

        // Dolu kareyi tüket: " x " → üç kırmızı hücre.
        assert_eq!(hucreleri_bekle(&session, &wake, 3).len(), 3);

        // İkinci çağrı hasarsız: ne kare ne iterasyon. Kapı düşseydi aynı üç
        // hücre yeniden emilir ve sayaç büyürdü.
        let mut sayac = 0;
        assert!(session.frame(|_| sayac += 1).is_none());
        assert_eq!(sayac, 0, "hasarsız kare sink'i çağırdı");
    }

    #[test]
    fn ters_videoda_sonuk_bayragi_arka_plani_koyultur() {
        let wake = Arc::new(TestWake::default());
        // DIM + INVERSE + kırmızı ön plan: ön plan arka plan olur ve sönük
        // uygulanır. alacritty kitaplığı `Named(Red)`i `DimRed`e çevirmez.
        let session = oturum(
            "printf '\\033[2;7;31mx\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        let hucreler = hucreleri_bekle(&session, &wake, 1);
        assert_eq!(hucreler[0].rgba, color::rgba(color::dim(color::default(1))));
        // Sönük olmayan kırmızıdan gerçekten farklı.
        assert_ne!(hucreler[0].rgba, color::rgba(color::default(1)));
    }

    #[test]
    fn bos_yazma_pty_yazicisini_kilitlemez() {
        let wake = Arc::new(TestWake::default());
        let session = oturum(
            "read x; printf '\\033[42m%s\\033[0m\\n' \"$x\"; sleep 5",
            Arc::clone(&wake),
        );

        // Sıfır baytlık `Msg::Input` `EventLoop`'un yazıcısını kalıcı olarak
        // kilitlerdi: arkasından gelen her tuş kuyrukta kalırdı.
        session.write(b"");
        session.write(b"ab\n");

        // İki yeşil hücre: shell girdiyi okuyup geri yazabildi.
        let hucreler = hucreleri_bekle(&session, &wake, 2);
        let yesil = color::rgba(color::default(2));
        assert!(hucreler.iter().all(|c| c.rgba == yesil), "{hucreler:?}");
    }

    #[test]
    fn sifir_boyut_yoksayilir() {
        let wake = Arc::new(TestWake::default());
        let session = oturum("sleep 5", Arc::clone(&wake));

        // Açılış karesi: grid boş ama pencere bir kez boyanmalı (bayrak
        // `Adapter::new`'da `true` başlıyor).
        assert!(session.frame(|_| ()).is_some());
        assert!(session.frame(|_| ()).is_none());

        // Simge durumuna inen pencere `bounds()`'tan sıfır hesaplatabilir.
        // Bu boyut grid'e HİÇ ulaşmamalı: 1 sütuna kırpmak alacritty'de
        // panik yerine daha kötüsünü yapar, geçmişi kalıcı olarak yok eder.
        // Hasar işaretlenmemesi resize'ın hiç olmadığının kanıtı.
        // Dönüş değeri de sözleşmenin parçası: çağıran hücre piksel boyutunu
        // buna bakarak uyguluyor.
        assert!(!session.resize(0, 24, (9, 18)));
        assert!(!session.resize(80, 0, (9, 18)));
        assert!(!session.resize(0, 0, (9, 18)));
        assert!(
            session.frame(|_| ()).is_none(),
            "dejenere boyut grid'e ulaştı"
        );

        // Gerçek boyut değişimi hasar işaretler.
        assert!(session.resize(80, 24, (9, 18)));
        assert!(session.frame(|_| ()).is_some());
        // Aynı boyut ikinci kez: değişiklik yok, hasar yok.
        assert!(!session.resize(80, 24, (9, 18)));
        assert!(session.frame(|_| ()).is_none());
        // Yalnız hücre piksel boyutu değişse de bu bir değişikliktir: PTY'ye
        // giden `TIOCSWINSZ` onu taşıyor (Retina'ya taşınan pencere).
        assert!(session.resize(80, 24, (18, 36)));
    }

    #[test]
    fn shutdown_okuyucuyu_bitirir() {
        let wake = Arc::new(TestWake::default());
        let session = oturum("sleep 30", Arc::clone(&wake));
        assert!(session.reader_alive());

        let baslangic = Instant::now();
        session.shutdown();
        assert!(!session.reader_alive());
        // `sleep 30` sürerken bile SIGHUP yolu hemen dönmeli.
        assert!(
            baslangic.elapsed() < Duration::from_secs(5),
            "{:?}",
            baslangic.elapsed()
        );
        // İkinci çağrı sessizce döner.
        session.shutdown();
    }

    #[test]
    fn cocuk_olunce_child_exit_gelir() {
        let wake = Arc::new(TestWake::default());
        let session = oturum("exit 3", Arc::clone(&wake));

        assert_eq!(wake.bekle_cikis(Duration::from_secs(5)), Some(Some(3)));
        drop(session);
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn yaris_wake_ve_frame() {
        let wake = Arc::new(TestWake::default());
        // Çıktı bilerek kısıtlı: aranan şey yarış, kuyruk şişirmesi değil.
        // Kısıtsız `printf` döngüsü saniyede milyonlarca `Msg::Input`
        // biriktirir ve sınama yarışı değil belleği ölçer.
        let session = Arc::new(oturum(
            "while :; do printf '\\033[42mx\\033[0m'; sleep 0.01; done",
            Arc::clone(&wake),
        ));

        let bitis = Instant::now() + Duration::from_secs(2);
        let yazanlar: Vec<_> = (0..4)
            .map(|n| {
                let session = Arc::clone(&session);
                std::thread::spawn(move || {
                    while Instant::now() < bitis {
                        session.write(b" ");
                        // Sütun sayısı oynasın ki reflow da yarışa girsin.
                        let _ = session.resize(40 + n % 2, 10, (9, 18));
                        std::thread::sleep(Duration::from_millis(1));
                    }
                })
            })
            .collect();

        let mut kare = 0u64;
        while Instant::now() < bitis {
            if session.frame(|_| ()).is_some() {
                kare += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        for yazan in yazanlar {
            yazan.join().unwrap();
        }
        assert!(kare > 0, "yarış boyunca hiç kare üretilmedi");
        // Aranan hata sınıfı tam olarak budur: okuyucu thread paniklerse
        // `shutdown()` yalnız stderr'e yazar ve sınama yeşil kalırdı.
        // Çocuk sonsuz döngüde, thread'in bitmiş olmasının tek açıklaması panik.
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        assert!(wake.bekle(1, Duration::ZERO) > 0, "hiç uyandırma gelmedi");
        session.shutdown();
    }
}
