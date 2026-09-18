//! Dock'un çizilecek hâli: [`DockState`] ile [`DockContext`] → dock hücreleri.
//!
//! [`crate::Session::frame`]'in ızgara için yaptığını dock için bu modül
//! yapıyor ve aynı kuralla: **karar burada, boyama orada**. Sınırdan metin
//! değil **hücreler** geçiyor (renk, biçim, sütun, satır), caret'in sütunu ve
//! yüzeyin iki rengi; kabuğun safhası, `region_highlight`'ın sözdizimi,
//! `PREDISPLAY`/`POSTDISPLAY` ayrımı **ve bağlam satırının taşma kuralı** bu
//! tarafta kalıyor — çizen taraf "ne anlama geldiğini" bilmiyor. Taşmanın
//! burada durması bir yer tercihi değil: hangi yarının kısalacağı
//! (yol kısalır, dal kısalmaz) bir ürün kararı, piksel kararı değil.
//!
//! **İki satır, iki ömür:** üst satır aynadan doğuyor ve tuş başına
//! tazeleniyor, alt satır bağlamdan ve prompt başına.
//!
//! Gövde **saf**: kilit almıyor, `Session` görmüyor. Tek çağıranı
//! [`crate::Session::dock`] ve o yaprak kilidi alıp bırakıyor; sınamalar
//! buraya PTY'siz giriyor.

use crate::color::{self, LinearRgba, Theme};
use crate::session::{Cell, UnderlineStyle};
use crate::shell::{
    DockContext, DockState, DockStatus, HighlightColor, HighlightStyle, ShellPhase, ShellState,
};

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
    /// Prompt işaretinin rengi: kabuğun safhası.
    ///
    /// **Renk geçiyor, şekil geçmiyor.** İşaret bir hücre değil: sınırdan bir
    /// karakter olarak geçerse kullanıcının fontunun `>`'ü çizilir, oysa o
    /// terminalin kendi işareti (`bt_atlas::RuleKind::Chevron`). Karar burada
    /// — hangi renk, yani kabuk ne yapıyor — boyama orada.
    ///
    /// Izgaranın blok şeridiyle **aynı sözlük** ([`crate::Block::stripe`]) ve
    /// artık aynı şekil: ikisi de safha renginde bir prompt işareti.
    pub sigil: LinearRgba,
}

/// Metnin başladığı sütun: işaret bir hücre, bir hücre de nefes payı.
///
/// Sabit, çünkü işaret **tek** karakter ve ayna onu görmüyor — aynanın
/// `PREDISPLAY`'i kabuğun prompt'u, bu ise terminalin kendi işareti.
///
/// **Yalnız giriş satırının hizası**; bağlam satırı sol kenardan başlıyor
/// ([`CONTEXT_COL`]).
const TEXT_COL: u16 = 2;

/// Bağlam satırının iki yanını ayıran işaret; iki yanında birer boşluk.
const SEPARATOR: &str = " | ";

/// Bağlam satırının başladığı sütun: dock'un **sol kenarı**.
///
/// Giriş satırının metniyle değil, `>` işaretiyle hizalı. [`TEXT_COL`]'dan
/// başlasaydı — ve başlıyordu — bağlam satırı sebepsiz girintili görünürdü
/// (kullanıcı, 012 phase-9: "bu path gösterimi niye indenti var gibi"): metnin
/// hizası işaretin açtığı boşluğu bir girinti gibi okutuyor, oysa bağlam
/// giriş satırının devamı değil, dock'un **altbilgisi**.
const CONTEXT_COL: u16 = 0;

/// Bağlam satırının dock-yerel satır numarası; giriş satırının **altı**.
///
/// `DOCK_ROWS`'un ikinci satırı ve orası bu crate'te değil `bt-gpu`'da
/// sayılıyor; buradaki sabit onun tüketicisi, ikinci bir kaynak değil.
const CONTEXT_ROW: u16 = 1;

