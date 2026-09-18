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
/// Tampon yalnız gerçekten çizim yapılacaksa sıfırlanır.
pub(crate) fn draw(font: &CTFont, ch: char, m: Metrics, target: &mut [u8]) -> DrawResult {
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
    let position = CGPoint::new(0.0, baseline);
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
    /// mekanizma: bu enum yordamsal çizilen sprite'ların kümesi — fonttan
    /// gelmiyor, yüzden bağımsız ([`crate::Face::Regular`]'a çivili) ve
    /// atlasta kendi payını tutuyor. Beşi alt çizgi, biri üstü çizili, biri
    /// de bu.
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
    let ratio = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).clamp(0.0, 1.0);
    (ratio * 255.0).round() as u8
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
