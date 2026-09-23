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

// Efekt kimlikleri — Rust karşılığı `glyph_fx::Effect::id` (`Keypress` /
// `Erase`). Gelişler 1..16, hayaletler 16..32: girdinin türü kimlikten
// okunuyor. Gelişlerin sırası `discussion.md` → Karar 6'nın tablosu.
constant uint FX_FADE = 1;
constant uint FX_RISE = 2;
constant uint FX_POP = 3;
constant uint FX_EXTRUDE = 4;
constant uint FX_HEAT = 5;
constant uint FX_ECHO = 6;
constant uint FX_DROP = 7;
constant uint FX_INK = 8;
constant uint FX_SQUEEZE = 9;
constant uint FX_RECEDE = 16;
constant uint FX_GHOST_FIRST = 16;

// Dörtlünün her yana şişme payı, hücre ölçüsü cinsinden. En geniş taşmayı
// `echo`'nun kopyası yapıyor: geniş glyph'in iki hücrelik kutusu ECHO_SCALE
// kadar büyüyünce her yarının dörtlüsü kendi hücresinden (ECHO_SCALE - 1)
// hücre taşıyor, yani pay ≥ 0.8. Pay aynı zamanda geniş glyph'in öteki
// yarısının yeri: sol yarının dörtlüsü sağ yarının hücresini de örtmeli ki
// kutunun merkezine göre dönüşen mürekkep oraya düşebilsin.
constant float FX_PAD = 1.0;

// **Genlikler tasarım sabiti**, ölçülmüş değil ve hepsi hücre oranında —
// punto büyüyünce birlikte büyüyor. Offscreen karelerde gözle seçildi
// (`phase-4.md` → Uygulama Notları).

// `rise`: glyph'in doğduğu yer, hücre yüksekliğinin bu kadarı aşağısı.
// "Biraz altından": taban çizgisinin altına inen bir harf bir satır aşağıdan
// geliyor gibi okunurdu.
constant float RISE_DISTANCE = 0.3;

// `pop`: doğduğu ölçek ve yaylanmanın sertliği. Sertlik kapalı formun
// (`ease_out_back`) tek sabiti; 2.2 tepe ölçeği ≈ 1.08 veriyor — "bir an
// biraz büyür", sıçrar değil.
constant float POP_START = 0.5;
constant float POP_BACK = 2.2;

// `extrude`: doğduğu yatay ölçek. Sıfır değil, çünkü sıfır ölçekte ters
// dönüşüm tanımsız; bu kadar ince bir şerit ilk karede zaten görünmüyor.
constant float EXTRUDE_START = 0.05;

// `echo`: kopyanın vardığı ölçek ve başladığı saydamlık. Kopya "soluk" —
// glyph'in kendisiyle yarışmamalı.
constant float ECHO_SCALE = 1.8;
constant float ECHO_ALPHA = 0.4;

// `drop`: glyph'in düştüğü yükseklik (hücre yüksekliğinin oranı) ve sekişin
// sertliği (`ease_out_back`; 1.7 hedefi yüksekliğin ~%10'u kadar aşıyor —
// "hafifçe seker").
constant float DROP_HEIGHT = 0.45;
constant float DROP_BACK = 1.7;

// `ink`: mürekkebin ön cephesinin yumuşaklığı, "derinlik" biriminde (0..1).
// Keskin bir eşik kenarda merdiven, çok yumuşağı düz bir `fade` olurdu.
constant float INK_SOFTNESS = 0.35;

// `squeeze`: doğduğu oran (yatayda dar, dikeyde uzun) ve esnemenin
// sertliği — `pop`'la aynı kapalı form, daha yumuşak.
constant float SQUEEZE_X = 0.55;
constant float SQUEEZE_Y = 1.3;
constant float SQUEEZE_BACK = 1.5;

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

// Hedefi bir an aşıp geri dönen eğri — `pop`, `drop` ve `squeeze`'in
// "kapalı formu" (Karar 6). 0'da 0, 1'de 1; `back` aşmanın sertliği, tepe
// 1 + 4·back³ / (27·(back + 1)²).
static float ease_out_back(float t, float back) {
    float u = t - 1.0;
    return 1.0 + (back + 1.0) * u * u * u + back * u * u;
}

