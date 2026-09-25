# Phase 2 — Okuyucu döngünün sahibi `bt-core`

## Özet

alacritty'nin PTY okuyucu döngüsü `bt-core`'a kapsüllenmiş bir kopya olarak
geçer ve ayrıştırıcı `Term`'i `Handler`'ı aktaran bir sarmalayıcıdan görür;
davranış bayt bayt bugünkü.

_Requirements: R2, R2.1, R2.2_

## Değişiklikler

- **`Cargo.toml`** — `alacritty_terminal = "=0.26.0"` (kopya o sürümün
  döngüsü ve vte'si; `Cargo.lock` değişmemeli).
- **`crates/bt-core/src/` (yeni modül, ör. `reader.rs`)** — alacritty
  `event_loop.rs`'in uyarlanmış kopyası: `Msg`, `State`, gönderici, `spawn`
  → `JoinHandle<(…, State)>` şekli **aynı**, yani `Session::shutdown` /
  `begin_shutdown` ve `Teardown` akışı yerinden oynamıyor. Dosyanın başında
  Apache-2.0 §4(b) bildirimi (kaynak, sürüm, değiştirildiği). Korunacak
  sözleşmeler: lease'in `pty_read` boyunca tutulması (modül başlığındaki
  `term` → `shell` kilit sırası buna dayanıyor), `MAX_LOCKED_READ`,
  `Wakeup` kuralı (`sync_bytes_count() < processed && processed > 0`),
  DEC 2026 zaman aşımında `stop_sync`'in de **sarmalayıcıya** verilmesi.
- **Sarmalayıcı** (`ClusterHandler` ya da benzeri) — `&mut Term<Adapter>`
  sarar; bütün `Handler` metotları tek `macro_rules!` listesinden aktarılır
  ve `impl`'in üstünde `#[deny(clippy::missing_trait_methods)]`. `input` bu
  phase'de de düz aktarım.
- **`crates/bt-core/src/session.rs`** — `EventLoop::new` / `spawn` yerine
  yeni döngü; `TappedPty` aynen kalır ve doc'u ("tek araya giren") bugüne
  çevrilir; `Reader` tipi, modül başlığının `_terminal_lease` atfı ve
  `EventLoop`'a atıf yapan yorumlar yeni döngüye bağlanır.
- **`assets/bundle/`** — değişiklik gerekmiyor (alacritty zaten lisans
  metninde); gerekiyorsa Uygulama Notları'na yaz.

## Kabul

- Bütün `bt-core` sınamaları, `shutdown_returns_within_limit`,
  `a_dropped_shutdown_handle_still_finishes_the_teardown` ve `race_*`
  değişmeden yeşil.
- Bir aktarımı silmek `make clippy`'yi kırmızıya çeviriyor (bir kez elle
  sınanıp geri alınır; Uygulama Notları'na bir satır).
- DEC 2026 blok içindeki çıktı zaman aşımında da uygulanıyor (sınama).
- `make duman` yeşil ve jetonlar önceki koşunun şeklinde.

## Checklist

- [ ] Sürüm sabitlendi, `Cargo.lock` değişmedi
- [ ] Döngü kopyası + Apache bildirimi
- [ ] Sarmalayıcı: makro listesi + `missing_trait_methods`
- [ ] `stop_sync` ve `Wakeup` kuralı
- [ ] `session.rs` bağlandı, doc'lar güncel
- [ ] Test: DEC 2026 zaman aşımı sarmalayıcıdan geçiyor
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
