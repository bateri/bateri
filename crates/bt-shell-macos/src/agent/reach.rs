//! A program that wants a terminal on another machine (`bateri split -S prod`) or work with one
//! (`bateri send p9@prod`): how it reaches the machine, and the pane that shows the terminal once
//! it runs there.
//!
//! Every program is answered `relay` and sends the command it means, which **this runs** over the
//! connection bateri keeps to that machine — the one place the app runs a program for another, for
//! a server given `full` ([`relay`]) — and says how it ended; a program on a computer that asks
//! for its own machine is answered `here`, and does it itself. The terminal a `split` started is
//! asked for afterwards (a `place` whose `server` is the same name). All of them ask here again
//! whether the machine is one bateri keeps terminals on: nothing the command says is trusted, and
//! the level of the machine that asked (`full`, [`serve::serve`]) was judged before any got this
//! far.
//!
//! They wait for something the main thread must not: `ssh -G` runs a program, the connection may
//! need a password the person types into the sheet of the pane the program runs in, and a relayed
//! `wait` lasts as long as the job. They run on a thread of their own ([`std::thread`]) and the
//! letter, which is `Send`, is answered from there (the conversation waits for the app's answer for
//! as long as it takes); the pane is made on the main queue, where the windows are.

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

/// `ssh`: how the server `name` is reached — `relay`, the word for the app running what the program
/// means over the connection bateri keeps to it ([`relay`]); when the machine that asks is no other
/// machine, `here` (the command does it itself). The sheet that asks for a password opens on the
/// pane the program runs in; run in no pane (a cron job) nothing asks, and a server that wants one
/// is refused.
pub(super) fn ssh(
    app: &AppDelegate,
    letter: Letter,
    server: &Server,
    from: Option<&str>,
    name: String,
) {
    let relayed = !local::is_account(&server.account);
    // The machine that asks is no other machine: there is nothing to reach, and the command does
    // it itself (`here`), on this computer as on the server (`serve` judged that it may, with no
    // level).
    if (!relayed && name == LOCAL_SERVER) || (relayed && server.host_name() == Some(name.as_str()))
    {
        letter.answer(Reply::Text(Reach::Here.text()).encode().into_bytes());
        return;
    }
    if name == LOCAL_SERVER {
        // This computer needs no login for a program on a server, which has it run here; it is as
        // `commands` lets it be.
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
            let reached = backend.reach(
                std::slice::from_ref(&name),
                ask,
                &hold_service::terminals_of,
            );
            letter.answer(reached_reply(reached).encode().into_bytes());
        });
    if let Err(error) = started {
        eprintln!("bateri: no thread to reach a server: {error}");
    }
}

/// What a server reached, or not, says to the program that asked.
fn reached_reply(reached: Result<Reached, String>) -> Reply {
    let reached = match reached {
        Ok(reached) => reached,
        Err(text) => return Reply::Failed(Failure::new(Kind::Failed, text)),
    };
    if reached.level == CommandLevel::Off {
        return Reply::Failed(run::refused_off(&reached.server));
    }
    Reply::Text(Reach::Relay.text())
}

/// `relay`: the `bateri` command a program on a `full` server means for another machine — a server
/// or this computer — run there, and how it ended. The machine is found again here (the connection
/// bateri keeps), the command is built from its words ([`run::command`]) and nothing else of what
/// the program said is run. It lasts as long as the job it waits for, and ends with the
/// conversation ([`run::serve`]).
pub(super) fn relay(
    app: &AppDelegate,
    letter: Letter,
    server: &Server,
    from: Option<&str>,
    relay: Relay,
) {
    let ask = from
        .and_then(|from| find_pane(app, &server.account, from))
        .map_or(Ask::Never, |(_, pane, _)| pane.command_ask(&relay.server));
    let local_level = app.settings().commands_level;
    let backend = hold_service::backend();
    let started = thread::Builder::new()
        .name("bateri relay".into())
        .spawn(move || {
            let gone = letter.gone();
            let reply = run::serve(
                backend,
                ask,
                local_level,
                &hold_service::terminals_of,
                &relay,
                &gone,
            );
            letter.answer(reply.encode().into_bytes());
        });
    if let Err(error) = started {
        eprintln!("bateri: no thread to relay a command: {error}");
    }
}

/// A `place` whose terminal runs on `place.server`: the server is found on a thread, then the
/// pane is made on the main queue beside the anchor — a terminal of this computer.
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
        letter.answer(Reply::Failed(run::refused_off(&host)).encode().into_bytes());
        return;
    }
    let mut world = World::new(app);
    let reply = serve::place_on(server, &target, from, place, &mut world);
    letter.answer(reply.encode().into_bytes());
    world.finish();
}
