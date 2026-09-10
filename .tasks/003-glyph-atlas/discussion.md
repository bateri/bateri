# Glyph ve atlas — Tartışma

Bu set bir tasarım sorusu değil, birbirinden bağımsız **altı karar noktası**
taşıyor; biçim karar-listesi.

## Karar 1: `Session::frame()` sınırı nasıl genişler?

Bugün sink yalnız arka plan veriyor (`CellBg { col, row, rgba }`). Glyph için
karakter, ön plan rengi ve biçim bayrakları da geçmeli. Kapsül sözleşmesi
(`pub` API'de alacritty tipi yok) korunmak zorunda.

**(a) Tek zengin tip, tek sink.** `CellBg` → `Cell { col, row, ch, fg, bg,
flags }`; sink her hücre için bir kez çağrılır, `bg == varsayılan && ch == ' '`
olan hücreler atlanır.

- Artı: tek geçiş, tek kilit tutuşu, tek `debug_assert` yüzeyi. Renderer
  hücreyi bir bütün olarak görür ve arka planı ile glyph'ini aynı yerde ayırır.
- Eksi: bugün yalnız arka planı olan hücreler için de karakter alanı taşınır.
  `hucre=K` jetonunun anlamı değişir (arka plan sayacıydı) — duman
  sözleşmesinin yeniden yazılması gerekir.

**(b) İki sink, tek geçiş.** `frame(bg: impl FnMut(CellBg), glyph: impl
FnMut(CellGlyph)) -> Option<Cursor>`; aynı döngüde ikisi de çağrılır.

- Artı: `hucre=K` jetonu aynı kalır, `CellBg` hiç değişmez, geri uyumlu.
  Renderer iki listeyi ayrı doldurur ve iki pipeline'a ayrı besler.
- Eksi: iki kapanış, iki jenerik parametre; imza gürültülü ve üçüncü bir
  katman geldiğinde (alt çizgi kuralları, komut bloğu şeridi) üçüncü sink
  gelir. `mt-gpu`'nun `cell_rule` pipeline'ı tam da o üçüncü katman.

**(c) Snapshot tipi.** `frame()` bir `&Frame`-benzeri yapıyı doldurur;
`bt-core` bir çizim listesi verir, sink hiç yok.

- Artı: imza sabitlenir, katman eklemek tipin içinde kalır.
- Eksi: `bt-core` GPU'nun kare düzenini bilmeye başlar — `plan.md`'nin
  002'de bilerek kurduğu sınır tam olarak buydu (`Frame` `pub(crate)` ve
  `bt-shell` onu görmüyor). Katman yönünü tersine çeviren tek seçenek bu.

## Karar 2: Gerçek font metriği katmanları nasıl aşar?

`CELL_PX` `bt-shell`'de bir sabit; gerçek değeri `bt-atlas` hesaplayacak
(`CTFont` advance + ascent/descent). Ama **`bt-shell` `bt-atlas`'ı görmez** —
bağımlılık `bt-shell → bt-gpu → bt-atlas`.

**(a) `bt-gpu` metriği yeniden yayınlar.** `Renderer::cell_metrics() ->
CellMetrics { width_px, height_px, baseline }`; `bt-shell` grid ölçüsünü ondan
türetir.

- Artı: katman yönü korunur, `bt-shell` tek bir yerden okur ve ölçek
  değişiminde (`windowDidChangeBackingProperties:`) yeniden sorar.
- Eksi: `bt-gpu` metrik için bir geçiş yolu (pass-through) açmış olur.

**(b) `bt-shell` `bt-atlas`'ı doğrudan görsün.** Katman tablosuna yeni bir
kenar.

- Artı: en kısa yol.
- Eksi: `CLAUDE.md`'nin katman tablosu değişir ve "hiçbir bağımlılık yukarı
  gitmez" kuralının yanına ikinci bir yatay kenar eklenir. Ölçüyü kim
  sahiplenir sorusu bulanıklaşır: font `bt-atlas`'ta, ama hücre boyutu
  renderer'ın da bildiği bir şey.

