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

use crate::cluster::{ClusterId, Clusters, Walk};
use crate::color::{self, LinearRgba, Theme};
use crate::session::{Cell, CellHalf, SelectKind, SelectionRun, UnderlineStyle, WORD_SEPARATORS};

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
    /// Caret'in giriş bloğundaki yeri — satır **ve** sütun (032): uzun satır
    /// sarılıyor, yani caret ikinci görsel satırda da durabiliyor. Satır
    /// dikey pencerenin içinde, dock-yerel (`0` = çizilen ilk giriş satırı).
    /// `None` → caret çizilmez (ZLE satır düzenlemiyor ya da ayna okunamadı).
    pub caret: Option<DockCaret>,
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
    ///
    /// `None` → işaret çizilmez: dikey pencere kaydı ve girişin **ilk**
    /// satırı ekranda değil (032). İşaret prompt'un yeri; bir devam satırının
    /// yanında durursa komut orada başlıyormuş gibi okunurdu.
    ///
    /// Fareyle seçimin koşuları bu tipte **değil**: görsel satır başına bir
    /// koşu ve sayısı satır sayısına bağlı, yani `Copy` bir alana sığmıyor —
    /// çağıranın tamponuna akıyorlar ([`crate::Session::dock`]'un
    /// `selection`'ı, [`crate::SelectionRuns`] emsali).
    pub sigil: Option<LinearRgba>,
}

/// Dock caret'inin yeri: giriş bloğunda **ekran** sütunu ve dikey pencerenin
/// içindeki satır (032).
///
/// İki sayı adlı alanlarda, yan yana iki `u16` değil — [`DockCols`]'un
/// gerekçesi: ters geçirilseler belirti yalnız sarılmış satırda görünürdü.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockCaret {
    pub col: u16,
    pub row: u16,
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
    /// Giriş bloğunun genişliği: ızgaranın sütun sayısı. Dock aynı sütunları
    /// kullanıyor ve taşan satır **sarılıyor** (032 Karar 3).
    pub grid: u16,
    /// Bağlam satırının bütçesi. Ayrı bir sayı, çünkü o satır **küçük
    /// puntoda** çiziliyor: aynı piksel şeridine daha çok harf sığıyor.
    /// Sayıyı çizen taraf veriyor (`bt_gpu`'nun `context_cols`'u), bu crate
    /// piksel görmüyor — değer bir **bütçe**, punto kararı değil. `grid` ile
    /// eşit geçilirse satır bugünkü gibi davranır.
    pub context: u16,
}

/// Dock'un giriş bloğuna ayrılabilecek yer: ızgaranın satırlarının hangi
/// payına kadar ve kaç sütunda sarılarak — [`crate::Session::frame`]'in
/// argümanı (032).
///
/// **Yerleşim kararı çizenin**, sayıları `bt-gpu` veriyor ([`DockCols`]'un
/// emsali): tavan bir tasarım oranı (`bt_gpu`'nun `DOCK_MAX_SHARE`'i) ve bu
/// crate piksel de pencere de görmüyor. **Oran geçiyor, satır sayısı değil**:
/// ızgaranın satır sayısının tek okuması `frame()`'in `Term` kilidinin altında
/// ([`crate::Cursor::rows`]) ve çizen taraf onun ikinci bir kopyasını
/// tutmuyor — bütçe o okumaya uygulanıyor. `frame()` çizilecek giriş satırı
/// sayısını ([`crate::Cursor::input_rows`]) bastırma kararıyla **aynı
/// okumada** bu bütçeyle kırpıyor ve dock'un çizimi sayıyı argüman alıyor,
/// ikinci kez türetmiyor (aynanın kendisinin iki tur arasında ilerlemesi ayrı,
/// bir karelik bilinen sınır: [`render_with`]).
///
/// İki sayı tek tipte ve adlı alanlarda, `DockCols` ile aynı gerekçe: yan yana
/// iki sayı sessizce ters geçirilebilirdi.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockBudget {
    /// Giriş satırlarının tavanı, ızgaranın satırlarının **oranı** olarak
    /// (`0.5` → yarısı); aşağı yuvarlanıyor ve `0` da en az bir satır demek
    /// (dock'un giriş satırı hiç kaybolmuyor).
    pub share: f32,
    /// Sarmanın genişliği, sütun: dock'un giriş bloğunun ızgarayla paylaştığı
    /// genişlik ([`DockCols::grid`]).
    pub cols: u16,
}

impl DockBudget {
    /// `grid_rows` satırlık bir ızgarada bu bütçeyle çizilecek giriş satırı
    /// sayısı, `needed` satır isteyen bir görüntü için: tavana kırpılmış ve
    /// **en az bir**.
    pub(crate) fn fit(self, needed: usize, grid_rows: u16) -> u16 {
        // audit: oran `[0, 1]`'de beklenir ama sınır dışı bir değer de panik
        // değil — `as` doyuruyor, `min` yine satır sayısında kesiyor.
        let cap = (f32::from(grid_rows) * self.share).floor() as u16;
        let needed = u16::try_from(needed).unwrap_or(u16::MAX);
        needed.min(cap).max(1)
    }
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

/// Tek giriş satırlı dock'ta bağlam satırının dock-yerel satır numarası —
/// bu modülün sınamalarının sayısı.
///
/// Üretimde sabit değil: bağlam satırı giriş bloğunun **altında**, yani
/// satırı giriş satırı sayısının ta kendisi ([`crate::Cursor::input_rows`],
/// [`render_with`]'in `input_rows`'u). `bt-gpu` aynı sayıdan bandın dibine
/// yerleştiriyor.
#[cfg(test)]
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
/// **Konum iki eksende** (032 phase-6): `(row, col)` dikey pencerenin satırı
/// ve ekran sütunu, hücreler de kendi `(row, col)`'larıyla — sarılan girişte
/// satırı dolduran harf alt satıra efektiyle geçiyor. Düzenlemenin
/// **arkasında** sarmayla yer değiştiren metin düzenlemeye girmiyor: yeni
/// konumunda animasyonsuz (uçuştakiler `bt-gpu`'da statik glyph'lerini
/// bulamayıp bitiyor). **`shift` ve `Shift` satır cinsinden**: dikey
/// pencerenin tepesi kayınca uçuştakiler metinle birlikte kayıyor (030'un
/// yatay penceresinin kaymasının dikey karşılığı; yatay pencere 032 Karar 3
/// ile emekli). Tepeyi karşılaştıran taraf [`crate::Session::dock`], çünkü
/// son **çizilen** tepe çizimin değil izin bilgisi ([`with_shift`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DockEdit {
    /// Glyph'ler geldi. `(row, col)` koşunun **ilk** hücresi (yeni
    /// pencerede); hücreler normal sink'e de gidiyor, hangisinin çizileceği
    /// boyamanın kararı.
    Arrive {
        row: u16,
        col: u16,
        cells: EditCells,
        shift: i32,
    },
    /// Glyph'ler gitti. `(row, col)` silinmenin yeri (caret, yeni pencerede)
    /// ve hayaletler **eski** düzenin konumlarında, eski satırın vurgusuyla
    /// çözülmüş.
    Erase {
        row: u16,
        col: u16,
        ghosts: EditCells,
        shift: i32,
    },
    /// Metin değişmedi ama dikey pencere `by` satır kaydı (caret tavanı
    /// aşan girişte satır değiştirdi ya da tekerlek): uçuştaki efektler yalnız
    /// kayıyor, hiçbiri bitmiyor.
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
/// **Ham, çünkü konum yok:** ekran sütunu düzenden çıkıyor ve onu [`render`]
/// kendi yürüyüşünde zaten hesaplıyor; ikinci bir kopyası burada doğmuyor.
/// Eski taraftan yalnız yeni tamponda artık olmayan şey taşınıyor — hayaletin
/// karakteri ve vurgusu — çünkü çağıran ([`crate::Session::dock`]) bu
/// hesaptan hemen sonra tamponu yeni aynayla eziyor.
// `Delete`'in hayalet listesi kod noktası kapasitesiyle büyük (~0.8 KB,
// [`GHOST_CHARS`]): değer karede bir kez ve yığında doğuyor, `Box` ise hem
// kare başına bir ayırma hem `Copy`'nin kaybı olurdu.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Change {
    /// Canlanmayan değişim.
    Reset,
    /// Ayna ilerledi ama `BUFFER` aynı (öneri değişti, caret kıpırdadı):
    /// uçuştaki efektler yerinde kalıyor.
    Same,
    /// Yeni görüntünün `start..end` karakterleri eklendi.
    Insert { start: usize, end: usize },
    /// Eski `BUFFER`'dan glyph'ler silindi; yenisinde yoklar.
    Delete { ghosts: Ghosts },
}

/// Silinen kod noktaları ve eski satırdaki vurguları.
///
/// **Bütün kod noktaları**, yalnız glyph'ler değil (035): hayaletlerin
/// düzeni kümeyi yeni düzendekiyle aynı kurabilsin — `🇹🇷`'nin hayaleti tek
/// glyph, `❤️`'nin VS16'sı taban karakterinin emoji sunumunu taşıyor.
/// Sıfır genişlikli kod noktası düzende hücre almıyor, yani kümeleme
/// kapalıyken hayaletlerin konumu bugünküyle aynı.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Ghosts {
    len: usize,
    chars: [(char, HighlightStyle); GHOST_CHARS],
}

/// [`Ghosts`]'un kod noktası kapasitesi: [`EDIT_MAX`] glyph'in her biri
/// birkaç kod noktalı bir küme olabiliyor (`👍🏽` iki, `❤️` iki, aile beş).
/// Aşan silme [`Change::Reset`]'e düşüyor — [`EDIT_MAX`]'ın kuralı.
const GHOST_CHARS: usize = EDIT_MAX * 4;

/// `index`'i içeren kümenin aralığı, `[start, end)` — dock'un seçim uçları,
/// ⇧←/⇧→ adımı ve dört düzenleme tuşunun (035 Karar 7) kümeyi bölmemesi.
/// Kümeleme kapalıyken tek kod noktası; `index` metnin dışındaysa `None`.
///
/// Tek küme kuralı ([`Walk`]): düzen, ızgara ve tazelik kapısıyla aynı.
pub(crate) fn cluster_span(
    chars: impl IntoIterator<Item = char>,
    index: usize,
    cluster: bool,
) -> Option<(usize, usize)> {
    if !cluster {
        return chars.into_iter().nth(index).map(|_| (index, index + 1));
    }
    let mut found = None;
    Walk::new().run(chars, |span| {
        if (span.start..span.end).contains(&index) {
            found = Some((span.start, span.end));
        }
    });
    found
}

/// `index` `text`'te bir küme sınırı mı (035 R4.2): düzenleme bir kümenin
/// içinden başlıyor ya da bitiyorsa (`🇹🇷`'nin yalnız `🇷`'si silindi, `👍`'e
/// ten rengi eklendi) canlanan şey yarım bir glyph olurdu ve fark
/// [`Change::Reset`]'e düşüyor — metin anında belirir. Tek küme kuralı
/// ([`Walk`]); sona eşit indeks sınır.
fn is_cluster_boundary(text: &str, index: usize) -> bool {
    let mut boundary = index == 0;
    let mut len = 0;
    Walk::new().run(text.chars(), |cluster| {
        boundary |= cluster.start == index;
        len = cluster.end;
    });
    boundary || index >= len
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
pub(crate) fn change(old: &DockState, new: &DockState) -> Option<Change> {
    (old.answers != new.answers || old.status != new.status).then(|| diff(old, new))
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
/// tuşun tabanı o. `PREDISPLAY` ya da `PREBUFFER` değiştiyse metin
/// kaymıştır, `Reset`.
pub(crate) fn diff(old: &DockState, new: &DockState) -> Change {
    let old_buffer = match old.status {
        DockStatus::Live => {
            // `PREBUFFER` değiştiyse ZLE bir satırı kabul etti ya da bıraktı:
            // `BUFFER`'ın satırı kaydı, düzenleme yazım değil.
            if old.predisplay != new.predisplay || old.prebuffer != new.prebuffer {
                return Change::Reset;
            }
            old.buffer.as_str()
        }
        // Ekranda hiçbir şey yok.
        DockStatus::Idle => "",
        _ => return Change::Reset,
    };
    if new.status != DockStatus::Live {
        return Change::Reset;
    }
    if old_buffer == new.buffer {
        return Change::Same;
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
    // Glyph sayısı: kümeleme açıkken **küme** (035) — `🇹🇷` tek girdi ve tek
    // glyph, iki RI değil. Aralığın iki ucu aşağıda küme sınırı diye
    // sınanıyor, yani aralığı tek başına kümelemek bağlamındakiyle aynı.
    let glyphs = |run: &mut dyn Iterator<Item = char>| {
        if new.cluster {
            let mut count = 0;
            Walk::new().run(run, |cluster| count += usize::from(cluster.width > 0));
            count
        } else {
            run.filter(|&ch| column_width(ch) > 0).count()
        }
    };
    let bounds = |text: &str, at: &[usize]| {
        !new.cluster || at.iter().all(|&index| is_cluster_boundary(text, index))
    };
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
        // Eklemenin iki ucu yeni metinde, birleşme noktası eski metinde küme
        // sınırı olmalı: `👍`'e eklenen ten rengi yeni bir glyph değil.
        if !bounds(&new.buffer, &[start, caret]) || !bounds(old_buffer, &[start]) {
            return Change::Reset;
        }
        if !fits(glyphs(
            &mut new.buffer.chars().skip(start).take(caret - start),
        )) {
            return Change::Reset;
        }
        Change::Insert {
            start: pre + start,
            end: pre + caret,
        }
    } else {
        // Silme: `new == old[..caret] ++ old[caret + k..]`. `Idle` tabanda
        // eski satır boş, yani buraya eşit uzunlukta bir değiştirme düşüyor
        // ve `k = 0` onu aşağıdaki `fits`'te eliyor.
        let count = old_len - new_len;
        // Silinen satır sonu hayaletlerin düzenini kırar: hayalet listesi
        // yalnız glyph taşıyor ve `\n`'in arkasındakiler aynı satıra dizilirdi.
        if old_buffer
            .chars()
            .skip(caret)
            .take(count)
            .any(|ch| ch == '\n')
        {
            return Change::Reset;
        }
        let rest = old_buffer
            .chars()
            .take(caret)
            .chain(old_buffer.chars().skip(caret + count));
        if count == 0 || !new.buffer.chars().eq(rest) {
            return Change::Reset;
        }
        // Silinen aralığın iki ucu eski metinde, birleşme noktası yeni
        // metinde küme sınırı olmalı: `🇹🇷`'nin yarısı silinmiş bir glyph
        // değil, `🇹x🇷`'den `x`'in silinmesi iki yarıyı bayrağa birleştiriyor.
        if !bounds(old_buffer, &[caret, caret + count]) || !bounds(&new.buffer, &[caret]) {
            return Change::Reset;
        }
        let mut ghosts = Ghosts {
            len: 0,
            chars: [(' ', HighlightStyle::default()); GHOST_CHARS],
        };
        let run = old_buffer.chars().enumerate().skip(caret).take(count);
        for (index, ch) in run {
            let Some(slot) = ghosts.chars.get_mut(ghosts.len) else {
                return Change::Reset;
            };
            // Vurgu **eski** görüntüden: yenisinde bu karakter yok.
            *slot = (ch, style_at(old, pre + index));
            ghosts.len += 1;
        }
        if !fits(glyphs(&mut ghosts.as_slice().iter().map(|&(ch, _)| ch))) {
            return Change::Reset;
        }
        Change::Delete { ghosts }
    }
}

/// Aynanın **görüntüsü**, karakter karakter: `PREDISPLAY ++ BUFFER ++
/// POSTDISPLAY` — [`DockState::cursor`]'ın ve `region_highlight`'ın uzayı.
fn display(state: &DockState) -> impl Iterator<Item = char> + '_ {
    state
        .predisplay
        .chars()
        .chain(state.buffer.chars())
        .chain(state.postdisplay.chars())
}

/// Dock'un **akışı**: `PREBUFFER ++ PREDISPLAY ++ BUFFER ++ POSTDISPLAY`
/// (032 Karar 2). `PREBUFFER` ZLE'nin kabul ettiği önceki satırlar ve her
/// zaman `\n`'le bitiyor, yani düzenlenebilir satırlar kendiliğinden bir alt
/// satırdan ve aynı girintiden başlıyor. Akışın indeksi görüntününkinden
/// [`prebuffer_chars`] kadar ileride — `CURSOR` ve `region_highlight` o
/// kaydırmayla okunuyor.
fn stream(state: &DockState) -> impl Iterator<Item = char> + '_ {
    state.prebuffer.chars().chain(display(state))
}

