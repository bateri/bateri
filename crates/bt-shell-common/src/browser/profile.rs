//! A profile's key and what the helper is given for it.
//!
//! The key has three parts: the **machine** (where the program runs: the Mac or one of kablo's
//! servers), the **application** (the name the program gave itself) and the **name** the page
//! asked for. Two keys that differ in any part are two profiles with their own cookies and storage:
//! a `localhost:3000` of one server never shares a login or a cache with the one of another. A
//! profile with no name is not kept: it lives in the helper's memory and goes with its browser.
//!
//! The folder name a key gets is a hash of its three parts, each length-prefixed (the application
//! comes from the program, so `a` + `bc` and `ab` + `c` must not meet), with the readable bits
//! in front for a person who opens the data folder. The helper takes `[A-Za-z0-9._-]{1,64}` and
//! nothing that starts with a dot.

use std::fmt;

use crate::digest::sha256_hex;

/// The longest name a page may give (`[a-z0-9-]`, the kit's rule).
pub const NAME_MAX: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ProfileKey {
    machine: String,
    application: String,
    name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// A name of `[a-z0-9-]`, 1-32 characters, or none.
    Name,
    /// A machine or an application that is empty.
    Empty,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::Name => f.write_str("a profile's name is 1-32 of a-z, 0-9 and -"),
            KeyError::Empty => f.write_str("a profile needs a machine and an application"),
        }
    }
}

/// Whether `name` is one a page may give.
pub fn valid_name(name: &str) -> bool {
    (1..=NAME_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl ProfileKey {
    pub fn new(machine: &str, application: &str, name: Option<&str>) -> Result<Self, KeyError> {
        if machine.is_empty() || application.is_empty() {
            return Err(KeyError::Empty);
        }
        if name.is_some_and(|name| !valid_name(name)) {
            return Err(KeyError::Name);
        }
        Ok(Self {
            machine: machine.to_owned(),
            application: application.to_owned(),
            name: name.map(str::to_owned),
        })
    }

    pub fn machine(&self) -> &str {
        &self.machine
    }

    pub fn application(&self) -> &str {
        &self.application
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Whether the profile is kept on disk.
    pub fn persistent(&self) -> bool {
        self.name.is_some()
    }

    /// The folder under the helper's root, `None` for a profile that is not kept.
    pub fn dir(&self) -> Option<String> {
        let name = self.name.as_deref()?;
        let mut parts = String::new();
        for part in [&self.machine, &self.application, name] {
            parts.push_str(&part.len().to_string());
            parts.push(':');
            parts.push_str(part);
        }
        let digest = &sha256_hex(parts.as_bytes())[..16];
        Some(format!("{}-{name}-{digest}", slug(&self.application)))
    }
}

/// The application's name in a folder's name: lower-case letters and digits, 12 at the most.
fn slug(application: &str) -> String {
    let slug: String = application
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(12)
        .collect();
    if slug.is_empty() {
        "app".to_owned()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(machine: &str, application: &str, name: &str) -> String {
        ProfileKey::new(machine, application, Some(name))
            .unwrap()
            .dir()
            .unwrap()
    }

    #[test]
    fn the_key_is_machine_application_and_name() {
        let base = dir("mac", "browserpen", "work");
        assert_eq!(base, dir("mac", "browserpen", "work"));
        assert_ne!(base, dir("server:prod", "browserpen", "work"));
        assert_ne!(base, dir("mac", "textpen", "work"));
        assert_ne!(base, dir("mac", "browserpen", "home"));
    }

    #[test]
    fn parts_that_run_together_are_still_apart() {
        assert_ne!(dir("a", "bc", "x"), dir("ab", "c", "x"));
        assert_ne!(dir("a:1", "b", "x"), dir("a", "1:b", "x"));
    }

    #[test]
    fn the_folder_is_a_name_the_helper_takes() {
        let long = "a".repeat(200);
        for application in [
            "browserpen",
            "My App!",
            "../../etc",
            "ünï",
            long.as_str(),
            ".",
        ] {
            let folder = dir("server:prod.example.com", application, &"n".repeat(32));
            assert!((1..=64).contains(&folder.len()), "{folder}");
            assert!(!folder.starts_with('.'), "{folder}");
            assert!(
                folder
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)),
                "{folder}"
            );
        }
    }

    #[test]
    fn a_profile_without_a_name_is_kept_nowhere() {
        let key = ProfileKey::new("mac", "browserpen", None).unwrap();
        assert!(!key.persistent());
        assert_eq!(key.dir(), None);
    }

    #[test]
    fn names_and_parts_that_are_not_allowed() {
        let long = "a".repeat(33);
        for name in ["", "UPPER", "a b", "a/b", "ü", long.as_str(), ".."] {
            assert_eq!(
                ProfileKey::new("mac", "x", Some(name)),
                Err(KeyError::Name),
                "{name:?}"
            );
        }
        assert_eq!(ProfileKey::new("", "x", None), Err(KeyError::Empty));
        assert_eq!(ProfileKey::new("mac", "", None), Err(KeyError::Empty));
    }
}
