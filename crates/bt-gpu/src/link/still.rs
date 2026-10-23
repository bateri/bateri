//! A still of the pane: the picture a card over a split tab's chip shows of
//! each pane, composed again from the session on demand and drawn into
//! textures that are the card's, not the window's.
//!
//! **It is the frame's other target, not another frame.** The picture is the
//! pane's frame as the content frame would build it for the window — the
//! grid, the fill band, the dock, the pictures a program placed — because it
//! goes through the same composition ([`push_lists`], [`place_dock`],
//! [`place_caret`] and the same [`Frame`] setters, in the same order), and
//! it is drawn by the same renderer, at the surface's own size so that every
//! cell lands where it does on screen. What differs is what a frame *does*
//! and a still does not:
//!
//! - **It asks nothing and takes nothing.** `take_damage` is never called —
//!   the damage the window is owed stays owed — and neither is anything that
//!   plants it: no `Waker`, no `request_frame`, no pacer. `Session::frame`
//!   runs, as it would for the next content frame, and so does its own
//!   bookkeeping (the alternate screen's watch, the search slot, the
//!   observation of unseen rows): idempotent reads and once-per-transition
//!   notices that the next frame would make just the same.
//! - **It counts nowhere.** `content=`, `motion=`, `slide=`, `requests=`, the
//!   completion queue, [`Stats`] and the renderer's `frames=`/`cells=` belong
//!   to frames; a still reaches none of them (the renderer's side is
//!   [`Renderer::snapshot`](crate::Renderer)).
//! - **It steps no animator.** Nothing of the link's own — its [`Motion`],
//!   blink, typing effects, scroll bar, `Origin`, last caret — is read or
//!   written; the still keeps a [`Shot`] of its own for the frame and the
//!   buffers `Session::frame` and `Session::dock` fill, and a [`Motion`] born
//!   for the call that is told the targets and finished. So the picture is the
//!   state the animations are heading to: the cursor where it will be, the
//!   content where it will settle, the dock arrived, no typing effect in
//!   flight, the scroll bar down. The caret is drawn in its open phase, in the
//!   pane's focus: solid in the key window's terminal, hollow otherwise.
//! - **It does not tell the session where it was drawn.** `set_grid_top` (the
//!   slide's covering strip) is left to the last content frame; the dock's
//!   own hit-test window is written by `Session::dock` with the very numbers
//!   the content frame writes.
//!
//! **Known limit**: `Session::dock` also reads the window it last recorded to
//! shift the typing effects of an input that scrolls inside its box; a still
//! made between two content frames of such an input moves that reference
//! by a row for one frame. Nothing else is shared.

use bt_core::ScrollGlide;

use super::*;
use crate::renderer::shrink::Chain;

/// What a still keeps between calls: the frame and the buffers that composing
/// it fills, and the textures the reduction runs through ([`Chain`]).
///
/// Made empty ([`Shot::default`]); the first still fills it and the textures
/// are allocated for that still's sizes, reused while the sizes stay and
/// remade when they change. **Dropping it gives back everything** — the
/// owner (a pane, for as long as a card shows it) holds it only as long as it
/// is wanted.
#[derive(Default)]
pub struct Shot {
    frame: Frame,
    fill: Vec<bt_core::Cell>,
    blocks: Blocks,
    selection: SelectionRuns,
    search: SearchRuns,
    marks: TrackMarks,
    dock: DockState,
    dock_context: DockContext,
    dock_selection: Vec<SelectionRun>,
    chain: Option<Chain>,
}

/// A still, as pixels: `width × height` of them, four bytes each — red, green,
/// blue, alpha, rows top down — in the **sRGB** colour space (the encoded
/// values, as an image carries them), alpha opaque.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl DisplayLink {
    /// The pane as it is now, brought to `tile` pixels: the whole of its
    /// frame, drawn at the surface's size and shrunk on the GPU
    /// ([`Renderer::snapshot`](crate::Renderer)) so only the tile crosses to
    /// the CPU. When the tile's shape is not the pane's, the picture fills
    /// the tile and keeps its left edge and its bottom.
    ///
    /// `None` when there is nothing to show: the surface has no size yet, the
    /// tile is empty or the GPU said no. A still is a courtesy, so a failure
    /// is not reported to the frame path's policy — the owner shows the last
    /// one it had, or none.
    ///
    /// Main thread, like the link. **Not a frame** — the module header says
    /// what that means.
    pub fn snapshot(&self, shot: &mut Shot, tile: (u32, u32)) -> Option<Snapshot> {
        self.core.snapshot(shot, tile)
    }
}

