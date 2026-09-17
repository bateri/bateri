//! Kabuğun bastığı OSC işaretleri, onların tuttuğu oturum durumu, **komut
//! bloğu defteri** ve **ZLE'nin görüntü aynası**.
//!
//! Tarayıcının **iki kolu** var ve ikisi de aynı bayt akışından besleniyor:
//! [`MARK_OSC`] oturumun safhasını ve blok kimliklerini taşır, [`DOCK_OSC`]
//! satır düzenleyicinin (ZLE) o anki görüntüsünü. İkisi tek durum makinesinde,
//! çünkü akış tek: ayrı tarayıcılar aynı diziyi iki kez çerçevelerdi ve
//! çerçeveleme kuralının (aşağıdaki üç madde) iki kopyası doğardı.
//!
//! Dört sorumluluk, tek modül: baytlardan işaret çıkarmak ([`parse_mark`]),
//! işaretlerden oturum safhası tutmak ([`ShellState`]), blok kimliği başına
//! akıbet tutup şeridin çizilip çizilmeyeceğine karar vermek ([`BlockLog`],
//! [`ShellLog::stripe`]) ve aynanın beş değişkenini çözülmüş bir kayda
//! indirmek ([`DockState`]). Dördü aynı yerde, çünkü dördünü de **aynı**
//! işaret akışı besliyor; ayrılsalardı `D`'nin çıkış kodu bir modülden ötekine
//! elden ele geçerdi. Defterin tavanı `scrollback`'ten türüyor ve "bilinmeyen
//! kimlik çizilmez" kararı da burada — renderer'ın göreceği tek şey çözülmüş
//! renk.
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
//! Baytları ayrıştırıcıya giderken tarıyoruz. Aynı gerekçe [`DOCK_OSC`] için
//! de geçerli: `vte` onu **tanımıyor**, yükü `osc_dispatch`'in `_` koluna
//! düşürüp atıyor (`vte-0.15.0/src/ansi.rs`, `unhandled`). Yük bütünüyle
//! oraya ulaşıyor — `osc_raw` `std` altında sınırsız bir `Vec` ve 1024'lük
//! `MAX_OSC_RAW` yalnız `no_std` kolunda geçerli — ama ulaştığı yerde
//! okunmuyor.
//!
//! **Bedeli adıyla:** o `_` kolu yükü düşürmeden önce bayt başına bir
//! `write!` ile tanı dizgisi kuruyor ve dizgiyi `debug!`'tan **önce**
//! kurduğu için log seviyesi bunu kısa devre yapmıyor. Yani aynanın her tuş
//! vuruşu ayrıştırıcı tarafında yük uzunluğuyla orantılı bir ayırma daha
//! doğuruyor.
//!
//! Bu bizim kusurumuz değil, alacritty'nin **tanımadığı her** OSC'ye
//! davranışı; biz yalnız o yola sık uğrayan bir dizi soktuk. Kaçışı iki:
//! diziyi akıştan **çıkarmak** (tarayıcının "baytlara dokunmaz" sözünü bozar
//! ve dizi chunk sınırını aşabildiği için yerinde yapılamaz) ya da taşıyıcıyı
//! **DCS**'e çevirmek (`put` yükü ne tamponluyor ne formatlıyor; `discussion.md`
//! → Karar 5'te **değerlendirilmemiş** bir alternatif, 5b gibi elenmiş değil).
//! İkisi de ölçüm sonrasının işi: maliyet **kare yolunda değil** okuyucu
//! thread'inde ve aynı tuş vuruşunun kabuk tarafındaki base64 kodlaması zaten
//! baskın terim. Bilinen sınır olarak duruyor; ölçümü R6.2'nin (tuş başına
//! maliyet) borcunda.
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
/// arasındaki sınır, Input Dock'un (012) ilk sorusu. Ayrımı burada tutmak
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

/// ZLE'nin görüntü aynası — dock'un çizeceği her şey, **çözülmüş**.
///
/// Beş değişken taşınıyor ([`DOCK_OSC`]'un yükü) ve burada üç dizgi, bir
/// sütun ve bir aralık listesine iniyor. Yalnız `BUFFER` taşınsaydı bastırma
/// bilgi kaybına dönerdi: `POSTDISPLAY` autosuggestions'ın önerisi,
/// `region_highlight` de syntax highlighting'in rengi — ikisi de en yaygın iki
/// eklenti ve dock onlarsız kullanıcının gördüğünden **eksik** olurdu
/// (`discussion.md` → Karar 8b).
///
/// **Yeniden kullanılan tampondur, kayıt değil.** Tarayıcı her tuş vuruşunda
/// kendi kopyasını yerinde tazeliyor, [`ShellLog`] onu [`Clone::clone_from`]
/// ile kilidin altına alıyor ve [`crate::Session::dock_state`] yine
/// `clone_from` ile dışarı veriyor; üç adımda da dizgiler `clear()` +
/// `push_str` ile kapasitelerini koruyor. Sabit durumda tuş başına **sıfır**
/// ayırma var — ölçüt `CLAUDE.md`'nin kare başına maliyet kuralı ve bu tip
/// kare başına okunuyor.
///
/// `Clone` elle yazıldı: `derive` yalnız `clone`'u üretir ve varsayılan
/// `clone_from` "`*self = source.clone()`"dır, yani bu tipin tek önemli
/// özelliğini — kapasiteyi yeniden kullanmasını — sessizce kaybederdi.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DockState {
    /// Dock çizilebilir mi ve çizilemiyorsa neden.
    pub status: DockStatus,
    /// `PREDISPLAY` — ZLE'nin satırın **önüne** koyduğu, düzenlenemeyen metin.
    pub predisplay: String,
    /// `BUFFER` — kullanıcının yazdığı, düzenlenebilir metin.
    pub buffer: String,
    /// `POSTDISPLAY` — satırın **arkasına** eklenen, düzenlenemeyen metin;
    /// bugünkü tek üreticisi zsh-autosuggestions'ın önerisi.
    pub postdisplay: String,
    /// Caret'in **karakter** ofseti, [`Highlight::start`] ile **aynı uzayda**:
    /// `PREDISPLAY ++ BUFFER ++ POSTDISPLAY` dizgisinin başından sayılıyor.
    ///
    /// zsh'in `$CURSOR`'ı `BUFFER`'ın başından sayar; kaydırma sınırın bu
    /// tarafında yapılıyor ki iki ofset alanı tek uzayda kalsın. İkisi ayrı
    /// uzaylarda dursaydı çizen taraf birini `PREDISPLAY` uzunluğuyla
    /// kaydırmayı unuttuğu anda caret'i prompt boyu kadar kaydırırdı — ve
    /// `PREDISPLAY` boş olmadığı için bu **her satırda** olurdu.
    ///
    /// Bayt değil karakter: dock'un sorusu "kaçıncı hücreye çizeyim".
    pub cursor: usize,
    /// `region_highlight` — görüntünün renklendirilmiş aralıkları,
    /// [`Highlight::start`]'ın doc'undaki tek uzaya **normalize edilmiş**.
    pub highlights: Vec<Highlight>,
    /// `PREDISPLAY ++ BUFFER ++ POSTDISPLAY`'in karakter sayısı —
    /// [`Self::cursor`] ile aynı uzayın boyu.
    ///
    /// **Saklanıyor, çünkü zaten sayılmış:** çözücü ofsetleri kırpmak için üç
    /// uzunluğu da hesaplıyor. Tüketicisi [`ShellLog::suppressed_input`] ve
    /// oradan `Session::frame` — bastırılan aralığın **alt** ucu bundan
    /// türüyor. Kare başına yeniden saymak `frame()`'in `Term` kilidi
    /// öncesine O(n) bir gezinti eklerdi.
    pub display_chars: usize,
    /// Görüntünün **son boşluk olmayan** karakteri; boş satırda `None`.
    ///
    /// Bastırmanın **tazelik kapısı** bunu kullanıyor: ızgaradaki giriş
    /// satırının son mürekkepli hücresiyle karşılaştırılıyor ve uyuşmazsa
    /// ayna bayat sayılıp bastırma bırakılıyor. Boşluk **dışlanıyor**, çünkü
    /// boşluk hücresi sınırdan hiç geçmiyor (`frame()`'in atlama kapısı) ve
    /// `ls ` yazan kullanıcıda her karede yanlış alarm verirdi.
    ///
    /// Burada saklanıyor, çünkü çözücünün metni zaten elinde; kare başına
    /// yeniden taramak `Term` kilidi öncesine O(n) eklerdi.
    pub last_ink: Option<char>,
}

