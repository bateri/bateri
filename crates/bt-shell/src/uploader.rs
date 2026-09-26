//! Yükleme kuyruğunun **pencere yarısı** (037 Karar 7 → Kullanıcı kararı):
//! damladan onay sayfasına, sayfadan akışa, akıştan dock'un durum satırına
//! ve uygulamanın Dock simgesine giden AppKit ve dispatch işi.
//!
//! Kural ve metin `upload`'da (saf, sınanan); burada yalnız bağlama var.
//! Arka plan thread'lerinin her haberi ana kuyruğa **pencere kimliğiyle**
//! gidiyor ve pencereyi listeden buluyor (alternatif ekran habercisinin
//! örüntüsü): kapanmış bir sekmenin haberi sessizce düşüyor.
//!
//! **Boşta sıfır kare:** ilerleme haberi akış sürerken en sık
//! [`upload::TICK`]'te bir ve ana kuyrukta en çok bir tane; kuyruk bitince
//! sonuç satırı [`upload::LINGER`] kadar kalıp `None`'la kalkıyor ve bir
//! daha kare istenmiyor — durma koşulu o.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Instant;

use block2::RcBlock;
use bt_core::{HostMark, Transfer};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSApplication, NSImageScaling, NSImageView, NSMenu,
    NSMenuItem, NSModalResponse, NSProgressIndicator, NSProgressIndicatorStyle, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, ns_string};

use crate::app;
use crate::quote;
use crate::upload::{self, Job, Local, Outcome, ProbeReply, Shared};
use crate::window::TerminalWindow;

/// Arka planın yoklamasından ana thread'e dönen her şey.
struct Asked {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    reported: bool,
    result: Result<(Vec<Local>, ProbeReply), String>,
}

/// Onaylanan damla: kuyruğa girecek öğeler.
struct Confirmed {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    jobs: Vec<Job>,
}

/// `id`'li pencereyi ana thread'de bulur ve `work`'ü ona uygular.
fn on_window(id: u64, work: impl FnOnce(&TerminalWindow) + Send + 'static) {
    DispatchQueue::main().exec_async(move || {
        // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
        let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
        if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
            work(&window);
        }
    });
}

