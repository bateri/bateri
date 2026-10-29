//! The pictures of terminals no pane shows: a kablo terminal that runs in the background on a
//! server, its screen as the server built it ([`hold_service::picture`]), drawn at the size of a
//! card by the same composition a pane's still goes through ([`bt_gpu::Stage`]).
//!
//! **Pull, like the pane's still** (`.claude/rules/render.md`): a picture is asked for when the
//! view that shows it wants it again (its refresh interval), never on a clock of its own, and
//! drawing one is not a frame — the stage has no window to count it in. What is kept is the
//! session the bytes built (a few kilobytes of grid) and the last image drawn from it;
//! [`Screens::release`] gives them back when the view closes.
//!
//! **The server is read, not attached to.** The question is a `read` (kablo's
//! `docs/PROTOCOL.md`: a picture that answers a read has no `attached` before it and nothing
//! streams after), so the terminal's other clients, its size and its journal are as they were.
//! The screen carries no kitty images and its `1016` bit means nothing here (`bt_core::pictured`).

use std::collections::HashMap;

use bt_core::{Session, SessionOptions, Settings, Theme};
use bt_gpu::Stage;
use bt_shell_common::grid::size_of_grid;
use bt_shell_common::kablo::service::Screen;
use objc2_core_foundation::CFRetained;
use objc2_core_graphics::CGImage;

use crate::snapshot;

/// A terminal on a server: its account ([`bt_shell_common::ssh_wrap::host_key`]) and its name
/// there.
pub(crate) type Key = bt_shell_common::overview::Key;

/// What is kept of one terminal.
struct Kept {
    /// The server's mark of the screen: the same offset and size are the same screen.
    offset: u64,
    cols: u16,
    rows: u16,
    /// The cell the session was made for; another font makes another session.
    cell: (u16, u16),
    session: Session,
    /// The last image drawn from the session, and the tile (pixels) it is of.
    image: Option<((u32, u32), CFRetained<CGImage>)>,
}

/// The pictures one view keeps.
#[derive(Default)]
pub(crate) struct Screens {
    stage: Option<Stage>,
    kept: HashMap<Key, Kept>,
}

/// The stage, opened on first use with the settings' font; `None` when the GPU said no.
fn opened<'a>(stage: &'a mut Option<Stage>, settings: &Settings) -> Option<&'a mut Stage> {
    if stage.is_none() {
        *stage = Stage::new(&settings.font).ok();
    }
    let stage = stage.as_mut()?;
    stage.set_font(&settings.font);
    Some(stage)
}

impl Screens {
    /// The server's `screen` of `key` came. `true` when it is not the screen already kept (the
    /// picture is to be drawn again); the same offset in the same size is the same screen, and
    /// nothing is built.
    pub(crate) fn arrived(
        &mut self,
        key: Key,
        screen: &Screen,
        settings: &Settings,
        theme: &Theme,
        scale: f64,
    ) -> bool {
        let Some(stage) = opened(&mut self.stage, settings) else {
            return false;
        };
        let cell = stage.cell_metrics(scale).cell_px();
        if let Some(kept) = self.kept.get(&key)
            && (kept.offset, kept.cols, kept.rows, kept.cell)
                == (screen.offset, screen.cols, screen.rows, cell)
        {
            return false;
        }
        if screen.cols == 0 || screen.rows == 0 {
            return false;
        }
        let options = SessionOptions {
            command: None,
            working_directory: None,
            home: None,
            env: HashMap::new(),
            cols: screen.cols,
            rows: screen.rows,
            cell_px: cell,
            terminal: settings.terminal(),
            theme: *theme,
            dock: false,
            cluster: true,
            initial_input: None,
            shell_marks: false,
            pane_uuid: None,
            hostname: None,
            replay: None,
            views: None,
        };
        let Ok(session) = bt_core::pictured(options, &screen.bytes) else {
            return false;
        };
        self.kept.insert(
            key,
            Kept {
                offset: screen.offset,
                cols: screen.cols,
                rows: screen.rows,
                cell,
                session,
                image: None,
            },
        );
        true
    }

    /// The picture of `key`'s screen at `tile` pixels (the whole screen, brought down on the
    /// GPU), the one already drawn when nothing changed. `None` when no screen came yet or the
    /// GPU said no.
    pub(crate) fn image(
        &mut self,
        key: &Key,
        settings: &Settings,
        scale: f64,
        tile: (u32, u32),
    ) -> Option<CFRetained<CGImage>> {
        if let Some((drawn, image)) = self.kept.get(key)?.image.as_ref()
            && *drawn == tile
        {
            return Some(image.clone());
        }
        let Self { stage, kept } = self;
        let stage = opened(stage, settings)?;
        let kept = kept.get_mut(key)?;
        let size = size_of_grid(kept.cols, kept.rows, stage.cell_metrics(scale));
        let still = stage.snapshot(&kept.session, kept.cols, scale, size, tile)?;
        let image = snapshot::image(&still)?;
        kept.image = Some((tile, image.clone()));
        Some(image)
    }

    /// Lets go of everything kept: the sessions, the images and the stage's textures.
    pub(crate) fn release(&mut self) {
        self.kept.clear();
        self.stage = None;
    }

    /// The terminals kept pictures of the ones in `wanted` only: the rest went from the view.
    pub(crate) fn retain(&mut self, wanted: impl Fn(&Key) -> bool) {
        self.kept.retain(|key, _| wanted(key));
    }
}

#[cfg(test)]
mod tests {
    use bt_core::Theme;

    use super::*;

    fn key() -> Key {
        ("u@rasp:22".to_owned(), "claude".to_owned())
    }

    fn screen(offset: u64, bytes: &[u8]) -> Screen {
        Screen {
            offset,
            cols: 40,
            rows: 10,
            bytes: bytes.to_vec(),
        }
    }

    #[test]
    fn a_screen_from_the_server_is_a_picture_of_the_tile_size_and_the_same_screen_is_not_built_twice()
     {
        let settings = Settings::default();
        let theme = Theme::BATERI;
        let mut screens = Screens::default();
        let tile = (200, 100);
        assert!(
            screens.image(&key(), &settings, 2.0, tile).is_none(),
            "no screen came yet"
        );
        let red = screen(7, b"\x1b[41m   \x1b[0m");
        assert!(screens.arrived(key(), &red, &settings, &theme, 2.0));
        let image = screens
            .image(&key(), &settings, 2.0, tile)
            .expect("a picture");
        assert_eq!(
            (CGImage::width(Some(&image)), CGImage::height(Some(&image))),
            (tile.0 as usize, tile.1 as usize)
        );
        // The same offset in the same size is the same screen: not built again, and the
        // picture drawn is the one asked for again.
        assert!(!screens.arrived(key(), &red, &settings, &theme, 2.0));
        let again = screens
            .image(&key(), &settings, 2.0, tile)
            .expect("a picture");
        assert!(std::ptr::eq(&*image, &*again), "drawn once");
        // Another offset is another screen.
        assert!(screens.arrived(key(), &screen(9, b"\x1b[44m   "), &settings, &theme, 2.0));
        screens.release();
        assert!(screens.image(&key(), &settings, 2.0, tile).is_none());
    }
}
