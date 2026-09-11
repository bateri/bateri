//! Bir terminal oturumu: PTY, okuyucu thread ve grid.
//!
//! Crate'in kapsül sözleşmesi `lib.rs`'te; burada onun iki pratik sonucu
//! yaşıyor: alacritty'nin `EventListener`'ı `Adapter`'da bizim `Wake`'imize
//! çevrilir, ve `Term` kilidi yalnız `frame()` ile `resize()`'da alınır —
//! üçüncü bir alan yeri yoktur, kilit sırası oradan okunur.

use std::collections::HashMap;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::JoinHandle;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg, State};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, RenderableContent, Term};
use alacritty_terminal::tty::{self, Pty, Shell};
use alacritty_terminal::vte::ansi::CursorShape;

use crate::color::{self, LinearRgba};
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
    /// İmlecin altındaki hücrede **ters**: değer paletin arka planıdır, çünkü
    /// imleç bloğu opak ve glyph'in altında (`plan.md` → R4.1).
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
    /// **İmlecin altındaki hücrede her zaman `None`**: [`Cell::fg`] orada
    /// tersine döndüğü için çizgi de onunla birlikte dönsün. Ayrıntısı
    /// [`Session::frame`]'in imleç dalında.
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

/// İmlecin karedeki yeri.
///
/// `row` her zaman görünür pencereye kırpılıdır. Kaydırma geçmişine bakarken
/// imleç ekranın dışına çıkar; o durumda `visible` kapanır ve `row` gerçek
/// satırı değil kırpılmış değeri taşır — konuma güvenen bir tüketici
/// (kaydırma, IME) çıkmadan önce buranın sözleşmesini genişletmeli.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
}

