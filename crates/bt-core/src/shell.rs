//! Kabuğun bastığı OSC işaretleri, onların tuttuğu oturum durumu, **komut
//! bloğu defteri** ve **ZLE'nin görüntü aynası**.
//!
//! Tarayıcının **üç OSC kolu** var ve üçü de aynı bayt akışından besleniyor:
//! [`MARK_OSC`] oturumun safhasını ve blok kimliklerini taşır, [`DOCK_OSC`]
//! satır düzenleyicinin (ZLE) o anki görüntüsünü, [`CWD_OSC`] de çalışma
//! dizinini. Üçü tek durum makinesinde, çünkü akış tek: ayrı tarayıcılar aynı
//! diziyi üç kez çerçevelerdi ve çerçeveleme kuralının (aşağıdaki üç madde)
//! üç kopyası doğardı.
//!
//! **Dördüncü kol OSC değil CSI** ve ötekilerden iki yanıyla ayrılıyor:
//! tanıdığı tek dizi `CSI 2 J`, ve **yükü yok**. Tuttuğu şey bir yük değil bir
//! sayı — kaç kez "ekranı kasten temizle" geçtiği ([`Scanner::take_screen_clears`]).
//! Aynı durum makinesinde, çünkü çerçeveleme yine tek: bozuk bir CSI'da takılıp
//! kalan bir tarayıcı peşinden gelen `ESC ] 133;…`'ü yutar ve bloklar,
//! bastırma, dock **sessizce** ölürdü. `ESC [` bugüne kadar `Ground`'a
//! düşüyordu; o kol ancak aranan diziyi görmediği için zararsızdı, yoksa
//! bir CSI'nın içindeki `]` bizde yeni bir OSC açabilirdi.
//!
//! Neden bu diziyi terminalin **kendisi** izliyor: alacritty
//! `ClearMode::All`'ü birincil ekranda `clear_viewport()` ile karşılıyor
//! (`term/mod.rs:1794`), yani görünen satırları **geçmişe itiyor**. Ekran
//! boşalıyor ama `history_size()` büyüyor; "boşluğu geçmişle doldur" kuralı
//! (017) onu ayırt edemezse Ctrl-L'i geri alırdı.
//!
//! Beş sorumluluk, tek modül: baytlardan işaret çıkarmak ([`parse_mark`]),
//! işaretlerden oturum safhası tutmak ([`ShellState`]), blok kimliği başına
//! akıbet tutup şeridin çizilip çizilmeyeceğine karar vermek ([`BlockLog`],
//! [`ShellLog::stripe`]), aynanın beş değişkenini çözülmüş bir kayda
//! indirmek ([`DockState`]) ve dock'un bağlam satırını — dizin ile dal —
//! tutmak ([`DockContext`]). Beşi aynı yerde, çünkü beşini de **aynı** işaret
//! akışı besliyor; ayrılsalardı `D`'nin çıkış kodu bir modülden ötekine elden
//! ele geçerdi. Defterin tavanı `scrollback`'ten türüyor ve "bilinmeyen
//! kimlik çizilmez" kararı da burada — renderer'ın göreceği tek şey çözülmüş
//! renk.
//!
//! **Ayna ile bağlamın ömrü ayrı** ve bu ayrım tiplere yazılı: [`DockState`]
//! tuş başına geliyor ve `line-finish`'te sıfırlanıyor, [`DockContext`]
//! prompt başına geliyor ve komut koşarken de ekranda kalıyor.
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
//! Baytları ayrıştırıcıya giderken tarıyoruz. Aynı gerekçe [`DOCK_OSC`] ve
//! [`CWD_OSC`] için de geçerli: `vte` ikisini de **tanımıyor**, yükü
//! `osc_dispatch`'in `_` koluna düşürüp atıyor (`vte-0.15.0/src/ansi.rs`,
//! `unhandled`; yorumladığı numaralar 0, 2, 4, 8, 10–12, 22, 50, 52, 104 ve
//! 110–112). Yük bütünüyle oraya ulaşıyor — `osc_raw` `std` altında sınırsız
//! bir `Vec` ve 1024'lük `MAX_OSC_RAW` yalnız `no_std` kolunda geçerli — ama
//! ulaştığı yerde okunmuyor. Dizin için bu, alacritty'nin bir olayının
//! **düşmesi** değil: OSC 7 hiçbir olay doğurmuyor, yani `Event::Title`'ın
//! boş kolu bu kolun yerine geçemezdi.
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
use std::fmt::{self, Write as _};
use std::path::Path;
use std::time::{Duration, Instant};

use unicode_width::UnicodeWidthChar;

use crate::dock::{self, DockPoint};
use crate::session::{CellHalf, SelectKind};
use crate::settings::{HostMark, HostRule};

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
    /// `D` geldi; kabuk kodu okunamayacak şekilde bastıysa `exit` `None`.
    Finished {
        exit: Option<i32>,
        /// `C` ile `D` arasında geçen süre, milisaniye.
        ///
        /// Bloğun **kendi içinde**, yan tabloda değil: akıbetle aynı ömre
        /// sahip ve halkanın tahliyesi ikisini birlikte atıyor. `u32` tavanı
        /// 49 gün; ondan uzun süren komutun sayacı doyuyor, sarmıyor.
        ///
        /// Süreyi hiç görmemiş blokta sıfır: `C` gelmeden `D` gelirse
        /// (kimliksiz `A`'dan sonra gelen `D`, ya da entegrasyonun yarısı)
        /// uydurulmuş bir süre yazmak yerine eşiğin altına düşülüyor, yani
        /// sayaç çizilmiyor.
        ///
        /// **Bilinen sınır: ölçülen şey komutun kendisi değil, `C` ile `D`
        /// arası.** İki işareti de basan kancalarımız `add-zsh-hook` ile
        /// **sona** ekleniyor (gerekçeleri `bateri.zsh`'te: `preexec`'te
        /// çıpanın kapanışı, `precmd`'de `psvar` yuvası), yani kullanıcının
        /// kendi kancaları ikisinden de önce koşuyor. Sonuç iki yönlü ve
        /// kısmen birbirini götürüyor: `C` geç basılıyor (süre kısalır), `D`
        /// kullanıcının `precmd`'lerinden sonra basılıyor (süre uzar). Pay
        /// kancaların süresi kadar — starship gibi prompt başına bir binary
        /// koşturan kurulumda on milisaniyeler.
        ///
        /// **Kendi işimiz payın içinde değil:** `D` `precmd`'in ilk işi —
        /// dalın `git` fork'undan, OSC 7'den ve `psvar`'dan **önce**.
        elapsed_ms: u32,
    },
}

/// Defterin girdi başına bütçesi — [`BlockLog`]'un doc'undaki sayının
/// **doğrulanmış** hâli.
///
/// Rust `Option<i32>`'nin etiketindeki niche'i [`Outcome`]'ın ayrımı için
/// kullanıyor, yani boyut elle toplanabilir bir sayı değil (elle 16 çıkıyor ve
/// ilk yazımda öyle yazılmıştı): `Finished`'a bir alan eklemek 12'yi sessizce
/// büyütür ve 10 000 satırlık scrollback'te sekme başına ödenen bellek de
/// öyle. Assert kırılınca hem burası hem `BlockLog`'un bütçe cümlesi aynı
/// commit'te güncellenir.
const _: () = assert!(size_of::<Outcome>() == 12);

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
/// Beş görüntü değişkeni taşınıyor ([`DOCK_OSC`]'un yükü; yanlarında
/// `KEYMAP` ve 032'den beri `PREBUFFER`) ve burada dizgilere, bir sütuna ve
/// bir aralık listesine iniyor. Yalnız `BUFFER` taşınsaydı bastırma
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
    /// `PREBUFFER` — çok satırlı bir komutun ZLE'nin **kabul ettiği** önceki
    /// satırları (`for`, heredoc, `\`-devam); her zaman `\n`'le bitiyor ve
    /// artık düzenlenemiyor. Aynanın yedinci, **isteğe bağlı** gövdesi (032);
    /// eski betikte boş.
    ///
    /// **Görüntü uzayının dışında:** [`Self::cursor`], [`Self::display_chars`],
    /// [`Self::last_ink`] ve `region_highlight` onu saymıyor — zsh'in
    /// uzayları da saymıyor. Dock onu düzenlenebilir satırların **üstünde**
    /// çiziyor, seçilebilir ve kopyalanabilir ama salt okunur
    /// ([`crate::dock::dock_layout`]'un akışı `PREBUFFER ++ görüntü`); dolu
    /// olması bastırmanın üst tabanını çıpanın satırına indiriyor
    /// ([`SuppressedInput::from_anchor`]).
    pub prebuffer: String,
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
    /// Görüntünün **son satırının** son boşluk olmayan karakteri; o satır
    /// boşsa (`echo a\n`'in ardı da) `None`.
    ///
    /// Bastırmanın **tazelik kapısı** bunu kullanıyor: ızgaradaki giriş
    /// satırının **son** satırının son mürekkepli hücresiyle karşılaştırılıyor ve uyuşmazsa
    /// ayna bayat sayılıp bastırma bırakılıyor. Boşluk **dışlanıyor**, çünkü
    /// boşluk hücresi sınırdan hiç geçmiyor (`frame()`'in atlama kapısı) ve
    /// `ls ` yazan kullanıcıda her karede yanlış alarm verirdi.
    ///
    /// Burada saklanıyor, çünkü çözücünün metni zaten elinde; kare başına
    /// yeniden taramak `Term` kilidi öncesine O(n) eklerdi.
    pub last_ink: Option<char>,
    /// ZLE **ekleme** keymap'inde mi: basılan basılabilir tuş metne dönüşüyor
    /// mu ([`INSERT_KEYMAPS`]).
    ///
    /// Tek tüketicisi yapıştırmanın dar istisnası
    /// ([`crate::Session::can_be_typed`]) ve orada zorunlu: istisnanın bütün
    /// gerekçesi "bu metni kullanıcı elle yazsa aynı sonucu verirdi" ve o
    /// cümle yalnız ekleme keymap'inde doğru. `vicmd`'de aynı baytlar komut —
    /// panodaki `dd` satırı siler.
    ///
    /// **Ad değil `bool`:** sınırdan çözülmüş geçiyor (`DockState`'in geri
    /// kalanıyla aynı kural) ve adı saklamak kare başına bir `String` daha
    /// tutmak olurdu. Sınıflandırma çözme anında, tek yerde.
    ///
    /// Varsayılanı `false` ve bu **güvenli yön**: alanı hiç göndermeyen eski
    /// bir betikle koşan pencere (`plan.md` → Göç) istisnayı kaybeder, yani
    /// sarılı yapıştırmaya — phase-5 öncesinin davranışına — döner.
    pub insert_keymap: bool,
    /// Bu aynanın **cevap verdiği** kullanıcı girdisi: ayna çözüldüğü anda
    /// okunan girdi nesli (`Session`'ın `key_gen`'i).
    ///
    /// Tazelik kapısının zamansal yarısı: nesil o andan beri ilerlemediyse
    /// kullanıcının son girdisinin aynası gelmiş demektir ve ızgaranın ne
    /// dediğine bakmaya gerek yok. **İçeriğin yanında** duruyor, serbest bir
    /// bayrakta değil: kare yolu onu metinle aynı yaprak kilit turunda
    /// okuyor, yani bayat bir okuma bayat damgayı da beraberinde getiriyor ve
    /// kapı içerik karşılaştırmasına düşüyor — yanlışın yönü güvenli
    /// (`.tasks/025-tazelik-zamansal/discussion.md` → Muhakeme).
    ///
    /// Yazan tek yer [`ShellLog::apply_scan_answering`]; tarayıcının
    /// sahnelediği kopyada anlamsız ve sıfır.
    ///
    /// **`Idle` ayna da damgalı** (`End` kolu, 030): dock'un yazım
    /// animasyonları ([`crate::DockEdit`]) canlanacak glyph sayısını bu
    /// damganın farkıyla sınırlıyor ve Enter'dan sonraki ilk tuşun tabanı
    /// `Idle` ayna. Sıfır damgalı bir taban o sınırı boşa düşürür, prompt'taki
    /// ilk yapıştırma harf harf canlanırdı. Tazelik kapısı `Idle`'ı hiç
    /// okumuyor ([`ShellLog::suppressed_input`] `Live` ister), yani ona etkisi
    /// yok.
    pub answers: u64,
    /// Ayna kümeyle mi okunuyor (035, `SessionOptions::cluster`): dock'un
    /// düzeni ([`crate::dock::layout_with`]) ve [`Self::last_ink`] emoji
    /// dizisini tek küme sayıyor.
    ///
    /// **Aynanın içeriği değil, okunuşu** ve oturum boyunca sabit: tarayıcı
    /// kendi kopyasına açılışta yazıyor ([`Scanner::cluster`]), `clone_from`
    /// taşıyor, [`Self::reset`] dokunmuyor. Burada durmasının sebebi
    /// tüketicilerin hepsinin elinde zaten bu kayıt olması — dock'un çizimi,
    /// isabet testi, satır sayısı ve tazelik kapısı; ayrı bir argüman o
    /// imzaların hepsine bir parametre eklerdi.
    pub cluster: bool,
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
        self.prebuffer.clear();
        self.prebuffer.push_str(&source.prebuffer);
        self.cursor = source.cursor;
        self.display_chars = source.display_chars;
        self.last_ink = source.last_ink;
        self.insert_keymap = source.insert_keymap;
        self.answers = source.answers;
        self.cluster = source.cluster;
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
        self.prebuffer.clear();
        self.cursor = 0;
        self.display_chars = 0;
        self.last_ink = None;
        // Güvenli yön: gösteremediğimiz bir satırın keymap'i de bilinmiyor ve
        // "bilmiyorum" yapıştırmayı sarılı yola göndermeli.
        self.insert_keymap = false;
        self.answers = 0;
        self.highlights.clear();
    }
}

/// Dock'un **bağlam satırı**: çalışma dizini ve git dalı.
///
/// [`DockState`]'ten **ayrı bir tip** ve bu ayrım zorunlu, bir düzen tercihi
/// değil: ayna tuş başına geliyor ve `line-finish`'te sıfırlanıyor
/// ([`DockState::reset`]), bağlam ise **prompt başına** geliyor ve komut
/// koşarken de ekranda kalmak zorunda. Tek tipte dursalardı aynanın her
/// sıfırlaması bağlamı da silerdi — kullanıcı Enter'a bastığı anda dizin
/// kaybolurdu. Ayrıca aynanın kaydı tarayıcıda `clone_from` ile toptan
/// tazeleniyor ve dizin **başka bir koldan** (OSC 7) geliyor: tek tipte
/// her ayna güncellemesi dizini üstüne yazardı.
///
/// [`DockState`] ile aynı tampon disiplini: `clone_from` kapasiteleri
/// koruyor, yani kare başına ayırma yok.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct DockContext {
    /// Kabuğun çalışma dizini, **tam yol**; OSC 7'den geliyor ve hiç
    /// gelmediyse boş.
    pub cwd: String,
    /// Git dalı; depo değilse ya da okunamadıysa **boş**. Detached HEAD'de
    /// dal yerine kısa SHA — kabuk hangisi olduğunu söylemiyor, yalnız
    /// gösterilecek adı gönderiyor.
    pub branch: String,
    /// Uzak oturumun hedefi (037 Karar 1: host, tür, yeniden koşturulacak
    /// argv ve satırı); uzak oturum yoksa `None` (036).
    ///
    /// Yazarı `bt-shell`'in süreç tablosu yoklaması
    /// ([`crate::Session::set_remote`]); `C`, `D` ve `A`'da kendiliğinden
    /// siliniyor ([`ShellLog::apply`]). Bağlamın içinde, çünkü kare yolu
    /// bağlamı kilidin altında `clone_from` ile alıyor ve çizimi kilitten
    /// sonra yapıyor: `ShellLog`'un kendi alanı olsaydı ya kare başına bir
    /// `String` ayırmak ya da kilidi çizim boyunca tutmak gerekirdi.
    pub remote: Option<RemoteTarget>,
    /// Etkin uzak host'un **çözülmüş** işareti (037 Karar 2); yalnız
    /// [`Self::remote`] doluyken anlamlı, yerelde [`HostMark::None`].
    ///
    /// Desen burada değil `ShellLog`'da ve çözüm iki kenarda (uzak durumun
    /// ve listenin değişimi): kare yolu desen görmüyor, yalnız bunu okuyor.
    pub remote_mark: HostMark,
    /// Uzak tarafın OSC 7 dizini (036 Karar 4); gelmediyse boş. **Yalnız
    /// [`Self::remote`] doluyken okunuyor** — etkin değilken de yazılıyor
    /// (yabancı yetkili OSC 7), yoklama OSC 7'den sonra sonuçlanabilsin diye.
    pub remote_cwd: String,
}

impl Clone for DockContext {
    fn clone(&self) -> Self {
        let mut fresh = Self::default();
        fresh.clone_from(self);
        fresh
    }

    fn clone_from(&mut self, source: &Self) {
        self.cwd.clear();
        self.cwd.push_str(&source.cwd);
        self.branch.clear();
        self.branch.push_str(&source.branch);
        // Kapasite korunuyor: ayırma yalnız uzak oturumun **kenarında**
        // (`Option::clone_from` `Some`/`Some`'da `RemoteTarget::clone_from`'a
        // iniyor).
        self.remote.clone_from(&source.remote);
        self.remote_mark = source.remote_mark;
        self.remote_cwd.clear();
        self.remote_cwd.push_str(&source.remote_cwd);
    }
}

impl DockContext {
    /// Uzak oturumun host'u; yerelde `None`.
    pub fn remote_host(&self) -> Option<&str> {
        self.remote.as_ref().map(|target| target.host.as_str())
    }

    /// Uzak durumu siler; **başlığın girdisi değiştiyse** (host vardı)
    /// `true`. Uzak yuva da gidiyor: bir sonraki oturumun dizini değil.
    fn clear_remote(&mut self) -> bool {
        self.remote_cwd.clear();
        self.remote_mark = HostMark::None;
        self.remote.take().is_some()
    }
}

/// Uzak oturumun türü (037 Karar 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RemoteKind {
    #[default]
    Ssh,
    Mosh,
}

/// Uzak oturumun hedefi — yoklamanın bulduğu, bütün olarak (037 Karar 1).
///
/// ⌘T ve yeniden bağlanma aynı komutu **yeniden koşturuyor**: host tek
/// başına yetmiyor (port, `-i`, `-J` olmadan ikinci bağlantı kurulamaz).
/// `bt-core` pid ya da `libc` görmüyor, taşınan şey dizgi; kaçırma kuralı da
/// `bt-shell`'in (`quote`), burada yalnız sonucu ([`Self::line`]) duruyor.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RemoteTarget {
    /// Kullanıcının yazdığı gibi (`prod`, `deploy@10.0.0.5`); `ssh://`
    /// şeması ve port atılmış (036 Karar 3).
    pub host: String,
    pub kind: RemoteKind,
    /// Yeniden koşturulacak argv — ssh'ta yerel yönlendirmeler (`-L -R -D`),
    /// `-M` ve `-f` ayıklanmış; mosh'ta `mosh` + betiğin argümanları.
    pub argv: Vec<String>,
    /// [`Self::argv`]'nin kabuk için kaçırılmış, okunur satırı.
    pub line: String,
}

#[cfg(test)]
impl RemoteTarget {
    /// Sınamaların hedefi: `ssh {host}`.
    pub(crate) fn ssh(host: &str) -> Self {
        Self {
            host: host.to_owned(),
            kind: RemoteKind::Ssh,
            argv: vec!["ssh".to_owned(), host.to_owned()],
            line: format!("ssh {host}"),
        }
    }
}

impl Clone for RemoteTarget {
    fn clone(&self) -> Self {
        let mut fresh = Self::default();
        fresh.clone_from(self);
        fresh
    }

    /// Kare yolu bağlamı her karede `clone_from` ile alıyor: türetilmiş
    /// `Clone`'un varsayılanı (`*self = source.clone()`) her karede argv'yi
    /// ve iki dizgiyi yeniden ayırırdı. `Vec<String>::clone_from` öğelerin
    /// kapasitesini koruyor.
    fn clone_from(&mut self, source: &Self) {
        self.host.clone_from(&source.host);
        self.kind = source.kind;
        self.argv.clone_from(&source.argv);
        self.line.clone_from(&source.line);
    }
}

