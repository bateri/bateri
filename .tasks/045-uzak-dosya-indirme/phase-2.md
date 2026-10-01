# Phase 2 — Kuyruk iki yönlü, indirme süreci

## Özet

`Uploads` iki yönü ve üç şeridi taşıyan `Transfers`'a genelleşir, `ssh … tar
c | tar x` indirmesi eklenir; upload bayt bayt aynı kalır.

_Requirements: R2, R2.1, R2.2, R2.3_

## Değişiklikler

- **`crates/bt-shell-common/src/upload.rs`** (gerekirse `transfer.rs`'e
  bölünür) — `Job`'a yön ve şerit; kuyruk tek sıralı şeridi sürer, önizleme
  ve Finder şeritleri sıra beklemeden başlar ama satır özeti, toplamlar,
  liste, durdurma sorusu ve ⌘. hepsini sayar (Karar 6, 7). Metinler yönü
  okur (`titled` `↑`/`↓`, özet `↑1 ↓2`, sonuç ve bildirim satırları, "Stop
  downloading?", liste başlığı); `RowAction`'a "Show in Finder" / "Open".
- **`crates/bt-shell-common/src/download.rs`** (yeni) — `transfer` aynası:
  uzakta `tar c`, yerelde `/usr/bin/tar x -C {geçici}`; `TarWatcher` ve
  `Shared` aynen; bitince `rename` hedefe (çakışma kararı çağırandan:
  Keep both → "ad 2", Replace → eskinin üstüne), iptal/hata/disk dolu
  geçiciyi siler. Uzak mtime tar'dan korunur.
- **`crates/bt-shell-macos/src/uploader.rs`** — popover "Show transfers",
  satırda yön oku, biten indirmede "Show in Finder"
  (`activateFileViewerSelectingURLs`), önizlemede "Open"; Dock simgesi ve
  başlık toplamları iki yönden; karantina etiketi (`NSURL`
  `NSURLQuarantinePropertiesKey`, Karar 15) indirmenin bitişinde.
- **`crates/bt-shell-macos/Cargo.toml`** — gereken `objc2-foundation`
  bayrakları; `Cargo.lock` değişirse dur (Karar 15).

## Kabul

- Upload sınamaları değişmeden geçer (R2.3).
- Yeni sınamalar: karışık kuyrukta sıra (önizleme ve Finder şeridi beklemez,
  kuyruk birer birer), özet `↑1 ↓2`, başlık `↓ N%`, durdurma sorusunun iki
  yönü, yerel `tar` ile gerçek bir indirme (uzak yerine yerel `sh -c 'tar
  c …'` enjekte edilir) — geçici → rename, iptalde geçicinin silinmesi,
  Keep both adı.
- `make check` yeşil; `make linux` yeşil.

## Checklist

- [ ] `Transfers`: yön + şerit, metinler
- [ ] `download.rs`: süreç, geçici ad, çakışma, temizlik
- [ ] uploader: popover, Show in Finder/Open, Dock simgesi, başlık, karantina
- [ ] Test: Kabul listesi
- [ ] Doğrulama geçti (`make check`, `make linux`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (yalnız `Cargo.lock` değiştiyse kalır)
