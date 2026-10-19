//! Settings ▸ Keyboard: every command of the menu with its shortcuts, and the way to change them.
//!
//! The page holds no shortcuts of its own. Its rows are
//! [`shortcuts::Page`](bt_shell_common::shortcuts::Page) — the table read against the menu as it
//! stands, so a command has the title and the place the menu gives it — and the shortcuts are what
//! `settings.toml`'s `[keys]` says. A change is an edit of that file (one line, or two when a key
//! is taken from the command that had it) handed to `AppDelegate::save_edits`; the page
//! shows it only when it comes back from the file.
//!
//! What the user is doing is the page's one piece of state: listening for a command's shortcut
//! (the row shows a [`ShortcutRecorder`]), or a key waiting to be confirmed (the row shows the
//! note and its button). Both are dropped when the command is gone from the page, when the
//! search changes, and when anything else takes the keyboard.
//!
//! The table is rebuilt, whole, whenever anything changes: a hundred rows are cheap, and the
//! rows' views are not kept — so nothing here touches a row's views after an event that reloads
//! the table.

use std::cell::{Cell, OnceCell, RefCell};

use block2::RcBlock;
use bt_core::{Chord, Settings};
use bt_shell_common::shortcuts::{
    self, BUILT_IN_HEADING, Filter, Outcome, Page, PageRow, Resolved,
};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSAlert, NSAlertFirstButtonReturn, NSBezelStyle, NSButton, NSColor, NSControlSize,
    NSControlTextEditingDelegate, NSEvent, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSLayoutAttribute, NSLayoutConstraintOrientation,
    NSLineBreakMode, NSMenu, NSMenuItem, NSModalResponse, NSScrollView, NSSearchField,
    NSSegmentSwitchTracking, NSSegmentedControl, NSStackView, NSStackViewGravity,
    NSStrikethroughStyleAttributeName, NSTableCellView, NSTableColumn, NSTableView,
    NSTableViewDataSource, NSTableViewDelegate, NSTableViewGridLineStyle,
    NSTableViewSelectionHighlightStyle, NSTableViewStyle, NSTextField, NSTrackingArea,
    NSTrackingAreaOptions, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSAttributedStringKey, NSDictionary, NSInteger, NSNumber,
    NSObjectNSDelayedPerforming, NSObjectProtocol, NSRect, NSRunLoopCommonModes, NSString,
    ns_string,
};

use super::recorder::{Recorded, ShortcutRecorder};
use crate::app;
use crate::palette::menu_nodes;

/// The wrap width of a note under a row, in points: the table's width less the margins.
const NOTE_WIDTH: f64 = 440.0;

/// Who a recorder's news is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source {
    /// The shortcut in this slot of this command.
    Row { name: &'static str, slot: usize },
    /// Find by Key.
    Find,
}

/// What Find by Key is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Find {
    #[default]
    Off,
    Listening,
    Key(Chord),
}

/// One line of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Heading(String),
    Command(&'static str),
    /// The index of a built-in line key.
    BuiltIn(usize),
    /// Nothing matches the search.
    Empty,
}

/// A key waiting for the user's yes.
struct Pending {
    name: &'static str,
    outcome: Outcome,
}

