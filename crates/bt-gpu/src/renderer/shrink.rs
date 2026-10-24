//! A still of the pane: its frame drawn at the pane's own size into a target
//! that is ours (not the window's), brought to a tile's size by a chain of
//! halvings on the GPU, and only then read back — as RGBA bytes.
//!
//! **Why a chain and not a readback of the whole picture**: the picture is the
//! pane's size in pixels (1400×1800 is 10 MB) and a card wants a tile of a
//! few hundred by a hundred and some, twice a second, for each pane of a
//! tab. Reading the big one would move ten megabytes across the bus to throw
//! nine tenths of every row away; reducing first moves the tile.
//!
//! **Why halvings**: one pass that averaged a 5× larger rectangle would need
//! 25 taps per pixel; a pass that averages 2×2 is four, and `n` of them reach
//! a `2ⁿ` reduction. Each pass is the same pipeline (`shrink.wgsl`) and the
//! last one brings what is left (at most 2× the tile) to the tile's exact
//! size — and crops, when the tile's shape is not the pane's ([`plan`]).
//!
//! **The colour space is the target's**: every texture is [`FORMAT`], an sRGB
//! format, so a tap decodes to linear light, the four are averaged there, and
//! the write encodes again. The bytes that come back are sRGB-encoded — what
//! an image in the sRGB space is — and an average of black and white comes
//! back as 188, not 128 (`a_checker_shrinks_to_its_linear_average`).
//!
//! **Nothing here is a frame**: [`Renderer::snapshot`] goes through
//! [`Renderer::encode`], not [`Renderer::draw`], so the frame counters
//! (`frames=`, `cells=`, `glyphs=`, `rules=`), the in-flight queue and the
//! GPU timing are untouched. The textures belong to the caller's
//! [`Chain`], allocated once for a (picture, tile) pair and kept for as many
//! snapshots as it fits; dropping the chain gives them back.

use super::ui::UiList;
use super::*;

/// The most halvings a chain will make: `2¹²` is far past any picture the
/// device can hold, so the cap only protects against a degenerate tile.
const MAX_HALVINGS: usize = 12;

/// How long a snapshot waits for the GPU to finish before it gives up: a
/// snapshot is a courtesy to a card, never worth a stalled main thread.
const READ_TIMEOUT: Duration = Duration::from_millis(500);

/// The shape of the reduction from a picture to a tile — arithmetic only, so
/// it is tested without a GPU.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Plan {
    /// The sizes of the halved textures, in the order the chain makes them;
    /// empty when the picture is at most twice the tile.
    pub halvings: Vec<(u32, u32)>,
    /// The part of the last texture that fills the tile: u0, v0, u1, v1,
    /// normalised, top left first.
    pub crop: [f32; 4],
}

/// The plan for a `source`-sized picture and a `tile`.
///
/// **The picture fills the tile**: it is scaled until it covers it in both
/// directions and what spills over is cropped. The part that stays is the
/// **left edge and the bottom** — a terminal's text starts at the left and
/// its newest line is the last one, so those are the two things a smaller
/// picture must not lose. When the tile has the picture's shape (the card's
/// map is shaped like the tab's area), nothing is cropped.
///
/// **Halvings until the last pass is at most 2×**: pass `n` of the chain
/// holds `2⁻ⁿ` of the picture's resolution, and the last one brings it to
/// the tile's resolution — it must be between 1× and 2× for its four taps to
/// cover its footprint.
pub(crate) fn plan(source: (u32, u32), tile: (u32, u32)) -> Plan {
    let (sw, sh) = (f64::from(source.0.max(1)), f64::from(source.1.max(1)));
    let (tw, th) = (f64::from(tile.0.max(1)), f64::from(tile.1.max(1)));
    let fill = (tw / sw).max(th / sh);
    let shown = ((tw / fill).min(sw), (th / fill).min(sh));
    let crop = [
        0.0,
        ((sh - shown.1) / sh) as f32,
        (shown.0 / sw) as f32,
        1.0,
    ];
    let mut halvings = Vec::new();
    // Each halving halves the resolution; the last pass may then be 2× at
    // most: `2 · fill · 2ⁿ ≥ 1`.
    let mut factor = 1u32;
    while 2.0 * fill * f64::from(factor) < 1.0 && halvings.len() < MAX_HALVINGS {
        factor *= 2;
        halvings.push((
            source.0.div_ceil(factor).max(1),
            source.1.div_ceil(factor).max(1),
        ));
    }
    Plan { halvings, crop }
}