// Glyph'in bir noktadaki boyası: maske düzleminde ön plan rengi ve kapsama,
// renk düzleminde dokunun kendisi. **Yuvanın dışı örneklenmiyor** ve kural
// tek yerde: her örnekleme (ana glyph, `echo`'nun kopyası) buradan geçiyor.
// Sınır yarı açık, statik yolun fragment merkezleriyle aynı: `g` hücrenin
// içindeyse texel merkezine düşüyor.
static float4 paint(float2 g, float2 uv0, float3 fg, bool colored,
                    texture2d<float> mask, texture2d<float> color,
                    float2 cell_px, float2 uv_size) {
    if (any(g < 0.0) || any(g >= cell_px)) {
        return float4(0.0);
    }
    float2 uv = uv0 + g / cell_px * uv_size;
    // `nearest`: cell.metal'deki gerekçe (yuvalar arasında pay yok).
    constexpr sampler s(coord::normalized, filter::nearest, address::clamp_to_edge);
    if (colored) {
        // Renk düzlemi: `emoji_fragment`'in aynısı — renk dokudan.
        return color.sample(s, uv);
    }
    return float4(fg, mask.sample(s, uv).r);
}

// `ink`'in "derinliği": noktanın 3×3 komşuluğunun ortalama kapsaması. Çizginin
// çekirdeği (her yanı mürekkep) 1'e, kenarı 0'a yakın — tek texel'in kapsaması
// ince fontta çoğu pikselde kısmi, yani çekirdeği kenardan ayıramazdı.
// Komşular hücreye **kıstırılıyor**: yuvanın dışı burada da örneklenmiyor.
static float ink_depth(float2 g, float2 uv0, texture2d<float> mask,
                       float2 cell_px, float2 uv_size) {
    constexpr sampler s(coord::normalized, filter::nearest, address::clamp_to_edge);
    float sum = 0.0;
    for (int dy = -1; dy <= 1; dy++) {
        for (int dx = -1; dx <= 1; dx++) {
            float2 q = clamp(g + float2(dx, dy), float2(0.5), cell_px - 0.5);
            sum += mask.sample(s, uv0 + q / cell_px * uv_size).r;
        }
    }
    return sum / 9.0;
}

