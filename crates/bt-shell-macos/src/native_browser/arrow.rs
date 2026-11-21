//! What the arrow of a swipe between pages looks like and where it is, as numbers: a dark disc that
//! comes out of the room's edge as the fingers move, a lighter one that grows inside it as the pull
//! fills, and an arrow on top (Chrome's indicator, which is the same three things). Pure, so it is
//! tested without a renderer; [`super::Pictures`] puts it in the draw list.

use super::swipe::{Arrow, Way};
use std::time::Duration;

/// How long the arrow takes to go back to its edge after a pull that was too short.
pub(super) const WITHDRAW: Duration = Duration::from_millis(180);
/// How long the arrow takes to fade out when the browser goes.
pub(super) const TAKEN: Duration = Duration::from_millis(220);

/// The disc's radius and its margin from the edge when full, points.
const RADIUS: f64 = 20.0;
const MARGIN: f64 = 10.0;

/// The side of the arrow's picture, pixels.
pub(super) const GLYPH: u32 = 64;

/// What shows of the arrow now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Look {
    pub way: Way,
    /// 0 at the edge, 1 when the swipe would count (it may go past).
    pub fill: f64,
    pub opacity: f64,
}

/// What shows `since` the arrow was set to `arrow`; `None` where nothing does (hidden, or the
/// motion that follows the fingers' leaving is over).
pub(super) fn look(arrow: Arrow, since: Duration) -> Option<Look> {
    match arrow {
        Arrow::Hidden => None,
        Arrow::Pulled { way, fill } => Some(Look {
            way,
            fill,
            opacity: 1.0,
        }),
        Arrow::Taken { way } => (since < TAKEN).then(|| Look {
            way,
            fill: 1.0,
            opacity: 1.0 - since.as_secs_f64() / TAKEN.as_secs_f64(),
        }),
        Arrow::Withdrawn { way, fill } => (since < WITHDRAW).then(|| {
            let t = since.as_secs_f64() / WITHDRAW.as_secs_f64();
            // Eases out: quick at first, settling at the edge.
            let left = (1.0 - t) * (1.0 - t);
            Look {
                way,
                fill: fill * left,
                opacity: 1.0,
            }
        }),
    }
}

/// Whether the arrow moves by itself, which is to say that frames are wanted until it stops.
pub(super) fn moves_alone(arrow: Arrow) -> bool {
    matches!(arrow, Arrow::Taken { .. } | Arrow::Withdrawn { .. })
}

/// Where the parts are, in the pixels of the room's visible rectangle `(x, y, w, h)`; `scale` is
/// pixels to the point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Place {
    /// The dark disc: x, y, w, h.
    pub disc: [f32; 4],
    /// The disc that grows inside it as the pull fills.
    pub filling: [f32; 4],
    /// The arrow's picture.
    pub glyph: [f32; 4],
    /// The disc's radius.
    pub radius: f32,
}

pub(super) fn place(look: Look, visible: [f32; 4], scale: f32) -> Place {
    let r = RADIUS as f32 * scale;
    let margin = MARGIN as f32 * scale;
    let [x, y, w, h] = visible;
    let travel = look.fill.clamp(0.0, 1.0) as f32;
    // At 0 the disc is just outside the edge, at 1 it stands `margin` inside it.
    let centre_x = match look.way {
        Way::Back => x - r + travel * (2.0 * r + margin),
        Way::Forward => x + w + r - travel * (2.0 * r + margin),
    };
    let centre_y = y + h / 2.0;
    let square = |radius: f32| {
        [
            centre_x - radius,
            centre_y - radius,
            2.0 * radius,
            2.0 * radius,
        ]
    };
    let glyph = 1.15 * r;
    Place {
        disc: square(r),
        filling: square(r * travel),
        glyph: [centre_x - glyph / 2.0, centre_y - glyph / 2.0, glyph, glyph],
        radius: r,
    }
}

/// The arrow's picture: white on nothing, `GLYPH` pixels a side, rows from the top, premultiplied
/// RGBA. An arrow that points the way back (to the left), or its mirror.
pub(super) fn glyph_pixels(way: Way) -> Vec<u8> {
    let size = f64::from(GLYPH);
    // The shaft and the two strokes of the head, in shares of the side.
    let strokes = [
        ((0.22, 0.5), (0.78, 0.5)),
        ((0.22, 0.5), (0.46, 0.26)),
        ((0.22, 0.5), (0.46, 0.74)),
    ];
    let half = 0.055;
    let mut pixels = Vec::with_capacity((GLYPH * GLYPH * 4) as usize);
    for row in 0..GLYPH {
        for column in 0..GLYPH {
            let mut x = (f64::from(column) + 0.5) / size;
            let y = (f64::from(row) + 0.5) / size;
            if way == Way::Forward {
                x = 1.0 - x;
            }
            let near = strokes
                .iter()
                .map(|(from, to)| distance((x, y), *from, *to))
                .fold(f64::MAX, f64::min);
            // A pixel is covered by how far the stroke's edge is into it.
            let cover = ((half - near) * size + 0.5).clamp(0.0, 1.0);
            let a = (cover * 255.0).round() as u8;
            pixels.extend_from_slice(&[a, a, a, a]);
        }
    }
    pixels
}

