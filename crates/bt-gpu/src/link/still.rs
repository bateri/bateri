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
//! **A pane that shows a page is the page's picture**, not the grid under it: the link keeps the
//! list the page's last frame drew ([`PageStills`], written by the frame path when a frame
//! reaches the screen) and a still draws that — at the size it was built for, over the colour it
//! was drawn over — instead of composing the terminal. The page is not asked for anything:
//! `UiPage::build` takes `&mut self` and moves the page's own state (what it asked for, when it
//! wants a frame, what it lays out), which is a frame's business, so a still reads what the
//! frame left. It is therefore the page as it is *on screen*, an animation in flight at the
//! step it was drawn at, and a page that has not drawn yet has no picture of its own: the
//! terminal's is what the tile shows until it has. The image textures the list names are the
//! live ones, used where they stand ([`Renderer::snapshot_ui`](crate::Renderer)).
//!
//! **Known limit**: `Session::dock` also reads the window it last recorded to
//! shift the typing effects of an input that scrolls inside its box; a still
//! made between two content frames of such an input moves that reference
//! by a row for one frame. Nothing else is shared.

use std::cell::RefCell;

use bt_core::ScrollGlide;

use super::*;
use crate::renderer::shrink::Chain;
use crate::renderer::ui::UiList;

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
    /// A pane that shows a page that has drawn is the page's last picture
    /// ([`PageStills`]), whatever the terminal under it did.
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

/// The picture a page's last frame left on the surface, kept to be shown again: the list it drew
/// and what the list is in. Written by the page's frames (`Core::page_tick`), read by stills.
#[derive(Default)]
pub(super) struct PageStills {
    drawn: RefCell<Option<Drawn>>,
}

/// A list as the frame drew it.
struct Drawn {
    list: UiList,
    /// The surface the list was built for, pixels: the list is in them.
    size: (u32, u32),
    /// What the pass cleared to under the list.
    clear: LinearRgba,
}

impl PageStills {
    /// A frame of the page reached the screen: `list`, built for a `size` surface and drawn over
    /// `clear`, is what is shown from now on. Frame path (main thread).
    pub(super) fn drew(&self, list: UiList, size: (u32, u32), clear: LinearRgba) {
        if size.0 == 0 || size.1 == 0 {
            return self.forget();
        }
        *self.drawn.borrow_mut() = Some(Drawn { list, size, clear });
    }

    /// What is on screen is not a page's frame any more, or no longer to be trusted as one.
    pub(super) fn forget(&self) {
        self.drawn.replace(None);
    }

    /// Whether a page has drawn and a still of the pane is its picture.
    pub(super) fn held(&self) -> bool {
        self.drawn.try_borrow().is_ok_and(|drawn| drawn.is_some())
    }

    /// The kept picture as a `tile`-sized one, through `chain` ([`Renderer::snapshot_ui`]):
    /// reads the list and draws it, changes nothing of it. `None` when the GPU said no.
    pub(super) fn snapshot(
        &self,
        renderer: &Renderer,
        chain: &mut Option<Chain>,
        tile: (u32, u32),
    ) -> Option<Snapshot> {
        let held = self.drawn.try_borrow().ok()?;
        let drawn = held.as_ref()?;
        let rgba = renderer
            .snapshot_ui(chain, drawn.clear, &drawn.list, drawn.size, tile)
            .ok()?;
        Some(Snapshot {
            width: tile.0,
            height: tile.1,
            rgba,
        })
    }
}