**(c) Metrik `bt-core`'a taşınsın.** Grid ölçüsünün zaten sahibi orası
(`Session::resize(cols, rows, cell_px)`).

- Artı: `bt-shell` zaten `bt-core`'u görüyor.
- Eksi: `bt-core` **platformsuz**; `CTFont` oraya giremez. Metrik yalnız
  taşınabilir ama üretilemez — çözmediği bir sorun.

## Karar 3: Rasterizasyon ve kenar yumuşatma

**(a) Grayscale AA, tek kanal (R8Unorm atlas).** `CGBitmapContext` gri
tonlamalı, glyph başına tek alfa kanalı; shader alfa ile ön planı karıştırır.

- Artı: atlas dokusu dörtte bir yer kaplar, shader tek satır, blend basit
  (`src_alpha`/`one_minus_src_alpha`). macOS 10.14'ten beri sistemin kendisi
  de subpixel AA'yı bıraktı; Retina'da fark gözle zor ayrılır.
- Eksi: Retina olmayan harici ekranda metin subpixel'e göre daha yumuşak
  görünür.

**(b) Subpixel (LCD) AA, üç kanal.** Glyph başına RGB alfa; doğru karıştırma
**dual-source blending** ya da iki geçiş ister.

- Artı: düşük DPI ekranda daha keskin.
- Eksi: blend karmaşıklığı, atlas üç kat yer, renkli arka planda renk
  saçaklanması, ve macOS'un kendi bıraktığı bir yol.

**(c) Renkli glyph (emoji) ayrı yol.** `CTFontDrawGlyphs` emoji için BGRA
verir; tek kanallı atlasla aynı dokuya sığmaz.

- Bu bir seçenek değil, (a)/(b)'nin üstüne gelen bir **kapsam sorusu**:
  emoji bu sette mi, sonraki sette mi? Bugün `bt-core` hücresi 24 bayt ve
  grapheme kümeleri `CellExtra` yan tablosunda — emoji zaten ayrı bir yol
  isteyecek.

## Karar 4: sRGB ve gamma

002 bu kararı bilerek erteledi. Bugün yüzey `BGRA8Unorm` ve **hiç blend yok**;
glyph gelince alfa karıştırma başlıyor ve gamma görünür hale geliyor.

**(a) `BGRA8Unorm_sRGB` yüzey + lineer blend.** Metal donanımda sRGB→lineer
çevirir, blend lineer uzayda koşar, yazarken geri çevirir.

- Artı: fiziksel olarak doğru karıştırma; ince metin ne şişer ne incelir.
  Tema renkleri sRGB olarak yazıldığı için sabitler değişmez.
- Eksi: `cell_bg`'nin bugünkü çıktısı da dönüşümden geçer — 002'nin renkleri
  bir tık değişir ve `cell_bg_pikseli_gpu_tarafinda_boyar` offscreen sınaması
  beklenen baytı günceller.

**(b) `BGRA8Unorm` kalsın, blend sRGB uzayında.** Bugünkü hâl.

- Artı: hiçbir şey değişmez, sınama aynı kalır.
- Eksi: alfa karıştırma yanlış uzayda; koyu zeminde açık metin şişer. Terminal
  ekranının tamamı bu blend'den geçtiği için etkisi her karakterde.

**(c) Ara yol: gamma'yı shader'da elle.** Yüzey `BGRA8Unorm`, dönüşüm shader'da.

- Artı: geçişi kademelendirir.
- Eksi: donanımın bedava yaptığı işi elle yapmak; her yeni pipeline'ın
  hatırlaması gereken bir kural doğar.

## Karar 5: Atlas biçimi ve tahliye

**(a) Tek sabit doku + raf (shelf) paketleyici, tahliye yok.** Glyph'ler
geldikçe raflara dizilir; doku dolarsa yeni glyph çizilmez (ya da tek seferlik
büyütülür).

