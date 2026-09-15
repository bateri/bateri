//! Font zinciri ve hücre metriği.
//!
//! Zincirin can alıcı noktası şu: `CTFontCreateWithName` **hata vermez**.
//! İstenen aile yoksa CoreText elindeki en yakın fontu döndürür ve çağıran
//! hiçbir şey fark etmez. Bu yüzden "font açıldı" bir kanıt değildir; açılan
//! fontun kendi bildirdiği aile adı istenenle karşılaştırılır.

use std::ptr::{self, NonNull};

use objc2_core_foundation::{CFRetained, CFString, CGFloat, CGSize};
use objc2_core_graphics::CGGlyph;
use objc2_core_text::{CTFont, CTFontOrientation, CTFontSymbolicTraits};

/// Tercih sırası. Bulunamayan ad **sessizce** atlanır: SF Mono Xcode ile
/// gelir, her makinede yoktur ve yokluğu bir kusur değil tasarlanmış bir geri
/// düşüştür. Uyarı yalnız tabanın da ikame edilmesi hâlinde anlamlı.
const PREFERRED: [&str; 1] = ["SF Mono"];

/// Garanti taban: macOS'un her sürümünde kurulu. Ayrı bir sabit olmasının
/// sebebi tip düzeyinde bir güvence — zincir boş dönemez, dolayısıyla
/// `Option`/`expect` yolu hiç doğmaz.
const FALLBACK: &str = "Menlo";

/// Font yüzü — **tipografi kavramı**, SGR bayrağı değil.
///
/// `bt-core`'un `bold`/`italic` bayraklarıyla dört varyantı aynı, **sebepleri
/// ayrı**: oradaki terminal semantiği (SGR 1 / SGR 3), buradaki CoreText
/// trait'i. İkisini "aynı görünüyorlar" diye tek tipte birleştirmek
/// `bt-atlas`'a bir `bt-core` kenarı eklemek demek olurdu ve o kenar
/// `alacritty_terminal`'i saf-CoreText crate'ine çeker. Çeviri `bt-gpu`'da,
/// ikisini birden gören tek katmanda.
// `repr(u8)`: bkz. `RuleKind` — anahtar `slot()`'un sıcak yolunda.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Face {
    #[default]
    Regular = 0,
    Bold = 1,
    Italic = 2,
    BoldItalic = 3,
}

impl Face {
    /// Uyarı metninde geçen ad.
    fn name(self) -> &'static str {
        match self {
            Face::Regular => "Regular",
            Face::Bold => "Bold",
            Face::Italic => "Italic",
            Face::BoldItalic => "BoldItalic",
        }
    }

    /// Yüzün CoreText trait maskesi.
    ///
    /// `Regular` **çağrılmaz**: düz yüz türetilmiyor, zincirden geliyor.
    /// Boş maske dönseydi `derive_face` onu "gerçek yüz değil" sentineli olarak
    /// okumak zorunda kalır ve `None` iki anlam taşırdı.
    fn traits(self) -> CTFontSymbolicTraits {
        match self {
            // audit: ulaşılamaz ve bunu **modül sınırı** koruyor, çağıran
            // disiplini değil: `derive_face` font.rs'e özel ve tek çağıranı
            // `[Bold, Italic, BoldItalic]` üzerinde dönüyor. `pub(crate)`
            // olsaydı crate içinden `Face::Regular` ile çağıran biri
            // derleyiciden uyarı almadan buraya düşer, panik de `slot()`
            // üzerinden ana thread'de kareyi düşürürdü.
            Face::Regular => unreachable!("düz yüz türetilmiyor, zincirden geliyor"),
            Face::Bold => CTFontSymbolicTraits::TraitBold,
            Face::Italic => CTFontSymbolicTraits::TraitItalic,
            Face::BoldItalic => CTFontSymbolicTraits::TraitBold | CTFontSymbolicTraits::TraitItalic,
        }
    }
}

