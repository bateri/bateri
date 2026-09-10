#include <metal_stdlib>
using namespace metal;

// Rust karşılığı: bt_gpu::frame::Instance,
// #[repr(C)] { pos: [f32; 2], size: [f32; 2], rgba: [f32; 4] }.
//
// Düzen iki tarafta alan alan aynı olmak zorunda: float2 8 hizalı, float4 16
// hizalı → pos@0, size@8, rgba@16, stride 32.
struct Instance {
    float2 pos;   // sol üst köşe, piksel
    float2 size;  // genişlik/yükseklik, piksel
    // Lineer RGBA: hedef BGRA8Unorm_sRGB ve kodlamayı ROP yapıyor. Buraya
    // ya da fragment'e bir gamma düzeltmesi eklemek paleti İKİ KEZ kodlar.
    float4 rgba;
};

// Her iki taraf KENDİ assert'iyle bağlı. Yalnız Rust'ta assert etmek yetmez:
// buraya eklenen bir alan stride'ı 32'den kaydırır, Rust tarafı bunu göremez
// ve GPU ikinci instance'tan itibaren her şeyi yanlış okur. `offsetof` MSL'de
// yok, `__builtin_offsetof` var.
static_assert(sizeof(Instance) == 32, "Instance stride 32 olmalı");
static_assert(__builtin_offsetof(Instance, size) == 8, "size@8");
static_assert(__builtin_offsetof(Instance, rgba) == 16, "rgba@16");

struct Out {
    float4 position [[position]];
    // Renk instance boyunca sabit; `flat` rasterizer'ın fragment başına
    // interpolasyonunu kaldırır.
    float4 rgba [[flat]];
};

// inst `device`: instance_id ile ıraksak indeksleniyor ve grid'le büyüyor
// (200×60'ta 384 KB). `constant` küçük ve tekdüze okunan veri içindir, boyut
// sınırı vardır. viewport_px gerçekten tekdüze, o `constant` kalıyor.
vertex Out cell_bg_vertex(uint vid [[vertex_id]],
                          uint iid [[instance_id]],
                          device const Instance* inst [[buffer(0)]],
                          constant float2& viewport_px [[buffer(1)]]) {
    Instance it = inst[iid];
    // vid 0..3 → (0,0) (1,0) (0,1) (1,1); triangle strip ikisini birim kareye
    // kapatır. Vertex buffer'da köşe verisi yok, dörtlü buradan türer.
    float2 corner = float2(vid & 1, vid >> 1);
    float2 ndc = (it.pos + corner * it.size) / viewport_px * 2.0 - 1.0;
    Out o;
    // Piksel uzayı sol-üst başlangıçlı, NDC sol-alt: y ters çevrilir.
    o.position = float4(ndc.x, -ndc.y, 0.0, 1.0);
    o.rgba = it.rgba;
    return o;
}

fragment float4 cell_bg_fragment(Out in [[stage_in]]) {
    return in.rgba;
}
