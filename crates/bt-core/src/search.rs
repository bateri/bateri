//! Geçmişte arama (⌘F, 033): sorgunun derlenmesi ve görünür satırların
//! eşleşmeleri.
//!
//! **İki bütçe var ve bu modül ilkinin** (`.tasks/033-gecmiste-arama/
//! discussion.md` → Karar 2, Muhakeme): vurgu her içerik karesinde
//! [`crate::Session::frame`]'in zaten aldığı `Term` kilidi turunda, yalnız
//! **çizilen** satırlar üzerinde koşuyor — maliyeti ekranın boyuyla sınırlı.
//! Bütün defterin sayımı ayrı bir yol (phase-5).
//!
//! Eşleştiricinin kendisi alacritty'nin (`RegexSearch`, `RegexIter`) ve o tip
//! `pub` API'de görünmüyor (`lib.rs` → kapsül sözleşmesi): dışarısı yalnız
//! [`SearchQuery`], [`SearchStatus`] ve sonuç koşularını ([`SearchRuns`])
//! görüyor. Sert satır sonunu aşan eşleşme yok — alacritty'nin tarayıcısı
//! sarılmamış satır sonunda durumunu sıfırlıyor — ve boş eşleşmeyi (`^`,
//! `a*`) kendisi atlıyor.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Line, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::term::search::{Match, RegexIter, RegexSearch};

use crate::color::{LinearRgba, Theme};

/// Kullanıcının sorgusu: metin ve paneldeki iki anahtar.
///
/// Sekme başına tutulması ve ayar dosyasına yazılmaması çağıranın işi (Karar
/// 6); bu crate yalnız derliyor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchQuery {
    /// Aranan metin; `regex` kapalıyken **düz** metin ([`escape`]).
    pub text: String,
    /// `.*` anahtarı: metin bir düzenli ifade.
    pub regex: bool,
    /// `Aa` anahtarı: açıkken her zaman büyük/küçük harf duyarlı, kapalıyken
    /// **akıllı** — metinde büyük harf varsa duyarlı, yoksa değil
    /// (alacritty'nin kendi kuralı, Karar 11).
    pub case_sensitive: bool,
}

/// Sorgunun derlenmiş hâli — panelin etiketinin girdisi (Karar 3).
///
/// Geçersiz desen **panik değil durum** (R1): kullanıcı `(` yazdığı anda
/// ekran bozulmamalı, etiket "Invalid pattern" demeli.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchStatus {
    /// Boş sorgu: tarama yok, vurgu yok.
    Empty,
    /// Derlenemeyen desen (sözdizimi ya da alacritty'nin karmaşıklık
    /// sınırı); tarama yok.
    Invalid,
    /// Desen derlendi, görünür satırlar her içerik karesinde taranıyor.
    Ready,
}

/// `regex-syntax`'ın meta karakterleri — `regex_syntax::is_meta_character`'ın
/// kümesinin aynısı.
///
/// Küme burada **kopya** ve bilerek: `regex-syntax`'ı doğrudan bağımlılık
/// yapmak `Cargo.lock`'a kenar eklerdi (Karar 11). Kopyanın bekçisi sınama:
/// her karakter kaçırılınca kendisini düz olarak eşleştirmek zorunda.
const META: &[char] = &[
    '\\', '.', '+', '*', '?', '(', ')', '|', '[', ']', '{', '}', '^', '$', '#', '&', '-', '~',
];

/// Düz metni desen olarak **kendisini** eşleştiren bir düzenli ifadeye
/// çevirir: her meta karakterin önüne ters bölü.
///
/// `pub`, çünkü ikinci tüketicisi ⌘E: regex kipindeyken seçilen metin
/// kaçırılarak girer (Karar 6).
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if META.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Sorguyu derler: boşsa `Empty`, derlenemezse `Invalid`, yoksa desen.
///
/// Düz metin önce kaçırılıyor, sonra — `Aa` açıksa — `(?-i)` öneki geliyor:
/// sıra ters olsaydı önekin kendisi kaçırılırdı. Akıllı kip ayrı bir kod değil
/// alacritty'nin varsayılanı (`RegexSearch::new` desende büyük harf arıyor);
/// önek yalnız duyarlılığı **zorluyor**.
pub(crate) fn compile(query: &SearchQuery) -> (SearchStatus, Option<RegexSearch>) {
    if query.text.is_empty() {
        return (SearchStatus::Empty, None);
    }
    let body = if query.regex {
        query.text.clone()
    } else {
        escape(&query.text)
    };
    let pattern = if query.case_sensitive {
        format!("(?-i){body}")
    } else {
        body
    };
    match RegexSearch::new(&pattern) {
        Ok(regex) => (SearchStatus::Ready, Some(regex)),
        Err(_) => (SearchStatus::Invalid, None),
    }
}

