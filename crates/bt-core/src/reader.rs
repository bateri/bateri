//! PTY'nin okuyucu döngüsü: G/Ç, ayrıştırma ve yazma kuyruğu.
//!
//! **Bu dosya bir kopyadır.** Kaynak `alacritty_terminal` 0.26.0,
//! `src/event_loop.rs` — Copyright Christian Duerr, Joe Wilm ve alacritty
//! katkıcıları, Apache License 2.0 altında (metni pakette
//! `THIRD-PARTY-LICENSES.txt`, atfı `Credits.html`). Apache-2.0 §4(b)
//! gereği: dosya **değiştirildi** (035 phase-2), değişiklikler şunlar —
//!
//! - ayrıştırıcı `Term`'i doğrudan değil [`ClusterHandler`] üzerinden
//!   görüyor, `advance`'te de DEC 2026 zaman aşımının `stop_sync`'inde de;
//! - `ref_test` kaydı ve `Notifier` çıkarıldı (ikisini de çağıran yok);
//! - `log::error!` satırları `eprintln!` oldu (`log` bu crate'in bağımlılığı
//!   değil, `CLAUDE.md` → loglama borcu);
//! - kanalın ölümü panik değil boş okuma (`bt-core`'da gerekçesiz panik
//!   yok; dal erişilemez); yeniden kaydın paniği gerekçesiyle korundu;
//! - PTY token'ları alacritty'de `pub(crate)`, değerleri buraya kopyalandı;
//! - yorumlar Türkçeye çevrildi.
//!
//! Neden kopya: kümeleme (035) ayrıştırıcının `Handler` çağrılarının
//! **arasına** girmek zorunda ve alacritty'nin döngüsü `Term`'i sabit tip
//! olarak veriyor (`.tasks/035-grapheme-dizileri/discussion.md` → Karar).
//! Sürüm bu yüzden `=0.26.0` ile sabit (kök `Cargo.toml`).
//!
//! Korunan sözleşmeler — `session.rs`'in kilit sırası ve kapanışı bunlara
//! yaslanıyor: terminal lease'i `pty_read` boyunca tutuluyor (modül
//! başlığındaki `term` → `shell` sırası), kilitli okuma [`MAX_LOCKED_READ`]
//! ile sınırlı, `Wakeup` yalnız senkronize edilmemiş bayt işlendiyse
//! gidiyor, ve [`EventLoop::spawn`] `(EventLoop, State)` çiftini
//! döndürüyor — `Pty`'nin düşmesi, yani `SIGHUP`, o çiftin düşmesinde.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi;
use alacritty_terminal::{thread, tty};
use polling::{Event as PollingEvent, Events, PollMode, Poller};

use crate::handler::ClusterHandler;

/// Zorunlu senkronizasyondan önce PTY'den okunacak en çok bayt.
const READ_BUFFER_SIZE: usize = 0x10_0000;

/// Terminal kilitliyken PTY'den okunacak en çok bayt.
const MAX_LOCKED_READ: usize = u16::MAX as usize;

/// Okuma/yazma fd'sinin poller token'ı (alacritty 0.26.0,
/// `tty/unix.rs`, `pub(crate)`). Kaydı `Pty::register` yapıyor, yani sayı
/// orada verilenle aynı olmak zorunda — sürüm sabiti bunun bekçisi.
const PTY_READ_WRITE_TOKEN: usize = 0;

/// Çocuk olayı borusunun poller token'ı (aynı kaynak, aynı gerekçe).
const PTY_CHILD_EVENT_TOKEN: usize = 1;

/// Döngüye gönderilen iletiler.
#[derive(Debug)]
pub(crate) enum Msg {
    /// PTY'ye yazılacak baytlar.
    Input(Cow<'static, [u8]>),

    /// Döngü kapanmalı.
    Shutdown,

    /// PTY yeniden boyutlanmalı.
    Resize(WindowSize),
}

/// Döngünün kanalının gönderen ucu: iletiyi kuyruğa koyar ve poller'ı
/// uyandırır.
#[derive(Clone)]
pub(crate) struct EventLoopSender {
    sender: Sender<Msg>,
    poller: Arc<Poller>,
}

impl EventLoopSender {
    /// Alıcı düştüyse ya da poller uyandırılamadıysa `Err`; çağıranın tek
    /// sorusu "gitti mi", ayrıntı taşınmıyor.
    pub(crate) fn send(&self, msg: Msg) -> Result<(), ()> {
        self.sender.send(msg).map_err(|_| ())?;
        self.poller.notify().map_err(|_| ())
    }
}

/// Bir tamponun ne kadarının yazıldığını izler.
struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

impl Writing {
    #[inline]
    fn new(c: Cow<'static, [u8]>) -> Writing {
        Writing {
            source: c,
            written: 0,
        }
    }

