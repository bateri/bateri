//! Kabuğun bastığı OSC 133 işaretleri ve onların tuttuğu oturum durumu.
//!
//! Bu modül **saf**: elinde ne `Session`, ne `Wake`, ne kilit var. Bayt
//! dilimi girer, işaret çıkar. Tarayıcının hiçbir kare isteyememesi bir kural
//! değil tipin şekli — beslediği yer (`read()`, okuyucu thread'i)
//! `advance()`'ten **önce** koşuyor; kare isteyebilseydi eski ızgara ile yeni
//! durumu aynı karede çizerdi.
//!
//! **Neden kendi tarayıcımız var:** `vte` OSC 133'ü tanımıyor ve `Handler`
//! trait'inde "bilinmeyen OSC" kancası yok, yani `Term`'ü saran bir tip bile
//! bu diziyi göremiyor (`.tasks/009-shell-entegrasyonu/context.md` → Kanıt).
//! Baytları ayrıştırıcıya giderken tarıyoruz.
//!
//! **Çerçeveleme `vte` ile paritelidir** ve bu zorunlu: tarayıcının gördüğü
//! dizi sınırı ile ızgaranın gördüğü aynı olmalı, yoksa iki taraf aynı akıştan
//! iki farklı hikâye okur. `vte-0.15.0/src/lib.rs`'in durum tablosundan
//! çıkan üç kural:
//!
//! - Dizi `ESC ]` ile başlar (`advance_esc`, `0x5D`) — ve `ESC` ile `]`
//!   arasına bayt girebilir: `advance_esc` C0'ların 0x18/0x1A dışındakilerini
//!   `execute` edip **durumu değiştirmiyor**, 0x7F'ten büyük baytları hiç
//!   tanımıyor. Yani `ESC \r ] 133;A BEL` ızgarada geçerli bir işaret.
//! - Diziyi **dört** bayt bitirir: `BEL` (0x07), `CAN` (0x18), `SUB` (0x1A) ve
//!   **çıplak `ESC`** (0x1B). Sonuncusu sürprizdir: `advance_osc_string` ESC'i
//!   görünce diziyi `ESC \`'in `\`'ini beklemeden **hemen** dağıtıyor. Yani
//!   `ESC ] 133;A ESC [ 0 m` de geçerli bir işarettir ve `ESC` her durumdan
//!   `Escape`'e götürdüğü için "bir sonraki ESC'e zıpla" taraması eksiksizdir.
//! - Dizinin içindeki C0 kontrol baytları (0x00–0x06, 0x08–0x17, 0x19,
//!   0x1C–0x1F) **yüke girmez**; `vte` onları sessizce atıyor. Biz de atıyoruz,
//!   yoksa satır sonu yapıştırılmış bir `D;0\r` yükü bizde bozuk görünürdü.

use std::collections::VecDeque;

/// Kabuğun akışa bastığı tek bir OSC 133 işareti.
///
/// Dördü de kabuktan bağımsızdır: tipte ne zsh, ne bash, ne fish geçer
/// (R2.4). Yeni bir kabuk eklemek yalnız bir betik yazmaktır.
///
/// **Kimliği yalnız iki varyant taşır** (`A` ve `D`), çünkü bloğu açan ve
/// kapatan onlar; `B` ile `C` bloğun *içinde* duruyor ve kimlikleri
/// tekrarlamak akışa bayt eklemekten başka bir şey yapmazdı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// `A` — prompt burada başlıyor; blok da burada açılıyor.
    PromptStart { id: Option<u32> },
    /// `B` — prompt bitti, bundan sonrası kullanıcının yazdığı komut.
    PromptEnd,
    /// `C` — komut koşmaya başladı, bundan sonrası çıktı.
    CommandStart,
    /// `D` — komut bitti. Kod **opsiyoneldir**: kabuk `D`'yi çıplak da
    /// basabilir ve okunamayan bir parametre komutun bittiği bilgisini
    /// çürütmez — "bitti ama kodu bilmiyorum" doğru cevaptır. Kimlik de
    /// opsiyonel ve aynı gerekçeyle: kimliksiz bir `D` durumu yine ilerletir,
    /// yalnız deftere yazacak bir yeri yoktur.
    CommandEnd { exit: Option<i32>, id: Option<u32> },
}

/// Oturumun kabuk hakkında bildiği her şey.
///
/// `Copy` ve küçük: [`crate::Session::shell_state`] onu kilidin altından
/// kopyalayarak veriyor ([`crate::Session::theme`] emsali).
///
/// **Seviye `enum`'u yok.** Ürün dilindeki "seviye 0" (entegrasyon yok)
/// kodda bu tipin **yokluğudur** (`Option<ShellState>`); ayrı bir kayıt
/// tutulsaydı SSH'ın öte tarafında ikisi çelişirdi — yerelde entegrasyon
/// kurulu ama uzakta işaret gelmiyor (`discussion.md` → Karar 3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShellState {
    /// Kabuk şu anda ne yapıyor.
    pub phase: ShellPhase,
    /// En son biten komutun çıkış kodu; hiç komut bitmediyse ya da kabuk kodu
    /// okunamayacak şekilde bastıysa `None`.
    pub last_exit: Option<i32>,
}