struct State {
    /// The menu as it was last read; read again when the window is shown.
    menu: Vec<bt_shell_common::palette::MenuNode>,
    menu_stale: bool,
    settings: Settings,
    /// The file cannot be written: the page reads, it does not record.
    locked: bool,
    resolved: Resolved,
    page: Page,
    filter: Filter,
    query: String,
    find: Find,
    entries: Vec<Entry>,
    listening: Option<(&'static str, usize)>,
    pending: Option<Pending>,
    /// A key the field refused, and why: shown under the row while it keeps listening.
    refused: Option<(&'static str, String)>,
    /// The field of the row being listened to, once its cell is made.
    recorder: Option<Retained<ShortcutRecorder>>,
    /// What the fields heard, to be worked on a turn later.
    inbox: Vec<(Source, Recorded)>,
    /// The row to bring into view after the next reload.
    show: Option<&'static str>,
}

impl State {
    fn new() -> State {
        let settings = Settings::default();
        let resolved = shortcuts::resolve(&settings);
        State {
            menu: Vec::new(),
            menu_stale: true,
            page: Page::new(&[], &resolved),
            resolved,
            settings,
            locked: false,
            filter: Filter::All,
            query: String::new(),
            find: Find::Off,
            entries: Vec::new(),
            listening: None,
            pending: None,
            refused: None,
            recorder: None,
            inbox: Vec::new(),
            show: None,
        }
    }

    /// The lines of the table for the filter, the search and the key.
    fn lines(&self) -> Vec<Entry> {
        let key = match self.find {
            Find::Key(chord) => Some(chord),
            _ => None,
        };
        let listing = self.page.listing(self.filter, &self.query, key.as_ref());
        let mut out = Vec::new();
        for (title, rows) in &listing.sections {
            if rows.is_empty() {
                continue;
            }
            out.push(Entry::Heading((*title).to_owned()));
            out.extend(rows.iter().map(|row| Entry::Command(row.command.name)));
        }
        if !listing.built_in.is_empty() {
            out.push(Entry::Heading(BUILT_IN_HEADING.to_owned()));
            for line in &listing.built_in {
                if let Some(at) = self
                    .page
                    .built_in()
                    .iter()
                    .position(|own| std::ptr::eq(own, *line))
                {
                    out.push(Entry::BuiltIn(at));
                }
            }
        }
        if out.is_empty() {
            out.push(Entry::Empty);
        }
        out
    }

    /// Drops what the user was doing to a command the page no longer has.
    fn forget_gone(&mut self) {
        if let Some((name, _)) = self.listening
            && self.page.row(name).is_none()
        {
            self.listening = None;
        }
        if self
            .pending
            .as_ref()
            .is_some_and(|pending| self.page.row(pending.name).is_none())
        {
            self.pending = None;
        }
        if self
            .refused
            .as_ref()
            .is_some_and(|(name, _)| self.page.row(name).is_none())
        {
            self.refused = None;
        }
    }
}

pub(super) struct Ivars {
    state: RefCell<State>,
    views: OnceCell<Views>,
    /// A table being reloaded drops its cells, and a field that loses the keyboard because of it
    /// has not been left by the user.
    reloading: Cell<bool>,
}

/// The controls the page is made of.
struct Views {
    search: Retained<NSSearchField>,
    filter: Retained<NSSegmentedControl>,
    find: Retained<NSButton>,
    find_field: Retained<ShortcutRecorder>,
    restore: Retained<NSButton>,
    table: Retained<NSTableView>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the page implements no `Drop` and is born with
    // `init`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriKeyboardPage"]
    #[ivars = Ivars]
    pub(super) struct KeyboardPage;

    unsafe impl NSObjectProtocol for KeyboardPage {}

    unsafe impl NSTableViewDataSource for KeyboardPage {
        #[unsafe(method(numberOfRowsInTableView:))]
        fn number_of_rows(&self, _table: &NSTableView) -> NSInteger {
            self.ivars().state.borrow().entries.len() as NSInteger
        }
    }

    unsafe impl NSControlTextEditingDelegate for KeyboardPage {}

    unsafe impl NSTableViewDelegate for KeyboardPage {
        #[unsafe(method_id(tableView:viewForTableColumn:row:))]
        fn view_for_row(
            &self,
            _table: &NSTableView,
            _column: Option<&NSTableColumn>,
            row: NSInteger,
        ) -> Option<Retained<NSView>> {
            match usize::try_from(row) {
                Ok(index) => self.cell(index),
                Err(_) => None,
            }
        }

        #[unsafe(method(tableView:isGroupRow:))]
        fn is_group_row(&self, _table: &NSTableView, row: NSInteger) -> bool {
            usize::try_from(row)
                .ok()
                .and_then(|row| self.ivars().state.borrow().entries.get(row).cloned())
                .is_some_and(|entry| matches!(entry, Entry::Heading(_)))
        }

        #[unsafe(method(tableView:shouldSelectRow:))]
        fn should_select_row(&self, _table: &NSTableView, _row: NSInteger) -> bool {
            false
        }
    }

    impl KeyboardPage {
        /// A shortcut's chip: listen for a new one in its place.
        #[unsafe(method(chipClicked:))]
        fn chip_clicked(&self, sender: Option<&AnyObject>) {
            if let Some((name, slot)) = self.target_of(sender) {
                self.listen_to(name, slot);
            }
        }

        /// Add Shortcut on a row that has none.
        #[unsafe(method(addClicked:))]
        fn add_clicked(&self, sender: Option<&AnyObject>) {
            self.add_for(sender);
        }

        /// Add Another Shortcut in a chip's menu.
        #[unsafe(method(addAnother:))]
        fn add_another(&self, sender: Option<&AnyObject>) {
            self.add_for(sender);
        }

        /// Remove Shortcut in a chip's menu.
        #[unsafe(method(removeShortcut:))]
        fn remove_shortcut(&self, sender: Option<&AnyObject>) {
            let Some((name, slot)) = self.target_of(sender) else {
                return;
            };
            let edit = {
                let st = self.ivars().state.borrow();
                shortcuts::by_name(name)
                    .and_then(|command| shortcuts::remove_edit(&st.resolved, command, slot))
            };
            self.stop_listening();
            match edit {
                Some(edit) => self.save(vec![edit]),
                None => self.reload(),
            }
        }

        /// Reset: the line leaves the file and the command has its own shortcuts again.
        #[unsafe(method(resetClicked:))]
        fn reset_clicked(&self, sender: Option<&AnyObject>) {
            let Some((name, _)) = self.target_of(sender) else {
                return;
            };
            self.stop_listening();
            match shortcuts::by_name(name) {
                Some(command) => self.save(vec![shortcuts::reset_edit(command)]),
                None => self.reload(),
            }
        }

        /// Use Here, Assign Anyway.
        #[unsafe(method(confirmClicked:))]
        fn confirm_clicked(&self, _sender: Option<&AnyObject>) {
            let pending = self.ivars().state.borrow_mut().pending.take();
            match pending {
                Some(pending) => self.save(pending.outcome.edits),
                None => self.reload(),
            }
        }

        #[unsafe(method(cancelClicked:))]
        fn cancel_clicked(&self, _sender: Option<&AnyObject>) {
            self.stop_listening();
            self.reload();
        }

        #[unsafe(method(searchChanged:))]
        fn search_changed(&self, sender: Option<&AnyObject>) {
            let Some(field) = sender.and_then(|s| s.downcast_ref::<NSSearchField>()) else {
                return;
            };
            let text = field.stringValue().to_string();
            {
                let mut st = self.ivars().state.borrow_mut();
                if st.query == text {
                    return;
                }
                st.query = text;
            }
            self.stop_listening();
            self.refilter();
        }

        #[unsafe(method(filterChanged:))]
        fn filter_changed(&self, sender: Option<&AnyObject>) {
            let Some(control) = sender.and_then(|s| s.downcast_ref::<NSSegmentedControl>()) else {
                return;
            };
            let filter = usize::try_from(control.selectedSegment())
                .ok()
                .and_then(|at| Filter::ALL.get(at).copied())
                .unwrap_or_default();
            self.ivars().state.borrow_mut().filter = filter;
            self.stop_listening();
            self.refilter();
        }

        /// Find by Key: listen for a key, or let go of the one that is set.
        #[unsafe(method(findClicked:))]
        fn find_clicked(&self, _sender: Option<&AnyObject>) {
            {
                let mut st = self.ivars().state.borrow_mut();
                st.find = match st.find {
                    Find::Off => Find::Listening,
                    Find::Listening | Find::Key(_) => Find::Off,
                };
                st.listening = None;
                st.pending = None;
                st.refused = None;
            }
            self.refilter();
            if self.ivars().state.borrow().find == Find::Listening
                && let Some(views) = self.ivars().views.get()
            {
                views.find_field.listen();
            }
        }

        #[unsafe(method(restoreClicked:))]
        fn restore_clicked(&self, _sender: Option<&AnyObject>) {
            self.ask_to_restore();
        }

        /// The row the page was opened on comes into view, and the keyboard goes to its field.
        #[unsafe(method(settle:))]
        fn settle(&self, _unused: Option<&AnyObject>) {
            let Some(views) = self.ivars().views.get() else {
                return;
            };
            let shown = {
                let mut st = self.ivars().state.borrow_mut();
                st.show.take().and_then(|name| {
                    st.entries
                        .iter()
                        .position(|entry| matches!(entry, Entry::Command(own) if *own == name))
                })
            };
            if let Some(at) = shown {
                // Rows below the ones made are of an estimated height until they are made:
                // each scroll makes the rows it passes and the next one lands closer.
                for _ in 0..4 {
                    views.table.scrollRowToVisible(at as NSInteger);
                    views.table.layoutSubtreeIfNeeded();
                }
            }
            let field = self.ivars().state.borrow().recorder.clone();
            if let Some(field) = field {
                field.listen();
            }
        }

        /// The keyboard goes to the field of the row that listens.
        #[unsafe(method(focusRecorder:))]
        fn focus_recorder(&self, _unused: Option<&AnyObject>) {
            let field = self.ivars().state.borrow().recorder.clone();
            if let Some(field) = field {
                field.listen();
            }
        }

        /// What the fields heard, a turn after they heard it.
        #[unsafe(method(drain:))]
        fn drain(&self, _unused: Option<&AnyObject>) {
            let heard = std::mem::take(&mut self.ivars().state.borrow_mut().inbox);
            for (source, heard) in heard {
                match source {
                    Source::Row { name, slot } => self.row_heard(name, slot, heard),
                    Source::Find => self.find_heard(heard),
                }
            }
        }
    }
);

/// A label of one line.
fn text(mtm: MainThreadMarker, text: &str, size: f64, color: &NSColor) -> Retained<NSTextField> {
    let label = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(size)));
    label.setTextColor(Some(color));
    label.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    // The title gives way before the shortcuts do.
    label.setContentCompressionResistancePriority_forOrientation(
        250.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    label
}

/// A note that wraps under a row.
fn note(mtm: MainThreadMarker, text: &str, color: &NSColor) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setFont(Some(&NSFont::systemFontOfSize(
        NSFont::smallSystemFontSize(),
    )));
    label.setTextColor(Some(color));
    label.setPreferredMaxLayoutWidth(NOTE_WIDTH);
    label
}

