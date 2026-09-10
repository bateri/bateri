//! bt-atlas — glyph rasterizasyonu ve atlas paketleme.
//!
//! CoreText ile rasterizasyon, sabit yuva ızgarası ve hücre metriği burada
//! yaşar. Sözleşme: yalnız `objc2-core-text` / `objc2-core-graphics` (ve
//! ikisinin ortak tabanı `objc2-core-foundation`) görülür; AppKit ve **Metal
//! görülmez**. Dokunun sahibi `bt-gpu`'dur — buradan çıkan şey bir yuva
//! numarası ve CPU bitmap'idir, `MTLTexture` değil; `bt-gpu` onu
//! `replaceRegion` ile kendi `R8Unorm` dokusuna yazıyor.
//!
//! Kutu çizim karakterleri, emoji ve font seti ayarı kapsam dışı (003 →
//! `plan.md` → Kapsam Dışı); ikinci font yüzü isteyen `BOLD`/`ITALIC` 004'ün.

mod font;
mod raster;

use std::collections::HashMap;

use objc2_core_foundation::CFRetained;
use objc2_core_text::CTFont;

pub use font::Metrics;
use raster::Cizim;

/// Yuva 0 **rezident tofu**: dolu atlasta ve `.notdef`'te buraya düşülür.
///
/// Sessiz kayıp (glyph hiç çizilmez) yerine görünür kayıp (kutu çizilir):
/// eksik font ekranda kendini gösterir, log'da beklemez. İçeriğini `bt-gpu`
/// doku kurulumunda bir kez yazar ([`Atlas::tofu_bitmap`]) ve bir daha
/// dokunmaz — "rezident" tam olarak bu demek.
pub const TOFU: u16 = 0;

/// Atlas dokusunun hedeflenen kenarı, piksel.
///
/// **Ölçüm iddiası değil**, bir kapasite tercihi: yuva sayısı hücre
/// ölçüsünden türüyor ve gerçek doluluk [`Atlas::occupancy`] sayacından okunur
/// (`/measure`). Tahliye yok — dolan atlas tofu'ya düşer, LRU 00X'in işi.
const DOKU_KENARI: u16 = 1024;

/// `point_size * scale` çarpımının kabul aralığı.
///
/// Üst sınır keyfi değil: `u16` metriğin sonuna kadar giden bir punto yuva
/// başına gigabaytlık tampon ister ve `texture_px()` Metal'in doku sınırını
/// katbekat aşar. Alt sınır okunmayan puntoları keser. Ayar dosyası geldiğinde
/// (00X) doğrulama orada da yapılır ama sınırın **burada** olması şart:
/// `Atlas` kendi değişmezini çağıranın disiplinine bırakmıyor.
const PUNTO_EN_AZ: f64 = 4.0;
const PUNTO_EN_COK: f64 = 144.0;

/// Dokuya yazılacak tek yuva: **nereye** ve **ne**.
///
/// İkisi aynı dönüşte geliyor çünkü `replaceRegion` ikisine birden ihtiyaç
/// duyuyor. Ayrı olsalardı (`slot` + ayrıca `slot_origin`) çağıran `&mut`
/// ödüncü elindeyken `&self` istemek zorunda kalır ve yükleme döngüsü
/// derlenmezdi — sınır ödünç kuralının yanlış tarafından geçerdi.
pub struct Upload<'a> {
    /// Yuvanın doku içindeki sol üst köşesi, piksel.
    pub origin: (u16, u16),
    /// Tam bir yuva dolusu `R8` kapsama verisi ([`Metrics::slot_bytes`]).
    pub bytes: &'a [u8],
}

