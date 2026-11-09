//! What the app answers when kablo, on a machine, asks about programs (`package state`, `package
//! offer`, `package fetch`, `package key-changed`: [`kablo_bateri::package`]).
//!
//! **The machine decides; the app answers, and goes to the internet only when asked.** What a
//! machine has, what it wants, which key it trusts for which program, whether a release is good
//! enough: all of it is kablo's, on the machine, which judges every release again before it is
//! put in use. The app is the machine's way to the internet and to the person's list of
//! programs. It follows three rules, which are what makes a server's request safe to answer:
//!
//! 1. **A source on this computer is for this computer's own account.** A `file:///` address, and
//!    an address that names a host on this machine or its network, are looked at only when the
//!    account that asks is this computer's own; a program on a server that asks for one is
//!    refused, so that no server reads a file of the person's or knocks at the router.
//! 2. **A server's sources are `https` and public.** Every address the answer would read — the
//!    `app.json`, its signature, the `previous` ones, the program's file — is judged so, by the
//!    [`Fetch`] the request is answered with.
//! 3. **Only bytes whose signature and sum hold go to a server.** The `app.json` is read with
//!    `pencere-host`'s reader, which checks the signature against the key the file names; the
//!    program's file is checked against the sum that file signs; what fails is not sent.
//!
//! Each account has a limit on how often the app goes to the internet for it ([`Limits`]); a
//! source asked about by ten servers is looked at once ([`Limits::keep`]). A program nobody
//! follows is answered too and nothing of it is kept: the file is downloaded to a folder of its
//! own, sent, and deleted.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use bt_core::app_source_address;
use bt_core::pencere::apps::{Version, is_name};
use kablo_bateri::message::{Failure, Kind, Reply};
use kablo_bateri::package::{Answer, Fetch as Bring, KeyChanged, Offer, Offered, Package, State};

use super::check::{self, Chosen};
use super::fetch::Fetch;
use super::{Config, store, upload};
use crate::kablo::install::Channel;

/// The longest a line of what is said to a machine is.
const SAID_MAX: usize = 300;

/// The longest a name of a machine's, or of a program on it, is in a notice.
const SAID_NAME: usize = 60;

/// Who asks: the account a letter came from, on the connection bateri keeps to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Who {
    pub account: String,
    /// The machine as the person typed it (`rasp`): what a notice about it says.
    pub host: String,
    /// Whether the account is this computer's own.
    pub local: bool,
}

/// How much the app goes to the internet for a machine.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// The span an account's `most` is counted in.
    pub window: Duration,
    /// The most the app goes to the internet for one account in a `window`: a look at a source
    /// that was not looked at lately, or a program's download. A card of the most rows
    /// (`card::ROWS_MAX`) asks for a look and a download of each, so it fits.
    pub most: usize,
    /// How long what was read of a source is kept for the next machine that asks.
    pub keep: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            window: Duration::from_secs(10 * 60),
            most: 100,
            keep: Duration::from_secs(5 * 60),
        }
    }
}

/// What a machine said it has (`package state`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineState {
    pub target: String,
    pub installed: Vec<String>,
    pub not_here: Vec<String>,
}

/// A program that stopped on a machine for a key (`package key-changed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyChange {
    pub account: String,
    /// The machine as the person typed it.
    pub host: String,
    pub name: String,
    pub key: String,
}

impl KeyChange {
    /// What the notification says: the title, and the body that names the command that decides.
    /// The name and the machine are the machine's words, so they are cut and cleaned.
    pub fn notice(&self) -> (String, String) {
        let say = |text: &str| -> String {
            text.chars()
                .filter(|c| !c.is_control())
                .take(SAID_NAME)
                .collect()
        };
        let (name, host) = (say(&self.name), say(&self.host));
        (
            format!("{name} stopped updating on {host}"),
            format!(
                "Its author\u{2019}s key changed, so the version there keeps running. Run bateri package update {name} on {host} to decide."
            ),
        )
    }
}