impl TerminalWindow {
    /// Finder damlası uzak oturumda (037 Karar 7): yerel ölçüm ve uzak
    /// yoklama arka planda, sonra onay sayfası. `false` → damla reddedildi
    /// (yerel oturum, ya da başka bir sayfa sürüyor — iki sayfa üst üste
    /// açılamaz).
    pub(crate) fn upload_drop(&self, paths: Vec<String>) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let Some((command, target, cwd)) = session.remote_target() else {
            return false;
        };
        if !self.uploads().borrow().can_accept() {
            return false;
        }
        let mark = session
            .remote_mark()
            .map_or(HostMark::None, |(_, mark)| mark);
        let ssh = upload::ssh_argv(&target);
        let host = target.host;
        let reported = !cwd.is_empty();
        let id = self.id();
        self.uploads().borrow_mut().set_asking(true);
        let spawned = thread::Builder::new()
            .name("upload probe".into())
            .spawn(move || {
                let dir = reported.then_some(cwd.as_str());
                let result = upload::probe(&ssh, &host, dir, &paths);
                let asked = Asked {
                    command,
                    ssh,
                    host,
                    mark,
                    reported,
                    result,
                };
                on_window(id, move |window| window.upload_asked(asked));
            });
        if spawned.is_err() {
            self.uploads().borrow_mut().set_asking(false);
            return false;
        }
        true
    }

    /// Damla bu sekmeye bırakılabilir mi — `draggingEntered:`'ın sorusu: bir
    /// yükleme sayfası (yoklama dahil) sürerken hayır.
    pub(crate) fn accepts_drop(&self) -> bool {
        self.uploads().borrow().can_accept()
    }

    /// Yoklama döndü: onay ya da hata sayfası.
    fn upload_asked(&self, asked: Asked) {
        // Oturum bu arada bittiyse (ssh kapandı) sorulacak bir şey yok.
        let alive = self
            .session()
            .and_then(|session| session.remote_target())
            .is_some_and(|(command, ..)| command == asked.command);
        if !alive {
            self.uploads().borrow_mut().set_asking(false);
            return;
        }
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        let confirmed = match asked.result {
            Err(text) => {
                alert.setMessageText(&NSString::from_str(&format!(
                    "Can't upload to {}",
                    asked.host
                )));
                alert.setInformativeText(&NSString::from_str(&text));
                alert.addButtonWithTitle(ns_string!("OK"));
                None
            }
            Ok((items, reply)) => {
                let sheet = upload::sheet(&asked.host, asked.reported, &items, &reply);
                alert.setMessageText(&NSString::from_str(&sheet.message));
                alert.setInformativeText(&NSString::from_str(&sheet.informative));
                let confirm = alert.addButtonWithTitle(&NSString::from_str(sheet.button));
                confirm.setEnabled(sheet.enabled);
                let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
                // Esc elle (`window::alert`'in gerekçesi).
                cancel.setKeyEquivalent(ns_string!("\u{1b}"));
                sheet.enabled.then(|| Confirmed {
                    command: asked.command,
                    ssh: asked.ssh,
                    host: asked.host,
                    mark: asked.mark,
                    jobs: items
                        .into_iter()
                        .map(|local| Job {
                            local,
                            dir: reply.dir.clone(),
                        })
                        .collect(),
                })
            }
        };
        let id = self.id();
        // Blok `Fn`: yük bir kez alınıyor.
        let confirmed = RefCell::new(confirmed);
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: sayfanın tamamlanma bloğu AppKit'in ana thread'inde koşar.
            let mtm = MainThreadMarker::new().expect("sayfa bloğu ana thread'dedir");
            let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) else {
                return;
            };
            drop(window.upload_alert().take());
            window.uploads().borrow_mut().set_asking(false);
            if response != NSAlertFirstButtonReturn {
                return;
            }
            if let Some(confirmed) = confirmed.borrow_mut().take() {
                window.upload_confirmed(confirmed);
            }
        });
        self.upload_alert().replace(Some(alert.clone()));
        alert.beginSheetModalForWindow_completionHandler(self.ns_window(), Some(&answered));
    }

    /// Onay: öğeler kuyruğun sonuna, kuyruk boştaysa ilk öğe başlıyor.
    fn upload_confirmed(&self, confirmed: Confirmed) {
        let alive = self
            .session()
            .and_then(|session| session.remote_target())
            .is_some_and(|(command, ..)| command == confirmed.command);
        if !alive {
            return;
        }
        let queued = self.uploads().borrow_mut().enqueue(
            confirmed.command,
            confirmed.ssh,
            confirmed.host,
            confirmed.mark,
            confirmed.jobs,
        );
        if queued {
            self.upload_next();
        }
    }

    /// Sıradaki öğeyi arka plan thread'inde başlatır (bir öğe akarken no-op).
    fn upload_next(&self) {
        let Some((ssh, job, shared)) = self.uploads().borrow_mut().start_next() else {
            return;
        };
        self.upload_refresh();
        let id = self.id();
        let spawned = thread::Builder::new().name("upload".into()).spawn({
            let shared = Arc::clone(&shared);
            move || {
                let outcome = upload::transfer(&ssh, &job.local, &job.dir, &shared, || {
                    tick(id, &shared);
                });
                on_window(id, move |window| window.upload_finished(outcome));
            }
        });
        if let Err(error) = spawned {
            self.upload_finished(Outcome::Failed(error.to_string()));
        }
    }

    /// İlerleme haberi: durum satırını ve Dock simgesini tazeler.
    fn upload_refresh(&self) {
        let status = self.uploads().borrow_mut().status(Instant::now());
        if let Some(status) = status {
            self.show_transfer(Some(status));
        }
        refresh_dock_tile(self.mtm());
    }

    /// Akan öğe bitti: yolu yapıştır, sıradakine geç ya da sonucu göster.
    fn upload_finished(&self, outcome: Outcome) {
        let finished = self.uploads().borrow_mut().finish(outcome);
        if let Some(path) = finished.paste
            && let Some(session) = self.session()
            && session.remote_target().map(|(command, ..)| command) == Some(finished.command)
        {
            session.paste(format!("{} ", quote::shell_quote(&[path])).into_bytes());
        }
        match finished.end {
            Some((line, serial)) => self.show_end(line, serial),
            None => self.upload_next(),
        }
        refresh_dock_tile(self.mtm());
    }

    /// Sonuç satırını gösterir ve [`upload::LINGER`] sonra kaldırır.
    fn show_end(&self, line: Transfer, serial: u64) {
        self.show_transfer(Some(line));
        let id = self.id();
        let Ok(when) = DispatchTime::try_from(upload::LINGER) else {
            return;
        };
        // Hata kolu bugün temsil edilmiyor (link'in saatiyle aynı gerekçe);
        // düşerse satır bir sonraki yüklemeye kadar kalır.
        let _ = DispatchQueue::main().after(when, move || {
            // audit: ana kuyrukta koşan blok tanımı gereği ana thread'dedir.
            let mtm = MainThreadMarker::new().expect("ana kuyruk ana thread'dir");
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id))
                && window.uploads().borrow().linger_over(serial)
            {
                window.show_transfer(None);
            }
        });
    }

    /// Durum satırını oturuma yazar ve farenin düğme sorusu için saklar.
    fn show_transfer(&self, transfer: Option<Transfer>) {
        if let Some(session) = self.session() {
            session.set_transfer(transfer.as_ref());
        }
        self.uploads().borrow_mut().set_shown(transfer);
    }

    /// ⌘. ve satırın ✕'i: bütün kuyruk.
    pub(crate) fn cancel_uploads(&self) {
        let end = self.uploads().borrow_mut().cancel();
        if let Some((line, serial)) = end {
            self.show_end(line, serial);
        }
        refresh_dock_tile(self.mtm());
    }

    /// Sekme kapanıyor (`begin_close`): kuyruk iptal ve **bırakılıyor** —
    /// sonucu gösterecek bir dock kalmadı ve akış thread'inin haberi bu
    /// pencereyi artık bulamayacak; Dock simgesi şimdi tazeleniyor, yoksa
    /// yarım bir çubukta donardı.
    pub(crate) fn abandon_uploads(&self) {
        self.uploads().borrow_mut().abandon();
        refresh_dock_tile(self.mtm());
    }

    /// Listenin ✕'i: yalnız o öğe.
    pub(crate) fn cancel_upload_at(&self, index: usize) {
        self.uploads().borrow_mut().remove(index);
        self.upload_refresh();
    }

    /// Uzak oturum bitti mi (`refresh_title`'ın kenarı): bittiyse
    /// bekleyenler iptal, akan öğe kendi bağlantısıyla bitiyor.
    pub(crate) fn check_upload_connection(&self) {
        let Some(command) = self.uploads().borrow().command() else {
            return;
        };
        let current = self
            .session()
            .and_then(|session| session.remote_target())
            .map(|(command, ..)| command);
        if current == Some(command) {
            return;
        }
        let end = self.uploads().borrow_mut().close();
        if let Some((line, serial)) = end {
            self.show_end(line, serial);
        }
        refresh_dock_tile(self.mtm());
    }

    /// Durum satırında dock-yerel `col` sütununa tık (bağlam satırında,
    /// küçük sınıfın adımında): düğmeye düştüyse işini yapar ve `true`.
    /// `context` bağlam satırının bütçesi (`bt_gpu::context_cols`).
    pub(crate) fn upload_click(&self, col: u16, context: u16, at: NSPoint) -> bool {
        let control = {
            let uploads = self.uploads().borrow();
            let Some(shown) = uploads.shown() else {
                return false;
            };
            let Some(start) = bt_core::transfer_controls_col(shown, context) else {
                return false;
            };
            let Some(offset) = col.checked_sub(start) else {
                return false;
            };
            upload::control_at(usize::from(offset))
        };
        match control {
            Some(upload::Control::Cancel) => self.cancel_uploads(),
            Some(upload::Control::List) => self.show_upload_list(at),
            None => return false,
        }
        true
    }

    /// "▴ list": dock'un üstünde küçük bir liste — öğe başına ✕ ve "Cancel
    /// All" (Kullanıcı kararı 4). AppKit'in açılır menüsü: klavye ve
    /// erişilebilirlik bedava, kapanması tıklamanın kendisi.
    fn show_upload_list(&self, at: NSPoint) {
        let items = self.uploads().borrow().items();
        if items.is_empty() {
            return;
        }
        let mtm = self.mtm();
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        for (index, (name, running)) in items.iter().enumerate() {
            let title = if *running {
                format!("✕  {name}  (uploading)")
            } else {
                format!("✕  {name}")
            };
            // SAFETY: seçici bu sınıfın `cancelUploadItem:`'ı ve tek
            // `Option<&AnyObject>` alıyor; hedef açıkça bu pencere.
            let item = unsafe {
                NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(&title),
                    Some(sel!(cancelUploadItem:)),
                    ns_string!(""),
                )
            };
            // audit: liste kuyruğun boyu kadar; `isize`'a sığar.
            item.setTag(index as isize);
            // SAFETY: hedef bu pencere nesnesi ve menü onu yalnız tık anında
            // kullanıyor; nesne pencere listesinde yaşıyor.
            unsafe { item.setTarget(Some(self)) };
            menu.addItem(&item);
        }
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        // SAFETY: seçici bu sınıfın `cancelUpload:`'ı.
        let all = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                ns_string!("Cancel All"),
                Some(sel!(cancelUpload:)),
                ns_string!(""),
            )
        };
        // SAFETY: yukarıdakinin gerekçesi.
        unsafe { all.setTarget(Some(self)) };
        menu.addItem(&all);
        let view: &NSView = self.view();
        menu.popUpMenuPositioningItem_atLocation_inView(None, at, Some(view));
    }
}

