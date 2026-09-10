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

    let (w, h) = (usize::from(m.cell_px.0), usize::from(m.cell_px.1));
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
