//! bt-atlas — glyph rasterizasyonu ve atlas paketleme.
//!
//! CoreText ile rasterizasyon, sabit yuva ızgarası ve hücre metriği burada
//! yaşar. Sözleşme: yalnız `objc2-core-text` / `objc2-core-graphics` (ve
//! ikisinin ortak tabanı `objc2-core-foundation`) görülür; AppKit ve **Metal
//! görülmez**. Dokunun sahibi `bt-gpu`'dur — buradan çıkan şey bir yuva
//! numarası ve CPU bitmap'idir, `MTLTexture` değil; `bt-gpu` onu
//! `replaceRegion` ile kendi `R8Unorm` dokusuna yazıyor.
//!
//! Dört font yüzü (`Face`) ve kural çizgileri (`RuleKind`) burada: kural
//! sprite'ları fonttan glyph almıyor, yordamsal çiziliyor. Aile ayardan
//! gelir (007 phase-5) ve makinede yoksa zincire düşülür; bunu söyleyen
//! [`FontIssue`] çağırana döner, bu crate kimseye bir şey basmaz. Kutu çizim
//! karakterleri, emoji ve yedek font listesi (bir glyph'i başka fonttan
//! almak) hâlâ kapsam dışı ve ayrı setlere kaldı.

mod font;
mod raster;

use std::collections::HashMap;

use font::Faces;
pub use font::{Face, FontIssue, Metrics, SizeClass};
use objc2_core_foundation::CFRetained;
use objc2_core_text::CTFont;
use raster::DrawResult;
pub use raster::RuleKind;

/// Atlasta yuva tutan şey: bir karakter ya da bir kural çizgisi.
///
/// İkisi aynı ızgarada yaşıyor çünkü ikisi de **hücre boyunda bir kapsama
/// maskesi**; `bt-gpu` ikisini de aynı `cell` pipeline'ından çiziyor ve rengi
/// instance'tan veriyor. Emoji bu birliği bozar (iki hücre, renkli doku) ve
/// tam bu yüzden ayrı bir sete bırakıldı.
// `repr(u8)`: bkz. `RuleKind`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sprite {
    Char(char),
    Rule(RuleKind),
}

/// Yuva 0 **rezident tofu**: dolu atlasta ve `.notdef`'te buraya düşülür.
///
/// Sessiz kayıp (glyph hiç çizilmez) yerine görünür kayıp (kutu çizilir):
/// eksik font ekranda kendini gösterir, log'da beklemez. İçeriğini `bt-gpu`
/// doku kurulumunda bir kez yazar ([`Atlas::tofu_bitmap`]) ve bir daha
/// dokunmaz — "rezident" tam olarak bu demek.
pub const TOFU: u16 = 0;

/// Kural sprite'larına ayrılan yuva payı — [`RuleKind`]'ın varyant sayısı.
///
/// Kapasitenin bu kadarı karakterlere kapalı. Bkz. [`Atlas::slot`].
const RULE_RESERVE: u16 = 7;

/// Atlas dokusunun hedeflenen kenarı, piksel.
///
/// **Ölçüm iddiası değil**, bir kapasite tercihi: yuva sayısı hücre
/// ölçüsünden türüyor ve gerçek doluluk [`Atlas::occupancy`] sayacından okunur
/// (`/measure`). Tahliye yok — dolan atlas tofu'ya düşer, LRU 00X'in işi.
const TEXTURE_EDGE: u16 = 1024;

/// `point_size * scale` çarpımının kabul aralığı.
///
/// Üst sınır keyfi değil: `u16` metriğin sonuna kadar giden bir punto yuva
/// başına gigabaytlık tampon ister ve `texture_px()` Metal'in doku sınırını
/// katbekat aşar. Alt sınır okunmayan puntoları keser. Ayar ayrıştırıcısı
/// (`bt-core`) yalnız "sonlu ve sıfırdan büyük" diyor, aralığın **tek sahibi
/// burası**: `Atlas` kendi değişmezini çağıranın disiplinine bırakmıyor, ve
/// ölçüt `punto × ölçek` olduğu için ayar tarafında bir tavan pencere ekran
/// değiştirdikçe anlamını değiştirirdi. Kırpma **sessiz**
/// (`.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 4).
/// Bağlam satırının gösterim fontuna oranı.
///
/// **Ölçülmüş bir sayı değil, bir tasarım sabiti** (`CellMetrics::GUTTER_PT`
/// emsali): kullanıcının seçimi, belirgin bir hiyerarşi versin diye. Oran,
/// mutlak punto değil — Cmd +/− ile gösterim fontu büyüyünce bağlam satırı da
/// büyür ve iki satırın ilişkisi sabit kalır.
///
/// Çarpım [`effective_point_size`]'ın aralığına giriyor, yani çok küçük
/// gösterim fontunda taban puntoya oturuyor: 5pt'nin %80'i 4.0, tam sınır.
pub(crate) const CONTEXT_SCALE: f64 = 0.8;

const MIN_POINT_SIZE: f64 = 4.0;
const MAX_POINT_SIZE: f64 = 144.0;

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
    faces: Faces,
    /// Bağlam satırının düz yüzü: aynı aile, [`CONTEXT_SCALE`] katı punto.
    ///
    /// Dört yüzü değil **tek** yüzü tutuyor, çünkü küçük sınıfın tek
    /// tüketicisi dock'un bağlam satırı ve orada kalın/eğik yok
    /// ([`SizeClass`]). Dört yüz kurmak üç CoreText türetmesi ve ikinci bir
    /// "yüz edinilemedi" uyarısı demekti — ikisi de karşılığı olmayan bedel.
    small: CFRetained<CTFont>,
    metrics: Metrics,
    /// Küçük yüzün ilerleme genişliği, piksel: bağlam satırının sütun adımı.
    ///
    /// **Yalnız genişlik**, çünkü küçük glyph de büyük yuvaya, büyük hücrenin
    /// taban çizgisine rasterize ediliyor ([`Atlas::slot`]): yükseklik ve
    /// taban ortak, ayrışan tek şey harflerin arasındaki mesafe.
    context_cell_w: u16,
    /// Kurulduğu (aile, punto, ölçek). [`Atlas::ensure`]'nin ölçütü.
    key: Key,
    /// Zincirin istenen aile için söylediği; `None` → istenen açıldı ya da
    /// aile istenmedi.
    font_issue: Option<FontIssue>,
    /// Izgaranın (sütun, satır) yuva sayısı.
    grid: (u16, u16),
    /// Karakterin **çözümlendiği** yuva — yalnız yüklenenler değil: fontun
    /// tanımadığı karakter de burada [`TOFU`] olarak yaşıyor, yoksa aynı
    /// karakter her karede CoreText'e yeniden sorulurdu.
    slots: HashMap<(Sprite, Face, SizeClass), u16>,
    /// Bir sonraki boş yuva; [`TOFU`] ayrılmış olduğu için 1'den başlar.
    /// `slots.len()`'den türetilemez: tofu'ya çözümlenen kayıtlar yuva
    /// harcamıyor, yani iki sayı bilerek ayrışıyor.
    next: u16,
    /// Tek yuvalık çizim tamponu. Alan olması kare başına yeniden ayırmayı
    /// önlüyor; içeriği her yeni glyph'te üzerine yazılır.
    buffer: Vec<u8>,
    /// Rezident tofu kutusu; ömür boyu değişmez.
    tofu: Vec<u8>,
}

/// Atlasın anahtarı: bu dördünden biri değişirse metrik, raster ve yuva
/// eşlemesi geçersizdir.
///
/// `line_height` de anahtarın parçası, çünkü hücre yüksekliğini o da
/// belirliyor: yuva boyu değişince bütün raster geçersiz.
#[derive(Debug, PartialEq)]
struct Key {
    family: Option<String>,
    point_size: f64,
    scale: f64,
    line_height: f64,
}

impl Key {
    /// Karşılaştırma **tam eşitlik**: punto ve ölçek ayrık değerler arasında
    /// sıçrıyor, aralarında yorumlanacak bir yakınlık yok. Aile adı olduğu gibi
    /// — `"menlo"` ile `"Menlo"` aynı fontu açsa da ayrı anahtar; bedeli tek
    /// bir yeniden kurulum.
    fn is(&self, family: Option<&str>, point_size: f64, scale: f64, line_height: f64) -> bool {
        self.family.as_deref() == family
            && self.point_size == point_size
            && self.scale == scale
            && self.line_height == line_height
    }
}

