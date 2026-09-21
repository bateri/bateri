//! Tek glyph'i alfa baytlarına çizer.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_core_graphics::{CGBitmapContextCreate, CGContext, CGImageAlphaInfo};
use objc2_core_text::CTFont;

use crate::font::{self, Metrics};

/// [`draw`]'in sonucu.
///
/// İki başarısızlık ayrı varyant çünkü **teşhisleri** ayrı, davranışları değil:
/// ikisi de tofu'ya düşer ve ikisi de önbelleğe girer. `NoContext`'un
/// önbelleğe girmesi ilk bakışta yanlış görünür ("geçici hata") ama
/// `CGBitmapContextCreate`'in karakterle ilgili tek bir argümanı yok — hepsi
/// atlasın ömrü boyunca sabit, yani bir kez başarısızsa hep başarısız.
/// Önbelleğe **girmeseydi** her hücre her karede başarısız bir bağlam kurulumu
/// öderdi ve tek bir glyph bile çizilmezdi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DrawResult {
    Drawn,
    /// Fontun bu karakter için glyph'i yok (`.notdef`).
    NoGlyph,
    /// `CGBitmapContext` kurulamadı.
    NoContext,
}

/// `target`e `ch`'in kapsama (alfa) baytlarını çizer.
///
/// `cell_advance` hücrenin **kesirli** ilerlemesi ([`font::space_advance`]) ve
/// glyph'in yatay ortalanmasının tek girdisi; `m.cell_px.0` onun yukarı
/// yuvarlanmışıdır ve buraya girmez (gerekçe [`font::space_advance`]'in
/// doc'unda).
///
/// Tampon yalnız gerçekten çizim yapılacaksa sıfırlanır.
pub(crate) fn draw(
    font: &CTFont,
    ch: char,
    m: Metrics,
    cell_advance: CGFloat,
    target: &mut [u8],
) -> DrawResult {
    // `debug_assert` değil: bu satır aşağıdaki `unsafe` bloğun ön koşulu.
    // CG'ye `width`/`height` `m`'den, işaretçi `target`ten gidiyor; ikisi
    // ayrışırsa CG kısa tamponun ötesine yazar ve release derlemede hiçbir şey
    // fark etmez — `make hepsi` sınamaları debug koşuyor.
    assert_eq!(target.len(), m.slot_bytes(), "tampon tam bir yuva olmalı");

    let Some(glyph) = font::glyph_index(font, ch) else {
        return DrawResult::NoGlyph;
    };

    let (w, h) = m.cell_wh();
    // Alfa-only bağlam: renk uzayı **yok** (`space: None`), bileşen başına
    // 8 bit, satır adımı tam hücre genişliği. Beyaz çizilen glyph'in kapsama
    // değeri doğrudan alfa baytı olur; ayrı bir kanal ayıklama adımı doğmaz
    // ve tampon zaten atlasın `R8Unorm` düzeninde.
    // SAFETY: `target` w*h bayt ve bağlam yaşadığı sürece (bu fonksiyonun
    // sonuna kadar) canlı; ölçüler tamponla tutarlı. Bağlam düştükten sonra
    // `target`e yalnız Rust tarafından erişilir.
    let ctx = unsafe {
        CGBitmapContextCreate(
            target.as_mut_ptr().cast::<c_void>(),
            w,
            h,
            8,
            w,
            None,
            CGImageAlphaInfo::Only.0,
        )
    };
    let Some(ctx) = ctx else {
        return DrawResult::NoContext;
    };
    // Sıfırlama bağlam kurulduktan **sonra**: başarısız iki dalda çağıran
    // tampona hiç bakmıyor (tofu rezident ve dokuda), yani oradaki memset
    // tamamen boşa giderdi.
    target.fill(0);

    CGContext::set_should_antialias(Some(&ctx), true);
    // Subpixel AA kapalı: atlas tek kanal ve macOS 10.14'ten beri sistemin
    // kendisi de subpixel'i bıraktı (discussion.md → karar 3a). İki çağrı
    // ayrı ayrı gerekli: `allows_font_smoothing` bağlamın iznini, `should`
    // o çizimdeki tercihi kapatıyor.
    CGContext::set_allows_font_smoothing(Some(&ctx), false);
    CGContext::set_should_smooth_fonts(Some(&ctx), false);
    // Alfa-only bağlamda gri bileşen yok sayılır; anlamı olan alfa.
    CGContext::set_gray_fill_color(Some(&ctx), 1.0, 1.0);

    // CG'nin başlangıcı sol **alt**, bizim ızgaramız sol üst: taban çizgisi
    // hücrenin altından `cell_h - baseline_px` kadar yukarıda. Çıkarma taşmaz:
    // `font::metrics` yüksekliği taban + (descent+leading) olarak kuruyor ve
    // ikinci parça en az 1.
    let baseline = CGFloat::from(m.cell_px.1 - m.baseline_px);
    // Glyph hücrede **yatay olarak ortalanıyor**: yedek fontun ilerlemesi
    // hücrenin ilerlemesinden dar olabiliyor ve sola yapışmış bir işaret
    // komşularının arasında hizasız görünür. Kural **evrensel**, yedeğe
    // koşullu değil — eşaralıklı taban fontta her glyph'in ilerlemesi
    // hücrenin ilerlemesinin ta kendisi, yani çıkarma tam olarak sıfır ve
    // taban fontun rasteri bit bit aynı kalıyor (bekçisi
    // `every_base_glyph_advance_is_the_cell_advance`). Koşullu yazılsaydı
    // "yedek mi" sorusu çizim yoluna ikinci bir dal, sınamaya da ikinci bir
    // kod yolu eklerdi.
    //
    // `max(0.0)`: genişlik kapısı yalnız **yedekte** koşuyor, taban font
    // eşaralıklı olmayabilir ([`font::FontIssue::NotMonospaced`]) ve geniş bir
    // glyph'i hücreyi aşabilir. Komşu hücreye taşma **mümkün değil** — bağlam
    // tam bir yuva genişliğinde ve CG oraya kırpıyor — yani mesele taşma değil
    // kırpmanın **yönü**: negatif kaydırma glyph'in solunu keser, kırpma ise
    // sağdan olmalı. Latin yazıda harf soldan tanınıyor; sol kenarı kesilmiş
    // bir 'W' ile 'V' ayırt edilemez.
    let x = ((cell_advance - font::glyph_advance(font, glyph)) / 2.0).max(0.0);
    let position = CGPoint::new(x, baseline);
    // SAFETY: tek glyph, tek konum, sayı ikisiyle tutarlı; bağlam canlı.
    unsafe { font.draw_glyphs(NonNull::from(&glyph), NonNull::from(&position), 1, &ctx) };
    DrawResult::Drawn
}

/// Kural çizgisi çeşidi — atlasta karakter gibi yuva tutar.
///
/// Yüzden bağımsız: kalın metnin altındaki çizgi kalın değildir. Çağıran
/// bunları her zaman [`crate::Face::Regular`] ile sorar ve `Atlas::slot` bunu
/// ayrıca normalize ediyor.
// `repr(u8)`: türetilen `Hash` discriminant'ı varsayılan olarak `isize`
// yazıyor — 8 bayt. Anahtar `slot()`'un sıcak yolunda ve hash'e giren her
// bayt kare başına hücre başına ödeniyor.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleKind {
    Single,
    Double,
    Curl,
    Dotted,
    Dashed,
    Strike,
    /// Prompt işareti: `>` yerine geçen chevron.
    ///
    /// **Bir kural çizgisi değil ama aynı aileden** ve burada olmasının sebebi
    /// mekanizma: bu enum yordamsal çizilen sprite'ların **kendi kod noktası
    /// olmayan** kümesi — fonttan gelmiyor, yüzden bağımsız
    /// ([`crate::Face::Regular`]'a çivili) ve atlasta kendi payını tutuyor.
    /// Beşi alt çizgi, biri üstü çizili, biri de bu.
    ///
    /// Yordamsal çizimin **ikinci** kümesi karakterler ([`is_procedural`]) ve
    /// o küme buraya girmiyor: `Sprite::Char` olarak yaşıyor, yuva payı
    /// ([`crate::RULE_RESERVE`]) almıyor ve `bt-gpu` onu sıradan bir harften
    /// ayırt etmiyor. Ayıran şey "kim çiziyor" değil "adı var mı": kural
    /// çizgisi bir SGR biçimi, blok bir karakter.
    ///
    /// İki küme aynı mürekkebi paylaşsa da **sınırları ayrı**: bu enum
    /// kapalı ve yedi üyeli, öteki 413 karakter ve kod noktası aralıklarıyla
    /// tanımlı. Bir çizgi çizim karakteri ([`Family::Line`]) buraya
    /// eklenemez — `Single`'ın altı da bir `─` çiziyor ama o alt çizgi,
    /// konumu fontun `underline` metriğinde; `─` hücrenin ortasında ve
    /// komşusuyla döşemek zorunda.
    ///
    /// Fonttan bir `>` **almıyoruz** ve sebebi ürün kararı: işaret terminalin
    /// kendi işareti, kullanıcının fontunun değil. Font değişince prompt'un
    /// şekli değişmemeli (012 phase-9, kullanıcı: "bunu daha hoş kendin
    /// çizebilir misin").
    Chevron,
}