fn stack(
    mtm: MainThreadMarker,
    views: &[&NSView],
    orientation: NSUserInterfaceLayoutOrientation,
    spacing: f64,
) -> Retained<NSStackView> {
    let stack = NSStackView::stackViewWithViews(&NSArray::from_slice(views), mtm);
    stack.setOrientation(orientation);
    stack.setSpacing(spacing);
    if orientation == NSUserInterfaceLayoutOrientation::Vertical {
        stack.setAlignment(NSLayoutAttribute::Leading);
    } else {
        stack.setAlignment(NSLayoutAttribute::CenterY);
    }
    stack
}

/// What a row says on the left and its shortcuts on the right: the words give way, the shortcuts
/// keep their size and the right edge.
fn row_line(mtm: MainThreadMarker, words: &NSView, keys: &NSView) -> Retained<NSView> {
    let top = NSView::new(mtm);
    for view in [words, keys] {
        view.setTranslatesAutoresizingMaskIntoConstraints(false);
        top.addSubview(view);
    }
    words.setContentCompressionResistancePriority_forOrientation(
        250.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    keys.setContentCompressionResistancePriority_forOrientation(
        900.0,
        NSLayoutConstraintOrientation::Horizontal,
    );
    keys.setContentHuggingPriority_forOrientation(900.0, NSLayoutConstraintOrientation::Horizontal);
    // The line is as tall as the taller of the two, and 24 points at least.
    let least = top.heightAnchor().constraintEqualToConstant(24.0);
    least.setPriority(250.0);
    for constraint in [
        words
            .leadingAnchor()
            .constraintEqualToAnchor(&top.leadingAnchor()),
        keys.trailingAnchor()
            .constraintEqualToAnchor(&top.trailingAnchor()),
        words
            .trailingAnchor()
            .constraintLessThanOrEqualToAnchor_constant(&keys.leadingAnchor(), -8.0),
        words
            .centerYAnchor()
            .constraintEqualToAnchor(&top.centerYAnchor()),
        keys.centerYAnchor()
            .constraintEqualToAnchor(&top.centerYAnchor()),
        top.heightAnchor()
            .constraintGreaterThanOrEqualToAnchor(&words.heightAnchor()),
        top.heightAnchor()
            .constraintGreaterThanOrEqualToAnchor(&keys.heightAnchor()),
        least,
    ] {
        constraint.setActive(true);
    }
    top
}

/// A control or a label as a view.
fn as_view<T>(view: Retained<T>) -> Retained<NSView>
where
    T: objc2::Message + AsRef<NSView>,
{
    // SAFETY: `AsRef<NSView>` is the type's own superclass chain.
    let raw: &NSView = (*view).as_ref();
    raw.retain()
}

struct RowIvars {
    /// What the row offers only under the pointer: Add Shortcut beside a lone shortcut, so the
    /// second one is not found in the chip's menu alone and every row is not a row of buttons.
    on_hover: Option<Retained<NSView>>,
}

define_class!(
    // SAFETY: NSView is designed for subclassing; the row implements no `Drop` and is born with
    // `initWithFrame:`.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateriKeyboardRow"]
    #[ivars = RowIvars]
    struct CommandRow;

    unsafe impl NSObjectProtocol for CommandRow {}

    impl CommandRow {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.reveal(true);
        }

        /// The table is made again under the pointer after every change: the row born there hears
        /// no `mouseEntered:`, only the pointer's next move.
        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, _event: &NSEvent) {
            self.reveal(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.reveal(false);
        }
    }
);

impl CommandRow {
    fn new(mtm: MainThreadMarker, on_hover: Option<Retained<NSView>>) -> Retained<Self> {
        let hovers = on_hover.is_some();
        let this = Self::alloc(mtm).set_ivars(RowIvars { on_hover });
        // SAFETY: NSView's designated initializer.
        let this: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: NSRect::ZERO] };
        if hovers {
            let options = NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::MouseMoved
                | NSTrackingAreaOptions::ActiveInKeyWindow
                | NSTrackingAreaOptions::InVisibleRect;
            // SAFETY: `owner` is the row, which owns the area and outlives it; no user info.
            // `InVisibleRect` follows the view's bounds, so the rect is ignored.
            let area = unsafe {
                NSTrackingArea::initWithRect_options_owner_userInfo(
                    NSTrackingArea::alloc(),
                    NSRect::ZERO,
                    options,
                    Some(&this),
                    None,
                )
            };
            this.addTrackingArea(&area);
        }
        this
    }

    fn reveal(&self, shown: bool) {
        if let Some(view) = &self.ivars().on_hover
            && view.isHidden() == shown
        {
            view.setHidden(!shown);
        }
    }
}

/// The tag a control carries: which line of the table, and which of its shortcuts.
fn tag_of(entry: usize, slot: usize) -> isize {
    ((entry << 2) | (slot & 3)) as isize
}

