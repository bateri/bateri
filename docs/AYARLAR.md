# Ayarlar

bateri'nin kullanıcı ayarları tek bir TOML dosyasında, renk temaları ayrı
dosyalarda durur. Bu belge anahtarların, tema biçiminin, varsayılanların ve
dosya bozukken ne olacağının **tek sahibidir**; kod tarafındaki karşılığı
`crates/bt-core/src/settings.rs` ve `theme.rs` (ayrıştırma),
`crates/bt-shell/src/settings.rs` (okuma ve tema adının çözümü),
`watch.rs` (dosyaların izlenmesi), `menu.rs` ve `zoom.rs` (View menüsü:
tema seçimi ve geçici punto), `clipboard.rs` ve `app.rs`'in `ShellWake`'i
(OSC 52'nin yuvası, ana kuyruğa geçişi ve panoya yazması),
`crates/bt-atlas/src/font.rs` (font ailesinin bulunması),
`crates/bt-gpu/src/motion.rs` ve `link.rs` (imleç hareketinin ve Hareketi
Azalt'ın uygulanması; üç değerli ayarın tek `bool`'a indiği yer
`app.rs`'in `resolve_reduce_motion`'ı), `crates/bt-shell/src/child.rs`
(hangi kabuk koşuyor, sarmalayıcı betiği nerede) ve `app.rs`'in
`shell_integration_env`'i (shell entegrasyonu kurulacak mı ve hangi ortamla);
betiğin kendisi `assets/shell/zsh/`.

## Dosyanın yeri

```
~/.config/bateri/settings.toml
```

Dosya yoksa her şey varsayılanıyla çalışır ve hiçbir uyarı çıkmaz.
**bateri ▸ Settings…** (Cmd ,) dosyayı editörde açar, yoksa önce yaratır
(bkz. [Settings…](#settings)); dizini ve dosyayı elle oluşturmak da yeterli.
Dosya başka bir yere sembolik bağ olabilir (dotfile deposu); bağın hedefi
okunur.

Uygulama bu dosyaya iki yerden yazar: **Settings…** dosya yokken şablonu
yaratır, **View ▸ Theme ▸** ile tema seçince `[appearance] theme` satırını
yazar (bkz. [View ▸ Theme ▸](#view--theme-)). Var olan dosyanın başka hiçbir
satırına dokunulmaz.

Değişiklik **kaydettiğiniz anda** geçerli olur — ayar dosyasında da,
kullanılan temanın dosyasında da; kabuk ve içindeki program yaşamaya devam
eder. Tek istisna [`[shell] integration`](#shell): kabuk çoktan doğduğu için
o anahtar sonraki oturumda geçerlidir. Editörün kaydı nasıl yaptığı fark etmez (yerinde yazma, boşaltma,
geçici dosya ve üstüne taşıma, sembolik bağın hedefine yazma). Sistemin açık/koyu
görünümü de anında izlenir: tema `"system"` iken (varsayılan) Sistem
Ayarları'nda görünüm değişince pencere de değişir.

İzlenen yer `~/.config/bateri/` dizinidir. Uygulama açıkken bu dizin **hiç
yoksa** kabuktan oluşturmak izlemeyi başlatmaz: değişiklikler uygulamayı
yeniden açınca ya da bir kez Settings… seçilince görülür, sonrası kayıt
anında izlenir. Settings…'in yarattığı dizin hemen izlenir.

### Settings…

Kısayol ABD düzenli klavyede Cmd `,`. macOS menü kısayolunu klavye düzenine
göre yerleştirir; Türkçe Q klavyede aynı tuş **Cmd `ö`**, menüde de öyle
görünür. View ▸ Bigger da bu düzende `⌘:` görünür.

bateri ▸ Settings… (Cmd ,) ayar dosyasını açar: önce `.toml` dosyalarını
açan uygulamayla, o yoksa varsayılan metin editörüyle (çoğu makinede
TextEdit).

Dosya yoksa dizini ve dosyayı aşağıdaki şablonla yaratır. Şablon hiçbir şeyi
değiştirmez: varsayılanı olan her anahtar varsayılan değeriyle yazılıdır,
değeri yerinde değiştirip kaydetmek yeter.

- Var olan dosyaya **dokunmaz** — bozuk olsa da, başka yere sembolik bağ olsa
  da, bağın hedefi olmasa da.
- Dosya yaratılamazsa (izin yok, `~/.config/bateri` bir dosya) ya da hiçbir
  uygulama açamazsa başlık çubuğunda söylenir; ikincisinde dosyanın yolu da
  yazılır.
- Şablondaki değerler yaratıldığı günün varsayılanlarıdır: sonraki bir
  sürümde bir varsayılan değişirse bu dosya eski değeri tutar. Satırı silmek
  anahtarı güncel varsayılana döndürür.

### Şablon

```toml
# bateri settings. Changes apply as soon as you save this file.
# A key you delete goes back to its default.

[terminal]
# Lines of history kept above the screen, from 0 to 100000.
scrollback = 10000

[appearance]
# "system" follows the macOS light/dark appearance. Any other value is a theme
# used in both: a file themes/NAME.toml next to this one, or a built-in theme,
# "bateri" (dark) or "bateri-light" (light).
theme = "system"
# The themes used while theme = "system".
light_theme = "bateri-light"
dark_theme = "bateri"

[font]
# A family name as shown in Font Book. Without it bateri uses SF Mono, or
# Menlo when SF Mono is not installed.
# family = "Menlo"
# Size in points.
size = 13

[clipboard]
# Lets programs in the terminal, also over ssh, copy text to the clipboard
# (OSC 52): "copy" allows it, "off" does not. They can never read it.
osc52 = "copy"

[motion]
# How the cursor travels between cells: "spring" glides and eases into place,
# "ease" glides for a fixed time, "snap" jumps there at once.
cursor_motion = "spring"
# Whether to tone animations down to a short fade: "system" follows the macOS
# Reduce Motion setting, "on" and "off" decide it here.
reduce_motion = "system"

[shell]
# Whether bateri sets up the shell so it can report where prompts and commands
# begin and end: "auto" does it for shells bateri knows, "off" never does.
# Unlike every other key here, this one only takes effect in shells started
# after the change; shells already open keep what they were started with.
integration = "auto"
```

Blok bir sınamayla şablona bağlıdır (`documented_template_is_the_template`).

### View ▸ Theme ▸

Menü her açılışta yeniden kurulur:

- **Match System** — `theme = "system"`: tema macOS'un görünümünü izler
  (`light_theme` / `dark_theme`).
- Gömülü temalar: `bateri`, `bateri-light`.
- `~/.config/bateri/themes/` altındaki her `{ad}.toml`, adıyla. Dizine yeni
  dosya koymak menüyü bir sonraki açılışta günceller. Nokta ile başlayan
  dosyalar ve `system.toml` listelenmez; gömülü bir temayı gölgeleyen dosya
  (`themes/bateri.toml`) ayrıca listelenmez, gömülü adın öğesi onu seçer.

İşaretli öğe ayar dosyasındaki `theme` değeridir.

Bir öğe seçmek temayı **ayar dosyasına yazar** ve dosya kaydedilmiş gibi hemen
uygulanır; uygulama yeniden açılınca da aynı tema gelir. Yazılan tek şey
`[appearance] theme` satırının değeridir:

- Yorumlar, boş satırlar, anahtarların sırası, tanınmayan anahtarlar ve
  satırın yanındaki yorum yerinde kalır.
- `light_theme` ve `dark_theme` değişmez: sabit bir tema seçip sonra
  Match System'e dönmek açık/koyu çiftini geri getirir.
- `[appearance]` bölümü yoksa dosyanın sonuna eklenir; `theme` yoksa bölümün
  içine. Satır içi (`appearance = { … }`) ya da noktalı
  (`appearance.theme = …`) yazılış korunur.
- Dosya yoksa önce [şablonla](#şablon) yaratılır, sonra satır yazılır.
- Dosya sembolik bağsa **hedefi** yazılır, bağ bağ olarak kalır.
- Satır sonları korunur: ilk satırı Windows satır sonuyla (CRLF) biten dosya
  CRLF kalır. Son satırın sonunda satır sonu yoksa eklenir.

Dosyaya **yazılmayan** durumlar — dosyanın içeriği olduğu gibi kalır ve başlık
çubuğu sebebini söyler (`…; the theme was not saved`):

- dosya geçersiz TOML ya da okunamıyor (izin, hedefi olmayan sembolik bağ);
- `appearance` bir bölüm değil (`appearance = 1`, `[[appearance]]`) ya da
  `theme` bir bölüm (`[appearance.theme]`): üstüne yazmak içeriğini silerdi.

Uyarı bir sonraki başarılı seçimde ya da dosya okunabilir ve geçerli
kaydedilince kalkar.

## Hata olursa

Hata pencerenin başlık çubuğunda, başlığın yanında İngilizce görünür:

```
bateri – settings.toml: line 2: `terminal.scrollback` must be an integer, found a string; using 10000
```

Birden çok hata varsa ilki ve kalanların sayısı yazılır (`(+2 more)`);
hepsi ayrıca standart hata çıkışına `bateri:` önekiyle basılır. Terminal her
durumda açılır.

**Tam ekranda** başlık çubuğu gizlenir ve uyarı onunla birlikte görünmez
olabilir (denenmedi); dosyayı pencere modunda açıp bakmak ya da stderr'deki
kopya yolu kalır.

| açılışta | sonuç |
|---|---|
| dosya yok | varsayılanlar, uyarı yok |
| dosya okunamıyor (izin, UTF-8 olmayan içerik, düz dosya değil, hedefi olmayan sembolik bağ) | varsayılanlar, yalnız `osc52` **kapalı**; uyarı |
| geçersiz TOML | **bütün** ayarlar varsayılan, yalnız `osc52` **kapalı**; uyarı satırı gösterir |
| bir anahtarın değeri kabul edilmiyor | yalnız o anahtar varsayılan (ya da sınırı; `osc52` için kapalı), uyarı |
| tanınmayan anahtar ya da bölüm | sessizce yoksayılır |
| seçilen tema bulunamıyor | görünüme uyan gömülü tema (koyuda `bateri`, açıkta `bateri-light`), uyarı |
| tema dosyası okunamıyor, boş ya da geçersiz TOML | görünüme uyan gömülü tema, uyarı (aynı adlı gömülü tema **kullanılmaz**) |
| tema dosyasında bir renk kabul edilmiyor | yalnız o renk `bateri`'ninki, uyarı |
| font ailesi bulunamıyor | varsayılan font (SF Mono, yoksa Menlo), uyarı |
| font ailesi eşaralıklı değil | aile yine kullanılır, uyarı |

Tema kuralı görünüm değişiminde de aynı: yeni görünümün teması
kullanılamıyorsa o görünüme uyan gömülü tema gelir — pencere öteki
görünümün temasında kalmaz.

**Kaydettiğiniz anda** ise kural düzenlemeyi korur: yarım kalmış bir kayıt
ekranı bozmaz, uyarı çıkar ve dosyayı düzeltip kaydedince uyarı kalkar.

| kayıt anında | sonuç |
|---|---|
| ayar dosyası geçersiz TOML ya da okunamıyor | **hiçbir ayar değişmez**, uyarı |
| ayar dosyası silindi ya da boşaltıldı | ayarlar değişmez, uyarı yok; varsayılanlar uygulamayı yeniden açınca gelir |
| bir anahtarın değeri kabul edilmiyor | o anahtar **değişmez**, uyarı; tavanı aşan `scrollback` tavana iner, kabul edilmeyen `osc52` **kapanır** |
| anahtar dosyadan silindi | o anahtar varsayılanına döner |
| seçilen tema bulunamıyor, dosyası okunamıyor, boş ya da geçersiz TOML | **ekrandaki tema kalır**, uyarı |
| font ailesi bulunamıyor | varsayılan font, uyarı; adı düzeltip kaydedince uyarı kalkar |

Menüden tema seçerken dosya geçersiz ya da okunamıyorsa dosyaya **yazılmaz**;
bkz. [View ▸ Theme ▸](#view--theme-).

Silinen dosyanın ayarları değiştirmemesi bilerek: çoğu editör kaydederken
eski dosyayı bir an kenara taşır ya da önce boşaltıp sonra yazar; varsayılanlara
dönmek her kayıtta pencereyi çakar, `scrollback` büyütülmüşse geçmişin fazlasını
silerdi. Kabul edilmeyen değerin anahtarı değiştirmemesi de: `scrollback`'i
yanlışlıkla metin olarak kaydetmek varsayılana düşseydi geçmişin fazlası o
anda silinirdi.

"Geçersiz TOML" sözdizimi hatasından geniştir: aynı anahtarı iki kez yazmak
ve TOML'un tam sayı sınırını (9 223 372 036 854 775 807) aşan bir sayı da
dosyanın tamamını geçersiz yapar.

Tanınmayan anahtarın sessiz kalması bilerek: sonraki sürümlerin anahtarını
bugünkü sürüm hata diye göstermemeli.

`osc52`'nin kuralın dışında kalması da bilerek: dosya okunamayınca ya da
değeri yanlış yazılınca (`"of"`) kullanıcının onu kapatıp kapatmadığı
bilinemez, ve yanlış tahmin öteki anahtarlarda ekranda görünürken burada
görünmez — uzaktaki bir program panoya sessizce yazabilirdi. Kapalıya düşmek
geri alınabilir: dosyayı düzeltip kaydetmek yeter.

## Anahtarlar

### `[terminal]`

```toml
[terminal]
scrollback = 10000
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `scrollback` | tam sayı, `0`–`100000` | `10000` | geçmişte tutulan satır sayısı |

- `100000`'den büyük değer **`100000`** olur ve uyarı verir. Sınır
  alacritty'nin kendi ayar sınırı (`MAX_SCROLLBACK_LINES`); ölçülmüş bir
  bellek bütçesi değil.
- Negatif ya da tam sayı olmayan değer (`"lots"`, `1.5`) varsayılana döner
  ve uyarı verir.
- `0` geçerli: geçmiş tutulmaz.
- Değer uygulama açıkken değişince **hemen** uygulanır: küçültmek fazla
  satırları o anda siler, sonra büyütmek silineni geri getirmez. Yazarken
  kendiliğinden kaydeden bir editörde ara değer de (`100000` → `1`) kayıttır.

Bölüm satır içi de yazılabilir: `terminal = { scrollback = 5000 }`.
`[[terminal]]` (bölüm dizisi) bölüm sayılmaz ve uyarı verir.

### `[appearance]`

```toml
[appearance]
theme = "system"
light_theme = "bateri-light"
dark_theme = "bateri"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `theme` | `"system"` ya da tema adı | `"system"` | kullanılacak renk teması |
| `light_theme` | tema adı | `"bateri-light"` | `theme = "system"` iken açık görünümün teması |
| `dark_theme` | tema adı | `"bateri"` | `theme = "system"` iken koyu görünümün teması |

- `theme = "system"` temayı macOS'un görünümüne bırakır: açıkta
  `light_theme`, koyuda `dark_theme`. Görünüm değişince tema anında değişir.
- `theme = "{ad}"` görünümden bağımsız sabit bir temadır; `light_theme` ve
  `dark_theme` o sırada okunmaz ama yerinde kalır — `"system"`'e dönünce
  çift geri gelir.
- `"system"` bir tema adı değildir: `themes/system.toml` seçilemez, ve
  `light_theme`/`dark_theme` bu değeri kabul etmez (kendi varsayılanına
  döner, uyarı verir).
- Ad önce `~/.config/bateri/themes/{ad}.toml` olarak aranır, yoksa gömülü
  temalar arasında. Gömülü iki tema var: `bateri` (koyu) ve `bateri-light`
  (açık).
- Aynı adlı bir dosya gömülü temayı **gölgeler**: `themes/bateri.toml`
  yazan kullanıcı gömülü `bateri`'yi değil kendi dosyasını görür.
- Hiçbir yerde bulunamayan ad görünüme uyan gömülü temaya döner (koyuda
  `bateri`, açıkta `bateri-light`) ve uyarı verir; açılışta da görünüm
  değişince de.
- Boş ad ya da `/` içeren ad (`"../x"`) kabul edilmez, anahtarın
  varsayılanına döner ve uyarı verir: tema `themes/` dizininin dışından
  okunmaz.

### `[font]`

```toml
[font]
family = "Menlo"
size = 13
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `family` | metin | yok (SF Mono, yoksa Menlo) | yazı ailesi |
| `size` | sayı, `0`'dan büyük | `13` | punto |

- `family` bir **aile adıdır**, Font Kitabı'nda görünen ad (`"JetBrains
  Mono"`, `"Menlo"`); büyük/küçük harf fark etmez. Tek bir yüzün PostScript
  adı (`"Menlo-Regular"`) aile sayılmaz ve "bulunamadı" uyarısı verir.
- Aile makinede yoksa varsayılan font kullanılır ve başlık çubuğunda
  söylenir: `font "Fira Code" not found; using Menlo`. Adı düzeltip
  kaydedince font değişir, uyarı kalkar.
- Eşaralıklı olmayan bir aile (`"Helvetica"`) **reddedilmez**, uyarı verir:
  `font "Helvetica" is not monospaced; text may not line up`. Hücre
  genişliği boşluk karakterinden gelir; ondan geniş harfler hücreye
  kırpılır.
- `family = ""` ya da anahtarın olmaması varsayılan font demektir, uyarı
  vermez.
- `size` tam sayı da ondalıklı da olabilir (`13`, `13.5`). Sıfır, negatif,
  `nan`, `inf` ya da sayı olmayan değer varsayılana (açılışta `13`, kayıt
  anında o anki punto) döner ve uyarı verir.
- Punto ekranın ölçeğiyle çarpılıp **sessizce** 4–144 aralığına çekilir:
  Retina ekranda (2×) yazılan punto 2–72 arasında etkilidir, normal ekranda
  4–144; dışındaki değer en yakın sınır gibi çizilir. Uyarı yok, çünkü aynı
  değer pencere ekran değiştirdikçe sınırın bir içinde bir dışında
  kalabilirdi.
- Çok büyük puntoda glyph atlası çabuk dolar: dolduktan sonra ekranda ilk
  kez görünen karakterler kutu (□) olarak çizilir. Punto küçültülünce ya da
  uygulama yeniden açılınca geçer.
- Font kaydettiğiniz anda değişir: pencere boyutu aynı kalır, sütun ve satır
  sayısı yeni hücreye göre yeniden hesaplanır ve kabuk ile içindeki program
  (vim, less) yeni boyutu pencere boyutlandırılmış gibi alır; uzun satırlar
  yeniden sarılır.
- Kalın ve eğik yüzü olmayan ailede o metin düz yüzle çizilir; bu uyarı
  vermez (standart hata çıkışına bir satır düşer).

#### Geçici punto: Cmd +, Cmd −, Cmd 0

**View ▸ Bigger** (Cmd +), **Smaller** (Cmd −) ve **Actual Size** (Cmd 0)
puntoyu **geçici** olarak değiştirir: dosyaya yazılmaz, uygulama kapanınca
gider.

- Her basış bir punto; aralık 4–72. Aralığın ucundaki basış hiçbir şey
  yapmaz, yani tuşu basılı tutup geri dönmek hemen görünür. Dosyadaki `size`
  aralığın dışındaysa basış yalnız aralığa doğru çalışır.
- **Actual Size** dosyadaki `size`'a döner.
- Dosyada `size`'ı değiştirip kaydetmek geçici farkı bırakır: yazdığınız
  punto görünür. `family` ya da başka bir anahtarı değiştirmek farkı korur.

### `[clipboard]`

```toml
[clipboard]
osc52 = "copy"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `osc52` | `"copy"` ya da `"off"` | `"copy"` | terminaldeki programın panoya yazıp yazamayacağı |

- **OSC 52**, bir programın terminal üzerinden panoya metin yazma dizisidir.
  En bilinen kullanımı ssh'la bağlanılan makinedeki vim ya da tmux: orada
  kopyalanan metin bu Mac'in panosuna gelir, uzak makinenin panoya erişimi
  olmasa da. `"copy"` buna izin verir, `"off"` diziyi yoksayar.
- Yazılan pano, Cmd-C'nin yazdığı **genel panodur**. Program art arda çok
  sayıda kopya yollarsa yalnız sonuncusu panoda kalır.
- **Okuma yönü yok**, hiçbir değerle açılmaz: terminaldeki bir program
  panonuzdaki metni okuyamaz. Bu yüzden `"paste"` gibi bir değer yoktur.
- **Bedeli:** `"copy"` iken arka planda koşan bir program da (uzaktaki dahil)
  panoya yazabilir ve sizin kopyaladığınızı değiştirebilir. İstemiyorsanız
  `"off"`.
- Dizinin hedefi fark etmez: birincil seçime (`p`, `s`) yazan dizi de genel
  panoya yazar. macOS'ta tek pano var; vim'de `*` ile `+` burada aynı panodur
  ve Neovim `*`'ı `p` diye yollar, yani `clipboard=unnamed` ayarlı bir
  Neovim'in ssh'taki kopyası da gelir. Boş metin panoyu silmez, yoksayılır.
- Kopyanın boyut sınırı yok. Pano yazılırken pencere yeni kare çizmez; çok
  büyük bir kopyada bu fark edilebilir (hangi boyutta olduğu ölçülmedi).
- Tanınmayan değer (`"paste"`, `"Copy"`, `true`) ve `[clipboard]`'ın bölüm
  olmaması **kapalıya** düşer ve uyarı verir — öteki anahtarlar gibi
  varsayılana (açık) değil; açılışta okunamayan ya da geçersiz ayar dosyası
  da (bkz. [Hata olursa](#hata-olursa)).
- Kaydettiğiniz anda geçerli olur; açık programı yeniden başlatmak gerekmez.

### `[motion]`

```toml
[motion]
cursor_motion = "spring"
reduce_motion = "system"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `cursor_motion` | `"snap"`, `"ease"` ya da `"spring"` | `"spring"` | imlecin hücreler arasında nasıl gittiği |
| `reduce_motion` | `"system"`, `"on"` ya da `"off"` | `"system"` | animasyonların kısılıp kısılmayacağı |

- **`"spring"`** — imleç yeni yerine bir yayla kayar ve yavaşlayarak oturur;
  hedefi aşmaz. Uzak bir sıçrama yakın bir sıçramadan biraz uzun sürer.
- **`"ease"`** — kayma **sabit** sürer, mesafe ne olursa olsun; sonuna doğru
  yavaşlar, hedefi aşmaz.
- **`"snap"`** — kayma yok, imleç doğrudan yeni hücrede görünür ve içerik de
  anında yerine gider. Hareketi tamamen kapatmanın yolu bu.
- Aynı stil **içeriğin yükselmesini** de sürer: bateri içeriği pencerenin
  tabanına yaslar, yani yeni bir satır geldiğinde geçmiş yukarı kayar ve imleç
  dipteki satırında durur. Kayan şey bütün ızgaradır, imleç değil.
- **Izgaranın başka sebeple yer değiştirmesi kaymaz:** geçmişte kaydırmak
  (tekerlek), pencereyi boyutlandırmak, fontu ya da puntoyu değiştirmek ve tam
  ekran bir uygulamaya girip çıkmak (vim, less, htop) her şeyi animasyonsuz
  yerine koyar. Ortak sebep: bunların hiçbirinde içerik kendi büyümesiyle
  yükselmedi, ızgara başka bir sebeple yer değiştirdi.
- **`clear` bunun istisnası:** dolu bir ekranı temizlemek prompt'u yukarıdan
  aşağıya kaydırır. Doluluğun bir hamlede daralması ile satır satır daralması
  bugün ayırt edilmiyor ve ayırmanın ölçülmüş bir eşiği yok; o ölçülene kadar
  `clear` kayan tarafta kalıyor.
- Kaydettiğiniz anda geçerli olur. O sırada kayan bir imleç varsa `"snap"`
  onu hedefinde bitirir, öteki iki stil kaymayı bulunduğu yerden devralır:
  imleç hiçbir stil değişiminde ışınlanmaz.
- Tanınmayan değer (`"sprong"`, `"Spring"`, `true`) yalnız bu anahtarı
  etkiler (açılışta `"spring"`, kayıt anında ekrandaki stil) ve uyarı görünür.

`reduce_motion` animasyonların kısılıp kısılmayacağını söyler:

- **`"system"`** — macOS'un Sistem Ayarları ▸ Erişilebilirlik ▸ Görüntü ▸
  Hareketi Azalt ayarını izler. Ayarı açıp kapatmak bateri'yi yeniden
  başlatmadan etkiler.
- **`"on"`** — sistem kapalıyken de kısar, **`"off"`** sistem açıkken de
  kısmaz. İkisi sistemi hiç okumaz.
- Kısıldığında imleç kaymaz: yeni hücresinde **kısa bir belirmeyle** (90 ms)
  görünür, eski hücresinde iz bırakmaz. Kısılan şey kaymanın kendisi, imlecin
  görünürlüğü değil.
- İçeriğin yükselmesi kısıldığında **belirmez, anında yerine gider**: her yeni
  satırda bütün ekranın belirmesi, kısmaya çalıştığı hareketten beter olurdu.
- Belirme **duraksamadan sonraki** harekete aittir: normal yazma hızında her
  harf imleci yeni yerinde kısa bir belirmeyle gösterir. Hareketler
  belirmenin süresinden daha sık geldiğinde — çıktı akarken ya da çok hızlı
  yazarken — imleç tam opak kalır ve yeni yerine sessizce geçer; yoksa
  saniyede on kereden hızlı bir titreme doğardı (eski davranışta imleç akan
  çıktıda büsbütün görünmez oluyordu).
- `cursor_motion = "snap"` bunun **üstündedir**: hareketi zaten kapatmış
  olan kullanıcıya Hareketi Azalt bir belirme *eklemez*.
- Kaydettiğiniz anda geçerli olur. O sırada kayan bir imleç varsa hedefinde
  bitirilir — açarken de kapatırken de imleç ışınlanmaz.
- Tanınmayan değer (`"yes"`, `"System"`, `true`) yalnız bu anahtarı etkiler.

### `[shell]`

```toml
[shell]
integration = "auto"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `integration` | `"auto"` ya da `"off"` | `"auto"` | kabuğa entegrasyon kurulsun mu |

Entegrasyon, kabuğun terminale "prompt burada başladı, komut burada koştu, şu
kodla bitti" demesini sağlar. Bugün yalnız **zsh** için var; başka bir kabukta
(bash, fish) `"auto"` da hiçbir şey yapmaz ve terminal olduğu gibi çalışır.

- **`"auto"`** — kabuk zsh ise `ZDOTDIR` bateri'nin kendi dizinini gösterir.
  O dizindeki dosyalar **sizin** başlangıç dosyalarınızı yükler, `ZDOTDIR`'ı
  özgün değerine geri koyar (yoksa siler) ve kabuğun kendi kancalarına
  işaretleri ekler. Komut geçmişiniz (`HISTFILE`) de kendi dizininde kalır.
- **`"off"`** — hiçbir şey kurulmaz.
- **Dosyalarınıza yazılmaz.** Ne `.zshrc`'ye ne başka bir rc dosyasına tek
  satır eklenir; entegrasyon yalnız bir ortam değişkenidir, yani kapatmak iz
  bırakmaz.
- Başka bir aracın kurduğu **gerçek** OSC 133 işaretleri `"off"` iken de
  okunur: anahtarın anlamı "sarmalayıcıyı kurma", "işaretleri görmezden gel"
  değil.
- SSH ile uzak bir makineye geçtiğinizde orada bizim betiğimiz yoktur ve
  işaretler gelmez. Bu bir arıza değil; terminal olağan hâlinde çalışır.

**Bu anahtar öteki anahtarlar gibi kayıt anında uygulanmaz** — tek istisna
budur. Entegrasyon kabuk **doğarken** kuruluyor, dosyayı kaydettiğinizde kabuk
çoktan doğmuş oluyor: değer **sonraki oturumda** geçerli olur, açık pencere
etkilenmez.

Uygulama açılmıyorsa (bozuk bir kabuk yapılandırması yüzünden pencere hemen
kapanıyorsa) anahtarı **elle** kapatabilirsiniz; bateri'ye hiç ihtiyaç yok.
Başka bir terminalden başlayın.

Entegrasyon varsayılan olarak açık olduğu için ayar dosyanız **hiç
olmayabilir** — Settings…'i bir kez bile açmadıysanız yoktur. Önce o hâli
geçin; dosya yoksa tek komut yeter ve gerisini okumanıza gerek kalmaz:

```sh
mkdir -p ~/.config/bateri
[ -e ~/.config/bateri/settings.toml ] || printf '[shell]\nintegration = "off"\n' \
  > ~/.config/bateri/settings.toml
```

Dosya zaten varsa onu açın:

```sh
open -e ~/.config/bateri/settings.toml
```

`[shell]` bölümü varsa `integration` satırını `"off"` yapın; yoksa dosyanın
sonuna iki satır ekleyin:

```toml
[shell]
integration = "off"
```

Bölümü **iki kez** yazmayın: TOML aynı bölümün tekrarını kabul etmez ve dosya
bütünüyle okunamaz hâle gelir (başlık çubuğu bunu söyler).

- Tanınmayan değer (`"on"`, `"Auto"`, `false`) yalnız bu anahtarı etkiler
  (açılışta `"auto"`) ve uyarı görünür.

## Temalar

Kullanıcı temaları şu dizinde, tema başına bir dosya:

```
~/.config/bateri/themes/{ad}.toml
```

Dosyanın adı (`.toml` olmadan) temanın adıdır ve `[appearance] theme`'e
yazılan budur. Uyarılar dosyanın adıyla gelir:

```
bateri – themes/paper.toml: line 3: `ansi.red` must be a color like "#rrggbb", found "red"; using #d16d6a
```

### Biçim

Altı rol kökte, 16 ANSI rengi `[ansi]` bölümünde. Renk `"#rrggbb"` biçiminde
bir metindir (büyük harf de olur; `#rgb` ve alfa yok).

| anahtar | anlamı |
|---|---|
| `background` | varsayılan arka plan, pencerenin zemini |
| `foreground` | varsayılan ön plan |
| `dim` | sönük (SGR 2) yazılmış varsayılan ön plan |
| `accent` | vurgu; imleç ve **koşan** komutun işareti |
| `success` | durum: başarı; sıfır çıkış koduyla biten komutun işareti |
| `error` | durum: hata; sıfırdan farklı çıkış koduyla biten komutun işareti |
| `[ansi]` `black` `red` `green` `yellow` `blue` `magenta` `cyan` `white` | ANSI 0–7 |
| `[ansi]` `bright_black` … `bright_white` | ANSI 8–15, aynı sırada |

- **Her anahtar opsiyoneldir.** Eksik anahtar gömülü `bateri` temasından
  gelir; yalnız zemini değiştiren iki satırlık bir dosya geçerli bir temadır.
- Gömülü bir temayı **gölgeleyen** dosyada (`themes/bateri-light.toml`) eksik
  anahtar o gömülü temanın kendisinden gelir: yalnız `accent` yazmak açık
  temayı yalnız imleciyle değiştirir.
- Kabul edilmeyen renk (`"red"`, `"#12345"`, sayı) tabanın (`bateri` ya da
  gölgelenen gömülü tema) değerini alır ve uyarı verir; öteki renkler yine
  okunur.
- **Boş dosya** (ya da yalnız boşluk) kullanılamaz sayılır: çoğu editör
  kaydederken dosyayı önce boşaltır ve kaydın ortasında pencere tabana
  çakmamalı. Kaydettiğiniz anda ekrandaki tema kalır, açılışta görünüme uyan
  gömülü tema gelir; ikisinde de uyarı çıkar.
- Tanınmayan anahtar sessizce yoksayılır. Sonraki sürümlerin kalan iki durum
  rolü (uyarı, bilgi) bu yüzden bugünden yazılabilir.
- **Sönük metin** (SGR 2) iki yoldan gelir. Varsayılan ön plan sönükse
  temanın `dim` rengi kullanılır. Adlı ve 256 renkli metnin sönüğü ise bir
  kuraldır: renk temanın `background`'una doğru üçte bir yol alır — koyu
  temada koyulaşır, açık temada açılır. Siyah zeminde bu, alacritty'nin ve
  vte'nin "rengin üçte ikisi" kuralıyla aynıdır.
- `dim`, `success` ve `error` de her anahtar gibi eksikse `bateri`'den gelir:
  açık bir temada `dim` yazılmazsa sönük varsayılan metin koyu temanın grisiyle
  (`#909093`), `success`/`error` yazılmazsa komut işaretleri koyu temanın yeşil
  ve kırmızısıyla çizilir.

### Gömülü `bateri`

Bir temaya başlamanın en kısa yolu bunu kopyalayıp değiştirmek:

```toml
background = "#1a1c21"
foreground = "#d8d9dd"
dim = "#909093"
accent = "#7a9cc6"
success = "#8bb58b"
error = "#d16d6a"

[ansi]
black = "#22252b"
red = "#d16d6a"
green = "#8bb58b"
yellow = "#d6b16a"
blue = "#7a9cc6"
magenta = "#b08ec0"
cyan = "#79b3b3"
white = "#c8c9cc"
bright_black = "#4a4e57"
bright_red = "#e58b88"
bright_green = "#a4cba4"
bright_yellow = "#e8c988"
bright_blue = "#9bb8dc"
bright_magenta = "#c9aad8"
bright_cyan = "#96caca"
bright_white = "#e6e7ea"
```

### Gömülü `bateri-light`

Açık görünümün varsayılanı. Açık zeminde okunur kalsın diye sarı ve
camgöbeği koyu, doygun tonlarda; beyaz (`white`, `bright_white`) adının
anlamını korur ve açık uçta durur.

```toml
background = "#f5f6f8"
foreground = "#24262c"
dim = "#696b70"
accent = "#3d6aa8"
success = "#3b7a3b"
error = "#b5423d"

[ansi]
black = "#2b2e35"
red = "#b5423d"
green = "#3b7a3b"
yellow = "#8f6a00"
blue = "#3a66a6"
magenta = "#8a4c9c"
cyan = "#23787f"
white = "#b9bbc1"
bright_black = "#70737b"
bright_red = "#c9504a"
bright_green = "#4a8f4a"
bright_yellow = "#a67c00"
bright_blue = "#4a78ba"
bright_magenta = "#9d5db0"
bright_cyan = "#2f8a92"
bright_white = "#dcdee3"
```

İki blok da bir sınamayla gömülü temasına bağlıdır
(`documented_blocks_are_the_embedded_themes`): gömülü tema değişip blok
değişmezse sınama düşer.
