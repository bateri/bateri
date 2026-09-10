//! Font zinciri ve hücre metriği.
//!
//! Zincirin can alıcı noktası şu: `CTFontCreateWithName` **hata vermez**.
//! İstenen aile yoksa CoreText elindeki en yakın fontu döndürür ve çağıran
//! hiçbir şey fark etmez. Bu yüzden "font açıldı" bir kanıt değildir; açılan
//! fontun kendi bildirdiği aile adı istenenle karşılaştırılır.

use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFRetained, CFString, CGFloat, CGSize};
use objc2_core_graphics::CGGlyph;
use objc2_core_text::{CTFont, CTFontOrientation};

/// Tercih sırası. Bulunamayan ad **sessizce** atlanır: SF Mono Xcode ile
/// gelir, her makinede yoktur ve yokluğu bir kusur değil tasarlanmış bir geri
/// düşüştür. Uyarı yalnız tabanın da ikame edilmesi hâlinde anlamlı.
const TERCIHLER: [&str; 1] = ["SF Mono"];

/// Garanti taban: macOS'un her sürümünde kurulu. Ayrı bir sabit olmasının
/// sebebi tip düzeyinde bir güvence — zincir boş dönemez, dolayısıyla
/// `Option`/`expect` yolu hiç doğmaz.
const TABAN: &str = "Menlo";

/// Hücre ölçüsü, **fiziksel piksel**.
///
/// `Atlas::new`'in `scale` parametresi punto ile çarpılıp fonta girer, yani
/// ekran ölçeği buradaki sayıların içindedir. Ölçeğin anahtarın parçası
/// olması şart: @1x'te rasterize edilmiş glyph @2x'te **hatasız** bulanıklaşır
/// ve belirti yalnız iki ekranlı makinede görünür (discussion.md → Muhakeme).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Metrics {
    /// (genişlik, yükseklik).
    pub cell_px: (u16, u16),
    /// Hücrenin **üstünden** taban çizgisine piksel; glyph oradan oturur.
    /// [`Metrics::cell_px`]'in yüksekliğini aşmaz — `metrics()` sınırlıyor.
    pub baseline_px: u16,
}

impl Metrics {
    /// Tek yuvanın bayt sayısı (`R8`: piksel başına bir bayt).
    ///
    /// Yuva geometrisinin **tek sahibi** burası: atlasın tamponu, tofu çizimi
    /// ve raster hedefi üçü de bunu okuyor. Geometri değişirse (kenar payı,
    /// hizalama dolgusu) düzeltilecek tek aritmetik nokta var; üçe dağılmış
    /// olsaydı biri unutulduğunda tamponlar sessizce ayrışırdı.
    pub fn slot_bytes(self) -> usize {
        usize::from(self.cell_px.0) * usize::from(self.cell_px.1)
    }
}

/// Adı verilen aileyi açar ve CoreText'in gerçekten verdiği aile adını
/// **birlikte** döndürür. İkisi ayrışıyorsa istenen font makinede yok.
pub(crate) fn ac(ad: &str, punto: CGFloat) -> (CFRetained<CTFont>, String) {
    let istenen = CFString::from_str(ad);
    // SAFETY: `matrix` null → birim matris; `CTFontCreateWithName` bunu
    // açıkça destekliyor ve dönüş non-null.
    let font = unsafe { CTFont::with_name(&istenen, punto, ptr::null()) };
    // SAFETY: `font` az önce yaratıldı ve bu kapsamda canlı.
    let donen = unsafe { font.family_name() };
    (font, donen.to_string())
}

/// Zinciri yürür: ilk gerçekten bulunan tercih, yoksa [`TABAN`].
pub(crate) fn zincirden_ac(punto: CGFloat) -> CFRetained<CTFont> {
    for ad in TERCIHLER {
        let (font, donen) = ac(ad, punto);
        if donen == ad {
            return font;
        }
    }
    let (font, donen) = ac(TABAN, punto);
    if donen != TABAN {
        // Buraya düşülmesi beklenmez. Düşülürse metrik ve glyph'ler bilinmeyen
        // bir fonttan gelir; sessiz kalırsa yanlış hücre boyutu "her şey
        // normal" gibi görünür. Süreç çıktısı, UI dizgisi değil: Türkçe, ve
        // öneki depodaki öteki stderr satırlarıyla aynı (`bateri:`) — ayrı bir
        // önek, `bateri` diye süzen okuyucunun tam da bu satırı kaçırması
        // demek olurdu.
        eprintln!("bateri: '{TABAN}' bulunamadı, CoreText '{donen}' ikame etti");
    }
    font
}

