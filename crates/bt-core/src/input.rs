//! Girdi kodlaması: ok tuşu, fare düğmesi ve tekerlek raporu → PTY baytları.
//!
//! **Saf ve kilitsiz.** Kip sorusu (`TermMode`) çağıranın `Term` kilidinde
//! cevaplanır, buraya yalnız cevabı gelir; kararın kendisi ve baytlar burada,
//! PTY'siz sınanıyor. Klavyenin okları ile tekerleğin okları aynı [`arrow`]'dan
//! geçer — ok baytı depoda tek yerde yazılı. Farenin iki olayı da tek
//! gövdeden çıkıyor ([`mouse_report`]): tekerleğin basışı ile düğmenin
//! bas/bırakması aynı kodlamayı, aynı sınırı ve aynı reddi paylaşıyor.

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

impl MouseEncoding {
    /// Koordinatın **sığmadığı** ilk değer; SGR ondalık olduğu için sınırsız.
    ///
    /// Sayının tek yeri burası ve iki tüketicisi var: raporun reddi
    /// ([`mouse_report`], `>= limit` → `None`) ve bırakmanın kırpması
    /// ([`MouseEncoding::clamp`]). İki yerde yazılsaydı biri değişip öteki
    /// kalabilirdi ve sapma sessiz olurdu — kırpılan koordinat yine
    /// reddedilirdi.
    ///
    /// Değerler alacritty'ninki: düz kipte `32 + 1 + 222 = 255` son bayt,
    /// UTF-8 kipinde `32 + 1 + 2014 = 2047` iki baytlık UTF-8'in son değeri.
    fn limit(self) -> Option<u16> {
        match self {
            MouseEncoding::Sgr => None,
            MouseEncoding::Utf8 => Some(2015),
            MouseEncoding::Normal => Some(223),
        }
    }

    /// Koordinatı kodlamanın son sığan değerine indirir.
    ///
    /// **Yalnız bırakmanın yolu** ([`crate::Session::mouse_button`], R6):
    /// basışta sığmayan koordinat reddedilir, bırakmada kırpılır. Sebep
    /// asimetrik çünkü hâller asimetrik — reddedilen basış hiç başlamamış bir
    /// jest, düşürülen bırakma ise uygulamada **takılı kalmış bir düğme**.
    /// Hafifçe yanlış bir koordinat ondan iyidir.
    pub(crate) fn clamp(self, pos: u16) -> u16 {
        self.limit().map_or(pos, |limit| pos.min(limit - 1))
    }
}

/// Uygulamanın seçtiği kodlama. Tekerlek ([`wheel_route`]) ile düğme
/// ([`button_route`]) aynı tablodan okuyor.
fn mouse_encoding(mode: TermMode) -> MouseEncoding {
    if mode.contains(TermMode::SGR_MOUSE) {
        MouseEncoding::Sgr
    } else if mode.contains(TermMode::UTF8_MOUSE) {
        MouseEncoding::Utf8
    } else {
        MouseEncoding::Normal
    }
}

/// Fare düğmesi — raporun alt iki biti.
///
/// Tekerlek burada **yok**: onun düğmesi ([`WHEEL_UP`], [`WHEEL_DOWN`])
/// `bt-shell`'in görmediği bir sayı, çünkü tekerleğin yönünü
/// [`crate::Session::scroll_wheel`] satır işaretinden kendisi türetiyor.
/// Üçüncü fiziksel düğmenin ötesi de yok: X10 iki bit taşıyor ve `3` bırakma
/// için ayrılmış.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// Fare olayının değiştiricileri: ikisi rapora girer, biri arbitrajı yapar.
///
/// **Shift rapora hiç girmiyor** ve bu bir eksik değil, [`button_route`]'un
/// sonucu: Shift basılıyken olay uygulamaya değil seçime gidiyor, yani
/// raporda görünebileceği bir kol yok. Bit (4) yine de kurulsaydı hiçbir
/// uygulamanın okuyamayacağı bir değer yazardık. Alan bu yüzden burada, ayrı
/// bir `shift` argümanı olarak değil: kuralın iki yarısı ("arbitraja girer",
/// "rapora girmez") tek tipte yan yana duruyor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseModifiers {
    /// Terminalin kaçış yolu — bkz. tipin doc'u.
    pub shift: bool,
    /// macOS'ta Option; xterm'in 8 biti.
    pub meta: bool,
    /// xterm'in 16 biti.
    pub control: bool,
}

