# Phase 2 — `bt-shell-common` doğuyor, on bir modül taşınıyor

## Özet

Yeni workspace üyesi `bt-shell-common` on bir modülü alır; macOS gövdeleri
`cfg(target_os = "macos")` arkasında, yeni Linux kodu yok. `bt-shell`
(henüz adı değişmiyor) ona bağlanır; davranış aynı.

_Requirements: R2.1, R2.2, R2.3, R2.4_

## Değişiklikler

- **`crates/bt-shell-common/`** (yeni) — `Cargo.toml` (`bt-core`, `bt-gpu`,
  `libc`; `[target.'cfg(target_os = "macos")'.dependencies] dispatch2`;
  `test-support = []` özelliği) ve `lib.rs` (İngilizce başlık: crate'in
  sınırı — AppKit'siz, platform gövdeleri `cfg`'li ve adlı —, katman yeri,
  discussion.md'ye işaretçi). Modüller `git mv` ile taşınır; `pub(crate)` →
  `pub` yalnız crate dışından kullanılan öğelerde.
- **Platform gövdeleri** — `jobs::Libproc` ve `libc::proc_*`/`sysctl`
  çağrıları, `watch` modülünün tamamı (bugün yalnız `dispatch2` gövdesi var; Linux gövdesi phase-4), `child`'ın `login_argv`'si ve
  paket yolu (`bundle_shell_dir`) `cfg(macos)`; bu phase'te yalnız macOS'ta
  derlendiği için Linux'u derlemek iddia edilmez (phase-3). `child`'ın
  `NSLocale` okuması **`bt-shell`'e** iner: ortak `locale_env` sistemin
  `(dil, bölge)` çiftini argüman alır (Karar 2), `primary_language` onunla
  birlikte gider ya da ortakta `pub` kalır — `bt-shell`'in çağrısı aynı
  değeri üretir.
- **Sınama yardımcıları** — `settings::TempRoot`, `child::SilentWake` ve
  `wait_until` `#[cfg(any(test, feature = "test-support"))]`; `bt-shell`'in
  `[dev-dependencies]`'i ortak crate'i `test-support` ile alır (`bt-atlas`
  `fixture` emsali).
- **`crates/bt-shell/`** — `Cargo.toml`'a `bt-shell-common`; `lib.rs`'in
  `mod` satırları gider, `use bt_shell_common::…` gelir; `libc`/`dispatch2`
  kullanımı kalan modüllerin ihtiyacına göre. Taşınan modüllere bakan doc
  yolları (`crate::settings::…`) güncellenir.
- **Kök `Cargo.toml`** — `members` ve `[workspace.dependencies]`'e
  `bt-shell-common`.
- **Belge işaretçileri ve sözleşme** (`CLAUDE.md` → "aynı commit'te
  düzelir") — `docs/AYARLAR.md` (`crates/bt-shell/src/settings.rs`,
  `child.rs`); `CLAUDE.md`'nin katman diyagramı `bt-shell-common`'ı kazanır,
  tabloya onun satırı girer (sorumluluk: taşınan modüller; platform:
  AppKit/Foundation yok, `dispatch2` yalnız `watch`'ın macOS gövdesi,
  `libc`) ve `bt-shell` satırı taşınanları bırakır; taşınan modüllere dosya
  yoluyla bakan cümleler. Adlandırma phase-5'in. `app.rs`'e
  bakanlar (`docs/OLCUMLER.md`, `.claude/is-akisi/olcum.md`) bu phase'te
  değişmez — `app.rs` yerinde.

## Kabul

- macOS'ta `cargo test --workspace -- --list`: sınama adları, crate
  başlıkları (`Running …`) atılıp modül yolu karşılaştırılarak, phase-1'le
  aynı küme (yalnız crate değişti).
- `Cargo.lock` farkında `source =` satırı yok (yalnız yol üyesi ve
  kenarlar); `make denetim`'in Cargo uyarısı beklenen hâl.
- `make hepsi` yeşil.

## Checklist

- [x] `bt-shell-common` üyesi ve on bir modül taşındı
- [x] macOS gövdeleri `cfg(macos)`; `NSLocale` okuması `bt-shell`'de
- [x] `test-support` özelliği ve dev-dependency
- [x] Belge işaretçileri güncel
- [x] Test: sınama adları kümesi phase-1'le aynı
- [x] Test: `Cargo.lock`'ta dış crate yok
- [x] Doğrulama geçti (`make hepsi`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (kilit dosyası değişti)

## Uygulama Notları

- **Doc yolları yeniden yazılmadı:** `bt-shell`'in kökünde
  `use bt_shell_common::{child, …, zoom};` var, yani `crate::settings::…`
  yolları (kod ve doc) olduğu gibi çözülüyor; taşınan modüllerin kendi
  `crate::` yolları ortak crate'in içinde zaten doğru.
- **`pub`'a açılan kümenin ölçüsü:** önce derleyicinin "hiç kullanılmıyor" ve
  "özel" tanıları, sonra `bt-shell`'de adı geçmeyen on üç öğe tek tek geri
  `pub(crate)`'e indirilip `clippy -D warnings` ile sınandı; sekizi indi
  (`click_kind`, `feed`, `format_bytes`, `parse_probe`, `probe_failure`,
  `probe_script`, `progress`, `SSH_VALUED`), beşi imzada göründüğü için `pub`
  (`Groups`, `ProcessTable`, `ListRow`, `Sheet`, `Target`).
- **`cfg(macos)` phase metninden geniş:** `login_argv` ve `bundle_shell_dir`'in
  yanında giriş zincirinin tamamı (`login_command`, `login_command_from`,
  `passwd_name`) ve `jobs`'ta `parse_procargs` ile üç sınama
  (`procargs_layout_…`, iki gerçek süreç tablosu sınaması) — hepsi macOS
  gövdesinin parçası; macOS'ta sınama adları aynı. Linux kolları phase-3'ün.
- **`NSLocale` okuması** `bt-shell`'in yeni `locale` modülünde
  (`system_locale`); ortak `locale_env(system)` çifti argüman alıyor,
  `primary_language` ortakta `pub` ve sınaması yerinde.
- **`test-support` dev-dependency'si bugün tüketicisiz:** `bt-shell`'in
  sınamaları bu yardımcılara uzanmıyor; bağ planın istediği gibi kurulu.
- **Bilerek bırakılanlar:** `child.rs`'te `locale_installed` doc'undaki
  "`bt-shell` builds only on macOS" cümlesi phase-3'ün (kolun Linux doc'u),
  `bt-shell/src/lib.rs` başlığının taşınan modülleri sayması phase-5'in
  (yeniden yazılan başlık).
- **`/code-review` bulgusu:** `bundle_shell_dir`'in `cfg(macos)`'u
  `zsh_wrapper_dir`'i Linux'ta çözümsüz bırakıyordu; `cfg(not(macos))` kolu
  `None` veriyor (`repo_wrapper_dir` emsali) — phase-3'ün planladığı şekil.
- `make duman` tetiklenmiyordu (davranış değişmedi); yine koşuldu, yeşil.
