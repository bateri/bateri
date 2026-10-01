# Phase 6 — Ayar penceresinde Remote Files

## Özet

Ayar penceresine beşinci kategori: önizleme, temizlik ve indirme satırları,
klasör seçici ve Clear Now.

_Requirements: R8_

## Değişiklikler

- **`crates/bt-shell-macos/src/settings_window.rs`** — `Category::RemoteFiles`
  ("Remote Files", sembol `network`); `Key`'e sekiz anahtar; popup satırları
  (`preview_max_size`, `preview_keep`, `preview_limit`, `download_conflict`
  — seçenek listeleri `bt-core`'un `NAMES` tablolarından), switch satırları
  (`preview_read_only`, `download_notify`). İki yeni satır türü: **klasör
  satırı** (yol etiketi + Change… → `NSOpenPanel` klasör seçimi →
  `SettingsEdit`; önizlemede Show in Finder) ve **kullanım satırı** ("340 MB
  · 12 files" + Clear Now → phase 4'ün yöntemi; boyut arka planda ölçülür,
  pencere açılınca ve Clear Now sonrası tazelenir). Pencere yine durum
  tutmaz; yazma tek düzenlemeden (029).
- **`docs/AYARLAR.md`** — pencerenin yeni kategorisi bir cümleyle.

## Kabul

- `make check` yeşil.
- Gözle kontrol: kategori görünür; her satır değişince `settings.toml`'da
  yalnız o anahtar yazılır ve canlı uygulanır; Change… klasörü değiştirir;
  Clear Now önizlemeleri siler ve kullanım sıfırlanır.

## Checklist

- [ ] Kategori ve popup/switch satırları
- [ ] Klasör satırı (NSOpenPanel) ve kullanım satırı (Clear Now)
- [ ] (phase-4'ten) Clear Now `AppDelegate::sweep_previews(Sweep::ClearNow)`'u çağırır (arka plan thread'i, kurtarılanları kendisi bildirir); bitince kullanımı tazelemek için bugün tamamlanma kancası yok — gerekirse yönteme eklenir. Kullanım ölçümü `preview_cache`'e (taramanın `scan`'i, `.index` ve `.bateri-download-*` hariç) eklenebilir
- [ ] `docs/AYARLAR.md`
- [ ] Doğrulama geçti (`make check`)
