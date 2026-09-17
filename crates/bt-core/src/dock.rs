//! Aynanın çizilecek hâli: [`DockState`] → dock hücreleri.
//!
//! [`crate::Session::frame`]'in ızgara için yaptığını dock için bu modül
//! yapıyor ve aynı kuralla: **karar burada, boyama orada**. Sınırdan metin
//! değil **hücreler** geçiyor (renk, biçim, sütun), caret'in sütunu ve
//! yüzeyin iki rengi; kabuğun safhası, `region_highlight`'ın sözdizimi ve
//! `PREDISPLAY`/`POSTDISPLAY` ayrımı bu tarafta kalıyor — çizen taraf
//! "ne anlama geldiğini" bilmiyor.
//!
//! Gövde **saf**: kilit almıyor, `Session` görmüyor. Tek çağıranı
//! [`crate::Session::dock`] ve o yaprak kilidi alıp bırakıyor; sınamalar
//! buraya PTY'siz giriyor.

use crate::color::{self, LinearRgba, Theme};
use crate::session::{Cell, UnderlineStyle};
use crate::shell::{DockState, DockStatus, HighlightColor, HighlightStyle, ShellPhase, ShellState};

/// Dock'un karedeki yüzeyi — hücrelerin **dışında** kalan her şey, çözülmüş.
///
/// Hücreler sink'ten akıyor (`frame()` emsali); burada yalnız kare başına tek
/// olan değerler var. Safha ve çıkış kodu sınırı **geçmiyor**: `>` işaretinin
/// rengi burada çözülüyor, çizen taraf onu sıradan bir glyph olarak alıyor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dock {
    /// Yüzeyin zemini; **opak** olmak zorunda (bkz. [`render`]).
    pub ground: LinearRgba,
    /// Dock'u ızgaradan ayıran saç çizgisi.
    pub separator: LinearRgba,
    /// Caret'in dock satırındaki sütunu; `None` → caret çizilmez (ZLE satır
    /// düzenlemiyor ya da ayna okunamadı).
    pub caret: Option<u16>,
    /// Caret bloğunun altında kalan metnin rengi — ızgaradaki
    /// [`crate::Cursor::text`] ile aynı kural ve aynı değer.
    pub caret_text: LinearRgba,
}

/// Giriş işareti; dock'un ilk sütununda durur.
const SIGIL: char = '>';

/// Metnin başladığı sütun: işaret bir hücre, bir hücre de nefes payı.
///
/// Sabit, çünkü işaret **tek** karakter ve ayna onu görmüyor — aynanın
/// `PREDISPLAY`'i kabuğun prompt'u, bu ise terminalin kendi işareti.
const TEXT_COL: u16 = 2;