/// Dört yüz, `Face` sırasında. Düz yüz zincirden, ötekiler ondan türer.
pub(crate) struct Faces {
    fonts: [CFRetained<CTFont>; 4],
    /// Gerçekten **edinilen** yüzler; edinilemeyen düz yüze çökmüş demektir.
    ///
    /// Bu bilgi saklanmasaydı çağıran hangi yüzü aldığını bilemezdi ve
    /// [`Faces::effective`]'in kapattığı yuva israfı sessizce açık kalırdı.
    acquired: [bool; 4],
}

/// İstenen ailenin kullanıcıya söylenmesi gereken sonucu.
///
/// **İkisi birden olamaz:** bulunamayan ailenin yerine zincir açılıyor ve
/// zincirin iki fontu da eşaralıklı. Tip bu yüzden liste değil.
///
/// Metin yok: bu crate UI dizgisi kurmuyor, yalnız olguyu veriyor. Tip
/// `bt-atlas`'ın dışına çıkmaz — `bt-gpu` kendi bildirim tipine çevirir
/// (`bt-shell` bu crate'i görmüyor, 003 R5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontIssue {
    /// Aile makinede yok; `using` zincirin gerçekten açtığı ailenin adı.
    FamilyNotFound { requested: String, using: String },
    /// Aile açıldı ama CoreText onu eşaralıklı saymıyor. **Reddedilmiyor**:
    /// hücre boşluğun genişliğinden türüyor ve harfler hücreye kırpılarak
    /// çiziliyor, yani bozuk ama çalışan bir ekran. `family` CoreText'in
    /// bildirdiği ad.
    NotMonospaced { family: String },
}

impl Faces {
    /// Zincirden açar ([`open_chain`]) ve üç yüzü türetir.
    pub(crate) fn from_chain(
        family: Option<&str>,
        point_size: CGFloat,
    ) -> (Self, Option<FontIssue>) {
        let (regular, issue) = open_chain(family, point_size);
        (Self::derive(regular), issue)
    }

    /// Verilen düz yüzden türetir.
    ///
    /// Ayrı bir kurucu, sınama için: zincirin tabanı (Menlo) dört yüzü de
    /// taşıyor, yani geri düşüş dalı gerçek bir fontla ancak **tek yüzlü** bir
    /// aile verilerek ateşlenebiliyor.
    pub(crate) fn derive(regular: CFRetained<CTFont>) -> Self {
        let mut fonts = [regular.clone(), regular.clone(), regular.clone(), regular];
        let mut acquired = [true, false, false, false];
        let mut missing: Vec<&str> = Vec::new();
        for face in [Face::Bold, Face::Italic, Face::BoldItalic] {
            match derive_face(&fonts[Face::Regular as usize], face) {
                Some(font) => {
                    fonts[face as usize] = font;
                    acquired[face as usize] = true;
                }
                None => missing.push(face.name()),
            }
        }
        if !missing.is_empty() {
            // Atlas kurulumunda bir kez — `slot()` çizim yolunda ve orada
            // basılan bir satır kare başına tekrarlanırdı. **"Ömürde bir kez"
            // değil:** `Atlas::ensure` aile/punto/ölçek değişince atlası (ve bunu)
            // yeniden kuruyor, yani pencere Retina ile harici ekran arasında
            // taşınırsa satır tekrar düşer. Kabul edilen bedel; susturmak
            // `Faces`'in dışında kalıcı bir durum ister. Önek
            // `open_chain`'ınkiyle aynı (`bateri:`), aynı gerekçeyle.
            eprintln!(
                "bateri: font ailesinde {} yüzü yok, düz yüz kullanılıyor",
                missing.join(", ")
            );
        }
        Self { fonts, acquired }
    }

    pub(crate) fn get(&self, face: Face) -> &CTFont {
        &self.fonts[face as usize]
    }

