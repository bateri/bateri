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

/// Kabuğun akışa bastığı tek bir OSC 133 işareti.
///
/// Dördü de kabuktan bağımsızdır: tipte ne zsh, ne bash, ne fish geçer
/// (R2.4). Yeni bir kabuk eklemek yalnız bir betik yazmaktır.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    /// `A` — prompt burada başlıyor.
    PromptStart,
    /// `B` — prompt bitti, bundan sonrası kullanıcının yazdığı komut.
    PromptEnd,
    /// `C` — komut koşmaya başladı, bundan sonrası çıktı.
    CommandStart,
    /// `D` — komut bitti. Kod **opsiyoneldir**: kabuk `D`'yi çıplak da
    /// basabilir ve okunamayan bir parametre komutun bittiği bilgisini
    /// çürütmez — "bitti ama kodu bilmiyorum" doğru cevaptır.
    CommandEnd { exit: Option<i32> },
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

impl ShellState {
    /// İşareti duruma uygular; ilk işaret durumu **doğurur**.
    ///
    /// Yuva `Option` olduğu için "hiç işaret görmedik" ile "prompt'tayız"
    /// karışmıyor: besleyen yokken yuva boş kalır ve dışarıya "entegrasyon
    /// yok" der.
    pub(crate) fn apply(slot: &mut Option<Self>, mark: Mark) {
        let state = slot.get_or_insert(Self {
            phase: ShellPhase::Prompt,
            last_exit: None,
        });
        match mark {
            Mark::PromptStart => state.phase = ShellPhase::Prompt,
            Mark::PromptEnd => state.phase = ShellPhase::Input,
            Mark::CommandStart => state.phase = ShellPhase::Running,
            Mark::CommandEnd { exit } => {
                state.phase = ShellPhase::Finished;
                // Kodu **her hâlde** tazeliyoruz: okunamayan bir kodu eskisiyle
                // doldurmak, biten komutu başkasının koduyla etiketlemek olurdu.
                state.last_exit = exit;
            }
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
        b"A" => Some(Mark::PromptStart),
        b"B" => Some(Mark::PromptEnd),
        b"C" => Some(Mark::CommandStart),
        b"D" => {
            let exit = fields
                .next()
                .and_then(|field| std::str::from_utf8(field).ok())
                .and_then(|text| text.parse().ok());
            Some(Mark::CommandEnd { exit })
        }
        _ => None,
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
            scanner.feed(chunk, |mark| seen.push(mark));
        }
        seen
    }

    fn marks(bytes: &[u8]) -> Vec<Mark> {
        marks_of_chunks(&[bytes])
    }

    #[test]
    fn four_marks_are_recognised() {
        assert_eq!(marks(b"\x1b]133;A\x07"), vec![Mark::PromptStart]);
        assert_eq!(marks(b"\x1b]133;B\x07"), vec![Mark::PromptEnd]);
        assert_eq!(marks(b"\x1b]133;C\x07"), vec![Mark::CommandStart]);
        assert_eq!(
            marks(b"\x1b]133;D\x07"),
            vec![Mark::CommandEnd { exit: None }]
        );
    }

    #[test]
    fn command_end_carries_the_exit_code() {
        assert_eq!(
            marks(b"\x1b]133;D;0\x07"),
            vec![Mark::CommandEnd { exit: Some(0) }]
        );
        assert_eq!(
            marks(b"\x1b]133;D;130\x07"),
            vec![Mark::CommandEnd { exit: Some(130) }]
        );
    }

    #[test]
    fn unreadable_exit_code_still_ends_the_command() {
        // Komutun bittiği bilgisi kodundan değerli: yükün parametresi bozuk
        // olsa da işaret düşmez, yalnız kod bilinmez.
        assert_eq!(
            marks(b"\x1b]133;D;abc\x07"),
            vec![Mark::CommandEnd { exit: None }]
        );
    }

    #[test]
    fn attributes_beside_the_mark_are_tolerated() {
        assert_eq!(marks(b"\x1b]133;A;aid=42\x07"), vec![Mark::PromptStart]);
        assert_eq!(
            marks(b"\x1b]133;D;0;aid=42\x07"),
            vec![Mark::CommandEnd { exit: Some(0) }]
        );
    }

    #[test]
    fn both_terminators_end_the_sequence() {
        assert_eq!(marks(b"\x1b]133;A\x07"), vec![Mark::PromptStart]);
        assert_eq!(marks(b"\x1b]133;A\x1b\\"), vec![Mark::PromptStart]);
    }

    #[test]
    fn bare_escape_dispatches_like_vte() {
        // `vte` diziyi ESC'i görünce dağıtıyor, `\`'i beklemeden: peş peşe iki
        // dizi araya sonlandırıcı girmeden de okunur.
        assert_eq!(
            marks(b"\x1b]133;A\x1b]133;B\x07"),
            vec![Mark::PromptStart, Mark::PromptEnd]
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
                vec![Mark::CommandEnd { exit: Some(7) }],
                "bölünme noktası {at}"
            );
        }
    }

    #[test]
    fn escape_survives_the_bytes_vte_executes_in_place() {
        // `advance_esc` bu baytlarda `Escape`'te kalıyor, yani ardından gelen
        // `]` diziyi gerçekten açıyor. Ground'a düşen bir tarayıcı işareti
        // sessizce kaybeder ve durum ızgaradan ayrılırdı.
        assert_eq!(marks(b"\x1b\r]133;A\x07"), vec![Mark::PromptStart]);
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

        assert_eq!(seen, vec![Mark::PromptStart]);
        assert_eq!(scanner.payload.capacity(), PAYLOAD_LIMIT);
    }

    #[test]
    fn control_bytes_inside_the_payload_are_dropped_like_vte() {
        // `vte` yüke almıyor; almasaydık satır sonu yapışmış bir kod bozuk
        // görünürdü.
        assert_eq!(
            marks(b"\x1b]133;D;0\r\x07"),
            vec![Mark::CommandEnd { exit: Some(0) }]
        );
    }

    #[test]
    fn plain_text_around_the_marks_is_ignored() {
        assert_eq!(
            marks(b"merhaba\x1b]133;A\x07dunya\x1b]133;B\x07$ ls\r\n"),
            vec![Mark::PromptStart, Mark::PromptEnd]
        );
    }

    #[test]
    fn marks_walk_the_state_through_a_whole_command() {
        let mut slot = None;
        ShellState::apply(&mut slot, Mark::PromptStart);
        assert_eq!(slot.map(|s| s.phase), Some(ShellPhase::Prompt));

        ShellState::apply(&mut slot, Mark::PromptEnd);
        assert_eq!(slot.map(|s| s.phase), Some(ShellPhase::Input));

        ShellState::apply(&mut slot, Mark::CommandStart);
        assert_eq!(slot.map(|s| s.phase), Some(ShellPhase::Running));

        ShellState::apply(&mut slot, Mark::CommandEnd { exit: Some(2) });
        assert_eq!(
            slot,
            Some(ShellState {
                phase: ShellPhase::Finished,
                last_exit: Some(2),
            })
        );
    }

    #[test]
    fn an_unreadable_code_does_not_inherit_the_previous_one() {
        let mut slot = None;
        ShellState::apply(&mut slot, Mark::CommandEnd { exit: Some(2) });
        ShellState::apply(&mut slot, Mark::CommandEnd { exit: None });
        assert_eq!(slot.and_then(|s| s.last_exit), None);
    }
}