/// Pencerenin (ve native sekmenin) başlığı — öncelik sırası
/// `.tasks/026-sekmeler/discussion.md` → Karar 7.
///
/// 1. **Uygulamanın OSC 0/2 başlığı** (vim, ssh, Claude Code, oh-my-zsh'in
///    `termsupport`'u). Boş başlık yok sayılır: `\e]2;\a` bir başlık değil,
///    sekmeyi adsız bırakırdı.
/// 2. **Çalışma dizininin son bileşeni** (OSC 7); ev dizininin kendisi `~`,
///    kök `/`. Alt dizin `~/proj` değil `proj` — sekme dar ve ayırt edici
///    olan son bileşen.
/// 3. `bateri`.
///
/// **Uzak oturum etkinken** (036 Karar 5, `remote` = host) başlık
/// [`crate::dock::REMOTE_MARK`] önekini taşıyor: `⇄ {OSC başlığı}`, başlık
/// yoksa `⇄ {host}`; yerel dizin hiç sorulmuyor. Önek koşulsuz, çünkü uzak
/// kabukların çoğu başlığa `user@host: dir` basıyor ve sekmeler arasında
/// uzağı ayırt eden şey o; alternatif ekranda (uzakta vim) dock kalktığı için
/// göstergeyi yalnız başlık taşıyor.
///
/// Saf ve üç yuvadan beslenir; okuyan [`crate::Session::title`]. Ev dizini
/// argüman, çünkü bu crate ortam okumaz — değeri uygulama veriyor
/// ([`crate::SessionOptions::home`]).
pub(crate) fn title_of(
    osc_title: Option<&str>,
    cwd: Option<&str>,
    home: Option<&Path>,
    remote: Option<&str>,
) -> String {
    let osc_title = osc_title.filter(|title| !title.trim().is_empty());
    if let Some(host) = remote {
        let mark = crate::dock::REMOTE_MARK;
        return format!("{mark} {}", osc_title.unwrap_or(host));
    }
    if let Some(title) = osc_title {
        return title.to_owned();
    }
    let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty()) else {
        return "bateri".to_owned();
    };
    let path = Path::new(cwd);
    if home.is_some_and(|home| home == path) {
        return "~".to_owned();
    }
    match path.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        // Son bileşeni olmayan mutlak yol yalnız kök: `file_name` `/` için
        // `None` veriyor. Tarayıcı yalnız mutlak yol geçiriyor, yani göreli
        // bir `..` buraya düşmez; düşse de yolun kendisi dürüst bir başlık.
        None => cwd.to_owned(),
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
    /// Ayna **okundu ve geçerli**, ama görüntü dock'un **çizmediği** bir
    /// kontrol karakteri taşıyor (`Ctrl-V Ctrl-A`'nın `\x01`'i): dock o
    /// sütunu boş bırakırdı, ZLE ise ızgarada okunur bir `^A` basıyor.
    ///
    /// Veri sağlam, **yüzey dar**; gösteremediğimiz satır da caret'i de
    /// ızgarada kalır. (032'ye kadar bir kardeşi vardı, satır sonlu görüntü
    /// `Multiline`; dock çok satırı çizmeyi öğrenince kalktı ve `\n` bu kolun
    /// kontrol karakteri sayılmıyor.)
    /// Bu kol gelmeden önce kontrol karakterinin akıbeti tazelik kapısının
    /// **tesadüfüne** kalıyordu: `^A` satırın son karakteriyse iki taraf
    /// uyuşmuyor ve satır ızgarada kalıyordu, ama ortadaysa (`\x01foo`) iki
    /// taraf da `'o'` diyor, kapı geçiyor ve satır dock'a gidiyordu — `^A`'nın
    /// sütunu boş, yani kullanıcı yazdığını **hiçbir yerde** görmüyordu. Kol
    /// kararı konumdan bağımsız kılıyor (025, `discussion.md` → Karar 1).
    ///
    /// **Sekme bu kolun dışında** ve gerekçe bilgi: sekme bir şey söylemiyor,
    /// dock'taki boş sütunu kayıp değil — ızgarada da boşluğa açılıyor.
    /// İstisnasız Ctrl-V Tab satırı dock'tan ızgaraya düşerdi.
    ///
    /// **Dönüş kuralı kendiliğinden:** durum her ayna yükünde yeniden
    /// hesaplanıyor, yani kontrol karakteri silinince bir sonraki aynada
    /// `Live`. Tuş başına değil satırın şekline bağlı olması şart — "bir
    /// sonraki tuşta dön" deseydi caret ızgara ile dock arasında gidip gelirdi. **Kolun ömrü bir yer tutucuya bağlı**: dock
    /// kontrol karakterini zsh gibi `^X` diye çizdiği gün bu kol silinir
    /// (`docs/YOL-HARITASI.md`).
    Control,
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
/// Kayıt başına 12 bayt: varsayılan 10 000 satırda 120 KB. (013'e kadar 8
/// bayttı; [`Outcome::Finished`] çıkış kodunun yanına geçen süreyi de aldı.
/// Sayı [`Outcome`]'ın yanındaki `const` assert ile bağlı — yazılıp
/// doğrulanmamış bir bütçe tam da bu satırda sessizce eskirdi.)
///
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

    /// `D` ile kapanan bloğun kodunu ve süresini işler; defterde olmayan
    /// kimlik yoksayılır.
    fn finish(&mut self, id: u32, exit: Option<i32>, elapsed_ms: u32) {
        if let Some(at) = self.index_of(id) {
            self.entries[at] = Outcome::Finished { exit, elapsed_ms };
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

/// Dock'un giriş satırındaki fareyle seçim (031 phase-4): iki uç, adım ve
/// çözülmüş aralık — **`BUFFER`'ın karakter indeksleriyle**.
///
/// **Aynanın yanında yaşıyor, içinde değil** ([`ShellLog::dock_selection`]).
/// [`DockState`]'in içinde dursaydı kare yolunun farkı (`dock::change` /
/// `diff`) onu da karşılaştırır ve her sürükleme adımı 030'un yazım
/// efektlerini `Reset`'lerdi; üstelik tarayıcı aynayı toptan `clone_from`
/// ile tazeliyor ve seçimi her tuşta ezerdi. Seçim yine de aynaya **bağlı**:
/// `BUFFER` değişince kalkıyor ([`ShellLog::apply_dock`]), çünkü indeksler
/// artık başka bir metni gösterirdi.
///
/// Aralık uçlar değiştiğinde bir kez çözülüyor (`dock::selection_range`) ve
/// burada saklanıyor: kare yolu kelime aramıyor, yalnız iki sayı okuyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DockSelection {
    /// Basışın noktası; sürükleme ve Shift+tıklama onu taşımıyor.
    anchor: DockPoint,
    /// Sürüklemenin ucu.
    head: DockPoint,
    kind: SelectKind,
    /// `[start, end)`; boş seçimde `start == end`.
    range: (usize, usize),
    /// Aynanın kümeleme bayrağı ([`DockState::cluster`]): uçlar ve ⇧←/⇧→
    /// adımı küme sınırında (035 R4.2). Seçimle birlikte taşınıyor, yani
    /// uzatma onu yeniden sormuyor.
    cluster: bool,
}

impl DockSelection {
    /// `buffer` seçimin ait olduğu `BUFFER`: aralık ona karşı çözülüyor.
    pub(crate) fn new(
        kind: SelectKind,
        anchor: DockPoint,
        head: DockPoint,
        buffer: &str,
        cluster: bool,
    ) -> Self {
        Self {
            anchor,
            head,
            kind,
            range: dock::selection_range(buffer, kind, anchor, head, cluster),
            cluster,
        }
    }

    /// Ucu `head`'e taşır; çapa ve adım yerinde (sürükleme, Shift+tıklama).
    pub(crate) fn extended(self, head: DockPoint, buffer: &str) -> Self {
        Self::new(self.kind, self.anchor, head, buffer, self.cluster)
    }

    /// Seçili aralık; boşsa `None` — sürüklemesiz tık hiçbir şey seçmez.
    pub(crate) fn range(&self) -> Option<(usize, usize)> {
        (self.range.0 < self.range.1).then_some(self.range)
    }

    /// Sürüklemesiz **tek tıklamanın** düştüğü sınır — tıkla-caret'in hedefi
    /// (031 R4.1). Yalnız boş `Simple` seçimde: çift ve üçlü tıklama caret'i
    /// oynatmıyor. Sürüklenip başladığı yere geri getirilen fare de boş
    /// `Simple` bırakıyor ve caret'i taşıyor — metin alanlarının davranışı.
    pub(crate) fn click(&self) -> Option<usize> {
        (self.kind == SelectKind::Simple && self.range.0 == self.range.1).then_some(self.range.0)
    }

    /// ⇧← / ⇧→ (031 Karar 8): seçimin **hareketli ucunu** bir karakter
    /// oynatır; seçim yoksa `caret`'ten başlar. Sonuç her zaman `Simple` —
    /// kelime ya da satır adımıyla başlamış seçim klavyede harf adımıyla
    /// büyüyor (metin alanlarının davranışı).
    ///
    /// **Hareketli uç** başın çapaya göre yönünden: çapanın solundaysa
    /// aralığın başı, değilse sonu. Boş aralıkta (klavyeyle daraltılmış seçim)
    /// iki uç aynı nokta ve adım oradan.
    ///
    /// Adım **karakter** ama birleştirici ile tabanı arasına düşmüyor:
    /// `dock::selection_range`'ın `boundary` kuralının klavyedeki hâli, yoksa
    /// `é`'nin aksanı tabanından ayrı seçilebilirdi. Kümeleme açıkken
    /// (`cluster`, 035) adım **küme**: `🇹🇷`'nin yarısı seçilemiyor.
    pub(crate) fn stepped(
        current: Option<Self>,
        caret: usize,
        forward: bool,
        buffer: &str,
        cluster: bool,
    ) -> Self {
        let chars: Vec<char> = buffer.chars().collect();
        let len = chars.len();
        let (fixed, active) = match current {
            Some(selection) => {
                let (start, end) = selection.range;
                // Yarı sıralanmıyor (`CellHalf` `Ord` değil): sol < sağ.
                let order = |point: DockPoint| (point.index, point.half == CellHalf::Right);
                let backward = order(selection.head) < order(selection.anchor);
                if backward { (end, start) } else { (start, end) }
            }
            None => {
                let caret = caret.min(len);
                // Caret bir kümenin **içindeyse** (ZLE oraya koyabiliyor) ⇧←'in
                // sabit ucu kümenin arkası: yoksa `boundary` onu kümenin başına
                // indirir ve ilk adım boş bir seçim verirdi (`/code-review`).
                let fixed = if cluster && !forward {
                    dock::cluster_span(chars.iter().copied(), caret, true)
                        .filter(|&(start, _)| start < caret)
                        .map_or(caret, |(_, end)| end)
                } else {
                    caret
                };
                (fixed, caret)
            }
        };
        let zero_width = |index: usize| {
            chars
                .get(index)
                .is_some_and(|&ch| dock::column_width(ch) == 0)
        };
        let mut moved = active;
        let span = |index| dock::cluster_span(chars.iter().copied(), index, true);
        if cluster {
            // Hareketli uç bir kümenin sınırında (aralık [`boundary`]'den);
            // bir sonraki sınır kümenin arkası, bir önceki öncekinin başı.
            moved = if forward {
                span(moved).map_or(moved, |(_, end)| end)
            } else {
                moved
                    .checked_sub(1)
                    .and_then(span)
                    .map_or(moved, |(start, _)| start)
            };
        } else if forward {
            if moved < len {
                moved += 1;
                while moved < len && zero_width(moved) {
                    moved += 1;
                }
            }
        } else if moved > 0 {
            moved -= 1;
            while moved > 0 && zero_width(moved) {
                moved -= 1;
            }
        }
        let point = |index| DockPoint {
            index,
            half: CellHalf::Left,
        };
        Self::new(
            SelectKind::Simple,
            point(fixed),
            point(moved),
            buffer,
            cluster,
        )
    }
}

/// Okuyucu thread'in yazdığı, kare yolunun okuduğu kabuk defteri.
///
/// İki kayıt **tek** yaprak kilidin altında: ikisini de besleyen aynı işaret
/// akışı ve ikisini de okuyan aynı kare. Ayrı kilitler, aynı kareyi bir
/// işaretin iki yarısı arasında yakalayabilirdi.
/// Düzenleme komutunun beklenen sonucu: `BUFFER` ve caret, komutun
/// gönderildiği **nesille** damgalı.
///
/// Basılı ⌫'nin tekrarı aynadan hızlı gelebiliyor; kapı bayat aynaya
/// bakıp kapansaydı tekrar ZLE'ye kod noktası olarak gider ve `🇹🇷🇺🇸`'de
/// ikinci ⌫ yalnız `🇷`'yi silerdi. Komutun etkisini biz tanımlıyoruz
/// (`d;S;E;L`: `[S,E)` silinir, caret `S`), yani sonuç kesin; yanlış çıktığı
/// tek yol widget'ın komutu reddetmesi ya da kabuğun dışından bir yazım ve
/// ikisi de uzunluğu değiştiriyor — sonraki komutun `L`'si tutmuyor, widget
/// hiçbir şey yapmıyor: tekrar kaybolur, küme bölünmez
/// (`.tasks/035-grapheme-dizileri/phase-5.md` → Uygulama Notları).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DockPrediction {
    /// Komutun gönderilmesiyle doğan nesil ([`crate::Session`]'ın
    /// `key_gen`'i); başka bir nesilde tahmin geçersiz.
    pub(crate) generation: u64,
    pub(crate) buffer: String,
    /// `BUFFER`'da karakter indeksi.
    pub(crate) caret: usize,
}

pub(crate) struct ShellLog {
    /// Kabuğun o anki durumu; `None` = entegrasyon yok.
    pub(crate) state: Option<ShellState>,
    pub(crate) blocks: BlockLog,
    /// ZLE'nin görüntü aynası. Aynı kilidin altında, çünkü aynı akıştan
    /// besleniyor ve aynı kare okuyor: ayrı bir kilit, kareyi safha ile
    /// aynanın çeliştiği bir anda yakalayabilirdi — `Input` safhasında
    /// ızgarayı bastırıp dock'a bir önceki satırı çizmek gibi.
    pub(crate) dock: DockState,
    /// Dock'un bağlam satırı: dizin ve dal. Aynanın **yanında**, içinde değil
    /// ([`DockContext`]); aynı kilit, ayrı ömür.
    pub(crate) context: DockContext,
    /// `[remote] hosts`'un desen listesi (037 Karar 2); uzak host'un işareti
    /// ([`DockContext::remote_mark`]) bundan, iki kenarda çözülüyor.
    pub(crate) host_rules: Vec<HostRule>,
    /// Dock'un fareyle seçimi; `None` → seçim yok. Aynanın **yanında**
    /// ([`DockSelection`]'ın doc'u) ve aynı kilidin altında: `BUFFER`
    /// değişince onu silen yazıcı (okuyucu thread) ile aralığı okuyan kare
    /// aynı turda görüyor.
    pub(crate) dock_selection: Option<DockSelection>,
    /// Dock'un dikey penceresinin **tekerlekle seçilmiş** tepesi (032 phase-4);
    /// `None` → pencere caret'i izliyor ([`crate::dock::render_with`]).
    ///
    /// Tavanı aşan girişte pencere yalnız caret'i izleseydi üstteki
    /// satırlara fare hiç ulaşamazdı. Aynanın **yanında**, seçimin gerekçesiyle
    /// ([`Self::dock_selection`]): tarayıcı aynayı toptan tazeliyor. Ömrü
    /// caret'in yerinde kalmasına bağlı — `BUFFER`, `PREBUFFER` ya da caret
    /// değişince (yazmak, ok tuşu) kalkıyor ve pencere caret'e dönüyor; öneri
    /// değişimi onu kaldırmıyor.
    pub(crate) dock_scroll: Option<usize>,
    /// Kabuk **bu prompt'ta** düzenleme widget'ını bağladı mı (`8133;w`,
    /// 031) — düzenleme kapısının dördüncü koşulu
    /// ([`crate::Session::can_edit_dock`]).
    ///
    /// Aynanın **yanında**, içinde değil: tarayıcı [`DockState`]'i her `u`
    /// yükünde toptan `clone_from` ile tazeliyor ve yetenek prompt başına bir
    /// kez geliyor, yani içinde dursaydı ilk tuşta silinirdi. Ömrü prompt'un:
    /// `line-finish` (`e`) ve prompt'un başı (`A`) siliyor. `A` ikinci bir
    /// kemer — `line-finish`'in koşmadığı bir çıkış (kesilen satır) yeteneği
    /// sonraki prompt'un `w`'sine kadar taşımasın; yanlışın yönü "düzenleme
    /// yok".
    pub(crate) dock_editable: bool,
    /// Son düzenleme komutunun **beklenen** sonucu (035 phase-5): ayna o
    /// komuta cevap verene kadar düzenleme kapısı bu satıra bakıyor
    /// ([`DockPrediction`]). Ömrü yalnız bir nesil — araya giren her girdi
    /// onu geçersiz kılıyor; `e` ve `A` de siliyor.
    pub(crate) dock_pending: Option<DockPrediction>,
    /// Koşan komutun başlangıç anı; komut koşmuyorken `None`.
    ///
    /// **Tek alan, blok başına değil:** aynı anda tek komut koşar, çünkü
    /// `C` ile `D` arasında kabuk bir sonraki prompt'u basmıyor. Defterin her
    /// girdisine bir `Instant` koymak 10 000 satırlık scrollback'te sekme
    /// başına ödenen ölü bir bedel olurdu.
    ///
    /// `Instant`, sistem saati değil: kullanıcı saati değiştirse ya da yaz
    /// saati geçse bile süre geriye akmaz.
    pub(crate) running_since: Option<Instant>,
    /// Komut nesli: safha `Running`'e her **geçişte** bir artıyor (036
    /// Karar 2).
    ///
    /// Uzak oturum yoklamasının bayat cevap kapısı: yoklama ana thread'de,
    /// `D` okuyucu thread'de, ve arada biten komutun cevabı bir sonrakine
    /// sızmamalı. Çağıran nesli yoklamadan önce alıyor
    /// ([`crate::Session::running_command`]) ve cevapla geri veriyor
    /// ([`crate::Session::set_remote`]); tutmazsa cevap düşüyor.
    ///
    /// **İkinci `C` geçiş değil** ve nesli oynatmıyor — saatin "ilk `C`
    /// kazanır" kuralının ([`Self::running_since`]) aynı yeri: iTerm2'nin komut
    /// ortasındaki `C`'si koşan bir ssh'ın cevabını geçersiz kılmamalı.
    pub(crate) command: u64,
    /// Kabuk **bizim** kimliğimizi taşıyan bir işaret bastı mı
    /// (`bt_block=`'lı `A` ya da `D`) — yapışkan; [`Self::apply`]'ın yabancı
    /// işaret kapısının ön koşulu.
    ///
    /// Kapı bu bayrak olmadan kurulamaz: entegrasyonu kapalı ama kendi OSC
    /// 133'ünü basan bir kabukta (iTerm2, kitty) **bütün** işaretler
    /// kimliksiz ve uzak durum hiç silinmezdi.
    ours: bool,
    /// Son `Running` geçişinin açtığı komut **bizim** `D`/`A`'mızla henüz
    /// kapanmadı ([`Self::running_command`]'ın ikinci kolu).
    ///
    /// Safha tek başına yetmiyor: ssh'ın öbür ucundaki entegrasyonun `A`'sı
    /// yoklamadan **önce** aynı okumada gelebiliyor ve safhayı `Prompt`'a
    /// çekiyor — komut hâlâ koşarken nesil `None` görünür, yoklama düşer ve
    /// gösterge hiç çıkmazdı.
    command_open: bool,
    /// Devrin **ham** cevabı, en son gözlendiği hâliyle.
    ///
    /// Damga [`Self::apply_scan`]'de tutuluyor — tek giriş noktası ve yaprak
    /// kilidin altında, yani `Term`'e hiç dokunmadan. Kare yolunda tutulsaydı
    /// iki kare arası hiç işaret gelmeyen bir pencerede damga hiç kıpırdamaz,
    /// gelen bir işaret de iki kare arasında **iz bırakmadan** geçerdi.
    caret_raw: CaretHome,
    /// [`Self::caret_raw`] en son ne zaman **değişti**.
    ///
    /// Değişmeyen gözlem damgayı kıpırdatmıyor: her tuş vuruşu bir ayna olayı
    /// doğuruyor ve damga onlarla tazelenseydi tutma hiç dolmazdı.
    caret_since: Instant,
    /// `line-finish` (`8133;e`) **tutuluyor**: ne zaman geldi (032 Karar 11).
    ///
    /// zsh her `PS2` kabulünde `line-finish` koşuyor, arada `precmd` yok ve
    /// safha `Input` kalıyor (ölçüldü, zpty); hemen ardından `line-init`'in
    /// aynası (`u`, `PREBUFFER` dolu) geliyor. `e` aynayı anında sıfırlasaydı
    /// çok satırlı dock'ta her ⏎ bandı bir kare küçültüp yeniden büyütür,
    /// kabul edilen satır bir an ızgarada belirirdi. Tutulurken aynanın
    /// görüntüsü, bandı ve bastırması **olduğu gibi** duruyor; `u` gelirse
    /// yeni ayna geçiyor, bir OSC 133 işareti (`C`: komut koştu, `A`: yeni
    /// prompt) ya da [`HANDOVER_HOLD`] dolarsa ([`Self::expire_end`]) bugünkü
    /// sıfırlama. Süre caret tutmasının saati, ikinci bir sayı yok.
    ///
    /// **Aynanın yanında, içinde değil:** her tüketici `status == Live`
    /// soruyor ve tutma boyunca `Live` görmeli; yeni bir durum varyantı
    /// dokuz tüketicinin dokuzunu da değiştirirdi.
    end_since: Option<Instant>,
}

/// Caret'in sahibi: ızgara mı, dock mu.
///
/// **Tek yüklem, iki tüketici.** [`crate::dock::render`] caret'i çizmek için,
/// [`crate::Session::frame`] ızgaranın imlecini gizlemek için soruyor; ikisi
/// ayrı ayrı yazılsaydı aynı karede iki caret (ya da hiç caret) doğardı —
/// gözlenen kusur tam da buydu (012 phase-8).
///
/// **Bu, giriş satırının bastırılmasından ayrı bir sorudur.** Bastırma hangi
/// **hücrelerin** atlanacağını soruyor ve cevabı çıpaya bağlı;
/// burada sorulan şey caret'in **yeri** ve çıpayla ilgisi yok — sıfır
/// genişlikli prompt hiçbir hücre yazmadığı için çıpa yokken de caret dock'un.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CaretHome {
    /// Izgara çiziyor.
    Grid,
    /// Dock çiziyor.
    Dock,
}

/// Dock→Grid devrinin **tutulma süresi** (histerezis).
///
/// **Seçilmiş, ölçülmemiş.** İki ucu da gerekçeli: alt sınır ölçülmüş
/// (`context.md` → Kanıt: `ls` koşarken safha 44 ms sürüyor, yani 44 ms'nin
/// altındaki her tutma `ls`'i hiç yakalamaz) ve buradaki değer onun üç katından
/// fazla — `git status` sınıfı komutlar da kapsansın. Üst sınırın emsali 013:
/// bir saniyeyi geçmeyen komutun sayacı **gösterilmiyor**, yani kullanıcının
/// "koşuyor" saydığı eşik zaten bir saniye; tutma onun çok altında kalmalı ki
/// gerçekten koşan komut caret'ini ızgarada göstersin.
///
/// **Üçüncü sayıyla ilişkisi yazılı olmalı:** imleç animasyonu ~230 ms'de
/// yerleşiyor (`bt_gpu::motion`, `OMEGA`'nın doc'u) ve bu değer onun
/// **altında**. Sonucu şu: dolan her tutma caret'i animatör hâlâ yoldayken
/// serbest bırakıyor, yani tutmayı aşan komutlarda tek bir temiz hedefleme
/// iki hedeflemeye bölünüyor. Bilinen bedel, `.tasks/015-imlec-cilasi/phase-1.md`
/// → Bilinen sınırlar; değeri değiştiren bu ilişkiyi hesaba katmalı. Emsal
/// `bt_gpu::motion`'ın `const _: () = assert!(EASE_DURATION < TIME_CEILING)`'ı
/// — orada iki sayı aynı crate'te olduğu için şart derleyiciye yazılabiliyor,
/// burada crate sınırı geçtiği için yalnız bu cümle var.
///
/// `docs/OLCUMLER.md`'nin konusu **değil**: bu bir his eşiği, ölçüm değil
/// (emsal `FADE_DURATION`).
pub(crate) const HANDOVER_HOLD: Duration = Duration::from_millis(150);

/// İki son tarihten **yakın** olanı; ikisi de boşsa boş.
///
/// Serbest ve saf, **sınanabilirlik için**: `min`'in sessizce yazmaya (ezmeye)
/// dönmesi iki yönde de görünmez bir kusur olurdu — ya koşan komutun sayacı
/// donar ya devir hiç gerçekleşmez. Emsal `bt_gpu::link`'in `due_clock`'u,
/// o da tam bu sebeple saf bir yardımcıya çıkarılmıştı.
pub(crate) fn sooner(a: Option<Duration>, b: Option<Duration>) -> Option<Duration> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// Devrin bir andaki cevabı: caret'in sahibi ve tutmanın kalanı.
///
/// **Tek kayıt, çünkü tek `now`.** İkisi ayrı ayrı sorulsaydı iki farklı ana
/// ait olurlardı; [`SuppressedInput`] ile aynı gerekçe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CaretDecision {
    /// Caret'i bu karede kim çiziyor.
    pub(crate) home: CaretHome,
    /// Tutma **cevabı çevirirken** kalan süre; saate yalnız bu giriyor.
    /// `None` → tutma yok, yani istenecek bir kare de yok.
    pub(crate) hold_left: Option<Duration>,
}

/// Devrin **ham** cevabı: safha ile aynanın durumundan, tutma uygulanmadan.
///
/// Ayrı fonksiyon, çünkü damganın izlediği şey budur ([`ShellLog::observe_caret`]):
/// tutma damgadan türüyor ve damgaya geri beslenemez — beslenseydi tutma kendi
/// kendini süresiz uzatırdı.
fn caret_home_raw(shell: Option<ShellState>, status: DockStatus) -> CaretHome {
    match (shell.map(|state| state.phase), status) {
        (Some(ShellPhase::Running), _)
        | (_, DockStatus::Unavailable(_) | DockStatus::Control)
        | (Some(ShellPhase::Input), DockStatus::Idle) => CaretHome::Grid,
        _ => CaretHome::Dock,
    }
}

/// Caret'in sahibini safha, aynanın durumu ve **tutma** ile çözer.
///
/// Serbest fonksiyon ve defteri görmüyor; üretimde tek çağıranı
/// [`ShellLog::caret`], sınamalar yükleme kurmadan yüklemi sorabilsin diye
/// serbest kaldı.
///
/// `held` = Dock→Grid devri şu an **tutuluyor mu** (histerezis, R1.1). Tutmanın
/// süresi ve damgası defterde ([`HANDOVER_HOLD`], [`ShellLog::caret`]); buraya
/// yalnız kararı geliyor, çünkü bu fonksiyon saat görmüyor.
///
/// **Yalnız Dock→Grid yönü tutulur.** Ters yön geciktirilseydi komut bitince
/// caret ızgarada asılı kalır, kullanıcı yazmaya başladığında dock'ta
/// caret'siz bir satır görürdü — yanlışın yönü güvenli değil.
///
/// **`Unavailable` ile `Control` tutmanın dışında.** O iki kolun gerekçesi
/// aşağıda yazılı ve koşulsuz: gösteremediğimiz satır ızgarada duruyor,
/// caret'i de orada durmalı, *yoksa kullanıcı yazdığı yeri göremez*. Tutma
/// onları da kapsasaydı `^A` taşıyan bir yapıştırmadan sonra caret 150 ms
/// boyunca dock'un prompt işaretinin yanında durur, yani düzeltilen belirti
/// kısalmış hâliyle geri gelirdi. Üstelik arıza bir sıçrama
/// **üretmiyor** — kullanıcı geri silmeden ayna `Live`'a dönmüyor — yani
/// tutmanın orada kazancı sıfır, bedeli caret'in 150 ms boş bir dock'ta
/// durması olurdu. Carve-out yüklemin **içinde**, çünkü dışarıda olsaydı
/// `caret_home(_, Unavailable, true)` `Dock` döner ve yüklem yalan söylerdi.
///
/// **Kural tek cümle: caret satırın nerede çizildiğine uyar.** Giriş satırı
/// ızgaradaysa caret de ızgarada, dock'taysa dock'ta. Dört hâl ızgaranın:
///
/// - `Running` — komut koşuyor. Satırın sahibi o: `cat`'in beklediği girdi,
///   `ssh`'ın parola istemi ve vim'in kendi imleci ızgarada yaşıyor.
/// - `Unavailable` — gösteremediğimiz bir satır var ve ızgarada duruyor
///   (R1.2); caret'i de orada durmalı, yoksa kullanıcı yazdığı yeri göremez.
/// - `Control` — görüntü dock'un çizmediği bir kontrol karakteri taşıyor
///   ([`DockStatus::Control`]). Aynı cümlenin ikinci uygulaması: satır
///   ızgarada kaldığı için caret de orada. (Satır sonu 032'den beri bu
///   listede değil: dock çok satırı kendisi çiziyor.)
/// - `Input` + `Idle` — kabuk "kullanıcı yazıyor" diyor ama ZLE satırı
///   **bırakmış**. Bastırma da tam burada kalkıyor (R3.3): `CORRECT`'in
///   `[nyae]` sorusu, `zle -M` mesajı, `line-finish` ile Enter arası. Satır
///   ızgaraya döndüğü için caret de dönmek zorunda.
///
/// **Kalan her hâl dock'un ve `state == None` buna dahil.** Açılışta (zsh'in rc
/// süresi), prompt çizilirken (`Prompt`) ve her komutun bitişiyle yeni prompt
/// arasında (`Finished`; içinde `precmd`'in `git rev-parse` fork'u var) ortada
/// bir giriş satırı **yok** — dock boş bir caret gösteriyor ve kullanıcının
/// yazmaya başlayacağı yer orası. Kapıyı "kabuk en az bir kez konuştu mu"ya
/// bağlamak caret'i o pencerelerde ızgarada bırakır ve prompt gelince
/// **sıçratırdı** — düzeltilen kusur buydu.
///
/// **Bilinen pencere:** `B` prompt'un içinde basılıyor, ayna ise ZLE'nin
/// `line-init`'inde doğuyor; arada safha `Input` ama durum `Idle`, yani caret
/// bir an ızgarada. Pencere zsh'in kendi açılışı kadar — fork yok, I/O yok — ve
/// kapatmanın yolu "ayna hiç gelmedi" ile "ZLE bıraktı"yı ayıran yeni bir
/// durum tutmak. Ölçülmüş bir belirti olmadan o durumu eklemiyoruz; giderilen
/// pencere (`Finished`, bir `git` fork'u) bunun kat kat üstünde.
/// *(015 phase-1: pencere artık **tutmanın içinde eriyor** — zsh'in `line-init`'i
/// [`HANDOVER_HOLD`]'un çok altında, yani o an hiç raporlanmıyor. Yukarıdaki
/// kayıt tarihli ve duruyor: pencerenin kendisi kapanmadı, görünmez oldu.)*
///
/// **İkinci bilinen sınır:** entegrasyon kurulu ama betik sessizce ölürse caret
/// dock'ta kalır ve yazdıkça kıpırdamaz. Yanıltıcı ama görünür (dock boş, blok
/// şeridi yok), yani bu deponun yasakladığı "sessizce yanlış" sınıfına girmiyor.
pub(crate) fn caret_home(shell: Option<ShellState>, status: DockStatus, held: bool) -> CaretHome {
    match caret_home_raw(shell, status) {
        CaretHome::Grid
            if held && !matches!(status, DockStatus::Unavailable(_) | DockStatus::Control) =>
        {
            CaretHome::Dock
        }
        home => home,
    }
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
    /// Aynanın **hiç karakteri yok**: ne görüntüde (`PREDISPLAY ++ BUFFER ++
    /// POSTDISPLAY`) ne `PREBUFFER`'da — tazelik kapısının boş ayna sorusu
    /// (`Session::frame`'in `blank_mirror`'ı, 025).
    ///
    /// **Karakter, sütun değil** (032): 032'ye kadar ölçüt "caret'in iki
    /// yanında sıfır sütun"du ve satır farkında değildi — tek başına bir
    /// `\n` imleci bir satır aşağı itiyor, yani "imleç çıpanın satırında
    /// olmak zorunda" öncülü artık yalnız gerçekten boş aynada doğru.
    /// `PREBUFFER` doluysa (`for>` satırı) imleç meşru olarak çıpanın
    /// aşağısında ve çıpa sorusu hiç sorulmuyor.
    pub(crate) blank: bool,
    /// Bastırmanın üst tabanı **çıpanın satırı** mı (032 Karar 7): `PREBUFFER`
    /// dolu (ZLE önceki satırları kabul etti, `PS2`'leriyle birlikte hepsi
    /// girişin parçası) ya da `line-finish` tutuluyor
    /// ([`ShellLog::expire_end`]; kabul edilen satır `PREBUFFER`'a geçmek
    /// üzere). İkisinde de düzen yürüyüşünün üst ucu ızgarayı bilmiyor —
    /// `PS2`'nin genişliği aynada yok — ve çıpa kesin veri.
    pub(crate) from_anchor: bool,
    /// Görüntünün son mürekkebi ([`DockState::last_ink`]) — tazelik kapısının
    /// aynadaki yarısı.
    pub(crate) last_ink: Option<char>,
    /// ZLE ekleme keymap'inde mi ([`DockState::insert_keymap`]); yapıştırmanın
    /// dar istisnasının üçüncü koşulu.
    ///
    /// Aynı kayıtta, çünkü aynı yaprak kilit turundan çıkıyor: ayrı okunsaydı
    /// keymap ile safha farklı anlara ait olabilir ve istisna, kullanıcının
    /// çoktan `vicmd`'ye geçtiği bir satırda açık kalabilirdi.
    pub(crate) insert_keymap: bool,
    /// Aynanın cevap verdiği girdi nesli ([`DockState::answers`]) — tazelik
    /// kapısının zamansal yarısı. `last_ink` ile **aynı kayıtta**, çünkü ikisi
    /// aynı aynaya ait olmak zorunda: ayrı okunsalardı yeni bir damga eski bir
    /// içeriği "taze" ilan edebilirdi.
    pub(crate) answers: u64,
}

