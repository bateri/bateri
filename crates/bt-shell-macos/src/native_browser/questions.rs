//! What the engine asks of the person: a context menu, a dialog of the web page, files, a download.
//! One at a time, in the order they came; each is answered once, whatever happens to the pane or
//! the page meanwhile (a question that is dropped says "no": [`bt_shell_common::browser::rooms`]).
//!
//! A menu is AppKit's own pop-up and runs the main queue while it is up. The rest are sheets of
//! the pane's tab, through the sheet gate ([`crate::sheets`]), and wait their turn when another
//! sheet is up. Nothing of a download is written before the person has said where: the question
//! names the file (as safe to write as it can be made), the size and the site, and the folder is
//! the one of the person's `download_dir` setting.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use block2::RcBlock;
use browser_ipc::message::{DialogKind, FileMode};
use bt_shell_common::browser::downloads;
use bt_shell_common::browser::rooms::{Dialog, Download, Files, Menu};
use bt_shell_common::native_view::Id;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSMenu, NSMenuItem, NSModalResponse, NSModalResponseOK,
    NSOpenPanel, NSSavePanel, NSTextField,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint, NSString, NSURL, ns_string};

use super::PageBrowsers;
use crate::pane::TerminalPane;
use crate::sheets::{self, Asker};

/// The response a dialog's sheet ends with when its dialog is void: a code of ours, none of
/// AppKit's, which says that nobody is to be answered.
const VOID: NSModalResponse = -9001;

pub(super) enum Question {
    Menu(Id, Menu),
    Dialog(Dialog),
    Files(Files),
    Download(Download),
}

define_class!(
    // SAFETY: NSObject is designed for subclassing; this has no `Drop` and one ivar of no
    // interest to the superclass.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriBrowserMenuTarget"]
    #[ivars = Cell<Option<isize>>]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        /// A line of the engine's menu was picked: its command is the item's tag.
        #[unsafe(method(picked:))]
        fn picked(&self, sender: &NSMenuItem) {
            self.ivars().set(Some(sender.tag()));
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Cell::new(None));
        // SAFETY: `init` is NSObject's designated initializer.
        unsafe { msg_send![super(this), init] }
    }
}

impl PageBrowsers {
    /// A question of the engine: asked now, or when the one before it is answered.
    pub(super) fn ask(&self, pane: &TerminalPane, question: Question) {
        self.questions.borrow_mut().push_back(question);
        self.present(pane);
    }

    /// The next question, if none is up.
    pub(super) fn present(&self, pane: &TerminalPane) {
        loop {
            if self.asking.get() {
                return;
            }
            let Some(question) = self.questions.borrow_mut().pop_front() else {
                return;
            };
            match question {
                Question::Menu(id, menu) => self.pop_menu(pane, id, menu),
                Question::Dialog(dialog) => {
                    if let Some(dialog) = self.sheet_for(pane, dialog, Question::Dialog) {
                        self.show_dialog(pane, dialog);
                    }
                }
                Question::Files(files) => {
                    if let Some(files) = self.sheet_for(pane, files, Question::Files) {
                        self.show_files(pane, files);
                    }
                }
                Question::Download(download) => {
                    if let Some(download) = self.sheet_for(pane, download, Question::Download) {
                        self.show_download(pane, download);
                    }
                }
            }
        }
    }

    /// The question back if a sheet can open for it now. If another sheet is up it goes back in the
    /// queue and waits: the engine waits (a web page's dialog blocks it), and the question is
    /// asked again when a sheet ends ([`Self::seat_may_be_free`], which `sheets::open_parked`
    /// calls where the end of a sheet is heard).
    fn sheet_for<T>(
        &self,
        pane: &TerminalPane,
        question: T,
        wrap: impl FnOnce(T) -> Question,
    ) -> Option<T> {
        if sheets::seat(Asker::Pane(pane)).is_some_and(|seat| !seat.is_taken()) {
            return Some(question);
        }
        self.questions.borrow_mut().push_front(wrap(question));
        self.asking.set(true);
        self.seat_wait.set(true);
        None
    }