impl KeyboardPage {
    pub(super) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(Ivars {
            state: RefCell::new(State::new()),
            views: OnceCell::new(),
            reloading: Cell::new(false),
        });
        // SAFETY: NSView's `init` takes no arguments and the ivars are set.
        let this: Retained<Self> = unsafe { msg_send![super(this), init] };
        this.build();
        this
    }

    fn target(&self) -> &AnyObject {
        self.as_ref()
    }

    /// The search, the filters and the table.
    fn build(&self) {
        let mtm = self.mtm();
        self.setTranslatesAutoresizingMaskIntoConstraints(false);

        let search = NSSearchField::new(mtm);
        search.setPlaceholderString(Some(ns_string!("Search commands")));
        search.setSendsSearchStringImmediately(true);
        // SAFETY: the target is a weak reference; the page lives as long as the settings window,
        // which lives for the whole process.
        unsafe {
            search.setTarget(Some(self.target()));
            search.setAction(Some(sel!(searchChanged:)));
        }

        let labels = NSArray::from_retained_slice(
            &Filter::ALL
                .iter()
                .map(|filter| NSString::from_str(filter.title()))
                .collect::<Vec<_>>(),
        );
        // SAFETY: the target is a weak reference and the selector is this class's.
        let filter = unsafe {
            NSSegmentedControl::segmentedControlWithLabels_trackingMode_target_action(
                &labels,
                NSSegmentSwitchTracking::SelectOne,
                Some(self.target()),
                Some(sel!(filterChanged:)),
                mtm,
            )
        };
        filter.setSelectedSegment(0);

        // SAFETY: as above.
        let find = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Find by Key"),
                Some(self.target()),
                Some(sel!(findClicked:)),
                mtm,
            )
        };
        let weak = Weak::new(self);
        let find_field = ShortcutRecorder::new(mtm, "Type shortcut", move |heard| {
            if let Some(page) = weak.load() {
                page.hear(Source::Find, heard);
            }
        });
        find_field.setHidden(true);
        // SAFETY: as above.
        let restore = unsafe {
            NSButton::buttonWithTitle_target_action(
                ns_string!("Restore Defaults…"),
                Some(self.target()),
                Some(sel!(restoreClicked:)),
                mtm,
            )
        };

        let finder = stack(
            mtm,
            &[&find, &find_field],
            NSUserInterfaceLayoutOrientation::Horizontal,
            0.0,
        );
        let controls = stack(
            mtm,
            &[&filter, &finder],
            NSUserInterfaceLayoutOrientation::Horizontal,
            12.0,
        );
        controls.addView_inGravity(&restore, NSStackViewGravity::Trailing);
        let toolbar = stack(
            mtm,
            &[&search, &controls],
            NSUserInterfaceLayoutOrientation::Vertical,
            10.0,
        );
        toolbar.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&toolbar);
        for constraint in [
            search
                .widthAnchor()
                .constraintEqualToAnchor(&toolbar.widthAnchor()),
            controls
                .widthAnchor()
                .constraintEqualToAnchor(&toolbar.widthAnchor()),
        ] {
            constraint.setActive(true);
        }

        let table = NSTableView::new(mtm);
        table.setStyle(NSTableViewStyle::FullWidth);
        table.setHeaderView(None);
        table.setSelectionHighlightStyle(NSTableViewSelectionHighlightStyle::None);
        table.setUsesAutomaticRowHeights(true);
        table.setGridStyleMask(NSTableViewGridLineStyle::SolidHorizontalGridLineMask);
        table.setAllowsEmptySelection(true);
        let column = NSTableColumn::initWithIdentifier(NSTableColumn::alloc(mtm), ns_string!("k"));
        table.addTableColumn(&column);
        // SAFETY: the source and the delegate are weak references; the page outlives the table.
        unsafe {
            table.setDataSource(Some(ProtocolObject::from_ref(self)));
            table.setDelegate(Some(ProtocolObject::from_ref(self)));
        }
        let scroll = NSScrollView::new(mtm);
        scroll.setDocumentView(Some(&table));
        scroll.setHasVerticalScroller(true);
        scroll.setDrawsBackground(false);
        scroll.setAutohidesScrollers(true);
        scroll.setTranslatesAutoresizingMaskIntoConstraints(false);
        self.addSubview(&scroll);

        for constraint in [
            toolbar
                .topAnchor()
                .constraintEqualToAnchor(&self.topAnchor()),
            toolbar
                .leadingAnchor()
                .constraintEqualToAnchor(&self.leadingAnchor()),
            toolbar
                .trailingAnchor()
                .constraintEqualToAnchor(&self.trailingAnchor()),
            scroll
                .topAnchor()
                .constraintEqualToAnchor_constant(&toolbar.bottomAnchor(), 12.0),
            scroll
                .leadingAnchor()
                .constraintEqualToAnchor(&self.leadingAnchor()),
            scroll
                .trailingAnchor()
                .constraintEqualToAnchor(&self.trailingAnchor()),
            scroll
                .bottomAnchor()
                .constraintEqualToAnchor(&self.bottomAnchor()),
        ] {
            constraint.setActive(true);
        }

        let _ = self.ivars().views.set(Views {
            search,
            filter,
            find,
            find_field,
            restore,
            table,
        });
    }

    // ─── Fed by the window ─────────────────────────────────────────────

    /// The menu is read again the next time the page is built: its titles and places change
    /// with the language and with what the tabs are on.
    pub(super) fn invalidate_menu(&self) {
        self.ivars().state.borrow_mut().menu_stale = true;
    }

    /// The page comes on screen: built again if the menu has been read since.
    pub(super) fn appear(&self) {
        if self.ivars().state.borrow().menu_stale {
            self.rebuild();
        }
    }

    /// The file's shortcuts, read again.
    pub(super) fn refresh(&self, settings: &Settings, locked: bool) {
        {
            let mut st = self.ivars().state.borrow_mut();
            st.settings = settings.clone();
            st.locked = locked;
            if locked {
                st.listening = None;
                st.pending = None;
                st.refused = None;
                if st.find == Find::Listening {
                    st.find = Find::Off;
                }
            }
        }
        if self.isHidden() {
            // Not on screen: the table is made when the page comes (`appear`), from the settings
            // kept above and the menu as it is by then — not at each of the window's refreshes.
            self.invalidate_menu();
        } else {
            self.rebuild();
        }
        if let Some(views) = self.ivars().views.get() {
            views.restore.setEnabled(!locked);
        }
    }

    /// Opens the page on `name`: no filter, no search, the command in view and listening for its
    /// new shortcut.
    pub(super) fn show_command(&self, name: &'static str) {
        {
            let mut st = self.ivars().state.borrow_mut();
            st.filter = Filter::All;
            st.query.clear();
            st.find = Find::Off;
            st.pending = None;
            st.refused = None;
            st.show = Some(name);
            st.listening = (!st.locked).then_some((name, 0));
        }
        if let Some(views) = self.ivars().views.get() {
            views.search.setStringValue(ns_string!(""));
            views.filter.setSelectedSegment(0);
        }
        self.invalidate_menu();
        self.rebuild();
    }

    // ─── What is shown ─────────────────────────────────────────────────

    /// The page, made again from the menu and the settings.
    fn rebuild(&self) {
        let (stale, settings) = {
            let st = self.ivars().state.borrow();
            (st.menu_stale, st.settings.clone())
        };
        // Reading the menu validates its items and fills the submenus a delegate fills: with no
        // borrow held.
        let menu = stale.then(|| menu_nodes(self.mtm()));
        {
            let mut st = self.ivars().state.borrow_mut();
            if let Some(menu) = menu {
                st.menu = menu;
                st.menu_stale = false;
            }
            st.resolved = shortcuts::resolve(&settings);
            st.page = Page::new(&st.menu, &st.resolved);
            st.forget_gone();
            st.entries = st.lines();
        }
        self.reload();
    }

    /// The lines of the table again, for a new filter or search.
    fn refilter(&self) {
        {
            let mut st = self.ivars().state.borrow_mut();
            st.entries = st.lines();
        }
        self.reload();
    }

    /// Draws the table anew, then gives the keyboard to the field that listens.
    fn reload(&self) {
        let Some(views) = self.ivars().views.get() else {
            return;
        };
        self.ivars().reloading.set(true);
        self.ivars().state.borrow_mut().recorder = None;
        views.table.reloadData();
        views.table.layoutSubtreeIfNeeded();
        self.ivars().reloading.set(false);
        self.sync_toolbar();
        // The table is laid out a turn later when it was hidden or has just changed size: the
        // row to show is scrolled to then.
        if self.ivars().state.borrow().show.is_some() {
            self.later(sel!(settle:));
        }
        let field = self.ivars().state.borrow().recorder.clone();
        if let Some(field) = field {
            field.listen();
        }
    }

    /// The buttons say what the filters are doing.
    fn sync_toolbar(&self) {
        let Some(views) = self.ivars().views.get() else {
            return;
        };
        // Read with no borrow held after: hiding the field that listens takes its keyboard, and
        // the field tells the page so (`find_heard`), which writes the state.
        let find = self.ivars().state.borrow().find;
        let (title, listening) = match find {
            Find::Off => ("Find by Key".to_owned(), false),
            Find::Listening => ("Find by Key".to_owned(), true),
            Find::Key(chord) => (
                format!(
                    "Clear Key {}",
                    shortcuts::glyphs(&bt_core::Shortcut::Chord(chord)).unwrap_or_default()
                ),
                false,
            ),
        };
        views.find.setTitle(&NSString::from_str(&title));
        views.find.setHidden(listening);
        views.find_field.setHidden(!listening);
        views.find.setEnabled(true);
    }

    // ─── The cells ─────────────────────────────────────────────────────

    /// The view of line `index`.
    fn cell(&self, index: usize) -> Option<Retained<NSView>> {
        let mtm = self.mtm();
        let (entry, built) = {
            let st = self.ivars().state.borrow();
            let entry = st.entries.get(index)?.clone();
            let built = match &entry {
                Entry::Heading(title) => (heading_cell(mtm, title), None),
                Entry::Empty => (empty_cell(mtm), None),
                Entry::BuiltIn(at) => (built_in_cell(mtm, st.page.built_in().get(*at)?), None),
                Entry::Command(name) => self.command_cell(mtm, &st, index, name)?,
            };
            (entry, built)
        };
        let (view, field) = built;
        if let Some(field) = field {
            self.ivars().state.borrow_mut().recorder = Some(field);
            // The table may be laid out after `reload` has asked: the keyboard goes to the
            // field once the turn that made it is over.
            self.later(sel!(focusRecorder:));
        }
        let _ = entry;
        Some(view)
    }

    fn command_cell(
        &self,
        mtm: MainThreadMarker,
        st: &State,
        index: usize,
        name: &'static str,
    ) -> Option<(Retained<NSView>, Option<Retained<ShortcutRecorder>>)> {
        let row = st.page.row(name)?;
        let primary = NSColor::labelColor();
        let secondary = NSColor::secondaryLabelColor();
        let title = text(mtm, &row.title, 13.0, &primary);
        let mut texts: Vec<&NSView> = vec![&title];
        let submenu = row
            .submenu()
            .map(|submenu| text(mtm, submenu, 11.0, &secondary));
        if let Some(submenu) = &submenu {
            texts.push(submenu);
        }
        let words = stack(mtm, &texts, NSUserInterfaceLayoutOrientation::Vertical, 1.0);

        let mut field = None;
        let mut keys: Vec<Retained<NSView>> = Vec::new();
        // Left of the chips, so the chips and Reset keep their place when it shows.
        let on_hover = (!row.chips.is_empty()
            && row.chips.len() < bt_core::MAX_SHORTCUTS
            && row.command.family.is_none()
            && st.listening.is_none_or(|(own, _)| own != name)
            && !st.locked)
            .then(|| {
                let add = self.button(mtm, "Add Shortcut", sel!(addClicked:), tag_of(index, 0));
                add.setHidden(true);
                as_view(add)
            });
        if let Some(add) = &on_hover {
            keys.push(add.clone());
        }
        for (slot, chip) in row.chips.iter().enumerate() {
            if st.listening == Some((name, slot)) {
                let recorder = self.recorder(mtm, name, slot);
                keys.push(Retained::into_super(recorder.clone()));
                field = Some(recorder);
            } else {
                keys.push(as_view(self.chip(mtm, st, row, index, slot, &chip.text)));
            }
        }
        if st.listening == Some((name, row.chips.len())) {
            let recorder = self.recorder(mtm, name, row.chips.len());
            keys.push(Retained::into_super(recorder.clone()));
            field = Some(recorder);
        } else if row.chips.is_empty() {
            let add = self.button(mtm, "Add Shortcut", sel!(addClicked:), tag_of(index, 0));
            add.setEnabled(!st.locked);
            keys.push(as_view(add));
        }
        if row.changed {
            let reset = self.button(mtm, "Reset", sel!(resetClicked:), tag_of(index, 0));
            reset.setEnabled(!st.locked);
            keys.push(as_view(reset));
        }
        let key_views: Vec<&NSView> = keys.iter().map(|view| &**view).collect();
        let chips = stack(
            mtm,
            &key_views,
            NSUserInterfaceLayoutOrientation::Horizontal,
            6.0,
        );
        let top = row_line(mtm, &words, &chips);

        let mut rows: Vec<Retained<NSView>> = vec![top.clone()];
        let orange = NSColor::systemOrangeColor();
        let red = NSColor::systemRedColor();
        if let Some(pending) = st.pending.as_ref().filter(|pending| pending.name == name) {
            if let Some(words) = &pending.outcome.note {
                rows.push(as_view(note(mtm, words, &orange)));
            }
            let cancel = self.button(mtm, "Cancel", sel!(cancelClicked:), tag_of(index, 0));
            let mut verbs: Vec<Retained<NSView>> = vec![as_view(cancel)];
            if let Some(verb) = pending.outcome.button {
                verbs.push(as_view(self.button(
                    mtm,
                    verb,
                    sel!(confirmClicked:),
                    tag_of(index, 0),
                )));
            }
            let refs: Vec<&NSView> = verbs.iter().map(|view| &**view).collect();
            rows.push(Retained::into_super(stack(
                mtm,
                &refs,
                NSUserInterfaceLayoutOrientation::Horizontal,
                8.0,
            )));
        }
        if let Some((_, words)) = st.refused.as_ref().filter(|(own, _)| *own == name) {
            rows.push(as_view(note(mtm, words, &red)));
        }
        if let Some(words) = &row.note {
            rows.push(as_view(note(mtm, words, &secondary)));
        }
        let refs: Vec<&NSView> = rows.iter().map(|view| &**view).collect();
        let whole = stack(mtm, &refs, NSUserInterfaceLayoutOrientation::Vertical, 4.0);
        let cell = CommandRow::new(mtm, on_hover);
        whole.setTranslatesAutoresizingMaskIntoConstraints(false);
        cell.addSubview(&whole);
        for constraint in [
            whole
                .topAnchor()
                .constraintEqualToAnchor_constant(&cell.topAnchor(), 6.0),
            whole
                .bottomAnchor()
                .constraintEqualToAnchor_constant(&cell.bottomAnchor(), -6.0),
            whole
                .leadingAnchor()
                .constraintEqualToAnchor_constant(&cell.leadingAnchor(), 4.0),
            whole
                .trailingAnchor()
                .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -4.0),
            top.widthAnchor()
                .constraintEqualToAnchor(&whole.widthAnchor()),
        ] {
            constraint.setActive(true);
        }
        Some((Retained::into_super(cell), field))
    }

    /// A shortcut as a button: it listens for a new one in its place, and its menu adds another or
    /// takes it away.
    fn chip(
        &self,
        mtm: MainThreadMarker,
        st: &State,
        row: &PageRow,
        index: usize,
        slot: usize,
        title: &str,
    ) -> Retained<NSButton> {
        let chip = self.button(mtm, title, sel!(chipClicked:), tag_of(index, slot));
        chip.setEnabled(!st.locked);
        let menu = NSMenu::new(mtm);
        if row.chips.len() < bt_core::MAX_SHORTCUTS && row.command.family.is_none() {
            menu.addItem(&self.menu_item(
                mtm,
                "Add Another Shortcut",
                sel!(addAnother:),
                index,
                slot,
            ));
        }
        menu.addItem(&self.menu_item(mtm, "Remove Shortcut", sel!(removeShortcut:), index, slot));
        if !st.locked {
            // SAFETY: a responder's menu is the context menu AppKit shows for a right click.
            unsafe { chip.setMenu(Some(&menu)) };
        }
        chip
    }

    fn menu_item(
        &self,
        mtm: MainThreadMarker,
        title: &str,
        action: objc2::runtime::Sel,
        index: usize,
        slot: usize,
    ) -> Retained<NSMenuItem> {
        // SAFETY: the target is a weak reference and the page outlives its menus.
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(action),
                ns_string!(""),
            )
        };
        // SAFETY: as above.
        unsafe { item.setTarget(Some(self.target())) };
        item.setTag(tag_of(index, slot));
        item
    }

    fn button(
        &self,
        mtm: MainThreadMarker,
        title: &str,
        action: objc2::runtime::Sel,
        tag: isize,
    ) -> Retained<NSButton> {
        // SAFETY: the target is a weak reference and the selector is this class's.
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(self.target()),
                Some(action),
                mtm,
            )
        };
        button.setBezelStyle(NSBezelStyle::Push);
        button.setControlSize(NSControlSize::Small);
        button.setTag(tag);
        button
    }

    /// The field of a row that is listening.
    fn recorder(
        &self,
        mtm: MainThreadMarker,
        name: &'static str,
        slot: usize,
    ) -> Retained<ShortcutRecorder> {
        let weak = Weak::new(self);
        ShortcutRecorder::new(mtm, "Type shortcut", move |heard| {
            if let Some(page) = weak.load() {
                page.hear(Source::Row { name, slot }, heard);
            }
        })
    }

    // ─── What the user does ────────────────────────────────────────────

    /// The command and the slot a control stands for.
    fn target_of(&self, sender: Option<&AnyObject>) -> Option<(&'static str, usize)> {
        let tag = if let Some(button) = sender.and_then(|s| s.downcast_ref::<NSButton>()) {
            button.tag()
        } else {
            sender.and_then(|s| s.downcast_ref::<NSMenuItem>())?.tag()
        };
        let tag = usize::try_from(tag).ok()?;
        let st = self.ivars().state.borrow();
        match st.entries.get(tag >> 2)? {
            Entry::Command(name) => Some((*name, tag & 3)),
            _ => None,
        }
    }

    /// Listens for one more shortcut of the command a control stands for.
    fn add_for(&self, sender: Option<&AnyObject>) {
        if let Some((name, _)) = self.target_of(sender) {
            let slot = self.chips_of(name);
            self.listen_to(name, slot);
        }
    }

    fn chips_of(&self, name: &str) -> usize {
        self.ivars()
            .state
            .borrow()
            .page
            .row(name)
            .map_or(0, |row| row.chips.len())
    }

    /// Starts listening for the shortcut in `slot` of `name`.
    fn listen_to(&self, name: &'static str, slot: usize) {
        {
            let mut st = self.ivars().state.borrow_mut();
            if st.locked {
                return;
            }
            st.listening = Some((name, slot));
            st.pending = None;
            st.refused = None;
            if st.find == Find::Listening {
                st.find = Find::Off;
            }
        }
        self.reload();
    }

    /// Stops listening and drops what was waiting.
    fn stop_listening(&self) {
        let mut st = self.ivars().state.borrow_mut();
        st.listening = None;
        st.pending = None;
        st.refused = None;
    }

    /// The news of a field: kept, and worked on after the event that brought it is over.
    fn hear(&self, source: Source, heard: Recorded) {
        if self.ivars().reloading.get() {
            return;
        }
        self.ivars().state.borrow_mut().inbox.push((source, heard));
        self.later(sel!(drain:));
    }

    /// Has `action` (one of this class's, taking one optional argument) run once the event or the
    /// layout in progress is over; the run loop keeps the page alive until it has.
    fn later(&self, action: objc2::runtime::Sel) {
        // SAFETY: `action` is this class's and takes one optional argument.
        unsafe {
            self.performSelector_withObject_afterDelay_inModes(
                action,
                None,
                0.0,
                &NSArray::from_slice(&[NSRunLoopCommonModes]),
            );
        }
    }

    fn row_heard(&self, name: &'static str, slot: usize, heard: Recorded) {
        if self.ivars().state.borrow().listening != Some((name, slot)) {
            return;
        }
        match heard {
            Recorded::Escape | Recorded::Lost => {
                self.stop_listening();
                self.reload();
            }
            Recorded::Delete => {
                let edit = {
                    let st = self.ivars().state.borrow();
                    shortcuts::by_name(name)
                        .and_then(|command| shortcuts::remove_edit(&st.resolved, command, slot))
                };
                self.stop_listening();
                match edit {
                    Some(edit) => self.save(vec![edit]),
                    None => self.reload(),
                }
            }
            Recorded::Pressed(chord) => {
                let outcome = {
                    let st = self.ivars().state.borrow();
                    shortcuts::record(&st.page, &st.resolved, name, slot, &chord)
                };
                self.row_outcome(name, outcome);
            }
            Recorded::Twice(tapped) => {
                let outcome = {
                    let st = self.ivars().state.borrow();
                    shortcuts::record_twice(&st.page, &st.resolved, name, slot, tapped)
                };
                self.row_outcome(name, outcome);
            }
        }
    }

    /// What a pressed key (or a double press) came to for the row that listens: refused, with a
    /// note and the field still listening; written at once; or kept with its question.
    fn row_outcome(&self, name: &'static str, outcome: Option<shortcuts::Outcome>) {
        let Some(outcome) = outcome else {
            return;
        };
        if outcome.is_refused() {
            // The field keeps listening: the user presses another key.
            self.ivars().state.borrow_mut().refused =
                Some((name, outcome.note.unwrap_or_default()));
            self.reload();
        } else if outcome.is_accepted() {
            self.stop_listening();
            self.save(outcome.edits);
        } else {
            {
                let mut st = self.ivars().state.borrow_mut();
                st.listening = None;
                st.refused = None;
                st.pending = Some(Pending { name, outcome });
            }
            self.reload();
        }
    }

    fn find_heard(&self, heard: Recorded) {
        {
            let mut st = self.ivars().state.borrow_mut();
            if st.find != Find::Listening {
                return;
            }
            st.find = match heard {
                Recorded::Pressed(chord) => Find::Key(chord),
                // A double press has no chord to look for: the field listens on.
                Recorded::Twice(_) => return,
                Recorded::Escape | Recorded::Delete | Recorded::Lost => Find::Off,
            };
        }
        self.refilter();
    }

    /// Writes the edits; the page shows them when the file comes back.
    fn save(&self, edits: Vec<bt_core::SettingsEdit>) {
        if let Some(delegate) = app::delegate(self.mtm()) {
            let _ = delegate.save_edits(&edits);
        }
        self.reload();
    }

    /// Restore Defaults…: a question first, since it clears every line of `[keys]`.
    fn ask_to_restore(&self) {
        let mtm = self.mtm();
        let Some(window) = self
            .window()
            .filter(|window| window.attachedSheet().is_none())
        else {
            return;
        };
        let alert = NSAlert::new(mtm);
        alert.setMessageText(ns_string!("Restore every shortcut to its default?"));
        alert.setInformativeText(ns_string!(
            "This takes the shortcuts out of [keys] in settings.toml. Its comments and its other settings stay."
        ));
        alert.addButtonWithTitle(ns_string!("Restore Defaults"));
        alert.addButtonWithTitle(ns_string!("Cancel"));
        let page = self.retain();
        let answered = RcBlock::new(move |response: NSModalResponse| {
            if response == NSAlertFirstButtonReturn {
                page.stop_listening();
                page.save(vec![shortcuts::restore_edit()]);
            }
        });
        alert.beginSheetModalForWindow_completionHandler(&window, Some(&answered));
    }
}

