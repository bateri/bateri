//! Slot resolution and fan-out: `GlyphCell`/`RuleCell` lists → atlas slots →
//! `GlyphInstance` lists.
//!
//! The renderer reads this module's output and keeps no copy of its rules.
//! The one step that touches the GPU — where a freshly allocated slot's bytes
//! go — is the single method of [`SlotUpload`] (`Queue::write_texture` in the
//! renderer). Everything else — which slot a glyph takes, whether a wide glyph
//! becomes one quad or two, which plane's list it lands in, the baked uv — is
//! decided here, once.
//!
//! The colour plane's monotonic counter keeps its meaning (the colour plane
//! has its own counter): uvs are baked at list-building time from the atlas's
//! own slot origins.

use bt_atlas::{Atlas, Face, Half, Metrics, Placed, Plane, RuleKind, SizeClass, Sprite};
use bt_core::Clusters;

use crate::frame::{FxCell, FxInstance, GlyphCell, GlyphInstance, RuleCell};

/// Where a freshly allocated slot's bytes are written: the backend's two
/// plane textures.
///
/// Called **at allocation time**, inside [`Atlas::slot`]'s answer, and that is
/// load-bearing for the colour plane: its texture is lazy and is created by
/// the implementation on the first colour upload. Created a frame earlier,
/// the question "which character is colour" would need a second cascade walk;
/// a frame later, this slot would enter the atlas's cache unwritten and the
/// emoji would stay invisible **for good**.
pub(crate) trait SlotUpload {
    /// Writes one full slot at `origin` into `plane`'s texture. `bytes` is
    /// exactly [`slot_layout`]'s length. An implementation that cannot get the
    /// colour texture skips the write (the emoji is not drawn; no panic).
    fn upload(&mut self, plane: Plane, origin: (u16, u16), metrics: Metrics, bytes: &[u8]);
}

/// A slot's byte length and row pitch for `plane`: mask `w*h` / `w`, colour
/// `4*w*h` / `4*w`. `metrics` is the atlas's **slot** metric
/// (`Atlas::slot_metrics`), not the grid's.
///
/// One copy: wgpu's `bytes_per_row` comes from here, and if it drifted from
/// the buffer the GPU would read past a short buffer — silently. The lengths come from
/// `bt-atlas` (`slot_bytes`/`slot_bytes_rgba`), the slot geometry's single
/// owner.
pub(crate) fn slot_layout(metrics: Metrics, plane: Plane) -> (usize, usize) {
    let w = usize::from(metrics.cell_px.0);
    match plane {
        Plane::Mask => (metrics.slot_bytes(), w),
        Plane::Color => (metrics.slot_bytes_rgba(), w * 4),
    }
}

/// Fills the frame's glyph lists: glyphs **and** rules into `mask` (one list,
/// one draw call: glyphs first, rules after — a strikeout must cross **over**
/// its letter), colour glyphs into `color`. Both lists are cleared first.
///
/// The caller owns the lists so that their capacity survives from frame to
/// frame: no allocation in the steady state.
#[allow(clippy::too_many_arguments)] // two out-lists + the upload sink; a struct would only rename them
pub(crate) fn glyph_lists(
    atlas: &mut Atlas,
    upload: &mut impl SlotUpload,
    glyphs: &[GlyphCell],
    clusters: &Clusters,
    rules: &[RuleCell],
    mask: &mut Vec<GlyphInstance>,
    color: &mut Vec<GlyphInstance>,
) {
    // The **slot** metric: uploads are slot-sized; the grid's cell is
    // read only for the wide glyph's second quad ([`fan`]).
    let metrics = atlas.slot_metrics();
    let (tw, th) = atlas.texture_px();
    mask.clear();
    color.clear();
    // `clear` keeps the capacity, so the steady state does not allocate;
    // `reserve` only flattens the frame where capacity is exceeded **for the
    // first time** (underlining a block of text makes glyph + rule jump) —
    // one growth instead of several copies mid-loop.
    mask.reserve(glyphs.len() + rules.len());
    // The texture size is loop-invariant: invert once and multiply, instead
    // of two f32 divisions per sprite.
    let inv = (1.0 / f32::from(tw), 1.0 / f32::from(th));
    for glyph in glyphs {
        // **The list is chosen by plane.** Emoji needs another pipeline,
        // another texture and another fragment; mixed into one list, one draw
        // call could not ask for two fragments.
        for part in fan(atlas, upload, metrics, inv, glyph, clusters)
            .into_iter()
            .flatten()
        {
            let list = match part.plane {
                Plane::Mask => &mut *mask,
                Plane::Color => &mut *color,
            };
            list.push(GlyphInstance {
                pos: part.pos,
                uv0: part.uv0,
                rgba: glyph.rgba,
            });
        }
    }
    for rule in rules {
        let (uv0, _) = slot_uv(atlas, upload, metrics, inv, rule_ask(rule.kind));
        mask.push(GlyphInstance {
            pos: rule.pos,
            uv0,
            rgba: rule.rgba,
        });
    }
}

