//! Dosya izleme: `dispatch2`'nin vnode kaynakları.
//!
//! Değişiklik gelmedikçe hiçbir şey uyanmaz — yoklama yok, thread yok;
//! çekirdek bir dosyaya dokunulunca kaynağın kuyruğuna iş atar. Kararın kaydı
//! `.tasks/007-ayarlar-ve-tema/discussion.md` → Karar 2.
//!
//! **Ayar bilmez:** hangi yolların izleneceği çağıranın (`settings`'in yol
//! yardımcıları, `app`'in uygulayıcısı); burası bir yolun dizin mi dosya mı
//! olduğuna bakıp kaynağını kurar. İki tür:
//!
//! - **Dizin** (`WRITE | DELETE | RENAME`): girdinin doğumu, silinmesi, üstüne
//!   taşınması — editörün "geçici dosyaya yaz, üstüne taşı" kaydı. `DELETE` ve
//!   `RENAME` dizinin **kendisi** gidince haber veriyor: bir sonraki kurulum
//!   bayat tanıtıcıyı düşürsün.
//! - **Dosya** (`WRITE | EXTEND | ATTRIB | DELETE | RENAME`): yerinde yazma
//!   (`>>`, nano), yazmadan boşaltma (`: >`) ve sembolik bağın **hedefindeki**
//!   kayıt dizine iz bırakmaz. Dosya `O_EVTONLY` ile açılır (yalnız olay
//!   tanıtıcısı) ve açılış bağı izler; kaynak hedefe bağlanır.
//!
//! **Kurulum tek atımlık.** Kaynak olaydan sonra da yaşar ama baktığı inode
//! artık yolda olmayabilir (üstüne taşınan dosya). Çağıran her olayda
//! yeniden kurar ve sıra **önce kur, sonra oku**: tersinde okumayla kurulum
//! arasına düşen kayıt hiçbir olay doğurmaz ve ekranda eski içerik kalır.
//! Önce kurmanın bedeli en çok fazladan bir olay, o da boş fark.
//!
//! **Olmayan yol kaynak doğurmaz ve hata değildir.** Sonradan yaratılan bir
//! dizini hiçbir şey görmez (üst dizin izlenmiyor); yeniden kurmayı dışarıdan
//! tetiklemek çağıranın işi.
//!
//! İşleyici fonksiyon işaretçisiyle kurulur (`set_event_handler_f`):
//! `bt-shell`'e `block2` kenarı yok. Kaynak başına bir `Box` context'i fd'yi
//! ve bildirimi taşır; onu **iptal işleyicisi** düşürür, çünkü libdispatch
//! tanıtıcıyı kapatmanın güvenli anını orada veriyor — iptal eşzamansız ve
//! koşmakta olan işleyici o sırada context'i okuyor olabilir.

use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dispatch2::{
    _dispatch_source_type_vnode, DispatchObject, DispatchQueue, DispatchRetained, DispatchSource,
    dispatch_source_vnode_flags_t as Vnode,
};

/// Bir olayın bildirimi; kaynağın kuyruğunda koşar.
///
/// `Send + Sync`, çünkü iptal işleyicisi onu kuyruğun thread'inde düşürüyor.
/// Üretimdekinin yakaladığı hiçbir şey yok (`app`'in hedefsiz eylemi), yani
/// ana kuyruğa bağlılığı tipte değil kuyruk seçiminde.
pub(crate) type Notify = Arc<dyn Fn() + Send + Sync>;

/// Dizinin olayları; gerekçesi modül başında.
const DIR_EVENTS: usize = (Vnode::DISPATCH_VNODE_WRITE.0
    | Vnode::DISPATCH_VNODE_DELETE.0
    | Vnode::DISPATCH_VNODE_RENAME.0) as usize;

/// Dosyanın olayları; `EXTEND` eklemenin (`>>`) kendi bayrağı.
///
/// `ATTRIB` yazmadan boşaltma için (`: > dosya`, `truncate -s 0`): kqueue onu
/// `WRITE` değil öznitelik olayı diye veriyor. Bedeli `touch` ve `chmod`'un da
/// yeniden okutması, o da boş fark. Okumanın kendisi olay **doğurmuyor**
/// (`reading_does_not_notify`); doğursaydı her olayda kaynağı kurup okuyan
/// uygulayıcı kendi kendini sonsuza dek uyandırırdı.
const FILE_EVENTS: usize = (Vnode::DISPATCH_VNODE_WRITE.0
    | Vnode::DISPATCH_VNODE_EXTEND.0
    | Vnode::DISPATCH_VNODE_ATTRIB.0
    | Vnode::DISPATCH_VNODE_DELETE.0
    | Vnode::DISPATCH_VNODE_RENAME.0) as usize;

