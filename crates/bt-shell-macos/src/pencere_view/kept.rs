//! The person's pasteboard while an agent's requests drive web views.
//!
//! A web view runs an agent's call as a person's gesture, so a page's Copy button writes the
//! general pasteboard (the only one a web view writes) and nothing in the app is asked. The
//! requests that drive a page therefore keep the pasteboard and put it back if the page wrote it.
//!
//! **One keeping for the process.** Requests on two panes can overlap, and a second one that
//! began after the first page had copied would keep the page's text as the person's. The first
//! request to begin takes what the pasteboard holds; the last to end puts it back, once; a request
//! that begins in between keeps nothing of its own.
//!
//! **What is read is only what may be written back.** The data of every type is read only for an
//! action that can make a page copy (a click, a key, a command, a menu line): reading a lazily
//! provided type (a large image, a spreadsheet range) makes its owner produce it, on the main
//! thread. For the others only the change count is taken, and a change is told, not undone.
//! A pasteboard that holds something concealed or transient (a password manager's, marked by its
//! type) is never read and never written back: writing it again would leave it there for good.
//!
//! **The person's own copy is not undone.** Whatever bateri itself writes ([`copy`]) counts
//! ([`own_writes`]); a pasteboard that bateri wrote meanwhile is left as it is. A copy in another
//! application in the same time is not told apart and is undone with the page's: the time is the
//! request's, at most the call's limit and the page's time to settle.
//!
//! [`copy`]: crate::clipboard::copy
//! [`own_writes`]: crate::clipboard::own_writes

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardWriting};
use objc2_foundation::{NSArray, NSData, NSString};

use crate::clipboard::own_writes;

/// The type a password manager marks what it copies with: not to be kept by clipboard managers.
const CONCEALED: &str = "org.nspasteboard.ConcealedType";

/// The type of what is meant to be gone soon.
const TRANSIENT: &str = "org.nspasteboard.TransientType";

const TEXT: &str = "public.utf8-plain-text";
const PNG: &str = "public.png";
const TIFF: &str = "public.tiff";
const FILE_URL: &str = "public.file-url";

/// What one item of the pasteboard held: each type with its data.
type Item = Vec<(Retained<NSString>, Retained<NSData>)>;

/// What the pasteboard was when the first request began.
struct Entry {
    /// Its change count then.
    count: isize,
    /// How many times bateri wrote it itself by then.
    own: u64,
    /// The requests that have begun and not ended.
    runs: usize,
    /// Every type of every item, read for a request that may make a page copy.
    items: Option<Vec<Item>>,
    /// It held something concealed or transient: not read, and not to be written back.
    concealed: bool,
    /// A type that was listed gave no data: what is written back is not all of what was there.
    partial: bool,
}

thread_local! {
    /// The keeping of the process: the main thread's, where the requests are carried out.
    static ENTRY: RefCell<Option<Entry>> = const { RefCell::new(None) };
}

impl Entry {
    /// Reads what `board` holds, unless it is concealed.
    fn read(&mut self, board: &NSPasteboard) {
        let mut items = Vec::new();
        for item in &board.pasteboardItems().unwrap_or_default() {
            let kinds = item.types();
            if kinds.iter().any(|kind| {
                let kind = kind.to_string();
                kind == CONCEALED || kind == TRANSIENT
            }) {
                self.concealed = true;
                return;
            }
            let mut kept = Vec::new();
            for kind in &kinds {
                match item.dataForType(&kind) {
                    Some(data) => kept.push((kind.clone(), data)),
                    None => self.partial = true,
                }
            }
            items.push(kept);
        }
        self.items = Some(items);
    }

    /// Whether the pasteboard is as the keeping found it.
    fn untouched(&self, board: &NSPasteboard) -> bool {
        board.changeCount() == self.count && own_writes() == self.own
    }
}