/// Bir hücreye sığan tam dalga sayısı.
///
/// Periyot `cell_px.0 / WAVE_COUNT` ve bu bölmenin **tam** olması şart:
/// sprite tek hücre genişliğinde ve komşularıyla döşeniyor, yani periyot
/// hücreyi tam bölmezse iki hücrenin sınırında faz kırılır ve çok hücreli bir
/// alt çizgi kesintili görünür. `1` bu kısıtı inşaen sağlıyor (periyot =
/// hücre genişliği) ve en yumuşak dalgayı veriyor; büyütülecekse `cell_px.0`'ı
/// bölen bir değer seçilmeli.
const WAVE_COUNT: f32 = 1.0;

/// Kıvrımın dikey kaplamı, kalınlığın katı olarak.
///
/// Dalganın göz tarafından dalga olarak görülmesi için gereken en küçük
/// kaplam. Tabanı alt çizginin tabanına çakılı ve o zaten hücrenin içinde
/// (`font::rule_envelope`), yani kıvrım inşaen içeride.
const CURL_FACTOR: f32 = 3.0;

/// `target`e kural çizgisinin kapsama baytlarını çizer.
///
/// [`draw`]'in kardeşi ama **CG kullanmıyor**: `tofu_buffer` gibi doğrudan
/// bayt yazıyor. Üç kazanç — çizim deterministik (CG'nin antialias sürümüne
/// bağlı değil, sınama tam yapı assert edebilir), başarısızlık dalı hiç
/// doğmuyor (`NoContext` yok, dönüş `()`), ve font hiç sorulmuyor.
pub(crate) fn draw_rule(kind: RuleKind, m: Metrics, target: &mut [u8]) {
    // audit: `draw`'inkiyle aynı ön koşul, aynı gerekçe. Tampon `m`'den
    // boyutlandırılmış `self.buffer`, yani ayrışma yapısal olarak imkânsız;
    // assert onu sınırda ve adıyla yakalıyor, `band`/`curl`'in döngüsünde
    // anlamsız bir indeks paniği olarak değil.
    assert_eq!(target.len(), m.slot_bytes(), "tampon tam bir yuva olmalı");
    // Tampon paylaşılıyor ve içinde bir önceki glyph'in pikselleri var;
    // sıfırlanmazsa kural çizgisinin altından o glyph görünür.
    target.fill(0);

    let (position, thickness) = match kind {
        RuleKind::Strike => m.strikeout_px,
        _ => m.underline_px,
    };
    // Desen periyodu tam sayı aritmetiğinde kalıyor: `as usize` turu doğmuyor.
    let thick = usize::from(thickness);
    let (w, h) = m.cell_wh();
    let (position, thickness) = (f32::from(position), f32::from(thickness));

    match kind {
        // Kesintisiz desen: periyot 1, dolu 1.
        RuleKind::Single | RuleKind::Strike => band(target, m, position, thickness, 1, 1),
        RuleKind::Double => {
            band(target, m, position, thickness, 1, 1);
            // İkinci çizgi **önce aşağıya**. Alt çizgi ile hücre tabanı
            // arasındaki satırlar boş (13pt: çizgi 14, hücre 17 → 15-16 boş)
            // ve orası glyph gövdesinden uzak. Yukarı taşımak `a e o` gibi
            // harflerin son gövde satırına girer ve iki çizgi ayrı görünmek
            // yerine harflerin dibine yapışık tek kalın çizgi gibi okunur.
            // Aşağıda yer yoksa yukarı düşülür.
            let below = position + 2.0 * thickness;
            let second = if below + thickness <= h as f32 {
                below
            } else {
                (position - 2.0 * thickness).max(0.0)
            };
            band(target, m, second, thickness, 1, 1);
        }
        // Nokta ve kesik: periyot kalınlığa bağlı, yani punto büyüdükçe desen
        // de büyüyor ve @2x'te sıkışmış görünmüyor. Alt sınır gerekmiyor —
        // `font::rule_envelope` kalınlığı zaten `>= 1`'e bağlıyor.
        RuleKind::Dotted => {
            let p = dividing_period(2 * thick, w);
            band(target, m, position, thickness, p, (p / 2).max(1));
        }
        RuleKind::Dashed => {
            let p = dividing_period(6 * thick, w);
            band(target, m, position, thickness, p, (p * 2 / 3).max(1));
        }
        RuleKind::Curl => curl(target, m, position, thickness),
        RuleKind::Chevron => chevron(target, m),
    }
}

/// Prompt işareti: iki kolu ortada birleşen bir chevron.
///
/// **Dikey merkezi üstü çizili metriğinden.** Yeni bir sayı uydurmaya gerek
/// yok: üstü çizili çizgisi tam da x-height'ın ortasında duruyor, yani
/// küçük harflerin optik merkezi. İşaret oraya oturunca metinle aynı hizada
/// okunuyor; hücrenin geometrik merkezi taban çizgisinin altına düşer ve
/// işaret metne göre alçak görünürdü.
///
/// **Yüksekliği x-height, genişliği onun yarısı.** İlki de türetilmiş: üstü
/// çizili merkezi ile taban çizgisi arasındaki mesafe x-height'ın yarısı, yani
/// kolların dikey açıklığı doğrudan fontun kendi ölçüsünden geliyor. Oran
/// 1:2 chevron'un olağan tipografik oranı ve tek bir sayı — ikinci bir
/// tasarım sabiti doğmuyor.
///
/// **Kalınlık alt çizginin kalınlığı.** İkinci bir kalınlık sayısı iki kaynak
/// olurdu ve punto/ölçek değişiminde ayrışırdı.
///
/// Ink yatayda hücrenin ortasına toplanıyor ve genişliği hücrenin yarısını
/// aşmıyor: işaret ızgarada **sol payın içinde** çiziliyor
/// (`bt_gpu::Frame::push_block`) ve pay bir hücreden dar olabilir. Taşsaydı
/// komut metninin ilk harfine binerdi.
fn chevron(target: &mut [u8], m: Metrics) {
    let (w, h) = m.cell_wh();
    let (strike_top, strike_thick) = m.strikeout_px;
    let center_y = f32::from(strike_top) + f32::from(strike_thick) / 2.0;
    // x-height'ın yarısı; taban çizgisi merkezin altında olmasaydı (dejenere
    // metrik) kollar sıfıra iner ve işaret hiç çizilmez — panik değil, boşluk.
    let half_h = (f32::from(m.baseline_px) - center_y).max(0.0);
    let half_w = half_h / 2.0;
    let center_x = w as f32 / 2.0;
    // Yarı kalınlık: kapsama mesafeden hesaplanıyor, yani çizginin **ekseni**
    // ile piksel merkezi arasındaki uzaklık.
    let half_stroke = f32::from(m.underline_px.1).max(1.0) / 2.0;

    // Kolların uçları ve tepe noktası. `>` sola açık: uçlar solda, tepe sağda.
    let apex = (center_x + half_w, center_y);
    let upper = (center_x - half_w, center_y - half_h);
    let lower = (center_x - half_w, center_y + half_h);

    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let distance = distance_to_segment(px, py, upper, apex)
                .min(distance_to_segment(px, py, apex, lower));
            // Yarım piksellik geçiş bandı: `band`'in kenar yumuşatmasıyla aynı
            // sertlik. Daha genişi işareti bulanıklaştırır, daha darı
            // merdivenlendirir.
            let value = (half_stroke + 0.5 - distance).clamp(0.0, 1.0);
            // audit: `y < h` ve `x < w`, yani indeks `w * h`'nin altında.
            target[y * w + x] = (value * 255.0).round() as u8;
        }
    }
}

/// Bir noktanın doğru parçasına uzaklığı; chevron'un kenar yumuşatması buna
/// bakıyor.
///
/// CG **kullanmıyor**, `band` ve `curl` ile aynı gerekçe: çizim deterministik
/// kalıyor (sınama tam yapı assert edebiliyor), başarısızlık dalı doğmuyor ve
/// font hiç sorulmuyor.
fn distance_to_segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (abx, aby) = (b.0 - a.0, b.1 - a.1);
    let (apx, apy) = (px - a.0, py - a.1);
    let length = abx * abx + aby * aby;
    // Dejenere parça (sıfır uzunluk) uç noktaya uzaklığa iniyor: `half_h`
    // sıfır olduğunda bu dal koşuyor ve bölme hiç yapılmıyor.
    let t = if length > 0.0 {
        ((apx * abx + apy * aby) / length).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (apx - t * abx, apy - t * aby);
    (dx * dx + dy * dy).sqrt()
}