    /// A sheet ended where the pane's questions sit: a question that waited for the seat is asked
    /// if the seat is free now.
    pub(crate) fn seat_may_be_free(&self, pane: &TerminalPane) {
        if !self.seat_wait.get()
            || sheets::seat(Asker::Pane(pane)).is_some_and(|seat| seat.is_taken())
        {
            return;
        }
        self.seat_wait.set(false);
        self.asking.set(false);
        self.present(pane);
    }

    /// The page moved on while its dialog was up: the dialog numbered `req` is taken down and is
    /// answered by nobody, whether it is still in the line, a sheet that is open or one parked
    /// behind another. One that was answered already is none of these, and nothing is done.
    pub(super) fn dialog_void(&self, pane: &TerminalPane, req: u32) {
        let queued = {
            let mut questions = self.questions.borrow_mut();
            let at = questions.iter().position(
                |question| matches!(question, Question::Dialog(dialog) if dialog.req() == req),
            );
            at.and_then(|at| questions.remove(at))
        };
        if let Some(Question::Dialog(dialog)) = queued {
            dialog.void();
            return;
        }
        let alert = self
            .dialogs
            .borrow()
            .iter()
            .find(|(number, _)| *number == req)
            .map(|(_, alert)| alert.clone());
        // Ending the sheet runs its completion block, which finds the code and says nothing; a
        // parked one is ended the same way without ever opening.
        if let Some(alert) = alert
            && let Some(seat) = sheets::seat(Asker::Pane(pane))
        {
            seat.end(&alert.window(), VOID);
        }
    }

    /// A sheet was answered: the next question may be asked.
    fn answered(pane: &TerminalPane) {
        if let Some(browsers) = pane.page_browsers() {
            browsers.asking.set(false);
            browsers.present(pane);
        }
    }

    // --- The menu ----------------------------------------------------------------------------

