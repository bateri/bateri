//! Ayrıştırıcı ile `Term` arasındaki sarmalayıcı.
//!
//! Okuyucu döngü ([`crate::reader`]) `Term`'i ayrıştırıcıya doğrudan değil
//! bu tipin içinden veriyor: kümeleme (035) `input`'lar **arasına** girmek
//! ve araya giren her başka çağrıda kümeyi kapatmak zorunda, yani
//! ayrıştırıcının `Handler` çağrılarının hepsini görmeli. Kümeleme kapalıyken
//! (`SessionOptions::cluster`) her çağrı olduğu gibi `Term`'e gidiyor —
//! davranış bayt bayt alacritty'ninki.
//!
//! **Kümeleme `input`'ta ve yalnız orada.** Gelen kod noktası açık kümeyi
//! uzatmıyorsa `Term::input`'a gidiyor; uzatıyor ve kümenin sütunu
//! değişmiyorsa baş hücrenin `zerowidth`'ine iniyor; kümeyi bir sütundan
//! ikiye çıkarıyorsa baş hücre alacritty'nin **kendi** geniş yolundan
//! yeniden yazılıyor (bkz. [`ClusterHandler::widen`]). Kural
//! [`crate::cluster`]'da.
//!
//! **Açık kümenin konumu saklanmıyor, ızgaradan türetiliyor** — alacritty'nin
//! `zerowidth` dalının yöntemiyle (imleç − 1, bekleyen sarmada imlecin
//! kendisi, spacer'dan geri). Okumalar parça parça geliyor ve arada ana
//! thread `Term`'i değiştirebiliyor (resize, ⌘K); saklanan bir konum o
//! aralıkta bayatlardı, türetilen konum alacritty'nin bugünkü `zerowidth`
//! yolunun açıklığıyla aynı açıklıkta. Tek durum "son `Handler` çağrısı
//! `input` mıydı" biti ve döngünün `State`'inde yaşıyor, çünkü bu tip her
//! `advance`'te yeniden doğuyor; öteki bütün aktarımlar biti düşürüyor —
//! araya giren bir `CUP` ya da SGR kümeyi kapatıyor.
//!
//! **Aktarım listesi tek makroda ve bekçili.** `Handler`'ın her metodunun
//! boş bir varsayılanı var; listeden düşen bir metot derlenir ama
//! `Term`'in uygulamasına hiç ulaşmaz ve belirtisi sessizdir (ör. bir
//! kaçış dizisi yok sayılır). `impl`'in üstündeki
//! `clippy::missing_trait_methods` o düşüşü `make clippy`'de kırmızıya
//! çeviriyor; vte'ye yeni bir metot gelirse de aynı yerden.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};
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
/// kümenin tek biti (`last_input`) döngünün `State`'inde yaşıyor.
pub(crate) struct ClusterHandler<'a, U: EventListener> {
    term: &'a mut Term<U>,
    /// Kümeleme açık mı — `SessionOptions::cluster`'ın değeri.
    cluster: bool,
    /// Son `Handler` çağrısı `input` mıydı: açık bir küme var mı.
    last_input: &'a mut bool,
}

impl<'a, U: EventListener> ClusterHandler<'a, U> {
    pub(crate) fn new(term: &'a mut Term<U>, cluster: bool, last_input: &'a mut bool) -> Self {
        Self {
            term,
            cluster,
            last_input,
        }
    }

    /// Açık kümenin baş hücresi — alacritty'nin `zerowidth` dalının
    /// (`Term::input`) yöntemi: bekleyen sarmada imlecin kendi hücresi,
    /// değilse bir solundaki; spacer'a düşerse geniş hücreye geri.
    ///
    /// Sütun ayrıca kırpılıyor: imleç alacritty'nin sözleşmesiyle ızgaranın
    /// içinde, ama `bt-core`'da indeksleme paniği de yasak.
    fn head(&self) -> Point {
        let grid = self.term.grid();
        let cursor = &grid.cursor;
        let mut column = cursor.point.column;
        if !cursor.input_needs_wrap {
            column.0 = column.saturating_sub(1);
        }
        column.0 = column.0.min(grid.columns().saturating_sub(1));
        let line = cursor.point.line;
        if grid[line][column].flags.contains(Flags::WIDE_CHAR_SPACER) {
            column.0 = column.saturating_sub(1);
        }
        Point::new(line, column)
    }

    /// Baş hücrenin kümesi: taban karakter + `zerowidth`.
    fn open(&self, at: Point) -> String {
        let cell = &self.term.grid()[at.line][at.column];
        let mut open = String::new();
        open.push(cell.c);
        open.extend(cell.zerowidth().unwrap_or_default());
        open
    }