/// İstenen periyodu hücre genişliğini **tam bölen** en yakın değere yuvarlar.
///
/// `WAVE_COUNT`'nın kıvrım için taşıdığı kısıtın nokta/kesik karşılığı ve
/// aynı sebeple var: sprite tek hücre genişliğinde, komşularıyla döşeniyor ve
/// `x % period` deseni hücre sınırında faz kırar. Ölçüldü (bu makine,
/// Menlo 13pt@1x): `w = 8`, `Dashed`'in istediği periyot 6 → `8 % 6 = 2`,
/// yani iki komşu hücrede tire uzunlukları farklı görünürdü. Kıvrımda kısıt
/// uygulanıp burada uygulanmaması bir gözden kaçmaydı.
pub(crate) fn dividing_period(wanted: usize, w: usize) -> usize {
    let wanted = wanted.clamp(1, w.max(1));
    (wanted..=w).find(|p| w % p == 0).unwrap_or(w.max(1))
}

/// Yatay bant: `[top, top + thickness)` satırlarını `desen`in kabul ettiği
/// sütunlarda boyar. Kısmi kaplanan satır **kısmi alfa** alıyor — kalınlık
/// tam sayı olmak zorunda değil ve kenar yumuşatması bedava geliyor.
fn band(target: &mut [u8], m: Metrics, top: f32, thickness: f32, period: usize, filled: usize) {
    let (w, h) = m.cell_wh();
    let (y0, y1) = (top, top + thickness);
    for y in 0..h {
        let value = coverage(y, y0, y1);
        if value == 0 {
            continue;
        }
        for x in 0..w {
            if x % period < filled {
                // `max`: `Double`'ın iki bandı çakışırsa koyu olan kazanır.
                // audit: `y < h` ve `x < w`, yani indeks `w * h`'nin altında.
                target[y * w + x] = target[y * w + x].max(value);
            }
        }
    }
}

/// `[y, y+1)` pikselinin `[y0, y1)` bandıyla kesişimi → alfa baytı.
///
/// Tek sahip: `band` ve `curl` aynı kenar yumuşatma kuralını kullanmak
/// zorunda, yoksa kıvrım ötekilerden farklı yumuşaklık alır ve belirti
/// sessizdir — beş çeşidi karşılaştıran sınama "farklı olsunlar" dediği için
/// bunu göremez.
fn coverage(y: usize, y0: f32, y1: f32) -> u8 {
    (overlap(y, y0, y1) * 255.0).round() as u8
}

/// `[i, i+1)` pikselinin `[a, b)` aralığıyla kesişimi — **oran olarak**.
///
/// [`coverage`]'ın içinden çıkarıldı çünkü dikdörtgen iki eksende birden
/// örtüşüyor ve iki oranın **çarpımı** bir kez yuvarlanmak zorunda: iki
/// `coverage` baytını çarpmak iki kez yuvarlar ve `▀` ile `▄`'ün doygun
/// toplamı 255'te durmaz — ondan bir iki eksik kalır, yani alt alta iki
/// yarım blok arasında bu setin kapatmaya geldiği şeridin sönük bir
/// kopyası belirirdi.
fn overlap(i: usize, a: f32, b: f32) -> f32 {
    (b.min(i as f32 + 1.0) - a.max(i as f32)).clamp(0.0, 1.0)
}

/// Kıvrımlı çizgi: bandın merkezi sütun boyunca sinüsle salınıyor.
///
/// Dalga bandının **altı alt çizginin altına çakılı**: `position +
/// CURL_FACTOR * thickness`, hücre tabanına kırpılarak. Kırpma burada
/// **gerekli** — `font::rule_envelope` yalnız `position + thickness`'ı
/// hücrenin içine oturtuyor, kıvrım ise onun `CURL_FACTOR` katı kadar aşağı
/// iniyor ve tabanı taşabiliyor.
fn curl(target: &mut [u8], m: Metrics, position: f32, thickness: f32) {
    let (w, h) = m.cell_wh();
    // Dalga bandı alt çizgiden **aşağı** doğru büyüyor, yukarı değil: alt
    // çizgi ile hücre tabanı arasındaki satırlar boş ve orası glyph
    // gövdesinden uzak. Yukarı büyüseydi dalganın tepesi harf tabanlarıyla
    // birleşirdi (13pt: tepe 12 = `a e o`'nun son gövde satırı).
    // Genlik burada türüyor, `Metrics`'te değil: fonttan gelen bir ölçü değil,
    // bu çizicinin tasarım sabiti. `Metrics` "fonttan türeyen hücre
    // geometrisi" olarak kalıyor — tek tüketicisi olan bir sabiti `pub` bir
    // alana koymak onu `bt-gpu`'ya da gösterirdi.
    let top = position;
    let bottom = (position + CURL_FACTOR * thickness).min(h as f32);
    // Merkez ekseni bandın içinde kalsın: yarım kalınlık pay bırakılıyor.
    let (y_bottom, y_top) = (bottom - thickness / 2.0, top + thickness / 2.0);
    let mid = (y_bottom + y_top) / 2.0;
    let amplitude = (y_bottom - y_top) / 2.0;
    for x in 0..w {
        // Piksel **merkezinden** örnekleniyor ve bir hücreye tam
        // `WAVE_COUNT` dalga sığıyor: sprite komşularıyla döşendiğinde faz
        // kırılmıyor (bkz. `WAVE_COUNT`).
        let phase = core::f32::consts::TAU * (x as f32 + 0.5) * WAVE_COUNT / w as f32;
        let center = mid + amplitude * phase.sin();
        let (y0, y1) = (center - thickness / 2.0, center + thickness / 2.0);
        for y in 0..h {
            let value = coverage(y, y0, y1);
            if value > 0 {
                // audit: `y < h` ve `x < w`.
                target[y * w + x] = value;
            }
        }
    }
}

/// Yordamsal çizilen karakter aileleri.
///
/// [`RuleKind`]'ın ikinci kümesi: fonttan gelmiyorlar, hücre ölçüsünden
/// hesaplanıyorlar ve yüzden bağımsızlar. Farkları bir **karakter** olmaları
/// — atlasta [`crate::Sprite::Char`] olarak yaşıyorlar, yani `bt-gpu` onları
/// sıradan harflerden ayırt etmiyor ve sınır hiç değişmiyor.
enum Family {
    /// U+2580–U+259F — blok elemanları: yarımlar, sekizde bir merdivenleri,
    /// çeyrekler ve üç gölge.
    Block,
    /// U+2800–U+28FF — Braille deseni; alt 8 bit doğrudan nokta maskesi.
    Braille,
    /// U+2500–U+257F — çizgi çizim: dört kol × {yok, ince, kalın, çift},
    /// kesikli aile ve yuvarlak köşeler. **Köşegenler hariç**, bkz.
    /// [`family`].
    Line,
}

/// Karakterin yordamsal ailesi — **kapsamın tek sahibi**.
///
/// [`is_procedural`] ile [`draw_procedural`] aynı fonksiyonu çağırıyor,
/// çünkü ikisi `Atlas::slot`'un **iki ayrı** kolundan okunuyor: biri
/// normalizasyonda ("bu karakter yüze duyarsız mı"), öteki çizimde ("fonta
/// mı soracağız"). İki kopya sessizce kayardı ve kaymanın tehlikeli yönü de
/// sessiz olanı: kapıda var / normalizasyonda yok olsaydı aynı bitmap dört
/// yüz için dört ayrı yuva tutardı (`Atlas::slot`'un kendi doc'u).
fn family(ch: char) -> Option<Family> {
    match ch {
        '\u{2580}'..='\u{259F}' => Some(Family::Block),
        '\u{2800}'..='\u{28FF}' => Some(Family::Braille),
        // Köşegenler (`╱╲╳`) kapsamın içinde **bilerek bırakılmış bir delik**
        // (`discussion.md` → Karar 3B): mesafe alanı onları da çizebilirdi,
        // üçü de nadir ve setin ölçüsü kapsamı kapalı tutmaktan geçti. Kol
        // aşağıdaki aralığın **üstünde** durmak zorunda, yoksa delik kapanır.
        //
        // Deliğin ikinci bir işi var ve o da bilerek: bu üç karakter Menlo
        // Regular'da olup Bold'da olmayan tek kalan blok, yani
        // `face_fallback_is_cached_under_the_requested_face`'in fikstürü
        // (019'un yüz merdiveni kolunun bu makinedeki tek bekçisi) burada
        // yaşıyor.
        '\u{2571}'..='\u{2573}' => None,
        '\u{2500}'..='\u{257F}' => Some(Family::Line),
        _ => None,
    }
}