/// The part of a `source`-sized picture that fills a `tile`, as [`plan`] cuts
/// it: u0, v0, u1, v1, normalised, top left first. For a picture that does not
/// come out of this renderer but must fill the same tile the same way — a web
/// view's, which the shell has taken and shrinks itself: the rule is one, so a
/// card's tiles do not keep the left edge and the bottom of some pictures and
/// the middle of others.
pub fn tile_crop(source: (u32, u32), tile: (u32, u32)) -> [f32; 4] {
    plan(source, tile).crop
}

/// The immediates of one pass that reads `crop` of a `from`-sized texture into
/// a `to`-sized one: where its four taps stand (`shrink.wgsl`'s `tap`).
///
/// The taps sit `(r − 1) / 2` texels from a pixel's centre, `r` being the
/// texels a target pixel covers: at 2 they land on the centres of the 2×2
/// block under it (an exact average), at 1 on its own texel (a copy), and in
/// between the footprint is covered as well as four bilinear taps can.
fn pass_immediates(from: (u32, u32), to: (u32, u32), crop: [f32; 4]) -> ShrinkImmediates {
    let tap = |from: u32, to: u32, span: f32| -> f32 {
        let texels = f64::from(span) * f64::from(from) / f64::from(to.max(1));
        (((texels - 1.0) / 2.0).clamp(0.0, 0.5) / f64::from(from.max(1))) as f32
    };
    ShrinkImmediates {
        src: crop,
        tap: [
            tap(from.0, to.0, crop[2] - crop[0]),
            tap(from.1, to.1, crop[3] - crop[1]),
        ],
        pad: [0.0; 2],
    }
}

/// One pass: its target, the bind group that reads the pass before it, and
/// its immediates.
struct Pass {
    target: Target,
    bind: wgpu::BindGroup,
    imm: ShrinkImmediates,
}

/// The textures of a snapshot, made for one (picture, tile) pair and kept:
/// the picture, each halving, the tile, and the buffer the tile is read
/// through. The caller owns it (`Shot`); letting go of it frees them all.
pub(crate) struct Chain {
    size: (u32, u32),
    tile: (u32, u32),
    /// The picture the frame is drawn into.
    full: Target,
    passes: Vec<Pass>,
    readback: wgpu::Buffer,
    /// The tile's row pitch in the readback buffer (a multiple of 256).
    pitch: u32,
    #[cfg(test)]
    serial: u64,
}

