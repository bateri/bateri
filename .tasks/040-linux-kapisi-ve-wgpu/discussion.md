# Linux kapısı ve wgpu renderer'ı — Tartışma

## Kullanıcı kararları (2026-09-28, kesin — yeniden sorulmaz)

Konu kullanıcıyla konuşmada kuruldu. Aşağıdakiler **ne** sorusunun cevabı; bu
dosyanın kararları **nasıl** sorusunun:

1. Linux pencere katmanı **winit**. Sekme, arama çubuğu ve ayar penceresini
   GPU'da kendimiz çiziyoruz.
2. **Wayland birincil**. Kalite kapıları (animasyon ritmi, boşta sıfır kare)
   orada. X11 çalışır ama ritmi best-effort; bu, adıyla yazılı bilinen sınır.
3. Linux MVP'de ayar penceresi de var.
4. Yeni bağımlılıklar **onaylı**: `wgpu`, `winit`, `freetype`, `fontconfig`,
   `harfrust`/`rustybuzz`; gerekirse gerekçeyle `raw-window-handle`, `zbus`
   vb. Her biri kararıyla birlikte `CLAUDE.md`'ye yazılır, `Cargo.lock` depoda
   kalır.

Varılan mimari altı adım:

0. Linux kapısı.
1. `bt-gpu` → wgpu.
2. `bt-atlas` → `FontSystem` trait'i (CoreText | FreeType + fontconfig +
   HarfBuzz).
3. `bt-shell` → `bt-shell-common` + `bt-shell-macos` + `bt-shell-linux`.
4. `bt-shell-linux` (winit).
5. Paketleme (.deb/AppImage).

Hedef katman:
`bateri → bt-shell-{macos,linux} → bt-shell-common → bt-gpu → {bt-atlas, bt-core}`.

## Karar 1: Setlere bölme ve bu setin sınırı → ✅ 040 = Linux kapısı + wgpu; font, shell ayrımı ve sonrası yol haritasında

Altı adım tek sete sığmıyor. Her biri ayrı bir kapı ve ayrı bir risk taşıyor.
wgpu denemesi de yolun geri kalanını **durdurabilecek** tek adım.

**Bu set (040):**

- Linux kapısı.
- wgpu: deneme, pipeline'ların taşınması, ritmin ve yüzeyin geçişi, duman
  sabitlerinin yeniden gözlemi, Metal'in sökülmesi.

`FontSystem` ve `bt-shell` ayrımı bu setin **dışında**. İkisi de denemenin
sonucuna bağlı değil. Phase-2'de meşru olarak durabilecek bir set, o durağa
ihtiyaç duymayan phase'leri taşımamalı. Kalan sıra `docs/YOL-HARITASI.md`'de
`—` satırları olarak yazılı.

Reddedilen yollar:

- **(a) Kullanıcının saydığı ilk dört adım tek sette.** On phase'i aşar ve
  denemenin durağı başka bir işin ortasına düşer.
- **(b) Önce font ve shell ayrımı.** "wgpu olmaz" cevabı Linux yolunu yeniden
  çizer, ve `bt-shell-common`'ın `bt-gpu`'yu nasıl gördüğü o cevaba bağlı.

## Karar 2: Linux kapısı → ✅ Docker, `make linux`, `make hepsi`'nin dışında

`make linux` hedefi depodaki tek bir imaj tarifiyle koşar. Tarifin içeriği:

- Taban `rust` resmi imajı, yerel rustc'nin sürümüne pinli.
- İmajın içinde `zsh` ve `LANG=C.UTF-8`. İkisi de ölçüldü: yerelsiz imajda üç
  ayna sınaması düşüyor (`context.md` → Kanıt).

Koşu:

- `cargo clippy -D warnings` ve `cargo test`, ikisi de `--locked` ile.
  Konteyner depoyu yazılabilir bağlıyor. `--locked` olmadan `Cargo.lock`'u
  yeniden yazabilir ve `make denetim`'in kilit uyarısını sahte tetikler.
