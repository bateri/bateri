//! Font zinciri ve hücre metriği.
//!
//! Zincirin can alıcı noktası şu: `CTFontCreateWithName` **hata vermez**.
//! İstenen aile yoksa CoreText elindeki en yakın fontu döndürür ve çağıran
//! hiçbir şey fark etmez. Bu yüzden "font açıldı" bir kanıt değildir; açılan
//! fontun kendi bildirdiği aile adı istenenle karşılaştırılır.

use std::ptr::{self, NonNull};

use objc2_core_foundation::{
    CFAttributedString, CFDictionary, CFIndex, CFNumber, CFRange, CFRetained, CFString, CFType,
    CGFloat, CGRect, CGSize,
};
use objc2_core_graphics::CGGlyph;
use objc2_core_text::{
    CTFont, CTFontDescriptor, CTFontOrientation, CTFontSymbolicTraits, CTLine, CTRun,
    kCTFontAttributeName, kCTFontFamilyNameAttribute, kCTFontSymbolicTrait, kCTFontTraitsAttribute,
};

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

/// Bir sprite'ın hangi **punto sınıfında** rasterize edileceği.
///
/// [`Face`] ile **dik** bir eksen ve bilerek ayrı: yüz metnin biçimi (SGR 1 /
/// SGR 3'ün karşılığı), bu onun ölçüsü. `Face`'e beşinci bir varyant olarak
/// eklenseydi "kalın küçük" temsil edilemez olur ve [`Faces::effective`]'in
/// merdiveni iki ayrı soruyu tek sıraya dizerdi.
///
/// Küçük sınıfın **tek** tüketicisi dock'un bağlam satırı; o satır terminalin
/// kendi altbilgisi ve kabuğun biçimlendirmesi oraya hiç girmiyor, bu yüzden
/// küçük tarafta yalnız düz yüz rasterize ediliyor (`Atlas::slot`).
// `repr(u8)`: bkz. `RuleKind` — anahtar `slot()`'un sıcak yolunda.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum SizeClass {
    #[default]
    Normal = 0,
    Small = 1,
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

    /// Renk düzleminin tek yuvası (`RGBA8`: piksel başına **dört** bayt).
    ///
    /// Aynı yuva geometrisi, başka format — ve tek sahip kuralı bozulmuyor:
    /// ikisi de [`Metrics::cell_wh`]'den türüyor, yani kenar payı ya da
    /// hizalama dolgusu bir gün girerse düzeltilecek yer hâlâ tek. Sayıyı
    /// `slot_bytes() * 4` diye yazmak da olurdu; ayrı bir isim, tamponu
    /// kuranın hangi düzlemde olduğunu **söylemesini** zorunlu kılıyor ve
    /// `raster::draw`'un assert'i yanlış düzlemi yakalıyor.
    pub fn slot_bytes_rgba(self) -> usize {
        self.slot_bytes() * 4
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
    let issue = (!is_monospaced(&font)).then_some(FontIssue::NotMonospaced { family: returned });
    (font, issue)
}

/// CoreText fontu eşaralıklı sayıyor mu — "eşaralıklı" ölçütünün **tek
/// yeri**: zincirin uyarısı ([`open_chain`]) da ayar penceresinin listesi
/// ([`monospaced_families`]) de bunu soruyor, yani listeden seçilen aile
/// uyarı almaz.
fn is_monospaced(font: &CTFont) -> bool {
    // SAFETY: `font` çağıranın elinde canlı.
    let traits = unsafe { font.symbolic_traits() };
    traits.contains(CTFontSymbolicTraits::TraitMonoSpace)
}

