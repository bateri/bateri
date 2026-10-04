//! A command line walked against a table of options: what each option
//! takes and what its value means, for the clients' and tools' guide bars
//! (`database`, `container`).
//!
//! **A table is the whole truth**: an option it does not know stops the
//! walk ([`Walk::unknown`]). Such an option may take the next argument as
//! its value — the arguments after it are then not what they seem — so the
//! caller shows less, never a guess.

/// What an option takes after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Takes {
    /// Nothing: a flag.
    Nothing,
    /// One value: attached where the client allows it (`-hdb`,
    /// `--host=db`), else the next argument.
    Value,
    /// A value only when attached (`-pSECRET`, `--password=SECRET`); alone
    /// it takes nothing (mysql's optional argument, MySQL booleans'
    /// `--reconnect=0`, pflag's `--dry-run`).
    Attached,
    /// The next `n` arguments (sqlite's `-lookaside SIZE N`).
    Values(usize),
    /// Every argument after it (sqlite's `-A`, redis-cli's `--cluster`).
    Rest,
}

/// One option: its spelling, what it takes and what its value is.
pub(super) type Opt<R> = (&'static str, Takes, R);

/// How a program reads its command line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dialect {
    /// getopt: `-abc` is `-a -b -c`, `-hdb` and `--host=db` attach a value,
    /// a value that is not attached is the next argument whatever it looks
    /// like, `--` ends the options (psql, mysql).
    Getopt,
    /// spf13/pflag, the Go command lines (kubectl, docker, podman):
    /// getopt's forms, and besides `-n=prod` attaches `prod` and a flag
    /// takes a written value (`--rm=false`, `-t=true`).
    Pflag,
    /// Each argument a whole option, its value the next argument (redis-cli,
    /// sqlite3).
    Whole,
    /// yargs (mongosh): `--host=db` and `-pSECRET` attach a value; a value
    /// that is not attached is the next argument only when that is not an
    /// option.
    Yargs,
}

/// A command line walked against a table ([`walk`]).
#[derive(Debug)]
pub(super) struct Walk<'a, R> {
    /// The known options met, in order, with their values.
    pub(super) options: Vec<(R, Option<&'a str>)>,
    /// The arguments that are not options, in order — only those before
    /// the first unknown option; after a `--` every argument is one.
    pub(super) positionals: Vec<&'a str>,
    /// How many of [`Self::positionals`] came before a `--`; `None` when
    /// there was none. A Go command's operands end there and the command it
    /// runs begins (`kubectl exec pod -- sh`).
    pub(super) before_dashes: Option<usize>,
    /// An option the table does not know (or one missing its value) was
    /// met and the walk stopped there: the target is unknown.
    pub(super) unknown: bool,
}

impl<R> Default for Walk<'_, R> {
    fn default() -> Self {
        Self {
            options: Vec::new(),
            positionals: Vec::new(),
            before_dashes: None,
            unknown: false,
        }
    }
}

impl<R: PartialEq> Walk<'_, R> {
    pub(super) fn has(&self, role: R) -> bool {
        self.options.iter().any(|(seen, _)| *seen == role)
    }

    /// The positionals before a `--`: a Go command's operands.
    pub(super) fn operands(&self) -> &[&str] {
        let end = self.before_dashes.unwrap_or(self.positionals.len());
        self.positionals.get(..end).unwrap_or_default()
    }
}

/// Walks `args` (argv without argv[0]) in `dialect`, `lookup` naming each
/// option; every program here reads its options wherever they stand
/// (getopt and pflag permute, the others loop over the whole line).
pub(super) fn walk<'a, R: Copy>(
    args: &'a [String],
    dialect: Dialect,
    lookup: fn(&str) -> Option<(Takes, R)>,
) -> Walk<'a, R> {
    walk_from(args, dialect, lookup, false).0
}

/// [`walk`] for a command that stops reading options at its first operand
/// (docker's `run IMAGE COMMAND…`, `exec CONTAINER COMMAND…`): the options
/// before it and the arguments from the operand on — `None` when the walk
/// stopped at an unknown option or there is no operand.
pub(super) fn walk_to_operand<'a, R: Copy>(
    args: &'a [String],
    dialect: Dialect,
    lookup: fn(&str) -> Option<(Takes, R)>,
) -> (Walk<'a, R>, Option<&'a [String]>) {
    walk_from(args, dialect, lookup, true)
}

fn walk_from<'a, R: Copy>(
    args: &'a [String],
    dialect: Dialect,
    lookup: fn(&str) -> Option<(Takes, R)>,
    to_operand: bool,
) -> (Walk<'a, R>, Option<&'a [String]>) {
    let mut walk = Walk::default();
    let mut index = 0;
    let mut ended = false;
    while let Some(arg) = args.get(index) {
        if ended || arg == "-" || !arg.starts_with('-') {
            if to_operand {
                return (walk, args.get(index..));
            }
            walk.positionals.push(arg);
            index += 1;
            continue;
        }
        index += 1;
        if arg == "--" && dialect != Dialect::Whole {
            ended = true;
            walk.before_dashes = Some(walk.positionals.len());
            continue;
        }
        let next = args.get(index..).unwrap_or_default();
        let step = match dialect {
            Dialect::Getopt => getopt(arg, next, lookup, &mut walk.options, false),
            Dialect::Pflag => getopt(arg, next, lookup, &mut walk.options, true),
            Dialect::Whole => whole(arg, next, lookup, &mut walk.options),
            Dialect::Yargs => yargs(arg, next, lookup, &mut walk.options),
        };
        match step {
            Some(consumed) => index += consumed,
            None => {
                walk.unknown = true;
                break;
            }
        }
    }
    (walk, None)
}

