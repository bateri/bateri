//! The service the application owns: what a page's vibration and sounds are played with, per view.
//!
//! One for the application, so that what the application does itself (a pane that lands, a tab
//! that is dropped) is played with the same patterns and the same rules as a page's. A pane
//! reaches it through its owner (`PaneHost`) and tells it which view is asking; the service keeps
//! what the kit's rules say it keeps: a view has one vibration, which a new one takes the
//! place of and which ends with it, and plays up to [`MOST_VOICES`] sounds at once, over each other,
//! which end with it.
//!
//! The device and the speaker are what a test replaces ([`Engine`], [`Speaker`]): the rules here
//! are pure, and the real ones are not started until a vibration or a sound is first asked for.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use bt_shell_common::agent::ready::Outcome;
use bt_shell_common::native_view::haptic::{self, Haptic, Pattern};
use bt_shell_common::native_view::sound::{self, Ask, Sound, Voices};
use bt_shell_common::native_view::sounds;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::NSSound;
use objc2_foundation::NSData;

use super::{Haptics, Told};

/// What vibrates.
pub trait Engine {
    /// Plays `pattern` in place of what plays, where a finger is on the glass. What came of it is
    /// told to `told` when it is known, which may be later and from another thread: nothing
    /// waits for it.
    fn play(&self, pattern: &Pattern, told: Option<Told>);
    /// Ends what plays.
    fn stop(&self);
}

impl Engine for Haptics {
    fn play(&self, pattern: &Pattern, told: Option<Told>) {
        Haptics::play(self, pattern, told);
    }

    fn stop(&self) {
        Haptics::stop(self);
    }
}

/// What plays the sounds: each one numbered, so that the one the rules end can be named.
pub trait Speaker {
    /// Starts `sound` over what plays, at `volume` (from 0 to 1), as `id`: whether it started.
    /// `owner` is the view whose file the sound was made of (what is kept ready of it goes with
    /// the view, [`Speaker::release`]); none for a sound of the kit's, which is kept for the
    /// application's life.
    fn play(&mut self, id: u64, sound: &Arc<Sound>, volume: f32, owner: Option<u64>) -> bool;
    /// Lets go of what was kept ready of the sounds `view` made of its files.
    fn release(&mut self, _view: u64) {}
    /// Ends the sound `id`, if it still plays.
    fn stop(&mut self, id: u64);
}

/// A sound that was played, as the data the system plays from.
struct Prepared {
    /// The sound, held so that the address that names it is not given to another.
    sound: Arc<Sound>,
    data: Retained<NSData>,
    bytes: usize,
    /// The view whose file it is made of, which it is let go of with; none for the kit's.
    owner: Option<u64>,
}

/// The most sounds the system's player keeps ready, and the most bytes of them: a page that plays
/// one sound over and over (a file it decoded once) is not made to turn every sample into a WAV
/// file each time, on the thread that draws. What a view's files made is let go of with the view.
const MOST_PREPARED: usize = 8;
const MOST_PREPARED_BYTES: usize = 48 << 20;

/// The system's sound player, `NSSound`, which plays what it is given at the system's volume and
/// silence: each sound is its samples as a WAV file in memory.
#[derive(Default)]
struct SystemSpeaker {
    /// The sounds that were started and may still play: a sound that is let go ends.
    playing: Vec<(u64, Retained<NSSound>)>,
    /// What was made ready, oldest first.
    prepared: Vec<Prepared>,
}

impl SystemSpeaker {
    /// The data `sound` is played from: made once and kept for the next time the same sound is
    /// asked for (what the kit varies a little on each play is a sound of its own each time).
    fn data_of(&mut self, sound: &Arc<Sound>, owner: Option<u64>) -> Option<Retained<NSData>> {
        if let Some(kept) = self
            .prepared
            .iter()
            .find(|kept| Arc::ptr_eq(&kept.sound, sound))
        {
            return Some(kept.data.clone());
        }
        let wav = sounds::wav(sound)?;
        let bytes = wav.len();
        let data = NSData::with_bytes(&wav);
        if bytes <= MOST_PREPARED_BYTES {
            let mut held: usize = self.prepared.iter().map(|kept| kept.bytes).sum();
            while !self.prepared.is_empty()
                && (self.prepared.len() >= MOST_PREPARED || held + bytes > MOST_PREPARED_BYTES)
            {
                held -= self.prepared.remove(0).bytes;
            }
            self.prepared.push(Prepared {
                sound: Arc::clone(sound),
                data: data.clone(),
                bytes,
                owner,
            });
        }
        Some(data)
    }
}

