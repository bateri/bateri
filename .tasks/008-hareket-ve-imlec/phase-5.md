# Phase 5 — Reduce Motion

## Özet

Sistemin Hareketi Azalt ayarı ve `[motion] reduce_motion` anahtarı her
animasyonu 90 ms'lik belirmeye indirir; ikisi de canlı izlenir, süreli koşu
ikisini de görmez.

_Requirements: R6, R5 (reduce_motion yarısı)_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — `[motion] reduce_motion`, üç değerli
  dizgi: `"system"` (varsayılan), `"on"`, `"off"`. `bool` değil, çünkü
  "sistemi izle" en olası seçim ve `bool`'da onu ifade etmenin tek yolu
  anahtarı **silmek** olurdu — bu dosyada anahtar silinmiyor.
- **`crates/bt-shell/src/app.rs`** — `"system"` iken
  `NSWorkspace::accessibilityDisplayShouldReduceMotion` okunur ve
  `NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification` ile canlı
  izlenir (emsali `apply_appearance`'ın açık/koyu izlemesi; bildirim
  `NSWorkspace`'in kendi merkezinden gelir). Karar **çözülmüş bir değer**
  olarak `bt-gpu`'ya iner: `bt-gpu` AppKit görmez (katman tablosu).
  `Inputs`'un doc'undaki "dört ayrı koşul" listesi beşinciyi kazanır: süreli
  koşu sistem ayarını okumaz, yoksa duman satırı ölçen makinenin
  erişilebilirlik ayarına bağlanırdı.
- **`crates/bt-gpu/src/motion.rs`** — indirgeme tek yerde: açıkken stil ne
  olursa olsun animasyon 90 ms'lik bir **belirme**dir; imleç yeni konumda
  görünür, eski konumda iz bırakmaz (çapraz solma değil — ikinci bir
  dikdörtgen ve alfa karışımı, görünmeyen bir kazanç için). Durma koşulu
  aynı disiplinde: süre dolunca yerleşir.
- **`crates/bt-gpu/src/frame.rs` + `shaders/`** — imleç dikdörtgeni belirme
  boyunca **alfa** taşır. `cell_bg` pipeline'ında harmanlama açık değilse bu
  phase onu açar (ya da imleci kendi harmanlı koluna alır); aynı alfa
  `cell`'in imleç uniform'una da geçer ki blok altındaki metin rengi
  dikdörtgenle birlikte belirsin — yoksa harf, henüz görünmeyen bir bloğun
  rengine boyanır.
- **`docs/AYARLAR.md`** — `### [motion]` bölümüne ikinci anahtar; sistem
  ayarının nereden okunduğu ve önceliği.

## Kabul

- Sistem ayarı açıkken (System Settings ▸ Accessibility ▸ Display ▸ Reduce
  Motion) imleç kaymaz, yeni konumda belirir; ayarı koşu sırasında açıp
  kapatmak pencereyi yeniden başlatmadan etkiler.
- `reduce_motion = "off"` sistem açıkken de kaymayı korur; `"on"` sistem
  kapalıyken de belirmeye indirir.
- Tanınmayan değer yalnız kendi anahtarını varsayılanda bırakır ve tanı
  gösterir.
- `make duman` etkilenmez ve makinenin erişilebilirlik ayarından bağımsızdır.

## Yayın Etkisi

**ayar şeması** — yeni anahtar `[motion] reduce_motion`, varsayılan
`"system"`; `docs/AYARLAR.md` aynı commit'te. **shader** — imleç alfası
`.metal` tarafına dokunuyorsa `make shader` koşar ve uniform düzeni alan alan
doğrulanır. terminfo yok · tema yok · shell entegrasyonu yok · app bundle yok ·
yeni bağımlılık yok.

Gerçekleşen (blok yazılırken bilinmiyordu, bkz. Uygulama Notları): `.metal`
**değişti** (`cell` fragment'i ezme yerine `mix`), `make shader` koştu ve
`CursorBlock`'un düzeni değişmedi — değişen tek şey alfanın **anlamı**
(renk bileşeni değil karışım çarpanı), iki taraftaki `static_assert`/
`offset_of` bağları yerinde. Ayrıca `cell_bg` pipeline'ı harmanlı oldu ve
`Blend` enum'u kalktı; `objc2-app-kit`'in `NSAccessibility` bayrağı açıldı
(yeni crate değil, `Cargo.lock` değişmedi).

`CLAUDE.md`'nin "`reduce_motion` ve sistemin Reduce Motion ayarı her
animasyonu 90 ms'lik solmaya indirir" cümlesi bu commit'le artık koda karşılık
geliyor; 90 ms **seçilmiş** bir sayıdır ve doc'u bunu söyler.

## Checklist

- [x] `settings.rs`: `reduce_motion`, üç değer, varsayılan `"system"`
- [x] `app.rs`: `NSWorkspace` okuması + bildirim gözlemcisi + hermetiklik
      (`Inputs` doc'u)
- [x] `motion.rs`: 90 ms belirme, tek yerde indirgeme, durma koşulu
- [x] İmleç alfası (`frame.rs` + gerekiyorsa shader/harmanlama)
- [x] `docs/AYARLAR.md`
- [x] Test: üç değerin ayrıştırılması; indirgemenin animasyonu 90 ms'de
      yerleştirmesi; hermetik koşunun sistem ayarını görmemesi
- [x] Doğrulama geçti (`make hepsi` + `make duman`, shader değiştiyse
      `make shader`)
- [x] Riskli phase: `/code-review` koştu, bulgular giderildi
- [x] Yayın etkisi yazıldı

## Uygulama Notları

Sapmalar; planın söylemediği ya da başka türlü öngördüğü yerler.

- **İndirgeme dördüncü bir *kip*, dördüncü bir stil değil.** `Motion`'a bir
  `reduce` bayrağı girdi ve stille birlikte `Motion::mode`'da tek karara
  iniyor (`Mode::Snap|Ease|Spring|Fade`). `State::settled` artık stili değil
  kipi alıyor; `Fade`'in durma koşulu `ease`'inkiyle **aynı biçim** (saat ya
  da gidecek yol yok), yalnız sabiti ayrı. Ayar üç değerli kalıyor, yani
  kullanıcıya "azaltılmışken hangi stil" diye bir soru açılmadı.
- **`snap` indirgemenin üstünde ve bu planın söylemediği bir ürün kararı.**
  `cursor_motion = "snap"` diyen kullanıcı hareketi zaten kapatmış; Hareketi
  Azalt'ın oraya bir belirme *eklemesi* erişilebilirlik ayarının anlamını ters
  çevirirdi. `mode()` bu yüzden `Snap`'i bayraktan önce eşliyor; kural
  `docs/AYARLAR.md` ve `CLAUDE.md`'de de yazılı.
- **İki setter de "uçuşta mı" sorusunu eski kiple sormak zorunda ve fade bunu
  iki yeni yoldan kırıyordu** (phase-4'ün `snap` ve uzun-yay derslerinin
  devamı; ikisi de `/code-review` öncesi yazıldı, sınamayla yakalandı):
  - `set_reduce` **iki yönde de** uçuştakini bitiriyor. Kapanışta devralma
    kolu koşsaydı `ease` `from`'u bir önceki hücrede bulur ve imleç geldiği
    yere dönüp yeniden kayardı; `spring` ise hedefinde ve hızsız olduğu için
    anında yerleşir, yani yarı saydam imleci ekranda bırakırdı
    (`reduce_off_mid_fade_does_not_slide_backwards`).
  - `set_style` belirme kipinde `ease` ↔ `spring` için **no-op**: devralma
    `from`'u bulunulan konuma çeker, `from == target` olur ve belirme
    *yerleşmiş* görünür — link o kareyi hiç çizmeden uyur
    (`style_change_during_a_fade_keeps_fading`).
- **`sync` belirme kipinde konumu da anında oturtuyor.** `advance`'ın `Fade`
  kolu boş olduğu için bu satır olmadan imleç ilk içerik karesinde **eski**
  hücresinde alfa sıfırla çizilir ve ancak belirme bitince yerine atlardı —
  `kare`, `icerik` ve `hareket` sayaçlarının üçü de doğru kalırdı, yani kapı
  görmezdi.
- **`cell_bg` pipeline'ı harmanlı oldu ve `Blend` enum'u tamamen kalktı.**
  Belirme bloğun alfasını 1'in altına indiren tek yol; arka planların alfası
  `LinearRgba`'nın tek kurucusu yüzünden her zaman `1.0`, yani onlar için
  harmanlama opak yazmayla birebir aynı (offscreen sınamalar değişmeden
  geçti). İki pipeline da alfa isteyince iki değerli enum tek değere düştü:
  parametre ve tek `if`'i silindi, iki pipeline'ın ayrı durma sebebi artık
  yalnız shader çifti. `Renderer`'ın ilgili üç doc'u aynı commit'te düzeldi.
- **Alfa `LinearRgba`'ya girmedi.** O tip paletin uzayını taşıyor
  (`bt-core`), opaklık ise bu karenin çizim durumu; temaya alfa açmak "yarı
  saydam accent" diye ayrışabilen ikinci bir gerçek doğururdu. `frame.rs`'te
  yerel bir `with_alpha` son bileşeni değiştiriyor ve **iki yere birden**
  yazıyor — blok instance'ına ve `cell` uniform'una. Ayrılsalardı harf henüz
  görünmeyen bir bloğun rengine boyanırdı; `cursor_alpha_reaches_the_block_
  and_the_text` tam bunu tutuyor.
- **Harmanlamanın GPU'da koştuğunu ayrı bir offscreen sınama tutuyor**
  (`cursor_alpha_is_blended_on_the_gpu`). `frame.rs`'inki alfanın listeye
  **yazıldığını** gösteriyor, boyandığını değil — depo kuralının ("CPU sayacı
  GPU'nun boyadığını kanıtlamaz") tam karşılığı; harmanlama bu sınama olmadan
  yalnız α=1'de, yani opak yazmadan ayırt edilemediği noktada kanıtlıydı.
  Ölçüt iki uçta eşitlik (α=0 imleçsiz kareyle **birebir** aynı), ortada sıra:
  renk beklentisini hesaplamak lineer karışımı sRGB'ye kodlamak olurdu ve o
  tablo `bt-gpu`'da yok.
- **`cell.metal`'de ezme karışıma döndü** (`mix(..., inside ? cursor.rgba.a :
  0.0)`). Alfa 1.0'da ifade eski hâliyle birebir aynı, yani yerleşmiş imleçte
  görsel sonuç değişmedi. Karışım yalnız RGB'de: çıkıştaki alfa hâlâ atlasın
  kapsaması, yoksa glyph'in kenarı imlecin opaklığıyla inceltilirdi.
- **`FADE_DURATION` (90 ms) `DT_MAX`'ten (100 ms) küçük ve bu bilerek
  bırakıldı.** Tek bir vahşi kare belirmeyi tek adımda bitirebilir; örtülmenin
  yolu bunu zaten istiyor (`Motion::finish`), geri kalan hâlde bedel bir kez
  görünmeyen bir belirme. İlişki sabitin doc'unda yazılı ki `DT_MAX`'i düşüren
  biri onu kazara anlamlı hâle getirmesin.
- **Hermetiklik saf bir fonksiyona çıktı.** `resolve_reduce_motion(inputs,
  setting, system)` — `system` bir closure, `bool` değil: `"on"`/`"off"`
  diyen kullanıcıda ve süreli koşuda `NSWorkspace`'e hiç gidilmiyor ve sınama
  bunu closure'ın paniğiyle tutuyor (dönüşü `false` sabitlemek, okuyup yok
  sayan bir kodu da geçirirdi). `Inputs` doc'undaki "dört ayrı koşul" listesi
  beşincisini kazandı.
- **Gözlemci `NSWorkspace`'in kendi merkezinde**, varsayılan
  `NSNotificationCenter`'da değil — Apple bildirimi oradan yayınlıyor ve
  yanlış merkeze abone olmak sessizce hiç haber almamak olurdu. Sökülmüyor:
  `AppDelegate` sürecin ömrü boyunca yaşıyor (görünüm izlemesinin deseni).
  `objc2-app-kit`'in `NSAccessibility` bayrağı açıldı; yeni crate değil, var
  olan bağımlılığın bayrağı ve `Cargo.lock` değişmedi.
- **`Changes`'e ayrı alan eklenmedi:** iki `[motion]` anahtarı da aynı yere,
  aynı çağrı yerinde gidiyor. `reload_settings`'te `apply_reduce_motion`
  ayarlar yazıldıktan **sonra** çağrılıyor, çünkü o yol değeri ivardan okuyor
  — üç çağıranı (açılış, sistem bildirimi, kayıt) ortak olsun diye.
- **`/code-review` iki bulgu verdi, ikisi de gerçekti ve ikisi de düzeltildi**
  (skill kendini arka planda başlattı — benim seçimim değil; yoklamadan
  bildirimini bekledim, `proje.md`'nin yasakladığı şey "başlat ve yokla"):
  - **`advance`'ın yerleşme kolu `from`'u bayat bırakıyordu.** `pos` ve `vel`
    hedefe çekiliyordu ama `from` kaymanın çıkış noktasında kalıyordu; oysa
    `ease` ile `fade`'in durma koşulu tam olarak `from == target`. Yayın taşma
    kırpmasıyla **erken** oturan bir kayma (küçük `elapsed`) bu yüzden kip
    değişince "uçuşta" görünüyordu — üstelik link o kareyi yerleşmiş sayıp
    çoktan **uyumuş** oluyor, yani `advance` bir daha koşmuyor. Süreli koşuda
    var olmayan bir animasyon yüzünden `MotionUnsettled`, belirmede de
    sebepsiz yarı saydam bir imleç. Çare tek satır: yerleşme kolu artık
    `finish()` ile **aynı** durumu bırakıyor. Sınama önce yazıldı, kırmızı
    düştüğü görüldü, sonra düzeltildi
    (`a_flight_that_settles_early_stays_settled_in_every_mode`).
  - **Hareket dalı `FailureStreak`'in durağını atlıyordu.** O tipin sözleşmesi
    "art arda ikinci hatada bayrak dikilmez, sıradaki callback hasar bulamaz
    ve uyur" idi; 008'den beri "hasar yok" dalı koşulsuz uyumuyor, yerleşmemiş
    animasyon varken çiziyor. Yani kalıcı bir çizim hatası, kaymanın süre
    tavanı (0,7 sn) dolana kadar tazeleme hızında hata satırı basardı — tam
    olarak `FailureStreak`'in önlemek için yazıldığı şey. `Retry::draw_failed`
    artık "bütçe bitti mi" dönüyor ve hareket dalı bütçe bitince animasyonu
    hedefinde bitiriyor: iki durak birlikte çalışıyor. Hasar yolu dönüşü
    okumuyor, orada durak zaten bayrağın dikilmemesi.
- **Duman koşusu (bu oturum, debug):** `kare=30 hucre=8 glif=6 kural=15
  istek=4 icerik=3 hareket=27 sessiz=1741.82ms kapanis=clean`, çıkış 0 —
  phase-4'ün satırıyla **birebir** aynı sayılar. Hermetik koşu sistem ayarını
  da ayar dosyasını da okumadığı için beklenen buydu. Phase-3/4'te ajanın
  oturumunda kırmızı düşen kapı bu kez yeşil koştu; o sınır (compositor'ün
  pencereyi sürmemesi) ortadan kalkmış değil, bu koşuda tetiklenmedi.
- **Bildirim zinciri `[elle]` doğrulandı — otomatik kapısı yok ve olamaz.**
  `NSWorkspace`'in bildirimi → `accessibilityDisplayDidChange:` →
  `apply_reduce_motion` → `set_reduce_motion` yolunu hiçbir sınama tutmuyor:
  yanlış merkeze abone olmak ya da seçimi kaçırmak **sessizce** hiç haber
  almamak olurdu, yani belirti "hata" değil "hiçbir şey olmaması". Kullanıcı
  koşuda doğruladı: Sistem Ayarları ▸ Erişilebilirlik ▸ Ekran ▸ Hareketi
  Azalt'ı **açık pencerede** aktifleştirince imleç animasyonu değişiyor,
  azalıyor — yani ayar yeniden başlatmadan uygulanıyor (`## Kabul`'ün ilk
  maddesi). Sistemdeki açılış değeri `reduceMotion = 0` idi, yani gözlenen
  şey gerçekten geçişin kendisi.
  **Açık kalem:** hızlı yazarken belirmenin titreme gibi okunup okunmadığı
  henüz bakılmadı. Plan çapraz solmayı bilerek dışarıda bıraktı
  (`plan.md` → Kapsam Dışı) ve bu bir kod kusuru değil bir tasarım kararı;
  ölçen tek araç göz, yani phase-6 da kapatamaz.
- **Bir kez görülen SIGSEGV kaydediliyor, gizlenmiyor.** `cargo test -p
  bt-shell --lib` paralel koşuda bir kez sinyal 11 ile düştü (bütün sınamalar
  `ok` bastıktan sonra, süreç çıkışında). Sonrasında aynı binary'de **20/20**
  temiz koştu, tek thread'de 89/89 geçti, dokunulmamış HEAD'de de 6/6 temiz —
  yani ne tekrarlanabildi ne de bu phase'e bağlanabildi. Muhtemel zemin
  pencere/AppKit'e dokunan sınamaların (`clipboard`) ana thread dışında
  koşması; kapıya bağlanmadı, `make hepsi` yeşil.
