//! Finder damlasının uzak dizine yüklenmesi (037 Karar 7 → Kullanıcı
//! kararı): uzak oturumda bırakılan dosya ya da klasör onaydan sonra
//! `tar c | ssh … tar x` akışıyla uzak kabuğun dizinine gidiyor. Hiçbir şey
//! kendiliğinden yapıştırılmıyor (037 phase-7); sonuç satırı nereye
//! gittiğini söylüyor.
//!
//! Üç yarı:
//!
//! - **Saf:** ssh argv'sinin çevirisi ([`ssh_argv`]), uzak betikler ve
//!   tırnaklama, yoklamanın cevabı ([`parse_probe`]), tar akışının başlık
//!   okuyucusu ([`TarWatcher`]), satırın ve sayfanın metni. Sınanan yarı.
//! - **Süreç:** yerel ölçüm ([`measure`]), yoklama ([`probe`]) ve akış
//!   ([`transfer`]) — hepsi arka plan thread'inde, sonuç ana kuyruğa.
//! - **Kuyruk** ([`Uploads`]): ana thread'in durumu — sıra, ilerleme,
//!   sonuç satırı. AppKit'siz; sayfayı ve dispatch'i `window` kuruyor.
//!
//! **Parola sorulamaz.** GUI'den doğan ssh'ın kontrol terminali yok;
//! `BatchMode=yes` ile anahtar/agent yoksa anında ve açık bir hatayla düşüyor,
//! asılı kalmıyor. Kullanıcının `ControlMaster`'ı varsa ona biniyor, yani
//! parolalı bir host'ta bile açık bir ana bağlantı yetiyor.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use bt_core::{
    HostMark, RemoteKind, RemoteTarget, Transfer, TransferAction, TransferControls, TransferTone,
};

use crate::jobs::SSH_VALUED;

/// İlerleme haberinin en sık aralığı — **tasarım sabiti**. Her haber bir
/// içerik karesi (satır ve çubuk değişti); ekran hızında haber, gözün
/// okuyamayacağı bir sayaç için kare yakmak olurdu. Beşte bir saniye çubuğu
/// akıcı, sayıları okunur tutuyor.
pub(crate) const TICK: Duration = Duration::from_millis(200);

/// Hızın ölçüldüğü pencere — **tasarım sabiti**: anlık hız paket paket
/// zıplıyor, uzun ortalama ise yavaşlamayı geç söylüyor.
const SPEED_WINDOW: Duration = Duration::from_secs(3);

/// Sonuç satırının (`✓ 3 files uploaded`, `Cancelled — …`) dock'ta kaldığı
/// süre — **tasarım sabiti**. Durma koşulu bu: süre dolunca satır kalkıyor
/// ve bir daha kare istenmiyor.
pub(crate) const LINGER: Duration = Duration::from_secs(4);

// ─── ssh ve uzak betikler ────────────────────────────────────────────────

/// Değer almadan **korunan** ssh bayrakları: adres ailesi, agent/X11
/// yönlendirmesi, sıkıştırma, GSSAPI, sessizlik. Bağlantının kendisini
/// değiştirenler; kalanı (`-t -n -N -W -s -O -v -f -M` …) akışı bozar ya da
/// gürültü üretir ve düşüyor.
const KEPT_FLAGS: &str = "46AaCgKkqXxYy";

/// Değeriyle **korunan** ssh seçenekleri: bind adresi, şifre, config, pkcs11,
/// kimlik, sıçrama, kullanıcı, MAC, `-o`, etiket, port, denetim soketi.
/// Hangisinin değer aldığını `jobs`'un tablosu söylüyor ([`SSH_VALUED`]);
/// ikinci bir tablo yok.
const KEPT_VALUED: &str = "BbcFIiJlmoPpS";

/// Uzak oturumun hedefinden yüklemenin ssh argv'si (sonuna betik eklenecek).
///
/// **Bizim seçeneklerimiz başta**, çünkü ssh bir config anahtarının **ilk**
/// değerini alıyor: kullanıcının `-o RequestTTY=force`'u arkada kalmalı.
/// `-T` (tty yok: akış ikili), `BatchMode=yes` (parola sorulamaz, anında
/// düş), `ControlMaster=no` (açık bir ana bağlantıyı **kullan** ama ana
/// olma: arka plana düşen bir ana bağlantı akışın boru ucunu tutardı).
///
/// ssh'ta hedefin argv'si seçenek seçenek süzülüyor ([`KEPT_FLAGS`],
/// [`KEPT_VALUED`]); hedeften sonra gelen seçenekler de okunuyor (OpenSSH
/// onları yeniden ayrıştırıyor) ve uzak komut (`-t prod tmux`) düşüyor.
/// mosh'ta ssh argv'si yok: yalnız host, varsayılan ssh ayarlarıyla.
pub(crate) fn ssh_argv(target: &RemoteTarget) -> Vec<String> {
    let mut argv = vec![
        "-T".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ControlMaster=no".to_owned(),
    ];
    let program = match target.kind {
        RemoteKind::Mosh => {
            argv.push(target.host.clone());
            "ssh".to_owned()
        }
        RemoteKind::Ssh => {
            let program = target.argv.first().cloned().unwrap_or_else(|| "ssh".into());
            let args = target.argv.get(1..).unwrap_or_default();
            let mut destination = None;
            let mut index = 0;
            while let Some(arg) = args.get(index) {
                index += 1;
                if arg == "--" {
                    if destination.is_none() {
                        destination = args.get(index).cloned();
                    }
                    break;
                }
                let Some(cluster) = arg.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
                    if destination.is_some() {
                        // Hedeften sonraki ilk seçenek-olmayan: uzak komut.
                        break;
                    }
                    destination = Some(arg.clone());
                    continue;
                };
                for (at, flag) in cluster.char_indices() {
                    if SSH_VALUED.contains(flag) {
                        let attached = &cluster[at + flag.len_utf8()..];
                        let value = if attached.is_empty() {
                            index += 1;
                            args.get(index - 1).cloned()
                        } else {
                            Some(attached.to_owned())
                        };
                        if KEPT_VALUED.contains(flag)
                            && let Some(value) = value
                        {
                            argv.push(format!("-{flag}"));
                            argv.push(value);
                        }
                        break;
                    }
                    if KEPT_FLAGS.contains(flag) {
                        argv.push(format!("-{flag}"));
                    }
                }
            }
            argv.extend(destination.or_else(|| Some(target.host.clone())));
            program
        }
    };
    argv.insert(0, program);
    argv
}

/// POSIX tek tırnağı; içteki `'` `'"'"'` olarak — **ters bölü üretmiyor**.
///
/// Uzak komutu önce kullanıcının **giriş kabuğu** okuyor (fish, csh da
/// olabilir) ve fish tek tırnağın içinde `\'` ile `\\`'yi kaçış sayıyor;
/// `'\''` biçimi iç içe tırnakta ters bölüyü dış katmana taşır ve fish'te
/// komutu bozardı. `"'"` her kabukta aynı okunuyor.
fn sq(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len() + 2);
    quoted.push('\'');
    for c in text.chars() {
        if c == '\'' {
            quoted.push_str("'\"'\"'");
        } else {
            quoted.push(c);
        }
    }
    quoted.push('\'');
    quoted
}

/// Uzak komut: POSIX betiği `sh -c`'ye sarılmış hâli. Giriş kabuğu yalnız
/// `sh -c '…'`'yi okuyor, yani betiğin sözdizimi kabuktan bağımsız.
fn remote_command(script: &str) -> String {
    format!("sh -c {}", sq(script))
}

/// Bu yolun uzak betiğe güvenle girip giremeyeceği: **ters bölü ve kontrol
/// karakteri taşımamalı.** İkisi de iki katmanlı tırnağı (giriş kabuğu + `sh
/// -c`) fish'te ya da csh'ta bozuyor; böyle bir ad sayfada açıkça
/// reddediliyor (bilinen sınır), sessizce yanlış yere yazılmıyor.
fn is_safe(text: &str) -> bool {
    !text.chars().any(|c| c == '\\' || c.is_control())
}

/// Yoklamanın cevabının başladığını söyleyen işaret: giriş kabuğunun rc
/// dosyası (`.bashrc`'nin `echo`'su) çıktıya karışabiliyor ve işaretten önce
/// gelen her şey atlanıyor.
const PROBE_MARK: &str = "BT-UPLOAD";

/// Hedef dizine geçilemediğinde betiğin çıkış kodu.
const NO_DIRECTORY: i32 = 3;

/// Sayfa açılmadan önce uzakta sorulanlar, **tek bağlantıda**: dizinin tam
/// yolu (`dir` yoksa ev dizini), `df -Pk`'nın satırı, `tar`'ın varlığı ve
/// her adın hedefte olup olmadığı (klasör mü). Adlar değil **indeksler**
/// yazılıyor: satır sonu taşıyan bir ad (reddediliyor ama) ayrıştırmayı
/// bozamasın.
pub(crate) fn probe_script(dir: Option<&str>, names: &[String]) -> String {
    let mut script = String::new();
    if let Some(dir) = dir {
        let _ = write!(script, "cd {} || exit {NO_DIRECTORY}; ", sq(dir));
    }
    let _ = write!(
        script,
        "echo {PROBE_MARK}; pwd; df -Pk . | tail -n 1 | sed 's/^/BT-DF /'; \
         command -v tar >/dev/null 2>&1 && echo BT-TAR; "
    );
    for (index, name) in names.iter().enumerate() {
        let name = sq(name);
        let _ = write!(
            script,
            "{{ [ -e {name} ] || [ -L {name} ]; }} && echo BT-E{index}; \
             [ -d {name} ] && echo BT-D{index}; "
        );
    }
    script.push_str("exit 0");
    script
}

/// Yoklamanın cevabı.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProbeReply {
    /// Hedef dizinin uzaktaki tam yolu (`pwd`).
    pub(crate) dir: String,
    /// Boş alan, bayt; `df` okunamadıysa `None` (kontrol yapılamaz, engel
    /// de olmaz).
    pub(crate) free: Option<u64>,
    /// Uzakta `tar` var mı.
    pub(crate) tar: bool,
    /// Hedefte zaten olan adlar: indeks ve klasör mü.
    pub(crate) existing: Vec<(usize, bool)>,
}

/// Yoklamanın standart çıktısı → cevap; işaret yoksa `None` (betik hiç
/// koşmadı).
pub(crate) fn parse_probe(out: &str) -> Option<ProbeReply> {
    let mut lines = out.lines().skip_while(|line| line.trim() != PROBE_MARK);
    lines.next()?;
    let dir = lines.next()?.to_owned();
    let mut reply = ProbeReply {
        dir,
        ..ProbeReply::default()
    };
    let mut existing: Vec<(usize, bool)> = Vec::new();
    // Her satır kendi işaretini taşıyor: `df` hiçbir şey basmasa (yok, ya
    // da bağlama noktasında desteklenmiyor) sıradaki satır onun yerine
    // okunmasın.
    for line in lines {
        let line = line.trim();
        if let Some(df) = line.strip_prefix("BT-DF ") {
            reply.free = df_available(df);
        } else if line == "BT-TAR" {
            reply.tar = true;
        } else if let Some(index) = line.strip_prefix("BT-E").and_then(|n| n.parse().ok()) {
            existing.push((index, false));
        } else if let Some(index) = line
            .strip_prefix("BT-D")
            .and_then(|n| n.parse::<usize>().ok())
            && let Some(entry) = existing.iter_mut().find(|(at, _)| *at == index)
        {
            entry.1 = true;
        }
    }
    reply.existing = existing;
    Some(reply)
}

/// `df -Pk`'nın veri satırından boş alan, bayt. Aygıt adında ya da bağlama
/// noktasında boşluk olabilir: sütun, doluluk yüzdesinin (`NN%`) **hemen
/// solu** diye bulunuyor, baştan sayılarak değil.
fn df_available(line: &str) -> Option<u64> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let capacity = fields.iter().position(|field| {
        field.ends_with('%') && field[..field.len() - 1].parse::<u32>().is_ok()
    })?;
    let kib: u64 = fields.get(capacity.checked_sub(1)?)?.parse().ok()?;
    kib.checked_mul(1024)
}

/// Uzakta akışı açan betik: hedef dizine geç ve standart girdiden çıkar.
/// `-p` izinleri koruyor, `-o` sahipliği **korumuyor** — root olarak
/// yüklenen dosya yerel kullanıcının uid'sine düşmesin.
fn extract_script(dir: &str) -> String {
    format!("cd {} && exec tar -x -p -o -f -", sq(dir))
}

/// İptalde (ya da disk dolunca) yarım kalan dosyayı silen betik; `rel`
/// akıştaki yolu (`./static/app.js`), dizine göre.
fn cleanup_script(dir: &str, rel: &str) -> String {
    format!("cd {} && rm -f -- {}", sq(dir), sq(rel))
}

// ─── tar akışının okuyucusu ──────────────────────────────────────────────

/// Blok boyu.
const BLOCK: usize = 512;

/// pax başlığının biriktirilen en çok boyu — `path=` kaydı için fazlasıyla
/// yeter; daha büyüğü (uzun bir öznitelik listesi) okunmadan geçiyor.
const PAX_LIMIT: usize = 64 * 1024;

/// Kaydın türü.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    /// Sıradan dosya (`0`, `\0`, `7`) ya da sabit bağ (`1`): sayılan.
    File,
    /// pax genişletilmiş başlığı (`x`): sonraki kaydın yolu burada olabilir.
    Pax,
    /// Kalan her şey (klasör, sembolik bağ, küresel pax).
    Other,
}

