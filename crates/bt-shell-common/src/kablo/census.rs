//! What a look at a machine says of kablo on it: the versions that are installed and the
//! terminals that run.
//!
//! The look is one answer, whole or nothing ([`Census::parse`]): the directories of
//! [`kablo_proto::install::ROOT`] and [`kablo_proto::install::FORMER_ROOT`] that hold a binary
//! (each as its path under the home directory, one per line), [`MARK`], and then what `kablo
//! list --json` says — **all** of the machine's terminals, this user's, whoever started them. A
//! listing that was cut short or is not an array of terminals is no answer: nothing is known of a
//! machine then, and nothing is removed, renewed or forgotten by it.
//!
//! Two decisions are made of one: which terminals are renewed with the kablo this computer brings
//! ([`Census::renewable`]) and which versions nothing runs from any more
//! ([`super::install::unused`]). The server's own list is also what says that a terminal the
//! ledger holds is gone ([`Census::has`]).

use kablo_proto::SessionId;
use kablo_proto::install;

use super::install::Channel;

/// The line between the directories and the listing in a look's answer.
pub const MARK: &str = "---";

/// The script of a look: the directories of both roots that hold a kablo, [`MARK`], then
/// `listing`, the shell words that print the machine's terminals as `kablo list --json` does. The
/// one owner of the roots' loop: a look that is told by a connection and one that is made to take
/// bateri off a machine read the same answer ([`Census::parse`]). One line of `sh`, like the
/// others.
pub fn script(listing: &str) -> String {
    let (root, former) = (install::ROOT, install::FORMER_ROOT);
    format!(
        "for r in {root} {former}; do for d in $r/*/; do [ -x $d/kablo ] && echo ${{d%/}}; done; done; \
         echo {MARK}; {listing}"
    )
}

/// The machine as it is now, as the look `script` ([`script`]) says it, when the answer is a
/// whole one: a script that failed is no answer.
pub fn look(channel: &dyn Channel, script: &str) -> Option<Census> {
    let reply = channel.run(script, &[]).ok()?;
    if reply.status != Some(0) {
        return None;
    }
    Census::parse(&reply.out)
}

/// A terminal of the machine, as `kablo list --json` says it (the keys that are not named here
/// are not read).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terminal {
    pub id: SessionId,
    /// It runs: not one that ended and is kept to be read, nor one that does not answer.
    pub running: bool,
    /// How many clients are attached to it. A listing that does not say (no real one) is taken
    /// for so many that nobody could tell the terminal is idle.
    pub clients: u32,
    /// The release its relay runs from.
    pub version: String,
    /// The release its warden runs from.
    pub warden_version: String,
}

/// What a look at the machine found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Census {
    /// The directories of installed versions, each as its path under the home directory.
    pub dirs: Vec<String>,
    /// The terminals of the machine.
    pub terminals: Vec<Terminal>,
}

impl Census {
    /// Reads a look's answer: the directories, [`MARK`], the listing. `None` for an answer that
    /// is not a whole one.
    pub fn parse(out: &str) -> Option<Census> {
        let (dirs, listing) = out.split_once(&format!("{MARK}\n"))?;
        Some(Census {
            dirs: dirs
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
            terminals: Reader::new(listing).terminals()?,
        })
    }

    /// The machine has a terminal of this name.
    pub fn has(&self, id: &SessionId) -> bool {
        self.terminals.iter().any(|terminal| terminal.id == *id)
    }

    /// Some terminal runs its relay or its warden from the release `name`.
    pub fn runs(&self, name: &str) -> bool {
        self.terminals
            .iter()
            .any(|terminal| terminal.version == name || terminal.warden_version == name)
    }

    /// The terminals to renew with the kablo of `package` (the version of the one this computer
    /// brings, and has put on the machine), in the order the listing has them.
    ///
    /// - A terminal that does not run, or runs the package's release in both halves, is not one.
    /// - One that runs a release **newer** than the package's is never moved back, and one whose
    ///   version cannot be read is not touched.
    /// - A package that is a release moves only releases, and only those of the ledger (`kept`:
    ///   the terminals this application keeps on the machine) or that nobody is attached to: a
    ///   terminal another person or another computer has open is theirs, and renews itself.
    ///   One that runs a build for development is never moved by a release.
    /// - A package that is a build for development moves only the ledger's.
    pub fn renewable(&self, kept: &[SessionId], package: &str) -> Vec<SessionId> {
        let Some(package_numbers) = Release::of(package) else {
            return Vec::new();
        };
        self.terminals
            .iter()
            .filter(|terminal| terminal.running)
            .filter(|terminal| terminal.version != package || terminal.warden_version != package)
            .filter(|terminal| {
                let (Some(relay), Some(warden)) = (
                    Release::of(&terminal.version),
                    Release::of(&terminal.warden_version),
                ) else {
                    return false;
                };
                let newer = relay.numbers > package_numbers.numbers
                    || warden.numbers > package_numbers.numbers;
                let ours = kept.contains(&terminal.id);
                if newer {
                    false
                } else if package_numbers.development {
                    ours
                } else {
                    !relay.development && !warden.development && (ours || terminal.clients == 0)
                }
            })
            .map(|terminal| terminal.id.clone())
            .collect()
    }
}

/// A version as a release or a build for development: `0.9.1` or `0.9.1-dev+3f2a9c1d0b44`.
struct Release {
    numbers: [u64; 3],
    development: bool,
}

