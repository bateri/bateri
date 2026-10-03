# Taşan glyph: aralıkları `1`'in altına açmak

## Hedef

`line_height` ve `letter_spacing` `0.5`'e kadar inebilsin. Hücre fontun
istediğinden küçük olduğunda harf **kesilmeden** komşu hücreye taşsın (iTerm2
davranışı). `1.0` tam olarak fontun kendi aralığı olsun. Kararlar
`discussion.md` → Karar'da.

## Gereksinimler

- **R1** — Atlas iki geometri taşır: yuva (glyph metriği, aralık `max(·,1)`)
  ve hücre (ızgara metriği, gerçek aralık). `≥ 1`'de ikisi eşit, ofset sıfır.
  - **R1.1** — Izgara metriğinin açığı işaretli yuvarlanır ve
    ascent:descent oranında dağıtılır; iki parça da en az 1 px kalır.
    `lh ∈ [0.5, 1)`'de, `MIN_POINT_SIZE` dahil, `0 < baseline_px < cell_px.1`.
  - **R1.2** — `line_height = 1.0`'da fazladan piksel yok. Hücre doğal
    yüksekliğe eşit ve glyph'in taban çizgisi yerinde.
- **R2** — Glyph'ler yuvaya bugünkü yoldan rasterize edilir. Döşenen
  yordamsal aile (blok, çizgi, Braille) ve tofu hücreye çizilip yuvaya
  ofsetle konur. Alt çizgi, üstü çizili ve chevron glyph'e bağlıdır.
  - **R2.1** — Geniş glyph'in iki yarısı bölme çizgisinde kırpılır;
    birleşimleri tek parça raster.
  - **R2.2** — Ortalama her zaman `cols × hücre ilerlemesi` kutusunda;
    yedek glyph kapısının **sınırı** o kutunun iki yanına pay eklenmiş hâli
    (`cols = 1`'de yuva, `cols = 2`'de `hücre + yuva`).
- **R3** — `bt-gpu` yuvayı atlastan okur (`slot_layout`, `write_slot`,
  `uv_size`). Glyph ve emoji dörtgeni yuva boyunda ve `slot_offset` kadar
  geride.
  - **R3.1** — Taşan mürekkep aynı yüzeyde komşu satırın zemininin üstünde
    görünür. Izgaranın tepe satırının aksanı doldurma bandı görünürken de
    görünür. Caret bant zemininin altında kalır.
  - **R3.2** — Dock'un glyph'leri bandın tepesine kadar taşar, bandın dışına
    çıkmaz. Geliş efekti `t = 1`'de statik glyph'le aynı.
  - **R3.3** — Yazım efektleri yuvayı örnekler: sınır, `texel` ve shatter
    ızgarası.
- **R4** — Ayar aralıkları `0.5..=2.0`. Ayar penceresi, şablon,
  `docs/AYARLAR.md` ve `CLAUDE.md` yeni sözleşmeyi anlatır.
- **R5** — phase-0'dan sonra `≥ 1`'de görüntü değişmez (`raster_digest`'in
  farkı boş + GPU'da `slot_offset == 0`).

## Yaklaşım

0. **phase-0 — `1.0`'daki fazladan piksel** (`bt-atlas`, hücre ölçüsünü sabitleyen sınamalar): R1.2. Varsayılan hücre 1 px kısalıyor, yuva işinden ayrı commit.
1. **phase-1 — iki metrik ve yuva sahipliği** (`bt-atlas` + `bt-gpu`'nun yuva
   okuyucuları, shader yok).
   - Atlasta glyph ve ızgara metriği. İşaretli açık, ofset fonksiyonu,
     hücreye çizilip yuvaya konan yordamsal aile, `Half`'ın kırpması ve
     kapının kutusu.
   - `bt-gpu`'da yuvanın tek kaynağı atlas.
   - Ayar aralığı hâlâ `≥ 1`. Görünür değişiklik yok; `raster_digest` farkı boş.
2. **phase-2 — taşan dörtgen** (`bt-gpu`, shader'lar; riskli).
   - Immediates'e `slot_px`/`slot_offset`. `cell.wgsl` ve `glyph_fx.wgsl`
     buna göre değişiyor.
   - Glyph viewport'u taşma payıyla; ızgara ve bant sırası (R3.1); dock'un
     viewport'u (R3.2).
   - Offscreen bekçiler. Aralık hâlâ `≥ 1`, `< 1` yalnız sınamalarda.
3. **phase-3 — aralığı aç** (`bt-core`, ayar penceresi, belgeler): R4.

## Kapsam Dışı

- Glyph'i küçülterek daraltma (Seçenek B, reddedildi).
- Pencere kenarında taşmayı korumak: pencere sert bir kenar.
- Dock'un bandının dışına taşma: dock ayrı bir panel.

## Akış

```
settings (0.5..=2) ─► FontOptions ─► Spacing ─► Atlas
                                                 ├─ glyph metriği = yuva (max(·,1))  ─► raster (bugünkü yol)
                                                 ├─ ızgara metriği = hücre (işaretli) ─► PTY, fare, caret, zemin, döşenen sprite
                                                 └─ slot_offset = f(yuva, hücre)
Renderer ─► immediates {cell_px, slot_px, slot_offset, uv_size}
         ─► dörtgen = pos − slot_offset + corner·slot_px
plan: ızgara zemin → vurgu → caret → bant zemin → vurgu → ızgara glyph → bant glyph → dock
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-0 | ✅ |
| phase-1 | |
| phase-2 | |
| phase-3 | |
| kapı | |