impl Clone for DockState {
    fn clone(&self) -> Self {
        let mut fresh = Self::default();
        fresh.clone_from(self);
        fresh
    }

    fn clone_from(&mut self, source: &Self) {
        self.status = source.status;
        self.predisplay.clear();
        self.predisplay.push_str(&source.predisplay);
        self.buffer.clear();
        self.buffer.push_str(&source.buffer);
        self.postdisplay.clear();
        self.postdisplay.push_str(&source.postdisplay);
        self.cursor = source.cursor;
        self.display_chars = source.display_chars;
        self.last_ink = source.last_ink;
        self.highlights.clear();
        self.highlights.extend_from_slice(&source.highlights);
    }
}

impl DockState {
    /// Metni ve aralıkları boşaltır; kapasiteler durur.
    ///
    /// Durumu **çağıran** yazar: bayat metni bırakmamak her iki çağıranın da
    /// (`End`, `Unavailable`) ortak işi, hangi duruma geçileceği değil.
    fn reset(&mut self) {
        self.predisplay.clear();
        self.buffer.clear();
        self.postdisplay.clear();
        self.cursor = 0;
        self.display_chars = 0;
        self.last_ink = None;
        self.highlights.clear();
    }
}

/// Aynanın o anki hâli — dock'un çizilip çizilmeyeceğinin tek yanıtı.
///
/// `Unavailable` ayrı bir varyant, `Idle`'ın içinde **değil**: ikisi aynı
/// şeyi göstermiyor. `Idle`'da çizilecek bir satır yok (ZLE düzenlemiyor),
/// `Unavailable`'da **var ama gösteremiyoruz** — ve fark phase-4'ün bastırma
/// kararını belirliyor: gösteremediğimiz satır ızgarada durmalı, yoksa
/// kullanıcı yazdığını hiçbir yerde görmez. Bugünkü `Skip` kolunun çağırana
/// sinyal vermemesi tam da bu belirtiyi doğuruyordu (R1.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DockStatus {
    /// ZLE satır düzenlemiyor: hiç ayna gelmedi ya da `line-finish` geldi.
    #[default]
    Idle,
    /// Alanlar taze ve geçerli.
    Live,
    /// Ayna geldi ama okunamadı; alanlar **boş**.
    Unavailable(DockFault),
}

/// Aynanın neden okunamadığı.
///
/// İkisi ayrı, çünkü ikisi ayrı şeyi söylüyor: `Overflow` sınırın dar
/// olduğunu (ve sınır [`DOCK_PAYLOAD_LIMIT`]'in doc'unda türetilmiş bir
/// tasarım sayısı), `Malformed` kanalın bozulduğunu. Tek varyanta
/// indirilseydi "sınırı büyütmem mi gerek" sorusunun yanıtı kaybolurdu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockFault {
    /// Yük [`DOCK_PAYLOAD_LIMIT`]'i aştı.
    Overflow,
    /// Yük çözülemedi: alan sayısı, base64 ya da UTF-8.
    Malformed,
}

/// `region_highlight`'ın bir kaydı: görüntünün bir aralığı ve stili.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Highlight {
    /// Aralığın başı, **karakter** ofseti.
    ///
    /// Uzay tek ve normalize: ofsetler `PREDISPLAY ++ BUFFER ++ POSTDISPLAY`
    /// dizgisinin başından sayılıyor. zsh iki uzay kullanıyor — kayıt `P` ile
    /// başlıyorsa ofset `PREDISPLAY`'in, başlamıyorsa `BUFFER`'ın başından
    /// (`zshzle(1)`, `region_highlight`) — ve ikisini sınırın **bu** tarafında
    /// birleştirmek çizen tarafı `PREDISPLAY`'in uzunluğunu bilmekten
    /// kurtarıyor. R1.3'ün "çözülmüş geçer"i budur.
    pub start: usize,
    /// Aralığın sonu, dışlamalı.
    pub end: usize,
    pub style: HighlightStyle,
}

/// Bir aralığın stili — zsh'in "character highlighting" spesifikasyonunun
/// bizim tanıdığımız yarısı.
///
/// Tanınmayan bileşen (`blink`, `dim`, bilinmeyen bir ad) **sessizce düşer**,
/// kaydı düşürmez: aynanın işi kullanıcının gördüğünü taşımak ve tanımadığımız
/// bir niteliğe takılıp bütün aralığı renksiz bırakmak bilgiyi büsbütün
/// kaybetmek olurdu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HighlightStyle {
    pub fg: Option<HighlightColor>,
    pub bg: Option<HighlightColor>,
    pub bold: bool,
    pub underline: bool,
    /// `standout` — zsh'in ters video'su; SGR 7'nin karşılığı.
    pub standout: bool,
}

/// Bir stil bileşeninin rengi; temaya **burada** bağlanmıyor.
///
/// Çözüm `frame()`'de, [`crate::Theme`] elde olduğunda: renk uzayı sınırı
/// geçerken lineerleşiyor (`CLAUDE.md` → Renk uzayı) ve bu modülün teması yok.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HighlightColor {
    /// 0–255; ilk 16'sı temanın [`crate::Theme::ansi`]'si, üstü 256 renk küpü.
    Indexed(u8),
    /// `#rrggbb` → `0xRRGGBB`.
    Rgb(u32),
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
            capacity: Self::capacity_for(scrollback),
        }
    }

    /// Tavanın `scrollback`'ten türemesi — [`BlockLog::new`] ile
    /// [`BlockLog::set_capacity`]'nin **tek** kaynağı.
    ///
    /// İki yerde ayrı ayrı yazılsaydı taban (`BLOCK_LOG_FLOOR`) birinde
    /// unutulabilir ve canlı küçültülen bir `scrollback` defteri sıfıra
    /// indirebilirdi.
    fn capacity_for(scrollback: usize) -> usize {
        scrollback.max(BLOCK_LOG_FLOOR)
    }

    /// `scrollback` kayıt anında değişince tavanı da taşır.
    ///
    /// Tavan eskiden yalnız oturum doğarken belirleniyordu ve `scrollback`
    /// **canlı uygulanan** bir ayar: büyütülen geçmişin fazlası renksiz
    /// kalıyordu (`/code-review`, 010 kapı). Küçültmede fazlalık en eskiden
    /// atılıyor — halkanın kendi tahliye kuralı, ikinci bir politika yok.
    fn set_capacity(&mut self, scrollback: usize) {
        self.capacity = Self::capacity_for(scrollback);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
            self.first = self.first.wrapping_add(1);
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
    /// ZLE'nin görüntü aynası. Aynı kilidin altında, çünkü aynı akıştan
    /// besleniyor ve aynı kare okuyor: ayrı bir kilit, kareyi safha ile
    /// aynanın çeliştiği bir anda yakalayabilirdi — `Input` safhasında
    /// ızgarayı bastırıp dock'a bir önceki satırı çizmek gibi.
    pub(crate) dock: DockState,
}