/// Kabuğun o anki safhası — dört işaretin her birine bir tane.
///
/// `A` ve `B` **birleştirilmedi**: "prompt çiziliyor" ile "kullanıcı yazıyor"
/// arasındaki sınır, Input Dock'un (014) ilk sorusu. Ayrımı burada tutmak
/// bedava, sonradan geri kazanmak değil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellPhase {
    /// `A` — prompt çiziliyor.
    Prompt,
    /// `B` — prompt bitti, kullanıcı komutunu yazıyor.
    Input,
    /// `C` — komut koşuyor, ekrana çıktısı akıyor.
    Running,
    /// `D` — komut bitti, yeni prompt henüz gelmedi. Kodu `last_exit`'te.
    Finished,
}

/// Bir bloğun akıbeti — defterin tuttuğu ham kayıt.
///
/// `Pending`, "koşuyor" **demek değil**: boş bir prompt'a basılan Enter da
/// `A` doğurur ama hiç komut koşmadığı için `D` gelmez. İkisini ayırt eden
/// bilgi [`ShellState::phase`]'te ve ayrımı yapan [`ShellLog::stripe`]; defter
/// yalnız gördüğünü kaydeder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// `A` geldi, `D` gelmedi.
    Pending,
    /// `D` geldi; kabuk kodu okunamayacak şekilde bastıysa `None`.
    Finished(Option<i32>),
}

/// Bir bloğun **çizilebilir** durumu; renge [`crate::Session::frame`]'de
/// temadan iniyor.
///
/// Üç değer, çünkü bugün çizilen üç renk var. Çizilmeyen durumun adı bu enum'da
/// değil, onu üreten fonksiyonun `None`'ı: "bilinmeyen hiçbir hâlde çizilmez"
/// bir renk seçimi değil, çizim kararı.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Stripe {
    /// Komut koşuyor — rengi temanın `accent`'ı.
    Running,
    /// Sıfır çıkış koduyla bitti.
    Success,
    /// Sıfırdan farklı çıkış koduyla bitti.
    Error,
}

/// Defterin en az tutacağı blok sayısı.
///
/// Tavan `scrollback`'ten türüyor (aşağıda) ve o **sıfır olabilir**: geçmiş
/// tutmayan bir oturumda bile ekrandaki blokların rengi gerekiyor. Taban bu
/// yüzden var ve bir **tasarım sabiti**, ölçüm değil ([`PAYLOAD_LIMIT`]
/// emsali): en büyük makul pencerede bile bir ekran dolusu prompt'un çok
/// üstünde.
const BLOCK_LOG_FLOOR: usize = 256;

/// `blok kimliği → akıbet` defteri; sabit halka.
///
/// **Tahliye sinyali beklenmiyor** ve bu bir eksiklik değil, veri yokluğu:
/// çıpa ızgaradan satır sıfırlanınca sessizce düşüyor ve alacritty bunu
/// yayınlamıyor. Halka bu yüzden tahliyeyi **tasarımla** çözüyor — en eskiyi
/// üstüne yazarak.
///
/// Kimlikler kabuk tarafından **birer birer artırılıyor**, yani halka her an
/// bitişik bir kimlik aralığı tutuyor ve arama indeks aritmetiğidir; doğrusal
/// tarama kare başına görünür blok sayısıyla çarpılırdı.
///
/// **Tavan `scrollback`'ten türüyor** ve sabit bir sayı değil: blok başına en
/// az bir satır (prompt) düştüğü için geçmişte görünebilecek blok sayısının
/// üst sınırı odur. Sabit bir tavan seçilseydi ya scrollback'in altında kalıp
/// hâlâ ekranda olan blokları renksiz bırakır ya da boşuna yer tutardı.
/// Kayıt başına 8 bayt: varsayılan 10 000 satırda 80 KB.
/// **Bilinen sınır:** tavan oturum doğarken belirleniyor; `scrollback` canlı
/// büyütülürse halka büyümüyor ve aradaki fark kadar eski blok rengini
/// kaybediyor — şerit **çizilmez**, yanlış çizilmez.
pub(crate) struct BlockLog {
    /// `entries[i]`, kimliği `first + i` olan bloğun akıbeti.
    entries: VecDeque<Outcome>,
    /// `entries[0]`'ın kimliği; defter boşken anlamsız.
    first: u32,
    capacity: usize,
}

