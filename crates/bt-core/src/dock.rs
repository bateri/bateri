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

use unicode_width::UnicodeWidthChar;

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

/// Dock'un iki satırının sütun bütçesi.
///
/// **Tek tip, iki sayı** ve ayrı parametre olarak taşınmıyorlar: ikisi de
/// `u16` ve ikisi de "kaç sütun" — imzada yan yana dursalardı çağıran onları
/// sessizce ters geçirebilirdi ve belirti yalnız dar pencerede, yalnız bağlam
/// satırında görünürdü. Aynı sebeple [`Session::dock`] de bu tipi alıyor
/// ([`crate::Session::dock`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockCols {
    /// Giriş satırının genişliği: ızgaranın sütun sayısı. Dock aynı sütunları
    /// kullanıyor ve taşan satır soldan pencereleniyor.
    pub grid: u16,
    /// Bağlam satırının bütçesi. Ayrı bir sayı, çünkü o satır **küçük
    /// puntoda** çiziliyor: aynı piksel şeridine daha çok harf sığıyor.
    /// Sayıyı çizen taraf veriyor (`bt_gpu`'nun `context_cols`'u), bu crate
    /// piksel görmüyor — değer bir **bütçe**, punto kararı değil. `grid` ile
    /// eşit geçilirse satır bugünkü gibi davranır.
    pub context: u16,
}

/// Metnin başladığı sütun: işaret bir hücre, bir hücre de nefes payı.
///
/// Sabit, çünkü işaret **tek** karakter ve ayna onu görmüyor — aynanın
/// `PREDISPLAY`'i kabuğun prompt'u, bu ise terminalin kendi işareti.
///
/// **Yalnız giriş satırının hizası**; bağlam satırı sol kenardan başlıyor
/// ([`CONTEXT_COL`]).
///
/// Crate dışına `DOCK_TEXT_COL` adıyla çıkıyor: `bt-gpu`'nun yazım efektleri
/// pencereyle kayan bir hayaleti metnin sütunlarının dışında (işaretin
/// üstünde) bırakmıyor. İkinci bir kopya değil, aynı sabitin okuyucusu.
pub const TEXT_COL: u16 = 2;

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

/// Bir düzenlemenin taşıyabileceği en çok glyph — **tasarım sabiti**.
///
/// Canlanan düzenleme yazımın kendisi: basılı Backspace kare başına bir
/// glyph, hızlı yazım iki-üç. Sınırı aşan bir düzenleme yazım gibi
/// okunmuyor (kare yolu bir süre durmuş ve girdi birikmiş demek) ve
/// [`DockEdit::Reset`]'e düşüyor — yanlışın yönü güvenli, metin anında
/// belirir. Sabit kapasite kare başına ayırmayı sıfırda tutuyor.
pub const EDIT_MAX: usize = 8;

/// Dock'un giriş satırında **bu karede** ne değişti — yazım animasyonlarının
/// girdisi (030).
///
/// Karar burada: hangi glyph'i kullanıcı yazdı, hangisini sildi, hangi değişim
/// canlanmamalı (yapıştırma, geçmiş, tamamlama). Zaman ve çizim `bt-gpu`'da.
/// Kural ve tablosu `.tasks/030-dock-yazim-animasyonlari/discussion.md` →
/// Karar 2; sınırdan neden ikinci bir sink geçtiği → Karar 3.
///
/// **Pencerenin kayması `shift` olarak geçiyor, `Reset` olarak değil**
/// (kullanıcı kararı, `plan.md` → R1.3): taşan satırda her tuş pencereyi
/// kaydırıyor ve kayma sıfırlasaydı uzun bir komutta hiçbir harf canlanmazdı.
/// `shift` son çizilen karedeki metnin ekranda kaç sütun kaydığı — eski
/// pencerenin attığı sütun eksi yenisininki, yani sağa pozitif. Uçuştaki
/// efektler o kadar kayıyor; sayının tek üreticisi [`render`]'ın pencereleme
/// hesabı, `bt-gpu` sütun defteri tutmuyor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockEdit {
    /// Glyph'ler geldi. `col` koşunun **ilk** ekran sütunu (yeni pencerede);
    /// hücreler normal sink'e de gidiyor, hangisinin çizileceği boyamanın
    /// kararı.
    Arrive {
        col: u16,
        cells: EditCells,
        shift: i32,
    },
    /// Glyph'ler gitti. `col` silinmenin ekran sütunu (caret'in sütunu, yeni
    /// pencerede) ve hayaletler **eski** satırın vurgusuyla çözülmüş.
    Erase {
        col: u16,
        ghosts: EditCells,
        shift: i32,
    },
    /// Metin değişmedi ama pencere kaydı (caret taşan satırda gezindi):
    /// uçuştaki efektler yalnız kayıyor, hiçbiri bitmiyor.
    Shift { by: i32 },
    /// Canlanmayan bir değişim: uçuştaki her efekt bitmeli.
    Reset,
}

/// [`DockEdit`]'in hücreleri: sabit kapasiteli ([`EDIT_MAX`]) bir liste.
///
/// Yalnız **glyph'i olan** hücreler: boşluk ve spacer sütunu hareket edecek
/// mürekkep taşımıyor, zeminleri normal sink'ten çiziliyor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditCells {
    len: usize,
    cells: [Cell; EDIT_MAX],
}

impl EditCells {
    fn empty() -> Self {
        Self {
            len: 0,
            cells: [Cell::default(); EDIT_MAX],
        }
    }

    /// Hücreler, sütun sırasıyla.
    pub fn as_slice(&self) -> &[Cell] {
        self.cells.get(..self.len).unwrap_or(&[])
    }

    /// Doluysa sessizce düşürür; kapasiteyi [`diff`] zaten sınırlıyor.
    fn push(&mut self, cell: Cell) {
        if let Some(slot) = self.cells.get_mut(self.len) {
            *slot = cell;
            self.len += 1;
        }
    }
}

/// Hücrelerden bir kap; [`EDIT_MAX`]'ı aşan hücre sessizce düşer.
///
/// Sınırın öteki yakası (`bt-gpu`'nun sınamaları) düzenleme kurabilsin diye:
/// üretimde kabı yalnız [`render`] dolduruyor.
impl FromIterator<Cell> for EditCells {
    fn from_iter<I: IntoIterator<Item = Cell>>(cells: I) -> Self {
        let mut out = Self::empty();
        for cell in cells {
            out.push(cell);
        }
        out
    }
}

/// Aynanın son çizilen hâlinden bu yana ne değişti — [`render`]'ın
/// [`DockEdit`]'e çevireceği ham hâl.
///
/// **Ham, çünkü sütun yok:** ekran sütunu pencerelemeden çıkıyor ve onu
/// [`render`] kendi döngüsünde zaten hesaplıyor; ikinci bir kopyası burada
/// doğmuyor. Eski taraftan yalnız yeni tamponda artık olmayan şey taşınıyor —
/// hayaletin karakteri, vurgusu ve eski pencerenin kayması — çünkü çağıran
/// ([`crate::Session::dock`]) bu hesaptan hemen sonra tamponu yeni aynayla
/// eziyor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Change {
    /// Canlanmayan değişim.
    Reset,
    /// Ayna ilerledi ama `BUFFER` aynı (öneri değişti, caret kıpırdadı).
    /// Pencere kaydıysa [`DockEdit::Shift`]: uçuştaki efektler metinle
    /// birlikte kaymalı, yoksa eski sütunlarında kalırlardı.
    Same { old_skip: Option<usize> },
    /// Yeni görüntünün `start..end` karakterleri eklendi.
    Insert {
        old_skip: Option<usize>,
        start: usize,
        end: usize,
    },
    /// Eski `BUFFER`'dan glyph'ler silindi; yenisinde yoklar.
    Delete {
        old_skip: Option<usize>,
        ghosts: Ghosts,
    },
}

/// Silinen glyph'ler ve eski satırdaki vurguları.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ghosts {
    len: usize,
    chars: [(char, HighlightStyle); EDIT_MAX],
}

impl Ghosts {
    fn as_slice(&self) -> &[(char, HighlightStyle)] {
        self.chars.get(..self.len).unwrap_or(&[])
    }
}