impl Speaker for SystemSpeaker {
    fn play(&mut self, id: u64, sound: &Arc<Sound>, volume: f32, owner: Option<u64>) -> bool {
        self.playing.retain(|(_, started)| started.isPlaying());
        let Some(data) = self.data_of(sound, owner) else {
            return false;
        };
        let Some(started) = NSSound::initWithData(NSSound::alloc(), &data) else {
            return false;
        };
        started.setVolume(volume.clamp(0.0, 1.0));
        if !started.play() {
            return false;
        }
        self.playing.push((id, started));
        true
    }

    fn stop(&mut self, id: u64) {
        if let Some(at) = self.playing.iter().position(|(known, _)| *known == id) {
            let (_, started) = self.playing.remove(at);
            started.stop();
        }
    }

    fn release(&mut self, view: u64) {
        self.prepared.retain(|kept| kept.owner != Some(view));
    }
}

/// The view the application's own plays are for: no page is behind it. Not a number a pane has
/// (they count up from 0).
pub const APPLICATION: u64 = u64::MAX;

/// Whether a vibration a view asks for is one to play: the view is the one the person works in (the
/// key window, the tab on screen) and, in it, the pane the keyboard is in or the one the pointer
/// is over. The finger is the device's to check.
#[must_use]
pub fn is_felt(active: bool, keyboard_here: bool, pointer_over: bool) -> bool {
    active && (keyboard_here || pointer_over)
}

#[derive(Default)]
struct State {
    /// The view the vibration that plays is for.
    vibrating: Option<u64>,
    /// What each view plays.
    voices: HashMap<u64, Voices>,
    /// The number of the next sound, which is also what makes two plays of one sound differ.
    next_sound: u64,
}

/// The vibration and the sounds of the views of the application.
pub struct Feel {
    open: Box<dyn Fn() -> Box<dyn Engine>>,
    engine: OnceCell<Box<dyn Engine>>,
    speaker: RefCell<Box<dyn Speaker>>,
    now: Box<dyn Fn() -> f64>,
    state: RefCell<State>,
}

impl Default for Feel {
    fn default() -> Self {
        Self::new()
    }
}

impl Feel {
    /// The service with the real trackpad and speaker, which are not touched yet.
    #[must_use]
    pub fn new() -> Self {
        let began = Instant::now();
        Self::with(
            Box::new(|| Box::new(Haptics::open())),
            Box::new(SystemSpeaker::default()),
            Box::new(move || began.elapsed().as_secs_f64()),
        )
    }

    /// The service with `open` to make the vibration's engine when it is first asked for,
    /// the speaker and the clock (seconds on any one clock).
    #[must_use]
    pub fn with(
        open: Box<dyn Fn() -> Box<dyn Engine>>,
        speaker: Box<dyn Speaker>,
        now: Box<dyn Fn() -> f64>,
    ) -> Self {
        Self {
            open,
            engine: OnceCell::new(),
            speaker: RefCell::new(speaker),
            now,
            state: RefCell::new(State::default()),
        }
    }

    /// What a view asks of the vibration. `felt` is whether it is to be played ([`is_felt`]): what
    /// is not is dropped, and is not kept for when it is.
    pub fn haptic(&self, view: u64, ask: &Haptic, felt: bool) {
        match ask {
            Haptic::Play(pattern) => {
                if felt {
                    self.vibrate(view, pattern, None);
                } else {
                    // A new vibration takes the place of the view's last, felt or not: the old
                    // one is not left to play on under a page that has moved on.
                    self.end_haptic(view);
                }
            }
            Haptic::Stop => self.end_haptic(view),
        }
    }

