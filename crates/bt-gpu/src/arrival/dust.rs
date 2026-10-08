//! The dust scene's model — **pure**: positions, opacities and times as
//! functions of a few numbers, no GPU and no clock.
//!
//! Specks of dust drift around the dock while the shell starts; at the first
//! prompt they are pulled onto the dock's top line from left to right and the
//! line is woven behind them, its tip lit. Only the dust: no light is drawn
//! behind it, and no part of the window lights the motes more than another.
//!
//! **One mote is a function of its index, the width and the time.** Its place,
//! depth and wandering come from a seeded generator, so the same pane width at
//! the same time draws the same dust and nothing is kept between frames. About
//! a third gather in three loose clouds, the rest are scattered; most are small
//! and far, a few are near, large and soft, and each glints now and then.
//! Every number is the design's — chosen by eye, not measured; the
//! generator is the design's too, ported with its draws in the design's order
//! so the dust is the dust that was approved.
//!
//! **The weld front is the landing seen from the line.** A mote on the line
//! lands at [`landing`]`(x / width, 0)`; the front of the woven line at time `t`
//! is the `x / width` that lands at `t` ([`front_at`]). One formula, so the
//! line is whole exactly when the last mote on it has landed.
//!
//! **Lengths are points, times seconds.** The frame turns points into pixels
//! where it has the scale.

use std::f32::consts::TAU;

use super::{CAP, SHOW};

/// How many motes there are.
pub(crate) const MOTES: usize = 120;

/// How far above the dock the dust may drift, points: the zone is the dock's
/// band and this much above it.
pub(crate) const ABOVE_PT: f32 = 120.0;

/// Room left clear at the bottom of the band, points: the motes fade out
/// before the window's edge.
const FLOOR_PT: f32 = 6.0;

/// The zone's least height below the dock's top, points.
const FLOOR_MIN_PT: f32 = 8.0;

/// How long the scene takes to fade in after the hold, seconds.
const FADE_IN: f32 = 0.22;

/// How brightly every mote is lit: a mote's opacity is its depth times this
/// times its glint. The same across the window — motes lit more in one band
/// would read as a beam of light, and the scene is the dust alone. Chosen by
/// eye.
const LIGHT: f32 = 1.0;

// The landing. A mote's turn comes `LAND_BASE + LAND_SWEEP·(x / width) +
// LAND_RISE·(height above the line)` seconds after the arrival; it then takes
// `FLIGHT` to reach the line, and glows `GLOW` after.
const LAND_BASE: f32 = 0.020;
const LAND_SWEEP: f32 = 0.160;
const LAND_RISE: f32 = 0.00035;
/// Motes higher than this, points, are not made to wait any longer: it bounds
/// how late any mote can land.
const RISE_CAP_PT: f32 = 160.0;
const FLIGHT: f32 = 0.24;
const GLOW: f32 = 0.18;
/// How far right a mote lands from where it was, points.
const LAND_SHIFT_PT: f32 = 10.0;

/// The woven line's lit tip: it is whole from [`HOT_AT`] and fades over
/// [`HOT_SPAN`]; it trails the front by at most [`HOT_LENGTH_PT`].
const HOT_AT: f32 = 0.40;
const HOT_SPAN: f32 = 0.32;
pub(crate) const HOT_LENGTH_PT: f32 = 150.0;

/// Seconds from the arrival at which nothing of the scene is left: the last
/// mote has landed and glowed out (the worst case, [`LAND_BASE`] plus the
/// longest sweep and rise) and the tip has faded.
pub(crate) const END_SECS: f32 = HOT_AT + HOT_SPAN;
const _: () = assert!(
    LAND_BASE + LAND_SWEEP + LAND_RISE * RISE_CAP_PT + FLIGHT + GLOW <= END_SECS,
    "a mote could still be lit when the scene ends"
);

/// How far the scene has faded in `waited` seconds after birth.
fn fade_in(waited: f32) -> f32 {
    unit((waited - SHOW as f32) / FADE_IN)
}

