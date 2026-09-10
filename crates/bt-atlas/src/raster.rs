//! Tek glyph'i alfa baytlarına çizer.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_core_foundation::{CGFloat, CGPoint};
use objc2_core_graphics::{CGBitmapContextCreate, CGContext, CGImageAlphaInfo};
use objc2_core_text::CTFont;

use crate::font::{self, Metrics};

/// [`ciz`]'in sonucu.
///
/// İki başarısızlık ayrı varyant çünkü **teşhisleri** ayrı, davranışları değil:
/// ikisi de tofu'ya düşer ve ikisi de önbelleğe girer. `BaglamYok`'un
/// önbelleğe girmesi ilk bakışta yanlış görünür ("geçici hata") ama
/// `CGBitmapContextCreate`'in karakterle ilgili tek bir argümanı yok — hepsi
/// atlasın ömrü boyunca sabit, yani bir kez başarısızsa hep başarısız.
/// Önbelleğe **girmeseydi** her hücre her karede başarısız bir bağlam kurulumu
/// öderdi ve tek bir glyph bile çizilmezdi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cizim {
    Cizildi,
    /// Fontun bu karakter için glyph'i yok (`.notdef`).
    GlifYok,
    /// `CGBitmapContext` kurulamadı.
    BaglamYok,
}

/// `hedef`e `ch`'in kapsama (alfa) baytlarını çizer.
///
/// Tampon yalnız gerçekten çizim yapılacaksa sıfırlanır.
pub(crate) fn ciz(font: &CTFont, ch: char, m: Metrics, hedef: &mut [u8]) -> Cizim {
    // `debug_assert` değil: bu satır aşağıdaki `unsafe` bloğun ön koşulu.
    // CG'ye `width`/`height` `m`'den, işaretçi `hedef`ten gidiyor; ikisi
    // ayrışırsa CG kısa tamponun ötesine yazar ve release derlemede hiçbir şey
    // fark etmez — `make hepsi` sınamaları debug koşuyor.
    assert_eq!(hedef.len(), m.slot_bytes(), "tampon tam bir yuva olmalı");

    let Some(glif) = font::glif(font, ch) else {
        return Cizim::GlifYok;
    };

    let (w, h) = m.cell_wh();
    // Alfa-only bağlam: renk uzayı **yok** (`space: None`), bileşen başına
    // 8 bit, satır adımı tam hücre genişliği. Beyaz çizilen glyph'in kapsama
    // değeri doğrudan alfa baytı olur; ayrı bir kanal ayıklama adımı doğmaz
    // ve tampon zaten atlasın `R8Unorm` düzeninde.
    // SAFETY: `hedef` w*h bayt ve bağlam yaşadığı sürece (bu fonksiyonun
    // sonuna kadar) canlı; ölçüler tamponla tutarlı. Bağlam düştükten sonra
    // `hedef`e yalnız Rust tarafından erişilir.
    let ctx = unsafe {
        CGBitmapContextCreate(
            hedef.as_mut_ptr().cast::<c_void>(),
            w,
            h,
            8,
            w,
            None,
            CGImageAlphaInfo::Only.0,
        )
    };
    let Some(ctx) = ctx else {
        return Cizim::BaglamYok;
    };
    // Sıfırlama bağlam kurulduktan **sonra**: başarısız iki dalda çağıran
    // tampona hiç bakmıyor (tofu rezident ve dokuda), yani oradaki memset
    // tamamen boşa giderdi.
    hedef.fill(0);

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
    let taban = CGFloat::from(m.cell_px.1 - m.baseline_px);
    let konum = CGPoint::new(0.0, taban);
    // SAFETY: tek glyph, tek konum, sayı ikisiyle tutarlı; bağlam canlı.
    unsafe { font.draw_glyphs(NonNull::from(&glif), NonNull::from(&konum), 1, &ctx) };
    Cizim::Cizildi
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
}

/// Bir hücreye sığan tam dalga sayısı.
///
/// Periyot `cell_px.0 / DALGA_SAYISI` ve bu bölmenin **tam** olması şart:
/// sprite tek hücre genişliğinde ve komşularıyla döşeniyor, yani periyot
/// hücreyi tam bölmezse iki hücrenin sınırında faz kırılır ve çok hücreli bir
/// alt çizgi kesintili görünür. `1` bu kısıtı inşaen sağlıyor (periyot =
/// hücre genişliği) ve en yumuşak dalgayı veriyor; büyütülecekse `cell_px.0`'ı
/// bölen bir değer seçilmeli.
const DALGA_SAYISI: f32 = 1.0;

