//! Yükleme kuyruğunun **pencere yarısı** (037 Karar 7 → Kullanıcı kararı):
//! damladan onay sayfasına, sayfadan akışa, akıştan dock'un durum satırına,
//! "Show files (N)" popover'ına, durdurma sorusuna, pencere başlığına,
//! bildirime ve uygulamanın Dock simgesine giden AppKit ve dispatch işi.
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
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Instant;

use block2::RcBlock;
use bt_core::{HostMark, Transfer, TransferAction};
use dispatch2::{DispatchQueue, DispatchTime};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSAlertSecondButtonReturn, NSApplication, NSBox, NSBoxType,
    NSButton, NSColor, NSControlSize, NSEvent, NSEventMask, NSFont, NSFontWeightRegular,
    NSImageScaling, NSImageView, NSLineBreakMode, NSModalResponse, NSModalResponseAbort, NSPopover,
    NSPopoverBehavior, NSProgressIndicator, NSProgressIndicatorStyle, NSTextField, NSView,
    NSViewController,
};
use objc2_foundation::{NSBundle, NSPoint, NSRect, NSRectEdge, NSSize, NSString, ns_string};
// Kullanımdan kalkmış bildirim API'si: gerekçesi `notify`'ın doc'unda.
#[allow(deprecated)]
use objc2_foundation::{NSUserNotification, NSUserNotificationCenter};

