//! bt-core — terminal modelinin platformsuz çekirdeği.
//!
//! VT durum makinesi, grid, scrollback, PTY ve okuyucu thread burada yaşar;
//! `alacritty_terminal` **kapsüllüdür**: `pub` API'de alacritty tipi görünmez,
//! dışarısı yalnız `Session`, `CellBg`, `Cursor` ve `Wake` görür. Kendi
//! grid'imize geçiş (00X) bu sınırın arkasında yapılır ve renderer'ı bilmez.
//!
//! Sözleşme: bu crate macOS'a özgü hiçbir kütüphane görmez — `objc2*`,
//! `core-text`, `metal` yok — ve Linux'ta derlenebilir kalır; Vulkan kapısı
//! bu ayrımın üstüne kurulur. Unix PTY (`libc`, `rustix`) serbesttir, o kapıyı
//! kapatmaz.
//!
//! Denetim `/audit` mercek 1'dedir (`.claude/skills/audit/SKILL.md`) ve
//! bağımlılık düzeyinde bir vekildir; gerçek kapı
//! `--target x86_64-unknown-linux-gnu` ile derlemedir, `rustup` gelene kadar kapalı.

mod color;
mod session;
mod wake;

pub use color::{DEFAULT_BG, DEFAULT_CURSOR};
pub use session::{CellBg, Cursor, Session, SessionOptions};
pub use wake::Wake;

/// Hücre sabit boyuttadır ve sabit burada bağlanır: alacritty `Cell` = `c` 4 +
/// `fg` 4 + `bg` 4 + `flags` 2 + dolgu + `Option<Arc<CellExtra>>` 8 = 24 bayt.
/// Seyrek veri (grapheme kümesi, alt çizgi rengi, hyperlink) zaten yan
/// tabloda — `CellExtra`. Bu sayı değişirse `CLAUDE.md`'nin hücre maddesi
/// aynı commit'te değişir: 10 000 satırlık scrollback'i sekme başına
/// büyüten şey budur.
///
/// Kapsamı dürüstçe: `Cell` bizim tipimiz değil, bu assert bu depodaki
/// hiçbir hareketi engellemez — bir **sürüm kanaryasıdır**, `cargo update`
/// hücre başına belleği sessizce değiştirirse derlemeyi kırıp kararı insana
/// verir. Kendi hücremiz geldiğinde (00X) assert ona taşınır.
const _: () = assert!(size_of::<alacritty_terminal::term::cell::Cell>() == 24);
