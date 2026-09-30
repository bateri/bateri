# Phase 0 — Bit eşitliğinin tanığı

## Özet

macOS'ta atlasın gözlenebilir her çıktısını public API'den özetleyen bir
sınama dosyası ve onu ebeveyn commit'e karşı koşma yöntemi (`discussion.md` →
Karar 4).

_Requirements: R1.1_

## Değişiklikler

- **`crates/bt-atlas/tests/raster_digest.rs`** (yeni) — `cfg(target_os =
  "macos")`, `#[ignore]`. Yalnız `bt_atlas`'ın public API'si: `Atlas::new`,
  `metrics()`, `font_issue()`, `context_cell_w` benzeri public ölçüler,
  envanterin her `slot()` cevabı (`Placed`) ve `Upload` baytları (sol + sağ),
  küme için `intern`. Envanter ve ölçü kombinasyonları Karar 4'te. Çıktı
  satır başına `anahtar<TAB>özet`; özet std'nin hasher'ı değil (sürümler
  arası kararlı olmalı), sabit bir FNV-1a gibi kod içinde yazılı bir özet.
  Yorumlar İngilizce.
- **Karşılaştırma yöntemi** — yeni `Makefile` hedefi yok; yöntem dosyanın
  başlık yorumunda: `git worktree add` ile ebeveynde ve çalışma ağacında
  `cargo test -p bt-atlas --release --test raster_digest -- --ignored
  --nocapture`, iki çıktının `diff`'i boş.

## Kabul

- Sınama macOS'ta koşuyor ve envanterin her kalemi için satır basıyor;
  aynı ağaçta iki koşu aynı çıktıyı veriyor (determinizm).
- `make hepsi` yeşil (sınama `#[ignore]`, kapıyı yavaşlatmıyor).

## Checklist

- [ ] Envanter: ASCII × dört yüz, küçük sınıf, yedek, küçültülen yedek (`⧉`),
      geniş karakter, emoji, küme, yordamsal aile, kurallar, tofu, olmayan ve
      orantılı aile; 13/16 pt × @1x/@2x × iki satır aralığı
- [ ] Özet kod içinde, sürümden bağımsız
- [ ] Test: iki ardışık koşunun çıktısı aynı
- [ ] Doğrulama geçti (`make hepsi`)