#[derive(Default)]
struct Inner {
    cache: HashMap<String, (Instant, Chosen)>,
    spent: HashMap<String, VecDeque<Instant>>,
    machines: HashMap<String, MachineState>,
    changes: Vec<KeyChange>,
    /// The (account, name, key) that were told of, so that a machine that says it again, as every
    /// start of its `watch` does, is not told of again. Bounded: a machine cannot grow it without
    /// end.
    told: HashSet<(String, String, String)>,
}

/// How many changes of key are remembered as told of.
const TOLD_MAX: usize = 256;

/// The app's answers to `package`.
pub struct Answerer {
    config: Config,
    fetch: Arc<dyn Fetch>,
    limits: Limits,
    inner: Mutex<Inner>,
}

impl Answerer {
    pub fn new(config: Config, fetch: Arc<dyn Fetch>) -> Self {
        Self::with_limits(config, fetch, Limits::default())
    }

    pub fn with_limits(config: Config, fetch: Arc<dyn Fetch>, limits: Limits) -> Self {
        Self {
            config,
            fetch,
            limits,
            inner: Mutex::new(Inner::default()),
        }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The reply to `package`, from `who`. `machine` is the way to write a file on the machine
    /// that asks, for a `fetch`. This waits for the internet and the machine: it is called on a
    /// thread of its own.
    pub fn answer(&self, who: &Who, package: Package, machine: &dyn Channel) -> Reply {
        let answered = match package {
            Package::State(state) => {
                self.said_state(who, state);
                return Reply::Done;
            }
            Package::KeyChanged(change) => {
                self.said_key_changed(who, &change);
                return Reply::Done;
            }
            Package::Offer(offer) => self.offer(who, &offer),
            Package::Fetch(bring) => self.bring(who, &bring, machine).map(|()| Answer::Brought),
        };
        match answered {
            Ok(answer) => Reply::Package(answer),
            Err(failure) => Reply::Failed(failure),
        }
    }

    /// What the machine of `account` last said it has, if it did.
    pub fn machine(&self, account: &str) -> Option<MachineState> {
        self.inner().machines.get(account).cloned()
    }

    /// The programs that stopped for a key since the last call, each once for as long as this
    /// bateri runs.
    pub fn take_key_changes(&self) -> Vec<KeyChange> {
        std::mem::take(&mut self.inner().changes)
    }

    /// Forgets what was read of the sources, so that the next ask looks again (Check Now).
    pub fn forget(&self) {
        self.inner().cache.clear();
    }

    fn said_state(&self, who: &Who, state: State) {
        self.inner().machines.insert(
            who.account.clone(),
            MachineState {
                target: state.target,
                installed: state.installed,
                not_here: state.not_here,
            },
        );
    }

    fn said_key_changed(&self, who: &Who, change: &KeyChanged) {
        // This computer's own key changes are decided on the Apps page, not told of; and a name
        // that is not a program's name is not the word of a kablo.
        if who.local || !is_name(&change.name) {
            return;
        }
        let mut inner = self.inner();
        if inner.told.len() >= TOLD_MAX {
            return;
        }
        let key = (who.account.clone(), change.name.clone(), change.key.clone());
        if inner.told.insert(key) {
            inner.changes.push(KeyChange {
                account: who.account.clone(),
                host: who.host.clone(),
                name: change.name.clone(),
                key: change.key.clone(),
            });
        }
    }

    /// The address a source stands for, and the source as the machine writes it down. The first
    /// two rules are here.
    fn resolve(&self, who: &Who, source: &str) -> Result<(String, String), Failure> {
        let (address, told) = match app_source_address(source) {
            Some(address) => (address, source.to_owned()),
            None if is_name(source) => {
                let address = store::followed(&self.config.root, source)
                    .ok_or_else(|| failed(format!("bateri follows no program called {source}")))?;
                (address.clone(), address)
            }
            None => {
                return Err(failed(format!(
                    "{} is not the address of an app.json, nor user/repo, nor a program bateri follows",
                    shown(source)
                )));
            }
        };
        if !who.local {
            if address.starts_with("file:") {
                return Err(refused(
                    "a source on the computer of the bateri app is for that computer's own programs, not for a server",
                ));
            }
            if !public(&address) {
                return Err(refused(
                    "a server's source must be an https address out on the internet",
                ));
            }
        }
        Ok((address, told))
    }

    /// The release of the source at `address` this bateri opens: from what was read lately, or
    /// from the internet.
    fn release(&self, who: &Who, address: &str) -> Result<Chosen, Failure> {
        // What this computer read with its own reach is not handed to a server from the cache.
        let key = format!("{}{address}", if who.local { "local:" } else { "server:" });
        {
            let inner = self.inner();
            if let Some((at, chosen)) = inner.cache.get(&key)
                && at.elapsed() < self.limits.keep
            {
                return Ok(chosen.clone());
            }
        }
        self.spend(&who.account)?;
        let chosen = if who.local {
            check::choose(&self.config, address, &*self.fetch)
        } else {
            check::choose(&self.config, address, &PublicOnly(&*self.fetch))
        }
        .map_err(failed)?;
        let mut inner = self.inner();
        let keep = self.limits.keep;
        inner.cache.retain(|_, (at, _)| at.elapsed() < keep);
        inner.cache.insert(key, (Instant::now(), chosen.clone()));
        Ok(chosen)
    }

    /// Counts one trip to the internet for `account`; refused when it has made too many lately.
    fn spend(&self, account: &str) -> Result<(), Failure> {
        let now = Instant::now();
        let mut inner = self.inner();
        let spent = inner.spent.entry(account.to_owned()).or_default();
        while spent
            .front()
            .is_some_and(|at| now.duration_since(*at) > self.limits.window)
        {
            spent.pop_front();
        }
        if spent.len() >= self.limits.most {
            return Err(refused(
                "bateri has gone to the internet for this machine too often just now: try again in a few minutes",
            ));
        }
        spent.push_back(now);
        Ok(())
    }

    fn offer(&self, who: &Who, offer: &Offer) -> Result<Answer, Failure> {
        let (address, told) = self.resolve(who, &offer.source)?;
        let chosen = self.release(who, &address)?;
        let manifest = &chosen.manifest;
        let have = offer.have.as_deref().and_then(Version::parse);
        if have.is_some_and(|have| manifest.version <= have) {
            return Ok(Answer::Current);
        }
        if !manifest.files.contains_key(&offer.target) {
            return Err(failed(super::Failure::NoFile(offer.target.clone())));
        }
        let text = |bytes: &[u8]| {
            String::from_utf8(bytes.to_vec()).map_err(|_| failed("the release is not text"))
        };
        Ok(Answer::Offered(Offered {
            source: told,
            manifest: text(&chosen.bytes)?,
            signature: text(&chosen.signature)?,
        }))
    }

    fn bring(&self, who: &Who, bring: &Bring, machine: &dyn Channel) -> Result<(), Failure> {
        let (address, _) = self.resolve(who, &bring.source)?;
        let version = Version::parse(&bring.version)
            .ok_or_else(|| failed(format!("{} is not a version", shown(&bring.version))))?;
        let chosen = self.release(who, &address)?;
        if chosen.manifest.name != bring.name || chosen.manifest.version != version {
            return Err(failed(
                "the source has another release now than the one that was offered: look again",
            ));
        }
        self.spend(&who.account)?;
        let fetch: &dyn Fetch = &*self.fetch;
        let (bytes, sum) = if who.local {
            check::program_file(&self.config, &address, &chosen, &bring.target, fetch)
        } else {
            check::program_file(
                &self.config,
                &address,
                &chosen,
                &bring.target,
                &PublicOnly(fetch),
            )
        }
        .map_err(failed)?;
        upload::put(machine, &bring.name, version, &sum, &bring.part, &bytes).map_err(failed)
    }
}

/// The network as a server is allowed to see it: public `https` addresses and nothing else.
struct PublicOnly<'a>(&'a dyn Fetch);