    #[inline]
    fn advance(&mut self, n: usize) {
        self.written += n;
    }

    #[inline]
    fn remaining_bytes(&self) -> &[u8] {
        &self.source[self.written..]
    }

    #[inline]
    fn finished(&self) -> bool {
        self.written >= self.source.len()
    }
}

/// Döngünün değişken durumunun tamamı: yazma kuyruğu, yazılmakta olan
/// tampon ve ayrıştırıcı.
#[derive(Default)]
pub(crate) struct State {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
    parser: ansi::Processor,
    /// Son `Handler` çağrısı `input` mıydı — [`ClusterHandler`]'ın tek
    /// durumu. Sarmalayıcı her `advance`'te yeniden doğduğu için burada:
    /// iki `read` parçasına bölünen bir küme (`👍` · `🏽`) yine kapanmamış.
    last_input: bool,
}

impl State {
    #[inline]
    fn ensure_next(&mut self) {
        if self.writing.is_none() {
            self.goto_next();
        }
    }

    #[inline]
    fn goto_next(&mut self) {
        self.writing = self.write_list.pop_front().map(Writing::new);
    }

    #[inline]
    fn take_current(&mut self) -> Option<Writing> {
        self.writing.take()
    }

    #[inline]
    fn needs_write(&self) -> bool {
        self.writing.is_some() || !self.write_list.is_empty()
    }

    #[inline]
    fn set_current(&mut self, new: Option<Writing>) {
        self.writing = new;
    }
}

/// Bir sonraki iletiye bakabilen alıcı: zaman aşımı kolu "kanalda ileti
/// var mı"yı iletiyi tüketmeden soruyor.
struct PeekableReceiver<T> {
    rx: Receiver<T>,
    peeked: Option<T>,
}

impl<T> PeekableReceiver<T> {
    fn new(rx: Receiver<T>) -> Self {
        Self { rx, peeked: None }
    }

    fn peek(&mut self) -> Option<&T> {
        if self.peeked.is_none() {
            self.peeked = self.rx.try_recv().ok();
        }

        self.peeked.as_ref()
    }