use crate::app;
use crate::upload::{
    self, Ended, Job, Local, Outcome, ProbeReply, RowAction, RowStatus, Shared, Stop, StopQuestion,
    UploadList,
};
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
        if !self.accepts_drop() {
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
        self.uploads().borrow().can_accept() && self.upload_stop().borrow().is_none()
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
                let busy = self.uploads().borrow().busy();
                let sheet = upload::sheet(&asked.host, asked.reported, &items, &reply, busy);
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
            // Kuyruk zaten akıyorduysa yeni kalemler listede ve sayıda.
            self.upload_refresh();
        }
    }

    /// Sıradaki öğeyi arka plan thread'inde başlatır (bir öğe akarken no-op).
    fn upload_next(&self) {
        let Some((ssh, job, shared)) = self.uploads().borrow_mut().start_next(Instant::now())
        else {
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

    /// İlerleme haberi: durum satırını, popover'ı, başlığı ve Dock simgesini
    /// tazeler.
    fn upload_refresh(&self) {
        let status = self.uploads().borrow_mut().status(Instant::now());
        if let Some(status) = status {
            self.show_transfer(Some(status));
            self.rehover_upload();
        }
        self.refresh_upload_list();
        self.refresh_upload_title();
        refresh_dock_tile(self.mtm());
    }

    /// Akan öğe bitti: sıradakine geç ya da sonucu göster. Hiçbir yol
    /// yapıştırılmıyor (037 phase-7): sonuç satırı nereye gittiğini söylüyor.
    fn upload_finished(&self, outcome: Outcome) {
        let ended = self.uploads().borrow_mut().finish(outcome);
        // Durdurma sorusu biten kalem içindiyse sayfa kendiliğinden kapanıyor.
        self.dismiss_stale_stop();
        match ended {
            Some(ended) => self.show_end(ended),
            None => self.upload_next(),
        }
        refresh_dock_tile(self.mtm());
    }

    /// Sonuç satırını gösterir, popover'ı ve eskiyen soruyu kapatır, başlığı
    /// eski hâline döndürür, bateri arkadaysa bildirim gönderir ve satırı
    /// [`upload::LINGER`] sonra kaldırır.
    fn show_end(&self, ended: Ended) {
        self.close_upload_list();
        self.dismiss_stale_stop();
        self.show_transfer(Some(ended.line));
        self.refresh_upload_title();
        if let Some((title, body)) = ended.notice {
            notify(self.mtm(), &title, &body);
        }
        let id = self.id();
        let serial = ended.serial;
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

    /// Başlığın `↑ N% · ` öneki (037 phase-7): yüzde değiştiyse başlığı
    /// yeniden yazar — yüzde başına en çok bir kez. Alternatif ekranda dock
    /// yok ve ilerlemeyi gösteren tek yer başlık ile sekme.
    fn refresh_upload_title(&self) {
        let changed = self.uploads().borrow_mut().title_percent_changed();
        if changed {
            self.apply_title();
        }
    }

    /// Durum satırını oturuma yazar ve farenin düğme sorusu için saklar.
    /// El imlecini AppKit'in cursor rect'i kuruyor
    /// (`BateriView::upload_cursor_rects`); düğmeler belirdi ya da kalktıysa
    /// rect'ler burada yenileniyor, yani düğme kalkınca farenin altında el
    /// asılı kalmıyor.
    fn show_transfer(&self, transfer: Option<Transfer>) {
        if let Some(session) = self.session() {
            session.set_transfer(transfer.as_ref());
        }
        self.uploads().borrow_mut().set_shown(transfer);
        // `borrow_mut` bitti: rect'lerin hesabı `uploads`'ı yeniden ödünç
        // alıyor.
        self.view().sync_cursor_rects();
    }

    /// Gösterilen satırın düğmelerinin dock-yerel sütun aralıkları
    /// (`context` bağlam satırının bütçesi) — cursor rect'lerin girdisi, tık
    /// ve hover'la aynı yerleşimden (`bt_core::transfer_button_span`).
    pub(crate) fn upload_button_spans(&self, context: u16) -> Vec<(u16, u16)> {
        let uploads = self.uploads().borrow();
        let Some(shown) = uploads.shown() else {
            return Vec::new();
        };
        [TransferAction::List, TransferAction::Cancel]
            .into_iter()
            .filter_map(|action| bt_core::transfer_button_span(shown, context, action))
            .collect()
    }

    /// Fare bağlam satırında dock-yerel `col` sütununda (`None` → satırın
    /// dışında; `context` bağlam satırının bütçesi): üstündeki düğme
    /// **değiştiyse** satırı yeniden yazar — hover tonu, kare yalnız o
    /// kenarda (037 phase-6). İmleci kurmuyor: el cursor rect'ten, burada
    /// yalnız bayatsa yenileniyor. Yükleme yoksa ilk soruda çıkıyor —
    /// boştaki pencerenin her hareketi bir ödünç almaya mal oluyor.
    pub(crate) fn upload_hover(&self, at: Option<(u16, u16)>) {
        let fresh = {
            let mut uploads = self.uploads().borrow_mut();
            let Some(shown) = uploads.shown() else {
                return;
            };
            let hover =
                at.and_then(|(col, context)| bt_core::transfer_button_at(shown, context, col));
            uploads.set_hover(hover)
        };
        match fresh {
            Some(fresh) => self.show_transfer(Some(fresh)),
            // Dock'un çizilen yeri oynadıysa (punto, pencere boyu, bant) el
            // imlecinin dikdörtgeni de oynamalı.
            None => self.view().sync_cursor_rects(),
        }
    }

    /// Hover'ı farenin **şimdiki** yerinden yeniden hesaplar: düğmeler sağa
    /// yaslı ve genişlikleri durumdan (kalem sayısı), yani satır kıpırdamayan
    /// farenin altında değişebiliyor — `/code-review`.
    pub(crate) fn rehover_upload(&self) {
        let at = self.view().pointer_context_column();
        self.upload_hover(at);
    }

    /// Pencere key olmaktan çıktı: `mouseMoved:` artık gelmiyor, düğme
    /// hover'da asılı kalmasın.
    pub(crate) fn unhover_upload(&self) {
        self.upload_hover(None);
    }

    /// Durdurma isteği (037 phase-7): ⌘., satırın `Cancel`/`Cancel all`'ı
    /// ve popover'ın `Cancel all`'ı `all`, popover satırının `Cancel`'ı
    /// değil. Akan kalem [`upload::STOP_ASK_AFTER`]'dan uzun süredir
    /// akıyorsa önce sorulur; açık bir yükleme sayfası varken (onay ya da
    /// soru) istek düşüyor — iki sayfa üst üste açılamaz.
    pub(crate) fn request_stop(&self, all: bool) {
        if self.uploads().borrow().asking()
            || self.upload_alert().borrow().is_some()
            || self.upload_stop().borrow().is_some()
        {
            return;
        }
        let request = self.uploads().borrow().stop_request(all, Instant::now());
        match request {
            None => {}
            Some(Stop::Now { id, all }) => self.apply_stop(id, all),
            Some(Stop::Ask(question)) => {
                self.close_upload_list();
                self.ask_stop(question);
            }
        }
    }

    /// Durdurmayı uygular ([`Uploads::stop`]); sonuç hemen belliyse gösterir.
    fn apply_stop(&self, id: Option<u64>, all: bool) {
        let ended = self.uploads().borrow_mut().stop(id, all);
        match ended {
            Some(ended) => self.show_end(ended),
            None => self.upload_refresh(),
        }
        refresh_dock_tile(self.mtm());
    }

    /// "Stop uploading?" sayfası: `Keep uploading` varsayılan (Return) ve
    /// Esc, `Stop` yıkıcı. Sayfa açıkken yükleme sürüyor; kalem bu arada
    /// biterse sayfa kendiliğinden kapanıyor ([`Self::dismiss_stale_stop`]).
    ///
    /// **Esc elle**: `NSAlert` bir düğmeye tek tuş eşdeğeri veriyor — ilk
    /// düğme Return'ü taşıyor ve Esc'i de ona vermek Return'ü alırdı. Sayfa
    /// süresince bir yerel olay izleyicisi sayfanın penceresindeki Esc'i
    /// `Keep uploading` cevabına çeviriyor.
    fn ask_stop(&self, question: StopQuestion) {
        let mtm = self.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&question.title));
        alert.setInformativeText(&NSString::from_str(&question.text));
        alert.addButtonWithTitle(ns_string!("Keep uploading"));
        let stop = alert.addButtonWithTitle(ns_string!("Stop"));
        stop.setHasDestructiveAction(true);
        let id = self.id();
        let (item, all) = (question.id, question.all);
        let answered = RcBlock::new(move |response: NSModalResponse| {
            // audit: sayfanın tamamlanma bloğu AppKit'in ana thread'inde koşar.
            let mtm = MainThreadMarker::new().expect("sayfa bloğu ana thread'dedir");
            let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) else {
                return;
            };
            let sheet = window.upload_stop().borrow_mut().take();
            if let Some(sheet) = sheet {
                remove_monitor(sheet.monitor);
            }
            if response == NSAlertSecondButtonReturn {
                window.apply_stop(Some(item), all);
            }
        });
        let sheet_window = alert.window();
        let monitor = add_key_monitor(move |event| {
            if event.keyCode() != ESCAPE {
                return false;
            }
            // audit: yerel olay izleyicisi ana thread'de koşar.
            let mtm = MainThreadMarker::new().expect("olay izleyicisi ana thread'dedir");
            // Pencere olay anında karşılaştırılıyor: sayfanın numarası ancak
            // gösterildikten sonra kesin (`/code-review`).
            let on_sheet = event
                .window(mtm)
                .is_some_and(|window| Retained::as_ptr(&window) == Retained::as_ptr(&sheet_window));
            if !on_sheet {
                return false;
            }
            if let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) {
                window
                    .ns_window()
                    .endSheet_returnCode(&sheet_window, NSAlertFirstButtonReturn);
            }
            true
        });
        self.upload_stop().replace(Some(StopSheet {
            alert: alert.clone(),
            id: item,
            monitor,
        }));
        alert.beginSheetModalForWindow_completionHandler(self.ns_window(), Some(&answered));
    }

    /// Durdurma sorusu artık akmayan bir kalem içinse sayfayı kapatır
    /// (cevap "vazgeç" sayılıyor): soru o kalemin kaybını söylemişti.
    fn dismiss_stale_stop(&self) {
        let running = self.uploads().borrow().running_id();
        let stale = self
            .upload_stop()
            .borrow()
            .as_ref()
            .filter(|sheet| running != Some(sheet.id))
            .map(|sheet| sheet.alert.window());
        if let Some(sheet_window) = stale {
            self.ns_window()
                .endSheet_returnCode(&sheet_window, NSModalResponseAbort);
        }
    }

    /// Sekme kapanıyor (`begin_close`): kuyruk iptal ve **bırakılıyor** —
    /// sonucu gösterecek bir dock kalmadı ve akış thread'inin haberi bu
    /// pencereyi artık bulamayacak; Dock simgesi şimdi tazeleniyor, yoksa
    /// yarım bir çubukta donardı.
    pub(crate) fn abandon_uploads(&self) {
        self.close_upload_list();
        self.uploads().borrow_mut().abandon();
        self.dismiss_stale_stop();
        refresh_dock_tile(self.mtm());
    }

    /// Popover satırının düğmesi (`tag` kalemin kimliği): akan kalemde
    /// `Cancel` (30 sn'yi geçtiyse önce sorar), bekleyende `Remove` (sormaz).
    pub(crate) fn upload_row_action(&self, id: u64) {
        let action = self.uploads().borrow().list().and_then(|list| {
            list.rows
                .into_iter()
                .find(|row| row.id == id)
                .and_then(|row| row.status.action())
        });
        match action {
            Some(RowAction::Cancel) => self.request_stop(false),
            Some(RowAction::Remove) => {
                self.uploads().borrow_mut().remove(id);
                self.upload_refresh();
            }
            None => {}
        }
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
        let ended = self.uploads().borrow_mut().close();
        match ended {
            Some(ended) => self.show_end(ended),
            // Bekleyenler artık başlamayacak: popover kapanıyor.
            None => self.refresh_upload_list(),
        }
        refresh_dock_tile(self.mtm());
    }

    /// Durum satırında dock-yerel `col` sütununa tık (bağlam satırında,
    /// küçük sınıfın adımında): düğmeye düştüyse işini yapar ve `true`.
    /// `context` bağlam satırının bütçesi (`bt_gpu::context_cols`).
    pub(crate) fn upload_click(&self, col: u16, context: u16) -> bool {
        let (action, span) = {
            let uploads = self.uploads().borrow();
            let Some(shown) = uploads.shown() else {
                return false;
            };
            (
                bt_core::transfer_button_at(shown, context, col),
                bt_core::transfer_button_span(shown, context, TransferAction::List),
            )
        };
        match action {
            Some(TransferAction::Cancel) => self.request_stop(true),
            Some(TransferAction::List) => self.toggle_upload_list(span),
            None => return false,
        }
        true
    }

    /// "Show files (N)": kuyruğun popover'ı (037 phase-7) — düğmeye bağlı,
    /// `transient`: dışarı tık, Esc ya da düğmeye yeniden basmak kapatıyor.
    ///
    /// **Düğmeye yeniden basmak**: `transient` popover dışarıdaki basışta
    /// kendini kapatıyor ve aynı basış buraya da varıyor — açık görünen
    /// popover'ı yeniden açmamak için kapanışı tetikleyen olayın zamanı
    /// saklanıyor (`popoverWillClose:`) ve bu olaysa hiçbir şey yapılmıyor.
    fn toggle_upload_list(&self, span: Option<(u16, u16)>) {
        let shown = self
            .upload_list()
            .borrow()
            .as_ref()
            .map(|list| list.popover.clone());
        if let Some(popover) = shown {
            // Kapanmakta olan (ya da AppKit'in kapattığı) popover da
            // temizleniyor: izleyicisi ikinciyle üst üste kalmasın.
            let was_shown = popover.isShown();
            self.close_upload_list();
            if was_shown {
                return;
            }
        }
        if self.closed_by_current_event() {
            return;
        }
        let Some(list) = self.uploads().borrow().list() else {
            return;
        };
        if list.rows.len() <= 1 {
            return;
        }
        let Some(rect) = span.and_then(|(start, end)| self.view().context_span_rect(start, end))
        else {
            return;
        };
        let mtm = self.mtm();
        let popover = NSPopover::new(mtm);
        popover.setBehavior(NSPopoverBehavior::Transient);
        popover.setDelegate(Some(ProtocolObject::from_ref(self)));
        let controller = NSViewController::new(mtm);
        let content = NSView::new(mtm);
        let (size, live) = self.fill_list_view(&content, &list);
        controller.setView(&content);
        popover.setContentViewController(Some(&controller));
        popover.setContentSize(size);
        let id = self.id();
        let number = self.ns_window().windowNumber();
        // Esc: terminal penceresi key kalıyor, yani Esc popover'a değil
        // `keyDown:`'a — oradan uzak kabuğa — giderdi. İzleyici popover
        // açıkken bu penceredeki Esc'i yutup popover'ı kapatıyor.
        let monitor = add_key_monitor(move |event| {
            if event.keyCode() != ESCAPE || event.windowNumber() != number {
                return false;
            }
            // audit: yerel olay izleyicisi ana thread'de koşar.
            let mtm = MainThreadMarker::new().expect("olay izleyicisi ana thread'dedir");
            let Some(window) = app::delegate(mtm).and_then(|app| app.window(id)) else {
                return false;
            };
            if window.upload_list().borrow().is_some() {
                window.close_upload_list();
                return true;
            }
            // Popover Esc'i kendisi kapattıysa da tuş kabuğa gitmesin.
            window.closed_by_current_event()
        });
        self.upload_list().replace(Some(UploadPopover {
            popover: popover.clone(),
            shape: shape_of(&list),
            live,
            monitor,
        }));
        let view: &NSView = self.view();
        popover.showRelativeToRect_ofView_preferredEdge(rect, view, NSRectEdge::MinY);
        let opened = self.uploads().borrow_mut().set_list_open(true);
        if let Some(line) = opened {
            self.show_transfer(Some(line));
        }
    }

    /// Kapanışı tetikleyen olay şimdiki olay mı ([`Self::toggle_upload_list`]).
    fn closed_by_current_event(&self) -> bool {
        let now = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .map(|event| event.timestamp());
        now.is_some() && self.list_closed_at().get() == now
    }

    /// `popoverWillClose:`: kapanışı tetikleyen olayın zamanı.
    pub(crate) fn upload_list_will_close(&self) {
        let now = NSApplication::sharedApplication(self.mtm())
            .currentEvent()
            .map(|event| event.timestamp());
        self.list_closed_at().set(now);
    }

    /// Popover'ı kapatır ve düğmeyi basılı tondan çıkarır; kapalıysa no-op.
    /// `popoverDidClose:` da buraya düşüyor (AppKit kendisi kapattı).
    pub(crate) fn close_upload_list(&self) {
        let Some(list) = self.upload_list().borrow_mut().take() else {
            return;
        };
        remove_monitor(list.monitor);
        if list.popover.isShown() {
            list.popover.close();
        }
        let closed = self.uploads().borrow_mut().set_list_open(false);
        if let Some(line) = closed {
            self.show_transfer(Some(line));
        }
        // Popover açıkken hareketler ona gitti: fare artık başka yerde olabilir.
        self.rehover_upload();
    }

    /// Popover açıkken ilerlemeyi yerinde tazeler; kalemler ya da hâlleri
    /// değiştiyse içeriği yeniden kurar. Kuyruk bittiyse ya da kalem sayısı
    /// bire indiyse popover kapanıyor (düğme de kalkıyor).
    ///
    /// Yerinde, çünkü her 200 ms'de yeniden kurulan bir düğme basılıyken
    /// kaybolur ve tık düşerdi.
    fn refresh_upload_list(&self) {
        if self.upload_list().borrow().is_none() {
            return;
        }
        let list = self.uploads().borrow().list();
        let Some(list) = list.filter(|list| list.rows.len() > 1) else {
            self.close_upload_list();
            return;
        };
        let rebuild = {
            let open = self.upload_list().borrow();
            let Some(open) = open.as_ref() else {
                return;
            };
            if open.shape == shape_of(&list) {
                let running = list.rows.iter().filter_map(|row| match &row.status {
                    RowStatus::Running { fraction, detail } => Some((*fraction, detail)),
                    _ => None,
                });
                for ((bar, label), (fraction, detail)) in open.live.iter().zip(running) {
                    bar.setDoubleValue(fraction);
                    label.setStringValue(&NSString::from_str(detail));
                }
                None
            } else {
                open.popover
                    .contentViewController()
                    .map(|c| (open.popover.clone(), c.view()))
            }
        };
        if let Some((popover, content)) = rebuild {
            for sub in content.subviews().iter() {
                sub.removeFromSuperview();
            }
            let (size, live) = self.fill_list_view(&content, &list);
            popover.setContentSize(size);
            if let Some(open) = self.upload_list().borrow_mut().as_mut() {
                open.shape = shape_of(&list);
                open.live = live;
            }
        }
    }

    /// Popover'ın içeriğini `content`'e yerleştirir: başlık, kalem başına
    /// bir satır, ayraç ve `Cancel all ⌘.`. Döner: içeriğin boyu ve akan
    /// kalemlerin tazelenecek görünümleri (çubuk, ayrıntı).
    ///
    /// Yerleşim elle ve yukarıdan aşağı (view çevrilmemiş; y en sonda
    /// tabandan hesaplanıyor): satır sayısı küçük ve yükseklik hâlden belli.
    fn fill_list_view(&self, content: &NSView, list: &UploadList) -> (NSSize, Vec<Live>) {
        let mtm = self.mtm();
        let target: &AnyObject = self.as_ref();
        let left = LIST_WIDTH - 2.0 * LIST_PAD - ROW_BUTTON_WIDTH - LIST_PAD;
        let mut placed: Vec<(Retained<NSView>, NSRect)> = Vec::new();
        let mut live = Vec::new();
        let mut top = LIST_PAD;
        let mut place = |view: Retained<NSView>, x: f64, top: f64, w: f64, h: f64| {
            placed.push((view, NSRect::new(NSPoint::new(x, top), NSSize::new(w, h))));
        };

        let title = label(mtm, &list.title, 12.0, &NSColor::secondaryLabelColor());
        title.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
        place(
            Retained::into_super(Retained::into_super(title)),
            LIST_PAD,
            top,
            LIST_WIDTH - 2.0 * LIST_PAD,
            LINE_HEIGHT,
        );
        top += LINE_HEIGHT + ROW_GAP;

        for row in &list.rows {
            let row_top = top;
            let name = label(mtm, &row.name, 13.0, &NSColor::labelColor());
            name.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
            place(
                Retained::into_super(Retained::into_super(name)),
                LIST_PAD,
                top,
                left,
                NAME_HEIGHT,
            );
            top += NAME_HEIGHT;
            let detail = match &row.status {
                RowStatus::Running { fraction, detail } => {
                    let bar = NSProgressIndicator::initWithFrame(
                        NSProgressIndicator::alloc(mtm),
                        NSRect::ZERO,
                    );
                    bar.setStyle(NSProgressIndicatorStyle::Bar);
                    bar.setIndeterminate(false);
                    bar.setControlSize(NSControlSize::Small);
                    bar.setMinValue(0.0);
                    bar.setMaxValue(1.0);
                    bar.setDoubleValue(*fraction);
                    place(
                        Retained::into_super(bar.clone()),
                        LIST_PAD,
                        top + 2.0,
                        left,
                        BAR_HEIGHT,
                    );
                    top += BAR_HEIGHT + 4.0;
                    let text = label(mtm, detail, 11.0, &NSColor::secondaryLabelColor());
                    live.push((bar, text.clone()));
                    text
                }
                RowStatus::Waiting(detail) => {
                    label(mtm, detail, 11.0, &NSColor::secondaryLabelColor())
                }
                RowStatus::Done(detail) => label(mtm, detail, 11.0, &NSColor::systemGreenColor()),
            };
            place(
                Retained::into_super(Retained::into_super(detail)),
                LIST_PAD,
                top,
                left,
                LINE_HEIGHT,
            );
            top += LINE_HEIGHT;
            let dest = label(mtm, &row.dest, 11.0, &NSColor::tertiaryLabelColor());
            dest.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
            place(
                Retained::into_super(Retained::into_super(dest)),
                LIST_PAD,
                top,
                left,
                LINE_HEIGHT,
            );
            top += LINE_HEIGHT;
            if let Some(action) = row.status.action() {
                let title = match action {
                    RowAction::Cancel => ns_string!("Cancel"),
                    RowAction::Remove => ns_string!("Remove"),
                };
                // SAFETY: seçici bu sınıfın `uploadRowAction:`'ı ve tek
                // `Option<&AnyObject>` alıyor; hedef bu pencere nesnesi ve
                // pencere listesinde yaşıyor.
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        title,
                        Some(target),
                        Some(sel!(uploadRowAction:)),
                        mtm,
                    )
                };
                button.setControlSize(NSControlSize::Small);
                // audit: kimlik sayaç; `isize`'a sığar (kalem sayısı küçük).
                button.setTag(row.id as isize);
                button.sizeToFit();
                let size = button.frame().size;
                let width = size.width.max(ROW_BUTTON_WIDTH);
                // Dikeyde satırın ortasında.
                let middle = row_top + (top - row_top - size.height) / 2.0;
                place(
                    Retained::into_super(Retained::into_super(button)),
                    LIST_WIDTH - LIST_PAD - width,
                    middle,
                    width,
                    size.height,
                );
            }
            top += ROW_GAP;
        }

        let rule = NSBox::new(mtm);
        rule.setBoxType(NSBoxType::Separator);
        place(
            Retained::into_super(rule),
            LIST_PAD,
            top,
            LIST_WIDTH - 2.0 * LIST_PAD,
            1.0,
        );
        top += 1.0 + ROW_GAP;
        // SAFETY: seçici bu sınıfın `cancelUpload:`'ı; hedefin gerekçesi
        // yukarıdaki.
        let all = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Cancel all ⌘."),
                Some(target),
                Some(sel!(cancelUpload:)),
                mtm,
            )
        };
        all.setControlSize(NSControlSize::Small);
        all.sizeToFit();
        let size = all.frame().size;
        place(
            Retained::into_super(Retained::into_super(all)),
            LIST_WIDTH - LIST_PAD - size.width,
            top,
            size.width,
            size.height,
        );
        top += size.height + LIST_PAD;

        // View çevrilmemiş: yukarıdan ölçülen `top` tabandan `y`'ye.
        let height = top;
        for (view, frame) in placed {
            view.setFrame(NSRect::new(
                NSPoint::new(frame.origin.x, height - frame.origin.y - frame.size.height),
                frame.size,
            ));
            content.addSubview(&view);
        }
        let size = NSSize::new(LIST_WIDTH, height);
        content.setFrameSize(size);
        (size, live)
    }
}