fragment float4 glyph_fx_fragment(FxOut in [[stage_in]],
                                  texture2d<float> mask [[texture(0)]],
                                  texture2d<float> color [[texture(1)]],
                                  constant CursorBlock& cursor [[buffer(0)]],
                                  constant float2& cell_px [[buffer(1)]],
                                  constant float2& uv_size [[buffer(2)]],
                                  constant float4& heat [[buffer(3)]]) {
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
    // (kutu sağa uzanıyor), 2 = sağ yarı (kutu sola). `extrude`'un çıpası da
    // kutunun sol kenarı, yarınınki değil.
    float cx = half_ == 0u ? cell_px.x * 0.5 : (half_ == 1u ? cell_px.x : 0.0);
    float2 center = float2(cx, cell_px.y * 0.5);
    float box_left = half_ == 2u ? -cell_px.x : 0.0;

    float2 g = in.local;
    float alpha = 1.0;
    // Maske düzleminde ön planın rengi; `heat` onu değiştiriyor.
    float3 fg = in.rgba.rgb;
    // `echo`'nun kopyası: ikinci örnekleme noktası ve saydamlığı (0 = yok).
    float2 echo_g = float2(0.0);
    float echo_alpha = 0.0;
    // `ink`'in eşiği; 0'da bütün mürekkep görünüyor.
    float ink_front = 0.0;
    // **`t = 1` dalı statik yolun aritmetiğine iniyor**: geliş yerine
    // oturduğunda ters dönüşüm hiç koşmuyor, yani `g` statik yoldaki
    // interpolasyonun ta kendisi ve son efekt karesinden statik çizime
    // devirde piksel sıçramıyor (`plan.md` → R5). Her dal `t → 1`'de de
    // özdeşliğe yaklaşıyor: son efekt karesi statik glyph'ten ancak eğrinin
    // kalanı kadar ayrışıyor.
    if (t < 1.0) {
        float e = ease_out(t);
        if (id == FX_FADE) {
            alpha = e;
        } else if (id == FX_RISE) {
            g.y -= RISE_DISTANCE * cell_px.y * (1.0 - e);
            alpha = e;
        } else if (id == FX_POP) {
            float s = mix(POP_START, 1.0, ease_out_back(t, POP_BACK));
            g = center + (in.local - center) / s;
            alpha = e;
        } else if (id == FX_EXTRUDE) {
            float s = mix(EXTRUDE_START, 1.0, e);
            g.x = box_left + (in.local.x - box_left) / s;
            // Belirme de var: ilk karenin ince şeridi `nearest` örneklemede
            // kesik kesik bir çizgi olarak görünüyordu.
            alpha = e;
        } else if (id == FX_HEAT) {
            // Soğuma Karar 6'nın eğrisiyle değil `smoothstep`'le: çıkışta
            // yavaşlayan kübik rengi ilk çeyrekte büyük ölçüde soğutuyordu ve
            // kızgın renk tek bir karede kalıyordu. `smoothstep` başta yavaş —
            // renk görünür kalıyor — ve sonda da yavaş, yani kendi rengine
            // eğimsiz varıyor.
            fg = mix(heat.rgb, in.rgba.rgb, smoothstep(0.0, 1.0, t));
            // Renk düzleminde `fg` okunmuyor (renk dokudan, `emoji_fragment`'in
            // kuralı): emoji boyanmıyor, `ink`'teki gibi düz belirme.
            if (colored) {
                alpha = e;
            }
        } else if (id == FX_ECHO) {
            alpha = e;
            float s = mix(1.0, ECHO_SCALE, e);
            echo_g = center + (in.local - center) / s;
            echo_alpha = ECHO_ALPHA * (1.0 - e);
        } else if (id == FX_DROP) {
            g.y += DROP_HEIGHT * cell_px.y * (1.0 - ease_out_back(t, DROP_BACK));
            alpha = e;
        } else if (id == FX_INK) {
            // Renk düzleminde eşik yok (emojinin "derinliği" kapsamadan
            // okunamıyor, kenarı çoğu zaman tam opak): düz belirme.
            if (colored) {
                alpha = e;
            } else {
                // Cephe zamanda doğrusal: çıkışta yavaşlayan eğriyle yayılma
                // ilk karede bitiyordu ve "dolma" hiç görülmüyordu.
                ink_front = 1.0 - t;
            }
        } else if (id == FX_SQUEEZE) {
            float b = ease_out_back(t, SQUEEZE_BACK);
            float2 s = float2(mix(SQUEEZE_X, 1.0, b), mix(SQUEEZE_Y, 1.0, b));
            g = center + (in.local - center) / s;
            alpha = e;
        } else if (id == FX_RECEDE) {
            float s = mix(1.0, RECEDE_SCALE, e);
            g = center + (in.local - center) / s;
            alpha = 1.0 - e;
        }
    }

    float4 c = paint(g, in.uv0, fg, colored, mask, color, cell_px, uv_size);
    if (ink_front > 0.0) {
        float depth = ink_depth(g, in.uv0, mask, cell_px, uv_size);
        c.a *= smoothstep(ink_front - INK_SOFTNESS, ink_front, depth);
    }
    c.a *= alpha;
    if (echo_alpha > 0.0) {
        // Kopya glyph'in ALTINDA: "üstünden dağılan" halka glyph'i örtmemeli.
        // Ön çarpımlı birleşim, sonra geri bölme — blend `SourceAlpha`.
        float4 copy = paint(echo_g, in.uv0, fg, colored, mask, color, cell_px, uv_size);
        copy.a *= echo_alpha;
        float a = c.a + copy.a * (1.0 - c.a);
        float3 rgb = c.rgb * c.a + copy.rgb * copy.a * (1.0 - c.a);
        c = a > 0.0 ? float4(rgb / a, a) : float4(0.0);
    }

    if (colored) {
        // İmleç uniform'u renk düzleminde okunmuyor (`emoji_fragment`).
        return c;
    }
    // İmleç bloğunun altındaki metin: `cell_fragment`'in karışımının aynısı
    // (hayalet de caret'in altında silinebiliyor ve blok onun üstünde).
    // `heat`'in rengi de bloğun altında imlecin metin rengine dönüyor:
    // kızgın renk imlecin kendi rengi ve blokla aynı renkte bir harf
    // görünmezdi.
    float2 p = in.position.xy;
    bool inside = all(p >= cursor.rect.xy) && all(p < cursor.rect.zw);
    float3 rgb = mix(c.rgb, cursor.rgba.rgb, inside ? cursor.rgba.a : 0.0);
    return float4(rgb, in.rgba.a * c.a);
}
