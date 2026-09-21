# Atlas tahliyesi — dokunun kenarı yuva hedefinden türüyor

## Hedef

Atlasın kapasitesi büyük puntoda ailenin altına düşmesin: dokunun kenarı
sabit 1024 olmaktan çıkıp **hedeflenen yuva sayısından** türesin. Varsayılan
puntoda hiçbir şey değişmesin.

## Gereksinimler

- **R1** — Doku kenarı hücre ölçüsünden türüyor: kapasite hedefin altındaysa
  kenar ikiye katlanıyor, bir tavana kadar.
  - **R1.1** — Hedef **ölçülmüş 422'den** türetilmiş bir tasarım sabiti
    (`GUTTER_PT`/`CONTEXT_SCALE` emsali), havadan bir sayı değil.
  - **R1.2** — Tavan (`MAX_EDGE`) adıyla yazılı; aritmetik sınırsız büyümüyor.
  - **R1.3** — Taban 1024: varsayılan punto bugünkü kenarı koruyor.
- **R2** — **Varsayılan yol bit bit aynı.** 13pt@2x'te `occupancy().1 == 1984`
  ve `texture_px()` değişmiyor, yani raster de değişmiyor.
- **R3** — Değişmez sınanıyor: **kabul edilen her (punto × ölçek ×
  `line_height`) için kapasite ≥ aile + tofu (422).**
  - **R3.1** — Köşe gerçekten ölçülüyor: `MAX_POINT_SIZE` × `MAX_LINE_HEIGHT`
    tavana çarptığında da değişmez sağlanıyor. Sağlanmıyorsa tavan yükselir
    ya da değişmez yazılı bir istisna alır — **sayı doğrulanmadan kabul
    edilmiyor**.
- **R4** — `u16` kırpması yolun dışında kalıyor ve **neden** kaldığı yazılı.
- **R5** — `bt-gpu` değişmiyor: kenar `texture_px()`'in arkasında kalıyor.
- **R6** — Kodda tahliyeyi bekleyen dört `00X` yorumu ile `CLAUDE.md`'nin
  `bt-atlas` satırı ve yol haritasının borç maddesi bu kararla hizalanıyor.
  - **R6.1** — Borç **kapanmıyor, daralıyor**: kalan senaryo adıyla ve
    "ölçülmedi" etiketiyle yazılıyor.

## Yaklaşım

1. `TEXTURE_EDGE` sabiti üçe ayrılıyor: `SLOT_TARGET` (hedef yuva),
   `MIN_EDGE` = 1024 (bugünkü taban), `MAX_EDGE` = 4096 (tavan).
2. `Atlas::new` ızgarayı kurarken (`lib.rs:253`) kenarı tabandan başlatıp
   kapasite hedefin altında kaldıkça ve tavana varmadıkça ikiye katlıyor;
   `grid` o kenardan türüyor.
3. `texture_px()`, `slot_origin()`, `capacity()` ve `negative_cache_cap()`
   zaten `grid`'den türüyor — dokunulmuyor. `bt-gpu` `texture_px()`'ten
   okuduğu için hiç değişmiyor.
4. `TEXTURE_EDGE`'i sabit diye bağlayan üç sınama iddiası (`lib.rs:1505`,
   `1633`, `1637`) türetilmiş kenara göre yeniden yazılıyor.
5. Değişmez ve varsayılan-yol bekçileri ekleniyor (R2, R3, R3.1).
6. Doc borcu aynı commit'te: dört `00X` yorumu, `CLAUDE.md`, yol haritası.

## Kapsam Dışı

- **Tahliye (LRU) ve kare-başına geri dönüşüm.** Gerekçe `discussion.md` →
  Karar; kalan senaryo ölçülmedi ve çaresi bu set değil.
- **Kullanıcıya dönük "atlas doldu" tanısı.** Geliştirici kanalı (`yuva=`)
  zaten var; kullanıcı tanısı ayrı bir karar.
- **Sekme başına doku paylaşımı.** `Atlas` bugün `Renderer`'ın alanı;
  sekmelerin paylaşıp paylaşmayacağı 025'in sorusu.
- **`replaceRegion`'ın staging + blit düzeltmesi.** Bu set canlı yuvanın
  üstüne yazmıyor, yani sınır bugünkü hâlinde kalıyor.

## Akış

```
Atlas::new(family, point_size, scale, line_height)
  └─ metrics.cell_px = (w, h)
     └─ edge = MIN_EDGE
        while capacity(edge) < SLOT_TARGET && edge < MAX_EDGE { edge *= 2 }
        └─ grid = (edge / w, edge / h)
              ├─ capacity()      = grid.0 * grid.1
              ├─ texture_px()    = (grid.0 * w, grid.1 * h)   → bt-gpu buradan
              ├─ slot_origin()   = grid'den
              └─ negative_cache_cap() = 2 * capacity()
```

13pt@2x: kapasite 1984 ≥ hedef → kenar 1024, **bugünkü hâl**.
29pt@2x: 406 < hedef → 2048 → 1624 ≥ hedef → dur.

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | |
| kapı | |