/// Kıvrımın dikey kaplamı, kalınlığın katı olarak.
///
/// Dalganın göz tarafından dalga olarak görülmesi için gereken en küçük
/// kaplam. Tabanı alt çizginin tabanına çakılı ve o zaten hücrenin içinde
/// (`font::kural_zarfi`), yani kıvrım inşaen içeride.
const KIVRIM_KAT: f32 = 3.0;

/// `hedef`e kural çizgisinin kapsama baytlarını çizer.
///
/// [`ciz`]'in kardeşi ama **CG kullanmıyor**: `tofu_tamponu` gibi doğrudan
/// bayt yazıyor. Üç kazanç — çizim deterministik (CG'nin antialias sürümüne
/// bağlı değil, sınama tam yapı assert edebilir), başarısızlık dalı hiç
/// doğmuyor (`BaglamYok` yok, dönüş `()`), ve font hiç sorulmuyor.
pub(crate) fn ciz_kural(kind: RuleKind, m: Metrics, hedef: &mut [u8]) {
    // audit: `ciz`'inkiyle aynı ön koşul, aynı gerekçe. Tampon `m`'den
    // boyutlandırılmış `self.tampon`, yani ayrışma yapısal olarak imkânsız;
    // assert onu sınırda ve adıyla yakalıyor, `bant`/`kivrim`'in döngüsünde
    // anlamsız bir indeks paniği olarak değil.
    assert_eq!(hedef.len(), m.slot_bytes(), "tampon tam bir yuva olmalı");
    // Tampon paylaşılıyor ve içinde bir önceki glyph'in pikselleri var;
    // sıfırlanmazsa kural çizgisinin altından o glyph görünür.
    hedef.fill(0);

    let (konum, kalinlik) = match kind {
        RuleKind::Strike => m.strikeout_px,
        _ => m.underline_px,
    };
    // Desen periyodu tam sayı aritmetiğinde kalıyor: `as usize` turu doğmuyor.
    let kal = usize::from(kalinlik);
    let (w, h) = m.cell_wh();
    let (konum, kalinlik) = (f32::from(konum), f32::from(kalinlik));

    match kind {
        // Kesintisiz desen: periyot 1, dolu 1.
        RuleKind::Single | RuleKind::Strike => bant(hedef, m, konum, kalinlik, 1, 1),
        RuleKind::Double => {
            bant(hedef, m, konum, kalinlik, 1, 1);
            // İkinci çizgi **önce aşağıya**. Alt çizgi ile hücre tabanı
            // arasındaki satırlar boş (13pt: çizgi 14, hücre 17 → 15-16 boş)
            // ve orası glyph gövdesinden uzak. Yukarı taşımak `a e o` gibi
            // harflerin son gövde satırına girer ve iki çizgi ayrı görünmek
            // yerine harflerin dibine yapışık tek kalın çizgi gibi okunur.
            // Aşağıda yer yoksa yukarı düşülür.
            let asagi = konum + 2.0 * kalinlik;
            let ikinci = if asagi + kalinlik <= h as f32 {
                asagi
            } else {
                (konum - 2.0 * kalinlik).max(0.0)
            };
            bant(hedef, m, ikinci, kalinlik, 1, 1);
        }
        // Nokta ve kesik: periyot kalınlığa bağlı, yani punto büyüdükçe desen
        // de büyüyor ve @2x'te sıkışmış görünmüyor. Alt sınır gerekmiyor —
        // `font::kural_zarfi` kalınlığı zaten `>= 1`'e bağlıyor.
        RuleKind::Dotted => {
            let p = bolen_periyot(2 * kal, w);
            bant(hedef, m, konum, kalinlik, p, (p / 2).max(1));
        }
        RuleKind::Dashed => {
            let p = bolen_periyot(6 * kal, w);
            bant(hedef, m, konum, kalinlik, p, (p * 2 / 3).max(1));
        }
        RuleKind::Curl => kivrim(hedef, m, konum, kalinlik),
    }
}

/// İstenen periyodu hücre genişliğini **tam bölen** en yakın değere yuvarlar.
///
/// `DALGA_SAYISI`'nın kıvrım için taşıdığı kısıtın nokta/kesik karşılığı ve
/// aynı sebeple var: sprite tek hücre genişliğinde, komşularıyla döşeniyor ve
/// `x % periyot` deseni hücre sınırında faz kırar. Ölçüldü (bu makine,
/// Menlo 13pt@1x): `w = 8`, `Dashed`'in istediği periyot 6 → `8 % 6 = 2`,
/// yani iki komşu hücrede tire uzunlukları farklı görünürdü. Kıvrımda kısıt
/// uygulanıp burada uygulanmaması bir gözden kaçmaydı.
pub(crate) fn bolen_periyot(istenen: usize, w: usize) -> usize {
    let istenen = istenen.clamp(1, w.max(1));
    (istenen..=w).find(|p| w % p == 0).unwrap_or(w.max(1))
}

