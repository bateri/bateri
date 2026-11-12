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
//! "theme" is not one of Python Environment's.
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
    ("quitAndEndPrograms", "exit kill all"),
    ("tabsAtTop", "tab position bar horizontal"),
    ("tabsAtSide", "tab position sidebar vertical column left"),
    (
        "toggleSplitZoom",
        // "focus" first: it is what people call it, and the first word ranks a command over
        // the others the word finds (the commands that move the keyboard between panes).
        "focus maximize pane split fullscreen expand enlarge restore",
    ),
    (
        "toggleFloatPane",
        "pane dock undock overlay window popup detach",
    ),
    ("newFloatingPane", "pane overlay window popup"),
    (
        "minimizeFloatingPane",
        "pane float overlay window popup hide capsule",
    ),
    (
        "toggleFloatingPanes",
        "pane float overlay window popup hide show capsule minimize",
    ),
    ("equalizeSplits", "balance even panes"),
    ("selectNextSplit", "pane focus"),
    ("selectPreviousSplit", "pane focus"),
    ("selectSplit", "pane focus arrow"),
    ("resizeSplit", "pane size grow shrink"),
    ("swapSplit", "pane exchange"),
    ("moveFloatingPane", "pane float overlay window popup nudge"),
    ("movePaneToNewTab", "pane detach split"),
    ("movePaneToNewWindow", "pane detach split"),
    ("movePaneToPreviousTab", "pane"),
    ("movePaneToNextTab", "pane"),
    ("moveTabToNewGroup", "folder collect"),
    ("markHost", "production staging development color host ssh"),
    (
        "openAllBackground",
        "background terminals server remote running",
    ),
    ("forgetPassword", "keychain ssh login"),
    ("cancelUpload", "stop transfer"),
    ("matchSystemTheme", "auto macos appearance"),
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
    fn every_command_on_a_split_is_found_by_pane() {
        // The menu says "split" (Select Split ▸ Left), people type "pane".
        for selector in [
            "splitRight",
            "splitDown",
            "selectPreviousSplit",
            "selectNextSplit",
            "selectSplit",
            "resizeSplit",
            "swapSplit",
            "equalizeSplits",
            "toggleSplitZoom",
            "toggleFloatPane",
            "newFloatingPane",
            "minimizeFloatingPane",
            "toggleFloatingPanes",
            "moveFloatingPane",
            "movePaneToNewTab",
            "movePaneToNewWindow",
            "movePaneToPreviousTab",
            "movePaneToNextTab",
        ] {
            assert!(
                for_selector(selector)
                    .split_whitespace()
                    .any(|w| w.starts_with("pane")),
                "{selector}"
            );
        }
    }

    #[test]
    fn zoom_pane_is_found_by_focus_and_by_the_name_it_had() {
        // People call it focusing a pane; the menu called it Zoom Split.
        let words = for_selector("toggleSplitZoom");
        for word in ["focus", "split", "maximize"] {
            assert!(words.split_whitespace().any(|w| w == word), "{word}");
        }
        // It is the first word there and of no other command: "focus" ranks it over the
        // commands that move the keyboard between panes.
        assert_eq!(words.split_whitespace().next(), Some("focus"));
        for other in ["selectNextSplit", "selectPreviousSplit", "selectSplit"] {
            let words = for_selector(other);
            assert!(words.split_whitespace().any(|w| w == "focus"), "{other}");
            assert_ne!(words.split_whitespace().next(), Some("focus"), "{other}");
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