/// Görünür pencerenin üstünde ve altında sarılmış bir satırın devamı için en
/// çok kaç satıra bakılacağı.
///
/// **Neden var:** görünür tepenin satırı yukarıdan sarılarak geliyorsa
/// eşleşme orada değil satırın mantıksal başında başlıyor; tarama tepeden
/// başlasaydı yarım bir eşleşme bulur (`o+`'nın kuyruğu) ya da tam bir
/// eşleşmeyi hiç bulmazdı. Tavan ise satır sonu basmayan bir akışın (`cat`
/// ile tek satırlık bir dosya) taramayı defterin tamamına yaymasını
/// engelliyor.
///
/// **Ölçülmedi**, tasarım sabiti: alacritty'nin kendi görünür arama sınırıyla
/// (`MAX_SEARCH_LINES`) aynı sayı. Tavanda kesilen satırın ortasından başlayan
/// eşleşme yüz satır yukarıda ve görünür bir koşu üretmiyor.
const WRAP_REACH: i32 = 100;

/// Satırın son hücresi sarma bayrağını taşıyor mu — yani bir sonraki satır
/// bunun devamı mı.
fn wraps<T>(term: &Term<T>, line: Line) -> bool {
    term.grid()[line]
        .last()
        .is_some_and(|cell| cell.flags.contains(Flags::WRAPLINE))
}

/// `top..=bottom` satırlarına değen eşleşmeleri soldan sağa, yukarıdan aşağı
/// verir; aralık sarılmış satırların mantıksal uçlarına genişletiliyor
/// ([`WRAP_REACH`]).
///
/// **`Term` kilidi tutulurken** çağrılır ([`crate::Session::frame`]). İki uç
/// da defterin içinde olmalı (`topmost_line..screen_lines`); çağıran onları
/// kendi kanalının satırlarından kuruyor, yani öyleler.
pub(crate) fn scan<T>(
    term: &Term<T>,
    regex: &mut RegexSearch,
    top: Line,
    bottom: Line,
    mut each: impl FnMut(&Match),
) {
    let highest = Line((top.0 - WRAP_REACH).max(term.topmost_line().0));
    let lowest = Line((bottom.0 + WRAP_REACH).min(term.bottommost_line().0));
    let mut start = top;
    while start > highest && wraps(term, Line(start.0 - 1)) {
        start = Line(start.0 - 1);
    }
    let mut end = bottom;
    while end < lowest && wraps(term, end) {
        end = Line(end.0 + 1);
    }
    let from = Point::new(start, Column(0));
    let to = Point::new(end, term.last_column());
    for found in RegexIter::new(from, to, Direction::Right, term, regex) {
        each(&found);
    }
}

/// Hücrenin **mürekkebi** var mı: gizli olmayan, spacer olmayan, boşluk
/// olmayan bir karakter.
fn inked(cell: &TermCell) -> bool {
    const BLANK: Flags = Flags::HIDDEN
        .union(Flags::WIDE_CHAR_SPACER)
        .union(Flags::LEADING_WIDE_CHAR_SPACER);
    !cell.flags.intersects(BLANK) && cell.c != ' '
}

/// Eşleşme en az bir **mürekkepli** hücreye değiyor mu.
///
/// **Vurgu içerik yaratmaz** (031'in seçim kuralı, arama için): yalnız
/// boşluktan oluşan bir eşleşme (` ` sorgusu, `\s+`) ızgaranın boş
/// satırlarını ve satır sonlarının görünmez kuyruğunu boyardı — ekranda
/// "burada bir şey var" diyen ama hiçbir şey göstermeyen bloklar. Gizli metin
/// (`\e[8m`) de sayılmıyor: çizilmeyen bir metnin yeri vurgulanmamalı.
pub(crate) fn has_ink<T>(term: &Term<T>, found: &Match) -> bool {
    let (start, end) = (*found.start(), *found.end());
    (start.line.0..=end.line.0).any(|line| {
        let first = if line == start.line.0 {
            start.column.0
        } else {
            0
        };
        let last = if line == end.line.0 {
            end.column.0
        } else {
            usize::MAX
        };
        term.grid()[Line(line)]
            .into_iter()
            .enumerate()
            .skip(first)
            .take_while(|&(col, _)| col <= last)
            .any(|(_, cell)| inked(cell))
    })
}

/// Arama vurgusunun bir satırlık parçası: `row` satırında `first..=last`
/// sütunları ([`crate::SelectionRun`]'ın uzayı).
///
/// Seçim koşusundan iki bit fazlası var ve ikisi de çizimin (phase-2)
/// girdisi:
///
/// - `current` — geçerli eşleşmenin koşusu; `search_current` rengiyle
///   çiziliyor, ötekiler `search_match` ile.
/// - `continues` — koşu bir önceki satırdaki koşunun **aynı eşleşmedeki**
///   devamı. Köşeler eşleşme başına hesaplanıyor (Karar 7): ardışık
///   satırlardaki iki ayrı eşleşme tek şekle kaynamamalı, sarılan tek eşleşme
///   kaynamalı — ayıran tek şey bu bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchRun {
    pub row: u16,
    pub first: u16,
    /// Dahil. Geniş karakterde spacer'ın sütunu — iki yarı da vurgulu.
    pub last: u16,
    pub current: bool,
    pub continues: bool,
}

