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
//! sorusu `term` tutulurken okur, `set_theme` tek başına yazar; `shell`'i de
//! okuyucu thread'i tek başına yazar, `shell_state` tek başına okur.

use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionRange, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Osc52 as TermOsc52, RenderableContent, Term};
// `EventedReadWrite` ada geliyor çünkü `io::Read` gövdesi `Pty::reader()`'ı
// çağırıyor, yani kendi impl bloğunun dışından; `EventedPty` ve `io::Read`
// gelmiyor, onların tek çağrı yeri kendi impl blokları.
use alacritty_terminal::tty::{self, EventedReadWrite as _, Pty, Shell};
use alacritty_terminal::vte::ansi::CursorShape;
// `Event` adı bu modülde alacritty'nin olayına ait; `polling`'inki `TappedPty`
// dışında hiç geçmediği için ada gelen o, takma alan o.
use polling::{Event as PollingEvent, PollMode, Poller};

use crate::color::{self, LinearRgba, Theme};
use crate::input::{self, Arrow, WHEEL_DOWN, WHEEL_UP, WheelRoute};
use crate::shell::{Scanner, ShellState};
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
    /// Bu alan, yukarıdaki "konuma güvenen ilk tüketici sözleşmeyi
    /// genişletmeli" cümlesinin karşılığıdır; genişleten tüketici hareket
    /// oldu, IME değil.
    pub display_offset: i32,
}

/// Oturumun açılış ayarları.
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// `None` → kullanıcının `$SHELL`'i login kabuk olarak; `Some((program,
    /// args))` → tam olarak o komut (duman ve sınamalar bunu kullanır ki
    /// sonuç kullanıcının rc dosyasına bağlı olmasın).
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
/// kıpırdatır.** `\033[H` hiçbir hücre yazmıyor — sekiz arka plan, altı glyph
/// ve on beş kural bit bit yerinde — ama imleci `\n`'in bıraktığı satırdan
/// ekranın başına alıyor, yani her koşuda **bir** imleç hareketi doğuyor.
/// Kapının `hareket > 0` gerekliliği buna dayanıyor (008 Karar 8): onsuz
/// imleç yalnız çıktının kendisiyle oynardı ve ilk karenin çıktıdan önce mi
/// sonra mı düştüğü koşudan koşuya değişiyor (`kare=1↔2`), yani kapı
/// animasyonun koştuğunu göremeyebilirdi.
///
/// **Aradaki uyku cömert (1 s) ve bu bir pay değil, kapının şartı.** Açılış
/// süresi (`acilis=`) ölçülmedi; `\033[H` ilk içerik karesinden **önce**
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
             sleep 1; printf '\\033[H'; sleep 10"
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
}

