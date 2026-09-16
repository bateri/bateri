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
use bt_atlas::{Atlas, Face, FontIssue, Metrics, Sprite, TOFU};
use bt_core::{FontOptions, LinearRgba};
use dispatch2::DispatchData;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSString;
use objc2_metal::{
    MTLBlendFactor, MTLBuffer, MTLClearColor, MTLCommandBuffer, MTLCommandBufferStatus,
    MTLCommandEncoder, MTLCommandQueue, MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary,
    MTLLoadAction, MTLOrigin, MTLPixelFormat, MTLPrimitiveType, MTLRegion, MTLRenderCommandEncoder,
    MTLRenderPassDescriptor, MTLRenderPipelineDescriptor, MTLRenderPipelineState,
    MTLResourceOptions, MTLSize, MTLStorageMode, MTLStoreAction, MTLTexture, MTLTextureDescriptor,
    MTLTextureUsage,
};
use objc2_quartz_core::CAMetalDrawable;

use crate::frame::{Frame, GlyphCell, GlyphInstance, RuleCell};
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
}

/// Hücrenin **fiziksel piksel** ölçüsü (ölçek uygulanmış); `bt-shell` grid
/// boyutunu ve PTY'ye giden `TIOCSWINSZ`'i bundan türetir.
///
/// Alan `private` ve kurucusu sıfırı eleyen [`CellMetrics::new`]: bu tipin işi
/// bir demeti adlandırmak değil, **taşımak**. `pub` bir alan olsaydı
/// `CellMetrics { cell_px: (0, 0) }` `bt-shell`'den kurulabilirdi ve
/// `900.0 / 0.0` → `inf`, `inf as u16` → `65535`, yani 65535×65535'lik bir
/// grid ile o boyda bir `TIOCSWINSZ`. `Session::resize` yalnız sıfır grid'i
/// eliyor; bu sessizce geçerdi. Şimdi geçemiyor: **≥ 1 garantisi tipin
/// içinde**, kaynağı `bt_atlas::Metrics` (`font::round_up` 1'e kırpar).
///
/// `bt_atlas::Metrics`'i yeniden ihraç **etmiyor**: `bt-shell`'in bir
/// `bt-atlas` tipi görmesi katman tablosunu bulanıklaştırırdı (`CLAUDE.md`),
/// ve atlasın `baseline_px`'i sınırın bu tarafında hiçbir işe yaramaz —
/// glyph'i taban çizgisine oturtmak `bt-atlas`'ın kendi işi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellMetrics {
    cell_px: (u16, u16),
}

impl CellMetrics {
    /// Sıfır bileşen yoksa ölçü, varsa `None`.
    ///
    /// Alan `private` ama kurucu `pub`: garanti "kimse kuramasın" ile değil
    /// **"kuran sıfırı geçiremesin"** ile sağlanıyor. Aradaki fark sınamada
    /// görünür — `bt-shell`'in grid aritmetiği bir Metal device kurmadan
    /// sınanabilir kalıyor, oysa yalnız `Renderer::cell_metrics`'in
    /// kurabildiği bir tip o testleri GPU'ya bağlardı.
    pub fn new(width: u16, height: u16) -> Option<Self> {
        (width > 0 && height > 0).then_some(Self {
            cell_px: (width, height),
        })
    }

    /// (genişlik, yükseklik).
    ///
    /// Tip `bt-gpu` ile `bt-shell` arasında **taşınıyor**; demet yalnız
    /// değerin tipi bırakmak zorunda olduğu üç yerde açılıyor: sayıya dönüp
    /// bölmeye girerken (`split_into_grid`), `bt-core`'a geçerken
    /// (`SessionOptions.cell_px`, `Session::resize` — `bt-core` `bt-gpu`'yu
    /// göremez, katman kuralının bedeli bu) ve `#[repr(C)]` kare kurucusuna
    /// girerken (`Frame::clear`). Bunların dışında demet dolaşmaz.
    pub fn cell_px(self) -> (u16, u16) {
        self.cell_px
    }
}

pub struct Renderer {
    /// Kurucuda elde olan device; tampon ayırmak için kare başına
    /// `queue.device()` mesajı atmaya gerek yok.
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    /// Hücre arka planlarını ve imleci çizen pipeline; instanced quad.
    ///
    /// **Alfa blend açık ve tek müşterisi imleç:** arka planların alfası her
    /// zaman `1.0` ([`bt_core::LinearRgba`]'nın tek kurucusu öyle yazıyor),
    /// yani onlar için blend'in sonucu opak yazmayla birebir aynı. Belirme
    /// (Hareketi Azalt) imlecin dikdörtgenini `1.0`'ın altına indiren tek yol
    /// ve o dikdörtgen bu listenin **son** instance'ı, yani altındaki hücre
    /// arka planının üstüne karışıyor.
    cell_bg: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Glyph'leri çizen pipeline; aynı quad, atlas örneklemesi.
    ///
    /// Ayrı pipeline çünkü **ayrı shader çifti**: fragment atlası örnekliyor
    /// ve imleç uniform'unu okuyor. (Blend durumu artık ikisinde de aynı, yani
    /// ayrılığın sebebi değil.)
    cell: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
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
        let queue = device.newCommandQueue().ok_or(GpuError::NoCommandQueue)?;