const NOT_PUBLIC: &str = "refused: not an https address out on the internet";

impl Fetch for PublicOnly<'_> {
    fn get(&self, address: &str, limit: u64) -> Result<Vec<u8>, String> {
        if public(address) {
            self.0.get(address, limit)
        } else {
            Err(NOT_PUBLIC.to_owned())
        }
    }

    fn download(&self, address: &str, limit: u64, path: &Path) -> Result<(), String> {
        if public(address) {
            self.0.download(address, limit, path)
        } else {
            Err(NOT_PUBLIC.to_owned())
        }
    }
}

/// Whether `address` is `https` and names a host out on the internet: not a name of this machine
/// or of a network behind it, and not an address of one (a loopback, private, link-local, shared
/// or unspecified one, in any of the forms an address is written in), and no login in it.
pub fn public(address: &str) -> bool {
    let Some(rest) = address.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.contains('@') || authority.is_empty() {
        return false;
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.split(':').next().unwrap_or_default(),
    };
    let lower = host.to_ascii_lowercase();
    if lower.is_empty()
        || lower == "localhost"
        || [".localhost", ".local", ".internal", ".lan", ".home.arpa"]
            .iter()
            .any(|suffix| lower.ends_with(suffix))
    {
        return false;
    }
    if let Ok(ip) = lower.parse::<IpAddr>() {
        return !private(ip);
    }
    // A name has dots and ends in letters: `127.1`, `2130706433` and `0x7f.1` are addresses that
    // `curl` reads, and none of them is a host out on the internet.
    let last = lower.rsplit('.').next().unwrap_or_default();
    lower.contains('.')
        && last.len() >= 2
        && last.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
        && !last.starts_with("0x")
}

