#include <metal_stdlib>
using namespace metal;

// Dock'un yazım efektleri (030): gelen glyph'in belirmesi, silinenin
// hayaleti. Kararların gerekçesi
// `.tasks/030-dock-yazim-animasyonlari/discussion.md` → Karar 5 ve 6.
//
// **Geometri üretilmiyor, ters dönüşüm var.** Dörtlü hücreden efekt payı
// kadar şişiyor (FX_PAD) ve fragment kendi noktasını efektin TERS
// dönüşümüyle glyph uzayına çeviriyor: küçülen bir glyph'in pikseli büyüyen
// bir noktadan örnekleniyor. Sonuç yuvanın dışına düşerse örnekleme YOK —
// komşu yuva başka bir glyph ve yuvalar arasında pay yok.

// Rust karşılığı: bt_gpu::frame::FxInstance,
// #[repr(C)] { pos: [f32; 2], uv0: [f32; 2], rgba: [f32; 4], fx: [f32; 4] }.
//
// İlk üç alan `GlyphInstance` (cell.metal) ile aynı yerde; `fx` efektin
// parametreleri: x = ilerleme t, y = kimlik | düzlem << 5 | yarı << 6 (TAM
// SAYI, f32 olarak — bit kalıbı değil), z = tohum, w = yedek. Düzen dolgusuz:
// float2 8, float4 16 hizalı → pos@0, uv0@8, rgba@16, fx@32, stride 48.
struct FxInstance {
    float2 pos;   // hücrenin sol üst köşesi, piksel (dock-yerel)
    float2 uv0;   // atlastaki yuvanın sol üst köşesi, normalize
    float4 rgba;  // lineer ön plan; cell.metal'deki uyarı burada da geçerli
    float4 fx;
};

static_assert(sizeof(FxInstance) == 48, "FxInstance stride 48 olmalı");
static_assert(__builtin_offsetof(FxInstance, uv0) == 8, "uv0@8");
static_assert(__builtin_offsetof(FxInstance, rgba) == 16, "rgba@16");
static_assert(__builtin_offsetof(FxInstance, fx) == 32, "fx@32");

// cell.metal'deki `CursorBlock`'un aynısı: her .metal ayrı derleniyor ve
// her kopya KENDİ assert'iyle Rust'a bağlı.
struct CursorBlock {
    float4 rect;
    float4 rgba;
};

static_assert(sizeof(CursorBlock) == 32, "CursorBlock 32 bayt olmalı");
static_assert(__builtin_offsetof(CursorBlock, rgba) == 16, "rgba@16");

// Efekt kimlikleri — Rust karşılığı `glyph_fx::KeypressFx::id` /
// `EraseFx::id`. Gelişler 1..16, hayaletler 16..32: girdinin türü
// kimlikten okunuyor.
constant uint FX_FADE = 1;
constant uint FX_RECEDE = 16;
constant uint FX_GHOST_FIRST = 16;

// Dörtlünün her yana şişme payı, hücre ölçüsü cinsinden. Bugünkü iki efekt
// glyph'i hücresinin dışına taşırmıyor; pay, hücrenin dışına çıkan efektlerin
// (sonraki phase'ler) ve iki hücrelik geniş glyph'in öteki yarısının yeri.
constant float FX_PAD = 1.0;

// `recede`'in vardığı ölçek — tasarım sabiti. Glyph merkezine doğru bu
// orana küçülürken söner; sıfıra inseydi son karelerde bir noktaya büzülen
// mürekkep "çekildi" değil "yutuldu" okunurdu.
constant float RECEDE_SCALE = 0.6;

struct FxOut {
    float4 position [[position]];
    // Hücrenin sol üst köşesine göre piksel; interpolasyon İSTER. `uv`
    // taşınmıyor: örnekleme noktası ters dönüşümden sonra belli oluyor.
    float2 local;
    float2 uv0 [[flat]];
    float4 rgba [[flat]];
    float4 fx [[flat]];
};

