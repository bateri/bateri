//! Bir terminal oturumu: PTY, okuyucu thread ve grid.
//!
//! Crate'in kapsül sözleşmesi `lib.rs`'te; burada onun iki pratik sonucu
//! yaşıyor: alacritty'nin `EventListener`'ı `Adapter`'da bizim `Wake`'imize
//! çevrilir, ve `Term` kilidi **yalnız** şu çağrı yerlerinde alınır:
//! `frame`, `resize`, seçim yolu (`set_selection`, `update_selection`,
//! `clear_selection`, `selection_text`), kaydırma yolu (`scroll_wheel`,
//! `scroll_page`), kullanıcı girdisinin gönderimi (`send_input`: seçimin
//! temizliği, dibe dönüş ve okun kip sorusu aynı kilitte), `paste`'in kip
//! sorgusu (`bracketed_paste`) ve terminal seçeneklerinin canlı değişimi
//! (`set_terminal_options`).
//! Kilit **sırası** her yerde aynıdır — `term` önce, `size` sonra; yeni bir yer
//! eklerken bu sıraya uyulur, çünkü iki kilit ters sırada alınırsa kilitlenme
//! doğar. `theme` ve `shell` bu sıranın dışında birer **yaprak** kilittir:
//! tutulurken başka hiçbir kilit alınmaz, yani hangi kilidin altında alındığı
//! önemsizdir — `frame` temanın kopyasını `term`'den önce alıp bırakır, renk
//! sorusu `term` tutulurken okur, `set_theme` tek başına yazar.
//!
//! `shell` için kural tek yönlüdür ve yönü şudur: **`shell` tutulurken `term`
//! alınmaz.** Okuyucu thread `shell`'i zaten `term`'ün *altında* yazıyor —
//! alacritty `pty_read` boyunca terminal lease'ini elinde tutuyor
//! (`event_loop.rs`, `_terminal_lease`) ve bizim `TappedPty::read`'imiz o
//! guard altında koşuyor — yani `term` → `shell` sırası okuyucunun kendi
//! sırası ve kilitlenemez. Kapanabilecek tek döngünün öteki kenarı ters yön
//! olurdu, o yüzden `frame`'in ikinci fazı (blok şeritleri) `shell`'i **`term`
//! bırakıldıktan sonra** alıyor, `shell_state` ve `set_terminal_options` da
//! öyle. (`/audit`, 010 kapı: buradaki eski gerekçe yasağı ters yöne koyuyordu
//! ve okuyucunun kendi sırasını tehlikeli gösteriyordu.)

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::TermMode;
// Grid hücresi **takma adla**: bu modülün `Cell`'i sınırdan geçen kare kaydı
// ve ikisi aynı ada gelseydi hangi bütçeye tabi olduğu okunamazdı
// (`CLAUDE.md` → hücre sabit boyuttadır).
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, Osc52 as TermOsc52, RenderableContent, Term};
// `EventedReadWrite` ada geliyor çünkü `io::Read` gövdesi `Pty::reader()`'ı
// çağırıyor, yani kendi impl bloğunun dışından; `EventedPty` ve `io::Read`
// gelmiyor, onların tek çağrı yeri kendi impl blokları.
use alacritty_terminal::tty::{self, EventedReadWrite as _, Pty, Shell};
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Handler};
// `Event` adı bu modülde alacritty'nin olayına ait; `polling`'inki `TappedPty`
// dışında hiç geçmediği için ada gelen o, takma alan o.
use polling::{Event as PollingEvent, PollMode, Poller};

use crate::color::{self, LinearRgba, Theme};
use crate::dock::{self, Dock, DockCols};
use crate::input::{
    self, Arrow, ButtonRoute, MouseButton, MouseEncoding, MouseModifiers, WHEEL_DOWN, WHEEL_UP,
    WheelRoute,
};
use crate::settings::{CaretShape, CursorBlink};
use crate::shell::{
    COUNTER_FLOOR, CaretHome, Counter, DockContext, DockState, Precision, Scanner, ShellLog,
    ShellState, Stripe,
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
/// değil, bu tipin kare başına maliyeti olur — ve o ölçüm bekliyor.
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
    /// Devrin tek yükleminin (`shell::caret_home` + üç ön koşul + **tutma**)
    /// sınırdan geçen hâli: [`Session::dock`] onu **argüman** olarak alıyor ve yeniden
    /// hesaplamıyor. `!visible` ile karıştırılmamalı — imleç uygulamanın
    /// gizlemesiyle de, geçmişe kaydırmayla da görünmez olur ve o hâllerde
    /// devralan kimse yoktur.
    pub caret_in_dock: bool,
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
    pub fill: u16,
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
}

impl Blocks {
    /// Bu karede ızgarada çizilecek bloklar, satır sırasıyla.
    pub fn as_slice(&self) -> &[Block] {
        &self.resolved
    }

    /// Bu karede **doldurma bandında** çizilecek bloklar; satırlar
    /// fill-yerel (`0..fill`).
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
    /// Çocuğa **eklenen** ortam değişkenleri; geri kalanı miras.
    ///
    /// Öncelik, güçlüden zayıfa: `TERM` ve `COLORTERM` (bu crate'in sabiti,
    /// ezilemez — `TERM` bir sözleşme, bkz. `CLAUDE.md`) > bu harita >
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

/// alacritty `Config`'ini seçeneklerin **tamamından** kurar — açılışın
/// (`Session::spawn`) ve canlı değişimin ([`Session::set_terminal_options`])
/// tek yolu.
///
/// Geri kalan alanlar (`semantic_escape_chars`, imleç biçimleri,
/// `kitty_keyboard`) alacritty'nin varsayılanında: onları hiçbir yer
/// kurmuyor, yani iki çağrı arasında da oynamıyorlar.
/// `term_config_keeps_every_other_field` bunu çiviliyor.
fn term_config(options: TerminalOptions) -> Config {
    Config {
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
        }))
    }

    /// Uygulamanın sorduğuna PTY'den yanıt verir. Kanala yazmak kilitsizdir;
    /// `Term` kilidi tutulurken çağrılmak serbesttir.
    fn reply(&self, text: String) {
        // Sıfır baytlık yazma `EventLoop`'un yazıcısını kilitler: `write`
        // `Ok(0)` döner, öge kuyruğun başına geri konur ve bir daha hiç
        // emilmez — poller seviye tetiklemeli olduğu için thread de %100'de
        // döner. alacritty kendi `Notifier`'ında aynı korumayı taşıyor.
        if text.is_empty() {
            return;
        }
        if let Some(sender) = self.0.sender.get() {
            // Kanal yalnız kapanışta ölür; o yolda sessiz kalmak doğrudur.
            let _ = sender.send(Msg::Input(text.into_bytes().into()));
        }
    }
}

