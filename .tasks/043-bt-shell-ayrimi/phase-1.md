# Phase 1 — Taşınacak modüllerin yerinde çevirisi

## Özet

Phase-2'de `bt-shell-common`'a taşınacak on bir modülün yorumları,
doc-comment'leri, `assert!` gerekçeleri ve tanı metinleri yerinde İngilizceye
çevrilir; kod değişmez (`discussion.md` → Karar 3, emsal
`.tasks/042-font-sistemi-linux/phase-1.md`).

_Requirements: R1_

## Değişiklikler

- **`crates/bt-shell/src/{settings,split,zoom,notices,gesture,quote,keys,upload,jobs,child,watch}.rs`**
  — dosyaların tamamı, sınama modülleri dahil. `eprintln!` tanılarının
  `bateri:` öneki korunur. Çeviri anlamı korur, "neden"i kısaltmaz;
  `.tasks/` işaretçileri ve Türkçe başlık alıntıları (işaretçi olarak) aynen
  kalır.
- Pencerede görünen UI dizgileri zaten İngilizce; dokunulmaz.
- Başka dosya yok (`lib.rs` ve AppKit modülleri yerinde kalıyor).

## Kabul

- Yorum satırları (`//`, `///`, `//!`) soyulmuş kaynağın ebeveynle farkı
  yalnız dizgi değişimleri (tanı ve `assert!` metinleri) — başka satır yok.
- macOS'ta `cargo test --workspace -- --list` sınama adları ebeveynle aynı.
- `make hepsi` yeşil.

## Checklist

- [ ] On bir modül çevrildi
- [ ] Test: yorumsuz fark yalnız dizgi değişimi
- [ ] Test: sınama adları listesi ebeveynle aynı
- [ ] Doğrulama geçti (`make hepsi`)
