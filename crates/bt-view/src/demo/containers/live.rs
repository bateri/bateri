//! What a program that shows the container list tells it as it goes: the readings of the
//! running containers, once a second.
//!
//! The page knows its containers (the list it opens with); a reading names one and says how
//! much CPU and memory it uses now. [`readings`] is the made-up host that gives them: the page
//! and the program that drives it both use it, so the numbers one makes up are the ones the
//! other expects — and there is one list of containers, not two.

use super::{Container, Containers};

/// How many CPU samples a container's line keeps.
const HISTORY: usize = 30;

/// The longest memory text a reading may carry: what a row has room for, and a bound on what a
/// program can make the page hold.
const MEMORY_TEXT: usize = 16;

/// What a running container uses now.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// The container's name, as the page lists it. A reading of a name the page does not list
    /// (or of a container that is not running) changes nothing.
    pub name: String,
    /// CPU, percent of one core.
    pub cpu: f32,
    /// Memory in use, as text (`112MiB`).
    pub memory: String,
    /// Its share of what the container may use, `0.0..=1.0`.
    pub share: f32,
}

impl Containers {
    /// Takes what the program reported: the line of each running container named moves on by a
    /// sample and its numbers become the reading's. Whether the page changed.
    pub fn live(&mut self, readings: &[Reading]) -> bool {
        let mut changed = false;
        for reading in readings {
            let Some(container) = self
                .containers
                .iter_mut()
                .find(|c| c.is_up() && c.name == reading.name)
            else {
                continue;
            };
            let cpu = if reading.cpu.is_finite() {
                reading.cpu.clamp(0.0, 10_000.0)
            } else {
                0.0
            };
            let share = if reading.share.is_finite() {
                reading.share.clamp(0.0, 1.0)
            } else {
                0.0
            };
            let memory: String = reading.memory.chars().take(MEMORY_TEXT).collect();
            if container.history.len() >= HISTORY {
                container.history.remove(0);
            }
            container.history.push(cpu);
            container.cpu = cpu;
            container.memory = (memory, share);
            changed = true;
        }
        changed
    }
}

/// The readings of the made-up host at its `tick`th second: one for each container the fixture
/// has running, near where its line stood, with a spike now and then. The same every time for
/// the same tick.
pub fn readings(tick: u32) -> Vec<Reading> {
    Containers::fixture()
        .containers
        .iter()
        .filter(|c| c.is_up())
        .map(|c| reading(c, tick))
        .collect()
}

fn reading(c: &Container, tick: u32) -> Reading {
    let seed = c
        .name
        .bytes()
        .fold(tick.wrapping_mul(2_654_435_761), |sum, byte| {
            sum.wrapping_mul(31).wrapping_add(u32::from(byte))
        });
    let mut state = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1_u32 << 24) as f32
    };
    let base = c.history.last().copied().unwrap_or(c.cpu).max(0.2);
    let (r, s, m) = (next(), next(), next());
    let spike = if s > 0.9 { base * 3.0 * r } else { 0.0 };
    let cpu = base * (0.7 + 0.6 * r) + spike;
    let share = (c.memory.1 + (m - 0.5) * 0.02).clamp(0.0, 1.0);
    Reading {
        name: c.name.to_owned(),
        cpu,
        memory: drift(&c.memory.0, 1.0 + (m - 0.5) * 0.02),
        share,
    }
}

/// `112MiB` scaled by `factor`, written the way the fixture writes it: three significant digits.
fn drift(text: &str, factor: f32) -> String {
    let digits = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(digits);
    let Ok(value) = number.parse::<f32>() else {
        return text.to_owned();
    };
    let value = value * factor;
    if value >= 100.0 {
        format!("{value:.0}{unit}")
    } else if value >= 10.0 {
        format!("{value:.1}{unit}")
    } else {
        format!("{value:.2}{unit}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tick_gives_a_reading_for_each_running_container_and_the_same_one_again() {
        let first = readings(3);
        assert_eq!(first, readings(3));
        assert_ne!(first, readings(4));
        let names: Vec<&str> = first.iter().map(|r| r.name.as_str()).collect();
        assert!(
            names.contains(&"api") && names.contains(&"toolbox"),
            "{names:?}"
        );
        assert!(
            !names.contains(&"mailer"),
            "a stopped container reports nothing"
        );
        for reading in &first {
            assert!(reading.cpu.is_finite() && reading.cpu >= 0.0, "{reading:?}");
            assert!((0.0..=1.0).contains(&reading.share), "{reading:?}");
        }
    }

    #[test]
    fn a_reading_moves_the_line_of_the_container_it_names_and_no_other() {
        let mut page = Containers::fixture();
        let before: Vec<(f32, usize)> = page
            .containers
            .iter()
            .map(|c| (c.cpu, c.history.len()))
            .collect();
        let api = page
            .containers
            .iter()
            .position(|c| c.name == "api")
            .unwrap();
        let reading = Reading {
            name: "api".into(),
            cpu: 12.5,
            memory: "130MiB".into(),
            share: 0.3,
        };
        assert!(page.live(&[reading]));
        for (at, c) in page.containers.iter().enumerate() {
            if at == api {
                assert_eq!(c.cpu, 12.5);
                assert_eq!(c.history.last(), Some(&12.5));
                assert_eq!(c.history.len(), 30, "the line keeps thirty samples");
                assert_eq!(c.memory, ("130MiB".to_owned(), 0.3));
            } else {
                assert_eq!((c.cpu, c.history.len()), before[at]);
            }
        }
    }

    #[test]
    fn a_reading_the_page_cannot_use_changes_nothing() {
        let mut page = Containers::fixture();
        let stopped = Reading {
            name: "mailer".into(),
            cpu: 5.0,
            memory: "1MiB".into(),
            share: 0.1,
        };
        let unknown = Reading {
            name: "nobody".into(),
            ..stopped.clone()
        };
        assert!(!page.live(&[stopped, unknown]));
    }

    #[test]
    fn what_a_program_makes_up_is_kept_within_what_the_page_can_hold() {
        let mut page = Containers::fixture();
        let wild = Reading {
            name: "api".into(),
            cpu: f32::NAN,
            memory: "x".repeat(10_000),
            share: f32::INFINITY,
        };
        assert!(page.live(&[wild]));
        let api = page.containers.iter().find(|c| c.name == "api").unwrap();
        assert_eq!(api.cpu, 0.0);
        assert_eq!(api.memory.0.chars().count(), 16);
        assert_eq!(api.memory.1, 0.0);
        let huge = Reading {
            name: "api".into(),
            cpu: 1e30,
            memory: String::new(),
            share: 7.0,
        };
        page.live(&[huge]);
        let api = page.containers.iter().find(|c| c.name == "api").unwrap();
        assert_eq!(api.cpu, 10_000.0);
        assert_eq!(api.memory.1, 1.0);
    }

    #[test]
    fn memory_drifts_in_the_fixtures_own_units() {
        assert_eq!(drift("112MiB", 1.0), "112MiB");
        assert_eq!(drift("7.2MiB", 1.01), "7.27MiB");
        assert_eq!(drift("936KiB", 1.0), "936KiB");
        assert_eq!(drift("", 1.0), "");
    }
}
