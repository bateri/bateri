//! What the three settings of `[feedback]` let play, as pure rules.
//!
//! `haptics` and `sounds` are the person's: what vibrates and what sounds at all. `apps` is what the
//! programs' views may ask for: a person who turns it off is rid of what pages play and still has
//! the commands they ran themselves (`bateri haptic`, `bateri sound`) and what bateri plays of its
//! own. A page that asks while it is closed is not told so: it is dropped, like a vibration
//! where no finger is on the glass. Stopping never waits for a switch: what ends what plays is
//! always allowed.

/// What the settings say, as the service reads them. All on is what the file says where it says
/// nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Switches {
    /// `feedback.haptics`: the trackpad vibrates.
    pub haptics: bool,
    /// `feedback.sounds`: sounds play.
    pub sounds: bool,
    /// `feedback.apps`: the programs' views may vibrate and sound.
    pub apps: bool,
}

impl Switches {
    /// What `settings` say.
    #[must_use]
    pub fn of(settings: &bt_core::Settings) -> Self {
        Self {
            haptics: settings.feedback_haptics,
            sounds: settings.feedback_sounds,
            apps: settings.feedback_apps,
        }
    }
}

impl Default for Switches {
    fn default() -> Self {
        Self {
            haptics: true,
            sounds: true,
            apps: true,
        }
    }
}

/// Who asks for a vibration or a sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asker {
    /// The view of a program: what the `apps` switch is for.
    Page,
    /// The person's own command or what bateri does itself.
    Person,
}

impl Switches {
    /// Whether a vibration `by` asks for plays. `apps` closed is no key of the person's own
    /// command, and a page that is not played is not told.
    #[must_use]
    pub fn plays_haptic(self, by: Asker) -> bool {
        self.haptics && (by == Asker::Person || self.apps)
    }

    /// Whether a sound `by` asks for plays, as [`Switches::plays_haptic`].
    #[must_use]
    pub fn plays_sound(self, by: Asker) -> bool {
        self.sounds && (by == Asker::Person || self.apps)
    }

    /// What a vibration that plays now has to end when the switches change from `self` to `new`:
    /// the vibration of the application's own, of a page's, or both.
    #[must_use]
    pub fn ends_haptic(self, new: Self) -> Ends {
        Ends {
            person: self.plays_haptic(Asker::Person) && !new.plays_haptic(Asker::Person),
            pages: self.plays_haptic(Asker::Page) && !new.plays_haptic(Asker::Page),
        }
    }

    /// What a sound that plays now has to end when the switches change, as [`Switches::ends_haptic`].
    #[must_use]
    pub fn ends_sounds(self, new: Self) -> Ends {
        Ends {
            person: self.plays_sound(Asker::Person) && !new.plays_sound(Asker::Person),
            pages: self.plays_sound(Asker::Page) && !new.plays_sound(Asker::Page),
        }
    }
}

/// Whose plays a change of the switches ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ends {
    /// What the person and the application play themselves.
    pub person: bool,
    /// What the programs' views play.
    pub pages: bool,
}

impl Asker {
    /// Who asks when the view is `view`: the application's own plays are the person's, and every
    /// other view is a program's.
    #[must_use]
    pub fn of(view: u64) -> Self {
        if view == super::service::APPLICATION {
            Asker::Person
        } else {
            Asker::Page
        }
    }
}

impl Ends {
    /// Whether what `view` plays ends.
    #[must_use]
    pub fn of(self, view: u64) -> bool {
        match Asker::of(view) {
            Asker::Person => self.person,
            Asker::Page => self.pages,
        }
    }

    /// Whether anything ends.
    #[must_use]
    pub fn any(self) -> bool {
        self.person || self.pages
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every combination of the three switches.
    fn every() -> Vec<Switches> {
        (0..8)
            .map(|bits| Switches {
                haptics: bits & 1 != 0,
                sounds: bits & 2 != 0,
                apps: bits & 4 != 0,
            })
            .collect()
    }

    #[test]
    fn the_switches_are_what_the_settings_say() {
        use bt_core::Settings;

        assert_eq!(Switches::of(&Settings::default()), Switches::default());
        let settings = Settings {
            feedback_haptics: false,
            feedback_apps: false,
            ..Settings::default()
        };
        assert_eq!(
            Switches::of(&settings),
            Switches {
                haptics: false,
                sounds: true,
                apps: false
            }
        );
    }

    #[test]
    fn the_settings_say_all_on_where_the_file_says_nothing() {
        let all = Switches::default();
        assert!(all.haptics && all.sounds && all.apps);
        for by in [Asker::Page, Asker::Person] {
            assert!(all.plays_haptic(by));
            assert!(all.plays_sound(by));
        }
    }

    #[test]
    fn a_page_plays_only_where_its_kind_and_apps_are_both_on() {
        for switches in every() {
            assert_eq!(
                switches.plays_haptic(Asker::Page),
                switches.haptics && switches.apps,
                "{switches:?}"
            );
            assert_eq!(
                switches.plays_sound(Asker::Page),
                switches.sounds && switches.apps,
                "{switches:?}"
            );
        }
    }

    #[test]
    fn the_persons_own_command_looks_at_its_kind_alone() {
        for switches in every() {
            assert_eq!(
                switches.plays_haptic(Asker::Person),
                switches.haptics,
                "{switches:?}"
            );
            assert_eq!(
                switches.plays_sound(Asker::Person),
                switches.sounds,
                "{switches:?}"
            );
        }
    }

    #[test]
    fn what_plays_ends_when_the_switch_that_lets_it_play_is_closed() {
        let all = Switches::default();
        // `apps` closed ends what pages play, not what the person's command started.
        let no_apps = Switches { apps: false, ..all };
        for ends in [all.ends_haptic(no_apps), all.ends_sounds(no_apps)] {
            assert_eq!(
                ends,
                Ends {
                    person: false,
                    pages: true
                }
            );
        }
        // The kind closed ends both.
        let no_haptics = Switches {
            haptics: false,
            ..all
        };
        assert_eq!(
            all.ends_haptic(no_haptics),
            Ends {
                person: true,
                pages: true
            }
        );
        assert!(!all.ends_sounds(no_haptics).any(), "sounds go on");
        let no_sounds = Switches {
            sounds: false,
            ..all
        };
        assert_eq!(
            all.ends_sounds(no_sounds),
            Ends {
                person: true,
                pages: true
            }
        );
        assert!(!all.ends_haptic(no_sounds).any(), "vibrations go on");
        // Opening a switch ends nothing, and a switch that was closed already has nothing to end.
        assert!(!no_apps.ends_haptic(all).any());
        assert!(!no_haptics.ends_haptic(no_haptics).any());
        assert!(
            !no_haptics
                .ends_haptic(Switches {
                    apps: false,
                    ..no_haptics
                })
                .any()
        );
    }
}