/// Karakter fonttan değil terminalden mi geliyor?
///
/// **Yordamsal çizim fontu koşulsuz yener** ve bu bir karar: kullanıcı bu
/// karakterleri taşıyan bir font seçse de yordamsal çizim kazanır. Gerekçe
/// döşeme — fontun em kutusu hücre kutusu değil ve bir fontun onu vermesini
/// garanti edecek hiçbir ölçüt yok. Ölçüldü (019 phase-2, kullanıcı ekran
/// görüntüsüyle bildirdi): Menlo 13pt'de 8×18 hücrenin yalnız 3–16 satırları
/// boyanıyor, yani alt alta iki `█` arasında ~5 piksel şerit kalıyor. 012
/// phase-9'un prompt işareti kararının aynısı: *işaret terminalin kendi
/// işareti, kullanıcının fontunun değil.*
pub(crate) fn is_procedural(ch: char) -> bool {
    family(ch).is_some()
}

/// `target`e yordamsal karakterin kapsama baytlarını çizer.
///
/// [`draw_rule`]'un ikizi ve aynı iki açılış satırıyla başlıyor; gerekçeleri
/// de aynı. Başarısız olamaz — font sorulmuyor, bağlam kurulmuyor — yani
/// çağıranın `Drawn`'ı bir varsayım değil tipin kendisi.
///
/// Kapsam dışı karakter **boş yuva** bırakıyor, panik değil: çağıran
/// ([`is_procedural`]) kapıyı zaten tutuyor, ama panik yolu bir çizicide
/// karşılığı olmayan bir risk — boş hücre görünür ve teşhis edilebilir bir
/// kayıp, panik ise pencerenin kendisi.
pub(crate) fn draw_procedural(ch: char, m: Metrics, target: &mut [u8]) {
    // audit: `draw`/`draw_rule` ile aynı ön koşul, aynı gerekçe.
    assert_eq!(target.len(), m.slot_bytes(), "tampon tam bir yuva olmalı");
    // Tampon paylaşılıyor ve içinde bir önceki glyph'in pikselleri var.
    target.fill(0);

    match family(ch) {
        Some(Family::Block) => block(ch, m, target),
        Some(Family::Braille) => braille(ch, m, target),
        Some(Family::Line) => line(ch, m, target),
        None => {}
    }
}

/// Kesirli dikdörtgen, kapsaması **toplanarak** — döşeyen parçalar için.
///
/// Toplama ile [`max_rect`] arasındaki fark bir zevk değil bir ölçüt:
/// birbirini döşeyen (ayrık) parçaların birleşimi **tam** kapsama vermek
/// zorunda. `▀` ile `▄` 13pt@2x'in h = 33'ünde 16.5 satırında buluşuyor ve ikisi de o
/// satıra 128 bırakıyor; `max` alsaydı hücrenin ortasında %50'lik bir şerit
/// kalırdı — yani bu setin kapatmaya geldiği kusurun hücre içine taşınmış
/// hâli. Doygun toplama onu 255'e kapatıyor.
fn add_rect(target: &mut [u8], m: Metrics, x0: f32, x1: f32, y0: f32, y1: f32) {
    rect(target, m, x0, x1, y0, y1, u8::saturating_add);
}

/// Kesirli dikdörtgen, kapsaması **piksel-max** ile — üst üste binen mürekkep.
///
/// Braille'in noktaları ayrı mürekkep lekeleri; geometri dejenere olup iki
/// nokta aynı piksele değdiğinde toplama onları sahte bir kalınlığa
/// çıkarırdı. `max` lekeleri dürüst tutuyor ve "maskenin sprite'ı = set
/// bitlerin sprite'larının piksel-max'i" değişmezi **yapısal** oluyor:
/// noktaların ayrıklığından değil, birleştiricinin kendisinden.
fn max_rect(target: &mut [u8], m: Metrics, x0: f32, x1: f32, y0: f32, y1: f32) {
    rect(target, m, x0, x1, y0, y1, u8::max);
}

/// İki eksende kesirli dikdörtgen; kenar yumuşatması [`overlap`]'ten bedava.
///
/// Oranlar **çarpılıp bir kez** yuvarlanıyor (bkz. [`overlap`]).
fn rect(target: &mut [u8], m: Metrics, x0: f32, x1: f32, y0: f32, y1: f32, join: fn(u8, u8) -> u8) {
    let (w, h) = m.cell_wh();
    for y in 0..h {
        let ry = overlap(y, y0, y1);
        if ry == 0.0 {
            continue;
        }
        for x in 0..w {
            let value = (ry * overlap(x, x0, x1) * 255.0).round() as u8;
            if value > 0 {
                // audit: `y < h` ve `x < w`, yani indeks `w * h`'nin altında.
                target[y * w + x] = join(target[y * w + x], value);
            }
        }
    }
}

/// Üç gölgenin (`░▒▓`, U+2591–U+2593) kapsama oranları.
///
/// **Ölçüm değil tasarım sabiti** (`CURL_FACTOR` emsali) ve sayılar
/// karakterlerin kendi tanımından: çeyrek, yarım, üç çeyrek yoğunluk.
///
/// Desen **yok, düz kapsama var** ve bu bilinçli: CP437'nin dama deseni tek
/// bitlik ekranların yoğunluk hilesiydi, atlas ise sekiz bitlik. Dama
/// yazılsaydı faz ancak adım hücrenin **iki** ölçüsünü de bölerse tutardı ve
/// tutmuyor — bu makinede 13pt@2x hücresi 16×33 ve 33 tek, yani her satır
/// sınırında desen kırılır, `░` ile dolu bir alanda yatay şeritler belirirdi.
/// Döşeme bu setin varlık sebebi; düz kapsama onu inşaen veriyor.
const SHADE_LEVELS: [f32; 3] = [0.25, 0.5, 0.75];

/// Çeyrek maskesinin bitleri: sol üst, sağ üst, sol alt, sağ alt.
const UL: u8 = 1;
const UR: u8 = 2;
const LL: u8 = 4;
const LR: u8 = 8;

/// U+2596–U+259F'in çeyrek maskeleri — **gerçek tablo**, formül yok.
///
/// Sıralama Unicode'un kendi sırası ve bir örüntüsü yok (`▖▗▘▙▚▛▜▝▞▟`):
/// tek çeyrekler üçe bölünmüş, üçlüler araya serpilmiş. Tablo karakter
/// adlarından yazıldı, sayaçtan değil.
const QUADRANTS: [(char, u8); 10] = [
    ('\u{2596}', LL),           // ▖ QUADRANT LOWER LEFT
    ('\u{2597}', LR),           // ▗ QUADRANT LOWER RIGHT
    ('\u{2598}', UL),           // ▘ QUADRANT UPPER LEFT
    ('\u{2599}', UL | LL | LR), // ▙ UPPER LEFT AND LOWER LEFT AND LOWER RIGHT
    ('\u{259A}', UL | LR),      // ▚ UPPER LEFT AND LOWER RIGHT
    ('\u{259B}', UL | UR | LL), // ▛ UPPER LEFT AND UPPER RIGHT AND LOWER LEFT
    ('\u{259C}', UL | UR | LR), // ▜ UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT
    ('\u{259D}', UR),           // ▝ QUADRANT UPPER RIGHT
    ('\u{259E}', UR | LL),      // ▞ UPPER RIGHT AND LOWER LEFT
    ('\u{259F}', UR | LL | LR), // ▟ UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT
];