    /// Starts `pattern` for `view`. The device is opened here, on the thread that asks (the main
    /// one), the first time a vibration is: the framework is loaded and the trackpads listed once.
    /// What came of it is told to `told`, if there is one, when it is known.
    fn vibrate(&self, view: u64, pattern: &Pattern, told: Option<Told>) {
        self.state.borrow_mut().vibrating = Some(view);
        self.engine
            .get_or_init(|| (self.open)())
            .play(pattern, told);
    }

    /// What a view asks of its sounds.
    pub fn sound(&self, view: u64, ask: &Ask) {
        match ask {
            Ask::Play(play) => {
                self.start_sound(view, play);
            }
            Ask::Stop => self.end_sounds(view, (self.now)()),
        }
    }

    /// Starts the sound `play` asks for, for `view`: whether it started. A sound that did not start
    /// is no voice of the view's, so that it does not silence one that plays.
    fn start_sound(&self, view: u64, play: &sound::Play) -> bool {
        let Some(samples) = &play.samples else {
            return false;
        };
        let now = (self.now)();
        let id = {
            let mut state = self.state.borrow_mut();
            state.next_sound += 1;
            state.next_sound
        };
        let owner = match play.source {
            sound::Source::File(_) => Some(view),
            sound::Source::Kit(_) => None,
        };
        let mut speaker = self.speaker.borrow_mut();
        if !speaker.play(id, samples, play.volume, owner) {
            return false;
        }
        let silenced = self
            .state
            .borrow_mut()
            .voices
            .entry(view)
            .or_default()
            .play(now, id, samples.seconds());
        if let Some(silenced) = silenced {
            speaker.stop(silenced);
        }
        true
    }

    /// The view is gone: its vibration and its sounds end with it.
    pub fn view_ended(&self, view: u64) {
        self.end_haptic(view);
        self.end_sounds(view, (self.now)());
        self.state.borrow_mut().voices.remove(&view);
        self.speaker.borrow_mut().release(view);
    }

    /// Plays the ready pattern called `name` for the application itself; what came of it is told
    /// to `told` when it is known. `false` where the name is not one the kit has ready (`told` is
    /// not called).
    pub fn play_ready_haptic(&self, name: &str, told: Told) -> bool {
        let Some(pattern) = haptic::preset(name) else {
            return false;
        };
        self.vibrate(APPLICATION, &pattern, Some(told));
        true
    }

    /// Plays the ready sound called `name` for the application itself.
    ///
    /// What came of it, or `None` where the name is not one the kit has ready.
    pub fn play_ready_sound(&self, name: &str) -> Option<Outcome> {
        let draw = self.state.borrow().next_sound;
        let samples = sounds::play_of(name, 1.0, draw)?;
        let play = sound::Play {
            source: sound::Source::Kit(name.to_owned()),
            volume: 1.0,
            pitch: 1.0,
            samples: Some(samples),
        };
        Some(if self.start_sound(APPLICATION, &play) {
            Outcome::Played
        } else {
            Outcome::Failed
        })
    }

    fn end_haptic(&self, view: u64) {
        let mut state = self.state.borrow_mut();
        if state.vibrating == Some(view) {
            state.vibrating = None;
            drop(state);
            if let Some(engine) = self.engine.get() {
                engine.stop();
            }
        }
    }