fn unit(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// Cubic ease-out, `0 → 1` (the arrival's curve, one copy of the shape).
fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - unit(x)).powi(3)
}

/// The opacity a mote is drawn with in an ink of relative luminance `ink`
/// over a ground of `ground`, for a mote of opacity `alpha` in the scene.
///
/// **The scene's opacities are how far a mote stands out, measured on black.**
/// The window blends in linear light, which spends little of the difference
/// between two dark colors and much between two light ones: the same opacity
/// that sets a light mote well apart from a black ground leaves a dark one
/// barely off a white ground (`0.15`: 39 points of lightness on black, 6 on
/// a light theme). So the opacity drawn is the one that sets the mote as far
/// from this ground in lightness (CIE L*) as `alpha` sets it from black, with
/// an ink as far from black as `ink` is from `ground` — on a black ground
/// exactly `alpha`, so the dark theme's dust is the dust it was tuned as. Any
/// theme, a user's too, gets dust that stands out alike.
pub(crate) fn opacity_over(alpha: f32, ink: f32, ground: f32) -> f32 {
    let (ink_l, ground_l) = (lightness(ink), lightness(ground));
    let reach = ink_l - ground_l;
    // An ink the ground's own colour shows nothing at any opacity.
    if reach.abs() < 0.5 {
        return alpha;
    }
    let apart = lightness(unit(alpha) * luminance_at(reach.abs()));
    let target = luminance_at(ground_l + apart.copysign(reach));
    unit((target - ground) / (ink - ground))
}

/// CIE lightness L* (`0..=100`) of a relative luminance.
fn lightness(luminance: f32) -> f32 {
    let y = luminance.clamp(0.0, 1.0);
    if y <= LAB_EPSILON {
        y * LAB_KAPPA
    } else {
        116.0 * y.cbrt() - 16.0
    }
}

/// The relative luminance of a CIE lightness: [`lightness`]'s inverse.
fn luminance_at(lightness: f32) -> f32 {
    let l = lightness.clamp(0.0, 100.0);
    if l <= LAB_EPSILON * LAB_KAPPA {
        l / LAB_KAPPA
    } else {
        ((l + 16.0) / 116.0).powi(3)
    }
}

/// CIE L*'s two constants: where its cube root gives way to a line, and the
/// line's slope (CIE's exact ratios, 216/24389 and 24389/27).
const LAB_EPSILON: f32 = 216.0 / 24389.0;
const LAB_KAPPA: f32 = 24389.0 / 27.0;

/// When a mote at `share` of the width (`0..=1`) and `rise` points above the
/// line is on the line, seconds after the arrival.
pub(crate) fn landing(share: f32, rise: f32) -> f32 {
    turn(share, rise) + FLIGHT
}

/// When a mote's flight begins, seconds after the arrival.
fn turn(share: f32, rise: f32) -> f32 {
    LAND_BASE + LAND_SWEEP * unit(share) + LAND_RISE * rise.clamp(0.0, RISE_CAP_PT)
}

/// How much of the line is woven `since` seconds after the arrival, as a share
/// of the width: [`landing`]'s inverse along the line.
pub(crate) fn front_at(since: f32) -> f32 {
    // Landing is linear in the share along the line, so its inverse is the
    // position between the first landing and the last.
    let (first, last) = (landing(0.0, 0.0), landing(1.0, 0.0));
    unit((since - first) / (last - first))
}

/// The scene's room: the window's width and how far down the dock reaches,
/// points. The dust lives from [`ABOVE_PT`] above the dock's top to `floor`
/// below it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Zone {
    pub(crate) width: f32,
    pub(crate) floor: f32,
}

impl Zone {
    /// The zone of a window `width_pt` wide whose dock band is `band_pt` high.
    pub(crate) fn for_dock(width_pt: f32, band_pt: f32) -> Self {
        Self {
            width: width_pt.max(1.0),
            floor: (band_pt - FLOOR_PT).max(FLOOR_MIN_PT),
        }
    }

    fn height(self) -> f32 {
        ABOVE_PT + self.floor
    }
}

