//! Bir pencerenin kabuğunun dışında ön planda koşan iş var mı, varsa adı ne
//! (028, kapatma onayının girdisi).
//!
//! **Yetkili süreç tablosu, OSC 133 değil** (`.tasks/028-kapatma-onayi/
//! discussion.md` → Karar 1): safha entegrasyonsuz kabukta (`integration =
//! "off"`, bash, fish) hiç yok, `exec bash` gibi bir geçişte kalıcı olarak
//! `Running`'de takılıyor ve programın adını söylemiyor. Terminalin ön plan
//! süreç grubu ise her kabukta aynı soruyu cevaplıyor: grup kabuğun kendi
//! grubu değilse kabuğun dışında bir iş ön planda.
//!
//! **Ön plan grubu kabuğun `e_tpgid`'inden**, PTY çocuğununkinden değil:
//! süresiz oturumda çocuk `login(1)` ve root'a ait, yani `PROC_PIDTBSDINFO`
//! onda sıfır bayt dönüyor (ölçüldü, discussion.md → Muhakeme). Kabuk
//! kullanıcının ve aynı çağrı onda `ps`'in TPGID sütununu veriyor. Ön plan
//! grubunun üyelerine yalnız kısa bilgi (`PROC_PIDT_SHORTBSDINFO`) soruluyor,
//! çünkü o root'a ait süreçte de (`sudo` grubu) çalışıyor.
//!
//! Adlar grubun **yapraklarından**: lider bir sarmalayıcı olabiliyor (lider
//! `bash`, program onun torunu `claude`; context.md'deki üçüncü satır).
//!
//! İki yarı: saf karar ([`foreground`], girdisi bir [`ProcessTable`]) ve
//! arayüzün `libc` gövdesi ([`Libproc`]). Karar sahte tabloyla, gövde gerçek
//! bir PTY'yle sınanıyor — sahte tablo login'in root olduğunu göremezdi.
//!
//! **İkinci tüketici: uzak oturum** (036, [`remote`]). Aynı ön plan grubu,
//! ters yön: kapatma sorusu grubun **yapraklarını** adlandırıyor, uzak oturum
//! grubun **en üstteki** ssh/mosh sürecini arıyor (`ssh -J`'in `ssh -W`
//! çocuğu jump host'u verirdi) ve onun argümanlarını okuyor
//! (`KERN_PROCARGS2`, yalnız aday adlı üyeler için). Yoklama `C` kenarında
//! ve kararsızsa sonraki çıktı kenarında (`window::RemoteProbe`); bilinen
//! sınırları `.tasks/036-ssh-uzak-oturum/discussion.md` → Karar 2: ssh'ı
//! sonradan başlatan sarmalayıcı betik ilk yoklamada "yerel" kilitleniyor,
//! `exec ssh` `C` üretmiyor, `~^Z` göstergeyi `fg`'ye kadar kaldırıyor.
//! Cevap host'tan fazlası (037 Karar 1, [`Target`]): aynı yere ikinci bir
//! kapı açacak argv de aynı yürüyüşten çıkıyor.
//!
//! **Bilinen sınırlar** (Karar 7): arka plan işleri (`sleep 100 &`) ön planda
//! değil ve sayılmıyor; kabuğun kendi içinde koşan iş (yerleşik döngü, `read`
//! bekleyen bir fonksiyon) ayrı bir grup açmıyor ve boşta görünüyor; `exec
//! vim` kabuğun pid'ini ve grubunu devraldığı için o da boşta.

use std::ffi::{c_int, c_void};

use bt_core::RemoteKind;

/// Kabuğun PTY çocuğuna göre yeri — kabuğu doğuran taraf biliyor ve
/// doğumda kaydediyor (`window::TerminalWindow::start_session`), ad
/// karşılaştırmasıyla tahmin edilmiyor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShellParent {
    /// Çocuk `login(1)`, kabuk onun çocuğu: süresiz oturumun iki yolu da
    /// (`child::login_command` ve alacritty'nin macOS yolu).
    Login,
    /// Çocuk kabuğun kendisi: süreli koşunun sabit betikleri ve gerçek PTY
    /// sınaması (login root izni istiyor, sınamada doğurulamıyor).
    Direct,
}

/// Ön planda ne var.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Foreground {
    /// Kabuk ön planda — ya da sorulacak bir kabuk kalmadı.
    Idle,
    /// Kabuğun dışında bir iş ön planda; adları pid sırasıyla, tekrarsız.
    /// **Boş vektör adsız demek**: tablo okunamadı ama koşuyor sayıldı.
    Running(Vec<String>),
}

/// Kabuğun kendi grubu ve terminalinin ön plan grubu — ikisi tek çağrıdan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Groups {
    pub(crate) own: u32,
    pub(crate) foreground: u32,
}

/// Uzak oturum yoklamasının cevabı ([`remote`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Probe {
    /// Kabuk hâlâ ön planda ya da çatalladığı çocuk henüz `exec` etmedi:
    /// sonraki çıktı kenarında yeniden sorulur.
    Undecided,
    /// Ön planda ssh/mosh yok, etkileşimli değil ya da tablo okunamadı.
    Local,
    /// Etkileşimli bir ssh/mosh ve hedefi ([`Target`]).
    Remote(Target),
}

/// Yoklamanın bulduğu uzak hedef (037 Karar 1): gösterilecek host ve aynı
/// yere ikinci bir kapı açacak argv. Kaçırılmış satırı `window::probe_remote`
/// üretiyor (`quote::command_line`), yani burada ne kabuk ne kaçırma var.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Target {
    /// Kullanıcının yazdığı gibi; `ssh://` şeması ve port atılmış.
    pub(crate) host: String,
    pub(crate) kind: RemoteKind,
    /// Yeniden koşturulacak argv ([`ssh_target`], [`mosh_argv`]).
    pub(crate) argv: Vec<String>,
}

/// Kararın süreç tablosundan sorduğu altı şey.
///
/// Listeler `Vec`, `Option` değil: macOS'un listeleme çağrıları "süreç yok"
/// ile "çocuğu yok"u ayırmıyor (ölçüldü: var olmayan pid'e sıfır), yani
/// `None` taşıyacak bir bilgi yok. Kabuğun canlılığının tek tanığı bu yüzden
/// okuyucu thread'i (`Session::reader_alive`).
pub(crate) trait ProcessTable {
    fn children(&self, pid: u32) -> Vec<u32>;
    /// Yalnız kabuğa sorulur (aynı kullanıcı): uzun bilgi root'ta okunmuyor.
    fn groups(&self, shell: u32) -> Option<Groups>;
    fn members(&self, group: u32) -> Vec<u32>;
    fn parent(&self, pid: u32) -> Option<u32>;
    fn name(&self, pid: u32) -> Option<String>;
    /// Sürecin argv'si; okunamıyorsa `None`. Yalnız aday adlı üyelere
    /// sorulur ([`remote`]).
    fn args(&self, pid: u32) -> Option<Vec<String>>;
}

