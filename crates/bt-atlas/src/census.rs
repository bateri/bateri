//! Yedek kapısının **taraması** ve gerçek araçların karakterleri için
//! **bekçi** (041 phase-1).
//!
//! Kapının kalibrasyonu iki kez bir örnek kümesine bakılarak yapıldı ve iki
//! kez de sınırın dışındaki karakteri kullanıcı buldu (`⏺`, `⎿`, sonra `⧉`).
//! Tarama bu yüzden örnek değil **envanter**: sembol ve emoji bloklarındaki
//! her kod noktasını kapının kendi adımlarından geçirip dört gruptan birine
//! koyuyor. Sonucu makinede kurulu fontlara bağlı, yani kapıya girmiyor —
//! `make tarama` ile elle koşuyor.
//!
//! Modül yalnız sınamada derleniyor: sınıflamanın üretimde tüketicisi yok.
//! Kapıyı kopyalamıyor, adımlarını çağırıyor; phase-2'den beri küçültme
//! kolunu da (`font::accept`) aynı yoldan görüyor.

use objc2_core_foundation::CGFloat;
use objc2_core_text::CTFont;

use crate::{Atlas, Face, font, raster};

/// Bir karakterin yedek kapısındaki yeri.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Class {
    /// Taban font çiziyor; yedek yolu hiç koşmuyor.
    InBase,
    /// Cascade'in adayı kapıdan geçti.
    Fallback { font: String, ratio: f64 },
    /// Cascade'in adayı da `.notdef` verdi.
    NoFont,
    /// Aday iki kapıdan döndü ama küçük puntolu kopyası kabul edildi (041).
    Shrunk { font: String, ratio: f64, fit: f64 },
    /// Aday var ama kapıdan döndü: ekranda kutu. `fit` sınırın üstünde,
    /// aday `.LastResort` ya da küçük kopya yeniden sınamadan döndü.
    Rejected { font: String, ratio: f64, fit: f64 },
}

/// `ch`'i kapının adımlarından **aynen** geçirir: [`font::glyph_index`]
/// (taban) → [`font::cascade_candidate`] → [`font::glyph_index`] (aday) →
/// [`font::accept`]. İkinci bir kapı yok; sınıflama yalnız adımların
/// cevaplarını ayrı ayrı adlandırıyor.
///
/// İki oran, ikisi de **kutuya** (`cell_advance × cols`) bölünmüş:
///
/// - `ratio` — mürekkep genişliği / kutu. "Mürekkep hücreden ne kadar
///   geniş": `⧉` Menlo 16pt'de ~1.11. Yerleşimden bağımsız, yani adayın
///   mürekkebi kutuda ortalansaydı gereken küçültme bu.
/// - `fit` — bugünkü yerleşimle ([`font::centre_shift`], sola yapışma
///   kuralı dahil) kapının geçmesi için adayın **kaç kat** küçültülmesi
///   gerektiği; sol ve sağ taşmanın büyüğünü taşıyor. Küçültme ilerlemeyi de
///   küçülttüğü ve ilerlemesi kutuyu hâlâ aşan glyph sola yapıştığı için
///   `ratio`'dan büyük olabilir: sağ boşluğu olmayan bir aday küçülünce
///   ortaya gelmiyor. Hesabı [`font::fit_ratio`], küçültme kolunun
///   katsayısıyla aynı fonksiyon.
///
/// `Fallback`'te `fit` taşınmıyor: geçen adayda küçültme sorusu yok.
pub(crate) fn classify(base: &CTFont, ch: char, cell_advance: CGFloat, cols: u8) -> Class {
    if font::glyph_index(base, ch).is_some() {
        return Class::InBase;
    }
    let candidate = font::cascade_candidate(base, ch);
    let Some(glyph) = font::glyph_index(&candidate, ch) else {
        return Class::NoFont;
    };
    // `.LastResort` raporda kapının kendi ölçütüyle adlanıyor
    // ([`font::is_last_resort`]), yani rapor ile kapı aynı fontu ayırıyor.
    let family = if font::is_last_resort(&candidate) {
        font::LAST_RESORT.to_string()
    } else {
        // SAFETY: `candidate` bu kapsamda canlı; saf okuma.
        unsafe { candidate.family_name() }.to_string()
    };
    let advance = font::glyph_advance(&candidate, glyph);
    let ink = font::glyph_ink(&candidate, glyph);
    let box_advance = cell_advance * CGFloat::from(cols);
    let ratio = ink.size.width / box_advance;
    let fit = font::fit_ratio(box_advance, advance, ink);
    match font::accept(candidate, glyph, cell_advance, cols) {
        Some(a) if a.shrunk => Class::Shrunk {
            font: family,
            ratio,
            fit,
        },
        Some(_) => Class::Fallback {
            font: family,
            ratio,
        },
        None => Class::Rejected {
            font: family,
            ratio,
            fit,
        },
    }
}

