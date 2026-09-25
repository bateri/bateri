//! Renderer: metallib'i yükler, pipeline'ı kurar, verilen drawable'a bir kare
//! çizer. Drawable'ı kimin sağladığını bilmez. Çizim **asenkrondur**: `commit`
//! GPU'yu beklemez ve kare sayacını `addCompletedHandler` artırır — yani sayaç
//! "sunuldu"yu değil "GPU hatasız bitirdi"yi sayar.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use block2::RcBlock;
use bt_atlas::{Atlas, Face, FontIssue, Half, Metrics, Placed, Plane, SizeClass, Sprite, TOFU};
use bt_core::{Clusters, FontOptions, LinearRgba};
use dispatch2::DispatchData;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBlendFactor, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandBufferStatus,
    MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary,
    MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder,
    MTLRenderPassDescriptor, MTLRenderPipelineDescriptor, MTLRenderPipelineState,
    MTLResourceOptions, MTLScissorRect, MTLSize, MTLStorageMode, MTLStoreAction, MTLTexture,
    MTLTextureDescriptor, MTLTextureUsage, MTLViewport,
};
use objc2_quartz_core::CAMetalDrawable;

use crate::frame::{
    CursorBlock, Frame, FxCell, FxInstance, GlyphCell, GlyphInstance, Instance, RuleCell,
};
use crate::{GpuError, Surface};

/// `addCompletedHandler:`e verilen blok; [`Renderer::completion`] kurar.
///
/// Ayrı bir tip olmasının sebebi ömrü: blok kurulumda bir kez ayrılır ve
/// karelerin tamamı boyunca yaşar. `link.rs` onu ivar'da tutar.
pub(crate) struct Completion(CompletionBlock);

/// `objc2-metal`'in `MTLCommandBufferHandler`'ı ham işaretçidir; blok tipinin
/// kendisi bu. Takma adın işi okunabilirlik: tip tek satıra sığmıyor ve adı
/// `Completion`'ın neyi sardığını söylüyor.
type CompletionBlock = RcBlock<dyn Fn(NonNull<ProtocolObject<dyn MTLCommandBuffer>>)>;

/// build.rs'in ürettiği metallib; derleme zamanında gömülür, dosya yoksa
/// `rustc` düşer — çalışma zamanına kalan tek şey fonksiyon adlarıdır.
static METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/default.metallib"));

/// Font seçiminin kullanıcıya söylenecek sonucu — `bt_atlas::FontIssue`'nun
/// **bu crate'teki** karşılığı.
///
/// Ayrı tip, çünkü `bt-shell` `bt-atlas`'ı görmüyor ve görmemeli
/// ([`CellMetrics`]'in gerekçesi, 003 R5): yeniden ihraç katman tablosunu
/// bulanıklaştırırdı. Metin yok; alt başlığın dizgisini kuran `bt-shell`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FontNotice {
    /// İstenen aile makinede yok; `using` açılan ailenin adı.
    FamilyNotFound { requested: String, using: String },
    /// Aile açıldı ama eşaralıklı değil; reddedilmedi.
    NotMonospaced { family: String },
}

impl From<FontIssue> for FontNotice {
    fn from(issue: FontIssue) -> Self {
        match issue {
            FontIssue::FamilyNotFound { requested, using } => {
                FontNotice::FamilyNotFound { requested, using }
            }
            FontIssue::NotMonospaced { family } => FontNotice::NotMonospaced { family },
        }
    }
}

/// Ayar penceresinin sorusu: `family` açılsa ne söylenirdi — listede
/// olmayan ailenin durumu ([`bt_atlas::family_issue`]). Renderer'sız, çünkü
/// pencere terminal penceresi yokken de açık olabiliyor.
pub fn family_notice(family: &str) -> Option<FontNotice> {
    bt_atlas::family_issue(family).map(FontNotice::from)
}

/// Atlas ve onun dokusu — **tek yerde**.
///
/// Ayrı iki alan olsalardı [`Atlas::ensure`]'ün `true`'su ("atlası yeniden
/// kurdum, dokuyu da yeniden ayır") bir satır düşürmekle kaçardı ve belirti
/// sessiz olurdu: ızgara geometrisi değişmiş bir atlastan bayat bir yuva
/// okumak, yuva aralık içinde kaldığı sürece `slot_origin`'in savunmasına
/// takılmaz ve **başka bir glyph** çizer. Burada sinyal bir hatırlama işi
/// değil, [`Renderer::sync_atlas`]'nin tek satırı: doku düşer, `draw`
/// yenisini kurar.
struct AtlasTexture {
    atlas: Atlas,
    /// `None` → doku henüz kurulmadı ya da `ensure` onu attı.
    ///
    /// Dokuyu **`draw` kuruyor**, `cell_metrics` değil: metrik yolu pencere
    /// boyutlandırmanın sıcak yolunda ve orada bir doku ayırmasına
    /// bağlanmamalı; ayrıca `cell_metrics` hata döndüremez, doku ayırması ise
    /// başarısız olabilir.
    texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    /// `sprite → slot → uv` çözümünün hedefi; alan olması kare başına
    /// yeniden ayırmayı önlüyor. Glyph'ler ve kurallar **tek** liste: ikisi de
    /// aynı pipeline'dan, tek draw call'da çiziliyor ve sıra (kurallar sonda)
    /// listedeki sıradır.
    ///
    /// Yapının değişmezine (atlas ↔ doku) katılmıyor, yalnız onunla aynı
    /// ödüncün altında yaşıyor. "Tek kuşak" garantisini veren bu alan
    /// değil, [`Renderer::encode_glyphs`]'in tek `borrow_mut`'u.
    instances: Vec<GlyphInstance>,
    /// Renk düzleminin dokusu; `None` → henüz hiç emoji görülmedi.
    ///
    /// **Tembel** ve bu bilinçli: doku maske dokusuyla aynı kenarda
    /// (`texture_px`) ama piksel başına dört bayt, yani varsayılan hücrede
    /// 1 MiB yerine 4 MiB. Emoji görmeyen bir oturum onu hiç ödemiyor.
    color_texture: Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    /// Emoji dörtlüleri — maskelerin listesinden **ayrı**.
    ///
    /// Ayrı olmak zorunda: başka bir pipeline, başka bir doku ve başka bir
    /// blend. Aynı listeye karışsalardı tek draw call iki fragment'i birden
    /// isteyemezdi.
    color_instances: Vec<GlyphInstance>,
    /// Yazım efektlerinin instance'ları (030) — iki düzlem **tek** listede,
    /// çünkü `glyph_fx` pipeline'ı iki dokuyu birden bağlıyor ve düzlemi
    /// instance'tan okuyor ([`FxInstance`]'ın `fx[1]`'i).
    fx_instances: Vec<FxInstance>,
}

/// Izgaranın **fiziksel piksel** geometrisi (ölçek uygulanmış): hücre ölçüsü
/// **ve** sol pay. `bt-shell` grid boyutunu, PTY'ye giden `TIOCSWINSZ`'i ve
/// fare çevirisini bundan türetir.
///
/// **İkisi neden tek tipte:** pay `cols` hesabına, çizim orijinine ve fare
/// eşlemesine birden giriyor (010 Karar 3). Üçü ayrı bir sabitten okusaydı
/// bir kare boyunca ayrışabilirlerdi ve belirti "fare bir sütun kayıyor"
/// olurdu — sessiz değil ama geç fark edilen türden. Burada ayrışamazlar:
/// üçü de **aynı değeri** taşıyan tek bir yapıdan okuyor.
///
/// Alanlar `private` ve kurucusu sıfırı eleyen [`CellMetrics::new`]: bu tipin
/// işi bir demeti adlandırmak değil, **taşımak**. `pub` bir alan olsaydı
/// `CellMetrics { cell_px: (0, 0), .. }` `bt-shell`'den kurulabilirdi ve
/// `900.0 / 0.0` → `inf`, `inf as u16` → `65535`, yani 65535×65535'lik bir
/// grid ile o boyda bir `TIOCSWINSZ`. `Session::resize` yalnız sıfır grid'i
/// eliyor; bu sessizce geçerdi. Şimdi geçemiyor: **≥ 1 garantisi tipin
/// içinde**, kaynağı `bt_atlas::Metrics` (`font::round_up` 1'e kırpar).
/// Pay aynı garantiyi **istemiyor**: bölen değil çıkan, ve sıfır payı meşru
/// bir cevap (bkz. [`CellMetrics::gutter_px`]).
///
/// `bt_atlas::Metrics`'i yeniden ihraç **etmiyor**: `bt-shell`'in bir
/// `bt-atlas` tipi görmesi katman tablosunu bulanıklaştırırdı (`CLAUDE.md`),
/// ve atlasın `baseline_px`'i sınırın bu tarafında hiçbir işe yaramaz —
/// glyph'i taban çizgisine oturtmak `bt-atlas`'ın kendi işi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellMetrics {
    cell_px: (u16, u16),
    context_cell_px: u16,
    gutter_px: u16,
    rule_px: u16,
}

impl CellMetrics {
    /// Komut bloğu şeridinin oturduğu sol payın **nokta** cinsinden genişliği;
    /// fiziksel piksele [`Renderer::cell_metrics`] çeviriyor ve **tek** yer
    /// orası.
    ///
    /// Ayar değil sabit (010 Karar 6): `command_gutter` bu sette bilerek yok,
    /// çünkü kayıt anında uygulanan bir ayar üç tüketiciyi aynı karede
    /// güncellemeye zorluyordu. Değeri ürün kararı — şerit artı iki yanında
    /// nefes payı — ve tipik punto/ölçekte `cols`'tan **en çok bir** sütun
    /// götürüyor; ölçülmüş bir sayı değil, o yüzden `docs/OLCUMLER.md`'nin
    /// konusu da değil.
    ///
    /// `private`: payı okuyan herkes onu [`CellMetrics`] ile **taşıyor**,
    /// sabitten değil. İkinci bir okuyucu tam da tipin önlediği ayrışmayı
    /// geri getirirdi.
    const GUTTER_PT: f64 = 8.0;

    /// Hücre ölçüsünün sıfır bileşeni yoksa geometri, varsa `None`.
    ///
    /// Alanlar `private` ama kurucu `pub`: garanti "kimse kuramasın" ile değil
    /// **"kuran sıfırı geçiremesin"** ile sağlanıyor. Aradaki fark sınamada
    /// görünür — `bt-shell`'in grid aritmetiği bir Metal device kurmadan
    /// sınanabilir kalıyor, oysa yalnız `Renderer::cell_metrics`'in
    /// kurabildiği bir tip o testleri GPU'ya bağlardı. Payın **argüman**
    /// olması aynı gerekçenin devamı: gövdeye gizlenmiş bir sabit, payı
    /// sorgulayan sınamaları da GPU'ya bağlardı.
    pub fn new(
        width: u16,
        height: u16,
        context_width: u16,
        gutter: u16,
        rule: u16,
    ) -> Option<Self> {
        // Kapı üçünü birden soruyor: bağlam genişliği de **bölen**
        // (`bt-gpu`'nun bağlam sütun bütçesi) ve sıfır geçseydi ızgaranınki
        // yakalanırken onunki sessizce geçerdi.
        (width > 0 && height > 0 && context_width > 0).then_some(Self {
            cell_px: (width, height),
            context_cell_px: context_width,
            gutter_px: gutter,
            rule_px: rule,
        })
    }

    /// (genişlik, yükseklik).
    ///
    /// Tip `bt-gpu` ile `bt-shell` arasında **taşınıyor**; demet yalnız
    /// değerin tipi bırakmak zorunda olduğu yerlerde açılıyor: sayıya dönüp
    /// bölmeye girerken (`split_into_grid`, `point_to_cell`, tekerleğin satır
    /// birimi) ve `bt-core`'a geçerken (`SessionOptions.cell_px`,
    /// `Session::resize` — `bt-core` `bt-gpu`'yu göremez, katman kuralının
    /// bedeli bu). Bunların dışında demet dolaşmaz; `Frame::clear` tipin
    /// **kendisini** alıyor, çünkü orijini de ondan okuyor.
    pub fn cell_px(self) -> (u16, u16) {
        self.cell_px
    }

    /// Solda ayrılan payın genişliği; ızgara buradan **sonra** başlar.
    ///
    /// Sıfır olabilir ve bu bir hata değil: payı sıfır olan bir geometri
    /// "ızgara kenardan başlıyor" demektir ve çıkarma da bölme de o değerle
    /// doğru çalışır. Üretimde sıfır yalnız dejenere ölçekte çıkar
    /// ([`Renderer::cell_metrics`]); sınamalar payın konu olmadığı yerde
    /// bilerek sıfır veriyor.
    pub fn gutter_px(self) -> u16 {
        self.gutter_px
    }

    /// Kural çizgisinin kalınlığı, piksel — fontun **kendi** alt çizgi
    /// metriği (`bt_atlas::Metrics::underline_px`'in ikinci bileşeni).
    ///
    /// İnce caret'lerin (alt çizgi, dikey çubuk) genişliği buradan geliyor ve
    /// ikinci bir tasarım sabiti uydurulmuyor — chevron'un kalınlığı da aynı
    /// metrikten. Punto ya da font değişince caret de onunla değişiyor.
    pub fn rule_px(self) -> u16 {
        self.rule_px
    }

    /// Dock'un bağlam satırındaki sütun adımı, piksel — küçük yüzün ilerleme
    /// genişliği (`bt_atlas::Atlas::context_cell_w`).
    ///
    /// **Yalnız genişlik**: küçük glyph de büyük yuvaya, büyük hücrenin taban
    /// çizgisine rasterize ediliyor, yani satır yüksekliği ve taban ortak.
    /// Band aritmetiği ([`crate::dock_px`]) bu yüzden hiç değişmiyor — bağlam
    /// satırı kendi bandında duruyor, yalnız harfleri küçük ve sık.
    ///
    /// `cell_px` gibi ≥ 1 ve aynı yapısal gerekçeyle: kaynağı
    /// `font::round_up`, sınırın bu tarafında alan private.
    pub fn context_cell_px(self) -> u16 {
        self.context_cell_px
    }
}

pub struct Renderer {
    /// Kurucuda elde olan device; tampon ayırmak için kare başına
    /// `queue.device()` mesajı atmaya gerek yok.
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    /// Hücre arka planlarını, blok şeritlerini ve dock zeminini çizen
    /// pipeline; instanced quad.
    ///
    /// **İmleç artık burada değil** (015 phase-2): caret kendi fragment'ine
    /// taşındı ([`Renderer::caret`]). Blend bu pipeline'da açık kalıyor ama
    /// **bugün müşterisi yok** — arka planların alfası her zaman `1.0`
    /// ([`bt_core::LinearRgba`]'nın tek kurucusu öyle yazıyor), yani sonuç
    /// opak yazmayla birebir aynı. Açık bırakılmasının sebebi alfayı
    /// isteyecek ilk tüketicinin (seçim vurgusu, dock zemini) bu pipeline'dan
    /// geçecek olması; kapatmak o günü sessiz bir kusura çevirirdi.
    cell_bg: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Glyph'leri çizen pipeline; aynı quad, atlas örneklemesi.
    ///
    /// Ayrı pipeline çünkü **ayrı shader çifti**: fragment atlası örnekliyor
    /// ve imleç uniform'unu okuyor. (Blend durumu artık ikisinde de aynı, yani
    /// ayrılığın sebebi değil.)
    cell: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Renkli emojiyi çizen pipeline; **aynı vertex** (`cell_vertex`), ayrı
    /// fragment ve ayrı blend.
    ///
    /// Dördüncü pipeline olmasının sebebi **tek** ayrım ve o fragment'in
    /// kendisi: rengi **dokudan** alıyor (instance'tan değil) ve imleç
    /// uniform'unu hiç okumuyor. Blend durumu `cell`'inkiyle **aynı** —
    /// `raster::draw_color` ön çarpımı yüklemeden önce geri aldığı için
    /// baytlar düz alfa.
    emoji: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Caret'i çizen pipeline; **aynı vertex**, ayrı fragment.
    ///
    /// Üçüncü pipeline olmasının sebebi `cell_bg`'den ayrı bir şekil dili:
    /// yuvarlak köşe, kenar ve hale bir SDF istiyor ve o hesabı her arka plan
    /// dörtgenine ödetmek kare başına binlerce fragment'e bedel bindirirdi.
    /// Vertex paylaşılıyor, yani ikinci bir köşe yolu yok (R2).
    caret: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Dock'un yazım efektlerini çizen pipeline (030) — **beşinci** ve kendi
    /// vertex'i, kendi instance'ı ([`FxInstance`]).
    ///
    /// `cell`'in vertex'ini paylaşamıyor: dörtlü hücreden efekt payı kadar
    /// şişiyor ve fragment noktayı efektin ters dönüşümüyle glyph uzayına
    /// çeviriyor, yani instance'ın efekt parametrelerini taşıması gerekiyor —
    /// `GlyphInstance`'ı genişletmek bütün glyph listelerinin stride'ını
    /// animasyonlu bir avuç glyph için büyütürdü
    /// (`.tasks/030-dock-yazim-animasyonlari/discussion.md` → Karar 5).
    /// Emoji için ayrı bir kardeş gerekmiyor: blend altı pipeline'da aynı ve
    /// iki doku birden bağlı, düzlem instance'tan.
    glyph_fx: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Fareyle seçimi çizen pipeline (031) — **altıncı**, `Instance`'ı aynen
    /// okuyan kendi vertex'i (`selection_vertex`) ve köşe maskeli fragment'i.
    ///
    /// `cell_bg`'den ayrı olmasının sebebi caret'inkiyle aynı (yuvarlak köşe
    /// bir SDF istiyor ve onu her arka plan dörtgenine ödetmenin anlamı yok);
    /// vertex'in ayrı olmasının sebebi fragment'in kendi dörtgenini bilmek
    /// zorunda olması — caret tek dörtgen olduğu için onu uniform'dan alıyor,
    /// seçimde dörtgen başına bir tane var.
    selection: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Son **gönderilen** karedeki arka plan hücresi sayısı; `make duman`'ın
    /// `hucre=K` jetonu. `frames`'in yanında duruyor çünkü ikisi de aynı
    /// soruya bakan tanı sayaçları ve tek yerden okunmaları gerekiyor.
    /// Dikkat: bu bir **CPU** sayacıdır, GPU'nun o hücreleri boyadığını
    /// kanıtlamaz — onu `cell_bg_paints_pixels_on_the_gpu` yapar.
    last_bg_count: AtomicUsize,
    /// Son **gönderilen** karede çizilen glyph sayısı; `make duman`'ın
    /// `glif=G` jetonu. `last_bg_count` ile aynı gerekçe ve aynı sınır:
    /// CPU sayacı, GPU'nun o glyph'leri boyadığını kanıtlamaz.
    last_glyph_count: AtomicUsize,
    /// Son **gönderilen** karede çizilen kural çizgisi sayısı; `make duman`'ın
    /// `kural=R` jetonu. İki kardeşiyle aynı gerekçe; jetonun neyi göremediği
    /// [`crate::frame::Frame::rule_count`]'ta yazılı ve tek yerde durmalı.
    last_rule_count: AtomicUsize,
    /// İstenen font; atlasın anahtarının aile ve punto yarısı.
    ///
    /// Ayar olarak **saklanıyor**, [`Renderer::cell_metrics`]'e parametre
    /// olarak gitmiyor: gitseydi ayar değeri her yeniden boyutlandırmada çağrı
    /// yoluna girer ve pencere geometrisi fonta dair bir şey bilmek zorunda
    /// kalırdı. Açılış değeri `bt_core::FontOptions::default()`: süreli koşu
    /// ([`Renderer::set_font`]'u hiç çağırmıyor) ile dosyasız kullanıcı aynı
    /// fontu görür ve varsayılan puntonun ikinci bir sahibi yok.
    ///
    /// `RefCell`: `atlas` ile aynı gerekçe. Ödüncü yalnız `set_font` ve
    /// `sync_atlas` alır, ikisi de çağrı sınırında bırakır.
    font: RefCell<FontOptions>,
    /// Font metriğinin ve glyph yuvalarının kaynağı, dokusuyla birlikte.
    ///
    /// `Option`, çünkü atlasın anahtarının ölçek yarısı **pencereden**
    /// gelir ve kurucu pencereyi görmez. Sabit bir 1.0 ile kurmak iki şeyi
    /// birden bozardı: retina makinede font zinciri açılışta boşuna bir kez
    /// daha koşar, ve metriği hiç sormadan atlası okuyan bir yol **sessizce
    /// @1x** çizerdi. `None` o yolu sessiz olmaktan çıkarıyor: glyph'i olan
    /// bir kare atlassız gelirse [`GpuError::NoAtlas`] ile düşer, @1x çizmez.
    ///
    /// `RefCell`, çünkü [`Renderer::cell_metrics`] ve [`Renderer::draw`]
    /// `&self` alıyor ([`Atlas::ensure`] ve [`Atlas::slot`] ise `&mut`) ve
    /// `bt-shell` renderer'ı bir `Rc` içinde tutuyor — paylaşılan bir
    /// sahiplikte `&mut` yolu yok. Ödüncü **yalnız** bu iki metot alır ve
    /// hiçbiri onu bir çağrı sınırının ötesine taşımaz.
    atlas: RefCell<Option<AtlasTexture>>,
    /// **Tamamlanan** kare sayısı; `make duman` bunu okur.
    ///
    /// `Arc`: sayacı artıran tamamlanma bloğu `Renderer`'dan bağımsız yaşar
    /// (Metal onu kendi thread'inde, kendi kopyasıyla çağırır). `Relaxed`
    /// yeter: `fetch_add` sayım kaybetmez ve okuyan yalnız "> 0" sorar.
    frames: Arc<AtomicU64>,
}

impl Renderer {
    /// Çizim hedefinin piksel formatı — **tek kaynak**.
    ///
    /// `_sRGB`: fragment çıktısı **lineer** sayılır ve donanım yazarken
    /// kodlar, yani alfa karıştırma lineer uzayda koşar (glyph'in tek sebebi
    /// bu). Karşılığı `bt_core::color::linear_rgba`; ikisi birlikte değişir —
    /// biri lineerleşmeden ötekine geçilirse palet griye açılır.
    ///
    /// Alan değil `const`: kurucusu tek ve koşulsuz atıyordu, yani örnek
    /// başına saklanan türetilebilir durumdu. `const` olunca değer bir
    /// `Renderer` olmadan da okunabiliyor ve "lineer palet + sRGB olmayan
    /// hedef" temsil edilebilir bir durum olmaktan çıkıyor: ikinci bir
    /// renderer'a (offscreen, ekran görüntüsü) düz `BGRA8Unorm` geçen kişi
    /// gürültülü bir Metal istisnası değil **sessizce yanlış renk** alırdı.
    pub(crate) const PIXEL_FORMAT: MTLPixelFormat = MTLPixelFormat::BGRA8Unorm_sRGB;

    /// Sistem varsayılan device ile; bt-shell yalnız bunu çağırır ve
    /// `objc2-metal`'i hiç görmez.
    ///
    /// Piksel formatı parametre değil, [`Renderer::PIXEL_FORMAT`].
    pub fn system_default() -> Result<Self, GpuError> {
        let device = MTLCreateSystemDefaultDevice().ok_or(GpuError::NoDevice)?;
        // `include_bytes!` 'static verir; kopyasız kurucu doğru olan.
        let data = DispatchData::from_static_bytes(METALLIB);
        let library = device
            .newLibraryWithData_error(&data)
            .map_err(GpuError::Library)?;
        // Arka planlar opak (alfaları `1.0`), yani blend onlar için no-op;
        // açık olmasının tek sebebi imlecin belirmesi (bkz. `cell_bg` alanı).
        let cell_bg = pipeline(&device, &library, "cell_bg_vertex", "cell_bg_fragment")?;
        // Glyph'ler arka planların üstüne **karışarak** geliyor: atlas bir
        // kapsama maskesi, renk instance'tan. Blend lineer uzayda koşuyor ve
        // sebebi tam olarak bu (`PIXEL_FORMAT` → `_sRGB`).
        let cell = pipeline(&device, &library, "cell_vertex", "cell_fragment")?;
        // Emoji: `cell_vertex`'i **aynen** paylaşıyor, fragment'i ayrı:
        // rengi dokudan alıyor, instance'tan değil. Baytlar düz alfa (ön
        // çarpım yüklemeden önce geri alınıyor, `raster::unpremultiply`),
        // yani blend öteki pipeline'larınkiyle aynı.
        let emoji = pipeline(&device, &library, "cell_vertex", "emoji_fragment")?;
        // Caret: `cell_bg_vertex`'i paylaşıyor, fragment'i ayrı. Blend zaten
        // açık ve burada **zorunlu** — hale tanımı gereği yarı saydam.
        let caret = pipeline(&device, &library, "cell_bg_vertex", "caret_fragment")?;
        // Yazım efektleri: kendi vertex'i (şişen dörtlü) ve fragment'i (ters
        // dönüşüm, iki doku).
        let glyph_fx = pipeline(&device, &library, "glyph_fx_vertex", "glyph_fx_fragment")?;
        // Seçim: kendi vertex'i (dörtgeni fragment'e taşıyor) ve köşe maskeli
        // fragment'i; blend yuvarlak köşenin kenar yumuşatması için.
        let selection = pipeline(&device, &library, "selection_vertex", "selection_fragment")?;
        let queue = device.newCommandQueue().ok_or(GpuError::NoCommandQueue)?;

        Ok(Self {
            device,
            caret,
            queue,
            cell_bg,
            cell,
            emoji,
            glyph_fx,
            selection,
            font: RefCell::new(FontOptions::default()),
            atlas: RefCell::new(None),
            last_bg_count: AtomicUsize::new(0),
            last_glyph_count: AtomicUsize::new(0),
            last_rule_count: AtomicUsize::new(0),
            frames: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn surface(&self) -> Surface {
        Surface::new(&self.device, Self::PIXEL_FORMAT)
    }

    /// Verilen backing ölçeğinde ızgara geometrisi: hücre ölçüsü ve sol pay.
    ///
    /// `scale` parametre çünkü ekran ölçeği çalışırken değişebilir
    /// (`windowDidChangeBackingProperties:`, harici ekran) ve atlas ölçeği
    /// önbellek anahtarının parçası olarak taşır: aynı `Renderer` iki ölçekte
    /// iki farklı metrik verir. @1x rasterize edilmiş bir glyph @2x'te
    /// hatasız bulanıklaşır ve belirti yalnız iki ekranlı makinede görünür.
    /// Pay da aynı ölçeğe bağlı ve **aynı çağrıdan** çıkıyor: ikisi ayrı
    /// çağrılardan gelseydi ölçek değişiminde bir kare boyunca ayrışabilirlerdi.
    ///
    /// `bt-shell` `bt-atlas`'ı görmüyor, metrik buradan geçiyor; katman
    /// tablosu (`CLAUDE.md`) değişmeden `CELL_PX` yer tutucusu ölebildi.
    pub fn cell_metrics(&self, scale: f64) -> CellMetrics {
        let (metrics, context_w) = self.sync_atlas(scale);
        let (w, h) = metrics.cell_px;
        // `as u16` doygun: NaN ve negatif ölçek sıfır pay verir (ızgara
        // kenardan başlar, `split_into_grid` ile fare eşlemesi ikisi de doğru
        // çalışır), dev ölçek 65535'te durur. Yuvarlama hücreninkiyle aynı
        // yönde değil ama olması da gerekmiyor: pay bölen değil çıkan, bir
        // piksel oynaması ızgarayı kaydırmaz, yalnız payı bir piksel
        // değiştirir.
        let gutter = (CellMetrics::GUTTER_PT * scale).round() as u16;
        // audit: `bt_atlas::Metrics.cell_px` çıplak bir `pub` alan, yani ≥ 1
        // garantisi bir crate ötede (`font::round_up` 1'e kırpar) ve tipin
        // kendisi taşımıyor. Yapı gövdesiyle kurmak bu boşluğu sessiz
        // bırakırdı; `expect` onu programlama hatasına çevirir. Panik yolu
        // değil: PTY okuma ve ayrıştırma bu satırdan geçmez, burası
        // pencere geometrisi yolu.
        CellMetrics::new(w, h, context_w, gutter, metrics.underline_px.1)
            .expect("bt-atlas hücre ölçüsünü 1'e kırpar")
    }

    /// Atlasın yuva doluluğu: (kullanılan, toplam).
    ///
    /// [`Renderer::cell_metrics`] ile aynı gerekçe: `bt-shell`'in `bt-atlas`
    /// kenarı yok ve olmamalı. Değer `bt-atlas`'ta doğuyor, `bt-gpu` yeniden
    /// yayımlıyor.
    ///
    /// Metrik hiç sorulmadıysa atlas **yok** (alan `Option`, anahtarı
    /// pencereden geliyor) ve cevap `(0, 0)`. Bu bir hata değil, doğru cevap:
    /// açılmamış bir atlasın yuvası da yok. Sıfır uydurulmuş bir değer
    /// olsaydı `panic!` gerekirdi — burası `report_and_exit` yolunda ve
    /// kapanışta panik, raporun kendisini yutardı.
    pub fn atlas_occupancy(&self) -> (usize, usize) {
        self.atlas
            .borrow()
            .as_ref()
            .map_or((0, 0), |tex| tex.atlas.occupancy())
    }

    /// Renk düzleminin yuva doluluğu; `yuva2=U/T` jetonunun kaynağı.
    ///
    /// [`Renderer::atlas_occupancy`] ile aynı gerekçe ve aynı `(0, 0)` kuralı:
    /// açılmamış bir atlasın yuvası da yok.
    pub fn color_atlas_occupancy(&self) -> (usize, usize) {
        self.atlas
            .borrow()
            .as_ref()
            .map_or((0, 0), |tex| tex.atlas.color_occupancy())
    }

    /// İstenen fontu değiştirir; önceki istekten farklıysa `true`.
    ///
    /// Atlası **kurmuyor**, yalnız isteği saklıyor: anahtarı değiştiren ve
    /// dokuyu düşüren tek yer [`Renderer::sync_atlas`] kalıyor. Yeni font
    /// sonraki [`Renderer::cell_metrics`]'te açılır; `true` çağırana "hücre
    /// ölçüsünü yeniden sor, grid'i yeniden kur" der (`bt-shell`'in
    /// `refresh_geometry`'si). [`Renderer::font_notice`] de o ana kadar eski
    /// atlasınkini söyler.
    pub fn set_font(&self, font: &FontOptions) -> bool {
        let mut current = self.font.borrow_mut();
        if *current == *font {
            return false;
        }
        current.clone_from(font);
        true
    }

    /// Açık atlasın fontu için kullanıcıya söylenecek şey; atlas henüz yoksa
    /// ya da söylenecek bir şey yoksa `None`.
    ///
    /// Atlas **en son** [`Renderer::cell_metrics`]'te kuruldu: cevap o
    /// çağrının fontunu söyler. Ölçek değişimi atlası yeniden kursa da aynı
    /// cevabı verir — aile ve eşaralık ölçekle değişmiyor.
    pub fn font_notice(&self) -> Option<FontNotice> {
        let atlas = self.atlas.borrow();
        let issue = atlas.as_ref()?.atlas.font_issue()?;
        Some(FontNotice::from(issue.clone()))
    }

    /// Atlası istenen fonta ve `scale` ölçeğine getirir ve metriğini verir.
    ///
    /// **Atlasın anahtarını (aile + punto + ölçek) değiştiren tek yer
    /// burasıdır** — yani ızgara geometrisini. `draw` atlası okumakla
    /// kalmıyor, yuva da açıyor ([`Atlas::slot`] `&mut` alır) ama ızgarayı
    /// değiştirmiyor; ayrım tam olarak dokunun ne zaman düşmesi gerektiğidir.
    ///
    /// [`Atlas::ensure`]'ün `true`'su burada dokuyu düşürüyor: yuva eşlemesi
    /// ve [`Atlas::texture_px`] değişmiş olabilir, eski boyutlu dokuya yeni
    /// metrikle yazmak sessizce bozardı. Sinyali bir `let _` ile düşürmek
    /// **derleyici tarafından kabul edilir** (`unused_must_use` yalnız çıplak
    /// ifade deyimine bakar), yani `#[must_use]` burada bir bekçi değil bir
    /// niyet beyanı; bekçi bu satırın kendisi.
    fn sync_atlas(&self, scale: f64) -> (Metrics, u16) {
        let font = self.font.borrow();
        let family = font.family.as_deref();
        let mut slot = self.atlas.borrow_mut();
        let atlas_tex = slot.get_or_insert_with(|| AtlasTexture {
            atlas: Atlas::new(family, font.size, scale, font.line_height),
            texture: None,
            instances: Vec::new(),
            color_texture: None,
            color_instances: Vec::new(),
            fx_instances: Vec::new(),
        });
        if atlas_tex
            .atlas
            .ensure(family, font.size, scale, font.line_height)
        {
            atlas_tex.texture = None;
            // **Renk dokusu da düşmek zorunda.** `Atlas::ensure` atlası
            // baştan kuruyor (`*self = Self::new(..)`), yani `color_next`
            // sıfırlanıyor **ve** doku kenarı değişebiliyor (kenar
            // `SLOT_TARGET` ile hücre ölçüsünden türüyor). Eski kenarda
            // kalan bir renk dokusu yeni ızgaranın köşeleriyle yazılırdı:
            // `replaceRegion` dokunun dışına taşar. Cmd+ ile puntoyu
            // büyütmek ya da pencereyi başka ölçekli bir ekrana taşımak bu
            // yolu ekranda emoji varken tetikliyor.
            atlas_tex.color_texture = None;
        }
        // Ödünç değil **metrik** dönüyor: atlas ödüncünün bir çağrı sınırını
        // aşabildiği tek yer burasıydı ve [`Renderer::encode_glyphs`]'in
        // dayandığı özellik tam olarak bunun olmaması. Bağlam genişliği de
        // aynı ödüncün içinden çıkıyor ve aynı sebeple demetle: ikinci bir
        // çağrıda alınsaydı araya düşen bir `ensure` ikisini ayrı atlaslardan
        // verirdi.
        (atlas_tex.atlas.metrics(), atlas_tex.atlas.context_cell_w())
    }

    /// GPU'nun hatasız bitirdiği kare sayısı. Anlamın sahibi artık
    /// `addCompletedHandler`: `commit` etmek bitirmek değildir, bitirmek de
    /// hatasız bitirmek değildir.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Son gönderilen karede çizilen arka plan hücresi sayısı (imleç hariç).
    pub fn last_bg_count(&self) -> usize {
        self.last_bg_count.load(Ordering::Relaxed)
    }

    /// Son gönderilen karede çizilen glyph sayısı.
    pub fn last_glyph_count(&self) -> usize {
        self.last_glyph_count.load(Ordering::Relaxed)
    }

    /// Son gönderilen karede çizilen kural çizgisi sayısı.
    pub fn last_rule_count(&self) -> usize {
        self.last_rule_count.load(Ordering::Relaxed)
    }

    /// Kare tamamlanınca çağrılacak bloğu **bir kez** kurar.
    ///
    /// Blok kare başına kurulmuyor: taşıdığı hiçbir şey kareden kareye
    /// değişmiyor, oysa her kurulum bir heap ayırması ve birkaç `Arc`
    /// sayaç hareketi demek — hepsi tazeleme hızında. Metal `Block_copy` ile
    /// kendi referansını aldığı için aynı blok her komut tamponuna eklenebilir.
    ///
    /// Başarı kolu **komut tamponunu** geçiriyor, `()` değil: kareyi kim
    /// istediyse GPU'nun kendi damgalarını (`GPUStartTime`/`GPUEndTime`) ondan
    /// okuyabilsin diye. Renderer bu damgaları kendisi okumuyor — okusaydı
    /// ölçüm kapalıyken de kare başına iki ObjC çağrısı öderdi ve ölçüm
    /// politikası "ne çizeceğini bilen" tarafa sızardı.
    ///
    /// Blok tek ve paylaşılmış, yani `on_complete`'in yakalayabileceği tek şey
    /// bütün karelerin **ortak** durumudur (R3.2); `Send + Sync` sınırı da o
    /// yüzden var. Kare eşleştirmesi yok — bekleyen iddiaların hiçbiri "şu
    /// kare" sorusunu sormuyor, hepsi dağılım soruyor.
    pub(crate) fn completion(
        &self,
        on_complete: impl Fn(Result<&ProtocolObject<dyn MTLCommandBuffer>, GpuError>)
        + Send
        + Sync
        + 'static,
    ) -> Completion {
        let frames = Arc::clone(&self.frames);
        Completion(RcBlock::new(
            move |cmd: NonNull<ProtocolObject<dyn MTLCommandBuffer>>| {
                // SAFETY: Metal handler'ı tamamlanmış ve canlı bir komut
                // tamponuyla, tampon başına bir kez çağırır.
                let cmd = unsafe { cmd.as_ref() };
                // Tamamlanmak sunulmak değildir: Error durumunda drawable boş
                // kalır ve sayaç artarsa `make duman` siyah pencereyi yeşil geçer.
                if cmd.status() == MTLCommandBufferStatus::Error {
                    on_complete(Err(GpuError::CommandFailed(cmd.error())));
                    return;
                }
                frames.fetch_add(1, Ordering::Relaxed);
                on_complete(Ok(cmd));
            },
        ))
    }

    /// Tek kare: arka planı `clear` ile boya, `frame`'in dikdörtgenlerini çiz,
    /// sun. **Asenkron**: `commit` GPU'yu beklemez, dönen `Ok` yalnız "komut
    /// tamponu yola çıktı" demektir.
    ///
    /// `completion` GPU işi bitirince **Metal'in thread'inde** çağrılır ve
    /// karenin gerçek akıbetini taşır. Sonucu renderer yorumlamaz: yeniden
    /// deneme ve durma koşulu kareyi isteyenin işidir (bkz. `link.rs`), bu
    /// yüzden hata buradan loglanmaz da. Senkron hata `Err` ile döner;
    /// çağıran ikisini de **aynı** politikadan geçirmeli.
    pub(crate) fn draw(
        &self,
        drawable: &ProtocolObject<dyn CAMetalDrawable>,
        clear: LinearRgba,
        frame: &Frame,
        completion: &Completion,
    ) -> Result<(), GpuError> {
        // Metal/CA çağrıları iç geçicileri autorelease havuzuna atar; kare
        // başına bir havuz, run loop'suz bir thread'den çağrılınca birikimi önler.
        autoreleasepool(|_| {
            let cmd = self
                .queue
                .commandBuffer()
                .ok_or(GpuError::NoCommandBuffer)?;
            self.encode_pass(&cmd, &drawable.texture(), clear, frame)?;

            // SAFETY: blok geçerli bir işaretçi ve `completion` çağrı boyunca
            // yaşıyor; Metal `Block_copy` ile kendi referansını alır.
            unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&completion.0)) };
            // Kare yola çıktı: jeton bu noktada güncellenir, encode edilemeyen
            // kare `hucre=` sayısını kirletmez.
            self.last_bg_count
                .store(frame.bg_count(), Ordering::Relaxed);
            self.last_glyph_count
                .store(frame.glyph_count(), Ordering::Relaxed);
            self.last_rule_count
                .store(frame.rule_count(), Ordering::Relaxed);
            cmd.presentDrawable(drawable.as_ref());
            cmd.commit();
            Ok(())
        })
    }

