# Ayarlar

bateri'nin kullanıcı ayarları tek bir TOML dosyasında, renk temaları ayrı
dosyalarda durur. Bu belge anahtarların, tema biçiminin, varsayılanların ve
dosya bozukken ne olacağının **tek sahibidir**; kod tarafındaki karşılığı
`crates/bt-core/src/settings.rs` ve `theme.rs` (ayrıştırma),
`crates/bt-shell/src/settings.rs` (okuma ve tema adının çözümü).

## Dosyanın yeri

```
~/.config/bateri/settings.toml
```

Dosya yoksa her şey varsayılanıyla çalışır ve hiçbir uyarı çıkmaz. Dizini ve
dosyayı elle oluşturmak yeterli. Dosya başka bir yere sembolik bağ olabilir
(dotfile deposu); bağın hedefi okunur.

Değişiklik — ayar dosyasında da tema dosyasında da — **uygulamayı yeniden
açınca** geçerli olur.

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

| durum | sonuç |
|---|---|
| dosya yok | varsayılanlar, uyarı yok |
| dosya okunamıyor (izin, UTF-8 olmayan içerik, düz dosya değil, hedefi olmayan sembolik bağ) | varsayılanlar, uyarı |
| geçersiz TOML | **bütün** ayarlar varsayılan, uyarı satırı gösterir |
| bir anahtarın değeri kabul edilmiyor | yalnız o anahtar varsayılan (ya da sınırı), uyarı |
| tanınmayan anahtar ya da bölüm | sessizce yoksayılır |
| seçilen tema bulunamıyor | `bateri` teması, uyarı |
| tema dosyası okunamıyor ya da geçersiz TOML | `bateri` teması, uyarı (aynı adlı gömülü tema **kullanılmaz**) |
| tema dosyasında bir renk kabul edilmiyor | yalnız o renk `bateri`'ninki, uyarı |

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

Bölüm satır içi de yazılabilir: `terminal = { scrollback = 5000 }`.
`[[terminal]]` (bölüm dizisi) bölüm sayılmaz ve uyarı verir.

### `[appearance]`

```toml
[appearance]
theme = "bateri"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `theme` | metin, tema adı | `"bateri"` | kullanılacak renk teması |

- Ad önce `~/.config/bateri/themes/{ad}.toml` olarak aranır, yoksa gömülü
  temalar arasında. Bugün gömülü tek tema var: `bateri`.
- Aynı adlı bir dosya gömülü temayı **gölgeler**: `themes/bateri.toml`
  yazan kullanıcı gömülü `bateri`'yi değil kendi dosyasını görür.
- Hiçbir yerde bulunamayan ad `bateri`'ye döner ve uyarı verir.
- Boş ad ya da `/` içeren ad (`"../x"`) kabul edilmez, `"bateri"` olur ve
  uyarı verir: tema `themes/` dizininin dışından okunmaz.

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
- Adlı ve 256 renkli sönük metin bugün hâlâ rengin üçte ikisine iner; `dim`
  yalnız varsayılan ön planın sönüğüdür.

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

Blok bir sınamayla gömülü temaya bağlıdır
(`documented_bateri_block_is_the_embedded_theme`): gömülü tema değişip bu
blok değişmezse sınama düşer.