impl BlockLog {
    pub(crate) fn new(scrollback: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            first: 0,
            capacity: scrollback.max(BLOCK_LOG_FLOOR),
        }
    }

    /// `A` ile açılan bloğu deftere yazar.
    fn start(&mut self, id: u32) {
        // Aralıktaki bir kimliğin ikinci kez açılması: bloğu yeniden açıyoruz,
        // defteri silmiyoruz. Ardındakiler artık geçersiz — o kimlikler bir
        // önceki turdan kalma.
        if let Some(at) = self.index_of(id) {
            self.entries.truncate(at + 1);
            self.entries[at] = Outcome::Pending;
            return;
        }
        // Bitişik değilse defter bu kimliği yorumlayamaz ve eskisini taşımak
        // iki ayrı sayacın bloklarını tek aralıkta gösterirdi.
        //
        // **İkisi de savunma kolu.** Sayacımız kabuk örneği boyunca monoton ve
        // `exec zsh` onu "sıfırlamıyor": `.zshrc` ilk prompt'tan önce
        // `__bateri_restore` çağırıyor, yani yeniden doğan kabuk kullanıcının
        // `ZDOTDIR`'ını miras alıyor, sarmalayıcıyı hiç yüklemiyor ve tek bir
        // işaret bile basmıyor. Buraya düşmenin yolu bizim basmadığımız bir
        // `bt_block=` olurdu — alanın bize özel olmasının ikinci sebebi bu.
        if self.entries.is_empty() || id != self.first.wrapping_add(self.entries.len() as u32) {
            self.entries.clear();
            self.first = id;
        }
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
            self.first = self.first.wrapping_add(1);
        }
        self.entries.push_back(Outcome::Pending);
    }

    /// `D` ile kapanan bloğun kodunu işler; defterde olmayan kimlik yoksayılır.
    fn finish(&mut self, id: u32, exit: Option<i32>) {
        if let Some(at) = self.index_of(id) {
            self.entries[at] = Outcome::Finished(exit);
        }
    }

    /// Bloğun akıbeti; defterde yoksa `None` ve o hâlde şerit çizilmez.
    fn get(&self, id: u32) -> Option<Outcome> {
        self.index_of(id).map(|at| self.entries[at])
    }

    /// Defterin **en son açtığı** blok; defter boşken `None`.
    ///
    /// Kimlikler bitişik ve artan olduğu için son kayıt son `A`'dır — "koşan
    /// blok hangisi" sorusunun tek yanıtı bu ([`ShellLog::running`]).
    fn last(&self) -> Option<(u32, Outcome)> {
        let at = self.entries.len().checked_sub(1)?;
        Some((self.first.wrapping_add(at as u32), self.entries[at]))
    }

    /// Kimliğin halkadaki yeri; aralığın dışındaki kimlik `None`.
    ///
    /// `checked_sub`: tavanı aşıp düşmüş (kimlik `first`'ten küçük) bir blok
    /// sarmayla halkanın sonuna düşmemeli.
    fn index_of(&self, id: u32) -> Option<usize> {
        let at = id.checked_sub(self.first)? as usize;
        (at < self.entries.len()).then_some(at)
    }
}

/// Okuyucu thread'in yazdığı, kare yolunun okuduğu kabuk defteri.
///
/// İki kayıt **tek** yaprak kilidin altında: ikisini de besleyen aynı işaret
/// akışı ve ikisini de okuyan aynı kare. Ayrı kilitler, aynı kareyi bir
/// işaretin iki yarısı arasında yakalayabilirdi.
pub(crate) struct ShellLog {
    /// Kabuğun o anki durumu; `None` = entegrasyon yok.
    pub(crate) state: Option<ShellState>,
    pub(crate) blocks: BlockLog,
}

impl ShellLog {
    pub(crate) fn new(scrollback: usize) -> Self {
        Self {
            state: None,
            blocks: BlockLog::new(scrollback),
        }
    }

    /// İşareti hem duruma hem deftere uygular; ilk işaret durumu **doğurur**.
    ///
    /// Durum yuvası `Option` olduğu için "hiç işaret görmedik" ile
    /// "prompt'tayız" karışmıyor: besleyen yokken yuva boş kalır ve dışarıya
    /// "entegrasyon yok" der.
    pub(crate) fn apply(&mut self, mark: Mark) {
        let state = self.state.get_or_insert(ShellState {
            phase: ShellPhase::Prompt,
            last_exit: None,
        });
        match mark {
            Mark::PromptStart { id } => {
                state.phase = ShellPhase::Prompt;
                if let Some(id) = id {
                    self.blocks.start(id);
                }
            }
            Mark::PromptEnd => state.phase = ShellPhase::Input,
            Mark::CommandStart => state.phase = ShellPhase::Running,
            Mark::CommandEnd { exit, id } => {
                state.phase = ShellPhase::Finished;
                // Kodu **her hâlde** tazeliyoruz: okunamayan bir kodu eskisiyle
                // doldurmak, biten komutu başkasının koduyla etiketlemek olurdu.
                state.last_exit = exit;
                if let Some(id) = id {
                    self.blocks.finish(id, exit);
                }
            }
        }
    }

    /// **Koşan** bloğun kimliği; yoksa `None`.
    ///
    /// İki koşul birlikte: safha `Running` **ve** defterin son kaydı hâlâ açık.
    /// İkincisi olmasaydı kimliksiz bir `A`'dan sonra gelen `C` safhayı
    /// `Running`'e alır, defterin son kaydı ise bir önceki (bitmiş) blok olur
    /// ve o blok koşuyormuş gibi boyanırdı.
    pub(crate) fn running(&self) -> Option<u32> {
        if self.state?.phase != ShellPhase::Running {
            return None;
        }
        match self.blocks.last()? {
            (id, Outcome::Pending) => Some(id),
            (_, Outcome::Finished(_)) => None,
        }
    }

