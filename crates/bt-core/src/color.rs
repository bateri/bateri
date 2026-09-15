//! Renk çözümü: bir hücrenin `Color`'ı ile ekrandaki RGBA arasındaki tek yol.
//!
//! Paletin sahibi [`Theme`]: zemin, ön plan, sönük ön plan, imleç ve 16 ANSI
//! rengi tek değerde. Renderer palet bilmez: `frame()` çözülmüş RGBA verir,
//! clear ve imleç rengini de aynı temadan alır. Tema dosyasının ayrıştırıcısı
//! `theme` modülünde.
//!
//! Renkler `0xRRGGBB` olarak yazılır — palet her yerde böyle yazılır ve
//! `Rgb { r, g, b }` üçlüsü onaltı satırlık bir tabloyu okunmaz eder.

use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};

/// Çizim hedefinin uzayındaki renk: **lineer** RGBA.
///
/// Newtype, çünkü simetrik hatanın yalnız yarısı temsil edilemezdi: `bt-gpu`
/// tarafında "sRGB olmayan hedef" bir `const`la (`Renderer::PIXEL_FORMAT`)
/// kapatıldı, ama sınırın bu tarafında renk çıplak bir `[f32; 4]`'tü ve uzayı
/// yalnız bir yorum söylüyordu. Oraya sRGB-kodlu bir float (`c / 255.0`)
/// yazan renk açılır — `0x1a1c21` `0x5a5d65` griye — ve belirti sessizdir.
/// Alan private ve tek kurucusu [`LinearRgba::from_srgb`], yani dönüşüm
/// tipin içinde: uzayı artık bir yorum değil tip taşıyor.
///
/// `Eq` yok, `PartialEq` var: bileşenler `f32` ve karşılaştırılan şey hep
/// aynı tablodan çıkmış iki değer, hesaplanmış iki değer değil.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinearRgba([f32; 4]);

impl LinearRgba {
    /// sRGB kodlu 8-bit üçlüden — **tek kurucu**.
    ///
    /// Girdi kasten `u8`: renkler her yerde `0xRRGGBB` yazılır ve
    /// lineerleştirme crate'in içinde kalır. Lineer float alan bir kurucu
    /// olsaydı newtype yalnız bir ad olurdu; kapattığı hata "sRGB float'ı
    /// lineer yuvaya koymak" ve o hata ancak dönüşüm **burada** olduğunda
    /// temsil edilemez hâle geliyor.
    ///
    /// Paletin tek sahibi olmasını bu kurucu vermiyor, o ayrı bir kural
    /// (bu modülün başı): rengi buradan üretebilmek onu palete yazmak
    /// değildir.
    pub const fn from_srgb(r: u8, g: u8, b: u8) -> Self {
        Self([
            // audit: `u8 as usize` 0..=255, tablo 256 girdilik — indeks tipin
            // kendisiyle sınırlı, sınır kontrolü kodgen'de de eleniyor.
            SRGB_LINEAR[r as usize],
            SRGB_LINEAR[g as usize],
            SRGB_LINEAR[b as usize],
            1.0,
        ])
    }

    /// GPU'ya giden dört bileşen.
    ///
    /// `const`: `Theme::BATERI.background_linear()` gibi sabitlerin derleme
    /// zamanında da açılması gerekiyor (`bt-gpu`'nun sınamaları).
    pub const fn to_array(self) -> [f32; 4] {
        self.0
    }
}

