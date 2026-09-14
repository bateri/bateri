//! Girdi kodlaması: ok tuşu ve tekerlek raporu → PTY baytları.
//!
//! **Saf ve kilitsiz.** Kip sorusu (`TermMode`) çağıranın `Term` kilidinde
//! cevaplanır, buraya yalnız cevabı gelir; kararın kendisi ve baytlar burada,
//! PTY'siz sınanıyor. Klavyenin okları ile tekerleğin okları aynı [`arrow`]'dan
//! geçer — ok baytı depoda tek yerde yazılı.

use alacritty_terminal::term::TermMode;

/// Ok tuşu: klavyenin dördü, tekerleğin ikisi.
///
/// `bt-shell` tuşu buna çevirir, baytı değil: biçim DECCKM'e bağlı ([`arrow`])
/// ve kip `Term`'de yaşıyor ([`crate::Session::write_arrow`]). Baytı `bt-shell`
/// yazsaydı kipi bilmesi, yani ya tutması ya da alacritty tipini görmesi
/// gerekirdi. Tekerleğin okları da aynı yoldan: DECCKM sorusu depoda tek yerde.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arrow {
    Up,
    Down,
    Right,
    Left,
}

/// Okun baytları: DECCKM (`\e[?1h`, `APP_CURSOR`) açıkken SS3 (`\eOA`),
/// kapalıyken CSI (`\e[A`).
///
/// İki biçimin sebebi `TERM`: `xterm-256color`'ın terminfo'su
/// `smkx=\E[?1h\E=` ve `kcuu1=\EOA` diyor, yani terminfo okuyan uygulama
/// (less, ncurses) açılışta DECCKM'i açar ve SS3 bekler; kipi açmayan
/// uygulama CSI bekler. alacritty'nin klavye bağları aynı ikiliyi taşıyor.
pub(crate) fn arrow(arrow: Arrow, mode: TermMode) -> [u8; 3] {
    let intro = if mode.contains(TermMode::APP_CURSOR) {
        b'O'
    } else {
        b'['
    };
    let last = match arrow {
        Arrow::Up => b'A',
        Arrow::Down => b'B',
        Arrow::Right => b'C',
        Arrow::Left => b'D',
    };
    [0x1b, intro, last]
}

/// Tekerleğin geriye (yukarı) düğmesi; ileriye (aşağı) [`WHEEL_DOWN`].
/// Tekerleğin bırakma olayı yok, rapor yalnız basmadır.
pub(crate) const WHEEL_UP: u8 = 64;
pub(crate) const WHEEL_DOWN: u8 = 65;

/// Fare raporunun kodlaması — uygulamanın DECSET 1006/1005 ile seçtiği.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MouseEncoding {
    /// 1006: ondalık, sınırsız.
    Sgr,
    /// 1005: koordinat UTF-8 karakteri, 2015'te kesilir.
    Utf8,
    /// X10/normal: koordinat tek bayt, 223'te kesilir.
    Normal,
}

/// Tekerleğin karar tablosu (006 `phase-3b.md` §1) — sıra tablonun sırası
/// ve alacritty'nin `scroll_terminal`'ınınkiyle aynı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WheelRoute {
    /// Uygulama fare raporu istedi (1000/1002/1003), ekran fark etmez.
    Report(MouseEncoding),
    /// Alternate screen + DECSET 1007, Shift basılı değil: ok tuşu (biçimi
    /// [`arrow`] kipten okuyor).
    Arrows,
    /// Alternate screen, ama 1007 kapalı ya da Shift basılı.
    Ignore,
    /// Birincil ekran: görünen pencere kayar.
    Scroll,
}

/// Kipten ve Shift'ten tekerleğin yolu.
pub(crate) fn wheel_route(mode: TermMode, shift: bool) -> WheelRoute {
    // `intersects`, `contains` değil: `MOUSE_MODE` üç bitin birleşimi ve
    // uygulama çoğu zaman yalnız birini açar (`\e[?1000h`).
    if mode.intersects(TermMode::MOUSE_MODE) {
        let encoding = if mode.contains(TermMode::SGR_MOUSE) {
            MouseEncoding::Sgr
        } else if mode.contains(TermMode::UTF8_MOUSE) {
            MouseEncoding::Utf8
        } else {
            MouseEncoding::Normal
        };
        WheelRoute::Report(encoding)
    } else if !mode.contains(TermMode::ALT_SCREEN) {
        WheelRoute::Scroll
    } else if mode.contains(TermMode::ALTERNATE_SCROLL) && !shift {
        WheelRoute::Arrows
    } else {
        WheelRoute::Ignore
    }
}

