//! Renk çözümü: bir hücrenin `Color`'ı ile ekrandaki RGBA arasındaki tek yol.
//!
//! Tema modeli (sekiz rol, palet dosyaları) 00X'te gelir; burası o gelene
//! kadar tek sahiptir. Renderer palet bilmez: `frame()` çözülmüş RGBA verir.
//!
//! Renkler `0xRRGGBB` olarak yazılır — palet her yerde böyle yazılır ve
//! `Rgb { r, g, b }` üçlüsü onaltı satırlık bir tabloyu okunmaz eder.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, Rgb};

/// Varsayılan arka plan, **lineer** RGBA (`lineer_rgba`, crate-içi).
/// **Tek sahibi burasıdır**: pencerenin clear rengi de,
/// `frame()`'in "bu hücre varsayılan, çizilmesin" kararı da buradan okur. İki
/// yerde dursaydı biri değişince pencere ile hücreler ayrı renk olurdu.
pub const DEFAULT_BG: [f32; 4] = lineer_rgba(rgb(BG));

/// İmleç bloğunun rengi, **lineer** RGBA (`lineer_rgba`, crate-içi). Renderer'da
/// sabit durmasın diye burada: renk kararı
/// paletin, çizim kararı renderer'ın.
pub const DEFAULT_CURSOR: [f32; 4] = lineer_rgba(rgb(CURSOR));

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
///
/// `#[inline]`: `lineer_rgba` ile aynı sıcak yol ve aynı gerekçe. `default`
/// ile **birlikte** işaretlenir; yalnız biri alınırsa çağrı ötekine kayar.
#[inline]
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
///
/// `#[inline]`: tek çarpma, ama `resolve`/`default` ile aynı sıcak yolda ve
/// aynı sebeple (LTO kapalı) crate sınırında çağrıya dönüşüyordu.
#[inline]
pub(crate) fn dim(color: Rgb) -> Rgb {
    color * DIM
}

/// Paletin `index` numaralı rengi. Numaralandırma alacritty'nin `term::color`
/// tablosudur: 0..16 ANSI, 16..232 küp, 232..256 gri rampa, 256+ rol renkleri.
///
/// Soğuk değil: taze bir oturumda `Colors` tablosu boştur (yalnız OSC 4/10/11
/// doldurur), yani `resolve` her hücrede buraya düşer.
#[inline]
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

/// sRGB kodlu 8-bit kanal → lineer f32. Kaynağı IEC 61966-2-1'in transfer
/// fonksiyonu (`c/12.92`, kırılmadan sonra `((c+0.055)/1.055)^2.4`).
///
/// Tablo, elle bakılan bir sabit listesi **değil**, türetilmiş bir veridir:
/// `srgb_tablosu_transfer_fonksiyonunu_izler` her girdiyi formüle bağlar.
/// Tablo olmasının sebebi `const`luk — `powf` stable'da `const` değil, oysa
/// `DEFAULT_BG` ve `DEFAULT_CURSOR` `const`.
// rustfmt tabloyu girdi başına bir satıra açıyor: 64 satır 256 olur ve
// dosyanın geri kalanı okunmaz hâle gelir. `ANSI` tablosuyla aynı gerekçe.
#[rustfmt::skip]
const SRGB_LINEER: [f32; 256] = [
    0.0, 0.000303527, 0.000607054, 0.000910581,
    0.001214108, 0.001517635, 0.001821162, 0.0021246888,
    0.002428216, 0.0027317428, 0.00303527, 0.0033465358,
    0.0036765074, 0.004024717, 0.004391442, 0.0047769533,
    0.0051815165, 0.0056053917, 0.006048833, 0.0065120906,
    0.00699541, 0.007499032, 0.008023193, 0.008568126,
    0.009134059, 0.009721218, 0.010329823, 0.010960094,
    0.011612245, 0.012286488, 0.0129830325, 0.013702083,
    0.014443844, 0.015208514, 0.015996294, 0.016807375,
    0.017641954, 0.01850022, 0.019382361, 0.020288562,
    0.02121901, 0.022173885, 0.023153367, 0.024157632,
    0.02518686, 0.026241222, 0.027320892, 0.02842604,
    0.029556835, 0.030713445, 0.031896032, 0.033104766,
    0.034339808, 0.035601314, 0.03688945, 0.038204372,
    0.039546236, 0.0409152, 0.04231141, 0.04373503,
    0.045186203, 0.046665087, 0.048171826, 0.049706567,
    0.051269457, 0.052860647, 0.054480277, 0.05612849,
    0.05780543, 0.059511237, 0.061246052, 0.063010015,
    0.064803265, 0.06662594, 0.06847817, 0.070360094,
    0.07227185, 0.07421357, 0.07618538, 0.07818742,
    0.08021982, 0.08228271, 0.08437621, 0.08650046,
    0.08865558, 0.09084171, 0.093058966, 0.09530747,
    0.09758735, 0.099898726, 0.10224173, 0.104616486,
    0.107023105, 0.10946171, 0.11193243, 0.114435375,
    0.116970666, 0.11953843, 0.122138776, 0.12477182,
    0.12743768, 0.13013647, 0.13286832, 0.13563333,
    0.13843161, 0.14126329, 0.14412847, 0.14702727,
    0.14995979, 0.15292615, 0.15592647, 0.15896083,
    0.16202937, 0.1651322, 0.1682694, 0.17144111,
    0.1746474, 0.17788842, 0.18116425, 0.18447499,
    0.18782078, 0.19120169, 0.19461784, 0.19806932,
    0.20155625, 0.20507874, 0.20863687, 0.21223076,
    0.2158605, 0.2195262, 0.22322796, 0.22696587,
    0.23074006, 0.23455058, 0.23839757, 0.24228112,
    0.24620132, 0.25015828, 0.2541521, 0.25818285,
    0.26225066, 0.2663556, 0.2704978, 0.2746773,
    0.27889428, 0.28314874, 0.28744084, 0.29177064,
    0.29613826, 0.30054379, 0.3049873, 0.30946892,
    0.31398872, 0.31854677, 0.3231432, 0.3277781,
    0.33245152, 0.33716363, 0.34191442, 0.34670407,
    0.3515326, 0.35640013, 0.3613068, 0.3662526,
    0.3712377, 0.37626213, 0.38132602, 0.38642943,
    0.39157248, 0.39675522, 0.40197778, 0.4072402,
    0.4125426, 0.41788507, 0.42326766, 0.4286905,
    0.43415365, 0.43965718, 0.4452012, 0.4507858,
    0.45641103, 0.462077, 0.4677838, 0.47353148,
    0.47932017, 0.48514995, 0.49102086, 0.49693298,
    0.5028865, 0.50888133, 0.5149177, 0.52099556,
    0.5271151, 0.5332764, 0.5394795, 0.54572445,
    0.55201143, 0.5583404, 0.5647115, 0.57112485,
    0.57758045, 0.58407843, 0.59061885, 0.59720176,
    0.60382736, 0.61049557, 0.6172066, 0.6239604,
    0.63075715, 0.63759685, 0.6444797, 0.65140563,
    0.65837485, 0.6653873, 0.67244315, 0.6795425,
    0.6866853, 0.69387174, 0.7011019, 0.70837575,
    0.7156935, 0.7230551, 0.73046076, 0.7379104,
    0.7454042, 0.7529422, 0.7605245, 0.76815116,
    0.7758222, 0.7835378, 0.7912979, 0.7991027,
    0.80695224, 0.8148466, 0.82278574, 0.8307699,
    0.838799, 0.8468732, 0.8549926, 0.8631572,
    0.8713671, 0.8796224, 0.8879231, 0.8962694,
    0.9046612, 0.91309863, 0.92158186, 0.9301109,
    0.9386857, 0.9473065, 0.9559733, 0.9646863,
    0.9734453, 0.9822506, 0.9911021, 1.0,
];

