//! Pano köprüsü: `bt-core`'un bildiği metni AppKit panosuna taşır.
//!
//! Panoya dokunan yalnız `bt-shell-macos` (AppKit); `bt-core` bayt görür, pano
//! görmez (`CLAUDE.md` → katman düzeni). Kopyanın metni phase-1'in tek metin
//! yolundan gelir (`Session::selection_text`); ikinci bir metin yolu, ikinci
//! bir sarma hatası demek olurdu.
//!
//! İki yön de `NSPasteboard` üstünden; pano her çağrıya **parametre** olarak
//! gelir. Üretimde ikisi de genel panodur (`copy` kullanıcının Cmd-C'si,
//! `read` Cmd-V'si), sınamada ise yön başına ayrı bir benzersiz pano verilir:
//! başsız ortamda genel panoya dokunulamaz ve iki sınama birbirinin içeriğini
//! görmemeli. Panoyu parametre yapmak, canlılık çapasını da sınanabilir
//! kılıyor (`board()`).
//!
//! `copy`/`read` adları AppKit'in de değil `NSPasteboard`'un da değil, bu
//! köprünün kendi sözlüğüdür: `copy` yazma yönü, `read` okuma yönüdür.
//!
//! Üçüncü yol uzaktan kopya (OSC 52): metin okuyucu thread'de doğar ve ana
//! kuyruğa [`PendingCopy`] yuvasından geçer; panoya yine `copy` yazar.

use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};

use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
use objc2_foundation::NSString;

/// Seçili metni panoya yazar. Yazılacak metin yoksa pano **el değmeden**
/// kalır ve `false` döner.
///
/// Boş yazma genel panoyu temizler ve kullanıcının başka uygulamadan
/// kopyaladığını silerdi. Kapı bu yüzden iki hâli birden eler:
///
/// - `None` → seçim yok.
/// - `Some("")` → seçim **var** ama boş metin veriyor: boşluklardan oluşan
///   bir satırın üstünde sürükleme. O satırda alacritty'nin `line_length()`'i
///   sıfır, yani `selection_to_string()` `Some("")` döner. Bu hâl kapıdan
///   geçseydi prompt altındaki boş bir satırı seçip Cmd-C demek kullanıcının
///   panosunu boşaltırdı — `None` kapısının engellediği kaybın ta kendisi.
///   (Sürüklemesiz tık buraya düşmez: iki ucu eşit seçim boştur ve `None`
///   verir.)
///
/// **Yalnız boşluk** (`Some("   ")`) elenmez: satırın içindeki boşlukları
/// seçip kopyalamak meşru ve o metin boş değil.
pub(crate) fn copy(board: &NSPasteboard, text: Option<String>) -> bool {
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        return false;
    };
    // `clearContents` + `setString`: `setString` tek başına da yazar ama o
    // zaman eski tipler (RTF, TIFF) panoda kalır ve yapıştıran taraf metin
    // yerine onları alabilir.
    board.clearContents();
    let string = NSString::from_str(&text);
    // SAFETY: `unsafe` blok `NSPasteboardTypeString` **statik** erişimi için;
    // `setString:forType:`'ın kendisi `unsafe` değil (`objc2` onu güvenli
    // sarmalıyor). Statik gerçek bir `NSPasteboardType` kaydı ve `None`'a
    // çözümlenmiyor.
    //
    // Bağlam `MainThreadMarker` değil: `NSPasteboard` `objc2`'de `AnyThread`,
    // yani tip ana thread'i zorlamıyor. Ama `NSPasteboard` **eşzamanlı**
    // kullanıma dayanıklı değil — ayrı benzersiz panolar bile süreç çapında
    // bir tip önbelleğini paylaşıyor (`+[NSPasteboard(NSTypeConversion) …]`)
    // ve iki thread aynı anda dokununca `_updateTypeCacheIfNeeded`'de
    // SIGSEGV ya da bir panonun ötekinin metnini okuması doğuyor. Üretimde
    // bütün çağıranlar ana thread'de, yani sıralı; sınamalar aynı sırayı
    // [`tests::pasteboard_lock`] ile kuruyor.
    unsafe { board.setString_forType(&string, NSPasteboardTypeString) }
}