/// Esc'in tuş kodu (ANSI düzeninden bağımsız, donanım kodu).
const ESCAPE: u16 = 53;

/// Popover'ın genişliği, iç payı, satırlar arası boşluk ve satır
/// düğmesinin en az genişliği — **tasarım sabitleri**, onaylanan demonun
/// ölçüleri (pt).
const LIST_WIDTH: f64 = 340.0;
const LIST_PAD: f64 = 12.0;
const ROW_GAP: f64 = 10.0;
const ROW_BUTTON_WIDTH: f64 = 70.0;
/// Satırların yüksekliği: ad (13 pt), küçük satırlar (11–12 pt) ve çubuk.
const NAME_HEIGHT: f64 = 18.0;
const LINE_HEIGHT: f64 = 15.0;
const BAR_HEIGHT: f64 = 10.0;

/// Akan bir satırın tazelenen görünümleri: çubuk ve ayrıntı.
type Live = (Retained<NSProgressIndicator>, Retained<NSTextField>);

/// Açık "Show files (N)" popover'ı ve tazeleme için tuttukları.
pub(crate) struct UploadPopover {
    popover: Retained<NSPopover>,
    /// Kalemlerin kimliği ve hâli: değişirse içerik yeniden kuruluyor.
    shape: Vec<(u64, u8)>,
    /// Akan kalemlerin çubuğu ve ayrıntısı, satır sırasıyla.
    live: Vec<Live>,
    /// Esc izleyicisi (kapanışta kaldırılıyor).
    monitor: Option<Retained<AnyObject>>,
}

