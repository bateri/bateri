//! bt-core — terminal modelinin platformsuz çekirdeği.
//!
//! Buraya VT durum makinesi, grid ve scrollback, PTY, OSC ayrıştırma, komut
//! blokları, seçim ve ayar modeli gelir (002+). Sözleşme: bu crate hiçbir
//! platform kütüphanesi görmez — `objc2*`, `core-text`, `metal` yok — ve
//! Linux'ta derlenebilir kalır; Vulkan kapısı bu ayrımın üstüne kurulur.
//!
//! Denetim `/audit` mercek 1'dedir (`.claude/skills/audit/SKILL.md`) ve
//! bağımlılık düzeyinde bir vekildir; gerçek kapı
//! `--target x86_64-unknown-linux-gnu` ile derlemedir, `rustup` gelene kadar kapalı.