/// Makinedeki eşaralıklı ailelerin adları, harf duyarsız sırayla — ayar
/// penceresinin Font listesi.
///
/// Bir aile listeye ancak zincirin onu **uyarısız** açacağı hâlde giriyor:
/// CoreText adı kendi ailesine çözüyor ([`same_family`]) ve açtığı font
/// eşaralıklı ([`is_monospaced`]). Nokta ile başlayan sistem aileleri
/// (`.AppleSystemUIFont`) kullanıcıya gösterilmiyor.
///
/// Adaylar CoreText'in eşaralıklı bitine göre eşleştirdiği tanımlayıcılardan
/// geliyor, makinedeki bütün ailelerden değil: her aileyi açıp sormak
/// yüzlerce font açmak demek ve pencere açılırken beklenirdi. Eşleştirme
/// yalnız bir ön süzgeç — son söz yine yukarıdaki iki ölçütün, yani listeye
/// ölçütün kabul etmediği bir aile giremez.
pub fn monospaced_families() -> Vec<String> {
    // `TraitMonoSpace` biti `1 << 10`; `i32`'ye kayıpsız sığıyor.
    let mono = CFNumber::new_i32(CTFontSymbolicTraits::TraitMonoSpace.bits() as i32);
    // SAFETY: iki anahtar da CoreText'in dışa açtığı sabit, program boyunca
    // canlı.
    let (traits_key, symbolic_key) = unsafe { (kCTFontTraitsAttribute, kCTFontSymbolicTrait) };
    let traits = CFDictionary::from_slices(&[symbolic_key], &[&*mono]);
    let attributes = CFDictionary::from_slices(&[traits_key], &[&*traits]);
    // SAFETY: sözlük CoreText'in beklediği biçimde — `kCTFontTraitsAttribute`
    // altında `kCTFontSymbolicTrait` → `CFNumber`.
    let wanted = unsafe { CTFontDescriptor::with_attributes(attributes.as_opaque()) };
    // SAFETY: `wanted` canlı; zorunlu anahtar kümesi yok.
    let Some(matches) = (unsafe { wanted.matching_font_descriptors(None) }) else {
        return Vec::new();
    };
    // SAFETY: işlevin belgesine göre dizinin öğeleri font tanımlayıcısı.
    let matches = unsafe { matches.cast_unchecked::<CTFontDescriptor>() };
    // SAFETY: anahtar CoreText'in sabiti.
    let family_key = unsafe { kCTFontFamilyNameAttribute };
    let mut names: Vec<String> = matches
        .iter()
        // SAFETY: tanımlayıcı dizinin elinde canlı.
        .filter_map(|descriptor| unsafe { descriptor.attribute(family_key) })
        .filter_map(|name| name.downcast::<CFString>().ok())
        .map(|name| name.to_string())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort_by_cached_key(|name| name.to_lowercase());
    names.dedup();
    // Punto önemsiz: aile ve eşaralıklılık puntodan bağımsız.
    const PROBE_SIZE: CGFloat = 12.0;
    names.retain(|name| {
        let (font, returned) = open(name, PROBE_SIZE);
        same_family(&returned, name) && is_monospaced(&font)
    });
    names
}