/// Whether an address is one of this machine or a network behind it.
fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => private_v4(ip),
        IpAddr::V6(ip) => private_v6(ip),
    }
}

fn private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        // 100.64.0.0/10, shared address space.
        || (a == 100 && (64..128).contains(&b))
        // 0.0.0.0/8 and 240.0.0.0/4.
        || a == 0
        || a >= 240
}

fn private_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return private_v4(v4);
    }
    let [first, ..] = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // fc00::/7, unique local; fe80::/10, link local.
        || first & 0xfe00 == 0xfc00
        || first & 0xffc0 == 0xfe80
}

/// A line of what was asked, for a message: no control character, not long.
fn shown(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(80)
        .collect();
    clean
}

fn one_line(text: &str) -> String {
    let mut line: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(SAID_MAX)
        .collect();
    line.truncate(line.trim_end().len());
    line
}

fn failed(why: impl std::fmt::Display) -> Failure {
    Failure::new(Kind::Failed, one_line(&why.to_string()))
}

fn refused(why: &str) -> Failure {
    Failure::new(Kind::Refused, one_line(why))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_https_address_out_on_the_internet_is_public() {
        for address in [
            "https://github.com/bateri/textpen/releases/latest/download/app.json",
            "https://apps.example.org:8443/a/app.json",
            "https://[2606:4700::1111]/app.json",
            "https://8.8.8.8/app.json",
            "https://xn--bcher-kva.example/app.json",
        ] {
            assert!(public(address), "{address}");
        }
        for address in [
            "http://github.com/app.json",
            "file:///Users/a/app.json",
            "https://",
            "https:///app.json",
            "https://localhost/app.json",
            "https://LOCALHOST:8080/app.json",
            "https://app.localhost/app.json",
            "https://nas.local/app.json",
            "https://wiki.internal/app.json",
            "https://intranet/app.json",
            "https://127.0.0.1/app.json",
            "https://127.1/app.json",
            "https://0x7f000001/app.json",
            "https://2130706433/app.json",
            "https://0.0.0.0/app.json",
            "https://10.1.2.3/app.json",
            "https://172.16.0.1/app.json",
            "https://172.31.255.255/app.json",
            "https://192.168.0.1/app.json",
            "https://169.254.169.254/app.json",
            "https://100.64.0.1/app.json",
            "https://[::1]/app.json",
            "https://[::]/app.json",
            "https://[fe80::1]/app.json",
            "https://[fd00::1]/app.json",
            "https://[::ffff:192.168.1.1]/app.json",
            "https://me@example.org/app.json",
        ] {
            assert!(!public(address), "{address}");
        }
        // The neighbours of the private ranges are public.
        for address in [
            "https://172.15.0.1/app.json",
            "https://172.32.0.1/app.json",
            "https://100.63.0.1/app.json",
            "https://100.128.0.1/app.json",
        ] {
            assert!(public(address), "{address}");
        }
    }
}
