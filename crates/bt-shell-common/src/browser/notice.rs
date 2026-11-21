//! The release notice's offer and what bateri does with it: an offer is taken only when
//! it is a version bateri may run, speaks the protocol bateri speaks and is newer than what is
//! installed. Nothing here reads the network or the disk: the verdict is a function.

use super::version::Version;

/// The archive the notice names for this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    /// The file's name in the release (no directories).
    pub name: String,
    pub size: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: String,
}

/// A release notice, read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub version: Version,
    /// The oldest helper bateri may run, by the notice's word.
    pub minimum: Version,
    /// The Chromium the helper embeds, for the person (the engine's size and name in the sheet).
    pub chromium: String,
    pub protocol: u32,
    pub asset: Asset,
}

/// Why an offer is not taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The helper speaks another conversation than this bateri.
    Protocol { offered: u32, ours: u32 },
    /// The notice names a version below its own minimum: it was not made by the release.
    Inconsistent,
    /// Older than what is installed or than a minimum bateri has seen: an old notice put again
    /// (or a release that went back), never installed.
    Downgrade { offered: Version, floor: Version },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Newer than what is installed (or nothing is installed): install this.
    Install(Offer),
    /// What is installed is the offer's version.
    Current,
    Refused(Refusal),
}

/// What an offer comes to. `floor` is the highest minimum any notice has named and the highest
/// version installed (`Layout::floor`): nothing below it is ever installed. `installed` is the
/// newest version on disk.
pub fn judge(offer: Offer, protocol: u32, floor: &Version, installed: Option<&Version>) -> Verdict {
    if offer.protocol != protocol {
        return Verdict::Refused(Refusal::Protocol {
            offered: offer.protocol,
            ours: protocol,
        });
    }
    if offer.version < offer.minimum {
        return Verdict::Refused(Refusal::Inconsistent);
    }
    let floor = installed.map_or_else(|| floor.clone(), |have| have.max(floor).clone());
    if offer.version < floor {
        return Verdict::Refused(Refusal::Downgrade {
            offered: offer.version,
            floor,
        });
    }
    match installed {
        Some(have) if *have == offer.version => Verdict::Current,
        _ => Verdict::Install(offer),
    }
}

/// The platform's name in the notice's assets (`arm64`, `x86_64`).
pub fn arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        other => other,
    }
}

/// The offer a notice makes to this machine (`arch`): `None` when it has no archive for it.
#[cfg(target_os = "macos")]
pub fn offer(manifest: &browser_ipc::manifest::Manifest, arch: &str) -> Option<Offer> {
    let asset = manifest.asset(arch)?;
    Some(Offer {
        version: Version::parse(&manifest.version)?,
        minimum: Version::parse(&manifest.minimum)?,
        chromium: manifest.chromium.clone(),
        protocol: manifest.protocol,
        asset: Asset {
            name: asset.name.clone(),
            size: asset.size,
            sha256: asset.sha256.clone(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    fn offer(version: &str, minimum: &str, protocol: u32) -> Offer {
        Offer {
            version: v(version),
            minimum: v(minimum),
            chromium: "156.0".into(),
            protocol,
            asset: Asset {
                name: format!("bateri-browser-{version}-arm64.zip"),
                size: 10,
                sha256: "0".repeat(64),
            },
        }
    }

    #[test]
    fn a_newer_offer_is_installed_and_the_same_one_is_current() {
        let floor = v("0");
        assert!(matches!(
            judge(offer("0.2.0", "0.1.0", 1), 1, &floor, Some(&v("0.1.0"))),
            Verdict::Install(_)
        ));
        assert!(matches!(
            judge(offer("0.2.0", "0.1.0", 1), 1, &floor, None),
            Verdict::Install(_)
        ));
        assert_eq!(
            judge(offer("0.2.0", "0.1.0", 1), 1, &floor, Some(&v("0.2.0"))),
            Verdict::Current
        );
    }

    #[test]
    fn an_older_offer_than_the_installed_is_never_taken() {
        let verdict = judge(offer("0.1.0", "0.1.0", 1), 1, &v("0"), Some(&v("0.2.0")));
        assert_eq!(
            verdict,
            Verdict::Refused(Refusal::Downgrade {
                offered: v("0.1.0"),
                floor: v("0.2.0")
            })
        );
    }

    #[test]
    fn a_minimum_seen_once_holds_for_good() {
        // Nothing installed any more, but a notice once said 0.5 was the oldest to run.
        let verdict = judge(offer("0.4.0", "0.1.0", 1), 1, &v("0.5"), None);
        assert!(matches!(
            verdict,
            Verdict::Refused(Refusal::Downgrade { .. })
        ));
        assert!(matches!(
            judge(offer("0.5.0", "0.1.0", 1), 1, &v("0.5"), None),
            Verdict::Install(_)
        ));
    }

    #[test]
    fn a_protocol_bateri_does_not_speak_is_not_installed() {
        assert_eq!(
            judge(offer("9.0.0", "0.1.0", 2), 1, &v("0"), None),
            Verdict::Refused(Refusal::Protocol {
                offered: 2,
                ours: 1
            })
        );
    }

    #[test]
    fn a_version_below_its_own_minimum_is_not_a_release() {
        assert_eq!(
            judge(offer("0.1.0", "0.3.0", 1), 1, &v("0"), None),
            Verdict::Refused(Refusal::Inconsistent)
        );
    }

    #[test]
    fn versions_are_compared_as_numbers_in_the_rules() {
        assert!(matches!(
            judge(offer("10.0.0", "0.1.0", 1), 1, &v("0"), Some(&v("9.0.0"))),
            Verdict::Install(_)
        ));
    }
}