/// Akıttığımız tar baytlarının **başlıklarını** okuyan durum makinesi:
/// kaç dosya bitti, kaç içerik baytı geçti ve şu an hangi dosya yazılıyor.
///
/// İlerleme bu yüzden kesin: baytları biz taşıyoruz ve her dosyanın sınırını
/// görüyoruz. Yarım dosya ([`Self::current`]) iptalde silinecek olan.
///
/// bsdtar uzun, ASCII dışı ya da büyük dosyada kaydın önüne bir pax başlığı
/// (`x`) koyuyor ve oradaki `path=` sonraki başlığın adını geçersiz kılıyor;
/// boy alanı büyük dosyada 256 tabanlı olabiliyor. İkisi de okunuyor.
#[derive(Debug)]
pub(crate) struct TarWatcher {
    header: Vec<u8>,
    /// Kaydın kalan veri baytı (dolgu hariç).
    remaining: u64,
    /// Verinin arkasındaki dolgu.
    padding: u64,
    entry: Entry,
    pax: Vec<u8>,
    /// pax'ın söylediği, sonraki kaydın yolu.
    next_path: Option<String>,
    /// Başlığı geçmiş ama verisi bitmemiş dosya.
    pub(crate) current: Option<String>,
    /// Verisi bütünüyle geçmiş dosya sayısı.
    pub(crate) files: u64,
    /// Geçen içerik baytı (yalnız dosyaların verisi).
    pub(crate) bytes: u64,
}

impl Default for TarWatcher {
    fn default() -> Self {
        Self {
            header: Vec::with_capacity(BLOCK),
            remaining: 0,
            padding: 0,
            entry: Entry::Other,
            pax: Vec::new(),
            next_path: None,
            current: None,
            files: 0,
            bytes: 0,
        }
    }
}

impl TarWatcher {
    /// Akışın bir sonraki parçası.
    pub(crate) fn feed(&mut self, mut bytes: &[u8]) {
        while !bytes.is_empty() {
            if self.remaining > 0 {
                let take =
                    usize::try_from(self.remaining).map_or(bytes.len(), |r| r.min(bytes.len()));
                let (data, rest) = bytes.split_at(take);
                match self.entry {
                    Entry::File => self.bytes += data.len() as u64,
                    Entry::Pax if self.pax.len() + data.len() <= PAX_LIMIT => {
                        self.pax.extend_from_slice(data);
                    }
                    Entry::Pax | Entry::Other => {}
                }
                self.remaining -= take as u64;
                bytes = rest;
                if self.remaining == 0 {
                    self.entry_done();
                }
                continue;
            }
            if self.padding > 0 {
                let take =
                    usize::try_from(self.padding).map_or(bytes.len(), |p| p.min(bytes.len()));
                self.padding -= take as u64;
                bytes = &bytes[take..];
                continue;
            }
            let take = (BLOCK - self.header.len()).min(bytes.len());
            self.header.extend_from_slice(&bytes[..take]);
            bytes = &bytes[take..];
            if self.header.len() == BLOCK {
                let header = std::mem::take(&mut self.header);
                self.begin(&header);
                self.header = header;
                self.header.clear();
            }
        }
    }

    /// Bir başlık bloğu tamamlandı.
    fn begin(&mut self, header: &[u8]) {
        // Sıfır blok arşivin sonu (ya da dolgusu).
        if header.iter().all(|&byte| byte == 0) {
            return;
        }
        let size = tar_size(&header[124..136]);
        let kind = header[156];
        self.entry = match kind {
            b'0' | 0 | b'7' | b'1' => Entry::File,
            b'x' => Entry::Pax,
            _ => Entry::Other,
        };
        let name = self.next_path.take();
        if self.entry == Entry::File {
            self.current = Some(name.unwrap_or_else(|| tar_name(header)));
        } else if self.entry == Entry::Pax {
            self.pax.clear();
        }
        self.remaining = size;
        self.padding = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
        if size == 0 {
            self.entry_done();
        }
    }

    /// Kaydın verisi bitti.
    fn entry_done(&mut self) {
        match self.entry {
            Entry::File => {
                self.files += 1;
                self.current = None;
            }
            Entry::Pax => self.next_path = pax_path(&self.pax),
            Entry::Other => {}
        }
        self.entry = Entry::Other;
    }
}

/// Başlığın boy alanı: sekizlik ASCII ya da (yüksek bit kuruluysa) 256
/// tabanlı ikili.
fn tar_size(field: &[u8]) -> u64 {
    if field.first().is_some_and(|&byte| byte & 0x80 != 0) {
        return field[1..]
            .iter()
            .fold(u64::from(field[0] & 0x7f), |acc, &byte| {
                (acc << 8) | u64::from(byte)
            });
    }
    field
        .iter()
        .skip_while(|&&byte| byte == b' ')
        .take_while(|&&byte| (b'0'..=b'7').contains(&byte))
        .fold(0, |acc, &byte| acc * 8 + u64::from(byte - b'0'))
}

/// ustar başlığının adı: `prefix/name` (prefix boşsa yalnız ad).
fn tar_name(header: &[u8]) -> String {
    let field = |range: std::ops::Range<usize>| {
        let raw = &header[range];
        let end = raw.iter().position(|&byte| byte == 0).unwrap_or(raw.len());
        String::from_utf8_lossy(&raw[..end]).into_owned()
    };
    let name = field(0..100);
    let prefix = if &header[257..262] == b"ustar" {
        field(345..500)
    } else {
        String::new()
    };
    if prefix.is_empty() {
        name
    } else {
        format!("{prefix}/{name}")
    }
}

/// pax kayıtlarından (`"{uzunluk} {anahtar}={değer}\n"`) `path`.
fn pax_path(data: &[u8]) -> Option<String> {
    let mut rest = data;
    let mut path = None;
    while !rest.is_empty() {
        let space = rest.iter().position(|&byte| byte == b' ')?;
        let length: usize = std::str::from_utf8(&rest[..space]).ok()?.parse().ok()?;
        if length <= space || length > rest.len() {
            return path;
        }
        let record = &rest[space + 1..length];
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Some(value) = record.strip_prefix(b"path=") {
            path = Some(String::from_utf8_lossy(value).into_owned());
        }
        rest = &rest[length..];
    }
    path
}

// ─── metin ───────────────────────────────────────────────────────────────

/// Bayt sayısı, ondalık birimlerle (Finder'ın birimi): `512 B`, `18.2 MB`.
pub(crate) fn format_bytes(bytes: u64) -> String {
    let (value, unit) = scaled(bytes, bytes);
    if unit == "B" {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {unit}")
    }
}

/// `done / total`, ikisi de **toplamın** biriminde: `18.2 / 44.6 MB`.
fn format_pair(done: u64, total: u64) -> String {
    let (value, unit) = scaled(done, total);
    let (whole, _) = scaled(total, total);
    if unit == "B" {
        format!("{done} / {total} B")
    } else {
        format!("{value:.1} / {whole:.1} {unit}")
    }
}

/// `bytes`'ı `reference`'ın birimine çevirir.
fn scaled(bytes: u64, reference: u64) -> (f64, &'static str) {
    const UNITS: [(u64, &str); 3] = [(1_000_000_000, "GB"), (1_000_000, "MB"), (1_000, "KB")];
    for (size, unit) in UNITS {
        if reference >= size {
            return (bytes as f64 / size as f64, unit);
        }
    }
    (bytes as f64, "B")
}

/// Saniyede bayt: `1.2 MB/s`.
fn format_rate(per_second: f64) -> String {
    // audit: yuvarlama yalnız birimi seçmek için; değer f64'ten basılıyor.
    let whole = per_second.max(0.0) as u64;
    let (value, unit) = scaled(whole, whole);
    if unit == "B" {
        format!("{whole} B/s")
    } else {
        let exact = per_second / (whole as f64 / value);
        format!("{exact:.1} {unit}/s")
    }
}

/// Kalan süre: `22s`, `1m 05s`, `1h 02m`.
fn format_duration(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3600, seconds % 3600 / 60),
    }
}

/// `n file(s)`.
fn files_word(count: u64) -> &'static str {
    if count == 1 { "file" } else { "files" }
}

/// `done of total`, ikisi de **toplamın** biriminde: `48.0 of 96.0 MB`
/// (durdurma sorusu).
fn format_of(done: u64, total: u64) -> String {
    format_pair(done, total).replacen(" / ", " of ", 1)
}

/// Kuyruğun nasıl bittiği.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// Hepsi yüklendi.
    Done,
    /// ⌘., satırın `Cancel`'ı ya da popover'ın iptali.
    Cancelled,
    /// Uzakta disk doldu.
    DiskFull,
    /// Uzak oturum (ssh) kapandı; bekleyenler iptal.
    Closed,
    /// Akış hata verdi; ssh'ın ya da tar'ın son satırı.
    Failed(String),
}

/// Kalemin listedeki ve sonuç satırındaki adı: klasörde sondaki `/`
/// (`static/`), dosyada adın kendisi.
fn label(local: &Local) -> String {
    if local.dir {
        format!("{}/", local.name)
    } else {
        local.name.clone()
    }
}

/// Kuyruk bittiğinde kalemlerin özeti: kaç kalem, kaçı yüklendi, hepsinin
/// ortak hedefi (varsa) ve tek kalemin adı.
struct Tally<'a> {
    items: usize,
    done: usize,
    /// Bütün kalemler aynı dizine gittiyse o dizin.
    dest: Option<&'a str>,
    /// Tek kalemse adı ([`label`]).
    single: Option<String>,
}

impl<'a> Tally<'a> {
    fn of(entries: &'a [Item]) -> Self {
        let dest = entries.first().map(|entry| entry.job.dir.as_str());
        let same = entries
            .iter()
            .all(|entry| Some(entry.job.dir.as_str()) == dest);
        Self {
            items: entries.len(),
            done: entries
                .iter()
                .filter(|entry| entry.state == EntryState::Done)
                .count(),
            dest: dest.filter(|_| same),
            single: match entries {
                [entry] => Some(label(&entry.job.local)),
                _ => None,
            },
        }
    }

    /// `backup.tar.gz`, `static/` ya da `3 files`.
    fn what(&self) -> String {
        self.single.clone().unwrap_or_else(|| {
            // audit: kalem sayısı damla başına; `u64`'e sığar.
            format!("{} {}", self.items, files_word(self.items as u64))
        })
    }
}

/// Hatanın sebebi, satırın küçük harfli biçiminde (`disk full on prod`).
fn failure_reason(end: &End, host: &str) -> Option<String> {
    match end {
        End::Done | End::Cancelled => None,
        End::DiskFull => Some(format!("disk full on {host}")),
        End::Closed => Some(format!("connection to {host} lost")),
        End::Failed(reason) => Some(reason.clone()),
    }
}

/// Sonuç satırının gövdesi, tonu ve tonda çizilen baştaki karakter sayısı
/// (037 phase-7): başarı yeşil, iptal sönük, hata metni kırmızı ve
/// arkasındaki sayım sönük.
fn end_line(end: &End, host: &str, tally: &Tally<'_>) -> (String, TransferTone, usize) {
    if let Some(reason) = failure_reason(end, host) {
        let head = format!("Failed — {reason}");
        let lead = head.chars().count();
        let body = format!("{head} · {} of {} uploaded", tally.done, tally.items);
        return (body, TransferTone::Error, lead);
    }
    let body = match (end, tally.dest) {
        (End::Done, Some(dest)) => format!("✓ {} → {dest}", tally.what()),
        (End::Done, None) => format!(
            "✓ {} {} uploaded",
            tally.items,
            // audit: kalem sayısı damla başına; `u64`'e sığar.
            files_word(tally.items as u64)
        ),
        _ if tally.items > 1 => format!(
            "Cancelled — {} of {} uploaded, partial file removed",
            tally.done, tally.items
        ),
        _ => "Cancelled — partial file removed".to_owned(),
    };
    let tone = if *end == End::Done {
        TransferTone::Success
    } else {
        TransferTone::Quiet
    };
    let lead = body.chars().count();
    (body, tone, lead)
}

/// bateri arkadayken gösterilecek bildirim (037 phase-7): başlık ve gövde.
/// İptal kullanıcının kendi eylemi — bildirim yok.
fn end_notice(end: &End, host: &str, tally: &Tally<'_>) -> Option<(String, String)> {
    if let Some(reason) = failure_reason(end, host) {
        let mut chars = reason.trim_end_matches('.').chars();
        let reason = chars
            .next()
            .map(|first| first.to_uppercase().chain(chars).collect::<String>())
            .unwrap_or_default();
        return Some((
            "Upload failed".to_owned(),
            format!("{reason}. {} of {} uploaded.", tally.done, tally.items),
        ));
    }
    if *end != End::Done {
        return None;
    }
    let body = match tally.dest {
        Some(dest) => format!("to {host}:{dest}"),
        None => format!("to {host}"),
    };
    Some((format!("{} uploaded", tally.what()), body))
}

// ─── yerel ölçüm ─────────────────────────────────────────────────────────

/// Bırakılan bir öğe: yolu, adı, klasör mü, kaç dosya ve kaç bayt — ikisi
/// de **yerelde, yükleme başlamadan** (sayfa onları söylüyor, ilerleme onlara
/// göre).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Local {
    pub(crate) path: PathBuf,
    pub(crate) name: String,
    pub(crate) dir: bool,
    pub(crate) files: u64,
    pub(crate) bytes: u64,
}

