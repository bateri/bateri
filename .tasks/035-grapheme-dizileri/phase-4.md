# Phase 4 — Çizim ve dock düzenlemesi kümeyi görüyor

## Özet

Küme sınırdan `bt-gpu`'nun küme tablosuna, oradan atlasa gidiyor; ızgara,
doldurma bandı, dock ve yazım efektleri onu tek glyph çiziyor ve dock'un
seçimi/silmesi/farkı kümeyi bölmüyor — yine oturum seçeneğinin arkasında.

_Requirements: R4, R4.1, R4.2, R4.3_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — sınır `Cell`'ine `Option` küme
  indeksi (niche'li, 4 bayt); taban `ch` aynen. `frame()`'in iki sink'i ve
  `Session::dock` kümeyi `bt-gpu`'nun verdiği `&mut` tabloya ekleyip
  indeksini koyar (`SelectionRuns` emsali; tablo imzada tek argüman, üç
  yüzey aynı tipi alır). Kümesiz hücre hiçbir şey ödemez — küme yalnız
  **geniş** (iki sütunlu) ve birden çok kod noktalı hücrede doğar; tek
  sütunlu birleştirici (`é`, `⌚︎`) bugünkü gibi taban karakterle (Karar 6). `Cell`'in doc'undaki boyut
  cümlesi yeni ölçüyle güncellenir (ölç, tahmin etme).
- **`crates/bt-gpu/src/frame.rs` / `link.rs`** — küme tablosu (baytlar +
  aralıklar) listelerle birlikte yaşar ve temizlenir: ızgara, doldurma bandı
  (kareler arası tamponlanan `Vec<Cell>` dahil), dock ve efekt listeleri
  indeksleri **kendi** tablolarına göre taşır. `GlyphCell` kümeyi taşır.
- **`crates/bt-gpu/src/renderer.rs`** — `fan` ve `prepare_fx` kümeli
  hücrede `atlas.intern(&str)` → `Sprite::Cluster`; interning yalnız
  burada (sink atlası ödünç alamaz — 023). Kapı taban karaktere düştüyse
  `Sprite::Char(ch)`.
- **`crates/bt-core/src/dock.rs`** — dock seçimi (`selection_range`,
  sürükleme uçları), `d;S;E`'nin `S`/`E`'si ve `diff`'in düzenleme
  aralığı küme sınırına hizalanır; bir kümenin içine düşen uç kümenin
  sınırına iner. Geliş/hayalet efekti kümeyi tek glyph taşır.
- **`crates/bt-shell/src/keys.rs` + `Session::dock_key`** — düzenleme kapısı
  açıkken caret'in bitişiğindeki küme birden çok kod noktalıysa ⌫/⌦/←/→
  widget komutuna gider (Karar 7); değilse bugünkü yol (yazım efektleri ve
  `self-insert` aynen). Kapı kapalıyken değişiklik yok.
- Izgara seçiminin kopyası kümeyi alacritty'nin satır metninden zaten
  bütün alıyor — sınamayla sabitlenir, kod değişmez.

## Kabul

- Offscreen renderer sınaması: `🇹🇷` taşıyan tek geniş hücre renk
  düzleminden iki dörtlü basıyor (kutu yuvası değil); doldurma bandında ve
  dock'ta aynı.
- Hareket karesinde (listeler korunurken) küme glyph'i değişmiyor.
- Dock'ta `🇹🇷` üstüne çift tık / ⌫ / ⇧← kümeyi bütün seçiyor, siliyor,
  daraltıyor; seçimsiz ⌫ ve ← (kapı açık) kümeyi bütün siliyor / geçiyor
  ve kabuğa tek `d;S;E;L` gidiyor; yazım efekti tek glyph.
- Izgarada seçilen `👨‍👩‍👧` kopyası beş kod noktası.
- Bayrak kapalıyken bütün sınamalar bit bit aynı; `make duman` jetonları
  aynı şekilde.

## Checklist

- [ ] Sınır `Cell`'i + tablo argümanı + üç yüzey
- [ ] `bt-gpu` tablosu listelerle yaşıyor; `fan` ve `prepare_fx`
- [ ] Dock seçimi / silme / fark hizası
- [ ] İsabet testinin sağ yarısı küme sonuna: `hit` kümeli düzende baş
  karakterin `DockPoint`'ini veriyor, `boundary` `index + 1`'e adımlıyor
  (yalnız sıfır genişlikleri atlıyor) ve `🇹🇷`'nin sağ yarısına tık
  `🇹`/`🇷` arasına düşüyor; `hit`'in `index + 1 < len` geri dönüşü de aynı.
  Adım `Placed::end`'e (phase-3'te doğdu) ← phase-3 `/code-review`
- [ ] Dört tuşun küme kolu (kapı açıkken)
- [ ] Test: renderer (üç yüzey), hareket karesi, dock düzenleme, ızgara kopyası
- [ ] Doğrulama geçti (`make hepsi` + `make duman`)
