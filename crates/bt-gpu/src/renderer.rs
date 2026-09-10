//! Renderer: metallib'i yükler, pipeline'ı kurar, verilen drawable'a bir kare
//! çizer. Drawable'ı kimin sağladığını bilmez. Çizim **asenkrondur**: `commit`
//! GPU'yu beklemez ve kare sayacını `addCompletedHandler` artırır — yani sayaç
//! "sunuldu"yu değil "GPU hatasız bitirdi"yi sayar.

use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use block2::RcBlock;
use dispatch2::DispatchData;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::ProtocolObject;
use objc2_foundation::ns_string;
use objc2_metal::{
    MTLClearColor, MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary, MTLLoadAction, MTLPixelFormat,
    MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLResourceOptions, MTLStoreAction,
    MTLTexture,
};
use objc2_quartz_core::CAMetalDrawable;

use crate::frame::Frame;
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

pub struct Renderer {
    /// Kurucuda elde olan device; tampon ayırmak için kare başına
    /// `queue.device()` mesajı atmaya gerek yok.
    device: Retained<ProtocolObject<dyn MTLDevice>>,
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    /// Hücre arka planlarını ve imleci çizen tek pipeline; instanced quad.
    cell_bg: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
    /// Son **gönderilen** karedeki arka plan hücresi sayısı; `make duman`'ın
    /// `hucre=K` jetonu. `frames`'in yanında duruyor çünkü ikisi de aynı
    /// soruya bakan tanı sayaçları ve tek yerden okunmaları gerekiyor.
    /// Dikkat: bu bir **CPU** sayacıdır, GPU'nun o hücreleri boyadığını
    /// kanıtlamaz — onu `cell_bg_pikseli_gpu_tarafinda_boyar` yapar.
    last_bg_count: AtomicUsize,
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
    /// bu). Karşılığı `bt_core::color::lineer_rgba`; ikisi birlikte değişir —
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
        let vs = library
            .newFunctionWithName(ns_string!("cell_bg_vertex"))
            .ok_or(GpuError::MissingFunction("cell_bg_vertex"))?;
        let fs = library
            .newFunctionWithName(ns_string!("cell_bg_fragment"))
            .ok_or(GpuError::MissingFunction("cell_bg_fragment"))?;