/// Kararın kendisi: `child` PTY'nin çocuğunun pid'i.
///
/// Başarısızlık kolları (R1.5) yanlışın yönüne göre: çocuksuz `login`
/// (sekme doğar doğmaz ⌘W) boşta, çünkü kapatılacak bir iş yok; kabuğun
/// grubu okunamazsa **adsız koşuyor**, çünkü sistematik bir kırılma sessiz
/// "hiç sormuyor" değil görünür "hep soruyor" olmalı.
pub(crate) fn foreground(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Foreground {
    // `login` tek bir kabuk çatallıyor; henüz yoksa kapatılacak iş de yok.
    let Some(shell) = shell_pid(parent, child, table) else {
        return Foreground::Idle;
    };
    // Sıfır grup "terminalin ön planı yok" demek: kabuğun kontrol terminali
    // okunmuyor ve o da okunamayan tablonun kolu — `members(0)` bir grubun
    // değil çekirdeğin cevabı olurdu.
    let Some(groups) = table.groups(shell).filter(|groups| groups.foreground != 0) else {
        return Foreground::Running(Vec::new());
    };
    if groups.foreground == groups.own {
        return Foreground::Idle;
    }
    Foreground::Running(names(groups.foreground, table))
}

/// Kabuğun pid'i: doğrudan yolda çocuğun kendisi, `login` yolunda onun tek
/// çocuğu (henüz yoksa `None`).
fn shell_pid(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Option<u32> {
    match parent {
        ShellParent::Direct => Some(child),
        ShellParent::Login => table.children(child).first().copied(),
    }
}

/// Ön planda uzak bir oturum var mı (036 Karar 2, 3).
///
/// Başarısızlığın dili [`foreground`]'ınkinin **tersi**: okunamayan tablo
/// `Local`. Orada güvenli yön "hep sor"du, burada göstergenin **olmaması** —
/// ve kararsız sayılsaydı sistematik bir okuma hatası komut boyunca her
/// çıktı kenarında bir yoklama doğururdu.
///
/// `Undecided` yalnız iki hâlde: kabuğun grubu hâlâ ön planda (`C` fork'tan
/// önce basılıyor) ya da hiçbir ssh/mosh tanınmadı ve grubun bir üyesi
/// kabuğun adını taşıyor (çatallanmış, henüz `exec` etmemiş çocuk). Bedeli
/// Karar 2'de adıyla: kabuğun kendi içinde koşan döngü ya da `zsh betik`
/// komut boyunca kararsız kalıyor ve çıktı kenarı başına (ana kuyruk turu
/// başına en çok bir) yoklama doğuruyor.
pub(crate) fn remote(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Probe {
    let Some(shell) = shell_pid(parent, child, table) else {
        return Probe::Local;
    };
    let Some(groups) = table.groups(shell).filter(|groups| groups.foreground != 0) else {
        return Probe::Local;
    };
    if groups.foreground == groups.own {
        return Probe::Undecided;
    }
    let mut members = table.members(groups.foreground);
    if members.is_empty() {
        return Probe::Local;
    }
    members.sort_unstable();
    let names: Vec<Option<String>> = members.iter().map(|&pid| table.name(pid)).collect();
    // Tanınan üyeler ve hedefleri; argv yalnız aday adlılara soruluyor.
    let recognized: Vec<(u32, Option<Target>)> = members
        .iter()
        .zip(&names)
        .filter_map(|(&pid, name)| {
            let name = name.as_deref()?;
            if !is_candidate(name) {
                return None;
            }
            let args = table.args(pid)?;
            Some((pid, remote_target(name, &args)?))
        })
        .collect();
    let is_recognized = |pid: u32| recognized.iter().any(|&(member, _)| member == pid);
    // Grubun en üstteki tanınan süreçleri — atası grupta tanınan bir süreç
    // olan üye bir alt adım (`ssh -J`'in `ssh -W` çocuğu, mosh'un bootstrap
    // ssh'ı). Yürüyüş grubun boyuyla sınırlı: bozuk bir tablo (kendi
    // ebeveyni olan süreç) döngü kuramasın.
    let top = recognized.iter().filter(|(pid, _)| {
        let mut ancestor = table.parent(*pid);
        for _ in 0..members.len() {
            let Some(up) = ancestor.filter(|up| members.contains(up)) else {
                return true;
            };
            if is_recognized(up) {
                return false;
            }
            ancestor = table.parent(up);
        }
        true
    });
    // Birden çok tepe varsa (boru hattı) etkileşimli olan kazanıyor:
    // `ssh backup cat dump | ssh prod`'da ilki yerel cevabı dayatmamalı.
    let mut found = false;
    for (_, target) in top {
        found = true;
        if let Some(target) = target {
            return Probe::Remote(target.clone());
        }
    }
    if found {
        return Probe::Local;
    }
    // Hiçbir şey tanınmadı ama bir üye hâlâ kabuğun adını taşıyor:
    // çatallanmış, henüz `exec` etmemiş çocuk — boru hattının öbür yanı
    // (`ssh prod | tee log`'da `tee`) önce `exec` etmiş olabilir.
    let shell_name = table.name(shell);
    if shell_name.is_some() && names.contains(&shell_name) {
        return Probe::Undecided;
    }
    Probe::Local
}

/// Argv'si okunmaya değer ad: ssh, mosh'un istemcisi ve mosh betiğini koşan
/// Perl (`/usr/bin/perl` `perl5.NN`'e `exec` ediyor, yani ad önekle).
fn is_candidate(name: &str) -> bool {
    name == "ssh" || name == "mosh-client" || name.starts_with("perl")
}

/// Tanınan bir sürecin hedefi: dıştaki `None` "tanınmadı", içteki `None`
/// "tanındı ama etkileşimli bir uzak oturum değil".
fn remote_target(name: &str, args: &[String]) -> Option<Option<Target>> {
    let rest = args.get(1..).unwrap_or_default();
    if name == "ssh" {
        // argv[0] sürecin verdiği gibi (`ssh`, `/usr/bin/ssh`): takma adla
        // (`alias s=ssh`) yazılan komut da süreçte `ssh`.
        let program = args.first().map_or("ssh", String::as_str);
        return Some(ssh_target(program, rest));
    }
    if name == "mosh-client" {
        return Some(mosh_client_target(rest));
    }
    // Yorumlayıcı: ilk seçenek-olmayan argüman betiğin yolu.
    let script = rest.iter().position(|arg| !arg.starts_with('-'))?;
    let base = rest[script].rsplit('/').next().unwrap_or_default();
    let script_args = &rest[script + 1..];
    (base == "mosh").then(|| {
        mosh_target(script_args).map(|host| Target {
            host,
            kind: RemoteKind::Mosh,
            argv: mosh_argv(script_args),
        })
    })
}

/// mosh'un yeniden koşturma argv'si: `mosh` + betiğin argümanları, olduğu
/// gibi (037 Karar 6). Yerel yönlendirme mosh'ta yok, ayıklanacak bir şey de.
fn mosh_argv<S: AsRef<str>>(args: &[S]) -> Vec<String> {
    std::iter::once("mosh")
        .chain(args.iter().map(AsRef::as_ref))
        .map(str::to_owned)
        .collect()
}

/// ssh'ın değer alan kısa seçenekleri (`ssh(1)`'in SYNOPSIS'i).
const SSH_VALUED: &str = "BbcDEeFIiJLlmOoPpQRSWw";
/// Varlığı oturumu etkileşimsiz yapan seçenekler: tünel (`-N`), yönlendirme
/// (`-W`), denetim (`-O`), sorgu (`-Q`, `-G`, `-V`) ve pty'siz (`-T`).
const SSH_NON_INTERACTIVE: &str = "NWOQGVT";
/// Yeniden koşturmada **düşen** seçenekler (037 Karar 6): yerel
/// yönlendirmeler (`-L`, `-R`, `-D`, değerleriyle) ikinci oturumda aynı yerel
/// portu bağlamaya çalışıp uyarı basar ya da `ExitOnForwardFailure`'da hiç
/// bağlanmaz; `-M` ikinci bir ControlMaster açar; `-f` arka plana düşer.
/// `-o LocalForward=…` biçimi ayıklanmıyor (bilinen sınır).
const SSH_NOT_REPEATED: &str = "LRDMf";

/// ssh argv'si (argv[0] `program`, `args` sonrası) → etkileşimli oturumun
/// hedefi (036 Karar 3) ve yeniden koşturma argv'si (037 Karar 6).
///
/// Hedeften sonra bir komut varsa oturum `-t` olmadan etkileşimli değil:
/// `ssh prod uptime` bir saniyelik komut ve dock'u kısıp açmak tam da
/// kullanıcının reddettiği sıçrama olurdu.
///
/// argv aynı yürüyüşten ([`SshSession::options`]'ın `kept`'i), ikinci bir
/// ayrıştırıcıdan değil: [`SSH_NOT_REPEATED`] düşüyor, kalan her şey
/// sırasıyla — hedef ve `-t`'li uzak komut dahil.
fn ssh_target(program: &str, args: &[String]) -> Option<Target> {
    let mut session = SshSession::default();
    let mut argv = vec![program.to_owned()];
    let (index, terminated) = session.options(args, 0, &mut argv);
    let target = args.get(index)?;
    argv.push(target.clone());
    // OpenSSH hedeften sonra seçenekleri **yeniden** ayrıştırıyor
    // (`ssh prod -p 2222`); `--` ile bitmişse ayrıştırmıyor.
    let rest = if terminated {
        index + 1
    } else {
        session.options(args, index + 1, &mut argv).0
    };
    let command = rest < args.len();
    if session.quiet || (command && !session.tty) {
        return None;
    }
    argv.extend(args[rest..].iter().cloned());
    Some(Target {
        host: ssh_host(target),
        kind: RemoteKind::Ssh,
        argv,
    })
}

/// ssh seçeneklerinin etkileşim hakkında söyledikleri.
#[derive(Default)]
struct SshSession {
    /// `-t` ya da `RequestTTY=yes|force`.
    tty: bool,
    /// Etkileşimsiz bir kip: [`SSH_NON_INTERACTIVE`], `RequestTTY=no` ya da
    /// `SessionType=none|subsystem`.
    quiet: bool,
}

impl SshSession {
    /// `args[index..]`'teki seçenek kümesini okur; ilk seçenek-olmayan
    /// argümanın indeksini ve `--` ile bitip bitmediğini döndürür.
    ///
    /// Okuduğu seçenekleri yeniden koşturma için `kept`'e yazıyor (037
    /// Karar 6): [`SSH_NOT_REPEATED`]'in bayrağı kümeden düşüyor, değer
    /// alanın değeri de (bitişik ya da ayrı argüman); bayrağı kalmayan küme
    /// bütünüyle düşüyor (`-fM` → yok, `-vL 1:x:1` → `-v`).
    fn options(
        &mut self,
        args: &[String],
        mut index: usize,
        kept: &mut Vec<String>,
    ) -> (usize, bool) {
        while let Some(arg) = args.get(index) {
            if arg == "--" {
                kept.push(arg.clone());
                return (index + 1, true);
            }
            let Some(cluster) = arg.strip_prefix('-').filter(|rest| !rest.is_empty()) else {
                break;
            };
            index += 1;
            let mut flags = String::from("-");
            let mut separate = None;
            for (at, flag) in cluster.char_indices() {
                if SSH_NON_INTERACTIVE.contains(flag) {
                    self.quiet = true;
                }
                if flag == 't' {
                    self.tty = true;
                }
                let repeated = !SSH_NOT_REPEATED.contains(flag);
                if repeated {
                    flags.push(flag);
                }
                if SSH_VALUED.contains(flag) {
                    // Değer ya kümenin kalanı (`-p22`) ya sonraki argüman.
                    let attached = &cluster[at + flag.len_utf8()..];
                    let value = if attached.is_empty() {
                        index += 1;
                        let value = args.get(index - 1);
                        if repeated {
                            separate = value.cloned();
                        }
                        value.map(String::as_str)
                    } else {
                        if repeated {
                            flags.push_str(attached);
                        }
                        Some(attached)
                    };
                    if flag == 'o'
                        && let Some(value) = value
                    {
                        self.config(value);
                    }
                    break;
                }
            }
            if flags.len() > 1 {
                kept.push(flags);
            }
            kept.extend(separate);
        }
        (index, false)
    }

    /// `-o Anahtar=değer` (ya da boşlukla): etkileşimi değiştiren iki anahtar.
    fn config(&mut self, option: &str) {
        let (key, value) = option
            .split_once(['=', ' ', '\t'])
            .map_or((option, ""), |(key, value)| (key, value.trim()));
        if key.eq_ignore_ascii_case("RequestTTY") {
            if value.eq_ignore_ascii_case("yes") || value.eq_ignore_ascii_case("force") {
                self.tty = true;
            } else if value.eq_ignore_ascii_case("no") {
                self.quiet = true;
            }
        } else if key.eq_ignore_ascii_case("SessionType")
            && (value.eq_ignore_ascii_case("none") || value.eq_ignore_ascii_case("subsystem"))
        {
            self.quiet = true;
        }
    }
}

/// Hedef yazıldığı gibi; yalnız `ssh://` biçiminde şema ve port atılıyor.
fn ssh_host(target: &str) -> String {
    let Some(rest) = target.strip_prefix("ssh://") else {
        return target.to_owned();
    };
    let rest = rest.split('/').next().unwrap_or_default();
    let (user, host) = match rest.rsplit_once('@') {
        Some((user, host)) => (Some(user), host),
        None => (None, rest),
    };
    let host = match host.strip_prefix('[') {
        // IPv6 köşeli parantezde; port kapanıştan sonra.
        Some(inner) => inner.split(']').next().unwrap_or_default(),
        None => host.split(':').next().unwrap_or_default(),
    };
    match user {
        Some(user) => format!("{user}@{host}"),
        None => host.to_owned(),
    }
}

/// mosh betiğinin ayrı argümanla değer alan seçenekleri (`--ssh=…` gibi
/// eşittirli biçim zaten tek argüman).
const MOSH_VALUED: [&str; 9] = [
    "-p",
    "--port",
    "--ssh",
    "--server",
    "--client",
    "--predict",
    "--family",
    "--bind-server",
    "--experimental-remote-ip",
];

/// mosh betiğinin argv'si (betiğin yolundan sonrası) → hedef: ilk
/// seçenek-olmayan argüman. Uzak komut mosh'ta da etkileşimli bir
/// terminalde koşuyor, yani onu elemiyor.
fn mosh_target<S: AsRef<str>>(args: &[S]) -> Option<String> {
    let mut words = args.iter().map(AsRef::as_ref);
    while let Some(word) = words.next() {
        if word == "--" {
            return words.next().map(str::to_owned);
        }
        if !word.starts_with('-') {
            return Some(word.to_owned());
        }
        if MOSH_VALUED.contains(&word) {
            words.next();
        }
    }
    None
}

/// `mosh-client`'ın `-#`'i: betik ona kendi komut satırını tek argümanda
/// veriyor (`"-# {argv} |"`), yani hedef o satırın mosh ayrıştırmasından.
///
/// Yeniden koşturma argv'si de o satırdan: `mosh` + boşlukla bölünmüş
/// sözcükleri (037 Karar 6). **Bilinen sınır:** betik satırı tırnaksız
/// birleştiriyor, yani boşluklu bir değer (`--ssh="ssh -i k"`) geri
/// kurulamıyor ve bölünmüş hâliyle yazılıyor — host'un 036'daki sınırıyla
/// aynı kök.
fn mosh_client_target(args: &[String]) -> Option<Target> {
    let at = args.iter().position(|arg| arg.starts_with("-#"))?;
    let mut line = args[at].strip_prefix("-#").unwrap_or_default().trim();
    if line.is_empty() {
        line = args.get(at + 1)?.trim();
    }
    let line = line.strip_suffix('|').unwrap_or(line);
    // Satır tırnaksız birleştirilmiş: `--ssh="ssh -i ~/.ssh/k"` sözcüklere
    // bölünüyor ve değeri seçenek-olmayan bir sözcük bırakıyor. Host'ta
    // olamayacak karakter taşıyan sözcük bu yüzden atlanıyor.
    let words: Vec<&str> = line
        .split_whitespace()
        .filter(|word| !word.contains(['/', '=', '~']))
        .collect();
    let host = mosh_target(&words)?;
    let all: Vec<&str> = line.split_whitespace().collect();
    Some(Target {
        host,
        kind: RemoteKind::Mosh,
        argv: mosh_argv(&all),
    })
}

/// Ön plan grubunun adları: yapraklar (grubun başka bir üyesinin ebeveyni
/// olmayan üyeler) pid sırasıyla ve tekrarsız; yaprak yoksa liderin adı
/// (liderin pid'i grubun kimliği), o da yoksa hiçbiri.
fn names(group: u32, table: &impl ProcessTable) -> Vec<String> {
    let mut members = table.members(group);
    members.sort_unstable();
    let parents: Vec<Option<u32>> = members.iter().map(|&pid| table.parent(pid)).collect();
    let mut names: Vec<String> = Vec::new();
    for &pid in &members {
        if parents.contains(&Some(pid)) {
            continue;
        }
        if let Some(name) = table.name(pid)
            && !names.contains(&name)
        {
            names.push(name);
        }
    }
    if names.is_empty() {
        names.extend(table.name(group));
    }
    names
}

/// [`ProcessTable`]'ın macOS gövdesi: `libc`'nin Apple yarısı (`libproc`).
/// Kare yolunda değil: kapanış anında ve komut başladığında (uzak oturum
/// yoklaması), ana thread'de birkaç sistem çağrısı.
pub(crate) struct Libproc;

impl ProcessTable for Libproc {
    fn children(&self, pid: u32) -> Vec<u32> {
        pid_list(libc::proc_listchildpids, pid)
    }

    fn groups(&self, shell: u32) -> Option<Groups> {
        // SAFETY: `proc_bsdinfo` yalnız tamsayı ve `c_char` dizisi taşıyan düz
        // bir C yapısı; sıfır bayt onun geçerli bir değeri.
        let info: libc::proc_bsdinfo = unsafe { pid_info(shell, libc::PROC_PIDTBSDINFO)? };
        Some(Groups {
            own: info.pbi_pgid,
            foreground: info.e_tpgid,
        })
    }

    fn members(&self, group: u32) -> Vec<u32> {
        pid_list(libc::proc_listpgrppids, group)
    }

    fn parent(&self, pid: u32) -> Option<u32> {
        short_info(pid).map(|info| info.pbsi_ppid)
    }

    /// Önce `proc_name` (uzun ad), olmazsa kısa bilginin `comm`'u: ilki root'a
    /// ait süreçte başarısız (ölçüldü, `launchd`'de sıfır), ikincisi 16
    /// karakterde kesik ama her süreçte okunuyor.
    fn name(&self, pid: u32) -> Option<String> {
        let c_pid = c_int::try_from(pid).ok()?;
        // `pbi_name`'in boyu: `MAXCOMLEN`'in iki katı.
        let mut buf = [0u8; 2 * libc::MAXCOMLEN];
        // SAFETY: tampon bu çerçevenin ve boyu çağrıya olduğu gibi gidiyor;
        // çekirdek en çok o kadar bayt yazar.
        let len = unsafe { libc::proc_name(c_pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        match usize::try_from(len) {
            Ok(len) if len > 0 => {
                Some(String::from_utf8_lossy(&buf[..len.min(buf.len())]).into_owned())
            }
            _ => {
                let info = short_info(pid)?;
                let bytes: Vec<u8> = info
                    .pbsi_comm
                    .iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8)
                    .collect();
                (!bytes.is_empty()).then(|| String::from_utf8_lossy(&bytes).into_owned())
            }
        }
    }

    fn args(&self, pid: u32) -> Option<Vec<String>> {
        process_args(pid)
    }
}

/// `sysctl(KERN_PROCARGS2)`: düzen `argc` (4 bayt), exec yolu, NUL dolgusu,
/// `argc` tane NUL sonlu argüman — ardından **ortam** geliyor, okunmuyor.
fn process_args(pid: u32) -> Option<Vec<String>> {
    let pid = c_int::try_from(pid).ok()?;
    // Tampon `kern.argmax` boyunda: boş tamponlu sorgu gerçek boyu değil bu
    // tavanı veriyor.
    // Tavan süreç ömründe değişmiyor: bir kez soruluyor.
    static ARGMAX: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let capacity = (*ARGMAX.get_or_init(|| {
        let mut argmax: c_int = 0;
        let mut size = size_of::<c_int>();
        let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
        // SAFETY: `mib` iki elemanlı ve çerçevenin; çıktı tek bir `c_int` ve
        // boyu çağrıya olduğu gibi gidiyor.
        let status = unsafe {
            libc::sysctl(
                mib.as_mut_ptr(),
                2,
                (&raw mut argmax).cast(),
                &raw mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        usize::try_from(argmax).ok().filter(|_| status == 0)
    }))?;
    let mut buf = vec![0u8; capacity];
    let mut len = capacity;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // SAFETY: tampon `capacity` bayt ve bu çerçevenin; çekirdek en çok
    // `len` bayt yazar ve yazdığını `len`'e koyar.
    let status = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buf.as_mut_ptr().cast(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if status != 0 {
        return None;
    }
    parse_procargs(buf.get(..len)?)
}

/// [`process_args`]'ın saf yarısı.
fn parse_procargs(buf: &[u8]) -> Option<Vec<String>> {
    let argc = usize::try_from(i32::from_ne_bytes(buf.get(..4)?.try_into().ok()?)).ok()?;
    let rest = &buf[4..];
    // Exec yolu, sonra NUL dolgusu: argv[0] boş olamaz, yani ilk NUL olmayan
    // bayt argv'nin başı.
    let path_end = rest.iter().position(|&b| b == 0)?;
    let start = path_end + rest[path_end..].iter().position(|&b| b != 0)?;
    let mut args = Vec::with_capacity(argc);
    let mut fields = rest[start..].split(|&b| b == 0);
    for _ in 0..argc {
        args.push(String::from_utf8_lossy(fields.next()?).into_owned());
    }
    Some(args)
}

/// `proc_listchildpids` ve `proc_listpgrppids`'in ortak imzası.
type PidLister = unsafe extern "C" fn(libc::pid_t, *mut c_void, c_int) -> c_int;

/// Bir listeleme çağrısının pid'leri.
///
/// **Tamponun boyu çağrının kendi cevabından**: boş tamponla çağrı bir üst
/// sınır veriyor ve o, ölçüldüğü üzere sistemdeki bütün süreçlerin sayısı —
/// iki çağrı arasında doğan bir çocuğa da yer var. Sabit küçük bir tampon
/// **sessizce** kırpılırdı (ölçüldü: bir pid'lik tampona `launchd`'nin
/// çocukları için 1). Dönüş bayt değil **pid sayısı** (ölçüldü).
fn pid_list(list: PidLister, key: u32) -> Vec<u32> {
    let Ok(key) = libc::pid_t::try_from(key) else {
        return Vec::new();
    };
    // SAFETY: boş tampon, sıfır boy: çağrı yalnız bir tahmin döndürüyor ve
    // hiçbir şey yazmıyor.
    let estimate = unsafe { list(key, std::ptr::null_mut(), 0) };
    let capacity = usize::try_from(estimate).unwrap_or(0);
    let Ok(bytes) = c_int::try_from(capacity * size_of::<libc::pid_t>()) else {
        return Vec::new();
    };
    if capacity == 0 {
        return Vec::new();
    }
    let mut pids: Vec<libc::pid_t> = vec![0; capacity];
    // SAFETY: tampon `capacity` pid boyunda ve bu çerçevenin; çağrıya giden
    // boy tam o kadar bayt.
    let filled = unsafe { list(key, pids.as_mut_ptr().cast(), bytes) };
    pids.truncate(usize::try_from(filled).unwrap_or(0).min(capacity));
    pids.into_iter()
        .filter_map(|pid| u32::try_from(pid).ok())
        .filter(|&pid| pid > 0)
        .collect()
}

/// Kısa bilgi: ebeveyn ve `comm` — root'a ait süreçte de okunuyor.
fn short_info(pid: u32) -> Option<libc::proc_bsdshortinfo> {
    // SAFETY: `proc_bsdshortinfo` yalnız tamsayı ve `c_char` dizisi taşıyan
    // düz bir C yapısı; sıfır bayt onun geçerli bir değeri.
    unsafe { pid_info(pid, libc::PROC_PIDT_SHORTBSDINFO) }
}

/// `proc_pidinfo`'nun tek yapılı çeşitleri. Yalnız **tam** doluluk başarı:
/// root'a ait süreçte uzun bilgi sıfır bayt dönüyor ve yarım bir yapı
/// sıfırlarıyla "grup 0" diye okunurdu.
///
/// # Safety
///
/// `T` bütün bitleri sıfır olan değeri geçerli olan düz bir C yapısı ve
/// `flavor`'ın çekirdeğin o yapıyı yazdığı çeşit olmalı.
unsafe fn pid_info<T>(pid: u32, flavor: c_int) -> Option<T> {
    let pid = c_int::try_from(pid).ok()?;
    let size = c_int::try_from(size_of::<T>()).ok()?;
    // SAFETY: çağıranın sözü — sıfır `T` geçerli.
    let mut info: T = unsafe { std::mem::zeroed() };
    // SAFETY: tampon tam bir `T` ve boyu çağrıya olduğu gibi gidiyor; çekirdek
    // en çok o kadar bayt yazar.
    let written = unsafe { libc::proc_pidinfo(pid, flavor, 0, (&raw mut info).cast(), size) };
    (written == size).then_some(info)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use bt_core::{
        CaretShape, CursorBlink, Osc52, Session, SessionOptions, TerminalOptions, Theme,
    };

    use super::*;
    use crate::child::{SilentWake, wait_until};

    /// Sahte süreç: ebeveyn, grup, ad (`None` → adı okunamıyor).
    struct Proc {
        parent: u32,
        group: u32,
        name: Option<&'static str>,
        args: Option<Vec<&'static str>>,
    }

    /// Sahte tablo. `terminal` kabuğun `e_tpgid`'i; `None` → kabuğun uzun
    /// bilgisi okunamıyor.
    struct Table {
        procs: HashMap<u32, Proc>,
        terminal: Option<u32>,
        members_readable: bool,
    }

    impl Table {
        fn new(terminal: Option<u32>) -> Self {
            Self {
                procs: HashMap::new(),
                terminal,
                members_readable: true,
            }
        }

        fn with(mut self, pid: u32, parent: u32, group: u32, name: &'static str) -> Self {
            self.procs.insert(
                pid,
                Proc {
                    parent,
                    group,
                    name: Some(name),
                    args: None,
                },
            );
            self
        }

        /// Argv'li süreç; argv[0] adın kendisi.
        fn run(mut self, pid: u32, parent: u32, group: u32, argv: &[&'static str]) -> Self {
            let name = argv[0].rsplit('/').next().expect("ad");
            let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
            self.procs.insert(
                pid,
                Proc {
                    parent,
                    group,
                    name: Some(name),
                    args: Some(argv.to_vec()),
                },
            );
            self
        }
    }

    impl ProcessTable for Table {
        fn children(&self, pid: u32) -> Vec<u32> {
            let mut kids: Vec<u32> = self
                .procs
                .iter()
                .filter(|(_, proc)| proc.parent == pid)
                .map(|(&kid, _)| kid)
                .collect();
            kids.sort_unstable();
            kids
        }

        fn groups(&self, shell: u32) -> Option<Groups> {
            Some(Groups {
                own: self.procs.get(&shell)?.group,
                foreground: self.terminal?,
            })
        }

        fn members(&self, group: u32) -> Vec<u32> {
            if !self.members_readable {
                return Vec::new();
            }
            // Sıra bilerek karışık: karar pid sırasını kendisi kurmalı.
            let mut members: Vec<u32> = self
                .procs
                .iter()
                .filter(|(_, proc)| proc.group == group)
                .map(|(&pid, _)| pid)
                .collect();
            members.sort_unstable_by(|a, b| b.cmp(a));
            members
        }

        fn parent(&self, pid: u32) -> Option<u32> {
            self.procs.get(&pid).map(|proc| proc.parent)
        }

        fn name(&self, pid: u32) -> Option<String> {
            self.procs.get(&pid)?.name.map(str::to_owned)
        }

        fn args(&self, pid: u32) -> Option<Vec<String>> {
            let args = self.procs.get(&pid)?.args.as_ref()?;
            Some(args.iter().map(|&arg| arg.to_owned()).collect())
        }
    }

    fn running(names: &[&str]) -> Foreground {
        Foreground::Running(names.iter().map(|&name| name.to_owned()).collect())
    }

    /// `login` 100 → `zsh` 101 (grup 101); ön plan grubu çağıranın.
    fn login_shell(terminal: u32) -> Table {
        Table::new(Some(terminal))
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh")
    }

    #[test]
    fn login_shell_in_the_foreground_is_idle() {
        assert_eq!(
            foreground(ShellParent::Login, 100, &login_shell(101)),
            Foreground::Idle
        );
    }

    #[test]
    fn a_program_in_the_foreground_is_running_by_name() {
        let table = login_shell(200).with(200, 101, 200, "vim");
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            running(&["vim"])
        );
    }

    #[test]
    fn a_wrapper_leader_yields_the_name_of_its_leaf() {
        // context.md'deki üçüncü satır: lider `bash` bir sarmalayıcı, program
        // onun torunu. Liderin adı yanlış cevap olurdu.
        let table = login_shell(300)
            .with(300, 101, 300, "bash")
            .with(301, 300, 300, "Orca")
            .with(302, 301, 300, "claude");
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            running(&["claude"])
        );
    }

    #[test]
    fn a_pipeline_names_every_leaf_once_in_pid_order() {
        // `cat | grep a | grep b`: üç yaprak, iki ad.
        let table = login_shell(400)
            .with(400, 101, 400, "cat")
            .with(401, 101, 400, "grep")
            .with(402, 101, 400, "grep");
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            running(&["cat", "grep"])
        );
    }

    #[test]
    fn a_login_without_a_shell_yet_is_idle() {
        // Sekme doğar doğmaz ⌘W: `login` kabuğu henüz çatallamadı.
        let table = Table::new(Some(100)).with(100, 1, 100, "login");
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            Foreground::Idle
        );
    }

    #[test]
    fn an_unreadable_shell_counts_as_running_without_a_name() {
        let table = Table::new(None)
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh");
        assert_eq!(foreground(ShellParent::Login, 100, &table), running(&[]));
        assert_eq!(
            foreground(ShellParent::Login, 100, &login_shell(0)),
            running(&[])
        );
    }

    #[test]
    fn an_unreadable_group_falls_back_to_the_leader_then_to_no_name() {
        let mut table = login_shell(200).with(200, 101, 200, "vim");
        table.members_readable = false;
        assert_eq!(
            foreground(ShellParent::Login, 100, &table),
            running(&["vim"])
        );
        table.procs.get_mut(&200).expect("vim tabloda").name = None;
        assert_eq!(foreground(ShellParent::Login, 100, &table), running(&[]));
    }

    #[test]
    fn a_direct_child_is_the_shell_itself() {
        // Doğrudan yolda `sleep`'in ebeveyni çocuğun kendisi: "ön plan
        // çocuğun çocuğuysa boşta" gibi bayraksız bir kural onu boşta
        // sayardı (discussion.md → Reddedilenler).
        let idle = Table::new(Some(101)).with(101, 1, 101, "zsh");
        assert_eq!(
            foreground(ShellParent::Direct, 101, &idle),
            Foreground::Idle
        );
        let busy = Table::new(Some(200))
            .with(101, 1, 101, "zsh")
            .with(200, 101, 200, "sleep");
        assert_eq!(
            foreground(ShellParent::Direct, 101, &busy),
            running(&["sleep"])
        );
    }

    /// `login` 100 → `zsh` 101; ön plan grubu 200 ve üyeleri `argv`'lerle.
    fn probe_of(procs: &[(u32, u32, &[&'static str])]) -> Probe {
        let mut table = login_shell(200);
        for &(pid, parent, argv) in procs {
            table = table.run(pid, parent, 200, argv);
        }
        remote(ShellParent::Login, 100, &table)
    }

    /// [`probe_of`]'un **yalnız host'a** bakan hâli: 036'nın sınamaları
    /// hedefin argv'sini ve türünü sormuyor (037'ninkiler [`target_of`]).
    fn remote_of(procs: &[(u32, u32, &[&'static str])]) -> Probe {
        match probe_of(procs) {
            Probe::Remote(target) => remote_host(&target.host),
            other => other,
        }
    }

    fn remote_host(host: &str) -> Probe {
        Probe::Remote(Target {
            host: host.to_owned(),
            kind: RemoteKind::Ssh,
            argv: Vec::new(),
        })
    }

    /// Tek süreçli grubun bütün hedefi; uzak değilse sınama düşer.
    fn target_of(argv: &[&'static str]) -> Target {
        match probe_of(&[(200, 101, argv)]) {
            Probe::Remote(target) => target,
            other => panic!("uzak bir hedef beklendi: {argv:?} → {other:?}"),
        }
    }

    fn words(argv: &[&str]) -> Vec<String> {
        argv.iter().map(|arg| (*arg).to_owned()).collect()
    }

    #[test]
    fn the_rerun_argv_drops_local_forwards_master_and_background() {
        // 037 Karar 6: `-L -R -D` değerleriyle, `-M` ve `-f` düşüyor; kalan
        // her şey sırasıyla.
        let target = target_of(&["ssh", "-p", "2222", "-J", "jump", "-L", "8080:x:80", "prod"]);
        assert_eq!(target.host, "prod");
        assert_eq!(target.kind, RemoteKind::Ssh);
        assert_eq!(
            target.argv,
            words(&["ssh", "-p", "2222", "-J", "jump", "prod"])
        );
        // Birleşik kümeler 036'nın yürüyüşüyle bölünüyor: bitişik değer
        // bayrağıyla gidiyor, bayrağı kalmayan küme bütünüyle düşüyor.
        assert_eq!(
            target_of(&[
                "ssh", "-vL", "1:x:1", "-p2222", "-MR9:y:9", "-D1080", "-4", "prod"
            ])
            .argv,
            words(&["ssh", "-v", "-p2222", "-4", "prod"])
        );
        assert_eq!(
            target_of(&["ssh", "-fM", "-o", "User=x", "--", "prod"]).argv,
            words(&["ssh", "-o", "User=x", "--", "prod"])
        );
        // Hedeften sonraki seçenekler de süzülüyor (OpenSSH onları yeniden
        // okuyor); argv[0] sürecin verdiği gibi.
        assert_eq!(
            target_of(&["/usr/bin/ssh", "prod", "-L", "1:x:1", "-v"]).argv,
            words(&["/usr/bin/ssh", "prod", "-v"])
        );
    }

    #[test]
    fn a_forced_tty_command_is_kept_in_the_rerun_argv() {
        assert_eq!(
            target_of(&["ssh", "-t", "prod", "tmux", "attach"]).argv,
            words(&["ssh", "-t", "prod", "tmux", "attach"])
        );
        // `-f` etkileşimsiz kalıyor (036'nın cevabı değişmedi).
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-fN", "prod"])]),
            Probe::Local
        );
    }

    #[test]
    fn mosh_reruns_as_mosh_with_the_script_arguments() {
        // Betik görülüyorsa `mosh` + betiğin argümanları, olduğu gibi.
        let probe = probe_of(&[(
            200,
            101,
            &[
                "/usr/bin/perl5.34",
                "-w",
                "/opt/homebrew/bin/mosh",
                "--ssh=ssh -p 2",
                "prod",
            ],
        )]);
        let Probe::Remote(target) = probe else {
            panic!("mosh uzak: {probe:?}");
        };
        assert_eq!(target.kind, RemoteKind::Mosh);
        assert_eq!(target.host, "prod");
        assert_eq!(target.argv, words(&["mosh", "--ssh=ssh -p 2", "prod"]));
        // Yalnız `mosh-client` görüldüyse `-#` satırı, boşlukla bölünmüş.
        let target = target_of(&[
            "mosh-client",
            "-# -p 60001 --ssh=ssh deploy@prod |",
            "10.0.0.5",
            "60001",
        ]);
        assert_eq!(target.kind, RemoteKind::Mosh);
        assert_eq!(target.host, "deploy@prod");
        assert_eq!(
            target.argv,
            words(&["mosh", "-p", "60001", "--ssh=ssh", "deploy@prod"])
        );
    }

    #[test]
    fn the_shell_in_the_foreground_is_undecided() {
        // `C` fork'tan önce basılıyor: kabuk hâlâ ön planda.
        assert_eq!(
            remote(ShellParent::Login, 100, &login_shell(101)),
            Probe::Undecided
        );
    }

    #[test]
    fn a_forked_shell_child_is_undecided() {
        // Çatallanmış ama henüz `exec` etmemiş çocuk kabuğun adını taşıyor.
        let table = login_shell(200).with(200, 101, 200, "zsh");
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Undecided);
    }

    #[test]
    fn an_interactive_ssh_is_remote_by_its_target() {
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "prod"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-p", "2222", "-v", "deploy@10.0.0.5"])]),
            remote_host("deploy@10.0.0.5")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-p2222", "-4v", "prod"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-o", "User=x", "--", "prod"])]),
            remote_host("prod")
        );
    }

    #[test]
    fn a_jump_host_child_does_not_hide_the_target() {
        // `ssh -J jump prod` aynı grupta `ssh -W prod:22 jump` doğuruyor;
        // yaprak kuralı jump host'u verirdi.
        assert_eq!(
            remote_of(&[
                (200, 101, &["ssh", "-J", "jump", "prod"]),
                (201, 200, &["ssh", "-W", "[prod]:22", "jump"]),
            ]),
            remote_host("prod")
        );
    }

    #[test]
    fn a_remote_command_is_local_unless_forced_onto_a_tty() {
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "prod", "uptime"])]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-t", "prod", "tmux", "attach"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-N", "-L", "8080:localhost:80", "prod"])]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-T", "prod"])]),
            Probe::Local
        );
        assert_eq!(remote_of(&[(200, 101, &["ssh", "-V"])]), Probe::Local);
    }

    #[test]
    fn options_after_the_target_are_options() {
        // OpenSSH hedeften sonra seçenekleri yeniden ayrıştırıyor.
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "prod", "-p", "2222", "-v"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "prod", "-p", "2222", "uptime"])]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "--", "prod", "-v"])]),
            Probe::Local
        );
    }

    #[test]
    fn config_options_decide_the_session_too() {
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-o", "RequestTTY=force", "prod", "tmux"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(
                200,
                101,
                &["ssh", "-oSessionType=none", "-L", "1:x:1", "prod"]
            )]),
            Probe::Local
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "-o", "requesttty no", "prod"])]),
            Probe::Local
        );
    }

    #[test]
    fn an_interactive_ssh_wins_over_a_batch_one_in_a_pipeline() {
        assert_eq!(
            remote_of(&[
                (200, 101, &["ssh", "backup", "cat", "dump"]),
                (201, 101, &["ssh", "prod"]),
            ]),
            remote_host("prod")
        );
    }

    #[test]
    fn a_half_forked_pipeline_is_undecided() {
        // `ssh prod | tee log`: `tee` `exec` etti, ssh yanı henüz değil.
        let table = login_shell(200)
            .with(200, 101, 200, "zsh")
            .run(201, 101, 200, &["tee", "log"]);
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Undecided);
    }

    #[test]
    fn an_ssh_uri_drops_the_scheme_and_the_port() {
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "ssh://deploy@h:2222"])]),
            remote_host("deploy@h")
        );
        assert_eq!(
            remote_of(&[(200, 101, &["ssh", "ssh://[::1]:22"])]),
            remote_host("::1")
        );
    }

    #[test]
    fn mosh_is_found_above_its_bootstrap_ssh() {
        // Perl betiği; bootstrap ssh onun çocuğu ve uzak komut taşıyor.
        assert_eq!(
            remote_of(&[
                (
                    200,
                    101,
                    &[
                        "/usr/bin/perl5.34",
                        "-w",
                        "/opt/homebrew/bin/mosh",
                        "--ssh=ssh -p 2",
                        "prod"
                    ]
                ),
                (
                    201,
                    200,
                    &[
                        "ssh",
                        "-n",
                        "-tt",
                        "-S",
                        "none",
                        "prod",
                        "--",
                        "mosh-server",
                        "new"
                    ]
                ),
            ]),
            remote_host("prod")
        );
    }

    #[test]
    fn mosh_client_names_the_target_from_its_command_line() {
        assert_eq!(
            remote_of(&[(200, 101, &["mosh-client", "-# prod |", "10.0.0.5", "60001"])]),
            remote_host("prod")
        );
        assert_eq!(
            remote_of(&[(
                200,
                101,
                &[
                    "mosh-client",
                    "-# -p 60001 --ssh=ssh deploy@prod |",
                    "10.0.0.5",
                    "60001"
                ]
            )]),
            remote_host("deploy@prod")
        );
        // Tırnaklı `--ssh` değeri tırnaksız birleşmiş.
        assert_eq!(
            remote_of(&[(
                200,
                101,
                &[
                    "mosh-client",
                    "-# --ssh=ssh -i ~/.ssh/k -o Port=2 prod |",
                    "10.0.0.5",
                    "1"
                ]
            )]),
            remote_host("prod")
        );
    }

    #[test]
    fn other_programs_and_unreadable_groups_are_local() {
        assert_eq!(remote_of(&[(200, 101, &["cat"])]), Probe::Local);
        // Perl ama mosh değil.
        assert_eq!(
            remote_of(&[(200, 101, &["perl", "script.pl", "prod"])]),
            Probe::Local
        );
        // Grup okunamıyor.
        let mut table = login_shell(200).run(200, 101, 200, &["ssh", "prod"]);
        table.members_readable = false;
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Local);
        // Kabuğun kendisi okunamıyor.
        let table = Table::new(None)
            .with(100, 1, 100, "login")
            .with(101, 100, 101, "zsh");
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Local);
        // Argv okunamayan ssh tanınmıyor.
        let table = login_shell(200).with(200, 101, 200, "ssh");
        assert_eq!(remote(ShellParent::Login, 100, &table), Probe::Local);
    }

    #[test]
    fn procargs_layout_yields_argv_without_the_environment() {
        let mut buf = 2i32.to_ne_bytes().to_vec();
        buf.extend_from_slice(b"/bin/sleep\0\0\0\0/bin/sleep\0\x33\x30\0HOME=/x\0");
        assert_eq!(
            parse_procargs(&buf),
            Some(vec!["/bin/sleep".to_owned(), "30".to_owned()])
        );
        assert_eq!(parse_procargs(&buf[..3]), None);
    }

    #[test]
    fn the_process_table_reads_a_real_argv() {
        // `KERN_PROCARGS2` gövdesinin tanığı: aynı kullanıcının bilinen
        // argv'li bir çocuğu.
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("sleep doğmadı");
        let args = Libproc.args(child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(args, Some(vec!["/bin/sleep".to_owned(), "30".to_owned()]));
    }

    #[test]
    fn the_process_table_sees_a_real_foreground_job() {
        // Okuyucunun tek tanığı: sahte tablo login'in root olduğunu da,
        // `e_tpgid`'in hangi süreçte okunduğunu da göremez. Kontrol terminali
        // olmayan bir ortamda `e_tpgid` sıfır gelirse sınama atlanmıyor,
        // düşüyor — o durumda okuyucu yanlış.
        let session = Session::spawn(
            SessionOptions {
                command: Some((
                    "/bin/zsh".to_owned(),
                    vec!["-f".to_owned(), "-i".to_owned()],
                )),
                working_directory: None,
                home: None,
                env: HashMap::new(),
                cols: 40,
                rows: 10,
                cell_px: (9, 18),
                terminal: TerminalOptions {
                    scrollback: 100,
                    osc52: Osc52::Off,
                    cursor: CaretShape::default(),
                    blink: CursorBlink::default(),
                },
                theme: Theme::BATERI,
                dock: false,
                cluster: false,
            },
            Arc::new(SilentWake),
        )
        .expect("oturum açılamadı");
        let child = session.child_pid();
        let now = || foreground(ShellParent::Direct, child, &Libproc);

        wait_until("kabuk ön planda görünmedi", || now() == Foreground::Idle);
        // Süre sınamanın son tarihinden uzun: iş, iddia okunurken bitmemeli.
        session.write(b"sleep 30\n");
        wait_until("ön plandaki `sleep` görülmedi", || {
            now() == running(&["sleep"])
        });
        // Ctrl-C işi öldürüyor ve ön plan kabuğa dönüyor: geride süreç kalmaz.
        session.write(b"\x03");
        wait_until("iş bitince kabuk ön plana dönmedi", || {
            now() == Foreground::Idle
        });
        session.shutdown();
        assert!(!session.reader_alive(), "oturum kapanmadı");
    }
}