/// Soldan kısaltılmış yolun başındaki işaret.
const ELLIPSIS: char = '…';

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
    context: &DockContext,
    shell: Option<ShellState>,
    theme: &Theme,
    cols: u16,
    owned: bool,
    mut sink: impl FnMut(Cell),
) -> Dock {
    let mut surface = Dock {
        ground: theme.background_linear(),
        separator: theme.separator_linear(),
        caret: None,
        caret_text: theme.background_linear(),
        sigil: sigil_color(shell, theme),
    };
    if cols == 0 {
        return surface;
    }
    // **Caret'in sahibi burada sorulmuyor, cevabı hazır geliyor** (`owned`).
    // Eskiden burada [`caret_home`] ikinci kez çağrılıyordu ve o çağrı
    // `Session::frame`'in üç ön koşulunu (pencerenin dock'u var mı,
    // alternatif ekranda mıyız, ayna taze mi) **bilmiyordu**: bayat aynada
    // ızgara imlecini görünür verirken dock da caret'ini veriyordu, çizen
    // taraf dock'u seçiyor ve kullanıcının yazdığı taze satır caret'siz
    // kalıyordu. Yüklem tek, hesabı da tek — ve aynı değişiklik iki ayrı
    // kilit turundan türetme yarışını da kapatıyor.
    // İşaret **sink'ten geçmiyor**: bir hücre değil, yüzeyin bir alanı
    // ([`Dock::sigil`]). Hücre olsaydı kullanıcının fontunun `>`'ü çizilirdi.
    //
    // Bağlam satırı aynanın **durumundan önce**: dizin ve dal ZLE satırı
    // düzenlemese de doğru ve kullanıcı komut koşarken de onlara bakıyor.
    // Aşağıdaki `Live` kapısının altında kalsaydı her komutta kaybolurdu.
    render_context(context, theme, cols, &mut sink);

    let available = usize::from(cols.saturating_sub(TEXT_COL));
    if available == 0 {
        return surface;
    }
    // `Live` olmayan ayna metin çizdirmiyor ve ikisi de doğru cevap: `Idle`'da
    // ZLE satır düzenlemiyor, `Unavailable`'da gösteremediğimiz bir satır var
    // ve alanları zaten boş (`DockState::reset`). Ayrımı tüketen yer phase-4'ün
    // bastırma kararı, burası değil.
    //
    // **Caret yine de çizilebilir**: metni olmayan bir satır caret'siz bir
    // satır demek değil. Açılışta ve iki komut arasında ayna `Idle` ve dock
    // boş, ama kullanıcının yazmaya başlayacağı yer orası — imleci o
    // pencerelerde ızgarada tutmak caret'i prompt gelince sıçratırdı.
    if state.status != DockStatus::Live {
        if owned {
            surface.caret = Some(TEXT_COL);
        }
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
        caret: owned.then(|| TEXT_COL + (state.cursor - skip) as u16),
        ..surface
    }
}