/// Karakterin glyph numarası; font karakteri tanımıyorsa `None`.
pub(crate) fn glif(font: &CTFont, ch: char) -> Option<CGGlyph> {
    let mut utf16 = [0u16; 2];
    let birim_sayisi = ch.encode_utf16(&mut utf16).len();
    let mut glifler = [0 as CGGlyph; 2];
    // İşaretçiler **dilimden** türetiliyor, `&dizi[0]`'dan değil: BMP dışı bir
    // karakterde `birim_sayisi` 2 ve CoreText ikinci elemana da dokunuyor
    // (düşük vekili okur, karşılığına 0 yazar). Tek elemanlık bir referanstan
    // türetilen işaretçinin provenance'ı o ikinci erişimi kapsamaz — bugün
    // çalışır, aliasing modeline göre tanımsızdır.
    // SAFETY: iki dilim de iki eleman taşıyor ve bu kapsamda canlı;
    // `birim_sayisi` ≤ 2, yani sayı ikisiyle de tutarlı.
    let _ = unsafe {
        font.glyphs_for_characters(
            NonNull::from(&mut utf16[..]).cast::<u16>(),
            NonNull::from(&mut glifler[..]).cast::<CGGlyph>(),
            birim_sayisi as isize,
        )
    };
    // Dönüş değeri **ölçüt değil**: surrogate çiftinde ikinci UTF-16 birimi
    // için glyph üretilmez ve fonksiyon `false` döner, oysa glyph birinci
    // birimdedir ve geçerlidir. Tek ölçüt `.notdef` (0) mü sorusu.
    (glifler[0] != 0).then_some(glifler[0])
}

/// Hücre ölçüsünü fontun kendi metriğinden türetir.
pub(crate) fn metrics(font: &CTFont) -> Metrics {
    // SAFETY: `font` canlı; üçü de saf okuma.
    let (ascent, descent, leading) = unsafe { (font.ascent(), font.descent(), font.leading()) };
    // Yükseklik iki parçanın **ayrı ayrı** yuvarlanıp toplanmasıyla bulunuyor,
    // `yukari(ascent + descent + leading)` ile değil. Fark ölçülebilir bir
    // kırpmaydı: bu makinede Menlo 13pt ascent 12.067, descent 3.066 veriyor
    // ve toplamı yukarı yuvarlamak 16 ediyor — taban 13'e oturunca alta 3
    // piksel kalıyor, oysa font 3.066 istiyor. Kaybedilen şey `g j p q y ,`
    // altındaki son kapsama satırı; belirti "yazı biraz garip" olurdu. Sayılar
    // font sürümüne bağlı ve eskiyebilir, **iddia eskimez**: bekçisi
    // `descender_hucreye_sigar` ve o metriği fontun kendisinden okuyor.
    let taban = yukari(ascent);
    let cell_px = (
        yukari(bosluk_advance(font)),
        // `saturating_add`: iki parça da `u16::MAX`'e kadar çıkabiliyor.
        taban.saturating_add(yukari(descent + leading)),
    );
    Metrics {
        cell_px,
        // Taban hücrenin içinde kalıyor ve bu artık bir dilek değil sonuç:
        // alt parça `yukari` yüzünden en az 1, yani `baseline_px < cell_px.1`.
        // `raster`'ın `cell_h - baseline` çıkarması bu yüzden taşmıyor.
        baseline_px: taban,
    }
}

/// Boşluğun yatay advance'i — hücre genişliği.
///
/// Monospace varsayımı zincirin kendisinde (SF Mono / Menlo). Ölçülen karakter
/// boşluk çünkü her fontta var; seçim gövdede sabit, çünkü başka bir karakterle
/// çağrılması hücre genişliğini fontun o harfine bağlamak olurdu.
fn bosluk_advance(font: &CTFont) -> CGFloat {
    let Some(glif) = glif(font, ' ') else {
        return 0.0;
    };
    let mut olcu = [CGSize::ZERO; 1];
    // SAFETY: tek glyph, tek ölçü hücresi; sayı ikisiyle de tutarlı.
    unsafe {
        font.advances_for_glyphs(
            CTFontOrientation::Horizontal,
            NonNull::from(&glif),
            olcu.as_mut_ptr(),
            1,
        );
    }
    olcu[0].width
}

/// Yukarı yuvarlar ve `u16`'ya sıkıştırır.
///
/// Alt sınır 1: bozuk ya da bulunamayan bir fontta metrik sıfır dönebilir ve
/// sıfır genişlikli hücre ızgarayı sıfıra böler. Üst sınır tipin kendisi.
///
/// NaN ayrıca ele alınıyor çünkü `clamp` onu **geçirir** ve `NaN as u16` 0
/// eder: alt sınır sessizce delinir ve hata bölmede patlar, kaynağında değil.
fn yukari(v: CGFloat) -> u16 {
    if !v.is_finite() {
        return 1;
    }
    v.ceil().clamp(1.0, f64::from(u16::MAX)) as u16
}