/// Gerçek araçların ekrana bastığı karakterler. Kutu çıkan her biri
/// kullanıcının göreceği bir kusur; kullanıcı yenisini bulunca buraya eklenir,
/// yani aynı kusur ikinci kez sessizce geri gelmez.
const TOOL_CHARS: [char; 26] = [
    // Claude Code: araç işareti, sonuç ağacı, artifact bağlantısı, spinner
    // yıldızları, ayraç, kip göstergesi, duraklatma, kesinti.
    '⏺', '⎿', '⧉', '✻', '✢', '✳', '✶', '·', '⏵', '⏸', '↯',
    // Spinner'lar: Braille (yordamsal) ve çeyrek daireler.
    '⠋', '⠙', '◐', '◓', '⣾', '⣽',
    // git / starship / p10k: Nerd Font'un dal ve powerline işaretleri (PUA),
    // durum işaretleri, ileri/geri, prompt karakterleri, nokta.
    '\u{E0A0}', '\u{E0B0}', '✔', '✘', '⇡', '⇣', '❯', '❮', '●',
];

/// [`TOOL_CHARS`]'tan bugün **kutu** çıkanlar (Menlo 16pt @2x, bu makine).
///
/// Liste yalnız küçülür: bir karakter artık çiziliyorsa bekçi kırmızı düşer
/// ve onu buradan çıkarmayı ister ([`tofu_drift`]). Böylece düzelen bir
/// karakter sessizce "beklenen kutu" olarak kalmıyor, ve düzeltme geri
/// alınırsa bekçi onu yeniden görüyor.
///
/// `U+E0A0`/`U+E0B0` hiçbir kurulu fontta yok, cascade `.LastResort`'u
/// veriyor. R3.2 onun küçültülmesini yasaklıyor ([`font::is_last_resort`]),
/// yani küçültme bunları boşaltmıyor; boşaltan şey Nerd Font kurulu bir
/// makine. `⧉` 041 phase-2'de listeden çıktı: küçültülerek çiziliyor.
const EXPECTED_TOFU: [char; 2] = ['\u{E0A0}', '\u{E0B0}'];

/// Gözlemi (karakter, kutu mu) beklenen kutu listesiyle karşılaştırır ve
/// her sapmayı bir satır olarak verir; boş dönüş yeşil.
///
/// İki yönlü: listede olmayan kutu kusurdur, listede olup artık çizilen
/// karakter de listenin güncellenmesi gerektiğini söyler. Listede olup
/// gözlenmeyen karakter de sapma: liste bekçinin karakterleri dışında
/// bir şey iddia edemez.
fn tofu_drift(observed: &[(char, bool)], expected: &[char]) -> Vec<String> {
    let mut drift = Vec::new();
    for &(ch, tofu) in observed {
        let listed = expected.contains(&ch);
        if tofu && !listed {
            drift.push(format!("'{ch}' (U+{:04X}) kutu çıkıyor", u32::from(ch)));
        }
        if !tofu && listed {
            drift.push(format!(
                "'{ch}' (U+{:04X}) artık çiziliyor — EXPECTED_TOFU'dan çıkar",
                u32::from(ch)
            ));
        }
    }
    for &ch in expected {
        if !observed.iter().any(|&(c, _)| c == ch) {
            drift.push(format!(
                "'{ch}' (U+{:04X}) EXPECTED_TOFU'da ama bekçinin listesinde yok",
                u32::from(ch)
            ));
        }
    }
    drift
}

