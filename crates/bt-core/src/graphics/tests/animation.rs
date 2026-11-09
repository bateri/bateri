//! The animations the terminal runs: a gap that has passed takes the next
//! frame, and a clock says when the one after it is due. Time is made up here:
//! each test says what the hour is.

use std::time::{Duration, Instant};

use super::frames::{features, pictures_of};
use super::*;

const WINDOW: Window = Window {
    first: 0,
    last: 23,
    shift: 0,
};

fn after(start: Instant, millis: u64) -> Instant {
    start + Duration::from_millis(millis)
}

/// Image 7, shown, with three frames of these gaps in milliseconds (a gap of
/// -1 is a frame that is passed over). The frames are all different.
fn animated(rig: &mut Rig, gaps: [i32; 3]) {
    rig.run(&wire("a=T,f=32,s=4,v=3,i=7,C=1", &rgba(4, 3, 1)));
    for (seed, gap) in [(2, gaps[1]), (3, gaps[2])] {
        rig.run(&wire(
            &format!("a=f,i=7,s=4,v=3,z={gap}"),
            &rgba(4, 3, seed),
        ));
    }
    rig.run(&wire(&format!("a=a,i=7,r=1,z={}", gaps[0]), &[]));
}

/// A command that came at `now`, and what it answered.
fn command_at(rig: &mut Rig, now: Instant, control: &str) -> Vec<u8> {
    let mut now = now;
    rig.run_at(&wire(control, &[]), &mut now, &mut Vec::new())
}

/// The images on show played up to `now`; when the next frame is due.
fn play(rig: &Rig, now: Instant) -> Option<Instant> {
    rig.hub
        .with_images_mut(|images| images.advance(false, (10, 20), 80, &[WINDOW], &[], now))
}

fn current(rig: &Rig) -> usize {
    rig.hub.with_images(|images| {
        images
            .main()
            .get(7)
            .map_or(usize::MAX, |image| image.current)
    })
}

#[test]
fn a_running_animation_takes_the_next_frame_when_the_gap_has_passed() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let start = Instant::now();
    assert!(command_at(&mut rig, start, "a=a,i=7,s=3").is_empty());
    assert_eq!(play(&rig, after(start, 99)), Some(after(start, 100)));
    assert_eq!(current(&rig), 0);
    assert_eq!(play(&rig, after(start, 100)), Some(after(start, 150)));
    assert_eq!(current(&rig), 1);
    assert_eq!(play(&rig, after(start, 150)), Some(after(start, 350)));
    assert_eq!(current(&rig), 2);
    // After the last it begins again, for ever.
    assert_eq!(play(&rig, after(start, 350)), Some(after(start, 450)));
    assert_eq!(current(&rig), 0);
}

#[test]
fn one_frame_is_taken_at_a_time_and_a_late_clock_does_not_catch_up() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let start = Instant::now();
    command_at(&mut rig, start, "a=a,i=7,s=3");
    let late = after(start, 10_000);
    assert_eq!(play(&rig, late), Some(after(late, 50)));
    assert_eq!(current(&rig), 1);
}

#[test]
fn a_frame_with_no_gap_is_passed_over() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, -1, 100]);
    let start = Instant::now();
    command_at(&mut rig, start, "a=a,i=7,s=3");
    assert_eq!(play(&rig, after(start, 100)), Some(after(start, 200)));
    assert_eq!(current(&rig), 2);
}

#[test]
fn an_animation_that_is_stopped_or_has_nothing_to_wait_for_asks_for_nothing() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let start = Instant::now();
    assert_eq!(play(&rig, after(start, 500)), None, "never started");
    assert_eq!(current(&rig), 0);
    command_at(&mut rig, start, "a=a,i=7,s=3");
    command_at(&mut rig, after(start, 10), "a=a,i=7,s=1");
    assert_eq!(play(&rig, after(start, 500)), None, "stopped");
    assert_eq!(current(&rig), 0);

    let mut gapless = Rig::new();
    animated(&mut gapless, [-1, -1, -1]);
    command_at(&mut gapless, start, "a=a,i=7,s=3");
    assert_eq!(play(&gapless, after(start, 500)), None, "no gap to wait");

    let mut single = Rig::new();
    single.run(&wire("a=T,f=32,s=4,v=3,i=7,C=1", &rgba(4, 3, 1)));
    single.run(&wire("a=a,i=7,r=1,z=50", &[]));
    command_at(&mut single, start, "a=a,i=7,s=3");
    assert_eq!(play(&single, after(start, 500)), None, "one frame");
}

#[test]
fn the_loading_mode_waits_on_the_last_frame_for_more_frames() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let start = Instant::now();
    command_at(&mut rig, start, "a=a,i=7,s=2");
    play(&rig, after(start, 100));
    play(&rig, after(start, 150));
    assert_eq!(current(&rig), 2);
    assert_eq!(
        play(&rig, after(start, 350)),
        None,
        "no clock for a frame that is not there"
    );
    assert_eq!(current(&rig), 2, "and the animation does not begin again");
    rig.run(&wire("a=f,i=7,s=4,v=3,z=30", &rgba(4, 3, 4)));
    assert_eq!(play(&rig, after(start, 351)), Some(after(start, 381)));
    assert_eq!(current(&rig), 3);
}