/// Bir renk teması: **tek kaynak**.
///
/// Pencerenin clear rengi ([`Theme::background_linear`]), `frame()`'in "bu
/// hücre varsayılan, çizilmesin" kararı, imleç bloğu
/// ([`Theme::accent_linear`]) ve uygulamanın renk sorusuna (OSC 10/11) verilen
/// yanıt hep aynı değerden okunur. İki yerde dursalardı biri değişince
/// pencere ile hücreler ayrı renk olurdu.
///
/// Sekiz rollü modelin 007'de tüketicisi olan dördü burada; dört durum rolü
/// 013 ile gelir. Alanlar `0xRRGGBB` (üst bayt okunmaz) ve `pub`: tip bir
/// kayıt, `Settings` gibi; geçerliliğini kuran yol tema ayrıştırıcısı
/// ([`Theme::parse`]). Alacritty'nin `Rgb`'si `pub` yüzde görünmez.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Varsayılan arka plan; pencerenin zemini.
    pub background: u32,
    /// Varsayılan ön plan.
    pub foreground: u32,
    /// Sönük (SGR 2) varsayılan ön plan. Adlı ve dolaylı renklerin sönüğü
    /// bir kuraldan, zemine karıştırarak gelir ([`dim_toward`]); bu rol
    /// yalnız varsayılan ön planın.
    pub dim: u32,
    /// Vurgu; bugün imleç bloğu.
    pub accent: u32,
    /// 16 ANSI rengi: siyah, kırmızı, yeşil, sarı, mavi, macenta, camgöbeği,
    /// beyaz, sonra aynı sırada parlak sekizlisi.
    pub ansi: [u32; 16],
}

impl Theme {
    /// Gömülü koyu tema — ayar dosyası yokken ve süreli koşuda geçerli olan.
    ///
    /// ANSI tonları nötr gri taban üstünde doygunluğu kırılmış renkler.
    /// Siyah bilerek arka plandan ayrıdır — `\e[40m` çizilmeyen bir hücre
    /// değil, görünür bir blok olmalı. `dim`, `foreground × 2/3`'ün vte
    /// çarpımıyla (`f32`, kesme) sonucu: 006'ya kadar sönük ön plan böyle
    /// hesaplanıyordu. Rol bir değer, kural değil — 007 phase-3'te adlı
    /// renklerin sönüğü zemine karışmaya başladığında bu değer yerinde kaldı.
    ///
    /// `const`: `bt-gpu`'nun sınamaları clear ve imleç rengini `const`
    /// bağlamda buradan alıyor. sRGB tablosunun `const` olmasının gerekçesi
    /// de bu.
    // Satır başına dörtlü düzen ve sağdaki ad yorumları taşıyıcı bilgidir:
    // rengin hangi ANSI adına düştüğü ancak bu hizadan okunuyor. rustfmt
    // tabloyu tek sütuna açıp hizayı yok ediyor.
    #[rustfmt::skip]
    pub const BATERI: Theme = Theme {
        background: 0x1a1c21,
        foreground: 0xd8d9dd,
        dim: 0x909093,
        accent: 0x7a9cc6,
        ansi: [
            0x22252b, 0xd16d6a, 0x8bb58b, 0xd6b16a, // siyah   kırmızı  yeşil    sarı
            0x7a9cc6, 0xb08ec0, 0x79b3b3, 0xc8c9cc, // mavi    macenta  camgöbeği beyaz
            0x4a4e57, 0xe58b88, 0xa4cba4, 0xe8c988, // parlak sekizlisi, aynı sırada
            0x9bb8dc, 0xc9aad8, 0x96caca, 0xe6e7ea,
        ],
    };

