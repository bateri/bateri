# Font sistemi soyutlaması ve Linux font yığını

## Hedef

`bt-atlas`'ın CoreText'e bağlı yarısı bir `FontSystem` trait'inin arkasına
geçer; macOS'ta CoreText kalır ve raster, hücre ölçüsü ve yedek kararları bit
bit aynıdır. Linux'ta FreeType + fontconfig + harfrust aynı atlası besler ve
`make linux` `bt-atlas`'ı ve lavapipe üstünde `bt-gpu`'nun piksel sınamalarını
koşar. Gerekçeler `discussion.md` → Karar.

## Gereksinimler

- **R1** — macOS'ta fark yok.
  - **R1.1** — Public API'den okuyan bir tanık (metrik, tanı, `Placed`,
    yuva baytları) ebeveyn commit'le aynı çıktıyı verir; her macOS phase'inin
    kabul ölçütü (Karar 4).
  - **R1.2** — `bt-atlas`'ın dış API'si, `bt-gpu` ve `bt-shell`'in çağrıları
    değişmez.
- **R2** — Kural yarısı platformsuz: `centre_shift`, `ink_fits_placed`,
  `fit_ratio`, `accept`, `shrink`, `Accepted::rise`, hücre formülü, zincirin
  istenen-aile kolu ve `unpremultiply` mantığı değişmeden, `f64`/`InkRect`
  tipleriyle (Karar 2).
- **R3** — Statik `FontSystem` trait'i (`Font: Clone`, glyph `u32`, Karar 3'ün
  yüzeyi) ve `cfg` takma adı; CoreText tipleri yalnız macOS arka ucunda
  (Karar 1, 3).
- **R4** — Taşınan kodun yorumları, doc'ları ve tanı metinleri İngilizce;
  çeviri kod değişikliğinden ayrı commit (Karar 9).
- **R5** — Sınamalar bölünmüş: değişmez bekçileri fikstürle platformsuz,
  kalibrasyon `cfg(target_os = "macos")`, `census` macOS'a özgü (Karar 7).
- **R6** — Linux'ta maske yolu: fontconfig zinciri (`monospace` varsayılanı),
  dört yüz, kesirli metrik, mürekkep, `FcFontSort` cascade'i, hinting'siz gri
  AA ile maske çizimi (Karar 6).
- **R7** — Linux'ta renk ve küme: `CBDT` emoji `FT_LOAD_COLOR` + yalnız
  küçülten örnekleyici, `harfrust` şekillendirmesi, tek bayt kaynağı
  (Karar 6).
- **R8** — Kapı büyür: `make linux` `bt-core`, `bt-atlas` ve `bt-gpu`'yu
  koşar; imaj derleme paketleri, fontlar ve lavapipe'la; `bt-gpu`'da yalnız
  `cfg` düzenlemeleri ve fikstür (Karar 8).
- **R9** — Sözleşme güncel: `CLAUDE.md` (katman tablosu, bağımlılık
  paragrafı), `proje.md` → Doğrulama, `bt-atlas` başlık yorumu, `Makefile` ve
  Dockerfile yorumları.

## Yaklaşım

0. Tanığı yaz (`crates/bt-atlas/tests/raster_digest.rs`), ebeveyne karşı
   koşma yöntemiyle birlikte.
1. Taşınacak kodun yorumlarını yerinde İngilizceye çevir.
2. Kural yarısını platformsuz modüle taşı, tipleri nötrle.
3. Trait'i ve CoreText arka ucunu kur, çizimi arka uca indir, sınamaları
   böl, `objc2` bağımlılıklarını macOS hedefine al.
4. FreeType/fontconfig arka ucunun maske yolunu yaz, imajı ve `make linux`'u
   `bt-atlas`'a büyüt.
5. Renk ve küme yolunu yaz.
6. `bt-gpu`'yu Linux'ta derle ve lavapipe'ta koş, sözleşmeyi güncelle.

**Eskalasyon:** phase-4'ün crate/sürüm seçimleri, bağlanma biçimi ve imaj
paketleri kullanıcı onayı bekliyor (`discussion.md` → Karar). Phase 0–3
yalnız macOS'ta ve yeni bağımlılık getirmiyor.

## Kapsam Dışı

- `make tarama`'nın Linux kolu ve `SHRINK_LIMIT`'in Linux kalibrasyonu
  (sabit ölçülmeden kullanılıyor — bilinen sınır).
- `COLRv1` emoji (FreeType 2.12 çizmiyor), LCD/subpixel AA, fontconfig'in
  hinting tercihlerine uymak.
- Linux'un üçüncü taraf bildirim dosyası (paketleme seti).
- `bt-shell` ve pencere tarafı (sonraki setler).
- Kapı mantığının, atlasın ve yordamsal çizimin değişmesi.

## Akış

```
                 platformsuz (bt-atlas)
  Atlas::slot ── gate (accept / shrink / fit_ratio / centre_shift / rise)
       │         cell formula · open_chain · Faces · unpremultiply
       │                     │
       ▼                     ▼  type Backend = cfg(target_os)
  FontSystem ──┬── coretext  (macOS: CTFont, CGBitmapContext)
               └── freetype  (Linux: fontconfig → bytes → FT_Face + harfrust)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-0 | ✅ |
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| phase-4 | ✅ |
| phase-5 | ✅ |
| phase-6 | |
| kapı | |