- Artı: en basit; terminal fontu tek boyutlu ve karakter kümesi pratikte
  sınırlı — bir oturumda kaç ayrı glyph görülür?
- Eksi: CJK ya da yoğun Unicode kullanımında doku dolabilir ve **belirtisi
  sessizdir** (glyph kaybolur). Doluluk bir jetona bağlanmalı.

**(b) Raf paketleyici + LRU tahliye.** Dolunca en eski glyph atılır.

- Artı: sınırsız karakter kümesi.
- Eksi: tahliye edilen glyph'in aynı karede kullanılıyor olması bir sınıf hata
  doğurur (kare içi kilitleme gerekir).

**(c) Boyut sınıfına göre birden çok doku.** Metalterm'in `packer`'ı ayrı bir
modül olduğuna göre buna yakın bir şey yapıyor olabilir; envanterimiz iç
yapısını söylemiyor.

Ölçüm bekleyen soru: bir oturumda kaç ayrı glyph görülüyor ve doku ne kadar
doluyor. **Sayı uydurulmayacak** — atlas doluluğu bir jetona bağlanıp
`/measure` ile okunur.

## Karar 6: Font seti ve fallback zinciri

**(a) Yalnız sistem fontları.** SF Mono → Menlo → Apple Color Emoji.

- Artı: paketlenecek dosya yok, lisans sorusu yok, bundle küçük.
- Eksi: SF Mono'nun konumu sürümlere göre oynadı (bir dönem yalnız Terminal.app
  içindeydi); Menlo her zaman var ama görünüşü tarihli.

**(b) Paketli JetBrains Mono + sistem fallback.** Referansın varsayılanı.

- Artı: her makinede aynı görünüm; ürünün kimliği fontla başlıyor.
- Eksi: bundle'a font dosyası girer (OFL, attribution borcu — bundle setiyle
  birlikte), ve font yükleme yolu (`CTFontManagerRegisterFontsForURL`) bir
  `.app` paketi ister; bugün `.app` yok.

**(c) Ayarla seçilebilir, varsayılan sistem.** `family` ayar anahtarı
(referansın envanterinde var) — ama ayar dosyası bu sette yok.

**Kapsam sorusu:** kutu çizim karakterleri (`─│┌┐└┘├┤┬┴┼` ve blok elemanları).
Font'tan alınırsa komşu hücreler arasında **bir piksellik boşluk** kalır ve
TUI çerçeveleri kırık görünür; referansın `boxdraw` diye ayrı bir modülü
olması bunları **elle çizdiğini** söylüyor. Bu sette mi, sonrasında mı?

## Muhakeme (10 Eylül 2026)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | TEMİZ (koşullu: emoji ve kutu çizim bu sette değilse) |

**Kabul edilen itirazlar → plan değişikliği:**

- **Karar 1(b) derlenmez.** İki sink, `link.rs`'teki tek çağrı yerinde `Frame`'i
  iki kez `&mut` yakalar (E0499). Kaçış yolları (`RefCell` ile hücre başına
  `borrow_mut`, ya da iki geçici `Vec`) `Frame`'in "kare başına yeniden ayırma
  yok" sözünü ve `push_bg`'nin sıralama değişmezini bozuyor. **İki jüri
  bağımsız buldu.** → (b) elendi.
- **(a)'ya yazılan eksi yanlıştı.** "`hucre=K` jetonunun anlamı değişir" doğru
  değil: sayacın sahibi sink değil `Frame::push_bg`. `bg: Option<[f32; 4]>`
  olursa `bg_count` yalnız `Some`'ları sayar ve `hucre=8` bit bit aynı kalır;
  `sabit_shell_arka_plan_hucreleri_verir` oynamaz. → (a) seçildi, `bg` `Option`.
