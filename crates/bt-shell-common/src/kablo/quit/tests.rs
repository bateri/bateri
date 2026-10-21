use super::Event::{ConnectionLost, Crash, Quit, QuitAndEnd, Update};
use super::Fate::{Ends, EndsAtTheNextLaunch, Keeps};
use super::*;

/// Every cell of the table, as it is written in the module's doc and in the settings'
/// documentation: nothing is left to a default arm.
#[test]
fn every_cell_of_the_table_is_what_the_user_was_told() {
    use KeepRunning::{Crash as OnCrash, Quit as OnQuit, Update as OnUpdate};
    let table = [
        (OnUpdate, [Keeps, Keeps, EndsAtTheNextLaunch, Ends, Ends]),
        (OnCrash, [Keeps, Keeps, Keeps, Ends, Ends]),
        (OnQuit, [Keeps, Keeps, Keeps, Keeps, Ends]),
    ];
    for (keep, cells) in table {
        for (event, expected) in [ConnectionLost, Update, Crash, Quit, QuitAndEnd]
            .into_iter()
            .zip(cells)
        {
            assert_eq!(fate(keep, event), expected, "{keep:?} × {event:?}");
        }
    }
}

/// What a lost connection does is nothing, in every row; that is what makes a terminal on a
/// server worth having.
#[test]
fn a_lost_connection_and_an_update_never_end_a_terminal() {
    for (_, keep) in KeepRunning::NAMES {
        assert_eq!(fate(*keep, ConnectionLost), Keeps, "{keep:?}");
        assert_eq!(fate(*keep, Update), Keeps, "{keep:?}");
    }
}

/// The names the settings accept are exactly the rows above: a value added to the setting is
/// a row of the table, and this fails until it is written there.
#[test]
fn the_table_has_a_row_for_every_value_of_the_setting() {
    assert_eq!(KeepRunning::NAMES.len(), 3);
}

#[test]
fn a_launch_ends_what_a_crash_left_only_where_the_table_says_and_only_if_nothing_came_over() {
    assert!(ends_leftovers(KeepRunning::Update, false));
    assert!(!ends_leftovers(KeepRunning::Update, true));
    for keep in [KeepRunning::Crash, KeepRunning::Quit] {
        assert!(!ends_leftovers(keep, false), "{keep:?}");
        assert!(!ends_leftovers(keep, true), "{keep:?}");
    }
}