    fn recv(&mut self) -> Option<T> {
        // alacritty kanalın ölümünde (`Disconnected`) panikliyor. Burada o
        // dal erişilemez — gönderen uçlardan biri (`EventLoop::tx`) döngünün
        // kendi alanı, döngü yaşadıkça kanal ölmez — ve panik yasağı onu boş
        // okumaya indiriyor.
        self.peeked.take().or_else(|| self.rx.try_recv().ok())
    }
}

/// Okuyucu döngü: PTY G/Ç'si ve `Term`'i güncelleyen ayrıştırıcı.
pub(crate) struct EventLoop<T: tty::EventedPty, U: EventListener> {
    poll: Arc<Poller>,
    pty: T,
    rx: PeekableReceiver<Msg>,
    tx: Sender<Msg>,
    terminal: Arc<FairMutex<Term<U>>>,
    event_proxy: U,
    drain_on_exit: bool,
    /// Kümeleme açık mı (`SessionOptions::cluster`); sarmalayıcıya her
    /// çağrıda geçiyor.
    cluster: bool,
}

impl<T, U> EventLoop<T, U>
where
    T: tty::EventedPty + OnResize + Send + 'static,
    U: EventListener + Send + 'static,
{
    pub(crate) fn new(
        terminal: Arc<FairMutex<Term<U>>>,
        event_proxy: U,
        pty: T,
        drain_on_exit: bool,
        cluster: bool,
    ) -> io::Result<EventLoop<T, U>> {
        let (tx, rx) = mpsc::channel();
        let poll = Poller::new()?.into();
        Ok(EventLoop {
            poll,
            pty,
            tx,
            rx: PeekableReceiver::new(rx),
            terminal,
            event_proxy,
            drain_on_exit,
            cluster,
        })
    }

    pub(crate) fn channel(&self) -> EventLoopSender {
        EventLoopSender {
            sender: self.tx.clone(),
            poller: self.poll.clone(),
        }
    }

    /// Kanalı boşaltır; `Shutdown` geldiyse `false`.
    fn drain_recv_channel(&mut self, state: &mut State) -> bool {
        while let Some(msg) = self.rx.recv() {
            match msg {
                Msg::Input(input) => state.write_list.push_back(input),
                Msg::Resize(window_size) => self.pty.on_resize(window_size),
                Msg::Shutdown => return false,
            }
        }

        true
    }

    #[inline]
    fn pty_read(&mut self, state: &mut State, buf: &mut [u8]) -> io::Result<()> {
        let mut unprocessed = 0;
        let mut processed = 0;

        // Bir sonraki terminal kilidini PTY okumasına ayır. Lease okuma
        // boyunca tutuluyor ve `TappedPty::read` onun altında koşuyor —
        // `session.rs`'in `term` → `shell` kilit sırası buna dayanıyor.
        let _terminal_lease = Some(self.terminal.lease());
        let mut terminal = None;

        loop {
            // PTY'den oku.
            match self.pty.reader().read(&mut buf[unprocessed..]) {
                // macOS'ta PTY'de okunacak bir şey kalmayınca gelen cevap.
                Ok(0) if unprocessed == 0 => break,
                Ok(got) => unprocessed += got,
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        // Ayrıştırma yetiştiyse ve PTY bloklayacaksa poller'a dön.
                        if unprocessed == 0 {
                            break;
                        }
                    }
                    _ => return Err(err),
                },
            }

            // Terminali kilitlemeyi dene.
            let terminal = match &mut terminal {
                Some(terminal) => terminal,
                None => terminal.insert(match self.terminal.try_lock_unfair() {
                    // Tampon sınırındaysak kilidi bekleyerek al.
                    None if unprocessed >= READ_BUFFER_SIZE => self.terminal.lock_unfair(),
                    None => continue,
                    Some(terminal) => terminal,
                }),
            };

            // Gelen baytları ayrıştır — `Term`'e sarmalayıcının içinden.
            state.parser.advance(
                &mut ClusterHandler::new(&mut **terminal, self.cluster, &mut state.last_input),
                &buf[..unprocessed],
            );

            processed += unprocessed;
            unprocessed = 0;

            // Terminali gereğinden uzun kilitli tutma.
            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // İşlenen baytların hepsi senkronize değilse yeniden çizim iste.
        if state.parser.sync_bytes_count() < processed && processed > 0 {
            self.event_proxy.send_event(Event::Wakeup);
        }

        Ok(())
    }