/// Aynayı bu karenin dock hücrelerine çevirir.
///
/// **Zemin opak olmak zorunda** ve bu bir zevk değil yapısal bir şart: kayma
/// boyunca ızgaranın ötelemesi hedefinden büyük (`bt_gpu`'nun `Slide`'ı), yani
/// en alt satır dock'un üstüne taşıyor. Dock ızgaradan **sonra** çizildiği ve
/// zemini opak olduğu için taşan piksel görünmüyor; yarı saydam bir zemin
/// kayma karelerinde titrerdi.
///
/// `cols` ızgaranın genişliği: dock aynı sütunları kullanıyor ve taşan metin
/// **soldan pencerelenip** caret görünür tutuluyor (kırpmak caret'i ekrandan
/// düşürürdü — yazdığını görmeyen bir giriş satırı). Pencereleme karakter
/// biriminde: geniş glyph bu sette henüz yok (`CLAUDE.md`).
pub(crate) fn render(
    state: &DockState,
    shell: Option<ShellState>,
    theme: &Theme,
    cols: u16,
    mut sink: impl FnMut(Cell),
) -> Dock {
    let surface = Dock {
        ground: theme.background_linear(),
        separator: theme.separator_linear(),
        caret: None,
        caret_text: theme.background_linear(),
    };
    if cols == 0 {
        return surface;
    }
    sink(Cell {
        col: 0,
        row: 0,
        ch: Some(SIGIL),
        fg: sigil_color(shell, theme),
        ..Cell::default()
    });

    // `Live` olmayan ayna metin çizdirmiyor ve ikisi de doğru cevap: `Idle`'da
    // ZLE satır düzenlemiyor, `Unavailable`'da gösteremediğimiz bir satır var
    // ve alanları zaten boş (`DockState::reset`). Ayrımı tüketen yer phase-4'ün
    // bastırma kararı, burası değil.
    if state.status != DockStatus::Live {
        return surface;
    }
    let available = usize::from(cols.saturating_sub(TEXT_COL));
    if available == 0 {
        return surface;
    }
    // Caret sağ kenarı geçince görüntü **soldan** kayıyor; caret son sütunda
    // durur. Ofset yalnız caret'e bağlı, metnin toplam uzunluğuna değil:
    // uzunluğu saymak bütün dizgiyi bir kez daha gezmek olurdu ve cevabı
    // değiştirmezdi.
    let skip = (state.cursor + 1).saturating_sub(available);

    let fixed = theme.foreground_linear();
    // Öneri sönük: "henüz yazılmamış metin" ile SGR 2'nin sorduğu şey aynı.
    let suggestion = theme.dim_linear();
    let stream = state
        .predisplay
        .chars()
        .chain(state.buffer.chars())
        .map(|ch| (ch, fixed))
        .chain(state.postdisplay.chars().map(|ch| (ch, suggestion)));

    for (index, (ch, base)) in stream.enumerate() {
        let Some(offset) = index.checked_sub(skip) else {
            continue;
        };
        if offset >= available {
            break;
        }
        // audit: `offset < available ≤ cols` ve `cols` `u16`; toplam taşamaz.
        let col = TEXT_COL + offset as u16;
        let cell = cell(ch, col, base, style_at(state, index), theme);
        // `frame()`'in atlama kapısının dock karşılığı: ne mürekkebi, ne
        // zemini, ne çizgisi olan hücre sink'e hiç uğramaz. Vurgusuz bir
        // satırda boşlukların çoğu buradan eleniyor ve `hucre=` jetonunun
        // dock kardeşi olmadığı için sayının tek tüketicisi bu tasarruf.
        if cell.ch.is_some() || cell.bg.is_some() || cell.underline != UnderlineStyle::None {
            sink(cell);
        }
    }

    Dock {
        // audit: `skip`'in tanımı gereği `cursor - skip < available ≤ cols`.
        caret: Some(TEXT_COL + (state.cursor - skip) as u16),
        ..surface
    }
}

/// `>` işaretinin rengi: kabuğun safhası.
///
/// Blok şeridiyle **aynı sözlük** (`ShellLog::stripe`): koşan komut vurgu,
/// biten komut çıkış koduna göre başarı ya da hata. Ayrı bir renk seçilseydi
/// aynı gerçeği iki yerde iki türlü anlatan bir pencere olurdu.
///
/// Entegrasyonsuz oturum (`None`) buraya gelmiyor — dock'u olmayan pencere
/// bu modülü hiç çağırmıyor — ama cevabı yine de vurgu: işaret, safhayı
/// bilmediğimizde de giriş satırının işareti.
fn sigil_color(shell: Option<ShellState>, theme: &Theme) -> LinearRgba {
    match shell {
        Some(ShellState {
            phase: ShellPhase::Finished,
            last_exit: Some(code),
        }) => {
            if code == 0 {
                theme.success_linear()
            } else {
                theme.error_linear()
            }
        }
        _ => theme.accent_linear(),
    }
}

/// Görüntünün `index` numaralı karakterine uygulanan stil.
///
/// Kayıtlar **sırayla** uygulanıyor ve sonraki kazanıyor: zsh de
/// `region_highlight`'ı listenin sırasıyla uyguluyor, yani üstüne yazan bir
/// eklenti (autosuggestions'ın üstüne syntax highlighting) burada da üstte
/// kalıyor.
///
/// Karakter başına bütün listeyi gezmek kareselleşiyor ama iki çarpanı da
/// küçük: sütun sayısı bir pencere, kayıt sayısı bir satırın jetonları.
/// Aralıkları sıralayıp tek geçişe indirmek kayıtların **çakışabilmesi**
/// yüzünden sıralamadan fazlasını ister; ölçülmüş bir ihtiyaç beklemeden
/// yazılmadı.
fn style_at(state: &DockState, index: usize) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    for highlight in &state.highlights {
        if !(highlight.start..highlight.end).contains(&index) {
            continue;
        }
        let applied = highlight.style;
        style = HighlightStyle {
            fg: applied.fg.or(style.fg),
            bg: applied.bg.or(style.bg),
            bold: style.bold || applied.bold,
            underline: style.underline || applied.underline,
            standout: style.standout || applied.standout,
        };
    }
    style
}

