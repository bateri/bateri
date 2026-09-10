//! bt-gpu — Metal renderer: shader'lar, çizim yüzeyi, hareket, overlay'ler.
//!
//! `bt-core`'dan "ne çizileceğini" alır, "ne anlama geldiğini" bilmez:
//! escape dizisi tanıyan bir dal buraya girmez (`CLAUDE.md` → tuzaklar).
//! `CAMetalLayer`'ın sahibi bu crate'tir; `bt-shell` yalnız `&CALayer` alır.
//! Gövde phase-2'de dolar.