/// `PREBUFFER`'ın karakter sayısı: akış ile görüntü uzayı arasındaki
/// kaydırma, ve **seçilebilir metnin** (`PREBUFFER ++ BUFFER`, [`selectable`])
/// `BUFFER`'dan önceki kısmı.
pub(crate) fn prebuffer_chars(state: &DockState) -> usize {
    state.prebuffer.chars().count()
}

/// Dock'ta **seçilebilen** metin: `PREBUFFER ++ BUFFER` (032 Karar 2) —
/// [`DockPoint`]'in ve seçim aralığının uzayı. `PREBUFFER` seçilip
/// kopyalanabiliyor (bütün döngüyü kopyalamak beklenen şey) ama ZLE onu
/// düzenleyemiyor: ona değen aralık düzenleme komutu doğurmuyor
/// (`Session::dock_edit_line`). `PREBUFFER` boşken `BUFFER`'ın kendisi,
/// ayırma yok.
pub(crate) fn selectable(state: &DockState) -> std::borrow::Cow<'_, str> {
    if state.prebuffer.is_empty() {
        std::borrow::Cow::Borrowed(&state.buffer)
    } else {
        std::borrow::Cow::Owned(format!("{}{}", state.prebuffer, state.buffer))
    }
}

/// [`layout`]'un **dock** parametrizasyonu (032 Karar 3 ve 7): ilk satır da
/// devam satırları da metnin sütunundan ([`TEXT_COL`], asma girinti), genişlik
/// ızgaranınki. Sütunlar doğrudan **ekran** sütunu — işaretin ve nefes payının
/// iki sütunu satırın içinde sayılıyor.
///
/// Tüketicileri dört ve kopyası yok: çizim ([`render_with`]), fareyle isabet
/// ([`hit`]), `frame()`'in satır sayısı ([`needed_rows`]) ve silmenin
/// hayaletleri (aynı yürüyüşün caret'ten başlayan hâli). İsabet testi kendi
/// yürüyüşünü yazsaydı sarma ya da geniş karakter kuralı ikisinde ayrıştığı
/// gün fare bir sütun — artık bir satır da — kayar ve belirti sessiz olurdu
/// (024'ün "tek tablo" gerekçesinin yürüyüşteki karşılığı).
pub(crate) fn dock_layout<T>(
    items: impl IntoIterator<Item = (char, T)>,
    caret: usize,
    cols: u16,
    cluster: bool,
    line: impl FnMut(VisualLine),
    place: impl FnMut(Placed<T>),
) -> LayoutEnd {
    let text = usize::from(TEXT_COL);
    layout_with(
        items,
        caret,
        usize::from(cols),
        text,
        text,
        cluster,
        line,
        place,
    )
}

/// Görüntünün dock'ta istediği giriş satırı sayısı — sarılmış hâliyle,
/// tavansız. `Live` olmayan ayna ve metnin sığmadığı genişlik tek satır.
///
/// [`crate::Session::frame`] bunu bastırma kararıyla **aynı kilit turunda**
/// soruyor ve bütçeyle ([`DockBudget::fit`]) kırpıp sınırdan veriyor; dock'un
/// çizimi aynı sayıyı argüman olarak alıyor, ikinci kez türetmiyor.
///
/// **Öneri (`POSTDISPLAY`) bandı büyütmüyor**: satırlar metnin
/// (`PREDISPLAY` ile `BUFFER`) ve caret'in kapladığı yere kadar sayılıyor. Autosuggestions'ın
/// önerisi her tuşta toptan değişiyor ve boyu dalgalanıyor (`git ` →
/// `git status --short`); sayılsaydı sarma sınırında bant her tuşta büyüyüp
/// küçülür, yazarken bütün ızgara nefes alırdı. Öneri yürüyüşte **kalıyor**
/// (düzen tek, sütunlar aynı) ve metnin satırlarına sığanı çiziliyor, taşanı
/// kırpılıyor — 030'un tek satırlık dock'unda sağ kenarda kesildiği gibi.
///
/// Yürüyüş çizimin yürüyüşünün ta kendisi, önerisiz bir akış değil: öneri
/// caret'in satırını değiştirebiliyor (satır sonuna sığmayan ilk geniş öneri
/// harfi caret'i alt satıra indiriyor) ve iki ayrı akış bandı dikey
/// pencereden bir satır ayırırdı.
pub(crate) fn needed_rows(state: &DockState, cols: u16) -> usize {
    if state.status != DockStatus::Live || cols <= TEXT_COL {
        return 1;
    }
    measure(state, cols).1
}

/// [`needed_rows`]'un yürüyüşü: caret'in satırı ve metnin (öneri hariç)
/// kapladığı satır sayısı. Çizim de aynı ölçüyü okuyor ([`render_with`]:
/// dikey pencerenin tepesi ve "tek satır mı" kapısı), yani bant, pencere ve
/// efektlerin kapısı aynı sayıya bakıyor — ayrı ölçülerde öneriyle sarılan
/// tek satırlık giriş her tuşta efektlerini sıfırlıyordu (`/code-review`).
fn measure(state: &DockState, cols: u16) -> (usize, usize) {
    let shift = prebuffer_chars(state);
    let text = shift + state.predisplay.chars().count() + state.buffer.chars().count();
    // Metnin son satırı: metinden başlayan her satır, artı metnin sonundaki
    // satır sonunun açtığı boş satır (sarmanın açtığı satır değil — o öneriye
    // ait). Satır sonu satırı `end + 1`'den, sarma `end`'den açıyor.
    let mut lines = 0;
    let mut last = 0;
    let mut previous_end = None;
    let end = dock_layout(
        stream(state).map(|ch| (ch, ())),
        shift + state.cursor,
        cols,
        state.cluster,
        |line| {
            let after_newline = previous_end.is_some_and(|end: usize| end + 1 == line.start);
            if lines == 0 || line.start < text || (line.start == text && after_newline) {
                last = lines;
            }
            previous_end = Some(line.end);
            lines += 1;
        },
        |_| {},
    );
    (end.caret_row, end.caret_row.max(last) + 1)
}

/// Dikey pencerenin ilk satırı: caret'in satırı görünür kalacak **en küçük**
/// kayma (032 Karar 4). Durumsuz — 030'un yatay `window_skip`'inin dikey
/// ikizi: pencere caret'i izliyor, kendi geçmişini tutmuyor. `shown`
/// çizilecek giriş satırı sayısı; `0` da bir satır.
fn window_top(caret_row: usize, shown: usize) -> usize {
    caret_row.saturating_sub(shown.max(1) - 1)
}

/// Akışın bir karakteri hangi dizgiden: yalnız `PREBUFFER ++ BUFFER`
/// seçilebiliyor (031 Karar 8, 032 Karar 2), `PREDISPLAY` ile öneri isabet
/// testinde `BUFFER`'ın iki ucuna iniyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Pre,
    /// Seçilebilir metnin ([`selectable`]) bu karakter indeksi — `PREBUFFER`
    /// ya da `BUFFER`.
    Buffer(usize),
    Post,
}

/// Dock'un giriş bloğunda bir nokta: seçilebilir metnin ([`selectable`],
/// `PREBUFFER ++ BUFFER`; `PREBUFFER` boşken `BUFFER`'ın kendisi) karakter
/// indeksi ve
/// karakterin hangi yarısı — ızgaranın [`crate::SelectionPoint`]'inin
/// **metin uzayındaki** karşılığı (alacritty'nin `Anchor`'ı: nokta + yan).
/// Satır yok: sarma bir görüntü kararı, indeks hangi görsel satırda olursa
/// olsun aynı karakteri gösteriyor.
///
/// `index ≥ metnin uzunluğu` geçerli ve anlamı "satırın sonundaki boşluk",
/// ızgarada satırın sağındaki boş hücreler gibi: metnin hemen sağındaki
/// sütun `len`, bir ötesi `len + 1`… Öneriye ya da boşluğa yapılan tıklama
/// oraya iniyor; sınır (`Simple`) `len`'e kırpılıyor, kelime (`Word`) ise
/// ızgaradaki gibi yalnız bitişik sütunda son kelimeyi alıyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DockPoint {
    pub(crate) index: usize,
    pub(crate) half: CellHalf,
}

/// Fare isabet testi: dock'un giriş bloğunda `row`. satırın (dikey
/// pencerenin içinde) `col` sütununun `half` yarısı → seçilebilir metinde
/// ([`selectable`]) bir nokta.
/// Ayna `Live` değilse ya da metin sığmıyorsa `None` (seçilecek metin yok).
///
/// **Yürüyüş [`render_with`]'inkinin ta kendisi** ([`dock_layout`]): aynı
/// akış, aynı genişlik, aynı sarma — yani nokta çizildiği yere düşüyor,
/// geniş karakter ve satır sonu dahil. `top` çağırandan geliyor, çünkü
/// sorulan şey **ekrandaki** pencere ([`crate::Session::dock`]'un bıraktığı
/// iz), canlı aynanın bugün hesaplayacağı pencere değil.
///
/// Kurallar:
/// - `BUFFER` karakterinin içinde yarı **glyph'in** yarısı: geniş karakterin
///   sol sütunu sol yarı, sağ sütunu (spacer) sağ yarı — hücre değil glyph.
/// - `PREDISPLAY` `BUFFER`'ın başına, öneri (`POSTDISPLAY`) `BUFFER`'ın
///   sonuna iner: ikisi de seçilemiyor ama tıklamanın gideceği yer belli.
/// - `PREBUFFER` (032) seçilebilir metnin başı: kendi karakterine iner. Caret
///   oraya taşınamıyor — o karar düzenleme kapısında, burada değil.
/// - Satırın solundaki sütun (işaret, nefes payı, asma girinti) o satırın ilk
///   çizilen karakterinin sol yarısı.
/// - Satırın sağındaki boşluk: `BUFFER` alt satırda sürüyorsa (sarma) o
///   satırın son karakterinin sağ yarısı — ızgaranın sarılmış satırındaki
///   kural; satır `BUFFER`'ın sonuysa `BUFFER`'ın sonu ve ötesi.
pub(crate) fn hit(
    state: &DockState,
    top: usize,
    cols: u16,
    row: u16,
    col: u16,
    half: CellHalf,
) -> Option<DockPoint> {
    if state.status != DockStatus::Live || cols <= TEXT_COL {
        return None;
    }
    let shift = prebuffer_chars(state);
    let buffer = state.buffer.chars().count();
    // Seçilebilir metnin uzunluğu: boşluk ve öneri onun ötesine iniyor.
    let len = shift + buffer;
    let pre = state.predisplay.chars().count();
    let part = |index: usize| match index.checked_sub(shift) {
        None => Part::Buffer(index),
        Some(shown) => match shown.checked_sub(pre) {
            None => Part::Pre,
            Some(offset) if offset < buffer => Part::Buffer(shift + offset),
            Some(_) => Part::Post,
        },
    };
    // Boşluk ve öneri, metnin (`PREDISPLAY` + `BUFFER`) çizilen son
    // sütununun bir sağından uzaklığıyla `len`'in ötesine iniyor.
    let blank = |text_end: u16| DockPoint {
        index: len + usize::from(col.saturating_sub(text_end)),
        half: CellHalf::Left,
    };
    let at = |part: Part, half: CellHalf, text_end: u16| match part {
        Part::Pre => DockPoint {
            index: shift,
            half: CellHalf::Left,
        },
        Part::Buffer(index) => DockPoint { index, half },
        Part::Post => blank(text_end),
    };
    let target = top + usize::from(row);
    let mut lines = 0;
    let mut target_line = None;
    let mut found = None;
    let mut last = None;
    let mut text_end = TEXT_COL;
    dock_layout(
        stream(state).map(|ch| (ch, ())),
        shift + state.cursor,
        cols,
        state.cluster,
        |line| {
            if lines == target {
                target_line = Some(line);
            }
            lines += 1;
        },
        |placed| {
            if found.is_some() || placed.row != target || !placed.fits(cols) {
                return;
            }
            let part = part(placed.index);
            // audit: `fits` → `col + width ≤ cols` ve `cols` `u16`.
            let (start, end) = (placed.col as u16, (placed.col + placed.width) as u16);
            if col < start {
                // Satırın solu: ilk çizilen karakterin sol yarısı. Sütunlar
                // satırın içinde bitişik, yani buraya yalnız ilk karakterde
                // düşülebilir.
                found = Some(at(part, CellHalf::Left, text_end));
            } else if col < end {
                // Glyph'in yarısı yarım sütun cinsinden: `2 · width` yarım
                // sütun ve ilk `width`'i sol yarı.
                let halves = usize::from(col - start) * 2 + usize::from(half == CellHalf::Right);
                let side = if halves < placed.width {
                    CellHalf::Left
                } else {
                    CellHalf::Right
                };
                found = Some(at(part, side, text_end));
            } else {
                if part != Part::Post {
                    text_end = end;
                }
                // Kümenin kod noktası sayısıyla (035): sarma sorusu kümenin
                // **arkasına** bakıyor, baş karakterin değil.
                last = Some((part, placed.end - placed.index));
            }
        },
    );
    if found.is_some() {
        return found;
    }
    Some(match last {
        // `BUFFER` bu satırdan sonra sürüyor (sarma): son çizilenin sağ yarısı.
        // Son çizilen bir kümeyse sorulan şey kümenin arkası (`span`): `🇹🇷`
        // ile biten `BUFFER`'da `🇷` sürüyor sayılmamalı.
        Some((Part::Buffer(index), span)) if index + span < len => {
            at(Part::Buffer(index), CellHalf::Right, text_end)
        }
        Some((Part::Pre, _)) if buffer > 0 => at(Part::Buffer(shift), CellHalf::Left, text_end),
        // Çizilen karakteri olmayan satır (satır sonunun açtığı boş satır ya
        // da pencerenin ötesi): satırın başladığı yer.
        None => match target_line.map(|line| part(line.start)) {
            Some(Part::Buffer(index)) => DockPoint {
                index,
                half: CellHalf::Left,
            },
            Some(Part::Pre) => at(Part::Pre, CellHalf::Left, TEXT_COL),
            _ => blank(TEXT_COL),
        },
        _ => blank(text_end),
    })
}

/// Dock seçiminin `BUFFER`'daki karakter aralığı, `[start, end)` — iki uç
/// ve adımdan. Boş seçimde `start == end`.
///
/// **Davranışın sahibi alacritty** ve bu fonksiyon onun metin uzayındaki
/// kopyası (031 Karar 5): `Simple` uçların yarısından sınır çizer
/// (`range_simple`), `Word` iki ucu kelime sınırına genişletir
/// (`range_semantic` — [`WORD_SEPARATORS`], parantez eşleme ve ayırıcının
/// üstüne çift tıklama kuralı dahil), `Line` **mantıksal satırı** (032
/// Karar 6): iki ucun satırları, `\n`'ler arası ve sarılmış görsel
/// satırlarıyla birlikte — ızgaranın üçlü tıklaması da sarılmış mantıksal
/// satırı seçiyor, macOS metin alanlarının paragraf seçimi de. Satır sonu
/// aralığa girmiyor. Tek mantıksal satırda (`\n`'siz `BUFFER`) sonuç bütün
/// `BUFFER`, yani 031'in cevabı; bütün `BUFFER` ⌘A'nın işi olarak kalıyor.
/// Izgarada aynı dizgi aynı aralığı veriyor; bekçisi
/// `a_dock_word_matches_the_grid_word` (`session.rs`).
///
/// Kare yolunda **koşmuyor**: aralık seçim değiştiğinde bir kez çözülüp
/// seçimin yanında saklanıyor ([`crate::shell::DockSelection`]).
///
/// `cluster` aynanın kümeleme bayrağı ([`DockState::cluster`]): uçlar küme
/// sınırına iniyor ([`boundary`]). Kelime ve satır adımı zaten sınırda —
/// ayırıcılar ve `\n` hiçbir kümenin içinde değil.
pub(crate) fn selection_range(
    buffer: &str,
    kind: SelectKind,
    anchor: DockPoint,
    head: DockPoint,
    cluster: bool,
) -> (usize, usize) {
    let chars: Vec<char> = buffer.chars().collect();
    let len = chars.len();
    match kind {
        SelectKind::Line => {
            let (low, high) = (
                anchor.index.min(head.index).min(len),
                anchor.index.max(head.index).min(len),
            );
            let start = (0..low)
                .rev()
                .find(|&index| chars[index] == '\n')
                .map_or(0, |index| index + 1);
            let end = (high..len)
                .find(|&index| chars[index] == '\n')
                .unwrap_or(len);
            (start, end.max(start))
        }
        SelectKind::Simple => {
            let (a, h) = (
                boundary(&chars, anchor, cluster),
                boundary(&chars, head, cluster),
            );
            (a.min(h), a.max(h))
        }
        SelectKind::Word => {
            let (start, end) = if anchor.index <= head.index {
                (anchor.index, head.index)
            } else {
                (head.index, anchor.index)
            };
            // Parantez yalnız **noktasal** seçimde (çift tık, sürüklemesiz):
            // alacritty'nin kuralı; sürüklenen kelime seçimi eşleşmeyi aramaz.
            if start == end
                && let Some(matching) = bracket_match(&chars, start)
            {
                return (start.min(matching), start.max(matching) + 1);
            }
            let start = word_start(&chars, start).min(len);
            let end = (word_end(&chars, end) + 1).min(len);
            (start.min(end), end)
        }
    }
}

