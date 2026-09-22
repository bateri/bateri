# Glyph yedeği — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md)

Seçili fontta olmayan **tek hücrelik** karakter artık macOS'un kendi font
cascade'inden geliyor; motive eden belirti kapandı (`⏵` U+23F5 Menlo'da yok ve
Claude Code'un `⏵⏵ auto mode on` göstergesi iki kutu çıkıyordu). Kabul ölçütü
**geometrik ve tek**: adayın ilerlemesi hücrenin ilerlemesini aşıyorsa kutu
kalıyor, yani emoji, CJK, `.LastResort` ve Braille aynı kapıdan eleniyor —
aile adı karşılaştırması, trait biti ve sihirli dizge yok. Dışarıya görünen
başka bir değişiklik yok: yuva anahtarı, atlas biçimi, ayar şeması ve
`frame()` sınırı aynı; taban fontun rasteri bit bit aynı.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make duman
```

`make duman` gerekli çünkü çizim yolu değişti (`raster::draw` glyph'i artık
hücrede ortalıyor). `make shader`, `make terminfo`, `make test-yaris` ve
`make kur` **gereksiz**: `.metal`, `build.rs`, `assets/` ve paylaşılan durum
değişmedi.

### Beklenen çıktı

- `make hepsi` yeşil; `bt-atlas` 40 sınama.
- `make duman`: `hucre=8 glif=6 kural=15` — **değişmemiş olmalı**. Bu üçü
  ortalamanın taban fontta no-op olduğunun gerçek pencereden tanığı;
  oynarlarsa kaydırma sıfır değil demektir.
- Ölçüm sayısı **yok**: kare süresi, gecikme ve bellek iddiası taşımıyoruz.
  Yedeğin soğuk açılış bedeli emanette (bkz. B.1).

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `make duman` yeşil ve jetonlar beklenen (`kare=30 hucre=8 glif=6
      kural=15 icerik=3 hareket=27 sessiz=1754.22ms kapanis=clean`)
- [x] `/code-review` (set aralığı `1bd77b5^..HEAD`) koştu, 14 bulgu kapatıldı
- [x] `/audit` koştu; `make denetim` temiz, iki mercek bulgulu ve kapatıldı

## B. Yayın (doğrulamadan SONRA)

### B.1 Yedeğin kare süresine etkisini ölç `[komut]`

Phase-1'in `## Yayın Etkisi`'ndeki **ölçüm bekliyor** kalemi. Yedek arama
`Atlas::slot`'un çizim yolunda, yani display link callback'inde; bir font
ailesinin **ilk** açılışı kare bütçesiyle karşılaştırılabilir ölçüde ve
sayısı [phase-1.md](phase-1.md) → Uygulama Notları'nda **emanette**. Gerçek
pencerede kare süresine etkisi gösterilmeli ve sayılar oradan
`docs/OLCUMLER.md`'ye taşınmalı — emanetin sahibi `/measure`.

```sh
/measure
```

Bu bir **kapı değil**: ölçüm kullanıcı istediğinde koşar
(`.claude/is-akisi/proje.md` → Doğrulama).

### B.2 Yol haritasının iki yeni borcu `[elle]`

Set sırasında ölçülüp [docs/YOL-HARITASI.md](../../docs/YOL-HARITASI.md)'ye
yazıldı; ikisi de **bu setin dışında** ve ürün kararı bekliyor:

1. **Blok, çizgi ve Braille fonttan geliyor ve döşemiyor.** Kullanıcının
   bildirdiği maskot kusuru yedeğin konusu değildi — o karakterler Menlo'da
   var. Çaresi yordamsal çizim; 021'in "kutu çizim" yarısı bu gerekçeyle
   ayrılabilir.
2. **`line_height = 1.0` bir no-op değil**, hücreye bir piksel ekliyor.
   Düzeltmesi her kullanıcının ızgarasını oynatır.

Karar verilene kadar yapılacak bir şey yok; ikisi de kayıtlı.

### Yayın Checklist

- [~] B.1 — `/measure` ile yedeğin kare süresine **etkisi**. İki sebeple kapanmıyor: önce/sonra karşılaştırması GPU sütununun kararsızlığına düşüyor, ve ölçüm yükü (`load_shell`) düz ASCII bastığı için **yedek yoluna hiç girmiyor** — yani bugünkü kancayla tanığı yok. Emanetteki
      sayılar `docs/OLCUMLER.md`'ye taşındı
- [~] B.2 — yol haritasındaki iki borç için ürün kararı alındı: **birincisi
      (blok/çizgi/Braille yordamsal çizim) 2026-09-21'de sete bağlandı →
      `.tasks/021-kutu-cizim/`**; ikincisi (`line_height = 1.0` bir piksel
      ekliyor) hâlâ açık ve karar gerektirene kadar öyle kalıyor

## Geri Alma

Tek adım: setin üç commit'ini revert et (`9fdc90d`, `3d9b752`, `9d3f64f`).
Yedek arama tek bir kolda yaşıyor (`Atlas::slot`'un `Sprite::Char` çizim
adımı) ve dışarıya hiçbir yeni yüzey açmadı.

- **Ayar şeması:** geri alınacak şey yok — yeni anahtar eklenmedi, varsayılan
  değişmedi, anahtar silinmedi. `settings.toml`'daki tek değişiklik şablonun
  **yorumu**; eski şablonla yazılmış dosyalar zaten aynen okunuyor.
- **Tema, terminfo, app bundle, shell entegrasyonu:** dokunulmadı.
- **Atlas ve doku:** yuva anahtarı `(Sprite, Face, SizeClass)` olarak kaldı,
  `R8Unorm` biçim ve `TEXTURE_EDGE` değişmedi; revert sonrası eski atlas
  aritmetiği olduğu gibi geçerli.
- **Raster:** ortalama taban fontta tam olarak sıfır kaydırma ürettiği için
  revert görüntüde bir fark yaratmaz — yalnız yedekten gelen glyph'ler kutuya
  döner.
- **Belgeler:** `CLAUDE.md`, `docs/AYARLAR.md` ve `docs/YOL-HARITASI.md` aynı
  commit'lerde geri gelir. Yol haritasının **iki yeni borcu** ölçülmüş bilgi
  taşıyor; revert ediliyorsa onlar elle korunmalı, yoksa kullanıcının
  bildirdiği maskot kusurunun teşhisi kaybolur.