        let desc = MTLRenderPipelineDescriptor::new();
        desc.setVertexFunction(Some(&vs));
        desc.setFragmentFunction(Some(&fs));
        // SAFETY: indeks 0 her render pipeline'da vardır.
        unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) }
            .setPixelFormat(Self::PIXEL_FORMAT);
        let cell_bg = device
            .newRenderPipelineStateWithDescriptor_error(&desc)
            .map_err(GpuError::Pipeline)?;
        let queue = device.newCommandQueue().ok_or(GpuError::NoCommandQueue)?;

        Ok(Self {
            device,
            queue,
            cell_bg,
            last_bg_count: AtomicUsize::new(0),
            frames: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn surface(&self) -> Surface {
        Surface::new(&self.device, Self::PIXEL_FORMAT)
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

    /// Kare tamamlanınca çağrılacak bloğu **bir kez** kurar.
    ///
    /// Blok kare başına kurulmuyor: taşıdığı hiçbir şey kareden kareye
    /// değişmiyor, oysa her kurulum bir heap ayırması ve birkaç `Arc`
    /// sayaç hareketi demek — hepsi tazeleme hızında. Metal `Block_copy` ile
    /// kendi referansını aldığı için aynı blok her komut tamponuna eklenebilir.
    pub(crate) fn completion(
        &self,
        on_complete: impl Fn(Result<(), GpuError>) + Send + Sync + 'static,
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
                on_complete(Ok(()));
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
        clear: [f32; 4],
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
        clear: [f32; 4],
        frame: &Frame,
    ) -> Result<(), GpuError> {
        let pass = MTLRenderPassDescriptor::new();
        // SAFETY: indeks 0 her render pass'te vardır.
        let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
        att.setTexture(Some(texture));
        // Arka planı yükün kendisi boyar. 001 bunu tam ekran bir quad'la
        // yapıyordu; Clear aynı işi çizim çağrısı harcamadan yapıyor.
        att.setLoadAction(MTLLoadAction::Clear);
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
        let sonuc = self.encode_cells(&enc, frame, texture);
        enc.endEncoding();
        sonuc
    }

    /// Kareyi tek bir instanced çizim çağrısına encode eder.
    fn encode_cells(
        &self,
        enc: &ProtocolObject<dyn MTLRenderCommandEncoder>,
        frame: &Frame,
        texture: &ProtocolObject<dyn MTLTexture>,
    ) -> Result<(), GpuError> {
        let instances = frame.instances();
        // Sıfır uzunluklu `newBufferWithBytes` Metal doğrulamasında geçersiz;
        // hücresiz karede clear yükü tek başına yeter.
        if instances.is_empty() {
            return Ok(());
        }
        // Kare başına yeni tampon: üçlü tamponlama bilinçli olarak
        // reddedildi (002 discussion.md → Muhakeme), `/measure` sonrası
        // yeniden bakılır. Komut tamponu buffer'ı tamamlanana kadar tutar.
        // SAFETY: `instances` bu blok boyunca yaşıyor; Metal baytları kurucuda
        // kopyalar. Uzunluk dilimin kendi baytıdır, düzen `Instance`'ın
        // `offset_of` assert'leriyle `.metal`'e bağlı.
        let buffer = unsafe {
            self.device.newBufferWithBytes_length_options(
                NonNull::from(instances).cast(),
                size_of_val(instances),
                MTLResourceOptions::StorageModeShared,
            )
        }
        .ok_or(GpuError::NoInstanceBuffer)?;

        // Shader tarafı `constant float2&`; tek bileşenli bir struct'a
        // sarmak ikinci bir Rust ↔ MSL düzen sözleşmesi açardı.
        let viewport_px: [f32; 2] = [texture.width() as f32, texture.height() as f32];
        enc.setRenderPipelineState(&self.cell_bg);
        // SAFETY: `viewport_px` bu blok boyunca yaşıyor ve Metal baytları encode
        // anında kopyalıyor. Buffer indeksleri shader'ın `[[buffer(0)]]` /
        // `[[buffer(1)]]` bildirimleriyle aynı; dörtlü köşe vertex_id'den
        // türetildiği için vertex buffer'da köşe verisi yok.
        unsafe {
            enc.setVertexBuffer_offset_atIndex(Some(&buffer), 0, 0);
            enc.setVertexBytes_length_atIndex(
                NonNull::from(&viewport_px).cast(),
                size_of_val(&viewport_px),
                1,
            );
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

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use bt_core::CellBg;
    use objc2_metal::{
        MTLOrigin, MTLRegion, MTLSize, MTLStorageMode, MTLTextureDescriptor, MTLTextureUsage,
    };

    use super::*;

    #[test]
    fn metallib_gomulu_ve_gecerli() {
        // Metal kütüphanesi dosyası "MTLB" sihirli sayısıyla başlar.
        assert_eq!(&METALLIB[..4], b"MTLB");
    }

    #[test]
    fn cell_bg_pipeline_kurulur() {
        // Device yoksa `ignored` değil açık hata: bu makinede Metal var,
        // yokluğu bir kusurdur. Pipeline'ın kurulması shader'ın derlendiğini
        // ve fonksiyon adlarının metallib'de bulunduğunu kanıtlar.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        assert_eq!(r.frames(), 0);
    }

    /// Sınama için küçük bir offscreen render hedefi; `Shared` depolama
    /// `getBytes` ile CPU'dan okumaya izin verir.
    fn hedef_doku(r: &Renderer, kenar: usize) -> Retained<ProtocolObject<dyn MTLTexture>> {
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                // Formatı `Renderer`'dan: pipeline hangi formata derlendiyse
                // hedef de o. Elle yazılsaydı sRGB geçişi burada assert'le
                // değil Metal doğrulama istisnasıyla düşerdi — ve istisna
                // sınamanın ne aradığını hiç söylemez.
                Renderer::PIXEL_FORMAT,
                kenar,
                kenar,
                false,
            )
        };
        desc.setUsage(MTLTextureUsage::RenderTarget);
        desc.setStorageMode(MTLStorageMode::Shared);
        r.device
            .newTextureWithDescriptor(&desc)
            .expect("offscreen doku")
    }

    #[test]
    fn tamamlanma_blogu_kareyi_sayar_ve_sonucu_iletir() {
        // `frames()`'in anlamı bu phase'de değişti: "commit edildi" değil,
        // "GPU hatasız bitirdi". O anlamı yalnız `make duman` görüyordu ve
        // orası "> 0" diye soruyor — sayacın hiç artmaması yeşil geçerdi.
        //
        // Adının söylemediği: **hatalı** tamponun sayılmadığı. `Error`
        // durumunu isteyerek üretmenin güvenilir bir yolu yok (cihaz kaybı,
        // zaman aşımı), o dal burada koşmuyor — sınama adının bunu iddia
        // etmemesi de bu yüzden.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        let gorulen = Arc::new(Mutex::new(Vec::new()));
        let completion = {
            let gorulen = Arc::clone(&gorulen);
            r.completion(move |sonuc| gorulen.lock().unwrap().push(sonuc.is_ok()))
        };

        let cmd = r.queue.commandBuffer().expect("komut tamponu");
        // SAFETY: blok geçerli ve `completion` çağrı boyunca yaşıyor.
        unsafe { cmd.addCompletedHandler(RcBlock::as_ptr(&completion.0)) };
        cmd.commit();
        // `waitUntilCompleted` tamamlanma handler'ları dönene kadar bekler.
        cmd.waitUntilCompleted();

        assert_eq!(r.frames(), 1, "hatasız biten kare sayılmalı");
        assert_eq!(*gorulen.lock().unwrap(), vec![true]);
    }

    #[test]
    fn cell_bg_pikseli_gpu_tarafinda_boyar() {
        // Bu sınama, phase-2'nin sildiği "çizim çağrısı pipeline'dan geçti"
        // kanıtının yerine geçiyor ve daha fazlasını söylüyor: buffer
        // indeksleri, NDC dönüşümü, y ters çevirme, instance stride'ı ve
        // GPU'nun `Instance` düzenini doğru okuması. İki taraftaki assert'ler
        // düzeni derleme zamanında bağlar ama hiçbir zaman ÇALIŞTIRMAZ;
        // burası çalıştırıyor. Pencere gerekmediği için başsız ortamda da koşar.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        const KENAR: usize = 16;
        let texture = hedef_doku(&r, KENAR);

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
        frame.push_bg(CellBg {
            col: 0,
            row: 0,
            rgba: [1.0, 0.0, 0.0, 1.0],
        });
        frame.push_bg(CellBg {
            col: 1,
            row: 1,
            rgba: [0.0, 1.0, 0.0, 1.0],
        });
        frame.push_bg(CellBg {
            col: 0,
            row: 1,
            rgba: bt_core::DEFAULT_BG,
        });

        // Clear rengi de **ara ton**, ve bilerek paletten: üretimde pencerenin
        // görünen zemininin tamamı bu yoldan geliyor (`frame()` varsayılan
        // arka planlı hücreleri eliyor, `link.rs` clear'a `DEFAULT_BG` veriyor).
        // Saf mavi bırakılsaydı `MTLClearColor`'ın sRGB hedefteki semantiği
        // sınanmamış kalırdı: onu "hedefin uzayına çevireyim" diye bir kez
        // daha kodlayan biri pencere zeminini karartır, hücreleri doğru
        // bırakır ve bütün sınamalar yeşil geçerdi.
        //
        // Clear **`DEFAULT_CURSOR`**, `DEFAULT_BG` değil: `DEFAULT_BG` hücrede
        // kullanıldı ve iki yolun ayrı ayrı kanıtlanması ayrık iki renk ister.
        // Buraya `DEFAULT_BG` "düzeltilirse" sınama hücre yolu ile clear
        // yolunu birbirinden ayırt edemez hâle gelir.
        let cmd = r.queue.commandBuffer().expect("komut tamponu");
        r.encode_pass(&cmd, &texture, bt_core::DEFAULT_CURSOR, &frame)
            .expect("pass encode edilemedi");
        cmd.commit();
        cmd.waitUntilCompleted();
        assert_ne!(cmd.status(), MTLCommandBufferStatus::Error);

        let mut pikseller = vec![0u8; KENAR * KENAR * 4];
        // SAFETY: tampon KENAR×KENAR×4 bayt, bölge dokunun tamamı, satır
        // adımı KENAR*4. Doku `StorageModeShared` ve komut tamponu tamamlandı.
        unsafe {
            texture.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                NonNull::new(pikseller.as_mut_ptr()).expect("tampon").cast(),
                KENAR * 4,
                MTLRegion {
                    origin: MTLOrigin { x: 0, y: 0, z: 0 },
                    size: MTLSize {
                        width: KENAR,
                        height: KENAR,
                        depth: 1,
                    },
                },
                0,
            );
        }

        // Bayt sırası B, G, R, A (formatın `_sRGB` eki sırayı değiştirmez).
        let piksel = |x: usize, y: usize| {
            let i = (y * KENAR + x) * 4;
            (pikseller[i + 2], pikseller[i + 1], pikseller[i])
        };
        assert_eq!(piksel(2, 2), (255, 0, 0), "ilk hücre sol üstte kırmızı");
        assert_eq!(piksel(12, 12), (0, 255, 0), "ikinci hücre sağ altta yeşil");
        // Paletin baytları burada elle yazılı (`BG` ve `CURSOR` `bt-core`'da
        // private). Tema modeli geldiğinde bu üçlüler onunla birlikte
        // güncellenir; bugün onları kaynağa bağlayacak bir `pub` yol yok.
        let yakin = |gorulen: (u8, u8, u8), beklenen: (u8, u8, u8), ne: &str| {
            // ±1: 8-bit sRGB kodlaması yuvarlama taşır ve Metal spec'i bit
            // birebirlik değil doğruluk sınırı verir. Bit aransaydı kapı
            // sürücü sürümüne rehin olurdu.
            assert!(
                gorulen.0.abs_diff(beklenen.0) <= 1
                    && gorulen.1.abs_diff(beklenen.1) <= 1
                    && gorulen.2.abs_diff(beklenen.2) <= 1,
                "{ne}: {gorulen:02x?} ≠ {beklenen:02x?}"
            );
        };
        // sRGB round-trip: `lineer_rgba`'nın lineerleştirmesi ile donanımın
        // yazarken yaptığı kodlama birbirini tersine çevirmeli, yani ekrana
        // giden bayt paletin yazıldığı bayt olmalı. Lineerleştirme düşerse
        // `0x1a1c21` `0x5a5d65` griye açılır — geçişin sessiz kalabileceği
        // tek yer burasıydı; saf kırmızı ve yeşil bunu göremez, ikisi de
        // sRGB transfer fonksiyonunun sabit noktaları.
        yakin(piksel(2, 12), (0x1a, 0x1c, 0x21), "hücre ara tonu");
        yakin(piksel(12, 2), (0x7a, 0x9c, 0xc6), "boş çeyrek clear rengi");
    }
}
