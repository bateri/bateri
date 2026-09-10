//! GPU yolunun hata tipi; her varyant adıyla söyler.

use std::fmt;

use objc2::rc::Retained;
use objc2_foundation::NSError;

/// GPU yolunun hataları. Hepsi adıyla söyler: `make duman`'ın kırmızısı ve
/// `bateri` main'in stderr satırı buradan gelir, sessiz `None` yok.
#[derive(Debug)]
pub enum GpuError {
    /// `MTLCreateSystemDefaultDevice` `None` döndü.
    NoDevice,
    Library(Retained<NSError>),
    /// metallib yüklendi ama adı verilen fonksiyon yok.
    MissingFunction(&'static str),
    Pipeline(Retained<NSError>),
    NoCommandQueue,
    NoCommandBuffer,
    /// Instance tamponu ayrılamadı (bellek baskısı).
    NoInstanceBuffer,
    /// Atlas dokusu ayrılamadı (bellek baskısı ya da doku sınırı).
    NoAtlasTexture,
    /// Glyph'i olan bir kare, atlası hiç kurulmamış bir renderer'a geldi.
    ///
    /// Atlasın anahtarı (punto + backing ölçeği) pencereden gelir ve onu
    /// kuran tek yer `Renderer::cell_metrics`. Bu hata "ölçeği söylemeden
    /// glyph çizmeye çalışıldı" demektir; alternatifi @1x bir atlas uydurup
    /// **sessizce** yanlış boyda çizmekti.
    NoAtlas,
    NoRenderEncoder,
    /// Komut tamponu `Error` durumuyla bitti (GPU hatası, zaman aşımı, cihaz
    /// kaybı); kare sunulmadı, sayaç artmaz. **Asenkron gelir:** `draw`
    /// çoktan `Ok` dönmüştür, bu hata tamamlanma kapanışına düşer.
    CommandFailed(Option<Retained<NSError>>),
}

impl fmt::Display for GpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => write!(f, "Metal cihazı yok (MTLCreateSystemDefaultDevice)"),
            Self::Library(e) => write!(f, "metallib yüklenemedi: {e}"),
            Self::MissingFunction(name) => write!(f, "shader fonksiyonu yok: {name}"),
            Self::Pipeline(e) => write!(f, "pipeline kurulamadı: {e}"),
            Self::NoCommandQueue => write!(f, "komut kuyruğu kurulamadı"),
            Self::NoCommandBuffer => write!(f, "komut tamponu alınamadı"),
            Self::NoInstanceBuffer => write!(f, "instance tamponu ayrılamadı"),
            Self::NoAtlasTexture => write!(f, "atlas dokusu ayrılamadı"),
            Self::NoAtlas => write!(f, "atlas kurulmadı: önce cell_metrics(scale) çağrılmalı"),
            Self::NoRenderEncoder => write!(f, "render encoder kurulamadı"),
            Self::CommandFailed(Some(e)) => write!(f, "komut tamponu hatayla bitti: {e}"),
            Self::CommandFailed(None) => write!(f, "komut tamponu hatayla bitti"),
        }
    }
}

impl std::error::Error for GpuError {}
