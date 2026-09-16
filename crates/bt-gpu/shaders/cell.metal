#include <metal_stdlib>
using namespace metal;

// Rust karşılığı: bt_gpu::frame::GlyphInstance,
// #[repr(C)] { pos: [f32; 2], uv0: [f32; 2], rgba: [f32; 4] }.
//
// `size` ve uv boyutu instance'ta YOK: bu sette her glyph tam bir hücre
// boyunda (sabit yuva ızgarası) ve ikisi de kare boyunca sabit, uniform
// olarak geliyorlar. Yan etkisi düzenin dolgusuz örtüşmesi: float2 8, float4
// 16 hizalı → pos@0, uv0@8, rgba@16, stride 32. Araya bir `float2 size`
// girseydi rgba 32'ye kayar, MSL 48 bayt eder ve Rust'ın 40'ıyla ayrışırdı.
struct GlyphInstance {
    float2 pos;   // hücrenin sol üst köşesi, piksel
    float2 uv0;   // atlastaki yuvanın sol üst köşesi, normalize
    // Lineer RGBA: hedef BGRA8Unorm_sRGB ve kodlamayı ROP yapıyor. Buraya
    // ya da fragment'e bir gamma düzeltmesi eklemek paleti İKİ KEZ kodlar.
    float4 rgba;
};

// cell_bg.metal ile aynı gerekçe: her iki taraf KENDİ assert'iyle bağlı.
// Buraya eklenen bir alan stride'ı kaydırır, Rust tarafı bunu göremez.
static_assert(sizeof(GlyphInstance) == 32, "GlyphInstance stride 32 olmalı");
static_assert(__builtin_offsetof(GlyphInstance, uv0) == 8, "uv0@8");
static_assert(__builtin_offsetof(GlyphInstance, rgba) == 16, "rgba@16");

// Rust karşılığı: bt_gpu::frame::CursorBlock,
// #[repr(C)] { rect: [f32; 4], rgba: [f32; 4] }.
//
// İmlecin piksel dikdörtgeni ve bloğun ALTINDA kalan metnin rengi. Kare
// boyunca tek değer, o yüzden instance değil uniform. Dikdörtgen min/max:
// fragment testi toplama yapmadan iki karşılaştırmaya iniyor. Görünmez imleç
// dejenere bir dikdörtgendir (hepsi sıfır) — ayrı bir bayrak yok, çünkü bayrak
// ile dikdörtgen ayrışabilen iki gerçek olurdu.
struct CursorBlock {
    float4 rect;  // x0, y0, x1, y1 — piksel, sol üst başlangıçlı
    // Lineer; rgb'nin kaynağı bt_core::Cursor::text. ALFA BİR RENK DEĞİL, bu
    // karedeki imleç opaklığı (bt_gpu::motion::Motion::alpha): Hareketi Azalt
    // açıkken imleç yeni hücresinde belirir ve bloğun alfasıyla AYNI değer
    // buraya da yazılır. Aşağıda karışım çarpanı olarak okunuyor.
    float4 rgba;
};

static_assert(sizeof(CursorBlock) == 32, "CursorBlock 32 bayt olmalı");
static_assert(__builtin_offsetof(CursorBlock, rgba) == 16, "rgba@16");

struct Out {
    float4 position [[position]];
    // uv interpolasyon İSTER: dörtlünün içinde atlas yuvasını tarıyor.
    float2 uv;
    // Renk instance boyunca sabit; `flat` fragment başına interpolasyonu kaldırır.
    float4 rgba [[flat]];
};

// inst `device`: cell_bg ile aynı gerekçe (instance_id ile ıraksak, grid'le
// büyür). Üç uniform `constant`: üçü de kare boyunca tekdüze.
vertex Out cell_vertex(uint vid [[vertex_id]],
                       uint iid [[instance_id]],
                       device const GlyphInstance* inst [[buffer(0)]],
                       constant float2& viewport_px [[buffer(1)]],
                       constant float2& cell_px [[buffer(2)]],
                       constant float2& uv_size [[buffer(3)]]) {
    GlyphInstance it = inst[iid];
    float2 corner = float2(vid & 1, vid >> 1);
    float2 ndc = (it.pos + corner * cell_px) / viewport_px * 2.0 - 1.0;
    Out o;
    // Piksel uzayı sol-üst başlangıçlı, NDC sol-alt: y ters çevrilir. Atlas
    // dokusunun kendi y'si de sol-üst başlangıçlı (replaceRegion satır
    // satır yazıyor), yani uv ters ÇEVRİLMEZ — ikisi aynı yönde.
    o.position = float4(ndc.x, -ndc.y, 0.0, 1.0);
    o.uv = it.uv0 + corner * uv_size;
    o.rgba = it.rgba;
    return o;
}