    /// Yuva anahtarına girecek yüz: **edinilemeyen yüz `Regular`'a çöker**.
    ///
    /// Anahtarın istenen yüzü değil **çizilen** yüzü taşıması şart. Tek yüzlü
    /// bir ailede (`Monaco`) `(Char, Bold)` ile `(Char, Regular)` bayt bayt
    /// aynı bitmap'i iki ayrı yuvada tutardı; dört yüzle atlas dört kat hızlı
    /// dolar, fazlalık glyph'ler tofu'ya düşer ve belirti sessizdir.
    /// `Sprite::Rule`'un `Regular`'a indirilmesiyle aynı olgunun ikinci yüzü:
    /// istenen yüz ile çizilen yüz aynı olmak zorunda değil.
    pub(crate) fn effective(&self, face: Face) -> Face {
        // Merdiven, düz düşüş değil: `BoldItalic`'i doğrudan `Regular`'a
        // indirmek **kalınlığı da** düşürürdü. Gerçek bir `Bold Italic` yüzü
        // olmayan ama `Bold` taşıyan aile yaygın; orada SGR 1;3 metni düz
        // çıkardı, oysa kalın yüz elde mevcut.
        let ladder: &[Face] = match face {
            Face::BoldItalic => &[Face::BoldItalic, Face::Bold, Face::Italic],
            Face::Bold => &[Face::Bold],
            Face::Italic => &[Face::Italic],
            Face::Regular => &[],
        };
        ladder
            .iter()
            .copied()
            .find(|&f| self.acquired[f as usize])
            .unwrap_or(Face::Regular)
    }
}

/// `regular`den `face`in yüzünü türetir; edinemezse `None`.
///
/// Denetim **iki kapılı ve aile adı karşılaştırması yapmıyor**. Aile
/// karşılaştırması burada totoloji olurdu: API'nin sözleşmesi zaten "aynı
/// ailede yeni bir font, yoksa NULL" ve `Menlo-Bold`'un ailesi `Menlo`.
///
/// 1. **`nil` mi** — tipte, `Option` olarak geliyor.
/// 2. **İstenen trait'i gerçekten edindi mi** — CoreText istenen yüzü
///    bulamazsa **düz yüzü geri verebiliyor** ve o sessiz ikame,
///    `open_chain`'ın `CTFontCreateWithName` için yaşadığı hatanın ta
///    kendisi. Tek fark: orada aile adına, burada trait maskesine bakılıyor.
fn derive_face(regular: &CTFont, face: Face) -> Option<CFRetained<CTFont>> {
    let wanted = face.traits();
    // SAFETY: `regular` canlı; `matrix` null geçerli. **Dikkat:** copy ailesinde
    // null "birim matris" değil, **kaynak fontun matrisi korunur** demek —
    // `open()`'taki `CTFontCreateWithName` gerekçesiyle karıştırılmamalı, orada
    // null gerçekten birim matristir. İstenen de bu: türetilen yüz kaynağın
    // dönüşümünü aynen taşısın, yoksa bir gün matrisli bir font zincire
    // girdiğinde eğim iki kez uygulanır. `size` 0.0 → kaynağın puntosu korunur.
    let font = unsafe { regular.copy_with_symbolic_traits(0.0, ptr::null(), wanted, wanted) }?;
    // SAFETY: `font` az önce yaratıldı ve bu kapsamda canlı.
    let returned = unsafe { font.symbolic_traits() };
    returned.contains(wanted).then_some(font)
}

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
    /// Alt çizgi: (hücrenin üstünden konum, kalınlık).
    ///
    /// `konum + kalınlık` **asla** `cell_px.1`'i aşmaz — [`rule_envelope`]
    /// sınırlıyor. Aşsaydı çizgi komşu satırın tepesinde belirirdi ve belirti
    /// sessiz olurdu.
    pub underline_px: (u16, u16),
    /// Üstü çizili: (konum, kalınlık). Aynı güvence.
    pub strikeout_px: (u16, u16),
}

impl Metrics {
    /// Tek yuvanın bayt sayısı (`R8`: piksel başına bir bayt).
    ///
    /// Yuva geometrisinin **tek sahibi** burası: atlasın tamponu, tofu çizimi
    /// ve raster hedefi üçü de bunu okuyor. Geometri değişirse (kenar payı,
    /// hizalama dolgusu) düzeltilecek tek aritmetik nokta var; üçe dağılmış
    /// olsaydı biri unutulduğunda tamponlar sessizce ayrışırdı.
    pub fn slot_bytes(self) -> usize {
        let (w, h) = self.cell_wh();
        w * h
    }