/// A section's heading.
fn heading_cell(mtm: MainThreadMarker, title: &str) -> Retained<NSView> {
    let label = text(mtm, title, 11.0, &NSColor::secondaryLabelColor());
    label.setFont(Some(&NSFont::boldSystemFontOfSize(11.0)));
    fill(mtm, &label, 2.0)
}

/// The page when nothing matches.
fn empty_cell(mtm: MainThreadMarker) -> Retained<NSView> {
    let label = text(
        mtm,
        "No command matches.",
        13.0,
        &NSColor::secondaryLabelColor(),
    );
    fill(mtm, &label, 12.0)
}

/// `text` crossed out, in the secondary colour.
fn struck(text: &str, size: f64) -> Retained<NSAttributedString> {
    let single = NSNumber::new_isize(1);
    let color = NSColor::secondaryLabelColor();
    let font = NSFont::systemFontOfSize(size);
    let values: [&AnyObject; 3] = [single.as_ref(), color.as_ref(), font.as_ref()];
    // SAFETY: AppKit's three attribute keys (extern statics), each with the value type it
    // documents — an `NSNumber` (the single line), an `NSColor` and an `NSFont`.
    unsafe {
        let attributes = NSDictionary::<NSAttributedStringKey, AnyObject>::from_slices(
            &[
                NSStrikethroughStyleAttributeName,
                NSForegroundColorAttributeName,
                NSFontAttributeName,
            ],
            &values,
        );
        NSAttributedString::new_with_attributes(&NSString::from_str(text), &attributes)
    }
}

