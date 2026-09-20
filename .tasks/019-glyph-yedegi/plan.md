# Glyph yedeği

## Hedef

Seçili fontta olmayan **tek hücrelik** bir karakter, sistemin cascade'inden
gelen bir fontla çizilsin; sığmayan her şey bugünkü kutuda (`TOFU`) kalsın.

Motive eden belirti: `⏵` (U+23F5) Menlo'da yok ve Claude Code'un
`⏵⏵ auto mode on` göstergesi iki kutu çıkıyor.

## Gereksinimler

- **R1** — `⏵` yedek fonttan çiziliyor. Kol yalnız `Face::Regular`'da ve
  `Sprite::Char`'da koşuyor; `SizeClass`'ın ikisi de **ayrı ayrı**
  değerlendiriliyor ve yedeğin tabanı o sınıfın kendi fontu.
- **R2** — Red **geometrik**: yedek glyph'in ilerlemesi hücre genişliğini
  aşıyorsa `TOFU`. Aile adı karşılaştırması, trait biti ve sihirli dizge
  **yok**.
  - **R2.1** — Emoji, `.LastResort` ve geniş glyph bu tek kapıdan eleniyor
    (ölçüldü: 2.17× / 1.83× / 1.66×).
- **R3** — Yuva anahtarı **değişmiyor** (`(Sprite, Face, SizeClass)`), yeni
  önbellek, yeni tavan ve yeni tahliye politikası **yok**. Arama `NoGlyph`
  yaprağının içinde, yani anahtar başına atlas ömründe bir kez.
- **R4** — Varsayımı kırılan dört sınama yeniden kuruluyor ve
  `UNKNOWN_CHAR` **`'漢'` kalıyor** (1.66× ile reddediliyor, yani hâlâ
  `TOFU`'ya düşüyor ve `occupancy().0 == 1` hâlâ doğru).
- **R5** — Glyph hücrede **ortalanıyor** ve kural evrensel: taban monospace
  fontta kaydırma sıfır, yani çıktı bit bit aynı.
- **R6** — Kodla çelişen her cümle **aynı commit'te** düzeliyor
  (`CLAUDE.md`, `lib.rs`'in başlık yorumu).

## Yaklaşım

1. `font::fallback_font(base, ch, cell_px) -> Option<CFRetained<CTFont>>`:
   `CTFont::for_string` ile aday, `glyph_index` ile glyph, `advance <=
   cell_px` ile kapı. Üçü de tek fonksiyonda; `None` "kabul edilmedi".
2. `Atlas::slot`'un `DrawResult::NoGlyph` yaprağına **tek kol**: yüz
   merdiveninin (`NoGlyph && face != Regular → Regular`) **altına**, negatif
   önbellek + `TOFU` kolunun **üstüne**. Böylece merdiven önce tüketiliyor ve
   yedek yalnız hiçbir yüzde bulunamayan karakterde koşuyor.
3. Taban font `SizeClass`'tan: `Normal → faces.get(Regular)`,
   `Small → &self.small`. Punto `for_string` ile miras geçiyor.
4. `raster::draw` glyph'i `(cell_px - advance) / 2` kadar sağa kaydırıyor.
5. Dört sınama yeniden kuruluyor, beş yeni sınama ekleniyor.
6. Belgeler.

## Kapsam Dışı

- **Emoji ve geniş glyph** — 021'in işi. Bu set onları çizmiyor, **eliyor**;
  eleme R2'nin kapısından geçiyor, ayrı bir kolu yok.
- **Kullanıcı ayarlı yedek liste** (`[font] fallback = [...]`) — ayar anahtarı
  geri alınamaz ("bilinmeyen anahtar asla silinmez"), asıl şikâyet onsuz
  kapanıyor.
- **Atlas tahliyesi (LRU)** — `lib.rs:62` zaten "00X'in işi" diyor. Bu set
  doluluk **oranını** artırıyor ama tavanı değiştirmiyor; genişlik kapısı en
  büyük tüketiciyi (CJK) zaten eliyor.
- **`make duman`'ın jeton satırı** — reçete saf ASCII ve sözleşmeye üç sınama
  ile üç tarihsel kayıt bağlı; değişimi ayrı commit ister.

## Akış

```
Atlas::slot(Char(ch), face, size)
  ├─ önbellek isabeti ────────────────────────────→ yuva
  ├─ kapasite dolu ───────────────────────────────→ TOFU (önbelleklenmez)
  └─ raster::draw(taban font, ch)
       ├─ Drawn ──────────────────────────────────→ yeni yuva
       ├─ NoGlyph && face != Regular ─── merdiven ─→ Regular'ı dene
       └─ NoGlyph && face == Regular
            └─ YENİ: fallback_font(taban, ch, cell_px)
                 ├─ Some(font) → raster::draw(font, ch) → yeni yuva
                 │                (aynı anahtar, aynı Upload yolu)
                 └─ None ──────→ negatif önbellek + TOFU   (bugünkü kol)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| kapı | ✅ |