    /// Verilen dokuya tek bir render pass encode eder.
    ///
    /// Drawable'ın dokusunu da offscreen bir dokuyu da aynı yol besler:
    /// sınama GPU'nun gerçekten boyadığını buradan okuyor, yani `draw`'un
    /// kanıtlanan kısmı pencereye bağlı değil.
    fn encode_pass(
        &self,
        cmd: &ProtocolObject<dyn MTLCommandBuffer>,
        texture: &ProtocolObject<dyn MTLTexture>,
        clear: LinearRgba,
        frame: &Frame,
    ) -> Result<(), GpuError> {
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: indeks 0 her render pass'te vardır.
        let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        att.setTexture(Some(texture));
        // Arka planı yükün kendisi boyar. 001 bunu tam ekran bir quad'la
        // yapıyordu; Clear aynı işi çizim çağrısı harcamadan yapıyor.
        att.setLoadAction(MTLLoadAction::Clear);
        let clear = clear.to_array();
        att.setClearColor(MTLClearColor {
            red: f64::from(clear[0]),
            green: f64::from(clear[1]),
            blue: f64::from(clear[2]),
            alpha: f64::from(clear[3]),
        });
        att.setStoreAction(MTLStoreAction::Store);

        let enc = cmd
            .renderCommandEncoderWithDescriptor(&pass)
            .ok_or(GpuError::NoRenderEncoder)?;
        // `?` ile erken dönmek YASAK: encoder `endEncoding` görmeden düşerse
        // Metal "released without endEncoding" istisnası atar ve süreci
        // öldürür — `NoBuffer`'ı zarifçe döndürme amacının tam tersi.
        //
        // Sıra çizim sırasıdır (003 → R4.1): önce komut bloğu şeritleri, sonra arka
        // planlar **ve imleç**, sonra glyph'ler, en sonda kurallar (son ikisi
        // `encode_glyphs`'te, aynı pipeline'da). Ters olsaydı imleç altındaki
        // harfi örterdi — imleç opak ve `Frame`'in arka plan listesinin
        // sonunda; imlecin üstündeki alt çizgi de aynı sıradan bedavaya
        // görünür kalıyor.
        //
        // Şeridin **başta** olması bir örtüşme kararı değil katman kararı: pay
        // ızgaranın solunda ayrılmış bir bölge ve hiçbir hücre oraya
        // düşmüyor (`Frame::push_block`), yani bugün sıra piksel farkı
        // üretmiyor. Zemin katmanı olarak en altta durması ileride paya bir
        // şey daha çizen kişinin doğru varsayımla başlamasını sağlıyor.
        // Viewport tek yerde türetiliyor: iki encoder da aynı dokuya çiziyor
        // ve ayrı ayrı sormaları kare başına iki fazladan objc mesajı ile
        // ayrışabilen iki tanım demekti.
        let viewport_px: [f32; 2] = [texture.width() as f32, texture.height() as f32];
        // **Dikey öteleme burada, tek satırda ve iki pipeline birden.**
        // `Frame` onu listelere işlemiyor (gerekçe `Frame::origin_px`): sink
        // hücreyi basma anında pişiriyor, doluluk sayısı ise döngü bitince
        // doğuyor. Viewport dönüşümü NDC'den pencere koordinatına geçerken
        // uygulanıyor, yani arka plan, glyph, kural ve şerit dördü de aynı
        // miktarda kayıyor — shader'a ve `#[repr(C)]` düzenine dokunmadan.
        //
        // **Boy dokunun boyu kalıyor** ve `viewport_px` uniform'u da: ikisi
        // NDC ölçeğinin iki yarısı ve ayrışırlarsa ızgara ezilir (shader
        // pikseli dokunun boyuna göre normalize ediyor, viewport ise NDC'yi
        // kendi boyuna geriyor). Öteleme bu yüzden viewport'u dokunun
        // **altına** taşırıyor; taşan fragment'leri Metal kırpıyor
        // ("Fragments that lie outside of the viewport are clipped",
        // `MTLRenderCommandEncoder`). Kanaryası `content_sticks_to_the_bottom_*`
        // sınamaları: üst bölge clear rengiyle kalmalı, alt bölge boyanmalı.
        //
        // `znear`/`zfar` Metal'in varsayılanı (0..1): kırpma düzlemleri
        // konumu değiştirmiyor, ama `setViewport` hepsini birden istiyor.
        enc.setViewport(MTLViewport {
            originX: 0.0,
            originY: f64::from(frame.origin_px()),
            width: f64::from(viewport_px[0]),
            height: f64::from(viewport_px[1]),
            znear: 0.0,
            zfar: 1.0,
        });
        let result = self
            // **Blok işaretleri artık sprite**, dikdörtgen değil: dock'un
            // chevron'uyla aynı şekil, aynı renk sözlüğü. Kendi encode'u var
            // ve en altta kalıyor — işaret sol payda duruyor, yani hücrelerin
            // arka planıyla hiç kesişmiyor.
            //
            // Ters çevirme dikdörtgeni **dejenere** veriliyor: caret sol paya
            // hiç gitmiyor ve gerçeğini geçirmek iddiayı "caret payın üstünden
            // geçerse işaretin rengi dönsün"e genişletirdi — istenen bir şey
            // değil.
            .encode_glyphs(
                &enc,
                &[],
                frame.clusters(),
                frame.stripes(),
                &CursorBlock::default(),
                frame.cell_px(),
                viewport_px,
            )
            .and_then(|()| self.encode_quads(&enc, frame.bg_instances(), viewport_px))
            // **Arama zeminden sonra, seçimden önce** (033 Karar 7): önce
            // bütün eşleşmeler, üstüne geçerli eşleşme, en üstte kullanıcının
            // seçimi — Esc geçerli eşleşmeyi seçime çevirdiğinde de seçim
            // görünür kalıyor. Metin hepsinin üstünde kendi renginde.
            .and_then(|()| self.encode_search(&enc, frame, false, viewport_px))
            // **Seçim zeminden sonra, caret'ten ve glyph'lerden önce** (031):
            // metin seçimin üstünde kendi renginde okunuyor ve imleç seçimin
            // üstünde kalıyor — ters videonun "imleç kazanır" kuralı, artık
            // piksel sırasıyla.
            .and_then(|()| {
                self.encode_selection(
                    &enc,
                    frame.selection_instances(),
                    frame.selection_rgba(),
                    frame.selection_radius(),
                    viewport_px,
                )
            })
            // **Caret arka planlardan sonra, glyph'lerden önce** ve gerekçe
            // **dolu** caret'e ait: blok opak, altındaki harf onun üstüne ve
            // `cursor_block`'un ters çevirdiği renkle çiziliyor. Caret kendi
            // pipeline'ına taşındı (015 phase-2) ama sıradaki yeri değişmedi.
            //
            // **İçi boş caret'te gerekçenin iki yarısı da düşüyor** (dolgu yok,
            // `CursorBlock` dejenere) ve bedeli kayıtlı: hücrenin kenarına
            // mürekkep koyan bir glyph halkanın üstüne çiziliyor. Bugün seyrek
            // (kutu çizim henüz yok), 018'de görünür olacak — bilinen sınır,
            // `.tasks/015-imlec-cilasi/phase-3.md`.
            //
            // Bu yuva caret ızgaradayken doluyor; dock bandına girmişse liste
            // boş ve instance aşağıdaki dock encode'unda ([`Frame::push_caret`]).
            .and_then(|()| {
                self.encode_caret(
                    &enc,
                    frame.grid_caret().as_slice(),
                    frame.caret_core(),
                    frame.caret_sdf(),
                    viewport_px,
                )
            })
            .and_then(|()| {
                self.encode_glyphs(
                    &enc,
                    frame.glyphs(),
                    frame.clusters(),
                    frame.rules(),
                    frame.cursor_block(),
                    frame.cell_px(),
                    viewport_px,
                )
            })
            .and_then(|()| self.encode_fill(&enc, frame, viewport_px))
            .and_then(|()| self.encode_dock(&enc, frame, viewport_px));
        enc.endEncoding();
        result
    }

    /// Doldurma bandı: **üçüncü koordinat uzayı**, ızgaranın üstüne.
    ///
    /// Ayrı bir `setViewport` ve gerekçesi dock'unkinin ikizi ama ters yönde:
    /// bant ötelemeden muaf değil, ötelemenin **üstünde** duruyor
    /// (`originY = origin_px − fill_px`, [`Frame::fill_origin_px`]). Satırları
    /// fill-yerel doğuyor ve hangi ekran satırına düştükleri ancak burada,
    /// **encode anında** belli oluyor — push anında pişirilseydi hareket
    /// karesi (listeler korunur, yalnız `origin_px` değişir) bandı yerinde
    /// dondururdu (R3.1).
    ///
    /// Orijin **negatife inebilir** ve bırakılıyor: bandın pencereye sığmayan
    /// en eski satırları tepeden taşıyor ve Metal onları kırpıyor (017
    /// phase-0'ın ölçümü; dock'un `max(0.0)` kırpması oraya ait, çünkü orada
    /// doğru cevap dejenere bir dock). Boy ve `viewport_px` uniform'u yine
    /// dokunun boyu: ikisi NDC ölçeğinin iki yarısı.
    ///
    /// **Sıra: ızgaradan sonra, dock'tan önce.** Izgaranın *listeleri* bandın
    /// içine hiç girmiyor (hepsi `y ≥ origin_px`), giren tek şey ötelemeden
    /// muaf olan caret; band bu yüzden ondan sonra çiziliyor — devir
    /// karelerinde bandın hücreleri caret'in üstünde kalıyor. Dock'tan önce
    /// olması ise zorunlu: dock'un opak zemini en altta kalmak, yani en son
    /// çizilmek zorunda.
    fn encode_fill(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        frame: &Frame,
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        // **Geri alma şeridi** (R2.4): bant sıfır satırsa üçüncü viewport hiç
        // kurulmuyor ve çizilen kare doldurmasız hâliyle bit bit aynı. Dock'u
        // olmayan pencere de bu daldan çıkıyor — orada `Session::fill_rows`
        // koşulsuz sıfır döndürüyor, yani kapı `bt-core`'da açılıyor ve burada
        // yalnız kapanıyor.
        if frame.fill_rows() == 0 {
            return Ok(());
        }
        enc.setViewport(MTLViewport {
            originX: 0.0,
            originY: f64::from(frame.fill_origin_px()),
            width: f64::from(viewport_px[0]),
            height: f64::from(viewport_px[1]),
            znear: 0.0,
            zfar: 1.0,
        });
        self.encode_quads(enc, frame.fill_bg(), viewport_px)
            // Arama bandın zemininden sonra, harflerinden önce — ızgaranın
            // sırası (033 Karar 8); bantta seçim çizilmiyor.
            .and_then(|()| self.encode_search(enc, frame, true, viewport_px))
            .and_then(|()| {
                self.encode_glyphs(
                    enc,
                    frame.fill_glyphs(),
                    // Band ızgarayla aynı `frame()` çağrısından, aynı tablo.
                    frame.clusters(),
                    frame.fill_rules(),
                    // Ters çevirme dikdörtgeni **dejenere** (emsal: sol payın
                    // blok işaretleri): bandın caret yuvası yok — caret'in
                    // ekran satırı yerleşik karede her zaman içeriğin içinde.
                    // Gerçeğini geçirmek, kaymanın ortasında bandın üstünden
                    // geçen bir caret'in altındaki harfi zemin rengine
                    // boyardı: çizilmemiş bir caret için okunmaz bir hücre.
                    &CursorBlock::default(),
                    frame.cell_px(),
                    viewport_px,
                )
            })
    }

    /// Dock yüzeyi: **ikinci koordinat uzayı**, ızgaranın üstüne.
    ///
    /// Ayrı bir `setViewport` ve gerekçesi yapısal: dock ötelemeden muaf olmak
    /// zorunda ve muafiyeti aritmetikle kurmak (`- origin_px`) **çalışmaz** —
    /// [`Frame::clear`] ötelemeyi sıfırlıyor, `set_origin_rows` ise sink'ten
    /// sonra çağrılıyor, yani dock hücreleri basılırken o değer henüz
    /// bilinmiyor. Kendi viewport'u olunca dock listeleri ötelemeyi hiç
    /// görmüyor; kaymanın yerleşip yerleşmemesi dock'u ilgilendirmiyor.
    ///
    /// Orijin dokunun **altına** yaslanıyor (`yükseklik − dock payı`): dock
    /// pencerenin dibinde duruyor ve ızgaranın altında kalan artık şerit
    /// (hücre boyuna bölünmeden artan piksel) dock ile içerik arasında kalıyor.
    /// Boy ve `viewport_px` uniform'u dokunun boyu kalıyor — ızgara
    /// viewport'uyla aynı gerekçe: ikisi NDC ölçeğinin iki yarısı ve
    /// ayrışırlarsa yüzey ezilir.
    ///
    /// **Sıra: en sonda** — ızgaranın encode'larından **ve** doldurma
    /// bandından sonra. Kayma boyunca ızgaranın
    /// öteleme hedefi aşılıyor ve en alt satır dock'un üstüne taşıyor
    /// (`LinkDelegate::set_origin`); dock'un opak zemini onu örtüyor. Ters
    /// sırada taşan satır dock'un metninin üstünde görünürdü.
    fn encode_dock(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        frame: &Frame,
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        // Dock'u olmayan pencere (entegrasyonsuz kabuk, süreli koşu) ikinci
        // viewport'u hiç kurmuyor: `hucre=8 glif=6 kural=15` duman
        // sözleşmesinin ölçüldüğü yol bu daldan geçmiyor.
        if frame.dock().is_none() {
            return Ok(());
        }
        // **Sıfırda kırpılıyor** ve gerekçesi 2026-09-20'de **düzeltildi**.
        // Eski cümle "negatif bir `originY` Metal'in doğrulamasına düşerdi —
        // süreci öldüren bir istisna" diyordu; ölçülmemiş bir varsayımdı ve
        // **yanlıştı**. Ölçüm 017 phase-0: Apple M1 Pro / macOS 26.4.1, API
        // doğrulama katmanı **açıkken** de negatif orijin kabul ediliyor ve
        // viewport'un üstünde kalan fragment'ler kırpılıyor; tanık
        // [`tests::a_negative_viewport_origin_draws_and_clips_from_the_top`].
        //
        // Kırpma yine de **kalıyor**, çünkü kendi gerekçesi duruyor:
        // dock'tan alçak bir pencerede (simge durumuna inerken ya da kullanıcı
        // pencereyi dibe kadar kısarken) fark negatife iner ve doğru cevap
        // dejenere — dock pencerenin tamamını kaplar. Negatif bırakılsaydı
        // dock kendi bandının üstüne, yani ızgaranın alanına taşardı.
        // Izgaranın payı zaten sıfır satıra inmiş oluyor (`split_into_grid`)
        // ve `Session::resize` o boyutu yoksayıyor.
        //
        // **İki orijin** (032): zemin ve saç çizgileri **çizilen bandın**
        // viewport'undan (`yükseklik − bant`, animasyonun o anki değeri),
        // hücreler, caret ve efektler **yerleşimin** viewport'undan
        // (`yükseklik − yerleşim`). Hücreler push anında pişiyor ve hareket
        // karesi onları yeniden basmıyor; yerleşim dibe yaslı olduğu için bant
        // büyüyüp küçülürken metin yerinde kalıyor, yalnız bandın tepesi
        // yükselip iniyor. Bant yerleşime eşitken (dinlenen kare) iki orijin
        // aynı sayı ve kare 032'den önceki hâliyle bit bit aynı.
        let band_y = (viewport_px[1] - frame.dock_band_px()).max(0.0);
        let origin_y = (viewport_px[1] - frame.dock_layout_px()).max(0.0);
        enc.setViewport(viewport_at(band_y, viewport_px));
        // Zemin ve ayraç önce: dock'un kendi arka planları (vurgu aralıkları,
        // caret) onların üstüne gelmek zorunda.
        let ground = self.encode_quads(enc, &frame.dock_ground(viewport_px[0]), viewport_px);
        enc.setViewport(viewport_at(origin_y, viewport_px));
        // **Büyüyen bant kırpıyor.** Yerleşim bandın o anki tepesinin üstüne
        // taşıyorsa (bant henüz yükselmedi) taşan giriş satırları zeminsiz,
        // ızgaranın alt satırlarının üstüne çizilirdi. Kırpma yalnız o
        // karelerde: dinlenen bantta kurulsaydı yazım efektlerinin saç
        // çizgisinin üstüne taşan payını (`glyph_fx.metal` → `FX_PAD`) keserdi.
        let clipped = band_y > origin_y;
        if clipped {
            enc.setScissorRect(scissor_below(band_y, viewport_px));
        }
        let result = ground
            .and_then(|()| self.encode_quads(enc, frame.dock_bg(), viewport_px))
            // **Seçim ızgaradakiyle aynı sırada** (031 R3.2): vurgu
            // aralıklarının zemininden sonra, caret'ten ve glyph'lerden önce —
            // metin seçimin üstünde kendi renginde, caret seçimin üstünde.
            .and_then(|()| {
                self.encode_selection(
                    enc,
                    frame.dock_selection_instances(),
                    frame.selection_rgba(),
                    frame.selection_radius(),
                    viewport_px,
                )
            })
            // Caret'in dock yuvası: opak zeminden **sonra** (yoksa zemin onu
            // örterdi) ve glyph'lerden **önce** (yoksa harfi boyardı).
            // Instance pencere uzayında doğuyor, viewport ise dock-yerel:
            // farkı burada geri veriyoruz. Devir karelerinde caret dock'un
            // bandına taşıyor ve bu encode en sonda olduğu için her şeyin
            // üstünde kalıyor — yarısı kırpılmış bir blok görünmüyor.
            .and_then(|()| {
                // **Caret makasın dışında**: devirde ya da yeni bir giriş
                // satırında bandın tepesine değen caret yarısı kesik bir blok
                // olurdu; bütün kalıp bandın üstünde bir kare kadar görünmesi
                // kesik bir bloktan iyi (`/code-review`).
                if clipped {
                    enc.setScissorRect(scissor_below(0.0, viewport_px));
                }
                let caret = self.encode_caret(
                    enc,
                    frame.dock_caret(origin_y).as_slice(),
                    frame.caret_core(),
                    frame.caret_sdf(),
                    viewport_px,
                );
                if clipped {
                    enc.setScissorRect(scissor_below(band_y, viewport_px));
                }
                caret
            })
            // **Hayaletler dock glyph'lerinden önce** (030): satır ortasında
            // silinen harfin yerine kayan harf hayaletin üstünde durmalı —
            // metin anında akıyor, hayalet onun altında sönüyor.
            .and_then(|()| {
                self.encode_fx(
                    enc,
                    frame.dock_ghosts(),
                    frame.fx_clusters(),
                    frame.dock_fx_heat(),
                    frame.cell_px(),
                    viewport_px,
                    origin_y,
                )
            })
            .and_then(|()| {
                // **Gelişler glyph'lerden sonra, kurallardan önce** ve bu
                // yüzden uçuşta geliş varken çağrı ikiye bölünüyor: altı çizili
                // bir harf gelirken çizgisi onun **üstünde** kalmalı, yoksa
                // efekt bitip statik çizime dönülen karede çizgi harfin
                // altından üstüne sıçrardı (`t = 1`'de piksel eşitliği,
                // `plan.md` → R5). Uçuşta geliş yoksa çağrı bugünkü tek çağrı.
                let arrivals = frame.dock_arrivals();
                let (glyph_rules, late_rules) = if arrivals.is_empty() {
                    (frame.dock_rules(), &[][..])
                } else {
                    (&[][..], frame.dock_rules())
                };
                // Caret'in dikdörtgeni fragment'in `[[position]]`'ı ile
                // karşılaştırılıyor ve o koordinat viewport dönüşümünden
                // **sonraki**, yani pencere uzayı — ızgaranınkiyle **aynı**
                // dikdörtgen. Tek caret, tek ters çevirme: caret ızgaradaysa
                // dock'un glyph'leri onunla zaten kesişmiyor.
                self.encode_glyphs(
                    enc,
                    frame.dock_glyphs(),
                    frame.dock_clusters(),
                    glyph_rules,
                    frame.cursor_block(),
                    frame.cell_px(),
                    viewport_px,
                )
                .and_then(|()| {
                    self.encode_fx(
                        enc,
                        arrivals,
                        // Gelişler statik glyph'in kopyası: dock'un tablosu.
                        frame.dock_clusters(),
                        frame.dock_fx_heat(),
                        frame.cell_px(),
                        viewport_px,
                        origin_y,
                    )
                })
                .and_then(|()| {
                    self.encode_glyphs(
                        enc,
                        &[],
                        frame.dock_clusters(),
                        late_rules,
                        frame.cursor_block(),
                        frame.cell_px(),
                        viewport_px,
                    )
                })
            });
        // Kırpma geri alınıyor: dock en son çiziliyor ama sıradaki bir encode
        // (bugün yok) bandın altında kalmış bir makasla başlamamalı.
        if clipped {
            enc.setScissorRect(scissor_below(0.0, viewport_px));
        }
        result
    }