/// Kare kapısı: aynanın damgası ya da durumu ilerlemediyse değişim yok ve
/// [`diff`] hiç koşmuyor.
///
/// Yeni girdi yoksa canlanacak bir düzenleme de yok, yani olağan içerik
/// karesinin (koşan komutun çıktısı, sayaç) bedeli bir karşılaştırma. Durum
/// da soruluyor, çünkü damgayı taşımayan bir geçiş (`Unavailable` sıfır
/// damgalı) uçuştaki efektleri bitirmeli.
///
/// `old` **son çizilen** ayna olmak zorunda: çağıranın tamponu
/// ([`crate::Session::dock`]'un `into`'su), yeni aynayla ezilmeden önce.
pub(crate) fn change(old: &DockState, new: &DockState, cols: u16) -> Option<Change> {
    (old.answers != new.answers || old.status != new.status).then(|| diff(old, new, cols))
}

/// İki ayna arasındaki düzenleme: yalnız tek bitişik ekleme ya da silme
/// canlanıyor ve glyph sayısı aradaki girdi sayısını aşamıyor.
///
/// **Yalnız `BUFFER`:** `POSTDISPLAY` (autosuggestions'ın önerisi) her tuşta
/// toptan değişiyor ve kullanıcının yazdığı değil. **Yön `CURSOR`'dan**:
/// ekleme yeni caret'te biter, silme (Backspace de ileri silme de) yeni
/// caret'te başlar. Hipotez türetilmiyor, **sınanıyor** — tutmazsa `Reset`.
///
/// **Glyph genişliği sıfırdan büyük karakter:** `❤️` iki kod noktası ama
/// tek girdi ve tek glyph; birleştirici [`render`]'da da hücre almıyor.
///
/// **Taban `Live` ya da `Idle`:** `Idle` boş satır — Enter'dan sonraki ilk
/// tuşun tabanı o. `PREDISPLAY` değiştiyse metin kaymıştır, `Reset`.
pub(crate) fn diff(old: &DockState, new: &DockState, cols: u16) -> Change {
    let (old_buffer, old_skip) = match old.status {
        DockStatus::Live => {
            if old.predisplay != new.predisplay {
                return Change::Reset;
            }
            let available = usize::from(cols.saturating_sub(TEXT_COL));
            (old.buffer.as_str(), Some(window_skip(old, available)))
        }
        // Ekranda hiçbir şey yok, yani kayacak bir şey de yok.
        DockStatus::Idle => ("", None),
        _ => return Change::Reset,
    };
    if new.status != DockStatus::Live {
        return Change::Reset;
    }
    if old_buffer == new.buffer {
        return Change::Same { old_skip };
    }
    let Some(inputs) = new.answers.checked_sub(old.answers) else {
        return Change::Reset;
    };
    let pre = new.predisplay.chars().count();
    let Some(caret) = new.cursor.checked_sub(pre) else {
        return Change::Reset;
    };
    let old_len = old_buffer.chars().count();
    let new_len = new.buffer.chars().count();
    let glyphs =
        |run: &mut dyn Iterator<Item = char>| run.filter(|&ch| column_width(ch) > 0).count();
    let fits = |count: usize| count > 0 && count <= EDIT_MAX && count as u64 <= inputs;

    if new_len > old_len {
        // Ekleme: `old == new[..start] ++ new[caret..]`.
        let Some(start) = caret.checked_sub(new_len - old_len) else {
            return Change::Reset;
        };
        let rest = new
            .buffer
            .chars()
            .take(start)
            .chain(new.buffer.chars().skip(caret));
        if !old_buffer.chars().eq(rest) {
            return Change::Reset;
        }
        if !fits(glyphs(
            &mut new.buffer.chars().skip(start).take(caret - start),
        )) {
            return Change::Reset;
        }
        Change::Insert {
            old_skip,
            start: pre + start,
            end: pre + caret,
        }
    } else {
        // Silme: `new == old[..caret] ++ old[caret + k..]`. `Idle` tabanda
        // eski satır boş, yani buraya eşit uzunlukta bir değiştirme düşüyor
        // ve `k = 0` onu aşağıdaki `fits`'te eliyor.
        let count = old_len - new_len;
        let rest = old_buffer
            .chars()
            .take(caret)
            .chain(old_buffer.chars().skip(caret + count));
        if count == 0 || !new.buffer.chars().eq(rest) {
            return Change::Reset;
        }
        let mut ghosts = Ghosts {
            len: 0,
            chars: [(' ', HighlightStyle::default()); EDIT_MAX],
        };
        let run = old_buffer.chars().enumerate().skip(caret).take(count);
        for (index, ch) in run.filter(|&(_, ch)| column_width(ch) > 0) {
            let Some(slot) = ghosts.chars.get_mut(ghosts.len) else {
                return Change::Reset;
            };
            // Vurgu **eski** görüntüden: yenisinde bu karakter yok.
            *slot = (ch, style_at(old, pre + index));
            ghosts.len += 1;
        }
        if !fits(ghosts.len) {
            return Change::Reset;
        }
        Change::Delete { old_skip, ghosts }
    }
}

