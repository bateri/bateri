//! The font options of the Font page: a switch for each option the selected font offers.
//!
//! The list is the font's own ([`bt_shell_common::font_options`], the palette reads the same
//! one): Ligatures and Hide Secrets first, then the font's stylistic sets and variants under the
//! names it gives them, and Slashed Zero. A font that offers none (Menlo) hides the group — the
//! window does, from the options it asks for — and changing the font rebuilds the switches, so the
//! options on screen are always the selected font's.
//!
//! A switch keeps nothing of its own: it shows the file's choice, and flipping it writes the
//! option's edits and comes back from the file like every other control of the window. A refresh
//! that finds the same options only sets the switches' state and enabled flag, so the control
//! being clicked is not replaced under the pointer; the switches are rebuilt only when the list
//! of options changes.

use std::cell::RefCell;

use bt_core::FontOptions;
use bt_shell_common::font_options::FontOption;
use objc2::rc::Retained;
use objc2::{ClassType, MainThreadMarker, MainThreadOnly, Message, sel};
use objc2_app_kit::{
    NSLayoutAttribute, NSStackView, NSSwitch, NSTextField, NSUserInterfaceLayoutOrientation, NSView,
};
use objc2_foundation::{NSArray, NSInteger, NSString};

use super::{SettingsWindow, set_switch};

/// The vertical space between two options, in points.
const SPACING: f64 = 6.0;

/// The Options row's control: the stack its switches are in.
pub(super) struct Options {
    stack: Retained<NSStackView>,
    /// The options on screen, by id, and the switch of each, in the stack's order.
    shown: RefCell<Vec<(String, Retained<NSSwitch>)>>,
}

impl Options {
    /// The empty stack the row's control is, before there is anything to show.
    pub(super) fn stack(mtm: MainThreadMarker) -> Retained<NSStackView> {
        let stack = NSStackView::stackViewWithViews(&NSArray::new(), mtm);
        stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
        stack.setAlignment(NSLayoutAttribute::Leading);
        stack.setSpacing(SPACING);
        stack
    }

    pub(super) fn new(stack: Retained<NSStackView>) -> Self {
        Self {
            stack,
            shown: RefCell::new(Vec::new()),
        }
    }

    /// The stack, as the row's control view.
    pub(super) fn view(&self) -> Retained<NSView> {
        let view: &NSView = &self.stack;
        view.retain()
    }

    /// Shows a switch for each of `options`, on as `font`'s choices say; each switch's tag is
    /// the option's place in `options`, which the window keeps for the action. The same options as
    /// last time only update the switches in place.
    pub(super) fn show(
        &self,
        window: &SettingsWindow,
        options: &[FontOption],
        font: &FontOptions,
        enabled: bool,
    ) {
        let mut shown = self.shown.borrow_mut();
        let same = shown.len() == options.len()
            && shown
                .iter()
                .zip(options)
                .all(|((id, _), option)| id == option.id());
        if !same {
            for view in self.stack.arrangedSubviews().iter() {
                self.stack.removeArrangedSubview(&view);
                view.removeFromSuperview();
            }
            shown.clear();
            let mtm = window.mtm();
            for (at, option) in options.iter().enumerate() {
                let switch = NSSwitch::new(mtm);
                switch.setTag(NSInteger::try_from(at).unwrap_or_default());
                // SAFETY: the target is a weak reference and the window lives for the whole
                // process; the selector is the window's own action.
                unsafe {
                    switch.setTarget(Some(window.target()));
                    switch.setAction(Some(sel!(fontOptionChanged:)));
                }
                let label = NSTextField::labelWithString(&NSString::from_str(option.title()), mtm);
                let line = NSStackView::stackViewWithViews(
                    &NSArray::from_slice(&[
                        switch.as_super().as_super(),
                        label.as_super().as_super(),
                    ]),
                    mtm,
                );
                line.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
                line.setSpacing(8.0);
                let line: Retained<NSView> = Retained::into_super(line);
                self.stack.addArrangedSubview(&line);
                shown.push((option.id().to_owned(), switch));
            }
        }
        for ((_, switch), option) in shown.iter().zip(options) {
            set_switch(switch, option.is_on(font));
            switch.setEnabled(enabled);
        }
    }
}
