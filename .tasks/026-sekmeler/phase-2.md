# Phase 2 — `bt-core`'u hazırla ve başlığı bağla

## Özet

Kapanış başlat/bekle diye bölünür, oturum OSC 7 dizinini verir, başlık bir
yaprak yuvada birikir ve değişince `Wake` haber verir; tek pencere artık
uygulamanın ya da dizinin başlığını gösterir.

_Requirements: R2, R2.1, R2.2, R2.3, R2.4, R5_

## Değişiklikler

- **`crates/bt-core/src/session.rs` — kapanış** — `Session::shutdown`'ın
  bugünkü gövdesi iki parçaya ayrılır: başlatan (`Msg::Shutdown` +
  "PTY teardown" thread'i, kanalın alıcısını taşıyan bir tutamak döner;
  ikinci çağrı `None`) ve tutamağın verilen son tarihe kadar bekleyen
  yöntemi (`Teardown` döner). `shutdown()` = başlat + `now + SHUTDOWN_GRACE`
  bekle; doc'u, altı `Teardown` kolu ve `Unbounded` dalı (thread
  kurulamazsa) aynen. Tutamak beklenmeden düşerse teardown thread'i işini
  yine bitirir — sınama bunu söyler.
- **`session.rs` — dizin** — `Session::working_directory() -> Option<PathBuf>`:
  `ShellLog::context.cwd`'yi yaprak kilit altında kopyalar; boşsa `None`.
  `Term` kilidine dokunmaz (`shell_state` örüntüsü).
- **`session.rs` — başlık** — `Adapter`'ın `Event::Title`/`ResetTitle` kolu
  artık boş değil: başlığı `AdapterInner`'da bir yaprak `Mutex`'e yazar (ya
  da siler) ve değiştiyse `Wake::title_changed()`'i çağırır. Kol `Term`
  kilidi altında geliyor; yaprak kilidi orada almanın emsali `ColorRequest`
  ve `blink` — kilit sırası `Term` → yaprak, tersi yok.
  `set_terminal_options`'ın doc'undaki "o kol bugün boş" cümlesi düzeltilir.
- **`TappedPty`** — OSC 7 kolu `ShellLog`'a yazdığı dizin **değiştiyse**
  `Wake::title_changed()` çağırır (`TappedPty` `Arc<dyn Wake>`'e erişim
  kazanır). Dizin değişmeyen `precmd` haber doğurmaz.
- **`crates/bt-core/src/shell.rs` ya da `session.rs`** — saf başlık kuralı:
  `(osc_title: Option<&str>, cwd: Option<&str>, home: Option<&Path>) -> String`;
  boş OSC başlığı yok sayılır, ev dizini `~`, kök `/`, hiçbiri yoksa
  `bateri`. `Session::title()` iki yuvayı okuyup kurala verir; ev dizini
  `SessionOptions`'tan (bt-core ortam okumaz — değeri `bt-shell` verir).
- **`crates/bt-core/src/wake.rs`** — `fn title_changed(&self);` (varsayılan
  gövde yok, trait kuralı). Doc: okuyucu thread'de ve `Term` kilidi altında
  gelebilir, uygulayan yük taşımaz ve kuyruğa en çok bir iş atar.
- **`crates/bt-shell/src/app.rs` / `window.rs`** — `ShellWake::title_changed`:
  atomik bir "bekliyor" bayrağı + `exec_async` (`PendingCopy` örüntüsü,
  yeni mekanizma değil); ana kuyruktaki iş bayrağı indirir,
  `session.title()`'ı okur ve `NSWindow::setTitle` yazar. `ShellWake` hangi
  pencereye yazacağını pencere kimliğinden bulur (phase-1'in haberci
  örüntüsü). Kapanış yolu `shutdown()`'ı çağırmaya devam eder — paralellik
  phase-3'te.
- **Test yardımcıları** — `bt-core`'daki `TestWake` yeni metodu sayar.

