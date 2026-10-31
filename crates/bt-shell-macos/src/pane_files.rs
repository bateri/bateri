//! The files a program's view is given by the person, and the panels it asks for: what the
//! pane does for a view with files (pencere interface 2.1 for a script).
//!
//! **Only such a view takes them.** Files let go over its page, pasted into it or chosen in the
//! Open panel it asked for become handles the session made ([`entries_given`]) and are told to
//! the page with [`Page::give`]; the program reads a file by its handle, in pieces, on the
//! thread of the view's files ([`ViewFiles`]). A pane with no view is what it was: the paths
//! are told, or written to the line, or uploaded ([`native_view::drop_way`]). The path of a file
//! is told to the page only where the program reads the same files as the person: a local
//! session.
//!
//! **A panel is a sheet of the pane's**, on its seat ([`sheets::seat`]): one at a time, the
//! session holds the page to that ([`Session::module_pick`]), and a pane with no window to
//! hold a sheet turns the request away the way the person's Cancel does. What the person chose
//! comes back to the page and then to the session, which may be asked for another.
//!
//! [`Page::give`]: bt_shell_common::native_view::Page::give

use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{Arc, Weak};

use block2::RcBlock;
use bt_shell_common::native_view::{
    FileSource, Panel, Point as PagePoint, ViewFiles, entries_given, place_saved,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2::{MainThreadMarker, MainThreadOnly, msg_send};
use objc2_app_kit::{NSModalResponse, NSModalResponseOK, NSOpenPanel, NSSavePanel};
use objc2_foundation::{NSArray, NSString, NSURL};

use crate::pane::TerminalPane;
use crate::sheets::{self, Asker};

/// The type of a file that has none the system knows.
const UNKNOWN_TYPE: &str = "application/octet-stream";

/// The media type of the file at `path`, from its extension (the system's table of types), as
/// the guess a page is told. `application/octet-stream` where the system knows none.
pub(crate) fn media_type_of(path: &Path) -> String {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return UNKNOWN_TYPE.to_owned();
    };
    let Some(class) = AnyClass::get(c"UTType") else {
        return UNKNOWN_TYPE.to_owned();
    };
    // SAFETY: `+[UTType typeWithFilenameExtension:]` takes an `NSString` and returns a
    // nullable `UTType`.
    let kind: Option<Retained<AnyObject>> =
        unsafe { msg_send![class, typeWithFilenameExtension: &*NSString::from_str(extension)] };
    let Some(kind) = kind else {
        return UNKNOWN_TYPE.to_owned();
    };
    // SAFETY: `-[UTType preferredMIMEType]` returns a nullable `NSString`.
    let mime: Option<Retained<NSString>> = unsafe { msg_send![&*kind, preferredMIMEType] };
    mime.map_or_else(|| UNKNOWN_TYPE.to_owned(), |mime| mime.to_string())
}

/// The file-system path of `url`, if it is a file URL.
fn path_of(url: &NSURL) -> Option<PathBuf> {
    if !url.isFileURL() {
        return None;
    }
    url.path().map(|path| PathBuf::from(path.to_string()))
}

impl TerminalPane {
    /// What takes the files let go over the pane (`native_view::drop_way`'s question): `None`
    /// where no page is up, and otherwise whether the view that page is has files.
    pub(crate) fn files_mode(&self) -> Option<bool> {
        self.page_is_up().then(|| self.view_files().is_some())
    }

    /// `count` files are dragged over the page at `at` (the view's points): a page whose view has
    /// files is told, so that it can show where they would land.
    pub(crate) fn files_drag_over(&self, at: PagePoint, count: usize) {
        if self.files_mode() != Some(true) {
            return;
        }
        if let Some(handle) = self.page_handle() {
            handle.page().files_over(at, count);
            self.drain_page();
        }
    }

    /// The files dragged over the page went from it without being let go.
    pub(crate) fn files_drag_left(&self) {
        if self.files_mode() != Some(true) {
            return;
        }
        if let Some(handle) = self.page_handle() {
            handle.page().files_left();
            self.drain_page();
        }
    }

    /// `paths` were let go on the page at `at`: it is given them as files, each with the handle
    /// the session made of it. `false` when nothing could be given (a view that ended, files that
    /// are gone, no room for another handle), so that the drop is turned away.
    pub(crate) fn give_dropped(&self, at: PagePoint, paths: &[String]) -> bool {
        self.give_files(at, FileSource::Drop, paths)
    }