/// Zincirin `family`'yi açarken söyleyeceği şey — ayar penceresinin Font
/// listesinde **olmayan** bir ailenin durumu (`— not found` / `— not
/// monospaced`, 029 Karar 3). Soru [`open_chain`]'in ta kendisi, yani
/// pencerenin dediği ile alt başlığın dediği ayrışamaz.
pub fn family_issue(family: &str) -> Option<FontIssue> {
    // Punto önemsiz: aile ve eşaralıklılık puntodan bağımsız.
    const PROBE_SIZE: CGFloat = 12.0;
    open_chain(Some(family), PROBE_SIZE).1
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
///
/// `line_height` kullanıcının satır aralığı çarpanı (`[font] line_height`,
/// taban `1.0`). Fazlalık glyph'in **altına ve üstüne eşit** dağılıyor: yarısı
/// taban çizgisini aşağı itiyor, kalanı altta kalıyor. Tek yana eklenseydi
/// metin hücresinin içinde yukarı ya da aşağı kayar ve satır aralığı açıldıkça
/// bu kayma büyürdü.
///
/// Alt çizgi ve üstü çizili **kendiliğinden** takip ediyor: ikisi de tabandan
/// ölçülüyor ve taban zaten kaymış oluyor. Ayrı bir düzeltme eklenseydi
/// çarpan büyüdükçe çizgiler harften kopardı.
pub(crate) fn metrics(font: &CTFont, line_height: f64) -> Metrics {
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
    let natural = round_up(ascent).saturating_add(round_up(descent + leading));
    // Çarpan **hücreye** uygulanıyor, ascent'e değil: ölçüt satırlar arası
    // mesafe ve o mesafenin fontça tanımı `ascent + descent + leading`.
    // `1.0`'da fazlalık sıfır, yani bu yol varsayılanda bir no-op.
    let extra = round_up(f64::from(natural) * (line_height - 1.0));
    let above = extra / 2;
    let baseline = round_up(ascent).saturating_add(above);
    let cell_px = (
        round_up(space_advance(font)),
        // `saturating_add`: iki parça da `u16::MAX`'e kadar çıkabiliyor.
        natural.saturating_add(extra),
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
/// (Menlo) sınır hiç zorlanmıyor — 13pt'de alt çizgi 14+1, hücre 18 — ama
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

/// Boşluğun yatay advance'i — hücre genişliği, **kesirli**.
///
/// Monospace varsayımı zincirin kendisinde (SF Mono / Menlo); ayarın ailesi
/// eşaralıklı değilse hücre yine boşluktan türer, geniş harfler kırpılır ve
/// bunu [`FontIssue::NotMonospaced`] söyler. Ölçülen karakter
/// boşluk çünkü her fontta var; seçim gövdede sabit, çünkü başka bir karakterle
/// çağrılması hücre genişliğini fontun o harfine bağlamak olurdu.
///
/// Dönüş **yuvarlanmamış** ve `pub(crate)` olmasının sebebi bu: ızgaranın
/// adımı yuvarlanmış hâli ([`Metrics::cell_px`]) ama iki tüketici kesirli
/// hâli istiyor — [`fallback_font`]'un mürekkep kapısı ile
/// [`crate::raster::draw`]'in ortalaması. İkisi de yuvarlanmışla çalışsaydı
/// taban fontun **kendi** glyph'i hücreden dar görünür (7.827 < 8) ve
/// ortalama her glyph'i yarım pikselin altında kaydırırdı: çıktı bit bit aynı
/// kalmazdı. Bekçisi `the_cell_is_the_rounded_advance`.
pub(crate) fn space_advance(font: &CTFont) -> CGFloat {
    let Some(glyph) = glyph_index(font, ' ') else {
        return 0.0;
    };
    glyph_advance(font, glyph)
}

/// Bir glyph'in yatay ilerlemesi, **kesirli**.
///
/// [`space_advance`]'ten ayrılmasının sebebi ikinci çağıran: yedek adayın
/// mürekkep kapısı ile `raster::draw`'in ortalaması karakterin **kendi**
/// glyph'ini ölçüyor, boşluğu değil.
pub(crate) fn glyph_advance(font: &CTFont, glyph: CGGlyph) -> CGFloat {
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

/// Bir glyph'in **mürekkep** kutusu: gerçekten boyanacak piksellerin sınırı,
/// taban çizgisinin soluna/üstüne göre ve **kesirli**.
///
/// [`glyph_advance`]'ten ayrı bir ölçü ve ikisinin ayrışması kapının varlık
/// sebebi: bir sembol fontunun glyph'i ilerlemesinden dar boyayabiliyor
/// (`⏺` U+23FA, STIX Two Math'te ilerleme hücrenin 1.046 katı ama mürekkep
/// 0.914'ü — ölçüldü, bu makine, Menlo 16pt). İlerlemeyi ölçen bir kapı onu
/// eler ve kullanıcı yerinde bir kutu görür.
pub(crate) fn glyph_ink(font: &CTFont, glyph: CGGlyph) -> CGRect {
    let mut rect = [CGRect::ZERO; 1];
    // SAFETY: tek glyph, tek ölçü hücresi; sayı ikisiyle de tutarlı.
    unsafe {
        font.bounding_rects_for_glyphs(
            CTFontOrientation::Horizontal,
            NonNull::from(&glyph),
            rect.as_mut_ptr(),
            1,
        );
    }
    rect[0]
}

/// Glyph'in hücre içindeki yatay kaydırması — **tek formül, iki tüketici**.
///
/// Çizim ([`crate::raster::draw`]) glyph'i buraya koyuyor, kapı
/// ([`fallback_font`]) mürekkebi buradan ölçüyor. Ayrı yazılsalardı kapı
/// çizilmeyecek bir yerleşimi sınar ve ikisi sessizce ayrışırdı: kabul edilen
/// bir aday hücrenin dışına boyayabilir ya da sığan bir aday elenirdi.
///
/// Argüman **kutunun** ilerlemesi, hücrenin değil: tek hücrelik bir glyph'te
/// ikisi aynı sayı, iki sütunluk bir karakterde kutu iki hücre
/// ([`crate::Half`]). Adı 023'te `cell_advance`'ten `box_advance`'e çevrildi
/// ve tek satırlık bir yeniden adlandırma değildi: aynı sayı hem kapıya hem
/// çizime gidiyor, yani yalnız birinde sütunla çarpılsa kapı çizilmeyecek bir
/// yerleşimi sınardı.
///
/// `max(0.0)` çizimin kendi kuralı: ilerlemesi kutuyu aşan bir glyph sola
/// yapışıyor, çünkü kırpma sağdan olmalı — gerekçe [`crate::raster::draw`]'in
/// gövdesinde. Kapının bunu **aynen** paylaşması şart, yoksa negatif bir
/// kaydırma varsayıp adayın solunu hücrenin içinde sanırdı.
pub(crate) fn centre_shift(box_advance: CGFloat, advance: CGFloat) -> CGFloat {
    ((box_advance - advance) / 2.0).max(0.0)
}

/// Adayın boyayacağı piksel **kutunun** içinde mi kalıyor.
///
/// Kutu tek hücre ya da iki hücre (`box_advance`): geniş ilan edilmiş bir
/// karakter iki sütun işgal ediyor, yani mürekkebi iki hücreye sığıyorsa
/// kabul edilmeli. Sıranın kendisi [`fallback_font`]'ta.
///
/// Ölçüt yatay ve yalnız yatay. Dikeyi de sınamak bugün **hiçbir adayı
/// elemiyor** (ölçüldü: yatay kapıyı geçen her aday hücrenin taban çizgisi
/// penceresine de sığıyor; dikeyde taşan tek küme emoji ve o zaten yatayda
/// dönüyor), yani ikinci ölçüt yazılmış ama tanığı olmayan bir kural olurdu.
/// Sınır adıyla yazılı: dikeyde taşan bir aday bugün kutuya değil **kırpmaya**
/// düşer.
fn ink_fits_box(font: &CTFont, glyph: CGGlyph, box_advance: CGFloat) -> bool {
    ink_fits_placed(
        box_advance,
        glyph_advance(font, glyph),
        glyph_ink(font, glyph),
    )
}

/// [`ink_fits_box`]'in fontsuz gövdesi: ilerlemesi `advance`, mürekkebi
/// `ink` olan bir glyph [`centre_shift`]'in koyduğu yerde kutuya sığıyor mu.
///
/// Ayrı olmasının sebebi tarama (`census`): bir adayın **ne kadar
/// küçültülürse** sığacağını soruyor ve ölçeklenmiş ölçüleri fontsuz
/// veriyor. Kural tek yerde kalıyor, yani taramanın "sığar" dediği ile
/// kapının kabul ettiği ayrışamaz.
pub(crate) fn ink_fits_placed(box_advance: CGFloat, advance: CGFloat, ink: CGRect) -> bool {
    let left = ink.origin.x + centre_shift(box_advance, advance);
    // Sol kenar da sınanıyor: negatif `origin.x` taşıyan bir aday hücreye
    // soldan taşar ve CG onu **soldan** keser. Latin yazıda harf soldan
    // tanınıyor, yani o kırpma sessiz bir bozulma olurdu — kutu dürüsttür.
    left >= 0.0 && left + ink.size.width <= box_advance
}

/// `ch`'i çizebilen bir sistem fontu — **hücreye sığıyorsa**.
///
/// Üç adım tek fonksiyonda, çünkü üçü tek soruyu yanıtlıyor: "bu karakteri
/// kabul edilebilir bir fontla çizebilir miyiz". `None` "aday bulunamadı"
/// değil **"kabul edilmedi"** demek ve çağıran ikisini ayırt etmek zorunda
/// değil — ikisinin de cevabı [`crate::TOFU`].
///
/// 1. **Aday.** `CTFontCreateForString` cascade'i bizim için yürüyor ve
///    [`glyph_index`]'in sarıldığı `CTFontGetGlyphsForCharacters`'ın
///    **yapmadığı** tam olarak bu: o yalnız verilen fonta bakıyor, cascade'e
///    düşmüyor. Setin varlık sebebi bu fark (`⏵` U+23F5 Menlo'da yok).
/// 2. **Glyph.** Aday gerçekten çizebiliyor mu. Aday `base`'in kendisi
///    dönebilir ve o hâlde bu adım `None` verir — buraya ancak `base`
///    `.notdef` verdikten sonra düşülüyor, yani ayrı bir "aynı font mu"
///    karşılaştırması gerekmiyor.
/// 3. **Mürekkep kapısı** ([`ink_fits_cell`]). Aday, çizileceği yerde
///    hücrenin dışına boyuyor mu — ve **reddin tek ölçütü bu**: emoji, CJK ve
///    `.LastResort` aynı kapıdan eleniyor, çünkü onların mürekkebi
///    ilerlemeleri kadar geniş. Aile adı karşılaştırması, trait biti ve
///    sihirli dizge yok; ölçülen sayılar
///    `.tasks/019-glyph-yedegi/phase-1.md`'de. Sınır **kesirli** hücre
///    ilerlemesi ([`space_advance`]), yuvarlanmış hücre genişliği değil: aynı
///    sayı `raster::draw`'in ortalamasını da besliyor ve iki iş için iki sayı
///    tutmak ikisini ayrıştırırdı.
///
/// Ölçüt bir dönem **ilerlemeydi** (`advance <= cell_advance`) ve belirtisi
/// kullanıcıda görüldü: Claude Code'un araç işareti `⏺` (U+23FA) kutu
/// çıkıyordu. Sebep ölçüldü — STIX Two Math'ten gelen aday hücreden %4.6
/// geniş **ilerliyor** ama %8.6 dar **boyuyor**, yani ilerlemeyi ölçen kapı
/// hücreye rahat sığan bir glyph'i eliyordu. 019'un kalibrasyon örneklerinde
/// (2.17× / 1.83× / 1.66×) 1.0'ın yakınında hiçbir aday yoktu ve kapı sembol
/// fontlarına karşı hiç sınanmamıştı.
///
/// Ölçütün değişmesi ters yöndeki boşluğu da kapatıyor: dar ilerleyip geniş
/// boyayan bir aday artık **kutu**, eskiden sessizce sağdan kırpılıyordu.
/// "Kutu ya da tam glyph" ilk kez bir dilek değil sözleşme.
///
/// Mürekkebi olmayan aday kapıdan **geçer** (sıfır genişlik her hücreye
/// sığar); çizilecek şey görünmez bir glyph olur, kutu değil. Bugün bu yol
/// doğmuyor çünkü birleştirici işaretler grid hücresine ayrı bir sprite
/// olarak hiç gelmiyor.
///
/// **Log yok:** fonksiyon [`crate::Atlas::slot`]'un çizim yolunda ve glyph
/// başına basılan bir satır kare bütçesinin ortasına düşerdi
/// ([`Faces::derive`]'ın yazılı kuralı).
pub(crate) fn fallback_font(
    base: &CTFont,
    ch: char,
    cell_advance: CGFloat,
    cols: u8,
) -> Option<Accepted> {
    let candidate = cascade_candidate(base, ch);
    let glyph = glyph_index(&candidate, ch)?;
    accept(candidate, glyph, cell_advance, cols)
}

/// [`fallback_font`]'un 1. adımı: cascade'in `ch` için önerdiği font.
///
/// Ayrı olmasının sebebi tarama (`census`): karakteri kapının **aynı**
/// adımlarından geçirip her adımın cevabını ayrı raporluyor. Adım
/// kopyalansaydı tarama bir gün kapının sormadığı bir soruyu sorardı.
///
/// Dönüş hiç boş değil: kimsenin çizemediği karakterde CoreText
/// `.LastResort`'u veriyor ve o da bir glyph döndürüyor, yani "aday yok"
/// cevabı buradan değil sonraki adımdan ([`glyph_index`]) doğuyor.
pub(crate) fn cascade_candidate(base: &CTFont, ch: char) -> CFRetained<CTFont> {
    let mut utf8 = [0u8; 4];
    let text = CFString::from_str(ch.encode_utf8(&mut utf8));
    let range = CFRange {
        location: 0,
        // `CFString` UTF-16 birimi sayıyor, bayt değil: BMP dışı karakterde
        // aralık iki birim ve `1` verilseydi vekil çiftinin yarısı istenirdi.
        length: ch.len_utf16() as CFIndex,
    };
    // SAFETY: `base` ve `text` bu kapsamda canlı; `range` string'in tamamı.
    unsafe { base.for_string(&text, range) }
}

/// Mürekkep kapısı: adayın glyph'i önce tek hücreye, sonra (iki sütun ilan
/// edilmişse) iki hücreye sığıyor mu.
///
/// Tek glyph'lik yedek ([`fallback_font`]) ile grapheme dizisinin
/// ([`shape_cluster`]) **ortak** kapısı — "kutu ya da tam glyph" sözleşmesi
/// ikisinde de aynı sıradan geçiyor, yani dizinin glyph'i tek kod noktalı
/// emojiden farklı bir ölçütle kabul edilemez. Taramanın (`census`) da
/// kapısı bu, yani `pub(crate)`.
pub(crate) fn accept(
    candidate: CFRetained<CTFont>,
    glyph: CGGlyph,
    cell_advance: CGFloat,
    cols: u8,
) -> Option<Accepted> {
    // **Sıra zorunlu: önce tek hücre.** Tek hücreye sığan bir aday bugün de
    // sığıyor ve tek yuvadan çiziliyor; doğrudan iki hücrelik kutuyla
    // sorulsaydı `centre_shift` onu iki hücrenin ortasına kaydırır ve
    // *bugün çalışan* bir çizim yerinden oynardı. Ölçüldü (023 `context.md`):
    // 65 karakter geniş ilan edilmiş ama mürekkebi tek hücreye sığıyor —
    // 21'i Menlo'nun kendi glyph'i, 44'ü cascade'den narin mürekkeple gelen
    // CJK noktalaması ve fullwidth formlar (`、 。 》 ！`). Yan kazanç
    // kapasite: o 65 ikinci bir yuva da harcamıyor.
    if ink_fits_box(&candidate, glyph, cell_advance) {
        return Some(Accepted {
            font: candidate,
            glyph,
            cols: 1,
        });
    }
    // İkinci kapı yalnız **iki sütun ilan edilmiş** karakterde açılıyor. Tek
    // sütunlu bir karaktere iki hücre vermek komşusunun üstüne boyamak olurdu:
    // ızgara ona spacer ayırmıyor ve o hücrenin kendi mürekkebi var. Ölçüt bu
    // yüzden `min(sütun, mürekkep)`.
    if cols >= 2 && ink_fits_box(&candidate, glyph, cell_advance * CGFloat::from(cols)) {
        return Some(Accepted {
            font: candidate,
            glyph,
            cols,
        });
    }
    None
}

/// Grapheme dizisini (`🇹🇷`, `👨‍👩‍👧`, `👍🏽`, `❤️`) **tek glyph**'e şekillendirir
/// ve [`fallback_font`]'un kapısından geçirir; `None` "tek glyph değil ya da
/// kapıdan döndü" demek ve çağıran taban karaktere düşüyor (035 R1.1).
///
/// Şekillendirme `CTLine`'dan, çünkü dizinin glyph'i hiçbir kod noktasının
/// glyph'i değil: bayrağın iki RI'si, ZWJ ailesi ve ten rengi fontun
/// ligatür/`morx` tablosunda **tek** glyph'e birleşiyor ve bunu soran tek
/// API satır düzeni. [`glyph_index`]'in sarıldığı
/// `CTFontGetGlyphsForCharacters` kod noktası başına bakıyor ve `🇹🇷`'yi iki
/// ayrı harf olarak verirdi.
///
/// Font **iki kez** soruluyor ve ikisi ayrı sorular: aday dizginin tamamı
/// için cascade'den (`CTFontCreateForString`, [`fallback_font`]'un 1. adımı)
/// ve satıra o veriliyor; ölçülen ve çizilen font ise **run'ın kendi**
/// fontu. Aday dizinin bir parçasını çizemezse `CTLine` şekillendirme
/// sırasında yeniden ikame edebiliyor ve ilerleme, mürekkep ve düzlem
/// glyph'i gerçekten üreten fontun ölçüsü olmak zorunda — başka bir fontun
/// glyph numarasını adayla çizmek bambaşka bir harf çizerdi.
///
/// **Log yok**, [`fallback_font`] ile aynı gerekçe: çizim yolunda.
pub(crate) fn shape_cluster(
    base: &CTFont,
    text: &str,
    cell_advance: CGFloat,
    cols: u8,
) -> Option<Accepted> {
    let string = CFString::from_str(text);
    let range = CFRange {
        location: 0,
        // UTF-16 birimi, bayt değil ([`fallback_font`]'un aynı tuzağı): ZWJ
        // ailesi beş kod noktası ama sekiz birim.
        length: text.encode_utf16().count() as CFIndex,
    };
    // SAFETY: `base` ve `string` bu kapsamda canlı; `range` dizginin tamamı.
    let candidate = unsafe { base.for_string(&string, range) };
    // SAFETY: anahtar CoreText'in dışa açtığı sabit, program boyunca canlı.
    let font_key = unsafe { kCTFontAttributeName };
    let attributes = CFDictionary::from_slices(&[font_key], &[&*candidate]);
    // SAFETY: ayırıcı varsayılan (`None`), dizgi ve sözlük canlı; sözlük
    // CoreText'in beklediği biçimde — `kCTFontAttributeName` → `CTFont`.
    let attributed =
        unsafe { CFAttributedString::new(None, Some(&string), Some(attributes.as_opaque())) }?;
    // SAFETY: `attributed` canlı; satır onu kopyalıyor.
    let line = unsafe { CTLine::with_attributed_string(&attributed) };
    // SAFETY: `line` canlı; saf okuma.
    if unsafe { line.glyph_count() } != 1 {
        return None;
    }
    // SAFETY: `line` canlı; belgeye göre dizinin öğeleri `CTRun`.
    let runs = unsafe { line.glyph_runs() };
    // SAFETY: işlevin belgesi öğe tipini `CTRun` diye veriyor.
    let runs = unsafe { runs.cast_unchecked::<CTRun>() };
    // Tek glyph tek run demek; ikinci bir run'ın glyph'i olamaz.
    let run = runs.get(0)?;
    let mut glyph: CGGlyph = 0;
    // SAFETY: run tek glyph taşıyor (satırın sayısı 1), aralık `0..1` ve
    // tampon tek eleman.
    unsafe {
        run.glyphs(
            CFRange {
                location: 0,
                length: 1,
            },
            NonNull::from(&mut glyph),
        )
    };
    // `.notdef` bir glyph değil: fontun "bunu çizemem" cevabı.
    if glyph == 0 {
        return None;
    }
    // SAFETY: `run` canlı; öznitelik sözlüğü satırınkinin run'a düşen hâli,
    // anahtarları `CFString`.
    let attributes = unsafe { run.attributes() };
    // SAFETY: CoreText'in öznitelik sözlüğünün anahtarları `CFString`;
    // değerin tipi aşağıda `downcast` ile sınanıyor.
    let attributes = unsafe { attributes.cast_unchecked::<CFString, CFType>() };
    let font = attributes
        .get(font_key)
        .and_then(|font| font.downcast::<CTFont>().ok())
        .unwrap_or(candidate);
    accept(font, glyph, cell_advance, cols)
}

/// Fontun glyph'leri **renkli** mi.
///
/// Ölçüt fontun kendi trait biti (`kCTFontTraitColorGlyphs`), aile adı
/// **değil**: `CLAUDE.md`'nin mürekkep kapısı için yazdığı kural ("aile adı
/// karşılaştırması, trait biti ve sihirli dizge yok") o kapının ölçütü
/// hakkında ve burada konu başka — "bu glyph hangi düzleme rasterize
/// edilecek" sorusunun cevabı fontun gerçek bir özelliği. Apple Color
/// Emoji'yi adıyla aramak, aynı işi yapan başka bir renkli fontu (kullanıcının
/// kurduğu bir Nerd Font emoji seti) sessizce maske düzlemine düşürürdü.
pub(crate) fn has_color_glyphs(font: &CTFont) -> bool {
    // SAFETY: `font` çağrı boyunca canlı; dönüş bir bit kümesi.
    let traits = unsafe { font.symbolic_traits() };
    traits.contains(CTFontSymbolicTraits::TraitColorGlyphs)
}

/// Kabul edilen aday ve **kaç hücreye** sığdığı.
///
/// `cols` ızgaranın ayırdığı sütun sayısı değil, kapının kabul ettiği kutu:
/// iki sütun ilan edilmiş bir karakter tek hücreye sığıyorsa burada `1`
/// dönüyor ve tek yuvadan çiziliyor.
pub(crate) struct Accepted {
    pub(crate) font: CFRetained<CTFont>,
    /// Kapının ölçtüğü glyph — çizilecek olan da o. Tek kod noktasında
    /// [`glyph_index`]'in cevabı, dizide `CTLine`'ın şekillendirdiği.
    pub(crate) glyph: CGGlyph,
    pub(crate) cols: u8,
}

/// Yukarı yuvarlar ve `u16`'ya sıkıştırır.
///
/// Alt sınır 1: bozuk ya da bulunamayan bir fontta metrik sıfır dönebilir ve
/// sıfır genişlikli hücre ızgarayı sıfıra böler. Üst sınır tipin kendisi.
///
/// NaN ayrıca ele alınıyor çünkü `clamp` onu **geçirir** ve `NaN as u16` 0
/// eder: alt sınır sessizce delinir ve hata bölmede patlar, kaynağında değil.
pub(crate) fn round_up(v: CGFloat) -> u16 {
    if !v.is_finite() {
        return 1;
    }
    v.ceil().clamp(1.0, f64::from(u16::MAX)) as u16
}