/// Oturumun açılış ayarları.
#[derive(Clone, Debug)]
pub struct SessionOptions {
    /// `None` → kullanıcının `$SHELL`'i login kabuk olarak; `Some((program,
    /// args))` → tam olarak o komut (duman ve sınamalar bunu kullanır ki
    /// sonuç kullanıcının rc dosyasına bağlı olmasın).
    pub command: Option<(String, Vec<String>)>,
    pub cols: u16,
    pub rows: u16,
    /// Bir hücrenin piksel boyutu; PTY'ye `TIOCSWINSZ` ile gider, grafik
    /// uygulamaları (sixel, kitty) bunu okur.
    pub cell_px: (u16, u16),
    pub scrollback: usize,
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
/// olduğu için ikinci bir yük **buraya eklenmez**. Süre parametresi, ikinci
/// bir `printf`, "bir de şu kadar satır bas" — hiçbiri; her biri sekiz
/// hücreyi, altı glyph'i ya da on beş kuralı oynatır ve oynattığında üç
/// sınama ile `make duman` aynı anda ama ayrı ayrı yalan söyler.
pub fn smoke_shell() -> (String, Vec<String>) {
    (
        "/bin/sh".to_owned(),
        // Kaçışları printf çözer: Rust dizgisinde `\033` ilk baytı NUL yapardı.
        vec![
            "-c".to_owned(),
            "printf '\\033[41;1;4m bateri \\033[0m\\033[4m \\033[0;4:2m \\033[0;4:3m \
             \\033[0;4:4m \\033[0;4:5m \\033[0;9m \\033[0;4:3;58;5;196m \\033[0m\\n'; \
             sleep 10"
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
/// Viewport kaydırma **yok** (tekerlek işleyicisi yok), o yüzden kaydırılan
/// şey viewport değil **içerik**: her satır kirli düşer, grid yukarı kayar,
/// kare akışı kendiliğinden sürer. Ölçtüğümüz şey zaten bu — dolu bir karede
/// parse + [`Session::frame`] + encode + GPU maliyeti.
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
    dirty: Arc<AtomicBool>,
    /// PTY'nin bildiği son boyut; `TextAreaSizeRequest` bunu yanıtlar.
    size: Mutex<WindowSize>,
}

impl Adapter {
    fn new(wake: Arc<dyn Wake>, size: WindowSize) -> Self {
        Self(Arc::new(AdapterInner {
            wake,
            sender: OnceLock::new(),
            // Açılış karesi: pencere ilk kez boyansın diye kirli başlar.
            dirty: Arc::new(AtomicBool::new(true)),
            size: Mutex::new(size),
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
            // değil, `try_lock` da aynı thread'de hep düşer). Paletin
            // varsayılanıyla yanıtlıyoruz.
            //
            // **Bilinen sınır:** uygulama OSC 4/10/11 ile bir rengi
            // değiştirip sonra sorarsa eski değeri alır — "önce ata, sonra
            // sor" yaygın bir örüntüdür (arka planı okuyup açık/koyu tema
            // seçen editörler). Çizim yolu tabloyu doğru okuyor, yalnız
            // yanıt yolu okumuyor; ikisi ayrışıyor. Gerçek çözüm paletin
            // sahipliğinin alacritty'den bize geçmesi, yani 00X tema seti.
            Event::ColorRequest(index, format) => self.reply(format(color::default(index))),
            Event::TextAreaSizeRequest(format) => {
                let size = *lock(&self.0.size);
                self.reply(format(size));
            }
            // Başlık, zil ve pano bu sette yok; bilinmeyen dizi gibi sessizce
            // düşerler (`CLAUDE.md` → PTY yolunda panik yok). "Yoksayılır ve
            // LOGLANIR" kuralının ikinci yarısı borç: `tracing` henüz
            // bağımlılık değil, workspace'te hiçbir logger yok — alacritty'nin
            // kendi `log::error!` satırları da bu yüzden yere düşüyor.
            Event::Title(_)
            | Event::ResetTitle
            | Event::Bell
            | Event::ClipboardStore(..)
            | Event::ClipboardLoad(..)
            | Event::MouseCursorDirty
            | Event::CursorBlinkingChange
            | Event::Exit => {}
        }
    }
}

/// Okuyucu thread'in tutamağı. `join()` döngüyü ve PTY'yi geri verir;
/// `SIGHUP` bu ikilinin düşmesiyle gider.
type Reader = JoinHandle<(EventLoop<Pty, Adapter>, State)>;

/// PTY'si, okuyucu thread'i ve grid'i olan bir terminal oturumu.
pub struct Session {
    term: Arc<FairMutex<Term<Adapter>>>,
    sender: EventLoopSender,
    adapter: Adapter,
    /// Okuyucu thread; `shutdown()` alır, `Drop` de çağırır. `Option`
    /// "kapandı" demenin ve iki kez join etmemenin yoludur.
    reader: Mutex<Option<Reader>>,
}

impl Session {
    /// PTY'yi açar, shell'i başlatır ve okuyucu thread'i kurar.
    pub fn spawn(options: SessionOptions, wake: Arc<dyn Wake>) -> io::Result<Self> {
        let grid = GridSize::for_spawn(options.cols, options.rows);
        let size = window_size(grid, options.cell_px);

        let pty_options = tty::Options {
            shell: options
                .command
                .map(|(program, args)| Shell::new(program, args)),
            // `tty::setup_env()` ÇAĞRILMAZ: o, kendi sürecimizin ortamını
            // `set_var` ile değiştirir ve makinede alacritty kuruluysa
            // `TERM=alacritty` yazar. Ortamı çocuğa doğrudan veriyoruz.
            env: HashMap::from([
                ("TERM".to_owned(), "xterm-256color".to_owned()),
                ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ]),
            ..Default::default()
        };
        let pty = tty::new(&pty_options, size, 0)?;

        let adapter = Adapter::new(wake, size);
        let config = Config {
            scrolling_history: options.scrollback,
            ..Config::default()
        };
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
        })
    }

    /// Çizilecek kareyi verir: yeni içerik yoksa `None` ve **hiç iterasyon**.
    ///
    /// Kilit bir kez alınır. Hasar "çizilsin mi"ye karar verir, "ne
    /// çizileceğine" değil: drawable içeriği korunmadığı için her karede tam
    /// grid taranır.
    ///
    /// `sink` jeneriktir: hücre başına dinamik çağrı yerine satır içine
    /// alınır. **`Term` kilidi tutulurken** çağrılır ve kilit yeniden girilebilir
    /// değildir: `Session`'a geri giren bir sink (`resize`, `frame`) kendi
    /// kendini kilitler. Sink'in işi tamponu doldurmaktır, başka bir şey değil —
    /// `Wake` ile aynı sözleşme.
    pub fn frame(&self, mut sink: impl FnMut(Cell)) -> Option<Cursor> {
        // Bayrak kilit istemez, kilit ise ucuz değil: `FairMutex::lock()` iki
        // muteks alır ve okuyucu thread PTY'den okumaya başlamadan önce
        // aynı sıraya giriyor. Boştaki kare o sıraya hiç girmesin.
        // Swap ile lock arasına düşen bir `Wakeup` bayrağı yeniden diker;
        // en kötüsü fazladan bir kare, kaçan kare değil.
        if !self.adapter.0.dirty.swap(false, Ordering::AcqRel) {
            return None;
        }
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

        // İmleç döngüden **önce** çözülüyor: altındaki hücrenin ön planı ona
        // bağlı (aşağıda) ve o karar hücre çizilirken verilmek zorunda.
        let cursor_row = cursor.point.line.0 + offset;
        let cursor = Cursor {
            col: cursor.point.column.0 as u16,
            row: cursor_row.clamp(0, rows.saturating_sub(1)) as u16,
            // Kaydırma geçmişine bakarken imleç ekranın dışına çıkar.
            visible: cursor.shape != CursorShape::Hidden && (0..rows).contains(&cursor_row),
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

        for indexed in display_iter {
            let cell = indexed.cell;
            let flags = cell.flags;
            let inverse = flags.contains(Flags::INVERSE);
            let dim = flags.contains(Flags::DIM);

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
            // ters videoda o renk arka plan olmuştur.
            let mut back = color::resolve(if inverse { cell.fg } else { cell.bg }, colors);
            if inverse && dim {
                back = color::dim(back);
            }
            // Varsayılan arka plan çizilmez; `None` onun adı.
            let bg = (back != color::BG_RGB).then(|| color::linear_rgba(back));

            // `HIDDEN` (`\e[8m`) "mürekkep yok" demek ve **tek bir `let`**:
            // hem glyph'i hem kuralları düşürüyor. İki ayrı ifadeye
            // yazılsaydı biri sonradan değişip öteki eski kalabilirdi ve
            // belirti "gizli metin altı çizgisinden okunuyor" olurdu.
            let hidden = flags.contains(Flags::HIDDEN);
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
            let mut fore = color::resolve(if inverse { cell.bg } else { cell.fg }, colors);
            if !inverse && dim {
                fore = color::dim(fore);
            }
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
                .map(|c| color::linear_rgba(color::resolve(c, colors)));

            let col = indexed.point.column.0 as u16;
            // İmlecin altındaki hücre **ters** çiziliyor. İmleç bloğu opak ve
            // glyph'lerin altında (`plan.md` → R4.1); harf kendi ön planıyla
            // kalsaydı açık gri, açık mavi bloğun üstüne düşer ve okunmazdı.
            // Kararı `bt-core` veriyor çünkü kararın adı terminal
            // semantiğidir; `bt-gpu`'nun bileceği bir şey değil.
            // **Alt çizgi rengi de tersine dönüyor** — daha doğrusu düşüyor:
            // `None` "çizen taraf `fg`'yi kullansın" demek ve `fg` zaten
            // tersine döndü. Düşmeseydi SGR 58'li bir hücrede imleç bloğunun
            // üstündeki çizgi terminalin seçtiği renkte kalır, üstü çizili ise
            // (`fg`'yi kullanıyor) tersine dönerdi: aynı hücrede iki kural, iki
            // farklı davranış. Rengin imleç bloğuna yakın düştüğü durumda
            // çizgi büsbütün kaybolurdu.
            let (fore, underline_color) =
                if cursor.visible && (col, row) == (cursor.col, cursor.row) {
                    (color::BG_RGB, None)
                } else {
                    (fore, underline_color)
                };
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

        Some(cursor)
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

    /// Klavyeden ya da başka bir kaynaktan PTY'ye bayt akıtır.
    ///
    /// Boş dilim sessizce düşer: sıfır baytlık bir `Msg::Input`
    /// `EventLoop`'un yazıcısını kalıcı olarak kilitler (bkz. `Adapter::reply`).
    pub fn write(&self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.send(Msg::Input(bytes.to_vec().into()));
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
    /// **Bloklar.** `Pty::drop` `SIGHUP`'tan sonra `child.wait()` çağırıyor;
    /// sinyali yutan bir çocuk (`trap '' HUP`) bu çağrıyı süresiz bekletir.
    /// Kesecek olan `bt-shell`'in bekçi thread'i ve o yalnız `BT_RUN_SECONDS`
    /// yolunda kurulur: etkileşimli kullanımda böyle bir çocuk uygulamayı
    /// gerçekten asar (bilinen sınır, `.tasks/002-vt-motoru/phase-4.md`).
    pub fn shutdown(&self) {
        let Some(reader) = lock(&self.reader).take() else {
            return;
        };
        self.send(Msg::Shutdown);
        if reader.join().is_err() {
            eprintln!("bateri: okuyucu thread panikle bitti");
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
        self.shutdown();
    }
}

/// `WindowSize` `PartialEq` türetmiyor; dört alanı elle karşılaştırıyoruz.
fn same_size(a: WindowSize, b: WindowSize) -> bool {
    a.num_cols == b.num_cols
        && a.num_lines == b.num_lines
        && a.cell_width == b.cell_width
        && a.cell_height == b.cell_height
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

    /// Uyandırmaları sayar ve sınamanın beklemesine izin verir.
    #[derive(Default)]
    struct TestWake {
        state: Mutex<(u32, Option<Option<i32>>)>,
        cond: Condvar,
    }

    impl TestWake {
        /// En az `target` uyandırma gelene kadar bekler.
        fn wait_wakes(&self, target: u32, timeout: Duration) -> u32 {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .cond
                .wait_timeout_while(state, timeout, |(count, _)| *count < target)
                .unwrap();
            state.0
        }

        /// Çocuk ölene kadar bekler; zaman aşımında `None`.
        fn wait_exit(&self, timeout: Duration) -> Option<Option<i32>> {
            let state = self.state.lock().unwrap();
            let (state, _) = self
                .cond
                .wait_timeout_while(state, timeout, |(_, code)| code.is_none())
                .unwrap();
            state.1
        }
    }

    impl Wake for TestWake {
        fn wake(&self) {
            self.state.lock().unwrap().0 += 1;
            self.cond.notify_all();
        }

        fn child_exit(&self, code: Option<i32>) {
            self.state.lock().unwrap().1 = Some(code);
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
        spawn_with_command(("/bin/sh".into(), vec!["-c".into(), script.into()]), wake)
    }

    fn spawn_with_command(command: (String, Vec<String>), wake: Arc<TestWake>) -> Session {
        Session::spawn(
            SessionOptions {
                command: Some(command),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                scrollback: 100,
            },
            wake,
        )
        .unwrap()
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
            if session.frame(|c| cells.push(c)).is_some() && ready(&cells) {
                return cells;
            }
        }
    }

    /// Karenin arka plan boyayan hücreleri — `Frame`'in `bg_count`'unun
    /// saydığı küme.
    fn backgrounds(cells: &[Cell]) -> impl Iterator<Item = &Cell> {
        cells.iter().filter(|c| c.bg.is_some())
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
        let red = Some(color::linear_rgba(color::default(1)));
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
        let glyphs: String = cells.iter().filter_map(|c| c.ch).collect();
        assert_eq!(glyphs, "bateri", "{cells:?}");
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
        let red = Some(color::linear_rgba(color::default(196)));
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
    fn cursor_cell_drops_the_underline_color() {
        // İmlecin altındaki hücrede `fg` tersine dönüyor; SGR 58 rengi
        // dönmeseydi aynı hücredeki iki kural iki farklı davranış gösterirdi —
        // üstü çizili `fg`'yi kullandığı için ters, alt çizgi terminalin
        // seçtiği renkte. Rengin imleç bloğuna yakın düştüğü durumda çizgi
        // büsbütün kaybolurdu ve bunu hiçbir sayaç göremezdi.
        let wake = Arc::new(TestWake::default());
        // `\033[D` imleci X'in üstüne geri getiriyor.
        let session = spawn_session(
            "printf '\\033[4;58;5;196mX\\033[0m\\033[D'; sleep 5",
            Arc::clone(&wake),
        );

        // Ölçüt **imlecin varışı**: PTY okuması X ile `\033[D` arasında
        // bölünebilir ve o karede imleç hâlâ bir sağdadır. Hücre listesine
        // bağlanan bir ölçüt o kareyi kabul edip yanlış hücreyi doğrulardı.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = 0;
        let cell = loop {
            assert!(Instant::now() < deadline, "imleç X hücresine dönmedi");
            seen = wake.wait_wakes(seen + 1, Duration::from_millis(500));
            let mut cells = Vec::new();
            let Some(cursor) = session.frame(|c| cells.push(c)) else {
                continue;
            };
            if cursor.visible && (cursor.col, cursor.row) == (0, 0) {
                if let Some(cell) = cells.iter().copied().find(|c| c.ch == Some('X')) {
                    break cell;
                }
            }
        };

        // Kuralın kendisi duruyor — düşen yalnız rengi.
        assert_eq!(cell.underline, UnderlineStyle::Single, "{cell:?}");
        assert_eq!(
            cell.underline_color, None,
            "imleç hücresinde SGR 58 rengi düşmeli: {cell:?}"
        );
        assert_eq!(
            cell.fg,
            color::linear_rgba(color::BG_RGB),
            "imleç hücresinde ön plan tersine dönmeli: {cell:?}"
        );
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
        assert!(session.frame(|_| count += 1).is_none());
        assert_eq!(count, 0, "hasarsız kare sink'i çağırdı");
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
        assert_eq!(
            cells[0].bg,
            Some(color::linear_rgba(color::dim(color::default(1))))
        );
        // Sönük olmayan kırmızıdan gerçekten farklı.
        assert_ne!(cells[0].bg, Some(color::linear_rgba(color::default(1))));
        // Ters videoda ön plan hücrenin arka planından gelir ve **sönmez**:
        // `DIM` yalnız `cell.fg`'den doğan renge uygulanıyor.
        assert_eq!(cells[0].ch, Some('x'));
        assert_eq!(cells[0].fg, color::linear_rgba(color::BG_RGB));
    }

    #[test]
    fn char_under_cursor_is_drawn_inverted() {
        // İmleç bloğu opak ve glyph'in altında; harf kendi ön planıyla
        // kalsaydı açık gri, açık mavi bloğun üstüne düşer ve okunmazdı.
        // `bt-gpu` bunu göremez — imleci ayrı listede, hücreyi ayrı listede
        // çiziyor ve ikisinin çakıştığını bilmiyor.
        let wake = Arc::new(TestWake::default());
        // İmleç yazılan metnin **sonunda** durur; hücreyi imlecin altına
        // sokmak için geri sarıyoruz (`\b`).
        let session = spawn_session(
            "printf '\\033[41mAB\\033[0m\\b\\b'; sleep 5",
            Arc::clone(&wake),
        );

        let cells = wait_cells(&session, &wake, 2);
        let a = cells.iter().find(|c| c.col == 0).expect("ilk hücre");
        let b = cells.iter().find(|c| c.col == 1).expect("ikinci hücre");
        assert_eq!((a.ch, b.ch), (Some('A'), Some('B')), "{cells:?}");
        // İmleç 0. sütunda: oradaki harf arka plan rengine döner, komşusu
        // dönmez. İkisini birden sınamak "hepsini terse çevirdim" hatasını da
        // yakalıyor.
        assert_eq!(a.fg, color::linear_rgba(color::BG_RGB), "{cells:?}");
        assert_ne!(b.fg, color::linear_rgba(color::BG_RGB), "{cells:?}");
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
        let green = Some(color::linear_rgba(color::default(2)));
        assert!(backgrounds(&cells).all(|c| c.bg == green), "{cells:?}");
    }

    #[test]
    fn zero_size_is_ignored() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 5", Arc::clone(&wake));

        // Açılış karesi: grid boş ama pencere bir kez boyanmalı (bayrak
        // `Adapter::new`'da `true` başlıyor).
        assert!(session.frame(|_| ()).is_some());
        assert!(session.frame(|_| ()).is_none());

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
            session.frame(|_| ()).is_none(),
            "dejenere boyut grid'e ulaştı"
        );

        // Gerçek boyut değişimi hasar işaretler.
        assert!(session.resize(80, 24, (9, 18)));
        assert!(session.frame(|_| ()).is_some());
        // Aynı boyut ikinci kez: değişiklik yok, hasar yok.
        assert!(!session.resize(80, 24, (9, 18)));
        assert!(session.frame(|_| ()).is_none());
        // Yalnız hücre piksel boyutu değişse de bu bir değişikliktir: PTY'ye
        // giden `TIOCSWINSZ` onu taşıyor (Retina'ya taşınan pencere).
        assert!(session.resize(80, 24, (18, 36)));
    }

    #[test]
    fn shutdown_ends_the_reader() {
        let wake = Arc::new(TestWake::default());
        let session = spawn_session("sleep 30", Arc::clone(&wake));
        assert!(session.reader_alive());

        let started = Instant::now();
        session.shutdown();
        assert!(!session.reader_alive());
        // `sleep 30` sürerken bile SIGHUP yolu hemen dönmeli.
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        // İkinci çağrı sessizce döner.
        session.shutdown();
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
            if session.frame(|_| ()).is_some() {
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
}
