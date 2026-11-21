//! A helper's version: dotted numbers, compared as numbers (`10` is after `9`), `1.0` and `1.0.0`
//! the same.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};

/// At most this many parts, each of at most nine digits: a version is a folder's name and goes
/// through arithmetic, never a surprise.
const MAX_PARTS: usize = 4;
const MAX_DIGITS: usize = 9;

#[derive(Clone, Debug)]
pub struct Version(Vec<u64>);

impl Version {
    /// `None` for anything that is not 1-4 numbers of up to nine digits joined by dots.
    pub fn parse(text: &str) -> Option<Version> {
        let parts: Option<Vec<u64>> = text
            .split('.')
            .map(|part| {
                (!part.is_empty()
                    && part.len() <= MAX_DIGITS
                    && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
            })
            .collect();
        let parts = parts?;
        (1..=MAX_PARTS)
            .contains(&parts.len())
            .then_some(Version(parts))
    }

    /// The version before any: what a folder with no floor in it is.
    pub fn zero() -> Version {
        Version(vec![0])
    }

    fn significant(&self) -> &[u64] {
        let end = self
            .0
            .iter()
            .rposition(|&part| part != 0)
            .map_or(0, |last| last + 1);
        &self.0[..end]
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.significant() == other.significant()
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.significant().cmp(other.significant())
    }
}

impl Hash for Version {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.significant().hash(state);
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self.0.iter().map(u64::to_string).collect();
        f.write_str(&parts.join("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn a_version_is_compared_as_numbers() {
        assert!(v("10.0.0") > v("9.9.9"));
        assert!(v("156.0.8078.100") > v("156.0.8078.99"));
        assert!(v("0.2") > v("0.1.9"));
        assert_eq!(v("1.0"), v("1.0.0"));
        assert!(v("0.0.1") > Version::zero());
    }

    #[test]
    fn what_is_not_a_version_is_none() {
        for text in [
            "",
            ".",
            "1..2",
            "1.2.3.4.5",
            "a",
            "1.a",
            "-1",
            "1.2-beta",
            "0123456789",
            "../1",
        ] {
            assert!(Version::parse(text).is_none(), "{text:?}");
        }
        assert_eq!(v("0.1.0").to_string(), "0.1.0");
    }
}