    /// Bir bloğun şeridi; `None` → **çizilmez**.
    ///
    /// Defter ile safhanın birleştiği tek yer ve ikisi zaten aynı yaprak
    /// kilidin altında — ayrı dursalardı kare, bir işaretin iki yarısı
    /// arasında tutarsız bir çift okuyabilirdi.
    ///
    /// Koşan bloğun rengi **defterden değil safhadan** gelir ([`Self::running`]
    /// parametresi imzada bu yüzden var, kurallı tek istisna o): `D` henüz
    /// gelmediği için defterdeki kaydı `Pending` ve `Pending`'in kendisi
    /// "koşuyor" demek değil.
    ///
    /// Çizilmeyen dört durum tek `match`'te, çünkü dördü de aynı tezin
    /// parçası — bilinmeyeni yanlış çizmemek:
    ///
    /// - kimlik defterde yok (halka dolaştı ya da hiç görülmedi),
    /// - `Pending` ama koşmuyor (boş prompt'a basılan Enter, bekleyen prompt),
    /// - `Finished(None)`: komut bitti ama kod okunamadı — "bitti" için
    ///   nötr bir rol yok ve olmayan rolü `accent` ile taklit etmek koşmayan
    ///   bloğu koşuyor göstermek olurdu.
    pub(crate) fn stripe(&self, id: u32, running: Option<u32>) -> Option<Stripe> {
        if running == Some(id) {
            return Some(Stripe::Running);
        }
        match self.blocks.get(id)? {
            Outcome::Finished(Some(0)) => Some(Stripe::Success),
            Outcome::Finished(Some(_)) => Some(Stripe::Error),
            Outcome::Finished(None) | Outcome::Pending => None,
        }
    }
}

/// `ESC ] 133 ;` yükünün üst sınırı, bayt.
///
/// **Tasarım sabiti, ölçüm değil.** Standart yükler tek harf ile birkaç
/// anahtar-değerden ibaret (`A;aid=12345`, `D;0;aid=12345`); 256 bayt
/// bunların bir mertebe üstünde. Sınırın işi bir performans eşiği tutturmak
/// değil, bozuk ya da kötü niyetli bir akışın sonlandırıcı basmadan belleği
/// büyütmesini engellemek — sınırı aşan dizi düşürülür ve tarayıcı boşa döner.
const PAYLOAD_LIMIT: usize = 256;

/// Numara önekinin makul üst sınırı; aşan dizi bizim değildir.
///
/// `ESC ]` ardından rakam basıp sonlandırıcı basmayan bir akışta sayaç
/// taşmasın diye var; `133`'ün altı hane uzağında bir OSC numarası yok.
const MAX_OSC_NUMBER: u32 = 999_999;

/// Tarayıcının nerede olduğu. Chunk sınırında hayatta kalması gereken şey
/// yükün kendisi **değil**, bu durumun tamamı: "ESC gördüm" ve "rakamların
/// ortasındayım" da iki `read()` arasında taşınır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanState {
    /// Dizinin dışındayız; bir sonraki `ESC` aranıyor.
    Ground,
    /// `ESC` görüldü, `]` bekleniyor.
    Escape,
    /// `ESC ]` görüldü, OSC numarası toplanıyor.
    Number,
    /// `ESC ] 133 ;` görüldü, yük toplanıyor.
    Payload,
    /// Bizim dizimiz değil (ya da sınırı aştı): sonlandırıcıya kadar atlanıyor.
    Skip,
}

/// OSC 133'ü akışın içinden çeken durum makinesi.
///
/// **Numara kararı erken veriliyor:** `133` olmayan her dizi tampona
/// dokunmadan `Skip`'e düşer. Aksi hâlde her meşru OSC 52 kopyası (kilobayt,
/// megabayt) "sınırı aşan dizi" yoluna girer ve sınırın ayırt ettiği şey
/// kalmazdı.
pub(crate) struct Scanner {
    state: ScanState,
    /// `133;` sonrası yük; yalnız bizim dizimiz için dolar ve her dizide
    /// `clear()` ile yeniden kullanılır — dizi başına ayırma yok.
    payload: Vec<u8>,
    /// Toplanan OSC numarası ve hiç rakam görülüp görülmediği.
    number: u32,
    has_digit: bool,
}

impl Scanner {
    pub(crate) fn new() -> Self {
        Self {
            state: ScanState::Ground,
            payload: Vec::with_capacity(PAYLOAD_LIMIT),
            number: 0,
            has_digit: false,
        }
    }

    /// Dilimi tarar ve bulduğu her işareti `on_mark`'a verir.
    ///
    /// Baytlara **dokunmaz**: dilim `&[u8]`, dönüşte çağıran onu olduğu gibi
    /// ayrıştırıcıya geçirir.
    pub(crate) fn feed(&mut self, bytes: &[u8], mut on_mark: impl FnMut(Mark)) {
        let mut rest = bytes;
        while !rest.is_empty() {
            // Boşta hızlı yol: dizinin dışındayken tamponu bayt bayt
            // gezmiyoruz, bir sonraki `ESC`'e zıplıyoruz. Olağan akışın
            // neredeyse tamamı bu dal.
            if self.state == ScanState::Ground {
                match rest.iter().position(|&b| b == 0x1b) {
                    Some(at) => {
                        self.state = ScanState::Escape;
                        rest = &rest[at + 1..];
                    }
                    None => return,
                }
                continue;
            }
            let byte = rest[0];
            rest = &rest[1..];
            self.step(byte, &mut on_mark);
        }
    }