/// Bir karakterin hücresi: taban rengi + aralığın stili.
fn cell(ch: char, col: u16, base: LinearRgba, style: HighlightStyle, theme: &Theme) -> Cell {
    let mut fg = style.fg.map_or(base, |color| resolve(color, theme));
    let mut bg = style.bg.map(|color| resolve(color, theme));
    if style.standout {
        // Ters video: iki renk takaslanır. Aralığın kendi zemini yoksa yerine
        // yüzeyin zemini geçer — `frame()`'in `INVERSE` kolu da hücrenin
        // varsayılan arka planını aynı şekilde somutlaştırıyor.
        let behind = bg.unwrap_or_else(|| theme.background_linear());
        bg = Some(fg);
        fg = behind;
    }
    Cell {
        col,
        row: 0,
        // Mürekkepsiz hücrenin kuralı `frame()`'inkiyle aynı: boşluk glyph
        // üretmez (atlasta yuva harcar, tek piksel boyamaz). Kontrol
        // karakterleri de üretmiyor ve bu bir **bilinen sınır**: ZLE ham bayt
        // taşıyabiliyor (`Ctrl-V` ile yapıştırılmış bir kaçış dizisi) ve
        // bugün onlar dock'ta görünmez kalıyor. Yerinde bir yer tutucu
        // (`^C`) çizmek sütun aritmetiğini karakter biriminden çıkarır,
        // yani caret'in yerini de değiştirirdi.
        ch: (!ch.is_control() && ch != ' ').then_some(ch),
        fg,
        bg,
        bold: style.bold,
        italic: false,
        underline: if style.underline {
            UnderlineStyle::Single
        } else {
            UnderlineStyle::None
        },
        // SGR 58'in karşılığı `region_highlight`'ta yok: çizgi ön planı alır.
        underline_color: None,
        strikeout: false,
    }
}

