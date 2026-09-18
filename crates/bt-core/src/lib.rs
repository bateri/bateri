//! bt-core — terminal modelinin platformsuz çekirdeği.
//!
//! VT durum makinesi, grid, scrollback, PTY ve okuyucu thread burada yaşar;
//! `alacritty_terminal` **kapsüllüdür**: `pub` API'de alacritty tipi görünmez,
//! dışarısı yalnız `Session`, `Cell`, `UnderlineStyle`, `Cursor`, `Block`,
//! `Blocks`,
//! `SelectionPoint`, `CellHalf`, `Arrow`, `Wheel`, `LinearRgba`, `Theme`,
//! `ShellState`, `ShellPhase`, aynanın `DockState`, `DockStatus`, `DockFault`,
//! `Highlight`, `HighlightStyle`, `HighlightColor`'ı, bağlam satırının
//! `DockContext`'ini, dock yüzeyinin `Dock`'unu,
//! `Wake` ve ayar modelinin `Settings`, `Parsed`, `Diagnostic`, `CursorMotion`,
//! `ReduceMotion`'ı görür (tam
//! liste aşağıdaki `pub use` bloğu). `Osc52` alacritty'nin aynı adlı tipinin
//! karşılığı, kendisi değil. Kendi grid'imize geçiş (00X) bu sınırın
//! arkasında yapılır ve renderer'ı bilmez. `toml_edit` de aynı biçimde içeride
//! kalır: ayar modelinin ve tema dosyasının `pub` yüzünde TOML tipi yok;
//! temanın renkleri `0xRRGGBB`, alacritty'nin `Rgb`'si değil.
//!
//! Sınırın taşıdığı şey **karar**, piksel değil: `Cursor` imlecin yerini ve
//! bloğunun altında kalan metnin rengini veriyor ("imleç altındaki metin
//! okunur kalmalı" bir terminal semantiğidir), o rengi hangi piksellerin
//! alacağını çizen biliyor — **karar burada, boyama orada**. Ayrımın ölçütü
//! hücrenin bölünebilirliği: imleç bloğu iki hücre arasındayken sınır hücrenin
//! ortasından geçer ve burada verilecek bir hücre kararı onu göremez.
//! Komut bloğu (`Block`) aynı kuralın ikinci örneği: sınırdan satır aralığı ve
//! renk geçer, çıkış kodu geçmez — renderer'da escape dizisi ya da çıkış kodu
//! tanıyan bir dal yanlış yerdedir.
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
mod dock;
mod input;
mod session;
mod settings;
mod shell;
mod theme;
mod wake;

pub use color::{LinearRgba, Theme};
pub use dock::Dock;
pub use input::Arrow;
pub use session::{
    Block, Blocks, Cell, CellHalf, Cursor, DirtyFlag, Osc52, SHUTDOWN_GRACE, SelectionPoint,
    Session, SessionOptions, Teardown, TerminalOptions, UnderlineStyle, Wheel, load_shell,
    smoke_shell,
};
pub use settings::{
    CaretShape, Changes, CursorBlink, CursorMotion, Diagnostic, FontOptions, Parsed, ReduceMotion,
    SYSTEM_THEME, Settings, ShellIntegration,
};
pub use shell::{
    DockContext, DockFault, DockState, DockStatus, Highlight, HighlightColor, HighlightStyle,
    ShellPhase, ShellState,
};
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