/// Tek karakterin bugün kutu çıkıp çıkmadığı — atlasın düz yüzdeki sırasıyla:
/// yordamsal aile fonttan önce çiziliyor, kalanı kapının sınıflaması.
fn is_tofu(a: &Atlas, ch: char) -> bool {
    if raster::is_procedural(ch) {
        return false;
    }
    matches!(
        classify(a.faces.get(Face::Regular), ch, a.cell_advance, 1),
        Class::NoFont | Class::Rejected { .. }
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;

    use super::*;

    /// Gerçek araçların karakterleri kutu çıkmıyor — ya da çıktıkları
    /// [`EXPECTED_TOFU`]'da adıyla yazılı.
    ///
    /// **Bilinen sınır:** beklenti makinede kurulu fontlara bağlı. Nerd Font
    /// kurulu bir makinede `U+E0A0` gerçek bir glyph'e düşer ve bekçi "listeden
    /// çıkar" diye kırmızı düşer — kod doğruyken. `the_gate_decides_by_ink_alone`
    /// bu yüzden beklentiyi adaydan türetiyor; bu sınama ise bilerek **olguyu**
    /// sabitliyor, çünkü sorusu "kural doğru mu" değil "kullanıcı kutu görüyor
    /// mu". Taban aile ayardan bağımsız, zincirin varsayılanı.
    #[test]
    fn tool_chars_are_not_tofu() {
        let a = Atlas::new(None, 16.0, 2.0, 1.0);
        let observed: Vec<(char, bool)> =
            TOOL_CHARS.iter().map(|&ch| (ch, is_tofu(&a, ch))).collect();
        let drift = tofu_drift(&observed, &EXPECTED_TOFU);
        assert!(drift.is_empty(), "bekçi sapması:\n{}", drift.join("\n"));
    }

    /// Bekçinin karşılaştırması iki yönde de kırmızı: listede olmayan kutu
    /// ve listede olup artık çizilen karakter. Gerçek fontla kurulamayan
    /// yönler sentetik gözlemle sınanıyor.
    #[test]
    fn tofu_drift_flags_both_directions() {
        assert!(tofu_drift(&[('a', false), ('⧉', true)], &['⧉']).is_empty());
        let fixed = tofu_drift(&[('⧉', false)], &['⧉']);
        assert_eq!(fixed.len(), 1, "düzelen karakter görülmedi: {fixed:?}");
        assert!(fixed[0].contains("EXPECTED_TOFU'dan çıkar"), "{fixed:?}");
        let broken = tofu_drift(&[('a', true)], &[]);
        assert_eq!(broken.len(), 1, "yeni kutu görülmedi: {broken:?}");
        let stray = tofu_drift(&[], &['⧉']);
        assert_eq!(stray.len(), 1, "listede kalan yabancı görülmedi: {stray:?}");
    }

    /// Kabul edilen adayı, hücrenin **dört yanında birer hücre boşluk**
    /// bırakan 3×3 hücrelik bir tuvale çizer ve kapsama (alfa) haritasını
    /// verir; hücre tuvalin ortasında.
    ///
    /// Yuvanın kendi tamponu taşmayı göremez — CG yuvanın kenarında
    /// kırpıyor — yani "hücrenin içinde mi" sorusu ancak yuvadan geniş bir
    /// bağlamda sorulabiliyor. Glyph `x_offset = -w` ile ortadaki sütuna,
    /// taban çizgisi bir hücre boyu aşağı alınarak ortadaki satıra kayıyor;
    /// kutu, ortalama ve `rise` atlasın çiziminin aynısı (`rise` asıl
    /// hücrenin ölçüsünden).
    fn draw_wide(alt: &font::Accepted, m: crate::Metrics, cell: CGFloat) -> Vec<u8> {
        let wide = crate::Metrics {
            cell_px: (m.cell_px.0 * 3, m.cell_px.1 * 3),
            baseline_px: m.baseline_px + m.cell_px.1,
            ..m
        };
        let box_advance = cell * CGFloat::from(alt.cols);
        let offset = -CGFloat::from(m.cell_px.0);
        let rise = alt.rise(m);
        if font::has_color_glyphs(&alt.font) {
            let mut rgba = vec![0u8; wide.slot_bytes_rgba()];
            raster::draw_color_glyph(
                &alt.font,
                alt.glyph,
                wide,
                box_advance,
                offset,
                rise,
                &mut rgba,
            );
            rgba.chunks(4).map(|p| p[3]).collect()
        } else {
            let mut mask = vec![0u8; wide.slot_bytes()];
            raster::draw_glyph(
                &alt.font,
                alt.glyph,
                wide,
                box_advance,
                offset,
                rise,
                &mut mask,
            );
            mask
        }
    }

    /// Küçültülen glyph hücrenin içinde: `⧉` (Apple Symbols, maske) ve tek
    /// sütunlu emoji `🌡` (Apple Color Emoji, renk düzlemi) iki ölçekte
    /// kabul ediliyor, küçültme kolundan geliyor ve ortadaki hücrenin
    /// dışında tek bir kapsama pikseli yok — ne solda ne sağda, ne üstte ne
    /// altta (dikey ortalama, `font::Accepted::rise`, emojiyi de hücrenin
    /// içine alıyor). Renkli kopyanın trait'i korunuyor, yani düzlem ve
    /// çizim reçetesi aynı.
    #[test]
    fn shrunk_glyph_stays_inside_the_cell() {
        for (pt, scale) in [(16.0, 2.0), (13.0, 1.0)] {
            let a = Atlas::new(None, pt, scale, 1.0);
            let base = a.faces.get(Face::Regular);
            let m = a.metrics;
            for (ch, color) in [('⧉', false), ('🌡', true)] {
                let alt = font::fallback_font(base, ch, a.cell_advance, 1)
                    .unwrap_or_else(|| panic!("{ch} {pt}pt@{scale}x: kutu çıktı"));
                assert!(
                    alt.shrunk,
                    "{ch} {pt}pt@{scale}x küçültülmeden kabul edildi"
                );
                assert_eq!(alt.cols, 1);
                assert_eq!(font::has_color_glyphs(&alt.font), color, "{ch}: düzlem");
                let cov = draw_wide(&alt, m, a.cell_advance);
                let (w, h) = m.cell_wh();
                let mut ink = 0usize;
                for (i, &c) in cov.iter().enumerate() {
                    if c == 0 {
                        continue;
                    }
                    ink += 1;
                    let (x, y) = (i % (3 * w), i / (3 * w));
                    assert!(
                        (w..2 * w).contains(&x) && (h..2 * h).contains(&y),
                        "{ch} {pt}pt@{scale}x: ({x}, {y}) hücrenin \
                         ({w}..{}, {h}..{}) dışında",
                        2 * w,
                        2 * h
                    );
                }
                assert!(ink > 0, "{ch} {pt}pt@{scale}x: hiç mürekkep yok");
            }
        }
    }

    /// **Küçültülmüş tek hücrelik kabul, iki hücrelik isteğin cevabı değil**
    /// (041): `漢` tek hücreye küçülüyor ve sonra ızgaranın `Left` isteğinde
    /// tam boyuyla **çift** olarak geliyor — kısayol küçük kopyayı geniş
    /// hücrenin soluna koysaydı sağ yarı boş kalırdı ve sonuç isteklerin
    /// sırasına bağlı olurdu. Yüz merdiveninin takma adı da (kalın yüz düz
    /// yüze iniyor) biti taşıyor.
    #[test]
    fn a_shrunk_single_cell_does_not_answer_the_wide_request() {
        let mut a = Atlas::new(None, 13.0, 1.0, 1.0);
        for face in [Face::Regular, Face::Bold] {
            let ask = |a: &mut Atlas, half| {
                let (placed, upload) = a.slot(
                    crate::Sprite::Char('漢'),
                    face,
                    crate::SizeClass::Normal,
                    half,
                );
                (placed, upload.is_some())
            };
            let (whole, _) = ask(&mut a, crate::Half::Whole);
            assert_ne!(
                whole.slot,
                crate::TOFU,
                "{face:?}: '漢' tek hücreye küçülmeliydi"
            );
            assert_eq!(whole.half, crate::Half::Whole);
            let (left, _) = ask(&mut a, crate::Half::Left);
            assert_eq!(
                left.half,
                crate::Half::Left,
                "{face:?}: küçük kopya çifte cevap oldu"
            );
            assert_ne!(
                left.slot, whole.slot,
                "{face:?}: çift tek hücrenin yuvasında"
            );
        }
    }

    /// İki sütunlu emoji @1x'te iki hücreye de sığmıyor (`fit` iki hücrelik
    /// kutuda 1.062) ve aynı kol onu **iki hücrelik** kutuya küçültüyor: tek
    /// hücreye inmiyor, yani ızgaranın ayırdığı iki sütunu dolduruyor.
    #[test]
    fn wide_emoji_shrinks_into_two_cells_at_1x() {
        let a = Atlas::new(None, 13.0, 1.0, 1.0);
        let base = a.faces.get(Face::Regular);
        let alt = font::fallback_font(base, '😀', a.cell_advance, 2).expect("😀 @1x kutu çıktı");
        assert!(alt.shrunk, "@1x'te iki hücreye sığmıyordu; küçültülmeliydi");
        assert_eq!(alt.cols, 2, "küçültme iki hücrelik kutuya olmalı");
    }

    /// Sınırın **hemen üstündeki** aday kutu kalıyor: `🝇` (Apple Symbols,
    /// taramada `.LastResort` dışında sınırın üstündeki en küçük `fit`,
    /// 2.250). Sınıflama beklentiyi adayın kendi `fit`'inden sınıyor, yani
    /// sınır oynarsa sınama hangi tarafa düştüğünü söylüyor.
    #[test]
    fn just_above_the_limit_stays_tofu() {
        let a = Atlas::new(None, 16.0, 2.0, 1.0);
        let base = a.faces.get(Face::Regular);
        match classify(base, '🝇', a.cell_advance, 1) {
            Class::Rejected { fit, .. } => assert!(
                fit > font::SHRINK_LIMIT && fit < font::SHRINK_LIMIT * 1.05,
                "🝇 fit {fit:.3}: sınırın hemen üstünde olmalıydı"
            ),
            other => panic!("🝇 kutu kalmalıydı: {other:?}"),
        }
    }

    /// `.LastResort` sınırın içinde (`fit` 1.660) ama küçültülmüyor (R3.2):
    /// aday küçültme kolunun geometrik koşulunu karşılıyor ve yine kutu.
    #[test]
    fn last_resort_is_not_shrunk() {
        let a = Atlas::new(None, 16.0, 2.0, 1.0);
        let base = a.faces.get(Face::Regular);
        let ch = '\u{E0A0}';
        let candidate = font::cascade_candidate(base, ch);
        assert!(
            font::is_last_resort(&candidate),
            "U+E0A0 başka bir fonttan geldi"
        );
        let glyph = font::glyph_index(&candidate, ch).expect(".LastResort glyph vermedi");
        let fit = font::fit_ratio(
            a.cell_advance,
            font::glyph_advance(&candidate, glyph),
            font::glyph_ink(&candidate, glyph),
        );
        assert!(
            fit <= font::SHRINK_LIMIT,
            "fit {fit:.3} sınırın içinde olmalı"
        );
        assert!(font::accept(candidate, glyph, a.cell_advance, 1).is_none());
    }

    /// Bugün kapıyı geçen her aday **bit bit aynı** çiziliyor (R3.3): küçültme
    /// en son kol, yani iki kapıdan birini geçen aday aynı fontla (aynı
    /// nesne, aynı punto), `shrunk = false` ve sıfır `rise` ile dönüyor.
    /// Taranan blokların tamamı, 16pt @2x; `⏺` için raster ayrıca 041 öncesi
    /// yolla (`raster::draw`, `rise`'sız sarmalayıcı) bayt bayt karşılaştırılıyor.
    #[test]
    fn gate_accepted_candidates_are_unchanged() {
        let a = Atlas::new(None, 16.0, 2.0, 1.0);
        let base = a.faces.get(Face::Regular);
        let (cell, m) = (a.cell_advance, a.metrics);
        let mut passed = 0usize;
        for (_, first, last) in BLOCKS {
            for ch in (first..=last).filter_map(char::from_u32) {
                if raster::is_procedural(ch) || font::glyph_index(base, ch).is_some() {
                    continue;
                }
                let candidate = font::cascade_candidate(base, ch);
                let Some(glyph) = font::glyph_index(&candidate, ch) else {
                    continue;
                };
                let advance = font::glyph_advance(&candidate, glyph);
                let ink = font::glyph_ink(&candidate, glyph);
                if !font::ink_fits_placed(cell, advance, ink) {
                    continue;
                }
                let ptr = std::ptr::from_ref::<CTFont>(&candidate);
                let alt = font::accept(candidate, glyph, cell, 1).expect("kapıyı geçen döndü");
                assert!(!alt.shrunk, "{ch}: kapıyı geçen aday küçültüldü");
                assert!(std::ptr::eq(ptr, &*alt.font), "{ch}: font değişti");
                assert_eq!(alt.rise(m), 0.0, "{ch}: dikey kayma");
                passed += 1;
            }
        }
        assert!(passed > 300, "kapıyı geçen aday az: {passed}");

        let alt = font::fallback_font(base, '⏺', cell, 1).expect("⏺ kutu çıktı");
        let mut before = vec![0u8; m.slot_bytes()];
        let mut after = vec![0u8; m.slot_bytes()];
        raster::draw(&alt.font, '⏺', m, cell, 0.0, &mut before);
        raster::draw_glyph(&alt.font, alt.glyph, m, cell, 0.0, alt.rise(m), &mut after);
        assert_eq!(before, after, "⏺ rasteri değişti");
    }

    /// Taranan bloklar: (ad, ilk, son).
    const BLOCKS: [(&str, u32, u32); 13] = [
        ("Arrows", 0x2190, 0x21FF),
        ("Mathematical Operators", 0x2200, 0x22FF),
        ("Misc Technical", 0x2300, 0x23FF),
        ("Geometric Shapes", 0x25A0, 0x25FF),
        ("Misc Symbols", 0x2600, 0x26FF),
        ("Dingbats", 0x2700, 0x27BF),
        ("Misc Math Symbols-A", 0x27C0, 0x27EF),
        ("Supplemental Arrows-A", 0x27F0, 0x27FF),
        ("Supplemental Arrows-B", 0x2900, 0x297F),
        ("Misc Math Symbols-B", 0x2980, 0x29FF),
        ("Misc Symbols and Arrows", 0x2B00, 0x2BFF),
        ("Emoji (1F300–1FAFF)", 0x1F300, 0x1FAFF),
        ("Private Use Area", 0xE000, 0xF8FF),
    ];
    /// (punto, ölçek) — R1.1'in dört birleşimi.
    const COMBOS: [(f64, f64); 4] = [(13.0, 1.0), (13.0, 2.0), (16.0, 1.0), (16.0, 2.0)];
    /// Histogramın kova sınırları: <1.2 / 1.2–1.5 / 1.5–1.7 / 1.7+. İlk kova
    /// 1.0'ın altını da taşıyor: sola taşan ya da sola yapışıp sağdan taşan
    /// aday mürekkebi hücreden dar olsa da dönüyor.
    const EDGES: [f64; 3] = [1.2, 1.5, 1.7];
    /// Cascade'in "kimse çizemiyor" cevabı; [`classify`] onu kapının
    /// ölçütüyle bu adla etiketliyor.
    const LAST_RESORT: &str = font::LAST_RESORT;

    fn bucket(v: f64) -> usize {
        EDGES.iter().take_while(|&&e| v >= e).count()
    }

    fn histogram(values: &[f64]) -> String {
        let mut counts = [0usize; 4];
        for &v in values {
            counts[bucket(v)] += 1;
        }
        format!(
            "<1.2: {} · 1.2–1.5: {} · 1.5–1.7: {} · 1.7+: {}",
            counts[0], counts[1], counts[2], counts[3]
        )
    }

    /// Karakter ekranda görünmüyorsa (PUA) kod noktasıyla.
    fn show(ch: char) -> String {
        if ('\u{E000}'..='\u{F8FF}').contains(&ch) {
            format!("U+{:04X}", u32::from(ch))
        } else {
            ch.to_string()
        }
    }

    /// Ardışık kod noktalarını `ilk..son` aralıklarına toplar.
    fn spans(cps: &[u32]) -> String {
        let mut out: Vec<String> = Vec::new();
        let mut i = 0;
        while i < cps.len() {
            let mut j = i;
            while j + 1 < cps.len() && cps[j + 1] == cps[j] + 1 {
                j += 1;
            }
            out.push(if i == j {
                format!("{:04X}", cps[i])
            } else {
                format!("{:04X}..{:04X}", cps[i], cps[j])
            });
            i = j + 1;
        }
        out.join(" ")
    }

    fn range(values: &[f64]) -> String {
        let min = values.iter().copied().fold(f64::INFINITY, f64::min);
        let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        format!("{min:.3}..{max:.3}")
    }

    /// Bir adayın özeti: font başına dökümde tutulan.
    #[derive(Default)]
    struct FontRow {
        accepted: usize,
        shrunk: usize,
        fit2: Vec<f64>,
        rejected: Vec<(char, f64, f64)>,
        wide: usize,
    }

    /// Sembol ve emoji bloklarını kapıdan geçirip grup sayılarını, reddedilen
    /// adayların oran histogramını ve font başına dökümü basar.
    ///
    /// Her karakter **tek sütunlu** sorulur (`cols = 1`): `bt-atlas`
    /// `unicode-width`'i görmüyor ve genişliğin tek yetkilisi `bt-core`'un
    /// tablosu. Reddedilenin ve küçültülenin iki sütunlu sorulduğunda ne
    /// olduğu ayrıca soruluyor (`2h`, "2 hücrelik kutuda fit"): geniş ilan
    /// edilen karakter orada çizilir, tek sütunlu ilan edilen kutu kalır ya da
    /// tek hücreye küçülür. Hangisinin hangisi olduğu ızgaranın sorusu.
    ///
    /// Küçültme için iki tanık basılıyor: küçültülenlerin `fit` dağılımı ve
    /// sınırın içinde olup küçük kopyası yeniden sınamadan dönenler (sıfır
    /// olmalı; olmazsa [`font::SHRINK_LIMIT`]'in türetmesi eskidi).
    ///
    /// Yordamsal aralıklar atlanıyor: font sorulmadan çiziliyorlar. Taban
    /// aile `BT_SCAN_FONT`'tan (yoksa zincirin varsayılanı).
    #[test]
    #[ignore = "makinedeki fontlara bağlı bir envanter; `make tarama`"]
    fn census() {
        let family = std::env::var("BT_SCAN_FONT").ok();
        let mut out = String::new();
        for (pt, scale) in COMBOS {
            let a = Atlas::new(family.as_deref(), pt, scale, 1.0);
            let base = a.faces.get(Face::Regular);
            let cell = a.cell_advance;
            // SAFETY: `base` atlasın elinde canlı.
            let base_name = unsafe { base.family_name() }.to_string();
            let _ = writeln!(
                out,
                "\n=== {pt}pt @{scale}x — taban {base_name}, hücre {cell:.3} px ==="
            );
            let _ = writeln!(
                out,
                "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                "blok", "taban", "yedek", "küçük", "yok", "ret", "(2h)", "(LR)"
            );
            let mut fonts: BTreeMap<String, FontRow> = BTreeMap::new();
            let mut totals = [0usize; 7];
            let mut shrunk_fits: Vec<f64> = Vec::new();
            let mut shrink_failed: Vec<String> = Vec::new();
            // `.LastResort`'a düşen kod noktaları, PUA hariç (orada neredeyse
            // hepsi): hangi karakterde döndüğü phase-2'nin R3.2 sorusu.
            let mut last_resort: Vec<u32> = Vec::new();
            for (name, first, last) in BLOCKS {
                let mut row = [0usize; 7];
                for cp in first..=last {
                    let Some(ch) = char::from_u32(cp) else {
                        continue;
                    };
                    if raster::is_procedural(ch) {
                        continue;
                    }
                    match classify(base, ch, cell, 1) {
                        Class::InBase => row[0] += 1,
                        Class::Fallback { font, .. } => {
                            row[1] += 1;
                            fonts.entry(font).or_default().accepted += 1;
                        }
                        Class::Shrunk { font, fit, .. } => {
                            row[2] += 1;
                            shrunk_fits.push(fit);
                            let entry = fonts.entry(font).or_default();
                            entry.shrunk += 1;
                            if let Class::Rejected { fit, .. } | Class::Shrunk { fit, .. } =
                                classify(base, ch, cell, 2)
                            {
                                entry.fit2.push(fit);
                            }
                        }
                        Class::NoFont => row[3] += 1,
                        Class::Rejected { font, ratio, fit } => {
                            row[4] += 1;
                            let wide_class = classify(base, ch, cell, 2);
                            let wide =
                                !matches!(wide_class, Class::Rejected { .. } | Class::NoFont);
                            if let Class::Rejected { fit, .. } | Class::Shrunk { fit, .. } =
                                wide_class
                            {
                                fonts.entry(font.clone()).or_default().fit2.push(fit);
                            }
                            if wide {
                                row[5] += 1;
                            }
                            if font != LAST_RESORT && fit <= font::SHRINK_LIMIT {
                                shrink_failed.push(format!("{}({fit:.3})", show(ch)));
                            }
                            if font == LAST_RESORT {
                                row[6] += 1;
                                if first != 0xE000 {
                                    last_resort.push(cp);
                                }
                            }
                            let entry = fonts.entry(font).or_default();
                            entry.rejected.push((ch, ratio, fit));
                            entry.wide += usize::from(wide);
                        }
                    }
                }
                for (t, r) in totals.iter_mut().zip(row) {
                    *t += r;
                }
                let _ = writeln!(
                    out,
                    "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                    name, row[0], row[1], row[2], row[3], row[4], row[5], row[6]
                );
            }
            let _ = writeln!(
                out,
                "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                "TOPLAM",
                totals[0],
                totals[1],
                totals[2],
                totals[3],
                totals[4],
                totals[5],
                totals[6]
            );
            let _ = writeln!(
                out,
                "küçültülen: fit {} ({}) · sınırın içinde yeniden sınamadan dönen: {} {}",
                range(&shrunk_fits),
                histogram(&shrunk_fits),
                shrink_failed.len(),
                shrink_failed.join(" ")
            );

            let _ = writeln!(out, ".LastResort (PUA dışı): {}", spans(&last_resort));
            let real: Vec<&(char, f64, f64)> = fonts
                .iter()
                .filter(|(f, _)| f.as_str() != LAST_RESORT)
                .flat_map(|(_, r)| &r.rejected)
                .collect();
            let ratios: Vec<f64> = real.iter().map(|r| r.1).collect();
            let fits: Vec<f64> = real.iter().map(|r| r.2).collect();
            let listed: String = real
                .iter()
                .map(|r| format!("{}({:.3}) ", show(r.0), r.2))
                .collect();
            let _ = writeln!(
                out,
                "ret (.LastResort hariç, {} aday): {listed}",
                real.len()
            );
            let _ = writeln!(out, "  ratio {}", histogram(&ratios));
            let _ = writeln!(out, "  fit   {}", histogram(&fits));
            let _ = writeln!(
                out,
                "font başına (kabul / küçük / ret, 2h = iki sütunluda çizilen ret):"
            );
            for (font, row) in &fonts {
                if row.rejected.is_empty() {
                    let _ = writeln!(
                        out,
                        "  {font}: {} kabul / {} küçük — 2 hücrelik kutuda fit {}",
                        row.accepted,
                        row.shrunk,
                        range(&row.fit2)
                    );
                    continue;
                }
                let r: Vec<f64> = row.rejected.iter().map(|x| x.1).collect();
                let f: Vec<f64> = row.rejected.iter().map(|x| x.2).collect();
                let _ = writeln!(
                    out,
                    "  {font}: {} kabul / {} küçük / {} ret (2h {}) — ratio {} · fit {} · \
                     2 hücrelik kutuda fit {}",
                    row.accepted,
                    row.shrunk,
                    row.rejected.len(),
                    row.wide,
                    range(&r),
                    range(&f),
                    range(&row.fit2)
                );
                // Sınırın bulunacağı bölge: fit < 1.5'in karakterleri.
                let near: String = row
                    .rejected
                    .iter()
                    .filter(|x| x.2 < EDGES[1])
                    .map(|x| format!("{}({:.2}/{:.2}) ", show(x.0), x.1, x.2))
                    .collect();
                if !near.is_empty() && font != LAST_RESORT {
                    let _ = writeln!(out, "    fit<1.5: {near}");
                }
            }
        }
        println!("{out}");
    }
}
