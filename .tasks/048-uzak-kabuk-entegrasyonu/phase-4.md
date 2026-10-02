# Phase 4 — Ayar penceresi ve Shell menüsü

## Özet

Anahtarı arayüze taşı: Remote Files sayfasında genel onay kutusu, Shell
menüsünde host başına aç/kapa.

_Requirements: R6_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `SettingsEdit`'e genel anahtar ve
  host başına `integration` girdisi (`host_mark_plan`'ın emsali: tek girdi,
  biçim korunarak).
- **`crates/bt-shell-macos/src/settings_window.rs`** — Remote Files'ta
  "Set up shell integration on servers" onay kutusu, altında prod
  varsayılanını söyleyen tek satır.
- **`crates/bt-shell-macos/src/`** (Shell menüsü, "Mark “{host}” as ▸"ın
  yanı) — "Shell Integration on “{host}”" aç/kapa; durum ayarın çözümünden.
- **`docs/AYARLAR.md`** — arayüz satırları.

## Kabul

- `SettingsEdit` round-trip: bilinmeyen anahtar ve yorum korunuyor, host
  girdisi `mark`'ı bozmuyor.
- Menü öğesinin durumu çözüm sırasını izliyor (prod işaretli host'ta kapalı
  görünür).
- `make check` yeşil.
- Gözle kontrol: menüden kapat → bir sonraki `ssh` düz; ayar penceresinde
  kutu dosyayla eşzamanlı.

## Checklist

- [ ] `SettingsEdit` girdileri
- [ ] Ayar penceresi
- [ ] Shell menüsü
- [ ] `docs/AYARLAR.md`
- [ ] Test: round-trip, menü durumu
- [ ] Doğrulama geçti (`make check`)
