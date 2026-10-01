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

- [x] `Transfers`: yön + şerit, metinler
- [x] `download.rs`: süreç, geçici ad, çakışma, temizlik
- [x] uploader: popover, Show in Finder/Open, Dock simgesi, başlık, karantina
- [x] Test: Kabul listesi
- [x] Doğrulama geçti (`make check`, `make linux`)
- [~] Riskli phase: `/code-review` — `Cargo.lock` değişmedi, tetiklenmedi

## Uygulama Notları

- `Job`'a tek alan girdi: `way: Way` (`Up` | `Down { lane, conflict }`); iki yönde de `dir` uzak klasör, indirmede `local.path` iniş yolu, `local.name` uzak ad. `Item`'a `landed` (Keep both yeniden adlandırmış olabilir; "Show in Finder"/"Open" onu açar). Upload sınamalarında yalnız iki fixture kurucusu (`job()`, `entries()`) yeni alanı aldı ve tip adı `Uploads` → `Transfers`; tek bir doğrulama satırı değişmedi.
- Eski imzalar kuyruk şeridinin ince sarmalayıcısı kaldı (`start_next`, `finish`, `running_id`, `stop_request`); `uploader` yalnız kimlikli biçimleri kullanıyor (`start`, `finish_item`, `is_running`, `stop_request_item`). Kuyruk **hiçbir öğe akmıyorken** biter: kuyruk şeridi hata verse de akan önizlemenin raporu kaybolmaz; tek öğe akarken kural bugünküyle aynı.
- Satırın öznesi kuyruk şeridinin öğesi, yoksa en uzun akan; `↑1 ↓2` özeti yalnız karışık kuyrukta, tek yönlüde bugünkü `↑ k of n` (`↓` indirmede). Başlık oku akan öğelerden (`↑`, `↓`, karışıkta `↑↓`); `(ok, yüzde)` birlikte saklanıyor, ok aynı yüzdede dönebilir.
- Durdurma sorusu: `all`'da konu en uzun akan öğe; metin diğer akanları "N other transfer(s) stop(s)", biten indirmeleri "stays on this Mac" diye sayar; başlık ve varsayılan düğme yönden ("Stop downloading?", "Stop all transfers?", `Transfers::keep_label`).
- İndirmenin dolu diski yerel: `End`'e kol eklenmedi, `Outcome::DiskFull` indirmede `End::Failed("disk full on this Mac")`.
- `↓` dock satırına girdiği için `bt-core`'un `UPLOAD_GLYPHS`'ına ve `bt-atlas`'ın elle kopyasına eklendi (üç yer; Menlo küçük sınıfta kutu değil — `the_upload_row_has_no_box_in_the_small_class` yeşil).
- Karantina `download::transfer`'ın `seal` kancasında, öğe henüz gizli geçici klasördeyken (akış thread'inde; klasörde her giriş). `objc2-foundation`'ın varsayılan setinde (`NSURL`, `NSDictionary`) — `Cargo.toml` ve `Cargo.lock` değişmedi.
- Bilinen sınır: uzak giriş kabuğunun rc dosyası stdout'a yazarsa (`.bashrc`'de `echo`) tar akışı bozulur ve indirme yerel tar'ın satırıyla düşer — `scp`'nin de sınırı.