/// One mote, drawn as a soft round dot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Mote {
    /// The centre across the window, points.
    pub(crate) x: f32,
    /// The centre below the dock's top, points (negative: above it).
    pub(crate) y: f32,
    /// The diameter, points.
    pub(crate) size: f32,
    /// The opacity, `0..=1`; `0` is not drawn.
    pub(crate) alpha: f32,
    /// How soft its edge is, points (`0`: sharp).
    pub(crate) blur: f32,
    /// Where its colour is between the text's (`0`) and the accent (`1`).
    pub(crate) tint: f32,
}

/// The dust scene at one moment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dust {
    /// How long the motes have drifted, seconds: the time since birth,
    /// stopped at the arrival (or the cap) — the pull starts from where they
    /// were.
    pub(crate) drift: f32,
    /// The whole scene's opacity while it fades in, `0..=1`. It carries on
    /// through the arrival, so a prompt in the middle of the fade-in makes
    /// nothing jump.
    pub(crate) appear: f32,
    /// Seconds since the arrival; `None` while the shell has not spoken.
    pub(crate) landing: Option<f32>,
}

impl Dust {
    /// The scene `waited` seconds after the pane was born, before the shell
    /// has spoken (after the hold, which the caller keeps).
    pub(crate) fn waiting(waited: f32) -> Self {
        Self {
            drift: waited.min(CAP as f32),
            appear: fade_in(waited),
            landing: None,
        }
    }

    /// The scene `since` seconds after the shell spoke, `waited` seconds after
    /// the pane was born: the motes stopped drifting then and the fade-in
    /// carries on.
    pub(crate) fn pulled(waited: f32, since: f32) -> Self {
        Self {
            drift: waited.min(CAP as f32),
            appear: fade_in(waited + since),
            landing: Some(since),
        }
    }

    /// How much of the line is woven, as a share of its width.
    pub(crate) fn front(&self) -> f32 {
        self.landing.map_or(0.0, front_at)
    }

    /// The woven line's tip: lit from the first landing, out at [`END_SECS`].
    pub(crate) fn hot(&self) -> f32 {
        match self.landing {
            Some(since) if front_at(since) > 0.0 => 1.0 - unit((since - HOT_AT) / HOT_SPAN),
            _ => 0.0,
        }
    }

    /// Every mote, in index order — those that cannot be seen too (`alpha` is
    /// `0`), so a mote is the same index at every time.
    pub(crate) fn motes(&self, zone: Zone) -> impl Iterator<Item = Mote> + '_ {
        let clouds = Clouds::new(zone);
        (0..MOTES).map(move |index| self.mote(index, zone, &clouds))
    }

    fn mote(&self, index: usize, zone: Zone, clouds: &Clouds) -> Mote {
        let seed = Seed::of(index, zone, clouds);
        let tm = self.drift;
        let sway = |amp: f32, (f1, p1): (f32, f32), (f2, p2): (f32, f32)| {
            amp * (0.62 * (TAU * f1 * tm + p1).sin() + 0.38 * (TAU * f2 * tm + p2).sin())
        };
        let x = seed.x + seed.vx * tm + sway(seed.amp_x, seed.fx.0, seed.fx.1);
        let y = seed.y + seed.vy * tm + sway(seed.amp_y, seed.fy.0, seed.fy.1);

        // Fades at the zone's edges: slowly into the top, quickly at the
        // floor and the sides.
        let edge = unit((y + ABOVE_PT) / 60.0)
            * unit((zone.floor - y) / 8.0)
            * unit(x / 16.0)
            * unit((zone.width - x) / 16.0);
        // Lit alike wherever they are, glinting now and then.
        let glint = {
            let s = 0.5 + 0.5 * (TAU * seed.glint.0 * tm + seed.glint.1).sin();
            0.5 + 0.5 * s * s * s
        };
        let depth = if seed.near { 0.16 } else { 0.07 + 0.3 * seed.z };
        let base = (depth * LIGHT * glint).min(0.85) * edge;
        let tint = if seed.accent { 1.0 } else { 0.0 };
        let blur = if seed.near {
            0.8 + 1.4 * (seed.z - 0.9) / 0.1
        } else {
            0.0
        };
        let mote = Mote {
            x,
            y,
            size: seed.size,
            alpha: base * self.appear,
            blur,
            tint,
        };
        let Some(since) = self.landing else {
            return mote;
        };

        // The pull: from where it drifted to the line, a little to the right,
        // in the accent; then a glow that goes out.
        let rise = (-y).max(0.0);
        let due = turn(x / zone.width, rise);
        let flight = unit((since - due) / FLIGHT);
        if flight < 1.0 {
            Mote {
                x: x + LAND_SHIFT_PT * ease_out(flight),
                y: y * (1.0 - flight * flight),
                size: seed.size + (1.6 - seed.size) * flight,
                alpha: (base + (0.9 - base) * flight) * self.appear,
                blur: blur * (1.0 - flight),
                tint: tint + (1.0 - tint) * flight,
            }
        } else {
            let out = unit((since - due - FLIGHT) / GLOW);
            Mote {
                x: x + LAND_SHIFT_PT,
                y: 0.0,
                size: 1.0 + 2.0 * (1.0 - out),
                alpha: 0.95 * (1.0 - out) * self.appear,
                blur: 0.0,
                tint: 1.0,
            }
        }
    }
}

