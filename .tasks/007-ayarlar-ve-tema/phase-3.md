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

- **tema / materyal biçimi** — ikinci gömülü tema `bateri-light`; biçim
  değişmedi, kullanıcı temaları olduğu gibi okunur (taban hâlâ `bateri`).
- **ayar şeması** — `theme = "system"` varsayılan (eski varsayılan
  `"bateri"`), yeni `light_theme` (`"bateri-light"`) ve `dark_theme`
  (`"bateri"`); `"system"` ayrılmış değer. Dosyasız kullanıcıda davranış
  değişir: açık modda açık tema. `docs/AYARLAR.md` → `[appearance]`, hata
  tablosu, Temalar.
- Koyu temada SGR 2'li adlı renklerin değerleri birkaç basamak açılır
  (bilinçli, bekçi diff'i); varsayılan ön planın sönüğü (`dim` rolü) aynı.
- `CLAUDE.md` bugünkü hâl ve "Tema" maddesi; `bt-shell` `lib.rs` başlığı.
- Yeni bağımlılık yok: `objc2-app-kit`'e `NSAppearance` bayrağı,
  `Cargo.lock` değişmedi.

## Checklist

- [x] Sönük kural (zemine karıştırma), bekçinin bilinçli güncellenmesi
- [x] `Theme::BATERI_LIGHT` ve palet bekçisine eklenmesi
- [x] `system` / `light_theme` / `dark_theme` ve saf seçim fonksiyonu
- [x] `Session::set_theme` + kare isteği
- [x] Görünüm değişimi: view → hedefsiz eylem → uygulayıcı; hermetik dal
- [x] Test: sönük kural iki zeminde, seçim fonksiyonu, takas sonrası kare
- [x] `docs/AYARLAR.md`
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris` + `make duman`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi (paylaşılan durum)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

- **Sönük kural tam sayıda:** kanal başına `(2·renk + zemin) / 3`, kesmeyle
  (`color::dim_toward`). Siyah zeminde vte'nin `× 2/3`'üyle 256 değerin
  hepsinde bit bit aynı (`dim_on_black_is_vte`) — oranın gerekçesi bu.
  Palet tablosunun sönük sekizlisi (259..=266) de aynı kuraldan. Hedef
  temanın zemini, OSC 11 ile değişmiş zemin değil. `BATERI.dim` (`#909093`)
  yerinde kaldı: rol bir değer.
- **Test-first:** kural değişince eski literallerle tam üç çizim yolu
  sınaması düştü (sönük kırmızının üç yeri), rol değerleri geçti; literaller
  sonra bilerek güncellendi. `set_theme` sınaması mutasyonla sınandı (kare
  isteği kaldırılınca kırmızı).
- **`Session::set_theme` aynı temada no-op:** görünüm bildirimi vurgu rengi
  ve kontrast değişiminde de geliyor, koşulsuz kare boşta sıfır kareyi
  bozardı.
- **Bayrak kök `Cargo.toml`'da değil `bt-shell/Cargo.toml`'da** (crate'in
  `NSLocale`/`CALayer` deyişi). `Cargo.lock` değişmedi; phase yalnız
  paylaşılan durum yüzünden riskli.
- **Ayarlar artık saklanıyor** (`Ivars.settings`): görünüm uygulayıcısı
  `theme_for` için dosyayı yeniden okumadan seçmeli. `load_settings` yalnız
  temayı döndürüyor, `start_session` `scrollback`'i ivar'dan okuyor.
- **Yedek görünüme uyuyor, açılışta da görünüm değişiminde de** (R3.4
  "açılışta `bateri`, canlıda önceki tema" diyordu): bulunamayan ya da bozuk
  tema açık modda `bateri-light`'a, koyuda `bateri`'ye döner
  (`ThemeLoaded::or_embedded`, `Settings::default().theme_for(dark)`); iki
  yol tek yardımcıdan (`AppDelegate::choose_theme`). "Önceki tema kalır"
  görünüm değişiminde yanlış: ekrandaki tema öteki görünümün. O kural
  görünüm aynıyken tema dosyasının bozulduğu canlı yenilemeye (phase-4)
  kalıyor.
- **`light_theme`/`dark_theme` `"system"`'i reddeder** (kendine dönen
  seçim); `SYSTEM_THEME` `pub(crate)`, `bt-shell` `follows_system()`'e bakıyor
  ve sabit temada görünüm bildirimini dosya okumadan bırakıyor.
- **Koyu mu sorusu `NSApp.effectiveAppearance`'tan**, `bestMatchFromAppearancesWithNames`
  ile (yüksek kontrastlı koyu da koyu sayılır); view'dan okumaya gerek
  kalmadı. View `super`'i çağırıp hedefsiz eylemi `sendAction:to:from:` ile
  atıyor.
- **`bateri-light` değerleri** koyu temanın deyişiyle seçildi; WCAG kontrast
  oranı hesapla bakıldı (normal renkli sekizli zemine karşı ≥ 4.5:1, parlak
  sekizli ≥ 3.5:1, imleç bloğu üstündeki zemin rengi ≥ 5:1). `dim` rolü
  kuralın kendisinden (sınamada bağlı).
- **Belge bloğu sınaması genelleşti:** `documented_blocks_are_the_embedded_themes`
  iki gömülü temayı da `AYARLAR.md`'ye bağlıyor; başlık satır sonuyla
  aranıyor (`bateri` `bateri-light`'ın öneki).
- **Duman makine açık moddayken** iki koşuda `kare=1 hucre=8 glif=6
  kural=15`. Yarış sınaması `race_set_theme_and_frame` eklendi, iki profil
  yeşil.
- **Göz kontrolü** geçici `HOME` + renk örneği basan kabukla. Açılış açık
  modda `bateri-light`'la geldi; renkli ve parlak sekizli, sönük satır,
  `ls -G` ve `git diff` okunur, alt başlık boş. **Canlı geçiş** commit'ten
  sonra, yalnız pencerenin dikdörtgeni yakalanarak: sistem koyu → açık
  çevrilince pencerenin zemini açık → koyu → açık döndü, kabuk çıktısı
  yerinde kaldı, stderr boş. Pencere öne alınamadı (yarısı başka pencerenin
  arkasındaydı), görünen yarısı yetti. Bu kabloyu (view → hedefsiz eylem →
  uygulayıcı) **hiçbir sınama görmüyor**; tek kanıtı bu göz kontrolü.
- **`/code-review` (high) kararları.** Düzeltilen (düşük): ilk hâlde görünüm
  değişiminde kullanılamayan tema "ekrandaki kalır"dı; `dark_theme` bozukken
  koyu → açık → koyu geçiş açık temayı koyu modda bırakıyordu. Görünüm
  değişimi artık açılışın kuralından geçiyor, `live()` silindi, senaryo
  `appearance_switches_never_leave_the_other_appearances_theme`'de. Temiz
  bulunanlar: `dim_toward` taşmasız ve siyahta vte'yle aynı, `bateri-light`'ın
  `dim`'i kuraldan, `set_theme`'in kilit sırası, takas sonrası clear/imleç
  rengi, hedefsiz eylemin iki yolu (key pencere ve etkin olmayan uygulama),
  süreli koşunun görünümü okumaması, belge bloğu aramasının öneki.