/// Fills the typing effects' instance list: slot resolution and fan-out
/// through [`fan`], the **same** body as [`glyph_lists`] — no second copy.
///
/// A wide glyph yields two instances and each carries **which half** it is:
/// the shader takes the transform's centre from the two-cell box, otherwise
/// `recede` would split an emoji down the middle. The plane is in the
/// instance too: both textures are bound at once and the list is one. The
/// packing is the one `shaders/glyph_fx.*` decode:
/// `id | plane << 5 | half << 6`, a small integer that is exact in `f32`
/// ([`FxInstance`]).
/// What a rule sprite is asked for as: the one question both lists put, so
/// the dock's › arriving as an effect resolves to the very slot the static
/// one is drawn from.
fn rule_ask(kind: RuleKind) -> SlotAsk {
    SlotAsk {
        sprite: Sprite::Rule(kind),
        // Rules are **always** `Face::Regular`: the line under bold text is
        // not bold. `Atlas::slot` normalises this too; asking for the right
        // face here keeps that normalisation a second line of defence, not
        // the only one.
        face: Face::Regular,
        // Rules are always at display size: the context row has none, and
        // `Atlas::slot` normalises this as well.
        size: SizeClass::Normal,
        // A rule is one cell by definition; same normalisation.
        want: Half::Whole,
    }
}

pub(crate) fn fx_list(
    atlas: &mut Atlas,
    upload: &mut impl SlotUpload,
    cells: &[FxCell],
    clusters: &Clusters,
    out: &mut Vec<FxInstance>,
) {
    // The **slot** metric: uploads are slot-sized; the grid's cell is
    // read only for the wide glyph's second quad ([`fan`]).
    let metrics = atlas.slot_metrics();
    let (tw, th) = atlas.texture_px();
    let inv = (1.0 / f32::from(tw), 1.0 / f32::from(th));
    out.clear();
    for cell in cells {
        // A rule sprite (the dock's › arriving) is one whole cell of the mask
        // plane; the same instance, the same packing, only the slot differs.
        if let Some(kind) = cell.rule {
            let (uv0, placed) = slot_uv(atlas, upload, metrics, inv, rule_ask(kind));
            out.push(FxInstance {
                pos: cell.glyph.pos,
                uv0,
                rgba: cell.glyph.rgba,
                fx: [
                    cell.t,
                    fx_packed(cell.effect, placed.plane, placed.half),
                    cell.seed,
                    0.0,
                ],
            });
            continue;
        }
        for part in fan(atlas, upload, metrics, inv, &cell.glyph, clusters)
            .into_iter()
            .flatten()
        {
            out.push(FxInstance {
                pos: part.pos,
                uv0: part.uv0,
                rgba: cell.glyph.rgba,
                fx: [
                    cell.t,
                    fx_packed(cell.effect, part.plane, part.half),
                    cell.seed,
                    0.0,
                ],
            });
        }
    }
}

/// The effect id, the plane and the half of a wide glyph in the one `f32` the
/// shader unpacks (`id | plane << 5 | half << 6`).
fn fx_packed(effect: u32, plane: Plane, half: Half) -> f32 {
    let plane = match plane {
        Plane::Mask => 0,
        Plane::Color => 1,
    };
    let half = match half {
        Half::Whole => 0,
        Half::Left => 1,
        Half::Right => 2,
    };
    (effect | plane << 5 | half << 6) as f32
}

/// Whether an effect instance samples the colour plane (bit 5 of `fx[1]`, the
/// packing [`fx_list`] writes).
pub(crate) fn fx_is_color(instance: &FxInstance) -> bool {
    (instance.fx[1] as u32 >> 5) & 1 == 1
}

/// Where a glyph lives in the texture: the quad's position, the slot's uv, its
/// plane and which half of a wide glyph it is.
pub(crate) struct Part {
    pub(crate) pos: [f32; 2],
    pub(crate) uv0: [f32; 2],
    pub(crate) plane: Plane,
    pub(crate) half: Half,
}