/// The design's seeded generator (a 32-bit mixer): `0 ≤ value < 1`.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_add(0x6d2b_79f5);
        let mut q = self.0;
        q = (q ^ (q >> 15)).wrapping_mul(q | 1);
        q ^= q.wrapping_add((q ^ (q >> 7)).wrapping_mul(q | 61));
        let value = f64::from(q ^ (q >> 14)) / 4_294_967_296.0;
        // Rounding to `f32` can land on 1; the range is half-open.
        (value as f32).min(1.0 - f32::EPSILON)
    }

    /// A normally distributed value, mean 0 and spread 1 (Box–Muller).
    fn gauss(&mut self) -> f32 {
        let radius = (-2.0 * self.next().max(1.0e-6).ln()).sqrt();
        radius * (TAU * self.next()).cos()
    }
}

/// The three loose clouds a third of the motes gather in.
struct Clouds([Cloud; 3]);

#[derive(Clone, Copy)]
struct Cloud {
    x: f32,
    y: f32,
    spread_x: f32,
    spread_y: f32,
}

impl Clouds {
    fn new(zone: Zone) -> Self {
        let mut rng = Rng(4242);
        Self([(); 3].map(|()| {
            let x = zone.width * (0.12 + 0.76 * rng.next());
            let y = -ABOVE_PT + zone.height() * (0.25 + 0.6 * rng.next());
            let spread_x = 30.0 + 40.0 * rng.next();
            let spread_y = 18.0 + 22.0 * rng.next();
            Cloud {
                x,
                y,
                spread_x,
                spread_y,
            }
        }))
    }
}

/// What a mote is before time touches it.
struct Seed {
    x: f32,
    y: f32,
    z: f32,
    near: bool,
    size: f32,
    /// Frequency and phase of the two slow sines per axis.
    fx: ((f32, f32), (f32, f32)),
    fy: ((f32, f32), (f32, f32)),
    amp_x: f32,
    amp_y: f32,
    /// Drift, points per second.
    vx: f32,
    vy: f32,
    glint: (f32, f32),
    accent: bool,
}