/// Rengi renderer'ın beklediği **lineer** RGBA'ya çevirir.
///
/// Ad uzayı taşıyor çünkü bu depoda sessizce yanlış olabilecek tek şey bir
/// float'ın hangi uzayda olduğu: çizim hedefi `BGRA8Unorm_sRGB` ve donanım
/// fragment çıktısını lineer sayıp yazarken kodluyor. Burada `c / 255.0`
/// dönseydi palet `0x1a1c21`'den `0x5a5d65` griye açılırdı; bunu gören tek
/// bekçi `bt-gpu`'nun `cell_bg_pikseli_gpu_tarafinda_boyar` sınamasıdır ve
/// ancak **ara ton** bir renkle görüyor.
///
/// Aynı floatlar `MTLClearColor`'a da gidiyor — Metal sRGB hedefte clear
/// rengini de lineer okur, yani pencere zemini ile hücreler tek kaynaktan
/// düzeliyor.
///
/// `#[inline]`: hücre başına, kare başına çağrılıyor (`Session::frame`) ve
/// LTO kapalı (`[profile.release]` yok); işaret olmadan gövde crate sınırını
/// geçmiyordu — `nm -u` release rlib'inde tanımsız sembol gösteriyordu, yani
/// bir tablo aramasının etrafında gerçek bir çağrı kalıyordu.
#[inline]
pub(crate) const fn lineer_rgba(color: Rgb) -> [f32; 4] {
    [
        // audit: `u8 as usize` 0..=255, tablo 256 girdilik — indeks tipin
        // kendisiyle sınırlı, sınır kontrolü kodgen'de de eleniyor.
        SRGB_LINEER[color.r as usize],
        SRGB_LINEER[color.g as usize],
        SRGB_LINEER[color.b as usize],
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
    }

    #[test]
    fn srgb_tablosu_transfer_fonksiyonunu_izler() {
        // Tablo elle yazılmış 256 sayı değil, formülün donmuş hâli. Referans
        // f64'te hesaplanır; tablo f32 olduğu için epsilon yalnız f32
        // yuvarlamasını karşılar (mutlak hata ≤ ~6e-8).
        assert_eq!(SRGB_LINEER[0], 0.0);
        assert_eq!(SRGB_LINEER[255], 1.0, "beyaz lineerde de 1.0 kalmalı");
        for (i, &lineer) in SRGB_LINEER.iter().enumerate() {
            let c = i as f64 / 255.0;
            let beklenen = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            assert!(
                (f64::from(lineer) - beklenen).abs() < 1e-7,
                "{i}: {lineer} != {beklenen}"
            );
            // Tablonun gerçekten **lineerleştirdiğinin** kanıtı, GPU
            // istemeden: lineer değer sRGB-kodlu hâlinden (`i/255`) kesin
            // olarak küçüktür. Tablo yerine `c / 255.0` konsaydı bu satır
            // düşerdi. Uçlar (0 ve 255) transfer fonksiyonunun sabit
            // noktaları, eşitlik oradan gelir ve aralığın dışında bırakılır.
            if (1..255).contains(&i) {
                assert!(f64::from(lineer) < c, "{i}: {lineer} !< {c}");
            }
        }
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