/// Blok elemanları (U+2580–U+259F).
///
/// Ailenin üç yarısı var ve yalnız sonuncusu tablo istiyor: **iki aritmetik
/// koşu** (alttan ve soldan sekizde bir merdivenleri, yarımlar onların
/// dördüncü basamağı), **üç gölge** ve **on çeyrek**.
///
/// Sekizde bir dilimleri hücrenin kendi ölçüsünden bölünüyor, sabit bir
/// piksel sayısından değil: `h / 8` kesirli kalıyor ve kenar yumuşatması
/// [`rect`]'ten geliyor, yani merdiven her puntoda monoton ve `█` her
/// puntoda dolu.
fn block(ch: char, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let (w, h) = (w as f32, h as f32);
    let cp = u32::from(ch);
    match ch {
        // ▀ üst yarım. Alt merdivenin aynası değil kendi karakteri: Unicode
        // alt merdiveni 2581'den, sol merdiveni 258F'ten başlatıyor ve üst
        // yarımı ikisinin de dışında, aralığın başına koymuş.
        '\u{2580}' => add_rect(target, m, 0.0, w, 0.0, h / 2.0),
        // ▁▂▃▄▅▆▇█ — alttan n/8; sekizincisi dolu blok.
        '\u{2581}'..='\u{2588}' => {
            let n = (cp - 0x2580) as f32;
            add_rect(target, m, 0.0, w, h - h * n / 8.0, h);
        }
        // ▉▊▋▌▍▎▏ — soldan n/8, ama **azalarak**: 2589 yedi sekizde,
        // 258F bir sekizde. Kod noktası büyüdükçe dilim inceliyor, yani
        // sayaç 0x2590'tan geri sayıyor.
        '\u{2589}'..='\u{258F}' => {
            let n = (0x2590 - cp) as f32;
            add_rect(target, m, 0.0, w * n / 8.0, 0.0, h);
        }
        // ▐ sağ yarım.
        '\u{2590}' => add_rect(target, m, w / 2.0, w, 0.0, h),
        // ░▒▓ — düz kapsama (bkz. [`SHADE_LEVELS`]).
        '\u{2591}'..='\u{2593}' => {
            let level = SHADE_LEVELS[(cp - 0x2591) as usize];
            let value = (level * 255.0).round() as u8;
            target.fill(value);
        }
        // ▔ üst sekizde bir.
        '\u{2594}' => add_rect(target, m, 0.0, w, 0.0, h / 8.0),
        // ▕ sağ sekizde bir.
        '\u{2595}' => add_rect(target, m, w - w / 8.0, w, 0.0, h),
        // ▖▗▘▙▚▛▜▝▞▟ — çeyrekler, tablodan.
        _ => {
            let mask = QUADRANTS
                .iter()
                .find(|&&(c, _)| c == ch)
                .map_or(0, |&(_, mask)| mask);
            // Çeyrekler **ayrık döşüyor**: dördünün birleşimi `█`. Ortadaki
            // kesirli satır/sütun iki çeyrekten birer pay alıyor ve
            // [`add_rect`] onları 255'e kapatıyor.
            for (bit, (x0, x1, y0, y1)) in [
                (UL, (0.0, w / 2.0, 0.0, h / 2.0)),
                (UR, (w / 2.0, w, 0.0, h / 2.0)),
                (LL, (0.0, w / 2.0, h / 2.0, h)),
                (LR, (w / 2.0, w, h / 2.0, h)),
            ] {
                if mask & bit != 0 {
                    add_rect(target, m, x0, x1, y0, y1);
                }
            }
        }
    }
}

/// Braille noktasının kendi alt hücresini doldurma oranı.
///
/// **Ölçüm değil tasarım sabiti** (`CURL_FACTOR` emsali). İki şeyi birden
/// tutuyor: nokta küçük puntoda görünecek kadar büyük, komşu noktalardan
/// ayrılacak kadar küçük. Oran — mutlak piksel değil — yani punto ve ölçek
/// büyüdükçe nokta da büyüyor; sabit bir piksel yarıçapı @2x'te iğne başına
/// dönerdi.
///
/// Nokta bu phase'de bir **dikdörtgen**; yuvarlağın istediği mesafe alanı
/// phase-2'nin primitifi ve yalnız bunun için getirilmesi karşılığı olmayan
/// bir bedel olurdu — 13pt'de alt hücre 4×4.25 piksel, orada kare ile daire
/// arasındaki fark bir pikselin altında.
const BRAILLE_DOT_FILL: f32 = 0.7;

/// Braille deseni (U+2800–U+28FF).
///
/// **Tablo yok:** kod noktasının alt 8 biti doğrudan nokta maskesi — Unicode
/// bloğu tam olarak böyle tanımlıyor. Bitin hücresi de tanımdan geliyor:
/// noktalar 2×4 ızgarada ve numaralandırma tarihsel olarak önce 2×3'lük
/// hücreyi dolduruyor, dördüncü satır sonradan eklenmiş —
///
/// ```text
///   bit0  bit3        1 4
///   bit1  bit4   =    2 5
///   bit2  bit5        3 6
///   bit6  bit7        7 8
/// ```
///
/// yani bit 0–2 sol sütunun ilk üç satırı, bit 3–5 sağ sütunun ilk üç
/// satırı, bit 6 ve 7 de dördüncü satırın sol ve sağı.
fn braille(ch: char, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let (dot_w, dot_h) = (w as f32 / 2.0, h as f32 / 4.0);
    let mask = u32::from(ch) & 0xFF;
    for bit in 0..8u32 {
        if mask & (1 << bit) == 0 {
            continue;
        }
        let (col, row) = match bit {
            0..=2 => (0, bit),
            3..=5 => (1, bit - 3),
            6 => (0, 3),
            _ => (1, 3),
        };
        let (cx, cy) = ((col as f32 + 0.5) * dot_w, (row as f32 + 0.5) * dot_h);
        let (half_w, half_h) = (
            dot_w * BRAILLE_DOT_FILL / 2.0,
            dot_h * BRAILLE_DOT_FILL / 2.0,
        );
        max_rect(
            target,
            m,
            cx - half_w,
            cx + half_w,
            cy - half_h,
            cy + half_h,
        );
    }
}

/// Kalın çizginin ince çizgiye oranı.
///
/// **Ölçüm değil tasarım sabiti** ([`CURL_FACTOR`] emsali). Unicode ince ile
/// kalını ayrı kod noktalarına koyuyor (`─` U+2500 ince, `━` U+2501 kalın)
/// ama kalının *ne kadar* kalın olduğunu söylemiyor: ince
/// [`Metrics::underline_px`]'ten geliyor (depoda "çizgi kalınlığı"nın zaten
/// bir cevabı var), kalın için ikinci bir sayı gerekiyor. İki kat seçildi —
/// bir buçuk kat 13pt'de tam sayıya yuvarlandığında inceden ayrılmıyor, üç
/// kat hücre genişliğinin üçte birini yiyor.
const HEAVY_FACTOR: f32 = 2.0;

/// Bir kolun çizgi stili.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stroke {
    None,
    Light,
    Heavy,
    /// İki ince ray; aralarında bir kalınlık boşluk.
    Double,
}

// Kol indeksleri. Karşıt kol `dir ^ 1`: çiftler bilerek yan yana.
const UP: usize = 0;
const DOWN: usize = 1;
const LEFT: usize = 2;
const RIGHT: usize = 3;

/// Bir çizgi karakterinin geometri tarifi.
///
/// Kolların sırası [`UP`], [`DOWN`], [`LEFT`], [`RIGHT`]; `dashes` sıfırsa
/// çizgi düz, değilse hücre başına o kadar tire; `arc` köşeyi yuvarlatıyor.
#[derive(Clone, Copy)]
struct Recipe {
    arms: [Stroke; 4],
    dashes: u8,
    arc: bool,
}

const N: Stroke = Stroke::None;
const L: Stroke = Stroke::Light;
const H: Stroke = Stroke::Heavy;
const D: Stroke = Stroke::Double;

const fn plain(arms: [Stroke; 4]) -> Recipe {
    Recipe {
        arms,
        dashes: 0,
        arc: false,
    }
}

const fn dashed(arms: [Stroke; 4], dashes: u8) -> Recipe {
    Recipe {
        arms,
        dashes,
        arc: false,
    }
}

const fn arc(arms: [Stroke; 4]) -> Recipe {
    Recipe {
        arms,
        dashes: 0,
        arc: true,
    }
}