    /// Dock'un yazım efektlerini encode eder (030) — beşinci pipeline
    /// (`glyph_fx`), iki doku ve `heat`'in kızgın rengi
    /// ([`Frame::dock_fx_heat`]).
    ///
    /// **[`CursorBlock`] bağlanmıyor**: efekt caret'in üstünde kendi renginde
    /// çiziliyor, bloğun ters çevirmesine girmiyor. Ölçüldü (kullanıcı
    /// "animasyonlar hiç belli olmuyor" dedi): gelişin ilk anı henüz
    /// ayrılmamış caret'in içinde, Backspace'te ise caret hayaletin sütununa
    /// geliyor ve hayalet baştan sona bloğun içinde oynuyordu — ters çevrilen
    /// efekt caret'in bir parçası gibi okunuyordu. Bedeli: caret bir gelişin
    /// üstünde dururken efekt bitseydi devir karesinde harfin rengi ters
    /// çevrilmiş hâline sıçrardı; insert kipinde caret yazılan harfin
    /// üstünden efektten çok önce ayrılıyor.
    ///
    /// **Kendi viewport'unda, dock'unkinde değil**: dock'un viewport'u
    /// bandın tepesinden başlıyor ve onun üstü kırpılıyor, oysa `drop`
    /// hücrenin üstünden düşüyor, `sublime` yukarı süzülüyor ve giriş
    /// satırının üstünde yalnız ince bir nefes payı var — ilk kareler yarısı
    /// kesik bir harf gösteriyordu (offscreen kareler, phase-5). Efekt
    /// pencere uzayında çiziliyor (instance'lar `origin_y` kadar iniyor) ve
    /// payı kadar (`glyph_fx.metal` → `FX_PAD`) saç çizgisinin üstüne
    /// taşabiliyor; dock'un viewport'u çağrıdan sonra geri kuruluyor.
    ///
    /// Yuva çözümü [`Renderer::encode_glyphs`]'teki gibi atlas ödüncünün
    /// içinde doğup ölüyor. Renk dokusu henüz yoksa (hiç emoji görülmedi) renk
    /// düzlemindeki instance zaten doğamıyor; doğduysa ve doku kurulamadıysa
    /// o instance **çizilmiyor** — maske dokusunu renk diye okumak rastgele
    /// piksel olurdu.
    // `encode_glyphs`'in gerekçesi: tablo `cells`'in yarısı.
    #[allow(clippy::too_many_arguments)]
    fn encode_fx(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        cells: &[FxCell],
        clusters: &Clusters,
        heat: &[f32; 4],
        cell_px: [f32; 2],
        viewport_px: [f32; 2],
        origin_y: f32,
    ) -> Result<(), GpuError> {
        if cells.is_empty() {
            return Ok(());
        }
        let mut atlas = self.atlas.borrow_mut();
        let atlas_tex = atlas.as_mut().ok_or(GpuError::NoAtlas)?;
        atlas_tex.prepare_fx(&self.device, cells, clusters)?;
        if atlas_tex.color_texture.is_none() {
            atlas_tex
                .fx_instances
                .retain(|instance| (instance.fx[1] as u32 >> 5) & 1 == 0);
        }
        if atlas_tex.fx_instances.is_empty() {
            return Ok(());
        }
        for instance in &mut atlas_tex.fx_instances {
            instance.pos[1] += origin_y;
        }
        // audit: `prepare_fx` `Ok` döndüyse dokuyu kurmuştur.
        let mask = atlas_tex.texture.as_ref().expect("prepare_fx dokuyu kurdu");
        // Renk dokusu yoksa maske ikinci yuvaya da bağlanıyor: bağlanmamış
        // bir doku yuvası Metal doğrulamasında hata, ve renk düzleminde
        // instance kalmadığı için okunmuyor.
        let color = atlas_tex.color_texture.as_ref().unwrap_or(mask);
        let (cw, ch) = atlas_tex.atlas.metrics().cell_px;
        let (tw, th) = atlas_tex.atlas.texture_px();
        let uv_size: [f32; 2] = [f32::from(cw) / f32::from(tw), f32::from(ch) / f32::from(th)];
        // Düzen `FxInstance`'ın `offset_of` assert'leriyle `glyph_fx.metal`'e
        // bağlı.
        let buffer = self.instance_buffer(&atlas_tex.fx_instances)?;
        enc.setViewport(viewport_at(0.0, viewport_px));
        enc.setRenderPipelineState(&self.glyph_fx);
        // İndeksler `glyph_fx.metal`'in `[[buffer(n)]]`/`[[texture(n)]]`
        // bildirimleriyle aynı; fragment'in tampon alanı vertex'inkinden ayrı.
        vertex_uniform(enc, &viewport_px, 1);
        vertex_uniform(enc, &cell_px, 2);
        fragment_uniform(enc, &cell_px, 1);
        fragment_uniform(enc, &uv_size, 2);
        fragment_uniform(enc, heat, 3);
        // SAFETY: tampon ve dokular bu blok boyunca yaşıyor.
        unsafe {
            enc.setVertexBuffer_offset_atIndex(Some(&buffer), 0, 0);
            enc.setFragmentTexture_atIndex(Some(mask.as_ref()), 0);
            enc.setFragmentTexture_atIndex(Some(color.as_ref()), 1);
            enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                MTLPrimitiveType::TriangleStrip,
                0,
                4,
                atlas_tex.fx_instances.len(),
            );
        }
        enc.setViewport(viewport_at(origin_y, viewport_px));
        Ok(())
    }

    /// Instance dilimini kare başına yeni bir Metal tamponuna kopyalar.
    ///
    /// Kare başına yeni tampon: üçlü tamponlama bilinçli olarak reddedildi
    /// (002 discussion.md → Muhakeme), `/measure` sonrası yeniden bakılır.
    /// Komut tamponu buffer'ı tamamlanana kadar tutar. Karar **burada** tek
    /// yerde: her çizim yolu bu fonksiyondan geçiyor, yani değişirse hepsi
    /// birden değişir.
    fn instance_buffer<T>(
        &self,
        instances: &[T],
    ) -> Result<Retained<ProtocolObject<dyn MTLBuffer>>, GpuError> {
        // SAFETY: `instances` çağrı boyunca yaşıyor ve Metal baytları
        // kurucuda kopyalar; uzunluk dilimin kendi baytı. Baytların **düzeni**
        // bu fonksiyonun sözleşmesi değil — onu her çağrı yerindeki tipin
        // `offset_of` assert'leri `.metal` tarafına bağlıyor.
        unsafe {
            self.device.newBufferWithBytes_length_options(
                NonNull::from(instances).cast(),
                size_of_val(instances),
                MTLResourceOptions::StorageModeShared,
            )
        }
        .ok_or(GpuError::NoInstanceBuffer)
    }

    /// `cell_bg` pipeline'ının tek çizim yolu: dilimi tampona koyar ve
    /// instanced bir dörtlü çizer.
    ///
    /// **Yeni pipeline yok** (010 → R4.2): şerit de arka plan da aynı genel piksel
    /// dörtgeni — konum, boyut, lineer renk. İkisinin ayrı çağrı olmasının
    /// sebebi listelerinin ayrı olması ([`Frame::stripes`]), düzenlerinin
    /// farklı olması değil; hangi listenin hangi sırada geçtiğini çağrı yeri
    /// ([`Renderer::encode_pass`]) söylüyor. Liste başına bir sarmalayıcı
    /// metot yazmak yalnız eşlenecek yüzey üretirdi (`/code-review`, 010 kapı).
    fn encode_quads(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        instances: &[Instance],
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        // Sıfır uzunluklu `newBufferWithBytes` Metal doğrulamasında geçersiz;
        // hücresiz karede clear yükü tek başına yeter. Şerit tarafında bu dal
        // **normal hâl**: entegrasyonsuz oturumda hiç blok yok.
        if instances.is_empty() {
            return Ok(());
        }
        // Düzen `Instance`'ın `offset_of` assert'leriyle `cell_bg.metal`'e bağlı.
        enc.setRenderPipelineState(&self.cell_bg);
        self.draw_quads(enc, instances, viewport_px)
    }

    /// Seçimin parçalarını encode eder — [`Renderer::encode_caret`]'ın
    /// kardeşi: altıncı pipeline, iki fragment uniform'u (renk, yarıçap).
    /// Uniform'lar çağrı başına tek: bir pencerede tek seçim ve tek renk var,
    /// arama ise rol başına bir çağrı yapıyor ([`Renderer::encode_search`]);
    /// köşe kararı instance'ın maskesinde.
    fn encode_selection(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        instances: &[Instance],
        rgba: [f32; 4],
        radius: f32,
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        if instances.is_empty() {
            return Ok(());
        }
        enc.setRenderPipelineState(&self.selection);
        // İndeksler `selection_fragment`'in bildirimleriyle aynı.
        fragment_uniform(enc, &rgba, 0);
        fragment_uniform(enc, &radius, 1);
        self.draw_quads(enc, instances, viewport_px)
    }

    /// Arama vurgusunu encode eder (033): seçimin pipeline'ından iki çağrı,
    /// rol başına bir — renk uniform, yani iki rol iki encode. Sıra
    /// `search_match` → `search_current`: geçerli eşleşme ötekilerin
    /// üstünde. `fill` bandın listelerini seçiyor; viewport'u çağıran kurmuş.
    /// Arama kapalıyken iki liste de boş ve encoder hiçbir şey görmüyor.
    fn encode_search(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        frame: &Frame,
        fill: bool,
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        let (matched, current) = if fill {
            (
                frame.fill_search_match_instances(),
                frame.fill_search_current_instances(),
            )
        } else {
            (
                frame.search_match_instances(),
                frame.search_current_instances(),
            )
        };
        let radius = frame.selection_radius();
        self.encode_selection(enc, matched, frame.search_match_rgba(), radius, viewport_px)
            .and_then(|()| {
                self.encode_selection(
                    enc,
                    current,
                    frame.search_current_rgba(),
                    radius,
                    viewport_px,
                )
            })
    }

    /// Caret'i encode eder — [`Renderer::encode_quads`]'ın kardeşi, tek farkı
    /// üçüncü pipeline ve iki fragment uniform'u.
    ///
    /// Dilim ya boş ya **tek** elemanlı: caret kare başına tek dörtgen ve iki
    /// yuvadan yalnız biri dolu ([`Frame::push_caret`]). `&[Instance]` alması
    /// yine de doğru — `as_slice()` çağrı yerlerinde `Option`'ı dilime
    /// çeviriyor ve boş dal `encode_quads`'takiyle aynı sebeple erken dönüyor
    /// (sıfır uzunluklu `newBufferWithBytes` Metal doğrulamasında geçersiz).
    fn encode_caret(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        instances: &[Instance],
        core: [f32; 4],
        shape: [f32; 4],
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        if instances.is_empty() {
            return Ok(());
        }
        // **Uniform kare başına tek, instance sayısı değil.** İki caret
        // girdiği gün ikisi de aynı `core` dikdörtgenine göre SDF hesaplar ve
        // ikincisi ya boş ya tuhaf kırpılmış çıkar — sessiz bir kusur
        // (`/code-review`). Sözleşme doc'ta yazılıydı, artık koda da bağlı.
        debug_assert!(instances.len() == 1, "caret kare başına tek dörtgen");
        enc.setRenderPipelineState(&self.caret);
        // İndeksler `cell_bg.metal`'deki `caret_fragment`'in bildirimleriyle
        // aynı; fragment'in tampon alanı vertex'inkinden **ayrı**.
        fragment_uniform(enc, &core, 0);
        fragment_uniform(enc, &shape, 1);
        self.draw_quads(enc, instances, viewport_px)
    }

    /// İki çizim yolunun **ortak gövdesi**: tampon + vertex uniform'u + çizim.
    ///
    /// Ayrı fonksiyon, çünkü kare başına tampon ayırma kararı tek yerde
    /// kalmalı ([`Renderer::instance_buffer`]'ın doc'u bunu söylüyordu ve
    /// caret kendi encode'unu kazanınca gövde ikiye kopyalanmıştı). Üçlü
    /// tamponlamaya geçilirse ya da çizim çağrısı değişirse tek yer değişiyor.
    ///
    /// Pipeline ve fragment uniform'ları **çağıranın**: ikisi de yola göre
    /// ayrışan tek şey.
    fn draw_quads(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        instances: &[Instance],
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        let buffer = self.instance_buffer(instances)?;
        vertex_uniform(enc, &viewport_px, 1);
        // SAFETY: tampon bu blok boyunca yaşıyor; köşe verisi `vertex_id`'den.
        unsafe {
            enc.setVertexBuffer_offset_atIndex(Some(&buffer), 0, 0);
            enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                MTLPrimitiveType::TriangleStrip,
                0,
                4,
                instances.len(),
            );
        }
        Ok(())
    }

    /// Glyph'leri **ve kuralları** encode eder: yuva çözümü, eksik yuvaların
    /// yüklenmesi ve tek instanced çizim çağrısı. İkisi tek tamponda ve tek
    /// çağrıda, kurallar sonda (`AtlasTexture::prepare`).
    ///
    /// Atlas ödüncü **bu fonksiyonun içinde doğar ve burada ölür**. Yuva
    /// çözümünü `Session::frame`'in sink'ine hoist etmek doğal refleks
    /// (sink zaten hücre başına koşuyor) ama `link.rs` `Frame`'in ödüncünü
    /// `draw` boyunca tutuyor: aynı şekil atlas için kopyalansaydı ilk
    /// glyph'li karede `BorrowMutError` olurdu. Panik çizim yolunda ve
    /// `Retry` `GpuError` için tasarlandı, unwind için değil.
    // Tablo `glyphs`'in ayrılmaz yarısı (kimlikleri onu gösteriyor); ikisini
    // bir yapıya sarmak her çağrı yerine bir kurucu eklerdi.
    #[allow(clippy::too_many_arguments)]
    fn encode_glyphs(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        glyphs: &[GlyphCell],
        clusters: &Clusters,
        rules: &[RuleCell],
        cursor: &CursorBlock,
        cell_px: [f32; 2],
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        // Kapı ikisini birden soruyor: yalnız kural taşıyan bir kare (boş bir
        // satırın altındaki kıvrım) buradan geçmeli, hiçbir şey taşımayan kare
        // ise sıfır uzunluklu `newBufferWithBytes`'a ulaşmamalı.
        if glyphs.is_empty() && rules.is_empty() {
            return Ok(());
        }
        let mut atlas = self.atlas.borrow_mut();
        // "Önce metriği sor" sözleşmesi: atlasın anahtarı pencereden geliyor
        // ve `cell_metrics` onu kuran tek yer. Buraya `None` ile gelmek
        // "ölçeği hiç söylemeden glyph çizmek" demek; sessizce @1x bir atlas
        // uydurmak yerine kare düşer.
        let atlas_tex = atlas.as_mut().ok_or(GpuError::NoAtlas)?;
        atlas_tex.prepare(&self.device, glyphs, clusters, rules)?;
        // audit: `prepare` `Ok` döndüyse dokuyu kurmuştur; tek çıkış yolu `?`.
        let atlas_texture = atlas_tex.texture.as_ref().expect("prepare dokuyu kurdu");
        let instances = &atlas_tex.instances;

        // Hücre boyutu **karenin** (`Frame::clear`), uv boyutu **atlasın**.
        // İkisi normalde aynı ölçekten doğar; ayrıştıkları tek pencere ölçek
        // değişimiyle geometri olayı arasındaki tek karedir ve orada glyph
        // esner, bozulmaz — bir sonraki olay ikisini eşitler. Dörtlünün boyu
        // atlastan **alınamaz**: konum da (`GlyphCell::pos`), altındaki arka
        // plan da karenin ölçüsünden doğuyor ve yalnız boyu atlasa bağlamak
        // glyph'i hücresinden kaydırırdı.
        let (cw, ch) = atlas_tex.atlas.metrics().cell_px;
        let (tw, th) = atlas_tex.atlas.texture_px();
        let uv_size: [f32; 2] = [f32::from(cw) / f32::from(tw), f32::from(ch) / f32::from(th)];

        // **Emoji glyph'lerden ÖNCE, caret'ten sonra.** `encode_pass`'in
        // yazılı sırası (şeritler → arka planlar + caret → glyph + kural)
        // bozulmuyor, araya bir draw giriyor. Sıranın iki şartı var: emoji
        // arka planı örtmeli (o yüzden arka planlardan sonra) ve kural
        // çizgileri emojinin de üstünde kalmalı (o yüzden glyph'lerden önce —
        // üstü çizili bir emoji vurgulanmış görünmeli). Caret'ten sonra
        // olması bir karar: emoji opak, yani mürekkebinin altındaki caret
        // örtülüyor ve caret onun çevresinde bir halka olarak görünüyor.
        // Alternatifi emojiyi caret'ten önce çizmekti ve o hâlde blok caret
        // emojiyi **tümden** kapatırdı.
        //
        // Aynı ekleme **üç yüzeyde** birden kazanılıyor, çünkü bu fonksiyon
        // kare başına dört kez koşuyor (şeritler, ızgara, doldurma bandı,
        // dock) — 017'nin dersi tek yerde ödeniyor.
        if !atlas_tex.color_instances.is_empty() {
            // Doku yoksa liste de boş olmalıydı; yine de kapı: `ColorPlane`
            // doku ayıramazsa yüklemeyi atlıyor ve o karede liste dolu ama
            // doku `None` olabiliyor.
            if let Some(color_texture) = atlas_tex.color_texture.as_ref() {
                let color_buffer = self.instance_buffer(&atlas_tex.color_instances)?;
                enc.setRenderPipelineState(&self.emoji);
                vertex_uniform(enc, &viewport_px, 1);
                vertex_uniform(enc, &cell_px, 2);
                vertex_uniform(enc, &uv_size, 3);
                // **İmleç uniform'u yazılmıyor**: `emoji_fragment` onu hiç
                // okumuyor (rengi dokudan alıyor, paletten değil).
                // SAFETY: tampon ve doku bu blok boyunca yaşıyor; indeksler
                // `cell.metal`'in bildirimleriyle aynı.
                unsafe {
                    enc.setVertexBuffer_offset_atIndex(Some(&color_buffer), 0, 0);
                    enc.setFragmentTexture_atIndex(Some(color_texture.as_ref()), 0);
                    enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                        MTLPrimitiveType::TriangleStrip,
                        0,
                        4,
                        atlas_tex.color_instances.len(),
                    );
                }
            }
        }

        // **Maske listesi boş olabilir ve kapı bu yüzden burada.** Yalnız
        // emoji taşıyan bir kare mümkün (`glyphs` dolu ama hepsi renk
        // düzlemine gitti) ve o hâlde sıfır uzunluklu bir
        // `newBufferWithBytes` doğardı — fonksiyonun başındaki kapı
        // `GlyphCell`'leri sayıyor, düzleme göre ayrılmış **instance**'ları
        // değil.
        if instances.is_empty() {
            return Ok(());
        }
        // Düzen `GlyphInstance`'ın `offset_of` assert'leriyle `cell.metal`'e bağlı.
        let buffer = self.instance_buffer(instances)?;

        enc.setRenderPipelineState(&self.cell);
        // İndeksler `cell.metal`'in `[[buffer(n)]]` bildirimleriyle aynı.
        vertex_uniform(enc, &viewport_px, 1);
        vertex_uniform(enc, &cell_px, 2);
        vertex_uniform(enc, &uv_size, 3);
        // Fragment'in tampon indeksleri **ayrı bir alan**: vertex'in 0'ı
        // instance tamponu, fragment'in 0'ı imleç bloğu. Düzeni `CursorBlock`'un
        // `offset_of` assert'leri `cell.metal`'e bağlıyor.
        fragment_uniform(enc, cursor, 0);
        // SAFETY: tampon ve doku bu blok boyunca yaşıyor; doku indeksi
        // `cell.metal`'in `[[texture(0)]]` bildirimiyle aynı.
        unsafe {
            enc.setVertexBuffer_offset_atIndex(Some(&buffer), 0, 0);
            enc.setFragmentTexture_atIndex(Some(atlas_texture.as_ref()), 0);
            enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                MTLPrimitiveType::TriangleStrip,
                0,
                4,
                instances.len(),
            );
        }
        Ok(())
    }
}

/// Tek bir uniform'u vertex aşamasının `[[buffer(index)]]`'ine yazar.
///
/// Güvenli ve jenerik: SAFETY yükümlülüğünün tamamı ("işaretçi geçerli,
/// uzunluk tipin kendi baytı, Metal encode anında kopyalar") burada karşılanıyor.
/// **Taşımadığı** şey düzen sözleşmesi — hangi indeksin hangi shader
/// bildirimine karşılık geldiği çağrı yerinin işi ve orada yazılı.
fn vertex_uniform<T>(enc: &ProtocolObject<dyn MTLRenderCommandEncoder>, value: &T, index: usize) {
    // SAFETY: `value` çağrı boyunca yaşıyor ve Metal baytları encode anında
    // kopyalıyor; uzunluk `T`'nin kendi baytı.
    unsafe {
        enc.setVertexBytes_length_atIndex(NonNull::from(value).cast(), size_of_val(value), index);
    }
}

/// [`vertex_uniform`]'ın fragment aşaması kardeşi; aynı sözleşme, aynı sınır.
///
/// İki aşamanın tampon indeksleri **ayrı alanlardır**: fragment'in `0`'ı
/// vertex'in `0`'ıyla (instance tamponu) çakışmaz. Ayrı fonksiyon olmasının
/// sebebi de bu — tek bir sarmalayıcıya aşamayı parametre yapmak, çağrı
/// yerinde indeksin hangi alana ait olduğunu okunmaz kılardı.
fn fragment_uniform<T>(enc: &ProtocolObject<dyn MTLRenderCommandEncoder>, value: &T, index: usize) {
    // SAFETY: `value` çağrı boyunca yaşıyor ve Metal baytları encode anında
    // kopyalıyor; uzunluk `T`'nin kendi baytı.
    unsafe {
        enc.setFragmentBytes_length_atIndex(NonNull::from(value).cast(), size_of_val(value), index);
    }
}

/// Bir vertex/fragment çiftinden render pipeline; **ön çarpımsız alfa blend**.
///
/// Adlar **ayrı ayrı** parametre, `{name}_vertex` diye türetilmiyor: hata
/// varyantı eksik sembolün kendisini taşıyor ve türetilmiş bir ad "ikisinden
/// biri" demekle yetinirdi — metallib'de hangisinin olmadığını okuyanın
/// aramasına bırakırdı.
///
/// Blend **parametre değil**: altı pipeline da onu istiyor ve sebepleri ayrı —
/// `cell` alfayı atlasın kapsamasından üretiyor, `cell_bg`'de imlecin
/// belirmesi ([`crate::motion`]) dikdörtgeni saydamlaştırıyor, `glyph_fx`'te
/// efektin kendisi saydamlık, `selection`'da yuvarlak köşenin yumuşatması. Bir `enum`
/// parametresi 008 phase-5'e kadar iki değer taşıyordu; tek değere düşünce
/// hem kendisi hem tek `if`'i kalktı.
fn pipeline(
    device: &ProtocolObject<dyn MTLDevice>,
    library: &ProtocolObject<dyn MTLLibrary>,
    vs_name: &'static str,
    fs_name: &'static str,
) -> Result<Retained<ProtocolObject<dyn MTLRenderPipelineState>>, GpuError> {
    let vs = library
        .newFunctionWithName(&NSString::from_str(vs_name))
        .ok_or(GpuError::MissingFunction(vs_name))?;
    let fs = library
        .newFunctionWithName(&NSString::from_str(fs_name))
        .ok_or(GpuError::MissingFunction(fs_name))?;

    let desc = MTLRenderPipelineDescriptor::new();
    desc.setVertexFunction(Some(&vs));
    desc.setFragmentFunction(Some(&fs));
    // SAFETY: indeks 0 her render pipeline'da vardır.
    let att = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
    att.setPixelFormat(Renderer::PIXEL_FORMAT);
    att.setBlendingEnabled(true);
    // **Altı pipeline da ön çarpımsız fragment veriyor**, emoji dahil:
    // CoreGraphics renkli glyph'i ön çarpımlı yazıyor ama `raster::draw_color`
    // onu yüklemeden önce geri alıyor (gerekçe `raster::unpremultiply`'ın
    // doc'unda: ön çarpım sRGB-kodlanmış uzayda yapıldığı için doku başına
    // kanal çözümü onu karartıyordu). Yani 008 phase-5'in bu parametreyi
    // atma kararı **geri alınmadı** ve blend hâlâ parametre değil.
    att.setSourceRGBBlendFactor(MTLBlendFactor::SourceAlpha);
    att.setDestinationRGBBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    // Alfa kanalının **kaynak çarpanı `One`**, `SourceAlpha` değil.
    // Fragment ön çarpımsız veriyor (`rgb`, `a = kapsama`): renk için
    // doğru olan `sa·src + (1-sa)·dst`, ama aynı çarpanı alfaya uygulamak
    // `sa² + (1-sa)·dst_a` eder ve yarı kapsamalı bir kenarda hedefin
    // alfası 1'den 0.75'e düşer. `CAMetalLayer` `opaque` bayrağını
    // taşımıyor, yani compositor o deliği onurlandırır ve harflerin
    // kenarından pencerenin arkası sızar. `One` ile `sa + (1-sa)·dst_a`,
    // dst_a = 1 iken 1 kalır.
    att.setSourceAlphaBlendFactor(MTLBlendFactor::One);
    att.setDestinationAlphaBlendFactor(MTLBlendFactor::OneMinusSourceAlpha);
    device
        .newRenderPipelineStateWithDescriptor_error(&desc)
        .map_err(GpuError::Pipeline)
}

/// Dokunun boyunda, `origin_y`'den başlayan viewport — dock'un ve (sıfırla)
/// yazım efektlerinin ([`Renderer::encode_fx`]). Boy dokunun boyu: NDC
/// ölçeği `viewport_px` uniform'uyla aynı kalmalı (`encode_dock`'un doc'u).
/// Dokunun `top_px`'ten dibe kadarki şeridi, makas olarak (032, büyüyen
/// bant). Makas dokunun içinde kalmak zorunda (Metal'in doğrulaması), yani
/// sınırlar dokunun boyuna kırpılıyor ve en az bir satır bırakıyor; `0.0`
/// bütün doku.
fn scissor_below(top_px: f32, viewport_px: [f32; 2]) -> MTLScissorRect {
    let width = viewport_px[0].max(0.0) as usize;
    let height = viewport_px[1].max(0.0) as usize;
    let y = (top_px.max(0.0).round() as usize).min(height.saturating_sub(1));
    MTLScissorRect {
        x: 0,
        y,
        width,
        height: height - y,
    }
}

fn viewport_at(origin_y: f32, viewport_px: [f32; 2]) -> MTLViewport {
    MTLViewport {
        originX: 0.0,
        originY: f64::from(origin_y),
        width: f64::from(viewport_px[0]),
        height: f64::from(viewport_px[1]),
        znear: 0.0,
        zfar: 1.0,
    }
}

impl AtlasTexture {
    /// Dokuyu (gerekirse) kurar, eksik yuvaları yükler ve `instances`'ı bu
    /// karenin glyph'leri **ve kurallarıyla** doldurur — hepsi **tek** ödünç
    /// altında ve tek listede, çünkü ikisi de aynı `cell` pipeline'ının
    /// çizdiği hücre boyunda birer kapsama maskesi.
    ///
    /// **Bilinen sınır — `replaceRegion` uçuşta okunan dokuya yazıyor.**
    /// Izgara değişimi güvenli (`sync_atlas` dokuyu düşürür, burada yenisi
    /// doğar, eskisini komut tamponu kendi referansıyla yaşatır). Güvenli
    /// **olmayan**, aynı doku canlıyken yeni bir yuvanın yazılması: yuvaların
    /// ayrık olması yetmiyor, çünkü doku düzeni doğrusal değil ve bir yuvaya
    /// yazmak komşu yuvaların sütunlarını da taşıyan karoların
    /// oku-değiştir-yaz'ı olabiliyor. Belirti nadir ve tek karelik: hiç
    /// görülmemiş bir karakter, önceki kare hâlâ koşarken yüklenirse o karede
    /// bozuk çizilebilir. Doğru biçimi bir staging tamponu + aynı komut
    /// tamponunda blit encoder'ı (Metal'in kendi hazard takibi sıralar);
    /// bu sette yapılmadı, `## Uygulama Notları`'na geçti.
    fn prepare(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        glyphs: &[GlyphCell],
        clusters: &Clusters,
        rules: &[RuleCell],
    ) -> Result<(), GpuError> {
        self.ensure_texture(device)?;
        // audit: `ensure_texture` `Ok` döndüyse dokuyu kurmuştur.
        let texture = self.texture.as_ref().expect("doku hemen üstte kuruldu");
        let metrics = self.atlas.metrics();
        let (tw, th) = self.atlas.texture_px();

        self.instances.clear();
        self.color_instances.clear();
        // `clear` kapasiteyi koruyor, yani durağan hâlde ayırma yok; `reserve`
        // yalnız kapasitenin **ilk kez** aşıldığı kareyi düzleştiriyor (bir
        // blok metnin altı çizilince glyph + kural toplamı sıçrar) — iki
        // döngünün ortasında birden çok kez büyüyüp kopyalamak yerine bir kez.
        self.instances.reserve(glyphs.len() + rules.len());
        // Doku boyutu döngü değişmezi: tersi bir kez alınıp çarpılıyor, yoksa
        // sprite başına iki f32 bölmesi ödenirdi.
        let inv = (1.0 / f32::from(tw), 1.0 / f32::from(th));
        // **Tek liste, tek draw call: önce glyph'ler, sonra kurallar.** Sıra
        // bilerek — üstü çizili harfin ÜSTÜNDEN geçmeli. İmleç sırası bedava
        // geliyor: glyph geçişi zaten arka planlardan ve imleçten sonra
        // kodlanıyor (`encode_pass`), yani kural da imlecin üstüne düşüyor.
        for glyph in glyphs {
            // **Yelpazeleme [`fan`]'da**, burada ve yazım efektlerinde
            // ([`AtlasTexture::prepare_fx`]) aynı gövde.
            //
            // **Liste düzlemden seçiliyor.** Emoji başka bir pipeline, başka
            // bir doku ve başka bir blend istiyor; aynı listeye karışsalardı
            // tek draw call iki fragment'i birden isteyemezdi.
            for part in fan(
                &mut self.atlas,
                texture,
                &mut ColorPlane {
                    slot: &mut self.color_texture,
                    device,
                    edge: (tw, th),
                },
                metrics,
                inv,
                glyph,
                clusters,
            )
            .into_iter()
            .flatten()
            {
                let list = match part.plane {
                    Plane::Mask => &mut self.instances,
                    Plane::Color => &mut self.color_instances,
                };
                list.push(GlyphInstance {
                    pos: part.pos,
                    uv0: part.uv0,
                    rgba: glyph.rgba,
                });
            }
        }
        for rule in rules {
            // Kurallar **her zaman** `Face::Regular`: kalın metnin altındaki
            // çizgi kalın değildir. `Atlas::slot` bunu ayrıca normalize ediyor;
            // burada da doğru yüzü sormak o normalizasyonu bir savunma
            // katmanı olarak bırakıyor, tek dayanak yapmıyor.
            let (uv0, _) = slot_uv(
                &mut self.atlas,
                texture,
                &mut ColorPlane {
                    slot: &mut self.color_texture,
                    device,
                    edge: (tw, th),
                },
                metrics,
                inv,
                SlotAsk {
                    sprite: Sprite::Rule(rule.kind),
                    face: Face::Regular,
                    // Kurallar **her zaman** gösterim ölçüsünde: bağlam
                    // satırında kural yok ve `Atlas::slot` bunu ayrıca
                    // normalize ediyor.
                    size: SizeClass::Normal,
                    // Kural çizgisi tanımı gereği tek hücre; `Atlas::slot`
                    // bunu da normalize ediyor ve burada doğru yarıyı sormak
                    // o normalizasyonu savunma katmanı olarak bırakıyor.
                    want: Half::Whole,
                },
            );
            self.instances.push(GlyphInstance {
                pos: rule.pos,
                uv0,
                rgba: rule.rgba,
            });
        }
        Ok(())
    }

    /// Maske dokusunu (gerekirse) kurar ve rezident tofu'yu bir kez yazar —
    /// [`AtlasTexture::prepare`] ile [`AtlasTexture::prepare_fx`]'in ortak
    /// başı.
    fn ensure_texture(&mut self, device: &ProtocolObject<dyn MTLDevice>) -> Result<(), GpuError> {
        if self.texture.is_some() {
            return Ok(());
        }
        let (tw, th) = self.atlas.texture_px();
        let texture = new_atlas_texture(device, tw, th)?;
        // Rezident tofu bir kez yazılır ve bir daha dokunulmaz:
        // `Atlas::slot` tofu'ya düştüğünde bitmap **vermiyor**, çünkü veri
        // zaten burada.
        upload_slot(
            &texture,
            self.atlas.slot_origin(TOFU),
            self.atlas.metrics(),
            self.atlas.tofu_bitmap(),
            Plane::Mask,
        );
        self.texture = Some(texture);
        Ok(())
    }

    /// Yazım efektlerinin instance'larını kurar (030): yuva çözümü ve
    /// yelpazeleme [`AtlasTexture::prepare`]'inkiyle **aynı** gövdeden
    /// ([`fan`]), ikinci bir kopya yok.
    ///
    /// Geniş glyph iki instance veriyor ve her biri **hangi yarı** olduğunu
    /// taşıyor: shader dönüşümün merkezini iki hücrelik kutudan alıyor, yoksa
    /// `recede` bir emojiyi ortasından ikiye ayırırdı. Düzlem de instance'ta:
    /// iki doku birden bağlı, liste tek.
    fn prepare_fx(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        cells: &[FxCell],
        clusters: &Clusters,
    ) -> Result<(), GpuError> {
        self.ensure_texture(device)?;
        // audit: `ensure_texture` `Ok` döndüyse dokuyu kurmuştur.
        let texture = self.texture.as_ref().expect("doku hemen üstte kuruldu");
        let metrics = self.atlas.metrics();
        let (tw, th) = self.atlas.texture_px();
        let inv = (1.0 / f32::from(tw), 1.0 / f32::from(th));
        self.fx_instances.clear();
        for cell in cells {
            for part in fan(
                &mut self.atlas,
                texture,
                &mut ColorPlane {
                    slot: &mut self.color_texture,
                    device,
                    edge: (tw, th),
                },
                metrics,
                inv,
                &cell.glyph,
                clusters,
            )
            .into_iter()
            .flatten()
            {
                let plane = match part.plane {
                    Plane::Mask => 0,
                    Plane::Color => 1,
                };
                let half = match part.half {
                    Half::Whole => 0,
                    Half::Left => 1,
                    Half::Right => 2,
                };
                self.fx_instances.push(FxInstance {
                    pos: part.pos,
                    uv0: part.uv0,
                    rgba: cell.glyph.rgba,
                    // Paket `shaders/glyph_fx.metal`'in çözdüğüyle aynı:
                    // `kimlik | düzlem << 5 | yarı << 6`, küçük bir tam sayı
                    // ve `f32`'de birebir (bkz. [`FxInstance`]).
                    fx: [
                        cell.t,
                        (cell.effect | plane << 5 | half << 6) as f32,
                        cell.seed,
                        0.0,
                    ],
                });
            }
        }
        Ok(())
    }
}

/// Bir glyph'in dokudaki yeri: dörtlünün konumu, yuvanın uv'si, düzlemi ve
/// geniş glyph'in hangi yarısı olduğu.
struct Part {
    pos: [f32; 2],
    uv0: [f32; 2],
    plane: Plane,
    half: Half,
}

/// Bir glyph'in yuvası — ya da geniş glyph'in iki yarısı: **yelpazelemenin tek
/// gövdesi**.
///
/// **Burada, `Frame::push`'ta değil.** Gerekçe ödünç: "bir yuva mı iki mi"
/// kararını mürekkep kapısı veriyor, yani `Atlas::slot` — ve `push` atlası
/// ödünç alamıyor (`GlyphCell`'in uv'siz olmasının yazılı sebebi: sink'te
/// çözüm ödüncü `draw` boyunca canlı tutar ve ilk glyph'li karede
/// `BorrowMutError` verir). Burada atlas **zaten** ödünç alınmış ve `metrics`
/// elde.
///
/// Dört yüzey bedavaya geliyor: [`AtlasTexture::prepare`] kare başına dört kez
/// koşuyor (şeritler, ızgara, doldurma bandı, dock) ve yazım efektleri
/// ([`AtlasTexture::prepare_fx`]) de buradan geçiyor. 017'nin dersi — bir
/// yüzey ızgaradan türeyen her şeyi ayrıca kazanmak zorunda — tek yerde
/// ödeniyor.
fn fan(
    atlas: &mut Atlas,
    texture: &ProtocolObject<dyn MTLTexture>,
    color: &mut ColorPlane<'_>,
    metrics: Metrics,
    inv: (f32, f32),
    glyph: &GlyphCell,
    clusters: &Clusters,
) -> [Option<Part>; 2] {
    let want = if glyph.wide { Half::Left } else { Half::Whole };
    // **Küme burada atlasa iniyor** (035 Karar 4B): interning atlasın ödüncü
    // gerektiriyor ve sink onu alamıyor (023). İki yarı aynı sprite'tan.
    // Taban karaktere düşüş ikinci kez yazılmıyor: tabloda bulunamayan
    // kimlik `Char`, şekillenmeyen ya da kapıdan dönen küme ise
    // `Atlas::slot`'un kendi cevabı (taban karakter, R1.1).
    let sprite = glyph
        .cluster
        .and_then(|id| clusters.get(id))
        .map_or(Sprite::Char(glyph.ch), |text| atlas.intern(text));
    let (uv0, placed) = slot_uv(
        atlas,
        texture,
        color,
        metrics,
        inv,
        SlotAsk {
            sprite,
            face: glyph.face,
            size: glyph.size,
            want,
        },
    );
    let first = Part {
        pos: glyph.pos,
        uv0,
        plane: placed.plane,
        half: placed.half,
    };
    // İkinci dörtlü **yalnız kapı iki hücre dediyse**. Geniş ilan edilmiş ama
    // mürekkebi bir hücreye sığan karakter (`☕`, fullwidth `！`) `Whole`
    // dönüyor ve burası hiç koşmuyor — yoksa sağına boş bir dörtlü düşerdi.
    // Izgara ona zaten iki sütun ayırdığı için komşu hücre spacer ve glyph
    // vermiyor.
    if placed.half != Half::Left {
        return [Some(first), None];
    }
    let (uv1, right) = slot_uv(
        atlas,
        texture,
        color,
        metrics,
        inv,
        SlotAsk {
            sprite,
            face: glyph.face,
            size: glyph.size,
            want: Half::Right,
        },
    );
    [
        Some(first),
        Some(Part {
            pos: [glyph.pos[0] + f32::from(metrics.cell_px.0), glyph.pos[1]],
            uv0: uv1,
            plane: right.plane,
            half: right.half,
        }),
    ]
}

