# Phase 2 — bt-gpu: shader zinciri

## Özet

`.metal → build.rs → metallib → MTLLibrary → pipeline` zinciri `bt-gpu`'da
açılır ve gerçek device üstünde sınanır; pencere yok, `xcrun` ilk kez burada
gerekir.

_Requirements: R4, R4.1, R4.2, R5, R5.1_

---

## 1. Bağımlılıklar

`Cargo.toml` (workspace) `[workspace.dependencies]`'e:

```toml
objc2             = "0.6"
objc2-foundation  = "0.3"
objc2-metal       = "0.3"
objc2-quartz-core = "0.3"
# DispatchData kurucuları `block2` feature'ının arkasında; objc2-metal bunu
# açmaz. Kullanıcı onaylı bağımlılık kararı: discussion.md → Karar.
dispatch2         = { version = "0.3", features = ["block2"] }
```

`crates/bt-gpu/Cargo.toml`:

```toml
[dependencies]
bt-core  = { workspace = true }
bt-atlas = { workspace = true }
objc2 = { workspace = true }
objc2-foundation = { workspace = true }
objc2-metal = { workspace = true }
objc2-quartz-core = { workspace = true }
dispatch2 = { workspace = true }
```

Feature'lar varsayılan (framework crate'leri varsayılanda her başlığı açar);
derleme süresi büyürse daraltma 00X'in işidir, bugün ölçülmedi.

## 2. Shader

`crates/bt-gpu/shaders/quad.metal`

```metal
#include <metal_stdlib>
using namespace metal;

// Tam ekran üçgeni: vertex buffer yok, üç köşe vertex_id'den türetilir.
// (0,0) (2,0) (0,2) → clip uzayında (-1,-1) (3,-1) (-1,3); ekranı örter.
struct QuadOut { float4 position [[position]]; };

vertex QuadOut quad_vertex(uint vid [[vertex_id]]) {
    float2 p = float2((vid << 1) & 2, vid & 2);
    QuadOut o;
    o.position = float4(p * 2.0 - 1.0, 0.0, 1.0);
    return o;
}

// Rust karşılığı: bt_gpu::renderer::Uniforms, #[repr(C)] { colour: [f32; 4] }.
// float4 16 bayt hizalı; alan sırası ve boyutu iki tarafta aynı kalmalı.
struct Uniforms { float4 colour; };

fragment float4 quad_fragment(QuadOut in [[stage_in]],
                              constant Uniforms& u [[buffer(0)]]) {
    return u.colour;
}
```

## 3. build.rs

`crates/bt-gpu/build.rs` — **bu crate'te**, kökte değil: `bt-core`
GPU'suz ve Xcode'suz test edilebilir kalır.

```rust
//! shaders/*.metal → $OUT_DIR/default.metallib. Reçete tek yerde burasıdır;
//! `make shader` yalnız `touch` + `cargo build -p bt-gpu` sarmalayıcısıdır.

use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    // Dizin olarak izlenir: yeni eklenen .metal de yeniden derlemeyi tetikler.
    println!("cargo:rerun-if-changed=shaders");

    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    xcrun_kontrol();

    let mut airs = Vec::new();
    for entry in fs::read_dir("shaders").expect("shaders/ dizini okunamadı") {
        let path = entry.expect("dizin girdisi").path();
        if path.extension().is_some_and(|e| e == "metal") {
            let air = out.join(path.file_name().unwrap()).with_extension("air");
            kos(Command::new("xcrun").args(["-sdk", "macosx", "metal"])
                // macOS 26 SDK'sıyla üretilen metallib macOS 14'te çalışma
                // zamanında reddedilir; hedef LSMinimumSystemVersion ile aynı.
                .args(["-std=metal3.1", "-mmacos-version-min=14.0", "-c"])
                .arg(&path).arg("-o").arg(&air));
            airs.push(air);
        }
    }
    assert!(!airs.is_empty(), "shaders/ altında .metal yok");
    kos(Command::new("xcrun").args(["-sdk", "macosx", "metallib"])
        .args(&airs).arg("-o").arg(out.join("default.metallib")));
}

fn xcrun_kontrol() {
    let ok = Command::new("xcrun").args(["-sdk", "macosx", "-f", "metal"])
        .output().map(|o| o.status.success()).unwrap_or(false);
    assert!(ok, "bateri: `xcrun -sdk macosx -f metal` başarısız. Metal shader \
        derleyicisi için Xcode gerekiyor; Command Line Tools tek başına `metal`'ı \
        taşımaz. `xcode-select -p` Xcode'u göstermeli.");
}

fn kos(cmd: &mut Command) {
    let durum = cmd.status().unwrap_or_else(|e| panic!("{cmd:?} başlatılamadı: {e}"));
    assert!(durum.success(), "shader derlemesi başarısız: {cmd:?}");
}
```