impl Atlas {
    /// `family` ayarın aile adı (`None` → zincir), `point_size` mantıksal
    /// punto, `scale` ekranın backing ölçeği, `line_height` satır aralığı
    /// çarpanı (`1.0` → fontun kendi aralığı).
    ///
    /// Punto ile ölçek **çarpılıp** fonta girer: metrik ve raster aynı fiziksel
    /// piksel uzayında doğar, yani ölçek önbellek anahtarının parçasıdır. Aile
    /// de öyle: başka fontun metriği başka hücre demek. Anahtarın değişmesi
    /// hâlinde yapılacak şeyi [`Atlas::ensure`] biliyor.
    ///
    /// Bulunamayan aile **hata değil**: zincirdeki font açılır ve
    /// [`Atlas::font_issue`] bunu söyler. Terminal fontsuz açılamaz; yanlış
    /// yazılmış bir ad pencereyi kapatmamalı.
    pub fn new(family: Option<&str>, point_size: f64, scale: f64, line_height: f64) -> Self {
        let (faces, font_issue) =
            Faces::from_chain(family, effective_point_size(point_size, scale));
        // Metrik **yalnız düz yüzden**: hücre ızgarası yüze göre oynayamaz.
        // Kalın glyph aynı yuvaya rasterize olur ve bir piksel kırpılabilir —
        // her terminal bunu böyle yapıyor.
        let metrics = font::metrics(faces.get(Face::Regular), line_height);
        // Küçük yüz **aynı zincirden**: `font_issue` ikinci kez sorulmuyor ve
        // yok sayılıyor, çünkü aynı aileye aynı cevap gelir — ikinci bir kayıt
        // kullanıcıya aynı uyarıyı iki kez söyletirdi.
        let (small, _) = font::open_chain(
            family,
            effective_point_size(point_size * CONTEXT_SCALE, scale),
        );
        // `line_height` küçük yüze de uygulanıyor ama **yalnız genişliği**
        // alınıyor; satır aralığı hücrenin boyunu belirler ve o boy ortak.
        let context_cell_w = font::metrics(&small, line_height).cell_px.0;
        let (w, h) = metrics.cell_px;
        // `w`/`h` en az 1 (`font::round_up`), yani bölme güvenli; `max(1)` de
        // hücrenin dokudan büyük olduğu uç için.
        let grid = ((TEXTURE_EDGE / w).max(1), (TEXTURE_EDGE / h).max(1));
        Self {
            faces,
            small,
            metrics,
            context_cell_w,
            key: Key {
                family: family.map(str::to_owned),
                point_size,
                scale,
                line_height,
            },
            font_issue,
            grid,
            slots: HashMap::new(),
            next: TOFU + 1,
            buffer: vec![0u8; metrics.slot_bytes()],
            tofu: tofu_buffer(metrics),
        }
    }

    /// Anahtar ([`Atlas::new`]'in dörtlüsü) değiştiyse atlası
    /// yeniden kurar ve `true` döner.
    ///
    /// `true` aynı zamanda **"dokuyu yeniden ayır"** demektir: metrik ve
    /// dolayısıyla [`Atlas::texture_px`] değişmiş olabilir, eski boyutlu
    /// dokuya yeni metrikle yazmak sessizce bozar. Ölçek değişimini AppKit
    /// haber veriyor (`windowDidChangeBackingProperties:`), aile ve puntoyu
    /// ayar dosyası; bu metot iki kancanın da karşılığı ve yeniden kurma
    /// kararını çağıranın hatırlamasına bırakmıyor.
    #[must_use = "true ise atlas yeniden kuruldu: yuva eşlemesi ve doku boyutu değişmiş olabilir, doku da yeniden ayrılmalı"]
    pub fn ensure(
        &mut self,
        family: Option<&str>,
        point_size: f64,
        scale: f64,
        line_height: f64,
    ) -> bool {
        if self.key.is(family, point_size, scale, line_height) {
            return false;
        }
        *self = Self::new(family, point_size, scale, line_height);
        true
    }

    pub fn metrics(&self) -> Metrics {
        self.metrics
    }

    /// Bağlam satırının sütun adımı, piksel; bkz. [`Atlas::context_cell_w`].
    ///
    /// En az 1: `font::round_up` küçük yüzün ilerlemesini de 1'e kırpıyor,
    /// yani bölen olarak kullanmak güvenli.
    pub fn context_cell_w(&self) -> u16 {
        self.context_cell_w
    }

    /// İstenen ailenin sonucu: bulunamadı ya da eşaralıklı değil. Aile
    /// istenmediyse ya da istenen eşaralıklı bir aile açıldıysa `None`.
    ///
    /// Atlasla birlikte doğuyor ve yeniden kurulumda yeniden hesaplanıyor;
    /// ölçek değişimi aynı cevabı verir, yani pencereyi başka ekrana taşımak
    /// cevabı oynatmaz.
    pub fn font_issue(&self) -> Option<&FontIssue> {
        self.font_issue.as_ref()
    }

    /// Atlas dokusunun piksel boyutu; `bt-gpu` dokuyu buna göre ayırır.
    ///
    /// [`TEXTURE_EDGE`] değil **tam ızgara**: kenardaki artık şerit hiçbir
    /// yuvaya düşmez, ayırmanın da anlamı yok.
    pub fn texture_px(&self) -> (u16, u16) {
        let (w, h) = self.metrics.cell_px;
        (self.grid.0 * w, self.grid.1 * h)
    }