/// A line key that is built in: what it does and the key, struck through when a command took it.
fn built_in_cell(mtm: MainThreadMarker, line: &shortcuts::BuiltIn) -> Retained<NSView> {
    let title = text(mtm, line.title, 13.0, &NSColor::labelColor());
    let effect = text(mtm, line.effect, 11.0, &NSColor::secondaryLabelColor());
    let words = stack(
        mtm,
        &[&title, &effect],
        NSUserInterfaceLayoutOrientation::Vertical,
        1.0,
    );
    let key = text(mtm, &line.glyph, 13.0, &NSColor::secondaryLabelColor());
    if line.taken_by.is_some() {
        // The key is a command's now: the line no longer has it.
        key.setAttributedStringValue(&struck(&line.glyph, 13.0));
    }
    let mut right: Vec<Retained<NSView>> = Vec::new();
    if let Some(taker) = &line.taken_by {
        right.push(as_view(text(
            mtm,
            &format!("now {taker}’s"),
            11.0,
            &NSColor::secondaryLabelColor(),
        )));
    }
    right.push(as_view(key));
    let refs: Vec<&NSView> = right.iter().map(|view| &**view).collect();
    let keys = stack(
        mtm,
        &refs,
        NSUserInterfaceLayoutOrientation::Horizontal,
        8.0,
    );
    let top = row_line(mtm, &words, &keys);
    fill(mtm, &top, 6.0)
}