impl Release {
    fn of(version: &str) -> Option<Release> {
        let core = version.split(['-', '+']).next()?;
        let mut parts = core.split('.');
        let mut numbers = [0u64; 3];
        for number in &mut numbers {
            *number = parts.next()?.parse().ok()?;
        }
        parts.next().is_none().then_some(Release {
            numbers,
            development: core.len() != version.len(),
        })
    }
}

/// A reader of `kablo list --json`: an array of objects, one per terminal. Only the keys
/// [`Terminal`] has are read; the others (some are strings or objects of their own) are skipped
/// whole, so what a program or a label says cannot be taken for a key.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

/// A value this reader keeps.
enum Value {
    Text(String),
    Number(u64),
    Other,
}

impl<'a> Reader<'a> {
    fn new(text: &'a str) -> Reader<'a> {
        Reader {
            bytes: text.as_bytes(),
            at: 0,
        }
    }

    fn terminals(mut self) -> Option<Vec<Terminal>> {
        self.space();
        self.expect(b'[')?;
        let mut terminals = Vec::new();
        self.space();
        if self.peek() == Some(b']') {
            self.at += 1;
        } else {
            loop {
                self.space();
                terminals.push(self.terminal()?);
                self.space();
                match self.take()? {
                    b',' => {}
                    b']' => break,
                    _ => return None,
                }
            }
        }
        self.space();
        (self.at == self.bytes.len()).then_some(terminals)
    }

    fn terminal(&mut self) -> Option<Terminal> {
        self.expect(b'{')?;
        let (mut id, mut state, mut clients) = (None, None, None);
        let (mut version, mut warden_version) = (None, None);
        self.space();
        if self.peek() == Some(b'}') {
            return None;
        }
        loop {
            self.space();
            let key = self.text(true)?;
            self.space();
            self.expect(b':')?;
            self.space();
            let wanted = matches!(
                key.as_str(),
                "id" | "state" | "clients" | "version" | "warden_version"
            );
            let value = self.value(wanted)?;
            match (key.as_str(), value) {
                ("id", Value::Text(text)) => id = SessionId::new(&text).ok(),
                ("state", Value::Text(text)) => state = Some(text),
                ("clients", Value::Number(number)) => clients = Some(number),
                ("version", Value::Text(text)) => version = Some(text),
                ("warden_version", Value::Text(text)) => warden_version = Some(text),
                _ => {}
            }
            self.space();
            match self.take()? {
                b',' => {}
                b'}' => break,
                _ => return None,
            }
        }
        Some(Terminal {
            id: id?,
            running: state.is_none_or(|state| state == "running"),
            clients: clients.map_or(u32::MAX, |clients| {
                u32::try_from(clients).unwrap_or(u32::MAX)
            }),
            version: version?,
            warden_version: warden_version?,
        })
    }

    /// The value at the reader: kept when `keep`, else only walked past.
    fn value(&mut self, keep: bool) -> Option<Value> {
        match self.peek()? {
            b'"' => {
                let text = self.text(keep)?;
                Some(if keep {
                    Value::Text(text)
                } else {
                    Value::Other
                })
            }
            b'{' | b'[' => {
                self.skip_nested()?;
                Some(Value::Other)
            }
            _ => {
                let start = self.at;
                while self
                    .peek()
                    .is_some_and(|byte| !matches!(byte, b',' | b'}' | b']' | b' ' | b'\n'))
                {
                    self.at += 1;
                }
                let word = std::str::from_utf8(&self.bytes[start..self.at]).ok()?;
                if word.is_empty() {
                    return None;
                }
                Some(match word.parse::<u64>() {
                    Ok(number) => Value::Number(number),
                    Err(_) => Value::Other,
                })
            }
        }
    }

    /// A string; its text is built only when `keep` (the escapes of the ones that are not wanted
    /// are walked past).
    fn text(&mut self, keep: bool) -> Option<String> {
        self.expect(b'"')?;
        let mut text = Vec::new();
        loop {
            match self.take()? {
                b'"' => break,
                b'\\' => {
                    let escaped = self.take()?;
                    let byte = match escaped {
                        b'"' | b'\\' | b'/' => escaped,
                        b'n' => b'\n',
                        b't' => b'\t',
                        b'r' => b'\r',
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'u' => {
                            let hex = self.bytes.get(self.at..self.at + 4)?;
                            self.at += 4;
                            let code =
                                u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                            if keep {
                                let mut buffer = [0u8; 4];
                                // A half of a surrogate pair is no character: a name that has one
                                // is not one kablo made.
                                text.extend_from_slice(
                                    char::from_u32(code)?.encode_utf8(&mut buffer).as_bytes(),
                                );
                            }
                            continue;
                        }
                        _ => return None,
                    };
                    if keep {
                        text.push(byte);
                    }
                }
                byte => {
                    if keep {
                        text.push(byte);
                    }
                }
            }
        }
        String::from_utf8(text).ok()
    }

    /// Walks past an object or an array (and what is nested in it).
    fn skip_nested(&mut self) -> Option<()> {
        let mut depth = 0usize;
        loop {
            match self.peek()? {
                b'"' => {
                    self.text(false)?;
                    continue;
                }
                b'{' | b'[' => depth += 1,
                b'}' | b']' => depth -= 1,
                _ => {}
            }
            self.at += 1;
            if depth == 0 {
                return Some(());
            }
        }
    }

    fn space(&mut self) {
        while self
            .peek()
            .is_some_and(|byte| matches!(byte, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn take(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }

    fn expect(&mut self, byte: u8) -> Option<()> {
        (self.take()? == byte).then_some(())
    }
}

#[cfg(test)]
mod tests;