/// Renk dokusunun tembel kurucusu — [`slot_uv`]'nin dördüncü argümanı.
///
/// Tip, üç şeyi tek argümanda taşıyor (doku yuvası, device ve kenar) çünkü
/// üçü tek bir işi yapıyor: "gerektiğinde renk dokusunu kur ve ver".
/// Ayrı argümanlar olsaydı `slot_uv` yine `clippy`'nin sınırını aşardı.
struct ColorPlane<'a> {
    slot: &'a mut Option<Retained<ProtocolObject<dyn MTLTexture>>>,
    device: &'a ProtocolObject<dyn MTLDevice>,
    edge: (u16, u16),
}

impl ColorPlane<'_> {
    /// Dokuyu (gerekirse kurup) verir; kurulum başarısızsa `None`.
    ///
    /// Hata **yutuluyor** ve gerekçesi çağrı yeri: `slot_uv` kare yolunda ve
    /// `Result` döndürmüyor. 4 MiB ayıramayan bir makinede zaten daha büyük
    /// bir sorun var.
    ///
    /// **Bilinen sınır ve tam şekli** (set kapısı, `/code-review`): yuva
    /// `Atlas::slot`'ta **zaten** ayrılmış ve önbelleğe girmiş oluyor, yani
    /// yükleme atlanınca o yuva dokuda yazılmamış kalıyor. Sonraki bir
    /// istekte ayırma **başarılı olursa** doku doğuyor ama o eski yuvalar
    /// hâlâ yazılmamış — ve Metal yeni dokuyu **sıfırlamıyor**, yani sonuç
    /// saydam siyah değil **tanımsız bellek**. Belirti o birkaç emojide
    /// rastgele piksel. Yol bir ayırma hatası gerektiriyor, yani pratikte
    /// görülmedi; doğru çaresi ayırma başarısız olduğunda o isteğin yuvasını
    /// da geri almak ve o, `Atlas::slot`'un dönüşünü değiştirmeyi ister.
    fn get(&mut self) -> Option<&ProtocolObject<dyn MTLTexture>> {
        if self.slot.is_none() {
            *self.slot = new_color_texture(self.device, self.edge.0, self.edge.1).ok();
        }
        self.slot.as_deref()
    }
}

/// Renk düzleminin dokusu: `RGBA8Unorm_sRGB`, aynı yuva ızgarası.
///
/// **Format `_sRGB` olmak zorunda.** Hedef `BGRA8Unorm_sRGB` ve donanım
/// fragment çıktısını lineer sayıyor; düz `RGBA8Unorm` bir dokudan örneklenen
/// emoji **çözülmemiş** sRGB değerleri lineer sanır ve palet açar. Belirti
/// `CLAUDE.md` → "Renk uzayı sınırı geçer" maddesindeki sessiz kusurun
/// aynısı.
///
/// `Shared` depolama ve `ShaderRead` kullanımı maske dokusuyla aynı gerekçe
/// ([`new_atlas_texture`]).
fn new_color_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    width: u16,
    height: u16,
) -> Result<Retained<ProtocolObject<dyn MTLTexture>>, GpuError> {
    // SAFETY: sınıf metodu, argümanlar değer tipleri.
    let desc = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::RGBA8Unorm_sRGB,
            usize::from(width),
            usize::from(height),
            false,
        )
    };
    desc.setUsage(MTLTextureUsage::ShaderRead);
    desc.setStorageMode(MTLStorageMode::Shared);
    device
        .newTextureWithDescriptor(&desc)
        .ok_or(GpuError::NoAtlasTexture)
}

/// Atlasa sorulan yuvanın kimliği — [`Atlas::slot`]'un dört argümanı.
///
/// Dördü tek tipte, çünkü [`slot_uv`]'nin argüman sayısı `clippy`'nin
/// sınırını aşıyordu ve lint'i susturmak yanlış çare olurdu: bu dördü
/// gerçekten **tek bir şeyi** adlandırıyor — atlas anahtarının istek hâli.
struct SlotAsk {
    sprite: Sprite,
    face: Face,
    size: SizeClass,
    /// Çağıranın **istediği** yarı; cevabın yarısı bundan farklı olabilir
    /// (bkz. [`Half::Whole`]).
    want: Half,
}

/// Sprite'ın yuvasını çözer, yuva yeni açıldıysa dokuya yükler ve uv0'ını
/// verir.
///
/// Glyph ve kural döngülerinin **ortak gövdesi**; ikisinin ayrıldığı tek yer
/// sordukları sprite ve yüz. Kopyalansaydı `upload` dalı iki yerde yaşardı ve
/// birinde unutulan bir `upload_slot` "yuva var ama doku boş" demek olurdu —
/// ekranda görünmeyen bir glyph, hiçbir sayacın düşmediği.
///
/// `Upload` köşeyi zaten taşıyor — `bt-atlas` ikisini bilerek aynı dönüşte
/// veriyor. Yeni yuvada onu kullanmak hem sprite başına bir `%` + `/` çiftini
/// düşürüyor hem de aynı olguyu iki ayrı ifadeyle yazmayı önlüyor;
/// `slot_origin` yalnız önbellekli ve tofu yoluna kalıyor. (`upload` atlası
/// ödünç alıyor; `if let` onu tüketince ödünç bitiyor ve atlas yeniden
/// sorulabiliyor.)
fn slot_uv(
    atlas: &mut Atlas,
    texture: &ProtocolObject<dyn MTLTexture>,
    color: &mut ColorPlane<'_>,
    metrics: Metrics,
    inv: (f32, f32),
    ask: SlotAsk,
) -> ([f32; 2], Placed) {
    let (placed, upload) = atlas.slot(ask.sprite, ask.face, ask.size, ask.want);
    let (x, y) = if let Some(upload) = upload {
        // **Doku düzlemden seçiliyor, çağırandan değil.** `bytesPerRow` de
        // oradan: `upload_slot` satır adımını `Plane`'e göre türetiyor ve
        // ayrışırsa Metal kısa tamponun ötesini okur — belirti sessiz.
        let target = match upload.plane {
            Plane::Mask => Some(texture),
            // **Renk dokusu tam burada, ilk renkli yuvayla doğuyor.** Tembel
            // olmasının bedeli yok ama kazancı var: emoji görmeyen bir oturum
            // 4 MiB'ı hiç ödemiyor (kenar maskeninkiyle aynı, piksel başına
            // dört bayt). Kurulumun **yükleme anında** olması zorunlu: bir
            // kare önce kurulsaydı "hangi karakter renkli" sorusunu cascade'i
            // ikinci kez yürüyerek sormak gerekirdi, bir kare sonra
            // kurulsaydı bu yuva yazılmadan önbelleğe girer ve emoji
            // **kalıcı olarak** görünmez kalırdı.
            Plane::Color => color.get(),
        };
        // Renk dokusu **tembel** ve `prepare` onu emoji görünce kuruyor; yine
        // de `Option`: doku ayırması başarısız olabiliyor ve o hâlde emoji
        // **çizilmiyor**, panik yok. Yükleme atlanınca yuva dokuda yazılmamış
        // kalır ve o karede bir şey görünmez — bir sonraki karede doku kurulup
        // yuva yeniden yüklenmiyor (anahtar önbellekte), yani bu bilinen bir
        // sınır ve doku ayırmasının başarısız olduğu makinede zaten daha
        // büyük bir sorun var.
        if let Some(target) = target {
            upload_slot(target, upload.origin, metrics, upload.bytes, upload.plane);
            // **Çiftin sağ yarısı aynı dönüşte yükleniyor.** `bt-atlas` iki yuvayı
            // atomik ayırıyor ve ikisinin baytlarını birlikte veriyor; burada
            // atlanırsa sağ yuva dokuda **yazılmamış** kalır ve o karakterin sağ
            // yarısı komşu yuvanın bitmap'iyle çizilir — sessiz bir bozulma.
            if let Some(right) = upload.right {
                upload_slot(target, right, metrics, upload.right_bytes, upload.plane);
            }
        }
        upload.origin
    } else {
        atlas.slot_origin(placed.slot)
    };
    ([f32::from(x) * inv.0, f32::from(y) * inv.1], placed)
}

/// Atlas dokusu: tek kanal kapsama, yalnız shader okur.
///
/// `Shared` depolama, `Private` değil: yükleme yolu `replaceRegion`, yani
/// CPU doğrudan yazıyor ve bir blit encoder'ı ile staging tamponu bu setin
/// kazancını taşımaz. Apple silicon'da bellek zaten tek; ayrık GPU'lu
/// makinede bedeli örneklemede bir kopya olur ve karar `/measure` sonrası
/// yeniden bakılacak bir yer.
fn new_atlas_texture(
    device: &ProtocolObject<dyn MTLDevice>,
    width: u16,
    height: u16,
) -> Result<Retained<ProtocolObject<dyn MTLTexture>>, GpuError> {
    // SAFETY: sınıf metodu, argümanlar değer tipleri.
    let desc = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            MTLPixelFormat::R8Unorm,
            usize::from(width),
            usize::from(height),
            false,
        )
    };
    desc.setUsage(MTLTextureUsage::ShaderRead);
    desc.setStorageMode(MTLStorageMode::Shared);
    device
        .newTextureWithDescriptor(&desc)
        .ok_or(GpuError::NoAtlasTexture)
}

