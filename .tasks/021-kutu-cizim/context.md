# Kutu çizim — Bağlam

## Mevcut Durum

Kutu/blok çizim ve Braille karakterleri **fonttan geliyor**: `bt-atlas`
`Sprite::Char(ch)` yolunda `raster::draw` ile CoreText'in glyph'ini
rasterize ediyor ve yuvaya koyuyor. `bt-gpu` onları sıradan harflerden
ayırt etmiyor (`renderer.rs`: `Sprite::Char(glyph.ch)`).

Depoda **yordamsal çizim örüntüsü zaten var**: `RuleKind`'ın yedi sprite'ı
(beş alt çizgi, üstü çizili, prompt chevron'u) fonta hiç sorulmadan
`raster::draw_rule` ile hücre ölçüsünden hesaplanıyor. Gerekçesi 012
phase-9'da yazılı: *işaret terminalin kendi işareti, kullanıcının fontunun
değil.* Aynı cümle kutu çizim için de geçerli — çizgi terminalin ızgarasına
ait, fontun em kutusuna değil.

## Motivasyon

019'un ölçümü (2026-09-20, kullanıcı ekran görüntüsüyle bildirdi) **iki
ayrı kusur** çıkardı ve ikisi de glyph yedeğinin konusu değildi:

1. **Blok ve çizgi karakterleri Menlo'da var ama hücreyi doldurmuyor.**
   Alt alta iki `█` arasında şerit kalıyor; belirti yalnız satırlar
   arasında, çünkü yatayda doluyor.
2. **Braille Menlo'da yok**, Apple Braille'den geliyor ve ilerlemesi
   hücreninkini aşıyor — 019'un genişlik kapısından dönüyor, yani kutu
   kalıyor.

Kullanıcı görünürlüğü yüksek ve **sürekli**: Claude Code'un maskotu (blok
elemanları), spinner'ı (Braille) ve her TUI çerçevesi (htop, tmux, lazygit,
btop, `tree`). Yol haritası bu yüzden kutu çizimi emoji setinden ayırdı —
019'un aynı gerekçesiyle: yordamsal çizimin çıktısı yine tek kanallı
kapsama maskesi, `R8Unorm` atlas duruyor ve emoji'nin "ikinci atlas mı,
RGBA mı" çatalı hiç açılmıyor.

kitty, WezTerm, iTerm2 ve Alacritty bu karakterleri **bilerek** fonttan
almıyor.

## Kanıt

019 `phase-2.md` → Uygulama Notları (ölçüm oradan, yeniden ölçülmedi):

- **Blok:** Menlo 13pt'de 8×18 hücrenin yalnız **3–16** satırları boyanıyor;
  iki blok alt alta gelince **~5 piksel** şerit kalıyor. Kullanıcının ekran
  görüntüsündeki boşluk 11 aygıt pikseli ve @2x ile birebir tutuyor.
- **Braille:** Apple Braille'in ilerlemesi hücreninkinin **1.1354×**'i;
  019'un geometrik kapısı onu eliyor.
- **Etkilenen aralıklar:** U+2500–U+257F (çizgi çizim), U+2580–U+259F (blok
  elemanları), U+2800–U+28FF (Braille).

Bu sette **ölçülen** (yukarıdakinden ayrı, `/rfc` sırasında):

- **Yatay döşemede kesirli ilerleme sorunu yok.** Endişe gerçekti — hücrenin
  ilerlemesi kesirli (Menlo 13pt'de 7.827) ve yuva genişliği yukarı
  yuvarlanmış (8) — ama **ızgaranın adımı yuvarlanmışı kullanıyor**:
  `Frame::pos_at` `gutter_px + col * f32::from(cell_px.0)` diyor ve
  `cell_px` `(u16, u16)`. Yani hücre başına tam 8 piksel ilerleniyor;
  yuvayı tam dolduran bir sprite komşusuyla **bitişik** çiziliyor ve
  döşemede ne çakışma ne boşluk kalıyor. Dikeyde de aynı (`at[1] * h`).
  Kesirli ilerleme yalnız iki yerde okunuyor ve ikisi de çizim konumu
  değil: glyph'in hücre içinde ortalanması ve yedeğin genişlik kapısı.
  **Dock'un bağlam satırı da kesirli değil** (`Frame::column_px` →
  `context_cell_px()`, o da `u16`), ama orada ayrı bir engel var ve
  döşemeyle ilgisi yok: sütun adımı küçük yüzün ilerlemesi, yordamsal
  sprite ise **büyük hücre** genişliğinde çizilir — ikisi ayrışır ve komşu
  hücreler örtüşür. Küçük glyph'lerin bugün örtüşmemesinin sebebi dar
  mürekkepleri; hücreyi tam dolduran bir sprite o güvenceyi kaybeder.

## Mevcut Mimari

```
bt-core            bt-gpu                          bt-atlas
 Cell.ch  ──────▶  renderer: Sprite::Char(ch) ──▶  Atlas::slot(Char)
                                                     │
                                                     ├─ raster::draw(font, ch, …)
                                                     │    └─ NoGlyph → font::fallback_font
                                                     └─ (yuva, Upload)
 —                 renderer: Sprite::Rule(kind) ─▶  Atlas::slot(Rule)
                                                     └─ raster::draw_rule(kind, metrics)
                                                          fonta hiç sorulmuyor
```

Kutu çizim ikinci yolun mantığını (yordamsal, hücre ölçüsünden) birinci
yolun tipiyle (`Sprite::Char`, çünkü bir karakterdir) istiyor. Kesişim
`raster::draw`'ın içinde: font sorulmadan önce bir kapı.

**Sınır dokunulmuyor:** `bt-gpu` ve `bt-core` bu setten habersiz kalabilir —
`Sprite::Char(ch)` çağrısı aynı, yuva aritmetiği aynı, `RULE_RESERVE` aynı.
