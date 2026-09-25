//! Ayrıştırıcı ile `Term` arasındaki sarmalayıcı.
//!
//! Okuyucu döngü ([`crate::reader`]) `Term`'i ayrıştırıcıya doğrudan değil
//! bu tipin içinden veriyor: kümeleme (035) `input`'lar **arasına** girmek
//! ve araya giren her başka çağrıda kümeyi kapatmak zorunda, yani
//! ayrıştırıcının `Handler` çağrılarının hepsini görmeli. Bugün (phase-2)
//! her çağrı olduğu gibi `Term`'e gidiyor — davranış bayt bayt aynı.
//!
//! **Aktarım listesi tek makroda ve bekçili.** `Handler`'ın her metodunun
//! boş bir varsayılanı var; listeden düşen bir metot derlenir ama
//! `Term`'in uygulamasına hiç ulaşmaz ve belirtisi sessizdir (ör. bir
//! kaçış dizisi yok sayılır). `impl`'in üstündeki
//! `clippy::missing_trait_methods` o düşüşü `make clippy`'de kırmızıya
//! çeviriyor; vte'ye yeni bir metot gelirse de aynı yerden.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{
    Attr, CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, Hyperlink, KeyboardModes,
    KeyboardModesApplyBehavior, LineClearMode, Mode, ModifyOtherKeys, PrivateMode, Rgb,
    ScpCharPath, ScpUpdateMode, StandardCharset, TabulationClearMode,
};
// vte bu tipi yeniden ihraç etmiyor; kenarın gerekçesi kök `Cargo.toml`'da.
use cursor_icon::CursorIcon;

/// Ayrıştırıcının gördüğü `Handler`: `Term`'i ödünç alıyor ve çağrıları
/// ona aktarıyor. **Her `advance` ve `stop_sync` çağrısında** yeniden
/// doğuyor — bir kilit turunda birden çok `advance` olabilir (`pty_read`'in
/// okuma döngüsü) — yani kendi durumu yok: iki `read` parçasına bölünen bir
/// kümenin durumu (phase-3) döngünün `State`'inde yaşamak zorunda.
pub(crate) struct ClusterHandler<'a, U: EventListener> {
    term: &'a mut Term<U>,
}

impl<'a, U: EventListener> ClusterHandler<'a, U> {
    pub(crate) fn new(term: &'a mut Term<U>) -> Self {
        Self { term }
    }
}

/// `input` **dışındaki** bütün `Handler` metotlarını `Term`'e aktarır.
/// `input` elle yazılı, çünkü kümelemenin girdiği tek kapı o.
macro_rules! forward {
    ($( fn $name:ident(&mut self $(, $arg:ident: $ty:ty)*); )*) => {
        $(
            #[inline]
            fn $name(&mut self $(, $arg: $ty)*) {
                self.term.$name($($arg),*)
            }
        )*
    };
}

#[deny(clippy::missing_trait_methods)]
impl<U: EventListener> Handler for ClusterHandler<'_, U> {
    #[inline]
    fn input(&mut self, c: char) {
        self.term.input(c)
    }

    forward! {
        fn set_title(&mut self, title: Option<String>);
        fn set_cursor_style(&mut self, style: Option<CursorStyle>);
        fn set_cursor_shape(&mut self, shape: CursorShape);
        fn goto(&mut self, line: i32, col: usize);
        fn goto_line(&mut self, line: i32);
        fn goto_col(&mut self, col: usize);
        fn insert_blank(&mut self, count: usize);
        fn move_up(&mut self, rows: usize);
        fn move_down(&mut self, rows: usize);
        fn identify_terminal(&mut self, intermediate: Option<char>);
        fn device_status(&mut self, arg: usize);
        fn move_forward(&mut self, cols: usize);
        fn move_backward(&mut self, cols: usize);
        fn move_down_and_cr(&mut self, rows: usize);
        fn move_up_and_cr(&mut self, rows: usize);
        fn put_tab(&mut self, count: u16);
        fn backspace(&mut self);
        fn carriage_return(&mut self);
        fn linefeed(&mut self);
        fn bell(&mut self);
        fn substitute(&mut self);
        fn newline(&mut self);
        fn set_horizontal_tabstop(&mut self);
        fn scroll_up(&mut self, rows: usize);
        fn scroll_down(&mut self, rows: usize);
        fn insert_blank_lines(&mut self, rows: usize);
        fn delete_lines(&mut self, rows: usize);
        fn erase_chars(&mut self, count: usize);
        fn delete_chars(&mut self, count: usize);
        fn move_backward_tabs(&mut self, count: u16);
        fn move_forward_tabs(&mut self, count: u16);
        fn save_cursor_position(&mut self);
        fn restore_cursor_position(&mut self);
        fn clear_line(&mut self, mode: LineClearMode);
        fn clear_screen(&mut self, mode: ClearMode);
        fn clear_tabs(&mut self, mode: TabulationClearMode);
        fn set_tabs(&mut self, interval: u16);
        fn reset_state(&mut self);
        fn reverse_index(&mut self);
        fn terminal_attribute(&mut self, attr: Attr);
        fn set_mode(&mut self, mode: Mode);
        fn unset_mode(&mut self, mode: Mode);
        fn report_mode(&mut self, mode: Mode);
        fn set_private_mode(&mut self, mode: PrivateMode);
        fn unset_private_mode(&mut self, mode: PrivateMode);
        fn report_private_mode(&mut self, mode: PrivateMode);
        fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>);
        fn set_keypad_application_mode(&mut self);
        fn unset_keypad_application_mode(&mut self);
        fn set_active_charset(&mut self, index: CharsetIndex);
        fn configure_charset(&mut self, index: CharsetIndex, charset: StandardCharset);
        fn set_color(&mut self, index: usize, color: Rgb);
        fn dynamic_color_sequence(&mut self, prefix: String, index: usize, terminator: &str);
        fn reset_color(&mut self, index: usize);
        fn clipboard_store(&mut self, clipboard: u8, base64: &[u8]);
        fn clipboard_load(&mut self, clipboard: u8, terminator: &str);
        fn decaln(&mut self);
        fn push_title(&mut self);
        fn pop_title(&mut self);
        fn text_area_size_pixels(&mut self);
        fn text_area_size_chars(&mut self);
        fn set_hyperlink(&mut self, hyperlink: Option<Hyperlink>);
        fn set_mouse_cursor_icon(&mut self, icon: CursorIcon);
        fn report_keyboard_mode(&mut self);
        fn push_keyboard_mode(&mut self, mode: KeyboardModes);
        fn pop_keyboard_modes(&mut self, to_pop: u16);
        fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior);
        fn set_modify_other_keys(&mut self, mode: ModifyOtherKeys);
        fn report_modify_other_keys(&mut self);
        fn set_scp(&mut self, char_path: ScpCharPath, update_mode: ScpUpdateMode);
    }
}
