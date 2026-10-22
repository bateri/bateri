//! A program's own icon on the application's chrome: the strip over its view ([`super::strip`]),
//! its tab's tile, the tabs menu's row and the tab's card (`tab_bar`, which reaches here; this
//! module, the pane's, names none of the application's).
//!
//! What the program gave (`ViewStart`'s icons, which the protocol's host library checked and
//! bounded) is decoded once, when its view starts, for a light ground and for a dark one
//! ([`Faces::new`], `icon::pick`), and how each stands is read from its pixels
//! ([`bt_shell_common::view_icon::lay`]): a ground of its own, or a shape on the application's
//! face. The tab bar keeps what it drew in looks it compares to decide whether to draw again,
//! and those are `Copy`; so what goes there is a [`FaceId`], and the picture is looked up by it
//! when it is drawn ([`face`]). A face lives while its view does: when the view's last hold
//! goes, its faces are forgotten, and a look that still names one draws the program's kind as
//! before.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use bt_core::pencere::icon::{self, Icon};
use bt_shell_common::view_icon::{self, Lay};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_app_kit::{NSBitmapImageRep, NSImage};
use objc2_foundation::NSData;

/// The pixels a side of the picture asked for: a tab's tile is 22 points wide, on a Retina
/// screen. The strip's icon, smaller, takes the same picture scaled down.
const PIXELS: u32 = 44;

/// A program's icon, decoded, and how it stands.
pub(crate) struct Face {
    pub(crate) image: Retained<NSImage>,
    pub(crate) lay: Lay,
}

/// What names a [`Face`] in a look: `Copy` and compared by identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FaceId(u64);

impl FaceId {
    /// The number, for a platform-free record that carries it (`tabs::CardPane::face`).
    pub(crate) fn raw(self) -> u64 {
        self.0
    }

    /// The id a record's number names.
    pub(crate) fn of(raw: u64) -> Self {
        Self(raw)
    }
}

thread_local! {
    static FACES: RefCell<HashMap<FaceId, Rc<Face>>> = RefCell::new(HashMap::new());
    static NEXT: Cell<u64> = const { Cell::new(0) };
}

/// The face `id` names, while its view lives.
pub(crate) fn face(id: FaceId) -> Option<Rc<Face>> {
    FACES.with(|faces| faces.borrow().get(&id).cloned())
}

/// The faces of one view: its program's icon for a light ground and for a dark one (one face
/// when the same picture suits both), kept while this lives.
pub(crate) struct Faces {
    light: Option<FaceId>,
    dark: Option<FaceId>,
}

impl Faces {
    /// The faces of `icons`. A picture the system cannot decode is no face.
    pub(crate) fn new(icons: &[Icon]) -> Self {
        let light = icon::pick(icons, false, PIXELS);
        let dark = icon::pick(icons, true, PIXELS);
        let light_id = light.and_then(register);
        let dark_id = match (light, dark) {
            (Some(light), Some(dark)) if light == dark => light_id,
            (_, dark) => dark.and_then(register),
        };
        Self {
            light: light_id,
            dark: dark_id,
        }
    }

    /// The face for a ground that is dark or not.
    pub(crate) fn on(&self, dark: bool) -> Option<FaceId> {
        if dark { self.dark } else { self.light }
    }
}

impl Drop for Faces {
    fn drop(&mut self) {
        FACES.with(|faces| {
            let mut faces = faces.borrow_mut();
            for id in [self.light, self.dark].into_iter().flatten() {
                faces.remove(&id);
            }
        });
    }
}

/// Decodes `icon`, reads how it stands and keeps it under a new id.
fn register(icon: &Icon) -> Option<FaceId> {
    let data = NSData::with_bytes(&icon.bytes);
    // The pixels first: an image object is made of bytes it has not decoded yet, the bitmap is
    // not, so a picture whose pixels cannot be had is no face.
    let pixels = NSBitmapImageRep::imageRepWithData(&data)?;
    let side = |n: isize| u32::try_from(n).ok().filter(|n| *n > 0);
    let (wide, high) = (side(pixels.pixelsWide())?, side(pixels.pixelsHigh())?);
    let lay = view_icon::lay(wide, high, |x, y| {
        let alpha = pixels
            .colorAtX_y(x as isize, y as isize)
            .map_or(0.0, |color| color.alphaComponent());
        (alpha.clamp(0.0, 1.0) * 255.0).round() as u8
    });
    let image = NSImage::initWithData(NSImage::alloc(), &data)?;
    if !image.isValid() {
        return None;
    }
    let id = NEXT.with(|next| {
        next.set(next.get() + 1);
        FaceId(next.get())
    });
    FACES.with(|faces| faces.borrow_mut().insert(id, Rc::new(Face { image, lay })));
    Some(id)
}

