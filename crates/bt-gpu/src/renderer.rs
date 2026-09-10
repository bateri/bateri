//! Renderer: metallib'i yükler, pipeline'ı kurar, verilen drawable'a bir kare
//! çizer. Drawable'ı kimin sağladığını bilmez; kare sayacını yalnız sunulan
//! kare artırır.

use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

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
    /// Pipeline bu formata derlendi; `surface()` layer'ı aynı formatta kurar,
    /// ikisinin ayrışması yapısal olarak imkânsız kalsın.
    pixel_format: MTLPixelFormat,
    /// Sunulan kare sayısı. `Relaxed` yeter: yazan `draw` (birden çok thread
    /// olsa da `fetch_add` sayım kaybetmez), okuyan `make duman` yalnız "> 0"
    /// sorar; happens-before gereksinimi yok. Atomik olması 002'de display
    /// link kuyruğundan yazılıp ana thread'den okunabilsin diye.
    frames: AtomicU64,
}

impl Renderer {
    /// Sistem varsayılan device ile; bt-shell yalnız bunu çağırır ve
    /// `objc2-metal`'i hiç görmez.
    pub fn system_default() -> Result<Self, GpuError> {
        let device = MTLCreateSystemDefaultDevice().ok_or(GpuError::NoDevice)?;
        Self::new(device, MTLPixelFormat::BGRA8Unorm)
    }

    /// Pixel format doğrulanmaz: layer ya da pipeline'ın reddettiği bir format
    /// ObjC istisnasıyla süreci düşürür, `GpuError` dönmez. Bu yüzden crate-içi;
    /// dış dünya `system_default` ile `BGRA8Unorm` alır.
    pub(crate) fn new(
        device: Retained<ProtocolObject<dyn MTLDevice>>,
        pixel_format: MTLPixelFormat,
    ) -> Result<Self, GpuError> {
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
        unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(pixel_format);
        let cell_bg = device
            .newRenderPipelineStateWithDescriptor_error(&desc)
            .map_err(GpuError::Pipeline)?;
        let queue = device.newCommandQueue().ok_or(GpuError::NoCommandQueue)?;

        Ok(Self {
            device,
            queue,
            cell_bg,
            pixel_format,
            frames: AtomicU64::new(0),
        })
    }

    pub fn surface(&self) -> Surface {
        Surface::new(&self.device, self.pixel_format)
    }

    /// Sunulan (`presentDrawable` + `commit`) kare sayısı; `make duman` bunu okur.
    /// İskelette `waitUntilCompleted` sayesinde "GPU bitirdi" ile çakışır;
    /// 002 asenkron olunca bu anlam `addCompletedHandler`'a taşınır.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Yüzeyden bir drawable alıp [`Renderer::draw`]'a verir. Drawable'ı kimin
    /// sağladığını `draw` bilmez: phase-3'te display link hazır drawable'ı
    /// verir ve bu sarmalayıcıyı atlar.
    pub fn draw_surface(
        &self,
        surface: &Surface,
        clear: [f32; 4],
        frame: &Frame,
    ) -> Result<(), GpuError> {
        // Drawable autorelease'li döner ve objc2'nin sahipliği devralması
        // "best effort"; havuz burada olmazsa run loop'suz bir thread'de
        // drawable asılı kalır, layer'ın 3'lük havuzu tükenir ve sonraki
        // `nextDrawable` bloklar.
        autoreleasepool(|_| {
            let drawable = surface.layer().nextDrawable().ok_or(GpuError::NoDrawable)?;
            self.draw(&drawable, clear, frame)
        })
    }

    /// Tek kare: arka planı `clear` ile boya, `frame`'in dikdörtgenlerini çiz, sun.
    ///
    /// Bu phase'de senkron (`waitUntilCompleted`) — sayaç "GPU bitirdi" demek
    /// olsun; phase-3 display link gelince asenkron olur.
    pub fn draw(
        &self,
        drawable: &ProtocolObject<dyn CAMetalDrawable>,
        clear: [f32; 4],
        frame: &Frame,
    ) -> Result<(), GpuError> {
        // Metal/CA çağrıları iç geçicileri autorelease havuzuna atar; kare
        // başına bir havuz, run loop'suz bir thread'den çağrılınca birikimi önler.
        autoreleasepool(|_| {
            let cmd = self
                .queue
                .commandBuffer()
                .ok_or(GpuError::NoCommandBuffer)?;
            self.encode_pass(&cmd, &drawable.texture(), clear, frame)?;

            cmd.presentDrawable(drawable.as_ref());
            cmd.commit();
            cmd.waitUntilCompleted();
            // Tamamlanmak sunulmak değildir: Error durumunda drawable boş kalır
            // ve sayaç artarsa `make duman` siyah pencereyi yeşil geçer.
            if cmd.status() == MTLCommandBufferStatus::Error {
                return Err(GpuError::CommandFailed(cmd.error()));
            }
            self.frames.fetch_add(1, Ordering::Relaxed);
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
    fn hedef_doku(
        device: &ProtocolObject<dyn MTLDevice>,
        kenar: usize,
    ) -> Retained<ProtocolObject<dyn MTLTexture>> {
        let desc = unsafe {
            MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
                MTLPixelFormat::BGRA8Unorm,
                kenar,
                kenar,
                false,
            )
        };
        desc.setUsage(MTLTextureUsage::RenderTarget);
        desc.setStorageMode(MTLStorageMode::Shared);
        device
            .newTextureWithDescriptor(&desc)
            .expect("offscreen doku")
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
        let texture = hedef_doku(&r.device, KENAR);

        // 8×8 hücre, viewport 16×16 → dört çeyrek. Sol üstte kırmızı, sağ
        // altta yeşil, sağ üst boş. İki instance şart: tek instance `inst[0]`
        // stride'dan bağımsız okunur, yani stride hatası (32'den kayma) tek
        // instance'la GÖRÜNMEZ. İkincisi ancak doğru stride ile bulunur.
        // y ters çevirme bozuksa kırmızı ile yeşil yer değiştirir.
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

        let cmd = r.queue.commandBuffer().expect("komut tamponu");
        r.encode_pass(&cmd, &texture, [0.0, 0.0, 1.0, 1.0], &frame)
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

        // BGRA8Unorm: bayt sırası B, G, R, A.
        let piksel = |x: usize, y: usize| {
            let i = (y * KENAR + x) * 4;
            (pikseller[i + 2], pikseller[i + 1], pikseller[i])
        };
        assert_eq!(piksel(2, 2), (255, 0, 0), "ilk hücre sol üstte kırmızı");
        assert_eq!(piksel(12, 12), (0, 255, 0), "ikinci hücre sağ altta yeşil");
        assert_eq!(piksel(12, 2), (0, 0, 255), "boş çeyrek clear rengi kalmalı");
    }
}