`build.rs`'te `panic`/`assert` serbesttir: derleme zamanı, PTY yolu değil.

## 4. Hata tipi

`crates/bt-gpu/src/error.rs`

```rust
use objc2::rc::Retained;
use objc2_foundation::NSError;

#[derive(Debug)]
pub enum GpuError {
    /// `MTLCreateSystemDefaultDevice` `None` döndü.
    NoDevice,
    Library(Retained<NSError>),
    /// metallib yüklendi ama adı verilen fonksiyon yok — sessiz `None` değil,
    /// adıyla hata (`make duman`'ın kırmızısı buradan gelir).
    MissingFunction(&'static str),
    Pipeline(Retained<NSError>),
    NoCommandQueue,
    NoDrawable,
    NoCommandBuffer,
}

impl std::fmt::Display for GpuError { /* Türkçe, tek satır, adıyla */ }
impl std::error::Error for GpuError {}
```

## 5. Surface

`crates/bt-gpu/src/surface.rs`

```rust
//! Çizim yüzeyi: CAMetalLayer'ın sahibi bt-gpu'dur; bt-shell yalnız &CALayer alır.

pub struct Surface { layer: Retained<CAMetalLayer> }

impl Surface {
    pub fn new(device: &ProtocolObject<dyn MTLDevice>, pixel_format: MTLPixelFormat) -> Self {
        let layer = CAMetalLayer::new();          // objc2-quartz-core; `unsafe` mi, uygulamada bak
        layer.setDevice(Some(device));
        layer.setPixelFormat(pixel_format);
        layer.setFramebufferOnly(true);
        Self { layer }
    }
    /// bt-shell'in NSView'a takacağı şey; CAMetalLayer → CALayer deref.
    pub fn ca_layer(&self) -> &CALayer { &self.layer }
    /// Pencere boyutu/ölçeği değişince; piksel cinsinden.
    pub fn set_size(&self, width_px: f64, height_px: f64, scale: f64) {
        self.layer.setContentsScale(scale);
        self.layer.setDrawableSize(CGSize { width: width_px, height: height_px });
    }
    pub(crate) fn layer(&self) -> &CAMetalLayer { &self.layer }
}
```

## 6. Renderer

`crates/bt-gpu/src/renderer.rs`

```rust
// MTLCreateSystemDefaultDevice CoreGraphics'e link ister; crate bağımlılığı
// değildir, katman tablosunu bozmaz (objc2-metal/src/lib.rs belgeliyor).
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {}

static METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/default.metallib"));

/// quad.metal → Uniforms ile alan alan aynı. float4 = 16 bayt.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Uniforms { pub colour: [f32; 4] }

pub struct Renderer {
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pipeline: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    pixel_format: MTLPixelFormat,
    frames: Cell<u64>,
}

impl Renderer {
    /// Sistem varsayılan device ile; bt-shell yalnız bunu çağırır ve
    /// objc2-metal'i hiç görmez.
    pub fn system_default() -> Result<Self, GpuError> {
        let device = MTLCreateSystemDefaultDevice().ok_or(GpuError::NoDevice)?;
        Self::new(device, MTLPixelFormat::BGRA8Unorm)
    }

    pub fn new(device: Retained<ProtocolObject<dyn MTLDevice>>, pixel_format: MTLPixelFormat) -> Result<Self, GpuError> {
        let data = DispatchData::from_static_bytes(METALLIB);   // kopyasız
        let library = device.newLibraryWithData_error(&data).map_err(GpuError::Library)?;
        let vs = library.newFunctionWithName(ns_string!("quad_vertex"))
            .ok_or(GpuError::MissingFunction("quad_vertex"))?;
        let fs = library.newFunctionWithName(ns_string!("quad_fragment"))
            .ok_or(GpuError::MissingFunction("quad_fragment"))?;
        let desc = MTLRenderPipelineDescriptor::new();
        desc.setVertexFunction(Some(&vs));
        desc.setFragmentFunction(Some(&fs));
        unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(pixel_format);
        let pipeline = device.newRenderPipelineStateWithDescriptor_error(&desc).map_err(GpuError::Pipeline)?;
        let queue = device.newCommandQueue().ok_or(GpuError::NoCommandQueue)?;
        Ok(Self { device, queue, pipeline, pixel_format, frames: Cell::new(0) })
    }

    pub fn surface(&self) -> Surface { Surface::new(&self.device, self.pixel_format) }
    pub fn frames(&self) -> u64 { self.frames.get() }

    /// Tek kare: drawable al, quad'ı çiz, sun. İskelette senkron
    /// (`waitUntilCompleted`) — kare sayacı "GPU bitirdi" demek olsun;
    /// 002 display link gelince asenkron olur.
    pub fn draw(&self, surface: &Surface, colour: [f32; 4]) -> Result<(), GpuError> {
        let drawable = surface.layer().nextDrawable().ok_or(GpuError::NoDrawable)?;
        let cmd = self.queue.commandBuffer().ok_or(GpuError::NoCommandBuffer)?;
        let pass = MTLRenderPassDescriptor::new();
        let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        att.setTexture(Some(&drawable.texture()));
        att.setLoadAction(MTLLoadAction::DontCare);   // quad her pikseli yazar
        att.setStoreAction(MTLStoreAction::Store);
        let enc = cmd.renderCommandEncoderWithDescriptor(&pass).ok_or(GpuError::NoCommandBuffer)?;
        enc.setRenderPipelineState(&self.pipeline);
        let u = Uniforms { colour };
        unsafe {
            enc.setFragmentBytes_length_atIndex(NonNull::from(&u).cast(), size_of::<Uniforms>(), 0);
            enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
        }
        enc.endEncoding();
        cmd.presentDrawable(drawable.as_ref());
        cmd.commit();
        cmd.waitUntilCompleted();
        self.frames.set(self.frames.get() + 1);
        Ok(())
    }
}
```