/// Düğmenin alt iki biti. `3` burada **yok**: o kod bırakmaya
/// ([`mouse_report`]) ve düğmesiz harekete ([`motion_byte`]) ayrılmış.
fn button_base(button: MouseButton) -> u8 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    }
}

/// xterm'in değiştirici bitleri: Meta 8, Control 16. Shift'in 4'ü **hiç
/// kurulmuyor** — gerekçesi [`MouseModifiers`]'ın doc'unda.
fn modifier_bits(modifiers: MouseModifiers) -> u8 {
    let meta = if modifiers.meta { 8 } else { 0 };
    let control = if modifiers.control { 16 } else { 0 };
    meta | control
}

/// Basış/bırakma raporunun düğme baytı: düğmenin kodu artı değiştiriciler.
pub(crate) fn button_byte(button: MouseButton, modifiers: MouseModifiers) -> u8 {
    button_base(button) | modifier_bits(modifiers)
}

/// Hareket raporunun düğme baytı: **hareket biti** (32) artı basılı düğme,
/// düğme yoksa `3`.
///
/// xterm'in kodlaması bu: aynı `3` hem "bırakma" hem "düğmesiz" demek ve
/// ikisini ayıran şey 32 biti. Basılı düğmeli hareket (`\e[<32;..M`) ile
/// düğmesiz hareket (`\e[<35;..M`) bu yüzden tek fonksiyondan çıkıyor.
pub(crate) fn motion_byte(button: Option<MouseButton>, modifiers: MouseModifiers) -> u8 {
    const MOTION: u8 = 32;
    let base = button.map_or(3, button_base);
    MOTION | base | modifier_bits(modifiers)
}

/// Düğmenin karar tablosu — [`WheelRoute`]'un kardeşi, aynı örüntüde.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonRoute {
    /// Uygulama fare raporu istedi (1000/1002/1003) ve Shift basılı değil.
    Report(MouseEncoding),
    /// Kip kapalı ya da Shift basılı: jest terminalin, seçim başlıyor.
    Select,
}

/// Kipten ve Shift'ten düğmenin yolu.
///
/// **Tekerlekle asimetrik ve asimetri bilerek:** `wheel_route`'ta Shift fare
/// kipini geçersiz kılmaz (`mouse_mode_comes_first_on_either_screen`), burada
/// kılar. Sebep, iki kolda yarışan şeylerin farklı olması — tekerlekte
/// Shift'in üstüne binecek ikinci bir tüketici yok (kaydırma zaten
/// terminalin) ve macOS klasik farede Shift+tekerleği yatay deltaya çeviriyor,
/// yani o koldaki Shift zaten güvenilmez; düğmede ise iki gerçek tüketici var
/// (uygulamanın faresi ve kullanıcının seçimi) ve Shift **tek** kaçış yolu.
/// xterm'in konvansiyonu; iTerm2, kitty, WezTerm ve ghostty aynısını yapıyor.
/// Bekçisi `shift_overrides_the_button_but_not_the_wheel`.
pub(crate) fn button_route(mode: TermMode, shift: bool) -> ButtonRoute {
    // `intersects`, `contains` değil — `wheel_route`'un yazılı gerekçesi.
    if mode.intersects(TermMode::MOUSE_MODE) && !shift {
        ButtonRoute::Report(mouse_encoding(mode))
    } else {
        ButtonRoute::Select
    }
}

/// Kipten ve basılı düğmeden hareketin yolu. Cevap [`ButtonRoute`] **değil**
/// `Option`: hareket bir jest başlatmıyor, yani "seçim" kolu yok — rapor
/// istenmiyorsa olay düşüyor ve fare bugünkü işini (seçimi taşımak, ya da
/// hiçbir şey) sürdürüyor.
///
/// Üç kip burada **ayrışıyor** ve `MOUSE_MODE` birleşik sorulamaz: 1003
/// (`MOUSE_MOTION`) her hareketi ister, 1002 (`MOUSE_DRAG`) yalnız basılı
/// olanı, 1000 (`MOUSE_REPORT_CLICK`) hiçbirini. Düğme yolunda üçü aynı
/// cevabı veriyordu ([`button_route`]'un tek `intersects`'i), burada
/// vermiyorlar.
///
/// **Shift sorulmuyor**: rota basışta kilitleniyor
/// ([`crate::Session::mouse_button`]) ve jestin ortasında değişmiyor;
/// düğmesiz harekette de zaten bir jest yok.
pub(crate) fn motion_route(mode: TermMode, pressed: bool) -> Option<MouseEncoding> {
    let wanted =
        mode.contains(TermMode::MOUSE_MOTION) || (pressed && mode.contains(TermMode::MOUSE_DRAG));
    wanted.then(|| mouse_encoding(mode))
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
        WheelRoute::Report(mouse_encoding(mode))
    } else if !mode.contains(TermMode::ALT_SCREEN) {
        WheelRoute::Scroll
    } else if mode.contains(TermMode::ALTERNATE_SCROLL) && !shift {
        WheelRoute::Arrows
    } else {
        WheelRoute::Ignore
    }
}

