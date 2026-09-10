# Phase 2 — bt-gpu: cell_bg pipeline ve Frame

## Özet

Hücre arka planlarını ve imleci instanced quad'larla çizen ikinci pipeline;
`Frame` sink'ten dolar; 001'in tam ekran quad'ı ve `make duman` sözleşmesi bu
phase'de olduğu gibi kalır. Pencere yok, display link yok.

_Requirements: R2_

---

## 1. Shader

`crates/bt-gpu/shaders/cell_bg.metal`

```metal
#include <metal_stdlib>
using namespace metal;

// Rust: bt_gpu::frame::Instance, #[repr(C)] { pos: [f32;2], size: [f32;2], rgba: [f32;4] }
// float2+float2+float4 = 32 bayt; float4 16 hizalı → struct hizası 16, sırası aynı.
struct Instance { float2 pos; float2 size; float4 rgba; };
struct Viewport { float2 px; };                       // drawable boyutu (piksel)

struct Out { float4 position [[position]]; float4 rgba; };

vertex Out cell_bg_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                          constant Instance* inst [[buffer(0)]],
                          constant Viewport& vp [[buffer(1)]]) {
    float2 corner = float2(vid & 1, (vid >> 1) & 1);   // 0..3 → dört köşe, triangle strip
    float2 p = inst[iid].pos + corner * inst[iid].size;
    float2 ndc = p / vp.px * 2.0 - 1.0;
    Out o; o.position = float4(ndc.x, -ndc.y, 0.0, 1.0); o.rgba = inst[iid].rgba; return o;
}
fragment float4 cell_bg_fragment(Out in [[stage_in]]) { return in.rgba; }
```

`build.rs` dizini izlediği için yeni dosya kendiliğinden derlenir.

## 2. Frame

`crates/bt-gpu/src/frame.rs`

```rust
#[repr(C)] #[derive(Clone, Copy)]
pub struct Instance { pub pos: [f32; 2], pub size: [f32; 2], pub rgba: [f32; 4] }
const _: () = assert!(size_of::<Instance>() == 32);

/// Bir karenin çizim listesi; hasar varsa `Session::frame`'in sink'iyle dolar.
pub struct Frame { pub instances: Vec<Instance>, pub cell_px: (f32, f32), pub bg_count: usize }
impl Frame {
    pub fn clear(&mut self) { self.instances.clear(); self.bg_count = 0; }
    pub fn push_bg(&mut self, col: u16, row: u16, rgba: [f32; 4]) { …; self.bg_count += 1; }
    pub fn push_cursor(&mut self, col: u16, row: u16, rgba: [f32; 4]) { … }   // sayılmaz
}
```

`bg_count` duman'ın `hucre=K`'sıdır (imleç hariç) — phase-3 kullanır.

## 3. Renderer

`renderer.rs`: ikinci pipeline `cell_bg` (`quad` kalır), `Viewport` uniform.
`draw(&self, drawable, clear: [f32;4], frame: &Frame)`:

1. Pass: `loadAction = Clear` (arka plan `clear`); quad pipeline **kalkar**
   bu yoldan — tam ekran quad'ın işini `Clear` yapar. 001'in `draw` imzası
   (`colour`) → `Frame` boşken aynı görüntü.
2. `newBufferWithBytes_length_options(instances, StorageModeShared)` — kare
   başına; komut tamponu buffer'ı tutar. Boş `Frame`'de buffer kurulmaz.
3. `drawPrimitives_vertexStart_vertexCount_instanceCount(TriangleStrip, 0, 4, n)`.
4. Bu phase'de **senkron kalır** (`waitUntilCompleted` + `status`); asenkron
   phase-3'te.

`make duman` (001): `draw_surface(surface, ARKA_PLAN, &Frame::empty())` →
`kare=1 pipeline=ok` aynen.

## 4. Sınamalar

- `Instance` 32 bayt assert (derleme zamanı).
- `cell_bg_pipeline_kurulur`: device üstünde ikinci pipeline.
- `frame_bg_count_imleci_saymaz`.

---

## Uygulama Notları

## Yayın Etkisi

- **shader:** `cell_bg.metal` yeni; `Instance`/`Viewport` Rust ↔ MSL eşlemesi.
- Yeni bağımlılık: yok. Belgeler: yok. Ölçüm bekleyen iddia: yok.

---

## Checklist

- [ ] `cell_bg.metal`, `Frame`, ikinci pipeline, `draw(drawable, clear, &Frame)`
- [ ] Test: `Instance` 32 bayt; `cell_bg_pipeline_kurulur`; `frame_bg_count_imleci_saymaz`
- [ ] Test: `make duman` hâlâ `kare=1 pipeline=ok`
- [ ] Doğrulama geçti (`make hepsi`; koşullu: `make shader`, `make duman`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi (mercek 9: `Instance` ↔ MSL, `[[buffer(0/1)]]` indeksleri)
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