impl Chain {
    fn new(gpu: &Gpu, size: (u32, u32), tile: (u32, u32)) -> Self {
        let plan = plan(size, tile);
        let texture = |label, size: (u32, u32), usage| {
            Target::new(gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage,
                view_formats: &[],
            }))
        };
        // `COPY_DST` on the picture is for the pixel guards, which write a
        // known picture into it instead of drawing a frame.
        let full = texture(
            "snapshot picture",
            size,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
        );
        let mut passes: Vec<Pass> = Vec::with_capacity(plan.halvings.len() + 1);
        let mut from = (size, full.view.clone());
        let mut steps: Vec<((u32, u32), [f32; 4])> = plan
            .halvings
            .iter()
            .map(|&half| (half, [0.0, 0.0, 1.0, 1.0]))
            .collect();
        steps.push((tile, plan.crop));
        let last = steps.len() - 1;
        for (index, (to, crop)) in steps.into_iter().enumerate() {
            let (label, usage) = if index == last {
                (
                    "snapshot tile",
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                )
            } else {
                (
                    "snapshot halving",
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                )
            };
            let target = texture(label, to, usage);
            let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("snapshot pass"),
                layout: &gpu.plane_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&from.1),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&gpu.linear),
                    },
                ],
            });
            let next = (to, target.view.clone());
            passes.push(Pass {
                target,
                bind,
                imm: pass_immediates(from.0, to, crop),
            });
            from = next;
        }
        let pitch = (tile.0 * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("snapshot readback"),
            size: u64::from(pitch) * u64::from(tile.1),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        #[cfg(test)]
        static MADE: AtomicU64 = AtomicU64::new(0);
        Self {
            size,
            tile,
            full,
            passes,
            readback,
            pitch,
            #[cfg(test)]
            serial: MADE.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Which chain this is, by the order they were made in (a guard's way to
    /// tell a reused chain from a remade one).
    #[cfg(test)]
    pub(crate) fn serial(&self) -> u64 {
        self.serial
    }

    /// Whether this chain was made for these sizes.
    fn fits(&self, size: (u32, u32), tile: (u32, u32)) -> bool {
        self.size == size && self.tile == tile
    }

    /// The tile of whatever is in the picture now, as RGBA bytes: the passes,
    /// the copy to the readback buffer, a bounded wait for the GPU, then the
    /// rows without their padding and with red and blue where the target's
    /// format has them swapped.
    fn reduce(&self, gpu: &Gpu) -> Result<Vec<u8>, GpuError> {
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("snapshot"),
            });
        for pass in &self.passes {
            let mut render = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("snapshot pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &pass.target.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..wgpu::RenderPassDescriptor::default()
            });
            render.set_pipeline(&gpu.shrink);
            render.set_bind_group(0, &pass.bind, &[]);
            render.set_immediates(0, bytes_of(std::slice::from_ref(&pass.imm)));
            render.draw(0..3, 0..1);
        }
        let last = &self
            .passes
            .last()
            .expect("a chain has at least its tile pass")
            .target;
        encoder.copy_texture_to_buffer(
            last.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.pitch),
                    rows_per_image: Some(self.tile.1),
                },
            },
            last.texture.size(),
        );
        let submission = gpu.queue.submit([encoder.finish()]);
        let (sent, mapped) = std::sync::mpsc::channel();
        self.readback
            .map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = sent.send(result);
            });
        let deadline = std::time::Instant::now() + READ_TIMEOUT;
        let result = loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match gpu.device.poll(wgpu::PollType::Wait {
                submission_index: Some(submission.clone()),
                timeout: Some(left),
            }) {
                Ok(_) => {}
                Err(wgpu::PollError::Timeout) => {
                    return Err(GpuError::Wgpu("the snapshot's readback timed out".into()));
                }
                Err(e) => return Err(GpuError::Wgpu(e.to_string())),
            }
            if let Ok(result) = mapped.try_recv() {
                break result;
            }
            if std::time::Instant::now() >= deadline {
                return Err(GpuError::Wgpu("the snapshot's readback timed out".into()));
            }
        };
        result.map_err(|e| GpuError::Wgpu(e.to_string()))?;
        let data = self
            .readback
            .slice(..)
            .get_mapped_range()
            .map_err(|e| GpuError::Wgpu(e.to_string()))?;
        let row = self.tile.0 as usize * 4;
        let mut rgba = Vec::with_capacity(row * self.tile.1 as usize);
        for line in data
            .chunks_exact(self.pitch as usize)
            .take(self.tile.1 as usize)
        {
            for pixel in line[..row].chunks_exact(4) {
                rgba.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]]);
            }
        }
        drop(data);
        self.readback.unmap();
        Ok(rgba)
    }
}