    fn step(&mut self, byte: u8, on_mark: &mut impl FnMut(Mark)) {
        match self.state {
            // `advance_ground` ile aynı: buraya yalnız hızlı yol düşerse gelinir.
            ScanState::Ground => {
                if byte == 0x1b {
                    self.state = ScanState::Escape;
                }
            }
            // `vte::advance_esc`: `]` diziyi açar, kalan her şey (CSI, DCS,
            // tek harfli kaçışlar) bizi ilgilendirmiyor.
            ScanState::Escape => match byte {
                b']' => {
                    self.state = ScanState::Number;
                    self.number = 0;
                    self.has_digit = false;
                }
                // `advance_esc`'in `Escape`'te **bırakan** baytları: `ESC`'in
                // kendisi, C0'ların 0x18/0x1A dışındakileri (`execute`
                // ediliyor, durum değişmiyor) ve 0x7F'ten büyük her şey (son
                // `_ => ()` kolu). Ground'a düşseydik bunların ardından gelen
                // `]` bizim için yeni bir dizi açmazdı ve ızgaranın geçerli
                // saydığı `ESC \r ] 133;A BEL` işareti bizde kaybolurdu.
                0x00..=0x17 | 0x19 | 0x1b | 0x1c..=0x1f | 0x7f.. => {}
                _ => self.state = ScanState::Ground,
            },
            ScanState::Number => match byte {
                b'0'..=b'9' => {
                    self.number = self
                        .number
                        .saturating_mul(10)
                        .saturating_add(u32::from(byte - b'0'));
                    self.has_digit = true;
                    if self.number > MAX_OSC_NUMBER {
                        self.state = ScanState::Skip;
                    }
                }
                b';' => {
                    self.state = if self.has_digit && self.number == 133 {
                        self.payload.clear();
                        ScanState::Payload
                    } else {
                        ScanState::Skip
                    };
                }
                _ if is_terminator(byte) => self.finish(byte, None, on_mark),
                // `vte` bu baytları yüke almadan atıyor; parite için biz de.
                _ if is_ignored(byte) => {}
                _ => self.state = ScanState::Skip,
            },
            ScanState::Payload => {
                if is_terminator(byte) {
                    let mark = parse_mark(&self.payload);
                    self.finish(byte, mark, on_mark);
                } else if is_ignored(byte) {
                } else if self.payload.len() == PAYLOAD_LIMIT {
                    // Sınırı aşan dizi düşer; sonlandırıcıya kadar atlanır ki
                    // arkasından gelen sağlam dizi yine görülsün.
                    self.state = ScanState::Skip;
                } else {
                    self.payload.push(byte);
                }
            }
            ScanState::Skip => {
                if is_terminator(byte) {
                    self.finish(byte, None, on_mark);
                }
            }
        }
    }

    /// Diziyi kapatır ve sonlandırıcının kendisine göre bir sonraki duruma
    /// geçer: çıplak `ESC` diziyi bitirir **ve** yeni bir kaçışı açar
    /// (`vte::advance_osc_string`, `0x1B` kolu).
    fn finish(&mut self, terminator: u8, mark: Option<Mark>, on_mark: &mut impl FnMut(Mark)) {
        self.payload.clear();
        self.state = if terminator == 0x1b {
            ScanState::Escape
        } else {
            ScanState::Ground
        };
        if let Some(mark) = mark {
            on_mark(mark);
        }
    }
}

/// Diziyi bitiren baytlar (`vte::advance_osc_string`).
fn is_terminator(byte: u8) -> bool {
    matches!(byte, 0x07 | 0x18 | 0x1a | 0x1b)
}

/// Dizinin içinde yoksayılan C0 baytları (`vte::advance_osc_string`).
fn is_ignored(byte: u8) -> bool {
    matches!(byte, 0x00..=0x06 | 0x08..=0x17 | 0x19 | 0x1c..=0x1f)
}

/// `133;` sonrasındaki yükü işarete çevirir; tanımadığını **yoksayar**.
///
/// İlk alan işaretin kendisidir ve **tam** eşleşmelidir: `A;aid=12` bir
/// `PromptStart`, `AB` hiçbir şey. Kabuklar işaretin yanına anahtar-değer
/// iliştirebiliyor ve onları bilmemek işareti kaybetmek anlamına gelmemeli.
fn parse_mark(payload: &[u8]) -> Option<Mark> {
    let mut fields = payload.split(|&b| b == b';');
    match fields.next()? {
        b"A" => Some(Mark::PromptStart {
            id: fields.find_map(block_id),
        }),
        b"B" => Some(Mark::PromptEnd),
        b"C" => Some(Mark::CommandStart),
        b"D" => {
            let mut exit = None;
            let mut id = None;
            // Kod **konuma** bağlı (ilk alan), kimlik **ada** (`aid=`). Konum
            // sorusu bu yüzden ada bakan koldan sonra sorulur: `D;aid=7`
            // kodsuz ama kimlikli geçerli bir yüktür ve ilk alanı körlemesine
            // koda saysaydık kimliği yutardı.
            for (index, field) in fields.enumerate() {
                if let Some(value) = block_id(field) {
                    id = Some(value);
                } else if index == 0 {
                    exit = number(field);
                }
            }
            Some(Mark::CommandEnd { exit, id })
        }
        _ => None,
    }
}

