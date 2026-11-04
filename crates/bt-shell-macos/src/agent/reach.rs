//! A program that wants a terminal on another machine (`bateri split -S prod`) or work with one
//! (`bateri send p9@prod`): how it reaches the machine, and the pane that shows the terminal once
//! it runs there.
//!
//! A program on this Mac starts the terminal itself, over the `ssh` this answers with, and asks for
//! the pane afterwards (a `place` whose `server` is the same name). A program on a server has no
//! `ssh` to run: it is answered `relay` and sends the command it means, which **this runs** — the
//! one place a window runs a program, for a server given `full` ([`relay`]). All of
//! them ask here again whether the machine is one bateri keeps terminals on: nothing the script says
//! is trusted, and the level of the machine that asked (`full`, [`serve::serve`]) was judged before
//! any got this far.
//!
//! They wait for something the main thread must not: `ssh -G` runs a program, the connection may
//! need a password the person types into the sheet of the pane the program runs in, and a relayed
//! `wait` lasts as long as the job. They run on a thread of their own ([`std::thread`]) and the
//! letter, which is `Send`, is answered from there (the conversation waits for the application's
//! answer for as long as it takes); the pane is made on the main queue, where the windows are.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use bt_core::CommandLevel;
use bt_shell_common::agent::LOCAL_SERVER;
use bt_shell_common::agent::level::{self, Why};
use bt_shell_common::agent::message::{Failure, Kind, Place, Reach, Relay, Reply};
use bt_shell_common::agent::relay as run;
use bt_shell_common::agent::serve::{self, Server};
use bt_shell_common::kablo::local;
use bt_shell_common::kablo::service::{Letter, Reached};
use bt_shell_common::ssh_wrap::KabloHost;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;

use super::{World, find_pane, refuse};
use crate::app::{self, AppDelegate};
use crate::hold_service;
use crate::ssh_route::Ask;

/// `ssh`: the words of the `ssh` command that ride the connection bateri keeps to the server
/// `name`, one `arg` line each; to a program on a server, which cannot run it, the word `relay`
/// once the connection is made (the window runs what the program means to, [`relay`]); the machine
/// that asks is no other machine, and the word is `here` (the command does it itself). The sheet
/// that asks for a password opens on the pane the program runs in; run in no pane (a cron job)
/// nothing asks, and a server that wants one is refused.
pub(super) fn ssh(
    app: &AppDelegate,
    letter: Letter,
    server: &Server,
    from: Option<&str>,
    name: String,
) {
    let relayed = !local::is_account(&server.account);
    // The machine that asks is no other machine: there is nothing to reach, and the command does
    // it itself (`here`), on this Mac as on the server (`serve` judged that it may, with no level).
    if (!relayed && name == LOCAL_SERVER) || (relayed && server.host_name() == Some(name.as_str()))
    {
        letter.answer(Reply::Text(Reach::Here.text()).encode().into_bytes());
        return;
    }
    if name == LOCAL_SERVER {
        // This Mac needs no connection for a program on a server, which has it run there; it is
        // as `commands` lets it be.
        let reply = match app.settings().commands_level {
            CommandLevel::Off => Reply::Failed(Failure::new(
                Kind::Refused,
                level::refusal(CommandLevel::Off, Why::Off, None),
            )),
            _ => Reply::Text(Reach::Relay.text()),
        };
        letter.answer(reply.encode().into_bytes());
        return;
    }
    let Some(backend) = hold_service::backend() else {
        refuse(
            letter,
            Kind::Failed,
            "this bateri keeps no connection to servers",
        );
        return;
    };
    let ask = from
        .and_then(|from| find_pane(app, &server.account, from))
        .map_or(Ask::Never, |(_, pane, _)| pane.command_ask(&name));
    let started = thread::Builder::new()
        .name("bateri reach".into())
        .spawn(move || {
            let reached = backend.reach(std::slice::from_ref(&name), ask);
            letter.answer(reached_reply(reached, relayed).encode().into_bytes());
        });
    if let Err(error) = started {
        eprintln!("bateri: no thread to reach a server: {error}");
    }
}

/// What a server reached, or not, says to the program that asked.
fn reached_reply(reached: Result<Reached, String>, relayed: bool) -> Reply {
    let reached = match reached {
        Ok(reached) => reached,
        Err(text) => return Reply::Failed(Failure::new(Kind::Failed, text)),
    };
    if reached.level == CommandLevel::Off {
        return Reply::Failed(refused_off(&reached.server));
    }
    if relayed {
        return Reply::Text(Reach::Relay.text());
    }
    let mut words = String::new();
    for word in &reached.argv {
        words.push_str("arg ");
        words.push_str(word);
        words.push('\n');
    }
    Reply::Text(words)
}

/// A server whose `commands` are off: refused, with the setting that changes it.
fn refused_off(server: &KabloHost) -> Failure {
    Failure::new(
        Kind::Refused,
        level::refusal(
            CommandLevel::Off,
            Why::Off,
            (!server.host.is_empty()).then_some(server.host.as_str()),
        ),
    )
}

/// How many relayed commands run at once: each holds a thread, a process and a connection until the
/// job it waits for ends, and a program that asks for more is not asking for a person's work.
const MOST_RELAYS: usize = 8;

static RELAYS: AtomicUsize = AtomicUsize::new(0);

/// One of the places for a running relay, given back when it drops.
struct Slot;