- Hedef dizini `target/linux`, crate önbelleği adlı bir volume.
- Bugünkü kapsam yalnız `bt-core`. Kapsamı her set büyütüyor; sırası yol
  haritasında.

**Sürüm eşleşmesi kapının parçası:** yerel `rustc --version` imajın
etiketiyle uyuşmazsa `make linux` kırmızı düşer. Uyuşmazlık "koşamadı"
sayılmaz. Homebrew rustc pin'li değil; iki sürüm sessizce ayrışsaydı Linux
kırmızısı ile clippy kırmızısı birbirinden ayırt edilemezdi.

`make hepsi`'nin içinde **değil**, çünkü kapı komutu bir daemon'a bağlı
olmamalı. `proje.md` → Doğrulama'ya koşullu satır olarak giriyor. Tetik:
Linux'ta derlenen bir crate değişti. `[~]` yalnız Docker yoksa yazılır.

Reddedilen yollar:

- **`cargo check --target …`.** Homebrew rustc'ye hedef std'si eklenemiyor ve
  sınama koşmuyor.
- **Uzak CI.** Deponun uzak CI'ı yok. Aynı tarifle sonra eklenebilir.

## Karar 3: Denemenin kapsamı, geçme ölçütü ve durak → ✅ `cell_bg` + caret, ürün grafının dışında, ölçülmüş durak kuralı

Deneme `cell_bg` + `caret_fragment` (SDF) üzerinde. Seçim sebebi: sRGB,
bit-eşitlik ve SDF bekçileri orada, yani en ayırt edici parça.

**Deneme ürün grafının dışında.** Kurallar:

- wgpu bir **dev-dependency**.
- wgpu renderer'ı `cfg(test)` arkasında.
- Ürün binary'sinin grafı değişmiyor.
- Kullanılmayan `pub` kod ve `#[allow(dead_code)]` yok.

Durulursa geri alma tek `git revert`.