    /// `paths` were pasted into the page: it is given them as files, at its corner. `true`
    /// when the pasteboard's files were the page's (even if none could be given), so that their
    /// names are not pasted as text.
    pub(crate) fn give_pasted(&self, paths: &[String]) -> bool {
        let corner = PagePoint::new(0.0, 0.0);
        if !self.give_files(corner, FileSource::Paste, paths)
            && let Some(handle) = self.page_handle()
        {
            // None could be given (files that are gone, no room for another handle): the page
            // hears a paste with no files, as it hears the Open panel's Cancel, so that it can
            // say so instead of the person's ⌘V doing nothing at all.
            handle.page().give(corner, FileSource::Paste, Vec::new());
            self.drain_page();
        }
        true
    }

    /// A picture with no file of its own was pasted into the page (a screenshot): it is kept in
    /// the folder of the view's files as `image.png` and given as a file with no path. `false`
    /// when it could not be kept or given.
    pub(crate) fn give_pasted_image(&self, png: &[u8]) -> bool {
        let (Some(session), Some(files), Some(handle)) =
            (self.session(), self.view_files(), self.page_handle())
        else {
            return false;
        };
        let Ok(path) = files.scratch("image.png", png) else {
            return false;
        };
        let entries = entries_given(session, &files, &[path], false, &|_| "image/png".to_owned());
        if entries.is_empty() {
            return false;
        }
        handle
            .page()
            .give(PagePoint::new(0.0, 0.0), FileSource::Paste, entries);
        self.drain_page();
        true
    }

    fn give_files(&self, at: PagePoint, source: FileSource, paths: &[String]) -> bool {
        let (Some(session), Some(files), Some(handle)) =
            (self.session(), self.view_files(), self.page_handle())
        else {
            return false;
        };
        // The program reads the person's files only where it runs beside them.
        let local = session.remote_target().is_none();
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        let entries = entries_given(session, &files, &paths, local, &media_type_of);
        if entries.is_empty() {
            return false;
        }
        handle.page().give(at, source, entries);
        self.drain_page();
        true
    }

    /// The page asked for a panel (`Host::panel`): it goes to the session, which holds the page
    /// to one at a time and says when to show it (`Action::Pick`, `Action::Save`).
    pub(crate) fn ask_panel(&self, panel: Panel) {
        if self.files_mode() != Some(true) {
            return;
        }
        let Some(session) = self.session() else {
            return;
        };
        match panel {
            Panel::Pick { multiple } => session.module_pick(multiple),
            Panel::Save { name, media_type } => session.module_save(&name, media_type.as_deref()),
        }
    }

    /// The view with files that is up, held without keeping it: what a panel is asked for by,
    /// and what its answer is given to only if it is still up.
    fn asker(&self) -> Weak<ViewFiles> {
        self.view_files()
            .map_or_else(Weak::new, |files| Arc::downgrade(&files))
    }

    /// Whether the view with files that is up now is the one `asker` stands for. A view that
    /// ended while its panel was open, and another that opened in its place, is not given
    /// what the person chose for the first.
    fn is_asker(&self, asker: &Weak<ViewFiles>) -> bool {
        self.view_files()
            .zip(asker.upgrade())
            .is_some_and(|(now, then)| Arc::ptr_eq(&now, &then))
    }

    /// The seat a panel is shown on, if there is one that is free.
    fn panel_seat(&self) -> Option<sheets::Seat> {
        sheets::seat(Asker::Pane(self)).filter(|seat| !seat.is_taken())
    }