/// Noktanın sınırı: sol yarı karakterin önü, sağ yarı arkası — arkasındaki
/// sıfır genişlikliler (birleştiriciler) karakterle birlikte kalıyor, yoksa
/// `é` harfi aksanından ayrılırdı.
///
/// **Kümeleme açıkken birim küme** (035 R4.2): [`hit`] kümenin baş
/// karakterini veriyor ve sağ yarı kümenin **sonuna** iniyor — `🇹🇷`'nin sağ
/// yarısına tık iki RI'nin arasına değil bayrağın arkasına düşüyor. Sol yarı
/// da kümenin başına: uç hiçbir yoldan bir kümenin içinde kalmıyor.
fn boundary(chars: &[char], point: DockPoint, cluster: bool) -> usize {
    let len = chars.len();
    if cluster && point.index < len {
        let (start, end) = cluster_span(chars.iter().copied(), point.index, true)
            .unwrap_or((point.index, point.index + 1));
        return if point.half == CellHalf::Left {
            start
        } else {
            end
        };
    }
    if point.half == CellHalf::Left || point.index >= len {
        return point.index.min(len);
    }
    let mut next = point.index + 1;
    while chars.get(next).is_some_and(|&ch| column_width(ch) == 0) {
        next += 1;
    }
    next
}

/// `index`'teki karakter; `BUFFER`'ın sonundan ötesi **boşluk**, yani
/// ayırıcı — ızgarada satırın sağındaki boş hücrelerin karşılığı.
fn char_at(chars: &[char], index: usize) -> char {
    chars.get(index).copied().unwrap_or(' ')
}

fn is_separator(ch: char) -> bool {
    WORD_SEPARATORS.contains(ch)
}

/// Kelimenin başı: `point`'in **solunda** ilk ayırıcının bir sağı
/// (`semantic_search_left`). Noktanın kendisine bakılmıyor — ayırıcının
/// üstüne çift tıklamanın iki yandaki kelimeleri alması buradan.
fn word_start(chars: &[char], point: usize) -> usize {
    (0..point)
        .rev()
        .find(|&index| is_separator(char_at(chars, index)))
        .map_or(0, |index| index + 1)
}

/// Kelimenin sonu (dahil): `point`'in **sağında** ilk ayırıcının bir solu
/// (`semantic_search_right`). `BUFFER`'ın sonundan ötesi ayırıcı, yani arama
/// en geç orada biter.
fn word_end(chars: &[char], point: usize) -> usize {
    (point + 1..)
        .find(|&index| is_separator(char_at(chars, index)))
        .map_or(point, |index| index - 1)
}

/// `index`'teki parantezin eşi — alacritty'nin `bracket_search`'ü: aynı
/// türden her parantez bir eşi atlatıyor. Parantez değilse ya da eşi yoksa
/// `None`.
fn bracket_match(chars: &[char], index: usize) -> Option<usize> {
    const PAIRS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];
    let start = *chars.get(index)?;
    let (forward, end) = PAIRS.iter().find_map(|&(open, close)| {
        if open == start {
            Some((true, close))
        } else if close == start {
            Some((false, open))
        } else {
            None
        }
    })?;
    let mut depth = 0usize;
    let mut probe = |candidate: usize| {
        let ch = chars[candidate];
        if ch == end {
            if depth == 0 {
                return true;
            }
            depth -= 1;
        } else if ch == start {
            depth += 1;
        }
        false
    };
    if forward {
        (index + 1..chars.len()).find(|&candidate| probe(candidate))
    } else {
        (0..index).rev().find(|&candidate| probe(candidate))
    }
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
/// **sarılıyor** (032 Karar 3; 030'un soldan pencerelemesi emekli) — devam
/// satırları metnin sütunundan, geniş glyph satır sonunda **yarılanmıyor**,
/// sığmıyorsa alt satıra geçiyor ([`dock_layout`]). Sarılan giriş
/// `input_rows` satırı ([`crate::Cursor::input_rows`], tavana kırpılmış)
/// aşarsa **dikey pencere** caret'in satırını görünür tutuyor
/// ([`window_top`]); pencerenin dışındaki satırlar sink'e hiç uğramıyor —
/// yoksa bağlam satırının üstüne düşerlerdi.
///
/// `context_cols` bağlam satırının bütçesi ve ayrı bir sayı, çünkü o satır
/// **küçük puntoda** çiziliyor: aynı genişliğe daha çok harf sığıyor. Sayıyı
/// çizen taraf veriyor (`bt-gpu`), bu crate piksel görmüyor — `cols`'un
/// kendisiyle aynı sözleşme. İkisi eşit geçilirse satır bugünkü gibi davranır,
/// yani değer bir **bütçe**dir, punto kararı değil.
///
/// `change` son çizilen aynadan bu yana ne değiştiği ([`change`]'in cevabı);
/// [`DockEdit`]'e burada, **bu düzenin** sütunlarıyla çevrilip `edits`'e
/// basılıyor — karede en çok bir kez. Canlanma yalnız metnin çizildiği ve
/// caret'in dock'ta olduğu kolda: satır ızgaradaysa efektin konusu yok ve her
/// canlanmayan kol uçuştakileri bitirir (`Reset`). Konum **(satır, sütun)**
/// ve dikey pencerenin satırı (032 phase-6); pencere tepesinin kayması
/// (`shift`) burada değil çağıranda ([`with_shift`]).
///
/// `selection` dock seçiminin `BUFFER`'daki karakter aralığı (031,
/// [`crate::shell::DockSelection::range`]); `runs`'a **görsel satır başına**
/// bir koşu olarak, dikey pencerenin satır ve ekran sütunlarıyla çevriliyor —
/// ızgaranın [`crate::SelectionRun`]'ının aynısı ve aynı kuralla (031
/// Karar 4): koşu satırın ilk çizilir seçili hücresinden sonuncusuna,
/// aradaki boşluklar köprülü. `runs` baştan boşaltılıyor.
///
/// Dönüşün ikinci yarısı dikey pencerenin ilk satırı ([`window_top`]) ve
/// girişin tavansız satır sayısı: isabet testinin ve tekerleğin izi
/// ([`crate::Session::dock`] yazıyor).
///
/// `scroll` kullanıcının tekerlekle seçtiği pencere tepesi
/// ([`crate::shell::ShellLog::dock_scroll`]); `None` → pencere caret'i
/// izliyor. Seçilen tepe satır sayısına kırpılıyor; caret pencerenin
/// dışında kalırsa çizilmiyor (bir metin alanının caret'ten uzağa
/// kaydırılmış hâli) — yazmak ya da caret'i oynatmak izlemeyi geri getiriyor.
///
/// **Yürüyüş iki kez koşuyor** ve ikisi aynı fonksiyon ([`dock_layout`]):
/// pencerenin tepesi caret'in satırına bağlı ve o satır ancak yürüyüşün
/// sonunda belli ([`measure`]), hücreler ise tepeyi bilmeden basılamıyor.
/// İkinci bir sayı **üreticisi** değil, aynı yürüyüşün iki okuması.
///
/// **Bilinen sınır, bir kare:** `input_rows` `frame()`'in aynasından, çizim
/// bu çağrının aynasından; iki kilit turunun arasına sarma sınırını geçen bir
/// tuşun aynası düşerse o kare pencere bir satır kayık çizilir (tavanı aşan
/// girişin kuralı) ve bir sonraki kare düzeltir — `line-finish`'in aynı
/// aralıktaki bilinen sınırının kardeşi (`Session::frame`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_with(
    state: &DockState,
    context: &DockContext,
    shell: Option<ShellState>,
    theme: &Theme,
    cols: DockCols,
    input_rows: u16,
    scroll: Option<usize>,
    owned: bool,
    selection: Option<(usize, usize)>,
    change: Option<&Change>,
    runs: &mut Vec<SelectionRun>,
    clusters: &mut Clusters,
    mut sink: impl FnMut(Cell),
    mut edits: impl FnMut(DockEdit),
) -> (Dock, usize, usize) {
    runs.clear();
    let mut surface = Dock {
        ground: theme.background_linear(),
        separator: theme.separator_linear(),
        caret: None,
        caret_text: theme.background_linear(),
        sigil: Some(sigil_color(shell, theme)),
    };
    if cols.grid == 0 {
        settle(change, &mut edits);
        return (surface, 0, 1);
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
    render_context(context, theme, cols.context, input_rows, &mut sink);

    if cols.grid <= TEXT_COL {
        settle(change, &mut edits);
        return (surface, 0, 1);
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
            surface.caret = Some(DockCaret {
                col: TEXT_COL,
                row: 0,
            });
        }
        settle(change, &mut edits);
        return (surface, 0, 1);
    }
    let fixed = theme.foreground_linear();
    // Öneri sönük: "henüz yazılmamış metin" ile SGR 2'nin sorduğu şey aynı.
    let suggestion = theme.dim_linear();
    // **`PREBUFFER` akışın başında** (032 Karar 2, [`stream`]): ZLE'nin kabul
    // ettiği önceki satırlar düzenlenebilir satırların üstünde, aynı renkte
    // ve aynı girintide — dock bir editör, `for` döngüsü tek parça metin.
    // Görüntünün indeksleri (`CURSOR`, `region_highlight`, efektler) akışta
    // `shift` kadar ileride.
    let shift = prebuffer_chars(state);
    let pre = state.predisplay.chars().count();
    let buffer = state.buffer.chars().count();
    let text = shift + pre + buffer;
    let stream = stream(state)
        .enumerate()
        .map(|(index, ch)| (ch, if index < text { fixed } else { suggestion }));

    // **Caret'in yeri düzenden**: `CURSOR` karakter indeksi (ZLE'nin birimi),
    // görüntünün birimi ise (satır, sütun) — geniş bir karakter indeksi bir,
    // sütunu iki ilerletiyor, sarma da satırı. Tam dolan satırın ardındaki
    // caret alt satırın başında ([`layout`]'un caret kuralı): zsh'in
    // ızgarasıyla aynı, yani tam genişlikte yazılan satır dock'u da bir satır
    // büyütüyor (032 phase-1 → Uygulama Notları).
    let (caret_row, rows) = measure(state, cols.grid);
    let shown = usize::from(input_rows.max(1));
    let top = scroll.map_or_else(
        || window_top(caret_row, shown),
        |top| top.min(rows.saturating_sub(shown)),
    );
    let window = top..top + shown;
    // İlk satır pencerenin dışındaysa işaret de: prompt'un yeri ekranda değil.
    if top > 0 {
        surface.sigil = None;
    }
    let arriving = match change {
        Some(&Change::Insert { start, end }) if owned => shift + start..shift + end,
        _ => 0..0,
    };
    let mut arrive_at = None;
    let mut arrived = EditCells::empty();
    // Seçim seçilebilir metnin ([`selectable`]) uzayında; akışta `PREBUFFER`
    // aynı yerde, `BUFFER` ise `PREDISPLAY` kadar ileride. Akış indeksi
    // seçilebilir indekse çevriliyor, `PREDISPLAY` ile öneri hiçbir indekse
    // düşmüyor.
    let selected = selection.map_or(0..0, |(start, end)| start..end);
    let selectable_at = |index: usize| {
        if index < shift {
            Some(index)
        } else {
            let offset = (index - shift).checked_sub(pre)?;
            (offset < buffer).then_some(shift + offset)
        }
    };
    let mut run: Option<SelectionRun> = None;

    let end = dock_layout(
        stream,
        shift + state.cursor,
        cols.grid,
        state.cluster,
        |_| {},
        |placed| {
            // Pencerenin dışındaki satır (dikey pencere) ve sığmayan dejenere
            // karakter (bir sütunluk pencerede geniş glyph, [`layout`]'un "taşar"
            // kuralı) çizilmiyor: biri bağlam satırının, öteki ızgaranın dışına
            // düşerdi.
            if !window.contains(&placed.row) || !placed.fits(cols.grid) {
                return;
            }
            let Placed {
                index,
                end,
                ch,
                width,
                row,
                col,
                tag: base,
            } = placed;
            // audit: `row - top < shown ≤ input_rows` ve `col + width ≤ cols`;
            // ikisi de `u16`'dan geliyor, taşamaz.
            let (row, col) = ((row - top) as u16, col as u16);
            // `PREBUFFER`'ın vurgusu yok: `region_highlight` yalnız görüntünün.
            let style = index
                .checked_sub(shift)
                .map_or_else(HighlightStyle::default, |shown| style_at(state, shown));
            let is_selected = selectable_at(index).is_some_and(|at| selected.contains(&at));
            let lead = Cell {
                row,
                // Kümenin metni akıştan: küme nadir ve akış kısa, yani
                // yürüyüşün ikinci bir tamponu yok ([`placed_cluster`]).
                cluster: placed_cluster(end - index, width, clusters, || {
                    self::stream(state).skip(index).take(end - index)
                }),
                ..cell(ch, col, base, style, theme, width == 2, is_selected)
            };
            // **Seçim içerik yaratmaz** (Karar 4, ızgaranın kuralı): koşu ilk
            // çizilir seçili hücreden sonuncusuna uzanıyor. Ölçüt seçimsiz
            // hâlin çizilirliği — seçili hücrenin zemini düşüyor ve ona bakmak
            // yalnız zeminden ibaret bir hücreyi koşudan düşürürdü.
            if is_selected
                && (lead.ch.is_some()
                    || style.bg.is_some()
                    || style.standout
                    || lead.underline != UnderlineStyle::None)
            {
                // Geniş karakterde spacer'ın sütunu da: iki yarı da vurgulu.
                let last = col + width as u16 - 1;
                match &mut run {
                    Some(open) if open.row == row => open.last = last,
                    _ => {
                        // Yürüyüş satır sırasıyla, yani yeni satırın ilk seçili
                        // hücresi öncekinin koşusunu kapatıyor.
                        let done = run.replace(SelectionRun {
                            row,
                            first: col,
                            last,
                        });
                        runs.extend(done);
                    }
                }
            }
            // Gelen glyph'ler **aynı** döngüden ve aynı hücreyle: sarma, geniş
            // karakter ve kenar kuralı ikinci kez yazılmıyor.
            if arriving.contains(&index) {
                arrive_at.get_or_insert((row, col));
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
        },
    );
    runs.extend(run);

    // audit: caret'in sütunu `< cols` ([`layout`]'un caret kuralı: sıradaki
    // bir sütunluk karakterin sığdığı yer) ve satırı `window`'un içinde —
    // tekerlekle kaydırılmış pencerede dışındaysa aşağıda çizilmiyor.
    let caret = DockCaret {
        col: end.caret_col as u16,
        row: end.caret_row.saturating_sub(top) as u16,
    };
    let caret_shown = window.contains(&end.caret_row);
    match change {
        None | Some(Change::Same) => {}
        Some(Change::Insert { .. }) if owned => {
            // Koşunun hiçbir hücresi çizilmediyse (sığmayan dejenere glyph ya
            // da pencerenin dışı) konum yine caret'in yeri.
            let (row, col) = arrive_at.unwrap_or((caret.row, caret.col));
            edits(DockEdit::Arrive {
                row,
                col,
                cells: arrived,
                shift: 0,
            });
        }
        Some(Change::Delete { ghosts }) if owned => {
            // Hayaletler caret'ten başlayarak **eski düzenin** konumlarında:
            // silme (Backspace de ileri silme de) yeni caret'te başlıyor ve
            // caret'e kadarki önek iki aynada aynı, yani eski düzen o noktadan
            // aynı yürüyüşün caret'ten başlayan hâli — satırı aşan silmenin
            // hayaleti alt satırın başına iniyor, geniş glyph yarılanmıyor.
            // Pencerenin dışına düşen hayalet çizilmez (bağlam satırının
            // üstüne binerdi).
            let mut cells = EditCells::empty();
            layout_with(
                ghosts.as_slice().iter().copied(),
                usize::MAX,
                usize::from(cols.grid),
                end.caret_col,
                usize::from(TEXT_COL),
                // Hayalet listesi silinen **bütün** kod noktalarını taşıyor
                // ([`Ghosts`]), yani küme yeni düzendekiyle aynı kuruluyor:
                // `🇹🇷`'nin hayaleti tek glyph, yarım bayrak değil.
                state.cluster,
                |_| {},
                |placed| {
                    let row = end.caret_row + placed.row;
                    if !window.contains(&row) || !placed.fits(cols.grid) {
                        return;
                    }
                    // audit: `row - top < shown ≤ input_rows` ve `fits` →
                    // `col < cols`; ikisi de `u16`'dan.
                    let ghost = Cell {
                        row: (row - top) as u16,
                        cluster: placed_cluster(
                            placed.end - placed.index,
                            placed.width,
                            clusters,
                            || {
                                ghosts
                                    .as_slice()
                                    .iter()
                                    .skip(placed.index)
                                    .take(placed.end - placed.index)
                                    .map(|&(ch, _)| ch)
                            },
                        ),
                        ..cell(
                            placed.ch,
                            placed.col as u16,
                            fixed,
                            placed.tag,
                            theme,
                            placed.width == 2,
                            false,
                        )
                    };
                    if ghost.ch.is_some() {
                        cells.push(ghost);
                    }
                },
            );
            edits(DockEdit::Erase {
                row: caret.row,
                col: caret.col,
                ghosts: cells,
                shift: 0,
            });
        }
        Some(_) => edits(DockEdit::Reset),
    }

    (
        Dock {
            caret: (owned && caret_shown).then_some(caret),
            ..surface
        },
        top,
        rows,
    )
}

/// Karenin düzenlemesine dikey pencerenin kaymasını ekler: `by` satır
/// (`son çizilen tepe − yeni tepe`; pencere aşağı inince uçuştakiler yukarı
/// kayıyor, negatif).
///
/// Düzenleme varsa kayma onun alanında (`bt-gpu` karede tek düzenleme
/// alıyor; ayrı bir `Shift` onu ezerdi), yoksa tek başına [`DockEdit::Shift`]
/// — caret tavanı aşan girişte satır değiştirdi ya da tekerlek pencereyi
/// kaydırdı ama metin aynı. `Reset` zaten her şeyi bitiriyor. Kaymasız kare
/// düzenlemeyi olduğu gibi geçiriyor.
pub(crate) fn with_shift(edit: Option<DockEdit>, by: i32) -> Option<DockEdit> {
    if by == 0 {
        return edit;
    }
    Some(match edit {
        None => DockEdit::Shift { by },
        Some(DockEdit::Arrive {
            row, col, cells, ..
        }) => DockEdit::Arrive {
            row,
            col,
            cells,
            shift: by,
        },
        Some(DockEdit::Erase {
            row, col, ghosts, ..
        }) => DockEdit::Erase {
            row,
            col,
            ghosts,
            shift: by,
        },
        Some(DockEdit::Shift { by: before }) => DockEdit::Shift {
            by: before.saturating_add(by),
        },
        Some(DockEdit::Reset) => DockEdit::Reset,
    })
}

/// [`render_with`]'in seçimsiz, tek giriş satırlı hâli — bu modülün
/// sınamalarının çağrısı; iz (`top`) ve koşular atılıyor.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render(
    state: &DockState,
    context: &DockContext,
    shell: Option<ShellState>,
    theme: &Theme,
    cols: DockCols,
    owned: bool,
    change: Option<&Change>,
    sink: impl FnMut(Cell),
    edits: impl FnMut(DockEdit),
) -> Dock {
    render_with(
        state,
        context,
        shell,
        theme,
        cols,
        CONTEXT_ROW,
        None,
        owned,
        None,
        change,
        &mut Vec::new(),
        &mut Clusters::default(),
        sink,
        edits,
    )
    .0
}

