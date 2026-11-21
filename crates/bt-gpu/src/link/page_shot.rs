//! The picture of a page as it is now, for the one who asks for it (`bateri view … shot`): built
//! from the page, drawn into a texture of its own, brought to a tile and given back.
//!
//! A pane that shows a page has two pictures. The one on screen is the list the page's last
//! frame drew ([`super::still::PageStills`]) and a still of the pane is that. A pane that is not
//! seen — on another tab, in a window that is hidden — draws no frames, so what it last drew is
//! how it was when it was last seen; a picture of it as it is now is the page laid out once, here.
//!
//! **It is a still, not a frame**, by the same rules ([`super::still`]'s header): the list is
//! built and drawn into the picture's texture and never presented; the link's frame requests, its
//! pacer, its damage, its statistics, the theme it last drew and the page's clock are not read or
//! written. What building a list moves is the page's own — it is laid out as for a frame, which
//! is also what a reading of a page that is not seen does (`PageHandle::lay_out_unseen`) — and
//! the renderer's text atlas, which a list names glyphs of.
//!
//! The one thing it gives the link: when the text atlas filled and was cleared while the list was
//! built, the list the link kept from the last frame names glyphs that are no longer where it says,
//! so that list is replaced by this one, which is right (and kept only if there was one).

use bt_core::LinearRgba;

use super::{Core, DisplayLink, Shot, Snapshot};
use crate::Renderer;
use crate::renderer::shrink::Chain;
use crate::renderer::ui::{UiContext, UiList, UiPage};

/// The list `page` builds in `cx`, as a `tile`-pixel picture over `clear`, and the list itself
/// when the text atlas was cleared on the way (`flushed`): the one built after the clear is the
/// one that is right.
pub(super) fn picture_of(
    renderer: &Renderer,
    chain: &mut Option<Chain>,
    page: &mut dyn UiPage,
    cx: &UiContext<'_>,
    clear: LinearRgba,
    tile: (u32, u32),
) -> Option<(Snapshot, Option<UiList>)> {
    let mut list = page.build(cx);
    // The text atlas filled and was cleared while the list was built: glyphs placed before the
    // clear point at texels later ones took, so it is built again, as a frame does.
    let flushed = renderer.ui_flushed();
    if flushed {
        list = page.build(cx);
        let _ = renderer.ui_flushed();
    }
    let rgba = renderer
        .snapshot_ui(chain, clear, &list, cx.size_px, tile)
        .ok()?;
    let still = Snapshot {
        width: tile.0,
        height: tile.1,
        rgba,
    };
    Some((still, flushed.then_some(list)))
}

impl Core {
    /// The shown page as it is now at `now` (the pacer's clock), as a `tile`-sized picture: `None`
    /// when no page is shown, the surface has no size, the tile is empty, the page is being built
    /// (a call from inside its own build) or the GPU said no.
    fn snapshot_page_now(&self, shot: &mut Shot, tile: (u32, u32), now: f64) -> Option<Snapshot> {
        if tile.0 == 0 || tile.1 == 0 {
            return None;
        }
        let size_px = self.surface.size().filter(|(w, h)| *w > 0 && *h > 0)?;
        let theme = self.session.theme();
        let cx = UiContext {
            renderer: &self.renderer,
            size_px,
            scale: self.cell.get().scale(),
            theme: &theme,
            now,
            reduce_motion: self.motion.get().reduce(),
            minimum_contrast: self.session.minimum_contrast(),
        };
        let clear = theme.background_linear();
        let (still, replaced) = {
            let mut slot = self.page.try_borrow_mut().ok()?;
            let page = slot.as_mut()?;
            picture_of(
                &self.renderer,
                shot.chain_mut(),
                page.as_mut(),
                &cx,
                clear,
                tile,
            )?
        };
        if let Some(list) = replaced
            && self.page_still.held()
        {
            self.page_still.drew(list, size_px, clear);
        }
        Some(still)
    }

    /// A pane that shows a page: what a still of it is, for a request that wants the page as it is
    /// now (`fresh`), or as it last was on screen if it has drawn.
    fn snapshot_page(
        &self,
        shot: &mut Shot,
        tile: (u32, u32),
        fresh: bool,
        now: f64,
    ) -> Option<Snapshot> {
        if !fresh && self.page_still.held() {
            return self.snapshot(shot, tile);
        }
        self.snapshot_page_now(shot, tile, now)
    }
}

