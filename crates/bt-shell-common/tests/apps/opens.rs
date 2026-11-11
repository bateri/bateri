//! The followed programs that say they open a file or a folder, as Settings ▸ Apps lists them and the
//! click's rule asks after them.

use std::fs;
use std::time::SystemTime;

use bt_core::pencere::apps::Opens;
use bt_core::pencere::{SUPPORTED_VERSIONS, offered_views};
use bt_shell_common::apps::opens::{Opener, openers};
use bt_shell_common::apps::{Config, store};

use super::support::{Author, Net, Release, TARGET, Temp};

const SITE: &str = "https://apps.example.test";

fn address(name: &str) -> String {
    format!("{SITE}/{name}/app.json")
}

/// A folder in which `releases` have landed, each followed from its own address.
fn followed(label: &str, releases: &[Release]) -> Temp {
    let net = Net::default();
    let root = Temp::new(label);
    let author = Author::new(7);
    let mut sources = Vec::new();
    for release in releases {
        release.publish(&net, &address(&release.name), &author);
        store::approve(root.path(), &release.name, &author.key()).expect("approved");
        sources.push(address(&release.name));
    }
    let config = Config {
        root: root.path().to_owned(),
        target: TARGET.to_owned(),
        protocols: SUPPORTED_VERSIONS.to_vec(),
        views: offered_views(),
        in_app_key: None,
    };
    let statuses = bt_shell_common::apps::check(&config, &sources, true, &net, SystemTime::now());
    assert_eq!(statuses.len(), releases.len());
    root
}

fn opener(name: &str, opens: &[Opens]) -> Opener {
    Opener {
        name: name.to_owned(),
        opens: opens.to_vec(),
    }
}

#[test]
fn a_program_is_listed_when_its_app_json_says_it_opens_a_file_or_a_folder() {
    let root = followed(
        "opens-listed",
        &[
            Release::new("zed", "1.0.0").opens(r#"["file","folder"]"#),
            Release::new("both-ways", "1.0.0").opens(r#"["folder","file","folder"]"#),
            Release::new("files-only", "1.0.0").opens(r#"["file"]"#),
            Release::new("silent", "1.0.0"),
            Release::new("empty", "1.0.0").opens("[]"),
            // A kind this version does not know is not one, and a program with none says nothing.
            Release::new("future", "1.0.0").opens(r#"["bundle"]"#),
        ],
    );
    assert_eq!(
        openers(root.path()),
        [
            opener("both-ways", &[Opens::Folder, Opens::File]),
            opener("files-only", &[Opens::File]),
            opener("zed", &[Opens::File, Opens::Folder]),
        ]
    );
}

#[test]
fn it_is_the_newest_release_that_landed_that_speaks() {
    let net = Net::default();
    let root = Temp::new("opens-newest");
    let author = Author::new(7);
    let at = address("tool");
    // The first release opens files; the next one does not say so any more.
    Release::new("tool", "1.0.0")
        .opens(r#"["file"]"#)
        .publish(&net, &at, &author);
    store::approve(root.path(), "tool", &author.key()).expect("approved");
    let config = Config {
        root: root.path().to_owned(),
        target: TARGET.to_owned(),
        protocols: SUPPORTED_VERSIONS.to_vec(),
        views: offered_views(),
        in_app_key: None,
    };
    let sources = [at.clone()];
    bt_shell_common::apps::check(&config, &sources, true, &net, SystemTime::now());
    assert_eq!(openers(root.path()), [opener("tool", &[Opens::File])]);

    Release::new("tool", "1.1.0").publish(&net, &at, &author);
    bt_shell_common::apps::check(&config, &sources, true, &net, SystemTime::now());
    assert_eq!(
        store::newest(root.path(), "tool").map(|v| v.to_string()),
        Some("1.1.0".to_owned())
    );
    assert_eq!(openers(root.path()), []);
}

#[test]
fn a_folder_that_cannot_vouch_for_its_file_says_nothing() {
    let root = followed(
        "opens-vouch",
        &[
            Release::new("tampered", "1.0.0").opens(r#"["file"]"#),
            Release::new("unsigned", "1.0.0").opens(r#"["file"]"#),
            Release::new("kept", "1.0.0").opens(r#"["file"]"#),
        ],
    );
    let release = |name: &str| {
        store::release_dir(
            root.path(),
            name,
            store::newest(root.path(), name).expect("landed"),
        )
    };
    // The file changed after it was checked: the signature is no longer the file's.
    let file = release("tampered").join("app.json");
    let text = fs::read_to_string(&file).expect("the file");
    fs::write(&file, text.replace("\"file\"", "\"folder\"")).expect("changed");
    fs::remove_file(release("unsigned").join("app.json.sig")).expect("removed");
    assert_eq!(openers(root.path()), [opener("kept", &[Opens::File])]);
}
