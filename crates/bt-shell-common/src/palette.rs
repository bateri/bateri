//! The command palette's rules: what a row is, how a query finds it, how the pages and the
//! keys move, and what a setting's value is.
//!
//! Everything here is pure and tried without a window; the toolkit draws the list and feeds the
//! machine ([`Palette`]) its keys. The Linux shell will draw the same palette from the same rules.
//!
//! - [`Row`], [`Kind`], [`Scope`] — a line of the list, as data.
//! - [`matching`] and [`rank`] — letters in order and word starts, keywords and second lines;
//!   the nearness of Recent and of the tab just left; the clusters and their fallbacks.
//! - [`menu_rows`] — the application menu read as commands, and [`root_list`] / [`page_list`] — the
//!   list a query leaves on a page, under its headings.
//! - [`Palette`] — the field, the highlight, the page stack and the preview, driven by [`Key`]s.
//! - [`catalog`] — the settings the palette changes, each value an edit the settings model owns.
//! - [`root_rows`], [`page_rows`] and [`chosen`] — what each page lists and what choosing a
//!   settings row writes; [`theme_entries`] — the themes with their colours.
//! - [`Recent`] and [`Visits`] — what was chosen last and which tab was looked at last.
//! - [`Places`], [`tab_list`] and [`act`] — the tabs and groups of every window, the page ⌘P
//!   opens, the menu of a row (→) and what choosing does.
//! - [`synonyms`] — the words a menu command is also found by, keyed by its selector.
//! - [`PathQuery`] — a path typed in the field.
//! - [`Commands`], [`history_list`] and [`history_run`] — the commands the open panes ran, the
//!   page Command History and Jump to Command… list, and what a key does with one ([`Use`]);
//!   [`suggested`] — what the empty root suggests.
//! - [`Targets`], [`go`] and [`path_list`] — the folders, servers and ports ⌘O lists, a typed
//!   path's entries, and what choosing each does ([`Go`]).
//!
//! The palette's commands are the **menu's own** (the shell reads the menu, so a command exists
//! in the palette exactly when it exists in the menu); the rows of the menu are built by the
//! shell, the rule that finds them is here.

mod catalog;
mod fold;
mod history;
mod list;
mod matching;
mod menu;
mod pages;
mod path;
mod rank;
mod recent;
mod row;
mod state;
mod switch;
mod synonyms;
mod targets;
mod themes;
mod visits;

pub use catalog::{
    Choice, Control, Section, Setting, all as settings, find as setting, rows as setting_rows,
};
pub use history::{
    CommandFacts, Commands, FullScreen, OTHER_TABS_HEADING, Run, SUGGESTED_HEADING,
    SUGGESTED_LIMIT, THIS_TAB_HEADING, Use, Uses, actions as history_actions,
    actions_heading as history_actions_heading, ago, command_row, command_rows, history_id,
    history_list, program_of, run as history_run, suggested, uses as history_uses, write_line,
};
pub use list::{
    Fallbacks, GO_TO_HEADING, List, RECENT_HEADING, page as page_list, root as root_list,
};
pub use matching::{Match, score};
pub use menu::{
    CHANGE_SHORTCUT, GO_TO_FOLDERS_ACTION, GO_TO_TABS_ACTION, JUMP_TO_COMMAND_ACTION, MenuNode,
    MenuRow, MenuRows, Modifiers, PALETTE_ACTION, SHOW_HISTORY_ACTION, SHOW_THEMES_ACTION,
    change_shortcut, command_actions, command_actions_heading, menu_item_id, rows as menu_rows,
    shortcut, shortcut_command,
};
pub use pages::{Chosen, Sources, chosen, page_rows, root_rows, suggested_rows};
pub use path::PathQuery;
pub use rank::{
    CLUSTER_LIMIT, Cluster, Hit, Nearness, RECENT_BONUS, THEME_LIMIT, VISIT_BONUS, VISIT_STEP,
    cluster, fallbacks, recent_rows, root, search, tabs,
};
pub use recent::{HEADER as RECENT_HEADER, LIMIT as RECENT_LIMIT, Recent};
pub use row::{Kind, Preview, Row, Scope, Slot, Status, Swatch, Tone, command_id};
pub use state::{Effect, Key, Modifier, Palette, REST};
pub use switch::{
    Act, EARLIER_HEADING, GROUPS_HEADING, GroupFacts, Places, RECENT_TABS, Report, TabFacts, act,
    actions as tab_actions, actions_heading, group_id, group_key, group_row, group_rows,
    listing_order, status as tab_status, tab_id, tab_keys, tab_list, tab_row, tab_rows,
};
pub use synonyms::for_selector as command_words;
pub use targets::{
    Beside, ENTRY_LIMIT, FOLDER_LIMIT, FolderFacts, Go, HOST_LIMIT, HostFacts, OpenFolder,
    PortFacts, Seen, Targets, beside, cd_line, expand as expand_home, folder_facts, folder_row, go,
    host_facts, host_row, list_dirs, mark_status, path_list, port_row, ssh_line, tilde,
};
pub use themes::{Group as ThemeGroup, ThemeEntry, entries as theme_entries};
pub use visits::Visits;

#[cfg(test)]
mod tests;
