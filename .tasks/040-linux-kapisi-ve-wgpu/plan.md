# Linux kapısı ve wgpu renderer'ı

## Hedef

Linux'a giden yolun ilk adımı iki parçadan oluşuyor:

- `bt-core`'un Linux'ta yeşil olduğunu her seferinde söyleyen bir kapı
  (`make linux`).
- Renderer'ın doğrudan Metal'den wgpu'ya ölçümlü geçişi.

Set bittiğinde `bt-gpu`'nun doğrudan bağımlılığında ve kaynağında platform
kütüphanesi kalmıyor. macOS'ta kullanıcı **hiçbir fark görmüyor**: çizim,
animasyonlar, dock ve boşta sıfır kare aynı kalıyor.

## Gereksinimler

- **R1** — Linux kapısı
  - **R1.1** — `make linux`, depodaki imaj tarifiyle Docker'da
    `bt-core`'un `clippy -D warnings` ve `test`'ini `--locked` koşuyor, ve
    yeşil.
  - **R1.2** — Yerel rustc ile imaj sürümü uyuşmazsa kapı kırmızı düşüyor.
    `proje.md` → Doğrulama'da koşullu satırı var. `CLAUDE.md`'nin "kapı Linux
    hedefiyle derlemedir" cümlesi bu komuta bağlı.
- **R2** — Deneme
  - **R2.1** — `cell_bg` + caret, WGSL'de ve wgpu'da offscreen çiziliyor.
    Bekçilerinin wgpu ikizleri geçiyor (`MIDTONE`, bit bit dejenere caret,
    köşe/kenar/hale).
  - **R2.2** — Çapraz arka uç sahne karşılaştırması geçiyor: düz dolgu tam,
    AA/SDF kenarında ≤ 1/255.
  - **R2.3** — wgpu ürün grafının dışında (dev-dependency, `cfg(test)`).
    `proje.md`'nin WGSL satırları (Doğrulama, riskli tetik, merceği 6) aynı
    commit'te.
  - **R2.4** — Ölçüm kancası var: iki arka uçta offscreen N-kare döngüsü.
    Metal'in pencere yolu tabanı `docs/OLCUMLER.md`'de. Durak kuralı
    uygulanmış (`discussion.md` → Karar 3).
- **R3** — Pipeline'ların taşınması
  - **R3.1** — `cell` + `emoji` wgpu'da: atlasın iki düzlemi ve
    yüklemeleri. Sahneleri kâhin listesinde.
  - **R3.2** — `glyph_fx` + `selection` (arama vurgusu dahil) wgpu'da.
    Sahneleri kâhin listesinde.
  - **R3.3** — Renderer tamam. `render_offscreen` wgpu'da ve bütün
    `renderer.rs` bekçileri orada geçiyor. Doğrudan Metal'e inen
    sınamaların wgpu karşılığı var.
  - **R3.4** — Tamamlanma modeli (gönderim indeksi + `poll`) dört işi
    taşıyor: `kare=` hatasız, `Retry`, `acilis=`'ın anı, uykudan önceki kare.
    GPU damgası `TIMESTAMP_QUERY` ile, yoksa `unsupported`.
- **R4** — Geçiş
  - **R4.1** — `Pacer` (dört görev) ve platformsuz `tick(damga, hedef)`.
    macOS gerçeklemesi `bt-shell`'de, (b) yolu.
  - **R4.2** — `CAMetalLayer`'ı `bt-shell` kuruyor. wgpu yüzeyi ondan tek
    `unsafe` girişle açılıyor. wgpu normal bağımlılık, Metal renderer yalnız
    `cfg(test)` kâhini.
  - **R4.3** — `make duman` yeşil ve jetonlar aynı (anahtar silinmez). Pencere
    yolu ölçümü durak kuralından geçti ya da eskale edildi.
- **R5** — `IDLE_FRAME_LIMIT` ve `QUIET_FLOOR` yeniden gözlendi (sağlıklı ve
  bozuk dağılım, debug ve release). Değer ya da doc ayrı commit'te.
- **R6** — Söküm
  - **R6.1** — Metal renderer, `.metal`'ler, `build.rs` ve `bt-gpu`'nun
    `objc2*`/`dispatch2`/`block2` bağımlılıkları (dev dahil) gitti.
  - **R6.2** — `make denetim` `bt-gpu`'nun doğrudan bağımlılığında ve
    kaynağında platform kütüphanesi arıyor. (a) seçildiyse istisnası adıyla
    yazılı.
  - **R6.3** — `make shader` WGSL kanaryası. `CLAUDE.md`, `bt-gpu` başlığı,
    `proje.md` ve yol haritası güncel.

## Yaklaşım

1. `make linux`: imaj tarifi, sürüm eşleşmesi ve `--locked`, koşullu
   doğrulama satırı (phase-1).
2. Deneme: dev-dependency wgpu, `cfg(test)` wgpu renderer iskeleti, `cell_bg`
   + caret WGSL. Bekçi ikizleri ve kâhin sahne listesi, ölçüm kancası.
   `proje.md`'nin WGSL satırları. `/measure` ve durak kuralı (phase-2).
3. `cell` + `emoji` grubu ve atlas dokuları (phase-3).
4. `glyph_fx` + `selection` grubu, tamamlanma modeli, GPU damgası.
   `render_offscreen` wgpu'ya döner (phase-4).
5. Geçiş: `Pacer` + `tick`, macOS gerçeklemesi ve katman `bt-shell`'de. wgpu
   ürün bağımlılığı, Metal `cfg(test)` kâhini. Duman ve pencere yolu ölçümü
   (phase-5; ölçüm (a) isterse eskale → `phase-5b`).
6. Duman sabitlerinin yeniden gözlemi (phase-6).
7. Metal'in sökümü, denetim ve sözleşme belgeleri (phase-7).

Gerekçeler `discussion.md` → Karar 1–11.

## Kapsam Dışı

- `FontSystem` trait'i ve Linux font yığını, `bt-shell` ayrımı,
  `bt-shell-linux` (winit), Linux platform hizmetleri, paketleme. Sırası ve
  bağımlılıkları `docs/YOL-HARITASI.md`'de.
- `bt-gpu`'nun Linux'ta derlenip lavapipe üstünde sınanması. `bt-atlas`
  CoreText'ten ayrılınca, font setinde.
- Linux `Pacer`'ı ve yüzeyin `raw-window-handle` girişi. winit setinde.
- Materyal yüzey ve yeni görsel özellik. Bu set görüntüyü değiştirmez.
- Uzak CI. Aynı imaj tarifiyle sonra eklenebilir.

## Akış

```
phase-1  make linux ─────────────────────────────── (bağımsız, tek başına değerli)
phase-2  deneme: cell_bg+caret (cfg(test)) ── /measure ──► durak? ── evet ─► eskale, set durur
                                                              │ hayır
phase-3  cell+emoji  ─┐                                       ▼
phase-4  glyph_fx+selection+tamamlanma ─ render_offscreen → wgpu
phase-5  Pacer + tick + bt-shell katmanı ── /measure ──► durak? ── evet ─► eskale, phase-5b (a)
phase-6  IDLE_FRAME_LIMIT / QUIET_FLOOR gözlemi (ayrı commit)
phase-7  Metal söküm + denetim + belgeler + set kapısı
```

İki `durak?` düğümündeki `/measure` `[~]` olamaz: ölçüm yoksa phase kapanmaz ve otonom şerit eskale eder.

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | |
| phase-3 | |
| phase-4 | |
| phase-5 | |
| phase-6 | |
| phase-7 | |
| kapı | |