/// Bir yolu ölçer. Sembolik bağ **izlenmiyor** (tar da izlemiyor, bağı bağ
/// olarak taşıyor); okunamayan alt klasör atlanıyor — tar onda zaten hata
/// verecek ve satır onu söyleyecek.
pub(crate) fn measure(path: &Path) -> std::io::Result<Local> {
    let meta = std::fs::symlink_metadata(path)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut local = Local {
        path: path.to_owned(),
        name,
        dir: meta.is_dir(),
        files: 0,
        bytes: 0,
    };
    walk(path, &meta, &mut local);
    Ok(local)
}

fn walk(path: &Path, meta: &std::fs::Metadata, into: &mut Local) {
    if meta.is_file() {
        into.files += 1;
        into.bytes += meta.len();
    } else if meta.is_dir()
        && let Ok(entries) = std::fs::read_dir(path)
    {
        for entry in entries.flatten() {
            if let Ok(meta) = entry.path().symlink_metadata() {
                walk(&entry.path(), &meta, into);
            }
        }
    }
}

// ─── sayfa ───────────────────────────────────────────────────────────────

/// Onay sayfasının metni ve düğmesi.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sheet {
    pub(crate) message: String,
    pub(crate) informative: String,
    /// Onay düğmesinin adı: `Upload`, aynı adlı dosya varsa `Replace`,
    /// aynı adlı klasör varsa `Merge`.
    pub(crate) button: &'static str,
    /// Onay düğmesi açık mı: yer yetmiyorsa, uzakta tar yoksa ya da ad
    /// güvenle gönderilemiyorsa kapalı.
    pub(crate) enabled: bool,
}

/// Onay sayfası (Kullanıcı kararı 1; metni 037 phase-7): başlık neyi
/// nereye, ilk satır boyutu ve hedefi söylüyor (`96.0 MB → /var/www/app`,
/// klasörde dosya sayısıyla), birden çok kalemde adların listesi; aynı adlı
/// öğe, boş alan ve tar bugünkü gibi. `reported`: hedef uzak kabuğun OSC 7
/// dizini mi (değilse ev dizini ve sayfa bunu söylüyor). `busy`: bir
/// yükleme sürüyor — sayfa onun durmadığını söylüyor.
///
/// **Boş alan yalnız bu damlayla karşılaştırılıyor**: kuyrukta bekleyen
/// öğelerin henüz gitmemiş baytları düşülmüyor (bilinen sınır) — ikisini
/// toplayan bir "yetmiyor" hangi öğenin yetmediğini söyleyemezdi.
pub(crate) fn sheet(
    host: &str,
    reported: bool,
    items: &[Local],
    reply: &ProbeReply,
    busy: bool,
) -> Sheet {
    let one = items.len() == 1;
    let quoted = |name: &str| format!("“{name}”");
    let bytes: u64 = items.iter().map(|item| item.bytes).sum();
    let dest = &reply.dir;
    let (message, summary) = match items {
        [item] if item.dir => (
            format!("Upload folder {} to {host}?", quoted(&item.name)),
            format!(
                "{} {}, {} → {dest}",
                item.files,
                files_word(item.files),
                format_bytes(item.bytes)
            ),
        ),
        [item] => (
            format!("Upload {} to {host}?", quoted(&item.name)),
            format!("{} → {dest}", format_bytes(item.bytes)),
        ),
        _ => (
            format!("Upload {} items to {host}?", items.len()),
            format!("{} → {dest}", format_bytes(bytes)),
        ),
    };
    let mut lines = vec![summary];
    if !reported {
        lines.push(
            "That is the home folder, because the remote shell has not reported its folder."
                .to_owned(),
        );
    }
    if !one {
        lines.extend(items.iter().map(|item| item.name.clone()));
    }
    let clashes: Vec<(usize, bool)> = reply
        .existing
        .iter()
        .copied()
        .filter(|(index, _)| *index < items.len())
        .collect();
    let merges = clashes
        .iter()
        .any(|&(index, remote_dir)| remote_dir && items[index].dir);
    match clashes[..] {
        [] => {}
        [(index, remote_dir)] => {
            let kind = if remote_dir { "folder" } else { "file" };
            let what = if items[index].dir && remote_dir {
                "same-named files are replaced, others are kept."
            } else {
                "it will be replaced."
            };
            lines.push(format!(
                "A {kind} named {} already exists there: {what}",
                quoted(&items[index].name)
            ));
        }
        _ => lines.push(format!(
            "{} items already exist there: same-named files are replaced, others are kept.",
            clashes.len()
        )),
    }
    let mut enabled = true;
    if let Some(free) = reply.free
        && free < bytes
    {
        enabled = false;
        let needs = if one {
            format!("{} needs", quoted(&items[0].name))
        } else {
            "these items need".to_owned()
        };
        lines.push(format!(
            "{host} has {} free, {needs} {}.",
            format_bytes(free),
            format_bytes(bytes)
        ));
    }
    if !reply.tar {
        enabled = false;
        lines.push(format!(
            "tar is not installed on {host}, so nothing can be sent."
        ));
    }
    if let Some(item) = items.iter().find(|item| !is_safe(&item.name)) {
        enabled = false;
        lines.push(format!(
            "{} has a backslash or a control character in its name and can't be sent safely.",
            quoted(&item.name)
        ));
    }
    if busy {
        lines.push("Added to the queue; the current upload keeps going.".to_owned());
    }
    let button = if clashes.is_empty() {
        "Upload"
    } else if merges {
        "Merge"
    } else {
        "Replace"
    };
    Sheet {
        message,
        informative: lines.join("\n"),
        button,
        enabled,
    }
}

// ─── süreç ───────────────────────────────────────────────────────────────

/// Yoklamanın hatası: sayfanın yerine açılan hata sayfasının metni.
pub(crate) fn probe_failure(host: &str, code: Option<i32>, stderr: &str) -> String {
    let last = last_line(stderr);
    match code {
        Some(NO_DIRECTORY) => format!("The remote folder on {host} can no longer be opened."),
        _ if last.is_empty() => format!(
            "ssh could not connect to {host} without asking for a password. Uploads need \
             key-based login (ssh-agent) or an open ControlMaster connection."
        ),
        _ => format!(
            "ssh could not connect to {host} without asking for a password. Uploads need \
             key-based login (ssh-agent) or an open ControlMaster connection.\n\n{last}"
        ),
    }
}

/// Metnin son boş olmayan satırı.
fn last_line(text: &str) -> &str {
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
}

/// Uzak betiği ssh ile koşturur (girdisiz): çıkış kodu, stdout, stderr.
fn run_ssh(ssh: &[String], script: &str) -> std::io::Result<(Option<i32>, String, String)> {
    let output = Command::new(&ssh[0])
        .args(&ssh[1..])
        .arg(remote_command(script))
        .stdin(Stdio::null())
        .output()?;
    Ok((
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}

/// Sayfadan önceki iş, arka plan thread'inde: yerel ölçüm ve uzak yoklama.
/// `Err` → hata sayfasının metni.
pub(crate) fn probe(
    ssh: &[String],
    host: &str,
    dir: Option<&str>,
    paths: &[String],
) -> Result<(Vec<Local>, ProbeReply), String> {
    let items: Vec<Local> = paths
        .iter()
        .filter_map(|path| measure(Path::new(path)).ok())
        .collect();
    if items.is_empty() {
        return Err("The dropped items could not be read.".to_owned());
    }
    if let Some(dir) = dir.filter(|dir| !is_safe(dir)) {
        return Err(format!(
            "The remote folder {dir} has a backslash or a control character in its path and \
             can't be used safely."
        ));
    }
    let names: Vec<String> = items.iter().map(|item| item.name.clone()).collect();
    let (code, out, err) = run_ssh(ssh, &probe_script(dir, &names))
        .map_err(|error| format!("ssh could not be started: {error}"))?;
    match parse_probe(&out) {
        Some(reply) if code == Some(0) => Ok((items, reply)),
        _ => Err(probe_failure(host, code, &err)),
    }
}

/// Bir öğenin akışının paylaşılan durumu: ana thread iptal ediyor ve
/// ilerlemeyi okuyor, akış thread'i yazıyor.
#[derive(Debug, Default)]
pub(crate) struct Shared {
    cancel: AtomicBool,
    disk_full: AtomicBool,
    bytes: AtomicU64,
    files: AtomicU64,
    /// Akıştaki iki sürecin (yerel tar, ssh) pid'i — iptal onları
    /// **öldürüyor**: akış thread'i yavaş bir bağlantıda ssh'ın girdisine
    /// yazarken bloklu kalıyor ve bayrağa hiç bakmıyor; süreç ölünce yazım
    /// `EPIPE` ile dönüyor.
    pids: Mutex<Vec<u32>>,
    /// Ana kuyrukta bekleyen ilerleme haberi var mı (en çok bir).
    pub(crate) tick_pending: AtomicBool,
}

impl Shared {
    /// Bütün süreçleri öldürür (iptal ve disk dolu).
    fn kill(&self) {
        for &pid in self
            .pids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            // SAFETY: `kill(2)` yalnız bir pid ve sinyal alıyor; pid bu
            // akışın kendi çocuğu ve **toplanmadan önce**, bu kilidin
            // altında listeden çıkıyor ([`wait_untracked`]) — toplanmış ve
            // başka bir sürece verilmiş bir pid'e sinyal gidemez.
            // audit: pid `u32`'den `pid_t`'ye; macOS'ta pid'ler pozitif `i32`.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    /// İptal ister ve süreçleri öldürür.
    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
        self.kill();
    }

    /// Geçen içerik baytı ve biten dosya sayısı.
    pub(crate) fn progress(&self) -> (u64, u64) {
        (
            self.bytes.load(Ordering::Acquire),
            self.files.load(Ordering::Acquire),
        )
    }

    fn track(&self, children: &[&Child]) {
        let mut pids = self.pids.lock().unwrap_or_else(PoisonError::into_inner);
        pids.clear();
        pids.extend(children.iter().map(|child| child.id()));
        drop(pids);
        // Başlamadan iptal edildiyse (sıra geldiğinde ⌘.) hemen öldür.
        if self.cancel.load(Ordering::Acquire) {
            self.kill();
        }
    }

    fn untrack(&self, pid: u32) {
        self.pids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|&tracked| tracked != pid);
    }
}

/// Çocuğun çıkmasını bekler, pid'ini [`Shared`]'ın listesinden çıkarır ve
/// **ancak ondan sonra** toplar (`wait`). Sıra şart: toplanan pid çekirdekte
/// hemen başka bir sürece verilebilir ve listede kalsaydı araya düşen bir
/// iptal ilgisiz bir süreci öldürürdü. `waitid(…, WNOWAIT)` çıkışı toplamadan
/// bildiriyor.
fn wait_untracked(child: &mut Child, shared: &Shared) -> std::io::Result<std::process::ExitStatus> {
    let pid = child.id();
    loop {
        // SAFETY: `siginfo_t` düz bir C yapısı, sıfır geçerli bir başlangıç;
        // `waitid` yalnız ona yazıyor. pid bu thread'in kendi çocuğu.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: yukarıdaki; `WNOWAIT` çocuğu toplamıyor, `wait` aşağıda.
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                libc::id_t::from(pid),
                &raw mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        if result == 0 || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
        {
            break;
        }
    }
    shared.untrack(pid);
    child.wait()
}

/// Bir öğenin akışının sonucu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    Cancelled,
    DiskFull,
    Failed(String),
}

/// Uzakta diskin dolduğunu söyleyen satır (GNU tar, bsdtar ve busybox
/// aynı `strerror`'ı basıyor).
const DISK_FULL: &str = "No space left on device";