impl ShellLog {
    pub(crate) fn new(scrollback: usize) -> Self {
        Self {
            state: None,
            blocks: BlockLog::new(scrollback),
            dock: DockState::default(),
            context: DockContext::default(),
            host_rules: Vec::new(),
            dock_selection: None,
            dock_scroll: None,
            dock_editable: false,
            dock_pending: None,
            running_since: None,
            command: 0,
            ours: false,
            command_open: false,
            // Açılışta caret dock'un (`caret_home_raw(None, Idle)`), yani ilk
            // devir her zaman Dock→Grid yönünde ve tutma ona uygulanabilir.
            caret_raw: CaretHome::Dock,
            caret_since: Instant::now(),
            end_since: None,
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
    ///
    /// Dönüş çağıranın vereceği haberler ([`ScanOutcome`]): `Running`'e
    /// geçiş ve uzak durumun silinmesi (036).
    pub(crate) fn apply(&mut self, mark: Mark) -> ScanOutcome {
        // **Uzak oturumu yalnız BİZİM işaretimiz bitiriyor** (036 phase-3):
        // kabuk kimliğimizi bir kez bastıysa ve uzak oturum etkinse,
        // kimliksiz işaret (`A`/`D` kimliksiz, her `B` ve `C`) hiçbir şeye
        // dokunmuyor. Kaynağı ssh'ın öbür ucu: fish 4 ya da kitty/iTerm2
        // entegrasyonu aynı PTY'ye 133 basıyor ve uzak `A` göstergeyi
        // silerdi, uzak `C` yeni bir komut nesli açardı. Yerel kabuk ssh'ın
        // arkasında bloklu, yani o sırada gelen kimliksiz işaret bizim
        // olamaz. Kapı **uzak oturumla sınırlı**, `Running`'le değil: `exec
        // fish` gibi kimliğimizi bir daha basmayacak bir kabuğa geçişte
        // `Running` hiç bitmez, saat durmaz ve sayaç boşta kare isterdi.
        // Yoklamadan önceki yarış [`Self::command_open`]'da.
        let identified = match mark {
            Mark::PromptStart { id } | Mark::CommandEnd { id, .. } => id.is_some(),
            Mark::PromptEnd | Mark::CommandStart => false,
        };
        if identified {
            self.ours = true;
        } else if self.ours && self.context.remote.is_some() {
            return ScanOutcome::default();
        }
        // Komutu kapatan: bizim kimlikli `A`/`D`'miz; kimliğimizi hiç
        // görmemiş kabukta her `A`/`D`.
        if matches!(mark, Mark::PromptStart { .. } | Mark::CommandEnd { .. })
            && (identified || !self.ours)
        {
            self.command_open = false;
        }
        // **Tutulan `line-finish` her işarette biter** (Karar 11): `C` komutun
        // koştuğunu, `A` yeni prompt'u söylüyor — ikisinde de kabul edilen
        // satır artık ızgaranın kalıcı içeriği.
        if self.end_since.take().is_some() {
            self.end_line();
        }
        let mut outcome = ScanOutcome::default();
        let state = self.state.get_or_insert(ShellState {
            phase: ShellPhase::Prompt,
            last_exit: None,
        });
        match mark {
            Mark::PromptStart { id } => {
                state.phase = ShellPhase::Prompt;
                // Uzak durum **kendiliğinden** gidiyor (036 Karar 2): bitiş
                // için `bt-shell`'e gidiş-dönüş yok. `A` `D`'nin savunma kolu
                // — saatinki gibi, kaybolan bir `D` uzak göstergeyi sonraki
                // prompt'a taşımasın. Uzak kabuğun kimliksiz `A`'sı buraya
                // hiç ulaşmıyor (yukarıdaki kapı).
                outcome.title = self.context.clear_remote();
                self.dock_editable = false;
                self.dock_pending = None;
                // **Saatin ikinci sıfırlama noktası ve bir savunma kolu.**
                // Prompt basılıyorsa hiçbir komut koşmuyor, yani buradaki saat
                // tanım gereği bayat. Yalnız `D` tüketseydi kaybolan bir `D`
                // (yarıda kesilmiş OSC, kimliksiz kapanış) saati ayakta
                // bırakır ve **sonraki** bloğun `D`'si onu tüketirdi: anlık
                // bir komut "4m 12s" sürmüş görünürdü (`/code-review`, 013
                // kapı). Sıfırlamanın yönü güvenli — en kötüsü sayacın hiç
                // çıkmaması, uydurulmuş bir süre değil.
                self.running_since = None;
                if let Some(id) = id {
                    self.blocks.start(id);
                }
            }
            Mark::PromptEnd => state.phase = ShellPhase::Input,
            Mark::CommandStart => {
                // **Geçiş** yalnız safha `Running` değilken: ikinci `C`
                // (iTerm2 entegrasyonu) ne nesli ne uzak durumu oynatıyor
                // ([`Self::command`]). Yoklama `D`'ye kadar kilitli, yani o
                // `C` host'u silseydi gösterge geri gelmezdi.
                if state.phase != ShellPhase::Running {
                    self.command += 1;
                    self.command_open = true;
                    outcome.started = true;
                    outcome.title = self.context.clear_remote();
                }
                state.phase = ShellPhase::Running;
                // Saatin dikildiği yer: `C` komutun **çalışmaya başladığını**
                // söylüyor; prompt'un basılması ya da kullanıcının yazdığı
                // süre sayaca girmemeli.
                //
                // **İlk `C` kazanıyor, sonrakiler ezmiyor.** Kullanıcının
                // kabuğunda ikinci bir OSC 133 kaynağı olabilir (iTerm2'nin
                // `~/.iterm2_shell_integration.zsh`'ı, VS Code, Ghostty) ve o
                // da `C` basar — ölçüldü, kullanıcının makinesinde komut
                // başına **iki** `C` geliyor. Üzerine yazsaydık süre ikinci
                // işaretten başlardı; daha kötüsü, komut ortasında gelen bir
                // `C` (iTerm2 `precmd`'inin ^C kolu) saati sıfırlardı.
                // Sıfırlamanın tek yeri prompt (`A`).
                self.running_since.get_or_insert_with(Instant::now);
            }
            Mark::CommandEnd { exit, id } => {
                state.phase = ShellPhase::Finished;
                outcome.title = self.context.clear_remote();
                // Kodu **her hâlde** tazeliyoruz: okunamayan bir kodu eskisiyle
                // doldurmak, biten komutu başkasının koduyla etiketlemek olurdu.
                state.last_exit = exit;
                // **Saati yalnız BİZİM `D`'miz tüketiyor**, yani kimlik
                // taşıyan kapanış. Kimliksiz bir `D` bizim defterimize
                // yazamıyor (`blocks.finish` çağrılmıyor); saati yine de
                // tüketseydi ölçtüğümüz süre **çöpe giderdi**.
                //
                // Bu varsayımsal değil, kullanıcının makinesinde ölçüldü:
                // iTerm2'nin shell entegrasyonu kuruluyken her komut iki `D`
                // doğuruyor — önce onun kimliksizi, sonra bizimki. Kimliksiz
                // olan saati alıyor, bizimki boş buluyor ve süre **sıfır**
                // yazılıyordu; sayaç eşiğin altında kaldığı için hiç
                // çizilmiyordu. Belirti tam da buydu: "bitince süre
                // gözükmüyor".
                //
                // `take` yine zorunlu ama artık kimliğin içinde: kalsaydı iki
                // komut arasında (`Finished` safhası, içinde bir `git`
                // fork'u) bitmiş bir komut hâlâ koşuyormuş gibi sayılırdı.
                if let Some(id) = id {
                    let elapsed = self
                        .running_since
                        .take()
                        .map_or(0, |since| millis(since.elapsed()));
                    self.blocks.finish(id, exit, elapsed);
                }
            }
        }
        outcome
    }

    /// Tarayıcının çıkardığı olayı doğru kola uygular.
    ///
    /// Tek giriş noktası, çünkü okuyucu thread'i kilidi **olay başına** alıyor
    /// ve iki ayrı çağrı iki ayrı kilit turu demek olurdu.
    #[cfg(test)]
    pub(crate) fn apply_scan(&mut self, event: ScanEvent<'_>) {
        self.apply_scan_answering(event, 0);
    }

    /// [`Self::apply_scan`]'in üretimdeki hâli: ayna olayı `answers`'ı
    /// [`DockState::answers`]'a **içerikle aynı turda** yazıyor.
    ///
    /// Damga argüman, çünkü defter `Session`'ı görmüyor ve görmemeli; okuyan
    /// taraf (`session`'ın `TappedPty`'si) nesli olay başına bir atomik
    /// okumayla getiriyor.
    ///
    /// Dönüş çağıranın vereceği iki haber ([`ScanOutcome`]); ikisini de
    /// kilidi bıraktıktan sonra verir. Başlığın haberi yalnız **farklı** bir
    /// yerel OSC 7 dizininde ya da uzak durumun silinmesinde: aynı dizini
    /// basan her `precmd` haber doğursaydı her prompt ana kuyruğa boşuna bir
    /// iş atardı.
    pub(crate) fn apply_scan_answering(
        &mut self,
        event: ScanEvent<'_>,
        answers: u64,
    ) -> ScanOutcome {
        let mut outcome = ScanOutcome::default();
        match event {
            ScanEvent::Mark(mark) => outcome = self.apply(mark),
            ScanEvent::Dock(event) => self.apply_dock(event, answers),
            // Dizin **çözülmüş** geliyor: şemayı, yolu ve yüzde çözmeyi
            // tarayıcı yaptı, buraya yalnız çizilebilir bir yol ve yetkinin
            // yerel olup olmadığı ulaşıyor. Reddedilen bir OSC 7 hiç olay
            // doğurmuyor, yani eski yol yerinde kalıyor — yanlış yol
            // göstermektense bayat yol.
            //
            // **Hangi yuvaya** (036 Karar 4): uzak oturum etkinken **her**
            // OSC 7 uzak tarafın — yerel kabuk ssh'ın arkasında bloklu, yani
            // `file:///…` basan bir uzak kabuk yerel dizini ezmemeli. Etkin
            // değilken yabancı yetki uzak yuvaya: OSC 7 yoklamadan önce
            // gelebiliyor ve sonucu değiştirmemeli. Uzak yuva başlığa
            // girmiyor, yani haber yok.
            ScanEvent::Cwd { path, local } => {
                if self.context.remote.is_some() || !local {
                    self.context.remote_cwd.clear();
                    self.context.remote_cwd.push_str(path);
                } else if self.context.cwd != path {
                    self.context.cwd.clear();
                    self.context.cwd.push_str(path);
                    outcome.title = true;
                }
            }
        }
        self.observe_caret();
        outcome
    }

    /// Koşan komutun nesli ([`Self::command`]); komut koşmuyorsa
    /// `None`. Bayat cevap kapısının iki yarısının ([`crate::Session::running_command`],
    /// [`crate::Session::set_remote`]) **tek** tanımı: ayrışsalardı yoklama
    /// `set_remote`'un reddedeceği bir nesil alabilirdi.
    ///
    /// İkinci kol [`Self::command_open`]: kimliğimizi basan bir kabukta
    /// komut, safhayı yabancı bir `A` oynatmış olsa da bizim `D`'mize kadar
    /// koşuyor.
    pub(crate) fn running_command(&self) -> Option<u64> {
        let running = self
            .state
            .is_some_and(|state| state.phase == ShellPhase::Running);
        (running || (self.ours && self.command_open)).then_some(self.command)
    }

    /// Uzak oturumun host'unu yazar (036); **başlığın girdisi değiştiyse**
    /// `true`.
    ///
    /// Kapı çağıranda ([`crate::Session::set_remote`]: nesil ve safha); burada
    /// yalnız yazma. Boş host "uzak değil" demek — gösterilecek bir ad yok.
    ///
    /// **Kontrol karakteri taşıyan host da yok sayılıyor**: ad süreç
    /// tablosundan, yani kullanıcının yazdığı argv'den geliyor ve satır sonu
    /// ya da ESC pencere başlığına ve bağlam satırına (kutu olarak) giderdi.
    /// Yanlışın yönü güvenli: gösterge çıkmıyor, yanlış bir ad çizilmiyor.
    ///
    /// Hedef bütün olarak yazılıyor (037 Karar 1) ve işaret burada, desen
    /// listesinden ([`Self::host_rules`]) çözülüyor; dönüş yine yalnız
    /// **host**'un değişimi — başlığın girdisi o.
    pub(crate) fn set_remote(&mut self, target: Option<&RemoteTarget>) -> bool {
        let target = target
            .filter(|target| !target.host.is_empty() && !target.host.chars().any(char::is_control));
        let changed = self.context.remote_host() != target.map(|target| target.host.as_str());
        match target {
            Some(target) => {
                match &mut self.context.remote {
                    Some(slot) => slot.clone_from(target),
                    slot => *slot = Some(target.clone()),
                }
                self.context.remote_mark =
                    crate::settings::host_mark(&self.host_rules, &target.host);
            }
            // Uzak yuva kalıyor: yoklama "yerel" dediyse zaten okunmuyor ve
            // `C`/`D`/`A` onu siliyor.
            None => {
                self.context.remote = None;
                self.context.remote_mark = HostMark::None;
            }
        }
        changed
    }

    /// Host işaretlerinin desen listesini yazar ve etkin uzak host'un
    /// işaretini yeniden çözer (037 Karar 2); **işaret değiştiyse** `true`.
    /// Aynı liste no-op.
    pub(crate) fn set_host_rules(&mut self, rules: &[HostRule]) -> bool {
        if self.host_rules == rules {
            return false;
        }
        self.host_rules = rules.to_vec();
        let Some(host) = self.context.remote_host() else {
            return false;
        };
        let mark = crate::settings::host_mark(&self.host_rules, host);
        let changed = mark != self.context.remote_mark;
        self.context.remote_mark = mark;
        changed
    }

    /// Devrin ham cevabını damgalar; **değişmediyse damga kıpırdamaz**.
    fn observe_caret(&mut self) {
        let raw = caret_home(self.state, self.caret_status(), false);
        if raw != self.caret_raw {
            self.caret_raw = raw;
            // **Saat yalnız değişimde okunuyor.** `apply_scan` okuyucu
            // thread'in olay başına giriş noktası: her tuş vuruşu bir ayna
            // olayı, her prompt bir OSC 7 ve bir dal olayı doğuruyor ve
            // bunların ezici çoğunluğu ham cevabı değiştirmiyor. Okuma
            // dışarıda kalsaydı hepsi bedelsiz sanılan bir `Instant::now()`
            // öderdi.
            self.caret_since = Instant::now();
        }
    }

    /// Ayna olayını [`Self::dock`]'a uygular.
    ///
    /// Çizilemeyen iki hâlde (`End`, `Unavailable`) metin **boşaltılıyor**:
    /// bayat bir satır bırakmak, phase-4'te ızgara bastırılırken dock'un bir
    /// önceki komutu göstermesi demek olurdu — kullanıcının yazdığıyla
    /// gördüğünün sessizce ayrılması, bu deponun yasakladığı belirti sınıfı.
    ///
    /// **`BUFFER`'ı değişen ayna dock seçimini siler** (031 R3.4): indeksler
    /// artık başka bir metnin karakterlerini gösterirdi. Yalnız `BUFFER` —
    /// prompt'un yeniden çizilmesi (`PREDISPLAY`) ya da önerinin değişmesi
    /// seçili metni oynatmıyor. `End` ile `Unavailable` metni boşaltıyor,
    /// seçimi de.
    fn apply_dock(&mut self, event: DockEvent<'_>, answers: u64) {
        match event {
            DockEvent::Update(staged) => {
                self.end_since = None;
                if self.dock.buffer != staged.buffer || self.dock.prebuffer != staged.prebuffer {
                    self.dock_selection = None;
                    self.dock_scroll = None;
                }
                if self.dock.cursor != staged.cursor {
                    self.dock_scroll = None;
                }
                self.dock.clone_from(staged);
                self.dock.answers = answers;
            }
            DockEvent::End => {
                self.dock_selection = None;
                self.dock_scroll = None;
                self.dock_editable = false;
                self.dock_pending = None;
                // Boş satır da bir cevap: bkz. [`DockState::answers`]. Tutulan
                // satır da — `e` ⏎'in cevabı ve tazelik kapısı tutma boyunca
                // onu soruyor (imleç `PS2`'nin satırına inmiş olabilir).
                self.dock.answers = answers;
                // **Safha `Input`'ta ve ayna canlıysa tutuluyor** (Karar 11,
                // [`Self::end_since`]). Saat yalnız burada okunuyor, yani
                // ⏎ başına bir kez — [`Self::observe_caret`]'in kuralı.
                let typing = self
                    .state
                    .is_some_and(|state| state.phase == ShellPhase::Input);
                if typing && self.dock.status == DockStatus::Live {
                    self.end_since = Some(Instant::now());
                } else {
                    self.end_since = None;
                    self.end_line();
                }
            }
            DockEvent::Unavailable(fault) => {
                self.end_since = None;
                self.dock_selection = None;
                self.dock_scroll = None;
                self.dock.reset();
                self.dock.status = DockStatus::Unavailable(fault);
            }
            // Dal aynanın **kanalından** geliyor ama aynanın durumu değil:
            // `status`'a ve metne dokunmuyor. Boş gövde "depo değil" demek ve
            // dalı **siliyor** — bir önceki deponun dalı yeni dizinde asılı
            // kalsaydı kullanıcı yanlış dalda olduğunu sanırdı.
            DockEvent::Branch(branch) => {
                self.context.branch.clear();
                self.context.branch.push_str(branch);
            }
            DockEvent::Editable => self.dock_editable = true,
        }
    }

    /// `line-finish`'in sıfırlaması: metin boşalıyor, durum `Idle`; damga
    /// (`answers`) `e`'nin yazdığı yerde kalıyor — [`DockState::answers`]'ın
    /// "`Idle` ayna da damgalı" kuralı.
    fn end_line(&mut self) {
        let answers = self.dock.answers;
        self.dock.reset();
        self.dock.status = DockStatus::Idle;
        self.dock.answers = answers;
    }

    /// Tutulan `line-finish`'i ([`Self::end_since`]) süresi dolduysa
    /// sıfırlamaya çevirir; tutma sürüyorsa kalanı döndürür.
    ///
    /// Tek çağıranı [`crate::Session::frame`], aynanın okunduğu kilit
    /// turunun **başında** ve `caret`'in `now`'ıyla: tutmanın bittiği karede
    /// bastırma, bant, caret ve dock aynı sıfırlanmış aynayı görüyor. Kalan
    /// süre saate giriyor (`Cursor::next_tick`) — tek atımlık, durma koşulu
    /// adlı (tutma doldu ya da bir `u`/işaret onu bitirdi), yani boşta
    /// sıfır kare korunuyor. `Session::dock` çağırmıyor: aynı karede
    /// `frame()`'in kararından ayrışmasın.
    pub(crate) fn expire_end(&mut self, now: Instant) -> Option<Duration> {
        let since = self.end_since?;
        let left = HANDOVER_HOLD
            .checked_sub(now.saturating_duration_since(since))
            .filter(|left| !left.is_zero());
        if left.is_none() {
            self.end_since = None;
            self.end_line();
        }
        left
    }

    /// `line-finish` tutuluyor mu ([`Self::end_since`]). Tutma yalnız kare
    /// yolunda çözülüyor ([`Self::expire_end`]); kare çizmeyen bir pencerede
    /// (örtülmüş sekme) süresi dolmuş bir tutma kalabilir ve karar veren öteki
    /// yollar (yapıştırmanın sarma kararı) onu kapalı saymalı.
    pub(crate) fn holding_end(&self) -> bool {
        self.end_since.is_some()
    }

    /// Devrin sorduğu aynanın durumu: tutulan `line-finish` **zaten
    /// gelmiş** sayılıyor (`Idle`).
    ///
    /// Ham cevap tutmadan önceki gibi `e` anında `Grid`'e dönüyor ve caret
    /// tutması ([`HANDOVER_HOLD`]) aynı andan sayıyor — iki tutma aynı saatte
    /// bitiyor, `line-finish` zamanlaması 032 öncesiyle aynı. Aynanın kendisi
    /// ise tutma boyunca `Live`: çizim, bant ve bastırma onu okuyor.
    fn caret_status(&self) -> DockStatus {
        if self.end_since.is_some() {
            DockStatus::Idle
        } else {
            self.dock.status
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
            (_, Outcome::Finished { .. }) => None,
        }
    }

    /// Safha `Input` iken yazılmakta olan bloğun kimliği — aynanın durumuna
    /// **bakmadan** ([`Self::suppressed_input`]'ın ayna koşulsuz hâli).
    ///
    /// Tüketicisi ekranı temizlemenin korunan ilk satırı
    /// ([`crate::Session::clear_to_start`]): imlecin satırı çıpasızsa (çok
    /// satırlı girişin boş bir satırı) blok çıpadan bulunuyor, ve orada soru
    /// "satır nerede çiziliyor" değil "hangi satırlar girişin" — aynanın
    /// canlı olup olmaması cevabı değiştirmiyor. İkinci koşul
    /// [`Self::running`]'inkiyle aynı gerekçeyle.
    pub(crate) fn input_block(&self) -> Option<u32> {
        if self.state?.phase != ShellPhase::Input {
            return None;
        }
        match self.blocks.last()? {
            (id, Outcome::Pending) => Some(id),
            (_, Outcome::Finished { .. }) => None,
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
                blank: self.dock.display_chars == 0 && self.dock.prebuffer.is_empty(),
                from_anchor: !self.dock.prebuffer.is_empty() || self.end_since.is_some(),
                last_ink: self.dock.last_ink,
                insert_keymap: self.dock.insert_keymap,
                answers: self.dock.answers,
            }),
            (_, Outcome::Finished { .. }) => None,
        }
    }

    /// Aynanın görüntüsünü (`PREDISPLAY ++ BUFFER ++ POSTDISPLAY`) `into`'ya
    /// yazar, kapasitesini koruyarak; caret'in karakter indeksini döndürür
    /// ([`DockState::cursor`], aynı uzay).
    ///
    /// Tüketicisi bastırmanın satır aritmetiği ([`crate::dock::grid_span`]) ve
    /// [`Self::suppressed_input`] ile **aynı kilit turunda** çağrılıyor.
    /// `PREBUFFER` girmiyor: ızgarada o satırlar zsh'in `PS2`'siyle çoktan
    /// basılmış, düzeni yürüyen hesabın konusu değil (032 Karar 7).
    pub(crate) fn display_into(&self, into: &mut String) -> usize {
        into.clear();
        into.push_str(&self.dock.predisplay);
        into.push_str(&self.dock.buffer);
        into.push_str(&self.dock.postdisplay);
        self.dock.cursor
    }

