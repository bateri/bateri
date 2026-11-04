//! A program on this Mac that wants a terminal on a server (`bateri split -S prod`): how it
//! reaches the server, and the pane that shows the terminal once it runs there.
//!
//! The program starts the terminal itself, over the `ssh` this answers with, and asks for the
//! pane afterwards (a `place` whose `server` is the same name). **Both ask here again whether the
//! server is one bateri keeps terminals on**: nothing the script says is trusted, and the level of
//! this Mac (`full`, [`serve::serve`]) was judged before either got this far.
//!
//! Both wait for something the main thread must not: `ssh -G` runs a program, and the connection
//! may need a password the person types into the sheet of the pane the program runs in. They run
//! on a thread of their own ([`std::thread`]) and the letter, which is `Send`, is answered from
//! there; the pane is made on the main queue, where the windows are.

use std::thread;

use bt_core::CommandLevel;
use bt_shell_common::agent::level::{self, Why};
use bt_shell_common::agent::message::{Failure, Kind, Place, Reply};
use bt_shell_common::agent::serve::{self, Server};
use bt_shell_common::kablo::service::{Letter, Reached};
use bt_shell_common::ssh_wrap::KabloHost;
use dispatch2::DispatchQueue;
use objc2::MainThreadMarker;

use super::{World, find_pane, refuse};
use crate::app::{self, AppDelegate};
use crate::hold_service;
use crate::ssh_route::Ask;

/// `ssh`: the words of the `ssh` command that ride the connection bateri keeps to the server
/// `name`, one `arg` line each. The sheet that asks for a password opens on the pane the program
/// runs in; run in no pane (a cron job) nothing asks, and a server that wants one is refused.
pub(super) fn ssh(
    app: &AppDelegate,
    letter: Letter,
    server: &Server,
    from: Option<&str>,
    name: String,
) {
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
        return Reply::Failed(refused_off(&reached.server));
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
        level::refusal(CommandLevel::Off, Why::Off, Some(&server.host)),
    )
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
    let Some(backend) = hold_service::backend() else {
        refuse(
            letter,
            Kind::Failed,
            "this bateri keeps no connection to servers",
        );
        return;
    };
    let started = thread::Builder::new()
        .name("bateri reach".into())
        .spawn(move || {
            let found = backend.route(std::slice::from_ref(&name));
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
        level: app.settings().commands_for(&host.host),
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