    fn pop_menu(&self, pane: &TerminalPane, room: Id, menu: Menu) {
        let Some(origin) = pane.page_handle().and_then(|handle| {
            handle
                .page()
                .browsers()
                .iter()
                .find(|browser| browser.id == room)
                .map(|browser| (browser.rect.x, browser.rect.y))
        }) else {
            return;
        };
        let mtm = pane.mtm();
        let target = MenuTarget::new(mtm);
        let shown = NSMenu::new(mtm);
        shown.setAutoenablesItems(false);
        for line in &menu.items {
            if line.separator {
                shown.addItem(&NSMenuItem::separatorItem(mtm));
                continue;
            }
            // SAFETY: `initWithTitle:action:keyEquivalent:` takes two `NSString`s and a selector
            // `MenuTarget` implements; `setTarget:` keeps the target weakly and it outlives the
            // menu's tracking.
            let item = unsafe {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(&line.label),
                    Some(sel!(picked:)),
                    ns_string!(""),
                );
                item.setTarget(Some(&target));
                item
            };
            item.setTag(isize::try_from(line.command).unwrap_or(isize::MAX));
            item.setEnabled(line.enabled);
            if line.checked {
                item.setState(objc2_app_kit::NSControlStateValueOn);
            }
            shown.addItem(&item);
        }
        let at = NSPoint::new(
            f64::from(origin.0) + f64::from(menu.x),
            f64::from(origin.1) + f64::from(menu.y),
        );
        // Another question may not come while it is up: tracking runs the main queue.
        self.asking.set(true);
        shown.popUpMenuPositioningItem_atLocation_inView(None, at, Some(pane.view()));
        self.asking.set(false);
        let picked = target.ivars().get().and_then(|tag| i32::try_from(tag).ok());
        menu.pick(picked);
    }

    // --- A dialog of the web page --------------------------------------------------------------

    fn show_dialog(&self, pane: &TerminalPane, dialog: Dialog) {
        let Some(seat) = sheets::seat(Asker::Pane(pane)) else {
            return;
        };
        let mtm = pane.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&dialog.origin_line()));
        alert.setInformativeText(&NSString::from_str(&dialog.message));
        let prompt = dialog.kind == DialogKind::Prompt;
        let field = prompt.then(|| {
            let field =
                NSTextField::textFieldWithString(&NSString::from_str(&dialog.default_text), mtm);
            field.setFrameSize(objc2_foundation::NSSize::new(260.0, 22.0));
            alert.setAccessoryView(Some(&field));
            alert.window().setInitialFirstResponder(Some(&field));
            field
        });
        let (ok, cancel) = match dialog.kind {
            DialogKind::Alert => ("OK", None),
            DialogKind::Confirm | DialogKind::Prompt => ("OK", Some("Cancel")),
            DialogKind::BeforeUnload => ("Leave", Some("Stay")),
        };
        alert.addButtonWithTitle(&NSString::from_str(ok));
        if let Some(cancel) = cancel {
            let button = alert.addButtonWithTitle(&NSString::from_str(cancel));
            button.setKeyEquivalent(ns_string!("\u{1b}"));
        }
        self.asking.set(true);
        let (id, lookup) = (pane.id(), pane.lookup());
        let req = dialog.req();
        // The block is `Fn`: the dialog is answered once.
        let held = RefCell::new(Some((dialog, field)));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            let Some((dialog, field)) = held.borrow_mut().take() else {
                return;
            };
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            let pane = lookup(mtm, id);
            if let Some(browsers) = pane.as_ref().and_then(|pane| pane.page_browsers()) {
                browsers
                    .dialogs
                    .borrow_mut()
                    .retain(|(number, _)| *number != req);
            }
            if response == VOID {
                dialog.void();
            } else {
                let text = field.map(|field| field.stringValue().to_string());
                dialog.answer(
                    response == NSAlertFirstButtonReturn,
                    text.unwrap_or_default(),
                );
            }
            if let Some(pane) = pane {
                Self::answered(&pane);
            }
        });
        self.dialogs.borrow_mut().push((req, alert.clone()));
        seat.begin(&alert, &answered);
    }

    // --- Files -------------------------------------------------------------------------------

    fn show_files(&self, pane: &TerminalPane, files: Files) {
        let Some(seat) = sheets::seat(Asker::Pane(pane)) else {
            return;
        };
        let mtm = pane.mtm();
        let saving = files.mode == FileMode::Save;
        let open = (!saving).then(|| {
            let open = NSOpenPanel::openPanel(mtm);
            let folder = files.mode == FileMode::Folder;
            open.setCanChooseFiles(!folder);
            open.setCanChooseDirectories(folder);
            open.setAllowsMultipleSelection(files.mode == FileMode::OpenMultiple);
            open
        });
        let panel: Retained<NSSavePanel> = match &open {
            Some(open) => Retained::into_super(open.clone()),
            None => NSSavePanel::savePanel(mtm),
        };
        if !files.title.is_empty() {
            panel.setMessage(Some(&NSString::from_str(&files.title)));
        }
        let default = Path::new(&files.default_path);
        if saving {
            panel.setCanCreateDirectories(true);
            if let Some(name) = default.file_name() {
                panel.setNameFieldStringValue(&NSString::from_str(&name.to_string_lossy()));
            }
        }
        // Where the panel starts: the folder the page names, if there is one.
        let start = if default.is_dir() {
            Some(default)
        } else {
            default.parent()
        };
        if let Some(start) = start.filter(|path| path.is_dir()) {
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(
                &start.to_string_lossy(),
            ))));
        }
        self.asking.set(true);
        let (id, lookup) = (pane.id(), pane.lookup());
        let chosen = panel.clone();
        let held = RefCell::new(Some(files));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            let Some(files) = held.borrow_mut().take() else {
                return;
            };
            let paths: Vec<PathBuf> = if response == NSModalResponseOK {
                match &open {
                    Some(open) => open.URLs().iter().filter_map(|url| path_of(&url)).collect(),
                    None => chosen
                        .URL()
                        .and_then(|url| path_of(&url))
                        .into_iter()
                        .collect(),
                }
            } else {
                Vec::new()
            };
            files.choose(paths);
            // audit: the panel's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the panel block is on the main thread");
            if let Some(pane) = lookup(mtm, id) {
                Self::answered(&pane);
            }
        });
        seat.begin_panel(&panel, &answered);
    }

    // --- A download --------------------------------------------------------------------------

    fn show_download(&self, pane: &TerminalPane, download: Download) {
        let Some(seat) = sheets::seat(Asker::Pane(pane)) else {
            return;
        };
        let folder = download_folder(pane);
        let name = downloads::safe_name(&download.name);
        let mtm = pane.mtm();
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str(&format!("Save “{name}”?")));
        let mut lines = Vec::new();
        if let Some(site) = site_of(&download.url) {
            lines.push(format!("From {site}."));
        }
        if let Some(size) = download.size {
            lines.push(format!("{}.", size_words(size)));
        }
        lines.push(match &folder {
            Some(folder) => format!("It is saved in {}.", folder.display()),
            None => "The download folder cannot be found.".to_owned(),
        });
        alert.setInformativeText(&NSString::from_str(&lines.join(" ")));
        let save = alert.addButtonWithTitle(ns_string!("Save"));
        save.setEnabled(folder.is_some());
        let cancel = alert.addButtonWithTitle(ns_string!("Cancel"));
        cancel.setKeyEquivalent(ns_string!("\u{1b}"));
        self.asking.set(true);
        let (id, lookup) = (pane.id(), pane.lookup());
        let held = RefCell::new(Some(download));
        let answered = RcBlock::new(move |response: NSModalResponse| {
            let Some(download) = held.borrow_mut().take() else {
                return;
            };
            // Nothing is written unless the person said Save; a download not saved is dropped, and
            // the engine is told it is not wanted.
            if response == NSAlertFirstButtonReturn
                && let Some(folder) = &folder
            {
                let path = downloads::path_in(folder, &download.name, Path::exists);
                download.save(path);
            }
            // audit: the sheet's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the sheet block is on the main thread");
            if let Some(pane) = lookup(mtm, id) {
                Self::answered(&pane);
            }
        });
        seat.begin(&alert, &answered);
    }
}