/// Sabit yuva ızgarasında yaşayan glyph atlası.
///
/// Paketleyici yok: bu sette **tüm glyph'ler hücre boyutunda** (emoji ve kutu
/// çizim kapsam dışı), yani `yuva_no → piksel köşe` dönüşümü aritmetiktir.
pub struct Atlas {
    font: CFRetained<CTFont>,
    metrics: Metrics,
    /// Kurulduğu (punto, ölçek). [`Atlas::ensure`]'nin ölçütü.
    anahtar: (f64, f64),
    /// Izgaranın (sütun, satır) yuva sayısı.
    izgara: (u16, u16),
    /// Karakterin **çözümlendiği** yuva — yalnız yüklenenler değil: fontun
    /// tanımadığı karakter de burada [`TOFU`] olarak yaşıyor, yoksa aynı
    /// karakter her karede CoreText'e yeniden sorulurdu.
    yuvalar: HashMap<char, u16>,
    /// Bir sonraki boş yuva; [`TOFU`] ayrılmış olduğu için 1'den başlar.
    /// `yuvalar.len()`'den türetilemez: tofu'ya çözümlenen kayıtlar yuva
    /// harcamıyor, yani iki sayı bilerek ayrışıyor.
    sonraki: u16,
    /// Tek yuvalık çizim tamponu. Alan olması kare başına yeniden ayırmayı
    /// önlüyor; içeriği her yeni glyph'te üzerine yazılır.
    tampon: Vec<u8>,
    /// Rezident tofu kutusu; ömür boyu değişmez.
    tofu: Vec<u8>,
}

impl Atlas {
    /// `point_size` mantıksal punto, `scale` ekranın backing ölçeği.
    ///
    /// İkisi **çarpılıp** fonta girer: metrik ve raster aynı fiziksel piksel
    /// uzayında doğar, yani ölçek önbellek anahtarının parçasıdır. Anahtarın
    /// değişmesi hâlinde yapılacak şeyi [`Atlas::ensure`] biliyor.
    pub fn new(point_size: f64, scale: f64) -> Self {
        let font = font::zincirden_ac(etkin_punto(point_size, scale));
        let metrics = font::metrics(&font);
        let (w, h) = metrics.cell_px;
        // `w`/`h` en az 1 (`font::yukari`), yani bölme güvenli; `max(1)` de
        // hücrenin dokudan büyük olduğu uç için.
        let izgara = ((DOKU_KENARI / w).max(1), (DOKU_KENARI / h).max(1));
        Self {
            font,
            metrics,
            anahtar: (point_size, scale),
            izgara,
            yuvalar: HashMap::new(),
            sonraki: TOFU + 1,
            tampon: vec![0u8; metrics.slot_bytes()],
            tofu: tofu_tamponu(metrics),
        }
    }

