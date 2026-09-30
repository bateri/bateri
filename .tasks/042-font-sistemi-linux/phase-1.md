# Phase 1 — Taşınacak kodun yerinde çevirisi

## Özet

Phase 2–3'te taşınacak kodun yorumları, doc-comment'leri, `assert!`
gerekçeleri ve tanı metinleri yerinde İngilizceye çevrilir; kod değişmez
(`discussion.md` → Karar 9, dil kısıtı 040 `plan.md`).

_Requirements: R4, R1.1_

## Değişiklikler

- **`crates/bt-atlas/src/font.rs`** — dosyanın tamamı (bütünüyle taşınıyor).
  `eprintln!` tanı satırları da İngilizce; `bateri:` öneki korunur.
- **`crates/bt-atlas/src/raster.rs`** — yalnız platforma bağlı yarı:
  `DrawResult`, `draw`, `draw_glyph`, `draw_color_glyph`, `unpremultiply` ve
  dosya başlığı. Yordamsal çizim yerinde kalıyor ve dokunulmuyor.
- **`crates/bt-atlas/src/census.rs`** — iç yapılarla birlikte düzenlenecek;
  çevirisi burada.
- `lib.rs` çevrilmiyor (taşınmıyor); ileride eklenen yorum İngilizce.

Çeviri anlamı korur, "neden"i kısaltmaz; `.tasks/` işaretçileri aynen kalır.

## Kabul

- Yorum satırları (`//`, `///`, `//!`) soyulmuş kaynağın ebeveynle farkı
  yalnız dizgi değişimleri (tanı ve `assert!` metinleri) — başka satır yok.
- Tanık (phase-0) ebeveynle aynı.
- `make hepsi` yeşil.

## Checklist

- [x] `font.rs` çevrildi
- [x] `raster.rs`'in platforma bağlı yarısı çevrildi
- [x] `census.rs` çevrildi
- [x] Test: yorumsuz fark yalnız dizgi değişimi
- [x] Test: tanık ebeveynle aynı
- [x] Doğrulama geçti (`make hepsi`)

## Uygulama Notları

- `census.rs`'in rapor başlıkları ve sütun adları da çevrildi (`TOPLAM` →
  `TOTAL`): çıktıyı ayrıştıran yok, `make tarama` yalnız `cargo test` koşuyor.
- `unreachable!` metni satırı 100 sütunda tutacak kadar kısaltıldı; aksi
  hâlde rustfmt kolu bloğa açıp yorumsuz farkı dizgi dışına taşırdı.
- `raster.rs`'te bir doc satırı `CLAUDE.md` başlığını ("Renk uzayı sınırı
  geçer") Türkçe alıntılıyor — işaretçi, çeviri değil.