impl Core {
    fn snapshot(&self, shot: &mut Shot, tile: (u32, u32)) -> Option<Snapshot> {
        let size = self.surface.size()?;
        if tile.0 == 0 || tile.1 == 0 {
            return None;
        }
        let clear = compose_still(shot, &self.session, &self.view(), size);
        let rgba = self
            .renderer
            .snapshot(&mut shot.chain, clear, &shot.frame, size, tile)
            .ok()?;
        Some(Snapshot {
            width: tile.0,
            height: tile.1,
            rgba,
        })
    }
}

/// Builds `shot`'s frame for `session` drawn at `size` pixels and returns the
/// colour the pass clears to — the content frame ([`Core::tick`])'s
/// composition at rest, in the order the content frame has, which is required:
/// each step leans on the one before it (the module header).
fn compose_still(shot: &mut Shot, session: &Session, view: &View, size: (u32, u32)) -> LinearRgba {
    let viewport_height = size.1 as f32;
    let theme = session.theme();
    let frame = &mut shot.frame;
    frame.clear(view.cell, view.caret_style);
    shot.fill.clear();
    let mut clusters = frame.take_clusters();
    let mut pictures = frame.take_pictures();
    let cursor = session.frame(
        |cell| frame.push(cell),
        |cell| shot.fill.push(cell),
        &mut shot.blocks,
        &mut shot.selection,
        &mut shot.search,
        &mut shot.marks,
        &mut clusters,
        &mut pictures,
        // No share of a notch's glide: it moves the window the frame path
        // is scrolling, and a still is at rest.
        ScrollGlide::default(),
        DockBudget {
            share: crate::frame::DOCK_MAX_SHARE,
            cols: view.dock_cols,
        },
    );
    frame.put_clusters(clusters);
    frame.put_pictures(pictures);
    push_lists(
        frame,
        &cursor,
        &mut shot.fill,
        &shot.blocks,
        &shot.selection,
        &shot.search,
        view.focused,
    );
    place_edge(
        frame,
        view.edge,
        viewport_height,
        view.dock_rows,
        cursor.rows,
        view.cell,
    );
    frame.set_grid(cursor.cols, cursor.rows);
    let band_rows = cursor.band_rows();
    // A motion of its own, born with no arrival scene and no state: the dock
    // is drawn arrived, and the caret's target is all it is told.
    let mut motion = Motion::default();
    let dock_caret = place_dock(
        session,
        frame,
        view,
        &cursor,
        band_rows,
        DockBufs {
            state: &mut shot.dock,
            context: &mut shot.dock_context,
            selection: &mut shot.dock_selection,
        },
        &mut motion,
        // No typing effects: the dock at rest, every letter in its place.
        None,
        &theme,
    );
    let Placed { caret, band_target } =
        place_caret(frame, view, &cursor, dock_caret, band_rows, viewport_height);
    // The targets, and the animation told it has arrived: the offset, the
    // band and the cursor stand where the content frame is taking them.
    motion.sync(
        caret.map(|(at, _)| at),
        origin_target(cursor),
        band_target,
        cursor.display_offset,
        true,
        cursor.fill > 0,
    );
    motion.finish();
    compose(frame, motion, viewport_height, view.dock_rows);
    if let (Some(at), Some((_, text))) = (motion.position(), caret) {
        frame.push_caret(
            at,
            text,
            theme.cursor_linear(),
            // The open phase of a blink, and no arrival scene hides it.
            motion.alpha() * motion.arrival_caret(),
            cursor.shape,
            view.caret_focused,
        );
    }
    theme.background_linear()
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Instant;

    use bt_core::{CaretShape, CursorBlink, Osc52, SessionOptions, TerminalOptions, Wake};

    use super::*;
    use crate::renderer::tests::{grid, pixel_at};

    /// A `Wake` that hears everything and does nothing.
    struct QuietWake;

    impl Wake for QuietWake {
        fn wake(&self) {}
        fn child_exit(&self, _code: Option<i32>) {}
        fn copy_to_clipboard(&self, _text: String) {}
        fn title_changed(&self) {}
        fn search_changed(&self) {}
        fn command_started(&self) {}
        fn phase_edge(&self) {}
        fn remote_command_edge(&self) {}
        fn mirror_changed(&self) {}
        fn environment_changed(&self) {}
        fn remote_up(&self) {}
        fn remote_typed(&self) {}
        fn link_hover_lost(&self) {}
        fn pointer_changed(&self) {}
        fn blocks_changed(&self) {}
        fn unseen_changed(&self) {}
        fn program_status_changed(&self) {}
        fn bell(&self) {}
        fn notification(&self) {}
    }

    /// A 16×16 grid of 8×8-pixel cells (a 128-pixel square) on a shell that
    /// paints three cells red and then waits.
    fn session() -> Session {
        session_running("printf '\\033[41m   \\033[0m'; exec sleep 30")
    }

    /// The same grid on a shell that runs `script`.
    fn session_running(script: &str) -> Session {
        Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/sh".to_owned(),
                    vec!["-c".to_owned(), script.to_owned()],
                )),
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols: 16,
                rows: 16,
                cell_px: (8, 8),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                dock: false,
                cluster: true,
                initial_input: None,
                shell_marks: false,
                pane_uuid: None,
                hostname: None,
                replay: None,
                views: None,
            },
            Arc::new(QuietWake),
        )
        .expect("session")
    }

    fn view() -> View {
        View {
            cell: grid(8, 8),
            caret_style: CaretStyle::default(),
            edge: ContentEdge::default(),
            dock_rows: 0,
            dock_cols: 16,
            context: ContextStrip::default(),
            focused: true,
            caret_focused: true,
        }
    }

    /// Composes stills until `cells` painted cells are in the frame; the
    /// damage is never asked for, so the wait does not eat it.
    fn composed_until(session: &Session, shot: &mut Shot, cells: usize) -> LinearRgba {
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let clear = compose_still(shot, session, &view(), (128, 128));
            if shot.frame.bg_count() >= cells {
                return clear;
            }
            assert!(Instant::now() < deadline, "the shell's output never came");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The three red cells of [`session`].
    fn composed(session: &Session, shot: &mut Shot) -> LinearRgba {
        composed_until(session, shot, 3)
    }

    /// **A program on the alternate screen is in the picture as it is on screen** (vim, htop):
    /// the still reads the screen the session shows, not the one under it.
    #[test]
    fn a_program_on_the_alternate_screen_stands_in_the_picture() {
        // Three red cells on the main screen, then a full-screen program that paints six blue
        // ones at its top left and stays.
        let session = session_running(
            "printf '\\033[41m   \\033[0m'; printf '\\033[?1049h\\033[2J\\033[1;1H\\033[44m      '; \
             exec sleep 30",
        );
        let mut shot = Shot::default();
        // Until the program's six cells are the ones in the frame (the red three come first).
        let deadline = Instant::now() + std::time::Duration::from_secs(10);
        let clear = loop {
            let clear = compose_still(&mut shot, &session, &view(), (128, 128));
            if session.alt_screen() && shot.frame.bg_count() >= 6 {
                break clear;
            }
            assert!(Instant::now() < deadline, "the program's screen never came");
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(
            shot.frame.bg_count(),
            6,
            "the main screen's red cells are under the program's, not beside them"
        );
        let renderer = Renderer::new();
        let direct = renderer.render_offscreen(128, clear, &shot.frame);
        let blue = pixel_at(&direct, 128, 4, 4);
        assert!(
            blue.2 > 100 && blue.2 > blue.0 + 50,
            "the program's blue cells are not at the top: {blue:?}"
        );
        let red_place = pixel_at(&direct, 128, 4, 124);
        assert!(
            red_place.0 < 100 || red_place.0 <= red_place.2,
            "the main screen's bottom row shows through: {red_place:?}"
        );
    }

    #[test]
    fn a_still_is_the_frame_in_another_target_and_asks_nothing_of_the_session() {
        let session = session();
        let mut shot = Shot::default();
        let clear = composed(&session, &mut shot);

        // **The damage the window is owed is still owed**: the shell wrote,
        // the reader planted it, and many stills later nobody took it.
        assert!(
            session.take_damage(),
            "a still took the damage the next content frame is owed"
        );
        // And taking it was the test's: a still plants none either.
        let mut again = Shot::default();
        compose_still(&mut again, &session, &view(), (128, 128));
        assert!(!session.take_damage(), "a still planted damage");

        // **The same frame**: drawn through the still's path at the frame's
        // own size it is what drawing it into a window-sized target gives.
        let renderer = Renderer::new();
        let direct = renderer.render_offscreen(128, clear, &shot.frame);
        let counts = (
            renderer.frames(),
            renderer.last_bg_count(),
            renderer.last_glyph_count(),
            renderer.last_rule_count(),
        );
        let still = renderer
            .snapshot(&mut shot.chain, clear, &shot.frame, (128, 128), (128, 128))
            .expect("the still");
        assert_eq!(still.len(), 128 * 128 * 4);
        for (index, rgba) in still.chunks_exact(4).enumerate() {
            let at = index * 4;
            // Bgra in the direct picture, rgba in the still.
            let want = [direct[at + 2], direct[at + 1], direct[at], direct[at + 3]];
            for (got, want) in rgba.iter().zip(want) {
                assert!(
                    got.abs_diff(want) <= 1,
                    "pixel {index} differs: {rgba:?} against {want:?}"
                );
            }
        }
        // The three red cells are really in it — in the bottom row, where a
        // grid with one line of content keeps it — and the ground around them.
        let red = pixel_at(&direct, 128, 4, 124);
        assert!(
            red.0 > 100 && red.0 > red.1 + 50 && red.0 > red.2 + 50,
            "the painted cells are not red: {red:?}"
        );
        let ground = pixel_at(&direct, 128, 100, 100);
        assert_ne!(ground, red);

        // **It counts nowhere**: not a frame submitted, not a cell drawn, and
        // nothing left in flight for the completion poll to find.
        assert_eq!(
            counts,
            (
                renderer.frames(),
                renderer.last_bg_count(),
                renderer.last_glyph_count(),
                renderer.last_rule_count()
            ),
            "a still moved the frame counters"
        );
        assert!(!renderer.poll(|_| ()), "a still left a frame in flight");
    }

    /// **Structure as a guard**: a still is an extension of `Core` that may only read — it takes
    /// `&self` and its code (comments aside) never names the cells and counters a frame moves.
    /// The runtime guards above show the session's damage and the renderer's counters stay; this
    /// one covers what only `Core` holds (`content=`, `motion=`, `slide=`, `requests=`, `Stats`,
    /// the animators), which a test without a window cannot build a `Core` to read.
    #[test]
    fn the_still_path_never_names_the_links_state() {
        let source = include_str!("still.rs");
        let production = source
            .split("#[cfg(test)]")
            .next()
            .expect("the file has a first part");
        let code: String = production
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for state in [
            "self.waker",
            "self.stats",
            "self.retry",
            "self.motion",
            "self.blink",
            "self.glyph_fx",
            "self.scrollbar",
            "self.origin",
            "self.frame",
            "self.theme",
            "self.marks",
            "self.blocks",
            "content_frames",
            "motion_frames",
            "slide_frames",
            "take_damage",
            "request_frame",
            "set_grid_top",
            "keep_damage",
            "set_running",
            ".draw(",
            ".present(",
            ".poll(",
        ] {
            assert!(
                !code.contains(state),
                "the still path names `{state}`: a still may read the link, not move it"
            );
        }
        // And `Core::snapshot` borrows the link shared: nothing in it can be set.
        assert!(code.contains("fn snapshot(&self, shot: &mut Shot"));
    }

    #[test]
    fn a_still_made_twice_reuses_its_textures_and_remakes_them_for_another_size() {
        let session = session();
        let mut shot = Shot::default();
        let clear = composed(&session, &mut shot);
        let renderer = Renderer::new();
        let tile = (32, 32);
        let first = renderer
            .snapshot(&mut shot.chain, clear, &shot.frame, (128, 128), tile)
            .expect("the first still");
        let chain = shot.chain.as_ref().map(|chain| chain.serial());
        let second = renderer
            .snapshot(&mut shot.chain, clear, &shot.frame, (128, 128), tile)
            .expect("the second still");
        assert_eq!(
            shot.chain.as_ref().map(|chain| chain.serial()),
            chain,
            "the same sizes remade the textures"
        );
        assert_eq!(first, second);
        // A tile of another size gets its own.
        let other = renderer
            .snapshot(&mut shot.chain, clear, &shot.frame, (128, 128), (16, 16))
            .expect("a smaller still");
        assert_eq!(other.len(), 16 * 16 * 4);
    }
}