/// Pictures for tests: one that brings its ground and one that is a shape.
#[cfg(test)]
pub(crate) mod samples {
    use bt_core::pencere::icon::{Icon, IconFormat, IconTheme};
    use objc2::AnyThread;
    use objc2_foundation::{NSData, NSDataBase64DecodingOptions, NSString};

    use super::Faces;

    /// A 16-pixel square of one opaque colour, `#D97757`: a picture that brings its ground.
    pub(crate) const GROUND: &str = "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAYAAAAf8/9hAAAAGUlEQVR4nGO4WR7+nxLMMGrAqAGjBgwXAwBbv6YfPBdm3gAAAABJRU5ErkJggg==";
    /// Three stacked `#4C7CC4` boxes on nothing, 64 pixels: a shape (the example tool's icon).
    pub(crate) const MARK: &str = "iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAYAAACqaXHeAAAAy0lEQVR42u3YsQ3CMBBG4QzowhJTsIMrClgD0dEyAYUHSEXrUYwcKR1VSOP4e9JJVqr4j+zcvWkCAAD4QUg5hpRLSLl2Wu3d4z+brwepuCWAcqAAypYA6pFqtwBOt3c9P19LtfVQAbQNX+ZHvX7uS7V1LyHsEkD76uvm12rPBOAIuATHCWCk3+DwjdDYrfDwwxDAB/ABfAAfoBU2DBmHBeAIuAT5AD6AD+ADwAfwAXwAH8AH8AF8AB/AB/ABfAAfwAfwAXwAAAAAOucLnVucWIVm/igAAAAASUVORK5CYII=";

    pub(crate) fn icon(base64: &str, side: u32, theme: Option<IconTheme>) -> Icon {
        Icon {
            format: IconFormat::Png,
            width: side,
            height: side,
            theme,
            bytes: NSData::initWithBase64EncodedString_options(
                NSData::alloc(),
                &NSString::from_str(base64),
                NSDataBase64DecodingOptions::empty(),
            )
            .expect("base64")
            .to_vec(),
        }
    }

    /// The faces of [`GROUND`], for either ground.
    pub(crate) fn ground() -> Faces {
        Faces::new(&[icon(GROUND, 16, None)])
    }

    /// The faces of [`MARK`], for either ground.
    pub(crate) fn mark() -> Faces {
        Faces::new(&[icon(MARK, 64, None)])
    }
}

#[cfg(test)]
mod tests {
    use bt_core::pencere::icon::IconTheme;

    use super::samples::{GROUND, MARK, icon};
    use super::*;

    #[test]
    fn how_a_face_stands_is_read_from_its_pixels() {
        let faces = samples::ground();
        let ground = face(faces.on(false).expect("a face")).expect("kept");
        assert_eq!(ground.lay, Lay::Ground);
        // One picture for both grounds is one face.
        assert_eq!(faces.on(false), faces.on(true));
        let faces = samples::mark();
        assert_eq!(
            face(faces.on(true).expect("a face")).expect("kept").lay,
            Lay::Mark
        );
    }

    #[test]
    fn each_ground_has_its_own_picture_and_they_go_with_their_view() {
        let faces = Faces::new(&[
            icon(GROUND, 16, Some(IconTheme::Light)),
            icon(MARK, 64, Some(IconTheme::Dark)),
        ]);
        let (light, dark) = (
            faces.on(false).expect("light"),
            faces.on(true).expect("dark"),
        );
        assert_ne!(light, dark);
        assert_eq!(face(light).expect("kept").lay, Lay::Ground);
        assert_eq!(face(dark).expect("kept").lay, Lay::Mark);
        drop(faces);
        assert!(face(light).is_none() && face(dark).is_none());
    }

    #[test]
    fn bytes_the_system_cannot_decode_are_no_face() {
        let mut broken = icon(GROUND, 16, None);
        broken.bytes.truncate(20);
        let faces = Faces::new(&[broken]);
        assert_eq!((faces.on(false), faces.on(true)), (None, None));
        assert_eq!(Faces::new(&[]).on(true), None);
    }
}