/// Pencerenin soldan attığı sütun sayısı: caret ve **altındaki karakterin
/// tamamı** görünür kalacak kadar. Gerekçeleri [`render`]'ın gövdesinde.
///
/// Tek formül, iki tüketici: [`render`] ve [`diff`] (eski pencerenin kayması).
/// `Live` olmayan aynada metin yok, yani kayma da yok.
fn window_skip(state: &DockState, available: usize) -> usize {
    if state.status != DockStatus::Live || available == 0 {
        return 0;
    }
    let caret_width = state
        .predisplay
        .chars()
        .chain(state.buffer.chars())
        .chain(state.postdisplay.chars())
        .nth(state.cursor)
        .map_or(1, |ch| column_width(ch).max(1))
        .min(available);
    (state.cursor_col + caret_width).saturating_sub(available)
}

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
/// düşürürdü — yazdığını görmeyen bir giriş satırı). Pencereleme **sütun**
/// biriminde (024): karakter sayan bir pencere CJK'lı bir satırda caret'i
/// kenardan dışarı taşırdı. Kenarda geniş glyph **yarılanmıyor** — sığmayan
/// karakter hiç çizilmiyor ve o sütun boş kalıyor.
///
/// `context_cols` bağlam satırının bütçesi ve ayrı bir sayı, çünkü o satır
/// **küçük puntoda** çiziliyor: aynı genişliğe daha çok harf sığıyor. Sayıyı
/// çizen taraf veriyor (`bt-gpu`), bu crate piksel görmüyor — `cols`'un
/// kendisiyle aynı sözleşme. İkisi eşit geçilirse satır bugünkü gibi davranır,
/// yani değer bir **bütçe**dir, punto kararı değil.
///
/// `change` son çizilen aynadan bu yana ne değiştiği ([`change`]'in cevabı);
/// [`DockEdit`]'e burada, **bu pencerelemenin** sütunlarıyla çevrilip
/// `edits`'e basılıyor — karede en çok bir kez. Canlanma yalnız metnin
/// çizildiği ve caret'in dock'ta olduğu kolda: satır ızgaradaysa efektin
/// konusu yok ve her canlanmayan kol uçuştakileri bitirir (`Reset`).
///
/// Parametreler dokuz ve her biri ayrı bir girdi; bir yapıya toplamak yalnız
/// bu çağrı için bir tip doğururdu.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    state: &DockState,
    context: &DockContext,
    shell: Option<ShellState>,
    theme: &Theme,
    cols: DockCols,
    owned: bool,
    change: Option<&Change>,
    mut sink: impl FnMut(Cell),
    mut edits: impl FnMut(DockEdit),
) -> Dock {
    let mut surface = Dock {
        ground: theme.background_linear(),
        separator: theme.separator_linear(),
        caret: None,
        caret_text: theme.background_linear(),
        sigil: sigil_color(shell, theme),
    };
    if cols.grid == 0 {
        settle(change, &mut edits);
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
    render_context(context, theme, cols.context, &mut sink);

    let available = usize::from(cols.grid.saturating_sub(TEXT_COL));
    if available == 0 {
        settle(change, &mut edits);
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
        settle(change, &mut edits);
        return surface;
    }
    let fixed = theme.foreground_linear();
    // Öneri sönük: "henüz yazılmamış metin" ile SGR 2'nin sorduğu şey aynı.
    let suggestion = theme.dim_linear();
    let stream = || {
        state
            .predisplay
            .chars()
            .chain(state.buffer.chars())
            .map(|ch| (ch, fixed))
            .chain(state.postdisplay.chars().map(|ch| (ch, suggestion)))
    };

    // **Caret'in sütunu, indeksi değil** — ve **çözücüden** geliyor, burada
    // yeniden sayılmıyor. `CURSOR` karakter indeksi (ZLE'nin birimi) ama
    // görüntünün birimi sütun: geniş bir karakter indeksi bir, sütunu iki
    // ilerletiyor.
    //
    // İlk yazım bu öneki burada geziyordu ve `/audit` (mercek 4) onu iki
    // kusurla birden yakaladı: kare yolunda **ikinci** bir O(n) gezinti
    // (aşağıdaki `nth`'in yanında) ve aynı sayının **ikinci üreticisi** —
    // tam da bu setin kaçındığı koku. `DockState::cursor_col` phase-2'de
    // doğdu ve tek sahip o.
    let caret_col = state.cursor_col;
    // **Pencere caret'in altındaki karakterin tamamını ayırıyor.** İlk yazım
    // sabit `+ 1` idi ve set kapısı (`/code-review`) onu yakaladı: caret
    // kaydırılmış bir satırda geniş bir glyph'in üstünde duruyorsa o glyph
    // iki sütun ister, pencere biri ayırır ve sağ kenar kuralı glyph'i
    // **hiç çizmez** — caret boş bir hücrenin üstünde kalır. 024 öncesinde
    // caret'in altındaki karakter her zaman çiziliyordu, yani sabit `1` bir
    // regresyondu.
    //
    // Karar 2 ("kenarda yarılanma yok") caret'ten **sonraki** karakteri
    // kapsıyordu; bu satır onun altındakini kapsıyor ve ikisi aynı cümlenin
    // iki yarısı. Satır sonunda caret bir karakterin üstünde değil, o yüzden
    // pay 1'e iniyor.
    // Pay **pencereden büyük olamaz** (`min(available)`): `available == 1` ve
    // caret'in altında iki sütunluk bir karakter varken pay pencereyi aşar,
    // `caret_col - skip` negatife düşer ve aşağıdaki çıkarma taşar — debug'da
    // `bt-core`'da kare yolunda panik (depo bunu yasaklıyor), release'de
    // sarma ve caret prompt işaretinin payına düşer. Set kapısı
    // (`/code-review`) yakaladı; kökü payın kendisiydi, yani bir önceki
    // kapının düzeltmesi kendi kenarını doğurmuştu.
    //
    // Kırpma Karar 2'yi **bozmuyor**: sığmayan karakter yine çizilmiyor
    // (sağ kenar kuralı ayrı), yalnız caret son sütuna sabitleniyor.
    //
    // Formül [`window_skip`]'te, çünkü ikinci bir tüketicisi var: [`diff`]
    // eski pencerenin kaymasını aynı hesapla buluyor.
    let skip = window_skip(state, available);

    // **Pencerenin kayması sütun farkı olarak geçiyor** (kullanıcı kararı,
    // `plan.md` → R1.3): taşan satırda her tuş pencereyi kaydırıyor ve
    // kayma sıfırlasaydı uzun bir komutta hiçbir harf canlanmazdı. Fark
    // eski pencerenin attığı sütun eksi yenisininki — metin ekranda o kadar
    // sağa kaydı; uçuştaki efektler de o kadar kayıyor ([`DockEdit`]).
    let shift = match change {
        Some(
            Change::Same {
                old_skip: Some(old),
            }
            | Change::Insert {
                old_skip: Some(old),
                ..
            }
            | Change::Delete {
                old_skip: Some(old),
                ..
            },
        ) => {
            // audit: iki pencere de `≤ cols` sütun atıyor ve `cols` `u16`.
            *old as i32 - skip as i32
        }
        _ => 0,
    };
    let arriving = match change {
        Some(&Change::Insert { start, end, .. }) if owned => start..end,
        _ => 0..0,
    };
    let mut arrive_col = None;
    let mut arrived = EditCells::empty();

    // Mutlak sütun (kaydırma çıkarılmadan önce). Döngü boyunca birikiyor ve
    // `index`'ten **bağımsız**: ayrıştıkları yer tam olarak bu setin konusu.
    let mut col_acc = 0usize;
    for (index, (ch, base)) in stream().enumerate() {
        let width = column_width(ch);
        // **Sıfır genişlikli kod noktası hücre almıyor.** Birleştiriciler
        // (VS16, ZWJ, ten rengi) ızgarada da kendi hücresine sahip değil —
        // alacritty onları `CellExtra`'da tutuyor. Hücre verilseydi önceki
        // karakterin sütununa ikinci bir hücre düşer ve glyph'ini örterdi.
        if width == 0 {
            continue;
        }
        // Pencerenin **sol** yakası ve **tek** koşul yetiyor: `width >= 1`
        // (sıfır yukarıda döndü), yani tamamen soldaki karakter de
        // (`col_acc + width <= skip`) kenara binen karakter de bu testten
        // geçiyor. İkisini ayrı yazmak ölü bir disjunct olurdu ve sonraki
        // okuyanı yanlış yarıyı "düzeltmeye" çağırırdı (set kapısı,
        // `/code-review`). Kenara binen karakter çizilmiyor: yarım glyph
        // sessiz bir bozulma, boşluk görünür bir eksiklik
        // (`discussion.md` → Karar 2).
        if col_acc < skip {
            col_acc += width;
            continue;
        }
        let visible = col_acc - skip;
        // Pencerenin **sağ** yakası, aynı kural: sığmayan geniş karakter
        // yarılanmıyor, o sütun boş kalıyor ve döngü biter (sonraki
        // karakterler daha da sağda).
        if visible + width > available {
            break;
        }
        // audit: `visible < available ≤ cols` ve `cols` `u16`; toplam taşamaz.
        let col = TEXT_COL + visible as u16;
        let lead = cell(ch, col, base, style_at(state, index), theme, width == 2);
        // Gelen glyph'ler **aynı** döngüden ve aynı hücreyle: pencereleme,
        // geniş karakter ve kenar kuralı ikinci kez yazılmıyor.
        if arriving.contains(&index) {
            arrive_col.get_or_insert(col);
            if lead.ch.is_some() {
                arrived.push(lead);
            }
        }
        // `frame()`'in atlama kapısının dock karşılığı: ne mürekkebi, ne
        // zemini, ne çizgisi olan hücre sink'e hiç uğramaz. Vurgusuz bir
        // satırda boşlukların çoğu buradan eleniyor ve `hucre=` jetonunun
        // dock kardeşi olmadığı için sayının tek tüketicisi bu tasarruf.
        if lead.ch.is_some() || lead.bg.is_some() || lead.underline != UnderlineStyle::None {
            sink(lead);
        }
        if width == 2 {
            // **Spacer sütununa zemin.** Glyph'i yok (onu baş hücrenin
            // `wide`'ı çiziyor) ama zemini ve kuralları var: ızgaranın
            // `WIDE_CHAR_SPACER` kolunun aynısı ve gerekçesi `frame()`'de
            // yazılı — "hücreyi tümden elemek onun sağ yarısını renksiz
            // bırakırdı". Bu olmadan `region_highlight`'ın zemini geniş
            // karakterin sağ yarısında biterdi.
            let spacer = Cell {
                col: col + 1,
                ch: None,
                wide: false,
                ..lead
            };
            if spacer.bg.is_some() || spacer.underline != UnderlineStyle::None {
                sink(spacer);
            }
        }
        col_acc += width;
    }

    // audit: `skip`'in tanımı gereği `caret_col - skip < available ≤ cols`.
    let caret_visible = caret_col - skip;
    let caret_screen = TEXT_COL + caret_visible as u16;
    match change {
        None => {}
        Some(Change::Same { .. }) if shift == 0 => {}
        // Metin aynı, pencere kaydı: caret ızgaradaysa da uçuştakiler
        // bitmiyor, yalnız kayıyor — satır hâlâ bu pencerede çiziliyor.
        Some(Change::Same { .. }) => edits(DockEdit::Shift { by: shift }),
        Some(Change::Insert { .. }) if owned => edits(DockEdit::Arrive {
            // Koşunun tamamı pencerenin solunda kaldıysa (pencereden geniş
            // bir koşu) hücresi de yok; sütun yine caret'in solunda.
            col: arrive_col.unwrap_or(caret_screen),
            cells: arrived,
            shift,
        }),
        Some(Change::Delete { ghosts, .. }) if owned => {
            // Hayaletler caret'in sütunundan sağa: silme (Backspace de ileri
            // silme de) yeni caret'te başlıyor. Sütun **yeni** pencerede,
            // yani hayalet de metinle birlikte kaymış yerinde doğuyor.
            let mut cells = EditCells::empty();
            let mut offset = caret_visible;
            for &(ch, style) in ghosts.as_slice() {
                let width = column_width(ch);
                // Sağ yaka, satırın kuralıyla: sığmayan hayalet çizilmez.
                if offset + width > available {
                    break;
                }
                // audit: `offset + width <= available ≤ cols`; taşamaz.
                let ghost = cell(
                    ch,
                    TEXT_COL + offset as u16,
                    fixed,
                    style,
                    theme,
                    width == 2,
                );
                if ghost.ch.is_some() {
                    cells.push(ghost);
                }
                offset += width;
            }
            edits(DockEdit::Erase {
                col: caret_screen,
                ghosts: cells,
                shift,
            });
        }
        Some(_) => edits(DockEdit::Reset),
    }

    Dock {
        caret: owned.then_some(caret_screen),
        ..surface
    }
}

/// Metnin çizilmediği kolun düzenlemesi: aynanın ilerlediği her kol
/// uçuştakileri bitirir, `BUFFER`'ı değişmeyen ayna hiçbir şey basmaz.
fn settle(change: Option<&Change>, edits: &mut impl FnMut(DockEdit)) {
    if matches!(change, Some(change) if !matches!(change, Change::Same { .. })) {
        edits(DockEdit::Reset);
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
/// zevk, kaybı bilgi olurdu. **Bu satır karakter biriminde kalıyor** ve
/// gerekçesi giriş satırınınkinden başka: bağlam satırı **küçük boy
/// sınıfında** çiziliyor, sütun adımı küçük yüzün ilerlemesi ve geniş yol
/// orada kapalı (021'in emsali). Yani CJK'lı bir yol burada hâlâ sütun
/// kaydırıyor — bilinen sınır, bekçisi
/// `the_context_line_keeps_character_columns`.
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
    // Sönük olan **yeni bir renk değil**: sönüğün sönüğü
    // (`Theme::quiet_linear`), yani aynı kuralın (`dim_toward`) ikinci
    // uygulaması. Saç çizgisi bir adım daha ötede ve orada durmasının sebebi
    // var: o **mürekkep değil**, bu hâlâ okunması gereken bir yol.
    let normal = theme.dim_linear();
    let quiet = theme.quiet_linear();
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

/// Karakterin kaç **sütun** tuttuğu; sıfır genişlikli ise `0`.
///
/// Kaynak `unicode-width` ve bu bir tercih değil **zorunluluk**: ızgara aynı
/// crate'i kullanıyor (alacritty `Flags::WIDE_CHAR`'ı onunla kuruyor) ve
/// ikinci bir genişlik kaynağı ayrıştığı gün belirtisi sessiz olurdu — dock
/// bir sütun kayar. Kararın kaydı
/// `.tasks/024-dock-sutun-aritmetigi/discussion.md` → Karar 1.
///
/// **İki ayrı sıfır var ve ayrımı `Option` taşıyor.** `width()` kontrol
/// karakterlerinde `None`, birleştiricilerde (VS16, ZWJ, aksan) `Some(0)`
/// dönüyor (ölçüldü) ve ikisi burada **ayrı** karşılanıyor:
///
/// - `Some(0)` → **0 sütun.** Birleştirici ızgarada da kendi hücresine sahip
///   değil (alacritty `CellExtra`), yani sütun tüketmemesi doğru.
/// - `None` → **1 sütun.** Kontrol karakteri dock'ta çizilmiyor ([`cell`])
///   ama 024 öncesinde **sütununu tutuyordu** (her indeks bir sütundu) ve
///   sıfıra indirmek bir regresyon olurdu: `Ctrl-V` ile eklenmiş bir TAB'ın
///   iki yanındaki kelimeler birleşir ve caret kontrol karakteri başına bir
///   sütun sola kayardı. Set kapısı (`/code-review`) bunu yakaladı.
///
/// **Bilinen sınır ve yönü değişti.** Doğru görüntü ne 0 ne 1: zsh kontrol
/// karakterini `^C` diye **iki** sütunda gösteriyor. [`cell`]'in doc'u bir
/// yer tutucu çizmemenin gerekçesini "sütun aritmetiğini karakter biriminden
/// çıkarır" diye yazmıştı ve o kısıt bu setle **kalktı** — artık aritmetik
/// zaten sütun. Yani `^C` çizmek bugün mümkün; yapılmadı çünkü bu setin
/// konusu değil ve kimse istemedi. **025'ten beri sınır sekmeye daraldı:**
/// öteki kontrol karakterlerini taşıyan satır [`DockStatus::Control`] ile
/// ızgarada kalıyor ve bu fonksiyona hiç gelmiyor.
pub(crate) fn column_width(ch: char) -> usize {
    // `unwrap_or(1)`, `unwrap_or(0)` değil: bkz. doc.
    UnicodeWidthChar::width(ch).unwrap_or(1)
}

/// Bir karakterin hücresi: taban rengi + aralığın stili.
fn cell(
    ch: char,
    col: u16,
    base: LinearRgba,
    style: HighlightStyle,
    theme: &Theme,
    wide: bool,
) -> Cell {
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
        // karakterleri de üretmiyor, ama artık buraya yalnız **sekme**
        // ulaşıyor: öteki kontrol karakterlerini taşıyan satır
        // [`DockStatus::Control`] ile ızgarada kalıyor ve dock onu hiç
        // çizmiyor (025) — ZLE ham baytı ızgarada okunur bir `^A` diye
        // basıyor, dock ise o sütunu boş bırakırdı. Yerinde bir yer tutucu
        // (`^C`) çizmek 024'ten beri mümkün (aritmetik zaten sütun) ve o gün
        // `Control` kolu silinir. Ayrıntı [`column_width`]'in doc'unda.
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
        // **Geniş yol dock'ta artık açık** (024): yukarıdaki sütun karakter
        // indeksinden değil **genişlikten** birikiyor, yani iki hücrelik bir
        // glyph'in sağ sütunu gerçekten ayrılmış oluyor ve komşusunun üstüne
        // boyamıyor. 023'te bu satır `false` sabitiydi ve gerekçesi *o
        // aritmetikle* sağlamdı; aritmetik değişince değişmez de kalktı.
        wide,
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
            // Sütun ikizleri **aynı fonksiyondan** ([`column_width`]), yani
            // elle kurulmuş bir durum da üretimdeki aritmetiği taşıyor.
            display_cols: predisplay
                .chars()
                .chain(buffer.chars())
                .chain(postdisplay.chars())
                .map(column_width)
                .sum(),
            cursor_col: predisplay
                .chars()
                .chain(buffer.chars())
                .chain(postdisplay.chars())
                .take(cursor)
                .map(column_width)
                .sum(),
            last_ink: predisplay
                .chars()
                .chain(buffer.chars())
                .chain(postdisplay.chars())
                .filter(|ch| !ch.is_whitespace())
                .next_back(),
            // Bu modül okumuyor (tüketicisi `Session::can_be_typed`); canlı
            // bir satırın olağan hâli ekleme keymap'i.
            insert_keymap: true,
            // Tazelik kapısının damgası; `render` okumuyor.
            answers: 0,
        }
    }

    /// Bağlamsız çizim: yol da dal da boş (bu modülün eski sınamalarının hâli).
    /// İki satırın bütçesi eşit: bu modülün sınamaları **çizimi** soruyor,
    /// puntoyu değil. Bütçenin ayrıştığı hâlin kendi sınaması var
    /// (`context_line_spends_its_own_budget`).
    fn same(cols: u16) -> DockCols {
        DockCols {
            grid: cols,
            context: cols,
        }
    }

    fn draw(state: &DockState, cols: u16) -> (Vec<Cell>, Dock) {
        draw_with(state, &DockContext::default(), cols)
    }

    /// Çizilen hücreler, sütun sırasıyla.
    fn draw_with(state: &DockState, context: &DockContext, cols: u16) -> (Vec<Cell>, Dock) {
        let mut cells = Vec::new();
        // Sahiplik sınamanın girdisi değil: üretimde `Session::frame` veriyor,
        // burada aynı yüklemden türetiliyor ki bu modülün sınamaları
        // devrin kuralını değil **çizimi** sınasın. Tutma da bu yüzden kapalı
        // (`held: false`): histerezis devrin **ne zaman** görüneceğini
        // değiştiriyor, çizimini değil.
        let owned = caret_home(None, state.status, false) == CaretHome::Dock;
        let dock = render(
            state,
            context,
            None,
            &THEME,
            same(cols),
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
        );
        (cells, dock)
    }

    /// Safhanın caret'e etkisini soran sınamalar için: kabuğun durumu
    /// çağırandan.
    fn draw_as(state: &DockState, shell: Option<ShellState>, cols: u16) -> (Vec<Cell>, Dock) {
        let mut cells = Vec::new();
        let owned = caret_home(shell, state.status, false) == CaretHome::Dock;
        let dock = render(
            state,
            &DockContext::default(),
            shell,
            &THEME,
            same(cols),
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
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
    fn a_multiline_mirror_draws_nothing_and_keeps_the_caret_in_the_grid() {
        // **Dock'un giriş satırı bir tane.** Çok satırlı bir görüntüyü tek
        // satıra yassıltmak metni görünmez boşluklarla ezer ve caret'i
        // hiçbir harfin üstünde durmayan bir sütuna koyardı — belirti
        // kullanıcıda görüldü (2026-09-21, çok satırlı yapıştırma). Kural
        // `Unavailable`'ınkiyle aynı: satır da caret'i de ızgarada kalır.
        let state = DockState {
            status: DockStatus::Multiline,
            ..live("% ", "echo a\necho b", "", 4)
        };
        let (cells, dock) = draw(&state, COLS);
        assert_eq!(text(&cells), "", "gösteremediğimiz satır dock'a çizildi");
        assert_eq!(dock.caret, None, "çok satırlı aynada dock caret'i aldı");
        // Yüzeyin kendisi duruyor: bant kalkmıyor, yalnız sahiplik ızgaraya
        // geçiyor — bağlam satırı `Live` kapısının **üstünde** çiziliyor
        // ([`render`]), yani dizin ile dal yerinde kalıyor.
        let (with_context, _) = draw_with(&state, &context("/tmp/x", "main"), COLS);
        assert_eq!(row_text(&with_context, CONTEXT_ROW), "/tmp/x | main");
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
    fn the_shell_prompt_is_as_wide_as_the_dock_indent() {
        // **İki kaynak, tek sayı.** Dock'un metni işaretten `TEXT_COL` sütun
        // sonra başlıyor; ızgarada aynı hizayı veren şey zsh betiğinin
        // prompt'u, çünkü komut orada gerçekten o kadar içeriden başlıyor
        // (012 phase-11). Sabit paylaşılamıyor — biri Rust, biri kabuk — ama
        // ayrışmaları **sessiz** olurdu: ızgara ile dock farklı sütundan
        // başlar, kimse kızarmaz.
        //
        // Ölçüt tırnak içindeki boşluk sayısı. Boşluklar `%{…%}` dışında
        // olmak zorunda (zsh onları saymalı); içeri alınsalardı genişlik
        // sıfıra döner ve işaret komutun ilk harfini örterdi.
        let script = include_str!("../../../assets/shell/zsh/bateri.zsh");
        let line = script
            .lines()
            .find(|line| line.contains("__bateri_ps1="))
            .expect("betikte `__bateri_ps1` ataması yok");
        let spaces = format!("'{}'", " ".repeat(usize::from(TEXT_COL)));
        assert!(
            line.contains(&spaces),
            "prompt genişliği `TEXT_COL` ({TEXT_COL}) ile ayrışmış: {line}"
        );
        // Bir fazlası da geçmesin: `contains` tek başına "en az" derdi.
        let wider = format!("'{}'", " ".repeat(usize::from(TEXT_COL) + 1));
        assert!(
            !line.contains(&wider),
            "prompt bir sütun daha geniş: {line}"
        );
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
        let quiet = THEME.quiet_linear();
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
    fn context_line_spends_its_own_budget() {
        // **Bağlam satırının bütçesi giriş satırınınkinden ayrı** ve sebebi
        // punto: o satır küçük yüzle çiziliyor, aynı piksel şeridine daha çok
        // harf sığıyor. Sayıyı çizen taraf veriyor (`bt_gpu::context_cols`);
        // bu crate onu bir **bütçe** olarak alıyor, punto olarak değil.
        let state = live("", "ls", "", 2);
        let path = "/a/bb/ccc/dddd";
        let ctx = context(path, "main");

        // Dokuz sütunluk bir ızgarada giriş satırı dokuza sığıyor, bağlam
        // satırı ise yirmi bire (yol 14 + ayraç 3 + dal 4): kısaltma **büyük**
        // bütçeye göre hesaplanıyor ve yol tam çıkıyor.
        let mut cells = Vec::new();
        let owned = caret_home(None, state.status, false) == CaretHome::Dock;
        let wide = DockCols {
            grid: 9,
            context: 21,
        };
        render(
            &state,
            &ctx,
            None,
            &THEME,
            wide,
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
        );
        assert_eq!(row_text(&cells, 1), "/a/bb/ccc/dddd | main");
        // Giriş satırı **dokunulmamış**: iki bütçe birbirine karışmıyor.
        assert_eq!(text(&cells), "  ls");

        // Aynı ızgara, bütçe dar: kısaltma geri geliyor. Yani satırın gördüğü
        // sayı gerçekten `context_cols`, `cols` değil.
        let mut narrow = Vec::new();
        render(
            &state,
            &ctx,
            None,
            &THEME,
            same(9),
            owned,
            None,
            |cell| narrow.push(cell),
            |_| (),
        );
        assert_eq!(row_text(&narrow, 1), "…d | main");
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

    /// **Geniş karakter dock'ta iki sütun tutuyor** ve baş hücresi
    /// işaretlenmiş oluyor.
    ///
    /// Bu bekçi 023'te **tersinin** bekçisiydi
    /// (`the_dock_never_marks_a_cell_wide`) ve gerekçesi o aritmetikle
    /// sağlamdı: sütun karakter indeksinden türerken iki hücrelik bir glyph
    /// komşusunun üstüne boyardı. 024 aritmetiği değiştirdi, yani değişmez de
    /// kalktı — kutu silinmedi, **iddiası** değişti. Ders `/rfc` → Bulguyu
    /// işleme yolu'nda: gerçekten zorunlu bir kısıt bir sonraki sette
    /// kaldırılamazdı.
    ///
    /// Bağlam satırı kapsamın **dışında**: küçük sınıf, sütun adımı küçük
    /// yüzün ilerlemesi (021'in emsali).
    #[test]
    fn a_wide_char_takes_two_columns_in_the_dock() {
        let state = live("", "漢ls", "", 3);
        let (cells, dock) = draw(&state, COLS);
        let lead = cells
            .iter()
            .find(|cell| cell.ch == Some('漢'))
            .expect("sınama konusuz kalmasın: CJK çizilmiş olmalı");
        assert!(lead.wide, "baş hücre işaretlenmedi: {lead:?}");
        assert_eq!(lead.col, TEXT_COL, "metin ilk sütundan başlar");
        // Komşu sütun **glyph almıyor**: onu baş hücrenin `wide`'ı çiziyor
        // (`bt_gpu::AtlasTexture::prepare` yelpazeliyor). İkinci bir glyph
        // hücresi aynı yere iki dörtlü basardı.
        assert!(
            !cells
                .iter()
                .any(|cell| cell.col == TEXT_COL + 1 && cell.ch.is_some()),
            "spacer sütununa glyph düştü: {cells:?}"
        );
        // Ve sonraki harf **iki** sütun sonra: aritmetiğin tamamı bu satırda.
        let l = cells
            .iter()
            .find(|cell| cell.ch == Some('l'))
            .expect("'l' çizilmeli");
        assert_eq!(l.col, TEXT_COL + 2, "geniş karakter iki sütun tuttu");
        // Caret imleçten önceki **genişliklerin** toplamında: `漢ls` için
        // indeks 3 ama sütun 4.
        assert_eq!(
            dock.caret,
            Some(TEXT_COL + 4),
            "caret sütun değil indeks saydı"
        );
    }

    /// Sıfır genişlikli kod noktası **hücre almıyor**.
    ///
    /// Birleştiriciler (VS16, ZWJ, ten rengi) ızgarada da kendi hücresine
    /// sahip değil — alacritty onları `CellExtra`'da tutuyor. Hücre
    /// verilseydi önceki karakterin sütununa ikinci bir hücre düşer ve
    /// glyph'ini örterdi.
    #[test]
    fn a_zero_width_codepoint_gets_no_cell() {
        // `❤` + VS16: iki karakter, **bir** sütun (`❤` tek sütunlu).
        let state = live("", "\u{2764}\u{fe0f}x", "", 3);
        let (cells, _) = draw(&state, COLS);
        assert_eq!(
            cells.iter().filter(|c| c.ch.is_some()).count(),
            2,
            "VS16 kendi hücresini aldı: {cells:?}"
        );
        let x = cells
            .iter()
            .find(|cell| cell.ch == Some('x'))
            .expect("'x' çizilmeli");
        assert_eq!(x.col, TEXT_COL + 1, "VS16 sütun tüketti");
    }

    /// Vurgu geniş karakterde **iki hücreye** yayılıyor.
    ///
    /// `region_highlight`'ın aralıkları karakter indeksinde (ZLE'nin birimi)
    /// ama boyanan zemin hücre başına: spacer sütununa bir zemin hücresi
    /// düşmezse `"fix 🎉"` dizgisinin sarı zemini emojinin sağ yarısında
    /// biterdi. Izgaranın `WIDE_CHAR_SPACER` kolunun aynısı.
    #[test]
    fn a_highlight_covers_both_cells_of_a_wide_char() {
        let mut state = live("", "漢", "", 1);
        state.highlights.push(Highlight {
            start: 0,
            end: 1,
            style: HighlightStyle {
                bg: Some(HighlightColor::Indexed(3)),
                ..HighlightStyle::default()
            },
        });
        let (cells, _) = draw(&state, COLS);
        let painted: Vec<u16> = cells
            .iter()
            .filter(|cell| cell.bg.is_some())
            .map(|cell| cell.col)
            .collect();
        assert_eq!(
            painted,
            vec![TEXT_COL, TEXT_COL + 1],
            "vurgu geniş karakterin yalnız yarısını boyadı"
        );
    }

    /// Pencere kenarında geniş glyph **yarılanmıyor**.
    ///
    /// Sığmayan karakter hiç çizilmiyor ve o sütun boş kalıyor: 023'ün
    /// sözleşmesi "kutu ya da tam glyph" ve yarım glyph **sessiz** bir
    /// bozulma, boşluk ise görünür bir eksiklik
    /// (`discussion.md` → Karar 2).
    #[test]
    fn a_wide_char_is_never_split_at_the_window_edge() {
        // Bütçe `TEXT_COL + 2`: metne **iki** sütun kalıyor. `a` birini
        // yiyor, `漢` iki ister ve sığmıyor — yani hiç çizilmemeli ve son
        // sütun boş kalmalı. (Üç sütun verilseydi ikisi de sığardı; sınır
        // tam burası.)
        let cols = TEXT_COL + 2;
        // Caret `a`'nın üstünde (indeks 0), yani pencere kaymıyor ve sınanan
        // şey caret'ten **sonraki** karakterin sığmaması. Caret'in kendi
        // karakterinin sığmaması ayrı bir kural ve ayrı bir bekçisi var
        // (`the_window_reserves_the_whole_char_under_the_caret`).
        let state = live("", "a漢", "", 0);
        let mut cells = Vec::new();
        let owned = caret_home(None, state.status, false) == CaretHome::Dock;
        render(
            &state,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
        );
        assert!(
            cells.iter().any(|cell| cell.ch == Some('a')),
            "sığan karakter çizilmedi: {cells:?}"
        );
        assert!(
            !cells.iter().any(|cell| cell.ch == Some('漢')),
            "sığmayan geniş karakter yarılandı: {cells:?}"
        );
    }

    /// Kontrol karakteri **sütununu tutuyor**, çizilmese de.
    ///
    /// İki sıfırın ayrımı: birleştirici sütun tüketmiyor (ızgarada da kendi
    /// hücresi yok), kontrol karakteri tüketiyor. Sıfıra indirilmesi bir
    /// regresyon olurdu — `Ctrl-V` ile eklenmiş bir TAB'ın iki yanındaki
    /// kelimeler birleşir ve caret kontrol karakteri başına bir sütun sola
    /// kayardı. Set kapısı (`/code-review`) bunu yakaladı ve bu bekçi onu
    /// çiviliyor.
    ///
    /// Doğru görüntü ne 0 ne 1 (zsh `^C` diye **iki** sütun gösteriyor) ve o
    /// bilinen sınır [`column_width`]'in doc'unda; bekçi bugünkü davranışı
    /// koruyor, ideali dayatmıyor.
    #[test]
    fn a_control_char_keeps_its_column() {
        // `a` + TAB + `b`: üç sütun, ortadaki çizilmiyor.
        let state = live("", "a\tb", "", 3);
        let (cells, dock) = draw(&state, COLS);
        let drawn: Vec<(u16, Option<char>)> =
            cells.iter().map(|cell| (cell.col, cell.ch)).collect();
        assert_eq!(
            drawn,
            vec![(TEXT_COL, Some('a')), (TEXT_COL + 2, Some('b'))],
            "kontrol karakteri sütununu kaybetti: kelimeler birleşti"
        );
        assert_eq!(
            dock.caret,
            Some(TEXT_COL + 3),
            "caret kontrol karakterinin sütununu saymadı"
        );
    }

    /// **Pencere caret'in altındaki karakterin tamamını ayırıyor.**
    ///
    /// Set kapısının (`/code-review`) bulduğu regresyon: sabit `+ 1` payıyla
    /// caret kaydırılmış bir satırda geniş bir glyph'in üstünde durduğunda o
    /// glyph iki sütun ister, pencere biri ayırır ve sağ kenar kuralı glyph'i
    /// **hiç çizmez** — caret boş bir hücrenin üstünde kalırdı. 024 öncesinde
    /// caret'in altındaki karakter her zaman çiziliyordu.
    ///
    /// Karar 2 ("kenarda yarılanma yok") caret'ten **sonraki** karakteri
    /// kapsıyordu; bu bekçi onun altındakini kapsıyor.
    #[test]
    fn the_window_reserves_the_whole_char_under_the_caret() {
        // İki sütunluk bütçe, caret geniş karakterin üstünde (indeks 1).
        let cols = TEXT_COL + 2;
        let state = live("", "a漢", "", 1);
        let mut cells = Vec::new();
        let owned = caret_home(None, state.status, false) == CaretHome::Dock;
        let dock = render(
            &state,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            owned,
            None,
            |cell| cells.push(cell),
            |_| (),
        );
        let lead = cells
            .iter()
            .find(|cell| cell.ch == Some('漢'))
            .expect("caret'in altındaki karakter çizilmedi");
        assert!(lead.wide, "{lead:?}");
        assert_eq!(
            dock.caret,
            Some(lead.col),
            "caret kendi karakterinin üstünde durmalı"
        );
    }

    /// **Bağlam satırı karakter biriminde kalıyor** — bilinen sınır.
    ///
    /// Gerekçesi geniş glyph'in yokluğu değil **küçük boy sınıfı**: sütun
    /// adımı küçük yüzün ilerlemesi ve geniş yol orada kapalı (021'in
    /// emsali). Yani CJK'lı bir yol burada hâlâ sütun kaydırıyor.
    ///
    /// Bekçi 023'ün silinen sınamasının bıraktığı boşluğu dolduruyor: o,
    /// `render_context`'i CJK'lı bir `cwd` ile geçen **tek** sınamaydı ve
    /// yerine gelen dördü bağlam satırına hiç dokunmuyordu (set kapısı,
    /// `/code-review`).
    #[test]
    fn the_context_line_keeps_character_columns() {
        let state = live("", "ls", "", 2);
        let (cells, _) = draw_with(
            &state,
            &DockContext {
                cwd: "/tmp/漢字".into(),
                branch: "主".into(),
            },
            COLS,
        );
        let context: Vec<&Cell> = cells.iter().filter(|cell| cell.row == 1).collect();
        assert!(
            context.iter().any(|cell| cell.ch == Some('漢')),
            "sınama konusuz kalmasın: bağlam satırı CJK çizmeli"
        );
        // **Hiçbiri geniş işaretli değil** ve olmamalı: küçük sınıfta
        // `Atlas::slot` `Half`'ı zaten `Whole`'a normalize ediyor, yani
        // bayrak konsa bile yelpazeleme koşmaz — ama bayrağı koymak
        // sözleşmeyi iki yerde tutmak olurdu.
        assert!(
            context.iter().all(|cell| !cell.wide),
            "bağlam satırı geniş bayrağı koydu: {context:?}"
        );
        // Sütun **karakter** başına ilerliyor: `漢` ile `字` komşu sütunlarda.
        let cols_of: Vec<u16> = context
            .iter()
            .filter(|cell| cell.ch == Some('漢') || cell.ch == Some('字'))
            .map(|cell| cell.col)
            .collect();
        assert_eq!(cols_of.len(), 2, "iki CJK hücresi beklenir: {context:?}");
        assert_eq!(
            cols_of[1] - cols_of[0],
            1,
            "bağlam satırı sütun saymaya geçmiş (sınır kalktıysa doc'u düzelt)"
        );
    }

    /// Pencere caret'in karakterinden **darsa** taşma yok.
    ///
    /// `available == 1` ve caret'in altında iki sütunluk bir karakter: pay
    /// pencereden büyük, yani `caret_col - skip` negatife düşerdi. Debug'da
    /// `bt-core`'da kare yolunda panik (depo bunu yasaklıyor), release'de
    /// sarma ve caret prompt işaretinin payına düşer. Set kapısı
    /// (`/code-review`) yakaladı ve kökü bir önceki kapının düzeltmesiydi —
    /// `caret_width` payı eklenince kendi kenarını doğurdu.
    #[test]
    fn a_window_narrower_than_the_caret_char_does_not_underflow() {
        for (label, buffer, cursor) in [
            ("caret geniş karakterin üstünde", "漢", 0),
            ("tek sütunda geniş karakter + kuyruk", "漢a", 0),
        ] {
            let state = live("", buffer, "", cursor);
            let mut cells = Vec::new();
            let owned = caret_home(None, state.status, false) == CaretHome::Dock;
            let dock = render(
                &state,
                &DockContext::default(),
                None,
                &THEME,
                same(TEXT_COL + 1),
                owned,
                None,
                |cell| cells.push(cell),
                |_| (),
            );
            // Caret metin alanının **içinde**: prompt işaretinin payına
            // düşmüyor ve pencerenin dışına da taşmıyor.
            let caret = dock.caret.expect("{label}: caret dock'un");
            assert_eq!(caret, TEXT_COL, "{label}: caret {caret}");
        }
    }

    // ---- Yazım animasyonlarının düzenlemesi (030) ----
    //
    // `discussion.md` → Karar 2'nin tablosu: her satırı bir sınama.

    /// Kullanıcının yazdığı satır: `PREDISPLAY` boş, caret `BUFFER`'da,
    /// damga `answers`.
    fn typed(buffer: &str, cursor: usize, answers: u64) -> DockState {
        DockState {
            answers,
            ..live("", buffer, "", cursor)
        }
    }

    /// Satırın sonunda caret.
    fn at_end(buffer: &str, answers: u64) -> DockState {
        typed(buffer, buffer.chars().count(), answers)
    }

    /// `line-finish`'ten sonraki boş ayna, damgasıyla (`End` kolu).
    fn idle(answers: u64) -> DockState {
        DockState {
            status: DockStatus::Idle,
            answers,
            ..DockState::default()
        }
    }

    /// Eski aynadan yenisine: üretimdeki sıra — kapı, sonra çizim — ve
    /// çizimin bastığı düzenlemeler.
    fn edits_between(old: &DockState, new: &DockState, cols: u16) -> Vec<DockEdit> {
        let change = change(old, new, cols);
        let owned = caret_home(None, new.status, false) == CaretHome::Dock;
        let mut edits = Vec::new();
        render(
            new,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            owned,
            change.as_ref(),
            |_| (),
            |edit| edits.push(edit),
        );
        edits
    }

    /// Düzenlemenin karakterleri ve sütunları; `Shift` ve `Reset` → `None`.
    fn glyphs(edit: &DockEdit) -> Option<(u16, String, Vec<u16>)> {
        let (col, cells) = match edit {
            DockEdit::Arrive { col, cells, .. }
            | DockEdit::Erase {
                col, ghosts: cells, ..
            } => (*col, cells.as_slice()),
            DockEdit::Shift { .. } | DockEdit::Reset => return None,
        };
        Some((
            col,
            cells.iter().filter_map(|cell| cell.ch).collect(),
            cells.iter().map(|cell| cell.col).collect(),
        ))
    }

    fn only(edits: &[DockEdit]) -> &DockEdit {
        assert_eq!(
            edits.len(),
            1,
            "karede tek düzenleme bekleniyordu: {edits:?}"
        );
        &edits[0]
    }

    fn arrive(edits: &[DockEdit]) -> (u16, String) {
        match only(edits) {
            edit @ DockEdit::Arrive { .. } => {
                let (col, text, _) = glyphs(edit).expect("geliş");
                (col, text)
            }
            other => panic!("geliş bekleniyordu: {other:?}"),
        }
    }

    fn erase(edits: &[DockEdit]) -> (u16, String) {
        match only(edits) {
            edit @ DockEdit::Erase { .. } => {
                let (col, text, _) = glyphs(edit).expect("silme");
                (col, text)
            }
            other => panic!("silme bekleniyordu: {other:?}"),
        }
    }

    fn reset(edits: &[DockEdit], label: &str) {
        assert_eq!(edits, [DockEdit::Reset], "{label}");
    }

    #[test]
    fn a_typed_letter_arrives_left_of_the_caret() {
        let edits = edits_between(&at_end("l", 1), &at_end("ls", 2), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL + 1, "s".into()));
    }

    #[test]
    fn two_keys_in_one_frame_arrive_together() {
        // Taban son **çizilen** ayna: aradaki ayna atlandı, iki tuş tek koşu.
        let edits = edits_between(&at_end("l", 1), &at_end("lsa", 3), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL + 1, "sa".into()));
    }

    #[test]
    fn backspace_leaves_a_ghost_at_the_caret() {
        let edits = edits_between(&at_end("ls", 2), &at_end("l", 3), COLS);
        assert_eq!(erase(&edits), (TEXT_COL + 1, "s".into()));
        // Basılı Backspace: iki silme tek karede, hayaletler sağa doğru.
        let edits = edits_between(&at_end("lsa", 3), &at_end("l", 5), COLS);
        let DockEdit::Erase { ghosts, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        let cols: Vec<u16> = ghosts.as_slice().iter().map(|cell| cell.col).collect();
        assert_eq!(cols, [TEXT_COL + 1, TEXT_COL + 2]);
    }

    #[test]
    fn forward_delete_leaves_its_ghost_at_the_caret_too() {
        // `lsa`, caret `s`'nin üstünde, ileri silme: caret yerinde kalıyor.
        let edits = edits_between(&typed("lsa", 1, 1), &typed("la", 1, 2), COLS);
        assert_eq!(erase(&edits), (TEXT_COL + 1, "s".into()));
    }

    #[test]
    fn bulk_changes_do_not_animate() {
        for (label, old, new) in [
            // Tek girdi, çok glyph.
            ("yapıştırma", at_end("", 1), at_end("hello", 2)),
            ("Ctrl-U", at_end("git status", 2), at_end("", 3)),
            (
                "Tab tamamlama",
                at_end("git st", 1),
                at_end("git status", 2),
            ),
            // Ekleme de silme de değil: değiştirme.
            ("geçmiş", at_end("ls", 2), at_end("git status", 3)),
            ("eşit boyda geçmiş", at_end("ab", 2), at_end("cd", 3)),
            // Sınır kapasitede: girdi yetse de dokuz glyph yazım gibi okunmuyor.
            ("kapasite", at_end("", 0), at_end("abcdefghi", 9)),
        ] {
            reset(&edits_between(&old, &new, COLS), label);
        }
    }

    #[test]
    fn a_one_char_completion_animates_like_typing() {
        let edits = edits_between(&at_end("cd src", 1), &at_end("cd src/", 2), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL + 6, "/".into()));
    }

    #[test]
    fn a_dead_key_is_two_inputs_for_one_glyph() {
        let edits = edits_between(&at_end("", 0), &at_end("~", 2), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL, "~".into()));
    }

    #[test]
    fn a_combining_mark_is_not_a_glyph() {
        // `❤️` iki kod noktası, tek girdi (emoji paleti): sayılan glyph bir.
        let edits = edits_between(&at_end("", 0), &at_end("❤\u{FE0F}", 1), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL, "❤".into()));
    }

    #[test]
    fn a_mirror_without_input_changes_nothing() {
        // Damga da durum da aynı: kapı kapalı ve `diff` hiç koşmuyor — içerik
        // farklı olsa bile (girdisiz ayna: prompt yenilemesi, zamanlayıcı).
        let old = at_end("ls", 4);
        let new = at_end("ls -la", 4);
        assert_eq!(change(&old, &new, COLS), None);
        assert!(edits_between(&old, &new, COLS).is_empty());
    }

    #[test]
    fn a_new_suggestion_over_the_same_buffer_draws_nothing() {
        let old = at_end("l", 1);
        let new = DockState {
            answers: 2,
            ..live("", "l", "s -la", 1)
        };
        assert_eq!(
            change(&old, &new, COLS),
            Some(Change::Same { old_skip: Some(0) })
        );
        assert!(edits_between(&old, &new, COLS).is_empty());
    }

    #[test]
    fn leaving_live_resets() {
        // Enter: `line-finish` aynayı `Idle`'a indiriyor, satır ızgaraya geçti.
        reset(
            &edits_between(&at_end("ls", 2), &idle(3), COLS),
            "Live → Idle",
        );
        let broken = DockState {
            status: DockStatus::Unavailable(DockFault::Malformed),
            ..DockState::default()
        };
        reset(
            &edits_between(&at_end("ls", 2), &broken, COLS),
            "Live → Unavailable",
        );
        reset(
            &edits_between(&broken, &at_end("l", 1), COLS),
            "Unavailable → Live",
        );
    }

    #[test]
    fn the_first_letter_after_the_prompt_arrives() {
        // Taban `Idle`, boş satır: kural "iki taraf da Live" olsaydı her
        // komutun ilk harfi canlanmazdı.
        let edits = edits_between(&idle(5), &at_end("l", 6), COLS);
        assert_eq!(arrive(&edits), (TEXT_COL, "l".into()));
    }

    #[test]
    fn a_paste_as_the_first_action_does_not_animate() {
        // `Idle` taban damgalı (`End` kolu): girdi sınırı tek, beş glyph aşar.
        reset(
            &edits_between(&idle(5), &at_end("hello", 6), COLS),
            "prompt'taki ilk yapıştırma",
        );
    }

    #[test]
    fn a_wide_char_arrives_as_one_glyph_over_two_columns() {
        let edits = edits_between(&at_end("a", 1), &at_end("a漢", 2), COLS);
        let DockEdit::Arrive { col, cells, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(*col, TEXT_COL + 1);
        let [lead] = cells.as_slice() else {
            panic!("tek hücre bekleniyordu: {cells:?}");
        };
        assert_eq!(
            (lead.ch, lead.col, lead.wide),
            (Some('漢'), TEXT_COL + 1, true)
        );
        // Silinirken de tek hayalet, iki sütun.
        let edits = edits_between(&at_end("a漢", 2), &at_end("a", 3), COLS);
        let DockEdit::Erase { ghosts, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert!(ghosts.as_slice()[0].wide, "{ghosts:?}");
    }

    #[test]
    fn a_typed_space_still_marks_its_column() {
        // Boşluk glyph değil ama sütun kaydırıyor: uçuştaki gelişlerin
        // bitmesi bu sütuna bakıyor (`discussion.md` → Karar 3).
        let edits = edits_between(&at_end("ls", 1), &at_end("ls ", 2), COLS);
        let DockEdit::Arrive { col, cells, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(*col, TEXT_COL + 2);
        assert!(cells.as_slice().is_empty(), "{cells:?}");
    }

    #[test]
    fn edits_follow_the_prompt_width() {
        // `PREDISPLAY` metni sağa itiyor; sütun pencerelemenin kendisinden.
        let old = DockState {
            answers: 1,
            ..live("% ", "l", "", 3)
        };
        let new = DockState {
            answers: 2,
            ..live("% ", "ls", "", 4)
        };
        assert_eq!(
            arrive(&edits_between(&old, &new, COLS)),
            (TEXT_COL + 3, "s".into())
        );
        // `PREDISPLAY` değiştiyse metin kaydı: canlanma yok.
        let moved = DockState {
            answers: 2,
            ..live("%% ", "ls", "", 5)
        };
        reset(&edits_between(&old, &moved, COLS), "PREDISPLAY değişti");
    }

    #[test]
    fn a_ghost_keeps_the_color_of_the_old_line() {
        let mut old = at_end("ls", 2);
        old.highlights.push(Highlight {
            start: 1,
            end: 2,
            style: HighlightStyle {
                fg: Some(HighlightColor::Indexed(2)),
                ..HighlightStyle::default()
            },
        });
        // Yeni satırın vurgusu yok: renk yalnız eski tamponda.
        let edits = edits_between(&old, &at_end("l", 3), COLS);
        let DockEdit::Erase { ghosts, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(ghosts.as_slice()[0].fg, THEME.indexed_linear(2));
    }

    #[test]
    fn a_scrolled_window_places_the_ghost_on_screen() {
        // Pencere 6 sütun (`TEXT_COL` payından sonra 4), satır taşmış ve
        // soldan kaydırılmış; ileri silme caret'i ve pencereyi yerinde
        // bırakıyor — hayalet caret'in **ekran** sütununda.
        let cols = TEXT_COL + 4;
        let old = typed("abcdefgh", 5, 1);
        let new = typed("abcdegh", 5, 2);
        let edits = edits_between(&old, &new, cols);
        let (col, text) = erase(&edits);
        assert_eq!(text, "f");
        // skip = 5 + 1 - 4 = 2: `f` (sütun 5) ekranda 3. sütunda.
        assert_eq!(col, TEXT_COL + 3);
    }

    /// Düzenlemenin taşıdığı pencere kayması.
    fn shift_of(edits: &[DockEdit]) -> i32 {
        match only(edits) {
            DockEdit::Arrive { shift, .. } | DockEdit::Erase { shift, .. } => *shift,
            DockEdit::Shift { by } => *by,
            DockEdit::Reset => panic!("kayma bekleniyordu: Reset"),
        }
    }

    #[test]
    fn typing_at_the_end_of_an_overflowing_line_still_animates() {
        // Kullanıcı kararı (`plan.md` → R1.3): taşan satırın sonunda yazmak
        // pencereyi kaydırıyor ama harf yine canlanıyor; uçuştakiler metinle
        // birlikte bir sütun sola kayıyor.
        let cols = TEXT_COL + 4;
        let edits = edits_between(&at_end("abcd", 1), &at_end("abcde", 2), cols);
        // skip: eski 4 + 1 - 4 = 1, yeni 5 + 1 - 4 = 2. `e` caret'in solunda,
        // yani son metin sütununun bir solunda.
        assert_eq!(arrive(&edits), (TEXT_COL + 2, "e".into()));
        assert_eq!(shift_of(&edits), -1);
    }

    #[test]
    fn backspace_at_the_end_of_an_overflowing_line_still_animates() {
        // Silmek pencereyi geri kaydırıyor: metin sağa, hayalet de metnin
        // yeni yerinde — caret'in sütununda doğuyor.
        let cols = TEXT_COL + 4;
        let edits = edits_between(&at_end("abcde", 2), &at_end("abcd", 3), cols);
        assert_eq!(erase(&edits), (TEXT_COL + 3, "e".into()));
        assert_eq!(shift_of(&edits), 1);
    }

    #[test]
    fn a_caret_that_moves_the_window_shifts_without_settling() {
        // Metin aynı, caret satırın başına gitti ve pencere kaydı: uçuştakiler
        // bitmiyor, yalnız kayıyor (skip 5 → 0).
        let cols = TEXT_COL + 4;
        let edits = edits_between(&typed("abcdefgh", 8, 1), &typed("abcdefgh", 2, 2), cols);
        assert_eq!(edits, [DockEdit::Shift { by: 5 }]);
        // Kaymayan bir caret hareketi hiçbir şey basmıyor.
        let edits = edits_between(&typed("abcdefgh", 2, 1), &typed("abcdefgh", 1, 2), cols);
        assert!(edits.is_empty(), "{edits:?}");
    }

    #[test]
    fn a_line_owned_by_the_grid_does_not_animate() {
        // Caret ızgaradaysa satır da orada: efektin konusu dock'ta yazmak.
        let old = at_end("l", 1);
        let new = at_end("ls", 2);
        let change = change(&old, &new, COLS);
        let mut edits = Vec::new();
        render(
            &new,
            &DockContext::default(),
            None,
            &THEME,
            same(COLS),
            false,
            change.as_ref(),
            |_| (),
            |edit| edits.push(edit),
        );
        reset(&edits, "caret ızgarada");
    }
}
