//! Taking bateri off a server: Shell ▸ Remove bateri from “{host}”.
//!
//! Two steps over one channel, with the person's answer between them: **look**
//! ([`look`]) at what the machine runs, so the question can say how many terminals will end, and
//! **remove** ([`remove`]): end those terminals, then delete the folders bateri keeps its things
//! in. The terminals go first because it is the program in the folder that ends them.
//!
//! The one owner of the server's paths for this is [`script`]: bateri's folder
//! ([`kablo_bateri::paths::DIR`], which holds kablo, the shells' files and the programs), the one
//! it would be in when `XDG_DATA_HOME` is set, and the folder kablo had before it moved into
//! bateri's ([`kablo_proto::install::FORMER_ROOT`]). Nothing else on the machine is touched, and
//! nothing is removed when the home directory cannot be told from the root of the machine.
//!
//! **The look is of the whole machine** ([`census_script`](super::census_script)'s listing):
//! the terminals another computer of the person's started are the machine's too, and ending
//! kablo's folder takes them off their program, so the count the question gives is the
//! machine's. A look that is not a whole answer is no answer: nothing is removed from a machine
//! whose terminals are not known, because a terminal that is left running from a folder that
//! is gone cannot be reached again.

use std::time::Duration;

use kablo_bateri::paths;
use kablo_proto::SessionId;
use kablo_proto::install;

use super::{Census, Channel, census};

/// How long the removal may take: every terminal is asked to end at once, and one that does not
/// is made to after kablo's own wait of a few seconds.
pub const LIMIT: Duration = Duration::from_secs(60);

/// The shell words that set `k` to the kablo the machine can run: the one of `version` when it is
/// there, else the first of any version that answers `version` — a binary that is there and will
/// not run (the install fell over on it, a platform it is not built for) is no kablo, and no
/// terminal runs from it. `k` stays empty when there is none.
fn pick(version: &str) -> String {
    let (root, former) = (install::ROOT, install::FORMER_ROOT);
    let first = install::install_dir(version)
        .map(|dir| format!("{dir}/kablo "))
        .unwrap_or_default();
    format!(
        "k=$(for f in {first}{root}/*/kablo {former}/*/kablo; do \
         [ -x $f ] && $f version >/dev/null 2>&1 && {{ echo $f; break; }}; done)"
    )
}

/// The script that says what the machine has ([`Census`], the same answer a connection's look
/// gets): the directories that hold a kablo, [`census::MARK`], and the machine's terminals as the
/// kablo of `version` lists them ([`pick`]). A machine with no kablo that runs has no terminals:
/// `[]`. The status is that of the listing, so a listing that failed (a socket directory that is
/// not the user's own) is not an empty one. One line of `sh`, like the others.
pub fn look_script(version: &str) -> String {
    census::script(&format!(
        "{}; if [ -n \"$k\" ]; then $k list --json; else echo '[]'; fi",
        pick(version)
    ))
}

/// The machine as it is now, when the answer is a whole one ([`census::look`]).
pub fn look(channel: &dyn Channel, version: &str) -> Option<Census> {
    census::look(channel, &look_script(version))
}

/// The terminals of the machine that run: the ones [`script`] ends, and the ones the question
/// counts. One that has ended and is kept to be read is not one.
pub fn running(census: &Census) -> Vec<SessionId> {
    census
        .terminals
        .iter()
        .filter(|terminal| terminal.running)
        .map(|terminal| terminal.id.clone())
        .collect()
}

/// The script that ends the terminals `ids` and removes bateri's folders.
///
/// - It starts by going to the home directory, and it ends without having done anything when
///   there is none (`HOME` empty or `/`): the paths below are relative to it.
/// - Every terminal is asked to hang up at once with the kablo the machine has (the one of
///   `version` when it is there), and one that did not end is killed. An id that could be read as
///   an option (`-x`) is left out. With no terminal, kablo is not needed.
/// - Then the folders go (`rm -rf`, which removes a link and not what it points to). The status
///   is 0 when none of them is left, 1 when one is.
///
/// One line of `sh`, like the others.
pub fn script(version: &str, ids: &[SessionId]) -> String {
    let (dir, former) = (paths::DIR, install::FORMER_ROOT);
    let ids: Vec<&str> = ids
        .iter()
        .map(SessionId::as_str)
        .filter(|id| !id.starts_with('-'))
        .collect();
    let ending = if ids.is_empty() {
        String::new()
    } else {
        format!(
            "{}; if [ -n \"$k\" ]; then for i in {}; do ( $k end $i || $k end --force $i ) >/dev/null 2>&1 & done; wait; fi; ",
            pick(version),
            ids.join(" ")
        )
    };
    format!(
        "[ -n \"$HOME\" ] && [ \"$HOME\" != / ] && cd \"$HOME\" || exit 2; {ending}\
         rm -rf -- {dir} {former}; \
         case \"$XDG_DATA_HOME\" in /*) rm -rf -- \"$XDG_DATA_HOME/bateri\";; esac; \
         [ ! -e {dir} ] && [ ! -e {former} ]"
    )
}

/// Ends the terminals `ids` and removes bateri's folders from the machine ([`script`]). `Err` is a
/// line for the person: what the machine said, or that it said nothing.
pub fn remove(channel: &dyn Channel, version: &str, ids: &[SessionId]) -> Result<(), String> {
    let reply = channel
        .run(&script(version, ids), &[])
        .map_err(|error| format!("the connection broke: {error}"))?;
    match reply.status {
        Some(0) => Ok(()),
        Some(2) => {
            Err("the home directory of that machine is not one bateri may remove from".to_owned())
        }
        status => {
            let said = reply.err.trim();
            Err(if said.is_empty() {
                format!("the folders are still there (status {status:?})")
            } else {
                said.to_owned()
            })
        }
    }
}
