//! The other words a command is looked for by.
//!
//! The palette's commands are the menu's own (a row per menu item), so the words people
//! reach for that the menu's title does not carry — "zoom" for Bigger, "cls" for Clear to
//! Start, "sidebar" for Side — are kept here, **keyed by the item's selector** (the command's
//! identity, `cmd.<selector>`), not by its title: a title changes with the wording or the
//! language, the selector does not.
//!
//! What a word must not do is find a command it does not describe. "side" is not a keyword of
//! Split Right ("side by side" once was, and "side" then put Split Right above Tab Position);
//! "theme" is not one of Show Python Environment's.
//!
//! A command added to the menu adds its words here in the same change; one without an entry is
//! still found by its title.

/// Words by selector, the trailing colon left off (`splitRight`, not `splitRight:`).
const WORDS: &[(&str, &str)] = &[
    ("newTab", "open shell create"),
    ("newWindow", "open create"),
    ("newGroup", "folder session tabs collect"),
    ("newUngroupedTab", "loose plain open shell create"),
    ("newLocalTab", "mac open shell create"),
    ("splitRight", "vertical pane divide"),
    ("splitDown", "horizontal pane divide"),
    ("renameTab", "title name label"),
    ("renameGroup", "title name label folder"),
    ("detachTab", "window pop out detach"),
    ("mergeWindows", "combine join gather"),
    ("showTabList", "list overview all switch"),
    ("goToTab", "switch jump recent previous window"),
    ("goToFolder", "directory cd path open project change"),
    ("copyFolderPath", "directory pwd clipboard cwd"),
    (
        "jumpToCommand",
        "scroll output history previous block result go",
    ),
    (
        "copyLastCommand",
        "previous history clipboard pasteboard line",
    ),
    (
        "copyLastOutput",
        "previous result clipboard pasteboard text",
    ),
    ("showFolderInFinder", "reveal directory open"),
    ("moveGroupToNewWindow", "window pop out detach folder"),
    ("showNextTab", "switch"),
    ("showPreviousTab", "switch"),
    ("closeTab", "kill exit quit"),
    ("closeWindow", "kill exit quit"),
    ("findInScrollback", "search grep scrollback"),
    ("findNextMatch", "search"),
    ("findPreviousMatch", "search"),
    ("useSelectionForFind", "search selection"),
    ("clearToStart", "cls clear reset screen"),
    ("clearScrollback", "cls clear reset erase history buffer"),
    ("pasteEscaped", "quote paste escape"),
    ("undoMove", "back revert move"),
    ("makeFontBigger", "zoom in font increase larger size"),
    ("makeFontSmaller", "zoom out font decrease smaller size"),
    ("resetFontSize", "zoom reset font size"),
    ("scrollToTop", "home beginning"),
    ("scrollToBottom", "end latest"),
    ("scrollPageUp", "page"),
    ("scrollPageDown", "page"),
    ("openSettings", "preferences config options"),
    ("checkForUpdates", "upgrade version new"),
    ("relaunchWithHandover", "restart keep programs"),
    ("quitAndEndPrograms", "exit kill all"),
    ("tabsAtTop", "tab position bar horizontal"),
    ("tabsAtSide", "tab position sidebar vertical column left"),
    ("toggleSplitZoom", "maximize pane focus"),
    ("equalizeSplits", "balance even panes"),
    ("selectNextSplit", "pane focus"),
    ("selectPreviousSplit", "pane focus"),
    ("movePaneToNewTab", "pane detach split"),
    ("movePaneToNewWindow", "pane detach split"),
    ("moveTabToNewGroup", "folder collect"),
    ("markHost", "production staging development color host ssh"),
    ("forgetPassword", "keychain ssh login"),
    ("cancelUpload", "stop transfer"),
    ("matchSystemTheme", "auto macos appearance"),
    ("showWelcome", "intro tour onboarding help getting started"),
];

/// The extra words of the command with this selector (`splitRight` or `splitRight:`); empty
/// when it has none.
pub fn for_selector(selector: &str) -> &'static str {
    let selector = selector.trim_end_matches(':');
    WORDS
        .iter()
        .find(|(name, _)| *name == selector)
        .map_or("", |(_, words)| words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selector_finds_its_words_with_or_without_the_colon() {
        assert_eq!(for_selector("splitRight"), "vertical pane divide");
        assert_eq!(for_selector("splitRight:"), for_selector("splitRight"));
        assert_eq!(for_selector("noSuchSelector:"), "");
    }

    #[test]
    fn a_selector_is_listed_once() {
        for (n, (name, _)) in WORDS.iter().enumerate() {
            assert!(
                WORDS[n + 1..].iter().all(|(other, _)| other != name),
                "{name} twice"
            );
        }
    }

    #[test]
    fn split_right_is_not_found_by_side() {
        assert!(
            !for_selector("splitRight")
                .split_whitespace()
                .any(|w| w.starts_with("side"))
        );
    }
}