/// Metnin çizilmediği kolun düzenlemesi: aynanın ilerlediği her kol
/// uçuştakileri bitirir, `BUFFER`'ı değişmeyen ayna hiçbir şey basmaz.
fn settle(change: Option<&Change>, edits: &mut impl FnMut(DockEdit)) {
    if matches!(change, Some(change) if *change != Change::Same) {
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
fn render_context(
    context: &DockContext,
    theme: &Theme,
    cols: u16,
    row: u16,
    sink: &mut impl FnMut(Cell),
) {
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
            row,
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

/// Düzenin bir **görsel** satırı: hangi karakter aralığı, hangi sütundan.
///
/// Aralık [`layout`]'a verilen akışın karakter indeksinde ve **yarı açık**;
/// satırı kıran `\n` hiçbir satırın aralığında değil (glyph'i yok, sütunu
/// yok). Sarmanın ve satır sonunun bıraktığı satırlar aynı tipte: ayrımı
/// tüketicinin sorusu değil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct VisualLine {
    /// İlk karakterin indeksi.
    pub(crate) start: usize,
    /// Son karakterin bir sonrası; boş satırda `start`.
    pub(crate) end: usize,
    /// Satırın başladığı sütun ([`layout`]'un `first`/`rest`'i).
    pub(crate) col: usize,
}

/// [`layout`]'un kare başına tek olan cevabı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LayoutEnd {
    /// Caret'in görsel satırı, `0`'dan.
    pub(crate) caret_row: usize,
    /// Caret'in sütunu.
    pub(crate) caret_col: usize,
    /// Görsel satır sayısı, caret'in satırı dahil; en az 1.
    pub(crate) rows: usize,
}

/// Görüntünün **satır farkında** düzeni — tek yürüyüş (032 Karar 7).
///
/// Akışı `\n`'lerde böler ve `width` sütunda sarar; ilk satır `first`
/// sütunundan, devam satırlarının hepsi (sarmanın ve `\n`'in açtıkları)
/// `rest`'ten başlıyor. Görsel satırlar `line`'a sırayla akıyor, ayırma yok —
/// kare yolu bunu her karede koşuyor.
///
/// **Sütun sayısının tek yetkilisi yine [`column_width`]:** ızgaranın
/// sarması, dock'un çizimi ve bastırmanın satır aritmetiği aynı tablodan
/// sayıyor (024 Karar 1), yoksa biri gizlenir öteki görünürdü.
///
/// Kurallar ve her birinin nedeni:
///
/// - **`\n` sütun almaz, satır kırar.** Glyph'i yok; 032 öncesinde dock onu
///   tek satıra yassıltıyordu ve sütun tüketip metni eziyordu.
/// - **Geniş karakter yarılanmaz**, sığmıyorsa alt satıra geçer ve arkasında
///   boş bir sütun kalır — ızgaranın (`LEADING_WIDE_CHAR_SPACER`) kuralı; dock
///   aynı yürüyüşten sarıyor ([`dock_layout`]).
/// - **Sarma tembel:** satır yalnız bir karakter sığmadığında açılıyor, yani
///   tam dolan satırın ardından boş satır doğmuyor.
/// - **Caret, sıradaki karakterin gideceği yerde**; sonda ise bir sütunluk bir
///   karakterin gideceği yerde. Tam dolan satırın sonundaki caret bu yüzden
///   **alt satırın başında** ve o satır sayılıyor: zsh imleci satır sonunun
///   bekleyen sarma hâlinde bırakmıyor, alt satıra indiriyor — bastırmanın
///   eski formülündeki `saturating_sub(1)` kuralının karşılığı.
/// - **Sığmayan boş satır taşar, sonsuza sarmaz:** devam satırının başında
///   bile sığmayan karakter (bir sütunluk pencerede geniş glyph) yerinde
///   duruyor. `first` `rest`'ten sağdaysa ilk satır boş kalıp sarabiliyor —
///   ızgarada prompt'un bitirdiği satır.
///
/// `caret` akışın indeksinde; akıştan büyükse sona kırpılıyor.
pub(crate) fn layout(
    chars: impl IntoIterator<Item = char>,
    caret: usize,
    width: usize,
    first: usize,
    rest: usize,
    cluster: bool,
    line: impl FnMut(VisualLine),
) -> LayoutEnd {
    layout_with(
        chars.into_iter().map(|ch| (ch, ())),
        caret,
        width,
        first,
        rest,
        cluster,
        line,
        |_| {},
    )
}

/// Düzende bir karakterin yeri — [`layout_with`]'in karakter başına çıktısı.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placed<T> {
    /// Akıştaki sırası (sıfır genişlikliler ve `\n` de sayılıyor): görüntünün
    /// karakter indeksi, `region_highlight`'ın ve `CURSOR`'ın birimi.
    pub(crate) index: usize,
    /// Kümenin bir sonrası (035): kümeleme açıkken kümenin kalan kod
    /// noktaları (`🇹🇷`'nin `🇷`'si, VS16, ZWJ'li parçalar) `index..end`
    /// aralığında ve kendi `Placed`'leri yok; kapalıyken `index + 1`.
    pub(crate) end: usize,
    /// Kümenin **baş** karakteri — ızgaranın hücresindeki `c`.
    pub(crate) ch: char,
    /// Sütun genişliği, `1` ya da `2` ([`column_width`], kümeleme açıkken
    /// [`crate::cluster::width`]; sıfır buraya gelmiyor).
    pub(crate) width: usize,
    /// Görsel satır, `0`'dan.
    pub(crate) row: usize,
    /// Sütun ([`layout`]'un `first`/`rest`'i dahil).
    pub(crate) col: usize,
    /// Çağıranın karakterle taşıdığı veri (taban renk, vurgu).
    pub(crate) tag: T,
}

impl<T> Placed<T> {
    /// Karakter `cols` genişliğe sığıyor mu. Tek istisnası [`layout`]'un
    /// "sığmayan boş satır taşar" kuralı (bir sütunluk pencerede geniş glyph):
    /// çizen taraf onu çizmiyor, yoksa ızgaranın dışına yazardı.
    pub(crate) fn fits(&self, cols: u16) -> bool {
        self.col + self.width <= usize::from(cols)
    }
}

/// [`layout`]'un karakterleri de veren hâli: `place` her **görünür**
/// karakter için (sütun genişliği sıfırdan büyük) yerini alıyor — sıfır
/// genişlikli kod noktası hücre almıyor (ızgarada da kendi hücresi yok,
/// alacritty `CellExtra`), `\n` glyph'siz ve sütunsuz.
///
/// **Tek yürüyüş, iki okuyucu türü:** satır sayısını soranlar (`layout`,
/// bastırmanın ızgara hesabı) `place`'i boş geçiyor, hücre basanlar (dock'un
/// çizimi, isabet testi) onu kullanıyor. İki ayrı yürüyüş olsaydı sarma ya da
/// geniş karakter kuralı ikisinde ayrıştığı gün bant bir satır, fare bir
/// sütun kayardı.
///
/// **Kümeleme açıkken (`cluster`, 035) birim küme**, kod noktası değil:
/// ızgaranın sarmalayıcısıyla aynı kural ([`crate::cluster::extends`]) ve
/// aynı sütun ([`crate::cluster::width`]), yani `👨‍👩‍👧` ızgarada da dock'ta
/// da iki sütun ve bastırmanın aralığı ızgarayla ayrışmıyor. Küme tek
/// `Placed` (baş karakter ve baş karakterin etiketi), sarma kararı kümenin
/// tamamına; `caret` bir kümenin **içine** düşerse caret kümenin başında
/// (Karar 7) — kümenin ortasına yazılacak bir sütun yok.
// Sekizinci argüman `cluster` (035): oturumun tek bayrağı, dört çağıranın
// hepsi aynı yürüyüşü paylaştığı için bir yapıya sarılması kurucu eklerdi.
#[allow(clippy::too_many_arguments)]
pub(crate) fn layout_with<T>(
    items: impl IntoIterator<Item = (char, T)>,
    caret: usize,
    width: usize,
    first: usize,
    rest: usize,
    cluster: bool,
    mut line: impl FnMut(VisualLine),
    mut place: impl FnMut(Placed<T>),
) -> LayoutEnd {
    let width = width.max(1);
    let first = first.min(width);
    let mut row = 0;
    let mut col = first;
    let mut visual = VisualLine {
        start: 0,
        end: 0,
        col: first,
    };
    let mut at_caret = None;
    // Karakter `col`'a sığıyor mu; sığmıyorsa satır sarılabiliyor mu. Boş ve
    // `rest`'ten sağda olmayan satırda sarmak aynı yere dönmek olurdu.
    let fits = |col: usize, w: usize, visual: &VisualLine, at: usize| {
        col + w <= width || (visual.start == at && col <= rest)
    };
    let mut count = 0;
    // Açık kümenin metni; yalnız kümeleme açıkken dolduruluyor.
    let mut text = String::new();
    let mut items = items.into_iter().enumerate().peekable();
    while let Some((index, (ch, tag))) = items.next() {
        count = index + 1;
        if ch == '\n' {
            if index == caret {
                at_caret = Some(if fits(col, 1, &visual, index) {
                    (row, col)
                } else {
                    (row + 1, rest)
                });
            }
            visual.end = index;
            line(visual);
            row += 1;
            col = rest;
            visual = VisualLine {
                start: index + 1,
                end: index + 1,
                col: rest,
            };
            continue;
        }
        // Kümenin metni yalnız sıradaki kod noktası uzatabilecekse kuruluyor
        // ([`crate::cluster::Walk`]'un gerekçesi): düz metinde kare başına
        // ayırma yok. Tek kod noktalı kümenin sütunu [`column_width`]'in ta
        // kendisi, yani kümeleme düz metinde hiçbir şeyi değiştirmiyor.
        let (end, w) = if cluster
            && items
                .peek()
                .is_some_and(|&(_, (next, _))| crate::cluster::may_extend(next))
        {
            text.clear();
            text.push(ch);
            // Kümenin kalanı kendi `Placed`'ini almıyor; etiketi baş
            // karakterin (sıfır genişliklilerin bugünkü kuralı).
            while let Some((next, (c, _))) =
                items.next_if(|(_, (next, _))| crate::cluster::extends(&text, *next))
            {
                text.push(c);
                count = next + 1;
            }
            (count, crate::cluster::width(&text))
        } else {
            (index + 1, column_width(ch))
        };
        if !fits(col, w, &visual, index) {
            visual.end = index;
            line(visual);
            row += 1;
            col = rest;
            visual = VisualLine {
                start: index,
                end: index,
                col: rest,
            };
        }
        if (index..end).contains(&caret) {
            at_caret = Some((row, col));
        }
        if w > 0 {
            place(Placed {
                index,
                end,
                ch,
                width: w,
                row,
                col,
                tag,
            });
        }
        col += w;
    }
    visual.end = count;
    let (caret_row, caret_col) = match at_caret {
        Some(at) => at,
        None if fits(col, 1, &visual, count) => (row, col),
        None => {
            // Sondaki caret tam dolan satırın ardında: satırı kapat, caret'e
            // kendi (boş) satırını aç.
            line(visual);
            row += 1;
            visual = VisualLine {
                start: count,
                end: count,
                col: rest,
            };
            (row, rest)
        }
    };
    line(visual);
    LayoutEnd {
        caret_row,
        caret_col,
        rows: row + 1,
    }
}

