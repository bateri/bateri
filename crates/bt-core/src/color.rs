//! Renk çözümü: bir hücrenin `Color`'ı ile ekrandaki RGBA arasındaki tek yol.
//!
//! Tema modeli (sekiz rol, palet dosyaları) 00X'te gelir; burası o gelene
//! kadar tek sahiptir. Renderer palet bilmez: `frame()` çözülmüş RGBA verir.
//!
//! Renkler `0xRRGGBB` olarak yazılır — palet her yerde böyle yazılır ve
//! `Rgb { r, g, b }` üçlüsü onaltı satırlık bir tabloyu okunmaz eder.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, Rgb};

/// Varsayılan arka plan. **Tek sahibi burasıdır**: pencerenin clear rengi de,
/// `frame()`'in "bu hücre varsayılan, çizilmesin" kararı da buradan okur. İki
/// yerde dursaydı biri değişince pencere ile hücreler ayrı renk olurdu.
pub const DEFAULT_BG: [f32; 4] = rgba(rgb(BG));

/// İmleç bloğunun rengi. Renderer'da sabit durmasın diye burada: renk kararı
/// paletin, çizim kararı renderer'ın.
pub const DEFAULT_CURSOR: [f32; 4] = rgba(rgb(CURSOR));

/// Varsayılan arka planın `Rgb` hâli; `frame()` karşılaştırmayı burada yapar,
/// f32 eşitliği aramaz.
pub(crate) const BG_RGB: Rgb = rgb(BG);

/// Sönük (`DIM`) renklerin çarpanı. Çarpma vte'nin `impl Mul<f32> for Rgb`'si
/// (`vte/src/ansi.rs`, yorumu birebir "the default dim is just *2/3"): `f32`'de
/// hesaplar ve `clamp(0.0, 255.0)` uygular, yani kanal taşması diye bir sınıf yok.
const DIM: f32 = 2.0 / 3.0;

const BG: u32 = 0x1a1c21;
const FG: u32 = 0xd8d9dd;
const CURSOR: u32 = 0x7a9cc6;

/// 16 ANSI rengi; nötr gri taban üstünde doygunluğu kırılmış tonlar. Siyah
/// bilerek arka plandan ayrıdır — `\e[40m` çizilmeyen bir hücre değil, görünür
/// bir blok olmalı.
// Satır başına dörtlü düzen ve sağdaki ad yorumları taşıyıcı bilgidir:
// rengin hangi ANSI adına düştüğü ancak bu hizadan okunuyor. rustfmt tabloyu
// tek sütuna açıp hizayı yok ediyor.
#[rustfmt::skip]
const ANSI: [u32; 16] = [
    0x22252b, 0xd16d6a, 0x8bb58b, 0xd6b16a, // siyah   kırmızı  yeşil    sarı
    0x7a9cc6, 0xb08ec0, 0x79b3b3, 0xc8c9cc, // mavi    macenta  camgöbeği beyaz
    0x4a4e57, 0xe58b88, 0xa4cba4, 0xe8c988, // parlak sekizlisi, aynı sırada
    0x9bb8dc, 0xc9aad8, 0x96caca, 0xe6e7ea,
];

/// 6×6×6 renk küpünün kanal basamakları (xterm sözleşmesi).
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Hücrenin rengini ekranın rengine çevirir.
///
/// `colors` uygulamanın OSC 4/10/11 ile değiştirdiği tablodur ve girdileri
/// `None` olabilir; boş girdide bizim varsayılan paletimize düşülür.
pub(crate) fn resolve(color: Color, colors: &Colors) -> Rgb {
    let index = match color {
        Color::Spec(spec) => return spec,
        Color::Named(named) => named as usize,
        Color::Indexed(index) => index as usize,
    };
    // Tablo 269 girdilik; `NamedColor` en çok 268, `Indexed` en çok 255.
    colors[index].unwrap_or_else(|| default(index)) // audit: indeks sınırlı
}

/// Rengi sönük (`DIM`) hâline indirir.
pub(crate) fn dim(color: Rgb) -> Rgb {
    color * DIM
}