/// Bir öğeyi yükler: yerelde `tar c`, uzakta `ssh … tar x`, aradaki baytlar
/// bizden geçiyor ([`TarWatcher`]). **Arka plan thread'inde**; `tick`
/// ilerlemenin haberini ana kuyruğa atıyor (en sık [`TICK`]'te bir).
///
/// İptal ya da disk dolu: iki süreç öldürülüyor ve **yazılmakta olan** dosya
/// uzakta siliniyor — tek dosyada dosyanın kendisi, klasörde yalnız o an
/// yazılan; bitenler kalıyor (Kullanıcı kararı 5, 6).
pub(crate) fn transfer(
    ssh: &[String],
    local: &Local,
    dir: &str,
    shared: &Arc<Shared>,
    tick: impl Fn(),
) -> Outcome {
    let parent = local.path.parent().unwrap_or(Path::new("/"));
    let remote = Command::new(&ssh[0])
        .args(&ssh[1..])
        .arg(remote_command(&extract_script(dir)))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut remote = match remote {
        Ok(child) => child,
        Err(error) => return Outcome::Failed(format!("ssh could not be started: {error}")),
    };
    // `./ad`: `-` ile başlayan bir ad seçenek sanılmasın. macOS'un tar'ı
    // `._ad` AppleDouble kayıtları ve öznitelik pax'ları üretmesin — uzakta
    // çöp dosya ve GNU tar'da uyarı olurlardı.
    let local_tar = Command::new("/usr/bin/tar")
        .args([
            "-c",
            "-f",
            "-",
            "--no-mac-metadata",
            "--no-xattrs",
            "--no-acls",
            "-C",
        ])
        .arg(parent)
        .arg(format!("./{}", local.name))
        .env("COPYFILE_DISABLE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut local_tar = match local_tar {
        Ok(child) => child,
        Err(error) => {
            let _ = remote.kill();
            let _ = remote.wait();
            return Outcome::Failed(format!("tar could not be started: {error}"));
        }
    };
    shared.track(&[&local_tar, &remote]);

    let remote_err = collect_stderr(remote.stderr.take(), Some(Arc::clone(shared)));
    let local_err = collect_stderr(local_tar.stderr.take(), None);
    let mut watcher = TarWatcher::default();
    if let (Some(mut source), Some(mut sink)) = (local_tar.stdout.take(), remote.stdin.take()) {
        let mut buffer = vec![0; 64 * 1024];
        let mut last = Instant::now();
        loop {
            let read = match source.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            watcher.feed(&buffer[..read]);
            if sink.write_all(&buffer[..read]).is_err() {
                break;
            }
            shared.bytes.store(watcher.bytes, Ordering::Release);
            shared.files.store(watcher.files, Ordering::Release);
            if last.elapsed() >= TICK {
                last = Instant::now();
                tick();
            }
        }
        // Girdi burada kapanıyor: uzak tar akışın sonunu görsün.
    }
    let remote_status = wait_untracked(&mut remote, shared);
    let local_status = wait_untracked(&mut local_tar, shared);
    let remote_err = remote_err.join().unwrap_or_default();
    let local_err = local_err.join().unwrap_or_default();

    let interrupted = if shared.disk_full.load(Ordering::Acquire) {
        Some(Outcome::DiskFull)
    } else if shared.cancel.load(Ordering::Acquire) {
        Some(Outcome::Cancelled)
    } else {
        None
    };
    if let Some(outcome) = interrupted {
        if let Some(partial) = watcher.current.take() {
            let _ = run_ssh(ssh, &cleanup_script(dir, &partial));
        }
        return outcome;
    }
    if !local_status.is_ok_and(|status| status.success()) {
        let line = last_line(&local_err);
        return Outcome::Failed(if line.is_empty() {
            "the local tar failed".to_owned()
        } else {
            line.to_owned()
        });
    }
    match remote_status {
        Ok(status) if status.success() => Outcome::Done,
        Ok(status) => {
            let line = last_line(&remote_err);
            Outcome::Failed(if line.is_empty() {
                format!("ssh exited with {}", status.code().unwrap_or(-1))
            } else {
                line.to_owned()
            })
        }
        Err(error) => Outcome::Failed(error.to_string()),
    }
}

/// Bir sürecin hata çıktısını ayrı bir thread'de sonuna kadar okur (boru
/// dolup süreç bloklanmasın); `disk` verildiyse "disk dolu" satırında
/// akışı hemen durdurur — GNU tar hatadan sonra akışı yutmaya devam ediyor
/// ve kalan gigabaytlar boşa giderdi.
fn collect_stderr(
    stream: Option<std::process::ChildStderr>,
    disk: Option<Arc<Shared>>,
) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let Some(stream) = stream else {
            return String::new();
        };
        let mut text = String::new();
        for line in BufReader::new(stream).lines() {
            let Ok(line) = line else { break };
            if let Some(shared) = &disk
                && line.contains(DISK_FULL)
            {
                shared.disk_full.store(true, Ordering::Release);
                shared.kill();
            }
            text.push_str(&line);
            text.push('\n');
        }
        text
    })
}

/// Pencere ve sekme başlığı (037 phase-7): yükleme akarken bugünkü başlığın
/// önünde `↑ N% · ` — alternatif ekranda (vim) dock yok ve ilerlemeyi
/// gösteren tek yer bu; akmıyorsa başlık olduğu gibi.
pub(crate) fn titled(percent: Option<u8>, title: &str) -> String {
    match percent {
        Some(percent) => format!("↑ {percent}% · {title}"),
        None => title.to_owned(),
    }
}

// ─── kuyruk ──────────────────────────────────────────────────────────────

/// Akan kalemin durdurulması bu süreden uzun sürmüşse önce sorulur (037
/// phase-7) — **tasarım sabiti, ölçüm değil**. Kısa bir yüklemeyi durdurmak
/// ucuz (yeniden bırakmak saniyeler); yarım dakikayı geçen bir yüklemenin
/// kaybı ise tek bir yanlış tıkla gitmemeli.
pub(crate) const STOP_ASK_AFTER: Duration = Duration::from_secs(30);

/// Kuyruktaki bir öğe: yerel öğe ve uzak hedef dizin.
#[derive(Clone, Debug)]
pub(crate) struct Job {
    pub(crate) local: Local,
    pub(crate) dir: String,
}

/// Bir kalemin kuyruktaki hâli.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryState {
    Waiting,
    Running,
    Done,
}

/// Kuyruğun bir kalemi. Biten kalem listede **kalıyor** (037 phase-7):
/// popover onu `✓ Uploaded` diye gösteriyor ve sonuç satırı onu sayıyor.
#[derive(Debug)]
struct Item {
    /// Kuyruklar boyunca tekil: popover'ın düğmesi kalemi bununla buluyor —
    /// sıra, biten ve çıkarılan kalemlerle kayardı.
    id: u64,
    job: Job,
    state: EntryState,
}

/// Akmakta olan kalem.
#[derive(Debug)]
struct Current {
    id: u64,
    shared: Arc<Shared>,
    /// Akışın başladığı an: durdurma sorusunun ölçütü ([`STOP_ASK_AFTER`]).
    started: Instant,
}

/// Bir sekmenin yükleme kuyruğu — **o sekmenin ssh bağlantısının**
/// (Kullanıcı kararı 7): nesli uzak oturumun komut nesli; nesil değişirse
/// (ssh kapandı) bekleyenler iptal.
#[derive(Debug)]
struct Queue {
    command: u64,
    ssh: Vec<String>,
    host: String,
    mark: HostMark,
    entries: Vec<Item>,
    current: Option<Current>,
    /// Çubuğun paydası ve biten kalemlerin baytları: tek başına durdurulan
    /// kalemin gitmeyen kısmı paydadan düşüyor, gideni biten sayılıyor —
    /// çubuk geri gitmesin.
    bytes_total: u64,
    bytes_done: u64,
    /// Hızın örnekleri: (an, kuyruğun geçen baytı).
    samples: VecDeque<(Instant, u64)>,
    /// Son ölçülen hız, bayt/sn (popover'ın satırı da onu gösteriyor).
    rate: Option<f64>,
    /// Sıradaki öğe başlamayacak: kuyruk bu sonla bitiyor.
    ending: Option<End>,
    /// Akan öğe tek başına durduruldu: `Cancelled` sonucu kuyruğu
    /// bitirmiyor, kalem listeden çıkıyor ve sıradaki başlıyor.
    skip: bool,
}

impl Queue {
    fn running_entry(&self) -> Option<(usize, &Item)> {
        let id = self.current.as_ref()?.id;
        self.entries
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.id == id)
    }

    fn waiting(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state == EntryState::Waiting)
            .count()
    }

    fn done(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.state == EntryState::Done)
            .count()
    }
}

/// Bir sekmenin yükleme durumu (ana thread): sayfa sürüyor mu, kuyruk ve
/// sonuç satırının nesli.
#[derive(Debug, Default)]
pub(crate) struct Uploads {
    /// Yoklama ya da onay sayfası sürüyor: yeni damla reddediliyor (iki sayfa
    /// üst üste açılamaz).
    asking: bool,
    queue: Option<Queue>,
    /// Kalemlerin kimlik sayacı ([`Item::id`]).
    next_id: u64,
    /// Sonuç satırının nesli: bekleme bitince yalnız hâlâ aynı sonuç
    /// gösteriliyorsa satır kalkıyor.
    serial: u64,
    /// Dock'a son yazılan satır ([`Self::shown`]).
    shown: Option<Transfer>,
    /// Farenin altındaki düğme ve listenin açıklığı (037 phase-6): satır her
    /// tazelemede yeniden doğuyor, bu ikisi her doğumda ona damgalanıyor —
    /// yoksa 200 ms'lik tazeleme fare durumunu ezerdi.
    hover: Option<TransferAction>,
    list_open: bool,
    /// Başlığa son yazılan yüzde ([`Self::title_percent_changed`]).
    titled: Option<u8>,
}

/// Kuyruk bitti: sonuç satırı, nesli ve bateri arkadaysa gösterilecek
/// bildirim (başlık, gövde).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Ended {
    pub(crate) line: Transfer,
    pub(crate) serial: u64,
    pub(crate) notice: Option<(String, String)>,
}

/// Durdurma isteğinin cevabı ([`Uploads::stop_request`]).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Stop {
    /// Sormadan dur.
    Now { id: Option<u64>, all: bool },
    /// Önce sor: `id`'li kalem otuz saniyeden uzun süredir akıyor.
    Ask(StopQuestion),
}

/// Durdurma sorusu (037 phase-7): sayfanın başlığı ve metni, hangi kalem
/// için sorulduğu (bu arada biterse sayfa kendiliğinden kapanıyor) ve
/// bütün kuyruk mu.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct StopQuestion {
    pub(crate) id: u64,
    pub(crate) all: bool,
    pub(crate) title: String,
    pub(crate) text: String,
}

/// Popover'ın bir satırının hâli ve sol sütunun ikinci satırı.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum RowStatus {
    /// Akıyor: çubuğun oranı (`0..=1`) ve `18.2 / 96.0 MB · 1.2 MB/s`.
    Running { fraction: f64, detail: String },
    /// Sırada: `Waiting · 48.5 MB`.
    Waiting(String),
    /// Bitti: `✓ Uploaded · 96.0 MB`.
    Done(String),
}

/// Popover satırının sağındaki düğme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RowAction {
    /// Akan kalemi durdur (30 sn'yi geçtiyse önce sorar).
    Cancel,
    /// Bekleyen kalemi kuyruktan çıkar (sormaz).
    Remove,
}

impl RowStatus {
    /// Satırın düğmesi: akanda `Cancel`, bekleyende `Remove`, bitende yok.
    pub(crate) fn action(&self) -> Option<RowAction> {
        match self {
            Self::Running { .. } => Some(RowAction::Cancel),
            Self::Waiting(_) => Some(RowAction::Remove),
            Self::Done(_) => None,
        }
    }
}

/// Popover'ın bir satırı: ad (klasörde `static/ · 124 files`), hâl ve
/// sönük hedef satırı (`→ /var/www/app`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ListRow {
    pub(crate) id: u64,
    pub(crate) name: String,
    pub(crate) status: RowStatus,
    pub(crate) dest: String,
}

/// "Show files (N)"in popover'ı (037 phase-7): başlık ve satırlar.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct UploadList {
    pub(crate) title: String,
    pub(crate) rows: Vec<ListRow>,
}

impl Uploads {
    /// Yeni bir damla kabul edilebilir mi.
    ///
    /// İptal edilmiş ama akan öğesi henüz bitmemiş (yarım dosyası
    /// siliniyor) bir kuyruk da hayır: onay verilen damla ona eklenemez ve
    /// sessizce düşerdi.
    pub(crate) fn can_accept(&self) -> bool {
        !self.asking
            && self
                .queue
                .as_ref()
                .is_none_or(|queue| queue.ending.is_none())
    }

    /// Yoklama ya da sayfa başladı / bitti.
    pub(crate) fn set_asking(&mut self, asking: bool) {
        self.asking = asking;
    }

    /// Bir damlanın yoklaması ya da onay sayfası sürüyor mu: durdurma
    /// sorusu o arada açılmıyor (iki sayfa üst üste açılamaz).
    pub(crate) fn asking(&self) -> bool {
        self.asking
    }

    /// Kuyruk sürüyor mu (⌘.'nin kapısı).
    pub(crate) fn active(&self) -> bool {
        self.queue.is_some()
    }

    /// Kuyruk akıyor mu: onay sayfasının "kuyruğa eklendi" satırının kapısı.
    pub(crate) fn busy(&self) -> bool {
        self.queue
            .as_ref()
            .is_some_and(|queue| queue.ending.is_none())
    }

    /// Uzak oturumun nesli (ssh'ın kapandığını anlamak için).
    pub(crate) fn command(&self) -> Option<u64> {
        self.queue.as_ref().map(|queue| queue.command)
    }

    /// Onaylanan öğeleri kuyruğun sonuna ekler; kuyruk yoksa kurar. Kuyruk
    /// **başka** bir uzak oturumunsa (yeniden bağlanıldı, eskisi hâlâ
    /// bitiyor) eski kuyruk kapanmış sayılıyor ve yenisi onun arkasından
    /// değil yerine kurulamıyor — o hâlde `false` ve damla düşüyor.
    pub(crate) fn enqueue(
        &mut self,
        command: u64,
        ssh: Vec<String>,
        host: String,
        mark: HostMark,
        jobs: Vec<Job>,
    ) -> bool {
        let queue = self.queue.get_or_insert_with(|| Queue {
            command,
            ssh,
            host,
            mark,
            entries: Vec::new(),
            current: None,
            bytes_total: 0,
            bytes_done: 0,
            samples: VecDeque::new(),
            rate: None,
            ending: None,
            skip: false,
        });
        if queue.command != command || queue.ending.is_some() {
            return false;
        }
        for job in jobs {
            self.next_id += 1;
            queue.bytes_total += job.local.bytes;
            queue.entries.push(Item {
                id: self.next_id,
                job,
                state: EntryState::Waiting,
            });
        }
        true
    }

    /// Sıradaki öğeyi `now` anında başlatır: akış thread'inin girdileri. Bir
    /// öğe zaten akıyorsa ya da kuyruk bitiyorsa `None`.
    pub(crate) fn start_next(&mut self, now: Instant) -> Option<(Vec<String>, Job, Arc<Shared>)> {
        let queue = self.queue.as_mut()?;
        if queue.current.is_some() || queue.ending.is_some() {
            return None;
        }
        let entry = queue
            .entries
            .iter_mut()
            .find(|entry| entry.state == EntryState::Waiting)?;
        entry.state = EntryState::Running;
        let shared = Arc::new(Shared::default());
        queue.current = Some(Current {
            id: entry.id,
            shared: Arc::clone(&shared),
            started: now,
        });
        Some((queue.ssh.clone(), entry.job.clone(), shared))
    }

