//! Tries the trackpad's vibration on this Mac, by hand: what the vibration of a page is played
//! with, without a page.
//!
//! ```text
//! cargo run -p bt-shell-macos --example feel              what plays here, and the names
//! cargo run -p bt-shell-macos --example feel -- watch     says when a finger is on the glass
//! cargo run -p bt-shell-macos --example feel -- tick      puts a finger on, then plays `tick`
//! ```
//!
//! `watch` is how to tell whether the frames of a trackpad come while another application is in
//! front and the pointer is somewhere else: start it, switch to another application, touch the
//! trackpad and look at the lines.

use std::time::{Duration, Instant};

use bt_shell_common::native_view::haptic::{NAMES, preset};
use bt_shell_macos::feel::{Haptics, Kind};

const LOOK: Duration = Duration::from_secs(30);

fn main() {
    let haptics = Haptics::open();
    match haptics.kind() {
        Kind::Motor { devices } => {
            println!("the motor of {devices} trackpad(s), finger frames read")
        }
        Kind::Fallback => println!("AppKit's own patterns: a held event is not played"),
        Kind::Nothing => println!("no trackpad here vibrates"),
    }
    let Some(word) = std::env::args().nth(1) else {
        println!("names: {}", NAMES.join(" "));
        return;
    };
    if word == "watch" {
        let began = Instant::now();
        let mut last = false;
        println!(
            "watching for {} seconds; touch the trackpad",
            LOOK.as_secs()
        );
        while began.elapsed() < LOOK {
            let on = haptics.finger_on_glass();
            if on != last {
                println!(
                    "{:6.2}s  finger {}",
                    began.elapsed().as_secs_f32(),
                    if on { "on" } else { "off" }
                );
                last = on;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        return;
    }
    let Some(pattern) = preset(&word) else {
        eprintln!("`{word}` is not a ready pattern: {}", NAMES.join(" "));
        std::process::exit(64);
    };
    if matches!(haptics.kind(), Kind::Motor { .. }) {
        println!("put a finger on the trackpad ({} seconds)", LOOK.as_secs());
        let began = Instant::now();
        while !haptics.finger_on_glass() {
            if began.elapsed() > LOOK {
                eprintln!("no finger came");
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::sleep(Duration::from_millis(300));
    } else {
        println!("touch the trackpad now, the pattern plays in 3 seconds");
        std::thread::sleep(Duration::from_secs(3));
    }
    haptics.play(&pattern);
    std::thread::sleep(Duration::from_secs_f32(pattern.seconds() + 0.5));
}