impl Renderer {
    /// The pane's `frame` as a `tile`-sized picture: the frame drawn at
    /// `size` pixels into the chain's own picture, reduced, read back as RGBA
    /// bytes (`tile.0 × tile.1 × 4`, sRGB-encoded, rows top down).
    ///
    /// `chain` is the caller's: made here the first time and whenever the
    /// sizes change, reused otherwise. **Not a frame**: the draw goes through
    /// [`Renderer::encode`] and none of [`Renderer::draw`]'s bookkeeping runs,
    /// so a snapshot is invisible to the frame counters, the in-flight queue
    /// and the GPU timing.
    ///
    /// Errors are the snapshot's alone — a validation or out-of-memory error
    /// caught in the scopes here comes back as `Err` and the chain is dropped
    /// (a later snapshot builds a fresh one); nothing is reported to the frame
    /// path's failure policy, because no frame failed.
    pub(crate) fn snapshot(
        &self,
        chain: &mut Option<Chain>,
        clear: LinearRgba,
        frame: &Frame,
        size: (u32, u32),
        tile: (u32, u32),
    ) -> Result<Vec<u8>, GpuError> {
        self.snapshot_of(chain, size, tile, |full| {
            self.encode(full, clear, frame, None).map(drop)
        })
    }

    /// A page's `list` as a `tile`-sized picture: [`Renderer::snapshot`] for a pane that shows a
    /// page in place of its grid. `list` is in pixels of a `size` surface, as the page built it.
    ///
    /// **A reading of the last frame, not a frame of its own**: the image textures the list
    /// names are the live ones (made when the page handed their pixels over, kept while the
    /// lists drawn name them), used where they stand and never made, replaced or let go of here
    /// — `list` is the one the last frame drew, so what it needs is already held, and a texture
    /// that is not is left out of the picture. The text atlas is read the same way. The sizes,
    /// the errors and the counters are [`Renderer::snapshot`]'s.
    pub(crate) fn snapshot_ui(
        &self,
        chain: &mut Option<Chain>,
        clear: LinearRgba,
        list: &UiList,
        size: (u32, u32),
        tile: (u32, u32),
    ) -> Result<Vec<u8>, GpuError> {
        self.snapshot_of(chain, size, tile, |full| {
            self.encode_ui(full, clear, list, false).map(drop)
        })
    }

    /// The error scopes and the chain of a snapshot, around `draw` — what puts the picture into
    /// the chain's full-size target.
    fn snapshot_of(
        &self,
        chain: &mut Option<Chain>,
        size: (u32, u32),
        tile: (u32, u32),
        draw: impl FnOnce(&Target) -> Result<(), GpuError>,
    ) -> Result<Vec<u8>, GpuError> {
        if size.0 == 0 || size.1 == 0 || tile.0 == 0 || tile.1 == 0 {
            return Err(GpuError::Wgpu("a snapshot of nothing".into()));
        }
        let memory = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = self
            .gpu
            .device
            .push_error_scope(wgpu::ErrorFilter::Validation);
        let result = self.snapshot_in(chain, size, tile, draw);
        let errors = [block_on(validation.pop()), block_on(memory.pop())];
        if let Some(error) = errors.into_iter().flatten().next() {
            *chain = None;
            return Err(GpuError::Wgpu(error.to_string()));
        }
        if result.is_err() {
            *chain = None;
        }
        result
    }