    /// Gömülü açık tema — `[appearance] theme = "system"` açık görünümde bunu
    /// seçer (`light_theme`'in varsayılanı).
    ///
    /// Değerler göz kontrolüyle kabul edildi; ölçütleri:
    ///
    /// - **ANSI adlarının anlamı korunur.** 0 (siyah) koyu uç, 7 ve 15 (beyaz)
    ///   açık uç. Açık zeminde beyaz metin zayıf okunur ama adı "beyaz" olan
    ///   rengi koyulaştırmak onu zemin bloğu olarak kullanan uygulamayı
    ///   (`\e[47m`, tmux çubuğu) bozardı. Parlak beyaz yine zeminden ayrık:
    ///   `\e[107m` görünür bir blok kalmalı, `BATERI`'nin siyahıyla aynı
    ///   gerekçe.
    /// - **Renkli sekizli açık zeminde okunur.** Koyu temanın pastelleri beyaz
    ///   üstünde kaybolurdu; sarı ve camgöbeği bu yüzden koyu, doygun tonlarda
    ///   (hardal, petrol). Parlak sekizli normalden biraz açık ama metin rengi
    ///   olarak kullanılabilir kalır — `ls --color`'ın dizini, `git diff`'in
    ///   eklenen satırı.
    /// - **İmleç zeminden ve ön plandan ayrışır:** koyu mavi blok; altındaki
    ///   harf zemin rengiyle çizildiği için (`Session::frame`) bloğun zemin
    ///   rengine karşı da okunur olması gerekiyor.
    /// - `dim`, ön planın zemine karışmış hâli ([`dim_toward`]) — adlı
    ///   renklerin sönüğüyle aynı kural, ayrı bir zevk değil.
    #[rustfmt::skip]
    pub const BATERI_LIGHT: Theme = Theme {
        background: 0xf5f6f8,
        foreground: 0x24262c,
        dim: 0x696b70,
        accent: 0x3d6aa8,
        ansi: [
            0x2b2e35, 0xb5423d, 0x3b7a3b, 0x8f6a00, // siyah   kırmızı  yeşil    sarı
            0x3a66a6, 0x8a4c9c, 0x23787f, 0xb9bbc1, // mavi    macenta  camgöbeği beyaz
            0x70737b, 0xc9504a, 0x4a8f4a, 0xa67c00, // parlak sekizlisi, aynı sırada
            0x4a78ba, 0x9d5db0, 0x2f8a92, 0xdcdee3,
        ],
    };

    /// Uygulamaya gömülü temalar, adıyla. Kullanıcının `themes/{ad}.toml`'u
    /// aynı adı gölgeler; o karar `bt-shell`'in ad çözümünde.
    pub fn embedded(name: &str) -> Option<Theme> {
        EMBEDDED
            .iter()
            .find(|(embedded, _)| *embedded == name)
            .map(|(_, theme)| *theme)
    }

    /// Pencerenin clear rengi, **lineer** RGBA.
    pub const fn background_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.background))
    }

    /// İmleç bloğunun rengi, **lineer** RGBA. Renderer'da sabit durmasın diye
    /// burada: renk kararı temanın, çizim kararı renderer'ın.
    pub const fn accent_linear(&self) -> LinearRgba {
        linear_rgba(rgb(self.accent))
    }

    /// Paletin `index` numaralı rengi. Numaralandırma alacritty'nin
    /// `term::color` tablosudur: 0..16 ANSI, 16..232 küp, 232..256 gri rampa,
    /// 256+ rol renkleri.
    ///
    /// Soğuk değil: taze bir oturumda `Colors` tablosu boştur (yalnız OSC
    /// 4/10/11 doldurur), yani `resolve` her hücrede buraya düşer.
    #[inline]
    pub(crate) fn default(&self, index: usize) -> Rgb {
        match index {
            0..=15 => rgb(self.ansi[index]), // audit: kol indeksi 0..16'ya bağlar
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
            256 => rgb(self.foreground),
            257 => rgb(self.background),
            258 => rgb(self.accent),
            // 259..=266 sönük ANSI sekizlisi, 267 parlak ön plan, 268 sönük ön
            // plan.
            259..=266 => dim_toward(rgb(self.ansi[index - 259]), self.background_rgb()),
            267 => rgb(self.foreground),
            268 => rgb(self.dim),
            // Tablo 269 girdilik; buraya düşen bir indeks alacritty'nin
            // değişmesi demektir. Renk yerine arka plan verip sessiz kalırız,
            // panik etmeyiz.
            _ => rgb(self.background),
        }
    }

    /// Varsayılan arka planın `Rgb` hâli; `frame()` karşılaştırmayı bununla
    /// yapar, f32 eşitliği aramaz.
    pub(crate) const fn background_rgb(&self) -> Rgb {
        rgb(self.background)
    }
}