    /// Dar baş hücreyi geniş hücreye çevirir ve kümeyi (`open`, uzamış hâli)
    /// ona yazar.
    ///
    /// **Genişleme alacritty'nin kendi geniş yolundan**: imleç baş hücreye
    /// geri alınıyor ve `Term::input` bir yer tutucu geniş karakterle
    /// çağrılıyor — satır sonunun `LEADING_WIDE_CHAR_SPACER`'ı, kaydırma
    /// bölgesinin dibi ve DECAWM alacritty'de kalıyor, özel yolları
    /// (`write_at_cursor`, `wrapline`) yeniden yazılmıyor. Yazılan hücrenin
    /// şablonu (renk, bayrak, bağlantı) imlecin şablonu: baş hücreyi yazan
    /// da oydu, çünkü aradaki bir SGR kümeyi kapatırdı.
    ///
    /// **IRM'de baş hücrenin girişi önce geri alınıyor** (`delete_chars`):
    /// dar baş hücre yazılırken satır bir sütun kaydı, yer tutucu iki sütun
    /// daha kaydırırdı — iki sütunlu küme komşularını üç sütun iterdi.
    fn widen(&mut self, at: Point, open: &str) {
        let cursor = &mut self.term.grid_mut().cursor;
        cursor.point = at;
        cursor.input_needs_wrap = false;
        if self.term.mode().contains(TermMode::INSERT) {
            self.term.delete_chars(1);
        }
        self.term.input(WIDE_PLACEHOLDER);
        // Yazılan hücre yine aynı yöntemle: satır sonunda alt satıra inmiş
        // olabilir. DECAWM kapalıyken son sütunda alacritty hiçbir şey
        // yazmadan dönüyor; o hâlde bulunan hücre dar kalıyor ve küme onun
        // üstüne iniyor — sütun eksik ama glyph kaybolmuyor.
        let at = self.head();
        let cell = &mut self.term.grid_mut()[at.line][at.column];
        let mut chars = open.chars();
        cell.c = chars.next().unwrap_or(' ');
        chars.for_each(|c| cell.push_zerowidth(c));
    }
}

/// Genişlemenin yer tutucusu: `Term::input`'a iki sütunluk bir kod noktası
/// lazım ve hücrenin `c`'si hemen ardından taban karaktere dönüyor. Değeri
/// önemsiz; alacritty'nin charset eşlemesinin (DEC özel grafikleri)
/// dokunmadığı bir geniş karakter.
const WIDE_PLACEHOLDER: char = '\u{3000}';

/// `input` **dışındaki** bütün `Handler` metotlarını `Term`'e aktarır.
/// `input` elle yazılı, çünkü kümelemenin girdiği tek kapı o.
macro_rules! forward {
    ($( fn $name:ident(&mut self $(, $arg:ident: $ty:ty)*); )*) => {
        $(
            #[inline]
            fn $name(&mut self $(, $arg: $ty)*) {
                // Araya giren her çağrı açık kümeyi kapatıyor.
                *self.last_input = false;
                self.term.$name($($arg),*)
            }
        )*
    };
}