/// Tam bir yuvayı dokuya yazar.
fn upload_slot(
    texture: &ProtocolObject<dyn MTLTexture>,
    origin: (u16, u16),
    metrics: Metrics,
    bytes: &[u8],
    plane: Plane,
) {
    let (w, h) = (
        usize::from(metrics.cell_px.0),
        usize::from(metrics.cell_px.1),
    );
    // `debug_assert` değil: bu satır aşağıdaki `unsafe` bloğun ön koşulu.
    // Metal'e `width`/`height` metrikten, işaretçi dilimden gidiyor; ikisi
    // ayrışırsa Metal kısa tamponun ötesini okur ve belirti sessizdir.
    // Beklenen uzunluk `slot_bytes()`'tan geliyor: yuva geometrisinin tek
    // sahibi `bt-atlas` ve buraya `w * h` yazmak dördüncü bir kopyası olurdu —
    // yuvaya bir gün satır dolgusu girerse o taraf düzelir, bu satır sessizce
    // eski kalırdı.
    // Beklenen uzunluk **düzlemden**: maske `w*h`, renk `4*w*h`. İkisini tek
    // sayıya bağlamak yanlış düzlemin tamponunu sessizce geçirirdi.
    let (expected, row_bytes) = match plane {
        Plane::Mask => (metrics.slot_bytes(), w),
        Plane::Color => (metrics.slot_bytes_rgba(), w * 4),
    };
    assert_eq!(bytes.len(), expected, "tam bir yuva olmalı ({plane:?})");
    let region = MTLRegion {
        origin: MTLOrigin {
            x: usize::from(origin.0),
            y: usize::from(origin.1),
            z: 0,
        },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    // SAFETY: `bytes` w*h bayt ve çağrı boyunca canlı; bölge dokunun içinde
    // (`slot_origin` ızgara dışını `TOFU`'ya kırpıyor ve `texture_px` tam
    // ızgara kadar ayrılıyor). Satır adımı tam hücre genişliği: `bt-atlas`
    // yuvayı dolgu bırakmadan çiziyor.
    unsafe {
        texture.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            region,
            0,
            NonNull::from(bytes).cast::<c_void>(),
            row_bytes,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Instant;

    use bt_core::{
        Block, CaretShape, Cell, Cursor, SearchRun, SelectionRun, Theme, UnderlineStyle,
    };

    use super::*;
    use crate::glyph_fx::{Effect, Fx, Kind};
    use crate::stats::Stats;
    use bt_core::CaretStyle;
    use bt_core::{Erase, Keypress};

    /// Gömülü temanın zemini ve vurgusu: üretimde clear ve imleç rengi bu
    /// iki rolden geliyor (`link.rs`), sınamalar da aynı kaynaktan.
    const BACKGROUND: LinearRgba = Theme::BATERI.background_linear();
    const ACCENT: LinearRgba = Theme::BATERI.accent_linear();

    /// sRGB lineerleştirmesinin **hücre yolundaki** tanığı: bir ara ton.
    ///
    /// Bilerek temadan DEĞİL. Bekçinin duyarlılığı bir zevk değerine bağlı
    /// kalamaz: `0.0` ve `1.0` sRGB transfer fonksiyonunun sabit noktaları,
    /// yani zemin saf siyaha çekildiği gün (öyle oldu) bu iddia lineerleştirme
    /// olsa da olmasa da geçerdi ve tek bekçi sessizce körleşirdi. Değer eski
    /// zeminin ta kendisi — kaydı `CLAUDE.md`'de aynı sayıyla duruyor.
    const MIDTONE_SRGB: u32 = 0x1a1c21;
    const MIDTONE: LinearRgba = {
        let (r, g, b) = (
            (MIDTONE_SRGB >> 16) as u8,
            (MIDTONE_SRGB >> 8) as u8,
            MIDTONE_SRGB as u8,
        );
        LinearRgba::from_srgb(r, g, b)
    };

    /// Sol payı **sıfır** olan ızgara ölçüsü.
    ///
    /// Offscreen sınamaların örnekleme noktası `cell_rows`'ta `col * cw + x`,
    /// yani orijini sıfır varsayıyor: sıfır olmayan bir pay o noktaları
    /// kaydırır ve sınamalar hücre yerine clear rengini okurdu. Sıfır burada
    /// bir kolaylık değil **doğru soru**: bu sınamaların konusu payın
    /// geometrisi değil, GPU'nun hangi rengi hangi hücreye boyadığı. Payın
    /// orijine eklendiğini `frame.rs` tarafında `pos` sınamaları tutuyor.
    fn grid(width: u16, height: u16) -> CellMetrics {
        CellMetrics::new(width, height, width, 0, 1).expect("sıfır olmayan hücre")
    }

    /// Payı **sıfır olmayan** ızgara: halenin payı sol paydan türüyor
    /// ([`Frame::glow_px`]), yani paysız bir ızgarada hale hiç doğmuyor ve
    /// onu sınayan hiçbir şey göremez.
    fn grid_with_gutter(width: u16, height: u16, gutter: u16) -> CellMetrics {
        CellMetrics::new(width, height, width, gutter, 1).expect("sıfır olmayan hücre")
    }

    /// Yalnız arka planı olan hücre; `ch: None` glyph üretmez.
    fn bg_cell(col: u16, row: u16, bg: LinearRgba) -> Cell {
        Cell {
            col,
            row,
            fg: BACKGROUND,
            bg: Some(bg),
            ..Default::default()
        }
    }

    #[test]
    fn metallib_is_embedded_and_valid() {
        // Metal kütüphanesi dosyası "MTLB" sihirli sayısıyla başlar.
        assert_eq!(&METALLIB[..4], b"MTLB");
    }

    #[test]
    fn two_scales_give_two_metrics() {
        // Ölçek önbellek anahtarının parçası (`plan.md` → R1.2) ve metrik o
        // anahtarın gözle görülür ucu: @2x'te hücre büyümezse atlas ölçeği
        // yutuyor demektir ve glyph'ler hatasız bulanıklaşır.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let one = r.cell_metrics(1.0);
        let two = r.cell_metrics(2.0);
        assert!(
            two.cell_px().0 > one.cell_px().0 && two.cell_px().1 > one.cell_px().1,
            "@2x hücre @1x'ten büyük olmalı: {one:?} → {two:?}"
        );
        // Geri dönüş de çalışmalı: `ensure` tek yönlü bir kapı değil. Yoksa
        // harici ekran çıkarıldığında metrik @2x'te takılı kalır ve pencere
        // yarı yarıya az hücre gösterirdi.
        assert_eq!(r.cell_metrics(1.0), one, "aynı ölçek aynı metriği verir");
    }

    #[test]
    fn set_font_changes_the_metrics_on_the_next_ask() {
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let base = r.cell_metrics(1.0);
        // Açılış değeri ayar modelinin varsayılanı: süreli koşunun hücresi
        // dosyasız kullanıcınınkiyle aynı.
        assert!(
            !r.set_font(&FontOptions::default()),
            "varsayılan zaten istenmiş olmalı"
        );
        let large = FontOptions {
            size: 26.0,
            ..FontOptions::default()
        };
        assert!(r.set_font(&large), "punto değişti");
        assert!(!r.set_font(&large), "aynı istek değişim değil");
        let bigger = r.cell_metrics(1.0);
        assert!(
            bigger.cell_px().0 > base.cell_px().0 && bigger.cell_px().1 > base.cell_px().1,
            "26pt hücre 13pt'den büyük olmalı: {base:?} → {bigger:?}"
        );
        assert!(r.set_font(&FontOptions::default()));
        assert_eq!(r.cell_metrics(1.0), base, "varsayılana dönüş");
    }

    #[test]
    fn missing_family_becomes_a_notice_after_the_atlas_opens() {
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        assert_eq!(r.font_notice(), None, "atlas yokken söylenecek şey yok");
        r.cell_metrics(1.0);
        assert_eq!(r.font_notice(), None, "zincir sessiz");
        assert!(r.set_font(&FontOptions {
            family: Some("Bu Aile Yok 12345".to_owned()),
            ..FontOptions::default()
        }));
        r.cell_metrics(1.0);
        let Some(FontNotice::FamilyNotFound { requested, using }) = r.font_notice() else {
            panic!("bulunamadı bildirimi beklendi: {:?}", r.font_notice());
        };
        assert_eq!(requested, "Bu Aile Yok 12345");
        assert!(
            ["SF Mono", "Menlo"].contains(&using.as_str()),
            "zincirin ailesi: {using}"
        );
        assert!(r.set_font(&FontOptions {
            family: Some("Helvetica".to_owned()),
            ..FontOptions::default()
        }));
        r.cell_metrics(2.0);
        let proportional = Some(FontNotice::NotMonospaced {
            family: "Helvetica".to_owned(),
        });
        assert_eq!(r.font_notice(), proportional);
        // Ekran değişimi atlası yeniden kuruyor ama bildirim aynı: font
        // yuvası pencere ekrandan ekrana taşınırken oynamamalı.
        r.cell_metrics(1.0);
        assert_eq!(r.font_notice(), proportional, "ölçek bildirimi oynattı");
    }

    #[test]
    fn zero_component_metrics_cannot_be_built() {
        // Tipin taşıdığı tek garanti bu. Düşerse `bt-shell`'in bölmesi
        // `inf` verir, `inf as u16` 65535 eder ve `Session::resize`'ın sıfır
        // kapısına takılmadan 65535×65535'lik bir `TIOCSWINSZ` geçer.
        assert!(CellMetrics::new(0, 18, 0, 8, 1).is_none());
        assert!(CellMetrics::new(9, 0, 9, 8, 1).is_none());
        // Bağlam genişliği de **bölen** (`crate::frame::context_cols`), yani
        // aynı kapıdan geçiyor: sıfır geçseydi ızgaranınki yakalanırken dock'un
        // bağlam satırı sessizce sıfıra bölerdi.
        assert!(CellMetrics::new(9, 18, 0, 8, 1).is_none());
        let metrics = CellMetrics::new(9, 18, 7, 8, 1).expect("ölçü");
        assert_eq!(metrics.cell_px(), (9, 18));
        assert_eq!(metrics.context_cell_px(), 7);
        assert_eq!(metrics.gutter_px(), 8);
        // Pay **eliyor değil taşınıyor**: bölen değil çıkan, ve sıfır pay
        // "ızgara kenardan başlıyor" demek. Sıfırı burada da elemek, payı
        // konu etmeyen her sınamayı uydurma bir değer yazmaya zorlardı.
        assert_eq!(
            CellMetrics::new(9, 18, 9, 0, 1)
                .expect("sıfır pay meşru")
                .gutter_px(),
            0
        );
    }

    #[test]
    fn cell_metrics_are_never_zero() {
        // `bt-shell` bu iki sayıyı **bölen** olarak kullanıyor. Garantiyi
        // `bt-atlas` veriyor (`font::round_up` 1'e kırpar) ve `CellMetrics`'in
        // private alanı onu sınırın bu tarafında yapısal kılıyor; bu sınama
        // kaynaktaki kırpmanın hâlâ yerinde olduğunu söylüyor.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        for scale in [1.0, 2.0, 3.0] {
            let (w, h) = r.cell_metrics(scale).cell_px();
            assert!(w >= 1 && h >= 1, "ölçek {scale}: {w}×{h}");
        }
    }

    #[test]
    fn cell_bg_pipeline_builds() {
        // Device yoksa `ignored` değil açık hata: bu makinede Metal var,
        // yokluğu bir kusurdur. Pipeline'ın kurulması shader'ın derlendiğini
        // ve fonksiyon adlarının metallib'de bulunduğunu kanıtlar.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        assert_eq!(r.frames(), 0);
    }

    /// Sınama için küçük bir offscreen render hedefi; `Shared` depolama
    /// `getBytes` ile CPU'dan okumaya izin verir.
    fn target_texture(r: &Renderer, edge: usize) -> Retained<ProtocolObject<dyn MTLTexture>> {
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                // Formatı `Renderer`'dan: pipeline hangi formata derlendiyse
                // hedef de o. Elle yazılsaydı sRGB geçişi burada assert'le
                // değil Metal doğrulama istisnasıyla düşerdi — ve istisna
                // sınamanın ne aradığını hiç söylemez.
                Renderer::PIXEL_FORMAT,
                edge,
                edge,
                false,
            )
        };
        desc.setUsage(MTLTextureUsage::RenderTarget);
        desc.setStorageMode(MTLStorageMode::Shared);
        r.device
            .newTextureWithDescriptor(&desc)
            .expect("offscreen doku")
    }

    /// Dokunun tamamını CPU'ya okur.
    fn read_pixels(texture: &ProtocolObject<dyn MTLTexture>, edge: usize) -> Vec<u8> {
        let mut pixels = vec![0u8; edge * edge * 4];
        // SAFETY: tampon kenar×kenar×4 bayt, bölge dokunun tamamı, satır
        // adımı kenar*4. Doku `StorageModeShared` ve komut tamponu tamamlandı.
        unsafe {
            texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                NonNull::new(pixels.as_mut_ptr()).expect("tampon").cast(),
                edge * 4,
                MTLRegion {
                    origin: MTLOrigin { x: 0, y: 0, z: 0 },
                    size: MTLSize {
                        width: edge,
                        height: edge,
                        depth: 1,
                    },
                },
                0,
            );
        }
        pixels
    }

    /// Bayt sırası B, G, R, A (formatın `_sRGB` eki sırayı değiştirmez).
    fn pixel_at(pixels: &[u8], edge: usize, x: usize, y: usize) -> (u8, u8, u8) {
        let i = (y * edge + x) * 4;
        (pixels[i + 2], pixels[i + 1], pixels[i])
    }

    /// Ön plan olarak kullanılan doygun beyaz.
    ///
    /// Doygun: kapsaması tam olan piksel `(0xff, 0xff, 0xff)` baytını birebir
    /// veriyor, yani "kural ön plan rengiyle çizildi" iddiası eşitlikle
    /// sorulabiliyor. Paletten değil, çünkü sınamaların sorduğu şey renk değil
    /// **rengin nereden geldiği**.
    const WHITE: LinearRgba = LinearRgba::from_srgb(0xff, 0xff, 0xff);

    /// Kareyi offscreen bir dokuya çizer ve pikselleri CPU'ya okur.
    ///
    /// Altı sınamanın ortak gövdesi: doku kurulumu, encode, `commit`, bekleme
    /// ve durum kontrolü. Kopyalansaydı `MTLCommandBufferStatus::Error`
    /// kontrolü birinde unutulur ve o sınama boş bir dokuyu okuyup "kural
    /// çizilmedi" yerine anlamsız bir renk iddiası düşürürdü.
    ///
    /// `edge` ve `clear` parametre kalıyor: ikisi de yük taşıyor — kenar 16 ve
    /// 64 olarak ayrışıyor, clear rengi ise `cell_bg_paints_pixels_on_the_gpu`
    /// için bilerek ötekilerden **farklı** (hücre yolu ile clear yolu ayrık iki
    /// renkle kanıtlanıyor).
    fn render_offscreen(r: &Renderer, edge: usize, clear: LinearRgba, frame: &Frame) -> Vec<u8> {
        let texture = target_texture(r, edge);
        let cmd = r.queue.commandBuffer().expect("komut tamponu");
        r.encode_pass(&cmd, &texture, clear, frame)
            .expect("pass encode edilemedi");
        cmd.commit();
        cmd.waitUntilCompleted();
        assert_ne!(cmd.status(), MTLCommandBufferStatus::Error);
        read_pixels(&texture, edge)
    }

    /// `col` sütunundaki hücrenin pikselleri, **satır satır** (üstten alta).
    ///
    /// Satır yapısı korunuyor çünkü kural sınamalarının sorduğu şey tam olarak
    /// bir satırın x boyunca tekdüze olup olmadığı; düzleştirilmiş bir liste
    /// o soruyu soramaz. Düz liste isteyen `.concat()` diyor.
    fn cell_rows(
        pixels: &[u8],
        edge: usize,
        cell_px: (u16, u16),
        col: usize,
    ) -> Vec<Vec<(u8, u8, u8)>> {
        let (cw, ch) = (usize::from(cell_px.0), usize::from(cell_px.1));
        (0..ch)
            .map(|y| {
                (0..cw)
                    .map(|x| pixel_at(pixels, edge, col * cw + x, y))
                    .collect()
            })
            .collect()
    }

    /// Bir pikselin üç kanalının toplamı — "boyandı mı" sorusunun renk
    /// tablosu gerektirmeyen hâli.
    ///
    /// Ortak yardımcı, çünkü caret sınamalarının hepsi aynı soruyu soruyor ve
    /// her biri kendi kopyasını taşıyordu (`/code-review`): bir düzeltme
    /// kopyaların birinde unutulabilirdi.
    fn brightness(pixels: &[u8], edge: usize, x: usize, y: usize) -> u32 {
        let (r8, g8, b8) = pixel_at(pixels, edge, x, y);
        u32::from(r8) + u32::from(g8) + u32::from(b8)
    }

    /// Hücrenin **orta bandı**: üstten ve alttan yarıçap kadar çekilmiş,
    /// genişliği tam.
    ///
    /// Caret'in köşesi yuvarlandığından (015 phase-2) köşe pikselleri artık
    /// bloğun rengi değil; oradan geçen bir eşitlik iddiası **yuvarlaklığı**
    /// sınar, dolguyu değil. Çekme iddiayı zayıflatmıyor **ayırıyor**: bantta
    /// eşitlik hâlâ bit bit, köşenin kendi bekçisi ayrı
    /// ([`the_caret_corner_is_rounded`]).
    ///
    /// **Yalnız satırlar çekiliyor, sütunlar değil:** yuvarlaklık köşelerde
    /// ve `radius` ile `ch - radius` arasındaki her satırda şekil hücrenin
    /// **tam genişliğini** kaplıyor. Sütunları da çekmek dar hücrede bandı
    /// büsbütün yutardı (yarıçap hücre yüksekliğinden türüyor ve dar bir
    /// hücrede genişliğin yarısına yaklaşabiliyor).
    fn cell_body(
        pixels: &[u8],
        edge: usize,
        cell_px: (u16, u16),
        col: usize,
        inset: usize,
    ) -> Vec<(u8, u8, u8)> {
        let (cw, ch) = (usize::from(cell_px.0), usize::from(cell_px.1));
        assert!(inset * 2 < ch, "içeri çekme hücreyi yutuyor");
        (inset..ch - inset)
            .flat_map(|y| (0..cw).map(move |x| (x, y)))
            .map(|(x, y)| pixel_at(pixels, edge, col * cw + x, y))
            .collect()
    }

    /// Caret'in köşe yarıçapı bu hücre ölçüsünde kaç piksel — sınamaların
    /// çekme payı. **Üretimin kendi fonksiyonundan** okuyor, kopyasından
    /// değil: formül üç yerde yazılıydı ve biri değişince bekçi sessizce
    /// gevşerdi (`/code-review`).
    fn caret_radius_px(cell_px: (u16, u16), ratio: f32) -> usize {
        crate::frame::caret_radius_px((f32::from(cell_px.0), f32::from(cell_px.1)), ratio).ceil()
            as usize
    }

    /// Offscreen sınamaların ortak kurulumu: hücre ölçüsü + sığma kontrolü.
    ///
    /// Ölçek açıkça söyleniyor (`cell_metrics(1.0)`): atlasın anahtarı
    /// pencereden gelir, sınamanın penceresi yok ve söylenmezse kare
    /// `GpuError::NoAtlas` ile düşer. Sığma kontrolü ölü değil: büyük
    /// varsayılan puntolu bir makinede hücre dokuyu aşar ve `cell_rows`
    /// dokunun dışını okurdu.
    fn fitting_cell_px(r: &Renderer, edge: usize, cols: usize) -> (u16, u16) {
        let (cw, ch) = r.cell_metrics(1.0).cell_px();
        assert!(
            usize::from(cw) * cols <= edge && usize::from(ch) <= edge,
            "{cols}×({cw}×{ch}) offscreen dokuya sığmıyor"
        );
        (cw, ch)
    }

    /// Mürekkepli hücre; ön plan her çağrıda [`WHITE`]. `bg_cell` ile
    /// `rule_cell`'in yanındaki üçüncü şekil — glyph çizen üç sınama aynı
    /// dörtlüyü elle kuruyordu ve biri değişince ötekiler sessizce ayrışırdı.
    fn glyph_cell(col: u16, ch: char, bg: Option<LinearRgba>) -> Cell {
        Cell {
            col,
            row: 0,
            ch: Some(ch),
            fg: WHITE,
            bg,
            ..Default::default()
        }
    }

    /// Yalnız kural taşıyan hücre: `ch: None`, `bg: None` — duman reçetesinin
    /// yedi kural hücresinin aynısı. Ön plan her çağrıda [`WHITE`].
    fn rule_cell(col: u16, underline: UnderlineStyle) -> Cell {
        Cell {
            col,
            row: 0,
            fg: WHITE,
            underline,
            ..Default::default()
        }
    }

    #[test]
    fn completion_block_counts_frame_and_reports_result() {
        // `frames()`'in anlamı bu phase'de değişti: "commit edildi" değil,
        // "GPU hatasız bitirdi". O anlamı yalnız `make duman` görüyordu ve
        // orası "> 0" diye soruyor — sayacın hiç artmaması yeşil geçerdi.
        //
        // Adının söylemediği: **hatalı** tamponun sayılmadığı. `Error`
        // durumunu isteyerek üretmenin güvenilir bir yolu yok (cihaz kaybı,
        // zaman aşımı), o dal burada koşmuyor — sınama adının bunu iddia
        // etmemesi de bu yüzden.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let seen = Arc::new(Mutex::new(Vec::new()));
        let completion = {
            let seen = Arc::clone(&seen);
            r.completion(move |result| seen.lock().unwrap().push(result.is_ok()))
        };

        let cmd = r.queue.commandBuffer().expect("komut tamponu");
        // SAFETY: blok geçerli ve `completion` çağrı boyunca yaşıyor.
        unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&completion.0)) };
        cmd.commit();
        // `waitUntilCompleted` tamamlanma handler'ları dönene kadar bekler.
        cmd.waitUntilCompleted();

        assert_eq!(r.frames(), 1, "hatasız biten kare sayılmalı");
        assert_eq!(*seen.lock().unwrap(), vec![true]);
    }

    #[test]
    fn completion_hands_over_live_gpu_timestamps() {
        // Bloğun başarı kolu komut tamponunu geçiriyor ve o tampon **canlı**:
        // damgalar `Stats`'a varıyor. Sınadığı şey **boru**, donanımın
        // davranışı değil — Apple `GPUStartTime`/`GPUEndTime`'ı "başlamadı" /
        // "bildirim gelmedi" hâllerinde sıfır döndürebiliyor ve `record_gpu`
        // sıfırı **meşru** sayıp eliyor. Eski hâli `nanos.len() == 1` diyordu,
        // yani donanımın damga verme yeteneğini `make hepsi`'nin kırmızısına
        // çeviriyordu; kendi doc'uyla çelişiyordu (`/code-review` bulgusu).
        //
        // Kaybolan sinyal telafi edildi: "bu makine damga veriyor mu"
        // sorusunun cevabı artık `BT_FRAME_STATS=1` koşusunun `gpu_elenen=`
        // jetonunda — sıfır ise damgalar canlı, kare sayısına eşitse değil.
        //
        // Kapının kendisi burada değil: ölçümü isteyen taraf `link.rs` ve
        // kapalı kapıda bu iki çağrı hiç yapılmıyor.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let stats = Arc::new(Stats::new(Instant::now(), 1));
        let completion = {
            let stats = Arc::clone(&stats);
            r.completion(move |result| {
                if let Ok(cmd) = result {
                    stats.mark_startup();
                    stats.record_gpu(cmd.GPUStartTime(), cmd.GPUEndTime());
                }
            })
        };

        const EDGE: usize = 16;
        let texture = target_texture(&r, EDGE);
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
        let cmd = r.queue.commandBuffer().expect("komut tamponu");
        r.encode_pass(&cmd, &texture, BACKGROUND, &frame)
            .expect("pass encode edilemedi");
        // SAFETY: blok geçerli ve `completion` çağrı boyunca yaşıyor.
        unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&completion.0)) };
        cmd.commit();
        cmd.waitUntilCompleted();

        assert!(
            stats.startup().is_some(),
            "ilk tamamlanan kare açılış süresini kapatır"
        );
        // Kare **bir kez** kaydedildi: ya örnek olarak ya elenmiş olarak.
        // İkisinin toplamı boruyu pinler; hangisi olduğu donanımın işi.
        let gpu = stats.gpu();
        assert_eq!(
            gpu.nanos.len() as u64 + gpu.rejected,
            1,
            "tamamlanan kare tam bir kez kaydedilir"
        );
        assert!(
            stats.cpu_frame().nanos.is_empty(),
            "CPU aralıkları bloktan değil display link'ten yazılır"
        );
    }

    #[test]
    fn cell_bg_paints_pixels_on_the_gpu() {
        // Bu sınama, phase-2'nin sildiği "çizim çağrısı pipeline'dan geçti"
        // kanıtının yerine geçiyor ve daha fazlasını söylüyor: buffer
        // indeksleri, NDC dönüşümü, y ters çevirme, instance stride'ı ve
        // GPU'nun `Instance` düzenini doğru okuması. İki taraftaki assert'ler
        // düzeni derleme zamanında bağlar ama hiçbir zaman ÇALIŞTIRMAZ;
        // burası çalıştırıyor. Pencere gerekmediği için başsız ortamda da koşar.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 16;

        // 8×8 hücre, viewport 16×16 → dört çeyrek. Sol üstte kırmızı, sağ
        // altta yeşil, sol altta paletin arka planı, sağ üst boş. İki instance
        // şart: tek instance `inst[0]` stride'dan bağımsız okunur, yani stride
        // hatası (32'den kayma) tek instance'la GÖRÜNMEZ. İkincisi ancak doğru
        // stride ile bulunur. y ters çevirme bozuksa kırmızı ile yeşil yer
        // değiştirir.
        //
        // Üçüncüsü **ara ton** ve sRGB geçişinin tek bekçisi: saf 0.0/1.0
        // sRGB transfer fonksiyonunun sabit noktaları, yani kırmızı ve yeşil
        // lineerleştirme olsa da olmasa da aynı baytı verir.
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
        frame.push(bg_cell(1, 1, LinearRgba::from_srgb(0x00, 0xff, 0x00)));
        frame.push(bg_cell(0, 1, MIDTONE));

        // Clear rengi de **ara ton**, ve bilerek temadan: üretimde pencerenin
        // görünen zemininin tamamı bu yoldan geliyor (`frame()` varsayılan
        // arka planlı hücreleri eliyor, `link.rs` clear'a temanın zeminini
        // veriyor).
        // Saf mavi bırakılsaydı `MTLClearColor`'ın sRGB hedefteki semantiği
        // sınanmamış kalırdı: onu "hedefin uzayına çevireyim" diye bir kez
        // daha kodlayan biri pencere zeminini karartır, hücreleri doğru
        // bırakır ve bütün sınamalar yeşil geçerdi.
        //
        // Clear **vurgu** (`ACCENT`), zemin değil: zemin hücrede kullanıldı ve
        // iki yolun ayrı ayrı kanıtlanması ayrık iki renk ister. Buraya
        // `BACKGROUND` "düzeltilirse" sınama hücre yolu ile clear yolunu
        // birbirinden ayırt edemez hâle gelir.
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
        assert_eq!(pixel(2, 2), (255, 0, 0), "ilk hücre sol üstte kırmızı");
        assert_eq!(pixel(12, 12), (0, 255, 0), "ikinci hücre sağ altta yeşil");
        // Beklenen bayt temanın **yazıldığı** bayt: `Theme`'in alanları
        // `0xRRGGBB`. Değerlerin kendisini `bt-core`'un palet bekçisi elle
        // yazılı listeye bağlıyor; buradaki iddia değer değil round-trip.
        let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        let close_to = |seen: (u8, u8, u8), expected: (u8, u8, u8), what: &str| {
            // ±1: 8-bit sRGB kodlaması yuvarlama taşır ve Metal spec'i bit
            // birebirlik değil doğruluk sınırı verir. Bit aransaydı kapı
            // sürücü sürümüne rehin olurdu.
            assert!(
                seen.0.abs_diff(expected.0) <= 1
                    && seen.1.abs_diff(expected.1) <= 1
                    && seen.2.abs_diff(expected.2) <= 1,
                "{what}: {seen:02x?} ≠ {expected:02x?}"
            );
        };
        // sRGB round-trip: `linear_rgba`'nın lineerleştirmesi ile donanımın
        // yazarken yaptığı kodlama birbirini tersine çevirmeli, yani ekrana
        // giden bayt yazılan bayt olmalı. Lineerleştirme düşerse `MIDTONE`
        // (`0x1a1c21`) `0x5a5d65` griye açılır — geçişin sessiz kalabileceği
        // tek yer burası; saf kırmızı, yeşil **ve artık zemin de** bunu
        // göremez, üçü de sRGB transfer fonksiyonunun sabit noktaları.
        close_to(pixel(2, 12), srgb(MIDTONE_SRGB), "hücre ara tonu");
        close_to(
            pixel(12, 2),
            srgb(Theme::BATERI.accent),
            "boş çeyrek clear rengi",
        );
    }

    #[test]
    fn a_selection_run_paints_between_the_ground_and_the_glyph() {
        // 031 phase-2'nin GPU tanığı: seçim koşusu **zeminden sonra,
        // glyph'ten önce** çiziliyor. Üç hücrelik bir koşu: 0. sütunda
        // kırmızı zeminli bir hücre (koşu onu örtmeli), 1. sütunda beyaz `M`
        // (koşunun üstünde kendi renginde kalmalı), 2. sütun boş (köprü).
        // 3. sütun koşunun dışında ve clear rengiyle kalmalı.
        //
        // Koşunun rengi temadan değil **ara ton** ([`MIDTONE`]): sRGB'nin
        // sabit noktaları lineerleştirmenin unutulmasını göremezdi. Clear
        // `ACCENT` — koşu ile clear ayrık iki renk, yoksa "boyandı" ile
        // "boyanmadı" ayırt edilemez.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 4);
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
        frame.push(glyph_cell(1, 'M', None));
        frame.push_selection(
            &[SelectionRun {
                row: 0,
                first: 0,
                last: 2,
            }],
            MIDTONE,
        );
        assert_eq!(frame.bg_count(), 1, "koşu `hucre=` sayacına girmemeli");

        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        let near = |seen: (u8, u8, u8), expected: (u8, u8, u8)| {
            seen.0.abs_diff(expected.0) <= 1
                && seen.1.abs_diff(expected.1) <= 1
                && seen.2.abs_diff(expected.2) <= 1
        };
        let midtone = srgb(MIDTONE_SRGB);
        let accent = srgb(Theme::BATERI.accent);
        let cell = |col: usize| cell_rows(&pixels, EDGE, (cw, ch), col).concat();
        // Koşunun dört köşesi yuvarlak (phase-3): köşeye `inset` pikselden
        // yakın olanlar sorunun dışında, onları köşe bekçileri soruyor.
        let inset = caret_radius_px((cw, ch), crate::frame::SELECTION_RADIUS);
        let body = |col: usize, left: bool| -> Band {
            let rows = cell_rows(&pixels, EDGE, (cw, ch), col);
            let (w, h) = (usize::from(cw), usize::from(ch));
            let mut out = Vec::new();
            for (y, row) in rows.into_iter().enumerate() {
                for (x, p) in row.into_iter().enumerate() {
                    let edge_x = if left { x < inset } else { x >= w - inset };
                    if edge_x && (y < inset || y >= h - inset) {
                        continue;
                    }
                    out.push(p);
                }
            }
            out
        };
        // Zeminli hücre koşunun altında: kırmızı köşeler dışında görünmüyor.
        assert!(
            body(0, true).iter().all(|&p| near(p, midtone)),
            "koşu zeminin altında kaldı: {:02x?}",
            body(0, true)
        );
        // Glyph koşunun üstünde, kendi renginde; harfin boşlukları koşunun
        // renginde — clear rengi koşunun içinde hiçbir yerde yok.
        let glyph = cell(1);
        // Tam bayt aranmıyor: 1x'te dikey gövdeler bile yarım piksele
        // düşebiliyor ve doygun beyaz kapsaması tam bir piksel ister. Sorulan
        // şey harfin koşudan **açık** olması — altında kalsaydı hiç görünmezdi.
        let brightest = glyph
            .iter()
            .map(|p| u32::from(p.0) + u32::from(p.1) + u32::from(p.2))
            .max()
            .unwrap_or(0);
        assert!(
            brightest > 3 * 0xc0,
            "glyph koşunun altında kaldı: {brightest}"
        );
        assert!(
            glyph.iter().any(|&p| near(p, midtone)),
            "harfin çevresi boyanmadı"
        );
        assert!(!glyph.iter().any(|&p| near(p, accent)), "koşuda delik");
        // Köprü: mürekkepsiz sütun da koşunun renginde.
        assert!(
            body(2, false).iter().all(|&p| near(p, midtone)),
            "köprü boyanmadı"
        );
        // Koşunun dışı clear.
        assert!(cell(3).iter().all(|&p| near(p, accent)), "koşu taştı");
    }

    /// Seçim bekçilerinin ortak kurulumu: köşe yarıçapı birkaç piksel olsun
    /// diye **büyük** yapay hücre (40×80 → yarıçap 17.6), atlas gerekmiyor —
    /// karede glyph yok. Dönen şey pikselin ara tona (seçim) mi clear'a mı
    /// yakın olduğunu söyleyen okuyucu.
    fn render_selection(runs: &[SelectionRun]) -> impl Fn(usize, usize) -> &'static str {
        const EDGE: usize = 256;
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let mut frame = Frame::default();
        frame.clear(grid(40, 80), CaretStyle::default());
        assert!(
            (frame.selection_radius() - 17.6).abs() < 1e-4,
            "yarıçap varsayımı: {}",
            frame.selection_radius()
        );
        frame.push_selection(runs, MIDTONE);
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        let (midtone, accent) = (srgb(MIDTONE_SRGB), srgb(Theme::BATERI.accent));
        let near = |seen: (u8, u8, u8), expected: (u8, u8, u8)| {
            seen.0.abs_diff(expected.0) <= 1
                && seen.1.abs_diff(expected.1) <= 1
                && seen.2.abs_diff(expected.2) <= 1
        };
        move |x, y| {
            let p = pixel_at(&pixels, EDGE, x, y);
            if near(p, midtone) {
                "selection"
            } else if near(p, accent) {
                "clear"
            } else {
                "blend"
            }
        }
    }

    #[test]
    fn a_lone_selection_run_has_round_corners_and_a_solid_body() {
        // İki hücrelik koşu: x 0..80, y 0..80. Köşe pikselinin merkezi
        // yarıçapı 17.6 olan yayın çok dışında → clear; kenarın ortası ve
        // yayın içi seçim rengi — kenarlar piksel ızgarasında, yani düz
        // kenarda yarım alfa yok.
        let at = render_selection(&[SelectionRun {
            row: 0,
            first: 0,
            last: 1,
        }]);
        for (x, y) in [(0, 0), (79, 0), (79, 79), (0, 79)] {
            assert_eq!(at(x, y), "clear", "köşe ({x},{y}) yuvarlanmadı");
        }
        for (x, y) in [(40, 0), (0, 40), (79, 40), (40, 79), (40, 40), (8, 8)] {
            assert_eq!(at(x, y), "selection", "gövde ({x},{y}) boyanmadı");
        }
        assert_eq!(at(80, 40), "clear", "koşu taştı");
    }

    #[test]
    fn a_selection_step_fills_its_concave_corner() {
        // Üstte 2..=3 (x 80..160), altta 0..=3 (x 0..160): üst koşunun sol
        // alt köşesi içbükey. Dolgu [62.4,80]×[62.4,80]'de, merkezi
        // (62.4,62.4) olan dairenin dışını boyuyor: basamağın dibindeki piksel
        // seçim rengi, dairenin içindeki (72,72) clear.
        let at = render_selection(&[
            SelectionRun {
                row: 0,
                first: 2,
                last: 3,
            },
            SelectionRun {
                row: 1,
                first: 0,
                last: 3,
            },
        ]);
        assert_eq!(at(79, 79), "selection", "içbükey köşe dolmadı");
        assert_eq!(at(72, 72), "clear", "dolgu dairenin içini boyadı");
        // Açıkta kalan köşeler yuvarlak, örtülen köşeler kare.
        assert_eq!(at(80, 0), "clear", "üst koşunun sol üstü");
        assert_eq!(at(0, 80), "clear", "alt koşunun sol üstü");
        assert_eq!(at(159, 79), "selection", "hizalı sağ kenar dikişi");
        assert_eq!(at(159, 80), "selection", "hizalı sağ kenar dikişi");
        assert_eq!(at(100, 79), "selection", "iki satırın dikişi");
        assert_eq!(at(100, 80), "selection", "iki satırın dikişi");
        assert_eq!(at(159, 159), "clear", "alt koşunun sağ altı");
    }

    /// Arama bekçilerinin iki rengi: ara tonlar (sabit nokta değil), seçimin
    /// [`MIDTONE`]'undan ve clear'ın `ACCENT`'inden ayrık — dördü de piksel
    /// okumasında ayırt edilebilmeli.
    const MATCH_SRGB: u32 = 0x3c6e5a;
    const CURRENT_SRGB: u32 = 0x8a5a2c;

    fn srgb_linear(hex: u32) -> LinearRgba {
        LinearRgba::from_srgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
    }

    fn search_run(row: u16, first: u16, last: u16, current: bool, continues: bool) -> SearchRun {
        SearchRun {
            row,
            first,
            last,
            current,
            continues,
        }
    }

    /// Arama bekçilerinin ortak kurulumu, [`render_selection`] emsali: 40×80
    /// yapay hücre (yarıçap 17.6), atlas yok. `fill` sıfır değilse bant o
    /// kadar satır ve ızgara bir satır aşağıda ([`Frame::set_origin_rows`]),
    /// yani bandın 0. satırı pencerenin 0..80'i. Okuyucu pikselin hangi
    /// yüzeye ait olduğunu söyler.
    fn render_search(
        grid_runs: &[SearchRun],
        fill_runs: &[SearchRun],
        selection: &[SelectionRun],
    ) -> impl Fn(usize, usize) -> &'static str + use<> {
        const EDGE: usize = 256;
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let mut frame = Frame::default();
        frame.clear(grid(40, 80), CaretStyle::default());
        if !fill_runs.is_empty() {
            frame.set_fill_rows(1);
        }
        frame.push_search(
            grid_runs,
            srgb_linear(MATCH_SRGB),
            srgb_linear(CURRENT_SRGB),
        );
        frame.push_fill_search(fill_runs);
        frame.push_selection(selection, MIDTONE);
        if !fill_runs.is_empty() {
            frame.set_origin_rows(1.0);
        }
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        let near = |seen: (u8, u8, u8), expected: (u8, u8, u8)| {
            seen.0.abs_diff(expected.0) <= 1
                && seen.1.abs_diff(expected.1) <= 1
                && seen.2.abs_diff(expected.2) <= 1
        };
        move |x, y| {
            let p = pixel_at(&pixels, EDGE, x, y);
            [
                (MATCH_SRGB, "match"),
                (CURRENT_SRGB, "current"),
                (MIDTONE_SRGB, "selection"),
                (Theme::BATERI.accent, "clear"),
            ]
            .into_iter()
            .find(|&(hex, _)| near(p, srgb(hex)))
            .map_or("blend", |(_, name)| name)
        }
    }

    #[test]
    fn search_roles_paint_their_colors_under_the_selection() {
        // 033 Karar 7'nin sırası GPU'da: zemin → `search_match` →
        // `search_current` → seçim. Eşleşme 0. satırın 0..=2'si, geçerli
        // eşleşme 2. satırın 0..=1'i; seçim 0. satırın 2..=3'ü ve eşleşmenin
        // son hücresini örtüyor — kullanıcının seçimi aramanın üstünde.
        let at = render_search(
            &[
                search_run(0, 0, 2, false, false),
                search_run(2, 0, 1, true, false),
            ],
            &[],
            &[SelectionRun {
                row: 0,
                first: 2,
                last: 3,
            }],
        );
        assert_eq!(at(40, 40), "match", "eşleşme kendi renginde değil");
        assert_eq!(
            at(40, 200),
            "current",
            "geçerli eşleşme kendi renginde değil"
        );
        assert_eq!(at(100, 40), "selection", "seçim aramanın altında kaldı");
        assert_eq!(at(140, 40), "selection");
        // Yuvarlak köşe ve koşunun dışı.
        assert_eq!(at(0, 0), "clear", "eşleşmenin köşesi yuvarlanmadı");
        assert_eq!(at(0, 160), "clear", "geçerli eşleşmenin köşesi");
        assert_eq!(at(40, 120), "clear", "satırlar arası boyandı");
    }

    #[test]
    fn adjacent_matches_are_two_shapes_and_a_wrapped_match_is_one() {
        // Ardışık satırlarda hizalı iki koşu: iki ayrı eşleşmeyse dikişte
        // dört yuvarlak köşe (sol kenarın 79/80. satırları clear), tek
        // eşleşmenin sarılmasıysa düz kenar (boyalı).
        let runs = |continues| {
            [
                search_run(0, 0, 1, false, false),
                search_run(1, 0, 1, false, continues),
            ]
        };
        let at = render_search(&runs(false), &[], &[]);
        assert_eq!(at(0, 79), "clear", "üst eşleşmenin alt köşesi kare");
        assert_eq!(at(0, 80), "clear", "alt eşleşmenin üst köşesi kare");
        assert_eq!(at(40, 40), "match");
        assert_eq!(at(40, 120), "match");
        let at = render_search(&runs(true), &[], &[]);
        assert_eq!(at(0, 79), "match", "sarılan eşleşme ikiye bölündü");
        assert_eq!(at(0, 80), "match", "sarılan eşleşme ikiye bölündü");
    }

    #[test]
    fn the_fill_band_highlights_its_matches() {
        // 033 Karar 8: bandın satırları gerçek geçmiş, eşleşmeleri de
        // vurgulanıyor — bandın kendi viewport'unda. Bant 0..80'de (ızgara bir
        // satır aşağıda); ızgaranın aynı numaralı satırına (80..160) hiçbir
        // şey düşmemeli.
        let at = render_search(&[], &[search_run(0, 0, 1, true, false)], &[]);
        assert_eq!(at(40, 40), "current", "bantta vurgu yok");
        assert_eq!(at(40, 120), "clear", "bandın vurgusu ızgaraya düştü");
    }

    #[test]
    fn a_frame_without_search_draws_todays_picture() {
        // Arama kapalıyken (iki liste boş) encoder vurguyu hiç görmüyor ve
        // kare bugünküyle **bit bit** aynı; doldurmanın geri alma şeridiyle
        // aynı örüntü. Açıkken ayrışmak zorunda, yoksa eşitlik bir şey
        // söylemez.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let draw = |search: Option<&[SearchRun]>| {
            let mut frame = Frame::default();
            frame.clear(grid(16, 32), CaretStyle::default());
            frame.push(bg_cell(0, 0, MIDTONE));
            frame.set_fill_rows(1);
            frame.push_fill(bg_cell(1, 0, MIDTONE));
            if let Some(runs) = search {
                frame.push_search(runs, srgb_linear(MATCH_SRGB), srgb_linear(CURRENT_SRGB));
                frame.push_fill_search(&[]);
            }
            frame.set_origin_rows(1.0);
            render_offscreen(&r, EDGE, ACCENT, &frame)
        };
        let today = draw(None);
        assert!(today == draw(Some(&[])), "boş arama kareyi değiştirdi");
        assert!(
            today != draw(Some(&[search_run(0, 1, 2, false, false)])),
            "arama hiç çizilmedi"
        );
    }

    /// Bir dörtgen bölgenin pikselleri, [`pixel_at`]'in üçlüsüyle.
    ///
    /// Ad, tipin karmaşıklığı için değil okunurluk için: iki bekçi de "bu
    /// bölge hangi rengi taşıyor" diye soruyor ve imzada `Vec<(u8, u8, u8)>`
    /// çifti o soruyu söylemiyordu.
    type Band = Vec<(u8, u8, u8)>;

    /// Ötelenmiş bir karede 0. satırın hücresi, ötelendiği yerde boyanmış mı
    /// ve **eski yeri** clear rengiyle mi kalmış.
    ///
    /// İki bekçinin ortak gövdesi: kurulum (8 px hücre, 16 px doku, bir satır
    /// öteleme) ve iki bölgenin okunması. Kopyalansaydı "üst bölge boş kaldı"
    /// yarısı birinde unutulabilirdi — ve o yarı olmadan sınama, ötelemeyi
    /// **yok sayan** bir kodu da geçirir: alt bölgede zaten hücre yok diye
    /// bakmazdı.
    ///
    /// Dönüş `(üst bölge, alt bölge)`, satır satır: iddia hangi bölgenin hangi
    /// rengi taşıdığı.
    fn origin_shifted_halves(r: &Renderer, frame: &mut Frame, clear: LinearRgba) -> (Band, Band) {
        const EDGE: usize = 16;
        const CELL: usize = 8;
        // Bir satır öteleme: 0. satırın hücresi y ∈ [0, 8) yerine [8, 16)'ya
        // düşmeli. `set_origin_rows` piksele `clear`'ın hücre boyuyla
        // çeviriyor, yani ölçü bu çağrıdan **önce** kurulmuş olmalı.
        frame.set_origin_rows(1.0);
        assert_eq!(frame.origin_px(), CELL as f32, "öteleme piksele çevrilmedi");
        let pixels = render_offscreen(r, EDGE, clear, frame);
        let band = |y0: usize| {
            (y0..y0 + CELL)
                .flat_map(|y| (0..CELL).map(move |x| (x, y)))
                .map(|(x, y)| pixel_at(&pixels, EDGE, x, y))
                .collect::<Vec<_>>()
        };
        (band(0), band(CELL))
    }

    #[test]
    fn content_sticks_to_the_bottom_for_cell_bg() {
        // **Bu setin CPU→GPU dikişi.** Öteleme `setViewport` ile GPU'da
        // uygulanıyor, yani iki CPU listesini birbirine karşı ölçen bir sınama
        // inşa gereği doğru olan bir şeyi sınardı (`discussion.md` → Muhakeme
        // 2. tur, kabul 4). Sorulan şey boyanan **pikselin** kaydığı.
        //
        // Aynı zamanda `setViewport` kanaryası: viewport dokunun altına
        // taşıyor (origin 8 + boy 16 = 24 > 16) ve Metal'in taşan fragment'i
        // kırpması şart. Kırpmasaydı ya doğrulama hatası düşerdi ya alt bölge
        // sarardı; ikisi de burada görünür.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        frame.push(bg_cell(0, 0, red));

        // Clear **vurgu**: hücrenin rengiyle ayrık olmak zorunda, yoksa
        // "hücre kaydı" ile "her yer clear" ayırt edilemez.
        let (top, bottom) = origin_shifted_halves(&r, &mut frame, ACCENT);
        assert!(
            bottom.iter().all(|&p| p == (255, 0, 0)),
            "0. satırın hücresi bir satır aşağıda boyanmadı: {bottom:02x?}"
        );
        assert!(
            top.iter().all(|&p| p != (255, 0, 0)),
            "hücre eski satırında da kaldı: {top:02x?}"
        );
    }

    #[test]
    fn content_sticks_to_the_bottom_for_glyphs() {
        // Kardeşinin `cell` pipeline'ı için ikizi ve **bu setin asıl riski**:
        // iki pipeline ayrı `setRenderPipelineState` çağrısı ve ayrı shader
        // çifti, yani birinin ötelenip ötekinin ötelenmemesi temsil edilebilir
        // bir hâl — üstelik `make hepsi`'yi yeşil bırakan bir hâl. Tek satırlık
        // `setViewport` ikisini birden kaydırıyor; bu sınama onu koda bağlıyor,
        // yorum cümlesine değil.
        //
        // Glyph ölçüsü atlasın hücresine bağlı **değil**: sorulan şey dörtlünün
        // konumu ve 8 px hücreyle atlas yuvası esner, bozulmaz.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        // "Önce metriği sor": atlasın anahtarının ölçek yarısı pencereden
        // gelir, bu sınamanın penceresi yok ve söylenmezse kare
        // `GpuError::NoAtlas` ile düşer. Dönen ölçü **kullanılmıyor** — dörtlü
        // 8 px, yani atlas yuvası esner; sorulan şey konum, çözünürlük değil.
        r.cell_metrics(1.0);
        let mut frame = Frame::default();
        frame.clear(grid(8, 8), CaretStyle::default());
        // Arka plansız `M`: iddia yalnız `cell` pipeline'ına ait olsun.
        // `cell_bg` listesi boş kaldığı için üst bölgede tek tanık clear.
        frame.push(glyph_cell(0, 'M', None));
        assert_eq!(frame.bg_count(), 0, "arka plan iddiaya karışmamalı");

        let (top, bottom) = origin_shifted_halves(&r, &mut frame, BACKGROUND);
        let clear = {
            let hex = Theme::BATERI.background;
            ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
        };
        // Glyph'in tam baytı aranmıyor (emsal `glyphs_paint_pixels_on_the_gpu`):
        // sorulan şey "clear'dan farklı bir piksel hangi bölgede".
        assert!(
            bottom.iter().any(|&p| p.0.abs_diff(clear.0) > 1
                || p.1.abs_diff(clear.1) > 1
                || p.2.abs_diff(clear.2) > 1),
            "glyph bir satır aşağıda çizilmedi: {bottom:02x?}"
        );
        assert!(
            top.iter().all(|&p| p.0.abs_diff(clear.0) <= 1
                && p.1.abs_diff(clear.1) <= 1
                && p.2.abs_diff(clear.2) <= 1),
            "glyph eski satırında da kaldı: {top:02x?}"
        );
    }

    /// **017 phase-0'ın ölçümü, sınamaya çivilenmiş hâli.**
    ///
    /// 017'nin doldurma bandı ızgaranın **üstüne** düşecek ve adayı üçüncü bir
    /// `setViewport`: `originY = origin_px − fill_px`. O sayı kaymanın
    /// ortasında negatife iniyor, oysa [`Renderer::encode_dock`]'un kırpması
    /// *"negatif bir `originY` Metal'in doğrulamasına düşerdi — süreci öldüren
    /// bir istisna"* diyordu ve o cümle **ölçülmemişti**. Bu sınama onu
    /// ölçüyor; sayıları, makinesi ve doğrulama katmanının cevabı
    /// `.tasks/017-ekranin-geri-donusu/phase-0.md` → Uygulama Notları'nda.
    ///
    /// Sorulan tek şey `MTLViewport`'un `originY` alanı: hangi listenin
    /// çizildiği Metal'i ilgilendirmiyor, bu yüzden tanık ızgaranın **kendi**
    /// viewport'undan geçiyor ([`Frame::set_origin_rows`] negatif satırı kabul
    /// ediyor) ve ikinci bir encode yolu icat etmiyor.
    ///
    /// Beş değerin **dördü yerleşik değil** ve üçünde `originY` negatif, yani
    /// kaymanın ortası: yerleşik kare tek başına sorulsaydı kanarya dinlenmede
    /// geçer, 150 ms'lik kaymanın ortasında düşerdi (phase-0 → Kabul).
    #[test]
    fn a_negative_viewport_origin_draws_and_clips_from_the_top() {
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 16;
        const CELL: u16 = 8;
        // Doldurma **bir satır**: `fill_px` bir hücre, yani yerleşik karede
        // `origin_px` onun iki katı ve `originY` artıda; kayma boyunca
        // `origin_px` düşüyor ve fark sıfırı geçip negatife iniyor.
        const FILL_PX: f32 = CELL as f32;
        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
        let blue = LinearRgba::from_srgb(0x00, 0x00, 0xff);
        // Clear **vurgu**, kardeşleriyle aynı gerekçe: hücrelerin rengiyle
        // ayrık olmak zorunda, yoksa "bant kaydı" ile "her yer clear" ayırt
        // edilemez. ±1 çünkü vurgu bir ara ton ve 8-bit sRGB kodlaması
        // yuvarlama taşıyor (emsal `cell_bg_paints_pixels_on_the_gpu`).
        let clear = {
            let hex = Theme::BATERI.accent;
            ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
        };
        let near = |seen: (u8, u8, u8), want: (u8, u8, u8)| {
            seen.0.abs_diff(want.0) <= 1
                && seen.1.abs_diff(want.1) <= 1
                && seen.2.abs_diff(want.2) <= 1
        };

        let mut frame = Frame::default();
        frame.clear(grid(CELL, CELL), CaretStyle::default());
        // 0. satır doldurmanın, 1. satır içeriğin ilk satırı. İki ayrı renk
        // şart: "üst bant kırpıldı" ile "iki bant birden kaydı" ancak
        // birbirinden ayrık renklerle ayırt edilebiliyor.
        frame.push(bg_cell(0, 0, red));
        frame.push(bg_cell(0, 1, green));

        // Viewport'un boyu dokunun boyu kalıyor (üretimdeki gibi) ve iki
        // satır tam onu kaplıyor: viewport-yerel y ∈ [0, EDGE) içerisi,
        // dışarısı kırpılan.
        for origin_px in [2.0 * FILL_PX, FILL_PX, 5.0, 3.0, 0.0] {
            let origin_y = origin_px - FILL_PX;
            frame.set_origin_rows(origin_y / f32::from(CELL));
            assert_eq!(frame.origin_px(), origin_y, "öteleme piksele çevrilmedi");
            let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
            for y in 0..EDGE {
                let local = y as f32 - origin_y;
                let want = if !(0.0..EDGE as f32).contains(&local) {
                    // Viewport'un dışı — "Fragments that lie outside of the
                    // viewport are clipped". Negatif orijinin sınadığı yarı
                    // **üstteki** kırpma: doldurma bandının ekrana sığmayan
                    // parçası clear rengiyle kalmalı, sarmamalı.
                    clear
                } else if local < FILL_PX {
                    (255, 0, 0)
                } else {
                    (0, 255, 0)
                };
                let seen = pixel_at(&pixels, EDGE, 2, y);
                assert!(
                    near(seen, want),
                    "originY={origin_y}, y={y}: {seen:02x?} ≠ {want:02x?}"
                );
            }
        }

        // **Aynı encoder'da negatif viewport'un ardından ikincisi**: 2b-i'nin
        // şekli tam olarak bu (ızgara → doldurma → dock). Encoder negatif
        // orijinde düşseydi dock da çizilmezdi, yani dock'un zemini bütün
        // pass'in sağ çıktığının tanığı.
        frame.set_origin_rows(-FILL_PX / f32::from(CELL));
        frame.set_dock_rows(1);
        frame.open_dock(blue, WHITE);
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        assert!(
            near(pixel_at(&pixels, EDGE, 14, 12), (0, 0, 255)),
            "negatif viewport'tan sonra dock'un zemini çizilmedi"
        );
        // Ve ızgaranın kendisi: `originY = -8` ile 0. satır tamamen kırpıldı,
        // 1. satır dokunun tepesine oturdu.
        assert!(
            near(pixel_at(&pixels, EDGE, 2, 4), (0, 255, 0)),
            "negatif orijinde içeriğin satırı tepeye oturmadı"
        );
    }

    /// Doldurma bandının ölçüsü: bir satır, hücre boyu kadar.
    ///
    /// İki bekçinin ortak kurulumu bu sayı üzerinden okunuyor; elle yazılsaydı
    /// biri değişip öteki sessizce eski kalabilirdi.
    const FILL_ROWS: u16 = 1;

    #[test]
    fn the_fill_band_draws_above_the_content_and_rides_the_origin() {
        // **017 phase-3'ün tek görünür iddiası ve R3.1'in piksel yarısı.**
        // Bant ötelemenin üstüne çiziliyor (üçüncü `setViewport`,
        // `originY = origin_px − fill_px`) ve **hareket karesinde** — listeler
        // korunur, yalnız `origin_px` değişir — ızgarayla birlikte kayıyor.
        // Push anında pişmiş bir konum ikinci yarıyı düşürürdü: bant yerinde
        // donar, ızgara süzülür ve aradaki dikiş görünürdü.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 16;
        const CELL: u16 = 8;
        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
        // Clear **vurgu**, kardeşleriyle aynı gerekçe: iki bandın rengiyle de
        // ayrık olmak zorunda. ±1 çünkü vurgu bir ara ton.
        let clear = {
            let hex = Theme::BATERI.accent;
            ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
        };
        let near = |seen: (u8, u8, u8), want: (u8, u8, u8)| {
            seen.0.abs_diff(want.0) <= 1
                && seen.1.abs_diff(want.1) <= 1
                && seen.2.abs_diff(want.2) <= 1
        };

        let mut frame = Frame::default();
        frame.clear(grid(CELL, CELL), CaretStyle::default());
        // İçerik tek satır ve tabana yaslı: öteleme bir satır, üstünde bir
        // satırlık boşluk kalıyor ve doldurma tam oraya düşüyor.
        frame.push(bg_cell(0, 0, green));
        frame.set_fill_rows(FILL_ROWS);
        // Satır **fill-yerel**: `0` bandın tek satırı, ızgaranın 0. satırı
        // değil. İkisi aynı numarayı taşıyor ve ayrı uzaylarda çiziliyorlar —
        // iddia tam olarak bu.
        frame.push_fill(bg_cell(0, 0, red));
        frame.set_origin_rows(1.0);

        let cell_px = f32::from(CELL);
        let band = |pixels: &[u8], origin_px: f32| {
            for y in 0..EDGE {
                let from_fill = y as f32 - (origin_px - cell_px * f32::from(FILL_ROWS));
                let from_grid = y as f32 - origin_px;
                let want = if (0.0..cell_px).contains(&from_fill) {
                    (255, 0, 0)
                } else if (0.0..cell_px).contains(&from_grid) {
                    (0, 255, 0)
                } else {
                    // Ne bant ne içerik: doldurmanın taşan parçası kırpılıyor
                    // (üstte) ya da ızgaranın altında hiçbir şey yok.
                    clear
                };
                let seen = pixel_at(pixels, EDGE, 2, y);
                assert!(
                    near(seen, want),
                    "origin_px={origin_px}, y={y}: {seen:02x?} ≠ {want:02x?}"
                );
            }
        };

        // Yerleşik kare: bant [0, 8), içerik [8, 16).
        band(&render_offscreen(&r, EDGE, ACCENT, &frame), 8.0);

        // **Hareket karesi**: `link.rs` bu kolda `clear` da `frame()` de
        // çağırmıyor, yalnız ötelemeyi yeniden yazıyor. Yarım satır aşağıda
        // bandın orijini **negatife** iniyor (−4): üst yarısı kırpılmalı, alt
        // yarısı içeriğin hemen üstünde durmalı.
        frame.set_origin_rows(0.5);
        assert_eq!(
            frame.fill_origin_px(),
            -4.0,
            "bandın orijini negatife inmedi"
        );
        band(&render_offscreen(&r, EDGE, ACCENT, &frame), 4.0);
    }

    #[test]
    fn a_frame_without_fill_draws_todays_picture() {
        // **Geri alma şeridi** (R2.4/R3.2), 016'nın "yarıçap 0, hale 0"
        // örüntüsü: doldurma kapalıyken çizilen kare bugünküyle **bit bit**
        // aynı olmak zorunda ve bunu yalnız GPU söyleyebilir. Üçüncü viewport
        // koşulsuz kurulsaydı (ya da `clear` bandın boyunu unutsaydı) üçüncü
        // okuma birinciden ayrışırdı — sessiz kalabilecek tek kusur o.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 16;
        const CELL: u16 = 8;
        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);

        let mut frame = Frame::default();
        let today = |frame: &mut Frame| {
            frame.clear(grid(CELL, CELL), CaretStyle::default());
            frame.push(bg_cell(0, 0, green));
            frame.set_origin_rows(1.0);
        };

        today(&mut frame);
        let before = render_offscreen(&r, EDGE, ACCENT, &frame);

        // Aynı kare, bir satır doldurma ile: ayrışmak **zorunda**, yoksa
        // aşağıdaki eşitlik hiçbir şey söylemez.
        frame.clear(grid(CELL, CELL), CaretStyle::default());
        frame.push(bg_cell(0, 0, green));
        frame.set_fill_rows(FILL_ROWS);
        frame.push_fill(bg_cell(0, 0, red));
        frame.set_origin_rows(1.0);
        let filled = render_offscreen(&r, EDGE, ACCENT, &frame);
        assert!(before != filled, "doldurma bandı hiç çizilmedi");

        // Ve doldurma kapanınca: `clear` bandın boyunu da sıfırlıyor, yani
        // `encode_fill` erken dönüyor ve encoder doldurmayı hiç görmüyor.
        today(&mut frame);
        let after = render_offscreen(&r, EDGE, ACCENT, &frame);
        let diff = before.iter().zip(&after).position(|(a, b)| a != b);
        assert!(
            diff.is_none(),
            "doldurma kapanınca kare bugünküyle ayrıştı, ilk fark {diff:?}. baytta"
        );
    }

    #[test]
    fn command_marks_paint_the_gutter_on_the_gpu() {
        // İşaretin GPU tarafı: `Frame::stripes` bir **CPU** listesi ve kardeş
        // sayaçların aksine duman jetonu bile yok — bu sınama düşerse işaretin
        // çizildiğini söyleyen başka hiçbir bekçi kalmıyor.
        //
        // **İşaret artık bir sprite**, dikdörtgen değil: dock'un chevron'uyla
        // aynı şekil (012 phase-9, kullanıcı: "sonuç renk kutuları da bu yeni
        // > olacak"). Şeklin kendi bekçisi `bt-atlas`'ta; buranın işi boru
        // hattı — doğru satır, doğru renk, payın içinde.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        // Hücre **atlasın kendi ölçüsünde**: sprite'ı dörtte bir ölçeğe
        // indirmek kapsamayı eritir ve sınama şekli değil ölçeklemeyi ölçerdi.
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);
        assert!(usize::from(ch) * 3 <= EDGE, "üç satır dokuya sığmıyor");
        // Pay bir hücre; işaret artık payda değil **0. sütunda** (012
        // phase-11), yani pay saf sol kenar boşluğu.
        let gutter = cw;

        let mut frame = Frame::default();
        frame.clear(
            CellMetrics::new(cw, ch, cw, gutter, 1).expect("ölçü"),
            CaretStyle::default(),
        );
        // İki işaret, iki durum rengi: 0. satır başarılı, 2. satır başarısız.
        // Aradaki satır (çıktı) **işaretsiz** kalmalı.
        frame.push_block(Block {
            row: 0,
            stripe: Theme::BATERI.success_linear(),
        });
        frame.push_block(Block {
            row: 2,
            stripe: Theme::BATERI.error_linear(),
        });
        // Komutun **ilk harfinin** hücresi: 2. sütun, çünkü prompt gerçekten
        // iki sütun geniş (`__bateri_ps1`). İşaretin ona değmediğinin tanığı.
        frame.push(bg_cell(2, 0, WHITE));

        // Clear üç rengin de dışında: işaretin bulunmadığı her piksel bunu
        // okumalı ve "işaret payı aştı" hatası clear ile ayırt edilebilsin.
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
        let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);

        // Kenar yumuşatma yüzünden **eşitlik sorulamaz**: sprite'ın yalnız
        // çekirdeği tam kapsama veriyor. İddia bu yüzden mesafeye bakıyor —
        // bandın clear'dan en uzak pikseli hangi işaret rengine yakın.
        let distance = |a: (u8, u8, u8), b: (u8, u8, u8)| {
            i32::from(a.0).abs_diff(i32::from(b.0)).pow(2)
                + i32::from(a.1).abs_diff(i32::from(b.1)).pow(2)
                + i32::from(a.2).abs_diff(i32::from(b.2)).pow(2)
        };
        let clear = srgb(Theme::BATERI.accent);
        let band = usize::from(ch);
        // İşaretin bandı **0. sütun**, pay değil: pay artık boş.
        let mark_x = usize::from(gutter)..usize::from(gutter) + usize::from(cw);
        let boldest = |row: usize| {
            mark_x
                .clone()
                .flat_map(|x| (row * band..(row + 1) * band).map(move |y| (x, y)))
                .map(|(x, y)| pixel(x, y))
                .max_by_key(|&seen| distance(seen, clear))
                .expect("bant boş")
        };
        let (success, error) = (srgb(Theme::BATERI.success), srgb(Theme::BATERI.error));
        let first = boldest(0);
        assert!(
            distance(first, success) < distance(first, error),
            "ilk komut başarı renginde değil: {first:02x?}"
        );
        let second = boldest(2);
        assert!(
            distance(second, error) < distance(second, success),
            "ikinci komut hata renginde değil: {second:02x?}"
        );
        // **Çıktı satırı işaretsiz** (kullanıcı kararı, 010 teslim): payı clear
        // rengiyle **aynı** kalmalı, tek bir mürekkep pikseli bile yok. Bu,
        // işaretin bir hücre boyunda kaldığının tek piksel kanıtı — yükseklik
        // satır aralığına dönerse burası düşer.
        assert_eq!(boldest(1), clear, "çıktı satırı işaret aldı");

        // **Komutun harfi dokunulmamış:** 2. sütun beyaz kaldı. İşaret bir
        // hücre boyunda ve 0. sütunda; prompt iki sütun geniş olduğu için
        // aradaki 1. sütun da boş, yani işaret metne hiçbir ölçekte değmiyor.
        assert_eq!(
            pixel(
                usize::from(gutter) + 2 * usize::from(cw) + usize::from(cw) / 2,
                band / 2
            ),
            (255, 255, 255),
            "komutun ilk harfi işaretin sağında değil"
        );
        // **Sol pay boş.** İşaret oradan 0. sütuna taşındı; payda mürekkep
        // kalsaydı iki işaret arasındaki hiza yine bozuk olurdu.
        for x in 0..usize::from(gutter) {
            assert_eq!(pixel(x, band / 2), clear, "sol payda mürekkep var");
        }
    }

    #[test]
    fn the_dock_paints_the_bottom_band_and_the_sliding_grid_cannot_reach_it() {
        // **Phase-3'ün CPU→GPU dikişi.** İkinci `setViewport` iki şeyi birden
        // iddia ediyor ve ikisi de yalnız pikselden okunabiliyor: dock
        // dokunun **altına** yaslanıyor, ve ızgaranın ötelemesi onu
        // oynatmıyor. İkisini iki CPU listesini karşılaştırarak sormak inşa
        // gereği doğru olanı sınamak olurdu (`content_sticks_to_the_bottom_*`
        // ile aynı gerekçe).
        //
        // Üçüncü iddia kaymanın taşması: `LinkDelegate::set_origin`'in doc'u
        // "kayma boyunca öteleme hedefinden büyük, en alt satırın bir kısmı
        // pencerenin altında kalıyor" diyor — dock gelince o parça dock'un
        // **üstüne** düşüyor ve onu örten tek şey opak zemin. Ötelenmiş kare
        // bu yüzden aynı bantta yine dock'un renklerini vermeli.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 16;
        const CELL: u16 = 8;
        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
        let blue = LinearRgba::from_srgb(0x00, 0x00, 0xff);
        // Üç renk de **doygun**: sRGB transfer fonksiyonunun sabit noktaları,
        // yani bayt eşitlikle sorulabiliyor. Renk uzayının kendi bekçisi
        // `cell_bg_paints_pixels_on_the_gpu` ve orada ara ton var.

        // Tek satırlık dock: 8 piksel, yani doku ikiye bölünüyor — üstte
        // ızgara, altta dock. `DOCK_ROWS` burada **kullanılmıyor** ve bilerek:
        // renderer kaç satır olduğunu bilmiyor, yalnız verilen payı çiziyor.
        let mut frame = Frame::default();
        frame.clear(grid(CELL, CELL), CaretStyle::default());
        frame.push(bg_cell(0, 0, red));
        frame.push_dock(bg_cell(0, 0, blue));
        frame.set_dock_rows(1);
        frame.open_dock(green, WHITE);

        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);

        // Üst yarı ızgaranın: 0. satırın hücresi orada.
        assert_eq!(pixel(2, 2), (255, 0, 0), "ızgara hücresi üst yarıda değil");
        // Alt yarı dock'un: hücresi solda, zemini onun sağında **doku
        // genişliğince**. Zemin yalnız ızgaranın sütunlarını kaplasaydı
        // sağdaki artık şerit clear rengiyle kalırdı.
        assert_eq!(pixel(2, 12), (0, 0, 255), "dock hücresi boyanmadı");
        assert_eq!(pixel(14, 12), (0, 255, 0), "dock zemini dokuyu kaplamadı");
        // Ayraç dock'un en üst pikselinde ve zeminden ayrı: ikisi tek
        // dikdörtgene inseydi sınır kaybolurdu.
        assert_eq!(
            pixel(14, 8),
            (255, 255, 255),
            "ayraç dock'un tepesinde değil"
        );

        // **Aynı kare, bir satır ötelenmiş.** Izgaranın hücresi alt yarıya
        // taşıyor (`content_sticks_to_the_bottom_for_cell_bg`'nin kurulumu) ve
        // tam dock'un üstüne düşüyor. Dock ondan **sonra** çizildiği ve zemini
        // opak olduğu için alt yarı hiç kırmızı görmemeli; dock'un kendisi de
        // yerinden oynamamalı.
        frame.set_origin_rows(1.0);
        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
        assert_eq!(pixel(2, 12), (0, 0, 255), "dock ötelemeyle birlikte kaydı");
        assert_eq!(pixel(14, 12), (0, 255, 0), "dock zemini ötelemeyle kaydı");
        assert!(
            (8..EDGE).all(|y| (0..EDGE).all(|x| pixel(x, y) != (255, 0, 0))),
            "kayan ızgara dock'un üstünde göründü"
        );
        // Izgaranın eski yeri boşaldı: öteleme gerçekten uygulandı, yoksa
        // yukarıdaki iddia ötelemeyi **yok sayan** bir kodla da geçerdi.
        assert_ne!(pixel(2, 2), (255, 0, 0), "ızgara ötelenmedi");
    }

    #[test]
    fn a_growing_band_reveals_its_rows_from_the_bottom() {
        // **032 phase-2, piksel yarısı.** Üç giriş satırlık dock (dört satırlık
        // yerleşim): hücreler dibe yaslı ve yerleşimin viewport'undan, zemin
        // bandın o anki boyundan. Bant henüz yükselmemişken (fazla 0) bandın
        // tepesinin üstüne taşan giriş satırı **çizilmemeli** — zeminsiz,
        // ızgaranın alt satırlarının üstünde bir metin olurdu. Kırpmayı makas
        // yapıyor (`scissor_below`) ve yalnız büyüyen bantta.
        //
        // 48 px doku, 8 px hücre, paysız: yerleşim 32 px (16..48), PTY payı
        // iki satır (32..48). Satır 0 → 16..24, satır 2 → 32..40, bağlam
        // satırı → 40..48.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 48;
        const CELL: u16 = 8;
        let blue = LinearRgba::from_srgb(0x00, 0x00, 0xff);
        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let green = LinearRgba::from_srgb(0x00, 0xff, 0x00);
        let draw = |extra: f32| {
            let mut frame = Frame::default();
            frame.clear(grid(CELL, CELL), CaretStyle::default());
            frame.set_dock_rows(4);
            frame.push_dock(bg_cell(0, 0, blue));
            frame.push_dock(bg_cell(0, 2, blue));
            frame.push_dock(bg_cell(0, 3, red));
            frame.set_dock_band(EDGE as f32, extra);
            frame.open_dock(green, WHITE);
            render_offscreen(&r, EDGE, ACCENT, &frame)
        };

        // Dinlenen bant: üç satır da yerinde, zemin yerleşimi kaplıyor.
        let pixels = draw(2.0);
        let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
        assert_eq!(pixel(2, 20), (0, 0, 255), "ilk giriş satırı çizilmedi");
        assert_eq!(pixel(2, 36), (0, 0, 255), "son giriş satırı yerinde değil");
        assert_eq!(pixel(2, 44), (255, 0, 0), "bağlam satırı dipte değil");
        assert_eq!(pixel(2, 28), (0, 255, 0), "zemin giriş bloğunu kaplamadı");

        // Büyümenin başı: bant PTY payının boyunda. Bağlam satırı ve son giriş
        // satırı **aynı pikselde** (dibe yaslı), bandın üstündeki satır yok.
        let pixels = draw(0.0);
        let pixel = |x: usize, y: usize| pixel_at(&pixels, EDGE, x, y);
        assert_eq!(pixel(2, 44), (255, 0, 0), "bağlam satırı bantla kaydı");
        assert_eq!(pixel(2, 36), (0, 0, 255), "son giriş satırı bantla kaydı");
        assert_ne!(
            pixel(2, 20),
            (0, 0, 255),
            "bandın üstüne taşan satır çizildi"
        );
        assert_ne!(pixel(2, 28), (0, 255, 0), "zemin bandın üstüne taştı");
        // Bandın tepesi saç çizgisi: 48 − 16 = 32.
        assert_eq!(
            pixel(20, 32),
            (255, 255, 255),
            "üst saç çizgisi bandla yükselmedi"
        );
    }

    #[test]
    fn the_dock_draws_glyphs_and_its_own_caret() {
        // Dock'un ikinci pipeline'ı: glyph'ler ve caret. Caret'in dikdörtgeni
        // fragment'in `[[position]]`'ı ile karşılaştırılıyor ve o koordinat
        // viewport dönüşümünden **sonraki**, oysa dock listeleri dock-yerel —
        // ikisini `Frame::dock_caret(origin_y)` birleştiriyor. Kayma unutulsaydı
        // caret'in altındaki harf **ızgarada**, dock'un üstünde bir satırda
        // zemin rengine boyanırdı: `make hepsi`'yi yeşil bırakan, gözle
        // "bir hücre görünmez oldu" diye fark edilen bir kusur.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        // Izgaranın son satırı dock'un üstünde kalıyor; dock **tek** satır ve
        // dokunun dibinde.
        frame.push_dock(glyph_cell(0, 'M', Some(red)));
        // Caret **tek** ve pencere uzayında: dock bandının tepesini verip onu
        // bandın ilk satırına koyuyoruz. Yuvayı `Frame` seçiyor
        // (`Frame::push_caret`) ve encode onu dock'un zemininden sonra
        // çiziyor — bu sınamanın gördüğü piksel tam da o sıranın tanığı.
        let dock_top = (EDGE - usize::from(ch)) as f32;
        frame.set_dock_top(dock_top);
        frame.push_caret(
            [0.0, dock_top / f32::from(ch)],
            BACKGROUND,
            WHITE,
            1.0,
            CaretShape::Block,
            true,
        );
        frame.set_dock_rows(1);
        frame.open_dock(red, WHITE);

        let pixels = render_offscreen(&r, EDGE, ACCENT, &frame);
        // Dock dokunun **dibine** yaslı, tepeden hücre sayarak değil: ofset
        // `yükseklik − dock payı`. Tepeden sayılsaydı ızgaranın son satırı ile
        // dock arasında kalan artık şerit (hücre boyuna bölünmeden artan
        // piksel) kadar kayardı ve bant yanlış yeri okurdu.
        let top = EDGE - usize::from(ch);
        let cell: Vec<_> = (0..usize::from(ch))
            .flat_map(|y| (0..usize::from(cw)).map(move |x| (x, y)))
            .map(|(x, y)| pixel_at(&pixels, EDGE, x, top + y))
            .collect();

        // İki iddia ve ikincisi **tam olarak kaymanın bekçisi**: caret opak
        // beyaz bloğunu dock'un satırına çiziyor, altındaki `M` ise zemin
        // rengine boyanıyor. Kayma unutulsaydı dikdörtgen dock-yerel kalır,
        // yani ızgaranın ilk satırıyla karşılaştırılırdı: blok yine burada
        // çizilirdi (o instance viewport'tan geçiyor) ama harf kendi ön
        // planıyla, yani **beyaz** çizilirdi ve hücre beyazla tekdüze kalırdı.
        // Harf kaybolur, hiçbir sayaç görmezdi.
        const WHITE_PX: (u8, u8, u8) = (0xff, 0xff, 0xff);
        assert!(
            cell.contains(&WHITE_PX),
            "caret bloğu dock'un satırında çizilmedi: {cell:?}"
        );
        // Tam bayt aranmıyor: glyph kapsaması kenarlarda yarım ve en koyu
        // piksel bile zemine ancak yaklaşıyor (`glyph_differs_from_cell_background`
        // ile aynı gerekçe — kapı sistem fontunun sürümüne rehin olmamalı).
        // Sorulan şey farkın **yönü**: zemin (siyah) beyazdan koyu.
        let darkest = cell.iter().map(|p| p.0).min().expect("hücre boş değil");
        assert!(
            darkest < 0x80,
            "caret'in altındaki glyph zemin rengine boyanmadı: {cell:?}"
        );
    }

    #[test]
    fn glyph_differs_from_cell_background() {
        // `make duman`'ın `glif=G` jetonu bir CPU sayacı: atlas boş ve glyph
        // pipeline'ı hiç çizmese bile G > 0 basardı. GPU tarafını kanıtlayan
        // yer burası — ve **tam bayt aranmıyor**: hücrenin içi arka planıyla
        // tekdüze DEĞİL, o kadar. Baytlar aransaydı kapı sistem fontunun
        // sürümüne rehin olurdu.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        // Ölçek açıkça söyleniyor: atlasın anahtarı pencereden gelir, bu
        // sınamanın penceresi yok ve `Renderer` atlası `None` doğuyor.
        // Söylenmeseydi kare `GpuError::NoAtlas` ile düşerdi — sessizce @1x
        // çizmek yerine. Hücre boyutu da atlasınkiyle aynı olsun ki yuva
        // dörtlüye birebir otursun.
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        // Saf kırmızı arka plan üstüne beyaz `M` ve `.`: ikisi de doygun,
        // aradaki fark kapsama neyse o.
        //
        // **İki glyph şart**, `cell_bg`'nin iki instance'ıyla aynı gerekçe:
        // tek glyph'le uv aritmetiğinin bozulması GÖRÜNMEZ. uv0 yuva 0'a
        // çakılı kalsaydı (ör. `slot_origin` yoksayılsa) shader rezident tofu
        // kutusunu örnekler ve "arka plan var + farklı piksel var + ön plan
        // rengi taşıyor" iddialarının üçü de geçerdi. İki farklı yuvanın iki
        // farklı şey çizdiğini sormak o kapıyı kapatıyor: `M` hücreyi
        // doldurur, `.` yalnız tabanına küçük bir nokta koyar.
        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        for (col, glyph) in [(0u16, 'M'), (1, '.')] {
            frame.push(glyph_cell(col, glyph, Some(red)));
        }
        assert_eq!(frame.glyph_count(), 2);

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let (m, dot) = (
            cell_rows(&pixels, EDGE, (cw, ch), 0).concat(),
            cell_rows(&pixels, EDGE, (cw, ch), 1).concat(),
        );

        // Dört iddia, dört ayrı hata: arka plan hâlâ görünür (glyph dörtlüsü
        // hücreyi tümden boyamadı), en az bir piksel ondan farklı (glyph
        // gerçekten çizildi), farkın yönü ön plan rengi (kırmızının yeşili
        // yok, beyazınki var — yani fark `rgba`'dan geliyor, rastgele bir
        // çöpten değil) ve iki yuva iki farklı şey çiziyor (uv aritmetiği).
        let bg = (0xff, 0x00, 0x00);
        assert!(
            m.contains(&bg),
            "hücrenin içinde hiç arka plan kalmadı: {m:?}"
        );
        assert!(
            m.iter().any(|&p| p != bg),
            "hücrenin içi arka planla tekdüze: glyph çizilmedi"
        );
        assert!(
            m.iter().any(|&(_, g, _)| g > 0),
            "farklı piksel var ama ön plan rengi taşımıyor"
        );
        assert_ne!(m, dot, "iki yuva aynı şeyi çizdi: uv yuvaya bağlı değil");

        // Alfa: fragment ön çarpımsız veriyor ve blend'in **alfa** çarpanı
        // `One` olmak zorunda. `SourceAlpha` olsaydı yarı kapsamalı kenarda
        // hedefin alfası 1'den düşerdi; `CAMetalLayer` opak olmadığı için
        // compositor o deliği onurlandırır ve harf kenarından pencerenin
        // arkası sızardı. Hiçbir renk iddiası bunu göremez.
        let alphas: Vec<u8> = (0..usize::from(ch))
            .flat_map(|y| (0..usize::from(cw)).map(move |x| (x, y)))
            .map(|(x, y)| pixels[(y * EDGE + x) * 4 + 3])
            .collect();
        assert!(
            alphas.iter().all(|&a| a == 0xff),
            "glyph kenarında alfa deliği: {alphas:?}"
        );
    }

    #[test]
    fn atlas_occupancy_is_republished() {
        // `bt-shell`'in `bt-atlas` kenarı yok ve olmamalı; doluluk
        // `cell_metrics` deseniyle buradan geçiyor.
        //
        // Ölçüt **artış**, eşitlik değil: `Atlas::occupancy()`'yi burada
        // ikinci kez çağırmak, sınanan fonksiyonun gövdesini sınamanın içine
        // kopyalamak olurdu ve o `assert_eq!` hiçbir koşulda düşemezdi. Sabit
        // bir çift döndüren bir `atlas_occupancy` de onu geçerdi. İki farklı
        // glyph'in yuva sayısını **birer birer** artırması, değerin gerçekten
        // atlasın kendisinden geldiğini söylüyor.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        // Atlas ilk metrik sorusunda doğuyor; ondan önce doluluk (0, 0).
        assert_eq!(r.atlas_occupancy(), (0, 0), "atlas metrik sorulmadan yok");
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);
        // Taze atlas **boş değil**: rezident tofu yuvası zaten açılmış
        // ([`TOFU`]). Taban bu yüzden okunuyor, sıfır varsayılmıyor.
        let base = r.atlas_occupancy();
        assert!(base.1 > 0, "kapasite sıfır olamaz: {base:?}");
        assert!(base.0 < base.1, "taban kapasiteyi doldurmamalı: {base:?}");

        let mut used = base.0;
        for (col, glyph) in [(0u16, 'M'), (1, '.')] {
            let mut frame = Frame::default();
            frame.clear(grid(cw, ch), CaretStyle::default());
            frame.push(glyph_cell(col, glyph, None));
            render_offscreen(&r, EDGE, BACKGROUND, &frame);

            let now = r.atlas_occupancy();
            assert_eq!(now.0, used + 1, "{glyph:?} bir yuva açmalıydı: {now:?}");
            assert_eq!(now.1, base.1, "kapasite oynamamalı: {now:?}");
            used = now.0;
        }
    }

    #[test]
    fn rule_band_is_not_uniform_along_x() {
        // Kıvrım **bitmap'inin** dalga olduğunu `bt-atlas` kanıtlıyor
        // (`curl_is_really_a_wave`, GPU'suz). Buranın kanıtladığı, o dalganın
        // **GPU yolundan sağ çıktığı**: doğru yuvanın uv'siyle, `cell`
        // pipeline'ından, kural listesinin kendi geçişinde. Yuvayı sabitleyen
        // ya da kuralı düz çizgi olarak çizen bir kod ikisinin arasında
        // kaybolurdu — `kural=R` sayacı stil ayrımını göremiyor
        // (bkz. `Frame::rule_count`).
        //
        // **Tam bayt aranmıyor**: iddia "bandın satırı x boyunca tekdüze
        // değil". Baytlar aransaydı kapı fontun `underline_px`'ine ve
        // `CURL_FACTOR`'a rehin olurdu.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        // Düz çizgi **kontrol**: tek başına "bir satır tekdüze değil" iddiası
        // hücreyi çöple dolduran bir kodda da geçerdi. İkisi birlikte "kıvrım
        // dalgalı **ve** düz çizgi düz" diyor.
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.push(rule_cell(0, UnderlineStyle::Single));
        frame.push(rule_cell(1, UnderlineStyle::Curl));
        assert_eq!(frame.rule_count(), 2);
        assert_eq!(frame.glyph_count(), 0, "kural hücresi mürekkep üretmez");

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let uniform = |row: &Vec<(u8, u8, u8)>| row.iter().all(|p| *p == row[0]);
        let single = cell_rows(&pixels, EDGE, (cw, ch), 0);
        let curl = cell_rows(&pixels, EDGE, (cw, ch), 1);

        let clear = single[0][0];
        assert!(
            single.iter().flatten().any(|&p| p != clear),
            "düz alt çizgi hiç çizilmedi: {single:?}"
        );
        assert!(
            single.iter().all(uniform),
            "düz çizginin bandı x boyunca tekdüze değil: {single:?}"
        );
        assert!(
            curl.iter().any(|row| !uniform(row)),
            "kıvrımın hiçbir satırı x boyunca değişmiyor: dalga düz çizgiye düşmüş"
        );
    }

    #[test]
    fn sgr58_color_differs_from_foreground() {
        // SGR 58 `bt-core`'dan `Cell::underline_color` olarak geliyor ve
        // `Frame::push` onu `fg`'nin **yerine** koyuyor. Düşerse belirti
        // sessiz: çizgi çizilir, yalnız rengi yanlış olur ve sayaç oynamaz.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        // Sol hücre kontrol: aynı çizgi, SGR 58 **yok** → ön plan rengi.
        frame.push(rule_cell(0, UnderlineStyle::Single));
        frame.push(Cell {
            underline_color: Some(LinearRgba::from_srgb(0xff, 0x00, 0x00)),
            ..rule_cell(1, UnderlineStyle::Single)
        });

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let plain = cell_rows(&pixels, EDGE, (cw, ch), 0).concat();
        let colored = cell_rows(&pixels, EDGE, (cw, ch), 1).concat();

        // Tam kaplanan satır ön planı birebir veriyor: kapsama 1 → blend
        // kaynağı olduğu gibi yazıyor.
        let fg = (0xff, 0xff, 0xff);
        assert!(
            plain.contains(&fg),
            "SGR 58'siz kural ön plan rengiyle çizilmedi: {plain:?}"
        );
        assert!(
            !colored.contains(&fg),
            "SGR 58'li kuralın pikselleri ön plan rengini taşıyor: {colored:?}"
        );
        // Yön de sorulmalı: "farklı" tek başına çizilmemiş bir kuralda da
        // doğrudur. Kırmızı baskın bir piksel rengin `underline_color`'dan
        // geldiğini söylüyor.
        //
        // Karşılaştırma `u16`'da: `u8` olsaydı açık bir clear rengi (ya da
        // yeşil/mavi bir kural) `+ 64`'te taşar ve sınama yanlış pikseli
        // gösteren bir assert yerine "attempt to add with overflow" ile
        // ölürdü — `make hepsi` sınamaları debug koşuyor.
        assert!(
            colored
                .iter()
                .any(|&(red, green, blue)| { u16::from(red) > u16::from(green.max(blue)) + 64 }),
            "SGR 58'li kuralda kırmızı baskın piksel yok: {colored:?}"
        );
    }

    #[test]
    fn bold_and_regular_draw_differently() {
        // `(bold, italic)` → `Face` çevirisi `bt-gpu`'nun tek yeri ve sessizce
        // `Face::Regular` dönen bir hâli hiçbir sayaç göremez: `glif=G` aynı,
        // `bt-core`'un bayrağı aynı, atlas yuvayı yine verir. Aynı karakterin
        // iki yüzde iki farklı piksel kümesi vermesi tek kanıt.
        //
        // Bu sınama fontun **kalın yüzü taşımasına** dayanıyor. Taşımıyorsa
        // `Faces::effective` düz yüze çöker, iki hücre birebir aynı çizilir ve
        // sınama kırmızı düşer — yanlış bir yeşil vermez.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        for (col, bold) in [(0u16, false), (1, true)] {
            frame.push(Cell {
                col,
                row: 0,
                ch: Some('M'),
                fg: WHITE,
                bold,
                ..Default::default()
            });
        }

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let plain = cell_rows(&pixels, EDGE, (cw, ch), 0).concat();
        let bold = cell_rows(&pixels, EDGE, (cw, ch), 1).concat();
        assert_ne!(plain, bold, "kalın `M` düz `M` ile aynı çizildi");
    }

    /// Görünür imleç; blok altındaki metin rengi çağrıda söyleniyor çünkü her
    /// sınama onu ayrı bir iddia için seçiyor.
    fn cursor_at(col: u16, text: LinearRgba) -> Cursor {
        Cursor {
            next_tick: None,
            col,
            row: 0,
            visible: true,
            // Bu sınamalar pikseli soruyor; devir `link`'in sorusu.
            caret_in_dock: false,
            input_rows: 1,
            shape: CaretShape::Block,
            blink: false,
            text,
            // Kaydırma kararı hareketin işi (`motion.rs`); burada çizilen
            // piksel sorgulanıyor ve konum zaten `push_settled` ile hedefin
            // kendisi.
            display_offset: 0,
            // Öteleme `set_origin_rows`'un işi ve bu iki alan onun girdisi;
            // orijini konu eden sınamalar (`content_sticks_to_the_bottom_*`)
            // onu doğrudan söylüyor. Dolu ızgara, yani öteleme sıfır.
            content_rows: 1,
            // Doldurmanın çizimi phase-3'ün işi (017); bu sınamalar onu henüz
            // tüketmiyor.
            fill: 0,
            // Kesirli kaydırma (027) de bu listeyi ilgilendirmiyor: tam
            // satırda, tepe satırı yok.
            top_row: 0,
            scrolled: 0,
            scroll_frac: 0.0,
            scroll_generation: 0,
            rows: 1,
        }
    }

    /// İmleci **kendi** hücresine çizer: bu sınamaların hepsi yerleşmiş bloğa
    /// bakıyor, ara konuma değil (onun sınaması `frame.rs`'te).
    fn push_settled(frame: &mut Frame, cursor: Cursor, rgba: LinearRgba) {
        if cursor.visible {
            frame.push_caret(
                [f32::from(cursor.col), f32::from(cursor.row)],
                cursor.text,
                rgba,
                1.0,
                cursor.shape,
                true,
            );
        }
    }

    #[test]
    fn glyph_under_the_cursor_takes_the_cursor_text_color() {
        // `bt-core`'dan inen iddia (`char_under_cursor_is_drawn_inverted`),
        // artık piksel üstünden: blok altındaki harf `Cursor::text` ile
        // çiziliyor, kendi ön planıyla değil.
        //
        // Ölçüt **eşitlik**, "farklı" değil: imleç hücresi (A), aynı harfin
        // metin rengiyle ve blok renkli bir arka planın üstüne çizilmiş
        // hâliyle (B) **bit bit** aynı olmalı. İkisi aynı iki geçişten aynı
        // parametrelerle geçiyor, yani eşitlik meşru bir talep — ve kapsamanın
        // yarım olduğu kenar piksellerini de kapsıyor: alfa yolu ezilseydi
        // (RGB yerine RGBA yazılsaydı) kenarlar ayrışırdı.
        //
        // C kolu negatif kontrol: imleçsiz, kendi ön planıyla çizilmiş aynı
        // harf. A == C olsaydı "eziliyor" iddiası boş olurdu.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 3);

        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        // A: imlecin altında, harfin kendi ön planı beyaz.
        frame.push(glyph_cell(0, 'M', None));
        // B: imleçsiz ama harf zaten metin renginde, arka planı blok rengi.
        frame.push(Cell {
            fg: BACKGROUND,
            ..glyph_cell(1, 'M', Some(ACCENT))
        });
        // C: imleçsiz, kendi ön planıyla, aynı blok renkli zeminin üstünde.
        frame.push(glyph_cell(2, 'M', Some(ACCENT)));
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        // **İddia ikiye ayrıldı, toleransa çevrilmedi** (015 phase-2): caret'in
        // köşesi artık yuvarlak, yani köşe pikselleri bloğun rengi değil ve
        // oradan geçen bir eşitlik yuvarlaklığı sınardı. Gövdede eşitlik hâlâ
        // **bit bit**; köşenin ve halenin kendi bekçileri ayrı
        // ([`the_caret_corner_is_rounded`], [`the_caret_glow_spills_but_stops`]).
        let inset = caret_radius_px((cw, ch), bt_core::CURSOR_RADIUS as f32);
        let cell = |col| cell_body(&pixels, EDGE, (cw, ch), col, inset);
        let (a, b, c) = (cell(0), cell(1), cell(2));

        assert_eq!(
            a, b,
            "imleç altındaki harf metin rengiyle çizilmedi (A ≠ B)"
        );
        assert_ne!(a, c, "imleç dikdörtgeni harfin rengini hiç ezmedi (A = C)");
    }

    #[test]
    fn a_degenerate_caret_shape_paints_the_old_rectangle() {
        // **Geri alma yolunun bekçisi** (R8): "yarıçap 0, hale 0" desteklenen
        // ve sınanan bir hâl olmalı, yani çıktısı 014'ün düz dörtgeniyle bit
        // bit aynı. Ancak GPU söyleyebilir — dejenere kolda fragment `step`,
        // açık kolda `smoothstep` kullanıyor ve ikisinin kenar pikselleri
        // ayrışır. `smoothstep` o kola sızarsa burası kızarır.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        let mut frame = Frame::default();
        // **Dejenere kol artık ayardan sürülüyor** (016 R6): `cursor_radius = 0`
        // ve `cursor_glow = 0` desteklenen bir kullanıcı ayarı, yani geri alma
        // yolu bir sınama kancası değil **gerçek yol**.
        frame.clear(
            grid(cw, ch),
            CaretStyle {
                radius_ratio: 0.0,
                glow: 0.0,
                ..CaretStyle::default()
            },
        );
        // A: caret, dejenere şekille. B: aynı rengin düz arka planı — yani
        // caret'in kendi pipeline'ından önceki hâli.
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        frame.push(bg_cell(1, 0, ACCENT));

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let cell = |col| cell_rows(&pixels, EDGE, (cw, ch), col).concat();
        assert_eq!(
            cell(0),
            cell(1),
            "dejenere caret düz dörtgenden ayrıştı: geri alma yolu bozuk"
        );
    }

    #[test]
    fn the_caret_corner_is_rounded() {
        // Yarıçapın kendi bekçisi. `glyph_under_the_cursor_...` köşeleri
        // bilerek dışarıda bırakıyor (orta bant); yuvarlaklığın **gerçekten**
        // olduğunu söyleyen tek yer burası.
        //
        // **Referans caret'in DIŞINDAN** (`/code-review`): önceki hâli
        // karşılaştırmayı caret'in kendi karşı köşesiyle yapıyordu ve SDF
        // simetrik olduğu için iki köşenin `d`'si her zaman eşit — iddia
        // hiçbir yarıçap değerinde düşemeyen bir totolojiydi. Ölçü de artık
        // "eşit/farklı" değil **boyanma oranı**: köşe merkezden belirgin
        // biçimde daha sönük olmalı.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

        let mut frame = Frame::default();
        // **Yarıçap ayardan sürülüyor** (016 R6), ezmeden: `cursor_radius`
        // artık kullanıcı anahtarı ve bekçinin gerçek yolu geçmesi gerekiyor.
        // Oran üretim varsayılanından değil **açıkça** veriliyor — varsayılan
        // bir zevk sayısı ve 1x hücrede ~1.6 px'e denk geliyor, yani köşe
        // pikselinin çoğu hâlâ boyalı olurdu ve eşik hücre boyuna göre
        // kızardı.
        frame.clear(
            grid(cw, ch),
            CaretStyle {
                // Dar kenarın yarısı: blok bir stadyuma dönüyor.
                radius_ratio: 0.5,
                glow: 0.0,
                ..CaretStyle::default()
            },
        );
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);

        let sum = |x, y| brightness(&pixels, EDGE, x, y);
        let clear = sum(EDGE - 1, EDGE - 1);
        let middle = sum(usize::from(cw) / 2, usize::from(ch) / 2);
        assert!(middle > clear, "caret'in ortası boyanmamış");
        assert_eq!(sum(0, 0), clear, "köşe boyalı: shader yuvarlamıyor");
    }

    #[test]
    fn a_hollow_caret_paints_only_its_edge() {
        // **Kenar kolu ölü sevk edilmesin** (`/code-review`). `caret_shape()`
        // `stroke`'u bu sürümde sabit 0 veriyor, yani shader'ın `stroke > 0`
        // dalı hiç koşmamış olurdu ve phase-3 onu "zaten yazılmış ve geçmiş"
        // sanarak açardı. Sınama o dalı **şimdi** sürüyor.
        //
        // İkinci iş: `body -= inner` çıkarması kalın bir kenarda gövdeyi
        // tümden sıfırlayabilir. Kenar burada hücrenin dörtte biri, yani
        // ortası gerçekten boş kalmalı ama caret görünmez olmamalı.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        // Yarıçap ve hale kapalı; sınanan tek şey kenar bandı.
        let stroke = (f32::from(cw) / 4.0).max(1.0);
        frame.force_caret_sdf([0.0, stroke, 0.0, 0.0]);
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);

        let sum = |x, y| brightness(&pixels, EDGE, x, y);
        let clear = sum(EDGE - 1, EDGE - 1);
        let edge = sum(0, usize::from(ch) / 2);
        let middle = sum(usize::from(cw) / 2, usize::from(ch) / 2);
        assert!(edge > clear, "içi boş caret'in kenarı da çizilmedi");
        assert_eq!(middle, clear, "içi boş caret'in ortası boyalı");
    }

    #[test]
    fn the_caret_glow_spills_but_stops() {
        // **Hale dikdörtgenin DIŞINDA örnekleniyor** (R6): içeriden bakan bir
        // sınama haleyi göremez, çünkü orada gövde zaten opak.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        // Pay sınamada **üretimdekinden geniş** (varsayılan ~8): hale
        // söndükçe (0.35 → 0.10) 8 bitlik hedefte fark kuantalamaya
        // gömülüyor ve bekçi körleşiyor. Geniş pay örneklenen noktayı
        // halenin tepesine yaklaştırıyor; sınanan şey oran, mutlak piksel değil.
        const GUTTER: u16 = 16;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

        let mut frame = Frame::default();
        frame.clear(grid_with_gutter(cw, ch, GUTTER), CaretStyle::default());
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);

        // Caret payın sağından başlıyor: x ∈ [GUTTER, GUTTER + cw].
        let right = usize::from(GUTTER) + usize::from(cw);
        // Pay üretimdeki türetmenin **aynısı**: oran değişince bekçi de
        // kayar, yoksa hale küçülünce sınama boş bir noktaya bakardı.
        let pad = (f32::from(GUTTER) * crate::frame::CARET_GLOW_RATIO) as usize;
        let y = usize::from(ch) / 2;
        assert!(
            right + pad + 2 < EDGE,
            "örnekleme noktaları dokuya sığmıyor"
        );

        // Referans **uzaktan**: halenin ulaşamayacağı köşe. Sınırın hemen
        // ötesini referans almak dairesel olurdu — o nokta zaten sınanan şey.
        let clear = pixel_at(&pixels, EDGE, EDGE - 1, EDGE - 1);
        let inside_glow = pixel_at(&pixels, EDGE, right + pad / 2, y);
        assert_ne!(inside_glow, clear, "dikdörtgenin dışında hale yok");
        assert_eq!(
            pixel_at(&pixels, EDGE, right + pad + 2, y),
            clear,
            "hale payın ötesinde de boyuyor: sınırsız"
        );
    }

    #[test]
    fn the_glow_setting_reaches_the_pixels() {
        // **Ayarın gerçekten indiğinin tek kanıtı.** `cursor_glow` payı da
        // alfayı da ölçekliyor (tek his, iki sayı değil), yani kapatmak
        // dikdörtgenin dışını zemine döndürmeli ve açmak parlatmalı.
        // `Settings` sınamaları değerin **okunduğunu** gösteriyor, boyandığını
        // yalnız burası gösterir.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        const GUTTER: u16 = 16;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 1);
        let y = usize::from(ch) / 2;
        let at = usize::from(GUTTER) + usize::from(cw) + 2;
        assert!(at < EDGE, "örnekleme noktası dokuya sığmıyor");

        let sample = |glow: f64| {
            let mut frame = Frame::default();
            frame.clear(
                grid_with_gutter(cw, ch, GUTTER),
                CaretStyle {
                    radius_ratio: 0.0,
                    glow,
                    ..CaretStyle::default()
                },
            );
            push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);
            let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
            brightness(&pixels, EDGE, at, y)
        };

        let (off, on, strong) = (sample(0.0), sample(1.0), sample(2.0));
        assert!(
            off < on && on < strong,
            "gölge ayarı piksele inmiyor: {off} / {on} / {strong}"
        );
    }

    #[test]
    fn the_caret_glow_fades_with_the_caret() {
        // Hale caret'in kendi alfasıyla **çarpılıyor**, yani blink sönerken
        // hale de sönüyor (R6). Bekçi yine **dışarıdan** örnekliyor:
        // `cursor_alpha_is_blended_on_the_gpu` yalnız caret'in kendi hücresine
        // bakıyor ve bu belirtiyi göremez.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        // Geniş pay: kardeş bekçiyle aynı gerekçe (kuantalama).
        const GUTTER: u16 = 16;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 1);
        let y = usize::from(ch) / 2;
        let pad = (f32::from(GUTTER) * crate::frame::CARET_GLOW_RATIO) as usize;
        let at = usize::from(GUTTER) + usize::from(cw) + pad / 2;
        // `pixel_at` x'i sınırlamıyor: taşan bir indeks panik değil **bir alt
        // satırın** pikselini okur, yani sınama sessizce yanlış iddia eder
        // (`/code-review`). `fitting_cell_px` payı hiç görmüyor.
        assert!(at < EDGE, "örnekleme noktası dokuya sığmıyor");

        let sample = |alpha: f32| {
            let mut frame = Frame::default();
            frame.clear(grid_with_gutter(cw, ch, GUTTER), CaretStyle::default());
            frame.push_caret(
                [0.0, 0.0],
                BACKGROUND,
                ACCENT,
                alpha,
                CaretShape::Block,
                true,
            );
            let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
            brightness(&pixels, EDGE, at, y)
        };

        // Sönük caret hiç çizilmiyor, yani o noktada clear rengi kalıyor;
        // sıralama üç uçta da kesin ve renk tablosu gerektirmiyor.
        let (dark, half, full) = (sample(0.0), sample(0.5), sample(1.0));
        assert!(
            dark < half && half < full,
            "hale caret'in alfasını izlemiyor: {dark} / {half} / {full}"
        );
    }

    #[test]
    fn a_hollow_caret_leaves_the_glyph_its_own_color() {
        // **`glyph_under_the_cursor_takes_the_cursor_text_color`'ın odaksız
        // kardeşi ve tersini söylüyor.** Ters çevirme boyanan zemine
        // dayanıyor: içi boş caret'in ortasında boyanmış bir şey yok, yani
        // harf kendi ön planıyla kalmalı. Kalmasaydı zemin renginde çizilir
        // ve **görünmez** olurdu — içi boş caret metni yutardı.
        //
        // Örnekleme hücrenin **içi**: kenar bandı (`rule_px`) caret'in
        // kendisi ve orada eşitlik beklenmiyor.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        // A: odaksız caret'in altında. C: caret yok, aynı harf ve zemin.
        frame.push(glyph_cell(0, 'M', None));
        frame.push(glyph_cell(1, 'M', None));
        frame.push_caret(
            [0.0, 0.0],
            BACKGROUND,
            ACCENT,
            1.0,
            CaretShape::Block,
            false,
        );

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        // **İçeri çekme üretimden türüyor**, sabit değil (`/code-review`):
        // halkanın kalınlığı `rule_px` ve köşeyi yarıçap yiyor; sabit bir 3
        // ya büyük puntoda halkayı örneklemin içine alır ya da dar hücrede
        // aralığı boşaltıp hiçbir şey iddia etmeyen bir eşitliğe düşerdi.
        let inset = caret_radius_px((cw, ch), bt_core::CURSOR_RADIUS as f32)
            + usize::from(r.cell_metrics(1.0).rule_px()).max(1);
        assert!(
            inset * 2 < usize::from(cw).min(usize::from(ch)),
            "içeri çekme hücreyi yuttu"
        );
        let interior = |col: usize| {
            let (cwu, chu) = (usize::from(cw), usize::from(ch));
            (inset..chu - inset)
                .flat_map(|y| (inset..cwu - inset).map(move |x| (x, y)))
                .map(|(x, y)| pixel_at(&pixels, EDGE, col * cwu + x, y))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            interior(0),
            interior(1),
            "içi boş caret harfin rengini ezdi: metin görünmez olur"
        );
    }

    #[test]
    fn an_unfocused_caret_paints_a_ring_through_the_production_path() {
        // Halkanın **üretim yolundan** bekçisi: `a_hollow_caret_paints_only_
        // its_edge` shader'ın kolunu `force_caret_sdf` ile sürüyor, burada
        // kenarı açan şey odağın kendisi (`push_caret(.., focused=false)`).
        // İkisi bir arada olmasa "kol çalışıyor ama odak onu hiç açmıyor"
        // hâli sessiz kalırdı.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.push_caret(
            [0.0, 0.0],
            BACKGROUND,
            ACCENT,
            1.0,
            CaretShape::Block,
            false,
        );
        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);

        let sum = |x, y| brightness(&pixels, EDGE, x, y);
        let clear = sum(EDGE - 1, EDGE - 1);
        assert!(
            sum(0, usize::from(ch) / 2) > clear,
            "odaksız caret'in kenarı çizilmedi"
        );
        assert_eq!(
            sum(usize::from(cw) / 2, usize::from(ch) / 2),
            clear,
            "odaksız caret'in ortası dolu"
        );
    }

    #[test]
    fn cursor_alpha_is_blended_on_the_gpu() {
        // Hareketi Azalt'ın belirmesi (008 phase-5) **GPU'da** karışıyor:
        // `cell_bg` pipeline'ı bu phase'de harmanlı oldu ve `cell` fragment'i
        // ezme yerine `mix` yapıyor. `frame.rs`'in sayacı alfanın listeye
        // yazıldığını gösteriyor ama boyandığını gösteremez — depo kuralının
        // ("CPU sayacı GPU'nun boyadığını kanıtlamaz") tam karşılığı.
        //
        // Ölçüt iki uçta **eşitlik**, ortada **sıra**: renk beklentisini
        // hesaplamak lineer karışımı sRGB'ye kodlamak olurdu ve o tablo
        // burada yok. Uçlar zaten daha keskin bir iddia — alfa hiç
        // okunmasaydı üç kare de aynı çıkardı.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

        // Tek hücre, tek imleç: uniform kare başına **tek** değer, yani üç
        // opaklık üç ayrı kare demek.
        let render = |alpha: Option<f32>| {
            let mut frame = Frame::default();
            frame.clear(grid(cw, ch), CaretStyle::default());
            frame.push(glyph_cell(0, 'M', None));
            if let Some(alpha) = alpha {
                frame.push_caret(
                    [0.0, 0.0],
                    BACKGROUND,
                    ACCENT,
                    alpha,
                    CaretShape::Block,
                    true,
                );
            }
            cell_rows(
                &render_offscreen(&r, EDGE, BACKGROUND, &frame),
                EDGE,
                (cw, ch),
                0,
            )
            .concat()
        };
        let (none, clear, half, opaque) = (
            render(None),
            render(Some(0.0)),
            render(Some(0.5)),
            render(Some(1.0)),
        );

        // Alfa sıfır = imleç **hiç yok**: blok da altındaki harfin rengi de
        // dokunulmadan kalmalı. Harmanlama kapalı olsaydı burası opak bir
        // dikdörtgen olurdu.
        assert_eq!(clear, none, "alfa 0 imleci yine de opak çizdi");
        // Alfa bir = bu phase'den **önceki** hâl: yerleşmiş imleçte görsel
        // sonuç değişmedi.
        assert_ne!(opaque, none, "alfa 1 imleci hiç çizmedi");

        let mut strictly_between = 0usize;
        for (i, (&mid, (&off, &on))) in half.iter().zip(clear.iter().zip(opaque.iter())).enumerate()
        {
            for c in 0..3 {
                let (m, a, b) = (
                    [mid.0, mid.1, mid.2][c],
                    [off.0, off.1, off.2][c],
                    [on.0, on.1, on.2][c],
                );
                let (lo, hi) = (a.min(b), a.max(b));
                assert!(
                    (lo..=hi).contains(&m),
                    "piksel {i} bileşen {c}: {m} iki ucun ({lo}, {hi}) dışında"
                );
                if m > lo && m < hi {
                    strictly_between += 1;
                }
            }
        }
        // Ara kare uçlardan birine **eşit olmamalı**: eşit olsaydı alfa ikili
        // bir bayrak gibi davranıyor, gerçekten karışmıyor olurdu.
        assert!(
            strictly_between > 0,
            "alfa 0.5 iki uçtan birine düştü: karışım yok"
        );
    }

    #[test]
    fn cursor_rect_stops_at_its_own_cell() {
        // Dikdörtgenin **dışı** dokunulmaz kalmalı: ezme hücrenin kendisiyle
        // sınırlı, kareyle değil. Yakaladığı kusur dikdörtgenin **ölçüsü** —
        // hücre yerine kare, ya da hücre yerine iki hücre. Yakalamadığı, `<`
        // ile `<=` farkı: `[[position]]` fragment merkezini veriyor (x + 0.5)
        // ve hiçbir fragment tam sınıra düşmüyor (shader'da yazılı).
        //
        // **Ara konum da soruluyor** (`/audit` bulgusu): hareket geldiğinden
        // beri `at` kesirli olabiliyor ve "kendiliğinden sorulur" diye
        // bırakılan kol aslında hiç koşmuyordu — iki offscreen sınaması da
        // tam sayı konum veriyordu, yani kesirli dikdörtgeni yalnız CPU
        // birim sınamaları görüyordu.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        // Sınırı soran şey **kural**, glyph değil: band hücrenin ilk
        // sütununu da tam kaplıyor, yani "o sütun ezildi mi" sorusu fontun
        // hangi pikseli boyadığına bağlı kalmıyor (glyph'in ilk sütunu boş
        // olabilir ve sınama sessizce hiçbir şey sormaz).
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.push(rule_cell(0, UnderlineStyle::Single));
        frame.push(rule_cell(1, UnderlineStyle::Single));
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        // Komşunun **ilk sütunu**: dikdörtgenin `x1`'i tam oraya düşüyor.
        let first_column: Vec<(u8, u8, u8)> = cell_rows(&pixels, EDGE, (cw, ch), 1)
            .iter()
            .map(|row| row[0])
            .collect();
        assert!(
            first_column.contains(&(0xff, 0xff, 0xff)),
            "komşunun ilk sütunu da ezildi: dikdörtgen hücresinden taşıyor — {first_column:?}"
        );

        // İki hücrenin **ortasında** duran imleç: dikdörtgen artık iki
        // hücreye de taşıyor ve bu doğru — ama genişliği hâlâ bir hücre, yani
        // ikinci hücrenin **son** sütunu dokunulmadan kalmalı. Ölçünün kare
        // ya da iki hücre olduğu bir kusur burada da yakalanır, üstelik
        // kesirli konumda.
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.push(rule_cell(0, UnderlineStyle::Single));
        frame.push(rule_cell(1, UnderlineStyle::Single));
        let mut cursor = cursor_at(0, BACKGROUND);
        cursor.col = 0;
        frame.push_caret([0.5, 0.0], cursor.text, ACCENT, 1.0, cursor.shape, true);

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let last_column: Vec<(u8, u8, u8)> = cell_rows(&pixels, EDGE, (cw, ch), 1)
            .iter()
            .map(|row| row[usize::from(cw) - 1])
            .collect();
        assert!(
            last_column.contains(&(0xff, 0xff, 0xff)),
            "yarım hücre kaymış imleç iki hücreyi birden ezdi: {last_column:?}"
        );
    }

    #[test]
    fn rule_under_the_cursor_takes_the_cursor_text_color() {
        // `bt-core`'dan inen ikinci iddia
        // (`cursor_cell_drops_the_underline_color`), piksel üstünden.
        // Eskiden `frame()` imleç hücresinin SGR 58 rengini **düşürüyordu**;
        // artık hücre rengini koruyor ve dikdörtgen onu piksel olarak eziyor.
        // Sonuç aynı: blok altındaki çizgi de metin rengine dönüyor, yani
        // aynı hücredeki alt çizgi ile üstü çizili aynı davranıyor.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        // İki hücrede aynı SGR 58'li çizgi; imleç yalnız birinde.
        for col in [0, 1] {
            frame.push(Cell {
                underline_color: Some(red),
                ..rule_cell(col, UnderlineStyle::Single)
            });
        }
        push_settled(&mut frame, cursor_at(0, BACKGROUND), ACCENT);

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let under = cell_rows(&pixels, EDGE, (cw, ch), 0).concat();
        let plain = cell_rows(&pixels, EDGE, (cw, ch), 1).concat();

        let srgb = |hex: u32| ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
        let background = srgb(Theme::BATERI.background);
        // Kontrol: imleçsiz hücrede çizgi hâlâ SGR 58'in kırmızısı.
        assert!(
            plain.contains(&(0xff, 0x00, 0x00)),
            "imleçsiz hücrede SGR 58 rengi kayboldu: {plain:?}"
        );
        assert!(
            !under.contains(&(0xff, 0x00, 0x00)),
            "blok altındaki çizgi SGR 58 rengini korudu: {under:?}"
        );
        // Yön de sorulmalı: "kırmızı yok" tek başına hiç çizilmemiş bir
        // kuralda da doğrudur. Tam kaplanan band metin rengini veriyor —
        // ±1 tolerans, çünkü zemin bir **ara ton** ve 8-bit sRGB kodlaması
        // yuvarlama taşır (emsali `cell_bg_paints_pixels_on_the_gpu`).
        assert!(
            under.iter().any(|p| {
                p.0.abs_diff(background.0) <= 1
                    && p.1.abs_diff(background.1) <= 1
                    && p.2.abs_diff(background.2) <= 1
            }),
            "blok altındaki çizgi metin renginde çizilmedi: {under:?}"
        );
    }

    #[test]
    fn rule_over_cursor_stays_visible() {
        // Çizim sırası: arka planlar **ve imleç**, sonra glyph'ler, sonra
        // kurallar. İmleç bloğu opak ve altındaki her şeyi örter; kural ondan
        // sonra gelmezse imlecin üstündeki hücrede alt çizgi kaybolur ve
        // belirti yalnız imlecin durduğu tek hücrede görünür.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 1);

        let red = LinearRgba::from_srgb(0xff, 0x00, 0x00);
        let mut frame = Frame::default();
        frame.clear(grid(cw, ch), CaretStyle::default());
        frame.push(rule_cell(0, UnderlineStyle::Single));
        // Metin rengi bilerek kuralın kendi rengiyle **aynı** (beyaz): bu
        // sınamanın sorduğu şey renk değil **sıra**, ve dikdörtgenin ezmesi
        // onu bulandırmamalı. Rengin ezildiğini soran yer
        // `rule_under_the_cursor_takes_the_cursor_text_color`.
        push_settled(&mut frame, cursor_at(0, WHITE), red);

        let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let cell = cell_rows(&pixels, EDGE, (cw, ch), 0).concat();
        assert!(
            cell.contains(&(0xff, 0xff, 0xff)),
            "imlecin üstündeki kural örtüldü: {cell:?}"
        );
        // İkinci iddia bekçinin diğer yarısı: kural imleci tümden boyamamalı,
        // yoksa "görünür" iddiası imleci silen bir kodda da geçerdi.
        assert!(
            cell.contains(&(0xff, 0x00, 0x00)),
            "kural imleç bloğunun tamamını örttü: {cell:?}"
        );
    }

    #[test]
    fn renderer_without_atlas_refuses_glyphs() {
        // "Önce metriği sor" sözleşmesinin sınanabilir hâli. `Renderer`
        // atlası `None` doğuyor; ölçeği hiç söylemeden glyph çizen bir yol
        // sessizce @1x çizmek yerine kareyi düşürmeli. Sessiz olsaydı belirti
        // "retina makinede harfler yarım boy" olurdu ve hiçbir sınama görmezdi.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 32;
        let texture = target_texture(&r, EDGE);

        let mut frame = Frame::default();
        frame.clear(grid(8, 16), CaretStyle::default());
        frame.push(Cell {
            col: 0,
            row: 0,
            ch: Some('x'),
            fg: ACCENT,
            bg: None,
            ..Default::default()
        });

        let cmd = r.queue.commandBuffer().expect("komut tamponu");
        let result = r.encode_pass(&cmd, &texture, BACKGROUND, &frame);
        assert!(
            matches!(result, Err(GpuError::NoAtlas)),
            "atlassız kare sessizce geçti: {result:?}"
        );
        // Encoder yine de kapandı: hata `?` ile erken dönmüyor, yoksa Metal
        // "released without endEncoding" ile süreci öldürürdü.
        cmd.commit();
        cmd.waitUntilCompleted();
    }

    /// Geniş hücre **iki dörtlü** üretiyor: sol yarı yerinde, sağ yarı bir
    /// hücre sağda ve iki ayrı yuvadan.
    ///
    /// Yelpazeleme `Frame::push`'ta **değil** burada ve sebebi ödünç: "bir
    /// yuva mı iki mi" kararını mürekkep kapısı veriyor, yani `Atlas::slot`
    /// — sink ise atlası hiç görmüyor. Bekçi o kararın çizime gerçekten
    /// döndüğünü gösteriyor.
    #[test]
    fn a_wide_cell_becomes_two_quads() {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let mut tex = AtlasTexture {
            atlas: Atlas::new(None, 13.0, 1.0, 1.0),
            texture: None,
            instances: Vec::new(),
            color_texture: None,
            color_instances: Vec::new(),
            fx_instances: Vec::new(),
        };
        let cell_w = tex.atlas.metrics().cell_px.0;
        // `漢` mürekkebi iki hücre isteyen bir aday veriyor (cascade: PingFang
        // SC); tek hücrelik kapıdan dönüyor, iki hücrelik kapıdan geçiyor.
        let glyphs = [GlyphCell {
            pos: [0.0, 0.0],
            ch: '漢',
            face: Face::Regular,
            size: SizeClass::Normal,
            rgba: [1.0, 1.0, 1.0, 1.0],
            wide: true,
            cluster: None,
        }];
        tex.prepare(&device, &glyphs, &Clusters::default(), &[])
            .expect("prepare");
        assert_eq!(
            tex.instances.len(),
            2,
            "geniş hücre iki dörtlü üretmeli: {:?}",
            tex.instances.len()
        );
        assert_eq!(
            tex.instances[0].pos,
            [0.0, 0.0],
            "sol yarı hücrenin yerinde"
        );
        assert_eq!(
            tex.instances[1].pos,
            [f32::from(cell_w), 0.0],
            "sağ yarı tam bir hücre sağda"
        );
        assert_ne!(
            tex.instances[0].uv0, tex.instances[1].uv0,
            "iki yarı iki ayrı yuvadan okunmalı"
        );
    }

    /// Geniş ilan edilmiş ama mürekkebi bir hücreye sığan karakter **tek**
    /// dörtlü üretiyor.
    ///
    /// Sağına boş bir dörtlü düşmesi bir israf draw'ı olurdu ve ölçülen 65
    /// karakterin ("bugün çalışan çizimler") her birinde ödenirdi. Kararın
    /// sahibi kapı, çağıran değil — bekçi de tam bunu gösteriyor.
    #[test]
    fn a_wide_cell_that_fits_one_cell_stays_one_quad() {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let mut tex = AtlasTexture {
            atlas: Atlas::new(None, 13.0, 1.0, 1.0),
            texture: None,
            instances: Vec::new(),
            color_texture: None,
            color_instances: Vec::new(),
            fx_instances: Vec::new(),
        };
        // Menlo'nun kendi glyph'i, Unicode'a göre iki sütun: taban fontta
        // ilerleme hücrenin ilerlemesinin ta kendisi, yani tek hücre.
        let glyphs = [GlyphCell {
            pos: [0.0, 0.0],
            ch: '☕',
            face: Face::Regular,
            size: SizeClass::Normal,
            rgba: [1.0, 1.0, 1.0, 1.0],
            wide: true,
            cluster: None,
        }];
        tex.prepare(&device, &glyphs, &Clusters::default(), &[])
            .expect("prepare");
        assert_eq!(
            tex.instances.len(),
            1,
            "tek hücreye sığan geniş karakter ikinci dörtlü üretmemeli"
        );
    }

    /// Renk dokusunun formatı **`RGBA8Unorm_sRGB`** ve bu bir zevk değil
    /// sözleşme.
    ///
    /// Düz `RGBA8Unorm` bir doku sessizce yanlış olurdu: donanım örneklerken
    /// sRGB'yi **çözmez**, fragment değerleri lineer sanar ve hedef
    /// (`BGRA8Unorm_sRGB`) yazarken bir kez daha kodlar — palet açar.
    /// `CLAUDE.md` → "Renk uzayı sınırı geçer" maddesindeki sessiz kusurun
    /// aynısı ve doğrudan sorulan tek yer burası.
    #[test]
    fn the_color_plane_is_an_srgb_texture() {
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let texture = new_color_texture(&r.device, 64, 64).expect("renk dokusu");
        assert_eq!(
            texture.pixelFormat(),
            MTLPixelFormat::RGBA8Unorm_sRGB,
            "renk düzlemi sRGB olmalı"
        );
        // Maske düzlemi **değişmedi**: tek kanal kapsama sözleşmesi ayakta.
        let mask = new_atlas_texture(&r.device, 64, 64).expect("maske dokusu");
        assert_eq!(mask.pixelFormat(), MTLPixelFormat::R8Unorm);
    }

    /// **Sentetik ara tonlu tanık:** renk düzlemine yazılan bayt ekrana
    /// **aynı** bayt olarak çıkıyor.
    ///
    /// Tanığın sentetik olması **şart**: gerçek bir emojinin bitmap'i
    /// CoreGraphics'ten geliyor ve macOS sürümleri arasında bit bit sabit
    /// değil, yani ona bakan bir bekçi yanlış güven verir ("bileşimi sına,
    /// bileşeni değil"). Ara ton da şart: `0.00` ve `0xff` sRGB transfer
    /// fonksiyonunun **sabit noktaları**, yani doku formatı yanlış olsa da
    /// aynı baytı verirler — `cell_bg_paints_pixels_on_the_gpu`'nun
    /// `MIDTONE`'u ile birebir aynı gerekçe.
    ///
    /// Aynı sınama **ön çarpımın** da tanığı: alfa `0xff` (tam opak) ve
    /// baytlar ön çarpımlı, yani blend'in RGB kaynak çarpanı `One` ile
    /// `SourceAlpha` bu pikselde **aynı** sonucu verir; ayrıştıkları yer
    /// yarı saydam kenar ve onun bekçisi aşağıda.
    #[test]
    fn a_midtone_color_slot_survives_the_round_trip() {
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 16;
        // Ara ton: `cell_bg_paints_pixels_on_the_gpu`'nun `MIDTONE`'uyla aynı
        // gerekçeden seçildi, değeri onunla aynı olmak zorunda değil.
        const MID: (u8, u8, u8) = (0x80, 0x40, 0xc0);
        let seen = emoji_round_trip(&r, EDGE, MID, 0xff);
        // ±1: 8-bit sRGB kodlaması yuvarlama taşır
        // (`cell_bg_paints_pixels_on_the_gpu` ile aynı sınır).
        assert!(
            seen.0.abs_diff(MID.0) <= 1
                && seen.1.abs_diff(MID.1) <= 1
                && seen.2.abs_diff(MID.2) <= 1,
            "renk düzlemi round-trip: {seen:02x?} ≠ {MID:02x?}"
        );
    }

    /// **Yarı saydam kenarın tanığı:** kompozisyon **lineer** uzayda oluyor.
    ///
    /// Düz alfalı yarı saydam beyaz siyah zeminde çizilince sonuç lineer
    /// uzayda tam yarım, sRGB'ye kodlanınca **0xBC** olmak zorunda. Sayı
    /// gevşek bir "kararmadı" eşiği değil: blend zincirindeki her hata onu
    /// aşağı çekiyor ve en sinsi hâli **0x80** — ön çarpımı sRGB-kodlanmış
    /// uzayda bırakmanın imzası (`raster::unpremultiply`'ın doc'u).
    /// Eşiğin gevşek olduğu bir hâl bu ikisini ayırt edemezdi: set kapısı
    /// (`/code-review`) tam bunu yakaladı — eski eşik `> 0x60` idi ve
    /// karartılmış `0x80`'i geçiriyordu, yani sınamanın yazılı ölçütü ile
    /// iddiası ayrışmıştı.
    #[test]
    fn a_translucent_edge_composites_in_linear_space() {
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 16;
        // Düz alfa: tam beyaz, yarım alfa. `raster::draw_color` dokuya bunu
        // yazıyor (ön çarpımı kendi geri alıyor), yani sentetik yuva da
        // üretimdeki biçimde kuruluyor.
        let seen = emoji_round_trip(&r, EDGE, (0xff, 0xff, 0xff), 0x80);
        // encode(0.502) ≈ 0.7367 → 0xBC. ±2: sRGB kodlaması yuvarlama taşır
        // ve `0x80` (yanlış uzay) bu payın **çok** ötesinde.
        assert!(
            seen.0.abs_diff(0xbc) <= 2,
            "yarı saydam kenar lineer kompozit vermedi: {seen:02x?} ≠ ~0xbc \
             (0x80 civarı ön çarpımın sRGB uzayında kaldığını söyler)"
        );
    }

    /// Renk düzlemine tek bir yuva yazıp `emoji` pipeline'ıyla çizer ve
    /// sonucun ilk pikselini verir.
    ///
    /// Yolun **tamamı** koşuyor: `RGBA8Unorm_sRGB` doku, `cell_vertex`,
    /// `emoji_fragment` ve o pipeline'ın blend'i. `encode_pass`'ten
    /// geçmiyor, çünkü aranan şey sentetik baytlar — gerçek bir font
    /// karışırsa tanık bileşenin değil fontun tanığı olur.
    fn emoji_round_trip(r: &Renderer, edge: usize, rgb: (u8, u8, u8), alpha: u8) -> (u8, u8, u8) {
        let target = target_texture(r, edge);
        let color = new_color_texture(&r.device, edge as u16, edge as u16).expect("renk dokusu");
        // Tek hücrelik yuva: dokunun sol üst köşesine tekdüze bir renk.
        let cell = 8u16;
        let slot: Vec<u8> = (0..usize::from(cell) * usize::from(cell))
            .flat_map(|_| [rgb.0, rgb.1, rgb.2, alpha])
            .collect();
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: usize::from(cell),
                height: usize::from(cell),
                depth: 1,
            },
        };
        // SAFETY: `slot` 4*cell*cell bayt ve çağrı boyunca canlı; bölge
        // dokunun içinde, satır adımı tam genişlik × dört.
        unsafe {
            color.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
                region,
                0,
                NonNull::from(&slot[..]).cast::<c_void>(),
                usize::from(cell) * 4,
            );
        }

        let instance = GlyphInstance {
            pos: [0.0, 0.0],
            uv0: [0.0, 0.0],
            // `emoji_fragment` bunu **okumuyor**; yine de gerçekçi bir değer
            // veriliyor ki bir gün okunmaya başlarsa sınama sessizce
            // değişmesin.
            rgba: [1.0, 1.0, 1.0, 1.0],
        };
        let uv_size = [f32::from(cell) / edge as f32, f32::from(cell) / edge as f32];
        let cell_px = [f32::from(cell), f32::from(cell)];
        let viewport = [edge as f32, edge as f32];
        let buffer = r
            .instance_buffer(std::slice::from_ref(&instance))
            .expect("instance tamponu");

        let cmd = r.queue.commandBuffer().expect("komut tamponu");
        let desc = MTLRenderPassDescriptor::new();
        // SAFETY: indeks 0 her render pass'te vardır.
        let att = unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) };
        att.setTexture(Some(&target));
        att.setLoadAction(MTLLoadAction::Clear);
        // Zemin **siyah ve opak**: ön çarpım tanığının karşılaştırma tabanı
        // bu. Renkli bir zemin kararmayı maskelerdi.
        att.setClearColor(MTLClearColor {
            red: 0.0,
            green: 0.0,
            blue: 0.0,
            alpha: 1.0,
        });
        att.setStoreAction(MTLStoreAction::Store);
        let enc = cmd
            .renderCommandEncoderWithDescriptor(&desc)
            .expect("encoder");
        enc.setRenderPipelineState(&r.emoji);
        vertex_uniform(&enc, &viewport, 1);
        vertex_uniform(&enc, &cell_px, 2);
        vertex_uniform(&enc, &uv_size, 3);
        // SAFETY: tampon ve doku bu blok boyunca yaşıyor; indeksler
        // `cell.metal`'in bildirimleriyle aynı.
        unsafe {
            enc.setVertexBuffer_offset_atIndex(Some(&buffer), 0, 0);
            enc.setFragmentTexture_atIndex(Some(color.as_ref()), 0);
            enc.drawPrimitives_vertexStart_vertexCount_instanceCount(
                MTLPrimitiveType::TriangleStrip,
                0,
                4,
                1,
            );
        }
        enc.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        assert_ne!(cmd.status(), MTLCommandBufferStatus::Error);
        let pixels = read_pixels(&target, edge);
        pixel_at(&pixels, edge, 2, 2)
    }

    /// Kümenin (`🇹🇷`) üç yüzeyde de **tek** renkli glyph'e inmesi (035
    /// R4.1): ızgara, doldurma bandı ve dock hücreyi kendi tablosuyla
    /// taşıyor, `prepare` onu atlasa `Sprite::Cluster` diye soruyor ve geniş
    /// glyph renk düzleminden iki dörtlü basıyor — kutu yuvası değil, iki RI
    /// de değil. Listeler korunan (hareket) karede ikinci `prepare` aynı
    /// yuvaları veriyor: tablo listelerle birlikte yaşıyor.
    #[test]
    fn a_cluster_is_one_color_glyph_on_every_surface() {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let mut tex = AtlasTexture {
            // Retina: 13pt@1x'te bayrağın mürekkebi iki hücreyi aşıyor ve
            // küme taban karaktere düşüyor (`bt-atlas`'ın küme sınamalarının
            // ölçeği, 035 phase-1 → Uygulama Notları).
            atlas: Atlas::new(None, 13.0, 2.0, 1.0),
            texture: None,
            instances: Vec::new(),
            color_texture: None,
            color_instances: Vec::new(),
            fx_instances: Vec::new(),
        };
        let mut frame = Frame::default();
        frame.clear(grid(16, 32), CaretStyle::default());
        let mut clusters = frame.take_clusters();
        let cell = Cell {
            ch: Some('🇹'),
            wide: true,
            cluster: clusters.push("🇹🇷"),
            ..Default::default()
        };
        frame.put_clusters(clusters);
        frame.push(cell);
        frame.set_fill_rows(1);
        frame.push_fill(cell);
        let mut dock = frame.take_dock_clusters();
        let dock_cell = Cell {
            cluster: dock.push("🇹🇷"),
            ..cell
        };
        frame.put_dock_clusters(dock);
        frame.push_dock(dock_cell);

        let surfaces = [
            ("ızgara", frame.glyphs(), frame.clusters()),
            ("bant", frame.fill_glyphs(), frame.clusters()),
            ("dock", frame.dock_glyphs(), frame.dock_clusters()),
        ];
        let mut first = None;
        for (name, glyphs, clusters) in surfaces {
            for pass in ["içerik", "hareket"] {
                tex.prepare(&device, glyphs, clusters, &[])
                    .expect("prepare");
                // Renkli bayrak fontu yoksa sınama konusuz; `🎉`'nin emsali.
                if tex.color_instances.is_empty() && tex.instances.is_empty() {
                    return;
                }
                assert_eq!(
                    tex.color_instances.len(),
                    2,
                    "{name}/{pass}: bayrak renk düzleminden iki dörtlü olmalı"
                );
                assert!(
                    tex.instances.is_empty(),
                    "{name}/{pass}: maske listesine düştü (kutu ya da tek RI)"
                );
                let uvs: Vec<[f32; 2]> = tex.color_instances.iter().map(|part| part.uv0).collect();
                assert_eq!(*first.get_or_insert(uvs.clone()), uvs, "{name}/{pass}");
            }
        }
        // Tek başına `🇹` (kümesiz aynı hücre) **başka** yuvalar: yukarıdaki
        // dörtlüler kümenin, taban karakterin değil.
        let lone = [GlyphCell {
            cluster: None,
            ..frame.glyphs()[0]
        }];
        tex.prepare(&device, &lone, frame.clusters(), &[])
            .expect("prepare");
        let lone: Vec<[f32; 2]> = tex
            .color_instances
            .iter()
            .chain(&tex.instances)
            .map(|part| part.uv0)
            .collect();
        assert_ne!(
            first,
            Some(lone),
            "küme taban karakterin yuvasından çizildi"
        );
    }

    /// Renkli aday **renk düzlemine** gidiyor ve maske listesine hiç
    /// girmiyor.
    ///
    /// İki liste ayrı olmak zorunda: başka pipeline, başka doku, başka blend.
    /// Karışsalardı tek draw call iki fragment'i birden isteyemezdi ve emoji
    /// metnin ön plan rengiyle boyanırdı.
    #[test]
    fn a_color_glyph_goes_to_the_color_list() {
        let device = MTLCreateSystemDefaultDevice().expect("Metal device");
        let mut tex = AtlasTexture {
            atlas: Atlas::new(None, 13.0, 1.0, 1.0),
            texture: None,
            instances: Vec::new(),
            color_texture: None,
            color_instances: Vec::new(),
            fx_instances: Vec::new(),
        };
        // Emoji sunumu varsayılan olan bir kod noktası; ızgara ona iki sütun
        // ayırıyor, yani `wide` kurulu geliyor.
        let glyphs = [GlyphCell {
            pos: [0.0, 0.0],
            ch: '🎉',
            face: Face::Regular,
            size: SizeClass::Normal,
            rgba: [1.0, 1.0, 1.0, 1.0],
            wide: true,
            cluster: None,
        }];
        tex.prepare(&device, &glyphs, &Clusters::default(), &[])
            .expect("prepare");
        // Karakteri taşıyan renkli bir font kurulu değilse sınama konusuz —
        // ama kaçış dalı **regresyonu görmek zorunda**: `has_color_glyphs`
        // bozulursa `🎉` maske düzlemine düşer ve `color_instances` yine boş
        // kalır. O hâlde maske listesinin tofu'dan başka bir şey taşımaması
        // gerekiyor; taşıyorsa renkli bir glyph maske olarak çizilmiş demektir.
        if tex.color_instances.is_empty() {
            assert_eq!(
                tex.atlas.occupancy().0,
                1,
                "renkli aday maske düzleminde yuva açtı: `has_color_glyphs` düzlemi kaçırdı"
            );
            return;
        }
        assert_eq!(
            tex.color_instances.len(),
            2,
            "geniş emoji iki dörtlü üretmeli"
        );
        assert!(
            tex.instances.is_empty(),
            "renkli aday maske listesine girdi: {:?}",
            tex.instances.len()
        );
        assert!(
            tex.color_texture.is_some(),
            "renk dokusu ilk renkli yuvayla kurulmalı"
        );
        assert_eq!(
            tex.atlas.color_occupancy().0,
            2,
            "renk düzlemi iki yuva harcamalı"
        );
        assert_eq!(
            tex.atlas.occupancy().0,
            1,
            "maske düzlemi yalnız tofu'yu tutmalı"
        );
    }

    /// Atlas yeniden kurulunca **renk dokusu da düşüyor**.
    ///
    /// `Atlas::ensure` atlası baştan kuruyor: `color_next` sıfırlanıyor ve
    /// doku kenarı değişebiliyor (kenar `SLOT_TARGET` ile hücre ölçüsünden
    /// türüyor). Eski kenarda kalan bir renk dokusu yeni ızgaranın
    /// köşeleriyle yazılırsa `replaceRegion` dokunun **dışına** taşar —
    /// Cmd+ ile puntoyu büyütmek bu yolu ekranda emoji varken tetikliyor.
    /// Maske dokusunun aynı satırı 022'den beri var; bu bekçi ikisini
    /// birlikte tutuyor.
    #[test]
    fn rebuilding_the_atlas_drops_both_textures() {
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 32;
        // Emojiyi gerçek kare yolundan geçir: doku ancak ilk renkli yuvayla
        // doğuyor.
        let mut frame = Frame::default();
        let metrics = r.cell_metrics(1.0);
        frame.clear(metrics, CaretStyle::default());
        frame.push(Cell {
            col: 0,
            row: 0,
            ch: Some('🎉'),
            wide: true,
            ..Default::default()
        });
        render_offscreen(&r, EDGE, BACKGROUND, &frame);
        let color_before = r
            .atlas
            .borrow()
            .as_ref()
            .is_some_and(|tex| tex.color_texture.is_some());
        // Renkli bir font kurulu değilse doku hiç doğmuyor ve sınama konusuz.
        if !color_before {
            return;
        }
        assert!(
            r.atlas
                .borrow()
                .as_ref()
                .is_some_and(|tex| tex.texture.is_some()),
            "maske dokusu da kurulmuş olmalı"
        );
        // Punto değişimi atlasın anahtarını değiştiriyor, yani `ensure`
        // yeniden kuruyor.
        assert!(
            r.set_font(&FontOptions {
                size: 31.0,
                ..FontOptions::default()
            }),
            "punto değişti"
        );
        r.cell_metrics(1.0);
        let atlas = r.atlas.borrow();
        let tex = atlas.as_ref().expect("atlas duruyor");
        assert!(tex.texture.is_none(), "maske dokusu düşmedi");
        assert!(
            tex.color_texture.is_none(),
            "renk dokusu düşmedi: eski kenarda kalan doku taşan bir replaceRegion alır"
        );
    }

    // ---- Dock'un yazım efektleri (030): hermetik değişmezler (R5) ----
    //
    // Ara karelerin doğruluğu yalnız gözle; burada her efekt için döngüyle
    // sınanan şey uçlar ve sınırlar: geliş `t = 1`'de statik glyph'in ta
    // kendisi, hayalet `t = 1`'de düz zemin, komşu yuva hiç örneklenmiyor ve
    // geniş glyph tek kutu olarak dönüşüyor.

    /// `heat`'in kızgın rengi: ön plandan ([`WHITE`]) ve zeminden ayrı, yani
    /// `heat`'in ara karesi ikisiyle de karışmıyor.
    const HEAT: LinearRgba = LinearRgba::from_srgb(0xff, 0x80, 0x20);

    /// Tek satırlık dock'lu bir kare: zemin [`BACKGROUND`], hücreler dock'a
    /// basılı, efektler onların üstünde.
    fn dock_fx_frame(cell_px: (u16, u16), cells: &[Cell], fx: &[Fx]) -> Frame {
        let mut frame = Frame::default();
        frame.clear(grid(cell_px.0, cell_px.1), CaretStyle::default());
        for &cell in cells {
            frame.push_dock(cell);
        }
        frame.set_dock_fx(fx.iter().copied(), &Clusters::default(), HEAT);
        frame.set_dock_rows(1);
        frame.open_dock(BACKGROUND, BACKGROUND);
        frame
    }

    fn effect(cell: Cell, kind: Kind, effect: u32, t: f32) -> Fx {
        Fx {
            cell,
            kind,
            effect,
            t,
            seed: 0.0,
        }
    }

    fn wide_cell(col: u16, ch: char) -> Cell {
        Cell {
            wide: true,
            ..glyph_cell(col, ch, None)
        }
    }

    /// Dock satırının tepesi, piksel: dock dokunun dibine yaslı.
    fn dock_row_top(edge: usize, ch: u16) -> usize {
        edge - usize::from(ch)
    }

    #[test]
    fn an_arrival_at_its_end_is_the_static_glyph_pixel_for_pixel() {
        // Son efekt karesinden statik çizime devirde harf sıçramamalı: `t = 1`
        // dalı statik yolun aritmetiğine iniyor. Üç düzen: tek hücre, geniş
        // glyph'in iki yarısı ve renk düzlemi (emoji; renkli font kurulu
        // değilse tofu ve iddia maske düzleminde kalıyor).
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let cell_px = fitting_cell_px(&r, EDGE, 4);
        for &keypress in &Keypress::effects() {
            let id = keypress.id().expect("çizen efekt");
            for cell in [
                glyph_cell(2, 'M', None),
                wide_cell(2, '漢'),
                wide_cell(2, '🎉'),
            ] {
                let still = dock_fx_frame(cell_px, &[cell], &[]);
                // Önbellek ısıtılıyor: renkli font kurulu değilse `🎉` tofu'ya
                // düşüyor ve atlas negatif cevabı ilk soruluşta tek hücre,
                // önbellekten iki yarı veriyor (`bt_atlas::Atlas::slot`) —
                // karşılaştırılan şey efektin yolu, atlasın ilk sorusu değil.
                render_offscreen(&r, EDGE, BACKGROUND, &still);
                let moving =
                    dock_fx_frame(cell_px, &[cell], &[effect(cell, Kind::Arrival, id, 1.0)]);
                assert!(
                    moving.dock_glyphs().is_empty() && moving.dock_arrivals().len() == 1,
                    "geliş efektin yolundan çizilmiyor ({keypress:?}, {:?})",
                    cell.ch
                );
                let a = render_offscreen(&r, EDGE, BACKGROUND, &still);
                let b = render_offscreen(&r, EDGE, BACKGROUND, &moving);
                let top = dock_row_top(EDGE, cell_px.1);
                assert!(
                    (0..EDGE)
                        .any(|x| brightness(&a, EDGE, x, top + usize::from(cell_px.1) / 2) > 0)
                        || (top..EDGE).any(|y| (0..EDGE).any(|x| brightness(&a, EDGE, x, y) > 0)),
                    "statik glyph hiç çizilmedi — eşitlik bir şey iddia etmez ({:?})",
                    cell.ch
                );
                assert!(
                    a == b,
                    "t = 1'de geliş statik glyph'ten ayrışıyor ({keypress:?}, {:?})",
                    cell.ch
                );
            }
        }
    }

    #[test]
    fn a_ghost_starts_as_the_glyph_and_ends_as_bare_ground() {
        // Hayalet `t = 0`'da silinen glyph'in kendisi (silme anında harf
        // sıçramıyor), `t = 1`'de ise hiçbir şey: son efekt karesinden sonra
        // zemin düz.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let cell_px = fitting_cell_px(&r, EDGE, 4);
        let bare = render_offscreen(&r, EDGE, BACKGROUND, &dock_fx_frame(cell_px, &[], &[]));
        for &erase in &Erase::effects() {
            let id = erase.id().expect("çizen efekt");
            for cell in [glyph_cell(2, 'M', None), wide_cell(2, '漢')] {
                let glyph =
                    render_offscreen(&r, EDGE, BACKGROUND, &dock_fx_frame(cell_px, &[cell], &[]));
                assert_ne!(glyph, bare, "glyph çizilmedi ({:?})", cell.ch);
                let start = dock_fx_frame(cell_px, &[], &[effect(cell, Kind::Ghost, id, 0.0)]);
                let end = dock_fx_frame(cell_px, &[], &[effect(cell, Kind::Ghost, id, 1.0)]);
                assert!(
                    render_offscreen(&r, EDGE, BACKGROUND, &start) == glyph,
                    "t = 0'da hayalet silinen glyph değil ({erase:?}, {:?})",
                    cell.ch
                );
                assert!(
                    render_offscreen(&r, EDGE, BACKGROUND, &end) == bare,
                    "t = 1'de hayalet zemini düz bırakmıyor ({erase:?}, {:?})",
                    cell.ch
                );
            }
        }
    }

    #[test]
    fn an_effect_never_samples_its_neighbour_slot() {
        // Dörtlü efekt payı kadar şişiyor, ters dönüşüm hücrenin dışını
        // yuvanın dışına eşliyor ve ölçekleyen dallar doğrusal örnekliyor;
        // sınır testi ve texel merkezine kırpma olmasa komşu yuvanın glyph'i
        // (yuvalar arasında pay yok) efektin içinde belirirdi.
        //
        // **Ölçüt genlikten bağımsız**: aynı `.` iki atlasta çiziliyor — birinde
        // yuvasının iki yanı dolu (`@` önce, `#` sonra yuva alıyor), ötekinde
        // `.` tek başına. Çıktı komşuya bakmıyorsa iki kare özdeş. Önceki hâli
        // "hücrenin dışındaki her piksel sızıntıdır" diyordu ve bu, efektin
        // genliğini `.`'nın hücre içindeki boşluğuna bağlıyordu — kullanıcı
        // efektleri "hiç belli olmuyor" bulunca genlikler o zarfı aştı.
        //
        // Tolerans 2/255: iki atlasta `uv0` farklı ve doğrusal süzgecin alt
        // texel ağırlığı o farkın yuvarlamasıyla oynayabilir; bir sızıntı ise
        // komşunun mürekkebini, yani çok daha büyük bir farkı getirirdi.
        const EDGE: usize = 128;
        let crowded = Renderer::system_default().expect("Metal device ve pipeline");
        let alone = Renderer::system_default().expect("Metal device ve pipeline");
        let cell_px = fitting_cell_px(&crowded, EDGE, 8);
        // Atlas ölçüyle kuruluyor: ikinci renderer'ın da aynı ölçüsü olmalı.
        assert_eq!(fitting_cell_px(&alone, EDGE, 8), cell_px);
        let neighbours = [
            glyph_cell(2, '@', None),
            glyph_cell(3, '.', None),
            glyph_cell(4, '#', None),
        ];
        render_offscreen(
            &crowded,
            EDGE,
            BACKGROUND,
            &dock_fx_frame(cell_px, &neighbours, &[]),
        );
        let dot = glyph_cell(5, '.', None);
        render_offscreen(
            &alone,
            EDGE,
            BACKGROUND,
            &dock_fx_frame(cell_px, &[dot], &[]),
        );
        let kinds = Keypress::effects()
            .into_iter()
            .map(|fx| (Kind::Arrival, fx.id().expect("çizen efekt")))
            .chain(
                Erase::effects()
                    .into_iter()
                    .map(|fx| (Kind::Ghost, fx.id().expect("çizen efekt"))),
            );
        for (kind, id) in kinds {
            for t in [0.1, 0.25, 0.5, 0.75, 0.9] {
                // Gelişin statik glyph'i olmak zorunda (yoksa çizilmiyor).
                let statics: &[Cell] = if kind == Kind::Arrival { &[dot] } else { &[] };
                let frame = dock_fx_frame(cell_px, statics, &[effect(dot, kind, id, t)]);
                let a = render_offscreen(&crowded, EDGE, BACKGROUND, &frame);
                let b = render_offscreen(&alone, EDGE, BACKGROUND, &frame);
                let worst = a
                    .iter()
                    .zip(&b)
                    .map(|(x, y)| x.abs_diff(*y))
                    .max()
                    .unwrap_or(0);
                assert!(
                    worst <= 2,
                    "efekt komşu yuvaya bakıyor: dolu komşulu atlasla tek başına \
                     atlas {worst} ayrışıyor ({kind:?} {id}, t = {t})"
                );
            }
        }
    }

    #[test]
    fn heat_starts_in_the_cursor_color() {
        // `heat`'in rengi instance'tan değil uniform'dan (`Frame::dock_fx_heat`);
        // bağlanmasaydı ya da yanlış yuvaya bağlansaydı harf ön planın rengiyle
        // doğardı ve `t = 1` eşitliği bunu göremezdi. Ön plan [`WHITE`]
        // (kırmızısı mavisine eşit), [`HEAT`] turuncu: en parlak pikselde
        // kırmızı maviyi açıkça geçmeli.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let cell_px = fitting_cell_px(&r, EDGE, 4);
        let cell = glyph_cell(2, 'M', None);
        let id = Keypress::Heat.id().expect("çizen efekt");
        let pixels = render_offscreen(
            &r,
            EDGE,
            BACKGROUND,
            &dock_fx_frame(cell_px, &[cell], &[effect(cell, Kind::Arrival, id, 0.0)]),
        );
        let top = dock_row_top(EDGE, cell_px.1);
        let (red, _, blue) = (top..EDGE)
            .flat_map(|y| (0..EDGE).map(move |x| (x, y)))
            .map(|(x, y)| pixel_at(&pixels, EDGE, x, y))
            .max_by_key(|&(r8, g8, b8)| u32::from(r8) + u32::from(g8) + u32::from(b8))
            .expect("piksel var");
        assert!(
            u32::from(red) > u32::from(blue) + 64,
            "heat kızgın renkte doğmadı: en parlak piksel r={red} b={blue}"
        );
    }

    #[test]
    fn a_shattered_glyph_breaks_the_same_way_every_frame() {
        // `shatter`'ın parçaları tohumdan (`FxInstance`'ın `fx[2]`'si): aynı
        // girdi iki karede aynı parçaları vermeli — yoksa hareket karesinde
        // parçalar titrer —, başka bir tohum ise başka bir kırılma. İkinci
        // iddia shader'ın tohumu gerçekten okuduğunun tek tanığı.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let cell_px = fitting_cell_px(&r, EDGE, 4);
        let cell = glyph_cell(2, 'M', None);
        let id = Erase::Shatter.id().expect("çizen efekt");
        let draw = |seed: f32| {
            let fx = Fx {
                seed,
                ..effect(cell, Kind::Ghost, id, 0.5)
            };
            render_offscreen(&r, EDGE, BACKGROUND, &dock_fx_frame(cell_px, &[], &[fx]))
        };
        let first = draw(7.0);
        assert!(first == draw(7.0), "aynı tohum iki karede farklı kırıldı");
        assert!(first != draw(8.0), "tohum kırılmayı değiştirmiyor");
    }

    #[test]
    fn a_wide_glyph_transforms_as_one_box() {
        // Geniş glyph iki yuvaya bölünmüş ama tek kutu olarak dönüşmeli:
        // yarılar kendi merkezlerine küçülseydi (ya da `extrude`'da kendi sol
        // kenarlarından uzasaydı) ikisinin arasında — dikişte — boş bir şerit
        // açılırdı. Dikişin iki yanındaki sütunlarda statik glyph'in mürekkebi
        // var; kutunun merkezi ölçeklerin sabit noktası, kaymalar ise dikişi
        // mürekkepli bir satırdan geçiriyor — yani her efektin ara karesinde
        // de orada mürekkep kalmalı.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let cell_px = fitting_cell_px(&r, EDGE, 4);
        let (cw, ch) = (usize::from(cell_px.0), usize::from(cell_px.1));
        let han = wide_cell(1, '漢');
        let seam = 2 * cw;
        let top = dock_row_top(EDGE, cell_px.1);
        let inked = |pixels: &[u8]| {
            (top..top + ch).any(|y| {
                brightness(pixels, EDGE, seam - 1, y) > 0 || brightness(pixels, EDGE, seam, y) > 0
            })
        };
        let still = render_offscreen(&r, EDGE, BACKGROUND, &dock_fx_frame(cell_px, &[han], &[]));
        assert!(
            inked(&still),
            "önkoşul: statik `漢`'ın dikişte mürekkebi yok"
        );
        let kinds = Keypress::effects()
            .into_iter()
            .map(|fx| (Kind::Arrival, fx.id().expect("çizen efekt")))
            .chain(
                Erase::effects()
                    .into_iter()
                    .map(|fx| (Kind::Ghost, fx.id().expect("çizen efekt"))),
            );
        for (kind, id) in kinds {
            // Gelişin statik glyph'i olmak zorunda (yoksa çizilmiyor).
            let statics: &[Cell] = if kind == Kind::Arrival { &[han] } else { &[] };
            let frame = dock_fx_frame(cell_px, statics, &[effect(han, kind, id, 0.5)]);
            let pixels = render_offscreen(&r, EDGE, BACKGROUND, &frame);
            assert!(
                inked(&pixels),
                "geniş glyph dikişte yarıldı: iki yarı ayrı kutular gibi dönüştü ({kind:?} {id})"
            );
        }
    }
}