/// Açık durdurma sorusu: sayfa, hangi kalem için sorulduğu ve Esc
/// izleyicisi.
pub(crate) struct StopSheet {
    alert: Retained<NSAlert>,
    id: u64,
    monitor: Option<Retained<AnyObject>>,
}

/// Listenin şekli: kalemlerin kimliği ve hâli (akan 0, bekleyen 1, biten 2).
fn shape_of(list: &UploadList) -> Vec<(u64, u8)> {
    list.rows
        .iter()
        .map(|row| {
            let kind = match row.status {
                RowStatus::Running { .. } => 0,
                RowStatus::Waiting(_) => 1,
                RowStatus::Done(_) => 2,
            };
            (row.id, kind)
        })
        .collect()
}

/// Düz bir etiket: tek satır, seçilemez.
fn label(mtm: MainThreadMarker, text: &str, size: f64, color: &NSColor) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    // SAFETY: `NSFontWeightRegular` AppKit'in sabit bir global'i.
    let weight = unsafe { NSFontWeightRegular };
    field.setFont(Some(&NSFont::monospacedDigitSystemFontOfSize_weight(
        size, weight,
    )));
    field.setTextColor(Some(color));
    field
}

/// Yerel bir tuş izleyicisi kurar: `swallow` `true` dönerse olay yutulur.
/// Ana thread'de, `NSApp.sendEvent:`'ten önce koşuyor.
fn add_key_monitor(swallow: impl Fn(&NSEvent) -> bool + 'static) -> Option<Retained<AnyObject>> {
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit izleyiciye geçerli bir olay veriyor.
        let event_ref = unsafe { event.as_ref() };
        if swallow(event_ref) {
            std::ptr::null_mut()
        } else {
            event.as_ptr()
        }
    });
    // SAFETY: blok geçerli bir olay işaretçisi ya da null döndürüyor.
    unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::KeyDown, &block) }
}

