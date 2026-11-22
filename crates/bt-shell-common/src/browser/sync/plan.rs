//! What a Mac does with what the home tells it when it joins a profile: which records of the
//! home it writes into its own profile, which of its own it sends to the home, which it removes.
//!
//! The rules are the plan's (Karar 3 and 4), in one place and with no I/O:
//!
//! - **The home wins what it knows.** A record the home sends is written into the profile, except
//!   one the profile already has with the same value (nothing to write) — and a record this
//!   Mac changed while the home was out of reach and the home changed too is the home's.
//! - **What the home has never heard of is sent.** A record of the profile that the home did not
//!   send and the Mac never took from it is new: it goes to the home with base 0. So a Mac
//!   that joins a home that is empty gives its profile to it, and no Mac's empty profile
//!   ever takes anything from the home.
//! - **A record the Mac took and has changed since** (its value is not the one it took) goes to
//!   the home on the number it took, and the home decides (compare and write).
//! - **A computer away longer than the home remembers deletions** (mode 2) drops what the home
//!   does not have, whether it took it before or not, and sends none of it, so that a deletion
//!   that was let go of is not brought back.

use std::collections::{HashMap, HashSet};

use browser_ipc::message::Record;
use kablo_proto::profile::{Digest, Entry, Mode};

use super::records::{Id, digest_of};
use super::store::Known;

/// A record of the Mac's own profile, with the sum of its value as the home would keep it.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalRec {
    pub id: Id,
    pub record: Record,
    pub digest: Digest,
}

#[derive(Debug, Default)]
pub struct Plan {
    /// The home's records to write into the profile (a deletion is one with no value).
    pub apply: Vec<Entry>,
    /// Records that are what the Mac has already: the number and sum are noted, nothing is written.
    pub same: Vec<Entry>,
    /// Records of the profile to send, each on the number the Mac took it at (0: new).
    pub send: Vec<(LocalRec, u64)>,
    /// Records of the profile to remove: the home does not have them any more.
    pub remove: Vec<LocalRec>,
}

