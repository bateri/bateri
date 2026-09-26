//! Bir terminal oturumu: PTY, okuyucu thread ve grid.
//!
//! Crate'in kapsül sözleşmesi `lib.rs`'te; burada onun iki pratik sonucu
//! yaşıyor: alacritty'nin `EventListener`'ı `Adapter`'da bizim `Wake`'imize
//! çevrilir, ve `Term` kilidi **yalnız** şu çağrı yerlerinde alınır:
//! `frame`, `resize`, seçim yolu (`set_selection`, `extend_selection`,
//! `select_all`, `update_selection`, `clear_selection`, `selection_text`), kaydırma yolu (`scroll_wheel`,
//! `scroll_page`), kullanıcı girdisinin gönderimi (`send_input`: seçimin
//! temizliği, dibe dönüş ve okun kip sorusu aynı kilitte), `paste`'in kip
//! sorgusu (`bracketed_paste`), terminal seçeneklerinin canlı değişimi
//! (`set_terminal_options`) ve ekranı temizleme (`clear_to_start`,
//! `clear_scrollback`: dibe dönüş, kaydırma, geçmiş, seçim ve `2J` nesli
//! aynı kilitte).
//! Kilit **sırası** her yerde aynıdır — `term` önce, `size` sonra; yeni bir yer
//! eklerken bu sıraya uyulur, çünkü iki kilit ters sırada alınırsa kilitlenme
//! doğar. `theme`, `shell` ve `search` bu sıranın dışında birer **yaprak** kilittir:
//! tutulurken başka hiçbir kilit alınmaz, yani hangi kilidin altında alındığı
//! önemsizdir — `frame` temanın kopyasını `term`'den önce alıp bırakır, renk
//! sorusu `term` tutulurken okur, `set_theme` tek başına yazar; `frame`
//! aramanın desenini `term`'den önce ödünç alır ve bıraktıktan sonra geri
//! koyar (`SearchSlot`).
//!
//! `shell` için kural tek yönlüdür ve yönü şudur: **`shell` tutulurken `term`
//! alınmaz.** Okuyucu thread `shell`'i zaten `term`'ün *altında* yazıyor —
//! okuyucu döngü `pty_read` boyunca terminal lease'ini elinde tutuyor
//! (`reader.rs`, `_terminal_lease`; alacritty'nin döngüsünün kopyası ve bu
//! sözleşmeyi aynen koruyor) ve bizim `TappedPty::read`'imiz o
//! guard altında koşuyor — yani `term` → `shell` sırası okuyucunun kendi
//! sırası ve kilitlenemez. Kapanabilecek tek döngünün öteki kenarı ters yön
//! olurdu, o yüzden `frame`'in ikinci fazı (blok şeritleri) `shell`'i **`term`
//! bırakıldıktan sonra** alıyor, `shell_state` ve `set_terminal_options` da
//! öyle. (`/audit`, 010 kapı: buradaki eski gerekçe yasağı ters yöne koyuyordu
//! ve okuyucunun kendi sırasını tehlikeli gösteriyordu.)

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::ops::{DerefMut, RangeInclusive};
use std::path::PathBuf;
use std::sync::atomic::{
    AtomicBool, AtomicI64, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering,
};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Direction, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::TermMode;
// Grid hücresi **takma adla**: bu modülün `Cell`'i sınırdan geçen kare kaydı
// ve ikisi aynı ada gelseydi hangi bütçeye tabi olduğu okunamazdı
// (`CLAUDE.md` → hücre sabit boyuttadır).
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::search as search_engine;
use alacritty_terminal::term::{Config, Osc52 as TermOsc52, RenderableContent, Term};
// `EventedReadWrite` ada geliyor çünkü `io::Read` gövdesi `Pty::reader()`'ı
// çağırıyor, yani kendi impl bloğunun dışından; `EventedPty` ve `io::Read`
// gelmiyor, onların tek çağrı yeri kendi impl blokları.
use alacritty_terminal::tty::{self, EventedReadWrite as _, Pty, Shell};
use alacritty_terminal::vte::ansi::{ClearMode, CursorShape, CursorStyle, Handler};
// `Event` adı bu modülde alacritty'nin olayına ait; `polling`'inki `TappedPty`
// dışında hiç geçmediği için ada gelen o, takma alan o.
use polling::{Event as PollingEvent, PollMode, Poller};

use crate::cluster::{ClusterId, Clusters};
use crate::color::{self, LinearRgba, Theme};
use crate::dock::{self, Dock, DockBudget, DockCols, DockEdit, DockPoint};
use crate::identity::{TERM_PROGRAM, TERM_PROGRAM_VERSION, TabId};
use crate::input::{
    self, Arrow, ButtonRoute, MouseButton, MouseEncoding, MouseModifiers, WHEEL_DOWN, WHEEL_UP,
    WheelRoute,
};
use crate::reader::{EventLoop, EventLoopSender, Msg, State};
use crate::search::{
    self, SearchCover, SearchDirection, SearchQuery, SearchReport, SearchRun, SearchRuns,
    SearchSlot, SearchStatus,
};
use crate::settings::{CaretShape, CursorBlink, HostMark, HostRule};
use crate::shell::{
    COUNTER_FLOOR, CaretHome, Counter, DockContext, DockPrediction, DockSelection, DockState,
    DockStatus, Precision, RemoteTarget, Scanner, ShellLog, ShellState, Stripe, Transfer,
};
use crate::wake::Wake;

/// Alt çizgi çeşidi — beşi birbirini **dışlıyor**.
///
/// Bitflag değil enum, çünkü SGR'de de birbirini dışlıyorlar (mekanizması
/// [`Session::frame`]'in eşleme yorumunda). Bitflag olsaydı tip temsil
/// edilemeyen bir durumu (`Curl + Dotted`) taşıyabilir ve çizen tarafa bir
/// öncelik kuralı yazdırırdı.
///
/// Alacritty'nin `Flags`'i **yeniden ihraç edilmiyor** (`CLAUDE.md`):
/// `bt-core` alacritty'yi kapsüller, `pub` API'de alacritty tipi görünmez.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnderlineStyle {
    #[default]
    None,
    Single,
    Double,
    Curl,
    Dotted,
    Dashed,
}

/// Çizilecek tek hücre: bir arka plan, bir glyph, bir kural çizgisi ya da
/// hepsi.
///
/// Tek zengin tip, tek sink. İki ayrı sink (biri arka plan, biri glyph) grid'i
/// kare başına iki kez taratır ve `Term` kilidini iki kez aldırırdı; hücrenin
/// iki yüzü zaten aynı iterasyonda yan yana duruyor.
///
/// Biçim bayrakları **çözülmüş** geçer, ham geçmez: `INVERSE` ve `DIM` burada
/// renge iniyor, `HIDDEN` hem mürekkebi hem kuralları düşürüyor (aşağıda),
/// `BOLD`/`ITALIC` iki ayrı `bool`, beş alt çizgi bayrağı tek
/// [`UnderlineStyle`]. Alacritty'nin `Flags`'i hiçbir hâlde yeniden ihraç
/// edilmez — ettiği gün `bt-gpu` terminal semantiği bilmeye başlar.
///
/// **`CLAUDE.md`'nin 24 baytlık `const` assert'i bu tipe değil, alacritty'nin
/// grid hücresine bağlıdır** (`lib.rs`): 10 000 satırlık scrollback'i sekme
/// başına megabaytlarca büyüten o kayıt, bu değil. Buradaki alanlar kare
/// başına ve yalnız **çizilen** hücreler için doğuyor; sink jenerik
/// (`impl FnMut(Cell)`) ve satır içine alınıyor, yani kopyalama da bir çağrı
/// sınırından geçmiyor. Seyrek veri yan tabloya taşınacaksa ölçüt o assert
/// değil, bu tipin kare başına maliyeti olur. **Ölçüldü** (035): tip bugün
/// **76 bayt**, hizalama 4 — 023'ün 72'si artı küme kimliğinin 4'ü
/// ([`Cell::cluster`], niche'li `Option`); tek **tamponlanan** dizisi
/// doldurma bandının `Vec<Cell>`'i (`bt_gpu::link`, kapasitesi korunuyor),
/// kalan her yol satır içine alınmış değer-geçişli sink.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub col: u16,
    pub row: u16,
    /// Çizilecek karakter; **`None` = mürekkep yok**.
    ///
    /// `Option`, `' '` sentinel'i değil: boşluk karakterinin kendi başına
    /// meşru bir anlamı var ve "mürekkep yok"u onun üstüne yüklemek dört
    /// ayrık durumu (gerçek boşluk, `HIDDEN`, iki spacer) tek değere
    /// indirirdi — oysa dördü aynı şeyi istemiyor: `HIDDEN` kuralları da
    /// düşürüyor, boşluk düşürmüyor (altı çizili boşluk çizgisini alır,
    /// gizli metin almaz). `Option<char>` niche ile 4 bayt: ayrım bedava.
    pub ch: Option<char>,
    /// Ön plan; glyph bu renkle çizilir. **Her hücrede anlamlı**, `ch`
    /// `None` olsa bile: kural çizgisi ([`Cell::underline`],
    /// [`Cell::strikeout`]) mürekkebi olmayan bir hücrede de bir renk
    /// istiyor ve alan "boşlukta yer tutucu" olsaydı çizgi arka plan
    /// rengiyle, yani görünmez, çizilirdi. Üstü çizili **her zaman** bu
    /// rengi kullanır; alt çizgi ise [`Cell::underline_color`] doluysa onu,
    /// boşsa bunu — SGR'de üstü çizilinin ayrı bir rengi yok.
    ///
    /// İmleç bunu **ezmez**: hücre imlecin altında da kendi rengiyle geçer,
    /// bloğun örttüğü pikselleri çizen [`Cursor::text`] ile boyar (bkz.
    /// [`Cursor`]). Eskiden imlecin durduğu hücrede paletin arka planına
    /// çevriliyordu.
    pub fg: LinearRgba,
    /// `None` = varsayılan arka plan, çizilmez. `Frame` yalnız `Some`
    /// gördüğünde `bg_count`'u artırır: `hucre=K` jetonunun anlamı bit bit
    /// korunur ve `smoke_shell_yields_background_cells` oynamaz.
    pub bg: Option<LinearRgba>,
    /// SGR 1 ve 3. Font **yüzü değil bayrak**: `(bold, italic)` → `Face`
    /// çevirisi `bt-gpu`'da, çünkü `bt-atlas`'ın `Face`'i bir font kavramı,
    /// bu ikisi SGR semantiği. İkisi burada birleşseydi katman yönü ters
    /// dönerdi — `bt-atlas` `bt-core`'u görmüyor ve görmemeli.
    pub bold: bool,
    pub italic: bool,
    /// Beş bayrağın tek çözümü; `HIDDEN` hücrede [`UnderlineStyle::None`].
    pub underline: UnderlineStyle,
    /// SGR 58; `None` → çizen taraf [`Cell::fg`]'yi kullanır. `bg` ile
    /// birebir aynı örüntü: seyrek veri `Option`'da, varsayılanı olan
    /// tarafın adı `None`.
    ///
    /// İmleç bunu **düşürmez**: bloğun altında kalan çizgi rengini
    /// [`Cursor::text`]'ten alıyor ve bu bir piksel kararı (bkz. [`Cursor`]).
    /// Eskiden imlecin durduğu hücrede koşulsuz `None`'a düşüyordu.
    pub underline_color: Option<LinearRgba>,
    /// SGR 9; `HIDDEN` hücrede `false`.
    pub strikeout: bool,
    /// Hücre **iki sütun** genişliğinde bir karakterin baş hücresi mi.
    ///
    /// Izgaranın kararı, çizenin değil: alacritty `Flags::WIDE_CHAR`'ı
    /// `unicode-width`'e göre kuruyor ve sütun sayısının **tek yetkilisi** o.
    /// Bayrak yalnız **baş** hücrede; spacer hücresi (`WIDE_CHAR_SPACER`)
    /// bugünkü gibi mürekkepsiz geçiyor ve sağ yarıyı çizen taraf baş
    /// hücrenin bu bayrağından türetiyor. İkinci bir hücreye işaret koymak
    /// aynı olguyu iki yerde tutmak olurdu.
    ///
    /// **`true` "iki yuva" demek değil**, "iki sütun" demek: mürekkebi tek
    /// hücreye sığan geniş karakterler (`☕`, fullwidth `！`) tek yuvadan ve
    /// tek dörtlüden çiziliyor. O ayrımı [`crate`] dışında `bt-atlas`
    /// veriyor, çünkü kararı mürekkep kapısı veriyor.
    ///
    /// `LEADING_WIDE_CHAR_SPACER` bunu **almıyor**: satır sonuna sığmayan
    /// geniş karakterin bıraktığı boşluk bir baş hücre değil ve sağ yarı
    /// orada kopmuş bir glyph çizerdi.
    pub wide: bool,
    /// Hücre bir emoji **dizisinin** (`🇹🇷`, `👨‍👩‍👧`, `👍🏽`, `❤️`) baş hücresiyse
    /// dizginin çağıranın tablosundaki kimliği ([`Clusters`], 035 Karar 4B);
    /// [`Cell::ch`] yine taban karakter.
    ///
    /// Yalnız **geniş** ve birden çok kod noktalı hücrede ve yalnız
    /// kümeleme açıkken doğuyor: tek sütunlu birleştirici (`é`, `⌚︎`)
    /// bugünkü gibi taban karakterle çiziliyor (Karar 6), yani kümesiz
    /// hücre hiçbir şey ödemiyor ve kümeleme kapalıyken kare bit bit aynı.
    pub cluster: Option<ClusterId>,
}

/// **Yalnız sınama literalleri için**: `bt-gpu`'nun kare sınamaları hücreyi
/// `..Default::default()` ile kuruyor, böylece bu tipe alan eklemek onları
/// bir daha kırmıyor. Üretim yolunda tek kurucu [`Session::frame`] ve orada
/// her alan koşulsuz yazılıyor.
///
/// `fg` siyah: anlamlı bir varsayılan ön plan **yok** (palet kararı
/// `color`'ın) ve olsaydı unutulan bir alan ekranda makul görünüp sessizce
/// yanlış olurdu. [`LinearRgba`]'nın kendisi `Default` **almıyor**: tek
/// kurucusunun `from_srgb` olması renk uzayını tipe bağlayan şey.
impl Default for Cell {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            ch: None,
            fg: LinearRgba::from_srgb(0, 0, 0),
            bg: None,
            bold: false,
            italic: false,
            underline: UnderlineStyle::None,
            underline_color: None,
            strikeout: false,
            wide: false,
            cluster: None,
        }
    }
}

/// İmlecin karedeki yeri **ve bloğunun altında kalan metnin rengi**.
///
/// `row` her zaman görünür pencereye kırpılıdır. Kaydırma geçmişine bakarken
/// ([`Session::scroll_wheel`]) imleç ekranın dışına çıkar; o durumda
/// `visible` kapanır ve `row` gerçek satırı değil kırpılmış değeri taşır.
/// Kaydırmanın kendisi imleç konumunu okumuyor; konuma güvenen ilk tüketici
/// (IME) çıkmadan önce buranın sözleşmesini genişletmeli.
///
/// **Karar burada, boyama orada.** İmleç bloğu opak ve altındaki harfi
/// örtüyor; harf kendi ön planıyla kalsaydı açık gri, açık mavi bloğun üstüne
/// düşer ve okunmazdı. "Altındaki metin ne renk olmalı" bir terminal
/// semantiğidir ve bu crate'in kararı — [`Cursor::text`] onu sınırdan
/// geçiriyor. Hangi **piksellerin** o rengi alacağı çizenin işi: blok iki
/// hücre arasındayken (008) sınır hücrenin ortasından geçer ve bu crate o
/// sınırı göremez. Eskiden karar hücreye yazılıyordu (imlecin durduğu
/// hücrenin `fg`'si zemine çevriliyor, `underline_color`'ı düşürülüyordu);
/// o hâlde yarım örtülen hücrenin harfi görünmez oluyordu.
///
/// [`Eq`] yok: renk `f32` taşıyor (emsali `FontOptions`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
    /// Caret'i bu karede **dock** devraldı mı.
    ///
    /// Devrin tek yükleminin (`shell::caret_home` + dört ön koşul + **tutma**)
    /// sınırdan geçen hâli: [`Session::dock`] onu **argüman** olarak alıyor ve yeniden
    /// hesaplamıyor. `!visible` ile karıştırılmamalı — imleç uygulamanın
    /// gizlemesiyle de, geçmişe kaydırmayla da görünmez olur ve o hâllerde
    /// devralan kimse yoktur.
    pub caret_in_dock: bool,
    /// Dock'un bu karede çizeceği **giriş** satırı sayısı; bağlam satırı
    /// sayılmıyor. Verilen bütçenin ([`DockBudget::share`], ızgaranın
    /// satırlarının payı) altında: görüntünün sarılmış satır sayısı
    /// ([`dock::needed_rows`]) tavana kırpılmış hâli — aşan girişte dock kendi
    /// içinde dikey pencere açıyor.
    ///
    /// **Sıfır = giriş satırı yok** (036 Karar 8): uzak oturumda (ssh, mosh)
    /// dock yalnız bağlam satırından ibaret bir durum çubuğuna iniyor, caret
    /// ızgarada ([`Cursor::caret_in_dock`] `false`) ve dock'a tık hiçbir şey
    /// yapmıyor. Dock'u olmayan pencerede ve alternatif ekranda `1` — bant
    /// orada yok ve sayı ızgarayı ötelememeli.
    ///
    /// Bir **sınır kaydı**, grid hücresi değil: bandın çizilen yüksekliği
    /// (`bt-gpu`) ve dock'un hücrelerinin yerleşimi ([`Session::dock`]) aynı
    /// sayıyı okuyor ve sayı bastırma kararıyla **aynı okumada** doğuyor
    /// (`caret_in_dock`'un emsali: cevap hesaplandığı yerden geçer). PTY'nin
    /// ayırdığı pay bundan **bağımsız** — o `bt-gpu`'nun `DOCK_ROWS`'u ve hiç
    /// değişmiyor; bu sayı yalnız çizimi büyütüyor (032 Karar 1).
    ///
    /// Uzun tek satır sarılıp, satır sonlu görüntü (yapıştırma, `Esc-Enter`)
    /// ve `PREBUFFER` (`for`, heredoc) kendi satırlarıyla bandı büyütüyor.
    pub input_rows: u16,
    /// Caret'in **şekli** — uygulamanın DECSCUSR'ı ya da ayarın varsayılanı.
    ///
    /// `visible` ile ayrı sorular: bu "hangi biçim", o "çizilecek mi".
    /// Görünmezlik burada temsil edilmiyor (`Hidden` bloğa düşüyor,
    /// [`caret_shape_of`]) — iki yerde temsil edilen bir gerçek ayrışır ve
    /// belirti "gizli imleç çiziliyor" olurdu.
    ///
    /// Dock'un caret'i de aynı alandan besleniyor: caret tek, şekli de tek.
    pub shape: CaretShape,
    /// Caret bu an yanıp sönüyor mu — **karar**, faz değil.
    ///
    /// Fazın kendisi (şu an açık mı kapalı mı) boyayan tarafın işi: hareket
    /// karesi `bt-core`'a hiç uğramıyor ve burada üretilen bir faz o kola
    /// ulaşamazdı. Buradan geçen şey iki kaynağın birleşimi: uygulamanın
    /// DECSCUSR/DECSET 12 isteği ile kullanıcının `[terminal] cursor_blink`
    /// ayarı ([`CursorBlink::resolve`]).
    ///
    /// **Son söz burada değil.** `bt-gpu` bu biti iki şeyle daha `AND`'liyor
    /// ve ikisi de `bt-core`'un göremeyeceği şeyler: Hareketi Azalt
    /// (erişilebilirlik ayarı animasyon *eklemez*) ve pencerenin **odağı**
    /// (015 R7.4 — odakta olmayan pencerede blink duruyor, imleç görünür
    /// kalıyor). Yani buradaki `true` "sönecek" demek değil, "bu tarafta
    /// engel yok" demek.
    pub blink: bool,
    /// Bloğun altında kalan metnin (glyph **ve** kural çizgilerinin) rengi,
    /// **lineer** RGBA; bugünkü değeri temanın zemini.
    pub text: LinearRgba,
    /// Bu karenin kaydırma ofseti — imlecin **kendi** hareketini ızgaranın
    /// kaymasından ayıran tek sinyal.
    ///
    /// Konum değil bir **kimlik**: çizen taraf onu iki kare arasında
    /// karşılaştırıyor, sayı olarak kullanmıyor. Gereken sebep 008'in snap
    /// kuralı (Karar 5): imlecin kendi hareketi animasyonlu, altındaki
    /// dünyanın kayması anında. İkisi `row`'dan **ayırt edilemez** — geçmişe
    /// üç satır kaydırmak imleci ekranda üç satır aşağı taşır, tıpkı üç kez
    /// enter'a basmak gibi. Ofset oynadıysa imleç hareket etmedi, ızgara
    /// hareket etti.
    ///
    /// **011'den beri iki animatörü birden snap'liyor.** İçeriğin ötelemesi de
    /// (`content_rows`) kayıyor ve aynı ayrımı istiyor: enter'la yükselen
    /// içerik yumuşak kayar, tekerlekle gezilen geçmiş **kaymaz** — parmak
    /// neyi sürüklüyorsa onu görmeli. Kural aynı kaldı, tüketicisi ikiye
    /// çıktı.
    ///
    /// Bu alan, yukarıdaki "konuma güvenen ilk tüketici sözleşmeyi
    /// genişletmeli" cümlesinin karşılığıdır; genişleten tüketici hareket
    /// oldu, IME değil.
    pub display_offset: i32,
    /// İçeriğin tepeden kaç satır tuttuğu: `0..content_rows` aralığında
    /// çizilecek bir şey var, altı boş. Her zaman `1..=rows`.
    ///
    /// **Fact, ofset değil.** Çizen taraf bunu `rows - content_rows` ile
    /// ötelemeye çeviriyor; "içerik tabana yapışır" bir yerleşim kararı ve
    /// `bt-gpu`'nun — bu crate yalnız kaç satırın dolu olduğunu söylüyor
    /// (`CLAUDE.md` → karar burada, boyama orada; buradaki karar "hangi satır
    /// dolu", "nereye yapışacağı" değil).
    ///
    /// **İki kaynaktan birden doğuyor ve ikisi de gerekli**
    /// ([`Session::frame`]): çizilen en büyük satır ile imlecin satırı. İmleç
    /// tek başına yetmez — imleci yukarı taşıyan bir ilerleme çubuğu içeriği
    /// aşağı iterdi; çizilen satır tek başına yetmez — tamamı varsayılan
    /// zeminli boşluktan oluşan bir prompt satırı atlama kapısından geçmiyor
    /// ve giriş satırı boşluğa düşerdi.
    ///
    /// **Monoton değil:** imleci yukarı taşıyıp alt satırı `\e[K` ile silen
    /// bir program onu daraltıp genişletebilir. Değeri animasyona bağlayan
    /// taraf durma koşulunu bu salınıma göre yazmak zorunda.
    ///
    /// Alternatif ekranda `rows`, yani öteleme sıfır: vim ve htop ızgaranın
    /// tamamını sahipleniyor.
    ///
    /// **Geçmişe kaydırılmış pencerede de `rows`** (017): yaslama dibe yaslı
    /// pencerenin işi, çünkü doldurma bandı ile yaslama ekranı **tam** bölmek
    /// zorunda ve bir arada yaşayamıyorlar — `fill = rows - content_rows` ve
    /// `content_rows` görünür pencereden doğduğu için ikisi birlikte koşunca
    /// `fill + offset` sabit kalıyor, yani ekranın tepesi kaydırmayla hiç
    /// kıpırdamıyor. Gerekçenin tamamı [`Session::fill_rows`]'da, bekçisi
    /// `content_rows_come_from_the_visible_window_while_scrolled`.
    pub content_rows: u16,
    /// Üstte kalan boşluğun kaç satırı **geçmişle** dolduruldu (R2.1).
    ///
    /// `rows - content_rows` kadar boşluk var ve **temizlemeden beri** o kadar
    /// satır geldiyse hepsi, yoksa geleni:
    /// `fill = min(gap, temizlemeden beri gelen satır)`. Hiç temizleme olmamış
    /// oturumda ikinci terim defterin tamamıdır, yani formül
    /// `min(history_size, gap)`'e iniyor; kırpmanın gerekçesi ve ölçümü
    /// [`Session::fill_rows`]'da. Doldurma
    /// hücreleri [`Session::frame`]'in **ikinci** sink'inden geçiyor ve satır
    /// numaraları **fill-yerel**: `0..fill`, en üstteki en eski. Ekran satırına
    /// çeviren taraf çizen taraf — bu crate "hangi satırlar" der, "nereye"
    /// demez.
    ///
    /// **[`Cursor::content_rows`]'a girmiyor ve girmemeli**: doluluk "içerik
    /// tabana yapışsın" ötelemesinin tek girdisi ve doldurma tam da o
    /// ötelemenin açtığı boşluğu dolduruyor. Sayılsaydı öteleme kapanır,
    /// içerik tabandan kopardı — `27a0b98`'in maliyeti (R2.3).
    ///
    /// Sıfır **geri alma şeridi**: [`Session::fill_rows`] sıfır döndüğünde
    /// ikinci sink hiç çağrılmıyor ve kare bugünküyle bit bit aynı (R2.4).
    ///
    /// **Kayma uçuştayken bant uzuyor**: çizen tarafın bildirdiği ızgara
    /// tepesi ([`Session::set_grid_top`]) ile bu karede kayan satırlar
    /// ([`Cursor::scrolled`]) kadar, ki ızgara hedefinin altındayken
    /// tepesinde boş bir şerit açılmasın. Uzantı yerleşince ekranın dışında
    /// kalıyor; hesabı [`Session::slide_fill_rows`]'ta.
    ///
    /// **Kesrin tepe satırı buraya girmiyor** ([`Cursor::top_row`]): o satır
    /// aynı kanaldan geçiyor ama sayısı ayrı, yani `fill` bugünkü anlamında
    /// kalıyor — `filled` biti (`bt-gpu`) ve [`Session::fill_shown`] onu
    /// okuyor ve kaydırılmış pencerede açılmamalılar.
    pub fill: u16,
    /// Kesrin açtığı şeridi kapatan **tepe satırı** var mı: `0` ya da `1`.
    ///
    /// Kesir sıfırdan büyükken ([`Cursor::scroll_frac`]) ızgara kesir kadar
    /// aşağı çiziliyor ve tepede bir satırın parçası açılıyor; onu dolduran
    /// satır görünen pencerenin (dibe yaslı pencerede bandın) **hemen
    /// üstündeki** satır, `Line(-(offset + fill) - 1)`. Doldurma kanalından
    /// geçiyor ve kanalın **en üst** satırı: fill-yerel `0`, bandın satırları
    /// onun altında `top_row..top_row + fill`. Kanalın boyu yani
    /// `top_row + fill`.
    ///
    /// **Kapısı tek: satırın defterde olması.** Bandın kapıları (dock, Ctrl-L
    /// bayrağı, `display_offset`) burada yok — kaydırılmış pencerede, dock'suz
    /// pencerede ve Ctrl-L'den sonra da yukarı çıkan kullanıcı tepede boş bir
    /// yarım satır görmemeli. Kesir geçersizleşirse (defter silindi,
    /// tekerlek artık kaydırmıyor) kare yolu onu o karede sıfırlıyor.
    pub top_row: u16,
    /// Önceki kareden bu yana ekranın tepesinden **geçmişe kayan** satır
    /// sayısı; dolu ızgaranın kayma animasyonunun tek girdisi.
    ///
    /// Gereken sebep [`Cursor::content_rows`]'un kör noktası: ızgara dolunca
    /// doluluk `rows`'ta (dock'lu pencerede bir eksiğinde) sabitleniyor ve
    /// her yeni satır içeriği hücrelerin **içinde** kaydırıyor — öteleme hiç
    /// oynamıyor, yani ondan beslenen animatör hiçbir şey görmüyor ve satırlar
    /// süzülmeden sıçrıyordu. Belirti kullanıcıda görüldü: kayma ızgara
    /// dolana kadar var, sonra yok. Çizen taraf bu sayıyı ötelemenin
    /// **bulunduğu yerine** ekliyor, hedefine değil: ekran bir önceki karede
    /// neredeyse oradan başlıyor ve hedefe süzülüyor.
    ///
    /// Sıfır olduğu hâller: alternatif ekran, kaydırılmış pencere, boyutu
    /// değişen ızgara, kasten temizleme bayrağı ve bir önceki karede ekranda
    /// olmayan bir satır (ters kaydırma, `CSI 3 J`) — hepsinde ya kaydırma
    /// yok ya da ızgara başka bir sebeple yer değiştirdi.
    pub scrolled: u16,
    /// Kaydırmanın kesri, `[0, 1)` satır: ızgara bu kadar **aşağı** çizilecek
    /// ve tepede açılan şeridi [`Cursor::top_row`] kapatacak.
    ///
    /// Konum `display_offset + kesir` (dibe yaslı pencerede bandın boyu +
    /// kesir); tam satırın tek yetkilisi yine ofset, kesir onun üstüne
    /// eklenen tek sayı ([`Session::scroll_frac`]). Alternatif ekranda ve
    /// üstünde satır olmayan pencerede `0`.
    pub scroll_frac: f32,
    /// Kaydırma **nesli** — konum dışarıdan sıfırlandıkça artan sayı
    /// ([`ScrollGlide`]). Çizen taraf onu önceki kareninkiyle karşılaştırıp
    /// uçuştaki süzülmeyi bitiriyor: girdide dibe dönen pencereyi kalan pay
    /// geri çekmemeli.
    pub scroll_generation: u32,
    /// Bu karenin ızgara yüksekliği — [`Cursor::content_rows`]'un ölçeği.
    ///
    /// Redundant görünüyor (çizen taraf grid'i kendisi kurdu) ama değil:
    /// **aynı okumadan** geliyor. Tek alternatifi çizen tarafın kendi
    /// kopyasıydı ve o kopya `Session::resize`'ın **ret kolunda** ayrışırdı —
    /// dejenere bir boyut oturum tarafından yoksayılıyor, oysa çizen tarafın
    /// kopyası onu yazmış olurdu ve öteleme bir kare boyunca yanlış ızgara
    /// yüksekliğinden hesaplanırdı.
    pub rows: u16,
    /// Bu karenin çizdiği süre sayacı ne kadar sonra **başka bir şey**
    /// gösterecek; ilerletecek sayaç yoksa `None`.
    ///
    /// Kare talebinin **üçüncü** sebebi olan saatin tek girdisi
    /// (`bt-gpu::link` modül başlığı). Hasar grid'in değişmesine, hareket
    /// yerleşmemiş bir animasyona bağlı; koşan komutun sayacı ikisine de
    /// uymuyor — PTY'den bayt gelmiyor ve ekran hızında ilerletilecek bir şey
    /// yok.
    ///
    /// **"İçerik ne zaman değişecek" sorusunun cevabı burada, çizen tarafta
    /// değil:** sınır biçimin bir sonucu (koşan sayaç tam saniye gösteriyor)
    /// ve biçim bu crate'in kararı. Soru **daraltılmış**: saat artık iki son
    /// tarihi birleştiriyor ve ötekinin — imlecin yanıp sönme fazının —
    /// sahibi boyayan taraf (`bt_gpu::blink`), çünkü hareket karesi buraya hiç
    /// uğramıyor. `bt-gpu` yalnız verilen süreyi bekliyor, hesap
    /// yapmıyor — "karar burada, boyama orada"nın zaman eksenindeki hâli.
    ///
    /// **İki kaynağı var ve yakın olanı kazanıyor** (`shell::sooner`): koşan
    /// komutun süre sayacı ve caret devrinin **tutması** (015 phase-1). İkincisi
    /// tek atımlık; birleştirme yazma değil `min`, yoksa biri ötekini sessizce
    /// söndürürdü.
    ///
    /// **`None` durma koşuludur** ve dört yoldan doğuyor: komut bitti, koşan
    /// bloğun çıpası bu karede görünmüyor (yukarı kaydı), entegrasyon hiç yok,
    /// ya da bekleyen bir devir tutması kalmadı. Dördünde de saat sönüyor ve
    /// pencere boşta sıfır kareye dönüyor.
    pub next_tick: Option<Duration>,
}

/// Bir komut bloğunun karedeki izi: **komutun satırı** ve o komutun rengi.
///
/// **İşaret, bölge değil** (kullanıcı kararı, 010 teslim). Eskiden blok
/// kapladığı satır aralığını taşıyordu ve şerit çıktının da solunu boyuyordu;
/// bugün yalnız komutun kendi satırı işaretleniyor. Kazanç estetik değil
/// yapısal: "bu satır hangi bloğun" sorusunun cevabı ancak çıpası görünen
/// satırlar için **biliniyor**, ve bölge boyamak o bilgiyi tahmine
/// çeviriyordu. Tahminin iki bilinen kusuru (geçici prompt'ta üst bölgenin
/// yanlış renklenmesi, `exec zsh` sonrası payın kalıcı boyanması) bu tasarımda
/// temsil edilemiyor: çıpa görünmüyorsa işaret de yok.
///
/// **Çözülmüş geçer.** Çıkış kodu, blok kimliği ve kabuğun safhası bu sınırı
/// geçmez; çizen taraf "hangi satır, hangi renk" sorusunun yanıtını alır,
/// "neden o renk" sorusunu sormaz — `CLAUDE.md`'nin **karar burada, boyama
/// orada** kuralı. Renderer'da çıkış kodu tanıyan bir dal yanlış yerdedir.
///
/// Satır **görünür pencere** cinsinden; hücrelerle aynı `display_offset`'ten
/// çıkıyor, yani işaret kaydırmada bir kare geride kalmaz.
///
/// [`Eq`] yok: renk `f32` taşıyor ([`Cursor`] emsali).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    /// Komutun satırı — prompt'un çıpasını taşıyan satır.
    pub row: u16,
    /// İşaretin rengi, **lineer** RGBA.
    pub stripe: LinearRgba,
}

/// [`Session::frame`]'in blok listesi ve iki fazın arasındaki ara defter —
/// çağıranın karelere yaydığı tampon.
///
/// **Kare başına ayırma yok:** iki `Vec` de her karede `clear()` ile boşalır
/// ve ayrılan yeri korur. Çağıranda yaşamasının sebebi bu; `frame()`'in
/// içinde doğsaydı her kare iki ayırma ederdi.
///
/// Ara defter (çıpalar: `(kimlik, ilk satır, son dolu sütun)`) burada ve
/// **görünmez**: faz 1 `Term` kilidi altında onu dolduruyor, faz 2 kilit
/// bırakıldıktan sonra defterden renklendirip [`Blocks::as_slice`]'ı
/// üretiyor. Kimlik, çıkış kodu ve sütun sınırı geçmediği için tip opak.
#[derive(Debug, Default)]
pub struct Blocks {
    /// Faz 1'in topladığı çıpalar: `(blok kimliği, satır, son dolu sütun)`,
    /// satır sırasıyla.
    ///
    /// Üçüncü alan süre sayacının çakışma ölçütü: sayaç komutun metnine ya
    /// da seçim vurgusuna değecekse çizilmiyor. Faz 1'de toplanmasının sebebi kilit rejimi —
    /// sütun ızgara bilgisi ve faz 2 `Term`'ü çoktan bırakmış oluyor; ikinci
    /// bir tarama kilidi yeniden almak demekti.
    anchors: Vec<(u32, u16, u16)>,
    /// Faz 2'nin ürettiği liste; çizen taraf yalnız bunu görür.
    resolved: Vec<Block>,
    /// **Doldurma bandının kendi çıpaları**: `(blok kimliği, fill-yerel satır)`.
    ///
    /// Ayrı liste olmasının sebebi koordinat uzayı: bandın satırları
    /// `0..fill` ve ızgarayınkiler `0..content_rows` — ikisi ayrı
    /// `setViewport`'ta çiziliyor, yani tek listede toplanan satır numarası
    /// çizen tarafta anlamsız olurdu. Dock'un kendi listelerini taşıması da
    /// aynı örüntü.
    ///
    /// Üçüncü alan **yok**: süre sayacı bantta çizilmiyor ve `last_col` onun
    /// tek tüketicisiydi (gerekçe [`Blocks::fill_slice`]).
    fill_anchors: Vec<(u32, u16)>,
    /// Bandın çözülmüş listesi; satırlar **fill-yerel**.
    fill_resolved: Vec<Block>,
    /// Bastırılan giriş satırının görüntüsü (`PREDISPLAY ++ BUFFER ++
    /// POSTDISPLAY`) ve caret'in karakter indeksi — bastırmanın satır
    /// aritmetiğinin ([`crate::dock::grid_span`]) girdisi (032).
    ///
    /// Metin yaprak kilidin (`shell`) altında kopyalanıyor ve `Term` kilidinin
    /// altında yürünüyor: yürüyüş ızgaranın genişliğini ve imlecin sütununu
    /// istiyor, yaprak kilit ise `Term`'ün altına giremiyor. Burada, çünkü
    /// tampon çağıranın ömründe kapasitesini koruyor — kare başına ayırma yok
    /// (tipin öteki alanlarının gerekçesi).
    input: String,
    input_caret: usize,
    /// Aynanın okunuşu ([`crate::shell::DockState::cluster`]) — metinle
    /// aynı kilit turunda, aynı kayıttan.
    input_cluster: bool,
}

impl Blocks {
    /// Bu karede ızgarada çizilecek bloklar, satır sırasıyla.
    pub fn as_slice(&self) -> &[Block] {
        &self.resolved
    }

    /// Bu karede **doldurma bandında** çizilecek bloklar; satırlar
    /// fill-yerel (`0..top_row + fill`, [`Cursor::top_row`]).
    ///
    /// **Neden ayrı bir liste** (2026-09-20, kullanıcı bildirdi): bant ikinci
    /// bir yüzey ve ızgaradan türeyen her şeyi ayrıca kazanmak zorunda —
    /// hücreleri phase-2'de almıştı, blok işaretini almamıştı. Belirti şuydu:
    /// tamamlama listesi komut satırını geçmişe itiyor, liste kalkınca bant
    /// o satırı geri getiriyor ama **işaretsiz**, ve kullanıcı kaydırınca
    /// aynı satır ızgaradan geçtiği için işaret geri geliyordu. Ekranın Tab
    /// öncesine dönmesi 017'nin sözü; işaretsiz dönen satır o sözü tutmuyor.
    ///
    /// **Süre sayacı hâlâ bantta yok** ve bu bilinçli bir daraltma: sayaç
    /// hücre üretiyor (`Counter`), yani bandın sink'ine yazmak ve çakışma
    /// ölçütünü (`last_col`, `counted_row`) ikinci kez kurmak gerekirdi.
    /// İşaret bir `RuleCell` ve bandın `fill_rules` listesi zaten var.
    pub fn fill_slice(&self) -> &[Block] {
        &self.fill_resolved
    }
}

/// Seçim vurgusunun bir satırlık parçası: `row` satırında `first..=last`
/// sütunları, ekran koordinatında (`Cell` ile aynı uzay).
///
/// **Hücre değil satır koşusu** (031 `discussion.md` → Karar 4): koşu satırın
/// ilk çizilir seçili hücresinden sonuncusuna uzanıyor ve aradaki boşlukları
/// köprülüyor — kelime arası boşluk vurgulu (pano onu zaten kopyalıyor), satır
/// sonundaki boş kuyruk ve boş satır vurgusuz (seçim içerik yaratmaz).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionRun {
    pub row: u16,
    pub first: u16,
    /// Dahil. Geniş karakterde spacer'ın sütunu — iki yarı da vurgulu.
    pub last: u16,
}

/// [`Session::frame`]'in seçim koşuları ve iki rengi — [`Blocks`] emsali,
/// çağıranın karelere yaydığı tampon (kare başına ayırma yok).
///
/// **Renkler hazır geliyor, seçimi çizen taraf yapıyor** (Karar 9): odak
/// `bt-core`'a girmiyor, yani hangisinin çizileceğini `bt-gpu` biliyor. İkisi
/// de temanın aynı kopyasından, koşularla aynı karede — ayrı bir sorgudan
/// okunsalardı tema takasında bir kare ayrışırlardı.
#[derive(Debug)]
pub struct SelectionRuns {
    runs: Vec<SelectionRun>,
    color: LinearRgba,
    unfocused: LinearRgba,
}

/// Koşusuz boş tampon; renkler gömülü temadan, ilk kare üstüne yazıyor.
/// `LinearRgba`'nın `Default`'u yok (tek kurucusu `from_srgb`), yani renk
/// uydurulmuyor, temanın kendisinden geliyor.
impl Default for SelectionRuns {
    fn default() -> Self {
        Self {
            runs: Vec::new(),
            color: Theme::BATERI.selection_linear(),
            unfocused: Theme::BATERI.selection_unfocused_linear(),
        }
    }
}

impl SelectionRuns {
    /// Bu karenin koşuları, satır sırasıyla; satır başına en çok bir tane.
    pub fn as_slice(&self) -> &[SelectionRun] {
        &self.runs
    }

    /// Vurgunun rengi, **lineer**: odaktaki pencerede `color`, değilse
    /// zemine doğru soluklaşmış eşi ([`Theme::selection_unfocused_linear`]).
    pub fn color(&self, focused: bool) -> LinearRgba {
        if focused { self.color } else { self.unfocused }
    }
}

/// Oturumun açılış ayarları.
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// `Some((program, args))` → tam olarak o komut; `None` → alacritty'nin
    /// kendi varsayılanı (macOS'ta `/usr/bin/login …`, kullanıcının `$SHELL`'i
    /// login kabuk olarak).
    ///
    /// **`None` artık istisna, kural değil.** Üç ayrı çağıran `Some` veriyor ve
    /// gerekçeleri ayrı: süreli koşu sonucun kullanıcının rc dosyasına bağlı
    /// olmasını istemiyor (`smoke_shell`/`load_shell`), sınamalar aynı sebeple,
    /// normal oturum ise `login`'e `-q` geçirebilmek için komutu **kendi**
    /// kuruyor (`bt_shell::child::login_command`; `Last login:` banner'ı
    /// ızgaraya düşmesin diye). `None` o üçüncünün kullanıcı ya da kabuk
    /// çözülemediğinde düştüğü geri yol — banner döner, oturum çalışır.
    pub command: Option<(String, Vec<String>)>,
    /// Çocuğun başlangıç dizini. `None` → bizim sürecimizin dizinini miras
    /// alır.
    ///
    /// **Yalnız çocuğa** gider (fork'la exec arasında `chdir`); kendi
    /// sürecimizin dizini oynamaz. `chdir` exec'ten **önce** olduğu için
    /// [`SessionOptions::command`]'ın göreli program yolu ve argümanları da
    /// yeni dizine göre çözülür. Gidilemeyen bir yol (silinmiş, izinsiz)
    /// sessizce yoksayılır ve çocuk yine miras alır — alacritty'nin
    /// `pre_exec`'i `chdir`'in sonucuna bakmıyor; bu davranış
    /// `unreachable_working_directory_is_inherited` ile bağlı, alacritty onu
    /// değiştirirse orası kızarır. Hangi dizinin verileceği uygulamanın
    /// kararı, bu crate yalnız geçirir.
    pub working_directory: Option<PathBuf>,
    /// Kullanıcının ev dizini — yalnız başlığın `~` kuralı için
    /// ([`Session::title`]). Bu crate ortam okumaz, değeri uygulama veriyor;
    /// `None` → ev dizini sıradan bir dizin gibi son bileşeniyle görünür.
    pub home: Option<PathBuf>,
    /// Çocuğa **eklenen** ortam değişkenleri; geri kalanı miras.
    ///
    /// Öncelik, güçlüden zayıfa: `TERM`, `COLORTERM` ve kimlik ailesi
    /// (`TERM_PROGRAM`, `TERM_PROGRAM_VERSION`, `TERM_SESSION_ID`,
    /// `BATERI_TAB_URL`; bu crate'in yazdıkları, ezilemez — `TERM` bir
    /// sözleşme, bkz. `CLAUDE.md`) > bu harita >
    /// alacritty'nin koşulsuz yazdıkları (`USER`, `HOME`,
    /// `ALACRITTY_WINDOW_ID`, `WINDOWID`) > miras. Tek istisna alacritty'nin
    /// en sonda **sildiği** iki anahtar (`XDG_ACTIVATION_TOKEN`,
    /// `DESKTOP_STARTUP_ID`): haritada olsalar da çocuğa ulaşmaz. Kendi
    /// sürecimizin ortamı hiçbir hâlde değişmez (`tty::setup_env()`
    /// çağrılmaz).
    pub env: HashMap<String, String>,
    pub cols: u16,
    pub rows: u16,
    /// Bir hücrenin piksel boyutu; PTY'ye `TIOCSWINSZ` ile gider, grafik
    /// uygulamaları (sixel, kitty) bunu okur.
    pub cell_px: (u16, u16),
    /// Açılışın terminal seçenekleri; sonra [`Session::set_terminal_options`]
    /// değiştirir.
    pub terminal: TerminalOptions,
    /// Açılış teması; hangi temanın seçileceği uygulamanın kararı.
    pub theme: Theme,
    /// Bu pencerenin dock'u var mı — yani caret'i ızgaradan devralacak ikinci
    /// bir yüzey çizilecek mi.
    ///
    /// **İkinci bir kaynak değil, ikinci bir tüketici.** Kararı uygulama
    /// veriyor (entegrasyon kuruldu mu) ve aynı karar dock'un çizim payını da
    /// belirliyor; ikisi çağıranda **tek** ifadeden çıkıyor
    /// (`bt_shell::app`'in `birth`'ü). Burada ayrı bir alan olmasının sebebi
    /// katman yönü: pay `bt-gpu`'nun geometrisi, caret ise bu crate'in kare
    /// kararı ve `bt-core` yukarıyı göremiyor.
    ///
    /// `false` iken [`Session::frame`] imleci **hiçbir hâlde** gizlemiyor:
    /// dock'u olmayan pencerede caret'i devralacak kimse yok ve gizlemek
    /// pencereyi caret'siz bırakırdı. Duman reçetesi (`/bin/sh`) tam da bu kol.
    pub dock: bool,
    /// Emoji dizileri (`🇹🇷`, `👍🏽`, `👨‍👩‍👧`, `❤️`) tek hücrede kümelensin mi
    /// (035; kural `crate::cluster`'da).
    ///
    /// **Tek kaynak, bütün tüketicileri buradan:** ızgaranın sarmalayıcısı
    /// (okuyucu döngü), dock'un düzeni, bastırmanın ızgara yürüyüşü ve
    /// tazelik kapısının ayna yarısı. Açılışta bir kez okunuyor ve oturum
    /// boyunca sabit; ayrı ayrı açılabilseydi ızgara diziyi iki, dock dört
    /// sütun sayar ve bastırmanın aralığı ızgaradan ayrışırdı (032'nin "iki
    /// aritmetik" belirtisi). Uygulamanın her penceresi açıyor (`bt-shell`
    /// `window`); alan kalıyor, çünkü geri alma tek satır olmalı.
    pub cluster: bool,
    /// Kabuğa **ilk girdi** olarak yazılacak satır, `\r`'siz (037 Karar 6:
    /// uzak sekmede ⌘T aynı ssh/mosh komutunu yeni sekmede koşturuyor).
    /// `None` ya da boş satır → hiçbir şey yazılmıyor.
    ///
    /// Ne zaman yazılacağı [`SessionOptions::shell_marks`]'tan; yazım
    /// kullanıcı girdisinin yolundan (nesil ilerliyor, tazelik kapısı onu
    /// bir tuş gibi görüyor) ve **tek atımlık** — ikinci prompt'ta yeniden
    /// gitmiyor. Satırın kaçırılması çağıranın (`RemoteTarget::line`).
    pub initial_input: Option<String>,
    /// Kabuğa sarmalayıcımız kuruldu mu — yani kimliğimizi taşıyan OSC 133
    /// `A`'yı basacak mı. [`SessionOptions::dock`]'tan **ayrı**: `[shell]
    /// integration = "blocks"` sarmalayıcıyı kuruyor ama dock açmıyor.
    ///
    /// Tek tüketicisi [`SessionOptions::initial_input`]'un teslimi: `true`
    /// iken satır bizim **ilk kimlikli `A`'mızda** gidiyor (kabuk prompt'a
    /// vardı; rc dosyalarının okuduğu stdin'i — oh-my-zsh'in güncelleme
    /// sorusu — yemiyor), `false` iken doğumda, kabuğun typeahead'i olarak.
    /// **Bilinen sınır:** `true` ama kimlikli `A` hiç gelmiyorsa (bozuk rc,
    /// rc'nin sonunda `exec fish`) satır hiç gitmiyor; zaman aşımı ölçülmemiş
    /// bir sayı olurdu (037 Karar 6).
    pub shell_marks: bool,
    /// Sekmenin kimliği (038): verildiyse çocuk `TERM_SESSION_ID` ve
    /// `BATERI_TAB_URL` alır ([`TabId::url`]). `None` yalnız sınama ve
    /// gömülü kullanım; uygulama her pencerede veriyor (`bt-shell`
    /// `window`, `NSUUID`'den).
    pub tab_id: Option<TabId>,
}

/// Oturum yaşarken değişebilen terminal seçenekleri — alacritty `Config`'inin
/// **bizim kurduğumuz** alanları.
///
/// Ayrı bir tip, çünkü `Term::set_options` `Config`'in **tamamını** değiştirir
/// (alacritty `term/mod.rs:499-516`): tek bir alanı taşıyan bir çağrı ötekileri
/// varsayılana geri çekerdi ve geçmişin kırpılması geri dönülmez
/// (`grid/mod.rs:154-158`). Seçenekler bu yüzden hep **birlikte** gider ve
/// `Config`'e tek fonksiyonda (`term_config`) iner: `osc52` değişimi geçmişi
/// kırpmaz, `scrollback` değişimi OSC 52'yi açmaz.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalOptions {
    /// Geçmişte tutulan satır. Tavanı ayar ayrıştırıcısının kuralı
    /// (`settings::SCROLLBACK_MAX`); buraya ondan geçmiş değer gelir.
    pub scrollback: usize,
    /// Uygulamanın OSC 52 ile panoya yazıp yazamayacağı.
    pub osc52: Osc52,
    /// İmlecin **varsayılan** şekli; uygulamanın DECSCUSR'ı üstüne yazar.
    pub cursor: CaretShape,
    /// İmleç yanıp söner mi; `Auto` uygulamayı izler, ötekiler **ezer**.
    pub blink: CursorBlink,
}

/// [`CaretShape`]'i alacritty'nin şekline çevirir — `term_config`'in tek
/// kullanıcısı, ayrı bir `impl` hak etmiyor.
fn caret_shape(shape: CaretShape) -> CursorShape {
    match shape {
        CaretShape::Block => CursorShape::Block,
        CaretShape::Underline => CursorShape::Underline,
        CaretShape::Beam => CursorShape::Beam,
    }
}

/// Alacritty'nin şeklini sınırın şekline çevirir — `frame()`'in kullandığı yön.
///
/// `Hidden` ve `HollowBlock` **bloğa düşüyor** ve ikisi de adlandırılmış
/// karar: ilkini [`Cursor::visible`] zaten taşıyor (iki yerde temsil edilen
/// bir gerçek ayrışır), ikincisi odak kaybının hâli ve odak **hâlâ sınırdan
/// geçmiyor** — içi boş imleç 015 phase-3'te geldi ama `bt-gpu`'nun kendi
/// biti olarak (`DisplayLink::set_focused`). Buraya eklenmemesi bilerek:
/// `CaretShape` ayar dosyasının sözlüğü (`"block" | "underline" | "beam"`) ve
/// odak şekle **dik** bir eksen — ikisi tek enumda buluşsaydı "odaksız beam"
/// temsil edilemezdi. Alacritty'nin `HollowBlock`'u zaten ölü: bu kol onu
/// kendiliğinden üretmiyor.
fn caret_shape_of(shape: CursorShape) -> CaretShape {
    match shape {
        CursorShape::Underline => CaretShape::Underline,
        CursorShape::Beam => CaretShape::Beam,
        CursorShape::Block | CursorShape::HollowBlock | CursorShape::Hidden => CaretShape::Block,
    }
}

/// OSC 52'nin kipi: terminaldeki uygulama (ssh'taki vim de) panoya yazabilir mi.
///
/// alacritty'nin aynı adlı tipi yeniden ihraç edilmiyor (`lib.rs`) ve dört
/// değerinin ikisi burada **temsil edilemiyor**: okuma yönü yok (006 Karar 5
/// — uzaktaki bir program kullanıcının panosunu okuyamamalı). Varsayılan
/// değer bu tipin değil ayar modelinin kararı (`Settings::default`), tip
/// `Default` almıyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Osc52 {
    /// Dizi yoksayılır.
    Off,
    /// Yalnız yazma: dizi [`Wake::copy_to_clipboard`]'a gider.
    Copy,
}

impl Osc52 {
    /// Ayar dosyasındaki yazılışların tek listesi; ayrıştırıcısı
    /// `settings.rs`'te. Okuma yönünün değerleri (`"paste"`, alacritty'nin
    /// `"copy_paste"`'i) bilerek yok — okuma yönü yok (006 Karar 5).
    pub const NAMES: &'static [(&'static str, Self)] = &[("copy", Self::Copy), ("off", Self::Off)];

    /// Ayar dosyasındaki yazılışı.
    pub fn name(self) -> &'static str {
        crate::settings::name_in(Self::NAMES, self)
    }
}

/// alacritty `Config`'ini seçeneklerin **tamamından** kurar — açılışın
/// (`Session::spawn`) ve canlı değişimin ([`Session::set_terminal_options`])
/// tek yolu.
///
/// Kelimenin tanımı (`semantic_escape_chars`) seçeneklerden değil sabitten
/// ([`WORD_SEPARATORS`]); geri kalan alanlar (imleç biçimleri,
/// `kitty_keyboard`) alacritty'nin varsayılanında: onları hiçbir yer
/// kurmuyor, yani iki çağrı arasında da oynamıyorlar.
/// `term_config_keeps_every_other_field` bunu çiviliyor.
fn term_config(options: TerminalOptions) -> Config {
    Config {
        semantic_escape_chars: WORD_SEPARATORS.to_owned(),
        scrolling_history: options.scrollback,
        osc52: match options.osc52 {
            Osc52::Off => TermOsc52::Disabled,
            Osc52::Copy => TermOsc52::OnlyCopy,
        },
        // **Yalnız varsayılan.** Uygulamanın DECSCUSR'ı (`\e[5 q`) bunu
        // ezer ve ezmeli: vim insert modda çubuk istiyor.
        //
        // **`blinking` yalnız `Auto`'nun tabanı.** `"on"` ve `"off"` birer ezme
        // ve config'de temsil edilemiyorlar (`AdapterInner::blink`) — buraya
        // yazılan bir "hep sönsün" uygulamanın `\e[2 q`'suyla susturulurdu.
        //
        // Taban **`true`** ve bu kullanıcı bildirimiyle düzeldi (2026-09-19):
        // `false` iken `"auto"` düz bir promptta `"off"` ile **birebir aynıydı**
        // — ne zsh ne bizim betiğimiz DECSCUSR/DECSET 12 göndermiyor, yani
        // hiçbir şey blink istemiyordu ve üç değerden ikisi ayırt edilemiyordu.
        default_cursor_style: CursorStyle {
            shape: caret_shape(options.cursor),
            blinking: matches!(options.blink, CursorBlink::Auto),
        },
        ..Config::default()
    }
}

/// Hasarı işaretlemenin oturumdan bağımsız yolu; [`Session::dirty_flag`] verir.
///
/// `mark()` çağrılınca sıradaki `frame()` çizilecek bir kare döndürür.
/// **Kimseyi uyandırmaz:** uyandırmak çağıranın işi (kareyi isteyecek olan o).
///
/// **Durma koşulu çağıranındır ve zorunludur.** Kalıcı bir çizim hatası
/// "başarısız → bayrağı dik → yeniden dene" döngüsünü ekran tazeleme hızında
/// sonsuza çevirir; hata başına **tek** yeniden deneme, art arda ikinci hatada
/// kare talebi kesilir ve sıradaki `Wakeup` beklenir.
#[derive(Clone)]
pub struct DirtyFlag(Arc<AtomicBool>);

impl DirtyFlag {
    pub fn mark(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Duman koşusunun ve sınamaların ortak sabit shell'i.
///
/// Kullanıcının `$SHELL`'ine ve rc dosyasına bağlı olmayan bir komut: ilk
/// satıra **sekiz** kırmızı arka planlı hücre (`" bateri "`) basar, arkasına
/// **yedi** yalnız-kurallı hücre ekler, sonra uyur. Sekizin **altısında**
/// mürekkep var: iki ucu boşluk, ortası `bateri`; sekizi de kalın ve düz altı
/// çizili, yani font yüzü yolu da betikte.
///
/// Yedi kural hücresinin hepsi **boşluk ve varsayılan arka planlı**
/// (`bg: None`, `ch: None`): `hucre=8` ve `glif=6` bit bit duruyor, ama
/// yedisi de [`Session::frame`]'in atlama koşulunun kural yan tümcesinden
/// geçiyor — yani o yan tümce duman kapısından her koşuda geçmiş oluyor.
/// Sırası `Single, Double, Curl, Dotted, Dashed, Strikeout, Curl + SGR 58`.
///
/// **İki nokta yük taşıyor:** `\033[4;3m` ≠ `\033[4:3m`. Noktalı virgüllü
/// hâl `Underline + Italic`'tir (`[3] => Attr::Italic`), iki noktalı hâl
/// undercurl'dür (`[4, 3] => Attr::Undercurl`). Reçeteyi "sadeleştiren" biri
/// `:`'yı `;` yaparsa sınama sessizce düz-altı-çizili-eğik'e iner ve duman
/// yeşil kalır. Aradaki `\033[0;` de zorunlu: `Attr::Strike`
/// `ALL_UNDERLINES`'ı **kaldırmıyor**, sıfırlanmazsa o hücre kesikli **artı**
/// üstü çizili olur.
///
/// Uyku süresi duman koşusunun süresini (`BT_RUN_SECONDS`, varsayılan 3)
/// rahatça aşmalı: shell deadline'dan önce kendi kendine çıkarsa `ChildExit`
/// uygulamayı erken sonlandırır ve koşu ölçtüğü şeyi ölçmemiş olur. Üst sınır
/// artık yok — `run_deadline` çıkmadan önce `shutdown()` çağırıyor, yani
/// `SIGHUP` gidiyor ve artakalan çocuk uyku bitene kadar yaşamıyor.
///
/// Tek sahip olmasının sebebi sayıların kendisi: `make duman`'ın `hucre=8` ve
/// `glif=6` beklentisi ile `smoke_shell_yields_background_cells`,
/// `smoke_shell_yields_six_glyphs` ve `smoke_shell_distinguishes_five_styles`
/// sınamalarının 8'i, 6'sı ve 15'i aynı betiğe bağlı.
/// İki yerde ayrı yazılsalardı biri değişip diğeri sessizce eski kalırdı — ve
/// duman ikisini de yalnız "> 0" diye sorduğu için kimse fark etmezdi.
/// Bu hâliyle sınamalar, uygulamanın gerçekten koştuğu betiği doğruluyor.
///
/// Ölçüm yükü için [`load_shell`]: bu fonksiyon duman sayılarının sahibi
/// olduğu için ikinci bir yük **buraya eklenmez**. Süre parametresi, "bir de
/// şu kadar satır bas" — hiçbiri; her biri sekiz hücreyi, altı glyph'i ya da
/// on beş kuralı oynatır ve oynattığında üç sınama ile `make duman` aynı anda
/// ama ayrı ayrı yalan söyler.
///
/// **İkinci `printf` bir istisnadır ve tam olarak bir şey yapar: imleci
/// kıpırdatır.** `\033[2G` hiçbir hücre yazmıyor — sekiz arka plan, altı glyph
/// ve on beş kural bit bit yerinde — ama imleci `\n`'in bıraktığı satırın
/// başından aynı satırın ikinci sütununa alıyor, yani her koşuda **bir**
/// imleç hareketi doğuyor. Kapının `hareket > 0` gerekliliği buna dayanıyor
/// (008 Karar 8): onsuz imleç yalnız çıktının kendisiyle oynardı ve ilk
/// karenin çıktıdan önce mi sonra mı düştüğü koşudan koşuya değişiyor
/// (`kare=1↔2`), yani kapı animasyonun koştuğunu göremeyebilirdi.
///
/// **Hareket saf yatay ve bu bir sözleşme** (011 phase-0). Eskiden `\033[H`
/// idi, yani saf **dikey**: imleci (satır 1, sütun 0)'dan (0, 0)'a alıyordu.
/// İçerik tabana yapıştıktan sonra o hedef **`hareket=`'i sıfırlıyor**: ofset
/// `max(çizilen en büyük satır, imleç satırı)`'dan doğduğu için imleci yukarı
/// taşımak ofseti aynı miktarda aşağı kaydırıyor ve imlecin **ekran** satırı
/// değişmeden kalıyor — `Motion::sync`'in hedefi tam olarak o satır. Ekran
/// büsbütün hareketsiz **değil** (doluluk daraldığı için öteleme bir satır
/// kayar, `kayma=`), ama kapının gereklilik saydığı sayaç imlecinki ve o 0'a
/// düşerdi. Satır sabit kalmak **zorunda**; değiştiren bir hedef jetonu
/// sessizce sıfırlar.
///
/// **Mesafe de sözleşmenin parçası: tam bir hücre.** Eski `\033[H` bir satır
/// taşıyordu; yatay karşılığı bir **sütun** olmak zorunda, çünkü yay uzak
/// sıçramayı daha uzun uçuruyor ve yerleşme süresi `sessiz=` jetonunun
/// kuyruğundan yiyor. Üç sütunla (`\033[4G`) ölçüldüğünde taban
/// `QUIET_FLOOR`'un **kendi türetme kuralını** ihlal eder hâle geliyordu ("en
/// düşük sağlıklı gözlemin en çok yarısı") — kapı o koşuda hâlâ yeşildi, yani
/// kusur jetonun arkasında saklanıyordu. Sayıların ve türetmenin sahibi
/// `docs/OLCUMLER.md` → Boşta kare; mesafeyi büyüten biri `QUIET_FLOOR`'u
/// oradan yeniden türetmek zorunda ve reçetenin mesafesi o bloğun **dördüncü**
/// bağlı girdisidir.
///
/// **Aradaki uyku cömert (1 s) ve bu bir pay değil, kapının şartı.** Açılış
/// süresi (`acilis=`) ölçülmedi; `\033[2G` ilk içerik karesinden **önce**
/// işlenirse imleç zaten hedefte doğar, hareket hiç başlamaz ve `hareket > 0`
/// kod doğruyken kırmızı düşer. Uykuyu kısaltmak bu yarışı geri getirir.
/// Üç saniyelik koşuda 1 s uyku + yerleşme `sessiz=`'e rahat bir kuyruk
/// bırakıyor.
pub fn smoke_shell() -> (String, Vec<String>) {
    (
        "/bin/sh".to_owned(),
        // Kaçışları printf çözer: Rust dizgisinde `\033` ilk baytı NUL yapardı.
        vec![
            "-c".to_owned(),
            "printf '\\033[41;1;4m bateri \\033[0m\\033[4m \\033[0;4:2m \\033[0;4:3m \
             \\033[0;4:4m \\033[0;4:5m \\033[0;9m \\033[0;4:3;58;5;196m \\033[0m\\n'; \
             sleep 1; printf '\\033[2G'; sleep 10"
                .to_owned(),
        ],
    )
}

/// Ölçüm yükü: `secs` saniye boyunca kesintisiz çıktı akıtır.
///
/// [`smoke_shell`]'den **ayrı** ve öyle kalmalı — o, `hucre=8 glif=6
/// kural=15` sayılarının tek sahibi ve üç sınama o sayılara bağlı. Buradaki
/// komut değişince duman sayıları oynamaz; oynarsa ayrım kaybolmuş demektir.
///
/// Kaydırılan şey viewport değil **içerik**: yük pencereyi geriye almaz
/// ([`Session::scroll_wheel`] tekerleğin yolu, bu koşuda kimse çevirmiyor),
/// yani her satır kirli düşer, grid yukarı kayar, kare akışı kendiliğinden
/// sürer. Ölçtüğümüz şey zaten bu — dolu bir karede parse +
/// [`Session::frame`] + encode + GPU maliyeti.
pub fn load_shell(secs: u64) -> (String, Vec<String>) {
    // POSIX `$((...))` işaretli `intmax_t`: `u64::MAX` kabukta `-1`'e sarıyor
    // ve `end` **geçmişte** kalıyor, yani yük hiç koşmadan biter. Ölçüldü (bu
    // makine): `$(( now + 18446744073709551615 ))` → `now - 1`. Kırpma o
    // sessiz dalı kapatıyor; bu aralığın üstündeki bir "ölçüm süresi" zaten
    // bir kullanım hatası.
    let secs = secs.min(u64::from(u32::MAX));
    (
        "/bin/sh".to_owned(),
        vec![
            "-c".to_owned(),
            // `seq` değil `while`: sabit satır sayısı makineye göre ya erken
            // biter ya da hiç bitmez. Süre kapısı deterministik.
            //
            // **İç döngü şart, süs değil.** Saati her satırda sormak (`while
            // [ $(date +%s) -lt $end ]` doğrudan `printf`'i sarmalasa) satır
            // başına bir komut ikamesi + bir `date` `exec`'i demek: ölçülen
            // şey renderer değil `fork` gecikmesi olur. Ölçüldü (2 saniye,
            // bu makine): saat her satırda → **458** satır, 256'lık öbekler
            // hâlinde → **119 296** satır, yani ~260 kat. Yükün işi PTY'yi
            // doyurmak; doyuramayan yük dolu kareyi hiç göstermez.
            //
            // Saat yine de `date`: `$SECONDS` bash'in eklentisi ve `/bin/sh`
            // Linux'ta dash olur — `bt-core` platformsuz kalmak zorunda
            // (Vulkan kapısı), yani betik de POSIX kalır.
            //
            // **`+ 1` şart.** `date +%s` saniye çözünürlüklü: ilk örnekleme
            // saniyenin sonunda düşerse döngü `secs` değil `secs - 0.99`
            // sonra biter. Ölçüldü (bu makine, `secs = 3`): 2,4 / 2,9 / 2,9
            // saniye — yani çocuk `run_deadline`'dan **önce** ölüyor,
            // `ChildExit` koşuyu erken kapatıyor ve rapor yine "3 saniyelik
            // koşuda" diyor. `+ 1` ile 3,9 saniye, deadline rahatça içeride.
            // Aynı gerekçe `smoke_shell`'in `sleep 10`'unda da yazılı;
            // artakalan çocuğu deadline'ın `SIGHUP`'ı zaten kesiyor.
            //
            // `printf '%s\n' 'metin'` değil `printf 'metin\n'`: yük metninde
            // `%` de `\` de yok, ikinci argüman boşuna.
            format!(
                "end=$(($(date +%s) + {secs} + 1)); \
                 while [ $(date +%s) -lt $end ]; do \
                   n=0; \
                   while [ $n -lt 256 ]; do \
                     printf 'bateri olcum yuku 0123456789 abcdefghijklmnopqrstuvwxyz\\n'; \
                     n=$((n + 1)); \
                   done; \
                 done"
            ),
        ],
    )
}

/// `Term` boyutu `Dimensions` ister.
///
/// alacritty'nin `TermSize`'ı bu işi görürdü ve `#[cfg(test)]` ile kapalı da
/// değil — ama `term::test` modülünde, yani adı "sınama yardımcısı" diyor.
/// Ürün yolunda o adı taşımak yerine üç satırı kendimiz yazıyoruz.
///
/// `total_lines` sözleşme gereği "scrollback dahil toplam satır"dır, burada
/// `rows` dönüyor: alacritty dışarıdan geçirilen `Dimensions`'tan yalnız
/// `screen_lines` ve `columns` okuyor (scrollback'i `Config` veriyor).
/// Okumaya başlarsa scrollback sessizce sıfırlanır — o gün burası da değişir.
#[derive(Clone, Copy)]
struct GridSize {
    cols: usize,
    rows: usize,
}

impl GridSize {
    /// Sıfır sütun ya da satır alacritty'de taşmadır:
    /// `Dimensions::last_column()` = `Column(columns() - 1)`, `usize`'ta
    /// `0 - 1`. Açılışta grid'in var olması gerektiği için taban 1×1'de
    /// kesilir; henüz kaybedilecek geçmiş yok.
    ///
    /// **`resize` bu kurucuyu kullanmaz.** Var olan bir grid'i 1 sütuna
    /// çekmek yıkıcıdır: alacritty her sarmalı satırı tek sütuna açar ve
    /// `reversed.truncate(max_scroll_limit + lines)` ile geçmişin neredeyse
    /// tamamını atar; 80 sütuna dönmek onu geri getirmez. Dejenere boyut
    /// kırpılmaz, yoksayılır.
    fn for_spawn(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols.max(1) as usize,
            rows: rows.max(1) as usize,
        }
    }

    /// Kırpmadan. Çağıran dejenere boyutu zaten elemiş olmalı;
    /// `for_spawn`'nin karşılığıdır ve `resize` bunu kullanır.
    fn exact(cols: u16, rows: u16) -> Self {
        Self {
            cols: cols as usize,
            rows: rows as usize,
        }
    }
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

/// alacritty'nin `EventListener`'ı ile bizim `Wake`'imiz arasındaki adapter.
///
/// `Term` ve `EventLoop` ayrı ayrı birer kopya ister; ikisi de aynı `Arc`
/// gövdesini paylaşsın diye tip `Clone`'dur.
#[derive(Clone)]
struct Adapter(Arc<AdapterInner>);

struct AdapterInner {
    wake: Arc<dyn Wake>,
    /// `EventLoop`'un kanalı ancak `EventLoop::new`'dan sonra doğar, oysa
    /// adapter `Term::new`'dan önce gerekir; kanal bu yüzden sonradan takılır.
    sender: OnceLock<EventLoopSender>,
    /// Grid'de tüketilmemiş yeni içerik var mı.
    ///
    /// Kapı neden `Term::damage()` değil: alacritty her `damage()` çağrısında
    /// imleci koşulsuz kirletir (`damage_cursor`), yani "hasar yok" cevabı
    /// hiçbir zaman gelmez. Boşta sıfır kare ondan okunamaz. Bayrağı
    /// `Event::Wakeup` diker — alacritty'nin "yeni içerik var" sinyali odur.
    ///
    /// **Bilinen sınır:** bayrak pencereyi bilmiyor. Geçmişe kaydırılmış bir
    /// pencerede alacritty görünen satırları yeni çıktıya karşı sabitliyor
    /// (`grid.scroll_up` ofseti artırıyor), yani akan çıktı ekranda hiçbir şey
    /// değiştirmediği hâlde her `Wakeup` bir kare ister. Boşta değil (çıktı
    /// akıyor) ve kaydırma gelene kadar ulaşılamazdı; çaresi pencereye duyarlı
    /// hasar, bu setin işi değil.
    dirty: Arc<AtomicBool>,
    /// PTY'nin bildiği son boyut; `TextAreaSizeRequest` bunu yanıtlar.
    size: Mutex<WindowSize>,
    /// Paletin tek kaynağı; [`Session::theme`] okur.
    ///
    /// **Yaprak kilit** (`size` emsali): tutulurken başka kilit alınmaz ve
    /// tutan taraf yalnız kopyalar — `Theme` `Copy`, 80 bayt. İki okuyanı
    /// var: `frame()` kopyayı `Term` kilidinden **önce** alır (kilidin
    /// altında renk çözümüne ikinci bir muteks sokmasın), renk sorusu `Term`
    /// kilidi tutulurken alır. Biri ötekini içeriden hiç tutmadığı için sıra
    /// çakışmaz. Mutex, çünkü tema takas edilebilir; yazanı
    /// [`Session::set_theme`], o da kilidi tek başına alır.
    theme: Mutex<Theme>,
    /// İmlecin yanıp sönme ayarı — **yaprak kilit**, temanın komşusu.
    ///
    /// Neden `Term`'ün `Config`'inde değil: `"on"`/`"off"` birer **ezme** ve
    /// alacritty onu ifade edemiyor — uygulamanın `cursor_style`'ı
    /// `default_cursor_style`'ı her zaman yeniyor (`Term::cursor_style`), yani
    /// config'e yazılan bir "hep sönsün" `\e[2 q` ile susturulurdu. Karar
    /// `frame()`'de, `cursor_style()` okunduktan **sonra** uygulanıyor
    /// ([`CursorBlink::resolve`]).
    ///
    /// Kopya `Term` kilidinden **önce** alınıyor, temanınkiyle aynı turda ve
    /// aynı gerekçeyle: yaprak kilit `Term`'ün altına girmez.
    blink: Mutex<CursorBlink>,
    /// Uygulamanın OSC 0/2 başlığı; `None` → hiç gelmedi ya da
    /// `ResetTitle`. Okuyan [`Session::title`].
    ///
    /// **Yaprak kilit** (`theme` emsali). Yazanı `Title`/`ResetTitle` kolu ve
    /// o kol `Term` kilidi **tutulurken** geliyor — `ColorRequest`'in temayı
    /// orada alması gibi: sıra `Term` → yaprak, tersi hiçbir yerde yok.
    /// [`Session::title`] onu `Term`'e dokunmadan, tek başına alıyor.
    title: Mutex<Option<String>>,
    /// PTY çıktısının **nesli**: alacritty'nin her `Wakeup`'ı (ayrıştırılmış
    /// bir okuma turu) bir artırıyor, `Term` kilidi altında. Ekranı
    /// temizlemek defteri değiştiriyor ama bunu artırmıyor — onun nesli
    /// ayrı ([`AdapterInner::wipes`]). Geçerli arama
    /// eşleşmesinin kaymasının "arada çıktı var mı" sorusu
    /// ([`search::ledger_shift`]); oturumun kendi kare isteği
    /// ([`Session::request_frame`]) onu oynatmıyor.
    ledger: AtomicU64,
    /// Ekranı temizlemenin nesli (034): [`Session::clear_to_start`] ve
    /// [`Session::clear_scrollback`] bir artırıyor, `Term` kilidi altında.
    /// Geçerli arama eşleşmesinin "arada temizlik var mı" sorusu
    /// ([`search::LedgerMark::wipes`]) — `ledger`'dan ayrı, çünkü doymamış
    /// defterde `ledger` okunmuyor.
    wipes: AtomicU64,
    /// Arama açık mı (033) — [`Session::store_search`] yazıyor; defter
    /// haberinin kapısı.
    search_active: AtomicBool,
    /// Defter, dizinin son geçişinden beri değişti mi — **kenar**: `false →
    /// true` geçişi [`Wake::search_changed`] doğuruyor, tüketeni
    /// [`Session::search_step`] (bir sonraki geçişi baştan başlatarak).
    search_pending: AtomicBool,
}

impl Adapter {
    fn new(wake: Arc<dyn Wake>, size: WindowSize, theme: Theme, blink: CursorBlink) -> Self {
        Self(Arc::new(AdapterInner {
            wake,
            sender: OnceLock::new(),
            // Açılış karesi: pencere ilk kez boyansın diye kirli başlar.
            dirty: Arc::new(AtomicBool::new(true)),
            size: Mutex::new(size),
            theme: Mutex::new(theme),
            blink: Mutex::new(blink),
            title: Mutex::new(None),
            ledger: AtomicU64::new(0),
            wipes: AtomicU64::new(0),
            search_active: AtomicBool::new(false),
            search_pending: AtomicBool::new(false),
        }))
    }

    /// Kirli bayrağını diker ve uyandırır. Sıra önemli: bayrak uyandırmadan
    /// ÖNCE dikilir — ters sırada uyanan taraf bayrağı henüz görmeden bakar
    /// ve kare kaçar.
    fn wake_frame(&self) {
        self.0.dirty.store(true, Ordering::Release);
        self.0.wake.wake();
    }

    /// Arama açıksa defterin değiştiğini **kenarda** haber verir: bekleyen
    /// bir haber varsa ikincisi yok ([`AdapterInner::search_pending`]).
    fn search_changed(&self) {
        if self.0.search_active.load(Ordering::Acquire)
            && !self.0.search_pending.swap(true, Ordering::AcqRel)
        {
            self.0.wake.search_changed();
        }
    }

    /// Uygulamanın sorduğuna PTY'den yanıt verir. Kanala yazmak kilitsizdir;
    /// `Term` kilidi tutulurken çağrılmak serbesttir.
    fn reply(&self, text: String) {
        self.input(text.into_bytes());
    }
}

impl Adapter {
    /// Baytları döngünün yazma kuyruğuna koyar — uygulamanın sorusuna yanıt
    /// ([`Self::reply`]) ve oturumun ilk girdisi (037 Karar 6) buradan.
    fn input(&self, bytes: Vec<u8>) {
        // Sıfır baytlık yazma `EventLoop`'un yazıcısını kilitler: `write`
        // `Ok(0)` döner, öge kuyruğun başına geri konur ve bir daha hiç
        // emilmez — poller seviye tetiklemeli olduğu için thread de %100'de
        // döner. alacritty kendi `Notifier`'ında aynı korumayı taşıyor.
        if bytes.is_empty() {
            return;
        }
        if let Some(sender) = self.0.sender.get() {
            // Kanal yalnız kapanışta ölür; o yolda sessiz kalmak doğrudur.
            let _ = sender.send(Msg::Input(bytes.into()));
        }
    }

    /// Başlık yuvasını yazar; **değiştiyse** yaprak kilidi bıraktıktan sonra
    /// [`Wake::title_changed`]. `Term` kilidi tutulurken çağrılır.
    fn store_title(&self, title: Option<String>) {
        let changed = {
            let mut slot = lock(&self.0.title);
            if *slot == title {
                false
            } else {
                *slot = title;
                true
            }
        };
        if changed {
            self.0.wake.title_changed();
        }
    }
}

impl EventListener for Adapter {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                // Çıktının nesli ve arama haberi `Term` kilidi altında
                // (`pty_read` ayrıştırdıktan sonra kilidi tutarken yolluyor):
                // nesli okuyan gözlem onu içerikle aynı turda görüyor.
                self.0.ledger.fetch_add(1, Ordering::AcqRel);
                self.search_changed();
                self.wake_frame();
            }
            Event::ChildExit(status) => self.0.wake.child_exit(status.code()),
            Event::PtyWrite(text) => self.reply(text),
            // Renk sorusu `Term` kilidi tutulurken gelir; tabloyu okumak için
            // kilidi geri istemek kilitlenme olurdu (kilit yeniden girilebilir
            // değil, `try_lock` da aynı thread'de hep düşer). Temanın
            // paletiyle yanıtlıyoruz — tema yaprak kilitte, alması serbest.
            //
            // **Bilinen sınır:** uygulama OSC 4/10/11 ile bir rengi
            // değiştirip sonra sorarsa eski değeri alır — "önce ata, sonra
            // sor" yaygın bir örüntüdür (arka planı okuyup açık/koyu tema
            // seçen editörler). Çizim yolu tabloyu doğru okuyor, yalnız
            // yanıt yolu okumuyor; ikisi ayrışıyor. Kökü temada değil
            // alacritty'de: uygulamanın yazdığı `Colors` tablosu `Term`'ün
            // içinde, yani bu kilidin arkasında. Tema seti (007) sınırı
            // kapsam dışı bıraktı; çaresi tablonun `Term` dışına bir kopyası.
            Event::ColorRequest(index, format) => {
                let theme = *lock(&self.0.theme);
                self.reply(format(theme.default(index)));
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *lock(&self.0.size);
                self.reply(format(size));
            }
            // OSC 52 yazma yönü. Olay yalnız `Osc52::Copy`'de doğuyor (kapıyı
            // alacritty `Config` üzerinden tutuyor, `term_config`), `Term`
            // kilidi tutulurken geliyor — `Wake` sözleşmesi kolu taşıyor.
            //
            // **Hedef ayrımı yok:** `c` de `p`/`s` (birincil seçim) de genel
            // panoya gidiyor. macOS'ta tek pano var ve vim'in `*` ile `+`
            // kaydı orada aynı panodur; Neovim'in OSC 52 sağlayıcısı ise `*`'ı
            // `p` diye yolluyor. `clipboard=unnamed`'lı (macOS dotfile'larının
            // olağanı) bir kullanıcının ssh'taki kopyası `p`'yi düşürseydik
            // sessizce kaybolurdu. (alacritty macOS'ta seçimi düşürüyor; burada
            // bilerek ayrılıyoruz.)
            //
            // Boş metin iletilmez: `\e]52;c;\a` xterm'de panoyu **temizler**
            // ve uzaktaki bir programın kullanıcının panosunu silmesi bir
            // yazma değil. Pano köprüsünün kendi kapısı da boş yazmayı
            // reddediyor; burada elenmesi, köprünün son-yazma-kazanır
            // yuvasında önceki gerçek metnin boş bir metinle ezilmemesi için.
            //
            // **Bilinen sınır:** metnin boyu sınırsız. vte'nin OSC tamponu
            // `std`'de tavansız ve çözme bu değişiklikten önce de `Term`
            // kilidi altında yapılıyordu (alacritty'nin varsayılanı
            // `OnlyCopy`'ydi, olay burada düşüyordu); eklenen maliyet, panoya
            // yazmanın ana thread'de metnin boyuyla uzaması — çok büyük bir
            // kopyada pencere yazma bitene kadar kare çizemez, eşiği
            // ölçülmedi. Bir tavan seçilmiş bir sayı ister; sel gibi çıktı
            // basan bir program pencereyi zaten meşgul edebiliyor.
            Event::ClipboardStore(_, text) => {
                if !text.is_empty() {
                    self.0.wake.copy_to_clipboard(text);
                }
            }
            // Başlık: OSC 0/2 (ve başlık yığınının `CSI 23 t`'si) `Title`,
            // boşaltan yol `ResetTitle`. `Term` kilidi tutulurken geliyor;
            // yaprak kilit o yüzden yalnız karşılaştırıp yazmak için ve
            // haber kilit **bırakıldıktan sonra**. `Term::set_options` her
            // çağrıda güncel başlığı yeniden yolluyor (ayar kaydı): değişmeyen
            // başlık haber doğurmaz, yoksa her ayar kaydı pencere başına bir
            // ana kuyruk işi olurdu.
            //
            // **Bilinen sınır:** RIS (`\ec`) alacritty'nin başlığını olaysız
            // siliyor (`Term::reset_state`), yani yuva bir sonraki
            // `set_options`'a ya da OSC 0/2'ye kadar eski başlığı tutuyor.
            Event::Title(title) => self.store_title(Some(title)),
            Event::ResetTitle => self.store_title(None),
            // Zil bu sette yok; bilinmeyen dizi gibi sessizce
            // düşer (`CLAUDE.md` → PTY yolunda panik yok). "Yoksayılır ve
            // LOGLANIR" kuralının ikinci yarısı borç: `tracing` henüz
            // bağımlılık değil, workspace'te hiçbir logger yok — alacritty'nin
            // kendi `log::error!` satırları da bu yüzden yere düşüyor.
            //
            // `ClipboardLoad` OSC 52'nin okuma yönü ve yok (006 Karar 5):
            // `Osc52::OnlyCopy` onu zaten üretmiyor, kol bir sürüm değişikliğine
            // karşı boş.
            //
            // `MouseCursorDirty` `Term::scroll_display`'in **tek** olayı ve
            // burada yutuluyor: kaydırmanın karesini o değil
            // `Session::scroll_wheel` elle istiyor. Bu kola `dirty` dikmek
            // yanlış çare olurdu — olay kaymayan bir kaydırmada da (geçmişin
            // ucunda) ve fare raporlama kipinin her değişiminde (DECSET
            // 1000/1002/1003) gönderiliyor, yani boş kare doğururdu.
            Event::Bell
            | Event::ClipboardLoad(..)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Exit => {}
        }
    }
}

/// Okuma yolundan geçen baytları tarayan `Pty`.
///
/// `EventLoop` ([`crate::reader`]) PTY tipinde jenerik; **bayt** yoluna
/// araya giren tek şey bu sarmalayıcı ve **baytlara dokunmuyor** — [`io::Read::read`] içerideki `Pty`'den ne
/// okuduysa aynen döndürüyor, yalnız dönmeden önce dilimi tarayıcıya
/// gösteriyor. Ayrıştırıcı bu yüzden bugünküyle birebir aynı akışı görüyor.
/// Ayrıştırıcının **çağrı** yoluna giren ikincisi ayrı bir katman
/// ([`crate::handler::ClusterHandler`]): baytları değil `Term`'e giden
/// `Handler` çağrılarını görüyor, yani ikisi birbirinin işini görmüyor.
///
/// **`Reader = Self` kararın çekirdeği.** `Pty::reader()` `&mut File`
/// döndürüyor, yani okuyucuyu devretmek için ödünç yetiyor: taramak için
/// **ikinci bir fd'ye gerek yok**. Panelin "`pty.file().try_clone()` zorunlu"
/// itirazı bu yüzden reddedildi (`discussion.md` → Muhakeme) ve kazancı,
/// ölçülmüş kapanış dengesinin (`SIGHUP` penceresi, `kapanis=`) bu phase'de
/// gerçekten **dokunulmamış** kalması. Sarmalayıcı `Pty`'yi sahipleniyor, yani
/// `Drop`'un sırası da aynı.
///
/// Bu, [`Session::shutdown`]'ın borç olarak kaydettiği `try_clone`'u ne
/// çürütür ne engeller: oradaki dup **master'ı `wait` bloklarken boşaltmak**
/// için, okumayı devretmek için değil, ve yeri yine `Session::spawn` —
/// içerideki `pty` sarmalanmadan önce. İki iş ayrı; ikisini tek cümlede
/// karıştıran bir okuma borcu kapanmış sanır.
///
/// `pub` değil: katman kuralı gereği dışarıya alacritty tipi sızmaz.
struct TappedPty {
    pty: Pty,
    scanner: Scanner,
    /// [`Session::shell`]'in aynı yuvası. Yazan **yalnız** burası (okuyucu
    /// thread'i), okuyan [`Session::shell_state`].
    shell: Arc<Mutex<ShellLog>>,
    /// [`Session::screen_clears`]'in aynı yuvası: tarayıcının saydığı
    /// `CSI 2 J`. Burası **okuyucu thread'inin** yazarı; ikinci yazar ana
    /// thread'de terminal tarafı temizlik ([`Session::note_screen_clear`]).
    /// İkisi de yalnız artırıyor, yani sıraları önemsiz: sayaç bir nesil,
    /// okuyanı yalnız "değişti mi" diye soruyor.
    screen_clears: Arc<AtomicU32>,
    /// [`Session::key_gen`]'in aynı yuvası. Burası yalnız **okuyor**: ayna
    /// olayı çözüldüğü anda nesli damga olarak deftere geçiriyor.
    key_gen: Arc<AtomicU64>,
    /// `Adapter`'ın taşıdığı uyandırma ucunun kopyası: OSC 7 dizini
    /// **değişince** başlık haberi buradan gidiyor ([`Wake::title_changed`]).
    wake: Arc<dyn Wake>,
    /// Oturumun ilk girdisi, bizim ilk kimlikli `A`'mızı bekliyor
    /// ([`SessionOptions::shell_marks`]); `take` tek atımlığı veriyor.
    /// Doğumda yazılan kolda (sarmalayıcısız) burası hep `None`.
    initial_input: Option<String>,
    /// [`Session::held_input`]'in aynı yuvası: ilk girdi giderken tutulan
    /// baytlar **arkasından**, aynı kilit turunda ve aynı gönderimde gidiyor.
    held_input: HeldInput,
    /// Oturumun `Adapter`'ı — ilk girdi onun kanalından ([`Adapter::reply`])
    /// gidiyor, PTY'ye doğrudan değil: `read` `Term` kilidi tutulurken de
    /// koşabiliyor (`reader::EventLoop::pty_read` kilidi okumalar boyunca
    /// tutuyor) ve kanal kilitsiz. Baytları PTY'ye döngünün kendi yazma
    /// kuyruğu yazıyor.
    adapter: Adapter,
}

impl io::Read for TappedPty {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let read = self.pty.reader().read(buf)?;
        // Yalnız **bu turda** okunan dilim taranır; `EventLoop` tamponu
        // biriktirerek okuyor (`buf[unprocessed..]`) ve baştan taramak aynı
        // baytı iki kez işarete çevirirdi.
        //
        // Kilit yalnız işaret çıkınca alınıyor: olağan akışta closure hiç
        // çağrılmıyor, yani kabuk çıktısının hızlı yolu kilitsiz.
        //
        // Girdi nesli kilitten **önce** okunuyor ve bu yeterli: `send_input`
        // nesli baytları göndermeden önce artırıyor, yani bir tuşun aynası
        // buraya ulaştığında o tuşun nesli çoktan görünür. Kilit altında
        // okumak bir şey kazandırmazdı — neslin yazarı defterin kilidini
        // hiç almıyor.
        //
        // Haberler defterin kilidi **bırakıldıktan sonra**: `let`'in sonunda
        // guard düşüyor. Başlığınki yalnız başlığın girdisi değişince (farklı
        // dizin, uzak durumun silinmesi), komutunki yalnız `Running`'e
        // geçişte (`apply_scan_answering`'in dönüşü).
        //
        // İlk girdi de kilit bırakıldıktan sonra ve nesil **gönderimden
        // önce** ([`Session::key_gen`]'in doc'u; `send_input`'un sırası):
        // kabuğun bu girdiye cevabı olan ayna taze sayılmalı.
        let key_gen = &self.key_gen;
        let wake = &self.wake;
        let initial_input = &mut self.initial_input;
        let held_input = &self.held_input;
        let adapter = &self.adapter;
        self.scanner.feed(&buf[..read], |event| {
            let answers = key_gen.load(Ordering::Acquire);
            let outcome = lock(&self.shell).apply_scan_answering(event, answers);
            if outcome.title {
                wake.title_changed();
            }
            if outcome.started {
                wake.command_started();
            }
            if outcome.prompt
                && let Some(mut line) = initial_input.take()
            {
                line.push('\r');
                let mut bytes = line.into_bytes();
                // Tutulan girdi satırın **arkasında** ve gönderim yuvanın
                // kilidi altında: `send_input` yuvayı boş görünce gönderiyor,
                // yani ondan sonraki her tuş kanala bu satırdan sonra giriyor.
                let mut held = lock(held_input);
                if let Some(typed) = held.take() {
                    bytes.extend_from_slice(&typed);
                }
                key_gen.fetch_add(1, Ordering::Release);
                adapter.input(bytes);
            }
        });
        // **CSI kolu kilide hiç uğramıyor**: yükü yok, tüketicisi bir sayaç.
        //
        // Artış `advance`'ten **önce** oluyor ve bu bir kusur değil, kararın
        // kendisi: `EventLoop::pty_read` önce okuyor (yani burası koşuyor),
        // sonra `Term` kilidini alıp uyguluyor. Kare yolu sayacı **o kilidin
        // altında** okuduğu için "sayaç ilerledi ama baytlar henüz
        // uygulanmadı" hâlini görebiliyor ve orada bayrağı **kurmak** için
        // kullanıyor — düşürmek için değil ([`Session::observe_screen_clear`]).
        let clears = self.scanner.take_screen_clears();
        if clears > 0 {
            self.screen_clears.fetch_add(clears, Ordering::Relaxed);
        }
        Ok(read)
    }
}

impl tty::EventedReadWrite for TappedPty {
    type Reader = Self;
    type Writer = File;

    unsafe fn register(
        &mut self,
        poll: &Arc<Poller>,
        interest: PollingEvent,
        poll_opts: PollMode,
    ) -> io::Result<()> {
        // fd kaydı içerideki `Pty`'nin: hazır olma sinyali, token'lar ve çocuk
        // olayının boru hattı dokunulmadan kalıyor.
        //
        // SAFETY: trait'in koşulu "kaynaklar kayıtlarını **aşmalı**".
        // Kaydedilen fd'lerin sahibi `self.pty` ve onun sahibi de `self`:
        // sarmalayıcı `Pty`'yi **değer olarak** taşıyor, ödünç almıyor. İkisi
        // birlikte `EventLoop`'a taşınıyor ve `deregister` de aynı `self`
        // üzerinden geçtiği için kayıt, kaynağın düşmesinden önce kalkıyor.
        // `TappedPty`'nin `Pty`'yi geri verdiği bir yol yok.
        unsafe { self.pty.register(poll, interest, poll_opts) }
    }

    fn reregister(
        &mut self,
        poll: &Arc<Poller>,
        interest: PollingEvent,
        poll_opts: PollMode,
    ) -> io::Result<()> {
        self.pty.reregister(poll, interest, poll_opts)
    }

    fn deregister(&mut self, poll: &Arc<Poller>) -> io::Result<()> {
        self.pty.deregister(poll)
    }

    fn reader(&mut self) -> &mut Self {
        self
    }

    fn writer(&mut self) -> &mut File {
        self.pty.writer()
    }
}

impl tty::EventedPty for TappedPty {
    fn next_child_event(&mut self) -> Option<tty::ChildEvent> {
        self.pty.next_child_event()
    }
}

impl OnResize for TappedPty {
    fn on_resize(&mut self, window_size: WindowSize) {
        self.pty.on_resize(window_size);
    }
}

/// İlk girdi giderken tutulan kullanıcı girdisi (037 phase-4): `Some` →
/// tutuluyor, `None` → tutma yok ya da bitti. Yazarları iki thread —
/// kullanıcı girdisi ana thread'de ([`Session::send_or_hold`]), teslim okuyucu
/// thread'inde (`TappedPty::read`) — ve ikisi de aynı kanala **kilidin
/// altında** yazıyor; yaprak kilit, `Term`'e dokunmuyor.
type HeldInput = Arc<Mutex<Option<Vec<u8>>>>;

/// Tutulanı gönderen baytlar ([`Session::send_or_hold`]): satırı sonlandıran
/// (`\r` Enter, `\n` yapıştırma) ve kesen (`^C`). Kullanıcının "şimdi"
/// dediği tuşlar — kimlikli `A` hiç gelmezse klavyeyi canlı tutan kenar.
const RELEASES_HOLD: [u8; 3] = [b'\r', b'\n', 0x03];

/// Okuyucu thread'in tutamağı. `join()` döngüyü ve PTY'yi geri verir;
/// `SIGHUP` bu ikilinin düşmesiyle gider.
type Reader = JoinHandle<(EventLoop<TappedPty, Adapter>, State)>;

/// [`Session::shutdown`]'ın çocuğa tanıdığı süre.
///
/// Sayı **ölçümden** geliyor ve ölçüm iki kutup gösteriyor, arası yok
/// (`BT_SCROLL_TEST=1 BT_RUN_SECONDS=2`, sekiz koşu, debug, bu makine):
/// kapanış ya **0,44–0,70 ms**'de bitiyor (dört koşu) ya da hiç bitmiyor
/// (dört koşu — çocuk çıkışın içinde takılı, mekanizması
/// [`Session::shutdown`]'ın doc'unda). Yani süre "çocuğun düzgün kapanma
/// şansı" için değil, o şans bittikten sonra **kullanıcının ne kadar
/// bekleyeceği** için var:
///
/// - Kısa olsa ne kaybolur: ölçülen düzgün kapanışın üç mertebe üstünde
///   duruyoruz; daralan pay yavaş bir makinenin ya da release olmayan bir
///   yolun düzgün kapanışı olurdu — bu kadar paydan sonra kesilen bir
///   kapanış artık "yavaş" değil, takılmıştır.
/// - Uzun olsa ne kaybolur: bu süre Cmd-Q ile pencerenin kapanması
///   arasındaki gecikmenin tavanı. Yarım saniye donma sayılmıyor; saniyeler
///   sayılır.
///
/// **`pub` olmasının sebebi tek bir tüketici:** `bt-shell`'in duman bekçisi
/// bütçesini bunun **üstüne** kuruyor (`SHUTDOWN_GRACE` + sabit pay). İki
/// sayı ayrı ayrı yazılsaydı biri değişince öteki sessizce kayar ve bekçi ya
/// sağlıklı bir kapanışı keser ya da hiç kesmezdi.
pub const SHUTDOWN_GRACE: Duration = Duration::from_millis(500);

/// [`Session::shutdown`]'ın sonucu — duman/ölçüm raporundaki `kapanis=`
/// jetonunun kaynağı.
///
/// `bool` **değil** ve sebebi `Verdict`'inkiyle aynı: altı sonucun **dördü**
/// ayrı birer arıza ([`Teardown::Abandoned`], [`Teardown::ReaderPanicked`],
/// [`Teardown::Panicked`], [`Teardown::Unbounded`]) ve bunları tek bayrağa
/// katlamak tanıyı çağrı yerinde yeniden türetmeye zorlardı. Dördü aynı
/// ağırlıkta da değil: `bt-shell`'in duman kapısı yalnız iki panik kolunu
/// kırmızıya çeviriyor, ötekiler kayıtlı borç. Kalan ikisi arıza değil —
/// [`Teardown::Clean`] ve "zaten kapanmıştı" diyen
/// [`Teardown::AlreadyDone`]. Sınır dolan koşu bugüne kadar **yeşil bir jeton
/// satırıyla** geçiyordu — stderr'de bir satır vardı, jetonda iz yoktu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Teardown {
    /// Okuyucu thread bitti ve `Pty` düştü: çocuk arkada kalmadı.
    Clean,
    /// [`SHUTDOWN_GRACE`] doldu; çocuk çıkışın içinde bırakıldı ve onu süreç
    /// çıkışı toplayacak.
    Abandoned,
    /// Okuyucu thread **panikle** bitti. Kapanış tamamlandı (`Pty` unwind
    /// sırasında düştü, yani `SIGHUP` + `wait` yine koştu) ama panik
    /// projenin "PTY ve ayrıştırma yolunda panik yok" kuralının ihlali ve
    /// bunun jetonda izi olmalı — eskiden yalnız stderr'e bir satır düşüyor,
    /// rapor `temiz` diyordu.
    ReaderPanicked,
    /// Kapanış thread'i panikledi; PTY'nin durumu bilinmiyor.
    Panicked,
    /// Kapanış thread'i kurulamadı (OS thread sınırı): bu yolda **sınır
    /// yoktu** ve kapanış işi arkada, sahipsiz kaldı.
    Unbounded,
    /// İkinci ve sonraki çağrı. Hiçbir şey beklenmedi; kapanışın gerçek
    /// sonucunu **ilk** çağrı biliyor.
    AlreadyDone,
}

/// Başlamış bir kapanışın tutamağı ([`Session::begin_shutdown`]).
///
/// İçerideki kanal teardown thread'inin "bitti" haberini taşıyor; `None`
/// thread'in hiç kurulamadığı dal ([`Teardown::Unbounded`]). Tutamak
/// beklenmeden düşebilir — kapanış yine biter, yalnız sonucu kimse okumaz.
#[must_use = "düşen tutamak kapanışı durdurmaz ama sonucunu yutar"]
pub struct ShutdownHandle(Option<mpsc::Receiver<bool>>);

impl ShutdownHandle {
    /// Kapanışı en geç `deadline`'a kadar bekler ve sonucunu verir.
    ///
    /// Son tarih bir **an**, süre değil: Cmd-Q bütün oturumları başlatıp
    /// hepsini aynı son tarihe kadar bekliyor, yani sıradaki her bekleme
    /// kalan süreyi alıyor ve toplam bir [`SHUTDOWN_GRACE`]'i aşmıyor.
    /// Geçmiş bir son tarih sıfır beklemedir: bitmiş kapanış yine `Clean`
    /// döner, bitmemiş olan `Abandoned`.
    pub fn wait_until(self, deadline: Instant) -> Teardown {
        let Some(finished) = self.0 else {
            return Teardown::Unbounded;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        match finished.recv_timeout(remaining) {
            Ok(true) => Teardown::Clean,
            // Kapanış **bitti** ama okuyucu panikle bitti: çocuk toplandı,
            // yine de bu bir kural ihlali ve raporda görünmeli.
            Ok(false) => Teardown::ReaderPanicked,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                eprintln!("bateri: shell {SHUTDOWN_GRACE:?} içinde kapanmadı, arkada bırakıldı");
                Teardown::Abandoned
            }
            // Kanal göndermeden kapandı: kapanış thread'i panikledi ve bu
            // **hemen** dönüyor, yani süre dolmadı. İki durum tek satıra
            // katlanırsa tanı yalan söyler ("500 ms bekledim") ve kapanış
            // yolundaki bir panik sessizce yutulur.
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("bateri: kapanış thread'i panikle bitti, PTY'nin durumu bilinmiyor");
                Teardown::Panicked
            }
        }
    }
}

/// Seçim ucunun hücre içindeki yeri.
///
/// Sınırı belirleyen budur: bir hücrenin seçime girip girmemesi **farenin o
/// hücrenin ortasını geçip geçmediğine** bakar, yalnız hangi hücrede olduğuna
/// değil. İki uçta da kural tek cümledir — hücre, ortası iki uç arasında
/// kalıyorsa seçilir — ama ucun yarısı bu yüzden **aynaya bakar**: başlangıç
/// ucunda sol yarı kendi hücresini seçime katar (sağ yarı sınırı bir sonraki
/// hücrenin başına taşır), bitiş ucunda sağ yarı katar (sol yarı sınırı o
/// hücrenin başına çeker).
///
/// Bu alan olmadan kullanıcı, hedeflediği harfin **hemen soluna** basar —
/// doğal olan budur — ve o piksel bir önceki hücrenin sağ yarısına düşer;
/// yarı taşınmadığı için önceki harf de kopyalanır (006'da bildirilen kusur).
///
/// Geniş karakterde "hücre" iki hücrelik **glyph**'tir: baş hücre glyph'in
/// sol yarısı, spacer sağ yarısı sayılır (`anchor`). Hücre düzeyinde
/// kalsaydı spacer'ın sol yarısında biten seçim harfi kopyalar ama yalnız
/// yarısını vurgulardı.
///
/// Yarı alacritty'nin `Side`'ına birebir çevrilir, ama tip `pub` API'ye
/// **çıkmaz**: `bt-core` alacritty'yi kapsüller (`lib.rs`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellHalf {
    Left,
    Right,
}

/// Seçim ucu: hücre ve o hücrenin içindeki yarısı.
///
/// Hücre görünür pencere cinsindendir — seçimin iki kurucusu
/// ([`Session::set_selection`], [`Session::update_selection`]) onu
/// `display_offset` ile grid satırına indirir. Alanlar [`Cell`] ve [`Cursor`]
/// gibi adlı (`col`, `row`): demet olsaydı sütun ile satırın yer değiştirmesi
/// sessizce derlenirdi.
///
/// İki uç **eşit** verilebilir; bu sürüklemesiz tıktır ve alacritty böyle bir
/// seçimi boş sayar (`is_empty`): aralık doğmaz, kopyalanacak metin olmaz.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SelectionPoint {
    pub col: u16,
    pub row: u16,
    pub half: CellHalf,
}

/// Seçimin **adımı**: harf, kelime ya da satır — tek, çift ve üçlü tıklama
/// (`bt-shell`'in `clickCount` çevirisi).
///
/// Tip seçimin **içinde** yaşıyor ve sürükleme onu koruyor: çift tıklayıp
/// sürüklemek kelime adımıyla, üçlü tıklayıp sürüklemek satır adımıyla büyür,
/// Shift+tıklama da ([`Session::extend_selection`]) aynı adımla uzatır.
/// Davranışın sahibi alacritty (`Simple`, `Semantic`, `Lines`); bu enum
/// yalnız sınırın sözlüğü, `pub` API'de alacritty tipi görünmesin diye.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectKind {
    /// Harf harf; iki uç eşitse seçim boştur (sürüklemesiz tık).
    Simple,
    /// Kelime ([`WORD_SEPARATORS`]); tek noktada bile altındaki kelimeyi
    /// bütün alır.
    Word,
    /// Sarılmış **mantıksal** satır (`WRAPLINE`): ekrandaki iki fiziksel
    /// satır tek satır sayılır.
    Line,
}

/// Dock seçimi varken **terminalin** karşıladığı tuşlar (031 Karar 8) —
/// [`Session::dock_key`]'in sözlüğü. Kalan her tuş bugünkü yolundan gider
/// ve girdi seçimi kaldırır ([`Session::send_input`]).
///
/// `bt-core`'un tipi, çünkü karar burada: `bt-shell` `NSEvent`'i yalnız bu
/// altı tuşa çeviriyor, hangisinin ne yapacağını sormuyor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockKey {
    /// ⌫ — seçimi siler.
    Backspace,
    /// ⌦ (fn-⌫) — seçimi siler.
    Delete,
    /// ← — seçim kalkar, caret seçimin başına.
    Left,
    /// → — seçim kalkar, caret seçimin sonuna.
    Right,
    /// ⇧← — seçimin hareketli ucu bir karakter sola; seçim yoksa caret'ten
    /// başlar. Kabuğa hiçbir şey gitmez.
    ShiftLeft,
    /// ⇧→ — [`DockKey::ShiftLeft`]'in aynası.
    ShiftRight,
    /// ⇧⏎ — satırı çalıştırmadan imlecin yerine bir satır sonu ekler (varsa
    /// seçimin yerine). Seçim olsa da olmasa da tüketilir.
    NewLine,
    /// ⏎ — yalnız yeniden bağlanma teklifi varken ve satır boşken tüketilir
    /// (037 Karar 8): teklifin satırını yazılmış gibi gönderir. Teklif yoksa
    /// hiç tüketilmez ve Enter bayt bayt bugünkü yolundan gider.
    Enter,
}

/// Düzenleme kapısının açık olduğu andaki satır: `BUFFER`'ın karakter
/// sayısı, caret'in `BUFFER`'daki yeri ve seçim — **tek kilit turundan**.
///
/// Ayrı turlarda okunsalardı komutun `L`'si bir aynaya, aralığı öbürüne ait
/// olabilirdi; widget'ın `L` kemeri o hâli yakalar ama kemere hiç
/// düşmemek daha iyi.
struct DockEditLine {
    /// `BUFFER`'ın kendisi: komutun beklenen sonucu ondan hesaplanıyor
    /// ([`DockPrediction`]). Tuş başına bir kopya.
    buffer: String,
    len: usize,
    caret: usize,
    /// `PREBUFFER`'ın karakter sayısı: seçimin uzayı `PREBUFFER ++ BUFFER`
    /// ([`dock::selectable`]), komutunki `BUFFER` — aradaki kaydırma.
    shift: usize,
    selection: Option<DockSelection>,
    /// Satır aynadan mı (`true`) yoksa bekleyen komutun tahmininden mi.
    /// ⇧←/⇧→ yalnız aynada: seçim aynanın metnine karşı kuruluyor.
    fresh: bool,
    /// Caret'in solundaki ve sağındaki küme, `BUFFER`'da `[start, end)` —
    /// yalnız kümeleme açıkken ve birden çok kod noktalıysa (035 Karar 7):
    /// ⌫/← soldakini, ⌦/→ sağdakini bütün yürütüyor. Caret bir kümenin
    /// **içindeyse** (ZLE oraya koyabiliyor) iki taraf da o küme. Satırda bir
    /// **emoji** kümesi varsa tek kod noktalı komşu da komutla gidiyor
    /// (phase-5): ZLE'ye giden tuş tahmin zincirini kırar ve aynadan hızlı
    /// gelen tekrarı kapalı kapıdan ZLE'ye, yani kümenin ortasına düşürürdü
    /// ([`DockPrediction`]).
    before: Option<(usize, usize)>,
    after: Option<(usize, usize)>,
}

impl DockEditLine {
    /// Seçimin **`BUFFER`'daki** aralığı; seçim yoksa, boşsa ya da
    /// `PREBUFFER`'a değiyorsa `None` (032 Karar 2). ZLE kabul ettiği
    /// satırları düzenleyemiyor: o seçimde düzenleme tuşları komut
    /// göndermiyor, bugünkü yoldan gidiyor ve seçimi kaldırıyor (031 Karar
    /// 8'in "başka her tuş" kolu).
    fn buffer_range(&self) -> Option<(usize, usize)> {
        let (start, end) = self.selection.and_then(|selection| selection.range())?;
        Some((start.checked_sub(self.shift)?, end - self.shift))
    }
}

/// Terminalden kabuğa giden tek dizinin başı (`CSI 8133 ~`); biçimi
/// betiğin tel başlığında (`assets/shell/zsh/bateri.zsh`).
const DOCK_EDIT_PREFIX: &str = "\x1b[8133~";

/// Düzenleme komutu: `BUFFER`'ın `[start, end)` aralığını sil, caret'i
/// `start`'a koy; `len` terminalin gördüğü `${#BUFFER}` (tutmazsa widget
/// hiçbir şey yapmaz). Serbest ve saf: baytları sınama doğrudan karşılaştırıyor.
/// Tazeleme komutu (032 R5): widget'ın tanımadığı yük `BUFFER`'a dokunmuyor
/// ama aynayı yine basıyor, yani `r` "yalnız aynala" demek. Satır sonlu
/// yapıştırmanın arkasına gidiyor ([`Session::paste`]).
const DOCK_REFRESH_COMMAND: &[u8] = b"\x1b[8133~r\x07";

fn dock_edit_command(start: usize, end: usize, len: usize) -> Vec<u8> {
    format!("{DOCK_EDIT_PREFIX}d;{start};{end};{len}\x07").into_bytes()
}

impl SelectKind {
    fn alacritty(self) -> SelectionType {
        match self {
            Self::Simple => SelectionType::Simple,
            Self::Word => SelectionType::Semantic,
            Self::Line => SelectionType::Lines,
        }
    }
}

/// Kelimenin tek tanımı: **ayırıcılar**. Geri kalan her karakter — harf,
/// rakam, ASCII olmayan her şey (`│` hariç) ve `_ - . / ~ : @` — kelimenin
/// içinde kalır, yani yol (`~/src/a-b.rs`), `user@host`, `host:8080`,
/// `dosya.rs:42` ve sorgusuz bir URL tek çift tıkla gelir, `KEY=value`'da
/// `value` tek başına (`.tasks/031-fare-ile-secim/discussion.md` → Karar 5).
///
/// Ayırıcıyı kelimeden ayırmanın ötesindeki davranış **alacritty'nin
/// `Semantic`'inin**: parantez eşleme ve ayırıcının üstüne çift tıklama
/// kuralı (`selection.rs` `range_semantic`, `term/search.rs`). Dock'un kelime
/// sınırı (031 phase-4) aynı sabiti okuyor, ki iki yüzeyde kelime aynı şey
/// olsun. Ayar anahtarı **yok**: istek bir varsayılan istedi.
pub(crate) const WORD_SEPARATORS: &str = " \t`'\"│|;,=()[]{}<>!#$%&*+?\\^";

/// Ucun alacritty karşılığı: nokta + yan. Eşleme **tek yerde**: iki uç için
/// ayrı ayrı yazılsa biri `Left`/`Right` çevirmesinde kayabilir ve kayma
/// sessiz olurdu.
///
/// İki normalizasyon, ikisi de yarının kuralını ([`CellHalf`]) alacritty'nin
/// hücre düzeyindeki modeline taşımak için:
///
/// - **Geniş karakter:** baş hücre hep `Left`, spacer hep `Right` — yarı
///   glyph'e uygulanır.
/// - **Satır sonu:** son sütunun sağ yarısı, alt satırın başının sol yarısıyla
///   aynı sınırdır ve öyle yazılır. alacritty ikisini ayrı tutuyor ve iki uç o
///   sınırın iki yanına düşünce (arada hücre yok) üst satırın son hücresini
///   seçiyor. Grid'in son satırında alt satır yok, orada dokunulmaz.
///
/// Uç görünür pencere cinsinden gelir ve grid satırına **burada**, o anki
/// `display_offset` ile iner: seçimin iki kurucusu (`set_selection`,
/// `update_selection`) aynı reçeteyi iki kez yazmasın.
///
/// Bayrak okuması kırpılmış noktadan — `pub` API'ye grid dışı bir sütun
/// gelirse indeksleme paniklemesin.
fn anchor<T>(term: &Term<T>, at: SelectionPoint) -> (Point, Side) {
    let offset = term.grid().display_offset() as i32;
    let (point, half) = (viewport_point((at.col, at.row), offset), at.half);
    let flags = term.grid()[point.grid_clamp(term, Boundary::Grid)].flags;
    let side = if flags.contains(Flags::WIDE_CHAR) {
        Side::Left
    } else if flags.contains(Flags::WIDE_CHAR_SPACER) {
        Side::Right
    } else {
        match half {
            CellHalf::Left => Side::Left,
            CellHalf::Right => Side::Right,
        }
    };
    if side == Side::Right
        && point.column == term.last_column()
        && point.line < term.bottommost_line()
    {
        return (Point::new(point.line + 1, Column(0)), Side::Left);
    }
    (point, side)
}

/// Bir ekran satırının **son mürekkepli** karakteri; boş satırda `None`.
///
/// Bastırmanın tazelik kapısının ızgara yarısı ([`DockState::last_ink`] öteki
/// yarısı). Mürekkep ölçütü [`Session::frame`]'in atlama kapısıyla **aynı**
/// olmak zorunda — boşluk, spacer ve gizli hücre saymıyor — yoksa iki taraf
/// aynı satıra bakıp farklı cevap verir.
///
/// Nokta `grid_clamp` ile kırpılıyor: kaydırılmış pencerede satır geçmişe
/// düşebilir ve indeksleme panik yasağının altında (`CLAUDE.md`).
fn last_ink_in_row<T>(term: &Term<T>, row: u16, offset: i32) -> Option<char> {
    let line = Point::new(Line(i32::from(row) - offset), Column(0))
        .grid_clamp(term, Boundary::Grid)
        .line;
    (0..term.columns()).rev().find_map(|col| {
        let cell = &term.grid()[line][Column(col)];
        let blank = cell.flags.contains(Flags::HIDDEN)
            || cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER.union(Flags::LEADING_WIDE_CHAR_SPACER))
            || cell.c == ' ';
        (!blank).then_some(cell.c)
    })
}

/// Bir ızgara satırının **kimliği**: ilk hücresinin adresi, `0` satır boşsa.
///
/// [`Session::scroll_probe`]'un ölçüsü. Adres satırın kendi hücre tamponundan
/// ve o tampon kaydırmada yerinde kalıyor: alacritty'nin defteri bir halka,
/// `scroll_up` satırları kopyalamıyor, halkanın başını çeviriyor; defter
/// doyduğunda en eski satırın tamponu yeni dip satır olarak **yeniden
/// kullanılıyor** ama o satır bir önceki karede ekranın tepesinde değildi,
/// yani karışmıyor. Tamponu değiştiren tek yol sütun sayısının değişmesi ve
/// orada boyut kapısı karşılaştırmayı zaten kapatıyor.
///
/// `line` çağıranın sözleşmesiyle defterin içinde (`-history..rows`); satır
/// hücresiz olamaz ama indeksleme yerine yineleyici kullanılıyor, çünkü
/// `bt-core`'da panik yasak (emsali doldurma döngüsünün `zip`'i).
fn row_identity<T>(term: &Term<T>, line: Line) -> usize {
    let line = line.grid_clamp(term, Boundary::Grid);
    (&term.grid()[line])
        .into_iter()
        .next()
        .map_or(0, |cell| std::ptr::from_ref(cell) as usize)
}

/// Verilen satırdan **yukarı** doğru, bloğun çıpasını taşıyan ilk satır;
/// hiçbir satırda yoksa `None`.
///
/// Tazelik kapısının **boş ayna** yarısı ([`last_ink_in_row`] mürekkep
/// yarısı). Ayna hiç karakter taşımıyorsa iki taraf da "boş" der ve
/// karşılaştırma vakuma düşer; ayıran şey o hâlde geometridir — boş bir giriş
/// satırının imleci prompt'un satırından ayrılamaz.
///
/// **Yön yukarı ve ilk bulunan kazanıyor:** çıpa prompt'un hücrelerinde,
/// yani imlecin satırında ya da üstünde. İmlecin satırında bulunursa ayna
/// taze, yukarıda bulunursa bayat, hiç bulunmazsa kapı susuyor.
///
/// **Maliyeti yalnız boş satırda ödeniyor** (çağrı yeri `blank_mirror`
/// kolunda) ve olağan hâlde tek satır geziliyor: boş prompt'ta çıpa zaten
/// imlecin satırında. `hyperlink()` yan tabloya iniyor ve bir `Arc` klonluyor
/// ([`Session::frame`]'in çıpa okumasının doc'u), o yüzden tarama kapının
/// altında duruyor, üstünde değil.
///
/// [`Session::frame`]'in döngüsündeki `suppress_from` ile **aynı satırı**
/// bulur ama onun yerine geçemez: o değer hücre hücre, çizim sırasında
/// doğuyor ve kapı döngüden önce karar vermek zorunda.
fn anchor_row_at_or_above<T>(term: &Term<T>, row: u16, offset: i32, block: u32) -> Option<u16> {
    (0..=row).rev().find(|&probe| {
        let line = Point::new(Line(i32::from(probe) - offset), Column(0))
            .grid_clamp(term, Boundary::Grid)
            .line;
        (0..term.columns()).any(|col| {
            term.grid()[line][Column(col)]
                .hyperlink()
                .and_then(|link| block_id(link.uri()))
                == Some(block)
        })
    })
}

/// Ekranı temizlemenin iki kipi ([`Session::clear_to_start`],
/// [`Session::clear_scrollback`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClearKind {
    /// ⌘K: korunan bloğun üstü ve geçmiş.
    ToStart,
    /// ⌥⌘K: yalnız geçmiş.
    Scrollback,
}

/// Ekranın `line` satırının taşıdığı blok kimliği — [`block_id`]'nin satır
/// hâli, [`block_row_continues`]'un okuma deyimi. İlk bulunan kazanıyor: bir
/// satırda iki bloğun çıpası olamıyor (bağlantı `preexec`'te kapanıyor).
fn row_block<T>(term: &Term<T>, line: Line) -> Option<u32> {
    term.grid()[line]
        .into_iter()
        .find_map(|cell| cell.hyperlink().and_then(|link| block_id(link.uri())))
}

/// ⌘K'nin **korunan ilk satırı** (034 R1.1): o satırın üstü ekrandan atılır.
///
/// Bloğun kimliğini taşıyan **en üstteki** ekran satırı, imlecin satırı ve
/// üstü arasında — [`anchor_row_at_or_above`] değil: o imlece **en yakın**
/// çıpalı satırı veriyor ve bağlantı `preexec`'e kadar açık olduğu için
/// sarılan ya da çok satırlı girişte bu imlecin kendi satırı, yani girişin
/// üst satırları giderdi (`discussion.md` → Muhakeme). Bitişik satırlarda
/// yürümek de değil: çok satırlı girişin **boş** satırı (`Esc-Enter` iki
/// kez) hiç hücre yazmıyor, zsh orada yalnız siliyor ve silme bağlantısız
/// hücre bırakıyor — yürüyüş o boşlukta durur, prompt ile girişin üstü
/// giderdi (`/code-review`, 034 phase-1). En üstteki satır doğru, çünkü
/// kimlik prompt başına tek: aynı kimliği ekranda taşıyan başka bir bölge
/// yok (Ctrl-L'in bıraktığı kopya geçmişte).
///
/// Blok 0. satırda başlıyorsa (ya da başı geçmişte) cevap `0`: hiçbir satır
/// atılmaz, yalnız geçmiş silinir.
///
/// Kimlik imlecin satırından; satır çıpasızsa ve kabuk girdi safhasındaysa
/// (`input_block`, imleç girişin henüz yazılmamış bir satırında) kabuğun
/// defterinden. İkisi de yoksa (komut koşuyor, entegrasyonsuz kabuk)
/// korunan yalnız imlecin satırı. Safha `Term` kilidinden önce okundu
/// (yaprak kilit sırası).
///
/// `Term` kilidi tutulurken, pencere dipteyken (imlecin satırı ekran satırı).
fn protected_top<T>(term: &Term<T>, input_block: Option<u32>) -> usize {
    let row = term.grid().cursor.point.line.0.max(0);
    let top = row_block(term, Line(row))
        .or(input_block)
        .and_then(|id| (0..=row).find(|&probe| row_block(term, Line(probe)) == Some(id)))
        .unwrap_or(row);
    usize::try_from(top).unwrap_or(0)
}

/// Kapıdan geçmiş bir hücrenin **mürekkep yarısı**: ön plan rengi ve kural
/// çizgileri.
///
/// Ayrı tip değil ayrı **parça**: [`Session::frame`]'in iki döngüsü de
/// (ızgara ve doldurma) aynı hücre kaydını kuruyor ve bu dört alan ikisinde de
/// birebir aynı. Sınırdan geçen [`Cell`]'in alanları, yalnız kapıdan sonra
/// çözülenleri.
struct CellStyle {
    fg: LinearRgba,
    underline: UnderlineStyle,
    strikeout: bool,
    underline_color: Option<LinearRgba>,
}

/// Hücrenin kümesi (035 Karar 4B/6): kümeleme açık, hücre **geniş** ve
/// taban karakterin arkasında `zerowidth` varsa dizgi tabloya iner.
///
/// Geniş şartı kararın ta kendisi: tek sütunlu birleştirici (`é`, Arapça
/// hareke, `⌚︎`) bugünkü gibi taban karakterle çiziliyor. `ch` `None`'sa
/// (gizli, spacer) çizilecek bir şey yok. Yan tabloya (`CellExtra`) inen
/// okuma yalnız geniş hücrede, yani düz metin bedeli bir bayrak testi.
/// Izgara sink'i ile doldurma sink'i **aynı** fonksiyondan: bandın satırı
/// ekrana çıktığındakiyle aynı glyph'i çizmek zorunda.
fn cell_cluster(
    enabled: bool,
    cell: &TermCell,
    ch: Option<char>,
    clusters: &mut Clusters,
) -> Option<ClusterId> {
    if !enabled || !cell.flags.contains(Flags::WIDE_CHAR) {
        return None;
    }
    let head = ch?;
    let rest = cell.zerowidth().filter(|rest| !rest.is_empty())?;
    clusters.push_chars(std::iter::once(head).chain(rest.iter().copied()))
}

/// Atlama kapısından **sonra** çözülen alanlar; `inverse`, `dim` ve `ruled`
/// kapıdan **önce** çözülmüş olarak geliyor.
///
/// Paylaşım bir stil kararı değil: bu bloğun iki tuzağı var ve ikisi de ikinci
/// bir kopyada sessizce ayrışırdı.
///
/// **Beş bayrak ayrı ayrı sorulur ve kıvrımlı önce gelir.** `UNDERCURL`
/// `UNDERLINE`'ı **içermez**: `Attr::Undercurl` önce `ALL_UNDERLINES`'ı
/// siliyor, sonra yalnız kendini ekliyor (alacritty `term/mod.rs`, beş kolun
/// beşi de öyle). Refleksle yazılmış tek bir `contains(UNDERLINE)` 004'ün
/// varlık sebebi olan dalgalı çizgiyi sessizce düz çizgiye indirirdi ve hiçbir
/// sayaç bunu göremezdi — `undercurl_text_yields_curl` görüyor.
///
/// `ruled` yanlışsa zincir hiç sorulmuyor: gizli hücre de, hiç kuralı olmayan
/// hücre de tek testte eleniyor. Geniş karakterin ikinci hücresi bayrakları
/// şablondan kopyaladığı için kural iki hücreye kendiliğinden yayılıyor —
/// bedava, ama "neden çalışıyor" sorusunun cevabı burası.
///
/// **SGR 58'in kapısı `ruled` değil alt çizginin kendisi**: adı "alt çizgi
/// rengi" ve SGR'de üstü çizilinin ayrı bir rengi yok. `ruled` olsaydı yalnız
/// üstü çizili bir hücre (`\e[9;58;5;196m`) rengi taşırdı ve onu okuyan çizici
/// üstü çiziliyi kırmızıya boyardı. Yan tabloya (`CellExtra`) iniyor, yani
/// bedeli ancak kapıdan geçen hücreler için ödeniyor.
fn cell_style(
    cell: &TermCell,
    inverse: bool,
    dim: bool,
    ruled: bool,
    colors: &Colors,
    theme: &Theme,
) -> CellStyle {
    let flags = cell.flags;
    // Ön plan **koşulsuz**: alan adının söylediği şey olmalı, yoksa kural
    // çizgisi mürekkebi olmayan bir hücrede arka plan rengiyle, yani görünmez
    // olarak çizilirdi.
    //
    // Ters video hücrenin iki rengini takas eder; `DIM` ise **ön plana**
    // uygulanır (adlı rengi sönük eşine çeviren kod alacritty'nin ikili
    // tarafında, kitaplıkta değil). İkisi birleşince kural şu: sönüklük,
    // `cell.fg`'den doğan renge gider — ters videoda o renk arka plan olmuştur.
    // Kural `color::resolve_fg`'de tek: iki dal aynı fonksiyondan geçiyor,
    // ters videolu dal sönük rolü unutamıyor.
    let fg = if inverse {
        color::resolve(cell.bg, colors, theme)
    } else {
        color::resolve_fg(cell.fg, dim, colors, theme)
    };
    let underline = if !ruled {
        UnderlineStyle::None
    } else if flags.contains(Flags::UNDERCURL) {
        UnderlineStyle::Curl
    } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
        UnderlineStyle::Double
    } else if flags.contains(Flags::DOTTED_UNDERLINE) {
        UnderlineStyle::Dotted
    } else if flags.contains(Flags::DASHED_UNDERLINE) {
        UnderlineStyle::Dashed
    } else if flags.contains(Flags::UNDERLINE) {
        UnderlineStyle::Single
    } else {
        UnderlineStyle::None
    };
    CellStyle {
        fg: color::linear_rgba(fg),
        underline,
        strikeout: ruled && flags.contains(Flags::STRIKEOUT),
        // `None` → çizen taraf `fg`'yi kullanır; `bg` ile birebir aynı örüntü
        // ve alacritty'nin `Color`'ı `pub` API'ye sızmıyor.
        underline_color: (underline != UnderlineStyle::None)
            .then(|| cell.underline_color())
            .flatten()
            .map(|c| color::linear_rgba(color::resolve(c, colors, theme))),
    }
}

/// Bastırılan giriş satırının ızgaradaki satır aralığı — `display: none`
/// satırlar; `None` → bastırma yok.
///
/// **Tek yüklem, üç tüketici** ([`Session::frame`]): atlanan hücreler, seçim
/// koşuları ve arama eşleşmeleri aynı aralıktan soruyor. Üst uç çıpanın
/// satırı ile aynanın tabanının alttakisi (`from.max(floor)`), alt uç
/// tazelik kapısını geçmiş imleç kuyruğu; ikisinin gerekçesi `frame()`'in
/// yorumlarında.
fn suppressed_rows(
    caret_in_dock: bool,
    from: Option<u16>,
    to: Option<u16>,
    floor: u16,
) -> Option<RangeInclusive<u16>> {
    match (caret_in_dock, from, to) {
        (true, Some(from), Some(to)) => Some(from.max(floor)..=to),
        _ => None,
    }
}

/// Arama taramasının penceresi: ızgaranın ofseti ve boyu, doldurma
/// kanalının boyu (`top_row + fill`) ve bastırılan satırlar.
struct SearchWindow {
    offset: i32,
    rows: i32,
    channel: i32,
    hidden: Option<RangeInclusive<u16>>,
}

/// Çizilen satırların eşleşmelerini koşulara çevirir (033 phase-1).
///
/// **`Term` kilidi tutulurken** ([`Session::frame`]). Satır `L` ızgarada
/// `L + offset` (`0..rows`), kanalda `L + offset + channel` (`0..channel`);
/// ikisinin dışı çizilmiyor ve koşu vermiyor. Eşleşme satır başına bir koşuya
/// bölünüyor, ilkinden sonrakiler `continues`.
///
/// **Dışlanan iki eşleşme**, ikisi de bütünüyle — yarım vurgulanmış bir
/// eşleşme sayımda (phase-5) tek eşleşme ama ekranda başka bir şey olurdu:
///
/// - Bastırılan giriş satırına değen (Karar 8): o satır ızgarada çizilmiyor
///   ve dock ayrı bir yüzey; görünmeyen bir satıra vurgu, sayı ya da gezinme
///   hedefi verilmez.
/// - Mürekkepsiz ([`search::has_ink`]): yalnız boşluktan oluşan eşleşme boş
///   satırları boyardı — vurgu içerik yaratmaz.
///
/// **Geçerli eşleşme** yuvanın seçtiği ([`SearchSlot::current`]): onunla
/// aynı yerdeki eşleşmenin koşuları `current`. Yuvada eşleşme yoksa (ya da
/// çıktı onu kaydırdıysa) hiçbir koşu işaretlenmiyor.
fn search_visible<T>(
    term: &Term<T>,
    regex: &mut search_engine::RegexSearch,
    current: Option<&search_engine::Match>,
    out: &mut SearchRuns,
    window: SearchWindow,
) {
    let SearchWindow {
        offset,
        rows,
        channel,
        hidden,
    } = window;
    let top = Line(-offset - channel);
    let bottom = Line(rows - 1 - offset);
    let last_col = term.columns().saturating_sub(1);
    search::scan(term, regex, top, bottom, |found| {
        let (start, end) = (*found.start(), *found.end());
        let lines = start.line.0..=end.line.0;
        if let Some(hidden) = &hidden
            && lines
                .clone()
                .any(|line| u16::try_from(line + offset).is_ok_and(|row| hidden.contains(&row)))
        {
            return;
        }
        if !search::has_ink(term, found) {
            return;
        }
        let is_current = current.is_some_and(|current| search::same_place(found, current));
        for line in lines {
            if line < top.0 || line > bottom.0 {
                continue;
            }
            let first = if line == start.line.0 {
                start.column.0
            } else {
                0
            };
            let last = if line == end.line.0 {
                // Geniş karakter iki sütun: koşu spacer'ına kadar (seçimin
                // aritmetiği).
                let wide = term.grid()[end].flags.contains(Flags::WIDE_CHAR);
                (end.column.0 + usize::from(wide)).min(last_col)
            } else {
                last_col
            };
            let (Ok(first), Ok(last)) = (u16::try_from(first), u16::try_from(last)) else {
                continue;
            };
            let run = |row: i32| {
                u16::try_from(row).ok().map(|row| SearchRun {
                    row,
                    first,
                    last,
                    current: is_current,
                    continues: line != start.line.0,
                })
            };
            let grid_row = line + offset;
            if grid_row >= 0 {
                out.runs.extend(run(grid_row));
            } else {
                out.fill_runs.extend(run(grid_row + channel));
            }
        }
    });
}
/// [`Session::with_search`]'ün `work`'üne verilen durum: yuvadan alınan
/// geçerli eşleşme ve başlangıç (değiştirilebilir, geri konuyor) ve son
/// karenin bastırılan satırları (salt okunur).
struct SearchState<'a> {
    current: Option<search_engine::Match>,
    origin: Option<Point>,
    hidden: Option<&'a RangeInclusive<i32>>,
}

/// Gezinmenin sıra numarasına etkisi ([`Session::with_search`]): dizin
/// eşleşmeleri saklamıyor, geçerli eşleşmenin sırası komşuya ±1 taşınıyor.
#[derive(Clone, Copy, Debug)]
enum OrdinalStep {
    /// Geçerli eşleşme yerinde.
    Kept,
    /// Komşuya geçti; `wrapped` → defterin ucunda sardı.
    Moved {
        direction: SearchDirection,
        wrapped: bool,
    },
    /// Sırası bilinmeyen yeni bir eşleşme seçildi.
    Fresh,
}

impl OrdinalStep {
    /// Sırayı taşır; dizin bitmişken sıra bulunamıyorsa `true` — çağıran
    /// bir geçiş daha ister.
    fn apply(self, index: &mut search::SearchIndex) -> bool {
        match self {
            Self::Kept => false,
            Self::Moved {
                direction: SearchDirection::Older,
                wrapped: false,
            } => {
                index.ordinal = index.ordinal.map(|ordinal| ordinal + 1);
                false
            }
            Self::Moved {
                direction: SearchDirection::Older,
                wrapped: true,
            } => {
                // En eskiden en yeniye.
                index.ordinal = Some(1);
                false
            }
            Self::Moved {
                direction: SearchDirection::Newer,
                wrapped: false,
            } => {
                index.ordinal = index
                    .ordinal
                    .and_then(|ordinal| ordinal.checked_sub(1))
                    .filter(|ordinal| *ordinal > 0);
                false
            }
            Self::Moved {
                direction: SearchDirection::Newer,
                wrapped: true,
            } => {
                // En yeniden en eskiye: sayısı ancak dizin bitmişse biliniyor.
                let done = index.next.is_none();
                index.ordinal = done.then_some(index.total);
                false
            }
            Self::Fresh => {
                index.ordinal = None;
                index.next.is_none()
            }
        }
    }
}

/// Nokta defterin içinde mi. Yuvada saklanan mutlak koordinat kayıp bir
/// kaymadan sonra defterin dışına düşebiliyor ([`search::track`])
/// ve alacritty'nin ızgara indeksi dışarıda panikler.
fn in_grid<T>(term: &Term<T>, point: Point) -> bool {
    point.line >= term.topmost_line()
        && point.line <= term.bottommost_line()
        && point.column <= term.last_column()
}

/// Eşleşmenin iki ucu da defterin içinde mi ([`in_grid`]).
fn match_in_grid<T>(term: &Term<T>, found: &search_engine::Match) -> bool {
    in_grid(term, *found.start()) && in_grid(term, *found.end())
}

/// Görünür pencerenin sağ alt hücresi — yazarken aramanın başladığı yer.
fn window_bottom<T>(term: &Term<T>) -> Point {
    let offset = term.grid().display_offset() as i32;
    let bottom = term.screen_lines() as i32 - 1 - offset;
    Point::new(Line(bottom), term.last_column())
}

/// Pencerenin **çizilen** satırları, mutlak: `offset` ofsetli pencerede
/// ızgara ve (dipteyse) doldurma bandı.
fn drawn_lines<T>(term: &Term<T>, offset: i32, band: i32) -> RangeInclusive<i32> {
    let band = if offset == 0 { band } else { 0 };
    -offset - band..=term.screen_lines() as i32 - 1 - offset
}

/// Yazarken geçerli eşleşme (Karar 3): pencerede çizilen bir eşleşme varsa
/// en alttaki, yoksa `origin`'den yukarı ilk eşleşme (uçta sarar).
fn nearest_match<T>(
    term: &Term<T>,
    regex: &mut search_engine::RegexSearch,
    origin: Point,
    hidden: Option<&RangeInclusive<i32>>,
    band: i32,
) -> Option<search_engine::Match> {
    let offset = term.grid().display_offset() as i32;
    let drawn = drawn_lines(term, offset, band);
    // Pencerenin dibinden yukarı ilk eşleşme ya pencerededir — o zaman en
    // alttaki odur — ya da yukarıda (ya da sarıp aşağıda) ve pencerede hiç
    // yoktur.
    let bottom = window_bottom(term);
    if let Some(found) = search::next_eligible(term, regex, bottom, Direction::Left, hidden)
        && drawn.contains(&found.end().line.0)
    {
        return Some(found);
    }
    search::next_eligible(term, regex, origin, Direction::Left, hidden)
}

/// Eşleşme panelin altında olmadan görünür mü (Karar 4): bütün satırları
/// çizilen pencerede ve örtülen satırlardakiler panelin solunda.
fn match_visible<T>(
    term: &Term<T>,
    found: &search_engine::Match,
    cover: SearchCover,
    band: i32,
) -> bool {
    let offset = term.grid().display_offset() as i32;
    let drawn = drawn_lines(term, offset, band);
    let covered_above = cover.first_row.saturating_sub(offset);
    let (start, end) = (*found.start(), *found.end());
    (start.line.0..=end.line.0).all(|line| {
        if !drawn.contains(&line) {
            return false;
        }
        let last = if line == end.line.0 {
            end.column.0
        } else {
            term.last_column().0
        };
        line >= covered_above || last < usize::from(cover.from_col)
    })
}

/// Eşleşmeyi pencerenin ortasına getiren **görsel tepe** ([`visual_top`]'un
/// uzayı): başlangıç satırı ekranın orta satırında, defterin iki ucuna
/// kırpılı. Tepe bandın içine düşüyorsa hedef dip — `1..=band` ekranda hiç
/// görülmeyen ofsetler ([`scroll_locked`]).
fn reveal_target<T>(term: &Term<T>, found: &search_engine::Match, band: i32) -> i32 {
    let rows = term.screen_lines() as i32;
    let history = i32::try_from(term.history_size()).unwrap_or(i32::MAX);
    let top = (rows / 2 - found.start().line.0).clamp(0, history);
    top.max(band)
}

/// Seçimin **ekranda** çizilen aralığı — `frame()`, kare kapısı ve temizleme
/// hep bunu sorar, ki "çizili mi" sorusunun tek cevabı olsun. Çıktının
/// görünür pencerenin üstüne ittiği bir aralık grid'de durur ama çizilmez.
fn visible_range<T>(selection: Option<&Selection>, term: &Term<T>) -> Option<SelectionRange> {
    let range = selection?.to_range(term)?;
    let offset = term.grid().display_offset() as i32;
    let top = Line(-offset);
    let bottom = Line(term.screen_lines() as i32 - 1 - offset);
    (range.end.line >= top && range.start.line <= bottom).then_some(range)
}

/// `line` satırının bir hücresi `id` bloğunun çıpasını taşıyor mu — o satırın
/// altındaki çıpalı satır komutun **devamı**, başı değil.
///
/// Soru blok işaretinin ve süre sayacının yeri için ([`Session::frame`]'in
/// çıpa toplaması, ızgara ve doldurma bandı): bağlantı `preexec`'e kadar açık
/// ve çok satırlı bir komutun bütün satırları kimliği taşıyor, yani "ilk
/// görünen çıpalı satır" ölçütü komutun başı ekranın üstüne kayınca işareti
/// onun devamına oturtuyordu (032 phase-5'te görüldü).
///
/// Defterin dışındaki satır (boş geçmiş, en eski satır) `false`: kırpma
/// satırı kendisine çevirir ve her prompt kendi devamı sayılırdı. Son
/// temizlemenin kalıntısı (`boundary`, [`Session::clear_boundary`]) da
/// `false`: Ctrl-L'nin yeniden bastığı prompt aynı kimliği taşıyor ve
/// işaretini kaybetmemeli. Tarama satır başına bir kez ve eşleşmede duruyor.
fn block_row_continues<T>(term: &Term<T>, line: Line, id: u32, boundary: usize) -> bool {
    if line < term.topmost_line() || line > term.bottommost_line() {
        return false;
    }
    if boundary != 0 && row_identity(term, line) == boundary {
        return false;
    }
    // `grid_clamp` yalnız çağrı yerinin emniyet kemeri: aralık yukarıda
    // soruldu (panik yasağı, `CLAUDE.md`).
    let line = Point::new(line, Column(0))
        .grid_clamp(term, Boundary::Grid)
        .line;
    term.grid()[line]
        .into_iter()
        .any(|cell| cell.hyperlink().and_then(|link| block_id(link.uri())) == Some(id))
}

/// Prompt hücresine iliştirilmiş OSC 8 bağlantısından blok kimliği; bizim
/// olmayan bağlantı `None`.
///
/// Şema **bize özel** (`bateri://block/`) ve kapı bu önek: `ls --hyperlink`'in
/// `file://`'ı, bir `man` sayfasının `https://`'i ya da kullanıcının kendi
/// prompt'undaki bağlantı buradan geçmez. Kimliği basan taraf
/// `assets/shell/zsh/bateri.zsh`.
///
/// Kimlik `u32` ve ondalık: betiğin sayacı `%9v` ile genişliyor, yani metin.
/// Şemanın öbür yolu (`bateri://tab/`, sekmenin dış adı) `crate::identity`'de.
fn block_id(uri: &str) -> Option<u32> {
    uri.strip_prefix("bateri://block/")?.parse().ok()
}

/// [`Session::scroll_wheel`]'in cevabı: tekerlek nereye gitti.
///
/// `Option<i32>` değil, çünkü `bt-shell` üç cevapta üç ayrı şey yapıyor:
/// kayan pencerede basılı sürüklemenin ucunu taşıyor, gönderimde tekerleğin
/// satır artığını **koruyor**, yoksayılan olayda artığı atıyor. Gönderim
/// `None`'a katlansaydı trackpad'le yavaş kaydırmada her olayın küsuratı
/// düşer ve uygulama sarsak kayardı; `Some(0)`'a katlansaydı artık yine
/// düşerdi.
///
/// **Kapı tipte**, davranışta görünmüyor: alacritty alternate grid'i geçmişsiz
/// kuruyor (`Grid::new(.., 0)`), yani alternate screen kapısı silinse de ofset
/// oynamaz, kare istenmez — [`Wheel::Ignored`] ile `Scrolled(0)` ekranda aynı.
/// `wheel_is_ignored_without_alternate_scroll_or_with_shift` bu yüzden
/// cevabın kendisini soruyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wheel {
    /// Birincil ekran: **ofset** `n` satır değişti; geçmişin iki ucunda `0`.
    ///
    /// Görsel hareket değil: doldurma bandı olan pencerenin ilk çentiği
    /// ofseti `fill + 1` yapıyor ama ekran bir satır kayıyor
    /// ([`scroll_locked`]). Tüketiciler yalnız "kaydı mı" diye soruyor.
    ///
    /// **Kesirli kollarda da ofset farkı** ([`ScrollIntent`]): kesir oynayıp
    /// ofset oynamadıysa `0`, süzülme isteği ise ofseti bu olayda hiç
    /// oynatmıyor ve hep `0` — istek kare yolunda teslim ediliyor. Yani
    /// `Scrolled(0)` burada "uç" demek değil, "tam satır geçmedi" demek;
    /// artığı sıfırlama kuralı ([`ScrollIntent::Lines`]'ın satır artığı) bu
    /// kollarda konusuz, çünkü artık yok.
    Scrolled(i32),
    /// Uygulamaya gitti: fare kipinde tekerlek raporu, alternate screen'de ok.
    Sent,
    /// Hiçbir şey gitmedi: alternate screen'de ok kapalı (`\e[?1007l`) ya da
    /// Shift basılı; fare kipinde işaretçi geçmişte ya da koordinat
    /// kodlamaya sığmıyor; uygulama yolunda sıfır satır.
    Ignored,
}

/// Tekerlek olayının **niyeti** — kaydırma kolunda ([`Session::scroll_wheel`]'in
/// birincil ekranı) miktarın nasıl uygulanacağı.
///
/// Sınıflama `bt-shell`'in işi (hassas delta mı, jestin ve momentumun fazı;
/// AppKit orada), uygulaması burada, çünkü kesir ve kaydırma nesli burada
/// yaşıyor. Tip AppKit görmüyor: dört kesirli kol ve bugünkü satır yolu.
///
/// **Ok ve rapor kolları niyete bakmıyor**: alternatif ekranda ve fare
/// kipinde tekerlek tam satırla gidiyor, bugünkü gibi (plan → Kapsam Dışı).
/// Kesirli kolların her biri olayın `rows`'unu da uyguluyor — yerleşme ve
/// jest başı olayları çoğunlukla sıfır delta taşıyor ama taşıdıklarında
/// düşmemeli.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollIntent {
    /// Tam satır, bugünkü yol ([`scroll_locked`]) — `smooth_scroll = "off"`,
    /// Hareketi Azalt ve `cursor_motion = "snap"`. Kalmış bir kesir varsa
    /// düşüyor: satır adımında dinlenen pencerenin tepesinde yarım satır
    /// asılı kalmamalı.
    Lines,
    /// Kesirli delta **doğrudan**: trackpad'in parmağı ve momentumu. Ekranı
    /// parmak sürüklüyor, animasyon yok.
    Direct,
    /// Çentik: miktar **süzülme isteği** olarak birikiyor ve kare yolu onu
    /// kare kare teslim ediyor ([`Session::take_scroll_glide`]).
    Glide,
    /// Jest ya da momentum bitti: pencere en yakın tam satıra süzülsün. Payı
    /// burada, kesri gören tarafta hesaplanıyor (`round(kesir) − kesir`).
    Settle,
    /// Parmak yeniden değdi ya da momentum başladı: bekleyen süzülme isteği
    /// düşüyor ve nesil artıyor, yani uçuştaki yerleşme de bitiyor — ekran
    /// yine parmağın (ya da momentumun) ve önceki jestin kalan payı onu
    /// parmaktan uzaklaştırmamalı. Model göreli olduğu için sıçrama yok:
    /// yeni deltalar kesrin durduğu yerden devam ediyor.
    GestureBegan,
}

/// Kaydırmanın **süzülme payı** ve ait olduğu **nesil**.
///
/// İki yerde aynı biçimle geçiyor: [`Session::take_scroll_glide`] bekleyen
/// isteği veriyor, [`Session::frame`] bu karede teslim edilen payı alıyor.
/// Nesil ikisinde de aynı soruyu yanıtlıyor: "bu miktar pencerenin **şu anki**
/// konumuna mı ait". Girdide dibe dönüş ve Shift+PgUp konumu dışarıdan
/// sıfırlıyor; o sıfırlamadan önce hesaplanmış bir pay sıfırlamadan sonra
/// uygulansaydı dibe dönen pencere yazarken bir satırın kesri kadar yukarı
/// kayardı ve orada kalırdı.
///
/// `rows` artı geriye (yukarı), `Session::scroll_wheel` ile aynı yön. `f32`:
/// payın bir kare içindeki kesri, hassasiyetin sınırı bir pikselin çok
/// altında.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollGlide {
    pub rows: f32,
    pub generation: u32,
}

impl ScrollGlide {
    /// Tek kelimeye paketlenmiş hâli ([`Session::scroll_glide`]): nesil üst
    /// yarıda, miktarın bitleri alt yarıda.
    fn pack(self) -> u64 {
        (u64::from(self.generation) << 32) | u64::from(self.rows.to_bits())
    }

    fn unpack(word: u64) -> Self {
        Self {
            // `as` kesmesi bilerek: iki yarıyı ayıran şey tam olarak o.
            rows: f32::from_bits(word as u32),
            generation: (word >> 32) as u32,
        }
    }
}

/// [`Session::mouse_button`]'ın cevabı: düğme olayı nereye gitti. Basış da
/// bırakma da buradan cevaplanıyor.
///
/// `bool` değil ve sebebi iki ayrı "hayır": *kip kapalı* (jest terminalin,
/// seçim başlamalı) ile *kip açık ama rapor düştü* (koordinat kodlamaya
/// sığmadı ya da satır uygulamanın ekranında değil). `bool` ikisini
/// birleştirseydi geçmişe kaydırılmış pencerede fare kipindeki tıklama
/// sessizce seçim başlatırdı — kullanıcının uygulamaya gönderdiğini sandığı
/// tık ekranda bir vurgu bırakırdı. [`Wheel`]'in üç varyantlı olmasının
/// gerekçesi de aynı aileden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    /// Rapor uygulamaya gitti; jest artık uygulamanın.
    Sent,
    /// Jest terminalin: kip kapalı ya da Shift basılı. **Seçimi `bt-shell`
    /// başlatıyor**, burası yalnız yolu söylüyor.
    Select,
    /// Hiçbir şey gitmedi: kip açık ama işaretçinin satırı uygulamanın
    /// ekranında değil (pencere geçmişte) ya da koordinat kodlamaya sığmıyor;
    /// bırakmada ise kip bu arada kapanmış.
    Ignored,
}

/// Dock'un son çizilen penceresi: isabet testinin izi (031 R3.5).
///
/// Fare ekrandakine tıklıyor ve ekrandaki satır **son çizilen** ayna; canlı
/// ayna o kareden beri ilerlemiş olabilir (yeni bir tuşun aynası, öneri).
/// İz dikey pencerenin tepesini (032: sarılan giriş tavanı aşınca), sarmanın
/// genişliğini ve o aynanın `BUFFER`'ının uzunluğunu taşıyor:
/// canlı `BUFFER` aynı uzunluktaysa metin aynı kabul edilip çizilen kaymayla
/// eşleniyor, değilse tık seçim doğurmuyor — ekranda görülmemiş bir metne
/// seçim kurmaktansa hiç kurmamak (bir kare sonra doğru satır çizilmiş olur).
/// Uzunluk **bayt**: karşılaştırma kare yolunda ikinci bir sayım
/// istemesin; bir damga, bir ölçü değil.
#[derive(Clone, Copy, Debug)]
struct DockWindow {
    top: usize,
    /// Son **çizilen** karenin tepesi. `top` tekerlekle kare beklemeden
    /// değişiyor ([`Session::dock_scroll`]: sürüklemenin ucu aynı olayda yeni
    /// pencereye çözülsün); yazım efektlerinin satır kayması ise ekranda
    /// görülen tepeden ölçülmeli (`dock::with_shift`), yoksa tekerleğin
    /// kaydırdığı karede uçuştaki hayaletler eski satırlarında kalırdı.
    painted: usize,
    /// Girişin tavansız satır sayısı: tekerleğin kaydırabileceği son tepe
    /// `rows - shown` ([`Session::dock_scroll`]).
    rows: usize,
    /// Çizilen giriş satırı sayısı: pencerenin dışındaki bir satıra (ayrı
    /// karelerden yayınlanan geometri) düşen nokta reddediliyor.
    shown: u16,
    cols: u16,
    /// Seçilebilir metnin (`PREBUFFER ++ BUFFER`) bayt uzunluğu.
    buffer_bytes: usize,
}

impl DockWindow {
    /// `point` giriş bloğunda bir hücre: satırı dikey pencerenin içinde,
    /// sütunu ekran sütunu (`bt-shell`'in `window_point_dock`'u).
    fn hit(&self, state: &DockState, point: SelectionPoint) -> Option<DockPoint> {
        if state.prebuffer.len() + state.buffer.len() != self.buffer_bytes
            || point.row >= self.shown
        {
            return None;
        }
        dock::hit(state, self.top, self.cols, point.row, point.col, point.half)
    }
}

/// Caret'in `BUFFER`'daki karakter indeksi: aynanın caret'i görüntü
/// uzayında (`PREDISPLAY ++ BUFFER ++ …`, [`DockState::cursor`]), ZLE'nin
/// `$CURSOR`'ı ve düzenleme komutu ise `BUFFER`'ın başından sayıyor.
fn buffer_caret(state: &DockState) -> usize {
    state
        .cursor
        .saturating_sub(state.predisplay.chars().count())
        .min(state.buffer.chars().count())
}

/// PTY'si, okuyucu thread'i ve grid'i olan bir terminal oturumu.
pub struct Session {
    term: Arc<FairMutex<Term<Adapter>>>,
    sender: EventLoopSender,
    adapter: Adapter,
    /// Okuyucu thread; `shutdown()` alır, `Drop` de çağırır. `Option`
    /// "kapandı" demenin ve iki kez join etmemenin yoludur.
    reader: Mutex<Option<Reader>>,
    /// Kabuğun OSC 133 ile bildirdiği durum ve blok defteri.
    ///
    /// **Yaprak kilit** (`theme` emsali): tutulurken başka kilit alınmaz ve
    /// tutan taraf yalnız kopyalar — [`ShellState`] `Copy` ve küçük, defterden
    /// de kimlik başına tek bir akıbet okunuyor. Yazanı okuyucu thread'i,
    /// okuyanı [`Session::shell_state`] ile kare yolu.
    ///
    /// `Adapter`'da değil `Session`'da, çünkü `Adapter` alacritty'nin
    /// olaylarını karşılıyor ve bu duruma **hiç** dokunmuyor; işaretler
    /// olaylardan değil ham bayt akışından geliyor. `Arc`, çünkü aynı yuvanın
    /// öteki ucu [`TappedPty`] ile okuyucu thread'inde.
    shell: Arc<Mutex<ShellLog>>,
    /// Uygulama alternatif ekranda mı — [`Session::frame`]'in yayınladığı,
    /// [`Session::alt_screen`]'in okuduğu değer.
    ///
    /// **Atomik, kilit değil** ve sebebi tek: okuyanı kare yolu, kare başına.
    /// `Term` kilidini ikinci kez almak (emsali `bracketed_paste`) kare başına
    /// ikinci bir kilit turu demekti ve o kilit okuyucunun ayrıştırma
    /// lease'inin arkasında bekleyebiliyor. Yazan **tek** yer `frame()` ve
    /// orada kilit zaten tutuluyor, yani `Relaxed` yetiyor: değerin
    /// görünürlüğü karenin kendi sırasına bağlı, başka bir veriyle
    /// eşlenmiyor.
    ///
    /// **"Son karedeki hâl" demek**, "şu andaki" değil — ve bu bir eksiklik
    /// değil, istenen şey: tüketicisi ızgara yüksekliğini o karenin
    /// `content_rows`'uyla tutarlı tutmak zorunda. Bayat kalamıyor, çünkü
    /// `?1049h`/`l` baytları da her okuma turu gibi `Wakeup` doğuruyor
    /// (`alacritty_terminal`'ın `pty_read`'i) ve arkasından bir içerik karesi
    /// geliyor.
    alt_screen: AtomicBool,
    /// Caret'in sahibi dock mu — **son karedeki** hâl ([`Cursor::caret_in_dock`]),
    /// [`Session::frame`]'in yayınladığı değer.
    ///
    /// [`Session::alt_screen`]'in ikizi ve aynı gerekçeyle atomik: yazan tek
    /// yer `frame()`, okuyan Edit ▸ Select All ([`Session::select_all`]) —
    /// ⌘A dock caret'in sahibiyken dock'un satırını seçiyor (031 Karar 7) ve
    /// o kararı yeniden türetmek `frame()`'in üç ön koşulunu ikinci kez
    /// yazmak olurdu.
    caret_in_dock: AtomicBool,
    /// Geçmişte aramanın derlenmiş deseni ve nesli (033) — **yaprak kilit**,
    /// `theme` emsali; ödünç alma kuralı [`SearchSlot`]'ta.
    search: Mutex<SearchSlot>,
    /// Defterin tavanı (`scrollback`) — geçerli arama eşleşmesinin kaymasının
    /// "defter doydu mu" sorusu ([`search::ledger_shift`]). Yazanı açılış ve
    /// [`Session::set_terminal_options`].
    scrollback: AtomicUsize,
    /// Kullanıcının kaydırmasının **birikmiş ofset farkı**
    /// ([`Session::scroll_user`]): doymuş defterde ofsetin çıktıdan gelen
    /// payı bundan ayrılıyor ([`search::LedgerMark::user`]).
    user_scroll: AtomicI64,
    /// Dock'un **son çizilen** penceresinin izi ([`DockWindow`]): isabet
    /// testi canlı aynaya değil ekrandakine bakıyor (031 R3.5).
    ///
    /// Yaprak kilit ve `shell`'den **ayrı**: yazanı [`Session::dock`]
    /// pencereleme hesabından **sonra** (kayma orada doğuyor), `shell`
    /// kilidini bıraktıktan sonra; okuyanı isabet testi, `shell`'i almadan
    /// önce. İkisi iç içe hiç alınmıyor.
    dock_window: Mutex<Option<DockWindow>>,
    /// "Ekran kasten temizlendi" olaylarının **nesil sayacı**, bayrağın
    /// kendisi değil. **İki yazarı** var ve ikisi de yalnız artırıyor:
    /// okuyucu thread'i tarayıcının saydığı `CSI 2 J`'yi baytlar
    /// uygulanmadan **önce** ([`TappedPty::read`]), ana thread terminal
    /// tarafı temizliği (⌘K/⌥⌘K) `Term` kilidi altında ve uygulandıktan
    /// **sonra** ([`Session::note_screen_clear`]). Okuyanı
    /// [`Session::observe_screen_clear`]; ikisinin sırası farklı ama sonucu
    /// aynı — kilidin altında okunan nesil ya henüz uygulanmamış bir `2J`'yi
    /// (bayrağı kurmak için okunuyor, düşürmek için değil) ya da çoktan
    /// uygulanmış bir temizliği gösteriyor.
    ///
    /// `Arc`, çünkü öteki ucu sarmalayıcıyla okuyucu thread'inde ([`shell`]
    /// emsali). Atomik ve **yaprak kilit değil**, çünkü okunduğu yer `Term`
    /// kilidinin **altı**: bir muteks orada yasak (modül kuralı), atomik değil.
    ///
    /// [`shell`]: Session::shell
    screen_clears: Arc<AtomicU32>,
    /// Kullanıcı girdisinin **nesli**: [`Session::send_input`]'tan geçen her
    /// gönderim bir artırıyor, baytlar gitmeden **önce**.
    ///
    /// Tazelik kapısının zamansal yarısı (025): okuyucu thread'i ayna
    /// çözüldüğü anda bunu okuyup aynanın yanına damga olarak koyuyor
    /// ([`crate::shell::DockState::answers`]); kare yolu damga ile güncel
    /// nesli karşılaştırıyor ve eşitse kullanıcının son girdisinin aynası
    /// gelmiş demektir. Sıra zorunlu: artış gönderimden sonra olsaydı tuşun
    /// kendi aynası bir önceki nesille damgalanabilir ve kapı onu sonsuza
    /// kadar "cevapsız" sayardı.
    ///
    /// `screen_clears`'ın emsali ama yönü ters: orada okuyucu yazıyor, kare
    /// yolu okuyor; burada ana thread yazıyor, ikisi okuyor. Tek yazar
    /// kuralı aynı. `Adapter::reply` ve tekerlek raporu buradan geçmiyor ve
    /// geçmemeli — onlar kullanıcının yazdığı bir şey değil.
    key_gen: Arc<AtomicU64>,
    /// İlk girdi ([`SessionOptions::initial_input`]) bizim ilk kimlikli
    /// `A`'mızı beklerken **tutulan** kullanıcı girdisi; `None` → tutma yok
    /// ([`HeldInput`]).
    held_input: HeldInput,
    /// [`Session::screen_clears`]'in kare yolunun **hesaba kattığı** hâli.
    ///
    /// İkisi ayrıştığı anda ortada henüz sindirilmemiş bir temizleme var
    /// demektir ve o kare bayrağı **kurar** (birincil ekranda); eşitken
    /// bayrağın ömrü ızgaraya **ve deftere** bakar. Sayacı burada tutmanın sebebi tam olarak bu: bir `bool` "ekran
    /// temizlendi mi" sorusunu yanıtlar, nesil ise "bu temizlemeyi hesaba
    /// kattım mı" sorusunu — ve yarışı kapatan ikincisi.
    screen_seen: AtomicU32,
    /// Ekran **kasten** temizlendi ve defter o temizlemeden sonra henüz
    /// büyümedi (R1.1, R1.2; ekranın doğal yoldan dolması ikinci kol).
    ///
    /// **Tek tüketicisi [`Session::fill_rows`]** ve okuma yeri sıranın kendisi:
    /// ömür bu karede işledikten **sonra**, yani aynı karede gelmiş taze bir
    /// `CSI 2 J` doldurmayı da kapatıyor.
    ///
    /// `screen_seen` ile birlikte tek yazıcısı [`Session::frame`] ve yazma
    /// `Term` kilidi tutulurken oluyor, yani okuma-değiştirme-yazma turunu
    /// serileştiren şey kilidin kendisi — `Relaxed` bu yüzden yetiyor
    /// ([`Session::alt_screen`] emsali).
    screen_cleared: AtomicBool,
    /// Bayrak kurulduğunda defterin boyu — düşme ölçütünün **damgası** (R1.2).
    ///
    /// Soru tek: "temizlemeden **sonra** geçmişe satır düştü mü". Düştüyse
    /// geçmişin en yeni satırları artık temizleme öncesine ait değildir, yani
    /// doldurma Ctrl-L'i geri almaz ve bayrağın işi bitmiştir.
    ///
    /// **Damga bir kare geç alınıyor** ([`Session::UNSTAMPED`]) ve bu bir
    /// zamanlama zorunluluğu: okuyucu thread nesli `advance`'ten **önce**
    /// artırıyor, yani bayrağı kuran kare ızgarayı temizlenmeden **önce**
    /// görebiliyor. O karenin defter boyu temizleme öncesine ait olurdu ve
    /// `CSI 2 J`'nin kendisi görünen satırları geçmişe ittiği için
    /// (alacritty `clear_viewport`) bir sonraki karede damga **anında**
    /// aşılır, bayrak da hemen düşerdi. Damgayı nesil yerleştikten sonraki
    /// ilk kareye bırakmak bunu kapatıyor; bedeli o karede gelmiş fazladan
    /// satırların damgayı biraz yükseltmesi, yani bayrağın biraz **uzun**
    /// yaşaması — yanlışın yönü güvenli.
    screen_clear_history: AtomicUsize,
    /// Son `CSI 2 J`'nin geçmişe ittiği **en yeni** satırın kimliği
    /// ([`row_identity`]); `0` → yok.
    ///
    /// Blok işaretinin devam satırı kuralının ([`block_row_continues`])
    /// istisnası: Ctrl-L aynı prompt'u **aynı kimlikle** yeniden basıyor ve
    /// temizleme eski prompt satırını geçmişe itiyor, yani yeni prompt'un
    /// üstünde aynı kimliği taşıyan bir satır duruyor — ama o satır komutun
    /// başı değil, temizlenmiş ekranın kalıntısı (`/code-review`, 032 kapı).
    /// Kimlik satırın tamponu, sayı değil: halka kaydırmada tamponu yerinde
    /// tutuyor. **Bilinen sınır:** defter doyunca tampon yeni bir satıra
    /// yeniden verilebilir ve o satır bir devam satırıysa işaret ona oturur
    /// (kuraldan önceki hâl, yönü görünür).
    clear_boundary: AtomicUsize,
    /// Dibe yaslı pencerede ekranda duran doldurma bandının boyu
    /// ([`Cursor::fill`]) — kaydırma yolunun okuduğu tek kare kalıntısı.
    ///
    /// **Neden saklanıyor:** bant görünürken ekran, `display_offset == fill`
    /// olan bir pencereyle aynı satırları gösteriyor (bandın tepesi
    /// `Line(-fill)`, dibi içeriğin son satırı), yani bant bir **sanal
    /// kaydırmadır**. Tekerlek bunu bilmezse ilk çentik `display_offset`'i
    /// `1`'e taşır ve ekran bir satır yukarı değil `fill - 1` satır **aşağı**
    /// sıçrar. [`scroll_locked`] bu değeri kaydırmanın başlangıç noktası
    /// sayıyor.
    ///
    /// **Neden yeniden hesaplanmıyor:** `fill`'in girdisi doluluk sayısı ve
    /// o, ızgaranın bütün hücrelerini gezen taramadan doğuyor
    /// ([`Session::frame`]). Tekerlek yolunda ikinci bir tarama hem pahalı
    /// hem ikinci bir doğruluk kaynağı olurdu; ekranda **duran** sayı zaten
    /// kullanıcının baktığı sayı.
    ///
    /// Yalnız `display_offset == 0` olan kareler yazıyor; gerekçe yazan
    /// yerde.
    fill_shown: AtomicU16,
    /// Bir önceki dibe yaslı karenin **ekran tepesindeki satırın kimliği**
    /// ([`row_identity`]); `0` "karşılaştırılacak kare yok" demek.
    ///
    /// Kaydırma sayısının ([`Cursor::scrolled`]) tek girdisi ve ölçüt
    /// defterin boyu **değil**: `history_size()` `scrollback`'te doyuyor ve
    /// o andan sonra her kaydırmada sabit kalıyor, yani ondan türeyen bir
    /// sayı uzun bir oturumda sessizce sıfıra inerdi — kullanıcının bildirdiği
    /// kusurun ("ızgara dolunca kayma yok oluyor") aynı şekli, bu sefer
    /// on bin satır sonra. Satırın kimliği doymuyor: alacritty'nin defteri bir
    /// halka ve kaydırma satırları taşımıyor, halkanın başını çeviriyor
    /// (`Storage::rotate`), yani bir satırın hücre tamponu geçmişe düşerken
    /// yerinde kalıyor ve kimliği onunla birlikte `Line(-k)`'ya iniyor.
    scroll_probe: AtomicUsize,
    /// [`Session::scroll_probe`]'u alan karenin ızgara boyutu,
    /// `rows << 16 | cols`. Boyut değiştiyse karşılaştırma yapılmıyor: pencereyi
    /// küçültmek satırları geçmişe itiyor ve bu bir kaydırma değil.
    scroll_probe_size: AtomicU32,
    /// Izgaranın tepesi **çizildiği hâliyle** pencerenin tepesinden kaç satır
    /// aşağıda (yukarı yuvarlanmış) — çizen tarafın bildirdiği tek sayı
    /// ([`Session::set_grid_top`]).
    ///
    /// Doldurma bandının geçici uzantısının ölçüsü: kayma uçuştayken ızgara
    /// hedefinin altında duruyor ve üstünde açılan şerit ancak bu sayı
    /// bilinirse geçmişle kapatılabiliyor. Animasyonun kendisi `bt-gpu`'da ve
    /// bu crate onu görmüyor; gördüğü şey bir satır sayısı.
    grid_top: AtomicU16,
    /// `grid_top`'un **bandın kısalığından** gelen payı: dock bandı PTY
    /// payından kısa çiziliyor (uzak oturumda yalnız bağlam satırı, 036 Karar
    /// 8) ve ızgara o kadar aşağıda — geçici bir kayma değil, uzak oturum
    /// boyunca kalıcı. Ayrı sayı, çünkü kaydırılmış pencerede de kapatılmak
    /// zorunda ve orada `grid_top`'un geri kalanı (öteleme boşluğu) doldurulmamalı
    /// ([`Session::slide_fill_rows`]).
    grid_lowered: AtomicU16,
    /// Kaydırmanın **kesri**: `[0, 1)` satır, `f64` bitleri ([`Cursor::scroll_frac`]).
    ///
    /// Kaydırma konumunun **tek** yeni parçası ve bilerek göreli: tam satırın
    /// tek yetkilisi yine `display_offset` ve onu yalnız [`scroll_locked`]
    /// oynatıyor, bant eşlemesiyle birlikte. Mutlak bir konum tutulsaydı
    /// `display_offset`'in dört dış yazıcısı (girdide dibe dönüş, Shift+PgUp,
    /// geçmişteyken gelen çıktı, resize) her karede ezilirdi
    /// (`discussion.md` → Muhakeme). Kesrin yönü **geriye**: ızgara kesir
    /// kadar aşağı çiziliyor ve tepede açılan şeridi ızgaranın hemen
    /// üstündeki satır kapatıyor ([`Cursor::top_row`]).
    ///
    /// **Kilit rejimi `Term` kilidi**: okuyan ve yazan her yol (tekerlek, kare
    /// yolunun payı, dibe dönüş, sayfa) onu tutarken dokunuyor, yani
    /// okuma-değiştirme-yazma turunu kilit serileştiriyor ve `Relaxed`
    /// yetiyor ([`Session::screen_cleared`] emsali). Atomik yalnız `Sync`
    /// için; `Term`'ün yanında durmamasının sebebi alacritty'nin tipi.
    scroll_frac: AtomicU64,
    /// Bekleyen **süzülme isteği** ve **kaydırma nesli**, tek kelimede
    /// ([`ScrollGlide::pack`]).
    ///
    /// İstek çentiğin ve yerleşmenin birikimi (satır, işaretli); kare yolu
    /// onu alıp sıfırlıyor ([`Session::take_scroll_glide`]) ve animatörü
    /// üstünden kare kare teslim ediyor. Nesil konumun **dışarıdan**
    /// sıfırlandığı her seferde artıyor: girdide dibe dönüş, Shift+PgUp,
    /// satır adımı, jest ya da momentum başı ve kare yolunun normalleştirmesi.
    ///
    /// **Tek kelime olmasının sebebi yarış**, stil değil. Yazanlar olay yolu
    /// (`Term` kilidi altında) ve kare yolu `Term` kilidine girmeden alıyor;
    /// iki ayrı atomik olsaydı dibe dönüşün "isteği düşür, nesli artır"ı ile
    /// kare yolunun "isteği al, neslini oku"su arasına düşen bir alım yeni
    /// nesle ait bir çentiği eski nesille etiketler ve çentik kaybolurdu.
    /// Yaprak kilit de olamaz: yazanlardan biri `Term`'ü tutuyor (modül
    /// kuralı). `fetch_update` üç yolu da tek atomik adıma indiriyor.
    scroll_glide: AtomicU64,
    /// Pencerenin dock'u var mı ([`SessionOptions::dock`]).
    ///
    /// Doğumda kararlaşıyor ve bir daha değişmiyor, o yüzden ne kilit ne
    /// atomik: `[shell] integration` **sonraki oturumda** geçerli (`CLAUDE.md`)
    /// ve dock'un varlığı ona bağlı.
    dock: bool,
    /// [`SessionOptions::cluster`]; `dock` gibi doğumda kararlaşıyor. Sınır
    /// hücresinin kümesi ([`Cell::cluster`]) yalnız açıkken doğuyor.
    cluster: bool,
    /// [`SessionOptions::home`]; yalnız [`Session::title`] okuyor.
    home: Option<PathBuf>,
    /// PTY'nin çocuğunun pid'i ([`Session::child_pid`]). Doğumda alınıyor,
    /// çünkü `Pty` bir kez [`TappedPty`]'ye sarılınca okuyucu thread'ine
    /// gidiyor ve bu yakadan bir daha görülmüyor.
    child_pid: u32,
}

impl Session {
    /// [`Session::screen_clear_history`]'nin "henüz damgalanmadı" değeri.
    ///
    /// Sentinel bir **tavan** ve seçim bilerek: damgasız bir bayrak
    /// `history > stamp` karşılaştırmasını hiçbir defter boyuyla geçemiyor,
    /// yani ayrı bir dal olmadan "damga gelene kadar düşme" anlamına geliyor.
    /// Gerçek bir defter boyu olamaz — tavanı `SCROLLBACK_MAX`.
    const UNSTAMPED: usize = usize::MAX;

    /// PTY'yi açar, shell'i başlatır ve okuyucu thread'i kurar.
    pub fn spawn(options: SessionOptions, wake: Arc<dyn Wake>) -> io::Result<Self> {
        let grid = GridSize::for_spawn(options.cols, options.rows);
        let size = window_size(grid, options.cell_px);

        // `tty::setup_env()` ÇAĞRILMAZ: o, kendi sürecimizin ortamını
        // `set_var` ile değiştirir ve makinede alacritty kuruluysa
        // `TERM=alacritty` yazar. Ortamı çocuğa doğrudan veriyoruz.
        //
        // Sıra öncelik sırasıdır (`SessionOptions::env`): ek ortamın
        // haritasına iki sabit **sonra** giriyor — aynı anahtarı ezen onlar.
        let mut env = options.env;
        env.insert("TERM".to_owned(), "xterm-256color".to_owned());
        env.insert("COLORTERM".to_owned(), "truecolor".to_owned());
        // Kimlik ailesi aynı katmanda (038). Ezilemez olmasının yan kazancı:
        // başka bir terminalden miras kalan `TERM_PROGRAM=Apple_Terminal`,
        // `/etc/zshrc` üzerinden Apple'ın oturum betiğini sarmalayıcının
        // `ZDOTDIR`'ına yazdırıyordu (`.tasks/038-terminal-kimligi/context.md`
        // → Kanıt).
        env.insert("TERM_PROGRAM".to_owned(), TERM_PROGRAM.to_owned());
        env.insert(
            "TERM_PROGRAM_VERSION".to_owned(),
            TERM_PROGRAM_VERSION.to_owned(),
        );
        if let Some(id) = &options.tab_id {
            env.insert("TERM_SESSION_ID".to_owned(), id.as_str().to_owned());
            env.insert("BATERI_TAB_URL".to_owned(), id.url());
        }
        let pty_options = tty::Options {
            shell: options
                .command
                .map(|(program, args)| Shell::new(program, args)),
            working_directory: options.working_directory,
            env,
            ..Default::default()
        };
        let home = options.home;
        let pty = tty::new(&pty_options, size, 0)?;
        let child_pid = pty.child().id();
        // Yuva `EventLoop`'tan **önce** doğuyor: bir ucu sarmalayıcıyla okuyucu
        // thread'ine gidiyor, öteki ucu `Session`'da kalıyor.
        // Defterin tavanı `scrollback`'ten: blok başına en az bir satır düştüğü
        // için geçmişte görünebilecek blok sayısının üst sınırı odur.
        let scrollback = options.terminal.scrollback;
        let mut log = ShellLog::new(scrollback);
        // Tarayıcının kopyası her aynada bunun üstüne yazılıyor; ilk aynadan
        // önce de aynı okunuş olsun.
        log.dock.cluster = options.cluster;
        let shell = Arc::new(Mutex::new(log));
        // Sayacın da iki ucu var ve ikisi de aynı gerekçeyle burada doğuyor.
        let screen_clears = Arc::new(AtomicU32::new(0));
        let key_gen = Arc::new(AtomicU64::new(0));
        // **Blink de açılışta geçiyor**, temanın yanında: tek yazıcısı
        // `set_terminal_options` olsaydı ayar yalnız oturum içinde bir kayıttan
        // **sonra** uygulanır, taze pencerede sessizce yok sayılırdı.
        //
        // `TappedPty`'den **önce**: ilk girdi onun kanalından gidiyor.
        let adapter = Adapter::new(
            Arc::clone(&wake),
            size,
            options.theme,
            options.terminal.blink,
        );
        // İlk girdinin iki teslim yolu (037 Karar 6): sarmalayıcılı oturumda
        // okuyucu thread'i bizim ilk kimlikli `A`'mızda, sarmalayıcısızda
        // aşağıda, doğumda. Boş satır hiç yok: sıfır baytlık bir `Input`
        // yazıcıyı kilitlerdi (`Adapter::reply`) ve `\r` tek başına boş bir
        // komut koştururdu.
        let initial_input = options.initial_input.filter(|line| !line.is_empty());
        let (at_prompt, at_birth) = if options.shell_marks {
            (initial_input, None)
        } else {
            (None, initial_input)
        };
        // Tutma yalnız ilk girdi **prompt'u beklerken** (Karar 6'nın
        // sarmalayıcılı kolu); doğumda yazılan satır zaten her tuştan önce.
        let held_input: HeldInput = Arc::new(Mutex::new(at_prompt.is_some().then(Vec::new)));
        let pty = TappedPty {
            pty,
            scanner: Scanner::new().cluster(options.cluster),
            shell: Arc::clone(&shell),
            screen_clears: Arc::clone(&screen_clears),
            key_gen: Arc::clone(&key_gen),
            wake,
            initial_input: at_prompt,
            held_input: Arc::clone(&held_input),
            adapter: adapter.clone(),
        };

        let config = term_config(options.terminal);
        let term = Arc::new(FairMutex::new(Term::new(config, &grid, adapter.clone())));

        let event_loop = EventLoop::new(
            Arc::clone(&term),
            adapter.clone(),
            pty,
            pty_options.drain_on_exit,
            options.cluster,
        )?;
        let sender = event_loop.channel();
        // Kanal ancak burada doğar; adapter'ın kopyaları aynı gövdeyi
        // paylaştığı için tek `set` hepsini bağlar.
        let _ = adapter.0.sender.set(sender.clone());

        let session = Self {
            term,
            sender,
            adapter,
            reader: Mutex::new(Some(event_loop.spawn())),
            shell,
            // Açılışta alternatif ekran yok; ilk içerik karesi zaten yazacak.
            alt_screen: AtomicBool::new(false),
            caret_in_dock: AtomicBool::new(false),
            search: Mutex::new(SearchSlot::default()),
            scrollback: AtomicUsize::new(scrollback),
            user_scroll: AtomicI64::new(0),
            dock_window: Mutex::new(None),
            screen_clears,
            key_gen,
            held_input,
            // Açılışta sindirilmemiş temizleme yok: sayaç da, hesaba katılan
            // nesil de sıfır. Üçünü de sıfırdan başlatmak, ilk karenin
            // bayrağı sebepsiz kurmasını önlüyor.
            screen_seen: AtomicU32::new(0),
            screen_cleared: AtomicBool::new(false),
            // Damga da yok: bayrak kurulu olmadığı için okunmuyor, ilk kare
            // onu defterin boyuyla değiştiriyor.
            screen_clear_history: AtomicUsize::new(Self::UNSTAMPED),
            clear_boundary: AtomicUsize::new(0),
            // Bant da yok: ilk kare doldurmayı hesaplayıp yazacak.
            fill_shown: AtomicU16::new(0),
            // Karşılaştırılacak kare yok: ilk kare kimliği yazıp sıfır döner.
            scroll_probe: AtomicUsize::new(0),
            scroll_probe_size: AtomicU32::new(0),
            // İlk karede kayma yok, ızgara hedefinde.
            grid_top: AtomicU16::new(0),
            grid_lowered: AtomicU16::new(0),
            // Açılışta pencere dipte ve tam satırda; istek yok, nesil sıfır.
            scroll_frac: AtomicU64::new(0),
            scroll_glide: AtomicU64::new(0),
            dock: options.dock,
            cluster: options.cluster,
            home,
            child_pid,
        };
        // Sarmalayıcısız oturumun ilk girdisi: kabuğun typeahead'i, kullanıcı
        // girdisinin **aynı** yolundan (nesil dahil). Taze oturumda seçim ve
        // kaydırma yok, yani `send_input`'un öteki işleri no-op.
        if let Some(mut line) = at_birth {
            line.push('\r');
            session.write_owned(line.into_bytes());
        }
        Ok(session)
    }

    /// Çizilecek kareyi verir — **koşulsuz tarar**, hasar sormaz.
    ///
    /// Hasar sorusu [`Session::take_damage`]'de ve bilerek ayrı: çağıran
    /// tarama başlamadan **önce** karar vermek zorunda. Sebep hareket karesi
    /// (`bt-gpu`) — grid kirli değilken de çizilen bir kare var ve o yol
    /// çizim listesini **temizlemeden** kullanıyor; iki soru tek çağrıda
    /// kalsaydı liste temizlendikten sonra "aslında hasar yokmuş" öğrenilir
    /// ve hareket karesi boş bir ekrana bakardı.
    ///
    /// İkisi **birlikte** çağrılır ve sıra zorunlu: `take_damage()` `true`
    /// derse `frame()`. Tersi (hasarsız `frame()`) yanlış değil ama boşuna —
    /// `Term` kilidini alır ve aynı kareyi yeniden kurar.
    ///
    /// `Term` kilidi bir kez alınır; temanın kopyası ondan önce. Hasar
    /// "çizilsin mi"ye karar verir, "ne çizileceğine" değil: drawable içeriği
    /// korunmadığı için her karede tam grid taranır.
    ///
    /// Dönüşteki [`Cursor`] imlecin yerini **ve** bloğunun altında kalan
    /// metnin rengini taşır: **karar burada, boyama orada**. Hücreler imleci
    /// hiç bilmiyor — hiçbiri onun yüzünden rengini değiştirmiyor — ve
    /// bloğun örttüğü pikselleri çizen eziyor. Ayrımın sebebi hücrenin
    /// bölünemeyişi: blok iki hücre arasındayken (008) sınır hücrenin
    /// ortasından geçer ve burada verilecek bir hücre kararı o sınırı
    /// göremez.
    ///
    /// `sink` jeneriktir: hücre başına dinamik çağrı yerine satır içine
    /// alınır. **`Term` kilidi tutulurken** çağrılır ve kilit yeniden girilebilir
    /// değildir: `Session`'a geri giren bir sink (`resize`, `frame`) kendi
    /// kendini kilitler. Sink'in işi tamponu doldurmaktır, başka bir şey değil —
    /// `Wake` ile aynı sözleşme.
    ///
    /// **`fill_sink` ikinci ve ayrı bir kanaldır** (R2.1): üstte kalan boşluğu
    /// dolduran geçmiş satırları oradan geçiyor, satır numaraları
    /// **fill-yerel** (`0..fill`) ve sayısı [`Cursor::fill`]'de. Ayrı olması
    /// zorunlu — dock örüntüsü: doldurma kendi listelerine giriyor, ızgaranın
    /// sayaçlarına (`hucre=`/`glif=`/`kural=`) **girmiyor** (R3.2) ve satır
    /// numaraları ızgaranınkilerle çakıştığı için tek kanalda ayırt
    /// edilemezlerdi. [`Session::fill_rows`] sıfır derse hiç çağrılmıyor.
    ///
    /// `blocks` **iki fazlıdır** ve sırası zorunlu. Faz 1, `Term` kilidi
    /// altında: prompt hücrelerinin OSC 8 çıpasından blok kimliği çekilir ve
    /// `(kimlik, ilk satır)` çiftleri toplanır. Faz 2, kilit **bırakıldıktan
    /// sonra**: kimlikler kabuk defterinden renklendirilir. Ters sıra bu
    /// modülün yazılı kuralını çiğnerdi — yaprak kilit (`shell`) `Term`
    /// kilidinin altına girmez.
    ///
    /// `selection` seçimin **satır koşularını** ve iki rengini alıyor
    /// ([`SelectionRuns`]); `blocks` gibi her karede boşalıp doluyor. Koşular
    /// ızgaranın ekran satırlarında, bastırılan giriş satırı hariç — doldurma
    /// bandı seçilemiyor, dock'un seçimi ayrı.
    ///
    /// **`glide` kare yolunun bu karede teslim ettiği süzülme payı** (027): aynı
    /// `Term` kilidinde, tarama başlamadan uygulanıyor ve **uyandırmıyor** —
    /// animasyonun kare talebi `Waker::wake`'ten geçemez (`CLAUDE.md` → Boşta
    /// sıfır kare), kareyi zaten çizen taraf istiyor. Ayrı bir çağrı olsaydı
    /// kare başına ikinci bir kilit turu olurdu. Nesli güncel nesilden farklı
    /// pay düşüyor ([`ScrollGlide`]); sıfır pay kareyi bugünküyle aynı
    /// bırakıyor.
    ///
    /// **`budget` dock'un giriş bloğuna ayrılabilecek yer** ([`DockBudget`]):
    /// tavan ve sarma genişliği çizen tarafın yerleşim kararı, bu crate
    /// yalnız sayıyı kırpıyor ve [`Cursor::input_rows`] olarak sınırdan
    /// veriyor.
    ///
    /// **`search` arama vurgusunun koşuları** ([`SearchRuns`], 033): arama
    /// etkinken çizilen satırların eşleşmeleri, ızgara ve doldurma kanalı
    /// ayrı listelerde; desen yoksa iki liste de boş ve tarama koşmuyor.
    // Argümanlar karenin tek okumasından geçen iki sink ve çağıranın
    // tamponları (`Session::dock` emsali); bir yapıda toplamak yalnız bu
    // çağrı için bir tip doğururdu.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &self,
        mut sink: impl FnMut(Cell),
        mut fill_sink: impl FnMut(Cell),
        blocks: &mut Blocks,
        selection: &mut SelectionRuns,
        search: &mut SearchRuns,
        clusters: &mut Clusters,
        glide: ScrollGlide,
        budget: DockBudget,
    ) -> Cursor {
        // Tema `Term` kilidinden **önce** ve kopya olarak: yaprak kilit
        // kare boyunca tutulmaz, `Term` kilidinin altına ikinci bir muteks
        // girmez. Kopya ile kilit arasına düşen bir takas en çok bir kare
        // eski renkle çizer; takası yazan zaten kare istiyor.
        let theme = *lock(&self.adapter.0.theme);
        // Temanın komşusu, aynı turda ve aynı gerekçeyle: yaprak kilit `Term`'ün
        // altına girmez.
        let blink = *lock(&self.adapter.0.blink);
        let background = theme.background_rgb();
        // **Bastırma kararı da `Term` kilidinden önce** ve temayla aynı
        // gerekçe: yaprak kilit (`shell`) `Term` kilidinin altına girmez
        // (modül başlığı). Tek okuma, çünkü safha ile aynanın durumu tek
        // yüklemde birleşiyor (`ShellLog::suppressed_input`); ayrı
        // okumalardan alınsalardı safha `Input`, ayna `Live` görünür ve ikisi
        // **aynı ana** ait olmazdı.
        //
        // **Sağladığı şey bu kadar ve fazlası iddia edilmiyor:** `frame()` ile
        // [`Session::dock`] aynı kareyi çizerken yaprak kilidi **ayrı ayrı**
        // alıyor (`bt_gpu::link`). İkisinin arasına düşen bir `line-finish`
        // ızgarası bastırılmış, dock'u boşalmış **bir** kare doğurur —
        // kullanıcının Enter'a bastığı an. Bir karelik ve kapatmanın yolu iki
        // çağrıyı tek kilit turuna indirmek, yani `bt-gpu` sınırını
        // değiştirmek; bilinen sınır olarak duruyor (012 phase-4).
        //
        // Kopya ile `Term` kilidi arasına düşen bir işaret de en çok bir kare
        // eski kararla çizer; işareti yazan zaten kare istiyor.
        //
        // **Caret'in sahibi aynı turdan**: ayrı bir `lock()` ile sorulsaydı iki
        // cevap iki ana ait olurdu ve aralarına düşen bir `line-finish` imleci
        // gizlenmiş **ve** dock'u boşalmış bir kare doğururdu.
        //
        // **Görüntünün metni de aynı turdan** (032): bastırmanın satır
        // aritmetiği onu `Term` kilidinin altında yürüyor ([`dock::grid_span`])
        // ve ayrı bir turdan alınsaydı sayılar başka bir aynaya ait olurdu.
        // Yalnız bastırılan bir satır varken kopyalanıyor.
        //
        // **Dock'un satır sayısı da aynı turdan** (032 phase-3): bant, bastırma
        // ve caret aynı aynaya ait olmalı. Yürüyüş dock parametrizasyonu
        // ([`dock::needed_rows`]; asma girinti, ızgaranın genişliği) ve
        // bastırmanın ızgara parametrizasyonundan **ayrı** — sarılan satırda
        // ikisi ayrışıyor (dock iki sütun girintili sarıyor, zsh 0. sütundan),
        // biri ötekinin yerine kullanılamaz. Metin gerektirmiyor, kopya yok.
        //
        // **Tutulan `line-finish` de bu turun başında çözülüyor** (032 Karar
        // 11, [`crate::shell::ShellLog::expire_end`]): süresi dolduysa ayna
        // burada sıfırlanıyor, yani bastırma, bant ve caret aynı aynayı
        // görüyor; dolmadıysa kalan saate giriyor.
        //
        // **Uzak oturum da bu turdan** (036 Karar 8): giriş satırı sayısı ve
        // caret'in sahibi ([`crate::shell::ShellLog::caret`]) aynı uzak
        // durumu görmeli — ayrı turlarda araya düşen bir `D` sıfır satırlık
        // bir bantta dock caret'i doğururdu.
        let (suppressed_block, caret, needed_rows, end_left, remote) = {
            let now = Instant::now();
            let mut log = lock(&self.shell);
            let end_left = log.expire_end(now);
            let suppressed = log.suppressed_input();
            if suppressed.is_some() {
                blocks.input_caret = log.display_into(&mut blocks.input);
                blocks.input_cluster = log.dock.cluster;
            }
            (
                suppressed,
                log.caret(now),
                dock::needed_rows(&log.dock, budget.cols),
                end_left,
                log.context.remote.is_some(),
            )
        };
        blocks.anchors.clear();
        blocks.resolved.clear();
        blocks.fill_anchors.clear();
        blocks.fill_resolved.clear();
        selection.runs.clear();
        selection.color = theme.selection_linear();
        selection.unfocused = theme.selection_unfocused_linear();
        // **Aramanın deseni ödünç, `Term` kilidinden önce** ([`SearchSlot`]):
        // `RegexIter` `&mut` istiyor ve yaprak kilit `Term`'ün altına girmez.
        // Desen yoksa (arama kapalı, sorgu boş ya da geçersiz) tarama hiç
        // koşmuyor ve iki liste boş kalıyor (R2.2).
        search.clear();
        search.colors = search::SearchColors::of(&theme);
        let (search_generation, mut search_pattern, mut search_tracking) = {
            let mut slot = lock(&self.search);
            (slot.generation, slot.pattern.take(), slot.tracking())
        };
        let search_taken = search_tracking.mark;
        let mut term = self.term.lock();
        // **Süzülme payı taramadan önce**: aşağıdaki her şey (ofset, bayrağın
        // ömrü, doldurma, kayma sayısı) payın taşıdığı pencereyi görmeli.
        // Nesil kilidin altında okunuyor, çünkü onu artıran yollar (dibe
        // dönüş, sayfa) da kilidi tutuyor — sıfırlamadan önce hesaplanmış bir
        // pay sıfırlamadan sonra uygulanamaz.
        let generation = ScrollGlide::unpack(self.scroll_glide.load(Ordering::Relaxed)).generation;
        if glide.rows != 0.0 && glide.generation == generation {
            let frac = f64::from_bits(self.scroll_frac.load(Ordering::Relaxed));
            let _ = self.scroll_fraction(&mut term, frac, f64::from(glide.rows), self.band_shown());
        }

        let rows = term.screen_lines() as i32;
        // Alternatif ekranda blok **yok**: vim'in tamponunda prompt da komut da
        // yok, oradaki satırlar hiçbir bloğa ait değil. Bayrak kilidin altında
        // okunup faz 2'ye taşınıyor; yalnız çıpa toplamayı kapatmak yetmezdi,
        // çıpasız pencerenin geri düşüşü (aşağıda) tam ekranı boyardı.
        let alt_screen = term.mode().contains(TermMode::ALT_SCREEN);
        // Bayrak sınırın öteki tarafına da **buradan** geçiyor
        // ([`Session::alt_screen`]): kilidi zaten elimizde ve okunan değer tam
        // da bu karenin `content_rows`'uyla tutarlı olan değer. İkinci bir
        // sorgu kilidi kare başına bir kez daha alırdı.
        //
        // **`swap` çünkü düşen kenarın da bir tüketicisi var**: alternatif
        // ekrandan çıkışta imlecin stili kullanıcının tabanına dönüyor.
        let was_alt = self.alt_screen.swap(alt_screen, Ordering::Relaxed);
        if was_alt && !alt_screen {
            // **Uygulama bitti, bıraktığı imleç durumu da bitsin.**
            // `set_cursor_style(None)` DECSCUSR'ın kaydını siliyor ve
            // `Term::cursor_style()` `config.default_cursor_style`'a, yani
            // `[terminal] cursor` + `cursor_blink`'in tabanına düşüyor.
            //
            // Gerekçe ölçüldü (2026-09-20, kullanıcı bildirdi): `cursor_blink
            // = "auto"` açılışta sönen imleci vim'den **bir kez** geçtikten
            // sonra kalıcı olarak kaybediyordu. Suçlu DECSCUSR değil
            // terminfo: `xterm-256color`'da `cnorm = \e[?12l \e[?25h`, yani
            // "imleci normal görünür yap" komutunun **içinde** blink'i kapatan
            // özel mod 12 var. vim, less, man, htop — `cnorm` gönderen her
            // program çıkarken blink'i öldürüyor ve geri açan kimse yok.
            // Ölçüm: vim'in bütün oturumu 160 bayt ve içinde `\e[?12h` ile
            // `\e[?12l` var, DECSCUSR **hiç** yok.
            //
            // alacritty bunu kendiliğinden yapmıyor ve bu onun bilinçli
            // tercihi: `cursor_style` `Term` seviyesinde tek bir alan,
            // `swap_alt` ona hiç dokunmuyor (0.26.0, `term/mod.rs:714`).
            // Yani kural host'un, ve yeri burası.
            //
            // **Şekil de resetleniyor**, yalnız blink değil: ölçüt "uygulama
            // bitti" ve ikisi de aynı alanın parçası. Bedeli bir prompt'luk —
            // zsh'in vi-kipi stilini `zle-line-init`'te yeniden gönderiyor.
            term.set_cursor_style(None);
        }
        let RenderableContent {
            display_iter,
            cursor,
            display_offset,
            colors,
            ..
        } = term.renderable_content();
        let offset = display_offset as i32;

        // İmleç döngüden **önce** çözülüyor, ama artık hücrelerin rengi için
        // değil: blok altındaki metin bir piksel işi oldu (bkz. [`Cursor`]) ve
        // karar hücre başına değil kare başına bir kez veriliyor. Döngünün
        // imleçten hâlâ istediği tek şey `contains_cell`'in sorduğu blok
        // imleç istisnası, o yüzden şekil ve grid noktası ayrı tutuluyor:
        // `Cursor` ikisini de taşımıyor. İstisna **imlecin durduğu** hücre
        // içindir — oraya hücrenin kendi noktası verilince her seçimin ilk ve
        // son hücresi vurgusuz kalıyordu.
        let cursor_shape = cursor.shape;
        // **Blink iki kaynağın birleşimi ve birleşme yeri burası.**
        // `RenderableCursor` blink bitini taşımıyor (yalnız `shape` ve
        // `point`), yani `cursor_style()` ayrıca soruluyor — phase-1'in şekli
        // oradan almamasının sebebi de buydu, o değer zaten çözülmüş geliyor.
        // Kullanıcının ayarı **sonra** uygulanıyor: `"on"`/`"off"` birer ezme
        // ve config'e yazılamıyorlar (`AdapterInner::blink`).
        let requested_blink = blink.resolve(term.cursor_style().blinking);
        let cursor_point = cursor.point;
        let cursor_row = cursor.point.line.0 + offset;
        let cursor_col = cursor.point.column.0 as u16;
        // Kırpılmış ekran satırı; iki tüketicisi var ve ikisi de kırpılmışını
        // istiyor — [`Cursor::row`]'un sözleşmesi ve aşağıdaki doluluk sayısı.
        let cursor_screen_row = cursor_row.clamp(0, rows.saturating_sub(1)) as u16;
        // Kaydırma geçmişine bakarken imleç ekranın dışına çıkar.
        let cursor_visible = cursor.shape != CursorShape::Hidden && (0..rows).contains(&cursor_row);
        // Izgara yüksekliği `u16` olarak giriyor (`GridSize::for_spawn` 1'e
        // kırpar, `Session::resize` sıfırı eler), yani kesme kayıpsız ve
        // değer **en az 1**: `content_rows`'un `1..=rows` sözleşmesi buradan.
        let grid_rows = u16::try_from(rows).unwrap_or(u16::MAX);
        // Aynı gerekçeyle genişlik: süre sayacı sağa yaslanıyor ve ölçüsü
        // `Term` kilidi düştükten sonra gerekiyor, yani buradan taşınıyor.
        let grid_cols = u16::try_from(term.columns()).unwrap_or(u16::MAX);
        // Doluluk sayısının çizilen yarısı; döngü onu atlama kapısından
        // **sonra** büyütüyor (bkz. aşağıda).
        let mut drawn_rows = 0u16;
        // Bastırmanın **üst** ucu: yazılmakta olan bloğun çıpasını taşıyan ilk
        // satır. Döngü içinde doğuyor, çünkü cevabı ancak ızgara biliyor —
        // hangi satırda olduğu kabuğun bildiği bir şey değil, her karede
        // çıpadan okunuyor (010'un tezi; kaydırma ve reflow sonrası da doğru
        // kalmasının sebebi bu).
        //
        // Çıpası pencerede **görünmeyen** blok `None` bırakır ve bastırma hiç
        // koşmaz: geriye kaydırılmış bir pencerede giriş satırı zaten
        // görünmüyor, görünüyorsa da eksik değil fazla göstermek güvenli olan.
        let mut suppress_from: Option<u16> = None;
        // Bastırmanın **alt** ucu. İmlecin satırı **yetmiyor**: ZLE caret'i
        // tamponun içinde serbestçe gezdiriyor ve sarmalı bir satırda Ctrl-A
        // ya da yukarı ok imleci ilk satıra alınca kuyruk aşağıdaki
        // satırlarda kalır — dock bütün tamponu gösterirken ızgara kuyruğu
        // gösterir, yani phase'in kapatmaya geldiği çift görüntü geri döner
        // ve **kalıcı** olur (`/code-review`, phase-4).
        //
        // Uç kesin veriden çıkıyor, davranıştan sezilmiyor: caret'in sütunu
        // ızgaradan, metin aynadan, ve ikisini birleştiren **tek düzen
        // yürüyüşü** ([`dock::grid_span`] → [`dock::layout`], 032 Karar 7):
        // görüntü zsh'in ızgaradaki düzeniyle yürünüyor — ilk satır prompt'un
        // bittiği sütundan (imlecin sütunundan gözleniyor), sarma ızgaranın
        // genişliğinde — ve imlecin üstünde ve altında kaç satır kaldığı
        // oradan okunuyor. Tam dolan satırın bir satır fazla vermemesi (eski
        // formülün `saturating_sub(1)`'i) düzenin caret kuralında; tek
        // satırlık görüntüde sonuç eski sütun bölmesiyle **aynı** ve bir
        // bekçi ikisini karşılaştırıyor
        // (`dock::tests::grid_span_matches_the_column_division_on_one_line`).
        //
        // **Yürüyüşün eski bölmeden ayrıldığı tek yer geniş karakter** ve
        // ayrılık bir düzeltme: satır sonuna sığmayan geniş glyph ızgarada alt
        // satıra iniyor ve bölme bunu görmüyordu, yani imleçten sonraki
        // kuyruğu **eksik** sayıyor, son satırı ızgarada sızdırıyordu. Satır sonu (`PS2`, Esc-Enter)
        // taşıyan görüntü (032) de aynı yürüyüşten: `BUFFER`'ın devam satırları
        // 0. sütundan, alt uç imleçten sonraki satırlar sarmalarıyla
        // ([`dock::grid_span`]). Tek fazla-bastırma yolu hâlâ bayat ayna
        // (`line-pre-redraw` çizimden önce koşuyor) ve o bir karelik.
        //
        // **Birim sütun, karakter değil** (024): birleştirici sıfır, geniş
        // glyph iki sütun — [`dock::column_width`]'in tablosu, ızgaranın
        // sarmasıyla aynı. Karakterle sayıldığında NFD bir dosya adı (`é` =
        // iki karakter, bir sütun) satırı fazla yuvarlıyor ve altındaki
        // tamamlama listesinin ilk satırını gizliyordu. Set kapısı
        // (`/code-review`) yakalamıştı.
        let span = suppressed_block.map(|_| {
            let (above, below) = dock::grid_span(
                &blocks.input,
                blocks.input_caret,
                usize::from(cursor_col),
                term.columns(),
                blocks.input_cluster,
            );
            (
                u16::try_from(above).unwrap_or(u16::MAX),
                u16::try_from(below).unwrap_or(u16::MAX),
            )
        });
        let suppress_to = suppressed_block.zip(span).and_then(|(input, (_, below))| {
            let to = cursor_screen_row
                .saturating_add(below)
                .min(grid_rows.saturating_sub(1));
            // **Tazelik kapısı.** Bastırma aynanın *güncel* olduğuna
            // güveniyor ve bunu sınayan hiçbir şey yoktu: ayna bayatlarsa
            // ızgara gizlenir, dock eski metni gösterir ve kullanıcı
            // yazdığını **hiçbir yerde** görmez. Ölçülmüş örneği
            // `bracketed-paste-magic`: yapıştırılan metni `zle -U` ile
            // kuyruğa geri basıyor, ZLE typeahead varken redisplay'i atlıyor
            // ve `line-pre-redraw` — dolayısıyla aynamız — bir sonraki tuşa
            // kadar hiç koşmuyor. Kabuk tarafında üç çare ölçüldü ve üçü de
            // kapalı (kancayı yeniden bağlamak, widget'ı sarmalamak,
            // eklentinin `paste-finish`'i — sonuncusunda `BUFFER` henüz boş).
            //
            // Kapı **kip sezmiyor**, iki kesin veriyi karşılaştırıyor:
            // ızgaranın son mürekkebi ile aynanınki. Yanlış alarmın yönü
            // güvenli — bastırmayı bırakır, yani en kötü ihtimalle kullanıcı
            // satırı iki yerde görür; sessizce kaybetmez.
            //
            // **Kapının kör noktası: iki boşluk aynı şey değil.** Ayna hiç
            // karakter taşımıyorsa son mürekkebi `None`, boş bir ızgara
            // satırınınki de `None` — karşılaştırma vakumda "taze" diyor.
            // Meşru hâli boş prompt (kullanıcı henüz yazmadı) ve o hâl
            // bastırmaya **girmek zorunda**: girmeseydi 012 phase-8'in kusuru
            // geri gelirdi (satır gizli ama yer kaplıyor). Bayat hâli
            // **ölçüldü** (2026-09-21, kullanıcı bildirdi + saf PTY ile
            // doğrulandı): zsh bracketed yapıştırmanın **son satır sonunu
            // tamponda tutuyor** (`BUFFER='echo a\necho b\n'`, `CURSOR=14`),
            // yani ızgaranın imleci yapıştırmanın bıraktığı **boş** satırda
            // duruyor; `bracketed-paste-magic` de aynayı bir tuş boyunca boş
            // bıraktığı için iki `None` eşleşiyor, bastırma açılıyor ve caret
            // metnin yanında değil dock'un prompt işaretinin yanında kalıyordu.
            //
            // Ayıran ikinci kesin veri **çıpanın satırı**: ayna hiç karakter
            // taşımıyorsa imleci prompt'un satırından aşağı itecek bir şey
            // yoktur, yani imleç çıpanın satırında olmak **zorunda**. Boş
            // prompt'ta öyle (betiğin `PS1`'i iki boşluk basıyor ve o iki
            // hücre çıpayı taşıyor); yapıştırmadan sonra değil — çıpa
            // yukarıda, imleç aşağıdaki boş satırda.
            //
            // **Çıpa hiç bulunamazsa kapı susuyor** ve bu bilerek: hücresiz
            // bir prompt'ta (012 phase-7'nin kurduğu hâl) söyleyecek bir şey
            // yok, yani karar bugünkü karşılaştırmaya kalıyor. Kapının yönü
            // her iki kolda da aynı: şüpheli hâl bastırmayı **bırakıyor**,
            // yani en kötü ihtimalle satır iki yerde görünür.
            //
            // **Boşluğun ölçütü karakter** (032, [`crate::shell::SuppressedInput::blank`]):
            // tek başına bir `\n` de imleci aşağı itiyor. `PREBUFFER`'lı
            // aynada soru hiç sorulmuyor — `for>` satırında imleç meşru
            // olarak çıpanın aşağısında.
            let blank_mirror = input.blank;
            let at_anchor = !blank_mirror
                || anchor_row_at_or_above(&term, to, offset, input.block)
                    .is_none_or(|anchor| anchor == to);
            //
            // **Önce zamansal soru** (025): kullanıcının son girdisinin
            // aynası geldiyse ayna tazedir ve ızgaranın ne dediğine bakmak
            // gerekmiyor — içerik karşılaştırması bir **vekildi** ve zsh
            // karakteri dönüştürdüğü her hâlde kırılıyordu (`🥰` aynada ham,
            // ızgarada `<0001f970>`: kapı düşüyor, caret yazarken ızgaraya
            // sıçrıyordu). Damga aynanın **yanında** geliyor
            // ([`crate::shell::DockState::answers`]), yani bayat bir okuma
            // bayat damga getirir ve karar aşağıdaki içerik kapısına kalır.
            // Cevap gelmediyse (yapıştırmanın `bracketed-paste-magic` kolu)
            // kapı bugünkü hâliyle koşuyor: iki taraf da henüz eski ve
            // eşleşiyorlar, ya da ayna geride ve düşüyor. Kısa devre taramayı
            // da atlatıyor.
            //
            // **İki bilinen sınır, adıyla** (`discussion.md` → Karar 2). Bir
            // tuşun aynası yoldayken hemen bir yapıştırma giderse ayna
            // yapıştırmanın nesliyle damgalanır ve kapı onu bir tuş boyunca
            // cevap sanar — terminalin "ZLE bunu işledi mi" sorusunu bilme
            // yolu yok. Ve kabuğun **dışından** gelen yazım (bir arka plan
            // işinin giriş satırına bastığı çıktı) nesli oynatmıyor: satır
            // düzenlenirken bastırılan aralıkta gizli kalıyor, Enter'dan sonra
            // geçmişte görünüyor. İkisini ayırmanın yolu bir yazım nesli ve
            // tarayıcının yazım kavramı yok. Üçüncüsü ters yönde ve eski
            // davranış: zsh'in **redisplay'siz** tuttuğu bir tuş (emacs'ın
            // `^X` öneki, vi'de çıplak `Esc`'in bekleme süresi) nesli
            // ilerletiyor ama ayna doğurmuyor, yani o süre kapı içeriğe
            // düşüyor ve dönüştürülmüş karakterli satır ızgaraya çıkıyor.
            let answered = input.answers == self.key_gen.load(Ordering::Acquire);
            let fresh =
                answered || (last_ink_in_row(&term, to, offset) == input.last_ink && at_anchor);
            fresh.then_some(to)
        });
        // Aralığın **üst tabanı**, aynı aritmetiğin öteki yönü: caret'ten
        // önceki metin imlecin satırının üstünde kaç satır tutuyor.
        //
        // Üstü yalnız çıpaya bağlamak yetmiyordu: çıpa prompt'un satırında
        // duruyor ve imleç oradan uzaklaşırsa (arada bir şey basılırsa,
        // `zle -I`'nin iş bildirimi gibi) aradaki satırlar girişin olmadığı
        // hâlde bastırılırdı — gözlendi, bir `od` dökümü bütünüyle
        // kayboluyordu. İki aday arasından **alttaki** seçiliyor: çıpa
        // satırı, ya da aynanın hesapladığı ilk satır.
        //
        // **`PREBUFFER` doluysa taban çıpanın satırı** (032 Karar 7,
        // [`crate::shell::SuppressedInput::from_anchor`]): kabul edilmiş
        // satırlar `PS2`'leriyle ızgarada ve `PS2`'nin genişliği aynada yok;
        // bağlantı `preexec`'e kadar açık, yani bütün komut çıpayı taşıyor.
        // Taban `0` → aşağıdaki `from.max(floor)` çıpayı seçiyor.
        let suppress_floor = suppressed_block.zip(span).map_or(0, |(input, (above, _))| {
            if input.from_anchor {
                0
            } else {
                cursor_screen_row.saturating_sub(above)
            }
        });
        // **Devrin tek yüklemi ve dört tüketicisi var**: hangi hücrelerin
        // atlanacağı, imlecin çizilip çizilmeyeceği, **doluluk sayısı** ve
        // dock'un kendi caret'i (`Session::dock`'a argüman olarak gidiyor).
        // Dördü de "giriş satırı ızgaranın mı, dock'un mu" sorusunun yanıtına
        // bağlı ve ayrı ayrı sorulduklarında ayrışıyorlardı — gözlenen kusur
        // tam da o ayrışmaydı (012 phase-8, kullanıcı): boş prompt'ta hiçbir
        // hücre çıpayı taşımadığı için satır **çizilmiyor ama doluluğa
        // giriyordu**, ilk tuşta çıpa doğunca doluluk bir satır düşüyor ve
        // bütün ızgara oynuyordu. Yazınca aşağı, silince yukarı.
        //
        // Üç ön koşul ve üçü de zorunlu:
        //
        // - `self.dock` — pencerenin devralacak bir yüzeyi var. Yoksa ne
        //   caret'i ne satırı verecek kimse var; dock'suz pencerede satırı
        //   gizlemek kullanıcının yazdığını **hiçbir yerde** göstermemek olurdu.
        // - `!alt_screen` — alternatif ekranda dock kalkıyor (phase-7).
        // - Bastırılan bir satır varsa **tazelik kapısı**: `suppress_to` onu
        //   taşıyor (bayat aynada `None`). Bastırılan satır yokken sorulacak
        //   bir tazelik de yok — dock metin değil boş bir caret gösteriyor.
        //
        // **Dördüncüsü uzak oturum** (036 Karar 8) ve burada değil
        // `caret.home`'un içinde ([`crate::shell::ShellLog::caret`]), çünkü
        // tutmadan **önce** uygulanmak zorunda: `C`'den sonraki tutma
        // penceresine düşen `set_remote` aksi hâlde `input_rows == 0` ile
        // `caret_in_dock` doğurur ve caret bağlam satırına otururdu. Orada
        // kalan süreyi de söndürüyor — çevrilmeyen cevap saat kurmuyor.
        //
        // Burada `suppress_floor <= suppress_to` diye bir karşılaştırma
        // **yok** ve olmamalı: ikisi de imlecin satırından türüyor
        // (`floor = row - above`, `to = row + below`), yani karşılaştırma hiç
        // yanlış olamaz — totolojiydi ve kontrol ettiğini sandığı dejenere
        // hâli (çıpa imlecin aşağısında) ifade bile edemiyordu. O hâlin
        // gerçek kapısı atlama döngüsünde: `from.max(suppress_floor)`.
        let caret_in_dock = self.dock
            && !alt_screen
            && caret.home == CaretHome::Dock
            && (suppressed_block.is_none() || suppress_to.is_some());
        // Select All'ün okuduğu kopya ([`Session::caret_in_dock`]'un doc'u).
        self.caret_in_dock.store(caret_in_dock, Ordering::Relaxed);

        // Mürekkebi olmayan dört durum tek `None`'a iniyor ve çizen taraf
        // bayrak sormuyor. Biri `HIDDEN` (`\e[8m`) ve o bu maskede **değil**:
        // gizlilik mürekkebin yanında kuralları da düşürdüğü için aşağıda
        // tek bir `let`'te yaşıyor — iki ifadeye yazılsaydı sonradan
        // ayrışabilirlerdi. İkisi spacer:
        // - `WIDE_CHAR_SPACER`: geniş karakterin ikinci hücresi. Hücrenin
        //   `c`'si alacritty'de zaten `' '` (spacer `write_at_cursor(' ')` ile
        //   yazılıyor), ama arka planı geniş karakterin şablonundan geliyor:
        //   hücreyi tümden elemek onun sağ yarısını renksiz bırakırdı.
        // - `LEADING_WIDE_CHAR_SPACER`: satır sonuna sığmayan geniş karakterin
        //   bıraktığı boşluk; aynı gerekçe.
        // Dördüncüsü boşluk karakterinin kendisi: onun da mürekkebi yok ve
        // **altı çiziliyken de yok**. 003 burada tersini öngörmüştü; karar
        // tersine çıktı, çünkü `Some(' ')` bir boşluk glyph'i yükletirdi —
        // atlasta yuva harcar, tek piksel boyamaz. Altı çizili boşluğun
        // istediği bir kural çizgisi ve onu aşağıdaki atlama koşulu taşıyor.
        const SPACERS: Flags = Flags::WIDE_CHAR_SPACER.union(Flags::LEADING_WIDE_CHAR_SPACER);

        // Kural çizgisi isteyen bayrakların maskesi: atlama koşulunun kural
        // yarısı **tek** test olsun diye. "Hangi çeşit" sorusu (aşağıdaki
        // zincir) kapıdan sonra sorulur — `fg` ve `underline_color`'ın kapıdan
        // sonra çözülmesiyle aynı disiplin: kapı "çizilecek bir şey var mı"
        // diye sorar, "ne" diye değil.
        const RULES: Flags = Flags::ALL_UNDERLINES.union(Flags::STRIKEOUT);

        // Vurgu aralığı kare başına bir kez çözülüyor: `to_range` hücrenin
        // yanında değil burada, çünkü yan tablolara inmiyor ve hücre başına
        // sorulacak bir şey değil. Şekil yukarıda okundu (`cursor_shape`):
        // aşağıdaki `contains_cell` blok imlecin sınır istisnasını soruyor ve
        // onu hücre başına okumak aynı değeri her hücrede yeniden okumak
        // olurdu.
        let selected_range = visible_range(term.selection.as_ref(), &term);
        // Seçim koşusunun birikimi: satır başına ilk ve son **çizilir seçili**
        // sütun. `display_iter` satır sırasıyla geldiği için tek bir açık koşu
        // yetiyor; satır değişince kapanıp tampona iniyor.
        let mut open_run: Option<SelectionRun> = None;
        // Devam satırı olduğu anlaşılan son `(satır, kimlik)`: çıpa satırın
        // bütün hücrelerinde, üstteki satırın taraması satır başına bir kez.
        let mut continued: Option<(u16, u32)> = None;
        // Henüz tüketilmemiş bir temizleme: geçmişin en yeni satırı onun
        // kalıntısı ([`Session::clear_boundary`]). Nesli tüketen yer aşağıda
        // (`observe_screen_clear`), burası yalnız okuyor. **Damgalanmamış
        // bayrakta bir kez daha**, damganın kendi gerekçesiyle: nesli
        // tüketen kare ızgarayı henüz temizlenmemiş görebiliyor ve o karede
        // okunan satır kalıntı değil; sonraki kare (damganın alındığı) doğru
        // satırı görüyor.
        let pending =
            self.screen_clears.load(Ordering::Relaxed) != self.screen_seen.load(Ordering::Relaxed);
        let unstamped = self.screen_cleared.load(Ordering::Relaxed)
            && self.screen_clear_history.load(Ordering::Relaxed) == Self::UNSTAMPED;
        if !alt_screen && (pending || unstamped) {
            let boundary = if term.history_size() > 0 {
                row_identity(&term, Line(-1))
            } else {
                0
            };
            self.clear_boundary.store(boundary, Ordering::Relaxed);
        }
        let clear_boundary = self.clear_boundary.load(Ordering::Relaxed);

        for indexed in display_iter {
            let cell = indexed.cell;
            let flags = cell.flags;
            let dim = flags.contains(Flags::DIM);
            let hidden = flags.contains(Flags::HIDDEN);
            // **Seçim artık hücreyi boyamıyor, satır koşusu veriyor** (031
            // phase-2). Vurgu temanın `selection` rengi ve çizen taraf onu
            // zeminle glyph'ler arasına düz dörtgen olarak koyuyor; buradaki
            // tek iş hangi hücrelerin koşuyu belirlediğini bulmak. Ters video
            // takası (`inverse ^ selected`) kalktı: seçili hücrenin metni
            // **kendi ön planıyla** ve ters video çözülmüş olarak çiziliyor
            // (Karar 3) — seçili ters videolu bir hücre (vim'in durum satırı)
            // normal ön planıyla okunuyor, sözdizimi renkleri seçimde
            // kaybolmuyor. İmleç seçimin üstünde kalıyor: koşu caret'ten önce
            // çiziliyor.
            //
            // Gizli metin koşunun **ucunu** belirlemez: `HIDDEN` "çizme" demek
            // ve seçim onu delseydi gizli hücrenin yeri boyalı bir blok olarak
            // görünürdü. İçeride kalırsa köprüleniyor — iki yanında görünen
            // seçili metin varken aradaki boşluk da boşluk gibi vurgulu.
            // `contains` değil `contains_cell`, iki sebeple: blok imleç
            // seçimin **ucunda** durursa o hücre seçilmiş sayılmaz
            // (alacritty'nin istisnası; imlecin **kendi** noktası bu yüzden
            // veriliyor — hücrenin noktası verilince istisna her uca
            // uygulanıyordu), ve aralık bir spacer'da başlarsa geniş
            // karakterin baş hücresi de vurgulanır.
            // `set_selection` spacer'dan başlayan aralık kurmaz (`anchor`
            // spacer'ı `Right` yapıyor), ama seçimden sonra satır yeniden
            // yazılıp o hücre spacer olursa aralık orada başlar.
            //
            // **Seçim içeriği vurgular, içerik yaratmaz.** Aralık boş
            // hücreleri de kapsıyor ve onları boyamak "burada bir şey var"
            // demek oluyordu: boş ekranda fareyi sürükleyen kullanıcı koca bir
            // blok görüyor, üstelik o seçim **hiçbir şey kopyalamıyor**
            // (gözlendi, 2026-09-18). Ölçüt "mürekkep" **değil** "çizilir mi"
            // (zemin de sütunu işgal ediyor; 013 Karar 7): ters videolu bir
            // boşluk — vim'in durum satırı, tmux çubuğu — mürekkepsizdir ama
            // görünürdür ve koşuyu uzatır; varsayılan zeminli boş hücre
            // görünmezdir ve koşuyu uzatmaz. Birim hücre değil **satır**
            // (Karar 4): koşu ilk çizilir seçili hücreden sonuncusuna uzanıyor,
            // yani kelime arası boşluk vurgulu (pano onu zaten kopyalıyor),
            // satır sonundaki boş kuyruk ve boş bir ara satır vurgusuz.
            let plain_inverse = flags.contains(Flags::INVERSE);
            let plain_back = if plain_inverse {
                color::resolve_fg(cell.fg, dim, colors, &theme)
            } else {
                color::resolve(cell.bg, colors, &theme)
            };
            // `HIDDEN` (`\e[8m`) "mürekkep yok" demek ve **tek bir `let`**
            // (yukarıdaki `hidden`): hem glyph'i hem kuralları düşürüyor, hem
            // de seçim vurgusunu dışlıyor. Üç ayrı ifadeye yazılsaydı biri
            // sonradan değişip ötekiler eski kalabilirdi ve belirti "gizli
            // metin altı çizgisinden/vurgusundan okunuyor" olurdu.
            let ch = (!hidden && !flags.intersects(SPACERS) && cell.c != ' ').then_some(cell.c);
            // Kapının kural yarısı tek maske testi; **hangi** çeşit olduğu
            // kapıdan sonra sorulur. `!hidden` maskenin dışında değil
            // **içinde**: dışarıda kalsaydı gizli ve altı çizili bir hücre
            // kapıdan geçer ve `sink`'e çizilecek hiçbir şeyi olmadan varırdı.
            let ruled = !hidden && flags.intersects(RULES);
            // **Spacer da çizilir sayılıyor** ve bu şart (`/code-review`, 014
            // kapı): geniş karakterin ikinci hücresinin kendi mürekkebi yok
            // (`ch` onu eliyor) ve zemini varsayılan olabilir, yani ölçüt
            // yalnız mürekkep + zemin + kural olsaydı seçili bir CJK
            // karakterinin **yarısı** vurgusuz kalırdı — `selection_text()`
            // onu bütün kopyalarken.
            let drawable =
                plain_back != background || ch.is_some() || ruled || flags.intersects(SPACERS);
            let selected = !hidden
                && drawable
                && selected_range
                    .as_ref()
                    .is_some_and(|range| range.contains_cell(&indexed, cursor_point, cursor_shape));
            // `display_iter` yalnız görünür pencereyi verir: aralığı
            // `-offset ..= -offset + screen_lines - 1`'e kırpar, yani satır
            // zaten 0..rows. Sözleşme değişirse debug derlemesi haber verir;
            // sürüm derlemesinde hücre başına dal kalmaz.
            let row = indexed.point.line.0 + offset;
            debug_assert!((0..rows).contains(&row), "display_iter pencere dışı: {row}");
            // `try_from` sürüm derlemesinde de tutar: negatif bir satır
            // sessizce 65535'e sarsaydı renderer tampon dışına yazardı.
            let Ok(row) = u16::try_from(row) else {
                continue;
            };
            let col = indexed.point.column.0 as u16;
            // **Koşu atlama kapısından ÖNCE** ve sıra zorunlu: varsayılan
            // zeminli spacer ve boşluk kapıya takılıyor (çizecek bir şeyi
            // yok), sonra olsaydı seçili bir CJK karakterinin sağ yarısı
            // koşudan düşerdi. Geniş karakterin baş hücresi koşuyu
            // spacer'ının sütununa kadar uzatıyor: aralık baş hücrede bitince
            // spacer'ın kendisi seçili sayılmıyor ama glyph iki sütun (süre
            // sayacının `last_col` aritmetiğinin aynısı).
            if selected {
                let last = if flags.contains(Flags::WIDE_CHAR) {
                    col.saturating_add(1)
                } else {
                    col
                };
                match open_run.as_mut() {
                    Some(run) if run.row == row => run.last = run.last.max(last),
                    _ => {
                        selection.runs.extend(open_run.take());
                        open_run = Some(SelectionRun {
                            row,
                            first: col,
                            last,
                        });
                    }
                }
            }
            // Kapının zemin yarısı **seçimsiz** hâlin zemini: seçim kapıyı
            // oynatmıyor. Oynatsaydı seçili ters videolu bir boşluk (çizecek
            // tek şeyi zemini) kapıya takılır, satırın doluluğu (`drawn_rows`)
            // ve çıpa taraması seçimle değişirdi — seçim içerik yaratmadığı
            // gibi içerik de silmez.
            let plain_bg = (plain_back != background).then(|| color::linear_rgba(plain_back));

            // Atlama koşulu: ne boyanacak bir arka plan, ne çizilecek bir
            // mürekkep, ne de bir kural çizgisi. Boş grid'de bu koşul her
            // hücreye uyar ve sink hiç çağrılmaz — `frame()`'in boştaki
            // maliyeti iterasyonun kendisi.
            if plain_bg.is_none() && ch.is_none() && !ruled {
                continue;
            }
            // Seçili hücrenin zemini **çizilmiyor**: seçimin şeklinin altında
            // kalıyor ve opak şekil onu zaten örtüyor. Örtmediği tek yer
            // şeklin yuvarlak köşesi (031 phase-3) ve orada pencere zemini
            // görünüyor; zemin altta bırakılsaydı renkli bir satırın (ters
            // videolu durum satırı) köşesinde seçimin dışına taşan birkaç
            // piksellik kırık leke kalırdı — gözle bakıldı, çentik okunmuyor. Ters video da burada
            // çözülüyor — metin hücrenin kendi ön planıyla (Karar 3). Sönüklük
            // yine `cell.fg`'den doğan renge gider (`color::resolve_fg`), yani
            // seçili sönük ters video hücrede o renk ön plana döner.
            let bg = if selected { None } else { plain_bg };
            let inverse = plain_inverse && !selected;
            // **Doluluk sayısının çizilen yarısı** ([`Cursor::content_rows`]):
            // atlama kapısından **sonra**, yani yalnız gerçekten çizilen
            // satırlar sayılıyor. Kapıdan önce olsaydı `display_iter` bütün
            // pencereyi verdiği için boş ızgara da "dolu" görünür ve içerik
            // hiç ötelenmezdi. `saturating_add`: `row < rows ≤ u16::MAX`, yani
            // taşma temsil edilemez ama sarma sessiz olurdu.
            // **Faz 1.** Çıpa okuması da kapıdan sonra (R3.4), ön planla aynı
            // gerekçeyle: `hyperlink()` yan tabloya (`CellExtra`) iniyor ve
            // çizilmeyen hücre için ödenmemeli — kapının üstünde olsaydı boş
            // bir 80×24 ızgarada ~1900 hücrede ödenirdi, altında ~50'de.
            //
            // **Maliyet `extra`sız hücrede** bir boş kontrol, `extra`lı hücrede
            // bir `Arc` klonu: alacritty `hyperlink()`'i `Option<Hyperlink>`
            // **döndürüyor** ve `CellExtra.hyperlink` alanı `pub` değil, yani
            // ödünç veren bir yol yok. Prompt'un her hücresi bağlantılı olduğu
            // için üç satırlık bir prompt kare başına birkaç yüz atomik sayaç
            // turu ediyor (`/code-review`, 010 kapı). Kapatmanın yolu
            // alacritty'de bir `&Hyperlink` erişimcisi; ölçülmüş bir ihtiyaç
            // beklemeden yukarı akım değiştirilmedi.
            //
            // **Kapının bedeli** (aynı bulgu): tamamı varsayılan zeminli
            // boşluktan oluşan bir prompt satırı kapıdan geçmez, yani çıpa
            // vermez ve blok o satırdan değil sonraki dolu satırdan başlar.
            // Çok satırlı, ilk satırı boşluk dolgusu olan bir PS1'de görünür.
            //
            // Önek eşleşmesi yabancı bağlantıları da eliyor: `ls --hyperlink`
            // ya da bir `man` sayfasının `file://`'ı buraya düşmez.
            if !alt_screen && let Some(id) = cell.hyperlink().and_then(|link| block_id(link.uri()))
            {
                // Çıpa prompt'un **bütün** hücrelerinde; ilk satırı isteyen
                // taraf yalnız değişimi kaydediyor. Aynı kimliğin ikinci kez
                // görünmesi (araya başka bir kimlik girdikten sonra) yeni bir
                // çıpa sayılır: satırlar artan, aralıklar tutarlı kalır.
                //
                // **Devam satırı çıpa değil** (032 phase-6): prompt'un
                // bağlantısı `preexec`'e kadar açık, yani çok satırlı komutun
                // (ve sarılan uzun satırın) **bütün** satırları kimliği
                // taşıyor. Komutun ilk satırı pencerenin üstüne kayınca
                // "görünen ilk çıpalı satır" onun devamı olurdu ve işaret ile
                // sayaç oraya otururdu. Üstteki satır — geçmiş dahil — aynı
                // kimliği taşıyorsa bu satır komutun başı değil: işaret
                // komutun **kendi** satırında, görünmüyorsa hiç
                // ([`block_row_continues`]).
                if blocks.anchors.last().map(|&(last, ..)| last) != Some(id)
                    && continued != Some((row, id))
                {
                    let above = Line(i32::from(row) - offset - 1);
                    if block_row_continues(&term, above, id, clear_boundary) {
                        continued = Some((row, id));
                    } else {
                        // Son mürekkep sütunu sıfırdan başlıyor: hiç
                        // mürekkebi olmayan komut satırında (boş prompt) sayaç
                        // sağda, kimseye değmeden duruyor.
                        blocks.anchors.push((id, row, 0));
                    }
                }
                // Bastırmanın üst ucu aynı okumadan: yazılmakta olan bloğun
                // **ilk** çıpa satırı. `get_or_insert` ikinci satırı yazmıyor
                // — çok satırlı bir prompt'ta aralık en üstten başlamalı.
                if suppressed_block.is_some_and(|input| input.block == id) {
                    suppress_from.get_or_insert(row);
                }
            }
            // **Bastırma: giriş satırı ızgarada çizilmez** (R3.1). Aralık
            // prompt'un çıpa satırından imlecin satırına ve **bütün
            // sütunlar**: sütun aritmetiği yapılsaydı prompt'un bittiği sütun
            // sink'e bilinmek zorunda kalırdı ve o bilgi burada yok.
            //
            // **Kapı çıpa taramasından SONRA** ve sıra zorunlu (R3.2): naif
            // bir bastırma satırı tümden atlar, çıpayı da öldürür ve blok
            // şeridi kaybolurdu. Tarama glyph üretiminden bağımsız koşuyor;
            // bekçisi `a_suppressed_input_line_keeps_the_block_stripe`.
            //
            // **`drawn_rows`'un da üstünde**: bastırılan satır doluluğa
            // sayılmaz, yoksa 011'in tabana yapışması çizilmeyen bir satır
            // için yer ayırır ve dock ile içerik arasında boş bir şerit
            // kalırdı.
            if suppressed_rows(caret_in_dock, suppress_from, suppress_to, suppress_floor)
                .is_some_and(|hidden| hidden.contains(&row))
            {
                continue;
            }
            drawn_rows = drawn_rows.max(row.saturating_add(1));
            // Mürekkep yarısı **ancak burada** çözülüyor — atlama kapısından
            // sonra. Kapıdan önce olsaydı `Term` kilidi tutulurken çizilmeyen
            // her hücre için de ödenirdi (renk çözümü, yan tabloya iniş) ve boş
            // grid'de hücrelerin neredeyse tamamı çizilmiyor. Ortak parça
            // ([`cell_style`]): doldurma döngüsü de aynı yerden geçiyor.
            let style = cell_style(cell, inverse, dim, ruled, colors, &theme);

            // **Süre sayacının çakışma ölçütü**, faz 1'de toplanıyor: komut
            // satırının son mürekkepli sütunu. `display_iter` satır sırasıyla
            // geldiği ve komutun bütün hücreleri çıpayı taşıdığı (kapanış
            // `preexec`'te) için aranan çıpa her zaman **sonuncusu**; liste
            // taranmıyor.
            //
            // Ölçüt **mürekkep değil doluluk**: bu sütunda görünen bir şey
            // var mı. Buraya varan her hücre atlama kapısını geçmiştir, yani
            // ya zemini, ya mürekkebi, ya da bir kural çizgisi vardır —
            // üçünün de sütunu işgal ediyor.
            //
            // Ayrım iki gerçek kusuru kapatıyor (`/code-review`, 013 kapı):
            //
            // - **Seçim — bu yarısı artık ulaşılamaz ve kayıt olarak
            //   duruyor** (`/code-review`, 014 kapı). 013'te seçili satırın
            //   boş kuyruk hücreleri ters çevrilmiş zemin alıyordu ve sayaç
            //   onların üstüne düşseydi sönük ön plan okunmaz olurdu. 014
            //   seçimi "çizilir hücreye" kapadı (`drawable`), yani boş kuyruk
            //   artık hiç boyanmıyor; `last_col`'a da hiç varmıyor. Ölçütü
            //   doluluk tutan şey aşağıdaki geniş glyph, seçim değil.
            // - **Geniş glyph.** İkinci yarısı ayrı bir hücre ve kendi başına
            //   kapıdan geçmiyor, ama sütunu işgal ediyor. Öncüsünden
            //   `col + 1` diye kaydediliyor; yoksa `çç` ile biten bir komutta
            //   sayaç vaat ettiği bir hücrelik payı yerdi. Bugün görünmüyor
            //   (geniş glyph henüz çizilmiyor) ama aritmetik **şimdi** yanlış
            //   olurdu ve 015 onu görünür kılardı.
            if let Some((_, anchor_row, last_col)) = blocks.anchors.last_mut()
                && *anchor_row == row
            {
                let end = if flags.contains(Flags::WIDE_CHAR) {
                    col.saturating_add(1)
                } else {
                    col
                };
                *last_col = (*last_col).max(end);
            }
            // İmlecin altındaki hücreye burada **dokunulmuyor**: hücre kendi
            // renkleriyle sınırdan geçiyor, bloğun altında kalan pikselleri
            // [`Cursor::text`] ile çizen ezecek. Eskiden bu satırlarda hücrenin
            // `fg`'si zemine çevriliyor ve `underline_color`'ı düşürülüyordu;
            // hücre bölünemediği için yarım örtülen hücrenin **tamamı** ters
            // çizilirdi — 008'in kaydırdığı imlecin hedef hücresi henüz
            // örtülmeden görünmez olurdu.
            sink(Cell {
                col,
                row,
                ch,
                fg: style.fg,
                bg,
                // `Flags::BOLD_ITALIC` ikisinin birleşimi, ayrı bir bit
                // değil: `contains` her iki soruyu da doğru yanıtlıyor.
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline: style.underline,
                underline_color: style.underline_color,
                strikeout: style.strikeout,
                // Yukarıdaki `last_col` aritmetiği aynı bayrağı okuyor;
                // **aynı ifade** olmak zorunda değil ama aynı bayrak olmak
                // zorunda — ikisi ayrışsa sayacın payı ile çizimin genişliği
                // ayrılırdı.
                wide: flags.contains(Flags::WIDE_CHAR),
                cluster: cell_cluster(self.cluster, cell, ch, clusters),
            });
        }
        selection.runs.extend(open_run);
        // **Bastırılan giriş satırı koşu da vermiyor**: ızgarada çizilmiyor ve
        // yer kaplamıyor, yani oradaki bir koşu görünmeyen hücreleri — ya da
        // ötelemeyle dock'un bandına düşen boş bir şeridi — boyardı. Süzgeç
        // döngüden **sonra**, çünkü bastırmanın üst ucu (`suppress_from`)
        // çıpa taramasıyla döngünün içinde doğuyor; koşu ise kapıdan önce
        // birikmek zorunda.
        //
        // **Aralık tek yüklemden** ([`suppressed_rows`]) ve üç tüketicisi
        // var: atlanan hücreler (döngünün içinde), seçim koşuları ve arama
        // eşleşmeleri (aşağıda). Ayrı yazılsalardı biri ötekinden ayrışır ve
        // görünmeyen bir satır vurgulanırdı (015'in dersi).
        let hidden = suppressed_rows(caret_in_dock, suppress_from, suppress_to, suppress_floor);
        if let Some(hidden) = &hidden {
            selection.runs.retain(|run| !hidden.contains(&run.row));
        }
        let search_hidden = hidden
            .as_ref()
            .map(|rows| i32::from(*rows.start()) - offset..=i32::from(*rows.end()) - offset);

        // **Doluluk sayısı kayıttan önce bir yerele çıkıyor** ve sebebi iki
        // tüketicisi: bayrağın ömrü ([`Session::observe_screen_clear`]) ve
        // doldurmanın boşluğu ([`Session::fill_rows`]). İkisi de `Term` kilidi
        // altında ve ikisi de kayıt kurulmadan önce koşuyor; alan üstünden
        // okunsaydı kayıt yalnız sırayı taşıyan bir ara durak olurdu.
        //
        // **Alternatif ekranda ızgaranın tamamı**, yani öteleme sıfır: vim ve
        // htop bütün satırları sahipleniyor (bayrak kilidin altında zaten
        // okundu). Ana ekranda iki kaynağın maksimumu; gerekçesi
        // [`Cursor::content_rows`]'ta.
        //
        // **Kaydırılmış pencerede de görünür satırlardan** ve bu 011'in
        // kuralı: içerik her zaman tabana yaslı, geçmiş penceresinde de.
        // Bir dönem burada `offset != 0 => grid_rows` vardı ve belirti
        // ölçüldü (2026-09-20, gözle kontrol; kullanıcı): ızgaranın **boş**
        // alt satırları da doluluğa giriyor, öteleme kapanıyor ve bütün
        // içerik pencerenin tepesine sıçrıyordu — terminal aşağıdan yukarı
        // akar, o hâl kuralın kendisini deliyordu.
        //
        // Ölü kaydırmayı bu kol getirmiyordu: sebep doldurmanın kaydırılmış
        // pencerede de koşmasıydı (`fill + offset` sabit kalıyordu) ve kapı
        // [`Session::fill_rows`]'ta. `fill == 0` olduğu için burada doluluk
        // büyüdükçe öteleme küçülüyor, yani tepeden **yeni satır giriyor** —
        // istenen davranış tam olarak bu.
        let content_rows = if alt_screen {
            grid_rows
        } else if caret_in_dock {
            // **Giriş satırı yer de kaplamıyor** — `display: none`, gizli
            // bir satır değil. İmleç terimi burada düşüyor, çünkü imleç
            // ızgarada değil: sayılsaydı çizilmeyen bir satır için yer
            // ayrılır ve son çıktı satırı ile dock arasında boş bir şerit
            // kalırdı.
            //
            // **Kapı bastırmayla değil devirle aynı** ve fark ölçülebilir
            // bir kusurdu (gözlendi; kullanıcı): boş prompt'ta hiçbir hücre
            // çıpayı taşımıyor, yani bastırma çalışmıyor ama satır yine
            // çizilmiyordu — doluluk ise imleci sayıyordu. İlk tuşta çıpa
            // doğuyor, bastırma başlıyor ve doluluk **bir satır**
            // düşüyordu: ızgaranın tamamı yazarken aşağı, silerken yukarı
            // oynuyordu. Kapılar tek yükleme bağlanınca oynama kalmıyor.
            //
            // Taban 1: bütün pencerenin bastırıldığı dejenere hâlde
            // (ilk prompt, üstünde hiç çıktı yok) `drawn_rows` sıfır
            // kalır ve `content_rows`'un `1..=rows` sözleşmesi bozulurdu.
            drawn_rows.max(1)
        } else {
            drawn_rows.max(cursor_screen_row.saturating_add(1))
        };
        debug_assert!(
            (1..=grid_rows).contains(&content_rows),
            "doluluk sayısı ızgaranın dışında: {content_rows} / {grid_rows}"
        );
        // **Kasten temizleme bayrağının ömrü burada işliyor** ve yeri zorunlu:
        // `Term` kilidinin **içinde**, doluluk sayısıyla aynı okumada
        // ([`Session::observe_screen_clear`]). Defterin boyu da aynı okumadan:
        // düşme ölçütü "temizlemeden sonra geçmişe satır düştü mü" ve iki
        // sayının farklı turlardan gelmesi ölçütü uydurma yapardı.
        self.observe_screen_clear(term.history_size(), alt_screen, offset != 0);
        // **Doldurma bayrağın ömründen SONRA soruluyor** ve sıra zorunlu:
        // aynı karede gelmiş taze bir `CSI 2 J` bayrağı **kuruyor** ve
        // doldurma o kareyi de kapatmak zorunda. Ters sırada, baytları henüz
        // uygulanmamış bir Ctrl-L'in karesinde doldurma bir kereliğine koşar
        // ve temizlenen ekranı geri getirirdi.
        let gap = grid_rows.saturating_sub(content_rows);
        let gap_fill = self.fill_rows(&term, gap, alt_screen, offset != 0);
        // **Kaç satırın geçmişe kaydığı** — dolu ızgaranın kayma animasyonunun
        // tek girdisi ([`Cursor::scrolled`]). Kimlik her karede yazılıyor,
        // kaydırılmış pencerede ve alternatif ekranda sıfırla: oradan dönen
        // ilk kare karşılaştıracak bir şey bulmamalı.
        //
        // **Temizleme bayrağı kuruluyken sıfır** ve sıra bu yüzden
        // `observe_screen_clear`'dan sonra: `CSI 2 J` görünen satırları
        // geçmişe itiyor (alacritty `clear_viewport`) ve o itme bir kaydırma
        // gibi okunursa temizlenen ekran yukarı süzülerek giderdi — kullanıcı
        // onu silmek istedi, uğurlamak değil.
        let scrolled = self.scrolled_rows(&term, alt_screen, offset != 0, grid_rows, grid_cols);
        let scrolled = if self.screen_cleared.load(Ordering::Relaxed) {
            0
        } else {
            scrolled
        };
        // **Bandın geçici uzantısı**: kayma uçuştayken ızgara hedefinin altında
        // çiziliyor ve tepesinde açılan şerit boş kalırdı. Kapatan satırlar
        // tam da az önce ekranın tepesinden geçmişe düşenler, yani bant
        // yalnız **uzuyor** — yeni bir kaynak yok, aynı fill-yerel satırlar.
        let fill = gap_fill.max(self.slide_fill_rows(&term, gap, alt_screen, offset, scrolled));
        // **Bandın boyu kaydırma yoluna emanet ediliyor** ([`scroll_locked`]):
        // tekerlek `frame()`'in taramasını tekrarlayamaz (doluluk ızgaranın
        // bütün hücrelerini geziyor), ama ekranda duran bandı bilmek zorunda
        // — ilk çentiğin nereden başlayacağı ona bağlı. Yazan yalnız **dibe
        // yaslı** kare: kaydırılmış pencerede `fill` tanım gereği sıfır ve
        // üstüne yazsaydı dönüş çentiği bandı bulamaz, ekran dibe varırken
        // boşluk kadar zıplardı.
        //
        // **Bilinen sınır:** kullanıcı geçmişteyken arka planda gelen çıktı
        // ızgarayı doldurup dipteki boşluğu kapatabilir ve o zaman bu sayı
        // bayatlar — dönüşün son çentiği kullanıcıyı bir satır yerine dibe
        // indirir. Dar bir pencere (kaydırılmış **ve** çıktı akıyor **ve**
        // bandın kenarına iniliyor) ve yönü güvenli: inen kullanıcının zaten
        // gittiği yer dip. Kesin çare doluluğu iki kez saymak olurdu — biri
        // görünen pencere, biri ızgaranın kendisi — ve bedeli kare başına
        // ikinci bir tam tarama.
        //
        // **Yazılan `gap_fill`, `fill` değil**: bandın kaymanın açtığı geçici
        // uzantısı yerleşince ekranın dışında kalıyor (orijin sıfırın altına
        // iniyor, Metal kırpıyor), yani sanal kaydırmanın parçası değil.
        // Sayılsaydı dolu bir ızgarada tekerleğin ilk çentiği o uzantı kadar
        // satırı atlardı.
        if offset == 0 {
            self.fill_shown.store(gap_fill, Ordering::Relaxed);
        }
        // **Kesrin tepe satırı** ([`Cursor::top_row`]): kanalın en üstü, yani
        // bandın (bant yoksa görünen pencerenin) hemen üstü. Tek kapısı
        // satırın defterde olması.
        //
        // **Geçersiz kesir bu karede sıfırlanıyor**, nesil de artıyor:
        // şeridi kapatacak bir şey olmadan ızgarayı aşağı çizmek tepede boş
        // bir yarım satır bırakırdı. Kaydırma yolu böyle bir kesri zaten
        // doğurmuyor ([`scroll_fraction_locked`]); buraya düşüren şey
        // pencerenin altından değişen durum — `CSI 3 J`, alternatif ekrana
        // geçiş (plan → bilinen sınır), fare kipi.
        let stored = f64::from_bits(self.scroll_frac.load(Ordering::Relaxed));
        let history = term.history_size() as i64;
        // Kesrin geçerliliği **olay yolunun ölçüsüyle** soruluyor
        // ([`scroll_fraction_locked`]: tepenin üstünde, bandın sanal
        // kaydırmasıyla, defterde bir satır) ve tekerlek kaydırma koluna
        // gidiyor olmalı. Kol değiştiyse (alternatif ekran, fare kipine geçen
        // birincil ekran uygulaması) kesri artık hiçbir tekerlek olayı
        // silemez; bırakılsaydı ızgara yarım satır aşağıda asılı kalırdı.
        let valid = input::wheel_route(*term.mode(), false) == WheelRoute::Scroll
            && i64::from(visual_top(offset, self.band_shown())) < history;
        let scroll_frac = if stored > 0.0 && valid {
            // `f32`'ye yuvarlama `1.0`'a varabilir ve `[0, 1)` sözleşmesini
            // delerdi; sınır `1`'in hemen altındaki `f32`.
            (stored as f32).min(1.0 - f32::EPSILON / 2.0)
        } else {
            if stored != 0.0 {
                self.scroll_frac.store(0, Ordering::Relaxed);
                self.bump_scroll_generation();
            }
            0.0
        };
        // Satırın **çizilmesi** ise kanalın bu karedeki boyuna bakıyor:
        // tepe satırı kanalın en üstü, kaymanın uzantısı dahil. Uzantı
        // defterin tamamını tutuyorsa üstünde satır yok ve tepe satırı
        // doğmuyor, ama kesir kalıyor — o kare kaymanın ortasında ve uzantı
        // pencerenin tepesine zaten ulaşıyor ([`Session::slide_fill_rows`]).
        let has_row_above = i64::from(offset) + i64::from(fill) < history;
        let top_row = u16::from(scroll_frac > 0.0 && has_row_above);
        let channel = top_row + fill;
        // **Geçmişten okuma ayrı bir döngü ve bu stil değil zorunluluk.**
        // Yukarıdaki döngünün `debug_assert!((0..rows).contains(&row))`
        // bekçisi negatif satırda patlardı, ve doldurulan satırlar
        // `drawn_rows`'a **girmemeli**: girselerdi öteleme kapanır, içerik
        // tabandan kopardı (R2.3, `27a0b98`'in maliyeti).
        //
        // Satır numarası **fill-yerel** (`0..top_row + fill`) ve sırası
        // geçmişin kendi sırası: `0` en eski (kesir varsa tepe satırı),
        // sonuncusu görünen pencerenin hemen üstü. Ekran satırına çeviren
        // taraf **çizen** taraf (phase-3) — bu crate "hangi satırlar" der,
        // "nereye" demez.
        //
        // `grid_clamp` bir emniyet kemeri: kanalın bütün satırları defterin
        // içinde (bant `fill <= history_size`, tepe satırı yukarıdaki kapı),
        // ama `bt-core`'da indeksleme panik yasağının altında (R2.5) ve yasağı
        // tip değil **çağrı yeri** taşıyor.
        let mut fill_continued: Option<(u16, u32)> = None;
        for fill_row in 0..channel {
            // **Ofset terimi tepe satırı için**: bant yalnız dibe yaslı
            // pencerede koşuyor ([`Session::fill_rows`]) ve orada terim sıfır,
            // yani bandın satırları bugünküyle aynı. Kaydırılmış pencerede
            // `fill == 0` ve kanalın tek satırı `Line(-offset - 1)`.
            let line = Line(i32::from(fill_row) - i32::from(channel) - offset)
                .grid_clamp(&*term, Boundary::Grid);
            let cells = &term.grid()[line];
            // **`zip`, indeksleme değil** ve gerekçesi panik yasağı (R2.5):
            // `Row`'un `Index`'i sınır dışında panikliyor ve `make denetim`
            // indekslemeyi göremiyor — aradığı şey `unwrap`/`expect`/`panic!`.
            // Bugün taşma yok, çünkü alacritty her satırı `columns()` boyunda
            // tutuyor (`grow_columns`/`shrink_columns`), ama o değişmez **bu
            // crate'te adlandırılmamış** ve bir reflow değişikliği onu kare
            // yolunda süreç öldüren bir paniğe çevirirdi. `zip` kısa olanda
            // duruyor, yani sınır tipin kendisinden geliyor;
            // `grid_clamp`'in satır için yaptığının sütun ikizi.
            for (col, cell) in (0..grid_cols).zip(cells) {
                // **Bandın çıpası ızgaranınkiyle aynı yerden** — hücrenin
                // kendi OSC 8 bağlantısından ([`Blocks::fill_anchors`]).
                // Geçmişe inen satır bağlantısını yanında götürüyor, yani
                // burada yeni bir kaynak yok; eksik olan yalnız **okuyan**
                // döngüydü.
                //
                // `alt_screen` kapısı yok: doldurma alternatif ekranda zaten
                // koşmuyor ([`Session::fill_rows`]), yani koşul ölü bir dal
                // olurdu.
                //
                // Devam satırı kuralı da ızgaranınkiyle aynı: bandın tepesinde
                // çok satırlı bir komutun ortası duruyorsa işaret orada değil.
                if let Some(id) = cell.hyperlink().and_then(|link| block_id(link.uri()))
                    && blocks.fill_anchors.last().map(|&(last, _)| last) != Some(id)
                    && fill_continued != Some((fill_row, id))
                {
                    let above = Line(i32::from(fill_row) - i32::from(channel) - offset - 1);
                    if block_row_continues(&term, above, id, clear_boundary) {
                        fill_continued = Some((fill_row, id));
                    } else {
                        blocks.fill_anchors.push((id, fill_row));
                    }
                }
                let flags = cell.flags;
                let dim = flags.contains(Flags::DIM);
                let hidden = flags.contains(Flags::HIDDEN);
                // **Seçim bu döngüde yok ve bu bir karar, unutulmuş bir dal
                // değil.** Doldurulan satırlar seçilemiyor (plan → Kapsam
                // Dışı; R5.1 orijinin üstündeki tıklamayı reddediyor) ve
                // vurgulanmaları "burada seçilebilir bir şey var" derdi —
                // gözün gördüğü ile panonun verdiği tam da bu deponun
                // yasakladığı yerde ayrışırdı. Izgara da aynı cevabı veriyor:
                // tamamı geçmişte kalan bir aralık `visible_range`'den
                // geçmiyor, yani bugün de çizilmiyor.
                let inverse = flags.contains(Flags::INVERSE);
                let back = if inverse {
                    color::resolve_fg(cell.fg, dim, colors, &theme)
                } else {
                    color::resolve(cell.bg, colors, &theme)
                };
                let bg = (back != background).then(|| color::linear_rgba(back));
                let ch = (!hidden && !flags.intersects(SPACERS) && cell.c != ' ').then_some(cell.c);
                let ruled = !hidden && flags.intersects(RULES);
                // Atlama kapısı ızgaranınkiyle **aynı** olmak zorunda: iki
                // taraf aynı hücreye bakıp farklı cevap verseydi doldurulan
                // satır ekrana çıktığındakinden farklı görünürdü.
                if bg.is_none() && ch.is_none() && !ruled {
                    continue;
                }
                let style = cell_style(cell, inverse, dim, ruled, colors, &theme);
                fill_sink(Cell {
                    col,
                    row: fill_row,
                    ch,
                    fg: style.fg,
                    bg,
                    bold: flags.contains(Flags::BOLD),
                    italic: flags.contains(Flags::ITALIC),
                    underline: style.underline,
                    underline_color: style.underline_color,
                    strikeout: style.strikeout,
                    // Izgara sink'iyle **aynı** bayrak, aynı gerekçe: bandın
                    // satırı ekrana çıktığındakiyle aynı genişlikte
                    // çizilmek zorunda.
                    wide: flags.contains(Flags::WIDE_CHAR),
                    cluster: cell_cluster(self.cluster, cell, ch, clusters),
                });
            }
        }

        // **Arama vurgusu** (033): çizilen satırların eşleşmeleri, ızgara ve
        // doldurma kanalı birlikte — ikisi ardışık satırlar
        // (`-offset - channel ..= rows - 1 - offset`), yani kanalın dibinden
        // ızgaranın tepesine sarılan bir eşleşme iki listeye bölünüyor ama
        // tek eşleşme kalıyor. Döngülerden **sonra**, çünkü bastırmanın üst
        // ucu ızgara döngüsünde doğuyor (seçim süzgecinin gerekçesi).
        if let Some(regex) = search_pattern.as_mut() {
            // Geçerli eşleşme önce içeriğine yapıştırılıyor (phase-5): çıktı
            // defteri kaydırdıysa vurgusu kaydırılmış yerinde.
            let now = self.ledger_now(&term);
            search::track(
                &term,
                &mut search_tracking,
                now,
                self.scrollback.load(Ordering::Relaxed),
            );
            search_visible(
                &term,
                regex,
                search_tracking.current.as_ref(),
                search,
                SearchWindow {
                    offset,
                    rows,
                    channel: i32::from(channel),
                    hidden: hidden.clone(),
                },
            );
        }

        // **İmleç döngülerden sonra kuruluyor** ve sebebi iki alan:
        // `content_rows` ancak ızgara döngüsü bitince, `fill` de ondan sonra
        // biliniyor. İmlecin girdilerinin tamamı (şekil, nokta, satır,
        // görünürlük) döngüden **önce** çözüldü, yani yukarıdaki "imleç
        // döngüden önce çözülüyor" cümlesi ayakta; burada yalnız kayıt
        // kuruluyor. Yer değiştirmesinin alternatifi iki alanı sıfırla doğurup
        // sonra düzeltmekti ve o, bir kare boyunca yanlış olan bir alan
        // demekti.
        let mut cursor = Cursor {
            col: cursor_col,
            row: cursor_screen_row,
            // **Bastırılan satırın imleci de çizilmez.** Caret dock'ta
            // (`Dock::caret`) ve ikisi birden çizilseydi kullanıcı iki caret
            // görürdü — üstelik ızgaradaki, altındaki harf bastırıldığı için
            // boş bir blok olarak dururdu.
            visible: cursor_visible && !caret_in_dock,
            // **Devrin cevabı sınırdan geçiyor, ikinci kez hesaplanmıyor.**
            // `visible` bu soruyu yanıtlamıyor: imleç uygulamanın gizlemesiyle
            // (`\e[?25l`) ya da geçmişe kaydırmayla da görünmez olur ve o iki
            // hâlde caret'i devralan kimse yok. İkisi ayrı sorulduğunda
            // ayrışıyorlardı — `dock::render` yalnız `caret_home`'u biliyordu,
            // buradaki üç ön koşulu (pencerenin dock'u, alternatif ekran,
            // tazelik) bilmiyordu — ve bayat aynada **iki caret** doğuyordu.
            caret_in_dock,
            // Bütçeye kırpılmış satır sayısı: tavan ızgaranın satırlarının
            // payı ve satır sayısı burada, `Term` kilidinin altında okundu —
            // çizen taraf onun ikinci bir kopyasını tutmuyor.
            //
            // Dock'u olmayan pencerede ve alternatif ekranda tek satır: bant
            // orada yok ve `bt-shell`'in resize'ı payı sıfırlayana kadarki geçiş
            // karesinde `Live` kalmış bir ayna (ZLE'nin `edit-command-line`'ı
            // vim'i açarken) ızgarayı bant kadar yukarı itmemeli
            // (`/code-review`).
            //
            // **Uzak oturumda sıfır** (036 Karar 8): ssh sürerken kabuğun
            // giriş satırı yok, dock yalnız bağlam satırına iniyor ve ızgara
            // aşağı çiziliyor (`bt-gpu`'nun bant fazlası). Alternatif ekranda
            // kural değişmiyor — dock orada zaten kalkıyor.
            input_rows: if !self.dock || alt_screen {
                1
            } else if remote {
                0
            } else {
                budget.fit(needed_rows, grid_rows)
            },
            // Şekil döngüden **önce** okundu (`cursor_shape`) ve oradan
            // geliyor: `RenderableCursor` onu `Term::cursor_style()`'dan
            // çözüyor, yani DECSCUSR ile ayarın varsayılanı zaten birleşmiş
            // hâlde. İkinci bir `cursor_style()` çağrısı aynı değeri ikinci
            // kez okumak olurdu.
            shape: caret_shape_of(cursor_shape),
            // **Çizilmeyen caret sönmez.** Ölçüt `visible` değil "bir yerde
            // caret var mı": `visible` dock devrinde `false` oluyor
            // (`cursor_visible && !caret_in_dock`) ve ona bakmak dock'ta
            // yazarken blink'i öldürürdü. Gizli imleçte (`\e[?25l`, htop)
            // ise hiçbir caret çizilmiyor ve blink'in açık kalması pencereyi
            // saniyede iki kez uyandırıp **birebir aynı** kareyi çizdirirdi —
            // R9'un "imleç gizlenir" durma koşulu bu satır.
            blink: requested_blink && (cursor_visible || caret_in_dock),
            // Blok opak ve altındaki metni örtüyor: zemin rengi onu yeniden
            // okunur kılıyor. Kaynak `theme`, hücrelerinkiyle **aynı** —
            // ayrışsalardı imlecin altındaki harf bloğa değil eski bir palete
            // göre seçilirdi.
            text: color::linear_rgba(background),
            // Aynı `RenderableContent`'ten, yani `row`'u kuran ofsetin ta
            // kendisi: ikisi ayrı okunsaydı araya düşen bir kaydırma
            // "ofset aynı ama satır oynadı" diye yanlış bir animasyon
            // başlatırdı.
            display_offset: offset,
            content_rows,
            // Yukarıdaki döngünün saydığı satır sayısı; doluluğa **girmiyor**
            // ([`Cursor::fill`]).
            fill,
            top_row,
            scrolled,
            scroll_frac,
            // Normalleştirmeden **sonraki** nesil: bu karede kesir düştüyse
            // çizen taraf süzülmeyi aynı karede bitirsin.
            scroll_generation: ScrollGlide::unpack(self.scroll_glide.load(Ordering::Relaxed))
                .generation,
            rows: grid_rows,
            // Faz 2 dolduruyor: koşan bloğun çıpasının bu karede **görünüp
            // görünmediği** ancak orada biliniyor ve saatin durma koşulu tam
            // olarak o.
            next_tick: None,
        };
        drop(term);
        // Desen yuvaya geri, **yalnız nesil aynıysa**: tur sürerken yeni bir
        // sorgu geldiyse (ya da arama kapandıysa) yuvadaki onundur.
        //
        //
        // Bastırılan satırlar da yuvaya, **mutlak** satır olarak ve arama
        // kapalıyken de: gezinme ve sayım karenin dışında koşuyor ve vurgunun
        // dışladığını dışlamak zorunda ([`search::eligible`]) — ilk sorgu
        // (aramanın henüz hiç karesi yokken) da. Bedeli içerik karesi başına
        // yarışmasız bir yaprak kilit.
        {
            let mut slot = lock(&self.search);
            slot.hidden = search_hidden;
            if let Some(pattern) = search_pattern
                && slot.generation == search_generation
                && slot.pattern.is_none()
            {
                slot.pattern = Some(pattern);
                slot.settle(search_taken, search_tracking);
            }
        }

        // **Faz 2**, `Term` kilidi düştükten sonra: kimlikler kabuk
        // defterinden renklendirilir ve süre sayaçları basılır.
        //
        // Sayaç hücreleri kilit **düştükten sonra** doğuyor ve bu bir kusur
        // değil sözleşmenin devamı: sink'in sırası sözleşmesiz (`Cell` kendi
        // satır/sütununu taşıyor) ve süre ızgarada değil kabuk defterinde
        // yaşıyor, yani `Term`'ü tutarak okunacak hiçbir şey yok.
        //
        // **Alternatif ekranda saat de yok:** dal hiç koşmuyor, yani
        // `next_tick` `None` kalıyor. vim'in içinde koşan bir komutun sayacı
        // zaten çizilmiyor ve görünmeyen bir sayı için kare istemek boşta
        // sıfır kare sözleşmesini bozardı.
        if !alt_screen {
            cursor.next_tick = self.resolve_blocks(blocks, &theme, grid_cols, &mut sink);
        }
        // **Devrin tutması saate `min`'lenerek giriyor, yazarak değil.**
        // `resolve_blocks` `next_tick`'i doğrudan **eziyordu** ve o yol koşan
        // bloğun çıpasının görünür olmasına bağlı; devir buna bağlanamaz.
        // Emsal, iki son tarihi tek saatte birleştiren `arm_clock` (014
        // phase-2): yakın olan kazanır, uzak olan kaybolmaz.
        //
        // Üç ön koşulun ikisi burada tekrar soruluyor (`self.dock`,
        // `!alt_screen`), çünkü tutma ancak `caret_in_dock`'u çevirebildiğinde
        // bir kare hak ediyor: dock'u olmayan ya da alternatif ekrandaki
        // pencerede cevap zaten `Grid` ve beklenen hiçbir şey yok.
        //
        // **Üçüncü ön koşul (tazelik) sorulmuyor ve sorulamaz da:** tutma
        // yalnız ham cevap `Grid` iken doluyor, ham cevabın `Grid` olması ise
        // `Running`, `Unavailable` ya da `Input`+`Idle` demek;
        // `suppressed_input()` ise yalnız `Input`+`Live`'da dolu. İkisi
        // **tek istisnayla ayrık**: tutulan `line-finish` (032 Karar 11)
        // aynayı `Live` bırakıyor, devri ise `Idle` sayıyor — o süre bastırma
        // da sürüyor ve `e`'nin damgası (⏎'in cevabı) tazelik kapısını
        // geçiriyor. İstisnanın kendi saati var (`end_left`) ve iki tutma
        // aynı andan sayıyor.
        if self.dock && !alt_screen {
            cursor.next_tick = crate::shell::sooner(
                cursor.next_tick,
                crate::shell::sooner(caret.hold_left, end_left),
            );
        }
        cursor
    }

    /// "Ekran kasten temizlendi" bayrağını bu karenin ızgarasıyla uzlaştırır
    /// (R1.1, R1.2).
    ///
    /// **Çağrı yeri sözleşmenin parçası: `Term` kilidi tutulurken.** İki
    /// sebeple ve ikisi de ölçülebilir birer kusur:
    ///
    /// 1. Sayaç kilidin altında okunuyor, çünkü okuyucu thread onu
    ///    `advance`'ten **önce** artırıyor ([`TappedPty::read`]). Kilitten
    ///    önce okunsaydı (`suppressed_input` örüntüsü) kare yolu eski sayacı
    ///    alır, sonra lease'in arkasında bekler ve **temizlenmiş** ızgarayı
    ///    bayrak kurulmamışken görürdü.
    /// 2. Okuma-değiştirme-yazma turunu serileştiren şey kilidin kendisi;
    ///    `compare_exchange`'e gerek bırakmayan da o. Kilit bırakıldıktan
    ///    sonra yazılsaydı, arada gelen taze bir `CSI 2 J` ezilirdi —
    ///    Ctrl-L'i sessizce geri alan dizi tam olarak bu.
    ///
    /// **Sayacın iki yazarı var** ([`Session::screen_clears`]): okuyucunun
    /// saydığı `CSI 2 J` ve ana thread'in terminal tarafı temizliği
    /// ([`Session::note_screen_clear`], ⌘K/⌥⌘K). Buradaki mantık ikisini
    /// ayırt etmiyor ve etmemeli: ikincisi kilit altında, uygulandıktan sonra
    /// artırıyor, yani (1)'in "henüz uygulanmamış" penceresi onda hiç yok ve
    /// bayrağı kuran kare zaten temiz ekranı görüyor. Temizlik geçmişi de
    /// sildiği için damga sıfırdan alınıyor.
    ///
    /// **Yeni nesil, doldurma kuralını aynı karede eziyor.** Sayaç
    /// [`Session::screen_seen`]'den farklıysa ortada henüz hesaba katılmamış
    /// bir temizleme var ve o kare bayrağı **kurar** — ızgara hâlâ dolu
    /// görünse bile, çünkü baytlar bu karede uygulanmamış olabilir. Ömrün
    /// ikinci yarısı ancak nesil eşitken işliyor.
    ///
    /// **Alternatif ekranın `CSI 2 J`'si nesli tüketir ama bayrağı kurmaz**
    /// (phase-1b, Fix A). Semantik alacritty'den: `ClearMode::All` ALT_SCREEN
    /// altında `reset_region(..)` çağırıyor, `clear_viewport()` **değil** —
    /// geçmiş büyümüyor ve birincil ekranın durumuna hiç dokunulmuyor, yani
    /// geri getirilmeyecek bir şey yok. Nesil yine de tüketiliyor: yoksa
    /// birikmiş sayaç alternatif ekrandan çıkışta bayrağı kurardı ve `vim`
    /// kullanan her oturumda doldurma kalıcı olarak kapanırdı — waive'lerin
    /// reddinin yarısı bu.
    ///
    /// **Düşürmenin ölçütü "temizlemeden sonra defter büyüdü mü".** Büyüdüyse
    /// geçmişe temizlemeden **sonra** satır düşmüş demektir ve doldurma o
    /// kadarını güvenle geri verebilir. Damga [`Session::screen_clear_history`]'de
    /// ve bir kare geç alınıyor; gerekçesi orada.
    ///
    /// **Bayrak bir eşik değil bir kapı: "kaç satır" sorusunun cevabı
    /// [`Session::fill_rows`]'ta.** Tek satırlık bir büyüme bayrağı düşürüyor
    /// ama boşluğun tamamını açmıyor — doldurma damgayı ikinci kez, bu kez
    /// **taze satır sayısı** olarak okuyor ve `fill`'i onunla kırpıyor.
    /// Kırpma olmasaydı bayrağın düşmesi ile "geri getirilebilir" arasındaki
    /// fark ekrana çıkardı; ölçüldü ve sayısı oradaki doc'ta.
    ///
    /// Bayrağın kırpmadan **ayrı** durmasının sebebi damgasız pencere: bayrak
    /// kurulduğu karede damga henüz alınmamış (`UNSTAMPED`) ve orada "taze
    /// satır" hesaplanamıyor. Kapı o kareyi kapatıyor.
    ///
    /// `content_rows == rows` kolu **yok**: phase-1'in ölçütüydü,
    /// [`Cursor::content_rows`] dock'lu pencerede giriş satırını saymadığı
    /// için (`drawn_rows.max(1)`, tavan `rows - 1`) pratikte erişilemez —
    /// ölçüldü (`phase-2.md` → Uygulama Notları §7b). Kırpma geldikten sonra
    /// ikinci bir kol olarak tutmanın da anlamı kalmadı: doymuş defterde
    /// (aşağıda) bayrağı düşürse bile `fill` kırpmadan sıfır çıkıyor, yani
    /// kol **ölü**.
    ///
    /// **İki koşul ölçütün üstünde:**
    ///
    /// - `alt_screen` — alternatif ekranda doluluk tanım gereği `rows` ve
    ///   `history_size()` etkin ızgaradan geliyor, yani alt ekranda sıfır;
    ///   onsuz `vim`'in her karesi bayrağı düşürürdü.
    /// - `scrolled` (`display_offset != 0`) — geçmişe kaydırılmış pencere
    ///   geçmiş satırlarıyla dolar ve `full` doğru olur; tanığı deponun kendi
    ///   sınaması `content_rows_come_from_the_visible_window_while_scrolled`.
    ///   Onsuz **tek bir tekerlek jesti** Ctrl-L'i geri alırdı: bayrak
    ///   kaydırma sırasında düşer, kullanıcı dibe dönünce
    ///   (`display_offset == 0`) doldurma temizlenmiş ekranı geri doldururdu.
    ///   R2.2'nin `display_offset == 0` kapısı bunu kurtarmıyor — o kapı
    ///   doldurmayı kaydırma *sırasında* durduruyor, bayrağın kaybı ise
    ///   kalıcı. (`/code-review`, 017 phase-1.) Damga ölçütü onu gereksiz
    ///   kılıyor (kaydırma defteri büyütmüyor) ama koşul ucuz ve bekçisi
    ///   `scrolling_into_history_never_drops_the_flag` yerinde duruyor.
    ///
    /// **Bilinen sınır 0 — doymuş defter.** `history_size()` `scrollback`'te
    /// doyuyor (alacritty `increase_scroll_limit`), yani on bin satırlık bir
    /// oturumda defter büyümeyi bırakıyor: damganın üstüne çıkacak bir sayı
    /// kalmıyor ve o oturumda bir Ctrl-L'den sonra doldurma bir daha
    /// koşmuyor. Tek damgayla kapatılamıyor — gereken şey "geçmişe itilen
    /// satır" sayacı ve o sayaç doymuş defterde de artmak zorunda, yani
    /// `history_size` ondan türetilemiyor. Yönü güvenli (doldurma yapmamak
    /// phase-1'in davranışı) ve phase-1b'ye göre bir **gerileme değil**:
    /// bayrağın phase-1'deki ömrü dock'lu pencerede zaten erişilemezdi.
    ///
    /// **Bilinen sınır 1:** eşzamanlı güncelleme (`\e[?2026h`) baytları
    /// `vte::ansi::Processor`'da tamponluyor, yani o blokun içindeki bir
    /// `CSI 2 J` uygulanmadan **önce** birden çok kare geçebilir ve nesil
    /// eşitlendikten sonra dolu bir ızgara bayrağı düşürebilir. Birincil
    /// ekranda eşzamanlı güncelleme kullanan kabuk yok; yanlışın yönü kötü
    /// ama olasılığı, kapatmanın bedelini (uygulanan bayt sayacı, alacritty'de
    /// kanca yok) hak etmiyor.
    ///
    /// **Bilinen sınır 2:** `alt_screen` de nesil gibi tek okumadan geliyor,
    /// yani mod değişimiyle `CSI 2 J`'yi **aynı** PTY okumasında taşıyan bir
    /// tur iki yönde de yanılabilir. `?1049h` + `2J` (her `vim` açılışı) bir
    /// kere fazladan kurabilir — kendi kendini onarıyor, çünkü birincil
    /// ekranda defter büyüyünce bayrak düşüyor. `?1049l` + `2J` ise gerçek bir
    /// temizlemeyi atlayabilir; yönü kötü ama böyle basan uygulama yok ve
    /// kapatmanın bedeli uygulanan bayta kanca takmak.
    ///
    /// **Bilinen sınır 3:** `2J` ile `3J` **ayrı** PTY okumalarına düşerse
    /// damga `2J` sonrası / `3J` öncesi boydan alınır; `3J` defteri sıfırlar
    /// (`clear_history`) ve bayrak defterin o eski damgayı yeniden aşmasını
    /// bekler. Pratikte olmuyor: `clear(1)` üçünü (`\e[H\e[2J\e[3J`) tek
    /// `write` ile basıyor, yani damga sıfırdan alınıyor. Çaresi `>` yerine
    /// `!=` **değil**: pencereyi büyütmek geçmişten satır çekiyor, yani defter
    /// **küçülebiliyor** ve `!=` orada bayrağı düşürüp temizleme öncesi
    /// satırları geri getirirdi.
    fn observe_screen_clear(&self, history: usize, alt_screen: bool, scrolled: bool) {
        let clears = self.screen_clears.load(Ordering::Relaxed);
        if clears != self.screen_seen.load(Ordering::Relaxed) {
            self.screen_seen.store(clears, Ordering::Relaxed);
            if !alt_screen {
                self.screen_clear_history
                    .store(Self::UNSTAMPED, Ordering::Relaxed);
                self.screen_cleared.store(true, Ordering::Relaxed);
            }
            return;
        }
        // **Bayrak düşükken damgaya dokunulmuyor** ve bu kırpmanın koşulu:
        // damga [`Session::fill_rows`]'a "temizlemeden beri kaç satır geldi"
        // diye de hizmet ediyor, yani hiç temizleme olmamış bir oturumda
        // `UNSTAMPED` kalmak **zorunda** — orada damgalansaydı kırpma taze bir
        // pencerede doldurmayı sıfıra indirirdi.
        if !self.screen_cleared.load(Ordering::Relaxed) {
            return;
        }
        let stamp = self.screen_clear_history.load(Ordering::Relaxed);
        if stamp == Self::UNSTAMPED {
            // **Damga alternatif ekranda alınmıyor** ve bu bir zorunluluk:
            // `history_size()` etkin ızgaradan geliyor, alt ekranda sıfır.
            // Ctrl-L'den hemen sonra `vim` açılsaydı damga sıfır olur, çıkışta
            // birincil ekranın defteri onu anında aşar ve bayrak yanlışlıkla
            // düşerdi. Damgasız kalan bayrak düşmüyor: `UNSTAMPED` tavan.
            if !alt_screen {
                self.screen_clear_history.store(history, Ordering::Relaxed);
            }
        } else if history > stamp && !alt_screen && !scrolled {
            self.screen_cleared.store(false, Ordering::Relaxed);
        }
    }

    /// Üstte kalan boşluğun kaç satırı geçmişle dolacak (R2.1, R2.2).
    ///
    /// **Doldurmanın tek boğaz noktası** ve bu bir geri alma şeridi (R2.4):
    /// burası sıfır döndüğünde ikinci sink hiç çağrılmıyor, [`Cursor::fill`]
    /// sıfır kalıyor ve sınırdan geçen kare bugünküyle **bit bit** aynı oluyor
    /// — 016'nın "yarıçap 0, hale 0" kolunun aynı örüntüsü. Koşullar iki yere
    /// dağılsaydı geri alma yolu da ikiye bölünürdü.
    ///
    /// Sayı `min(gap, taze satır)`: boşluk kadar satır isteniyor, temizlemeden
    /// beri o kadar gelmemişse gelen kadarı. Hiç temizleme olmamış oturumda
    /// "taze" defterin tamamıdır, yani formül R2.1'in `min(history_size, gap)`'i
    /// — yeni oturumda defter boş, `0` ve hiçbir ek okuma yok.
    ///
    /// **Üçüncü terim phase-1b'de geldi ve R2.1'den sapmadır**; gerekçesi
    /// aşağıda, gövdede, ölçülmüş hâliyle.
    ///
    /// **Dört kapı ve dördü de zorunlu:**
    ///
    /// - `self.dock` — doldurmanın tüketicisi dock'lu pencere. Ölçüt
    ///   [`Cursor::caret_in_dock`] **değil** pencerenin dock'u olması (R2.2):
    ///   devir tuşa, safhaya ve aynanın tazeliğine bağlı oynuyor ve boşluk
    ///   onlardan bağımsız.
    /// - `!alt_screen` — vim ve htop ızgaranın tamamını sahipleniyor, boşluk
    ///   zaten yok; dock da kalkıyor.
    /// - Bayrak temiz — kullanıcı ekranı **kasten** temizlediyse geri
    ///   gelmemeli (R1). Bayrağın ömrü [`Session::observe_screen_clear`]'da ve
    ///   çağrı sırası zorunlu: ömür **önce** işliyor. Bayrak kapı, kırpma
    ///   ölçü: kapı "hiç" der, kırpma "ne kadar".
    /// - `!scrolled` (`display_offset == 0`) — **bant dibe yaslı pencerenin
    ///   işi.** Kaydırılmış pencerede üstteki boşluk zaten geçmişle dolu ve
    ///   ikinci kez doldurmak aynı satırları iki kez gösterirdi.
    ///   Kapı bir dönem kaldırılmıştı ve belirti ölçüldü (2026-09-20, gözle
    ///   kontrol; kullanıcı): doldurma kaydırılmış pencerede de koşunca
    ///   `fill = rows - content_rows` her çentikte bir azalıyor,
    ///   `fill + offset` **sabit** kalıyor ve bandın okuma noktası
    ///   `-(fill + offset)` hiç kıpırdamıyor — kaydırma boşluk kadar çentik
    ///   boyunca ölü görünüyordu. Yaslamanın (011) kendisi bu kolda
    ///   **değişmiyor**: kaydırılmış pencerede de içerik tabana yaslı ve
    ///   `fill == 0` olduğu için doluluk büyüdükçe öteleme küçülüyor, yani
    ///   tepeden yeni satır giriyor. Kullanıcının kaybettiği süreklilik
    ///   kapının değil kaydırmanın sorumluluğu ve karşılığı
    ///   [`scroll_locked`]'ın `band` terimi.
    ///
    /// **Safha kapısı yok** ve bu ölçülmüş bir karar (`discussion.md` → Karar
    /// 5): Enter kolunda `\e[J` `Running` safhasından geçiyor ve safha kapısı
    /// olsaydı o karede `fill` sıfır kalır, dönüş animasyonsuz olurdu.
    fn fill_rows<T>(&self, term: &Term<T>, gap: u16, alt_screen: bool, scrolled: bool) -> u16 {
        if !self.dock || alt_screen || scrolled || self.screen_cleared.load(Ordering::Relaxed) {
            return 0;
        }
        // **Üçüncü terim: temizlemeden beri gelen satır sayısı.** Bayrağın
        // düşmesi "defterin en yenileri artık temizleme öncesine ait değil"
        // demiyor, yalnız "bir satır geldi" diyor; boşluk o bir satırdan
        // büyükse aradaki fark doğrudan kullanıcının sildiği ekrandır.
        // Ölçüldü (2026-09-20, `/code-review` 017 phase-1b): Ctrl-L → 12
        // satırlık çıktı (defter +3) → yedi satırlık delik, ve doldurulan yedi
        // satırın **dördü** temizleme öncesine aitti
        // (`["27","28","29","30","1","2","3"]`). Kırpmayla aynı sahne
        // `["1","2","3"]` veriyor.
        //
        // `UNSTAMPED` "hiç temizleme olmadı" demek ve orada kırpma **yok**:
        // taze bir pencerede defterin tamamı serbest. `saturating_sub` bir
        // emniyet kemeri — pencereyi büyütmek geçmişten satır çekiyor, yani
        // defter damganın altına inebiliyor ve orada doğru cevap sıfır.
        let fresh = self.fresh_history(term);
        gap.min(u16::try_from(fresh).unwrap_or(u16::MAX))
    }

    /// Önceki kareden bu yana ekranın tepesinden geçmişe kayan satır sayısı
    /// ([`Cursor::scrolled`]).
    ///
    /// Önceki karenin tepe satırının kimliği ([`Session::scroll_probe`])
    /// defterde aranıyor: `Line(-k)`'da bulunduysa `k` satır kaydı. Arama
    /// defterin tamamına kadar gidiyor, çünkü hızlı akan çıktı bir karede
    /// ekrandan fazla satır kaydırabilir; bedeli işaretçi karşılaştırması ve
    /// yalnız kimlik değiştiğinde ödeniyor. Bulunamadıysa (ters kaydırma
    /// satırı aşağı itti, `CSI 3 J` defteri sildi) cevap sıfır — yanlışın
    /// yönü güvenli, satırlar bugünkü gibi sıçrar.
    ///
    /// **Bilinen sınır:** kaydırma bölgesi tepeden başlayıp dipten önce biten
    /// bir program (altta sabit bir durum satırı) birincil ekranda kaydırırsa
    /// sayı doğru ama animasyon sabit satırı da oynatır. Bu programlar
    /// pratikte alternatif ekranda koşuyor ve orada sayı sıfır.
    fn scrolled_rows<T>(
        &self,
        term: &Term<T>,
        alt_screen: bool,
        scrolled: bool,
        grid_rows: u16,
        grid_cols: u16,
    ) -> u16 {
        let probe = if alt_screen || scrolled {
            0
        } else {
            row_identity(term, Line(0))
        };
        let size = (u32::from(grid_rows) << 16) | u32::from(grid_cols);
        let prev = self.scroll_probe.swap(probe, Ordering::Relaxed);
        let prev_size = self.scroll_probe_size.swap(size, Ordering::Relaxed);
        if probe == 0 || prev == 0 || prev == probe || prev_size != size {
            return 0;
        }
        let history = i32::try_from(term.history_size()).unwrap_or(i32::MAX);
        (1..=history)
            .find(|&k| row_identity(term, Line(-k)) == prev)
            .map_or(0, |k| u16::try_from(k).unwrap_or(u16::MAX))
    }

    /// Doldurma bandının kaymanın açtığı şeridi kapatan boyu ([`Cursor::fill`]).
    ///
    /// Izgaranın tepesi bu karede en çok `grid_top + scrolled` satır aşağıda:
    /// çizen taraf bir önceki karenin yerini bildirdi ([`Session::set_grid_top`])
    /// ve bu karede kayan satırlar onu o kadar aşağıdan başlatacak. Bant o
    /// kadar satırla pencerenin tepesine ulaşıyor.
    ///
    /// **Kapılar [`Session::fill_rows`]'unkiler** — alternatif ekran,
    /// kaydırılmış pencere, temizleme bayrağı ve temizlemeden beri gelen
    /// satır kırpması — tek farkla: dock'suz pencerede de koşuyor, ama yalnız
    /// ızgara **doluyken** (`gap == 0`). Orada üstte kalıcı bir boşluk yok, yani
    /// uzantı yerleşince tamamen ekranın dışında kalıyor ve dock'suz
    /// pencerenin "boşluk boş kalır" kuralına dokunmuyor. Dolmamış dock'suz
    /// pencerede uzantı boşluğa taşıp orada kalırdı.
    ///
    /// Tavan `gap + grid_rows`: çizen taraf kaymayı hedefin en çok bir ekran
    /// altından başlatıyor, bandın ondan fazlası hiç görünmez.
    ///
    /// **Kaydırılmış pencerede yalnız bandın kısalığı** ([`Session::grid_lowered`],
    /// 036): uzak oturumda ızgara bir satır artı boşluk aşağıda ve tepedeki
    /// şerit geçmişe bakarken de açık. O pay her çentikte **aynı**, yani
    /// kaydırma her çentikte tam bir satır ilerliyor — 017'nin reddettiği
    /// şey çentikle değişen öteleme boşluğunu doldurmaktı ve o burada
    /// doldurulmuyor. Satırlar pencerenin tepesinin üstündekiler, yani
    /// defterde ofsetin ötesinde kalan kadar.
    fn slide_fill_rows<T>(
        &self,
        term: &Term<T>,
        gap: u16,
        alt_screen: bool,
        offset: i32,
        scrolled: u16,
    ) -> u16 {
        if alt_screen || !(self.dock || gap == 0) || self.screen_cleared.load(Ordering::Relaxed) {
            return 0;
        }
        if offset != 0 {
            let above = self
                .fresh_history(term)
                .saturating_sub(usize::try_from(offset).unwrap_or(usize::MAX));
            return self
                .grid_lowered
                .load(Ordering::Relaxed)
                .min(u16::try_from(above).unwrap_or(u16::MAX));
        }
        let rows = u16::try_from(term.screen_lines()).unwrap_or(u16::MAX);
        // Bir ekran ya da fazlası tek karede kaydıysa çizen taraf kaymayı
        // bitiriyor (`bt_gpu::Motion::scroll_in`), yani uzatacak şerit yok.
        let scrolled = if scrolled >= rows { 0 } else { scrolled };
        let wanted = self
            .grid_top
            .load(Ordering::Relaxed)
            .saturating_add(scrolled)
            .min(gap.saturating_add(rows));
        wanted.min(u16::try_from(self.fresh_history(term)).unwrap_or(u16::MAX))
    }

    /// Temizlemeden beri geçmişe düşen satır sayısı; hiç temizleme olmadıysa
    /// defterin tamamı. İki doldurma hesabının ortak kırpması
    /// ([`Session::fill_rows`], [`Session::slide_fill_rows`]).
    fn fresh_history<T>(&self, term: &Term<T>) -> usize {
        let history = term.history_size();
        let stamp = self.screen_clear_history.load(Ordering::Relaxed);
        if stamp == Self::UNSTAMPED {
            history
        } else {
            history.saturating_sub(stamp)
        }
    }

    /// Izgaranın tepesinin çizildiği yer, pencerenin tepesinden satır
    /// cinsinden ve yukarı yuvarlanmış — bir sonraki [`Session::frame`]'in
    /// doldurma bandını ne kadar uzatacağı ([`Cursor::fill`]).
    ///
    /// Yazan çizen taraf (`bt-gpu`) ve her içerik karesinden **önce**: bu
    /// crate animasyonu görmüyor, yalnız sonucunu bir satır sayısı olarak
    /// alıyor. Yazılmazsa değer sıfır ve bant bugünkü boyunda kalıyor.
    ///
    /// `lowered` bunun **bandın kısalığından** gelen payı ([`Session::grid_lowered`]):
    /// kaydırılmış pencerede de kapatılan tek kısım.
    pub fn set_grid_top(&self, rows: u16, lowered: u16) {
        self.grid_top.store(rows, Ordering::Relaxed);
        self.grid_lowered.store(lowered, Ordering::Relaxed);
    }

    /// Faz 2: çıpalardan komut işaretleri, defterden renkler.
    ///
    /// Ayrı fonksiyon, çünkü **kilit rejimi ayrı**: burada yalnız `shell`
    /// yaprak kilidi alınıyor ve `Term` kilidi çoktan düşmüş olmalı. Garantiyi
    /// tip sistemi vermiyor, **çağrı yeri** veriyor: [`Session::frame`] guard'ı
    /// açıkça `drop` ediyor ve bu fonksiyon guard'ı parametre olarak almıyor,
    /// yani buraya bir `Term` guard'ı taşımanın yolu imzayı değiştirmekten
    /// geçer.
    ///
    /// Sırayı zorunlu kılan şey `Term` → `shell` yönünün tehlikesi **değil**
    /// (okuyucu thread zaten o sırada alıyor, bkz. modül başlığı); burada
    /// `Term`'ün bırakılmış olmasının sebebi kilidi kare boyunca tutmamak —
    /// renk çözümü ızgarayı hiç okumuyor ve `Term`'i tutarak yapılsaydı
    /// okuyucu thread'i boşuna bekletirdi.
    ///
    /// **Gövde bir döngüden ibaret ve bu tasarımın kendisi.** Eskiden burada
    /// üç kol daha vardı: pencerenin üstündeki bölgeyi bir önceki bloğa
    /// yazmak, çıpasız pencereyi koşan bloğa yazmak ve o kolun kaydırma
    /// kapısı. Üçü de "bu satır hangi bloğun" sorusunu **tahmin** ediyordu ve
    /// üçünün de kendi bilinen kusuru vardı. İşaret bölge değil satır olunca
    /// (bkz. [`Block`]) soru sorulmuyor bile: çıpası görünen satır
    /// işaretlenir, görünmeyen işaretlenmez.
    fn resolve_blocks(
        &self,
        blocks: &mut Blocks,
        theme: &Theme,
        cols: u16,
        mut sink: impl FnMut(Cell),
    ) -> Option<Duration> {
        // Yıkım: aşağıdaki kapatma yalnız `resolved`'ı ödünç alsın, döngü
        // `anchors`'ı okuyabilsin. Tek bir `&mut blocks` ikisini de tutar ve
        // ödünç denetleyicisi haklı olarak reddeder.
        let Blocks {
            anchors,
            resolved,
            fill_anchors,
            fill_resolved,
            ..
        } = blocks;
        let shell = lock(&self.shell);
        let running = shell.running();
        let counter_fg = theme.dim_linear();
        // **Saatin durma koşulu burada doğuyor** (013 phase-2): koşan bloğun
        // çıpası bu karede görünmüyorsa (yukarı kaymış, alternatif ekran)
        // sayaç da çizilmiyor, yani ilerletecek bir şey yok ve saat sönüyor.
        // Kimlik defterde olup ekranda olmadığında kare istemek, kimsenin
        // görmediği bir sayıyı güncellemek olurdu.
        let mut next_tick = None;
        let mut counted_row = None;
        for &(id, row, last_col) in anchors.iter() {
            // **Sayaç şeritten bağımsız.** Kodu okunamamış bir blok
            // (`Finished { exit: None }`) şerit **almıyor** ("bilinmeyen
            // çizilmez") ama süresi biliniyor; onu da gizlemek bilinen bir
            // şeyi saklamak olurdu.
            if let Some(duration) = shell.duration(id, running) {
                let live = running == Some(id);
                // Aynı satırda ikinci bir çıpa: sayaç **bir kez** çiziliyor.
                // `last_col` yalnız son çıpaya işleniyor (döngünün
                // `last_mut`'u), yani öncekiler sıfır mürekkeple geçip aynı
                // sütunlara ikinci bir sayaç basardı — üst üste binen
                // glyph'ler. zsh'in `PROMPT_SP`'si bunu pratikte zor
                // doğuruyor ama savunma yorumda değil kodda olmalı
                // (`/code-review`, 013 kapı).
                let taken = counted_row == Some(row);
                // Eşiğin altı çizilmiyor ama **döngüden çıkılmıyor**: şerit
                // aşağıda, süreden bağımsız çözülüyor.
                let drawn = if taken {
                    false
                } else if duration >= COUNTER_FLOOR {
                    let counter = Counter::new(
                        duration,
                        if live {
                            Precision::Whole
                        } else {
                            Precision::Tenths
                        },
                    );
                    let text = counter.as_str();
                    match Self::counter_col(text.chars().count(), last_col, cols) {
                        Some(start) => {
                            for (offset, ch) in text.chars().enumerate() {
                                sink(Cell {
                                    col: start.saturating_add(offset as u16),
                                    row,
                                    // **Boşluk glyph değil.** Izgara yolu da
                                    // `' '`i `None`'a düşürüyor (`let ch`) ve
                                    // sebebi orada yazılı: boşluk atlasta bir
                                    // yuva, tamponda bir instance ve GPU'da
                                    // tamamen şeffaf bir dörtlü harcardı.
                                    // `1m 05s`'in boşluğu için ödenmesin —
                                    // üstelik `yuva=` jetonunu da şişirirdi
                                    // (`/code-review`, 013 kapı).
                                    ch: (ch != ' ').then_some(ch),
                                    fg: counter_fg,
                                    // Zemin **yok**: sayaç ızgaranın üstünde
                                    // yüzen bir rozet değil, satırın sağ
                                    // ucundaki boş hücrelere yazılmış metin.
                                    // Zemin verilseydi seçim vurgusunun ve
                                    // ters çevrilmiş imlecin üstüne basardı.
                                    bg: None,
                                    bold: false,
                                    italic: false,
                                    underline: UnderlineStyle::None,
                                    underline_color: None,
                                    strikeout: false,
                                    // Sayacın rakamları ASCII: `Counter`
                                    // yalnız rakam, `.`, `m` ve `s` üretiyor.
                                    wide: false,
                                    cluster: None,
                                });
                            }
                            counted_row = Some(row);
                            true
                        }
                        // Sığmadı: sayaç bu satırda **hiç** çizilmiyor ve
                        // `drawn` yanlış kalıyor, yani saat de kurulmuyor.
                        None => false,
                    }
                } else {
                    false
                };
                // **Saat yalnız görünen bir şey için kuruluyor.** İki meşru
                // hâl var: sayaç çizildi (bir sonraki kademede değişecek) ya
                // da süre henüz eşiğin altında (bir saniye dolunca
                // **belirecek**). Sığmadığı için çizilmeyen sayaç üçüncü bir
                // hâl ve orada uyanmak, kimsenin görmediği bir sayıyı
                // güncellemek olurdu — üstelik metin yalnız uzadığı için
                // sonradan sığması da beklenmiyor (pencere genişlerse zaten
                // hasar doğuyor ve karar yeniden veriliyor).
                if live && (drawn || duration < COUNTER_FLOOR) {
                    next_tick = Some(crate::shell::next_tick(duration));
                }
            }
            // Defterin tanımadığı kimlik (halka dolaştı, sayaç sıfırlandı) ve
            // koşmayan `Pending` (boş prompt'a basılan Enter, bekleyen prompt)
            // `None` döner: **bilinmeyen çizilmez**, 010'un savunma tezi.
            let Some(stripe) = shell.stripe(id, running) else {
                continue;
            };
            resolved.push(Block {
                row,
                stripe: match stripe {
                    Stripe::Running => theme.accent_linear(),
                    Stripe::Success => theme.success_linear(),
                    Stripe::Error => theme.error_linear(),
                },
            });
        }
        // **Bandın şeritleri aynı defterden, ayrı listeye**
        // ([`Blocks::fill_slice`]). Döngü sayaçsız: süre hücre üretiyor ve
        // bandın sink'i bu fazda kapalı — karar ve gerekçe `fill_slice`'ın
        // doc'unda. "Bilinmeyen çizilmez" kuralı burada da aynen geçerli.
        for &(id, row) in fill_anchors.iter() {
            let Some(stripe) = shell.stripe(id, running) else {
                continue;
            };
            fill_resolved.push(Block {
                row,
                stripe: match stripe {
                    Stripe::Running => theme.accent_linear(),
                    Stripe::Success => theme.success_linear(),
                    Stripe::Error => theme.error_linear(),
                },
            });
        }
        next_tick
    }

    /// Süre sayacının başlayacağı sütun; sığmıyorsa `None` ve sayaç o satırda
    /// **hiç** çizilmez.
    ///
    /// **Çakışmada sayaç kaybeder** (013 Karar 7): kullanıcının yazdığı komut
    /// hiçbir koşulda örtülmez — seçim vurgusu da öyle. Tersi seçilseydi uzun bir komutun son
    /// harfleri sessizce bir sayıya dönerdi ve belirti "komutum yanlış
    /// görünüyor" diye okunurdu.
    ///
    /// Ölçüt en az **bir boş hücre**: sayaç komutun son harfine yapışırsa
    /// ikisi tek kelime gibi okunur.
    ///
    /// **Sağ kenarda da bir hücre boş kalıyor** ve sebebi simetri: ızgara sol
    /// kenardan bir pay bırakıyor (`CellMetrics::gutter_px`, 8 pt — kabaca bir
    /// hücre), sağdan bırakmıyor (`split_into_grid` yalnız bir pay düşüyor).
    /// Son sütuna oturan sayaç bu yüzden pencerenin kenarına **yapışıyordu**;
    /// gözlendi (kullanıcı). Payı hücre cinsinden vermek kararın burada
    /// kalmasını sağlıyor — piksel `bt-core`'un bilmediği bir birim — ve
    /// sonuç iki yanı da bir hücre boş, yani sayaç yüzer gibi duruyor.
    ///
    /// **Izgaranın kendi sağ kenarı hâlâ paysız** ve bu bilinçli: uzun bir
    /// çıktı satırının kenara dayanması terminalin olağan davranışı, sayaç
    /// ise bizim koyduğumuz bir işaret. Payı ızgaranın tamamına vermek
    /// `split_into_grid`'i, yani PTY'ye bildirilen sütun sayısını değiştirir.
    fn counter_col(len: usize, last_col: u16, cols: u16) -> Option<u16> {
        let len = u16::try_from(len).ok()?;
        // Sağdaki pay `len`'in üstüne: sayaç `cols - 1`'e değil `cols - 2`'ye
        // kadar uzanıyor.
        let start = cols.checked_sub(len.checked_add(1)?)?;
        // `last_col` hiç dolu hücresi olmayan satırda 0 ve o hâlde de doğru
        // çalışıyor: 0. sütun boşsa sayaç yine 1'den itibaren serbest.
        (start > last_col.saturating_add(1)).then_some(start)
    }

    /// Hasarı **tüketir**: `true` → çizilecek yeni içerik var.
    ///
    /// [`Session::frame`]'in içinden çıkarıldı ve gerekçesi orada. Burada
    /// duran yarısı maliyet: bayrak kilit istemez, kilit ise ucuz değil —
    /// `FairMutex::lock()` iki muteks alır ve okuyucu thread PTY'den okumaya
    /// başlamadan önce aynı sıraya giriyor. Boştaki kare o sıraya hiç
    /// girmesin.
    ///
    /// Swap ile `frame()`'in kilidi arasına düşen bir `Wakeup` bayrağı
    /// yeniden diker; en kötüsü fazladan bir kare, kaçan kare değil.
    ///
    /// **Tüketen tek yer burası olmalı.** İki çağıran arka arkaya sorarsa
    /// ikincisi `false` alır ve o karenin içeriği çizilmeden kalır.
    pub fn take_damage(&self) -> bool {
        self.adapter.0.dirty.swap(false, Ordering::AcqRel)
    }

    /// Fareyle seçimin iki ucu — aralık modeli burada yaşar, çünkü "hangi
    /// hücreler" grid bilgisidir (Karar 1).
    ///
    /// Uçlar görünür pencere cinsinden (sütun, satır) **ve yarısıyla** gelir;
    /// grid satırına o anki `display_offset` ile inilir. Aralık grid
    /// mutlağında tutulduğu için alacritty'nin kendi döndürme mantığı
    /// kaydırınca onu içerikle birlikte taşır — buraya fazladan bir kaydırma
    /// kolu yazılmaz.
    ///
    /// Yeni seçimin **kurucusu** budur (fare basışı); sürüklemenin aktif ucu
    /// [`Session::update_selection`]'dan geçer ve çapaya dokunmaz.
    ///
    /// Her uç yarısını kendisi taşır — kuralın tamamı [`CellHalf`]'ta. Uçlar
    /// sırasız verilebilir: alacritty bölgeyi her okuyuşta kendisi sıralıyor
    /// (`is_empty`, `to_range`, `rotate`), yani burada ayrıca sıralanmıyor.
    ///
    /// Değişim kirli bayrağını diker **ve uyandırır**: `resize`'ın tersine
    /// uyandırma `bt-shell`'e bırakılamaz — farenin vardığı `view` link'e
    /// uzanamıyor, elindeki tek tutamak bu oturum.
    ///
    /// `kind` seçimin adımı ([`SelectKind`]) ve seçimle birlikte saklanır:
    /// sürükleme ([`Session::update_selection`]) ve Shift+tıklama
    /// ([`Session::extend_selection`]) onu koruyor.
    pub fn set_selection(&self, kind: SelectKind, start: SelectionPoint, end: SelectionPoint) {
        let term = self.term.lock();
        let (start_point, start_side) = anchor(&term, start);
        let (end_point, end_side) = anchor(&term, end);
        let mut selection = Selection::new(kind.alacritty(), start_point, start_side);
        selection.update(end_point, end_side);
        self.store_selection(term, selection);
    }

    /// Shift+tıklama: var olan seçimin **ucunu** `end`'e taşır, çapaya ve
    /// tipe dokunmadan — kelime seçimi kelime adımıyla, satır seçimi satır
    /// adımıyla uzar. Seçim yoksa tıklanan noktadan boş bir `Simple` başlar
    /// (sürüklemesiz tık gibi); sürükleme oradan harf adımıyla büyür.
    ///
    /// [`Session::update_selection`]'dan **ayrı**, çünkü o seçimsiz hâlde
    /// bilerek susuyor (basışsız hareket seçim doğurmamalı); Shift+tıklama
    /// ise bir basış, yani jest başlatabilir.
    ///
    /// Geçmişe itilmiş (ekranda çizilmeyen) bir seçim de uzar: seçim her
    /// hâlde saklanıyor ([`Session::set_selection`]'ın gerekçesi) ve çapası
    /// grid mutlağında duruyor — Terminal.app'in davranışı.
    pub fn extend_selection(&self, end: SelectionPoint) {
        let term = self.term.lock();
        let (point, side) = anchor(&term, end);
        let selection = match term.selection.clone() {
            Some(mut selection) => {
                selection.update(point, side);
                selection
            }
            None => Selection::new(SelectionType::Simple, point, side),
        };
        self.store_selection(term, selection);
    }

    /// Edit ▸ Select All (⌘A): geçmişin tepesinden ekranın dibine bütün
    /// satırlar — Terminal.app'in normu. `Lines`, çünkü "bütün" satır
    /// cinsinden bir söz ve uçların yarısı sorulmamalı.
    ///
    /// Uçlar grid mutlağında kuruluyor, `anchor`'dan geçmeden: o görünür
    /// pencere hücresini grid satırına indiriyor, burada ise pencere hiç
    /// sorulmuyor.
    ///
    /// **Sahibe gidiyor** (031 Karar 7): dock caret'in sahibiyken ve satırda
    /// metin varken dock'un bütün `BUFFER`'ı seçiliyor — kullanıcının yazdığı
    /// yer orası. Satır boşsa ızgara: dock'ta seçilecek bir şey yok ve boş
    /// bir ⌘A'nın geçmişi seçmesi Terminal.app'in normu.
    pub fn select_all(&self) {
        if self.caret_in_dock.load(Ordering::Relaxed) && self.dock_select_all() {
            return;
        }
        let term = self.term.lock();
        let top = Point::new(term.topmost_line(), Column(0));
        let bottom = Point::new(term.bottommost_line(), term.last_column());
        let mut selection = Selection::new(SelectionType::Lines, top, Side::Left);
        selection.update(bottom, Side::Right);
        self.store_selection(term, selection);
    }

    /// Seçimin üç kurucusunun ortak kuyruğu: saklar ve **çizilen aralık**
    /// değiştiyse kare ister. Kilit burada bırakılıyor, ki `request_frame`
    /// `Term` kilidi tutulurken koşmasın.
    fn store_selection(
        &self,
        mut term: impl DerefMut<Target = Term<Adapter>>,
        selection: Selection,
    ) {
        // Kapı **çizilen aralığa** bakar, uçlara değil (`visible_range`).
        // Uçlar karşılaştırılsaydı sürükleme her hücre sınırında
        // (`(c, Right)` → `(c+1, Left)`, aynı aralık) ve her sürüklemesiz tıkta
        // (boş seçim) ekrana hiçbir şey eklemeyen bir kare isterdi — sürükleme
        // bugün `update_selection`'dan geçiyor ve orada da aynı kapı. Seçim yine
        // de **her seferinde** saklanır: "çizilen aralık aynı" seçimin aynı
        // olduğu anlamına gelmez. Çıktının geçmişe ittiği bir seçimin yerine
        // boş bir tık gelince iki taraf da görünmez (`None == None`), ama
        // saklanmasaydı Cmd-C ekranda olmayan eski metni kopyalardı —
        // `selection_text` görünürlüğe bakmıyor.
        let changed =
            visible_range(term.selection.as_ref(), &term) != visible_range(Some(&selection), &term);
        term.selection = Some(selection);
        drop(term);
        // **Tek sahip** (031 Karar 7): ızgarada seçim başlamak dock'unkini
        // kaldırıyor. `Term` kilidi düştükten sonra — yaprak kilit onun altına
        // girmiyor.
        let dock = self.clear_dock_selection();
        if changed || dock {
            self.request_frame();
        }
    }

    /// Sürüklemenin aktif ucu: var olan seçimin **yalnız bitişini** taşır.
    ///
    /// Seçimin başlangıcı (çapa) burada hiç okunmuyor — alacritty onu grid
    /// mutlağında tutuyor, [`Session::set_selection`]'ın kurduğu yerde;
    /// `anchor` yardımcısı yalnız **bitişin** alacritty karşılığını çözüyor. Çapayı pencere
    /// satırı olarak `bt-shell`'de tutup her olayda iki uçla yeniden kurmak
    /// kaydırmaya dayanmıyordu: basılı sürüklemenin ortasında pencere
    /// kayınca aynı satır numarası başka bir içeriği gösterir ve seçim
    /// başka yerden başlamış olurdu. Grid-mutlak çapa hem [`Session::scroll_wheel`]
    /// ile kaymayı hem de çıktının içeriği yukarı itmesini (alacritty
    /// `rotate`) kendiliğinden taşıyor.
    ///
    /// Seçim yoksa sessizdir: yeni seçim **doğurmaz**. Seçim sürüklemenin
    /// ortasında da düşebilir — alternate screen'e geçiş onu siliyor
    /// (`swap_alt`) ya da çıktı başlangıcı geçmişin dışına itiyor — ve o
    /// hâlde fare hareketi basışsız bir seçim başlatmamalı.
    ///
    /// Kare kapısı [`Session::set_selection`]'ınkiyle aynı: çizilen aralık
    /// değişmediyse kare istenmez.
    pub fn update_selection(&self, end: SelectionPoint) {
        let mut term = self.term.lock();
        let (point, side) = anchor(&term, end);
        let before = visible_range(term.selection.as_ref(), &term);
        let Some(selection) = term.selection.as_mut() else {
            return;
        };
        selection.update(point, side);
        let changed = before != visible_range(term.selection.as_ref(), &term);
        drop(term);
        if changed {
            self.request_frame();
        }
    }

    /// Seçimi temizler. Ekranda çizili bir aralık yoksa sessizdir — seçim hiç
    /// yoksa da, sürüklemesiz tıkın bıraktığı boş seçimse de, çıktının
    /// geçmişe ittiği bir seçimse de: bayrak dikilmez, kare istenmez.
    pub fn clear_selection(&self) {
        // Kilit gövdeden önce düşüyor: `request_frame` `Term` kilidi (çift
        // muteksli `FairMutex`) tutulurken koşmamalı — `set_selection`'daki
        // `drop` disiplininin aynısı. Geçici kilit `let`'in sonunda düşüyor.
        let had = clear_selection_locked(&mut self.term.lock());
        if had {
            self.request_frame();
        }
    }

    /// Tekerlek ve trackpad; artı değer geriye (yukarı). Karar kipe bakar ve
    /// kip `Term`'de yaşıyor, yani karar burada — `bt-shell` kip tutmaz.
    /// Tablonun kendisi saf (`input::wheel_route`), sırası alacritty'nin
    /// `scroll_terminal`'ınınki:
    ///
    /// 1. **Fare raporlama kipi** (1000/1002/1003), ekran fark etmez → satır
    ///    başına bir tekerlek raporu, işaretçinin hücresi için (vim `mouse=a`,
    ///    htop). Ok gitseydi rapor bekleyen uygulamada boşa düşerdi.
    /// 2. **Alternate screen + DECSET 1007**, Shift basılı değil → satır başına
    ///    bir ok (less, man). 1007 alacritty'de varsayılan açık.
    /// 3. **Alternate screen**, geri kalanı → [`Wheel::Ignored`]: birincil
    ///    ekranın geçmişine inmek uygulamanın ekranının altını gösterirdi.
    /// 4. **Birincil ekran** → görünen pencere kayar, [`Wheel::Scrolled`].
    ///
    /// **Kare.** Kaydırma kirli bayrağını elle diker **ve uyandırır**:
    /// alacritty'nin `Term::scroll_display`'i kareyi kendisi istemez (tek olayı
    /// `MouseCursorDirty`, `Adapter` yutuyor) ve tekerleğin vardığı `view`
    /// link'e uzanamıyor. Yalnız kayan pencere kare ister — trackpad momentumu
    /// geçmişin ucunda da olay yağdırır. Uygulamaya gönderim ise kare
    /// **istemez**: uygulama ekranını yeniden çizince okuyucunun `Wakeup`'ı
    /// kareyi getirir.
    ///
    /// **Gönderim `write_owned`'dan geçmez**, doğrudan kanala gider. O kapı
    /// girdide pencereyi dibe döndürüyor ve seçimi temizliyor: birincil ekranda
    /// fare kipi açıkken (pencereyi Shift+PgUp geçmişe almış olabilir) her
    /// tekerlek raporu pencereyi dibe atar ve raporlanan hücre kullanıcının
    /// baktığı yerden kayardı; alternate screen'de dönüş zaten boş iş ve ikinci
    /// bir `Term` kilidi olurdu. alacritty'nin rapor ve ok yolu da ne dibe
    /// dönüyor ne seçimi temizliyor (`write_to_pty`'ye doğrudan).
    ///
    /// **Seçim de durur** ve bunun dayanağı alacritty ile aynı davranmak, bir
    /// garanti değil. Bedeli bilinen: tekerlek oku klavyenin okuyla aynı
    /// baytlar ve ekranı satır kaydırmakla değil **baştan çizerek** yenileyen
    /// bir uygulamada (htop, fzf listesi) vurgu aynı hücrelerde kalır, altındaki
    /// metin değişir — `write_owned`'daki "girdi seçimi temizler" gerekçesinin
    /// aynısı. Satır kaydıran uygulamada (less'in `LF`/`\eM`'si) seçim içerikle
    /// birlikte döner ve doğru kalır. Uygulamanın yazdığı hücrede seçimi
    /// düşürmek iki yolu da kapsardı; o, girdiden bağımsız ayrı bir iş.
    ///
    /// **İşaretçi** (`at`) görünen pencerenin hücresidir ve `half` okunmaz:
    /// rapor hücre çözünürlüğünde (SGR-pixel, 1016, kapsam dışı). Tip
    /// [`SelectionPoint`], çünkü `bt-shell`'in fare çevirisi onu zaten veriyor
    /// ve alanları adlı — sütun ile satırın yer değiştirmesi derlenmez. Hücre
    /// `display_offset` ile uygulamanın satırına iner; satır uygulamanın
    /// ekranında değilse (pencere geçmişte, satır `< 0`) rapor gitmez —
    /// alacritty'nin kuralı.
    ///
    /// **Tekrar kırpılır**: bir olayda uygulamaya en çok **bir sayfa** (görünen
    /// satır sayısı) gider. `bt-shell` deltayı `f64`'ten doyurarak çeviriyor ve
    /// `i32::MAX` tekrarlık bir tampon kurulmamalı. Sayfa ölçülmüş bir sınır
    /// değil, `Term`'in verdiği tek ölçü; kaydırma yolunun kırpması
    /// (ulaşılabilir aralık, `scroll_locked`) burada yok, çünkü uygulamanın
    /// ne kadar kayabileceğini terminal bilmiyor.
    ///
    /// **Olay iki hâliyle birden iniyor** (027 Karar 6): `rows` kesirli
    /// miktar, `lines` aynı olayın tam satırı (`bt-shell`'in artık yolu).
    /// Rota **önce** seçiliyor ve kesir yalnız kaydırma kolunda anlam taşıyor;
    /// ok ve rapor kolları `lines`'la, niyete bakmadan. Kaydırma kolunda
    /// niyetin her varyantı ne yapıyor [`ScrollIntent`]'te, dönen
    /// [`Wheel::Scrolled`]'in kesirli kollardaki anlamı orada.
    ///
    /// **Kesirli kolun karesi**: ofset ya da kesir değiştiyse ve süzülme
    /// isteği biriktiyse kare istenir, hiçbiri değilse istenmez — geçmişin
    /// ucunda yağan momentum da, dipte aşağı dönen çentik de boş kare
    /// üretmemeli (R1.2).
    pub fn scroll_wheel(
        &self,
        rows: f64,
        lines: i32,
        intent: ScrollIntent,
        at: SelectionPoint,
        shift: bool,
    ) -> Wheel {
        let mut term = self.term.lock();
        let unit = match input::wheel_route(*term.mode(), shift) {
            WheelRoute::Scroll => {
                // Yol zaten birincil ekran; `None` yalnız `scroll_locked`'ın
                // kendi kip kapısından gelebilir.
                let scrolled = self.scroll_by_intent(&mut term, rows, lines, intent);
                drop(term);
                let Some((moved, changed)) = scrolled else {
                    return Wheel::Ignored;
                };
                if changed {
                    self.request_frame();
                }
                return Wheel::Scrolled(moved);
            }
            WheelRoute::Ignore => return Wheel::Ignored,
            WheelRoute::Arrows => {
                let arrow = if lines > 0 { Arrow::Up } else { Arrow::Down };
                input::arrow(arrow, *term.mode()).to_vec()
            }
            WheelRoute::Report(encoding) => {
                let offset = term.grid().display_offset() as i32;
                let line = viewport_point((at.col, at.row), offset).line;
                let button = if lines > 0 { WHEEL_UP } else { WHEEL_DOWN };
                let report = u16::try_from(line.0)
                    .ok()
                    .and_then(|row| input::mouse_report(encoding, button, true, at.col, row));
                let Some(report) = report else {
                    return Wheel::Ignored;
                };
                report
            }
        };
        let count = lines.unsigned_abs().min(term.screen_lines() as u32) as usize;
        drop(term);
        // Sıfır satır boş bir `Msg::Input` olurdu ve o, `EventLoop`'un
        // yazıcısını kalıcı olarak kilitler (`Adapter::reply`).
        if count == 0 {
            return Wheel::Ignored;
        }
        self.send(Msg::Input(unit.repeat(count).into()));
        Wheel::Sent
    }

    /// Fare düğmesi: basış ya da bırakma. Karar kipe bakar ve kip `Term`'de
    /// yaşıyor, yani karar burada — `bt-shell` kip tutmaz ve **kipi
    /// soramaz** (`bracketed_paste`'in yazılı gerekçesi: kapı dışarıdan
    /// sorulabilseydi by-pass edilebilirdi).
    ///
    /// **Arbitraj tek cümle:** fare kipi (1000/1002/1003) açık ve Shift
    /// basılı değilse jest uygulamanın, değilse terminalin. Shift **tek**
    /// kaçış yolu ve onsuz uygulama içinde fareyle metin seçilemezdi;
    /// tablonun kendisi saf (`input::button_route`) ve tekerlekle
    /// asimetrisinin gerekçesi orada.
    ///
    /// **Bırakma basışın rotasını izler** ve rotayı `bt-shell` basışta
    /// kilitliyor: Shift her olayda okunsaydı sürüklemenin ortasında Shift'i
    /// bırakmak seçim jestini rapor jestine çevirirdi. Bu yüzden `pressed`
    /// `false` olan çağrıda **Shift sorulmuyor**. Kip **yine de soruluyor**
    /// ve bu R6'nın ("bırakma asla düşürülmez") daraltması: söz koordinat
    /// içindi, kip için değil. Basış raporlandıktan sonra uygulama çıkıp
    /// `\e[?1000l` göndermişse (vim kapandı) bırakma raporu **kabuğa**
    /// giderdi — `\e[<0;5;3m` bir zsh komut satırına düşerdi. alacritty de
    /// `on_mouse_release`'te kipi yeniden soruyor.
    ///
    /// **Koordinat basışta reddedilir, bırakmada kırpılır.** İşaretçinin
    /// satırı `display_offset` ile uygulamanın satırına iner; basışta o satır
    /// uygulamanın ekranında değilse ya da kodlamaya sığmıyorsa rapor gitmez
    /// ([`Click::Ignored`], alacritty'nin kuralı). Bırakmada aynı ret
    /// uygulamada **takılı kalmış bir düğme** bırakırdı — jest zaten
    /// başlamış — ve hafifçe yanlış bir koordinat ondan iyidir
    /// ([`crate::input::MouseEncoding::clamp`], bekçisi
    /// `release_follows_press`).
    ///
    /// **Gönderim `send`'den geçiyor, `send_input`'tan değil** (tekerleğin
    /// rapor kolunun aynısı, `wheel_and_replies_keep_the_selection`): rapor
    /// kullanıcının yazdığı bir şey değil, uygulamaya iletilen bir olay.
    /// Seçim durur ve pencere dibe dönmez — dönseydi geçmişe bakan pencere
    /// her tıkta dibe fırlar, raporlanan hücre de kullanıcının baktığı yerden
    /// kayardı. Bedeli `write_owned`'ın doc'undaki gerekçenin aynısı ve
    /// bilinerek ödeniyor: uygulama rapordan sonra ekranını yeniden çizerse
    /// vurgu aynı hücrelerde kalır, altındaki metin değişir.
    ///
    /// **Kare istenmez:** uygulama ekranını yeniden çizince okuyucunun
    /// `Wakeup`'ı kareyi getirir (tekerleğin rapor kolu gibi).
    ///
    /// [`Click::Select`] kolunda seçimi **burası başlatmıyor**: çapa
    /// `bt-shell`'in `set_selection`'ıyla iniyor, yani bu kolda iki `Term`
    /// kilidi var. Yarış adıyla yazılı ve zararsız: kip iki kilit arasında
    /// kapanırsa çapa yine atılır ve kullanıcı bir kez fazladan seçim
    /// başlatır; ters yönde (kip açılırsa) rapor değil seçim olur. İkisi de
    /// bir tıklık ve yönü güvenli — çapayı buraya almak `Session`'ın
    /// arbitrajına seçim politikasını da yüklerdi.
    pub fn mouse_button(
        &self,
        button: MouseButton,
        pressed: bool,
        at: SelectionPoint,
        modifiers: MouseModifiers,
    ) -> Click {
        let term = self.term.lock();
        // Shift **yalnız basışta** soruluyor: bırakmanın rotası basışta
        // kilitlendi. Kip ikisinde de soruluyor ve `Select` iki kolda iki
        // ayrı şey demek — basışta jest terminalin, bırakmada gidecek yer
        // yok.
        let shift = pressed && modifiers.shift;
        let encoding = match input::button_route(*term.mode(), shift) {
            ButtonRoute::Select if pressed => return Click::Select,
            ButtonRoute::Select => return Click::Ignored,
            ButtonRoute::Report(encoding) => encoding,
        };
        let offset = term.grid().display_offset() as i32;
        let byte = input::button_byte(button, modifiers);
        let report = mouse_report_at(encoding, byte, at, offset, pressed);
        drop(term);
        self.send_report(report)
    }

    /// Raporu gönderir; boş rapor [`Click::Ignored`]. Kilit **bırakılmış**
    /// olmalı: `send` kanala yazıyor ve `Term`'ü tutarken beklemenin anlamı
    /// yok.
    fn send_report(&self, report: Option<Vec<u8>>) -> Click {
        let Some(report) = report else {
            return Click::Ignored;
        };
        self.send(Msg::Input(report.into()));
        Click::Sent
    }

    /// Fare hareketi: 1003'te her zaman, 1002'de yalnız bir düğme
    /// basılıyken, 1000'de hiç ([`crate::input::motion_route`]).
    ///
    /// `button` **basılı olanı** söylüyor, `None` düğmesiz hareket demek —
    /// kararın girdisi bu, çünkü 1002'nin "yalnız sürüklerken"i tam olarak o
    /// soru. Basılıyken hangi düğme olduğu rapora da giriyor.
    ///
    /// **Kısma burada değil, çağıranda.** Rapor hücre başına en çok bir kez
    /// gitmeli ve karşılaştırma `bt-shell`'de, bu çağrıdan **önce** koşuyor
    /// (`ViewIvars::motion_cell`): aynı hücrede kalan hareket `Term`
    /// kilidine hiç uğramıyor. Durum burada yaşasaydı `bt-core` fare
    /// konumunu tutmaya başlardı ve kilit hareket başına ödenirdi.
    ///
    /// Gerisi [`Session::mouse_button`]'ın basış kolunun aynısı: tek kilit,
    /// `send` (`send_input` değil), satır uygulamanın ekranında değilse ya da
    /// koordinat sığmıyorsa [`Click::Ignored`]. **Kırpma yok** — bırakmanın
    /// kırpması takılı düğmeyi önlemek içindi, düşen bir hareket raporu
    /// hiçbir şeyi asılı bırakmıyor.
    ///
    /// [`Click::Select`] bu yoldan **hiç dönmüyor**: hareket bir jest
    /// başlatmıyor, başlamış bir jestin devamı. Tip yine de ortak, çünkü
    /// çağıran iki fonksiyonu aynı kolda tüketiyor.
    pub fn mouse_motion(
        &self,
        button: Option<MouseButton>,
        at: SelectionPoint,
        modifiers: MouseModifiers,
    ) -> Click {
        let term = self.term.lock();
        let Some(encoding) = input::motion_route(*term.mode(), button.is_some()) else {
            return Click::Ignored;
        };
        let offset = term.grid().display_offset() as i32;
        let byte = input::motion_byte(button, modifiers);
        let report = mouse_report_at(encoding, byte, at, offset, true);
        drop(term);
        self.send_report(report)
    }

    /// Ok tuşu — klavyenin oku baytla değil tuşla girer; neden [`Arrow`]'da.
    ///
    /// [`Session::write`] gibi kullanıcı girdisidir: seçimi temizler ve
    /// pencereyi dibe döndürür; kip sorusu ile ikisi **aynı** `Term` kilidinde
    /// (`send_input`), yani ok tuşu da vuruş başına tek kilit öder.
    pub fn write_arrow(&self, arrow: Arrow) {
        self.send_input(|mode| input::arrow(arrow, mode).to_vec());
    }

    /// Görünen pencereyi `pages` sayfa kaydırır (Shift+PgUp/PgDn); artı değer
    /// geriye. `None` → alternate screen ve `bt-shell` o cevapta tuşu
    /// uygulamaya geçiriyor; `Some(n)` → pencere `n` satır kaydı, uçta `0`.
    /// Tekerleğin karar tablosundan **geçmez**: klavyedir, rapor ya da ok
    /// üretmez.
    ///
    /// Sayfa **görünen satır sayısı** ve o sayı `Term`'de: "bir sayfa kaç
    /// satır" terminalin kararı, `bt-shell`'in piksel aritmetiği değil. View
    /// sayfayı kendi ölçü önbelleğinden türetseydi ekran boyunun ikinci bir
    /// kopyasını taşır, ölçü yokken de tuşu sessizce uygulamaya düşürürdü.
    ///
    /// **Kesir sıfırlanıyor ve nesil artıyor** ([`Session::reset_scroll`]):
    /// sayfa tam satırlık bir adım ve uçuştaki bir süzülme onun varış yerini
    /// kaydırmamalı.
    pub fn scroll_page(&self, pages: i32) -> Option<i32> {
        let (moved, dropped) = {
            let mut term = self.term.lock();
            let dropped = self.reset_scroll();
            let lines = pages.saturating_mul(term.screen_lines() as i32);
            (
                self.scroll_user(&mut term, lines, self.band_shown()),
                dropped,
            )
        };
        if dropped {
            self.request_frame();
        } else {
            self.wake_if_moved(moved);
        }
        moved
    }

    /// Edit ▸ Clear to Start (⌘K): ekranı ve geçmişi siler, **o anki bloğu**
    /// bırakır — Terminal.app'in Clear to Start'ı (034 Karar 1, Seçenek A).
    ///
    /// Korunan ilk satırın üstü ızgaranın tepesinden dışarı kaydırılıyor,
    /// geçmiş siliniyor; yukarı kaydıracak hiçbir şey kalmıyor. Korunan
    /// bloğun ne olduğu [`protected_top`]'ta: imlecin satırının blok
    /// kimliğini taşıyan bitişik satırlar (sarılan ve çok satırlı giriş,
    /// `PREBUFFER`, çok satırlı `PS1`), kimlik yoksa (komut koşuyor,
    /// entegrasyonsuz kabuk) imlecin satırı.
    ///
    /// **Kabuğa ve koşan programa tek bayt gitmiyor**: temizlik terminalin
    /// defterinde oluyor, `send_input`'a hiç uğramıyor — yani komut koşarken
    /// de çalışıyor ve `cat`'in girdisine `^L` yazmıyor.
    ///
    /// Alternatif ekranda hiçbir şey yapmıyor ve `false` dönüyor (Karar 2):
    /// birincil ızgaranın geçmişi `Term::inactive_grid`'de ve alan özel.
    pub fn clear_to_start(&self) -> bool {
        self.clear(ClearKind::ToStart)
    }

    /// Edit ▸ Clear Scrollback (⌥⌘K): yalnız geçmişi siler; ızgara bayt bayt
    /// aynı kalıyor. [`Session::clear_to_start`]'ın gövdesi, dışarı kaydırılan
    /// satır sıfır — alternatif ekran kuralı da aynı.
    pub fn clear_scrollback(&self) -> bool {
        self.clear(ClearKind::Scrollback)
    }

    /// İki temizliğin tek gövdesi, **tek** `Term` kilidi turunda.
    ///
    /// Sıra zorunlu:
    ///
    /// - **Dibe dönüş kaydırmadan önce** — `send_input`'un ikilisinin aynısı
    ///   (`reset_scroll` + dibe `scroll_user`, gerekçesi [`Session::write_owned`]'ın
    ///   doc'unda): `Grid::scroll_up` kaydırılmış pencerenin ofsetini
    ///   büyütüyor, yani önce dönülmezse pencere geçmişe itilirdi.
    /// - **Kaydırma bölgeyi (DECSTBM) bilerek atlıyor**: `Grid::scroll_up`
    ///   ekranın tamamında. Temizlik bölgeye değil ekrana ait — bölgeyle
    ///   kaydırsaydık altta sabit satırı olan bir programda korunan blok
    ///   bölgenin dışında kalır ve hiçbir şey gitmezdi.
    /// - `Grid::scroll_up` imleci ve seçimi **taşımıyor** (alacritty'nin
    ///   `Term::scroll_up_relative`'i taşıyor ama bölgeye bağlı): imleç ve
    ///   DECSC'nin kaydı aynı miktarda elle düşüyor, seçim koşulsuz kalkıyor
    ///   — `ClearMode::Saved` yalnız geçmişe değen seçimi süzüyor ve kayan
    ///   satırlardaki seçim yanlış metni vurgulardı.
    /// - Geçmiş **kaydırmadan sonra** siliniyor: dışarı kaydırılan satırlar
    ///   önce geçmişe düşüyor.
    /// - `2J` nesli temizlik **uygulandıktan sonra** ([`Session::note_screen_clear`]):
    ///   kare yolu nesli gördüğünde ızgara zaten temiz.
    ///
    /// Kilitten sonra dock seçimi (yaprak kilit `Term`'ün dışında,
    /// `send_input` emsali), arama haberi ve kare.
    fn clear(&self, kind: ClearKind) -> bool {
        // Yaprak kilit `Term`'den **önce** (modül başlığı): `shell` tutulurken
        // `Term` alınmaz.
        let input_block = lock(&self.shell).input_block();
        {
            let mut term = self.term.lock();
            if term.mode().contains(TermMode::ALT_SCREEN) {
                return false;
            }
            self.reset_scroll();
            self.scroll_user(&mut term, i32::MIN, self.band_shown());
            if kind == ClearKind::ToStart {
                let top = protected_top(&term, input_block);
                if top > 0 {
                    let rows = term.screen_lines();
                    let grid = term.grid_mut();
                    grid.scroll_up(&(Line(0)..Line(rows as i32)), top);
                    let by = i32::try_from(top).unwrap_or(i32::MAX);
                    grid.cursor.point.line = Line((grid.cursor.point.line.0 - by).max(0));
                    grid.saved_cursor.point.line =
                        Line((grid.saved_cursor.point.line.0 - by).max(0));
                }
            }
            term.clear_screen(ClearMode::Saved);
            clear_selection_locked(&mut term);
            self.note_screen_clear();
            self.adapter.0.wipes.fetch_add(1, Ordering::AcqRel);
        }
        self.clear_dock_selection();
        self.adapter.search_changed();
        self.request_frame();
        true
    }

    /// `CSI 2 J` neslinin **ikinci yazarı** ([`Session::screen_clears`]):
    /// terminal tarafı temizlik (⌘K/⌥⌘K) bayrağın bütün tüketicilerine gerçek
    /// bir `2J` gibi görünsün diye — doldurma kapısı, kayma sayısı, damga ve
    /// [`Session::clear_boundary`].
    ///
    /// **`Term` kilidi tutulurken ve temizlik uygulandıktan sonra** çağrılır.
    /// Okuyucunun kuralının tersi yönde ("uygulamadan önce say") ama aynı
    /// sonuç: kare yolu sayacı `Term` kilidinin altında okuyor ve nesli
    /// gördüğü turda ızgara zaten temiz, yani bayrağı kuran kare temiz ekranı
    /// görüyor. Geçmiş boş olduğu için bayrak olmadan da doldurma bugün
    /// tesadüfen sıfır verirdi; nesil temizliği bayrağın **bugünkü ve
    /// gelecekteki** tüketicilerine tek kelimeyle söylüyor (`discussion.md`
    /// → Muhakeme, reddedilenler).
    fn note_screen_clear(&self) {
        self.screen_clears.fetch_add(1, Ordering::Relaxed);
    }

    /// Ekranda duran doldurma bandının boyu, kaydırmanın anladığı tipte.
    ///
    /// [`Session::fill_shown`]'ın tek okuyucusu; kaydırma yollarının hepsi
    /// (kare yolunun süzülme payı dahil) buradan geçiyor ki "bant nereden
    /// başlar" sorusunun tek cevabı olsun.
    fn band_shown(&self) -> i32 {
        i32::from(self.fill_shown.load(Ordering::Relaxed))
    }

    /// Kaydırma kolunun niyete göre gövdesi ([`ScrollIntent`]). **`Term`
    /// kilidi tutulurken** çağrılır: kesrin kilit rejimi o
    /// ([`Session::scroll_frac`]).
    ///
    /// Dönüş `(ofset farkı, kare gerekiyor mu)`; `None` alternatif ekran.
    fn scroll_by_intent<T: EventListener>(
        &self,
        term: &mut Term<T>,
        rows: f64,
        lines: i32,
        intent: ScrollIntent,
    ) -> Option<(i32, bool)> {
        let band = self.band_shown();
        let frac = f64::from_bits(self.scroll_frac.load(Ordering::Relaxed));
        match intent {
            ScrollIntent::Lines => {
                // Satır adımı da konumu dışarıdan sıfırlayan bir yol: kalmış
                // kesir düşüyor ve **nesil artıyor**, yoksa ayar `off`'a
                // çevrilirken uçuşta kalan bir süzülmenin payı güncel nesille
                // gelir ve satır adımının sildiği kesri geri getirirdi.
                let dropped = self.reset_scroll();
                let moved = self.scroll_user(term, lines, band)?;
                Some((moved, moved != 0 || dropped))
            }
            ScrollIntent::Direct => {
                let (moved, next) = self.scroll_fraction(term, frac, rows, band)?;
                Some((moved, moved != 0 || next != frac))
            }
            ScrollIntent::Glide => {
                // **Uçta istek birikmez**: dipte aşağı, tepede yukarı dönen
                // çentik süzülme boyunca kare üstüne kare isterdi ve her pay
                // kırpmaya çarpardı — hiçbir şey değiştirmeyen bir animasyon.
                if rows == 0.0 || !rows.is_finite() || !scroll_room(term, frac, band, rows > 0.0) {
                    return Some((0, false));
                }
                self.add_glide(rows);
                Some((0, true))
            }
            ScrollIntent::Settle => {
                let (moved, next) = self.scroll_fraction(term, frac, rows, band)?;
                // `round` yarıyı yukarı (geriye) atıyor ve yukarıdaki satır
                // kesir sıfırdan büyükken var olmak zorunda, yani iki yön de
                // ulaşılabilir bir satıra varıyor.
                let correction = next.round() - next;
                if correction != 0.0 {
                    self.add_glide(correction);
                }
                Some((moved, moved != 0 || next != frac || correction != 0.0))
            }
            ScrollIntent::GestureBegan => {
                self.bump_scroll_generation();
                let (moved, next) = self.scroll_fraction(term, frac, rows, band)?;
                Some((moved, moved != 0 || next != frac))
            }
        }
    }

    /// Kesirli deltanın **tek** yazıcısı: olay yolu da kare yolunun payı da
    /// buradan geçiyor ([`scroll_fraction_locked`] + kesrin kaydı). `Term`
    /// kilidi tutulurken; kare istemez, çağıran karar verir.
    ///
    /// Dönüş `(ofset farkı, yeni kesir)`.
    fn scroll_fraction<T: EventListener>(
        &self,
        term: &mut Term<T>,
        frac: f64,
        rows: f64,
        band: i32,
    ) -> Option<(i32, f64)> {
        let (moved, next) = scroll_fraction_locked(term, frac, rows, band)?;
        self.note_user_scroll(moved);
        self.scroll_frac.store(next.to_bits(), Ordering::Relaxed);
        Some((moved, next))
    }

    /// [`scroll_locked`]'ın oturumdan çağrısı: ofsetin kullanıcıdan gelen
    /// payını da biriktiriyor ([`Session::user_scroll`]). Kaydırmanın bütün
    /// yolları buradan ya da [`Session::scroll_fraction`]'dan geçiyor —
    /// yoksa doymuş defterde arama eşleşmesi kullanıcının kaydırması kadar
    /// yanlış satıra kayardı ([`search::ledger_shift`]).
    fn scroll_user<T: EventListener>(
        &self,
        term: &mut Term<T>,
        lines: i32,
        band: i32,
    ) -> Option<i32> {
        let moved = scroll_locked(term, lines, band)?;
        self.note_user_scroll(moved);
        Some(moved)
    }

    /// Kullanıcının kaydırdığı ofset farkını biriktirir. `Term` kilidi
    /// altında çağrılıyor, yani gözlem ([`Session::ledger_now`]) onu ofsetle
    /// aynı turda görüyor.
    fn note_user_scroll(&self, moved: i32) {
        if moved != 0 {
            self.user_scroll
                .fetch_add(i64::from(moved), Ordering::Relaxed);
        }
    }

    /// Defterin bu anki hâli ([`search::LedgerMark`]); `Term` kilidi
    /// tutulurken.
    fn ledger_now<T>(&self, term: &Term<T>) -> search::LedgerMark {
        search::LedgerMark {
            history: term.history_size(),
            offset: term.grid().display_offset(),
            user: self.user_scroll.load(Ordering::Relaxed),
            epoch: self.adapter.0.ledger.load(Ordering::Acquire),
            wipes: self.adapter.0.wipes.load(Ordering::Acquire),
            columns: term.columns(),
            lines: term.screen_lines(),
            alt: term.mode().contains(TermMode::ALT_SCREEN),
        }
    }

    /// Bekleyen süzülme isteğini alır ve sıfırlar — **kare yolunun** çağrısı,
    /// `Term` kilidinin dışında.
    ///
    /// Dönen nesil isteğin ait olduğu nesil; çizen taraf payı
    /// [`Session::frame`]'e aynı nesille geri veriyor ve arada konum dışarıdan
    /// sıfırlandıysa pay düşüyor ([`ScrollGlide`]).
    pub fn take_scroll_glide(&self) -> ScrollGlide {
        self.update_glide(|glide| ScrollGlide { rows: 0.0, ..glide })
    }

    /// Süzülme isteğine `rows` ekler; nesil yerinde kalır.
    ///
    /// Toplam `f64`'te yapılıp `i32` aralığına kırpılıyor, sonra `f32`'ye
    /// iniyor: sonlu ama dev bir delta `f32`'de sonsuza taşar ve ters yöndeki
    /// ilk ekleme isteği NaN'a çevirip süzülmeyi sessizce öldürürdü. Aralık
    /// kaydırmanın kendi alanı (`scroll_locked`'ın `i32`'si), yani kırpma
    /// hiçbir ulaşılabilir hedefi değiştirmiyor.
    fn add_glide(&self, rows: f64) {
        self.update_glide(|glide| {
            let sum =
                (f64::from(glide.rows) + rows).clamp(f64::from(i32::MIN), f64::from(i32::MAX));
            ScrollGlide {
                rows: sum as f32,
                ..glide
            }
        });
    }

    /// Bekleyen isteği düşürür ve nesli artırır: konum dışarıdan
    /// sıfırlandı, uçuştaki süzülme de bitsin.
    fn bump_scroll_generation(&self) {
        self.update_glide(|glide| ScrollGlide {
            rows: 0.0,
            generation: glide.generation.wrapping_add(1),
        });
    }

    /// [`Session::scroll_glide`]'ın tek atomik adımı; **önceki** değeri
    /// döndürür. Üç yazıcısının (alım, ekleme, nesil) ortak gövdesi, ki
    /// paketleme tek yerde kalsın.
    fn update_glide(&self, change: impl Fn(ScrollGlide) -> ScrollGlide) -> ScrollGlide {
        let word = self
            .scroll_glide
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |word| {
                Some(change(ScrollGlide::unpack(word)).pack())
            })
            // Kapanış her zaman `Some` veriyor, yani `Err` kolu yok; `Err`'in
            // taşıdığı da aynı kelime.
            .unwrap_or_else(|word| word);
        ScrollGlide::unpack(word)
    }

    /// Kaydırma konumunu dışarıdan tam satıra sıfırlar: kesir sıfır, istek
    /// düşer, nesil artar. `Term` kilidi tutulurken; kesir sıfırdan büyüktü
    /// ise `true` — ekran değişti ve kare gerekiyor.
    ///
    /// Nesil **her** çağrıda artıyor, kesir sıfır olsa da: uçuştaki bir
    /// süzülme payını henüz teslim etmemiş olabilir (kesir sıfır, istek
    /// çoktan alınmış) ve o hâlde de bitmek zorunda — yazmaya başlayan
    /// kullanıcının penceresi dipten geri çekilmemeli.
    fn reset_scroll(&self) -> bool {
        self.bump_scroll_generation();
        self.scroll_frac.swap(0, Ordering::Relaxed) != 0
    }

    /// Pencere gerçekten kaydıysa kare ister. Kaymayan kaydırma (geçmişin
    /// ucu, `Some(0)`) ve alternate screen (`None`) istemez — trackpad
    /// momentumu uçta da olay yağdırır ve her biri boş bir kare olurdu. `Term`
    /// kilidi bırakıldıktan sonra çağrılır ([`Session::request_frame`]).
    fn wake_if_moved(&self, moved: Option<i32>) {
        if moved.is_some_and(|n| n != 0) {
            self.request_frame();
        }
    }

    /// Kirli bayrağını diker ve uyandırır — `Adapter`'ın `Wakeup` kolunun
    /// kullandığı yolun **kendisi** (`Adapter::wake_frame`), ikinci bir kopyası
    /// değil: "bayrak uyandırmadan önce" sırası tek yerde yaşıyor. `Wakeup`
    /// olayından geçmiyor, çünkü o kol çıktının neslini artırıyor ve arama
    /// dizinine "defter değişti" diyor (033) — seçim ve kaydırma defteri
    /// değiştirmiyor. `Term` kilidi **bırakıldıktan sonra**
    /// çağrılır: uyandırma çift muteksli `FairMutex` tutulurken koşmamalı.
    ///
    /// **İstisnası ekranı temizlemek** ([`Session::clear_to_start`]): defteri
    /// gerçekten değiştiriyor, ama haberini yine bu yoldan değil adıyla
    /// veriyor — temizliğin nesli ve arama haberi, sonra bu kare isteği.
    ///
    /// `resize` bunu **kullanmıyor**: onun uyandırması `bt-shell`'in işi
    /// (link'i kendisi açıyor). Buradaki çağıranların (seçim ve kaydırma)
    /// ise elinde link yok.
    fn request_frame(&self) {
        self.adapter.wake_frame();
    }

    /// Seçili aralığın metni — kopyalamanın (phase-2) ve sınamaların **tek**
    /// yolu. Satır sarma ve geniş karakter spacer'ları alacritty'nin içinde
    /// çözülür; ikinci bir metin yolu, ikinci bir sarma hatası demek olurdu.
    ///
    /// Okumadır, seçimi **temizlemez**: Cmd-C girdi değil, `send_input`'a hiç
    /// varmaz — kopyaladıktan sonra vurgu ekranda kalır (alacritty de öyle).
    ///
    /// **Sahibin metni** (031 Karar 7): pencerede tek seçim var, dock'ta bir
    /// aralık seçiliyse onun `BUFFER` dilimi, değilse ızgaranınki. İkisi aynı
    /// anda dolu olamıyor — birinde başlamak ötekini temizliyor.
    pub fn selection_text(&self) -> Option<String> {
        if let Some(text) = self.dock_selection_text() {
            return Some(text);
        }
        self.term.lock().selection_to_string()
    }

    /// Seçim var mı — metni kurmadan ([`Session::selection_text`]'in ucuz
    /// sorusu): menünün doğrulaması (⌘E) her açılışta geçmişin tamamını
    /// dizgiye çevirmesin. Dock'un seçimi `BUFFER`'ın dilimi, yani kısa.
    pub fn has_selection(&self) -> bool {
        self.dock_selection_text()
            .is_some_and(|text| !text.is_empty())
            || self
                .term
                .lock()
                .selection
                .as_ref()
                .is_some_and(|selection| !selection.is_empty())
    }

    /// Dock'ta yeni seçim: basış, tıklama sayısının adımıyla (031 R3.1).
    ///
    /// `point` dock'un giriş bloğunda bir hücre ve yarısı: `col` **ekran**
    /// sütunu (işaret 0'da, metin [`crate::DOCK_TEXT_COL`]'da), `row` giriş
    /// bloğunun **çizilen** satırı (dikey pencerenin içinde, 032) — ızgaranın
    /// [`SelectionPoint`]'i, dock-yerel. Nokta **son çizilen** pencereye
    /// karşı çözülüyor ([`Session::dock_window`]); ayna o kareden beri
    /// `BUFFER`'ı değiştirdiyse ya da `Live` değilse seçim yok.
    ///
    /// **Tek sahip**: ızgaranın seçimi her durumda kalkıyor — dock'a yapılan
    /// tık "başka yere tıklamak". Sürüklemesiz tek tık boş seçimdir ve
    /// eski vurguyu kaldırır (ızgaranın kuralı).
    pub fn dock_select(&self, kind: SelectKind, point: SelectionPoint) {
        let grid = clear_selection_locked(&mut self.term.lock());
        let dock = self.change_dock_selection(point, |_, point, buffer, _, cluster| {
            point.map(|point| DockSelection::new(kind, point, point, buffer, cluster))
        });
        if grid || dock {
            self.request_frame();
        }
    }

    /// Shift+tıklama dock'ta: var olan seçimin ucunu taşır, adımı korur;
    /// seçim yoksa **caret'ten** tıklanan noktaya bir `Simple` başlar — bir
    /// metin alanının Shift+tıklaması (031 phase-5; phase-4'te tıklanan
    /// noktadan boş başlıyordu, çünkü caret'i taşıyan yol henüz yoktu).
    /// Izgaranın seçimi kalkar (tek sahip).
    pub fn dock_extend(&self, point: SelectionPoint) {
        let grid = clear_selection_locked(&mut self.term.lock());
        let dock = self.change_dock_selection(point, |current, point, buffer, caret, cluster| {
            let point = point?;
            Some(match current {
                Some(selection) => selection.extended(point, buffer),
                None => {
                    let caret = DockPoint {
                        index: caret,
                        half: CellHalf::Left,
                    };
                    DockSelection::new(SelectKind::Simple, caret, point, buffer, cluster)
                }
            })
        });
        if grid || dock {
            self.request_frame();
        }
    }

    /// Dock'ta basılı sürüklemenin ucu: yalnız bitiş taşınır. Seçim yoksa
    /// **sessiz** — `BUFFER` sürüklemenin ortasında değiştiyse seçim kalktı
    /// ve fare hareketi yeni bir seçim doğurmamalı
    /// ([`Session::update_selection`]'ın kuralı). Nokta çözülemezse (ayna
    /// değişti) uç yerinde kalır.
    pub fn dock_drag(&self, point: SelectionPoint) {
        let changed = self.change_dock_selection(point, |current, point, buffer, _, _| {
            let current = current?;
            Some(point.map_or(current, |point| current.extended(point, buffer)))
        });
        if changed {
            self.request_frame();
        }
    }

    /// Dock seçiminin üç kurucusunun ortak gövdesi: noktayı çizilen pencereye
    /// karşı çözer, `change`'in cevabını yazar ve **çizilen aralık**
    /// değiştiyse `true` döner — ızgaranın `store_selection` kapısı: yarım
    /// hücre titreyen bir sürükleme boş kare istemesin.
    ///
    /// Kilitler sırayla, iç içe değil: önce iz, sonra `shell`.
    fn change_dock_selection(
        &self,
        at: SelectionPoint,
        change: impl FnOnce(
            Option<DockSelection>,
            Option<DockPoint>,
            &str,
            usize,
            bool,
        ) -> Option<DockSelection>,
    ) -> bool {
        let window = *lock(&self.dock_window);
        let mut log = lock(&self.shell);
        let point = window.and_then(|window| window.hit(&log.dock, at));
        let before = log.dock_selection.and_then(|selection| selection.range());
        // Seçimin uzayı `PREBUFFER ++ BUFFER` ([`dock::selectable`]); caret
        // o uzayda `PREBUFFER` kadar ileride.
        let caret = dock::prebuffer_chars(&log.dock) + buffer_caret(&log.dock);
        let text = dock::selectable(&log.dock);
        let after = change(log.dock_selection, point, &text, caret, log.dock.cluster);
        drop(text);
        log.dock_selection = after;
        before != after.and_then(|selection| selection.range())
    }

    /// Tekerlek (ya da kenara değen sürükleme) dock'un giriş bloğunun
    /// üstünde: dikey pencereyi `lines` satır kaydırır — artı **geriye**
    /// (yukarı), [`Session::scroll_wheel`] ile aynı yön (032 phase-4).
    ///
    /// `true` → olay dock'un: giriş tavanı aşıyor ve pencere kayabiliyor (uçta
    /// kıpırdamasa da — ızgaraya düşmesin, kullanıcı dock'u kaydırıyor).
    /// `false` → dock taşmıyor ya da hiç çizilmedi; çağıran ızgarayı kaydırır.
    ///
    /// Tepe **son çizilen** pencereden ([`Session::dock_window`]) hesaplanıyor
    /// ve ize de hemen yazılıyor: sürüklemenin ucu aynı olayda yeni pencereye
    /// karşı çözülsün. Çizim tepeyi [`crate::shell::ShellLog::dock_scroll`]'dan
    /// okuyor; caret'in yeri değişince (yazmak, ok) o kalkıyor ve pencere yine
    /// caret'i izliyor.
    pub fn dock_scroll(&self, lines: i32) -> bool {
        let Some(window) = *lock(&self.dock_window) else {
            return false;
        };
        let shown = usize::from(window.shown);
        if window.rows <= shown {
            return false;
        }
        let last = window.rows - shown;
        let top = (window.top as i64 - i64::from(lines)).clamp(0, last as i64) as usize;
        if top != window.top {
            lock(&self.shell).dock_scroll = Some(top);
            if let Some(drawn) = lock(&self.dock_window).as_mut() {
                drawn.top = top;
            }
            self.request_frame();
        }
        true
    }

    /// ⌘A'nın dock kolu: `Live` ve boş olmayan bir satırda seçilebilir
    /// metnin tamamı — `PREBUFFER ++ BUFFER`, yani ekrandaki bütün komut
    /// (032: `for` döngüsünü kopyalamak beklenen şey; `PREBUFFER`'a değen
    /// seçimde düzenleme tuşları bugünkü yoldan gidiyor). Seçecek bir şey
    /// yoksa `false` ve çağıran ızgaraya düşer.
    ///
    /// Adım `Simple` ve iki uç metnin iki ucu: `Line` 032'den beri
    /// **mantıksal satırı** seçiyor ve çok satırlı `BUFFER`'da yalnız ilk
    /// satırı alırdı.
    fn dock_select_all(&self) -> bool {
        let changed = {
            let mut log = lock(&self.shell);
            let text = dock::selectable(&log.dock);
            if log.dock.status != DockStatus::Live || text.is_empty() {
                return false;
            }
            let point = |index| DockPoint {
                index,
                half: CellHalf::Left,
            };
            let before = log.dock_selection.and_then(|selection| selection.range());
            let all = DockSelection::new(
                SelectKind::Simple,
                point(0),
                point(text.chars().count()),
                &text,
                log.dock.cluster,
            );
            drop(text);
            log.dock_selection = Some(all);
            before != all.range()
        };
        let grid = clear_selection_locked(&mut self.term.lock());
        if changed || grid {
            self.request_frame();
        }
        true
    }

    /// Dock seçiminin metni: seçilebilir metnin (`PREBUFFER ++ BUFFER`)
    /// seçili dilimi, sona satır sonu **eklenmeden** — ızgaranın satır seçimi
    /// (`Lines`) `\n` taşıyor ama dock'un satırını kabuğa geri yapıştırmak
    /// onu **çalıştırırdı**. Metnin **içindeki** satır sonları (çok satırlı
    /// komut) olduğu gibi kalıyor.
    fn dock_selection_text(&self) -> Option<String> {
        let log = lock(&self.shell);
        let (start, end) = log.dock_selection?.range()?;
        Some(
            log.dock
                .prebuffer
                .chars()
                .chain(log.dock.buffer.chars())
                .skip(start)
                .take(end - start)
                .collect(),
        )
    }

    /// Dock seçimini kaldırır; çizili bir aralık kalktıysa `true` (kareyi
    /// çağıran istiyor — genellikle başka bir sebeple zaten istiyor).
    fn clear_dock_selection(&self) -> bool {
        lock(&self.shell)
            .dock_selection
            .take()
            .is_some_and(|selection| selection.range().is_some())
    }

    /// **Düzenleme kapısı** (031 R4.4): dock'un satırını değiştiren bir komut
    /// şu an ZLE'ye gidebilir mi. Dört koşul, dördü de var olan yüklemler:
    ///
    /// - dock satırın sahibi ([`ShellLog::suppressed_input`]: safha `Input`,
    ///   ayna `Live`, blok açık) — `Running`, `Unavailable` ve
    ///   `Control` burada kapanıyor;
    /// - ZLE ekleme keymap'inde ([`DockState::insert_keymap`]) — `vicmd`'de
    ///   dizi bağlı değil ve baytları **komut** olurdu (`~` harfin büyüklüğünü
    ///   çeviriyor; ölçüldü);
    /// - ayna kullanıcının son girdisinin cevabı (`answers == key_gen`, 025)
    ///   — gönderilen indeksler ZLE'nin gördüğü `BUFFER`'a ait olmalı;
    /// - kabuk bu prompt'ta widget'ı bağladığını söyledi
    ///   ([`ShellLog::dock_editable`]) — bağlamasız kabukta sondaki BEL
    ///   `send-break` olur ve satır ölür (ölçüldü, 031 → Muhakeme).
    ///
    /// Kapı kapalıyken **hiçbir komut gitmiyor**: seçim yine kopyalanabilir,
    /// tuşlar bugünkü yolundan gidip seçimi kaldırır.
    pub fn can_edit_dock(&self) -> bool {
        self.dock_edit_line().is_some()
    }

    /// Yeniden bağlanma teklifinin ⏎'si (037 Karar 8); `true` → satır gitti.
    ///
    /// Kapı: teklif var, dock caret'in sahibi (son karenin cevabı, ⌘A'nın
    /// okuduğu), ayna kullanıcının son girdisinin cevabı, satır (`BUFFER`,
    /// `PREBUFFER` ve öneri) boş, `line-finish` tutulmuyor ve ZLE **ekleme
    /// keymap'inde** — giden şey yazılmış gibi satır + `\r` ve `vicmd`'de o
    /// baytlar komut olurdu (`s` satırı değiştirip `sh prod`'u koştururdu;
    /// [`Session::can_be_typed`]'ın aynı kemeri). Yer tutucu da aynı kapının
    /// arkasında çiziliyor (`dock::render_reconnect`). Düzenleme widget'ına
    /// (`8133;w`) **bağlı değil**: komut değil metin gidiyor ve geçmişe
    /// giriyor. Teklif yoksa ilk soruda `false` — Enter'ın yolu bayt bayt
    /// bugünkü.
    ///
    /// Gönderim `send_input`'tan ve yaprak kilit **bırakıldıktan sonra**
    /// (`send_input` onu yeniden alıyor); teklifi de o siliyor.
    fn reconnect(&self) -> bool {
        let generation = self.key_gen.load(Ordering::Acquire);
        let line = {
            let log = lock(&self.shell);
            let Some(offer) = &log.context.reconnect else {
                return false;
            };
            let typable = log
                .suppressed_input()
                .is_some_and(|input| input.answers == generation && input.insert_keymap);
            if !(typable
                && !log.holding_end()
                && log.dock.buffer.is_empty()
                && log.dock.prebuffer.is_empty()
                && log.dock.postdisplay.is_empty()
                && self.caret_in_dock.load(Ordering::Relaxed))
            {
                return false;
            }
            let mut line = offer.line.clone().into_bytes();
            line.push(b'\r');
            line
        };
        self.write_owned(line);
        true
    }

    /// Kapı açıksa satırın o anki hâli; kapalıysa `None`. Tek kilit turu.
    ///
    /// Satırın iki kaynağı var: son girdiye cevap veren ayna ya da, ayna
    /// yoldayken, son düzenleme komutunun beklenen sonucu
    /// ([`DockPrediction`]; aynı nesil). Tahminde seçim yok — her gönderim
    /// seçimi kaldırıyor ve arada fareyle kurulan seçim bayat aynanın
    /// metnine karşı kuruldu: kapı kapanıyor.
    fn dock_edit_line(&self) -> Option<DockEditLine> {
        let generation = self.key_gen.load(Ordering::Acquire);
        let log = lock(&self.shell);
        let input = log.suppressed_input()?;
        if !(log.dock_editable && input.insert_keymap) {
            return None;
        }
        let fresh = input.answers == generation;
        let (buffer, caret, selection) = if fresh {
            (
                log.dock.buffer.as_str(),
                buffer_caret(&log.dock),
                log.dock_selection,
            )
        } else {
            let pending = log
                .dock_pending
                .as_ref()
                .filter(|p| p.generation == generation && log.dock_selection.is_none())?;
            (pending.buffer.as_str(), pending.caret, None)
        };
        // Birden çok kod noktalı komşu küme komutla gidiyor (phase-4). Tek kod
        // noktalı komşu yalnız **emoji kümesi** (iki sütunlu) taşıyan satırda:
        // zincir orada kırılmamalı ([`DockEditLine::before`]). Ölçüt
        // birleştirici değil emoji, çünkü NFD bir yol (`Masaüstü`, macOS'un
        // dosya adları) satırı kümeli yapıp düz tuşları ZLE'nin
        // bağlamalarından (autopair gibi eklentiler) koparırdı. Kalan
        // satırda yazım efektleri ve ZLE'nin kendi silmesi aynen.
        let emoji = log.dock.cluster && {
            let mut found = false;
            crate::cluster::Walk::new().run(buffer.chars(), |c| {
                found |= c.end - c.start > 1 && c.width > 1;
            });
            found
        };
        let span = |index: Option<usize>| {
            let index = index.filter(|_| log.dock.cluster)?;
            dock::cluster_span(buffer.chars(), index, true)
                .filter(|(start, end)| emoji || end - start > 1)
        };
        Some(DockEditLine {
            buffer: buffer.to_owned(),
            len: buffer.chars().count(),
            caret,
            shift: dock::prebuffer_chars(&log.dock),
            selection,
            fresh,
            before: span(caret.checked_sub(1)),
            after: span(Some(caret)),
        })
    }

    /// Düzenleme komutunu kullanıcı girdisinin tek hunisinden gönderir: nesil
    /// ilerler (widget'ın aynası ona cevap olur), iki seçim de kalkar.
    ///
    /// Komutun beklenen sonucu gönderimin nesliyle damgalanıyor
    /// ([`DockPrediction`]): ayna gelene kadar kapı ona bakıyor. Damga
    /// gönderimden **sonra** ve yaprak kilidin ayrı turunda — `send_input`
    /// o kilidi kendisi alıyor; araya giren tek yazıcı okuyucu thread ve o
    /// yalnız aynayı tazeliyor, tahmin aynanın cevabına yenik düşüyor.
    fn send_dock_edit(&self, line: &DockEditLine, start: usize, end: usize) {
        let bytes = dock_edit_command(start, end, line.len);
        let Some(generation) = self.send_input(|_| bytes) else {
            return;
        };
        let buffer = line
            .buffer
            .chars()
            .enumerate()
            .filter(|&(index, _)| !(start..end).contains(&index))
            .map(|(_, ch)| ch)
            .collect();
        lock(&self.shell).dock_pending = Some(DockPrediction {
            generation,
            buffer,
            caret: start,
        });
    }

    /// Edit ▸ Cut etkin mi: dock'ta boş olmayan bir seçim var **ve** kapı
    /// açık. Izgarada kesilecek bir şey yok (031 Karar 7).
    pub fn can_cut(&self) -> bool {
        self.dock_edit_line()
            .is_some_and(|line| line.buffer_range().is_some())
    }

    /// Dock seçimini siler (`d;S;E;L`) — ⌫/⌦'nin, ve yazmadan ya da
    /// yapıştırmadan **önce** çağrıldığında "seçimin yerine yaz"ın ilk yarısı:
    /// metin ardından olağan yoldan gidiyor ([`Session::write`],
    /// [`Session::paste`] — sarma kararı hâlâ `paste`'in), yani tele hiç
    /// girmiyor. Seçim yoksa ya da kapı kapalıysa hiçbir şey göndermez ve
    /// `false` döner; çağıran o zaman bugünkü yoldan devam eder.
    pub fn dock_delete_selection(&self) -> bool {
        let Some(line) = self.dock_edit_line() else {
            return false;
        };
        let Some((start, end)) = line.buffer_range() else {
            return false;
        };
        self.send_dock_edit(&line, start, end);
        true
    }

    /// Edit ▸ Cut (⌘X): seçili metin ve silme komutu. Kapı kapalıysa ya da
    /// seçim yoksa `None` ve pano el değmeden kalır.
    pub fn dock_cut(&self) -> Option<String> {
        let text = self.dock_selection_text()?;
        self.dock_delete_selection().then_some(text)
    }

    /// Dock'taki jestin bırakılışı: sürüklemesiz tek tıklama caret'i
    /// tıklanan sınıra taşır (031 R4.1, `d;N;N;L`). Öneriye ya da satırın
    /// sağındaki boşluğa düşen tık `BUFFER`'ın sonuna iniyor (isabet testi).
    ///
    /// Caret zaten oradaysa **gönderilmiyor**: boşa bir girdi nesli ilerletir
    /// ve kapı, cevabı gelene kadar bir sonraki düzenlemeyi beklerdi.
    pub fn dock_click(&self) {
        let Some(line) = self.dock_edit_line() else {
            return;
        };
        // `PREBUFFER`'a düşen tık caret'i taşımıyor: ZLE o satırları kabul
        // etti ve caret'in gidebileceği yer yalnız `BUFFER` (032 Karar 2).
        let Some(index) = line
            .selection
            .and_then(|s| s.click())
            .and_then(|index| index.checked_sub(line.shift))
        else {
            return;
        };
        if index != line.caret {
            self.send_dock_edit(&line, index, index);
        }
    }

    /// Dock seçimi varken tuşun terminaldeki karşılığı (031 Karar 8);
    /// `true` → tuş tüketildi, çağıran onu kabuğa **göndermemeli**.
    ///
    /// Kapı kapalıysa hep `false`: tuş bugünkü yolundan gider, seçim kalkar.
    /// Seçim yoksa ⇧←/⇧→ tüketiliyor (caret'ten seçim başlatır); ⌫, ⌦, ←, →
    /// yalnız caret'in bitişiğinde birden çok kod noktalı bir küme varsa
    /// (035 Karar 7), yoksa bugünkü yolunda. ⇧←/⇧→ da kapıya bağlı, kabuğa hiçbir şey
    /// göndermese bile: seçim caret'ten başlıyor ve caret'in yeri ancak taze
    /// bir aynada doğru — `vicmd`'de ise tuş vi'nin.
    pub fn dock_key(&self, key: DockKey) -> bool {
        // ⏎ düzenleme kapısından **önce** ve ona bağlı değil (Karar 8): giden
        // şey yazılmış baytlar, widget komutu değil.
        if key == DockKey::Enter {
            return self.reconnect();
        }
        let Some(line) = self.dock_edit_line() else {
            return false;
        };
        // `PREBUFFER`'a değen seçim düzenleme tuşlarında seçim yokmuş gibi:
        // tuş bugünkü yolundan gidiyor ve seçimi kaldırıyor (032 Karar 2).
        let range = line.buffer_range();
        match (key, range) {
            (DockKey::Backspace | DockKey::Delete, Some((start, end))) => {
                self.send_dock_edit(&line, start, end);
            }
            (DockKey::Left, Some((start, _))) => self.send_dock_edit(&line, start, start),
            (DockKey::Right, Some((_, end))) => self.send_dock_edit(&line, end, end),
            // Tahmin satırında ⇧←/⇧→ bugünkü yolundan (phase-5 öncesi kapalı
            // kapının cevabı): seçim bayat aynanın metnine kurulamaz.
            (DockKey::ShiftLeft | DockKey::ShiftRight, _) if !line.fresh => return false,
            (DockKey::ShiftLeft | DockKey::ShiftRight, _) => {
                let forward = key == DockKey::ShiftRight;
                let changed = {
                    let mut log = lock(&self.shell);
                    // Kapıdan bu yana ayna değiştiyse (`BUFFER` başka) seçim
                    // o aynanın metnine karşı yeniden kurulmamalı: tuş düşer.
                    if log.dock.buffer.chars().count() != line.len
                        || dock::prebuffer_chars(&log.dock) != line.shift
                    {
                        return true;
                    }
                    let before = log.dock_selection.and_then(|s| s.range());
                    // Seçimin uzayında (`PREBUFFER ++ BUFFER`): ⇧← `PREBUFFER`'a
                    // da uzanabiliyor — seçmek serbest, düzenlemek değil.
                    let text = dock::selectable(&log.dock);
                    let after = DockSelection::stepped(
                        log.dock_selection,
                        line.shift + line.caret,
                        forward,
                        &text,
                        log.dock.cluster,
                    );
                    drop(text);
                    log.dock_selection = Some(after);
                    before != after.range()
                };
                // Tek sahip: klavyeyle başlayan dock seçimi ızgaranınkini
                // kaldırıyor, fareyle başlayanın kuralı.
                let grid = clear_selection_locked(&mut self.term.lock());
                if changed || grid {
                    self.request_frame();
                }
            }
            // **Satır sonu yapıştırmanın yolundan** (`paste`): bracketed sarma
            // her keymap'te harfi harfine ekliyor, yani `viins`'te de satırı
            // çalıştırmıyor — `\e\r` orada `vicmd`'ye geçip satırı kabul
            // ederdi, `^V^J` ise `vicmd`'de çıplak `^J` olurdu. Seçimin yerine
            // geçmek ve aynayı tazeleyen komut da oradan bedava geliyor. Sarma
            // kapalıysa çıplak `\n` satırı çalıştırırdı: tuş bugünkü yolundan
            // (Enter) gidiyor.
            // Yukarıda cevaplandı.
            (DockKey::Enter, _) => return false,
            (DockKey::NewLine, _) => {
                if !self.bracketed_paste() {
                    return false;
                }
                self.paste(b"\n".to_vec());
            }
            // **Seçimsiz dört tuş kümeyi bütün yürütüyor** (035 Karar 7): ZLE
            // kod noktası kod noktası yürüyor ve `🇹🇷`'de ⌫ yalnız `🇷`'yi
            // silerdi. Satırda birden çok kod noktalı bir küme varsa tuş
            // widget'ın tek komutu oluyor (⌫/⌦ `[S,E)`, ←/→ `S == E`) ve
            // beklenen sonucu basılı tuşun tekrarına kapıyı açık tutuyor
            // ([`DockPrediction`]); kümesiz satırda ve satırın ucunda bugünkü
            // yol — `self-insert`, yazım efektleri, ZLE'nin silmesi ve
            // sondaki →'nun öneriyi kabul etmesi aynen.
            (DockKey::Backspace | DockKey::Delete | DockKey::Left | DockKey::Right, None) => {
                let span = match key {
                    DockKey::Backspace | DockKey::Left => line.before,
                    _ => line.after,
                };
                let Some((start, end)) = span else {
                    return false;
                };
                match key {
                    DockKey::Left => self.send_dock_edit(&line, start, start),
                    DockKey::Right => self.send_dock_edit(&line, end, end),
                    _ => self.send_dock_edit(&line, start, end),
                }
            }
        }
        true
    }

    /// Oturumun o anki teması, kopya olarak — `bt-gpu`'nun clear ve imleç
    /// rengi buradan okunur, `frame()`'in zemin atlaması ve renk sorusunun
    /// yanıtıyla **aynı** kaynaktan.
    ///
    /// Yaprak kilidi alır ve bırakır; `Term` kilidine dokunmaz, `frame()`'in
    /// `sink`'inden de çağrılabilir.
    pub fn theme(&self) -> Theme {
        *lock(&self.adapter.0.theme)
    }

    /// Kabuğun OSC 133 ile bildirdiği durum, kopya olarak — hiç işaret
    /// gelmediyse `None`.
    ///
    /// `None`'ın anlamı **"entegrasyon yok"**: kabuk bizim sarmalayıcımızla
    /// açılmadı, kullanıcı ayarla kapattı ya da oturum SSH ile başka bir
    /// makineye geçti. Üçü de arıza değil sessiz geri düşüş, bu yüzden üçü tek
    /// cevap veriyor.
    ///
    /// [`Session::theme`] ile aynı şekil: yaprak kilidi alır ve bırakır,
    /// `Term` kilidine dokunmaz, `frame()`'in `sink`'inden de çağrılabilir.
    /// Aynı yaprak kilidi `frame()` de alıyor (010, faz 2) ama `Term` kilidini
    /// bıraktıktan **sonra**; ikisi hiçbir yerde iç içe girmiyor.
    pub fn shell_state(&self) -> Option<ShellState> {
        lock(&self.shell).state
    }

    /// Kabuğun son OSC 7 dizini; hiç gelmediyse `None`.
    ///
    /// [`Session::shell_state`] ile aynı şekil: yaprak kilidi alır, kopyalar,
    /// bırakır; `Term` kilidine dokunmaz. Tüketicisi yeni sekmenin başlangıç
    /// dizini ve başlık.
    pub fn working_directory(&self) -> Option<PathBuf> {
        let log = lock(&self.shell);
        (!log.context.cwd.is_empty()).then(|| PathBuf::from(&log.context.cwd))
    }

    /// Pencerenin başlığı: uygulamanın OSC 0/2 başlığı → dizinin son
    /// bileşeni (ev `~`) → `bateri`; uzak oturumda `⇄ {OSC başlığı}`, yoksa
    /// `⇄ {host}` (kural `shell::title_of`).
    ///
    /// İki yaprak kilidi **sırayla** alır, iç içe değil; `Term` kilidine
    /// dokunmaz. Dizin ile uzak host **aynı** kilit turunda: ayrı turlarda
    /// araya düşen bir `D` yerel dizini uzak bir host'la eşleştirebilirdi.
    /// Kare yolu bunu çağırmıyor: değişimi [`Wake::title_changed`] (ya da
    /// [`Session::set_remote`]'in dönüşü) haber veriyor ve okuyan o haberin
    /// alıcısı.
    pub fn title(&self) -> String {
        let osc = lock(&self.adapter.0.title).clone();
        let (cwd, remote) = {
            let log = lock(&self.shell);
            (
                log.context.cwd.clone(),
                log.context.remote_host().map(str::to_owned),
            )
        };
        crate::shell::title_of(
            osc.as_deref(),
            Some(&cwd),
            self.home.as_deref(),
            remote.as_deref(),
        )
    }

    /// Koşan komutun nesli; komut koşmuyorsa `None` (036 Karar 2;
    /// tanımı `ShellLog::running_command`).
    ///
    /// Uzak oturum yoklamasının ilk yarısı: çağıran nesli yoklamadan
    /// **önce** alır ve cevabı onunla [`Session::set_remote`]'e geri verir.
    /// Yaprak kilit; `Term`'e dokunmaz.
    pub fn running_command(&self) -> Option<u64> {
        lock(&self.shell).running_command()
    }

    /// Uzak oturumun hedefini bildirir (`None` = yerel; 037 Karar 1: host,
    /// tür, argv ve satır bütün olarak); başlığın girdisi (host)
    /// **değiştiyse** `true` ve çağıran başlığı tazeler (036 Karar 1, 5).
    /// Host'un işareti burada, desen listesinden çözülüyor
    /// ([`Session::set_host_marks`]).
    ///
    /// **Bayat cevap kapısı:** `command` [`Session::running_command`]'ın
    /// verdiği nesil; tutmuyorsa ya da komut koşmuyorsa çağrı no-op —
    /// yoklama ile `D` arasında biten komutun cevabı sonraki prompt'a
    /// sızmamalı. Uzak durumu silmek için çağrı yok: `C`, `D` ve `A` onu
    /// kendiliğinden siliyor.
    ///
    /// Değiştiyse kare ister ([`Session::set_theme`] örüntüsü: bağlam satırı
    /// ve dock'un üst çizgisi değişti, alacritty'nin hasarı bunu bilmiyor).
    /// Yaprak kilit `request_frame`'den önce düşüyor; `Term`'e dokunulmuyor.
    pub fn set_remote(&self, command: u64, target: Option<&RemoteTarget>) -> bool {
        let (changed, repaint) = {
            let mut log = lock(&self.shell);
            if log.running_command() == Some(command) {
                let mark = log.context.remote_mark;
                let changed = log.set_remote(target);
                (changed, changed || mark != log.context.remote_mark)
            } else {
                (false, false)
            }
        };
        if repaint {
            self.request_frame();
        }
        changed
    }

    /// `[remote] hosts`'un desen listesini yazar ve etkin uzak host'un
    /// işaretini yeniden çözer (037 Karar 2) — ayar dosyasının açılışı ve
    /// canlı yenilemesi. **İşaret değiştiyse** `true` ve kare ister
    /// ([`Session::set_theme`] emsali: dock'un iki rengi değişti, alacritty'nin
    /// hasarı bunu bilmiyor); aynı liste ya da işareti oynatmayan liste
    /// no-op. Dönüş görünen bir şeyin değiştiğini söylüyor, listenin değil.
    ///
    /// Yaprak kilit, `request_frame`'den önce düşüyor; `Term`'e dokunmuyor.
    /// Kare yolu desen görmüyor, yalnız çözülmüş işareti okuyor.
    pub fn set_host_marks(&self, rules: &[HostRule]) -> bool {
        let changed = lock(&self.shell).set_host_rules(rules);
        if changed {
            self.request_frame();
        }
        changed
    }

    /// Uzak oturumun host'u (gösterildiği gibi, `user@` dahil) ve çözülmüş
    /// işareti; yerelde `None` (037 Karar 4, 5). Okuyanlar sekmenin noktası
    /// ve Shell ▸ Mark … as ▸ — ikisi de ana thread'de, kenarda; kare yolu
    /// bunu çağırmıyor. Yaprak kilit, `Term`'e dokunmuyor.
    pub fn remote_mark(&self) -> Option<(String, HostMark)> {
        let log = lock(&self.shell);
        log.context
            .remote_host()
            .map(|host| (host.to_owned(), log.context.remote_mark))
    }

    /// Uzak hedefin kabuk için kaçırılmış satırı (`ssh -p 2222 prod`); yerelde
    /// `None` (037 Karar 6). Okuyanı ⌘T: yeni sekmenin ilk girdisi
    /// ([`SessionOptions::initial_input`]). Ana thread'de, kenarda; yaprak
    /// kilit, `Term`'e dokunmuyor.
    pub fn remote_line(&self) -> Option<String> {
        lock(&self.shell)
            .context
            .remote
            .as_ref()
            .map(|target| target.line.clone())
    }

    /// Uzak oturumun **tek okumada** hedefi, uzak dizini ve komutunun nesli;
    /// yerelde ya da komut koşmuyorken `None` (037 Karar 7).
    ///
    /// Okuyanı Finder damlasının yüklemesi: nesil damlada alınıyor ve
    /// onaydan sonra ve kuyruğun ömrü boyunca yeniden soruluyor — tutmuyorsa
    /// ssh bitmiş, bekleyenler iptal (037 Karar 7). Üçü aynı yaprak
    /// kilit turunda, yani bir `D` hedefi eskisiyle, nesli yenisiyle
    /// eşleştiremiyor. Ana thread'de, kenarda; `Term`'e dokunmuyor.
    pub fn remote_target(&self) -> Option<(u64, RemoteTarget, String)> {
        let log = lock(&self.shell);
        let command = log.running_command()?;
        let target = log.context.remote.clone()?;
        Some((command, target, log.context.remote_cwd.clone()))
    }

    /// Yükleme kuyruğunun durum satırını yazar (`None` = kaldır; 037 Karar
    /// 7). **Değiştiyse** kare ister ([`Session::set_remote`] örüntüsü: satır
    /// alacritty'nin hasarında yok) ve `true` döner.
    ///
    /// Durma koşulu çağıranın: kuyruk bitince sonuç satırı bir süre kalıp
    /// `None`'la kalkıyor ve bu çağrı aynı değeri ikinci kez aldığında kare
    /// istemiyor — boşta sıfır kare. Yaprak kilit `request_frame`'den önce
    /// düşüyor.
    pub fn set_transfer(&self, transfer: Option<&Transfer>) -> bool {
        let changed = {
            let mut log = lock(&self.shell);
            let slot = &mut log.context.transfer;
            if slot.as_ref() == transfer {
                false
            } else {
                match (slot.as_mut(), transfer) {
                    (Some(current), Some(fresh)) => current.clone_from(fresh),
                    _ => *slot = transfer.cloned(),
                }
                true
            }
        };
        if changed {
            self.request_frame();
        }
        changed
    }

    /// Uygulama alternatif ekranda mı — **son karedeki** hâl.
    ///
    /// Kendi sorgusu, [`Cursor`]'ın alanı **değil**: `Cursor` bir kare kaydı
    /// (imlecin yeri, doluluk), bu ise oturumun o andaki gerçeği ve tüketicisi
    /// çizim değil **pencere geometrisi** — dock alternatif ekranda kalkıyor
    /// (012 → R5.2). `Cursor`'a eklenseydi kare başına doğan bir kayda oturum
    /// ömrü olan bir bilgi binerdi.
    ///
    /// Kilit **almıyor**: değeri [`Session::frame`] `Term` kilidi altındayken
    /// yayınlıyor ([`Session::alt_screen`] alanının doc'u). Yani cevap
    /// çizilen son kareyle tutarlı ve sorgunun bedeli bir atomik okuma.
    ///
    /// **Çağıran resize'ı burada yapmaz.** Değer kare yolundan okunuyor ve
    /// pencere geometrisini kare çizilirken değiştirmek (drawable boyutu,
    /// ızgara, `DisplayLink` yerleşimi) tam da o karenin altını oymak olurdu;
    /// çağrı bir sonraki ana kuyruk turuna bırakılır (`bt-shell`).
    pub fn alt_screen(&self) -> bool {
        self.alt_screen.load(Ordering::Relaxed)
    }

    /// ZLE'nin görüntü aynası, `into`'ya **yerinde** kopyalanır.
    ///
    /// [`Self::shell_state`] ile aynı kilit örüntüsü — yaprak kilidi alır ve
    /// bırakır, `Term` kilidine dokunmaz — ama dönüş şekli farklı ve sebebi
    /// tek: [`DockState`] `Copy` değil, üç dizgi ile bir liste taşıyor.
    /// Kopyayı döndürseydi kare başına dört ayırma doğardı; `into` kendi
    /// kapasitesini koruyor ve sabit durumda ayırma **sıfır** (R1.3).
    pub fn dock_state(&self, into: &mut DockState) {
        into.clone_from(&lock(&self.shell).dock);
    }

    /// Dock'un bu karedeki hücrelerini sink'e basar ve yüzeyini verir.
    ///
    /// [`Session::frame`]'in dock karşılığı ve aynı sözleşme: sınırdan
    /// **çözülmüş** hücreler geçiyor (renk, biçim, sütun), kabuğun safhası ve
    /// `region_highlight`'ın sözdizimi geçmiyor. Karar burada, boyama orada.
    ///
    /// **`Term` kilidine hiç dokunmuyor**: dock ızgarayı okumuyor, aynayı
    /// okuyor. Yaprak kilit bir kez alınıp bırakılıyor ve ikisi de (ayna +
    /// safha) aynı okumadan çıkıyor — ayrı çağrılardan alınsalardı araya düşen
    /// bir işaret `>`'i bir kareliğine metinle çelişen bir renge boyardı.
    /// Tema kopyası kilitten **önce**, `frame()`'deki örüntünün aynısı.
    ///
    /// `into` çağıranın tamponu ([`Session::dock_state`] ile aynı gerekçe:
    /// kare başına ayırma yok). `cols` ızgaranın genişliği; dock aynı
    /// sütunları kullanıyor ve taşan satırı pencerelemek için gerekiyor
    /// ([`crate::dock::render`]). İki satırın bütçesi tek tipte
    /// ([`DockCols`]) ve ayrı sayılar: bağlam satırı küçük puntoda çizildiği
    /// için aynı genişliğe daha çok harf sığıyor; sayıyı çizen taraf veriyor,
    /// bu crate piksel görmüyor.
    ///
    /// **Dock'u olmayan pencere bunu hiç çağırmıyor**: ayrım oturum doğarken
    /// (`bt-shell`, entegrasyon kuruldu mu) kararlaşıyor ve bu crate onu
    /// bilmiyor — bilseydi "kabuk entegrasyonu kuruldu mu" sorusunun ikinci
    /// bir kaydı doğardı.
    ///
    /// **İkinci sink yazım animasyonlarının** (030): son çizilen aynadan bu
    /// yana eklenen ya da silinen glyph'i, ekran sütunuyla, **en çok bir**
    /// [`DockEdit`] olarak basıyor. Fark çağıranın tamponuna karşı alınıyor,
    /// yani `into` **son çizilen** ayna olmak zorunda — tek çağıranı
    /// `bt-gpu`'nun içerik karesi ve tamponu ondan başka kimse yazmıyor.
    /// Farkın bedeli damga kapısının arkasında (`dock::change`):
    /// yeni girdi yoksa tek bir karşılaştırma. Tavanı aşan girişte dikey
    /// pencerenin kayması düzenlemeyle birlikte **satır** farkı olarak geçiyor
    /// ([`DockEdit`]'in doc'u); metin değişmeden kayan pencere tek başına
    /// `Shift`.
    ///
    /// **`input_rows` çizilecek giriş satırı sayısı** ve `frame()`'in cevabı
    /// ([`Cursor::input_rows`]): burada yeniden türetilmiyor, `caret_in_dock`
    /// gibi hesaplandığı yerden geçiyor. Bağlam satırı onun **altında**, yani
    /// satır numarası bu sayının ta kendisi; iki ayrı kilit turundan
    /// türetilseydi bant ile dock'un satırları bir kare ayrışabilirdi.
    ///
    /// **`runs` seçimin görsel satır başına koşuları** (032): uzun satır
    /// sarılıyor ve seçim birden çok satıra yayılabiliyor. Çağıranın tamponu
    /// ([`SelectionRuns`] emsali), her çağrıda boşalıp doluyor; satırları
    /// dikey pencerenin içinde, dock-yerel.
    // Argümanlar karenin tek okumasından geçen ayrı sayılar ve tamponlar
    // (`frame()`'in cevabı, çağıranın tamponları, iki sink); bir yapıda
    // toplamak yalnız bu çağrı için bir tip doğururdu.
    #[allow(clippy::too_many_arguments)]
    pub fn dock(
        &self,
        cols: DockCols,
        input_rows: u16,
        into: &mut DockState,
        context: &mut DockContext,
        caret_in_dock: bool,
        runs: &mut Vec<SelectionRun>,
        clusters: &mut Clusters,
        sink: impl FnMut(Cell),
        edits: impl FnMut(DockEdit),
    ) -> Dock {
        let theme = *lock(&self.adapter.0.theme);
        let (shell, change, selection, scroll) = {
            let shell = lock(&self.shell);
            // Fark **kopyadan önce**: `into` şu an son çizilen ayna ve bir
            // satır sonra yenisiyle eziliyor.
            let change = dock::change(into, &shell.dock);
            into.clone_from(&shell.dock);
            // Bağlam **aynı kilit turunda**: ayrı bir turda alınsaydı araya
            // düşen bir prompt dizini yeni, dalı eski bir satırla eşleştirirdi.
            context.clone_from(&shell.context);
            // Seçim de: `BUFFER` değişince onu silen yazıcı aynı kilidi
            // tutuyor, yani aralık bu aynanın metnine ait.
            let range = shell.dock_selection.and_then(|selection| selection.range());
            (shell.state, change, range, shell.dock_scroll)
        };
        let mut edit = None;
        let (dock, top, rows) = dock::render_with(
            into,
            context,
            shell,
            &theme,
            cols,
            input_rows,
            scroll,
            caret_in_dock,
            selection,
            change.as_ref(),
            runs,
            clusters,
            sink,
            |made| edit = Some(made),
        );
        // İsabet testinin izi: bu karenin penceresi ([`Session::dock_window`]).
        // Dikey pencerenin kayması **son çizilen** tepeden: uçuştaki efektler
        // metinle birlikte kaysın (032 phase-6). İlk karede kayma yok.
        let painted = lock(&self.dock_window).map(|drawn| drawn.painted);
        let by = painted.map_or(0, |painted| painted as i64 - top as i64);
        // audit: kırpma yalnız tip; iki tepe de girişin satır sayısıyla
        // sınırlı.
        let by = by.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
        dock::with_shift(edit, by).into_iter().for_each(edits);
        *lock(&self.dock_window) = Some(DockWindow {
            top,
            painted: top,
            // Sıfır giriş satırında (uzak oturum) sıfır: isabet testi her
            // noktayı reddediyor ([`DockWindow::hit`]), tekerlek dock'u
            // kaydırmıyor — tıklanacak bir giriş satırı yok (036 R5.3).
            shown: input_rows,
            rows,
            cols: cols.grid,
            buffer_bytes: into.prebuffer.len() + into.buffer.len(),
        });
        dock
    }

    /// Geçmişte aramanın sorgusunu kurar ve derlenmiş hâlinin durumunu
    /// döndürür (033); vurguyu bir sonraki içerik karesi çiziyor.
    ///
    /// Derleme yaprak kilidin **dışında**: uzun bir desenin DFA kurulumu
    /// kare yolunun yuvayı beklediği süreye girmesin. Nesil her çağrıda
    /// artıyor, yani kare yolunun ödünç aldığı eski desen geri konmuyor
    /// ([`SearchSlot`]). Kare yalnız ekranda bir şey değişebiliyorsa
    /// isteniyor: önceki ya da yeni sorgudan biri bir desen taşımalı — boş
    /// sorgudan boş sorguya geçiş boşta sıfır kareyi bozmaz.
    ///
    /// **Geçerli eşleşmeyi de seçiyor** (Karar 3): pencerede çizilen bir
    /// eşleşme varsa en alttaki, yoksa aramanın başladığı pencerenin dibinden
    /// ([`SearchSlot::origin`]) yukarı ilk eşleşme. Pencereyi oynatmıyor —
    /// açığa çıkarmak panelin geometrisini bilen çağıranın ikinci adımı
    /// ([`Session::search_reveal`]).
    pub fn set_search(&self, query: &SearchQuery) -> SearchStatus {
        let (status, mut pattern) = search::compile(query);
        let (origin, hidden) = {
            let slot = lock(&self.search);
            (slot.origin, slot.hidden.clone())
        };
        let (current, origin, mark) = match pattern.as_mut() {
            Some(regex) => {
                let term = self.term.lock();
                let origin = origin
                    .filter(|point| in_grid(&term, *point))
                    .unwrap_or_else(|| window_bottom(&term));
                let band = self.search_band(&term);
                let current = nearest_match(&term, regex, origin, hidden.as_ref(), band);
                (current, Some(origin), Some(self.ledger_now(&term)))
            }
            // Desensiz arama defteri izlemiyor ([`search::track`]): başlangıç
            // bayatlamasın diye düşüyor, sonraki sorgu pencerenin dibinden.
            None => (None, None, None),
        };
        self.store_search(pattern, current, origin, mark);
        status
    }

    /// Aramayı kapatır: desen, geçerli eşleşme, başlangıç ve dizin düşer; bir
    /// sonraki içerik karesi vurgusuz.
    pub fn clear_search(&self) {
        self.store_search(None, None, None, None);
    }

    /// Geçerli eşleşmeden `direction` yönündeki bir sonrakine geçer (⏎/⌘G,
    /// ⇧⏎/⇧⌘G; Karar 3) ve onu açığa çıkarır ([`Session::search_reveal`]'ın
    /// kuralı). Uçta sarar; geçerli eşleşme yoksa [`Session::set_search`]'ün
    /// seçimi.
    ///
    /// **Hedef vurgunun kümesinden** ([`search::eligible`]): bastırılan
    /// satıra değen ya da mürekkepsiz eşleşme atlanıyor — ⏎ pencereyi
    /// görünmeyen bir yere götürmemeli. Aynı küme dizinin kümesi, yani sıra
    /// numarası komşuya ±1 taşınıyor (uçta sararak).
    ///
    /// Desen yuvadan **ödünç** ([`SearchSlot`]), `Term` kilidinden önce;
    /// arada yeni bir sorgu geldiyse sonuç yazılmıyor (nesil kuralı).
    pub fn search_next(
        &self,
        direction: SearchDirection,
        cover: SearchCover,
        smooth: bool,
    ) -> SearchReport {
        self.with_search(|term, regex, state| {
            let band = self.search_band(term);
            let current = state
                .current
                .take()
                .filter(|found| match_in_grid(term, found));
            let (found, step) = match current {
                Some(current) => {
                    let way = match direction {
                        SearchDirection::Older => Direction::Left,
                        SearchDirection::Newer => Direction::Right,
                    };
                    let from = search::step_past(term, &current, way);
                    let found = search::next_eligible(term, regex, from, way, state.hidden);
                    // Sarma: yukarı giderken bulunan aşağıda (ya da tersi).
                    let step = found.as_ref().map(|found| {
                        let wrapped = match direction {
                            SearchDirection::Older => found.start() >= current.start(),
                            SearchDirection::Newer => found.start() <= current.start(),
                        };
                        OrdinalStep::Moved { direction, wrapped }
                    });
                    (found, step.unwrap_or(OrdinalStep::Fresh))
                }
                None => {
                    let origin = state
                        .origin
                        .filter(|point| in_grid(term, *point))
                        .unwrap_or_else(|| window_bottom(term));
                    let found = nearest_match(term, regex, origin, state.hidden, band);
                    (found, OrdinalStep::Fresh)
                }
            };
            // Sorguyu daraltmak bulunan yerin yakınında kalsın: başlangıç
            // geçerli eşleşmenin ucuna çekiliyor.
            if let Some(found) = &found {
                state.origin = Some(*found.end());
            }
            state.current = found;
            (self.reveal_current(term, state, cover, smooth), step)
        })
        .unwrap_or_default()
    }

    /// Geçerli eşleşme görünür değilse pencereyi ona götürür (Karar 4):
    /// panelin altında ([`SearchCover`]) ya da pencerenin dışındaysa satırı
    /// ortaya gelecek kadar, bir ekran içinde süzülerek (027'nin `Glide`'ı),
    /// uzaktaysa hedefin bir ekran yakınına anında konup kalan ekranı
    /// süzülerek; `smooth == false` (`smooth_scroll = "off"`, Hareketi
    /// Azalt, `snap`) anında. Görünür eşleşmede pencere **oynamaz**.
    ///
    /// Dönen rapor dizinin o anki hâli ([`SearchReport`]).
    pub fn search_reveal(&self, cover: SearchCover, smooth: bool) -> SearchReport {
        self.with_search(|term, _, state| {
            if state
                .current
                .as_ref()
                .is_some_and(|found| !match_in_grid(term, found))
            {
                state.current = None;
            }
            (
                self.reveal_current(term, state, cover, smooth),
                OrdinalStep::Kept,
            )
        })
        .unwrap_or_default()
    }

    /// Dizinin bir parçası (phase-5, Karar 2-B): bütün defterin sayımını
    /// dipten yukarı [`search::CHUNK_LINES`] satır ilerletir ve panelin
    /// raporunu verir. Arama kapalıysa (desen yok) `None`.
    ///
    /// Sürücüsü `bt-shell` — ana kuyrukta parça başına bir tur, `complete`
    /// gelene kadar; tuş olayları parçaların arasına giriyor.
    ///
    /// **Yakınsama kuralı:** defter haberi ([`Wake::search_changed`])
    /// uçuştaki geçişi **kesmez**. Geçiş bitince bekleyen haber varsa
    /// tüketilip tam olarak bir yeni geçiş başlıyor; haber kenarda kurulduğu
    /// için bir çıktı patlaması başına en çok bir ek geçiş. Rapor ancak geçiş
    /// bittiğinde **ve** bekleyen haber yokken `complete` — sürücünün durma
    /// koşulu. Kesen bir kural `yes` akarken geçişi hiç bitirmez ve ana
    /// kuyruk `Term`'i sonsuza dek her turda kilitlerdi.
    ///
    /// **Parça kare istemez** — sonucu yalnız AppKit'in etiketi. Tek istisna
    /// defterin kaymasıyla kaybedilen geçerli eşleşmenin geçişin sonunda
    /// yeniden seçilmesi ([`search::Relocate`]): vurgusu ekranda yer
    /// değiştiriyor.
    ///
    /// Desen dizinin **kendi** kopyası ([`search::SearchIndex::pattern`]):
    /// kare yolunun ödünç aldığı desenle yarışmıyor; `Term` altında hiçbir
    /// kilit alınmıyor.
    pub fn search_step(&self) -> Option<SearchReport> {
        let (generation, mut pattern, mut index, mut tracking, hidden) = {
            let mut slot = lock(&self.search);
            if !slot.active {
                return None;
            }
            if slot.index.pattern.is_none() {
                // Başka bir parça uçuşta (yalnız sınamalar koşturabiliyor):
                // bekleyen haber ona kalıyor.
                return Some(self.report_of(&slot));
            }
            if slot.index.next.is_none() {
                if !self.adapter.0.search_pending.swap(false, Ordering::AcqRel) {
                    return Some(self.report_of(&slot));
                }
                slot.index.restart(None);
            }
            let Some(pattern) = slot.index.pattern.take() else {
                return Some(self.report_of(&slot));
            };
            (
                slot.generation,
                pattern,
                std::mem::take(&mut slot.index),
                slot.tracking(),
                slot.hidden.clone(),
            )
        };
        let taken = tracking.mark;
        let term = self.term.lock();
        search::track(
            &term,
            &mut tracking,
            self.ledger_now(&term),
            self.scrollback.load(Ordering::Relaxed),
        );
        let offset = term.grid().display_offset() as i32;
        let window = drawn_lines(&term, offset, self.search_band(&term));
        search::index_chunk(
            &term,
            &mut pattern,
            &mut index,
            hidden.as_ref(),
            &tracking,
            window,
            search::CHUNK_LINES,
        );
        drop(term);
        let (report, relocated) = {
            let mut slot = lock(&self.search);
            if slot.generation != generation {
                // Araya yeni bir sorgu (ya da kapanış) girdi: parça düşüyor.
                return Some(self.report_of(&slot));
            }
            index.pattern = Some(pattern);
            let mut relocated = false;
            // Kaybedilen geçerli eşleşme ancak **son** geçişin sonunda
            // yeniden seçiliyor: bekleyen bir haber varsa dizinin
            // koordinatları çoktan kaymış olabilir.
            if index.next.is_none()
                && tracking.relocate.is_some()
                && !self.adapter.0.search_pending.load(Ordering::Acquire)
            {
                tracking.relocate = None;
                if let Some((found, ordinal, _)) = index.candidate.take() {
                    tracking.origin = Some(*found.end());
                    tracking.current = Some(found);
                    index.ordinal = Some(ordinal);
                    relocated = true;
                }
            }
            slot.index = index;
            let settled = slot.settle(taken, tracking);
            (self.report_of(&slot), relocated && settled)
        };
        if relocated {
            self.request_frame();
        }
        Some(report)
    }

    /// Panelin raporu yuvanın hâlinden ([`SearchReport`]).
    fn report_of(&self, slot: &SearchSlot) -> SearchReport {
        let pending = self.adapter.0.search_pending.load(Ordering::Acquire);
        SearchReport {
            // Geçerli eşleşme yoksa da dizinin saydığı eşleşme "var" demek:
            // sonradan gelen çıktı vurgulanıyor ve sayılıyor, etiket "No
            // matches" dememeli.
            found: slot.current.is_some() || slot.relocate.is_some() || slot.index.total > 0,
            total: slot.index.total,
            ordinal: slot.current.as_ref().and(slot.index.ordinal),
            complete: slot.active
                && slot.index.pattern.is_some()
                && slot.index.next.is_none()
                && !pending,
        }
    }

    /// Geçerli eşleşmeyi ızgaranın seçimi yapar (Esc, kapatma düğmesi; Karar
    /// 5): ⌘C onu hemen kopyalar. Pencere oynamıyor. Eşleşme yoksa `false`.
    ///
    /// Uçlar defterin mutlak koordinatında, yani eşleşme doldurma bandında
    /// da olsa seçim kuruluyor — bant seçim çizmiyor (017'nin borcu, Karar
    /// 5'in bilinen sınırı) ama ⌘C kopyalıyor.
    pub fn select_search_match(&self) -> bool {
        let tracking = lock(&self.search).tracking();
        let term = self.term.lock();
        // Son gözlemden beri çıktı geldiyse eşleşme içeriğine yapışsın.
        let mut tracking = tracking;
        search::track(
            &term,
            &mut tracking,
            self.ledger_now(&term),
            self.scrollback.load(Ordering::Relaxed),
        );
        let Some(found) = tracking.current else {
            return false;
        };
        if !match_in_grid(&term, &found) {
            return false;
        }
        let mut selection = Selection::new(SelectionType::Simple, *found.start(), Side::Left);
        selection.update(*found.end(), Side::Right);
        self.store_selection(term, selection);
        true
    }

    /// Aramanın `Term` kilidi altındaki işlerinin ortak kalıbı: deseni ve
    /// durumu yaprak yuvadan **önce** alır, `Term` kilidi altında geçerli
    /// eşleşmeyi içeriğine yapıştırıp ([`search::track`]) `work`'ü koşturur,
    /// kilit düştükten sonra nesil aynıysa hepsini geri koyar, sıra
    /// numarasını `work`'ün adımıyla taşır ve pencere oynadıysa ya da geçerli
    /// eşleşme değiştiyse kare ister. Desen yoksa (arama kapalı, sorgu boş ya
    /// da geçersiz) `None`.
    fn with_search(
        &self,
        work: impl FnOnce(
            &mut Term<Adapter>,
            &mut search_engine::RegexSearch,
            &mut SearchState<'_>,
        ) -> (bool, OrdinalStep),
    ) -> Option<SearchReport> {
        let (generation, mut pattern, mut tracking, hidden) = {
            let mut slot = lock(&self.search);
            (
                slot.generation,
                slot.pattern.take(),
                slot.tracking(),
                slot.hidden.clone(),
            )
        };
        let taken = tracking.mark;
        let regex = pattern.as_mut()?;
        let mut term = self.term.lock();
        search::track(
            &term,
            &mut tracking,
            self.ledger_now(&term),
            self.scrollback.load(Ordering::Relaxed),
        );
        let mut state = SearchState {
            current: tracking.current.take(),
            origin: tracking.origin,
            hidden: hidden.as_ref(),
        };
        let (moved, step) = work(&mut term, regex, &mut state);
        // Kaydırma ofseti oynattı: iz o hâle çekiliyor ki bir sonraki gözlem
        // kullanıcının kaydırmasını çıktı sanmasın.
        tracking.mark = Some(self.ledger_now(&term));
        drop(term);
        let SearchState {
            current, origin, ..
        } = state;
        if current.is_some() {
            tracking.relocate = None;
        }
        tracking.current = current;
        tracking.origin = origin;
        let (changed, report) = {
            let mut slot = lock(&self.search);
            if slot.generation == generation && slot.pattern.is_none() {
                slot.pattern = pattern;
                let index = &mut slot.index;
                let restart = step.apply(index);
                if restart {
                    // Sırası bilinmeyen yeni bir geçerli eşleşme ve bitmiş
                    // bir dizin: sayım bir geçiş daha koşup sırayı buluyor.
                    // Rapordan **önce**: `complete` yanlış dönmeli ki
                    // çağıran sürücüyü kursun.
                    self.adapter.0.search_pending.store(true, Ordering::Release);
                }
                let changed = slot.settle(taken, tracking);
                (changed, self.report_of(&slot))
            } else {
                (false, self.report_of(&slot))
            }
        };
        if moved || changed {
            self.request_frame();
        }
        Some(report)
    }

    /// Geçerli eşleşmeyi açığa çıkarır — [`Session::search_next`] ile
    /// [`Session::search_reveal`]'ın ortak kuyruğu, `Term` kilidi altında.
    /// Pencere oynadıysa `true`.
    fn reveal_current(
        &self,
        term: &mut Term<Adapter>,
        state: &SearchState<'_>,
        cover: SearchCover,
        smooth: bool,
    ) -> bool {
        let band = self.search_band(term);
        state
            .current
            .as_ref()
            .is_some_and(|found| self.reveal_locked(term, found, cover, band, smooth))
    }

    /// Eşleşme görünür değilse pencereyi ona götürür; oynadıysa `true`.
    /// `Term` kilidi tutulurken — kaydırmanın bütün yolları gibi
    /// ([`scroll_locked`], [`Session::add_glide`]).
    fn reveal_locked(
        &self,
        term: &mut Term<Adapter>,
        found: &search_engine::Match,
        cover: SearchCover,
        band: i32,
        smooth: bool,
    ) -> bool {
        // Alternatif ekranın defteri yok: gezinme kaydırmıyor (Karar 8).
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        if match_visible(term, found, cover, band) {
            // **Pencere yerinde kalıyor — uçuştaki süzülme dahil.** Önceki
            // gezinmenin süzülmesi hâlâ yoldaysa görünür eşleşmeyi taşıyıp
            // götürürdü (⏎'den hemen sonra ⇧⏎; `/code-review`): nesil artıyor
            // ve süzülme bulunduğu yerde bitiyor. Kesir düştüyse kare gerekir.
            return self.reset_scroll();
        }
        let offset = term.grid().display_offset() as i32;
        let delta = reveal_target(term, found, band) - visual_top(offset, band);
        if delta == 0 {
            return false;
        }
        // Konumu dışarıdan sıfırlayan her yol gibi (027): uçuştaki süzülme
        // düşer, kesir sıfırlanır, nesil artar.
        self.reset_scroll();
        let rows = term.screen_lines() as i32;
        if !smooth {
            self.scroll_user(term, delta, band);
        } else if delta.abs() <= rows {
            self.add_glide(f64::from(delta));
        } else {
            // **Uzakta konup son ekranı süz** (`scroll_in`'in patlama
            // emsali): her gezinme aynı hareketle okunuyor ve uzun bir
            // süzülme binlerce satırı göz önünden geçirirdi.
            let screen = delta.signum() * rows;
            self.scroll_user(term, delta - screen, band);
            self.add_glide(f64::from(screen));
        }
        true
    }

    /// Doldurma bandının arama için boyu: dibe yaslı birincil ekranda
    /// ekrandaki bant ([`Session::band_shown`]), alternatif ekranda sıfır.
    fn search_band<T>(&self, term: &Term<T>) -> i32 {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            0
        } else {
            self.band_shown()
        }
    }

    /// [`Session::set_search`] ile [`Session::clear_search`]'ün ortak yazımı:
    /// desen, geçerli eşleşme, başlangıç ve defterin izi; dizin yeni desenin
    /// kopyasıyla baştan (desen yoksa boş), bekleyen defter haberi düşüyor —
    /// baştan başlayan geçiş onu zaten kapsıyor.
    fn store_search(
        &self,
        pattern: Option<search_engine::RegexSearch>,
        current: Option<search_engine::Match>,
        origin: Option<Point>,
        mark: Option<search::LedgerMark>,
    ) {
        let active = pattern.is_some();
        let was_active = {
            let mut slot = lock(&self.search);
            slot.generation = slot.generation.wrapping_add(1);
            slot.index = search::SearchIndex::default();
            if let Some(pattern) = &pattern {
                slot.index.restart(Some(pattern.clone()));
            }
            slot.pattern = pattern;
            slot.current = current;
            slot.origin = origin;
            slot.mark = mark;
            slot.relocate = None;
            self.adapter
                .0
                .search_pending
                .store(false, Ordering::Release);
            self.adapter
                .0
                .search_active
                .store(active, Ordering::Release);
            std::mem::replace(&mut slot.active, active)
        };
        // Kare isteği yaprak kilit **bırakıldıktan sonra** (`set_theme`
        // emsali): `request_frame` uyandırıcıya gidiyor ve yuvayı tutarken
        // dışarı uzanmamalı.
        if active || was_active {
            self.request_frame();
        }
    }

    /// Temayı takas eder ve kare ister — zemin, hücre renkleri, clear ve
    /// imleç sıradaki karede yeni temadan.
    ///
    /// **Aynı tema no-op:** ne yazılır ne kare istenir. Çağıranı (sistem
    /// görünümü) yalnız açık/koyu değişiminde değil vurgu rengi ya da
    /// kontrast ayarında da uyanıyor; koşulsuz kare boşta sıfır kareyi
    /// bozardı.
    ///
    /// Kare **istenmek zorunda**: alacritty'nin hasarı okunmuyor (kapı
    /// `dirty` bayrağı), yani takası çizime taşıyan başka bir sinyal yok.
    /// Yaprak kilit `request_frame`'den önce düşüyor; `Term` kilidine
    /// dokunulmuyor. Açık bir `frame()` kopyasını zaten aldıysa en çok bir
    /// kare eski renkle çizer ve bu çağrının kare isteği onu düzeltir.
    pub fn set_theme(&self, theme: Theme) {
        let changed = {
            let mut current = lock(&self.adapter.0.theme);
            let changed = *current != theme;
            *current = theme;
            changed
        };
        if changed {
            self.request_frame();
        }
    }

    /// Terminal seçeneklerini değiştirir ve kare ister — ayar dosyasının canlı
    /// yenilemesi.
    ///
    /// **Değişimi çağıran süzer** (`bt-shell`, `Settings::changes`): burada
    /// "aynı mı" sorusu yok ve güncel seçenekler saklanmıyor, tek sahipleri
    /// uygulamanın ayarları. Aynı değerle çağrı zararsız ama boşuna: `Term`
    /// kilidi, bütün ekranın hasarı ve bir kare.
    ///
    /// `Config` seçeneklerin tamamından kuruluyor (`term_config`); geçmiş
    /// küçülünce alacritty fazla satırları **hemen** siler ve kaydırma ofsetini
    /// yeni tavana kırpar — büyütmek silineni geri getirmez. Seçim o satırlarda
    /// kaldıysa `Selection::to_range` onu grid'e kırpıyor, çizim ve kopyalama
    /// panik görmez.
    ///
    /// Kare **istenmek zorunda**: ofset değişmiş olabilir ve alacritty'nin
    /// hasarı okunmuyor. İstek `Term` kilidi bırakıldıktan sonra
    /// ([`Session::request_frame`]).
    ///
    /// **Kilit altında giden olay:** `Term::set_options` başlık olayını
    /// (`Title`/`ResetTitle`) `Term` kilidi **tutulurken** `Adapter`'a
    /// yolluyor. Kol başlığı yaprak kilide yazıyor (`Term` → yaprak, emsali
    /// `ColorRequest`) ve değişmeyen başlık için haber doğurmuyor; `Term`'e
    /// geri uzanan bir yol açarsa bu çağrı kendi kendini kilitler
    /// (`race_set_terminal_options_and_frame` asılı kalır).
    pub fn set_terminal_options(&self, options: TerminalOptions) {
        let scrollback = options.scrollback;
        // Blink `Term`'ün config'inde temsil edilemiyor (`AdapterInner::blink`),
        // o yüzden yaprak kilide yazılıyor. **`Term` kilidinden önce**: ters
        // sıra okuyucu thread'in sırasıyla döngü kapatırdı.
        *lock(&self.adapter.0.blink) = options.blink;
        self.term.lock().set_options(term_config(options));
        // Blok defterinin tavanı da `scrollback`'ten türüyor ve o ayar **canlı
        // uygulanıyor**: burada taşınmasaydı büyütülen geçmişin fazlası
        // renksiz kalırdı (`/code-review`, 010 kapı). Kilit sırası zorunlu —
        // `Term` guard'ı bir üstteki ifadenin sonunda düştü, yaprak kilit
        // ondan **sonra** alınıyor; ters sıra okuyucu thread'in sırasıyla
        // (`Term` → `shell`) döngü kapatırdı.
        lock(&self.shell).set_scrollback(scrollback);
        self.scrollback.store(scrollback, Ordering::Relaxed);
        // Küçülen `scrollback` geçmişi kırpıyor: arama açıksa sayım baştan.
        self.adapter.search_changed();
        self.request_frame();
    }

    /// Hasarı uzaktan işaretleyebilen tutamak.
    ///
    /// Oturumun kendisine referans **vermez** ve bu kasıtlı: tutamağı tutan
    /// taraf (`bt-gpu`'nun `Waker`'ı, oradan da Metal'in tamamlanma bloğu)
    /// hiçbir koşulda `Arc<Session>` maddileştirmemeli. Maddileştirseydi son
    /// güçlü referans okuyucu ya da GPU thread'inde düşebilir, `Drop` →
    /// `shutdown()` → `join()` zinciri orada koşar ve thread kendi kendini
    /// beklerdi (`EDEADLK`) — `Wake` sözleşmesinin (`wake.rs`) yasakladığı
    /// tam olarak bu. `Weak<Session>` bile yetmez: `upgrade()` o referansı
    /// çağrı süresince maddileştirir.
    pub fn dirty_flag(&self) -> DirtyFlag {
        DirtyFlag(Arc::clone(&self.adapter.0.dirty))
    }

    /// Klavyeden PTY'ye bayt akıtır — kullanıcı girdisi.
    ///
    /// Boş dilim sessizce düşer: sıfır baytlık bir `Msg::Input`
    /// `EventLoop`'un yazıcısını kalıcı olarak kilitler (bkz. `Adapter::reply`).
    ///
    /// Boş olmayan girdi **seçimi temizler ve pencereyi dibe döndürür**, ikisi
    /// için `Term` kilidini bir kez alır: geçmişe bakarken yazılan satır
    /// görünmez kalmasın, eski vurgu değişen metnin üstünde kalmasın. Gerekçe
    /// ve bedeli `write_owned`'da — iki yol da oradan geçiyor.
    pub fn write(&self, bytes: &[u8]) {
        self.write_owned(bytes.to_vec());
    }

    /// Klavyenin **metni** — AppKit'in metin yığınının teslim ettiği harf
    /// (`insertText:`). [`Session::write`]'tan tek farkı dock seçimi: seçim
    /// varken ve düzenleme kapısı açıkken metin seçimin **yerine** yazılır
    /// (031 Karar 8) — önce silme komutu, sonra harf olağan yoldan, yani yine
    /// `self-insert`'ten geçiyor. Fonksiyon tuşları, Enter ve Control'lü
    /// baytlar `write`'ta kalıyor: onlar seçimi kaldırıp bugünkü işini yapar.
    pub fn type_text(&self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.dock_delete_selection();
        self.write(text.as_bytes());
    }

    /// Sahiplenen yazma: `write` ile aynı kapı, ama baytları bir kez daha
    /// kopyalamaz. Yapıştırma yükü pano mertebesinde olabilir (kopyalanmış
    /// bir log dosyası); sarma dalı tamponu zaten kuruyorken `write`'a
    /// dilimle gitmek ikinci bir tam kopya demekti.
    ///
    /// Boş vektör sessizce düşer; `write` de `paste` de bu kapıdan geçer ve
    /// ok tuşuyla ([`Session::write_arrow`]) birlikte hepsi `send_input`'ta
    /// buluşur — kullanıcı girdisinin **tek** gönderim noktası orası.
    /// `Msg::Input`'u kuran öteki iki yer kullanıcı girdisi değil, ve aşağıdaki
    /// dibe dönüş de seçimin temizliği de onlara uygulanmaz: `Adapter::reply`
    /// uygulamanın sorusuna yanıt (orada `Session` yok, adapter kendi kanalına
    /// yazıyor ve `Term` kilidini okuyucu thread tutuyor),
    /// [`Session::scroll_wheel`] tekerleğin raporunu ya da okunu uygulamaya
    /// veriyor (gerekçesi orada). Cmd-C ise hiç yazmıyor
    /// ([`Session::selection_text`]). Yolların hangisinin temizlediğini
    /// `input_clears_the_selection` ve `wheel_and_replies_keep_the_selection`
    /// çiviliyor.
    ///
    /// **Girdi seçimi temizler.** Kalsaydı yazılan satır ve yanıtı seçili
    /// hücrelerin üstüne düşebilirdi: vurgu hücrede kalır, altındaki metin
    /// değişir ve Cmd-C kullanıcının seçtiğini değil o an orada duranı
    /// kopyalardı. alacritty'nin kitaplığı yalnız silme dizilerinde
    /// (`intersects_range`) temizliyor, yazmada değil. Emsal alacritty'nin
    /// ikilisi: `on_terminal_input_start` hem tuş girdisinde hem yapıştırmanın
    /// iki dalında seçimi temizleyip dibe dönüyor. Kare yalnız **çizili** bir
    /// aralık kalkınca istenir (`clear_selection_locked`, phase-1'in kare
    /// kuralı): sürüklemesiz tıkın bıraktığı boş seçim her tıktan sonraki ilk
    /// tuşa boş bir kare isteterdi.
    ///
    /// **Girdi pencereyi dibe döndürür.** Geçmişe bakarken yazılan girdi
    /// görünmeyen bir satıra giderdi ve yanıtı da görünmezdi: alacritty
    /// kaydırılmış pencereyi yeni çıktıya karşı sabitliyor (`grid.scroll_up`
    /// ofseti artırıyor). Birincil ekranın ofseti alternate screen gidiş
    /// dönüşünden de sağ çıkıyor (`swap_alt` grid'i bütün olarak takas
    /// ediyor), yani geçmişte yazılan `vim` kapanınca istem yine görünmezdi.
    /// Emsal alacritty'nin ikilisi (tuş ve yapıştırma girdisinde
    /// `Scroll::Bottom`); kitaplık bunu yapmıyor.
    ///
    /// Bedeli girdi başına bir `Term` kilidi — sürükleme yolunun olay başına
    /// ödediğiyle aynı. Kilidi atlayan bir "kaydırılmış mı" önbelleği
    /// kurulmadı: alternate screen'de alt grid'in ofseti okunur ve birincil
    /// ekran hâlâ kaydırılmışken önbellek "dipte" derdi.
    fn write_owned(&self, bytes: Vec<u8>) {
        // `send_input` boş girdiyi kendisi de düşürüyor; buradaki erken dönüş
        // `Term` kilidini hiç almamak için.
        if bytes.is_empty() {
            return;
        }
        self.send_input(|_| bytes);
    }

    /// Kullanıcı girdisinin gönderimi: seçimi temizler, pencereyi dibe
    /// döndürür ve baytları **aynı** `Term` kilidinde, kipi görerek kurar
    /// (`bytes`). Ok tuşu kipi soruyor (DECCKM); ikinci bir kilit almasın diye
    /// soru buraya taşındı.
    ///
    /// Boş bayt ne gönderilir ne pencereyi ne seçimi oynatır: sıfır baytlık
    /// `Msg::Input` `EventLoop`'un yazıcısını kalıcı olarak kilitler
    /// (`Adapter::reply`).
    /// Kural çağıranlara bırakılmadı — ok gibi kipe bağlı bir sonraki tuş
    /// buradan geçecek.
    ///
    /// `bytes` kilit **altında** koşar ve `Session`'a dokunmamalı: `FairMutex`
    /// yeniden girilebilir değil, geri giren bir closure kendi kendini kilitler.
    ///
    /// Gönderimin doğurduğu nesli döndürür; boş baytta `None`.
    fn send_input(&self, bytes: impl FnOnce(TermMode) -> Vec<u8>) -> Option<u64> {
        let (bytes, redraw, moved) = {
            let mut term = self.term.lock();
            let bytes = bytes(*term.mode());
            if bytes.is_empty() {
                return None;
            }
            // Seçim dibe dönüşten **önce** düşer: "çizili miydi" sorusu
            // kullanıcının baktığı pencereye sorulmalı. Pencere kayarsa kare
            // zaten isteniyor; kaymazsa iki soru aynı cevabı verir.
            let cleared = clear_selection_locked(&mut term);
            // Dibe dönüş **kesri de** sıfırlıyor ve nesli artırıyor: uçuştaki
            // bir süzülme yazan kullanıcının penceresini dipten geri
            // çekmemeli ([`Session::reset_scroll`]).
            let dropped = self.reset_scroll();
            (
                bytes,
                cleared || dropped,
                self.scroll_user(&mut term, i32::MIN, self.band_shown()),
            )
        };
        // Dock'un seçimi de (031 R3.4): girdi iki seçimi birden temizliyor,
        // tek huni burası. Yaprak kilit `Term`'ün **dışında**.
        let redraw = self.clear_dock_selection() || redraw;
        // Yeniden bağlanma teklifi de (037 Karar 8): **ilk tuşta** kalkıyor —
        // kullanıcı başka bir şey yazmaya başladıysa niyeti bağlanmak değil.
        // Yer tutucu alacritty'nin hasarında yok, yani kare burada isteniyor.
        let redraw = lock(&self.shell).context.reconnect.take().is_some() || redraw;
        // Tek istek: temizlik, kesir ve dönüş aynı kareyi istiyor, vuruş başına
        // iki uyandırma olmasın. "Kaydı mı" kuralı `wake_if_moved`'da kalıyor.
        if redraw {
            self.request_frame();
        } else {
            self.wake_if_moved(moved);
        }
        // Nesil **gönderimden önce** ([`Session::key_gen`]'in doc'u).
        let generation = self.key_gen.fetch_add(1, Ordering::Release) + 1;
        self.send_or_hold(bytes);
        Some(generation)
    }

    /// Baytları gönderir — ya da ilk girdi henüz gitmediyse **tutar**
    /// ([`HeldInput`]; phase-3'ten devralınan `/code-review` bulgusu).
    ///
    /// ⌘T'nin uzak sekmesinde satır bizim ilk kimlikli `A`'mızda gidiyor ve
    /// o ana kadar yazılan tuşlar ZLE'nin typeahead'inde satırın **önüne**
    /// yapışırdı (`ls` + `ssh prod⏎` → `lsssh prod`). Tutulan baytlar
    /// satırdan sonra, aynı sırayla gidiyor: ssh'ın girdisine, yani uzağa —
    /// kullanıcı o sekmeyi uzak için açtı.
    ///
    /// **Tutma satır satır çözülüyor, süreyle değil:** satırı sonlandıran ya
    /// da kesen bir bayt (`\r`, `\n`, `^C` — [`RELEASES_HOLD`]) o ana kadar
    /// biriken baytları gönderiyor ve tutma **sürüyor** — ssh'ın satırı hâlâ
    /// bekliyor ve arkasından yazılan yarım satır onun önüne yapışırdı
    /// (`ls⏎pwd` → `ls` koşar, sonra `pwd` satırın arkasından). Tutmayı
    /// yalnız satırın teslimi bitiriyor. Kimlikli `A` hiç gelmezse (Karar 6'nın
    /// bilinen sınırı: rc'nin sonunda `exec fish`) ya da rc stdin'den satır
    /// okuyorsa klavye ölmüyor: her ⏎ ya da ^C yazılanı yolluyor. Bedeli
    /// (yankısızlık, tek tuş okuyan bir sorunun ⏎ istemesi) phase-4'ün
    /// Uygulama Notları'nda.
    ///
    /// Gönderim yuvanın kilidi **altında**: okuyucu thread'in teslimi de
    /// aynı kilitte ve aynı kanala yazıyor, yani kanalın sırası tutmanın
    /// sırası.
    fn send_or_hold(&self, bytes: Vec<u8>) {
        let mut held = lock(&self.held_input);
        let Some(typed) = held.as_mut() else {
            self.send(Msg::Input(bytes.into()));
            return;
        };
        typed.extend_from_slice(&bytes);
        if bytes.iter().any(|byte| RELEASES_HOLD.contains(byte)) {
            self.send(Msg::Input(std::mem::take(typed).into()));
        }
    }

    /// Yapıştırma bu kapıdan girer — **ham bayt bu kapının dışında kalır**.
    ///
    /// Ham yapıştırma (baytları dümdüz yazmak), kabuk satırında çalışan bir
    /// uygulamaya (vim, REPL, `read`) yapıştırınca satırları tek tek
    /// **çalıştırır**. Bunun çaresi bracketed paste (`\e[200~` … `\e[201~`):
    /// uygulamaya "bu bir yapıştırma" denir. Uygulama istemişse (DECSET 2004)
    /// sar, istememişse ham yaz — uygulamanın istemediğini terminalin
    /// bilemeyeceği bir şeyi terminal çözemez (Karar 3).
    ///
    /// Kip **tutulmaz**; alacritty'nin `Term`'inde yaşar, kilit altında
    /// sorgulanır. `session.write`'a doğrudan yapıştırma baytı verilmez —
    /// sarmalayan bu fonksiyondur.
    ///
    /// Sarmanın **tek istisnası** [`Session::can_be_typed`]: dock satırın
    /// sahibiyken, satır sonu ve kontrol karakteri taşımayan bir yük
    /// yazılmış girdi gibi akıtılır. Gerekçesi ve neden güvenliği
    /// düşürmediği orada.
    ///
    /// Boş yapıştırma iki dalda da sessizdir: sarma dalı bile boş
    /// `\e[200~\e[201~` çifti yazmaz, çünkü sıfır baytlık bir `Msg::Input`
    /// yazıcıyı kilitler (`write_owned`'ın kapısı). Boş olmayan yapıştırma
    /// [`Session::write`] gibi seçimi temizler ve pencereyi dibe döndürür —
    /// alacritty'nin `paste`'i de iki dalında `on_terminal_input_start`'ı
    /// çağırıyor.
    ///
    /// Baytları **sahiplenerek** alır (`&[u8]` değil): pano yükü megabayt
    /// mertebesine çıkabilir ve yapıştırma yolunda iki tam kopya
    /// (dilim → `Vec` → `Msg`) ana thread'de ödenmemeli. Çağıran zaten
    /// sahibi (`clipboard::read` → `String` → `into_bytes`), sarma dalı da
    /// tamponu kendisi kuruyor.
    pub fn paste(&self, bytes: Vec<u8>) {
        if bytes.is_empty() {
            return;
        }
        // Tazeleme kararı **her şeyden önce**: seçimin silinmesi nesli
        // ilerletiyor ve kapının "ayna cevap verdi" koşulunu kapatırdı.
        let refresh = self.paste_refreshes(&bytes);
        // **Seçimin yerine yapıştır** (031 Karar 8): dock'ta seçim varken ve
        // düzenleme kapısı açıkken önce seçim silinir, yük ardından bu
        // yoldan — sarma kararı yine aşağıda, silmeden bağımsız.
        self.dock_delete_selection();
        // Sarma sorgusu **önce ve tek başına**: iki kilit (`Term`, sonra
        // `shell`) ardışık alınıyor, iç içe değil.
        if self.bracketed_paste() && !self.can_be_typed(&bytes) {
            let mut wrapped = Vec::with_capacity(bytes.len() + 12 + DOCK_REFRESH_COMMAND.len());
            // `b"\e[200~"` yazılamaz: `\e` Rust kaçışı değil. Altı baytın
            // altısı da ASCII, `extend_from_slice` kopyalar.
            wrapped.extend_from_slice(b"\x1b[200~");
            // `ESC` ve `ETX` süzülüyor — sarma **kendi iğnesini**
            // koruyamazsa bracketed paste'in varlık sebebi kalmaz: panoya
            // `\x1b[201~` koyan bir süreç bölgeyi erken kapatır ve gerisi
            // uygulamaya **yazılmış girdi** olarak varır (kullanıcı Cmd-V'ye
            // basar basmaz satır çalışır, Enter yok, ekranda fark yok).
            // Çıplak `ETX` aynı deliğin ikinci hâli: ön plandaki işe SIGINT.
            // Emsal alacritty (`ActionContext::paste`) aynı iki baytı süzüyor.
            wrapped.extend(bytes.into_iter().filter(|b| !matches!(b, 0x1b | 0x03)));
            wrapped.extend_from_slice(b"\x1b[201~");
            // Aynı yazımda, kapanış iğnesinin arkasında: ayrı bir gönderim
            // nesli ikinci kez ilerletir ve araya kullanıcının tuşu girebilirdi.
            if refresh {
                wrapped.extend_from_slice(DOCK_REFRESH_COMMAND);
            }
            self.write_owned(wrapped);
        } else {
            // Ham dal sıfır kopya: sahiplenen bayt doğrudan kanala gider.
            self.write_owned(bytes);
        }
    }

    /// Yapıştırma **yazılmış girdi gibi** akıtılabilir mi — sarmanın dar
    /// istisnası.
    ///
    /// **Neden bir istisna var.** Dock canlıyken sarılı yapıştırma zsh'in
    /// `bracketed-paste-magic`'ine düşüyor ve o widget, yapıştırdığını
    /// vurgulu gösterip **bir sonraki tuşa kadar bekliyor**
    /// (`zle .read-command`, zsh 5.9 kaynağı). O sürede ZLE'nin görüntü
    /// kancası koşmadığı için ayna bayat kalıyor, tazelik kapısı bastırmayı
    /// bırakıyor ve kullanıcının yapıştırdığı metin dock yerine ızgarada
    /// beliriyor — giriş satırının sahibi bir tuşluk süre dock olmuyor.
    /// Kabuk tarafında beş çare ölçüldü ve yalnız bu sonuncusu çalışıyor
    /// (012 phase-4 → Uygulama Notları).
    ///
    /// **Güvenlik neden kaybolmuyor.** Sarmanın koruduğu iki şey de koşulun
    /// dışında kalıyor: satır sonu yoksa hiçbir satır kendiliğinden
    /// **çalışmaz** ([`Session::paste`]'in varlık sebebi), kontrol karakteri
    /// yoksa hiçbir tuş bağlaması tetiklenmez. Geriye kalan, kullanıcının
    /// elle yazabileceği düz bir metin: onu yazılmış girdi gibi akıtmak
    /// tanımı gereği aynı sonucu verir.
    ///
    /// **Dört koşul da zorunlu ve en dar hâliyle:**
    ///
    /// - Dock satırın sahibi ([`ShellLog::suppressed_input`], yani safha
    ///   `Input` **ve** ayna `Live`). Değilse hiç dokunulmuyor: vim, `less`,
    ///   `read` ve entegrasyonsuz oturum bugünkü korumalı yolda kalıyor.
    /// - ZLE **ekleme** keymap'inde ([`DockState::insert_keymap`]).
    /// - Yük geçerli UTF-8 — pano metni zaten `String`'den geliyor, ama
    ///   ölçüt baytta değil **karakterde** olmalı.
    /// - Hiç kontrol karakteri yok. [`char::is_control`] C0'ı da C1'i de
    ///   kapsıyor; satır sonu, sekme ve `ESC` üçü de bu testin içinde, yani
    ///   ayrıca sayılmıyorlar.
    ///
    /// **Keymap koşulu sonradan eklendi ve eksikliği bir kusurdu**
    /// (`/code-review`, 012 phase-6): üstteki "kullanıcı elle yazsa aynı
    /// sonucu verirdi" cümlesi her basılabilir baytın `self-insert`'e bağlı
    /// olmasını varsayıyor ve o varsayım yalnız ekleme keymap'inde doğru.
    /// `bindkey -v` kullanan biri Esc'e bastığında ZLE `vicmd`'ye geçiyor ama
    /// safha hâlâ `Input`, ayna hâlâ `Live`, blok hâlâ açık — yani kapı
    /// açılıyordu ve baytlar **komut** olarak yorumlanıyordu: panodaki `dd`
    /// satırı siler, `x` karakter siler, `p` kill-ring'i yapıştırır. Hiçbiri
    /// çalışmıyor (satır sonu yok) ama tampon sessizce değişiyordu. İki sarılı
    /// yol (`bracketed-paste` ve `bracketed-paste-magic`) her keymap'te
    /// harfi harfine ekliyor, yani istisna kapanınca davranış doğruya dönüyor.
    /// Satır sonlu yapıştırmanın arkasına tazeleme komutu eklenecek mi
    /// (032 R5): yük satır sonu taşıyor **ve** düzenleme kapısının dört
    /// koşulu yapıştırmadan **önce** açık ([`Session::can_edit_dock`]).
    ///
    /// **Neden.** `bracketed-paste-magic` (oh-my-zsh kuruyor) yükü `zle -U`
    /// ile kuyruğa geri basıyor ve ZLE typeahead varken redisplay'i
    /// atlıyor: ayna bir tuş boyunca bayat, tazelik kapısı satırı o süre
    /// ızgarada tutuyor. Kuyruğun arkasındaki komut widget'ı koşturuyor ve
    /// widget aynayı yapıştırmanın sonucuyla basıyor. Tek satırlık yük
    /// buraya hiç uğramıyor: [`Session::can_be_typed`] onu zaten sarmıyor.
    ///
    /// **Kapı tam**, gevşek değil: `vicmd`'de dizi bağlı değil ve baytları
    /// komut olurdu, bağlamasız kabukta BEL `send-break`.
    fn paste_refreshes(&self, bytes: &[u8]) -> bool {
        bytes.iter().any(|b| matches!(b, b'\n' | b'\r')) && self.dock_edit_line().is_some()
    }

    fn can_be_typed(&self, bytes: &[u8]) -> bool {
        // Tutulan `line-finish` (032 Karar 11) istisnayı kapatıyor: ayna
        // kabul edilmiş satırın, keymap'i de onun — yeni satırınki değil.
        let typed = {
            let log = lock(&self.shell);
            !log.holding_end()
                && log
                    .suppressed_input()
                    .is_some_and(|input| input.insert_keymap)
        };
        if !typed {
            return false;
        }
        std::str::from_utf8(bytes).is_ok_and(|text| !text.chars().any(char::is_control))
    }

    /// DECSET 2004 (bracketed paste) set mi — kipi `Term`'den, kilit altında
    /// sorar. `paste()`'in iki dalını ayıran tek soru; tutulmaz.
    ///
    /// `pub` değil `fn`: kapı `paste()`'tir — kip dışarıdan sorgulansaydı
    /// biri `write`'a ham yapıştırma baytı vererek sarma kapısını by-pass
    /// edebilirdi. Sınamalar aynı modülde, erişir.
    fn bracketed_paste(&self) -> bool {
        self.term.lock().mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Grid'i ve PTY'yi yeni boyuta getirir. Reflow alacritty'nindir.
    ///
    /// `true` → boyut gerçekten değişti ve uygulandı. `false` iki durumda
    /// döner: boyut dejenere (yoksayıldı) ya da zaten aynı. Çağıranın buna
    /// ihtiyacı var çünkü hücre piksel boyutunu **kendi** tarafında da
    /// tutuyor: yoksayılan bir boyutu orada uygulamak grid'i eski ölçüde
    /// bırakıp çizimi yeni ölçüye kaydırırdı.
    #[must_use]
    pub fn resize(&self, cols: u16, rows: u16, cell_px: (u16, u16)) -> bool {
        // Simge durumuna inen ya da sıfır yükseklikli pencere 0 hesaplatabilir.
        // Bu boyut kırpılmaz, YOKSAYILIR: 1 sütuna reflow geçmişi kalıcı
        // olarak yok eder ve PTY'ye 1×1 winsize gitmesi tam ekran uygulamaları
        // bozar. Görünmeyen pencerede çizecek bir şey de yok.
        if cols == 0 || rows == 0 {
            return false;
        }
        let grid = GridSize::exact(cols, rows);
        let size = window_size(grid, cell_px);

        // Ucuz kapı önce. Canlı boyutlandırmada geometri bildirimlerinin
        // (`bt-shell`: içerik view'ının çerçeve değişimi) çoğu hücre sınırını geçmez ve hiçbir şey yapmaz;
        // `Term`'ün kilidi ise okuyucunun ayrıştırma lease'inin arkasında
        // bekleyebilir. Küçük kilitle eleyip oraya hiç girmiyoruz. Guard
        // `term`'den ÖNCE düşüyor, kilit sırası (term → size) bozulmuyor.
        let changed = !same_size(*lock(&self.adapter.0.size), size);
        if !changed {
            return false;
        }

        // Üç adım tek kilit tutuşunda: grid, adapter'ın bildiği boyut ve
        // PTY'ye giden mesaj. Ayrı ayrı yapılsalardı eşzamanlı iki resize
        // grid'i bir sayıda, `TIOCSWINSZ`'i başkasında bırakabilirdi.
        // Okuyucu thread de aynı sırayla (term → size) kilit alıyor,
        // kilitlenme yok; `send` kilitsizdir.
        let mut term = self.term.lock();
        let mut prev = lock(&self.adapter.0.size);
        // Ön kapıdan iki eşzamanlı resize birlikte geçebilir; ikincisi burada
        // yakalanır. Koşulsuz dikilen bayrak "boşta sıfır kare"yi delerdi.
        if same_size(*prev, size) {
            return false;
        }
        term.resize(grid);
        *prev = size;
        self.send(Msg::Resize(size));
        // Boyut değişimi grid'i değiştiren ama `Wakeup` üretmeyen tek yol,
        // bayrağı bu yüzden elle dikiyoruz. Uyandırmak çağıranın işi
        // (`bt-shell` resize'dan sonra link'i açar); bayrak kimseyi uyandırmaz.
        self.adapter.0.dirty.store(true, Ordering::Release);
        // Yeniden sarma defterin satırlarını değiştirdi: arama açıksa sayım
        // baştan (033; geçerli eşleşmenin kaybı `search::ledger_shift`'te).
        self.adapter.search_changed();
        true
    }

    /// Okuyucu thread'i durdurur ve shell çocuğunu bitirir.
    ///
    /// `Msg::Shutdown` → `join` → dönen `(EventLoop, State)`'in düşmesi.
    /// `SIGHUP` o son adımda, `Pty`'nin `Drop`'unda gider. İkinci çağrı
    /// sessizce döner.
    ///
    /// **En çok [`SHUTDOWN_GRACE`] bekler**, çünkü son iki adım bu thread'de
    /// değil: tutamak ayrı bir thread'e taşınıyor, `join` ve düşme orada
    /// koşuyor, bu fonksiyon onları sınırlı bekliyor. Süre dolarsa çocuk
    /// arkada bırakılır ve bir satır stderr'e düşer.
    ///
    /// Sınırın sebebi `Pty::drop`'un `SIGHUP`'tan sonra çağırdığı
    /// `child.wait()` ve o çağrının **iki** ayrı sebeple dönmemesi:
    ///
    /// 1. Sinyali yutan çocuk (`trap '' HUP`) hiç ölmez; `wait` çocuk kendi
    ///    kendine bitene kadar bekler (ölçüldü: 10 saniyelik betikte
    ///    `shutdown` 10,0 saniye bloklamıştı).
    /// 2. PTY'ye **yazmakta** olan çocuk `SIGHUP`'ı alıp çıkışa girer ama
    ///    çıkışın içinde takılır (`ps` durumu `?Es`): master fd'yi artık
    ///    kimse okumuyor — okuyucu thread bitti ve fd `Pty`'nin bir alanı,
    ///    yani `Drop` gövdesinden **sonra** kapanıyor. O çocuk `SIGKILL` ile
    ///    de kurtulmuyor (ölçüldü); ancak sürecimiz ölüp master kapandığında
    ///    gidiyor. Ölçüm yükünün koşularını asan mekanizma buydu.
    ///
    /// **Sonucu döndürüyor** ([`Teardown`]) ve bu bir tanı yüzeyi: sınır dolan
    /// koşu (çocuk arkada kaldı) ve **panikle biten okuyucu** eskiden yalnız
    /// stderr'de görünüyordu, yani duman/ölçüm raporunun jeton satırı ikisini
    /// de **yeşil** basıyordu. İkinci ve
    /// sonraki çağrılar [`Teardown::AlreadyDone`] döner: gerçek sonucu ilk
    /// çağrı bilir, `Drop`'unki değil.
    ///
    /// Yani sınır çocuğu **iyileştirmiyor**, kapanışı sınırlıyor: süresi
    /// dolan yolda çocuk çıkışın içinde kalır ve onu süreç çıkışı toplar.
    /// Kalıcı çare (2) için master'ı `wait` bloklarken boşaltmaktır; bu
    /// crate'ten yolu `Session::spawn`'da `pty.file().try_clone()` ile
    /// master'ın bir kopyasını almaktan geçer (`EventLoop` `Pty`'yi
    /// `join`'den sonra **vermiyor**, yani kopya baştan alınmak zorunda).
    /// Yapılmadı ve borç olarak kayıtlı: sınır her hâlde gerekiyor, (1)
    /// boşaltmakla çözülmüyor.
    ///
    /// **İki şeyi vaat etmiyor.** Süre dolduğunda `SIGHUP`'ın gittiği garanti
    /// değil: yavaş olan adım `join` ise (okuyucu thread hâlâ ayrıştırıyorsa)
    /// `Pty::drop` daha başlamamıştır ve çocuğa asıl hangup'ı süreç çıkışının
    /// master'ı kapatması verir. Sınır da her yolda yok: kapanış thread'i
    /// kurulamazsa (OS thread sınırı) bu fonksiyon sınırsız kalır, gövdedeki
    /// yorum o dalın iki sonucunu sayıyor.
    pub fn shutdown(&self) -> Teardown {
        match self.begin_shutdown() {
            Some(handle) => handle.wait_until(Instant::now() + SHUTDOWN_GRACE),
            None => Teardown::AlreadyDone,
        }
    }

    /// [`Session::shutdown`]'ın **başlatan** yarısı: `Msg::Shutdown`'ı yollar,
    /// `join` ile düşmeyi "PTY teardown" thread'ine verir ve **beklemez**.
    /// İkinci ve sonraki çağrılar `None` döner (kapanış zaten başladı).
    ///
    /// Bölünmenin sebebi çok pencere: bir sekmeyi kapatmak ana thread'i
    /// yarım saniyeye kadar durdurmamalı ve Cmd-Q bütün oturumları **tek**
    /// [`SHUTDOWN_GRACE`] içinde paralel kapatabilmeli — önce hepsi başlar,
    /// sonra ortak bir son tarihe kadar beklenir
    /// ([`ShutdownHandle::wait_until`]).
    ///
    /// Tutamak beklenmeden düşerse kapanış **yine biter**: teardown thread'i
    /// kanalın ölüsüne yazar ve sonucu yutar; `SIGHUP` ile `child.wait()`
    /// `Pty::drop`'ta, o thread'de koşar. Sonucu kimse bilmez — tanı yüzeyi
    /// yalnız stderr satırlarıdır (`Drop for Session`'ın durumu).
    pub fn begin_shutdown(&self) -> Option<ShutdownHandle> {
        let reader = lock(&self.reader).take()?;
        self.send(Msg::Shutdown);

        // `join` de düşme de bloklayabilir (iki sebep `shutdown`'ın doc'unda),
        // yani ikisi de bu thread'de koşmuyor. Kanal iki şey taşıyor:
        // **zamanlama** ("bitti" haberi gelmezse sınır dolmuştur) ve
        // okuyucunun paniğe düşüp düşmediği. İkincisi `()` ile taşınamazdı ve
        // taşınmayınca panikleyen bir okuyucu raporda `temiz` görünüyordu.
        let (done, finished) = mpsc::channel();
        let teardown = thread::Builder::new()
            .name("PTY teardown".to_owned())
            .spawn(move || {
                let tail = reader.join();
                let reader_ok = tail.is_ok();
                if !reader_ok {
                    eprintln!("bateri: okuyucu thread panikle bitti");
                }
                // Düşme `send`'den **önce**, açıkça: `SIGHUP` ve
                // `child.wait()` `Pty::drop`'ta koşuyor, yani sınırın
                // kapsaması gereken iş bu satır. `send` öne alınsa sınır
                // yalnız `join`'i kapsar ve belirti sessizce geri döner —
                // `shutdown_returns_within_limit`'in alt sınırı tam bunu
                // kırmızıya çeviriyor.
                drop(tail);
                // Alıcı düşmüşse (tutamak beklenmeden bırakıldı) sonuç
                // yutulur; kapanışın kendisi yukarıda çoktan bitti.
                let _ = done.send(reader_ok);
            });
        // Thread kurulamazsa (OS thread sınırı) **sınır yoktur** ve bu dalda
        // kapanışın nerede koştuğu bir yarışa bağlı: tutamak `spawn`
        // başarısız olurken closure ile birlikte çoktan düşmüştür, yani
        // `(EventLoop, State)` çiftini ya okuyucu thread kendi bitişinde
        // düşürür (kimse bloklanmaz, ama `SIGHUP` + `child.wait()` sınırsız
        // koşar) ya da — okuyucu thread çoktan bitmişse — çift orada düştüğü
        // için `Pty::drop` bu thread'i bloklar. İkisi de sessiz kalmasın.
        Some(match teardown {
            Ok(_) => ShutdownHandle(Some(finished)),
            Err(err) => {
                eprintln!("bateri: kapanış thread'i kurulamadı ({err}), kapanış sınırsız");
                ShutdownHandle(None)
            }
        })
    }

    /// PTY'nin çocuğunun pid'i — **kabuğun değil**, en azından her zaman
    /// değil: süresiz oturumda çocuk `login(1)` ve kabuk onun çocuğu
    /// (`.tasks/028-kapatma-onayi/context.md` → Süreç tarafı). Hangisi
    /// olduğunu komutu kuran taraf biliyor, bu crate değil.
    ///
    /// Çocuk biçildikten sonra **bayatlar**: sayı aynı kalıyor, işletim
    /// sistemi onu başka bir sürece verebilir. Tüketen taraf onu
    /// [`Session::reader_alive`] ile birlikte sorar — okuyucu thread çocuğun
    /// çıkışını görünce bitiyor.
    pub fn child_pid(&self) -> u32 {
        self.child_pid
    }

    /// Okuyucu thread hâlâ çalışıyor mu. `false` ya kapandığımız ya da
    /// shell'in öldüğü anlamına gelir.
    pub fn reader_alive(&self) -> bool {
        lock(&self.reader)
            .as_ref()
            .is_some_and(|reader| !reader.is_finished())
    }

    fn send(&self, msg: Msg) {
        // Kanal alıcısı okuyucu thread'in dönüş değerinde yaşıyor, yani
        // `join` edilene kadar `send` başarılı olur: shell kendi kendine
        // çıktıktan sonra yazılan baytlar kimsenin emmeyeceği bir kuyruğa
        // gider. "Gitti mi" sorusunun cevabı `reader_alive()`. Bu dal
        // yalnız `shutdown` sonrası yazmayı yakalar; giriş yolunda panik
        // yasak, bir satır iz yeter.
        if self.sender.send(msg).is_err() {
            eprintln!("bateri: okuyucu thread kapalı, PTY'ye yazılamadı");
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // `shutdown()`suz düşen bir oturumda okuyucu döngü `Msg::Shutdown`
        // almaz; kanalın ölümü alacritty'nin kopyasında panik değil boş okuma
        // ([`crate::reader`]) ve döngü çocuk çıkana kadar koşar.
        //
        // Sonuç bilerek yutuluyor: `Drop`'un raporlayacak bir yeri yok. Rapor
        // yolunda buraya zaten [`Teardown::AlreadyDone`] kalır
        // (`bt-shell` `shutdown()`'ı kendi çağırıp sonucu basıyor); rapor
        // olmayan bir yolda ise gerçek sonuç burada düşer ve **kimse
        // bakmaz** — o yolda kapanışın nasıl bittiğini söyleyen şey
        // `shutdown()`'ın kendi stderr satırlarıdır.
        let _ = self.shutdown();
    }
}

/// `WindowSize` `PartialEq` türetmiyor; dört alanı elle karşılaştırıyoruz.
fn same_size(a: WindowSize, b: WindowSize) -> bool {
    a.num_cols == b.num_cols
        && a.num_lines == b.num_lines
        && a.cell_width == b.cell_width
        && a.cell_height == b.cell_height
}

/// Görünen pencereyi kilit altında kaydırır: kip kapısı ve kırpma. Kaydırmanın
/// üç yolunun (tekerlek, sayfa, girdide dibe dönüş) ortak gövdesi; kareyi
/// çağıran ister, çünkü uyandırma kilit bırakıldıktan sonra koşmalı.
///
/// `None` → alternate screen: alt grid geçmişsiz ve birincil ekranın ofseti
/// orada kalıyor; kaydırılacak bir şey yok. `Some(n)` → **ofset** `n` satır
/// değişti, geçmişin iki ucunda `0`.
///
/// `band` ekranda duran doldurma bandının boyu ([`Session::fill_shown`]) ve
/// kaydırmanın başlangıç noktasını belirliyor; gerekçe gövdede. `0` geçmek
/// bandı hiç olmayan pencerenin hâli, yani kuralın kapalı kolu.
///
/// Dönen sayı **görsel** hareket değil ofset farkı: bandı olan pencerenin ilk
/// çentiği ofseti `band + 1` yapıyor ama ekran bir satır kayıyor. İki
/// tüketici de yalnız "kaydı mı" diye soruyor ([`Session::wake_if_moved`],
/// `bt-shell`'in `follow_pointer`'ı), o yüzden fark ayrı bir tip istemiyor.
fn scroll_locked<T: EventListener>(term: &mut Term<T>, lines: i32, band: i32) -> Option<i32> {
    if term.mode().contains(TermMode::ALT_SCREEN) {
        return None;
    }
    let before = term.grid().display_offset() as i32;
    // **Bant bir sanal kaydırmadır** ([`Session::fill_shown`]): dibe yaslı
    // pencerede ekranın tepesi `Line(-band)` ve orası `display_offset == band`
    // olan bir pencerenin de tepesi. Kaydırma bu yüzden `0`'dan değil
    // `band`'den devam ediyor — aradaki `1..=band` aralığı ekranda **hiç
    // görülmeyen** ofsetler: orada bant kalkıyor ama gösterilen satırlar
    // bandın gösterdiklerinin ta kendisi, yani o ofsetlerde durmak
    // kaydırmayı `band` çentik boyunca ölü gösterirdi. Ölçüldü (2026-09-20,
    // gözle kontrol; kullanıcı).
    //
    // İki uç da aynı kuralla kapanıyor: yukarı çıkarken `band`'in üstüne
    // atlanıyor, aşağı inerken `band`'e **değen** hedef dibe (`0`) düşüyor.
    // Kaydırma aralığa hiç sokmadığı için bandın iki kenarı da bir uçurum
    // değil.
    //
    // **Ama kaydırma tek giriş değil:** pencereyi büyütmek geçmişten satır
    // çekiyor ve alacritty ofseti o kadar düşürüyor
    // (`grid/resize.rs`'te `grow_lines` → `display_offset.saturating_sub`;
    // [`Session::resize`] dibe snap'lemiyor). Yani `before` aralığın
    // **içinde** doğabiliyor ve orada kural kendi amacının tersine çalışırdı:
    // yukarı bir çentik `target`'ı `band`'in altında tutar ve kullanıcıyı
    // yukarı çıkmak isterken dibe indirirdi. İçeriden kaydırma bu yüzden
    // **düz**: aralık birkaç çentikte terk ediliyor ve dibe varış zaten
    // kaydırmanın doğal ucu.
    // **Üst uç dışarıda** (`1..band`, `1..=band` değil) ve fark ölçüldü
    // (2026-09-20, kullanıcı: "seri scroll'da dalgalanıyor"): `offset ==
    // band` kaydırmanın **meşru varış noktası** — defter tam bandın boyu
    // kadarsa yukarı çıkan pencere orada duruyor (`clamp`) ve orası
    // görsel olarak dibin ta kendisi. Muafiyetin içine alınınca aşağı inen
    // çentik dibe snap'lemek yerine `band-1`, `band-2` … diye tek tek
    // iniyordu ve `offset == 1`'den `0`'a geçişte bant birden geri gelip
    // ekranı bandın boyu kadar zıplatıyordu. Aralığın **içi** yalnız
    // resize'ın bırakabileceği ofsetler.
    let inside = (1..band).contains(&before);
    let virtual_before = if before == 0 { band } else { before };
    let target = virtual_before.saturating_add(lines);
    let lines = if target <= band && !inside {
        -before
    } else {
        target - before
    };
    // Kırpma **ulaşılabilir** aralığa (`[-ofset, geçmiş - ofset]`): ötesi
    // zaten aynı yere varır, ama alacritty ofseti `i32`'de topluyor
    // (`offset + count`) ve kırpılmamış bir delta debug derlemesinde
    // **panik**, sürümde ters yöne sarma olurdu. Bu aralıkta toplam
    // `[0, geçmiş]`'ten çıkamaz. `bt-shell` deltayı `f64`'ten doyurarak
    // çeviriyor, yani `i32::MAX` ulaşılabilir bir değer. Geçmişin `i32`'ye
    // sığması alacritty'nin kendi varsayımı (`display_offset as i32`);
    // sığmazsa kırpılır.
    let history = i32::try_from(term.history_size()).unwrap_or(i32::MAX);
    term.scroll_display(Scroll::Delta(lines.clamp(-before, history - before)));
    Some(term.grid().display_offset() as i32 - before)
}

/// Kesirli kaydırmanın gövdesi: `frac + rows`'un tam kısmını [`scroll_locked`]'a
/// indirir ve kalan kesri döndürür. Dönüş `(ofset farkı, yeni kesir)`;
/// `None` alternatif ekran (`scroll_locked`'ın kapısı).
///
/// **Bant eşlemesi ikinci kez yazılmıyor**: tam satırı taşıyan yine
/// [`scroll_locked`], yani bandın sanal kaydırması, `1..band` muafiyeti ve
/// kırpma olduğu gibi geçerli. Bu fonksiyonun kendi kuralı tek ve iki uçtan
/// kesir düşürüyor (R1.2):
///
/// - **Tam kısım gidemediyse kesir yok.** Ekranın tepesindeki satır
///   ([`visual_top`]) tam kısım kadar oynamadıysa kaydırma bir uca
///   dayanmıştır: dipte negatif, geçmişin tepesinde pozitif kesir yerine
///   pencere uçta tam satırda duruyor. Resize'ın bıraktığı iç ofsetten dibe
///   inen adım da buraya düşüyor (orada tepe bant kadar sıçrıyor) — o iniş
///   zaten süreksiz.
/// - **Üstünde satır olmayan pencerede kesir yok**: kesrin açtığı şeridi
///   kapatacak satır ([`Cursor::top_row`]) defterde değilse şerit boş kalırdı.
///
/// Sonlu olmayan toplam hiçbir şeyi değiştirmiyor — NaN kesre girseydi
/// sonraki her toplam NaN olur ve kaydırma sessizce ölürdü (`bt-shell`'in
/// `wheel_lines` emsali).
fn scroll_fraction_locked<T: EventListener>(
    term: &mut Term<T>,
    frac: f64,
    rows: f64,
    band: i32,
) -> Option<(i32, f64)> {
    if term.mode().contains(TermMode::ALT_SCREEN) {
        return None;
    }
    let total = frac + rows;
    if !total.is_finite() {
        return Some((0, frac));
    }
    // **Tam satıra yakın toplam tam satırdır**: kare yolunun payı `f32`'de
    // geliyor ([`ScrollGlide`]) ve yerleşmenin `−0.3`'ü `f32`'de
    // `−0.30000001`. Toplam `−1.2e-8` olur, `floor` onu bir satır aşağı
    // atar ve pencere `0.99999998` kesirle yerleşmemiş kalırdı — tepe
    // satırı açık, sonraki satır adımı iki satır sıçrıyor
    // (`/code-review`, 027 phase-1). Eşik bir pikselin çok altında; kesir
    // bu kadar küçükse zaten çizilemiyor.
    let nearest = total.round();
    let total = if (total - nearest).abs() < 1e-5 {
        nearest
    } else {
        total
    };
    let whole = total.floor();
    // `as` doyuruyor; ulaşılabilir aralığa kırpma `scroll_locked`'ın işi.
    let lines = whole as i32;
    let before = term.grid().display_offset() as i32;
    let moved = if lines == 0 {
        0
    } else {
        scroll_locked(term, lines, band)?
    };
    let top = visual_top(before + moved, band);
    let reached = i64::from(top) - i64::from(visual_top(before, band)) == i64::from(lines);
    let history = i32::try_from(term.history_size()).unwrap_or(i32::MAX);
    let frac = if reached && top < history {
        total - whole
    } else {
        0.0
    };
    Some((moved, frac))
}

/// Ekranın tepesindeki satırın geçmişteki derinliği: `Line(-visual_top)`.
///
/// Dibe yaslı pencerede bant bir sanal kaydırma ([`Session::fill_shown`]) ve
/// tepe `Line(-band)`; kaydırılmış pencerede bant yok ve tepe ofsetin kendisi.
/// Kesrin iki kuralı da bu sayıya bakıyor, ofsete değil: bandın ilk
/// çentiğinde ofset `band + 1` sıçrarken tepe tam bir satır oynuyor.
fn visual_top(offset: i32, band: i32) -> i32 {
    if offset == 0 { band } else { offset }
}

/// Konum `up` yönünde oynayabilir mi — süzülme isteğinin uç kapısı.
///
/// Kesir sıfırdan büyükse iki yön de açık (üstündeki satır var olmak zorunda,
/// [`scroll_fraction_locked`]). Tam satırda yukarı: tepenin üstünde defterde
/// satır var mı; aşağı: pencere dipte değil mi.
fn scroll_room<T>(term: &Term<T>, frac: f64, band: i32, up: bool) -> bool {
    if frac > 0.0 {
        return true;
    }
    let offset = term.grid().display_offset() as i32;
    if up {
        let history = i32::try_from(term.history_size()).unwrap_or(i32::MAX);
        visual_top(offset, band) < history
    } else {
        offset != 0
    }
}

/// Seçimi kilit altında düşürür; ekranda **çizili** bir aralık gittiyse `true`
/// — kareyi çağıran ister, `scroll_locked` gibi. Temizliğin iki yolunun
/// ortak gövdesi: girdide `send_input` (üretimdeki tek temizlik yolu) ve
/// [`Session::clear_selection`] (`pub` API, bugün yalnız sınamalardan
/// çağrılıyor). Tek gövde, "kare ne zaman" sorusunun tek cevabı olsun diye:
/// hiç seçim yoksa da, sürüklemesiz tıkın boş seçimiyse de, ekranda olmayan
/// (geçmişte kalan) bir seçimse de `false`.
fn clear_selection_locked<T>(term: &mut Term<T>) -> bool {
    let selection = term.selection.take();
    visible_range(selection.as_ref(), term).is_some()
}

/// Raporun gövdesi: işaretçinin hücresini uygulamanın satırına indirir ve
/// baytları kurar. **Kilidi çağıran tutuyor** — `offset` onun altından
/// okundu; burası saf.
///
/// `pressed` yalnız iki şeyi seçiyor ve ikisi de aynı ayrımdan: raporun
/// biçimi (SGR'da `M`/`m`) ve **sığmayan koordinatın akıbeti**. Jest
/// başlatan ya da süren olay (basış, hareket) reddediliyor; jesti bitiren
/// bırakma kırpılıyor, çünkü düşen bırakma uygulamada takılı kalmış bir
/// düğme bırakır (R6).
fn mouse_report_at(
    encoding: MouseEncoding,
    byte: u8,
    at: SelectionPoint,
    offset: i32,
    pressed: bool,
) -> Option<Vec<u8>> {
    let line = viewport_point((at.col, at.row), offset).line;
    if pressed {
        let row = u16::try_from(line.0).ok()?;
        return input::mouse_report(encoding, byte, true, at.col, row);
    }
    // Satır yalnız **negatife** kaçabilir (pencere geçmişte, ya da işaretçi
    // doldurma bandında): `display_offset` eksi olmadığı için üstten taşma
    // yok. Kodlamanın tavanı ayrıca kırpılıyor — sütun da, satır da.
    let row = u16::try_from(line.0.max(0)).unwrap_or(u16::MAX);
    let (col, row) = (encoding.clamp(at.col), encoding.clamp(row));
    input::mouse_report(encoding, byte, false, col, row)
}

/// Görünür pencere hücresini grid noktasına çevirir.
fn viewport_point((col, row): (u16, u16), display_offset: i32) -> Point {
    Point::new(
        Line(i32::from(row) - display_offset),
        Column(usize::from(col)),
    )
}

fn window_size(grid: GridSize, cell_px: (u16, u16)) -> WindowSize {
    WindowSize {
        num_cols: grid.cols as u16,
        num_lines: grid.rows as u16,
        cell_width: cell_px.0,
        cell_height: cell_px.1,
    }
}

/// Zehirlenmiş kilitten içeriği geri alır. Bu kilitlerin altında `Option`
/// ve dört sayı var; panik anında da tutarlılar. Kapanış yolunda ikinci bir
/// panik üretmenin kimseye faydası yok.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::sync::{Condvar, Mutex};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::settings::HostMark;
    use crate::shell::{DockContext, DockFault, DockState, DockStatus, ShellPhase};

    /// Sınamaların teması: gömülü koyu tema, `bt-shell`'in süreli koşusu gibi.
    const THEME: Theme = Theme::BATERI;

    /// Sınamaların dock bütçesi: üretimin oranı (ızgaranın yarısı) ve
    /// sınamaların varsayılan genişliği ([`test_options`]'ın 40 sütunu) —
    /// sarmanın genişliği ızgaranınki olmak zorunda. Tavanı soran sınama kendi
    /// bütçesini kuruyor.
    const BUDGET: DockBudget = DockBudget {
        share: 0.5,
        cols: 40,
    };

    /// Hasar sorusu + tarama, eski `frame()`'in şekliyle: `None` → kare
    /// istenmedi.
    ///
    /// Üretimde ikisi ayrı çağrılıyor ([`Session::take_damage`]'in doc'u
    /// sebebi yazıyor), ama bu sınamaların sorduğu şey neredeyse hep "bu olay
    /// kare istedi mi" — yani tek ifade. Ayrı ayrı yazılsalardı her sınama
    /// aynı iki satırı kopyalar ve hasarı yanlışlıkla iki kez tüketen bir
    /// sınama kendi kendini sessizce yeşile çevirirdi.
    ///
    /// Blok tamponu **burada doğup burada ölüyor**: bu sınamaların sorduğu şey
    /// kare isteğiydi, blok değil. Blokları okuyan sınamalar
    /// [`blocks_if_damaged`] ile tamponu kendileri tutar.
    fn frame_if_damaged(session: &Session, sink: impl FnMut(Cell)) -> Option<Cursor> {
        session.take_damage().then(|| {
            session.frame(
                sink,
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            )
        })
    }

    /// [`frame_if_damaged`]'in seçim soran kardeşi: hücreler, koşular ve
    /// doluluk birlikte — seçimin hücreyi değil koşuyu boyadığını ve
    /// doluluğa dokunmadığını aynı karede sormak için.
    fn runs_if_damaged(session: &Session) -> Option<(Vec<Cell>, Vec<SelectionRun>, Cursor)> {
        session.take_damage().then(|| {
            let mut cells = Vec::new();
            let mut runs = SelectionRuns::default();
            let cursor = session.frame(
                |c| cells.push(c),
                |_| (),
                &mut Blocks::default(),
                &mut runs,
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            (cells, runs.as_slice().to_vec(), cursor)
        })
    }

    fn run(row: u16, first: u16, last: u16) -> SelectionRun {
        SelectionRun { row, first, last }
    }

    /// [`frame_if_damaged`]'in blok soran kardeşi: tamponu çağıran tutar,
    /// böylece sınama hem hücreleri hem şeritleri görebilir.
    fn blocks_if_damaged(session: &Session, blocks: &mut Blocks) -> bool {
        session.take_damage() && {
            session.frame(
                |_| (),
                |_| (),
                blocks,
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            true
        }
    }

    /// Uyandırmaları sayar, pano yazmalarını kaydeder ve sınamanın
    /// beklemesine izin verir.
    #[derive(Default)]
    struct TestWake {
        state: Mutex<TestWakeState>,
        cond: Condvar,
    }

    #[derive(Default)]
    struct TestWakeState {
        wakes: u32,
        exit: Option<Option<i32>>,
        /// [`Wake::copy_to_clipboard`]'ın metinleri, geliş sırasıyla.
        copies: Vec<String>,
        /// [`Wake::title_changed`] kaç kez geldi.
        titles: u32,
        /// [`Wake::search_changed`] kaç kez geldi.
        searches: u32,
        /// [`Wake::command_started`] kaç kez geldi.
        commands: u32,
    }

    impl TestWake {
        /// En az `target` uyandırma gelene kadar bekler.
        fn wait_wakes(&self, target: u32, timeout: Duration) -> u32 {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .cond
                .wait_timeout_while(state, timeout, |state| state.wakes < target)
                .unwrap();
            state.wakes
        }

        /// Çocuk ölene kadar bekler; zaman aşımında `None`.
        fn wait_exit(&self, timeout: Duration) -> Option<Option<i32>> {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .cond
                .wait_timeout_while(state, timeout, |state| state.exit.is_none())
                .unwrap();
            state.exit
        }

        /// Şimdiye kadar gelen pano metinleri.
        fn copies(&self) -> Vec<String> {
            self.state.lock().unwrap().copies.clone()
        }

        /// Şimdiye kadar gelen başlık haberleri.
        fn titles(&self) -> u32 {
            self.state.lock().unwrap().titles
        }

        /// En az `target` komut haberi gelene kadar bekler.
        fn wait_commands(&self, target: u32, timeout: Duration) -> u32 {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .cond
                .wait_timeout_while(state, timeout, |state| state.commands < target)
                .unwrap();
            state.commands
        }

        /// En az `target` başlık haberi gelene kadar bekler.
        fn wait_titles(&self, target: u32, timeout: Duration) -> u32 {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .cond
                .wait_timeout_while(state, timeout, |state| state.titles < target)
                .unwrap();
            state.titles
        }
    }

    impl Wake for TestWake {
        fn wake(&self) {
            self.state.lock().unwrap().wakes += 1;
            self.cond.notify_all();
        }

        fn child_exit(&self, code: Option<i32>) {
            self.state.lock().unwrap().exit = Some(code);
            self.cond.notify_all();
        }

        // Sınamanın uygulayıcısı kilit alıyor, `wake` gibi: sözleşmenin
        // "kilit almaz" yasağı üretim içindir, buradaki muteks yapraktır ve
        // tutulurken `Session`'a girilmez.
        fn copy_to_clipboard(&self, text: String) {
            self.state.lock().unwrap().copies.push(text);
            self.cond.notify_all();
        }

        fn title_changed(&self) {
            self.state.lock().unwrap().titles += 1;
            self.cond.notify_all();
        }

        fn search_changed(&self) {
            self.state.lock().unwrap().searches += 1;
            self.cond.notify_all();
        }

        fn command_started(&self) {
            self.state.lock().unwrap().commands += 1;
            self.cond.notify_all();
        }
    }

    /// [`smoke_shell`]'in ta kendisiyle bir oturum: `make duman`'ın koştuğu
    /// betiğin sayılarını doğrulayan üç sınamanın ortak kurulumu. Betiğin
    /// `sleep`'i uzun ama önemsiz — oturum düşerken `SIGHUP` çocuğu keser.
    fn spawn_smoke(wake: Arc<TestWake>) -> Session {
        spawn_with_command(smoke_shell(), wake)
    }

    fn spawn_session(script: &str, wake: Arc<TestWake>) -> Session {
        spawn_with_command(sh(script), wake)
    }

    /// Blink'i **açılıştan** açık oturum: ayarın `Session::spawn` yolundan
    /// geçtiğini sınayan tek kurulum.
    fn spawn_blinking_session(script: &str, wake: Arc<TestWake>) -> Session {
        let mut options = test_options(sh(script), 40);
        options.terminal.blink = CursorBlink::On;
        Session::spawn(options, wake).unwrap()
    }

    /// Dock'u **olan** oturum: caret'i devralacak bir yüzey var, yani
    /// [`Session::frame`] ızgaranın imlecini gizleyebilir.
    ///
    /// Ayrı yardımcı, çünkü ayrım gerçek: entegrasyonsuz bir pencerede dock
    /// yok ve orada imleci gizlemek pencereyi caret'siz bırakırdı. Sınamaların
    /// varsayılanı dock'suz ([`test_options`]).
    fn spawn_docked_session(script: &str, wake: Arc<TestWake>) -> Session {
        spawn_docked_with_cols(script, 40, wake)
    }

    /// [`spawn_docked_session`]'ın genişliği çağırandan gelen hâli.
    fn spawn_docked_with_cols(script: &str, cols: u16, wake: Arc<TestWake>) -> Session {
        let mut options = test_options(sh(script), cols);
        options.dock = true;
        Session::spawn(options, wake).unwrap()
    }

    /// `/bin/sh -c script` komutu.
    fn sh(script: &str) -> (String, Vec<String>) {
        ("/bin/sh".into(), vec!["-c".into(), script.into()])
    }

    fn spawn_with_command(command: (String, Vec<String>), wake: Arc<TestWake>) -> Session {
        spawn_with_cols(command, 40, wake)
    }

    /// Geniş grid isteyen sınamalar için (fare raporunun 223 sütunluk sınırı).
    fn spawn_with_cols(command: (String, Vec<String>), cols: u16, wake: Arc<TestWake>) -> Session {
        Session::spawn(test_options(command, cols), wake).unwrap()
    }

    /// Sınamaların ortak açılış ayarları: dizin ve ek ortam **boş**, yani
    /// çocuk sınama sürecinin dizinini ve ortamını miras alır. İkisini
    /// sınayanlar bunun üstüne yazar.
    fn test_options(command: (String, Vec<String>), cols: u16) -> SessionOptions {
        SessionOptions {
            command: Some(command),
            working_directory: None,
            home: None,
            env: HashMap::new(),
            cols,
            rows: 10,
            cell_px: (9, 18),
            terminal: TerminalOptions {
                scrollback: 100,
                osc52: Osc52::Copy,
                cursor: CaretShape::default(),
                blink: CursorBlink::default(),
            },
            theme: THEME,
            // Varsayılan **dock'suz**: sınamaların çoğu `/bin/sh` koşuyor ve
            // gerçek uygulamada o oturum dock almıyor. Caret'i devralacak bir
            // yüzeyi olduğunu iddia eden sınama bunu `spawn_docked_*` ile
            // açıkça söylüyor.
            dock: false,
            cluster: false,
            initial_input: None,
            shell_marks: false,
            tab_id: None,
        }
    }

    #[test]
    fn shell_state_stays_empty_without_marks() {
        // Tarayıcı artık akışa bağlı, ama işaret basmayan bir kabukta yuva boş
        // kalmalı: `None`'ın anlamı "besleyen yok" değil **"entegrasyon yok"**
        // ve bu sette gerçek kabukların çoğu böyle (zsh betiği phase-3'te
        // iniyor, SSH'ın öte tarafında hiç inmiyor).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf 'merhaba'; sleep 5", Arc::clone(&wake));
        wait_settled(&session);
        assert_eq!(session.shell_state(), None);
    }

    /// Bütün ızgaranın glyph'leri, satır satır — ilk girdi sınamalarının
    /// "hangi işaret basıldı" sorusu.
    fn grid_glyphs(cells: &[Cell]) -> String {
        let rows = cells.iter().map(|cell| cell.row + 1).max().unwrap_or(0);
        (0..rows)
            .map(|row| row_text(cells, row))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_initial_input_waits_for_our_first_identified_prompt() {
        // 037 Karar 6: sarmalayıcılı oturumda ilk girdi bizim ilk kimlikli
        // `A`'mızda gidiyor. Sahte kabuk üç kapı kuruyor: kimliksiz `A`'dan
        // sonra bir saniye bekleyip bir şey geldiyse `EARLY`, kimlikli
        // `A`'dan sonra satırı okuyup `GOT:`, ikinci kimlikli `A`'dan sonra
        // bir saniye bekleyip bir şey geldiyse `TWICE`. `read -t` şart: onsuz
        // erken gelen baytlar tty tamponunda bekler ve sonraki `read`
        // onları ayırt edilemez biçimde tüketirdi.
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(
            sh("stty -echo; \
                printf '\\033]133;A\\007'; \
                if read -t 1 x; then printf 'EARLY'; fi; \
                printf '\\033]133;A;bt_block=1\\007'; \
                read x; printf 'GOT:%s\\r\\n' \"$x\"; \
                printf '\\033]133;A;bt_block=2\\007'; \
                if read -t 1 y; then printf 'TWICE'; fi; \
                printf 'END'; sleep 5"),
            80,
        );
        options.initial_input = Some("echo hi".to_owned());
        options.shell_marks = true;
        let generation = |session: &Session| session.key_gen.load(Ordering::Acquire);
        let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
        // İki `read -t 1` iki saniye; pay onların üstüne.
        let mut text = String::new();
        wait_until("sahte kabuk bitmedi", Duration::from_secs(8), || {
            let mut cells = Vec::new();
            session.frame(
                |cell| cells.push(cell),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            text = grid_glyphs(&cells);
            text.contains("END")
        });
        assert!(
            !text.contains("EARLY"),
            "kimliksiz A ilk girdiyi saldı: {text:?}"
        );
        assert!(text.contains("GOT:echohi"), "ilk girdi gelmedi: {text:?}");
        assert!(
            !text.contains("TWICE"),
            "ilk girdi ikinci A'da yinelendi: {text:?}"
        );
        // Kullanıcı girdisinin yolu: nesil tam bir kez ilerledi.
        assert_eq!(generation(&session), 1);
    }

    #[test]
    fn the_initial_input_goes_at_birth_without_the_wrapper() {
        // Sarmalayıcısız oturum (zsh değil, entegrasyon kapalı): satır doğumda,
        // kabuğun typeahead'i olarak. Yankı kapalı, yani ızgaradaki `birth-42`
        // satırın **çıktısı**, yankısı değil.
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(sh("stty -echo; read x; eval \"$x\"; sleep 5"), 80);
        options.initial_input = Some("echo birth-$((6*7))".to_owned());
        let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
        wait_frame(&session, &wake, |cells| {
            grid_glyphs(cells).contains("birth-42")
        });
        assert_eq!(session.key_gen.load(Ordering::Acquire), 1);
    }

    /// Sahte kabuğun ızgarası, `until` görünene kadar (ilk girdi ve teklif
    /// sınamaları).
    fn wait_text(session: &Session, until: &str) -> String {
        let mut text = String::new();
        wait_until(until, Duration::from_secs(8), || {
            let mut cells = Vec::new();
            session.frame(
                |cell| cells.push(cell),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            text = grid_glyphs(&cells);
            text.contains(until)
        });
        text
    }

    #[test]
    fn keys_typed_before_the_initial_input_follow_it() {
        // phase-3'ün `/code-review` bulgusu: ⌘T'nin uzak sekmesinde satır
        // bizim ilk `A`'mızda gidiyor ve ondan önce yazılan tuşlar tutuluyor,
        // sonra satırın **arkasından** aynı sırayla gidiyor — `lsfirst`
        // değil, `first` sonra `ls`. Sahte kabuk `READY`'den sonra bir
        // saniye `A`'yı bekletiyor; o arada yazılan `ls` tutulmalı.
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(
            sh("stty -echo; printf 'READY'; sleep 1; \
                printf '\\033]133;A;bt_block=1\\007'; \
                read x; read y; printf 'GOT:%s|%s\\r\\n' \"$x\" \"$y\"; sleep 5"),
            80,
        );
        options.initial_input = Some("first".to_owned());
        options.shell_marks = true;
        let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
        wait_text(&session, "READY");
        session.write(b"l");
        session.write(b"s");
        // Teslim nesli bir kez daha ilerletiyor: satır ve tutulanlar gitti.
        wait_until("ilk girdi gitmedi", Duration::from_secs(5), || {
            session.key_gen.load(Ordering::Acquire) == 3
        });
        session.write(b"\r");
        let text = wait_text(&session, "GOT:");
        assert!(text.contains("GOT:first|ls"), "{text:?}");
    }

    #[test]
    fn a_return_sends_the_held_input_without_a_prompt() {
        // Kimlikli `A` hiç gelmezse (Karar 6'nın bilinen sınırı: `exec fish`,
        // rc'nin stdin sorusu) klavye ölmüyor: ⏎ biriken baytları o anda
        // gönderiyor.
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(
            sh("stty -echo; printf 'READY'; read x; printf 'GOT:%s\\r\\n' \"$x\"; sleep 5"),
            80,
        );
        options.initial_input = Some("first".to_owned());
        options.shell_marks = true;
        let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
        wait_text(&session, "READY");
        session.write(b"ls");
        assert_eq!(lock(&session.held_input).as_deref(), Some(&b"ls"[..]));
        session.write(b"\r");
        // Gönderildi ama tutma sürüyor: satır hâlâ `A`'yı bekliyor.
        assert_eq!(lock(&session.held_input).as_deref(), Some(&b""[..]));
        let text = wait_text(&session, "GOT:");
        assert!(text.contains("GOT:ls"), "{text:?}");
    }

    #[test]
    fn a_line_after_the_release_still_follows_the_initial_input() {
        // `/code-review` (phase-4): ⏎ tutulanı gönderince tutma bitseydi,
        // arkasından yazılan yarım satır ssh'ın satırının önüne yapışırdı
        // (`ls⏎pwd` → `pwdfirst`). Tutma satırın teslimine kadar sürüyor.
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(
            sh("stty -echo; printf 'READY'; read x; sleep 1; \
                printf '\\033]133;A;bt_block=1\\007'; \
                read y; read z; printf 'GOT:%s|%s|%s\\r\\n' \"$x\" \"$y\" \"$z\"; sleep 5"),
            80,
        );
        options.initial_input = Some("first".to_owned());
        options.shell_marks = true;
        let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
        wait_text(&session, "READY");
        session.write(b"ls\r");
        session.write(b"pwd");
        wait_until("ilk girdi gitmedi", Duration::from_secs(5), || {
            session.key_gen.load(Ordering::Acquire) == 3
        });
        assert_eq!(*lock(&session.held_input), None, "teslim tutmayı bitirir");
        session.write(b"\r");
        let text = wait_text(&session, "GOT:");
        assert!(text.contains("GOT:ls|first|pwd"), "{text:?}");
    }

    /// Teklif sınamalarının oturumu: sahte kabuk bir satır okuyup basıyor;
    /// defter prompt'ta, ayna canlı ve boş, dock caret'in sahibi — gerçek
    /// kabuğun yerine elle kuruluyor. Düzenleme widget'ı **bildirilmemiş**
    /// (`8133;w` yok): ⏎ ona bağlı değil.
    fn offered_session(offer: bool) -> (Session, Arc<TestWake>) {
        let wake = Arc::new(TestWake::default());
        let options = test_options(
            sh("stty -echo; read x; printf 'GOT:%s\\r\\n' \"$x\"; sleep 5"),
            80,
        );
        let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
        {
            let mut log = lock(&session.shell);
            log.apply(crate::shell::Mark::PromptStart { id: Some(1) });
            log.apply(crate::shell::Mark::PromptEnd);
            log.dock.status = DockStatus::Live;
            log.dock.insert_keymap = true;
            assert!(!log.dock_editable);
            if offer {
                log.context.reconnect = Some(crate::shell::Reconnect {
                    host: "prod".to_owned(),
                    mark: HostMark::None,
                    line: "ssh -p 2222 prod".to_owned(),
                });
            }
        }
        session.caret_in_dock.store(true, Ordering::Relaxed);
        (session, wake)
    }

    #[test]
    fn return_on_the_empty_line_sends_the_offered_line() {
        let (session, _wake) = offered_session(true);
        assert!(session.dock_key(DockKey::Enter));
        assert_eq!(
            lock(&session.shell).context.reconnect,
            None,
            "gönderim siliyor"
        );
        assert_eq!(session.key_gen.load(Ordering::Acquire), 1, "yazılmış gibi");
        let text = wait_text(&session, "GOT:");
        assert!(text.contains("GOT:ssh-p2222prod"), "{text:?}");
    }

    #[test]
    fn return_without_an_offer_is_not_the_docks() {
        // Teklif yoksa Enter bayt bayt bugünkü yolundan: tüketilmiyor ve
        // PTY'ye hiçbir şey gitmiyor (nesil kıpırdamıyor).
        let (session, _wake) = offered_session(false);
        assert!(!session.dock_key(DockKey::Enter));
        assert_eq!(session.key_gen.load(Ordering::Acquire), 0);
        // Teklif varken de kapının öteki üç koşulu.
        let (session, _wake) = offered_session(true);
        lock(&session.shell).dock.buffer = "ls".to_owned();
        assert!(!session.dock_key(DockKey::Enter), "satır dolu");
        lock(&session.shell).dock.buffer.clear();
        lock(&session.shell).dock.postdisplay = "ls -la".to_owned();
        assert!(!session.dock_key(DockKey::Enter), "öneri var");
        lock(&session.shell).dock.postdisplay.clear();
        lock(&session.shell).dock.insert_keymap = false;
        assert!(!session.dock_key(DockKey::Enter), "vicmd");
        lock(&session.shell).dock.insert_keymap = true;
        session.caret_in_dock.store(false, Ordering::Relaxed);
        assert!(!session.dock_key(DockKey::Enter), "caret ızgarada");
        session.caret_in_dock.store(true, Ordering::Relaxed);
        lock(&session.shell).dock.answers = 7;
        assert!(!session.dock_key(DockKey::Enter), "ayna bayat");
        assert_eq!(session.key_gen.load(Ordering::Acquire), 0);
        assert!(lock(&session.shell).context.reconnect.is_some());
    }

    #[test]
    fn any_input_drops_the_offer() {
        // Ömür ilk tuşta (Karar 8): yazılan harf teklifi kaldırıyor ve silip
        // boşaltmak onu geri getirmiyor.
        let (session, _wake) = offered_session(true);
        session.write(b"x");
        assert_eq!(lock(&session.shell).context.reconnect, None);
        assert!(!session.dock_key(DockKey::Enter));
    }

    #[test]
    fn the_frame_reports_one_input_row_without_a_mirror() {
        // Aynasız oturumda (entegrasyonsuz kabuk) dock'un görüntüsü yok:
        // `frame()` her tavanda **bir** giriş satırı bildiriyor; sıfır pay da
        // bir satır (dock'un giriş satırı hiç kaybolmuyor).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf 'merhaba'; sleep 5", Arc::clone(&wake));
        wait_settled(&session);
        for share in [0.0, 0.5, 1.0] {
            let cursor = session.frame(
                |_| (),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                DockBudget { share, cols: 80 },
            );
            assert_eq!(cursor.input_rows, 1, "pay {share}");
        }
    }

    #[test]
    fn marks_from_the_stream_walk_the_shell_state() {
        // Tek bir gerçek PTY turunda iki iddia birden: durum işaretleri
        // izliyor **ve** baytlar aynen geçiyor. İkincisi ızgaradan okunuyor —
        // ekranda `abcd` varsa hem tarayıcı akışı tüketmemiş hem de dizinin
        // çerçevesi `vte`'ninkiyle aynı hizada (ayrı hizalasaydı işaretin bir
        // parçası harf olarak basılırdı).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf 'ab\\033]133;A\\007\\033]133;B\\007cd\\033]133;C\\007\\033]133;D;3\\007'; \
             sleep 5",
            Arc::clone(&wake),
        );

        // Beklenen satır `wait_frame`'in ölçütü: gelmezse orada, zaman aşımının
        // mesajıyla düşer.
        wait_frame(&session, &wake, |cells| row_text(cells, 0) == "abcd");

        let finished = ShellState {
            phase: ShellPhase::Finished,
            last_exit: Some(3),
        };
        wait_until(
            "işaretler duruma düşmedi",
            Duration::from_secs(5),
            || session.shell_state() == Some(finished),
        );
    }

    /// `printf`'e girecek bir **çıpalı prompt**: OSC 133 `A` (kimlikli), OSC 8
    /// ile sarılmış `$ ` ve `B`. Betiğin (`assets/shell/zsh/bateri.zsh`)
    /// bastığı dizilerin aynısı, PS1 genişletmesi olmadan.
    fn anchored_prompt(id: u32) -> String {
        format!(
            "\\033]133;A;bt_block={id}\\007\
             \\033]8;;bateri://block/{id}\\007$ \\033]8;;\\007\
             \\033]133;B\\007"
        )
    }

    /// ZLE'nin aynasının bir karesi: `u` kolu, gövdeler base64.
    ///
    /// Betiğin (`assets/shell/zsh/bateri.zsh`) `__bateri_dock_redraw`'unun
    /// bastığı dizinin aynısı; `PREDISPLAY`, `POSTDISPLAY` ve
    /// `region_highlight` boş, çünkü bastırmanın kapısı aynanın **durumu**,
    /// içeriği değil.
    ///
    /// Keymap alanı `bWFpbg==`, yani `main`: ZLE'nin olağan hâli ve
    /// yapıştırmanın dar istisnasının koşulu
    /// ([`Session::can_be_typed`]). Onu sınayan kol kendi dizisini kuruyor.
    fn mirror(buffer_b64: &str, cursor: usize) -> String {
        format!("\\033]8133;u;{cursor};;{buffer_b64};;;bWFpbg==\\007")
    }

    /// Bitmiş bir blok + yazılmakta olan bir satır; aynanın kolu çağırandan.
    ///
    /// Üç satır doğuyor: `$ cmd1` (blok 1, başarıyla bitmiş), `out` ve
    /// `$ ls -la` (blok 2, kullanıcı hâlâ yazıyor). Bastırmanın bütün
    /// sınamaları bu ızgarayı paylaşıyor — ayrıştıkları tek şey `tail`.
    fn spawn_typing_session(tail: &str, wake: Arc<TestWake>) -> Session {
        spawn_docked_session(
            &format!(
                "printf '{}cmd1{}{}ls -la{tail}'; sleep 5",
                anchored_prompt(1),
                ran(1, 0, "out"),
                anchored_prompt(2),
            ),
            wake,
        )
    }

    /// Sayacın **iki yanı da boş**: komuta da pencerenin kenarına da değmiyor.
    ///
    /// Çakışma kuralının tek yeri burası ve ölçütü kesin. Sol yanı
    /// gevşetilirse uzun bir komutun son harfleri sessizce bir sayıya dönerdi;
    /// sağ yanı gevşetilirse sayaç pencere kenarına yapışırdı (gözlendi,
    /// kullanıcı) ve ızgaranın sol payıyla uyumsuz dururdu.
    #[test]
    fn the_counter_keeps_a_cell_on_both_sides() {
        // 80 sütun, dört harflik sayaç: son sütun (79) **boş pay**, yani
        // sayaç 75–78'i kaplıyor ve 75'ten başlıyor.
        assert_eq!(Session::counter_col(4, 10, 80), Some(75));

        // **Sol sınırın iki yakası.** 74. sütun boşluk payı.
        assert_eq!(Session::counter_col(4, 73, 80), Some(75));
        assert_eq!(
            Session::counter_col(4, 74, 80),
            None,
            "sayaç komutun harfine yapıştı"
        );

        // Hiç dolu hücresi olmayan satır (boş prompt): sayaç serbest.
        assert_eq!(Session::counter_col(4, 0, 80), Some(75));

        // Satıra sığmıyor: çıkarmaların ikisi de taşmıyor, sayaç düşüyor.
        assert_eq!(Session::counter_col(10, 0, 5), None);
        // Tam sığıyor ama sağ paya yer kalmıyor: yine düşüyor.
        assert_eq!(Session::counter_col(4, 0, 4), None);
    }

    /// Bir saniyeyi geçen komutun süresi satırın sağ ucunda ve **sönük**.
    ///
    /// Saat gerçek: betik `C` ile `D` arasında uyuyor, yani sınama
    /// `Instant`'in yakalandığı yolu da geçiyor. Sahte bir süre enjekte
    /// edilseydi `apply`'ın iki kolu (dikme ve tüketme) sınanmadan kalırdı.
    #[test]
    fn a_slow_command_shows_its_duration_at_the_right_edge() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                // Uyku **1,2 değil 2,5 saniye**: koşan sayaç ancak eşikten
                // (1 sn) sonra doğuyor, yani 1,2'de "koşuyor" penceresi ~200
                // ms kalıyordu ve yüklü bir makinede yoklama onu atlayıp ilk
                // örneği `D`'den sonra alabiliyordu — sonra da "koşan sayaç
                // ondalık gösteriyor" diye düşüyordu (`/code-review`, 013
                // kapı). 2,5'te pencere ~1,5 saniye.
                "printf '{}ls -la\\033]133;C\\007\\r\\nout\\r\\n'; sleep 2.5; \
                 printf '\\033]133;D;0;bt_block=1\\007'; sleep 5",
                anchored_prompt(1),
            ),
            Arc::clone(&wake),
        );

        // Satırın sağ ucundaki sayaç; komut satırı henüz basılmamışsa `None`
        // (oturum yeni doğdu), sayaç henüz eşiği geçmemişse boş dizgi.
        let read_counter =
            |cells: &[Cell]| Some(row_glyphs(cells, 0).strip_prefix("$ls-la")?.to_owned());
        let poll = |ready: &dyn Fn(&str) -> bool, what: &str| {
            let deadline = Instant::now() + Duration::from_secs(8);
            loop {
                assert!(Instant::now() < deadline, "{what} gelmedi");
                let mut cells = Vec::new();
                session.frame(
                    |cell| cells.push(cell),
                    |_| (),
                    &mut Blocks::default(),
                    &mut SelectionRuns::default(),
                    &mut SearchRuns::default(),
                    &mut Clusters::default(),
                    ScrollGlide::default(),
                    BUDGET,
                );
                if read_counter(&cells).is_some_and(|counter| ready(&counter)) {
                    return cells;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        };

        // **Koşarken tam saniye.** Ondalık gösterseydi saat ilk on saniye
        // boyunca saniyede on kare isterdi (`Precision`'ın doc'u).
        let running = poll(&|counter| !counter.is_empty(), "koşan sayaç");
        let counter = read_counter(&running).expect("komut satırı bozuk");
        assert!(
            counter.ends_with('s') && !counter.contains('.'),
            "koşan sayaç ondalık gösteriyor: {counter:?}"
        );

        // **Bitince ondalık.** Değer artık donmuş, yani hiçbir kareye mal
        // olmuyor ve ondalık gerçek bilgi taşıyor. Tam sayı yazılmıyor: süre
        // gerçek saatten geliyor ve `1.2`/`1.3` arası yarış olurdu — **biçimi**
        // sınayan yer `the_counter_reads_its_four_tiers`.
        let cells = poll(&|counter| counter.contains('.'), "bitmiş sayaç");
        let counter = read_counter(&cells).expect("komut satırı bozuk");
        assert!(
            counter.len() == 4 && counter.ends_with('s'),
            "bitmiş sayaç beklenen biçimde değil: {counter:?}"
        );

        let leftmost = cells
            .iter()
            .filter(|cell| cell.row == 0 && cell.col >= 30)
            .map(|cell| cell.col)
            .min();
        assert_eq!(
            leftmost,
            Some(40 - 4 - 1),
            "sayaç sağa yaslanmadı ya da kenar payını yemedi"
        );

        // Renk: bloğun **üstverisi**, komutun parçası değil.
        let dim = Theme::BATERI.dim_linear();
        assert!(
            cells
                .iter()
                .filter(|cell| cell.row == 0 && cell.col >= 30)
                .all(|cell| cell.fg == dim && cell.bg.is_none()),
            "sayaç sönük renkte ve zeminsiz olmalı"
        );
        session.shutdown();
    }

    /// Bir saniyenin altındaki komut **hiç** sayaç doğurmuyor.
    ///
    /// Talebin özü bu: her `ls`'in yanında `0.01s` yazması gürültü olurdu.
    /// Eşik kaldırılırsa burası kızarır.
    #[test]
    fn a_quick_command_shows_no_duration() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '{}ls -la{}'; sleep 5",
                anchored_prompt(1),
                ran(1, 0, "out"),
            ),
            Arc::clone(&wake),
        );

        // Bloğun bittiğini görene kadar bekle, sonra satırın tamamına bak.
        let cells = wait_frame(&session, &wake, |cells| {
            row_glyphs(cells, 1).contains("out")
        });
        assert_eq!(
            row_glyphs(&cells, 0),
            "$ls-la",
            "eşiğin altındaki komut sayaç doğurdu"
        );
        session.shutdown();
    }

    /// Süre **sonraki prompt geldikten sonra da** duruyor.
    ///
    /// Gerçek zsh'te `D` ile bir sonraki `A` aynı `precmd`'de arka arkaya
    /// basılıyor (`bateri.zsh` → `__bateri_precmd`), yani kullanıcının
    /// gördüğü hâl "bitmiş blok + yeni prompt". Öteki sınamalar `D`'de
    /// duruyordu ve bu pencereyi hiç denemiyordu.
    #[test]
    fn a_finished_duration_survives_the_next_prompt() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '{}ls -la\\033]133;C\\007\\r\\nout\\r\\n'; sleep 2.5; \
                 printf '\\033]133;D;0;bt_block=1\\007{}'; sleep 5",
                anchored_prompt(1),
                anchored_prompt(2),
            ),
            Arc::clone(&wake),
        );

        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            assert!(Instant::now() < deadline, "bitmiş süre görünmedi");
            let mut cells = Vec::new();
            session.frame(
                |cell| cells.push(cell),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            let row = row_glyphs(&cells, 0);
            if let Some(counter) = row.strip_prefix("$ls-la")
                && counter.contains('.')
            {
                assert!(counter.ends_with('s'), "bitmiş sayaç bozuk: {counter:?}");
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        session.shutdown();
    }

    /// Sayaç sığmıyorsa **saat de kurulmuyor**.
    ///
    /// Dar bir pencerede uzun bir komut sayacı hiç çizdirmiyor; saat yine de
    /// kurulsaydı pencere sonsuza kadar saniyede bir uyanır ve **hiçbir
    /// pikseli** değiştirmezdi (`/code-review`, 013 kapı). Eşiğin altı ayrı:
    /// orada sayaç henüz yok ama bir saniye dolunca **belirecek**, yani tik
    /// meşru.
    #[test]
    fn a_counter_that_does_not_fit_does_not_arm_the_clock() {
        let wake = Arc::new(TestWake::default());
        // Genişlik 40; komut metni sağ uca kadar uzuyor, yani hiçbir sayaç
        // sığmıyor.
        let session = spawn_docked_session(
            &format!(
                "printf '{}{}\\033]133;C\\007'; sleep 5",
                anchored_prompt(1),
                "x".repeat(38),
            ),
            Arc::clone(&wake),
        );

        // Eşiğin altında tik **var** (sayaç belirecek), üstünde **yok**
        // (sayaç hiç çizilmeyecek). İkinci hâli beklemek yeterli: birincisi
        // zaten `the_clock_runs_with_the_command_and_stops_with_it`'in işi.
        let deadline = Instant::now() + Duration::from_secs(6);
        loop {
            assert!(Instant::now() < deadline, "sığmayan sayaç saati söndürmedi");
            let mut cells = Vec::new();
            let cursor = session.frame(
                |cell| cells.push(cell),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            // Komut satırı gerçekten sağ uca dayanmış olmalı, yoksa sınama
            // sığmama kolunu hiç denemeden yeşil geçerdi.
            if row_glyphs(&cells, 0).len() >= 38 && cursor.next_tick.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        session.shutdown();
    }

    /// Saat komutla birlikte kuruluyor ve komutla birlikte sönüyor.
    ///
    /// **Durma koşulunun bekçisi.** `next_tick` komut bittikten sonra da dolu
    /// kalsaydı pencere sonsuza kadar saniyede bir kare isterdi ve belirti
    /// sessiz olurdu: uygulama çalışır, pil gider. Hiçbir sayaç bunu görmez —
    /// `make duman`'ın reçetesi entegrasyonsuz koştuğu için kapı da göremez.
    #[test]
    fn the_clock_runs_with_the_command_and_stops_with_it() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '{}sleep 2\\033]133;C\\007'; sleep 1.2; \
                 printf '\\033]133;D;0;bt_block=1\\007'; sleep 5",
                anchored_prompt(1),
            ),
            Arc::clone(&wake),
        );

        // Komut koşarken saat kurulu. Tik bir saniyeyi **aşmıyor**: koşan
        // sayaç tam saniye gösteriyor, yani bir sonraki değişim en geç bir
        // saniye sonra.
        let mut tick = None;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && tick.is_none() {
            tick = session
                .frame(
                    |_| (),
                    |_| (),
                    &mut Blocks::default(),
                    &mut SelectionRuns::default(),
                    &mut SearchRuns::default(),
                    &mut Clusters::default(),
                    ScrollGlide::default(),
                    BUDGET,
                )
                .next_tick;
            std::thread::sleep(Duration::from_millis(20));
        }
        let tick = tick.expect("komut koşarken saat kurulmadı");
        assert!(
            tick <= Duration::from_secs(1),
            "tik bir saniyeyi aştı: {tick:?}"
        );

        // `D` gelince saat sönüyor.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(Instant::now() < deadline, "komut bitti, saat sönmedi");
            if session
                .frame(
                    |_| (),
                    |_| (),
                    &mut Blocks::default(),
                    &mut SelectionRuns::default(),
                    &mut SearchRuns::default(),
                    &mut Clusters::default(),
                    ScrollGlide::default(),
                    BUDGET,
                )
                .next_tick
                .is_none()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        session.shutdown();
    }

    /// Tek bir satırın mürekkebi, sütun sırasında.
    ///
    /// [`glyph_text`] bastırma sınamalarına yetmiyor: boşluk hücresi sink'e
    /// hiç girmediği için `"ls -la"` ızgarada dursa bile metinde `"ls-la"`
    /// görünür ve `contains` **sessizce** yanlış cevap verir. Bastırmanın
    /// iddiası zaten satır bazlı — "bu satırda hücre var mı, yok mu".
    fn row_glyphs(cells: &[Cell], row: u16) -> String {
        let mut row_cells: Vec<&Cell> = cells.iter().filter(|cell| cell.row == row).collect();
        row_cells.sort_by_key(|cell| cell.col);
        row_cells.iter().filter_map(|cell| cell.ch).collect()
    }

    /// Ayna beklenen duruma **ve** kabuk `Input`'a gelene kadar bekler.
    ///
    /// İkisi birden, çünkü bastırmanın yüklemi ikisinin birleşimi
    /// (`ShellLog::suppressed_input`); yalnız birini beklemek sınamayı
    /// yarışa açardı.
    ///
    /// **`Idle` beklemek tek başına hiçbir şey sormaz:** o, [`DockStatus`]'ün
    /// `#[default]`'u, yani hiç ayna gelmemiş bir oturumda da doğru. Bırakma
    /// kolunu sınayan yer önce `Live`'ı geçmek zorunda — yoksa `u` yükünü
    /// büsbütün düşüren bir regresyon sınamayı **yeşil** bırakırdı.
    fn wait_mirror(session: &Session, status: DockStatus) {
        let mut state = DockState::default();
        wait_until(
            "ayna beklenen duruma gelmedi",
            Duration::from_secs(5),
            || {
                session.dock_state(&mut state);
                state.status == status
                    && session.shell_state().map(|s| s.phase) == Some(ShellPhase::Input)
            },
        );
        wait_settled(session);
    }

    #[test]
    fn an_empty_prompt_keeps_its_caret_in_the_dock_alone() {
        // **Gözlenen kusur** (kullanıcı, 012 phase-7): boşta bekleyen bir
        // prompt'ta ızgara dock'un caret'inin yanında **ikinci bir imleç**
        // çiziyordu. Sebebi zincirin en başındaydı: sıfır genişlikli `PS1`
        // hiçbir hücre yazmıyor ve kullanıcı da henüz bir şey yazmadığı için
        // ZLE yazmıyor — yani çıpayı **taşıyan hücre yok**, `suppress_from`
        // `None` kalıyor ve hücre kapısıyla birlikte imleç kapısı da
        // açılmıyordu.
        //
        // Çare kapıları ayırmak: hücreler hangi satırların atlanacağını
        // bilmek zorunda (çıpa), caret'in yeri ise bir satır sorusu değil.
        // Bu yüzden prompt burada **hücresiz** kuruluyor — `anchored_prompt`
        // `$ ` bastığı için kusuru hiç göstermezdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '\\033]133;A;bt_block=1\\007\
                 \\033]8;;bateri://block/1\\007\\033]133;B\\007{}'; sleep 5",
                // Boş tampon, imleç başta: kullanıcının hiçbir şey yazmadığı an.
                mirror("", 0),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);

        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(
            !cursor.visible,
            "boş prompt'ta ızgara ikinci bir imleç çizdi: {cursor:?}"
        );
        session.shutdown();
    }

    #[test]
    fn the_first_keystroke_does_not_move_the_grid() {
        // **Gözlenen kusur** (kullanıcı, 012 phase-8): `ls` çıktısı duran bir
        // pencerede dock'a bir harf yazınca ızgaranın tamamı bir satır aşağı,
        // silince bir satır yukarı oynuyordu. Kullanıcının teşhisi birebir
        // doğruydu: satır çizilmiyor ama **yer kaplıyordu** — `visibility:
        // hidden`, oysa `display: none` gerek.
        //
        // Sebep iki kapının ayrışmasıydı. Boş prompt'ta sıfır genişlikli `PS1`
        // hiçbir hücre yazmıyor, yani çıpayı taşıyan hücre yok ve bastırma
        // çalışmıyor; doluluk sayısı da imlecin satırını sayıyordu. İlk tuşta
        // hücre doğuyor, çıpa beliriyor, bastırma başlıyor ve doluluk bir
        // satır düşüyordu.
        //
        // Ölçülen şey **fark**, mutlak sayı değil: doluluk ilk tuşta
        // değişmemeli.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                // Çıktı, sonra **hücresiz** prompt (çıpa açık kalıyor), sonra
                // boş ayna. İkinci `printf` kullanıcının ilk tuşu: harf
                // ızgaraya düşüyor (çıpayı o taşıyacak) ve ayna onu bildiriyor.
                "printf 'out\\r\\n\\033]133;A;bt_block=1\\007\
                 \\033]8;;bateri://block/1\\007\\033]133;B\\007{}'; sleep 1; \
                 printf 'l{}'; sleep 5",
                mirror("", 0),
                mirror("bA==", 1),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        let empty = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );

        wait_until("ilk tuş aynaya düşmedi", Duration::from_secs(3), || {
            let mut dock = DockState::default();
            session.dock_state(&mut dock);
            dock.status == DockStatus::Live && dock.buffer == "l"
        });
        let typed = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );

        assert_eq!(
            empty.content_rows, typed.content_rows,
            "ilk tuşta ızgara oynadı: {empty:?} → {typed:?}"
        );
        // Ve oynamamasının sebebi satırın **hiç** yer kaplamaması: yalnız
        // çıktının satırı sayılıyor, giriş satırı değil.
        assert_eq!(typed.content_rows, 1, "giriş satırı yer kapladı: {typed:?}");
        session.shutdown();
    }

    #[test]
    fn the_grid_keeps_no_cursor_before_the_dock_comes_alive() {
        // **Gözlenen kusur** (kullanıcı, 012 phase-8): pencere açılırken —
        // zsh'in rc'si koşarken, henüz hiçbir işaret gelmemişken — caret
        // ızgaradaydı ve prompt gelince dock'a **sıçrıyordu**. Aynı pencere her
        // komuttan sonra da açılıyor (`Finished`, içinde `precmd`'in `git`
        // fork'u var), yani kusur açılışa özgü değil, her komutta tekrarlıyor.
        //
        // Ayna burada `Idle` ve kabuk hiç konuşmadı: eski kapı ("bastırılacak
        // bir blok var mı") bu hâlde hiçbir şey sormuyordu.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session("printf 'hazir'; sleep 5", Arc::clone(&wake));
        wait_until("çıktı gelmedi", Duration::from_secs(2), || {
            let mut cells = Vec::new();
            session.frame(
                |cell| cells.push(cell),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            !cells.is_empty()
        });

        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(
            !cursor.visible,
            "dock canlanmadan ızgarada imleç var: {cursor:?}"
        );
        session.shutdown();
    }

    #[test]
    fn a_running_command_takes_the_cursor_back_to_the_grid() {
        // Değişmezin öteki ucu ve gizlemenin dizginı: komut koşarken satırın
        // sahibi ızgara. `cat`'in beklediği girdi, `ssh`'ın parola istemi ve
        // `read`'in satırı orada yaşıyor — caret dock'ta kalsaydı kullanıcı
        // yazdığı yeri göremezdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '{}{}\\033]133;C\\007'; sleep 5",
                anchored_prompt(1),
                mirror("", 0),
            ),
            Arc::clone(&wake),
        );
        wait_until("komut koşmadı", Duration::from_secs(2), || {
            session.shell_state().map(|s| s.phase) == Some(ShellPhase::Running)
        });

        // **Devir tutma süresi kadar gecikiyor** (015 R1.1): yüklem `Running`
        // der demez değil, `HANDOVER_HOLD` dolunca ızgaraya geçiyor. Bekleme
        // bu sınamayı aynı zamanda "tutma gerçekten doluyor"un bekçisi
        // yapıyor — süresiz tutma burayı kızdırır.
        wait_until("caret ızgaraya dönmedi", Duration::from_secs(2), || {
            session
                .frame(
                    |_| (),
                    |_| (),
                    &mut Blocks::default(),
                    &mut SelectionRuns::default(),
                    &mut SearchRuns::default(),
                    &mut Clusters::default(),
                    ScrollGlide::default(),
                    BUDGET,
                )
                .visible
        });
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(cursor.visible, "koşan komutta ızgara imleçsiz: {cursor:?}");
        session.shutdown();
    }

    /// **Tutma gerçekten bir kare istiyor.** Bağlayan tek satırın bekçisi.
    ///
    /// `sooner` ile `hold_left` ayrı ayrı çivili, ama ikisini `Cursor`'a
    /// bağlayan satır silinse hiçbiri kızarmazdı: entegrasyon sınamaları
    /// `frame()`'i döngüde çağırdığı için saati **atlıyorlar**. Bedeli
    /// koşan komutu olmayan kolda görünür — `CORRECT`'in `[nyae]`'i,
    /// `zle -M`, `line-finish` ile Enter arası — çünkü orada başka bir son
    /// tarih yok: saat hiç kurulmaz, link uyur ve caret bir sonraki tuşa
    /// kadar dock'ta kalırdı.
    ///
    /// Defter **doğrudan** sürülüyor: tutma penceresi 150 ms ve PTY'nin
    /// zamanlamasına bırakılsaydı sınama yüklü bir makinede açığa düşerdi.
    #[test]
    fn a_held_handover_asks_for_a_frame() {
        let wake = Arc::new(TestWake::default());
        // Betik sessiz: okuyucu thread araya işaret sokmasın.
        let session = spawn_docked_session("sleep 5", Arc::clone(&wake));

        // Prompt → giriş → `line-finish`: ham cevap `Grid`, tutma başlıyor.
        {
            use crate::shell::{DockEvent, Mark, ScanEvent};
            let mut log = lock(&session.shell);
            log.apply_scan(ScanEvent::Mark(Mark::PromptStart { id: None }));
            log.apply_scan(ScanEvent::Mark(Mark::PromptEnd));
            log.apply_scan(ScanEvent::Dock(DockEvent::End));
        }
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(cursor.caret_in_dock, "tutma devri gizlemeliydi: {cursor:?}");
        let tick = cursor.next_tick.expect("tutma kare istemedi");
        assert!(
            tick <= crate::shell::HANDOVER_HOLD && !tick.is_zero(),
            "tik tutmanın kalanı olmalı, oysa {tick:?}"
        );

        // Komut bitti: ham cevap `Dock`'a döndü, saat sönmeli.
        {
            use crate::shell::{Mark, ScanEvent};
            let mut log = lock(&session.shell);
            log.apply_scan(ScanEvent::Mark(Mark::CommandEnd {
                exit: Some(0),
                id: None,
            }));
        }
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(
            cursor.next_tick, None,
            "bekleyen tutma yokken saat sönmeli: {cursor:?}"
        );
        session.shutdown();
    }

    #[test]
    fn a_session_without_a_dock_always_keeps_its_own_cursor() {
        // **Gizlemenin ön koşulu devralacak bir yüzeyin olması.** Dock'suz
        // pencerede caret'i alacak kimse yok ve gizlemek pencereyi caret'siz
        // bırakırdı. Kurulum bilerek [`an_empty_prompt_keeps_its_caret_in_the_dock_alone`]
        // ile aynı — tek fark dock'un yokluğu, yani sınanan şey tam olarak o.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!(
                "printf '\\033]133;A;bt_block=1\\007\
                 \\033]8;;bateri://block/1\\007\\033]133;B\\007{}'; sleep 5",
                mirror("", 0),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);

        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(
            cursor.visible,
            "dock'suz pencere caret'siz kaldı: {cursor:?}"
        );
        session.shutdown();
    }

    #[test]
    fn the_input_line_leaves_the_grid_while_the_dock_shows_it() {
        // Bastırmanın kendisi (R3.1): ayna canlıyken kullanıcının yazdığı
        // satır ızgaraya **hiç** düşmüyor, çünkü onu dock çiziyor. phase-3'ün
        // bıraktığı çift görüntü burada kapanıyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session(&mirror("bHMgLWxh", 6), Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Live);

        let mut cells = Vec::new();
        let cursor = session.frame(
            |cell| cells.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(row_glyphs(&cells, 0), "$cmd1", "geçmiş kayboldu");
        assert_eq!(row_glyphs(&cells, 1), "out", "çıktı kayboldu");
        // Giriş satırı **tamamen** boş: prompt'un `$`'ı da gitti, çünkü aralık
        // çıpa satırından başlıyor ve `$ ` o satırda. Bilinçli ara durum —
        // prompt'u phase-5 devralıyor.
        assert_eq!(row_glyphs(&cells, 2), "", "giriş satırı ızgarada");
        // **İmleç de çizilmiyor**: caret dock'ta ve ikisi birden çizilseydi
        // kullanıcı iki caret görürdü.
        assert!(!cursor.visible, "{cursor:?}");
        // **Doluluk bastırılan satırı saymıyor**: iki satır çizildi, üçüncüsü
        // bastırıldı. Sayılsaydı dock ile içerik arasında boş bir şerit
        // kalırdı.
        assert_eq!(cursor.content_rows, 2, "{cursor:?}");
        session.shutdown();
    }

    #[test]
    fn a_suppressed_input_line_keeps_the_block_stripe() {
        // **R3.2'nin bekçisi.** Naif bir bastırma (satırı döngüde tümden
        // atlamak) çıpayı da öldürür ve bitmiş bloğun şeridi kaybolurdu;
        // belirti sessiz olurdu, çünkü şeridi çizen taraf "kimlik yok" ile
        // "blok yok"u ayırt etmiyor. Kapı bu yüzden çıpa taramasından
        // **sonra**.
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session(&mirror("bHMgLWxh", 6), Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Live);

        let mut blocks = Blocks::default();
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        let rows: Vec<u16> = blocks.as_slice().iter().map(|block| block.row).collect();
        assert_eq!(rows, [0], "bastırma blok şeridini düşürdü: {blocks:?}");
        session.shutdown();
    }

    #[test]
    fn the_grid_keeps_the_input_line_when_the_mirror_cannot_show_it() {
        // Gösteremediğimiz satır ızgarada **kalmak zorunda** (R1.2): aşımda
        // dock boş ve bastırma da yapılsaydı kullanıcı yazdığını hiçbir yerde
        // görmezdi. `Idle` ile `Unavailable`'ı ayıran varyantın tükettiği yer
        // burası — ve ZLE'nin beş değişkenin dışında çizdiği kipler
        // (`CORRECT`'in `[nyae]`'i, `read` istemi) aynı kapıdan `Idle` ile
        // geçiyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session("\\033]8133;o\\007", Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Unavailable(DockFault::Overflow));

        let mut cells = Vec::new();
        let cursor = session.frame(
            |cell| cells.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(
            row_glyphs(&cells, 2),
            "$ls-la",
            "ayna gösteremiyorken ızgara da bastırıldı"
        );
        assert!(cursor.visible, "{cursor:?}");
        assert_eq!(cursor.content_rows, 3, "{cursor:?}");
        session.shutdown();
    }

    #[test]
    fn a_stale_mirror_leaves_the_input_line_in_the_grid() {
        // **Tazelik kapısı.** Bastırma aynanın güncel olduğuna güveniyor;
        // güvenmenin bedeli, ayna bayatlarsa kullanıcının yazdığını **hiçbir
        // yerde** görmemesi. Ölçülmüş örneği `bracketed-paste-magic`:
        // yapıştırılan metni `zle -U` ile kuyruğa geri basıyor, ZLE typeahead
        // varken redisplay'i atlıyor ve ayna bir sonraki tuşa kadar
        // güncellenmiyor.
        //
        // Burada aynı hâl elle kuruluyor: ızgarada `ls -la`, aynada onun bir
        // önceki hâli (`ls`). Kapı uyuşmazlığı görüp bastırmayı bırakmalı.
        //
        // **Ve ayna cevapsız** (025): kullanıcının son girdisi aynadan sonra
        // gitmiş. Nesil elle ilerletiliyor (`screen_clears`'ın emsali), çünkü
        // gerçek bir yazım `/bin/sh`'in yankısıyla ızgarayı da değiştirirdi;
        // ölçülen hâl de tam bu — yapıştırma gitti, ayna gelmedi. Nesil
        // ilerlemeseydi ayna cevap sayılır ve kapı içeriğe hiç bakmazdı.
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session(&mirror("bHM", 2), Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Live);
        session.key_gen.fetch_add(1, Ordering::Release);

        let mut cells = Vec::new();
        let cursor = session.frame(
            |cell| cells.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(
            row_glyphs(&cells, 2),
            "$ls-la",
            "ayna bayatken ızgara da bastırıldı: kullanıcı yazdığını hiçbir yerde görmez"
        );
        assert!(cursor.visible, "{cursor:?}");
        // **Ve dock caret'i almıyor.** Set kapısının (`/code-review`) bulgusu
        // tam buradaydı: sahiplik ikinci kez `dock::render`'ın içinde
        // hesaplanıyordu ve o çağrı tazelik kapısını **bilmiyordu**, yani bu
        // karede ızgara imlecini gösterirken dock da caret'ini veriyordu.
        // Çizen taraf dock'u tercih ettiği için (`link.rs`) kullanıcının
        // yazdığı taze satır caret'siz, caret de bayat metnin üstünde
        // kalıyordu — bastırmanın kurtardığı satırı devir geri kaybediyordu.
        assert!(!cursor.caret_in_dock, "{cursor:?}");
        let dock = session.dock(
            DockCols {
                grid: 40,
                context: 40,
            },
            1,
            &mut DockState::default(),
            &mut DockContext::default(),
            cursor.caret_in_dock,
            &mut Vec::new(),
            &mut Clusters::default(),
            |_| (),
            |_| (),
        );
        assert!(
            dock.caret.is_none(),
            "bayat aynada iki caret: ızgara gösteriyor, dock da sahipleniyor"
        );
        session.shutdown();
    }

    /// **Birleştirici taşıyan satır bastırılıyor** — tazelik kapısının iki
    /// tarafı artık aynı birimi okuyor.
    ///
    /// Kusur kullanıcıda görüldü ve ölçüldü (2026-09-22): `❤️` yazınca giriş
    /// satırı dock'tan ızgaraya fırlıyordu. Sebebi birim ayrışması — ayna
    /// son `char`'ı veriyor (`U+FE0F`), ızgara hücrenin `c`'sini
    /// (`U+2764`; birleştirici alacritty'de `CellExtra`'da) — yani kapı
    /// **hiçbir zaman** eşleşemiyor ve her tuşta "bayat" diyordu.
    ///
    /// Bekçi kusurun tam tersini soruyor: caret dock'ta, ızgara imleci gizli.
    #[test]
    fn a_combining_mark_does_not_make_the_mirror_look_stale() {
        let wake = Arc::new(TestWake::default());
        // Izgarada `❤️`, aynada aynısı: `U+2764 U+FE0F`, iki karakter.
        let session = spawn_docked_session(
            &format!(
                "printf '{}\\342\\235\\244\\357\\270\\217{}'; sleep 5",
                anchored_prompt(1),
                mirror("4p2k77iP", 2),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        // **Cevapsız ayna** (025): nesil ilerletilmeseydi zamansal kısa devre
        // kapıyı içerik karşılaştırmasına hiç düşürmez ve bu bekçi bir şey
        // sınamazdı. İçerik kapısı yapıştırmadan sonra hâlâ tek hakem.
        session.key_gen.fetch_add(1, Ordering::Release);
        let mut cells = Vec::new();
        let cursor = session.frame(
            |cell| cells.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(
            cursor.caret_in_dock,
            "birleştirici kapıyı düşürdü: satır ızgaraya fırladı ({cursor:?})"
        );
        assert!(
            !cursor.visible,
            "bastırma koştuysa ızgara imleci gizli olmalı: {cursor:?}"
        );
        session.shutdown();
    }

    /// **Kullanıcının bildirdiği kusur** (2026-09-22, 025): `🥰` yazınca caret
    /// dock'tan ızgaraya sıçrıyordu.
    ///
    /// zsh U+1F970'i kendi basılabilirlik tablosunda bulamıyor ve ızgaraya
    /// ters videolu `<0001f970>` yazıyor (`zsh -f`, saf pty ile ölçüldü);
    /// ayna ise ham emojiyi taşıyor. İçerik kapısı `'>'` ile `'🥰'`'yi hiçbir
    /// zaman eşleştiremez. Ama ayna kullanıcının son girdisine **cevap**
    /// olarak geldi, yani tazedir: bastırma açık, caret dock'ta.
    #[test]
    fn a_transformed_char_keeps_the_caret_in_the_dock() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '{}\\033[7m<0001f970>\\033[27m{}'; sleep 5",
                anchored_prompt(1),
                // `🥰` = F0 9F A5 B0.
                mirror("8J+lsA", 1),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        let mut cells = Vec::new();
        let cursor = session.frame(
            |cell| cells.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(
            cursor.caret_in_dock,
            "zsh'in dönüştürdüğü karakter kapıyı düşürdü: caret ızgaraya sıçradı ({cursor:?})"
        );
        assert!(!cursor.visible, "{cursor:?}");
        assert_eq!(row_glyphs(&cells, 0), "", "giriş satırı ızgarada kaldı");
        session.shutdown();
    }

    /// **Dock'un çizmediği kontrol karakteri satırı ızgarada tutuyor** —
    /// konumu ne olursa olsun (025, [`DockStatus::Control`]).
    ///
    /// ZLE `Ctrl-V Ctrl-A`'yı ızgarada okunur bir `^A` diye basıyor; dock ise
    /// o sütunu boş bırakırdı. Bu kol gelmeden önce karar tazelik kapısının
    /// tesadüfüne kalıyordu: `^A` sondayken iki taraf uyuşmuyor ve satır
    /// ızgarada kalıyordu, **ortadayken** ikisi de `'o'` diyor, satır dock'a
    /// gidiyor ve `^A` hiçbir yerde görünmüyordu. İki konum da sınanıyor.
    #[test]
    fn a_control_char_the_dock_cannot_draw_stays_in_the_grid() {
        // `\x01foo` ve `foo\x01`, base64.
        for (grid, b64) in [("^Afoo", "AWZvbw"), ("foo^A", "Zm9vAQ")] {
            let wake = Arc::new(TestWake::default());
            let session = spawn_docked_session(
                &format!(
                    "printf '{}{grid}{}'; sleep 5",
                    anchored_prompt(1),
                    mirror(b64, 4),
                ),
                Arc::clone(&wake),
            );
            wait_mirror(&session, DockStatus::Control);
            let mut cells = Vec::new();
            let cursor = session.frame(
                |cell| cells.push(cell),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            assert_eq!(
                row_glyphs(&cells, 0),
                format!("${grid}"),
                "dock'un gösteremediği satır ızgarada da gizlendi ({grid})"
            );
            assert!(!cursor.caret_in_dock, "{grid}: {cursor:?}");
            assert!(cursor.visible, "{grid}: {cursor:?}");
            session.shutdown();
        }
    }

    /// **Bilinen sınır, adıyla** (025, `discussion.md` → Karar 2): damga
    /// aynanın **ne zaman** geldiğini söylüyor, hangi girdiye cevap olduğunu
    /// değil.
    ///
    /// Bir tuşun aynası yoldayken hemen bir yapıştırma giderse ayna
    /// yapıştırmanın nesliyle damgalanır: ızgara yapıştırmayı almıştır, ayna
    /// hâlâ tuşun hâlidir ve kapı onu cevap sayar — bir tuş boyunca bastırma
    /// eski içerikle açık kalır. Terminalin "ZLE bu girdiyi işledi mi"
    /// sorusunu bilme yolu yok.
    ///
    /// **Damgalama elle taklit ediliyor**: pty'siz düzenekte ZLE yok, yani
    /// "ayna yoldayken" hâlini üretmek uygulamanın kendisi olurdu. Taklit
    /// şu: ızgara ile ayna ayrışmış (`ls -la` / `ls`) ama ayna **güncel**
    /// nesille damgalı. Sınır bir gün kapanırsa bekçi kırmızıya döner ve bu
    /// cümle güncellenir.
    #[test]
    fn a_key_answered_by_an_older_mirror_is_a_known_limit() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session(&mirror("bHM", 2), Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Live);
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(
            cursor.caret_in_dock,
            "sınır kapanmış görünüyor — bekçiyi ve discussion.md'yi güncelle ({cursor:?})"
        );
        session.shutdown();
    }

    #[test]
    fn an_unanswered_blank_mirror_below_the_anchor_is_stale() {
        // **Kapının kör noktası** (kullanıcı, 2026-09-21): çok satırlı
        // yapıştırmada `bracketed-paste-magic` aynayı bir tuş boyunca boş
        // bırakıyor, zsh de bracketed yapıştırmanın **son satır sonunu
        // tamponda tutuyor** (saf PTY ile ölçüldü: `BUFFER='echo a\necho b\n'`),
        // yani ızgaranın imleci yapıştırmanın bıraktığı boş satırda duruyor.
        // İki `None` eşleşip "taze" diyordu: bastırma açılıyor, caret metnin
        // yanında değil dock'un prompt işaretinin yanında kalıyordu.
        //
        // Ayıran veri çıpanın satırı: karakteri olmayan bir ayna imleci
        // prompt'un satırından aşağı itemez.
        //
        // **Adında "cevapsız" var** (025): gerçekten gelmiş boş bir ayna
        // (`zle -I`'dan sonraki redisplay) çıpanın altında olsa da artık
        // taze ve bu doğru — satır boş, dock'un caret'i doğru yerde. Kör
        // nokta yalnız ayna **gelmemişken** bir kör nokta; nesil o yüzden
        // aşağıda elle ilerletiliyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '{}cmd1{}{}echo a\r\necho b\r\n{}'; sleep 5",
                anchored_prompt(1),
                ran(1, 0, "out"),
                anchored_prompt(2),
                // Boş ayna: yapıştırma ZLE'ye ulaştı ama `line-pre-redraw`
                // henüz koşmadı.
                mirror("", 0),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        session.key_gen.fetch_add(1, Ordering::Release);

        let mut cells = Vec::new();
        let cursor = session.frame(
            |cell| cells.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(
            !cursor.caret_in_dock,
            "boş ayna boş satıra uydu: caret metnin yanında değil dock'ta ({cursor:?})"
        );
        assert!(cursor.visible, "{cursor:?}");
        // Yapıştırılan satırlar ızgarada duruyor — gösterilen yer ile
        // caret'in yeri artık aynı.
        assert_eq!(row_glyphs(&cells, 2), "$echoa");
        assert_eq!(row_glyphs(&cells, 3), "echob");
        session.shutdown();
    }

    #[test]
    fn a_blank_mirror_on_the_anchor_row_is_fresh() {
        // Yukarıdakinin karşı ucu ve **kapının asıl işi**: boş prompt'ta ayna
        // da satır da boş, ama satır gerçekten dock'un. Kapı bu kolu
        // kaybetseydi 012 phase-8'in kusuru geri gelirdi — satır gizli ama
        // yer kaplıyor.
        //
        // Prompt burada **gerçek `PS1`'in şekliyle** kuruluyor: iki sıfır
        // genişlikli işaret artı iki gerçek boşluk (`dock::TEXT_COL`), yani
        // çıpayı taşıyan ama mürekkebi olmayan iki hücre. `anchored_prompt`'ın
        // `$ `'ı kapının **öteki** yarısına takılırdı — mürekkebi var, aynanın
        // yok (`docs/YOL-HARITASI.md`: kapı prompt hücrelerini de sayıyor).
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '{}cmd1{}\\033]133;A;bt_block=2\\007\
                 \\033]8;;bateri://block/2\\007  \\033]8;;\\007\
                 \\033]133;B\\007{}'; sleep 5",
                anchored_prompt(1),
                ran(1, 0, "out"),
                mirror("", 0),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        // **Cevapsız ayna** (025): nesil ilerletilmeseydi zamansal kısa devre
        // kapıyı içerik karşılaştırmasına hiç düşürmez ve bu bekçi bir şey
        // sınamazdı. İçerik kapısı yapıştırmadan sonra hâlâ tek hakem.
        session.key_gen.fetch_add(1, Ordering::Release);

        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(
            cursor.caret_in_dock,
            "boş prompt'ta caret dock'un olmalı: {cursor:?}"
        );
        assert!(!cursor.visible, "{cursor:?}");
        session.shutdown();
    }

    #[test]
    fn a_wrapped_input_line_is_suppressed_below_the_cursor_row_too() {
        // **`/code-review`'un orta bulgusu.** Aralığın altı eskiden imlecin
        // satırıydı; ZLE caret'i tamponun içinde serbestçe gezdirdiği için
        // sarmalı bir satırda Ctrl-A (ya da yukarı ok) imleci ilk satıra
        // alınca kuyruk aşağıdaki satırlarda **kalıyordu** — dock bütün
        // tamponu, ızgara da kuyruğu gösteriyordu. Kalıcı çift görüntü, yani
        // tam da bu phase'in kapatmaya geldiği şey.
        //
        // Ölçüt satır 4: bastırma girişin son satırında **durmak** zorunda,
        // yoksa tamamlama listesini de yutardı.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_with_cols(
            &format!(
                // 20 sütun: `$ ` + 30 karakter iki satıra sarıyor (2 ve 3).
                // Sonra dördüncü satıra ZLE'nin girişin dışında çizdiği bir
                // şey (tamamlama listesi emsali), sonra imleç girişin **ilk**
                // satırına, metnin başına (`\033[2A` iki yukarı, `\033[3G`
                // üçüncü sütun).
                "printf '{}cmd1{}{}{}\\r\\nlist-item\\033[2A\\033[3G{}'; sleep 5",
                anchored_prompt(1),
                ran(1, 0, "out"),
                anchored_prompt(2),
                "abcdefghijklmnopqrstuvwxyz0123",
                mirror("YWJjZGVmZ2hpamtsbW5vcHFyc3R1dnd4eXowMTIz", 0),
            ),
            20,
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);

        let mut cells = Vec::new();
        let cursor = session.frame(
            |cell| cells.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(row_glyphs(&cells, 2), "", "girişin ilk satırı ızgarada");
        assert_eq!(row_glyphs(&cells, 3), "", "sarmalı kuyruk ızgarada sızdı");
        assert_eq!(
            row_glyphs(&cells, 4),
            "list-item",
            "bastırma girişin son satırında durmadı"
        );
        assert_eq!(cursor.content_rows, 5, "{cursor:?}");
        session.shutdown();
    }

    #[test]
    fn the_grid_takes_the_input_line_back_when_zle_lets_go() {
        // **Bırakma kolunun bekçisi (R3.3).** ZLE satırı bıraktığında
        // (`line-finish` → `e` → `Idle`) bastırma kalkıyor ve yazılan şey
        // ızgaranın sıradan bir satırı oluyor. Gerçek hayatta bu yol iki kez
        // geçiliyor: Enter'da ve `CORRECT`'in `[nyae]`'inde — ikincisinde
        // `line-finish` istemden **önce** koşuyor (probe'la doğrulandı), yani
        // ızgara devralmış oluyor ve ayrı bir tetiğe gerek kalmıyor.
        //
        // **Geçiş sınanıyor, son hâl değil.** `Idle` [`DockStatus`]'ün
        // varsayılanı, yani ona *doğrudan* varan bir sınama `u` yükünü
        // büsbütün düşüren bir regresyonda bile yeşil kalırdı. Ayna bu yüzden
        // önce `Live` oluyor, bastırma orada doğrulanıyor, `e` ondan **sonra**
        // geliyor: gerçek hayattaki sıranın (yaz → Enter) ta kendisi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf '{}cmd1{}{}ls -la{}'; sleep 1; printf '\\033]8133;e\\007'; sleep 5",
                anchored_prompt(1),
                ran(1, 0, "out"),
                anchored_prompt(2),
                mirror("bHMgLWxh", 6),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        let mut live = Vec::new();
        session.frame(
            |cell| live.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(row_glyphs(&live, 2), "", "ayna canlıyken bastırma yok");

        // **`line-finish` safha `Input`'tayken tutuluyor** (032 Karar 11):
        // ayna `HANDOVER_HOLD` boyunca `Live` kalıyor ve tutmayı kare yolu
        // çözüyor (`ShellLog::expire_end`), yani `Idle`'ı görmek için kare
        // koşmak gerekiyor. Caret'in tutması aynı andan sayıyor; `content_rows`
        // caret'e bağlı, yani ölçüm devir gerçekleştikten sonra alınmalı.
        let mut state = DockState::default();
        wait_until(
            "ayna tutmadan sonra Idle olmadı",
            Duration::from_secs(5),
            || {
                session.frame(
                    |_| (),
                    |_| (),
                    &mut Blocks::default(),
                    &mut SelectionRuns::default(),
                    &mut SearchRuns::default(),
                    &mut Clusters::default(),
                    ScrollGlide::default(),
                    BUDGET,
                );
                session.dock_state(&mut state);
                state.status == DockStatus::Idle
            },
        );
        wait_until("caret ızgaraya dönmedi", Duration::from_secs(2), || {
            session
                .frame(
                    |_| (),
                    |_| (),
                    &mut Blocks::default(),
                    &mut SelectionRuns::default(),
                    &mut SearchRuns::default(),
                    &mut Clusters::default(),
                    ScrollGlide::default(),
                    BUDGET,
                )
                .visible
        });
        let mut cells = Vec::new();
        let cursor = session.frame(
            |cell| cells.push(cell),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(
            row_glyphs(&cells, 2),
            "$ls-la",
            "ZLE satırı bıraktı ama ızgara hâlâ bastırılıyor"
        );
        assert!(cursor.visible, "{cursor:?}");
        assert_eq!(cursor.content_rows, 3, "{cursor:?}");
        session.shutdown();
    }

    /// Komutun koşup bitmesi: `C` (kullanıcının Enter'ı), bir satır çıktı,
    /// kodlu `D`. İki satır sonu de gerçek akıştan: biri Enter'ın, biri
    /// çıktının.
    fn ran(id: u32, code: i32, out: &str) -> String {
        format!("\\033]133;C\\007\\r\\n{out}\\r\\n\\033]133;D;{code};bt_block={id}\\007")
    }

    /// Beklenen şekle varan ilk karenin bloklarını verir.
    ///
    /// [`wait_frame`] ile aynı örüntü ve aynı gerekçe: ölçüt sınamanın
    /// kendisinde, zaman aşımı burada. Tampon döngü boyunca **aynı**, yani
    /// yeniden kullanımı da sınanıyor — bir karenin artığı ötekine sızsaydı
    /// ölçütler tutmazdı.
    fn wait_blocks(
        session: &Session,
        wake: &TestWake,
        ready: impl Fn(&[Block]) -> bool,
    ) -> Vec<Block> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut blocks = Blocks::default();
        let mut seen = 0;
        loop {
            assert!(
                Instant::now() < deadline,
                "beklenen bloklar gelmedi: {:?}",
                blocks.as_slice()
            );
            seen = wake.wait_wakes(seen + 1, Duration::from_millis(500));
            if blocks_if_damaged(session, &mut blocks) && ready(blocks.as_slice()) {
                return blocks.as_slice().to_vec();
            }
        }
    }

    /// İşaretlerin satırı ve rengi, karşılaştırması okunur olsun diye ikili
    /// demet.
    fn marks(blocks: &[Block]) -> Vec<(u16, LinearRgba)> {
        blocks.iter().map(|b| (b.row, b.stripe)).collect()
    }

    #[test]
    fn each_command_row_gets_its_own_mark() {
        // Üç prompt, çünkü sonuncusu **çizilmiyor**: `A` geldi, `D` gelmedi ve
        // kabuk `Input`'ta, yani `Pending` ama koşmuyor. İşaret komutun kendi
        // satırında ve **yalnız** orada: aradaki çıktı satırları (1 ve 3) hiç
        // işaret almıyor — bölge boyama bu tasarımda yok (bkz. `Block`).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!(
                "printf '{}cmd1{}{}cmd2{}{}'; sleep 5",
                anchored_prompt(1),
                ran(1, 0, "out1"),
                anchored_prompt(2),
                ran(2, 1, "out2"),
                anchored_prompt(3),
            ),
            Arc::clone(&wake),
        );
        // Satırlar: 0 `$ cmd1`, 1 `out1`, 2 `$ cmd2`, 3 `out2`, 4 `$ `.
        // İşaretler 0 ve 2'de; 1, 3 ve 4 boş — 4 boş çünkü üçüncü blok
        // `Pending` ama koşmuyor.
        let blocks = wait_blocks(&session, &wake, |blocks| blocks.len() == 2);
        assert_eq!(
            marks(&blocks),
            [(0, THEME.success_linear()), (2, THEME.error_linear())]
        );
        session.shutdown();
    }

    #[test]
    fn a_scrolled_off_command_leaves_no_mark() {
        // İlk prompt geçmişe kayıyor (10 satırlık pencere, 12 satır çıktı).
        // Eskiden bu hâlde üstteki bölge birinci bloğun rengine boyanıyordu;
        // bugün işaret komutun satırında ve o satır ekranda değil, yani
        // **işaret de yok**. Kaybolan bir bilgi değil: ekranda o komut yok.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!(
                "printf '{}cmd1\\033]133;C\\007'; \
                 for i in 1 2 3 4 5 6 7 8 9 10 11 12; do printf 'out\\r\\n'; done; \
                 printf '\\033]133;D;0;bt_block=1\\007{}'; sleep 5",
                anchored_prompt(1),
                anchored_prompt(2),
            ),
            Arc::clone(&wake),
        );
        // İkinci prompt son satırda ve `Pending` (koşmuyor), yani o da
        // çizilmiyor: pencere tamamen işaretsiz.
        wait_until("kabuk `Input`'a geçmedi", Duration::from_secs(5), || {
            session.shell_state().map(|s| s.phase) == Some(ShellPhase::Input)
        });
        wait_settled(&session);
        let mut blocks = Blocks::default();
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(blocks.as_slice(), []);
        session.shutdown();
    }

    #[test]
    fn output_rows_never_carry_a_mark() {
        // **Kullanıcı kararı (010 teslim): işaret komutun, çıktının değil.**
        // Uzun bir çıktının ortasına kaydırıldığında pencerede komut satırı
        // yok, yani işaret de yok — ve bu tutarlı: aynı satır her kaydırma
        // konumunda aynı görünüyor. Eski tasarımda bu pencere ya boyanıyor ya
        // boyanmıyordu (hangisi olduğu kaydırma konumuna bağlıydı).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!(
                "printf '{}cmd1\\033]133;C\\007'; \
                 i=0; while [ $i -lt 25 ]; do printf 'out\\r\\n'; i=$((i + 1)); done; \
                 printf '\\033]133;D;0;bt_block=1\\007{}'; sleep 5",
                anchored_prompt(1),
                anchored_prompt(2),
            ),
            Arc::clone(&wake),
        );
        wait_until("kabuk `Input`'a geçmedi", Duration::from_secs(5), || {
            session.shell_state().map(|s| s.phase) == Some(ShellPhase::Input)
        });
        wait_settled(&session);

        // On satır yukarı: pencere tamamen `out` satırlarına düşüyor.
        let mut blocks = Blocks::default();
        session.term.lock().scroll_display(Scroll::Delta(10));
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(blocks.as_slice(), [], "çıktı satırı işaret aldı");

        // Dibe dönünce komutun satırı yine görünmüyor (27 satırlık içerikte
        // 10 satırlık pencere), ama ikinci prompt görünüyor ve `Pending`
        // olduğu için çizilmiyor: yine boş.
        session.term.lock().scroll_display(Scroll::Bottom);
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(blocks.as_slice(), []);
        session.shutdown();
    }

    #[test]
    fn a_running_command_is_marked_on_its_own_row() {
        // Koşan komut `accent` alıyor ve **tek satır**: eskiden bu blok
        // pencerenin dibine kadar uzuyordu ve çıktısız bir komutta boş ekrana
        // uzun bir bar çiziyordu (kullanıcı bulgusu, 010 teslim).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!(
                "printf '{}sleep\\033]133;C\\007'; sleep 5",
                anchored_prompt(1)
            ),
            Arc::clone(&wake),
        );
        let blocks = wait_blocks(&session, &wake, |blocks| blocks.len() == 1);
        assert_eq!(marks(&blocks), [(0, THEME.accent_linear())]);
        session.shutdown();
    }

    /// Sarmalayıcının kaynak dizini (`assets/shell/zsh`).
    fn wrapper_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell/zsh")
    }

    /// Kullanıcının hiçbir başlangıç dosyasının bulunmadığı bir `HOME`.
    ///
    /// Sarmalayıcı kullanıcının dosyalarını **okuyor** (`__bateri_begin`),
    /// yani gerçek bir ev dizini sınamayı makinede kurulu eklentilere
    /// bağlardı: çalışan bir sınama başka birinin `.zshrc`'siyle kırmızıya
    /// düşerdi.
    fn empty_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bateri-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("geçici ev dizini kurulamadı");
        dir
    }

    /// Ayna beklenen hâle gelene kadar bekler; tampon çağıranın.
    ///
    /// [`wait_frame`] ile aynı örüntü: ölçüt sınamada, zaman aşımı burada.
    /// Tampon döngü boyunca **aynı**, yani yeniden kullanımı da sınanıyor.
    fn wait_dock(session: &Session, dock: &mut DockState, ready: impl Fn(&DockState) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            session.dock_state(dock);
            if ready(dock) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "ayna beklenen hâle gelmedi: {dock:?}"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn the_mirror_follows_a_real_zle_session() {
        // Betiğin kendi sınamaları (`shell.rs`) kodlayıcıyı ZLE olmadan
        // koşturuyor; burada sınanan **tesisat**: kanca gerçekten bağlanıyor
        // mu, gerçek bir satır düzenlemesinde koşuyor mu, bastığı dizi
        // ızgaranın akışından geçip aynaya varıyor mu.
        let home = empty_home("dock");
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(("/bin/zsh".into(), vec!["-i".into()]), 40);
        options
            .env
            .insert("HOME".into(), home.display().to_string());
        options
            .env
            .insert("ZDOTDIR".into(), wrapper_dir().display().to_string());
        let session = Session::spawn(options, wake).unwrap();

        let mut dock = DockState::default();

        // İlk ayna prompt çizilir çizilmez geliyor: `line-pre-redraw` boş
        // tamponda da koşuyor. Phase-4'ün bastırma kararı buna dayanacak —
        // gelmeseydi prompt anında dock ölü kalırdı.
        wait_dock(&session, &mut dock, |dock| {
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });

        // Komut BİLEREK uzun sürüyor: biten satırın ardından gelen yeni
        // prompt aynayı hemen yeniden açar ve aşağıdaki `Idle` penceresi
        // ölçülemeyecek kadar dar kalırdı. Sınama uykunun bitmesini
        // beklemiyor, yalnız pencereyi genişletiyor.
        let typed = "echo çığır; sleep 3";
        session.write(typed.as_bytes());
        wait_dock(&session, &mut dock, |dock| dock.buffer == typed);
        // İmleç de akıyor ve **karakter** sayıyor: çok baytlı harfler bayt
        // sayılsaydı caret satırın sonunu aşardı.
        assert_eq!(
            dock.cursor,
            dock.predisplay.chars().count() + typed.chars().count()
        );

        // Enter: `line-finish` aynayı kapatıyor.
        session.write(b"\r");
        wait_dock(&session, &mut dock, |dock| {
            dock.status == DockStatus::Idle && dock.buffer.is_empty()
        });

        session.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Kareyi `ready` doğru dönene kadar koşar; ızgaranın hücreleri ve
    /// imleç. Canlı zsh sınamalarının ortak bekleyişi — ayna ile ızgara iki
    /// ayrı akış ve ikisinin de oturması gerekiyor.
    fn frame_until(
        session: &Session,
        budget: DockBudget,
        ready: impl Fn(&[Cell], &Cursor) -> bool,
    ) -> (Vec<Cell>, Cursor) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut cells = Vec::new();
            let cursor = session.frame(
                |cell| cells.push(cell),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                ScrollGlide::default(),
                budget,
            );
            if ready(&cells, &cursor) {
                return (cells, cursor);
            }
            assert!(
                Instant::now() < deadline,
                "kare beklenen hâle gelmedi: {cursor:?}, ızgara {:?}",
                (0..cursor.rows)
                    .map(|row| row_glyphs(&cells, row))
                    .collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Izgaranın herhangi bir satırı `needle`'ı taşıyor mu (boşluksuz
    /// glyph dizisi — [`row_glyphs`] boşluk hücresi üretmiyor).
    fn grid_shows(cells: &[Cell], rows: u16, needle: &str) -> bool {
        (0..rows).any(|row| row_glyphs(cells, row).contains(needle))
    }

    /// **Çok satırlı yapıştırma dock'ta** — uçtan uca (032 phase-4).
    ///
    /// Gerçek zsh bracketed yapıştırmayı alıyor, `BUFFER`'ı satır sonlarıyla
    /// birlikte tutuyor (ölçüldü, saf PTY: `BUFFER='echo a\necho b\n'`), ZLE
    /// kancası onu aynaya basıyor ve ayna `Live`: satırlar dock'ta, ızgarada
    /// bastırılmış, caret dock'ta. 032'ye kadar bu ayna `Multiline`'dı ve
    /// satır da caret de ızgarada kalıyordu. Son satır sonu tamponda, yani
    /// ızgaranın imleci boş bir satırda — tazelik kapısının son mürekkebi o
    /// satırdan sorulduğu için (`the_last_ink_comes_from_the_last_row_of_the_display`)
    /// cevapsız bir karede de taze.
    #[test]
    fn a_bracketed_multiline_paste_stays_in_the_dock() {
        let (session, home) = spawn_editing_zsh("dock-paste");
        let mut dock = DockState::default();
        session.write(b"\x1b[200~echo a\necho b\n\x1b[201~");
        wait_dock(&session, &mut dock, |dock| {
            dock.status == DockStatus::Live && dock.buffer == "echo a\necho b\n"
        });
        let budget = DockBudget {
            share: 0.5,
            cols: 60,
        };
        let (_, cursor) = frame_until(&session, budget, |cells, cursor| {
            cursor.caret_in_dock
                && !grid_shows(cells, cursor.rows, "echoa")
                && !grid_shows(cells, cursor.rows, "echob")
        });
        assert_eq!(cursor.input_rows, 3, "iki satır ve sondaki boş satır");

        // **İçerik kapısı tek başına da taze diyor**: nesli ileri alıp
        // zamansal soruyu düşürünce karar ızgaranın son satırı (boş) ile
        // aynanın son satırına (boş) kalıyor.
        session.key_gen.fetch_add(1, Ordering::AcqRel);
        let (_, cursor) = frame_until(&session, budget, |_, cursor| cursor.caret_in_dock);
        assert_eq!(cursor.input_rows, 3);

        session.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    /// **`bracketed-paste-magic`'in arkasındaki tazeleme** (032 R5), gerçek
    /// ZLE'de. Widget yükü `zle -U` ile kuyruğa geri basıyor ve typeahead
    /// varken redisplay atlanıyor; `r` komutu kuyruğun **arkasında** geldiği
    /// için widget aynayı yapıştırmanın sonucuyla basıyor ve ayna son
    /// girdinin cevabı oluyor — kapı bir tuş beklemeden yeniden açık.
    #[test]
    fn a_multiline_paste_is_answered_under_bracketed_paste_magic() {
        let (session, home) = spawn_editing_zsh("dock-paste-magic");
        let mut dock = DockState::default();
        session.write(
            b"autoload -Uz bracketed-paste-magic; zle -N bracketed-paste bracketed-paste-magic\r",
        );
        wait_dock(&session, &mut dock, |dock| {
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });
        wait_until(
            "kapı yeni prompt'ta açılmadı",
            Duration::from_secs(10),
            || session.can_edit_dock(),
        );
        session.paste(b"echo a\necho b".to_vec());
        wait_until(
            "yapıştırmanın aynası cevap vermedi",
            Duration::from_secs(5),
            || {
                session.dock_state(&mut dock);
                dock.buffer == "echo a\necho b" && session.can_edit_dock()
            },
        );
        session.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    /// **`for` döngüsü dock'ta** — `PREBUFFER` uçtan uca (032 phase-4).
    ///
    /// `for i in 1 2; do` ⏎: zsh satırı kabul ediyor, `PS2`'yi basıyor ve
    /// yeni satırın aynası `PREBUFFER`'ı taşıyor. Dock iki satır çiziyor,
    /// ızgarada `for` satırı da `PS2` satırı da bastırılmış (taban çıpanın
    /// satırı), caret dock'ta. ⏎ anındaki `line-finish` tutuluyor (Karar 11):
    /// kabul edilen satır arada ızgaraya dönmüyor — tutmanın zamanlaması
    /// burada değil `shell.rs`'in bekçisinde, canlı sınamada yarışa açık.
    #[test]
    fn a_for_loop_keeps_its_rows_in_the_dock() {
        let (session, home) = spawn_editing_zsh("dock-for");
        let mut dock = DockState::default();
        let budget = DockBudget {
            share: 0.5,
            cols: 60,
        };
        session.write(b"for i in 1 2; do");
        wait_dock(&session, &mut dock, |dock| {
            dock.buffer == "for i in 1 2; do"
        });
        session.write(b"\r");
        wait_dock(&session, &mut dock, |dock| {
            dock.status == DockStatus::Live
                && dock.prebuffer == "for i in 1 2; do\n"
                && dock.buffer.is_empty()
        });
        session.write(b"echo $i");
        wait_dock(&session, &mut dock, |dock| dock.buffer == "echo $i");
        let (cells, cursor) = frame_until(&session, budget, |cells, cursor| {
            cursor.caret_in_dock && !grid_shows(cells, cursor.rows, "echo$i")
        });
        assert_eq!(cursor.input_rows, 2, "PREBUFFER ve BUFFER");
        assert!(
            !grid_shows(&cells, cursor.rows, "fori"),
            "kabul edilen satır ızgarada bastırılmalı"
        );

        // Dock'un çizimi: `for` satırı üstte, `echo $i` altında, caret onun
        // sonunda.
        let mut into = DockState::default();
        let mut dock_cells = Vec::new();
        let drawn = session.dock(
            DockCols {
                grid: 60,
                context: 60,
            },
            cursor.input_rows,
            &mut into,
            &mut DockContext::default(),
            cursor.caret_in_dock,
            &mut Vec::new(),
            &mut Clusters::default(),
            |cell| dock_cells.push(cell),
            |_| (),
        );
        assert_eq!(
            row_glyphs(&dock_cells, 0),
            "fori in 1 2; do".replace(' ', "")
        );
        assert_eq!(row_glyphs(&dock_cells, 1), "echo$i");
        assert_eq!(
            drawn.caret,
            Some(crate::dock::DockCaret {
                col: dock::TEXT_COL + 7,
                row: 1
            })
        );

        session.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    /// Gerçek zsh + sarmalayıcı: 031'in düzenleme sınamalarının kurulumu.
    /// Dönen oturum boş bir prompt'ta, ayna `Live` ve yetenek görülmüş.
    fn spawn_editing_zsh(name: &str) -> (Session, PathBuf) {
        let home = empty_home(name);
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(("/bin/zsh".into(), vec!["-i".into()]), 60);
        options.dock = true;
        options
            .env
            .insert("HOME".into(), home.display().to_string());
        options
            .env
            .insert("ZDOTDIR".into(), wrapper_dir().display().to_string());
        let session = Session::spawn(options, wake).unwrap();
        wait_until(
            "kapı prompt'ta açılmadı",
            Duration::from_secs(10),
            || session.can_edit_dock(),
        );
        (session, home)
    }

    /// Satır `buffer`'a varana ve **kapı yeniden açılana** kadar bekler — bir
    /// sonraki komut ancak aynanın cevabından sonra gidebilir.
    fn wait_edited(session: &Session, dock: &mut DockState, buffer: &str) {
        wait_dock(session, dock, |dock| dock.buffer == buffer);
        wait_until("kapı yeniden açılmadı", Duration::from_secs(5), || {
            session.can_edit_dock()
        });
        session.dock_state(dock);
    }

    /// `BUFFER`'ın `[start, end)` aralığını seçer (fareyi atlayarak; isabet
    /// testinin kendi sınamaları var).
    fn select_dock(session: &Session, start: usize, end: usize) {
        let mut log = lock(&session.shell);
        let point = |index| DockPoint {
            index,
            half: CellHalf::Left,
        };
        log.dock_selection = Some(DockSelection::new(
            SelectKind::Simple,
            point(start),
            point(end),
            &log.dock.buffer,
            false,
        ));
    }

    /// Caret'in `BUFFER`'daki yeri, aynadan.
    fn caret_of(dock: &DockState) -> usize {
        buffer_caret(dock)
    }

    #[test]
    fn the_widget_deletes_moves_and_types_over_a_selection_in_zle() {
        // **Uçtan uca, emacs keymap'i** (031 phase-5): terminalin komutu
        // gerçek ZLE'de `BUFFER`'ı değiştiriyor ve cevabı aynaya dönüyor.
        let (session, home) = spawn_editing_zsh("dock-edit");
        let mut dock = DockState::default();
        session.write("hello wörld".as_bytes());
        wait_edited(&session, &mut dock, "hello wörld");

        // ⌫ seçimi siler, caret seçimin başına.
        select_dock(&session, 0, 6);
        assert!(session.dock_key(DockKey::Backspace));
        wait_edited(&session, &mut dock, "wörld");
        assert_eq!(caret_of(&dock), 0);

        // Tıkla-caret: boş `Simple` seçim → `d;N;N;L`.
        select_dock(&session, 3, 3);
        session.dock_click();
        wait_dock(&session, &mut dock, |dock| caret_of(dock) == 3);

        // Seçimin üstüne yazma: silme + harf olağan yoldan.
        wait_edited(&session, &mut dock, "wörld");
        select_dock(&session, 1, 3);
        session.type_text("Z");
        wait_edited(&session, &mut dock, "wZld");
        assert_eq!(caret_of(&dock), 2);

        // → seçimi sonuna daraltır, ← başına.
        select_dock(&session, 1, 3);
        assert!(session.dock_key(DockKey::Right));
        wait_dock(&session, &mut dock, |dock| caret_of(dock) == 3);
        wait_edited(&session, &mut dock, "wZld");
        select_dock(&session, 1, 3);
        assert!(session.dock_key(DockKey::Left));
        wait_dock(&session, &mut dock, |dock| caret_of(dock) == 1);

        // **`L` tutmazsa no-op**: bayat bir aynaya göre gönderilmiş komut
        // satıra dokunmuyor, ardından yazılan harf olağan yerine gidiyor.
        wait_edited(&session, &mut dock, "wZld");
        session.write(b"\x1b[8133~d;0;4;99\x07");
        session.write(b"Y");
        wait_edited(&session, &mut dock, "wYZld");

        session.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_widget_is_bound_in_viins_and_in_a_keymap_linked_to_main() {
        // Bağlama her `line-init`'te `main`/`emacs`/`viins`'e: `bindkey -v`
        // sonrası ve kendi keymap'ini `main`'e bağlayan kullanıcıda da komut
        // tutuyor (031 → Muhakeme, bağlamasız dizi satırı bozuyordu).
        for setup in [
            "bindkey -v",
            "bindkey -N mymap emacs; bindkey -A mymap main",
        ] {
            let (session, home) = spawn_editing_zsh("dock-keymap");
            let mut dock = DockState::default();
            session.write(format!("{setup}\r").as_bytes());
            // Yeni prompt: `line-finish` kapıyı kapattı, `line-init` açtı.
            wait_dock(&session, &mut dock, |dock| {
                dock.status == DockStatus::Live && dock.buffer.is_empty()
            });
            wait_until(
                "kapı yeni prompt'ta açılmadı",
                Duration::from_secs(5),
                || session.can_edit_dock(),
            );
            session.write("abc dëf".as_bytes());
            wait_edited(&session, &mut dock, "abc dëf");
            select_dock(&session, 0, 4);
            assert!(session.dock_key(DockKey::Delete), "{setup}");
            wait_edited(&session, &mut dock, "dëf");
            assert_eq!(caret_of(&dock), 0, "{setup}");
            session.shutdown();
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn shift_return_inserts_a_newline_without_running_the_line() {
        // ⇧⏎ yapıştırmanın yolundan tek bir `\n`: satır çalışmıyor, caret
        // yeni satırın başında ve yazılan harf oraya gidiyor — `viins`'te de
        // (`\e\r` orada satırı kabul ederdi).
        for setup in ["bindkey -e", "bindkey -v"] {
            let (session, home) = spawn_editing_zsh("dock-newline");
            let mut dock = DockState::default();
            session.write(format!("{setup}\r").as_bytes());
            wait_dock(&session, &mut dock, |dock| {
                dock.status == DockStatus::Live && dock.buffer.is_empty()
            });
            wait_until(
                "kapı yeni prompt'ta açılmadı",
                Duration::from_secs(5),
                || session.can_edit_dock(),
            );
            session.write(b"echo a");
            wait_edited(&session, &mut dock, "echo a");
            assert!(session.dock_key(DockKey::NewLine), "{setup}");
            wait_edited(&session, &mut dock, "echo a\n");
            session.write(b"b");
            wait_edited(&session, &mut dock, "echo a\nb");
            assert_eq!(caret_of(&dock), "echo a\nb".chars().count(), "{setup}");
            session.shutdown();
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn the_gate_closes_when_the_line_is_finished() {
        // Yetenek prompt'un: Enter'dan sonra komut koşarken kapı kapalı, yeni
        // prompt'un `line-init`'i yeniden açıyor.
        let (session, home) = spawn_editing_zsh("dock-gate");
        let mut dock = DockState::default();
        session.write(b"sleep 2\r");
        // Girdi nesli kapıyı zaten kapatıyor (ayna henüz cevap vermedi);
        // sınanan şey yeteneğin kendisinin `line-finish`'le gitmesi.
        wait_dock(&session, &mut dock, |dock| dock.status == DockStatus::Idle);
        assert!(!lock(&session.shell).dock_editable, "{dock:?}");
        assert!(!session.can_edit_dock());
        wait_until(
            "kapı yeni prompt'ta açılmadı",
            Duration::from_secs(10),
            || session.can_edit_dock(),
        );
        session.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_continuation_row_does_not_take_the_block_mark() {
        // 032 phase-5'te görüldü: prompt'un bağlantısı `preexec`'e kadar açık,
        // yani çok satırlı komutun **bütün** satırları çıpayı taşıyor. İlk
        // satır geçmişe kayınca işaret devam satırına, ızgaranın tepesine
        // oturuyor ve orada kalıyordu. Kural: üstteki satır (geçmiş dahil)
        // aynı kimliği taşıyorsa satır komutun başı değil.
        //
        // Sahne: çıpalı üç satırlık komut (`$ a`, `b`, `c`), yedi satır
        // çıktı; on satırlık ızgarada `$ a` geçmişe düşüyor, `b` tepede.
        // Aynı betiğin ikinci bloğu karşı sınama: komutu bütünüyle görünen
        // blok işaretini ilk satırında alıyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033]133;A;bt_block=1\\007\\033]8;;bateri://block/1\\007\
             $ a\\r\\nb\\r\\nc\\033]8;;\\007\\033]133;B\\007\\033]133;C\\007\\r\\n'; \
             seq 1 7; printf '\\033]133;D;0;bt_block=1\\007'; sleep 5",
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| {
            row_text(cells, 0) == "b" && row_text(cells, 8) == "7"
        });
        wait_settled(&session);
        let mut blocks = Blocks::default();
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(blocks.as_slice(), [], "işaret devam satırına oturdu");

        // Pencere bir satır yukarı: komutun başı görünüyor, işaret onun
        // satırında ve yalnız orada.
        session.term.lock().scroll_display(Scroll::Delta(1));
        let mut blocks = Blocks::default();
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        let rows: Vec<u16> = blocks.as_slice().iter().map(|block| block.row).collect();
        assert_eq!(rows, [0], "{:?}", blocks.as_slice());
        session.shutdown();
    }

    #[test]
    fn a_window_without_anchors_is_empty_at_the_prompt() {
        // Aynı pencere, `Input` safhasında: hangi bloğa ait olduğunu söyleyen
        // tek şey çıpa ve o görünmüyor. **Bilinmeyen çizilmez.**
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033]133;A;bt_block=1\\007\\033]133;C\\007out\\r\\n\
             \\033]133;D;0;bt_block=1\\007\\033]133;A;bt_block=2\\007\\033]133;B\\007'; \
             sleep 5",
            Arc::clone(&wake),
        );
        wait_until("kabuk `Input`'a geçmedi", Duration::from_secs(5), || {
            session.shell_state().map(|s| s.phase) == Some(ShellPhase::Input)
        });
        wait_settled(&session);
        let mut blocks = Blocks::default();
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(blocks.as_slice(), []);
        session.shutdown();
    }

    #[test]
    fn the_alternate_screen_has_no_blocks() {
        // vim'in tamponunda ne prompt var ne komut; oradaki satırlar hiçbir
        // bloğa ait değil. Geri düşüş kolu da kapalı olmalı: kabuk `Running`
        // ve çıpa yok, yani kapı yalnız çıpa toplamayı kesseydi tam ekran
        // boyanırdı.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!(
                "printf '{}vim\\033]133;C\\007\\033[?1049hduzenleyici\\r\\n'; sleep 5",
                anchored_prompt(1),
            ),
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| row_text(cells, 0) == "duzenleyici");
        wait_settled(&session);
        let mut blocks = Blocks::default();
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert_eq!(blocks.as_slice(), []);
        session.shutdown();
    }

    #[test]
    fn session_is_send_and_sync() {
        // Renderer `Arc<Session>`'ı ana thread'de, okuyucu thread'i PTY'de
        // kullanır; bu iki sınır derleme zamanında bağlanmalı.
        fn require_send_sync<T: Send + Sync>() {}
        require_send_sync::<Session>();
    }

    /// `count` **arka planlı** hücre taşıyan ilk kareyi bekler; dönen liste
    /// karenin tamamıdır.
    ///
    /// Ölçüt sink çağrısı sayısı **değil**, arka planlı hücre sayısı: sink
    /// artık mürekkebi olan hücreleri de veriyor ve PTY'nin yankısı
    /// (`read x` betiğinde yazılan "ab") sayıyı sessizce şişirirdi. Arka plan
    /// sayısı bu sınamaların gerçekten baktığı şey ve betiklerden türüyor.
    fn wait_cells(session: &Session, wake: &TestWake, count: usize) -> Vec<Cell> {
        wait_frame(session, wake, |cells| backgrounds(cells).count() == count)
    }

    /// [`wait_frame`]'in ikizi ama **imleci** döndürür: sınır kaydının
    /// hücre olmayan yarısını soran sınamalar için.
    fn wait_cursor(session: &Session, wake: &TestWake, ready: impl Fn(&[Cell]) -> bool) -> Cursor {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = 0;
        loop {
            assert!(Instant::now() < deadline, "beklenen kare gelmedi");
            seen = wake.wait_wakes(seen + 1, Duration::from_millis(500));
            let mut cells = Vec::new();
            if let Some(cursor) = frame_if_damaged(session, |c| cells.push(c))
                && ready(&cells)
            {
                return cursor;
            }
        }
    }

    /// `ready` "bu kare beklediğim kare" diyene kadar bekler.
    ///
    /// Ölçütün parametre olmasının sebebi PTY okumasının bölünebilmesi:
    /// hücrelerin bir kısmı bir karede, kalanı sonrakinde gelebilir. Ölçüt
    /// karenin **son** parçasına bağlanmazsa sınama erken dönüp eksik bir
    /// kareyi doğrular.
    fn wait_frame(
        session: &Session,
        wake: &TestWake,
        ready: impl Fn(&[Cell]) -> bool,
    ) -> Vec<Cell> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut cells = Vec::new();
        let mut seen = 0;
        loop {
            assert!(
                Instant::now() < deadline,
                "beklenen çıktı gelmedi: {cells:?}"
            );
            seen = wake.wait_wakes(seen + 1, Duration::from_millis(500));
            cells.clear();
            if frame_if_damaged(session, |c| cells.push(c)).is_some() && ready(&cells) {
                return cells;
            }
        }
    }

    /// Karenin arka plan boyayan hücreleri — `Frame`'in `bg_count`'unun
    /// saydığı küme.
    fn backgrounds(cells: &[Cell]) -> impl Iterator<Item = &Cell> {
        cells.iter().filter(|c| c.bg.is_some())
    }

    /// Karenin mürekkebini tek dizide verir: `od` hex dökümü buradan okunur.
    fn glyph_text(cells: &[Cell]) -> String {
        cells.iter().filter_map(|c| c.ch).collect()
    }

    #[test]
    fn smoke_shell_yields_background_cells() {
        let wake = Arc::new(TestWake::default());
        // `make duman`'ın koştuğu betiğin sekiz **arka planlı** hücre
        // verdiğini doğrulayan yer burası.
        let session = spawn_smoke(Arc::clone(&wake));

        let cells = wait_cells(&session, &wake, 8);
        let backs: Vec<_> = backgrounds(&cells).collect();

        // " bateri " → sekiz hücre, hepsi ilk satırda ve kırmızı.
        assert!(backs.iter().all(|c| c.row == 0), "{backs:?}");
        assert_eq!(
            backs.iter().map(|c| c.col).collect::<Vec<_>>(),
            (0..8).collect::<Vec<_>>()
        );
        let red = Some(color::linear_rgba(THEME.default(1)));
        assert!(backs.iter().all(|c| c.bg == red), "{backs:?}");
    }

    #[test]
    fn smoke_shell_yields_six_glyphs() {
        // `make duman`'ın `glif=G` kapısının bağlandığı sayı. Ayrı bir
        // sınama, çünkü ayrı bir iddia: `hucre=8` sink'in hücre ürettiğini,
        // `glif=6` **mürekkep** ürettiğini söylüyor. Sekizi de arka planlı
        // olduğu için tek bir sayı ikisini birden kanıtlayamazdı — boşluklu
        // iki uç tam da farkın yaşadığı yer.
        let wake = Arc::new(TestWake::default());
        let session = spawn_smoke(Arc::clone(&wake));

        let cells = wait_cells(&session, &wake, 8);
        assert_eq!(glyph_text(&cells), "bateri", "{cells:?}");
    }

    #[test]
    fn smoke_shell_distinguishes_five_styles() {
        // Duman kapısının **göremediği** yarı: `kare/hucre/glif` üçlüsü beş
        // alt çizgi stilini birbirinden ayırdığımızı hiç sormuyor, çünkü
        // yedi kural hücresi ne arka plan ne mürekkep üretiyor. Reçetenin
        // stil dizisini bağlayan tek yer burası.
        let wake = Arc::new(TestWake::default());
        let session = spawn_smoke(Arc::clone(&wake));

        // Ölçüt arka plan sayısı **değil** karenin tamamı: yedi kural
        // hücresi sekiz arka planlı hücreden sonra geliyor ve PTY okuması
        // ikisinin arasında bölünebilir — `wait_cells(.., 8)` o karede dönüp
        // kuralları hiç görmezdi. Eksik kalan kare zaman aşımıyla, fazla
        // hücre veren kare aşağıdaki `assert_eq!` ile düşer.
        let cells = wait_frame(&session, &wake, |cells| cells.len() >= 15);
        // Sekiz arka planlı + yedi yalnız-kurallı.
        assert_eq!(cells.len(), 15, "{cells:?}");
        assert!(cells.iter().all(|c| c.row == 0), "{cells:?}");

        // İlk sekiz: `\033[41;1;4m` → kalın + düz alt çizgi. `Face` yolunun
        // betikte gerçekten olduğunun kanıtı.
        assert!(
            cells[..8]
                .iter()
                .all(|c| c.bold && !c.italic && c.underline == UnderlineStyle::Single),
            "{cells:?}"
        );

        // Yedisi de mürekkepsiz ve arka plansız: `hucre=8` ve `glif=6` bit
        // bit duruyor, yani bu hücreler yalnız kural yan tümcesinden geçti.
        let rules = &cells[8..];
        assert!(
            rules.iter().all(|c| c.bg.is_none() && c.ch.is_none()),
            "{cells:?}"
        );

        use UnderlineStyle::{Curl, Dashed, Dotted, Double, None as NoLine, Single};
        assert_eq!(
            rules.iter().map(|c| c.underline).collect::<Vec<_>>(),
            vec![Single, Double, Curl, Dotted, Dashed, NoLine, Curl],
            "{cells:?}"
        );
        // Üstü çizili yalnız altıncıda ve alt çizgisi yok: `\033[0;9m`
        // sıfırlamayı da sınıyor.
        assert_eq!(
            rules.iter().map(|c| c.strikeout).collect::<Vec<_>>(),
            vec![false, false, false, false, false, true, false],
            "{cells:?}"
        );
        // SGR 58 yalnız sonuncuda; ondan öncekiler ön plana düşüyor.
        let red = Some(color::linear_rgba(THEME.default(196)));
        assert_eq!(
            rules.iter().map(|c| c.underline_color).collect::<Vec<_>>(),
            vec![None, None, None, None, None, None, red],
            "{cells:?}"
        );
    }

    #[test]
    fn load_shell_carries_duration() {
        // Süre komuta girmezse ölçüm koşusu ya hiç bitmez ya deadline'dan
        // önce biter; ikisi de sessiz. `4242` betiğin sabit metninde geçmeyen
        // bir sayı, yani eşleşme gerçekten parametreden geliyor.
        let (program, args) = load_shell(4242);
        assert_eq!(program, "/bin/sh");
        assert!(args.iter().any(|a| a.contains("4242")), "{args:?}");
        // Ayrılık sözleşmesinin sınanabilir hâli: `smoke_shell` `hucre=8
        // glif=6 kural=15` sayılarının tek sahibi ve ikinci yük onun
        // gövdesine girmez. İkisi bir gün aynı betiğe düşerse burası kırmızı.
        assert_ne!(args, smoke_shell().1);
    }

    #[test]
    fn undercurl_text_yields_curl() {
        // Eşleme zincirini tek bir `contains(UNDERLINE)`'a indiren
        // mutasyonun kırmızı düştüğü yer; gerekçesi `frame()`'in eşleme
        // yorumunda. Odaklı sınama: duman reçetesi yeniden yazılsa da ayakta
        // kalır ve hatanın yerini nokta atışı gösterir.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033[41;4:3mx\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        let cells = wait_cells(&session, &wake, 1);
        assert_eq!(cells[0].underline, UnderlineStyle::Curl, "{cells:?}");
    }

    #[test]
    fn strikeout_only_cell_carries_no_underline_color() {
        // SGR 58'in adı "alt çizgi rengi" ve SGR'de üstü çizilinin ayrı bir
        // rengi yok. Renk kapısı `ruled`'a bağlansaydı bu hücre rengi taşır
        // ve onu okuyan çizici üstü çiziliyi kırmızıya boyardı — duman
        // reçetesinde 9 ile 58 aynı hücrede buluşmadığı için oradan
        // görülmezdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033[41;9;58;5;196mX\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        let cells = wait_cells(&session, &wake, 1);
        assert!(cells[0].strikeout, "{cells:?}");
        assert_eq!(cells[0].underline, UnderlineStyle::None, "{cells:?}");
        assert_eq!(cells[0].underline_color, None, "{cells:?}");
    }

    #[test]
    fn underlined_space_cell_passes_sink() {
        // Altı çizili boşluk: `bg: None`, `ch: None` — ama bir kural var,
        // yani atlama koşulundan **geçmeli**. `ch` `Some(' ')` olsaydı atlas
        // hiçbir piksel boyamayan bir yuva harcardı; istenen bir çizgi.
        let wake = Arc::new(TestWake::default());
        // Kırmızı çapa bilerek **sonra**: `wait_cells` arka planlı hücre
        // sayıyor, çapa göründüğünde boşluk aynı karede zaten işlenmiştir.
        let session = spawn_session(
            "printf '\\033[4m \\033[0;41mA\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        let cells = wait_cells(&session, &wake, 1);
        let space = cells
            .iter()
            .find(|c| c.col == 0)
            .expect("altı çizili boşluk sink'e gelmedi");
        assert_eq!((space.bg, space.ch), (None, None), "{cells:?}");
        assert_eq!(space.underline, UnderlineStyle::Single, "{cells:?}");
    }

    #[test]
    fn clean_frame_does_not_call_sink() {
        let wake = Arc::new(TestWake::default());
        // Shell ÇIKTI üretmeli: boş grid'de her hücre varsayılan arka planlı
        // olduğu için sink zaten çağrılmazdı, yani kirli kapısı tamamen
        // silinse bile sayaç 0 kalır ve sınama hiçbir şey bağlamazdı.
        let session = spawn_session("printf '\\033[41m x \\033[0m'; sleep 5", Arc::clone(&wake));

        // Dolu kareyi tüket: " x " → üç kırmızı hücre.
        assert_eq!(wait_cells(&session, &wake, 3).len(), 3);

        // İkinci çağrı hasarsız: ne kare ne iterasyon. Kapı düşseydi aynı üç
        // hücre yeniden emilir ve sayaç büyürdü.
        let mut count = 0;
        assert!(frame_if_damaged(&session, |_| count += 1).is_none());
        assert_eq!(count, 0, "hasarsız kare sink'i çağırdı");
    }

    #[test]
    fn dim_colors_on_the_draw_path_are_pinned() {
        // Sönük renklerin bekçisi **çizim yolundan** geçiyor ve beklenen
        // değerleri elle yazılı: paletten hesaplanan bir beklenti
        // sönüklük kuralı değişince kendisi de değişir ve kuralın değiştiğini
        // hiçbir sınama görmezdi. Üç hâl: varsayılan ön plan, adlı renk ve
        // ters video (sönüklük orada arka plana gidiyor).
        //
        // Sütunlar: 0 sönük varsayılan, 2 sönük kırmızı, 4 sönük ters video;
        // imleç 5. sütunda, hiçbirinin rengini çevirmiyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033[2mA\\033[0m \\033[2;31mB\\033[0m \\033[2;7mC\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        let cells = wait_frame(&session, &wake, |cells| glyph_text(cells) == "ABC");
        let at_col = |col| {
            *cells
                .iter()
                .find(|c| c.col == col)
                .unwrap_or_else(|| panic!("{col}. sütun karede yok: {cells:?}"))
        };
        // Temanın `dim` rolü: `0xd8d9dd × 2/3`, vte'nin `f32` çarpımı ve
        // kesmesiyle. Rol bir değer, kural değişince yerinde kaldı.
        let dim_foreground = LinearRgba::from_srgb(0x90, 0x90, 0x93);
        let a = at_col(0);
        assert_eq!((a.fg, a.bg), (dim_foreground, None), "{a:?}");
        // `0xd16d6a`, zemine doğru üçte bir: kanal başına
        // `(2·kaynak + zemin) / 3`, kesmeyle. Kural 007 phase-3'te `× 2/3`'ten
        // buna geçti; sayı **yine** `0x8b4846` ama aynı kural, çünkü `bateri`nin
        // zemini artık saf siyah ve karışım terimi düşüyor. Kuralın kendisi
        // açık temada ayrışıyor ve orada sınanıyor
        // ([`color::tests::dim_colors_move_toward_the_background`]).
        let b = at_col(2);
        assert_eq!(
            (b.fg, b.bg),
            (LinearRgba::from_srgb(0x8b, 0x48, 0x46), None),
            "{b:?}"
        );
        // Ters videoda sönük ön plan arka plana geçer; ön plan paletin arka
        // planı ve **sönmez**.
        let c = at_col(4);
        assert_eq!(
            (c.fg, c.bg),
            (
                LinearRgba::from_srgb(0x00, 0x00, 0x00),
                Some(dim_foreground)
            ),
            "{c:?}"
        );
    }

    #[test]
    fn dim_flag_darkens_background_in_inverse_video() {
        let wake = Arc::new(TestWake::default());
        // DIM + INVERSE + kırmızı ön plan: ön plan arka plan olur ve sönük
        // uygulanır. alacritty kitaplığı `Named(Red)`i `DimRed`e çevirmez.
        let session = spawn_session(
            "printf '\\033[2;7;31mx\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        let cells = wait_cells(&session, &wake, 1);
        // Elle yazılı: `0xd16d6a`'nın zemine karışmış sönüğü (bkz.
        // `dim_colors_on_the_draw_path_are_pinned`).
        assert_eq!(cells[0].bg, Some(LinearRgba::from_srgb(0x8b, 0x48, 0x46)));
        // Sönük olmayan kırmızıdan gerçekten farklı.
        assert_ne!(cells[0].bg, Some(color::linear_rgba(THEME.default(1))));
        // Ters videoda ön plan hücrenin arka planından gelir ve **sönmez**:
        // `DIM` yalnız `cell.fg`'den doğan renge uygulanıyor.
        assert_eq!(cells[0].ch, Some('x'));
        assert_eq!(cells[0].fg, THEME.background_linear());
    }

    #[test]
    fn cursor_carries_the_text_color_and_leaves_cells_alone() {
        // Sınırın bu tarafındaki iddia: **karar burada, boyama orada.** Renk
        // `Cursor` ile geçiyor ve imlecin durduğu hücre hiçbir şey
        // kaybetmiyor — ne kendi ön planını ne SGR 58'ini. Geri dönüş
        // (hücreyi burada ters çevirmek) `bt-gpu`'nun piksel sınamalarından
        // **geçerdi**: iki kez çevrilen renk aynı piksele varır. Görünen
        // belirti yalnız yarım örtülen hücrede çıkardı (008 → Karar 3) ve o
        // hücreyi hiçbir sınama göremez, çünkü hücre bölünemiyor.
        let wake = Arc::new(TestWake::default());
        // `\033[D` imleci X'in üstüne geri getiriyor; X hem mürekkep hem SGR
        // 58'li bir kural taşıyor, yani eski dalın düşürdüğü iki değer de
        // burada.
        let session = spawn_session(
            "printf '\\033[4;58;5;196m\\033[31mX\\033[0m\\033[D'; sleep 5",
            Arc::clone(&wake),
        );

        // Ölçüt **imlecin varışı**: PTY okuması X ile `\033[D` arasında
        // bölünebilir ve o karede imleç hâlâ bir sağdadır. Hücre listesine
        // bağlanan bir ölçüt o kareyi kabul edip yanlış hücreyi doğrulardı.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = 0;
        let (cursor, cell) = loop {
            assert!(Instant::now() < deadline, "imleç X hücresine dönmedi");
            seen = wake.wait_wakes(seen + 1, Duration::from_millis(500));
            let mut cells = Vec::new();
            let Some(cursor) = frame_if_damaged(&session, |c| cells.push(c)) else {
                continue;
            };
            if cursor.visible && (cursor.col, cursor.row) == (0, 0) {
                if let Some(cell) = cells.iter().copied().find(|c| c.ch == Some('X')) {
                    break (cursor, cell);
                }
            }
        };

        assert_eq!(
            cursor.text,
            THEME.background_linear(),
            "blok altındaki metin temanın zemininde olmalı: {cursor:?}"
        );
        // Hücre imleci hiç görmemiş gibi: kendi kırmızısı ve kendi SGR 58'i.
        assert_eq!(cell.underline, UnderlineStyle::Single, "{cell:?}");
        assert_eq!(
            cell.fg,
            color::linear_rgba(THEME.default(1)),
            "imleç hücrenin ön planını ezdi: {cell:?}"
        );
        assert_eq!(
            cell.underline_color,
            Some(color::linear_rgba(THEME.default(196))),
            "imleç hücrenin SGR 58 rengini düşürdü: {cell:?}"
        );
    }

    #[test]
    fn hidden_text_loses_ink_keeps_background() {
        let wake = Arc::new(TestWake::default());
        // `\e[8m` (conceal) ön planı gizler, arka planı değil. Bayrak
        // `bt-gpu`'ya geçmediği için burada çözülmek zorunda: geçmeseydi ve
        // burada da elenmeseydi gizlenmiş metin ekranda okunurdu.
        let session = spawn_session(
            "printf '\\033[41;8mgizli\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        let cells = wait_cells(&session, &wake, 5);
        assert!(cells.iter().all(|c| c.ch.is_none()), "{cells:?}");
        assert_eq!(backgrounds(&cells).count(), 5, "{cells:?}");
    }

    #[test]
    fn hidden_text_drops_rules_too() {
        let wake = Arc::new(TestWake::default());
        // `\e[8m` "mürekkep yok" demek. Kurallar mürekkepten ayrı bir yoldan
        // çiziliyor, yani `HIDDEN` orada da sorulmazsa altı çizili gizli
        // metin çizgisiyle okunur ve gizleme delinir. Arka plan yerinde kalır.
        let session = spawn_session(
            "printf '\\033[41;8;4:3;9mgizli\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        let cells = wait_cells(&session, &wake, 5);
        assert!(
            cells
                .iter()
                .all(|c| c.underline == UnderlineStyle::None && !c.strikeout),
            "{cells:?}"
        );
        assert_eq!(backgrounds(&cells).count(), 5, "{cells:?}");
    }

    #[test]
    fn empty_write_does_not_stall_pty_writer() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "read x; printf '\\033[42m%s\\033[0m\\n' \"$x\"; sleep 5",
            Arc::clone(&wake),
        );

        // Sıfır baytlık `Msg::Input` `EventLoop`'un yazıcısını kalıcı olarak
        // kilitlerdi: arkasından gelen her tuş kuyrukta kalırdı.
        session.write(b"");
        session.write(b"ab\n");

        // İki yeşil hücre: shell girdiyi okuyup geri yazabildi.
        let cells = wait_cells(&session, &wake, 2);
        let green = Some(color::linear_rgba(THEME.default(2)));
        assert!(backgrounds(&cells).all(|c| c.bg == green), "{cells:?}");
    }

    /// `options` ile açılan çocuğun çıktısını **tek dizgi** olarak verir;
    /// çıktı `;` ile bitmeli.
    ///
    /// `;` bekleme ölçütü: satırın son baytı, yani görününce satırın tamamı
    /// gelmiştir. İğneyle beklemek (`wait_ink`) yanlış çıktıda beş saniyelik
    /// zaman aşımına düşer ve neyin geldiğini değil neyin gelmediğini
    /// söylerdi; burada kıyas `assert_eq!` ile, gelen değer ekranda.
    /// Boşluk mürekkep değil (`Cell::ch`), dizgiden düşer.
    fn child_output(options: SessionOptions) -> String {
        let wake = Arc::new(TestWake::default());
        let session = Session::spawn(options, wake.clone()).unwrap();
        glyph_text(&wait_ink(&session, &wake, ";"))
    }

    /// Çocuğun `pwd -P`'si, karşılaştırılacak biçimde.
    ///
    /// `-P` ve `canonicalize` birlikte: macOS'ta `/var` `/private/var`'a bir
    /// sembolik bağ, ve mantıksal yol miras kalan `PWD`'ye bağlı.
    fn cwd_line(dir: &std::path::Path) -> String {
        let dir = dir.canonicalize().unwrap();
        format!("cwd={};", dir.display()).replace(' ', "")
    }

    const PRINT_CWD: &str = "printf 'cwd=%s;' \"$(pwd -P)\"; sleep 5";

    #[test]
    fn working_directory_sets_child_cwd() {
        // Sınama sürecinin dizininden (crate kökü) farklı olması yeter.
        let dir = std::env::temp_dir();
        let options = SessionOptions {
            working_directory: Some(dir.clone()),
            home: None,
            ..test_options(sh(PRINT_CWD), 200)
        };

        assert_eq!(child_output(options), cwd_line(&dir));
    }

    #[test]
    fn unreachable_working_directory_is_inherited() {
        // Belgelenen davranışın bekçisi (`SessionOptions::working_directory`):
        // yol yoksa açılış düşmüyor, çocuk miras alıyor.
        let options = SessionOptions {
            working_directory: Some("/nonexistent/bt-core-working-directory".into()),
            home: None,
            ..test_options(sh(PRINT_CWD), 200)
        };

        assert_eq!(
            child_output(options),
            cwd_line(&std::env::current_dir().unwrap())
        );
    }

    #[test]
    fn child_inherits_cwd_without_working_directory() {
        let options = test_options(sh(PRINT_CWD), 200);

        assert_eq!(
            child_output(options),
            cwd_line(&std::env::current_dir().unwrap())
        );
    }

    #[test]
    fn extra_env_reaches_child_without_overriding_term() {
        // İki sabit de listede **ezilmeye çalışılıyor**: öncelik sırası
        // (`SessionOptions::env`) yalnız burada bağlı.
        let script = "printf 'env=%s|%s|%s;' \"$BT_PROBE\" \"$TERM\" \"$COLORTERM\"; sleep 5";
        let options = SessionOptions {
            env: HashMap::from([
                ("BT_PROBE".into(), "reached".into()),
                ("TERM".into(), "dumb".into()),
                ("COLORTERM".into(), "none".into()),
            ]),
            ..test_options(sh(script), 200)
        };

        assert_eq!(
            child_output(options),
            "env=reached|xterm-256color|truecolor;"
        );
    }

    #[test]
    fn identity_env_reaches_child_without_being_overridden() {
        // Kimlik ailesi `TERM`'ün katmanında (038): ek ortam dördünü de
        // başka değerle veriyor ve hiçbiri çocuğa ulaşmıyor.
        let script = "printf 'id=%s|%s|%s|%s;' \"$TERM_PROGRAM\" \
                      \"$TERM_PROGRAM_VERSION\" \"$TERM_SESSION_ID\" \
                      \"$BATERI_TAB_URL\"; sleep 5";
        let id = TabId::parse("0F1E2D3C-4B5A-6978-8796-A5B4C3D2E1F0").unwrap();
        let options = SessionOptions {
            env: HashMap::from([
                ("TERM_PROGRAM".into(), "Apple_Terminal".into()),
                ("TERM_PROGRAM_VERSION".into(), "0".into()),
                ("TERM_SESSION_ID".into(), "foreign".into()),
                ("BATERI_TAB_URL".into(), "bateri://tab/foreign".into()),
            ]),
            tab_id: Some(id.clone()),
            ..test_options(sh(script), 200)
        };

        assert_eq!(
            child_output(options),
            format!(
                "id=bateri|{TERM_PROGRAM_VERSION}|{}|{};",
                id.as_str(),
                id.url()
            )
        );
    }

    #[test]
    fn identity_env_without_tab_id_leaves_session_keys_to_the_map() {
        // `tab_id: None`'da ezilecek sabit yok: haritadaki değer çocuğa aynen
        // geçiyor; `TERM_PROGRAM` yine koşulsuz.
        let script = "printf 'id=%s|%s|%s;' \"$TERM_PROGRAM\" \
                      \"$TERM_SESSION_ID\" \"$BATERI_TAB_URL\"; sleep 5";
        let options = SessionOptions {
            env: HashMap::from([
                ("TERM_PROGRAM".into(), "Apple_Terminal".into()),
                ("TERM_SESSION_ID".into(), "outer".into()),
                ("BATERI_TAB_URL".into(), "outer-url".into()),
            ]),
            ..test_options(sh(script), 200)
        };

        assert_eq!(child_output(options), "id=bateri|outer|outer-url;");
    }

    #[test]
    fn color_request_is_answered_from_the_theme() {
        // Renk sorusunun yanıtı çizimle **aynı temadan**: gömülü olmayan bir
        // temayla açılıp zemin (OSC 11) ve kırmızı (OSC 4;1) soruluyor. Yanıt
        // çocuğun stdin'ine PTY'den döner; `od` onu hex'e döküyor.
        //
        // `-icanon` şart: yanıtta satır sonu yok ve kanonik kip onu satır
        // bitene kadar tutardı. `-echo` da: yankılanan yanıt ekrana basılıp
        // mürekkebi kirletirdi. `-N` baytı iki yanıtın toplamına bağlıyor ki
        // `od` blok dolmasını beklemeden dökümü bitirsin.
        let theme = Theme {
            background: 0x123456,
            ansi: {
                let mut ansi = THEME.ansi;
                ansi[1] = 0xabcdef;
                ansi
            },
            ..THEME
        };
        let reply = |osc: &str, hex: u32| {
            let (r, g, b) = (hex >> 16 & 0xff, hex >> 8 & 0xff, hex & 0xff);
            format!("\x1b]{osc};rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}\x07")
        };
        let replies = reply("11", 0x123456) + &reply("4;1", 0xabcdef);
        let script = format!(
            "stty -icanon -echo; printf '\\033]11;?\\007\\033]4;1;?\\007'; \
             od -An -tx1 -N {}; sleep 5",
            replies.len()
        );
        let wake = Arc::new(TestWake::default());
        let session = Session::spawn(
            SessionOptions {
                theme,
                ..test_options(sh(&script), 200)
            },
            wake.clone(),
        )
        .unwrap();

        // Boşluklar mürekkep değil: `od`'nin satırları tek hex dizisine iner.
        let needle: String = replies.bytes().map(|b| format!("{b:02x}")).collect();
        wait_ink(&session, &wake, &needle);
        assert_eq!(session.theme(), theme);
    }

    #[test]
    fn set_theme_repaints_from_the_new_theme() {
        // Takas kare istemeli ve sıradaki kare **yeni** temanın zeminini
        // atlamalı. `b`'nin arka planı truecolor ile koyu temanın zemini
        // (`#1a1c21`): koyu temada çizilmiyor, açık temaya geçince zeminden
        // ayrık bir renk olup boyanıyor — atlama kararı eski temada kalsaydı
        // `b` yine `None` dönerdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf 'a\\033[48;2;0;0;0mb\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );
        let cells = wait_frame(&session, &wake, |cells| glyph_text(cells) == "ab");
        let dark_bg = LinearRgba::from_srgb(0x00, 0x00, 0x00);
        assert!(cells.iter().all(|c| c.bg.is_none()), "{cells:?}");
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "takastan önce kare kalmamalı"
        );

        // Aynı tema no-op: kare istenmez.
        session.set_theme(THEME);
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "aynı tema kare istedi"
        );

        let light = Theme::BATERI_LIGHT;
        let before = wake.wait_wakes(0, Duration::ZERO);
        session.set_theme(light);
        assert!(
            wake.wait_wakes(before + 1, Duration::ZERO) > before,
            "takas uyandırmadı"
        );
        assert_eq!(session.theme(), light);
        let mut cells = Vec::new();
        assert!(
            frame_if_damaged(&session, |c| cells.push(c)).is_some(),
            "takas kare istemedi"
        );
        let at_col = |col| *cells.iter().find(|c| c.col == col).expect("hücre karede");
        let (a, b) = (at_col(0), at_col(1));
        assert_eq!(
            (a.fg, a.bg),
            (color::linear_rgba(light.default(256)), None),
            "{a:?}"
        );
        assert_eq!(b.bg, Some(dark_bg), "{b:?}");
    }

    #[test]
    fn term_config_keeps_every_other_field() {
        // `set_options` `Config`'in tamamını değiştiriyor: bir seçeneğin
        // değişimi öteki alanları oynatmamalı. Bizim iki alanımız birbirini
        // koruyor — `osc52` değişimi geçmişi kırpmıyor, `scrollback` değişimi
        // OSC 52'yi açmıyor — ve geri kalanı alacritty'nin varsayılanında
        // kalıyor. `..Config::default()`'u ikinci bir yerde yazan bir çağrı
        // buradan geçmez ama bu kurucuyu bozan bir değişiklik burada düşer.
        let options = TerminalOptions {
            scrollback: 100,
            osc52: Osc52::Off,
            // **Varsayılan olmayan** bilerek: fixture varsayılanla
            // doldurulursa `default_cursor_style` zaten `Config::default()`'a
            // eşit olur, aşağıdaki döngü onu sıfırlama listesine eklemeden
            // geçer ve guard'ın vaadi ("bu alanların dışında hiçbir şey
            // kurulmuyor") sessizce yalan olurdu. Aynısı blink için de
            // geçerli: `Off` varsayılan olduğu için aşağıdaki "blink
            // kurulmadı" iddiası onunla **boş** kalırdı.
            cursor: CaretShape::Beam,
            blink: CursorBlink::On,
        };
        let before = term_config(options);
        assert_eq!(
            (before.scrolling_history, before.osc52),
            (100, TermOsc52::Disabled)
        );

        let scrolled = term_config(TerminalOptions {
            scrollback: 7,
            ..options
        });
        assert_eq!(
            (scrolled.scrolling_history, scrolled.osc52),
            (7, TermOsc52::Disabled),
            "scrollback değişimi OSC 52'yi açtı"
        );

        let copying = term_config(TerminalOptions {
            osc52: Osc52::Copy,
            ..options
        });
        assert_eq!(
            (copying.scrolling_history, copying.osc52),
            (100, TermOsc52::OnlyCopy),
            "osc52 değişimi geçmişi oynattı"
        );

        let shaped = term_config(TerminalOptions {
            cursor: CaretShape::Underline,
            blink: CursorBlink::default(),
            ..options
        });
        assert_eq!(
            (shaped.scrolling_history, shaped.osc52),
            (100, TermOsc52::Disabled),
            "şekil değişimi başka alanı oynattı"
        );
        assert_eq!(
            shaped.default_cursor_style.shape,
            CursorShape::Underline,
            "şekil ayardan gelmedi"
        );
        // **Blink config'e yazılmıyor ve yazılmamalı.** `"on"`/`"off"` birer
        // ezme ve alacritty onları ifade edemiyor (`AdapterInner::blink`);
        // buraya `matches!(options.blink, On)` yazan bir sadeleştirme
        // uygulamanın `\e[2 q`'suyla susturulabilir bir "hep sönsün" üretirdi.
        // Fixture `On` taşıdığı için bu iddia gerçekten bir kapı.
        assert!(
            !shaped.default_cursor_style.blinking,
            "ezme config'e yazıldı"
        );
        // `Auto`'nun tabanı ise **açık**: kapalı olsaydı hiçbir şey blink
        // istemediği için `"auto"` düz promptta `"off"` ile aynı olurdu
        // (kullanıcı bildirdi, 2026-09-19).
        let auto = term_config(TerminalOptions {
            blink: CursorBlink::Auto,
            ..options
        });
        assert!(
            auto.default_cursor_style.blinking,
            "auto'nun tabanı kapalı: off'tan ayırt edilemez"
        );

        // Kelimenin tanımı her kurulumda aynı sabit — seçeneklerin hiçbiri
        // onu oynatmıyor ve alacritty'nin varsayılanına dönmüyor.
        for config in [&before, &scrolled, &copying, &shaped, &auto] {
            assert_eq!(config.semantic_escape_chars, WORD_SEPARATORS);
        }

        // **Dört** alanın dışında hiçbir şey kurulmuyor.
        for config in [before, scrolled, copying, shaped] {
            assert_eq!(
                Config {
                    scrolling_history: Config::default().scrolling_history,
                    osc52: Config::default().osc52,
                    default_cursor_style: Config::default().default_cursor_style,
                    semantic_escape_chars: Config::default().semantic_escape_chars,
                    ..config
                },
                Config::default()
            );
        }
    }

    #[test]
    fn set_terminal_options_trims_history_and_repaints() {
        // On satırlık ekranda altmış satır: geçmiş elliyi aşar (tavan 100).
        // Pencere geçmişe kaydırılmışken tavan ona inince hem geçmiş hem
        // kaydırma ofseti kırpılmalı ve ekrandaki satırlar değiştiği için
        // kare istenmeli — alacritty'nin hasarı okunmuyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "lines() { i=0; while [ $i -lt 60 ]; do echo $i; i=$((i + 1)); done; }; \
             lines; read _; lines; printf end; sleep 5",
            Arc::clone(&wake),
        );
        wait_until("geçmiş dolmadı", Duration::from_secs(5), || {
            session.term.lock().history_size() >= 50
        });
        wait_settled(&session);
        assert_eq!(session.scroll_page(4), Some(40));
        let _ = frame_if_damaged(&session, |_| ());

        let before = wake.wait_wakes(0, Duration::ZERO);
        session.set_terminal_options(TerminalOptions {
            scrollback: 10,
            osc52: Osc52::Copy,
            cursor: CaretShape::default(),
            blink: CursorBlink::default(),
        });
        {
            let term = session.term.lock();
            assert_eq!(term.history_size(), 10);
            assert_eq!(term.grid().display_offset(), 10);
        }
        assert!(
            wake.wait_wakes(before + 1, Duration::ZERO) > before,
            "seçenek değişimi uyandırmadı"
        );
        assert!(
            frame_if_damaged(&session, |_| ()).is_some(),
            "seçenek değişimi kare istemedi"
        );

        // Yeni tavan kalıcı: sonraki altmış satır geçmişi yine ona kırpıyor.
        session.write(b"\n");
        wait_ink(&session, &wake, "end");
        assert_eq!(session.term.lock().history_size(), 10);
    }

    /// `osc52` kipiyle açılan bir oturum; geri kalanı [`test_options`].
    fn spawn_with_osc52(script: &str, osc52: Osc52, wake: Arc<TestWake>) -> Session {
        let options = test_options(sh(script), 40);
        let options = SessionOptions {
            terminal: TerminalOptions {
                osc52,
                ..options.terminal
            },
            ..options
        };
        Session::spawn(options, wake).unwrap()
    }

    // OSC 52 sınamalarının yükleri base64 (alacritty `STANDARD`, dolgulu):
    // `aGVsbG8=` = "hello", `c2Vs` = "sel", `b2Zm` = "off", `b24=` = "on".
    //
    // Olumsuz iddialar "şu kadar bekledim, gelmedi" değil: diziden **sonra**
    // basılan iğne karede görününce dizi ayrıştırılmış demek (`Term` sırayla
    // işliyor), pano kaydına o anda bakılıyor.

    #[test]
    fn osc52_store_reaches_wake_for_every_target_but_not_empty() {
        // macOS'ta tek pano: `p` ve `s` de (Neovim'in `*` kaydı `p` yolluyor)
        // `c` gibi `Wake`'e ulaşır; iki sonlandırıcı (BEL, ST) da. Boş metin
        // panoyu silerdi, düşer — sırası kayıtta görülüyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_with_osc52(
            "printf '\\033]52;p;c2Vs\\007\\033]52;s;b24=\\033\\\\\\033]52;c;\\007\
             \\033]52;c;aGVsbG8=\\007end'; sleep 5",
            Osc52::Copy,
            Arc::clone(&wake),
        );
        wait_ink(&session, &wake, "end");
        assert_eq!(wake.copies(), ["sel", "on", "hello"]);
    }

    #[test]
    fn osc52_off_reaches_nothing() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_with_osc52(
            "printf '\\033]52;c;aGVsbG8=\\007end'; sleep 5",
            Osc52::Off,
            Arc::clone(&wake),
        );
        wait_ink(&session, &wake, "end");
        assert_eq!(wake.copies(), Vec::<String>::new());
    }

    #[test]
    fn set_terminal_options_switches_osc52_live() {
        // Kip canlı değişiyor: `Term::set_options` `Config`'in tamamını
        // değiştiriyor ve alacritty kapıyı her dizide ondan okuyor. Açılışta
        // açık, kapatılınca dizi düşüyor, yeniden açılınca geçiyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_with_osc52(
            "read _; printf '\\033]52;c;b2Zm\\007one'; \
             read _; printf '\\033]52;c;b24=\\007two'; sleep 5",
            Osc52::Copy,
            Arc::clone(&wake),
        );
        let options = |osc52| TerminalOptions {
            scrollback: 100,
            osc52,
            cursor: CaretShape::default(),
            blink: CursorBlink::default(),
        };

        session.set_terminal_options(options(Osc52::Off));
        session.write(b"\n");
        wait_ink(&session, &wake, "one");
        assert_eq!(wake.copies(), Vec::<String>::new());

        session.set_terminal_options(options(Osc52::Copy));
        session.write(b"\n");
        wait_ink(&session, &wake, "two");
        assert_eq!(wake.copies(), ["on"]);
    }

    /// `needle` mürekkepte görünene kadar kare bekler; `od` satırı bölünmüş
    /// PTY okumasıyla parça parça gelebilir.
    fn wait_ink(session: &Session, wake: &TestWake, needle: &str) -> Vec<Cell> {
        wait_frame(session, wake, |cells| glyph_text(cells).contains(needle))
    }

    /// `ready` doğru dönene kadar 20 ms aralıkla sorar; `timeout` dolarsa
    /// `what` gerekçesiyle düşer. Kip ve durulma beklemelerinin ortak
    /// iskeleti — kare değil **durum** bekleyenler için (kare bekleyen
    /// `wait_frame`).
    fn wait_until(what: &str, timeout: Duration, mut ready: impl FnMut() -> bool) {
        let deadline = Instant::now() + timeout;
        while !ready() {
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn the_alternate_screen_flag_crosses_the_boundary_with_the_frame() {
        // **Bayrağı yayınlayan `frame()`**, ayrı bir sorgu değil: değer
        // çizilen karenin `content_rows`'uyla tutarlı olmak zorunda (dock'un
        // payı ondan düşülüyor). Sınama tam da o sözleşmeyi tutuyor —
        // `?1049h` baytlarından sonra bir **kare** koşmadan cevabın
        // değişmemesi doğru, değişmesi kuralın bozulması olurdu.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf 'normal'; sleep 0.3; printf '\\033[?1049h'; sleep 0.3; \
             printf '\\033[?1049l'; sleep 5",
            Arc::clone(&wake),
        );
        assert!(!session.alt_screen(), "açılışta alternatif ekran");

        let seen = |want: bool, what: &str| {
            wait_until(what, Duration::from_secs(5), || {
                // Kareyi **biz** koşuyoruz: bayrak ancak kare yolundan
                // yayınlanıyor ve üretimde de öyle (display link).
                frame_if_damaged(&session, |_| ());
                session.alt_screen() == want
            });
        };
        seen(true, "alternatif ekrana girilmedi");
        seen(false, "alternatif ekrandan çıkılmadı");
        session.shutdown();
    }

    /// 2004 kipi set olana kadar bekler: erken giden bir `paste` ham yazardı
    /// ve sınama yanlış şeyi doğrular, yanlış kırmızıyı değil.
    fn wait_bracketed_mode(session: &Session) {
        wait_until("2004 kipi açılmadı", Duration::from_secs(5), || {
            session.bracketed_paste()
        });
    }

    /// Kare akışı durulana kadar bekler: **tek** `None` yetmez. Okumanın
    /// karesi içerik görünür olduktan *sonra* düşebilir — `dirty` bayrağını
    /// `Event::Wakeup` dikiyor ve o, `term.process()` kilidi bırakıldıktan
    /// sonra koşuyor. Ölçüt bu yüzden **arka arkaya iki** `None`, aralarında
    /// 20 ms: ilk `None` o gecikmiş `Wakeup`'tan önce düşmüş olabilir.
    fn wait_settled(session: &Session) {
        wait_until("kare akışı durulmadı", Duration::from_secs(2), || {
            frame_if_damaged(session, |_| ()).is_none() && {
                std::thread::sleep(Duration::from_millis(20));
                frame_if_damaged(session, |_| ()).is_none()
            }
        });
    }

    /// Yapıştırma yükü: 20 bayt. `od` 16 baytlık bloklar hâlinde okur ve blok
    /// dolmadan döküm basmaz — kısa bir yük ("AB\n") yankıdan öteye gitmez,
    /// döküm satırı hiç kareye düşmezdi (deneyle doğrulandı: 3 bayt →
    /// sessizlik, 16+ bayt → döküm; kanonik modun satır tamponu değil,
    /// `od`'nin okuma boyutu). Sarılmış hâl (6 + 20 + 6) tam iki blok eder,
    /// kapanış iğnesi ikinci döküm satırından okunur; ham hâlde ilk blok dolar.
    /// Dolgu hex'i iğnelerle çakışmıyor ("4142" yok, "1b" yok).
    const PASTE_PAYLOAD: &[u8] = b"AB0123456789abcdefg\n";

    #[test]
    fn paste_wraps_when_bracketed_mode_set() {
        // Çocuk önce 2004'ü açıyor (`\e[?2004h` PTY→Term yönü, uygulamadan),
        // sonra `od` stdin'i hex'e döküyor. `paste()` o kipi kilit altında
        // sorgulayıp sarıyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf '\\033[?2004h'; exec od -An -tx1", Arc::clone(&wake));

        wait_bracketed_mode(&session);

        session.paste(PASTE_PAYLOAD.to_vec());
        // `1b5b3230307e` = `\e[200~`, `1b5b3230317e` = `\e[201~`. İğneler
        // boşluksuz: `od` alanları boşlukla ayırıyor ama boşluk `frame()`'de
        // mürekkep değil (`ch: None`), yani mürekkep dizisinde boşluk yok.
        // Satır genişliği bölünebilir, o yüzden ölçüt iki ayrı iğne — tek
        // uzun iğne bölünmüş satırda tutmazdı.
        let cells = wait_ink(&session, &wake, "1b5b3230307e");
        assert!(
            glyph_text(&cells).contains("4142"),
            "yük sarmanın içinde ham gitmeli: {cells:?}"
        );
        // Kapanış iğnesi tek `paste` ile gelmez: sarılı yük 32 bayt, `od`
        // ilk 16 baytı ilk okumada döküyor, kalan 16 baytın 10'u ikinci
        // okumada `od`'nin blok tamponunda kalıyor, son 6 bayt (`\e[201~`)
        // ise satır sonu görmeden PTY tamponundan çıkmıyor. İkinci `paste`
        // iki tamponu da akıtıyor — dökülen ikinci satır birinci yapıştırmanın
        // kapanışını taşıyor. İkinci yapıştırma da `paste()` yolundan gidiyor:
        // ham bayt bu sınamada da `write`'a değmiyor.
        session.paste(PASTE_PAYLOAD.to_vec());
        wait_ink(&session, &wake, "1b5b3230317e");
    }

    /// Dock'un satırın sahibi olduğu bir çocuk: 2004 açık, çıpalı prompt,
    /// canlı ayna — sonra `od` stdin'i hex'e döküyor.
    fn spawn_docked_od(wake: Arc<TestWake>) -> Session {
        spawn_docked_od_in(wake, &mirror("", 0))
    }

    /// [`spawn_docked_od`]'nin keymap'i çağırandan gelen hâli.
    fn spawn_docked_od_in(wake: Arc<TestWake>, mirror: &str) -> Session {
        spawn_docked_session(
            &format!(
                "printf '\\033[?2004h{}{mirror}'; exec od -An -tx1",
                anchored_prompt(1),
            ),
            wake,
        )
    }

    #[test]
    fn paste_is_typed_when_the_dock_owns_the_line() {
        // **Sarmanın dar istisnası.** Dock canlıyken sarılı yapıştırma zsh'in
        // `bracketed-paste-magic`'ine düşüyor, o da bir sonraki tuşa kadar
        // bekliyor ve ayna bayat kaldığı için metin dock yerine ızgarada
        // beliriyor. Satır sonu ve kontrol karakteri taşımayan yük yazılmış
        // girdi gibi gidiyor: güvenliğin koruduğu iki şey de koşulun dışında.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_od(Arc::clone(&wake));
        wait_bracketed_mode(&session);
        wait_mirror(&session, DockStatus::Live);

        session.paste(b"abcdefghijklmnopqrst".to_vec());
        // **Satır sonu `paste()`'ten değil `write()`'tan**: yükün kendisinde
        // satır sonu olsaydı istisnanın koşulu bozulurdu, ama kanonik kipteki
        // tty satır sonu görmeden `od`'ye tek bayt vermez. Ayrı bir `write`
        // tamponu akıtıyor ve sarma kapısına hiç uğramıyor.
        session.write(b"\n");
        // `6162` = "ab". Sarılsaydı dökümde ondan **önce** açılış iğnesi
        // (`1b5b3230307e`) dururdu; aynı hücrelerde yokluğu ölçüt.
        let cells = wait_ink(&session, &wake, "6162");
        assert!(
            !glyph_text(&cells).contains("1b5b3230307e"),
            "dock satırın sahibiyken düz metin sarıldı: {cells:?}"
        );
        session.shutdown();
    }

    /// Düzenlenebilir bir dock satırı (`hello`) ve arkasında `od`: kapının
    /// dört koşulu `tail` ile bozulabiliyor. 60 sütun, çünkü `od`'nin 16
    /// baytlık satırı (48 karakter) tek satıra sığsın.
    fn spawn_editable_od(wake: Arc<TestWake>, mirror: &str, tail: &str) -> Session {
        spawn_docked_with_cols(
            &format!(
                "printf '\\033[?2004h{}{mirror}{tail}'; exec od -An -tx1",
                anchored_prompt(1),
            ),
            60,
            wake,
        )
    }

    /// `hello` (`aGVsbG8=`), caret sonda, keymap `main`.
    const HELLO: &str = "\\033]8133;u;5;;aGVsbG8=;;;bWFpbg==\\007";
    /// Yetenek: bu prompt'ta widget bağlı.
    const EDITABLE: &str = "\\033]8133;w\\007";

    #[test]
    fn the_edit_command_is_the_wire_format() {
        assert_eq!(dock_edit_command(1, 3, 5), b"\x1b[8133~d;1;3;5\x07");
        assert_eq!(dock_edit_command(0, 0, 0), b"\x1b[8133~d;0;0;0\x07");
    }

    #[test]
    fn each_condition_of_the_edit_gate_closes_it_alone() {
        // Dört koşul, her biri tek başına (031 R4.4). Kapalı kapıda seçim
        // duruyor ama tuş tüketilmiyor: bugünkü yolundan gidecek.
        let open = spawn_editable_od(Arc::new(TestWake::default()), HELLO, EDITABLE);
        wait_mirror(&open, DockStatus::Live);
        wait_until("kapı açılmadı", Duration::from_secs(5), || {
            open.can_edit_dock()
        });
        select_dock(&open, 1, 3);
        assert!(open.can_cut());
        // Nesil ilerledi, ayna henüz cevap vermedi: bayat.
        open.key_gen.fetch_add(1, Ordering::Release);
        assert!(!open.can_edit_dock(), "bayat aynada kapı açık");
        assert!(!open.dock_key(DockKey::Backspace));
        assert!(!open.can_cut());
        open.shutdown();

        for (name, mirror, tail) in [
            // `dmljbWQ=` = `vicmd`.
            (
                "vicmd",
                "\\033]8133;u;5;;aGVsbG8=;;;dmljbWQ=\\007",
                EDITABLE,
            ),
            ("yetenek yok", HELLO, ""),
            ("Running", HELLO, "\\033]8133;w\\007\\033]133;C\\007"),
        ] {
            let session = spawn_editable_od(Arc::new(TestWake::default()), mirror, tail);
            wait_until(
                &format!("{name}: ayna gelmedi"),
                Duration::from_secs(5),
                || {
                    let mut state = DockState::default();
                    session.dock_state(&mut state);
                    state.buffer == "hello" && session.shell_state().is_some()
                },
            );
            wait_settled(&session);
            select_dock(&session, 1, 3);
            assert!(!session.can_edit_dock(), "{name}: kapı açık");
            assert!(!session.dock_key(DockKey::Backspace), "{name}");
            assert!(!session.dock_key(DockKey::ShiftLeft), "{name}");
            session.dock_click();
            assert_eq!(
                session.key_gen.load(Ordering::Acquire),
                0,
                "{name}: komut gitti"
            );
            session.shutdown();
        }
    }

    #[test]
    fn backspace_over_a_selection_writes_exactly_the_edit_command() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_editable_od(Arc::clone(&wake), HELLO, EDITABLE);
        wait_mirror(&session, DockStatus::Live);
        select_dock(&session, 1, 3);
        assert!(session.dock_key(DockKey::Backspace));
        // Komut 15 bayt; satır sonu `od`'nin 16 baytlık bloğunu tamamlıyor.
        session.write(b"\n");
        let cells = wait_ink(&session, &wake, "0a");
        assert!(
            glyph_text(&cells).contains("1b5b383133337e643b313b333b35070a"),
            "komut baytları: {cells:?}"
        );
        assert_eq!(
            lock(&session.shell).dock_selection,
            None,
            "komut seçimi kaldırmadı"
        );
        session.shutdown();
    }

    /// `a🇹🇷` (`YfCfh7nwn4e3`), caret sonda (3), keymap `main`.
    const FLAG_LINE: &str = "\\033]8133;u;3;;YfCfh7nwn4e3;;;bWFpbg==\\007";

    #[test]
    fn plain_keys_walk_a_cluster_whole() {
        // 035 Karar 7: kapı açıkken caret'in bitişiğindeki bayrak ⌫ ile
        // bütün siliniyor, ← ile bütün geçiliyor — kabuğa tek komut. Seçim
        // yok; kümeleme kapalıyken aynı tuş bugünkü yolundan (tüketilmiyor).
        for (name, cluster, key, needle) in [
            ("⌫", true, DockKey::Backspace, Some("643b313b333b33070a")),
            ("←", true, DockKey::Left, Some("643b313b313b33070a")),
            ("kümesiz ⌫", false, DockKey::Backspace, None),
        ] {
            let wake = Arc::new(TestWake::default());
            let mut options = test_options(
                sh(&format!(
                    "printf '\\033[?2004h{}{FLAG_LINE}{EDITABLE}'; exec od -An -tx1",
                    anchored_prompt(1),
                )),
                60,
            );
            options.dock = true;
            options.cluster = cluster;
            let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
            wait_mirror(&session, DockStatus::Live);
            wait_until(
                &format!("{name}: kapı açılmadı"),
                Duration::from_secs(5),
                || session.can_edit_dock(),
            );
            let Some(needle) = needle else {
                assert!(!session.dock_key(key), "{name}: tuş tüketildi");
                assert_eq!(session.key_gen.load(Ordering::Acquire), 0, "{name}");
                session.shutdown();
                continue;
            };
            assert!(session.dock_key(key), "{name}: tuş tüketilmedi");
            // Komut 15 bayt; satır sonu `od`'nin bloğunu tamamlıyor.
            session.write(b"\n");
            let cells = wait_ink(&session, &wake, "0a");
            assert!(
                glyph_text(&cells).contains(needle),
                "{name}: komut baytları: {cells:?}"
            );
            session.shutdown();
        }
    }

    /// `🇹🇷a🇺🇸` (`8J+HufCfh7dh8J+HuvCfh7g=`), caret sonda (5), keymap `main`.
    const FLAGS_LINE: &str = "\\033]8133;u;5;;8J+HufCfh7dh8J+HuvCfh7g=;;;bWFpbg==\\007";

    /// Kümeli dock'ta `od`'ye bağlı bir oturum: kabuk hiç ayna basmıyor,
    /// yani ilk komuttan sonra ayna **bayat kalıyor** — basılı tuşun
    /// tekrarının aynadan hızlı geldiği an, kalıcı olarak.
    fn spawn_line_od(wake: &Arc<TestWake>, line: &str) -> Session {
        let mut options = test_options(
            sh(&format!(
                "printf '\\033[?2004h{}{line}{EDITABLE}'; exec od -An -tx1",
                anchored_prompt(1),
            )),
            60,
        );
        options.dock = true;
        options.cluster = true;
        let session = Session::spawn(options, Arc::clone(wake) as Arc<dyn Wake>).unwrap();
        wait_mirror(&session, DockStatus::Live);
        wait_until("kapı açılmadı", Duration::from_secs(5), || {
            session.can_edit_dock()
        });
        session
    }

    #[test]
    fn a_held_backspace_never_splits_a_cluster() {
        // 035 phase-5: ayna ilk komuta cevap vermeden gelen tekrarlar
        // komutun beklenen sonucuna karşı karar veriyor. Kümeli satırda tek
        // kod noktalı `a` da komutla gidiyor, yoksa zincir kırılır ve
        // üçüncü ⌫ ZLE'ye gidip `🇹🇷`'nin yalnız `🇷`'sini silerdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_line_od(&wake, FLAGS_LINE);
        for _ in 0..3 {
            assert!(session.dock_key(DockKey::Backspace), "tekrar kapıdan döndü");
        }
        // Satır boşaldı: dördüncü ⌫ bugünkü yolda (ucunda bölünecek bir şey yok).
        assert!(!session.dock_key(DockKey::Backspace));
        // Üç komut 45 bayt; üç satır sonu `od`'nin üçüncü bloğunu tamamlıyor.
        session.write(b"\n\n\n");
        let cells = wait_ink(&session, &wake, "0a0a0a");
        assert!(
            glyph_text(&cells).contains(
                "643b333b353b35071b5b383133337e643b323b333b33071b5b383133337e643b303b323b3207"
            ),
            "komut baytları: {cells:?}"
        );
        session.shutdown();
    }

    #[test]
    fn only_an_emoji_line_routes_single_code_points() {
        // NFD bir yol (`u` + U+0308, macOS'un dosya adları) satırı emoji
        // satırı yapmıyor: tek kod noktalı `x` ZLE'nin kendi silmesinde
        // kalıyor (autopair gibi bağlamalar), birleştiricili `ü` yine bütün.
        let wake = Arc::new(TestWake::default());
        // Caret `ü` ile `x`'in arasında (2).
        let session = spawn_line_od(&wake, "\\033]8133;u;2;;dcyIeA==;;;bWFpbg==\\007");
        assert!(!session.dock_key(DockKey::Delete), "düz `x` komutla gitti");
        assert!(session.dock_key(DockKey::Left), "`ü` bütün geçilmedi");
        session.shutdown();
    }

    #[test]
    fn other_input_ends_the_prediction() {
        // Tahmin yalnız kendi neslinde: araya giren girdinin etkisini
        // bilmiyoruz, kapı yine bayat aynaya bakıp kapanıyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_line_od(&wake, FLAGS_LINE);
        assert!(session.dock_key(DockKey::Left));
        assert!(session.can_edit_dock(), "tahmin kapıyı açmadı");
        // ⇧←/⇧→ tahmin satırında bugünkü yolundan: seçim bayat aynaya kurulmaz.
        assert!(!session.dock_key(DockKey::ShiftLeft), "⇧← tahminde yutuldu");
        session.write(b"x");
        assert!(
            !session.can_edit_dock(),
            "araya giren girdiden sonra kapı açık"
        );
        assert!(!session.dock_key(DockKey::Backspace));
        session.shutdown();
    }

    #[test]
    fn shift_arrows_select_from_the_caret_without_writing() {
        // ⇧←/⇧→ yalnız terminalde (031 Karar 8): kabuğa hiçbir şey gitmiyor,
        // yani nesil de ilerlemiyor ve kapı açık kalıyor.
        let session = spawn_editable_od(Arc::new(TestWake::default()), HELLO, EDITABLE);
        wait_mirror(&session, DockStatus::Live);
        wait_until("kapı açılmadı", Duration::from_secs(5), || {
            session.can_edit_dock()
        });
        assert!(
            !session.dock_key(DockKey::Backspace),
            "seçimsiz ⌫ tüketildi"
        );
        assert!(session.dock_key(DockKey::ShiftLeft));
        assert!(session.dock_key(DockKey::ShiftLeft));
        let range = lock(&session.shell).dock_selection.and_then(|s| s.range());
        assert_eq!(range, Some((3, 5)));
        assert_eq!(session.dock_selection_text().as_deref(), Some("lo"));
        assert_eq!(session.key_gen.load(Ordering::Acquire), 0);
        assert!(session.can_edit_dock());
        session.shutdown();
    }

    #[test]
    fn typing_and_pasting_replace_the_selection() {
        // Metin tele girmiyor: önce `d`, sonra olağan yol — yapıştırmanın
        // sarma kararı dahil (tek satır, dock sahibi: yazılmış girdi gibi).
        for (name, act, needle) in [
            // Kanonik tty `od`'ye satır sonunda veriyor, `od` de yalnız dolu
            // 16 baytlık blokları döküyor: iğne ilk bloğun sonu.
            ("yazma", 0, "643b313b333b35075a"),
            ("yapıştırma", 1, "643b313b333b3507616263"),
        ] {
            let wake = Arc::new(TestWake::default());
            let session = spawn_editable_od(Arc::clone(&wake), HELLO, EDITABLE);
            wait_bracketed_mode(&session);
            wait_mirror(&session, DockStatus::Live);
            select_dock(&session, 1, 3);
            if act == 0 {
                session.type_text("Z");
            } else {
                session.paste(b"abcdefghijklmnopqrst".to_vec());
            }
            session.write(b"\n");
            let cells = wait_ink(&session, &wake, needle);
            assert!(
                !glyph_text(&cells).contains("1b5b3230307e"),
                "{name}: sarıldı: {cells:?}"
            );
            session.shutdown();
        }
    }

    #[test]
    fn paste_stays_wrapped_outside_an_insert_keymap() {
        // **İstisnanın dördüncü koşulu.** `bindkey -v` kullanan biri Esc'e
        // bastığında ZLE `vicmd`'ye geçiyor; safha hâlâ `Input`, ayna hâlâ
        // `Live`, blok hâlâ açık — yani öteki üç koşul da sağlanıyor. Ham
        // akıtsaydık baytlar **komut** olurdu: panodaki `dd` satırı siler.
        // Sarılı yol her keymap'te harfi harfine ekliyor, doğru cevap o
        // (`/code-review`, 012 phase-6).
        let wake = Arc::new(TestWake::default());
        // `dmljbWQ=` = `vicmd`.
        let session = spawn_docked_od_in(Arc::clone(&wake), "\\033]8133;u;0;;;;;dmljbWQ=\\007");
        wait_bracketed_mode(&session);
        wait_mirror(&session, DockStatus::Live);

        session.paste(b"abcdefghijklmnopqrst".to_vec());
        session.write(b"\n");
        let cells = wait_ink(&session, &wake, "6162");
        assert!(
            glyph_text(&cells).contains("1b5b3230307e"),
            "vicmd'de yapıştırma sarılmadı: {cells:?}"
        );
        session.shutdown();
    }

    #[test]
    fn paste_stays_wrapped_when_the_mirror_carries_no_keymap() {
        // **Eski betikle koşan pencere** (`plan.md` → Göç): keymap alanı yok,
        // yani hangi keymap'te olduğumuzu bilmiyoruz. Bilmemek istisnayı
        // **kapatıyor** — phase-5 öncesinin sarılı yoluna dönüyoruz, ki bu her
        // keymap'te doğru.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_od_in(Arc::clone(&wake), "\\033]8133;u;0;;;;\\007");
        wait_bracketed_mode(&session);
        wait_mirror(&session, DockStatus::Live);

        session.paste(b"abcdefghijklmnopqrst".to_vec());
        session.write(b"\n");
        let cells = wait_ink(&session, &wake, "6162");
        assert!(
            glyph_text(&cells).contains("1b5b3230307e"),
            "keymap'siz aynada yapıştırma sarılmadı: {cells:?}"
        );
        session.shutdown();
    }

    #[test]
    fn paste_stays_wrapped_when_it_carries_a_newline() {
        // İstisnanın **sınırı** ve varlık sebebi: satır sonu taşıyan yük ham
        // gitseydi kullanıcı Cmd-V'ye basar basmaz satır çalışırdı. Dock
        // canlı olsa bile sarma duruyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_od(Arc::clone(&wake));
        wait_bracketed_mode(&session);
        wait_mirror(&session, DockStatus::Live);

        session.paste(PASTE_PAYLOAD.to_vec());
        wait_ink(&session, &wake, "1b5b3230307e");
        session.shutdown();
    }

    #[test]
    fn the_refresh_command_is_the_wire_format() {
        assert_eq!(DOCK_REFRESH_COMMAND, b"\x1b[8133~r\x07");
        assert!(DOCK_REFRESH_COMMAND.starts_with(DOCK_EDIT_PREFIX.as_bytes()));
    }

    #[test]
    fn a_multiline_paste_asks_for_a_refresh_behind_the_closing_bracket() {
        // 032 R5: sarılı yükün **arkasında**, aynı yazımda. Sarılı hâl
        // 6 + 4 + 6 = 16 bayt, yani `od`'nin ilk satırı; komut (9 bayt) ve
        // ardından yazılan yedi bayt ikinci satırı dolduruyor — iğne tek
        // döküm satırında kalıyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_editable_od(Arc::clone(&wake), HELLO, EDITABLE);
        wait_bracketed_mode(&session);
        wait_mirror(&session, DockStatus::Live);
        wait_until("kapı açılmadı", Duration::from_secs(5), || {
            session.can_edit_dock()
        });
        session.paste(b"ab\nc".to_vec());
        session.write(b"ABCDEF\n");
        let cells = wait_ink(&session, &wake, "4142434445460a");
        let text = glyph_text(&cells);
        assert!(
            text.contains("1b5b3230317e1b5b383133337e7207"),
            "tazeleme kapanış iğnesinin arkasında değil: {cells:?}"
        );
        session.shutdown();
    }

    #[test]
    fn the_paste_refresh_goes_only_through_the_full_edit_gate() {
        // Kapının dört kolu (vicmd, yetenek yok, bayat ayna) ve tek satırlık
        // yük: hiçbirinde komut eklenmiyor. `vicmd`'de baytlar komut olurdu,
        // bağlamasız kabukta BEL `send-break`.
        let open = spawn_editable_od(Arc::new(TestWake::default()), HELLO, EDITABLE);
        wait_mirror(&open, DockStatus::Live);
        wait_until("kapı açılmadı", Duration::from_secs(5), || {
            open.can_edit_dock()
        });
        assert!(open.paste_refreshes(b"a\nb"));
        assert!(open.paste_refreshes(b"a\rb"));
        assert!(!open.paste_refreshes(b"ab"), "tek satırda tazeleme");
        open.key_gen.fetch_add(1, Ordering::Release);
        assert!(!open.paste_refreshes(b"a\nb"), "bayat aynada tazeleme");
        open.shutdown();

        for (name, mirror, tail) in [
            (
                "vicmd",
                "\\033]8133;u;5;;aGVsbG8=;;;dmljbWQ=\\007",
                EDITABLE,
            ),
            ("yetenek yok", HELLO, ""),
        ] {
            let session = spawn_editable_od(Arc::new(TestWake::default()), mirror, tail);
            wait_mirror(&session, DockStatus::Live);
            wait_settled(&session);
            assert!(!session.paste_refreshes(b"a\nb"), "{name}");
            session.shutdown();
        }
    }

    #[test]
    fn paste_strips_escape_and_etx_from_wrapped_payload() {
        // Sarma **kendi iğnesini** korumalı: yükteki `ESC`/`ETX` süzülmezse
        // panoya `\x1b[201~` koyan bir süreç bölgeyi erken kapatır ve gerisi
        // uygulamaya yazılmış girdi olarak varır — kullanıcı Cmd-V'ye basar
        // basmaz satır çalışır (enjeksiyon). Ölçüt `od` dökümü: "ABCDEF"
        // bitişik görünüyorsa aradaki `ESC`/`ETX` düşmüş demektir, çünkü
        // süzülmeseydi döküm "41421b4344…" olurdu (ilk 16 bayt: sarma iğnesi
        // + yükün ilk on baytı).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf '\\033[?2004h'; exec od -An -tx1", Arc::clone(&wake));
        wait_bracketed_mode(&session);

        session.paste(b"AB\x1bCD\x03EF0123456789\n".to_vec());
        let cells = wait_ink(&session, &wake, "414243444546");
        // Sarma yine de sarmaya devam ediyor — süzme iğneyi yemiyor.
        assert!(glyph_text(&cells).contains("1b5b3230307e"), "{cells:?}");
    }

    #[test]
    fn paste_writes_raw_when_bracketed_mode_unset() {
        // 2004 açılmadı: `paste()` ham yazar, sarma baytı gitmez.
        //
        // Çocuk `od`: stdin'i hex'e döküp stdout'a yazar. Yapıştırılan
        // baytların **çocuğa ne olarak gittiğini** karenin mürekkebinden
        // okumanın yolu — sarma baytları (`\e[200~`) alacritty tarafından
        // yutulduğu için yankı karesinden okunamaz, `od` dökümünden okunur.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("od -An -tx1", Arc::clone(&wake));

        // Kip kapalı: `paste()`'ten önce de kapalı olmalı ki sınama yanlış
        // dalı doğrulamasın. `od` açılışta satır basmaz, canlılık çapası yok —
        // gerek de yok: bayt geldiyse çocuk yaşıyor, gelmediyse zaman aşımı var.
        assert!(!session.bracketed_paste());

        session.paste(PASTE_PAYLOAD.to_vec());
        let cells = wait_ink(&session, &wake, "4142");
        assert!(
            !glyph_text(&cells).contains("1b"),
            "sarılmamış yapıştırmada kaçış baytı olmamalı: {cells:?}"
        );
    }

    #[test]
    fn paste_empty_writes_nothing() {
        // Boş yapıştırma sessizdir: ham dal da sarma dalı da PTY'ye gitmez —
        // sarma dalında bile `\e[200~\e[201~` boş çifti yazılmamalı, sıfır
        // baytlık `Msg::Input` yazıcıyı kilitlerdi (`Adapter::reply`).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf '\\033[?2004h'; sleep 5", Arc::clone(&wake));

        wait_bracketed_mode(&session);

        // **Önce akış durulsun** (`wait_settled`): tek bir `None` beklemek
        // yarışırdı — gecikmiş `printf` karesi "yapıştırma kare doğurdu" diye
        // okunurdu.
        wait_settled(&session);

        session.paste(Vec::new());
        // Sessizliğin kanıtı yankı: PTY'ye bayt gitseydi satır disiplini onu
        // yankılardı (ECHO açık) ve yankı bir kare doğururdu. Yankının tur
        // atması için bir nefes bekleniyor; sonra kare olmamalı.
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "boş paste kare doğurdu — PTY'ye bayt gitmiş"
        );
    }

    #[test]
    fn zero_size_is_ignored() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));

        // Açılış karesi: grid boş ama pencere bir kez boyanmalı (bayrak
        // `Adapter::new`'da `true` başlıyor).
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        // Simge durumuna inen pencere `bounds()`'tan sıfır hesaplatabilir.
        // Bu boyut grid'e HİÇ ulaşmamalı: 1 sütuna kırpmak alacritty'de
        // panik yerine daha kötüsünü yapar, geçmişi kalıcı olarak yok eder.
        // Hasar işaretlenmemesi resize'ın hiç olmadığının kanıtı.
        // Dönüş değeri de sözleşmenin parçası: çağıran hücre piksel boyutunu
        // buna bakarak uyguluyor.
        assert!(!session.resize(0, 24, (9, 18)));
        assert!(!session.resize(80, 0, (9, 18)));
        assert!(!session.resize(0, 0, (9, 18)));
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "dejenere boyut grid'e ulaştı"
        );

        // Gerçek boyut değişimi hasar işaretler.
        assert!(session.resize(80, 24, (9, 18)));
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        // Aynı boyut ikinci kez: değişiklik yok, hasar yok.
        assert!(!session.resize(80, 24, (9, 18)));
        assert!(frame_if_damaged(&session, |_| ()).is_none());
        // Yalnız hücre piksel boyutu değişse de bu bir değişikliktir: PTY'ye
        // giden `TIOCSWINSZ` onu taşıyor (Retina'ya taşınan pencere).
        assert!(session.resize(80, 24, (18, 36)));
    }

    /// Seçim ucu kurucusu — sınama gövdelerini kısaltır. Hücre aralığı ile
    /// yarısı ayrı ayrı okunsun diye konum ve yarı **ayrı argüman**.
    fn at(col: u16, row: u16, half: CellHalf) -> SelectionPoint {
        SelectionPoint { col, row, half }
    }

    /// Yarı sınamalarının ortak sahnesi: satırın başında kırmızı zeminli
    /// `araba`. Beş hücre çizildiyse metin grid'dedir.
    fn word_session() -> Session {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033[41maraba\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );
        assert_eq!(wait_cells(&session, &wake, 5).len(), 5);
        session
    }

    /// Kelime sınamalarının sahnesi: ayırıcıların (`WORD_SEPARATORS`) ve
    /// kelime karakterlerinin ikisini de taşıyan tek satır, kırmızı zeminli
    /// ki 13 hücrenin 13'ü de sayılabilsin.
    fn separator_session() -> Session {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033[41ma b.c/d:e f=g\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );
        assert_eq!(wait_cells(&session, &wake, 13).len(), 13);
        session
    }

    #[test]
    fn a_double_click_selects_the_word_under_the_pointer() {
        let session = separator_session();
        // `.`, `/` ve `:` kelimenin içinde: yol, `host:port`, `dosya.rs:42`
        // tek çift tıkla gelir (Karar 5).
        session.set_selection(
            SelectKind::Word,
            at(4, 0, CellHalf::Left),
            at(4, 0, CellHalf::Left),
        );
        assert_eq!(session.selection_text().as_deref(), Some("b.c/d:e"));
        // `=` ayırıcı: `KEY=value`'da değer tek başına.
        session.set_selection(
            SelectKind::Word,
            at(12, 0, CellHalf::Left),
            at(12, 0, CellHalf::Left),
        );
        assert_eq!(session.selection_text().as_deref(), Some("g"));
        session.set_selection(
            SelectKind::Word,
            at(10, 0, CellHalf::Right),
            at(10, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("f"));
    }

    #[test]
    fn a_word_drag_grows_in_whole_words() {
        let session = separator_session();
        session.set_selection(
            SelectKind::Word,
            at(4, 0, CellHalf::Left),
            at(4, 0, CellHalf::Left),
        );
        // Uç `f`'nin sol yarısında: kelime adımı onu bütün alır, `=`'de durur.
        session.update_selection(at(10, 0, CellHalf::Left));
        assert_eq!(session.selection_text().as_deref(), Some("b.c/d:e f"));
        // Geriye sürükleme de kelime adımıyla: `a` bütün gelir, çapanın
        // kelimesi de kalır.
        session.update_selection(at(0, 0, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("a b.c/d:e"));
    }

    #[test]
    fn a_shift_click_extends_the_selection_and_keeps_its_kind() {
        let session = separator_session();
        session.set_selection(
            SelectKind::Word,
            at(2, 0, CellHalf::Left),
            at(2, 0, CellHalf::Left),
        );
        // Uç `g`'nin sol yarısında; `Word` onu bütün alır — `Simple` olsaydı
        // sol yarıda biten uç `g`'yi dışarıda bırakırdı.
        session.extend_selection(at(12, 0, CellHalf::Left));
        assert_eq!(session.selection_text().as_deref(), Some("b.c/d:e f=g"));
    }

    #[test]
    fn a_shift_click_without_a_selection_starts_a_simple_one() {
        let session = separator_session();
        session.extend_selection(at(2, 0, CellHalf::Left));
        // Sürüklemesiz tık gibi: boş seçim, kopyalanacak metin yok.
        assert_eq!(session.selection_text(), None);
        // Ama çapa orada: sürükleme oradan harf adımıyla büyür.
        session.update_selection(at(4, 0, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("b.c"));
    }

    /// Dock'ta kelime ızgaradaki kelimeyle **aynı şey** (031 Karar 5): aynı
    /// dizgi iki yüzeyde, her sütunda aynı aralığı veriyor — ayırıcılar,
    /// ayırıcının üstüne çift tıklama ve parantez eşleme dahil. Izgaranın
    /// cevabı alacritty'nin `Semantic`'inden, dock'unki onun kopyasından
    /// (`dock::selection_range`); ikisi ayrıştığı gün bu sınama kırmızı.
    #[test]
    fn a_dock_word_matches_the_grid_word() {
        for line in ["a b.c/d:e f=g", "f (a (b) c) [x] <y>", "k=v;x  y|z"] {
            let wake = Arc::new(TestWake::default());
            let session = spawn_session(
                &format!("printf '\\033[41m{line}\\033[0m'; sleep 5"),
                Arc::clone(&wake),
            );
            let len = line.chars().count();
            assert_eq!(wait_cells(&session, &wake, len).len(), len);
            for col in 0..len {
                let at_col = at(col as u16, 0, CellHalf::Left);
                session.set_selection(SelectKind::Word, at_col, at_col);
                let grid = session.selection_text().unwrap_or_default();
                let point = DockPoint {
                    index: col,
                    half: CellHalf::Left,
                };
                let (start, end) =
                    dock::selection_range(line, SelectKind::Word, point, point, false);
                let dock: String = line.chars().skip(start).take(end - start).collect();
                assert_eq!(dock, grid, "{line:?} sütun {col}");
            }
            session.shutdown();
        }
    }

    /// Dock seçiminin sahnesi: ızgarada `hello world`, altında çıpalı bir
    /// prompt ve aynası `echo foo bar` olan giriş satırı; dock bir kez
    /// **çizilmiş** — isabet testi çizilen pencereye bakıyor.
    fn dock_selection_session() -> Session {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "printf 'hello world\\r\\n{}echo foo bar{}'; sleep 5",
                anchored_prompt(1),
                mirror("ZWNobyBmb28gYmFy", 12),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        draw_dock(&session);
        session
    }

    /// Bir kare ve dock'u: üretimdeki sıra (`frame()` caret'in sahibini
    /// veriyor, dock onu alıyor ve penceresinin izini bırakıyor). Dönüşün
    /// ikinci yarısı seçimin görsel satır başına koşuları.
    fn draw_dock(session: &Session) -> (Dock, Vec<SelectionRun>) {
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        let mut runs = Vec::new();
        let dock = session.dock(
            DockCols {
                grid: 40,
                context: 40,
            },
            cursor.input_rows,
            &mut DockState::default(),
            &mut DockContext::default(),
            cursor.caret_in_dock,
            &mut runs,
            &mut Clusters::default(),
            |_| (),
            |_| (),
        );
        (dock, runs)
    }

    /// Dock'un giriş bloğunda `BUFFER`'ın `index`. karakterinin hücresi
    /// (ASCII, tek satır) ve yarısı.
    fn dock_point(index: u16, half: CellHalf) -> SelectionPoint {
        SelectionPoint {
            col: dock::TEXT_COL + index,
            row: 0,
            half,
        }
    }

    /// **Uzak oturumda giriş satırı yok** (036 R5.1, R5.3, R5.4): dock'lu
    /// pencerede `input_rows == 0`, caret ızgarada, dock'un işareti yok ve
    /// dock'a tık ne seçim kuruyor ne kabuğa komut gönderiyor — aynı nokta
    /// `set_remote`'tan önce `foo`'yu seçiyordu. Alternatif ekranda sayı
    /// bugünkü gibi `1`.
    /// 037 Karar 7: yüklemenin iki kapısı — hedef, uzak dizin ve nesil tek
    /// okumada (`remote_target`), durum satırı yalnız değişince kare istiyor
    /// ve dock'un üst çizgisini çubuğa çeviriyor.
    #[test]
    fn an_upload_reads_the_remote_target_and_paints_its_row() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "stty -echo; printf '{}'; read _; printf '\\033]133;C\\007'; sleep 5",
                anchored_prompt(1),
            ),
            Arc::clone(&wake),
        );
        assert_eq!(session.remote_target(), None, "yerelde hedef yok");
        session.write(b"\n");
        assert_eq!(wake.wait_commands(1, Duration::from_secs(5)), 1);
        let command = session.running_command().expect("`C`'den sonra koşuyor");
        assert_eq!(
            session.remote_target(),
            None,
            "komut koşuyor ama uzak değil"
        );
        assert!(session.set_remote(command, Some(&RemoteTarget::ssh("prod"))));
        assert_eq!(
            session.remote_target(),
            Some((command, RemoteTarget::ssh("prod"), String::new()))
        );

        let transfer = Transfer {
            host: "prod".into(),
            mark: HostMark::Production,
            body: "↑ a".into(),
            controls: crate::TransferControls::default(),
            progress: Some(5_000),
            ..Transfer::default()
        };
        assert!(session.set_transfer(Some(&transfer)));
        assert!(
            !session.set_transfer(Some(&transfer)),
            "aynı satır kare istemiyor"
        );
        let (dock, _) = draw_dock(&session);
        assert_eq!(dock.progress, Some(5_000));
        // Dolan kısım `info`, işaret boş izde (037 phase-7).
        assert_eq!(dock.edge, session.theme().info_linear());
        assert_eq!(dock.track, session.theme().error_linear());
        assert!(session.set_transfer(None));
        assert!(!session.set_transfer(None));
        let (dock, _) = draw_dock(&session);
        assert_eq!(dock.progress, None);
    }

    #[test]
    fn a_remote_session_has_no_input_row_in_the_dock() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "stty -echo; printf 'hello world\\r\\n{}echo foo bar{}'; read _; \
                 printf '\\033]133;C\\007'; read _; printf '\\033[?1049h'; sleep 5",
                anchored_prompt(1),
                mirror("ZWNobyBmb28gYmFy", 12),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        let (dock, _) = draw_dock(&session);
        assert!(dock.sigil.is_some(), "yerelde işaret var");
        session.dock_select(SelectKind::Word, dock_point(6, CellHalf::Left));
        assert_eq!(session.selection_text().as_deref(), Some("foo"));
        session.clear_selection();

        session.write(b"\n");
        assert_eq!(wake.wait_commands(1, Duration::from_secs(5)), 1);
        let command = session.running_command().expect("`C`'den sonra koşuyor");
        assert!(session.set_remote(command, Some(&RemoteTarget::ssh("prod"))));

        let (_, cursor) = frame_until(&session, BUDGET, |_, _| true);
        assert_eq!(cursor.input_rows, 0, "uzakta giriş satırı yok: {cursor:?}");
        assert!(!cursor.caret_in_dock, "uzakta caret ızgarada: {cursor:?}");
        assert!(cursor.visible, "ızgaranın imleci çiziliyor: {cursor:?}");
        let (dock, runs) = draw_dock(&session);
        assert_eq!(dock.sigil, None, "bağlam satırına işaret oturmamalı");
        assert_eq!(dock.caret, None);
        assert!(runs.is_empty());

        let generation = session.key_gen.load(Ordering::Acquire);
        session.dock_select(SelectKind::Word, dock_point(6, CellHalf::Left));
        assert_eq!(
            session.selection_text(),
            None,
            "giriş satırı yokken seçim yok"
        );
        session.dock_click();
        assert!(!session.dock_scroll(1), "kaydırılacak giriş bloğu yok");
        assert_eq!(
            session.key_gen.load(Ordering::Acquire),
            generation,
            "dock'a tık kabuğa hiçbir şey göndermemeli"
        );

        session.write(b"\n");
        let (_, cursor) = frame_until(&session, BUDGET, |_, cursor| {
            cursor.content_rows == cursor.rows
        });
        assert_eq!(cursor.input_rows, 1, "alternatif ekranda bugünkü değer");
    }

    #[test]
    fn a_marked_remote_host_paints_the_dock_edge_in_its_color() {
        // 037 Karar 2, 3: işaret `set_remote`'ta ve `set_host_marks`'ta
        // çözülüyor; dock'un üst çizgisi işaretin renginde, işaretsizde
        // bugünkü `info`. Değişen işaret kare istiyor, aynı liste istemiyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "stty -echo; printf '{}echo{}'; read _; printf '\\033]133;C\\007'; sleep 5",
                anchored_prompt(1),
                mirror("ZWNobw==", 4),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        assert!(
            !session.set_host_marks(&[HostRule {
                pattern: "prod-*".to_owned(),
                mark: HostMark::Staging,
            }]),
            "yerelde görünen bir şey değişmiyor"
        );
        session.write(b"\n");
        assert_eq!(wake.wait_commands(1, Duration::from_secs(5)), 1);
        let command = session.running_command().expect("`C`'den sonra koşuyor");
        assert_eq!(session.remote_mark(), None, "yerel");
        assert!(session.set_remote(command, Some(&RemoteTarget::ssh("deploy@prod-web"))));
        let theme = session.theme();
        assert_eq!(draw_dock(&session).0.edge, theme.warning_linear());
        // Sekmenin noktası ve menü aynı çözümü okuyor (037 Karar 4, 5).
        assert_eq!(
            session.remote_mark(),
            Some(("deploy@prod-web".to_owned(), HostMark::Staging))
        );

        let wakes = wake.state.lock().unwrap().wakes;
        let production = [HostRule {
            pattern: "prod-web".to_owned(),
            mark: HostMark::Production,
        }];
        assert!(session.set_host_marks(&production));
        assert!(
            wake.state.lock().unwrap().wakes > wakes,
            "işaret değişimi kare istemeli"
        );
        assert_eq!(draw_dock(&session).0.edge, theme.error_linear());
        assert_eq!(
            session.remote_mark().map(|(_, mark)| mark),
            Some(HostMark::Production)
        );
        assert!(!session.set_host_marks(&production), "aynı liste no-op");
        assert!(session.set_host_marks(&[]));
        assert_eq!(draw_dock(&session).0.edge, theme.info_linear());
    }

    #[test]
    fn the_dock_line_is_selected_by_drag_double_and_triple_click() {
        let session = dock_selection_session();
        // Çift tık `foo`'nun içinde.
        session.dock_select(SelectKind::Word, dock_point(6, CellHalf::Left));
        assert_eq!(session.selection_text().as_deref(), Some("foo"));
        // Üçlü tık bütün `BUFFER`, satır sonu **olmadan**: kabuğa geri
        // yapıştırılan satır çalışmasın.
        session.dock_select(SelectKind::Line, dock_point(1, CellHalf::Left));
        assert_eq!(session.selection_text().as_deref(), Some("echo foo bar"));
        // Sürükleme harf adımıyla; sürüklemesiz tık boş.
        session.dock_select(SelectKind::Simple, dock_point(0, CellHalf::Left));
        assert_eq!(session.selection_text(), None);
        session.dock_drag(dock_point(3, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("echo"));
        // Shift+tık ucu taşıyor, çapa yerinde.
        session.dock_extend(dock_point(7, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("echo foo"));
        // Çizim de aynı aralığı söylüyor: `e`'den ikinci `o`'ya.
        assert_eq!(
            draw_dock(&session).1,
            [SelectionRun {
                row: 0,
                first: dock::TEXT_COL,
                last: dock::TEXT_COL + 7,
            }]
        );
        session.shutdown();
    }

    /// Sarılan uzun satır (032 phase-3): `frame()` dock'un satır sayısını
    /// ızgaranın payıyla kırpıyor, dock'un fare yolu ikinci ve üçüncü görsel
    /// satırı seçebiliyor ve vurgu satır başına bir koşu.
    #[test]
    fn a_wrapped_dock_line_grows_to_its_ceiling_and_selects_across_rows() {
        let wake = Arc::new(TestWake::default());
        // 40 sütun, metne 38: 38 `a`, 38 `b`, 24 `c` — üç görsel satır, caret
        // sonda. Izgara on satır, yani yarısı beş.
        let session = spawn_docked_session(
            &format!(
                "printf '{}{}'; sleep 5",
                anchored_prompt(1),
                mirror(
                    "YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFiYmJiYmJiYmJiYmJi\
                     YmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmNjY2NjY2NjY2NjY2NjY2NjY2NjY2NjYw==",
                    100
                ),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        let rows = |share| {
            session
                .frame(
                    |_| (),
                    |_| (),
                    &mut Blocks::default(),
                    &mut SelectionRuns::default(),
                    &mut SearchRuns::default(),
                    &mut Clusters::default(),
                    ScrollGlide::default(),
                    DockBudget { share, cols: 40 },
                )
                .input_rows
        };
        assert_eq!(rows(0.5), 3, "tavanın altında bütün satırlar");
        assert_eq!(rows(0.2), 2, "tavan ızgaranın payı");
        assert_eq!(rows(0.0), 1, "en az bir satır");

        let (_, runs) = draw_dock(&session);
        assert!(runs.is_empty());
        let at = |col, row, half| SelectionPoint {
            col: dock::TEXT_COL + col,
            row,
            half,
        };
        // İkinci satırın başından üçüncünün ikinci harfine sürükleme.
        session.dock_select(SelectKind::Simple, at(0, 1, CellHalf::Left));
        session.dock_drag(at(1, 2, CellHalf::Right));
        let expected = format!("{}cc", "b".repeat(38));
        assert_eq!(session.selection_text(), Some(expected));
        let (_, runs) = draw_dock(&session);
        assert_eq!(
            runs,
            [
                SelectionRun {
                    row: 1,
                    first: dock::TEXT_COL,
                    last: dock::TEXT_COL + 37,
                },
                SelectionRun {
                    row: 2,
                    first: dock::TEXT_COL,
                    last: dock::TEXT_COL + 1,
                },
            ]
        );
        // Üçlü tık mantıksal satırı alıyor: satır sonu yok, yani `BUFFER`'ın
        // tamamı — sarılmış görsel satırlarıyla.
        session.dock_select(SelectKind::Line, at(5, 2, CellHalf::Left));
        assert_eq!(session.selection_text().map(|text| text.len()), Some(100));
        session.shutdown();
    }

    #[test]
    fn one_selection_per_window() {
        let session = dock_selection_session();
        let hello = at(1, 0, CellHalf::Left);
        session.set_selection(SelectKind::Word, hello, hello);
        assert_eq!(session.selection_text().as_deref(), Some("hello"));
        // Dock'ta başlamak ızgaranınkini kaldırıyor.
        session.dock_select(SelectKind::Word, dock_point(10, CellHalf::Left));
        assert_eq!(session.term.lock().selection_to_string(), None);
        assert_eq!(session.selection_text().as_deref(), Some("bar"));
        // Izgarada başlamak dock'unkini.
        session.set_selection(SelectKind::Word, hello, hello);
        assert_eq!(session.dock_selection_text(), None);
        assert_eq!(session.selection_text().as_deref(), Some("hello"));
        // Dock'a sürüklemesiz tık da "başka yere tıklamak".
        session.dock_select(SelectKind::Simple, dock_point(0, CellHalf::Left));
        assert_eq!(session.selection_text(), None);
        session.shutdown();
    }

    #[test]
    fn input_clears_the_dock_selection() {
        let session = dock_selection_session();
        session.dock_select(SelectKind::Word, dock_point(6, CellHalf::Left));
        assert_eq!(session.selection_text().as_deref(), Some("foo"));
        // Tek huni `send_input`: yazma, yapıştırma ve ok aynı kapıdan.
        session.write(b" ");
        assert_eq!(session.selection_text(), None);
        session.shutdown();
    }

    #[test]
    fn select_all_goes_to_the_dock_when_it_owns_the_caret() {
        let session = dock_selection_session();
        session.select_all();
        assert_eq!(session.selection_text().as_deref(), Some("echo foo bar"));
        assert_eq!(session.term.lock().selection_to_string(), None);
        session.shutdown();
    }

    /// **Tekerlek dock'un dikey penceresini kaydırıyor** (032 phase-4): tavanı
    /// aşan girişte caret'in dışındaki satırlara fare de ulaşıyor. Uçta
    /// kırpılıyor, taşmayan dock olayı ızgaraya bırakıyor; caret'in yeri
    /// değişince pencere caret'e dönüyor (bekçisi `shell.rs`'in `apply_dock`'u).
    #[test]
    fn the_wheel_scrolls_an_overflowing_dock() {
        let wake = Arc::new(TestWake::default());
        // Kırk sütunda, metin sütunundan sonra 38 harf: 231 harf yedi satır,
        // tavan (on satırın yarısı) beş.
        let session = spawn_docked_session(
            &format!(
                "printf '{}{}'; sleep 5",
                anchored_prompt(1),
                mirror(&"YWFh".repeat(77), 231),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        let (dock, _) = draw_dock(&session);
        assert!(dock.caret.is_some() && dock.sigil.is_none(), "{dock:?}");
        let top = || lock(&session.dock_window).map(|window| window.top);
        assert_eq!(top(), Some(2), "pencere caret'i izliyor");

        assert!(session.dock_scroll(1), "taşan dock tekerleği almalı");
        assert_eq!(top(), Some(1), "iz hemen: sürüklemenin ucu yeni pencerede");
        assert!(session.dock_scroll(10), "uçta da dock'un");
        let (dock, _) = draw_dock(&session);
        assert_eq!(top(), Some(0));
        assert_eq!(dock.caret, None, "caret pencerenin dışında");
        assert!(dock.sigil.is_some(), "ilk satır ekranda");
        assert!(session.dock_scroll(-100));
        draw_dock(&session);
        assert_eq!(top(), Some(2));
        session.shutdown();

        // Taşmayan dock tekerleği ızgaraya bırakıyor.
        let session = dock_selection_session();
        assert!(!session.dock_scroll(1));
        session.shutdown();
    }

    /// İsabet testi **çizilen** pencereye bakıyor: ayna o kareden beri
    /// başka bir `BUFFER`'a geçtiyse ekranda görülmemiş metne seçim kurulmuyor.
    #[test]
    fn a_click_on_a_line_that_was_not_drawn_selects_nothing() {
        let session = dock_selection_session();
        {
            let mut window = lock(&session.dock_window);
            let drawn = window.as_mut().expect("iz yok");
            drawn.buffer_bytes += 1;
        }
        session.dock_select(SelectKind::Word, dock_point(6, CellHalf::Left));
        assert_eq!(session.selection_text(), None);
        session.shutdown();
    }

    #[test]
    fn a_triple_click_selects_the_whole_wrapped_line() {
        let wake = Arc::new(TestWake::default());
        // On sütunda on beş harf: satır sarılıyor (`WRAPLINE`), yani iki
        // fiziksel satır tek mantıksal satır.
        let session = spawn_with_cols(
            sh("printf '\\033[41mabcdefghijklmno\\033[0m'; sleep 5"),
            10,
            Arc::clone(&wake),
        );
        assert_eq!(wait_cells(&session, &wake, 15).len(), 15);
        session.set_selection(
            SelectKind::Line,
            at(2, 0, CellHalf::Left),
            at(2, 0, CellHalf::Left),
        );
        // Satır seçimi satır sonunu da taşıyor (alacritty'nin `Lines`'ı,
        // Terminal.app'in üçlü tıklaması gibi) — ama sarılmanın yerinde satır
        // sonu **yok**: iki fiziksel satır tek satır olarak geliyor.
        assert_eq!(
            session.selection_text().as_deref(),
            Some("abcdefghijklmno\n")
        );
        // Alt yarıdan da aynı satır.
        session.set_selection(
            SelectKind::Line,
            at(1, 1, CellHalf::Left),
            at(1, 1, CellHalf::Left),
        );
        assert_eq!(
            session.selection_text().as_deref(),
            Some("abcdefghijklmno\n")
        );
    }

    #[test]
    fn select_all_covers_the_history_too() {
        let wake = Arc::new(TestWake::default());
        // İlk satır geçmişe itilecek kadar satır: seçim ekranın değil
        // geçmişin tepesinden başlamalı.
        let session = spawn_session(
            "printf '\\033[41mtop\\033[0m'; for i in $(seq 1 40); do echo; done; \
             printf '\\033[41mend\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );
        // `top` artık geçmişte: ekranda yalnız `end`'in üç kırmızı hücresi.
        wait_frame(&session, &wake, |cells| {
            cells.iter().any(|cell| cell.ch == Some('d'))
        });
        session.select_all();
        let text = session.selection_text().unwrap_or_default();
        assert!(text.starts_with("top"), "{text:?}");
        assert!(text.trim_end().ends_with("end"), "{text:?}");
    }

    #[test]
    fn selection_text_returns_selected_range() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033[41mhello world\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );

        // Seçimden önce metin yok.
        assert_eq!(session.selection_text(), None);
        // Çapa: 11 kırmızı hücre geldiyse metin grid'de.
        assert_eq!(wait_cells(&session, &wake, 11).len(), 11);

        // Başlangıç ucunun sol yarısı hücreyi katar, bitiş ucunun sağ yarısı
        // katar: beş harfin beşi de içeride — eski davranışla aynı sonuç,
        // çünkü eskiden yanlar sabit bu ikisiydi.
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(4, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("hello"));
        // Uçlar sırasız verilebilir: tersi aynı metni verir. Yarının sıraya
        // göre atanmadığının kanıtı da bu — ters çevrilen yarılar değil.
        session.set_selection(
            SelectKind::Simple,
            at(4, 0, CellHalf::Right),
            at(0, 0, CellHalf::Left),
        );
        assert_eq!(session.selection_text().as_deref(), Some("hello"));
        // Temizleyince metin de gider.
        session.clear_selection();
        assert_eq!(session.selection_text(), None);
    }

    #[test]
    fn selection_text_follows_the_half_of_the_left_end() {
        // 006'da bildirilen kusur, birebir: kullanıcı `araba`nın `raba`
        // kısmını seçiyor, kopyaya `araba` geliyordu. Sebep yarının hiç
        // sorulmamasıydı: hedeflediği harfin **hemen soluna** basan biri o
        // pikseli bir önceki hücrenin sağ yarısına düşürür ve sabit `Left`
        // yüzünden o hücre de aralığa girer. Aynı hücre aralığı, iki farklı
        // yarı, iki farklı metin — aralığı belirleyen şey yarı.
        let session = word_session();

        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Right),
            at(4, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("raba"));
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(4, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("araba"));
    }

    #[test]
    fn selection_text_follows_the_half_of_the_right_end() {
        // Bitiş ucu **aynaya** bakar: sağ yarı kendi hücresini seçime katar,
        // sol yarı sınırı o hücrenin başına çeker. Yani fare bir hücrenin
        // ortasını geçtiği an o hücre yanar — iki uçta da kural bu.
        let session = word_session();

        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(4, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("araba"));
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(4, 0, CellHalf::Left),
        );
        assert_eq!(session.selection_text().as_deref(), Some("arab"));
    }

    #[test]
    fn selection_with_both_ends_in_the_right_half_covers_nothing() {
        // İki uç da aynı hücrenin sağ yarısı — sürüklemesiz tık budur. Seçim
        // **boş** doğar (`is_empty` iki ucu da dışarıda bırakır), yani
        // aralık yok ve `selection_text()` `None`: sağ yarıya basmak hücreyi
        // seçime katmaz, tek başına hiçbir şeyi de katmaz. Kopya kapısının
        // `None` kolu bunun üstünde durur — metin yoksa pano el değmeden
        // kalır.
        let session = word_session();

        session.set_selection(
            SelectKind::Simple,
            at(2, 0, CellHalf::Right),
            at(2, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text(), None);
        // Sol yarısı da boş: iki uç birbirinin **aynısı** olduğu sürece
        // seçim doğmaz, yarı ne olursa olsun.
        session.set_selection(
            SelectKind::Simple,
            at(2, 0, CellHalf::Left),
            at(2, 0, CellHalf::Left),
        );
        assert_eq!(session.selection_text(), None);
    }

    #[test]
    fn selection_text_spans_wrapped_lines_without_newline() {
        // 40 sütunluk grid'e 45 karakter: satır sarıyor (WRAPLINE) ve seçim
        // metni satır sonu koymadan birleştiriyor.
        let text = "0123456789".repeat(4) + "01234";
        assert_eq!(text.len(), 45);
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!("printf '\\033[41m{text}\\033[0m'; sleep 5"),
            Arc::clone(&wake),
        );

        assert_eq!(wait_cells(&session, &wake, 45).len(), 45);
        // İki uç da kendi hücresini katan yarıda: 35. sütundan sarılan
        // satırın 4. sütununa kadar on hücre (eski sabit yanlarla aynı sonuç).
        session.set_selection(
            SelectKind::Simple,
            at(35, 0, CellHalf::Left),
            at(4, 1, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("5678901234"));
    }

    #[test]
    fn selection_text_skips_wide_char_spacers() {
        // `あ` iki hücrelik: ikincisi `WIDE_CHAR_SPACER` ve metne girmemeli.
        // Çapa dört arka plan hücresi — spacer'ın bg'si şablondan geliyor —
        // yani geniş karakter gerçekten iki hücre kaplamış.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf '\\033[41maあb\\033[0m'; sleep 5", Arc::clone(&wake));

        assert_eq!(wait_cells(&session, &wake, 4).len(), 4);
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(3, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("aあb"));
    }

    #[test]
    fn selection_redraws_only_when_the_drawn_range_changes() {
        // Kapı çizilen aralığa bakar, uçlara değil. Yarı artık ucun kendi
        // özelliği olduğu için aynı aralık iki farklı uç çiftinden doğabilir:
        // sürükleme hücre sınırını geçerken `(2, Right)` → `(3, Left)` olur ve
        // ikisi de 2. sütunda biter. Uçları karşılaştıran kapı bu geçişte
        // ekrana hiçbir şey eklemeyen bir kare isterdi. Sürüklemenin üretim
        // yolu `update_selection` ve kapısı aynı; ikisi de aşağıda sınanıyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        // Sürüklemesiz tık: seçim boş doğar, önceki seçim de yoktu — çizilecek
        // bir şey değişmedi.
        session.set_selection(
            SelectKind::Simple,
            at(2, 0, CellHalf::Right),
            at(2, 0, CellHalf::Right),
        );
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "boş seçim kare istememeli"
        );

        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(2, 0, CellHalf::Right),
        );
        assert!(
            frame_if_damaged(&session, |_| ()).is_some(),
            "yeni aralık kare istemeli"
        );
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        // Hücre sınırı geçildi, aralık aynı: 2. sütunda bitiyor.
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(3, 0, CellHalf::Left),
        );
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "aynı aralık kare istememeli"
        );
        // Sürüklemenin kendi yolu: uç ters yönde geri döndü, aralık yine aynı.
        // Kapı `set_selection`'da kalıp burada unutulsaydı her sürükleme olayı
        // kare isterdi.
        session.update_selection(at(2, 0, CellHalf::Right));
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "aynı aralığa sürükleme kare istememeli"
        );
        session.update_selection(at(4, 0, CellHalf::Right));
        assert!(
            frame_if_damaged(&session, |_| ()).is_some(),
            "aralığı büyüten sürükleme kare istemeli"
        );

        // Temizle, sonra yine sürüklemesiz tık: saklanan seçim boş, temizlemek
        // ekrandan bir şey silmez.
        session.clear_selection();
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        session.set_selection(
            SelectKind::Simple,
            at(1, 0, CellHalf::Left),
            at(1, 0, CellHalf::Left),
        );
        session.clear_selection();
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "boş seçimi temizlemek kare istememeli"
        );
    }

    #[test]
    fn drag_between_halves_across_a_line_break_selects_nothing() {
        // Satır sonunun sağ yarısından alt satırın başının sol yarısına: iki uç
        // de kendi hücresini dışarıda bırakır, arada hücre yok. alacritty
        // bunu boş saymıyor — önce bitişi üst satırın son hücresine geri
        // alıyor, uçlar eşitlendiği için başlangıcı kaydırmıyor ve **son
        // hücreyi** seçiyor. Yarının kuralına göre hiçbir şey seçilmemeli.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        session.set_selection(
            SelectKind::Simple,
            at(39, 0, CellHalf::Right),
            at(0, 1, CellHalf::Left),
        );
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "boş seçim kare istememeli"
        );
        assert_eq!(session.selection_text(), None);
    }

    #[test]
    fn selection_scrolled_into_history_is_not_drawn() {
        // Çıktı seçili satırı görünür pencerenin üstüne itti: aralık grid'de
        // duruyor ama ekranda değil. Kapı ve temizleme görünür pencereye bakar
        // — geçmişteki bir aralığı "çizili" saymak onun yerine gelen ilk tıkta
        // ekrana hiçbir şey eklemeyen bir kare isterdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033[41mx\\033[0m'; read _; seq 1 30; sleep 5",
            Arc::clone(&wake),
        );
        assert_eq!(wait_cells(&session, &wake, 1).len(), 1);
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(0, 0, CellHalf::Right),
        );
        assert!(frame_if_damaged(&session, |_| ()).is_some());

        // `read` satır sonunu bekliyor; gelince 30 satır `x`'i geçmişe iter.
        // Hazır: son satır (`30`) görünür.
        session.write(b"\n");
        wait_seq_tail(&session, &wake);

        session.set_selection(
            SelectKind::Simple,
            at(5, 5, CellHalf::Left),
            at(5, 5, CellHalf::Left),
        );
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "geçmişteki seçimin yerine boş seçim kare istememeli"
        );
        // Kare istenmedi ama seçim **değişti**: iki taraf da görünmez olduğu için
        // kapı "aynı" dedi, yine de boş seçim saklanmalı. Saklanmasaydı Cmd-C
        // ekranda olmayan eski `x`'i kopyalardı — `selection_text` görünürlüğe
        // bakmıyor.
        assert_eq!(session.selection_text(), None);
        session.clear_selection();
        assert!(frame_if_damaged(&session, |_| ()).is_none());
    }

    #[test]
    fn selection_highlights_its_first_and_last_cell() {
        // Koşu aralığın **iki ucunu da** kapsar. Blok imlecin sınır istisnası
        // (`contains_cell`) yalnız imlecin durduğu hücre içindir; imleç
        // noktası yerine hücrenin kendi noktası verilince istisna her sınır
        // hücresine uygulanıyor ve seçimin ilk ile son harfi hiç
        // vurgulanmıyordu. İmleç burada `araba`'nın sağında, 5. sütunda.
        let session = word_session();
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(4, 0, CellHalf::Right),
        );
        let (_, runs, _) = runs_if_damaged(&session).expect("seçim kare istemeli");
        assert_eq!(runs, vec![run(0, 0, 4)]);
    }

    #[test]
    fn selection_change_marks_dirty() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));
        // Açılış karesi + sessizlik: shell çıktı üretmiyor.
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(2, 0, CellHalf::Right),
        );
        assert!(
            frame_if_damaged(&session, |_| ()).is_some(),
            "seçim kirli bayrağını dikmeli"
        );
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        session.clear_selection();
        assert!(
            frame_if_damaged(&session, |_| ()).is_some(),
            "temizleme de kare istemeli"
        );
        assert!(frame_if_damaged(&session, |_| ()).is_none());
        // Boş seçimi temizlemek sessiz: bayrak dikilmez, kare istenmez.
        session.clear_selection();
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "boş temizleme kare istememeli"
        );
    }

    #[test]
    fn selected_cells_give_one_run_and_drop_their_ground() {
        // Vurgu hücreyi boyamıyor, **koşu** veriyor (031 phase-2): seçili
        // üç hücrenin zemini sınırdan düşüyor (seçimin şeklinin altında
        // kalacaktı), metinleri kendi renklerinde kalıyor. Reçetede yalnız
        // seçili aralık bg'li (`\033[41mell\033[0m`); `h` ile `o` varsayılan
        // bg'li.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf 'h\\033[41mell\\033[0mo'; sleep 5",
            Arc::clone(&wake),
        );
        let cells = wait_frame(&session, &wake, |cells| {
            cells.iter().filter_map(|c| c.ch).collect::<String>() == "hello"
        });
        assert_eq!(backgrounds(&cells).count(), 3, "{cells:?}");
        let plain: HashMap<_, _> = cells.iter().map(|c| (c.col, c.fg)).collect();
        session.set_selection(
            SelectKind::Simple,
            at(1, 0, CellHalf::Left),
            at(3, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("ell"));

        let (next, runs, _) = runs_if_damaged(&session).expect("seçim kare istemeli");
        assert_eq!(runs, vec![run(0, 1, 3)]);
        assert_eq!(
            backgrounds(&next).count(),
            0,
            "seçili zemin çizildi: {next:?}"
        );
        // Metin yerinde ve kendi renginde: seçim içerik silmez, rengi de
        // çevirmez (Karar 3).
        assert_eq!(
            next.iter().filter_map(|c| c.ch).collect::<String>(),
            "hello"
        );
        for cell in &next {
            assert_eq!(cell.fg, plain[&cell.col], "{cell:?}");
        }
    }

    #[test]
    fn selected_inverse_cell_is_drawn_in_its_own_foreground() {
        // Seçim ters videoyu **çözer** (Karar 3): seçili ters videolu hücre
        // kendi ön planıyla, zeminsiz çizilir — altında seçimin rengi var.
        //
        // Sütunlar: 0–1 ters video (ön plan kırmızı, arka plan yeşil), 2–3
        // aynısı artı `DIM`, 4 varsayılan renkli ters video boşluk. İmleç 5.
        // sütunda, yani `contains_cell`'in blok imleç istisnası hiçbirine
        // değmiyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033[7;31;42mab\\033[2mcd\\033[0;7m \\033[0m'; sleep 5",
            Arc::clone(&wake),
        );
        assert_eq!(wait_cells(&session, &wake, 5).len(), 5);
        let red = color::linear_rgba(THEME.default(1));
        let green = color::linear_rgba(THEME.default(2));
        let frame = |session: &Session| {
            let (cells, runs, _) = runs_if_damaged(session).expect("seçim kare istemeli");
            let colors = cells
                .iter()
                .map(|c| (c.col, (c.bg, c.fg)))
                .collect::<HashMap<_, _>>();
            (colors, runs)
        };

        // 1. ve 2. sütun seçili: biri düz, biri sönük ters video.
        session.set_selection(
            SelectKind::Simple,
            at(1, 0, CellHalf::Left),
            at(2, 0, CellHalf::Right),
        );
        let (drawn, runs) = frame(&session);
        assert_eq!(runs, vec![run(0, 1, 2)]);
        // Seçilmemiş ters video: renkler takaslı.
        assert_eq!(drawn[&0], (Some(red), green), "{drawn:?}");
        // Seçili ters video: zemin yok, metin hücrenin kendi ön planında.
        assert_eq!(drawn[&1], (None, red), "{drawn:?}");
        // `DIM` kuralı çözmeden sonra da aynı: sönüklük `cell.fg`'den doğan
        // renge gider. Seçilmemişte o renk arka plan, seçilide ön plan.
        // Elle yazılı: `0xd16d6a`'nın zemine karışmış sönüğü (bkz.
        // `dim_colors_on_the_draw_path_are_pinned`).
        let dim_red = LinearRgba::from_srgb(0x8b, 0x48, 0x46);
        assert_eq!(drawn[&2], (None, dim_red), "{drawn:?}");
        assert_eq!(drawn[&3], (Some(dim_red), green), "{drawn:?}");

        // Varsayılan renkli ters video boşluk **çizilirdir** (zemini
        // görünür), yani seçilince koşuyu kendi başına doğuruyor; zemini
        // seçimin altında kalıyor.
        assert!(drawn[&4].0.is_some(), "seçilmemiş boşluk boyalı: {drawn:?}");
        session.set_selection(
            SelectKind::Simple,
            at(4, 0, CellHalf::Left),
            at(4, 0, CellHalf::Right),
        );
        let (drawn, runs) = frame(&session);
        assert_eq!(runs, vec![run(0, 4, 4)]);
        assert!(drawn.get(&4).is_none_or(|c| c.0.is_none()), "{drawn:?}");
        assert_eq!(drawn[&1], (Some(red), green), "{drawn:?}");
    }

    #[test]
    fn selecting_empty_space_paints_nothing() {
        // **Ölçülmüş kusur** (kullanıcı, 2026-09-18): boş ekranda fareyi
        // sürüklemek koca bir vurgu bloğu doğuruyordu ve o seçim hiçbir şey
        // kopyalamıyordu — göz "seçtim" derken pano boş geliyordu. İçerik
        // tabana yaslandığı için (011) blok pencerenin ortasından başlıyor ve
        // "gözükenin altında ayrı bir alan varmış" gibi okunuyordu.
        //
        // Seçim içeriği vurgular, içerik yaratmaz: hiç hücre çizilmemeli.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf 'ab'; sleep 5", Arc::clone(&wake));
        wait_frame(&session, &wake, |cells| cells.len() == 2);

        // Aralık metni, sağındaki boş kuyruğu **ve** altındaki iki boş satırı
        // kapsıyor. Çizilen tek şey metnin kendisi olmalı.
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(10, 2, CellHalf::Right),
        );
        let (cells, runs, _) = runs_if_damaged(&session).expect("seçim kare istemeli");
        let drawn: Vec<_> = cells.iter().map(|c| (c.row, c.col)).collect();
        assert_eq!(drawn, vec![(0, 0), (0, 1)], "boş hücre boyandı: {cells:?}");
        // Koşu da metinde bitiyor: boş kuyruk ve boş satırlar koşusuz.
        assert_eq!(runs, vec![run(0, 0, 1)]);
    }

    #[test]
    fn a_run_bridges_the_gaps_between_words_but_not_the_tail() {
        // **Birim hücre değil satır** (031 Karar 4): `echo hello world`'ün
        // kelime arası boşlukları koşunun içinde — pano onları zaten
        // kopyalıyor, göz de artık görüyor. Satırın boş kuyruğu (aralık 20.
        // sütuna kadar gidiyor) dışarıda: seçim içerik yaratmaz.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf 'echo hello world'; sleep 5", Arc::clone(&wake));
        wait_frame(&session, &wake, |cells| cells.len() == 14);
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(20, 0, CellHalf::Right),
        );
        let (cells, runs, _) = runs_if_damaged(&session).expect("seçim kare istemeli");
        assert_eq!(runs, vec![run(0, 0, 15)]);
        // Köprü bir çizim kararı, hücre üretmiyor: boşluklar sink'e yine
        // uğramıyor.
        assert_eq!(cells.len(), 14, "{cells:?}");
    }

    #[test]
    fn an_empty_row_splits_the_runs() {
        // Çok satırlı seçimde boş bir ara satır koşu üretmiyor ve şekil orada
        // bölünüyor (Karar 4). Satır başına **bir** koşu: ikinci satırın
        // kelime arası boşluğu da köprülü.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf 'ab\\n\\ncd ef'; sleep 5", Arc::clone(&wake));
        wait_frame(&session, &wake, |cells| cells.len() == 6);
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(10, 2, CellHalf::Right),
        );
        let (_, runs, _) = runs_if_damaged(&session).expect("seçim kare istemeli");
        assert_eq!(runs, vec![run(0, 0, 1), run(2, 0, 4)]);
    }

    #[test]
    fn a_selection_does_not_move_the_content() {
        // Doluluk sayısı seçimden etkilenmez: seçili hücrenin zemini sınırdan
        // düşse de atlama kapısı **seçimsiz** hâle bakıyor. Bakmasaydı
        // yalnız zeminden ibaret bir satır (ters videolu boşluklar) seçilince
        // doluluktan düşer ve bütün ızgara bir satır kayardı. Satır 2 imlecin
        // üstünde: dolu olarak sayılan tek sebep o zeminler.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf 'a\\n\\n\\033[7m   \\033[0m\\n\\n'; sleep 5",
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| cells.len() == 4);
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(0, 0, CellHalf::Right),
        );
        let (_, _, before) = runs_if_damaged(&session).expect("seçim kare istemeli");
        session.set_selection(
            SelectKind::Simple,
            at(0, 2, CellHalf::Left),
            at(2, 2, CellHalf::Right),
        );
        let (cells, runs, after) = runs_if_damaged(&session).expect("seçim kare istemeli");
        assert_eq!(runs, vec![run(2, 0, 2)]);
        assert!(backgrounds(&cells).next().is_none(), "{cells:?}");
        assert_eq!(after.content_rows, before.content_rows);
    }

    #[test]
    fn the_selection_colors_come_from_the_theme() {
        // İki renk `bt-core`'dan hazır geliyor, hangisinin çizileceğini odak
        // bilen `bt-gpu` seçiyor (Karar 9). Tema takası bir sonraki karede
        // ikisini birden değiştiriyor — ayrı bir sorgu yok. Aramanın iki rolü
        // (033) aynı kopyadan ve arama **kapalıyken** de: renk koşudan
        // bağımsız, tarama değil.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf 'ab'; sleep 5", Arc::clone(&wake));
        wait_frame(&session, &wake, |cells| cells.len() == 2);
        let mut runs = SelectionRuns::default();
        let mut search = SearchRuns::default();
        let mut frame = |session: &Session| {
            session.frame(
                |_| (),
                |_| (),
                &mut Blocks::default(),
                &mut runs,
                &mut search,
                &mut Clusters::default(),
                ScrollGlide::default(),
                BUDGET,
            );
            (
                (runs.color(true), runs.color(false)),
                (search.match_color(true), search.match_color(false)),
                (search.current_color(true), search.current_color(false)),
            )
        };
        let colors = |theme: Theme| {
            (
                (theme.selection_linear(), theme.selection_unfocused_linear()),
                (
                    theme.search_match_linear(),
                    theme.search_match_unfocused_linear(),
                ),
                (
                    theme.search_current_linear(),
                    theme.search_current_unfocused_linear(),
                ),
            )
        };
        assert_eq!(frame(&session), colors(THEME));
        session.set_theme(Theme::BATERI_LIGHT);
        assert_eq!(frame(&session), colors(Theme::BATERI_LIGHT));
    }

    #[test]
    fn the_blink_setting_reaches_the_first_frame() {
        // **Ölçülmüş kusur** (`/code-review`, 014 phase-2): `Adapter::new`
        // blink'i varsayılanında kuruyordu ve `Session::spawn` ayarı hiç
        // yazmıyordu. Tek yazıcı `set_terminal_options` olduğu için özellik
        // taze pencerede sessizce ölü kalıyor, ancak kullanıcı ayar dosyasını
        // **yeniden kaydedince** hayat buluyordu — yani her açılışta bozuk,
        // alakasız bir düzenlemeden sonra "kendiliğinden düzeliyor".
        let wake = Arc::new(TestWake::default());
        let session = spawn_blinking_session("printf 'ab'; sleep 5", Arc::clone(&wake));
        let cursor = wait_cursor(&session, &wake, |cells| cells.len() == 2);
        assert!(cursor.blink, "ayar ilk kareye ulaşmadı");
    }

    #[test]
    fn a_hidden_cursor_does_not_blink() {
        // **R9'un "imleç gizlenir" durma koşulu.** Çizilmeyen bir caret sönmez:
        // açık kalsaydı `\e[?25l` gönderen bir TUI'de pencere saniyede iki kez
        // uyanıp **birebir aynı** kareyi çizerdi ve `IDLE_STOP` de hiç
        // dolmazdı, çünkü TUI'nin kendi çıktısı sayacı tazeliyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_blinking_session("printf 'ab\\033[?25l'; sleep 5", Arc::clone(&wake));
        let cursor = wait_cursor(&session, &wake, |cells| cells.len() == 2);
        assert!(!cursor.visible, "imleç gizlenmedi");
        assert!(!cursor.blink, "gizli imleç sönmeye devam ediyor");
    }

    #[test]
    fn a_wide_char_keeps_both_halves_highlighted() {
        // **Ölçülmüş kusur** (`/code-review`, 014 kapı): seçim vurgusuna
        // "çizilir mi" kapısı eklendiğinde geniş karakterin ikinci hücresi
        // (spacer) dışarıda kalıyordu — kendi mürekkebi yok ve zemini
        // varsayılan, yani seçili bir CJK karakterinin **yarısı** vurgusuz
        // kalıyordu; `selection_text()` ise onu bütün kopyalıyordu.
        //
        // Renkler **varsayılan** ve bu şart: komşu sınama (`wide_char_is_
        // selected_as_one_glyph`) zemini `\033[41m` ile boyadığı için spacer
        // oradan zaten çizilir görünüyor ve kusuru göremiyordu.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf '漢'; sleep 5", Arc::clone(&wake));
        wait_frame(&session, &wake, |cells| !cells.is_empty());

        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(1, 0, CellHalf::Right),
        );
        let (_, runs, _) = runs_if_damaged(&session).expect("seçim kare istemeli");
        assert_eq!(runs, vec![run(0, 0, 1)], "geniş karakterin yarısı vurgusuz");
    }

    #[test]
    fn wide_char_is_selected_as_one_glyph() {
        // Geniş karakter iki hücrelik **tek** glyph'tir ve yarı kuralı glyph'e
        // uygulanır, hücreye değil: baş hücre glyph'in sol yarısı, spacer sağ
        // yarısı. Hücre düzeyinde uygulansaydı bitiş ucu spacer'ın sol yarısına
        // düştüğünde aralık baş hücrede biterdi — harf kopyalanır ama yalnız
        // yarısı ters videolanırdı (`contains_cell` spacer'ı ancak kendisi
        // aralıktaysa vurgular). Başlangıç ucu da glyph'in dörtte üçüne kadar
        // harfi katardı.
        //
        // `あ` 0–1. hücreler (baş + spacer), `b` 2. hücre; üçü de kırmızı
        // bg'li. Vurgunun tanığı koşu: sütunu bir koşunun içinde mi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf '\\033[41mあb\\033[0m'; sleep 5", Arc::clone(&wake));
        let cells = wait_cells(&session, &wake, 3);
        assert!(cells.iter().any(|c| c.ch == Some('b')), "{cells:?}");
        let highlighted = |session: &Session, col: u16| {
            runs_if_damaged(session).is_some_and(|(_, runs, _)| {
                runs.iter()
                    .any(|r| r.row == 0 && (r.first..=r.last).contains(&col))
            })
        };

        // Bitiş ucu glyph'in sağ yarısında (spacer'ın sol yarısı): harf
        // içeride, **iki** hücresi de vurgulu.
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(1, 0, CellHalf::Left),
        );
        assert_eq!(session.selection_text().as_deref(), Some("あ"));
        assert!(highlighted(&session, 1), "spacer vurgulanmalı");

        // Bitiş ucu glyph'in sol yarısında (baş hücrenin sağ yarısı): harf
        // dışarıda, seçim boş.
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(0, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text(), None);

        // Başlangıç ucu glyph'in sağ yarısında: harf dışarıda, baş hücre
        // vurgusuz — metin ile vurgu aynı kararı veriyor.
        session.set_selection(
            SelectKind::Simple,
            at(1, 0, CellHalf::Left),
            at(2, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("b"));
        assert!(!highlighted(&session, 0), "baş hücre vurgulanmamalı");

        // Başlangıç ucu glyph'in sol yarısında: harf içeride.
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Right),
            at(2, 0, CellHalf::Right),
        );
        assert_eq!(session.selection_text().as_deref(), Some("あb"));
    }

    /// Kaydırma sınamalarının ortak sahnesi: 10 satırlık grid'e `seq 1 30`.
    /// Otuz satır + imlecin boş satırı = 31 satır, yani **21** satır geçmiş;
    /// dipte `22`…`30` ve imleç satırı görünür. Dönen oturumun karesi
    /// durulmuştur: sonraki `frame()` ancak kaydırmanın istediği kare olabilir.
    fn history_session(script: &str) -> (Session, Arc<TestWake>) {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(script, Arc::clone(&wake));
        wait_seq_tail(&session, &wake);
        (session, wake)
    }

    /// `seq 1 30`'un **son** satırı dipte görünene kadar bekler, sonra durulur.
    ///
    /// Ölçüt satırın kendisi (`row_text(.., 8) == "30"`): "sıfırıncı sütunda
    /// bir `3`, birinci sütunda bir `0`" diye iki bağımsız soru `3` ile `10`
    /// ekrandayken de tutar — `seq` çıktısı PTY okumasında bölünebilir ve
    /// sınama geçmişin yarısıyla başlardı. Dokuzuncu satırda imlecin boş
    /// satırı var.
    fn wait_seq_tail(session: &Session, wake: &TestWake) {
        wait_frame(session, wake, |cells| row_text(cells, 8) == "30");
        wait_settled(session);
    }

    /// Şimdiye kadarki uyandırma sayısı — beklemeden.
    fn wakes(wake: &TestWake) -> u32 {
        wake.wait_wakes(0, Duration::ZERO)
    }

    /// Karenin `row` satırındaki mürekkep — kaydırmanın **hangi** içeriği
    /// gösterdiğini okumanın yolu.
    fn row_text(cells: &[Cell], row: u16) -> String {
        cells
            .iter()
            .filter(|c| c.row == row)
            .filter_map(|c| c.ch)
            .collect()
    }

    /// Görünen pencerenin geçmişteki ofseti — `Term`'den doğrudan.
    fn display_offset(session: &Session) -> usize {
        session.term.lock().grid().display_offset()
    }

    /// Shift'siz tekerlek, işaretçi sol üstte — birincil ekranın kaydırma
    /// sınamalarının ortak çağrısı; işaretçi o dalda okunmuyor.
    fn scroll(session: &Session, lines: i32) -> Wheel {
        session.scroll_wheel(
            f64::from(lines),
            lines,
            ScrollIntent::Lines,
            at(0, 0, CellHalf::Left),
            false,
        )
    }

    /// Shift'siz sol tuş basışı — düğme sınamalarının ortak çağrısı.
    fn press(session: &Session, at: SelectionPoint) -> Click {
        session.mouse_button(MouseButton::Left, true, at, MouseModifiers::default())
    }

    /// Shift'siz sol tuş bırakması; Shift zaten okunmuyor (rota basışta
    /// kilitli).
    fn release(session: &Session, at: SelectionPoint) -> Click {
        session.mouse_button(MouseButton::Left, false, at, MouseModifiers::default())
    }

    /// Hasar sormadan bu anın imleç kaydı.
    ///
    /// [`frame_if_damaged`]'ten ayrı, çünkü sorulan şey kare isteği değil
    /// **kaydın içeriği**: doluluk sayısı kirli olmayan bir karede de doğru
    /// olmak zorunda (hareket karesi onu `link.rs`'te korunan bir değerden
    /// okuyor). Hasarsız `frame()` meşru, yalnız boşuna — doc'u öyle yazıyor.
    fn cursor_now(session: &Session) -> Cursor {
        session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        )
    }

    #[test]
    fn leaving_the_alt_screen_restores_the_cursor_baseline() {
        // **Kullanıcının bildirdiği kusurun bekçisi** (2026-09-20): `auto`
        // modunda açılışta sönen imleç, vim'den bir kez geçtikten sonra bir
        // daha hiç sönmüyordu.
        //
        // Sahne vim'i çağırmıyor, vim'in **gönderdiğini** gönderiyor ve o
        // ölçüldü: `\e[?12l` özel mod 12'nin reset'i ve terminfo'nun
        // `cnorm`'unun içinde geliyor (`xterm-256color`:
        // `cnorm = \e[?12l\e[?25h`), yani onu gönderen şey vim'in kendisi
        // değil "imleci normalleştir" komutu — less, man ve htop da aynısını
        // yapıyor. DECSCUSR sahnede **yok**, çünkü vim'in bütün oturumunda da
        // yoktu.
        //
        // Adımlar `sleep` ile ayrılıyor: aynı PTY okumasında gelselerdi
        // aradaki hâller hiç gözlenmez ve sınama kusuru göremezdi.
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(
            sh(
                "stty -echo; printf 'a\\n'; sleep 0.4; printf '\\033[?1049h'; \
                sleep 0.4; printf '\\033[?12l'; sleep 0.4; printf '\\033[?1049l'; sleep 5",
            ),
            40,
        );
        options.terminal.blink = CursorBlink::Auto;
        let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
        wait_ink(&session, &wake, "a");

        // `"auto"`nun tabanı açık (`docs/AYARLAR.md`): kapalı olsaydı `"off"`
        // ile ayırt edilemezdi.
        assert!(cursor_now(&session).blink, "auto'nun tabanı sönük başladı");

        // Alt ekranda `cnorm` blink'i kapatıyor — **bu doğru**, uygulamanın
        // dediği geçerli.
        // `cursor_now` **önce**: `alt_screen` bayrağını `frame()` diker
        // ([`Session::frame`]) ve `&&`'in kısa devresi onu hiç çağırmazsa
        // bayrak sonsuza kadar bayat kalır (ölçüldü, bu sınamayı yazarken).
        wait_until(
            "alt ekranda blink kapanmadı",
            Duration::from_secs(5),
            || {
                let blink = cursor_now(&session).blink;
                session.alt_screen() && !blink
            },
        );

        // Çıkışta taban geri geliyor. Kusurlu hâlde burası zaman aşımına
        // düşüyor (ölçüldü): bayrak düşüyor ama blink `false` kalıyordu.
        wait_until(
            "alt ekrandan çıkışta taban dönmedi",
            Duration::from_secs(5),
            || {
                let blink = cursor_now(&session).blink;
                !session.alt_screen() && blink
            },
        );
    }

    #[test]
    fn content_rows_count_the_drawn_rows_and_the_cursor_row() {
        // Tabana yapışmanın tek girdisi (`Cursor::content_rows`): `bt-gpu` onu
        // `rows - content_rows` ile ötelemeye çeviriyor. `stty -echo`: prompt
        // yok, ekranda tam olarak yazdığımız var.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("stty -echo; printf 'a\\nb\\n'; sleep 5", Arc::clone(&wake));
        wait_ink(&session, &wake, "ab");

        let cursor = cursor_now(&session);
        // Izgara aynı okumadan geliyor: çizen tarafın kendi kopyası
        // `Session::resize`'ın ret kolunda ayrışırdı (`Cursor::rows`).
        assert_eq!(cursor.rows, 10, "{cursor:?}");
        // `a` 0. satırda, `b` 1.'de, imleç 2.'de: üç satır dolu, yedi boş.
        assert_eq!(cursor.row, 2, "{cursor:?}");
        assert_eq!(cursor.content_rows, 3, "{cursor:?}");
    }

    #[test]
    fn content_rows_need_both_halves() {
        // İki kaynağın **ikisi de** gerekli ve her biri ötekinin körlüğünü
        // kapatıyor; biri unutulursa belirti sessiz bir yerleşim kusuru olur.
        let wake = Arc::new(TestWake::default());

        // (1) İmleç yarısı olmasa: mürekkebi olmayan satırlar atlama
        // kapısından geçmiyor, yani giriş satırı boşluğa düşerdi. `a` tek
        // dolu satır ama imleç üç satır aşağıda.
        let empty_tail = spawn_session(
            "stty -echo; printf 'a\\n\\n\\n'; sleep 5",
            Arc::clone(&wake),
        );
        wait_ink(&empty_tail, &wake, "a");
        let cursor = cursor_now(&empty_tail);
        assert_eq!(cursor.row, 3, "{cursor:?}");
        assert_eq!(
            cursor.content_rows, 4,
            "imleç yarısı düştü: boş satırlar sayılmadı ({cursor:?})"
        );

        // (2) Çizilen yarısı olmasa: imleci yukarı taşıyan bir ilerleme
        // çubuğu (`\e[H`) içeriği aşağı iterdi. Üç satır dolu, imleç 0.'da.
        let wake = Arc::new(TestWake::default());
        let cursor_up = spawn_session(
            "stty -echo; printf 'a\\nb\\nc\\n\\033[H'; sleep 5",
            Arc::clone(&wake),
        );
        wait_ink(&cursor_up, &wake, "abc");
        let cursor = cursor_now(&cursor_up);
        assert_eq!(cursor.row, 0, "{cursor:?}");
        assert_eq!(
            cursor.content_rows, 3,
            "çizilen yarısı düştü: imleç içeriği aşağı itti ({cursor:?})"
        );
    }

    #[test]
    fn the_alternate_screen_owns_every_row() {
        // vim ve htop ızgaranın tamamını sahipleniyor: doluluk `rows`, yani
        // öteleme sıfır. Tam ekran bir uygulamanın boş bıraktığı alt satırlar
        // yüzünden içeriğin aşağı kaymasını bu kol önlüyor.
        let (session, _wake) = dump_session(40, "printf '\\033[?1049h'", |mode| {
            mode.contains(TermMode::ALT_SCREEN)
        });
        let cursor = cursor_now(&session);
        assert_eq!(cursor.content_rows, cursor.rows, "{cursor:?}");
    }

    #[test]
    fn content_rows_come_from_the_visible_window_while_scrolled() {
        // **Geçmişte kaydırırken de içerik tabana yapışık kalır.** Doluluk
        // görünür satırlardan doğuyor, yani `display_offset > 0` iken kural
        // aynı: temizlenmiş bir pencerede tekerleğin ilk çentiği iki satırlık
        // içerik gösterir ve ikisi dipte durur.
        //
        // Bir dönem burada `offset != 0 => rows` kolu vardı (017, ölü
        // kaydırmayı çözmek için) ve **kullanıcı onu gördü**: ızgaranın boş
        // alt satırları doluluğa giriyor, öteleme kapanıyor ve bütün içerik
        // pencerenin tepesine sıçrıyordu. Ölü kaydırmanın sebebi bu kol
        // değilmiş — doldurmanın kaydırılmış pencerede de koşmasıymış, ve
        // kapısı [`Session::fill_rows`]'ta. Süreklilik de bu kolda değil
        // kaydırmada (`the_first_notch_continues_where_the_band_left_off`).
        //
        // `\e[2J\e[H` geçmişi silmiyor, yalnız görünen pencereyi: `seq`'in 30
        // satırı defterde duruyor ve kaydırılacak bir yer var.
        // İki adım **`read` ile sıralanıyor**: tek betikte arka arkaya
        // yazılsalardı ikisi aynı PTY okumasında gelebilir ve "geçmiş doldu"
        // ölçütü hiç gözlenmezdi — sınama boş bir ekranı temiz sanıp
        // kaydıracak yer bulamazdı (emsal
        // `set_terminal_options_switches_osc52_live`).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[H'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);

        session.write(b"\n");
        // Ölçüt doluluğun kendisi: tek dolu satır imlecin satırı.
        wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
            cursor_now(&session).content_rows == 1
        });
        assert_eq!(display_offset(&session), 0);

        // Bir çentik geriye: geçmişin son satırı 0. satıra, imleç 1.'ye.
        // Bant yok (pencere dock'suz), yani çentik de bir.
        assert_eq!(scroll(&session, 1), Wheel::Scrolled(1));
        let cursor = cursor_now(&session);
        assert_eq!(cursor.display_offset, 1, "{cursor:?}");
        assert_eq!(cursor.content_rows, 2, "{cursor:?}");

        // Pencere geçmişle dolunca öteleme kendiliğinden sıfıra iner: doluluk
        // `rows`'a **çıkıyor**, ama onu yazan şey görünür satırlar — bir dal
        // değil.
        assert!(matches!(scroll(&session, 20), Wheel::Scrolled(n) if n > 0));
        let cursor = cursor_now(&session);
        assert_eq!(cursor.content_rows, cursor.rows, "{cursor:?}");
    }

    /// Bu anın "ekran kasten temizlendi" bayrağı.
    ///
    /// Doğrudan atomikten: bayrağın tüketicisi [`Session::fill_rows`] ve o da
    /// `pub` değil — sınırdan geçen şey bayrak değil [`Cursor::fill`]. Yalnız
    /// sınama için `pub` bir kapı açmak sınırı sebepsiz genişletmek olurdu;
    /// `mod tests` modülün çocuğu, alanı görüyor.
    fn screen_cleared(session: &Session) -> bool {
        session.screen_cleared.load(Ordering::Relaxed)
    }

    #[test]
    fn a_deliberate_clear_sets_the_flag_until_the_screen_fills_again() {
        // Bayrağın ömrü (R1.1, R1.2): `CSI 2 J` kuruyor, ekran **doğal
        // yoldan** yeniden dolunca düşüyor. Bugün tüketicisi yok — bu sınama
        // phase-2'nin doldurma kapısını tek başına taşıyor.
        //
        // Üç adım `read` ile sıralanıyor: tek betikte arka arkaya yazılsalardı
        // ikisi aynı PTY okumasında gelir ve aradaki hâl hiç gözlenmezdi
        // (emsal `content_rows_come_from_the_visible_window_while_scrolled`).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[H'; read _; \
             seq 1 30; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);
        let cursor = cursor_now(&session);
        assert_eq!(cursor.content_rows, cursor.rows, "{cursor:?}");
        assert!(!screen_cleared(&session), "dolu ekranda bayrak kuruluydu");

        session.write(b"\n");
        wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
            cursor_now(&session).content_rows == 1
        });
        assert!(screen_cleared(&session), "`CSI 2 J` bayrağı kurmadı");

        session.write(b"\n");
        wait_until("ekran yeniden dolmadı", Duration::from_secs(5), || {
            let cursor = cursor_now(&session);
            cursor.content_rows == cursor.rows
        });
        assert!(
            !screen_cleared(&session),
            "ekran doğal yoldan doldu ama bayrak düşmedi"
        );
    }

    #[test]
    fn scrolling_into_history_never_drops_the_flag() {
        // **Ömrün üçüncü koşulu ve onsuz tek bir tekerlek jesti Ctrl-L'i geri
        // alıyordu** (`/code-review`, 017 phase-1): `content_rows` görünür
        // pencereden doğuyor, yani geçmişe kaydırılan pencere geçmiş
        // satırlarıyla dolunca `content_rows == rows`. Bayrak orada düşerse
        // kullanıcı dibe döndüğünde doldurma temizlediği ekranı geri doldurur
        // — kayıp kaydırmayla birlikte bitmiyor, **kalıcı**.
        //
        // Reçete deponun kendi tanığından alındı
        // (`content_rows_come_from_the_visible_window_while_scrolled`): orası
        // `\033[2J` sonrası 20 çentiğin `content_rows == rows` verdiğini zaten
        // sabitliyor, yani bu sınamanın öncülü ölçülmüş.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[H'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);

        session.write(b"\n");
        wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
            cursor_now(&session).content_rows == 1
        });
        assert!(screen_cleared(&session), "`CSI 2 J` bayrağı kurmadı");

        // Geçmişe kaydır: pencere dolu **görünüyor** ama ekran dolmadı.
        assert!(matches!(scroll(&session, 20), Wheel::Scrolled(n) if n > 0));
        let cursor = cursor_now(&session);
        assert!(cursor.display_offset > 0, "{cursor:?}");
        assert_eq!(cursor.content_rows, cursor.rows, "{cursor:?}");
        assert!(
            screen_cleared(&session),
            "geçmişe kaydırma bayrağı düşürdü: {cursor:?}"
        );

        // Ve dibe dönünce de duruyor: kayıp kaydırma sırasında olsaydı burada
        // görünürdü, çünkü doldurmanın kapısı (`display_offset == 0`) yeniden
        // açılıyor.
        session.term.lock().scroll_display(Scroll::Bottom);
        let cursor = cursor_now(&session);
        assert_eq!(cursor.display_offset, 0, "{cursor:?}");
        assert!(
            screen_cleared(&session),
            "dibe dönünce bayrak kayıptı: {cursor:?}"
        );
    }

    #[test]
    fn the_alternate_screen_never_drops_the_flag() {
        // **Ömrün ikinci koşulu zorunlu.** Alternatif ekranda doluluk tanım
        // gereği `rows` ([`Cursor::content_rows`]), yani `!alt_screen`
        // olmasaydı `vim`'in **her** karesi bayrağı düşürürdü — kullanıcı
        // Ctrl-L'den sonra vim'e girip çıkınca temizlediği ekran geri gelirdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; printf '\\033[2J\\033[H'; read _; printf '\\033[?1049h'; \
             sleep 5",
            Arc::clone(&wake),
        );
        // Nesil **burada** tüketiliyor: alternatif ekrana bayrağı yeni kuran
        // bir kareyle girilseydi sınama yanlış sebeple yeşil olurdu.
        wait_until("bayrak kurulmadı", Duration::from_secs(5), || {
            cursor_now(&session);
            screen_cleared(&session)
        });

        session.write(b"\n");
        wait_until(
            "alternatif ekrana geçilmedi",
            Duration::from_secs(5),
            || {
                cursor_now(&session);
                session.alt_screen()
            },
        );
        // Alternatif ekranda birkaç kare: doluluk `rows`, yani koruyan tek şey
        // ikinci koşul.
        for _ in 0..3 {
            let cursor = cursor_now(&session);
            assert_eq!(cursor.content_rows, cursor.rows, "{cursor:?}");
            assert!(
                screen_cleared(&session),
                "alternatif ekranın dolu ızgarası bayrağı düşürdü"
            );
        }
    }

    /// Alternatif ekrana girip `CSI 2 J` basıp çıkan tur; `armed` turun
    /// başındaki bayrağı seçiyor.
    ///
    /// Reçete `vim`'in ta kendisi: açılışta `?1049h` sonra `2J`, çıkışta
    /// `?1049l`. Adımlar `read` ile **ayrı ayrı** sıralanıyor — tek okumada
    /// gelselerdi mod değişimi ile temizleme aynı turda görünür ve sınama
    /// kendi ölçmek istediği kolu değil bir yarışı ölçerdi
    /// ([`Session::observe_screen_clear`] → Bilinen sınır 2).
    fn alternate_screen_round_trip(armed: bool) {
        // İki başlangıç durumu, aynı adım sayısı: bayraksız kol da bir şey
        // basıyor ki `read` adımları hizada kalsın.
        let first = if armed {
            "printf '\\033[2J\\033[H'"
        } else {
            "printf 'a\\n'"
        };
        let script = format!(
            "stty -echo; seq 1 30; read _; {first}; read _; printf '\\033[?1049h'; \
             read _; printf '\\033[2J'; read _; printf '\\033[?1049l'; sleep 5"
        );
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(&script, Arc::clone(&wake));
        wait_seq_tail(&session, &wake);

        session.write(b"\n");
        if armed {
            wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
                cursor_now(&session).content_rows == 1
            });
        } else {
            wait_ink(&session, &wake, "a");
        }
        assert_eq!(
            screen_cleared(&session),
            armed,
            "turun başlangıç durumu kurulamadı"
        );

        session.write(b"\n");
        wait_until(
            "alternatif ekrana geçilmedi",
            Duration::from_secs(5),
            || {
                cursor_now(&session);
                session.alt_screen()
            },
        );

        // Alternatif ekranın `CSI 2 J`'si: tarayıcı onu sayıyor, ömür ise
        // nesli tüketip bayrağa **dokunmuyor** (Fix A). Beklemenin ölçütü
        // sayaç, çünkü alternatif ekranda temizlemenin ızgarada görünür bir
        // izi yok — `reset_region(..)` zaten boş bir ekranı boşaltıyor.
        let before = screen_clears(&session);
        session.write(b"\n");
        wait_until(
            "alternatif ekranda `2J` sayılmadı",
            Duration::from_secs(5),
            || screen_clears(&session) > before,
        );
        for _ in 0..3 {
            cursor_now(&session);
            assert_eq!(
                screen_cleared(&session),
                armed,
                "alternatif ekranın `CSI 2 J`'si bayrağa dokundu"
            );
        }

        // Ve çıkışta da: nesil alternatif ekranda tüketildiği için birincil
        // ekrana dönüş bayrağı kurmuyor.
        session.write(b"\n");
        wait_until(
            "alternatif ekrandan çıkılmadı",
            Duration::from_secs(5),
            || {
                cursor_now(&session);
                !session.alt_screen()
            },
        );
        for _ in 0..3 {
            cursor_now(&session);
            assert_eq!(
                screen_cleared(&session),
                armed,
                "alternatif ekrandan çıkış bayrağı değiştirdi"
            );
        }
    }

    #[test]
    fn the_alternate_screen_clear_leaves_an_armed_flag_armed() {
        // **Fix A'nın birinci kolu** (phase-1b): Ctrl-L'den sonra `vim`'e
        // girip çıkmak bayrağı yeniden kurmamalı — ama düşürmemeli de.
        alternate_screen_round_trip(true);
    }

    #[test]
    fn the_alternate_screen_clear_leaves_a_clear_flag_clear() {
        // **Fix A'nın asıl kolu ve waive'lerin reddinin yarısı**: `vim`
        // açılışta `CSI 2 J` basıyor: bayrağı orada kurmak, doldurmayı ilk
        // `vim` kullanımından sonra **kalıcı olarak** kapatıyordu. Semantik
        // alacritty'den — alternatif ekranda `ClearMode::All` `reset_region`
        // çağırıyor, geçmiş büyümüyor, geri getirilmeyecek bir şey yok.
        alternate_screen_round_trip(false);
    }

    #[test]
    fn the_stamp_waits_for_the_frame_that_sees_the_clear() {
        // **Damga bir kare geç alınıyor** ve gerekçesi ölçülebilir bir kusur:
        // okuyucu thread nesli `advance`'ten **önce** artırıyor, yani bayrağı
        // kuran kare ızgarayı temizlenmeden önce görebiliyor. `CSI 2 J`
        // birincil ekranda görünen satırları geçmişe **itiyor** (alacritty
        // `clear_viewport`), yani o karenin defter boyu bir sonraki karede
        // anında aşılır ve bayrak hemen düşerdi.
        //
        // Yarışın penceresi zamanlamaya bağlı, o yüzden hermetik tanık ömrü
        // **doğrudan** çağırıyor; sayılar deponun kendi sahnesinden
        // (`seq 1 30`: defter 21, temizleme dokuz satır itiyor).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));

        session.screen_clears.fetch_add(1, Ordering::Relaxed);
        // Baytlar henüz uygulanmadı: ızgara dolu, defter temizleme öncesi boyda.
        session.observe_screen_clear(21, false, false);
        assert!(screen_cleared(&session), "taze nesil bayrağı kurmadı");

        // Temizleme uygulandı, defter dokuz satır büyüdü — ve damga **burada**
        // alınıyor. Bayat damgayla bu kare bayrağı düşürürdü.
        session.observe_screen_clear(30, false, false);
        assert!(screen_cleared(&session), "bayat damga bayrağı düşürdü");

        // Aynı boy: geçmişin en yenileri hâlâ temizleme öncesine ait.
        session.observe_screen_clear(30, false, false);
        assert!(screen_cleared(&session), "defter büyümeden bayrak düştü");

        // Tek satır yetiyor: ölçüt bir eşik değil, işaret.
        session.observe_screen_clear(31, false, false);
        assert!(
            !screen_cleared(&session),
            "defter büyüdü ama bayrak düşmedi"
        );
    }

    #[test]
    fn the_alternate_screen_neither_arms_nor_stamps_the_flag() {
        // İki kol tek sahnede, çünkü ikisi de `alt_screen`'in aynı okumasına
        // bağlı:
        //
        // 1. **Fix A** — alternatif ekranın `CSI 2 J`'si nesli tüketir,
        //    bayrağı kurmaz. Nesil tüketilmeseydi birincil ekrana dönüş onu
        //    kurardı ve `vim` kullanan her oturumda doldurma kapanırdı.
        // 2. **Damga alternatif ekranda alınmaz** — `history_size()` etkin
        //    ızgaradan geliyor ve alternatif ekranda sıfır. Ctrl-L'den hemen
        //    sonra `vim` açılsaydı damga sıfır olur, çıkışta birincil ekranın
        //    defteri onu anında aşar ve bayrak yanlışlıkla düşerdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));

        // (1) Alternatif ekranda gelen temizleme.
        session.screen_clears.fetch_add(1, Ordering::Relaxed);
        session.observe_screen_clear(0, true, false);
        assert!(
            !screen_cleared(&session),
            "alternatif ekranın `CSI 2 J`'si bayrağı kurdu"
        );
        session.observe_screen_clear(30, false, false);
        assert!(!screen_cleared(&session), "nesil tüketilmedi");

        // (2) Birincil ekranda kurulan bayrak, damgalanmadan alternatif ekrana
        // giriyor.
        session.screen_clears.fetch_add(1, Ordering::Relaxed);
        session.observe_screen_clear(21, false, false);
        assert!(screen_cleared(&session), "taze nesil bayrağı kurmadı");
        for _ in 0..3 {
            session.observe_screen_clear(0, true, false);
        }
        assert!(screen_cleared(&session), "alternatif ekran bayrağı düşürdü");

        // Birincil ekrana dönüş: damga sıfırdan değil **defterin kendi
        // boyundan** alınıyor, yani aynı boy bayrağı düşürmüyor.
        session.observe_screen_clear(30, false, false);
        session.observe_screen_clear(30, false, false);
        assert!(
            screen_cleared(&session),
            "alternatif ekranın sıfır defteri damgalandı"
        );
        session.observe_screen_clear(31, false, false);
        assert!(
            !screen_cleared(&session),
            "defter büyüdü ama bayrak düşmedi"
        );
    }

    #[test]
    fn a_saturated_history_never_drops_the_flag() {
        // **Bilinen sınır, gözlemle sabitlenmiş.** `history_size()`
        // `scrollback`'te doyuyor (alacritty `increase_scroll_limit`), yani on
        // bin satırlık bir oturumda defter büyümeyi bırakıyor ve damganın
        // üstüne çıkacak bir sayı kalmıyor: o oturumda bir Ctrl-L'den sonra
        // doldurma bir daha koşmuyor. Tek damgayla kapatılamıyor — gereken şey
        // doymuş defterde de artan bir "geçmişe itilen satır" sayacı.
        //
        // Sınama bunu **iddia etmiyor, gözlüyor**: yönü güvenli (doldurma
        // yapmamak phase-1'in davranışı) ve düzelme geldiğinde burası
        // kırmızıya düşüp kendini hatırlatıyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));

        session.screen_clears.fetch_add(1, Ordering::Relaxed);
        session.observe_screen_clear(100, false, false);
        assert!(screen_cleared(&session), "taze nesil bayrağı kurmadı");
        // Damga da tavanda: defter bir daha büyümüyor.
        for _ in 0..5 {
            session.observe_screen_clear(100, false, false);
        }
        assert!(
            screen_cleared(&session),
            "doymuş defterde bayrak düştü: bilinen sınır kapanmış olabilir"
        );
    }

    /// Bu anın nesil sayacı — tarayıcının saydığı `CSI 2 J`.
    ///
    /// [`screen_cleared`] emsali: sayaç `pub` değil, sınırdan geçen şey
    /// bayrak bile değil. Alternatif ekranda temizlemenin ızgarada görünür bir
    /// izi olmadığı için bekleyecek başka bir ölçüt yok.
    fn screen_clears(session: &Session) -> u32 {
        session.screen_clears.load(Ordering::Relaxed)
    }

    /// Bu anın imleç kaydı **ve** doldurma hücreleri.
    ///
    /// [`cursor_now`]'un doldurma soran kardeşi: doldurma ikinci bir sink'ten
    /// geçiyor ([`Session::frame`]) ve [`Session::fill_rows`] sıfır dediğinde o
    /// sink **hiç** çağrılmamalı. Geri alma şeridinin (R2.4) tanığı tam olarak
    /// o boş liste: sıfır dönen bir karede sınırdan bugünküyle bit bit aynı şey
    /// geçiyor.
    fn fill_now(session: &Session) -> (Cursor, Vec<Cell>) {
        let mut cells = Vec::new();
        let cursor = session.frame(
            |_| (),
            |cell| cells.push(cell),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        (cursor, cells)
    }

    /// Karenin **ekran** satırları, yukarıdan aşağı — bandı ve ızgarayı
    /// çizen tarafın (`bt-gpu`) birleştirdiği yerlere koyar.
    ///
    /// [`fill_now`]'un iki sink'i ayrı veriyor ve satır numaraları da ayrı
    /// uzaylarda (bant fill-yerel `0..fill`, ızgara `0..content_rows`); bu
    /// yardımcı ikisini tek bir `0..rows` dizisine indiriyor. Kaydırmanın
    /// bekçileri **tam olarak bunu** sormak zorunda: "ekran bir satır kaydı
    /// mı" sorusunun cevabı iki listenin hiçbirinde tek başına yok, çünkü
    /// bandın kısalması ile ızgaranın büyümesi birbirini gizleyebiliyor —
    /// kullanıcının gördüğü ölü kaydırma tam olarak böyle doğmuştu.
    ///
    /// Öteleme aritmetiği `bt-gpu`'nunkinin kopyası (`origin = rows -
    /// content_rows`, bandın tepesi `origin - fill`) ve ikinci bir kaynak
    /// olmasının bedeli kabul: sınırdan geçen şey satır numarası, ekran
    /// konumu değil.
    fn screen_now(session: &Session) -> (Cursor, Vec<String>) {
        let mut grid = Vec::new();
        let mut band = Vec::new();
        let cursor = session.frame(
            |cell| grid.push(cell),
            |cell| band.push(cell),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        let origin = cursor.rows - cursor.content_rows;
        let band_top = origin - cursor.fill;
        let rows = (0..cursor.rows)
            .map(|screen| {
                if screen >= origin {
                    row_text(&grid, screen - origin)
                } else if screen >= band_top {
                    // Kesrin tepe satırı kanalın en üstünde ve bu listenin
                    // dışında: yardımcı tam satırları soruyor, yarım satırı
                    // değil.
                    row_text(&band, screen - band_top + cursor.top_row)
                } else {
                    // Bandın da ızgaranın da dokunmadığı satır: gerçekten boş.
                    String::new()
                }
            })
            .collect();
        (cursor, rows)
    }

    /// Ekranı `seq 1 30` ile doldurup üstten **beş satırlık bir delik** açan
    /// dock'lu oturum; doldurmanın bütün sınamalarının ortak sahnesi.
    ///
    /// Tab→Ctrl-C reçetesinin hermetik eşdeğeri: `\e[4A\e[J` içeriği yukarıdan
    /// kısaltıyor, tıpkı tamamlama listesi kapanınca olduğu gibi. Kasten
    /// temizleme **değil** — tarayıcı yalnız `CSI 2 J` sayıyor (R1.1), ED 0
    /// bayrağı kurmuyor. İki adım `read` ile sıralanıyor (emsal
    /// `content_rows_come_from_the_visible_window_while_scrolled`).
    ///
    /// Sonuç: ekranda `22`…`26`, defterde 21 satır (`1`…`21`), `gap == 5`.
    fn gapped_session(dock: bool) -> (Session, Arc<TestWake>) {
        let script = "stty -echo; seq 1 30; read _; printf '\\033[4A\\033[J'; sleep 5";
        let wake = Arc::new(TestWake::default());
        let session = if dock {
            spawn_docked_session(script, Arc::clone(&wake))
        } else {
            spawn_session(script, Arc::clone(&wake))
        };
        wait_seq_tail(&session, &wake);
        session.write(b"\n");
        // Ölçüt "doluluk kısaldı", kesin sayı değil: dock'lu pencerede beş
        // (`caret_in_dock` imleci saymıyor), dock'suzda altı (imlecin satırı
        // da sayılıyor) — ikisinin de ortak yanı dokuzdan inmiş olması ve
        // `\e[4A`'nın tek başına hiçbir şeyi kısaltmaması.
        wait_until(
            "içerik yukarıdan kısalmadı",
            Duration::from_secs(5),
            || cursor_now(&session).content_rows <= 6,
        );
        (session, wake)
    }

    #[test]
    fn the_gap_above_fills_with_the_newest_history_rows() {
        // **R2.1 ve R2.3 birlikte**: boşluk defterin en yeni satırlarıyla
        // doluyor ve `content_rows` bundan **etkilenmiyor**. İkincisi bu
        // phase'in en somut kazancı — doldurulan satırlar doluluğa girseydi
        // öteleme kapanır, içerik tabandan kopardı (`27a0b98`'in maliyeti).
        let (session, _wake) = gapped_session(true);

        let (cursor, cells) = fill_now(&session);
        // Doluluk `\e[4A\e[J` öncesiyle aynı aritmetikten doğuyor: beş dolu
        // satır, beş satırlık delik.
        assert_eq!(cursor.content_rows, 5, "{cursor:?}");
        assert_eq!(
            cursor.fill,
            cursor.rows - cursor.content_rows,
            "boşluk kadar doldurulmadı: {cursor:?}"
        );

        // Satırlar **fill-yerel** (`0..fill`) ve sıraları defterin sırası: `0`
        // en eski, `fill - 1` içeriğin hemen üstü. Ekranda `22`…`26` durduğuna
        // göre doldurma `17`…`21` olmak zorunda — "en yeniler" iddiasını
        // taşıyan tek şey bu liste.
        let text: Vec<String> = (0..cursor.fill).map(|r| row_text(&cells, r)).collect();
        assert_eq!(text, ["17", "18", "19", "20", "21"], "{cells:?}");
    }

    #[test]
    fn a_deliberate_clear_keeps_the_gap_empty() {
        // **Setin varlık sebebinin öteki yarısı** (R2.2): Ctrl-L'den sonra
        // üstte kocaman bir boşluk var ve doldurma koşmuyor — kullanıcı ekranı
        // kasten temizlediyse geri gelmemeli.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[H'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);

        session.write(b"\n");
        wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
            cursor_now(&session).content_rows == 1
        });
        assert!(screen_cleared(&session), "`CSI 2 J` bayrağı kurmadı");

        let (cursor, cells) = fill_now(&session);
        // Boşluk gerçekten var: sınama boş bir `gap`le yanlış sebeple yeşil
        // olmasın.
        assert!(cursor.rows - cursor.content_rows > 0, "{cursor:?}");
        assert_eq!(cursor.fill, 0, "temizlenen ekran geri doldu: {cursor:?}");
        assert!(cells.is_empty(), "ikinci sink boşuna çağrıldı: {cells:?}");
    }

    #[test]
    fn a_clear_without_new_history_keeps_the_gap_empty() {
        // **Damga ölçütünün öteki yarısı** (phase-1b): ekran kaymadıysa defter
        // büyümez ve bayrak durur. Doğru cevap bu — geçmişin en yenileri hâlâ
        // temizleme öncesine ait, yani doldurma Ctrl-L'i geri alırdı.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[H'; read _; \
             printf 'hi\\n'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);

        session.write(b"\n");
        wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
            cursor_now(&session).content_rows == 1
        });
        assert!(screen_cleared(&session), "`CSI 2 J` bayrağı kurmadı");

        // Tek satırlık çıktı ekranı kaydırmıyor: defter olduğu yerde kalıyor.
        session.write(b"\n");
        wait_ink(&session, &wake, "hi");

        let (cursor, cells) = fill_now(&session);
        assert!(cursor.rows - cursor.content_rows > 0, "{cursor:?}");
        assert!(
            screen_cleared(&session),
            "defter büyümeden bayrak düştü: {cursor:?}"
        );
        assert_eq!(cursor.fill, 0, "temizlenen ekran geri doldu: {cursor:?}");
        assert!(cells.is_empty(), "ikinci sink boşuna çağrıldı: {cells:?}");
    }

    #[test]
    fn the_fill_stops_at_the_rows_that_arrived_after_the_clear() {
        // **`/code-review`'un ölçülmüş bulgusu** (017 phase-1b): bayrağın
        // düşmesi "boşluğun tamamı geri verilebilir" demiyor. Defter tek satır
        // büyüyünce bayrak düşüyor, ama doldurma `gap` satır çekiyor — aradaki
        // fark doğrudan kullanıcının sildiği ekran.
        //
        // Sahne ölçüldü: Ctrl-L (defter 30) → `seq 1 12` üç satır kaydırıyor
        // (defter 33) → yedi satırlık delik. Kırpmasız hâlde doldurulan yedi
        // satır `["27","28","29","30","1","2","3"]`, yani **dördü** temizleme
        // öncesine aitti. Kırpmayla üçü kalıyor ve üçü de temizlemeden sonra
        // gelmiş.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[H'; read _; \
             seq 1 12; read _; printf '\\033[6A\\033[J'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);

        session.write(b"\n");
        wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
            cursor_now(&session).content_rows == 1
        });
        assert!(screen_cleared(&session), "`CSI 2 J` bayrağı kurmadı");

        session.write(b"\n");
        wait_frame(&session, &wake, |cells| row_text(cells, 8) == "12");
        wait_settled(&session);
        assert!(
            !screen_cleared(&session),
            "defter büyüdü ama bayrak düşmedi"
        );

        session.write(b"\n");
        wait_until(
            "içerik yukarıdan kısalmadı",
            Duration::from_secs(5),
            || cursor_now(&session).content_rows <= 3,
        );

        let (cursor, cells) = fill_now(&session);
        // Delik doldurmadan **büyük**: kırpma gerçekten kırpıyor.
        assert!(
            cursor.rows - cursor.content_rows > cursor.fill,
            "sahne kırpmasız kuruldu: {cursor:?}"
        );
        let text: Vec<String> = (0..cursor.fill).map(|r| row_text(&cells, r)).collect();
        assert_eq!(text, ["1", "2", "3"], "{cells:?}");
    }

    #[test]
    fn a_grown_history_lets_the_gap_fill_again() {
        // **phase-1b'nin varlık sebebi ve waive'lerin reddinin ölçüldüğü yer.**
        // Reçete kullanıcının gördüğü hâliyle: Ctrl-L → yirmi satırlık çıktı →
        // Tab → Ctrl-C. Düşme ölçütü `content_rows == rows` iken burada
        // `fill == 0` çıkıyordu — dock'lu pencerede doluluk giriş satırını
        // saymadığı için tavan `rows - 1` ve yüklem erişilemez
        // (`phase-2.md` → Uygulama Notları §7b).
        //
        // Dört adım `read` ile sıralanıyor: aynı PTY okumasında gelselerdi
        // aradaki hâller hiç gözlenmezdi (emsal
        // `content_rows_come_from_the_visible_window_while_scrolled`).
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[H'; read _; \
             seq 1 20; read _; printf '\\033[4A\\033[J'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);

        // (1) Ctrl-L: bayrak kuruluyor.
        session.write(b"\n");
        wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
            cursor_now(&session).content_rows == 1
        });
        assert!(screen_cleared(&session), "`CSI 2 J` bayrağı kurmadı");

        // (2) Yirmi satır çıktı: defter büyüyor, yani geçmişin en yenileri
        // artık temizleme öncesine ait değil. **Ölçüt ekranın dolması değil
        // son satırın mürekkebi** ve fark bu phase'in kendisi: dock'lu
        // pencerede imleç boş bir alt satırda duruyor, yani `content_rows`
        // dokuzda kalıyor ve `rows`'a hiç çıkmıyor (ölçüldü — bu bekleme
        // `content_rows == rows` yazılınca zaman aşımına düşüyor).
        session.write(b"\n");
        wait_frame(&session, &wake, |cells| row_text(cells, 8) == "20");
        wait_settled(&session);
        assert!(
            !screen_cleared(&session),
            "defter temizlemeden sonra büyüdü ama bayrak düşmedi"
        );

        // (3) Tab listesinin kapanışı: üstten dört satırlık delik.
        session.write(b"\n");
        wait_until(
            "içerik yukarıdan kısalmadı",
            Duration::from_secs(5),
            || cursor_now(&session).content_rows <= 6,
        );

        let (cursor, cells) = fill_now(&session);
        assert!(cursor.rows - cursor.content_rows > 0, "{cursor:?}");
        assert!(
            cursor.fill > 0,
            "Ctrl-L'den sonra doldurma bir daha hiç koşmadı: {cursor:?}"
        );
        // Ölçüldü (2026-09-20): düzeltmeden önce `gap == 5` ve `fill == 0`;
        // sonra boşluğun tamamı doluyor.
        assert_eq!(
            cursor.fill,
            cursor.rows - cursor.content_rows,
            "boşluk kadar doldurulmadı: {cursor:?}"
        );
        assert!(!cells.is_empty(), "ikinci sink çağrılmadı: {cursor:?}");
    }

    #[test]
    fn the_fill_band_carries_its_own_block_marks() {
        // **Kullanıcının bildirdiği kusur** (2026-09-20): tamamlama listesi
        // komut satırını geçmişe itiyor, liste kalkınca bant o satırı geri
        // getiriyor ama **blok işareti olmadan**; kaydırınca aynı satır
        // ızgaradan geçtiği için işaret geri geliyordu. Ekranın Tab öncesine
        // dönmesi 017'nin sözü ve işaretsiz dönen satır onu tutmuyor.
        //
        // Sahne zsh istemiyor: çıpa hücrenin kendi OSC 8 bağlantısı
        // (`anchored_prompt`) ve geçmişe inen satır onu yanında götürüyor.
        // Eksik olan yalnız **okuyan** döngüydü.
        //
        // Akış: çıpalı komut satırı → on iki satır çıktı (çıpalı satır
        // geçmişe düşer) → komut biter (şerit çözülebilir olur) → `\e[4A\e[J`
        // boşluk açar → bant geçmişin en yenilerini geri getirir.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "stty -echo; printf '{}komut\\r\\n\\033]133;C\\007'; seq 1 12; \
                 printf '\\033]133;D;0;bt_block=1\\007'; read _; \
                 printf '\\033[4A\\033[J'; sleep 5",
                anchored_prompt(1),
            ),
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| row_text(cells, 8) == "12");
        wait_settled(&session);
        session.write(b"\n");
        wait_until(
            "içerik yukarıdan kısalmadı",
            Duration::from_secs(5),
            || cursor_now(&session).content_rows <= 6,
        );

        let mut blocks = Blocks::default();
        let mut band = Vec::new();
        let cursor = session.frame(
            |_| (),
            |cell| band.push(cell),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        assert!(cursor.fill > 0, "sahne bantsız kuruldu: {cursor:?}");

        // Çıpalı satır bandın **içinde** ve metni tanınıyor: `$ ` prompt'u
        // artı komut. Bu olmadan aşağıdaki iddia boşa düşerdi.
        let band_text: Vec<String> = (0..cursor.fill).map(|r| row_text(&band, r)).collect();
        let anchored = band_text
            .iter()
            .position(|row| row.contains("komut"))
            .unwrap_or_else(|| panic!("çıpalı satır bantta değil: {band_text:?}"));

        // **Asıl iddia**: bandın kendi blok listesi o satırı taşıyor, ve
        // satır numarası **fill-yerel** — çizen taraf onu bandın kendi
        // `setViewport`'unda kullanıyor.
        let marks = blocks.fill_slice();
        assert_eq!(
            marks.len(),
            1,
            "bandın blok listesi beklenen tek işareti vermedi: {marks:?} / {band_text:?}"
        );
        assert_eq!(
            usize::from(marks[0].row),
            anchored,
            "işaret çıpalı satırın hizasında değil: {marks:?} / {band_text:?}"
        );
        // Izgaranın listesi **karışmıyor**: satır geçmişte, yani ızgarada
        // gösterilecek bir bloğu yok.
        assert_eq!(blocks.as_slice(), [], "işaret ızgaranın listesine sızdı");
        session.shutdown();
    }

    #[test]
    fn a_prompt_redrawn_after_a_clear_keeps_its_block_mark() {
        // `/code-review` (032 kapı): Ctrl-L aynı prompt'u aynı kimlikle
        // yeniden basıyor ve `CSI 2 J` eski prompt satırını geçmişe itiyor.
        // Yeni prompt'un üstündeki satır aynı kimliği taşıyor ama komutun
        // başı değil — işaret yeni prompt'ta kalmalı.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf '\\033]133;A;bt_block=1\\007\\033]8;;bateri://block/1\\007$ ls\
             \\033[H\\033[2J\\033]8;;bateri://block/1\\007$ ls\\033]8;;\\007\
             \\033]133;B\\007\\033]133;C\\007'; sleep 5",
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| {
            row_text(cells, 0).starts_with("$ls")
        });
        wait_until("komut koşmadı", Duration::from_secs(5), || {
            session.shell_state().map(|s| s.phase) == Some(ShellPhase::Running)
        });
        wait_settled(&session);
        assert!(
            session.term.lock().history_size() > 0,
            "sahne: temizleme eski prompt'u geçmişe itmedi"
        );
        let mut blocks = Blocks::default();
        session.frame(
            |_| (),
            |_| (),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        let rows: Vec<u16> = blocks.as_slice().iter().map(|block| block.row).collect();
        assert_eq!(rows, [0], "temizlemeden sonraki prompt işaretini kaybetti");
        session.shutdown();
    }

    #[test]
    fn a_continuation_row_at_the_top_of_the_fill_band_takes_no_mark() {
        // Izgaranın devam satırı kuralının bant ikizi: doldurma bandının
        // tepesine çok satırlı bir komutun ortası düşerse işaret orada değil.
        // Sahne `the_fill_band_carries_its_own_block_marks`'ınki, komut beş
        // satır (`k1`..`k5`) ve boşluk bandın tepesine `k1`'i sığdırmıyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; printf '\\033]133;A;bt_block=1\\007\\033]8;;bateri://block/1\\007\
             $ k1\\r\\nk2\\r\\nk3\\r\\nk4\\r\\nk5\\033]8;;\\007\\033]133;B\\007\
             \\r\\n\\033]133;C\\007'; seq 1 12; \
             printf '\\033]133;D;0;bt_block=1\\007'; read _; \
             printf '\\033[4A\\033[J'; sleep 5",
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| {
            (0..10).any(|row| row_text(cells, row) == "12")
        });
        wait_settled(&session);
        session.write(b"\n");
        wait_until(
            "içerik yukarıdan kısalmadı",
            Duration::from_secs(5),
            || cursor_now(&session).content_rows <= 6,
        );
        let mut blocks = Blocks::default();
        let mut band = Vec::new();
        let cursor = session.frame(
            |_| (),
            |cell| band.push(cell),
            &mut blocks,
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        let band_text: Vec<String> = (0..cursor.fill).map(|r| row_text(&band, r)).collect();
        // Sahnenin kendisi: bandın tepesi komutun bir devam satırı ve
        // komutun başı bantta değil — yoksa iddia boşa düşerdi.
        assert!(
            band_text.first().is_some_and(|row| row.starts_with('k'))
                && !band_text.iter().any(|row| row.contains("k1")),
            "sahne kurulamadı: {band_text:?}"
        );
        assert_eq!(
            blocks.fill_slice(),
            [],
            "işaret bandın tepesindeki devam satırına oturdu: {band_text:?}"
        );
        session.shutdown();
    }

    #[test]
    fn every_notch_moves_the_screen_by_one_row_at_most() {
        // **Seri kaydırmanın bekçisi** (2026-09-20, kullanıcı: "yukarı aşağı
        // seri scroll ettiğimde varolan sonuçları bi dalgalandırıyor").
        //
        // Ölçüt tek ve bütünsel: **her çentikte ekran ya aynı kalır ya tam
        // bir satır kayar.** Ara bir hâl yok — bant ile ızgaranın rol
        // değiştirdiği anda ekranın bandın boyu kadar zıplaması tam olarak
        // bu ölçütü deliyordu.
        //
        // Sahne kullanıcının durumunu taklit ediyor: **defter bandın boyu
        // kadar**, yani yukarı çıkan pencere `offset == fill`'de duruyor
        // (`clamp`). Eski kural o ofseti muafiyetin içine alıyor, aşağı inen
        // çentik dibe snap'lemek yerine tek tek iniyor ve son adımda bant
        // birden geri gelip ekranı zıplatıyordu.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; seq 1 12; read _; printf '\\033[4A\\033[J'; sleep 5",
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| row_text(cells, 8) == "12");
        wait_settled(&session);
        session.write(b"\n");
        wait_until(
            "içerik yukarıdan kısalmadı",
            Duration::from_secs(5),
            || cursor_now(&session).content_rows <= 6,
        );

        let (start, mut prev) = screen_now(&session);
        assert!(start.fill > 0, "sahne bantsız kuruldu: {start:?}");

        // Üç yukarı, üç aşağı: uçlarda doyma meşru, ara adımlarda değil.
        for (step, notch) in [1, 1, 1, -1, -1, -1].into_iter().zip(1..) {
            scroll(&session, step);
            let (cursor, now) = screen_now(&session);
            let up = now[1..] == prev[..prev.len() - 1];
            let down = now[..now.len() - 1] == prev[1..];
            assert!(
                now == prev || up || down,
                "çentik {notch} ekranı bir satırdan fazla oynattı: \
                 {cursor:?}\n  önce: {prev:?}\n  sonra: {now:?}"
            );
            prev = now;
        }

        // Dönüşün sonu başlangıcın ta kendisi: bant geri gelmiş ve ekran
        // kımıldamamış olmalı.
        let (end, back) = screen_now(&session);
        assert_eq!(end.fill, start.fill, "bant geri gelmedi: {end:?}");
        assert_eq!(back, prev, "son kare kendiyle tutarsız");
    }

    #[test]
    fn the_first_notch_continues_where_the_band_left_off() {
        // **Bandın en pahalı bekçisi ve kullanıcının iki kez bildirdiği
        // kusurun tam ölçütü**: tekerleğin ilk çentiği ekranı **bir satır**
        // yukarı taşımalı.
        //
        // İki yanlış cevabı birden reddediyor ve ikisi de denendi
        // (2026-09-20, gözle kontrol; kullanıcı):
        //
        // 1. `display_offset == 0` kapısı + yaslamanın kaydırmada da koşması:
        //    ilk çentik bandı **düşürüyor**, ekranın üstü kapkara kalıyordu.
        // 2. Bandın viewport'la birlikte kayması: `fill = rows -
        //    content_rows` ve `content_rows` her çentikte bir büyüdüğü için
        //    `fill + offset` sabit kalıyor, yani ekranın tepesi boşluk kadar
        //    çentik boyunca **hiç kıpırdamıyordu** ("sanki bi scroll sayıyor").
        //
        // Ölçüt ekranın kendisi ([`screen_now`]), bandın ya da ızgaranın
        // listesi değil: iki yanlış cevabın ikincisinde her iki liste de
        // "doğru" görünüyordu, ayrıştıran şey birleşimleri.
        let (session, _wake) = gapped_session(true);
        let (before, top) = screen_now(&session);
        assert!(before.fill >= 2, "sahne doldurmasız kuruldu: {before:?}");
        assert_eq!(before.display_offset, 0, "{before:?}");
        assert!(
            top.iter().all(|row| !row.is_empty()),
            "sahne dolu bir ekranla kurulmadı: {top:?}"
        );

        // Ofset **bandın üstüne atlıyor** (`band + 1`), ekran bir satır
        // kayıyor: dönen sayı görsel hareket değil ofset farkı
        // ([`scroll_locked`]).
        let notch = scroll(&session, 1);
        assert_eq!(
            notch,
            Wheel::Scrolled(i32::from(before.fill) + 1),
            "ilk çentik bandın üstünden devam etmedi"
        );

        let (after, rows) = screen_now(&session);
        assert_eq!(
            after.display_offset,
            i32::from(before.fill) + 1,
            "{after:?}"
        );
        // Bant kalktı. Doluluk bu sahnede `rows`'a **çıkıyor** ama bunu
        // yazan şey bir dal değil görünür satırlar: viewport'un on satırı da
        // mürekkepli (yaslama yerinde duruyor, `content_rows_come_from_the_\
        // visible_window_while_scrolled`).
        assert_eq!(after.fill, 0, "kaydırılmış pencerede bant kaldı: {after:?}");
        assert_eq!(after.content_rows, after.rows, "{after:?}");

        // **Ekranın tamamı tam bir satır aşağı kaydı.** Tek bir satırı
        // karşılaştırmak yetmez: ölü kaydırmada da bandın tepesi aynı
        // kalıyor ama ızgara büyüyordu.
        assert_eq!(rows[1..], top[..top.len() - 1], "ekran bir satır kaymadı");
        assert_ne!(rows[0], top[0], "ekranın tepesi kıpırdamadı");
    }

    #[test]
    fn scrolling_out_of_the_bands_interval_is_plain() {
        // **Bandın aralığına kaydırmadan da girilebiliyor** ve orada kural
        // kendi amacının tersine çalışırdı. Yol resize: pencereyi büyütmek
        // geçmişten satır çekiyor ve alacritty ofseti o kadar düşürüyor
        // (`grid/resize.rs` → `grow_lines`), [`Session::resize`] de dibe
        // snap'lemiyor. `1..=fill` aralığında yukarı bir çentik `target`'ı
        // hâlâ `fill`'in altında tutar, yani muafiyet olmasaydı yukarı çıkmak
        // isteyen kullanıcı **dibe** inerdi.
        //
        // Sahne resize'ın kendisini kurmuyor, onun **bıraktığı hâli** kuruyor:
        // ofseti `scroll_display` ile doğrudan aralığa koyuyor, çünkü sınanan
        // şey resize değil o hâlden çıkış.
        let (session, _wake) = gapped_session(true);
        let before = cursor_now(&session);
        assert!(before.fill >= 3, "sahne dar bir bantla kuruldu: {before:?}");

        assert!(matches!(scroll(&session, 1), Wheel::Scrolled(n) if n > 0));
        // Aralığın **içine**: bandın boyundan bir eksik.
        let inside = i32::from(before.fill) - 1;
        session
            .term
            .lock()
            .scroll_display(Scroll::Delta(inside - (i32::from(before.fill) + 1)));
        assert_eq!(display_offset(&session), usize::try_from(inside).unwrap());

        // Yukarı bir çentik: bir satır yukarı, dibe değil.
        assert_eq!(scroll(&session, 1), Wheel::Scrolled(1));
        assert_eq!(
            display_offset(&session),
            usize::try_from(inside + 1).unwrap(),
            "aralığın içinden yukarı çıkmak dibe indirdi"
        );
    }

    #[test]
    fn scrolling_back_onto_the_band_lands_at_the_bottom() {
        // Bandın **alt** kenarı da bir uçurum değil: geri inen pencere
        // `1..=fill` ofsetlerinde duramaz (orada bant yok ama gösterilen
        // satırlar bandınkilerle aynı), o yüzden `fill`'e değen hedef doğrudan
        // dibe düşüyor. Durabilseydi dönüş yolu da aynı sayıda ölü çentik
        // yerdi — simetrik kusur.
        let (session, _wake) = gapped_session(true);
        let (before, top) = screen_now(&session);
        assert!(before.fill >= 2, "sahne doldurmasız kuruldu: {before:?}");

        // İki çentik yukarı, sonra iki çentik geri.
        assert!(matches!(scroll(&session, 2), Wheel::Scrolled(n) if n > 0));
        let up = cursor_now(&session);
        assert_eq!(up.display_offset, i32::from(before.fill) + 2, "{up:?}");

        assert!(matches!(scroll(&session, -1), Wheel::Scrolled(-1)));
        assert!(matches!(scroll(&session, -1), Wheel::Scrolled(n) if n < 0));

        let (back, rows) = screen_now(&session);
        assert_eq!(back.display_offset, 0, "dibe dönülmedi: {back:?}");
        assert_eq!(back.fill, before.fill, "bant geri gelmedi: {back:?}");
        assert_eq!(rows, top, "ekran başladığı yere dönmedi");
    }

    #[test]
    fn scrolling_into_history_keeps_the_gap_empty() {
        // R2.2'nin kapısı: kaydırılmış pencerede doldurma **koşmuyor** ve
        // ikinci sink hiç çağrılmıyor. Gerekçe artık "boşluk zaten dolu"
        // değil — boşluk **yok**: yaslama da kaydırmada kalkıyor
        // ([`Session::frame`]), yani viewport ekranın tamamını dolduruyor ve
        // örtülecek bir şey kalmıyor.
        let (session, _wake) = gapped_session(true);
        assert!(fill_now(&session).0.fill > 0, "sahne doldurmasız kuruldu");

        assert!(matches!(scroll(&session, 1), Wheel::Scrolled(n) if n > 0));
        let (cursor, cells) = fill_now(&session);
        assert_ne!(cursor.display_offset, 0, "{cursor:?}");
        assert_eq!(cursor.fill, 0, "{cursor:?}");
        assert!(cells.is_empty(), "ikinci sink çağrıldı: {cells:?}");
    }

    #[test]
    fn a_window_without_a_dock_never_fills_the_gap() {
        // İlk kapı (`SessionOptions::dock`): doldurmanın tüketicisi dock'lu
        // pencere ve ayrım oturum doğarken kararlaşıyor. Aynı sahne, tek fark
        // bayrak — yani bu sınama **geri alma şeridinin** kendisi (R2.4):
        // dock'suz pencerede sınırdan geçen kare bugünküyle bit bit aynı.
        let (session, _wake) = gapped_session(false);

        let (cursor, cells) = fill_now(&session);
        // Dock'suz pencerede doluluk imleci de sayıyor (`caret_in_dock`
        // yanlış), yani delik bir satır küçük — ama hâlâ var.
        assert!(cursor.rows - cursor.content_rows > 0, "{cursor:?}");
        assert_eq!(cursor.fill, 0, "dock'suz pencere doldu: {cursor:?}");
        assert!(cells.is_empty(), "ikinci sink boşuna çağrıldı: {cells:?}");
    }

    /// Shift'siz kesirli tekerlek, işaretçi sol üstte — [`scroll`]'un
    /// kesirli kardeşi. Tam satır hâli yalnız ok ve rapor kollarının girdisi
    /// ve bu sınamalar birincil ekranda, yani kesmenin biçimi önemsiz.
    fn smooth(session: &Session, rows: f64, intent: ScrollIntent) -> Wheel {
        session.scroll_wheel(
            rows,
            rows.trunc() as i32,
            intent,
            at(0, 0, CellHalf::Left),
            false,
        )
    }

    /// Pencerenin **görsel** konumu, satır cinsinden: ekranın tepesindeki
    /// satırın derinliği artı kesir. Dibe yaslı pencerede tepe bandın tepesi
    /// (bant bir sanal kaydırma, [`Session::fill_shown`]); ofset ancak
    /// banttan sonra sayıyor.
    fn position(cursor: &Cursor) -> f64 {
        let top = if cursor.display_offset == 0 {
            i32::from(cursor.fill)
        } else {
            cursor.display_offset
        };
        f64::from(top) + f64::from(cursor.scroll_frac)
    }

    #[test]
    fn fractional_deltas_move_the_screen_continuously() {
        // **R1.1'in bekçisi**: kesirli deltaların toplamı tam satırları
        // **sürekli** üretiyor — konum her adımda tam olarak delta kadar
        // oynuyor ve tam satır geçtiğinde ekran tek bir satır kayıyor.
        // Bantlı ve bantsız pencere ayrı sahneler, çünkü bandın ilk çentiği
        // ofseti `band + 1`'e taşıyor ([`scroll_locked`]) ve kesir o
        // eşlemenin üstünden geçmek zorunda — `0.75 + 0.25` bant boyunca
        // sıçramamalı.
        for (name, (session, _wake)) in [
            ("bantlı", gapped_session(true)),
            ("bantsız", history_session("stty -echo; seq 1 30; sleep 5")),
        ] {
            let (start, top) = screen_now(&session);
            assert_eq!(start.display_offset, 0, "{name}: {start:?}");
            let mut prev = (start, top.clone());
            // Üç satır yukarı, üç satır geri; çeyrek satırlık adımlarla.
            for (rows, step) in std::iter::repeat_n(0.25, 12)
                .chain(std::iter::repeat_n(-0.25, 12))
                .zip(1..)
            {
                smooth(&session, rows, ScrollIntent::Direct);
                let (cursor, now) = screen_now(&session);
                assert!(
                    (position(&cursor) - position(&prev.0) - rows).abs() < 1e-6,
                    "{name}, adım {step}: konum deltayı izlemedi\n  önce: {:?}\n  sonra: {cursor:?}",
                    prev.0
                );
                assert!(
                    (0.0..1.0).contains(&cursor.scroll_frac),
                    "{name}, adım {step}: {cursor:?}"
                );
                let old = &prev.1;
                let up = now[1..] == old[..old.len() - 1];
                let down = now[..now.len() - 1] == old[1..];
                assert!(
                    now == *old || up || down,
                    "{name}, adım {step}: ekran bir satırdan fazla oynadı\n  önce: {old:?}\n  sonra: {now:?}"
                );
                prev = (cursor, now);
            }
            // Dönüşün sonu başlangıcın ta kendisi.
            let (end, back) = screen_now(&session);
            assert_eq!(end.scroll_frac, 0.0, "{name}: {end:?}");
            assert_eq!(end.fill, start.fill, "{name}: bant geri gelmedi: {end:?}");
            assert_eq!(back, top, "{name}: ekran başladığı yere dönmedi");
        }
    }

    #[test]
    fn the_top_row_is_the_row_above_the_screen() {
        // **R1.4**: kesir sıfırdan büyükken doldurma kanalının en üstünde
        // ekranın tepesinin hemen üstündeki satır geliyor. Ölçüt sayının
        // kendisi değil **süreklilik**: kesir tamamlanınca ızgaranın ilk
        // satırı tam da o satır olmalı, yoksa yarım satırdan tam satıra
        // geçerken tepe başka bir şeye dönerdi.
        //
        // Kaydırılmış pencere: `Line(-offset - 1)`. Ekranın dipte tepesi
        // `22`, yani iki satır yukarıda tepe `20` ve üstü `19`.
        let (session, _wake) = history_session("stty -echo; seq 1 30; sleep 5");
        smooth(&session, 2.5, ScrollIntent::Direct);
        let (cursor, cells) = fill_now(&session);
        assert_eq!(cursor.display_offset, 2, "{cursor:?}");
        assert_eq!(cursor.scroll_frac, 0.5, "{cursor:?}");
        assert_eq!(cursor.top_row, 1, "{cursor:?}");
        // Kanal yalnız tepe satırı taşıyor: bant kaydırılmış pencerede yok ve
        // `fill` onu saymıyor.
        assert_eq!(cursor.fill, 0, "{cursor:?}");
        assert_eq!(row_text(&cells, 0), "19", "{cells:?}");
        smooth(&session, 0.5, ScrollIntent::Direct);
        let (whole, top) = screen_now(&session);
        assert_eq!(whole.top_row, 0, "{whole:?}");
        assert_eq!(top[0], "19", "kesir tamamlanınca tepe başka bir satır oldu");

        // Bantlı dip: tepe satırı **bandın** üstünde ve bandın satırları onun
        // altına iniyor. Bant `17`…`21`'i gösteriyor, üstü `16`.
        let (session, _wake) = gapped_session(true);
        let before = cursor_now(&session);
        smooth(&session, 0.5, ScrollIntent::Direct);
        let (cursor, cells) = fill_now(&session);
        assert_eq!(cursor.display_offset, 0, "{cursor:?}");
        assert_eq!(
            cursor.fill, before.fill,
            "bandın boyu kesirden etkilendi: {cursor:?}"
        );
        assert_eq!(cursor.top_row, 1, "{cursor:?}");
        let channel: Vec<String> = (0..cursor.top_row + cursor.fill)
            .map(|row| row_text(&cells, row))
            .collect();
        assert_eq!(channel, ["16", "17", "18", "19", "20", "21"], "{cells:?}");
        // Kesrin bandın sanal kaydırmasına dokunmadığının tanığı: tekerleğin
        // okuduğu sayı hâlâ bandın kendisi.
        assert_eq!(
            session.fill_shown.load(Ordering::Relaxed),
            before.fill,
            "tepe satırı bandın boyuna karıştı"
        );
    }

    #[test]
    fn the_top_row_ignores_the_bands_gates() {
        // **Tepe satırının tek kapısı defter** (`discussion.md` → Muhakeme):
        // bandın kapıları burada yok. Ctrl-L'den sonra bant kapalı (bayrak),
        // ama yukarı çıkan kullanıcı kesirde boş bir yarım satır görmemeli.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[H'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);
        session.write(b"\n");
        wait_until("ekran temizlenmedi", Duration::from_secs(5), || {
            cursor_now(&session);
            session.screen_cleared.load(Ordering::Relaxed)
        });
        smooth(&session, 0.5, ScrollIntent::Direct);
        let cursor = cursor_now(&session);
        assert_eq!(cursor.fill, 0, "bayrak kuruluyken bant açıldı: {cursor:?}");
        assert_eq!(cursor.scroll_frac, 0.5, "{cursor:?}");
        assert_eq!(
            cursor.top_row, 1,
            "Ctrl-L tepe satırını kapattı: {cursor:?}"
        );

        // Dock'suz pencerede de (bandın ilk kapısı): kesir bant yokken de
        // geçerli.
        let (session, _wake) = gapped_session(false);
        smooth(&session, 0.25, ScrollIntent::Direct);
        let cursor = cursor_now(&session);
        assert_eq!(cursor.top_row, 1, "{cursor:?}");
        assert_eq!(cursor.fill, 0, "{cursor:?}");
    }

    #[test]
    fn a_whole_row_frame_has_no_top_row() {
        // Kesir sıfırken kanal bugünküyle aynı: tepe satırı yok, dock'suz
        // dolu pencerede ikinci sink hiç çağrılmıyor.
        let (session, _wake) = history_session("stty -echo; seq 1 30; sleep 5");
        let (cursor, cells) = fill_now(&session);
        assert_eq!((cursor.scroll_frac, cursor.top_row), (0.0, 0), "{cursor:?}");
        assert!(cells.is_empty(), "ikinci sink boşuna çağrıldı: {cells:?}");
        // Tam satır yolu (`off`) kesir doğurmuyor.
        assert_eq!(
            smooth(&session, 1.0, ScrollIntent::Lines),
            Wheel::Scrolled(1)
        );
        let (cursor, cells) = fill_now(&session);
        assert_eq!((cursor.scroll_frac, cursor.top_row), (0.0, 0), "{cursor:?}");
        assert!(cells.is_empty(), "{cells:?}");
    }

    #[test]
    fn the_edges_keep_no_fraction_and_ask_for_no_frame() {
        // **R1.2**: dipte negatif, geçmişin tepesinde pozitif kesir kalmıyor
        // ve hiçbir şeyi değiştirmeyen olay kare istemiyor — momentum uçta da
        // olay yağdırıyor ve her biri boş bir kare olurdu.
        let (session, wake) = history_session("stty -echo; seq 1 30; sleep 5");
        let woken = wakes(&wake);
        assert_eq!(
            smooth(&session, -0.5, ScrollIntent::Direct),
            Wheel::Scrolled(0)
        );
        assert_eq!(
            smooth(&session, -1.0, ScrollIntent::Glide),
            Wheel::Scrolled(0)
        );
        assert_eq!(cursor_now(&session).scroll_frac, 0.0);
        assert_eq!(session.take_scroll_glide().rows, 0.0, "dipte istek birikti");
        assert_eq!(wakes(&wake), woken, "dipte kare istendi");

        // Geçmişin tepesi: 21 satırlık defter.
        smooth(&session, 100.0, ScrollIntent::Direct);
        let top = cursor_now(&session);
        assert_eq!((top.display_offset, top.scroll_frac), (21, 0.0), "{top:?}");
        let woken = wakes(&wake);
        assert_eq!(
            smooth(&session, 0.5, ScrollIntent::Direct),
            Wheel::Scrolled(0)
        );
        assert_eq!(
            smooth(&session, 1.0, ScrollIntent::Glide),
            Wheel::Scrolled(0)
        );
        assert_eq!(cursor_now(&session).scroll_frac, 0.0, "tepede kesir kaldı");
        assert_eq!(
            session.take_scroll_glide().rows,
            0.0,
            "tepede istek birikti"
        );
        assert_eq!(wakes(&wake), woken, "tepede kare istendi");

        // Uçtan geri dönüş serbest ve kare istiyor.
        assert_eq!(
            smooth(&session, -0.25, ScrollIntent::Direct),
            Wheel::Scrolled(-1)
        );
        let back = cursor_now(&session);
        assert_eq!(
            (back.display_offset, back.scroll_frac),
            (20, 0.75),
            "{back:?}"
        );
        assert!(wakes(&wake) > woken, "geri dönüş kare istemedi");
    }

    #[test]
    fn output_while_scrolled_keeps_the_fraction() {
        // **R1.3, dış yazıcı**: geçmişteyken gelen çıktı ofseti alacritty'nin
        // kendi kuralıyla artırıyor (görünen satırlar yerinde kalsın) ve kesre
        // dokunmuyor; sonraki delta yeni ofsetten devam ediyor, tam satırı
        // geri almıyor. Mutlak bir konum tutulsaydı tam da burada ezerdi.
        //
        // Satır sonu `send`'den gidiyor, `write`'tan değil: kullanıcı girdisi
        // pencereyi dibe döndürür ve sahne kendi kendini sıfırlardı.
        let (session, _wake) = history_session("stty -echo; seq 1 30; read _; seq 31 33; sleep 5");
        smooth(&session, 2.5, ScrollIntent::Direct);
        session.send(Msg::Input(b"\n".to_vec().into()));
        wait_until(
            "çıktı geçmişi büyütmedi",
            Duration::from_secs(5),
            || display_offset(&session) == 5,
        );
        let cursor = cursor_now(&session);
        assert_eq!(cursor.scroll_frac, 0.5, "çıktı kesre dokundu: {cursor:?}");

        smooth(&session, 0.25, ScrollIntent::Direct);
        let cursor = cursor_now(&session);
        assert_eq!(
            (cursor.display_offset, cursor.scroll_frac),
            (5, 0.75),
            "delta çıktının taşıdığı ofseti geri aldı: {cursor:?}"
        );
    }

    #[test]
    fn input_and_a_page_reset_the_fraction_and_the_generation() {
        // **R1.3, iç yazıcılar**: girdide dibe dönüş ve Shift+PgUp kesri
        // sıfırlıyor, bekleyen isteği düşürüyor ve nesli artırıyor. Nesil
        // kare yolunun payını da eliyor: sıfırlamadan önce hesaplanmış bir
        // pay dibe dönen pencereyi geri çekmemeli.
        let (session, wake) = history_session("stty -echo; seq 1 30; sleep 5");
        let start = cursor_now(&session).scroll_generation;
        smooth(&session, 1.5, ScrollIntent::Direct);
        smooth(&session, 1.0, ScrollIntent::Glide);

        session.write(b"x");
        let cursor = cursor_now(&session);
        assert_eq!(
            (cursor.display_offset, cursor.scroll_frac),
            (0, 0.0),
            "{cursor:?}"
        );
        assert_eq!(cursor.scroll_generation, start + 1, "{cursor:?}");
        let pending = session.take_scroll_glide();
        assert_eq!(pending.rows, 0.0, "girdi bekleyen isteği düşürmedi");
        assert_eq!(pending.generation, start + 1);

        // Eski neslin payı düşüyor, güncelinki uygulanıyor — ve kare yolunun
        // payı **uyandırmıyor**: kareyi zaten çizen taraf istiyor.
        let woken = wakes(&wake);
        let stale = ScrollGlide {
            rows: 0.5,
            generation: start,
        };
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            stale,
            BUDGET,
        );
        assert_eq!(cursor.scroll_frac, 0.0, "eski neslin payı uygulandı");
        let live = ScrollGlide {
            rows: 0.5,
            generation: start + 1,
        };
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut SearchRuns::default(),
            &mut Clusters::default(),
            live,
            BUDGET,
        );
        assert_eq!(cursor.scroll_frac, 0.5, "{cursor:?}");
        assert_eq!(wakes(&wake), woken, "kare yolunun payı uyandırdı");

        // Shift+PgUp: tam bir sayfa, kesirsiz.
        assert_eq!(session.scroll_page(1), Some(10));
        let cursor = cursor_now(&session);
        assert_eq!(
            (cursor.display_offset, cursor.scroll_frac),
            (10, 0.0),
            "{cursor:?}"
        );
        assert_eq!(cursor.scroll_generation, start + 2, "{cursor:?}");
    }

    #[test]
    fn notches_settles_and_momentum_are_glide_requests() {
        // Olay yolunun üç istek kolu (R1.5): çentik isteği biriktirip kare
        // istiyor, yerleşme payını kesirden hesaplıyor, momentum başı
        // bekleyeni düşürüp nesli artırıyor. Payı teslim eden kare yolu.
        let (session, wake) = history_session("stty -echo; seq 1 30; sleep 5");
        let woken = wakes(&wake);
        assert_eq!(
            smooth(&session, 1.0, ScrollIntent::Glide),
            Wheel::Scrolled(0)
        );
        assert!(wakes(&wake) > woken, "çentik kare istemedi");
        assert_eq!(
            display_offset(&session),
            0,
            "çentik ofseti olay anında oynattı"
        );
        assert_eq!(session.take_scroll_glide().rows, 1.0);
        assert_eq!(
            session.take_scroll_glide().rows,
            0.0,
            "istek alınınca sıfırlanmadı"
        );

        // Yerleşme en yakın satıra: `0.7` → `+0.3`.
        smooth(&session, 0.7, ScrollIntent::Direct);
        smooth(&session, 0.0, ScrollIntent::Settle);
        let settle = session.take_scroll_glide();
        assert!((settle.rows - 0.3).abs() < 1e-6, "{settle:?}");
        // Tam satırdaki yerleşme hiçbir şey istemiyor.
        let (session, wake) = history_session("stty -echo; seq 1 30; sleep 5");
        let woken = wakes(&wake);
        smooth(&session, 0.0, ScrollIntent::Settle);
        assert_eq!(session.take_scroll_glide().rows, 0.0);
        assert_eq!(wakes(&wake), woken, "tam satırdaki yerleşme kare istedi");

        // Momentum başı bekleyeni düşürüyor ve nesli artırıyor; kendi
        // deltası doğrudan uygulanıyor.
        let generation = cursor_now(&session).scroll_generation;
        smooth(&session, 1.0, ScrollIntent::Glide);
        smooth(&session, 0.25, ScrollIntent::GestureBegan);
        let cursor = cursor_now(&session);
        assert_eq!(cursor.scroll_frac, 0.25, "{cursor:?}");
        let pending = session.take_scroll_glide();
        assert_eq!(pending.rows, 0.0, "momentum bekleyen isteği düşürmedi");
        assert_eq!(pending.generation, generation + 1);
    }

    #[test]
    fn a_settle_delivered_in_f32_lands_on_a_whole_row() {
        // `/code-review` (027 phase-1): yerleşmenin payı `f32`'de teslim
        // ediliyor ve `0.3 + (−0.30000001)` `floor`'da bir satır aşağı düşüp
        // pencereyi `0.99999998` kesirle bırakıyordu. Kesirlerin dördü de
        // ölçüldü; hepsi tam satıra oturmalı.
        let (session, _wake) = history_session("stty -echo; seq 1 30; sleep 5");
        smooth(&session, 5.0, ScrollIntent::Direct);
        for frac in [0.1, 0.2, 0.3, 0.4, 0.6, 0.7] {
            smooth(&session, frac, ScrollIntent::Direct);
            smooth(&session, 0.0, ScrollIntent::Settle);
            let glide = session.take_scroll_glide();
            let cursor = session.frame(
                |_| (),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut Clusters::default(),
                glide,
                BUDGET,
            );
            assert_eq!(
                (cursor.display_offset, cursor.scroll_frac, cursor.top_row),
                (if frac < 0.5 { 5 } else { 6 }, 0.0, 0),
                "{frac}: {cursor:?}"
            );
            // Bir sonraki tur aynı tabandan.
            smooth(
                &session,
                5.0 - f64::from(cursor.display_offset),
                ScrollIntent::Direct,
            );
        }
    }

    #[test]
    fn a_huge_glide_does_not_poison_the_request() {
        // Sonlu ama dev bir delta `f32`'de sonsuza taşsaydı ters yöndeki ilk
        // ekleme isteği NaN yapar ve süzülme sessizce ölürdü.
        let (session, _wake) = history_session("stty -echo; seq 1 30; sleep 5");
        smooth(&session, 1e300, ScrollIntent::Glide);
        smooth(&session, -1e300, ScrollIntent::Glide);
        smooth(&session, 1.0, ScrollIntent::Glide);
        let glide = session.take_scroll_glide();
        assert!(glide.rows.is_finite(), "{glide:?}");
    }

    #[test]
    fn a_whole_row_step_and_mouse_mode_retire_the_fraction() {
        // Satır adımı (`off`) konumu dışarıdan sıfırlayan bir yol: kesir
        // düşüyor ve nesil artıyor, yoksa uçuştaki süzülmenin payı güncel
        // nesille gelip kesri geri getirirdi (`/code-review`, 027 phase-1).
        let (session, _wake) =
            history_session("stty -echo; seq 1 30; read _; printf '\\033[?1000h'; sleep 5");
        smooth(&session, 2.5, ScrollIntent::Direct);
        let before = cursor_now(&session).scroll_generation;
        smooth(&session, 1.0, ScrollIntent::Lines);
        let cursor = cursor_now(&session);
        assert_eq!(cursor.scroll_frac, 0.0, "{cursor:?}");
        assert_eq!(cursor.scroll_generation, before + 1, "{cursor:?}");

        // Fare kipine geçen birincil ekran uygulaması: tekerlek artık rapor
        // ve kesri hiçbir olay silemez, yani kare yolu siliyor. Satır sonu
        // `send`'den — kullanıcı girdisi kesri zaten sıfırlardı.
        smooth(&session, 0.5, ScrollIntent::Direct);
        assert_eq!(cursor_now(&session).scroll_frac, 0.5);
        session.send(Msg::Input(b"\n".to_vec().into()));
        wait_until(
            "fare kipi kesri düşürmedi",
            Duration::from_secs(5),
            || cursor_now(&session).scroll_frac == 0.0,
        );
    }

    /// Dolu ızgaraya `read`'in ardından `after` betiğini koşturur ve
    /// kareler boyunca [`Cursor::scrolled`]'ı toplar, `needle` görünüp akış
    /// durulana kadar. Son karenin imlecini ve hücrelerini de verir.
    ///
    /// Toplam, tek kare değil: çıktının kaç kareye bölüneceği PTY okumasının
    /// keyfi ve bekçinin sorusu "toplam kaç satır kaydı".
    fn scrolled_total(
        session: &Session,
        wake: &TestWake,
        needle: &str,
    ) -> (u32, Cursor, Vec<Cell>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut total = 0u32;
        let mut seen = 0;
        loop {
            assert!(
                Instant::now() < deadline,
                "beklenen çıktı gelmedi: {needle}"
            );
            seen = wake.wait_wakes(seen + 1, Duration::from_millis(500));
            let mut cells = Vec::new();
            if let Some(cursor) = frame_if_damaged(session, |c| cells.push(c)) {
                // **Bileşim**: kayan her satırın bandı da aynı karede geliyor,
                // yoksa kaymanın açtığı şerit bir kare boş kalırdı. Bu
                // yardımcının pencereleri dolu, yani bandın tamamı uzantı;
                // ekran boyu kaydırmada kayma bitiriliyor ve bant da yok.
                assert!(
                    cursor.scrolled == 0
                        || cursor.scrolled >= cursor.rows
                        || cursor.fill >= cursor.scrolled,
                    "kayma bandsız: {cursor:?}"
                );
                total += u32::from(cursor.scrolled);
                if glyph_text(&cells).contains(needle) {
                    // Akışın kuyruğu: aynı okumada gelmemiş bir satır sonu
                    // daha kaydırabilir.
                    std::thread::sleep(Duration::from_millis(100));
                    let mut tail = Vec::new();
                    let last = frame_if_damaged(session, |c| tail.push(c));
                    if let Some(last) = last {
                        total += u32::from(last.scrolled);
                        return (total, last, tail);
                    }
                    return (total, cursor, cells);
                }
            }
        }
    }

    #[test]
    fn a_full_grid_reports_the_rows_that_scrolled_off_the_top() {
        // **Kullanıcının bildirdiği kusurun bekçisi** (2026-09-23): ızgara
        // dolunca yeni satırlar süzülmeden sıçrıyordu, çünkü doluluk
        // sabitleniyor ve animatörün tek girdisi oydu. Kaydırmanın sayısı
        // sınırdan ayrı bir alanla geçiyor ve üç satırlık çıktı dolu ızgarayı
        // tam üç satır kaydırır.
        let (session, wake) = history_session("stty -echo; seq 1 30; read _; seq 31 33; sleep 5");
        // Karşılaştırmanın tabanı: kimliği yazan bir kare.
        assert_eq!(cursor_now(&session).scrolled, 0);
        session.write(b"\n");
        let (total, cursor, _) = scrolled_total(&session, &wake, "33");
        assert_eq!(total, 3, "{cursor:?}");
        // Doluluk hâlâ tavanda — animatörün eski girdisi hiçbir şey görmüyor.
        assert_eq!(cursor.content_rows, cursor.rows, "{cursor:?}");
    }

    #[test]
    fn the_scroll_count_survives_a_saturated_history() {
        // **Ölçüt defterin boyu değil satırın kimliği** ve bu sınama sebebi:
        // `history_size()` `scrollback`'te doyuyor ve oradan türeyen sayı
        // uzun bir oturumda sessizce sıfıra inerdi — aynı kusur on bin satır
        // sonra. Defter burada beş satırda doyuyor.
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(sh("stty -echo; seq 1 30; read _; seq 31 33; sleep 5"), 40);
        options.terminal.scrollback = 5;
        let session = Session::spawn(options, Arc::clone(&wake) as Arc<dyn Wake>).unwrap();
        wait_seq_tail(&session, &wake);
        assert_eq!(session.term.lock().history_size(), 5, "defter doymadı");
        assert_eq!(cursor_now(&session).scrolled, 0);
        session.write(b"\n");
        let (total, cursor, _) = scrolled_total(&session, &wake, "33");
        assert_eq!(total, 3, "{cursor:?}");
    }

    #[test]
    fn a_deliberate_clear_is_not_a_scroll() {
        // `CSI 2 J` görünen satırları geçmişe itiyor; kaydırma diye sayılsaydı
        // temizlenen ekran yukarı süzülerek giderdi.
        let (session, wake) = history_session(
            "stty -echo; seq 1 30; read _; printf '\\033[2J\\033[Hcleared'; sleep 5",
        );
        assert_eq!(cursor_now(&session).scrolled, 0);
        session.write(b"\n");
        let (total, cursor, _) = scrolled_total(&session, &wake, "cleared");
        assert_eq!(total, 0, "{cursor:?}");
    }

    #[test]
    fn the_band_grows_over_the_rows_that_just_scrolled_off() {
        // Kayma uçuştayken ızgara hedefinin altında çiziliyor; tepesinde
        // açılan şeridi az önce kayan satırlar kapatıyor. Dolu ve dock'suz
        // pencere: kalıcı boşluk yok, yani bandın tamamı geçici uzantı.
        let (session, wake) = history_session("stty -echo; seq 1 30; read _; seq 31 33; sleep 5");
        assert_eq!(cursor_now(&session).fill, 0, "dolu ızgarada bant vardı");
        session.write(b"\n");
        wait_ink(&session, &wake, "33");
        wait_settled(&session);
        // Çizen taraf: "ızgara şu an üç satır aşağıda".
        session.set_grid_top(3, 0);
        let (cursor, cells) = fill_now(&session);
        assert_eq!(cursor.fill, 3, "{cursor:?}");
        // Ekranın tepesi `25` (`26..33` ve imlecin satırı); üstündekiler
        // geçmişin en yenileri.
        let text: Vec<String> = (0..cursor.fill).map(|r| row_text(&cells, r)).collect();
        assert_eq!(text, ["22", "23", "24"], "{cells:?}");
        // Sanal kaydırmanın boyu uzantıyı saymıyor: tekerlek dolu ızgarada
        // hâlâ `0`'dan başlıyor.
        assert_eq!(session.fill_shown.load(Ordering::Relaxed), 0);
        // Kayma yerleşti: bant eski boyuna döner.
        session.set_grid_top(0, 0);
        assert_eq!(fill_now(&session).0.fill, 0);
    }

    #[test]
    fn a_lowered_grid_keeps_its_strip_filled_while_scrolled_back() {
        // **Bandın kısalığı** (036 Karar 8): uzak oturumda ızgara bir satır
        // artı boşluk aşağıda ve tepedeki şerit geçmişe bakarken de açık.
        // Kaydırılmış pencerede yalnız o pay kapatılıyor — her çentikte aynı
        // sayı, yani ekran çentik başına tam bir satır kayıyor. Kaymanın
        // geçici uzantısı orada bugünkü gibi yok.
        let (session, _wake) = history_session("stty -echo; seq 1 30; read _; sleep 5");
        session.set_grid_top(2, 2);
        let (cursor, cells) = fill_now(&session);
        assert_eq!(cursor.fill, 2, "{cursor:?}");
        let text: Vec<String> = (0..cursor.fill).map(|r| row_text(&cells, r)).collect();
        assert_eq!(text, ["20", "21"], "{cells:?}");

        session.term.lock().scroll_display(Scroll::Delta(1));
        let (cursor, cells) = fill_now(&session);
        assert_eq!(cursor.display_offset, 1);
        assert_eq!(
            cursor.fill, 2,
            "kaydırılmış pencerede şerit açık: {cursor:?}"
        );
        let text: Vec<String> = (0..cursor.fill).map(|r| row_text(&cells, r)).collect();
        assert_eq!(text, ["19", "20"], "pencerenin tepesinin üstü: {cells:?}");

        // Yalnız kaymanın uzantısı: kaydırılmış pencerede bant yok.
        session.set_grid_top(2, 0);
        assert_eq!(fill_now(&session).0.fill, 0);
    }

    #[test]
    fn an_empty_history_and_the_alternate_screen_leave_the_gap_empty() {
        // (1) **Boş defter**: yeni oturumda `min(history_size, gap)` sıfır ve
        // hiçbir ek okuma yok — doldurma yokluktan içerik uydurmuyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session("stty -echo; printf 'a\\n'; sleep 5", Arc::clone(&wake));
        wait_ink(&session, &wake, "a");
        let (cursor, cells) = fill_now(&session);
        assert!(cursor.rows - cursor.content_rows > 0, "{cursor:?}");
        assert_eq!(cursor.fill, 0, "boş defterden satır doğdu: {cursor:?}");
        assert!(cells.is_empty(), "ikinci sink boşuna çağrıldı: {cells:?}");

        // (2) **Alternatif ekran**: vim ve htop ızgaranın tamamını
        // sahipleniyor, yani `gap` zaten sıfır. Kapı bu yüzden bugün ikinci
        // bir kilit — ama R2.2'nin yazdığı kapı o ve `content_rows`'un
        // alternatif ekran kolu değişirse tek tutan şey bu olur.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; seq 1 30; read _; printf '\\033[?1049h'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);
        session.write(b"\n");
        wait_until(
            "alternatif ekrana geçilmedi",
            Duration::from_secs(5),
            || {
                cursor_now(&session);
                session.alt_screen()
            },
        );
        let (cursor, cells) = fill_now(&session);
        assert_eq!(cursor.content_rows, cursor.rows, "{cursor:?}");
        assert_eq!(cursor.fill, 0, "alternatif ekran doldu: {cursor:?}");
        assert!(cells.is_empty(), "ikinci sink boşuna çağrıldı: {cells:?}");
    }

    #[test]
    fn scroll_moves_the_display_offset_and_marks_dirty() {
        // `Term::scroll_display` kareyi **kendisi istemez**: tek olayı
        // `MouseCursorDirty` ve `Adapter` onu yutuyor. Kirli bayrağı elle
        // dikilmezse kaydırma grid'de olur ama ekrana hiç çıkmaz — bu sınama
        // ilk `frame()`'de kırmızıya düşer.
        let (session, wake) = history_session("seq 1 30; sleep 5");

        let woken = wakes(&wake);
        assert_eq!(scroll(&session, 3), Wheel::Scrolled(3));
        assert_eq!(display_offset(&session), 3);
        // Uyandırma da elle: `frame()` bayrağı doğrudan okuduğu için bayrağı
        // diken ama uyandırmayı unutan bir `request_frame` aşağıdaki
        // `frame()`'den geçerdi — uygulamada ise duraklamış display link hiç
        // açılmaz ve kaydırma, ilgisiz bir PTY çıktısı gelene kadar boyanmazdı.
        assert!(wakes(&wake) > woken, "kaydırma uyandırmadı");
        let mut cells = Vec::new();
        let cursor = frame_if_damaged(&session, |c| cells.push(c)).expect("kaydırma kare istemeli");
        // Pencere gerçekten geriye gitti: en üst satır `22` değil `19`. İmleç
        // dipteki satırla birlikte pencerenin altına düştü.
        assert_eq!(row_text(&cells, 0), "19", "{cells:?}");
        assert!(!cursor.visible, "{cursor:?}");
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        // Geçmişin tepesine: 21 - 3 = 18 satır daha. `i32::MAX` taşma
        // bekçisi — alacritty ofseti `i32`'de topluyor (`offset + count`),
        // kırpılmayan bir delta debug derlemesinde panik, sürümde ters yöne
        // sarma olurdu. Giriş yolunda panik yok.
        assert_eq!(scroll(&session, i32::MAX), Wheel::Scrolled(18));
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        // Tepede kaymayan tekerlek **kare istemez**: trackpad momentumu
        // tepede de olay yağdırır ve her biri boş bir kare olurdu.
        let woken = wakes(&wake);
        assert_eq!(scroll(&session, 1), Wheel::Scrolled(0));
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "kaymayan kaydırma kare istedi"
        );
        assert_eq!(wakes(&wake), woken, "kaymayan kaydırma uyandırdı");

        // Dibe dönüş ters yönde aynı yol: imleç yeniden görünür.
        assert_eq!(scroll(&session, i32::MIN), Wheel::Scrolled(-21));
        let cursor = frame_if_damaged(&session, |_| ()).expect("dibe dönüş kare istemeli");
        assert!(cursor.visible, "{cursor:?}");

        // Sayfa = görünen satır sayısı (10), `Term`'den; kırpma aynı gövdede.
        assert_eq!(session.scroll_page(1), Some(10));
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        assert_eq!(session.scroll_page(i32::MAX), Some(11));
        assert_eq!(session.scroll_page(-1), Some(-10));

        // Birincil ekranda Shift tekerleği değiştirmiyor (alacritty de öyle):
        // karar tablosunda Shift yalnız alternate screen'in okunu keser.
        assert_eq!(
            session.scroll_wheel(
                f64::from(-1),
                -1,
                ScrollIntent::Lines,
                at(0, 0, CellHalf::Left),
                true
            ),
            Wheel::Scrolled(-1)
        );
    }

    /// PTY'ye giden baytları okumanın sahnesi — phase-2'nin `od` kalıbı, bir
    /// farkla: çocuk önce satır disiplinini susturuyor (`stty -echo -icanon`).
    /// Ok ve rapor dizilerinde `\n` yok, kanonik kipte `od` onları hiç
    /// görmezdi; yankı da ayrıca kare doğururdu. Ardından `setup` koşar (kip
    /// açan `printf`, gerekirse geçmiş), sonra `od` stdin'i hex'e döker.
    ///
    /// Sıra yük taşıyor: kipler `stty`'den **sonra** basılıyor, yani `ready`
    /// tuttuğunda satır disiplini de hazır. Dönen oturumun karesi durulmuştur.
    fn dump_session(
        cols: u16,
        setup: &str,
        ready: impl Fn(TermMode) -> bool,
    ) -> (Session, Arc<TestWake>) {
        let wake = Arc::new(TestWake::default());
        let script = format!("stty -echo -icanon; {setup}; exec od -An -tx1");
        let session = spawn_with_cols(sh(&script), cols, Arc::clone(&wake));
        wait_until("kip açılmadı", Duration::from_secs(5), || {
            ready(*session.term.lock().mode())
        });
        wait_settled(&session);
        (session, wake)
    }

    /// PTY'ye **tam olarak** `expected`'ın gittiğini `od` dökümünden okur.
    ///
    /// `od` 16 baytlık blok dolmadan döküm basmaz; blok nokta (`2e`) ile
    /// tamamlanıyor ve iğne dolguyu da taşıyor. Sayıyı çivileyen bu: bir
    /// fazla dizi bloğu erken doldurur ve dolgunun bir kısmını dışarıda
    /// bırakır, bir eksik dizi bloğu hiç doldurmaz — iki hâlde de iğne
    /// görünmez. `expected` boşsa iğne 16 nokta: "hiçbir şey gitmedi".
    /// Dolgu `write`'tan gidiyor; önceki adımın bloğu her zaman tam kapandığı
    /// için adımlar aynı oturumda sıralanabilir — **iğneleri farklı olduğu
    /// sürece**. `wait_ink` bütün ekrana bakıyor: önceki adımın dökümü ekranda
    /// kaldığı için aynı iğneyi soran ikinci adım hemen yeşil geçer (üstelik
    /// `od` tekrar eden bloğu `*` diye basıyor). "Hiçbir şey gitmedi" bu yüzden
    /// oturum başına en çok bir kez ve ilk adım olarak sorulur.
    fn expect_sent(session: &Session, wake: &TestWake, expected: &[u8]) {
        let fill = vec![b'.'; 16 - expected.len() % 16];
        session.write(&fill);
        let needle: String = expected
            .iter()
            .chain(&fill)
            .map(|b| format!("{b:02x}"))
            .collect();
        wait_ink(session, wake, &needle);
    }

    #[test]
    fn alternate_screen_wheel_sends_arrows() {
        // R3.3: `less` ve `man` tekerlekle kayar — satır başına bir ok, geriye
        // yukarı. DECSET 1007'yi kimse açmıyor: alacritty'de varsayılan açık.
        let (session, wake) = dump_session(40, "printf '\\033[?1049h'", |mode| {
            mode.contains(TermMode::ALT_SCREEN)
        });
        assert_eq!(scroll(&session, 3), Wheel::Sent);
        expect_sent(&session, &wake, &b"\x1b[A".repeat(3));
        assert_eq!(scroll(&session, -2), Wheel::Sent);
        expect_sent(&session, &wake, &b"\x1b[B".repeat(2));
        // Doyan delta (`bt-shell` `f64`'ten `i32`'ye) bir olayda en çok bir
        // sayfa, yani 10 ok: `i32::MAX` tekrarlık tampon kurulmaz.
        assert_eq!(scroll(&session, i32::MAX), Wheel::Sent);
        expect_sent(&session, &wake, &b"\x1b[A".repeat(10));
    }

    #[test]
    fn arrows_follow_decckm_from_keyboard_and_wheel() {
        // DECCKM kapalı: CSI. `\e[?2004h` yalnız hazır olma işareti — kapalı
        // DECCKM'in gözlenebilir bir kipi yok, 2004 oklara dokunmuyor.
        let (plain, wake) = dump_session(40, "printf '\\033[?2004h'", |mode| {
            mode.contains(TermMode::BRACKETED_PASTE)
        });
        plain.write_arrow(Arrow::Up);
        plain.write_arrow(Arrow::Left);
        expect_sent(&plain, &wake, b"\x1b[A\x1b[D");

        // DECCKM açık (less ve ncurses'ın `smkx`'i): SS3 — klavye de tekerlek
        // de, çünkü ikisi aynı yardımcıdan geçiyor.
        let (app, wake) = dump_session(40, "printf '\\033[?1049h\\033[?1h'", |mode| {
            mode.contains(TermMode::ALT_SCREEN | TermMode::APP_CURSOR)
        });
        app.write_arrow(Arrow::Down);
        assert_eq!(scroll(&app, 1), Wheel::Sent);
        expect_sent(&app, &wake, b"\x1bOB\x1bOA");
    }

    #[test]
    fn mouse_mode_wheel_sends_sgr_reports() {
        // Fare kipi alternate scroll'dan **önce** gelir: `mouse=a` açık vim
        // ok değil rapor bekler. Sahne bu yüzden alternate screen'de ve 1007
        // (varsayılan) açık — sıra ters olsa ok giderdi. Shift fare kipini
        // geçmiyor (değiştirici bitleri kapsam dışı, düğme kodu aynı).
        let (session, wake) = dump_session(
            40,
            "printf '\\033[?1049h\\033[?1000h\\033[?1006h'",
            |mode| mode.contains(TermMode::ALT_SCREEN | TermMode::SGR_MOUSE),
        );
        let pointer = at(4, 2, CellHalf::Right);
        assert_eq!(
            session.scroll_wheel(f64::from(2), 2, ScrollIntent::Lines, pointer, false),
            Wheel::Sent
        );
        expect_sent(&session, &wake, &b"\x1b[<64;5;3M".repeat(2));
        assert_eq!(
            session.scroll_wheel(f64::from(-1), -1, ScrollIntent::Lines, pointer, true),
            Wheel::Sent
        );
        expect_sent(&session, &wake, b"\x1b[<65;5;3M");
    }

    #[test]
    fn mouse_mode_wheel_sends_plain_and_utf8_reports() {
        // Düz kodlama (1006 yok): koordinat tek bayt. Grid 224 sütun, yani
        // 223. sütun ekranda ve sınır gerçek bir hücrede sınanıyor.
        let (plain, wake) = dump_session(224, "printf '\\033[?1000h'", |mode| {
            mode.contains(TermMode::MOUSE_REPORT_CLICK)
        });
        // Sığmayan koordinat hiçbir şey göndermez — önce, çünkü "hiçbir şey"
        // ancak boş blokta 16 nokta diye okunur.
        assert_eq!(
            plain.scroll_wheel(
                f64::from(1),
                1,
                ScrollIntent::Lines,
                at(223, 2, CellHalf::Left),
                false
            ),
            Wheel::Ignored
        );
        expect_sent(&plain, &wake, b"");
        assert_eq!(
            plain.scroll_wheel(
                f64::from(1),
                1,
                ScrollIntent::Lines,
                at(222, 2, CellHalf::Left),
                false
            ),
            Wheel::Sent
        );
        expect_sent(&plain, &wake, &[0x1b, b'[', b'M', 96, 255, 35]);

        // UTF-8 kodlama (1005): 95. sütundan itibaren koordinat iki bayt.
        let (utf8, wake) = dump_session(224, "printf '\\033[?1000h\\033[?1005h'", |mode| {
            mode.contains(TermMode::UTF8_MOUSE)
        });
        assert_eq!(
            utf8.scroll_wheel(
                f64::from(1),
                1,
                ScrollIntent::Lines,
                at(94, 2, CellHalf::Left),
                false
            ),
            Wheel::Sent
        );
        assert_eq!(
            utf8.scroll_wheel(
                f64::from(-1),
                -1,
                ScrollIntent::Lines,
                at(95, 2, CellHalf::Left),
                false
            ),
            Wheel::Sent
        );
        expect_sent(
            &utf8,
            &wake,
            &[
                0x1b, b'[', b'M', 96, 127, 35, 0x1b, b'[', b'M', 97, 0xc2, 0x80, 35,
            ],
        );
    }

    #[test]
    fn wheel_report_skips_a_pointer_in_history() {
        // Birincil ekranda fare kipi açık ve pencere geçmişte: işaretçinin
        // satırı uygulamanın ekranında yoksa rapor gitmez (alacritty
        // `point.line < 0`); varsa rapor **uygulamanın** satırını taşır.
        // Pencere `Term`'den doğrudan kaydırılıyor: fare kipinde tekerlek
        // kaydırmaz (rapor olur) ve `write` pencereyi dibe döndürür.
        let (session, wake) =
            dump_session(40, "seq 1 30; printf '\\033[?1000h\\033[?1006h'", |mode| {
                mode.contains(TermMode::SGR_MOUSE)
            });
        session.term.lock().scroll_display(Scroll::Delta(5));

        // Ofset 5: görünen 4. satır geçmişin son satırı.
        assert_eq!(
            session.scroll_wheel(
                f64::from(1),
                1,
                ScrollIntent::Lines,
                at(0, 4, CellHalf::Left),
                false
            ),
            Wheel::Ignored
        );
        expect_sent(&session, &wake, b"");
        // Yeniden 5 geri (dolgu `write`'ı dibe döndürdü): görünen 7. satır
        // uygulamanın 2. satırı → rapor 3 der.
        session.term.lock().scroll_display(Scroll::Delta(5));
        assert_eq!(
            session.scroll_wheel(
                f64::from(1),
                1,
                ScrollIntent::Lines,
                at(0, 7, CellHalf::Left),
                false
            ),
            Wheel::Sent
        );
        expect_sent(&session, &wake, b"\x1b[<64;1;3M");
    }

    #[test]
    fn wheel_is_ignored_without_alternate_scroll_or_with_shift() {
        // Karar tablosunun üçüncü satırı: alternate screen'de ok yoksa tekerlek
        // **yoksayılır** — birincil ekranın geçmişine de inmez. Birincil
        // ekranda geçmiş var (`seq`) ki kip kapısı olmasa kaydırmanın
        // gidebileceği bir yer olsun.
        //
        // Sonuç `Ignored` — `Scrolled(0)` **değil**: alacritty alternate grid'i
        // geçmişsiz kuruyor (`Grid::new(.., 0)`), yani kapı silinse de ofset
        // oynamaz ve kare istenmezdi. Ayrım tipte ve sınamanın kendisi.
        let (shifted, wake) = dump_session(40, "seq 1 30; printf '\\033[?1049h'", |mode| {
            mode.contains(TermMode::ALT_SCREEN)
        });
        assert_eq!(
            shifted.scroll_wheel(
                f64::from(3),
                3,
                ScrollIntent::Lines,
                at(0, 0, CellHalf::Left),
                true
            ),
            Wheel::Ignored
        );
        // Sıfır satır da sessiz: boş `Msg::Input` `EventLoop`'un yazıcısını
        // kalıcı olarak kilitlerdi (`Adapter::reply`).
        assert_eq!(scroll(&shifted, 0), Wheel::Ignored);
        // Sayfa yolu tablodan geçmiyor: `bt-shell` bu `None`'da Shift+PgUp'ı
        // uygulamaya düz PgUp olarak veriyor.
        assert_eq!(shifted.scroll_page(1), None);
        expect_sent(&shifted, &wake, b"");

        // `\e[?1007l`: uygulama tekerleğin ok olmasını istemedi. 1007 önce
        // kapanıyor ki hazır olma işareti (1049) ikisinin de işlendiğini söylesin.
        let (plain, wake) =
            dump_session(40, "seq 1 30; printf '\\033[?1007l\\033[?1049h'", |mode| {
                mode.contains(TermMode::ALT_SCREEN)
            });
        assert_eq!(scroll(&plain, 3), Wheel::Ignored);
        assert!(
            frame_if_damaged(&plain, |_| ()).is_none(),
            "yoksayılan tekerlek kare istedi"
        );
        expect_sent(&plain, &wake, b"");
    }

    #[test]
    fn wheel_to_the_app_requests_no_frame() {
        // Rapor ya da ok göndermek kare **istemez**: uygulama ekranını yeniden
        // çizince okuyucunun `Wakeup`'ı kareyi getirir. Boşta sıfır kare. Yankı
        // yok (`-echo`) ve gönderilen bayt 16'nın altında, yani `od` da basmıyor:
        // kare isteyebilecek tek aday gönderimin kendisi.
        let (arrows, wake) = dump_session(40, "printf '\\033[?1049h'", |mode| {
            mode.contains(TermMode::ALT_SCREEN)
        });
        let woken = wakes(&wake);
        assert_eq!(scroll(&arrows, 3), Wheel::Sent);
        assert!(
            frame_if_damaged(&arrows, |_| ()).is_none(),
            "ok kare istedi"
        );
        assert_eq!(wakes(&wake), woken, "ok uyandırdı");

        let (reports, wake) = dump_session(40, "printf '\\033[?1000h\\033[?1006h'", |mode| {
            mode.contains(TermMode::SGR_MOUSE)
        });
        let woken = wakes(&wake);
        assert_eq!(scroll(&reports, 1), Wheel::Sent);
        assert!(
            frame_if_damaged(&reports, |_| ()).is_none(),
            "rapor kare istedi"
        );
        assert_eq!(wakes(&wake), woken, "rapor uyandırdı");
    }

    #[test]
    fn input_returns_the_view_to_the_bottom() {
        // Geçmişe bakarken yazılan girdi pencereyi dibe döndürür. Dönmeseydi
        // kullanıcı göremediği bir satıra yazardı: alacritty kaydırılmış
        // pencereyi yeni çıktıya karşı **sabitliyor** (`grid.scroll_up`
        // ofseti artırıyor), yani `ls` + Enter'in çıktısı da görünmez kalırdı.
        // alacritty'nin ikilisi aynı şeyi tuş girdisinde yapıyor; kitaplık
        // yapmıyor.
        let (session, _wake) = history_session("seq 1 30; sleep 5");

        assert_eq!(scroll(&session, 5), Wheel::Scrolled(5));
        wait_settled(&session);
        session.write(b"x");
        assert_eq!(display_offset(&session), 0, "yazma dibe döndürmedi");
        // Kare `write` dönmeden istenmiş olmalı — yankının `Wakeup`'ını
        // beklemeden: dönüş, okuyucu thread'in işi değil.
        let cursor = frame_if_damaged(&session, |_| ()).expect("dibe dönüş kare istemeli");
        assert!(cursor.visible, "{cursor:?}");

        // Yapıştırma da aynı kapıdan: `paste` → `write_owned`.
        assert_eq!(scroll(&session, 5), Wheel::Scrolled(5));
        session.paste(b"y".to_vec());
        assert_eq!(display_offset(&session), 0, "yapıştırma dibe döndürmedi");

        // Ok tuşu da: kip sorusuyla dibe dönüş aynı kilitte.
        assert_eq!(scroll(&session, 5), Wheel::Scrolled(5));
        wait_settled(&session);
        session.write_arrow(Arrow::Up);
        assert_eq!(display_offset(&session), 0, "ok tuşu dibe döndürmedi");
        assert!(
            frame_if_damaged(&session, |_| ()).is_some(),
            "ok tuşunun dibe dönüşü kare istemeli"
        );

        // Boş girdi pencereye dokunmaz: yazılacak bayt yoksa dibe dönmek de
        // yok (ve `Term` kilidi hiç alınmaz).
        assert_eq!(scroll(&session, 5), Wheel::Scrolled(5));
        session.write(b"");
        session.paste(Vec::new());
        assert_eq!(display_offset(&session), 5, "boş girdi pencereyi oynattı");
    }

    #[test]
    fn input_clears_the_selection() {
        // Yazınca seçim kalkar (alacritty `on_terminal_input_start`). Üç
        // kullanıcı girdisi de aynı kapıdan (`send_input`) geçiyor; üçü de
        // sınanıyor ki biri o kapıyı by-pass edince görünsün.
        //
        // `stty -echo` şart: yankı kendi `Wakeup`'ını doğururdu ve "kare
        // istendi" iddiası temizleme olmadan da geçerdi. Uyku uzun: çocuk
        // çıkınca `ChildExit`'in `Wakeup`'ı sondaki "kare yok" iddiasını yavaş
        // bir koşuda kızartırdı.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; printf '\\033[41maraba\\033[0m'; sleep 60",
            Arc::clone(&wake),
        );
        assert_eq!(wait_cells(&session, &wake, 5).len(), 5);

        /// `araba`'yı seçer ve seçimin karesini tüketir: sonraki kare ancak
        /// seçimden sonra olan bir şeyin karesi olabilir.
        fn select_word(session: &Session) {
            session.set_selection(
                SelectKind::Simple,
                at(0, 0, CellHalf::Left),
                at(4, 0, CellHalf::Right),
            );
            wait_settled(session);
        }

        /// `araba` seçiliyken `input` koşar: seçim gitmeli, kare istenmiş ve
        /// vurgu ekrandan kalkmış olmalı.
        fn clears(session: &Session, what: &str, input: impl FnOnce(&Session)) {
            select_word(session);
            input(session);
            assert_eq!(session.selection_text(), None, "{what} seçimi temizlemedi");
            // Kare `input` dönmeden istenmiş olmalı: yankı yok, çocuk da
            // hiçbir şey basmıyor — isteyebilecek tek aday temizliğin kendisi.
            let mut cells = Vec::new();
            assert!(
                frame_if_damaged(session, |c| cells.push(c)).is_some(),
                "{what}: kalkan vurgu kare istemedi"
            );
            // Vurgu gerçekten gitti: beş hücre yeniden kendi kırmızısında.
            let red = Some(color::linear_rgba(THEME.default(1)));
            assert_eq!(
                backgrounds(&cells).filter(|c| c.bg == red).count(),
                5,
                "{what}: vurgu kaldı: {cells:?}"
            );
        }
        clears(&session, "yazma", |s| s.write(b"x"));
        clears(&session, "yapıştırma", |s| s.paste(b"y".to_vec()));
        clears(&session, "ok tuşu", |s| s.write_arrow(Arrow::Up));

        // Boş girdi seçime dokunmaz — pencereye de dokunmuyor
        // (`input_returns_the_view_to_the_bottom`): gönderilecek bayt yoksa
        // kullanıcı girdisi de yok.
        select_word(&session);
        session.write(b"");
        session.paste(Vec::new());
        assert_eq!(session.selection_text().as_deref(), Some("araba"));

        // Ekranda çizili aralık yoksa kare istenmez: sürüklemesiz tık boş seçim
        // bırakır ve temizliği her tıktan sonraki ilk tuşa boş bir kare
        // isteterdi.
        session.set_selection(
            SelectKind::Simple,
            at(2, 0, CellHalf::Right),
            at(2, 0, CellHalf::Right),
        );
        wait_settled(&session);
        session.write(b"x");
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "boş seçimin temizliği kare istedi"
        );

        // Temizlik ve dibe dönüş birlikte: **tek** uyandırma. Geçmişe bakan
        // pencerede çizili bir seçim var; girdi hem onu kaldırıyor hem pencereyi
        // dibe alıyor. İki ayrı istek vuruş başına display link'i iki kez
        // uyandırırdı. Uyandırma `write` dönmeden sayılır — yankı yok, çıktı
        // yok, başka aday yok.
        let (history, wake) = history_session("stty -echo; seq 1 30; sleep 60");
        assert_eq!(scroll(&history, 5), Wheel::Scrolled(5));
        history.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(1, 0, CellHalf::Right),
        );
        wait_settled(&history);
        let woken = wakes(&wake);
        history.write(b"x");
        assert_eq!(history.selection_text(), None);
        assert_eq!(display_offset(&history), 0, "yazma dibe döndürmedi");
        assert_eq!(wakes(&wake), woken + 1, "temizlik ve dönüş ayrı uyandırdı");

        // Seçim grid'de ama ekranda değil (pencere geçmişten dibe döndü):
        // temizlik kare istemez — kapı seçimin varlığına değil çizili aralığa
        // bakıyor. Seçim yine de gider, yoksa Cmd-C görünmeyen metni kopyalardı.
        assert_eq!(scroll(&history, 5), Wheel::Scrolled(5));
        history.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(1, 0, CellHalf::Right),
        );
        assert_eq!(scroll(&history, -5), Wheel::Scrolled(-5));
        wait_settled(&history);
        history.write(b"x");
        assert_eq!(history.selection_text(), None);
        assert!(
            frame_if_damaged(&history, |_| ()).is_none(),
            "görünmeyen seçimin temizliği kare istedi"
        );
    }

    #[test]
    fn wheel_and_replies_keep_the_selection() {
        // Tekerlek raporu ve tekerlek okları uygulamaya gidiyor, yanıt
        // uygulamanın sorusuna gidiyor — hiçbiri kullanıcının yazdığı bir şey
        // değil ve seçimi temizlemiyor. alacritty'de de öyle: `scroll_terminal`
        // ve `mouse_report` `write_to_pty`'ye doğrudan yazıyor,
        // `on_terminal_input_start`'tan geçmiyor.
        //
        // Kipler metinden **sonra** basılıyor: kip görününce `araba` da
        // ayrıştırılmış demek.
        let (start, end) = (at(0, 0, CellHalf::Left), at(4, 0, CellHalf::Right));
        let (reports, _wake) = dump_session(40, "printf 'araba\\033[?1000h\\033[?1006h'", |mode| {
            mode.contains(TermMode::SGR_MOUSE)
        });
        reports.set_selection(SelectKind::Simple, start, end);
        assert_eq!(scroll(&reports, 1), Wheel::Sent);
        assert_eq!(
            reports.selection_text().as_deref(),
            Some("araba"),
            "tekerlek raporu seçimi temizledi"
        );
        // Yanıt, okuyucu thread'in `PtyWrite` olayından — `Adapter`'ın gerçek
        // girişi.
        reports
            .adapter
            .send_event(Event::PtyWrite("\x1b[0n".to_owned()));
        assert_eq!(
            reports.selection_text().as_deref(),
            Some("araba"),
            "yanıt seçimi temizledi"
        );
        // Karşıt: aynı oturumda kullanıcı girdisi temizliyor.
        reports.write(b"x");
        assert_eq!(reports.selection_text(), None);

        let (arrows, _wake) = dump_session(40, "printf '\\033[?1049haraba\\033[?2004h'", |mode| {
            mode.contains(TermMode::ALT_SCREEN | TermMode::BRACKETED_PASTE)
        });
        arrows.set_selection(SelectKind::Simple, start, end);
        assert_eq!(scroll(&arrows, 1), Wheel::Sent);
        assert_eq!(
            arrows.selection_text().as_deref(),
            Some("araba"),
            "tekerlek oku seçimi temizledi"
        );
    }

    #[test]
    fn button_reports_keep_the_selection_and_the_scrollback() {
        // Düğme raporu `write_owned`'ın değil tekerleğin kolundan geçiyor
        // (`send`, `send_input` değil): rapor kullanıcının yazdığı bir şey
        // değil, uygulamaya iletilen bir olay. İki yan etki de sınanıyor,
        // çünkü `send_input` ikisini birden yapar.
        let (session, _wake) = dump_session(40, "printf 'araba\\033[?1000h\\033[?1006h'", |mode| {
            mode.contains(TermMode::SGR_MOUSE)
        });
        session.set_selection(
            SelectKind::Simple,
            at(0, 0, CellHalf::Left),
            at(4, 0, CellHalf::Right),
        );
        assert_eq!(press(&session, at(1, 0, CellHalf::Left)), Click::Sent);
        assert_eq!(
            session.selection_text().as_deref(),
            Some("araba"),
            "düğme raporu seçimi temizledi"
        );
        // Karşıt: aynı oturumda kullanıcı girdisi temizliyor.
        session.write(b"x");
        assert_eq!(session.selection_text(), None);

        // Geçmişe bakan pencere dibe fırlamıyor. Geçmiş **bir sayfadan kısa**
        // (14 satır, 10 satırlık ekran): bir sayfalık kaydırma geçmişin
        // ucunda duruyor ve alt satırlar uygulamanın ekranında kalıyor
        // (`row - offset >= 0`), yani rapor gerçekten gidiyor ve sorulan şey
        // ofset. Tam sayfalık geçmişte her görünür satır negatife düşerdi.
        let (scrolled, _wake) =
            dump_session(40, "seq 1 14; printf '\\033[?1000h\\033[?1006h'", |mode| {
                mode.contains(TermMode::SGR_MOUSE)
            });
        assert!(scrolled.scroll_page(1).is_some_and(|moved| moved > 0));
        assert_eq!(press(&scrolled, at(0, 9, CellHalf::Left)), Click::Sent);
        // Ölçüt "sıfır değil": `send_input` olsaydı ofset **dibe**, yani tam
        // sıfıra inerdi. Eşitlik sorulmuyor çünkü uygulamanın bu arada
        // basacağı bir satır ofseti meşru olarak artırır.
        assert!(
            display_offset(&scrolled) > 0,
            "düğme raporu pencereyi dibe döndürdü"
        );
    }

    #[test]
    fn motion_reports_follow_the_mode_not_the_pointer() {
        // 1002 yalnız basılıyken, 1003 her zaman. İki kip iki oturumda,
        // çünkü "hiçbir şey gitmedi" iğnesi oturum başına bir kez ve **ilk**
        // adım sorulabiliyor (`expect_sent`'in kuralı).
        let origin = at(0, 0, CellHalf::Left);
        let hover =
            |session: &Session| session.mouse_motion(None, origin, MouseModifiers::default());

        // 1002: düğmesiz hareket düşüyor, basılı hareket gidiyor.
        let (drag, wake) = dump_session(40, "printf '\\033[?1002h\\033[?1006h'", |mode| {
            mode.contains(TermMode::SGR_MOUSE)
        });
        assert_eq!(hover(&drag), Click::Ignored);
        expect_sent(&drag, &wake, b"");
        assert_eq!(
            drag.mouse_motion(Some(MouseButton::Left), origin, MouseModifiers::default()),
            Click::Sent
        );
        // `32` hareket biti, sol düğme `0`.
        expect_sent(&drag, &wake, b"\x1b[<32;1;1M");

        // 1003: düğmesiz hareket de gidiyor, düğme kodu `35` (`32 | 3`).
        let (motion, wake) = dump_session(40, "printf '\\033[?1003h\\033[?1006h'", |mode| {
            mode.contains(TermMode::SGR_MOUSE)
        });
        assert_eq!(hover(&motion), Click::Sent);
        expect_sent(&motion, &wake, b"\x1b[<35;1;1M");
    }

    #[test]
    fn release_follows_press() {
        // R6: basış raporlandıysa bırakma **düşürülmez**, kırpılır —
        // düşürmek uygulamada takılı kalmış bir düğme bırakırdı. Ölçüt bu
        // yüzden asimetrik ve asimetri bilerek: aynı koordinat basışta
        // reddediliyor (jest hiç başlamamış), bırakmada kırpılıyor (jest
        // zaten başlamış).
        let (session, wake) =
            dump_session(40, "seq 1 30; printf '\\033[?1000h\\033[?1006h'", |mode| {
                mode.contains(TermMode::SGR_MOUSE)
            });
        // Pencereyi geçmişe al: klavye kaydırması karar tablosundan geçmiyor,
        // tekerlek bu kipte rapora giderdi.
        assert!(session.scroll_page(1).is_some_and(|moved| moved > 0));
        let top = at(0, 0, CellHalf::Left);
        assert_eq!(press(&session, top), Click::Ignored, "satır geçmişte");
        assert_eq!(release(&session, top), Click::Sent, "bırakma düştü");
        // Dökümü okumak için dibe dön — kaydırılmış pencere yeni çıktıyı
        // göstermez ve `expect_sent` ekrandan okuyor.
        assert!(session.scroll_page(-2).is_some());
        // Kırpılmış koordinat: negatif satır 0'a, yani SGR'ın 1'ine iniyor.
        expect_sent(&session, &wake, b"\x1b[<0;1;1m");

        // Rota kilitli ama **kip yeniden soruluyor** (R6'nın daraltması):
        // basıştan sonra uygulama çıkıp `\e[?1000l` göndermişse bırakma
        // raporu kabuğun komut satırına düşerdi. "Hiçbir şey gitmedi" iğnesi
        // oturum başına bir kez ve **ilk** adım (`expect_sent`'in kuralı),
        // bu yüzden ayrı oturum.
        let (closed, wake) = dump_session(
            40,
            "printf '\\033[?1000h\\033[?1006h\\033[?1000l'",
            |mode| mode.contains(TermMode::SGR_MOUSE) && !mode.intersects(TermMode::MOUSE_MODE),
        );
        assert_eq!(release(&closed, at(0, 0, CellHalf::Left)), Click::Ignored);
        expect_sent(&closed, &wake, b"");
    }

    #[test]
    fn drag_anchor_survives_a_scroll() {
        // phase-1'in devri: basılı sürüklemenin ortasında kaydırma. Çapa
        // pencere satırı olarak tutulsaydı (eski `set_selection(çapa, uç)`
        // yolu) kaydırmadan sonra başka bir içeriğe işaret ederdi — `30`'dan
        // başlayan sürükleme `27`'den başlamış gibi olurdu. Çapa artık
        // alacritty'nin grid-mutlak başlangıcında yaşıyor.
        let (session, _wake) = history_session("seq 1 30; sleep 5");

        // Basış: `30`'un sıfırının sağ yarısı (8. satır, 1. sütun).
        let press = at(1, 8, CellHalf::Right);
        session.set_selection(SelectKind::Simple, press, press);
        // Tekerlek üç satır geriye, fare pencerenin tepesine: orada artık `19`.
        assert_eq!(scroll(&session, 3), Wheel::Scrolled(3));
        session.update_selection(at(0, 0, CellHalf::Left));

        let text = session.selection_text().expect("seçim metni");
        assert!(
            text.starts_with("19") && text.ends_with("30"),
            "çapa kaydırmayla kaydı: {text:?}"
        );
    }

    #[test]
    fn update_selection_without_a_selection_is_silent() {
        // Basışsız sürükleme (seçim yok) seçim doğurmaz ve kare istemez:
        // güncelleme yalnız var olan seçimin ucunu taşır.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));
        assert!(frame_if_damaged(&session, |_| ()).is_some());

        session.update_selection(at(3, 0, CellHalf::Right));
        assert_eq!(session.selection_text(), None);
        assert!(frame_if_damaged(&session, |_| ()).is_none());
    }

    #[test]
    fn shutdown_ends_the_reader() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 30", Arc::clone(&wake));
        assert!(session.reader_alive());

        let started = Instant::now();
        // Sonuç **döndürülüyor**: jeton satırı bunu basacak ve sınır dolan bir
        // koşunun yeşil görünmesi böyle bitiyor.
        assert_eq!(session.shutdown(), Teardown::Clean);
        assert!(!session.reader_alive());
        // `sleep 30` sürerken bile SIGHUP yolu hemen dönmeli.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        // İkinci çağrı sessizce döner ve bunu **söyler**: `Drop`'un çağrısı
        // buraya düşüyor ve onun `Clean` demesi ilk çağrının sonucunu silerdi.
        assert_eq!(session.shutdown(), Teardown::AlreadyDone);
    }

    /// `shutdown()`'ı ölçerek çağırır ve **üst** sınırı doğrular; dönen süre
    /// çağıranın kendi ölçütü için.
    ///
    /// Üst sınır tek yerde: sözleşme `SHUTDOWN_GRACE` artı zamanlama payı.
    /// Pay iki saniye, çünkü `make test-yaris` bütün takımı tek thread'de
    /// koşuyor ve o koşuda thread kurulumu gecikebilir — pay olmasa sınama
    /// sınırı değil makinenin yükünü ölçerdi.
    fn shutdown_within_grace(session: &Session) -> Duration {
        let started = Instant::now();
        session.shutdown();
        let elapsed = started.elapsed();
        assert!(
            elapsed < SHUTDOWN_GRACE + Duration::from_secs(2),
            "kapanış sınırı aşıldı: {elapsed:?}"
        );
        elapsed
    }

    /// **DEC 2026 bloğu kapanmasa da zaman aşımında uygulanıyor** — ve
    /// okuyucu döngünün kendi kolundan (035 phase-2): bloğun baytları
    /// ayrıştırıcının tamponunda bekliyor, `EventLoop` poller'ı vte'nin son
    /// tarihine kadar bekletiyor ve süre dolunca `stop_sync` tamponu
    /// sarmalayıcıya ([`crate::handler::ClusterHandler`]) veriyor.
    /// **Bir parite bekçisi**: kolun varlığını sınıyor, `Term`'e sarmalayıcıdan
    /// mı doğrudan mı gittiğini ayırt edemiyor (sarmalayıcı bugün düz
    /// aktarım); o ayrımı phase-3'ün kümeleme sınaması görecek.
    /// Kol düşseydi `synced` hiç görünmezdi: `sleep` boyunca başka bayt
    /// gelmiyor, yani tamponu boşaltacak ikinci bir yol yok.
    #[test]
    fn an_unterminated_synchronized_update_lands_on_timeout() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf '\\033[?2026hsynced'; sleep 5", wake);
        frame_until(&session, BUDGET, |cells, cursor| {
            grid_shows(cells, cursor.rows, "synced")
        });
    }

    /// **Zaman aşımıyla uygulanan DEC 2026 bloğu da kümeleniyor** (035
    /// phase-3; phase-2'nin parite bekçisinin açık kalan yarısı): blok
    /// `🇹🇷` taşıyor ve kapanmıyor, yani baytlar `Term`'e yalnız
    /// `stop_sync`'ten ulaşıyor. Sarmalayıcıdan geçmeselerdi iki RI iki dar
    /// hücre olurdu; kümeli oturumda tek geniş hücre ve `🇷` hiçbir hücrede
    /// yok.
    /// `🇹🇷` (iki RI), `👨‍👩‍👧` (ZWJ ailesi) ve tek sütunlu `é` (`e` + U+0301)
    /// yan yana, arkalarında bir çapa noktası.
    const CLUSTERS_PRINTF: &str = "printf '\\360\\237\\207\\271\\360\\237\\207\\267\
\\360\\237\\221\\250\\342\\200\\215\\360\\237\\221\\251\\342\\200\\215\\360\\237\\221\\247\
e\\314\\201.'; sleep 5";

    /// Kümelemeli (ya da kümelemesiz) bir oturumun çapaya kadarki karesi ve
    /// kare tablosu.
    fn cluster_frame(cluster: bool) -> (Session, Vec<Cell>, Clusters) {
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(sh(CLUSTERS_PRINTF), 40);
        options.cluster = cluster;
        let session = Session::spawn(options, wake).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let mut cells = Vec::new();
            let mut clusters = Clusters::default();
            session.frame(
                |cell| cells.push(cell),
                |_| (),
                &mut Blocks::default(),
                &mut SelectionRuns::default(),
                &mut SearchRuns::default(),
                &mut clusters,
                ScrollGlide::default(),
                BUDGET,
            );
            if cells.iter().any(|cell| cell.ch == Some('.')) {
                return (session, cells, clusters);
            }
            assert!(Instant::now() < deadline, "çapa gelmedi: {cells:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_wide_cluster_reaches_the_frame_as_one_string() {
        // 035 R4.1: baş hücre taban karakteri ve tablodaki dizgiyi taşıyor;
        // tek sütunlu birleştirici (`é`) bugünkü gibi taban karakter (Karar 6).
        let (session, cells, clusters) = cluster_frame(true);
        let text = |ch| {
            let cell = cells
                .iter()
                .find(|cell| cell.ch == Some(ch))
                .unwrap_or_else(|| panic!("{ch} çizilmedi: {cells:?}"));
            cell.cluster
                .and_then(|id| clusters.get(id).map(str::to_owned))
        };
        assert_eq!(text('🇹').as_deref(), Some("🇹🇷"));
        assert_eq!(
            text('👨').as_deref(),
            Some("👨\u{200D}👩\u{200D}👧"),
            "aile beş kod noktası"
        );
        assert_eq!(text('e'), None, "tek sütunlu birleştirici küme doğurdu");
        assert_eq!(text('.'), None);
        session.shutdown();
    }

    #[test]
    fn without_clustering_no_cell_carries_a_cluster() {
        // Bayrak kapalıyken kare bugünküyle aynı: tablo boş, kimlik yok.
        let (session, cells, clusters) = cluster_frame(false);
        assert!(cells.iter().all(|cell| cell.cluster.is_none()), "{cells:?}");
        assert!(clusters.is_empty());
        session.shutdown();
    }

    #[test]
    fn a_selected_family_copies_every_code_point() {
        // Izgara seçiminin kopyası kümeyi alacritty'nin satır metninden
        // bütün alıyor (035 R4.2) — kod değişmedi, sözleşme sabitlendi.
        let (session, cells, _) = cluster_frame(true);
        let family = cells
            .iter()
            .find(|cell| cell.ch == Some('👨'))
            .expect("aile çizilmedi");
        session.set_selection(
            SelectKind::Simple,
            at(family.col, family.row, CellHalf::Left),
            at(family.col + 1, family.row, CellHalf::Right),
        );
        assert_eq!(
            session.selection_text().as_deref(),
            Some("👨\u{200D}👩\u{200D}👧")
        );
        session.shutdown();
    }

    #[test]
    fn a_timed_out_synchronized_update_is_clustered() {
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(
            sh("printf '\\033[?2026h\\360\\237\\207\\271\\360\\237\\207\\267.'; sleep 5"),
            40,
        );
        options.cluster = true;
        let session = Session::spawn(options, wake).unwrap();
        let (cells, _) = frame_until(&session, BUDGET, |cells, cursor| {
            grid_shows(cells, cursor.rows, ".")
        });
        let flag = cells
            .iter()
            .find(|cell| cell.ch == Some('🇹'))
            .expect("bayrağın baş hücresi çizilmeli");
        assert!(flag.wide, "küme geniş hücre olmadı: {flag:?}");
        assert!(
            cells.iter().all(|cell| cell.ch != Some('🇷')),
            "ikinci RI kendi hücresini aldı — `stop_sync` sarmalayıcıyı atladı"
        );
    }

    #[test]
    fn shutdown_returns_within_limit() {
        let wake = Arc::new(TestWake::default());
        // `shutdown()`'ın eski doc'unun tek sebep gibi anlattığı senaryo:
        // sinyali **yutan** çocuk. `Pty::drop`'un `child.wait()`'i böyle bir
        // çocukta süresiz bekler, sınırlı bekleme onu kesiyor.
        //
        // `sleep` sonlu ve bu bir süs değil: `trap ''` sinyali `SIG_IGN`
        // yapıyor, `sleep` onu **miras alıyor** ve süreç çıkışı master'ı
        // kapatsa bile ikisi ölmüyor — sonsuz bir döngü her `cargo test`'ten
        // sonra makinede kalırdı. Beş saniye sınırın on katı: aşağıdaki alt
        // sınır erken ölen bir çocuğu kırmızıya çevirdiği için bu pay
        // kısaltılabilir ama sıfırlanamaz.
        let session = spawn_session(
            "trap '' HUP; printf '\\033[41mx\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );
        // Çapa: kırmızı hücre geldiyse `trap` satırı koştu ve çocuk
        // `sleep`'te. Çapa olmadan sınama boşuna yeşil kalabilir — `trap`'a
        // henüz varmamış bir çocuk `SIGHUP` ile zaten hemen ölür.
        wait_cells(&session, &wake, 1);

        let elapsed = shutdown_within_grace(&session);
        // İkinci yarı: sınır gerçekten **dolmuş** olmalı. Bu çocuk ölmüyor,
        // yani hemen dönen bir `shutdown` sınırı değil çocuğun erken
        // ölümünü ölçerdi ve sınama boşuna yeşil kalırdı.
        assert!(elapsed >= SHUTDOWN_GRACE, "{elapsed:?}");
    }

    #[test]
    fn shutdown_with_busy_writer() {
        let wake = Arc::new(TestWake::default());
        // Asılmanın **gerçek** üreticisi ve bu sette setin kendi yükü:
        // kapanış anında PTY'ye yazan çocuk, yani `shutdown()`'ın doc'undaki
        // ikinci sebep (çıkışın içinde takılma). Sınır olmasa bu sınama
        // asılırdı — takılma her koşuda değil, kuyruğun kapanış anında dolu
        // olmasına bağlı; alt sınır bu yüzden `shutdown_returns_within_limit`
        // tarafında pinli.
        let session = spawn_with_command(load_shell(5), Arc::clone(&wake));
        // Yük gerçekten akıyor: ilk uyandırma geldiyse çocuk yazmaya başladı,
        // yani kapanış anında kuyruk dolu olabilir.
        assert!(
            wake.wait_wakes(1, Duration::from_secs(5)) > 0,
            "ölçüm yükü hiç çıktı üretmedi"
        );

        shutdown_within_grace(&session);
    }

    #[test]
    fn a_dropped_shutdown_handle_still_finishes_the_teardown() {
        // Sekme kapanışının yolu (026 R3.6): kapanış başlatılır, tutamak
        // **beklenmeden** düşer. Teardown thread'i işini yine bitirmeli —
        // `SIGHUP` `Pty::drop`'ta, yani o thread'in `join`'den sonraki
        // adımında gidiyor. Çocuk sinyali bir dosyaya yazarak doğruluyor.
        let wake = Arc::new(TestWake::default());
        // Ad yalnız süreç kimliğiyle benzersiz: yol kabuğun komut satırına
        // tırnaksız giriyor, `ThreadId(..)`'nin parantezi sözdizimini bozardı.
        let dir =
            std::env::temp_dir().join(format!("bateri-dropped-handle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("geçici dizin kurulamadı");
        let ready = dir.join("ready");
        let hup = dir.join("hup");
        let session = spawn_session(
            &format!(
                "trap 'echo x > {hup}; exit' HUP; echo x > {ready}; while :; do sleep 0.05; done",
                hup = hup.display(),
                ready = ready.display(),
            ),
            Arc::clone(&wake),
        );
        // Çapa: `trap` kuruldu. Olmadan `SIGHUP` tuzaktan önce gelip çocuğu
        // dosyasız öldürebilir ve sınama sebepsiz kızarırdı.
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.exists() {
            assert!(Instant::now() < deadline, "betik hazır olmadı");
            std::thread::sleep(Duration::from_millis(10));
        }

        let handle = session.begin_shutdown().expect("ilk çağrı tutamak verir");
        assert!(
            session.begin_shutdown().is_none(),
            "ikinci çağrı kapanışı yeniden başlatmamalı"
        );
        drop(handle);

        let deadline = Instant::now() + Duration::from_secs(5);
        while !hup.exists() {
            assert!(
                Instant::now() < deadline,
                "düşen tutamaktan sonra çocuk SIGHUP almadı"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        // Kapanış başladığı için `shutdown()` da artık bir şey beklemiyor.
        assert_eq!(session.shutdown(), Teardown::AlreadyDone);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn title_follows_the_application_and_a_changed_directory() {
        // OSC 7 → başlık dizinin son bileşeni; aynı dizini basan ikinci
        // `precmd` haber doğurmaz; OSC 2 kazanır; başlık yığınından `None`
        // çıkaran `CSI 23 t` `ResetTitle` doğurur ve başlık dizine döner.
        let wake = Arc::new(TestWake::default());
        //
        // Her adım bir `read`'in arkasında: betik sınamanın satır sonunu
        // bekliyor, yani okumalar uykuların süresiyle yarışmıyor
        // (`/code-review`: paralel koşuda 0,5 sn'lik aralık aşılabiliyordu).
        let session = spawn_session(
            "stty -echo; read _; printf '\\033]7;file:///tmp\\007'; \
             printf '\\033]7;file:///tmp\\007'; read _; \
             printf '\\033[22;0t\\033]2;selam\\007'; read _; \
             printf '\\033[23;0t'; sleep 5",
            Arc::clone(&wake),
        );
        // Betik ilk `read`'de bekliyor: bu okuma ilk OSC 7'den önce.
        assert_eq!(session.title(), "bateri", "hiçbir kaynak yokken");

        session.write(b"\n");
        assert!(wake.wait_titles(1, Duration::from_secs(5)) >= 1);
        assert_eq!(session.title(), "tmp");
        assert_eq!(session.working_directory(), Some(PathBuf::from("/tmp")));

        session.write(b"\n");
        assert!(wake.wait_titles(2, Duration::from_secs(5)) >= 2);
        assert_eq!(session.title(), "selam");

        session.write(b"\n");
        assert!(wake.wait_titles(3, Duration::from_secs(5)) >= 3);
        assert_eq!(session.title(), "tmp");
        // Aynı dizini basan ikinci OSC 7 dördüncü bir haber doğurmadı.
        // Betik son adımdan sonra sessiz; haberlerin sayısı üçte durmalı.
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(wake.titles(), 3);

        // Ayar kaydı `Term::set_options` üzerinden başlığı yeniden yolluyor;
        // değişmeyen başlık haber doğurmaz.
        session.set_terminal_options(TerminalOptions {
            scrollback: 100,
            osc52: Osc52::Off,
            cursor: CaretShape::default(),
            blink: CursorBlink::default(),
        });
        assert_eq!(wake.titles(), 3);
    }

    #[test]
    fn a_command_start_is_reported_once_per_transition() {
        // 036 Karar 2: haber `Running`'e **geçişte**; aynı komutta ikinci `C`
        // (iTerm2) geçiş değil. Nesil yalnız koşarken var ve yoklamanın
        // cevabı onu tutmazsa düşüyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; read _; printf '\\033]133;C\\007\\033]133;C\\007'; read _; \
             printf '\\033]133;D;0\\007\\033]133;A\\007\\033]133;C\\007'; sleep 5",
            Arc::clone(&wake),
        );
        assert_eq!(session.running_command(), None, "komut koşmuyor");

        session.write(b"\n");
        assert_eq!(wake.wait_commands(1, Duration::from_secs(5)), 1);
        let first = session.running_command().expect("`C`'den sonra koşuyor");
        assert!(session.set_remote(first, Some(&RemoteTarget::ssh("prod"))));
        assert_eq!(session.title(), "⇄ prod");
        // Dock'suz pencerede uzak oturum giriş satırı sayısını oynatmıyor:
        // bant yok (036 R5.1).
        let (_, cursor) = frame_until(&session, BUDGET, |_, _| true);
        assert_eq!(cursor.input_rows, 1, "dock'suz pencere: {cursor:?}");

        session.write(b"\n");
        assert_eq!(wake.wait_commands(2, Duration::from_secs(5)), 2);
        let second = session.running_command().expect("ikinci komut koşuyor");
        assert_ne!(first, second, "yeni komut yeni nesil");
        // `D` uzak durumu sildi ve başlığa haber verdi.
        assert_eq!(session.title(), "bateri");
        // Bayat nesil: ilk komutun cevabı ikinciye yazılmıyor.
        assert!(!session.set_remote(first, Some(&RemoteTarget::ssh("prod"))));
        assert_eq!(session.title(), "bateri");
        // İkinci `C` haber doğurmadı: sayı ikide duruyor.
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(wake.state.lock().unwrap().commands, 2);
    }

    #[test]
    fn child_exit_fires_when_child_dies() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("exit 3", Arc::clone(&wake));

        assert_eq!(wake.wait_exit(Duration::from_secs(5)), Some(Some(3)));
        drop(session);
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_wake_and_frame() {
        let wake = Arc::new(TestWake::default());
        // Çıktı bilerek kısıtlı: aranan şey yarış, kuyruk şişirmesi değil.
        // Kısıtsız `printf` döngüsü saniyede milyonlarca `Msg::Input`
        // biriktirir ve sınama yarışı değil belleği ölçer.
        let session = Arc::new(spawn_session(
            "while :; do printf '\\033[42mx\\033[0m'; sleep 0.01; done",
            Arc::clone(&wake),
        ));

        let deadline = Instant::now() + Duration::from_secs(2);
        let writers: Vec<_> = (0..4)
            .map(|n| {
                let session = Arc::clone(&session);
                std::thread::spawn(move || {
                    while Instant::now() < deadline {
                        session.write(b" ");
                        // Sütun sayısı oynasın ki reflow da yarışa girsin.
                        let _ = session.resize(40 + n % 2, 10, (9, 18));
                        std::thread::sleep(Duration::from_millis(1));
                    }
                })
            })
            .collect();

        let mut frames = 0u64;
        while Instant::now() < deadline {
            if frame_if_damaged(&session, |_| ()).is_some() {
                frames += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        for writer in writers {
            writer.join().unwrap();
        }
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        // Aranan hata sınıfı tam olarak budur: okuyucu thread paniklerse
        // `shutdown()` yalnız stderr'e yazar ve sınama yeşil kalırdı.
        // Çocuk sonsuz döngüde, thread'in bitmiş olmasının tek açıklaması panik.
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        assert!(
            wake.wait_wakes(1, Duration::ZERO) > 0,
            "hiç uyandırma gelmedi"
        );
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_color_request_and_frame() {
        // Temanın yaprak kilidini iki thread birlikte alıyor: okuyucu renk
        // sorusunu `Term` kilidi **altında**, ana thread `frame()`'de `Term`
        // kilidinden **önce** ve `theme()`'de tek başına. Kilit sırası
        // bozulursa sınama düşmez, **asılı kalır** — belirti koşunun
        // bitmemesi.
        //
        // Yanıtlar arka plandaki `cat`'e akıyor: okunmayan yanıt girdi
        // kuyruğunu doldurur ve sınama kilit değil kuyruk ölçerdi. `</dev/tty`
        // şart: etkileşimsiz kabuk arka plan işine stdin olarak `/dev/null`
        // veriyor, yalın `cat` hemen çıkar ve hiçbir şey emmez (PTY'de
        // denendi).
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_session(
            "stty -icanon -echo; cat </dev/tty >/dev/null & \
             while :; do printf '\\033]11;?\\007\\033]4;1;?\\007\\033[42mx\\033[0m'; sleep 0.01; done",
            Arc::clone(&wake),
        ));

        let deadline = Instant::now() + Duration::from_secs(2);
        let reader = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut seen = 0;
                while Instant::now() < deadline {
                    assert_eq!(session.theme(), THEME);
                    let _ = session.resize(40 + (seen % 2) as u16, 10, (9, 18));
                    seen += 1;
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };

        let mut frames = 0u64;
        while Instant::now() < deadline {
            if frame_if_damaged(&session, |_| ()).is_some() {
                frames += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        reader.join().unwrap();
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_set_theme_and_frame() {
        // Takas yaprak kilidi **yazıyor**, üç okuyanla birlikte: okuyucu
        // thread renk sorusunda (`Term` kilidi altında), ana thread `frame()`'de
        // ve link'in `theme()`'inde. Takas eden thread `set_theme`'in
        // `request_frame`'ini de yarıştırıyor. Kilit sırası bozulursa sınama
        // asılı kalır (bkz. `race_color_request_and_frame`).
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_session(
            "stty -icanon -echo; cat </dev/tty >/dev/null & \
             while :; do printf '\\033]11;?\\007\\033[2;31mx\\033[0m'; sleep 0.01; done",
            Arc::clone(&wake),
        ));

        let deadline = Instant::now() + Duration::from_secs(2);
        let swapper = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut swaps = 0u64;
                while Instant::now() < deadline {
                    let theme = if swaps % 2 == 0 {
                        Theme::BATERI_LIGHT
                    } else {
                        THEME
                    };
                    session.set_theme(theme);
                    swaps += 1;
                    std::thread::sleep(Duration::from_millis(1));
                }
                swaps
            })
        };

        let mut frames = 0u64;
        while Instant::now() < deadline {
            if frame_if_damaged(&session, |_| ()).is_some() {
                frames += 1;
                let theme = session.theme();
                assert!(theme == THEME || theme == Theme::BATERI_LIGHT);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(swapper.join().unwrap() > 0, "hiç takas olmadı");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_set_terminal_options_and_frame() {
        // `set_terminal_options` `Term` kilidini ana thread'den alıyor ve
        // `set_options` o kilit altında `Adapter`'a başlık olayı yolluyor;
        // okuyucu thread aynı kilitte satır basıp geçmişi büyütüyor ve OSC 52
        // ile `Wake::copy_to_clipboard`'u çağırıyor, kaydıran thread ofseti
        // oynatıyor. Tavan iki değer arasında gidip gelirken geçmiş ve ofset
        // kırpılıyor; OSC 52 kipi ayrı bir ritimle açılıp kapanıyor ki iki
        // alanın bütün birleşimleri yarışsın. Başlık ya da pano kolu bir gün
        // kilit alırsa sınama asılı kalır (bkz. `race_color_request_and_frame`).
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_session(
            "while :; do printf 'x\\n\\033]52;c;aGVsbG8=\\007'; sleep 0.005; done",
            Arc::clone(&wake),
        ));

        let deadline = Instant::now() + Duration::from_secs(2);
        let setter = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut sets = 0u64;
                while Instant::now() < deadline {
                    let scrollback = if sets % 2 == 0 { 5 } else { 50 };
                    let osc52 = if sets % 6 < 3 {
                        Osc52::Copy
                    } else {
                        Osc52::Off
                    };
                    session.set_terminal_options(TerminalOptions {
                        scrollback,
                        osc52,
                        cursor: CaretShape::default(),
                        blink: CursorBlink::default(),
                    });
                    let _ = session.scroll_page(1);
                    sets += 1;
                    std::thread::sleep(Duration::from_millis(1));
                }
                sets
            })
        };

        let mut frames = 0u64;
        while Instant::now() < deadline {
            if frame_if_damaged(&session, |_| ()).is_some() {
                frames += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(setter.join().unwrap() > 0, "hiç seçenek değişmedi");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        assert!(session.term.lock().history_size() <= 50);
        // Pano kolu gerçekten yarıştı: kipin yarısı açıkken iki saniyede
        // yüzlerce dizi basılıyor.
        assert!(
            wake.copies().iter().any(|text| text == "hello"),
            "yarış boyunca pano kolu hiç koşmadı"
        );
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_key_gen_and_mirror_stamp() {
        // 025 okuyucu thread ile ana thread arasına bir **atomik** soktu
        // (`Session.key_gen`): ana thread her girdide artırıyor, okuyucu ayna
        // olayında okuyup damga yapıyor. Kabuk her okuduğu satırı aynaya
        // koyuyor, yani `k`. girdinin aynasının tamponu `k`.
        //
        // Değişmez **sıra**: bir aynanın damgası cevap verdiği girdiden asla
        // küçük değil. Artış gönderimden sonra olsaydı `k`. girdinin aynası
        // `k - 1` ile damgalanabilir ve kapı son tuşun aynasını sonsuza kadar
        // cevapsız sayardı. (Tersi — damga girdiden büyük — bilinen sınır:
        // `a_key_answered_by_an_older_mirror_is_a_known_limit`.) Damga ile
        // tampon aynı `DockState`'ten, aynı kilit turunda okunuyor.
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_session(
            "stty -echo; while read l; do \
             b=$(printf %s \"$l\" | base64); \
             printf '\\033]8133;u;0;;%s;;;bWFpbg==\\007' \"$b\"; done",
            Arc::clone(&wake),
        ));

        let deadline = Instant::now() + Duration::from_secs(2);
        let writer = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut sent = 0u64;
                while Instant::now() < deadline {
                    sent += 1;
                    session.write(format!("{sent}\n").as_bytes());
                    std::thread::sleep(Duration::from_millis(1));
                }
                sent
            })
        };

        let mut state = DockState::default();
        let mut seen = 0u64;
        while Instant::now() < deadline {
            session.dock_state(&mut state);
            if let Ok(answered) = state.buffer.parse::<u64>() {
                let now = session.key_gen.load(Ordering::Acquire);
                assert!(
                    answered <= state.answers,
                    "{answered}. girdinin aynası {} ile damgalandı",
                    state.answers
                );
                assert!(state.answers <= now, "damga nesli aştı");
                seen += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(writer.join().unwrap() > 0, "hiç girdi gitmedi");
        assert!(seen > 0, "yarış boyunca hiç ayna gelmedi");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_shell_state_and_frame() {
        // 009 okuyucu thread ile ana thread arasına **beşinci** paylaşılan
        // muteksi soktu (`Session.shell`) ama `race_*` ailesine karşılığını
        // eklememişti (`/audit`, 009 kapısı): dört çiftin her birinin bir
        // sınaması var, bu çiftin yoktu.
        //
        // Yarışan iki yol: okuyucu thread `TappedPty::read` içinde OSC 133
        // tararken `shell` kilidini alıyor (ve aynı turda `Term`'ü de tutuyor,
        // alacritty ilk turdan sonra kilidi tutarak okuyor), ana thread ise
        // `shell_state()` ile yalnız `shell`'i, `frame()` ile **önce** `Term`'ü
        // sonra `shell`'i alıyor.
        //
        // **Neyi tutuyor, neyi tutmuyor** (`/audit`, 010 kapı): bu sınama
        // `shell`'i **tutarken `Term` isteyen** bir düzenlemede asılır — yasak
        // olan tek yön o. `shell`'i `Term`'ün altında alan bir düzenleme ise
        // burada **kalmaz** ve kalmamalı: okuyucu thread zaten öyle alıyor
        // (yukarıdaki parantez) ve o sıra kilitlenemez. Eski yorum tersini
        // iddia ediyordu, yani falsifiye edilebilir ama yanlış bir bekçi
        // iddiasıydı.
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_session(
            // Dört işaret de akıyor: prompt, komut başlangıcı, çalışma ve
            // çıkış kodu. `sleep` yok — tarayıcıyı chunk sınırlarıyla da
            // yorsun diye akış kesintisiz.
            "while :; do printf '\\033]133;A\\007$ \\033]133;B\\007\
             \\033]133;C\\007out\\n\\033]133;D;0\\007'; done",
            Arc::clone(&wake),
        ));

        let deadline = Instant::now() + Duration::from_secs(2);
        let reader = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut seen = 0u64;
                while Instant::now() < deadline {
                    if session.shell_state().is_some() {
                        seen += 1;
                    }
                }
                seen
            })
        };

        let mut frames = 0u64;
        while Instant::now() < deadline {
            if frame_if_damaged(&session, |_| ()).is_some() {
                frames += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(reader.join().unwrap() > 0, "kabuk durumu hiç okunmadı");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_dock_state_and_frame() {
        // 012 aynı yaprak kilide **üçüncü** kaydı (`ShellLog.dock`) ekledi ve
        // onu yazan yol yeni: tarayıcı artık base64 çözüp kilidin altında
        // `clone_from` yapıyor, yani kilit altında geçen süre işaret kolundan
        // uzun. `race_shell_state_and_frame`'in çiftini ayna için tekrarlıyor.
        //
        // Kilit sırası iddiası aynı: `dock_state()` yalnız `shell`'i alıyor,
        // `frame()` önce `Term`'ü sonra `shell`'i. `shell` tutulurken `Term`
        // isteyen bir düzenleme burada asılır.
        let wake = Arc::new(TestWake::default());
        // **Dock'lu oturum şart** ve bu sınama bir dönem onsuz koştu: caret'in
        // devri `SessionOptions::dock`'a bağlandığında (012, "Devri tek
        // kaynağa bağla") `spawn_session`'ın dock'suz varsayılanı bu iddiayı
        // ulaşılamaz kıldı — sınama sessizce değil **kırmızı** kaldı, ama
        // `make test-yaris` koşullu olduğu için fark edilmedi. Bisect: ilk
        // kırmızı commit `3273647`.
        let session = Arc::new(spawn_docked_session(
            // Ayna dizisi ile işaret dizisi **birlikte** akıyor: iki kol tek
            // tarayıcıda ve tek kilitte buluşuyor.
            "while :; do printf '\\033]8133;u;2;;bHM=;;\\007\\033]133;B\\007\
             \\033]8133;e\\007'; done",
            Arc::clone(&wake),
        ));

        let deadline = Instant::now() + Duration::from_secs(2);
        let reader = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut into = DockState::default();
                let mut seen = 0u64;
                while Instant::now() < deadline {
                    session.dock_state(&mut into);
                    if into.status != DockStatus::Idle {
                        seen += 1;
                    }
                }
                seen
            })
        };

        // Kare yolu üretimdeki sırayı izliyor: ızgara, sonra dock. `dock()`
        // **iki** kilidi ardışık alıyor (tema, sonra `shell`) ve ikisi de
        // yaprak; iç içe girseler ya da `Term` tutulurken alınsalar burası
        // asılırdı. `dock_state()`'in tek kilitli hâli okuyucu thread'de
        // yarışmaya devam ediyor, yani iki şekil aynı anda sınanıyor.
        let mut dock = DockState::default();
        let mut context = DockContext::default();
        let mut frames = 0u64;
        let mut docked = 0u64;
        while Instant::now() < deadline {
            // Devrin cevabı üretimdeki gibi `frame()`'den geliyor: `dock()`
            // onu yeniden hesaplasaydı iki kilit turu arasında ayrışabilirdi
            // ve bu sınama tam da o ayrışmanın koşullarını zorluyor.
            if let Some(cursor) = frame_if_damaged(&session, |_| ()) {
                frames += 1;
                if session
                    .dock(
                        DockCols {
                            grid: 80,
                            context: 80,
                        },
                        1,
                        &mut dock,
                        &mut context,
                        cursor.caret_in_dock,
                        &mut Vec::new(),
                        &mut Clusters::default(),
                        |_| (),
                        |_| (),
                    )
                    .caret
                    .is_some()
                {
                    docked += 1;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(reader.join().unwrap() > 0, "ayna hiç okunmadı");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(docked > 0, "kare yolu aynayı hiç canlı görmedi");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_cluster_split_and_resize() {
        // Kümeleme (035) açık kümenin konumunu saklamıyor, her kod noktasında
        // ızgaradan türetiyor ([`crate::handler::ClusterHandler`]). Yarışan
        // yol: bir kümenin iki yarısı iki ayrı `read`'le geliyor (`printf`
        // arasında `sleep`) ve arada ana thread `resize` ile satırı yeniden
        // akıtıyor — baş hücre sütunu değişmiş, spacer'a ya da ızgaranın
        // dışına düşmüş olabilir. Aranan hata sınıfı panik (indeksleme,
        // `bt-core`'da yasak) ve okuyucunun ölmesi.
        let wake = Arc::new(TestWake::default());
        let mut options = test_options(
            sh("while :; do \
                printf 'ab\\360\\237\\221\\215'; sleep 0.002; \
                printf '\\360\\237\\217\\275\\360\\237\\207\\271'; sleep 0.002; \
                printf '\\360\\237\\207\\267\\342\\235\\244'; sleep 0.002; \
                printf '\\357\\270\\217 '; sleep 0.002; \
                done"),
            40,
        );
        options.cluster = true;
        let session = Arc::new(Session::spawn(options, wake).unwrap());

        let deadline = Instant::now() + Duration::from_secs(2);
        let resizer = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut n = 0u16;
                while Instant::now() < deadline {
                    // Tek ve çift genişlikler: kümenin satır sonuna düştüğü
                    // ve son iki sütuna sığdığı hâller sırayla.
                    let _ = session.resize(3 + n % 9, 4 + n % 3, (9, 18));
                    n = n.wrapping_add(1);
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };

        let mut frames = 0u64;
        while Instant::now() < deadline {
            if frame_if_damaged(&session, |_| ()).is_some() {
                frames += 1;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        resizer.join().unwrap();
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_screen_clear_and_frame() {
        // 017 okuyucu thread ile ana thread arasına **altıncı** paylaşılan
        // yuvayı soktu (`Session.screen_clears`) ve bu kez muteks değil
        // atomik: `Term` kilidinin altında okunuyor, yani yaprak kilit
        // olamazdı ([`Session::observe_screen_clear`]).
        //
        // Yarışan iki yol: okuyucu thread sayacı `TappedPty::read` içinde,
        // yani baytları **uygulamadan önce** artırıyor; ana thread onu
        // `frame()` içinde, `Term` kilidi tutulurken ve doluluk sayısıyla
        // aynı okumada tüketiyor.
        //
        // **Bekçinin yakaladığı kusur sessiz:** sayacı kilitten önce okuyan
        // (ya da nesil karşılaştırmasını hiç yapmayan) bir düzenleme
        // `CSI 2 J`'yi kaybeder ve 017 phase-2'nin doldurması Ctrl-L'i geri
        // alır. Yükleme çevrilebilir: ekran **taze temizlenmişse** bayrak
        // kurulu olmak zorundur — betikte ekranı tek satıra indiren tek şey
        // `CSI 2 J`. Yüklem phase-1b'de daraldı ("ekran dolu değilse"ydi);
        // gerekçesi çağrı yerinde.
        //
        // **İki düzenleme üstünde kırmızıya düştüğü ölçüldü** (2026-09-20,
        // üçer koşu): nesil karşılaştırması olmayan düz bayrak, ve sayacı
        // `Term` kilidinden önce okuyup bayrağı kilit bırakıldıktan sonra
        // yazan yerleşim. Üçüncü bir düzenleme — doldurma kuralının taze
        // nesli ezmesi ama nesli **tüketmemesi** — yeşil kalıyor ve kalması
        // doğru: kayıp tek karelik ve o karede `content_rows == rows`, yani
        // `gap` sıfır ve doldurma zaten çizmezdi.
        let wake = Arc::new(TestWake::default());
        // **Dock'lu oturum** ve bu phase-2'nin eklediği tek ayrıntı: bayrağın
        // tek tüketicisi doldurma ([`Session::fill_rows`]) ve onun ilk kapısı
        // `SessionOptions::dock`. Dock'suz koşan bir yarışta `fill` her hâlde
        // sıfır kalır, yani aşağıdaki iddia boşa düşerdi.
        let session = Arc::new(spawn_docked_session(
            // Kısa bir `sleep` **şart** ve ailenin kesintisiz akışından bu
            // yüzden ayrılıyor: temizlemeden sonra ekranın boş kaldığı bir
            // pencere olmazsa kayıp bir bayrak bir sonraki `CSI 2 J` ile
            // anında yerine konur ve yüklem hiçbir şey görmez (ölçüldü —
            // kesintisiz akışta iki düzenleme de yeşil kalıyordu).
            "stty -echo; while :; do seq 1 12; printf '\\033[2J\\033[H'; sleep 0.01; done",
            Arc::clone(&wake),
        ));

        let deadline = Instant::now() + Duration::from_secs(2);
        let reader = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut seen = 0u64;
                while Instant::now() < deadline {
                    // Okuyucu tarafın yarıştırdığı yol: `Term` kilidini
                    // almayan bir sorgu aynı anda koşuyor.
                    if session.shell_state().is_none() {
                        seen += 1;
                    }
                }
                seen
            })
        };

        // **Kare yolu ailenin 1 ms'lik uykusu olmadan koşuyor ve hasar da
        // sormuyor** — ikisi de bilerek. Yarışın penceresi "ana thread `Term`
        // kilidini tutarken okuyucu sayacı artırıyor" ve o pencerenin boyu
        // doğrudan kare yolunun kilidi tutma oranı. Uykulu ve hasar soran
        // döngüde oran binde birkaç kalıyordu: iki bozuk düzenleme de üçte
        // bir koşuda yeşil geçiyordu (ölçüldü). Kilidi hiç bırakmamak da
        // yanlış olurdu — `yield_now` okuyucuya sıra veriyor.
        let mut frames = 0u64;
        let mut armed = 0u64;
        let mut roomy = 0u64;
        while Instant::now() < deadline {
            let (cursor, fill) = fill_now(&session);
            frames += 1;
            let cleared = screen_cleared(&session);
            if cleared {
                armed += 1;
            }
            // `armed > 0`: ilk temizlemeden **önce** ekran doğal olarak boş ve
            // bayrak da doğru olarak kurulu değil; yüklem ancak ilk `CSI 2 J`
            // görüldükten sonra anlamlı.
            //
            // **Ölçüt phase-1b'de daraldı: "ekran dolu değil" değil, "ekran
            // taze temizlenmiş"** (`content_rows == 1`, betiğin `sleep 0.01`
            // penceresi). Eski ölçüt bayrağın düşme koşulunu yazıyordu ve o
            // koşul dock'lu pencerede **erişilemezdi** — bayrak bir kere
            // kurulunca yarış boyunca asılı kalıyordu, yani yüklem bedavaya
            // doğruydu. Düşme ölçütü artık "defter temizlemeden sonra büyüdü
            // mü" ve `seq 1 12` onu her turda meşru olarak düşürüyor
            // (ölçüldü: `content_rows = 9`, `fill = 1`).
            //
            // Daralan yüklem **daha güçlü**: `content_rows == 1` yalnız
            // temizlemenin hemen ardından doğru — defterin büyümesi ekranın
            // dolmasını gerektiriyor, yani o pencerede bayrağın düşmesinin
            // meşru bir yolu yok. Kayıp bir bayrak eskiden bir önceki turdan
            // kalma kurulu bayrakla örtülebiliyordu; artık örtülemiyor.
            if armed > 0 && !session.alt_screen() && cursor.content_rows == 1 {
                roomy += 1;
                assert!(cleared, "temizlenmiş ekran bayraksız kaldı: {cursor:?}");
                // **Bayrağın tek tüketicisi, yarışın altında**: kayıp bir
                // bayrağın belirtisi "Ctrl-L geri alındı"dır ve o belirti tam
                // olarak burada doğar. Sıralama da sınanıyor — ömür
                // doldurmadan **önce** işliyor, yani taze bir nesil aynı
                // karede doldurmayı kapatıyor.
                assert_eq!(cursor.fill, 0, "temizlenen ekran doldu: {cursor:?}");
                assert!(fill.is_empty(), "ikinci sink çağrıldı: {fill:?}");
            }
            std::thread::yield_now();
        }
        assert!(reader.join().unwrap() > 0, "kabuk durumu hiç okunmadı");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(armed > 0, "yarış boyunca bayrak hiç kurulmadı");
        assert!(
            roomy > 0,
            "yarış boyunca ekran hiç taze temizlenmedi: yüklem boşta"
        );
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    // --- Geçmişte arama (033 phase-1) ---

    /// Düz metin sorgusu; `Aa` kapalı (akıllı kip).
    fn plain(text: &str) -> SearchQuery {
        SearchQuery {
            text: text.into(),
            regex: false,
            case_sensitive: false,
        }
    }

    /// Hasar sormadan bu anın arama koşuları — iki liste birlikte.
    fn search_now(session: &Session) -> (Cursor, SearchRuns) {
        let mut runs = SearchRuns::default();
        let cursor = session.frame(
            |_| (),
            |_| (),
            &mut Blocks::default(),
            &mut SelectionRuns::default(),
            &mut runs,
            &mut Clusters::default(),
            ScrollGlide::default(),
            BUDGET,
        );
        (cursor, runs)
    }

    /// Koşuların `(satır, ilk, son)` üçlüleri — bitleri ayrıca sorulmayan
    /// sınamalar için.
    fn spans(runs: &[SearchRun]) -> Vec<(u16, u16, u16)> {
        runs.iter()
            .map(|run| (run.row, run.first, run.last))
            .collect()
    }

    /// Tek satırlık metin basan dock'suz oturum; kare durulmuş.
    fn text_session(text: &str) -> Session {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!("stty -echo; printf '%s\\n' '{text}'; sleep 5"),
            Arc::clone(&wake),
        );
        // Bütün mürekkep, satır sırasıyla: sarılan metin de beklenebilsin.
        let ink: String = text.chars().filter(|c| *c != ' ').collect();
        wait_frame(&session, &wake, |cells| {
            cells.iter().filter_map(|cell| cell.ch).collect::<String>() == ink
        });
        wait_settled(&session);
        session
    }

    #[test]
    fn escape_prefixes_every_meta_character() {
        assert_eq!(search::escape("a.b"), "a\\.b");
        assert_eq!(search::escape("(x)|y"), "\\(x\\)\\|y");
        assert_eq!(search::escape("düz metin"), "düz metin");
    }

    #[test]
    fn a_plain_query_matches_every_meta_character_literally() {
        // **Kaçırma kümesinin bekçisi** (Karar 11): küme `regex-syntax`'ın
        // meta kümesinin kopyası ve eksik bir karakter ya desen hatası
        // (`(`) ya da yanlış eşleşme (`.` her harfi) doğururdu. Satırda her
        // meta karakter bir kez geçiyor; düz sorgu tam onun sütununu bulmalı.
        let line = "a\\.+*?()|[]{}^$#&-~";
        let session = text_session(line);
        for (col, ch) in line.chars().enumerate() {
            let status = session.set_search(&plain(&ch.to_string()));
            assert_eq!(status, SearchStatus::Ready, "{ch:?}");
            let (_, runs) = search_now(&session);
            let col = col as u16;
            assert_eq!(spans(runs.as_slice()), [(0, col, col)], "{ch:?}");
        }
        // Çok karakterli düz metin de tek eşleşme.
        session.set_search(&plain("()|"));
        let (_, runs) = search_now(&session);
        assert_eq!(spans(runs.as_slice()), [(0, 6, 8)]);
        session.shutdown();
    }

    #[test]
    fn the_case_is_smart_unless_forced() {
        let session = text_session("Foo foo FOO");
        session.set_search(&plain("foo"));
        let (_, runs) = search_now(&session);
        assert_eq!(
            spans(runs.as_slice()),
            [(0, 0, 2), (0, 4, 6), (0, 8, 10)],
            "küçük harfli sorgu duyarsız olmalı"
        );
        session.set_search(&plain("Foo"));
        let (_, runs) = search_now(&session);
        assert_eq!(
            spans(runs.as_slice()),
            [(0, 0, 2)],
            "büyük harf duyarlılık açar"
        );
        session.set_search(&SearchQuery {
            case_sensitive: true,
            ..plain("foo")
        });
        let (_, runs) = search_now(&session);
        assert_eq!(spans(runs.as_slice()), [(0, 4, 6)], "Aa her zaman duyarlı");
        // Regex kipinde de aynı önek.
        session.set_search(&SearchQuery {
            text: "f.o".into(),
            regex: true,
            case_sensitive: true,
        });
        let (_, runs) = search_now(&session);
        assert_eq!(spans(runs.as_slice()), [(0, 4, 6)]);
        session.shutdown();
    }

    #[test]
    fn an_invalid_or_empty_query_scans_nothing() {
        let session = text_session("abc (x)");
        let invalid = SearchQuery {
            text: "(".into(),
            regex: true,
            case_sensitive: false,
        };
        assert_eq!(session.set_search(&invalid), SearchStatus::Invalid);
        let (_, runs) = search_now(&session);
        assert!(runs.as_slice().is_empty(), "{runs:?}");
        assert_eq!(session.set_search(&plain("")), SearchStatus::Empty);
        let (_, runs) = search_now(&session);
        assert!(runs.as_slice().is_empty(), "{runs:?}");
        // Aynı metin düz sorguda geçerli.
        assert_eq!(session.set_search(&plain("(")), SearchStatus::Ready);
        let (_, runs) = search_now(&session);
        assert_eq!(spans(runs.as_slice()), [(0, 4, 4)]);
        session.shutdown();
    }

    #[test]
    fn empty_and_blank_matches_are_not_highlighted() {
        // Boş eşleşme (`^`, `z*`) vurgulanmaz (Karar 11) ve yalnız boşluktan
        // oluşan eşleşme de: vurgu içerik yaratmaz.
        let session = text_session("abc def");
        for pattern in ["^", "z*", "\\s+", " "] {
            session.set_search(&SearchQuery {
                text: pattern.into(),
                regex: true,
                case_sensitive: false,
            });
            let (_, runs) = search_now(&session);
            assert!(runs.as_slice().is_empty(), "{pattern:?}: {runs:?}");
            assert!(runs.fill_slice().is_empty(), "{pattern:?}: {runs:?}");
        }
        // Boşluk mürekkepli bir eşleşmenin **içinde** vurgulanıyor.
        session.set_search(&plain("c d"));
        let (_, runs) = search_now(&session);
        assert_eq!(spans(runs.as_slice()), [(0, 2, 4)]);
        session.shutdown();
    }

    #[test]
    fn search_is_empty_while_off_and_survives_across_frames() {
        let session = text_session("needle");
        let (_, runs) = search_now(&session);
        assert!(
            runs.as_slice().is_empty(),
            "arama kapalıyken koşu: {runs:?}"
        );
        session.set_search(&plain("needle"));
        // Desen ödünç alınıp geri konuyor: ikinci kare de buluyor.
        for _ in 0..2 {
            let (_, runs) = search_now(&session);
            let run = runs.as_slice();
            assert_eq!(spans(run), [(0, 0, 5)]);
            assert!(run[0].current && !run[0].continues, "{run:?}");
        }
        session.clear_search();
        let (_, runs) = search_now(&session);
        assert!(
            runs.as_slice().is_empty(),
            "kapandıktan sonra koşu: {runs:?}"
        );
        session.shutdown();
    }

    #[test]
    fn two_matches_on_one_row_are_two_runs_and_the_lowest_is_current() {
        // Bantlı dip ([`gapped_session`]): ızgarada `22`…`26`, bantta
        // `17`…`21`. `2` bantta `20` ve `21`'in başında, ızgarada her satırın
        // başında ve `22`'de iki kez — ayrı eşleşmeler, ayrı koşular.
        let (session, _wake) = gapped_session(true);
        session.set_search(&plain("2"));
        let (cursor, runs) = search_now(&session);
        assert_eq!(cursor.fill, 5, "{cursor:?}");
        assert_eq!(spans(runs.fill_slice()), [(3, 0, 0), (4, 0, 0)], "{runs:?}");
        assert_eq!(
            spans(runs.as_slice()),
            [
                (0, 0, 0),
                (0, 1, 1),
                (1, 0, 0),
                (2, 0, 0),
                (3, 0, 0),
                (4, 0, 0)
            ],
            "{runs:?}"
        );
        // Geçerli: görünürdeki en alttaki, yani `26`'nın `2`'si — tek koşu.
        let current: Vec<_> = runs
            .as_slice()
            .iter()
            .chain(runs.fill_slice())
            .filter(|run| run.current)
            .map(|run| (run.row, run.first))
            .collect();
        assert_eq!(current, [(4, 0)], "{runs:?}");
        // Bantta tek satır: `19` fill-yerel ikinci satırda.
        session.set_search(&plain("19"));
        let (_, runs) = search_now(&session);
        assert_eq!(spans(runs.fill_slice()), [(2, 0, 1)], "{runs:?}");
        assert!(runs.as_slice().is_empty(), "{runs:?}");
        assert!(runs.fill_slice()[0].current, "{runs:?}");
    }

    #[test]
    fn a_scrolled_window_and_its_top_row_are_searched() {
        // Kaydırılmış pencere (ofset 2): ekranda `20`…`29`, kesrin tepe
        // satırı `19` kanalın fill-yerel `0`'ında ([`Cursor::top_row`]).
        let (session, _wake) = history_session("stty -echo; seq 1 30; sleep 5");
        smooth(&session, 2.5, ScrollIntent::Direct);
        session.set_search(&plain("25"));
        let (cursor, runs) = search_now(&session);
        assert_eq!(
            (cursor.display_offset, cursor.top_row),
            (2, 1),
            "{cursor:?}"
        );
        assert_eq!(spans(runs.as_slice()), [(5, 0, 1)], "{runs:?}");
        assert!(runs.fill_slice().is_empty(), "{runs:?}");
        session.set_search(&plain("19"));
        let (_, runs) = search_now(&session);
        assert_eq!(spans(runs.fill_slice()), [(0, 0, 1)], "{runs:?}");
        assert!(runs.as_slice().is_empty(), "{runs:?}");
        // Pencerenin dışındaki satır koşu vermiyor.
        session.set_search(&plain("15"));
        let (_, runs) = search_now(&session);
        assert!(runs.as_slice().is_empty() && runs.fill_slice().is_empty());
    }

    #[test]
    fn a_wrapped_match_is_split_per_row_and_continues() {
        // 40 sütun: 38 `x` + `abcd` → `ab` 0. satırın sonunda, `cd` 1.
        // satırın başında. Tek eşleşme, iki koşu, ikincisi devam.
        let text = format!("{}abcd", "x".repeat(38));
        let session = text_session(&text);
        session.set_search(&plain("abcd"));
        let (_, runs) = search_now(&session);
        assert_eq!(
            runs.as_slice(),
            [
                SearchRun {
                    row: 0,
                    first: 38,
                    last: 39,
                    current: true,
                    continues: false,
                },
                SearchRun {
                    row: 1,
                    first: 0,
                    last: 1,
                    current: true,
                    continues: true,
                },
            ]
        );
        session.shutdown();
    }

    #[test]
    fn a_match_that_starts_above_the_window_is_found() {
        // Sarılmış satırın başı geçmişte, kuyruğu ekranın tepesinde: tarama
        // görünür tepeden başlasaydı `abcd`'yi hiç bulmazdı (`search::scan`'in
        // mantıksal satır genişletmesi). 10 satır: sarılı iki satır + `1`…`8`
        // + imleç = 11, yani sarılı satırın ilk yarısı geçmişte.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            &format!(
                "stty -echo; printf '%s\\n' '{}abcd'; seq 1 8; sleep 5",
                "x".repeat(38)
            ),
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| row_text(cells, 8) == "8");
        wait_settled(&session);
        session.set_search(&plain("abcd"));
        let (cursor, runs) = search_now(&session);
        assert_eq!((cursor.fill, cursor.top_row), (0, 0), "{cursor:?}");
        assert_eq!(
            runs.as_slice(),
            [SearchRun {
                row: 0,
                first: 0,
                last: 1,
                current: true,
                continues: true,
            }]
        );
        session.shutdown();
    }

    #[test]
    fn a_hard_line_break_ends_every_match() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; printf 'ab\\ncd\\n'; sleep 5",
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| row_text(cells, 1) == "cd");
        session.set_search(&SearchQuery {
            text: "b.*c".into(),
            regex: true,
            case_sensitive: false,
        });
        let (_, runs) = search_now(&session);
        assert!(runs.as_slice().is_empty(), "{runs:?}");
        session.shutdown();
    }

    #[test]
    fn the_suppressed_input_line_is_not_searched() {
        // Izgara: `$ cmd1` (0), `out` (1), `$ ls -la` (2, dock'ta; ızgarada
        // bastırılmış). Görünmeyen satır vurgu almaz (Karar 8).
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session(&mirror("bHMgLWxh", 6), Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Live);
        session.set_search(&plain("ls"));
        let (cursor, runs) = search_now(&session);
        assert!(cursor.caret_in_dock, "{cursor:?}");
        assert!(
            runs.as_slice().is_empty(),
            "bastırılan satır arandı: {runs:?}"
        );
        session.set_search(&plain("cmd"));
        let (_, runs) = search_now(&session);
        assert_eq!(spans(runs.as_slice()), [(0, 2, 4)], "{runs:?}");
        session.shutdown();
    }

    #[test]
    fn the_alternate_screen_searches_its_visible_grid() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; printf 'hello\\n\\033[?1049h\\033[Hvim hello'; sleep 5",
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| row_text(cells, 0) == "vimhello");
        session.set_search(&plain("hello"));
        let (cursor, runs) = search_now(&session);
        assert_eq!(cursor.content_rows, cursor.rows, "{cursor:?}");
        assert_eq!(spans(runs.as_slice()), [(0, 4, 8)], "{runs:?}");
        assert!(runs.fill_slice().is_empty(), "{runs:?}");
        session.shutdown();
    }

    // --- Gezinme (033 phase-4) ---

    /// Dizini sonuna kadar sürer (`bt-shell`'in sürücüsünün sınamadaki
    /// eşi) ve son raporu verir; tavan sonsuz bir sürücüye karşı.
    fn count_all(session: &Session) -> SearchReport {
        for _ in 0..10_000 {
            let report = session.search_step().expect("arama açık olmalı");
            if report.complete {
                return report;
            }
        }
        panic!("dizin bitmedi");
    }

    /// Panelin hiçbir şeyi örtmediği pencere.
    const OPEN: SearchCover = SearchCover {
        first_row: i32::MIN,
        from_col: 0,
    };

    /// Geçerli eşleşmenin başladığı mutlak satır ve sütun — yuvadan.
    fn current_at(session: &Session) -> Option<(i32, usize)> {
        let slot = lock(&session.search);
        slot.current
            .as_ref()
            .map(|found| (found.start().line.0, found.start().column.0))
    }

    /// Kaydırma konumu ve bekleyen süzülme: `(ofset, istek)`.
    fn scroll_state(session: &Session) -> (i32, f32) {
        let offset = session.term.lock().grid().display_offset() as i32;
        (offset, session.take_scroll_glide().rows)
    }

    /// `seq 1 30` + 10 satır: ekranda `22`…`30` (satır 0…8), geçmişte
    /// `1`…`21` (`Line(-21)`…`Line(-1)`). `7` üç yerde: `Line(-15)`,
    /// `Line(-5)`, `Line(5)`.
    fn sevens() -> (Session, Arc<TestWake>) {
        let (session, wake) = history_session("stty -echo; seq 1 30; sleep 5");
        assert_eq!(session.set_search(&plain("7")), SearchStatus::Ready);
        (session, wake)
    }

    #[test]
    fn typing_picks_the_lowest_visible_match_and_leaves_the_window() {
        let (session, _wake) = sevens();
        assert_eq!(current_at(&session), Some((5, 1)), "`27`'nin `7`'si");
        let report = session.search_reveal(OPEN, true);
        assert!(report.found, "{report:?}");
        assert_eq!(
            scroll_state(&session),
            (0, 0.0),
            "görünür eşleşmede pencere oynadı"
        );
        assert_eq!(
            count_all(&session),
            SearchReport {
                found: true,
                total: 3,
                ordinal: Some(1),
                complete: true,
            },
            "en alttaki en yeni"
        );
        session.shutdown();
    }

    #[test]
    fn typing_without_a_visible_match_searches_up_from_the_window() {
        let (session, _wake) = history_session("stty -echo; seq 1 30; sleep 5");
        session.set_search(&plain("15"));
        assert_eq!(current_at(&session), Some((-7, 0)));
        let report = session.search_reveal(OPEN, false);
        assert!(report.found, "{report:?}");
        // Ortalanıyor: `Line(-7)` ekranın 5. satırına, ofset 12.
        assert_eq!(scroll_state(&session), (12, 0.0));
        session.shutdown();
    }

    #[test]
    fn next_goes_up_to_older_matches_and_wraps() {
        // `smooth == false`: pencere anında, satırı ortada.
        // Sıra numarası da komşuya taşınıyor (phase-5): önce sayım bitsin.
        let (session, _wake) = sevens();
        assert_eq!(count_all(&session).ordinal, Some(1));
        let report = session.search_next(SearchDirection::Older, OPEN, false);
        assert_eq!(current_at(&session), Some((-5, 1)), "`17`");
        assert_eq!(scroll_state(&session), (10, 0.0));
        assert_eq!((report.ordinal, report.total), (Some(2), 3), "{report:?}");
        let report = session.search_next(SearchDirection::Older, OPEN, false);
        assert_eq!(current_at(&session), Some((-15, 0)), "`7`");
        assert_eq!(scroll_state(&session).0, 20);
        assert_eq!(report.ordinal, Some(3), "{report:?}");
        // Tepede sarıyor: en yenisine, dibe.
        let report = session.search_next(SearchDirection::Older, OPEN, false);
        assert_eq!(current_at(&session), Some((5, 1)), "`27`");
        assert_eq!(scroll_state(&session).0, 0);
        assert_eq!(report.ordinal, Some(1), "{report:?}");
        // Aşağı yön de dipte sarıyor: en eskisine.
        let report = session.search_next(SearchDirection::Newer, OPEN, false);
        assert_eq!(current_at(&session), Some((-15, 0)), "`7`");
        assert_eq!(report.ordinal, Some(3), "{report:?}");
        let report = session.search_next(SearchDirection::Newer, OPEN, false);
        assert_eq!(current_at(&session), Some((-5, 1)), "`17`");
        assert_eq!(report.ordinal, Some(2), "{report:?}");
        assert!(report.complete, "gezinme sayımı bozmamalı: {report:?}");
        session.shutdown();
    }

    #[test]
    fn a_near_match_glides_and_a_far_one_lands_then_glides_the_last_screen() {
        let (session, _wake) = sevens();
        // Bir ekran içinde (10 satır): pencere yerinde, istek 10 satır.
        session.search_next(SearchDirection::Older, OPEN, true);
        assert_eq!(scroll_state(&session), (0, 10.0));
        // Süzülme teslim edilmeden bir sonraki: hedef 20 satır uzakta. Eski
        // istek düşüyor (nesil), pencere bir ekran yakına konuyor ve son
        // ekran süzülüyor.
        session.search_next(SearchDirection::Older, OPEN, true);
        assert_eq!(scroll_state(&session), (10, 10.0));
        session.shutdown();
    }

    #[test]
    fn a_visible_match_stops_the_glide_toward_the_previous_one() {
        // ⏎ süzülmeyi başlatıyor, teslim edilmeden ⇧⏎ görünür eşleşmeye
        // dönüyor: bekleyen istek düşmeli ve nesil artmalı, yoksa süzülme
        // pencereyi eşleşmeden uzağa taşırdı.
        let (session, _wake) = sevens();
        let before = session.take_scroll_glide().generation;
        session.search_next(SearchDirection::Older, OPEN, true);
        session.search_next(SearchDirection::Newer, OPEN, true);
        assert_eq!(current_at(&session), Some((5, 1)), "`27`");
        let glide = session.take_scroll_glide();
        assert_eq!(glide.rows, 0.0, "{glide:?}");
        assert_eq!(glide.generation, before.wrapping_add(2), "{glide:?}");
        session.shutdown();
    }

    #[test]
    fn a_match_under_the_panel_is_revealed_and_one_beside_it_is_not() {
        // `22` 0. satırda, 0…1. sütunlarda.
        let (session, _wake) = history_session("stty -echo; seq 1 30; sleep 5");
        session.set_search(&plain("22"));
        assert_eq!(current_at(&session), Some((0, 0)));
        let beside = SearchCover {
            first_row: 1,
            from_col: 10,
        };
        session.search_reveal(beside, true);
        assert_eq!(
            scroll_state(&session),
            (0, 0.0),
            "panelin solundaki eşleşme görünür"
        );
        let over = SearchCover {
            first_row: 1,
            from_col: 1,
        };
        session.search_reveal(over, true);
        assert_eq!(
            scroll_state(&session),
            (0, 5.0),
            "panelin altındaki eşleşme ortaya"
        );
        session.shutdown();
    }

    #[test]
    fn a_match_in_the_fill_band_is_visible() {
        // Bantlı dip: `19` bantta ([`gapped_session`]).
        let (session, _wake) = gapped_session(true);
        search_now(&session);
        session.set_search(&plain("19"));
        let report = session.search_reveal(OPEN, true);
        assert!(report.found, "{report:?}");
        assert_eq!(
            scroll_state(&session),
            (0, 0.0),
            "bant satırı görünür sayılmadı"
        );
        session.shutdown();
    }

    #[test]
    fn navigation_skips_the_suppressed_line_and_blank_matches() {
        // `ls` yalnız bastırılan satırda: gezinmenin hedefi değil.
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session(&mirror("bHMgLWxh", 6), Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Live);
        search_now(&session);
        session.set_search(&plain("ls"));
        assert_eq!(current_at(&session), None);
        let report = session.search_next(SearchDirection::Older, OPEN, true);
        assert!(!report.found, "{report:?}");
        // `1` hem `cmd1`'de hem bastırılan satırda değil: tek hedef.
        session.set_search(&plain("cmd"));
        let first = current_at(&session);
        assert_eq!(first, Some((0, 2)));
        session.search_next(SearchDirection::Older, OPEN, true);
        assert_eq!(current_at(&session), first, "tek eşleşmede sarıp aynı yere");
        session.shutdown();
        // Mürekkepsiz eşleşme de atlanıyor.
        let session = text_session("abc def");
        session.set_search(&SearchQuery {
            text: "\\s+".into(),
            regex: true,
            case_sensitive: false,
        });
        assert_eq!(current_at(&session), None);
        session.shutdown();
    }

    #[test]
    fn the_current_match_is_the_one_marked_in_the_frame() {
        let (session, _wake) = sevens();
        session.search_next(SearchDirection::Older, OPEN, false);
        let (_, runs) = search_now(&session);
        let current: Vec<_> = runs
            .as_slice()
            .iter()
            .filter(|run| run.current)
            .map(|run| (run.row, run.first))
            .collect();
        // Ofset 10: `17` (`Line(-5)`) ekranın 5. satırında.
        assert_eq!(current, [(5, 1)], "{runs:?}");
        session.shutdown();
    }

    #[test]
    fn escape_turns_the_current_match_into_the_selection() {
        let (session, _wake) = sevens();
        session.search_next(SearchDirection::Older, OPEN, true);
        let before = scroll_state(&session).0;
        assert!(session.select_search_match());
        assert_eq!(session.selection_text().as_deref(), Some("7"));
        assert_eq!(scroll_state(&session).0, before, "seçim pencereyi oynattı");
        session.clear_search();
        assert!(!session.select_search_match(), "kapalı aramada seçim");
        session.shutdown();
    }

    #[test]
    fn the_alternate_screen_does_not_scroll_to_a_match() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; printf 'hello\\n\\033[?1049h\\033[Hvim hello'; sleep 5",
            Arc::clone(&wake),
        );
        wait_frame(&session, &wake, |cells| row_text(cells, 0) == "vimhello");
        session.set_search(&plain("hello"));
        let report = session.search_next(SearchDirection::Older, OPEN, true);
        assert!(report.found, "{report:?}");
        assert_eq!(scroll_state(&session), (0, 0.0));
        // Sayım alternatif ekranın görünür ızgarası (Karar 8): birincil
        // ekranın `hello`'su sayılmıyor.
        let report = count_all(&session);
        assert_eq!((report.total, report.ordinal), (1, Some(1)), "{report:?}");
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_set_search_and_frame() {
        // Desen kare boyunca **ödünçte** ([`SearchSlot`]) ve sorgu ana
        // thread'den her an değişebiliyor. Kilit sırası bozulursa sınama
        // asılı kalır; geri koyma kuralı bozulursa son sorgu kaybolur ya da
        // eski desen yenisinin yerine geçer — sondaki iki kare onu soruyor.
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_session(
            "stty -echo; while :; do printf 'alpha beta\\n'; sleep 0.01; done",
            Arc::clone(&wake),
        ));
        let deadline = Instant::now() + Duration::from_secs(2);
        let writer = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut sets = 0u64;
                while Instant::now() < deadline {
                    match sets % 3 {
                        0 => {
                            session.set_search(&plain("alpha"));
                        }
                        1 => {
                            session.set_search(&plain("beta"));
                        }
                        _ => session.clear_search(),
                    }
                    sets += 1;
                    std::thread::sleep(Duration::from_millis(1));
                }
                sets
            })
        };
        let mut frames = 0u64;
        while Instant::now() < deadline {
            search_now(&session);
            frames += 1;
        }
        assert!(writer.join().unwrap() > 0, "hiç sorgu yazılmadı");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        session.set_search(&plain("beta"));
        for _ in 0..2 {
            let (_, runs) = search_now(&session);
            assert!(
                runs.as_slice().iter().all(|run| run.first == 6),
                "eski desen yenisinin yerine geçti: {runs:?}"
            );
            assert!(!runs.as_slice().is_empty(), "son sorgu kayboldu");
        }
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_search_next_and_frame() {
        // Gezinme deseni kareyle **aynı yuvadan** ödünç alıyor ve ikinci bir
        // `Term` kilidi sahibi: kilit sırası bozulursa sınama asılı kalır,
        // geri koyma kuralı bozulursa desen kaybolur ve son gezinme hiçbir
        // şey bulamaz.
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_session(
            "stty -echo; while :; do printf 'alpha beta\\n'; sleep 0.01; done",
            Arc::clone(&wake),
        ));
        session.set_search(&plain("alpha"));
        let deadline = Instant::now() + Duration::from_secs(2);
        let walker = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut steps = 0u64;
                while Instant::now() < deadline {
                    let direction = if steps % 2 == 0 {
                        SearchDirection::Older
                    } else {
                        SearchDirection::Newer
                    };
                    session.search_next(direction, OPEN, steps % 3 == 0);
                    if steps % 5 == 0 {
                        session.set_search(&plain("alpha"));
                    }
                    steps += 1;
                    std::thread::sleep(Duration::from_millis(1));
                }
                steps
            })
        };
        let mut frames = 0u64;
        while Instant::now() < deadline {
            search_now(&session);
            session.take_scroll_glide();
            frames += 1;
        }
        assert!(walker.join().unwrap() > 0, "hiç gezinilmedi");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        let report = session.search_next(SearchDirection::Older, OPEN, true);
        assert!(report.found, "desen yarışta kayboldu: {report:?}");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    // --- Bütün defterin sayımı (033 phase-5) ---

    /// `scrollback`'i çağırandan gelen dock'suz oturum: doymuş defterin
    /// sınamaları.
    fn spawn_with_scrollback(script: &str, scrollback: usize, wake: Arc<TestWake>) -> Session {
        let mut options = test_options(sh(script), 40);
        options.terminal.scrollback = scrollback;
        Session::spawn(options, wake).unwrap()
    }

    /// Defterin mutlak `line` satırının metni, sondaki boşluklar kırpılmış.
    fn line_text(session: &Session, line: i32) -> String {
        let term = session.term.lock();
        term.grid()[Line(line)]
            .into_iter()
            .map(|cell| cell.c)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    /// Geçerli eşleşmenin satırının metni — "doğru içerikte mi" sorusu.
    fn current_text(session: &Session) -> Option<String> {
        current_at(session).map(|(line, _)| line_text(session, line))
    }

    /// Dizini tek başına, `chunk` satırlık parçalarla sonuna kadar sürer ve
    /// sayısını verir (sürücüsüz; parça dikişinin sınaması).
    fn index_with(session: &Session, query: &SearchQuery, chunk: i32) -> usize {
        let (_, pattern) = search::compile(query);
        let mut regex = pattern.expect("geçerli desen");
        let mut index = search::SearchIndex::default();
        index.restart(None);
        let term = session.term.lock();
        while index.next.is_some() {
            search::index_chunk(
                &term,
                &mut regex,
                &mut index,
                None,
                &search::Tracking::default(),
                0..=0,
                chunk,
            );
        }
        index.total
    }

    #[test]
    fn chunk_seams_neither_lose_nor_double_count_a_wrapped_match() {
        // 25 satır × 100 karakter, 40 sütunda üçer satıra sarılıyor ve her
        // satırda iki `needle` tam sarma sınırını (40 ve 80) geçiyor: dikiş
        // hangi satıra düşerse düşsün her eşleşme bir kez sayılmalı.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; for i in $(seq 1 25); do printf '%036dneedle%034dneedle%018d\\n' 0 0 0; done; sleep 5",
            Arc::clone(&wake),
        );
        wait_until(
            "sarılmış satırlar gelmedi",
            Duration::from_secs(5),
            || session.term.lock().history_size() >= 66,
        );
        wait_settled(&session);
        for chunk in [1, 2, 3, 4, 5, 7, 11, search::CHUNK_LINES] {
            assert_eq!(
                index_with(&session, &plain("needle"), chunk),
                50,
                "parça {chunk}"
            );
        }
        // Sürücünün yolu da aynı sayıyı veriyor.
        session.set_search(&plain("needle"));
        let report = count_all(&session);
        assert_eq!((report.total, report.ordinal), (50, Some(1)), "{report:?}");
        session.shutdown();
    }

    #[test]
    fn the_whole_ledger_is_counted_newest_first() {
        // `seq 1 60`: `1` her sayının her birinde (`11` iki kez).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("stty -echo; seq 1 60; sleep 5", Arc::clone(&wake));
        wait_until("defter dolmadı", Duration::from_secs(5), || {
            line_text(&session, 8) == "60"
        });
        let expected = (1..=60)
            .map(|n: u32| n.to_string().matches('1').count())
            .sum::<usize>();
        session.set_search(&plain("1"));
        let report = count_all(&session);
        assert_eq!(report.total, expected, "{report:?}");
        // Geçerli eşleşme en alttaki görünür `1` (`51`), sırası 1.
        assert_eq!(current_text(&session).as_deref(), Some("51"));
        assert_eq!(report.ordinal, Some(1), "{report:?}");
        // En eskiye (sarma) gidince sıra toplam.
        let report = session.search_next(SearchDirection::Newer, OPEN, false);
        assert_eq!(current_text(&session).as_deref(), Some("1"));
        assert_eq!(report.ordinal, Some(expected), "{report:?}");
        session.shutdown();
    }

    #[test]
    fn a_new_query_restarts_the_count_and_a_step_asks_for_no_frame() {
        let (session, wake) = sevens();
        let before = wakes(&wake);
        let report = count_all(&session);
        assert_eq!(report.total, 3, "{report:?}");
        assert_eq!(wakes(&wake), before, "parça kare istedi");
        // Yeni sorgu dizini baştan kuruyor: eski neslin sayısı taşınmıyor.
        session.set_search(&plain("2"));
        let report = session.search_step().expect("arama açık");
        // `2`, `12`, `20`…`29` (`22` iki kez): 13.
        assert_eq!(report.total, 13, "{report:?}");
        assert!(report.complete, "{report:?}");
        session.clear_search();
        assert_eq!(session.search_step(), None, "kapalı arama saymaz");
        session.shutdown();
    }

    #[test]
    fn the_index_and_the_highlight_agree_on_one_screen() {
        // Bastırılan satır (`ls -la`) ve mürekkepsiz eşleşme ikisinde de
        // yok: vurgunun eşleşme sayısı (satır başına bölünmemiş) dizinin
        // sayısına eşit.
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session(&mirror("bHMgLWxh", 6), Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Live);
        session.set_search(&SearchQuery {
            text: "[a-z]+| +".into(),
            regex: true,
            case_sensitive: false,
        });
        let (cursor, runs) = search_now(&session);
        assert!(cursor.caret_in_dock, "{cursor:?}");
        let highlighted = runs
            .as_slice()
            .iter()
            .chain(runs.fill_slice())
            .filter(|run| !run.continues)
            .count();
        let report = count_all(&session);
        assert_eq!(report.total, highlighted, "{runs:?} {report:?}");
        // `cmd` ve `out`: bastırılan `ls`/`la` ve boşluklar yok.
        assert_eq!(report.total, 2, "{report:?}");
        session.shutdown();
    }

    #[test]
    fn a_ledger_change_is_announced_once_until_the_index_takes_it() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; seq 1 30; while read x; do echo \"$x\"; done",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);
        session.set_search(&plain("7"));
        count_all(&session);
        let searches = || wake.state.lock().unwrap().searches;
        let base = searches();
        session.write(b"a\n");
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "a"
        });
        session.write(b"b\n");
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "b"
        });
        assert_eq!(
            searches(),
            base + 1,
            "tüketilmemiş haber ikincisini doğurdu"
        );
        let report = count_all(&session);
        assert!(report.complete, "{report:?}");
        session.write(b"c\n");
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "c"
        });
        assert_eq!(
            searches(),
            base + 2,
            "tüketilen haberden sonra yenisi gelmedi"
        );
        // Arama kapalıyken haber yok.
        session.clear_search();
        session.write(b"d\n");
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "d"
        });
        assert_eq!(searches(), base + 2, "kapalı aramaya haber gitti");
        session.shutdown();
    }

    #[test]
    fn a_scrolled_window_keeps_the_current_match_on_its_content() {
        // Defter doymamış: `history_size` farkı.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; seq 1 30; sleep 1; seq 31 35; sleep 5",
            Arc::clone(&wake),
        );
        wait_until("defter dolmadı", Duration::from_secs(5), || {
            line_text(&session, 8) == "30"
        });
        session.scroll_page(1);
        session.set_search(&plain("15"));
        assert_eq!(current_at(&session), Some((-7, 0)));
        wait_until("ikinci çıktı gelmedi", Duration::from_secs(5), || {
            session.term.lock().history_size() == 26
        });
        let (_, runs) = search_now(&session);
        assert_eq!(current_at(&session), Some((-12, 0)), "içeriğinden kaydı");
        assert_eq!(current_text(&session).as_deref(), Some("15"));
        assert!(
            runs.as_slice().iter().any(|run| run.current),
            "geçerli vurgu kayboldu: {runs:?}"
        );
        session.shutdown();

        // Defter doymuş, pencere kaydırılmış: `display_offset` farkı, eksi
        // kullanıcının kendi kaydırması.
        let wake = Arc::new(TestWake::default());
        let session = spawn_with_scrollback(
            "stty -echo; seq 1 40; sleep 1; seq 41 43; sleep 5",
            20,
            Arc::clone(&wake),
        );
        wait_until("defter dolmadı", Duration::from_secs(5), || {
            line_text(&session, 8) == "40"
        });
        session.scroll_page(1);
        session.set_search(&plain("25"));
        assert_eq!(current_text(&session).as_deref(), Some("25"));
        // Kullanıcı bir satır daha kaydırıyor: çıktı sayılmamalı.
        session.scroll_page(-1);
        session.scroll_page(1);
        wait_until("ikinci çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "43"
        });
        search_now(&session);
        assert_eq!(
            current_text(&session).as_deref(),
            Some("25"),
            "doymuş defterde kaydı"
        );
        session.shutdown();
    }

    #[test]
    fn a_saturated_ledger_never_shows_the_current_match_on_a_wrong_row() {
        // Doymuş defterin dibinde kayma bilinemiyor: geçerli eşleşme yanlış
        // satıra geçmek yerine kayboluyor, sayımın sonunda en yakına dönüyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_with_scrollback(
            "stty -echo; seq 1 50; read x; seq 51 53; sleep 5",
            20,
            Arc::clone(&wake),
        );
        wait_until("defter dolmadı", Duration::from_secs(5), || {
            line_text(&session, 8) == "50"
        });
        session.set_search(&plain("45"));
        assert_eq!(current_text(&session).as_deref(), Some("45"));
        session.write(b"\n");
        wait_until("ikinci çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "53"
        });
        let (_, runs) = search_now(&session);
        let shown = current_text(&session);
        assert!(
            shown.is_none() || shown.as_deref() == Some("45"),
            "geçerli eşleşme yanlış satırda: {shown:?}"
        );
        if shown.is_none() {
            assert!(!runs.as_slice().iter().any(|run| run.current), "{runs:?}");
        }
        let before = wakes(&wake);
        let report = count_all(&session);
        assert!(report.found, "{report:?}");
        assert_eq!(current_text(&session).as_deref(), Some("45"));
        assert_eq!(report.ordinal, Some(1), "{report:?}");
        if shown.is_none() {
            assert!(wakes(&wake) > before, "yeniden seçim kare istemedi");
        }
        session.shutdown();
    }

    #[test]
    fn matches_that_arrive_later_are_found_and_navigation_orders_them() {
        // Sorgu yazılırken eşleşme yok; çıktıyla geliyor. Vurgu ve sayım onu
        // görüyor, etiket "No matches" dememeli; ⏎ onu geçerli yapınca sıra
        // bitmiş dizinde bir geçiş daha isteyip bulunuyor (`/code-review`).
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; seq 1 5; read x; echo zebra; sleep 5",
            Arc::clone(&wake),
        );
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 4) == "5"
        });
        session.set_search(&plain("zebra"));
        assert!(!count_all(&session).found, "henüz eşleşme yok");
        session.write(b"\n");
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 5) == "zebra"
        });
        let report = count_all(&session);
        assert_eq!(current_at(&session), None);
        assert!(report.found, "sayılan eşleşme 'yok' denildi: {report:?}");
        assert_eq!((report.total, report.ordinal), (1, None), "{report:?}");
        let report = session.search_next(SearchDirection::Older, OPEN, false);
        assert_eq!(current_text(&session).as_deref(), Some("zebra"));
        assert!(
            !report.complete,
            "sırasız eşleşme bitmiş sayıldı: {report:?}"
        );
        let report = count_all(&session);
        assert_eq!(report.ordinal, Some(1), "{report:?}");
        session.shutdown();
    }

    #[test]
    fn ledger_shift_uses_only_exact_sources() {
        use search::{LedgerMark, Shift, ledger_shift};
        let mark = |history, offset, user, epoch| LedgerMark {
            history,
            offset,
            user,
            epoch,
            wipes: 0,
            columns: 40,
            lines: 10,
            alt: false,
        };
        // Doymamış: geçmişin büyümesi, pencere nerede olursa olsun.
        assert_eq!(
            ledger_shift(mark(10, 0, 0, 1), mark(13, 0, 0, 2), 100),
            Shift::By(3)
        );
        assert_eq!(
            ledger_shift(mark(10, 4, 0, 1), mark(13, 7, 0, 2), 100),
            Shift::By(3)
        );
        // Çıktı yok: kullanıcının kaydırması kayma değil.
        assert_eq!(
            ledger_shift(mark(20, 4, 0, 1), mark(20, 9, 5, 1), 20),
            Shift::Still
        );
        // Doymuş ve kaydırılmış: ofset farkı eksi kullanıcının payı.
        assert_eq!(
            ledger_shift(mark(20, 4, 0, 1), mark(20, 9, 2, 2), 20),
            Shift::By(3)
        );
        // Doymuş dip, tavandaki ofset, boyut ve ekran değişimi: kayıp.
        assert_eq!(
            ledger_shift(mark(20, 0, 0, 1), mark(20, 0, 0, 2), 20),
            Shift::Lost
        );
        assert_eq!(
            ledger_shift(mark(20, 4, 0, 1), mark(20, 20, 0, 2), 20),
            Shift::Lost
        );
        let mut wide = mark(10, 0, 0, 1);
        wide.columns = 80;
        assert_eq!(ledger_shift(mark(10, 0, 0, 1), wide, 100), Shift::Lost);
        let mut alt = mark(0, 0, 0, 2);
        alt.alt = true;
        assert_eq!(ledger_shift(mark(10, 0, 0, 1), alt, 100), Shift::Lost);
        // Silinen geçmiş.
        assert_eq!(
            ledger_shift(mark(10, 0, 0, 1), mark(0, 0, 0, 2), 100),
            Shift::Lost
        );
        // Terminal tarafı temizlik (034): geçmiş 0 → 0 kalsa da, nesil
        // oynamasa da kayıp — `history` farkı onu `Still` sanardı.
        let mut wiped = mark(0, 0, 0, 1);
        wiped.wipes = 1;
        assert_eq!(ledger_shift(mark(0, 0, 0, 1), wiped, 100), Shift::Lost);
        let mut saturated = mark(20, 4, 0, 1);
        saturated.wipes = 1;
        assert_eq!(ledger_shift(mark(20, 4, 0, 1), saturated, 20), Shift::Lost);
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_search_step_and_frame() {
        // Dizin ikinci bir `Term` kilidi sahibi ve kendi desen kopyasını
        // taşıyor; çıktı akarken haber, sorgu değişimi ve kare aynı yuvaya
        // yazıyor. Kilit sırası bozulursa sınama asılı kalır; nesil kuralı
        // bozulursa eski sorgunun sayımı yenisine karışır.
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_session(
            "stty -echo; while :; do printf 'alpha beta\\n'; sleep 0.01; done",
            Arc::clone(&wake),
        ));
        session.set_search(&plain("alpha"));
        let deadline = Instant::now() + Duration::from_secs(2);
        let driver = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut steps = 0u64;
                while Instant::now() < deadline {
                    session.search_step();
                    if steps % 50 == 0 {
                        session.set_search(&plain(if steps % 100 == 0 { "beta" } else { "alpha" }));
                    }
                    if steps % 7 == 0 {
                        session.search_next(SearchDirection::Older, OPEN, false);
                    }
                    steps += 1;
                }
                steps
            })
        };
        let mut frames = 0u64;
        while Instant::now() < deadline {
            search_now(&session);
            frames += 1;
        }
        assert!(driver.join().unwrap() > 0, "hiç sayılmadı");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(session.search_step().is_some(), "dizin yarışta kayboldu");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }

    // --- Ekranı temizle (034 phase-1) ---

    /// Kabuğun gerçekten bastığı çıpalı prompt: bağlantı `preexec`'e kadar
    /// **açık** ([`anchored_prompt`] onu `$ `'dan sonra kapatıyor), yani
    /// arkasından yazılan giriş de kimliği taşıyor — sarılan ve çok satırlı
    /// girişin sınamalarının öncülü.
    fn open_prompt(id: u32) -> String {
        format!(
            "\\033]133;A;bt_block={id}\\007\
             \\033]8;;bateri://block/{id}\\007$ \\033]133;B\\007"
        )
    }

    /// Ekranın bütün satırları, sondaki boşluklar kırpılmış — ⌥⌘K'nin "bayt
    /// bayt aynı"sının ve ⌘K'nin "yalnız korunan satırlar"ının okuması.
    fn screen_lines(session: &Session) -> Vec<String> {
        (0..10).map(|line| line_text(session, line)).collect()
    }

    /// İmlecin ekran konumu, `Term`'den doğrudan.
    fn cursor_point(session: &Session) -> (i32, usize) {
        let term = session.term.lock();
        let point = term.grid().cursor.point;
        (point.line.0, point.column.0)
    }

    fn history_size(session: &Session) -> usize {
        session.term.lock().history_size()
    }

    #[test]
    fn clear_to_start_leaves_only_the_prompt_row_at_the_top() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!("stty -echo; seq 1 30; printf '{}'; sleep 5", open_prompt(1)),
            Arc::clone(&wake),
        );
        wait_until("prompt gelmedi", Duration::from_secs(5), || {
            line_text(&session, 9) == "$"
        });
        // Kayma sayısının öncülü: önceki karenin tepe satırı ölçülmüş olsun.
        cursor_now(&session);
        assert!(history_size(&session) > 0, "sahne geçmişsiz kuruldu");
        let (_, col) = cursor_point(&session);

        assert!(session.clear_to_start());
        assert_eq!(history_size(&session), 0, "geçmiş silinmedi");
        assert_eq!(cursor_point(&session), (0, col), "imleç prompt'la gitmedi");
        let mut expected = vec![String::new(); 10];
        expected[0] = "$".into();
        assert_eq!(screen_lines(&session), expected);

        let cursor = cursor_now(&session);
        assert_eq!(cursor.fill, 0, "{cursor:?}");
        assert_eq!(cursor.scrolled, 0, "{cursor:?}");
        assert!(screen_cleared(&session), "temizlik `2J` neslini artırmadı");
        let cursor = cursor_now(&session);
        assert_eq!(cursor.fill, 0, "damgalı karede bant açıldı: {cursor:?}");
        session.shutdown();
    }

    #[test]
    fn clear_to_start_keeps_every_row_of_a_wrapped_input() {
        // Bekçi: bağlantı açık olduğu için girişin iki satırı da kimliği
        // taşıyor; imlece en yakın çıpalı satır (`anchor_row_at_or_above`)
        // girişin **alt** satırı olurdu ve üstü giderdi.
        let wake = Arc::new(TestWake::default());
        let typed = "a".repeat(60);
        let session = spawn_docked_session(
            &format!(
                "stty -echo; seq 1 30; printf '{}{typed}'; sleep 5",
                open_prompt(1)
            ),
            Arc::clone(&wake),
        );
        wait_until("giriş sarılmadı", Duration::from_secs(5), || {
            line_text(&session, 9) == "a".repeat(22)
        });

        assert!(session.clear_to_start());
        assert_eq!(history_size(&session), 0);
        let lines = screen_lines(&session);
        assert_eq!(lines[0], format!("$ {}", "a".repeat(38)), "{lines:?}");
        assert_eq!(lines[1], "a".repeat(22), "{lines:?}");
        assert!(lines[2..].iter().all(String::is_empty), "{lines:?}");
        assert_eq!(cursor_point(&session), (1, 22));
        session.shutdown();
    }

    #[test]
    fn clear_to_start_keeps_the_prompt_above_an_empty_input_row() {
        // Çok satırlı girişin henüz yazılmamış satırı (`Esc-Enter`): imlecin
        // satırı çıpasız ama kabuk girdi safhasında, yani blok o kimliğin
        // çıpasından yürünüyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "stty -echo; seq 1 30; printf '{}echo\\r\\n'; sleep 5",
                open_prompt(1)
            ),
            Arc::clone(&wake),
        );
        wait_until("giriş gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "$ echo" && cursor_point(&session) == (9, 0)
        });

        assert!(session.clear_to_start());
        let lines = screen_lines(&session);
        assert_eq!(lines[0], "$ echo", "{lines:?}");
        assert!(lines[1..].iter().all(String::is_empty), "{lines:?}");
        assert_eq!(cursor_point(&session), (1, 0));
        session.shutdown();
    }

    #[test]
    fn clear_to_start_keeps_a_multiline_input_across_an_empty_row() {
        // `/code-review` bulgusu (034 phase-1): `echo 1`, iki kez
        // `Esc-Enter`, `echo 2`. Ortadaki boş satır hücre yazmıyor, yani
        // bağlantı taşımıyor; bitişik satırlarda yürüyen bir kural orada
        // durur ve prompt ile `echo 1`'i silerdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "stty -echo; seq 1 30; printf '{}echo 1\\r\\n\\033]8;;\\007\\r\\n\
                 \\033]8;;bateri://block/1\\007echo 2'; sleep 5",
                open_prompt(1)
            ),
            Arc::clone(&wake),
        );
        wait_until("giriş gelmedi", Duration::from_secs(5), || {
            line_text(&session, 9) == "echo 2"
        });
        assert_eq!(line_text(&session, 7), "$ echo 1");
        assert_eq!(line_text(&session, 8), "");

        assert!(session.clear_to_start());
        let lines = screen_lines(&session);
        assert_eq!(lines[0], "$ echo 1", "{lines:?}");
        assert_eq!(lines[1], "", "{lines:?}");
        assert_eq!(lines[2], "echo 2", "{lines:?}");
        assert!(lines[3..].iter().all(String::is_empty), "{lines:?}");
        assert_eq!(cursor_point(&session), (2, 6));
        session.shutdown();
    }

    #[test]
    fn clear_to_start_under_a_running_command_keeps_the_cursor_row_and_sends_nothing() {
        // Koşan komut: imlecin satırı çıpasız, korunan yalnız o. `cat -v`
        // kabuğa ya da programa giden her baytı görünür kılıyor — `^L`
        // gitseydi satır `^Lz` olurdu.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("stty -echo; seq 1 30; exec cat -v", Arc::clone(&wake));
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "30"
        });

        assert!(session.clear_to_start());
        assert_eq!(history_size(&session), 0);
        assert_eq!(screen_lines(&session), vec![String::new(); 10]);
        assert_eq!(cursor_point(&session), (0, 0));

        session.write(b"z\n");
        wait_until("program yanıt vermedi", Duration::from_secs(5), || {
            !line_text(&session, 0).is_empty()
        });
        assert_eq!(
            line_text(&session, 0),
            "z",
            "temizlik programa bayt yolladı"
        );
        session.shutdown();
    }

    #[test]
    fn clear_scrollback_leaves_the_screen_untouched() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("stty -echo; seq 1 30; sleep 5", Arc::clone(&wake));
        wait_seq_tail(&session, &wake);
        let before = (screen_lines(&session), cursor_point(&session));
        assert!(history_size(&session) > 0, "sahne geçmişsiz kuruldu");

        assert!(session.clear_scrollback());
        assert_eq!(history_size(&session), 0, "geçmiş silinmedi");
        assert_eq!((screen_lines(&session), cursor_point(&session)), before);
        session.shutdown();
    }

    #[test]
    fn clearing_does_nothing_on_the_alternate_screen() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; seq 1 30; read _; printf '\\033[?1049halt'; read _; \
             printf '\\033[?1049l'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);
        let alt = || session.term.lock().mode().contains(TermMode::ALT_SCREEN);
        session.write(b"\n");
        wait_until(
            "alternatif ekrana geçilmedi",
            Duration::from_secs(5),
            || alt() && screen_lines(&session).iter().any(|line| line == "alt"),
        );
        let before = (screen_lines(&session), cursor_point(&session));

        assert!(!session.clear_to_start(), "alternatif ekranda temizlendi");
        assert!(!session.clear_scrollback(), "alternatif ekranda temizlendi");
        assert_eq!((screen_lines(&session), cursor_point(&session)), before);

        // Birincil ekranın geçmişi de yerinde.
        session.write(b"\n");
        wait_until(
            "birincil ekrana dönülmedi",
            Duration::from_secs(5),
            || !alt(),
        );
        assert_eq!(history_size(&session), 21);
        assert_eq!(line_text(&session, 8), "30");
        session.shutdown();
    }

    #[test]
    fn after_a_clear_the_fill_brings_back_only_the_new_rows() {
        // `the_fill_stops_at_the_rows_that_arrived_after_the_clear`'ın
        // reçetesi, `CSI 2 J` yerine temizlikle: `seq 101 110` tek satırı
        // geçmişe itiyor ve doldurma yalnız onu veriyor — silinen ekran geri
        // gelmiyor.
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            "stty -echo; seq 1 30; read _; seq 101 110; read _; \
             printf '\\033[6A\\033[J'; sleep 5",
            Arc::clone(&wake),
        );
        wait_seq_tail(&session, &wake);

        assert!(session.clear_to_start());
        // Bayrağı kuran kare ve damgayı alan kare, çıktı gelmeden: damga bir
        // kare geç alınıyor ([`Session::screen_clear_history`]).
        cursor_now(&session);
        cursor_now(&session);
        assert!(screen_cleared(&session), "temizlik bayrağı kurmadı");

        session.write(b"\n");
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 8) == "110"
        });
        assert_eq!(history_size(&session), 1);
        session.write(b"\n");
        wait_until("içerik kısalmadı", Duration::from_secs(5), || {
            cursor_now(&session).content_rows <= 4
        });

        let (cursor, cells) = fill_now(&session);
        assert!(!screen_cleared(&session), "defter büyüdü ama bayrak durdu");
        assert!(
            cursor.rows - cursor.content_rows > cursor.fill,
            "sahne kırpmasız kuruldu: {cursor:?}"
        );
        let text: Vec<String> = (0..cursor.fill).map(|r| row_text(&cells, r)).collect();
        assert_eq!(text, ["101"], "{cells:?}");
        session.shutdown();
    }

    #[test]
    fn clearing_drops_both_selections_and_returns_the_window_to_the_bottom() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_docked_session(
            &format!(
                "seq 1 30; printf 'hello world\\r\\n{}echo foo bar{}'; sleep 5",
                anchored_prompt(1),
                mirror("ZWNobyBmb28gYmFy", 12),
            ),
            Arc::clone(&wake),
        );
        wait_mirror(&session, DockStatus::Live);
        draw_dock(&session);
        assert!(matches!(scroll(&session, 3), Wheel::Scrolled(n) if n > 0));
        session
            .scroll_frac
            .store(0.5f64.to_bits(), Ordering::Relaxed);
        {
            let mut term = session.term.lock();
            let mut selection = Selection::new(
                SelectionType::Simple,
                Point::new(Line(0), Column(0)),
                Side::Left,
            );
            selection.update(Point::new(Line(0), Column(3)), Side::Right);
            term.selection = Some(selection);
        }
        select_dock(&session, 0, 4);
        assert!(lock(&session.shell).dock_selection.is_some());

        assert!(session.clear_to_start());
        assert!(
            session.term.lock().selection.is_none(),
            "ızgara seçimi kaldı"
        );
        assert!(
            lock(&session.shell).dock_selection.is_none(),
            "dock seçimi kaldı"
        );
        assert_eq!(display_offset(&session), 0, "pencere dibe dönmedi");
        assert_eq!(
            session.scroll_frac.load(Ordering::Relaxed),
            0,
            "kesir kaldı"
        );
        session.shutdown();
    }

    #[test]
    fn clearing_loses_the_current_match_even_with_an_empty_history() {
        // Geçmiş 0 → 0: `history` farkı sıfır ve temizlik olmasa kayma
        // `Still` çıkar — geçerli eşleşme artık boş olan satırı gösterirdi.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "stty -echo; printf 'foo\\nbar\\n'; sleep 5",
            Arc::clone(&wake),
        );
        wait_until("çıktı gelmedi", Duration::from_secs(5), || {
            line_text(&session, 1) == "bar"
        });
        assert_eq!(history_size(&session), 0, "sahne geçmişle kuruldu");
        assert_eq!(session.set_search(&plain("foo")), SearchStatus::Ready);
        assert_eq!(current_at(&session), Some((0, 0)));
        count_all(&session);
        let searches = || wake.state.lock().unwrap().searches;
        let base = searches();

        assert!(session.clear_to_start());
        assert_eq!(history_size(&session), 0);
        assert_eq!(searches(), base + 1, "sayım yeniden başlatılmadı");
        search_now(&session);
        assert_eq!(current_at(&session), None, "kaymış satır geçerli kaldı");
        let report = count_all(&session);
        assert_eq!(report.total, 0, "{report:?}");
        assert_eq!(current_at(&session), None);
        session.shutdown();
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_clear_to_start_and_frame() {
        // 034 `screen_clears`'a **ikinci yazar** getirdi: ana thread ⌘K'de
        // `Term` kilidi altında artırıyor, okuyucu thread `CSI 2 J`'yi
        // kilitsiz sayıyor, kare yolu ikisini kilit altında tüketiyor.
        // Çıktı akarken üç yol yarışıyor; kilit sırası bozulursa sınama
        // asılı kalır; birincil ekranda her temizlik `true` dönmek zorunda.
        let wake = Arc::new(TestWake::default());
        let session = Arc::new(spawn_docked_session(
            "stty -echo; while :; do seq 1 12; printf '\\033[2J\\033[H'; sleep 0.01; done",
            Arc::clone(&wake),
        ));
        let deadline = Instant::now() + Duration::from_secs(2);
        let clearer = {
            let session = Arc::clone(&session);
            std::thread::spawn(move || {
                let mut clears = 0u64;
                while Instant::now() < deadline {
                    let cleared = if clears % 3 == 0 {
                        session.clear_scrollback()
                    } else {
                        session.clear_to_start()
                    };
                    assert!(cleared, "birincil ekranda temizlenmedi");
                    clears += 1;
                    std::thread::sleep(Duration::from_millis(3));
                }
                clears
            })
        };
        let mut frames = 0u64;
        while Instant::now() < deadline {
            cursor_now(&session);
            frames += 1;
        }
        assert!(clearer.join().unwrap() > 0, "hiç temizlenmedi");
        assert!(frames > 0, "yarış boyunca hiç kare üretilmedi");
        assert!(session.reader_alive(), "okuyucu thread yarışta öldü");
        session.shutdown();
    }
}