    /// The Open panel the session said to show (`Action::Pick`): files only, one or more as
    /// the page asked. The person's choice is told to the page, and the session that the panel
    /// is gone.
    pub(crate) fn show_pick(&self, panel: u32, multiple: bool) {
        // The view that asked may be gone by now: no panel for it.
        let asker = self.asker();
        let seat = self.is_asker(&asker).then(|| self.panel_seat()).flatten();
        let Some(seat) = seat else {
            self.pick_chosen(panel, Vec::new(), &asker);
            return;
        };
        let open = NSOpenPanel::openPanel(self.mtm());
        open.setCanChooseFiles(true);
        open.setCanChooseDirectories(false);
        open.setAllowsMultipleSelection(multiple);
        // The pane by id: the panel's block must not pin the pane.
        let (id, lookup) = (self.id(), self.lookup());
        let chosen = open.clone();
        let answered = RcBlock::new(move |response: NSModalResponse| {
            let paths: Vec<String> = if response == NSModalResponseOK {
                chosen
                    .URLs()
                    .iter()
                    .filter_map(|url| path_of(&url))
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect()
            } else {
                Vec::new()
            };
            // audit: the panel's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the panel block is on the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.pick_chosen(panel, paths, &asker);
            }
        });
        seat.begin_panel(&open, &answered);
    }

    /// What the person chose in the Open panel (none: they turned it away) is told to the page,
    /// and then the session that the panel is gone.
    fn pick_chosen(&self, panel: u32, paths: Vec<String>, asker: &Weak<ViewFiles>) {
        if self.is_asker(asker) {
            let corner = PagePoint::new(0.0, 0.0);
            let given = !paths.is_empty() && self.give_files(corner, FileSource::Pick, &paths);
            if !given && let Some(handle) = self.page_handle() {
                // Turned away, or nothing could be given of what was chosen: the page hears
                // it as it hears the person's Cancel, with no files.
                handle.page().give(corner, FileSource::Pick, Vec::new());
                self.drain_page();
            }
        }
        if let Some(session) = self.session() {
            session.panel_closed(panel);
        }
    }

    /// The Save panel the session said to show (`Action::Save`), proposing `name`, in the
    /// folder macOS last had the person in. The place chosen is told to the page, and the
    /// session that the panel is gone.
    pub(crate) fn show_save(&self, panel: u32, name: &str) {
        let asker = self.asker();
        let seat = self.is_asker(&asker).then(|| self.panel_seat()).flatten();
        let Some(seat) = seat else {
            self.save_chosen(panel, None, &asker);
            return;
        };
        let save = NSSavePanel::savePanel(self.mtm());
        save.setNameFieldStringValue(&NSString::from_str(name));
        save.setCanCreateDirectories(true);
        let (id, lookup) = (self.id(), self.lookup());
        let chosen = save.clone();
        let answered = RcBlock::new(move |response: NSModalResponse| {
            let target = if response == NSModalResponseOK {
                chosen.URL().and_then(|url| path_of(&url))
            } else {
                None
            };
            // audit: the panel's completion block runs on AppKit's main thread.
            let mtm = MainThreadMarker::new().expect("the panel block is on the main thread");
            if let Some(pane) = lookup(mtm, id) {
                pane.save_chosen(panel, target, &asker);
            }
        });
        seat.begin_panel(&save, &answered);
    }

    fn save_chosen(&self, panel: u32, target: Option<PathBuf>, asker: &Weak<ViewFiles>) {
        if self.is_asker(asker)
            && let (Some(session), Some(files), Some(handle)) =
                (self.session(), self.view_files(), self.page_handle())
        {
            let saving = target.and_then(|target| place_saved(session, &files, target));
            handle.page().saving(saving);
            self.drain_page();
        }
        if let Some(session) = self.session() {
            session.panel_closed(panel);
        }
    }

    /// The panel a web view's file input asks for (`runOpenPanelWithParameters`): the Open panel
    /// as a sheet of the pane's, files only. `answer` is called once, with the URLs chosen or
    /// null where the person turned it away or there is no seat to show it on.
    pub(crate) fn web_open_panel(
        &self,
        multiple: bool,
        answer: RcBlock<dyn Fn(*mut NSArray<NSURL>)>,
    ) {
        let Some(seat) = self.panel_seat() else {
            answer.call((ptr::null_mut(),));
            return;
        };
        let open = NSOpenPanel::openPanel(self.mtm());
        open.setCanChooseFiles(true);
        open.setCanChooseDirectories(false);
        open.setAllowsMultipleSelection(multiple);
        let chosen = open.clone();
        let answered = RcBlock::new(move |response: NSModalResponse| {
            if response == NSModalResponseOK {
                let urls = chosen.URLs();
                answer.call((Retained::as_ptr(&urls).cast_mut(),));
            } else {
                answer.call((ptr::null_mut(),));
            }
        });
        seat.begin_panel(&open, &answered);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The media type a page is told of a file is the system's guess from its extension, and
    /// `application/octet-stream` where the system has none (or the file has no extension).
    #[test]
    fn a_files_media_type_is_the_systems_guess_from_its_extension() {
        assert_eq!(media_type_of(Path::new("/tmp/a.png")), "image/png");
        assert_eq!(media_type_of(Path::new("notes.txt")), "text/plain");
        assert_eq!(media_type_of(Path::new("/tmp/no-extension")), UNKNOWN_TYPE);
        assert_eq!(
            media_type_of(Path::new("a.no-such-extension-xyz")),
            UNKNOWN_TYPE
        );
    }

    /// The wiring a window server and a program's view would be needed to see, read from the
    /// sources: who is given files and who is not, where the panels are shown, and what the
    /// drop and the paste do first.
    #[test]
    fn the_files_of_a_view_are_wired_where_the_panes_other_parts_read_them() {
        let pane = include_str!("pane.rs");
        let pane = &pane[..pane.find("#[cfg(test)]").unwrap_or(pane.len())];
        let body = |source: &'static str, signature: &str| -> &'static str {
            let at = source.find(signature).expect(signature);
            &source[at..at + source[at..].find("\n    }\n").expect("its end")]
        };
        // Only a view whose interface has files is given the person's files.
        let start = body(pane, "    fn start_native_view(");
        assert!(
            start.contains("has_files(start.kind, &start.version)"),
            "{start}"
        );
        assert!(
            start.contains("files,\n"),
            "the page's files are the optional ones"
        );
        // The panels are the pane's, on its seat, and no longer turned away by the drain.
        let views = body(pane, "    pub(crate) fn drain_views(");
        assert!(views.contains("self.show_pick(panel, multiple)"), "{views}");
        assert!(views.contains("self.show_save(panel, &name)"), "{views}");
        assert!(!views.contains("panel_closed("), "{views}");
        let host = &pane[pane
            .find("impl PageDriveHost for PageHost")
            .expect("the host")..];
        let host = &host[..host.find("\n}\n").expect("its end")];
        assert!(
            host.contains("fn panel(&mut self, panel: PagePanel)"),
            "{host}"
        );
        assert!(host.contains("self.0.ask_panel(panel)"), "{host}");
        // A request for a panel is the session's to hold to one at a time, and only for a view
        // with files; the seat is the pane's.
        let this = include_str!("pane_files.rs");
        let this = &this[..this.find("#[cfg(test)]").expect("the tests")];
        let ask = body(this, "    pub(crate) fn ask_panel(");
        assert!(ask.contains("self.files_mode() != Some(true)"), "{ask}");
        assert!(ask.contains("session.module_pick(multiple)"), "{ask}");
        assert!(ask.contains("session.module_save("), "{ask}");
        assert!(this.contains("sheets::seat(Asker::Pane(self))"));
        // Pasted files none of which could be given still reach the page, with no files.
        let pasted = body(this, "    pub(crate) fn give_pasted(");
        assert!(
            pasted.contains("give(corner, FileSource::Paste, Vec::new())"),
            "{pasted}"
        );
        // What the person chose goes to the view that asked, and to no other that is up by then.
        for signature in [
            "    fn pick_chosen(",
            "    fn save_chosen(",
            "    pub(crate) fn show_pick(",
            "    pub(crate) fn show_save(",
        ] {
            let chosen = body(this, signature);
            assert!(chosen.contains("is_asker("), "{chosen}");
        }
        assert_eq!(
            this.matches("begin_panel(").count(),
            3,
            "pick, save and the web view's"
        );
        // The drop and the paste: the way is decided in one place, and a view's files come
        // before the line's text.
        let view = include_str!("view.rs");
        let view = &view[..view.find("#[cfg(test)]").unwrap_or(view.len())];
        let perform = body(view, "        fn perform_drag_operation(");
        assert!(
            perform.contains("drop_way(pane.as_ref().and_then(|pane| pane.files_mode()), remote)")
        );
        for way in ["Handles", "Paths", "Upload", "Paste"] {
            assert!(
                perform.contains(&format!("DropWay::{way}")),
                "{way}: {perform}"
            );
        }
        assert!(
            view.contains("fn dragging_updated("),
            "the files over a page are told at each move"
        );
        let paste = body(view, "        fn paste_clipboard(");
        assert!(
            paste.find("self.paste_files(").expect("files first")
                < paste.find("page_editor_menu('v')").expect("the editor"),
            "{paste}"
        );
        // A file input of a web page is the pane's Open panel.
        let web = include_str!("pencere_view.rs");
        assert!(
            web.contains("webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:")
        );
        assert!(web.contains("pane.web_open_panel(multiple, handler)"));
    }
}
