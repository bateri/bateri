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

- [ ] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [ ] `Pacer` (dört görev, zaman tabanı sözleşmesi) + platformsuz `tick(damga, hedef)`
- [ ] `Waker`, `arm_clock`, uykudan önceki kare poll'u `Pacer`'a bağlı
- [ ] wgpu `Surface` + `Renderer` ürün yolunda, Metal `cfg(test)` kâhini
- [ ] `bt-shell`: `CAMetalLayer` + macOS `Pacer` ((b))
- [ ] Jeton değerleri (`unsupported`), `CLAUDE.md` satırları
- [ ] Test: `make duman` yeşil, jetonlar aynı; gözle üç yüzey
- [ ] `/measure` pencere yolu → `docs/OLCUMLER.md`; durak kuralı uygulandı
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make shader` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