    fn end_sounds(&self, view: u64, now: f64) {
        let ended = self
            .state
            .borrow_mut()
            .voices
            .get_mut(&view)
            .map(|voices| voices.stop_all(now))
            .unwrap_or_default();
        let mut speaker = self.speaker.borrow_mut();
        for id in ended {
            speaker.stop(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;
    use std::sync::Arc;

    use bt_shell_common::native_view::haptic::Event;
    use bt_shell_common::native_view::sound::MOST_VOICES;

    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Heard {
        Played(Pattern),
        Stopped,
    }

    #[derive(Clone, Default)]
    struct Motor(Rc<RefCell<Vec<Heard>>>);

    impl Engine for Motor {
        fn play(&self, pattern: &Pattern, told: Option<Told>) {
            self.0.borrow_mut().push(Heard::Played(pattern.clone()));
            if let Some(told) = told {
                told(Outcome::Played);
            }
        }

        fn stop(&self) {
            self.0.borrow_mut().push(Heard::Stopped);
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Sounded {
        Started(u64, f32),
        Ended(u64),
    }

    #[derive(Clone, Default)]
    struct Speakers(Rc<RefCell<Vec<Sounded>>>);

    impl Speaker for Speakers {
        fn play(&mut self, id: u64, _sound: &Arc<Sound>, volume: f32, _owner: Option<u64>) -> bool {
            self.0.borrow_mut().push(Sounded::Started(id, volume));
            true
        }

        fn stop(&mut self, id: u64) {
            self.0.borrow_mut().push(Sounded::Ended(id));
        }
    }

    struct Rig {
        feel: Feel,
        motor: Motor,
        speaker: Speakers,
        clock: Rc<Cell<f64>>,
        opened: Rc<Cell<u32>>,
    }

    fn rig() -> Rig {
        let motor = Motor::default();
        let speaker = Speakers::default();
        let clock = Rc::new(Cell::new(0.0));
        let opened = Rc::new(Cell::new(0));
        let (engine, count, time) = (motor.clone(), Rc::clone(&opened), Rc::clone(&clock));
        let feel = Feel::with(
            Box::new(move || {
                count.set(count.get() + 1);
                Box::new(engine.clone())
            }),
            Box::new(speaker.clone()),
            Box::new(move || time.get()),
        );
        Rig {
            feel,
            motor,
            speaker,
            clock,
            opened,
        }
    }

    fn pattern() -> Pattern {
        Pattern::new([Event::tap(0.0, 0.5, 0.5)]).expect("a pattern")
    }

    fn play_sound(seconds: f32, volume: f32) -> Ask {
        let frames = (48_000.0 * seconds) as usize;
        let samples = Sound::new(48_000, 1, vec![0.1; frames]).expect("a sound");
        Ask::Play(sound::Play {
            source: sound::Source::Kit("tap".to_owned()),
            volume,
            pitch: 1.0,
            samples: Some(Arc::new(samples)),
        })
    }

    #[test]
    fn the_device_is_not_opened_until_a_vibration_is_asked_for_and_not_to_end_one() {
        let rig = rig();
        rig.feel.haptic(1, &Haptic::Stop, true);
        rig.feel.view_ended(1);
        assert_eq!(rig.opened.get(), 0);
        rig.feel.haptic(1, &Haptic::Play(pattern()), true);
        rig.feel.haptic(1, &Haptic::Play(pattern()), true);
        assert_eq!(rig.opened.get(), 1, "once, however often it is asked");
    }

    #[test]
    fn a_vibration_that_is_not_to_be_felt_is_dropped_and_not_kept() {
        let rig = rig();
        rig.feel.haptic(1, &Haptic::Play(pattern()), false);
        assert!(rig.motor.0.borrow().is_empty());
        assert_eq!(rig.opened.get(), 0);
    }

    #[test]
    fn the_view_a_vibration_is_for_is_the_only_one_that_can_end_it() {
        let rig = rig();
        rig.feel.haptic(1, &Haptic::Play(pattern()), true);
        rig.feel.haptic(2, &Haptic::Stop, true);
        assert_eq!(
            rig.motor.0.borrow().len(),
            1,
            "another view's end does not cut it"
        );
        rig.feel.haptic(1, &Haptic::Stop, true);
        assert_eq!(rig.motor.0.borrow().last(), Some(&Heard::Stopped));
        // It is ended: the end of the view finds nothing to end.
        rig.feel.view_ended(1);
        assert_eq!(rig.motor.0.borrow().len(), 2);
    }

    #[test]
    fn a_new_vibration_takes_the_place_of_the_one_that_plays_whichever_view_it_is_of() {
        let rig = rig();
        rig.feel.haptic(1, &Haptic::Play(pattern()), true);
        rig.feel.haptic(2, &Haptic::Play(pattern()), true);
        rig.feel.haptic(1, &Haptic::Stop, true);
        assert_eq!(
            rig.motor.0.borrow().len(),
            2,
            "the view that was replaced no longer owns it"
        );
        rig.feel.view_ended(2);
        assert_eq!(rig.motor.0.borrow().last(), Some(&Heard::Stopped));
    }

    #[test]
    fn the_end_of_a_view_ends_its_vibration_and_every_sound_it_plays() {
        let rig = rig();
        rig.feel.haptic(7, &Haptic::Play(pattern()), true);
        rig.feel.sound(7, &play_sound(2.0, 0.5));
        rig.feel.sound(7, &play_sound(2.0, 1.0));
        rig.feel.sound(8, &play_sound(2.0, 1.0));
        rig.feel.view_ended(7);
        assert_eq!(rig.motor.0.borrow().last(), Some(&Heard::Stopped));
        let heard = rig.speaker.0.borrow().clone();
        assert_eq!(
            heard,
            vec![
                Sounded::Started(1, 0.5),
                Sounded::Started(2, 1.0),
                Sounded::Started(3, 1.0),
                Sounded::Ended(1),
                Sounded::Ended(2),
            ],
            "the other view's sound plays on"
        );
    }

    #[test]
    fn the_ninth_sound_of_a_view_ends_the_one_that_has_played_longest() {
        let rig = rig();
        for _ in 0..MOST_VOICES {
            rig.feel.sound(1, &play_sound(5.0, 1.0));
        }
        assert!(
            rig.speaker
                .0
                .borrow()
                .iter()
                .all(|heard| matches!(heard, Sounded::Started(..)))
        );
        rig.feel.sound(1, &play_sound(5.0, 1.0));
        let heard = rig.speaker.0.borrow().clone();
        // The new one starts first, so that what does not start silences nothing; the one that
        // played longest ends at once after.
        assert_eq!(
            heard[MOST_VOICES],
            Sounded::Started(MOST_VOICES as u64 + 1, 1.0)
        );
        assert_eq!(heard[MOST_VOICES + 1], Sounded::Ended(1));
        assert_eq!(heard.len(), MOST_VOICES + 2);
    }

    #[test]
    fn a_sound_that_has_ended_by_itself_does_not_count_among_the_eight() {
        let rig = rig();
        for _ in 0..MOST_VOICES {
            rig.feel.sound(1, &play_sound(0.5, 1.0));
        }
        rig.clock.set(2.0);
        rig.feel.sound(1, &play_sound(0.5, 1.0));
        assert!(
            rig.speaker
                .0
                .borrow()
                .iter()
                .all(|heard| matches!(heard, Sounded::Started(..)))
        );
    }

    #[test]
    fn the_end_of_the_sounds_of_a_view_ends_those_still_playing_and_no_other_view_s() {
        let rig = rig();
        rig.feel.sound(1, &play_sound(0.1, 1.0));
        rig.feel.sound(1, &play_sound(5.0, 1.0));
        rig.feel.sound(2, &play_sound(5.0, 1.0));
        rig.clock.set(1.0);
        rig.feel.sound(1, &Ask::Stop);
        let heard = rig.speaker.0.borrow().clone();
        assert_eq!(heard.last(), Some(&Sounded::Ended(2)));
        assert_eq!(
            heard
                .iter()
                .filter(|heard| matches!(heard, Sounded::Ended(_)))
                .count(),
            1
        );
    }

    #[test]
    fn a_sound_the_kit_has_not_made_ready_is_not_played() {
        let rig = rig();
        let play = sound::Play {
            source: sound::Source::File("a.wav".to_owned()),
            volume: 1.0,
            pitch: 1.0,
            samples: None,
        };
        rig.feel.sound(1, &Ask::Play(play));
        assert!(rig.speaker.0.borrow().is_empty());
    }

    /// What the service says came of the ready pattern `name`, or `None` where it is not one.
    fn ready_haptic(feel: &Feel, name: &str) -> Option<Outcome> {
        let said = std::sync::Arc::new(std::sync::Mutex::new(None));
        let sink = std::sync::Arc::clone(&said);
        let known = feel.play_ready_haptic(
            name,
            Box::new(move |outcome| *sink.lock().expect("a lock") = Some(outcome)),
        );
        known.then(|| said.lock().expect("a lock").take().expect("told at once"))
    }

    #[test]
    fn a_new_vibration_that_is_not_felt_still_ends_the_one_the_view_had() {
        let rig = rig();
        rig.feel.haptic(1, &Haptic::Play(pattern()), true);
        rig.feel.haptic(1, &Haptic::Play(pattern()), false);
        assert_eq!(rig.motor.0.borrow().len(), 2);
        assert_eq!(rig.motor.0.borrow().last(), Some(&Heard::Stopped));
        // Another view's is not the view's to end: the one of view 2 plays on.
        rig.feel.haptic(2, &Haptic::Play(pattern()), true);
        rig.feel.haptic(1, &Haptic::Play(pattern()), false);
        assert_eq!(rig.motor.0.borrow().len(), 3);
        assert!(matches!(
            rig.motor.0.borrow().last(),
            Some(Heard::Played(_))
        ));
    }

    #[test]
    fn what_is_kept_ready_of_a_views_files_goes_with_the_view_and_the_kits_stays() {
        let kit = sounds::play_of("tap", 1.0, 0).expect("a ready sound");
        let file = sounds::play_of("snap", 1.0, 0).expect("a ready sound");
        let mut speaker = SystemSpeaker::default();
        let first = speaker.data_of(&kit, None).expect("data");
        assert!(
            Retained::as_ptr(&first)
                == Retained::as_ptr(&speaker.data_of(&kit, None).expect("data")),
            "made once, kept for the next time the same sound is played"
        );
        let _ = speaker.data_of(&file, Some(7));
        assert_eq!(speaker.prepared.len(), 2);
        speaker.release(8);
        assert_eq!(
            speaker.prepared.len(),
            2,
            "another view's end lets go of nothing"
        );
        speaker.release(7);
        assert_eq!(speaker.prepared.len(), 1);
        assert!(speaker.prepared[0].owner.is_none(), "the kit's stays");
    }

    #[test]
    fn the_ready_names_play_for_the_application_and_a_name_that_is_none_says_which_there_are() {
        let rig = rig();
        for name in haptic::NAMES {
            assert_eq!(ready_haptic(&rig.feel, name), Some(Outcome::Played));
        }
        assert_eq!(rig.motor.0.borrow().len(), haptic::NAMES.len());
        for name in sound::NAMES {
            assert_eq!(rig.feel.play_ready_sound(name), Some(Outcome::Played));
        }
        assert_eq!(
            rig.speaker
                .0
                .borrow()
                .iter()
                .filter(|h| matches!(h, Sounded::Started(..)))
                .count(),
            sound::NAMES.len()
        );

        assert_eq!(ready_haptic(&rig.feel, "nope"), None);
        assert_eq!(rig.feel.play_ready_sound("nope"), None);
    }

    /// A speaker that starts nothing: the system could not play what it was given.
    struct Mute(Rc<RefCell<Vec<u64>>>);

    impl Speaker for Mute {
        fn play(
            &mut self,
            _id: u64,
            _sound: &Arc<Sound>,
            _volume: f32,
            _owner: Option<u64>,
        ) -> bool {
            false
        }

        fn stop(&mut self, id: u64) {
            self.0.borrow_mut().push(id);
        }
    }

    #[test]
    fn a_sound_the_system_could_not_start_is_said_so_and_is_no_voice_of_the_view() {
        let stopped = Rc::new(RefCell::new(Vec::new()));
        let feel = Feel::with(
            Box::new(|| Box::new(Motor::default())),
            Box::new(Mute(Rc::clone(&stopped))),
            Box::new(|| 0.0),
        );
        assert_eq!(feel.play_ready_sound("tap"), Some(Outcome::Failed));
        // Nine that did not start have silenced nothing: they were never playing.
        for _ in 0..=MOST_VOICES {
            feel.play_ready_sound("tap").expect("a ready sound");
        }
        assert!(stopped.borrow().is_empty(), "{:?}", stopped.borrow());
    }

    #[test]
    fn a_vibration_is_felt_only_in_the_view_the_person_works_in_and_where_they_are() {
        assert!(is_felt(true, true, false));
        assert!(is_felt(true, false, true));
        assert!(
            !is_felt(true, false, false),
            "the key window's tab, but not this pane"
        );
        assert!(
            !is_felt(false, true, true),
            "the pane is in a tab behind or a window that is not the key"
        );
    }
}