## Kabul

- Sınamalar: başlık kuralının kolları (OSC başlığı kazanır, boş OSC dizine
  düşer, ev `~`, kök `/`, hiçbiri → `bateri`, `ResetTitle` dizine döner);
  `title_changed` OSC 0/2 ile ve **değişen** OSC 7 ile geliyor, aynı dizini
  basan ikinci `precmd` ile gelmiyor; başlat/bekle bölünmesi `shutdown`'ın
  mevcut sınamalarını (`shutdown_returns_within_limit`,
  `shutdown_with_busy_writer`) aynen geçiriyor; beklenmeden düşen tutamak
  çocuğu yine topluyor.
- `race_set_terminal_options_and_frame` (başlık kolu artık dolu) ve öteki
  `race_*` sınamaları yeşil.
- `make duman` jetonları aynı (`hucre=8 glif=6 kural=15`, `kapanis=clean`).
- Elle: zsh'te `cd /tmp` → pencere başlığı `tmp`; `printf '\e]2;selam\a'` →
  `selam`; vim/Claude Code kendi başlığını gösteriyor.

## Checklist

- [x] `Session`'da kapanış başlat/bekle; `shutdown()` davranışı aynı
- [x] `Session::working_directory()`
- [x] Başlık yaprak yuvası, saf kural, `Session::title()`
- [x] `Wake::title_changed` iki kaynaktan; `ShellWake` ≤1 iş ile pencereye yazıyor
- [x] `wake.rs`, `set_terminal_options` doc'u ve `CLAUDE.md`'nin `bt-core`
      satırı (OSC 0/2 artık başlığa gidiyor) aynı commit'te
- [x] Test: başlık kuralı, haber kaynakları, bölünmüş kapanış
- [~] Doğrulama geçti (`make hepsi` + `make test-yaris` yeşil; `make duman`
      ortam (bkz. phase-1): HEAD'de de aynı `MotionUnsettled`, sessiz ~917 ms)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (tek bulgu: başlık
      sınamasının ilk okuması yarışlıydı, betiğe açılış beklemesi eklendi)

## Uygulama Notları

- **Tutamak tipi `ShutdownHandle`** (`bt-core` ihraç ediyor):
  `Session::begin_shutdown() -> Option<ShutdownHandle>` ve
  `ShutdownHandle::wait_until(Instant) -> Teardown`. Son tarih süre değil
  **an** — phase-3'ün paralel ⌘Q'su bütün tutamakları aynı son tarihe kadar
  bekleyecek. Thread kurulamayan dal tutamağın içinde (`Unbounded`),
  stderr satırı başlatırken.
- **OSC 7'nin "değişti" bilgisi `ShellLog::apply_scan_answering`'in
  dönüşü** (`bool`); `TappedPty` haberi defterin kilidi düştükten sonra
  veriyor ve `Adapter`'ın `Wake`'inin bir kopyasını taşıyor. `#[cfg(test)]
  apply_scan` `()` dönmeye devam ediyor (onlarca sınama çağrı yeri).
- **Başlık kuralı `shell::title_of`**, ev dizini `SessionOptions::home`
  (yeni alan; `bt-shell` `child::home()` veriyor).
- **Pencere oturum yuvasına girer girmez başlığı bir kez okuyor**
  (`start_session` → `refresh_title`): yuvadan önce gelen bir haber boş
  yuva bulup düşmüş olabilir.
- **Bilinen sınır:** RIS (`\ec`) alacritty'nin başlığını olaysız siliyor
  (`Term::reset_state`), yuva bir sonraki `set_options`'a ya da OSC 0/2'ye
  kadar eski başlığı tutuyor.
- `ResetTitle` sınaması başlık yığınıyla (`CSI 22 t` / `CSI 23 t`) kuruldu:
  boş OSC 2 `Some("")` doğuruyor, `ResetTitle` değil.