impl Slot {
    fn take() -> Option<Slot> {
        RELAYS
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |running| {
                (running < MOST_RELAYS).then_some(running + 1)
            })
            .ok()
            .map(|_| Slot)
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        RELAYS.fetch_sub(1, Ordering::AcqRel);
    }
}

/// `relay`: the `bateri` command a program on a `full` server means for another machine — a server
/// or this Mac — run there, and how it ended. The machine is found again here (the connection
/// bateri keeps, or a shell on this Mac), the command is built from its words
/// ([`run::remote_line`]) and nothing else of what the program said is run.
pub(super) fn relay(
    app: &AppDelegate,
    letter: Letter,
    server: &Server,
    from: Option<&str>,
    relay: Relay,
) {
    let Some(slot) = Slot::take() else {
        refuse(
            letter,
            Kind::Failed,
            "too many commands are relayed at once: wait for one to end",
        );
        return;
    };
    let ask = from
        .and_then(|from| find_pane(app, &server.account, from))
        .map_or(Ask::Never, |(_, pane, _)| pane.command_ask(&relay.server));
    let local_level = app.settings().commands_level;
    let backend = hold_service::backend();
    let started = thread::Builder::new()
        .name("bateri relay".into())
        .spawn(move || {
            let _slot = slot;
            let reply = relayed(backend.as_deref(), ask, local_level, &relay);
            letter.answer(reply.encode().into_bytes());
        });
    if let Err(error) = started {
        eprintln!("bateri: no thread to relay a command: {error}");
    }
}

/// The reply to a relay: the command run on the machine it names, or why it was not.
fn relayed(
    backend: Option<&dyn bt_shell_common::kablo::service::Backend>,
    ask: Ask,
    local_level: CommandLevel,
    relay: &Relay,
) -> Reply {
    let line = run::remote_line(&run::not_set_up(&relay.server), &relay.words);
    let command = if relay.server == LOCAL_SERVER {
        if local_level == CommandLevel::Off {
            return Reply::Failed(Failure::new(
                Kind::Refused,
                level::refusal(CommandLevel::Off, Why::Off, None),
            ));
        }
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return Reply::Failed(Failure::new(Kind::Failed, "this Mac has no home directory"));
        };
        run::local_command(&home, &line)
    } else {
        let Some(backend) = backend else {
            return Reply::Failed(Failure::new(
                Kind::Failed,
                "this bateri keeps no connection to servers",
            ));
        };
        match backend.reach(std::slice::from_ref(&relay.server), ask) {
            Err(text) => return Reply::Failed(Failure::new(Kind::Failed, text)),
            Ok(reached) if reached.level == CommandLevel::Off => {
                return Reply::Failed(refused_off(&reached.server));
            }
            Ok(reached) => run::ssh_command("ssh", &reached.argv, &line),
        }
    };
    match run::execute(command, run::TIME, run::OUTPUT) {
        Ok(relayed) => Reply::Relayed(relayed),
        Err(text) => Reply::Failed(Failure::new(Kind::Failed, text)),
    }
}

/// A `place` whose terminal runs on `place.server`: the server is found on a thread, then the
/// pane is made on the main queue beside the anchor — a terminal of this Mac.
pub(super) fn place_on(letter: Letter, server: Server, from: Option<String>, place: Place) {
    let Some(name) = place.server.clone() else {
        refuse(
            letter,
            Kind::Usage,
            "`server` names the server the terminal runs on",
        );
        return;
    };
    let backend = hold_service::backend();
    if backend.is_none() && name != LOCAL_SERVER {
        refuse(
            letter,
            Kind::Failed,
            "this bateri keeps no connection to servers",
        );
        return;
    }
    let started = thread::Builder::new()
        .name("bateri reach".into())
        .spawn(move || {
            let found = match &backend {
                _ if name == LOCAL_SERVER => Some(local::host()),
                Some(backend) => backend.route(std::slice::from_ref(&name)),
                None => None,
            };
            DispatchQueue::main().exec_async(move || {
                // audit: a block running on the main queue is on the main thread by definition.
                let mtm = MainThreadMarker::new().expect("the main queue is the main thread");
                match app::delegate(mtm) {
                    Some(app) => {
                        placed(&app, letter, &server, from.as_deref(), &place, &name, found)
                    }
                    None => letter.hang_up(),
                }
            });
        });
    if let Err(error) = started {
        eprintln!("bateri: no thread to find a server: {error}");
    }
}

fn placed(
    app: &AppDelegate,
    letter: Letter,
    server: &Server,
    from: Option<&str>,
    place: &Place,
    name: &str,
    found: Option<KabloHost>,
) {
    let Some(host) = found else {
        refuse(
            letter,
            Kind::Failed,
            &format!("{name} is not a server bateri keeps terminals on"),
        );
        return;
    };
    let target = Server {
        level: if local::is_account(&host.key) {
            app.settings().commands_level
        } else {
            app.settings().commands_for(&host.host)
        },
        account: host.key.clone(),
        host: host.host.clone(),
        argv: host.argv.clone(),
    };
    if target.level == CommandLevel::Off {
        letter.answer(Reply::Failed(refused_off(&host)).encode().into_bytes());
        return;
    }
    let mut world = World::new(app);
    let reply = serve::place_on(server, &target, from, place, &mut world);
    letter.answer(reply.encode().into_bytes());
    world.finish();
}
