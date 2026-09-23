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

- [x] `selection_fragment` + maske kodlaması
- [x] Köşe kararı fonksiyonu + sınaması
- [x] Altıncı pipeline ve encode sırası
- [x] Test: offscreen köşe ve içbükey piksel bekçileri
- [x] Zeminli seçili hücrenin köşesi (phase-2'den devir): `frame()` seçili hücrenin zeminini düşürüyor, yani yuvarlak köşede renkli bir satırın (vim durum satırı, `\e[7m`) yerine pencere zemini görünecek — gözle bak; çentik okunuyorsa zemini koşunun altında bırak (`session.rs`, `let bg = if selected`)
- [x] Seçim renginin okunurluğu (orkestratör, phase-2'nin gözle kontrolünden): koyu temada `#2b3a50` üstünde ANSI mavi en zayıf okunan metin. Gömülü iki temanın `selection` değerini, 16 ANSI rengi + `foreground` + `dim` seçimin üstünde okunur kalacak şekilde gözden geçir (kontrast oranını hesapla, sayıyı Uygulama Notları'na yaz); seçim yine sakin ve zemine yakın kalsın. Metin rengini değiştirmek Karar 3'ün dışında — yalnız rol değeri.
- [x] `CLAUDE.md`
- [x] Doğrulama geçti (`make shader`, `make hepsi`, `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi

## Uygulama Notları

- Vertex **ayrı** (`selection_vertex`), planda `cell_bg_vertex` yazıyordu:
  fragment kendi dörtgenini bilmek zorunda ve `cell_bg_vertex`'in çıkışı onu
  taşımıyor. `Instance` aynen; dörtgen merkezine göre `local` (interpolasyonlu)
  + `half_size`/`mask` (`flat`) varying. Maske: köşe başına 1 dışbükey / 0
  kare; içbükey dolgu ayrı `r×r` instance, dairenin merkezi olan köşe `-1`.
- `Frame::push_selection` artık dilimin tamamını alıyor (köşe komşuya bağlı);
  saf karar `frame::selection_corners` → `[Corner; 4]`. Dolguyu yalnız dar
  koşu doğuruyor, yani her basamak bir kez. Çaprazdan değen koşular (üst 5'ten,
  alt 4'e) iki ayrı yuvarlak parça.
- Eski bekçi (`a_selection_run_paints_between_the_ground_and_the_glyph`)
  köşelerden `caret_radius_px` kadar içeride soruyor. İçbükey bekçisi dolgu
  kapatılarak kırmızı görüldü, sonra yeşil.
- Zeminli seçili hücre (phase-2 devri): zemin **düşük kaldı**. Gözle (ters
  videolu durum satırı): köşede birkaç piksellik pencere zemini okunmuyor;
  zemini altta bırakmak seçimin köşesinde açık gri kırık leke bırakırdı.
  Yorum `session.rs`'te.
- Seçim rengi. Ölçüt: zeminde 3:1'i geçen her metin rengi (koyuda
  `black`/`bright_black`, açıkta `white`/`bright_white` zaten geçmiyor ve
  dışarıda) seçimde de 3:1'i geçsin (WCAG oranı). Koyu `#2b3a50` ölçütü
  geçiyordu (en zayıf `red` 3.35, `blue` 4.06) — mavinin zor okunması oran
  değil **ton çakışması** (mavi metin mavi zeminde), WCAG onu ölçmüyor. Yeni
  koyu `#283042` (daha az doygun arduvaz): `red` 3.84, `blue` 4.65, `dim`
  4.14, `foreground` 9.35; zemine karşı 1.59 (eskisi 1.82). Açık `#c9d8ee`
  ölçütü **geçmiyordu** (`bright_yellow` 2.64, `bright_green` 2.73,
  `bright_cyan` 2.81); yeni `#dde6f3`: `bright_yellow` 3.03, `bright_green`
  3.14, `bright_cyan` 3.23, `blue` 4.60; zemine karşı 1.16 (eskisi 1.34).
  Gözle (geçici paket, iki tema): mavi metin okunur, seçim sakin ve görünür.
- Gözle (geçici paket): çok satırlı akış seçimi tek parça, basamakların iç
  köşesi yuvarlak, hizalı kenarlar dikişsiz; çift tık tek kelimede dört köşe
  yuvarlak. Yarıçap caret'inki (13pt@2x'te ~3 px) — küçük ama aynı dil.
- `/code-review` (medium): doğruluk bulgusu yok; tek nit (sınama yardımcısı
  `grid`'in doc yorumunu ayırıyordu) düzeltildi.
