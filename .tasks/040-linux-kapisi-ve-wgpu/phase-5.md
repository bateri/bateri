# Phase 5 — Geçiş: `Pacer`, platformsuz `tick`, katman `bt-shell`'de

## Özet

Pencere yolunu wgpu renderer'ına bağlamak:

- `link.rs`'in dört platform görevi bir `Pacer`'ın arkasına giriyor.
- Callback mantığı platformsuz `tick(damga, hedef)`'e taşınıyor.
- macOS gerçeklemesi ((b) yolu) ve `CAMetalLayer` `bt-shell`'e çıkıyor.
- wgpu normal bağımlılık oluyor. Metal renderer yalnız `cfg(test)`
  kâhini olarak kalıyor.
- Pencere yolu ölçülüp durak kuralından geçiriliyor.

_Requirements: R4.1, R4.2, R4.3_

_Kısıt: yazılan/taşınan kodun yorumları, doc-comment'leri ve tanı metinleri İngilizce (plan.md → Yaklaşım, dil kısıtı)._

## Değişiklikler

### `crates/bt-gpu/src/link.rs`

**`Pacer` trait'i** (`discussion.md` → Karar 7). Dört görevi var:

1. vsync tik'i;
2. her thread'den `set_running`;
3. gecikmeli tek uyandırma;
4. `now()`.

`now()` tik damgasıyla **aynı tabanda**. Damga, sağlayıcı biliyorsa hedef
sunum anı; bilmiyorsa `now()`. Bu sözleşme trait'in doc'unda yazılı, ve
`quiet_since`/`sessiz=` onu okuyor. İkinci bir saat okuması yok (bugünkü
yasak, `link.rs`'in ilgili yorumu).

**Callback'in gövdesi platformsuz bir `tick(damga, hedef)`'e taşınıyor.**

- Hedef iki biçimde gelebiliyor: "yüzeyden al" (b) ya da "bu dokuyu
  kullan" (a).
- Modül başlığının sözleşmesi aynen kalıyor: link paused durur, kare
  istemenin üç yolu, hareketin `Waker::wake`'e dokunmaması, saatin iki tadı,
  uyku testinin dört terimi.
- `Waker` her thread'den `Pacer::set_running`'i çağırıyor.
- `arm_clock` `Pacer`'ın gecikmeli uyandırmasını kullanıyor, tek uyandırma
  kuralı değişmiyor (`due_clock`).
- phase-4'ün "uykudan önceki kare" poll'u da aynı uyandırmaya bağlanıyor.

`dispatch2`, `objc2*`, `CAMetalDisplayLink`, `CACurrentMediaTime` bu
dosyadan çıkıyor.

### wgpu'ya geçen dosyalar

- **`crates/bt-gpu/src/surface.rs`** — wgpu yüzeyi. Tek `unsafe` giriş
  `bt-shell`'in verdiği katman işaretçisi
  (`SurfaceTargetUnsafe::CoreAnimationLayer`). Piksel boyutu yüzeyin
  yapılandırmasında. `framebufferOnly`'nin karşılığı: yalnız
  `RENDER_ATTACHMENT` kullanımı.
- **`crates/bt-gpu/src/renderer.rs`** — ürün yolu wgpu renderer'ı. phase-2–4
  modülü `cfg(test)`'ten çıkıp `Renderer`'ın kendisi oluyor. Metal renderer
  `cfg(test)` kâhini.
  - `system_default`, `cell_metrics`, `set_font`, `font_notice`, sayaçlar:
    **pub API aynı**; `bt-shell`'in çağrıları yalnız yüzey girişinde
    değişiyor.
  - Temizleme rengi `Theme::background_linear`'dan (sRGB sözleşmesi
    aynen).
- **`crates/bt-gpu/Cargo.toml`** — `wgpu` normal bağımlılık. `objc2-metal`,
  `dispatch2` ve `block2` kâhin için yalnız dev-dependency. `build.rs` kâhinin
  metallib'i için phase-7'ye kadar kalıyor.
- **`crates/bt-gpu/src/lib.rs`** — başlık yorumu wgpu'ya ve `Pacer`'a göre
  güncelleniyor. `Pacer` ihraç ediliyor.

### `bt-shell` tarafı

- **`crates/bt-shell/src/pane.rs`** (+ gerekiyorsa yeni bir `pacer` modülü)
  - `CAMetalLayer`'ı kurup view'a takıyor, `contentsScale`'i veriyor.
  - macOS `Pacer`'ı: `NSView.displayLink` zamanlayıcısı ana run loop'ta,
    `set_running`'in `MainThreadBound` karşılığı, `dispatch2::after` ile
    gecikmeli uyandırma, `now()` = `CACurrentMediaTime` ve damga =
    `targetTimestamp`.
  - Görünürlük, örtülme ve `stop`'un yolu bugünküyle aynı.