/// Blok kimliğini taşıyan **bize özel** alan adı.
///
/// **`aid` DEĞİL** ve bu ayrım kritik: `aid` semantic-prompts şartnamesinde
/// tanımlı, "uygulama kimliği" anlamına gelen ve genellikle **pid** taşıyan
/// bir alan — yani oturum boyunca *sabit*, bizimki gibi prompt başına artan
/// bir sayaç değil. Onu kimlik diye okusaydık, şartnameye uyan herhangi bir
/// entegrasyon (kullanıcının kendi rc'si, iç içe bir REPL, SSH'ın öte yakası)
/// her prompt'ta aynı değeri basar, defter onu bitişiksiz görüp kendini
/// **silerdi**; aralığa denk düşen bir `D;kod;aid=pid` ise bizim bloğumuzun
/// rengini başkasının koduyla ezerdi — tam da A′'nın reddedilme sebebi olan
/// "yanlış renk". Yabancı `aid` bu yüzden eskisi gibi **yoksayılıyor**.
const BLOCK_ID_FIELD: &[u8] = b"bt_block=";

/// `bt_block={sayı}` alanının değeri; başka her alan `None`.
fn block_id(field: &[u8]) -> Option<u32> {
    number(field.strip_prefix(BLOCK_ID_FIELD)?)
}

