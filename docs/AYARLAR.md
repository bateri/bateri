# Ayarlar

bateri'nin kullanıcı ayarları tek bir TOML dosyasında, renk temaları ayrı
dosyalarda durur. Bu belge anahtarların, tema biçiminin, varsayılanların ve
dosya bozukken ne olacağının **tek sahibidir**; kod tarafındaki karşılığı
`crates/bt-core/src/settings.rs` ve `theme.rs` (ayrıştırma),
`crates/bt-shell/src/settings.rs` (okuma ve tema adının çözümü),
`watch.rs` (dosyaların izlenmesi).

## Dosyanın yeri

```
~/.config/bateri/settings.toml
```

Dosya yoksa her şey varsayılanıyla çalışır ve hiçbir uyarı çıkmaz. Dizini ve
dosyayı elle oluşturmak yeterli. Dosya başka bir yere sembolik bağ olabilir
(dotfile deposu); bağın hedefi okunur.

Değişiklik **kaydettiğiniz anda** geçerli olur — ayar dosyasında da,
kullanılan temanın dosyasında da; kabuk ve içindeki program yaşamaya devam
eder. Editörün kaydı nasıl yaptığı fark etmez (yerinde yazma, boşaltma,
geçici dosya ve üstüne taşıma, sembolik bağın hedefine yazma). Sistemin açık/koyu
görünümü de anında izlenir: tema `"system"` iken (varsayılan) Sistem
Ayarları'nda görünüm değişince pencere de değişir.

İzlenen yer `~/.config/bateri/` dizinidir. Uygulama açıkken bu dizin **hiç
yoksa** oluşturmak izlemeyi başlatmaz: dizini kabuktan ilk kez oluşturan
kullanıcı değişikliklerini uygulamayı yeniden açınca görür, sonrası kayıt
anında izlenir.

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
| dosya okunamıyor (izin, UTF-8 olmayan içerik, düz dosya değil, hedefi olmayan sembolik bağ) | varsayılanlar, uyarı |
| geçersiz TOML | **bütün** ayarlar varsayılan, uyarı satırı gösterir |
| bir anahtarın değeri kabul edilmiyor | yalnız o anahtar varsayılan (ya da sınırı), uyarı |
| tanınmayan anahtar ya da bölüm | sessizce yoksayılır |
| seçilen tema bulunamıyor | görünüme uyan gömülü tema (koyuda `bateri`, açıkta `bateri-light`), uyarı |
| tema dosyası okunamıyor ya da geçersiz TOML | görünüme uyan gömülü tema, uyarı (aynı adlı gömülü tema **kullanılmaz**) |
| tema dosyasında bir renk kabul edilmiyor | yalnız o renk `bateri`'ninki, uyarı |

Tema kuralı görünüm değişiminde de aynı: yeni görünümün teması
kullanılamıyorsa o görünüme uyan gömülü tema gelir — pencere öteki
görünümün temasında kalmaz.

**Kaydettiğiniz anda** ise kural düzenlemeyi korur: yarım kalmış bir kayıt
ekranı bozmaz, uyarı çıkar ve dosyayı düzeltip kaydedince uyarı kalkar.

| kayıt anında | sonuç |
|---|---|
| ayar dosyası geçersiz TOML ya da okunamıyor | **hiçbir ayar değişmez**, uyarı |
| ayar dosyası silindi ya da boşaltıldı | ayarlar değişmez, uyarı yok; varsayılanlar uygulamayı yeniden açınca gelir |
| bir anahtarın değeri kabul edilmiyor | o anahtar **değişmez**, uyarı; tavanı aşan `scrollback` tavana iner |
| anahtar dosyadan silindi | o anahtar varsayılanına döner |
| seçilen tema bulunamıyor, dosyası okunamıyor ya da geçersiz TOML | **ekrandaki tema kalır**, uyarı |

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

Dört rol kökte, 16 ANSI rengi `[ansi]` bölümünde. Renk `"#rrggbb"` biçiminde
bir metindir (büyük harf de olur; `#rgb` ve alfa yok).

| anahtar | anlamı |
|---|---|
| `background` | varsayılan arka plan, pencerenin zemini |
| `foreground` | varsayılan ön plan |
| `dim` | sönük (SGR 2) yazılmış varsayılan ön plan |
| `accent` | vurgu; bugün imleç |
| `[ansi]` `black` `red` `green` `yellow` `blue` `magenta` `cyan` `white` | ANSI 0–7 |
| `[ansi]` `bright_black` … `bright_white` | ANSI 8–15, aynı sırada |

- **Her anahtar opsiyoneldir.** Eksik anahtar gömülü `bateri` temasından
  gelir; yalnız zemini değiştiren iki satırlık bir dosya geçerli bir temadır.
- Kabul edilmeyen renk (`"red"`, `"#12345"`, sayı) `bateri`'nin değerini alır
  ve uyarı verir; öteki renkler yine okunur.
- Tanınmayan anahtar sessizce yoksayılır. Sonraki sürümlerin dört durum rolü
  (başarı, uyarı, hata, bilgi) bu yüzden bugünden yazılabilir.
- **Sönük metin** (SGR 2) iki yoldan gelir. Varsayılan ön plan sönükse
  temanın `dim` rengi kullanılır. Adlı ve 256 renkli metnin sönüğü ise bir
  kuraldır: renk temanın `background`'una doğru üçte bir yol alır — koyu
  temada koyulaşır, açık temada açılır. Siyah zeminde bu, alacritty'nin ve
  vte'nin "rengin üçte ikisi" kuralıyla aynıdır.
- `dim` de her anahtar gibi eksikse `bateri`'den gelir (`#909093`): açık bir
  temada `dim` yazılmazsa sönük varsayılan metin koyu temanın grisiyle
  çizilir.

### Gömülü `bateri`

Bir temaya başlamanın en kısa yolu bunu kopyalayıp değiştirmek:

```toml
background = "#1a1c21"
foreground = "#d8d9dd"
dim = "#909093"
accent = "#7a9cc6"

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