/// Aynanın renk kaydı → çizilecek renk.
fn resolve(color: HighlightColor, theme: &Theme) -> LinearRgba {
    match color {
        HighlightColor::Indexed(index) => theme.indexed_linear(index),
        HighlightColor::Rgb(hex) => color::linear_hex(hex),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{DockFault, Highlight};

    const THEME: Theme = Theme::BATERI;

    /// Sütun sayısı: sınamaların çoğu pencerelemeyi sormuyor ve bu genişlik
    /// onların metnini rahat alıyor.
    const COLS: u16 = 40;

    fn live(predisplay: &str, buffer: &str, postdisplay: &str, cursor: usize) -> DockState {
        DockState {
            status: DockStatus::Live,
            predisplay: predisplay.into(),
            buffer: buffer.into(),
            postdisplay: postdisplay.into(),
            cursor,
            highlights: Vec::new(),
            // Çözücünün saydığı uzunluk; burada elle kuruluyor çünkü bu
            // modülün sınamaları tele hiç uğramıyor. `render` okumuyor —
            // tüketicisi bastırma (`ShellLog::suppressed_input`).
            display_chars: predisplay.chars().count()
                + buffer.chars().count()
                + postdisplay.chars().count(),
            last_ink: predisplay
                .chars()
                .chain(buffer.chars())
                .chain(postdisplay.chars())
                .filter(|ch| !ch.is_whitespace())
                .next_back(),
        }
    }

    /// Çizilen hücreler, sütun sırasıyla.
    fn draw(state: &DockState, cols: u16) -> (Vec<Cell>, Dock) {
        let mut cells = Vec::new();
        let dock = render(state, None, &THEME, cols, |cell| cells.push(cell));
        (cells, dock)
    }

    /// Satırın **sütun sütun** görüntüsü: hiç hücre üretilmeyen sütun da
    /// mürekkepsiz hücre de boşluk. Hücreleri sırayla dizmek yetmezdi —
    /// işaretle metin arasındaki nefes payı (hiç hücre üretmiyor) o dizgide
    /// görünmez ve sütun aritmetiği sınanmamış kalırdı.
    fn text(cells: &[Cell]) -> String {
        let width = cells.iter().map(|cell| cell.col + 1).max().unwrap_or(0);
        let mut line = vec![' '; usize::from(width)];
        for cell in cells {
            line[usize::from(cell.col)] = cell.ch.unwrap_or(' ');
        }
        line.into_iter().collect()
    }

    #[test]
    fn the_sigil_leads_and_the_text_follows_it() {
        // `cursor` **görüntü** uzayında (`DockState::cursor` normalize edilmiş
        // geliyor): `% ` iki karakter, imleç `ls -la`'nın sonunda, yani 8.
        let (cells, dock) = draw(&live("% ", "ls -la", "", 8), COLS);
        assert_eq!(text(&cells), "> % ls -la");
        // `>` + `%lsla` + `-`: vurgusuz iki boşluk hiçbir şey çizmiyor ve
        // sink'e de uğramıyor (`frame()`'in atlama kapısının dock karşılığı).
        assert_eq!(cells.len(), 7, "boşluklar hücre üretti");
        assert_eq!(cells[0].col, 0, "işaret ilk sütunda");
        // `PREDISPLAY`'in ilk karakteri metnin ilk sütununda: iki dizgi tek
        // görüntü ve aralarında boşluk yok.
        assert_eq!(cells[1].col, TEXT_COL);
        // Caret `CURSOR`'ın görüntü uzayındaki yeri (`DockState::cursor`
        // zaten normalize): `% ` iki karakter, imleç `ls -la`'nın sonunda.
        assert_eq!(dock.caret, Some(TEXT_COL + 8));
    }

    #[test]
    fn the_suggestion_is_dim_and_the_typed_text_is_not() {
        // Aynanın taşıdığı üç dizginin ikisi kullanıcının gördüğü metin, biri
        // öneri; ayrımı **renk** taşıyor. Tek renge inselerdi autosuggestions
        // kurulu bir oturumda yazılan ile önerilen ayırt edilemezdi.
        let (cells, _) = draw(&live("% ", "cd", " ~/src", 2), COLS);
        let typed = cells.iter().find(|cell| cell.ch == Some('c')).expect("c");
        let suggested = cells.iter().find(|cell| cell.ch == Some('~')).expect("~");
        assert_eq!(typed.fg, THEME.foreground_linear());
        assert_eq!(suggested.fg, THEME.dim_linear());
    }

    #[test]
    fn a_highlight_paints_its_range_and_nothing_else() {
        let mut state = live("", "echo hi", "", 7);
        state.highlights.push(Highlight {
            start: 0,
            end: 4,
            style: HighlightStyle {
                fg: Some(HighlightColor::Indexed(2)),
                bold: true,
                ..HighlightStyle::default()
            },
        });
        let (cells, _) = draw(&state, COLS);

        let green = THEME.indexed_linear(2);
        for cell in &cells[1..5] {
            assert_eq!(cell.fg, green, "aralık boyanmadı");
            assert!(cell.bold);
        }
        // Aralığın dışı tabanda kalmalı: bitişi **dışlamalı**. `hi`'nin
        // `i`'si seçildi çünkü `h` `echo`'da da geçiyor ve oradaki aralığın
        // içinde — ilk eşleşme sınamayı kendi iddiasının tersine çevirirdi.
        let outside = cells.iter().find(|cell| cell.ch == Some('i')).expect("i");
        assert_eq!(outside.fg, THEME.foreground_linear());
        assert!(!outside.bold);
    }

    #[test]
    fn standout_swaps_the_two_colors() {
        // zsh'in `standout`'u SGR 7'nin karşılığı ve ters video hücrenin iki
        // rengini takaslıyor. Aralığın kendi zemini yoksa yerine yüzeyin
        // zemini geçiyor — yoksa takas "renksiz bir arka planla" yapılır ve
        // harf görünmez olurdu.
        let mut state = live("", "x", "", 1);
        state.highlights.push(Highlight {
            start: 0,
            end: 1,
            style: HighlightStyle {
                standout: true,
                ..HighlightStyle::default()
            },
        });
        let (cells, _) = draw(&state, COLS);
        let x = cells.iter().find(|cell| cell.ch == Some('x')).expect("x");
        assert_eq!(x.fg, THEME.background_linear());
        assert_eq!(x.bg, Some(THEME.foreground_linear()));
    }

    #[test]
    fn the_sigil_takes_the_phase_color() {
        // `>` işareti safhayı söylüyor ve sözlük blok şeridininkiyle aynı.
        let state = live("", "", "", 0);
        let color = |shell| {
            let mut first = None;
            render(&state, shell, &THEME, COLS, |cell| {
                first.get_or_insert(cell.fg);
            });
            first.expect("işaret her hâlde çizilir")
        };
        assert_eq!(color(None), THEME.accent_linear());
        assert_eq!(
            color(Some(ShellState {
                phase: ShellPhase::Running,
                last_exit: None
            })),
            THEME.accent_linear()
        );
        assert_eq!(
            color(Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(0)
            })),
            THEME.success_linear()
        );
        assert_eq!(
            color(Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(1)
            })),
            THEME.error_linear()
        );
        // Kodu okunamayan `D` hata sayılmıyor: "bitti ama kodu bilmiyorum"
        // bir hata değil (`Mark::CommandEnd`).
        assert_eq!(
            color(Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: None
            })),
            THEME.accent_linear()
        );
    }

    #[test]
    fn an_idle_or_unavailable_mirror_draws_only_the_sigil() {
        // İkisi de metin çizdirmiyor **ve caret vermiyor**: düzenlenmeyen bir
        // satırın caret'i ekranda ikinci bir imleç olurdu.
        for status in [
            DockStatus::Idle,
            DockStatus::Unavailable(DockFault::Overflow),
        ] {
            let state = DockState {
                status,
                ..live("% ", "ls", "", 2)
            };
            let (cells, dock) = draw(&state, COLS);
            assert_eq!(text(&cells), ">", "{status:?} metin çizdirdi");
            assert_eq!(dock.caret, None, "{status:?} caret verdi");
        }
    }

    #[test]
    fn a_long_line_scrolls_from_the_left_and_keeps_the_caret_visible() {
        // Taşmada kırpmak caret'i ekrandan düşürürdü: kullanıcı yazdığını
        // görmezdi. Görüntü soldan kayıyor ve caret son sütunda duruyor.
        let cols = 10;
        let buffer: String = ('a'..='z').collect();
        let (cells, dock) = draw(&live("", &buffer, "", 26), cols);

        // Caret satırın **sonunda**, yani son harfin bir sağındaki boş
        // sütunda: pencereye yedi harf ile caret'in yeri sığıyor.
        assert_eq!(text(&cells), "> tuvwxyz", "pencere sağ uca yapışmadı");
        // Caret son sütunda: `cols - 1`. Bir ötesi pencerenin dışı olurdu.
        assert_eq!(dock.caret, Some(cols - 1));

        // Caret başa dönünce pencere de başa döner.
        let (cells, dock) = draw(&live("", &buffer, "", 0), cols);
        assert_eq!(text(&cells), "> abcdefgh");
        assert_eq!(dock.caret, Some(TEXT_COL));
    }

    #[test]
    fn a_window_too_narrow_for_text_still_answers() {
        // Dejenere genişlikler: sıfır sütunda hiçbir şey, işaretin sığdığı
        // ama metnin sığmadığı genişlikte yalnız işaret. İkisi de panik
        // değil — pencere simge durumuna inerken bu genişlikler gerçekten
        // geliyor (`split_into_grid`).
        let (cells, dock) = draw(&live("", "ls", "", 2), 0);
        assert!(cells.is_empty());
        assert_eq!(dock.caret, None);

        let (cells, dock) = draw(&live("", "ls", "", 2), TEXT_COL);
        assert_eq!(text(&cells), ">");
        assert_eq!(dock.caret, None);
    }

    #[test]
    fn the_surface_colors_come_from_the_theme() {
        // Zemin **opak** ve temanın kendisi: kayma boyunca taşan ızgara satırı
        // onun altında kalmalı. Ayraç türetilmiş bir değer, yeni bir rol değil.
        let (_, dock) = draw(&live("", "", "", 0), COLS);
        assert_eq!(dock.ground, THEME.background_linear());
        assert_eq!(dock.ground.to_array()[3], 1.0, "zemin saydam");
        assert_eq!(dock.separator, THEME.separator_linear());
        assert_ne!(dock.separator, dock.ground, "ayraç zeminle aynı renk");
        // Caret'in altındaki metin ızgaradakiyle aynı kuraldan: zemin rengi.
        assert_eq!(dock.caret_text, THEME.background_linear());
    }
}