/// Bastırılacak giriş satırının iki ucu; `Copy`.
///
/// Aralığın **üstünü** kimlik verir (çıpası o satırda), **altını** imlecin
/// arkasında kalan metin. İkisi tek kayıtta, çünkü ikisi de aynı yaprak kilit
/// turundan çıkıyor; ayrı okunsalardı farklı anlara ait olabilirlerdi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SuppressedInput {
    /// Yazılmakta olan bloğun kimliği; **satırı** [`crate::Session::frame`]
    /// çıpadan bulur — kabuk hangi satırda olduğunu bilmiyor.
    pub(crate) block: u32,
    /// Caret'ten **sonra** gelen karakter sayısı; girişin imleç satırının
    /// altında kaç satır daha sürdüğü bundan çıkıyor.
    pub(crate) chars_after_cursor: usize,
    /// Caret'ten **önce** gelen karakter sayısı ([`DockState::cursor`]);
    /// girişin imleç satırının üstünde kaç satır sürdüğü bundan çıkıyor.
    ///
    /// Aralığın üstünü yalnız çıpaya bağlamak **yetmiyor**: çıpa prompt'un
    /// satırında duruyor ve imleç oradan uzaklaşırsa (araya başka bir şey
    /// basılırsa) ikisinin arasındaki satırlar girişin değil, yine de
    /// bastırılırdı. Ayna kaç satır tuttuğunu biliyor; üst uç ikisinin
    /// **alttakini** seçiyor.
    pub(crate) chars_before_cursor: usize,
    /// Görüntünün son mürekkebi ([`DockState::last_ink`]) — tazelik kapısının
    /// aynadaki yarısı.
    pub(crate) last_ink: Option<char>,
}

impl ShellLog {
    pub(crate) fn new(scrollback: usize) -> Self {
        Self {
            state: None,
            blocks: BlockLog::new(scrollback),
            dock: DockState::default(),
        }
    }