    /// Hücre ölçüsü `usize` olarak — indeksleme ve döngü sınırı için.
    ///
    /// [`Metrics::slot_bytes`] ile aynı gerekçe: açımı dörde dağıtmak yerine
    /// tek sahipte tutuyor.
    pub(crate) fn cell_wh(self) -> (usize, usize) {
        (usize::from(self.cell_px.0), usize::from(self.cell_px.1))
    }
}

/// Adı verilen aileyi açar ve CoreText'in gerçekten verdiği aile adını
/// **birlikte** döndürür. İkisi ayrışıyorsa istenen font makinede yok.
pub(crate) fn open(name: &str, point_size: CGFloat) -> (CFRetained<CTFont>, String) {
    let wanted = CFString::from_str(name);
    // SAFETY: `matrix` null → birim matris; `CTFontCreateWithName` bunu
    // açıkça destekliyor ve dönüş non-null.
    let font = unsafe { CTFont::with_name(&wanted, point_size, ptr::null()) };
    // SAFETY: `font` az önce yaratıldı ve bu kapsamda canlı.
    let returned = unsafe { font.family_name() };
    (font, returned.to_string())
}

/// Zinciri yürür: istenen aile (varsa), sonra [`PREFERRED`], sonra
/// [`FALLBACK`]. Kullanıcıya söylenecek bir şey varsa ikinci değerde.
///
/// İstenen aile de öteki halkalar gibi **dönen adla** sınanıyor
/// ([`same_family`]): olmayan ad için CoreText bu makinede Helvetica
/// veriyor, yani sınanmasaydı yanlış yazılmış her ad orantılı bir fontla
/// açılırdı ve belirti "eşaralıklı değil" uyarısı olurdu — asıl hatayı değil
/// bir yan etkisini söyleyen.
pub(crate) fn open_chain(
    family: Option<&str>,
    point_size: CGFloat,
) -> (CFRetained<CTFont>, Option<FontIssue>) {
    let Some(requested) = family else {
        return (open_default(point_size).0, None);
    };
    let (font, returned) = open(requested, point_size);
    if !same_family(&returned, requested) {
        let (font, using) = open_default(point_size);
        let issue = FontIssue::FamilyNotFound {
            requested: requested.to_owned(),
            using,
        };
        return (font, Some(issue));
    }
    // SAFETY: `font` az önce yaratıldı ve bu kapsamda canlı.
    let traits = unsafe { font.symbolic_traits() };
    let issue = (!traits.contains(CTFontSymbolicTraits::TraitMonoSpace))
        .then_some(FontIssue::NotMonospaced { family: returned });
    (font, issue)
}

/// CoreText'in bildirdiği aile adı istenen ad mı — **harf duyarsız**.
///
/// CoreText adı harf duyarsız buluyor (`"menlo"` → `Menlo`, ölçüldü) ama
/// adı kendi yazımıyla bildiriyor; birebir karşılaştırma bulunan fontu yok
/// sayardı. PostScript adı (`Menlo-Regular`) **eşleşmez**: CoreText onu da
/// açıyor ama aile adı başka, ve o ad ailenin tek bir yüzünü söylüyor —
/// ayarın istediği aile (`docs/AYARLAR.md`).
fn same_family(returned: &str, requested: &str) -> bool {
    returned.to_lowercase() == requested.to_lowercase()
}

/// Ayar aile istemediğinde açılan font ve CoreText'in bildirdiği adı.
pub(crate) fn open_default(point_size: CGFloat) -> (CFRetained<CTFont>, String) {
    for name in PREFERRED {
        let (font, returned) = open(name, point_size);
        if returned == name {
            return (font, returned);
        }
    }
    let (font, returned) = open(FALLBACK, point_size);
    if returned != FALLBACK {
        // Buraya düşülmesi beklenmez. Düşülürse metrik ve glyph'ler bilinmeyen
        // bir fonttan gelir; sessiz kalırsa yanlış hücre boyutu "her şey
        // normal" gibi görünür. Süreç çıktısı, UI dizgisi değil: Türkçe, ve
        // öneki depodaki öteki stderr satırlarıyla aynı (`bateri:`) — ayrı bir
        // önek, `bateri` diye süzen okuyucunun tam da bu satırı kaçırması
        // demek olurdu.
        eprintln!("bateri: '{FALLBACK}' bulunamadı, CoreText '{returned}' ikame etti");
    }
    (font, returned)
}

