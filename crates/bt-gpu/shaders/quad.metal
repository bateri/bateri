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