    /// Caret'in bu an kimin ve tutmanın kalanı — [`caret_home`]'un defter
    /// üstündeki yüzü.
    ///
    /// [`crate::Session::frame`] bunu [`Self::suppressed_input`] ile **aynı
    /// kilit turunda** okuyor: ayrı turlardan alınsalardı ikisi ayrı ana ait
    /// olurdu. Aynı gerekçe `home` ile `hold_left`'i de tek kayda topluyor —
    /// ikisi de tek bir `now`'dan çıkıyor.
    ///
    /// **Kalan süre yalnız tutma cevabı çevirirken doluyor.** Ham cevap zaten
    /// `Dock` ise ortada beklenen bir şey yok ve boşta bir pencereye kare
    /// istemek boşta sıfır kare sözleşmesini bozardı. Saatin üç şartı da
    /// burada karşılanıyor: içerik gerçekten değişiyor (caret yer değiştiriyor
    /// **ve** doluluk sayısı oynuyor), tek atımlık, ve durma koşulu
    /// adlandırılmış — tutma doldu ya da yüklem `Dock`'a geri döndü.
    ///
    /// **Uzak oturum tutmadan önce** (036 Karar 8): uzakta dock'un giriş
    /// satırı yok (`Cursor::input_rows == 0`), yani devralacak bir yüzey de
    /// yok — caret ızgarada, tutma yok. Tutma sonra uygulansaydı `C`'den
    /// hemen sonra gelen `set_remote` caret'i 150 ms boyunca bağlam satırına
    /// oturturdu; tutmanın gerekçesi (yarı yolda geri dönen caret) burada
    /// konusuz, çünkü uzak durum yalnız `D`/`A`/yeni `C` ile kalkıyor. Kalan
    /// süre de `None`: çevrilmeyen bir cevap için kare istenmez. Ham cevabın
    /// damgası ([`Self::observe_caret`]) buna bakmıyor — uzak durum safhayı
    /// değiştirmiyor.
    pub(crate) fn caret(&self, now: Instant) -> CaretDecision {
        if self.context.remote.is_some() {
            return CaretDecision {
                home: CaretHome::Grid,
                hold_left: None,
            };
        }
        let held = HANDOVER_HOLD
            .checked_sub(now.saturating_duration_since(self.caret_since))
            .filter(|left| !left.is_zero());
        let status = self.caret_status();
        let home = caret_home(self.state, status, held.is_some());
        // Karşılaştırılan iki cevap da **aynı kapıdan** geçiyor ve yalnız
        // `held`'de ayrılıyorlar: ham cevabı ikinci bir yoldan türetmek
        // (`caret_home_raw`'u doğrudan çağırmak) ikisinin ayrışmasını mümkün
        // kılardı. Ayrışsalardı `home != raw` tutmanın hiç uygulanmadığı bir
        // kolda da doğru olur ve pencere 150 ms'de bir hiçbir şeyi
        // değiştirmeyen kare isterdi — `sessiz=` jetonunun son savunma hattı
        // olduğu sessiz sızıntı sınıfı.
        let unheld = caret_home(self.state, status, false);
        CaretDecision {
            home,
            // Kalan süre **cevabın çevrilmiş olmasından** türüyor, ayrı bir
            // koşuldan değil: ikisi ayrı yazılsaydı ayrışabilirlerdi ve
            // tutmanın uygulanmadığı bir kolda (`Unavailable`) boşuna kare
            // istenirdi. Tek cümle: tutma cevabı çevirdiyse kalanı vardır.
            hold_left: held.filter(|_| home != unheld),
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
            Outcome::Finished { exit: Some(0), .. } => Some(Stripe::Success),
            Outcome::Finished { exit: Some(_), .. } => Some(Stripe::Error),
            Outcome::Finished { exit: None, .. } | Outcome::Pending => None,
        }
    }

    /// Bloğun **çizilebilir** süresi; sayaç doğurmayan her hâlde `None`.
    ///
    /// İki kaynak, tek soru: koşan blokta saatin yaşı, biten blokta defterin
    /// kaydı. Ayrı ayrı sorulsaydı çağıran "bu blok koşuyor mu" sorusunu
    /// ikinci kez sormak zorunda kalırdı ve [`Self::stripe`] ile ayrışabilirdi
    /// — ikisi de `running`'i **dışarıdan** alıyor, yani aynı karede aynı
    /// yanıta bakıyorlar.
    ///
    /// Eşik burada **uygulanmıyor**: "bir saniyeyi geçti mi" bir çizim kararı
    /// ve çizen taraf ([`crate::Session::frame`]) veriyor. Burada uygulansaydı
    /// saatin bir sonraki tikini hesaplayan yol da eşiği ikinci kez bilmek
    /// zorunda kalırdı.
    pub(crate) fn duration(&self, id: u32, running: Option<u32>) -> Option<Duration> {
        if running == Some(id) {
            // Koşan blok ama saat yok: entegrasyonun yarısı geldi (`A` var,
            // `C` yok). Uydurulmuş bir süre yerine sayaç yok.
            return self.running_since.map(|since| since.elapsed());
        }
        match self.blocks.get(id)? {
            Outcome::Finished { elapsed_ms, .. } => Some(Duration::from_millis(elapsed_ms.into())),
            Outcome::Pending => None,
        }
    }
}

/// [`Duration`]'ı milisaniyeye indirir, doyurarak.
///
/// `as` ile daraltma sarardı: 49 günden uzun süren bir komut (nohup'lanmış bir
/// derleme, unutulmuş bir `tail -f`) sayacı sıfırdan başlatırdı. Doyma yanlış
/// ama **monoton**; sarma yanlış ve şaşırtıcı.
fn millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).unwrap_or(u32::MAX)
}

/// Sayacın eşiği — bundan kısa süren komut hiç sayaç doğurmaz.
///
/// **Tasarım sabiti, ölçüm değil** (`docs/OLCUMLER.md`'ye girmez): her `ls`'in
/// yanında `0.01s` yazması gürültü olurdu, bir saniyeyi geçen komut ise iki
/// soru doğuruyor — koşarken "asıldı mı", bitince "ne kadar sürdü" — ve
/// ikisinin cevabı aynı sayı. Referans ürün aynı eşiği ayara açıyor
/// (`docs/ARASTIRMA.md` → `command_duration_threshold`); bizde bugün sabit.
pub(crate) const COUNTER_FLOOR: Duration = Duration::from_secs(1);

/// Sayacın çözünürlüğü — **koşan** ile **bitmiş** komutta ayrı, ve ayrımın
/// sebebi hem okuma hem pil.
///
/// Koşan sayaç her değişiminde bir kare istiyor (013 phase-2, saat). Onda bir
/// gösterseydi **saniyede on kare** ederdi, oysa koşarken sorulan soru
/// "asıldı mı" ve ondalık gürültüden ibaret. Bitmiş değer ise **donmuş**:
/// hiçbir kareye mal olmuyor, yani orada ondalığın bedeli sıfır ve bilgisi
/// gerçek — iki koşuyu karşılaştıran için `2.1s` ile `2.9s` fark eder.
///
/// Görünen sonuç: sayaç `1s, 2s, 3s` diye ilerliyor ve komut bitince `3.4s`
/// diye **oturuyor**. Sıçrama değil, kesinleşme.
///
/// **Ondalığın sınırı saniye kademesi, on saniye değil** (kullanıcı kararı):
/// bitmiş `45s` de `45.3s` olarak oturuyor, çünkü "asıl sayı" sorusu on
/// saniyeden sonra da geçerli ve bedeli yok. Dakika kademesinden itibaren
/// ondalık düşüyor — `1m 05.3s` hem uzun hem okunmuyor; orada aranan şey
/// zaten kaba büyüklük.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Precision {
    /// Koşan komut: tam saniye.
    Whole,
    /// Bitmiş komut: dakikanın altında onda bir.
    Tenths,
}

/// Koşan sayacın bir sonraki **görünür** değişimine kalan süre.
///
/// Saatin tek girdisi (013 phase-2) ve biçimin doğrudan sonucu: koşan sayaç
/// tam saniye gösterdiği için sınır bir sonraki tam saniye. Biçim değişirse
/// burası da değişmek zorunda ve ikisi yan yana duruyor — `bt-gpu` "ne zaman"
/// sorusunu hiç sormuyor, yalnız verilen süreyi bekliyor.
///
/// Eşiğin altında bir sonraki değişim sayacın **belirmesi**: `sleep 5`'in
/// ilk karesi eşikten önce çizilirse saat 1 saniyeye kuruluyor, 16 ms'ye
/// değil.
pub(crate) fn next_tick(elapsed: Duration) -> Duration {
    if elapsed < COUNTER_FLOOR {
        return COUNTER_FLOOR - elapsed;
    }
    // **Kademe başına ayrı çözünürlük.** Saat kademesinde metin (`1h 07m`)
    // dakikada bir değişiyor; saniyede bir uyandırmak bir saatte 3540
    // **aynı** kareyi çizdirirdi (`/code-review`, 013 kapı) ve modül
    // başlığına yeni yazdığımız "içerik gerçekten değişecek" şartını ilk
    // ihlal eden biz olurduk.
    let period = if elapsed.as_secs() < 3600 {
        Duration::from_secs(1)
    } else {
        Duration::from_secs(60)
    };
    // Bir sonraki tam sınıra kalan süre. Kalan sıfırsa tam periyot dönüyor:
    // sıfır süreli bir saat callback'i döngüye sokardı.
    let since =
        Duration::from_nanos(u64::try_from(elapsed.as_nanos() % period.as_nanos()).unwrap_or(0));
    period - since
}

/// Sayacın metni — **yığında**, kare başına ayırma yok.
///
/// `String` olsaydı koşan her blok için her karede bir ayırma ederdi: metin
/// saniyede bir değişiyor ama her karede yeniden üretiliyor.
///
/// Tavan temsil edilebilir en uzun metinden geliyor ve sabit bir tahmin değil:
/// süre `u32` milisaniye, yani en çok ~1193 saat, yani en uzun metin
/// `"1193h 03m"` — dokuz bayt. Tampon bir sınama ile bağlı
/// ([`the_longest_counter_fits_the_buffer`]).
pub(crate) struct Counter {
    text: [u8; Counter::CAPACITY],
    len: usize,
}

impl Counter {
    const CAPACITY: usize = 12;

    /// Süreyi metne çevirir.
    ///
    /// Dört kademe ve hepsi okuma sorusundan: onda bir, saniye, dakika, saat.
    /// Kırpma **yuvarlamanın yerine** bilinçli — `1.9s` yazarken 2.0 saniyeyi
    /// geçmiş bir komut olmasın; sayaç ileri değil geri dürüst olur.
    pub(crate) fn new(duration: Duration, precision: Precision) -> Self {
        let mut counter = Self {
            text: [0; Self::CAPACITY],
            len: 0,
        };
        let secs = duration.as_secs();
        // `write!` bir `fmt::Result` döndürüyor ve tek hata kolu tamponun
        // dolmasıdır — o da yukarıdaki tavanla temsil edilemez, bekçisi
        // `the_longest_counter_fits_the_buffer`. Sonucu yutmak yerine
        // `debug_assert` ile bağlanıyor: PTY yolunda panik yok.
        let written = if precision == Precision::Tenths && secs < 60 {
            let tenths = duration.as_millis() / 100;
            write!(counter, "{}.{}s", tenths / 10, tenths % 10)
        } else if secs < 60 {
            write!(counter, "{secs}s")
        } else if secs < 3600 {
            write!(counter, "{}m {:02}s", secs / 60, secs % 60)
        } else {
            write!(counter, "{}h {:02}m", secs / 3600, (secs / 60) % 60)
        };
        debug_assert!(written.is_ok(), "sayaç tamponu doldu: {duration:?}");
        counter
    }

    pub(crate) fn as_str(&self) -> &str {
        // Yazan tek yol `write_str` ve o `&str` alıyor, yani tampon her zaman
        // geçerli UTF-8. Bozuk kol boş dizgiye düşüyor: sayaç için panik
        // etmek, PTY yolunda panik yasağının ihlali olurdu.
        std::str::from_utf8(&self.text[..self.len]).unwrap_or("")
    }
}

impl fmt::Write for Counter {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(fmt::Error)?;
        let slot = self.text.get_mut(self.len..end).ok_or(fmt::Error)?;
        slot.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
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

/// Dizin kolunun OSC numarası — bizim seçimimiz değil, uyduğumuz sözleşme.
///
/// `7` "çalışma dizini" için fiilî standart: iTerm2, kitty, WezTerm, GNOME
/// Terminal ve VS Code aynı numarayı okuyor, oh-my-zsh'in `termsupport.zsh`'i
/// de aynı numarayı basıyor. Kendi numaramızı seçseydik yalnız kendi
/// betiğimizin bastığını görürdük.
///
/// **`vte` onu tanımıyor** ve bu, [`DOCK_OSC`] ile aynı durum: yük
/// `osc_dispatch`'in `unhandled` koluna düşüp atılıyor
/// (`vte-0.15.0/src/ansi.rs`; yorumlanan numaralar 0, 2, 4, 8, 10–12, 22, 50,
/// 52, 104 ve 110–112). Yani "alacritty'nin `Title` olayı sessizce düşüyor"
/// değil — **hiçbir olay doğmuyor**; dizin ancak bu kolla görülebiliyor.
const CWD_OSC: u32 = 7;

/// `ESC ] 7 ;` yükünün üst sınırı, bayt.
///
/// Sayı türetildi, seçilmedi ([`DOCK_PAYLOAD_LIMIT`] emsali):
///
/// - macOS'ta bir yolun tavanı `PATH_MAX`, yani 1024 bayt.
/// - En kötü hâlde her bayt yüzde kodlu → 3072.
/// - Üstüne `file://` şeması ile yetki bölümü.
/// - Yuvarlanmış tavan: **4 KiB**.
///
/// **Aşımın sonucu sessiz** ve bu, aynanın görünür aşımından (`DockFault`)
/// bilerek ayrı: gösteremediğimiz bir giriş satırı kullanıcının yazdığını
/// kaybettirir, gösteremediğimiz bir dizin ise yalnız bir önceki değeri
/// ekranda bırakır ve sonraki prompt onu tazeler.
const CWD_PAYLOAD_LIMIT: usize = 4 * 1024;

/// Numara önekinin makul üst sınırı; aşan dizi bizim değildir.
///
/// `ESC ]` ardından rakam basıp sonlandırıcı basmayan bir akışta sayaç
/// taşmasın diye var; `133`'ün altı hane uzağında bir OSC numarası yok.
const MAX_OSC_NUMBER: u32 = 999_999;

/// CSI parametresinin makul üst sınırı; aşan dizi bizim değildir.
///
/// [`MAX_OSC_NUMBER`]'ın emsali ve aynı işi görüyor: sonlandırıcı basmayan bir
/// akışta sayaç taşmasın. Aşımın cezası da aynı yönde — dizi **atılmıyor**,
/// yalnız tanınmaz oluyor; `2`'nin altı hane uzağında bir ED parametresi yok.
const MAX_CSI_PARAM: u32 = 999_999;

/// ED'nin "bütün ekranı temizle" parametresi: `CSI 2 J`.
///
/// `3J` (`ClearMode::Saved`) ve RIS için kol **yok** ve gerekçe ikisinde de
/// aynı: ikisi de `clear_history()` çağırıyor (`term/mod.rs:1806`, RIS için
/// `grid/mod.rs:341`), yani `history_size()` sıfıra iniyor ve "boşluğu
/// geçmişle doldur" kuralı **kendiliğinden** kapanıyor. Üçüncü bir kol
/// eklemek, bir bayrakla zaten kapalı olan bir yolu ikinci kez kapatmak
/// olurdu.
const ERASE_ALL: u32 = 2;

/// Tarayıcının nerede olduğu. Chunk sınırında hayatta kalması gereken şey
/// yükün kendisi **değil**, bu durumun tamamı: "ESC gördüm" ve "rakamların
/// ortasındayım" da iki `read()` arasında taşınır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanState {
    /// Dizinin dışındayız; bir sonraki `ESC` aranıyor.
    Ground,
    /// `ESC` görüldü, `]` (OSC) ya da `[` (CSI) bekleniyor.
    Escape,
    /// `ESC ]` görüldü, OSC numarası toplanıyor.
    Number,
    /// Bizim bir numaramız ve `;` görüldü; yük o kolun tamponuna toplanıyor.
    Payload(Arm),
    /// Bizim dizimiz değil (ya da sınırı aştı): sonlandırıcıya kadar atlanıyor.
    Skip,
    /// `ESC [` görüldü; CSI sonlandırıcısına kadar izleniyor.
    Csi(CsiScan),
}

/// CSI dizisinin bizi ilgilendiren kadarı.
///
/// **Tampon yok ve olmayacak:** tanıdığımız tek dizinin parametresi tek bir
/// sayı, yani toplanacak yük de yok. `vte`'nin dört CSI durumu
/// (`advance_csi_entry`, `_param`, `_intermediate`, `_ignore`) bizde **tek**
/// duruma iniyor, çünkü sorduğumuz soru tek: "bu dizi `CSI 2 J` mi". Ara
/// baytı, özel işareti ya da ikinci parametresi olan her dizinin cevabı aynı —
/// hayır — ve o cevabı [`Self::simple`] taşıyor.
///
/// `Default` **türetilmiyor**: türetilseydi `simple: false` olurdu, yani
/// "hiçbir dizi tanınmaz" — sessizce yanlış bir başlangıç. Tek doğru
/// başlangıcın adı [`CsiScan::new`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CsiScan {
    /// Toplanan tek parametre.
    param: u32,
    /// Hiç rakam görüldü mü. Parametresiz `CSI J` **ED 0** demek (imleçten
    /// aşağısı), yani bizim dizimiz değil; ayrı bayrak olmasaydı `param`'ın
    /// sıfırı ile "hiç yazılmadı" karışırdı.
    has_digit: bool,
    /// Dizi hâlâ "tek parametreli, işaretsiz, ara baytsız" mı.
    simple: bool,
}

impl CsiScan {
    /// `ESC [`'in hemen ardındaki hâl.
    fn new() -> Self {
        Self {
            param: 0,
            has_digit: false,
            simple: true,
        }
    }

    /// Dizi tanıdığımız tek dizi mi — sonlandırıcısı da dahil.
    fn is_erase_all(&self, final_byte: u8) -> bool {
        self.simple && self.has_digit && self.param == ERASE_ALL && final_byte == b'J'
    }
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
    /// [`CWD_OSC`] — dizin kolu.
    Cwd,
}

