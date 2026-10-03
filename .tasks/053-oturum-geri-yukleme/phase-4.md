# Phase 4 — Ayar penceresi ve belgeler

## Özet

`restore_windows` ayar penceresinde seçilebilir olur ve davranış belgelere
girer.

_Requirements: R4.2_

## Değişiklikler

- **`crates/bt-shell-macos/src/settings_window.rs`** — Terminal
  kategorisinde `restore_windows` satırı (`ConfirmClose`'un `Choice` emsali:
  `Key`, anahtar yolu, tanı satırı).
- **`docs/AYARLAR.md`** — şablona ve `### [terminal]` bölümüne anahtar: üç
  değer, varsayılan, kullanılamayan dosyada `"layout"`, nereye yazıldığı,
  geçmişin düz metin olarak diske indiği ve Time Machine'in onu yedekleyeceği,
  uzak pane'in davranışı.
- **`CHANGELOG.md`** — kullanıcının gördüğü dille tek madde.
- **`CLAUDE.md`** — bugünkü hâl paragrafına kural + tek cümle gerekçe +
  işaretçi (`.tasks/053-oturum-geri-yukleme/discussion.md` → Karar): kayıt
  `shutdown`'da, tek kurulum yolu `TerminalWindow::restore`, geçmiş VT
  baytı olarak oynatılıyor, uzak satır çalıştırılmadan; `settings.toml`'u
  okuyan anahtar listesine `restore_windows`.

## Kabul

- `every_row_receives_its_own_diagnostic` ve ayar penceresinin mevcut
  sınamaları yeşil, yeni satır dahil.
- Pencereden yazılan değer dosyada yalnız o anahtarı değiştirir.

## Checklist

- [ ] Ayar penceresi satırı
- [ ] `"layout"` ve `"off"` açılışta önceki bir `"all"` kaydının geçmişini **oynatmaz**, okumadan siler (`"layout"` düzeni yine kurar): kullanıcı geçmişi istemediğini söyledikten sonra eski geçmişi bir kez daha göstermek beklenmedik (phase-3 → Uygulama Notları'ndaki davranışın düzeltmesi; orkestratör kararı 2026-10-03)
- [ ] `restore_windows`'un kabul edilmeyen değeri (ör. `"Off"`) `"layout"`'a düşer, `"all"`'a değil: düzen gelir, geçmiş diske yazılmaz (phase-1 → Waive'in kararı; `osc52` emsalinin güvenli yönü). Tanı yine satırda; bekçi `Settings` sınamasında
- [ ] `docs/AYARLAR.md`
- [ ] `CHANGELOG.md`
- [ ] `CLAUDE.md`
- [ ] Doğrulama geçti (`make check` + `make smoke`)
