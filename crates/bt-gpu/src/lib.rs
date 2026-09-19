//! bt-gpu — Metal renderer: shader'lar, çizim yüzeyi, hareket, overlay'ler.
//!
//! `bt-core`'dan "ne çizileceğini" alır, "ne anlama geldiğini" bilmez:
//! escape dizisi tanıyan bir dal buraya girmez (`CLAUDE.md` → tuzaklar).
//! `CAMetalLayer`'ın sahibi bu crate'tir; `bt-shell` yalnız `&CALayer` alır
//! ve `objc2-metal`'i hiç görmez — device'ı `Renderer::system_default` kurar.
//! Kareyi de bu crate sürer: [`DisplayLink`] `Session`'dan okur, çizer ve
//! hasar tükenince durur; `bt-shell` yalnız pencereyi ve [`Waker`]'ı bağlar.
//! Hücre metriği de buradan geçer ([`Renderer::cell_metrics`]): kaynağı
//! `bt-atlas`'ın font ölçüsüdür ama `bt-shell` o crate'i görmez, [`CellMetrics`]
//! görür — katman tablosu iki kenarla değil bir kenarla yaşar. Atlasın
//! **dokusu** da bu crate'in: `bt-atlas` yuva numarası ve CPU bitmap'i verir,
//! `replaceRegion` ile dokuya yazan ve `cell` pipeline'ıyla çizen buradır.
//!
//! **Üç pipeline var** ve ikisi vertex'i paylaşıyor: `cell_bg` (arka planlar,
//! blok şeritleri, dock zemini), `cell` (glyph'ler ve kurallar; atlası
//! örnekliyor) ve `caret` (`cell_bg_vertex` + `caret_fragment`). Üçüncüsü
//! ayrı, çünkü caret'in yuvarlak köşesi, kenarı ve halesi bir SDF istiyor ve
//! o hesabı kare başına binlerce arka plan dörtgenine ödetmenin anlamı yok.
//! Kare yolunun **ölçüm defteri** de burada ([`Stats`]): zamanı kim
//! üretiyorsa örneği de o topluyor — CPU aralıkları display link'ten, GPU
//! deltası Metal'in tamamlanma bloğundan. Bu crate hiçbir şey **basmaz**;
//! defteri kuran ve kapanışta okuyan `bt-shell`'dir. İstatistiğin kuralı da
//! burada: p95'in anlamlı olduğu en küçük örnek sayısı ([`MIN_SAMPLES`])
//! defterin yanında tanımlı, `bt-shell` onu `taban=` diye **basıyor** ama
//! değerini kendisi seçmiyor.

mod blink;
mod error;
mod frame;
mod link;
mod motion;
mod renderer;
mod stats;
mod surface;

pub use error::GpuError;
pub use frame::{DOCK_ROWS, dock_px};
pub use link::{DisplayLink, Layout, Origin, Waker};
pub use renderer::{CellMetrics, FontNotice, Renderer};
pub use stats::{MIN_SAMPLES, Samples, Stats};
pub use surface::Surface;
