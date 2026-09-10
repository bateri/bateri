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
    NoDrawable,
    NoCommandBuffer,
    NoRenderEncoder,
    /// Komut tamponu `Error` durumuyla bitti (GPU hatası, zaman aşımı, cihaz
    /// kaybı); kare sunulmadı, sayaç artmaz.
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
            Self::NoDrawable => write!(f, "drawable alınamadı"),
            Self::NoCommandBuffer => write!(f, "komut tamponu alınamadı"),
            Self::NoRenderEncoder => write!(f, "render encoder kurulamadı"),
            Self::CommandFailed(Some(e)) => write!(f, "komut tamponu hatayla bitti: {e}"),
            Self::CommandFailed(None) => write!(f, "komut tamponu hatayla bitti"),
        }
    }
}

impl std::error::Error for GpuError {}