/// Dock'un **alt** satırı: `{tam yol} | {dal}`, sol altta ve sönük.
///
/// **Taşmada yol soldan kısalır, dal asla kısalmaz.** Gerekçe iki ayrı:
/// yolun bilgisi kuyruğunda (hangi klasördesin), yani baştan kesmek en
/// bilgilendirici yarıyı atardı; dalın ise **hiçbir** yarısı atılamaz —
/// kısaltılmış bir dal adı (`mai…`) kullanıcıya başka bir dalda olduğunu
/// düşündürebilir ve bu, bu deponun yasakladığı "sessizce yanlış" sınıfı.
///
/// Kısaltma **karakter** biriminde ve bileşen sınırına yaslanmıyor: sınıra
/// yaslamak kullanılabilir sütunların bir kısmını boş bırakırdı ve kazancı
/// zevk, kaybı bilgi olurdu. Geniş glyph bu sette yok (`CLAUDE.md`).
///
/// **Ayraç iki yan da doluysa çizilir.** Depo olmayan dizinde asılı bir `|`
/// "dal okunamadı" derdi; okunacak dal yok.
fn render_context(context: &DockContext, theme: &Theme, cols: u16, sink: &mut impl FnMut(Cell)) {
    let available = usize::from(cols.saturating_sub(CONTEXT_COL));
    if available == 0 {
        return;
    }
    let branch_chars = context.branch.chars().count();
    let path_chars = context.cwd.chars().count();
    // Bütçe **önce dala** ayrılıyor; yol kalanı alıyor. Ayraç da yolun
    // tarafında sayılıyor, çünkü yol düşerse ayraç da düşüyor.
    //
    // **Sığmayan dal kırpılmıyor, düşüyor.** Dalın "hiçbir yarısı atılamaz"
    // kuralının (yukarıdaki doc) dejenere genişlikteki karşılığı bu: `release/2.1`
    // dalını on iki sütunda `release` diye göstermek, kullanıcıya **var
    // olmayan bir dalda** olduğunu söylerdi ve işaret koymak (`rele…`) da onu
    // düzeltmezdi — kısalmış bir dal adı zaten yanlış okunabilir. Hiç
    // göstermemek bilgi kaybı ama yanlış bilgi değil; o genişlikte pencere
    // zaten okunmuyor (`/code-review`, 012 phase-7).
    let shows_branch = branch_chars > 0 && branch_chars <= available;
    let path_budget = if shows_branch {
        available
            .saturating_sub(branch_chars)
            .saturating_sub(SEPARATOR.chars().count())
    } else {
        // Dal çizilmiyorsa genişliğin tamamı yolun: onun kısaltması **işaretli**
        // (`…`), yani yanlış okunamaz.
        available
    };

    // `skip` yolun **başından** atılan karakter sayısı; `mark` kısaltmanın
    // görünür işareti. Yol hiç çizilmiyorsa ikisi de baştan susuyor.
    let (mark, skip) = if path_budget == 0 || path_chars == 0 {
        (None, path_chars)
    } else if path_chars <= path_budget {
        (None, 0)
    } else {
        // İşaretin kendisi de bir sütun: kuyruktan `path_budget - 1` karakter.
        (Some(ELLIPSIS), path_chars - (path_budget - 1))
    };
    let shows_path = mark.is_some() || skip < path_chars;
    let separator = if shows_path && shows_branch {
        SEPARATOR
    } else {
        ""
    };

    // **Yolun son bileşeni öne çıkıyor, öncesi geri çekiliyor.** Kullanıcının
    // aradığı bilgi "hangi klasördeyim"; üst dizinler onu yerleştiren bağlam.
    // İkisi aynı tonda olunca göz son bileşeni aramak zorunda kalıyordu.
    //
    // Sönük olan **yeni bir renk değil**: sönüğün sönüğü, yani ayracın ta
    // kendisi (`Theme::separator_linear` — "temanın en sessiz mürekkebi",
    // kendi doc'u öyle diyor). İkinci bir zevk sabiti girmiyor, hiyerarşi tek
    // kuraldan (`dim_toward`) iki kez geçerek doğuyor.
    let normal = theme.dim_linear();
    let quiet = theme.separator_linear();
    // Son bileşenin yoldaki **karakter** sırası: son `/`'ten sonrası.
    // Bölme yok, `char_indices` değil `enumerate`: aşağıdaki `skip` de
    // karakter sayıyor ve ikisi aynı birimde olmak zorunda.
    let head_end = context
        .cwd
        .chars()
        .enumerate()
        .filter(|(_, ch)| *ch == '/')
        .map(|(index, _)| index + 1)
        .last()
        .unwrap_or(0);
    // Son bileşen boşsa (`/`, ya da sondaki eğik çizgi) ayrım yapılmıyor:
    // yolun tamamı öne çıkıyor. Yanlışın yönü güvenli — fazla vurgulamak
    // bilgiyi gizlemez, hepsini soluklaştırmak gizlerdi.
    let head_end = if head_end >= path_chars { 0 } else { head_end };

    let line = mark
        // Kısaltma işareti atılan **üst** dizinlerin yerinde duruyor, yani
        // onlarla aynı tonda.
        .map(|ch| (ch, quiet))
        .into_iter()
        .chain(
            context
                .cwd
                .chars()
                .skip(skip)
                .enumerate()
                .map(|(offset, ch)| {
                    (
                        ch,
                        if skip + offset < head_end {
                            quiet
                        } else {
                            normal
                        },
                    )
                }),
        )
        // Ayraç bir bölme işareti, içerik değil: en sessiz tonda.
        .chain(separator.chars().map(|ch| (ch, quiet)))
        .chain(
            shows_branch
                .then(|| context.branch.chars().map(|ch| (ch, normal)))
                .into_iter()
                .flatten(),
        );
    // `take` bir bekçi, bir politika değil: yukarıdaki bütçe zaten `available`
    // sütunu aşmıyor. Sağdan taşan bir hücre ızgaranın dışına yazardı ve o
    // aritmetik hatası burada sessizce durur.
    for (offset, (ch, fg)) in line.take(available).enumerate() {
        // Boşluk glyph üretmiyor (`cell`'in kuralı); ayracın iki yanı da
        // buradan eleniyor.
        if ch == ' ' {
            continue;
        }
        sink(Cell {
            // audit: `offset < available ≤ cols` ve `cols` `u16`; toplam taşamaz.
            col: CONTEXT_COL + offset as u16,
            row: CONTEXT_ROW,
            ch: Some(ch),
            // Satırın tamamı sönük kalıyor — bağlam okunur ama giriş satırıyla
            // yarışmaz — ve **içinde** ikinci bir kademe var (yukarıda).
            fg,
            ..Cell::default()
        });
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
    // Sahiplik artık `render`'ın argümanı; yüklemi yalnız burası çağırıyor,
    // üretimde cevabı `Session::frame` veriyor.
    use crate::shell::{CaretHome, DockFault, Highlight, caret_home};

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
            // Bu modül okumuyor (tüketicisi `Session::can_be_typed`); canlı
            // bir satırın olağan hâli ekleme keymap'i.
            insert_keymap: true,
        }
    }

    /// Bağlamsız çizim: yol da dal da boş (bu modülün eski sınamalarının hâli).
    fn draw(state: &DockState, cols: u16) -> (Vec<Cell>, Dock) {
        draw_with(state, &DockContext::default(), cols)
    }

    /// Çizilen hücreler, sütun sırasıyla.
    fn draw_with(state: &DockState, context: &DockContext, cols: u16) -> (Vec<Cell>, Dock) {
        let mut cells = Vec::new();
        // Sahiplik sınamanın girdisi değil: üretimde `Session::frame` veriyor,
        // burada aynı yüklemden türetiliyor ki bu modülün sınamaları
        // devrin kuralını değil **çizimi** sınasın.
        let owned = caret_home(None, state.status) == CaretHome::Dock;
        let dock = render(state, context, None, &THEME, cols, owned, |cell| {
            cells.push(cell)
        });
        (cells, dock)
    }

    /// Safhanın caret'e etkisini soran sınamalar için: kabuğun durumu
    /// çağırandan.
    fn draw_as(state: &DockState, shell: Option<ShellState>, cols: u16) -> (Vec<Cell>, Dock) {
        let mut cells = Vec::new();
        let owned = caret_home(shell, state.status) == CaretHome::Dock;
        let dock = render(
            state,
            &DockContext::default(),
            shell,
            &THEME,
            cols,
            owned,
            |cell| cells.push(cell),
        );
        (cells, dock)
    }

    fn context(cwd: &str, branch: &str) -> DockContext {
        DockContext {
            cwd: cwd.into(),
            branch: branch.into(),
        }
    }

    /// Giriş satırının **sütun sütun** görüntüsü.
    fn text(cells: &[Cell]) -> String {
        row_text(cells, 0)
    }

    /// Bir satırın **sütun sütun** görüntüsü: hiç hücre üretilmeyen sütun da
    /// mürekkepsiz hücre de boşluk. Hücreleri sırayla dizmek yetmezdi —
    /// işaretle metin arasındaki nefes payı (hiç hücre üretmiyor) o dizgide
    /// görünmez ve sütun aritmetiği sınanmamış kalırdı.
    fn row_text(cells: &[Cell], row: u16) -> String {
        let on_row = || cells.iter().filter(|cell| cell.row == row);
        let width = on_row().map(|cell| cell.col + 1).max().unwrap_or(0);
        let mut line = vec![' '; usize::from(width)];
        for cell in on_row() {
            line[usize::from(cell.col)] = cell.ch.unwrap_or(' ');
        }
        line.into_iter().collect()
    }

    #[test]
    fn the_sigil_leads_and_the_text_follows_it() {
        // `cursor` **görüntü** uzayında (`DockState::cursor` normalize edilmiş
        // geliyor): `% ` iki karakter, imleç `ls -la`'nın sonunda, yani 8.
        let (cells, dock) = draw(&live("% ", "ls -la", "", 8), COLS);
        // İşaret **hücre değil**: yüzeyin bir alanı ([`Dock::sigil`]) ve
        // şeklini `bt-gpu` çiziyor. Metin bu yüzden iki sütun boşlukla
        // başlıyor — işaretin ve nefes payının yeri.
        assert_eq!(text(&cells), "  % ls -la");
        // `%lsla` + `-`: vurgusuz iki boşluk hiçbir şey çizmiyor ve sink'e de
        // uğramıyor (`frame()`'in atlama kapısının dock karşılığı).
        assert_eq!(cells.len(), 6, "boşluklar hücre üretti");
        // `PREDISPLAY`'in ilk karakteri metnin ilk sütununda: iki dizgi tek
        // görüntü ve aralarında boşluk yok.
        assert_eq!(cells[0].col, TEXT_COL);
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
        for cell in &cells[0..4] {
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
        // İşaret safhayı söylüyor ve sözlük blok şeridininkiyle aynı — artık
        // şekil de aynı (`bt_atlas::RuleKind::Chevron`). Sınırdan yalnız renk
        // geçiyor: karakter geçseydi kullanıcının fontunun `>`'ü çizilirdi.
        let state = live("", "", "", 0);
        let color = |shell| draw_as(&state, shell, COLS).1.sigil;
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
        // İkisi de **metin** çizdirmiyor: `Idle`'da ZLE satır düzenlemiyor,
        // `Unavailable`'da alanlar zaten boş. Caret ayrı bir soru ve yanıtları
        // ayrışıyor — bkz. aşağıdaki iki sınama.
        for status in [
            DockStatus::Idle,
            DockStatus::Unavailable(DockFault::Overflow),
        ] {
            let state = DockState {
                status,
                ..live("% ", "ls", "", 2)
            };
            let (cells, _) = draw(&state, COLS);
            assert_eq!(text(&cells), "", "{status:?} metin çizdirdi");
        }
    }

    #[test]
    fn an_idle_mirror_still_keeps_the_caret_unless_a_command_runs() {
        // **Metinsiz satır caret'siz satır demek değil.** Açılışta ve iki komut
        // arasında ayna `Idle`, ama kullanıcının yazmaya başlayacağı yer dock.
        // Caret'i o pencerelerde ızgarada tutmak, prompt gelince **sıçratırdı**
        // — gözlenen kusur buydu (012 phase-8).
        let state = DockState {
            status: DockStatus::Idle,
            ..live("% ", "ls", "", 2)
        };
        for shell in [
            None,
            Some(ShellState {
                phase: ShellPhase::Prompt,
                last_exit: None,
            }),
            Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(0),
            }),
        ] {
            let (_, dock) = draw_as(&state, shell, COLS);
            assert_eq!(dock.caret, Some(TEXT_COL), "{shell:?} caret vermedi");
        }
        // Komut koşarken satırın sahibi ızgara: `cat`'in beklediği girdi ve
        // `ssh`'ın parola istemi orada yaşıyor.
        let (_, dock) = draw_as(
            &state,
            Some(ShellState {
                phase: ShellPhase::Running,
                last_exit: None,
            }),
            COLS,
        );
        assert_eq!(dock.caret, None, "koşan komutta dock caret verdi");
    }

    #[test]
    fn an_unavailable_mirror_leaves_the_caret_to_the_grid() {
        // Gösteremediğimiz satır ızgarada duruyor (R1.2); caret'i de orada
        // durmalı, yoksa kullanıcı yazdığı yeri göremez. `Idle`'dan ayrıldığı
        // tek nokta bu ve [`DockStatus`]'ün varlık sebebi de bu ayrım.
        let state = DockState {
            status: DockStatus::Unavailable(DockFault::Overflow),
            ..live("% ", "ls", "", 2)
        };
        let (_, dock) = draw_as(&state, None, COLS);
        assert_eq!(dock.caret, None);
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
        assert_eq!(text(&cells), "  tuvwxyz", "pencere sağ uca yapışmadı");
        // Caret son sütunda: `cols - 1`. Bir ötesi pencerenin dışı olurdu.
        assert_eq!(dock.caret, Some(cols - 1));

        // Caret başa dönünce pencere de başa döner.
        let (cells, dock) = draw(&live("", &buffer, "", 0), cols);
        assert_eq!(text(&cells), "  abcdefgh");
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
        assert_eq!(text(&cells), "");
        assert_eq!(dock.caret, None);
    }

    #[test]
    fn the_context_line_sits_under_the_input_and_is_dim() {
        let (cells, _) = draw_with(&live("", "ls", "", 2), &context("/tmp/x", "main"), COLS);
        assert_eq!(text(&cells), "  ls");
        // Yol, ayraç, dal — yan yana ve dock'un **sol kenarında**, yani `>`
        // işaretiyle hizalı. Giriş metniyle hizalansaydı bağlam sebepsiz
        // girintili görünürdü (bkz. [`CONTEXT_COL`]).
        assert_eq!(row_text(&cells, 1), "/tmp/x | main");
        // **Satırın içinde iki kademe var.** Aranan bilgi "hangi klasördeyim",
        // yani yolun son bileşeni; üst dizinler onu yerleştiren bağlam ve
        // geri çekiliyor. Dal da aranan bilgi, o yüzden öne çıkanla aynı
        // tonda. Ayraç bölme işareti, içerik değil.
        let tone = |col: u16| {
            cells
                .iter()
                .find(|cell| cell.row == 1 && cell.col == col)
                .unwrap_or_else(|| panic!("bağlam satırında {col}. sütun yok"))
                .fg
        };
        let normal = THEME.dim_linear();
        let quiet = THEME.separator_linear();
        assert_ne!(normal, quiet, "iki kademe aynı renge düştü: ayrım görünmez");
        for col in 0..=4 {
            assert_eq!(tone(col), quiet, "`/tmp/` öne çıktı ({col}. sütun)");
        }
        assert_eq!(tone(5), normal, "aktif klasör (`x`) geri çekildi");
        assert_eq!(tone(7), quiet, "ayraç içerik gibi çizildi");
        for col in 9..=12 {
            assert_eq!(tone(col), normal, "dal geri çekildi ({col}. sütun)");
        }
    }

    #[test]
    fn a_rootless_or_root_path_is_all_foreground() {
        // İki dejenere hâl ve ikisinde de "son bileşen" ayrımı anlamsız:
        // kökte (`/`) ayrımı yapacak bir üst dizin yok, eğik çizgisiz bir
        // yolda da. Yanlışın yönü **güvenli**: tamamı öne çıkıyor. Ters
        // seçim (tamamı soluk) kullanıcının aradığı tek bilgiyi gizlerdi.
        for path in ["/", "tmp"] {
            let (cells, _) = draw_with(&live("", "", "", 0), &context(path, ""), COLS);
            for cell in cells.iter().filter(|cell| cell.row == 1) {
                assert_eq!(cell.fg, THEME.dim_linear(), "{path}: {cell:?}");
            }
        }
    }

    #[test]
    fn the_context_line_lives_even_when_the_mirror_does_not() {
        // Bağlam aynanın ömrüne bağlı değil: komut koşarken ZLE satırı
        // bırakıyor (`Idle`) ama dizin hâlâ doğru ve kullanıcı ona bakıyor.
        for status in [
            DockStatus::Idle,
            DockStatus::Unavailable(DockFault::Overflow),
        ] {
            let state = DockState {
                status,
                ..live("", "ls", "", 2)
            };
            let (cells, _) = draw_with(&state, &context("/tmp/x", "main"), COLS);
            assert_eq!(text(&cells), "", "{status:?} metin çizdirdi");
            assert_eq!(row_text(&cells, 1), "/tmp/x | main", "{status:?}");
        }
    }

    #[test]
    fn a_missing_branch_takes_the_separator_with_it() {
        // Depo olmayan dizinde yalnız yol; asılı bir ayraç "dal okunamadı" der
        // ve o yanlış olurdu.
        let state = live("", "", "", 0);
        let (cells, _) = draw_with(&state, &context("/tmp/x", ""), COLS);
        assert_eq!(row_text(&cells, 1), "/tmp/x");
        // Simetrik: yol yokken (henüz OSC 7 gelmedi) de ayraç yok.
        let (cells, _) = draw_with(&state, &context("", "main"), COLS);
        assert_eq!(row_text(&cells, 1), "main");
        // İkisi de yoksa satır hiç doğmuyor.
        let (cells, _) = draw_with(&state, &DockContext::default(), COLS);
        assert_eq!(row_text(&cells, 1), "");
    }

    #[test]
    fn a_narrow_dock_trims_the_path_from_the_left_and_keeps_the_branch() {
        // Kuyruk daha bilgilendirici: hangi depodasın sondaki bileşenlerde
        // yazıyor. Dal **asla** kısalmıyor — kısaltılmış bir dal adı yanlış
        // dalda olduğunu düşündürürdü.
        let state = live("", "", "", 0);
        let path = "/a/bb/ccc/dddd";

        // 20 sütun: bağlam sol kenardan başladığı için yirmisi de onun,
        // ` | main` yedisini alıyor, yola 13 — yani `…` ile birlikte son on
        // iki karakter. Sol kenara çekilmek yola **iki sütun kazandırdı**.
        let (cells, _) = draw_with(&state, &context(path, "main"), 20);
        assert_eq!(row_text(&cells, 1), "…/bb/ccc/dddd | main");

        // Daralınca kırpılan hep yol: dokuz sütunda ondan `…d` kalıyor,
        // `main` bütün duruyor.
        let (cells, _) = draw_with(&state, &context(path, "main"), 9);
        assert_eq!(row_text(&cells, 1), "…d | main");

        // Yol için tek sütun bile kalmayınca yalnız dal kalıyor, ayraçsız:
        // kırpılacak şey dal değil.
        let (cells, _) = draw_with(&state, &context(path, "main"), 7);
        assert_eq!(row_text(&cells, 1), "main");

        // Dal **tam** sığdığında yolu tümden düşürüyor: bütçe önce dalın.
        let (cells, _) = draw_with(&state, &context(path, "main"), 4);
        assert_eq!(row_text(&cells, 1), "main");

        // **Dal bile sığmıyorsa hiç çizilmiyor**, kırpılmıyor: `main`'i `ma`
        // diye göstermek kullanıcıya var olmayan bir dalda olduğunu söylerdi.
        // Kalan genişlik yolun ve onun kısaltması işaretli.
        let (cells, _) = draw_with(&state, &context(path, "main"), 3);
        assert_eq!(row_text(&cells, 1), "…dd");
        // Dal sığmıyor ve yol da yoksa satır büsbütün boş — yanlış bir şey
        // göstermektense hiçbir şey.
        let (cells, _) = draw_with(&state, &context("", "main"), 3);
        assert_eq!(row_text(&cells, 1), "");

        // Sığan yol kısalmıyor ve `…` eklenmiyor.
        let (cells, _) = draw_with(&state, &context(path, "main"), 40);
        assert_eq!(row_text(&cells, 1), "/a/bb/ccc/dddd | main");
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