/// Gömülü temaların tablosu; [`Theme::embedded`] okur.
const EMBEDDED: [(&str, Theme); 2] = [
    ("bateri", Theme::BATERI),
    ("bateri-light", Theme::BATERI_LIGHT),
];

/// Sönük (SGR 2) rengin **tek kuralı**: renk, zemine doğru üçte bir yol alır.
///
/// Neden zemine: sönüklük "zeminle arasındaki farkı azalt" demek. 006'ya
/// kadarki kural vte'nin `× 2/3`'üydü (`impl Mul<f32> for Rgb`, yorumu birebir
/// "the default dim is just *2/3") ve o siyaha doğru karıştırmanın ta
/// kendisi — açık zeminde sönük metni **koyulaştırıp** öne çıkarıyordu.
///
/// Oran bir tasarım sabiti, ölçüm değil. Üçte bir seçildi çünkü siyah zeminde
/// vte'nin çarpımıyla bit bit aynı sonucu veriyor (`dim_on_black_is_vte`):
/// alışılmış sönüklük koyu temada en az kayar, `BATERI`'nin zemini siyaha
/// yakın olduğu için değerler birkaç basamak açılır.
///
/// Uzay sRGB 8-bit, vte'ninkiyle aynı; lineerleştirme sınırda kalır
/// (`linear_rgba`). Lineer uzayda karıştırmak koyu zeminde algıda çok daha
/// az söndürürdü. Tam sayı bölmesi keser — vte'nin `as u8`'i de kesiyor.
///
/// Hedef **temanın** zemini, uygulamanın OSC 11 ile değiştirdiği değil: clear
/// rengi ve `frame()`'in atlama kararı da temadan okuyor.
///
/// `const`: `Theme::default` gibi palet yolunda ve tek bir tamsayı
/// aritmetiği.
const fn dim_toward(color: Rgb, background: Rgb) -> Rgb {
    Rgb {
        r: dim_channel(color.r, background.r),
        g: dim_channel(color.g, background.g),
        b: dim_channel(color.b, background.b),
    }
}

const fn dim_channel(color: u8, background: u8) -> u8 {
    // audit: en çok (2·255 + 255) / 3 = 255, `u8`'e sığar.
    ((2 * color as u16 + background as u16) / 3) as u8
}

/// 6×6×6 renk küpünün kanal basamakları (xterm sözleşmesi).
const CUBE: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// Hücrenin rengini ekranın rengine çevirir.
///
/// `colors` uygulamanın OSC 4/10/11 ile değiştirdiği tablodur ve girdileri
/// `None` olabilir; boş girdide temanın paletine düşülür.
///
/// `#[inline]`: `linear_rgba` ile aynı sıcak yol ve aynı gerekçe.
/// `Theme::default` ile **birlikte** işaretlenir; yalnız biri alınırsa çağrı
/// ötekine kayar.
#[inline]
pub(crate) fn resolve(color: Color, colors: &Colors, theme: &Theme) -> Rgb {
    let index = match color {
        Color::Spec(spec) => return spec,
        Color::Named(named) => named as usize,
        Color::Indexed(index) => index as usize,
    };
    // Tablo 269 girdilik; `NamedColor` en çok 268, `Indexed` en çok 255.
    colors[index].unwrap_or_else(|| theme.default(index)) // audit: indeks sınırlı
}