    /// Akan kalemin kimliği (durdurma sayfasının kapanış sorusu).
    pub(crate) fn running_id(&self) -> Option<u64> {
        self.queue
            .as_ref()?
            .current
            .as_ref()
            .map(|current| current.id)
    }

    /// Durdurma isteği (037 phase-7), `now` anında: `all` bütün kuyruk (⌘.,
    /// satırın `Cancel`/`Cancel all`'ı, popover'ın `Cancel all`'ı), değilse
    /// akan kalem (popover satırının `Cancel`'ı). Tek kalemli kuyrukta ikisi
    /// aynı şey. Akan kalem [`STOP_ASK_AFTER`]'dan uzun süredir akıyorsa
    /// soru, değilse hemen; kuyruk yoksa `None`.
    pub(crate) fn stop_request(&self, all: bool, now: Instant) -> Option<Stop> {
        let queue = self.queue.as_ref()?;
        let all = all || queue.entries.len() <= 1;
        let Some(current) = &queue.current else {
            return Some(Stop::Now { id: None, all });
        };
        if now.saturating_duration_since(current.started) <= STOP_ASK_AFTER {
            return Some(Stop::Now {
                id: Some(current.id),
                all,
            });
        }
        let Some((_, entry)) = queue.running_entry() else {
            return Some(Stop::Now { id: None, all });
        };
        let local = &entry.job.local;
        let (sent, _) = current.shared.progress();
        let mut text = format!(
            "{}: {} will be lost.",
            label(local),
            format_of(sent.min(local.bytes), local.bytes)
        );
        if all {
            let waiting = queue.waiting();
            if waiting > 0 {
                // audit: kalem sayısı damla başına; `u64`'e sığar.
                let _ = write!(
                    text,
                    " {waiting} waiting {} won't be uploaded.",
                    files_word(waiting as u64)
                );
            }
            let done = queue.done();
            if done > 0 {
                let (noun, verb) = if done == 1 {
                    ("file", "stays")
                } else {
                    ("files", "stay")
                };
                let _ = write!(text, " {done} finished {noun} {verb} on {}.", queue.host);
            }
        }
        let title = if all && queue.entries.len() > 1 {
            "Stop all uploads?"
        } else {
            "Stop uploading?"
        };
        Some(Stop::Ask(StopQuestion {
            id: current.id,
            all,
            title: title.to_owned(),
            text,
        }))
    }

