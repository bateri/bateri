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
//! **Bilinen sınırlar** (Karar 7): arka plan işleri (`sleep 100 &`) ön planda
//! değil ve sayılmıyor; kabuğun kendi içinde koşan iş (yerleşik döngü, `read`
//! bekleyen bir fonksiyon) ayrı bir grup açmıyor ve boşta görünüyor; `exec
//! vim` kabuğun pid'ini ve grubunu devraldığı için o da boşta.

use std::ffi::{c_int, c_void};

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

/// Kararın süreç tablosundan sorduğu beş şey.
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
}

/// Kararın kendisi: `child` PTY'nin çocuğunun pid'i.
///
/// Başarısızlık kolları (R1.5) yanlışın yönüne göre: çocuksuz `login`
/// (sekme doğar doğmaz ⌘W) boşta, çünkü kapatılacak bir iş yok; kabuğun
/// grubu okunamazsa **adsız koşuyor**, çünkü sistematik bir kırılma sessiz
/// "hiç sormuyor" değil görünür "hep soruyor" olmalı.
pub(crate) fn foreground(parent: ShellParent, child: u32, table: &impl ProcessTable) -> Foreground {
    let shell = match parent {
        ShellParent::Direct => child,
        // `login` tek bir kabuk çatallıyor; henüz yoksa kapatılacak iş de yok.
        ShellParent::Login => match table.children(child).first() {
            Some(&shell) => shell,
            None => return Foreground::Idle,
        },
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
/// Kare yolunda değil, yalnız kapanış anında ve ana thread'de birkaç sistem
/// çağrısı.
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