fn number<T: std::str::FromStr>(field: &[u8]) -> Option<T> {
    std::str::from_utf8(field).ok()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Diziyi verilen parçalar hâlinde besler; tarayıcı parçalar arasında
    /// durumunu taşımak zorunda.
    fn marks_of_chunks(chunks: &[&[u8]]) -> Vec<Mark> {
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        for chunk in chunks {
            scanner.feed(chunk, |mark| seen.push(mark));
        }
        seen
    }

    fn marks(bytes: &[u8]) -> Vec<Mark> {
        marks_of_chunks(&[bytes])
    }

    #[test]
    fn four_marks_are_recognised() {
        assert_eq!(
            marks(b"\x1b]133;A\x07"),
            vec![Mark::PromptStart { id: None }]
        );
        assert_eq!(marks(b"\x1b]133;B\x07"), vec![Mark::PromptEnd]);
        assert_eq!(marks(b"\x1b]133;C\x07"), vec![Mark::CommandStart]);
        assert_eq!(
            marks(b"\x1b]133;D\x07"),
            vec![Mark::CommandEnd {
                exit: None,
                id: None
            }]
        );
    }

    #[test]
    fn command_end_carries_the_exit_code() {
        assert_eq!(
            marks(b"\x1b]133;D;0\x07"),
            vec![Mark::CommandEnd {
                exit: Some(0),
                id: None
            }]
        );
        assert_eq!(
            marks(b"\x1b]133;D;130\x07"),
            vec![Mark::CommandEnd {
                exit: Some(130),
                id: None
            }]
        );
    }

    #[test]
    fn unreadable_exit_code_still_ends_the_command() {
        // Komutun bittiği bilgisi kodundan değerli: yükün parametresi bozuk
        // olsa da işaret düşmez, yalnız kod bilinmez.
        assert_eq!(
            marks(b"\x1b]133;D;abc\x07"),
            vec![Mark::CommandEnd {
                exit: None,
                id: None
            }]
        );
    }

    #[test]
    fn the_block_id_is_read_from_the_payload() {
        assert_eq!(
            marks(b"\x1b]133;A;bt_block=42\x07"),
            vec![Mark::PromptStart { id: Some(42) }]
        );
        assert_eq!(
            marks(b"\x1b]133;D;0;bt_block=42\x07"),
            vec![Mark::CommandEnd {
                exit: Some(0),
                id: Some(42)
            }]
        );
    }

    #[test]
    fn unknown_attributes_beside_the_mark_are_still_tolerated() {
        // Kabuklar işaretin yanına tanımadığımız anahtar-değerler iliştirebiliyor;
        // onları bilmemek ne işareti ne kimliği kaybettirmeli.
        assert_eq!(
            marks(b"\x1b]133;A;cl=m;bt_block=9\x07"),
            vec![Mark::PromptStart { id: Some(9) }]
        );
        assert_eq!(
            marks(b"\x1b]133;A;bt_block=\x07"),
            vec![Mark::PromptStart { id: None }]
        );
    }

    #[test]
    fn a_code_less_command_end_keeps_its_id() {
        // İlk alan körlemesine koda sayılsaydı `bt_block=7`'yi yutardı ve blok
        // defterde hiç kapanmazdı.
        assert_eq!(
            marks(b"\x1b]133;D;bt_block=7\x07"),
            vec![Mark::CommandEnd {
                exit: None,
                id: Some(7)
            }]
        );
    }

    #[test]
    fn both_terminators_end_the_sequence() {
        assert_eq!(
            marks(b"\x1b]133;A\x07"),
            vec![Mark::PromptStart { id: None }]
        );
        assert_eq!(
            marks(b"\x1b]133;A\x1b\\"),
            vec![Mark::PromptStart { id: None }]
        );
    }

    #[test]
    fn bare_escape_dispatches_like_vte() {
        // `vte` diziyi ESC'i görünce dağıtıyor, `\`'i beklemeden: peş peşe iki
        // dizi araya sonlandırıcı girmeden de okunur.
        assert_eq!(
            marks(b"\x1b]133;A\x1b]133;B\x07"),
            vec![Mark::PromptStart { id: None }, Mark::PromptEnd]
        );
        // Ve ESC'ten sonra gelen CSI diziyi bozmuyor.
        assert_eq!(marks(b"\x1b]133;C\x1b[0m"), vec![Mark::CommandStart]);
    }

    #[test]
    fn sequence_split_at_every_byte_survives() {
        let seq: &[u8] = b"\x1b]133;D;7\x07";
        for at in 0..=seq.len() {
            let (head, tail) = seq.split_at(at);
            assert_eq!(
                marks_of_chunks(&[head, tail]),
                vec![Mark::CommandEnd {
                    exit: Some(7),
                    id: None
                }],
                "bölünme noktası {at}"
            );
        }
    }

    #[test]
    fn escape_survives_the_bytes_vte_executes_in_place() {
        // `advance_esc` bu baytlarda `Escape`'te kalıyor, yani ardından gelen
        // `]` diziyi gerçekten açıyor. Ground'a düşen bir tarayıcı işareti
        // sessizce kaybeder ve durum ızgaradan ayrılırdı.
        assert_eq!(
            marks(b"\x1b\r]133;A\x07"),
            vec![Mark::PromptStart { id: None }]
        );
        assert_eq!(marks(b"\x1b\x07]133;B\x07"), vec![Mark::PromptEnd]);
        assert_eq!(marks(b"\x1b\x80]133;C\x07"), vec![Mark::CommandStart]);

        // Ve `Escape`'i gerçekten **bitiren** iki C0'da (0x18, 0x1A) dizi
        // açılmıyor — `advance_esc` onları Ground'a götürüyor.
        assert_eq!(marks(b"\x1b\x18]133;A\x07"), vec![]);
        assert_eq!(marks(b"\x1b\x1a]133;A\x07"), vec![]);
    }

    #[test]
    fn unknown_submark_and_broken_payload_are_ignored() {
        assert_eq!(marks(b"\x1b]133;Z\x07"), vec![]);
        assert_eq!(marks(b"\x1b]133;AB\x07"), vec![]);
        assert_eq!(marks(b"\x1b]133;\x07"), vec![]);
        assert_eq!(marks(b"\x1b]133\x07"), vec![]);
    }

    #[test]
    fn other_osc_numbers_never_touch_the_buffer() {
        // OSC 52'nin yükü meşru olarak megabayt olabilir; sınırın ayırt ettiği
        // şey kalsın diye o yol tampona hiç uğramaz.
        let mut stream = b"\x1b]52;c;".to_vec();
        stream.extend(std::iter::repeat_n(b'Z', 100_000));
        stream.push(0x07);
        stream.extend_from_slice(b"\x1b]133;B\x07");

        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        scanner.feed(&stream, |mark| seen.push(mark));

        assert_eq!(seen, vec![Mark::PromptEnd]);
        assert_eq!(scanner.payload.capacity(), PAYLOAD_LIMIT);
    }

    #[test]
    fn oversized_payload_is_dropped_and_the_next_sequence_survives() {
        let mut stream = b"\x1b]133;D;".to_vec();
        stream.extend(std::iter::repeat_n(b'9', PAYLOAD_LIMIT + 1));
        stream.push(0x07);
        stream.extend_from_slice(b"\x1b]133;A\x07");

        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        scanner.feed(&stream, |mark| seen.push(mark));

        assert_eq!(seen, vec![Mark::PromptStart { id: None }]);
        assert_eq!(scanner.payload.capacity(), PAYLOAD_LIMIT);
    }

    #[test]
    fn control_bytes_inside_the_payload_are_dropped_like_vte() {
        // `vte` yüke almıyor; almasaydık satır sonu yapışmış bir kod bozuk
        // görünürdü.
        assert_eq!(
            marks(b"\x1b]133;D;0\r\x07"),
            vec![Mark::CommandEnd {
                exit: Some(0),
                id: None
            }]
        );
    }

    #[test]
    fn plain_text_around_the_marks_is_ignored() {
        assert_eq!(
            marks(b"merhaba\x1b]133;A\x07dunya\x1b]133;B\x07$ ls\r\n"),
            vec![Mark::PromptStart { id: None }, Mark::PromptEnd]
        );
    }

    #[test]
    fn marks_walk_the_state_through_a_whole_command() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: None });
        assert_eq!(log.state.map(|s| s.phase), Some(ShellPhase::Prompt));

        log.apply(Mark::PromptEnd);
        assert_eq!(log.state.map(|s| s.phase), Some(ShellPhase::Input));

        log.apply(Mark::CommandStart);
        assert_eq!(log.state.map(|s| s.phase), Some(ShellPhase::Running));

        log.apply(Mark::CommandEnd {
            exit: Some(2),
            id: None,
        });
        assert_eq!(
            log.state,
            Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(2),
            })
        );
    }

    #[test]
    fn an_unreadable_code_does_not_inherit_the_previous_one() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::CommandEnd {
            exit: Some(2),
            id: None,
        });
        log.apply(Mark::CommandEnd {
            exit: None,
            id: None,
        });
        assert_eq!(log.state.and_then(|s| s.last_exit), None);
    }

    /// Bir bloğu açıp kapatır; defterin olağan akışı.
    fn run_block(log: &mut ShellLog, id: u32, exit: Option<i32>) {
        log.apply(Mark::PromptStart { id: Some(id) });
        log.apply(Mark::CommandStart);
        log.apply(Mark::CommandEnd { exit, id: Some(id) });
    }

    #[test]
    fn the_log_remembers_each_block_by_its_id() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        run_block(&mut log, 1, Some(0));
        run_block(&mut log, 2, Some(130));
        log.apply(Mark::PromptStart { id: Some(3) });

        assert_eq!(log.blocks.get(1), Some(Outcome::Finished(Some(0))));
        assert_eq!(log.blocks.get(2), Some(Outcome::Finished(Some(130))));
        // Açık ama kapanmamış: koşuyor ya da boş prompt.
        assert_eq!(log.blocks.get(3), Some(Outcome::Pending));
        assert_eq!(log.blocks.get(4), None);
    }

    #[test]
    fn a_command_end_without_a_start_is_ignored() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::CommandEnd {
            exit: Some(1),
            id: Some(77),
        });
        assert_eq!(log.blocks.get(77), None);
    }

    #[test]
    fn the_oldest_block_falls_out_of_the_full_log() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        for id in 1..=(BLOCK_LOG_FLOOR as u32 + 2) {
            run_block(&mut log, id, Some(0));
        }
        // Düşen bloğun rengi yok; kare yolu onu **çizmez**, yanlış çizmez.
        assert_eq!(log.blocks.get(1), None);
        assert_eq!(log.blocks.get(2), None);
        assert_eq!(log.blocks.get(3), Some(Outcome::Finished(Some(0))));
        assert_eq!(
            log.blocks.get(BLOCK_LOG_FLOOR as u32 + 2),
            Some(Outcome::Finished(Some(0)))
        );
    }

    #[test]
    fn reopening_an_id_drops_only_what_followed_it() {
        // Savunma kolu: sayacımız kabuk örneği boyunca monoton, yani bu
        // yalnız bizim basmadığımız bir `bt_block=` ile olur. Olduğunda da
        // defterin tamamı değil, o kimlikten SONRASI düşer.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        run_block(&mut log, 1, Some(0));
        run_block(&mut log, 2, Some(0));
        run_block(&mut log, 1, Some(3));

        assert_eq!(log.blocks.get(1), Some(Outcome::Finished(Some(3))));
        assert_eq!(log.blocks.get(2), None);
    }

    #[test]
    fn a_foreign_aid_is_ignored() {
        // `aid` şartnamede "uygulama kimliği"dir ve genellikle pid taşır, yani
        // oturum boyunca SABİTTİR. Kimlik diye okusaydık şartnameye uyan bir
        // entegrasyon (kullanıcının rc'si, iç içe REPL, SSH'ın öte yakası) her
        // prompt'ta aynı değeri basar ve defteri her seferinde bitişiksiz
        // kılardı; aralığa denk düşen bir `D` ise bizim bloğumuzun rengini
        // başkasının koduyla ezerdi.
        assert_eq!(
            marks(b"\x1b]133;A;aid=4711\x07"),
            vec![Mark::PromptStart { id: None }]
        );
        assert_eq!(
            marks(b"\x1b]133;D;1;aid=4711\x07"),
            vec![Mark::CommandEnd {
                exit: Some(1),
                id: None
            }]
        );

        // Ve yabancı işaretler bizim defterimize dokunamaz.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        run_block(&mut log, 1, Some(0));
        log.apply(Mark::PromptStart { id: None });
        log.apply(Mark::CommandEnd {
            exit: Some(1),
            id: None,
        });
        assert_eq!(log.blocks.get(1), Some(Outcome::Finished(Some(0))));
    }

    #[test]
    fn the_capacity_follows_the_scrollback() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR * 4);
        for id in 1..=(BLOCK_LOG_FLOOR as u32 * 4) {
            run_block(&mut log, id, Some(0));
        }
        // Taban olsaydı bu blok çoktan düşmüş olurdu.
        assert_eq!(
            log.blocks.get(BLOCK_LOG_FLOOR as u32 + 1),
            Some(Outcome::Finished(Some(0)))
        );
    }
}