- **`flags` şimdi geçmesin.** `INVERSE` ve `DIM` zaten `bt-core` içinde renge
  çözülüyor; sınırı geçmesi gereken `BOLD`/`ITALIC` (ikinci font yüzü) ve
  `UNDERLINE`/`STRIKEOUT` (kural çizgisi) ve ikisi de bu sette yok. Tek çağrı
  yeri olan bir struct'ı sonradan genişletmek üç satır.
- **Karar 4(a)'nın gerekçesi yanlıştı ve bekçisi kör.** "Tema sabitleri
  değişmez" tutmuyor: sRGB hedefte fragment çıktısı **lineer** sayılır, oysa
  `color::rgba()` sRGB-kodlu float veriyor (`c / 255.0`) ve aynı floatlar
  `MTLClearColor`'a da gidiyor. Geçişte `0x1a1c21` ≈ `0x59` griye açılır —
  **paletin tamamı**. Dahası `cell_bg_pikseli_gpu_tarafinda_boyar` bunu
  göremez: girdileri yalnız `0.0` ve `1.0`, ikisi de sRGB transfer
  fonksiyonunun sabit noktaları. Sınama yeşil kalırken pencere açılır. **Üç
  jüri de buldu.**
- **Karar 4 tek pipeline ile ortadan kalkıyor** — panelin en güçlü önerisi:
  hücrenin arka planı ile glyph'i aynı dikdörtgen, tek instance ikisini birden
  taşırsa framebuffer blend hiç doğmaz ve gamma sorusu tek `mix` satırına iner.
  **→ `## Karar`'da DÜŞTÜ** (animasyon kriteri; gerekçe orada). Panel bu
  kriteri bilmiyordu; kayıt olduğu gibi duruyor çünkü öneri kendi çerçevesinde
  doğruydu ve tek pipeline'a dönülürse gerekçesi hazır.
- **Blok imleç glyph'i örter.** `Frame` imleci bilerek en sona koyuyor
  ("arka planların üstüne gelsin diye"); glyph gelince imlecin altındaki
  karakter opak blokla örtülür ve 003'ün çözdüğü şikâyetin küçük bir kopyası
  geri gelir. Tartışmada hiç geçmiyordu. Tek pipeline'da çözümü daha temiz:
  imleç hücresi bir instance ve `fg`/`bg` takas edilerek çizilir.
- **`Flags::WIDE_CHAR_SPACER` ertelenemez.** `session.rs` bugün yalnız
  `INVERSE`/`DIM` okuyor; spacer hücreleri elenmezse CJK satırlarında ikinci
  hücreye hayalet glyph çizilir. Emoji 004'e gidebilir, bu gidemez.
- **Atlas önbellek anahtarı backing scale taşımalı** — ilk commit'ten
  itibaren. `windowDidChangeBackingProperties:` yolu `cell_px`'i yeniden
  hesaplıyor ama `bt-atlas`'a haber vermeyecek; @1x'te rasterize edilmiş glyph
  @2x'te yanlış boyutta olur ve **hatasız** bulanıklaşır. Sonradan takmak bir
  cache-invalidation avı, belirtisi yalnız iki ekranlı makinede.
- **`glif=` jetonu süs değil, kapının kendisi.** Boş bir atlas ve hiç çizmeyen
  bir glyph yoluyla `kare=1 hucre=8 pipeline=ok` yine basılır ve `make duman`
  yeşil geçer. Jeton eklenmeli, kapı `g > 0` sormalı ve G `hucre=8`in
  bağlandığı gibi bir sınamaya bağlanmalı (`" bateri "` → 6 boşluksuz glyph).
- **CoreText font bulunamazsa hata vermez, ikame eder.** Ad karşılaştırılıp tek
  satır uyarı basılmazsa yanlış font sessizdir.
- **`hedef_doku` piksel formatını elle yazıyor.** `Renderer`'dan okumalı;
  bugün de sözleşmeyi bozan tek yer o test yardımcısı.
