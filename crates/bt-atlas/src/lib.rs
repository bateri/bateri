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
//! [`FontIssue`] çağırana döner, bu crate kimseye bir şey basmaz.
//!
//! **Yordamsal çizilen ikinci küme karakterlerdir** ve fonta hiç sorulmadan
//! kazanırlar (`raster::is_procedural`): blok elemanları (U+2580–U+259F),
//! Braille (U+2800–U+28FF), çizgi çizim (U+2500–U+257F, **köşegenler
//! `╱╲╳` hariç**) ve terminalin grafik kümesi (U+23B8–U+23BF: iki dikey
//! kenar çizgisi, dört tarama satırı, iki köşe — `⎷` U+23B7 dışarıda).
//! Gerekçe döşeme — fontun em kutusu hücre kutusu değil ve
//! Menlo'nun `█`'i hücreyi doldurmuyor, alt alta iki blok arasında şerit
//! kalıyor. Son aile bir kusuru da kapatıyor: `⎿` (Claude Code araç
//! sonuçlarının işareti) cascade'den **hücreye sığmayan** bir glyph'le
//! geliyordu ve kutu çıkıyordu. Yüzden bağımsızlar (dört yüz tek yuva; ince/kalın ayrımı zaten
//! karakterin kendisinde), ama **yalnız büyük sınıfta**: dock'un bağlam
//! satırında sütun adımı küçük yüzün ilerlemesi ve büyük hücre genişliğinde
//! bir sprite orada komşusunun üstüne binerdi.
//!
//! Seçili fontta olmayan **tek hücrelik** karakter sistemin cascade'inden
//! geliyor (`font::fallback_font`) ve kapı **geometrik**: adayın
//! **boyayacağı piksel** hücrenin dışına taşıyorsa reddediliyor. Ölçülen şey
//! ilerleme değil mürekkep, çünkü sembol fontlarının glyph'leri
//! ilerlemelerinden dar boyuyor (`⏺` U+23FA) ve ilerlemeyi ölçen bir kapı
//! onları hücreye sığdıkları hâlde eliyordu. Emoji, CJK ve geniş glyph hâlâ
//! [`TOFU`] — onları gerçekten çizmek (iki hücre, renkli doku) ayrı bir sete
//! kaldı.

mod font;
mod raster;

use std::collections::HashMap;

use font::Faces;
pub use font::{Face, FontIssue, Metrics, SizeClass, family_issue, monospaced_families};
use objc2_core_foundation::{CFRetained, CGFloat};
use objc2_core_text::CTFont;
use raster::DrawResult;
pub use raster::RuleKind;

/// Atlasta yuva tutan şey: bir karakter, bir kural çizgisi ya da bir grapheme
/// dizisi.
///
/// Üçü aynı ızgarada yaşıyor çünkü üçü de **hücre boyunda** yuvalara
/// rasterize oluyor: emoji ve geniş glyph 023'ten beri iki yarıya ([`Half`])
/// ve renk düzlemine ([`Plane`]) bölünerek aynı birliğe girdi, yani ayrı bir
/// doku ya da ayrı bir paketleyici doğmadı.
///
/// [`Sprite::Cluster`] bir dizginin (bayrak `🇹🇷`, ZWJ `👨‍👩‍👧`, ten rengi
/// `👍🏽`, VS16 `❤️`) **atlasın kendi** interner'ındaki kimliği
/// ([`Atlas::intern`]); `Sprite` o sayede `Copy + Hash` kalıyor ve yuva
/// anahtarı bir dizgi taşımıyor. Dizi tek glyph'e şekillenmezse cevabı taban
/// karakterinki ([`Atlas::slot`]).
///
/// Yordamsal çizilen karakterler (blok, Braille) **üçüncü bir varyant
/// almadı**: bir karakterdirler ve `Char` olarak yaşıyorlar. `Sprite::Box`
/// açmak `bt-gpu`'nun bugünkü tek satırını "bu karakter hangi sprite"
/// sorusuna çevirir, yani renderer'a terminal semantiği sızdırırdı; kapı
/// bu yüzden [`Atlas::slot`]'un içinde.
// `repr(u8)`: bkz. `RuleKind`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sprite {
    Char(char),
    Rule(RuleKind),
    Cluster(u32),
}

/// Bir glyph'in hücre ızgarasındaki **yarısı** — yuva anahtarının dördüncü
/// ekseni ve [`SizeClass`]'ın kardeşi.
///
/// Geniş karakter iki hücre boyunda bir kutuya ortalanıp **iki** yuvaya
/// rasterize ediliyor; her yuva yine tam bir hücre, yani doku düzeni,
/// `slot_bytes` ve ızgara aritmetiği hiç değişmiyor. Dörtlü de tek hücre
/// kalıyor: `bt-gpu` iki instance basıyor ve `GlyphInstance`'ın 32 baytlık
/// stride'ı ile `cell_px` uniform'u el değmiyor.
///
/// Eksen [`Sprite`]'a **varyant olarak eklenmedi** ve gerekçe o tipin
/// doc'unda yazılı: `Sprite`'a eklenen bir kol renderer'a "bu karakter hangi
/// sprite" sorusunu sızdırır. Buradaki eksen ise çağıranın **taşıdığı** bir
/// istek — `Face` ile `SizeClass` gibi — ve `Atlas::slot` onu normalize
/// ediyor.
///
/// [`Half::Whole`] "tek hücre" demek ve **geniş karakterlerde de doğabiliyor**:
/// mürekkebi bir hücreye sığan geniş ilan edilmiş karakter (`☕`, fullwidth
/// `！`) tek yuvadan çiziliyor. Kararı kapı veriyor ([`font::fallback_font`]),
/// çağıran değil.
// `repr(u8)`: bkz. `RuleKind`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Half {
    Whole,
    Left,
    Right,
}

/// Atlasın **hangi düzlemi** — maske mi renk mi.
///
/// İki düzlem tek [`Atlas`]'ın içinde ve bu bilinçli: ikinci bir `Atlas`
/// beş CoreText türetmesini (dört yüz + küçük yüz) ve aynı anahtardan
/// **ikinci bir [`Metrics`]**'i doğururdu. `bt-gpu`'nun `sync_atlas`'ı tam
/// bunu önlemek için var ("ikinci bir çağrıda alınsaydı araya düşen bir
/// `ensure` ikisini ayrı atlaslardan verirdi") ve [`Atlas::context_cell_w`]'in
/// doc'u aynı kokuyu adıyla yazıyor.
///
/// Yuvalar **iki düzlemde de hücre boyunda**, yani [`Half`] mekanizması geniş
/// emojinin geometrisini de çözüyor ve ızgara aritmetiği
/// ([`Atlas::slot_origin`], [`Atlas::capacity`]) ikisi için ortak. Ayrışan tek
/// şey piksel formatı: maske `R8`, renk `RGBA8` — ve her düzlemin **kendi
/// monoton sayacı** var, çünkü uv `bt-gpu`'nun `prepare`'inde çözüm anında
/// pişiyor ve kare ortasında anlamı değişen paylaşımlı bir sayaç önceki
/// geçişlerin uv'lerini geçersizleştirirdi.
// `repr(u8)`: bkz. `RuleKind`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Plane {
    Mask,
    Color,
}

/// [`Atlas::slot`]'un cevabı: yuva **ve** hangi yarının kullanıldığı.
///
/// İkinci alan bir kolaylık değil zorunluluk: "bir yuva mı iki mi" kararını
/// mürekkep kapısı veriyor, yani ancak burada biliniyor — çağıran (`bt-gpu`)
/// ise ikinci instance'ı basıp basmayacağına karar vermek zorunda. Alan
/// olmasaydı `☕`'nin sağına boş bir dörtlü düşerdi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub slot: u16,
    /// Yuvanın yaşadığı düzlem; çağıran dokuyu ve pipeline'ı buna göre
    /// seçiyor. Tofu **her zaman** [`Plane::Mask`]: kutu bir maske.
    pub plane: Plane,
    /// Kapının kabul ettiği kutu: [`Half::Whole`] tek hücre, [`Half::Left`]
    /// iki hücrenin solu. [`Half::Right`] yalnız `Left` dönmüş bir karakter
    /// için sorulur.
    pub half: Half,
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

/// Atlasın hedeflediği yuva sayısı — dokunun kenarı **bundan** türüyor.
///
/// **Ölçüm iddiası değil, bir tasarım sabiti** (`GUTTER_PT` ve
/// [`CONTEXT_SCALE`] emsali) ama türetmesi ölçülmüş bir sayıdan: yordamsal
/// aile **421** karakter (`docs/OLCUMLER.md` → Atlas yuva ayak izi) ve
/// atlastan istediği yuva **429** — tofu (1) ile karakterlere kapalı kural
/// payı ([`RULE_RESERVE`], 7) üstüne biniyor, çünkü [`Atlas::slot`]
/// karakterlere `capacity() - RULE_RESERVE` veriyor ve `next` 1'den
/// başlıyor. Kural "ailenin payı atlasın yarısını geçmesin", yani
/// `2 × 429 = 858`, yukarı yuvarlanmış **1024**.
///
/// Sabit **düşük riskli** ve bu onu dürüstçe bir tasarım sabiti yapıyor:
/// `(450, 1624]` aralığındaki *her* değer 13pt, 28pt ve 29pt@2x'te aynı
/// davranışı veriyor — 13pt zaten 1984 yuvayla tabanda kalıyor, 28 ile 29pt
/// ise ikisi de bir kez katlanıyor.
///
/// Bu sayı bir **taban**, bir tavan değil: hücre küçükse kapasite hedefi
/// katbekat aşar (13pt@2x → 1984) ve kimse kırpmaz.
const SLOT_TARGET: u32 = 1024;

/// Doku kenarının tabanı, piksel — **bugünkü davranışın koruma sözü**.
///
/// Varsayılan punto bu kenarda kalıyor ([`SLOT_TARGET`]'ı zaten aşıyor), yani
/// ızgara, `texture_px()` ve raster bit bit değişmiyor. Düşürülürse o söz
/// bozulur: varsayılan puntonun dokusu küçülür ve yuva sayısı düşer.
const MIN_EDGE: u16 = 1024;

/// Doku kenarının tavanı, piksel.
///
/// Tavan olmadan büyüme [`MAX_POINT_SIZE`] × `MAX_LINE_HEIGHT` köşesinde
/// sınırsız sürerdi. 4096 Metal'in doku sınırının (16384) katbekat altında ve
/// o köşede bile kapasiteyi ailenin üstünde tutuyor — sayısı
/// `capacity_clears_the_family_at_every_accepted_size`'da **hesaplanıyor**,
/// buraya yazılmıyor.
const MAX_EDGE: u16 = 4096;

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
    /// Geniş glyph'in **sağ** yarısı — aynı dönüşte, aynı `&mut` ödüncünden.
    ///
    /// İki yarı **atomik**: aynı çağrıda iki yuva ayrılıyor, ikisi de bu
    /// dönüşle yükleniyor ve sağ yarı için ikinci bir `slot()` turu
    /// beklenmiyor. Ayrı turlara bölünseydi kapasite sınırı ikisinin
    /// **arasına** düşebilirdi — sol yuva açılır, sağ tofu'ya düşer ve ekranda
    /// yarım glyph + yarım kutu belirirdi. Bu tipin tek dönüşü o hâli
    /// **temsil edemiyor**: ya iki yarı birden gelir ya hiçbiri.
    pub right: Option<(u16, u16)>,
    /// Sağ yarının baytları; [`Upload::right`] `Some` ise anlamlı.
    pub right_bytes: &'a [u8],
    /// Baytların hangi düzleme yazılacağı — formatı ve satır adımını o
    /// belirliyor. `bt-gpu` `bytesPerRow`'u buradan türetmek zorunda:
    /// ayrışırsa Metal kısa tamponun ötesini okur ve belirti sessizdir.
    pub plane: Plane,
}