/// Bastırmanın satır aritmetiği: giriş imlecin ızgaradaki satırının kaç satır
/// **üstünden** başlıyor ve kaç satır **altına** uzanıyor.
///
/// [`layout`]'un **ızgara** parametrizasyonu (032 Karar 7): zsh'in düzeni —
/// ilk satır prompt'un bittiği sütundan, devam satırları `0`'dan. Prompt'un
/// genişliği aynada yok ama gözleniyor: imlecin ızgaradaki sütunu
/// (`cursor_col`) eksi imleçten önceki metnin sütunu, `width` modunda.
///
/// **Gözlem imlecin mantıksal satırı ilk satırsa kesin.** İmleç bir `\n`'in
/// arkasındaysa ilk satırın başı bu sütundan çıkmıyor ve [`TEXT_COL`]
/// varsayılıyor: bastırma yalnız dock'lu kademede koşuyor ve orada `PS1`
/// tam o kadar sütun (betiğin iki boşluğu ile bu sabiti bir sınama
/// bağlıyor). Varsayım yanlışsa üst uç çağıranda çıpanın satırıyla
/// kırpılıyor (`from.max(floor)`), yani prompt'un üstüne taşamaz.
/// **Bilinen sınır:** `PS2` satırında (`for> `) `BUFFER`'ın ilk satırı
/// `PS2`'nin arkasında başlıyor ve genişliği aynada yok; o hâlde
/// `PREBUFFER` dolu ve üst taban zaten çıpanın satırı
/// ([`crate::shell::SuppressedInput::from_anchor`]), yani bu sayı
/// kullanılmıyor.
///
/// **Bilinen sınır, yönü güvenli:** geniş karakterin satır sonunda bıraktığı
/// boş sütun imleçten **önceyse** gözlenen başlangıç o kadar sağa kayar ve
/// üst uç bir satır fazla çıkabilir; üst uç çağıranda çıpanın satırıyla
/// kırpıldığı için (`from.max(floor)`) prompt'un üstüne taşamaz. Eski sütun
/// bölmesinin de aynı sınırı vardı. İmleçten **sonrası** ise artık doğru:
/// orada bölme boşluğu görmüyor ve kuyruğu eksik sayıyordu.
pub(crate) fn grid_span(
    display: &str,
    caret: usize,
    cursor_col: usize,
    width: usize,
    cluster: bool,
) -> (usize, usize) {
    let width = width.max(1);
    // İmlecin mantıksal satırında, imleçten önceki sütunlar — kümeleme
    // açıkken kümeyle ([`layout_with`]'in birimi). **İmlecin içine düştüğü
    // küme sayılmıyor**: ZLE kümeyi bilmiyor (wcwidth, kod noktası kod
    // noktası), yani `👍🏽`'den sonra ← `CURSOR`'ı `🏽`'nin önüne koyuyor ve
    // ızgaranın imleci kümenin baş sütununda; düzen de caret'i kümenin
    // başına oturtuyor (Karar 7). Yarım kümeyi saymak başlangıç sütununu
    // iki sütun sola kaydırır ve sarma sınırında bastırma bir satır şaşardı
    // (`/code-review`, phase-3).
    let mut on_line = 0;
    let mut first_line = true;
    if cluster {
        crate::cluster::Walk::new().run(display.chars(), |at| {
            if at.end > caret {
                return;
            }
            if at.head == '\n' {
                on_line = 0;
                first_line = false;
            } else {
                on_line += at.width;
            }
        });
    } else {
        for ch in display.chars().take(caret) {
            if ch == '\n' {
                on_line = 0;
                first_line = false;
            } else {
                on_line += column_width(ch);
            }
        }
    }
    let first = if first_line {
        (cursor_col % width + width - on_line % width) % width
    } else {
        usize::from(TEXT_COL)
    };
    let end = layout(display.chars(), caret, width, first, 0, cluster, |_| {});
    (end.caret_row, end.rows - 1 - end.caret_row)
}

/// Bir karakterin hücresi: taban rengi + aralığın stili.
///
/// **Seçili hücre ızgaranın kuralıyla** (031 Karar 3): metin kendi ön
/// planıyla, ters video çözülmüş, zemini düşük — seçimin rengi onun yerine
/// geçiyor. `region_highlight`'ın `standout`'u (zsh'in yapıştırma vurgusu
/// varsayılan olarak o) seçimde normal ön planıyla okunuyor.
fn cell(
    ch: char,
    col: u16,
    base: LinearRgba,
    style: HighlightStyle,
    theme: &Theme,
    wide: bool,
    selected: bool,
) -> Cell {
    let mut fg = style.fg.map_or(base, |color| resolve(color, theme));
    let mut bg = style.bg.map(|color| resolve(color, theme));
    if selected {
        bg = None;
    } else if style.standout {
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
        // Kümeyi çağıran koyuyor: bu fonksiyon tek karakter görüyor.
        cluster: None,
    }
}