/// [`add_key_monitor`]'ün izleyicisini kaldırır.
fn remove_monitor(monitor: Option<Retained<AnyObject>>) {
    if let Some(monitor) = monitor {
        // SAFETY: nesne `addLocalMonitor…`'ün döndürdüğü izleyici ve bir kez
        // kaldırılıyor (sahibi yuvadan alınıyor).
        unsafe { NSEvent::removeMonitor(&monitor) };
    }
}

/// bateri arkadayken macOS bildirimi (037 phase-7): kuyruk bitti, hata verdi
/// ya da bağlantı koptu. Önde iken yok — sonuç dock'ta ve başlıkta.
///
/// `NSUserNotification` (Foundation, varsayılan bayrak seti — yeni crate
/// yok): yerine geçen `UserNotifications` çerçevesi ayrı bir crate
/// (`objc2-user-notifications`) ve o bir bağımlılık kararı. API macOS 11'den
/// beri kullanımdan kalkmış sayılıyor; paketlenmemiş süreçte (`cargo run`)
/// merkez `nil` dönüyor, o yüzden paket kimliği yoksa hiç çağrılmıyor.
#[allow(deprecated)]
fn notify(mtm: MainThreadMarker, title: &str, body: &str) {
    if NSApplication::sharedApplication(mtm).isActive() {
        return;
    }
    if NSBundle::mainBundle().bundleIdentifier().is_none() {
        return;
    }
    let notification = NSUserNotification::new();
    notification.setTitle(Some(&NSString::from_str(title)));
    notification.setInformativeText(Some(&NSString::from_str(body)));
    NSUserNotificationCenter::defaultUserNotificationCenter().deliverNotification(&notification);
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
