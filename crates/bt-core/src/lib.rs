//! bt-core — terminal modelinin platformsuz çekirdeği.
//!
//! VT durum makinesi, grid, scrollback, PTY ve okuyucu thread burada yaşar;
//! `alacritty_terminal` **kapsüllüdür**: `pub` API'de alacritty tipi görünmez,
//! dışarısı yalnız `Session`, `Cell`, `UnderlineStyle`, `Cursor`,
//! `SelectionPoint`, `CellHalf`, `Arrow`, `Wheel`, `LinearRgba`, `Theme`,
//! `Wake` ve ayar modelinin `Settings`, `Parsed`, `Diagnostic`'i görür (tam
//! liste aşağıdaki `pub use` bloğu). `Osc52` alacritty'nin aynı adlı tipinin
//! karşılığı, kendisi değil. Kendi grid'imize geçiş (00X) bu sınırın
//! arkasında yapılır ve renderer'ı bilmez. `toml_edit` de aynı biçimde içeride
//! kalır: ayar modelinin ve tema dosyasının `pub` yüzünde TOML tipi yok;
//! temanın renkleri `0xRRGGBB`, alacritty'nin `Rgb`'si değil.
//!
//! Sözleşme: bu crate macOS'a özgü hiçbir kütüphane görmez — `objc2*`,
//! `core-text`, `metal` yok — ve Linux'ta derlenebilir kalır; Vulkan kapısı
//! bu ayrımın üstüne kurulur. Unix PTY (`libc`, `rustix`) serbesttir, o kapıyı
//! kapatmaz.
//!
//! Denetim `make denetim`'dedir (`Makefile`, her `make hepsi`'de koşar) ve
//! bağımlılık düzeyinde bir vekildir; gerçek kapı
//! `--target x86_64-unknown-linux-gnu` ile derlemedir, `rustup` gelene kadar kapalı.

mod color;
mod input;
mod session;
mod settings;
mod theme;
mod wake;

pub use color::{LinearRgba, Theme};
pub use input::Arrow;
pub use session::{
    Cell, CellHalf, Cursor, DirtyFlag, Osc52, SHUTDOWN_GRACE, SelectionPoint, Session,
    SessionOptions, Teardown, TerminalOptions, UnderlineStyle, Wheel, load_shell, smoke_shell,
};
pub use settings::{Changes, Diagnostic, FontOptions, Parsed, SYSTEM_THEME, Settings};
pub use wake::Wake;

/// Hücre sabit boyuttadır ve sabit burada bağlanır: **alacritty'nin** hücresi
/// (bizim [`Cell`]'imiz değil — o bir kare çıktısı, bu bir grid kaydı) =
/// `c` 4 + `fg` 4 + `bg` 4 + `flags` 2 + dolgu + `Option<Arc<CellExtra>>` 8
/// = 24 bayt.
/// Seyrek veri (grapheme kümesi, alt çizgi rengi, hyperlink) zaten yan
/// tabloda — `CellExtra`. Bu sayı değişirse `CLAUDE.md`'nin hücre maddesi
/// aynı commit'te değişir: 10 000 satırlık scrollback'i sekme başına
/// büyüten şey budur.
///
/// Kapsamı dürüstçe: ölçülen tip bizim değil, bu assert bu depodaki
/// hiçbir hareketi engellemez — bir **sürüm kanaryasıdır**, `cargo update`
/// hücre başına belleği sessizce değiştirirse derlemeyi kırıp kararı insana
/// verir. Kendi hücremiz geldiğinde (00X) assert ona taşınır.
const _: () = assert!(size_of::<alacritty_terminal::term::cell::Cell>() == 24);