    /// [`Renderer::snapshot_of`] inside its error scopes.
    fn snapshot_in(
        &self,
        chain: &mut Option<Chain>,
        size: (u32, u32),
        tile: (u32, u32),
        draw: impl FnOnce(&Target) -> Result<(), GpuError>,
    ) -> Result<Vec<u8>, GpuError> {
        if !chain.as_ref().is_some_and(|chain| chain.fits(size, tile)) {
            *chain = None;
        }
        let chain = chain.get_or_insert_with(|| Chain::new(self.gpu, size, tile));
        draw(&chain.full)?;
        chain.reduce(self.gpu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A chain over a picture whose pixels (B, G, R, A) the test writes.
    fn chain_over(
        gpu: &'static Gpu,
        size: (u32, u32),
        tile: (u32, u32),
        pixels: &[[u8; 4]],
    ) -> Chain {
        assert_eq!(pixels.len(), (size.0 * size.1) as usize);
        let chain = Chain::new(gpu, size, tile);
        let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
        gpu.queue.write_texture(
            chain.full.texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        chain
    }

    const BLACK: [u8; 4] = [0, 0, 0, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    #[test]
    fn the_plan_halves_until_the_last_pass_is_at_most_twice_the_tile() {
        // A picture 5× the tile: two halvings leave 1.25×.
        let five = plan((1400, 1800), (280, 360));
        assert_eq!(five.halvings, vec![(700, 900), (350, 450)]);
        assert_eq!(
            five.crop,
            [0.0, 0.0, 1.0, 1.0],
            "the same shape crops nothing"
        );
        // Exactly 2× needs no halving, 4× needs one: the last pass does the
        // rest, always at most 2×.
        assert!(plan((560, 720), (280, 360)).halvings.is_empty());
        assert_eq!(plan((1120, 1440), (280, 360)).halvings, vec![(560, 720)]);
        // A tile the picture does not shrink to is one plain pass.
        assert!(plan((100, 100), (200, 200)).halvings.is_empty());
        // Odd sizes round up, so no row of the picture is lost.
        assert_eq!(plan((9, 5), (1, 1)).halvings, vec![(5, 3), (3, 2)]);
    }

    #[test]
    fn the_crop_other_pictures_are_cut_by_is_the_plans() {
        for (source, tile) in [
            ((400, 800), (200, 100)),
            ((800, 400), (100, 200)),
            ((1400, 1800), (280, 360)),
            ((9, 5), (1, 1)),
        ] {
            assert_eq!(tile_crop(source, tile), plan(source, tile).crop);
        }
    }

    #[test]
    fn the_plan_fills_the_tile_and_keeps_the_left_edge_and_the_bottom() {
        // A tall picture in a wide tile: the width fills, the top is cut.
        let tall = plan((400, 800), (200, 100));
        assert_eq!(tall.crop, [0.0, 0.75, 1.0, 1.0]);
        // A wide picture in a tall tile: the height fills, the right is cut.
        let wide = plan((800, 400), (100, 200));
        assert_eq!(wide.crop, [0.0, 0.0, 0.25, 1.0]);
        // Whatever the shape, the crop is inside the picture.
        for (source, tile) in [((1400, 1800), (276, 120)), ((300, 90), (276, 180))] {
            let [u0, v0, u1, v1] = plan(source, tile).crop;
            assert!(u0 == 0.0 && (0.0..1.0).contains(&v0) && u1 > 0.0 && u1 <= 1.0 && v1 == 1.0);
        }
    }

    #[test]
    fn a_pass_puts_its_taps_on_the_centres_of_the_block_it_averages() {
        // A halving (2 texels per pixel): half a texel from the centre.
        let half = pass_immediates((8, 4), (4, 2), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(half.tap, [0.5 / 8.0, 0.5 / 4.0]);
        // A copy (1 texel per pixel): the taps sit on the pixel itself.
        let copy = pass_immediates((8, 4), (8, 4), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(copy.tap, [0.0, 0.0]);
        // A crop counts only the part it shows.
        let crop = pass_immediates((8, 8), (4, 4), [0.0, 0.0, 0.5, 0.5]);
        assert_eq!(crop.tap, [0.0, 0.0]);
    }

    #[test]
    fn a_checker_shrinks_to_its_linear_average() {
        // **The colour space's witness**: black and white are the sRGB
        // transfer function's fixed points, their average is not. Averaged in
        // linear light (0.5) and encoded again, grey is 188; averaged in the
        // encoded values it would be 128.
        let gpu = Gpu::shared();
        let pixels = [
            BLACK, WHITE, BLACK, WHITE, //
            WHITE, BLACK, WHITE, BLACK, //
            BLACK, WHITE, BLACK, WHITE, //
            WHITE, BLACK, WHITE, BLACK,
        ];
        let chain = chain_over(gpu, (4, 4), (2, 2), &pixels);
        let tile = chain.reduce(gpu).expect("the reduction");
        assert_eq!(tile.len(), 2 * 2 * 4);
        for pixel in tile.chunks_exact(4) {
            for channel in &pixel[..3] {
                assert!(
                    (187..=189).contains(channel),
                    "a checker averaged to {channel}, not the linear average 188 ({pixel:?})"
                );
            }
            assert_eq!(pixel[3], 255);
        }
    }

    #[test]
    fn a_chain_of_halvings_averages_every_pixel_once() {
        // 8×8 → 1×1 through three passes: a picture whose left half is white
        // and right half black is, in linear light, half-white again.
        let gpu = Gpu::shared();
        let pixels: Vec<[u8; 4]> = (0..64)
            .map(|index| if index % 8 < 4 { WHITE } else { BLACK })
            .collect();
        let chain = chain_over(gpu, (8, 8), (1, 1), &pixels);
        assert_eq!(chain.passes.len(), 3, "two halvings and the tile");
        let tile = chain.reduce(gpu).expect("the reduction");
        assert!(
            (187..=189).contains(&tile[0]),
            "the chain lost or doubled a pixel: {tile:?}"
        );
    }

    #[test]
    fn a_tile_the_size_of_the_picture_is_the_picture() {
        let gpu = Gpu::shared();
        let pixels: Vec<[u8; 4]> = (0..16u8)
            .map(|index| [index * 16, 255 - index * 12, index * 5 + 3, 255])
            .collect();
        let chain = chain_over(gpu, (4, 4), (4, 4), &pixels);
        let tile = chain.reduce(gpu).expect("the reduction");
        for (got, want) in tile.chunks_exact(4).zip(&pixels) {
            // RGBA out of BGRA in.
            for (g, w) in got.iter().zip([want[2], want[1], want[0], want[3]]) {
                assert!(g.abs_diff(w) <= 1, "{got:?} is not {want:?}");
            }
        }
    }

    #[test]
    fn a_tile_of_another_shape_keeps_the_bottom_and_the_left() {
        let gpu = Gpu::shared();
        // 4 wide, 8 tall: red on the left half, green on the right; the top
        // half black. A 4×4 tile shows the bottom half, red | green.
        let pixels: Vec<[u8; 4]> = (0..32)
            .map(|index| {
                let (x, y) = (index % 4, index / 4);
                if y < 4 {
                    BLACK
                } else if x < 2 {
                    [0, 0, 255, 255]
                } else {
                    [0, 255, 0, 255]
                }
            })
            .collect();
        let chain = chain_over(gpu, (4, 8), (4, 4), &pixels);
        let tile = chain.reduce(gpu).expect("the reduction");
        let at = |x: usize, y: usize| &tile[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(at(0, 0), [255, 0, 0, 255], "red, from the bottom half");
        assert_eq!(at(3, 3), [0, 255, 0, 255], "green, from the bottom half");
        // 8 wide, 4 tall into a 4×4 tile: the left half stays.
        let pixels: Vec<[u8; 4]> = (0..32)
            .map(|index| {
                if index % 8 < 4 {
                    [0, 0, 255, 255]
                } else {
                    WHITE
                }
            })
            .collect();
        let chain = chain_over(gpu, (8, 4), (4, 4), &pixels);
        let tile = chain.reduce(gpu).expect("the reduction");
        assert!(
            tile.chunks_exact(4).all(|pixel| pixel == [255, 0, 0, 255]),
            "the right half came into the tile: {tile:?}"
        );
    }
}