/// A glyph's slot — or a wide glyph's two halves: **the single body of the
/// fan-out**.
///
/// **Here, not in `Frame::push`.** The reason is the borrow: whether a glyph
/// takes one slot or two is decided by the ink gate, i.e. `Atlas::slot`, and
/// `push` cannot borrow the atlas (why `GlyphCell` carries no uv: resolving in
/// the sink would keep the borrow alive across `draw` and the first frame with
/// a glyph would hit `BorrowMutError`). Here the atlas is already borrowed.
///
/// `metrics` is the atlas's **slot** metric (the uploads' size); the
/// right half's quad steps by the **grid's** cell, read from
/// `Atlas::metrics` here.
///
/// Every surface gets it for free: [`glyph_lists`] runs per list (stripes,
/// grid, fill band, dock) and the typing effects fan out through here too.
/// The lesson — a surface must earn everything derived from the grid on its
/// own — is paid in one place.
pub(crate) fn fan(
    atlas: &mut Atlas,
    upload: &mut impl SlotUpload,
    metrics: Metrics,
    inv: (f32, f32),
    glyph: &GlyphCell,
    clusters: &Clusters,
) -> [Option<Part>; 2] {
    let want = if glyph.wide { Half::Left } else { Half::Whole };
    // The second quad steps by the **grid's** cell, not the slot: the
    // right half is the next column.
    let cell_w = atlas.metrics().cell_px.0;
    // **The cluster reaches the atlas here**: interning needs
    // the atlas's borrow and the sink cannot take it. Both halves come
    // from the same sprite. Falling back to the base character is not written
    // twice: an id missing from the table is `Char`, and a cluster that does
    // not shape or fails the gate is `Atlas::slot`'s own answer.
    let sprite = glyph
        .cluster
        .and_then(|id| clusters.get(id))
        .map_or(Sprite::Char(glyph.ch), |text| atlas.intern(text));
    let (uv0, placed) = slot_uv(
        atlas,
        upload,
        metrics,
        inv,
        SlotAsk {
            sprite,
            face: glyph.face,
            size: glyph.size,
            want,
        },
    );
    let first = Part {
        pos: glyph.pos,
        uv0,
        plane: placed.plane,
        half: placed.half,
    };
    // The second quad **only if the gate said two cells**. A character
    // declared wide whose ink fits one cell (`☕`, fullwidth `！`) returns
    // `Whole` and this never runs — otherwise an empty quad would land to its
    // right. The grid already reserved two columns for it, so the neighbour is
    // a spacer and yields no glyph.
    if placed.half != Half::Left {
        return [Some(first), None];
    }
    let (uv1, right) = slot_uv(
        atlas,
        upload,
        metrics,
        inv,
        SlotAsk {
            sprite,
            face: glyph.face,
            size: glyph.size,
            want: Half::Right,
        },
    );
    [
        Some(first),
        Some(Part {
            pos: [glyph.pos[0] + f32::from(cell_w), glyph.pos[1]],
            uv0: uv1,
            plane: right.plane,
            half: right.half,
        }),
    ]
}

/// The identity of the slot asked of the atlas — [`Atlas::slot`]'s four
/// arguments, named as one thing: the atlas key in its request form.
struct SlotAsk {
    sprite: Sprite,
    face: Face,
    size: SizeClass,
    /// The half the caller **wants**; the answer's half may differ (see
    /// [`Half::Whole`]).
    want: Half,
}

/// Resolves a sprite's slot, uploads it if it was just allocated and returns
/// its uv0.
///
/// The **common body** of the glyph and rule loops; the two differ only in
/// the sprite and face they ask for. Copied, the upload branch would live in
/// two places, and an upload forgotten in one would mean "the slot exists but
/// the texture is empty" — an invisible glyph no counter notices.
///
/// `Upload` already carries the origin — `bt-atlas` returns both in the same
/// answer on purpose. Using it for a new slot drops a `%` + `/` pair per sprite
/// and avoids stating the same fact twice; `slot_origin` is left to the cached
/// and tofu paths. (`upload` borrows the atlas; consuming it in the `if let`
/// ends the borrow so the atlas can be asked again.)
fn slot_uv(
    atlas: &mut Atlas,
    upload: &mut impl SlotUpload,
    metrics: Metrics,
    inv: (f32, f32),
    ask: SlotAsk,
) -> ([f32; 2], Placed) {
    let (placed, fresh) = atlas.slot(ask.sprite, ask.face, ask.size, ask.want);
    let (x, y) = if let Some(fresh) = fresh {
        // **The texture is chosen by plane, not by the caller**, and so is the
        // row pitch ([`slot_layout`]).
        upload.upload(fresh.plane, fresh.origin, metrics, fresh.bytes);
        // **The pair's right half is uploaded in the same answer.** `bt-atlas`
        // allocates both slots atomically and hands both byte runs together;
        // skipped here, the right slot would stay unwritten and the
        // character's right half would be drawn with a neighbour's bitmap — a
        // silent corruption.
        if let Some(right) = fresh.right {
            upload.upload(fresh.plane, right, metrics, fresh.right_bytes);
        }
        fresh.origin
    } else {
        atlas.slot_origin(placed.slot)
    };
    ([f32::from(x) * inv.0, f32::from(y) * inv.1], placed)
}