/// Hücrenin **`fg`'sinden doğan** rengi, sönüklük dahil çözer.
///
/// Sönüklüğün tek kuralı burada, çünkü iki yerde uygulanıyor: normal hücrede
/// ön plana, ters videoda arka plana (`Session::frame`). İki dala ayrı ayrı
/// yazılsaydı biri değişip öteki eski kalabilirdi.
///
/// Varsayılan ön plan sönükse çözümden **önce** temanın `dim` rolü alınır —
/// alacritty uygulamasının `DimForeground`'ı. Sonucu: OSC 10 ile değişmiş bir
/// ön plan sönük hücrede o rengin sönüğü değil, rolün kendisi olur (alacritty
/// de öyle). Öteki renkler çözülür ve zemine karışır ([`dim_toward`]).
///
/// `#[inline]`: `resolve` ile aynı sıcak yol.
#[inline]
pub(crate) fn resolve_fg(color: Color, dim: bool, colors: &Colors, theme: &Theme) -> Rgb {
    match color {
        Color::Named(NamedColor::Foreground) if dim => rgb(theme.dim),
        color if dim => dim_toward(resolve(color, colors, theme), theme.background_rgb()),
        color => resolve(color, colors, theme),
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
/// `srgb_table_follows_transfer_function` her girdiyi formüle bağlar.
/// Tablo olmasının sebebi `const`luk — `powf` stable'da `const` değil, oysa
/// [`Theme::background_linear`] ve [`Theme::accent_linear`] `const fn`.
// rustfmt tabloyu girdi başına bir satıra açıyor: 64 satır 256 olur ve
// dosyanın geri kalanı okunmaz hâle gelir. `Theme::BATERI`'nin ANSI
// tablosuyla aynı gerekçe.
#[rustfmt::skip]
const SRGB_LINEAR: [f32; 256] = [
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
/// bekçi `bt-gpu`'nun `cell_bg_paints_pixels_on_the_gpu` sınamasıdır ve
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
pub(crate) const fn linear_rgba(color: Rgb) -> LinearRgba {
    LinearRgba::from_srgb(color.r, color.g, color.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const THEME: Theme = Theme::BATERI;

    #[test]
    fn default_background_has_one_source() {
        // Hücrenin `Named(Background)`'ı, `frame()`'in atlama kararı ve
        // pencerenin clear rengi temanın **aynı** alanından gelmeli;
        // ayrılırlarsa boş hücreler pencereden farklı boyanır.
        let background = rgb(THEME.background);
        assert_eq!(THEME.default(NamedColor::Background as usize), background);
        assert_eq!(THEME.background_rgb(), background);
        assert_eq!(THEME.background_linear(), linear_rgba(background));
        // İmleç de rolünden: renk sorusunun yanıtı ile çizilen blok ayrışmasın.
        assert_eq!(
            THEME.default(NamedColor::Cursor as usize),
            rgb(THEME.accent)
        );
        assert_eq!(THEME.accent_linear(), linear_rgba(rgb(THEME.accent)));
    }

    #[test]
    fn bateri_palette_is_pinned() {
        // Paletin 19 değeri **elle yazılmış** bir listeye bağlı: tablodan
        // hesaplanan bir beklenti, tabloya düşen yazım hatasını kendisi de
        // taşırdı. Değerleri bilerek değiştiren bu listeyi de değiştirir.
        #[rustfmt::skip]
        const EXPECTED: [(usize, u32); 19] = [
            (0, 0x22252b), (1, 0xd16d6a), (2, 0x8bb58b), (3, 0xd6b16a),
            (4, 0x7a9cc6), (5, 0xb08ec0), (6, 0x79b3b3), (7, 0xc8c9cc),
            (8, 0x4a4e57), (9, 0xe58b88), (10, 0xa4cba4), (11, 0xe8c988),
            (12, 0x9bb8dc), (13, 0xc9aad8), (14, 0x96caca), (15, 0xe6e7ea),
            (256, 0xd8d9dd), // ön plan
            (257, 0x1a1c21), // arka plan
            (258, 0x7a9cc6), // imleç
        ];
        for (index, hex) in EXPECTED {
            assert_eq!(THEME.default(index), rgb(hex), "{index}");
        }
    }

    #[test]
    fn bateri_light_palette_is_pinned() {
        // `bateri_palette_is_pinned`'in açık tema karşılığı, aynı gerekçe;
        // sönük ön plan rolü de listede (268), çünkü o da elle seçildi.
        #[rustfmt::skip]
        const EXPECTED: [(usize, u32); 20] = [
            (0, 0x2b2e35), (1, 0xb5423d), (2, 0x3b7a3b), (3, 0x8f6a00),
            (4, 0x3a66a6), (5, 0x8a4c9c), (6, 0x23787f), (7, 0xb9bbc1),
            (8, 0x70737b), (9, 0xc9504a), (10, 0x4a8f4a), (11, 0xa67c00),
            (12, 0x4a78ba), (13, 0x9d5db0), (14, 0x2f8a92), (15, 0xdcdee3),
            (256, 0x24262c), // ön plan
            (257, 0xf5f6f8), // arka plan
            (258, 0x3d6aa8), // imleç
            (268, 0x696b70), // sönük ön plan
        ];
        let light = Theme::BATERI_LIGHT;
        for (index, hex) in EXPECTED {
            assert_eq!(light.default(index), rgb(hex), "{index}");
        }
        // Parlak beyaz zeminden ayrık: `\e[107m` görünür bir blok.
        assert_ne!(light.ansi[15], light.background);
        // `dim` rolü kuralın kendisinden: ön planın zemine karışmış hâli.
        assert_eq!(
            rgb(light.dim),
            dim_toward(rgb(light.foreground), light.background_rgb())
        );
    }

    #[test]
    fn embedded_themes_are_found_by_name() {
        assert_eq!(Theme::embedded("bateri"), Some(Theme::BATERI));
        assert_eq!(Theme::embedded("bateri-light"), Some(Theme::BATERI_LIGHT));
        assert_eq!(
            Theme::embedded("Bateri"),
            None,
            "ad büyük-küçük harfe duyarlı"
        );
        assert_eq!(Theme::embedded(""), None);
    }

    #[test]
    fn srgb_table_follows_transfer_function() {
        // Tablo elle yazılmış 256 sayı değil, formülün donmuş hâli. Referans
        // f64'te hesaplanır; tablo f32 olduğu için epsilon yalnız f32
        // yuvarlamasını karşılar (mutlak hata ≤ ~6e-8).
        assert_eq!(SRGB_LINEAR[0], 0.0);
        assert_eq!(SRGB_LINEAR[255], 1.0, "beyaz lineerde de 1.0 kalmalı");
        for (i, &linear) in SRGB_LINEAR.iter().enumerate() {
            let c = i as f64 / 255.0;
            let expected = if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
            assert!(
                (f64::from(linear) - expected).abs() < 1e-7,
                "{i}: {linear} != {expected}"
            );
            // Tablonun gerçekten **lineerleştirdiğinin** kanıtı, GPU
            // istemeden: lineer değer sRGB-kodlu hâlinden (`i/255`) kesin
            // olarak küçüktür. Tablo yerine `c / 255.0` konsaydı bu satır
            // düşerdi. Uçlar (0 ve 255) transfer fonksiyonunun sabit
            // noktaları, eşitlik oradan gelir ve aralığın dışında bırakılır.
            if (1..255).contains(&i) {
                assert!(f64::from(linear) < c, "{i}: {linear} !< {c}");
            }
        }
    }

    #[test]
    fn palette_indices_follow_xterm() {
        assert_eq!(THEME.default(1), rgb(THEME.ansi[1]));
        // 16 = küpün başı (0,0,0), 231 = sonu (255,255,255).
        assert_eq!(THEME.default(16), rgb(0x000000));
        assert_eq!(THEME.default(231), rgb(0xffffff));
        // Gri rampa 8'den başlar, 10'ar artar.
        assert_eq!(THEME.default(232), rgb(0x080808));
        assert_eq!(THEME.default(255), rgb(0xeeeeee));
    }

    #[test]
    fn dim_colors_move_toward_the_background() {
        // Kuralın asıl değişmezi: sönük renk, kaynağı ile zemin arasında
        // durur — kanal kanal. Koyu temada bu "koyulaşır", açıkta "açılır"
        // demek; iki zemin de sınanıyor ki kural yeniden siyaha doğru
        // çarpmaya dönerse açık tema düşsün.
        for theme in [Theme::BATERI, Theme::BATERI_LIGHT] {
            let bg = theme.background_rgb();
            for index in 259..=266 {
                let (source, dimmed) = (theme.default(index - 259), theme.default(index));
                for (s, d, b) in [
                    (source.r, dimmed.r, bg.r),
                    (source.g, dimmed.g, bg.g),
                    (source.b, dimmed.b, bg.b),
                ] {
                    assert!(
                        s.min(b) <= d && d <= s.max(b),
                        "{index}: {source:?} → {dimmed:?}"
                    );
                }
            }
        }
        let sum = |c: Rgb| u32::from(c.r) + u32::from(c.g) + u32::from(c.b);
        // Koyu temada kırmızının sönüğü kararır, açık temada açılır.
        assert!(sum(Theme::BATERI.default(260)) < sum(Theme::BATERI.default(1)));
        assert!(sum(Theme::BATERI_LIGHT.default(260)) > sum(Theme::BATERI_LIGHT.default(1)));
        // Elle yazılı: `(2·0xd1 + 0x1a) / 3`, `(2·0x6d + 0x1c) / 3`,
        // `(2·0x6a + 0x21) / 3`, tam sayı bölmesiyle.
        assert_eq!(THEME.default(260), rgb(0x945251));
    }

    #[test]
    fn dim_on_black_is_vte() {
        // Oranın gerekçesi (`dim_toward`'ın doc'u): siyah zeminde kural vte'nin
        // `× 2/3`'üyle bit bit aynı. Her kanal değeri sınanıyor; `f32`'nin
        // `2/3`'ü tam değerin biraz üstünde olduğu için kesme aynı yere düşüyor.
        let black = rgb(0x000000);
        for c in 0..=255u8 {
            let color = Rgb { r: c, g: c, b: c };
            assert_eq!(dim_toward(color, black), color * (2.0 / 3.0), "{c}");
        }
        // `BATERI`'nin `dim` rolü 006'nın hesabının donmuş hâli.
        assert_eq!(rgb(THEME.dim), dim_toward(rgb(THEME.foreground), black));
    }

    #[test]
    fn dim_default_foreground_takes_the_role() {
        // Rol çözümden **önce**: tabloda (OSC 10) ön plan değişmiş olsa da
        // sönük varsayılan ön plan temanın `dim`'i. Rol bilerek ön planın
        // `× 2/3`'ünden ayrık seçildi ki iki yol karışınca sınama görsün.
        let theme = Theme {
            dim: 0x123456,
            ..THEME
        };
        let mut colors = Colors::default();
        colors[NamedColor::Foreground] = Some(rgb(0xffffff));
        let foreground = Color::Named(NamedColor::Foreground);
        assert_eq!(resolve_fg(foreground, true, &colors, &theme), rgb(0x123456));
        // Sönük olmayan ön plan tabloyu okur.
        assert_eq!(
            resolve_fg(foreground, false, &colors, &theme),
            rgb(0xffffff)
        );
        // Adlı renk çözülüp zemine karışır; rol ona dokunmaz.
        let red = Color::Named(NamedColor::Red);
        assert_eq!(
            resolve_fg(red, true, &colors, &theme),
            dim_toward(rgb(THEME.ansi[1]), THEME.background_rgb())
        );
    }

    #[test]
    fn osc_table_overrides_palette() {
        let mut colors = Colors::default();
        let custom = rgb(0x010203);
        colors[1] = Some(custom);
        let red = Color::Named(NamedColor::Red);
        assert_eq!(resolve(red, &colors, &THEME), custom);
        assert_eq!(resolve(Color::Indexed(1), &colors, &THEME), custom);
        // Doğrudan verilen renk tabloya hiç sormaz.
        let green = rgb(THEME.ansi[2]);
        assert_eq!(resolve(Color::Spec(green), &colors, &THEME), green);
        // Tabloda olmayan girdi temadan gelir.
        assert_eq!(resolve(Color::Indexed(2), &colors, &THEME), green);
    }
}
