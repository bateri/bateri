# Phase 3 — Yuvarlak köşeli tek parça şekil

## Özet

Satır koşuları kendi fragment'inde yuvarlak dikdörtgen olarak çiziliyor:
açıkta kalan köşe dışbükey yuvarlak, komşu koşuyla birleşen köşe kare,
basamakta içbükey dolgu.

_Requirements: R2.5_

## Değişiklikler

- **`crates/bt-gpu/shaders/cell_bg.metal`** — `selection_fragment` (ve gerekirse
  maskeyi `flat` taşıyan bir vertex çıkışı). `Instance` **aynen**: `rgba`
  yuvası köşe maskesini taşır (köşe başına kare / dışbükey / içbükey dolgu),
  renk ile yarıçap çıplak `float4`/`float` uniform (`caret_fragment`'in
  hizalama kaçışı). Mesafe `rounded_box_sdf`'ten; içbükey dolgu parçası
  r×r'lik karede dairenin **dışını** boyar. Kenar yumuşatması caret'inkiyle
  aynı (±0.5 px `smoothstep`). Yeni `#[repr(C)]` ↔ `.metal` çifti yok; stride
  assert'leri değişmez.
- **`crates/bt-gpu/src/frame.rs`** — koşulardan köşe kararı: bir köşe, o
  kenardaki komşu satırın koşusu o köşeyi örtmüyorsa dışbükey; örtüyorsa kare;
  komşu koşu bu koşunun kenarını aşıyorsa basamağın dışına bir içbükey dolgu
  parçası. Karar saf bir fonksiyonda ve sınanıyor. Yarıçap
  `caret_radius_px(cell_px, bt_core::CURSOR_RADIUS)` — kullanıcının
  `cursor_radius`'u değil (Karar 10); tek hücrelik koşuda yarım boya kırpılır.
- **`crates/bt-gpu/src/renderer.rs`** — altıncı pipeline (`cell_bg_vertex` +
  `selection_fragment`), blend `SourceAlpha`; ızgara geçişinde seçim listesi
  bu pipeline'la. Offscreen bekçi: tek koşunun köşe pikseli zemin, ortası seçim
  rengi; iki satırlı basamakta içbükey köşenin pikseli seçim rengi.
- **`CLAUDE.md`** — pipeline sayısı cümlesi (bugün bayat: `glyph_fx`
  beşinci; seçim altıncı) ve seçimin yüzeyi (köşe dili caret'in varsayılan
  oranı).

## Kabul

- Köşe kararının sınaması: tek satır (dört köşe yuvarlak), iki eşit satır
  (iç köşeler kare), basamak (bir içbükey dolgu), boş ara satırla bölünmüş
  seçim (iki ayrı şekil).
- Offscreen piksel bekçileri yeşil.
- `make shader` ve `make hepsi` yeşil; `make duman` jetonları değişmez.
- Gözle: çok satırlı seçim tek parça, köşeleri caret'le aynı dilde.

## Checklist

- [ ] `selection_fragment` + maske kodlaması
- [ ] Köşe kararı fonksiyonu + sınaması
- [ ] Altıncı pipeline ve encode sırası
- [ ] Test: offscreen köşe ve içbükey piksel bekçileri
- [ ] `CLAUDE.md`
- [ ] Doğrulama geçti (`make shader`, `make hepsi`, `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
