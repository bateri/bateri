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
    MTLCommandBuffer, MTLCommandBufferStatus, MTLCommandEncoder, MTLCommandQueue,
    MTLCreateSystemDefaultDevice, MTLDevice, MTLLibrary, MTLLoadAction, MTLPixelFormat,
    MTLPrimitiveType, MTLRenderCommandEncoder, MTLRenderPassDescriptor,
    MTLRenderPipelineDescriptor, MTLRenderPipelineState, MTLStoreAction,
};
use objc2_quartz_core::CAMetalDrawable;

use crate::{GpuError, Surface};

/// build.rs'in ürettiği metallib; derleme zamanında gömülür, dosya yoksa
/// `rustc` düşer — çalışma zamanına kalan tek şey fonksiyon adlarıdır.
static METALLIB: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/default.metallib"));

/// `shaders/quad.metal` → `Uniforms` ile alan alan aynı: tek `float4`, 16 bayt.
/// Dış API `[f32; 4]` alır; bu tip yalnız encode sınırında yaşar.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct Uniforms {
    pub(crate) colour: [f32; 4],
}

pub struct Renderer {
    queue: Retained<ProtocolObject<dyn MTLCommandQueue>>,
    pipeline: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
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
            .newFunctionWithName(ns_string!("quad_vertex"))
            .ok_or(GpuError::MissingFunction("quad_vertex"))?;
        let fs = library
            .newFunctionWithName(ns_string!("quad_fragment"))
            .ok_or(GpuError::MissingFunction("quad_fragment"))?;

        let desc = MTLRenderPipelineDescriptor::new();
        desc.setVertexFunction(Some(&vs));
        desc.setFragmentFunction(Some(&fs));
        // SAFETY: indeks 0 her render pipeline'da vardır.
        unsafe { desc.colorAttachments().objectAtIndexedSubscript(0) }.setPixelFormat(pixel_format);
        let pipeline = device
            .newRenderPipelineStateWithDescriptor_error(&desc)
            .map_err(GpuError::Pipeline)?;
        let queue = device.newCommandQueue().ok_or(GpuError::NoCommandQueue)?;

        Ok(Self {
            queue,
            pipeline,
            pixel_format,
            frames: AtomicU64::new(0),
        })
    }

    pub fn surface(&self) -> Surface {
        // Device saklanmaz: kuyruk kendi device'ını taşır.
        Surface::new(&self.queue.device(), self.pixel_format)
    }

    /// Sunulan (`presentDrawable` + `commit`) kare sayısı; `make duman` bunu okur.
    /// İskelette `waitUntilCompleted` sayesinde "GPU bitirdi" ile çakışır;
    /// 002 asenkron olunca bu anlam `addCompletedHandler`'a taşınır.
    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    /// Yüzeyden bir drawable alıp [`Renderer::draw`]'a verir. Drawable'ı kimin
    /// sağladığını `draw` bilmez: 002'de display link hazır drawable'ı verir
    /// ve bu sarmalayıcıyı atlar.
    pub fn draw_surface(&self, surface: &Surface, colour: [f32; 4]) -> Result<(), GpuError> {
        // Drawable autorelease'li döner ve objc2'nin sahipliği devralması
        // "best effort"; havuz burada olmazsa run loop'suz bir thread'de
        // drawable asılı kalır, layer'ın 3'lük havuzu tükenir ve sonraki
        // `nextDrawable` bloklar.
        autoreleasepool(|_| {
            let drawable = surface.layer().nextDrawable().ok_or(GpuError::NoDrawable)?;
            self.draw(&drawable, colour)
        })
    }

    /// Tek kare: quad'ı çiz, sun. İskelette senkron (`waitUntilCompleted`) —
    /// sayaç "GPU bitirdi" demek olsun; 002 display link gelince asenkron olur.
    pub fn draw(
        &self,
        drawable: &ProtocolObject<dyn CAMetalDrawable>,
        colour: [f32; 4],
    ) -> Result<(), GpuError> {
        // Metal/CA çağrıları iç geçicileri autorelease havuzuna atar; kare
        // başına bir havuz, run loop'suz bir thread'den çağrılınca birikimi önler.
        autoreleasepool(|_| {
            let cmd = self
                .queue
                .commandBuffer()
                .ok_or(GpuError::NoCommandBuffer)?;

            let pass = MTLRenderPassDescriptor::new();
            // SAFETY: indeks 0 her render pass'te vardır.
            let att = unsafe { pass.colorAttachments().objectAtIndexedSubscript(0) };
            att.setTexture(Some(&drawable.texture()));
            att.setLoadAction(MTLLoadAction::DontCare); // quad her pikseli yazar
            att.setStoreAction(MTLStoreAction::Store);

            let enc = cmd
                .renderCommandEncoderWithDescriptor(&pass)
                .ok_or(GpuError::NoRenderEncoder)?;
            enc.setRenderPipelineState(&self.pipeline);
            let u = Uniforms { colour };
            // SAFETY: `u` bu blok boyunca yaşıyor ve Metal baytları encode anında
            // kopyalıyor; boyut `size_of::<Uniforms>()`, shader'daki float4 ile aynı.
            // Vertex buffer bağlı değil: `quad_vertex` köşeleri vertex_id'den
            // türetir, 3 köşe tam ekran üçgeninin tamamıdır.
            unsafe {
                enc.setFragmentBytes_length_atIndex(
                    NonNull::from(&u).cast(),
                    size_of::<Uniforms>(),
                    0,
                );
                enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
            }
            enc.endEncoding();
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metallib_gomulu_ve_gecerli() {
        // Metal kütüphanesi dosyası "MTLB" sihirli sayısıyla başlar.
        assert_eq!(&METALLIB[..4], b"MTLB");
    }

    #[test]
    fn library_ve_pipeline_kurulur() {
        // Device yoksa `ignored` değil açık hata: bu makinede Metal var,
        // yokluğu bir kusurdur.
        let r = Renderer::system_default().expect("Metal device ve pipeline");
        assert_eq!(r.frames(), 0);
    }
}