vertex FxOut glyph_fx_vertex(uint vid [[vertex_id]],
                             uint iid [[instance_id]],
                             device const FxInstance* inst [[buffer(0)]],
                             constant float2& viewport_px [[buffer(1)]],
                             constant float2& cell_px [[buffer(2)]]) {
    FxInstance it = inst[iid];
    float2 corner = float2(vid & 1, vid >> 1);
    float2 pad = cell_px * FX_PAD;
    float2 local = -pad + corner * (cell_px + 2.0 * pad);
    float2 ndc = (it.pos + local) / viewport_px * 2.0 - 1.0;
    FxOut o;
    o.position = float4(ndc.x, -ndc.y, 0.0, 1.0);
    o.local = local;
    o.uv0 = it.uv0;
    o.rgba = it.rgba;
    o.fx = it.fx;
    return o;
}

// Çıkışta yavaşlayan kübik — Karar 6'nın tek eğrisi.
static float ease_out(float t) {
    float u = 1.0 - t;
    return 1.0 - u * u * u;
}

fragment float4 glyph_fx_fragment(FxOut in [[stage_in]],
                                  texture2d<float> mask [[texture(0)]],
                                  texture2d<float> color [[texture(1)]],
                                  constant CursorBlock& cursor [[buffer(0)]],
                                  constant float2& cell_px [[buffer(1)]],
                                  constant float2& uv_size [[buffer(2)]]) {
    uint packed = uint(round(in.fx.y));
    uint id = packed & 31u;
    bool colored = ((packed >> 5) & 1u) != 0u;
    uint half_ = (packed >> 6) & 3u;
    float t = saturate(in.fx.x);
    bool ghost = id >= FX_GHOST_FIRST;

    // Hayalet `t = 1`'de tam saydam: son efekt karesinden sonra zemin düz.
    if (ghost && t >= 1.0) {
        return float4(0.0);
    }

    // Dönüşümün merkezi GLYPH'in kutusu, yarının hücresi değil: geniş glyph
    // iki yuvaya bölünmüş olsa da tek kutu olarak dönüşmeli, yoksa `recede`
    // bir emojiyi ortasından ikiye ayırırdı. 0 = tek hücre, 1 = sol yarı
    // (kutu sağa uzanıyor), 2 = sağ yarı (kutu sola).
    float cx = half_ == 0u ? cell_px.x * 0.5 : (half_ == 1u ? cell_px.x : 0.0);
    float2 center = float2(cx, cell_px.y * 0.5);

    float2 g = in.local;
    float alpha = 1.0;
    // **`t = 1` dalı statik yolun aritmetiğine iniyor**: geliş yerine
    // oturduğunda ters dönüşüm hiç koşmuyor, yani `g` statik yoldaki
    // interpolasyonun ta kendisi ve son efekt karesinden statik çizime
    // devirde piksel sıçramıyor (`plan.md` → R5).
    if (t < 1.0) {
        float e = ease_out(t);
        if (id == FX_FADE) {
            alpha = e;
        } else if (id == FX_RECEDE) {
            float s = mix(1.0, RECEDE_SCALE, e);
            g = center + (in.local - center) / s;
            alpha = 1.0 - e;
        }
    }

    // **Yuvanın dışı örneklenmiyor.** Sınır yarı açık, statik yolun
    // fragment merkezleriyle aynı: `g` hücrenin içindeyse texel merkezine
    // düşüyor.
    if (any(g < 0.0) || any(g >= cell_px)) {
        return float4(0.0);
    }
    float2 uv = in.uv0 + g / cell_px * uv_size;
    // `nearest`: cell.metal'deki gerekçe (yuvalar arasında pay yok).
    constexpr sampler s(coord::normalized, filter::nearest, address::clamp_to_edge);

    if (colored) {
        // Renk düzlemi: `emoji_fragment`'in aynısı — renk dokudan, imleç
        // uniform'u okunmuyor.
        float4 c = color.sample(s, uv);
        return float4(c.rgb, c.a * alpha);
    }
    float coverage = mask.sample(s, uv).r;
    // İmleç bloğunun altındaki metin: `cell_fragment`'in karışımının aynısı
    // (hayalet de caret'in altında silinebiliyor ve blok onun üstünde).
    float2 p = in.position.xy;
    bool inside = all(p >= cursor.rect.xy) && all(p < cursor.rect.zw);
    float3 rgb = mix(in.rgba.rgb, cursor.rgba.rgb, inside ? cursor.rgba.a : 0.0);
    return float4(rgb, in.rgba.a * coverage * alpha);
}