/// U+2500–U+257F'in kol tabloları — **gerçek tablo**, formül yok.
///
/// Sıra kod noktasının sırası (indeks = `cp - 0x2500`) ve bir sayaç değil:
/// `251C..2523`'ün (yukarı, aşağı, sağ) kalın maskeleri sırayla 000, 001,
/// 100, 010, 110, 101, 011, 111, yani bir **permütasyon**; aynı ailenin
/// `252C..2533` karşılığı (sol, sağ, aşağı) ise 000, 100, 010, 110, 001,
/// 101, 011, 111 — başka bir permütasyon. Formül aramak yanlış bir örüntüyü
/// sessizce kodlardı; tablo karakter **adlarından** yazıldı ve bekçisi
/// adları Unicode veritabanından okuyan [`crate::tests`] tarafında
/// (`the_arm_table_matches_the_unicode_names`).
///
/// Köşegenlerin (`╱╲╳`) satırları boş: [`family`] onları kapsamın dışında
/// tutuyor, yani bu üç satır hiç okunmuyor. Tablo yine de 128 satır, çünkü
/// indeks aritmetiği deliği atlayamaz.
#[rustfmt::skip]
const LINES: [Recipe; 128] = [
    plain([N, N, L, L]),      // ─ LIGHT HORIZONTAL
    plain([N, N, H, H]),      // ━ HEAVY HORIZONTAL
    plain([L, L, N, N]),      // │ LIGHT VERTICAL
    plain([H, H, N, N]),      // ┃ HEAVY VERTICAL
    dashed([N, N, L, L], 3),  // ┄ LIGHT TRIPLE DASH HORIZONTAL
    dashed([N, N, H, H], 3),  // ┅ HEAVY TRIPLE DASH HORIZONTAL
    dashed([L, L, N, N], 3),  // ┆ LIGHT TRIPLE DASH VERTICAL
    dashed([H, H, N, N], 3),  // ┇ HEAVY TRIPLE DASH VERTICAL
    dashed([N, N, L, L], 4),  // ┈ LIGHT QUADRUPLE DASH HORIZONTAL
    dashed([N, N, H, H], 4),  // ┉ HEAVY QUADRUPLE DASH HORIZONTAL
    dashed([L, L, N, N], 4),  // ┊ LIGHT QUADRUPLE DASH VERTICAL
    dashed([H, H, N, N], 4),  // ┋ HEAVY QUADRUPLE DASH VERTICAL
    plain([N, L, N, L]),      // ┌ LIGHT DOWN AND RIGHT
    plain([N, L, N, H]),      // ┍ DOWN LIGHT AND RIGHT HEAVY
    plain([N, H, N, L]),      // ┎ DOWN HEAVY AND RIGHT LIGHT
    plain([N, H, N, H]),      // ┏ HEAVY DOWN AND RIGHT
    plain([N, L, L, N]),      // ┐ LIGHT DOWN AND LEFT
    plain([N, L, H, N]),      // ┑ DOWN LIGHT AND LEFT HEAVY
    plain([N, H, L, N]),      // ┒ DOWN HEAVY AND LEFT LIGHT
    plain([N, H, H, N]),      // ┓ HEAVY DOWN AND LEFT
    plain([L, N, N, L]),      // └ LIGHT UP AND RIGHT
    plain([L, N, N, H]),      // ┕ UP LIGHT AND RIGHT HEAVY
    plain([H, N, N, L]),      // ┖ UP HEAVY AND RIGHT LIGHT
    plain([H, N, N, H]),      // ┗ HEAVY UP AND RIGHT
    plain([L, N, L, N]),      // ┘ LIGHT UP AND LEFT
    plain([L, N, H, N]),      // ┙ UP LIGHT AND LEFT HEAVY
    plain([H, N, L, N]),      // ┚ UP HEAVY AND LEFT LIGHT
    plain([H, N, H, N]),      // ┛ HEAVY UP AND LEFT
    plain([L, L, N, L]),      // ├ LIGHT VERTICAL AND RIGHT
    plain([L, L, N, H]),      // ┝ VERTICAL LIGHT AND RIGHT HEAVY
    plain([H, L, N, L]),      // ┞ UP HEAVY AND RIGHT DOWN LIGHT
    plain([L, H, N, L]),      // ┟ DOWN HEAVY AND RIGHT UP LIGHT
    plain([H, H, N, L]),      // ┠ VERTICAL HEAVY AND RIGHT LIGHT
    plain([H, L, N, H]),      // ┡ DOWN LIGHT AND RIGHT UP HEAVY
    plain([L, H, N, H]),      // ┢ UP LIGHT AND RIGHT DOWN HEAVY
    plain([H, H, N, H]),      // ┣ HEAVY VERTICAL AND RIGHT
    plain([L, L, L, N]),      // ┤ LIGHT VERTICAL AND LEFT
    plain([L, L, H, N]),      // ┥ VERTICAL LIGHT AND LEFT HEAVY
    plain([H, L, L, N]),      // ┦ UP HEAVY AND LEFT DOWN LIGHT
    plain([L, H, L, N]),      // ┧ DOWN HEAVY AND LEFT UP LIGHT
    plain([H, H, L, N]),      // ┨ VERTICAL HEAVY AND LEFT LIGHT
    plain([H, L, H, N]),      // ┩ DOWN LIGHT AND LEFT UP HEAVY
    plain([L, H, H, N]),      // ┪ UP LIGHT AND LEFT DOWN HEAVY
    plain([H, H, H, N]),      // ┫ HEAVY VERTICAL AND LEFT
    plain([N, L, L, L]),      // ┬ LIGHT DOWN AND HORIZONTAL
    plain([N, L, H, L]),      // ┭ LEFT HEAVY AND RIGHT DOWN LIGHT
    plain([N, L, L, H]),      // ┮ RIGHT HEAVY AND LEFT DOWN LIGHT
    plain([N, L, H, H]),      // ┯ DOWN LIGHT AND HORIZONTAL HEAVY
    plain([N, H, L, L]),      // ┰ DOWN HEAVY AND HORIZONTAL LIGHT
    plain([N, H, H, L]),      // ┱ RIGHT LIGHT AND LEFT DOWN HEAVY
    plain([N, H, L, H]),      // ┲ LEFT LIGHT AND RIGHT DOWN HEAVY
    plain([N, H, H, H]),      // ┳ HEAVY DOWN AND HORIZONTAL
    plain([L, N, L, L]),      // ┴ LIGHT UP AND HORIZONTAL
    plain([L, N, H, L]),      // ┵ LEFT HEAVY AND RIGHT UP LIGHT
    plain([L, N, L, H]),      // ┶ RIGHT HEAVY AND LEFT UP LIGHT
    plain([L, N, H, H]),      // ┷ UP LIGHT AND HORIZONTAL HEAVY
    plain([H, N, L, L]),      // ┸ UP HEAVY AND HORIZONTAL LIGHT
    plain([H, N, H, L]),      // ┹ RIGHT LIGHT AND LEFT UP HEAVY
    plain([H, N, L, H]),      // ┺ LEFT LIGHT AND RIGHT UP HEAVY
    plain([H, N, H, H]),      // ┻ HEAVY UP AND HORIZONTAL
    plain([L, L, L, L]),      // ┼ LIGHT VERTICAL AND HORIZONTAL
    plain([L, L, H, L]),      // ┽ LEFT HEAVY AND RIGHT VERTICAL LIGHT
    plain([L, L, L, H]),      // ┾ RIGHT HEAVY AND LEFT VERTICAL LIGHT
    plain([L, L, H, H]),      // ┿ VERTICAL LIGHT AND HORIZONTAL HEAVY
    plain([H, L, L, L]),      // ╀ UP HEAVY AND DOWN HORIZONTAL LIGHT
    plain([L, H, L, L]),      // ╁ DOWN HEAVY AND UP HORIZONTAL LIGHT
    plain([H, H, L, L]),      // ╂ VERTICAL HEAVY AND HORIZONTAL LIGHT
    plain([H, L, H, L]),      // ╃ LEFT UP HEAVY AND RIGHT DOWN LIGHT
    plain([H, L, L, H]),      // ╄ RIGHT UP HEAVY AND LEFT DOWN LIGHT
    plain([L, H, H, L]),      // ╅ LEFT DOWN HEAVY AND RIGHT UP LIGHT
    plain([L, H, L, H]),      // ╆ RIGHT DOWN HEAVY AND LEFT UP LIGHT
    plain([H, L, H, H]),      // ╇ DOWN LIGHT AND UP HORIZONTAL HEAVY
    plain([L, H, H, H]),      // ╈ UP LIGHT AND DOWN HORIZONTAL HEAVY
    plain([H, H, H, L]),      // ╉ RIGHT LIGHT AND LEFT VERTICAL HEAVY
    plain([H, H, L, H]),      // ╊ LEFT LIGHT AND RIGHT VERTICAL HEAVY
    plain([H, H, H, H]),      // ╋ HEAVY VERTICAL AND HORIZONTAL
    dashed([N, N, L, L], 2),  // ╌ LIGHT DOUBLE DASH HORIZONTAL
    dashed([N, N, H, H], 2),  // ╍ HEAVY DOUBLE DASH HORIZONTAL
    dashed([L, L, N, N], 2),  // ╎ LIGHT DOUBLE DASH VERTICAL
    dashed([H, H, N, N], 2),  // ╏ HEAVY DOUBLE DASH VERTICAL
    plain([N, N, D, D]),      // ═ DOUBLE HORIZONTAL
    plain([D, D, N, N]),      // ║ DOUBLE VERTICAL
    plain([N, L, N, D]),      // ╒ DOWN SINGLE AND RIGHT DOUBLE
    plain([N, D, N, L]),      // ╓ DOWN DOUBLE AND RIGHT SINGLE
    plain([N, D, N, D]),      // ╔ DOUBLE DOWN AND RIGHT
    plain([N, L, D, N]),      // ╕ DOWN SINGLE AND LEFT DOUBLE
    plain([N, D, L, N]),      // ╖ DOWN DOUBLE AND LEFT SINGLE
    plain([N, D, D, N]),      // ╗ DOUBLE DOWN AND LEFT
    plain([L, N, N, D]),      // ╘ UP SINGLE AND RIGHT DOUBLE
    plain([D, N, N, L]),      // ╙ UP DOUBLE AND RIGHT SINGLE
    plain([D, N, N, D]),      // ╚ DOUBLE UP AND RIGHT
    plain([L, N, D, N]),      // ╛ UP SINGLE AND LEFT DOUBLE
    plain([D, N, L, N]),      // ╜ UP DOUBLE AND LEFT SINGLE
    plain([D, N, D, N]),      // ╝ DOUBLE UP AND LEFT
    plain([L, L, N, D]),      // ╞ VERTICAL SINGLE AND RIGHT DOUBLE
    plain([D, D, N, L]),      // ╟ VERTICAL DOUBLE AND RIGHT SINGLE
    plain([D, D, N, D]),      // ╠ DOUBLE VERTICAL AND RIGHT
    plain([L, L, D, N]),      // ╡ VERTICAL SINGLE AND LEFT DOUBLE
    plain([D, D, L, N]),      // ╢ VERTICAL DOUBLE AND LEFT SINGLE
    plain([D, D, D, N]),      // ╣ DOUBLE VERTICAL AND LEFT
    plain([N, L, D, D]),      // ╤ DOWN SINGLE AND HORIZONTAL DOUBLE
    plain([N, D, L, L]),      // ╥ DOWN DOUBLE AND HORIZONTAL SINGLE
    plain([N, D, D, D]),      // ╦ DOUBLE DOWN AND HORIZONTAL
    plain([L, N, D, D]),      // ╧ UP SINGLE AND HORIZONTAL DOUBLE
    plain([D, N, L, L]),      // ╨ UP DOUBLE AND HORIZONTAL SINGLE
    plain([D, N, D, D]),      // ╩ DOUBLE UP AND HORIZONTAL
    plain([L, L, D, D]),      // ╪ VERTICAL SINGLE AND HORIZONTAL DOUBLE
    plain([D, D, L, L]),      // ╫ VERTICAL DOUBLE AND HORIZONTAL SINGLE
    plain([D, D, D, D]),      // ╬ DOUBLE VERTICAL AND HORIZONTAL
    arc([N, L, N, L]),        // ╭ LIGHT ARC DOWN AND RIGHT
    arc([N, L, L, N]),        // ╮ LIGHT ARC DOWN AND LEFT
    arc([L, N, L, N]),        // ╯ LIGHT ARC UP AND LEFT
    arc([L, N, N, L]),        // ╰ LIGHT ARC UP AND RIGHT
    plain([N, N, N, N]),      // ╱ köşegen — kapsam dışı, bu satır okunmuyor
    plain([N, N, N, N]),      // ╲ köşegen — kapsam dışı
    plain([N, N, N, N]),      // ╳ köşegen — kapsam dışı
    plain([N, N, L, N]),      // ╴ LIGHT LEFT
    plain([L, N, N, N]),      // ╵ LIGHT UP
    plain([N, N, N, L]),      // ╶ LIGHT RIGHT
    plain([N, L, N, N]),      // ╷ LIGHT DOWN
    plain([N, N, H, N]),      // ╸ HEAVY LEFT
    plain([H, N, N, N]),      // ╹ HEAVY UP
    plain([N, N, N, H]),      // ╺ HEAVY RIGHT
    plain([N, H, N, N]),      // ╻ HEAVY DOWN
    plain([N, N, L, H]),      // ╼ LIGHT LEFT AND HEAVY RIGHT
    plain([L, H, N, N]),      // ╽ LIGHT UP AND HEAVY DOWN
    plain([N, N, H, L]),      // ╾ HEAVY LEFT AND LIGHT RIGHT
    plain([H, L, N, N]),      // ╿ HEAVY UP AND LIGHT DOWN
];