/// Bir yol listesinin kaynakları. Düşünce hepsi iptal edilir.
pub(crate) struct Watch {
    sources: Vec<DispatchRetained<DispatchSource>>,
}

/// Kaynağın context'i: fd kaynak yaşadıkça açık kalmalı, bildirim de olay
/// başına çağrılıyor.
struct Context {
    /// Okunmuyor; tutulması fd'yi açık tutuyor ve düşmesi kapatıyor.
    _file: File,
    notify: Notify,
}

impl Watch {
    /// `paths`'in var olanlarına kaynak kurar; olaylar `queue`'da `notify`'ı
    /// çağırır.
    ///
    /// Yenisini kurup eskisini **sonra** düşürmek (`slot.replace(..)`) iki
    /// kurulumun arasında boşluk bırakmaz; o anda iki kaynağın birden
    /// haber vermesi zararsız.
    pub(crate) fn install(paths: &[PathBuf], queue: &DispatchQueue, notify: &Notify) -> Self {
        Self {
            sources: paths
                .iter()
                .filter_map(|path| arm(path, queue, notify))
                .collect(),
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        // İptal eşzamansız: context'i ve fd'yi iptal işleyicisi bırakıyor.
        // Kaynağın kendisi de o ana kadar libdispatch'in referansıyla yaşıyor.
        for source in &self.sources {
            source.cancel();
        }
    }
}

/// Tek yolun kaynağı; yol yoksa, dizin ya da düz dosya değilse ya da
/// açılamıyorsa `None`.
///
/// Tür `metadata` ile, açmadan **önce** soruluyor: FIFO'yu salt okunur açmak
/// bir yazar gelene kadar ana thread'i bekletirdi (`settings::read_text`'in
/// aynı eleği). `metadata` bağı izliyor, açılış da.
fn arm(
    path: &Path,
    queue: &DispatchQueue,
    notify: &Notify,
) -> Option<DispatchRetained<DispatchSource>> {
    let meta = std::fs::metadata(path).ok()?;
    let mask = if meta.is_dir() {
        DIR_EVENTS
    } else if meta.is_file() {
        FILE_EVENTS
    } else {
        return None;
    };
    // Açılamayan yol (izin) sessizce izlenmez: okuyucu aynı yolu okurken
    // hatayı zaten alt başlığa yazıyor.
    //
    // `O_EVTONLY`, salt okunur değil (`/code-review` bulgusu): tanıtıcı yalnız
    // olay içindir. Okuma tanıtıcısı bağın hedefi harici bir diskteyse onu
    // "kullanımda" tutup çıkarılmasını engeller ve iCloud'dan tahliye edilmiş
    // bir dosyada her yeniden kurulumda indirmeyi tetikleyebilirdi.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_EVTONLY)
        .open(path)
        .ok()?;
    let fd = file.as_raw_fd();
    let context = Box::into_raw(Box::new(Context {
        _file: file,
        notify: Arc::clone(notify),
    }));
    // SAFETY: tür libdispatch'in vnode sabiti ve maske o türün bayrakları;
    // tanıtıcı açık bir fd ve `context` onu iptal işleyicisine kadar tutuyor.
    // Kuyruk canlı bir referanstan; kaynak onu kendisi de tutar. `new` NULL
    // yalnız geçersiz argümanda döner.
    let source = unsafe {
        DispatchSource::new(
            (&raw const _dispatch_source_type_vnode).cast_mut(),
            fd as usize,
            mask,
            Some(queue),
        )
    };
    // Sıra zorunlu: işleyiciye giden context, işleyicinin **kurulduğu andaki**
    // context (`set_event_handler_f`'in doc'u) — önce context, sonra
    // işleyiciler, en son etkinleştirme. Etkinleştirilmemiş kaynak da
    // düşürülemez.
    //
    // SAFETY: `context` bir `Box<Context>`; olay işleyicisi yalnız okuyor,
    // iptal işleyicisi bir kez geri alıyor ve libdispatch iptal işleyicisini
    // koşan olay işleyicisi bittikten sonra, olay işleyicisini de iptalden
    // sonra hiç çağırmıyor.
    unsafe { source.set_context(context.cast()) };
    source.set_event_handler_f(on_event);
    source.set_cancel_handler_f(on_cancel);
    source.activate();
    Some(source)
}

extern "C" fn on_event(context: *mut c_void) {
    // SAFETY: `arm`'ın kurduğu `Box<Context>`; iptal işleyicisi henüz
    // koşmadı (sözleşme `arm`'da).
    let context = unsafe { &*context.cast::<Context>() };
    (context.notify)();
}

extern "C" fn on_cancel(context: *mut c_void) {
    // SAFETY: `arm`'ın kurduğu `Box<Context>`; iptal işleyicisi kaynak başına
    // bir kez koşar ve ondan sonra olay işleyicisi çağrılmaz.
    drop(unsafe { Box::from_raw(context.cast::<Context>()) });
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    use super::*;
    use crate::settings::{self, TempRoot};

    /// Olay beklemesinin tavanı. Olay normalde milisaniyeler içinde geliyor;
    /// tavan yalnız düşen sınamanın ne kadar bekleyeceği.
    const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

    /// Sınamanın kuyruğu ve bildirimi: olay başına kanala bir `()`.
    struct Probe {
        queue: DispatchRetained<DispatchQueue>,
        notify: Notify,
        events: Receiver<()>,
    }

    impl Probe {
        fn new() -> Self {
            let (sender, events) = mpsc::channel();
            Self {
                // Seri kuyruk: üretimdeki ana kuyruk gibi işleyiciler sırayla.
                queue: DispatchQueue::new("bateri.watch.test", None),
                notify: Arc::new(move || {
                    let _ = sender.send(());
                }),
                events,
            }
        }

        fn install(&self, root: &TempRoot) -> Watch {
            Watch::install(&settings::watched_paths(&root.0), &self.queue, &self.notify)
        }

        /// Bir olay gelmeli; gelmezse `what` ile düşer.
        fn expect_event(&self, what: &str) {
            assert!(self.events.recv_timeout(EVENT_TIMEOUT).is_ok(), "{what}");
        }

        /// Kuyruğu boşaltır: iptal edilmiş kaynağın **koşmakta olan**
        /// işleyicisi bitsin (bariyer), sonra kanalda biriken olaylar atılsın.
        /// Bir kaydın birden çok olay doğurması olağan (dizin + dosya).
        fn drain(&self) {
            self.queue.exec_sync(|| {});
            while self.events.try_recv().is_ok() {}
        }
    }

    fn append(path: &std::path::Path, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(path)
            .expect("dosya açılamadı");
        file.write_all(text.as_bytes()).expect("yazılamadı");
    }

    /// Editörün kaydı: geçici dosyaya yaz, üstüne taşı.
    fn save_by_rename(root: &TempRoot, text: &str) {
        let tmp = root.0.join("settings.toml.tmp");
        std::fs::write(&tmp, text).expect("yazılamadı");
        std::fs::rename(&tmp, root.0.join(settings::FILE_NAME)).expect("taşınamadı");
    }

    #[test]
    fn append_in_place_is_seen() {
        // Yerinde yazma (`>>`, nano) dizine iz bırakmaz: olayı dosyanın kendi
        // kaynağı veriyor.
        let root = TempRoot::new("watch-append");
        std::fs::write(root.0.join(settings::FILE_NAME), "").expect("yazılamadı");
        let probe = Probe::new();
        let watch = probe.install(&root);
        // Kök + `settings.toml`; `themes/` yok.
        assert_eq!(watch.sources.len(), 2);

        append(&root.0.join(settings::FILE_NAME), "[terminal]\n");
        probe.expect_event("yerinde yazma olay üretmedi");
    }

    #[test]
    fn truncation_without_write_is_seen() {
        // `: > settings.toml` ve `truncate -s 0` dosyaya hiç yazmıyor: kqueue
        // yalnız öznitelik olayı (`ATTRIB`) veriyor ve dizin de değişmiyor
        // (`/code-review` bulgusu, bu makinede ölçüldü).
        let root = TempRoot::new("watch-truncate");
        std::fs::write(root.0.join(settings::FILE_NAME), "[terminal]\n").expect("yazılamadı");
        let probe = Probe::new();
        let _watch = probe.install(&root);

        std::fs::OpenOptions::new()
            .write(true)
            .open(root.0.join(settings::FILE_NAME))
            .expect("dosya açılamadı")
            .set_len(0)
            .expect("boşaltılamadı");
        probe.expect_event("boşaltma olay üretmedi");
    }

    #[test]
    fn reading_does_not_notify() {
        // `ATTRIB` izleniyor ve okuyucu her olayda kaynağı kurduktan **sonra**
        // dosyayı okuyor: okuma bir öznitelik olayı (erişim zamanı) doğursaydı
        // oku → olay → yeniden kur → oku döngüsü ana thread'i sonsuza dek
        // döndürürdü. Olayın gelmediği bir süre bekleniyor; kısa ama
        // olayların milisaniyeler içinde geldiği ölçeğin çok üstünde.
        let root = TempRoot::new("watch-read");
        std::fs::write(root.0.join(settings::FILE_NAME), "[terminal]\n").expect("yazılamadı");
        let probe = Probe::new();
        let _watch = probe.install(&root);

        for _ in 0..3 {
            let _ = settings::load(&root.0);
        }
        probe.queue.exec_sync(|| {});
        assert!(
            probe
                .events
                .recv_timeout(Duration::from_millis(500))
                .is_err(),
            "okuma olay üretti: yeniden okuma döngüsü doğar"
        );
    }

    #[test]
    fn rename_over_is_seen_again_after_reinstall() {
        // Üstüne taşınan dosya yeni bir inode: eski dosya kaynağı artık
        // silinmiş dosyaya bakıyor. Yeniden kurulumdan sonra hem ikinci
        // taşıma hem **yeni** dosyaya yerinde yazma görülmeli — ikincisini
        // yalnız yeni dosyaya kurulmuş kaynak görebilir.
        let root = TempRoot::new("watch-rename");
        std::fs::write(root.0.join(settings::FILE_NAME), "").expect("yazılamadı");
        let probe = Probe::new();
        let mut watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);

        save_by_rename(&root, "[terminal]\nscrollback = 1\n");
        probe.expect_event("üstüne taşıma olay üretmedi");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);
        probe.drain();

