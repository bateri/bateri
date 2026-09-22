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
pub(crate) fn render(
    state: &DockState,
    context: &DockContext,
    shell: Option<ShellState>,
    theme: &Theme,
    cols: DockCols,
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
    if cols.grid == 0 {
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

    // **Caret'in sütunu, indeksi değil.** `CURSOR` karakter indeksi (ZLE'nin
    // birimi) ama görüntünün birimi sütun: geniş bir karakter indeksi bir,
    // sütunu iki ilerletiyor. Önek yalnız imlece kadar geziliyor, yani
    // maliyet eski hâlinkiyle aynı mertebede — tam dizgiyi ikinci kez
    // gezmekten kaçınmanın gerekçesi (eski yorum) hâlâ geçerli.
    let caret_col: usize = stream()
        .take(state.cursor)
        .map(|(ch, _)| column_width(ch))
        .sum();
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
    let caret_width = stream()
        .nth(state.cursor)
        .map_or(1, |(ch, _)| column_width(ch).max(1));
    let skip = (caret_col + caret_width).saturating_sub(available);

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

    Dock {
        // audit: `skip`'in tanımı gereği `caret_col - skip < available ≤ cols`.
        caret: owned.then(|| TEXT_COL + (caret_col - skip) as u16),
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
/// konusu değil ve kimse istemedi.
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
        // karakterleri de üretmiyor ve bu bir **bilinen sınır**: ZLE ham bayt
        // taşıyabiliyor (`Ctrl-V` ile yapıştırılmış bir kaçış dizisi) ve
        // bugün onlar dock'ta görünmez kalıyor. Yerinde bir yer tutucu
        // (`^C`) çizmek bir dönem sütun aritmetiğini karakter biriminden
        // çıkarırdı; o kısıt **024'te kalktı** (aritmetik zaten sütun) ve
        // yer tutucu bugün mümkün — yapılmadı çünkü kimse istemedi.
        // Ayrıntı [`column_width`]'in doc'unda.
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
        let dock = render(state, context, None, &THEME, same(cols), owned, |cell| {
            cells.push(cell)
        });
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
        render(&state, &ctx, None, &THEME, wide, owned, |cell| {
            cells.push(cell)
        });
        assert_eq!(row_text(&cells, 1), "/a/bb/ccc/dddd | main");
        // Giriş satırı **dokunulmamış**: iki bütçe birbirine karışmıyor.
        assert_eq!(text(&cells), "  ls");

        // Aynı ızgara, bütçe dar: kısaltma geri geliyor. Yani satırın gördüğü
        // sayı gerçekten `context_cols`, `cols` değil.
        let mut narrow = Vec::new();
        render(&state, &ctx, None, &THEME, same(9), owned, |cell| {
            narrow.push(cell)
        });
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
            |cell| cells.push(cell),
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
            |cell| cells.push(cell),
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
}
