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
//! Modül yalnız sınamada derleniyor: sınıflamanın üretimde tüketicisi yok ve
//! kapının kendisi (`font::fallback_font`) değişmiyor.

use objc2_core_foundation::{CGFloat, CGRect};
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
    /// Aday var ama kapıdan döndü: ekranda kutu.
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
///   ortaya gelmiyor. Ölçütü [`font::ink_fits_placed`], yani kapının kendi
///   kuralı.
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
    // SAFETY: `candidate` bu kapsamda canlı; saf okuma.
    let family = unsafe { candidate.family_name() }.to_string();
    let advance = font::glyph_advance(&candidate, glyph);
    let ink = font::glyph_ink(&candidate, glyph);
    let box_advance = cell_advance * CGFloat::from(cols);
    let ratio = ink.size.width / box_advance;
    match font::accept(candidate, glyph, cell_advance, cols) {
        Some(_) => Class::Fallback {
            font: family,
            ratio,
        },
        None => Class::Rejected {
            font: family,
            ratio,
            fit: fit_ratio(box_advance, advance, ink),
        },
    }
}

/// Glyph'i `1 / fit` ölçeğiyle küçültünce kapının geçtiği en küçük `fit`.
///
/// İkiye bölme, kapalı form değil: kural [`font::ink_fits_placed`]'in
/// kendisi ve kapalı form onu (sola yapışma kolu dahil) ikinci kez yazmak
/// olurdu. Ölçek sıfıra giderken her glyph kutunun ortasına küçülüp sığıyor,
/// yani alt uç her zaman geçer; üst uç (`1`) çağıranda kapıdan dönmüş aday.
fn fit_ratio(box_advance: CGFloat, advance: CGFloat, ink: CGRect) -> f64 {
    let fits = |s: CGFloat| {
        let mut scaled = ink;
        scaled.origin.x *= s;
        scaled.size.width *= s;
        font::ink_fits_placed(box_advance, advance * s, scaled)
    };
    if fits(1.0) {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..60 {
        let mid = (lo + hi) / 2.0;
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    1.0 / lo
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
/// - `⧉` Apple Symbols'tan geliyor, mürekkebi hücreden ~%11 geniş; 041
///   phase-2'nin küçültmesinin hedefi.
/// - `U+E0A0`/`U+E0B0` hiçbir kurulu fontta yok, cascade `.LastResort`'u
///   veriyor. R3.2 onun küçültülmesini yasaklıyor, yani küçültme bunları
///   boşaltmaz; boşaltan şey Nerd Font kurulu bir makine.
const EXPECTED_TOFU: [char; 3] = ['⧉', '\u{E0A0}', '\u{E0B0}'];

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
    /// Cascade'in "kimse çizemiyor" cevabı. Yalnız **raporun** ayrımı:
    /// kapı onu tanımıyor ve bu dizge kapıya girmiyor.
    const LAST_RESORT: &str = ".LastResort";

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
        rejected: Vec<(char, f64, f64)>,
        wide: usize,
    }

    /// Sembol ve emoji bloklarını kapıdan geçirip grup sayılarını, reddedilen
    /// adayların oran histogramını ve font başına dökümü basar.
    ///
    /// Her karakter **tek sütunlu** sorulur (`cols = 1`): `bt-atlas`
    /// `unicode-width`'i görmüyor ve genişliğin tek yetkilisi `bt-core`'un
    /// tablosu. Reddedilenin iki hücreye sığıp sığmadığı ayrıca soruluyor
    /// (`2 hücreye sığar`): geniş ilan edilen karakter orada çizilir, tek
    /// sütunlu ilan edilen kutu kalır. Hangisinin hangisi olduğu ızgaranın
    /// sorusu.
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
                "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                "blok", "taban", "yedek", "yok", "ret", "(2h)", "(LR)"
            );
            let mut fonts: BTreeMap<String, FontRow> = BTreeMap::new();
            let mut totals = [0usize; 6];
            // `.LastResort`'a düşen kod noktaları, PUA hariç (orada neredeyse
            // hepsi): hangi karakterde döndüğü phase-2'nin R3.2 sorusu.
            let mut last_resort: Vec<u32> = Vec::new();
            for (name, first, last) in BLOCKS {
                let mut row = [0usize; 6];
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
                        Class::NoFont => row[2] += 1,
                        Class::Rejected { font, ratio, fit } => {
                            row[3] += 1;
                            let wide =
                                matches!(classify(base, ch, cell, 2), Class::Fallback { .. });
                            if wide {
                                row[4] += 1;
                            }
                            if font == LAST_RESORT {
                                row[5] += 1;
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
                    "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                    name, row[0], row[1], row[2], row[3], row[4], row[5]
                );
            }
            let _ = writeln!(
                out,
                "{:<26} {:>6} {:>6} {:>6} {:>6} {:>6} {:>6}",
                "TOPLAM", totals[0], totals[1], totals[2], totals[3], totals[4], totals[5]
            );

            let _ = writeln!(out, ".LastResort (PUA dışı): {}", spans(&last_resort));
            let real: Vec<&(char, f64, f64)> = fonts
                .iter()
                .filter(|(f, _)| f.as_str() != LAST_RESORT)
                .flat_map(|(_, r)| &r.rejected)
                .collect();
            let ratios: Vec<f64> = real.iter().map(|r| r.1).collect();
            let fits: Vec<f64> = real.iter().map(|r| r.2).collect();
            let _ = writeln!(out, "ret (.LastResort hariç, {} aday):", real.len());
            let _ = writeln!(out, "  ratio {}", histogram(&ratios));
            let _ = writeln!(out, "  fit   {}", histogram(&fits));
            let _ = writeln!(
                out,
                "font başına (kabul / ret, 2h = iki hücreye sığan ret):"
            );
            for (font, row) in &fonts {
                if row.rejected.is_empty() {
                    let _ = writeln!(out, "  {font}: {} kabul", row.accepted);
                    continue;
                }
                let r: Vec<f64> = row.rejected.iter().map(|x| x.1).collect();
                let f: Vec<f64> = row.rejected.iter().map(|x| x.2).collect();
                let _ = writeln!(
                    out,
                    "  {font}: {} kabul / {} ret (2h {}) — ratio {} · fit {}",
                    row.accepted,
                    row.rejected.len(),
                    row.wide,
                    range(&r),
                    range(&f)
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