/// [`crate::Session::frame`]'in arama koşuları — [`crate::SelectionRuns`]
/// emsali, çağıranın karelere yaydığı tampon (kare başına ayırma yok).
///
/// **İki liste, iki koordinat uzayı** ([`crate::Blocks`]'un `fill_slice`
/// emsali): ızgaranın ekran satırları ve doldurma kanalının fill-yerel
/// satırları (`0..top_row + fill`, kesrin tepe satırı dahil,
/// [`crate::Cursor::top_row`]). İkisi ayrı `setViewport`'ta çiziliyor.
///
/// Arama kapalıyken, sorgu boş ya da geçersizken iki liste de **boş** ve
/// tarama hiç koşmuyor (R2.2'nin durma koşulu).
///
/// **Renkler de sınırdan hazır** ([`crate::SelectionRuns`] emsali, 031 Karar
/// 9): iki rol ve odaksız eşleri `frame()`'in zaten aldığı tema kopyasından
/// yazılıyor; hangisinin çizileceği odağı bilen `bt-gpu`'nun kararı.
#[derive(Debug)]
pub struct SearchRuns {
    pub(crate) runs: Vec<SearchRun>,
    pub(crate) fill_runs: Vec<SearchRun>,
    pub(crate) colors: SearchColors,
}

/// [`SearchRuns`]'ın dört rengi, lineer: iki rol × odak.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SearchColors {
    pub(crate) matched: LinearRgba,
    pub(crate) matched_unfocused: LinearRgba,
    pub(crate) current: LinearRgba,
    pub(crate) current_unfocused: LinearRgba,
}

impl SearchColors {
    pub(crate) const fn of(theme: &Theme) -> Self {
        Self {
            matched: theme.search_match_linear(),
            matched_unfocused: theme.search_match_unfocused_linear(),
            current: theme.search_current_linear(),
            current_unfocused: theme.search_current_unfocused_linear(),
        }
    }
}

/// Koşusuz boş tampon; renkler gömülü temadan, ilk kare üstüne yazıyor
/// ([`crate::SelectionRuns`]'ın `Default`'u ile aynı gerekçe: `LinearRgba`'nın
/// `Default`'u yok, renk uydurulmuyor).
impl Default for SearchRuns {
    fn default() -> Self {
        Self {
            runs: Vec::new(),
            fill_runs: Vec::new(),
            colors: SearchColors::of(&Theme::BATERI),
        }
    }
}

impl SearchRuns {
    /// Eşleşmelerin vurgusu: odaktaki pencerede `search_match`, değilse
    /// zemine doğru soluklaşmış eşi.
    pub fn match_color(&self, focused: bool) -> LinearRgba {
        if focused {
            self.colors.matched
        } else {
            self.colors.matched_unfocused
        }
    }

    /// Geçerli eşleşmenin vurgusu; [`SearchRuns::match_color`]'ın kuralı.
    pub fn current_color(&self, focused: bool) -> LinearRgba {
        if focused {
            self.colors.current
        } else {
            self.colors.current_unfocused
        }
    }

    /// Izgaranın koşuları, satır sırasıyla; bastırılan giriş satırı hariç.
    pub fn as_slice(&self) -> &[SearchRun] {
        &self.runs
    }

    /// Doldurma kanalının koşuları; satırlar fill-yerel.
    pub fn fill_slice(&self) -> &[SearchRun] {
        &self.fill_runs
    }

    pub(crate) fn clear(&mut self) {
        self.runs.clear();
        self.fill_runs.clear();
    }
}

/// Oturumun arama yuvası — **yaprak kilit** (`theme` emsali).
///
/// Derlenmiş desen `Term` kilidinin altında `&mut` istiyor (`RegexIter`) ve
/// "arama kilidi → `Term`" sırası modülün sözleşmesini çiğnerdi. Kare yolu
/// deseni `Term` kilidinden **önce** yuvadan alıp sahipleniyor, turdan sonra
/// nesil hâlâ aynıysa geri koyuyor; `Term` altında hiçbir kilit alınmıyor
/// (`discussion.md` → Muhakeme). Kopyalanmıyor, **ödünç veriliyor**: desen
/// dört tembel DFA'nın önbelleğini taşıyor ve kare başına klonlamak hem
/// ayırma hem soğuk önbellek olurdu.
#[derive(Debug, Default)]
pub(crate) struct SearchSlot {
    /// Her `set_search`/`clear_search`'te artar; ödünç alınan desen yalnız
    /// nesil değişmediyse geri konur — araya giren yeni sorgu kazanır.
    pub(crate) generation: u64,
    /// Yuvada duran desen; kare onu ödünç almışken `None`.
    pub(crate) pattern: Option<RegexSearch>,
    /// Bir desen var mı (ödünçte olsa bile) — kare isteğinin kapısı.
    pub(crate) active: bool,
}