    /// `scrollback` kayıt anında değiştiğinde defterin tavanını taşır;
    /// oturum durumuna (`state`) dokunmaz.
    pub(crate) fn set_scrollback(&mut self, scrollback: usize) {
        self.blocks.set_capacity(scrollback);
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

    /// Tarayıcının çıkardığı olayı doğru kola uygular.
    ///
    /// Tek giriş noktası, çünkü okuyucu thread'i kilidi **olay başına** alıyor
    /// ve iki ayrı çağrı iki ayrı kilit turu demek olurdu.
    pub(crate) fn apply_scan(&mut self, event: ScanEvent<'_>) {
        match event {
            ScanEvent::Mark(mark) => self.apply(mark),
            ScanEvent::Dock(event) => self.apply_dock(event),
        }
    }

    /// Ayna olayını [`Self::dock`]'a uygular.
    ///
    /// Çizilemeyen iki hâlde (`End`, `Unavailable`) metin **boşaltılıyor**:
    /// bayat bir satır bırakmak, phase-4'te ızgara bastırılırken dock'un bir
    /// önceki komutu göstermesi demek olurdu — kullanıcının yazdığıyla
    /// gördüğünün sessizce ayrılması, bu deponun yasakladığı belirti sınıfı.
    fn apply_dock(&mut self, event: DockEvent<'_>) {
        match event {
            DockEvent::Update(staged) => self.dock.clone_from(staged),
            DockEvent::End => {
                self.dock.reset();
                self.dock.status = DockStatus::Idle;
            }
            DockEvent::Unavailable(fault) => {
                self.dock.reset();
                self.dock.status = DockStatus::Unavailable(fault);
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

    /// Kullanıcının **şu an yazdığı** bloğun kimliği — giriş satırı ızgaradan
    /// bastırılacaksa `Some`, değilse `None`.
    ///
    /// [`crate::Session::frame`] bunu `Term` kilidinden **önce** okuyor
    /// ([`crate::Theme`] ile aynı örüntü) ve dönen kimlikle çıpa satırını
    /// buluyor: bastırılacak aralık o satırdan imlecin satırına.
    ///
    /// **Üç koşul birlikte ve üçü de zorunlu:**
    ///
    /// - Safha `Input` — kullanıcı yazıyor. `Prompt`'ta ZLE henüz satırı
    ///   almadı, `Running`/`Finished`'da yazdığı şey çoktan ızgaranın kalıcı
    ///   içeriği oldu.
    /// - Ayna `Live` — satırı **başka bir yerde** gösterebiliyoruz. `Idle` ve
    ///   `Unavailable` ayrı ayrı doğru cevaplar: ilkinde ZLE satır
    ///   düzenlemiyor (`line-finish` geldi), ikincisinde gösteremediğimiz bir
    ///   satır var ve ızgarada kalması **şart**, yoksa kullanıcı yazdığını
    ///   hiçbir yerde görmez (R1.2). Kapının bu katı [`DockStatus`]'ün varlık
    ///   sebebi.
    /// - Defterin son kaydı hâlâ açık — [`Self::running`]'in ikinci koşulunun
    ///   aynısı ve aynı gerekçeyle: kimliksiz bir `A`'dan sonra gelen `B`
    ///   safhayı `Input`'a alır, defterin son kaydı ise bir önceki (bitmiş)
    ///   blok olur ve bastırma **yanlış** satırdan başlardı.
    pub(crate) fn suppressed_input(&self) -> Option<SuppressedInput> {
        if self.state?.phase != ShellPhase::Input || self.dock.status != DockStatus::Live {
            return None;
        }
        match self.blocks.last()? {
            (block, Outcome::Pending) => Some(SuppressedInput {
                block,
                chars_after_cursor: self.dock.display_chars.saturating_sub(self.dock.cursor),
                chars_before_cursor: self.dock.cursor,
                last_ink: self.dock.last_ink,
            }),
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

/// İşaret kolunun OSC numarası — FinalTerm'ün "semantic prompt"u.
///
/// Bizim seçimimiz değil, uyduğumuz sözleşme: iTerm2, kitty, WezTerm, VS Code
/// ve Ghostty aynı numarayı okuyor, yani betiğimiz onların altında da blok
/// üretiyor.
const MARK_OSC: u32 = 133;

/// Ayna kolunun OSC numarası — **bizim** dizimiz.
///
/// Sayının kendisi bir karar ve iki ölçütü var:
///
/// **Çakışmamalı.** Dışlama listesi elle hatırlanmadı, grep'lendi:
/// ayrıştırıcımızın (`vte-0.15.0/src/ansi.rs`, `osc_dispatch`) yorumladığı
/// numaralar 0, 2, 4, 8, 10–12, 22, 50, 52, 104 ve 110–112; geri kalan her
/// şey `unhandled` koluna düşüyor. Üstüne yaygın entegrasyonların sahipli
/// numaraları: 7 (cwd), 9 (ConEmu/Windows Terminal), 133, 633 (VS Code), 777
/// (urxvt), 1337 (iTerm2/WezTerm), 9278 (Warp), 30001–30002 (kitty). 8133
/// hiçbirinde yok.
///
/// **Kısa olmalı.** Numara tuş **başına** akışa giriyor (R6.2); altı haneli
/// bir sayı her vuruşta iki bayt fazla demek. Dört hane, `133`'ün ikinci kolu
/// olduğunu söyleyen bir önekle: `8133`.
///
/// **Başka terminalde ne olur:** pratikte hiçbir şey, çünkü sarmalayıcı
/// yalnız bateri'nin `ZDOTDIR`'ı altında yükleniyor — yabancı bir terminal bu
/// diziyi hiç görmüyor. Tek istisna bateri'nin **içinde** koşan `tmux`/`screen`
/// ve ikisi de tanımadığı OSC'yi düşürüyor.
const DOCK_OSC: u32 = 8133;

/// `ESC ] 133 ;` yükünün üst sınırı, bayt.
///
/// **Tasarım sabiti, ölçüm değil.** Standart yükler tek harf ile birkaç
/// anahtar-değerden ibaret (`A;aid=12345`, `D;0;aid=12345`); 256 bayt
/// bunların bir mertebe üstünde. Sınırın işi bir performans eşiği tutturmak
/// değil, bozuk ya da kötü niyetli bir akışın sonlandırıcı basmadan belleği
/// büyütmesini engellemek — sınırı aşan dizi düşürülür ve tarayıcı boşa döner.
const PAYLOAD_LIMIT: usize = 256;

/// `ESC ] 8133 ;` yükünün üst sınırı, bayt.
///
/// [`PAYLOAD_LIMIT`] (256) bu kol için **yanlış**: bir komut satırı onu tek
/// başına aşar. Sayı türetildi, seçilmedi:
///
/// - 4096 karakterlik bir giriş — 200 sütunluk bir pencerede yirmi satır,
///   elle yazılan bir komut satırının mertebelerce üstü.
/// - En kötü hâlde karakter başına 4 bayt UTF-8 → 16 KiB.
/// - base64'ün 4/3 şişmesi → ~21 KiB.
/// - `region_highlight` aynı mertebede: sözdizimi vurgusu jeton başına bir
///   kayıt bırakıyor ve kayıt başına ~30 bayt.
/// - Yuvarlanmış tavan: **64 KiB**.
///
/// **Neyi yönetiyor:** doğruluğu değil, dock'un *kullanılabilirliğini*. Aşan
/// bir satır [`DockFault::Overflow`] ile görünür oluyor ve giriş ızgarada
/// kalıyor — kullanıcı yazdığını yine görüyor, yalnız dock'ta değil.
const DOCK_PAYLOAD_LIMIT: usize = 64 * 1024;

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
    /// Bizim bir numaramız ve `;` görüldü; yük o kolun tamponuna toplanıyor.
    Payload(Arm),
    /// Bizim dizimiz değil (ya da sınırı aştı): sonlandırıcıya kadar atlanıyor.
    Skip,
}

/// Tarayıcının iki kolu; yük hangi tampona ve hangi ayrıştırıcıya gidiyor.
///
/// Durumun içinde taşınıyor, ayrı bir alanda değil: yük toplanırken kolun
/// **her zaman** belli olması tipin şekliyle garanti — ayrı bir alan
/// `Ground`'da da anlamlı görünür ve "hangi koldayız" sorusu iki yerden
/// yanıtlanabilirdi.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    /// [`MARK_OSC`] — işaret kolu.
    Mark,
    /// [`DOCK_OSC`] — ayna kolu.
    Dock,
}

/// Tarayıcının dışarıya verdiği olay.
///
/// İki kol tek `enum`'da, çünkü tek çağrı: okuyucu thread'i kilidi olay
/// başına alıyor ve iki ayrı callback iki ayrı kilit turu doğururdu.
///
/// `Update` **ödünç veriyor**, sahiplenmiyor: aynanın çözülmüş hâli
/// tarayıcının kendi tamponunda duruyor ve tüketici onu kilidin altında
/// `clone_from` ile alıyor. Sahiplenseydi tuş başına üç `String` ile bir
/// `Vec` doğardı ([`DockState`]'in doc'u).
pub(crate) enum ScanEvent<'a> {
    Mark(Mark),
    Dock(DockEvent<'a>),
}

/// Ayna kolunun olayları.
pub(crate) enum DockEvent<'a> {
    /// Satır tazelendi; çözülmüş hâli ödünçte.
    Update(&'a DockState),
    /// `line-finish`: ZLE satırı bıraktı.
    End,
    /// Ayna geldi ama okunamadı. **Sinyal burada**: bugünkü `Skip` kolu
    /// çağırana hiçbir şey söylemiyordu (R1.2).
    Unavailable(DockFault),
}

/// İki OSC numarasını akışın içinden çeken durum makinesi.
///
/// **Numara kararı erken veriliyor:** bizim olmayan her dizi tampona
/// dokunmadan `Skip`'e düşer. Aksi hâlde her meşru OSC 52 kopyası (kilobayt,
/// megabayt) "sınırı aşan dizi" yoluna girer ve sınırın ayırt ettiği şey
/// kalmazdı.
///
/// **İki kolun tamponu ayrı.** Tek tampon paylaşılsaydı ya 133'ün dar sınırı
/// aynayı keser ya da aynanın geniş sınırı 133'ün koruduğu şeyi (sonlandırıcı
/// basmayan bir akışın belleği büyütmesi) bırakırdı — bir tampon iki sınıra
/// birden uyamaz.
pub(crate) struct Scanner {
    state: ScanState,
    /// `133;` sonrası yük; yalnız bizim dizimiz için dolar ve her dizide
    /// `clear()` ile yeniden kullanılır — dizi başına ayırma yok.
    payload: Vec<u8>,
    /// `8133;` sonrası yük. Ayrı tampon, ayrı sınır (yukarıda).
    dock: Vec<u8>,
    /// base64 çıktısının indiği ara tampon; her alanda yeniden kullanılır.
    decoded: Vec<u8>,
    /// Aynanın çözülmüş hâli — [`DockEvent::Update`]'in ödünç verdiği tampon.
    line: DockState,
    /// Toplanan OSC numarası ve hiç rakam görülüp görülmediği.
    number: u32,
    has_digit: bool,
}

impl Scanner {
    pub(crate) fn new() -> Self {
        Self {
            state: ScanState::Ground,
            payload: Vec::with_capacity(PAYLOAD_LIMIT),
            // Ayna tamponu **baştan** ayrılıyor, 133'ünki gibi: sabit durumda
            // tuş başına ayırma olmamalı ve büyüyerek gelen bir tampon ilk
            // satırlarda tam da onu yapardı. Oturum başına 64 KiB, grid'in
            // yanında ölçülemeyecek kadar küçük.
            dock: Vec::with_capacity(DOCK_PAYLOAD_LIMIT),
            decoded: Vec::new(),
            line: DockState::default(),
            number: 0,
            has_digit: false,
        }
    }

    /// Dilimi tarar ve bulduğu her olayı `on_event`'e verir.
    ///
    /// Baytlara **dokunmaz**: dilim `&[u8]`, dönüşte çağıran onu olduğu gibi
    /// ayrıştırıcıya geçirir.
    pub(crate) fn feed(&mut self, bytes: &[u8], mut on_event: impl FnMut(ScanEvent<'_>)) {
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
            self.step(byte, &mut on_event);
        }
    }

    fn step(&mut self, byte: u8, on_event: &mut impl FnMut(ScanEvent<'_>)) {
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
                    self.state = match (self.has_digit, self.number) {
                        (true, MARK_OSC) => {
                            self.payload.clear();
                            ScanState::Payload(Arm::Mark)
                        }
                        (true, DOCK_OSC) => {
                            self.dock.clear();
                            ScanState::Payload(Arm::Dock)
                        }
                        _ => ScanState::Skip,
                    };
                }
                _ if is_terminator(byte) => self.close(byte),
                // `vte` bu baytları yüke almadan atıyor; parite için biz de.
                _ if is_ignored(byte) => {}
                _ => self.state = ScanState::Skip,
            },
            ScanState::Payload(Arm::Mark) => {
                if is_terminator(byte) {
                    let mark = parse_mark(&self.payload);
                    self.close(byte);
                    if let Some(mark) = mark {
                        on_event(ScanEvent::Mark(mark));
                    }
                } else if is_ignored(byte) {
                } else if self.payload.len() == PAYLOAD_LIMIT {
                    // Sınırı aşan dizi düşer; sonlandırıcıya kadar atlanır ki
                    // arkasından gelen sağlam dizi yine görülsün.
                    self.state = ScanState::Skip;
                } else {
                    self.payload.push(byte);
                }
            }
            ScanState::Payload(Arm::Dock) => {
                if is_terminator(byte) {
                    // Çözme `close`'dan **önce**: `close` tamponu boşaltıyor.
                    let outcome = parse_dock(&self.dock, &mut self.decoded, &mut self.line);
                    self.close(byte);
                    on_event(ScanEvent::Dock(match outcome {
                        DockOutcome::Update => DockEvent::Update(&self.line),
                        DockOutcome::End => DockEvent::End,
                        DockOutcome::Unavailable(fault) => DockEvent::Unavailable(fault),
                    }));
                } else if is_ignored(byte) {
                } else if self.dock.len() == DOCK_PAYLOAD_LIMIT {
                    // 133'ün sessiz düşüşünün aksine aşım **anında** bildirilir:
                    // tüketici "gösteremiyorum" diyebilsin diye (R1.2). Dizinin
                    // kalanı yine atlanır ki arkasından geleni görelim.
                    self.state = ScanState::Skip;
                    on_event(ScanEvent::Dock(DockEvent::Unavailable(DockFault::Overflow)));
                } else {
                    self.dock.push(byte);
                }
            }
            ScanState::Skip => {
                if is_terminator(byte) {
                    self.close(byte);
                }
            }
        }
    }

    /// Diziyi kapatır ve sonlandırıcının kendisine göre bir sonraki duruma
    /// geçer: çıplak `ESC` diziyi bitirir **ve** yeni bir kaçışı açar
    /// (`vte::advance_osc_string`, `0x1B` kolu).
    fn close(&mut self, terminator: u8) {
        self.payload.clear();
        self.dock.clear();
        self.state = if terminator == 0x1b {
            ScanState::Escape
        } else {
            ScanState::Ground
        };
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
            // Kod **konuma** bağlı (ilk alan), kimlik **ada** — ve o ad
            // `BLOCK_ID_FIELD`, yani `bt_block=`; `aid=` **değil** (gerekçe
            // aşağıda, `block_id`'nin doc'unda: yabancı bir `aid` bizim
            // sayacımızla karışmamalı). Konum sorusu bu yüzden ada bakan koldan
            // sonra sorulur: `D;bt_block=7` kodsuz ama kimlikli geçerli bir
            // yüktür ve ilk alanı körlemesine koda saysaydık kimliği yutardı.
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

/// [`parse_dock`]'un üç sonucu; olaya [`Scanner::step`] çeviriyor.
///
/// Ayrı bir tip, çünkü `parse_dock` `DockEvent`'i **üretemez**: `Update`
/// varyantı `line`'ı ödünç alıyor ve fonksiyon onu `&mut` tutuyor.
enum DockOutcome {
    Update,
    End,
    Unavailable(DockFault),
}

/// Ayna yükünü çözer ve `line`'a yazar.
///
/// **Tel biçimi** (alanlar `;` ile, gövdeler base64):
///
/// ```text
/// ESC ] 8133 ; u ; {CURSOR} ; {PREDISPLAY} ; {BUFFER} ; {POSTDISPLAY} ; {region_highlight} BEL
/// ESC ] 8133 ; e BEL
/// ESC ] 8133 ; o BEL
/// ```
///
/// `u` satırı tazeler, `e` (`line-finish`) kapatır, `o` kabuğun "bu görüntü
/// aynaya sığmıyor" demesidir. **Fazladan alan
/// yoksayılır** — [`parse_mark`]'ın bilinmeyen anahtar-değeri tolere etmesiyle
/// aynı gerekçe: phase-4'ün özel kip sinyali bu ayrıştırıcıyı yeniden açmadan
/// eklenebilmeli.
///
/// **Neden base64:** gövdeler kullanıcının yazdığı metin, yani içlerinde `;`,
/// `ESC` ve C0 baytları olabilir — üçü de dizinin çerçevesini bozar. base64'ün
/// alfabesinde üçünden hiçbiri yok, yani çerçeveleme kuralları (yukarıdaki üç
/// madde) gövdeye hiç dokunmuyor. Kodlayan taraf saf zsh; fork yok.
///
/// `region_highlight` kayıtları gövdenin içinde satır sonuyla ayrılıyor.
///
/// Bozuk yükte `line` **boşaltılıyor**: yarım yazılmış bir kayıt hiçbir yere
/// yayılmıyor (`Malformed` → [`ShellLog::apply_dock`] zaten sıfırlıyor) ama
/// tamponu kirli bırakmak sonraki okumayı akıl yürütme borcuna çevirirdi.
fn parse_dock(payload: &[u8], decoded: &mut Vec<u8>, line: &mut DockState) -> DockOutcome {
    let mut fields = payload.split(|&b| b == b';');
    let Some(op) = fields.next() else {
        return unavailable(line, DockFault::Malformed);
    };
    match op {
        b"e" => DockOutcome::End,
        // **Aşımın kabuk tarafındaki ucu.** [`DOCK_PAYLOAD_LIMIT`] yükü burada
        // keserken kabuk onu **kodlamış** oluyor; `o` kodlamadan önce
        // ölçtüğünü söylüyor. İkisi aynı bütçenin iki yakası ve ayrı ayrı
        // gerekli: bu uç kabuğun tuş başına harcadığı zamanı, öteki uç bizim
        // belleğimizi koruyor. Sonucu aynı olmak **zorunda**, yoksa sınırın
        // hangi tarafta tutulduğu kullanıcıya farklı davranış olarak yansırdı.
        b"o" => unavailable(line, DockFault::Overflow),
        b"u" => match decode_line(&mut fields, decoded, line) {
            Some(()) => DockOutcome::Update,
            // Durum da yazılıyor: `decode_line` daha ilk satırda `Live` diyor
            // ve yarım kalan bir çözüm onu olduğu gibi bırakırsa tarayıcının
            // tamponu "canlı" adı altında boş metin taşırdı.
            None => unavailable(line, DockFault::Malformed),
        },
        _ => unavailable(line, DockFault::Malformed),
    }
}

/// Tamponu boşaltır, durumu yazar ve sonucu döndürür.
///
/// Üç çağıran da aynı şeyi yapmak zorunda: gösteremediğimiz bir satırın metni
/// tamponda kalırsa sonraki okuma "bu metin taze mi" sorusunu akıl yürütmeyle
/// yanıtlamak zorunda kalır.
fn unavailable(line: &mut DockState, fault: DockFault) -> DockOutcome {
    line.reset();
    line.status = DockStatus::Unavailable(fault);
    DockOutcome::Unavailable(fault)
}

/// `u` yükünün beş alanını `line`'a çözer; eksik ya da bozuk alanda `None`.
fn decode_line<'a>(
    fields: &mut impl Iterator<Item = &'a [u8]>,
    decoded: &mut Vec<u8>,
    line: &mut DockState,
) -> Option<()> {
    line.status = DockStatus::Live;
    // İmleç alanı tel sırasında önce geliyor ama normalize edilmesi
    // `PREDISPLAY` çözülene kadar bekliyor.
    let cursor_in_buffer: usize = number(fields.next()?)?;
    decode_text(fields.next()?, decoded, &mut line.predisplay)?;
    decode_text(fields.next()?, decoded, &mut line.buffer)?;
    decode_text(fields.next()?, decoded, &mut line.postdisplay)?;

    // Üç uzunluk da burada: ofsetlerin tek uzaya inmesi ([`Highlight::start`])
    // ve görüntünün dışına taşan bir ofsetin kırpılması bunları istiyor.
    let predisplay_chars = line.predisplay.chars().count();
    let text_chars = predisplay_chars + line.buffer.chars().count();
    let display_chars = text_chars + line.postdisplay.chars().count();
    // `$CURSOR` en çok `$#BUFFER`'dır; kırpma kabuğun sözüne güvenmemek için.
    line.cursor = predisplay_chars
        .checked_add(cursor_in_buffer)?
        .min(text_chars);
    line.display_chars = display_chars;
    // Sondan ilk boşluk olmayan karakter; üç gövde görüntü sırasında.
    line.last_ink = line
        .predisplay
        .chars()
        .chain(line.buffer.chars())
        .chain(line.postdisplay.chars())
        .filter(|ch| !ch.is_whitespace())
        .next_back();

    decoded.clear();
    decode_base64(fields.next()?, decoded)?;
    let entries = std::str::from_utf8(decoded).ok()?;
    line.highlights.clear();
    line.highlights.extend(
        entries
            .lines()
            .filter_map(|entry| parse_highlight(entry, predisplay_chars, display_chars)),
    );
    Some(())
}

/// base64 alanını çözer ve `into`'ya **kapasitesini koruyarak** yazar.
fn decode_text(field: &[u8], decoded: &mut Vec<u8>, into: &mut String) -> Option<()> {
    decoded.clear();
    decode_base64(field, decoded)?;
    let text = std::str::from_utf8(decoded).ok()?;
    into.clear();
    into.push_str(text);
    Some(())
}

/// `region_highlight`'ın bir kaydı: `[P]{başlangıç} {bitiş} {spec} [memo=…]`.
///
/// `memo=` ve tanınmayan kuyruk alanları yoksayılıyor (`zshzle(1)` onları
/// serbest bırakıyor).
///
/// **Aralık `display_chars`'a kırpılıyor ve boş kalan düşüyor.** Çizen tarafa
/// metnin dışını gösteren bir ofset taşımak orada bir `panic` (ya da sessiz
/// bir kırpma) borcu doğururdu ve taşan ofset varsayımsal değil: bayat bir
/// `BUFFER` anlık görüntüsünden `region_highlight` kuran her eklenti üretir.
/// Ters aralık da aynı kapıdan düşüyor.
fn parse_highlight(
    entry: &str,
    predisplay_chars: usize,
    display_chars: usize,
) -> Option<Highlight> {
    let mut parts = entry.split_whitespace();
    let first = parts.next()?;
    // `P` öneki ofseti `PREDISPLAY`'in başına bağlıyor; öneksizi `BUFFER`'ın.
    let (start_text, shift) = match first.strip_prefix('P') {
        Some(rest) => (rest, 0),
        None => (first, predisplay_chars),
    };
    let start = start_text
        .parse::<usize>()
        .ok()?
        .checked_add(shift)?
        .min(display_chars);
    let end = parts
        .next()?
        .parse::<usize>()
        .ok()?
        .checked_add(shift)?
        .min(display_chars);
    let style = parse_style(parts.next()?);
    (start < end).then_some(Highlight { start, end, style })
}

/// zsh'in adlı renkleri, `HighlightColor::Indexed` sırasıyla.
const HIGHLIGHT_COLOR_NAMES: [&str; 8] = [
    "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
];

/// `fg=red,bold` gibi bir spec'i stile çevirir; tanınmayan bileşen düşer.
fn parse_style(spec: &str) -> HighlightStyle {
    let mut style = HighlightStyle::default();
    for part in spec.split(',') {
        match part {
            "bold" => style.bold = true,
            "underline" => style.underline = true,
            "standout" => style.standout = true,
            _ => {
                if let Some(value) = part.strip_prefix("fg=") {
                    style.fg = parse_highlight_color(value);
                } else if let Some(value) = part.strip_prefix("bg=") {
                    style.bg = parse_highlight_color(value);
                }
            }
        }
    }
    style
}

/// `#rrggbb`, `0`–`255` ya da adlı renk; `default` ve tanınmayan → `None`.
fn parse_highlight_color(value: &str) -> Option<HighlightColor> {
    if let Some(hex) = value.strip_prefix('#') {
        return (hex.len() == 6)
            .then(|| u32::from_str_radix(hex, 16).ok())
            .flatten()
            .map(HighlightColor::Rgb);
    }
    if let Ok(index) = value.parse::<u8>() {
        return Some(HighlightColor::Indexed(index));
    }
    let at = HIGHLIGHT_COLOR_NAMES
        .iter()
        .position(|&name| name == value)?;
    Some(HighlightColor::Indexed(at as u8))
}

/// base64 alfabesinde olmayan baytın tablodaki karşılığı.
const B64_INVALID: u8 = 0xff;

/// `bayt → 6 bit` çözüm tablosu; alfabe dışı her bayt [`B64_INVALID`].
const B64_DECODE: [u8; 256] = {
    let mut table = [B64_INVALID; 256];
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut at = 0;
    while at < alphabet.len() {
        table[alphabet[at] as usize] = at as u8;
        at += 1;
    }
    table
};

/// base64'ü `out`'a çözer; bozuk girdide `None` ve `out` yarım kalabilir
/// (çağıran onu kullanmıyor).
///
/// **Elle yazıldı:** bir base64 crate'i mimari karardır (`proje.md` → Yayın
/// etkisi) ve bu phase onu açmıyor; tablo + `chunks_exact` otuz satır.
///
/// **Dolgu opsiyonel.** Kodlayan taraf saf zsh ve dolgu basmayan bir uygulama
/// da geçerli base64 üretir; dolguyu şart koşmak kanalı kodlayıcının bir
/// uygulama ayrıntısına bağlardı. Dolgudan sonra gövde uzunluğu 4'e bölünmeli
/// ya da 2/3 artık bırakmalı — 1 artık base64 değildir.
fn decode_base64(input: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let body = match input {
        [rest @ .., b'=', b'='] => rest,
        [rest @ .., b'='] => rest,
        rest => rest,
    };
    let mut chunks = body.chunks_exact(4);
    for chunk in chunks.by_ref() {
        let a = b64_value(chunk[0])?;
        let b = b64_value(chunk[1])?;
        let c = b64_value(chunk[2])?;
        let d = b64_value(chunk[3])?;
        // Maskeler **zorunlu**, süs değil: altı bitlik bir değeri maskesiz
        // kaydırmak `u8`'i taşırır ve debug'da panik olur — `bt-core`'da
        // gerekçesiz panik yok (`CLAUDE.md`).
        out.push((a << 2) | (b >> 4));
        out.push(((b & 0x0f) << 4) | (c >> 2));
        out.push(((c & 0x03) << 6) | d);
    }
    match chunks.remainder() {
        [] => Some(()),
        [a, b] => {
            let (a, b) = (b64_value(*a)?, b64_value(*b)?);
            out.push((a << 2) | (b >> 4));
            Some(())
        }
        [a, b, c] => {
            let (a, b, c) = (b64_value(*a)?, b64_value(*b)?, b64_value(*c)?);
            out.push((a << 2) | (b >> 4));
            out.push(((b & 0x0f) << 4) | (c >> 2));
            Some(())
        }
        // Tek artık base64 değildir: altı bit bir bayt etmiyor.
        _ => None,
    }
}

fn b64_value(byte: u8) -> Option<u8> {
    match B64_DECODE[byte as usize] {
        B64_INVALID => None,
        value => Some(value),
    }
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
            scanner.feed(chunk, |event| {
                if let ScanEvent::Mark(mark) = event {
                    seen.push(mark);
                }
            });
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
        scanner.feed(&stream, |event| {
            if let ScanEvent::Mark(mark) = event {
                seen.push(mark);
            }
        });

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
        scanner.feed(&stream, |event| {
            if let ScanEvent::Mark(mark) = event {
                seen.push(mark);
            }
        });

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

    /// Testlerin kodlayıcısı — üretimde karşılığı kabuğun saf zsh kolu.
    fn b64(bytes: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let (a, b, c) = (
                u32::from(chunk[0]),
                chunk.get(1).map_or(0, |&b| u32::from(b)),
                chunk.get(2).map_or(0, |&b| u32::from(b)),
            );
            let word = a << 16 | b << 8 | c;
            for slot in 0..4 {
                if slot <= chunk.len() {
                    out.push(ALPHABET[(word >> (18 - 6 * slot) & 0x3f) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// Ayna dizisi; `highlights` satır sonuyla birleştirilip base64'lenir.
    fn dock_update(
        cursor: usize,
        pre: &str,
        buffer: &str,
        post: &str,
        highlights: &[&str],
    ) -> Vec<u8> {
        format!(
            "\x1b]8133;u;{cursor};{};{};{};{}\x07",
            b64(pre.as_bytes()),
            b64(buffer.as_bytes()),
            b64(post.as_bytes()),
            b64(highlights.join("\n").as_bytes()),
        )
        .into_bytes()
    }

    /// `DockEvent` ödünç verdiği için sınamalar sahiplenilmiş bir kopya tutar.
    #[derive(Debug, PartialEq, Eq)]
    enum DockSnapshot {
        Update(DockState),
        End,
        Unavailable(DockFault),
    }

    fn dock_events_of_chunks(chunks: &[&[u8]]) -> Vec<DockSnapshot> {
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        for chunk in chunks {
            scanner.feed(chunk, |event| {
                if let ScanEvent::Dock(event) = event {
                    seen.push(match event {
                        DockEvent::Update(line) => DockSnapshot::Update(line.clone()),
                        DockEvent::End => DockSnapshot::End,
                        DockEvent::Unavailable(fault) => DockSnapshot::Unavailable(fault),
                    });
                }
            });
        }
        seen
    }

    fn dock_events(bytes: &[u8]) -> Vec<DockSnapshot> {
        dock_events_of_chunks(&[bytes])
    }

    /// Bir güncellemenin tek `DockState`'i; başka bir şey geldiyse düşer.
    fn dock_line(bytes: &[u8]) -> DockState {
        match dock_events(bytes).pop() {
            Some(DockSnapshot::Update(line)) => line,
            other => panic!("güncelleme bekleniyordu, gelen: {other:?}"),
        }
    }

    #[test]
    fn the_dock_arm_decodes_the_five_variables() {
        let line = dock_line(&dock_update(
            3,
            "❯ ",
            "git sta",
            "tus",
            &["0 3 fg=green,bold"],
        ));
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.predisplay, "❯ ");
        assert_eq!(line.buffer, "git sta");
        assert_eq!(line.postdisplay, "tus");
        // `$CURSOR` 3, `PREDISPLAY` iki karakter: görüntü uzayında 5.
        assert_eq!(line.cursor, 5);
        assert_eq!(
            line.highlights,
            vec![Highlight {
                // `PREDISPLAY` iki karakter: öneksiz ofset onun ardından sayılır.
                start: 2,
                end: 5,
                style: HighlightStyle {
                    fg: Some(HighlightColor::Indexed(2)),
                    bold: true,
                    ..HighlightStyle::default()
                },
            }]
        );
    }

    #[test]
    fn highlight_offsets_collapse_into_one_space() {
        // `P` öneki ofseti PREDISPLAY'in başına bağlıyor, öneksizi BUFFER'ın:
        // ikisi de görüntünün başından sayılan tek uzaya iniyor (R1.3).
        let line = dock_line(&dock_update(
            0,
            "ab",
            "cd",
            "",
            &["P0 2 fg=red", "0 2 bg=4"],
        ));
        assert_eq!(line.highlights[0].start, 0);
        assert_eq!(line.highlights[0].end, 2);
        assert_eq!(
            line.highlights[0].style.fg,
            Some(HighlightColor::Indexed(1))
        );
        assert_eq!(line.highlights[1].start, 2);
        assert_eq!(line.highlights[1].end, 4);
        assert_eq!(
            line.highlights[1].style.bg,
            Some(HighlightColor::Indexed(4))
        );
    }

    #[test]
    fn a_highlight_keeps_what_it_understands_and_drops_the_rest() {
        // `memo=` serbest bir kuyruk alanı, `blink` tanımadığımız bir nitelik;
        // ikisi de kaydı düşürmemeli — düşseydi bütün aralık renksiz kalırdı.
        let line = dock_line(&dock_update(
            0,
            "",
            "xy",
            "",
            &["0 2 fg=#ff8800,underline,blink memo=zsh-syntax-highlighting"],
        ));
        assert_eq!(
            line.highlights,
            vec![Highlight {
                start: 0,
                end: 2,
                style: HighlightStyle {
                    fg: Some(HighlightColor::Rgb(0xff8800)),
                    underline: true,
                    ..HighlightStyle::default()
                },
            }]
        );

        // Ters aralık ve okunamayan ofset kaydı düşürüyor, diziyi değil.
        let line = dock_line(&dock_update(0, "", "xy", "", &["5 1 fg=red", "a b fg=red"]));
        assert_eq!(line.highlights, vec![]);
    }

    #[test]
    fn offsets_never_point_past_the_mirrored_text() {
        // Bayat bir `BUFFER` anlık görüntüsünden kurulan `region_highlight`
        // metnin dışını gösterebiliyor; çizen tarafa taşımak orada bir kırpma
        // ya da panik borcu doğururdu.
        let line = dock_line(&dock_update(
            0,
            "ab",
            "cd",
            "",
            &["0 99 fg=red", "50 60 fg=red"],
        ));
        assert_eq!(line.highlights.len(), 1);
        assert_eq!(line.highlights[0].start, 2);
        assert_eq!(line.highlights[0].end, 4);

        // İmleç de kabuğun sözüne bırakılmıyor: en çok `BUFFER`'ın sonu.
        assert_eq!(dock_line(&dock_update(99, "ab", "cd", "ef", &[])).cursor, 4);
    }

    #[test]
    fn the_dock_end_closes_the_mirror() {
        assert_eq!(dock_events(b"\x1b]8133;e\x07"), vec![DockSnapshot::End]);
    }

    #[test]
    fn extra_trailing_fields_are_tolerated() {
        // İleriye dönük alan: phase-4'ün özel kip sinyali bu ayrıştırıcıyı
        // yeniden açmadan eklenebilmeli.
        let mut sequence = dock_update(1, "", "ab", "", &[]);
        sequence.pop();
        sequence.extend_from_slice(b";mode=isearch\x07");
        assert_eq!(dock_line(&sequence).buffer, "ab");
    }

    #[test]
    fn padding_is_optional() {
        // Kodlayan taraf saf zsh; dolguyu şart koşmak kanalı onun bir uygulama
        // ayrıntısına bağlardı.
        let padded = dock_line(&dock_update(0, "", "abcd", "", &[]));
        let bare = dock_line(b"\x1b]8133;u;0;;YWJjZA;;\x07");
        assert_eq!(padded.buffer, "abcd");
        assert_eq!(bare.buffer, "abcd");
    }

    #[test]
    fn a_broken_payload_is_reported_not_panicked() {
        // Üç bozulma, tek yanıt: gösteremiyoruz.
        for sequence in [
            &b"\x1b]8133;u;0;;!!!!;;\x07"[..], // base64 alfabesi dışı
            &b"\x1b]8133;u;0;;YQ;\x07"[..],    // alan eksik
            &b"\x1b]8133;u;abc;;;;\x07"[..],   // imleç sayı değil
            &b"\x1b]8133;u;0;;gA;;\x07"[..],   // geçersiz UTF-8
            &b"\x1b]8133;z\x07"[..],           // tanınmayan işlem
            &b"\x1b]8133;\x07"[..],            // boş yük
        ] {
            assert_eq!(
                dock_events(sequence),
                vec![DockSnapshot::Unavailable(DockFault::Malformed)],
                "dizi: {:?}",
                String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn an_oversized_dock_payload_is_visible_and_the_next_sequence_survives() {
        // 133'ün sessiz düşüşünün aksine aşım çağırana **bir sonuç** döner
        // (R1.2); ardından gelen sağlam dizi yine görülür.
        let mut stream = b"\x1b]8133;u;0;;".to_vec();
        stream.extend(std::iter::repeat_n(b'A', DOCK_PAYLOAD_LIMIT + 1));
        stream.push(0x07);
        stream.extend_from_slice(&dock_update(1, "", "ok", "", &[]));

        let seen = dock_events(&stream);
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0], DockSnapshot::Unavailable(DockFault::Overflow));
        assert!(matches!(&seen[1], DockSnapshot::Update(line) if line.buffer == "ok"));
    }

    /// Kabuk betiğinin (`assets/shell/zsh/bateri.zsh`) yolu.
    ///
    /// Sınama onu **kaynağından** koşturuyor, paketten değil: `make kur`
    /// kopyayı `cmp` ile denetliyor, yani ikisinin aynılığının kapısı orada.
    fn script_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shell/zsh")
    }

    /// Betiğin verilen ZLE durumu için bastığı baytlar.
    ///
    /// **Kodlayan gerçekten zsh.** Bu sınamaların tuttuğu şey `parse_dock`'un
    /// doğruluğu değil — onun kendi sınamaları var — telin **iki ucunun**
    /// aynı biçimi konuşması: kodlayıcı kabukta, çözücü burada ve ikisi ayrı
    /// dillerde yazılı.
    ///
    /// `zsh -f`: kullanıcının hiçbir başlangıç dosyası okunmuyor, yani sonuç
    /// makinede kurulu eklentilerden bağımsız.
    ///
    /// **Değerler ortamdan geçiyor**, betiğe gömülü değil: taşınan şey tam da
    /// `;`, `ESC`, ters bölü ve tırnak gibi baytlar ve onları bir zsh
    /// dizgisine gömmek sınamayı alıntılama kurallarının sınamasına
    /// çevirirdi.
    ///
    /// Betik koşamıyorsa (zsh yok) sınama **düşer**, atlanmaz: bt-core Linux
    /// hedefiyle *derleniyor*, sınamaları macOS'ta koşuyor ve orada
    /// `/bin/zsh` her zaman var.
    fn script_output(
        cursor: usize,
        pre: &str,
        buffer: &str,
        post: &str,
        highlights: &[&str],
    ) -> Vec<u8> {
        run_script(
            "source $ZDOTDIR/bateri.zsh
             PREDISPLAY=$T_PRE BUFFER=$T_BUF POSTDISPLAY=$T_POST CURSOR=$T_CURSOR
             region_highlight=( ${(f)T_HL} )
             __bateri_dock_redraw",
            &[
                ("T_CURSOR", &cursor.to_string()),
                ("T_PRE", pre),
                ("T_BUF", buffer),
                ("T_POST", post),
                ("T_HL", &highlights.join("\n")),
            ],
        )
    }

    fn run_script(body: &str, env: &[(&str, &str)]) -> Vec<u8> {
        let mut command = std::process::Command::new("zsh");
        command
            .args(["-f", "-c", body])
            .env("ZDOTDIR", script_path());
        for (key, value) in env {
            command.env(key, value);
        }
        let output = command.output().expect("zsh koşmadı");
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "betik temiz koşmadı: {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    #[test]
    fn the_script_encodes_what_the_scanner_decodes() {
        // Gövdeler base64 tam da bu baytlar için: `;` alanı, `ESC` diziyi
        // bitirirdi. Ters bölü de burada — kodlayıcının ilk taslağı onu
        // aritmetiğin `##` biçimiyle okuyor ve 92 yerine 32 görüyordu.
        let buffer = "echo 'a;b' \\ \u{1b}[0m çığır";
        let line = dock_line(&script_output(
            5,
            "❯ ",
            buffer,
            " --dry-run",
            &[
                "P0 2 fg=blue",
                "0 4 fg=green,bold memo=zsh-syntax-highlighting",
            ],
        ));

        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.predisplay, "❯ ");
        assert_eq!(line.buffer, buffer);
        assert_eq!(line.postdisplay, " --dry-run");
        // `$CURSOR` 5, `PREDISPLAY` iki karakter: görüntü uzayında 7.
        assert_eq!(line.cursor, 7);
        assert_eq!(line.highlights.len(), 2);
        assert_eq!(line.highlights[0].start, 0);
        assert_eq!(line.highlights[0].end, 2);
        assert_eq!(line.highlights[1].start, 2);
        assert_eq!(line.highlights[1].end, 6);
        assert!(line.highlights[1].style.bold);
    }

    #[test]
    fn the_script_encodes_every_padding_remainder() {
        // base64 üçer bayt öğütüyor; artığı 0, 1 ve 2 olan üç uzunluk da
        // sınanıyor. UTF-8 karakter başına birden çok bayt, yani "karakter
        // sayısı" ile "bayt sayısı" burada ayrışıyor.
        for text in ["abc", "abcd", "abcde", "ç", "çi", "çığ", "😀"] {
            let line = dock_line(&script_output(0, "", text, "", &[]));
            assert_eq!(line.buffer, text, "metin: {text}");
        }
        // Boş görüntü de geçerli: prompt çizilir çizilmez gelen ilk ayna bu.
        let line = dock_line(&script_output(0, "", "", "", &[]));
        assert_eq!(line.buffer, "");
        assert_eq!(line.status, DockStatus::Live);
    }

    #[test]
    fn the_script_closes_the_mirror_when_the_line_is_finished() {
        let bytes = run_script("source $ZDOTDIR/bateri.zsh; __bateri_dock_finish", &[]);
        assert_eq!(dock_events(&bytes), vec![DockSnapshot::End]);
    }

    #[test]
    fn a_line_too_long_to_mirror_is_refused_before_it_is_encoded() {
        // Kabuk tarafındaki kapı: kodlama tuş başına koşuyor ve maliyeti
        // uzunlukla doğrusal, yani terminalin zaten reddedeceği bir yükü
        // kodlamak boşa harcanan zamandır. İki ucun sonucu **aynı** olmalı
        // (`DockFault::Overflow`), yoksa sınırın hangi tarafta tutulduğu
        // kullanıcıya farklı davranış olarak yansırdı.
        let long = "x".repeat(4097);
        assert_eq!(
            dock_events(&script_output(0, "", &long, "", &[])),
            vec![DockSnapshot::Unavailable(DockFault::Overflow)]
        );

        // Sınırın altındaki satır aynada; kapı sessizce daralmıyor.
        let fits = "x".repeat(4096);
        assert_eq!(
            dock_line(&script_output(0, "", &fits, "", &[])).buffer,
            fits
        );

        // **Dördüncü gövde de kapıya tabi.** Sözdizimi vurgusu jeton başına bir
        // kayıt bırakıyor, yani kısa bir metnin yanında `region_highlight`
        // kendi başına sınırı aşabilir; kapı yalnız metni ölçseydi yorumu
        // gerçekten yaptığından fazlasını iddia ederdi.
        let many: Vec<String> = (0..200)
            .map(|at| format!("{at} {at} fg=green memo=zsh-syntax-highlighting"))
            .collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert_eq!(
            dock_events(&script_output(0, "", "ls", "", &many)),
            vec![DockSnapshot::Unavailable(DockFault::Overflow)]
        );
    }

    #[test]
    fn the_two_arms_do_not_touch_each_others_buffers() {
        // Aynanın geniş sınırı 133'ün dar sınırını gevşetmemeli; 133'ün dar
        // sınırı da aynayı kesmemeli. Tamponların ayrı olmasının kanıtı.
        let mut scanner = Scanner::new();
        let mut marks = Vec::new();
        let mut lines = Vec::new();
        let mut stream = dock_update(2, "", "ls", "", &[]);
        stream.extend_from_slice(b"\x1b]133;B\x07");
        stream.extend_from_slice(&dock_update(3, "", "lsx", "", &[]));
        scanner.feed(&stream, |event| match event {
            ScanEvent::Mark(mark) => marks.push(mark),
            ScanEvent::Dock(DockEvent::Update(line)) => lines.push(line.buffer.clone()),
            ScanEvent::Dock(_) => {}
        });

        assert_eq!(marks, vec![Mark::PromptEnd]);
        assert_eq!(lines, vec!["ls".to_string(), "lsx".to_string()]);
        assert_eq!(scanner.payload.capacity(), PAYLOAD_LIMIT);
        assert_eq!(scanner.dock.capacity(), DOCK_PAYLOAD_LIMIT);
    }

    #[test]
    fn a_dock_sequence_split_at_every_byte_survives() {
        let sequence = dock_update(1, "p", "ab", "c", &["0 1 fg=red"]);
        let expected = dock_line(&sequence);
        for at in 0..=sequence.len() {
            let (head, tail) = sequence.split_at(at);
            let seen = dock_events_of_chunks(&[head, tail]);
            assert_eq!(
                seen,
                vec![DockSnapshot::Update(expected.clone())],
                "bölünme noktası {at}"
            );
        }
    }

    #[test]
    fn the_log_clears_the_mirror_when_it_cannot_be_drawn() {
        // Bayat metin bırakmak, ızgara bastırılırken dock'un bir önceki
        // komutu göstermesi demek olurdu.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let staged = DockState {
            status: DockStatus::Live,
            buffer: "git status".to_string(),
            cursor: 10,
            ..DockState::default()
        };
        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        assert_eq!(log.dock.buffer, "git status");

        log.apply_scan(ScanEvent::Dock(DockEvent::Unavailable(DockFault::Overflow)));
        assert_eq!(
            log.dock.status,
            DockStatus::Unavailable(DockFault::Overflow)
        );
        assert_eq!(log.dock.buffer, "");
        assert_eq!(log.dock.cursor, 0);

        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        log.apply_scan(ScanEvent::Dock(DockEvent::End));
        assert_eq!(log.dock.status, DockStatus::Idle);
        assert_eq!(log.dock.buffer, "");
    }

    #[test]
    fn the_mirror_reuses_its_buffers() {
        // R1.3'ün ölçütü: sabit durumda tuş başına ayırma yok. Kapasitenin
        // ikinci turda büyümemesi bunun gözlenebilir yüzü.
        let mut scanner = Scanner::new();
        let long = "x".repeat(200);
        let sequence = dock_update(0, "", &long, "", &[]);
        scanner.feed(&sequence, |_| {});
        let capacity = scanner.line.buffer.capacity();
        for _ in 0..10 {
            scanner.feed(&sequence, |_| {});
        }
        assert_eq!(scanner.line.buffer.capacity(), capacity);
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