/// `view` in a table cell, inset.
fn fill(mtm: MainThreadMarker, view: &NSView, inset: f64) -> Retained<NSView> {
    let cell = NSTableCellView::new(mtm);
    view.setTranslatesAutoresizingMaskIntoConstraints(false);
    cell.addSubview(view);
    for constraint in [
        view.topAnchor()
            .constraintEqualToAnchor_constant(&cell.topAnchor(), inset),
        view.bottomAnchor()
            .constraintEqualToAnchor_constant(&cell.bottomAnchor(), -inset),
        view.leadingAnchor()
            .constraintEqualToAnchor_constant(&cell.leadingAnchor(), 4.0),
        view.trailingAnchor()
            .constraintEqualToAnchor_constant(&cell.trailingAnchor(), -4.0),
    ] {
        constraint.setActive(true);
    }
    Retained::into_super(cell)
}

#[cfg(test)]
mod tests {
    use bt_shell_common::palette::MenuNode;
    use bt_shell_common::shortcuts::Outcome;

    use super::*;

    /// A page over a small menu: Shell has Split Right and Split Down, Edit has Copy Last Output.
    fn state() -> State {
        let menu = vec![
            MenuNode::holder(
                0,
                "Shell",
                vec![
                    MenuNode::item(1, "Split Right", "splitRight:"),
                    MenuNode::item(2, "Split Down", "splitDown:"),
                ],
            ),
            MenuNode::holder(
                10,
                "Edit",
                vec![MenuNode::item(11, "Copy Last Output", "copyLastOutput:")],
            ),
        ];
        let mut st = State::new();
        st.resolved = shortcuts::resolve(&st.settings);
        st.page = Page::new(&menu, &st.resolved);
        st.entries = st.lines();
        st
    }