#[test]
fn v_is_the_number_of_loops_plus_one_and_the_animation_is_run_again_with_v_and_a_frame() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let start = Instant::now();
    command_at(&mut rig, start, "a=a,i=7,s=3,v=2");
    play(&rig, after(start, 100));
    play(&rig, after(start, 150));
    assert_eq!(
        play(&rig, after(start, 350)),
        None,
        "one loop, and it is over"
    );
    assert_eq!(current(&rig), 2, "on the last frame");
    assert_eq!(play(&rig, after(start, 900)), None);
    // Run again, with no limit, from the first frame.
    command_at(&mut rig, after(start, 1000), "a=a,i=7,c=1,s=3,v=1");
    assert_eq!(play(&rig, after(start, 1100)), Some(after(start, 1150)));
    assert_eq!(current(&rig), 1);

    let mut thrice = Rig::new();
    animated(&mut thrice, [10, 10, 10]);
    command_at(&mut thrice, start, "a=a,i=7,s=3,v=3");
    let mut seen = Vec::new();
    for step in 1..=20u64 {
        if play(&thrice, after(start, step * 10)).is_none() {
            break;
        }
        seen.push(current(&thrice));
    }
    // Twice through the three frames, and the last one stays.
    assert_eq!(seen, [1, 2, 0, 1, 2]);
    assert_eq!(current(&thrice), 2);
}

#[test]
fn the_current_frame_is_set_by_a_command_and_is_a_new_picture() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let before = pictures_of(&rig);
    let start = Instant::now();
    command_at(&mut rig, start, "a=a,i=7,c=3");
    assert_eq!(current(&rig), 2);
    let after_it = pictures_of(&rig);
    assert_eq!(after_it.grid()[0].pixels[..4], rgba(4, 3, 3)[..4]);
    assert_ne!(after_it.grid()[0].image, before.grid()[0].image);
    assert_eq!(after_it.grid()[0].key, before.grid()[0].key);
    assert_eq!(
        after_it.grid()[0].edits.since(before.grid()[0].image),
        Dirty::Rect([0, 0, 4, 3])
    );
    // No such frame, and the frame that is on: nothing happens.
    let held = after_it.grid()[0].image;
    command_at(&mut rig, start, "a=a,i=7,c=9");
    command_at(&mut rig, start, "a=a,i=7,c=3");
    assert_eq!(pictures_of(&rig).grid()[0].image, held);
}

#[test]
fn a_frame_taken_by_the_animation_is_the_picture_the_frame_draws() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let start = Instant::now();
    command_at(&mut rig, start, "a=a,i=7,s=3");
    let before = pictures_of(&rig);
    play(&rig, after(start, 100));
    let moved = pictures_of(&rig);
    assert_eq!(moved.grid()[0].pixels[..4], rgba(4, 3, 2)[..4]);
    assert_ne!(moved.grid()[0].image, before.grid()[0].image);
    assert_eq!(moved.grid()[0].key, before.grid()[0].key);
}

#[test]
fn a_gap_is_set_for_a_frame_and_a_negative_one_passes_it_over() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let gaps = |rig: &Rig| {
        rig.hub.with_images(|images| {
            images
                .main()
                .get(7)
                .map(|image| {
                    image
                        .frames
                        .iter()
                        .map(|frame| frame.gap)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    };
    let start = Instant::now();
    command_at(&mut rig, start, "a=a,i=7,r=2,z=75");
    assert_eq!(gaps(&rig), [100, 75, 200]);
    command_at(&mut rig, start, "a=a,i=7,r=3,z=-5");
    assert_eq!(gaps(&rig), [100, 75, 0]);
    command_at(&mut rig, start, "a=a,i=7,r=9,z=10");
    assert_eq!(gaps(&rig), [100, 75, 0], "there is no ninth frame");
}

#[test]
fn only_an_image_the_windows_show_is_played() {
    let mut rig = Rig::new();
    animated(&mut rig, [100, 50, 200]);
    let start = Instant::now();
    command_at(&mut rig, start, "a=a,i=7,s=3");
    let elsewhere = Window {
        first: 100,
        last: 120,
        shift: 0,
    };
    let played = rig.hub.with_images_mut(|images| {
        images.advance(false, (10, 20), 80, &[elsewhere], &[], after(start, 500))
    });
    assert_eq!(played, None);
    assert_eq!(current(&rig), 0);
    // And not the images of the other screen.
    let alt = rig.hub.with_images_mut(|images| {
        images.advance(true, (10, 20), 80, &[WINDOW], &[], after(start, 500))
    });
    assert_eq!(alt, None);
    assert_eq!(current(&rig), 0);
}

#[test]
fn what_an_animation_command_answers_is_the_error_for_an_image_that_is_not_there() {
    let mut rig = Rig::new();
    let start = Instant::now();
    let answer = command_at(&mut rig, start, "a=a,i=9,s=3");
    let text = error_of(&answer);
    assert!(text.starts_with("\x1b_Gi=9;ENOENT:"), "{text:?}");
    animated(&mut rig, [100, 50, 200]);
    assert!(command_at(&mut rig, start, "a=a,i=7,s=3").is_empty());
}

#[test]
fn the_animation_keys_are_a_feature_and_a_state_that_is_not_one_is_not_done() {
    let mut rig = Rig::new();
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=a,s=3,v=2,r=1,z=40,c=2,Y=5"),
        "\x1b_Gi=31,e=a,s=3,v=2,r=1,z=40,c=2;OK\x1b\\"
    );
    assert_eq!(
        features(&mut rig, "a=q,i=31,e=a,s=9"),
        "\x1b_Gi=31,e=a;OK\x1b\\"
    );
}