/// Yatay bant: `[ust, ust + kalinlik)` satırlarını `desen`in kabul ettiği
/// sütunlarda boyar. Kısmi kaplanan satır **kısmi alfa** alıyor — kalınlık
/// tam sayı olmak zorunda değil ve kenar yumuşatması bedava geliyor.
fn bant(hedef: &mut [u8], m: Metrics, ust: f32, kalinlik: f32, periyot: usize, dolu: usize) {
    let (w, h) = m.cell_wh();
    let (y0, y1) = (ust, ust + kalinlik);
    for y in 0..h {
        let deger = kaplama(y, y0, y1);
        if deger == 0 {
            continue;
        }
        for x in 0..w {
            if x % periyot < dolu {
                // `max`: `Double`'ın iki bandı çakışırsa koyu olan kazanır.
                // audit: `y < h` ve `x < w`, yani indeks `w * h`'nin altında.
                hedef[y * w + x] = hedef[y * w + x].max(deger);
            }
        }
    }
}

/// `[y, y+1)` pikselinin `[y0, y1)` bandıyla kesişimi → alfa baytı.
///
/// Tek sahip: `bant` ve `kivrim` aynı kenar yumuşatma kuralını kullanmak
/// zorunda, yoksa kıvrım ötekilerden farklı yumuşaklık alır ve belirti
/// sessizdir — beş çeşidi karşılaştıran sınama "farklı olsunlar" dediği için
/// bunu göremez.
fn kaplama(y: usize, y0: f32, y1: f32) -> u8 {
    let oran = (y1.min(y as f32 + 1.0) - y0.max(y as f32)).clamp(0.0, 1.0);
    (oran * 255.0).round() as u8
}

/// Kıvrımlı çizgi: bandın merkezi sütun boyunca sinüsle salınıyor.
///
/// Dalga bandının **altı alt çizginin altına çakılı** (`konum + kalinlik`).
/// O sınır `font::kural_zarfi` tarafından zaten hücrenin içine oturtulduğu
/// için kıvrım da inşaen içeride; ayrıca kırpılmasına gerek yok.
fn kivrim(hedef: &mut [u8], m: Metrics, konum: f32, kalinlik: f32) {
    let (w, h) = m.cell_wh();
    // Dalga bandı alt çizgiden **aşağı** doğru büyüyor, yukarı değil: alt
    // çizgi ile hücre tabanı arasındaki satırlar boş ve orası glyph
    // gövdesinden uzak. Yukarı büyüseydi dalganın tepesi harf tabanlarıyla
    // birleşirdi (13pt: tepe 12 = `a e o`'nun son gövde satırı).
    // Genlik burada türüyor, `Metrics`'te değil: fonttan gelen bir ölçü değil,
    // bu çizicinin tasarım sabiti. `Metrics` "fonttan türeyen hücre
    // geometrisi" olarak kalıyor — tek tüketicisi olan bir sabiti `pub` bir
    // alana koymak onu `bt-gpu`'ya da gösterirdi.
    let ust = konum;
    let alt = (konum + KIVRIM_KAT * kalinlik).min(h as f32);
    // Merkez ekseni bandın içinde kalsın: yarım kalınlık pay bırakılıyor.
    let (y_alt, y_ust) = (alt - kalinlik / 2.0, ust + kalinlik / 2.0);
    let orta = (y_alt + y_ust) / 2.0;
    let genlik = (y_alt - y_ust) / 2.0;
    for x in 0..w {
        // Piksel **merkezinden** örnekleniyor ve bir hücreye tam
        // `DALGA_SAYISI` dalga sığıyor: sprite komşularıyla döşendiğinde faz
        // kırılmıyor (bkz. `DALGA_SAYISI`).
        let faz = core::f32::consts::TAU * (x as f32 + 0.5) * DALGA_SAYISI / w as f32;
        let merkez = orta + genlik * faz.sin();
        let (y0, y1) = (merkez - kalinlik / 2.0, merkez + kalinlik / 2.0);
        for y in 0..h {
            let deger = kaplama(y, y0, y1);
            if deger > 0 {
                // audit: `y < h` ve `x < w`.
                hedef[y * w + x] = deger;
            }
        }
    }
}
