//! Çekirdekten dış dünyaya tek yönlü sinyal.

/// Okuyucu thread'in dış dünyaya haber verme yolu.
///
/// Çağrılar **okuyucu thread'de** gelir ve alacritty'nin `Term` kilidi
/// TUTULURKEN gelebilir (`pty_read` tamponu ayrıştırdıktan sonra kilidi hâlâ
/// elinde tutarken `Wakeup` yollar). Uygulayan bu yüzden üç şey yapmaz:
/// `Session`'a geri girmez, bloklamaz, kilit almaz — yalnız başka bir
/// thread'e "bir şey oldu" der. `bt-gpu`'daki `Waker` bunun karşılığıdır:
/// ana kuyruğa tek bir iş atar. Tek istisna **yaprak** kilit: alınıp hemen
/// bırakılan, altında başka kilit alınmayan bir yuva (`Theme`'in yaprak
/// kilidi emsali; üretimde `ShellWake`'in sökülebilir `Waker` yuvası).
///
/// **Sahiplik:** `Session` bu nesneyi `Arc` ile tutar. Uygulayan da
/// `Arc<Session>` tutarsa çember kapanır: `Drop for Session` hiç koşmaz,
/// okuyucu thread hiç `join` edilmez ve sekme başına bir PTY ile bir thread
/// sızar. `Session`'a bakmak gerekiyorsa `Weak` ile bakılır — ve `wake()`
/// içinden `shutdown()`'a varan **senkron** bir yol açılmaz. Yasak duruyor,
/// bedeli değişti: `shutdown()` artık `join`'i ayrı bir thread'e alıp
/// sınırlı beklediği için son güçlü referans okuyucu thread'de düşse bile
/// `EDEADLK` paniği olmaz — onun yerine o thread yarım saniye durur, bir
/// satır "arkada bırakıldı" basılır ve kapanış hiç bitmez.
///
/// **`Drop`'un koştuğu thread sözleşmenin parçası.** Sınır dolduğunda
/// `(EventLoop, State)` çifti `"PTY teardown"` thread'inde kalır ve o çift
/// `Adapter` üzerinden bu nesnenin bir `Arc` kopyasını taşır: son kopya
/// oraya düşerse **`Wake::drop` o thread'de koşar**. Dolayısıyla uygulayanın
/// `Drop`'u da bloklamaz — özellikle ana kuyruğa senkron iş atmaz. Üretimdeki
/// uygulayan `bt-shell`'in `ShellWake`'i ve taşıdığı `bt-gpu` `Waker`'ında tam
/// böyle bir alan var (`MainThreadBound<Retained<CAMetalDisplayLink>>`); bu
/// yüzden `Waker` pencere kapanırken ana thread'de **sökülüyor** ve
/// `ShellWake` hangi thread'de düşerse düşsün onu taşımıyor.
///
/// Çağrıların hiçbirinin varsayılan gövdesi yok: yeni bir çağrı eklendiğinde
/// uygulayan onu unutamasın, derleme söylesin.
pub trait Wake: Send + Sync + 'static {
    /// Grid değişti; bir kare gerekebilir.
    fn wake(&self);

    /// Shell çocuğu bitti. `code` yalnız normal çıkışta doludur; sinyalle
    /// ölen çocukta `None`'dur.
    fn child_exit(&self, code: Option<i32>);

    /// Terminaldeki uygulama OSC 52 ile panoya `text` yazmak istedi (ssh'taki
    /// vim'in kopyası). Yalnız `Osc52::Copy` kipinde gelir; `text` boş
    /// değildir. Dizinin hedefi (`c`, `p`, `s`) taşınmıyor: tek panolu bir
    /// platformda ayrım yok (`Adapter`'ın kolu).
    ///
    /// Üstteki üç yasak burada da geçerli ve en çok burada sınanır: panoya
    /// yazmak `Term` kilidi altında yapılamayacak kadar yavaş olabilir (metin
    /// sınırsız), yani uygulayan metni kilitsiz bir yuvaya koyup yazmayı başka
    /// bir thread'e bırakır. Durmadan OSC 52 basan bir uygulama bu çağrıyı
    /// saniyede yüzlerce kez yapabilir; uygulayanın kuyruğa sınırsız iş
    /// yığmaması onun işi.
    ///
    /// Kapanış sırasında gelen yazmanın panoya ulaşmaması zararsızdır.
    fn copy_to_clipboard(&self, text: String);

    /// Oturumun başlığı ([`crate::Session::title`]) değişmiş olabilir:
    /// uygulamanın OSC 0/2 başlığı değişti ya da kabuğun OSC 7 dizini
    /// **değişti** (aynı dizini basan `precmd` haber doğurmaz).
    ///
    /// İki kaynağın iki thread durumu var: OSC 0/2 `Term` kilidi **altında**
    /// gelir (okuyucu thread'de ya da `Session::set_terminal_options`'ı
    /// çağıran thread'de — `Term::set_options` başlık olayını yeniden
    /// yolluyor, değişmediyse haber yok), OSC 7 okuyucu thread'de kilitsiz.
    /// Üstteki üç yasak ikisinde de geçerli.
    ///
    /// **Yük taşımaz:** alıcı başlığı `Session::title`'dan kendisi okur, yani
    /// birbirini kovalayan iki değişiklik bayat bir değerle davranamaz.
    /// Uygulayan kuyruğa **en çok bir** iş atar — başlığını her komutta
    /// basan bir kabuk ya da döngüdeki `printf` çağrıyı sık yapabilir ve
    /// görülecek olan zaten son başlık.
    fn title_changed(&self);

    /// Geçmişte arama açıkken **defter değişti** (033): PTY çıktısı geldi
    /// ya da pencere yeniden sarıldı. Alıcı sayım dizinini sürer
    /// ([`crate::Session::search_step`]); dizin bir sonraki geçişini baştan
    /// başlatıyor.
    ///
    /// **Kenarda ve yüksüz** ([`Wake::title_changed`] emsali): bekleyen haber
    /// dizin onu tüketene kadar ikincisini doğurmuyor, yani `yes` akarken de
    /// çağrı sayısı geçiş sayısıyla sınırlı. Okuyucu thread'de `Term` kilidi
    /// **tutulurken** ya da ana thread'de (`resize`) gelir; üstteki üç yasak
    /// geçerli. Arama kapalıyken hiç gelmez.
    fn search_changed(&self);

    /// Kabuğun safhası `Running`'e **geçti** (OSC 133 `C`; 036 Karar 2): bir
    /// komut başladı. Alıcı ön plandaki programı yoklayıp
    /// [`crate::Session::set_remote`] ile uzak oturumu bildirir.
    ///
    /// **Kenarda ve yüksüz** ([`Wake::title_changed`] emsali): aynı komutta
    /// ikinci bir `C` (iTerm2 entegrasyonu) geçiş değil ve haber doğurmuyor.
    /// Yük yok, çünkü alıcı komutun neslini kendisi okuyor
    /// ([`crate::Session::running_command`]) ve yoklamanın cevabını onunla
    /// geri veriyor — araya bir `D` girerse bayat cevap düşüyor. Okuyucu
    /// thread'de gelir; defterin yaprak kilidi bırakıldıktan sonra, ama
    /// sözleşme `Term` kilidinin tutulabileceğini varsayar ve üstteki üç yasak
    /// geçerli. Uygulayan kuyruğa **en çok bir** iş atar.
    fn command_started(&self);
}