impl DisplayLink {
    /// The page the pane shows, as a picture `tile` pixels across: the page as it was when its
    /// last frame reached the screen, or — when `fresh`, or when it has not drawn — the page laid
    /// out now, at `now` on the frames' clock. The terminal under the page is never the picture.
    ///
    /// Not a frame (the module header says what that means), so it asks nothing, counts nothing and
    /// presents nothing. Main thread, like the link. `None` when no page is shown, the surface or
    /// the tile has no size, or the GPU said no.
    pub fn snapshot_page(
        &self,
        shot: &mut Shot,
        tile: (u32, u32),
        fresh: bool,
        now: f64,
    ) -> Option<Snapshot> {
        self.core.snapshot_page(shot, tile, fresh, now)
    }
}

#[cfg(test)]
mod tests {
    use bt_core::Theme;

    use super::*;
    use crate::renderer::tests::WHITE;
    use crate::renderer::ui::{UiQuad, UiRect, tint};

    /// A page that draws a white box where it says: what the page is now is where its box is.
    struct Boxed {
        at: f32,
        built: usize,
    }

    impl UiPage for Boxed {
        fn build(&mut self, _: &UiContext<'_>) -> UiList {
            self.built += 1;
            let mut list = UiList::new();
            list.quad(UiQuad::new(UiRect::new(self.at, 40.0, 30.0, 20.0)).fill(tint(WHITE, 1.0)));
            list
        }
    }

    fn context<'a>(renderer: &'a Renderer, theme: &'a Theme) -> UiContext<'a> {
        UiContext {
            renderer,
            size_px: (128, 128),
            scale: 1.0,
            theme,
            now: 1.0,
            reduce_motion: false,
            minimum_contrast: 1.0,
        }
    }

    fn white_at(still: &Snapshot, x: usize, y: usize) -> bool {
        let at = (y * still.width as usize + x) * 4;
        still.rgba[at..at + 3] == [255, 255, 255]
    }

    /// A page that moved while nobody looked is the picture of where it is now, not of where its
    /// last frame left it, and the picture is made with no frame: nothing counted, nothing in
    /// flight, nothing of a frame's presented.
    #[test]
    fn a_page_that_is_not_seen_is_pictured_as_it_is_now_and_is_no_frame() {
        let renderer = Renderer::new();
        let theme = Theme::BATERI;
        let clear = theme.background_linear();
        let mut page = Boxed { at: 80.0, built: 0 };
        let counts = (
            renderer.frames(),
            renderer.last_bg_count(),
            renderer.last_glyph_count(),
        );
        let mut chain = None;
        let (still, replaced) = picture_of(
            &renderer,
            &mut chain,
            &mut page,
            &context(&renderer, &theme),
            clear,
            (128, 128),
        )
        .expect("a picture");
        assert_eq!(page.built, 1);
        assert!(replaced.is_none(), "no atlas was cleared");
        assert_eq!((still.width, still.height), (128, 128));
        assert!(
            white_at(&still, 90, 50),
            "the box is where the page says now"
        );
        assert!(!white_at(&still, 50, 50));
        // The same page, a smaller tile of the same shape: the same picture, brought down.
        let small = picture_of(
            &renderer,
            &mut chain,
            &mut page,
            &context(&renderer, &theme),
            clear,
            (64, 64),
        )
        .expect("a smaller picture")
        .0;
        assert_eq!(small.rgba.len(), 64 * 64 * 4);
        assert!(white_at(&small, 45, 25));
        assert_eq!(
            counts,
            (
                renderer.frames(),
                renderer.last_bg_count(),
                renderer.last_glyph_count()
            ),
            "a page's picture moved the frame counters"
        );
        assert!(
            !renderer.poll(|_| ()),
            "a page's picture left a frame in flight"
        );
    }

    /// The picture is made through the page's list and the renderer's, and through nothing of the
    /// link's own: a source scan, as the still's (`the_still_path_never_names_the_links_state`),
    /// with the one name it may use that still may not (the page, to build it) left out.
    #[test]
    fn a_pages_picture_never_names_the_links_frame_state() {
        let source = include_str!("page_shot.rs");
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
            "self.motion.set",
            "self.blink",
            "self.glyph_fx",
            "self.scrollbar",
            "self.origin",
            "self.frame",
            "self.theme.set",
            "self.page_frames",
            "self.page_contrast",
            "self.marks",
            "self.blocks",
            "content_frames",
            "motion_frames",
            "slide_frames",
            "ui_release_images",
            "take_damage",
            "request_frame",
            "poke_page",
            "rearm_page",
            ".draw_ui(",
            ".present(",
            ".poll(",
            ".acquire(",
            "wake_at(",
        ] {
            assert!(
                !code.contains(state),
                "a page's picture names `{state}`: it may read the link, not move it"
            );
        }
        // The page is borrowed for the build alone, and not when it is already being built.
        assert!(code.contains("self.page.try_borrow_mut()"));
    }
}
