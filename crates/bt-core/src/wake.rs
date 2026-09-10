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
/// içinden `shutdown()`'a varan **senkron** bir yol açılmaz: son güçlü
/// referans okuyucu thread'in içinde düşerse `join` thread'i kendi kendine
/// bekletir (`EDEADLK`) ve `Drop` içinde panik olur.
pub trait Wake: Send + Sync + 'static {
    /// Grid değişti; bir kare gerekebilir.
    fn wake(&self);

    /// Shell çocuğu bitti. `code` yalnız normal çıkışta doludur; sinyalle
    /// ölen çocukta `None`'dur.
    fn child_exit(&self, code: Option<i32>);
}