/// [`ShellLog::apply_scan_answering`]'in cevabı: okuyucu thread'in kilidi
/// bıraktıktan sonra vereceği haberler (036).
///
/// Tek dönüş yolu, iki haber: ikinci bir yol açmak (defterde bayrak, ayrı
/// sorgu) haberi kilidin ikinci bir turuna bağlardı.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ScanOutcome {
    /// Başlığın girdisi değişti — farklı bir yerel dizin ya da uzak durumun
    /// silinmesi → [`crate::Wake::title_changed`].
    pub(crate) title: bool,
    /// Safha `Running`'e **geçti** → [`crate::Wake::command_started`].
    pub(crate) started: bool,
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
    /// Çalışma dizini, **çözülmüş** tam yol, ve yetkinin bu makine olup
    /// olmadığı ([`LOCAL_AUTHORITIES`]). Reddedilen bir URI hiç olay
    /// doğurmuyor: "dizin okunamadı" diye bir hâl yok, çünkü doğru cevap
    /// eskisini bırakmak. Yabancı yetki 036'dan beri reddedilmiyor, uzak
    /// yuvaya gidiyor ([`ShellLog::apply_scan_answering`]).
    Cwd {
        path: &'a str,
        local: bool,
    },
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
    /// Git dalı; boş gövde "depo değil" demek. Aynanın kanalından geliyor
    /// (`precmd` basıyor) ama aynanın **durumuna** dokunmuyor.
    Branch(&'a str),
    /// Düzenleme widget'ı bu prompt'ta bağlı (`line-init`, 031); aynanın
    /// durumuna dokunmuyor ([`ShellLog::dock_editable`]).
    Editable,
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
    /// `7;` sonrası yük. Üçüncü tampon, üçüncü sınır — aynı gerekçe: bir
    /// tampon üç sınıra birden uyamaz.
    cwd: Vec<u8>,
    /// base64 çıktısının indiği ara tampon; her alanda yeniden kullanılır.
    decoded: Vec<u8>,
    /// Aynanın çözülmüş hâli — [`DockEvent::Update`]'in ödünç verdiği tampon.
    line: DockState,
    /// Çözülmüş dizin — [`ScanEvent::Cwd`]'in ödünç verdiği tampon.
    path: String,
    /// Çözülmüş dal — [`DockEvent::Branch`]'in ödünç verdiği tampon.
    branch: String,
    /// Toplanan OSC numarası ve hiç rakam görülüp görülmediği.
    number: u32,
    has_digit: bool,
    /// Son boşaltmadan bu yana görülen `CSI 2 J` sayısı.
    ///
    /// **Olay değil sayaç**, ve ayrım kasıtlı: [`ScanEvent`] "olay başına tek
    /// kilit turu" için var (kendi doc'u), CSI kolunun tüketicisi ise hiç
    /// kilit istemiyor — artıracağı şey bir atomik. Enum'a kol eklemek her
    /// olay yolunda bedelsiz ama anlamsız bir dal açardı.
    screen_clears: u32,
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
            // Dizin kolu prompt **başına** koşuyor, tuş başına değil; tampon
            // yine de baştan ayrılıyor, çünkü ölçüsü 4 KiB ve büyüyerek gelen
            // bir tampon ilk prompt'larda ayırma yapardı.
            cwd: Vec::with_capacity(CWD_PAYLOAD_LIMIT),
            decoded: Vec::new(),
            line: DockState::default(),
            path: String::new(),
            branch: String::new(),
            number: 0,
            has_digit: false,
            screen_clears: 0,
        }
    }

    /// Aynayı kümeyle okuyan tarayıcı ([`DockState::cluster`]); açılışta
    /// bir kez, `SessionOptions::cluster`'dan.
    pub(crate) fn cluster(mut self, on: bool) -> Self {
        self.line.cluster = on;
        self
    }

    /// Son çağrıdan bu yana görülen `CSI 2 J` sayısı; sayacı **boşaltır**.
    ///
    /// Sayı, çünkü tüketicisi bir **nesil sayacına** ekliyor
    /// (`Session::screen_clears`): kare yolunun sorduğu soru "ekran temizlendi
    /// mi" değil, "bu temizlemeyi hesaba kattım mı". Bayrak olsaydı iki
    /// temizlemenin arasına düşen bir kare ikincisini birincisi sanardı.
    pub(crate) fn take_screen_clears(&mut self) -> u32 {
        std::mem::take(&mut self.screen_clears)
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
            // `vte::advance_esc`: `]` OSC dizisini, `[` de CSI'yı açar; kalan
            // her şey (DCS, tek harfli kaçışlar) bizi ilgilendirmiyor.
            ScanState::Escape => match byte {
                b']' => {
                    self.state = ScanState::Number;
                    self.number = 0;
                    self.has_digit = false;
                }
                // **Dördüncü kol.** Bugüne kadar aşağıdaki `_ =>` ile
                // `Ground`'a düşüyordu; artık izleniyor, çünkü `CSI 2 J`
                // aranıyor ve çerçevelenmeyen bir CSI'nın içindeki `]` bizde
                // sahte bir OSC açabilirdi.
                b'[' => self.state = ScanState::Csi(CsiScan::new()),
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
                        (true, CWD_OSC) => {
                            self.cwd.clear();
                            ScanState::Payload(Arm::Cwd)
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
                    let outcome = parse_dock(
                        &self.dock,
                        &mut self.decoded,
                        &mut self.line,
                        &mut self.branch,
                    );
                    self.close(byte);
                    on_event(ScanEvent::Dock(match outcome {
                        DockOutcome::Update => DockEvent::Update(&self.line),
                        DockOutcome::End => DockEvent::End,
                        DockOutcome::Unavailable(fault) => DockEvent::Unavailable(fault),
                        DockOutcome::Branch => DockEvent::Branch(&self.branch),
                        DockOutcome::Editable => DockEvent::Editable,
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
            ScanState::Payload(Arm::Cwd) => {
                if is_terminator(byte) {
                    // Çözme `close`'dan **önce**: `close` tamponu boşaltıyor.
                    let read = parse_cwd(&self.cwd, &mut self.decoded, &mut self.path);
                    self.close(byte);
                    if let Some(local) = read {
                        on_event(ScanEvent::Cwd {
                            path: &self.path,
                            local,
                        });
                    }
                } else if is_ignored(byte) {
                } else if self.cwd.len() == CWD_PAYLOAD_LIMIT {
                    // Aşım **sessiz**, aynanın aksine: gösteremediğimiz bir
                    // dizin eski değeri ekranda bırakıyor ve sonraki prompt
                    // onu tazeliyor (`CWD_PAYLOAD_LIMIT`'in doc'u).
                    self.state = ScanState::Skip;
                } else {
                    self.cwd.push(byte);
                }
            }
            ScanState::Skip => {
                if is_terminator(byte) {
                    self.close(byte);
                }
            }
            // **OSC'nin çerçeveleme kuralları burada geçerli değil ve bu tek
            // başına bir kusur kaynağıydı:** `is_terminator` `BEL`'i (0x07)
            // dizi sonu sayıyor, `vte`'nin CSI durumları ise onu yerinde
            // `execute` edip durumu **değiştirmiyor** — yani `ESC [ 2 BEL J`
            // hâlâ bir ED 2. İki kümeyi paylaştırmak, ızgaranın gördüğü dizi
            // sınırı ile bizimkini ayırırdı; iki taraf aynı akıştan iki
            // farklı hikâye okur (modül başlığı).
            ScanState::Csi(mut csi) => match byte {
                // **İptal kuralları `vte::anywhere`'den birebir** (R1.3).
                // Taşınmasaydı bozuk bir CSI durumu takar ve peşinden gelen
                // `ESC ] 133;…` yutulurdu — bloklar, bastırma ve dock
                // **sessizce** ölürdü.
                0x18 | 0x1a => self.state = ScanState::Ground,
                0x1b => self.state = ScanState::Escape,
                // Dizinin içindeki C0'lar yerinde `execute` ediliyor; durum
                // duruyor, parametre etkilenmiyor.
                0x00..=0x17 | 0x19 | 0x1c..=0x1f => {}
                b'0'..=b'9' => {
                    csi.param = csi
                        .param
                        .saturating_mul(10)
                        .saturating_add(u32::from(byte - b'0'));
                    csi.has_digit = true;
                    if csi.param > MAX_CSI_PARAM {
                        csi.simple = false;
                    }
                    self.state = ScanState::Csi(csi);
                }
                // Ara baytlar (0x20–0x2F), parametre ayraçları (`;`, `:`) ve
                // özel işaretler (`<=>?`) — üçünün de cevabı aynı: dizi bizim
                // değil, ama **çerçeveleme sürüyor**. `ESC [ ? 1049 h` ne
                // bayrak kuruyor ne durumu takıyor.
                0x20..=0x3f => {
                    csi.simple = false;
                    self.state = ScanState::Csi(csi);
                }
                0x40..=0x7e => {
                    if csi.is_erase_all(byte) {
                        // **Doyuran toplama, saran değil.** Tüketici sayacı
                        // her okuma turunda boşaltıyor, yani tavana ancak tek
                        // bir `read()` içinde dört milyar temizlemeyle
                        // varılır; sarsaydı o okuma `0` döndürür ve "hiç
                        // temizleme olmadı" derdi — kaybın yanlış yönü.
                        self.screen_clears = self.screen_clears.saturating_add(1);
                    }
                    self.state = ScanState::Ground;
                }
                // `0x7F` ve 0x7F'ten büyük her şey `anywhere`'in `_ => ()`
                // kolu: yoksayılıyor, durum duruyor.
                _ => {}
            },
        }
    }

    /// Diziyi kapatır ve sonlandırıcının kendisine göre bir sonraki duruma
    /// geçer: çıplak `ESC` diziyi bitirir **ve** yeni bir kaçışı açar
    /// (`vte::advance_osc_string`, `0x1B` kolu).
    fn close(&mut self, terminator: u8) {
        self.payload.clear();
        self.dock.clear();
        self.cwd.clear();
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

/// Bu makineyi gösteren yetki (authority) değerleri — "yerel mi" sorusunun
/// cevabı, **kabul listesi değil** (036): yabancı yetkili OSC 7 de olay
/// doğuruyor ve uzak yuvaya gidiyor ([`ShellLog::apply_scan_answering`]),
/// yalnız yerel dizine yazmıyor.
///
/// **Adlı her host yabancı sayılıyor** ve bu, "kendi ad'ımızla karşılaştır"
/// yerine bilinçli seçildi: karşılaştırma `gethostname` demek, o da `bt-core`'a
/// yeni bir bağımlılık kenarı demek (`proje.md` → Yayın etkisi: yeni bağımlılık
/// mimari karardır). Kendi betiğimiz bu yüzden **boş yetkiyle** basıyor
/// (`file:///…`), yani kapı hiçbir zaman bir ad uyuşmasına bağlı değil —
/// makine yeniden adlandırılınca sessizce kapanmıyor.
///
/// **Bilinen sınır:** `file://$HOST$PWD` basan üçüncü taraf kancalar
/// (oh-my-zsh'in `termsupport.zsh`'i gibi) yerel dizine yazmıyor — uzak
/// yuvaya düşüyorlar, o da yalnız uzak oturum etkinken okunuyor ve `C`, `D`,
/// `A`'da siliniyor, yani yerel davranış 036'dan önceki gibi. Kayıp yalnız komutun
/// *ortasında* yapılan bir `cd`'nin canlı yansıması; dizin bir sonraki
/// prompt'ta kendi `precmd`'imizden zaten geliyor. İstenirse çare yine
/// bağımlılık değil politika: `bt-shell` (elinde `libc` var) adı okur ve
/// `SessionOptions` ile geçirir — `decide_locale` emsali.
const LOCAL_AUTHORITIES: [&str; 2] = ["", "localhost"];

/// `7;` sonrasındaki URI'yi çizilebilir bir yola çevirir ve yetkinin yerel
/// olup olmadığını döndürür; tanımadığını **yoksayar** (`None`).
///
/// **Yük alanlara bölünmüyor** ([`parse_mark`] ve [`parse_dock`]'un aksine):
/// `;` bir dosya adında geçerli bir karakter ve yükü bölseydik `/tmp/a;b`
/// yolunu `/tmp/a` diye okurduk.
///
/// Reddedilen her hâlin sonucu aynı ve **panik değil yoksayma**
/// (`CLAUDE.md` → PTY yolunda panik yok): şema `file:` değil, yetki UTF-8
/// değil, yol `/` ile başlamıyor, yüzde kaçışı bozuk ya da sonuç UTF-8 değil.
/// Yabancı yetki bir ret değil (036): cevap `Some(false)`.
fn parse_cwd(payload: &[u8], decoded: &mut Vec<u8>, into: &mut String) -> Option<bool> {
    // Şema harf duyarsız (RFC 3986 §3.1); `file:` beş bayt.
    let rest = payload
        .get(..5)
        .filter(|head| head.eq_ignore_ascii_case(b"file:"))?;
    let rest = &payload[rest.len()..];
    // Yetki bölümü **zorunlu**: `file:/tmp` biçimi geçerli bir URI ama onu da
    // kabul etmek "yetki yok" ile "yetki boş"u tek kola indirirdi ve pratikte
    // hiçbir kabuk basmıyor.
    let rest = rest.strip_prefix(b"//".as_slice())?;
    let at = rest.iter().position(|&b| b == b'/')?;
    let (authority, path) = rest.split_at(at);
    let authority = std::str::from_utf8(authority).ok()?;
    let local = LOCAL_AUTHORITIES
        .iter()
        .any(|local| authority.eq_ignore_ascii_case(local));

    decoded.clear();
    decode_percent(path, decoded)?;
    let text = std::str::from_utf8(decoded).ok()?;
    into.clear();
    into.push_str(text);
    Some(local)
}

/// Yüzde kaçışlarını çözer; `%` iki onaltılık haneyle **gelmek zorunda**.
///
/// Bozuk bir kaçışı olduğu gibi geçirmek de bir seçenekti ve reddedildi:
/// `%zz` taşıyan bir yol ya kodlayıcının bozulduğunu ya da yükün bizim
/// olmadığını söyler; ikisinde de doğru cevap yolu hiç göstermemek.
fn decode_percent(input: &[u8], out: &mut Vec<u8>) -> Option<()> {
    let mut rest = input;
    while let Some((&byte, tail)) = rest.split_first() {
        if byte != b'%' {
            out.push(byte);
            rest = tail;
            continue;
        }
        let digits = tail.get(..2)?;
        let high = char::from(digits[0]).to_digit(16)?;
        let low = char::from(digits[1]).to_digit(16)?;
        // audit: iki onaltılık hane en çok 0xff; `u8`'e sığar.
        out.push((high * 16 + low) as u8);
        rest = &tail[2..];
    }
    Some(())
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
    Branch,
    Editable,
}

/// Ayna yükünü çözer ve `line`'a yazar.
///
/// **Tel biçimi** (alanlar `;` ile, gövdeler base64):
///
/// ```text
/// ESC ] 8133 ; u ; {CURSOR} ; {PREDISPLAY} ; {BUFFER} ; {POSTDISPLAY} ; {region_highlight} BEL
/// ESC ] 8133 ; e BEL
/// ESC ] 8133 ; o BEL
/// ESC ] 8133 ; b ; {dal} BEL
/// ESC ] 8133 ; w BEL
/// ```
///
/// `u` satırı tazeler, `e` (`line-finish`) kapatır, `o` kabuğun "bu görüntü
/// aynaya sığmıyor" demesidir, `b` de dock'un bağlam satırındaki dalı taşır.
/// `w` (031) "bu prompt'ta düzenleme widget'ı bağlı" der: terminalin
/// kabuğa gönderdiği tek dizinin (`CSI 8133 ~`) ön koşulu; yükü yok ve
/// aynanın durumuna dokunmuyor, `b` gibi.
///
/// **`b` aynanın kanalında ama aynanın parçası değil:** tuş başına değil
/// **prompt başına** geliyor (`precmd`) ve satırın durumuna dokunmuyor. Kendi
/// OSC numarasını hak etmiyor — dizinin aksine (`CWD_OSC`) dal için bir
/// sözleşme yok, yani yeni bir numara yalnız bizim betiğimizin bastığı ikinci
/// bir kanal olurdu. **Fazladan alan
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
fn parse_dock(
    payload: &[u8],
    decoded: &mut Vec<u8>,
    line: &mut DockState,
    branch: &mut String,
) -> DockOutcome {
    let mut fields = payload.split(|&b| b == b';');
    let Some(op) = fields.next() else {
        return unavailable(line, DockFault::Malformed);
    };
    match op {
        b"e" => DockOutcome::End,
        // Dalın bozukluğu aynayı düşürmüyor: `Unavailable` "giriş satırını
        // gösteremiyorum" demek ve ızgarayı devreye sokuyor, oysa okunamayan
        // bir dal yalnız bağlam satırının bir yarısı. Bozuk gövde dalı
        // **boşaltıyor** — yanlış dal göstermektense dalsız bir satır.
        //
        // **Alanın hiç olmaması da aynı kapıdan geçiyor** (`b` ile `b;`
        // arasındaki fark bir kodlayıcı ayrıntısı ve ikisi de "dal yok"
        // demek). Bir zamanlar bu kol `Malformed` döndürüyordu ve o, tam da
        // üstteki cümlenin yasakladığı şeydi: kesilmiş bir `b` dizisi giriş
        // satırını dock'tan düşürüp ızgaraya geri gönderiyordu
        // (`/code-review`, 012 phase-6).
        b"b" => {
            branch.clear();
            if let Some(field) = fields.next() {
                decoded.clear();
                if decode_base64(field, decoded).is_some()
                    && let Ok(text) = std::str::from_utf8(decoded)
                {
                    branch.push_str(text);
                }
            }
            DockOutcome::Branch
        }
        // **Aşımın kabuk tarafındaki ucu.** [`DOCK_PAYLOAD_LIMIT`] yükü burada
        // keserken kabuk onu **kodlamış** oluyor; `o` kodlamadan önce
        // ölçtüğünü söylüyor. İkisi aynı bütçenin iki yakası ve ayrı ayrı
        // gerekli: bu uç kabuğun tuş başına harcadığı zamanı, öteki uç bizim
        // belleğimizi koruyor. Sonucu aynı olmak **zorunda**, yoksa sınırın
        // hangi tarafta tutulduğu kullanıcıya farklı davranış olarak yansırdı.
        b"o" => unavailable(line, DockFault::Overflow),
        b"w" => DockOutcome::Editable,
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

/// [`DockState::last_ink`]'in kümeli hâli: görüntünün son satırının son
/// mürekkepli **kümesinin** baş karakteri. Kümeler ancak ileri doğru
/// yürünebiliyor ([`crate::cluster::Walk`]), yani son `\n`'den sonrası
/// baştan taranıyor — satır sonunda cevap sıfırlanıyor.
fn last_cluster_ink(line: &DockState) -> Option<char> {
    let display = line
        .predisplay
        .chars()
        .chain(line.buffer.chars())
        .chain(line.postdisplay.chars());
    let mut ink = None;
    crate::cluster::Walk::new().run(display, |cluster| {
        if cluster.head == '\n' {
            ink = None;
        } else if cluster.head != ' ' && cluster.head != '\t' && cluster.width > 0 {
            ink = Some(cluster.head);
        }
    });
    ink
}

/// `u` yükünün alanlarını `line`'a çözer; zorunlu beşinden biri eksik ya da
/// herhangi bir metin gövdesi bozuksa `None`. Son ikisi (`KEYMAP`,
/// `PREBUFFER`) isteğe bağlı.
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
    // Görüntünün **son satırının** sondan ilk boşluk olmayan karakteri; üç
    // gövde görüntü sırasında.
    //
    // **Son satır, son karakter değil** (032): kapının öteki yarısı
    // ızgaranın **bir** satırını tarıyor — bastırmanın alt ucu, yani
    // görüntünün son satırı. `echo a\necho b\n` yapıştırmasında zsh son satır
    // sonunu tamponda tutuyor ve o satır **boş**; ayna `'b'` deseydi (ya da
    // `'\n'` — genişliği `None` olduğu için eski süzgeçten geçiyordu) boş
    // ızgara satırıyla hiç eşleşmez ve cevapsız her karede satır bayat
    // sayılırdı. Son `\n`'den sonrası boşsa `None`, ızgaranın boş satırıyla
    // aynı cevap. Sarma bunu bozmuyor: sarılan satırın son karakteri son
    // görsel satırda.
    //
    // **Bilinen sınır, yönü güvenli** (032 phase-4): `PS2` satırında ızgara
    // kullanıcının `for> ` mürekkebini taşıyor, ayna taşımıyor (`PS2`'ye
    // dokunulmuyor ve genişliği aynada yok). `BUFFER` boşken iki taraf
    // ayrışıyor ve cevapsız karede içerik kapısı "bayat" diyor; zamansal
    // kapı (`line-init`'in aynası ⏎'in cevabı) bugün olduğu gibi kurtarıyor,
    // kurtaramadığı anda (redisplay'siz tuş) satır iki yerde görünür.
    //
    // **Kümeleme açıkken (035) ölçüt kümenin baş karakteri**: ızgara
    // `👍🏽`'yi tek hücrede tutuyor ve hücrenin `c`'si `👍`; ayna `🏽` deseydi
    // kapı 024'ün belirtisini — satır her tuşta ızgaraya fırlar — geri
    // getirirdi. Aşağıdaki üç ölçüt aynen: kümeye katılan birleştirici
    // zaten ayrı sayılmıyor, başsız birleştirici (sütunu sıfır) atlanıyor.
    line.last_ink = if line.cluster {
        last_cluster_ink(line)
    } else {
        line.predisplay
            .chars()
            .chain(line.buffer.chars())
            .chain(line.postdisplay.chars())
            .rev()
            .take_while(|&ch| ch != '\n')
            // **Ölçüt `' '` ve `'\t'`; `is_whitespace()` değil** ve bu bilerek
            // dar: kapının öteki yarısı ızgarayı tarıyor
            // (`Session::last_ink_in_row`) ve o da `frame()`'in atlama kapısına
            // çivili — orada mürekkepsizlik yalnız boşluk, spacer ve gizli hücre.
            // `is_whitespace()` deseydik satır sonu NBSP (U+00A0, U+2007, U+3000)
            // taşıyan bir tamponda ayna önceki harfi, ızgara NBSP'yi söyler,
            // ikisi hiç eşleşmez ve satır kalıcı olarak **bayat** sayılırdı: hem
            // ızgarada hem dock'ta çizilirdi.
            //
            // **Sekme ise tersi ve ölçüldü** (kullanıcı, 2026-09-18): ayna **ham**
            // tamponu taşıyor, ızgara ise **çizilmiş** hâli tutuyor. Terminal
            // sekmeyi boşluğa açtığı için o karakter hücreye hiç ulaşmıyor —
            // gerçek zsh'te boş satırda Tab `BUFFER='\t'` yapıyor, yani ayna
            // `Some('\t')`, ızgara `None` diyor ve kapı düşüyordu. Belirtisi
            // görünürdü: bastırma kalkıyor, caret dock'tan ızgaraya sıçrıyordu.
            // Sekmeyi de mürekkepsiz saymak iki yarıyı yeniden eşitliyor — `"ls\t"`
            // ikisinde de `'s'`, `"\t"` ikisinde de `None`.
            //
            // **Ham kontrol karakteri bu karşılaştırmaya hiç gelmiyor** (025):
            // ZLE `\x01`'i ızgarada `^A` diye çiziyor ve dock onu hiç çizmiyor,
            // yani o satır [`DockStatus::Control`] ile ızgarada kalıyor ve
            // bastırılmıyor. Bir dönem burada "kalan sınır" diye yazılıydı ve
            // yazıldığından kötüydü: `^A` yalnız **son** karakterken kapı
            // düşüyordu, ortadayken satır dock'a gidip kayboluyordu.
            //
            // **Üçüncü ölçüt sıfır genişlik ve o 024'te geldi** (kullanıcı
            // bildirdi, ölçüldü): birleştirici kod noktaları (VS16, ZWJ, ten
            // rengi) ızgara hücresine **hiç girmiyor** — alacritty onları
            // `CellExtra`'da tutuyor ve `cell.c` taban karakteri taşıyor. Yani
            // `❤️` (U+2764 + U+FE0F) yazan bir tamponda ayna `U+FE0F`, ızgara
            // `U+2764` diyor ve ikisi **hiçbir zaman** eşleşmiyor: kapı kalıcı
            // olarak "bayat" der, bastırma her tuşta kalkar ve giriş satırı
            // dock'tan ızgaraya fırlar. Sekmenin yukarıdaki gerekçesiyle aynı
            // cümle — ayna **ham** tamponu, ızgara **çizilmiş** hâli taşıyor — ve
            // çaresi de aynı: ızgaraya ulaşmayan karakteri ayna da saymıyor.
            //
            // Ölçüt `unicode-width`'in `Some(0)`'ı, yani `dock::column_width`'in
            // beslendiği kaynağın ta kendisi. Kontrol karakterleri `None` dönüyor
            // ve bu süzgece **girmiyor**: taşıyan satır zaten `Control`.
            .find(|ch| *ch != ' ' && *ch != '\t' && UnicodeWidthChar::width(*ch) != Some(0))
    };

    decoded.clear();
    decode_base64(fields.next()?, decoded)?;
    let entries = std::str::from_utf8(decoded).ok()?;
    line.highlights.clear();
    line.highlights.extend(
        entries
            .lines()
            .filter_map(|entry| parse_highlight(entry, predisplay_chars, display_chars)),
    );

    // KEYMAP **opsiyonel alan** ve bu, telin "fazladan alan yoksayılır"
    // kuralının ters yönü: alan phase-6'nın kapısında eklendi ve açık bir
    // pencere hâlâ eski betikle koşuyor olabilir (`plan.md` → Göç). Yokluğu
    // yükü bozmuyor, yalnız `false` bırakıyor — yani yapıştırma sarılı yola
    // döner. Yön güvenli: eksik bilgi istisnayı **kapatıyor**, açmıyor.
    line.insert_keymap = fields.next().is_some_and(|field| {
        decoded.clear();
        decode_base64(field, decoded).is_some()
            && std::str::from_utf8(decoded).is_ok_and(|name| INSERT_KEYMAPS.contains(&name))
    });
    // PREBUFFER **yedinci, isteğe bağlı gövde** (032) ve `KEYMAP`'in
    // arkasında, çünkü tel yalnız sona büyüyebiliyor: eski betikle koşan
    // pencere onu hiç göndermiyor ve yokluğu yükü bozmuyor, boş bırakıyor.
    // Bozuk bir gövde ise öteki metin gövdeleriyle aynı kuralda — yük bozuk.
    //
    // Görüntü uzayına **girmiyor**: yukarıdaki üç uzunluk ve son mürekkep onu
    // görmüyor (zsh'in `CURSOR`'ı ve `region_highlight`'ı da görmüyor).
    line.prebuffer.clear();
    if let Some(field) = fields.next() {
        decode_text(field, decoded, &mut line.prebuffer)?;
    }

    // **Dock'un çizmediği kontrol karakteri** ([`DockStatus::Control`]).
    // Sekme istisna ve gerekçesi kolun doc'unda: bilgi taşımıyor. **Satır
    // sonu da istisna** (032): dock satırı kırıyor, yani onu gösterebiliyor —
    // 032'ye kadar satır sonlu görüntü kendi kolunda (`Multiline`) ızgarada
    // kalıyordu. `PREBUFFER` de soruluyor, çünkü dock onu da çiziyor.
    if line
        .prebuffer
        .chars()
        .chain(line.predisplay.chars())
        .chain(line.buffer.chars())
        .chain(line.postdisplay.chars())
        .any(|ch| ch.is_control() && ch != '\t' && ch != '\n')
    {
        line.status = DockStatus::Control;
    }
    Some(())
}

/// Basılan tuşun **metne dönüştüğü** zsh keymap'leri.
///
/// Liste bir **izin listesi** ve öyle olmak zorunda: tanımadığımız bir keymap
/// (`bindkey -N` ile kullanıcının yarattığı, ya da zsh'in ileride ekleyeceği
/// biri) ekleme keymap'i **sayılmıyor** ve yapıştırma sarılı yoldan gidiyor.
/// Yasak listesi olsaydı her yeni keymap adı sessizce istisnaya girerdi.
///
/// Üçü de aynı şeyi söylüyor ama üç ayrı yoldan: `main` zsh'in etkin
/// bağlamasının takma adı (emacs kipinde de vi'nin **ekleme** kipinde de
/// rapor edilen değer bu), `emacs` ile `viins` de doğrudan adlandırılmış
/// hâlleri. Dışarıda kalanlar: `vicmd` (tuşlar komut), `visual`, `viopp`,
/// `isearch` ve `command` — hiçbirinde basılan bayt metne dönüşmüyor.
const INSERT_KEYMAPS: [&str; 3] = ["main", "emacs", "viins"];

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
    use crate::session::CellHalf;

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

    /// Diziyi parçalar hâlinde besler; sayacı ve arkasından görülen işaretleri
    /// birlikte döndürür — CSI kolunun iki iddiası da ("bayrağı kurdu mu",
    /// "arkasındakini yuttu mu") tek çağrıda sorulabilsin diye.
    fn clears_and_marks_of_chunks(chunks: &[&[u8]]) -> (u32, Vec<Mark>) {
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        let mut clears = 0;
        for chunk in chunks {
            scanner.feed(chunk, |event| {
                if let ScanEvent::Mark(mark) = event {
                    seen.push(mark);
                }
            });
            clears += scanner.take_screen_clears();
        }
        (clears, seen)
    }

    fn clears(bytes: &[u8]) -> u32 {
        clears_and_marks_of_chunks(&[bytes]).0
    }

    #[test]
    fn only_erase_all_sets_the_screen_clear() {
        // Tanınan **tek** dizi `CSI 2 J`. `CSI J` parametresiz ED, yani ED 0
        // (imleçten aşağısı) ve `CSI 3 J` geçmişi siliyor — ikisi de ekranı
        // kasten temizlemek değil.
        assert_eq!(clears(b"\x1b[2J"), 1);
        assert_eq!(clears(b"\x1b[02J"), 1, "başındaki sıfır diziyi bozmamalı");
        assert_eq!(clears(b"\x1b[J"), 0);
        assert_eq!(clears(b"\x1b[0J"), 0);
        assert_eq!(clears(b"\x1b[1J"), 0);
        assert_eq!(clears(b"\x1b[3J"), 0);
        assert_eq!(clears(b"\x1b[22J"), 0);
        assert_eq!(clears(b"\x1b[2K"), 0, "sonlandırıcı da eşleşmeli");
        // Özel işaret (`?`, DECSED), ikinci parametre ve ara bayt: üçü de
        // diziyi tanınmaz yapıyor.
        assert_eq!(clears(b"\x1b[?2J"), 0);
        assert_eq!(clears(b"\x1b[2;2J"), 0);
        assert_eq!(clears(b"\x1b[2 J"), 0);
        // Parametre tavanı: sayaç taşmadan dizi tanınmaz oluyor.
        assert_eq!(clears(b"\x1b[99999999999999999999J"), 0);
        // İki temizleme iki kez sayılıyor: tüketici nesil sayacı, bayrak değil.
        assert_eq!(clears(b"\x1b[2J\x1b[2J"), 2);
    }

    #[test]
    fn a_csi_never_swallows_the_mark_behind_it() {
        // **Bu sınamanın kapattığı kusur sessiz:** bozuk bir CSI'da takılan
        // tarayıcı peşinden gelen `ESC ] 133;…`'ü yutar ve bloklar, giriş
        // satırının bastırılması, dock birlikte ölür.
        let mark = vec![Mark::PromptStart { id: None }];

        // (1) Tamamlanan CSI'lardan sonra: tanınan da tanınmayan da.
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[2J\x1b]133;A\x07"]),
            (1, mark.clone())
        );
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[?1049h\x1b]133;A\x07"]),
            (0, mark.clone())
        );
        // (2) Yarım kalan CSI'yı `ESC` iptal ediyor (`vte::anywhere`), yani
        // bizim dizimiz yine açılıyor.
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[2;3\x1b]133;A\x07"]),
            (0, mark.clone())
        );
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[\x1b]133;A\x07"]),
            (0, mark.clone())
        );
        // (3) `CAN`/`SUB` diziyi `Ground`'a götürüyor; oradan yeni bir dizi
        // ancak `ESC` ile açılır.
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[2\x18", b"\x1b]133;A\x07"]),
            (0, mark.clone())
        );
        assert_eq!(
            clears_and_marks_of_chunks(&[b"\x1b[2\x18]133;A\x07"]),
            (0, vec![])
        );
        // (4) Sonlandırıcısı hiç gelmeyen bir CSI'yı da `ESC` kurtarıyor:
        // tavan yalnız parametreyi tanınmaz yapıyor, durumu bırakmıyor.
        let long = b"\x1b["
            .iter()
            .copied()
            .chain(std::iter::repeat_n(b'9', 10_000));
        let stream: Vec<u8> = long.chain(b"\x1b]133;A\x07".iter().copied()).collect();
        assert_eq!(clears_and_marks_of_chunks(&[&stream]), (0, mark));
    }

    #[test]
    fn a_bel_inside_a_csi_is_not_a_terminator() {
        // OSC'nin sonlandırıcı kümesi CSI'da geçerli **değil**: `vte`'nin CSI
        // durumları C0'ları yerinde `execute` edip durumu değiştirmiyor, yani
        // `ESC [ 2 BEL J` hâlâ bir ED 2. İki kümeyi paylaştıran bir düzenleme
        // ızgaranın gördüğü dizi sınırıyla bizimkini ayırırdı.
        assert_eq!(clears(b"\x1b[2\x07J"), 1);
        assert_eq!(clears(b"\x1b[\r2J"), 1);
        // 0x7F ve 0x7F'ten büyük baytlar da yoksayılıyor (`anywhere`'in son
        // kolu), durumu bırakmıyor.
        assert_eq!(clears(b"\x1b[2\x7fJ"), 1);
        assert_eq!(clears(b"\x1b[2\x80J"), 1);
    }

    #[test]
    fn the_screen_clear_survives_a_split_at_every_byte() {
        // Durumun tamamı chunk sınırında taşınmak zorunda: "CSI'dayım",
        // "parametre 2" ve "dizi hâlâ sade" de iki `read()` arasında yaşıyor.
        let seq: &[u8] = b"\x1b[2J";
        for at in 0..=seq.len() {
            let (head, tail) = seq.split_at(at);
            assert_eq!(
                clears_and_marks_of_chunks(&[head, tail]).0,
                1,
                "bölünme noktası {at}"
            );
        }
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

    /// Bloğun kaydettiği çıkış kodu; defterde yoksa ya da hâlâ açıksa `None`.
    ///
    /// Aşağıdaki sınamalar **kodu** soruyor, süreyi değil: geçen süre gerçek
    /// saatten geliyor ve eşitlenemez. `Outcome`'ın tamamıyla karşılaştırmak
    /// onları saate bağımlı ve kırılgan yapardı.
    fn exit_of(log: &ShellLog, id: u32) -> Option<Option<i32>> {
        match log.blocks.get(id)? {
            Outcome::Finished { exit, .. } => Some(exit),
            Outcome::Pending => None,
        }
    }

    #[test]
    fn the_log_remembers_each_block_by_its_id() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        run_block(&mut log, 1, Some(0));
        run_block(&mut log, 2, Some(130));
        log.apply(Mark::PromptStart { id: Some(3) });

        assert_eq!(exit_of(&log, 1), Some(Some(0)));
        assert_eq!(exit_of(&log, 2), Some(Some(130)));
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
        assert_eq!(exit_of(&log, 3), Some(Some(0)));
        assert_eq!(exit_of(&log, BLOCK_LOG_FLOOR as u32 + 2), Some(Some(0)));
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

        assert_eq!(exit_of(&log, 1), Some(Some(3)));
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
        assert_eq!(exit_of(&log, 1), Some(Some(0)));
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
        Branch(String),
        Editable,
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
                        DockEvent::Branch(branch) => DockSnapshot::Branch(branch.to_owned()),
                        DockEvent::Editable => DockSnapshot::Editable,
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
    fn a_tab_in_the_buffer_carries_no_ink() {
        // **Ölçülmüş kusur** (kullanıcı, 2026-09-18): dock boşken Tab'a
        // basınca caret dock'tan ızgaraya sıçrıyordu. Zincir gerçek zsh'te
        // ölçüldü — Tab `BUFFER='\t'` yapıyor, ayna `Some('\t')` diyordu,
        // ızgara ise sekmeyi boşluğa açtığı için `None`; tazelik kapısı
        // (`Session::frame`) eşleşmeyince bastırma kalkıyor ve caret'in sahibi
        // değişiyordu.
        assert_eq!(dock_line(&dock_update(1, "", "\t", "", &[])).last_ink, None);
        // Sekme **sondayken** de önceki harfi bırakmalı: ızgarada o satırın
        // son mürekkebi yine `s`.
        assert_eq!(
            dock_line(&dock_update(3, "", "ls\t", "", &[])).last_ink,
            Some('s')
        );
        // Sekmenin önündeki metin etkilenmiyor.
        assert_eq!(
            dock_line(&dock_update(3, "", "\tls", "", &[])).last_ink,
            Some('s')
        );
        // Boşluğun kuralı değişmedi ve mürekkep hâlâ mürekkep.
        assert_eq!(
            dock_line(&dock_update(3, "", "ls ", "", &[])).last_ink,
            Some('s')
        );
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

    /// Dizin kolunun çözdüğü **yerel** yollar.
    fn cwd_events(bytes: &[u8]) -> Vec<String> {
        cwd_events_of(bytes, true)
    }

    /// Dizin kolunun çözdüğü, yetkisi yerel olan (`local`) ya da olmayan
    /// yollar.
    fn cwd_events_of(bytes: &[u8], local: bool) -> Vec<String> {
        let mut scanner = Scanner::new();
        let mut seen = Vec::new();
        scanner.feed(bytes, |event| {
            if let ScanEvent::Cwd { path, local: is } = event
                && is == local
            {
                seen.push(path.to_owned());
            }
        });
        seen
    }

    #[test]
    fn the_cwd_arm_decodes_a_percent_encoded_path() {
        // Yüzde çözme: boşluk ve çok baytlı karakter.
        assert_eq!(
            cwd_events(b"\x1b]7;file:///Users/a%20b/%C3%A7\x07"),
            ["/Users/a b/ç"]
        );
        // İki yetki de bu makine: boş ve `localhost`.
        assert_eq!(cwd_events(b"\x1b]7;file://localhost/tmp\x07"), ["/tmp"]);
        // Şema harf duyarsız (RFC 3986).
        assert_eq!(cwd_events(b"\x1b]7;FILE:///tmp\x07"), ["/tmp"]);
        // Yük alanlara **bölünmüyor**: `;` taşıyan bir yol geçerli.
        assert_eq!(cwd_events(b"\x1b]7;file:///tmp/a;b\x07"), ["/tmp/a;b"]);
        // Kodlanmamış bir yol da okunuyor: yüzde kodlaması zorunlu değil.
        assert_eq!(cwd_events(b"\x1b]7;file:///tmp/plain\x07"), ["/tmp/plain"]);
    }

    #[test]
    fn a_named_host_is_foreign_and_a_broken_uri_is_ignored() {
        // Adlı host **yabancı** (036): olay doğuruyor ama yerel değil, yani
        // yerel dizine hiç yazmıyor — uzak yuvaya gidiyor.
        let named = b"\x1b]7;file://remote.example/tmp\x07";
        assert_eq!(cwd_events(named), Vec::<String>::new());
        assert_eq!(cwd_events_of(named, false), ["/tmp"]);
        // Kalanların tek yanıtı: hiçbir olay. Yön güvenli — eski yol ekranda
        // kalıyor.
        for sequence in [
            &b"\x1b]7;/tmp\x07"[..],          // şema yok
            b"\x1b]7;http://host/tmp\x07",    // yabancı şema
            b"\x1b]7;file:/tmp\x07",          // yetki bölümü yok
            b"\x1b]7;file://localhost\x07",   // yol yok
            b"\x1b]7;file:///tmp/%zz\x07",    // bozuk yüzde
            b"\x1b]7;file:///tmp/%e0%80\x07", // UTF-8 değil
            b"\x1b]7;\x07",                   // boş yük
        ] {
            assert!(
                cwd_events(sequence).is_empty() && cwd_events_of(sequence, false).is_empty(),
                "dizi geçti: {}",
                String::from_utf8_lossy(sequence)
            );
        }
    }

    #[test]
    fn an_oversized_cwd_payload_is_dropped_and_the_next_sequence_survives() {
        // Aşım **sessiz** (ayna kolunun aksine): dizin görünmeyen bir ayrıntı
        // değil, eski değeri ekranda duruyor ve sonraki prompt onu tazeliyor.
        let mut stream = b"\x1b]7;file:///".to_vec();
        stream.extend(std::iter::repeat_n(b'a', CWD_PAYLOAD_LIMIT + 1));
        stream.push(0x07);
        stream.extend_from_slice(b"\x1b]7;file:///tmp\x07");

        assert_eq!(cwd_events(&stream), ["/tmp"]);
    }

    #[test]
    fn the_branch_op_touches_only_the_branch() {
        // Dal aynanın kanalından geliyor ama aynanın **durumu** değil: `b`
        // metni de `Live`/`Idle` ayrımını da olduğu gibi bırakmalı, yoksa
        // prompt başına gelen dal her satırı bir kareliğine söndürürdü.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(&dock_update(2, "% ", "ls", "", &[]), |event| {
            log.apply_scan(event);
        });
        scanner.feed(
            format!("\x1b]8133;b;{}\x07", b64(b"main")).as_bytes(),
            |event| log.apply_scan(event),
        );

        assert_eq!(log.context.branch, "main");
        assert_eq!(log.dock.status, DockStatus::Live);
        assert_eq!(log.dock.buffer, "ls");

        // Depo değilse gövde boş ve dal silinir — bir önceki deponun dalı
        // yeni dizinde asılı kalmamalı.
        scanner.feed(b"\x1b]8133;b;\x07", |event| log.apply_scan(event));
        assert_eq!(log.context.branch, "");
        assert_eq!(log.dock.status, DockStatus::Live);
    }

    /// **Hızlı komut devir doğurmuyor** — setin çekirdek iddiası.
    ///
    /// Ölçülmüş belirti (`context.md` → Kanıt): `ls` koşarken safha 44 ms
    /// sürüyor, imleç animasyonu 230 ms'de yerleşiyor; caret dock'tan çıkıp
    /// yarı yolda geri dönüyor ve göz bunu bir zıplama olarak okuyor.
    ///
    /// Devrin **`line-finish`'te** başladığı da burada çivileniyor: `C`
    /// gelmeden, ayna `Idle`'a düşer düşmez ham cevap `Grid` oluyor.
    /// `running_since`'e bağlanan bir eşik bu ilk geçişi göremezdi.
    #[test]
    fn a_fast_command_never_hands_the_caret_over() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(b"\x1b]133;A\x07\x1b]133;B\x07", |event| {
            log.apply_scan(event)
        });
        scanner.feed(&dock_update(2, "% ", "ls", "", &[]), |event| {
            log.apply_scan(event)
        });

        // **`now` olaylardan sonra alınıyor.** Önce alınsaydı `caret_since`
        // ondan ileride olur, `saturating_duration_since` sıfıra kırpar ve
        // sınama `HANDOVER_HOLD > 0` olan her değerde yeşil kalırdı — 1 ms'lik
        // bir tutma `ls`'in 44 ms'sini hiç yakalamadığı hâlde.
        let at_prompt = log.caret(Instant::now());
        assert_eq!(at_prompt.home, CaretHome::Dock, "promptta caret dock'un");
        assert_eq!(
            at_prompt.hold_left, None,
            "tutma yokken saat kurulmamalı: boşta sıfır kare"
        );

        // Enter → `line-finish`: ayna satırı bıraktı, safha hâlâ `Input`.
        scanner.feed(b"\x1b]8133;e\x07", |event| log.apply_scan(event));
        let now = Instant::now();
        // Ayna tutuluyor (032 Karar 11) ama devrin sorduğu durum `Idle`.
        assert_eq!(
            caret_home(log.state, log.caret_status(), false),
            CaretHome::Grid,
            "ham cevap `line-finish`'te çoktan `Grid`"
        );
        let handing_over = log.caret(now);
        assert_eq!(handing_over.home, CaretHome::Dock, "tutma devri gizlemeli");
        assert!(
            handing_over.hold_left.is_some(),
            "tutmanın kalanı kare istemeli, yoksa devir bir sonraki hasarı beklerdi"
        );

        // Komut koştu ve bitti — hepsi tutmanın içinde.
        scanner.feed(b"\x1b]133;C\x07", |event| log.apply_scan(event));
        assert_eq!(log.caret(now).home, CaretHome::Dock, "koşarken de gizli");
        scanner.feed(b"\x1b]133;D;0\x07\x1b]133;A\x07", |event| {
            log.apply_scan(event)
        });
        let after = log.caret(now);
        assert_eq!(after.home, CaretHome::Dock);
        assert_eq!(
            after.hold_left, None,
            "ham cevap `Dock`'a döndü: saat sönmeli (adlandırılmış durma koşulu)"
        );
    }

    /// **Yavaş komutun devri oluyor**, gecikmesi tutma süresi kadar; ve
    /// **ters yön hiç tutulmuyor**.
    ///
    /// İkisi tek sınamada, çünkü ikincisi birincisinin kabulü: tutma her iki
    /// yöne uygulansaydı komut bitince caret ızgarada asılı kalır ve kullanıcı
    /// yazmaya başladığında dock'ta caret'siz bir satır görürdü.
    #[test]
    fn a_slow_command_hands_over_after_the_hold_but_comes_back_at_once() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(b"\x1b]133;A\x07\x1b]133;B\x07", |event| {
            log.apply_scan(event)
        });
        scanner.feed(&dock_update(2, "% ", "sleep 2", "", &[]), |event| {
            log.apply_scan(event)
        });
        scanner.feed(b"\x1b]8133;e\x07\x1b]133;C\x07", |event| {
            log.apply_scan(event)
        });

        let now = Instant::now();
        assert_eq!(log.caret(now).home, CaretHome::Dock, "tutma sürüyor");
        // Tutmanın **tam** dolduğu an: kalan sıfır, yani devir görünür oluyor.
        let expired = now + HANDOVER_HOLD;
        let handed = log.caret(expired);
        assert_eq!(handed.home, CaretHome::Grid, "yavaş komutta devir olmalı");
        assert_eq!(handed.hold_left, None, "dolmuş tutma kare istemez");

        // Komut bitti: ters yön **anında**, tutma yok.
        scanner.feed(b"\x1b]133;D;0\x07", |event| log.apply_scan(event));
        let back = log.caret(expired);
        assert_eq!(back.home, CaretHome::Dock, "Grid→Dock geciktirilmemeli");
        assert_eq!(back.hold_left, None);
    }

    /// **Uzak oturum tutmadan önce** (036 Karar 8): `C`'den hemen sonra,
    /// tutma sürerken gelen `set_remote` caret'i ızgaraya alıyor ve saat
    /// kurmuyor — giriş satırı olmayan bir bantta caret bağlam satırına
    /// otururdu. `D` uzak durumu silince yüklem bugünkü cevabına dönüyor.
    #[test]
    fn a_remote_session_takes_the_caret_before_the_hold() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(b"\x1b]133;A\x07\x1b]133;B\x07", |event| {
            log.apply_scan(event)
        });
        scanner.feed(&dock_update(8, "% ", "ssh prod", "", &[]), |event| {
            log.apply_scan(event)
        });
        scanner.feed(b"\x1b]8133;e\x07\x1b]133;C\x07", |event| {
            log.apply_scan(event)
        });
        let now = Instant::now();
        assert_eq!(log.caret(now).home, CaretHome::Dock, "tutma sürüyor");

        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        let remote = log.caret(now);
        assert_eq!(remote.home, CaretHome::Grid, "uzakta caret ızgarada");
        assert_eq!(remote.hold_left, None, "çevrilmeyen cevap kare istemez");

        scanner.feed(b"\x1b]133;D;0\x07", |event| log.apply_scan(event));
        assert_eq!(log.context.remote, None);
        assert_eq!(log.caret(now + HANDOVER_HOLD).home, CaretHome::Dock);
    }

    /// Damga **değişimde** kıpırdıyor, her olayda değil.
    ///
    /// Her tuş vuruşu bir ayna olayı doğuruyor; damga onlarla tazelenseydi
    /// tutma hiç dolmaz ve yavaş komutta devir **hiç** gerçekleşmezdi.
    #[test]
    fn the_stamp_moves_on_change_not_on_every_event() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(b"\x1b]133;A\x07\x1b]133;B\x07", |event| {
            log.apply_scan(event)
        });
        scanner.feed(&dock_update(0, "% ", "", "", &[]), |event| {
            log.apply_scan(event)
        });
        scanner.feed(b"\x1b]8133;e\x07", |event| log.apply_scan(event));
        let stamped = log.caret_since;

        // Devirden sonra gelen olaylar ham cevabı değiştirmiyor (`Running` de
        // `Grid`), yani damga yerinde kalmalı.
        scanner.feed(b"\x1b]133;C\x07", |event| log.apply_scan(event));
        assert_eq!(
            log.caret_since, stamped,
            "değişmeyen cevap damgayı taşımamalı"
        );
    }

    /// **Satır sonu taşıyan görüntü `Live`** (032): dock satırı kırıyor, yani
    /// onu gösterebiliyor. 032'ye kadar bu ayna `Multiline`'dı ve satır da
    /// caret'i de ızgarada kalıyordu.
    #[test]
    fn a_newline_anywhere_in_the_display_keeps_the_mirror_live() {
        for (pre, buffer, post) in [
            ("", "echo a\necho b\n", ""),
            ("", "for x in 1 2 3; do\n  echo $x", ""),
            ("% \n", "ls", ""),
            ("", "ls", " a\nb"),
        ] {
            let line = dock_line(&dock_update(0, pre, buffer, post, &[]));
            assert_eq!(
                line.status,
                DockStatus::Live,
                "satır sonu taşıyan görüntü: {pre:?} {buffer:?} {post:?}"
            );
            assert_eq!(line.buffer, buffer);
        }
    }

    /// **Son mürekkep görüntünün son satırından** (032): kapının öteki
    /// yarısı ızgaranın son giriş satırını tarıyor. `echo a\necho b\n`
    /// yapıştırmasında zsh son satır sonunu tamponda tutuyor ve imleç boş bir
    /// satırda — ayna da `None` demeli. `\n`'in kendisi mürekkep değil (eski
    /// süzgeç genişliği `None` olduğu için onu geçiriyordu).
    #[test]
    fn the_last_ink_comes_from_the_last_row_of_the_display() {
        let ink = |buffer: &str| dock_line(&dock_update(0, "", buffer, "", &[])).last_ink;
        assert_eq!(ink("echo a\necho b\n"), None);
        assert_eq!(ink("echo a\necho b"), Some('b'));
        assert_eq!(ink("echo a\n  "), None);
        assert_eq!(ink("\n"), None);
        // Öneri de son satırda sayılıyor, satır sonu taşımıyorsa.
        assert_eq!(
            dock_line(&dock_update(0, "", "ls\ngi", "t", &[])).last_ink,
            Some('t')
        );
    }

    /// **Kümeli okunuşta son mürekkep son kümenin başı** (035): ızgaranın
    /// hücresi `👍🏽`'yi `c = 👍` ile tutuyor; ayna `🏽` deseydi kapı kalıcı
    /// olarak "bayat" derdi (024'ün bekçisinin kümeli kardeşi). Kapalı
    /// okunuş bugünkü gibi: ten rengi kendi başına bir mürekkep.
    #[test]
    fn the_clustered_last_ink_is_the_head_of_the_last_cluster() {
        let ink = |cluster: bool, buffer: &str| {
            let mut scanner = Scanner::new().cluster(cluster);
            let mut seen = None;
            scanner.feed(&dock_update(0, "", buffer, "", &[]), |event| {
                if let ScanEvent::Dock(DockEvent::Update(line)) = event {
                    assert_eq!(line.cluster, cluster, "okunuş aynaya taşınmadı");
                    seen = Some(line.last_ink);
                }
            });
            seen.expect("güncelleme bekleniyordu")
        };
        assert_eq!(ink(true, "ls 👍🏽"), Some('👍'));
        assert_eq!(ink(true, "🇹🇷"), Some('🇹'));
        assert_eq!(ink(true, "a 👨\u{200D}👩\u{200D}👧"), Some('👨'));
        assert_eq!(ink(true, "❤\u{FE0F} "), Some('❤'));
        assert_eq!(ink(true, "a\n🇹🇷\n"), None, "son satır boş");
        assert_eq!(ink(true, "ls\t"), Some('s'), "sekme mürekkep değil");
        // Kapalı okunuş: kod noktası kod noktası, sıfır genişlik atlanıyor.
        assert_eq!(ink(false, "ls 👍🏽"), Some('🏽'));
        assert_eq!(ink(false, "🇹🇷"), Some('🇷'));
        assert_eq!(ink(false, "❤\u{FE0F}"), Some('❤'));
    }

    /// **Dock'un çizmediği kontrol karakteri satırı `Control`'e indiriyor**
    /// (025) — konumdan ve gövdeden bağımsız; sekme hariç.
    #[test]
    fn a_control_char_anywhere_in_the_display_marks_the_mirror_control() {
        for (pre, buffer, post) in [
            ("", "\x01foo", ""),
            ("", "foo\x01", ""),
            ("", "echo \x1b[0m", ""),
            ("% \x7f", "ls", ""),
            ("", "ls", " \x02"),
        ] {
            let line = dock_line(&dock_update(0, pre, buffer, post, &[]));
            assert_eq!(
                line.status,
                DockStatus::Control,
                "kontrol karakteri taşıyan görüntü `Live` kaldı: {pre:?} {buffer:?} {post:?}"
            );
            assert_eq!(line.buffer, buffer, "alanlar duruyor");
        }
        // **Sekme istisna**: bilgi taşımıyor ve Ctrl-V Tab satırı dock'ta
        // kalmalı. Emoji, boş satır ve düz metin de `Live`.
        for buffer in ["ls\t", "\t", "🥰", "", "echo a"] {
            let line = dock_line(&dock_update(0, "% ", buffer, "", &[]));
            assert_eq!(line.status, DockStatus::Live, "{buffer:?}");
        }
        // Satır sonu kontrol karakteri sayılmıyor (032), öteki kontrol
        // karakteri satır sonlu görüntüde de `Control`.
        let both = dock_line(&dock_update(0, "", "a\x01\nb", "", &[]));
        assert_eq!(both.status, DockStatus::Control);
        // `PREBUFFER`'daki kontrol karakteri de: dock onu da çiziyor.
        let prebuffer = format!(
            "\x1b]8133;u;0;;{};;;{};{}\x07",
            b64(b"b"),
            b64(b"main"),
            b64(b"a\x01\n")
        );
        assert_eq!(dock_line(prebuffer.as_bytes()).status, DockStatus::Control);
        // Dönüş kendiliğinden: kontrol karakteri silinince bir sonraki ayna `Live`.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(&dock_update(4, "", "\x01foo", "", &[]), |event| {
            log.apply_scan(event)
        });
        assert_eq!(log.dock.status, DockStatus::Control);
        scanner.feed(&dock_update(3, "", "foo", "", &[]), |event| {
            log.apply_scan(event)
        });
        assert_eq!(log.dock.status, DockStatus::Live);
    }

    /// **`Control` de tutulmuyor** — `Unavailable`'ın ikizi ve aynı gerekçe:
    /// gösteremediğimiz satırın caret'i ızgarada, 150 ms bile olsa dock'ta
    /// durmamalı.
    #[test]
    fn a_control_mirror_is_never_held() {
        let typing = Some(ShellState {
            phase: ShellPhase::Input,
            last_exit: None,
        });
        for held in [false, true] {
            assert_eq!(
                caret_home(typing, DockStatus::Control, held),
                CaretHome::Grid,
                "held={held}"
            );
        }
    }

    /// **Ayna damgasını içerikle aynı turda alıyor** (025): `answers` ayna
    /// olayında yazılıyor, başka olaylarda kıpırdamıyor ve `End` onu o anki
    /// nesille yeniden damgalıyor (030).
    #[test]
    fn the_mirror_carries_the_generation_it_answers() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(&dock_update(2, "", "ls", "", &[]), |event| {
            log.apply_scan_answering(event, 7);
        });
        assert_eq!(log.dock.answers, 7);
        // İşaret ya da dal olayı damgaya dokunmuyor.
        scanner.feed(b"\x1b]133;B\x07", |event| {
            log.apply_scan_answering(event, 9);
        });
        assert_eq!(log.dock.answers, 7);
        scanner.feed(b"\x1b]8133;e\x07", |event| {
            log.apply_scan_answering(event, 9);
        });
        // `Input` safhasında `e` tutuluyor (032 Karar 11) ama damga hemen
        // güncel: tutulan satır ⏎'in cevabı.
        assert_eq!(log.dock.answers, 9, "tutulan ayna güncel damgayı taşımalı");
        // Kapanmış ayna **eski** damgayı taşımıyor, güncelini taşıyor: `Idle`
        // taban da dock'un yazım animasyonlarının girdi sınırına giriyor.
        let _ = log.expire_end(Instant::now() + HANDOVER_HOLD);
        assert_eq!(log.dock.status, DockStatus::Idle);
        assert_eq!(log.dock.answers, 9, "kapanmış ayna güncel damgayı taşımalı");
    }

    /// **`PS2` satırları arasındaki `line-finish` tutuluyor** (032 Karar 11).
    ///
    /// zsh her `PS2` kabulünde `e` basıyor ve hemen ardından yeni satırın
    /// aynası (`u`, `PREBUFFER` dolu) geliyor; arada safha `Input`. Tutma
    /// olmasaydı her ⏎ bandı bir kare küçültür, kabul edilen satır bir an
    /// ızgarada belirirdi. Yerini `Multiline`'ın tutulmama bekçisi aldı: o
    /// kol kalktı, tutmanın kuralı geldi.
    #[test]
    fn a_line_finish_while_typing_is_held_until_the_next_mirror() {
        let typing = |log: &mut ShellLog, scanner: &mut Scanner| {
            scanner.feed(b"\x1b]133;A;bt_block=1\x07\x1b]133;B\x07", |event| {
                log.apply_scan(event)
            });
            scanner.feed(&dock_update(16, "", "for i in 1 2; do", "", &[]), |event| {
                log.apply_scan_answering(event, 3);
            });
        };
        let end = |log: &mut ShellLog, scanner: &mut Scanner| {
            scanner.feed(b"\x1b]8133;e\x07", |event| {
                log.apply_scan_answering(event, 4);
            });
        };

        // `e` → görüntü, bant ve bastırma yerinde; taban çıpanın satırı.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        typing(&mut log, &mut scanner);
        end(&mut log, &mut scanner);
        let now = Instant::now();
        assert_eq!(log.dock.status, DockStatus::Live);
        assert_eq!(log.dock.buffer, "for i in 1 2; do");
        let input = log.suppressed_input().expect("tutulan satır bastırılmalı");
        assert!(input.from_anchor, "tutulan satırın tabanı çıpa");
        assert_eq!(input.answers, 4, "`e` ⏎'in cevabı");
        // Caret dock'ta ve saat kurulu: tutma dolunca bir kare gerekiyor.
        let caret = log.caret(now);
        assert_eq!(caret.home, CaretHome::Dock);
        assert!(caret.hold_left.is_some());
        assert!(log.expire_end(now).is_some());

        // `u` gelirse yeni ayna geçiyor ve tutma bitiyor.
        let next = format!(
            "\x1b]8133;u;0;;;;;{};{}\x07",
            b64(b"main"),
            b64(b"for i in 1 2; do\n")
        );
        scanner.feed(next.as_bytes(), |event| {
            log.apply_scan_answering(event, 4);
        });
        assert_eq!(log.expire_end(now), None);
        assert_eq!(log.dock.prebuffer, "for i in 1 2; do\n");
        assert!(
            log.suppressed_input()
                .is_some_and(|input| input.from_anchor)
        );
        assert_eq!(log.caret(now).home, CaretHome::Dock);

        // `C` (komut koştu) tutmayı anında bitiriyor.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        typing(&mut log, &mut scanner);
        end(&mut log, &mut scanner);
        scanner.feed(b"\x1b]133;C\x07", |event| log.apply_scan(event));
        assert_eq!(log.dock.status, DockStatus::Idle);
        assert_eq!(log.expire_end(Instant::now()), None);

        // Süre dolunca bugünkü sıfırlama; damga yerinde.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        typing(&mut log, &mut scanner);
        end(&mut log, &mut scanner);
        let later = Instant::now() + HANDOVER_HOLD;
        assert_eq!(log.expire_end(later), None);
        assert_eq!(log.dock.status, DockStatus::Idle);
        assert!(log.dock.buffer.is_empty());
        assert_eq!(log.dock.answers, 4);
        assert_eq!(log.caret(later).home, CaretHome::Grid);

        // Safha `Input` değilse tutma yok (`e` komut koşarken gelmez ama gelse
        // de bugünkü gibi): anında `Idle`.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        typing(&mut log, &mut scanner);
        scanner.feed(b"\x1b]133;C\x07", |event| log.apply_scan(event));
        end(&mut log, &mut scanner);
        assert_eq!(log.dock.status, DockStatus::Idle);
    }

    /// **Tekerleğin penceresi caret'in yerine bağlı** (032 phase-4): öneri
    /// değişimi onu bırakıyor, caret'in ya da metnin değişimi kaldırıyor —
    /// yazan ya da ok tuşuna basan kullanıcı caret'ini görmeli.
    #[test]
    fn the_dock_scroll_ends_when_the_caret_moves() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        let mut feed = |log: &mut ShellLog, bytes: &[u8]| {
            scanner.feed(bytes, |event| log.apply_scan(event));
        };
        feed(&mut log, &dock_update(2, "", "ls", "", &[]));
        log.dock_scroll = Some(0);
        feed(&mut log, &dock_update(2, "", "ls", " -la", &[]));
        assert_eq!(log.dock_scroll, Some(0), "öneri pencereyi bırakıyor");
        feed(&mut log, &dock_update(1, "", "ls", " -la", &[]));
        assert_eq!(log.dock_scroll, None, "caret oynadı");
        log.dock_scroll = Some(0);
        feed(&mut log, &dock_update(1, "", "lxs", "", &[]));
        assert_eq!(log.dock_scroll, None, "metin değişti");
    }

    /// **Boş ayna karakterle ölçülüyor** (032): tek başına bir `\n` imleci
    /// aşağı itiyor, yani boş değil; `PREBUFFER` doluysa da boş değil
    /// (`for>` satırında imleç meşru olarak çıpanın aşağısında) ve taban
    /// çıpa.
    #[test]
    fn a_blank_mirror_has_no_character_at_all() {
        let input = |update: &[u8]| {
            let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
            let mut scanner = Scanner::new();
            scanner.feed(b"\x1b]133;A;bt_block=1\x07\x1b]133;B\x07", |event| {
                log.apply_scan(event)
            });
            scanner.feed(update, |event| log.apply_scan(event));
            log.suppressed_input().expect("bastırılan satır")
        };
        let empty = input(&dock_update(0, "", "", "", &[]));
        assert!(empty.blank && !empty.from_anchor);
        // 025'in yapıştırma şekli: caret sondaki satır sonunun arkasında.
        let pasted = input(&dock_update(14, "", "echo a\necho b\n", "", &[]));
        assert!(!pasted.blank);
        assert_eq!(pasted.last_ink, None);
        assert!(!input(&dock_update(1, "", "\n", "", &[])).blank);
        let ps2 = format!(
            "\x1b]8133;u;0;;;;;{};{}\x07",
            b64(b"main"),
            b64(b"for i in 1 2; do\n")
        );
        let ps2 = input(ps2.as_bytes());
        assert!(!ps2.blank && ps2.from_anchor);
    }

    /// **Aynanın arızası tutulmuyor** — carve-out'un deterministik bekçisi.
    ///
    /// Entegrasyon sınaması (`the_grid_keeps_the_input_line_when_the_mirror_
    /// cannot_show_it`) bunu ancak `frame()` 150 ms içinde koşarsa görüyor,
    /// yani yüklü bir makinede carve-out silinse de yeşil kalabilirdi —
    /// **açığa düşen** bir bekçi. Buradaki sorgu saatten bağımsız.
    #[test]
    fn a_faulty_mirror_is_never_held() {
        let typing = Some(ShellState {
            phase: ShellPhase::Input,
            last_exit: None,
        });
        for fault in [DockFault::Overflow, DockFault::Malformed] {
            assert_eq!(
                caret_home(typing, DockStatus::Unavailable(fault), true),
                CaretHome::Grid,
                "gösteremediğimiz satırın caret'i tutulamaz: {fault:?}"
            );
        }
        // Karşı uç, aynı `held` ile: tutmanın gerçekten uygulandığı kol.
        // İkisi bir arada olmasa sınama "tutma hiç çalışmıyor" hâlinde de
        // yeşil kalırdı.
        assert_eq!(
            caret_home(typing, DockStatus::Idle, true),
            CaretHome::Dock,
            "`Input`+`Idle` tutulabilen kol"
        );
    }

    /// İki son tarih **birleşiyor**, biri ötekini ezmiyor.
    ///
    /// Bugün `resolve_blocks` `next_tick`'i doğrudan yazıyor ve o yol koşan
    /// bloğun çıpasının görünür olmasına bağlı; devir ona bağlanamaz. Ezme
    /// iki yönde de sessiz: ya sayaç donar ya devir hiç gerçekleşmez.
    #[test]
    fn two_deadlines_merge_into_the_sooner_one() {
        let tick = Duration::from_millis(600);
        let hold = Duration::from_millis(150);
        assert_eq!(sooner(Some(tick), Some(hold)), Some(hold));
        assert_eq!(sooner(Some(hold), Some(tick)), Some(hold), "sıra önemsiz");
        // Tek taraflı hâller: olan kazanır, olmayan kaybettirmez.
        assert_eq!(sooner(Some(tick), None), Some(tick));
        assert_eq!(sooner(None, Some(hold)), Some(hold));
        assert_eq!(sooner(None, None), None, "iki taraf da boşsa saat kurulmaz");
    }

    #[test]
    fn the_keymap_field_opens_the_gate_only_for_insert_keymaps() {
        // İzin listesi: tanıdığımız üç ad geçiyor, geri kalan **her şey**
        // (komut keymap'i, kullanıcının `bindkey -N` ile yarattığı ad, hiç
        // gelmemiş alan) kapalı. Yön güvenli — bilmemek istisnayı kapatıyor
        // (`Session::can_be_typed`).
        let with = |keymap: &str| {
            let sequence = format!(
                "\x1b]8133;u;0;;{};;;{}\x07",
                b64(b"ls"),
                b64(keymap.as_bytes())
            );
            dock_line(sequence.as_bytes()).insert_keymap
        };
        for keymap in ["main", "emacs", "viins"] {
            assert!(with(keymap), "{keymap} ekleme keymap'i sayılmadı");
        }
        for keymap in ["vicmd", "visual", "viopp", "isearch", "command", "mine", ""] {
            assert!(!with(keymap), "{keymap} ekleme keymap'i sayıldı");
        }
        // Alan hiç yoksa (eski betik) kapı kapalı, ama yük **bozuk değil**:
        // satır yine çiziliyor.
        let line = dock_line(&dock_update(0, "", "ls", "", &[]));
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.buffer, "ls");
        assert!(!line.insert_keymap);
    }

    #[test]
    fn a_six_body_mirror_from_an_old_script_still_decodes() {
        // **Eski betikle koşan pencere** (032 phase-1): yedinci gövde
        // (`PREBUFFER`) hiç yok. Yokluğu yükü bozmuyor, `PREBUFFER` boş
        // sayılıyor — `KEYMAP`'in emsali.
        let sequence = format!("\x1b]8133;u;2;;{};;;{}\x07", b64(b"ls"), b64(b"main"));
        let line = dock_line(sequence.as_bytes());
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.buffer, "ls");
        assert!(line.insert_keymap);
        assert_eq!(line.prebuffer, "");
    }

    #[test]
    fn the_seventh_body_carries_the_prebuffer_and_stays_out_of_the_line() {
        // `for i in 1 2` + Enter: ZLE önceki satırı `PREBUFFER`'a alıyor ve
        // `BUFFER` yeni satırla başlıyor. `PREBUFFER` **her zaman** `\n`'le
        // bitiyor ve aynayı `Live`'dan düşürmüyor. Görüntü uzayına girmiyor:
        // caret, uzunluk ve son mürekkep yalnız `PREDISPLAY ++ BUFFER ++
        // POSTDISPLAY`'den.
        let sequence = format!(
            "\x1b]8133;u;2;;{};;;{};{}\x07",
            b64(b"do"),
            b64(b"main"),
            b64(b"for i in 1 2\n")
        );
        let line = dock_line(sequence.as_bytes());
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.prebuffer, "for i in 1 2\n");
        assert_eq!(line.buffer, "do");
        assert_eq!(line.cursor, 2);
        assert_eq!(line.display_chars, 2);
        assert_eq!(line.last_ink, Some('o'));

        // Bozuk yedinci gövde öteki metin gövdeleriyle aynı kuralda: yük
        // bozuk, satır ızgarada.
        let broken = format!("\x1b]8133;u;0;;{};;;{};!!!!\x07", b64(b"ls"), b64(b"main"));
        assert_eq!(
            dock_events(broken.as_bytes()),
            vec![DockSnapshot::Unavailable(DockFault::Malformed)]
        );
    }

    #[test]
    fn a_broken_branch_never_drops_the_mirror() {
        // Dalın iki bozulma biçimi de yalnız dalı düşürmeli: `Unavailable`
        // "giriş satırını gösteremiyorum" demek ve ızgarayı devreye sokardı —
        // yani kesilmiş bir dal dizisi yüzünden kullanıcı yazdığını dock'ta
        // değil ızgarada görürdü (`/code-review`, 012 phase-6).
        for sequence in [
            &b"\x1b]8133;b\x07"[..], // alan hiç yok
            b"\x1b]8133;b;!!!!\x07", // base64 alfabesi dışı
            b"\x1b]8133;b;gA\x07",   // geçerli base64, geçersiz UTF-8
        ] {
            let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
            let mut scanner = Scanner::new();
            scanner.feed(&dock_update(2, "% ", "ls", "", &[]), |event| {
                log.apply_scan(event);
            });
            scanner.feed(sequence, |event| log.apply_scan(event));

            assert_eq!(
                log.dock.status,
                DockStatus::Live,
                "dizi aynayı düşürdü: {}",
                String::from_utf8_lossy(sequence)
            );
            assert_eq!(log.dock.buffer, "ls");
            assert_eq!(log.context.branch, "");
        }
    }

    #[test]
    fn the_cwd_survives_a_finished_line() {
        // Bağlam satırı aynanın ömrüne bağlı değil: `line-finish` metni
        // siliyor ama dizin ile dal bir sonraki prompt'a kadar duruyor.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        let mut stream = b"\x1b]7;file:///tmp\x07".to_vec();
        stream.extend_from_slice(format!("\x1b]8133;b;{}\x07", b64(b"main")).as_bytes());
        stream.extend_from_slice(&dock_update(0, "", "ls", "", &[]));
        stream.extend_from_slice(b"\x1b]8133;e\x07");
        scanner.feed(&stream, |event| log.apply_scan(event));

        assert_eq!(log.dock.status, DockStatus::Idle);
        assert_eq!(log.dock.buffer, "");
        assert_eq!(log.context.cwd, "/tmp");
        assert_eq!(log.context.branch, "main");
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
        script_output_after(cursor, "", pre, buffer, post, highlights)
    }

    /// [`script_output`], `PREBUFFER` doluyken (032).
    fn script_output_after(
        cursor: usize,
        prebuffer: &str,
        pre: &str,
        buffer: &str,
        post: &str,
        highlights: &[&str],
    ) -> Vec<u8> {
        run_script(
            "source $ZDOTDIR/bateri.zsh
             PREDISPLAY=$T_PRE BUFFER=$T_BUF POSTDISPLAY=$T_POST CURSOR=$T_CURSOR
             region_highlight=( ${(f)T_HL} ) KEYMAP=$T_KEYMAP PREBUFFER=$T_PREBUF
             __bateri_dock_redraw",
            &[
                ("T_CURSOR", &cursor.to_string()),
                ("T_PRE", pre),
                ("T_BUF", buffer),
                ("T_POST", post),
                ("T_HL", &highlights.join("\n")),
                // `$KEYMAP` ZLE'nin parametresi ve kanca dışında boş; sınama
                // onu elle kuruyor ki telin altıncı gövdesi de koşsun.
                ("T_KEYMAP", "main"),
                // `$PREBUFFER` da öyle; yedinci gövde.
                ("T_PREBUF", prebuffer),
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

        // Tel sağlam ama satır dock'un değil: `ESC` bir kontrol karakteri,
        // dock onu çizmiyor ve zsh ızgarada `^[` basıyor
        // ([`DockStatus::Control`], 025). Metin yine de eksiksiz çözülüyor —
        // aşağıdaki iddialar telin kendisini sınıyor.
        assert_eq!(line.status, DockStatus::Control);
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
        // Altıncı gövde de telden geçiyor: kabuk `$KEYMAP`'i basıyor ve
        // çözücü onu ekleme kapısına çeviriyor.
        assert!(line.insert_keymap, "keymap gövdesi telde kayboldu");
    }

    #[test]
    fn the_script_sends_the_prebuffer_as_the_seventh_body() {
        // `for` döngüsünün ikinci satırı: kabuk `PREBUFFER`'ı basıyor,
        // çözücü onu ayrı tutuyor ve satır tek satırlık `BUFFER`'la `Live`.
        let line = dock_line(&script_output_after(2, "for i in 1 2\n", "", "do", "", &[]));
        assert_eq!(line.status, DockStatus::Live);
        assert_eq!(line.prebuffer, "for i in 1 2\n");
        assert_eq!(line.buffer, "do");
        assert_eq!(line.cursor, 2);
        // Boş `PREBUFFER` (olağan tek satırlık komut) da bir alan: boş gövde.
        let line = dock_line(&script_output(0, "", "ls", "", &[]));
        assert_eq!(line.prebuffer, "");
        assert_eq!(line.status, DockStatus::Live);
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

        // **`PREBUFFER` de toplama giriyor** (032): yapıştırılmış bir
        // döngünün önceki satırları görüntünün parçası ve sınırı `BUFFER`
        // ile birlikte aşabilir.
        let before = format!("{}\n", "x".repeat(4095));
        assert_eq!(
            dock_events(&script_output_after(0, &before, "", "ls", "", &[])),
            vec![DockSnapshot::Unavailable(DockFault::Overflow)]
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
    fn the_script_prints_a_cwd_the_scanner_decodes() {
        // Kodlayan gerçekten zsh, çözen burası: yüzde kodlaması iki uçta ayrı
        // dillerde yazılı ve taşıdığı baytlar tam da URI'yi bozabilecek olanlar
        // (boşluk, `%`, `;`, çok baytlı karakter).
        //
        // Dizinler **gerçekten yaratılıyor**: `PWD`'yi elle atamak kabuğun
        // kendi değerini sınamak olmaktan çıkarırdı — zsh onu başlangıçta
        // kendisi kuruyor.
        let root = std::env::temp_dir().join(format!("bateri-cwd-{}", std::process::id()));
        let names = ["plain", "a b", "a%b", "a;b", "çığır", "😀"];
        for name in names {
            std::fs::create_dir_all(root.join(name)).expect("dizin yaratılamadı");
        }

        for name in names {
            let path = root.join(name);
            let path = path.to_string_lossy().into_owned();
            let bytes = run_script(
                "source $ZDOTDIR/bateri.zsh; cd -q -- $T_DIR; __bateri_cwd",
                &[("T_DIR", &path)],
            );
            assert_eq!(cwd_events(&bytes), [path.clone()], "yol: {path}");
        }

        // Kök dizin: yolun tek karakter olduğu kenar.
        let bytes = run_script("source $ZDOTDIR/bateri.zsh; cd -q -- /; __bateri_cwd", &[]);
        assert_eq!(cwd_events(&bytes), ["/"]);

        std::fs::remove_dir_all(&root).expect("geçici dizin silinemedi");
    }

    #[test]
    fn the_script_prints_the_branch_from_the_repository() {
        // Depo yokken dal boş; ayraç da onunla birlikte düşüyor
        // (`dock::render`). Geçici dizin **depo değil**, yani bu kol deponun
        // varlığına değil yokluğuna tanık.
        let outside = std::env::temp_dir();
        let bytes = run_script(
            "source $ZDOTDIR/bateri.zsh; cd -q -- $T_DIR; __bateri_branch_print",
            &[("T_DIR", &outside.to_string_lossy())],
        );
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        scanner.feed(&bytes, |event| log.apply_scan(event));
        assert_eq!(log.context.branch, "");

        // Deponun içinde dal adı geliyor. Kendi depomuz: `git` yoksa sınama
        // atlanmıyor, dal boş kalır ve iddia da onu söyler.
        let inside = script_path();
        let bytes = run_script(
            "source $ZDOTDIR/bateri.zsh; cd -q -- $T_DIR; __bateri_branch_print",
            &[("T_DIR", &inside.to_string_lossy())],
        );
        scanner.feed(&bytes, |event| log.apply_scan(event));
        assert!(
            !log.context.branch.is_empty(),
            "depo içinde dal boş kaldı: {:?}",
            log.context.branch
        );
    }

    #[test]
    fn the_three_arms_do_not_touch_each_others_buffers() {
        // Aynanın geniş sınırı 133'ün dar sınırını gevşetmemeli; 133'ün dar
        // sınırı da aynayı kesmemeli. Dizin kolunun sınırı da üçüncü bir
        // bütçe. Tamponların ayrı olmasının kanıtı.
        let mut scanner = Scanner::new();
        let mut marks = Vec::new();
        let mut lines = Vec::new();
        let mut paths = Vec::new();
        let mut stream = dock_update(2, "", "ls", "", &[]);
        stream.extend_from_slice(b"\x1b]133;B\x07");
        stream.extend_from_slice(b"\x1b]7;file:///tmp\x07");
        stream.extend_from_slice(&dock_update(3, "", "lsx", "", &[]));
        scanner.feed(&stream, |event| match event {
            ScanEvent::Mark(mark) => marks.push(mark),
            ScanEvent::Dock(DockEvent::Update(line)) => lines.push(line.buffer.clone()),
            ScanEvent::Dock(_) => {}
            ScanEvent::Cwd { path, .. } => paths.push(path.to_owned()),
        });

        assert_eq!(marks, vec![Mark::PromptEnd]);
        assert_eq!(lines, vec!["ls".to_string(), "lsx".to_string()]);
        assert_eq!(paths, vec!["/tmp".to_string()]);
        assert_eq!(scanner.payload.capacity(), PAYLOAD_LIMIT);
        assert_eq!(scanner.dock.capacity(), DOCK_PAYLOAD_LIMIT);
        assert_eq!(scanner.cwd.capacity(), CWD_PAYLOAD_LIMIT);
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
    fn a_new_buffer_clears_the_dock_selection_and_a_new_prompt_does_not() {
        // 031 R3.4: seçimin indeksleri `BUFFER`'ın karakterleri; `BUFFER`
        // değişince başka bir metni gösterirlerdi. Prompt'un yeniden
        // çizilmesi ya da önerinin değişmesi seçili metni oynatmıyor.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut staged = DockState {
            status: DockStatus::Live,
            predisplay: "% ".to_string(),
            buffer: "git status".to_string(),
            cursor: 12,
            ..DockState::default()
        };
        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        let word = DockPoint {
            index: 0,
            half: CellHalf::Left,
        };
        let select = |log: &mut ShellLog| {
            log.dock_selection = Some(DockSelection::new(
                SelectKind::Word,
                word,
                word,
                &log.dock.buffer,
                false,
            ));
        };
        select(&mut log);
        assert_eq!(log.dock_selection.and_then(|s| s.range()), Some((0, 3)));

        staged.predisplay = "%% ".to_string();
        staged.postdisplay = " -s".to_string();
        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        assert!(log.dock_selection.is_some(), "prompt değişimi seçimi sildi");

        staged.buffer = "git statu".to_string();
        log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
        assert_eq!(log.dock_selection, None, "yeni BUFFER seçimi silmedi");

        for (name, event) in [
            ("End", DockEvent::End),
            ("Unavailable", DockEvent::Unavailable(DockFault::Overflow)),
        ] {
            log.apply_scan(ScanEvent::Dock(DockEvent::Update(&staged)));
            select(&mut log);
            log.apply_scan(ScanEvent::Dock(event));
            assert_eq!(log.dock_selection, None, "{name} seçimi silmedi");
        }
    }

    #[test]
    fn shift_arrows_step_the_moving_end_of_the_selection() {
        let range = |selection: DockSelection| selection.range;
        // Seçim yoksa caret'ten başlıyor.
        let one = DockSelection::stepped(None, 2, true, "abcd", false);
        assert_eq!(range(one), (2, 3));
        let two = DockSelection::stepped(Some(one), 2, true, "abcd", false);
        assert_eq!(range(two), (2, 4));
        // Satırın sonunda duruyor.
        assert_eq!(
            range(DockSelection::stepped(Some(two), 2, true, "abcd", false)),
            (2, 4)
        );
        let back = DockSelection::stepped(Some(two), 2, false, "abcd", false);
        assert_eq!(range(back), (2, 3));
        // Boşa inen seçim ucunu kaybetmiyor: bir sonraki adım oradan.
        let empty = DockSelection::stepped(Some(back), 2, false, "abcd", false);
        assert_eq!((empty.range(), range(empty)), (None, (2, 2)));
        assert_eq!(
            range(DockSelection::stepped(Some(empty), 0, false, "abcd", false)),
            (1, 2)
        );

        // Baş çapanın solundaysa hareketli uç aralığın başı (sola sürüklenmiş
        // fare seçimi).
        let point = |index| DockPoint {
            index,
            half: CellHalf::Left,
        };
        let leftward = DockSelection::new(SelectKind::Simple, point(3), point(1), "abcd", false);
        assert_eq!(
            range(DockSelection::stepped(
                Some(leftward),
                0,
                false,
                "abcd",
                false
            )),
            (0, 3)
        );
        assert_eq!(
            range(DockSelection::stepped(
                Some(leftward),
                0,
                true,
                "abcd",
                false
            )),
            (2, 3)
        );

        // Kelime seçimi harf adımıyla büyüyor, sonu hareketli.
        let word = DockSelection::new(SelectKind::Word, point(1), point(1), "ab cd", false);
        assert_eq!(range(word), (0, 2));
        assert_eq!(
            range(DockSelection::stepped(Some(word), 0, true, "ab cd", false)),
            (0, 3)
        );

        // Birleştirici tabanından ayrılmıyor: `é` = `e` + U+0301.
        let text = "e\u{301}x";
        assert_eq!(
            range(DockSelection::stepped(None, 0, true, text, false)),
            (0, 2)
        );
        assert_eq!(
            range(DockSelection::stepped(None, 3, false, text, false)),
            (2, 3)
        );
        assert_eq!(
            range(DockSelection::stepped(None, 2, false, text, false)),
            (0, 2)
        );
    }

    #[test]
    fn shift_arrows_step_over_a_cluster_whole() {
        // 035 R4.2: `a🇹🇷b`'de ⇧← sondan bayrağı bütün alıyor, ⇧→ baştan
        // `a`'dan sonra bayrağı. Kapalı okunuşta adım kod noktası.
        let range = |selection: DockSelection| selection.range;
        let text = "a🇹🇷b";
        let back = DockSelection::stepped(None, 3, false, text, true);
        assert_eq!(range(back), (1, 3));
        // Caret iki RI'nin arasında: iki yön de bayrağı bütün alıyor.
        assert_eq!(
            range(DockSelection::stepped(None, 2, false, text, true)),
            (1, 3)
        );
        assert_eq!(
            range(DockSelection::stepped(None, 2, true, text, true)),
            (1, 3)
        );
        let forward = DockSelection::stepped(None, 1, true, text, true);
        assert_eq!(range(forward), (1, 3));
        assert_eq!(
            range(DockSelection::stepped(Some(forward), 1, true, text, true)),
            (1, 4)
        );
        assert_eq!(
            range(DockSelection::stepped(None, 3, false, text, false)),
            (2, 3)
        );
    }

    #[test]
    fn the_edit_capability_lives_for_one_prompt() {
        // `w` düzenleme kapısının dördüncü koşulu (031 phase-5): yükü yok,
        // aynanın durumuna dokunmuyor ve prompt'un ömrüyle gidiyor —
        // `line-finish` (`e`) de prompt'un başı (`A`) da siliyor.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let mut scanner = Scanner::new();
        let mut stream = dock_update(0, "", "ls", "", &[]);
        stream.extend_from_slice(b"\x1b]8133;w\x07");
        scanner.feed(&stream, |event| log.apply_scan(event));
        assert!(log.dock_editable, "w yeteneği kurmadı");
        assert_eq!(log.dock.status, DockStatus::Live, "w aynayı oynattı");
        assert_eq!(log.dock.buffer, "ls");

        // Aynanın her tuşu yeteneği silmiyor: tarayıcının `clone_from`'u
        // yalnız aynayı tazeliyor.
        scanner.feed(&dock_update(0, "", "ls -l", "", &[]), |event| {
            log.apply_scan(event)
        });
        assert!(log.dock_editable, "ayna yeteneği sildi");

        scanner.feed(b"\x1b]8133;e\x07", |event| log.apply_scan(event));
        assert!(!log.dock_editable, "line-finish yeteneği silmedi");

        scanner.feed(b"\x1b]8133;w\x07", |event| log.apply_scan(event));
        assert!(log.dock_editable);
        scanner.feed(b"\x1b]133;A;bt_block=3\x07", |event| log.apply_scan(event));
        assert!(!log.dock_editable, "prompt'un başı yeteneği silmedi");
    }

    #[test]
    fn the_script_arms_the_widget_before_it_announces_it() {
        // Betiğin `line-init` kancası: üç keymap'e bağlama, sonra `w`. ZLE
        // olmadan koşuyor (`zsh -f -c`), yani sınanan şey telin iki ucunun
        // aynı harfi konuşması; gerçek ZLE'deki bağlamayı `session.rs`'in
        // uçtan uca sınamaları görüyor.
        let bytes = run_script(
            "source $ZDOTDIR/bateri.zsh
             zmodload zsh/zle
             __bateri_dock_arm
             for map in main emacs viins; do bindkey -M $map $'\\e[8133~'; done",
            &[],
        );
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            text.starts_with("\x1b]8133;w\x07"),
            "yetenek basılmadı: {text:?}"
        );
        assert_eq!(
            text.matches("__bateri_dock_edit").count(),
            3,
            "widget üç keymap'e bağlanmadı: {text:?}"
        );
        assert_eq!(dock_events(&bytes[..9]), vec![DockSnapshot::Editable]);
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
        assert_eq!(exit_of(&log, BLOCK_LOG_FLOOR as u32 + 1), Some(Some(0)));
    }

    /// Sayacın dört kademesi; sınırların **iki yakası** da sınanıyor.
    ///
    /// Kademe sınırında bir `<` yerine `<=` yazmak belirtisi olmayan bir kusur
    /// olurdu: `10s` yerine `10.0s` yazan bir sayaç yanlış değil, yalnız
    /// tasarımın dışında — ve hiçbir derleyici onu görmez.
    #[test]
    fn the_counter_reads_its_four_tiers() {
        let text = |ms| {
            Counter::new(Duration::from_millis(ms), Precision::Tenths)
                .as_str()
                .to_owned()
        };

        // **Bitmiş değerde ondalık, saniye kademesinin tamamında.** Eşiğin
        // hemen üstünden dakikanın hemen altına: sınır on saniye **değil**
        // (kullanıcı kararı) — donmuş bir değerde ondalığın bedeli yok ve
        // "asıl sayı" sorusu on saniyeden sonra da geçerli.
        assert_eq!(text(1_000), "1.0s");
        assert_eq!(text(1_449), "1.4s");
        assert_eq!(text(9_999), "9.9s");
        assert_eq!(text(10_000), "10.0s");
        assert_eq!(text(45_300), "45.3s");
        assert_eq!(text(59_999), "59.9s");
        // Kırpma, yuvarlama değil: 1.49 saniye "1.4s", "1.5s" değil. Sayaç
        // ileri değil geri dürüst olsun.
        assert_eq!(text(1_499), "1.4s");

        // Dakikadan itibaren ondalık düşüyor: `1m 05.3s` hem uzun hem
        // okunmuyor, orada aranan şey kaba büyüklük. Saniye iki hane, yoksa
        // "1m 5s" ile "1m 50s" karışır.
        assert_eq!(text(60_000), "1m 00s");
        assert_eq!(text(65_000), "1m 05s");
        assert_eq!(text(3_599_999), "59m 59s");

        // Saat.
        assert_eq!(text(3_600_000), "1h 00m");
        assert_eq!(text(3_720_000), "1h 02m");
    }

    /// **Koşan sayaç ondalık göstermiyor**, bitmiş olan gösteriyor.
    ///
    /// Ayrımın sebebi hem okuma hem pil: koşan sayacın her değişimi bir kare
    /// istiyor, yani onda bir **saniyede on kare** ederdi. Ayrım kaldırılırsa
    /// burası kızarır ve saat sessizce 10 Hz'e çıkardı.
    ///
    /// İki uçta da sınanıyor, çünkü ondalık artık dakikaya kadar uzanıyor:
    /// koşan `45s` ile bitmiş `45.3s` aynı süreden doğuyor.
    #[test]
    fn a_running_counter_costs_one_frame_a_second() {
        let elapsed = Duration::from_millis(3_400);
        assert_eq!(
            Counter::new(elapsed, Precision::Whole).as_str(),
            "3s",
            "koşan sayaç ondalık gösteriyor"
        );
        assert_eq!(Counter::new(elapsed, Precision::Tenths).as_str(), "3.4s");

        // Onda birin eski tavanının (10 sn) üstü: koşan hâlâ tam saniye.
        let long = Duration::from_millis(45_300);
        assert_eq!(
            Counter::new(long, Precision::Whole).as_str(),
            "45s",
            "koşan sayaç on saniyeden sonra da tam saniye kalmalı"
        );
        assert_eq!(Counter::new(long, Precision::Tenths).as_str(), "45.3s");

        // Tik tam saniyeye kuruluyor: 3.4 saniyede 600 ms kaldı.
        assert_eq!(next_tick(elapsed), Duration::from_millis(600));
        // Tam saniyede sıfır değil **bir** saniye: sıfır süreli bir saat
        // callback'i döngüye sokardı.
        assert_eq!(next_tick(Duration::from_secs(3)), Duration::from_secs(1));
        // Eşiğin altında bir sonraki değişim sayacın **belirmesi**.
        assert_eq!(
            next_tick(Duration::from_millis(200)),
            Duration::from_millis(800)
        );
    }

    /// Saat kademesinde tik **dakikada bir**, saniyede bir değil.
    ///
    /// Metin (`1h 07m`) dakikada bir değişiyor; saniyede bir uyandırmak bir
    /// saatte 3540 **aynı** kareyi çizdirirdi ve modül başlığına yazdığımız
    /// "içerik gerçekten değişecek" şartını ilk ihlal eden biz olurduk
    /// (`/code-review`, 013 kapı).
    #[test]
    fn the_hour_tier_ticks_once_a_minute() {
        // 1 saat 7 dakika 20 saniye: bir sonraki dakikaya 40 saniye.
        let elapsed = Duration::from_secs(3600 + 7 * 60 + 20);
        assert_eq!(Counter::new(elapsed, Precision::Whole).as_str(), "1h 07m");
        assert_eq!(next_tick(elapsed), Duration::from_secs(40));

        // Tam dakikada sıfır değil **bir dakika**: sıfır süreli bir saat
        // callback'i döngüye sokardı.
        assert_eq!(
            next_tick(Duration::from_secs(3600)),
            Duration::from_secs(60)
        );

        // Sınırın altı hâlâ saniyede bir: `59m 59s` her saniye değişiyor.
        assert_eq!(next_tick(Duration::from_secs(3599)), Duration::from_secs(1));
    }

    /// Kaybolan bir `D` **sonraki** bloğa yazılmıyor.
    ///
    /// Saat yalnız `D`'de tüketilseydi yarıda kesilmiş bir OSC'den sonra
    /// bayat `Instant` ayakta kalır ve bir sonraki bloğun `D`'si onu
    /// tüketirdi: anlık bir komut dakikalarca sürmüş görünürdü
    /// (`/code-review`, 013 kapı). `A` ikinci sıfırlama noktası.
    #[test]
    fn a_lost_command_end_does_not_charge_the_next_block() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        // 1. blok koşuyor ama `D`'si hiç gelmiyor.
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::CommandStart);
        assert!(log.running_since.is_some());

        // 2. bloğun prompt'u: saat burada sıfırlanmalı.
        log.apply(Mark::PromptStart { id: Some(2) });
        assert!(
            log.running_since.is_none(),
            "`A` bayat saati temizlemeliydi"
        );

        // 2. blok `C` görmeden kapanıyor (kabuk yine de `D` basıyor).
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(2),
        });
        assert_eq!(
            log.duration(2, None),
            Some(Duration::ZERO),
            "kaybolan `D` sonraki bloğa süre yazdı"
        );
    }

    /// **İkinci bir OSC 133 kaynağı saati çalmıyor.**
    ///
    /// Kullanıcının kabuğunda iTerm2'nin entegrasyonu kuruluysa
    /// (`~/.iterm2_shell_integration.zsh`) her komut **iki** `C` ve **iki**
    /// `D` doğuruyor; onunki kimliksiz, bizimki `bt_block=` taşıyor. Dizi
    /// gerçek bir makinede ölçüldü ve aynen budur.
    ///
    /// Kimliksiz `D` saati tüketiyordu: bizimki boş buluyor, süre **sıfır**
    /// yazılıyor ve sayaç eşiğin altında kalıp hiç çizilmiyordu. Kullanıcının
    /// gördüğü kusur buydu ve hiçbir sınama göremiyordu, çünkü hepsi tek
    /// kaynaklı bir akış varsayıyordu.
    #[test]
    fn a_foreign_integration_does_not_steal_the_clock() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::PromptEnd);
        // İki `C`: yabancı + bizim. İlki kazanmalı.
        log.apply(Mark::CommandStart);
        log.apply(Mark::CommandStart);
        std::thread::sleep(Duration::from_millis(60));
        // İki `D`: önce yabancının kimliksizi, sonra bizimki.
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: None,
        });
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        log.apply(Mark::PromptStart { id: Some(2) });

        let measured = log.duration(1, None).expect("biten blokta süre olmalı");
        assert!(
            measured >= Duration::from_millis(50),
            "yabancı `D` saati çaldı, süre sıfıra düştü: {measured:?}"
        );
    }

    /// Tamponun tavanı temsil edilebilir en uzun metni alıyor.
    ///
    /// [`Counter::CAPACITY`] bir tahmin değil türetme: süre `u32` milisaniye,
    /// yani en çok ~1193 saat. Tavan küçültülürse `Counter::new`'in
    /// `debug_assert`'ü burada patlar — sürüm derlemesinde metin sessizce
    /// kırpılırdı.
    #[test]
    fn the_longest_counter_fits_the_buffer() {
        let longest = Counter::new(
            Duration::from_millis(u64::from(u32::MAX)),
            Precision::Tenths,
        );
        assert_eq!(longest.as_str(), "1193h 02m");
        assert!(
            longest.as_str().len() <= Counter::CAPACITY,
            "sayaç tamponu en uzun metni almıyor: {}",
            longest.as_str()
        );
    }

    /// `C` görmeden `D` gelen blok **sıfır** süre kaydeder, uydurma değil.
    ///
    /// Yol gerçek: kimliksiz bir `A`'dan sonra gelen `D`, ya da entegrasyonun
    /// yarısını basan bir kabuk. Sıfır eşiğin altında kalıyor, yani sayaç
    /// çizilmiyor — "bilinmeyen çizilmez" kuralının süre kolu.
    #[test]
    fn a_command_that_never_started_records_no_time() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });

        let duration = log.duration(1, None).expect("biten blokta süre olmalı");
        assert_eq!(duration, Duration::ZERO);
        assert!(duration < COUNTER_FLOOR, "sıfır süre eşiği geçmemeli");
    }

    /// Saat `D`'de **tükeniyor**: iki komut arası koşan bir komut yok.
    ///
    /// `take` yerine okuma yapılsaydı `Finished` safhasında (içinde bir `git`
    /// fork'u) bitmiş komut hâlâ sayıyormuş gibi görünürdü ve phase-2'de saat
    /// hiç durmazdı.
    #[test]
    fn the_clock_is_spent_when_the_command_ends() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::CommandStart);
        assert!(log.running_since.is_some(), "`C` saati dikmeliydi");

        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        assert!(log.running_since.is_none(), "`D` saati tüketmeliydi");
    }

    #[test]
    fn the_title_prefers_the_application_then_the_directory() {
        let home = Path::new("/Users/someone");
        // OSC 0/2 kazanır, dizin ne olursa olsun.
        assert_eq!(title_of(Some("vim"), Some("/tmp"), Some(home), None), "vim");
        // Boş (ya da yalnız boşluk) OSC başlığı yok sayılır, dizine düşer.
        assert_eq!(title_of(Some(""), Some("/tmp"), Some(home), None), "tmp");
        assert_eq!(title_of(Some("  "), Some("/tmp"), Some(home), None), "tmp");
        // `ResetTitle` yuvayı siliyor: başlık dizine döner.
        assert_eq!(
            title_of(None, Some("/usr/local/bin"), Some(home), None),
            "bin"
        );
        // Ev dizininin kendisi `~`, alt dizini son bileşen.
        assert_eq!(
            title_of(None, Some("/Users/someone"), Some(home), None),
            "~"
        );
        assert_eq!(
            title_of(None, Some("/Users/someone/"), Some(home), None),
            "~"
        );
        assert_eq!(
            title_of(None, Some("/Users/someone/proj"), Some(home), None),
            "proj"
        );
        // Ev dizini bilinmiyorsa ev de sıradan bir dizin.
        assert_eq!(
            title_of(None, Some("/Users/someone"), None, None),
            "someone"
        );
        // Kök.
        assert_eq!(title_of(None, Some("/"), Some(home), None), "/");
        // Hiçbiri: uygulamanın adı.
        assert_eq!(title_of(None, None, Some(home), None), "bateri");
        assert_eq!(title_of(None, Some(""), Some(home), None), "bateri");
    }

    #[test]
    fn a_remote_session_marks_the_title() {
        // 036 Karar 5: uzak etkinken dizin hiç sorulmuyor; OSC başlığı
        // önekle, yoksa host.
        let home = Path::new("/Users/someone");
        let remote = Some("prod");
        assert_eq!(
            title_of(Some("deploy@prod: ~"), Some("/tmp"), Some(home), remote),
            "⇄ deploy@prod: ~"
        );
        assert_eq!(title_of(None, Some("/tmp"), Some(home), remote), "⇄ prod");
        // Boş OSC başlığı yok sayılıyor: host'a düşer, dizine değil.
        assert_eq!(
            title_of(Some(" "), Some("/tmp"), Some(home), remote),
            "⇄ prod"
        );
        // Dizin hiç yokken de host.
        assert_eq!(title_of(None, None, None, remote), "⇄ prod");
    }

    #[test]
    fn only_a_different_directory_changes_the_title_input() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        let title = |log: &mut ShellLog, event| log.apply_scan_answering(event, 0).title;
        assert!(title(&mut log, local_cwd("/tmp")));
        // Aynı dizini basan ikinci `precmd` haber doğurmaz.
        assert!(!title(&mut log, local_cwd("/tmp")));
        assert!(title(&mut log, local_cwd("/")));
        // Dizin dışı olaylar başlığın girdisine hiç dokunmaz.
        assert!(!title(&mut log, ScanEvent::Mark(Mark::PromptEnd)));
        assert!(!title(&mut log, ScanEvent::Dock(DockEvent::End)));
        // Uzak durum yokken `A` ile `D` de dokunmaz.
        assert!(!title(
            &mut log,
            ScanEvent::Mark(Mark::PromptStart { id: None })
        ));
        assert_eq!(log.context.cwd, "/");
    }

    fn local_cwd(path: &str) -> ScanEvent<'_> {
        ScanEvent::Cwd { path, local: true }
    }

    fn foreign_cwd(path: &str) -> ScanEvent<'_> {
        ScanEvent::Cwd { path, local: false }
    }

    /// Komut koşan (`C`) bir defter; nesli ve safhası hazır.
    fn running_log() -> ShellLog {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: Some(1) });
        log.apply(Mark::PromptEnd);
        log.apply(Mark::CommandStart);
        log
    }

    #[test]
    fn only_the_transition_into_running_starts_a_command() {
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        assert_eq!(log.command, 0);
        let first = log.apply(Mark::CommandStart);
        assert!(first.started, "ilk `C` bir geçiş");
        assert_eq!(log.command, 1);
        // İkinci `C` (iTerm2) geçiş değil: nesil oynamıyor, haber yok.
        assert!(!log.apply(Mark::CommandStart).started);
        assert_eq!(log.command, 1);
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: None,
        });
        log.apply(Mark::PromptStart { id: None });
        assert!(log.apply(Mark::CommandStart).started);
        assert_eq!(log.command, 2);
    }

    #[test]
    fn the_second_command_start_keeps_the_remote_host() {
        let mut log = running_log();
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        // Komut ortasındaki ikinci `C` uzak durumu silmiyor: yoklama `D`'ye
        // kadar kilitli ve gösterge geri gelmezdi.
        let outcome = log.apply(Mark::CommandStart);
        assert_eq!(outcome, ScanOutcome::default());
        assert_eq!(log.context.remote_host(), Some("prod"));
    }

    #[test]
    fn end_and_prompt_clear_the_remote_state() {
        for mark in [
            Mark::CommandEnd {
                exit: Some(0),
                id: Some(1),
            },
            Mark::PromptStart { id: Some(2) },
        ] {
            let mut log = running_log();
            assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
            log.apply_scan_answering(foreign_cwd("/srv"), 0);
            assert_eq!(log.context.remote_cwd, "/srv");
            let outcome = log.apply(mark);
            assert!(outcome.title, "{mark:?}: silme başlığın girdisi");
            assert_eq!(log.context.remote, None, "{mark:?}");
            assert_eq!(log.context.remote_cwd, "", "{mark:?}");
            // Silinmiş durumu ikinci kez silmek haber değil.
            assert!(!log.apply(mark).title, "{mark:?}");
        }
    }

    #[test]
    fn foreign_marks_leave_a_remote_session_alone() {
        // ssh'ın öbür ucundaki fish 4 / kitty entegrasyonu: uzak `A`, `B`,
        // `C`, `D` bizim kimliğimizi taşımıyor ve ne uzak durumu siliyor ne
        // nesli ilerletiyor ne de safhayı `Running`'den çıkarıyor.
        let mut log = running_log();
        let command = log.running_command();
        assert!(command.is_some());
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        for mark in [
            Mark::PromptStart { id: None },
            Mark::PromptEnd,
            Mark::CommandStart,
            Mark::CommandEnd {
                exit: Some(1),
                id: None,
            },
        ] {
            assert_eq!(log.apply(mark), ScanOutcome::default(), "{mark:?}");
            assert_eq!(log.context.remote_host(), Some("prod"), "{mark:?}");
            assert_eq!(log.running_command(), command, "{mark:?}");
        }
        // Bizim `D`'miz komutu bitiriyor ve uzak durumu siliyor.
        let outcome = log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        assert!(outcome.title);
        assert_eq!(log.context.remote, None);
        assert_eq!(log.running_command(), None);
    }

    #[test]
    fn a_foreign_prompt_before_the_probe_keeps_the_command_running() {
        // Uzak `A` yoklamadan önce aynı okumada geldi: safha `Prompt`'a
        // döndü ama bizim `D`'miz gelmedi, yani komut koşuyor ve yoklamanın
        // cevabı kabul ediliyor.
        let mut log = running_log();
        let command = log.running_command();
        log.apply(Mark::PromptStart { id: None });
        log.apply(Mark::PromptEnd);
        assert_eq!(log.running_command(), command);
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        assert_eq!(log.running_command(), None);
    }

    #[test]
    fn a_foreign_shell_without_a_remote_session_drives_the_phase() {
        // `exec fish`: kimliğimiz bir daha gelmiyor. Uzak oturum yokken
        // fish'in işaretleri safhayı sürüyor, yoksa `Running` hiç bitmez ve
        // saat boşta kare isterdi.
        let mut log = running_log();
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: None,
        });
        log.apply(Mark::PromptStart { id: None });
        assert_ne!(
            log.state.map(|state| state.phase),
            Some(ShellPhase::Running)
        );
        assert_eq!(log.running_since, None, "saat durdu");
    }

    #[test]
    fn without_our_marks_foreign_marks_still_drive_the_phase() {
        // Entegrasyonu kapalı kabuk + kendi 133'ü: kimliğimiz hiç gelmedi,
        // yani kimliksiz `D` komutu bitirmek zorunda — yoksa `Running`
        // sonsuza kadar sürer.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply(Mark::PromptStart { id: None });
        log.apply(Mark::CommandStart);
        assert!(log.running_command().is_some());
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: None,
        });
        assert_eq!(log.running_command(), None);
    }

    #[test]
    fn a_new_command_clears_a_foreign_directory() {
        // Yoklamadan önce gelen yabancı OSC 7 uzak yuvada bekliyor; ama bir
        // önceki komutun kalıntısı bir sonrakine taşınmıyor.
        let mut log = ShellLog::new(BLOCK_LOG_FLOOR);
        log.apply_scan_answering(foreign_cwd("/srv"), 0);
        assert_eq!(log.context.remote_cwd, "/srv");
        log.apply(Mark::CommandStart);
        assert_eq!(log.context.remote_cwd, "");
    }

    #[test]
    fn osc7_routes_by_authority_and_remote_state() {
        let mut log = running_log();
        // Yerel yetki bugünkü gibi yerel dizine.
        assert!(log.apply_scan_answering(local_cwd("/Users/me"), 0).title);
        // Yabancı yetki uzak yuvaya; yerel dizin ve başlık kıpırdamıyor.
        let outcome = log.apply_scan_answering(foreign_cwd("/var/www"), 0);
        assert!(!outcome.title);
        assert_eq!(
            (log.context.cwd.as_str(), log.context.remote_cwd.as_str()),
            ("/Users/me", "/var/www")
        );
        // Uzak etkinken **boş yetki de** uzak yuvaya: ssh'ın arkasında yerel
        // kabuk bloklu.
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        let outcome = log.apply_scan_answering(local_cwd("/home/deploy"), 0);
        assert!(!outcome.title);
        assert_eq!(
            (log.context.cwd.as_str(), log.context.remote_cwd.as_str()),
            ("/Users/me", "/home/deploy")
        );
    }

    #[test]
    fn set_remote_reports_only_a_change() {
        let mut log = running_log();
        assert!(!log.set_remote(None), "yok → yok değişim değil");
        assert!(
            !log.set_remote(Some(&RemoteTarget::ssh(""))),
            "boş host uzak değil"
        );
        assert!(
            !log.set_remote(Some(&RemoteTarget::ssh("prod\n"))),
            "kontrol karakteri"
        );
        assert!(
            !log.set_remote(Some(&RemoteTarget::ssh("\u{1b}[31mprod"))),
            "ESC"
        );
        assert!(log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        assert!(!log.set_remote(Some(&RemoteTarget::ssh("prod"))));
        assert!(log.set_remote(Some(&RemoteTarget::ssh("deploy@10.0.0.5"))));
        assert_eq!(log.context.remote_host(), Some("deploy@10.0.0.5"));
        assert!(log.set_remote(None));
        assert_eq!(log.context.remote, None);
    }

    fn rule(pattern: &str, mark: HostMark) -> HostRule {
        HostRule {
            pattern: pattern.to_owned(),
            mark,
        }
    }

    #[test]
    fn the_remote_target_is_kept_whole_and_its_mark_resolved() {
        // 037 Karar 1, 2: hedef bütün olarak duruyor; işaret `set_remote`'ta
        // ve listenin değişiminde çözülüyor, `C`/`D`/`A` onu da siliyor.
        let mut log = running_log();
        assert!(!log.set_host_rules(&[rule("prod-*", HostMark::Production)]));
        let target = RemoteTarget {
            host: "deploy@prod-web-1".to_owned(),
            kind: RemoteKind::Ssh,
            argv: ["ssh", "-p", "2222", "deploy@prod-web-1"]
                .map(str::to_owned)
                .to_vec(),
            line: "ssh -p 2222 deploy@prod-web-1".to_owned(),
        };
        assert!(log.set_remote(Some(&target)));
        assert_eq!(log.context.remote.as_ref(), Some(&target));
        assert_eq!(log.context.remote_mark, HostMark::Production);

        // Aynı host, başka argv: hedef yazılıyor ama başlığın girdisi aynı.
        let other = RemoteTarget {
            argv: ["ssh", "deploy@prod-web-1"].map(str::to_owned).to_vec(),
            line: "ssh deploy@prod-web-1".to_owned(),
            ..target.clone()
        };
        assert!(!log.set_remote(Some(&other)));
        assert_eq!(log.context.remote.as_ref(), Some(&other));

        // Liste değişimi işareti yeniden çözüyor; aynı liste no-op, işareti
        // oynatmayan liste `false`.
        let staging = [rule("*", HostMark::Staging)];
        assert!(log.set_host_rules(&staging));
        assert_eq!(log.context.remote_mark, HostMark::Staging);
        assert!(!log.set_host_rules(&staging));
        assert!(!log.set_host_rules(&[rule("prod-web-?", HostMark::Staging)]));
        assert!(log.set_host_rules(&[]));
        assert_eq!(log.context.remote_mark, HostMark::None);

        // Bizim `D`'miz uzak durumu işaretiyle birlikte siliyor.
        assert!(log.set_host_rules(&staging));
        log.apply(Mark::CommandEnd {
            exit: Some(0),
            id: Some(1),
        });
        assert_eq!(log.context.remote, None);
        assert_eq!(log.context.remote_mark, HostMark::None);
    }
}
