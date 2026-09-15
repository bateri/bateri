//! Çekirdekten dış dünyaya tek yönlü sinyal.

/// Okuyucu thread'in dış dünyaya haber verme yolu.
///
/// Çağrılar **okuyucu thread'de** gelir ve alacritty'nin `Term` kilidi
/// TUTULURKEN gelebilir (`pty_read` tamponu ayrıştırdıktan sonra kilidi hâlâ
/// elinde tutarken `Wakeup` yollar). Uygulayan bu yüzden üç şey yapmaz:
/// `Session`'a geri girmez, bloklamaz, kilit almaz — yalnız başka bir
/// thread'e "bir şey oldu" der. `bt-gpu`'daki `Waker` bunun karşılığıdır:
/// ana kuyruğa tek bir iş atar.
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
/// `Drop`'u da bloklamaz — özellikle ana kuyruğa senkron iş atmaz (üretimdeki
/// uygulayan `bt-shell`'in `ShellWake`'i; taşıdığı `bt-gpu` `Waker`'ında tam
/// böyle bir alan var: `MainThreadBound<Retained<CAMetalDisplayLink>>`).
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
}