/// Panodaki metni okur. Metin yoksa (`None`) yapıştırma sessizdir.
pub(crate) fn read(board: &NSPasteboard) -> Option<String> {
    // SAFETY: yukarıdakiyle aynı statik erişimi.
    unsafe { board.stringForType(NSPasteboardTypeString) }.map(|s| s.to_string())
}

/// OSC 52'nin panoya gidecek metni: okuyucu thread koyar, ana kuyruk alır.
///
/// **Tek yuva, son yazma kazanır.** Durmadan OSC 52 basan bir uygulama
/// (döngüdeki `printf`) her dizi için ana kuyruğa bir iş atsaydı kuyruk
/// sınırsız büyür ve pano aynı saniyede yüz kez yazılırdı; kullanıcının
/// göreceği tek şey zaten sonuncusu. Yuva boşken dolduran çağrı **tek** iş
/// ister ([`PendingCopy::put`] `true`), iş yuvayı boşaltıp yazar
/// ([`PendingCopy::take`]). Değişmez: yuva doluysa onu alacak bir iş
/// kuyrukta ve henüz almamış — boş→dolu geçişi her zaman iş istiyor, işin
/// kendi takası yuvayı boşaltıyor. Yani kuyrukta en çok bir bekleyen, bir de
/// koşan iş olur ve son metin kaybolmaz.
///
/// **Kilitsiz**, çünkü `put` `Term` kilidi tutulurken okuyucu thread'de
/// çağrılıyor ve `Wake` uygulayanı kilit almaz (`bt-core`'un `wake.rs`'i;
/// `discussion.md` → Karar 5). `AtomicPtr` + `Box`: std'de sahip olunan bir
/// değeri atomik takaslayan başka tip yok.
///
/// AppKit'ten ayrık: yuva mantığı panosuz sınanıyor, panoyu alan taraf
/// seçiyor.
#[derive(Default)]
pub(crate) struct PendingCopy(AtomicPtr<String>);

impl PendingCopy {
    /// Metni yuvaya koyar; önceki metin henüz alınmadıysa düşer.
    ///
    /// `true` → yuva boştu, çağıran ana kuyruğa **bir** iş atmalı. `false` →
    /// yuvayı alacak iş zaten kuyrukta, yenisi gerekmiyor. Bloklamaz; düşen
    /// metnin serbest bırakılması bir kilit değil.
    pub(crate) fn put(&self, text: String) -> bool {
        let new = Box::into_raw(Box::new(text));
        let old = self.0.swap(new, Ordering::AcqRel);
        if old.is_null() {
            return true;
        }
        // SAFETY: yuvadaki her boş olmayan işaretçi yukarıdaki
        // `Box::into_raw`'dan geliyor ve takas onu yuvadan **atomik** olarak
        // çıkardı: başka hiçbir taraf (`take`, `Drop`, ikinci bir `put`) aynı
        // işaretçiyi göremez, yani sahiplik tek ve bir kez geri alınıyor.
        drop(unsafe { Box::from_raw(old) });
        false
    }

    /// Ana kuyruğun işi: yuvadaki metni alır ve yuvayı boşaltır; boşsa
    /// `None` (yarışta başka bir iş almış olabilir). Metni panoya pane'in
    /// sahibi yazıyor (`pane::PaneHost::copy_to_clipboard`; varsayılan kol
    /// genel panoya [`copy`] ile).
    pub(crate) fn take(&self) -> Option<String> {
        let old = self.0.swap(ptr::null_mut(), Ordering::AcqRel);
        // SAFETY: `put`'taki gerekçe — işaretçi `Box::into_raw`'dan ve takas
        // onu yuvadan tek başına çıkardı.
        (!old.is_null()).then(|| *unsafe { Box::from_raw(old) })
    }
}

impl Drop for PendingCopy {
    /// Alınmamış metni serbest bırakır — iş koşmadan uygulama kapanırsa.
    ///
    /// `bt-core`'un `Wake` sözleşmesine göre bu `Drop` `"PTY teardown"`
    /// thread'inde koşabilir; bir bellek serbest bırakmasından fazlası
    /// değil, bloklamaz.
    fn drop(&mut self) {
        let _ = self.take();
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, PoisonError};