    #[inline]
    fn pty_write(&mut self, state: &mut State) -> io::Result<()> {
        state.ensure_next();

        'write_many: while let Some(mut current) = state.take_current() {
            'write_one: loop {
                match self.pty.writer().write(current.remaining_bytes()) {
                    Ok(0) => {
                        state.set_current(Some(current));
                        break 'write_many;
                    }
                    Ok(n) => {
                        current.advance(n);
                        if current.finished() {
                            state.goto_next();
                            break 'write_one;
                        }
                    }
                    Err(err) => {
                        state.set_current(Some(current));
                        match err.kind() {
                            ErrorKind::Interrupted | ErrorKind::WouldBlock => break 'write_many,
                            _ => return Err(err),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn spawn(mut self) -> JoinHandle<(Self, State)> {
        thread::spawn_named("PTY reader", move || {
            let mut state = State::default();
            let mut buf = [0u8; READ_BUFFER_SIZE];

            let poll_opts = PollMode::Level;
            let mut interest = PollingEvent::readable(0);

            // TTY'yi `EventedReadWrite` arayüzünden kaydet.
            //
            // SAFETY: kaydın koşulu kaynakların kaydı aşması; fd'lerin sahibi
            // `self.pty` ve kayıt aşağıdaki `deregister`'la, `self` bu
            // thread'den dönmeden önce kalkıyor (alacritty'nin aynı çağrısı).
            if let Err(err) = unsafe { self.pty.register(&self.poll, interest, poll_opts) } {
                eprintln!("bateri: okuyucu döngü kaydı başarısız: {err}");
                return (self, state);
            }

            let mut events = Events::with_capacity(EVENTS_CAPACITY);

            'event_loop: loop {
                // Senkronize güncellemenin (DEC 2026) son tarihinde uyan.
                let handler = state.parser.sync_timeout();
                let timeout = handler
                    .sync_timeout()
                    .map(|st| st.saturating_duration_since(Instant::now()));

                events.clear();
                if let Err(err) = self.poll.wait(&mut events, timeout) {
                    match err.kind() {
                        ErrorKind::Interrupted => continue,
                        _ => {
                            eprintln!("bateri: okuyucu döngü yoklaması başarısız: {err}");
                            break 'event_loop;
                        }
                    }
                }

                // Senkronize güncellemenin zaman aşımı: tamponlanan baytlar
                // **sarmalayıcıya** gidiyor, `advance`'in gördüğü aynı yoldan
                // — `Term`'e doğrudan verilseydi kümeleme (035) bu kolda
                // atlanırdı.
                if events.is_empty() && self.rx.peek().is_none() {
                    state.parser.stop_sync(&mut ClusterHandler::new(
                        &mut *self.terminal.lock(),
                        self.cluster,
                        &mut state.last_input,
                    ));
                    self.event_proxy.send_event(Event::Wakeup);
                    continue;
                }

                // Kanalda ileti varsa işle.
                if !self.drain_recv_channel(&mut state) {
                    break;
                }

                for event in events.iter() {
                    match event.key {
                        PTY_CHILD_EVENT_TOKEN => {
                            if let Some(tty::ChildEvent::Exited(status)) =
                                self.pty.next_child_event()
                            {
                                if let Some(status) = status {
                                    self.event_proxy.send_event(Event::ChildExit(status));
                                }
                                if self.drain_on_exit {
                                    let _ = self.pty_read(&mut state, &mut buf);
                                }
                                self.terminal.lock().exit();
                                self.event_proxy.send_event(Event::Wakeup);
                                break 'event_loop;
                            }
                        }

                        PTY_READ_WRITE_TOKEN => {
                            if event.is_interrupt() {
                                // Ölü bir PTY'de G/Ç deneme.
                                continue;
                            }

                            if event.readable
                                && let Err(err) = self.pty_read(&mut state, &mut buf)
                            {
                                // Linux'ta istemci ucu kapanınca master'ın
                                // `read`'i `EIO` verebilir; kaçınılmaz
                                // `Exited` olayı için döngüye dön. `libc` bu
                                // crate'in bağımlılığı değil: 5, Linux'un
                                // `EIO`'su.
                                #[cfg(target_os = "linux")]
                                if err.raw_os_error() == Some(5) {
                                    continue;
                                }

                                eprintln!("bateri: PTY okunamadı: {err}");
                                break 'event_loop;
                            }

                            if event.writable
                                && let Err(err) = self.pty_write(&mut state)
                            {
                                eprintln!("bateri: PTY'ye yazılamadı: {err}");
                                break 'event_loop;
                            }
                        }
                        _ => (),
                    }
                }

                // Gerekiyorsa yazma ilgisini kaydet.
                let needs_write = state.needs_write();
                if needs_write != interest.writable {
                    interest.writable = needs_write;

                    // Yeni ilgiyle yeniden kaydet. Panik **kasıtlı** ve
                    // alacritty'ninkiyle aynı: `Session::begin_shutdown`
                    // okuyucunun çöküşünü `join`'in `Err`'inden tanıyor
                    // (`kapanis=`); sessiz bir `break` çocuğu canlı, pencereyi
                    // donmuş bırakıp kapanışı `temiz` gösterirdi.
                    if let Err(err) = self.pty.reregister(&self.poll, interest, poll_opts) {
                        panic!("okuyucu döngü yeniden kaydı başarısız: {err}"); // audit: alacritty paritesi, çöküş kapanış raporuna join'in Err'iyle ulaşıyor
                    }
                }
            }

            // Olay kaynakları burada düşmüyor, kayıt açıkça kaldırılıyor.
            let _ = self.pty.deregister(&self.poll);

            (self, state)
        })
    }
}

/// Poller'ın tek turda döndürdüğü en çok olay (alacritty'nin sayısı).
const EVENTS_CAPACITY: NonZeroUsize = match NonZeroUsize::new(1024) {
    Some(capacity) => capacity,
    None => panic!("olay kapasitesi sıfır olamaz"), // audit: const değerlendirmesi, sıfır derleme hatası olur
};