/// A request that drives a page begins: the first of them keeps the pasteboard, `data` says
/// whether what it holds is to be read too (the request may make the page copy).
pub(super) fn enter(board: &NSPasteboard, data: bool) {
    ENTRY.with_borrow_mut(|slot| match slot {
        None => {
            let mut entry = Entry {
                count: board.changeCount(),
                own: own_writes(),
                runs: 1,
                items: None,
                concealed: false,
                partial: false,
            };
            if data {
                entry.read(board);
            }
            *slot = Some(entry);
        }
        Some(entry) => {
            entry.runs += 1;
            // What is read after the page wrote would be the page's.
            if data && entry.items.is_none() && !entry.concealed && entry.untouched(board) {
                entry.read(board);
            }
        }
    });
}

/// A request ends. What the person is told, if the page wrote the pasteboard: the last request to
/// end puts it back, and says whether it could.
pub(super) fn leave(board: &NSPasteboard) -> Option<String> {
    ENTRY.with_borrow_mut(|slot| {
        let entry = slot.as_mut()?;
        entry.runs = entry.runs.saturating_sub(1);
        let wrote = board.changeCount() != entry.count && own_writes() == entry.own;
        if entry.runs > 0 {
            return wrote.then(|| {
                format!(
                    "the page wrote {} to the pasteboard: it is put back when the other requests end",
                    kind_of(board)
                )
            });
        }
        let entry = slot.take()?;
        if !wrote {
            return None;
        }
        Some(put_back(board, entry))
    })
}

/// The page wrote the pasteboard and no request is left: what was there is written back where it
/// can be, and what came of it is said.
fn put_back(board: &NSPasteboard, entry: Entry) -> String {
    let wrote = kind_of(board);
    if entry.concealed {
        return format!(
            "the page wrote {wrote} to the pasteboard; what was there is concealed (a password manager's) and was not written back"
        );
    }
    let Some(items) = entry.items else {
        return format!(
            "the page wrote {wrote} to the pasteboard and it was not put back: this request did not keep what was there"
        );
    };
    board.clearContents();
    let items: Vec<Retained<ProtocolObject<dyn NSPasteboardWriting>>> = items
        .iter()
        .map(|kept| {
            let item = NSPasteboardItem::new();
            for (kind, data) in kept {
                item.setData_forType(data, kind);
            }
            ProtocolObject::from_retained(item)
        })
        .collect();
    if !items.is_empty() && !board.writeObjects(&NSArray::from_retained_slice(&items)) {
        return format!(
            "the page wrote {wrote} to the pasteboard and what was there could not be written back"
        );
    }
    if entry.partial {
        return format!(
            "the page wrote {wrote} to the pasteboard: what was there was put back in part, some of it could not be read"
        );
    }
    format!("the page wrote {wrote} to the pasteboard: it was put back as it was")
}

/// What the pasteboard holds now, in a word for a sentence: text, an image, files or something.
fn kind_of(board: &NSPasteboard) -> &'static str {
    let kinds: Vec<String> = board
        .types()
        .map(|kinds| kinds.iter().map(|kind| kind.to_string()).collect())
        .unwrap_or_default();
    let has = |wanted: &str| kinds.iter().any(|kind| kind == wanted);
    if has(TEXT) {
        "text"
    } else if has(PNG) || has(TIFF) {
        "an image"
    } else if has(FILE_URL) {
        "files"
    } else {
        "something"
    }
}

#[cfg(test)]
mod tests {
    use objc2_foundation::NSString;

    use super::*;
    use crate::clipboard::{self, tests::pasteboard_lock};

    /// A live board of its own, where writes stick: none in a headless environment.
    fn board() -> Option<Retained<NSPasteboard>> {
        let board = NSPasteboard::pasteboardWithUniqueName();
        board.clearContents();
        board.setString_forType(&NSString::from_str("probe"), text_type());
        let alive = clipboard::read(&board).as_deref() == Some("probe");
        alive.then_some(board)
    }