/// Tek tekerlek basışının raporu. `col`/`row` 0 tabanlı ve **uygulamanın**
/// ekranında (grid satırı, görünen pencere değil). Kodlamaya sığmayan
/// koordinatta `None` — rapor gönderilmez, kırpılmış bir hücreye de gitmez.
pub(crate) fn wheel_report(
    encoding: MouseEncoding,
    button: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    let (utf8, limit) = match encoding {
        MouseEncoding::Sgr => {
            // `u32`: `u16::MAX + 1` taşmasın.
            let (col, row) = (u32::from(col) + 1, u32::from(row) + 1);
            return Some(format!("\x1b[<{button};{col};{row}M").into_bytes());
        }
        MouseEncoding::Utf8 => (true, 2015),
        MouseEncoding::Normal => (false, 223),
    };
    // Sınırlar alacritty'ninki: düz kipte `32 + 1 + 222 = 255` son bayt,
    // UTF-8 kipinde `32 + 1 + 2014 = 2047` iki baytlık UTF-8'in son değeri.
    if col >= limit || row >= limit {
        return None;
    }
    let mut report = vec![0x1b, b'[', b'M', 32 + button];
    for pos in [col, row] {
        let value = 32 + 1 + u32::from(pos);
        if utf8 {
            // `128`'in altında tek bayt, üstünde iki — UTF-8'in kendisi
            // (alacritty `0xC0 + v/64`, `0x80 + v&63` diye elle yazıyor).
            // Aralıkta vekil kod noktası yok, `from_u32` düşmez.
            let mut buf = [0; 4];
            report.extend_from_slice(char::from_u32(value)?.encode_utf8(&mut buf).as_bytes());
        } else {
            // `limit` yukarıda `value <= 255`'i garanti ediyor.
            report.push(value as u8);
        }
    }
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrow_follows_decckm() {
        let (csi, ss3) = (TermMode::empty(), TermMode::APP_CURSOR);
        assert_eq!(&arrow(Arrow::Up, csi), b"\x1b[A");
        assert_eq!(&arrow(Arrow::Down, csi), b"\x1b[B");
        assert_eq!(&arrow(Arrow::Right, csi), b"\x1b[C");
        assert_eq!(&arrow(Arrow::Left, csi), b"\x1b[D");
        assert_eq!(&arrow(Arrow::Up, ss3), b"\x1bOA");
        assert_eq!(&arrow(Arrow::Down, ss3), b"\x1bOB");
        assert_eq!(&arrow(Arrow::Right, ss3), b"\x1bOC");
        assert_eq!(&arrow(Arrow::Left, ss3), b"\x1bOD");
    }

    #[test]
    fn mouse_mode_comes_first_on_either_screen() {
        // `MOUSE_MODE` üç bitin **birleşimi**: `contains` üçünü birden ister ve
        // yalnız `\e[?1000h` açan uygulamada yanlış dala düşerdi. Her bit tek
        // başına sınanıyor.
        for bit in [
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
        ] {
            for screen in [TermMode::empty(), TermMode::ALT_SCREEN] {
                // 1007 ve Shift fare kipini geçersiz kılmaz.
                let mode = bit | screen | TermMode::ALTERNATE_SCROLL;
                for shift in [false, true] {
                    assert_eq!(
                        wheel_route(mode, shift),
                        WheelRoute::Report(MouseEncoding::Normal),
                        "{mode:?} shift={shift}"
                    );
                }
            }
        }
    }

    #[test]
    fn mouse_encoding_follows_the_mode() {
        let click = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            wheel_route(click | TermMode::SGR_MOUSE, false),
            WheelRoute::Report(MouseEncoding::Sgr)
        );
        assert_eq!(
            wheel_route(click | TermMode::UTF8_MOUSE, false),
            WheelRoute::Report(MouseEncoding::Utf8)
        );
        // alacritty ikisini birbirini dışlayarak kuruyor; ikisi birden gelse
        // SGR kazanır (alacritty'nin `mouse_report`'u da önce SGR'ı soruyor).
        assert_eq!(
            wheel_route(click | TermMode::SGR_MOUSE | TermMode::UTF8_MOUSE, false),
            WheelRoute::Report(MouseEncoding::Sgr)
        );
    }

    #[test]
    fn alternate_screen_turns_the_wheel_into_arrows() {
        let alt = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(wheel_route(alt, false), WheelRoute::Arrows);
        // DECCKM yolu değiştirmiyor, yalnız okun biçimini (`arrow`).
        assert_eq!(
            wheel_route(alt | TermMode::APP_CURSOR, false),
            WheelRoute::Arrows
        );
        // Shift ve `\e[?1007l` oku keser; birincil ekrana da düşmez.
        assert_eq!(wheel_route(alt, true), WheelRoute::Ignore);
        assert_eq!(wheel_route(TermMode::ALT_SCREEN, false), WheelRoute::Ignore);
    }

    #[test]
    fn primary_screen_scrolls_whatever_else_is_set() {
        // 1007 yalnız alternate screen'de anlamlı; Shift birincil ekranda
        // kaydırmayı değiştirmiyor (alacritty de öyle).
        for mode in [
            TermMode::empty(),
            TermMode::ALTERNATE_SCROLL,
            TermMode::ALTERNATE_SCROLL | TermMode::APP_CURSOR,
        ] {
            for shift in [false, true] {
                assert_eq!(wheel_route(mode, shift), WheelRoute::Scroll, "{mode:?}");
            }
        }
    }

    #[test]
    fn sgr_report_is_decimal_and_one_based() {
        assert_eq!(
            wheel_report(MouseEncoding::Sgr, WHEEL_UP, 4, 2).unwrap(),
            b"\x1b[<64;5;3M"
        );
        assert_eq!(
            wheel_report(MouseEncoding::Sgr, WHEEL_DOWN, 0, 0).unwrap(),
            b"\x1b[<65;1;1M"
        );
        // Sınır yok ve `u16`'nın tepesinde `+ 1` taşmıyor.
        assert_eq!(
            wheel_report(MouseEncoding::Sgr, WHEEL_UP, u16::MAX, 2015).unwrap(),
            b"\x1b[<64;65536;2016M"
        );
    }

    #[test]
    fn normal_report_is_one_byte_per_coordinate_up_to_222() {
        // `32 + düğme`, `32 + 1 + konum`.
        assert_eq!(
            wheel_report(MouseEncoding::Normal, WHEEL_UP, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 96, 37, 35]
        );
        // 222 son sığan: `32 + 1 + 222 = 255`.
        assert_eq!(
            wheel_report(MouseEncoding::Normal, WHEEL_DOWN, 222, 222).unwrap(),
            [0x1b, b'[', b'M', 97, 255, 255]
        );
        // 223 bayta sığmaz: rapor gitmez — sütunda da satırda da.
        assert_eq!(wheel_report(MouseEncoding::Normal, WHEEL_UP, 223, 0), None);
        assert_eq!(wheel_report(MouseEncoding::Normal, WHEEL_UP, 0, 223), None);
    }

    #[test]
    fn utf8_report_takes_two_bytes_from_95() {
        // 94 → `32 + 1 + 94 = 127`, tek bayt; 95 → 128, iki bayt.
        assert_eq!(
            wheel_report(MouseEncoding::Utf8, WHEEL_UP, 94, 0).unwrap(),
            [0x1b, b'[', b'M', 96, 127, 33]
        );
        assert_eq!(
            wheel_report(MouseEncoding::Utf8, WHEEL_UP, 95, 95).unwrap(),
            [0x1b, b'[', b'M', 96, 0xc2, 0x80, 0xc2, 0x80]
        );
        // 2014 son sığan: `32 + 1 + 2014 = 2047`, iki baytlık UTF-8'in tepesi.
        assert_eq!(
            wheel_report(MouseEncoding::Utf8, WHEEL_DOWN, 2014, 0).unwrap(),
            [0x1b, b'[', b'M', 97, 0xdf, 0xbf, 33]
        );
        assert_eq!(wheel_report(MouseEncoding::Utf8, WHEEL_UP, 2015, 0), None);
        assert_eq!(wheel_report(MouseEncoding::Utf8, WHEEL_UP, 0, 2015), None);
        // Düz kipin sınırı UTF-8'de geçerli değil.
        assert!(wheel_report(MouseEncoding::Utf8, WHEEL_UP, 223, 0).is_some());
    }
}