`lib.rs`: `pub mod error; pub mod renderer; pub mod surface;` + `pub use`.

## 7. Sınamalar

`crates/bt-gpu/src/renderer.rs` → `#[cfg(test)] mod tests`

```rust
#[test]
fn metallib_gomulu_ve_gecerli() {
    // Metal kütüphanesi dosyası "MTLB" sihirli sayısıyla başlar.
    assert_eq!(&METALLIB[..4], b"MTLB");
}

#[test]
fn library_ve_pipeline_kurulur() {
    // Device yoksa `ignored` değil açık hata: bu makinede Metal var, yokluğu bir kusurdur.
    let r = Renderer::system_default().expect("Metal device ve pipeline");
    assert_eq!(r.frames(), 0);
}
```

Eksik fonksiyon adı için sınama **yok**: ikinci bir metallib gerektirir; `Result`
yolu tip düzeyinde bağlıdır ve `make duman` (phase-3) onu koşturur.

## 8. Makefile

```make
.PHONY: shader
# build.rs'in yaptığını cargo'nun bayatlık takibini atlayarak koşturur;
# derleme reçetesi burada TEKRARLANMAZ.
shader:
	touch crates/bt-gpu/shaders/*.metal
	$(CARGO) build -p bt-gpu
```

---

## Uygulama Notları

## Yayın Etkisi

- **shader**: ilk `.metal` ve `build.rs`; `make shader` bu phase'de gerçek olur.
  `Uniforms` Rust/MSL alan eşlemesi: tek alan, `[f32; 4]` ↔ `float4`.
- **yeni bağımlılık**: `objc2`, `objc2-foundation`, `objc2-metal`,
  `objc2-quartz-core`, `dispatch2` (`block2`) — `discussion.md` → Karar ile
  kullanıcı onaylı. `Cargo.lock` farkı bu commit'te.
- Ölçüm bekleyen iddia: yok (derleme süresi ölçülmedi, iddia edilmedi).

---

## Checklist

- [ ] Workspace ve `bt-gpu` bağımlılıkları; `cargo tree -p bt-core` hâlâ temiz
- [ ] `shaders/quad.metal`, `build.rs` (`xcrun` kontrolü, `-mmacos-version-min=14.0`, dizin `rerun-if-changed`)
- [ ] `GpuError`, `Surface`, `Renderer` (`system_default`, `new`, `surface`, `draw`, `frames`)
- [ ] Test: `metallib_gomulu_ve_gecerli`, `library_ve_pipeline_kurulur` geçer
- [ ] Test: `quad.metal`'e kasıtlı sözdizimi hatası → `cargo build -p bt-gpu` satır numarasıyla düşer; geri al
- [ ] Test: `shaders/`'a yeni boş `.metal` eklenince `cargo build` yeniden derler (dizin izleme)
- [ ] `make shader` hedefi eklendi ve koşuyor
- [ ] Doğrulama geçti (`make hepsi`; koşullu: `make shader`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi (mercek 2 yeni bağımlılık: karar kaydına bağla; mercek 9 `Uniforms` eşlemesi)
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