/// A download came to its end: a whole file carries the mark Gatekeeper asks about before a
/// downloaded program first runs, as for a browser's download.
pub(super) fn downloaded(pane: &TerminalPane, path: &Path, done: bool) {
    if done {
        crate::uploader::quarantine(path, &pane.app_name());
    }
}

/// The folder the person's downloads go to: their `download_dir` setting, which must be there.
fn download_folder(pane: &TerminalPane) -> Option<PathBuf> {
    let setting = pane.remote_files().borrow().download_dir.clone();
    let folder = bt_core::expand_home(&setting, bt_shell_common::child::home().as_deref())?;
    folder.is_dir().then_some(folder)
}

fn path_of(url: &NSURL) -> Option<PathBuf> {
    url.path().map(|path| PathBuf::from(path.to_string()))
}

/// The site of an address, for the question: its host.
fn site_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    (!host.is_empty()).then(|| host.to_owned())
}

/// A size in the words a person reads.
fn size_words(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["bytes", "KB", "MB", "GB"];
    // A size in words keeps three digits: f64 holds a byte count far beyond what a download is.
    #[allow(clippy::cast_precision_loss)]
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1000.0 && unit + 1 < UNITS.len() {
        size /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} bytes")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

trait DialogWords {
    fn origin_line(&self) -> String;
}

impl DialogWords for Dialog {
    /// Whose words they are: the page's site, as a browser's own dialog says.
    fn origin_line(&self) -> String {
        match site_of(&self.origin) {
            Some(site) => format!("The page at {site} says"),
            None => "This page says".to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_is_the_host_of_an_address() {
        assert_eq!(
            site_of("https://example.com/a/b?c#d").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            site_of("http://user:pw@host:8080/x").as_deref(),
            Some("host:8080")
        );
        assert_eq!(site_of("data:text/plain,hi"), None);
        assert_eq!(site_of("https:///x"), None);
    }

    #[test]
    fn a_size_is_in_words() {
        assert_eq!(size_words(0), "0 bytes");
        assert_eq!(size_words(999), "999 bytes");
        assert_eq!(size_words(1_500), "1.5 KB");
        assert_eq!(size_words(250_000_000), "250.0 MB");
        assert_eq!(size_words(3_200_000_000), "3.2 GB");
    }
}
