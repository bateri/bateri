# Phase 3 — Çözümleme, açma politikası, jest ön-rotası (`bt-shell-common`)

## Özet

Yol adayını diskteki hedefe çeviren saf `resolve`, ne yapılacağını söyleyen
politika tablosu ve ⌘-tıkın jest defterindeki ön-rotası.

_Requirements: R5, R5.1, R6_

## Değişiklikler

- **`crates/bt-shell-common/src/links.rs`** (yeni) — `resolve(aday, cwd,
  home, stat)`: `~` ev dizinine, göreli yol OSC 7 dizinine, sonek atılmış;
  `stat` enjekte edilir (sınamada sahte, üretimde `std::fs::metadata`),
  sonuç `Dir` / `File { executable }` / yok. `action(hedef, tür) ->
  LinkAction::{OpenUrl, OpenFile, Reveal, OpenDir, Confirm, Swallow}`:
  `plan.md` → R5.1'in beyaz listesi; dosyada "bilinen içerik tipi" sorusu
  argüman (UTType sorgusu AppKit'te, phase-4), yani tablo beyaz listenin
  tümleyenini saf olarak taşır. `bateri://` her yoldan `Swallow`. Makine
  adını okuyan küçük fonksiyon (`libc::gethostname`, `child`'ın emsali).
  Kuyruk, `dispatch2` ve AppKit **yok** (`make audit`).
- **`crates/bt-shell-common/src/gesture.rs`** — `Gesture::pressed_link(aralık)`:
  rotayı basışta kilitler (`Drag::Ignore`), bırakmada `Release::Link`;
  `sent` bitini kurmaz, yani kayıp bırakma yolu (`take_lost_releases`) sızmaz.
  Doc'u yol haritasının "`button_route`'un dördüncü kolu" taslağının neden
  burada olduğunu `discussion.md` → Muhakeme'ye bağlar.

## Kabul

- `links::tests`: göreli/`~`/mutlak çözüm, sonek atılması, yok → bağlantı
  değil; politika tablosunun her satırı (çalıştırılabilir → `Reveal`,
  bilinmeyen tip → `Reveal`, dizin → `OpenDir`, `vscode://` → `Confirm`,
  `bateri://tab/x` → `Swallow`).
- `gesture::tests`: fare kipinde ⌘ + bağlantı basışı rapor da seçim de
  üretmez, sürükleme `Ignore`, bırakma `Release::Link` ve rapor yok; Shift
  basılı olsa da aynı.
- `make check` + `make linux` yeşil.

## Checklist

- [ ] `links.rs`: `resolve`, `action`, makine adı
- [ ] `gesture.rs`: `pressed_link`, `Release::Link`
- [ ] Test: çözüm, politika tablosu, jest ön-rotası
- [ ] Doğrulama geçti (`make check` + `make linux`)