/// Düzendeki bir kümenin sınır kimliği (035 Karar 4B/6): yalnız **geniş** ve
/// birden çok kod noktalı küme tabloya iniyor — ızgaranın kuralı
/// (`session::cell_cluster`). `chars` kümenin kod noktaları; yalnız küme
/// doğacaksa okunuyor, yani düz metin bir karşılaştırmadan fazlasını
/// ödemiyor. Kümeleme kapalıyken `len` her zaman `1`.
fn placed_cluster<I: Iterator<Item = char>>(
    len: usize,
    width: usize,
    clusters: &mut Clusters,
    chars: impl FnOnce() -> I,
) -> Option<ClusterId> {
    (len > 1 && width == 2)
        .then(|| clusters.push_chars(chars()))
        .flatten()
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

    /// Sütun sayısı: sınamaların çoğu sarmayı sormuyor ve bu genişlik
    /// onların metnini rahat alıyor.
    const COLS: u16 = 40;

    fn live(predisplay: &str, buffer: &str, postdisplay: &str, cursor: usize) -> DockState {
        DockState {
            status: DockStatus::Live,
            predisplay: predisplay.into(),
            buffer: buffer.into(),
            postdisplay: postdisplay.into(),
            prebuffer: String::new(),
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
            // Tazelik kapısının damgası; `render` okumuyor.
            answers: 0,
            // Kümesiz okunuş: kümeli sınamalar bunu açıyor.
            cluster: false,
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

    /// Tek satırlık dock'ta caret'in beklenen yeri.
    fn caret_at(col: u16) -> Option<DockCaret> {
        Some(DockCaret { col, row: 0 })
    }

    /// `input_rows` giriş satırlı çizim, seçimle: hücreler, yüzey, koşular ve
    /// dikey pencerenin tepesi (isabet testinin izi).
    fn draw_rows(
        state: &DockState,
        cols: u16,
        input_rows: u16,
        selection: Option<(usize, usize)>,
    ) -> (Vec<Cell>, Dock, Vec<SelectionRun>, usize) {
        draw_scrolled(state, cols, input_rows, None, selection)
    }

    /// [`draw_rows`]'un tekerlekle kaydırılmış pencereli hâli.
    fn draw_scrolled(
        state: &DockState,
        cols: u16,
        input_rows: u16,
        scroll: Option<usize>,
        selection: Option<(usize, usize)>,
    ) -> (Vec<Cell>, Dock, Vec<SelectionRun>, usize) {
        let mut cells = Vec::new();
        let mut runs = Vec::new();
        let (dock, top, _) = render_with(
            state,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            input_rows,
            scroll,
            true,
            selection,
            None,
            &mut runs,
            &mut Clusters::default(),
            |cell| cells.push(cell),
            |_| (),
        );
        (cells, dock, runs, top)
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
        assert_eq!(dock.caret, caret_at(TEXT_COL + 8));
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
        let color = |shell| draw_as(&state, shell, COLS).1.sigil.expect("işaret yok");
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
            assert_eq!(dock.caret, caret_at(TEXT_COL), "{shell:?} caret vermedi");
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
    fn a_multiline_mirror_draws_its_rows_and_keeps_the_caret() {
        // **032'ye kadar bu ayna `Multiline`'dı ve dock hiçbir şey
        // çizmiyordu** (satır da caret'i de ızgaradaydı; tek satıra
        // yassıltmak metni görünmez boşluklarla eziyordu). Artık satır sonu
        // satır kırıyor: her mantıksal satır kendi görsel satırında, metnin
        // sütunundan, caret de kendi satırında.
        let state = live("", "echo a\necho b", "", 9);
        assert_eq!(needed_rows(&state, COLS), 2);
        let (cells, dock, _, top) = draw_rows(&state, COLS, 2, None);
        assert_eq!(top, 0);
        assert_eq!(row_text(&cells, 0), "  echo a");
        assert_eq!(row_text(&cells, 1), "  echo b");
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 2,
                row: 1
            })
        );
    }

    /// **`PREBUFFER` düzenlenebilir satırların üstünde** (032 Karar 2): ZLE'nin
    /// kabul ettiği `for` satırı dock'ta, aynı girintide ve aynı renkte;
    /// işaret komutun başladığı satırda, caret `BUFFER`'ın satırında. Seçim
    /// onun üstüne de uzanıyor (seçilebilir, kopyalanabilir), isabet testi de
    /// oraya iniyor.
    #[test]
    fn the_prebuffer_rows_sit_above_the_editable_rows() {
        let state = DockState {
            prebuffer: "for i in 1 2; do\n".into(),
            ..live("", "echo $i", "", 7)
        };
        assert_eq!(needed_rows(&state, COLS), 2);
        let (cells, dock, _, top) = draw_rows(&state, COLS, 2, None);
        assert_eq!(top, 0);
        assert_eq!(row_text(&cells, 0), "  for i in 1 2; do");
        assert_eq!(row_text(&cells, 1), "  echo $i");
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 7,
                row: 1
            })
        );
        assert!(dock.sigil.is_some(), "işaret komutun ilk satırında");
        let fg = THEME.foreground_linear();
        assert!(
            cells
                .iter()
                .filter(|cell| cell.ch.is_some())
                .all(|cell| cell.fg == fg),
            "PREBUFFER metnin rengiyle çizilmeli"
        );

        // İsabet: `PREBUFFER`'a düşen nokta seçilebilir metnin başında, `BUFFER`'a
        // düşen `PREBUFFER`'ın uzunluğu kadar ileride.
        let shift = prebuffer_chars(&state);
        assert_eq!(shift, 17);
        let at = |row, col| hit(&state, 0, COLS, row, col, CellHalf::Left).map(|p| p.index);
        assert_eq!(at(0, TEXT_COL + 4), Some(4));
        assert_eq!(at(1, TEXT_COL), Some(shift));
        assert_eq!(at(1, TEXT_COL + 3), Some(shift + 3));
        assert_eq!(selectable(&state), "for i in 1 2; do\necho $i");

        // Seçim iki satıra yayılıyor: `PREBUFFER`'ın sonundan `BUFFER`'ın başına.
        let (_, _, runs, _) = draw_rows(&state, COLS, 2, Some((10, shift + 4)));
        assert_eq!(
            runs,
            [
                run(0, TEXT_COL + 11, TEXT_COL + 15),
                run(1, TEXT_COL, TEXT_COL + 3)
            ]
        );
    }

    #[test]
    fn the_context_row_sits_under_the_input_block() {
        // **Bağlam satırı giriş bloğunun altında** (032 phase-2): satır
        // numarası çizilecek giriş satırı sayısının ta kendisi, sabit `1`
        // değil. `bt-gpu` aynı sayıdan bandın dibine yerleştiriyor; burada
        // ayrışsalardı bağlam satırı bir giriş satırının yerine düşerdi.
        let mut cells = Vec::new();
        render_with(
            &live("", "ls", "", 2),
            &context("/tmp/x", "main"),
            None,
            &THEME,
            same(COLS),
            3,
            None,
            true,
            None,
            None,
            &mut Vec::new(),
            &mut Clusters::default(),
            |cell| cells.push(cell),
            |_| (),
        );
        assert_eq!(row_text(&cells, 3), "/tmp/x | main");
        assert_eq!(
            row_text(&cells, CONTEXT_ROW),
            "",
            "bağlam eski satırda kaldı"
        );
        assert_eq!(
            row_text(&cells, 0).trim(),
            "ls",
            "giriş satırı yerinden oynadı"
        );
    }

    #[test]
    fn the_budget_is_a_share_of_the_grid_and_keeps_one_row() {
        // Tavan ızgaranın satırlarının payı (032 Karar 4), aşağı yuvarlanmış;
        // dock'un giriş satırı hiç kaybolmuyor: sıfır pay da, tek satırlık
        // ızgara da bir satır veriyor.
        let half = DockBudget {
            share: 0.5,
            cols: 80,
        };
        assert_eq!(half.fit(1, 8), 1);
        assert_eq!(half.fit(9, 8), 4, "tavan ızgaranın yarısı");
        assert_eq!(half.fit(9, 9), 4, "yarım satır aşağı yuvarlanmalı");
        assert_eq!(half.fit(3, 1), 1);
        assert_eq!(half.fit(0, 10), 1);
        let none = DockBudget { share: 0.0, ..half };
        assert_eq!(none.fit(3, 8), 1);
    }

    #[test]
    fn a_long_line_wraps_under_the_text_column() {
        // 030'un soldan pencerelemesinin karşılığı (032 Karar 3): taşan satır
        // artık **sarılıyor** ve komutun tamamı görünüyor. Devam satırları
        // metnin sütunundan (asma girinti), caret sarılan satırın kendi
        // sütununda.
        let cols = 10;
        let buffer: String = ('a'..='z').collect();
        let state = live("", &buffer, "", 26);
        assert_eq!(needed_rows(&state, cols), 4);
        let (cells, dock, _, top) = draw_rows(&state, cols, 4, None);
        assert_eq!(top, 0);
        let rows: Vec<String> = (0..4).map(|row| row_text(&cells, row)).collect();
        assert_eq!(rows, ["  abcdefgh", "  ijklmnop", "  qrstuvwx", "  yz"]);
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 2,
                row: 3
            })
        );
        assert!(dock.sigil.is_some(), "ilk satır ekranda, işaret de");

        // Caret başa dönünce de bütün satırlar yerinde: pencere yok, sarma var.
        let (cells, dock, _, _) = draw_rows(&live("", &buffer, "", 0), cols, 4, None);
        assert_eq!(row_text(&cells, 2), "  qrstuvwx");
        assert_eq!(dock.caret, caret_at(TEXT_COL));
    }

    /// **Tavanı aşan giriş dikey pencere açıyor** (032 Karar 4): caret'in
    /// satırı görünür kalacak en küçük kayma, durumsuz — 030'un yatay
    /// penceresinin dikey ikizi. Pencerenin dışındaki satır sink'e hiç
    /// uğramıyor (bağlam satırının üstüne düşerdi); ilk satır dışarıdaysa
    /// işaret de.
    #[test]
    fn a_line_past_the_ceiling_keeps_the_caret_row_in_a_vertical_window() {
        let cols = 10;
        let buffer: String = ('a'..='z').collect();
        // Caret sonda: dört satırın son ikisi.
        let (cells, dock, _, top) = draw_rows(&live("", &buffer, "", 26), cols, 2, None);
        assert_eq!(top, 2);
        assert_eq!(row_text(&cells, 0), "  qrstuvwx");
        assert_eq!(row_text(&cells, 1), "  yz");
        assert!(
            cells.iter().all(|cell| cell.row < 2),
            "pencerenin dışı çizildi: {cells:?}"
        );
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 2,
                row: 1
            })
        );
        assert_eq!(dock.sigil, None, "ilk satır ekranda değil, işaret kalmalı");
        // Pencerenin iki satırı isabet testinin de iki satırı: `top` iz.
        let state = live("", &buffer, "", 26);
        assert_eq!(
            hit(&state, top, cols, 0, TEXT_COL, CellHalf::Left),
            Some(point(16, CellHalf::Left)),
            "pencerenin ilk satırı `q`"
        );
        // Caret ikinci satırda: pencere yukarıda duruyor, işaret yerinde.
        let (cells, dock, _, top) = draw_rows(&live("", &buffer, "", 9), cols, 2, None);
        assert_eq!(top, 0);
        assert_eq!(row_text(&cells, 1), "  ijklmnop");
        assert!(dock.sigil.is_some());
    }

    /// **Tekerlekle seçilen pencere tepesi** (032 phase-4): tavanı aşan
    /// girişte caret'in dışındaki satırlara fare de ulaşıyor. Tepe satır
    /// sayısına kırpılıyor; caret pencerenin dışındaysa çizilmiyor, işaret
    /// ilk satır ekrandaysa geri geliyor.
    #[test]
    fn a_scrolled_window_shows_the_rows_the_wheel_chose() {
        let cols = 10;
        let buffer: String = ('a'..='z').collect();
        let state = live("", &buffer, "", 26);
        let (cells, dock, _, top) = draw_scrolled(&state, cols, 2, Some(0), None);
        assert_eq!(top, 0);
        assert_eq!(row_text(&cells, 0), "  abcdefgh");
        assert_eq!(row_text(&cells, 1), "  ijklmnop");
        assert_eq!(dock.caret, None, "caret pencerenin dışında");
        assert!(dock.sigil.is_some(), "ilk satır ekranda, işaret de");
        // Taşan tepe son pencereye kırpılıyor: dört satırın son ikisi.
        let (cells, dock, _, top) = draw_scrolled(&state, cols, 2, Some(9), None);
        assert_eq!(top, 2);
        assert_eq!(row_text(&cells, 1), "  yz");
        assert_eq!(
            dock.caret,
            Some(DockCaret {
                col: TEXT_COL + 2,
                row: 1
            })
        );
    }

    /// `frame()`'in satır sayısı dock'un kendi düzeninden: `Live` olmayan
    /// ayna ve metnin sığmadığı genişlik tek satır; tam dolan satırın
    /// ardındaki caret bir satır açıyor (zsh'in ızgarasıyla aynı kural,
    /// [`layout`]); öneri bandı büyütmüyor (her tuşta boyu değişiyor).
    #[test]
    fn the_needed_rows_come_from_the_dock_layout() {
        let cols = TEXT_COL + 4;
        assert_eq!(needed_rows(&live("", "abc", "", 3), cols), 1);
        assert_eq!(needed_rows(&live("", "abcd", "", 0), cols), 1);
        assert_eq!(
            needed_rows(&live("", "abcd", "", 4), cols),
            2,
            "caret kuralı"
        );
        // Öneri bandı büyütmüyor: sarılan kısmı kırpılıyor.
        assert_eq!(needed_rows(&live("", "ab", "cdef", 2), cols), 1, "öneri");
        assert_eq!(needed_rows(&live("", "abcde", "fghijk", 5), cols), 2);
        let (cells, _, _, _) = draw_rows(&live("", "ab", "cdef", 2), cols, 1, None);
        assert_eq!(row_text(&cells, 0), "  abcd", "önerinin sığanı çizilmeli");
        assert!(cells.iter().all(|cell| cell.row == 0), "{cells:?}");
        assert_eq!(needed_rows(&live("", "abcdefghij", "", 0), TEXT_COL), 1);
        let idle = DockState {
            status: DockStatus::Idle,
            ..live("", "abcdefghij", "", 0)
        };
        assert_eq!(needed_rows(&idle, cols), 1);
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
            caret_at(TEXT_COL + 4),
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

    /// Satır sonunda geniş glyph **yarılanmıyor** (030'un pencere kenarı
    /// bekçisinin sarmadaki karşılığı, 032).
    ///
    /// Sığmayan karakter alt satıra geçiyor ve arkasında boş bir sütun
    /// kalıyor: 023'ün sözleşmesi "kutu ya da tam glyph" ve yarım glyph
    /// **sessiz** bir bozulma (`discussion.md` → Karar 2, 024). Izgaranın
    /// `LEADING_WIDE_CHAR_SPACER` kuralıyla aynı yer.
    #[test]
    fn a_wide_char_is_never_split_at_the_row_end() {
        // Metne **iki** sütun kalıyor: `a` birini yiyor, `漢` iki ister ve
        // sığmıyor — alt satırın başına iniyor, ilk satırın son sütunu boş.
        let cols = TEXT_COL + 2;
        let state = live("", "a漢", "", 0);
        let (cells, _, _, _) = draw_rows(&state, cols, 2, None);
        assert_eq!(row_text(&cells, 0), "  a", "ilk satır: {cells:?}");
        let lead = cells
            .iter()
            .find(|cell| cell.ch == Some('漢'))
            .expect("sığmayan geniş karakter kayboldu");
        assert_eq!((lead.row, lead.col), (1, TEXT_COL), "yarılandı: {lead:?}");
        assert!(lead.wide);
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
            caret_at(TEXT_COL + 3),
            "caret kontrol karakterinin sütununu saymadı"
        );
    }

    /// **Caret'in altındaki geniş karakter tam çiziliyor ve caret onun
    /// üstünde** — 030'un pencere bekçisinin sarmadaki karşılığı (032).
    ///
    /// Set kapısının (`/code-review`, 024) bulduğu regresyon: caret geniş bir
    /// glyph'in üstünde durduğunda glyph hiç çizilmiyor, caret boş bir
    /// hücrenin üstünde kalıyordu. Sarmada karakter satır sonuna sığmazsa alt
    /// satıra iniyor ve caret de onunla; tek satırlık pencerede (tavan) o
    /// satır görünür kalıyor.
    #[test]
    fn the_caret_stays_on_the_whole_wide_char_under_it() {
        // İki sütunluk genişlik, caret geniş karakterin üstünde (indeks 1).
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
            Some(DockCaret {
                col: lead.col,
                row: lead.row
            }),
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

    /// Satır caret'in karakterinden **darsa** taşma yok.
    ///
    /// Metne bir sütun ve caret'in altında iki sütunluk bir karakter:
    /// [`layout`]'un "sığmayan boş satır taşar" kolu. 030'un penceresinde
    /// burada `caret_col - skip` negatife düşüyordu (set kapısı,
    /// `/code-review`); sarmada karakter satırın dışına taşıyor ve **çizilmiyor**
    /// (ızgaranın dışına yazardı), caret metin sütununda kalıyor ve hiçbir
    /// sayı taşmıyor — debug'da `bt-core`'da kare yolunda panik yok.
    #[test]
    fn a_row_narrower_than_the_caret_char_does_not_underflow() {
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
            assert_eq!(caret.col, TEXT_COL, "{label}: caret {caret:?}");
            assert!(
                cells.iter().all(|cell| cell.col < TEXT_COL + 1),
                "{label}: pencerenin dışına yazıldı: {cells:?}"
            );
        }
    }

    // ---- Satır farkında düzen (032) ----

    /// Düzenin görsel satırları, metin olarak; ve sonu.
    fn laid_out(
        text: &str,
        caret: usize,
        width: usize,
        first: usize,
        rest: usize,
    ) -> (Vec<(String, usize)>, LayoutEnd) {
        let chars: Vec<char> = text.chars().collect();
        let mut lines = Vec::new();
        let end = layout(text.chars(), caret, width, first, rest, false, |line| {
            lines.push((chars[line.start..line.end].iter().collect(), line.col));
        });
        (lines, end)
    }

    fn end(caret_row: usize, caret_col: usize, rows: usize) -> LayoutEnd {
        LayoutEnd {
            caret_row,
            caret_col,
            rows,
        }
    }

    #[test]
    fn layout_breaks_at_newlines_and_the_newline_takes_no_column() {
        // Devam satırları `rest`'ten, ilk satır `first`'ten; `\n` hiçbir
        // satırın aralığında değil.
        let (lines, at) = laid_out("for i\ndo\ndone", 14, 20, 2, 2);
        assert_eq!(
            lines,
            vec![("for i".into(), 2), ("do".into(), 2), ("done".into(), 2)]
        );
        assert_eq!(at, end(2, 6, 3));
    }

    #[test]
    fn layout_wraps_at_the_width_and_lazily() {
        // Altı sütun, ilk satır 2'den: `abcd` sığıyor, `efghij` ikinci
        // satırı tam dolduruyor ve arkasında **boş satır doğmuyor** — caret
        // metnin ortasında.
        let (lines, at) = laid_out("abcdefghij", 1, 6, 2, 0);
        assert_eq!(lines, vec![("abcd".into(), 2), ("efghij".into(), 0)]);
        assert_eq!(at, end(0, 3, 2));
    }

    #[test]
    fn a_caret_after_a_full_row_starts_the_next_row() {
        // zsh imleci bekleyen sarma hâlinde bırakmıyor: tam dolan satırın
        // sonundaki caret alt satırın başında ve o satır sayılıyor.
        let (lines, at) = laid_out("abcd", 4, 4, 0, 0);
        assert_eq!(lines, vec![("abcd".into(), 0), (String::new(), 0)]);
        assert_eq!(at, end(1, 0, 2));
        // Dolmamış satırda caret satırın sonunda kalıyor.
        let (_, at) = laid_out("abc", 3, 4, 0, 0);
        assert_eq!(at, end(0, 3, 1));
    }

    #[test]
    fn a_wide_char_is_not_split_at_the_end_of_a_row() {
        // Beş sütun, `abcd` dört; `日` iki sütun ve beşinci sütuna sığmıyor:
        // bütünüyle alt satıra iniyor, sağda bir boş sütun kalıyor.
        let (lines, at) = laid_out("abcd日x", 4, 5, 0, 0);
        assert_eq!(lines, vec![("abcd".into(), 0), ("日x".into(), 0)]);
        // Caret geniş karakterin önünde: karakterin **gideceği** yerde, yani
        // alt satırın başında, eski satırın sonunda değil.
        assert_eq!(at, end(1, 0, 2));
        // Tam sığdığında inmiyor.
        let (lines, _) = laid_out("abc日", 0, 5, 0, 0);
        assert_eq!(lines, vec![("abc日".into(), 0)]);
    }

    #[test]
    fn a_trailing_newline_leaves_an_empty_last_row() {
        // `echo a` + satır sonu: ikinci satır boş ama var, caret orada.
        let (lines, at) = laid_out("echo a\n", 7, 20, 2, 2);
        assert_eq!(lines, vec![("echo a".into(), 2), (String::new(), 2)]);
        assert_eq!(at, end(1, 2, 2));
    }

    #[test]
    fn a_caret_right_after_a_newline_sits_at_the_next_row_start() {
        let (_, at) = laid_out("ab\ncd", 3, 20, 2, 2);
        assert_eq!(at, end(1, 2, 2));
        // Caret `\n`'in **önünde**: önceki satırın sonunda.
        let (_, at) = laid_out("ab\ncd", 2, 20, 2, 2);
        assert_eq!(at, end(0, 4, 2));
        // Tam dolan satırın ardındaki `\n` boş satır açmıyor: `\n`'in açtığı
        // satır sarmanın açacağıyla aynı satır ve önündeki caret orada.
        let (lines, at) = laid_out("abcd\ne", 4, 4, 0, 0);
        assert_eq!(lines, vec![("abcd".into(), 0), ("e".into(), 0)]);
        assert_eq!(at, end(1, 0, 2));
    }

    #[test]
    fn a_first_row_past_the_margin_wraps_before_its_first_char() {
        // Izgarada prompt satırı tam doldurmuş: ilk satır boş kalıyor, metin
        // alt satırdan başlıyor. Devam satırının başında bile sığmayan
        // karakter ise sonsuza sarmıyor, taşıyor.
        let (lines, at) = laid_out("ab", 0, 4, 4, 0);
        assert_eq!(lines, vec![(String::new(), 4), ("ab".into(), 0)]);
        assert_eq!(at, end(1, 0, 2));
        let (lines, _) = laid_out("日", 0, 1, 0, 0);
        assert_eq!(lines, vec![("日".into(), 0)]);
    }

    #[test]
    fn grid_span_matches_the_column_division_on_one_line() {
        // **Eşdeğerlik bekçisi** (032 phase-1): bastırmanın satır aritmetiği
        // sütun bölmesinden düzen yürüyüşüne taşındı ve tek satırlık bir
        // görüntüde sonuç **aynı** kalmak zorunda — tam dolan satırın
        // `saturating_sub(1)` kuralı dahil. Eski formül burada olduğu gibi
        // duruyor; tarama bütün küçük ızgaraları, imlecin her sütununu ve
        // caret'in iki yanındaki her uzunluğu deniyor.
        let old = |cursor_col: usize, before: usize, after: usize, cols: usize| {
            let above = before.saturating_sub(cursor_col).div_ceil(cols);
            let below = (cursor_col + after).saturating_sub(1) / cols;
            (above, below)
        };
        for cols in 1..=10 {
            for cursor_col in 0..cols {
                for before in 0..3 * cols {
                    for after in 0..3 * cols {
                        let text = "x".repeat(before + after);
                        assert_eq!(
                            grid_span(&text, before, cursor_col, cols, false),
                            old(cursor_col, before, after, cols),
                            "cols={cols} cursor_col={cursor_col} before={before} after={after}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn grid_span_counts_the_row_a_wide_char_is_pushed_to() {
        // Eski bölmenin görmediği tek ayrım ve yönü güvenli: beş sütunluk
        // ızgarada imleç 0. sütunda, arkasında `abcd日日日` — ilk `日` satıra
        // sığmıyor ve alt satıra iniyor, üçüncüsü de bu yüzden bir satır daha
        // aşağıda. Bölme 10 sütunu 5'e bölüp imlecin altında bir satır
        // diyordu (`(10 - 1) / 5`); ızgarada kuyruk iki satır aşağıda.
        assert_eq!(grid_span("abcd日日日", 0, 0, 5, false), (0, 2));
    }

    // ---- Kümeleme (035) ----

    /// Kümeli dizilerin parçaları ve her birinin yerine geçecek tek geniş
    /// karakter: kümeli okunuşta `👍🏽` bir `日` kadar yer tutmalı.
    const CLUSTERED: [(&str, &str); 6] = [
        ("🇹🇷", "日"),
        ("👍🏽", "日"),
        ("👨\u{200D}👩\u{200D}👧", "日"),
        ("❤\u{FE0F}", "日"),
        ("1\u{FE0F}\u{20E3}", "日"),
        ("x", "x"),
    ];

    /// Kümeli okunuşta bastırmanın ızgara yürüyüşü her kümeyi bir geniş
    /// karakter sayıyor — kümesiz yürüyüşün `日`'lı dizgide verdiğinin
    /// aynısı, satır sonuna düşen küme (`👍🏽` son iki sütunda ya da
    /// sığmayıp alt satırda) dahil. Izgaranın aynı eşdeğerliği
    /// `handler::tests`'te; ikisi birlikte "`grid_span` ızgarayla eşleşiyor".
    #[test]
    fn clustered_grid_span_counts_a_cluster_as_one_wide_char() {
        // Parça dizileri: her uzunlukta, her parça kombinasyonundan birkaçı.
        let sequences: Vec<Vec<usize>> = (0..CLUSTERED.len())
            .flat_map(|a| (0..CLUSTERED.len()).map(move |b| vec![a, 5, b, a, 5, 5, b, a, b]))
            .collect();
        for parts in &sequences {
            let clustered: Vec<&str> = parts.iter().map(|&i| CLUSTERED[i].0).collect();
            let wide: Vec<&str> = parts.iter().map(|&i| CLUSTERED[i].1).collect();
            for caret_part in 0..=parts.len() {
                let caret_of =
                    |pieces: &[&str]| -> usize { pieces[..caret_part].concat().chars().count() };
                for cols in 2..=9 {
                    for cursor_col in 0..cols {
                        assert_eq!(
                            grid_span(
                                &clustered.concat(),
                                caret_of(&clustered),
                                cursor_col,
                                cols,
                                true
                            ),
                            grid_span(&wide.concat(), caret_of(&wide), cursor_col, cols, false),
                            "{clustered:?} caret={caret_part} cols={cols} cursor_col={cursor_col}"
                        );
                    }
                }
            }
        }
    }

    /// ZLE kümeyi bilmiyor: `👍🏽`'den sonra ← `CURSOR`'ı `🏽`'nin önüne
    /// koyuyor ve ızgaranın imleci kümenin baş sütununda. Yürüyüş o hâli
    /// caret kümenin başındaymış gibi saymalı — yarım küme sayılsaydı
    /// başlangıç sütunu iki sola kayardı (`/code-review`, phase-3).
    #[test]
    fn a_caret_inside_a_cluster_counts_like_its_head_in_grid_span() {
        let text = "abc👍🏽de\u{1F1F9}\u{1F1F7}f";
        for cols in 2..=9 {
            for cursor_col in 0..cols {
                assert_eq!(
                    grid_span(text, 4, cursor_col, cols, true),
                    grid_span(text, 3, cursor_col, cols, true),
                    "ten rengi, cols={cols} cursor_col={cursor_col}"
                );
                assert_eq!(
                    grid_span(text, 8, cursor_col, cols, true),
                    grid_span(text, 7, cursor_col, cols, true),
                    "RI çifti, cols={cols} cursor_col={cursor_col}"
                );
            }
        }
    }

    /// Kapalı okunuşta kümeleme yok: `👍🏽` iki geniş karakter.
    #[test]
    fn unclustered_grid_span_keeps_code_points() {
        assert_eq!(grid_span("👍🏽👍🏽", 0, 0, 4, false), (0, 1));
        assert_eq!(grid_span("👍🏽👍🏽", 0, 0, 4, true), (0, 0));
    }

    /// Dock'un düzeni kümeyi tek geniş glyph çiziyor: aile iki sütun, tek
    /// hücre; arkasındaki harf iki sütun sağda. Kapalı okunuşta aile üç
    /// geniş glyph (ZWJ'ler sütunsuz).
    #[test]
    fn a_clustered_family_takes_two_dock_columns() {
        let family = "👨\u{200D}👩\u{200D}👧";
        let text = format!("{family}x");
        let count = text.chars().count();
        let mut state = live("", &text, "", count);
        state.cluster = true;
        let (cells, dock) = draw(&state, COLS);
        let glyphs: Vec<(char, u16)> = cells
            .iter()
            .filter_map(|cell| cell.ch.map(|ch| (ch, cell.col)))
            .collect();
        assert_eq!(glyphs, vec![('👨', TEXT_COL), ('x', TEXT_COL + 2)]);
        assert_eq!(dock.caret, caret_at(TEXT_COL + 3));
        state.cluster = false;
        let (cells, _) = draw(&state, COLS);
        let x = cells
            .iter()
            .find(|cell| cell.ch == Some('x'))
            .expect("'x' çizilmeli");
        assert_eq!(x.col, TEXT_COL + 6, "kapalı okunuş bugünkü gibi");
    }

    /// `CURSOR` kümenin **içine** düşerse caret kümenin başında (Karar 7):
    /// iki RI'nin arasında da, ZWJ'li dizinin ortasında da.
    #[test]
    fn a_caret_inside_a_cluster_sits_at_its_head() {
        for (text, inside) in [("a🇹🇷b", 2), ("a👨\u{200D}👩\u{200D}👧b", 3), ("a👍🏽b", 2)]
        {
            let mut state = live("", text, "", inside);
            state.cluster = true;
            let (_, dock) = draw(&state, COLS);
            assert_eq!(dock.caret, caret_at(TEXT_COL + 1), "{text:?} @ {inside}");
        }
    }

    /// Satır sayısı da aynı yürüyüşten: `👍🏽` son iki sütuna sığıyor ve
    /// bant bir satır kalıyor; kapalı okunuşta ten rengi alt satıra iniyor.
    #[test]
    fn a_cluster_on_the_last_two_columns_keeps_one_row() {
        // `TEXT_COL + 2` harf + küme = tam dolu satır; caret başta.
        let cols = TEXT_COL + 4;
        let mut state = live("", "ab👍🏽", "", 0);
        state.cluster = true;
        assert_eq!(needed_rows(&state, cols), 1);
        state.cluster = false;
        assert_eq!(needed_rows(&state, cols), 2);
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
        let change = change(old, new);
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
        assert_eq!(change(&old, &new), None);
        assert!(edits_between(&old, &new, COLS).is_empty());
    }

    #[test]
    fn a_new_suggestion_over_the_same_buffer_draws_nothing() {
        let old = at_end("l", 1);
        let new = DockState {
            answers: 2,
            ..live("", "l", "s -la", 1)
        };
        assert_eq!(change(&old, &new), Some(Change::Same));
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
        // `PREDISPLAY` metni sağa itiyor; sütun düzenin kendisinden.
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

    /// [`edits_between`]'in giriş satırı sayısı çağırandan gelen hâli: sarılan
    /// girişin satırları pencereye sığsın (tek satırlık pencerede alt satır
    /// çizilmez, efekti de doğmaz).
    fn edits_in_rows(old: &DockState, new: &DockState, cols: u16, rows: u16) -> Vec<DockEdit> {
        let change = change(old, new);
        let owned = caret_home(None, new.status, false) == CaretHome::Dock;
        let mut edits = Vec::new();
        render_with(
            new,
            &DockContext::default(),
            None,
            &THEME,
            same(cols),
            rows,
            None,
            owned,
            None,
            change.as_ref(),
            &mut Vec::new(),
            &mut Clusters::default(),
            |_| (),
            |edit| edits.push(edit),
        );
        edits
    }

    type Placement = ((u16, u16), Vec<(u16, u16, char)>);

    /// Düzenlemenin konumu ve hücreleri `(satır, sütun, karakter)` olarak.
    fn placed(edit: &DockEdit) -> Placement {
        let (at, cells) = match edit {
            DockEdit::Arrive {
                row, col, cells, ..
            } => ((*row, *col), cells.as_slice()),
            DockEdit::Erase {
                row, col, ghosts, ..
            } => ((*row, *col), ghosts.as_slice()),
            other => panic!("geliş ya da silme bekleniyordu: {other:?}"),
        };
        let cells = cells
            .iter()
            .filter_map(|cell| cell.ch.map(|ch| (cell.row, cell.col, ch)))
            .collect();
        (at, cells)
    }

    /// Sarılan girişte efektler **(satır, sütun)** konumunda (032 phase-6):
    /// satırı dolduran harf alt satıra sarılırken efektiyle geliyor, ikinci
    /// satırdaki Backspace hayaletini o satırda bırakıyor, birden çok
    /// karakterlik silmenin hayaletleri eski düzenin sarmasıyla alt satıra
    /// iniyor. Kayan harfler (sarmayla satır değiştiren kuyruk) düzenlemeye
    /// girmiyor — yeni konumlarında animasyonsuz.
    #[test]
    fn effects_land_on_their_row_and_column_across_a_wrapped_line() {
        let cols = TEXT_COL + 4;
        // Satır dolu (`abcd`), `e` alt satırın başına sarılıyor.
        let edits = edits_in_rows(&at_end("abcd", 1), &at_end("abcde", 2), cols, 2);
        let (at, cells) = placed(only(&edits));
        assert!(matches!(edits[0], DockEdit::Arrive { .. }), "{edits:?}");
        assert_eq!(at, (1, TEXT_COL));
        assert_eq!(cells, [(1, TEXT_COL, 'e')]);
        // Satırı dolduran harf: kendi satırında, caret alt satıra iniyor.
        let edits = edits_in_rows(&at_end("abc", 1), &at_end("abcd", 2), cols, 2);
        assert_eq!(placed(only(&edits)).1, [(0, TEXT_COL + 3, 'd')]);
        // İkinci satırda Backspace: hayalet ikinci satırda, caret'in sütununda.
        let edits = edits_in_rows(&at_end("abcdef", 1), &at_end("abcde", 2), cols, 2);
        assert!(matches!(edits[0], DockEdit::Erase { .. }), "{edits:?}");
        let (at, ghosts) = placed(only(&edits));
        assert_eq!(at, (1, TEXT_COL + 1));
        assert_eq!(ghosts, [(1, TEXT_COL + 1, 'f')]);
        // İlk satırda ileri silme: kuyruk bir satır yukarı sarılıyor ama
        // düzenleme yalnız silinen harf.
        let edits = edits_in_rows(&typed("abcdefgh", 1, 1), &typed("acdefgh", 1, 2), cols, 2);
        assert_eq!(placed(only(&edits)).1, [(0, TEXT_COL + 1, 'b')]);
        // Üç harflik silme satır sonunu aşıyor: hayaletler eski düzende —
        // ikisi ilk satırın sonunda, üçüncüsü alt satırın başında.
        let edits = edits_in_rows(&typed("abcdef", 2, 1), &typed("abf", 2, 4), cols, 2);
        assert_eq!(
            placed(only(&edits)).1,
            [
                (0, TEXT_COL + 2, 'c'),
                (0, TEXT_COL + 3, 'd'),
                (1, TEXT_COL, 'e')
            ]
        );
        // Kaymasız: pencere tepesi değişmedi.
        let DockEdit::Erase { shift, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(*shift, 0);
    }

    /// Dikey pencerenin kayması satır cinsinden ve düzenlemenin **içinde**:
    /// `bt-gpu` karede tek düzenleme alıyor, ayrı bir `Shift` onu ezerdi.
    /// Metin değişmediyse kayma tek başına geçiyor; kaymasız kare düzenlemeyi
    /// olduğu gibi bırakıyor.
    #[test]
    fn the_window_shift_rides_on_the_edit_in_rows() {
        let cols = TEXT_COL + 4;
        let edits = edits_in_rows(&at_end("abcdefgh", 1), &at_end("abcdefghi", 2), cols, 2);
        let edit = *only(&edits);
        assert_eq!(with_shift(Some(edit), 0), Some(edit));
        let Some(DockEdit::Arrive { row, shift, .. }) = with_shift(Some(edit), -1) else {
            panic!("{edit:?}");
        };
        assert_eq!((row, shift), (1, -1));
        assert_eq!(with_shift(None, -1), Some(DockEdit::Shift { by: -1 }));
        assert_eq!(with_shift(None, 0), None);
        assert_eq!(with_shift(Some(DockEdit::Reset), 2), Some(DockEdit::Reset));
    }

    /// `PREBUFFER`'ın altındaki satırda yazım: efekt `BUFFER`'ın satırında,
    /// `PREBUFFER`'ın satırları kadar aşağıda.
    #[test]
    fn an_edit_under_the_prebuffer_lands_on_the_buffer_row() {
        let cols = TEXT_COL + 20;
        let old = DockState {
            prebuffer: "for i in 1 2\n".into(),
            ..at_end("ech", 1)
        };
        let new = DockState {
            prebuffer: "for i in 1 2\n".into(),
            ..at_end("echo", 2)
        };
        let edits = edits_in_rows(&old, &new, cols, 2);
        assert_eq!(placed(only(&edits)).1, [(1, TEXT_COL + 3, 'o')]);
        // `PREBUFFER` değişti (ZLE bir satırı daha kabul etti): canlanmıyor.
        let accepted = DockState {
            prebuffer: "for i in 1 2\ndo\n".into(),
            ..at_end("echo", 3)
        };
        reset(
            &edits_in_rows(&new, &accepted, cols, 3),
            "PREBUFFER değişti",
        );
    }

    /// Öneriyle sarılan ama metni tek satır olan giriş hâlâ canlanıyor: "tek
    /// satır mı" kapısı bandın ölçüsünden (öneri hariç), yoksa uzun bir
    /// geçmiş önerisi her tuşun efektini sıfırlardı (`/code-review`).
    #[test]
    fn a_wrapping_suggestion_does_not_stop_the_effects() {
        let cols = TEXT_COL + 4;
        let old = at_end("a", 1);
        let new = DockState {
            answers: 2,
            ..live("", "ab", "cdefgh", 2)
        };
        assert_eq!(
            arrive(&edits_between(&old, &new, cols)),
            (TEXT_COL + 1, "b".into())
        );
    }

    #[test]
    fn typing_on_one_row_animates_without_a_shift() {
        let cols = TEXT_COL + 4;
        let edits = edits_between(&at_end("ab", 1), &at_end("abc", 2), cols);
        assert_eq!(arrive(&edits), (TEXT_COL + 2, "c".into()));
        let DockEdit::Arrive { shift, .. } = only(&edits) else {
            panic!("{edits:?}");
        };
        assert_eq!(*shift, 0);
    }

    #[test]
    fn a_caret_move_over_a_wrapped_line_draws_no_edit() {
        // Metin aynı, caret satırlar arasında gezindi: sarma bir görüntü
        // kararı, düzenleme değil — hiçbir şey basılmıyor.
        let cols = TEXT_COL + 4;
        let edits = edits_between(&typed("abcdefgh", 8, 1), &typed("abcdefgh", 2, 2), cols);
        assert!(edits.is_empty(), "{edits:?}");
    }

    #[test]
    fn a_line_owned_by_the_grid_does_not_animate() {
        // Caret ızgaradaysa satır da orada: efektin konusu dock'ta yazmak.
        let old = at_end("l", 1);
        let new = at_end("ls", 2);
        let change = change(&old, &new);
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

    // ---- Fareyle seçim (031 phase-4) ----

    /// Seçimle, tek giriş satırıyla çizim: hücreler, yüzey ve koşular.
    fn draw_selected(
        state: &DockState,
        cols: u16,
        selection: Option<(usize, usize)>,
    ) -> (Vec<Cell>, Dock, Vec<SelectionRun>) {
        let (cells, dock, runs, _) = draw_rows(state, cols, CONTEXT_ROW, selection);
        (cells, dock, runs)
    }

    fn run(row: u16, first: u16, last: u16) -> SelectionRun {
        SelectionRun { row, first, last }
    }

    fn point(index: usize, half: CellHalf) -> DockPoint {
        DockPoint { index, half }
    }

    /// İsabet testi **çizilen** blok üstünde: sarılan satır, satır sonuna
    /// sığmayıp alt satıra inen geniş karakter ve devam satırının asma
    /// girintisi dahil, her çizilen hücrenin (satır, sütun)'u o hücrenin
    /// karakterine iniyor. Beklenen `render`'ın kendi çıktısından — elle
    /// yazılmış bir tablo değil, yani iki yürüyüş ayrıştığı gün kırmızı.
    #[test]
    fn the_hit_test_lands_on_the_character_drawn_there() {
        // On iki sütun: metne on. `% a界bcde` dokuz sütun, `漢` sığmıyor ve
        // alt satıra iniyor — ilk satırın son sütunu boş.
        let buffer = "a界bcde漢fghi";
        let chars: Vec<char> = buffer.chars().collect();
        let state = live("% ", buffer, "ZQ", 2 + chars.len());
        let cols = TEXT_COL + 10;
        let (cells, _, _, top) = draw_rows(&state, cols, 2, None);
        assert_eq!(top, 0);
        let wrapped = cells
            .iter()
            .find(|cell| cell.ch == Some('漢'))
            .expect("漢 çizilmeli");
        assert_eq!(
            (wrapped.row, wrapped.col),
            (1, TEXT_COL),
            "sınama sarmayı sınamıyor"
        );
        let at = |row, col, half| hit(&state, top, cols, row, col, half).expect("isabet yok");
        let mut drawn = 0;
        for lead in cells.iter().filter(|cell| cell.ch.is_some()) {
            let ch = lead.ch.unwrap_or(' ');
            // Öneri `BUFFER`'ın sonuna iniyor.
            if "ZQ".contains(ch) {
                let hit = at(lead.row, lead.col, CellHalf::Left);
                assert!(hit.index >= chars.len(), "{ch}: {hit:?}");
                continue;
            }
            if "% ".contains(ch) {
                assert_eq!(
                    at(lead.row, lead.col, CellHalf::Right),
                    point(0, CellHalf::Left)
                );
                continue;
            }
            drawn += 1;
            let left = at(lead.row, lead.col, CellHalf::Left);
            assert_eq!(
                chars[left.index], ch,
                "({}, {}) başka karaktere indi",
                lead.row, lead.col
            );
            assert_eq!(left.half, CellHalf::Left);
            // Geniş karakterin **spacer** sütunu aynı karakterin sağ yarısı.
            let last = lead.col + u16::from(lead.wide);
            assert_eq!(
                at(lead.row, last, CellHalf::Right),
                point(left.index, CellHalf::Right),
                "{ch}"
            );
            if lead.wide {
                assert_eq!(
                    at(lead.row, last, CellHalf::Left),
                    point(left.index, CellHalf::Right),
                    "{ch}"
                );
            }
        }
        assert_eq!(drawn, chars.len(), "çizilen karakter eksik: {cells:?}");
        // İlk satırın sonundaki boş sütun: `BUFFER` alt satırda sürüyor, yani
        // `e`'nin sağ yarısı — ızgaranın sarılmış satırındaki kural.
        assert_eq!(
            at(0, TEXT_COL + 9, CellHalf::Left),
            point(5, CellHalf::Right)
        );
        // Devam satırının asma girintisi o satırın ilk karakterine iniyor.
        assert_eq!(at(1, 0, CellHalf::Right), point(6, CellHalf::Left));
    }

    #[test]
    fn the_hit_test_maps_the_prompt_and_the_blank_tail_to_the_buffer_ends() {
        let state = live("% ", "ls", "", 4);
        let (_, _, _, top) = draw_rows(&state, COLS, 1, None);
        let at = |col, half| hit(&state, top, COLS, 0, col, half);
        // `PREDISPLAY` seçilemiyor: başa iniyor.
        assert_eq!(
            at(TEXT_COL + 1, CellHalf::Right),
            Some(point(0, CellHalf::Left))
        );
        // Metnin sağındaki boşluk `BUFFER`'ın sonu ve ötesi: bitişik sütun
        // `len`, uzaktaki sütun ızgaranın boş hücresi gibi daha ötesi.
        assert_eq!(
            at(TEXT_COL + 4, CellHalf::Left),
            Some(point(2, CellHalf::Left))
        );
        let far = at(TEXT_COL + 20, CellHalf::Left).expect("isabet yok");
        assert_eq!(far, point(18, CellHalf::Left));
        // Sınır `len`'e kırpılıyor; kelime ızgaradaki gibi yalnız bitişikte
        // son kelimeyi alıyor, uzakta hiçbir şeyi.
        assert_eq!(
            selection_range("ls", SelectKind::Simple, far, far, false),
            (2, 2)
        );
        assert_eq!(
            selection_range("ls", SelectKind::Word, far, far, false),
            (2, 2)
        );
        let near = point(2, CellHalf::Left);
        assert_eq!(
            selection_range("ls", SelectKind::Word, near, near, false),
            (0, 2)
        );
        // `Live` olmayan aynada seçilecek metin yok.
        let idle = DockState {
            status: DockStatus::Idle,
            ..DockState::default()
        };
        assert_eq!(hit(&idle, 0, COLS, 0, TEXT_COL, CellHalf::Left), None);
    }

    /// Seçim ızgaranın görünüşüyle: tek satırlık koşu, kelime arası boşluk
    /// köprülü, kuyruktaki boşluk vurgusuz, geniş karakterin iki yarısı da
    /// içeride; seçili metin kendi ön planıyla, ters video çözülmüş ve zemini
    /// düşmüş (031 Karar 3, 4).
    #[test]
    fn a_dock_selection_is_one_run_over_what_is_drawn() {
        let mut state = live("% ", "ls 漢 x  ", "", 2);
        state.highlights.push(Highlight {
            start: 2,
            end: 4,
            style: HighlightStyle {
                standout: true,
                ..HighlightStyle::default()
            },
        });
        // `BUFFER`'ın tamamı: kuyruktaki iki boşluk seçili ama çizilir değil.
        let len = state.buffer.chars().count();
        let (cells, _, runs) = draw_selected(&state, COLS, Some((0, len)));
        // `l` metnin 2. sütununda (`% ` önek), `x` 8.'sinde (`漢` iki sütun).
        assert_eq!(runs, [run(0, TEXT_COL + 2, TEXT_COL + 8)]);
        let l = cells.iter().find(|cell| cell.ch == Some('l')).expect("l");
        assert_eq!(l.fg, THEME.foreground_linear(), "ters video çözülmedi");
        assert_eq!(l.bg, None, "seçili hücrenin zemini düşmedi");

        // Geniş karakterde bitiş spacer'ın sütunu.
        let (_, _, runs) = draw_selected(&state, COLS, Some((3, 4)));
        assert_eq!(runs, [run(0, TEXT_COL + 5, TEXT_COL + 6)]);
        // Yalnız boşluk: çizilecek bir şey yok, koşu yok (içerik yaratmaz).
        let (_, _, runs) = draw_selected(&state, COLS, Some((7, 9)));
        assert!(runs.is_empty(), "{runs:?}");
        // Seçimsiz satır standout'unu koruyor.
        let (cells, _, runs) = draw_selected(&state, COLS, None);
        assert!(runs.is_empty());
        let l = cells.iter().find(|cell| cell.ch == Some('l')).expect("l");
        assert_eq!(l.bg, Some(THEME.foreground_linear()));
    }

    /// **Satırlar arası seçim görsel satır başına bir koşu** (032 Karar 6):
    /// ızgaranın koşularıyla aynı şekil, yani çizen taraf köşeleri komşu
    /// satırın koşusuna bakarak tek parça çiziyor.
    #[test]
    fn a_dock_selection_across_wrapped_rows_is_one_run_per_row() {
        let cols = TEXT_COL + 8;
        let state = live("", "abcdefghijkl", "", 0);
        let (_, _, runs, _) = draw_rows(&state, cols, 2, Some((5, 10)));
        assert_eq!(
            runs,
            [
                run(0, TEXT_COL + 5, TEXT_COL + 7),
                run(1, TEXT_COL, TEXT_COL + 1)
            ]
        );
    }

    /// Satır sonuna sığmayan geniş karakterde yürüyüş **alt satıra geçiyor**
    /// ve arkasındaki dar karakter onun yanına düşüyor, ilk satırın boş kalan
    /// sütununa kaymıyor (030'un "yürüyüş biter" bekçisinin sarmadaki hâli;
    /// set kapısı, `/code-review`).
    #[test]
    fn a_wide_char_that_does_not_fit_wraps_and_the_next_follows_it() {
        let mut placed = Vec::new();
        layout_with(
            "abc漢d".chars().map(|ch| (ch, ())),
            0,
            4,
            0,
            0,
            false,
            |_| {},
            |at| placed.push((at.ch, at.row, at.col)),
        );
        assert_eq!(
            placed,
            [
                ('a', 0, 0),
                ('b', 0, 1),
                ('c', 0, 2),
                ('漢', 1, 0),
                ('d', 1, 2)
            ]
        );
    }

    #[test]
    fn a_dock_selection_outside_the_vertical_window_draws_nothing() {
        // Caret sonda, pencere iki satır: `BUFFER`'ın ilk satırı ekranda değil.
        let buffer = "abcdefghijklmnop";
        let cols = TEXT_COL + 8;
        let state = live("", buffer, "", buffer.len());
        let (_, _, runs, top) = draw_rows(&state, cols, 2, Some((0, 3)));
        assert_eq!(top, 1, "caret tam dolan satırın ardında: üçüncü satır");
        assert!(runs.is_empty(), "{runs:?}");
        // Kısmen görünen seçim pencerenin satırlarına kırpılıyor.
        let (_, _, runs, _) = draw_rows(&state, cols, 2, Some((0, buffer.len())));
        assert_eq!(runs, [run(0, TEXT_COL, TEXT_COL + 7)]);
    }

    #[test]
    fn simple_and_line_selections_resolve_to_buffer_ranges() {
        let buffer = "ls -la";
        let range = |kind, a, b| selection_range(buffer, kind, a, b, false);
        // Sürüklemesiz tık boş.
        let at = point(2, CellHalf::Left);
        assert_eq!(range(SelectKind::Simple, at, at), (2, 2));
        // Sol yarıdan sağ yarıya: iki uç da dahil.
        assert_eq!(
            range(
                SelectKind::Simple,
                point(3, CellHalf::Left),
                point(5, CellHalf::Right)
            ),
            (3, 6)
        );
        // Ters yön aynı aralık.
        assert_eq!(
            range(
                SelectKind::Simple,
                point(5, CellHalf::Right),
                point(3, CellHalf::Left)
            ),
            (3, 6)
        );
        // Satır: tek mantıksal satırda `BUFFER`'ın tamamı, noktadan bağımsız.
        assert_eq!(range(SelectKind::Line, at, at), (0, 6));
        // Satır sonlu `BUFFER`'da **mantıksal satır** (032 Karar 6): `\n`'ler
        // arası, satır sonu hariç; iki uç iki satırdaysa ikisi ve arası.
        let lines = "echo a\necho b\nx";
        let line = |a: usize, b: usize| {
            selection_range(
                lines,
                SelectKind::Line,
                point(a, CellHalf::Left),
                point(b, CellHalf::Left),
                false,
            )
        };
        assert_eq!(line(9, 9), (7, 13));
        assert_eq!(line(0, 0), (0, 6));
        assert_eq!(line(2, 14), (0, 15));
        assert_eq!(line(6, 6), (0, 6), "satır sonunun üstü kendi satırı");
        assert_eq!(line(40, 40), (14, 15), "sonun ötesi son satır");
        // Sağ yarı birleştiriciyi karakteriyle birlikte alıyor.
        let composed = "e\u{301}x";
        assert_eq!(
            selection_range(
                composed,
                SelectKind::Simple,
                point(0, CellHalf::Left),
                point(0, CellHalf::Right),
                false
            ),
            (0, 2)
        );
    }

    #[test]
    fn a_word_selection_follows_alacritty_semantic_rules() {
        let word = |buffer: &str, index: usize| {
            let at = point(index, CellHalf::Left);
            let (start, end) = selection_range(buffer, SelectKind::Word, at, at, false);
            buffer
                .chars()
                .skip(start)
                .take(end - start)
                .collect::<String>()
        };
        // Yol, `host:port` ve `=`'in iki yanı.
        assert_eq!(word("cd ~/src/a-b.rs", 5), "~/src/a-b.rs");
        assert_eq!(word("ssh me@host:22", 6), "me@host:22");
        assert_eq!(word("KEY=value", 6), "value");
        // Ayırıcının üstüne çift tık iki yandaki kelimeleri alıyor.
        assert_eq!(word("foo bar baz", 3), "foo bar");
        // Parantez eşini buluyor, iç içe de.
        assert_eq!(word("f (a (b) c) x", 2), "(a (b) c)");
        assert_eq!(word("f (a (b) c) x", 10), "(a (b) c)");
        // Satırın sonundaki boşluk son kelimeyi alıyor (ızgarada boş hücre).
        assert_eq!(word("git status", 10), "status");
        // Sürüklenen kelime seçimi iki ucu da genişletiyor.
        let (start, end) = selection_range(
            "one two three",
            SelectKind::Word,
            point(1, CellHalf::Left),
            point(9, CellHalf::Left),
            false,
        );
        assert_eq!((start, end), (0, 13));
    }

    // ---- Kümenin çizimi ve düzenlemesi (035 phase-4) ----

    /// Kümeli ayna (`cluster` açık), satır sonunda caret.
    fn clustered(buffer: &str, answers: u64) -> DockState {
        DockState {
            cluster: true,
            ..at_end(buffer, answers)
        }
    }

    /// Eski aynadan yenisine kümeli çizim: hücreler, düzenlemeler ve tablo.
    fn clustered_render(old: &DockState, new: &DockState) -> (Vec<Cell>, Vec<DockEdit>, Clusters) {
        let change = change(old, new);
        let (mut cells, mut edits, mut clusters) = (Vec::new(), Vec::new(), Clusters::default());
        render_with(
            new,
            &DockContext::default(),
            None,
            &THEME,
            same(COLS),
            CONTEXT_ROW,
            None,
            true,
            None,
            change.as_ref(),
            &mut Vec::new(),
            &mut clusters,
            |cell| cells.push(cell),
            |edit| edits.push(edit),
        );
        (cells, edits, clusters)
    }

    fn cluster_text(clusters: &Clusters, cell: &Cell) -> Option<String> {
        cell.cluster
            .and_then(|id| clusters.get(id))
            .map(str::to_owned)
    }

    #[test]
    fn a_dock_cluster_reaches_the_sink_as_one_string() {
        let state = clustered("a🇹🇷e\u{301}", 1);
        let (cells, _, clusters) = clustered_render(&state, &state);
        let texts: Vec<(Option<char>, Option<String>)> = cells
            .iter()
            .filter(|cell| cell.ch.is_some())
            .map(|cell| (cell.ch, cluster_text(&clusters, cell)))
            .collect();
        assert_eq!(
            texts,
            vec![
                (Some('a'), None),
                (Some('🇹'), Some("🇹🇷".into())),
                // Tek sütunlu birleştirici taban karakterle (Karar 6).
                (Some('e'), None),
            ]
        );
    }

    #[test]
    fn a_whole_cluster_erased_leaves_one_clustered_ghost() {
        // Widget `[S,E)`'yi siliyor: tek girdi, tek glyph, tek hayalet.
        let (_, edits, clusters) = clustered_render(&clustered("a🇹🇷", 1), &clustered("a", 2));
        let DockEdit::Erase { ghosts, .. } = only(&edits) else {
            panic!("silme bekleniyordu: {edits:?}");
        };
        let ghosts = ghosts.as_slice();
        assert_eq!(ghosts.len(), 1, "{ghosts:?}");
        assert!(ghosts[0].wide);
        assert_eq!(cluster_text(&clusters, &ghosts[0]).as_deref(), Some("🇹🇷"));
        // Gelen bayrak da tek glyph ve kümesiyle.
        let (_, edits, clusters) = clustered_render(&clustered("a", 1), &clustered("a🇹🇷", 2));
        let DockEdit::Arrive { cells, .. } = only(&edits) else {
            panic!("geliş bekleniyordu: {edits:?}");
        };
        assert_eq!(cells.as_slice().len(), 1);
        assert_eq!(
            cluster_text(&clusters, &cells.as_slice()[0]).as_deref(),
            Some("🇹🇷")
        );
    }

    #[test]
    fn an_edit_inside_a_cluster_does_not_animate_half_of_it() {
        // Kapı kapalıyken ZLE kod noktası siliyor: yarım bayrak canlanmıyor,
        // metin anında değişiyor. Ten rengi eklemek de yeni bir glyph değil.
        for (old, new) in [("a🇹🇷", "a🇹"), ("a👍", "a👍🏽"), ("🇹x🇷", "🇹🇷")]
        {
            let (_, edits, _) = clustered_render(&clustered(old, 1), &clustered(new, 2));
            assert_eq!(edits, vec![DockEdit::Reset], "{old:?} → {new:?}");
        }
        // Kümeleme kapalıyken bugünkü gibi: yarım bayrak tek RI'lik silme.
        let (old, new) = (at_end("a🇹🇷", 1), at_end("a🇹", 2));
        assert!(matches!(
            only(&edits_between(&old, &new, COLS)),
            DockEdit::Erase { .. }
        ));
    }

    #[test]
    fn selection_ends_snap_to_cluster_bounds() {
        let point = |index, half| DockPoint { index, half };
        // `a🇹🇷b`: bayrağın sağ yarısı arkasına, sol yarısı önüne iniyor.
        let simple = |a, h, cluster| selection_range("a🇹🇷b", SelectKind::Simple, a, h, cluster);
        assert_eq!(
            simple(point(0, CellHalf::Left), point(1, CellHalf::Right), true),
            (0, 3)
        );
        assert_eq!(
            simple(point(4, CellHalf::Left), point(2, CellHalf::Left), true),
            (1, 4),
            "kümenin içine düşen uç başına"
        );
        assert_eq!(
            simple(point(0, CellHalf::Left), point(1, CellHalf::Right), false),
            (0, 2),
            "kapalı okunuş kod noktası"
        );
        // Çift tık bayrağı bütün alıyor.
        let at = point(2, CellHalf::Left);
        assert_eq!(
            selection_range("a 🇹🇷 b", SelectKind::Word, at, at, true),
            (2, 4)
        );
    }

    #[test]
    fn the_right_half_of_a_cluster_hits_past_it() {
        // `🇹🇷` metin sütununda: sağ yarıya tık baş karakterin sağ yarısı ve
        // sınırı kümenin arkası — `🇹`/`🇷` arası değil.
        let state = clustered("🇹🇷", 1);
        let right = hit(&state, 0, COLS, 0, TEXT_COL + 1, CellHalf::Right).expect("isabet");
        assert_eq!(
            right,
            DockPoint {
                index: 0,
                half: CellHalf::Right
            }
        );
        let chars: Vec<char> = state.buffer.chars().collect();
        assert_eq!(boundary(&chars, right, true), 2);
        // Metnin sağındaki boşluk `BUFFER`'ın ötesine (sonu + uzaklık, kelime
        // seçiminin "boşluk" kuralı): son küme sarılmış bir satırın devamı
        // sayılmıyor — sayılsaydı cevap bayrağın sağ yarısı olurdu.
        assert_eq!(
            hit(&state, 0, COLS, 0, TEXT_COL + 5, CellHalf::Left),
            Some(DockPoint {
                index: 5,
                half: CellHalf::Left
            }),
        );
    }
}
