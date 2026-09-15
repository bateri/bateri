# Phase 3 — Açık tema ve sistem görünümü

## Özet

Sönük rengi zemine göre karıştır, gömülü `bateri-light`'ı ekle ve temayı
macOS'un açık/koyu görünümüne göre **canlı** seç.

_Requirements: R4, R4.1, R3.1, R10_

## Değişiklikler

- **`crates/bt-core/src/color.rs`** — **sönük kural:** SGR 2'li renk temanın
  zeminine doğru karıştırılır, sRGB 8-bit uzayında (vte'nin `×2/3`'üyle aynı
  uzay, lineerleştirme sınırda kalır). Karıştırma oranı bir tasarım sabiti;
  doc'u "ölçüm değil" der ve seçim gerekçesini yazar. `dim` rolünün dalı
  (phase-2) değişmez. phase-2'nin çizim yolu bekçisi **bilerek** yeni
  değerlere güncellenir — değişikliğin tek izi o sınamanın diff'i.
  `dim_colors_are_darker_than_source` kuralın yeni hâline göre yeniden
  yazılır: koyu zeminde koyulaşır, açık zeminde açılır.
- **`Theme::BATERI_LIGHT`** — açık tema: dört rol + 16 ANSI. Tasarım
  ölçütleri doc'ta: ANSI adlarının anlamı korunur (0 koyu, 7/15 açık uç),
  sarı ve camgöbeği açık zeminde okunur kalır, imleç zeminden ayrışır.
  Değerler göz kontrolüyle kabul edilir; palet bekçisine eklenir.
- **`crates/bt-core/src/settings.rs`** — `[appearance] theme` artık
  `"system"` (varsayılan) ya da ad; `light_theme` (varsayılan
  `bateri-light`), `dark_theme` (varsayılan `bateri`). Saf seçim fonksiyonu:
  (ayar, görünüm koyu mu) → tema adı.
- **`crates/bt-core/src/session.rs`** — `Session::set_theme`: yaprak kilit
  altında takas, ardından kare isteği (`request_frame`; alacritty'nin hasarı
  okunmuyor, `:413-416`). Açık bir kare ile takas aynı ana thread'de, yarış
  yok; okuyucu thread'in renk sorgusu kilitle ayrışır.
- **`crates/bt-shell/src/view.rs`** — `viewDidChangeEffectiveAppearance`:
  view uygulayıcıya **hedefsiz eylemle** ulaşır (responder zinciri → app
  delegate); view'a oturumdan başka referans eklenmez (`view.rs:130-156`).
- **`crates/bt-shell/src/app.rs`** — görünüm uygulayıcısı: etkin görünümün
  koyu olup olmadığını okur, seçim fonksiyonundan adı alır, ad çözümünden
  (phase-2) geçirip `set_theme`. Açılışta da aynı yol. **Hermetik dal:**
  süreli koşuda görünüm yoksayılır, tema `Theme::BATERI` — duman makinenin
  açık modundan etkilenmez.
- **Kök `Cargo.toml`** — `objc2-app-kit` feature listesine görünüm için
  gerekenler (`NSAppearance`); `Cargo.lock` değişirse phase riskli sayılır.
- **`docs/AYARLAR.md`** — `theme = "system"`, `light_theme`, `dark_theme`;
  `bateri-light`'ın tam bloğu (sınamayla bağlı); sönük renk davranışı.

## Kabul

- Sönük kural sınamaları: koyu temada sönük ön plan zeminle ön plan arasında;
  açık temada sönük metin **açılır**; ters videolu sönük hücre.
- Çizim yolu bekçisinin diff'i yalnız sönük değerleri değiştirir; palet
  bekçisinin bugünkü 19 değeri aynı.
- Seçim fonksiyonu: `system` + koyu → `dark_theme`; `system` + açık →
  `light_theme`; sabit ad görünümden bağımsız.
- `set_theme` sonrası sıradaki `frame()` yeni temanın zeminini atlar ve
  kare döndürür.
- `make test-yaris` yeşil (takas okuyucu thread'in okuduğu kilidi yazıyor).
- `make duman` jetonları değişmez, makine açık moddayken de.
- Göz: Sistem Ayarları'nda Açık/Koyu değişince pencere anında değişir;
  `bateri-light`'ta `ls --color`, `git diff`, sönük metin okunur.

## Yayın Etkisi

- **tema / materyal biçimi** — ikinci gömülü tema; biçim değişmedi.
- **ayar şeması** — `theme = "system"` varsayılan, `light_theme`,
  `dark_theme`. Dosyasız kullanıcıda davranış değişir: açık modda açık tema.
- Koyu temada SGR 2'li renklerin değerleri kayar (bilinçli, bekçi diff'i).

## Checklist

- [ ] Sönük kural (zemine karıştırma), bekçinin bilinçli güncellenmesi
- [ ] `Theme::BATERI_LIGHT` ve palet bekçisine eklenmesi
- [ ] `system` / `light_theme` / `dark_theme` ve saf seçim fonksiyonu
- [ ] `Session::set_theme` + kare isteği
- [ ] Görünüm değişimi: view → hedefsiz eylem → uygulayıcı; hermetik dal
- [ ] Test: sönük kural iki zeminde, seçim fonksiyonu, takas sonrası kare
- [ ] `docs/AYARLAR.md`
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi (paylaşılan durum)
- [ ] Yayın etkisi yazıldı