/// Sabit yuva ızgarasında yaşayan glyph atlası.
///
/// Paketleyici yok: bu sette **tüm sprite'lar hücre boyutunda** (emoji ve
/// geniş glyph kapsam dışı), yani `yuva_no → piksel köşe` dönüşümü
/// aritmetiktir. Yordamsal karakterler (blok, Braille, çizgi) o kısıtı
/// bozmuyor — tanımları gereği tam bir hücre; üstelik kısıtı asıl talep eden
/// onlar, çünkü döşemeleri hücrenin kenarında sürüyor.
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
    /// Hücrenin **kesirli** ilerlemesi, fiziksel piksel — büyük sınıf.
    ///
    /// [`Metrics::cell_px`]'in genişliği bunun yukarı yuvarlanmışı ve
    /// ızgaranın adımı o; kesirli hâli burada duruyor çünkü iki tüketici
    /// yuvarlanmışla çalışamıyor — yedek adayın mürekkep kapısı
    /// (`font::fallback_font`) ile glyph'in hücrede ortalanması
    /// (`raster::draw`). Gerekçenin tamamı `font::space_advance`'in doc'unda;
    /// iki sayının aynı ölçüyü verdiğinin bekçisi
    /// `the_cell_is_the_rounded_advance`.
    cell_advance: CGFloat,
    /// Küçük yüzün kesirli ilerlemesi: [`Atlas::cell_advance`]'in küçük sınıf
    /// ikizi. Yedek kapısı ve ortalama **sınıf başına** ayrı, çünkü ikisinin
    /// de sınırı o sınıfın kendi hücresi.
    context_advance: CGFloat,
    /// Küçük yüzün ilerleme genişliği, piksel: bağlam satırının sütun adımı.
    ///
    /// **Yalnız genişlik**, çünkü küçük glyph de büyük yuvaya, büyük hücrenin
    /// taban çizgisine rasterize ediliyor ([`Atlas::slot`]): yükseklik ve
    /// taban ortak, ayrışan tek şey harflerin arasındaki mesafe.
    ///
    /// [`Atlas::context_advance`]'in yuvarlanmışı ve **ondan türüyor**: iki
    /// ayrı yoldan hesaplanırsa (biri `font::metrics`, öteki `space_advance`)
    /// aynı ölçünün iki kaynağı olur.
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
    slots: HashMap<(Sprite, Face, SizeClass, Half), (u16, Plane)>,
    /// Bir sonraki boş yuva; [`TOFU`] ayrılmış olduğu için 1'den başlar.
    /// `slots.len()`'den türetilemez: tofu'ya çözümlenen kayıtlar yuva
    /// harcamıyor, yani iki sayı bilerek ayrışıyor.
    next: u16,
    /// Tek yuvalık çizim tamponu. Alan olması kare başına yeniden ayırmayı
    /// önlüyor; içeriği her yeni glyph'te üzerine yazılır.
    buffer: Vec<u8>,
    /// Geniş glyph'in sağ yarısının tamponu — [`Atlas::buffer`]'ın ikizi.
    ///
    /// İkinci bir tampon, tek tamponu iki kez kullanmaktan **ucuz ve
    /// doğru**: iki yarı aynı `Upload`'la dönüyor (atomiklik), yani ikisinin
    /// baytları aynı anda canlı olmak zorunda. Boyu tam bir yuva, yani
    /// varsayılan hücrede yüzlerce bayt.
    buffer_right: Vec<u8>,
    /// Renk düzleminin yuva sayacı — maskenin [`Atlas::next`]'inden **ayrı**.
    ///
    /// Ayrı olmasının gerekçesi [`Plane`]'in doc'unda: uv çözüm anında
    /// pişiyor. Yan kazanç **kapasite**: emoji yuvaları maskelerin havuzuna
    /// binmiyor ve tersi, yani emoji-ağır bir oturum harflerin yuvasını
    /// yemiyor.
    ///
    /// **Ayıran şey kapasite, kapı değil.** Çizimden önceki kapasite kapısı
    /// maskenin sayacına bakıyor (gerekçesi [`Atlas::slot`]'ta, üç seçenek
    /// tartışılarak), yani **dolu bir maske atlası emojiyi de reddediyor**.
    /// Tersi olmuyor: dolu bir renk düzlemi harfleri etkilemiyor.
    ///
    /// **Tofu payı yok**: renk düzleminde tofu doğmuyor (kutu bir maske), yani
    /// sayaç 0'dan başlıyor ve `capacity()`'nin tamamı emojiye açık.
    color_next: u16,
    /// Renkli yuvanın tamponu ve ikizi; [`Metrics::slot_bytes_rgba`] boyunda.
    ///
    /// Maskenin tamponundan **ayrı**: paylaşılan bir tampon iki formatı aynı
    /// diziye sığdırmayı, yani `raster::draw`'un ön koşul assert'ini
    /// gevşetmeyi isterdi — o assert `unsafe` bloğun ön koşulu ve yanlış
    /// düzlemin tamponunu yakalayan tek şey.
    color_buffer: Vec<u8>,
    color_buffer_right: Vec<u8>,
    /// Rezident tofu kutusu; ömür boyu değişmez.
    tofu: Vec<u8>,
    /// Interner'ın dizgileri: [`Sprite::Cluster`]'ın kimliği bu listenin
    /// indeksi.
    ///
    /// Tahliye yok ve politika **yuvalarınkiyle aynı**: kayıt atlasın ömrü
    /// boyunca yaşıyor, [`Atlas::ensure`] atlası yeniden kurunca yuvalarla
    /// birlikte düşüyor. Ayrı ömürlü olsaydı yeniden kurulmuş bir atlasta
    /// kimliği canlı ama yuvası ölü diziler kalırdı; yuvalarla aynı anda
    /// düşünce çağıranın elindeki eski kimlik ya yeniden sorulur ya da
    /// [`Atlas::slot`]'ta tofu'ya düşer.
    clusters: Vec<Box<str>>,
    /// Dizgi → kimlik; [`Atlas::clusters`]'ın ters yönü.
    cluster_ids: HashMap<Box<str>, u32>,
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
        let cell_advance = font::space_advance(faces.get(Face::Regular));
        // Küçük yüz **aynı zincirden**: `font_issue` ikinci kez sorulmuyor ve
        // yok sayılıyor, çünkü aynı aileye aynı cevap gelir — ikinci bir kayıt
        // kullanıcıya aynı uyarıyı iki kez söyletirdi.
        let (small, _) = font::open_chain(
            family,
            effective_point_size(point_size * CONTEXT_SCALE, scale),
        );
        // `line_height` **sorulmuyor**: satır aralığı yalnız hücrenin boyunu
        // büyütüyor ve o boy iki sınıfta ortak, genişlik ise fontun kendi
        // ilerlemesi. Bütün bir `Metrics` kurup içinden genişliği almak aynı
        // sayıyı ikinci bir yoldan türetmek olurdu.
        let context_advance = font::space_advance(&small);
        let context_cell_w = font::round_up(context_advance);
        let (w, h) = metrics.cell_px;
        // Kenar **yuva hedefinden** türüyor: hücre büyüdükçe kapasite düşüyor
        // ve bir yerde yordamsal ailenin (422 yuva) altına iniyor — ölçülen
        // kırılma Retina'da 29pt (`docs/OLCUMLER.md`). Taban [`MIN_EDGE`],
        // yani varsayılan punto bugünkü dokusunda kalıyor.
        //
        // `u32`'de sayılıyor: bölümlerin çarpımı küçük hücrede `u16`'yı aşar
        // (13pt@1x, 4096 kenar → 116 224). `grid` yine `u16`.
        //
        // **`capacity()`'nin `u16::MAX` kırpmasına giden yol yok** ve sebebi
        // döngünün kendisi: katlama yalnız kapasite hedefin **altındayken**
        // koşuyor ve her katlama kapasiteyi dörtle çarpıyor, yani büyümenin
        // ürettiği kapasite her zaman `4 × SLOT_TARGET`in (4096) altında.
        // Kırpma ancak hiç katlanmamış bir tabanda görülebilir ve orası
        // zaten bugünkü davranış.
        //
        // `w`/`h` en az 1 (`font::round_up`), yani bölme güvenli; `max(1)` de
        // hücrenin dokudan büyük olduğu uç için.
        let grid = grid_for(w, h);
        Self {
            faces,
            small,
            metrics,
            cell_advance,
            context_advance,
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
            buffer_right: vec![0u8; metrics.slot_bytes()],
            color_next: 0,
            color_buffer: vec![0u8; metrics.slot_bytes_rgba()],
            color_buffer_right: vec![0u8; metrics.slot_bytes_rgba()],
            tofu: tofu_buffer(metrics),
            clusters: Vec::new(),
            cluster_ids: HashMap::new(),
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
    /// Türetilmiş kenar değil **tam ızgara**: kenardaki artık şerit hiçbir
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

    /// Dizginin sprite'ı: aynı dizgi her zaman aynı kimliği alır.
    ///
    /// **Tek kod noktalı dizgi `Char`'a iniyor** — kümenin yolu yalnız birden
    /// çok kod noktasına açık ve tek karakteri `Cluster` olarak tutmak aynı
    /// glyph'i iki anahtarda, iki yuvada rasterize ederdi. Boş dizgi
    /// çizilecek bir şey taşımıyor; boşluğa iniyor.
    pub fn intern(&mut self, text: &str) -> Sprite {
        let mut chars = text.chars();
        let base = match (chars.next(), chars.next()) {
            (None, _) => return Sprite::Char(' '),
            (Some(ch), None) => return Sprite::Char(ch),
            (Some(ch), Some(_)) => ch,
        };
        if let Some(&id) = self.cluster_ids.get(text) {
            return Sprite::Cluster(id);
        }
        // Tablo **tavanlı** ve tavanı negatif önbelleğinki: tahliye yok ve
        // her farklı dizgi atlasın ömrü boyunca yaşıyor, yani tavansız bir
        // interner rastgele çıktının (`cat` edilmiş ikili veri, birleştirici
        // taşıyan geniş hücreler) belleğini hiç geri vermezdi. Tavanın
        // ötesindeki yeni dizi **taban karakterine** iniyor — şekillenmeyen
        // kümenin cevabı da o, yani görüntü 035 öncesinden kötü olmuyor; o
        // kadar farklı diziyi zaten atlasın yuvaları da tutamazdı.
        if self.clusters.len() >= self.negative_cache_cap() {
            return Sprite::Char(base);
        }
        let Ok(id) = u32::try_from(self.clusters.len()) else {
            return Sprite::Char(base);
        };
        self.clusters.push(text.into());
        self.cluster_ids.insert(text.into(), id);
        Sprite::Cluster(id)
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
        want: Half,
    ) -> (Placed, Option<Upload<'_>>) {
        // Anahtar **istenen** yüzü değil **çizilen** yüzü taşır. Üç ayrı
        // sebeple ayrışabiliyorlar ve üçü de aynı cümlenin yüzü:
        //   - kural çizgileri yüzden bağımsız (kalın metnin altındaki çizgi
        //     kalın değildir) ve ölçüden de: dock'un bağlam satırında kural
        //     yok, yani küçük bir kural sprite'ı hiç doğmaz,
        //   - fontta olmayan yüz düz yüze çökmüştür (`Faces::effective`),
        //   - küçük sınıfta yalnız düz yüz var ([`Atlas::small`]),
        //   - yordamsal çizilen karakter de kural gibi yüzden bağımsız
        //     ([`raster::is_procedural`]).
        // Normalizasyon **burada**, çağıranın disiplininde değil: ayrışan bir
        // anahtar bayt bayt aynı bitmap'i ayrı yuvalarda tutar, atlas kat kat
        // hızlı dolar ve belirti sessizdir.
        // `want` de normalize ediliyor ve iki yerde zorla `Whole`'a iniyor:
        // kural sprite'ları (yüzden ve ölçüden bağımsız, hep tek hücre) ve
        // **küçük sınıf**. İkincisinin gerekçesi 021'in yordamsal kapısıyla
        // aynı: dock'un bağlam satırının sütun adımı küçük yüzün ilerlemesi,
        // oysa kutu büyük hücreden türüyor — iki hücrelik bir glyph orada
        // komşusunun üstüne binerdi. Dock'un giriş satırı `Normal` ama oraya
        // `wide` hiç gelmiyor (`bt_core::dock`'un değişmezi), yani bu kol
        // yalnız bağlam satırını kapatıyor.
        let want = match (sprite, size) {
            (Sprite::Rule(_), _) | (_, SizeClass::Small) => Half::Whole,
            _ => want,
        };
        let (face, size) = match (sprite, size) {
            (Sprite::Rule(_), _) => (Face::Regular, SizeClass::Normal),
            (Sprite::Char(_), SizeClass::Small) => (Face::Regular, SizeClass::Small),
            // Unicode ince/kalın ayrımını **karakterin kendisinde** taşıyor
            // (`─` U+2500 ince, `━` U+2501 kalın), yani SGR bold'un çizgiyi
            // kalınlaştırması bilginin iki kez kodlanması olurdu. Yan kazanç:
            // dört yüz tek yuvayı paylaşıyor ve kalın bir TUI çerçevesi
            // atlasa dört kat değil bir kat biniyor.
            //
            // Desen `SizeClass::Normal`, `_` **değil**: `_` yazılsaydı küçük
            // istek de `Normal`'e zorlanır ve aşağıdaki `size == Normal`
            // guard'ı tam da kapatılmak istenen yerde açılırdı. Küçük sınıfta
            // kapının kapalı olmasının gerekçesi döşeme değil **ölçü
            // ayrışması**: `Metrics` büyük hücrenin, yani yordamsal sprite
            // büyük hücre genişliğinde çizilir, dock'un bağlam satırının
            // sütun adımı ise küçük yüzün ilerlemesi (`Frame::column_px`) —
            // hücreyi tam dolduran bir sprite orada komşusunun üstüne binerdi.
            (Sprite::Char(ch), SizeClass::Normal) if raster::is_procedural(ch) => {
                (Face::Regular, SizeClass::Normal)
            }
            (Sprite::Char(_), SizeClass::Normal) => (self.faces.effective(face), SizeClass::Normal),
            // Dizinin yüzü **düz**: şekillenen glyph renkli emoji fontundan
            // geliyor ve orada kalın/eğik yok, yani dört yüz dört ayrı yuvada
            // bayt bayt aynı bitmap'i tutardı. Şekillenmeyen dizinin taban
            // karakteri de düz yüzden soruluyor (aynı anahtarın takma adı).
            // Boy sınıfı korunuyor: küçük satırın glyph'i küçük fontun.
            (Sprite::Cluster(_), size) => (Face::Regular, size),
        };
        // Anahtar **istenen** yarıyı taşıyor ama cevabın yarısı istenenle
        // aynı olmak zorunda değil: `Left` istenip tek hücreye sığan bir
        // karakter `Whole` anahtarına yazılıyor, yani ikinci soruluşunda da
        // aynı yuvayı ve aynı cevabı veriyor. Kapı bu yüzden anahtar başına
        // atlasın ömründe bir kez koşuyor.
        let key = (sprite, face, size, want);
        if let Some(&(slot, plane)) = self.slots.get(&key) {
            return (
                Placed {
                    slot,
                    half: want,
                    plane,
                },
                None,
            );
        }
        // `Left` istendi ama karakter daha önce **tek hücrelik kabul**
        // edilmişse cevabı o veriyor. Bu dal olmasaydı `☕` iki kez rasterize
        // edilir, iki yuva harcar ve sağ yarısı boş kalırdı.
        //
        // **`TOFU` bu daldan geçmiyor ve kapı zorunlu.** Tek hücrelik bir
        // **ret** iki hücrelik isteğin cevabı **değil**: `Whole` isteği
        // `cols = 1` ile eleniyor ve o ölçüt iki hücrelikten kesin olarak
        // daha sıkı, yani çıkarım tek yönlü — `Left` reddedildiyse `Whole` da
        // reddedilir, tersi değil. Kapı olmadan yol şöyle ölüyordu: dock giriş
        // satırını **her zaman** `wide: false` ile soruyor
        // (`bt_core::dock`'un değişmezi) ve dock'un satırı `SizeClass::Normal`,
        // yani prompt'a yazılan bir CJK karakteri önce `Whole` olarak
        // sorulup negatif önbelleğe giriyor; Enter'dan sonra aynı karakter
        // ızgaraya `wide: true` ile geliyor, `Left` anahtarını bulamıyor,
        // buradan `TOFU` alıyor ve setin tamamı o karakter için atlasın ömrü
        // boyunca **ölü** kalıyordu.
        //
        // Ret **kaydın tamamıyla** tanınıyor, yuva numarasıyla değil: renk
        // düzleminin 0. yuvası ilk emojinin gerçek yuvası ve numaraya bakan
        // bir kapı onun tek hücrelik kabulünü ret sanıp ikinci kez
        // rasterize ederdi (`cluster_as_base`'in ve negatif önbellek
        // süzgecinin ikizi).
        if want == Half::Left {
            let whole = (sprite, face, size, Half::Whole);
            if let Some(&(slot, plane)) = self.slots.get(&whole)
                && (slot, plane) != (TOFU, Plane::Mask)
            {
                return (
                    Placed {
                        slot,
                        half: Half::Whole,
                        plane,
                    },
                    None,
                );
            }
        }
        // Kural sprite'larına **pay ayrılıyor**: altısı da yordamsal,
        // deterministik ve ömür boyu gerekli. Pay olmasaydı, birkaç bin farklı
        // glyph gördükten sonra (CJK metin, simge-ağır TUI) ızgara dolar ve o
        // andan itibaren altı çizili **her** hücrenin altında çizgi yerine
        // tofu kutusu belirirdi. Karakterler son `RULE_RESERVE` yuvayı yiyemez;
        // kurallar tembel kalır ama yerleri garantidir.
        let cap = match sprite {
            Sprite::Rule(_) => self.capacity(),
            Sprite::Char(_) | Sprite::Cluster(_) => self.capacity().saturating_sub(RULE_RESERVE),
        };
        // **Geniş istek iki yuva ister ve ikisini birden ister.** Sayı
        // `want`'tan geliyor, kapıdan değil: kapı ancak çizim sırasında
        // koşuyor ve o zamana kadar tahsis kararı verilmiş olmak zorunda.
        // Fazladan istemek güvenli yönde yanlış — tek hücreye sığan bir geniş
        // karakter bir yuva harcıyor, sınırın bir yuva berisinde de reddedilse
        // bir sonraki `ensure`'da yeri var. Ters yönde yanlış olsaydı sol yarı
        // açılır sağ yarı tofu'ya düşerdi.
        //
        // `Half::Right` buraya **hiç ulaşmıyor**: çifti kabul eden çağrı iki
        // anahtarı birden yazıyor, yani sağ yarı yukarıdaki önbellek
        // turundan dönüyor. Dolu atlasta ise önbelleğe yazılmadığı için
        // buraya düşer ve tofu alır — sol yarısı da aynı sayıdan tofu
        // aldığı için cevap tutarlı kalıyor.
        let need = u32::from(if want == Half::Left { 2u16 } else { 1 });
        // Ölçüt **maskenin** sayacı ve bu bilinçli bir daraltma. Düzlem ancak
        // çizim sırasında biliniyor, yani düzleme duyarlı bir ön kapı yok.
        // Üç seçenek tartıldı (ölçülmedi — hiçbirinin sayısı alınmadı, ayıran
        // şey ilk ikisinin **yapısal** kusuru):
        //
        //   - `min(next, color_next)`: kapı **hiç kapanmıyor**, çünkü
        //     `color_next` emoji görmeyen bir oturumda ömür boyu 0 —
        //     yani dolu atlasta her önbelleklenmemiş glyph kare başına bir
        //     `CGBitmapContext` + `draw_glyphs`, taban fontta olmayan
        //     karakterde üstüne bir cascade yürüyüşü ödüyordu. Ana thread'de.
        //   - `max(..)`: dolu renk düzlemi **harfleri** tofu'ya düşürürdü.
        //   - maskenin sayacı (bu): dolu maske atlası emojiyi de reddediyor.
        //
        // Üçüncüsü seçildi ve bedeli [`Atlas::color_next`]'in sözünü
        // **daraltıyor**: ayrı sayaç *kapasiteyi* ayırıyor (emoji maskenin
        // yuvalarını yemiyor, maske de emojininkileri) ama *kapıyı*
        // ayırmıyor. Kesin ölçüt tahsisten hemen önce, düzlem bilindiğinde
        // soruluyor.
        if u32::from(self.next) + need > u32::from(cap) {
            // Dolu atlas **önbelleklenmez**: bu, fontun kalıcı bir gerçeği
            // değil atlasın geçici hâli. Kapasite hücre ölçüsünden türüyor
            // ([`SLOT_TARGET`]), yani aynı karakter başka bir puntoda yuva
            // bulabilir ve buraya yazılacak kayıt onu tofu'ya çivilerdi.
            //
            // Buraya düşmek **tek karede hedeften fazla farklı glyph**
            // demek ve o senaryo **ölçülmedi** (022). Ölçülürse çaresi LRU
            // değil `encode_pass` sınırında geri dönüşüm: yuva numarası kare
            // verisinde saklanmıyor, `slot_uv` uv'yi çözüm anında pişiriyor
            // ve `prepare` kare başına dört kez koşuyor, yani kare
            // **ortasında** yapılan her yeniden kullanım önceki geçişlerin
            // uv'lerini geçersizleştirir.
            return (
                Placed {
                    slot: TOFU,
                    half: Half::Whole,
                    plane: Plane::Mask,
                },
                None,
            );
        }
        // Ödünç match'in scrutinee'sinde bırakılmıyor: `&mut self.buffer`
        // orada kalsaydı kolların içinde `&self.buffer` alınamazdı.
        // Kutunun ilerlemesi: `Whole` bir hücre, `Left` iki. `Right` buraya
        // ulaşmıyor (yukarıda).
        let result = match sprite {
            // **Yordamsal çizim fonttan önce.** Sıra zorunlu ve "fontta
            // yoksa yordamsal çiz" yanlış kol olurdu: `█` Menlo'da *var* ama
            // hücreyi doldurmuyor, yani o karakter yedeğe hiç gitmeden
            // bozuk geliyor. `⠋` ise Menlo'da yok ve yedek koşarsa Apple
            // Braille gelip genişlik kapısından döner. İkisini de kapatan
            // tek yer burası — ve kol aşağıdaki font kolunun **üstünde**
            // olduğu için `raster::draw` font yolu olarak saf kalıyor,
            // `DrawResult`'ın doc'u ("fontun cevabı") gerilime girmiyor.
            //
            // `size` **normalize edilmiş** olan: küçük sınıf yukarıdaki
            // kolda `Small` kalıyor, yani guard onu eliyor ve bağlam satırı
            // kutu karakterini fonttan almaya devam ediyor.
            // Yordamsal aile **tanımı gereği tek hücre**: blok elemanları,
            // Braille, çizgi çizim ve teknik küme baştan sona tek sütunlu
            // (ölçüldü, 023 envanteri). `Whole` bu yüzden bir varsayım değil,
            // ailenin kendi özelliği.
            Sprite::Char(ch) if size == SizeClass::Normal && raster::is_procedural(ch) => {
                raster::draw_procedural(ch, self.metrics, &mut self.buffer);
                (DrawResult::Drawn, Half::Whole, Plane::Mask)
            }
            Sprite::Char(ch) => {
                // **Metrik her iki sınıfta da büyük hücrenin**: küçük glyph
                // büyük yuvaya, büyük hücrenin taban çizgisine çiziliyor
                // (`raster::draw` glyph'i `(0, baseline)`'a koyuyor). Yuva
                // boyu ortak kaldığı için ızgara, doku ve `slot_bytes`
                // değişmiyor — ayrışan tek şey harfin kendi boyu, o da
                // fonttan geliyor.
                // Yedek kapısının sınırı da sınıf başına: küçük glyph küçük
                // hücrenin ilerlemesine sığmak zorunda, büyüğün değil.
                let (font, cell_advance) = match size {
                    SizeClass::Normal => (self.faces.get(face), self.cell_advance),
                    SizeClass::Small => (&*self.small, self.context_advance),
                };
                // **Taban font her zaman tek hücre.** Eşaralıklı taban fontta
                // her glyph'in ilerlemesi hücrenin ilerlemesinin ta kendisi
                // (bekçisi `every_base_glyph_advance_is_the_cell_advance`),
                // yani geniş ilan edilmiş bir karakter taban fontta varsa
                // orada tek hücreye çizilir ve çizimi **bit bit** eskisiyle
                // aynı kalır. Menlo'nun `☕ ⚡ ♈` ailesi tam bu kol: ölçülen
                // 65'in 21'i.
                let drawn =
                    raster::draw(font, ch, self.metrics, cell_advance, 0.0, &mut self.buffer);
                // **Yedek font.** Kol `NoGlyph` yaprağının içinde ve yalnız
                // düz yüzde koşuyor, yani sıra şu: önce yüz merdiveni
                // (aşağıdaki `face != Regular` kolu düz yüze iniyor), sonra
                // burası, en sonda negatif önbellek. Merdiven tüketilmeden
                // yedeğe gidilseydi kalın bir `─` sistem fontundan gelir ve
                // ailenin kendi düz yüzü hiç sorulmazdı.
                //
                // Kol `match drawn` içinde değil **çizim adımının içinde**,
                // iki mecburi sebeple: `match`'in kolları negatif önbellek
                // koluna düşemiyor (yedek reddedilirse oraya inmek gerekiyor)
                // ve `ch` yalnız burada kapsamda. Semantik sıra değişmiyor.
                //
                // Kabul edilen aday aşağıdaki `Drawn` kolundan geçiyor: aynı
                // anahtar, aynı `Upload`, aynı yuva aritmetiği. Yani arama
                // anahtar başına atlasın ömründe **bir kez** koşuyor — ret de
                // negatif önbelleğe giriyor.
                if drawn == DrawResult::NoGlyph && face == Face::Regular {
                    // Kapıya **sütun sayısı** gidiyor ve tek kaynağı çağıran:
                    // `bt-atlas` `unicode-width` görmüyor (yeni bir bağımlılık
                    // *ve* ızgaranınkiyle ayrışabilen ikinci bir genişlik
                    // yetkilisi olurdu). Sıra kapının içinde: önce tek hücre,
                    // sonra iki.
                    let cols = if want == Half::Left { 2 } else { 1 };
                    match font::fallback_font(font, ch, cell_advance, cols) {
                        Some(alt) => self.draw_accepted(&alt, cell_advance),
                        None => (drawn, Half::Whole, Plane::Mask),
                    }
                } else {
                    (drawn, Half::Whole, Plane::Mask)
                }
            }
            // **Dizi** `Char`'ın font kolunun kardeşi: yordamsal kapı ona
            // uygulanmıyor (bir dizi blok ya da çizgi karakteri değil) ve
            // aday yine sınıfın kendi fontundan cascade'e gidiyor.
            Sprite::Cluster(id) => {
                let Some(text) = self.clusters.get(id as usize) else {
                    // Kimlik bu atlasın interner'ında yok: çağıran yeniden
                    // kurulmadan önceki bir atlastan kalma bir kimlik
                    // taşıyor ([`Atlas::clusters`]). **Önbelleklenmiyor** —
                    // dolu atlasın gerekçesiyle: kimlik kalıcı bir gerçek
                    // değil, yeniden sorulduğunda başka bir dizgiye
                    // bağlanabilir. Panik değil, çünkü `slot()` display
                    // link'in callback'inde.
                    return (
                        Placed {
                            slot: TOFU,
                            half: Half::Whole,
                            plane: Plane::Mask,
                        },
                        None,
                    );
                };
                // Taban `intern`'ün ayırdığı gibi en az iki kod noktası
                // taşıyan dizginin ilk karakteri; boş olamaz ama `slot()`
                // çizim yolunda, yani varsayım bir panik değil boşluk.
                let base = text.chars().next().unwrap_or(' ');
                let (font, cell_advance) = match size {
                    SizeClass::Normal => (self.faces.get(face), self.cell_advance),
                    SizeClass::Small => (&*self.small, self.context_advance),
                };
                // Sütun sayısı `Char`'ınkiyle aynı kaynaktan (çağıranın
                // istediği yarı) ve kapının sırası aynı: önce tek, sonra iki.
                let cols = if want == Half::Left { 2 } else { 1 };
                match font::shape_cluster(font, text, cell_advance, cols) {
                    Some(alt) => self.draw_accepted(&alt, cell_advance),
                    // Tek glyph'e şekillenmedi ya da kapıdan döndü: cevap
                    // **taban karakterin** (035 R1.1). Kutu değil, çünkü
                    // taban karakteri çoğu zaman çizilebiliyor (`👍👍`'nin
                    // `👍`'si); yarım glyph değil, çünkü taban karakter kendi
                    // kapısından geçiyor.
                    None => return self.cluster_as_base(sprite, base, size, want),
                }
            }
            // Yordamsal çizim başarısız olamaz: font sorulmuyor, bağlam
            // kurulmuyor. `Drawn` bir varsayım değil, tipin kendisi.
            Sprite::Rule(kind) => {
                raster::draw_rule(kind, self.metrics, &mut self.buffer);
                (DrawResult::Drawn, Half::Whole, Plane::Mask)
            }
        };
        let (result, half, plane) = result;
        // Anahtar **çözülen** yarıyı taşıyor: `Left` istenip tek hücreye sığan
        // karakter `Whole`'a yazılıyor, yani ikinci soruluşunda önbellekten
        // aynı cevap dönüyor ve kapı bir daha koşmuyor.
        let key = (sprite, face, size, half);
        match result {
            // **Kararı veren kapı burası.** Yukarıdaki `need` kapısı iki
            // düzlemin boşta olanına bakıyor ve yalnız ikisi de doluyken
            // kapanıyor, yani buraya bir düzlemi dolu bir atlasla
            // gelinebiliyor. Düzlem artık biliniyor (aday fontun trait biti),
            // yani ölçüt kesin: o düzlemin kendi sayacı.
            DrawResult::Drawn
                if u32::from(match plane {
                    Plane::Mask => self.next,
                    Plane::Color => self.color_next,
                }) + u32::from(if half == Half::Left { 2u16 } else { 1 })
                    > u32::from(cap) =>
            {
                (
                    Placed {
                        slot: TOFU,
                        half: Half::Whole,
                        plane: Plane::Mask,
                    },
                    None,
                )
            }
            DrawResult::Drawn => {
                // Sayaç **düzlemin kendi sayacı**: iki düzlem aynı ızgara
                // aritmetiğini paylaşıyor ama yuva numaraları ayrı uzaylarda
                // (gerekçe [`Plane`]).
                let slot = match plane {
                    Plane::Mask => self.next,
                    Plane::Color => self.color_next,
                };
                // **Çift atomik.** İki yuva aynı ifadede ayrılıyor, iki
                // anahtar aynı ifadede yazılıyor ve iki bayt dizisi aynı
                // `Upload`'la dönüyor: sağ yarı için ikinci bir `slot()` turu
                // yok, yani kapasite sınırı ikisinin arasına düşemiyor.
                // Yukarıdaki `need` bu ifadenin ön koşulu.
                let pair = half == Half::Left;
                let step = if pair { 2 } else { 1 };
                match plane {
                    Plane::Mask => self.next += step,
                    Plane::Color => self.color_next += step,
                }
                self.slots.insert(key, (slot, plane));
                let right = pair.then(|| {
                    let right_slot = slot + 1;
                    self.slots
                        .insert((sprite, face, size, Half::Right), (right_slot, plane));
                    self.slot_origin(right_slot)
                });
                let origin = self.slot_origin(slot);
                let (bytes, right_bytes) = match plane {
                    Plane::Mask => (&self.buffer, &self.buffer_right),
                    Plane::Color => (&self.color_buffer, &self.color_buffer_right),
                };
                (
                    Placed { slot, half, plane },
                    Some(Upload {
                        origin,
                        bytes,
                        right,
                        right_bytes,
                        plane,
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
                let (placed, upload) = self.slot(sprite, Face::Regular, size, want);
                // `map` `upload`'ı tüketiyor ve `self.buffer` ödüncü burada
                // bitiyor; `insert` ancak ondan sonra mümkün. Tampon özyineli
                // çağrının çizdiği baytları hâlâ taşıyor, yani `Upload` aynı
                // içerikle yeniden kurulabiliyor.
                // Düz yüzün **çözdüğü** yarı yazılıyor, istenen değil:
                // merdivenden dönen cevap `Whole` olabilir (`☕`'nin kalın
                // yüzü) ve takma adı `Left` diye yazmak çağırana ikinci bir
                // instance bastırırdı.
                let origin = upload.as_ref().map(|upload| upload.origin);
                let right = upload.as_ref().and_then(|upload| upload.right);
                self.slots.insert(
                    (sprite, face, size, placed.half),
                    (placed.slot, placed.plane),
                );
                if right.is_some() {
                    // Sağ yarının takma adı da yazılıyor, yoksa kalın yüzde
                    // sorulan sağ yarı düz yüzü yeniden rasterize ederdi.
                    self.slots.insert(
                        (sprite, face, size, Half::Right),
                        (placed.slot.saturating_add(1), placed.plane),
                    );
                }
                let (bytes, right_bytes) = match placed.plane {
                    Plane::Mask => (&self.buffer, &self.buffer_right),
                    Plane::Color => (&self.color_buffer, &self.color_buffer_right),
                };
                let upload = origin.map(|origin| Upload {
                    origin,
                    bytes,
                    right,
                    right_bytes,
                    plane: placed.plane,
                });
                (placed, upload)
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
                // yuvalar) korunuyor: onları toptan atmak dokuyu da
                // düşürmeyi gerektirir ve o karar 022'de kapsam dışı
                // bırakıldı — kapasite hücre ölçüsünden türüyor, yani
                // pozitif tarafın dolması artık çok daha zor.
                //
                // **Bedel yedekle birlikte büyüdü** ve bu bilerek kabul
                // edildi: tahliyeden sonra geri sorulan karakter artık yalnız
                // `CTFontGetGlyphsForCharacters` değil bir cascade yürüyüşü de
                // ödüyor. Sıcak yürüyüş ölçüldü ve ucuz (setin `phase-1.md`'si
                // → Uygulama Notları); pahalı olan bir **ailenin ilk
                // açılışı** ve o tahliyeden etkilenmiyor — font CoreText'te
                // açık kalıyor, yeniden yüklenmiyor. Yani tahliyenin geri
                // getirdiği maliyet sıcak yürüyüş, soğuk açılış değil.
                if self.slots.len() >= self.negative_cache_cap() {
                    // Ölçüt **kaydın tamamı**, yuva numarası değil: renk
                    // düzleminin sayacı 0'dan başlıyor ve `TOFU` da 0, yani
                    // numaraya bakan bir süzgeç ilk emojinin **pozitif**
                    // kaydını da atardı. Belirti sessiz ve iki katlı: emoji
                    // bir sonraki görülüşünde yeniden rasterize olur, eski
                    // yuvası öksüz kalır ve `yuva2=` şişer.
                    self.slots
                        .retain(|_, &mut entry| entry != (TOFU, Plane::Mask));
                }
                self.slots.insert(key, (TOFU, Plane::Mask));
                // **Ret `want` anahtarına da yazılıyor** ve bu şart:
                // yukarıdaki `key` **çözülen** yarıyı taşıyor ve ret kolunda
                // o her zaman `Whole`, yani `Left` isteğinin kendi anahtarı
                // hiç yazılmazdı. Takma ad artık `TOFU`'yu geçirmediğine göre
                // o istek her karede yeniden bir cascade yürüyüşü öderdi —
                // ana thread'de, kare bütçesinin ortasında. Üç yarının üçü de
                // yazılıyor ve üçü de doğru: `cols = 1` ölçütü iki
                // hücrelikten kesin olarak daha sıkı, yani `Left`
                // reddedildiyse `Whole` da reddedilmiştir.
                if want == Half::Left {
                    self.slots
                        .insert((sprite, face, size, Half::Left), (TOFU, Plane::Mask));
                    self.slots
                        .insert((sprite, face, size, Half::Right), (TOFU, Plane::Mask));
                }
                (
                    Placed {
                        slot: TOFU,
                        half: Half::Whole,
                        plane: Plane::Mask,
                    },
                    None,
                )
            }
        }
    }

    /// Kapıdan geçmiş adayı çizer: düzlem, bir ya da iki yarı ve ikisinin
    /// birlikte başarısı.
    ///
    /// Yedek karakter ile grapheme dizisinin **ortak** çizimi; ayrı
    /// yazılsalardı iki yarının aynı kutuya ortalanması ve çiftin atomik
    /// kabulü iki kopyada yaşar, biri ayrıştığında öteki fark etmezdi.
    fn draw_accepted(
        &mut self,
        alt: &font::Accepted,
        cell_advance: CGFloat,
    ) -> (DrawResult, Half, Plane) {
        // **Düzlem adayın kendi özelliğinden**: renkli glyph taşıyan bir font
        // `RGBA8` düzlemine, ötekiler maskeye. Ölçüt trait biti, aile adı
        // değil (gerekçe [`font::has_color_glyphs`]).
        let plane = if font::has_color_glyphs(&alt.font) {
            Plane::Color
        } else {
            Plane::Mask
        };
        // İki yarı **aynı kutuya** ortalanıyor ve ikisi de aynı çağrıda
        // çiziliyor: sağ yarının ofseti tam sayı piksel, yani AA fazı ikisinde
        // birebir aynı.
        let pair = alt.cols >= 2;
        let box_advance = cell_advance * f64::from(alt.cols);
        let shift = f64::from(self.metrics.cell_px.0);
        let half = if pair { Half::Left } else { Half::Whole };
        // Tek çizici, iki reçete: `Plane` hangisi olacağını söylüyor ve tampon
        // da onunla eşleşiyor. Eşleşmezse `raster`'ın ön koşul assert'i düşer
        // — o assert yanlış düzlemi yakalayan tek şey.
        let left = match plane {
            Plane::Mask => raster::draw_glyph(
                &alt.font,
                alt.glyph,
                self.metrics,
                box_advance,
                0.0,
                &mut self.buffer,
            ),
            Plane::Color => raster::draw_color_glyph(
                &alt.font,
                alt.glyph,
                self.metrics,
                box_advance,
                0.0,
                &mut self.color_buffer,
            ),
        };
        if !pair {
            return (left, half, plane);
        }
        let right = match plane {
            Plane::Mask => raster::draw_glyph(
                &alt.font,
                alt.glyph,
                self.metrics,
                box_advance,
                shift,
                &mut self.buffer_right,
            ),
            Plane::Color => raster::draw_color_glyph(
                &alt.font,
                alt.glyph,
                self.metrics,
                box_advance,
                shift,
                &mut self.color_buffer_right,
            ),
        };
        // İki çağrı aynı fontun aynı glyph'ini soruyor, yani ikisi birden
        // başarılı ya da ikisi birden değil. Yine de **ikisi de** sınanıyor:
        // biri düşerse çift kabul edilmemeli, yoksa yarısı boş bir glyph
        // çizilirdi.
        let both = left == DrawResult::Drawn && right == DrawResult::Drawn;
        let worst = if both {
            DrawResult::Drawn
        } else {
            DrawResult::NoGlyph
        };
        (worst, half, plane)
    }

    /// Şekillenmeyen dizinin cevabı: **taban karakterin** yuvası, dizinin
    /// anahtarına takma adla.
    ///
    /// Yüz merdiveninin takma adıyla (`DrawResult::NoGlyph if face !=
    /// Regular` kolu) aynı örüntü ve aynı gerekçe: takma ad yazılmasaydı dizi
    /// her karede yeniden `CTLine` kurar, şekillendirir ve kapıdan döner —
    /// ana thread'de, kare bütçesinin ortasında. Taban karakterin kendi
    /// kaydı ayrı yaşıyor, yani ızgarada tek başına duran aynı karakter
    /// ikinci bir yuva açmıyor.
    fn cluster_as_base(
        &mut self,
        sprite: Sprite,
        base: char,
        size: SizeClass,
        want: Half,
    ) -> (Placed, Option<Upload<'_>>) {
        let (placed, upload) = self.slot(Sprite::Char(base), Face::Regular, size, want);
        // `upload` burada tüketiliyor ki `self.buffer` ödüncü bitsin; tampon
        // özyineli çağrının çizdiği baytları hâlâ taşıyor.
        let origin = upload.as_ref().map(|upload| upload.origin);
        let right = upload.as_ref().and_then(|upload| upload.right);
        let key = |half| (sprite, Face::Regular, size, half);
        // Taban karakterin **çözdüğü** yarı yazılıyor, istenen değil: `❤️`'nin
        // `❤`'si tek hücreye sığabiliyor ve takma adı `Left` diye yazmak
        // çağırana ikinci bir instance bastırırdı.
        self.slots
            .insert(key(placed.half), (placed.slot, placed.plane));
        // Sağ yarının takma adı çözülen yarıdan, yüklemeden değil: taban
        // karakter önbellekten döndüyse yükleme yok ama çift yine de iki
        // komşu yuva.
        if placed.half == Half::Left {
            self.slots.insert(
                key(Half::Right),
                (placed.slot.saturating_add(1), placed.plane),
            );
        }
        // Ret de **istenen** anahtara yazılıyor (negatif önbelleğin kuralı):
        // ret her zaman `Whole` çözüyor, yani `Left` isteği yazılmasaydı her
        // karede yeniden şekillendirilirdi. Düzlem de sorulmak zorunda:
        // `TOFU` maske düzleminin 0. yuvası, renk düzleminin 0. yuvası ise
        // ilk emojinin gerçek yuvası.
        if placed.slot == TOFU && placed.plane == Plane::Mask {
            self.slots.insert(key(want), (TOFU, Plane::Mask));
            if want == Half::Left {
                self.slots.insert(key(Half::Right), (TOFU, Plane::Mask));
            }
        }
        let (bytes, right_bytes) = match placed.plane {
            Plane::Mask => (&self.buffer, &self.buffer_right),
            Plane::Color => (&self.color_buffer, &self.color_buffer_right),
        };
        let upload = origin.map(|origin| Upload {
            origin,
            bytes,
            right,
            right_bytes,
            plane: placed.plane,
        });
        (placed, upload)
    }

    /// (kullanılan, toplam) yuva.
    ///
    /// Tofu kullanılan sayılır: doku o yuvayı da tutuyor ve doluluk oranı
    /// `/measure`'da bu iki sayıdan okunacak.
    pub fn occupancy(&self) -> (usize, usize) {
        (usize::from(self.next), usize::from(self.capacity()))
    }

    /// Renk düzleminin (kullanılan, toplam) yuvası.
    ///
    /// Maskeden **ayrı** yayımlanıyor ve gerekçesi jeton sözleşmesi: duman
    /// kapısının `yuva=` sayacı yalnız maske düzlemini sayıyor ve ikinci bir
    /// düzlemi ona toplamak "hangi düzlem doldu" sorusunu cevapsız bırakırdı.
    /// Göremediği bir düzlem tam olarak 021'in Braille şekli olurdu: sıfır
    /// yuva harcayan, sessiz.
    ///
    /// Toplam ikisinde de aynı ([`Atlas::capacity`]): iki düzlem aynı yuva
    /// ızgarasını paylaşıyor, ayrışan yalnız piksel formatı ve sayaç.
    pub fn color_occupancy(&self) -> (usize, usize) {
        (usize::from(self.color_next), usize::from(self.capacity()))
    }

    /// Haritanın kabul ettiği en çok kayıt sayısı — pozitif ve negatif
    /// birlikte. Kapasitenin **iki katı**: bir katı pozitif kayıtların
    /// olabildiği en büyük değer, ikincisi negatif önbelleğe bırakılan pay.
    ///
    /// Kastedilen kapasite [`Atlas::capacity`], yani **türetilmiş** kenardan
    /// çıkan sayı ([`SLOT_TARGET`]) — sabit bir tavan değil. Kenar
    /// katlandığında bu pay da onunla büyüyor ve büyümesi doğru: pozitif
    /// tarafta daha çok yuva varsa negatif tarafta da daha çok karakter
    /// denenmiş demektir.
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

/// Hücre ölçüsüne düşen doku kenarı, piksel — kararın **tek** kaynağı.
///
/// Taban [`MIN_EDGE`]; kapasite [`SLOT_TARGET`]'ın altında kaldıkça ve tavana
/// ([`MAX_EDGE`]) varmadıkça ikiye katlanıyor. Sınamalar bu fonksiyonu
/// **çağırıyor**, ikinci bir kopyasını yazmıyor: aynalanmış bir türetme
/// kendi hatasını göremez.
fn edge_for(w: u16, h: u16) -> u16 {
    let mut edge = MIN_EDGE;
    while slots_at(grid_at(edge, w, h)) < SLOT_TARGET && edge < MAX_EDGE {
        // `min` bir savunma refleksi değil, [`MAX_EDGE`]'in doc'unu **doğru**
        // kılan şey: guard katlamadan **önce** bakıyor, yani tavan
        // `MIN_EDGE * 2^k` değilse çarpım onu aşardı ve sabit adının
        // söylediği şeyi söylemez olurdu. Taşma da aynı satırda kapanıyor.
        edge = edge.saturating_mul(2).min(MAX_EDGE);
    }
    edge
}

/// Verilen kenarda ızgaranın satır/sütun sayısı — **tek ifade, iki okuyucu**
/// ([`edge_for`]'un kararı ile [`grid_for`]'un kurduğu ızgara).
///
/// İki kopya olsaydı sessizce ayrışabilirlerdi: büyüme döngüsü bir sayıya
/// göre "hedef tutturuldu" derken kurulan ızgara başka bir sayı verirdi.
fn grid_at(edge: u16, w: u16, h: u16) -> (u16, u16) {
    ((edge / w).max(1), (edge / h).max(1))
}

/// Izgaranın yuva sayısı. `u32`: çarpım küçük hücrede `u16`'yı aşıyor
/// (13pt@1x, 4096 kenar → 116 224).
fn slots_at((cols, rows): (u16, u16)) -> u32 {
    u32::from(cols) * u32::from(rows)
}

/// [`edge_for`]'un ızgaraya çevrilmiş hâli.
fn grid_for(w: u16, h: u16) -> (u16, u16) {
    grid_at(edge_for(w, h), w, h)
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
    /// rasterize etmeden koşuyor. **Havuz 022'de büyüdü** (yordamsal aile +
    /// ASCII × dört yüz ≈ 800 istek) çünkü kenar türetildikten sonra en
    /// küçük kapasite 564'e çıktı ve 95 karakterlik ASCII onu dolduramıyor;
    /// yani "onlarca" artık doğru değil, ama binlerce de değil ve seçimin
    /// gerekçesi aynı kalıyor. Değer [`MAX_POINT_SIZE`]'tur:
    /// üstünü istemek sessizce kırpılır ve sınama kapasiteyi yanlış sanırdı.
    const LARGE_POINT_SIZE: f64 = MAX_POINT_SIZE;
    /// Ayar ayrıştırıcısının kabul ettiği en büyük satır aralığı.
    ///
    /// Kaynağı `bt_core::settings::MAX_LINE_HEIGHT` ama **oradan
    /// okunamıyor**: katman yönü `bt-atlas`'ın `bt-core`'u görmesini
    /// yasaklıyor. Kopya bilinçli ve dar — yalnız en kötü köşeyi kurmak
    /// için; ikisi ayrışırsa bu sınama köşeyi kaçırır, yanlış çizim üretmez.
    const LARGEST_LINE_HEIGHT: f64 = 2.0;
    const POINT_SIZE: f64 = 13.0;
    /// Tofu'ya düşen karakter — ve **iki** kapıdan birden düşüyor.
    ///
    /// Menlo ile SF Mono CJK içermez, yani taban font `.notdef` veriyor
    /// (`CTFontGetGlyphsForCharacters` cascade'e inmiyor). Yedek aramanın
    /// gelişiyle yol bir adım uzadı: cascade **bir aday buluyor** (PingFang
    /// SC) ve o aday mürekkep kapısından dönüyor — hücre 7.827 iken mürekkebi
    /// 0.70'ten 12.49'a uzanıyor (ölçüldü, bu makine, Menlo 13pt). CJK'de
    /// ilerleme ile mürekkep birlikte geniş, yani kapı ölçütü değiştiğinde bu
    /// karakterin cevabı değişmedi. Bu sabite dayanan sınamalar "tofu" derken
    /// kapının da çalıştığını varsayıyor; kapının kendi bekçisi
    /// [`the_gate_decides_by_ink_alone`].
    const UNKNOWN_CHAR: char = '漢';
    /// Yedeğin **kabul ettiği** karakter ve setin varlık sebebi: `⏵` Menlo'da
    /// yok, Claude Code'un `⏵⏵ auto mode on` göstergesi iki kutu çıkıyordu.
    /// Ölçüldü (bu makine, macOS 26.4.1): STIX Two Math'ten geliyor,
    /// ilerlemesi hücrenin 0.84'ü ve mürekkebi 0.69'u — oran ölçekten bağımsız
    /// olduğu için iki boy sınıfında da kapıyı geçiyor. Kapı ilerlemeyi
    /// ölçerken de mürekkebi ölçerken de kabul ettiği tek karakter bu, yani
    /// **ölçüt değişikliğinin tanığı değil**: onun için [`INK_CHAR`] var.
    const FALLBACK_CHAR: char = '⏵';
    /// Kapının **ölçütünü** sınayan karakter: ilerlemesi hücreyi aşıyor ama
    /// mürekkebi hücreye sığıyor.
    ///
    /// `⏺` Claude Code'un araç işareti ve kullanıcıda kutu çıkıyordu. Ölçüldü
    /// (bu makine, Menlo 16pt): aday yine STIX Two Math, ilerlemesi hücrenin
    /// **1.046 katı** ama mürekkebi **0.914'ü** — yani ilerlemeyi ölçen kapı
    /// hücreye rahat sığan bir glyph'i eliyordu. İki ölçütün ayrıştığı tek
    /// tanık bu: [`FALLBACK_CHAR`] ikisinden de geçiyor, [`UNKNOWN_CHAR`]
    /// ikisinde de eleniyor, yani ölçüt geri alınsa onlar bunu görmezdi.
    ///
    /// Listenin ötekilerinde olduğu gibi beklenti **sabite yazılmıyor**:
    /// karakteri taşıyan bir font kurulu bir makinede taban fonttan gelir ve
    /// yedek yolu hiç koşmaz.
    const INK_CHAR: char = '⏺';
    /// Kapının **kuralını** sınamak için kullanılan karakterler: hepsi
    /// Menlo'da yok, yani yedek yoluna giriyorlar — `⠋` bir istisna ve
    /// listede kalma sebebi o: büyük sınıfta yordamsal çiziliyor, yani
    /// yedeğe hiç gelmiyor; küçük sınıfta kapı kapalı ve yol hâlâ açık.
    ///
    /// Listenin taşıdığı iddia "bunlar kutu olur" **değil** — o, makinede
    /// hangi fontların kurulu olduğuna bağlı bir olgu, kodun bir özelliği
    /// değil. `U+E0B0` bu makinede `.LastResort`'a düşüyor ama Nerd Font
    /// kurulu bir makinede (terminal kullanıcılarında çok yaygın) gerçek bir
    /// glyph'e düşer ve **çizilmesi doğru olur**. Beklentiyi listeye yazmak
    /// `make hepsi`'yi doğru kodda kırmızıya düşürürdü; bu yüzden beklenti
    /// listede değil, [`the_gate_decides_by_width_alone`] onu adayın kendi
    /// ilerlemesinden **türetiyor**.
    const GATE_PROBES: [char; 9] = [
        FALLBACK_CHAR,
        INK_CHAR,
        '𝔸',
        UNKNOWN_CHAR,
        '\u{E0B0}',
        '\u{10FFFD}',
        '🎉',
        '\u{F8FF}',
        // Braille: Apple Braille'den geliyor. **Büyük sınıfta kapıya hiç
        // gelmiyor** — yordamsal çiziliyor; listede kalmasının sebebi küçük
        // sınıf, orada yordamsal kapı kapalı ve yedek yolu hâlâ koşuyor.
        // Cevabı ölçütle birlikte **değişen** ikinci karakter: ilerlemesi
        // hücrenin 1.135 katı ama mürekkebi 2.62'den 8.34'e, yani 9.633'lük
        // hücrenin içinde (ölçüldü, Menlo 16pt) — küçük sınıfta artık
        // çiziliyor.
        '⠋',
    ];
    /// Hiçbir makinede olmayan aile; CoreText yerine başka bir font verir.
    const MISSING_FAMILY: &str = "Bu Aile Yok 12345";

    /// Zincirle kurulan atlas — ayarda aile yokken üretimin kurduğu.
    fn atlas(point_size: f64, scale: f64) -> Atlas {
        Atlas::new(None, point_size, scale, 1.0)
    }

    /// Bir boy sınıfının (ad, taban font, **kesirli** hücre ilerlemesi)
    /// üçlüsü. Yedekle ilgili her bekçi iki sınıfı da ayrı ayrı dolaşmak
    /// zorunda: taban font ve sınır sınıf başına ayrı, tek bir tanesinden
    /// geçen sınama ötekini hiç sınamamış olur.
    fn size_classes(a: &Atlas) -> [(&'static str, &CTFont, CGFloat); 2] {
        [
            ("düz yüz", a.faces.get(Face::Regular), a.cell_advance),
            ("küçük yüz", &a.small, a.context_advance),
        ]
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
    fn the_cell_is_the_rounded_advance() {
        // Hücre genişliği iki temsilde yaşıyor: kesirli ([`Atlas::cell_advance`],
        // yedek kapısı ile ortalamanın girdisi) ve yukarı yuvarlanmış
        // ([`Metrics::cell_px`], ızgaranın adımı). İkisi **aynı ölçü** olmak
        // zorunda; ayrışsalar kapı bir hücreye, ortalama başka bir hücreye
        // bakar ve belirti sessiz olur. Küçük sınıfta ayrıca bir tarihçe var:
        // `context_cell_w` bir dönem `font::metrics(&small, ..)` üzerinden
        // türüyordu, yani aynı sayının iki kaynağı vardı.
        for (point_size, scale) in [
            (POINT_SIZE, 1.0),
            (POINT_SIZE, 2.0),
            (LARGE_POINT_SIZE, 1.0),
        ] {
            let a = atlas(point_size, scale);
            assert_eq!(
                font::round_up(a.cell_advance),
                a.metrics.cell_px.0,
                "{point_size}×{scale}: büyük sınıfın iki temsili ayrıştı ({})",
                a.cell_advance
            );
            assert_eq!(
                font::round_up(a.context_advance),
                a.context_cell_w,
                "{point_size}×{scale}: küçük sınıfın iki temsili ayrıştı ({})",
                a.context_advance
            );
        }
    }

    #[test]
    fn every_base_glyph_advance_is_the_cell_advance() {
        // Ortalama **evrensel** ve taban fontta tam olarak sıfır olmak
        // zorunda: `(cell - advance) / 2` kesirli bir sonuç verse CG'nin kenar
        // yumuşatması değişir ve depodaki bütün piksel bekçilerinin
        // (`glyph_sits_on_the_baseline`, `descender_fits_in_the_cell`, …)
        // altı sessizce oyulur. Bu sınama o sıfırın **sebebini** tutuyor.
        //
        // İddia "font eşaralıklı" değil, ondan daha güçlü: her glyph'in
        // ilerlemesi hücrenin ilerlemesine **bit bit** eşit. Eşaralıklı
        // olmayan bir aile bunu düşürmez (zincirin tabanı Menlo) ama orada
        // ortalama gerçekten kaydırır — `raster::draw`'in `max(0.0)`'ı o yolu
        // adıyla anlatıyor.
        let a = atlas(POINT_SIZE, 1.0);
        // **Beş fontun beşi de**, iki değil: `cell_advance` düz yüzün ölçüsü
        // (`Metrics` yalnız ondan türüyor) ama atlas kalın, eğik ve kalın-eğik
        // yüzleri de **aynı** sayıyla ortalıyor. Kalın yüzü düz yüzünden dar
        // bir ailede her kalın glyph sağa kayardı ve kayma yalnız bir yönde
        // görünürdü — `max(0.0)` ötekini yutuyor. Bekçi düz yüzle küçük yüzle
        // sınırlı kalsaydı o kolu hiç görmezdi.
        let fonts = [
            ("düz yüz", a.faces.get(Face::Regular), a.cell_advance),
            ("kalın yüz", a.faces.get(Face::Bold), a.cell_advance),
            ("eğik yüz", a.faces.get(Face::Italic), a.cell_advance),
            ("kalın eğik", a.faces.get(Face::BoldItalic), a.cell_advance),
            ("küçük yüz", &a.small, a.context_advance),
        ];
        for (label, face_font, cell) in fonts {
            // Yazdırılabilir ASCII, kutu çizim ve Menlo'nun kendi simgeleri:
            // hücreden farklı ilerleyen bir glyph varsa buradan görünür.
            // Birleştirici işaretler de listede: ilerlemesi sıfır olan bir
            // glyph hücrenin **ortasına** rasterize olurdu ve belirti ancak
            // ekranda görünürdü (Menlo'da U+0301 tam hücre ilerliyor, yani bu
            // kol bugün kapalı — ölçüldü).
            for ch in (' '..='~').chain("─│┌┐└┘├┤┬┴┼✓⚠▶\u{0300}\u{0301}".chars())
            {
                let Some(glyph) = font::glyph_index(face_font, ch) else {
                    continue;
                };
                assert_eq!(
                    font::glyph_advance(face_font, glyph),
                    cell,
                    "{label}: '{ch}' hücreden farklı ilerliyor, ortalama artık no-op değil"
                );
            }
        }

        // İkinci yarı: `raster::draw` bu sayıyı **gerçekten** tüketiyor.
        // Yalnız yukarıdaki eşitlik sınansaydı iddia girdinin doğruluğundan
        // ibaret kalırdı; `draw`'in girdiyi yok sayıp yuvarlanmış hücreye
        // (`cell_px.0`) bakması — yani taban fontun her glyph'ini 0.09 piksel
        // kaydırıp bütün rasteri sessizce değiştirmesi — buradan geçerdi.
        // Ölçüt: daha geniş bir hücre ilerlemesi bitmap'i sağa itmeli.
        let m = a.metrics();
        let font = a.faces.get(Face::Regular);
        let mut own = vec![0u8; m.slot_bytes()];
        let mut wider = vec![0u8; m.slot_bytes()];
        assert_eq!(
            raster::draw(font, 'W', m, a.cell_advance, 0.0, &mut own),
            DrawResult::Drawn
        );
        // Dört piksel geniş bir hücre glyph'i iki piksel sağa iter.
        assert_eq!(
            raster::draw(font, 'W', m, a.cell_advance + 4.0, 0.0, &mut wider),
            DrawResult::Drawn
        );
        assert_ne!(
            own, wider,
            "`draw` hücre ilerlemesini yok sayıyor: ortalama girdiye bağlı değil"
        );
    }

    #[test]
    fn fallback_glyph_is_drawn_in_both_size_classes() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let (w, h) = a.metrics().cell_wh();
        // Kapsamanın en sağdaki sütunu; `the_small_class_is_narrower_…`'in
        // ölçütüyle aynı ve aynı sebeple: tek tek piksel değeri font sürümüne
        // bağlı, sınır değil.
        let ink_right = |bytes: &[u8]| {
            (0..w)
                .rev()
                .find(|&x| (0..h).any(|y| bytes[y * w + x] > 0))
                .expect("yedek glyph hiç piksel boyamadı")
        };

        let mut edge = Vec::new();
        for size in [SizeClass::Normal, SizeClass::Small] {
            let (placed, upload) = a.slot(
                Sprite::Char(FALLBACK_CHAR),
                Face::Regular,
                size,
                Half::Whole,
            );
            let slot = placed.slot;
            assert_ne!(
                slot, TOFU,
                "{size:?}: '{FALLBACK_CHAR}' yedekten gelmeli, kutu değil"
            );
            edge.push(ink_right(upload.expect("yeni yuva").bytes));
        }

        // İki sınıf **ayrı ayrı** değerlendiriliyor ve ayrı yuva tutuyor:
        // anahtar boy sınıfı taşımasaydı tek yuva çıkardı.
        assert_eq!(
            a.occupancy().0,
            3,
            "iki boy sınıfı ayrı yuva almalı (tofu dahil üç)"
        );
        // Ve yedeğin **tabanı** o sınıfın kendi fontu: küçük sınıfta aynı
        // yuvaya daha dar bir iz düşmeli. Bu olmadan "iki sınıf da çalışıyor"
        // iddiası, küçük satıra büyük punto glyph çizen bir uygulamadan
        // ayırt edilemezdi — ve belirti sessiz olurdu, çünkü bir şey yine
        // görünürdü.
        assert!(
            edge[1] < edge[0],
            "küçük sınıfın yedeği daralmadı: sağ kenar büyükte {}, küçükte {}",
            edge[0],
            edge[1]
        );
    }

    #[test]
    fn fallback_glyph_fits_the_cell() {
        // `slot != TOFU` kırpmayı **göremez**: CG hücrenin dışına taşan
        // mürekkebi sessizce kesiyor ve bitmap yine dolu görünür. Kapı
        // yatayda artık mürekkebi ölçüyor, ama **dikeyde ölçmüyor** (gerekçe
        // `font::ink_fits_cell`'in doc'unda: dikeyi eleyen tek küme emoji ve
        // o zaten yatayda dönüyor) — ascent'i yüksek bir aday kapıyı geçip
        // yine kırpılabilir. Ölçüt bu yüzden fontun kendi sınır dikdörtgeni
        // ve **dört kenar birden**: yatayda kapının tanığı, dikeyde tek
        // bekçi.
        let a = atlas(POINT_SIZE, 1.0);
        let m = a.metrics();
        // CG'nin başlangıcı sol alt: taban çizgisi yuvanın dibinden bu kadar
        // yukarıda (`raster::draw` ile aynı aritmetik).
        let baseline = f64::from(m.cell_px.1 - m.baseline_px);
        for (label, base, cell) in size_classes(&a) {
            let alt = font::fallback_font(base, FALLBACK_CHAR, cell, 1)
                .map(|accepted| accepted.font)
                .unwrap_or_else(|| panic!("{label}: '{FALLBACK_CHAR}' kapıdan geçmeli"));
            let glyph =
                font::glyph_index(&alt, FALLBACK_CHAR).expect("kapıyı geçen aday çizebiliyor");
            let rect = font::glyph_ink(&alt, glyph);
            let x = font::centre_shift(cell, font::glyph_advance(&alt, glyph));
            let (left, right) = (x + rect.origin.x, x + rect.origin.x + rect.size.width);
            assert!(left >= 0.0, "{label}: mürekkep soldan taştı ({left})");
            assert!(
                right <= cell,
                "{label}: mürekkep sağdan taştı ({right} > {cell})"
            );
            let (bottom, top) = (
                baseline + rect.origin.y,
                baseline + rect.origin.y + rect.size.height,
            );
            assert!(bottom >= 0.0, "{label}: mürekkep alttan taştı ({bottom})");
            let cell_h = f64::from(m.cell_px.1);
            assert!(
                top <= cell_h,
                "{label}: mürekkep üstten taştı ({top} > {cell_h})"
            );

            // Ve ortalama yedek yolunda **gerçekten** koşuyor: aynı adayı
            // kaydırmasız çizmek (sınırı tam glyph'in ilerlemesi yaparak,
            // yani kaydırmayı inşaen sıfırlayarak) başka bir bitmap veriyor.
            //
            // Ölçüt "mürekkebin ağırlık merkezi hücrenin ortasına yaklaştı"
            // **değil**: ortalanan şey glyph'in **ilerleme kutusu**, mürekkebi
            // değil, ve `⏵`'nin yan yatakları asimetrik (ölçüldü: solda 1.04,
            // sağda 0.13). Merkez ölçütü bu glyph'te yanlış yöne işaret eder
            // ve doğru uygulamayı kırmızıya düşürürdü. İddia bu yüzden daha
            // mütevazı ama yine gözlenebilir: kaydırma uygulanıyor ve taban
            // fontun tersine sıfır değil.
            let advance = font::glyph_advance(&alt, glyph);
            let mut centred = vec![0u8; m.slot_bytes()];
            let mut flush = vec![0u8; m.slot_bytes()];
            assert_eq!(
                raster::draw(&alt, FALLBACK_CHAR, m, cell, 0.0, &mut centred),
                DrawResult::Drawn
            );
            assert_eq!(
                raster::draw(&alt, FALLBACK_CHAR, m, advance, 0.0, &mut flush),
                DrawResult::Drawn
            );
            assert_ne!(
                centred, flush,
                "{label}: yedek glyph kaydırılmadı (kapı {cell}, ilerleme {advance})"
            );
        }
    }

    #[test]
    fn the_gate_decides_by_ink_alone() {
        // Bekçinin sınadığı şey "bu karakter kutu mu" **değil**: o, makinede
        // hangi fontların kurulu olduğuna bağlı bir olgu ve kodun özelliği
        // değil. `U+E0B0` bu makinede `.LastResort`'a düşüyor ve reddediliyor,
        // ama Nerd Font kurulu bir makinede gerçek bir glyph'e düşer ve
        // **çizilmesi doğru olur**; beklentiyi sabite yazmak `make hepsi`'yi
        // doğru kodda kırmızıya düşürürdü.
        //
        // Sınanan şey **kapının kuralı**: adayın boyayacağı piksel hücrenin
        // içinde kalıyorsa çiziliyor, taşıyorsa kutu. Beklenti adayın kendi
        // mürekkep kutusundan türetiliyor, yani ölçüt her makinede aynı — ve
        // gözlem ile beklenti iki ayrı çağrıdan geliyor (biri `fallback_font`,
        // öteki `slot`), yani totoloji değil: kapı `slot`'un yolunda
        // koşmuyorsa bu sınama düşer.
        //
        // Beklenti **ilerlemeden** türetilseydi bu sınama iki karakterde
        // kırmızı düşerdi ([`INK_CHAR`] ile `⠋`); listede kalmalarının sebebi
        // o — ölçütün geri alınması sessiz kalmamalı.
        let mut a = atlas(POINT_SIZE, 1.0);
        let classes = size_classes(&a);
        let mut plan: Vec<(char, SizeClass, bool, String)> = Vec::new();
        for ch in GATE_PROBES {
            for (i, size) in [SizeClass::Normal, SizeClass::Small]
                .into_iter()
                .enumerate()
            {
                let (label, base, cell) = classes[i];
                // Taban fontta varsa yedek yolu hiç koşmuyor: deneyin konusu değil.
                if font::glyph_index(base, ch).is_some() {
                    continue;
                }
                // Yordamsal çizilen karakter de deneyin konusu değil: kapı
                // ondan **önce** duruyor ve font hiç sorulmuyor. Kapının
                // yüklemi burada birebir tekrarlanıyor, `is_procedural(ch)`
                // tek başına değil — `⠋` küçük sınıfta hâlâ yedek yolundan
                // geçiyor ve bu sınamada kapalı kapının tek tanığı o.
                if size == SizeClass::Normal && raster::is_procedural(ch) {
                    continue;
                }
                // Aday hiç yoksa da kapının konusu değil — reddi kapı vermiyor.
                let Some(open) =
                    font::fallback_font(base, ch, CGFloat::INFINITY, 1).map(|a| a.font)
                else {
                    continue;
                };
                let glyph = font::glyph_index(&open, ch).expect("aday çizebiliyor");
                let advance = font::glyph_advance(&open, glyph);
                // Adayın **çizileceği yerdeki** mürekkebi: kaydırma
                // `raster::draw`'in uyguladığının ta kendisi
                // (`font::centre_shift`), yoksa sınama çizilmeyecek bir
                // yerleşimi ölçerdi.
                let ink = font::glyph_ink(&open, glyph);
                let left = ink.origin.x + font::centre_shift(cell, advance);
                let right = left + ink.size.width;
                let family = unsafe { open.family_name() }.to_string();
                plan.push((
                    ch,
                    size,
                    left >= 0.0 && right <= cell,
                    format!(
                        "{label}, {family}, mürekkep {left}..{right} / hücre {cell} \
                         (ilerleme {advance})"
                    ),
                ));
            }
        }

        let (mut fits, mut wide) = (0usize, 0usize);
        for (ch, size, should_fit, why) in plan {
            let slot = a
                .slot(Sprite::Char(ch), Face::Regular, size, Half::Whole)
                .0
                .slot;
            if should_fit {
                assert_ne!(slot, TOFU, "hücreye sığan aday çizilmedi: '{ch}' ({why})");
                fits += 1;
            } else {
                assert_eq!(slot, TOFU, "hücreye sığmayan aday çizildi: '{ch}' ({why})");
                wide += 1;
            }
        }
        // Deney **boşalamaz**: kapı iki yönde de gözlenmiş olmalı. Bu satır
        // olmasaydı bütün adayların elenmesi (ya da hepsinin geçmesi) sınamayı
        // sessizce anlamsızlaştırır ve yine yeşil kalırdı.
        assert!(
            fits > 0 && wide > 0,
            "kapı tek yönde sınandı: sığan {fits}, sığmayan {wide}"
        );
        // Reddedilen aday **yuva harcamıyor**; olmasaydı bir CJK dosyası
        // atlası tüketirdi. `fits` kadar yuva + tofu bekleniyor.
        assert_eq!(
            a.occupancy().0,
            fits + 1,
            "reddedilen aday yuva harcadı (sığan {fits})"
        );
    }

    #[test]
    fn same_char_gets_same_slot() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let (placed_first, upload) = a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let first = placed_first.slot;
        assert_ne!(first, TOFU, "tanınan karakter tofu'ya düşmemeli");
        assert!(upload.is_some(), "ilk soruluşta yükleme gelmeli");
        let (placed_second, again) = a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let second = placed_second.slot;
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
        let (placed_slot, upload) = a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let slot = placed_slot.slot;
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
        let (placed__, upload) = a.slot(
            Sprite::Char('W'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let _ = placed__.slot;
        let bytes = upload.expect("ilk soruluşta yükleme gelmeli").bytes;
        assert!(bytes.iter().any(|&b| b > 0), "'W' hiç piksel boyamadı");
        // Boşluk da tanınan bir glyph'tir ama hiçbir şey boyamaz: ölçüt
        // "bitmap doldu mu" değil, "raster çalıştı mı".
        let (placed_slot, blank) = a.slot(
            Sprite::Char(' '),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let slot = placed_slot.slot;
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
        // Tofu'ya düşen çağrı yükleme **vermez**: veri dokuda zaten. Yedek
        // aramanın gelişiyle bu iddianın kapsamı büyüdü: kabul edilmeyen aday
        // tampona hiç çizilmediği için yol yine buraya iniyor, yani "rezident"
        // sözü yedek reddinde de tutuyor. Kabul edilen aday `Drawn` kolundan
        // geçer ve orada yükleme **gelir** — ikisini karıştıran bir uygulama
        // dokuya boş tampon yazardı.
        assert!(
            a.slot(
                Sprite::Char(UNKNOWN_CHAR),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .1
            .is_none(),
            "rezident yuva yeniden yüklenmez"
        );
    }

    /// Renk düzleminin 0. yuvasındaki tek hücrelik kabul `Left` isteğine de
    /// cevap: numara `TOFU` ile aynı ama kayıt ret değil. Bekçi iç tabloyu
    /// kuruyor, çünkü tek hücreye sığan renkli bir glyph bugünkü fontlarda
    /// yok — kural yine de yolun kendisi.
    #[test]
    fn color_slot_zero_answers_the_left_request() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let sprite = Sprite::Char('😀');
        let whole = (sprite, Face::Regular, SizeClass::Normal, Half::Whole);
        a.slots.insert(whole, (TOFU, Plane::Color));
        let before = (a.occupancy(), a.color_occupancy());
        let (placed, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
        assert_eq!(
            (placed.slot, placed.half, placed.plane),
            (TOFU, Half::Whole, Plane::Color),
            "renk düzleminin 0. yuvası ret sanıldı"
        );
        assert!(
            upload.is_none(),
            "kabul edilmiş glyph yeniden rasterize edildi"
        );
        assert_eq!((a.occupancy(), a.color_occupancy()), before);
    }

    #[test]
    fn unknown_char_is_cached() {
        let mut a = atlas(POINT_SIZE, 1.0);
        assert_eq!(
            a.slot(
                Sprite::Char(UNKNOWN_CHAR),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
            TOFU
        );
        // Reddin kalıcılığı: ikinci soruluşta CoreText'e gidilmemeli. Bekçi iç
        // tabloya bakıyor çünkü FFI çağrısının olup olmadığı dışarıdan
        // gözlenemiyor.
        //
        // Yedek aramanın gelişiyle bu iddia **daha pahalı** bir şeyi koruyor.
        // Eskiden önbelleklenmeyen kayıt kare başına tek bir
        // `CTFontGetGlyphsForCharacters` demekti; artık ona bir
        // `CTFontCreateForString` de ekleniyor ve o cascade'i yürüyor. Soğuk
        // ilk çağrının bedeli kare bütçesiyle karşılaştırılabilir ölçüde;
        // sayısı ve ortamı `.tasks/019-glyph-yedegi/phase-1.md` → Uygulama
        // Notları'nda emanette (ilk `/measure` onu `docs/OLCUMLER.md`'ye
        // taşır). Kaydın **ana thread'de** doğduğu yer `slot()`'un çizim
        // yolu, yani tavansız bir sızıntı değil kare başına ödenen bir
        // gecikme olurdu.
        assert_eq!(
            a.slots.get(&(
                Sprite::Char(UNKNOWN_CHAR),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )),
            Some(&(TOFU, Plane::Mask)),
            "tofu çözümü önbelleğe girmeli"
        );
        assert_eq!(a.occupancy().0, 1, "tofu düşüşü yuva harcamamalı");
    }

    #[test]
    fn face_fallback_is_cached_under_the_requested_face() {
        // `╱` (U+2571) **ölçüldü** (bu makine, macOS 26.4.1, Menlo 13pt):
        // düz yüzde var, kalın yüzde yok. Yani glyph düzeyindeki geri düşüş
        // gerçek bir fontla ateşlenebiliyor.
        //
        // Fikstür **köşegen olmak zorunda** ve bu bir tesadüf değil: aynı
        // ölçüm Menlo Bold'da eksik olan kod noktalarını da saydı ve BMP ile
        // SMP'nin tamamında **tek** bir blok çıktı — U+2500–U+257F, tam 128
        // karakter. O bloğun tamamı 021'in kapsamında, yalnız üç köşegeni
        // (`╱╲╳`, Karar 3B) bilerek dışarıda. Yani bu sınamanın taşıyıcı
        // iddiasını ayakta tutan şey kapsamın o deliği: delik kapansaydı
        // `DrawResult::NoGlyph if face != Face::Regular` kolunun bu makinede
        // **hiç** bekçisi kalmazdı ve kol sessizce ölürdü.
        //
        // Bir dönem fikstür `─` (U+2500) idi ve doc'u "kalın bir TUI
        // çerçevesi bu koldan geçiyor" diyordu; artık geçmiyor, çerçeve
        // yordamsal çiziliyor ve `(Char('─'), Bold)` anahtarı hiç oluşmuyor.
        // Değiştirilmeseydi `bold == regular` ile `occupancy == 2` yeşil
        // kalır, sınama hiçbir şey sınamadan yaşardı.
        const FACE_LADDER_PROBE: char = '╱';
        let mut a = atlas(POINT_SIZE, 1.0);
        let regular = a
            .slot(
                Sprite::Char(FACE_LADDER_PROBE),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
        let bold = a
            .slot(
                Sprite::Char(FACE_LADDER_PROBE),
                Face::Bold,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;

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
            a.slots.get(&(
                Sprite::Char(FACE_LADDER_PROBE),
                Face::Bold,
                SizeClass::Normal,
                Half::Whole
            )),
            Some(&(regular, Plane::Mask)),
            "geri düşüş istenen yüzün anahtarıyla önbelleğe girmeli"
        );
        assert_eq!(a.occupancy().0, 2, "geri düşüş ikinci bir yuva harcadı");
    }

    /// Yordamsal ailenin karakterleri — **tek kaynak**, tarama BMP'nin
    /// tamamı ve süzgeç `raster::is_procedural`.
    ///
    /// Dar bir aralık yazmak implementasyonun tablosunu aynalamak olurdu;
    /// aynalanmış tablo kendi eksiğini göremez (021'in kol tablosu dersi).
    fn procedural_chars() -> impl Iterator<Item = char> {
        (0u32..=0xFFFF)
            .filter_map(char::from_u32)
            .filter(|&ch| raster::is_procedural(ch))
    }

    /// Ailenin atlastan istediği yuva sayısı — **kapasiteyle doğrudan
    /// karşılaştırılabilir** hâli.
    ///
    /// Karakterlerin eline geçen yuva `capacity()` değil: `Atlas::slot`
    /// onlara `capacity() - RULE_RESERVE` veriyor ve `next` tofu ayrıldığı
    /// için **1'den** başlıyor. Yani aile sığsın diye kapasitenin ailenin
    /// boyundan `1 + RULE_RESERVE` fazla olması gerekiyor. Bu yedi yuvayı
    /// saymamak değişmezi sessizce gevşetirdi ve son birkaç Braille
    /// karakteri kutu kalırken bekçi yeşil geçerdi.
    fn procedural_family_size() -> usize {
        procedural_chars().count() + 1 + usize::from(RULE_RESERVE)
    }

    /// Varsayılan yol **bit bit aynı** kalmalı (022 R2).
    ///
    /// Kenarın türetilmesi ancak hücre büyüdüğünde devreye giriyor; varsayılan
    /// punto zaten [`SLOT_TARGET`]'ın katbekat üstünde. Bu bekçi olmasaydı
    /// [`MIN_EDGE`] ya da [`SLOT_TARGET`] oynayınca varsayılan kullanıcının
    /// ızgarası, dokusu ve **rasteri** sessizce değişirdi.
    #[test]
    fn the_default_size_keeps_todays_texture() {
        let a = atlas(POINT_SIZE, 2.0);
        assert_eq!(
            edge_for(a.metrics.cell_px.0, a.metrics.cell_px.1),
            MIN_EDGE,
            "varsayılan punto tabanda kalmalı"
        );
        // Ölçülmüş sayı: `docs/OLCUMLER.md` → Atlas yuva ayak izi, 13pt@2x.
        assert_eq!(a.occupancy().1, 1984, "13pt@2x kapasitesi değişti");
    }

    /// Değişmez: **kabul edilen her ölçüde** kapasite yordamsal ailenin
    /// üstünde (022 R3).
    ///
    /// Bu bekçi 021'in doyma tablosunun yerine geçiyor: tablo bir gözlemdi,
    /// bu bir sözleşme. Aile + tofu = 422 yuva (`docs/OLCUMLER.md`); sayı
    /// burada **sabit olarak değil** `raster::is_procedural`'dan sayılarak
    /// türetiliyor, yani aileye karakter eklenirse bekçi kendiliğinden
    /// sıkılaşıyor.
    #[test]
    fn capacity_clears_the_family_at_every_accepted_size() {
        // **Aralık aynalanmıyor:** tarama BMP'nin tamamı ve süzgeç
        // `raster::is_procedural`'ın kendisi. Dar bir tarama aralığı
        // implementasyonun tablosunun ikinci kopyası olurdu ve aileye yeni
        // bir blok eklenince (Legacy Computing, U+1FB00–1FBFF — yol
        // haritasının sıradaki adayı) bekçi yeşil kalırdı: tam da 021'in
        // uyardığı sessiz ayrışma.
        let family = procedural_family_size();
        // Punto × ölçek çarpımı [`MIN_POINT_SIZE`]..[`MAX_POINT_SIZE`]
        // aralığına oturuyor, yani köşeyi kuran şey çarpımın tavanı ve
        // satır aralığının tavanı.
        // **İki eksen.** Hücre ölçüsü yalnız punto/ölçek/satır aralığından
        // değil **aileden** de geliyor ve kullanıcının ailesi reddedilmiyor,
        // yalnız uyarı alıyor (`proportional_family_opens_with_a_warning`).
        // Tek eksenli bir bekçi, tam da bu setin sözleşmeye çevirdiği kusuru
        // ikinci eksenden kaçırırdı.
        for family_name in [None, Some("Helvetica")] {
            for point_size in [MIN_POINT_SIZE, 13.0, 29.0, 56.0, MAX_POINT_SIZE] {
                for scale in [1.0, 2.0] {
                    for line_height in [1.0, LARGEST_LINE_HEIGHT] {
                        let a = Atlas::new(family_name, point_size, scale, line_height);
                        let total = a.occupancy().1;
                        assert!(
                            total >= family,
                            "{family_name:?} {point_size}pt@{scale}x \
                             lh={line_height}: kapasite {total} < aile {family}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn full_atlas_returns_tofu_without_caching() {
        // En küçük kapasiteyi veren köşe: en büyük punto **ve** en büyük
        // satır aralığı. Kenar tavana ([`MAX_EDGE`]) çarpıp orada duruyor,
        // yani kapasite burada dibini buluyor.
        let mut a = Atlas::new(None, LARGE_POINT_SIZE, 1.0, LARGEST_LINE_HEIGHT);
        let (used, total) = a.occupancy();
        assert_eq!(used, 1, "yeni atlasta yalnız tofu ayrılmış olmalı");
        // Havuz iki kümeden: yordamsal aile (fonta sorulmadan çizildiği için
        // **her zaman** yuva harcıyor) ve yazdırılabilir ASCII. Kapasite
        // artık hücre ölçüsünden türüdüğü için tek başına ASCII yetmiyor —
        // ve tofu'ya düşen karakter yuva **harcamıyor** (negatif önbellek),
        // yani havuz gerçekten çizilebilen karakterlerden kurulmak zorunda.
        // Aralık tablosu **aynalanmıyor**, süzgeç `raster::is_procedural`'ın
        // kendisi: ikinci bir kopya sessizce kayardı.
        // ASCII dört yüzde de ayrı yuva tutuyor; yordamsal aile `Regular`'a
        // normalize olduğu için **tek** kez sayılıyor (`Atlas::slot`).
        let pool: Vec<(char, Face)> = procedural_chars()
            .map(|ch| (ch, Face::Regular))
            .chain(
                [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic]
                    .into_iter()
                    .flat_map(|f| (' '..='~').map(move |ch| (ch, f))),
            )
            .collect();
        assert!(
            pool.len() > total,
            "sınama havuzu kapasiteyi aşmalı: havuz={} kapasite={total}",
            pool.len()
        );
        let dropped: Vec<(char, Face)> = pool
            .iter()
            .copied()
            .filter(|&(ch, face)| {
                a.slot(Sprite::Char(ch), face, SizeClass::Normal, Half::Whole)
                    .0
                    .slot
                    == TOFU
            })
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
        let rule = a
            .slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
        assert_ne!(rule, TOFU, "dolu atlasta kural sprite'ı tofu'ya düştü");
        // Dolu atlas geçici bir hâl: aynı karakter başka bir puntoda yuva
        // bulabilir, yani tofu'ya bağlı kalmamaları gerekiyor.
        for (ch, face) in dropped {
            assert!(
                !a.slots
                    .contains_key(&(Sprite::Char(ch), face, SizeClass::Normal, Half::Whole)),
                "'{ch}' ({face:?}) kalıcı olarak tofu'ya yazılmış"
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
        let (placed__, upload) = a.slot(
            Sprite::Char('W'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let _ = placed__.slot;
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

        // (4) **`1.0` yeniden üretilebilir.** Aynı dörtlü aynı metriği
        //     veriyor; `metrics()` saf, gizli bir duruma bağlı değil.
        //
        //     Bu satır bir dönem "`1.0` no-op" diye okunuyordu ve o iddia
        //     **yanlıştı**: `tight` da `1.0` ile kuruluyor, yani karşılaştırma
        //     totolojiydi. 019'un kapısında ölçüldü — `1.0`'da
        //     `extra = round_up(natural * 0.0)` ve `round_up`'ın tabanı 1,
        //     yani varsayılan yol hücreye **bir piksel ekliyor** (Menlo 13pt:
        //     font 17 istiyor, hücre 18 oluyor). Fazlalık alta düşüyor, taban
        //     çizgisi oynamıyor; belirti bir piksel fazla satır aralığı.
        //     Düzeltmesi bu setin dışında ve `docs/YOL-HARITASI.md`'de borç:
        //     her kullanıcının ızgarasını oynatır, yani ürün kararı.
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
        let (placed__, upload) = a.slot(
            Sprite::Char('g'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let _ = placed__.slot;
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
        let (placed__, upload) = a.slot(
            Sprite::Char('g'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let _ = placed__.slot;
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
        let (placed_letter, _) = a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let letter = placed_letter.slot;
        assert_ne!(letter, TOFU, "'A' Menlo'da var");

        // Tanınmayan karakter yuva harcamıyor, yani `next` onu
        // sınırlamıyor. Tavan olmasaydı harita gördüğü ayrı codepoint sayısı
        // kadar büyürdü ve bir ikili dosyayı `cat`'lemek bunu gerçek bir yola
        // çevirir. Crate'in tavanı olmayan tek sayısı burasıydı.
        //
        // **Havuz yedek aramadan sonra da çalışıyor**, ama artık her kayıt
        // bir `CTFontCreateForString` ödüyor; havuzun bedeli ölçüldü ve
        // daraltmak gerekmedi, sayısı ve ortamı
        // `.tasks/019-glyph-yedegi/phase-1.md` → Uygulama Notları'nda emanette.
        //
        // Havuz **filtreli** ve bu bir kolaylık değil zorunluluk: kapı
        // ilerlemeyi ölçerken CJK'nın tamamı dönüyordu, mürekkebi ölçerken
        // dar boyayan üyeleri (`丨` U+4E28 bir dikey çubuk) geçiyor ve yuva
        // alıyor. Deneyin konusu negatif önbellek, yani havuza yalnız
        // gerçekten reddedilenler giriyor; kapının kendi bekçisi
        // [`the_gate_decides_by_ink_alone`] ve filtre onun cevabını
        // sormaktan ibaret.
        let pool: Vec<char> = {
            let (_, base, cell) = size_classes(&a)[0];
            ('\u{4e00}'..'\u{9fff}')
                .filter(|&ch| font::fallback_font(base, ch, cell, 1).is_none())
                .take(cap * 3)
                .collect()
        };
        assert!(pool.len() > cap, "havuz tavanı aşmalı");
        for &ch in &pool {
            assert_eq!(
                a.slot(
                    Sprite::Char(ch),
                    Face::Regular,
                    SizeClass::Normal,
                    Half::Whole
                )
                .0
                .slot,
                TOFU,
                "'{ch}' Menlo/SF Mono'da yok ve yedeği hücreye sığmıyor"
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
            a.slots.get(&(
                Sprite::Char(last),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )),
            Some(&(TOFU, Plane::Mask)),
            "tahliyeden sonraki kayıt önbelleğe girmeli"
        );
        // Pozitif kayıt tahliyeye girmiyor: yuvası duruyor.
        assert_eq!(
            a.slot(
                Sprite::Char('A'),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
            letter,
            "pozitif kayıt tahliyede kayboldu"
        );
    }

    #[test]
    fn non_bmp_char_path_works() {
        // Surrogate çifti: `encode_utf16` iki birim üretiyor, CoreText ikinci
        // birime de dokunuyor ve glyph üretmeyip `false` dönüyor.
        // `font::glyph_index` o dönüşü bilerek yok sayıyor ve işaretçilerini
        // dilimden türetiyor; ikisinin gerekçesi de ancak bu yol koşarsa
        // sınanmış olur.
        //
        // Yedek aramanın gelişiyle BMP dışı yol **iki yerden** geçiyor ve
        // ikincisi yeni: `font::fallback_font`'un `CFRange`'i de UTF-16 birimi
        // sayıyor, yani `len_utf16` yerine `1` yazılsaydı vekil çiftinin
        // yarısı istenir ve cascade yanlış karakteri arardı. Bu sınama artık
        // o aralığın da bekçisi — sonuç yine tofu, ama sebebi uzadı: aday
        // (STIX Two Math) bulunuyor ve genişlik kapısından dönüyor (1.07×).
        let mut a = atlas(POINT_SIZE, 1.0);
        assert_eq!(
            a.slot(
                Sprite::Char('𝔸'),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
            TOFU,
            "Menlo/SF Mono matematik alfabesi içermez, yedeği de hücreye sığmaz"
        );
        // BMP dışı bir karakter kapıyı **geçebilse** aynı aralık çizim yolunda
        // da doğru olmak zorunda; `\u{10FFFD}` (.LastResort, 1.83×) ile `𝔸`
        // aynı kolun iki ucu ve ikisi de aday **buluyor** — aralık bozuk
        // olsaydı aday hiç bulunmazdı ve bu sınama yine yeşil kalırdı.
        let (label, base, _) = size_classes(&a)[0];
        assert!(
            font::fallback_font(base, '𝔸', CGFloat::INFINITY, 1).is_some(),
            "{label}: BMP dışı karakter için aday bulunamadı — `CFRange` şüpheli"
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
            // Tavan artık [`MAX_EDGE`]: kenar hedefe göre katlanabiliyor ama
            // orada duruyor. Uç girdiler (NaN, sonsuz, 1e9) puntoyu
            // aralığa oturttuğu için buraya da sonlu bir doku düşmeli.
            assert!(
                tw <= MAX_EDGE && th <= MAX_EDGE,
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
        a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
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
        a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        assert!(
            !a.ensure(None, POINT_SIZE, 1.0, 1.0),
            "aynı anahtar yeniden kurmamalı"
        );
        assert!(
            a.ensure(Some("Monaco"), POINT_SIZE, 1.0, 1.0),
            "aile değişti: yeniden kurulmalı"
        );
        assert_eq!(a.occupancy().0, 1, "yeni atlasta yalnız tofu");
        a.slot(
            Sprite::Char('A'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
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
    fn monospaced_families_are_the_ones_the_chain_accepts() {
        // Ayar penceresinin Font listesi: seçilebilen her aile zincirden
        // uyarısız açılır — ölçüt `open_chain`'inkiyle aynı.
        let families = monospaced_families();
        assert!(families.iter().any(|f| f == "Menlo"), "{families:?}");
        assert!(!families.iter().any(|f| f == "Helvetica"), "{families:?}");
        assert!(!families.iter().any(|f| f.starts_with('.')), "{families:?}");
        assert!(
            families
                .windows(2)
                .all(|pair| pair[0].to_lowercase() <= pair[1].to_lowercase()),
            "{families:?}"
        );
        for family in &families {
            let (_, issue) = font::open_chain(Some(family), POINT_SIZE);
            assert_eq!(issue, None, "{family}");
        }
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
        // Doku ızgarayı sarmalı ve kenarda bir hücreden fazlası boşa
        // gitmemeli. Kenar artık türetilmiş, yani sabite değil **ızgaranın
        // kendi kenarına** bakılıyor: satır/sütun sayısı ile hücre ölçüsünün
        // çarpımı dokuyu vermeli ve bir hücre daha eklenince kenarı aşmalı.
        // Eski sınama dokuyu sabit `TEXTURE_EDGE`'e bağlıyordu; kenar
        // türetildiğine göre bağlanacak yer `edge_for`. **Asıl sınır bu**:
        // doku türetilen kenarı aşarsa `slot_origin` son sütunun ötesini
        // gösterir ve `replaceRegion` satırın dışına yazar — belirti sessiz.
        let (tw, th) = a.texture_px();
        let edge = edge_for(w, h);
        assert!(
            tw <= edge && th <= edge,
            "doku kenarı aşıyor: {tw}×{th} > {edge}"
        );
        // Ve bir hücreden fazlası boşa gitmiyor.
        assert!(
            tw + w > edge && th + h > edge,
            "artık şerit bir hücreden büyük: {tw}×{th}, kenar {edge}"
        );
    }

    /// Yuvanın baytlarını kopyalar — `Upload` ödüncü atlası kilitliyor.
    fn slot_bytes_of(a: &mut Atlas, sprite: Sprite, face: Face) -> Vec<u8> {
        let (_, upload) = a.slot(sprite, face, SizeClass::Normal, Half::Whole);
        upload.expect("yeni yuva yükleme vermeli").bytes.to_vec()
    }

    #[test]
    fn bold_face_gets_own_slot() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let mut slots = Vec::new();
        for face in [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic] {
            let slot = a
                .slot(Sprite::Char('M'), face, SizeClass::Normal, Half::Whole)
                .0
                .slot;
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
        let (placed_small_slot, small_upload) = a.slot(
            Sprite::Char('M'),
            Face::Regular,
            SizeClass::Small,
            Half::Whole,
        );
        let small_slot = placed_small_slot.slot;
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
            a.slot(
                Sprite::Char('M'),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
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
                SizeClass::Small,
                Half::Whole
            )
            .0
            .slot,
            a.slot(
                Sprite::Rule(RuleKind::Single),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole
            )
            .0
            .slot,
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
                Half::Whole,
            )
            .0
            .slot;
        // Çağıran yanılıp yüz verse bile normalizasyon aynı yuvaya götürür;
        // yoksa altı çeşit dört yüzle yirmi dört yuva harcardı.
        let bold = a
            .slot(
                Sprite::Rule(RuleKind::Single),
                Face::BoldItalic,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
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

    /// Yordamsal değişmezlerin koştuğu (punto, ölçek) çiftleri.
    ///
    /// Üçü de gerekli ve her biri başka bir aritmetiği açıyor (ölçüler bu
    /// makinede, Menlo): 13pt@1x hücresi 8×18 — sekizde bir dilimleri
    /// kesirli düşüyor (18/8 = 2.25) ve kenar yumuşatması gerçekten koşuyor;
    /// 13pt@2x 16×33, yani **tek** yükseklik, yarım da kesire iniyor;
    /// 144pt@1x ise 87×169, dilimlerin çoğunun tam bölündüğü büyük hücre. Tek çiftte koşan bir
    /// değişmez ötekini hiç sınamamış olur — `envelope_stays_inside_cell`'in
    /// doc'undaki ders ("gerçek fontla kırpma dalı hiç ateşlenmiyor").
    const PROCEDURAL_SIZES: [(f64, f64); 3] = [
        (POINT_SIZE, 1.0),
        (POINT_SIZE, 2.0),
        (LARGE_POINT_SIZE, 1.0),
    ];

    /// Yordamsal sprite'ın baytları — `Atlas::slot`'tan **değil**, doğrudan.
    ///
    /// `LARGE_POINT_SIZE`'ta kapasite birkaç düzine yuva ve tek başına 256
    /// Braille deseni oraya sığmıyor: `slot()` üzerinden koşan bir değişmez
    /// sınaması tofu'ya düşer, `Upload` hiç gelmez ve bekçi geometriyi değil
    /// kapasiteyi sınamış olurdu. Kapının `slot()` yolunda gerçekten
    /// koştuğunu gösteren bekçiler ayrı ve 13pt'de
    /// ([`procedural_chars_share_one_slot_across_faces`],
    /// [`the_small_class_still_asks_the_font`]).
    fn procedural(m: Metrics, ch: char) -> Vec<u8> {
        let mut bytes = vec![0u8; m.slot_bytes()];
        raster::draw_procedural(ch, m, &mut bytes);
        bytes
    }

    /// İki sprite'ın piksel-piksel doygun toplamı.
    fn saturating_sum(a: &[u8], b: &[u8]) -> Vec<u8> {
        a.iter().zip(b).map(|(x, y)| x.saturating_add(*y)).collect()
    }

    /// İki sprite'ın piksel-max'i.
    fn pixel_max(a: &[u8], b: &[u8]) -> Vec<u8> {
        a.iter().zip(b).map(|(x, y)| *x.max(y)).collect()
    }

    #[test]
    fn the_full_block_fills_the_cell() {
        // **Bildirilen kusurun tam tersi**, `> 0` değil eşitlik: Menlo'nun
        // `█`'i 8×18 hücrenin yalnız 3–16 satırlarını boyuyor ve alt alta iki
        // blok arasında ~5 piksel şerit kalıyordu (019 phase-2, kullanıcı
        // ekran görüntüsüyle bildirdi). Tek bir eksik bayt o şeridin sönük
        // kopyasıdır, yani ölçüt "hiç mürekkep var mı" olamaz.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let bytes = procedural(m, '\u{2588}');
            let (w, _) = m.cell_wh();
            let gap = bytes.iter().position(|&b| b != 255);
            assert!(
                gap.is_none(),
                "{point_size}pt@{scale}x: `█` hücreyi doldurmadı, ilk eksik piksel \
                 ({}, {}) = {}",
                gap.unwrap_or(0) % w,
                gap.unwrap_or(0) / w,
                bytes[gap.unwrap_or(0)]
            );
        }
    }

    #[test]
    fn disjoint_blocks_tile_the_cell() {
        // Ayrık parçaların birleşimi **tam** kapsama vermek zorunda ve ölçüt
        // doygun toplam: 13pt@2x'in h = 33'ünde yarım 16.5'e düşüyor, iki
        // komşu parça o satıra 128'er bırakıyor. `max` alsaydı hücrenin **ortasında**
        // %50'lik bir şerit kalırdı — bu setin kapatmaya geldiği kusurun
        // hücre içine taşınmış hâli, ve `> 0` sınayan bir bekçi onu görmezdi.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let full = procedural(m, '\u{2588}');
            for (a, b, name) in [
                ('\u{2580}', '\u{2584}', "üst/alt yarım"),
                ('\u{258C}', '\u{2590}', "sol/sağ yarım"),
            ] {
                assert_eq!(
                    saturating_sum(&procedural(m, a), &procedural(m, b)),
                    full,
                    "{point_size}pt@{scale}x: {name} `█`'i vermedi"
                );
            }
            // Dört çeyrek de aynı yasayı taşıyor ve ayrıca **orta dikişi**
            // görüyor: yatay ile dikey kesirli satır/sütun aynı karede.
            let quarters = ['\u{2598}', '\u{259D}', '\u{2596}', '\u{2597}'];
            let union = quarters.iter().fold(vec![0u8; m.slot_bytes()], |acc, &ch| {
                saturating_sum(&acc, &procedural(m, ch))
            });
            assert_eq!(
                union, full,
                "{point_size}pt@{scale}x: dört çeyrek `█`'i vermedi"
            );
        }
    }

    #[test]
    fn the_eighth_ladders_are_nested() {
        // İki merdiven, iki yön: alttan `▁..█` kod noktası **artarken**
        // büyüyor, soldan `▏..▉` kod noktası **azalırken**. İkinci yön
        // Unicode'un kendi sıralaması ve tam da orada bir işaret hatası
        // sessiz kalırdı — merdiven yine merdiven görünür, yalnız ters.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let full = procedural(m, '\u{2588}');
            for (name, steps) in [
                ("alt", (0x2581..=0x2588).collect::<Vec<u32>>()),
                ("sol", (0x2589..=0x258F).rev().collect::<Vec<u32>>()),
            ] {
                let mut previous = vec![0u8; m.slot_bytes()];
                let mut previous_ink = 0u64;
                for cp in steps {
                    let ch = char::from_u32(cp).expect("blok kod noktası");
                    let bytes = procedural(m, ch);
                    // **İç içe**: her basamak bir öncekini kapsıyor.
                    assert!(
                        bytes.iter().zip(&previous).all(|(b, p)| b >= p),
                        "{point_size}pt@{scale}x: {name} merdiveni U+{cp:04X}'te geri gitti"
                    );
                    // Ve gerçekten **büyüyor**: hepsini aynı çizen bir kod
                    // iç içelik sınamasından geçerdi.
                    let ink: u64 = bytes.iter().map(|&b| u64::from(b)).sum();
                    assert!(
                        ink > previous_ink,
                        "{point_size}pt@{scale}x: {name} merdiveni U+{cp:04X}'te büyümedi \
                         ({previous_ink} → {ink})"
                    );
                    previous = bytes;
                    previous_ink = ink;
                }
                // Merdivenin son basamağı dolu blok: `▉` sol yedi sekizde
                // değil, `2589..=258F` tersten yürüdüğü için son adım
                // yedi sekizde kalıyor — o yüzden yalnız alt merdiven
                // karşılaştırılıyor.
                if name == "alt" {
                    assert_eq!(
                        previous, full,
                        "{point_size}pt@{scale}x: alt merdiven `█`'e varmadı"
                    );
                }
            }
        }
    }

    /// Çeyrek ve sekizde bir bloklarının Unicode adları
    /// (`unicodedata`, UCD 16.0), `raster::QUADRANTS`'ın oracle'ı.
    ///
    /// Çizgi ailesiyle aynı gerekçe ([`LINE_NAMES`]): bu da el yazması bir
    /// tablo ve geometriye bakan hiçbir değişmez "doğru geometri, yanlış
    /// karakter"i göremez — `▙` ile `▟`'nin maskeleri yer değiştirse dört
    /// çeyreğin birleşimi hâlâ `█` olurdu.
    // `rustfmt::skip`: hizalı ad yorumları tablonun gözle taranabilir
    // olmasının tek sebebi.
    #[rustfmt::skip]
    const QUARTER_NAMES: [(char, &str); 12] = [
        ('▔', "UPPER ONE EIGHTH BLOCK"),
        ('▕', "RIGHT ONE EIGHTH BLOCK"),
        ('▖', "QUADRANT LOWER LEFT"),
        ('▗', "QUADRANT LOWER RIGHT"),
        ('▘', "QUADRANT UPPER LEFT"),
        ('▙', "QUADRANT UPPER LEFT AND LOWER LEFT AND LOWER RIGHT"),
        ('▚', "QUADRANT UPPER LEFT AND LOWER RIGHT"),
        ('▛', "QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER LEFT"),
        ('▜', "QUADRANT UPPER LEFT AND UPPER RIGHT AND LOWER RIGHT"),
        ('▝', "QUADRANT UPPER RIGHT"),
        ('▞', "QUADRANT UPPER RIGHT AND LOWER LEFT"),
        ('▟', "QUADRANT UPPER RIGHT AND LOWER LEFT AND LOWER RIGHT"),
    ];

    #[test]
    fn the_quadrants_come_from_the_unicode_names() {
        // İki iddia, ikisi de addan: **tek** çeyrekler adlarının söylediği
        // çeyrekte duruyor (mürekkep orada, başka yerde değil) ve
        // **bileşik** olanlar adlarında sayılan tek çeyreklerin doygun
        // toplamı. Birincisi aynalamayı, ikincisi maske hatasını görüyor;
        // yalnız ikincisi yazılsaydı `▘` ile `▝` takası her iki sınamadan
        // da geçerdi, çünkü bileşikler de aynı takas edilmiş tekleri
        // kullanırdı.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            let singles = |name: &str| -> Vec<char> {
                name.trim_start_matches("QUADRANT ")
                    .split(" AND ")
                    .map(|quarter| match quarter {
                        "UPPER LEFT" => '▘',
                        "UPPER RIGHT" => '▝',
                        "LOWER LEFT" => '▖',
                        "LOWER RIGHT" => '▗',
                        other => panic!("tanınmayan çeyrek: {other}"),
                    })
                    .collect()
            };
            for (ch, name) in QUARTER_NAMES {
                let bytes = procedural(m, ch);
                assert!(
                    bytes.iter().any(|&b| b > 0),
                    "{point_size}pt@{scale}x: '{ch}' ({name}) hiç piksel boyamadı"
                );
                // Adın çizdiği kutu: sekizde birler kendi şeritleri,
                // çeyrekler kendi çeyrekleri, bileşikler bütün hücre.
                let (x0, x1, y0, y1) = match name {
                    "UPPER ONE EIGHTH BLOCK" => (0, w, 0, h.div_ceil(8)),
                    "RIGHT ONE EIGHTH BLOCK" => (w - w.div_ceil(8), w, 0, h),
                    _ if name.contains(" AND ") => (0, w, 0, h),
                    _ => {
                        let left = name.ends_with("LEFT");
                        let upper = name.contains("UPPER");
                        (
                            if left { 0 } else { w / 2 },
                            if left { w.div_ceil(2) } else { w },
                            if upper { 0 } else { h / 2 },
                            if upper { h.div_ceil(2) } else { h },
                        )
                    }
                };
                for y in 0..h {
                    for x in 0..w {
                        let outside = x < x0 || x >= x1 || y < y0 || y >= y1;
                        assert!(
                            !outside || bytes[y * w + x] == 0,
                            "{point_size}pt@{scale}x: '{ch}' ({name}) ({x}, {y}) \
                             pikselini boyadı — adının kutusunun dışında"
                        );
                    }
                }
                if name.contains(" AND ") {
                    let expected = singles(name)
                        .into_iter()
                        .fold(vec![0u8; m.slot_bytes()], |acc, quarter| {
                            saturating_sum(&acc, &procedural(m, quarter))
                        });
                    assert_eq!(
                        bytes, expected,
                        "{point_size}pt@{scale}x: '{ch}' ({name}) adındaki \
                         çeyreklerin toplamı değil"
                    );
                }
            }
        }
    }

    #[test]
    fn the_shades_are_flat_and_ordered() {
        // Gölgeler **desensiz** (bkz. `raster`'ın `SHADE_LEVELS` doc'u):
        // dama deseni ancak adım hücrenin iki ölçüsünü de bölerse döşer ve
        // bölmüyor: bu makinede 13pt@2x hücresi 16×33 ve 33 tek. Düz kapsama döşemeyi
        // inşaen veriyor ve bekçisi bu: her gölge tek değerli.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let mut previous = 0u8;
            for cp in 0x2591..=0x2593u32 {
                let ch = char::from_u32(cp).expect("gölge kod noktası");
                let bytes = procedural(m, ch);
                let first = bytes[0];
                assert!(
                    bytes.iter().all(|&b| b == first),
                    "{point_size}pt@{scale}x: U+{cp:04X} düz değil, desen döşemede kırılır"
                );
                assert!(
                    first > previous,
                    "{point_size}pt@{scale}x: U+{cp:04X} bir öncekinden koyu değil \
                     ({previous} → {first})"
                );
                previous = first;
            }
            assert!(previous < 255, "en koyu gölge dolu bloğa eşit olmamalı");
        }
    }

    #[test]
    fn braille_dots_come_from_the_code_point_bits() {
        // **Tablo yok**: alt 8 bit doğrudan nokta maskesi. Bekçi de tablosuz
        // — beklentiyi 256 desen için tek tek yazmak yerine sekiz **tek
        // noktanın** sprite'larından türetiyor, yani uygulamanın kendi
        // eşlemesini okumuyor.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let blank = procedural(m, '\u{2800}');
            assert!(
                blank.iter().all(|&b| b == 0),
                "{point_size}pt@{scale}x: boş Braille deseni mürekkep bıraktı"
            );

            let dots: Vec<Vec<u8>> = (0..8)
                .map(|bit| {
                    let ch = char::from_u32(0x2800 | (1u32 << bit)).expect("Braille kod noktası");
                    procedural(m, ch)
                })
                .collect();
            for (bit, dot) in dots.iter().enumerate() {
                assert!(
                    dot.iter().any(|&b| b > 0),
                    "{point_size}pt@{scale}x: bit {bit} hiç piksel boyamadı"
                );
            }
            // **Destekler ayrık**: iki nokta aynı piksele değseydi 2×4
            // ızgarası birbirine akar ve desen okunamazdı.
            for i in 0..8 {
                for j in i + 1..8 {
                    let touching = dots[i].iter().zip(&dots[j]).any(|(a, b)| *a > 0 && *b > 0);
                    assert!(
                        !touching,
                        "{point_size}pt@{scale}x: bit {i} ile bit {j} aynı piksele değdi"
                    );
                }
            }
            // Birleşim yasası, 256 desenin **hepsinde**.
            for mask in 0u32..=0xFF {
                let ch = char::from_u32(0x2800 | mask).expect("Braille kod noktası");
                let expected = (0..8)
                    .filter(|bit| mask & (1 << bit) != 0)
                    .fold(vec![0u8; m.slot_bytes()], |acc, bit| {
                        pixel_max(&acc, &dots[bit])
                    });
                assert_eq!(
                    procedural(m, ch),
                    expected,
                    "{point_size}pt@{scale}x: U+{:04X} bitlerinin birleşimi değil",
                    0x2800 | mask
                );
            }
        }
    }

    #[test]
    fn procedural_chars_share_one_slot_across_faces() {
        // Unicode ince/kalın ayrımını karakterin kendisinde taşıyor, yani SGR
        // bold'un bloğu kalınlaştırması bilginin iki kez kodlanması olurdu.
        // Yan kazanç ölçülebilir: dört yüz tek yuva.
        let mut a = atlas(POINT_SIZE, 1.0);
        let regular = a
            .slot(
                Sprite::Char('\u{2588}'),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            )
            .0
            .slot;
        assert_ne!(regular, TOFU, "yordamsal karakter tofu'ya düştü");
        for face in [Face::Bold, Face::Italic, Face::BoldItalic] {
            assert_eq!(
                a.slot(
                    Sprite::Char('\u{2588}'),
                    face,
                    SizeClass::Normal,
                    Half::Whole
                )
                .0
                .slot,
                regular,
                "{face:?} ayrı yuva tuttu"
            );
        }
        assert_eq!(a.occupancy().0, 2, "tofu + tek yuva bekleniyordu");
    }

    #[test]
    fn the_small_class_still_asks_the_font() {
        // Kapı küçük sınıfta **kapalı** ve gerekçe döşeme değil ölçü
        // ayrışması: `Metrics` büyük hücrenin, yani yordamsal sprite büyük
        // hücre genişliğinde çizilir; dock'un bağlam satırının sütun adımı
        // ise küçük yüzün ilerlemesi (`Frame::column_px`). Hücreyi tam
        // dolduran bir sprite orada komşusunun üstüne binerdi — ve bağlam
        // satırı yol ile dal taşıyor, ikisi de kullanıcı verisi.
        let mut a = atlas(POINT_SIZE, 1.0);
        let (placed_normal, normal) = a.slot(
            Sprite::Char('\u{2588}'),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        let normal_slot = placed_normal.slot;
        assert!(
            normal
                .expect("yeni yuva yükleme vermeli")
                .bytes
                .iter()
                .all(|&b| b == 255),
            "büyük sınıfta kapı açılmadı"
        );
        let (placed_small_slot, small) = a.slot(
            Sprite::Char('\u{2588}'),
            Face::Regular,
            SizeClass::Small,
            Half::Whole,
        );
        let small_slot = placed_small_slot.slot;
        let small = small.expect("yeni yuva yükleme vermeli").bytes.to_vec();
        assert_ne!(
            small_slot, normal_slot,
            "küçük sınıf büyüğün yuvasını paylaştı"
        );
        // Menlo'nun `█`'i hücreyi doldurmuyor — setin varlık sebebi tam bu.
        // Yani "tamamı 255 değil" burada fontun imzası.
        assert!(
            small.iter().any(|&b| b != 255),
            "küçük sınıfta kapı açıldı: sprite yordamsal çizilmiş"
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

    /// Çizgi ailesinin kol tablosunun **ikinci kopyası** ve kaynağı ayrı:
    /// bu liste karakterlerin Unicode adları (`unicodedata`, UCD 16.0;
    /// `BOX DRAWINGS ` öneki atılmış), `raster::LINES` ise geometriden
    /// yazılmış kol kümeleri. Uygulamanın tablosunu okuyan bir sınama hiçbir
    /// şey kanıtlamazdı ve kaçırdığı şeyin adı var: **doğru geometri, yanlış
    /// karakter** — aynalanmış ya da kaydırılmış bir tabloda her sprite
    /// kusursuz görünür, yalnız yanlış kod noktasında durur.
    // `rustfmt::skip`: hizalı ad yorumları tablonun gözle taranabilir
    // olmasının tek sebebi.
    #[rustfmt::skip]
    const LINE_NAMES: [&str; 128] = [
        "LIGHT HORIZONTAL",                            // ─
        "HEAVY HORIZONTAL",                            // ━
        "LIGHT VERTICAL",                              // │
        "HEAVY VERTICAL",                              // ┃
        "LIGHT TRIPLE DASH HORIZONTAL",                // ┄
        "HEAVY TRIPLE DASH HORIZONTAL",                // ┅
        "LIGHT TRIPLE DASH VERTICAL",                  // ┆
        "HEAVY TRIPLE DASH VERTICAL",                  // ┇
        "LIGHT QUADRUPLE DASH HORIZONTAL",             // ┈
        "HEAVY QUADRUPLE DASH HORIZONTAL",             // ┉
        "LIGHT QUADRUPLE DASH VERTICAL",               // ┊
        "HEAVY QUADRUPLE DASH VERTICAL",               // ┋
        "LIGHT DOWN AND RIGHT",                        // ┌
        "DOWN LIGHT AND RIGHT HEAVY",                  // ┍
        "DOWN HEAVY AND RIGHT LIGHT",                  // ┎
        "HEAVY DOWN AND RIGHT",                        // ┏
        "LIGHT DOWN AND LEFT",                         // ┐
        "DOWN LIGHT AND LEFT HEAVY",                   // ┑
        "DOWN HEAVY AND LEFT LIGHT",                   // ┒
        "HEAVY DOWN AND LEFT",                         // ┓
        "LIGHT UP AND RIGHT",                          // └
        "UP LIGHT AND RIGHT HEAVY",                    // ┕
        "UP HEAVY AND RIGHT LIGHT",                    // ┖
        "HEAVY UP AND RIGHT",                          // ┗
        "LIGHT UP AND LEFT",                           // ┘
        "UP LIGHT AND LEFT HEAVY",                     // ┙
        "UP HEAVY AND LEFT LIGHT",                     // ┚
        "HEAVY UP AND LEFT",                           // ┛
        "LIGHT VERTICAL AND RIGHT",                    // ├
        "VERTICAL LIGHT AND RIGHT HEAVY",              // ┝
        "UP HEAVY AND RIGHT DOWN LIGHT",               // ┞
        "DOWN HEAVY AND RIGHT UP LIGHT",               // ┟
        "VERTICAL HEAVY AND RIGHT LIGHT",              // ┠
        "DOWN LIGHT AND RIGHT UP HEAVY",               // ┡
        "UP LIGHT AND RIGHT DOWN HEAVY",               // ┢
        "HEAVY VERTICAL AND RIGHT",                    // ┣
        "LIGHT VERTICAL AND LEFT",                     // ┤
        "VERTICAL LIGHT AND LEFT HEAVY",               // ┥
        "UP HEAVY AND LEFT DOWN LIGHT",                // ┦
        "DOWN HEAVY AND LEFT UP LIGHT",                // ┧
        "VERTICAL HEAVY AND LEFT LIGHT",               // ┨
        "DOWN LIGHT AND LEFT UP HEAVY",                // ┩
        "UP LIGHT AND LEFT DOWN HEAVY",                // ┪
        "HEAVY VERTICAL AND LEFT",                     // ┫
        "LIGHT DOWN AND HORIZONTAL",                   // ┬
        "LEFT HEAVY AND RIGHT DOWN LIGHT",             // ┭
        "RIGHT HEAVY AND LEFT DOWN LIGHT",             // ┮
        "DOWN LIGHT AND HORIZONTAL HEAVY",             // ┯
        "DOWN HEAVY AND HORIZONTAL LIGHT",             // ┰
        "RIGHT LIGHT AND LEFT DOWN HEAVY",             // ┱
        "LEFT LIGHT AND RIGHT DOWN HEAVY",             // ┲
        "HEAVY DOWN AND HORIZONTAL",                   // ┳
        "LIGHT UP AND HORIZONTAL",                     // ┴
        "LEFT HEAVY AND RIGHT UP LIGHT",               // ┵
        "RIGHT HEAVY AND LEFT UP LIGHT",               // ┶
        "UP LIGHT AND HORIZONTAL HEAVY",               // ┷
        "UP HEAVY AND HORIZONTAL LIGHT",               // ┸
        "RIGHT LIGHT AND LEFT UP HEAVY",               // ┹
        "LEFT LIGHT AND RIGHT UP HEAVY",               // ┺
        "HEAVY UP AND HORIZONTAL",                     // ┻
        "LIGHT VERTICAL AND HORIZONTAL",               // ┼
        "LEFT HEAVY AND RIGHT VERTICAL LIGHT",         // ┽
        "RIGHT HEAVY AND LEFT VERTICAL LIGHT",         // ┾
        "VERTICAL LIGHT AND HORIZONTAL HEAVY",         // ┿
        "UP HEAVY AND DOWN HORIZONTAL LIGHT",          // ╀
        "DOWN HEAVY AND UP HORIZONTAL LIGHT",          // ╁
        "VERTICAL HEAVY AND HORIZONTAL LIGHT",         // ╂
        "LEFT UP HEAVY AND RIGHT DOWN LIGHT",          // ╃
        "RIGHT UP HEAVY AND LEFT DOWN LIGHT",          // ╄
        "LEFT DOWN HEAVY AND RIGHT UP LIGHT",          // ╅
        "RIGHT DOWN HEAVY AND LEFT UP LIGHT",          // ╆
        "DOWN LIGHT AND UP HORIZONTAL HEAVY",          // ╇
        "UP LIGHT AND DOWN HORIZONTAL HEAVY",          // ╈
        "RIGHT LIGHT AND LEFT VERTICAL HEAVY",         // ╉
        "LEFT LIGHT AND RIGHT VERTICAL HEAVY",         // ╊
        "HEAVY VERTICAL AND HORIZONTAL",               // ╋
        "LIGHT DOUBLE DASH HORIZONTAL",                // ╌
        "HEAVY DOUBLE DASH HORIZONTAL",                // ╍
        "LIGHT DOUBLE DASH VERTICAL",                  // ╎
        "HEAVY DOUBLE DASH VERTICAL",                  // ╏
        "DOUBLE HORIZONTAL",                           // ═
        "DOUBLE VERTICAL",                             // ║
        "DOWN SINGLE AND RIGHT DOUBLE",                // ╒
        "DOWN DOUBLE AND RIGHT SINGLE",                // ╓
        "DOUBLE DOWN AND RIGHT",                       // ╔
        "DOWN SINGLE AND LEFT DOUBLE",                 // ╕
        "DOWN DOUBLE AND LEFT SINGLE",                 // ╖
        "DOUBLE DOWN AND LEFT",                        // ╗
        "UP SINGLE AND RIGHT DOUBLE",                  // ╘
        "UP DOUBLE AND RIGHT SINGLE",                  // ╙
        "DOUBLE UP AND RIGHT",                         // ╚
        "UP SINGLE AND LEFT DOUBLE",                   // ╛
        "UP DOUBLE AND LEFT SINGLE",                   // ╜
        "DOUBLE UP AND LEFT",                          // ╝
        "VERTICAL SINGLE AND RIGHT DOUBLE",            // ╞
        "VERTICAL DOUBLE AND RIGHT SINGLE",            // ╟
        "DOUBLE VERTICAL AND RIGHT",                   // ╠
        "VERTICAL SINGLE AND LEFT DOUBLE",             // ╡
        "VERTICAL DOUBLE AND LEFT SINGLE",             // ╢
        "DOUBLE VERTICAL AND LEFT",                    // ╣
        "DOWN SINGLE AND HORIZONTAL DOUBLE",           // ╤
        "DOWN DOUBLE AND HORIZONTAL SINGLE",           // ╥
        "DOUBLE DOWN AND HORIZONTAL",                  // ╦
        "UP SINGLE AND HORIZONTAL DOUBLE",             // ╧
        "UP DOUBLE AND HORIZONTAL SINGLE",             // ╨
        "DOUBLE UP AND HORIZONTAL",                    // ╩
        "VERTICAL SINGLE AND HORIZONTAL DOUBLE",       // ╪
        "VERTICAL DOUBLE AND HORIZONTAL SINGLE",       // ╫
        "DOUBLE VERTICAL AND HORIZONTAL",              // ╬
        "LIGHT ARC DOWN AND RIGHT",                    // ╭
        "LIGHT ARC DOWN AND LEFT",                     // ╮
        "LIGHT ARC UP AND LEFT",                       // ╯
        "LIGHT ARC UP AND RIGHT",                      // ╰
        "LIGHT DIAGONAL UPPER RIGHT TO LOWER LEFT",    // ╱
        "LIGHT DIAGONAL UPPER LEFT TO LOWER RIGHT",    // ╲
        "LIGHT DIAGONAL CROSS",                        // ╳
        "LIGHT LEFT",                                  // ╴
        "LIGHT UP",                                    // ╵
        "LIGHT RIGHT",                                 // ╶
        "LIGHT DOWN",                                  // ╷
        "HEAVY LEFT",                                  // ╸
        "HEAVY UP",                                    // ╹
        "HEAVY RIGHT",                                 // ╺
        "HEAVY DOWN",                                  // ╻
        "LIGHT LEFT AND HEAVY RIGHT",                  // ╼
        "LIGHT UP AND HEAVY DOWN",                     // ╽
        "HEAVY LEFT AND LIGHT RIGHT",                  // ╾
        "HEAVY UP AND LIGHT DOWN",                     // ╿
    ];

    // Kol indeksleri — **sınamanın kendi sırası**, `raster`'ınkinden ayrı:
    // ikisi aynı sabiti paylaşsaydı oracle uygulamanın bir parçasını okumuş
    // olurdu.
    const NAMED_UP: usize = 0;
    const NAMED_DOWN: usize = 1;
    const NAMED_LEFT: usize = 2;
    const NAMED_RIGHT: usize = 3;

    /// Adın söylediği kol stili. `SINGLE` ile `LIGHT` aynı şey: çift çizgi
    /// ailesinde Unicode ince kolu "single" diye adlandırıyor.
    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Named {
        Light,
        Heavy,
        Double,
    }

    /// Bir çizgi karakterinin **adından** okunan tarifi.
    struct NamedLine {
        arms: [Option<Named>; 4],
        dashes: u8,
        arc: bool,
    }

    /// Unicode adını kol kümesine çevirir.
    ///
    /// Ad ` AND ` ile öbeklere ayrılıyor; her öbek bir yön kümesi ve —
    /// varsa — bir stil taşıyor. Stilsiz öbek adın **ilk** stilini miras
    /// alıyor (`LIGHT DOWN AND RIGHT` → ikisi de ince, `HEAVY VERTICAL AND
    /// RIGHT` → üçü de kalın). İki tuzak var ve ikisi de adlandırmanın
    /// kendisinden: `DOUBLE` bir stil ama `DOUBLE DASH`'te yoğunluk sayısı,
    /// ve `SINGLE` stil sözlüğünde yok — `LIGHT`'ın çift çizgi ailesindeki
    /// adı.
    ///
    /// Tanınmayan sözcük ya da yönsüz öbek **panik**: sessizce atlamak
    /// oracle'ı kendi kendine boşaltırdı.
    fn parse_line_name(name: &str) -> NamedLine {
        let words: Vec<&str> = name.split_whitespace().collect();
        let dashes = words
            .iter()
            .position(|&word| word == "DASH")
            .map_or(0u8, |at| match words[at - 1] {
                "DOUBLE" => 2,
                "TRIPLE" => 3,
                "QUADRUPLE" => 4,
                other => panic!("bilinmeyen yoğunluk: {other} ({name})"),
            });
        // Adın ilk stil sözcüğü: stilsiz öbeklerin mirası.
        let mut inherited = None;
        for (at, &word) in words.iter().enumerate() {
            let style = match word {
                "LIGHT" | "SINGLE" => Some(Named::Light),
                "HEAVY" => Some(Named::Heavy),
                "DOUBLE" if words.get(at + 1) != Some(&"DASH") => Some(Named::Double),
                _ => None,
            };
            if style.is_some() {
                inherited = style;
                break;
            }
        }

        let mut arms = [None; 4];
        for group in name.split(" AND ") {
            let mut style = None;
            let mut directions: Vec<usize> = Vec::new();
            let words: Vec<&str> = group.split_whitespace().collect();
            for (at, &word) in words.iter().enumerate() {
                match word {
                    "LIGHT" | "SINGLE" => style = Some(Named::Light),
                    "HEAVY" => style = Some(Named::Heavy),
                    "DOUBLE" if words.get(at + 1) != Some(&"DASH") => style = Some(Named::Double),
                    "DOUBLE" | "TRIPLE" | "QUADRUPLE" | "DASH" | "ARC" => {}
                    "UP" => directions.push(NAMED_UP),
                    "DOWN" => directions.push(NAMED_DOWN),
                    "LEFT" => directions.push(NAMED_LEFT),
                    "RIGHT" => directions.push(NAMED_RIGHT),
                    "VERTICAL" => directions.extend([NAMED_UP, NAMED_DOWN]),
                    "HORIZONTAL" => directions.extend([NAMED_LEFT, NAMED_RIGHT]),
                    other => panic!("adda tanınmayan sözcük: {other} ({name})"),
                }
            }
            assert!(!directions.is_empty(), "yönsüz öbek: {group} ({name})");
            let style = style
                .or(inherited)
                .unwrap_or_else(|| panic!("stilsiz ad: {name}"));
            for direction in directions {
                arms[direction] = Some(style);
            }
        }
        assert!(arms.iter().any(Option::is_some), "kolsuz ad: {name}");
        NamedLine {
            arms,
            dashes,
            arc: name.contains("ARC"),
        }
    }

    /// Kapsamdaki çizgi karakterleri: U+2500–U+257F, **köşegenler hariç**.
    fn line_chars() -> impl Iterator<Item = (char, NamedLine)> {
        (0x2500..=0x257Fu32)
            .filter(|cp| !(0x2571..=0x2573).contains(cp))
            .map(|cp| {
                let ch = char::from_u32(cp).expect("çizgi kod noktası");
                (ch, parse_line_name(LINE_NAMES[(cp - 0x2500) as usize]))
            })
    }

    /// Sprite'ın bir kenarındaki piksel profili — üst/alt kenarda satır,
    /// sol/sağ kenarda sütun.
    fn edge(bytes: &[u8], m: Metrics, side: usize) -> Vec<u8> {
        let (w, h) = m.cell_wh();
        match side {
            NAMED_UP => bytes[..w].to_vec(),
            NAMED_DOWN => bytes[(h - 1) * w..].to_vec(),
            NAMED_LEFT => (0..h).map(|y| bytes[y * w]).collect(),
            _ => (0..h).map(|y| bytes[y * w + w - 1]).collect(),
        }
    }

    /// Kolu tek başına taşıyan karakterin aynı kenardaki profili — dikişin
    /// ölçütü.
    fn reference_edge(m: Metrics, style: Named, vertical: bool) -> Vec<u8> {
        let ch = match (vertical, style) {
            (true, Named::Light) => '│',
            (true, Named::Heavy) => '┃',
            (true, Named::Double) => '║',
            (false, Named::Light) => '─',
            (false, Named::Heavy) => '━',
            (false, Named::Double) => '═',
        };
        let side = if vertical { NAMED_UP } else { NAMED_LEFT };
        edge(&procedural(m, ch), m, side)
    }

    /// Profildeki kesintisiz mürekkep kuşaklarının sayısı.
    fn runs(profile: &[u8]) -> usize {
        profile
            .iter()
            .zip(std::iter::once(&0).chain(profile))
            .filter(|(current, previous)| **current > 0 && **previous == 0)
            .count()
    }

    #[test]
    fn the_name_parser_reads_the_grammar() {
        // Oracle'ın kendi bekçisi: her adı aynı kola çeviren bozuk bir
        // ayrıştırıcı bütün sınamaları yeşil bırakırdı. Dört ad dilbilgisinin
        // dört tuzağını taşıyor — miras alınan stil, `DOUBLE DASH`'in stil
        // olmaması, `SINGLE`'ın ince demesi ve `ARC`.
        let probe = parse_line_name("UP HEAVY AND RIGHT DOWN LIGHT"); // ┞
        assert_eq!(
            probe.arms,
            [
                Some(Named::Heavy),
                Some(Named::Light),
                None,
                Some(Named::Light)
            ]
        );
        let probe = parse_line_name("HEAVY DOUBLE DASH HORIZONTAL"); // ╍
        assert_eq!(
            probe.arms,
            [None, None, Some(Named::Heavy), Some(Named::Heavy)]
        );
        assert_eq!((probe.dashes, probe.arc), (2, false));
        let probe = parse_line_name("VERTICAL SINGLE AND HORIZONTAL DOUBLE"); // ╪
        assert_eq!(
            probe.arms,
            [
                Some(Named::Light),
                Some(Named::Light),
                Some(Named::Double),
                Some(Named::Double)
            ]
        );
        let probe = parse_line_name("LIGHT ARC DOWN AND RIGHT"); // ╭
        assert_eq!(
            probe.arms,
            [None, Some(Named::Light), None, Some(Named::Light)]
        );
        assert!(probe.arc && probe.dashes == 0);
        // Ve ayrıştırıcı 125 adın **hepsini** okuyabiliyor: tanınmayan
        // sözcük ya da yönsüz öbek panik, yani bu tur sessiz kalmaz.
        assert_eq!(line_chars().count(), 125);
    }

    #[test]
    fn the_arms_come_from_the_unicode_names() {
        // Oracle **bağımsız**: beklenti karakterin Unicode adından
        // ayrıştırılıyor (bkz. [`LINE_NAMES`]), uygulamanın tablosundan
        // değil. Gördüğü şey aynalanmış ya da bir kaydırmış tablo: `├` ile
        // `┤` yer değiştirseydi ikisi de kusursuz çizilir, yalnız yanlış
        // kod noktasında dururdu ve geometriye bakan hiçbir değişmez bunu
        // göremezdi.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            for (ch, named) in line_chars() {
                let bytes = procedural(m, ch);
                for side in [NAMED_UP, NAMED_DOWN, NAMED_LEFT, NAMED_RIGHT] {
                    let profile = edge(&bytes, m, side);
                    let inked = profile.iter().any(|&b| b > 0);
                    // Kesikli çizginin **kapanış** kenarı boş: desen dolu
                    // başlıyor ve boşlukla bitiyor (`band`'in bugünkü
                    // davranışı da bu). Kol orada yok değil, tire orada yok.
                    let trailing = named.dashes > 0 && (side == NAMED_DOWN || side == NAMED_RIGHT);
                    if named.arms[side].is_some() && !trailing {
                        assert!(
                            inked,
                            "{point_size}pt@{scale}x: '{ch}' ({}) {side}. kolu \
                             kenara ulaşmadı",
                            LINE_NAMES[(u32::from(ch) - 0x2500) as usize]
                        );
                    }
                    if named.arms[side].is_none() {
                        assert!(
                            !inked,
                            "{point_size}pt@{scale}x: '{ch}' ({}) olmayan {side}. \
                             kolun kenarına mürekkep bıraktı",
                            LINE_NAMES[(u32::from(ch) - 0x2500) as usize]
                        );
                    }
                }
                assert_eq!(
                    named.arc,
                    matches!(ch, '╭' | '╮' | '╯' | '╰'),
                    "yay bayrağı adla uyuşmuyor: '{ch}'"
                );
            }
        }
    }

    #[test]
    fn arms_tile_across_the_cell_edge() {
        // **Dikiş sürekliliği**: kenardaki profil yalnız kolun *stiline*
        // bağlı olmak zorunda, karakterin geri kalanına değil. Yan yana iki
        // `─`, `├`'nin sağına konan `─`, `┼`'ın altına konan `│` — hepsi
        // aynı iddia, ve iddia bu tek eşitlikte: her karakterin kenar
        // profili o stilin tek kollu referansının profiline **eşit**.
        // Ölçüt eşitlik, "mürekkep var mı" değil: bir baytlık fark komşu
        // hücreler arasında sönük bir dikiş demek.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            // Referanslar **punto başına bir kez**: altısı da her kenarda
            // yeniden rasterize edilseydi üç boyda 1500 tam hücre çizimi
            // eder ve `make hepsi` her phase kapısında onu öderdi.
            // Dış indeks eksen (`vertical`), iç indeks `Named`'ın kendi
            // sırası — `style as usize` onu okuyor, yani iki liste birlikte
            // değişmek zorunda.
            let references: [[Vec<u8>; 3]; 2] = [
                [
                    reference_edge(m, Named::Light, false),
                    reference_edge(m, Named::Heavy, false),
                    reference_edge(m, Named::Double, false),
                ],
                [
                    reference_edge(m, Named::Light, true),
                    reference_edge(m, Named::Heavy, true),
                    reference_edge(m, Named::Double, true),
                ],
            ];
            for (ch, named) in line_chars() {
                let bytes = procedural(m, ch);
                for side in [NAMED_UP, NAMED_DOWN, NAMED_LEFT, NAMED_RIGHT] {
                    let Some(style) = named.arms[side] else {
                        continue;
                    };
                    // Tek muafiyet ve adıyla: kesikli çizginin **kapanış**
                    // kenarı, çünkü desen dolu başlayıp boşlukla bitiyor.
                    // **Yay muaf değil** — bir dönem öyleydi ve muafiyet
                    // gerçek bir kusuru örtüyordu: yarıçap hücre kenarına
                    // kadar gidince teğet noktası oraya düşüyor ve kenar
                    // sütununu sap yerine yay boyuyordu (rayın satırında 255
                    // yerine 246, altındakinde 0 yerine 13), üstelik dört
                    // köşenin yalnız ikisinde. `corner`'ın bir piksel
                    // içerlek yarıçapı onu kapattı; muafiyet kalkınca bu
                    // satır o düzeltmenin bekçisi oldu.
                    if named.dashes > 0 && (side == NAMED_DOWN || side == NAMED_RIGHT) {
                        continue;
                    }
                    let vertical = side == NAMED_UP || side == NAMED_DOWN;
                    assert_eq!(
                        edge(&bytes, m, side),
                        references[usize::from(vertical)][style as usize],
                        "{point_size}pt@{scale}x: '{ch}' {side}. kenarında \
                         dikiş kırıldı"
                    );
                }
            }
        }
    }

    #[test]
    fn disjoint_arms_unite_into_the_joint() {
        // Birleşim yasası: ayrık kol kümeli iki karakterin piksel-max'i
        // birleşim kümesinin karakteri. Yapısal olarak doğru olmak zorunda
        // — aynı kol her karakterde aynı dikdörtgeni veriyor — ve tam da bu
        // yüzden kırılması bir kaza değil, kolun uzantısının karaktere göre
        // değiştiğinin kanıtı olurdu.
        //
        // **Çift çizgi bu listede yok** ve sebebi geometri: `╔`'in üst rayı
        // köşeyi kapatmak için kavşağı geçiyor, `╬`'te ise aynı ray dirsek
        // yapıp duruyor (kanal açık kalmalı). Yani `╔ ∪ ╝ ≠ ╬` ve olması da
        // gerekmiyor; çift çizginin bekçisi
        // [`double_junctions_keep_the_channel_open`].
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            for (a, b, joint) in [
                ('┌', '┘', '┼'),
                ('┐', '└', '┼'),
                ('┏', '┛', '╋'),
                ('┓', '┗', '╋'),
                ('╴', '╶', '─'),
                ('╵', '╷', '│'),
                ('╸', '╺', '━'),
                ('╹', '╻', '┃'),
                ('├', '┤', '┼'),
            ] {
                assert_eq!(
                    pixel_max(&procedural(m, a), &procedural(m, b)),
                    procedural(m, joint),
                    "{point_size}pt@{scale}x: '{a}' ∪ '{b}' '{joint}' vermedi"
                );
            }
        }
    }

    #[test]
    fn double_junctions_keep_the_channel_open() {
        // Çift çizgi bir çizgi değil **iki duvarlı bir kanal**, ve
        // kavşaktaki bütün kararlar tek cümleden çıkıyor: kanal kapanmaz.
        // Bu bekçi rayın "dönmesi" ile "geçmesi" arasındaki farkın tek
        // tanığı — birleşim yasası da dikiş de o farkı göremez, ikisi de
        // kenarlara ve toplama bakıyor.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            // **İç** boşluk: mürekkebin arasında kalan boş satır. Ölçüt
            // "boş satır var mı" olamazdı — `╒`'nin üstünde kolu olmayan
            // sekiz boş satır var ve onlar kanal değil, karakterin dışı.
            let gap_row = |ch: char| {
                let bytes = procedural(m, ch);
                let inked = |y: usize| bytes[y * w..(y + 1) * w].iter().any(|&b| b > 0);
                (0..h).any(|y| !inked(y) && (0..y).any(inked) && (y + 1..h).any(inked))
            };
            let gap_column = |ch: char| {
                let bytes = procedural(m, ch);
                let inked = |x: usize| (0..h).any(|y| bytes[y * w + x] > 0);
                (0..w).any(|x| !inked(x) && (0..x).any(inked) && (x + 1..w).any(inked))
            };
            let full_row = |ch: char| {
                let bytes = procedural(m, ch);
                (0..h).any(|y| bytes[y * w..(y + 1) * w].iter().all(|&b| b == 255))
            };
            let full_column = |ch: char| {
                let bytes = procedural(m, ch);
                (0..w).any(|x| (0..h).all(|y| bytes[y * w + x] == 255))
            };
            let at = format!("{point_size}pt@{scale}x");

            // `╬` dört dirsek: ortasından hem boş bir satır hem boş bir
            // sütun geçiyor. `╋` aynı kollara sahip ve hiçbiri yok — ölçüt
            // "çizgi var mı" değil, kanalın açıklığı.
            assert!(gap_row('╬') && gap_column('╬'), "{at}: `╬` kanalı kapandı");
            assert!(
                !gap_row('╋') && !gap_column('╋'),
                "{at}: `╋` ortasında boşluk açtı"
            );
            // `╠`: dış duvar kesintisiz, iç duvar kırık. Kesintisiz duvar
            // yüzünden boş satır **yok**; olsaydı çerçevenin sol kenarı
            // T-kavşağında kopardı.
            assert!(full_column('╠'), "{at}: `╠`'in dış duvarı kesintisiz değil");
            assert!(!gap_row('╠'), "{at}: `╠` sol kenarı kopardı");
            assert!(full_row('╦'), "{at}: `╦`'in dış duvarı kesintisiz değil");
            assert!(!gap_column('╦'), "{at}: `╦` üst kenarı kopardı");
            // Tek ray çift raylı kavşağı **geçiyor** — karşı kolu varsa.
            // `╪`'nin dikey çizgisi baştan sona, `╫`'ün yatayı öyle.
            assert!(!gap_row('╪'), "{at}: `╪`'in dikey çizgisi ortadan koptu");
            assert!(!gap_column('╫'), "{at}: `╫`'ün yatay çizgisi ortadan koptu");
            // Karşı kolu yoksa **duruyor**: `╤`'nin sapı alt rayda başlıyor
            // ve iki ray arasındaki satır boş kalıyor.
            assert!(gap_row('╤'), "{at}: `╤`'nin sapı kanalı kapattı");
            // Ama köşede aynı sap **uzak** raya kadar gidiyor, yoksa `╒`
            // köşesiz kalırdı.
            assert!(!gap_row('╒'), "{at}: `╒`'nin sapı üst raya ulaşmadı");
        }
    }

    #[test]
    fn heavy_is_thicker_and_double_is_two_rails() {
        // Üç stil üç ayrı iddia taşıyor ve üçü de kenar profilinden
        // okunabiliyor: kalın inceden **kalın**, çift **iki ayrı** banttan.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            for vertical in [false, true] {
                let light = reference_edge(m, Named::Light, vertical);
                let heavy = reference_edge(m, Named::Heavy, vertical);
                let double = reference_edge(m, Named::Double, vertical);
                let ink = |profile: &[u8]| profile.iter().map(|&b| u32::from(b)).sum::<u32>();
                assert!(
                    ink(&heavy) > ink(&light),
                    "{point_size}pt@{scale}x (dikey={vertical}): kalın inceden kalın değil"
                );
                assert_eq!(runs(&light), 1, "ince çizgi tek bant olmalı");
                assert_eq!(runs(&heavy), 1, "kalın çizgi tek bant olmalı");
                assert_eq!(
                    runs(&double),
                    2,
                    "{point_size}pt@{scale}x (dikey={vertical}): çift çizgi iki \
                     ayrı bant olmalı"
                );
            }
        }
    }

    #[test]
    fn dashed_densities_collapse_only_with_the_period() {
        // `dividing_period` korunuyor (`discussion.md` → Karar 4): periyot
        // hücreyi tam bölmek zorunda, yoksa desen hücre sınırında faz kırar
        // ve döşeme bu setin varlık sebebi. Bedeli görünür bir bilgi kaybı
        // — bu makinede `w = 8`'de `┄` ile `╌` **aynı sprite'a** çöküyor —
        // ve bekçi onu listeye yazmıyor, **türetiyor**: iki yoğunluk ancak
        // periyotları eşitse eşit. Sayı listeye yazılsaydı başka bir
        // puntoda yanlış olurdu.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            for (axis, extent, family) in
                [("yatay", w, ['╌', '┄', '┈']), ("dikey", h, ['╎', '┆', '┊'])]
            {
                for (i, first) in family.into_iter().enumerate() {
                    for second in family.into_iter().skip(i + 1) {
                        let period = |ch: char| {
                            let dashes = match ch {
                                '╌' | '╎' => 2usize,
                                '┄' | '┆' => 3,
                                _ => 4,
                            };
                            raster::dividing_period(extent.div_ceil(dashes), extent)
                        };
                        assert_eq!(
                            procedural(m, first) == procedural(m, second),
                            period(first) == period(second),
                            "{point_size}pt@{scale}x {axis}: '{first}' ile '{second}' \
                             periyotları {} ve {}",
                            period(first),
                            period(second)
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_arcs_round_the_corner() {
        // Yay ayrı bir teknik değil ama ayrı bir **şekil**: `╭` ile `┌`
        // aynı kolları taşıyor, aynı kenarlara dokunuyor ve farklı
        // çiziliyor. Bekçi ikisinin arasındaki farkı istiyor, yoksa yay
        // bayrağı sessizce yok sayılabilirdi.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            for (arc, sharp) in [('╭', '┌'), ('╮', '┐'), ('╯', '┘'), ('╰', '└')] {
                assert_ne!(
                    procedural(m, arc),
                    procedural(m, sharp),
                    "{point_size}pt@{scale}x: '{arc}' keskin köşeyle aynı çizildi"
                );
            }
        }
    }

    #[test]
    fn the_technical_set_hugs_the_cell_edges() {
        // Bu kümenin U+2500 ailesinden ayrıldığı **tek** yer eksenin yeri:
        // orada kollar hücrenin ortasında buluşur, burada çizgiler kenarı
        // izler. Ölçüt piksel piksel eşitlik, "mürekkep var mı" değil —
        // ortada buluşan bir `⎿` de mürekkepli olurdu ama yarı boyda bir
        // köşe çizerdi.
        type Mask = fn(usize, usize, usize, usize, usize) -> bool;
        let cases: [(char, Mask); 4] = [
            ('\u{23B8}', |x, _y, _w, _h, thin| x < thin),
            ('\u{23B9}', |x, _y, w, _h, thin| x >= w - thin),
            ('\u{23BE}', |x, y, _w, _h, thin| x < thin || y < thin),
            ('\u{23BF}', |x, y, _w, h, thin| x < thin || y >= h - thin),
        ];
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            let thin = usize::from(m.underline_px.1.max(1));
            for (ch, mask) in cases {
                let bytes = procedural(m, ch);
                for y in 0..h {
                    for x in 0..w {
                        let want = if mask(x, y, w, h, thin) { 255 } else { 0 };
                        assert_eq!(
                            bytes[y * w + x],
                            want,
                            "{point_size}pt@{scale}x: '{ch}' ({x}, {y}) pikselinde \
                             kenar profili bozuk"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_technical_pairs_are_mirrors() {
        // Aynalama **yapısal**: ilk bant `[0, thin)`, son bant
        // `[uzunluk - thin, uzunluk)` ve ikisi birbirinin tam yansıması,
        // yani bu eşitlik yuvarlamadan bağımsız her ölçüde tutmak zorunda.
        // Kırılması "kenar çizgisi kenarda değil" demenin ikinci yolu.
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            let flip_h: Vec<u8> = (0..h)
                .flat_map(|y| (0..w).map(move |x| (y, w - 1 - x)))
                .map(|(y, x)| procedural(m, '\u{23B8}')[y * w + x])
                .collect();
            assert_eq!(
                flip_h,
                procedural(m, '\u{23B9}'),
                "{point_size}pt@{scale}x: '⎸' ile '⎹' birbirinin aynası değil"
            );
            let top = procedural(m, '\u{23BE}');
            let flip_v: Vec<u8> = (0..h)
                .flat_map(|y| {
                    let row = (h - 1 - y) * w;
                    top[row..row + w].to_vec()
                })
                .collect();
            assert_eq!(
                flip_v,
                procedural(m, '\u{23BF}'),
                "{point_size}pt@{scale}x: '⎾' ile '⎿' birbirinin aynası değil"
            );
        }
    }

    #[test]
    fn the_scan_lines_step_down_the_cell() {
        // İki iddia, ikisi de adın kendisinden: "HORIZONTAL SCAN LINE-N"
        //
        // 1. Satır **hücreyi boydan boya** geçiyor. Fonttan gelen hâli
        //    geçmiyordu: Monaco'nun mürekkebi 20 px hücrede 0.03–19.19,
        //    yani yan yana dizilen tarama satırları kesikli görünüyordu.
        // 2. Dokuz bandın 1, 3, 5, 7, 9'uncusu — ve **beşincisi `─`**,
        //    çünkü Unicode onu U+2500 ile birleştirdi. Beş bandın eşit
        //    aralıklı çıkması formülün ikinci bir sabit uydurmadığının
        //    tanığı; ±1 piksel payı `rail`'in ızgaraya oturtmasından
        //    (13pt@1x'te beş bant tam bölünüyor, 16pt@2x'te 9/9/8/9).
        for (point_size, scale) in PROCEDURAL_SIZES {
            let m = atlas(point_size, scale).metrics();
            let (w, h) = m.cell_wh();
            let mut starts = Vec::new();
            for ch in ['\u{23BA}', '\u{23BB}', '\u{2500}', '\u{23BC}', '\u{23BD}'] {
                let bytes = procedural(m, ch);
                let rows: Vec<usize> = (0..h)
                    .filter(|&y| bytes[y * w..y * w + w].iter().any(|&b| b > 0))
                    .collect();
                let (&first, &last) = (
                    rows.first().expect("tarama satırı boş çizildi"),
                    rows.last().expect("tarama satırı boş çizildi"),
                );
                assert_eq!(
                    rows.len(),
                    last - first + 1,
                    "{point_size}pt@{scale}x: '{ch}' tek bant değil"
                );
                for y in first..=last {
                    assert!(
                        bytes[y * w..y * w + w].iter().all(|&b| b == 255),
                        "{point_size}pt@{scale}x: '{ch}' {y}. satırda hücreyi \
                         boydan boya geçmiyor"
                    );
                }
                starts.push(first);
            }
            assert!(
                starts.windows(2).all(|pair| pair[0] < pair[1]),
                "{point_size}pt@{scale}x: tarama satırları yukarıdan aşağıya \
                 sıralı değil: {starts:?}"
            );
            let steps: Vec<usize> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
            let (low, high) = (
                *steps.iter().min().expect("dört adım"),
                *steps.iter().max().expect("dört adım"),
            );
            assert!(
                high - low <= 1,
                "{point_size}pt@{scale}x: bantlar eşit aralıklı değil: {steps:?}"
            );
        }
    }

    #[test]
    fn the_diagonals_stay_out_of_scope() {
        // Köşegenler kapsamın içinde **bilerek bırakılmış bir delik**
        // (Karar 3B) ve deliğin ikinci bir işi var:
        // `face_fallback_is_cached_under_the_requested_face`'in fikstürü
        // (`╱`) orada yaşıyor — bu makinede Menlo Regular'da olup Bold'da
        // olmayan tek blok U+2500–U+257F ve gerisi artık yordamsal.
        for ch in ['╱', '╲', '╳'] {
            assert!(
                !raster::is_procedural(ch),
                "'{ch}' kapsama girdi: yüz merdiveninin fikstürü kalmıyor"
            );
        }
        // Üç ailenin de iki ucu ve dışarıdaki komşuları. Taşan bir aralık
        // **sessiz**: `braille` maskeyi `& 0xFF` ile alıyor, `block`'un
        // çeyrek kolu tanımadığı karaktere sıfır maske veriyor, yani
        // aralığı bir karakter geniş yazmak boş sprite üretir ve geometriye
        // bakan hiçbir değişmez bunu göremez.
        for (ch, inside, family) in [
            ('\u{24FF}', false, "çizginin altı"),
            ('\u{2500}', true, "çizginin başı"), // ─
            ('\u{257F}', true, "çizginin sonu"), // ╿
            ('\u{2580}', true, "bloğun başı"),   // ▀
            ('\u{259F}', true, "bloğun sonu"),   // ▟
            ('\u{25A0}', false, "bloğun üstü"),  // ■, geometrik şekiller
            ('\u{27FF}', false, "Braille'in altı"),
            ('\u{2800}', true, "Braille'in başı"),
            ('\u{28FF}', true, "Braille'in sonu"),
            ('\u{2900}', false, "Braille'in üstü"),
            // Dördüncü aile ve **iki** komşusu anlamlı: U+23B7 (`⎷`) de
            // bilerek dışarıda (kök kuyruğu bir ray değil, cascade'den
            // gelen hâli kapıyı geçiyor), yani alt sınır bir sınır değil
            // bir **karar**.
            ('\u{23B7}', false, "teknik kümenin altı"), // ⎷
            ('\u{23B8}', true, "teknik kümenin başı"),  // ⎸
            ('\u{23BF}', true, "teknik kümenin sonu"),  // ⎿
            ('\u{23C0}', false, "teknik kümenin üstü"), // ⏀
        ] {
            assert_eq!(
                raster::is_procedural(ch),
                inside,
                "{family} (U+{:04X}) yanlış tarafta",
                u32::from(ch)
            );
        }
    }

    /// Geniş karakter **iki yuvadan** çiziliyor ve ikisi **aynı dönüşte**
    /// geliyor.
    ///
    /// Atomiklik bir kolaylık değil: iki yarı ayrı turlara bölünseydi kapasite
    /// sınırı ikisinin arasına düşebilir, sol yuva açılır, sağ tofu'ya düşer
    /// ve ekranda yarım glyph + yarım kutu belirirdi. `CLAUDE.md`'nin kuralı
    /// bunu adıyla yasaklıyor: "kutu görünür bir eksiklik, kırpılmış glyph
    /// sessiz bir bozulma".
    #[test]
    fn a_wide_char_takes_two_slots_in_one_answer() {
        let mut a = atlas(POINT_SIZE, 1.0);
        let before = a.occupancy().0;
        let (placed, upload) = a.slot(
            Sprite::Char(UNKNOWN_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_eq!(
            placed.half,
            Half::Left,
            "'{UNKNOWN_CHAR}' mürekkebi iki hücreye sığıyor: çift beklenir"
        );
        assert_ne!(placed.slot, TOFU, "kabul edilen çift tofu'ya düşmemeli");
        let upload = upload.expect("yeni çift yükleme vermeli");
        let right = upload.right.expect("sağ yarı aynı dönüşte gelmeli");
        assert_ne!(
            upload.origin, right,
            "iki yarı aynı yuvaya yazılıyor: köşeler ayrı olmalı"
        );
        assert_eq!(
            upload.bytes.len(),
            upload.right_bytes.len(),
            "iki yarı da tam bir yuva"
        );
        assert_eq!(
            a.occupancy().0 - before,
            2,
            "geniş karakter tam iki yuva harcamalı"
        );
        // Sağ yarı **ikinci bir kapı turu istemiyor**: çifti kabul eden çağrı
        // iki anahtarı birden yazdı, yani bu soru önbellekten dönüyor ve
        // cascade yürüyüşü kare bütçesinin ortasında bir daha koşmuyor.
        let (right_placed, right_upload) = a.slot(
            Sprite::Char(UNKNOWN_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Right,
        );
        assert_eq!(right_placed.slot, placed.slot + 1, "sağ yarı solun komşusu");
        assert!(
            right_upload.is_none(),
            "sağ yarı zaten yüklendi: doku el değmeden kalmalı"
        );
        assert_eq!(a.occupancy().0 - before, 2, "sağ yarı üçüncü yuva açmamalı");
    }

    /// **Kapı sırası: tek hücre önce.** Geniş ilan edilmiş ama mürekkebi bir
    /// hücreye sığan karakter tek yuvadan çiziliyor ve rasteri `Half::Whole`
    /// isteğiyle **bit bit** aynı.
    ///
    /// Ölçülen 65 karakterin ("geniş ilan edilmiş, tek hücreye sığıyor")
    /// sözleşmesi bu: 021'den beri çalışan çizimleri bu set oynatmıyor. Sıra
    /// ters olsaydı `centre_shift` onları iki hücrelik kutuya göre ortalar ve
    /// hepsi yerinden kayardı.
    #[test]
    fn a_wide_char_that_fits_one_cell_keeps_the_single_slot_raster() {
        // Taban fontun kendi glyph'i: eşaralıklı yüzde ilerleme hücrenin
        // ilerlemesinin ta kendisi, yani tanım gereği tek hücre. `☕` bu
        // makinede Menlo'da var ve Unicode onu iki sütun ilan ediyor —
        // ölçülen 21'in içinde.
        const NARROW_WIDE: char = '☕';
        let mut a = atlas(POINT_SIZE, 1.0);
        let whole = {
            let (placed, upload) = a.slot(
                Sprite::Char(NARROW_WIDE),
                Face::Regular,
                SizeClass::Normal,
                Half::Whole,
            );
            (placed, upload.map(|u| u.bytes.to_vec()))
        };
        let Some(whole_bytes) = whole.1 else {
            // Karakteri taşıyan bir font kurulu değilse sınama konusuz.
            return;
        };
        assert_ne!(whole.0.slot, TOFU, "'{NARROW_WIDE}' çizilebilir olmalı");

        let mut b = atlas(POINT_SIZE, 1.0);
        let (placed, upload) = b.slot(
            Sprite::Char(NARROW_WIDE),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_eq!(
            placed.half,
            Half::Whole,
            "tek hücreye sığan geniş karakter çifte dönüşmemeli"
        );
        let upload = upload.expect("yeni yuva yükleme vermeli");
        assert!(
            upload.right.is_none(),
            "tek hücrelik cevap sağ yarı vermemeli: çağıran boş dörtlü basardı"
        );
        assert_eq!(
            upload.bytes,
            &whole_bytes[..],
            "raster `Half::Whole` isteğiyle bit bit aynı kalmalı"
        );
        assert_eq!(b.occupancy().0, 2, "tek yuva + tofu");
    }

    /// Kapasite sınırı çiftin **arasına düşmüyor**: tam bir boş yuva varken
    /// istenen geniş karakterin iki yarısı da tofu dönüyor ve `next`
    /// kıpırdamıyor.
    ///
    /// Aranan şey "yarım glyph + yarım kutu"nun **yokluğu** ve o hâl
    /// atomiklik yüzünden yapısal olarak doğmuyor — yani bu bekçi kapıyı
    /// değil kapının **sınırını** sınıyor. Doluluğu arayan bir sınama boşa
    /// yeşil kalırdı.
    #[test]
    fn a_wide_char_is_rejected_whole_when_only_one_slot_is_left() {
        let mut a = Atlas::new(None, LARGE_POINT_SIZE, 1.0, LARGEST_LINE_HEIGHT);
        let cap = a.capacity().saturating_sub(RULE_RESERVE);
        // Havuz `full_atlas_returns_tofu_without_caching`'inkiyle **aynı
        // gerekçeyle** kuruluyor: tofu'ya düşen karakter yuva harcamıyor, yani
        // doldurma gerçekten çizilebilen karakterlerden olmak zorunda.
        // Yordamsal aile `Regular`'a normalize olduğu için bir kez, ASCII dört
        // yüzde de ayrı yuva tutuyor.
        let pool: Vec<(char, Face)> = procedural_chars()
            .map(|ch| (ch, Face::Regular))
            .chain(
                [Face::Regular, Face::Bold, Face::Italic, Face::BoldItalic]
                    .into_iter()
                    .flat_map(|f| (' '..='~').map(move |ch| (ch, f))),
            )
            .collect();
        // Tam **bir** yuva boş kalana kadar doldur.
        for &(ch, face) in &pool {
            if a.next + 1 >= cap {
                break;
            }
            a.slot(Sprite::Char(ch), face, SizeClass::Normal, Half::Whole);
        }
        assert_eq!(
            a.next + 1,
            cap,
            "havuz kapasiteyi doldurmalı: tam bir yuva boş kalacak"
        );
        let next_before = a.next;
        let (placed, upload) = a.slot(
            Sprite::Char('𠀀'),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_eq!(
            placed.slot, TOFU,
            "bir yuva iki yarıya yetmez: çift tümden reddedilmeli"
        );
        assert!(upload.is_none(), "reddedilen çift yükleme vermemeli");
        assert_eq!(a.next, next_before, "reddedilen çift yuva harcamamalı");
        // Sağ yarı da aynı cevabı veriyor: ekranda yarım glyph doğmuyor.
        let (right, _) = a.slot(
            Sprite::Char('𠀀'),
            Face::Regular,
            SizeClass::Normal,
            Half::Right,
        );
        assert_eq!(right.slot, TOFU, "sağ yarı da tofu olmalı");
        assert_eq!(a.next, next_before, "sağ yarı da yuva harcamamalı");
    }

    /// **Tek hücrelik ret, iki hücrelik isteğin cevabı değil.**
    ///
    /// Üretimdeki sıra tam bu: dock giriş satırını **her zaman**
    /// `wide: false` ile soruyor (`bt_core::dock`'un değişmezi) ve dock'un
    /// satırı `SizeClass::Normal`, yani prompt'a yazılan bir CJK karakteri
    /// önce `Half::Whole` olarak sorulup **negatif önbelleğe** giriyor.
    /// Enter'dan sonra aynı karakter ızgaraya `wide: true` ile geliyor. Takma
    /// ad o tofu kaydını geçirirse setin tamamı o karakter için atlasın ömrü
    /// boyunca ölü kalır — ve belirti sessiz: kutu çizilir, hiçbir sayaç
    /// kıpırdamaz.
    #[test]
    fn a_single_cell_rejection_does_not_answer_the_wide_request() {
        let mut a = atlas(POINT_SIZE, 1.0);
        // 1. Dock'un sorusu: tek hücre, ve `漢` oraya sığmıyor.
        let (whole, _) = a.slot(
            Sprite::Char(UNKNOWN_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Whole,
        );
        assert_eq!(
            whole.slot, TOFU,
            "'{UNKNOWN_CHAR}' tek hücreye sığmıyor: negatif önbelleğe girmeli"
        );
        // 2. Izgaranın sorusu: iki hücre. Aynı karakter artık çizilmeli.
        let (left, upload) = a.slot(
            Sprite::Char(UNKNOWN_CHAR),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_ne!(
            left.slot, TOFU,
            "tek hücrelik ret iki hücrelik isteği zehirledi"
        );
        assert_eq!(left.half, Half::Left, "çift beklenir");
        assert!(upload.is_some(), "yeni çift yükleme vermeli");

        // 3. **İki anahtar bir arada ve cevapları ayrı.** `Whole` hâlâ tofu
        // (tek hücreye gerçekten sığmıyor), `Left` gerçek yuva. İkisinin de
        // önbellekte olması şart: biri eksik olsaydı o istek her karede
        // yeniden cascade yürürdü — ana thread'de, kare bütçesinin ortasında.
        let key = |half| {
            (
                Sprite::Char(UNKNOWN_CHAR),
                Face::Regular,
                SizeClass::Normal,
                half,
            )
        };
        assert_eq!(
            a.slots.get(&key(Half::Whole)),
            Some(&(TOFU, Plane::Mask)),
            "tek hücrelik ret önbellekte kalmalı"
        );
        assert_eq!(
            a.slots.get(&key(Half::Left)).map(|&(slot, _)| slot),
            Some(left.slot),
            "iki hücrelik kabul de önbellekte olmalı"
        );
        assert_eq!(
            a.slots.get(&key(Half::Right)).map(|&(slot, _)| slot),
            Some(left.slot + 1),
            "sağ yarı da önbellekte: ikinci bir kapı turu koşmamalı"
        );
    }

    /// İki hücreye de sığmayan bir istek **kendi anahtarına** yazılıyor.
    ///
    /// Yazılmasaydı reddedilen bir [`Half::Left`] isteği her karede yeniden
    /// cascade yürürdü: ret kolu anahtarı **çözülen** yarıyla kuruyor ve o
    /// kolda çözülen yarı her zaman [`Half::Whole`], yani istenen yarının
    /// anahtarı hiç yazılmazdı. Takma ad artık tofu'yu geçirmediğine göre
    /// boşluk doğrudan bir kare bedeline dönüşürdü.
    #[test]
    fn a_rejected_wide_request_caches_its_own_key() {
        let mut a = atlas(POINT_SIZE, 1.0);
        // Hiçbir fontta olmayan bir kod noktası: cascade glyph veremiyorsa
        // `NoGlyph`, veriyorsa mürekkep kapısı karar veriyor — ikisinde de
        // sonuç tofu ve bu sınamanın sorduğu şey **anahtar**, hangi koldan
        // geldiği değil.
        const NOBODY: char = '\u{10FFFD}';
        let (placed, _) = a.slot(
            Sprite::Char(NOBODY),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        // Kabul edilirse sınama konusuz: bu makinede o karakteri iki hücreye
        // sığdıran bir font var demektir.
        if placed.slot != TOFU {
            return;
        }
        for half in [Half::Whole, Half::Left, Half::Right] {
            assert_eq!(
                a.slots
                    .get(&(Sprite::Char(NOBODY), Face::Regular, SizeClass::Normal, half)),
                Some(&(TOFU, Plane::Mask)),
                "{half:?} anahtarı yazılmadı: o istek her karede cascade yürür"
            );
        }
    }

    /// Setin beş örnek dizisi: bayrak (iki RI), ZWJ, ten rengi ve iki VS16 —
    /// `❤` ile `🌡` tek başına tek sütunlu 78'den, yani iki sütunu VS16
    /// getiriyor.
    const CLUSTERS: [&str; 5] = [
        "\u{1F1F9}\u{1F1F7}",                          // 🇹🇷
        "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}", // 👨‍👩‍👧
        "\u{1F44D}\u{1F3FD}",                          // 👍🏽
        "\u{2764}\u{FE0F}",                            // ❤️
        "\u{1F321}\u{FE0F}",                           // 🌡️
    ];

    /// Dizi sınamalarının ölçeği: **Retina**.
    ///
    /// 13pt@1x'te tek kod noktalı `👍` bile iki hücrelik kapıdan dönüyor
    /// (bayrağın glyph'inde ölçüldü: mürekkep 16.25 pt, iki hücre 15.65 pt;
    /// `👍` aynı kapıdan tofu'ya düşüyor) — 023'ün bugünkü hâli, bu setin konusu değil. O ölçekte dizinin kapıdan dönmesi
    /// şekillendirme hakkında hiçbir şey söylemez ve taban karaktere düşüş
    /// sınaması tofu'ya karşı boşuna yeşil kalırdı.
    const CLUSTER_SCALE: f64 = 2.0;

    /// Dizi **tek glyph**'e şekilleniyor ve renk düzleminde iki yarıyla
    /// geliyor.
    ///
    /// Ölçüt ızgaranın ayırdığı iki sütun: dizi glyph'inin geometrisi tek kod
    /// noktalı emojinin aynısı (035 `context.md` → Ölçülen: şekillendirme),
    /// yani 023'ün iki hücrelik kapısından geçmeli. Sağ yarının boş olmaması
    /// şart — boş bir sağ yarı "iki yuva aldı" sınamasını yeşil bırakıp
    /// ekranda yarım bir emoji çizerdi.
    #[test]
    fn a_cluster_takes_two_colour_slots() {
        let mut a = atlas(POINT_SIZE, CLUSTER_SCALE);
        for text in CLUSTERS {
            let sprite = a.intern(text);
            assert!(
                matches!(sprite, Sprite::Cluster(_)),
                "'{text}' birden çok kod noktası: küme olmalı"
            );
            let before = a.color_occupancy().0;
            let (placed, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
            assert_eq!(
                placed.plane,
                Plane::Color,
                "'{text}' renk düzleminde olmalı"
            );
            assert_eq!(placed.half, Half::Left, "'{text}' iki yarıyla gelmeli");
            let upload = upload.expect("yeni çift yükleme vermeli");
            assert!(
                upload.right.is_some(),
                "'{text}' sağ yarısı aynı dönüşte gelmeli"
            );
            assert!(
                upload.bytes.iter().any(|&b| b > 0),
                "'{text}' sol yarısı boş"
            );
            assert!(
                upload.right_bytes.iter().any(|&b| b > 0),
                "'{text}' sağ yarısı boş"
            );
            assert_eq!(
                a.color_occupancy().0 - before,
                2,
                "'{text}' tam iki renk yuvası harcamalı"
            );
        }
    }

    /// Aynı dizgi aynı kimliği ve aynı yuvayı alıyor; farklı dizgiler farklı
    /// kimlik.
    ///
    /// Kimlik anahtarın parçası, yani ikinci soruluşta yeni bir kimlik
    /// üretmek aynı glyph'i her karede yeniden şekillendirip yeni yuvaya
    /// koymak olurdu — atlas dolana kadar sessizce.
    #[test]
    fn the_same_cluster_is_interned_and_cached_once() {
        let mut a = atlas(POINT_SIZE, CLUSTER_SCALE);
        let first = a.intern(CLUSTERS[0]);
        assert_eq!(a.intern(CLUSTERS[0]), first, "aynı dizgi aynı kimlik");
        let ids: Vec<Sprite> = CLUSTERS.iter().map(|text| a.intern(text)).collect();
        for (i, x) in ids.iter().enumerate() {
            for y in &ids[i + 1..] {
                assert_ne!(x, y, "farklı dizgiler aynı kimliği aldı");
            }
        }
        let (placed, upload) = a.slot(first, Face::Regular, SizeClass::Normal, Half::Left);
        assert!(upload.is_some(), "ilk soruluş yükleme vermeli");
        let occupied = a.color_occupancy().0;
        // Yüz **düz yüze iniyor**: kalın bir satırdaki bayrak ayrı yuva açmamalı.
        for face in [Face::Regular, Face::Bold] {
            let sprite = a.intern(CLUSTERS[0]);
            let (again, upload) = a.slot(sprite, face, SizeClass::Normal, Half::Left);
            assert_eq!(again, placed, "{face:?}: ikinci soruluş aynı cevap");
            assert!(
                upload.is_none(),
                "{face:?}: yüklü yuva yeniden yüklenmemeli"
            );
        }
        assert_eq!(
            a.color_occupancy().0,
            occupied,
            "ikinci soruluş yuva açmamalı"
        );
        // Tek kod noktası küme değil: aynı glyph iki anahtarda tutulmamalı.
        assert_eq!(a.intern("A"), Sprite::Char('A'));
        // Tavan: tablo dolunca yeni dizi taban karakterine iniyor, bilinen
        // dizi kimliğini koruyor.
        let known = a.intern("\u{1F44D}\u{1F3FD}");
        let cap = a.negative_cache_cap();
        for n in 0..cap {
            let _ = a.intern(&format!("\u{1F44D}{n}"));
        }
        assert!(a.clusters.len() <= cap, "tablo tavanı aştı");
        assert_eq!(a.intern("\u{1F44D}\u{1F3FD}"), known);
        assert_eq!(
            a.intern("\u{1F4A9}\u{200D}\u{1F525}"),
            Sprite::Char('\u{1F4A9}')
        );
        assert_eq!(a.intern(""), Sprite::Char(' '));
    }

    /// Tek glyph'e şekillenmeyen dizgi **taban karakterin** cevabını alıyor —
    /// kutu değil, yarım glyph değil (R1.1).
    ///
    /// `👍👍` iki ayrı glyph'e şekilleniyor; ızgara onu hiç kümelemez ama
    /// sınır bu kolu sınamanın en temiz yolu. Cevap bayt bayt `Char('👍')`'nin
    /// ki: ayrı bir atlasta sorulan tek karakterle karşılaştırılıyor, yani
    /// "taban karakter" bir benzetme değil aynı raster.
    #[test]
    fn an_unshaped_cluster_answers_with_its_base_char() {
        const BASE: char = '\u{1F44D}'; // 👍
        let mut reference = atlas(POINT_SIZE, CLUSTER_SCALE);
        let (base, base_upload) = reference.slot(
            Sprite::Char(BASE),
            Face::Regular,
            SizeClass::Normal,
            Half::Left,
        );
        assert_eq!(base.plane, Plane::Color, "taban karakter çizilebilmeli");
        let base_upload = base_upload.expect("ilk soruluş yükleme vermeli");
        let base_bytes = (base_upload.bytes.to_vec(), base_upload.right_bytes.to_vec());

        let mut a = atlas(POINT_SIZE, CLUSTER_SCALE);
        let sprite = a.intern("\u{1F44D}\u{1F44D}");
        let (placed, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
        assert_eq!(
            placed, base,
            "şekillenmeyen dizi taban karakterin cevabını almalı"
        );
        let upload = upload.expect("ilk soruluş yükleme vermeli");
        assert_eq!(
            (upload.bytes.to_vec(), upload.right_bytes.to_vec()),
            base_bytes,
            "raster taban karakterinkiyle bit bit aynı olmalı"
        );
        // Takma ad yazıldı: ikinci soruluş şekillendirmeyi yeniden koşmuyor.
        let (again, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
        assert_eq!(again, placed);
        assert!(upload.is_none(), "takma ad önbellekte olmalı");
        let (right, _) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Right);
        assert_eq!(
            right.slot,
            placed.slot + 1,
            "sağ yarının takma adı da yazılmalı"
        );
    }

    /// Yeniden kurulan atlasta eski kimlik **tofu**, panik değil.
    ///
    /// Interner yuvalarla birlikte düşüyor ([`Atlas::clusters`]); çağıranın
    /// elinde kalmış bir kimlik `slot()`'a — display link'in callback'ine —
    /// gelebilir ve orada bir panik kareyi düşürürdü.
    #[test]
    fn a_stale_cluster_id_is_tofu() {
        let mut a = atlas(POINT_SIZE, CLUSTER_SCALE);
        let sprite = a.intern(CLUSTERS[0]);
        assert!(
            a.ensure(None, POINT_SIZE + 1.0, 1.0, 1.0),
            "anahtar değişti"
        );
        let (placed, upload) = a.slot(sprite, Face::Regular, SizeClass::Normal, Half::Left);
        assert_eq!(placed.slot, TOFU);
        assert!(upload.is_none());
        assert_eq!(
            a.intern(CLUSTERS[0]),
            sprite,
            "yeniden sorulan dizi yeniden kimlik alır"
        );
    }
}