impl Core {
    fn snapshot(&self, shot: &mut Shot, tile: (u32, u32)) -> Option<Snapshot> {
        if tile.0 == 0 || tile.1 == 0 {
            return None;
        }
        // A pane that shows a page that has drawn is that page, not the terminal under it. A GPU
        // that declines gives none rather than the grid the page covers.
        if self.page_still.held() {
            return self
                .page_still
                .snapshot(&self.renderer, &mut shot.chain, tile);
        }
        let size = self.surface.size()?;
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

/// What draws a picture of a session no pane shows (a kablo terminal that runs in the
/// background, [`bt_core::pictured`]): a renderer of its own, the frame buffers a still keeps
/// ([`Shot`]) and the reduction chain, with no window, no layer and no surface.
///
/// **Not a frame either**, for the module header's reasons and then some: the stage has no
/// link, so there is nothing of a window's it could count, wake or step. The composition is
/// [`compose_still`] — the pane's still, line for line — over a [`View`] of a pane that is not
/// focused, has no dock and no top edge: the grid, its fill band and the caret as a hollow
/// box, the way a card shows a pane of a window that is not key.
///
/// Main thread, like the link. Dropping it gives back the renderer's textures and the chain.
pub struct Stage {
    renderer: Renderer,
    shot: Shot,
}

impl Stage {
    /// A stage that sets its text in `font`.
    ///
    /// # Errors
    ///
    /// The GPU could not be opened.
    pub fn new(font: &bt_core::FontOptions) -> Result<Self, GpuError> {
        let renderer = Renderer::system_default()?;
        renderer.set_font(font);
        Ok(Self {
            renderer,
            shot: Shot::default(),
        })
    }

    /// Changes the font the stage sets its text in; `true` when it was another.
    pub fn set_font(&self, font: &bt_core::FontOptions) -> bool {
        self.renderer.set_font(font)
    }

    /// The cell the stage's font makes at `scale` (the pixels per point of the display the
    /// picture is for): what a session shown here is told its cell is, and what the pixel size
    /// of its grid is made of.
    pub fn cell_metrics(&self, scale: f64) -> CellMetrics {
        self.renderer.cell_metrics(scale)
    }

    /// `session`, a `cols`-column screen, drawn at `size` pixels and brought to `tile`; `None`
    /// when either is empty or the GPU said no (the caller keeps the picture it had).
    pub fn snapshot(
        &mut self,
        session: &Session,
        cols: u16,
        scale: f64,
        size: (u32, u32),
        tile: (u32, u32),
    ) -> Option<Snapshot> {
        if size.0 == 0 || size.1 == 0 || tile.0 == 0 || tile.1 == 0 {
            return None;
        }
        let view = View {
            cell: self.renderer.cell_metrics(scale),
            caret_style: CaretStyle::default(),
            edge: ContentEdge::default(),
            dock_rows: 0,
            dock_cols: cols,
            context: ContextStrip::default(),
            focused: false,
            caret_focused: false,
        };
        let clear = compose_still(&mut self.shot, session, &view, size);
        let rgba = self
            .renderer
            .snapshot(&mut self.shot.chain, clear, &self.shot.frame, size, tile)
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

    use bt_atlas::{Family, TextStyle};
    use bt_core::{CaretShape, CursorBlink, Osc52, SessionOptions, TerminalOptions, Wake};

    use super::*;
    use crate::renderer::tests::{WHITE, grid, pixel_at};
    use crate::renderer::ui::{UiBitmap, UiImage, UiQuad, UiRect, tint};

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

    /// **A kablo terminal's picture is drawn with no pane, no window and no connection**: the
    /// bytes a server builds a screen from, written to a session nobody types into, drawn by
    /// the stage into a tile of the size the card wants. The screen's colours are in the pixels,
    /// at the cells the bytes put them.
    #[test]
    fn a_picture_of_a_background_terminal_is_drawn_by_the_stage_with_no_pane() {
        let mut stage = Stage::new(&bt_core::FontOptions::default()).expect("a stage");
        let metrics = stage.cell_metrics(1.0);
        let (cell_w, cell_h) = metrics.cell_px();
        let inset = metrics.grid_inset();
        let (cols, rows) = (16u16, 8u16);
        // What the server says of a screen with a red word at its top left: the cursor moved
        // home and three cells painted, then the rest of the screen as it was.
        let bytes = b"\x1b[2J\x1b[H\x1b[41m   \x1b[0m\x1b[8;1H";
        let session = bt_core::pictured(
            SessionOptions {
                command: None,
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols,
                rows,
                cell_px: (cell_w, cell_h),
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
            bytes,
        )
        .expect("a pictured session");
        let size = (
            u32::from(inset.left) + u32::from(cols) * u32::from(cell_w) + u32::from(inset.right),
            u32::from(rows) * u32::from(cell_h) + u32::from(inset.foot),
        );
        let tile = (size.0 / 2, size.1 / 2);
        let still = stage
            .snapshot(&session, cols, 1.0, size, tile)
            .expect("a picture");
        assert_eq!((still.width, still.height), tile);
        let at = |x: u32, y: u32| {
            let i = ((y * tile.0 + x) * 4) as usize;
            (still.rgba[i], still.rgba[i + 1], still.rgba[i + 2])
        };
        // The middle of the second painted cell, halved to the tile.
        let red = at(
            (u32::from(inset.left) + u32::from(cell_w) * 3 / 2) / 2,
            u32::from(cell_h) / 4,
        );
        assert!(
            red.0 > red.1 + 60 && red.0 > red.2 + 60,
            "the painted cells are not red in the picture: {red:?}"
        );
        // Three columns to the right the screen is as the theme has it.
        let plain = at(
            (u32::from(inset.left) + u32::from(cell_w) * 8) / 2,
            u32::from(cell_h) / 4,
        );
        assert!(
            plain.0 <= plain.1 + 60,
            "the cells the bytes left alone are painted: {plain:?}"
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

    /// A page's list: a box, a two-by-two image drawn texel for pixel, and a line of text where
    /// the platform has proportional text. The surface is 128 pixels square.
    fn page_list(renderer: &Renderer, image: &UiBitmap<'_>) -> UiList {
        let mut list = UiList::new();
        list.quad(UiQuad::new(UiRect::new(40.0, 40.0, 30.0, 20.0)).fill(tint(WHITE, 1.0)));
        assert!(renderer.ui_image(
            &mut list,
            image,
            UiImage::new(UiRect::new(10.0, 12.0, 2.0, 2.0))
        ));
        if Renderer::ui_text_supported() {
            let font = renderer
                .ui_font(&TextStyle {
                    family: Family::System,
                    size_px: 24.0,
                    weight: 400,
                    italic: false,
                })
                .expect("the system face opens");
            let line = renderer.ui_shape(font, "HHHH").expect("shapes");
            renderer.ui_text(
                &mut list,
                &line,
                [10.0, 80.0],
                tint(WHITE, 1.0),
                UiRect::UNBOUNDED,
            );
        }
        list
    }

    /// Premultiplied sRGB texels, mid-tones: a missing decode moves each of them.
    const RED: [u8; 4] = [0xc8, 0x40, 0x40, 0xff];
    const GREEN: [u8; 4] = [0x40, 0xa0, 0x50, 0xff];
    const BLUE: [u8; 4] = [0x40, 0x60, 0xd0, 0xff];
    const CREAM: [u8; 4] = [0xe0, 0xd0, 0x90, 0xff];

    /// **A pane that shows a page is the page's last drawn picture**, boxes, images and text
    /// among it: the still is what the frame drew, pixel for pixel, and it is made without the
    /// page — the list is all it has.
    #[test]
    fn a_page_that_has_drawn_is_the_picture_a_still_makes_with_its_images() {
        let renderer = Renderer::new();
        let pixels: Vec<u8> = [RED, GREEN, BLUE, CREAM].concat();
        let image = UiBitmap {
            id: 1,
            width: 2,
            height: 2,
            pixels: &pixels,
        };
        let list = page_list(&renderer, &image);
        // The frame: drawn once, the texture is the live one.
        let clear = Theme::BATERI.background_linear();
        let drawn = renderer.render_ui_offscreen(128, clear, &list);
        assert_eq!(renderer.ui_images_held(), 1);
        let counts = (
            renderer.frames(),
            renderer.last_bg_count(),
            renderer.last_glyph_count(),
        );

        let stills = PageStills::default();
        assert!(!stills.held(), "a page that has not drawn has no picture");
        stills.drew(list, (128, 128), clear);
        assert!(stills.held());

        let mut chain = None;
        let still = stills
            .snapshot(&renderer, &mut chain, (128, 128))
            .expect("the page's still");
        assert_eq!((still.width, still.height), (128, 128));
        for (index, rgba) in still.rgba.chunks_exact(4).enumerate() {
            let at = index * 4;
            // Bgra in the frame, rgba in the still.
            let want = [drawn[at + 2], drawn[at + 1], drawn[at], drawn[at + 3]];
            for (got, want) in rgba.iter().zip(want) {
                assert!(
                    got.abs_diff(want) <= 1,
                    "pixel {index} differs: {rgba:?} against {want:?}"
                );
            }
        }
        let rgba = |x: usize, y: usize| {
            let at = (y * 128 + x) * 4;
            (still.rgba[at], still.rgba[at + 1], still.rgba[at + 2])
        };
        // The image's four texels are in their own colours, each on its pixel, and the box's
        // white is where the box is.
        for (texel, (x, y)) in [
            (RED, (10, 12)),
            (GREEN, (11, 12)),
            (BLUE, (10, 13)),
            (CREAM, (11, 13)),
        ] {
            let got = rgba(x, y);
            assert!(
                got.0.abs_diff(texel[0]) <= 1
                    && got.1.abs_diff(texel[1]) <= 1
                    && got.2.abs_diff(texel[2]) <= 1,
                "the image's pixel ({x}, {y}) is {got:?}, not {texel:?}"
            );
        }
        assert_eq!(rgba(50, 50), (255, 255, 255));
        if Renderer::ui_text_supported() {
            let ink = (10..90)
                .flat_map(|x| (80..110).map(move |y| (x, y)))
                .filter(|&(x, y)| rgba(x, y).0 > 200)
                .count();
            assert!(
                ink > 20,
                "the line of text is not in the still: {ink} white pixels"
            );
        }

        // **It reads, it does not move**: the texture the frame made is still the frame's, no
        // frame was counted and none is left in flight.
        assert_eq!(renderer.ui_images_held(), 1);
        assert_eq!(
            counts,
            (
                renderer.frames(),
                renderer.last_bg_count(),
                renderer.last_glyph_count()
            ),
            "a page's still moved the frame counters"
        );
        assert!(
            !renderer.poll(|_| ()),
            "a page's still left a frame in flight"
        );
        // And a tile of another size is the same picture brought to it.
        let small = stills
            .snapshot(&renderer, &mut chain, (32, 32))
            .expect("a smaller still");
        assert_eq!(small.rgba.len(), 32 * 32 * 4);
    }

    /// **A still does not run the frame path's rule for textures**: the frame lets go of the
    /// textures its list does not name, a still of a list that names fewer leaves them where
    /// they are for the page's next frame.
    #[test]
    fn a_still_of_a_page_leaves_the_live_textures_as_the_last_frame_held_them() {
        let renderer = Renderer::new();
        let one: Vec<u8> = RED.to_vec();
        let other: Vec<u8> = GREEN.to_vec();
        let bitmaps = [
            UiBitmap {
                id: 11,
                width: 1,
                height: 1,
                pixels: &one,
            },
            UiBitmap {
                id: 12,
                width: 1,
                height: 1,
                pixels: &other,
            },
        ];
        let rect = UiRect::new(8.0, 8.0, 4.0, 4.0);
        let mut both = UiList::new();
        for bitmap in &bitmaps {
            assert!(renderer.ui_image(&mut both, bitmap, UiImage::new(rect)));
        }
        let clear = Theme::BATERI.background_linear();
        renderer.render_ui_offscreen(32, clear, &both);
        assert_eq!(renderer.ui_images_held(), 2);

        // The list a still is made of names one of them.
        let mut names_one = UiList::new();
        assert!(renderer.ui_image(&mut names_one, &bitmaps[0], UiImage::new(rect)));
        let stills = PageStills::default();
        stills.drew(names_one, (32, 32), clear);
        let mut chain = None;
        let still = stills
            .snapshot(&renderer, &mut chain, (32, 32))
            .expect("the still");
        let red = &still.rgba[(10 * 32 + 10) * 4..][..3];
        assert!(
            red[0] > 150 && red[1] < 100,
            "the named image is not in the still: {red:?}"
        );
        assert_eq!(
            renderer.ui_images_held(),
            2,
            "a still let go of a texture the last frame held"
        );
    }

    /// A page's picture is the page's while it is up and drawn, the terminal's otherwise.
    #[test]
    fn a_pages_picture_is_kept_from_its_frame_until_it_is_forgotten() {
        let renderer = Renderer::new();
        let clear = Theme::BATERI.background_linear();
        let stills = PageStills::default();
        let mut chain = None;
        assert!(stills.snapshot(&renderer, &mut chain, (16, 16)).is_none());

        stills.drew(UiList::new(), (64, 64), clear);
        assert!(stills.held());
        // An empty list is a page that drew nothing: the ground, not the terminal's picture.
        let ground = stills
            .snapshot(&renderer, &mut chain, (16, 16))
            .expect("an empty page has a picture");
        assert_eq!(ground.rgba.len(), 16 * 16 * 4);

        // A surface of no size has no frame to remember.
        stills.drew(UiList::new(), (0, 64), clear);
        assert!(!stills.held());
        stills.drew(UiList::new(), (64, 64), clear);
        stills.forget();
        assert!(!stills.held());
        assert!(stills.snapshot(&renderer, &mut chain, (16, 16)).is_none());
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
            "self.page_frames",
            "self.page_contrast",
            "self.page.",
            ".build(",
            ".draw_ui(",
            "ui_release_images",
            "ui_flushed",
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