**Kapı** (sınama, `make hepsi`'de):

- Bugünkü bekçilerin wgpu ikizleri geçer:
  - `cell_bg_paints_pixels_on_the_gpu` (`MIDTONE`)
  - `a_degenerate_caret_shape_paints_the_old_rectangle` (bit bit; eşitlik
    arka ucun **içinde** tam)
  - köşe, kenar ve hale bekçileri
- Çapraz arka uç karşılaştırması da geçer (Karar 4).

**Ölçüm** (kapı değil; kullanıcı konunun kendisinde istedi). `/measure` iki
şeyi ölçer:

- offscreen N-kare döngüsü, iki arka uçta aynı `Stats` aralıklarıyla
  (`#[ignore]`'lu bir sınama kancası);
- bugünkü pencere yolunun Metal tabanı (`BT_FRAME_STATS` + `BT_SCROLL_TEST`,
  release ve debug).

Taban ancak şimdi alınabilir, çünkü Metal sökülünce bir daha alınamaz.

**Durak kuralı yeni bir sayı değil, projenin kendi gürültü kuralı**
(`docs/OLCUMLER.md` → Gürültü kuralı):

- Her arka uç için profil başına en az on koşu yapılır.
- CPU aralıklarında wgpu'nun dağılımı Metal'inkiyle **örtüşmüyorsa ve daha
  kötüyse** set durur ve eskale olur.
- Örtüşüyorsa sürer.

**Sonuç (phase-2):** kural tetiklendi (release `cpu_encode_p95` Metal
95–107 µs, wgpu 172–191 µs). Kullanıcı kararı 2026-09-29: (a) mutlak ölçek
kabul — fark kare başına ~80 µs, bütçenin çok altında, boşta sıfır kare
etkilenmiyor; set sürüyor.

GPU sütunu karara girmiyor. Aynı binary'de bile gezindiği kayıtlı
(`docs/OLCUMLER.md` → "GPU sütununun gezintisi"). Bu sütun raporlanır ama
karar vermez. Plana eşik yazılmaz.

## Karar 4: Taşımanın biçimi → ✅ paralel renderer, Metal tek bir sahne listesinde kâhin, bekçiler wgpu'ya taşınır

wgpu renderer'ı Metal'in yanında büyüyor. Metal renderer **kâhin**. Kâhinlik
tek bir karşılaştırma sınamasında yaşıyor:

- Sahne listesi (`Frame` üreten küçük fonksiyonlar) iki arka uçta çiziliyor
  ve piksel piksel karşılaştırılıyor.
- Düz dolgular tam eşit olmalı.
- SDF/AA kenarlarında kanal başına en çok 1/255 fark kabul ediliyor. Bu tek
  LSB, aynı hesabın iki derlemesine tanınıyor; bir ölçüm iddiası değil.
- Her grup taşındıkça listeye sahne ekleniyor.

Bekçiler iki arka uca **parametrik değil**. Bekçi Metal'de zaten yeşil ve
orada yeni bilgi üretmiyor.

- Grup taşınırken grubun **özgül** bekçilerinin wgpu ikizi yazılıyor.
- Renderer tamamlanınca `render_offscreen` (`renderer.rs`, 55 sınamanın ortak
  gövdesi) wgpu'ya dönüyor ve bütün bekçiler orada koşuyor.
- Doğrudan Metal'e inen az sayıdaki sınama wgpu karşılığını alıyor:
  tamamlanma, atlas dokusu, elle kurulan pass.

Gruplar ve sırası:

1. `cell_bg` + caret (deneme).
2. `cell` + `emoji`: atlasın iki düzlemi, `R8Unorm`/`RGBA8Unorm_sRGB`, düz
   alfa.
3. `glyph_fx` + `selection` (arama vurgusu aynı pipeline) + tamamlanma
   modeli ve GPU damgası.

Reddedilen yollar:

- **Yerinde yeniden yazım.** Kâhin kalmaz.
- **MSL'in makineyle çevrilmesi.** naga'nın MSL girişi yok.
- **İki arka uca parametrik bekçiler** (Sadelik jürisi). İki okuma yolunun
  soyutlaması son phase'te silinir, bilgi getirisi yok.

## Karar 5: Küçük değerler → ✅ `Features::IMMEDIATES`, yapı başına uniform buffer kaçışı

Bugünkü `set*Bytes` değerleri WGSL'de `var<immediate>` ile taşınıyor. Metal'de
wgpu onu `set*Bytes`'a indiriyor, Vulkan'da push constant oluyor.

- Bütçe Vulkan'ın garanti ettiği en küçük sınır.
- Onu aşan yapı bir uniform buffer'a gidiyor. Seçim deneme phase'inde yapı
  başına veriliyor, boyutlar ölçülerek.
- Düzen sözleşmesi `#[repr(C)]` ↔ WGSL. Bekçisi her pipeline'ı offscreen
  kuran sınama.

Reddedilen yol: her şeyi uniform buffer'a koymak. Kare başına yazım ve ofset
hizası eklerdi.

## Karar 6: Tamamlanma modeli → ✅ gönderim indeksi + `poll`; bugünkü bloğun dört işi adıyla taşınıyor

Bugünkü tamamlanma bloğunun işleri `renderer.rs` → `completion` ile
`link.rs`'te (Codebase-fit jürisi). Yeni model şu: her gönderimin bir
indeksi var. Tik'in başındaki engellemeyen `Device::poll`, bitmiş indeksi
ilerletiyor. Kare başına closure kurulmuyor; `completion`'ın "kare başına
blok kurulmaz" gerekçesi korunuyor.

Dört iş şöyle taşınıyor:

1. **`kare=` "hatasız biten" kalıyor.** Hata error scope'tan ve
   uncaptured/device-lost geri çağrısından geliyor. Hatalı indeksin karesi
   sayılmıyor, yoksa `make duman` siyah pencereyi yeşil geçerdi.
2. **Hata kolu `Retry::draw_failed`'e gidiyor.** Senkron ve asenkron hata
   bugünkü gibi tek politikadan geçiyor.
3. **`acilis=` ilk karenin bittiği anı ölçmeye devam ediyor.** Poll'un
   yapıldığı anı değil.
4. **Uykudan önceki son kare kaybolmuyor.** Link uyurken gönderilmiş ama
   bitmemiş kare varsa `Pacer`'ın gecikmeli uyandırmasına tek bir
   engellemeyen poll kuruluyor (saatin hareket tadı: hasar dikmiyor).
   **Durma koşulu:** kuyruk boş. Yeni thread ya da periyodik zamanlayıcı yok.

Kapanıştaki bekleyen poll, raporun okunmasından önce geliyor.

GPU deltası `TIMESTAMP_QUERY` ile ölçülüyor. Adaptör desteklemiyorsa ölçüm
jetonunun **anahtarı kalıyor**, değeri `unsupported` oluyor (jeton silinmez,
eklenir).

## Karar 7: Ritim → ✅ dört görevli `Pacer` + dışarıdan verilen hedef; (b) önce, (a) ölçüm isterse

(b) yolu AppKit istiyor (`NSView.displayLink`), yani link'e bir dikişin
girmesi bu sette zorunlu.

`link.rs`'in platforma değen **dört** görevi var. Dördüncüsünü Codebase-fit
jürisi buldu:

1. vsync tik'i.
2. Her thread'den `set_running` (bugün `MainThreadBound` + `setPaused`).
3. Gecikmeli tek uyandırma (bugün `dispatch2::after`).
4. **Zaman tabanı.** Tik'in damgası `dt`'yi, içerik son tarihini, blink'i,
   `arm_clock`'un gecikmesini ve `quiet_since`'i (`sessiz=`) besliyor.
   Kod, iki ayrı saatin okunmasını adıyla yasaklıyor.

`Pacer`'ın sözleşmesi:

- `now()` tik damgasıyla **aynı tabanda**.
- Damga, sağlayıcı biliyorsa **hedef sunum anı**, bilmiyorsa `now()`. Hangisi
  olduğu sözleşmede yazılı.

Callback'in bütün mantığı platformsuz bir `tick(damga, hedef)`'e taşınıyor.
Hedef iki biçimde gelebiliyor:

- **"yüzeyden al"**, yani (b);
- **"bu dokuyu kullan"**, yani (a).

Böylece iki kol aynı dikişe oturuyor. Ölçümün sonucu crate'ler arasında
taşıma doğurmuyor.

- **(b) önce:** display link yalnız zamanlayıcı. Drawable'ı wgpu kendisi
  alıyor.
- **(a) yalnız ölçüm isterse:** `CAMetalDisplayLink` kalıyor, doku
  `create_texture_from_hal` ile sarılıyor. İkinci bir Metal bağlama yığını
  girmiyor (`context.md` → Kanıt). Sağlayıcı Metal tipi gördüğü için
  `bt-gpu`'da `cfg(target_os = "macos")` arkasında yaşıyor ve denetimin
  istisnası adıyla yazılıyor.

**Durak (b) için de Karar 3'ün kuralı:** pencere yolunun dağılımı Metal
tabanıyla örtüşmüyorsa ve kötüyse eskale edilir, (a) phase'i açılır.

Bedel kayıtlı: (b) kaybederse zamanlayıcı sağlayıcısı boşa yazılmış olur.
Dikiş ve `tick` kalır.

macOS gerçeklemesi `bt-shell`'de yaşıyor. Shell ayrımı setinde
`bt-shell-macos`'a **dosya olarak** taşınır. Linux gerçeklemesi winit setinde
geliyor ve trait orada ikinci gerçeklemesini görünce **yeniden açılabilir**
(Sadelik jürisi). Bu bilinçli bir erteleme; bugünkü şekil tahmin değil,
macOS'un dört görevi.

`bt-gpu`'nun "platformsuz" iddiası bu sette yalnız **doğrudan
bağımlılık + kaynak** olarak doğrulanıyor (denetim). Linux'ta gerçekten
derlendiği font setinde görülüyor, çünkü `bt-atlas` bugün CoreText.

Reddedilen yol: trait yerine tek bir closure enjeksiyonu (Sadelik jürisi).
Dört görevi, özellikle zaman tabanını bir closure çiftine sığdırmak, sessiz
bir ikinci saat doğururdu.

## Karar 8: Yüzeyin sahipliği → ✅ `CAMetalLayer` `bt-shell`'de, wgpu yüzeyi ondan

`bt-shell` katmanı kurup view'a takıyor. Davranış bugünkü
`pane.rs` → `setLayer` ile aynı, sahiplik bir kat yukarı çıkıyor. `bt-gpu`
wgpu yüzeyini o katmandan açıyor: tek `unsafe` giriş
(`SurfaceTargetUnsafe::CoreAnimationLayer`), ve `bt-shell` wgpu tipi görmüyor.

- Ölçek (`contentsScale`) katmanın sahibinde.
- Piksel boyutu yüzeyin yapılandırmasında.
- Katman tablosunda `bt-shell`'in `objc2-quartz-core` satırı ("yalnız
  `CALayer` takma") aynı commit'te düzeliyor.

Linux'ta aynı giriş winit'in `raw-window-handle`'ı olacak.

Reddedilen yol: wgpu'nun `NSView` tutamacından kendi alt katmanını kurması.
View'ın katman barındırma düzenini değiştirirdi (033 → R4.1).

## Karar 9: Shader doğrulama → ✅ WGSL gömülü, pipeline kuran sınama; sözleşme satırları WGSL'in girdiği commit'te

- `.wgsl`'ler `include_str!` ile gömülüyor.
- Her pipeline'ı offscreen kuran sınama `make hepsi`'de. Bugünkü
  `metallib_is_embedded_and_valid`'in ardılı o.
- `proje.md`'nin üç yeri **deneme phase'inin kendi commit'inde** güncelleniyor
  (İşletme jürisi). Yoksa aradaki phase'lerde bir `.wgsl` düzen değişikliği ne
  `/code-review`'u ne kanaryayı tetiklerdi. Güncellenen yerler:
  - Doğrulama: "`.wgsl` değişti" satırı, `make shader`'ın `.wgsl` kolu;
  - Riskli phase tetikleyicisi: `#[repr(C)]` ↔ WGSL;
  - `/audit` merceği 6.
- `build.rs` ve `xcrun` şartı Metal'le birlikte son phase'te gidiyor; o zaman
  `CLAUDE.md`'ye tek cümle giriyor.

## Karar 10: wgpu'nun özellik seti → ✅ `std`, `wgsl`, `metal`, `vulkan`

`default-features = false`. `dx12`, `gles` ve `webgpu` kapalı. Gerekçe
`Cargo.toml` yorumunda.

`pollster` yok: yerel arka uçta future'lar hazır dönüyor ve std'nin
`Waker::noop`'uyla küçük bir `block_on` yetiyor.

## Karar 11: Geçiş sürerken `bt-gpu` ve duman sabitleri → ✅ donukluk notu + ayrı gözlem commit'i

- **040 bitene kadar `bt-gpu`'ya dokunan başka set açılmıyor.** Her shader
  düzeltmesi iki arka uca yazılmak zorunda kalırdı. Not yol haritasında
  (İşletme jürisi).
- **Duman sabitleri geçişten sonra yeniden gözleniyor.** `IDLE_FRAME_LIMIT`
  ve `QUIET_FLOOR`'un doc'u "kare yolunu değiştiren set gelince yeniden
  ölçülür" diyor, ve (b) sessizliğin saatini, Karar 6 kare sayımının yerini
  değiştiriyor. Sağlıklı ve bozuk dağılım gözleniyor, debug ve release.
- **Değer değişikliği kod phase'lerinden ayrı commit** (`proje.md` →
  Doğrulama). Bu setin kendi phase'i o.

## Muhakeme (2026-09-28)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU — Pacer tek gerçeklemeyle erken; bekçileri iki arka uca parametrik yapmak kâhini iki kez ödetiyor |
| Codebase-fit | SORUNLU — Pacer'ın dördüncü görevi (zaman tabanı) eksik; tamamlanma bloğunun dört işinden yalnız biri taşınıyor; ölçüm sonucu trait'in biçimini değiştiriyor |
| İşletme | SORUNLU — phase-2'de durulursa ara durum kirli ve "kötü" tanımsız; duman sabitleri ve `proje.md` satırları geçişte yeniden türetilmiyor; Docker kapısında sürüm ayrışması ve `--locked` |

**Kabul edilen itirazlar ve plana etkileri:**

- **Codebase-fit → zaman tabanı.** `Pacer` dört görevli; `now()` tik
  damgasıyla aynı tabanda, damganın anlamı sözleşmede (Karar 7).
- **Codebase-fit → tamamlanma.** Gönderim indeksi + `poll` modeli. `kare=`'nin
  "hatasız" anlamı, `Retry`, `acilis=`'ın anı ve uykudan önceki karenin
  akıbeti adıyla taşınıyor (Karar 6).
- **Codebase-fit → hedef dışarıdan.** `tick(damga, hedef)`; (a) ile (b) aynı
  dikişe oturuyor (Karar 7).
- **Codebase-fit → katman tablosu.** `objc2-quartz-core` satırı düzeliyor;
  denetimin doğrudan-bağımlılık ölçütü ve (a) istisnası adıyla yazılıyor.
- **Sadelik → parametrik değil.** Bekçiler wgpu'ya taşınıyor, Metal yalnız
  sahne listesinde kâhin (Karar 4).
- **Sadelik → erken soyutlama.** `Pacer`'ın winit setinde yeniden
  açılabileceği ve macOS gerçeklemesinin iki kez (dosya olarak) taşınacağı
  adıyla yazıldı (Karar 7).
- **İşletme → kirli ara durum.** Deneme dev-dependency ve `cfg(test)`; wgpu
  renderer'ı geçiş phase'ine kadar ürün grafına girmiyor, dead-code yok, tek
  revert (Karar 3).
- **İşletme → "kötü" tanımsız.** Durak projenin gürültü kuralıyla: on koşu,
  örtüşmeyen ve kötü dağılım. GPU sütunu karar dışı (Karar 3, 7).
- **İşletme → sözleşme satırları.** `proje.md`'nin üç yeri WGSL'in girdiği
  commit'te (Karar 9).
- **İşletme → duman sabitleri.** Ayrı gözlem phase'i (Karar 11).
- **İşletme → Docker.** Sürüm uyuşmazlığı kırmızı, `--locked`, `[~]` yalnız
  Docker yokken (Karar 2).
- **İşletme → donukluk.** Yol haritasında not (Karar 11).

**Reddedilenler:**

- **Sadelik → trait yerine closure enjeksiyonu.** Zaman tabanı ve
  set_running/wake_after birlikte düşünülünce closure çifti ikinci bir saatin
  kapısını açıyordu. Jürinin gözlemi ("tek gerçekleme, şekli tahmin") kabul
  edildi; erteleme notu yazıldı.
- **Sadelik → önce (a) inşa et.** Kullanıcının kuralı ölçümle karar. (b)
  Linux'un şekli ve en sade yol. (a)'nın bedeli dikişin hedef parametresiyle
  küçüldü.
- **İşletme → phase-2 sonu koşulsuz kullanıcı durağı.** Durak kuralı artık
  tanımlı ve projenin kendi kuralı. Koşulsuz durak, kullanıcının "kötüyse
  eskale" cümlesini "her durumda eskale"ye çevirirdi.

## Karar (2026-09-28, kullanıcı kararı 1–4 + otonom akış)

- **Seçilen:** Karar 1–11'deki ✅'ler, yani 040 = Linux kapısı + wgpu. Deneme
  ürün grafının dışında başlıyor. Paralel renderer ve Metal kâhin olarak
  sahne listesinde. Dört görevli `Pacer`, (b) önce. Tamamlanma gönderim
  indeksiyle. Duman sabitleri ayrı gözleniyor, Metal son phase'te sökülüyor.
  Seçimler panelden geçmiş otonom öneri. Ne yapılacağı (Linux, winit, Wayland
  birincil, MVP'de ayar penceresi, onaylı bağımlılıklar) kullanıcı kararı.
- **Reddedilen:** yukarıdaki kararların "Reddedilen" satırları ve Muhakeme →
  Reddedilenler.
