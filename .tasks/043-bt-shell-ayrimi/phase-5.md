# Phase 5 — `bt-shell` → `bt-shell-macos`, denetim ve sözleşme

## Özet

AppKit crate'i adını alır, `make denetim` yeni katman kuralını ve ortak
crate'in platform sınırını denetler, sözleşme belgeleri güncellenir; set
kapısı (`discussion.md` → Karar 2, 5).

_Requirements: R4.1, R4.2, R4.3, R4.4_

## Değişiklikler

- **`crates/bt-shell` → `crates/bt-shell-macos`** (`git mv`), paket adı
  `bt-shell-macos`; `lib.rs` başlığı İngilizce yeniden yazılır (AppKit
  kabuğu, ortak crate'e işaretçi, `pacer` burada). Dosyaların içeriği ve
  yorum dili değişmez (Karar 3).
- **Kök `Cargo.toml`** — `members` ve `[workspace.dependencies]`.
- **`crates/bateri`** — `Cargo.toml` ve `main.rs`'in `bt_shell::` yolları;
  `bundle_assets.rs`'in `bt-shell` anmaları.
- **`Makefile`** — `denetim`: `cargo tree -p bt-shell-common -e normal
  --depth 1`'de `objc2-app-kit|objc2-quartz-core|objc2-foundation|objc2-user-notifications|block2|bt-shell-macos`
  yok; `crates/bt-shell-common/src`'de `objc2|dispatch2|block2` yalnız
  `watch`'ın macOS gövdesinde (yorum satırı hariç); başlık yorumuna kural.
  `bt-shell` anan yorumlar (41, 227, 562).
- **Belgeler** — `CLAUDE.md` (`bt-shell` → `bt-shell-macos` adı: diyagram,
  tablo, Dil istisnası, `crates/bt-shell/` yolları; ortak crate'in satırı
  phase-2/3'te geldi), `.claude/is-akisi/proje.md` (Doğrulama'nın
  denetim satırı), `docs/OLCUMLER.md` ve `.claude/is-akisi/olcum.md`'nin
  `app.rs` yolları, `docs/YOL-HARITASI.md` (043 satırının kapanışı, sonraki
  satırlarda `bt-shell` anmaları gerektiği kadar), `tools/linux/Dockerfile`
  başlığı.

## Kabul

- `make denetim` temiz ve kural sınandı: ortak crate'e geçici bir
  `objc2_app_kit` satırı kırmızı veriyor (geri alındı).
- macOS'ta sınama adları phase-4'ünkiyle aynı (crate öneki hariç).
- `make kur` yeşil (`crates/bateri` değişti); `make duman` yeşil, jetonlar
  aynı anahtarlarla.
- `make linux` yeşil.
- Kapanış mesajının gözle kontrol satırı: paketli uygulamada ayar kaydı
  canlı uygulanıyor, ⌘W koşan işi soruyor, dock var, açılışta `Last login`
  yok.
- `make hepsi` yeşil.

## Checklist

- [ ] Dizin ve paket adı, `bateri`
- [ ] `make denetim` kuralları ve sınanması
- [ ] Belgeler güncel
- [ ] Test: sınama adları phase-4'le aynı
- [ ] Doğrulama geçti (`make hepsi` + `make linux` + `make kur` + `make duman`)