        Ok(Self {
            device,
            queue,
            cell_bg,
            cell,
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

    /// Verilen backing ölçeğinde hücre ölçüsü.
    ///
    /// `scale` parametre çünkü ekran ölçeği çalışırken değişebilir
    /// (`windowDidChangeBackingProperties:`, harici ekran) ve atlas ölçeği
    /// önbellek anahtarının parçası olarak taşır: aynı `Renderer` iki ölçekte
    /// iki farklı metrik verir. @1x rasterize edilmiş bir glyph @2x'te
    /// hatasız bulanıklaşır ve belirti yalnız iki ekranlı makinede görünür.
    ///
    /// `bt-shell` `bt-atlas`'ı görmüyor, metrik buradan geçiyor; katman
    /// tablosu (`CLAUDE.md`) değişmeden `CELL_PX` yer tutucusu ölebildi.
    pub fn cell_metrics(&self, scale: f64) -> CellMetrics {
        let (w, h) = self.sync_atlas(scale).cell_px;
        // audit: `bt_atlas::Metrics.cell_px` çıplak bir `pub` alan, yani ≥ 1
        // garantisi bir crate ötede (`font::round_up` 1'e kırpar) ve tipin
        // kendisi taşımıyor. Yapı gövdesiyle kurmak bu boşluğu sessiz
        // bırakırdı; `expect` onu programlama hatasına çevirir. Panik yolu
        // değil: PTY okuma ve ayrıştırma bu satırdan geçmez, burası
        // pencere geometrisi yolu.
        CellMetrics::new(w, h).expect("bt-atlas hücre ölçüsünü 1'e kırpar")
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
        Some(match issue {
            FontIssue::FamilyNotFound { requested, using } => FontNotice::FamilyNotFound {
                requested: requested.clone(),
                using: using.clone(),
            },
            FontIssue::NotMonospaced { family } => FontNotice::NotMonospaced {
                family: family.clone(),
            },
        })
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
    fn sync_atlas(&self, scale: f64) -> Metrics {
        let font = self.font.borrow();
        let family = font.family.as_deref();
        let mut slot = self.atlas.borrow_mut();
        let atlas_tex = slot.get_or_insert_with(|| AtlasTexture {
            atlas: Atlas::new(family, font.size, scale),
            texture: None,
            instances: Vec::new(),
        });
        if atlas_tex.atlas.ensure(family, font.size, scale) {
            atlas_tex.texture = None;
        }
        // Ödünç değil **metrik** dönüyor: atlas ödüncünün bir çağrı sınırını
        // aşabildiği tek yer burasıydı ve [`Renderer::encode_glyphs`]'in
        // dayandığı özellik tam olarak bunun olmaması.
        atlas_tex.atlas.metrics()
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
        // Sıra çizim sırasıdır (R4.1): önce arka planlar **ve imleç**, sonra
        // glyph'ler, en sonda kurallar (ikisi de `encode_glyphs`'te, aynı
        // pipeline'da). Ters olsaydı imleç altındaki harfi örterdi — imleç
        // opak ve `Frame`'in arka plan listesinin sonunda; imlecin üstündeki
        // alt çizgi de aynı sıradan bedavaya görünür kalıyor.
        // Viewport tek yerde türetiliyor: iki encoder da aynı dokuya çiziyor
        // ve ayrı ayrı sormaları kare başına iki fazladan objc mesajı ile
        // ayrışabilen iki tanım demekti.
        let viewport_px: [f32; 2] = [texture.width() as f32, texture.height() as f32];
        let result = self
            .encode_bg(&enc, frame, viewport_px)
            .and_then(|()| self.encode_glyphs(&enc, frame, viewport_px));
        enc.endEncoding();
        result
    }

    /// Instance dilimini kare başına yeni bir Metal tamponuna kopyalar.
    ///
    /// Kare başına yeni tampon: üçlü tamponlama bilinçli olarak reddedildi
    /// (002 discussion.md → Muhakeme), `/measure` sonrası yeniden bakılır.
    /// Komut tamponu buffer'ı tamamlanana kadar tutar. Karar **burada** tek
    /// yerde: iki pipeline da bu fonksiyondan geçiyor, yani değişirse ikisi
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

    /// Arka planları (ve imleci) tek bir instanced çizim çağrısına encode eder.
    fn encode_bg(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        frame: &Frame,
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        let instances = frame.bg_instances();
        // Sıfır uzunluklu `newBufferWithBytes` Metal doğrulamasında geçersiz;
        // hücresiz karede clear yükü tek başına yeter.
        if instances.is_empty() {
            return Ok(());
        }
        // Düzen `Instance`'ın `offset_of` assert'leriyle `cell_bg.metal`'e bağlı.
        let buffer = self.instance_buffer(instances)?;

        enc.setRenderPipelineState(&self.cell_bg);
        // İndeksler `cell_bg.metal`'in `[[buffer(0)]]` / `[[buffer(1)]]`
        // bildirimleriyle aynı.
        vertex_uniform(enc, &viewport_px, 1);
        // SAFETY: tampon bu blok boyunca yaşıyor; dörtlü köşe vertex_id'den
        // türetildiği için vertex buffer'da köşe verisi yok.
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
    fn encode_glyphs(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        frame: &Frame,
        viewport_px: [f32; 2],
    ) -> Result<(), GpuError> {
        let (glyphs, rules) = (frame.glyphs(), frame.rules());
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
        atlas_tex.prepare(&self.device, glyphs, rules)?;
        // audit: `prepare` `Ok` döndüyse dokuyu kurmuştur; tek çıkış yolu `?`.
        let atlas_texture = atlas_tex.texture.as_ref().expect("prepare dokuyu kurdu");
        let instances = &atlas_tex.instances;

        // Düzen `GlyphInstance`'ın `offset_of` assert'leriyle `cell.metal`'e bağlı.
        let buffer = self.instance_buffer(instances)?;

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

        enc.setRenderPipelineState(&self.cell);
        // İndeksler `cell.metal`'in `[[buffer(n)]]` bildirimleriyle aynı.
        vertex_uniform(enc, &viewport_px, 1);
        vertex_uniform(enc, &frame.cell_px(), 2);
        vertex_uniform(enc, &uv_size, 3);
        // Fragment'in tampon indeksleri **ayrı bir alan**: vertex'in 0'ı
        // instance tamponu, fragment'in 0'ı imleç bloğu. Düzeni `CursorBlock`'un
        // `offset_of` assert'leri `cell.metal`'e bağlıyor.
        fragment_uniform(enc, frame.cursor_block(), 0);
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
/// Blend **parametre değil**: iki pipeline da onu istiyor ve sebepleri ayrı —
/// `cell` alfayı atlasın kapsamasından üretiyor, `cell_bg`'de imlecin
/// belirmesi ([`crate::motion`]) dikdörtgeni saydamlaştırıyor. Bir `enum`
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
        rules: &[RuleCell],
    ) -> Result<(), GpuError> {
        let metrics = self.atlas.metrics();
        let (tw, th) = self.atlas.texture_px();
        if self.texture.is_none() {
            let texture = new_atlas_texture(device, tw, th)?;
            // Rezident tofu bir kez yazılır ve bir daha dokunulmaz:
            // `Atlas::slot` tofu'ya düştüğünde bitmap **vermiyor**, çünkü veri
            // zaten burada.
            upload_slot(
                &texture,
                self.atlas.slot_origin(TOFU),
                metrics,
                self.atlas.tofu_bitmap(),
            );
            self.texture = Some(texture);
        }
        // audit: hemen üstte kuruldu ya da zaten doluydu. `match` ile
        // kurtulunamıyor: `None` kolunda dokuyu kurup aynı ödünçten geri
        // vermek NLL'den geçmiyor.
        let texture = self.texture.as_ref().expect("doku hemen üstte kuruldu");

        self.instances.clear();
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
            let uv0 = slot_uv(
                &mut self.atlas,
                texture,
                metrics,
                inv,
                Sprite::Char(glyph.ch),
                glyph.face,
            );
            self.instances.push(GlyphInstance {
                pos: glyph.pos,
                uv0,
                rgba: glyph.rgba,
            });
        }
        for rule in rules {
            // Kurallar **her zaman** `Face::Regular`: kalın metnin altındaki
            // çizgi kalın değildir. `Atlas::slot` bunu ayrıca normalize ediyor;
            // burada da doğru yüzü sormak o normalizasyonu bir savunma
            // katmanı olarak bırakıyor, tek dayanak yapmıyor.
            let uv0 = slot_uv(
                &mut self.atlas,
                texture,
                metrics,
                inv,
                Sprite::Rule(rule.kind),
                Face::Regular,
            );
            self.instances.push(GlyphInstance {
                pos: rule.pos,
                uv0,
                rgba: rule.rgba,
            });
        }
        Ok(())
    }
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
    metrics: Metrics,
    inv: (f32, f32),
    sprite: Sprite,
    face: Face,
) -> [f32; 2] {
    let (slot, upload) = atlas.slot(sprite, face);
    let (x, y) = if let Some(upload) = upload {
        upload_slot(texture, upload.origin, metrics, upload.bytes);
        upload.origin
    } else {
        atlas.slot_origin(slot)
    };
    [f32::from(x) * inv.0, f32::from(y) * inv.1]
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
    assert_eq!(bytes.len(), metrics.slot_bytes(), "tam bir yuva olmalı");
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
            w,
        );
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Instant;

    use bt_core::{Cell, Cursor, Theme, UnderlineStyle};

    use super::*;
    use crate::stats::Stats;

    /// Gömülü temanın zemini ve vurgusu: üretimde clear ve imleç rengi bu
    /// iki rolden geliyor (`link.rs`), sınamalar da aynı kaynaktan.
    const BACKGROUND: LinearRgba = Theme::BATERI.background_linear();
    const ACCENT: LinearRgba = Theme::BATERI.accent_linear();

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
        assert!(CellMetrics::new(0, 18).is_none());
        assert!(CellMetrics::new(9, 0).is_none());
        assert_eq!(CellMetrics::new(9, 18).expect("ölçü").cell_px(), (9, 18));
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
        frame.clear((8, 8));
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
        frame.clear((8, 8));
        frame.push(bg_cell(0, 0, LinearRgba::from_srgb(0xff, 0x00, 0x00)));
        frame.push(bg_cell(1, 1, LinearRgba::from_srgb(0x00, 0xff, 0x00)));
        frame.push(bg_cell(0, 1, BACKGROUND));

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
        // giden bayt paletin yazıldığı bayt olmalı. Lineerleştirme düşerse
        // `0x1a1c21` `0x5a5d65` griye açılır — geçişin sessiz kalabileceği
        // tek yer burasıydı; saf kırmızı ve yeşil bunu göremez, ikisi de
        // sRGB transfer fonksiyonunun sabit noktaları.
        close_to(
            pixel(2, 12),
            srgb(Theme::BATERI.background),
            "hücre ara tonu",
        );
        close_to(
            pixel(12, 2),
            srgb(Theme::BATERI.accent),
            "boş çeyrek clear rengi",
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
        frame.clear((cw, ch));
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
            frame.clear((cw, ch));
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
        frame.clear((cw, ch));
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
        frame.clear((cw, ch));
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
        frame.clear((cw, ch));
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
            col,
            row: 0,
            visible: true,
            text,
            // Kaydırma kararı hareketin işi (`motion.rs`); burada çizilen
            // piksel sorgulanıyor ve konum zaten `push_settled` ile hedefin
            // kendisi.
            display_offset: 0,
        }
    }

    /// İmleci **kendi** hücresine çizer: bu sınamaların hepsi yerleşmiş bloğa
    /// bakıyor, ara konuma değil (onun sınaması `frame.rs`'te).
    fn push_settled(frame: &mut Frame, cursor: Cursor, rgba: LinearRgba) {
        frame.push_cursor(
            cursor,
            [f32::from(cursor.col), f32::from(cursor.row)],
            rgba,
            1.0,
        );
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
        frame.clear((cw, ch));
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
        let cell = |col| cell_rows(&pixels, EDGE, (cw, ch), col).concat();
        let (a, b, c) = (cell(0), cell(1), cell(2));

        assert_eq!(
            a, b,
            "imleç altındaki harf metin rengiyle çizilmedi (A ≠ B)"
        );
        assert_ne!(a, c, "imleç dikdörtgeni harfin rengini hiç ezmedi (A = C)");
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
            frame.clear((cw, ch));
            frame.push(glyph_cell(0, 'M', None));
            if let Some(alpha) = alpha {
                frame.push_cursor(cursor_at(0, BACKGROUND), [0.0, 0.0], ACCENT, alpha);
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
        // Phase-3'te dikdörtgen hücreler arasına oturunca aynı sınama ara
        // konumu kendiliğinden sorar.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const EDGE: usize = 64;
        let (cw, ch) = fitting_cell_px(&r, EDGE, 2);

        // Sınırı soran şey **kural**, glyph değil: band hücrenin ilk
        // sütununu da tam kaplıyor, yani "o sütun ezildi mi" sorusu fontun
        // hangi pikseli boyadığına bağlı kalmıyor (glyph'in ilk sütunu boş
        // olabilir ve sınama sessizce hiçbir şey sormaz).
        let mut frame = Frame::default();
        frame.clear((cw, ch));
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
        frame.clear((cw, ch));
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
        frame.clear((cw, ch));
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
        frame.clear((8, 16));
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
}