/// Akış thread'inin ilerleme haberi: ana kuyrukta en çok bir.
fn tick(id: u64, shared: &Arc<Shared>) {
    if shared.tick_pending.swap(true, Ordering::AcqRel) {
        return;
    }
    let shared = Arc::clone(shared);
    on_window(id, move |window| {
        shared.tick_pending.store(false, Ordering::Release);
        window.upload_refresh();
    });
}

/// Uygulamanın Dock simgesinde bütün pencerelerin yüklemesinin ilerlemesi
/// (Kullanıcı kararı 4). Yükleme yoksa simge kendi hâline dönüyor. İlk
/// yüklemeye kadar hiç dokunulmuyor — açılış (ve süreli koşu) Dock
/// simgesine uğramıyor.
fn refresh_dock_tile(mtm: MainThreadMarker) {
    let Some(app) = app::delegate(mtm) else {
        return;
    };
    let (sent, total) = app
        .windows()
        .iter()
        .filter_map(|window| window.uploads().borrow().totals())
        .fold((0u64, 0u64), |(a, b), (sent, total)| (a + sent, b + total));
    let tile = NSApplication::sharedApplication(mtm).dockTile();
    let active = app
        .windows()
        .iter()
        .any(|window| window.uploads().borrow().active());
    if !active {
        if tile.contentView(mtm).is_some() {
            tile.setContentView(None);
            tile.display();
        }
        return;
    }
    let fraction = if total == 0 {
        0.0
    } else {
        sent.min(total) as f64 / total as f64
    };
    let view = match tile.contentView(mtm) {
        Some(view) => view,
        None => {
            let view = dock_tile_view(mtm);
            tile.setContentView(Some(&view));
            view
        }
    };
    if let Some(bar) = view
        .subviews()
        .iter()
        .find_map(|sub| sub.downcast::<NSProgressIndicator>().ok())
    {
        bar.setDoubleValue(fraction);
    }
    tile.display();
}

/// Dock simgesinin içeriği: uygulamanın simgesi ve altında bir çubuk.
fn dock_tile_view(mtm: MainThreadMarker) -> Retained<NSView> {
    const SIDE: f64 = 128.0;
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIDE, SIDE));
    let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
    let icon = NSImageView::initWithFrame(NSImageView::alloc(mtm), frame);
    if let Some(image) = NSApplication::sharedApplication(mtm).applicationIconImage() {
        icon.setImage(Some(&image));
    }
    icon.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
    view.addSubview(&icon);
    let bar = NSProgressIndicator::initWithFrame(
        NSProgressIndicator::alloc(mtm),
        NSRect::new(NSPoint::new(12.0, 6.0), NSSize::new(SIDE - 24.0, 20.0)),
    );
    bar.setStyle(NSProgressIndicatorStyle::Bar);
    bar.setIndeterminate(false);
    bar.setMinValue(0.0);
    bar.setMaxValue(1.0);
    view.addSubview(&bar);
    view
}