    /// Panoya dokunan her sınamanın **ilk** satırı: `NSPasteboard`'a aynı
    /// anda tek thread girer (gerekçe [`copy`]'nin içinde).
    ///
    /// Gereken ana thread değil **sıra**: `--test-threads=1` her sınamayı
    /// yine bir işçi thread'de koşturuyor ve pano sınamaları orada 300/300
    /// geçti, paralel koşuda 5/300 düştü (2026-09-27). Kilit yalnız pano
    /// sınamalarını sıralıyor, kalan sınamalar paralel koşmaya devam ediyor.
    /// Zehirlenme yutuluyor: bir sınamanın iddiası öteki sınamaları
    /// düşürmemeli.
    pub(crate) fn pasteboard_lock() -> MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Canlı bir pano; başsız ortamda (CI, `cargo test` ssh üstünde) `None`.
    ///
    /// Benzersiz pano başsızda da **doğar**, ama yazma tutmaz. Çapa bu yüzden
    /// varlığa değil **yazıp okuyabilmeye** bağlı: tutan bir yazma gerçek bir
    /// pano sunucusu demek. Bunu `copy`/`read` üzerinden kurmak, canlılık
    /// ölçüsünü de köprünün kendi gövdesinden almak demek — ayrı bir yazma
    /// yolu kurup iğneyi iki yerde tutmaktan iyidir.
    fn board() -> Option<objc2::rc::Retained<NSPasteboard>> {
        // `pasteboardWithUniqueName` her çağrıda taze bir pano verir —
        // sınamalar birbirinin içeriğini görmez.
        let board = NSPasteboard::pasteboardWithUniqueName();
        const SENTINEL: &str = "bateri pano çapası";
        let alive =
            copy(&board, Some(SENTINEL.to_owned())) && read(&board).as_deref() == Some(SENTINEL);
        alive.then_some(board)
    }

    /// Başsız dala ortak gerekçe. Her iki sınama da aynı yolu tutar: pano
    /// yoksa **sessizce atlanır**. Boş bir panoya iddia kurmak kapının sahte
    /// yeşil verdiği yol olurdu — taze pano zaten boştur, `read` köprü
    /// çalışmasa da `None` döner.
    const HEADLESS: &str = "başsız ortamda pano yok, sınama atlandı";