// Atlas R8Unorm: tek kanal kapsama (alfa). Renk instance'tan gelir, dokudan
// değil — atlas glyph başına bir maske tutuyor, bir görüntü değil.
fragment float4 cell_fragment(Out in [[stage_in]],
                              texture2d<float> atlas [[texture(0)]],
                              constant CursorBlock& cursor [[buffer(0)]]) {
    // `nearest`, `linear` DEĞİL. Birebir oturan olağan durumda ikisi aynı
    // sonucu verir (fragment merkezleri texel merkezlerine düşer). Ayrıştıkları
    // karede — ölçek değişimiyle bir sonraki geometri olayı arasında — fark
    // ortaya çıkıyor: dörtlü yuvadan genişse linear'ın son sütunu komşu
    // yuvanın ilk sütununu karıştırır. Yuvalar arasında pay yok ve
    // `clamp_to_edge` yalnız dokunun kenarında kırpıyor; üstelik Metal yeni
    // dokuyu sıfırlamıyor, yani komşu henüz yazılmamış olabilir. `nearest`
    // her zaman yuvanın içinde kalıyor: o karede glyph köşeli görünür, ama
    // rastgele kapsama okumaz.
    constexpr sampler s(coord::normalized, filter::nearest, address::clamp_to_edge);
    float coverage = atlas.sample(s, in.uv).r;
    // İmleç bloğunun altındaki metin (glyph VE kural çizgisi) rengini
    // uniform'dan alır: blok opak ve altındaki harf kendi ön planıyla kalsaydı
    // okunmazdı. Karar bt-core'un (bt_core::Cursor::text), burası yalnız "bu
    // fragment dikdörtgenin içinde mi" diye soruyor — ve bu soru PİKSEL
    // başına sorulduğu için blok iki hücre arasındayken (008) hücrenin yarısı
    // ezilir, yarısı kendi rengiyle kalır. `[[position]]` sol üst başlangıçlı
    // ve instance pozisyonlarıyla aynı uzayda.
    //
    // Sınır yarı açık: [x0, x1) — komşu hücrenin ilk sütunu bu bloğa ait
    // değil. `<=` bugün AYNI sonucu verir ve bunu sınayan bir bekçi yok:
    // `[[position]]` fragment **merkezini** veriyor (x + 0.5), yani hiçbir
    // fragment tam sınıra düşmüyor. Yarı açık yazılmasının sebebi gelecek —
    // hareket ara konumları dikdörtgeni yarım piksellere oturtacak ve orada
    // iki biçim ayrışır.
    float2 p = in.position.xy;
    bool inside = all(p >= cursor.rect.xy) && all(p < cursor.rect.zw);
    // Ezme değil KARIŞIM ve çarpanı uniform'un alfası: blok belirirken
    // (Hareketi Azalt) harf de onunla birlikte belirmeli, yoksa henüz
    // görünmeyen bir bloğun rengine boyanır — zeminin üstünde zemin renginde
    // bir harf. Belirme dışında alfa 1.0, yani karışım tam ezmeye iniyor ve
    // sonuç bu satırın eski hâliyle birebir aynı.
    //
    // Karışım YALNIZ RGB'de: çıkıştaki alfa aşağıda kapsamadan geliyor ve
    // imlecin opaklığı oraya da sızsaydı glyph'in kenarı sessizce
    // inceltilirdi. Bloğun kendi saydamlığını cell_bg pipeline'ı çiziyor.
    float3 rgb = mix(in.rgba.rgb, cursor.rgba.rgb, inside ? cursor.rgba.a : 0.0);
    // Ön çarpımsız: blend src_alpha/one_minus_src_alpha ile eşleşiyor.
    return float4(rgb, in.rgba.a * coverage);
}