    /// Yuvanın doku içindeki sol üst köşesi, piksel. uv aritmetiği çağıranın.
    pub fn slot_origin(&self, slot: u16) -> (u16, u16) {
        // Izgara dışı yuva [`TOFU`]'ya düşer. Bu bir savunma refleksi değil,
        // gerçek bir yol: `ensure()` ızgarayı küçültebiliyor ve çağıranın
        // elinde bir önceki ölçekten kalma yuva numarası olabilir. `debug_assert`
        // yetmezdi — release'de dokunun dışını gösteren bir köşe döner,
        // `replaceRegion` sınır dışına yazar ve belirti sessizdir.
        let slot = if slot < self.capacity() { slot } else { TOFU };
        let (w, h) = self.metrics.cell_px;
        ((slot % self.grid.0) * w, (slot / self.grid.0) * h)
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
    pub fn slot(
        &mut self,
        sprite: Sprite,
        face: Face,
        size: SizeClass,
    ) -> (u16, Option<Upload<'_>>) {
        // Anahtar **istenen** yüzü değil **çizilen** yüzü taşır. Üç ayrı
        // sebeple ayrışabiliyorlar ve üçü de aynı cümlenin yüzü:
        //   - kural çizgileri yüzden bağımsız (kalın metnin altındaki çizgi
        //     kalın değildir) ve ölçüden de: dock'un bağlam satırında kural
        //     yok, yani küçük bir kural sprite'ı hiç doğmaz,
        //   - fontta olmayan yüz düz yüze çökmüştür (`Faces::effective`),
        //   - küçük sınıfta yalnız düz yüz var ([`Atlas::small`]).
        // Normalizasyon **burada**, çağıranın disiplininde değil: ayrışan bir
        // anahtar bayt bayt aynı bitmap'i ayrı yuvalarda tutar, atlas kat kat
        // hızlı dolar ve belirti sessizdir.
        let (face, size) = match (sprite, size) {
            (Sprite::Rule(_), _) => (Face::Regular, SizeClass::Normal),
            (Sprite::Char(_), SizeClass::Small) => (Face::Regular, SizeClass::Small),
            (Sprite::Char(_), SizeClass::Normal) => (self.faces.effective(face), SizeClass::Normal),
        };
        let key = (sprite, face, size);
        if let Some(&slot) = self.slots.get(&key) {
            return (slot, None);
        }
        // Kural sprite'larına **pay ayrılıyor**: altısı da yordamsal,
        // deterministik ve ömür boyu gerekli. Pay olmasaydı, birkaç bin farklı
        // glyph gördükten sonra (CJK metin, simge-ağır TUI) ızgara dolar ve o
        // andan itibaren altı çizili **her** hücrenin altında çizgi yerine
        // tofu kutusu belirirdi. Karakterler son `RULE_RESERVE` yuvayı yiyemez;
        // kurallar tembel kalır ama yerleri garantidir.
        let cap = match sprite {
            Sprite::Rule(_) => self.capacity(),
            Sprite::Char(_) => self.capacity().saturating_sub(RULE_RESERVE),
        };
        if self.next >= cap {
            // Dolu atlas **önbelleklenmez**: bu, fontun kalıcı bir gerçeği
            // değil atlasın geçici hâli. Tahliye geldiğinde (00X) buraya
            // yazılacak kayıt yanlış olurdu — yer açılmış ama karakter hâlâ
            // tofu'ya bağlı kalırdı.
            return (TOFU, None);
        }
        // Ödünç match'in scrutinee'sinde bırakılmıyor: `&mut self.buffer`
        // orada kalsaydı kolların içinde `&self.buffer` alınamazdı.
        let result = match sprite {
            Sprite::Char(ch) => {
                // **Metrik her iki sınıfta da büyük hücrenin**: küçük glyph
                // büyük yuvaya, büyük hücrenin taban çizgisine çiziliyor
                // (`raster::draw` glyph'i `(0, baseline)`'a koyuyor). Yuva
                // boyu ortak kaldığı için ızgara, doku ve `slot_bytes`
                // değişmiyor — ayrışan tek şey harfin kendi boyu, o da
                // fonttan geliyor.
                let font = match size {
                    SizeClass::Normal => self.faces.get(face),
                    SizeClass::Small => &self.small,
                };
                raster::draw(font, ch, self.metrics, &mut self.buffer)
            }
            // Yordamsal çizim başarısız olamaz: font sorulmuyor, bağlam
            // kurulmuyor. `Drawn` bir varsayım değil, tipin kendisi.
            Sprite::Rule(kind) => {
                raster::draw_rule(kind, self.metrics, &mut self.buffer);
                DrawResult::Drawn
            }
        };
        match result {
            DrawResult::Drawn => {
                let slot = self.next;
                self.next += 1;
                self.slots.insert(key, slot);
                let origin = self.slot_origin(slot);
                (
                    slot,
                    Some(Upload {
                        origin,
                        bytes: &self.buffer,
                    }),
                )
            }
            // İkisi de **kalıcı**: fontun o karakteri yoktur, ya da bağlam
            // kurulumu (argümanları atlas ömrü boyunca sabit) hep başarısızdır.
            // Önbelleğe girmeselerdi aynı karakter ekranda durduğu sürece her
            // karede yeniden CoreText'e sorulurdu.
            // Yüzler arası **kapsam farkı** gerçek: birçok ailede düz yüz
            // geniş bir Unicode bloğu taşırken kalın/eğik yalnız Latin
            // taşıyor. `Faces::effective`'in yüz düzeyinde yaptığı geri düşüşün
            // glyph düzeyindeki karşılığı bu — olmasaydı kalın bir satırdaki
            // '→' tofu kutusu olur, aynı karakter düz satırda düzgün çizilirdi.
            // Özyineleme tek adım: düz yüzde `face == Regular` ve bu kol
            // yeniden ateşlenmiyor.
            //
            // **İstenen anahtar da yazılıyor.** Yazılmasaydı geri düşüş her
            // karede yeniden yaşanırdı: `(Char('→'), Bold)` haritada hiç
            // görünmez, `raster::draw` kalın fontu her kare CoreText'e sorar
            // (`font::glyph_index`), `NoGlyph` alır ve düz yüze düşerdi — ve
            // bu, `slot()` çizim yolunda olduğu için ana thread'de, kare
            // bütçesinin ortasında. Tam olarak hemen yukarıdaki yorumun
            // "önbelleğe girmeselerdi her karede yeniden sorulurdu"
            // gerekçesi; o gerekçe bu kol için de geçerli. Düz yüz de
            // `NoGlyph` verirse takma ad `TOFU`'ya bağlanır ve negatif
            // önbelleğin tavanı onu da süpürür.
            // Boyut sınıfı **korunuyor**: bugün bu kol küçük sınıfta hiç
            // çalışmıyor (orada yüz zaten `Regular`, koşul kapalı), ama
            // `Normal` yazmak geri düşüşü sessizce büyük yüze bağlardı —
            // küçük satırın eksik glyph'i büyük harf olarak belirirdi.
            DrawResult::NoGlyph if face != Face::Regular => {
                let (slot, upload) = self.slot(sprite, Face::Regular, size);
                // `map` `upload`'ı tüketiyor ve `self.buffer` ödüncü burada
                // bitiyor; `insert` ancak ondan sonra mümkün. Tampon özyineli
                // çağrının çizdiği baytları hâlâ taşıyor, yani `Upload` aynı
                // içerikle yeniden kurulabiliyor.
                let origin = upload.map(|upload| upload.origin);
                self.slots.insert(key, slot);
                let upload = origin.map(|origin| Upload {
                    origin,
                    bytes: &self.buffer,
                });
                (slot, upload)
            }
            DrawResult::NoGlyph | DrawResult::NoContext => {
                // Tavan: negatif önbellek yuva harcamıyor, yani `next` onu
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
                // `capacity()` yeni kayıt sığıyor. Pozitif kayıtlar (gerçek
                // yuvalar) korunuyor; onların tahliyesi LRU'nun işi (00X).
                if self.slots.len() >= self.negative_cache_cap() {
                    self.slots.retain(|_, &mut slot| slot != TOFU);
                }
                self.slots.insert(key, TOFU);
                (TOFU, None)
            }
        }
    }

    /// (kullanılan, toplam) yuva.
    ///
    /// Tofu kullanılan sayılır: doku o yuvayı da tutuyor ve doluluk oranı
    /// `/measure`'da bu iki sayıdan okunacak.
    pub fn occupancy(&self) -> (usize, usize) {
        (usize::from(self.next), usize::from(self.capacity()))
    }

    /// Haritanın kabul ettiği en çok kayıt sayısı — pozitif ve negatif
    /// birlikte. Kapasitenin **iki katı**: bir katı pozitif kayıtların
    /// olabildiği en büyük değer, ikincisi negatif önbelleğe bırakılan pay.
    fn negative_cache_cap(&self) -> usize {
        usize::from(self.capacity()).saturating_mul(2)
    }

    /// Toplam yuva sayısı.
    ///
    /// `u16`'ya kırpılıyor: yuva numarası dışarıya `u16` olarak veriliyor ve
    /// çok küçük hücrelerde ızgara o sınırı aşabilir. Kırpma kapasiteyi
    /// daraltır, taşma ise yuvaları sessizce birbirine bindirirdi.
    fn capacity(&self) -> u16 {
        let total = u32::from(self.grid.0) * u32::from(self.grid.1);
        u16::try_from(total).unwrap_or(u16::MAX)
    }
}

/// Fonta girecek punto: ölçek çarpılmış ve aralığa oturtulmuş.
///
/// NaN ayrıca ele alınıyor çünkü `clamp` onu **geçirir**; taban puntoya düşmek
/// hem çökmekten hem sessizce bozulmaktan iyi — sonuç görünür şekilde yanlış
/// olur ve fark edilir.
fn effective_point_size(point_size: f64, scale: f64) -> f64 {
    let v = point_size * scale;
    if v.is_finite() {
        v.clamp(MIN_POINT_SIZE, MAX_POINT_SIZE)
    } else {
        MIN_POINT_SIZE
    }
}

/// Tofu kutusunu çizer: hücre kenarından bir piksel içeride, 1 px çerçeve.
///
/// Fontun `.notdef` glyph'i **kullanılmıyor**: bazı fontlarda boş, bazılarında
/// kutu ve hangisi olduğu font sürümüne bağlı. Çerçeveyi kendimiz çizmek
/// tofu'yu fonttan bağımsız kılıyor — "görünür kayıp" iddiası ancak böyle
/// tutuyor.
fn tofu_buffer(m: Metrics) -> Vec<u8> {
    let (w, h) = m.cell_wh();
    let mut target = vec![0u8; m.slot_bytes()];
    let (x0, x1) = (1usize, w.saturating_sub(2));
    let (y0, y1) = (1usize, h.saturating_sub(2));
    if x1 <= x0 || y1 <= y0 {
        // Hücre çerçeveye dar; boş yuva kutudan iyidir.
        return target;
    }
    // audit: `x1 < w` ve `y1 < h` (ikisi de `saturating_sub(2)`), yani en
    // büyük indeks `y1 * w + x1 < w * h` — dilim sınırı içinde.
    for x in x0..=x1 {
        target[y0 * w + x] = 0xff;
        target[y1 * w + x] = 0xff;
    }
    for y in y0..=y1 {
        target[y * w + x0] = 0xff;
        target[y * w + x1] = 0xff;
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sınama puntosu bilerek büyük: ızgara hücre ölçüsünden türüyor, yani
    /// büyük punto = az yuva. "Dolu atlas" sınaması böylece binlerce glyph
    /// rasterize etmeden, onlarcasıyla koşuyor. Değer [`MAX_POINT_SIZE`]'tur:
    /// üstünü istemek sessizce kırpılır ve sınama kapasiteyi yanlış sanırdı.
    const LARGE_POINT_SIZE: f64 = MAX_POINT_SIZE;
    const POINT_SIZE: f64 = 13.0;
    /// Menlo ve SF Mono CJK içermez ve `CTFontGetGlyphsForCharacters` başka
    /// fonta düşmez: bu karakter `.notdef` verir.
    const UNKNOWN_CHAR: char = '漢';
    /// Hiçbir makinede olmayan aile; CoreText yerine başka bir font verir.
    const MISSING_FAMILY: &str = "Bu Aile Yok 12345";

    /// Zincirle kurulan atlas — ayarda aile yokken üretimin kurduğu.
    fn atlas(point_size: f64, scale: f64) -> Atlas {
        Atlas::new(None, point_size, scale, 1.0)
    }

    #[test]
    fn metrics_are_in_a_sane_range() {
        let m = atlas(POINT_SIZE, 1.0).metrics();
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
    fn same_char_gets_same_slot() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let (first, upload) = a.slot(Sprite::Char('A'), Face::Regular, SizeClass::Normal);
        assert_ne!(first, TOFU, "tanınan karakter tofu'ya düşmemeli");
        assert!(upload.is_some(), "ilk soruluşta yükleme gelmeli");
        let (second, again) = a.slot(Sprite::Char('A'), Face::Regular, SizeClass::Normal);
        assert_eq!(first, second);
        assert!(
            again.is_none(),
            "yuva zaten yüklü: doku el değmeden kalmalı"
        );
    }

    #[test]
    fn upload_carries_slot_origin() {
        // Bu bekçinin asıl işi derlenmek: köşe ile baytlar ayrı çağrılardan
        // gelseydi `bt-gpu`'nun yükleme döngüsü `&mut` ödüncü elindeyken
        // `&self` istemek zorunda kalır ve derlenmezdi.
        let mut a = atlas(POINT_SIZE, 1.0);
        let slot_len = a.metrics().slot_bytes();
        // `bt-gpu`'nun yükleme döngüsünün şekli: yükleme kendi bloğunda
        // tüketilir, sonra aynı atlas uv için yeniden okunur. Köşe
        // `Upload`'nin içinde olmasaydı o blokta `&self` istemek gerekirdi
        // ve `slot`'un `&mut` ödüncü yüzünden derlenmezdi.
        let (slot, upload) = a.slot(Sprite::Char('A'), Face::Regular, SizeClass::Normal);
        let mut written = None;
        if let Some(y) = upload {
            assert_eq!(y.bytes.len(), slot_len);
            written = Some(y.origin);
        }
        assert_eq!(written, Some(a.slot_origin(slot)));
    }

    #[test]
    fn rasterized_glyph_is_not_empty() {
        // Bu bekçi olmadan "her şey çalışıyor ama atlas bomboş" durumu sessiz
        // kalır: yuva numaraları doğru, doku doğru boyutta, ekran boş.
        let mut a = atlas(POINT_SIZE, 1.0);
        let (_, upload) = a.slot(Sprite::Char('W'), Face::Regular, SizeClass::Normal);
        let bytes = upload.expect("ilk soruluşta yükleme gelmeli").bytes;
        assert!(bytes.iter().any(|&b| b > 0), "'W' hiç piksel boyamadı");
        // Boşluk da tanınan bir glyph'tir ama hiçbir şey boyamaz: ölçüt
        // "bitmap doldu mu" değil, "raster çalıştı mı".
        let (slot, blank) = a.slot(Sprite::Char(' '), Face::Regular, SizeClass::Normal);
        assert_ne!(slot, TOFU);
        assert!(
            blank.expect("yeni yuva").bytes.iter().all(|&b| b == 0),
            "boşluk boyamamalı"
        );
    }

    #[test]
    fn tofu_box_is_drawn_and_resident() {
        let mut a = atlas(POINT_SIZE, 1.0);
        assert_eq!(a.tofu_bitmap().len(), a.metrics().slot_bytes());
        assert!(
            a.tofu_bitmap().iter().any(|&b| b > 0),
            "tofu boş kutu olamaz"
        );
        // Tofu'ya düşen çağrı yükleme **vermez**: veri dokuda zaten.
        assert!(
            a.slot(Sprite::Char(UNKNOWN_CHAR), Face::Regular, SizeClass::Normal)
                .1
                .is_none(),
            "rezident yuva yeniden yüklenmez"
        );
    }

    #[test]
    fn unknown_char_is_cached() {
        let mut a = atlas(POINT_SIZE, 1.0);
        assert_eq!(
            a.slot(Sprite::Char(UNKNOWN_CHAR), Face::Regular, SizeClass::Normal)
                .0,
            TOFU
        );
        // Fontun bu karakteri tanımaması kalıcı: ikinci soruluşta CoreText'e
        // gidilmemeli. Bekçi iç tabloya bakıyor çünkü FFI çağrısının olup
        // olmadığı dışarıdan gözlenemiyor.
        assert_eq!(
            a.slots
                .get(&(Sprite::Char(UNKNOWN_CHAR), Face::Regular, SizeClass::Normal)),
            Some(&TOFU),
            "tofu çözümü önbelleğe girmeli"
        );
        assert_eq!(a.occupancy().0, 1, "tofu düşüşü yuva harcamamalı");
    }

    #[test]
    fn face_fallback_is_cached_under_the_requested_face() {
        // `─` (U+2500) **ölçüldü** (bu makine, Menlo 13pt): düz yüzde var,
        // kalın yüzde yok. Yani glyph düzeyindeki geri düşüş gerçek bir fontla
        // ateşlenebiliyor — ve hiç de seyrek bir durum değil: kalın bir TUI
        // çerçevesi bu koldan geçiyor.
        const BOX_DRAWING: char = '─';
        let mut a = atlas(POINT_SIZE, 1.0);
        let regular = a
            .slot(Sprite::Char(BOX_DRAWING), Face::Regular, SizeClass::Normal)
            .0;
        let (bold, _) = a.slot(Sprite::Char(BOX_DRAWING), Face::Bold, SizeClass::Normal);

        // Geri düşüşün kendisi: kalın istek tofu'ya değil düz yüzün yuvasına
        // çözülmeli, yoksa kalın bir satırdaki çerçeve kutu kutu görünürdü.
        assert_ne!(bold, TOFU, "kalın yüzde olmayan glyph tofu'ya düştü");
        assert_eq!(bold, regular, "geri düşüş düz yüzün yuvasını vermeli");

        // Asıl bekçi: **istenen** yüzün anahtarı da haritada. Olmasaydı bu
        // çözüm hiç önbelleğe girmez, `slot()` çizim yolunda olduğu için de
        // ekranda duran her kalın çerçeve hücresi **her karede** CoreText'e
        // yeniden sorulurdu — ana thread'de. Dışarıdan gözlenemediği için
        // bekçi iç tabloya bakıyor; `unknown_char_is_cached` ile aynı gerekçe.
        assert_eq!(
            a.slots
                .get(&(Sprite::Char(BOX_DRAWING), Face::Bold, SizeClass::Normal)),
            Some(&regular),
            "geri düşüş istenen yüzün anahtarıyla önbelleğe girmeli"
        );
        assert_eq!(a.occupancy().0, 2, "geri düşüş ikinci bir yuva harcadı");
    }

    #[test]
    fn full_atlas_returns_tofu_without_caching() {
        let mut a = atlas(LARGE_POINT_SIZE, 1.0);
        let (used, total) = a.occupancy();
        assert_eq!(used, 1, "yeni atlasta yalnız tofu ayrılmış olmalı");
        // Yazdırılabilir ASCII'nin tamamı: havuz kapasiteden büyük olmalı ve
        // harf/rakam (62) `MAX_POINT_SIZE`'taki kapasiteye yetmiyor.
        let pool: Vec<char> = (' '..='~').collect();
        assert!(
            pool.len() > total,
            "sınama havuzu kapasiteyi aşmalı: havuz={} kapasite={total}",
            pool.len()
        );
        let dropped: Vec<char> = pool
            .iter()
            .copied()
            .filter(|&ch| a.slot(Sprite::Char(ch), Face::Regular, SizeClass::Normal).0 == TOFU)
            .collect();
        assert!(!dropped.is_empty(), "kapasite aşılınca tofu beklenir");
        assert_eq!(
            a.occupancy(),
            (total - usize::from(RULE_RESERVE), total),
            "karakterler ızgarayı kural payı hariç doldurmalı"
        );
        // **Payın kendisi.** Karakterler tavana dayandıktan sonra bile kural
        // sprite'ı gerçek bir yuva alıyor. Pay olmasaydı bu noktadan itibaren
        // altı çizili her hücrenin altında çizgi yerine tofu kutusu belirirdi
        // ve belirti ancak uzun bir oturumdan sonra ortaya çıkardı.
        let (rule, _) = a.slot(
            Sprite::Rule(RuleKind::Single),
            Face::Regular,
            SizeClass::Normal,
        );
        assert_ne!(rule, TOFU, "dolu atlasta kural sprite'ı tofu'ya düştü");
        // Dolu atlas geçici bir hâl: tahliye gelince (00X) yer açılacak ve
        // bu karakterlerin tofu'ya bağlı kalmaması gerekiyor.
        for ch in dropped {
            assert!(
                !a.slots
                    .contains_key(&(Sprite::Char(ch), Face::Regular, SizeClass::Normal)),
                "'{ch}' kalıcı olarak tofu'ya yazılmış"
            );
        }
    }

    #[test]
    fn glyph_sits_on_the_baseline() {
        // Bu bekçi olmadan y ekseni ters çevrilse (CG'nin başlangıcı sol
        // **alt**) ya da taban yanlış hesaplansa bütün sınamalar yeşil kalır:
        // `rasterized_glyph_is_not_empty` yalnız "bir yerde piksel var"
        // diyor. `bt-gpu`'nun offscreen kapısı da göremezdi: o da "hücrenin
        // içi arka planla tekdüze değil" diyor, harfin doğru yerde olduğunu
        // değil. Ters bir taban ancak gözle görülürdü.
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (_, upload) = a.slot(Sprite::Char('W'), Face::Regular, SizeClass::Normal);
        let bytes = upload.expect("yeni yuva").bytes;
        let w = usize::from(m.cell_px.0);
        let has_ink = |row: usize| bytes[row * w..(row + 1) * w].iter().any(|&b| b > 0);
        let baseline = usize::from(m.baseline_px);
        // 'W' ne descender taşır ne aksan: kapsamanın tamamı tabanın üstünde.
        assert!(
            (0..baseline).any(has_ink),
            "taban çizgisinin üstü boş: {m:?}"
        );
        assert!(
            !(baseline..usize::from(m.cell_px.1)).any(has_ink),
            "'W' taban çizgisinin altına taşmamalı: {m:?}"
        );
    }

    #[test]
    fn line_height_grows_the_cell_and_keeps_the_glyph_centred() {
        // `[font] line_height` (kullanıcı: "satır aralarını biraz daha
        // açabilir miyiz? hatta bu bir değişken olabiliyor mu?").
        //
        // Üç iddia ve üçü de sessizce bozulabilir:
        let tight = atlas(POINT_SIZE, 1.0).metrics();
        let airy = Atlas::new(None, POINT_SIZE, 1.0, 1.5).metrics();

        // (1) **Yalnız yükseklik büyüyor.** Genişlik fontun advance'ından
        //     geliyor ve satır aralığıyla hiç ilgisi yok; büyüseydi eşaralıklı
        //     ızgara bozulur, metin seyrekleşirdi.
        assert_eq!(airy.cell_px.0, tight.cell_px.0, "genişlik de büyüdü");
        assert!(
            airy.cell_px.1 > tight.cell_px.1,
            "yükseklik büyümedi: {tight:?} → {airy:?}"
        );

        // (2) **Fazlalık altta ve üstte eşit.** Taban çizgisinin indiği kadar
        //     altta da yer açılmalı; tek yana eklenseydi metin hücrenin içinde
        //     kayar ve çarpan büyüdükçe kayma büyürdü. `±1`: fazlalık tek
        //     sayıysa yarısı aşağı yuvarlanıyor.
        let extra = airy.cell_px.1 - tight.cell_px.1;
        let above = airy.baseline_px - tight.baseline_px;
        let below = extra - above;
        assert!(
            above.abs_diff(below) <= 1,
            "fazlalık eşit dağılmadı: üstte {above}, altta {below}"
        );

        // (3) **Kurallar tabanla birlikte iniyor.** İkisi de tabandan
        //     ölçülüyor; ayrı bir düzeltme eklenseydi çarpan büyüdükçe alt
        //     çizgi harften kopardı.
        assert_eq!(
            airy.underline_px.0 - tight.underline_px.0,
            above,
            "alt çizgi tabanla inmedi"
        );
        assert_eq!(
            airy.strikeout_px.0 - tight.strikeout_px.0,
            above,
            "üstü çizili tabanla inmedi"
        );

        // (4) **`1.0` no-op.** Varsayılan yol fazladan bir piksel bile
        //     oynatmamalı, yoksa ayarı hiç yazmayan kullanıcının ızgarası
        //     sessizce değişirdi.
        assert_eq!(Atlas::new(None, POINT_SIZE, 1.0, 1.0).metrics(), tight);
    }

    #[test]
    fn a_taller_line_still_fits_the_descender() {
        // [`descender_fits_in_the_cell`]'in çarpanlı hâli: satır aralığı
        // açılınca 'g' alta doğru kaymamalı. Taban çizgisi fazlalığın yarısı
        // kadar iniyor, yani altta kalan boşluk da **büyüyor** — kırpma
        // ihtimali azalıyor, artmıyor. Yine de sınanıyor: aritmetik ters
        // kurulsaydı (fazlalığın tamamı üste) alt boşluk aynı kalır ve
        // yuvarlama bir pikseli yiyebilirdi.
        let mut a = Atlas::new(None, POINT_SIZE, 1.0, 1.5);
        let m = a.metrics();
        let (_, upload) = a.slot(Sprite::Char('g'), Face::Regular, SizeClass::Normal);
        let bytes = upload.expect("yeni yuva").bytes;
        let w = usize::from(m.cell_px.0);
        let has_ink = |row: usize| bytes[row * w..(row + 1) * w].iter().any(|&b| b > 0);
        assert!(
            !has_ink(usize::from(m.cell_px.1) - 1),
            "açık satır aralığında 'g' hücrenin dibine dayandı"
        );
        // Ve üstte de boşluk var: fazlalık tek yana gitmedi.
        assert!(!has_ink(0), "açık satır aralığında glyph tepeye dayandı");
    }

    #[test]
    fn descender_fits_in_the_cell() {
        // Taban çizgisi ile yükseklik **ayrı ayrı** yuvarlanmasaydı
        // (`round_up(ascent + descent + leading)` tek seferde) alta fontun
        // descent'inden az yer kalırdı ve 'g' gibi harflerin son kapsama
        // satırı kırpılırdı. Kırpılan glyph hücrenin son satırını doldurur;
        // sığan glyph orayı boş bırakır — ölçüt bu. 'W' ile sınamak yetmez:
        // descender'ı olmayan harf iki yuvarlamada da aynı görünür.
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (_, upload) = a.slot(Sprite::Char('g'), Face::Regular, SizeClass::Normal);
        let bytes = upload.expect("yeni yuva").bytes;
        let w = usize::from(m.cell_px.0);
        let has_ink = |row: usize| bytes[row * w..(row + 1) * w].iter().any(|&b| b > 0);
        let baseline = usize::from(m.baseline_px);
        assert!(
            has_ink(baseline),
            "'g' taban çizgisinin altına inmeli: {m:?}"
        );
        assert!(
            !has_ink(usize::from(m.cell_px.1) - 1),
            "descender hücrenin son satırında kırpılmış: {m:?}"
        );
    }

    #[test]
    fn negative_cache_is_capped_and_evicted() {
        let mut a = atlas(LARGE_POINT_SIZE, 1.0);
        let cap = a.negative_cache_cap();
        // Tanınan bir karakter önce yuvasını alsın: tahliyenin **yalnız**
        // negatif kayıtları attığını sınamak için bir pozitif kayıt gerek.
        let (letter, _) = a.slot(Sprite::Char('A'), Face::Regular, SizeClass::Normal);
        assert_ne!(letter, TOFU, "'A' Menlo'da var");

        // Tanınmayan karakter yuva harcamıyor, yani `next` onu
        // sınırlamıyor. Tavan olmasaydı harita gördüğü ayrı codepoint sayısı
        // kadar büyürdü ve bir ikili dosyayı `cat`'lemek bunu gerçek bir yola
        // çevirir. Crate'in tavanı olmayan tek sayısı burasıydı.
        let pool: Vec<char> = ('\u{4e00}'..'\u{9fff}').take(cap * 3).collect();
        assert!(pool.len() > cap, "havuz tavanı aşmalı");
        for &ch in &pool {
            assert_eq!(
                a.slot(Sprite::Char(ch), Face::Regular, SizeClass::Normal).0,
                TOFU,
                "'{ch}' Menlo/SF Mono'da yok"
            );
            assert!(
                a.slots.len() <= cap,
                "negatif önbellek tavanı aşıldı: {} > {cap}",
                a.slots.len()
            );
        }
        assert_eq!(a.occupancy().0, 2, "tofu düşüşleri yuva harcamamalı");

        // Tavan dolunca önbellekleme **durmuyor**, tahliye oluyor: tahliyeden
        // sonra gelen kayıt haritaya giriyor. Eski davranışta ("tavan dolu →
        // hiç yazma") burası boş dönerdi ve ekranda duran her desteklenmeyen
        // karakter her karede CoreText'e geri sorulurdu — `slot()` bu sette
        // çizim yoluna girdiği için bedeli ana thread'de ödenirdi.
        let last = *pool.last().expect("havuz boş değil");
        assert_eq!(
            a.slots
                .get(&(Sprite::Char(last), Face::Regular, SizeClass::Normal)),
            Some(&TOFU),
            "tahliyeden sonraki kayıt önbelleğe girmeli"
        );
        // Pozitif kayıt tahliyeye girmiyor: yuvası duruyor.
        assert_eq!(
            a.slot(Sprite::Char('A'), Face::Regular, SizeClass::Normal)
                .0,
            letter,
            "pozitif kayıt tahliyede kayboldu"
        );
    }

    #[test]
    fn non_bmp_char_path_works() {
        // Surrogate çifti: `encode_utf16` iki birim üretiyor, CoreText ikinci
        // birime de dokunuyor ve glyph üretmeyip `false` dönüyor. `font::glif`
        // o dönüşü bilerek yok sayıyor ve işaretçilerini dilimden türetiyor;
        // ikisinin gerekçesi de ancak bu yol koşarsa sınanmış olur.
        let mut a = atlas(POINT_SIZE, 1.0);
        assert_eq!(
            a.slot(Sprite::Char('𝔸'), Face::Regular, SizeClass::Normal)
                .0,
            TOFU,
            "Menlo/SF Mono matematik alfabesi içermez"
        );
    }

    #[test]
    fn broken_point_size_does_not_break_atlas() {
        // `NaN as u16` sıfırdır ve `clamp` NaN'ı geçirir: sınır konmasaydı
        // ızgara sıfıra bölerdi. Devasa punto ise yuva başına gigabaytlık
        // tampon isterdi.
        for (point_size, scale) in [(f64::NAN, 1.0), (13.0, f64::INFINITY), (1e9, 1.0)] {
            let a = atlas(point_size, scale);
            let m = a.metrics();
            assert!(
                m.cell_px.0 > 0 && m.cell_px.1 > 0,
                "{point_size}×{scale}: {m:?}"
            );
            let (tw, th) = a.texture_px();
            assert!(
                tw <= TEXTURE_EDGE && th <= TEXTURE_EDGE,
                "{point_size}×{scale}: {tw}×{th}"
            );
        }
    }

    #[test]
    fn scale_is_part_of_the_key() {
        let one = atlas(POINT_SIZE, 1.0).metrics();
        let two = atlas(POINT_SIZE, 2.0).metrics();
        assert_ne!(one.cell_px, two.cell_px, "@2x hücre @1x ile aynı olamaz");
        // Tam iki kat beklenmiyor: her ölçü ayrı ayrı yukarı yuvarlanıyor.
        assert!(
            two.cell_px.0 + 2 >= one.cell_px.0 * 2 && two.cell_px.0 <= one.cell_px.0 * 2 + 2,
            "@2x genişlik iki katına yakın olmalı: {one:?} → {two:?}"
        );
    }

    #[test]
    fn ensure_rebuilds_only_when_key_changes() {
        let mut a = atlas(POINT_SIZE, 1.0);
        a.slot(Sprite::Char('A'), Face::Regular, SizeClass::Normal);
        assert!(
            !a.ensure(None, POINT_SIZE, 1.0, 1.0),
            "aynı anahtar yeniden kurmamalı"
        );
        assert_eq!(a.occupancy().0, 2, "yuvalar korunmalı");
        assert!(
            a.ensure(None, POINT_SIZE, 2.0, 1.0),
            "ölçek değişti: yeniden kurulmalı"
        );
        assert_eq!(a.occupancy().0, 1, "yeni atlasta yalnız tofu");
        assert_eq!(a.metrics(), atlas(POINT_SIZE, 2.0).metrics());
    }

    #[test]
    fn ensure_rebuilds_when_family_changes() {
        // Monaco ile Menlo 13pt'de aynı hücreyi verebilir; ölçüt metrik değil
        // yuvaların sıfırlanması. Anahtarda aile olmasaydı eski fontun
        // glyph'leri yeni fontun atlasında kalırdı ve belirti sessizdi.
        let mut a = atlas(POINT_SIZE, 1.0);
        a.slot(Sprite::Char('A'), Face::Regular, SizeClass::Normal);
        assert!(
            !a.ensure(None, POINT_SIZE, 1.0, 1.0),
            "aynı anahtar yeniden kurmamalı"
        );
        assert!(
            a.ensure(Some("Monaco"), POINT_SIZE, 1.0, 1.0),
            "aile değişti: yeniden kurulmalı"
        );
        assert_eq!(a.occupancy().0, 1, "yeni atlasta yalnız tofu");
        a.slot(Sprite::Char('A'), Face::Regular, SizeClass::Normal);
        assert!(
            !a.ensure(Some("Monaco"), POINT_SIZE, 1.0, 1.0),
            "aynı aile yeniden kurmamalı"
        );
        assert_eq!(a.occupancy().0, 2, "yuvalar korunmalı");
        assert!(
            a.ensure(None, POINT_SIZE, 1.0, 1.0),
            "zincire dönüş de bir değişim"
        );
    }

    #[test]
    fn missing_family_opens_the_chain_and_says_so() {
        let a = Atlas::new(Some(MISSING_FAMILY), POINT_SIZE, 1.0, 1.0);
        let (_, chain) = font::open_default(POINT_SIZE);
        assert_eq!(
            a.font_issue(),
            Some(&FontIssue::FamilyNotFound {
                requested: MISSING_FAMILY.to_owned(),
                using: chain,
            })
        );
        // CoreText'in ikamesi (bu makinede Helvetica) değil, zincir açıldı.
        assert_eq!(a.metrics(), atlas(POINT_SIZE, 1.0).metrics());
        assert_eq!(atlas(POINT_SIZE, 1.0).font_issue(), None, "zincir sessiz");
    }

    #[test]
    fn family_name_is_matched_regardless_of_case() {
        // CoreText `"menlo"`'yu buluyor ve adı `"Menlo"` diye bildiriyor
        // (ölçüldü); birebir karşılaştırma bulunan fontu "yok" sayar ve
        // zincire düşerdi.
        for name in ["Menlo", "menlo", "MENLO"] {
            let a = Atlas::new(Some(name), POINT_SIZE, 1.0, 1.0);
            assert_eq!(a.font_issue(), None, "{name}");
        }
    }

    #[test]
    fn proportional_family_opens_with_a_warning() {
        // Helvetica her macOS'ta var ve eşaralıklı değil.
        let mut a = Atlas::new(Some("Helvetica"), POINT_SIZE, 1.0, 1.0);
        assert_eq!(
            a.font_issue(),
            Some(&FontIssue::NotMonospaced {
                family: "Helvetica".to_owned()
            })
        );
        // Reddedilmiyor, çiziliyor: hücre boşluktan dar olan 'W' yuvaya
        // kırpılarak rasterize olur, tampon taşmaz.
        let slot_len = a.metrics().slot_bytes();
        let bytes = slot_bytes_of(&mut a, Sprite::Char('W'), Face::Regular);
        assert_eq!(bytes.len(), slot_len);
        assert!(bytes.iter().any(|&b| b > 0), "'W' hiç piksel boyamadı");
    }

    #[test]
    fn missing_family_is_substituted() {
        // CoreText hata vermez, en yakın fontu verir: "font açıldı" bir kanıt
        // değildir ve zincir bu yüzden dönen adı karşılaştırıyor.
        const MISSING: &str = "Bu Aile Yok 12345";
        let (_, returned) = font::open(MISSING, POINT_SIZE);
        assert_ne!(returned, MISSING, "var olmayan aile için ikame beklenir");
    }

    #[test]
    fn slot_origin_walks_the_grid() {
        let a = atlas(POINT_SIZE, 1.0);
        let (w, h) = a.metrics().cell_px;
        let cols = a.grid.0;
        assert_eq!(a.slot_origin(TOFU), (0, 0));
        assert_eq!(a.slot_origin(1), (w, 0));
        assert_eq!(a.slot_origin(cols), (0, h), "ilk yuva bir alt satıra düşer");
        // Doku ızgarayı sarmalı ve kenarda bir hücreden fazlası boşa gitmemeli.
        let (tw, th) = a.texture_px();
        assert!(
            tw <= TEXTURE_EDGE && tw + w > TEXTURE_EDGE,
            "genişlik: {tw}"
        );
        assert!(
            th <= TEXTURE_EDGE && th + h > TEXTURE_EDGE,
            "yükseklik: {th}"
        );
    }

    /// Yuvanın baytlarını kopyalar — `Upload` ödüncü atlası kilitliyor.
    fn slot_bytes_of(a: &mut Atlas, sprite: Sprite, face: Face) -> Vec<u8> {
        let (_, upload) = a.slot(sprite, face, SizeClass::Normal);
        upload.expect("yeni yuva yükleme vermeli").bytes.to_vec()
    }

    #[test]
    fn bold_face_gets_own_slot() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let mut slots = Vec::new();
        for face in [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic] {
            let slot = a.slot(Sprite::Char('M'), face, SizeClass::Normal).0;
            assert_ne!(slot, TOFU, "{face:?} tofu'ya düştü");
            // Anahtar yüzü taşımasaydı dördü aynı yuvayı paylaşır ve kalın 'M'
            // düz 'M' olarak çizilirdi — sessiz, çünkü bir şey yine görünürdü.
            assert!(
                !slots.contains(&slot),
                "{face:?} başka bir yüzün yuvasını paylaştı"
            );
            slots.push(slot);
        }
        assert_eq!(slots.len(), 4);
    }

    #[test]
    fn the_small_class_is_narrower_and_keeps_its_own_slot() {
        let mut a = atlas(POINT_SIZE, 1.0);

        // **Yuva ızgarası ortak**: küçük glyph büyük yuvaya, büyük hücrenin
        // taban çizgisine çiziliyor. Doku boyu, `slot_bytes` ve ızgara bu
        // yüzden hiç değişmiyor — bütün ucuzluk buradan geliyor.
        let big = slot_bytes_of(&mut a, Sprite::Char('M'), Face::Regular);
        let (small_slot, small_upload) = a.slot(Sprite::Char('M'), Face::Regular, SizeClass::Small);
        let small = small_upload
            .expect("yeni yuva yükleme vermeli")
            .bytes
            .to_vec();
        assert_eq!(big.len(), small.len(), "küçük sınıf yuva boyunu oynattı");

        // Ayrı yuva: anahtar boyutu taşımasaydı küçük 'M' büyük 'M' olarak
        // çizilirdi — sessiz, çünkü bir şey yine görünürdü.
        assert_ne!(small_slot, TOFU, "küçük sınıf tofu'ya düştü");
        assert_ne!(
            small_slot,
            a.slot(Sprite::Char('M'), Face::Regular, SizeClass::Normal)
                .0,
            "küçük sınıf düz yüzün yuvasını paylaştı"
        );

        // **Harf gerçekten küçük.** Ölçüt kapsamanın en sağdaki sütunu: küçük
        // yüz aynı yuvada daha dar bir iz bırakmalı. Tek tek piksel değeri
        // değil sınır sınanıyor — kapsama font sürümüne bağlı, iddia değil.
        let ink_right = |bytes: &[u8]| {
            let (w, h) = a.metrics().cell_wh();
            (0..w)
                .rev()
                .find(|&x| (0..h).any(|y| bytes[y * w + x] > 0))
                .map(|x| x + 1)
                .unwrap_or(0)
        };
        assert!(
            ink_right(&small) < ink_right(&big),
            "küçük sınıf dar değil: küçük {}, büyük {}",
            ink_right(&small),
            ink_right(&big)
        );

        // Kural sprite'ları **ölçüden bağımsız**: bağlam satırında kural yok
        // ve `slot` bunu yüzle birlikte normalize ediyor.
        assert_eq!(
            a.slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Small
            )
            .0,
            a.slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Normal
            )
            .0,
        );
    }

    #[test]
    fn bold_glyph_fits_regular_face_slot() {
        // Metrik yalnız düz yüzden geliyor (R1.3); kalın glyph aynı yuvaya
        // rasterize oluyor. Kırpma kabul edilmiş bir bedel, ama yuvanın
        // **taşmaması** sözleşme: `raster::draw` tamponun boyunu assert ediyor.
        let mut a = atlas(POINT_SIZE, 1.0);
        let bytes = slot_bytes_of(&mut a, Sprite::Char('M'), Face::Bold);
        assert_eq!(bytes.len(), a.metrics().slot_bytes());
        assert!(
            bytes.iter().any(|&b| b > 0),
            "kalın 'M' hiç mürekkep vermedi"
        );
    }

    #[test]
    fn rule_sprites_are_not_empty_and_differ() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let mut seen: Vec<(RuleKind, Vec<u8>)> = Vec::new();
        for kind in [
            RuleKind::Single,
            RuleKind::Double,
            RuleKind::Curl,
            RuleKind::Dotted,
            RuleKind::Dashed,
            RuleKind::Strike,
            RuleKind::Chevron,
        ] {
            let bytes = slot_bytes_of(&mut a, Sprite::Rule(kind), Face::Regular);
            assert!(bytes.iter().any(|&b| b > 0), "{kind:?} hiç piksel boyamadı");
            for (prev_kind, prev_bytes) in &seen {
                // Beş stilin **ayırt edildiği** buranın işi. Hepsini düz
                // çizgiye düşüren bir kod `kural=R` jetonundan geçerdi.
                assert_ne!(
                    prev_bytes, &bytes,
                    "{kind:?} ile {prev_kind:?} aynı çizildi"
                );
            }
            seen.push((kind, bytes));
        }
    }

    #[test]
    fn rule_keeps_one_slot_regardless_of_face() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let regular = a
            .slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Normal,
            )
            .0;
        // Çağıran yanılıp yüz verse bile normalizasyon aynı yuvaya götürür;
        // yoksa altı çeşit dört yüzle yirmi dört yuva harcardı.
        let bold = a
            .slot(
                Sprite::Rule(RuleKind::Single),
                Face::BoldItalic,
                SizeClass::Normal,
            )
            .0;
        assert_eq!(regular, bold, "kural yüze göre ayrı yuva tuttu");
    }

    #[test]
    fn the_chevron_points_right_and_sits_on_the_x_height() {
        // **İşaret terminalin kendisi, fontun değil** (012 phase-9): `>`
        // karakteri yerine yordamsal bir chevron. Üç iddia, üçü de sessizce
        // bozulabilir.
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (w, h) = (usize::from(m.cell_px.0), usize::from(m.cell_px.1));
        let bytes = slot_bytes_of(&mut a, Sprite::Rule(RuleKind::Chevron), Face::Regular);

        // Her satırın en sağdaki boyalı sütunu: chevron sağa açıldığı için bu
        // dizi ortaya doğru artıp sonra azalmalı — tepe noktası ortada.
        let rights: Vec<Option<usize>> = (0..h)
            .map(|y| (0..w).rev().find(|&x| bytes[y * w + x] > 0))
            .collect();
        let apex_row = rights
            .iter()
            .enumerate()
            .filter_map(|(y, right)| right.map(|x| (x, y)))
            .max()
            .expect("chevron hiç piksel boyamadı")
            .1;

        // **Dikey merkez üstü çizilinin merkezi**, yani x-height'ın ortası:
        // hücrenin geometrik merkezi taban çizgisinin altına düşer ve işaret
        // metne göre alçak görünürdü.
        let center = usize::from(m.strikeout_px.0) + usize::from(m.strikeout_px.1) / 2;
        assert!(
            apex_row.abs_diff(center) <= 1,
            "tepe x-height merkezinde değil: {apex_row} / {center}"
        );

        // **Ink hücrenin ortasına toplanıyor.** Izgarada işaret sol payın
        // içinde çiziliyor ve pay bir hücreden dar olabilir; taşsaydı komut
        // metninin ilk harfine binerdi.
        let painted: Vec<usize> = (0..w)
            .filter(|&x| (0..h).any(|y| bytes[y * w + x] > 0))
            .collect();
        let (left, right) = (painted[0], painted[painted.len() - 1]);
        assert!(left > 0, "chevron sol kenara yapıştı: {left}");
        assert!(right < w - 1, "chevron sağ kenara yapıştı: {right}");

        // Ve simetrik: `>` işaretinin iki kolu aynı.
        let above = (0..center).filter(|&y| rights[y].is_some()).count();
        let below = (center + 1..h).filter(|&y| rights[y].is_some()).count();
        assert!(
            above.abs_diff(below) <= 1,
            "kollar simetrik değil: {above} / {below}"
        );
    }

    #[test]
    fn curl_is_really_a_wave() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (w, h) = (usize::from(m.cell_px.0), usize::from(m.cell_px.1));
        let bytes = slot_bytes_of(&mut a, Sprite::Rule(RuleKind::Curl), Face::Regular);
        // Her sütunun en üstteki boyalı satırı; dalga bunları oynatmalı.
        let tops: Vec<usize> = (0..w)
            .filter_map(|x| (0..h).find(|&y| bytes[y * w + x] > 0))
            .collect();
        assert_eq!(tops.len(), w, "kıvrım bazı sütunları hiç boyamadı");
        let (min_top, max_top) = (
            *tops.iter().min().expect("sütun var"),
            *tops.iter().max().expect("sütun var"),
        );
        // Düz bir çizgide bu fark **sıfırdır**. Kıvrımı düz çizgiye düşüren
        // bir kod tam burada kırmızı düşer — ve `kural=R` jetonu onu göremez.
        assert!(
            max_top - min_top >= 1,
            "kıvrım salınmıyor: tepe satırı {min_top}..{max_top} arasında sabit"
        );
    }

    #[test]
    fn curl_is_continuous_across_cell_edges() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        let (w, h) = m.cell_wh();
        let bytes = slot_bytes_of(&mut a, Sprite::Rule(RuleKind::Curl), Face::Regular);
        let tops: Vec<usize> = (0..w)
            .filter_map(|x| (0..h).find(|&y| bytes[y * w + x] > 0))
            .collect();
        // Hücreye **tam** sayıda dalga sığıyorsa (R2.4) sinüs orta eksene göre
        // ayna simetriktir: `center(x) + center(w-1-x)` sabittir. Sığmıyorsa
        // faz hücre sınırında kırılır ve çok hücreli bir alt çizgi kesintili
        // görünür — sprite tek hücre genişliğinde ve komşularıyla döşeniyor.
        //
        // Kenar sütunlarının **eşit** olmasını beklemek yanlış olurdu: bir tam
        // periyotta ilk ve son sütun eşit değil, orta eksene göre AYNADIR.
        let total = tops[0] + tops[w - 1];
        for x in 0..w {
            let pair = tops[x] + tops[w - 1 - x];
            assert!(
                pair.abs_diff(total) <= 1,
                "dalga periyodu hücreyi tam bölmüyor: x={x} çifti {pair}, kenar çifti {total}"
            );
        }
    }

    #[test]
    fn envelope_stays_inside_cell() {
        // **Sentetik girdi bilerek**: bu makinedeki Menlo alt çizgiyi 14+1'e
        // koyuyor, hücre 17 — yani gerçek fontla kırpma dalı hiç ateşlenmiyor
        // ve oradan yazılan bir sınama mutasyonu yakalayamazdı.
        // `font::rule_envelope`'nın sözleşmesi burada, saf aritmetik olarak sınanıyor.
        for (top, thick, h) in [(100u16, 3u16, 17u16), (16, 4, 17), (0, 99, 17)] {
            let (position, thickness) = font::rule_envelope(top, thick, h);
            assert!(
                position + thickness <= h,
                "zarf hücreyi aştı: girdi ({top},{thick},{h}) → ({position},{thickness})"
            );
            assert!(
                thickness >= 1,
                "kalınlık sıfıra indi: çizilmeyen çizgi kural değildir"
            );
        }
    }

    #[test]
    fn rule_envelope_fits_cell_with_real_font() {
        for point_size in [POINT_SIZE, LARGE_POINT_SIZE] {
            let m = atlas(point_size, 2.0).metrics();
            let h = m.cell_px.1;
            assert!(
                m.underline_px.0 + m.underline_px.1 <= h,
                "alt çizgi {point_size}pt'de taştı"
            );
            assert!(
                m.strikeout_px.0 + m.strikeout_px.1 <= h,
                "üstü çizili {point_size}pt'de taştı"
            );
        }
    }
    #[test]
    fn missing_face_falls_back_to_regular() {
        // Monaco **tek yüzlü**: bu makinede Bold/Italic/BoldItalic üçü de
        // türetilemiyor. Zincirin tabanı (Menlo) dördünü de taşıdığı için geri
        // düşüş dalı ancak böyle bir aileyle ateşlenebiliyor — `Faces::derive`
        // ayrı bir kurucu olarak tam bunun için var.
        let (monaco, name) = font::open("Monaco", POINT_SIZE);
        assert_eq!(
            name, "Monaco",
            "Monaco makinede yok; sınamanın öncülü düştü"
        );
        let faces = font::Faces::derive(monaco);
        for face in [Face::Bold, Face::Italic, Face::BoldItalic] {
            // Yüz düz yüze çöküyor VE anahtar da çöküyor: yoksa aynı bitmap
            // dört yuva harcardı.
            assert_eq!(
                faces.effective(face),
                Face::Regular,
                "{face:?} anahtarı çökmedi"
            );
        }
        // Menlo'da çökme yok — sınamanın kendisi de ayrımı görebiliyor olmalı.
        let menlo = font::Faces::derive(font::open("Menlo", POINT_SIZE).0);
        assert_eq!(
            menlo.effective(Face::Bold),
            Face::Bold,
            "Menlo'nun kalın yüzü çöktü"
        );
    }

    #[test]
    fn pattern_period_divides_cell_evenly() {
        // Nokta/kesik deseni `x % period` ile döşeniyor ve sprite tek hücre
        // genişliğinde: periyot hücreyi tam bölmezse iki komşu hücrede tire
        // uzunlukları farklı görünür. Kıvrımda bu kısıt `WAVE_COUNT` ile
        // inşaen sağlanıyordu, `band`'ta sağlanmıyordu — gözden kaçmıştı.
        for w in 1..=40usize {
            for wanted in 1..=40usize {
                let p = raster::dividing_period(wanted, w);
                assert!(p >= 1, "periyot sıfır olamaz (w={w}, istenen={wanted})");
                assert_eq!(w % p, 0, "periyot {p} hücreyi ({w}) bölmüyor");
            }
        }
    }
}