/// One getopt (or, with `pflag`, pflag) argument: a long option or a
/// cluster of short ones; the count of following arguments taken, `None`
/// for an unknown option or a missing value.
fn getopt<'a, R: Copy>(
    arg: &'a str,
    next: &'a [String],
    lookup: fn(&str) -> Option<(Takes, R)>,
    out: &mut Vec<(R, Option<&'a str>)>,
    pflag: bool,
) -> Option<usize> {
    if let Some(long) = arg.strip_prefix("--") {
        let (name, attached) = match long.split_once('=') {
            Some((name, value)) => (name, Some(value)),
            None => (long, None),
        };
        let (takes, role) = lookup(&format!("--{name}"))?;
        return match (takes, attached) {
            // pflag's flag with a written value: `--rm=false`.
            (Takes::Nothing, Some(_)) if pflag => {
                out.push((role, None));
                Some(0)
            }
            (Takes::Nothing, Some(_)) | (Takes::Values(_) | Takes::Rest, _) => None,
            (Takes::Nothing, None) => {
                out.push((role, None));
                Some(0)
            }
            (Takes::Value, Some(value)) => {
                out.push((role, Some(value)));
                Some(0)
            }
            (Takes::Value, None) => {
                out.push((role, Some(next.first()?)));
                Some(1)
            }
            (Takes::Attached, value) => {
                out.push((role, value));
                Some(0)
            }
        };
    }
    let cluster = &arg[1..];
    for (at, letter) in cluster.char_indices() {
        // The rest of the cluster after this letter: its value, if it
        // takes one — never another option's text.
        let rest = &cluster[at + letter.len_utf8()..];
        // pflag reads `-n=prod` as `-n prod`, and `-t=false` as the flag's
        // written value.
        let equals = pflag.then(|| rest.strip_prefix('=')).flatten();
        match lookup(&format!("-{letter}"))? {
            (Takes::Nothing, role) => {
                out.push((role, None));
                if equals.is_some() {
                    return Some(0);
                }
            }
            (Takes::Value, role) if !rest.is_empty() => {
                out.push((role, Some(equals.unwrap_or(rest))));
                return Some(0);
            }
            (Takes::Value, role) => {
                out.push((role, Some(next.first()?)));
                return Some(1);
            }
            (Takes::Attached, role) => {
                let value = equals.or((!rest.is_empty()).then_some(rest));
                out.push((role, value));
                return Some(0);
            }
            (Takes::Values(_) | Takes::Rest, _) => return None,
        }
    }
    Some(0)
}

/// One whole-argument option ([`Dialect::Whole`]).
fn whole<'a, R: Copy>(
    arg: &'a str,
    next: &'a [String],
    lookup: fn(&str) -> Option<(Takes, R)>,
    out: &mut Vec<(R, Option<&'a str>)>,
) -> Option<usize> {
    let (takes, role) = lookup(arg)?;
    match takes {
        Takes::Nothing => {
            out.push((role, None));
            Some(0)
        }
        Takes::Value => {
            out.push((role, Some(next.first()?)));
            Some(1)
        }
        Takes::Values(count) => {
            let values = next.get(..count)?;
            out.push((role, values.first().map(String::as_str)));
            Some(count)
        }
        Takes::Rest => {
            out.push((role, None));
            Some(next.len())
        }
        Takes::Attached => None,
    }
}

/// One yargs option ([`Dialect::Yargs`]).
fn yargs<'a, R: Copy>(
    arg: &'a str,
    next: &'a [String],
    lookup: fn(&str) -> Option<(Takes, R)>,
    out: &mut Vec<(R, Option<&'a str>)>,
) -> Option<usize> {
    let (name, attached, short) = match arg.strip_prefix("--") {
        Some(long) => match long.split_once('=') {
            Some((name, value)) => (format!("--{name}"), Some(value), false),
            None => (arg.to_owned(), None, false),
        },
        None => {
            let mut letters = arg[1..].chars();
            let letter = letters.next()?;
            let rest = letters.as_str();
            (
                format!("-{letter}"),
                (!rest.is_empty()).then_some(rest),
                true,
            )
        }
    };
    let (takes, role) = lookup(&name)?;
    match (takes, attached) {
        // `-qv`: a cluster of booleans, which no table here spells.
        (Takes::Nothing, Some(_)) if short => None,
        (Takes::Nothing, _) => {
            out.push((role, None));
            Some(0)
        }
        (Takes::Value | Takes::Attached, Some(value)) => {
            out.push((role, Some(value)));
            Some(0)
        }
        (Takes::Value, None) => match next.first().filter(|value| !value.starts_with('-')) {
            Some(value) => {
                out.push((role, Some(value)));
                Some(1)
            }
            None => {
                out.push((role, None));
                Some(0)
            }
        },
        (Takes::Attached, None) => {
            out.push((role, None));
            Some(0)
        }
        (Takes::Values(_) | Takes::Rest, _) => None,
    }
}

/// `name`'s row in `table`.
pub(super) fn row<R: Copy>(table: &[Opt<R>], name: &str) -> Option<(Takes, R)> {
    table
        .iter()
        .find(|(spelling, _, _)| *spelling == name)
        .map(|&(_, takes, role)| (takes, role))
}