/// Bir rayın **piksel ızgarasına oturtulmuş** bandı: `[başlangıç, başlangıç +
/// kalınlık)`.
///
/// Yuvarlama şart ve bedeli gözle görülür: bu makinede 13pt@1x hücresi 8×18,
/// yani dikey çizginin ekseni x = 4.0 ve yuvarlanmamış bir ince bant
/// `[3.5, 4.5)` iki sütuna %50'şer düşerdi — bütün dikey çizgiler gri,
/// yataylar (eksen 9.0) net. Kural yeni değil: alt çizginin konumu da
/// kalınlığı da zaten tam sayı ([`Metrics::underline_px`]) ve
/// [`font::rule_envelope`] kalınlığı `>= 1`'e bağlıyor, yani `start` tam
/// sayıyken bant da tam sayı kenarlarda bitiyor ve [`overlap`] kesir
/// üretmiyor.
fn rail(center: f32, thickness: f32) -> (f32, f32) {
    let start = (center - thickness / 2.0).round();
    (start, start + thickness)
}

/// Bir kolun raylarının **dik eksendeki** bantları.
///
/// Çift çizginin ray aralığı [`RuleKind::Double`]'ın bugünkü
/// `position + 2.0 * thickness`'ından geliyor: iki ray arasında tam bir
/// kalınlık boşluk kalıyor. Yeni bir tasarım sabiti doğmuyor.
fn rails(stroke: Stroke, center: f32, thin: f32) -> [Option<(f32, f32)>; 2] {
    match stroke {
        Stroke::None => [None, None],
        Stroke::Light => [Some(rail(center, thin)), None],
        Stroke::Heavy => [Some(rail(center, HEAVY_FACTOR * thin)), None],
        Stroke::Double => [
            Some(rail(center - thin, thin)),
            Some(rail(center + thin, thin)),
        ],
    }
}

/// Eksen çevirici: `along` kolun ilerleme ekseni, `across` rayın bandı.
///
/// [`max_rect`] üstünde duruyor ve birleştiricisi bilerek **piksel-max**:
/// kollar aynı mürekkebi paylaşıyor (kesişimde üst üste biniyorlar), yani
/// `add_rect`'in doygun toplamı kavşağı yapay olarak koyulaştırırdı. Yan
/// kazanç yapısal: birleşim yasası (`┌ ∪ ┘ == ┼`) aynı kolun her karakterde
/// **aynı dikdörtgeni** vermesinden çıkıyor.
fn stroke_rect(
    target: &mut [u8],
    m: Metrics,
    vertical: bool,
    along: (f32, f32),
    across: (f32, f32),
) {
    if vertical {
        max_rect(target, m, across.0, across.1, along.0, along.1);
    } else {
        max_rect(target, m, along.0, along.1, across.0, across.1);
    }
}

/// Bir rayın kenardan içeri doğru nereye kadar gideceği.
///
/// Üç hâl var ve üçü de çift çizgi kavşaklarının gerçek geometrisinden:
///
/// - **Dik kol yok** → ray merkezi geçer ([`f32::round`] ile ızgaraya
///   oturarak) ve karşı kolun rayıyla tam orada buluşur. `─`'nin iki kolu
///   böyle birleşiyor.
/// - **Geçiyor** → ray bütün dik rayları kesip en uzağının uzak kenarında
///   biter. Köşeyi kapatan şey bu: `╔`'in üst rayı sol dikey rayın **sol**
///   kenarına kadar gidiyor, yoksa köşede bir çentik kalırdı.
/// - **Dönüyor** → ray ilk rastladığı dik rayın uzak kenarında durur, yani
///   o rayın içine dirsek yapar. `╬`'in dört dirseği ve ortasındaki boş
///   kanal bundan doğuyor.
///
/// `turn` kararının kendisi [`arm`]'de; burası yalnız sonucu ölçüyor.
fn reach(bands: [Option<(f32, f32)>; 4], forward: bool, turn: bool, center: f32) -> f32 {
    let mut nearest: Option<(f32, f32)> = None;
    let mut farthest: Option<f32> = None;
    for band in bands.into_iter().flatten() {
        let (near_edge, far_edge) = if forward { band } else { (band.1, band.0) };
        let closer = nearest.is_none_or(|current| {
            let current_near = if forward { current.0 } else { current.1 };
            if forward {
                near_edge < current_near
            } else {
                near_edge > current_near
            }
        });
        if closer {
            nearest = Some(band);
        }
        farthest = Some(match farthest {
            Some(edge) if forward => edge.max(far_edge),
            Some(edge) => edge.min(far_edge),
            None => far_edge,
        });
    }
    match (turn, nearest, farthest) {
        (true, Some(band), _) => {
            if forward {
                band.1
            } else {
                band.0
            }
        }
        (false, _, Some(edge)) => edge,
        // Dik kol yok: merkezde buluşuyoruz.
        _ => center.round(),
    }
}

