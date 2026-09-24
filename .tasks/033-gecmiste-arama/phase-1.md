# Phase 1 — Arama çekirdeği `bt-core`'da

## Özet

Sorgu `bt-core`'da derlenir ve arama etkinken `frame()` görünür satırların
eşleşmelerini ızgara ve fill-yerel koşular olarak sınırdan verir; çizen taraf
henüz yok.

_Requirements: R1, R2, R2.1, R2.2_

## Değişiklikler

- **`crates/bt-core/src/search.rs`** (yeni modül) — sorgu tipi (metin, regex
  mi, `Aa` açık mı), düz metnin metakarakter kaçırması (`regex-syntax`'ın
  meta kümesiyle aynı küme; yeni bağımlılık yok), `Aa` açıkken `(?-i)` öneki,
  `RegexSearch::new`'in hatası durum olarak (`Invalid`). alacritty tipi
  `pub` API'de görünmez (`CLAUDE.md` → Bağımlılık).
- **`crates/bt-core/src/session.rs`** — `Session::set_search` / `clear_search`
  (nesil artırır, kare ister); `frame()`'e `SearchRuns` tamponu (`Blocks` /
  `SelectionRuns` emsali, kare başına ayırma yok). Koşu: satır, ilk/son
  sütun, **geçerli mi**, **bir önceki satırdaki koşunun devamı mı** (köşeler
  eşleşme başına, phase-2). İki liste: ızgaranın ekran satırları ve
  fill-yerel satırlar (bant + kesrin tepe satırı, `Cursor::top_row`'un
  uzayı).
  - Tarama `frame()`'in var olan `Term` turunda, yalnız çizilen satırlarda
    (`RegexIter`, sınır: görünür pencere + `fill` + tepe satırı); sert satır
    sonunu aşan eşleşme yok, sarılan satırda koşu satır başına bölünür.
  - Desenin kopyası **kilitsiz sahipli**: yaprak yuvadan `Term` kilidinden
    önce alınır, turdan sonra nesil hâlâ aynıysa geri konur; `Term` altında
    hiçbir kilit alınmaz (`discussion.md` → Muhakeme).
  - Bastırılan giriş satırı dışlanır — karar bastırmanın **tek yükleminden**
    (atlanan hücre kümesiyle aynı), ayrı sorulmaz (015'in dersi).
  - Alternatif ekranda görünür ızgara aranır; bant orada zaten yok.
  - Geçerli eşleşme bu phase'de "görünürdeki en alttaki"; gezinme phase-4.
  - Tarama yalnız sorgu geçerli ve boş değilken; hareket karesi `frame()`'e
    uğramıyor, yani orada zaten yok.
- **`crates/bt-core/src/lib.rs`** — yeni tiplerin ihracı.
- **`crates/bt-gpu/src/link.rs`** — tamponu tutar ve `frame()`'e geçirir
  (çizim phase-2); sınır imzası derlensin diye yalnız bu kadar.

## Kabul

- Hermetik sınamalar: kaçırma (her metakarakter düz eşleşiyor), akıllı ve
  duyarlı kip, geçersiz regex durum, boş eşleşme koşu üretmiyor; ızgarada,
  kaydırılmış pencerede, bantta ve tepe satırında koşuların satırı doğru;
  sarılan eşleşme iki koşu ve ikincisi "devam"; aynı satırda iki eşleşme iki
  koşu; bastırılan giriş satırında koşu yok; alternatif ekran; arama kapalıyken
  tampon boş.
- `make hepsi` yeşil; `make test-yaris` iki profilde yeşil.

## Checklist

- [ ] `search.rs`: sorgu, kaçırma, `Aa`, geçersiz durum
- [ ] `Session::set_search`/`clear_search`, desen kopyasının kilitsiz sahipliği
- [ ] `frame()`: `SearchRuns` (ızgara + fill-yerel), bastırma dışlaması, alternatif ekran
- [ ] Test: yukarıdaki senaryolar
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