    #[test]
    fn copy_writes_selection_text_to_clipboard() {
        let _pasteboard = pasteboard_lock();
        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("hello".to_owned())));
        assert_eq!(read(&board).as_deref(), Some("hello"));
    }

    #[test]
    fn copy_without_selection_leaves_board_untouched() {
        let _pasteboard = pasteboard_lock();
        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("önce".to_owned())));
        // Seçimsiz kopya `false` döner ve panodaki duranı silmez.
        assert!(!copy(&board, None));
        assert_eq!(read(&board).as_deref(), Some("önce"));
    }

    #[test]
    fn copy_of_empty_text_leaves_board_untouched() {
        let _pasteboard = pasteboard_lock();
        // `Some("")` = seçim var ama metin boş: boş bir satırın üstünde
        // sürükleme. Kapı bunu da eler, yoksa Cmd-C kullanıcının panosunu
        // boşaltırdı. Pano gerektirmeyen kısım her ortamda ölçülür
        // (`Some("")` hiç yazmaz), gerisi canlı pano ister.
        let fresh = NSPasteboard::pasteboardWithUniqueName();
        assert!(!copy(&fresh, Some(String::new())));

        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(copy(&board, Some("önce".to_owned())));
        assert!(!copy(&board, Some(String::new())));
        assert_eq!(read(&board).as_deref(), Some("önce"));
    }

    #[test]
    fn pending_copy_asks_for_one_job_and_keeps_the_last_text() {
        // Durmadan OSC 52 basan uygulama: yüz metin, tek iş, son metin.
        let slot = PendingCopy::default();
        let jobs = (0..100).filter(|i| slot.put(format!("metin {i}"))).count();
        assert_eq!(jobs, 1, "art arda gelen metinler tek iş istemeli");
        assert_eq!(slot.take().as_deref(), Some("metin 99"));
        assert_eq!(slot.take(), None);
        // İş yuvayı boşalttıktan sonraki metin yeniden iş ister: aksi hâlde
        // ikinci kopya hiç panoya ulaşmazdı.
        assert!(slot.put("sonra".to_owned()));
        assert_eq!(slot.take().as_deref(), Some("sonra"));
        // Alınmamış metinle düşen yuva onu serbest bırakıyor (sızıntıyı
        // `Drop` kapatıyor; burada yalnız paniksiz düştüğü görülüyor).
        assert!(slot.put("alınmadı".to_owned()));
        drop(slot);
    }

    #[test]
    fn pending_copy_delivers_to_the_given_board() {
        let _pasteboard = pasteboard_lock();
        // Dışarıdan verilen pano, genel pano değil: sınama kullanıcının
        // panosuna dokunmaz.
        let slot = PendingCopy::default();
        let fresh = NSPasteboard::pasteboardWithUniqueName();
        // Boş yuva panoya yazmaz — pano gerektirmeyen yarı. İşin iki adımı
        // üretimdeki gibi: yuvadan al (`take`), panoya yaz (`copy`, sahibin
        // varsayılan kolu).
        let deliver = |board: &NSPasteboard| copy(board, slot.take());
        assert!(!deliver(&fresh));

        let Some(board) = board() else {
            eprintln!("{HEADLESS}");
            return;
        };
        assert!(slot.put("ilk".to_owned()));
        assert!(!slot.put("son".to_owned()));
        assert!(deliver(&board));
        assert_eq!(read(&board).as_deref(), Some("son"));
        // Yuva boşaldı: ikinci iş panoyu el değmeden bırakır.
        assert!(!deliver(&board));
        assert_eq!(read(&board).as_deref(), Some("son"));
    }

    #[test]
    #[ignore = "make test-yaris ile koşar"]
    fn race_pending_copy_put_and_take() {
        // Üretimin şekli: tek okuyucu thread koyar, "ana kuyruk" thread'i
        // `put`'un istediği her iş için bir kez alır. İki değişmez:
        // - **Kuyrukta en çok bir bekleyen iş.** `waiting` işin gönderilişiyle
        //   ana thread'in onu alışı arasını sayıyor; `put` iş istediğinde
        //   sıfır olmalı. Her çağrıda iş isteyen bir `put` kuyruğu sınırsız
        //   büyütürdü ve burada düşer.
        // - **Son metin kaybolmaz:** dolu yuva hep bekleyen bir işe sahip;
        //   bozulursa son metin işsiz yuvada kalır.
        use std::sync::atomic::AtomicUsize;
        use std::sync::{Arc, mpsc};

        const TEXTS: usize = 20_000;
        let slot = Arc::new(PendingCopy::default());
        let waiting = Arc::new(AtomicUsize::new(0));
        let (jobs, queue) = mpsc::channel::<()>();
        let main = {
            let slot = Arc::clone(&slot);
            let waiting = Arc::clone(&waiting);
            std::thread::spawn(move || {
                let mut last = None;
                for () in queue {
                    // Sıra üretimdeki gibi: iş önce başlar, sonra yuvayı alır.
                    waiting.fetch_sub(1, Ordering::SeqCst);
                    if let Some(text) = slot.take() {
                        last = Some(text);
                    }
                }
                last
            })
        };
        let mut sent = 0usize;
        for i in 0..TEXTS {
            if slot.put(i.to_string()) {
                let before = waiting.fetch_add(1, Ordering::SeqCst);
                assert_eq!(before, 0, "{i}. metin ikinci bir bekleyen iş istedi");
                jobs.send(()).expect("ana thread yaşıyor");
                sent += 1;
            }
        }
        drop(jobs);
        let last = main.join().expect("ana thread paniklemedi");
        assert_eq!(last.as_deref(), Some((TEXTS - 1).to_string().as_str()));
        assert_eq!(slot.take(), None, "son metin işsiz yuvada kaldı");
        assert!(sent > 0);
    }
}
