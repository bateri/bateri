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

// ---------------------------------------------------------------------------
// Caret'in kendi fragment'i
// ---------------------------------------------------------------------------
//
// Vertex `cell_bg_vertex`'in TA KENDİSİ ve `Instance` aynen kullanılıyor:
// caret kare başına tek quad, köşeleri yine `vertex_id`'den türüyor. Ayrılan
// yalnız fragment, yani üçüncü pipeline ikinci bir vertex yolu doğurmuyor.
//
// İki uniform da ÇIPLAK `float4`, struct DEĞİL. Gerekçe hizalama: Rust'ta
// `[f32; 4]` 4, MSL'de `float4` 16 hizalı; ikisi bir struct'ın içinde
// buluşunca stride sessizce ayrışır (`CursorBlock`'un 32 baytı bu yüzden iki
// tarafta da assert'li). Tek başına argüman olarak ikisi de 16 bayt ve ofset
// 0 — tuzak hiç doğmuyor ve yeni bir #[repr(C)] ↔ .metal çifti gerekmiyor.
//
//   core  = BOYANAN dikdörtgen (x0, y0, x1, y1), PENCERE uzayı. `[[position]]`
//           viewport dönüşümünden SONRAKİ koordinat ve `core` da öyle yazılıyor
//           (bt_gpu::frame::Frame::caret_core); ikisi aynı uzayda olmasaydı
//           hale ötelenmiş bir karede yanlış yerde çizilirdi.
//   shape = (köşe yarıçapı, kenar kalınlığı, hale payı, halenin tepe alfası)
//
// Quad `core`'dan hale payı kadar BÜYÜK geliyor (`Caret::instance` şişiriyor);
// hale tam o farkın içinde yaşıyor. Şişmeyi fragment yeniden hesaplamıyor —
// ölçüsü zaten `shape.z`.

// Yuvarlak dikdörtgenin imzalı mesafesi; negatif = iç, sıfır = kenar.
// `p` merkeze göre, `half` yarım ölçü.
//
// Buradaki `min` bir POLİTİKA DEĞİL matematik ön koşulu: SDF yarıçapın yarım
// ölçüyü aşmamasını istiyor. Yarıçapın DEĞERİNE karar veren taraf tek ve o
// Rust (bt_gpu::frame::caret_radius_px) — hücre ölçüsünü bilen taraf orası ve
// sınamalar da oradan okuyor. Buradaki kırpma yalnız bozuk bir uniform'a
// karşı savunma.
static inline float rounded_box_sdf(float2 p, float2 half_size, float radius) {
    float r = min(radius, min(half_size.x, half_size.y));
    float2 q = abs(p) - half_size + r;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}

fragment float4 caret_fragment(Out in [[stage_in]],
                               constant float4& core [[buffer(0)]],
                               constant float4& shape [[buffer(1)]]) {
    float radius = shape.x;
    float stroke = shape.y;
    float glow = shape.z;
    float glow_alpha = shape.w;

    float2 center = (core.xy + core.zw) * 0.5;
    float2 half_size = (core.zw - core.xy) * 0.5;
    float d = rounded_box_sdf(in.position.xy - center, half_size, radius);

    // **Dejenere kolda kenar SERT adım.** Yarıçap ve hale sıfırken çıktı bit
    // bit eski düz dörtgenle aynı olmak zorunda: `smoothstep` o kolda da
    // koşsaydı kenar pikselleri yarım alfa alır ve 014'ten kalma piksel
    // sınamaları paritenin kanıtı olmaktan çıkardı.
    bool degenerate = (radius == 0.0f && glow == 0.0f);
    float body = degenerate ? step(d, 0.0f) : 1.0f - smoothstep(-0.5f, 0.5f, d);
    if (stroke > 0.0f) {
        // İçi boş caret: yalnız kenar bandı. Bandın iç sınırı -stroke, yani
        // dikdörtgenin içinde `stroke` piksel. `stroke == 0` dolu demek ve
        // phase-3'e kadar tek koşan kol o.
        float inner = degenerate ? step(d, -stroke)
                                 : 1.0f - smoothstep(-stroke - 0.5f, -stroke + 0.5f, d);
        body -= inner;
    }

    // Hale yalnız DIŞARIDA: içeride gövde zaten opak ve ikisini toplamak
    // kenarı olduğundan parlak yapardı. Kenarda tepe alfa, hale payının
    // ucunda sıfır.
    float halo = 0.0f;
    if (glow > 0.0f) {
        halo = (1.0f - smoothstep(0.0f, glow, max(d, 0.0f))) * glow_alpha * step(0.0f, d);
    }

    // Caret'in kendi alfası (hareket × blink) HEPSİNİ çarpıyor: hale blink'le
    // birlikte sönüyor ve ikinci bir yol yazılmıyor (R6).
    return float4(in.rgba.rgb, in.rgba.a * max(body, halo));
}