impl EventListener for Adapter {
    fn send_event(&self, event: Event) {
        match event {
            Event::Wakeup => {
                // Sıra önemli: bayrak uyandırmadan ÖNCE dikilir. Ters sırada
                // uyanan taraf bayrağı henüz görmeden bakar ve kare kaçar.
                self.0.dirty.store(true, Ordering::Release);
                self.0.wake.wake();
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
            // Başlık ve zil bu sette yok; bilinmeyen dizi gibi sessizce
            // düşerler (`CLAUDE.md` → PTY yolunda panik yok). "Yoksayılır ve
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
            Event::Title(_)
            | Event::ResetTitle
            | Event::Bell
            | Event::ClipboardLoad(..)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Exit => {}
        }
    }
}

/// Okuma yolundan geçen baytları tarayan `Pty`.
///
/// `EventLoop` PTY tipinde jenerik; araya giren tek şey bu sarmalayıcı ve
/// **baytlara dokunmuyor** — [`io::Read::read`] içerideki `Pty`'den ne
/// okuduysa aynen döndürüyor, yalnız dönmeden önce dilimi tarayıcıya
/// gösteriyor. Ayrıştırıcı bu yüzden bugünküyle birebir aynı akışı görüyor.
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
    /// `CSI 2 J`. Artıran **yalnız** burası.
    screen_clears: Arc<AtomicU32>,
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
        self.scanner.feed(&buf[..read], |event| {
            lock(&self.shell).apply_scan(event);
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

/// Prompt hücresine iliştirilmiş OSC 8 bağlantısından blok kimliği; bizim
/// olmayan bağlantı `None`.
///
/// Şema **bize özel** (`bateri://block/`) ve kapı bu önek: `ls --hyperlink`'in
/// `file://`'ı, bir `man` sayfasının `https://`'i ya da kullanıcının kendi
/// prompt'undaki bağlantı buradan geçmez. Kimliği basan taraf
/// `assets/shell/zsh/bateri.zsh`.
///
/// Kimlik `u32` ve ondalık: betiğin sayacı `%9v` ile genişliyor, yani metin.
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
    Scrolled(i32),
    /// Uygulamaya gitti: fare kipinde tekerlek raporu, alternate screen'de ok.
    Sent,
    /// Hiçbir şey gitmedi: alternate screen'de ok kapalı (`\e[?1007l`) ya da
    /// Shift basılı; fare kipinde işaretçi geçmişte ya da koordinat
    /// kodlamaya sığmıyor; uygulama yolunda sıfır satır.
    Ignored,
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
    /// Tarayıcının saydığı `CSI 2 J` — **nesil sayacı**, bayrağın kendisi
    /// değil. Artıranı okuyucu thread'i ([`TappedPty::read`]), okuyanı
    /// [`Session::observe_screen_clear`].
    ///
    /// `Arc`, çünkü öteki ucu sarmalayıcıyla okuyucu thread'inde ([`shell`]
    /// emsali). Atomik ve **yaprak kilit değil**, çünkü okunduğu yer `Term`
    /// kilidinin **altı**: bir muteks orada yasak (modül kuralı), atomik değil.
    ///
    /// [`shell`]: Session::shell
    screen_clears: Arc<AtomicU32>,
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
    /// Pencerenin dock'u var mı ([`SessionOptions::dock`]).
    ///
    /// Doğumda kararlaşıyor ve bir daha değişmiyor, o yüzden ne kilit ne
    /// atomik: `[shell] integration` **sonraki oturumda** geçerli (`CLAUDE.md`)
    /// ve dock'un varlığı ona bağlı.
    dock: bool,
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
        let pty_options = tty::Options {
            shell: options
                .command
                .map(|(program, args)| Shell::new(program, args)),
            working_directory: options.working_directory,
            env,
            ..Default::default()
        };
        let pty = tty::new(&pty_options, size, 0)?;
        // Yuva `EventLoop`'tan **önce** doğuyor: bir ucu sarmalayıcıyla okuyucu
        // thread'ine gidiyor, öteki ucu `Session`'da kalıyor.
        // Defterin tavanı `scrollback`'ten: blok başına en az bir satır düştüğü
        // için geçmişte görünebilecek blok sayısının üst sınırı odur.
        let shell = Arc::new(Mutex::new(ShellLog::new(options.terminal.scrollback)));
        // Sayacın da iki ucu var ve ikisi de aynı gerekçeyle burada doğuyor.
        let screen_clears = Arc::new(AtomicU32::new(0));
        let pty = TappedPty {
            pty,
            scanner: Scanner::new(),
            shell: Arc::clone(&shell),
            screen_clears: Arc::clone(&screen_clears),
        };

        // **Blink de açılışta geçiyor**, temanın yanında: tek yazıcısı
        // `set_terminal_options` olsaydı ayar yalnız oturum içinde bir kayıttan
        // **sonra** uygulanır, taze pencerede sessizce yok sayılırdı.
        let adapter = Adapter::new(wake, size, options.theme, options.terminal.blink);
        let config = term_config(options.terminal);
        let term = Arc::new(FairMutex::new(Term::new(config, &grid, adapter.clone())));

        let event_loop = EventLoop::new(
            Arc::clone(&term),
            adapter.clone(),
            pty,
            pty_options.drain_on_exit,
            false,
        )?;
        let sender = event_loop.channel();
        // Kanal ancak burada doğar; adapter'ın kopyaları aynı gövdeyi
        // paylaştığı için tek `set` hepsini bağlar.
        let _ = adapter.0.sender.set(sender.clone());

        Ok(Self {
            term,
            sender,
            adapter,
            reader: Mutex::new(Some(event_loop.spawn())),
            shell,
            // Açılışta alternatif ekran yok; ilk içerik karesi zaten yazacak.
            alt_screen: AtomicBool::new(false),
            screen_clears,
            // Açılışta sindirilmemiş temizleme yok: sayaç da, hesaba katılan
            // nesil de sıfır. Üçünü de sıfırdan başlatmak, ilk karenin
            // bayrağı sebepsiz kurmasını önlüyor.
            screen_seen: AtomicU32::new(0),
            screen_cleared: AtomicBool::new(false),
            // Damga da yok: bayrak kurulu olmadığı için okunmuyor, ilk kare
            // onu defterin boyuyla değiştiriyor.
            screen_clear_history: AtomicUsize::new(Self::UNSTAMPED),
            // Bant da yok: ilk kare doldurmayı hesaplayıp yazacak.
            fill_shown: AtomicU16::new(0),
            dock: options.dock,
        })
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
    pub fn frame(
        &self,
        mut sink: impl FnMut(Cell),
        mut fill_sink: impl FnMut(Cell),
        blocks: &mut Blocks,
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
        let (suppressed_block, caret) = {
            let log = lock(&self.shell);
            (log.suppressed_input(), log.caret(Instant::now()))
        };
        blocks.anchors.clear();
        blocks.resolved.clear();
        blocks.fill_anchors.clear();
        blocks.fill_resolved.clear();
        let mut term = self.term.lock();

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
        // ızgaradan, arkasındaki karakter sayısı aynadan. Son karakterin
        // sütunu `cursor_col + chars_after - 1`, satır farkı da onun `cols`'a
        // bölümü; `saturating_sub(1)` şart, çünkü satırı **tam dolduran**
        // metin bir satır fazla verirdi ve o satır tamamlama listesinin ilki
        // olurdu.
        //
        // **Hatası yönlü ve bu bilinçli:** `BUFFER`'da satır sonu (PS2,
        // Esc-Enter) ya da geniş glyph varsa gerçek satır sayısı hesaptan
        // büyüktür, yani **eksik** bastırılır — sızıntı o satırlarla sınırlı
        // kalır, fazla bastırma olmaz. Tek fazla-bastırma yolu bayat ayna
        // (`line-pre-redraw` çizimden önce koşuyor) ve o bir karelik.
        let suppress_to = suppressed_block.and_then(|input| {
            let cols = usize::from(term.columns().max(1) as u16);
            let last = usize::from(cursor_col).saturating_add(input.chars_after_cursor);
            let below = u16::try_from(last.saturating_sub(1) / cols).unwrap_or(u16::MAX);
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
            let blank_mirror = input.chars_before_cursor == 0 && input.chars_after_cursor == 0;
            let at_anchor = !blank_mirror
                || anchor_row_at_or_above(&term, to, offset, input.block)
                    .is_none_or(|anchor| anchor == to);
            let fresh = last_ink_in_row(&term, to, offset) == input.last_ink && at_anchor;
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
        let suppress_floor = suppressed_block.map_or(0, |input| {
            let cols = usize::from(term.columns().max(1) as u16);
            let above = input
                .chars_before_cursor
                .saturating_sub(usize::from(cursor_col))
                .div_ceil(cols);
            cursor_screen_row.saturating_sub(u16::try_from(above).unwrap_or(u16::MAX))
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

        for indexed in display_iter {
            let cell = indexed.cell;
            let flags = cell.flags;
            let dim = flags.contains(Flags::DIM);
            let hidden = flags.contains(Flags::HIDDEN);
            // Seçim vurgusu ters videodur — yeni shader/uniform yok, hücrenin
            // kendi iki rengi takaslanıyor. `^`, `||` değil: seçim ters
            // videoyu **çevirir**, seçili ters videolu hücre normal renklerine
            // döner. `||` ile ters videolu bir satırın (vim durum satırı, tmux
            // çubuğu) seçimi hiç görünmüyordu — seçili hücre seçilmemişle aynı
            // renkteydi. Emsal alacritty: hücreyi önce `INVERSE` için takaslıyor,
            // sonra varsayılan seçim renkleri (`CellBackground`/`CellForeground`)
            // takaslanmış iki rengi bir kez daha takaslıyor. İmlecin altındaki
            // hücreyle seçim çakışırsa imleç kazanır: seçim hücrenin
            // renklerini takaslıyor, imleç ise bloğun **piksellerini** eziyor
            // (bkz. [`Cursor`]) ve sonuncu söz çizenin.
            //
            // Gizli metin seçilince de vurgulanmaz: `HIDDEN` "çizme" demek ve
            // seçim onu delseydi gizli hücrenin yeri boyalı bir blok olarak
            // görünürdü. Gizli metni kopyalamak isteyen phase-2'de
            // `selection_text()`'e sorar — vurgu ile metin aynı kapıdan geçmek
            // zorunda değil. `contains` değil `contains_cell`, iki sebeple:
            // blok imleç seçimin **ucunda** durursa o hücre seçilmiş sayılmaz
            // (alacritty'nin istisnası; imlecin **kendi** noktası bu yüzden
            // veriliyor — hücrenin noktası verilince istisna her uca
            // uygulanıyordu), ve aralık bir spacer'da başlarsa geniş
            // karakterin baş hücresi de vurgulanır. Seçimin ortasındaki imleç
            // hücresi seçili sayılır ve `inverse`'i çevrilir; görünen sonuç
            // yalnız opak bloğun altında kalan arka plandır.
            // `set_selection` spacer'dan başlayan aralık kurmaz (`anchor`
            // spacer'ı `Right` yapıyor), ama seçimden sonra satır yeniden
            // yazılıp o hücre spacer olursa aralık orada başlar.
            // **Seçim içeriği vurgular, içerik yaratmaz.** Aralık boş
            // hücreleri de kapsıyor ve onları ters çevirmek "burada bir şey
            // var" demek oluyordu: boş ekranda fareyi sürükleyen kullanıcı
            // koca bir blok görüyor, üstelik o seçim **hiçbir şey
            // kopyalamıyor** (gözlendi, 2026-09-18). Vurgu ile metnin
            // ayrışması bu deponun yasakladığı sınıf — göz "seçtim" derken
            // pano boş geliyor.
            //
            // Ölçüt "mürekkep" **değil** "çizilir mi": zemin de sütunu işgal
            // ediyor (013 Karar 7'nin aynısı). Ters videolu bir boşluk —
            // vim'in durum satırı, tmux çubuğu — mürekkepsizdir ama
            // görünürdür ve seçilince vurgulanmalıdır; varsayılan zeminli boş
            // bir hücre ise görünmezdir ve seçim onu görünür kılmamalıdır.
            // İkisini ayıran şey aşağıdaki atlama koşulunun **ta kendisi**,
            // yalnız seçim uygulanmadan önceki hâliyle sorulmuş hâli.
            //
            // Satır taraması yok: soru hücrenin kendisine sorulabiliyor.
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
            // onu bütün kopyalarken. Bugün belirti geniş glyph'in tek yuvaya
            // kırpılmasıyla örtülü; 016 çizmeye başlayınca görünür olurdu ve
            // hiçbir sayaç göremezdi.
            let drawable =
                plain_back != background || ch.is_some() || ruled || flags.intersects(SPACERS);
            let selected = !hidden
                && drawable
                && selected_range
                    .as_ref()
                    .is_some_and(|range| range.contains_cell(&indexed, cursor_point, cursor_shape));
            let inverse = plain_inverse ^ selected;

            // **Arka plan önce**: atlama koşulunun ağır yarısı bu ve boş
            // grid'de hücrelerin neredeyse tamamı burada eleniyor. Ön plan
            // zincirini de bu satırın önüne almak, `Term` kilidi tutulurken
            // hücre başına ikinci bir renk çözümünü çizilmeyen hücreler için
            // de ödemek olurdu.
            //
            // Ters video hücrenin iki rengini takas eder; `DIM` ise **ön
            // plana** uygulanır (adlı rengi sönük eşine çeviren kod
            // alacritty'nin ikili tarafında, kitaplıkta değil). İkisi
            // birleşince kural şu: sönüklük, `cell.fg`'den doğan renge gider —
            // ters videoda o renk arka plan olmuştur. "Ters video" burada
            // seçimin çevirdiği `inverse`: seçili ters videolu hücrede sönüklük
            // yeniden ön plana döner.
            //
            // Kural `color::resolve_fg`'de tek: iki dal aynı fonksiyondan
            // geçiyor, ters videolu dal sönük rolü unutamıyor.
            // Seçim `plain_back`'i takaslıyor; seçilmemiş hücrede ikinci bir
            // çözüm yok, `plain_back` zaten cevap.
            let back = if selected {
                if inverse {
                    color::resolve_fg(cell.fg, dim, colors, &theme)
                } else {
                    color::resolve(cell.bg, colors, &theme)
                }
            } else {
                plain_back
            };
            // Varsayılan arka plan çizilmez; `None` onun adı.
            let bg = (back != background).then(|| color::linear_rgba(back));

            // Atlama koşulu: ne boyanacak bir arka plan, ne çizilecek bir
            // mürekkep, ne de bir kural çizgisi. Boş grid'de bu koşul her
            // hücreye uyar ve sink hiç çağrılmaz — `frame()`'in boştaki
            // maliyeti iterasyonun kendisi.
            if bg.is_none() && ch.is_none() && !ruled {
                continue;
            }
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
                if blocks.anchors.last().map(|&(last, ..)| last) != Some(id) {
                    // Son mürekkep sütunu sıfırdan başlıyor: hiç mürekkebi
                    // olmayan komut satırında (boş prompt) sayaç sağda,
                    // kimseye değmeden duruyor.
                    blocks.anchors.push((id, row, 0));
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
            if caret_in_dock
                && let (Some(from), Some(to)) = (suppress_from, suppress_to)
                && (from.max(suppress_floor)..=to).contains(&row)
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

            let col = indexed.point.column.0 as u16;
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
            });
        }

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
        let fill = self.fill_rows(
            &term,
            grid_rows.saturating_sub(content_rows),
            alt_screen,
            offset != 0,
        );
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
        if offset == 0 {
            self.fill_shown.store(fill, Ordering::Relaxed);
        }
        // **Geçmişten okuma ayrı bir döngü ve bu stil değil zorunluluk.**
        // Yukarıdaki döngünün `debug_assert!((0..rows).contains(&row))`
        // bekçisi negatif satırda patlardı, ve doldurulan satırlar
        // `drawn_rows`'a **girmemeli**: girselerdi öteleme kapanır, içerik
        // tabandan kopardı (R2.3, `27a0b98`'in maliyeti).
        //
        // Satır numarası **fill-yerel** (`0..fill`) ve sırası geçmişin kendi
        // sırası: `0` en eski, `fill - 1` defterin en yeni satırı, yani
        // içeriğin hemen üstü. Ekran satırına çeviren taraf **çizen** taraf
        // (phase-3) — bu crate "hangi satırlar" der, "nereye" demez.
        //
        // `grid_clamp` bir emniyet kemeri: `fill <= history_size` olduğu için
        // satır zaten defterin içinde, ama `bt-core`'da indeksleme panik
        // yasağının altında (R2.5) ve yasağı tip değil **çağrı yeri** taşıyor.
        for fill_row in 0..fill {
            // Ofset terimi **yok**: doldurma yalnız dibe yaslı pencerede
            // koşuyor ([`Session::fill_rows`]), yani `Line(-1)` her zaman
            // defterin en yeni satırı.
            let line =
                Line(i32::from(fill_row) - i32::from(fill)).grid_clamp(&*term, Boundary::Grid);
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
                if let Some(id) = cell.hyperlink().and_then(|link| block_id(link.uri()))
                    && blocks.fill_anchors.last().map(|&(last, _)| last) != Some(id)
                {
                    blocks.fill_anchors.push((id, fill_row));
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
                });
            }
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
            rows: grid_rows,
            // Faz 2 dolduruyor: koşan bloğun çıpasının bu karede **görünüp
            // görünmediği** ancak orada biliniyor ve saatin durma koşulu tam
            // olarak o.
            next_tick: None,
        };
        drop(term);

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
        // **ayrık**, yani tutma sürerken `suppressed_block` her zaman `None`.
        // Değişmez bu yüzden güçlü hâlinde yazılabiliyor: tutma boyunca
        // `caret_in_dock == self.dock && !alt_screen && home == Dock`.
        if self.dock && !alt_screen {
            cursor.next_tick = crate::shell::sooner(cursor.next_tick, caret.hold_left);
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
        let history = term.history_size();
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
        let stamp = self.screen_clear_history.load(Ordering::Relaxed);
        let fresh = if stamp == Self::UNSTAMPED {
            history
        } else {
            history.saturating_sub(stamp)
        };
        gap.min(u16::try_from(fresh).unwrap_or(u16::MAX))
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
    pub fn set_selection(&self, start: SelectionPoint, end: SelectionPoint) {
        let mut term = self.term.lock();
        let (start_point, start_side) = anchor(&term, start);
        let (end_point, end_side) = anchor(&term, end);
        let mut selection = Selection::new(SelectionType::Simple, start_point, start_side);
        selection.update(end_point, end_side);
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
        if changed {
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
    pub fn scroll_wheel(&self, lines: i32, at: SelectionPoint, shift: bool) -> Wheel {
        let mut term = self.term.lock();
        let unit = match input::wheel_route(*term.mode(), shift) {
            WheelRoute::Scroll => {
                // Yol zaten birincil ekran; `None` yalnız `scroll_locked`'ın
                // kendi kip kapısından gelebilir.
                let moved = scroll_locked(&mut term, lines, self.band_shown());
                drop(term);
                self.wake_if_moved(moved);
                return moved.map_or(Wheel::Ignored, Wheel::Scrolled);
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
    pub fn scroll_page(&self, pages: i32) -> Option<i32> {
        let moved = {
            let mut term = self.term.lock();
            let lines = pages.saturating_mul(term.screen_lines() as i32);
            scroll_locked(&mut term, lines, self.band_shown())
        };
        self.wake_if_moved(moved);
        moved
    }

    /// Ekranda duran doldurma bandının boyu, kaydırmanın anladığı tipte.
    ///
    /// [`Session::fill_shown`]'ın tek okuyucusu; üç kaydırma yolu da buradan
    /// geçiyor ki "bant nereden başlar" sorusunun tek cevabı olsun.
    fn band_shown(&self) -> i32 {
        i32::from(self.fill_shown.load(Ordering::Relaxed))
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
    /// **kendisi**, ikinci bir kopyası değil: "bayrak uyandırmadan önce"
    /// sırası tek yerde yaşıyor. `Term` kilidi **bırakıldıktan sonra**
    /// çağrılır: uyandırma çift muteksli `FairMutex` tutulurken koşmamalı.
    ///
    /// `resize` bunu **kullanmıyor**: onun uyandırması `bt-shell`'in işi
    /// (link'i kendisi açıyor). Buradaki çağıranların (seçim ve kaydırma)
    /// ise elinde link yok.
    fn request_frame(&self) {
        self.adapter.send_event(Event::Wakeup);
    }

    /// Seçili aralığın metni — kopyalamanın (phase-2) ve sınamaların **tek**
    /// yolu. Satır sarma ve geniş karakter spacer'ları alacritty'nin içinde
    /// çözülür; ikinci bir metin yolu, ikinci bir sarma hatası demek olurdu.
    ///
    /// Okumadır, seçimi **temizlemez**: Cmd-C girdi değil, `send_input`'a hiç
    /// varmaz — kopyaladıktan sonra vurgu ekranda kalır (alacritty de öyle).
    pub fn selection_text(&self) -> Option<String> {
        self.term.lock().selection_to_string()
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
    pub fn dock(
        &self,
        cols: DockCols,
        into: &mut DockState,
        context: &mut DockContext,
        caret_in_dock: bool,
        sink: impl FnMut(Cell),
    ) -> Dock {
        let theme = *lock(&self.adapter.0.theme);
        let shell = {
            let shell = lock(&self.shell);
            into.clone_from(&shell.dock);
            // Bağlam **aynı kilit turunda**: ayrı bir turda alınsaydı araya
            // düşen bir prompt dizini yeni, dalı eski bir satırla eşleştirirdi.
            context.clone_from(&shell.context);
            shell.state
        };
        dock::render(into, context, shell, &theme, cols, caret_in_dock, sink)
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
    /// yolluyor. O kol bugün boş; bir gün başlık çizilirse kolu kilit
    /// almamalı — `Wake` sözleşmesiyle aynı yasak, yoksa bu çağrı kendi
    /// kendini kilitler (`race_set_terminal_options_and_frame` asılı kalır).
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
    fn send_input(&self, bytes: impl FnOnce(TermMode) -> Vec<u8>) {
        let (bytes, cleared, moved) = {
            let mut term = self.term.lock();
            let bytes = bytes(*term.mode());
            if bytes.is_empty() {
                return;
            }
            // Seçim dibe dönüşten **önce** düşer: "çizili miydi" sorusu
            // kullanıcının baktığı pencereye sorulmalı. Pencere kayarsa kare
            // zaten isteniyor; kaymazsa iki soru aynı cevabı verir.
            let cleared = clear_selection_locked(&mut term);
            (
                bytes,
                cleared,
                scroll_locked(&mut term, i32::MIN, self.band_shown()),
            )
        };
        // Tek istek: temizlik ve dönüş aynı kareyi istiyor, vuruş başına iki
        // uyandırma olmasın. "Kaydı mı" kuralı `wake_if_moved`'da kalıyor.
        if cleared {
            self.request_frame();
        } else {
            self.wake_if_moved(moved);
        }
        self.send(Msg::Input(bytes.into()));
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
        // Sarma sorgusu **önce ve tek başına**: iki kilit (`Term`, sonra
        // `shell`) ardışık alınıyor, iç içe değil.
        if self.bracketed_paste() && !self.can_be_typed(&bytes) {
            let mut wrapped = Vec::with_capacity(bytes.len() + 12);
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
    fn can_be_typed(&self, bytes: &[u8]) -> bool {
        if !lock(&self.shell)
            .suppressed_input()
            .is_some_and(|input| input.insert_keymap)
        {
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

        // Ucuz kapı önce. Canlı boyutlandırmada `windowDidResize:`
        // çağrılarının çoğu hücre sınırını geçmez ve hiçbir şey yapmaz;
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
        let Some(reader) = lock(&self.reader).take() else {
            return Teardown::AlreadyDone;
        };
        self.send(Msg::Shutdown);

        // `join` de düşme de bloklayabilir (iki sebep yukarıda), yani ikisi
        // de bu thread'de koşmuyor. Kanal iki şey taşıyor: **zamanlama**
        // ("bitti" haberi gelmezse sınır dolmuştur) ve okuyucunun paniğe
        // düşüp düşmediği. İkincisi `()` ile taşınamazdı ve taşınmayınca
        // panikleyen bir okuyucu raporda `temiz` görünüyordu.
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
                let _ = done.send(reader_ok);
            });
        // Thread kurulamazsa (OS thread sınırı) **sınır yoktur** ve bu dalda
        // kapanışın nerede koştuğu bir yarışa bağlı: tutamak `spawn`
        // başarısız olurken closure ile birlikte çoktan düşmüştür, yani
        // `(EventLoop, State)` çiftini ya okuyucu thread kendi bitişinde
        // düşürür (kimse bloklanmaz, ama `SIGHUP` + `child.wait()` sınırsız
        // koşar) ya da — okuyucu thread çoktan bitmişse — çift orada düştüğü
        // için `Pty::drop` bu thread'i bloklar. İkisi de sessiz kalmasın.
        if let Err(err) = teardown {
            eprintln!("bateri: kapanış thread'i kurulamadı ({err}), kapanış sınırsız");
            return Teardown::Unbounded;
        }
        match finished.recv_timeout(SHUTDOWN_GRACE) {
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
        // `shutdown()`suz düşen bir oturumda alacritty'nin okuyucu thread'i
        // "event loop channel closed" diye panikler.
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
    use crate::shell::{DockContext, DockFault, DockState, DockStatus, ShellPhase};

    /// Sınamaların teması: gömülü koyu tema, `bt-shell`'in süreli koşusu gibi.
    const THEME: Theme = Theme::BATERI;

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
        session
            .take_damage()
            .then(|| session.frame(sink, |_| (), &mut Blocks::default()))
    }

    /// [`frame_if_damaged`]'in blok soran kardeşi: tamponu çağıran tutar,
    /// böylece sınama hem hücreleri hem şeritleri görebilir.
    fn blocks_if_damaged(session: &Session, blocks: &mut Blocks) -> bool {
        session.take_damage() && {
            session.frame(|_| (), |_| (), blocks);
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
                session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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
            session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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
            let cursor = session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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
                .frame(|_| (), |_| (), &mut Blocks::default())
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
                .frame(|_| (), |_| (), &mut Blocks::default())
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

        let cursor = session.frame(|_| (), |_| (), &mut Blocks::default());
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
        let empty = session.frame(|_| (), |_| (), &mut Blocks::default());

        wait_until("ilk tuş aynaya düşmedi", Duration::from_secs(3), || {
            let mut dock = DockState::default();
            session.dock_state(&mut dock);
            dock.status == DockStatus::Live && dock.buffer == "l"
        });
        let typed = session.frame(|_| (), |_| (), &mut Blocks::default());

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
            session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
            !cells.is_empty()
        });

        let cursor = session.frame(|_| (), |_| (), &mut Blocks::default());
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
                .frame(|_| (), |_| (), &mut Blocks::default())
                .visible
        });
        let cursor = session.frame(|_| (), |_| (), &mut Blocks::default());
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
        let cursor = session.frame(|_| (), |_| (), &mut Blocks::default());
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
        let cursor = session.frame(|_| (), |_| (), &mut Blocks::default());
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

        let cursor = session.frame(|_| (), |_| (), &mut Blocks::default());
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
        let cursor = session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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
        session.frame(|_| (), |_| (), &mut blocks);
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
        let cursor = session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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
        let wake = Arc::new(TestWake::default());
        let session = spawn_typing_session(&mirror("bHM", 2), Arc::clone(&wake));
        wait_mirror(&session, DockStatus::Live);

        let mut cells = Vec::new();
        let cursor = session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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
            &mut DockState::default(),
            &mut DockContext::default(),
            cursor.caret_in_dock,
            |_| (),
        );
        assert!(
            dock.caret.is_none(),
            "bayat aynada iki caret: ızgara gösteriyor, dock da sahipleniyor"
        );
        session.shutdown();
    }

    #[test]
    fn a_blank_mirror_below_the_anchor_is_stale() {
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

        let mut cells = Vec::new();
        let cursor = session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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

        let cursor = session.frame(|_| (), |_| (), &mut Blocks::default());
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
        let cursor = session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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
        session.frame(|cell| live.push(cell), |_| (), &mut Blocks::default());
        assert_eq!(row_glyphs(&live, 2), "", "ayna canlıyken bastırma yok");

        wait_mirror(&session, DockStatus::Idle);
        // Bastırma **anında** kalkıyor (`suppressed_input` aynanın `Live`
        // olmasını istiyor), caret ise tutma kadar gecikiyor (015 R1.1) —
        // ikisi ayrı yüklem ve ayrı hızda. `content_rows` caret'e bağlı,
        // yani ölçüm devir gerçekleştikten sonra alınmalı.
        wait_until("caret ızgaraya dönmedi", Duration::from_secs(2), || {
            session
                .frame(|_| (), |_| (), &mut Blocks::default())
                .visible
        });
        let mut cells = Vec::new();
        let cursor = session.frame(|cell| cells.push(cell), |_| (), &mut Blocks::default());
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
        session.frame(|_| (), |_| (), &mut blocks);
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
        session.frame(|_| (), |_| (), &mut blocks);
        assert_eq!(blocks.as_slice(), [], "çıktı satırı işaret aldı");

        // Dibe dönünce komutun satırı yine görünmüyor (27 satırlık içerikte
        // 10 satırlık pencere), ama ikinci prompt görünüyor ve `Pending`
        // olduğu için çizilmiyor: yine boş.
        session.term.lock().scroll_display(Scroll::Bottom);
        session.frame(|_| (), |_| (), &mut blocks);
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

    /// **Çok satırlı yapıştırma satırı ızgaraya bırakıyor** — uçtan uca.
    ///
    /// Zincirin tamamı sınanıyor: gerçek zsh bracketed yapıştırmayı alıyor,
    /// `BUFFER`'ı satır sonlarıyla birlikte tutuyor (ölçüldü, saf PTY:
    /// `BUFFER='echo a\necho b\n'`), ZLE kancası onu aynaya basıyor, çözücü
    /// "bu görüntü tek satıra sığmaz" diyor ve caret ızgarada kalıyor.
    ///
    /// Belirti kullanıcıda görüldü (2026-09-21): metin ızgarada, caret
    /// dock'un prompt işaretinin yanında — yazdığı yeri göremiyordu.
    #[test]
    fn a_bracketed_multiline_paste_leaves_the_line_and_the_caret_in_the_grid() {
        let home = empty_home("dock-paste");
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
        wait_dock(&session, &mut dock, |dock| {
            dock.status == DockStatus::Live && dock.buffer.is_empty()
        });

        // Bracketed yapıştırma: iki satır ve **kapanış satır sonu**. Son satır
        // sonu tamponda kalıyor, yani ızgaranın imleci boş bir satıra düşüyor
        // — tazelik kapısının kör noktasını doğuran şekil.
        session.write(b"\x1b[200~echo a\necho b\n\x1b[201~");
        wait_dock(&session, &mut dock, |dock| {
            dock.status == DockStatus::Multiline
        });
        assert!(
            dock.buffer.contains('\n'),
            "çok satırlı tampon beklenirdi: {dock:?}"
        );

        let cursor = session.frame(|_| (), |_| (), &mut Blocks::default());
        assert!(
            !cursor.caret_in_dock,
            "gösteremediğimiz satırın caret'i dock'ta kaldı: {cursor:?}"
        );
        assert!(cursor.visible, "{cursor:?}");

        session.shutdown();
        let _ = std::fs::remove_dir_all(&home);
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
        session.frame(|_| (), |_| (), &mut blocks);
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
        session.frame(|_| (), |_| (), &mut blocks);
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

        // **Üç** alanın dışında hiçbir şey kurulmuyor.
        for config in [before, scrolled, copying, shaped] {
            assert_eq!(
                Config {
                    scrolling_history: Config::default().scrolling_history,
                    osc52: Config::default().osc52,
                    default_cursor_style: Config::default().default_cursor_style,
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
        session.set_selection(at(0, 0, CellHalf::Left), at(4, 0, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("hello"));
        // Uçlar sırasız verilebilir: tersi aynı metni verir. Yarının sıraya
        // göre atanmadığının kanıtı da bu — ters çevrilen yarılar değil.
        session.set_selection(at(4, 0, CellHalf::Right), at(0, 0, CellHalf::Left));
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

        session.set_selection(at(0, 0, CellHalf::Right), at(4, 0, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("raba"));
        session.set_selection(at(0, 0, CellHalf::Left), at(4, 0, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("araba"));
    }

    #[test]
    fn selection_text_follows_the_half_of_the_right_end() {
        // Bitiş ucu **aynaya** bakar: sağ yarı kendi hücresini seçime katar,
        // sol yarı sınırı o hücrenin başına çeker. Yani fare bir hücrenin
        // ortasını geçtiği an o hücre yanar — iki uçta da kural bu.
        let session = word_session();

        session.set_selection(at(0, 0, CellHalf::Left), at(4, 0, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("araba"));
        session.set_selection(at(0, 0, CellHalf::Left), at(4, 0, CellHalf::Left));
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

        session.set_selection(at(2, 0, CellHalf::Right), at(2, 0, CellHalf::Right));
        assert_eq!(session.selection_text(), None);
        // Sol yarısı da boş: iki uç birbirinin **aynısı** olduğu sürece
        // seçim doğmaz, yarı ne olursa olsun.
        session.set_selection(at(2, 0, CellHalf::Left), at(2, 0, CellHalf::Left));
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
        session.set_selection(at(35, 0, CellHalf::Left), at(4, 1, CellHalf::Right));
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
        session.set_selection(at(0, 0, CellHalf::Left), at(3, 0, CellHalf::Right));
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
        session.set_selection(at(2, 0, CellHalf::Right), at(2, 0, CellHalf::Right));
        assert!(
            frame_if_damaged(&session, |_| ()).is_none(),
            "boş seçim kare istememeli"
        );

        session.set_selection(at(0, 0, CellHalf::Left), at(2, 0, CellHalf::Right));
        assert!(
            frame_if_damaged(&session, |_| ()).is_some(),
            "yeni aralık kare istemeli"
        );
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        // Hücre sınırı geçildi, aralık aynı: 2. sütunda bitiyor.
        session.set_selection(at(0, 0, CellHalf::Left), at(3, 0, CellHalf::Left));
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
        session.set_selection(at(1, 0, CellHalf::Left), at(1, 0, CellHalf::Left));
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

        session.set_selection(at(39, 0, CellHalf::Right), at(0, 1, CellHalf::Left));
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
        session.set_selection(at(0, 0, CellHalf::Left), at(0, 0, CellHalf::Right));
        assert!(frame_if_damaged(&session, |_| ()).is_some());

        // `read` satır sonunu bekliyor; gelince 30 satır `x`'i geçmişe iter.
        // Hazır: son satır (`30`) görünür.
        session.write(b"\n");
        wait_seq_tail(&session, &wake);

        session.set_selection(at(5, 5, CellHalf::Left), at(5, 5, CellHalf::Left));
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
        // Vurgu aralığın **iki ucunu da** kapsar. Blok imlecin sınır istisnası
        // (`contains_cell`) yalnız imlecin durduğu hücre içindir; imleç
        // noktası yerine hücrenin kendi noktası verilince istisna her sınır
        // hücresine uygulanıyor ve seçimin ilk ile son harfi hiç ters
        // videolanmıyordu. İmleç burada `araba`'nın sağında, 5. sütunda.
        let session = word_session();
        // Taban: seçimsiz `a`'nın ön planı. Kare istemek için seçim son
        // satıra kuruluyor — boş seçim (haklı olarak) kare istemez.
        let mut plain = Vec::new();
        session.set_selection(at(0, 9, CellHalf::Left), at(1, 9, CellHalf::Right));
        assert!(frame_if_damaged(&session, |c| plain.push(c)).is_some());
        let plain_fg = plain.iter().find(|c| c.col == 0).expect("a hücresi").fg;

        session.set_selection(at(0, 0, CellHalf::Left), at(4, 0, CellHalf::Right));
        let mut next = Vec::new();
        assert!(frame_if_damaged(&session, |c| next.push(c)).is_some());
        for col in 0..5 {
            let cell = next
                .iter()
                .find(|c| c.col == col && c.row == 0)
                .expect("hücre");
            assert_eq!(
                cell.bg,
                Some(plain_fg),
                "sütun {col} vurgulanmalı: {cell:?}"
            );
        }
    }

    #[test]
    fn selection_change_marks_dirty() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));
        // Açılış karesi + sessizlik: shell çıktı üretmiyor.
        assert!(frame_if_damaged(&session, |_| ()).is_some());
        assert!(frame_if_damaged(&session, |_| ()).is_none());

        session.set_selection(at(0, 0, CellHalf::Left), at(2, 0, CellHalf::Right));
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
    fn selected_cells_are_inverted_through_existing_pipe() {
        // Vurgu `cell_bg` borusundan ters video ile geçiyor. Reçetede yalnız
        // seçili aralık bg'li (`\033[41mell\033[0m`); `h` ile `o` varsayılan
        // bg'li, yani vurgusuz karede `bg: None` taşıyor. İddia iki yönlü:
        // seçili hücrede boya **beliriyor**, seçimsiz hücrede boya **yok**.
        //
        // Boyanın rengi takasın görünen yüzü **değil**: seçili hücrenin bg'si
        // kendi fg'sinden değil, ters video kuralının çözdüğü renkten gelir
        // (`resolve(if inverse { cell.fg } …)` — alacritty hücrenin `fg`'si
        // `Named(Foreground)`, çözüm paletin ön planıdır). Rengi soran, boruyu
        // değil paleti sorar; boruyu soran boyanın varlığı ile yokluğudur.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session(
            "printf 'h\\033[41mell\\033[0mo'; sleep 5",
            Arc::clone(&wake),
        );

        // Metin grid'e indi: beş glyph'li kare (`hello` — `h` ve `o`
        // mürekkepli ama boyasız, `frame()` onları mürekkep için geçiriyor).
        // Üçlük bg çapası burada **yanlış**: `h`/`o` bg'siz diye üçlük karede
        // yoktur sanıyorduk, ama kare mürekkebi de taşıyor. Mürekkep sayısına
        // bağlanan çapa hem erken-dönüşü hem bölünmüş PTY okumasını kapatıyor.
        let cells = wait_frame(&session, &wake, |cells| {
            cells.iter().filter_map(|c| c.ch).collect::<String>() == "hello"
        });
        assert_eq!(backgrounds(&cells).count(), 3, "{cells:?}");
        session.set_selection(at(1, 0, CellHalf::Left), at(3, 0, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("ell"));

        let mut next = Vec::new();
        assert!(frame_if_damaged(&session, |c| next.push(c)).is_some());
        // Seçili üç hücre boyalı, seçili olmayan iki hücre boyasız. `h`
        // vurgusuz karede eleniyordu (`bg: None, ch: Some` — `ch`'si var ama
        // bu döngü bg'ye bakıyor); seçim onu karesine sokmaz, sokmamalı.
        let painted: Vec<u16> = next
            .iter()
            .filter(|c| c.bg.is_some())
            .map(|c| c.col)
            .collect();
        assert_eq!(painted, vec![1, 2, 3], "{next:?}");
        // Seçimsiz `o` boyanın yokluğuyla duruyor.
        let outside = next.iter().find(|c| c.col == 4).expect("o hücresi");
        assert_eq!(outside.bg, None, "{outside:?}");
    }

    #[test]
    fn selected_inverse_cell_is_drawn_in_normal_colors() {
        // Seçim ters videoyu **çevirir**: seçili ters videolu hücre normal
        // renkleriyle çizilir (gerekçesi `frame()`'in `inverse` yorumunda).
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
        let red = THEME.default(1);
        let green = color::linear_rgba(THEME.default(2));
        let colors = |session: &Session| {
            let mut cells = Vec::new();
            assert!(frame_if_damaged(session, |c| cells.push(c)).is_some());
            cells
                .iter()
                .map(|c| (c.col, (c.bg, c.fg)))
                .collect::<HashMap<_, _>>()
        };

        // 1. ve 2. sütun seçili: biri düz, biri sönük ters video.
        session.set_selection(at(1, 0, CellHalf::Left), at(2, 0, CellHalf::Right));
        let drawn = colors(&session);
        // Seçilmemiş ters video: renkler takaslı.
        assert_eq!(
            drawn[&0],
            (Some(color::linear_rgba(red)), green),
            "{drawn:?}"
        );
        // Seçili ters video: takas geri alınmış, yani hücrenin kendi renkleri.
        assert_eq!(
            drawn[&1],
            (Some(green), color::linear_rgba(red)),
            "{drawn:?}"
        );
        // `DIM` kuralı çevirmeden sonra da aynı: sönüklük `cell.fg`'den doğan
        // renge gider. Seçilmemişte o renk arka plan, seçilide yine ön plan.
        // Elle yazılı: `0xd16d6a`'nın zemine karışmış sönüğü (bkz.
        // `dim_colors_on_the_draw_path_are_pinned`).
        let dim_red = LinearRgba::from_srgb(0x8b, 0x48, 0x46);
        assert_eq!(drawn[&2], (Some(green), dim_red), "{drawn:?}");
        assert_eq!(drawn[&3], (Some(dim_red), green), "{drawn:?}");

        // Varsayılan renkli ters video boşluk seçilince çizilmeyen hücreye
        // döner: arka planı varsayılan, mürekkebi ve kuralı yok. Atlama
        // koşulu onu elemeli — ekranın zeminiyle aynı renkte bir hücreyi
        // `sink`'e sokmak `hucre=` sayısını şişirirdi.
        assert!(drawn[&4].0.is_some(), "seçilmemiş boşluk boyalı: {drawn:?}");
        session.set_selection(at(4, 0, CellHalf::Left), at(4, 0, CellHalf::Right));
        let drawn = colors(&session);
        assert!(!drawn.contains_key(&4), "seçili boşluk çizildi: {drawn:?}");
        assert_eq!(
            drawn[&1],
            (Some(color::linear_rgba(red)), green),
            "{drawn:?}"
        );
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
        session.set_selection(at(0, 0, CellHalf::Left), at(10, 2, CellHalf::Right));
        let mut cells = Vec::new();
        assert!(frame_if_damaged(&session, |c| cells.push(c)).is_some());
        let drawn: Vec<_> = cells.iter().map(|c| (c.row, c.col)).collect();
        assert_eq!(drawn, vec![(0, 0), (0, 1)], "boş hücre boyandı: {cells:?}");
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

        session.set_selection(at(0, 0, CellHalf::Left), at(1, 0, CellHalf::Right));
        let mut cells = Vec::new();
        assert!(frame_if_damaged(&session, |c| cells.push(c)).is_some());
        let cols: Vec<_> = cells.iter().map(|c| c.col).collect();
        assert_eq!(
            cols,
            vec![0, 1],
            "geniş karakterin yarısı vurgusuz: {cells:?}"
        );
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
        // bg'li. Ters videoda vurgulu hücrenin bg'si seçimsiz karenin fg'si.
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("printf '\\033[41mあb\\033[0m'; sleep 5", Arc::clone(&wake));
        let cells = wait_cells(&session, &wake, 3);
        let plain_fg = cells
            .iter()
            .find(|c| c.ch == Some('b'))
            .expect("b hücresi")
            .fg;
        let highlighted = |session: &Session, col: u16| {
            let mut next = Vec::new();
            frame_if_damaged(session, |c| next.push(c));
            next.iter()
                .find(|c| c.col == col)
                .is_some_and(|c| c.bg == Some(plain_fg))
        };

        // Bitiş ucu glyph'in sağ yarısında (spacer'ın sol yarısı): harf
        // içeride, **iki** hücresi de vurgulu.
        session.set_selection(at(0, 0, CellHalf::Left), at(1, 0, CellHalf::Left));
        assert_eq!(session.selection_text().as_deref(), Some("あ"));
        assert!(highlighted(&session, 1), "spacer vurgulanmalı");

        // Bitiş ucu glyph'in sol yarısında (baş hücrenin sağ yarısı): harf
        // dışarıda, seçim boş.
        session.set_selection(at(0, 0, CellHalf::Left), at(0, 0, CellHalf::Right));
        assert_eq!(session.selection_text(), None);

        // Başlangıç ucu glyph'in sağ yarısında: harf dışarıda, baş hücre
        // vurgusuz — metin ile vurgu aynı kararı veriyor.
        session.set_selection(at(1, 0, CellHalf::Left), at(2, 0, CellHalf::Right));
        assert_eq!(session.selection_text().as_deref(), Some("b"));
        assert!(!highlighted(&session, 0), "baş hücre vurgulanmamalı");

        // Başlangıç ucu glyph'in sol yarısında: harf içeride.
        session.set_selection(at(0, 0, CellHalf::Right), at(2, 0, CellHalf::Right));
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
        session.scroll_wheel(lines, at(0, 0, CellHalf::Left), false)
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
        session.frame(|_| (), |_| (), &mut Blocks::default())
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
        let cursor = session.frame(|_| (), |cell| cells.push(cell), &mut Blocks::default());
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
        );
        let origin = cursor.rows - cursor.content_rows;
        let band_top = origin - cursor.fill;
        let rows = (0..cursor.rows)
            .map(|screen| {
                if screen >= origin {
                    row_text(&grid, screen - origin)
                } else if screen >= band_top {
                    row_text(&band, screen - band_top)
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
        let cursor = session.frame(|_| (), |cell| band.push(cell), &mut blocks);
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
            session.scroll_wheel(-1, at(0, 0, CellHalf::Left), true),
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
        assert_eq!(session.scroll_wheel(2, pointer, false), Wheel::Sent);
        expect_sent(&session, &wake, &b"\x1b[<64;5;3M".repeat(2));
        assert_eq!(session.scroll_wheel(-1, pointer, true), Wheel::Sent);
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
            plain.scroll_wheel(1, at(223, 2, CellHalf::Left), false),
            Wheel::Ignored
        );
        expect_sent(&plain, &wake, b"");
        assert_eq!(
            plain.scroll_wheel(1, at(222, 2, CellHalf::Left), false),
            Wheel::Sent
        );
        expect_sent(&plain, &wake, &[0x1b, b'[', b'M', 96, 255, 35]);

        // UTF-8 kodlama (1005): 95. sütundan itibaren koordinat iki bayt.
        let (utf8, wake) = dump_session(224, "printf '\\033[?1000h\\033[?1005h'", |mode| {
            mode.contains(TermMode::UTF8_MOUSE)
        });
        assert_eq!(
            utf8.scroll_wheel(1, at(94, 2, CellHalf::Left), false),
            Wheel::Sent
        );
        assert_eq!(
            utf8.scroll_wheel(-1, at(95, 2, CellHalf::Left), false),
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
            session.scroll_wheel(1, at(0, 4, CellHalf::Left), false),
            Wheel::Ignored
        );
        expect_sent(&session, &wake, b"");
        // Yeniden 5 geri (dolgu `write`'ı dibe döndürdü): görünen 7. satır
        // uygulamanın 2. satırı → rapor 3 der.
        session.term.lock().scroll_display(Scroll::Delta(5));
        assert_eq!(
            session.scroll_wheel(1, at(0, 7, CellHalf::Left), false),
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
            shifted.scroll_wheel(3, at(0, 0, CellHalf::Left), true),
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
            session.set_selection(at(0, 0, CellHalf::Left), at(4, 0, CellHalf::Right));
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
        session.set_selection(at(2, 0, CellHalf::Right), at(2, 0, CellHalf::Right));
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
        history.set_selection(at(0, 0, CellHalf::Left), at(1, 0, CellHalf::Right));
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
        history.set_selection(at(0, 0, CellHalf::Left), at(1, 0, CellHalf::Right));
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
        reports.set_selection(start, end);
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
        arrows.set_selection(start, end);
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
        session.set_selection(at(0, 0, CellHalf::Left), at(4, 0, CellHalf::Right));
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
        session.set_selection(press, press);
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
                        &mut dock,
                        &mut context,
                        cursor.caret_in_dock,
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
}