/// Karakterin glyph numarası; font karakteri tanımıyorsa `None`.
pub(crate) fn glyph_index(font: &CTFont, ch: char) -> Option<CGGlyph> {
    let mut utf16 = [0u16; 2];
    let unit_count = ch.encode_utf16(&mut utf16).len();
    let mut glyphs = [0 as CGGlyph; 2];
    // İşaretçiler **dilimden** türetiliyor, `&dizi[0]`'dan değil: BMP dışı bir
    // karakterde `unit_count` 2 ve CoreText ikinci elemana da dokunuyor
    // (düşük vekili okur, karşılığına 0 yazar). Tek elemanlık bir referanstan
    // türetilen işaretçinin provenance'ı o ikinci erişimi kapsamaz — bugün
    // çalışır, aliasing modeline göre tanımsızdır.
    // SAFETY: iki dilim de iki eleman taşıyor ve bu kapsamda canlı;
    // `unit_count` ≤ 2, yani sayı ikisiyle de tutarlı.
    let _ = unsafe {
        font.glyphs_for_characters(
            NonNull::from(&mut utf16[..]).cast::<u16>(),
            NonNull::from(&mut glyphs[..]).cast::<CGGlyph>(),
            unit_count as isize,
        )
    };
    // Dönüş değeri **ölçüt değil**: surrogate çiftinde ikinci UTF-16 birimi
    // için glyph üretilmez ve fonksiyon `false` döner, oysa glyph birinci
    // birimdedir ve geçerlidir. Tek ölçüt `.notdef` (0) mü sorusu.
    (glyphs[0] != 0).then_some(glyphs[0])
}

/// Hücre ölçüsünü fontun kendi metriğinden türetir.
pub(crate) fn metrics(font: &CTFont) -> Metrics {
    // SAFETY: `font` canlı; üçü de saf okuma.
    let (ascent, descent, leading) = unsafe { (font.ascent(), font.descent(), font.leading()) };
    // Yükseklik iki parçanın **ayrı ayrı** yuvarlanıp toplanmasıyla bulunuyor,
    // `round_up(ascent + descent + leading)` ile değil. Fark ölçülebilir bir
    // kırpmaydı: bu makinede Menlo 13pt ascent 12.067, descent 3.066 veriyor
    // ve toplamı yukarı yuvarlamak 16 ediyor — taban 13'e oturunca alta 3
    // piksel kalıyor, oysa font 3.066 istiyor. Kaybedilen şey `g j p q y ,`
    // altındaki son kapsama satırı; belirti "yazı biraz garip" olurdu. Sayılar
    // font sürümüne bağlı ve eskiyebilir, **iddia eskimez**: bekçisi
    // `descender_fits_in_the_cell` ve o metriği fontun kendisinden okuyor.
    let baseline = round_up(ascent);
    let cell_px = (
        round_up(space_advance(font)),
        // `saturating_add`: iki parça da `u16::MAX`'e kadar çıkabiliyor.
        baseline.saturating_add(round_up(descent + leading)),
    );
    // SAFETY: `font` canlı; üçü de saf okuma.
    let (u_pos, u_thick, x_h) = unsafe {
        (
            font.underline_position(),
            font.underline_thickness(),
            font.x_height(),
        )
    };
    // CoreText'in `underline_position`'ı **negatif**: taban çizgisinin altını
    // gösteriyor. Hücrenin üstünden ölçülen konuma çevirirken işaret çevriliyor.
    let thickness = round_up(u_thick);
    let underline_px = rule_envelope(
        baseline.saturating_add(round_up(-u_pos)),
        thickness,
        cell_px.1,
    );
    // Üstü çizilinin CoreText karşılığı **yok**; x-yüksekliğinin yarısı kadar
    // taban çizgisinin üstü, tipografide olağan yer. `saturating_sub`: küçük
    // puntoda x-yüksekliği tabanı aşabilir.
    let strikeout_px = rule_envelope(
        baseline.saturating_sub(round_up(x_h / 2.0)),
        thickness,
        cell_px.1,
    );
    Metrics {
        cell_px,
        // Taban hücrenin içinde kalıyor ve bu artık bir dilek değil sonuç:
        // alt parça `round_up` yüzünden en az 1, yani `baseline_px < cell_px.1`.
        // `raster`'ın `cell_h - baseline` çıkarması bu yüzden taşmıyor.
        baseline_px: baseline,
        underline_px,
        strikeout_px,
    }
}