    /// Anahtar ([`Atlas::new`]'in punto/ölçek çifti) değiştiyse atlası
    /// yeniden kurar ve `true` döner.
    ///
    /// `true` aynı zamanda **"dokuyu yeniden ayır"** demektir: metrik ve
    /// dolayısıyla [`Atlas::texture_px`] değişmiş olabilir, eski boyutlu
    /// dokuya yeni metrikle yazmak sessizce bozar. Ölçek değişimini AppKit
    /// haber veriyor (`windowDidChangeBackingProperties:`); bu metot o
    /// kancanın karşılığı ve yeniden kurma kararını çağıranın hatırlamasına
    /// bırakmıyor. Karşılaştırma **tam eşitlik**: ölçek ve punto ayrık
    /// değerler arasında sıçrıyor, aralarında yorumlanacak bir yakınlık yok.
    #[must_use = "true ise atlas yeniden kuruldu: yuva eşlemesi ve doku boyutu değişmiş olabilir, doku da yeniden ayrılmalı"]
    pub fn ensure(&mut self, point_size: f64, scale: f64) -> bool {
        if (point_size, scale) == self.anahtar {
            return false;
        }
        *self = Self::new(point_size, scale);
        true
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    /// Atlas dokusunun piksel boyutu; `bt-gpu` dokuyu buna göre ayırır.
    ///
    /// [`DOKU_KENARI`] değil **tam ızgara**: kenardaki artık şerit hiçbir
    /// yuvaya düşmez, ayırmanın da anlamı yok.
    pub fn texture_px(&self) -> (u16, u16) {
        let (w, h) = self.metrics.cell_px;
        (self.izgara.0 * w, self.izgara.1 * h)
    }

    /// Yuvanın doku içindeki sol üst köşesi, piksel. uv aritmetiği çağıranın.
    pub fn slot_origin(&self, slot: u16) -> (u16, u16) {
        // Izgara dışı yuva [`TOFU`]'ya düşer. Bu bir savunma refleksi değil,
        // gerçek bir yol: `ensure()` ızgarayı küçültebiliyor ve çağıranın
        // elinde bir önceki ölçekten kalma yuva numarası olabilir. `debug_assert`
        // yetmezdi — release'de dokunun dışını gösteren bir köşe döner,
        // `replaceRegion` sınır dışına yazar ve belirti sessizdir.
        let slot = if slot < self.kapasite() { slot } else { TOFU };
        let (w, h) = self.metrics.cell_px;
        ((slot % self.izgara.0) * w, (slot / self.izgara.0) * h)
    }

    /// Yuva [`TOFU`]'nun kalıcı içeriği; `bt-gpu` doku kurulumunda bir kez
    /// yazar. [`Atlas::slot`] tofu'ya düştüğünde bitmap **vermez**: veri
    /// zaten dokuda ve her düşüşte yeniden yüklemek boşa yazma olurdu.
    pub fn tofu_bitmap(&self) -> &[u8] {
        &self.tofu
    }

    /// Karakterin yuvası.
    ///
    /// İkinci değer yuva **yeni açıldıysa** dolu gelir; yüklü yuvada ve tofu
    /// düşüşünde `None`'dır ve doku el değmeden kalır.
    pub fn slot(&mut self, ch: char) -> (u16, Option<Upload<'_>>) {
        if let Some(&yuva) = self.yuvalar.get(&ch) {
            return (yuva, None);
        }
        if self.sonraki >= self.kapasite() {
            // Dolu atlas **önbelleklenmez**: bu, fontun kalıcı bir gerçeği
            // değil atlasın geçici hâli. Tahliye geldiğinde (00X) buraya
            // yazılacak kayıt yanlış olurdu — yer açılmış ama karakter hâlâ
            // tofu'ya bağlı kalırdı.
            return (TOFU, None);
        }
        // Ödünç match'in scrutinee'sinde bırakılmıyor: `&mut self.tampon`
        // orada kalsaydı kolların içinde `&self.tampon` alınamazdı.
        let cizim = raster::ciz(&self.font, ch, self.metrics, &mut self.tampon);
        match cizim {
            Cizim::Cizildi => {
                let yuva = self.sonraki;
                self.sonraki += 1;
                self.yuvalar.insert(ch, yuva);
                let origin = self.slot_origin(yuva);
                (
                    yuva,
                    Some(Upload {
                        origin,
                        bytes: &self.tampon,
                    }),
                )
            }
            // İkisi de **kalıcı**: fontun o karakteri yoktur, ya da bağlam
            // kurulumu (argümanları atlas ömrü boyunca sabit) hep başarısızdır.
            // Önbelleğe girmeselerdi aynı karakter ekranda durduğu sürece her
            // karede yeniden CoreText'e sorulurdu.
            Cizim::GlifYok | Cizim::BaglamYok => {
                // Tavan: negatif önbellek yuva harcamıyor, yani `sonraki` onu
                // sınırlamıyor. Bir ikili dosyayı `cat`'lemek milyonlarca ayrı
                // codepoint üretebilir ve harita sessizce büyürdü — crate'in
                // tavanı olmayan tek sayısı burasıydı.
                //
                // Tavan dolunca **negatif kayıtlar toptan atılıyor**, "artık
                // hiç önbellekleme" değil. Fark bu sette ortaya çıktı:
                // `slot()` artık çizim yolunda (`bt-gpu` onu display link
                // callback'inde çağırıyor), yani önbelleklenmeyen bir
                // karakter ekranda durduğu sürece **her kare** CoreText'e
                // geri sorulurdu — ana thread'de, kare bütçesinin ortasında.
                // Tahliye bedeli amortize: iki tahliye arasına en az
                // `kapasite()` yeni kayıt sığıyor. Pozitif kayıtlar (gerçek
                // yuvalar) korunuyor; onların tahliyesi LRU'nun işi (00X).
                if self.yuvalar.len() >= self.negatif_tavan() {
                    self.yuvalar.retain(|_, &mut yuva| yuva != TOFU);
                }
                self.yuvalar.insert(ch, TOFU);
                (TOFU, None)
            }
        }
    }

    /// (kullanılan, toplam) yuva.
    ///
    /// Tofu kullanılan sayılır: doku o yuvayı da tutuyor ve doluluk oranı
    /// `/measure`'da bu iki sayıdan okunacak.
    pub fn occupancy(&self) -> (usize, usize) {
        (usize::from(self.sonraki), usize::from(self.kapasite()))
    }

    /// Haritanın kabul ettiği en çok kayıt sayısı — pozitif ve negatif
    /// birlikte. Kapasitenin **iki katı**: bir katı pozitif kayıtların
    /// olabildiği en büyük değer, ikincisi negatif önbelleğe bırakılan pay.
    fn negatif_tavan(&self) -> usize {
        usize::from(self.kapasite()).saturating_mul(2)
    }

    /// Toplam yuva sayısı.
    ///
    /// `u16`'ya kırpılıyor: yuva numarası dışarıya `u16` olarak veriliyor ve
    /// çok küçük hücrelerde ızgara o sınırı aşabilir. Kırpma kapasiteyi
    /// daraltır, taşma ise yuvaları sessizce birbirine bindirirdi.
    fn kapasite(&self) -> u16 {
        let toplam = u32::from(self.izgara.0) * u32::from(self.izgara.1);
        u16::try_from(toplam).unwrap_or(u16::MAX)
    }
}

/// Fonta girecek punto: ölçek çarpılmış ve aralığa oturtulmuş.
///
/// NaN ayrıca ele alınıyor çünkü `clamp` onu **geçirir**; taban puntoya düşmek
/// hem çökmekten hem sessizce bozulmaktan iyi — sonuç görünür şekilde yanlış
/// olur ve fark edilir.
fn etkin_punto(point_size: f64, scale: f64) -> f64 {
    let v = point_size * scale;
    if v.is_finite() {
        v.clamp(PUNTO_EN_AZ, PUNTO_EN_COK)
    } else {
        PUNTO_EN_AZ
    }
}

/// Tofu kutusunu çizer: hücre kenarından bir piksel içeride, 1 px çerçeve.
///
/// Fontun `.notdef` glyph'i **kullanılmıyor**: bazı fontlarda boş, bazılarında
/// kutu ve hangisi olduğu font sürümüne bağlı. Çerçeveyi kendimiz çizmek
/// tofu'yu fonttan bağımsız kılıyor — "görünür kayıp" iddiası ancak böyle
/// tutuyor.
fn tofu_tamponu(m: Metrics) -> Vec<u8> {
    let (w, h) = (usize::from(m.cell_px.0), usize::from(m.cell_px.1));
    let mut hedef = vec![0u8; m.slot_bytes()];
    let (x0, x1) = (1usize, w.saturating_sub(2));
    let (y0, y1) = (1usize, h.saturating_sub(2));
    if x1 <= x0 || y1 <= y0 {
        // Hücre çerçeveye dar; boş yuva kutudan iyidir.
        return hedef;
    }
    // audit: `x1 < w` ve `y1 < h` (ikisi de `saturating_sub(2)`), yani en
    // büyük indeks `y1 * w + x1 < w * h` — dilim sınırı içinde.
    for x in x0..=x1 {
        hedef[y0 * w + x] = 0xff;
        hedef[y1 * w + x] = 0xff;
    }
    for y in y0..=y1 {
        hedef[y * w + x0] = 0xff;
        hedef[y * w + x1] = 0xff;
    }
    hedef
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sınama puntosu bilerek büyük: ızgara hücre ölçüsünden türüyor, yani
    /// büyük punto = az yuva. "Dolu atlas" sınaması böylece binlerce glyph
    /// rasterize etmeden, onlarcasıyla koşuyor. Değer [`PUNTO_EN_COK`]'tur:
    /// üstünü istemek sessizce kırpılır ve sınama kapasiteyi yanlış sanırdı.
    const BUYUK_PUNTO: f64 = PUNTO_EN_COK;
    const PUNTO: f64 = 13.0;
    /// Menlo ve SF Mono CJK içermez ve `CTFontGetGlyphsForCharacters` başka
    /// fonta düşmez: bu karakter `.notdef` verir.
    const TANINMAZ: char = '漢';

    #[test]
    fn metrik_makul_araliktadir() {
        let m = Atlas::new(PUNTO, 1.0).metrics();
        assert!(m.cell_px.0 > 0, "genişlik sıfır: {m:?}");
        assert!(m.cell_px.1 > m.cell_px.0, "monospace hücre uzundur: {m:?}");
        assert!(m.baseline_px > 0, "taban çizgisi sıfır: {m:?}");
        assert!(m.baseline_px <= m.cell_px.1, "taban hücrenin içinde: {m:?}");
        assert_eq!(
            m.slot_bytes(),
            usize::from(m.cell_px.0) * usize::from(m.cell_px.1)
        );
    }

    #[test]
    fn ayni_karakter_ayni_yuvayi_alir() {
        let mut a = Atlas::new(PUNTO, 1.0);
        let (ilk, yukleme) = a.slot('A');
        assert_ne!(ilk, TOFU, "tanınan karakter tofu'ya düşmemeli");
        assert!(yukleme.is_some(), "ilk soruluşta yükleme gelmeli");
        let (ikinci, tekrar) = a.slot('A');
        assert_eq!(ilk, ikinci);
        assert!(
            tekrar.is_none(),
            "yuva zaten yüklü: doku el değmeden kalmalı"
        );
    }

    #[test]
    fn yukleme_yuvasinin_kosesini_birlikte_verir() {
        // Bu bekçinin asıl işi derlenmek: köşe ile baytlar ayrı çağrılardan
        // gelseydi `bt-gpu`'nun yükleme döngüsü `&mut` ödüncü elindeyken
        // `&self` istemek zorunda kalır ve derlenmezdi.
        let mut a = Atlas::new(PUNTO, 1.0);
        let yuva_bayt = a.metrics().slot_bytes();
        // `bt-gpu`'nun yükleme döngüsünün şekli: yükleme kendi bloğunda
        // tüketilir, sonra aynı atlas uv için yeniden okunur. Köşe
        // `Upload`'nin içinde olmasaydı o blokta `&self` istemek gerekirdi
        // ve `slot`'un `&mut` ödüncü yüzünden derlenmezdi.
        let (yuva, yukleme) = a.slot('A');
        let mut yazilan = None;
        if let Some(y) = yukleme {
            assert_eq!(y.bytes.len(), yuva_bayt);
            yazilan = Some(y.origin);
        }
        assert_eq!(yazilan, Some(a.slot_origin(yuva)));
    }

    #[test]
    fn rasterize_edilen_glif_bos_degildir() {
        // Bu bekçi olmadan "her şey çalışıyor ama atlas bomboş" durumu sessiz
        // kalır: yuva numaraları doğru, doku doğru boyutta, ekran boş.
        let mut a = Atlas::new(PUNTO, 1.0);
        let (_, yukleme) = a.slot('W');
        let bytes = yukleme.expect("ilk soruluşta yükleme gelmeli").bytes;
        assert!(bytes.iter().any(|&b| b > 0), "'W' hiç piksel boyamadı");
        // Boşluk da tanınan bir glyph'tir ama hiçbir şey boyamaz: ölçüt
        // "bitmap doldu mu" değil, "raster çalıştı mı".
        let (yuva, bos) = a.slot(' ');
        assert_ne!(yuva, TOFU);
        assert!(
            bos.expect("yeni yuva").bytes.iter().all(|&b| b == 0),
            "boşluk boyamamalı"
        );
    }

    #[test]
    fn tofu_kutusu_cizilmis_ve_rezidenttir() {
        let mut a = Atlas::new(PUNTO, 1.0);
        assert_eq!(a.tofu_bitmap().len(), a.metrics().slot_bytes());
        assert!(
            a.tofu_bitmap().iter().any(|&b| b > 0),
            "tofu boş kutu olamaz"
        );
        // Tofu'ya düşen çağrı yükleme **vermez**: veri dokuda zaten.
        assert!(
            a.slot(TANINMAZ).1.is_none(),
            "rezident yuva yeniden yüklenmez"
        );
    }

    #[test]
    fn taninmayan_karakter_onbelleklenir() {
        let mut a = Atlas::new(PUNTO, 1.0);
        assert_eq!(a.slot(TANINMAZ).0, TOFU);
        // Fontun bu karakteri tanımaması kalıcı: ikinci soruluşta CoreText'e
        // gidilmemeli. Bekçi iç tabloya bakıyor çünkü FFI çağrısının olup
        // olmadığı dışarıdan gözlenemiyor.
        assert_eq!(
            a.yuvalar.get(&TANINMAZ),
            Some(&TOFU),
            "tofu çözümü önbelleğe girmeli"
        );
        assert_eq!(a.occupancy().0, 1, "tofu düşüşü yuva harcamamalı");
    }

    #[test]
    fn dolu_atlas_tofu_verir_ama_onbelleklemez() {
        let mut a = Atlas::new(BUYUK_PUNTO, 1.0);
        let (kullanilan, toplam) = a.occupancy();
        assert_eq!(kullanilan, 1, "yeni atlasta yalnız tofu ayrılmış olmalı");
        // Yazdırılabilir ASCII'nin tamamı: havuz kapasiteden büyük olmalı ve
        // harf/rakam (62) `PUNTO_EN_COK`'taki kapasiteye yetmiyor.
        let havuz: Vec<char> = (' '..='~').collect();
        assert!(
            havuz.len() > toplam,
            "sınama havuzu kapasiteyi aşmalı: havuz={} kapasite={toplam}",
            havuz.len()
        );
        let dusenler: Vec<char> = havuz
            .iter()
            .copied()
            .filter(|&ch| a.slot(ch).0 == TOFU)
            .collect();
        assert!(!dusenler.is_empty(), "kapasite aşılınca tofu beklenir");
        assert_eq!(
            a.occupancy(),
            (toplam, toplam),
            "ızgara sonuna kadar dolmalı"
        );
        // Dolu atlas geçici bir hâl: tahliye gelince (00X) yer açılacak ve
        // bu karakterlerin tofu'ya bağlı kalmaması gerekiyor.
        for ch in dusenler {
            assert!(
                !a.yuvalar.contains_key(&ch),
                "'{ch}' kalıcı olarak tofu'ya yazılmış"
            );
        }
    }

    #[test]
    fn glif_taban_cizgisinin_ustune_oturur() {
        // Bu bekçi olmadan y ekseni ters çevrilse (CG'nin başlangıcı sol
        // **alt**) ya da taban yanlış hesaplansa bütün sınamalar yeşil kalır:
        // `rasterize_edilen_glif_bos_degildir` yalnız "bir yerde piksel var"
        // diyor. `bt-gpu`'nun offscreen kapısı da göremezdi: o da "hücrenin
        // içi arka planla tekdüze değil" diyor, harfin doğru yerde olduğunu
        // değil. Ters bir taban ancak gözle görülürdü.
        let mut a = Atlas::new(PUNTO, 1.0);
        let m = a.metrics();
        let (_, yukleme) = a.slot('W');
        let bytes = yukleme.expect("yeni yuva").bytes;
        let w = usize::from(m.cell_px.0);
        let dolu = |satir: usize| bytes[satir * w..(satir + 1) * w].iter().any(|&b| b > 0);
        let taban = usize::from(m.baseline_px);
        // 'W' ne descender taşır ne aksan: kapsamanın tamamı tabanın üstünde.
        assert!((0..taban).any(dolu), "taban çizgisinin üstü boş: {m:?}");
        assert!(
            !(taban..usize::from(m.cell_px.1)).any(dolu),
            "'W' taban çizgisinin altına taşmamalı: {m:?}"
        );
    }

    #[test]
    fn descender_hucreye_sigar() {
        // Taban çizgisi ile yükseklik **ayrı ayrı** yuvarlanmasaydı
        // (`yukari(ascent + descent + leading)` tek seferde) alta fontun
        // descent'inden az yer kalırdı ve 'g' gibi harflerin son kapsama
        // satırı kırpılırdı. Kırpılan glyph hücrenin son satırını doldurur;
        // sığan glyph orayı boş bırakır — ölçüt bu. 'W' ile sınamak yetmez:
        // descender'ı olmayan harf iki yuvarlamada da aynı görünür.
        let mut a = Atlas::new(PUNTO, 1.0);
        let m = a.metrics();
        let (_, yukleme) = a.slot('g');
        let bytes = yukleme.expect("yeni yuva").bytes;
        let w = usize::from(m.cell_px.0);
        let dolu = |satir: usize| bytes[satir * w..(satir + 1) * w].iter().any(|&b| b > 0);
        let taban = usize::from(m.baseline_px);
        assert!(dolu(taban), "'g' taban çizgisinin altına inmeli: {m:?}");
        assert!(
            !dolu(usize::from(m.cell_px.1) - 1),
            "descender hücrenin son satırında kırpılmış: {m:?}"
        );
    }

    #[test]
    fn negatif_onbellek_tavanli_ve_tahliyeli() {
        let mut a = Atlas::new(BUYUK_PUNTO, 1.0);
        let tavan = a.negatif_tavan();
        // Tanınan bir karakter önce yuvasını alsın: tahliyenin **yalnız**
        // negatif kayıtları attığını sınamak için bir pozitif kayıt gerek.
        let (harf, _) = a.slot('A');
        assert_ne!(harf, TOFU, "'A' Menlo'da var");

        // Tanınmayan karakter yuva harcamıyor, yani `sonraki` onu
        // sınırlamıyor. Tavan olmasaydı harita gördüğü ayrı codepoint sayısı
        // kadar büyürdü ve bir ikili dosyayı `cat`'lemek bunu gerçek bir yola
        // çevirir. Crate'in tavanı olmayan tek sayısı burasıydı.
        let havuz: Vec<char> = ('\u{4e00}'..'\u{9fff}').take(tavan * 3).collect();
        assert!(havuz.len() > tavan, "havuz tavanı aşmalı");
        for &ch in &havuz {
            assert_eq!(a.slot(ch).0, TOFU, "'{ch}' Menlo/SF Mono'da yok");
            assert!(
                a.yuvalar.len() <= tavan,
                "negatif önbellek tavanı aşıldı: {} > {tavan}",
                a.yuvalar.len()
            );
        }
        assert_eq!(a.occupancy().0, 2, "tofu düşüşleri yuva harcamamalı");

        // Tavan dolunca önbellekleme **durmuyor**, tahliye oluyor: tahliyeden
        // sonra gelen kayıt haritaya giriyor. Eski davranışta ("tavan dolu →
        // hiç yazma") burası boş dönerdi ve ekranda duran her desteklenmeyen
        // karakter her karede CoreText'e geri sorulurdu — `slot()` bu sette
        // çizim yoluna girdiği için bedeli ana thread'de ödenirdi.
        let son = *havuz.last().expect("havuz boş değil");
        assert_eq!(
            a.yuvalar.get(&son),
            Some(&TOFU),
            "tahliyeden sonraki kayıt önbelleğe girmeli"
        );
        // Pozitif kayıt tahliyeye girmiyor: yuvası duruyor.
        assert_eq!(a.slot('A').0, harf, "pozitif kayıt tahliyede kayboldu");
    }

    #[test]
    fn bmp_disi_karakter_yolu_calisir() {
        // Surrogate çifti: `encode_utf16` iki birim üretiyor, CoreText ikinci
        // birime de dokunuyor ve glyph üretmeyip `false` dönüyor. `font::glif`
        // o dönüşü bilerek yok sayıyor ve işaretçilerini dilimden türetiyor;
        // ikisinin gerekçesi de ancak bu yol koşarsa sınanmış olur.
        let mut a = Atlas::new(PUNTO, 1.0);
        assert_eq!(
            a.slot('𝔸').0,
            TOFU,
            "Menlo/SF Mono matematik alfabesi içermez"
        );
    }

    #[test]
    fn bozuk_punto_atlasi_dusurmez() {
        // `NaN as u16` sıfırdır ve `clamp` NaN'ı geçirir: sınır konmasaydı
        // ızgara sıfıra bölerdi. Devasa punto ise yuva başına gigabaytlık
        // tampon isterdi.
        for (punto, scale) in [(f64::NAN, 1.0), (13.0, f64::INFINITY), (1e9, 1.0)] {
            let a = Atlas::new(punto, scale);
            let m = a.metrics();
            assert!(m.cell_px.0 > 0 && m.cell_px.1 > 0, "{punto}×{scale}: {m:?}");
            let (tw, th) = a.texture_px();
            assert!(
                tw <= DOKU_KENARI && th <= DOKU_KENARI,
                "{punto}×{scale}: {tw}×{th}"
            );
        }
    }

    #[test]
    fn olcek_anahtarin_parcasidir() {
        let bir = Atlas::new(PUNTO, 1.0).metrics();
        let iki = Atlas::new(PUNTO, 2.0).metrics();
        assert_ne!(bir.cell_px, iki.cell_px, "@2x hücre @1x ile aynı olamaz");
        // Tam iki kat beklenmiyor: her ölçü ayrı ayrı yukarı yuvarlanıyor.
        assert!(
            iki.cell_px.0 + 2 >= bir.cell_px.0 * 2 && iki.cell_px.0 <= bir.cell_px.0 * 2 + 2,
            "@2x genişlik iki katına yakın olmalı: {bir:?} → {iki:?}"
        );
    }

    #[test]
    fn ensure_yalniz_anahtar_degisince_kurar() {
        let mut a = Atlas::new(PUNTO, 1.0);
        a.slot('A');
        assert!(!a.ensure(PUNTO, 1.0), "aynı anahtar yeniden kurmamalı");
        assert_eq!(a.occupancy().0, 2, "yuvalar korunmalı");
        assert!(a.ensure(PUNTO, 2.0), "ölçek değişti: yeniden kurulmalı");
        assert_eq!(a.occupancy().0, 1, "yeni atlasta yalnız tofu");
        assert_eq!(a.metrics(), Atlas::new(PUNTO, 2.0).metrics());
    }

    #[test]
    fn bulunamayan_aile_ikame_edilir() {
        // CoreText hata vermez, en yakın fontu verir: "font açıldı" bir kanıt
        // değildir ve zincir bu yüzden dönen adı karşılaştırıyor.
        const YOK: &str = "Bu Aile Yok 12345";
        let (_, donen) = font::ac(YOK, PUNTO);
        assert_ne!(donen, YOK, "var olmayan aile için ikame beklenir");
    }

    #[test]
    fn yuva_kokeni_izgarada_yurur() {
        let a = Atlas::new(PUNTO, 1.0);
        let (w, h) = a.metrics().cell_px;
        let sutun = a.izgara.0;
        assert_eq!(a.slot_origin(TOFU), (0, 0));
        assert_eq!(a.slot_origin(1), (w, 0));
        assert_eq!(
            a.slot_origin(sutun),
            (0, h),
            "ilk yuva bir alt satıra düşer"
        );
        // Doku ızgarayı sarmalı ve kenarda bir hücreden fazlası boşa gitmemeli.
        let (tw, th) = a.texture_px();
        assert!(tw <= DOKU_KENARI && tw + w > DOKU_KENARI, "genişlik: {tw}");
        assert!(th <= DOKU_KENARI && th + h > DOKU_KENARI, "yükseklik: {th}");
    }
}