- **`crates/bt-shell/Cargo.toml`** — `objc2-quartz-core`'a `CAMetalLayer` ve
  zamanlayıcının gerektirdiği özellikler; `objc2-app-kit`'e gerekiyorsa
  display link.
- **`crates/bt-shell/src/app.rs`** — GPU damgası `unsupported` ise jeton
  değeri; anahtarlar silinmiyor.

### Belgeler

- **`CLAUDE.md`**
  - Katman tablosu: `bt-gpu` satırı (wgpu, `Pacer`) ve `bt-shell`'in
    `objc2-quartz-core` satırı ("yalnız `CALayer` takma" düzelir).
  - "Render yolu bloklanmaz" maddesi: `CAMetalDisplayLink` → `Pacer`.
  - Bağımlılık maddesindeki `wgpu` cümlesi "dev-dependency"den ürün bağımlılığına çevriliyor (cümle phase-2'de girdi).

### Ölçüm ve durak (son adım)

- Kullanıcının konuda istediği `/measure`: pencere yolu, phase-2'deki Metal
  tabanıyla aynı yük ve profillerde, en az on koşu.
- Durak kuralı `discussion.md` → Karar 3/7. Tetiklenirse eskale edilir ve
  `phase-5b` açılır ((a): `CAMetalDisplayLink` + `create_texture_from_hal`,
  `bt-gpu`'da `cfg(target_os = "macos")`). Tetiklenmezse sonuç Uygulama
  Notları'na tek satır.
- **Bu adım `[~]` olamaz.** Ölçüm `docs/OLCUMLER.md`'de değilse phase kapanmaz; otonom şerit burada durup eskale eder (waive değil). Kullanıcının "kötüyse eskale" cümlesi ölçümü zaten istiyor; atlamak durağı sessizce kaldırırdı.
- **Duman yalnız `IDLE_FRAME_LIMIT`/`QUIET_FLOOR` yüzünden kırmızıysa** (meşru olarak kayan bir sabit): phase-6'nın gözlemi öne çekilir ve onun ayrı commit'i bu phase'inkinden **önce** iner. Kural (`proje.md` → Doğrulama: sabit değişikliği kod phase'lerinden ayrı commit) iki sırada da tutuyor.

## Kabul

- `make hepsi`, `make test-yaris`, `make shader` yeşil.
- `make duman` yeşil. Jeton satırının anahtarları aynı ve sayaçları
  beklenen değerlerde (`hucre=`/`glif=`/`kural=`/`yuva=`).
- Pencerede gözle: ızgara, dock (yazım efekti dahil), doldurma bandı, seçim,
  arama vurgusu, caret kayması ve blink bugünküyle aynı.
- Boşta sıfır kare: `sessiz=` tabanın üstünde. Sabitlerin yeniden gözlemi
  phase-6'da.
- `bt-gpu/src` üretim kodunda (sınama modülleri hariç) `objc2`, `dispatch2`,
  `block2` yok.

## Checklist

- [x] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [x] `Pacer` (dört görev, zaman tabanı sözleşmesi) + platformsuz `tick(damga, hedef)`
- [x] `Waker`, `arm_clock`, uykudan önceki kare poll'u `Pacer`'a bağlı
- [x] phase-4'ten devir: `WgpuRenderer::poll` tik başında (`on_complete` → `retry.streak.succeeded` / `draw_failed`, `mark_startup`, `GpuSpan` → `record_gpu`), `in_flight()` uykudan önce tek gecikmeli poll'u kuruyor; `draw` bugün `&Target` alıyor → yüzey dokusu; ölçüm kapısı açıkken `set_gpu_timing(true)`, `gpu_timing_supported() == false` → jeton değeri `unsupported`
- [x] wgpu `Surface` + `Renderer` ürün yolunda, Metal `cfg(test)` kâhini
- [x] `bt-shell`: `CAMetalLayer` + macOS `Pacer` ((b))
- [x] Jeton değerleri (`unsupported`), `CLAUDE.md` satırları
- [ ] Test: `make duman` yeşil, jetonlar aynı; gözle üç yüzey
- [x] `/measure` pencere yolu → `docs/OLCUMLER.md`; durak kuralı uygulandı (tetiklendi → eskale)
- [ ] Metal pencere yolu tabanı `3c7a46e`'deki OLCUMLER bloğu (`cpu_encode_p95` 0,26–0,29 / `cpu_kare` 0,07–0,08 / gpu 0,25) — 28 Eylül tablosu değil (gürültülüydü, arada `123ae5d` ve `3c7a46e` regresyon düzeltmeleri girdi; kullanıcı notu, phase-3)
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make shader` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- **Dosya adları phase-7'ye kadar yerinde:** ürün renderer'ı `wgpu_renderer.rs`'teki
  `Renderer` (eski `WgpuRenderer`), Metal olanı `renderer.rs`'te `cfg(test)`
  `MetalRenderer`; `CellMetrics`/`FontNotice`/`scissor_rect_below` ürün olarak
  `renderer.rs`'te kaldı. Taşıma diff'i binlerce sınama satırını oynatırdı, Metal
  sökülünce dosyalar yer değiştirir.
- **Metal'in tamamlanma bloğu ve iki sınaması silindi** (phase-4 onları "Metal
  ürün renderer'ı olduğu için" geri koymuştu); `draw`/`surface` ve sayaçları da
  kâhinde kullanılmadığı için gitti. `endEncoding` bekçisi kâhinin `encode_pass`'ini
  koruduğu için kaldı. `GpuError`'ın `NSError` taşıyan üç varyantı `cfg(test)`.
- **`Pacer`'ın beşinci yöntemi `stop`** — birinci görevin sökümü (link'i
  `invalidate` edip run loop'tan çıkarmak; hedefin tutulmasını kırmak). **Başlatma
  her zaman asenkron** (tik'in kendi thread'inden de): `Retry`'nin tik içinden
  istediği kare, aynı tik'in çıkışta yaptığı `set_running(false)`'a yutulmasın.
  Durdurma tik thread'inde anında. `pending` birleştirmesi macOS pacer'ına taşındı;
  kapının ana kuyrukta ikinci okuması kalktı (pacer kapıyı bilmiyor) — yarışta tik'in
  ilk satırı kapıyı görüp duruyor, (b)'de o tik drawable ödemiyor.
- **`TickTarget` tek varyantlı** (`Surface`): (a)'nın dokusu kendi sağlayıcısıyla
  (phase-5b) eklenir, kullanılmayan varyant yazılmadı.
- **`Ticker` zayıf** (`Weak<Core>`): `displayLinkWithTarget:selector:` hedefi
  güçlü tutuyor, güçlü tutamak çember kurardı. `LinkDelegate` (ObjC sınıfı) düz
  `Core`'a döndü.
- **Drawable yalnız çizen tik'te**: içerik kolunda `take_damage`'dan sonra ve
  CPU aralıklarından **önce** (Metal'in aralıkları da `nextDrawable`'ı görmüyordu),
  hareket kolunda `set_origin`'den önce; uyuyan kol hiç almıyor. Sonuç üç sınıf:
  `Skip` (örtülü/zaman aşımı → hasar geri dikilir, pacer durur, sessiz; geri
  getiren görünürlük bildirimi ya da sonraki uyandırma),
  `Failed` (kayıp/doğrulama → `draw_failed`, yüzey "bayat" işaretlenir ve
  sonraki alım önce yeniden yapılandırır), `Outdated/Suboptimal` → bir kez
  yeniden yapılandırma (suboptimal doku `configure`'dan önce bırakılıyor).
  Hareket kolunda alım olmazsa animasyon hedefinde biter, `Frame`'in efekt
  listeleri boşalır ve hasar dikilir. Yapılandırılmamış yüzey (boyut yok) tik'i hasarı
  tüketmeden durduruyor.
- **`cpu_encode` artık plan + submit + `present`**: Metal'in aralığı da
  `presentDrawable`'ı içeren tamponun `commit`'ine kadardı.
- **Uykudan önceki kare** (Karar 6): `due_clock` üçüncü son tarihi alıyor —
  uçuşta kare varsa `POLL_DELAY` (120 Hz'in bir periyodu, tasarım sabiti) sonra
  hareket tadında `resume`; tik açılışında poll ediyor, çizecek şey yoksa yine
  uyuyor, kuyruk boşalınca kurulmuyor. Tek uyandırma kuralı korunuyor. Kapanışta
  `DisplayLink::drain` (sınırlı bekleme + poll) raporun `kare=`'yi okumasından önce.
- **GPU damgası**: ölçüm kapısı açıkken `set_gpu_timing(true)`; damgalı karede
  okunamayan açıklık `Stats::reject_gpu` ile `gpu_elenen`'e; `TIMESTAMP_QUERY`
  yoksa `gpu_p95=unsupported gpu_max=unsupported` (anahtarlar yerinde).
- **Yüzey**: `alpha_mode = PostMultiplied` (katmanın `opaque`'ı eskisi gibi
  `false`; her piksel zaten alfa 1), `Fifo`, gecikme 2 (→ 3 drawable, eski
  varsayılan), renk uzayı `Auto` → sRGB (katmanın varsayılanı). `configure`
  yalnız boyut değişince (wgpu-core kuyruğu bekliyor). wgpu-hal
  `allowsNextDrawableTimeout`'u `false` yapıyor; örtülü pencerede alım `Occluded`
  dönüyor, kapı da ondan önce.
- `Cargo.lock` değişmedi (wgpu dev-dependency olarak zaten kayıtlıydı).
- `/code-review` (medium) dört bulgu; üçü giderildi, biri waive: (1) `Suboptimal`
  dokusu yaşarken `configure` (`PreviousOutputExists`, Metal'de erişilemez);
  (2) `Lost`/`Validation`'dan sonra aynı boyda yeniden yapılandırma yoktu;
  (4) hareket kolunda başarısız alım `Frame`'in efekt
  listelerini donmuş bırakıyordu.
- **Gözle kontrol koşamadı:** computer-use için macOS Erişilebilirlik ve Ekran
  Kaydı izinleri verili değil; `bateri-dev.app` güncellendi (debug binary),
  kullanıcıda.
- **Üçüncü taraf bildirimleri (kullanıcı kararı 2026-09-30):** wgpu ürün grafına
  MIT seçeneği olmayan üç crate getiriyor — `codespan-reporting` (Apache-2.0),
  `foldhash` (Zlib), `libloading` (ISC); üçü de GPL-3 uyumlu, kabul edildi.
  `tools/third_party_notices.py`'ye izin listesi (`OWN_LICENSES`: Apache-2.0 +
  varsa NOTICE, Zlib, ISC) girdi; listede olmayan MIT'siz lisans yine betiği
  durduruyor. `/` ayraçlı eski ifade (`Apache-2.0/MIT`, rustc-hash) MIT seçeneği
  sayılıyor. `THIRD-PARTY-LICENSES.txt` yeniden üretildi (yalnız ekleme),
  `Credits.html`'in "MIT" cümlesi dört lisansa genişledi.
- **Waive — `/code-review` bulgu 3** ("`Skip` pacer'ı durdurup geri açtırmıyor"):
  denendi (durmadan sonraki tik'te yeniden deneme) ve **ölçülünce geri alındı** —
  hal'in doğumdan örtülü saydığı pencerede (bu oturumda ekran kapalıyken) link
  3 sn'de 359 kez tikledi, çünkü örtülme *değişmediği* için bildirim gelmiyor ve
  kapı açık kalıyor. Durmak doğru: görünür olmak bir değişim, bildirimi
  `set_visible(true)` → kare istiyor. `Timeout` Metal'de erişilemez.
- **Duman ile örtülme — davranış farkı:** wgpu-hal (#8309 çaresi) örtülü
  pencerede drawable vermiyor; Metal yolu örtülü pencereye de çiziyordu. Aynı
  ortamda (ekran uykuda/örtülü) `HEAD` duman yeşil, bu ağaç `kare=0` ile kırmızı;
  ekran açıkken bu ağaç yeşildi (oturumun başında). Kullanıcının gördüğünde fark
  yok (örtülü pencere zaten görünmüyor), ama `make duman` artık görünür bir
  pencere istiyor.
- **Ölçüm (2026-09-30, `docs/OLCUMLER.md` → `## Kare süresi`): durak
  tetiklendi.** `cpu_encode_p95` wgpu 0,39–0,49 ms, Metal tabanı 0,26–0,29 —
  örtüşmüyor, wgpu kötü; tanık `cpu_kare_p95` 0,07–0,10 (taban 0,07–0,08).
  Karar 7'ye göre eskale edildi. **Kullanıcı kararı (2026-09-30): mutlak ölçek
  kabul, (b) kaldı, `phase-5b` açılmadı** — hedef pil ve akıcılık; fark (~0,13 ms,
  ~1,5×) 8,33 ms'lik kare bütçesinin çok altında ve daha önce kabul edilen ~1,7×
  sınırının içinde. (a) kolu bilinen sınır olarak duruyor. Ortam tam sessiz
  değildi (yük ortalaması 4,44; syspolicyd %40, WindowServer %42); tanık sütun
  tabanın bandında.