    /// Durdurmayı uygular. `id` sorunun (ya da isteğin) kalemi: o kalem bu
    /// arada bittiyse ve başkası akıyorsa hiçbir şey yapılmıyor — soru o
    /// kalemin kaybını söylemişti. `all` değilse yalnız o kalem durur, kuyruk
    /// sürüyor; sonucu kalem bitince ([`Self::finish`]).
    pub(crate) fn stop(&mut self, id: Option<u64>, all: bool) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        let running = queue.current.as_ref().map(|current| current.id);
        if id.is_some() && id != running {
            return None;
        }
        // Sırada bekleyen yoksa akan kalemi durdurmak kuyruğun iptali:
        // sonuç `Cancelled — 1 of 2 uploaded, …` demeli, bitenlerin `✓`'sü
        // değil (`/code-review`).
        if all || queue.entries.len() <= 1 || queue.waiting() == 0 {
            return self.cancel();
        }
        if let Some(current) = &queue.current {
            queue.skip = true;
            current.shared.cancel();
        }
        None
    }

    /// Bütün kuyruğu iptal eder: bekleyenler başlamayacak, akan öğe
    /// öldürülüyor ve yarım dosyası siliniyor; sonuç öğe bitince. Akan öğe
    /// yoksa (akış thread'i doğamadı) sonuç hemen.
    fn cancel(&mut self) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        if queue.ending.is_none() {
            queue.ending = Some(End::Cancelled);
        }
        match &queue.current {
            Some(current) => {
                current.shared.cancel();
                None
            }
            None => self.end(),
        }
    }

    /// Kuyruğu iptal edip bırakır (sekme kapanıyor): akan öğe öldürülüyor ve
    /// yarım dosyası akış thread'inde siliniyor, sonuç kimseye gösterilmiyor.
    pub(crate) fn abandon(&mut self) {
        if let Some(queue) = self.queue.take()
            && let Some(current) = &queue.current
        {
            current.shared.cancel();
        }
    }

    /// Uzak oturum kapandı: bekleyenler iptal; akan öğe kendi bağlantısıyla
    /// bitiyor. Akan öğe yoksa sonuç hemen.
    pub(crate) fn close(&mut self) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        if queue.ending.is_none() {
            queue.ending = Some(End::Closed);
        }
        if queue.current.is_none() {
            return self.end();
        }
        None
    }

    /// Akan öğe bitti; kuyruk da bittiyse sonuç. Hiçbir şey kendiliğinden
    /// yapıştırılmıyor (037 phase-7): yükleme dakikalar sürebilir ve yol o
    /// an açık olan vim'e ya da mysql'e yazılırdı — sonuç satırı nereye
    /// gittiğini söylüyor.
    pub(crate) fn finish(&mut self, outcome: Outcome) -> Option<Ended> {
        let queue = self.queue.as_mut()?;
        let current = queue.current.take()?;
        let (bytes, _) = current.shared.progress();
        let index = queue
            .entries
            .iter()
            .position(|entry| entry.id == current.id)?;
        match outcome {
            Outcome::Done => {
                let entry = &mut queue.entries[index];
                entry.state = EntryState::Done;
                queue.bytes_done += entry.job.local.bytes;
            }
            // Tek başına durdurulan kalem listeden çıkıyor; gideni biten
            // sayılıyor, gitmeyeni paydadan düşüyor (çubuk geri gitmesin).
            Outcome::Cancelled if std::mem::take(&mut queue.skip) && queue.ending.is_none() => {
                let entry = queue.entries.remove(index);
                let sent = bytes.min(entry.job.local.bytes);
                queue.bytes_done += sent;
                queue.bytes_total -= entry.job.local.bytes - sent;
                // Bu arada bekleyenler çıkarıldıysa kuyruğun sonu bir iptal.
                if queue.waiting() == 0 {
                    queue.ending = Some(End::Cancelled);
                }
            }
            Outcome::Cancelled => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending.get_or_insert(End::Cancelled);
            }
            Outcome::DiskFull => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending = Some(End::DiskFull);
            }
            Outcome::Failed(reason) => {
                queue.entries[index].state = EntryState::Waiting;
                queue.bytes_done += bytes;
                queue.ending = Some(End::Failed(reason));
            }
        }
        if queue.waiting() == 0 || queue.ending.is_some() {
            self.end()
        } else {
            None
        }
    }

    /// Kuyruğu kapatır ve sonuç satırını verir.
    fn end(&mut self) -> Option<Ended> {
        let queue = self.queue.take()?;
        self.serial += 1;
        // ssh kapandı ama akan kalem kendi bağlantısıyla bitti ve bekleyen
        // yoktu: her şey vardı, sonuç bir başarı (`/code-review`).
        let all_done = queue
            .entries
            .iter()
            .all(|entry| entry.state == EntryState::Done);
        let end = match queue.ending {
            Some(End::Closed) if all_done => End::Done,
            ending => ending.unwrap_or(End::Done),
        };
        let tally = Tally::of(&queue.entries);
        let (body, tone, lead) = end_line(&end, &queue.host, &tally);
        let notice = end_notice(&end, &queue.host, &tally);
        Some(Ended {
            line: Transfer {
                host: queue.host,
                mark: queue.mark,
                body,
                tone,
                lead,
                controls: TransferControls::default(),
                progress: None,
            },
            serial: self.serial,
            notice,
        })
    }

    /// Dock'a son yazılan durum satırı — farenin düğme sorusunun girdisi
    /// (`bt_core::transfer_button_at` çizimle aynı yerleşimi okuyor).
    pub(crate) fn shown(&self) -> Option<&Transfer> {
        self.shown.as_ref()
    }

    /// [`Self::shown`]'ı yazar. Düğmesiz satır (sonuç ya da hiç) fare
    /// durumunu da düşürüyor: altında düğme kalmadı.
    pub(crate) fn set_shown(&mut self, transfer: Option<Transfer>) {
        if transfer.as_ref().is_none_or(|t| t.controls.items == 0) {
            self.hover = None;
            self.list_open = false;
        }
        self.shown = transfer;
    }

    /// Farenin altındaki düğme değişti mi; değiştiyse damgalı satır —
    /// yazılacak olan (çağıran oturuma verir). Aynı düğmede kalan hareket
    /// `None`: kare istenmiyor.
    pub(crate) fn set_hover(&mut self, hover: Option<TransferAction>) -> Option<Transfer> {
        // Düğmesiz satırda fare hiçbir düğmenin üstünde değil.
        let buttons = self.shown.as_ref().is_some_and(|t| t.controls.items > 0);
        let hover = hover.filter(|_| buttons);
        if self.hover == hover {
            return None;
        }
        self.hover = hover;
        self.restamp()
    }

    /// Farenin altındaki düğme (imlecin kararı).
    pub(crate) fn hover(&self) -> Option<TransferAction> {
        self.hover
    }

    /// Liste açıldı/kapandı; değiştiyse damgalı satır.
    pub(crate) fn set_list_open(&mut self, open: bool) -> Option<Transfer> {
        if self.list_open == open {
            return None;
        }
        self.list_open = open;
        self.restamp()
    }

    /// Gösterilen satırın düğme durumunu yeniler; düğmesiz satırda `None`.
    fn restamp(&self) -> Option<Transfer> {
        let mut shown = self.shown.clone()?;
        if shown.controls.items == 0 {
            return None;
        }
        shown.controls.hover = self.hover;
        shown.controls.list_open = self.list_open;
        Some(shown)
    }

    /// Kuyruğun geçen ve toplam baytı (Dock simgesinin çubuğu); kuyruk yoksa
    /// `None`.
    pub(crate) fn totals(&self) -> Option<(u64, u64)> {
        let queue = self.queue.as_ref()?;
        let running = queue
            .current
            .as_ref()
            .map_or(0, |current| current.shared.progress().0);
        Some((queue.bytes_done + running, queue.bytes_total))
    }

    /// Başlığın öneki için yüzde (037 phase-7): bir kalem akarken bütün
    /// kuyruğun baytlarına göre, aşağı yuvarlı; akmıyorsa ya da iptal
    /// ediliyorsa `None`. ssh kapandıysa akan kalem kendi bağlantısıyla
    /// bitiyor ve önek onunla kalıyor.
    pub(crate) fn percent(&self) -> Option<u8> {
        let queue = self.queue.as_ref()?;
        if queue.ending == Some(End::Cancelled) || queue.current.is_none() {
            return None;
        }
        let (sent, total) = self.totals()?;
        if total == 0 {
            return Some(0);
        }
        // audit: `sent ≤ total` kırpılıyor; oran 0..=100, `u8`'e sığar.
        Some((sent.min(total) as f64 / total as f64 * 100.0).floor() as u8)
    }

    /// Başlığın yüzdesi son yazılandan farklı mı; farklıysa yenisini
    /// saklar — başlık yüzde başına en çok bir kez yazılıyor.
    pub(crate) fn title_percent_changed(&mut self) -> bool {
        let percent = self.percent();
        if self.titled == percent {
            return false;
        }
        self.titled = percent;
        true
    }

    /// Başlığa en son yazılan yüzde ([`titled`]'ın girdisi).
    pub(crate) fn title_percent(&self) -> Option<u8> {
        self.titled
    }

    /// Bekleme bitti: sonuç satırı hâlâ bu nesilse ve yeni bir kuyruk
    /// başlamadıysa satır kalkıyor.
    pub(crate) fn linger_over(&self, serial: u64) -> bool {
        self.queue.is_none() && self.serial == serial
    }

    /// Popover'ın içeriği (037 phase-7): bütün kalemler — biten, akan ve
    /// bekleyen —, sırayla. Kuyruk yoksa ya da bitiyorsa `None`: popover
    /// kapanmalı.
    pub(crate) fn list(&self) -> Option<UploadList> {
        let queue = self.queue.as_ref()?;
        if queue.ending.is_some() {
            return None;
        }
        let rows = queue
            .entries
            .iter()
            .map(|entry| {
                let local = &entry.job.local;
                let name = if local.dir {
                    format!(
                        "{}/ · {} {}",
                        local.name,
                        local.files,
                        files_word(local.files)
                    )
                } else {
                    local.name.clone()
                };
                let status = match entry.state {
                    EntryState::Done => {
                        RowStatus::Done(format!("✓ Uploaded · {}", format_bytes(local.bytes)))
                    }
                    EntryState::Waiting => {
                        RowStatus::Waiting(format!("Waiting · {}", format_bytes(local.bytes)))
                    }
                    EntryState::Running => {
                        let sent = queue
                            .current
                            .as_ref()
                            .map_or(0, |current| current.shared.progress().0)
                            .min(local.bytes);
                        let mut detail = format_pair(sent, local.bytes);
                        if let Some(rate) = queue.rate {
                            detail.push_str(" · ");
                            detail.push_str(&format_rate(rate));
                        }
                        let fraction = if local.bytes == 0 {
                            0.0
                        } else {
                            sent as f64 / local.bytes as f64
                        };
                        RowStatus::Running { fraction, detail }
                    }
                };
                ListRow {
                    id: entry.id,
                    name,
                    status,
                    dest: format!("→ {}", entry.job.dir),
                }
            })
            .collect();
        Some(UploadList {
            title: format!("Uploading to {}", queue.host),
            rows,
        })
    }

    /// Bekleyen `id`'li kalemi kuyruktan çıkarır (popover'ın `Remove`'u);
    /// akan ya da biten kalemde no-op.
    pub(crate) fn remove(&mut self, id: u64) {
        let Some(queue) = &mut self.queue else {
            return;
        };
        if let Some(index) = queue
            .entries
            .iter()
            .position(|entry| entry.id == id && entry.state == EntryState::Waiting)
        {
            let entry = queue.entries.remove(index);
            queue.bytes_total -= entry.job.local.bytes;
        }
    }

    /// Dock'un durum satırı, `now` anında; kuyruk yoksa `None`.
    pub(crate) fn status(&mut self, now: Instant) -> Option<Transfer> {
        let queue = self.queue.as_mut()?;
        let current = queue.current.as_ref()?;
        let (bytes, files) = current.shared.progress();
        let sent = queue.bytes_done + bytes;
        queue.samples.push_back((now, sent));
        while queue
            .samples
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > SPEED_WINDOW)
        {
            queue.samples.pop_front();
        }
        let rate = match (queue.samples.front(), queue.samples.back()) {
            (Some(&(first, from)), Some(&(last, to)))
                if last.duration_since(first) >= Duration::from_millis(500) && to > from =>
            {
                Some((to - from) as f64 / last.duration_since(first).as_secs_f64())
            }
            _ => None,
        };
        queue.rate = rate;
        let (index, entry) = queue.running_entry()?;
        let local = &entry.job.local;
        let items = queue.entries.len();
        let mut body = String::from("↑ ");
        if items > 1 {
            let _ = write!(body, "{} of {} · ", index + 1, items);
        }
        body.push_str(&local.name);
        if local.dir {
            body.push('/');
        }
        // Klasörün dosya sayısı adın yanında: kalemin kendi bilgisi, baytlar
        // ise kuyruğun (kullanıcının onayladığı demo, 037 phase-7 sonrası).
        if local.dir {
            let _ = write!(
                body,
                " · {} of {} {}",
                files.min(local.files),
                local.files,
                files_word(local.files)
            );
        }
        // Bayt, hız ve kalan süre **bütün kuyruğun**: çubuk da kuyruğa göre
        // doluyor ve metin başka bir oranı söyleseydi ikisi çelişirdi.
        let total = queue.bytes_total;
        let shown = sent.min(total);
        body.push_str("  ");
        body.push_str(&format_pair(shown, total));
        if let Some(rate) = rate {
            body.push_str(" · ");
            body.push_str(&format_rate(rate));
            let left = total.saturating_sub(shown) as f64 / rate;
            // audit: kalan süre saniyeye yuvarlanıyor; sonsuz/NaN yok (`rate > 0`).
            body.push_str(" · ");
            body.push_str(&format_duration(left.ceil() as u64));
        }
        let progress = if queue.bytes_total == 0 {
            0
        } else {
            // audit: `sent ≤ bytes_total` kırpılıyor; oran onbinde, u16'ya sığar.
            (sent.min(queue.bytes_total) as f64 / queue.bytes_total as f64 * 10_000.0) as u16
        };
        Some(Transfer {
            host: queue.host.clone(),
            mark: queue.mark,
            body,
            controls: TransferControls {
                // Popover'ın gösterdiği sayı ([`Self::list`]): biten, akan ve
                // bekleyen kalemler.
                // audit: kuyruk damla başına öğe; `u16`'ya kırpılıyor.
                items: u16::try_from(items).unwrap_or(u16::MAX),
                list_open: self.list_open,
                hover: self.hover,
            },
            progress: Some(progress),
            ..Transfer::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(argv: &[&str]) -> Vec<String> {
        argv.iter().map(|&arg| arg.to_owned()).collect()
    }

    fn ssh_target(argv: &[&str]) -> RemoteTarget {
        RemoteTarget {
            host: "prod".to_owned(),
            kind: RemoteKind::Ssh,
            argv: words(argv),
            line: String::new(),
        }
    }

    const OURS: [&str; 6] = ["ssh", "-T", "-o", "BatchMode=yes", "-o", "ControlMaster=no"];

    fn with_ours(rest: &[&str]) -> Vec<String> {
        words(&OURS).into_iter().chain(words(rest)).collect()
    }

    #[test]
    fn the_upload_keeps_the_connection_options_and_drops_the_rest() {
        let target = ssh_target(&[
            "ssh", "-p", "2222", "-l", "deploy", "-J", "jump", "-i", "k", "prod",
        ]);
        assert_eq!(
            ssh_argv(&target),
            with_ours(&[
                "-p", "2222", "-l", "deploy", "-J", "jump", "-i", "k", "prod"
            ])
        );
        // tty, gürültü ve uzak komut düşüyor; bizimkiler başta, çünkü ssh bir
        // anahtarın ilk değerini alıyor.
        let target = ssh_target(&[
            "ssh",
            "-tv",
            "-o",
            "RequestTTY=force",
            "prod",
            "tmux",
            "attach",
        ]);
        assert_eq!(
            ssh_argv(&target),
            with_ours(&["-o", "RequestTTY=force", "prod"])
        );
        // Bitişik değer ve kümedeki korunan bayrak; hedeften sonraki seçenek.
        let target = ssh_target(&["/opt/ssh", "-Ap2222", "prod", "-i", "k"]);
        let mut expected = with_ours(&["-A", "-p", "2222", "-i", "k", "prod"]);
        expected[0] = "/opt/ssh".to_owned();
        assert_eq!(ssh_argv(&target), expected);
        // `--`'dan sonrası hedef.
        let target = ssh_target(&["ssh", "-C", "--", "deploy@prod"]);
        assert_eq!(ssh_argv(&target), with_ours(&["-C", "deploy@prod"]));
    }

    #[test]
    fn mosh_uploads_through_plain_ssh_to_the_host() {
        let target = RemoteTarget {
            host: "deploy@prod".to_owned(),
            kind: RemoteKind::Mosh,
            argv: words(&["mosh", "--ssh=ssh -p 2222", "deploy@prod"]),
            line: String::new(),
        };
        assert_eq!(ssh_argv(&target), with_ours(&["deploy@prod"]));
    }

    #[test]
    fn single_quotes_never_produce_a_backslash() {
        assert_eq!(sq("plain"), "'plain'");
        assert_eq!(sq("it's"), "'it'\"'\"'s'");
        assert!(!remote_command(&probe_script(Some("/a'b"), &["c'd".into()])).contains('\\'));
    }

    /// Betiği **iki katmanda** koşturur: giriş kabuğu (`shell -c`) ve
    /// onun açtığı `sh -c` — ssh'ın uzakta yaptığının yerel eşi.
    fn run_remote(shell: &str, script: &str, cwd: &Path) -> (Option<i32>, String) {
        let output = Command::new(shell)
            .arg("-c")
            .arg(remote_command(script))
            .current_dir(cwd)
            .output()
            .expect("kabuk koşmadı");
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stdout).into_owned(),
        )
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bt-upload-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("geçici dizin");
        dir
    }

    #[test]
    fn the_probe_runs_under_every_login_shell_and_reports_what_is_there() {
        let dir = scratch("probe");
        let target = dir.join("it's \"odd\" $HOME");
        std::fs::create_dir_all(target.join("static")).unwrap();
        std::fs::write(target.join("a b.txt"), "x").unwrap();
        let names: Vec<String> = ["a b.txt", "static", "new", "-dash"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let script = probe_script(Some(target.to_str().unwrap()), &names);
        for shell in ["/bin/sh", "/bin/bash", "/bin/zsh"] {
            let (code, out) = run_remote(shell, &format!("echo noise; {script}"), &dir);
            assert_eq!(code, Some(0), "{shell}: {out}");
            let reply = parse_probe(&out).unwrap_or_else(|| panic!("{shell}: {out}"));
            // `pwd` sembolik bağı çözmüyor olabilir (`/var` → `/private/var`).
            assert!(
                reply.dir.ends_with("it's \"odd\" $HOME"),
                "{shell}: {}",
                reply.dir
            );
            assert!(reply.tar, "{shell}");
            assert!(reply.free.is_some_and(|free| free > 0), "{shell}");
            assert_eq!(reply.existing, [(0, false), (1, true)], "{shell}");
        }
        // Dizin yoksa betik ayrı bir kodla çıkıyor.
        let script = probe_script(Some(dir.join("gone").to_str().unwrap()), &names);
        let (code, _) = run_remote("/bin/sh", &script, &dir);
        assert_eq!(code, Some(NO_DIRECTORY));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_probe_reply_skips_noise_and_reads_df_from_the_right() {
        let out = "Welcome!\nBT-UPLOAD\n/srv/app\nBT-DF /dev/disk 1 k 100 12000 50% /Volumes/My Disk\nBT-TAR\nBT-E1\nBT-D1\nBT-E3\n";
        assert_eq!(
            parse_probe(out),
            Some(ProbeReply {
                dir: "/srv/app".into(),
                free: Some(12_000 * 1024),
                tar: true,
                existing: vec![(1, true), (3, false)],
            })
        );
        assert_eq!(parse_probe("no mark\n"), None);
        // `df` hiçbir şey basmadıysa sıradaki işaret yerinde okunuyor.
        let reply = parse_probe("BT-UPLOAD\n/srv\nBT-TAR\nBT-E0\n").unwrap();
        assert_eq!((reply.free, reply.tar), (None, true));
        assert_eq!(reply.existing, [(0, false)]);
        assert_eq!(df_available("garbage"), None);
    }

    #[test]
    fn names_with_a_backslash_or_control_character_are_refused() {
        assert!(is_safe("it's \"fine\" $x"));
        assert!(!is_safe("a\\b"));
        assert!(!is_safe("a\nb"));
    }

    fn tar_of(dir: &Path, name: &str) -> Vec<u8> {
        let output = Command::new("/usr/bin/tar")
            .args([
                "-c",
                "-f",
                "-",
                "--no-mac-metadata",
                "--no-xattrs",
                "--no-acls",
                "-C",
            ])
            .arg(dir)
            .arg(format!("./{name}"))
            .env("COPYFILE_DISABLE", "1")
            .output()
            .expect("tar koşmadı");
        assert!(output.status.success());
        output.stdout
    }

    #[test]
    fn the_watcher_counts_files_and_content_bytes_through_pax_headers() {
        let dir = scratch("watch");
        let tree = dir.join("static");
        std::fs::create_dir_all(tree.join("css")).unwrap();
        std::fs::write(tree.join("app.js"), vec![b'a'; 1000]).unwrap();
        std::fs::write(tree.join("empty"), b"").unwrap();
        // Uzun ve ASCII dışı ad: bsdtar önüne bir pax başlığı koyuyor.
        let long = format!("ç{}.txt", "x".repeat(150));
        std::fs::write(tree.join("css").join(&long), vec![b'b'; 700]).unwrap();
        std::os::unix::fs::symlink("app.js", tree.join("link")).unwrap();
        let stream = tar_of(&dir, "static");
        let local = measure(&tree).unwrap();
        assert_eq!((local.files, local.bytes, local.dir), (3, 1700, true));

        // Tek yudumda ve tek tek baytlarla aynı sonuç.
        for chunk in [stream.len(), 1, 511, 513] {
            let mut watcher = TarWatcher::default();
            for part in stream.chunks(chunk) {
                watcher.feed(part);
            }
            assert_eq!((watcher.files, watcher.bytes), (3, 1700), "parça {chunk}");
            assert_eq!(watcher.current, None);
        }
        // Uzun adlı dosyanın ortasında kesilen akış: yarım dosya pax'ın
        // yolunu taşıyor.
        let at = stream
            .windows(700)
            .position(|window| window.iter().all(|&b| b == b'b'))
            .unwrap();
        let mut watcher = TarWatcher::default();
        watcher.feed(&stream[..at + 10]);
        assert_eq!(
            watcher.current.as_deref(),
            Some(format!("./static/css/{long}").as_str())
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tar_sizes_read_octal_and_base_256() {
        assert_eq!(tar_size(b"00000001750\0"), 1000);
        assert_eq!(tar_size(b"     1750 \0\0"), 1000);
        let mut big = [0u8; 12];
        big[0] = 0x80;
        big[11] = 0x01;
        big[10] = 0x02;
        assert_eq!(tar_size(&big), 0x0201);
        assert_eq!(
            pax_path(b"12 path=a/b\n20 mtime=1234567.5\n"),
            Some("a/b".into())
        );
        assert_eq!(pax_path(b"garbage"), None);
    }

    #[test]
    fn sizes_rates_and_durations_read_like_finder() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(18_200_000), "18.2 MB");
        assert_eq!(format_bytes(3_400_000_000), "3.4 GB");
        assert_eq!(format_pair(18_200_000, 44_600_000), "18.2 / 44.6 MB");
        assert_eq!(format_pair(500_000, 44_600_000), "0.5 / 44.6 MB");
        assert_eq!(format_pair(10, 900), "10 / 900 B");
        assert_eq!(format_rate(1_200_000.0), "1.2 MB/s");
        assert_eq!(format_rate(800.0), "800 B/s");
        assert_eq!(format_duration(22), "22s");
        assert_eq!(format_duration(65), "1m 05s");
        assert_eq!(format_duration(3720), "1h 02m");
    }

    fn entries(items: &[(&str, bool, &str, EntryState)]) -> Vec<Item> {
        items
            .iter()
            .enumerate()
            .map(|(id, &(name, dir, at, state))| Item {
                id: id as u64,
                job: Job {
                    local: local(name, dir, 1, 1),
                    dir: at.into(),
                },
                state,
            })
            .collect()
    }

    #[test]
    fn the_end_line_says_what_went_where() {
        use EntryState::{Done, Waiting};
        let line = |end: &End, items: &[(&str, bool, &str, EntryState)]| {
            end_line(end, "prod-web-1", &Tally::of(&entries(items)))
        };
        let one = [("backup.tar.gz", false, "/var/www/app", Done)];
        assert_eq!(
            line(&End::Done, &one),
            (
                "✓ backup.tar.gz → /var/www/app".into(),
                TransferTone::Success,
                30
            )
        );
        let folder = [("static", true, "/var/www/app", Done)];
        assert_eq!(line(&End::Done, &folder).0, "✓ static/ → /var/www/app");
        let three = [
            ("a", false, "/var/www/app", Done),
            ("b", false, "/var/www/app", Done),
            ("c", true, "/var/www/app", Done),
        ];
        assert_eq!(line(&End::Done, &three).0, "✓ 3 files → /var/www/app");
        let apart = [
            ("a", false, "/var/www/app", Done),
            ("b", false, "/srv", Done),
        ];
        assert_eq!(line(&End::Done, &apart).0, "✓ 2 files uploaded");

        let (body, tone, _) = line(&End::Cancelled, &[("a", false, "/srv", Waiting)]);
        assert_eq!(
            (body.as_str(), tone),
            ("Cancelled — partial file removed", TransferTone::Quiet)
        );
        let two = [("a", false, "/srv", Done), ("b", false, "/srv", Waiting)];
        assert_eq!(
            line(&End::Cancelled, &two).0,
            "Cancelled — 1 of 2 uploaded, partial file removed"
        );

        let none = [
            ("a", false, "/srv", Waiting),
            ("b", false, "/srv", Waiting),
            ("c", false, "/srv", Waiting),
        ];
        let (body, tone, lead) = line(&End::DiskFull, &none);
        assert_eq!(body, "Failed — disk full on prod-web-1 · 0 of 3 uploaded");
        assert_eq!(tone, TransferTone::Error);
        assert_eq!(
            body.chars().take(lead).collect::<String>(),
            "Failed — disk full on prod-web-1",
            "yalnız hata metni kırmızı"
        );
        assert_eq!(
            line(&End::Closed, &two).0,
            "Failed — connection to prod-web-1 lost · 1 of 2 uploaded"
        );
        assert_eq!(
            line(&End::Failed("tar: Permission denied".into()), &one).0,
            "Failed — tar: Permission denied · 1 of 1 uploaded"
        );
    }

    #[test]
    fn a_notice_is_sent_for_success_and_failure_but_not_for_a_cancel() {
        use EntryState::{Done, Waiting};
        let notice = |end: &End, items: &[(&str, bool, &str, EntryState)]| {
            end_notice(end, "prod-web-1", &Tally::of(&entries(items)))
        };
        let three = [
            ("a", false, "/var/www/app", Done),
            ("b", false, "/var/www/app", Done),
            ("c", false, "/var/www/app", Done),
        ];
        assert_eq!(
            notice(&End::Done, &three),
            Some((
                "3 files uploaded".into(),
                "to prod-web-1:/var/www/app".into()
            ))
        );
        let one = [("backup.tar.gz", false, "/srv", Done)];
        assert_eq!(
            notice(&End::Done, &one).map(|(title, _)| title).as_deref(),
            Some("backup.tar.gz uploaded")
        );
        let apart = [
            ("a", false, "/var/www/app", Done),
            ("b", false, "/srv", Done),
        ];
        assert_eq!(
            notice(&End::Done, &apart).map(|(_, body)| body).as_deref(),
            Some("to prod-web-1")
        );
        let none = [
            ("a", false, "/srv", Waiting),
            ("b", false, "/srv", Waiting),
            ("c", false, "/srv", Waiting),
        ];
        assert_eq!(
            notice(&End::DiskFull, &none),
            Some((
                "Upload failed".into(),
                "Disk full on prod-web-1. 0 of 3 uploaded.".into()
            ))
        );
        assert_eq!(
            notice(&End::Closed, &none).map(|(_, body)| body).as_deref(),
            Some("Connection to prod-web-1 lost. 0 of 3 uploaded.")
        );
        assert_eq!(notice(&End::Cancelled, &none), None, "iptal kullanıcının");
    }

    #[test]
    fn the_title_carries_the_percent_only_while_uploading() {
        assert_eq!(titled(Some(64), "⇄ prod-web-1"), "↑ 64% · ⇄ prod-web-1");
        assert_eq!(titled(None, "⇄ prod-web-1"), "⇄ prod-web-1");

        let mut uploads = queue_of(vec![job("a", false, 1, 100), job("b", false, 1, 100)]);
        assert_eq!(uploads.percent(), None, "henüz akmıyor");
        let (_, _, shared) = uploads.start_next(Instant::now()).unwrap();
        assert!(uploads.title_percent_changed(), "0% ilk kez");
        assert_eq!(uploads.title_percent(), Some(0));
        shared.bytes.store(1, Ordering::Release);
        assert!(!uploads.title_percent_changed(), "yüzde başına bir kez");
        shared.bytes.store(129, Ordering::Release);
        assert!(uploads.title_percent_changed());
        assert_eq!(uploads.title_percent(), Some(64));
        let _ = uploads.stop(None, true);
        assert!(
            uploads.title_percent_changed(),
            "iptal ediliyor: önek kalkar"
        );
        assert_eq!(uploads.title_percent(), None);
    }

    fn local(name: &str, dir: bool, files: u64, bytes: u64) -> Local {
        Local {
            path: PathBuf::from("/Users/me").join(name),
            name: name.to_owned(),
            dir,
            files,
            bytes,
        }
    }

    fn reply(existing: Vec<(usize, bool)>, free: Option<u64>) -> ProbeReply {
        ProbeReply {
            dir: "/var/www/app".into(),
            free,
            tar: true,
            existing,
        }
    }

    #[test]
    fn the_sheet_names_the_target_and_the_button_follows_what_exists() {
        let file = [local("backup.tar.gz", false, 1, 96_000_000)];
        let fresh = sheet("prod-web-1", true, &file, &reply(vec![], None), false);
        assert_eq!(fresh.message, "Upload “backup.tar.gz” to prod-web-1?");
        assert_eq!(fresh.informative, "96.0 MB → /var/www/app");
        assert_eq!((fresh.button, fresh.enabled), ("Upload", true));

        // Yükleme sürerken gelen damla: yükleme durmuyor.
        let busy = sheet("prod", true, &file, &reply(vec![], None), true);
        assert_eq!(
            busy.informative,
            "96.0 MB → /var/www/app\nAdded to the queue; the current upload keeps going."
        );

        let replace = sheet("prod", true, &file, &reply(vec![(0, false)], None), false);
        assert_eq!(replace.button, "Replace");
        assert!(
            replace.informative.contains(
                "A file named “backup.tar.gz” already exists there: it will be replaced."
            )
        );

        let folder = [local("static", true, 124, 38_200_000)];
        let merge = sheet(
            "prod",
            false,
            &folder,
            &reply(vec![(0, true)], Some(12_000_000)),
            false,
        );
        assert_eq!(merge.message, "Upload folder “static” to prod?");
        assert_eq!(merge.button, "Merge");
        assert!(!merge.enabled, "yer yetmiyor");
        assert_eq!(
            merge.informative,
            "124 files, 38.2 MB → /var/www/app\n\
             That is the home folder, because the remote shell has not reported its folder.\n\
             A folder named “static” already exists there: same-named files are replaced, \
             others are kept.\n\
             prod has 12.0 MB free, “static” needs 38.2 MB."
        );

        // Tek damlada birden çok öğe tek sayfada: toplam, hedef ve adlar.
        let three = [
            local("dump.sql", false, 1, 32_300_000),
            local("logs.tar", false, 1, 20_100_000),
            local("README.md", false, 1, 100_000),
        ];
        let many = sheet("prod-web-1", true, &three, &reply(vec![], None), false);
        assert_eq!(many.message, "Upload 3 items to prod-web-1?");
        assert_eq!(
            many.informative,
            "52.5 MB → /var/www/app\ndump.sql\nlogs.tar\nREADME.md"
        );

        let two = [local("a", false, 1, 10), local("b", true, 2, 20)];
        let mut no_tar = reply(vec![(0, false), (1, true)], None);
        no_tar.tar = false;
        let refused = sheet("prod", true, &two, &no_tar, false);
        assert_eq!(refused.message, "Upload 2 items to prod?");
        assert!(!refused.enabled);
        assert!(refused.informative.contains("2 items already exist there"));
        assert!(refused.informative.contains("tar is not installed on prod"));

        let odd = [local("a\\b", false, 1, 1)];
        assert!(!sheet("prod", true, &odd, &reply(vec![], None), false).enabled);
    }

    fn job(name: &str, dir: bool, files: u64, bytes: u64) -> Job {
        Job {
            local: local(name, dir, files, bytes),
            dir: "/srv".into(),
        }
    }

    fn queue_of(jobs: Vec<Job>) -> Uploads {
        let mut uploads = Uploads::default();
        assert!(uploads.enqueue(
            7,
            words(&["ssh", "prod"]),
            "prod".into(),
            HostMark::Production,
            jobs
        ));
        uploads
    }

    #[test]
    fn the_queue_runs_in_order_and_pastes_nothing() {
        let mut uploads = queue_of(vec![
            job("backup.tar.gz", false, 1, 44_600_000),
            job("static", true, 124, 1_000_000),
        ]);
        let start = Instant::now();
        let (_, first, shared) = uploads.start_next(start).expect("ilk öğe");
        assert_eq!(first.local.name, "backup.tar.gz");
        assert!(uploads.start_next(start).is_none(), "sıralı, paralel değil");

        shared.bytes.store(18_200_000, Ordering::Release);
        let status = uploads.status(start).expect("satır");
        assert_eq!(status.body, "↑ 1 of 2 · backup.tar.gz  18.2 / 45.6 MB");
        assert_eq!(
            status.controls,
            TransferControls {
                items: 2,
                ..TransferControls::default()
            },
        );
        assert_eq!(status.mark, HostMark::Production);
        // Çubuk bütün kuyruğun baytlarına göre: 18.2 / (44.6 + 1.0).
        assert_eq!(status.progress, Some(3_991));
        shared.bytes.store(19_400_000, Ordering::Release);
        let status = uploads.status(start + Duration::from_secs(1)).unwrap();
        assert_eq!(
            status.body,
            "↑ 1 of 2 · backup.tar.gz  19.4 / 45.6 MB · 1.2 MB/s · 22s"
        );

        // Biten kalem kuyruğu bitirmiyor ve hiçbir yol yapıştırılmıyor —
        // `finish`'in cevabında yapıştırılacak bir şey yok, yalnız sonuç.
        assert_eq!(uploads.finish(Outcome::Done), None);
        let (_, second, shared) = uploads.start_next(start).expect("ikinci öğe");
        assert_eq!(second.local.name, "static");
        shared.files.store(57, Ordering::Release);
        let status = uploads.status(start + Duration::from_secs(2)).unwrap();
        assert!(
            status
                .body
                .starts_with("↑ 2 of 2 · static/ · 57 of 124 files  "),
            "{}",
            status.body
        );
        assert!(status.body.contains(" / 45.6 MB"), "{}", status.body);
        // Biten kalem sayıda kalıyor: "Show files (2)".
        assert_eq!(status.controls.items, 2);

        let ended = uploads.finish(Outcome::Done).expect("kuyruk bitti");
        assert_eq!(ended.line.body, "✓ 2 files → /srv");
        assert_eq!(ended.line.tone, TransferTone::Success);
        assert_eq!(
            (ended.line.progress, ended.line.controls),
            (None, TransferControls::default()),
            "sonuç satırı düğmesiz"
        );
        assert_eq!(
            ended.notice,
            Some(("2 files uploaded".into(), "to prod:/srv".into()))
        );
        assert!(!uploads.active());
        assert!(uploads.linger_over(ended.serial));
    }

    #[test]
    fn cancelling_keeps_the_count_and_removes_the_partial_file() {
        let mut uploads = queue_of(vec![job("a", false, 1, 1_000), job("b", false, 1, 1)]);
        let (_, _, shared) = uploads.start_next(Instant::now()).unwrap();
        assert_eq!(uploads.stop(None, true), None, "sonuç öğe bitince");
        assert!(shared.cancel.load(Ordering::Acquire));
        let ended = uploads.finish(Outcome::Cancelled).expect("bitti");
        assert_eq!(
            ended.line.body,
            "Cancelled — 0 of 2 uploaded, partial file removed"
        );
        assert_eq!(ended.notice, None, "iptalde bildirim yok");
    }

    #[test]
    fn a_closed_connection_finishes_the_current_item_and_says_it_failed() {
        let mut uploads = queue_of(vec![job("a", false, 1, 1), job("b", false, 1, 1)]);
        let _ = uploads.start_next(Instant::now()).unwrap();
        assert_eq!(
            uploads.close(),
            None,
            "akan öğe kendi bağlantısıyla bitiyor"
        );
        let ended = uploads.finish(Outcome::Done).expect("bitti");
        assert_eq!(
            ended.line.body,
            "Failed — connection to prod lost · 1 of 2 uploaded"
        );
        assert_eq!(ended.line.tone, TransferTone::Error);
        // Bekleyen yoktu ve akan kalem kendi bağlantısıyla bitti: başarı.
        let mut uploads = queue_of(vec![job("a", false, 1, 1)]);
        let _ = uploads.start_next(Instant::now()).unwrap();
        assert_eq!(uploads.close(), None);
        let ended = uploads.finish(Outcome::Done).expect("bitti");
        assert_eq!(ended.line.body, "✓ a → /srv");
        assert_eq!(ended.line.tone, TransferTone::Success);
        // Akan öğe yoksa sonuç hemen.
        let mut uploads = queue_of(vec![job("a", false, 1, 1)]);
        assert!(uploads.close().is_some());
        // Kapanmış bir kuyruğa yeni damla girmiyor.
        let mut uploads = queue_of(vec![job("a", false, 1, 1)]);
        assert!(!uploads.enqueue(
            8,
            vec![],
            "prod".into(),
            HostMark::None,
            vec![job("b", false, 1, 1)]
        ));
    }

    #[test]
    fn the_popover_lists_every_item_with_its_state_destination_and_button() {
        let mut uploads = queue_of(vec![
            job("backup.tar.gz", false, 1, 96_000_000),
            job("static", true, 124, 38_200_000),
            job("photos.zip", false, 1, 48_500_000),
        ]);
        let start = Instant::now();
        let _ = uploads.start_next(start).unwrap();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let (_, _, shared) = uploads.start_next(start).unwrap();
        shared.bytes.store(18_200_000, Ordering::Release);
        let _ = uploads.status(start);
        shared.bytes.store(19_400_000, Ordering::Release);
        let _ = uploads.status(start + Duration::from_secs(1));

        let list = uploads.list().expect("liste");
        assert_eq!(list.title, "Uploading to prod");
        let rows: Vec<(&str, &str)> = list
            .rows
            .iter()
            .map(|row| (row.name.as_str(), row.dest.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                ("backup.tar.gz", "→ /srv"),
                ("static/ · 124 files", "→ /srv"),
                ("photos.zip", "→ /srv")
            ]
        );
        assert_eq!(
            list.rows[0].status,
            RowStatus::Done("✓ Uploaded · 96.0 MB".into())
        );
        let RowStatus::Running { fraction, detail } = &list.rows[1].status else {
            panic!("akan kalem");
        };
        assert_eq!(detail, "19.4 / 38.2 MB · 1.2 MB/s");
        assert!((fraction - 19.4 / 38.2).abs() < 1e-9);
        assert_eq!(
            list.rows[2].status,
            RowStatus::Waiting("Waiting · 48.5 MB".into())
        );
        let actions: Vec<_> = list.rows.iter().map(|row| row.status.action()).collect();
        assert_eq!(
            actions,
            [None, Some(RowAction::Cancel), Some(RowAction::Remove)],
            "bitende düğme yok"
        );

        // `Remove` bekleyeni sormadan çıkarıyor ve sayı düşüyor.
        uploads.remove(list.rows[2].id);
        assert_eq!(uploads.list().unwrap().rows.len(), 2);
        assert_eq!(uploads.status(start).unwrap().controls.items, 2);
        // Akan ya da biten kalemde `remove` no-op (akanı `stop` durduruyor).
        uploads.remove(list.rows[0].id);
        uploads.remove(list.rows[1].id);
        assert_eq!(uploads.list().unwrap().rows.len(), 2);
    }

    #[test]
    fn stopping_one_item_takes_it_off_the_list_and_the_queue_goes_on() {
        let mut uploads = queue_of(vec![
            job("a", false, 1, 10),
            job("b", false, 1, 20),
            job("c", false, 1, 30),
        ]);
        let now = Instant::now();
        let _ = uploads.start_next(now).unwrap();
        let first = uploads.running_id();
        assert_eq!(uploads.stop(first, false), None);
        assert_eq!(uploads.finish(Outcome::Cancelled), None, "kuyruk sürüyor");
        let (_, next, _) = uploads.start_next(now).expect("sıradaki");
        assert_eq!(next.local.name, "b");
        assert_eq!(
            uploads.list().unwrap().rows.len(),
            2,
            "durdurulan listeden çıktı"
        );
        assert_eq!(uploads.finish(Outcome::Done), None);
        let _ = uploads.start_next(now).unwrap();
        let ended = uploads.finish(Outcome::Done).unwrap();
        assert_eq!(ended.line.body, "✓ 2 files → /srv");

        // Soru bu arada biten kalem içindiyse durdurma başkasına dokunmuyor.
        let mut uploads = queue_of(vec![job("a", false, 1, 10), job("b", false, 1, 20)]);
        let _ = uploads.start_next(now).unwrap();
        let asked = uploads.running_id();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let (_, _, shared) = uploads.start_next(now).unwrap();
        assert_eq!(uploads.stop(asked, true), None);
        assert!(!shared.cancel.load(Ordering::Acquire), "yeni kalem sürüyor");

        // Bekleyen kalmadıysa akan kalemi durdurmak kuyruğun iptali: biten
        // kalem `✓` demiyor.
        let mut uploads = queue_of(vec![job("a", false, 1, 10), job("b", false, 1, 20)]);
        let _ = uploads.start_next(now).unwrap();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let _ = uploads.start_next(now).unwrap();
        let last = uploads.running_id();
        assert_eq!(uploads.stop(last, false), None);
        let ended = uploads.finish(Outcome::Cancelled).unwrap();
        assert_eq!(
            ended.line.body,
            "Cancelled — 1 of 2 uploaded, partial file removed"
        );
        assert_eq!(ended.notice, None);

        // Tek kalemde satırın durdurması kuyruğun iptali.
        let mut uploads = queue_of(vec![job("a", false, 1, 10)]);
        let _ = uploads.start_next(now).unwrap();
        let only = uploads.running_id();
        assert_eq!(uploads.stop(only, false), None);
        assert_eq!(
            uploads.finish(Outcome::Cancelled).unwrap().line.body,
            "Cancelled — partial file removed"
        );
    }

    #[test]
    fn a_long_upload_asks_before_it_stops() {
        let mut uploads = queue_of(vec![
            job("done.txt", false, 1, 10),
            job("backup.tar.gz", false, 1, 96_000_000),
            job("photos.zip", false, 1, 48_500_000),
        ]);
        let start = Instant::now();
        let _ = uploads.start_next(start).unwrap();
        assert_eq!(uploads.finish(Outcome::Done), None);
        let (_, _, shared) = uploads.start_next(start).unwrap();
        let id = uploads.running_id();
        shared.bytes.store(48_000_000, Ordering::Release);

        // Eşik tasarım sabiti; altında soru yok.
        let short = start + STOP_ASK_AFTER;
        assert_eq!(
            uploads.stop_request(true, short),
            Some(Stop::Now { id, all: true })
        );
        let long = start + STOP_ASK_AFTER + Duration::from_secs(1);
        assert_eq!(
            uploads.stop_request(true, long),
            Some(Stop::Ask(StopQuestion {
                id: id.unwrap(),
                all: true,
                title: "Stop all uploads?".into(),
                text: "backup.tar.gz: 48.0 of 96.0 MB will be lost. 1 waiting file won't \
                       be uploaded. 1 finished file stays on prod."
                    .into(),
            }))
        );
        // Popover satırının `Cancel`'ı yalnız o kalem: bekleyen ve biten
        // sayılmıyor.
        let Some(Stop::Ask(one)) = uploads.stop_request(false, long) else {
            panic!("soru");
        };
        assert_eq!(one.title, "Stop uploading?");
        assert_eq!(one.text, "backup.tar.gz: 48.0 of 96.0 MB will be lost.");
        assert!(!one.all);
        // Bekleyen kalemi çıkarmak hiç sormuyor (`remove`, `stop_request`
        // değil) — `the_popover_lists_every_item…`.
    }

    #[test]
    fn a_stopping_queue_refuses_new_drops_and_a_closing_tab_drops_the_queue() {
        let mut uploads = queue_of(vec![job("a", false, 1, 10)]);
        let (_, _, shared) = uploads.start_next(Instant::now()).unwrap();
        assert!(uploads.can_accept());
        assert!(uploads.busy());
        assert_eq!(uploads.stop(None, true), None);
        assert!(!uploads.can_accept(), "yarım dosya siliniyor");
        assert_eq!(uploads.list(), None, "popover kapanıyor");
        uploads.abandon();
        assert!(!uploads.active());
        assert!(shared.cancel.load(Ordering::Acquire));
        assert!(uploads.can_accept());
    }

    #[test]
    fn a_new_result_outlives_the_old_linger() {
        let mut uploads = queue_of(vec![job("a", false, 1, 1)]);
        let _ = uploads.start_next(Instant::now()).unwrap();
        let first = uploads.finish(Outcome::Done).unwrap().serial;
        assert!(uploads.enqueue(
            7,
            vec![],
            "prod".into(),
            HostMark::None,
            vec![job("b", false, 1, 1)]
        ));
        assert!(!uploads.linger_over(first), "yeni kuyruk sürüyor");
        let _ = uploads.start_next(Instant::now()).unwrap();
        let second = uploads.finish(Outcome::Done).unwrap().serial;
        assert!(
            !uploads.linger_over(first),
            "eski bekleme yeni sonucu silmesin"
        );
        assert!(uploads.linger_over(second));
    }

    #[test]
    fn the_pointer_state_survives_the_refresh_and_changes_only_on_edges() {
        let mut uploads = queue_of(vec![job("a", false, 1, 10), job("b", false, 1, 10)]);
        let now = Instant::now();
        let (_, _, _shared) = uploads.start_next(now).expect("ilk öğe");
        let status = uploads.status(now).expect("satır");
        uploads.set_shown(Some(status));
        let hovered = uploads
            .set_hover(Some(TransferAction::Cancel))
            .expect("değişti");
        assert_eq!(hovered.controls.hover, Some(TransferAction::Cancel));
        uploads.set_shown(Some(hovered));
        assert_eq!(
            uploads.set_hover(Some(TransferAction::Cancel)),
            None,
            "aynı düğmede kalan hareket kare istemiyor"
        );
        // 200 ms'lik tazeleme fare durumunu ezmiyor.
        let status = uploads.status(now).expect("satır");
        assert_eq!(status.controls.hover, Some(TransferAction::Cancel));
        let open = uploads.set_list_open(true).expect("değişti");
        assert!(open.controls.list_open);
        // Düğmesiz satır (sonuç) fare durumunu düşürüyor.
        uploads.set_shown(Some(Transfer::default()));
        assert_eq!(uploads.hover(), None);
        assert_eq!(
            uploads.set_hover(Some(TransferAction::List)),
            None,
            "düğme yok"
        );
        assert_eq!(uploads.hover(), None);
    }

    #[test]
    fn every_glyph_of_the_row_is_one_the_atlas_checks() {
        // `bt-atlas` bu karakterleri küçük sınıfta soruyor
        // (`the_upload_row_has_no_box_in_the_small_class`); satırın her
        // dizgesi o sözlüğün içinde kalmalı.
        use EntryState::{Done, Waiting};
        let two = entries(&[("a", false, "/srv", Done), ("b", true, "/x", Waiting)]);
        let same = entries(&[("a", false, "/srv", Done)]);
        let mut texts: Vec<String> = [End::Done, End::Cancelled, End::DiskFull, End::Closed]
            .iter()
            .flat_map(|end| {
                [
                    end_line(end, "h", &Tally::of(&two)).0,
                    end_line(end, "h", &Tally::of(&same)).0,
                ]
            })
            .collect();
        texts.push("↑ 1 of 2 · a/ · 1 of 2 files  1.0 / 2.0 MB · 1.0 MB/s · 1s".to_owned());
        for text in texts {
            for ch in text.chars().filter(|ch| !ch.is_ascii()) {
                assert!(bt_core::UPLOAD_GLYPHS.contains(&ch), "'{ch}' in {text:?}");
            }
        }
    }

    #[test]
    fn a_folder_travels_through_the_stream_and_arrives_whole() {
        // ssh'ın yerine yerel kabuk: `sh -c "<uzak komut>"` — uzakta koşacak
        // betiğin ve akışın ta kendisi, bağlantısız.
        let root = scratch("transfer");
        let source = root.join("src");
        let target = root.join("it's here");
        std::fs::create_dir_all(source.join("static/css")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(source.join("static/app.js"), vec![b'a'; 5000]).unwrap();
        std::fs::write(source.join("static/css/site.css"), b"body{}").unwrap();
        let item = measure(&source.join("static")).unwrap();
        let shared = Arc::new(Shared::default());
        let ticks = std::cell::Cell::new(0);
        let outcome = transfer(
            &words(&["/bin/sh", "-c"]),
            &item,
            target.to_str().unwrap(),
            &shared,
            || ticks.set(ticks.get() + 1),
        );
        assert_eq!(outcome, Outcome::Done);
        assert_eq!(shared.progress(), (5006, 2));
        assert_eq!(
            std::fs::read(target.join("static/css/site.css")).unwrap(),
            b"body{}"
        );
        assert_eq!(
            std::fs::read(target.join("static/app.js")).unwrap().len(),
            5000
        );
        // Uzakta tar hata verirse satırı sonuç oluyor.
        let outcome = transfer(
            &words(&["/bin/sh", "-c"]),
            &item,
            root.join("missing").to_str().unwrap(),
            &Arc::new(Shared::default()),
            || {},
        );
        assert!(matches!(outcome, Outcome::Failed(_)), "{outcome:?}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