        save_by_rename(&root, "[terminal]\nscrollback = 2\n");
        probe.expect_event("yeniden kurulumdan sonra ikinci kayıt olay üretmedi");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);
        probe.drain();

        append(&root.0.join(settings::FILE_NAME), "# son\n");
        probe.expect_event("yeni dosyaya yerinde yazma olay üretmedi");
    }

    #[test]
    fn symlink_target_write_is_seen() {
        // Dotfile deposu: `settings.toml` başka dizindeki dosyaya bağ. Hedefe
        // yazmak kökün dizinine iz bırakmaz; kaynak açılışta bağı izleyip
        // hedefe bağlanmış olmalı.
        let root = TempRoot::new("watch-symlink");
        let repo = TempRoot::new("watch-symlink-repo");
        let target = repo.0.join("settings.toml");
        std::fs::write(&target, "").expect("yazılamadı");
        std::os::unix::fs::symlink(&target, root.0.join(settings::FILE_NAME))
            .expect("bağ kurulamadı");
        let probe = Probe::new();
        let _watch = probe.install(&root);

        append(&target, "[terminal]\n");
        probe.expect_event("bağın hedefine yazma olay üretmedi");
    }

    #[test]
    fn recreated_directory_is_watched_after_reinstall() {
        // Dizin silinince olay gelir ve yeniden kurulum hiçbir şey kurmaz:
        // yol yok. Yeniden yaratılan dizini **kendiliğinden** hiçbir şey
        // görmez (üst dizin izlenmiyor, Karar 2); dış tetikle yeniden kurulum
        // kaynakları yeni dizine kurar ve sonraki yazma görülür.
        let root = TempRoot::new("watch-recreate");
        std::fs::write(root.0.join(settings::FILE_NAME), "").expect("yazılamadı");
        let probe = Probe::new();
        let mut watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);

        std::fs::remove_dir_all(&root.0).expect("silinemedi");
        probe.expect_event("dizinin silinmesi olay üretmedi");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 0, "silinmiş dizine kaynak kuruldu");
        probe.drain();

        std::fs::create_dir(&root.0).expect("dizin kurulamadı");
        std::fs::write(root.0.join(settings::FILE_NAME), "").expect("yazılamadı");
        watch = probe.install(&root);
        assert_eq!(watch.sources.len(), 2);
        append(&root.0.join(settings::FILE_NAME), "[terminal]\n");
        probe.expect_event("yeni dizindeki dosyaya yazma olay üretmedi");
    }

    #[test]
    fn missing_paths_install_nothing() {
        let root = TempRoot::new("watch-missing");
        let absent = root.0.join("absent");
        let probe = Probe::new();
        let watch = Watch::install(
            &settings::watched_paths(&absent),
            &probe.queue,
            &probe.notify,
        );
        assert_eq!(watch.sources.len(), 0);
    }
}