/// Kural çizgisini hücrenin **içine** oturtur: (üstten konum, kalınlık).
///
/// Dönüşün değişmezi `konum + kalınlık <= cell_h`. Bu depoda bugünkü fontla
/// (Menlo) sınır hiç zorlanmıyor — 13pt'de alt çizgi 14+1, hücre 17 — ama
/// kırpma bir dilek değil sözleşme: `underline_position` fontun kendi
/// verisidir ve descent'i dar bir font çizgiyi hücrenin dışına atabilir.
/// Belirti sessizdir: bir satırın alt çizgisi bir alttaki satırın tepesinde
/// belirir. Bekçisi `envelope_stays_inside_cell` ve o **sentetik** girdiyle
/// sınıyor, çünkü gerçek font bu dalı hiç ateşlemiyor.
pub(crate) fn rule_envelope(top: u16, thickness: u16, cell_h: u16) -> (u16, u16) {
    // Sıfır yüksekliğe sığan kural yok. `metrics()` üzerinden buraya
    // düşülemiyor (`round_up` her ölçüyü >= 1'e sabitliyor) ama fonksiyonun tek
    // varlık sebebi değişmezi taşımak: onu hem yazıp hem delmemeli.
    if cell_h == 0 {
        return (0, 0);
    }
    // Kalınlık hücreyi aşamaz; en az 1 — çizilmeyen çizgi kural değildir.
    let thickness = thickness.clamp(1, cell_h);
    (top.min(cell_h - thickness), thickness)
}

/// Boşluğun yatay advance'i — hücre genişliği.
///
/// Monospace varsayımı zincirin kendisinde (SF Mono / Menlo); ayarın ailesi
/// eşaralıklı değilse hücre yine boşluktan türer, geniş harfler kırpılır ve
/// bunu [`FontIssue::NotMonospaced`] söyler. Ölçülen karakter
/// boşluk çünkü her fontta var; seçim gövdede sabit, çünkü başka bir karakterle
/// çağrılması hücre genişliğini fontun o harfine bağlamak olurdu.
fn space_advance(font: &CTFont) -> CGFloat {
    let Some(glyph) = glyph_index(font, ' ') else {
        return 0.0;
    };
    let mut advance = [CGSize::ZERO; 1];
    // SAFETY: tek glyph, tek ölçü hücresi; sayı ikisiyle de tutarlı.
    unsafe {
        font.advances_for_glyphs(
            CTFontOrientation::Horizontal,
            NonNull::from(&glyph),
            advance.as_mut_ptr(),
            1,
        );
    }
    advance[0].width
}

/// Yukarı yuvarlar ve `u16`'ya sıkıştırır.
///
/// Alt sınır 1: bozuk ya da bulunamayan bir fontta metrik sıfır dönebilir ve
/// sıfır genişlikli hücre ızgarayı sıfıra böler. Üst sınır tipin kendisi.
///
/// NaN ayrıca ele alınıyor çünkü `clamp` onu **geçirir** ve `NaN as u16` 0
/// eder: alt sınır sessizce delinir ve hata bölmede patlar, kaynağında değil.
fn round_up(v: CGFloat) -> u16 {
    if !v.is_finite() {
        return 1;
    }
    v.ceil().clamp(1.0, f64::from(u16::MAX)) as u16
}
