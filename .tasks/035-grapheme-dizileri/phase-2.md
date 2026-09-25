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

- [x] Sürüm sabitlendi; `Cargo.lock` yalnız `cursor-icon` kenarıyla değişti (karar kaydında)
- [x] Döngü kopyası + Apache bildirimi
- [x] Sarmalayıcı: makro listesi + `missing_trait_methods`
- [x] `stop_sync` ve `Wakeup` kuralı
- [x] `session.rs` bağlandı, doc'lar güncel
- [x] Test: DEC 2026 zaman aşımı sarmalayıcıdan geçiyor
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Bağımlılık kenarı `cursor-icon` (kullanıcı kararı).**
  `Handler::set_mouse_cursor_icon`'un parametresi `cursor_icon::CursorIcon`;
  vte onu `ansi`'de özel bir `use` ile alıyor, ne vte ne alacritty yeniden
  ihraç ediyor, yani aktarım tipi adlandıramıyordu ve kenarsız yol
  `missing_trait_methods`'u kırmızı bırakıyordu (ölçüldü: tek bulgu o
  metot). Kullanıcı `bt-core`'a `cursor-icon = "1.2"` kenarını seçti:
  grafta zaten 1.2.0, `Cargo.lock`'ta yalnız `bt-core`'un listesine bir
  satır. Kayıt `discussion.md` → Karar ve kök `Cargo.toml`.
- **Kopyanın panik yolları düştü** (`bt-core`'da gerekçesiz panik yok):
  kanalın ölümü boş okuma (döngü kendi `tx`'ini tuttuğu için erişilemez),
  olay kapasitesi `const` (sıfır derleme hatası). `reregister`'in paniği
  `// audit:` gerekçesiyle **korundu** (`/code-review`): `break`'e indirmek
  çöküşü `kapanis=`'ten gizliyordu.
  `ref_test` kaydı ve `Notifier` çıkarıldı; `log::error!` → `eprintln!`
  (`log` bağımlılık değil). PTY token'ları alacritty'de `pub(crate)`,
  değerleri kopyalandı — `=0.26.0`'ın bir gerekçesi de bu. Linux `EIO`
  dalı `libc` yerine sayıyla (5), `libc` `bt-core`'un bağımlılığı değil.
- **Sarmalayıcı ayrı modülde** (`handler.rs`), döngü `reader.rs`'te: Apache
  bildirimi yalnız kopyanın dosyasında kalsın, phase-3'ün kümelemesi
  bizim dosyamıza girsin.
- **DEC 2026 sınaması eski döngüde de yeşil** — bir parite bekçisi;
  kırmızısı mutasyonla gösterildi: `stop_sync` çağrısı silinince
  `an_unterminated_synchronized_update_lands_on_timeout` düşüyor. Çağrının
  `Term`'e sarmalayıcıdan gittiğini bugün ayırt edemiyor (sarmalayıcı düz
  aktarım); o ayrım phase-3'ün checklist'ine devredildi.
- **`/code-review` bulguları giderildi:** `Drop` yorumu, CLAUDE.md'nin
  "okuyucu thread alacritty'nin" cümlesi, sınamanın doc bağlantısı,
  `ClusterHandler`'ın ömrü (kilit turu değil `advance` başına), pin
  yorumunun vte iddiası düzeltildi; `cursor-icon` `default-features =
  false` (vte'nin kenarıyla aynı özellik kümesi). Linux `EIO` sayısı (5)
  kaldı: `libc` kenarı ikinci bir bağımlılık kararı olurdu.
- **Lint bekçisi elle sınandı:** listeden `bell` silinince `cargo clippy -p
  bt-core -- -D warnings` "missing trait method provided by default: `bell`"
  ile kırmızı; geri alındı.