- **Karar 6'nın iki şıkkı bu sette uygulanamaz.** Paketli font
  `CTFontManagerRegisterFontsForURL` ile bir `.app` içinden yüklenir ve
  `make kur` bugün "henüz yok" diyor; `family` ayarı da yok. Soru "hangisi"
  değil "ileride hangisi" — karar listesinden çıkarıldı, `bt-atlas`'a tek
  satır varsayılan olarak girdi.
- **Atlas: raf paketleyici yerine sabit yuva ızgarası.** Emoji ve kutu çizim
  ertelendiğinde tüm glyph'ler hücre boyutunda; `yuva_no → uv` aritmetiktir.
  Raf paketleyici değişken boyut için vardır ve referansın `packer`'ı da tam
  olarak ertelediğimiz iki şeyle aynı crate'te. Taşma sessiz kalmasın: doluluk
  bir sayaca bağlanır ve dolu atlasta rezident bir "tofu" kutusu çizilir.
- **`bt-atlas` `MTLTexture` üretemez** (kendi `lib.rs` yorumu "Metal görmez"
  diyor): CPU bitmap + yerleşim dikdörtgeni verir, dokunun sahibi `bt-gpu`.
  Karar kaydına giren bir bölüşüm.
- **Bağımlılık kararı listede yoktu.** Set `bt-atlas`'a en az iki crate
  ekliyor ve seçenek tek değil: `core-text`/`core-graphics` (servo ailesi,
  `CLAUDE.md`'nin katman tablosunda adıyla yazılı) ikinci bir ObjC/CF
  sarmalayıcı yığını demek; `objc2-core-text`/`objc2-core-graphics` workspace'in
  zaten pinlediği aileyle aynı. → yeni karar noktası, kullanıcıya sorulur.

**Reddedilenler:**

- **sRGB'yi yine de ayrı bir phase olarak yap** (İşletme) — tek pipeline
  seçilirse blend hiç doğmuyor, yani düzeltilecek bir yanlışlık yok. Gamma
  tek `mix`'in uzayı olarak kalır ve istenirse tek satırda değişir.
- **`Renderer::cell_metrics()` yerine saf `bt_gpu::cell_metrics(scale)`**
  (Sadelik) — kısmen: font yükleme durum ister (`CTFont` nesnesi) ve önbellek
  anahtarı ölçeği taşımak zorunda. Metrik `Renderer`'dan sorulur ama içeride
  saf bir hesaba dayanır; imza ayrıntısı plan'da.
- **LRU tahliye** (discussion 5b) — kare-içi kilitleme sınıfı bir hata
  doğuruyor ve doluluk ölçülmeden ödenecek bir bedel değil. `/measure` sonrası
  yeniden açılır.

## Karar Noktaları

Kullanıcıya sorulacaklar, sırayla:

1. `frame()` sınırı: tek zengin tip mi, iki sink mi? (`hucre=` jetonunun
   akıbetini de belirler)
2. Font: paketli JetBrains Mono mu, sistem fontu mu? (`.app` paketi henüz yok)
3. Emoji ve kutu çizim bu sette mi, ertelenir mi?
4. sRGB geçişi şimdi mi? (002'nin offscreen sınaması güncellenir)

## Karar (10 Eylül 2026, kullanıcı onayı)

Kullanıcı iki soruyu da tek ölçüte bağladı: **"animasyon kurgulamaya en
elverişli olan hangisi ise ona göre seç."** Bu ölçüt panelin elinde yoktu ve
en güçlü öneriyi tersine çevirdi.

- **Seçilen: iki pipeline** (`cell_bg` arka planlar + imleç, `cell` glyph'ler),
  `BGRA8Unorm_sRGB` yüzey ve lineer blend.

  Gerekçe, referansın envanterinde yazılı üç animasyondan okunuyor
  (`docs/ARASTIRMA.md` → Hareket): `cursor_motion` **alt hücre
  interpolasyonlu** imleç kayması, `delete_mode: shatter` silinen glyph'in
  parçalanması, `keypress` glyph'in tek tek belirmesi. İlk ikisi glyph'in
  arka planından **bağımsız** hareket etmesini şart koşuyor:

  - Kayan imleç iki hücrenin arasındayken **iki farklı harfi** kısmen örter.
    Tek quad hem arka planı hem tek bir glyph'i taşıdığı için bunu çizemez;
    imlecin altındaki harf ya kaybolur ya imleçle birlikte zıplar.
  - `shatter` glyph'i döndürüp dağıtır; tek quad'da glyph'i döndürmek arka
    planı da döndürür ve hücre ızgarası bozulur.

  Üçüncüsü (`keypress`) tek pipeline'da da çalışırdı — ama üçünden biri.
  Referansın kendi ayrımı da bu okumayı destekliyor: `cell_bg` ve `cell` ayrı
  pipeline'lar, ve `decay` (silme efekti) **üçüncü** bir glyph-benzeri
  pipeline olarak duruyor. Bu projenin motivasyonu zaten Metalterm'in imleç
  animasyonlarıydı; onları imkânsızlaştıran bir sadeleştirme, sadeleştirme
  değil kapsam kesmedir.

- **Reddedilen: tek pipeline** (`Instance { pos, size, fg, bg, uv }`,
  `mix(bg, fg, atlas.r)`) — panelin sadelik merceğinin önerisi ve kendi
  çerçevesinde doğru: blend hiç doğmaz, sRGB kararı düşer, 002'nin offscreen
  sınaması oynamaz, imleç sorunu `fg`/`bg` takasıyla kapanır. Yalnız
  animasyonu kapatıyor. **Bedeli kabul ediliyor:** framebuffer blend, sRGB
  geçişi ve paletin lineerleştirilmesi bu setin işi.

- **Seçilen: `objc2-core-text` + `objc2-core-graphics`** (0.3 serisi).

  Animasyon açısından iki aile arasında fark **yok** — CoreText yalnız atlası
  dolduruyor, animasyon GPU tarafında yaşıyor. Ölçüt bu yüzden ikinciye kaydı:
  workspace zaten `objc2` 0.6 ailesini pinliyor (`objc2-metal`,
  `objc2-app-kit`, `objc2-quartz-core` hepsi 0.3), ve animasyonun zamanlama
  ucu (`CAMetalDisplayLink`, ileride `CAMediaTimingFunction`) o ailede. Tek
  FFI yığını, tek sürüm politikası, `Retained`/`MainThreadMarker` örüntüsü
  atlas kodunda da aynı kalır.

- **Reddedilen: `core-text 22` + `core-graphics`** (servo ailesi) — olgun ve
  referansın kullandığı yol, ama `core-foundation-rs` ikinci bir ObjC/CF
  sarmalayıcı yığını olarak `Cargo.lock`'a girer ve iki aile yan yana yaşar.
  `CLAUDE.md`'nin katman tablosundaki `core-text`/`core-graphics` adları bu
  kararla aynı commit'te güncellenir.

### Karar 4 yeniden açıldı — ve panelin uyarıları geçerli

İki pipeline seçildiği için sRGB kararı bu sete geri döndü. Panelin
bulduğu üç tuzak plana **koşul** olarak girer:

1. `color::rgba()` sRGB-kodlu float veriyor ve `const fn`; sRGB hedefte bu
   değer lineer sayılır ve palet açılır (`0x1a1c21` ≈ `0x59`).
   Lineerleştirme `powf` ister, `powf` stable'da `const` değil → 256 girdilik
   `const` tablo ya da dönüşümün `bt-core` dışına taşınması.
2. `cell_bg_pikseli_gpu_tarafinda_boyar` bu geçişi **göremez** (girdileri
   yalnız `0.0`/`1.0`, sRGB'nin sabit noktaları). Aynı commit'te sınamaya
   **ara ton** bir renk girer.
3. `hedef_doku` piksel formatını elle yazıyor; `Renderer`'dan okumalı, yoksa
   pipeline sRGB'ye geçince sınama assert'le değil doğrulama istisnasıyla
   düşer.