impl Seed {
    /// The draws are in the design's order: a different order is a different
    /// dust.
    fn of(index: usize, zone: Zone, clouds: &Clouds) -> Self {
        let mut rng = Rng((index as u32).wrapping_mul(7919).wrapping_add(17));
        let in_cloud = rng.next() < 0.32;
        let cloud = clouds.0[(rng.next() * 3.0) as usize % 3];
        let x = if in_cloud {
            cloud.x + cloud.spread_x * rng.gauss()
        } else {
            zone.width * rng.next()
        };
        let y = if in_cloud {
            cloud.y + cloud.spread_y * rng.gauss()
        } else {
            -ABOVE_PT + zone.height() * rng.next()
        };
        let z = rng.next().powf(1.7);
        let near = z > 0.9;
        let size = if near {
            3.0 + 3.0 * (z - 0.9) / 0.1
        } else {
            0.9 + 2.2 * z * z
        };
        let f1 = 0.08 + 0.22 * rng.next();
        let f2 = 0.17 + 0.35 * rng.next();
        let f3 = 0.06 + 0.2 * rng.next();
        let f4 = 0.2 + 0.4 * rng.next();
        let p1 = TAU * rng.next();
        let p2 = TAU * rng.next();
        let p3 = TAU * rng.next();
        let p4 = TAU * rng.next();
        let vx = (rng.next() - 0.5) * 9.0 * (0.4 + z);
        let vy = (rng.next() * 1.4 - 0.9) * 5.0 * (0.4 + z);
        let glint = (0.3 + 1.4 * rng.next(), TAU * rng.next());
        let accent = rng.next() < 0.07;
        Self {
            x,
            y,
            z,
            near,
            size,
            fx: ((f1, p1), (f2, p2)),
            fy: ((f3, p3), (f4, p4)),
            amp_x: 5.0 + 16.0 * z,
            amp_y: 4.0 + 10.0 * z,
            vx,
            vy,
            glint,
            accent,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting(drift: f32) -> Dust {
        Dust {
            drift,
            appear: 1.0,
            landing: None,
        }
    }

    fn landing_at(drift: f32, since: f32) -> Dust {
        Dust {
            drift,
            appear: 1.0,
            landing: Some(since),
        }
    }

    fn list(dust: &Dust, zone: Zone) -> Vec<Mote> {
        dust.motes(zone).collect()
    }

    const ZONES: [(f32, f32); 4] = [(300.0, 34.0), (800.0, 68.0), (1400.0, 68.0), (120.0, 30.0)];

    fn zone(width: f32, band: f32) -> Zone {
        Zone::for_dock(width, band)
    }

    #[test]
    fn the_same_time_gives_the_same_motes() {
        for (width, band) in ZONES {
            let zone = zone(width, band);
            let one = list(&waiting(1.3), zone);
            assert_eq!(one.len(), MOTES);
            assert_eq!(one, list(&waiting(1.3), zone), "{width}");
            assert_ne!(one, list(&waiting(1.4), zone), "time moves them: {width}");
        }
        assert_ne!(
            list(&waiting(1.0), zone(300.0, 68.0)),
            list(&waiting(1.0), zone(800.0, 68.0)),
            "the width spreads them"
        );
    }

    #[test]
    fn a_visible_mote_is_inside_the_zone_and_most_are_visible() {
        for (width, band) in ZONES {
            let z = zone(width, band);
            for step in 0..=30 {
                let drift = 0.18 + step as f32 * 0.094;
                let motes = list(&waiting(drift), z);
                let mut seen = 0;
                for mote in motes.iter().filter(|mote| mote.alpha > 0.0) {
                    seen += 1;
                    assert!(
                        (0.0..=z.width).contains(&mote.x)
                            && (-ABOVE_PT..=z.floor).contains(&mote.y),
                        "{width}x{band} at {drift}: ({}, {})",
                        mote.x,
                        mote.y
                    );
                    assert!(mote.alpha <= 1.0 && mote.size > 0.0 && mote.blur >= 0.0);
                }
                assert!(seen >= 40, "{width}x{band} at {drift}: only {seen} seen");
            }
        }
    }

    #[test]
    fn nothing_shows_before_the_scene_has_faded_in() {
        let dust = Dust {
            appear: 0.0,
            ..waiting(1.0)
        };
        assert!(
            list(&dust, zone(800.0, 68.0))
                .iter()
                .all(|mote| mote.alpha == 0.0)
        );
        let half = Dust {
            appear: 0.5,
            ..waiting(1.0)
        };
        let full = list(&waiting(1.0), zone(800.0, 68.0));
        for (half, full) in list(&half, zone(800.0, 68.0)).iter().zip(&full) {
            assert!((half.alpha - full.alpha * 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn some_motes_are_near_and_soft_and_the_rest_are_sharp() {
        let motes = list(&waiting(1.0), zone(800.0, 68.0));
        let soft = motes.iter().filter(|mote| mote.blur > 0.0).count();
        assert!((3..=30).contains(&soft), "{soft} soft motes");
        assert!(motes.iter().all(|mote| mote.size <= 6.0));
        assert!(motes.iter().any(|mote| mote.tint == 1.0), "accent motes");
        assert!(motes.iter().any(|mote| mote.tint == 0.0));
    }

    #[test]
    fn motes_land_left_to_right_and_the_higher_ones_a_little_later() {
        let mut last = f32::MIN;
        for step in 0..=20 {
            let at = landing(step as f32 / 20.0, 0.0);
            assert!(at > last, "{step}");
            last = at;
        }
        assert!(landing(0.5, 80.0) > landing(0.5, 0.0));
        assert_eq!(landing(-1.0, -5.0), landing(0.0, 0.0), "clamped");
        assert_eq!(landing(2.0, 1.0e6), landing(1.0, 160.0), "bounded");
    }

    #[test]
    fn a_mote_is_on_the_line_from_the_moment_it_lands() {
        let z = zone(800.0, 68.0);
        let drift = 1.2;
        let at_rest = list(&waiting(drift), z);
        let mut checked = 0;
        for (index, mote) in at_rest.iter().enumerate() {
            if mote.y >= 0.0 || mote.x < 0.0 || mote.x > z.width {
                continue;
            }
            let due = landing(mote.x / z.width, -mote.y);
            let before = list(&landing_at(drift, due - 0.01), z)[index];
            let after = list(&landing_at(drift, due + 0.01), z)[index];
            assert!(before.y < 0.0, "{index} is still on its way");
            assert_eq!(after.y, 0.0, "{index} is on the line");
            assert_eq!(after.tint, 1.0, "and lit in the accent");
            assert!(after.x > mote.x, "pulled a little right");
            checked += 1;
        }
        assert!(checked >= 20, "{checked}");
    }

    #[test]
    fn the_pull_starts_from_the_look_the_motes_had() {
        // The prompt moves nothing by itself: every mote's first pulled frame
        // is the one it was drawn as while waiting, blur and all.
        for (width, band) in ZONES {
            let z = zone(width, band);
            for drift in [0.3, 1.2, 3.0] {
                assert_eq!(
                    list(&waiting(drift), z),
                    list(&landing_at(drift, 0.0), z),
                    "{width}x{band} at {drift}"
                );
            }
        }
    }

    #[test]
    fn the_weld_front_is_the_landing_seen_from_the_line() {
        assert_eq!(front_at(0.0), 0.0);
        assert_eq!(front_at(landing(0.0, 0.0) - 0.001), 0.0);
        assert_eq!(front_at(landing(1.0, 0.0) + 0.001), 1.0);
        for step in 0..=20 {
            let share = step as f32 / 20.0;
            assert!(
                (front_at(landing(share, 0.0)) - share).abs() < 1e-4,
                "{share}"
            );
        }
        assert_eq!(landing_at(1.0, 0.1).front(), front_at(0.1));
        assert_eq!(waiting(1.0).front(), 0.0);
    }

    #[test]
    fn everything_has_landed_and_gone_out_by_the_end() {
        for (width, band) in ZONES {
            for step in 0..=29 {
                let drift = 0.18 + step as f32 * 0.1;
                let z = zone(width, band);
                let done = landing_at(drift, END_SECS);
                assert!(
                    list(&done, z).iter().all(|mote| mote.alpha == 0.0),
                    "{width}x{band} at {drift}"
                );
                assert_eq!(done.front(), 1.0);
                assert_eq!(done.hot(), 0.0, "the tip is out");
                let gone = landing_at(drift, 1.0);
                assert!(list(&gone, z).iter().all(|mote| mote.alpha == 0.0));
            }
        }
        // Just before the end something is still lit.
        let z = zone(800.0, 68.0);
        let late = landing_at(1.2, END_SECS - GLOW - 0.2);
        assert!(list(&late, z).iter().any(|mote| mote.alpha > 0.0));
    }

    /// The relative luminance of `0xRRGGBB`.
    fn luminance(hex: u32) -> f32 {
        let [r, g, b] = [16, 8, 0].map(|shift| ((hex >> shift) & 0xff) as u8);
        bt_core::LinearRgba::from_srgb(r, g, b).luminance()
    }

    #[test]
    fn on_black_a_mote_is_drawn_as_the_scene_says() {
        for ink in [0xd8d9dd, 0x7a9cc6, 0xffffff, 0x404040] {
            for step in 0..=20 {
                let alpha = step as f32 / 20.0;
                let drawn = opacity_over(alpha, luminance(ink), 0.0);
                assert!((drawn - alpha).abs() < 1e-4, "#{ink:06x} {alpha}: {drawn}");
            }
        }
    }

    #[test]
    fn on_a_light_ground_a_mote_stands_out_as_much_as_on_black() {
        // The light themes' text on their grounds. Drawn there, a mote is as
        // far from the ground in lightness as it is over black in an ink as far
        // from black — the blend worked out here, not by the function — and
        // a little less than the dark theme's own dust, as their text is a
        // little nearer its ground (82 and 79 points of lightness to 88).
        let apart = |alpha: f32, ink: f32, ground: f32| {
            (lightness(alpha * ink + (1.0 - alpha) * ground) - lightness(ground)).abs()
        };
        let dark_ink = luminance(0xd8d9dd);
        for (ink, ground) in [(0x24262c, 0xf5f6f8), (0x2a2520, 0xf3efe7)] {
            let (ink, ground) = (luminance(ink), luminance(ground));
            let mirror = luminance_at((lightness(ink) - lightness(ground)).abs());
            let mut last = 0.0;
            for step in 1..20 {
                let alpha = step as f32 / 20.0;
                let drawn = opacity_over(alpha, ink, ground);
                assert!(drawn > alpha && drawn <= 1.0, "{alpha}: {drawn}");
                assert!(drawn > last, "the order is kept: {alpha}");
                last = drawn;
                let here = apart(drawn, ink, ground);
                let mirrored = apart(alpha, mirror, 0.0);
                assert!(
                    (here - mirrored).abs() < 0.1,
                    "{alpha}: {here} vs {mirrored}"
                );
                let dark = apart(alpha, dark_ink, 0.0);
                assert!(
                    here <= dark && here >= dark * 0.85,
                    "{alpha}: {here} vs the dark {dark}"
                );
            }
            assert!(
                opacity_over(0.0, ink, ground) < 1e-6,
                "nothing stays nothing"
            );
            assert!(
                (opacity_over(1.0, ink, ground) - 1.0).abs() < 1e-4,
                "the ink itself"
            );
        }
    }

    #[test]
    fn an_ink_the_grounds_own_colour_is_left_as_it_is() {
        let grey = luminance(0x808080);
        assert_eq!(opacity_over(0.3, grey, grey), 0.3);
    }

    #[test]
    fn lightness_and_its_inverse_meet() {
        for step in 0..=100 {
            let l = step as f32;
            assert!((lightness(luminance_at(l)) - l).abs() < 1e-3, "{l}");
        }
        assert_eq!(lightness(0.0), 0.0);
        assert!((lightness(1.0) - 100.0).abs() < 1e-3);
    }

    #[test]
    fn the_tip_is_lit_while_the_line_is_woven_and_fades_after() {
        assert_eq!(waiting(1.0).hot(), 0.0);
        assert_eq!(landing_at(1.0, 0.1).hot(), 0.0, "no line yet, no tip");
        assert_eq!(landing_at(1.0, 0.3).hot(), 1.0);
        let fading = landing_at(1.0, 0.5).hot();
        assert!(fading > 0.0 && fading < 1.0, "{fading}");
        assert_eq!(landing_at(1.0, END_SECS).hot(), 0.0);
    }
}