    #[test]
    fn the_lines_are_the_menus_sections_then_the_built_in_keys() {
        let st = state();
        let mut shown = st.entries.iter();
        assert_eq!(shown.next(), Some(&Entry::Heading("Shell".to_owned())));
        assert_eq!(shown.next(), Some(&Entry::Command("split_right")));
        assert_eq!(shown.next(), Some(&Entry::Command("split_down")));
        assert_eq!(shown.next(), Some(&Entry::Heading("Edit".to_owned())));
        assert_eq!(shown.next(), Some(&Entry::Command("copy_last_output")));
        assert_eq!(
            shown.next(),
            Some(&Entry::Heading(BUILT_IN_HEADING.to_owned()))
        );
        let built_in = shown
            .filter(|entry| matches!(entry, Entry::BuiltIn(_)))
            .count();
        assert_eq!(built_in, 6);
    }

    #[test]
    fn a_search_that_matches_nothing_is_one_line_that_says_so() {
        let mut st = state();
        st.query = "zzzz".to_owned();
        assert_eq!(st.lines(), [Entry::Empty]);
        // A filter hides the headings of the sections it empties.
        st.query.clear();
        st.filter = Filter::NoShortcut;
        assert_eq!(
            st.lines(),
            [
                Entry::Heading("Edit".to_owned()),
                Entry::Command("copy_last_output")
            ]
        );
    }

    #[test]
    fn a_key_finds_its_command_and_the_line_keys_find_the_built_in_list() {
        let mut st = state();
        let chord = |text: &str| match bt_core::Shortcut::parse(text) {
            Ok(bt_core::Shortcut::Chord(chord)) => chord,
            other => panic!("{other:?}"),
        };
        st.find = Find::Key(chord("cmd+d"));
        assert_eq!(
            st.lines(),
            [
                Entry::Heading("Shell".to_owned()),
                Entry::Command("split_right")
            ]
        );
        st.find = Find::Key(chord("cmd+left"));
        let lines = st.lines();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(matches!(lines[1], Entry::BuiltIn(_)));
    }

    #[test]
    fn what_the_user_was_doing_to_a_command_the_page_lost_is_dropped() {
        let mut st = state();
        st.listening = Some(("quit", 0));
        st.refused = Some(("quit", "no".to_owned()));
        st.pending = Some(Pending {
            name: "quit",
            outcome: Outcome {
                verdict: shortcuts::Verdict::Free,
                note: None,
                button: None,
                edits: Vec::new(),
            },
        });
        st.forget_gone();
        assert!(st.listening.is_none() && st.refused.is_none() && st.pending.is_none());
        // The commands the page has are kept.
        st.listening = Some(("split_right", 1));
        st.forget_gone();
        assert_eq!(st.listening, Some(("split_right", 1)));
    }

    #[test]
    fn a_tag_carries_the_line_and_the_slot() {
        let tag = usize::try_from(tag_of(17, 1)).expect("positive");
        assert_eq!((tag >> 2, tag & 3), (17, 1));
        let tag = usize::try_from(tag_of(0, 0)).expect("positive");
        assert_eq!((tag >> 2, tag & 3), (0, 0));
    }
}