/// Paletin `index` numaralı rengi. Numaralandırma alacritty'nin `term::color`
/// tablosudur: 0..16 ANSI, 16..232 küp, 232..256 gri rampa, 256+ rol renkleri.
pub(crate) fn default(index: usize) -> Rgb {
    match index {
        0..=15 => rgb(ANSI[index]), // audit: kol indeksi 0..16'ya bağlar
        16..=231 => {
            let n = index - 16;
            Rgb {
                r: CUBE[n / 36],
                g: CUBE[(n / 6) % 6],
                b: CUBE[n % 6],
            }
        }
        232..=255 => {
            let v = 8 + 10 * (index - 232) as u8; // audit: kol bağlar, en çok 238
            Rgb { r: v, g: v, b: v }
        }
        256 => rgb(FG),
        257 => rgb(BG),
        258 => rgb(CURSOR),
        // 259..=266 sönük ANSI sekizlisi, 267 parlak ön plan, 268 sönük ön plan.
        259..=266 => rgb(ANSI[index - 259]) * DIM,
        267 => rgb(FG),
        268 => rgb(FG) * DIM,
        // Tablo 269 girdilik; buraya düşen bir indeks alacritty'nin değişmesi
        // demektir. Renk yerine arka plan verip sessiz kalırız, panik etmeyiz.
        _ => rgb(BG),
    }
}

const fn rgb(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

pub(crate) const fn rgba(color: Rgb) -> [f32; 4] {
    [
        color.r as f32 / 255.0,
        color.g as f32 / 255.0,
        color.b as f32 / 255.0,
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::NamedColor;

    #[test]
    fn varsayilan_arka_plan_tek_kaynak() {
        // Hücrenin `Named(Background)`'ı ile pencerenin clear rengi aynı
        // sabitten gelmeli; ayrılırlarsa boş hücreler pencereden farklı boyanır.
        assert_eq!(default(NamedColor::Background as usize), BG_RGB);
        assert_eq!(rgba(BG_RGB), DEFAULT_BG);
    }

    #[test]
    fn palet_indeksleri_xterm_sozlesmesi() {
        assert_eq!(default(1), rgb(ANSI[1]));
        // 16 = küpün başı (0,0,0), 231 = sonu (255,255,255).
        assert_eq!(default(16), rgb(0x000000));
        assert_eq!(default(231), rgb(0xffffff));
        // Gri rampa 8'den başlar, 10'ar artar.
        assert_eq!(default(232), rgb(0x080808));
        assert_eq!(default(255), rgb(0xeeeeee));
    }

    #[test]
    fn sonuk_renkler_kaynagindan_koyu() {
        // Sönük sekizli ANSI 0..8'in, sönük ön plan da ön planın altında kalır.
        for index in 259..=266 {
            let sonuk = default(index);
            let parlak = default(index - 259);
            assert!(sonuk.r < parlak.r || parlak.r == 0, "{index}");
            assert!(sonuk.g <= parlak.g && sonuk.b <= parlak.b, "{index}");
        }
        assert!(default(268).r < default(256).r);
        // Kırmızının sönüğü: 209/109/106 → 139/72/70. vte f32'de çarpıp kırpar.
        assert_eq!(default(260), rgb(0x8b4846));
    }

    #[test]
    fn osc_tablosu_paleti_ezer() {
        let mut colors = Colors::default();
        let ozel = rgb(0x010203);
        colors[1] = Some(ozel);
        assert_eq!(resolve(Color::Named(NamedColor::Red), &colors), ozel);
        assert_eq!(resolve(Color::Indexed(1), &colors), ozel);
        // Doğrudan verilen renk tabloya hiç sormaz.
        assert_eq!(resolve(Color::Spec(rgb(ANSI[2])), &colors), rgb(ANSI[2]));
        // Tabloda olmayan girdi paletten gelir.
        assert_eq!(resolve(Color::Indexed(2), &colors), rgb(ANSI[2]));
    }
}
