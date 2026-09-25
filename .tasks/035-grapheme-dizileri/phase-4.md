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

- [x] Sınır `Cell`'i + tablo argümanı + üç yüzey
- [x] `bt-gpu` tablosu listelerle yaşıyor; `fan` ve `prepare_fx`
- [x] Dock seçimi / silme / fark hizası
- [x] İsabet testinin sağ yarısı küme sonuna: `hit` kümeli düzende baş
  karakterin `DockPoint`'ini veriyor, `boundary` `index + 1`'e adımlıyor
  (yalnız sıfır genişlikleri atlıyor) ve `🇹🇷`'nin sağ yarısına tık
  `🇹`/`🇷` arasına düşüyor; `hit`'in `index + 1 < len` geri dönüşü de aynı.
  Adım `Placed::end`'e (phase-3'te doğdu) ← phase-3 `/code-review`
- [x] Dört tuşun küme kolu (kapı açıkken)
- [x] Test: renderer (üç yüzey), hareket karesi, dock düzenleme, ızgara kopyası
- [x] Doğrulama geçti (`make hepsi` + `make duman`)
- [x] Riskli phase (render thread → `make test-yaris`): `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Tablo tipi `bt-core`'da** (`Clusters`, `ClusterId(NonZeroU32)`): `frame()`
  ve `dock()` onu adlandırmak zorunda ve `bt-core` `bt-gpu`'yu görmüyor
  (`SelectionRuns` emsali); sahibi `Frame`. Üç tablo: `clusters` (ızgara +
  bant, tek `frame()` çağrısı), `dock_clusters`, `fx_clusters` (hayaletler).
  Doldurulurken `Frame::take_*`/`put_*` ile dışarı alınıyor — sink'ler
  `frame`'i ödünç alıyor. `Cell` 72 → **76 bayt** (ölçüldü).
- **Efektlerin kendi tablosu**: `GlyphFx` düzenlemenin kümesini dock
  tablosundan kendi tablosuna kopyalıyor ve tabloyu her `apply`'da yaşayan
  girdilerden yeniden kuruyor (`FX_MAX` sınırlı); `set_dock_fx` hayaletleri
  `fx_clusters`'a kopyalıyor, gelişler statik glyph'in kopyası olduğu için
  dock tablosunda. `apply`/`set_dock_fx`/`encode_*` birer tablo argümanı
  aldı (`encode_glyphs`/`encode_fx`'te `too_many_arguments` izni).
- **Hayaletler bütün kod noktalarını taşıyor** (`Ghosts`, `GHOST_CHARS =
  EDIT_MAX * 4`): hayaletlerin düzeni kümeyi kurabilsin diye (phase-3'ün
  `false`'u `state.cluster` oldu). `Change` büyüdü; `Box` `Copy`'yi ve kare
  başına bir ayırmayı götüreceği için `large_enum_variant` izni. Kümeleme
  kapalıyken sıfır genişlikli kod noktası düzende hücre almıyor, yani
  konumlar aynı.
- **Farkın hizası "küçült" değil "sına"**: kümeleme açıkken eklemenin ya da
  silmenin iki ucu ve birleşme noktası küme sınırı değilse `Reset` (yarım
  bayrak, ten rengi eklemek, `🇹x🇷`'den `x`); glyph sayısı küme sayısı.
- **Sağ yarı `boundary`'de, `DockPoint`'te değil**: `boundary`/`stepped`
  kümeleme açıkken `dock::cluster_span`'le (tek kural, `Walk`) küme sınırına
  iniyor; `DockSelection` bayrağı taşıyor, `selection_range` ve `stepped`
  birer `cluster` argümanı aldı. `hit`'in sarma geri dönüşü kümenin kod
  noktası sayısıyla (`Placed::end − index`) — eski `index + 1 < len`
  `🇹🇷`'yle biten satırın sağındaki tıkı bayrağın sağ yarısına indiriyordu.
- **`keys.rs` değişmedi**: `keys::dock_key` dört düz tuşu zaten koşulsuz
  `Session::dock_key`'e soruyor; küme kolu yalnız orada
  (`DockEditLine::before`/`after`).
- **Test-first sırası tutmadı**: sınamalar koddan sonra yazıldı; iki
  kritik bekçi mutasyonla kırmızı gösterildi (renderer'ın üç yüzey
  sınaması `fan` kümeyi yok sayınca, `hit`'in geri dönüşü eski kurala
  dönünce). Renderer sınaması Retina'da (13pt@1x'te bayrak taban karaktere
  düşüyor — phase-1'in ölçeği).
- **`/code-review` (iki düşük bulgu)**: (1) caret bir kümenin içindeyken
  seçimsiz ⇧← boş seçim veriyordu — `stepped`'in sabit ucu kümenin arkasına
  iniyor, sınamalı; (2) basılı ⌫'nin tekrarı ayna cevap verene kadar kapalı
  kapıdan ZLE'ye gidip kümeyi bölebiliyor — tuş davranışı kararı, bayrak
  kapalı ve kullanıcıya görünmüyor → phase-5 checklist'i.

