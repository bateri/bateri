# Ayarlar

bateri'nin kullanıcı ayarları tek bir TOML dosyasında durur. Bu belge
anahtarların, varsayılanların ve dosya bozukken ne olacağının **tek
sahibidir**; kod tarafındaki karşılığı `crates/bt-core/src/settings.rs`
(ayrıştırma) ve `crates/bt-shell/src/settings.rs` (okuma).

## Dosyanın yeri

```
~/.config/bateri/settings.toml
```

Dosya yoksa her şey varsayılanıyla çalışır ve hiçbir uyarı çıkmaz. Dizini ve
dosyayı elle oluşturmak yeterli. Dosya başka bir yere sembolik bağ olabilir
(dotfile deposu); bağın hedefi okunur.

Değişiklik **uygulamayı yeniden açınca** geçerli olur.

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