impl Adapter {
    fn new(wake: Arc<dyn Wake>, size: WindowSize, theme: Theme) -> Self {
        Self(Arc::new(AdapterInner {
            wake,
            sender: OnceLock::new(),
            // Açılış karesi: pencere ilk kez boyansın diye kirli başlar.
            dirty: Arc::new(AtomicBool::new(true)),
            size: Mutex::new(size),
            theme: Mutex::new(theme),
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
    shell: Arc<Mutex<Option<ShellState>>>,
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
        self.scanner.feed(&buf[..read], |mark| {
            ShellState::apply(&mut lock(&self.shell), mark);
        });
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
    /// Birincil ekran: görünen pencere `n` satır kaydı; geçmişin iki ucunda `0`.
    Scrolled(i32),
    /// Uygulamaya gitti: fare kipinde tekerlek raporu, alternate screen'de ok.
    Sent,
    /// Hiçbir şey gitmedi: alternate screen'de ok kapalı (`\e[?1007l`) ya da
    /// Shift basılı; fare kipinde işaretçi geçmişte ya da koordinat
    /// kodlamaya sığmıyor; uygulama yolunda sıfır satır.
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
    /// Kabuğun OSC 133 ile bildirdiği durum; yokluğu "entegrasyon yok" demek.
    ///
    /// **Yaprak kilit** (`theme` emsali): tutulurken başka kilit alınmaz ve
    /// tutan taraf yalnız kopyalar — [`ShellState`] `Copy` ve küçük. Yazanı
    /// okuyucu thread'i, okuyanı [`Session::shell_state`].
    ///
    /// `Adapter`'da değil `Session`'da, çünkü `Adapter` alacritty'nin
    /// olaylarını karşılıyor ve bu duruma **hiç** dokunmuyor; işaretler
    /// olaylardan değil ham bayt akışından geliyor. `Arc`, çünkü aynı yuvanın
    /// öteki ucu [`TappedPty`] ile okuyucu thread'inde.
    shell: Arc<Mutex<Option<ShellState>>>,
}

impl Session {
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
        let shell = Arc::new(Mutex::new(None));
        let pty = TappedPty {
            pty,
            scanner: Scanner::new(),
            shell: Arc::clone(&shell),
        };

        let adapter = Adapter::new(wake, size, options.theme);
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
    pub fn frame(&self, mut sink: impl FnMut(Cell)) -> Cursor {
        // Tema `Term` kilidinden **önce** ve kopya olarak: yaprak kilit
        // kare boyunca tutulmaz, `Term` kilidinin altına ikinci bir muteks
        // girmez. Kopya ile kilit arasına düşen bir takas en çok bir kare
        // eski renkle çizer; takası yazan zaten kare istiyor.
        let theme = *lock(&self.adapter.0.theme);
        let background = theme.background_rgb();
        let term = self.term.lock();

        let rows = term.screen_lines() as i32;
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
        let cursor_point = cursor.point;
        let cursor_row = cursor.point.line.0 + offset;
        let cursor = Cursor {
            col: cursor.point.column.0 as u16,
            row: cursor_row.clamp(0, rows.saturating_sub(1)) as u16,
            // Kaydırma geçmişine bakarken imleç ekranın dışına çıkar.
            visible: cursor.shape != CursorShape::Hidden && (0..rows).contains(&cursor_row),
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
        };

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
            let selected = !hidden
                && selected_range
                    .as_ref()
                    .is_some_and(|range| range.contains_cell(&indexed, cursor_point, cursor_shape));
            let inverse = flags.contains(Flags::INVERSE) ^ selected;

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
            let back = if inverse {
                color::resolve_fg(cell.fg, dim, colors, &theme)
            } else {
                color::resolve(cell.bg, colors, &theme)
            };
            // Varsayılan arka plan çizilmez; `None` onun adı.
            let bg = (back != background).then(|| color::linear_rgba(back));

            // `HIDDEN` (`\e[8m`) "mürekkep yok" demek ve **tek bir `let`**
            // (yukarıdaki `hidden`): hem glyph'i hem kuralları düşürüyor, hem
            // de seçim vurgusunu dışlıyor. Üç ayrı ifadeye yazılsaydı biri
            // sonradan değişip ötekiler eski kalabilirdi ve belirti "gizli
            // metin altı çizgisinden/vurgusundan okunuyor" olurdu.
            let ch = (!hidden && !flags.intersects(SPACERS) && cell.c != ' ').then_some(cell.c);
            // Kapının kural yarısı tek maske testi; **hangi** çeşit olduğu
            // kapıdan sonra sorulur (aşağıda). `!hidden` maskenin dışında
            // değil içinde: dışarıda kalsaydı gizli ve altı çizili bir hücre
            // kapıdan geçer, aşağıda `None`'a çözülür ve `sink`'e çizilecek
            // hiçbir şeyi olmadan varırdı.
            let ruled = !hidden && flags.intersects(RULES);

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
            // Ön plan **ancak burada** çözülüyor — atlama kapısından sonra.
            // Kapıdan önce olsaydı `Term` kilidi tutulurken çizilmeyen her
            // hücre için de ödenirdi ve boş grid'de hücrelerin neredeyse
            // tamamı çizilmiyor. Koşulsuz: alan adının söylediği şey olmalı,
            // yoksa kural çizgisi mürekkepsiz bir hücrede arka plan rengiyle
            // çizilir, yani görünmez olurdu.
            let fore = if inverse {
                color::resolve(cell.bg, colors, &theme)
            } else {
                color::resolve_fg(cell.fg, dim, colors, &theme)
            };
            // **Beş bayrak ayrı ayrı sorulur ve kıvrımlı önce gelir.**
            // `UNDERCURL` `UNDERLINE`'ı **içermez**: `Attr::Undercurl` önce
            // `ALL_UNDERLINES`'ı siliyor, sonra yalnız kendini ekliyor
            // (alacritty `term/mod.rs`, beş kolun beşi de öyle). Refleksle
            // yazılmış tek bir `contains(UNDERLINE)` bu setin varlık sebebi
            // olan dalgalı çizgiyi sessizce düz çizgiye indirirdi ve hiçbir
            // sayaç bunu göremezdi — `undercurl_text_yields_curl` görüyor.
            //
            // `ruled` yanlışsa hiç sorulmuyor: gizli hücre de, hiç kuralı
            // olmayan hücre de tek testte eleniyor. Geniş karakterin ikinci
            // hücresi bayrakları şablondan kopyaladığı için kural iki hücreye
            // kendiliğinden yayılıyor — bedava, ama "neden çalışıyor"
            // sorusunun cevabı burası.
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
            let strikeout = ruled && flags.contains(Flags::STRIKEOUT);
            // SGR 58 (`CellExtra`'da yaşıyor) ön planla **aynı kapıdan
            // sonra**, aynı gerekçeyle: yan tabloya inmenin ve palete
            // bakmanın bedeli çizilmeyen hücreler için ödenmesin — `extra`
            // yalnız alt çizgi rengi için değil, sıfır genişlikli birleşik
            // karakter ve hyperlink için de doluyor.
            //
            // Kapı `ruled` değil **alt çizginin kendisi**: adı "alt çizgi
            // rengi" ve SGR'de üstü çizilinin ayrı bir rengi yok. `ruled`
            // olsaydı yalnız üstü çizili bir hücre (`\e[9;58;5;196m`) rengi
            // taşırdı ve onu okuyan çizici üstü çiziliyi kırmızıya boyardı.
            // `None` → çizen taraf `fg`'yi kullanır; `bg` ile birebir aynı
            // örüntü ve alacritty'nin `Color`'ı `pub` API'ye sızmıyor.
            let underline_color = (underline != UnderlineStyle::None)
                .then(|| cell.underline_color())
                .flatten()
                .map(|c| color::linear_rgba(color::resolve(c, colors, &theme)));

            let col = indexed.point.column.0 as u16;
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
                fg: color::linear_rgba(fore),
                bg,
                // `Flags::BOLD_ITALIC` ikisinin birleşimi, ayrı bir bit
                // değil: `contains` her iki soruyu da doğru yanıtlıyor.
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline,
                underline_color,
                strikeout,
            });
        }

        cursor
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
                let moved = scroll_locked(&mut term, lines);
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
                    .and_then(|row| input::wheel_report(encoding, button, at.col, row));
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
            scroll_locked(&mut term, lines)
        };
        self.wake_if_moved(moved);
        moved
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
    /// `Term` kilidine dokunmaz, `frame()`'in `sink`'inden de çağrılabilir —
    /// ve `frame()` imzası bu yüzden değişmedi.
    pub fn shell_state(&self) -> Option<ShellState> {
        *lock(&self.shell)
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
        self.term.lock().set_options(term_config(options));
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
            (bytes, cleared, scroll_locked(&mut term, i32::MIN))
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
        if self.bracketed_paste() {
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
/// orada kalıyor; kaydırılacak bir şey yok. `Some(n)` → pencere `n` satır
/// kaydı, geçmişin iki ucunda `0`.
fn scroll_locked<T: EventListener>(term: &mut Term<T>, lines: i32) -> Option<i32> {
    if term.mode().contains(TermMode::ALT_SCREEN) {
        return None;
    }
    let before = term.grid().display_offset() as i32;
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
    use crate::shell::ShellPhase;

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
    fn frame_if_damaged(session: &Session, sink: impl FnMut(Cell)) -> Option<Cursor> {
        session.take_damage().then(|| session.frame(sink))
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
            },
            theme: THEME,
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
        // `0xd16d6a`, zemin `0x1a1c21`'e doğru üçte bir: kanal başına
        // `(2·kaynak + zemin) / 3`, kesmeyle. 007 phase-3'e kadar `× 2/3`'tü
        // (`0x8b4846`); değişikliğin tek izi bu satır.
        let b = at_col(2);
        assert_eq!(
            (b.fg, b.bg),
            (LinearRgba::from_srgb(0x94, 0x52, 0x51), None),
            "{b:?}"
        );
        // Ters videoda sönük ön plan arka plana geçer; ön plan paletin arka
        // planı ve **sönmez**.
        let c = at_col(4);
        assert_eq!(
            (c.fg, c.bg),
            (
                LinearRgba::from_srgb(0x1a, 0x1c, 0x21),
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
        assert_eq!(cells[0].bg, Some(LinearRgba::from_srgb(0x94, 0x52, 0x51)));
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
            "printf 'a\\033[48;2;26;28;33mb\\033[0m'; sleep 5",
            Arc::clone(&wake),
        );
        let cells = wait_frame(&session, &wake, |cells| glyph_text(cells) == "ab");
        let dark_bg = LinearRgba::from_srgb(0x1a, 0x1c, 0x21);
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

        // İki alanın dışında hiçbir şey kurulmuyor.
        for config in [before, scrolled, copying] {
            assert_eq!(
                Config {
                    scrolling_history: Config::default().scrolling_history,
                    osc52: Config::default().osc52,
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
        let dim_red = LinearRgba::from_srgb(0x94, 0x52, 0x51);
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
                    session.set_terminal_options(TerminalOptions { scrollback, osc52 });
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
        // `shell_state()` ile yalnız `shell`'i, `frame()` ile yalnız `Term`'ü
        // alıyor. İkisi ters sırada kilitlenirse bu sınama **asılı kalır** —
        // yaprak kilit iddiasının (modül başlığı) tek mekanik bekçisi bu.
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
}