/// The plan for a join: `entries` are what the home sent (all it has, or what is newer than the
/// Mac's `since`), `known` what the Mac took before, `local` what the profile holds now.
pub fn plan(entries: &[Entry], mode: Mode, known: &HashMap<Id, Known>, local: &[LocalRec]) -> Plan {
    let mut plan = Plan::default();
    let here: HashMap<&Id, &LocalRec> = local.iter().map(|l| (&l.id, l)).collect();
    let mut sent: HashSet<Id> = HashSet::new();
    for entry in entries {
        let id: Id = (entry.body.kind, entry.body.key.clone());
        sent.insert(id.clone());
        let have = here.get(&id);
        let same = match (entry.body.deleted, have) {
            (true, None) => true,
            (true, Some(_)) => false,
            (false, None) => false,
            (false, Some(l)) => l.digest == digest_of(&entry.body),
        };
        if same {
            plan.same.push(entry.clone());
        } else if let (Some(l), Some(k)) = (have, known.get(&id))
            && mode == Mode::Diff
            && k.applied >= entry.seq
        {
            // The home sent it again (what is newer than `since` comes again when an earlier
            // record was not applied) but the Mac took it already, and what differs is what the
            // Mac did to it since: that goes to the home on the number the Mac took, and the home
            // decides. The home's old copy is not written over it.
            plan.send.push(((*l).clone(), k.applied));
        } else {
            plan.apply.push(entry.clone());
        }
    }
    for l in local {
        if sent.contains(&l.id) {
            continue;
        }
        // The home sent the whole of what it has and this is not in it: it was deleted there (or
        // never was), and a deletion that may have been let go of must not be brought back.
        if mode == Mode::FullReplace {
            plan.remove.push(l.clone());
            continue;
        }
        match known.get(&l.id) {
            Some(k) if k.digest == l.digest => {}
            Some(k) => plan.send.push((l.clone(), k.applied)),
            None => plan.send.push((l.clone(), 0)),
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::sync::records::{id_of, record_digest, wire_of};
    use browser_ipc::message::CookieValue;
    use kablo_proto::profile::sum;

    fn cookie(name: &str, value: Option<&str>) -> Record {
        Record::Cookie {
            domain: "example.com".into(),
            path: "/".into(),
            name: name.into(),
            partition: None,
            value: value.map(|v| CookieValue {
                value: v.into(),
                expires: None,
                secure: false,
                http_only: false,
                same_site: 0,
                priority: 1,
                source_scheme: 0,
                source_port: -1,
            }),
        }
    }

    fn local(name: &str, value: &str) -> LocalRec {
        let record = cookie(name, Some(value));
        LocalRec {
            id: id_of(&record),
            digest: record_digest(&record, &[]),
            record,
        }
    }

    fn entry(seq: u64, record: &Record) -> Entry {
        Entry {
            seq,
            body: wire_of(record, &[]).body,
            writer: [9; 16],
            time: 0,
        }
    }

    fn known_of(l: &LocalRec, applied: u64) -> (Id, Known) {
        (
            l.id.clone(),
            Known {
                applied,
                digest: l.digest,
            },
        )
    }

    #[test]
    fn a_profile_that_is_empty_takes_all_the_home_has_and_gives_it_nothing() {
        let entries = [
            entry(1, &cookie("sid", Some("home"))),
            entry(2, &cookie("pref", Some("dark"))),
        ];
        let plan = plan(&entries, Mode::Full, &HashMap::new(), &[]);
        assert_eq!(plan.apply.len(), 2);
        assert!(plan.send.is_empty() && plan.remove.is_empty() && plan.same.is_empty());
    }

    #[test]
    fn what_the_home_does_not_know_is_sent_as_new_and_what_it_knows_is_the_homes() {
        let mine = [local("sid", "mac"), local("only-here", "x")];
        let entries = [entry(5, &cookie("sid", Some("home")))];
        let plan = plan(&entries, Mode::Full, &HashMap::new(), &mine);
        assert_eq!(plan.apply.len(), 1, "the home wins what it knows");
        assert_eq!(plan.send.len(), 1);
        assert_eq!(plan.send[0].0.id, mine[1].id);
        assert_eq!(plan.send[0].1, 0);
    }

    #[test]
    fn a_record_that_is_the_same_on_both_sides_is_not_written() {
        let mine = [local("sid", "same")];
        let entries = [entry(5, &cookie("sid", Some("same")))];
        let plan = plan(&entries, Mode::Full, &HashMap::new(), &mine);
        assert!(plan.apply.is_empty());
        assert_eq!(plan.same.len(), 1);
        assert!(plan.send.is_empty());
    }

    #[test]
    fn a_record_changed_here_while_the_home_was_away_goes_on_the_number_it_was_taken_at() {
        let before = local("sid", "old");
        let now = local("sid", "new");
        let known: HashMap<_, _> = [known_of(&before, 7)].into();
        // Nothing newer from the home: it is this Mac's change.
        let plan = plan(&[], Mode::Diff, &known, std::slice::from_ref(&now));
        assert_eq!(plan.send.len(), 1);
        assert_eq!(plan.send[0].1, 7);
        // Unchanged: nothing to say.
        let plan = super::plan(&[], Mode::Diff, &known, std::slice::from_ref(&before));
        assert!(plan.send.is_empty() && plan.apply.is_empty());
        // The home changed it too: the home wins.
        let entries = [entry(9, &cookie("sid", Some("theirs")))];
        let plan = super::plan(&entries, Mode::Diff, &known, &[now]);
        assert_eq!(plan.apply.len(), 1);
        assert!(plan.send.is_empty());
    }

    #[test]
    fn a_record_the_home_sends_again_that_the_mac_took_and_changed_is_the_macs_to_send() {
        // `since` lags behind a record that could not be applied, so an earlier one comes again.
        let taken = local("sid", "old");
        let now = local("sid", "changed while away");
        let known: HashMap<_, _> = [known_of(&taken, 7)].into();
        let again = [entry(7, &cookie("sid", Some("old")))];
        let plan = plan(&again, Mode::Diff, &known, std::slice::from_ref(&now));
        assert!(
            plan.apply.is_empty(),
            "the home's old copy is not written over it"
        );
        assert_eq!((plan.send.len(), plan.send[0].1), (1, 7));
        // A newer number is the home's change: the home wins, as before.
        let newer = [entry(8, &cookie("sid", Some("theirs")))];
        let plan = super::plan(&newer, Mode::Diff, &known, std::slice::from_ref(&now));
        assert_eq!((plan.apply.len(), plan.send.len()), (1, 0));
        // A whole state is another home's numbers: nothing is told by them.
        let plan = super::plan(&again, Mode::Full, &known, &[now]);
        assert_eq!((plan.apply.len(), plan.send.len()), (1, 0));
    }

    #[test]
    fn a_deletion_of_the_home_removes_what_the_profile_has_and_means_nothing_to_one_that_has_none()
    {
        let mine = [local("gone", "x")];
        let tombstone = entry(4, &cookie("gone", None));
        let plan = plan(
            std::slice::from_ref(&tombstone),
            Mode::Diff,
            &HashMap::new(),
            &mine,
        );
        assert_eq!(plan.apply.len(), 1);
        let plan = super::plan(&[tombstone], Mode::Diff, &HashMap::new(), &[]);
        assert!(plan.apply.is_empty());
        assert_eq!(plan.same.len(), 1, "its number is noted all the same");
    }

    #[test]
    fn a_computer_away_longer_than_deletions_are_remembered_drops_what_the_home_lacks() {
        let seen = local("took-before", "x");
        let never = local("never-sent", "y");
        let known: HashMap<_, _> = [known_of(&seen, 3)].into();
        let plan = plan(
            &[],
            Mode::FullReplace,
            &known,
            &[seen.clone(), never.clone()],
        );
        // Nothing of what the home lacks is sent, however it came to be here.
        assert_eq!(plan.send.len(), 0);
        let mut removed: Vec<_> = plan.remove.iter().map(|l| l.id.clone()).collect();
        removed.sort();
        let mut both = vec![seen.id.clone(), never.id.clone()];
        both.sort();
        assert_eq!(removed, both);
        let changed = local("took-before", "changed");
        let plan = super::plan(&[], Mode::FullReplace, &known, &[changed]);
        assert!(plan.send.is_empty());
        assert_eq!(plan.remove.len(), 1);
    }

    #[test]
    fn the_sum_of_a_record_does_not_depend_on_who_wrote_it_or_when() {
        let record = cookie("sid", Some("v"));
        let mut a = entry(1, &record);
        let mut b = entry(99, &record);
        a.writer = [1; 16];
        b.writer = [2; 16];
        b.time = 12345;
        assert_eq!(digest_of(&a.body), digest_of(&b.body));
        assert_ne!(digest_of(&a.body), sum(b""));
    }
}