#[deny(clippy::missing_trait_methods)]
impl<U: EventListener> Handler for ClusterHandler<'_, U> {
    fn input(&mut self, c: char) {
        if !self.cluster {
            return self.term.input(c);
        }
        let open_cluster = std::mem::replace(self.last_input, true);
        if !open_cluster || !crate::cluster::may_extend(c) {
            return self.term.input(c);
        }
        let at = self.head();
        let mut open = self.open(at);
        if !crate::cluster::extends(&open, c) {
            return self.term.input(c);
        }
        let narrow = !self.term.grid()[at.line][at.column]
            .flags
            .contains(Flags::WIDE_CHAR);
        open.push(c);
        // Genişlik **her** uzamadan sonra soruluyor, sıfır genişlikli koldan
        // gelenler dahil: `1` + VS16 + `U+20E3` genişlemeyi VS16'da yapıyor.
        if narrow && crate::cluster::width(&open) >= 2 {
            self.widen(at, &open);
        } else {
            self.term.grid_mut()[at.line][at.column].push_zerowidth(c);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::{Column, Line};
    use alacritty_terminal::term::Config;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::Processor;

    fn term(cols: usize, rows: usize) -> Term<VoidListener> {
        Term::new(Config::default(), &TermSize::new(cols, rows), VoidListener)
    }

    /// `bytes`'ı tek `advance`'le sarmalayıcıdan geçirir.
    fn feed(term: &mut Term<VoidListener>, cluster: bool, bytes: &str) {
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        parser.advance(
            &mut ClusterHandler::new(term, cluster, &mut last_input),
            bytes.as_bytes(),
        );
    }

    /// Izgaranın satırları, hücre hücre: geniş hücre `[…]`, spacer `·`,
    /// satır sonu spacer'ı `↵`; hücrenin metni `c` + `zerowidth`. Sondaki
    /// boş hücreler atılıyor.
    fn rows(term: &Term<VoidListener>) -> Vec<String> {
        let grid = term.grid();
        (0..grid.screen_lines())
            .map(|line| {
                let mut cells: Vec<String> = (0..grid.columns())
                    .map(|col| {
                        let cell = &grid[Line(line as i32)][Column(col)];
                        let mut text = String::from(cell.c);
                        text.extend(cell.zerowidth().unwrap_or_default());
                        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                            "·".to_owned()
                        } else if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                            "↵".to_owned()
                        } else if cell.flags.contains(Flags::WIDE_CHAR) {
                            format!("[{text}]")
                        } else {
                            text
                        }
                    })
                    .collect();
                while cells.last().is_some_and(|cell| cell == " ") {
                    cells.pop();
                }
                cells.join("|")
            })
            .collect()
    }

    /// Aynı baytlar, kümeleme kapalı ve açık.
    fn both(cols: usize, rows_: usize, bytes: &str) -> (Vec<String>, Vec<String>) {
        let mut off = term(cols, rows_);
        feed(&mut off, false, bytes);
        let mut on = term(cols, rows_);
        feed(&mut on, true, bytes);
        (rows(&off), rows(&on))
    }

    #[test]
    fn emoji_sequences_become_one_wide_cell() {
        let mut t = term(20, 2);
        feed(
            &mut t,
            true,
            "🇹🇷 👍🏽 👨\u{200D}👩\u{200D}👧 ❤\u{FE0F} 🏳\u{FE0F}\u{200D}🌈",
        );
        assert_eq!(
            rows(&t)[0],
            "[🇹🇷]|·| |[👍🏽]|·| |[👨\u{200D}👩\u{200D}👧]|·| |[❤\u{FE0F}]|·| \
             |[🏳\u{FE0F}\u{200D}🌈]|·"
        );
        assert_eq!(t.grid().cursor.point.column, Column(14));
    }

    /// Emoji dışı küme, VS15, dar kümenin arkasındaki ten rengi ve tek RI
    /// bugünkü hücrelerini veriyor.
    #[test]
    fn non_clusters_keep_todays_cells() {
        for text in ["لا", "⌚\u{FE0E}", "a🏽", "🇹", "e\u{301}x", "a\u{200D}b"] {
            let (off, on) = both(10, 2, text);
            assert_eq!(on, off, "{text:?}");
        }
    }

    /// Kapalı bayrak alacritty'nin kendisi: dizi bugünkü gibi parçalı.
    #[test]
    fn the_flag_off_is_alacritty() {
        let mut t = term(10, 2);
        feed(&mut t, false, "🇹🇷👍🏽");
        assert_eq!(rows(&t)[0], "🇹|🇷|[👍]|·|[🏽]|·");
    }

    /// Genişlemenin sonucu alacritty'nin **kendi** geniş karakterinin
    /// sonucuyla aynı — son sütun, IRM ve kaydırma bölgesinin dibi. Kıyas
    /// kümeleme kapalıyken aynı yere basılan `👍`.
    fn widening_matches_a_native_wide_char(cols: usize, rows_: usize, before: &str) {
        let mut native = term(cols, rows_);
        feed(&mut native, false, &format!("{before}👍"));
        let mut widened = term(cols, rows_);
        feed(&mut widened, true, &format!("{before}❤\u{FE0F}"));
        let expected: Vec<String> = rows(&native)
            .into_iter()
            .map(|row| row.replace('👍', "❤\u{FE0F}"))
            .collect();
        assert_eq!(rows(&widened), expected, "{before:?}");
        assert_eq!(
            widened.grid().cursor.point,
            native.grid().cursor.point,
            "{before:?}"
        );
    }

    #[test]
    fn widening_at_the_last_column_wraps_like_alacritty() {
        widening_matches_a_native_wide_char(10, 3, "123456789");
        let mut t = term(10, 3);
        feed(&mut t, true, "123456789❤\u{FE0F}");
        assert_eq!(rows(&t)[0], "1|2|3|4|5|6|7|8|9|↵");
        assert_eq!(rows(&t)[1], "[❤\u{FE0F}]|·");
    }

    #[test]
    fn widening_under_irm_shifts_the_neighbours_by_two() {
        // `abcdef`, imleç 1. sütuna, IRM açık.
        widening_matches_a_native_wide_char(10, 2, "abcdef\r\x1b[C\x1b[4h");
        let mut t = term(10, 2);
        feed(&mut t, true, "abcdef\r\x1b[C\x1b[4h❤\u{FE0F}");
        assert_eq!(rows(&t)[0], "a|[❤\u{FE0F}]|·|b|c|d|e|f");
    }

    #[test]
    fn widening_at_the_bottom_of_the_scroll_region_scrolls_the_region() {
        // Bölge 1–3. satırlar, imleç 3. satırın son sütununda; 4. satır
        // bölgenin dışında ve yerinde kalmalı.
        let before = "top\x1b[4;1Hout\x1b[1;3r\x1b[3;10H";
        widening_matches_a_native_wide_char(10, 4, before);
        let mut t = term(10, 4);
        feed(&mut t, true, &format!("{before}❤\u{FE0F}"));
        assert_eq!(
            rows(&t),
            ["", " | | | | | | | | |↵", "[❤\u{FE0F}]|·", "o|u|t"],
            "bölge bir satır kaydı, `top` gitti, `out` yerinde"
        );
    }

    /// DECAWM kapalıyken son sütunda alacritty geniş karakteri yazmıyor;
    /// genişleme panik yerine kümeyi dar hücrede bırakıyor.
    #[test]
    fn widening_without_autowrap_keeps_the_cluster_narrow() {
        let mut t = term(10, 2);
        feed(&mut t, true, "\x1b[?7l123456789❤\u{FE0F}");
        assert_eq!(rows(&t)[0], "1|2|3|4|5|6|7|8|9|❤\u{FE0F}");
    }

    #[test]
    fn an_intervening_call_closes_the_cluster() {
        let mut t = term(10, 2);
        // `CUP` imleci tam `👍`'nin arkasına koyuyor; yine de iki küme.
        feed(&mut t, true, "👍\x1b[1;3H🏽");
        assert_eq!(rows(&t)[0], "[👍]|·|[🏽]|·");
    }

    /// Küme ızgarada bir geniş karakterin yerini tutuyor — her genişlikte,
    /// satır sonuna düşen küme dahil (`👍🏽` son iki sütunda ya da sığmayıp
    /// alt satırda). `dock::tests`'in `grid_span` eşdeğerliğinin ızgara
    /// yarısı: ikisi birlikte bastırmanın aralığı ile ızgaranın
    /// ayrışmadığını söylüyor.
    #[test]
    fn a_cluster_takes_the_cells_of_one_wide_char_at_every_width() {
        let parts = [
            ("🇹🇷", "日"),
            ("👍🏽", "日"),
            ("x", "x"),
            ("👨\u{200D}👩\u{200D}👧", "日"),
            ("x", "x"),
            ("1\u{FE0F}\u{20E3}", "日"),
            ("❤\u{FE0F}", "日"),
            ("x", "x"),
        ];
        let clustered: String = parts.iter().map(|p| p.0).collect();
        let wide: String = parts.iter().map(|p| p.1).collect();
        for cols in 2..=9 {
            for lead in 0..cols {
                let before = " ".repeat(lead);
                let mut on = term(cols, 12);
                feed(&mut on, true, &format!("{before}{clustered}"));
                let mut native = term(cols, 12);
                feed(&mut native, false, &format!("{before}{wide}"));
                let heads = |t: &Term<VoidListener>| -> Vec<String> {
                    rows(t)
                        .into_iter()
                        .map(|row| {
                            row.split('|')
                                .map(|cell| match cell.strip_prefix('[') {
                                    Some(_) => "[W]".to_owned(),
                                    None if cell.chars().count() > 1 => "W?".to_owned(),
                                    None => cell.to_owned(),
                                })
                                .collect::<Vec<_>>()
                                .join("|")
                        })
                        .collect()
                };
                assert_eq!(heads(&on), heads(&native), "cols={cols} lead={lead}");
                assert_eq!(
                    on.grid().cursor.point,
                    native.grid().cursor.point,
                    "cols={cols} lead={lead}"
                );
            }
        }
    }

    /// İki `advance`'e bölünen küme kapanmıyor: bit döngünün `State`'inde.
    #[test]
    fn a_cluster_split_across_reads_stays_open() {
        let mut t = term(10, 2);
        let mut parser: Processor = Processor::new();
        let mut last_input = false;
        for part in ["🇹", "🇷", "👍", "🏽"] {
            parser.advance(
                &mut ClusterHandler::new(&mut t, true, &mut last_input),
                part.as_bytes(),
            );
        }
        assert_eq!(rows(&t)[0], "[🇹🇷]|·|[👍🏽]|·");
    }
}