/// Tek fare olayının raporu: tekerleğin basışı da düğmenin bas/bırakması da.
/// `col`/`row` 0 tabanlı ve **uygulamanın** ekranında (grid satırı, görünen
/// pencere değil). Kodlamaya sığmayan koordinatta `None` — rapor gönderilmez,
/// kırpılmış bir hücreye de gitmez; bırakmanın kırpması çağıranın işi
/// ([`MouseEncoding::clamp`]).
///
/// **Bırakmayı iki kodlama iki türlü söylüyor.** SGR'ın son baytı `M` yerine
/// `m` ve düğme kodu korunur, yani uygulama hangi düğmenin bırakıldığını
/// bilir. X10/UTF-8'de böyle bir yer yok: bırakma, düğme bitleri `3` yazılarak
/// söyleniyor ve **hangi** düğme olduğu kaybolur. Bu bir eksiklik değil
/// protokolün kendi sınırı; değiştirici bitleri korunuyor.
pub(crate) fn mouse_report(
    encoding: MouseEncoding,
    button: u8,
    pressed: bool,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    let Some(limit) = encoding.limit() else {
        // `u32`: `u16::MAX + 1` taşmasın.
        let (col, row) = (u32::from(col) + 1, u32::from(row) + 1);
        let last = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{button};{col};{row}{last}").into_bytes());
    };
    if col >= limit || row >= limit {
        return None;
    }
    let utf8 = encoding == MouseEncoding::Utf8;
    // Düğme bitleri (alt iki) `3` oluyor, değiştiriciler ve tekerlek biti
    // olduğu gibi kalıyor.
    let button = if pressed {
        button
    } else {
        (button & !0b11) | 3
    };
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
    fn shift_overrides_the_button_but_not_the_wheel() {
        // Setin **asimetrisi** ve bekçisi bilerek tekerleğinkinin yanında:
        // `mouse_mode_comes_first_on_either_screen` "Shift fare kipini
        // geçersiz kılmaz" diyor ve o cümle **yalnız tekerleğe** ait.
        // Kuralı düğmeye de uygulayan biri bu sınamayı kırar; kırmasaydı
        // uygulama içinde fareyle metin seçme yeteneği sessizce ölürdü.
        for bit in [
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
        ] {
            for screen in [TermMode::empty(), TermMode::ALT_SCREEN] {
                let mode = bit | screen | TermMode::ALTERNATE_SCROLL;
                assert_eq!(
                    button_route(mode, false),
                    ButtonRoute::Report(MouseEncoding::Normal),
                    "{mode:?}"
                );
                // Düğme: Shift terminali geri alıyor.
                assert_eq!(button_route(mode, true), ButtonRoute::Select, "{mode:?}");
                // Tekerlek: aynı kipte, aynı Shift'te rapor kalıyor.
                assert_eq!(
                    wheel_route(mode, true),
                    WheelRoute::Report(MouseEncoding::Normal),
                    "{mode:?}"
                );
            }
        }
    }

    #[test]
    fn button_selects_whenever_no_mouse_mode_is_set() {
        // Kip kapalıysa Shift'in bir hükmü yok: iki kolda da seçim. Alternate
        // screen ve 1007 düğmeyi hiç ilgilendirmiyor — tekerleğin ok/ignore
        // dalları burada **yok**, çünkü düğmenin gidecek üçüncü bir yeri yok.
        for mode in [
            TermMode::empty(),
            TermMode::ALT_SCREEN,
            TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL,
            TermMode::SGR_MOUSE,
            TermMode::APP_CURSOR,
        ] {
            for shift in [false, true] {
                assert_eq!(
                    button_route(mode, shift),
                    ButtonRoute::Select,
                    "{mode:?} shift={shift}"
                );
            }
        }
    }

    #[test]
    fn button_and_wheel_share_the_encoding_table() {
        // `mouse_encoding` tek yerde; iki yol da oradan okuyor.
        let click = TermMode::MOUSE_REPORT_CLICK;
        for (mode, encoding) in [
            (click, MouseEncoding::Normal),
            (click | TermMode::SGR_MOUSE, MouseEncoding::Sgr),
            (click | TermMode::UTF8_MOUSE, MouseEncoding::Utf8),
            (
                click | TermMode::SGR_MOUSE | TermMode::UTF8_MOUSE,
                MouseEncoding::Sgr,
            ),
        ] {
            assert_eq!(button_route(mode, false), ButtonRoute::Report(encoding));
            assert_eq!(wheel_route(mode, false), WheelRoute::Report(encoding));
        }
    }

    #[test]
    fn modifier_bits_are_meta_and_control_never_shift() {
        let (left, right) = (MouseButton::Left, MouseButton::Right);
        let none = MouseModifiers::default();
        assert_eq!(button_byte(left, none), 0);
        assert_eq!(button_byte(MouseButton::Middle, none), 1);
        assert_eq!(button_byte(right, none), 2);
        let meta = MouseModifiers { meta: true, ..none };
        let control = MouseModifiers {
            control: true,
            ..none
        };
        assert_eq!(button_byte(left, meta), 8);
        assert_eq!(button_byte(left, control), 16);
        assert_eq!(
            button_byte(
                right,
                MouseModifiers {
                    meta: true,
                    control: true,
                    shift: true,
                }
            ),
            // 2 | 8 | 16 — Shift'in 4'ü **yok**: o bit hiç kurulmuyor, çünkü
            // Shift'li olay `button_route`'ta seçime ayrılıyor ve rapora
            // gelmiyor.
            26
        );
    }

    #[test]
    fn motion_route_splits_the_three_mouse_modes() {
        // Düğme yolunda üç bit aynı cevabı veriyor
        // (`mouse_mode_comes_first_on_either_screen`); hareket yolunda
        // ayrışıyorlar ve ayrım kipin **anlamı**: 1000 tıklama, 1002
        // sürükleme, 1003 her hareket.
        let (click, drag, motion) = (
            TermMode::MOUSE_REPORT_CLICK,
            TermMode::MOUSE_DRAG,
            TermMode::MOUSE_MOTION,
        );
        let normal = Some(MouseEncoding::Normal);
        // 1000: hiçbir hareket.
        assert_eq!(motion_route(click, false), None);
        assert_eq!(motion_route(click, true), None);
        // 1002: yalnız basılıyken.
        assert_eq!(motion_route(drag, false), None);
        assert_eq!(motion_route(drag, true), normal);
        // 1003: her zaman.
        assert_eq!(motion_route(motion, false), normal);
        assert_eq!(motion_route(motion, true), normal);
        // Kip yokken de hiç.
        assert_eq!(motion_route(TermMode::empty(), true), None);
        // Kodlama düğmeyle aynı tablodan.
        assert_eq!(
            motion_route(motion | TermMode::SGR_MOUSE, false),
            Some(MouseEncoding::Sgr)
        );
    }

    #[test]
    fn motion_sets_bit_thirtytwo_and_three_without_a_button() {
        let none = MouseModifiers::default();
        // Düğmesiz hareket: `32 | 3`.
        assert_eq!(motion_byte(None, none), 35);
        // Basılı düğme: `32` artı düğmenin kodu.
        assert_eq!(motion_byte(Some(MouseButton::Left), none), 32);
        assert_eq!(motion_byte(Some(MouseButton::Middle), none), 33);
        assert_eq!(motion_byte(Some(MouseButton::Right), none), 34);
        // Değiştiriciler basış raporundaki bitlerin aynısı; Shift yine yok.
        assert_eq!(
            motion_byte(
                Some(MouseButton::Left),
                MouseModifiers {
                    meta: true,
                    control: true,
                    shift: true,
                }
            ),
            32 | 8 | 16
        );
        // Aynı bayt üç kodlamada da hareket olarak çıkıyor; `pressed = true`
        // çünkü hareketin bırakma biçimi yok.
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, 35, true, 4, 2).unwrap(),
            b"\x1b[<35;5;3M"
        );
        assert_eq!(
            mouse_report(MouseEncoding::Normal, 35, true, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 32 + 35, 37, 35]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, 32, true, 95, 0).unwrap(),
            [0x1b, b'[', b'M', 32 + 32, 0xc2, 0x80, 33]
        );
    }

    #[test]
    fn release_says_m_in_sgr_and_button_three_elsewhere() {
        // SGR: düğme kodu korunur, son bayt `m`.
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, 2, false, 4, 2).unwrap(),
            b"\x1b[<2;5;3m"
        );
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, 2, true, 4, 2).unwrap(),
            b"\x1b[<2;5;3M"
        );
        // X10/UTF-8: düğme bitleri `3`, yani **hangi** düğme olduğu kaybolur;
        // değiştiriciler kalıyor.
        assert_eq!(
            mouse_report(MouseEncoding::Normal, 2, false, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 32 + 3, 37, 35]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Normal, 2 | 16, false, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 32 + 3 + 16, 37, 35]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, 1, false, 95, 0).unwrap(),
            [0x1b, b'[', b'M', 32 + 3, 0xc2, 0x80, 33]
        );
    }

    #[test]
    fn clamp_lands_on_the_last_coordinate_the_encoding_accepts() {
        // Kırpılan değer **kabul edilen** değer olmalı: aynı sayıyı
        // `mouse_report` reddetmemeli, yoksa bırakma yine düşerdi (R6).
        for encoding in [MouseEncoding::Normal, MouseEncoding::Utf8] {
            let far = encoding.clamp(u16::MAX);
            assert_eq!(far, encoding.limit().unwrap() - 1);
            assert!(mouse_report(encoding, 0, false, far, far).is_some());
            // Sığan koordinat kırpmadan geçiyor.
            assert_eq!(encoding.clamp(7), 7);
        }
        // SGR sınırsız: kırpma kimliktir.
        assert_eq!(MouseEncoding::Sgr.clamp(u16::MAX), u16::MAX);
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
            mouse_report(MouseEncoding::Sgr, WHEEL_UP, true, 4, 2).unwrap(),
            b"\x1b[<64;5;3M"
        );
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, WHEEL_DOWN, true, 0, 0).unwrap(),
            b"\x1b[<65;1;1M"
        );
        // Sınır yok ve `u16`'nın tepesinde `+ 1` taşmıyor.
        assert_eq!(
            mouse_report(MouseEncoding::Sgr, WHEEL_UP, true, u16::MAX, 2015).unwrap(),
            b"\x1b[<64;65536;2016M"
        );
    }

    #[test]
    fn normal_report_is_one_byte_per_coordinate_up_to_222() {
        // `32 + düğme`, `32 + 1 + konum`.
        assert_eq!(
            mouse_report(MouseEncoding::Normal, WHEEL_UP, true, 4, 2).unwrap(),
            [0x1b, b'[', b'M', 96, 37, 35]
        );
        // 222 son sığan: `32 + 1 + 222 = 255`.
        assert_eq!(
            mouse_report(MouseEncoding::Normal, WHEEL_DOWN, true, 222, 222).unwrap(),
            [0x1b, b'[', b'M', 97, 255, 255]
        );
        // 223 bayta sığmaz: rapor gitmez — sütunda da satırda da.
        assert_eq!(
            mouse_report(MouseEncoding::Normal, WHEEL_UP, true, 223, 0),
            None
        );
        assert_eq!(
            mouse_report(MouseEncoding::Normal, WHEEL_UP, true, 0, 223),
            None
        );
    }

    #[test]
    fn utf8_report_takes_two_bytes_from_95() {
        // 94 → `32 + 1 + 94 = 127`, tek bayt; 95 → 128, iki bayt.
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 94, 0).unwrap(),
            [0x1b, b'[', b'M', 96, 127, 33]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 95, 95).unwrap(),
            [0x1b, b'[', b'M', 96, 0xc2, 0x80, 0xc2, 0x80]
        );
        // 2014 son sığan: `32 + 1 + 2014 = 2047`, iki baytlık UTF-8'in tepesi.
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_DOWN, true, 2014, 0).unwrap(),
            [0x1b, b'[', b'M', 97, 0xdf, 0xbf, 33]
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 2015, 0),
            None
        );
        assert_eq!(
            mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 0, 2015),
            None
        );
        // Düz kipin sınırı UTF-8'de geçerli değil.
        assert!(mouse_report(MouseEncoding::Utf8, WHEEL_UP, true, 223, 0).is_some());
    }
}
