//! bt-gpu — Metal renderer: shader'lar, çizim yüzeyi, hareket, overlay'ler.
//!
//! `bt-core`'dan "ne çizileceğini" alır, "ne anlama geldiğini" bilmez:
//! escape dizisi tanıyan bir dal buraya girmez (`CLAUDE.md` → tuzaklar).
//! `CAMetalLayer`'ın sahibi bu crate'tir; `bt-shell` yalnız `&CALayer` alır
//! ve `objc2-metal`'i hiç görmez — device'ı `Renderer::system_default` kurar.
//! Kareyi de bu crate sürer: [`DisplayLink`] `Session`'dan okur, çizer ve
//! hasar tükenince durur; `bt-shell` yalnız pencereyi ve [`Waker`]'ı bağlar.

mod error;
mod frame;
mod link;
mod renderer;
mod surface;

pub use error::GpuError;
pub use link::{DisplayLink, Waker};
pub use renderer::Renderer;
pub use surface::Surface;