/// Tek bir kol: kenardan merkeze doğru bir ya da iki ray.
///
/// **Dönme kuralı** ([`reach`]'in üçüncü hâli) iki cümlede yaşıyor ve ikisi
/// de çift çizginin "kanal" olmasından çıkıyor — çift çizgi bir çizgi değil
/// iki duvarlı bir kanal, kavşakta açılan duvar kanalı kapatmaz:
///
/// - **Çift rayın** kendi tarafındaki dik kol da çiftse ray döner: `╠`'te
///   sağ (iç) duvar kırılıp yatay raylara dirsek yapıyor, sol (dış) duvar
///   kesintisiz geçiyor.
/// - **Tek ray** (ince/kalın) yalnız dik kolların **ikisi de** çiftken **ve**
///   karşı kolu yokken döner: `╤`'nin sapı alt rayda duruyor, `╪`'nin dikey
///   çizgisi ise baştan sona geçiyor. Karşı kol koşulu olmasaydı `╪` ortadan
///   ikiye bölünürdü.
fn arm(spec: Recipe, dir: usize, m: Metrics, target: &mut [u8]) {
    let stroke = spec.arms[dir];
    if stroke == Stroke::None {
        return;
    }
    let (w, h) = m.cell_wh();
    let (w, h) = (w as f32, h as f32);
    let thin = f32::from(m.underline_px.1).max(1.0);
    let vertical = dir == UP || dir == DOWN;
    // Kenardan merkeze: yukarı ve sol koller 0'dan artarak, aşağı ve sağ
    // kollar hücrenin ucundan azalarak ilerliyor.
    let forward = dir == UP || dir == LEFT;
    let (along_extent, across_center) = if vertical { (h, w / 2.0) } else { (w, h / 2.0) };
    let along_center = along_extent / 2.0;
    // Dik kolların yönleri **ve** rayın kendi tarafı aynı çift: dikey bir
    // kolun rayları solda/sağda, yatay bir kolunkiler üstte/altta.
    let sides = if vertical { [LEFT, RIGHT] } else { [UP, DOWN] };
    let crossing = {
        let first = rails(spec.arms[sides[0]], along_center, thin);
        let second = rails(spec.arms[sides[1]], along_center, thin);
        [first[0], first[1], second[0], second[1]]
    };
    let single_turns = spec.arms[sides[0]] == Stroke::Double
        && spec.arms[sides[1]] == Stroke::Double
        // Karşıt kol: `dir ^ 1` (UP↔DOWN, LEFT↔RIGHT).
        && spec.arms[dir ^ 1] == Stroke::None;

    for (index, band) in rails(stroke, across_center, thin).into_iter().enumerate() {
        let Some(band) = band else { continue };
        let turn = if stroke == Stroke::Double {
            spec.arms[sides[index]] == Stroke::Double
        } else {
            single_turns
        };
        let edge = reach(crossing, forward, turn, along_center);
        let along = if forward {
            (0.0, edge)
        } else {
            (edge, along_extent)
        };
        stroke_rect(target, m, vertical, along, band);
    }
}

/// Kesikli çizgi ailesi (`┄┅┆┇┈┉┊┋╌╍╎╏`).
///
/// Periyot [`dividing_period`]'dan, yani hücreyi **tam bölen** en yakın
/// değere çekiliyor; döşeme bu setin varlık sebebi ve faz hücre sınırında
/// kırılamaz. Bedeli görünür bir bilgi kaybı ve kabul edildi
/// (`discussion.md` → Karar 4): bu makinede `w = 8`'de üçlü kesiğin istediği
/// periyot 3 → **4**'e çekiliyor ve `┄` ile `╌` **aynı sprite'a** çöküyor;
/// bölenleri seyrek bir ölçüde (144pt@1x'in `w = 87`'si: 1, 3, 29, 87) `╌`
/// hücre başına **tek** tireye iniyor. Bekçisi çökmeyi listeye yazmıyor,
/// **türetiyor**: aynı eksendeki iki yoğunluk ancak periyotları eşitse eşit
/// (`dashed_densities_collapse_only_with_the_period`).
///
/// Dolu oranı [`RuleKind::Dashed`]'in oranı (üçte iki); ikinci bir tasarım
/// sabiti doğmuyor.
fn dashes(spec: Recipe, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let thin = f32::from(m.underline_px.1).max(1.0);
    let vertical = spec.arms[UP] != Stroke::None;
    let (along_extent, across_center) = if vertical {
        (h, w as f32 / 2.0)
    } else {
        (w, h as f32 / 2.0)
    };
    let stroke = spec.arms[if vertical { UP } else { LEFT }];
    // Kesikli ailede çift çizgi yok: tek ray.
    let Some(band) = rails(stroke, across_center, thin)[0] else {
        return;
    };
    let period = dividing_period(
        along_extent.div_ceil(usize::from(spec.dashes).max(1)),
        along_extent,
    );
    let filled = (period * 2 / 3).max(1);
    for start in (0..along_extent).step_by(period) {
        let end = (start + filled).min(along_extent);
        stroke_rect(target, m, vertical, (start as f32, end as f32), band);
    }
}

/// Yuvarlak köşe (`╭╮╯╰`): iki sap ve onları birleştiren çeyrek yay.
///
/// Yay [`chevron`]'un mesafe alanının ikizi — orada `distance_to_segment`,
/// burada `|hypot(x - ox, y - oy) - r|`, aynı yarım piksellik geçiş bandı
/// ve aynı `half_stroke`. [`curl`] emsal **değil**: sütun başına tek bir `y`
/// örnekliyor ve çeyrek yayın dikey teğetinde bant kopardı.
///
/// Yarıçap dört köşede de aynı ve **oturtulmuş eksenlerden** türüyor:
/// merkezin hücre kenarlarına uzaklıklarının en küçüğü. Tek yönden
/// türetilseydi `╭──╮`'nin sol köşesi sağından dar olurdu; teğet noktaları
/// oturtulmamış eksenden alınsaydı sapla yay arasında yarım piksellik bir
/// kırık kalırdı.
///
/// Sap ile yayın dikişi **tam**: bant tam sayı kenarlarda
/// ([`rail`]) ve eksen hizalı bir vuruşta mesafe tabanlı kapsama ile
/// [`overlap`]'inki cebirsel olarak aynı sonucu veriyor (kalınlık `>= 1`,
/// yani yarı kalınlık `>= 0.5`).
fn corner(spec: Recipe, m: Metrics, target: &mut [u8]) {
    let (w, h) = m.cell_wh();
    let thin = f32::from(m.underline_px.1).max(1.0);
    let vertical_band = rail(w as f32 / 2.0, thin);
    let horizontal_band = rail(h as f32 / 2.0, thin);
    let axis_x = (vertical_band.0 + vertical_band.1) / 2.0;
    let axis_y = (horizontal_band.0 + horizontal_band.1) / 2.0;
    let radius = axis_x
        .min(w as f32 - axis_x)
        .min(axis_y)
        .min(h as f32 - axis_y);
    let right = spec.arms[RIGHT] != Stroke::None;
    let down = spec.arms[DOWN] != Stroke::None;
    let (ox, oy) = (
        if right {
            axis_x + radius
        } else {
            axis_x - radius
        },
        if down {
            axis_y + radius
        } else {
            axis_y - radius
        },
    );

    // Saplar: teğet noktasından hücre kenarına.
    let (x0, x1) = if right { (ox, w as f32) } else { (0.0, ox) };
    let (y0, y1) = if down { (oy, h as f32) } else { (0.0, oy) };
    max_rect(target, m, x0, x1, horizontal_band.0, horizontal_band.1);
    max_rect(target, m, vertical_band.0, vertical_band.1, y0, y1);

    let half_stroke = thin / 2.0;
    for y in 0..h {
        for x in 0..w {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            // Çeyrek kısıtı: yay merkezin **kolların tersi** tarafında.
            // Kesme tam teğet noktasında ve orada sap zaten dolu, yani
            // kesilen şey sapın altında kalıyor.
            let inside =
                if right { px <= ox } else { px >= ox } && if down { py <= oy } else { py >= oy };
            if !inside {
                continue;
            }
            let distance = ((px - ox).hypot(py - oy) - radius).abs();
            let value = (half_stroke + 0.5 - distance).clamp(0.0, 1.0);
            let value = (value * 255.0).round() as u8;
            // audit: `y < h` ve `x < w`, yani indeks `w * h`'nin altında.
            target[y * w + x] = target[y * w + x].max(value);
        }
    }
}

/// Çizgi çizim karakteri (U+2500–U+257F).
///
/// Tarif [`LINES`]'tan geliyor; çizim üç kola ayrılıyor ve üçü de aynı iki
/// primitifi kullanıyor (dikdörtgen ve mesafe alanı). Kapsam dışı indeks
/// **boş yuva** bırakıyor, panik değil: kapı ([`family`]) zaten tutuyor.
fn line(ch: char, m: Metrics, target: &mut [u8]) {
    let Some(&spec) = LINES.get((u32::from(ch) - 0x2500) as usize) else {
        return;
    };
    if spec.dashes > 0 {
        dashes(spec, m, target);
    } else if spec.arc {
        corner(spec, m, target);
    } else {
        for dir in [UP, DOWN, LEFT, RIGHT] {
            arm(spec, dir, m, target);
        }
    }
}