/// How far `p` is from the segment `a`–`b`.
fn distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let along = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    let (cx, cy) = (a.0 + along * dx, a.1 + along * dy);
    ((p.0 - cx).powi(2) + (p.1 - cy).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(arrow: Arrow, ms: u64) -> Option<Look> {
        look(arrow, Duration::from_millis(ms))
    }

    #[test]
    fn the_arrow_follows_the_fingers_and_goes_back_to_the_edge_when_let_go_short() {
        let pulled = Arrow::Pulled {
            way: Way::Back,
            fill: 0.4,
        };
        assert_eq!(
            at(pulled, 5000),
            Some(Look {
                way: Way::Back,
                fill: 0.4,
                opacity: 1.0
            })
        );
        let withdrawn = Arrow::Withdrawn {
            way: Way::Back,
            fill: 0.4,
        };
        let start = at(withdrawn, 0).unwrap();
        assert!((start.fill - 0.4).abs() < 1e-9);
        let half = at(withdrawn, 90).unwrap();
        assert!(half.fill > 0.0 && half.fill < 0.4 / 2.0, "{}", half.fill);
        assert_eq!(at(withdrawn, 180), None);
        assert!(moves_alone(withdrawn) && !moves_alone(pulled));
    }

    #[test]
    fn the_arrow_that_took_the_browser_fades_out_full() {
        let taken = Arrow::Taken { way: Way::Forward };
        let early = at(taken, 0).unwrap();
        assert_eq!((early.fill, early.opacity), (1.0, 1.0));
        let later = at(taken, 110).unwrap();
        assert!((later.opacity - 0.5).abs() < 1e-9);
        assert_eq!(at(taken, 220), None);
        assert_eq!(at(Arrow::Hidden, 0), None);
    }

    #[test]
    fn the_disc_comes_out_of_the_edge_it_belongs_to() {
        let room = [100.0, 40.0, 800.0, 600.0];
        let back = |fill: f64| {
            place(
                Look {
                    way: Way::Back,
                    fill,
                    opacity: 1.0,
                },
                room,
                2.0,
            )
        };
        // At nothing it is outside the room, touching its edge.
        let none = back(0.0);
        assert!((none.disc[0] + none.disc[2] - 100.0).abs() < 1e-3);
        assert_eq!(none.filling[2], 0.0);
        // Full, it stands a margin inside, and the inner disc is as big as it.
        let full = back(1.0);
        assert!(
            (full.disc[0] - (100.0 + 20.0)).abs() < 1e-3,
            "{:?}",
            full.disc
        );
        assert!((full.filling[2] - full.disc[2]).abs() < 1e-3);
        // Past full it does not go on.
        assert_eq!(back(2.5).disc, full.disc);
        // Vertically in the middle of what shows.
        assert!((full.disc[1] + full.disc[3] / 2.0 - 340.0).abs() < 1e-3);
        // Forward is the mirror at the other edge.
        let forward = place(
            Look {
                way: Way::Forward,
                fill: 1.0,
                opacity: 1.0,
            },
            room,
            2.0,
        );
        assert!((forward.disc[0] + forward.disc[2] - (900.0 - 20.0)).abs() < 1e-3);
        // The arrow is centred in the disc.
        let centre = |r: [f32; 4]| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0);
        assert_eq!(centre(forward.glyph), centre(forward.disc));
    }

    #[test]
    fn the_arrows_picture_is_an_arrow_and_its_mirror() {
        let back = glyph_pixels(Way::Back);
        let forward = glyph_pixels(Way::Forward);
        let side = GLYPH as usize;
        assert_eq!(back.len(), side * side * 4);
        let alpha = |pixels: &[u8], x: usize, y: usize| pixels[(y * side + x) * 4 + 3];
        // The shaft is solid, the corner is nothing, and the pixels are premultiplied.
        assert_eq!(alpha(&back, side / 2, side / 2), 255);
        assert_eq!(alpha(&back, 1, 1), 0);
        assert!(
            back.chunks(4)
                .all(|p| p[0] == p[3] && p[1] == p[3] && p[2] == p[3])
        );
        // The head is at the left of the way back and at the right of the way forward.
        assert_eq!(alpha(&back, side * 22 / 100, side / 2), 255);
        assert_eq!(alpha(&back, side * 90 / 100, side / 2), 0);
        for y in 0..side {
            for x in 0..side {
                assert_eq!(alpha(&back, x, y), alpha(&forward, side - 1 - x, y));
            }
        }
    }
}
