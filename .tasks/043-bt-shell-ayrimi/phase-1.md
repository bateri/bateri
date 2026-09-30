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

- [x] On bir modül çevrildi
- [x] Test: yorumsuz fark yalnız dizgi değişimi
- [x] Test: sınama adları listesi ebeveynle aynı
- [x] Doğrulama geçti (`make hepsi`)

## Uygulama Notları

- Bir düzine kadar `assert!`/`expect` metni birebir çeviriden kısa tutuldu (ör.
  `"sıralı, paralel değil"` → `"sequential only"`): uzun hâli rustfmt'nin
  çağrı genişliğini aşıp kodu yeniden sarardı ve yorumsuz fark dizgi
  dışına taşardı (042 phase-1 emsali).
- Yorumsuz farkın ölçüsü: yorum soyulmuş kaynakta satır sayıları ebeveynle
  aynı ve farklı satırların hepsi dizgi; `child.rs`'teki çok satırlı tek
  `assert!` metni de dizginin devamı.
- `settings.rs` sınamalarındaki `panic!`/`expect_event` metinleri de tanı
  sayılıp çevrildi; `notices.rs`'in Türkçe sınama girdileri (`"bozuk"`,
  `"ilk"`…) çıktıyla karşılaştırıldığı için veri olarak kaldı, `quote.rs`'in
  `İki Kelime`/`ğüşİÖÇ` örnekleri ve `upload.rs`'in `ç….txt`'si de.
- `upload.rs`'te `037 Karar 7 → Kullanıcı kararı` gibi başlık alıntıları,
  `child.rs`'te `CLAUDE.md` başlık alıntıları işaretçi olarak Türkçe kaldı;
  `split.rs`'te Karar 7'nin içeriğinden yapılan alıntı ("ayırıcı bir
  piksel") başlık değil, çevrildi.