    fn text_type() -> &'static NSString {
        // SAFETY: a static of AppKit's, read on the main thread (the tests hold the lock).
        unsafe { objc2_app_kit::NSPasteboardTypeString }
    }

    fn set(board: &NSPasteboard, text: &str) {
        board.clearContents();
        board.setString_forType(&NSString::from_str(text), text_type());
    }

    fn text(board: &NSPasteboard) -> Option<String> {
        clipboard::read(board)
    }

    fn reset() {
        ENTRY.with_borrow_mut(|slot| *slot = None);
    }

    /// Two requests that overlap keep the pasteboard once: the second, which began after the first
    /// page had copied, does not keep that copy as the person's, and the last to end puts back
    /// what the first found.
    #[test]
    fn overlapping_requests_keep_the_pasteboard_once_and_the_last_puts_it_back() {
        let _lock = pasteboard_lock();
        let Some(board) = board() else { return };
        reset();
        set(&board, "the person's");
        enter(&board, true);
        set(&board, "a page's");
        enter(&board, true);
        set(&board, "another page's");
        let first = leave(&board).expect("told while another is out");
        assert!(
            first.contains("put back when the other requests end"),
            "{first}"
        );
        assert_eq!(text(&board).as_deref(), Some("another page's"));
        let last = leave(&board).expect("told");
        assert_eq!(
            last,
            "the page wrote text to the pasteboard: it was put back as it was"
        );
        assert_eq!(text(&board).as_deref(), Some("the person's"));
        // Nothing is kept once they have ended.
        assert!(leave(&board).is_none());
    }

    /// A pasteboard the page did not write is left alone, and not written to at all.
    #[test]
    fn a_pasteboard_that_was_not_written_is_left_alone() {
        let _lock = pasteboard_lock();
        let Some(board) = board() else { return };
        reset();
        set(&board, "the person's");
        let count = board.changeCount();
        enter(&board, true);
        assert!(leave(&board).is_none());
        assert_eq!(board.changeCount(), count);
    }

    /// A request that cannot make a page copy takes no data: a change is told and not undone, and
    /// says what the page wrote.
    #[test]
    fn a_request_that_keeps_no_data_tells_a_write_and_does_not_undo_it() {
        let _lock = pasteboard_lock();
        let Some(board) = board() else { return };
        reset();
        set(&board, "the person's");
        enter(&board, false);
        set(&board, "a page's");
        let said = leave(&board).expect("told");
        assert!(
            said.contains("wrote text") && said.contains("was not put back"),
            "{said}"
        );
        assert_eq!(text(&board).as_deref(), Some("a page's"));
    }

    /// What the page wrote is told as what it is: an image is not text.
    #[test]
    fn what_the_page_wrote_is_told_by_its_type() {
        let _lock = pasteboard_lock();
        let Some(board) = board() else { return };
        reset();
        set(&board, "the person's");
        enter(&board, false);
        board.clearContents();
        let item = NSPasteboardItem::new();
        item.setData_forType(&NSData::from_vec(vec![1, 2, 3]), &NSString::from_str(PNG));
        board.writeObjects(&NSArray::from_retained_slice(&[
            ProtocolObject::from_retained(item),
        ]));
        let said = leave(&board).expect("told");
        assert!(said.contains("wrote an image"), "{said}");
    }

    /// What is concealed is never read and never written back, and the answer says so.
    #[test]
    fn a_concealed_pasteboard_is_not_written_back() {
        let _lock = pasteboard_lock();
        let Some(board) = board() else { return };
        reset();
        board.clearContents();
        let item = NSPasteboardItem::new();
        item.setString_forType(&NSString::from_str("hunter2"), text_type());
        item.setData_forType(&NSData::from_vec(vec![1]), &NSString::from_str(CONCEALED));
        board.writeObjects(&NSArray::from_retained_slice(&[
            ProtocolObject::from_retained(item),
        ]));
        enter(&board, true);
        set(&board, "a page's");
        let said = leave(&board).expect("told");
        assert!(
            said.contains("concealed") && said.contains("was not written back"),
            "{said}"
        );
        assert_eq!(text(&board).as_deref(), Some("a page's"));
    }

    /// The person's own copy in bateri is not undone, and a page's note is not made of it.
    #[test]
    fn a_copy_bateri_made_itself_is_left_as_it_is() {
        let _lock = pasteboard_lock();
        let Some(board) = board() else { return };
        reset();
        set(&board, "the person's");
        enter(&board, true);
        assert!(clipboard::copy(&board, Some("copied in bateri".into())));
        assert!(leave(&board).is_none());
        assert_eq!(text(&board).as_deref(), Some("copied in bateri"));
    }
}
