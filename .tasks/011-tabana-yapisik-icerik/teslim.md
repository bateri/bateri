# Tabana yapışık içerik ve yumuşak kayma — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-0.md](phase-0.md) · [phase-1.md](phase-1.md) ·
> [phase-2.md](phase-2.md)

İçerik artık pencerenin **tabanına** yaslanıyor ve yeni satır geldiğinde
yumuşak kayıyor: imleç dipteki satırında duruyor, geçmiş arkasından yukarı
akıyor. Dışarıya görünen tek şey bu his — **ayar anahtarı eklenmedi** (kayma
`cursor_motion`'ı izliyor), `TERM`, tema biçimi, shell entegrasyonu ve paket
değişmedi, yeni bağımlılık yok. İki sözleşme büyüdü: duman jeton satırına
`kayma=` eklendi (hiçbiri silinmedi) ve `make duman`'ın **reçetesi** değişti —
ikinci `printf` artık imleci satır değil sütun oynatıyor (`\033[2G`), çünkü
tabana yapışmada satır hedefi ekran hareketini sıfıra indiriyordu.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make shader       # kapı commit'i cell.metal'in bir yorumunu düzeltti
make duman        # gerçek pencere ister; başsız kabukta yanlış tanıyla düşer
```

`make hepsi` her phase'in kapısı. `make duman` tetiklendi çünkü pencereyi açan
davranış değişti (yerleşim + ikinci animatör). `make shader` **yalnız kapı
commit'inde** tetiklendi: `cell.metal`'in `[[position]]` yorumu `setViewport`
geldikten sonra yanlış kalmıştı (instance uzayı dönüşümden önce, `[[position]]`
sonra) ve yorum da olsa `.metal`'e dokunmak kanaryayı şart koşuyor. Uniform ve
`#[repr(C)]` düzenleri değişmedi.

Koşullu hedeflerin geri kalanı **gerekmiyor** ve ikisi de bilinçli:

- **`make test-yaris`** — paylaşılan duruma dokunulmadı. `Motion` ana thread'e
  bağlı bir `Cell` (`link.rs` callback'i ana run loop'ta); `content_rows`
  `frame()`'in kendi kilidi altında toplanıyor ve `Origin` ana thread'de
  yazılıp okunuyor.
- **`make kur`** — `assets/bundle/*`, `assets/shell/*`, `crates/bateri` ve
  `kur` hedefi değişmedi. **`make terminfo`** girdisi yok.

**Kapı ve ardındaki yön düzeltmesi davranış değiştirdi:** kayma artık tek
yönlü — içerik büyüyünce süzülüyor, daralınca (vim'den çıkış, dolu ekranda
`clear`) anında oturuyor. Reçete ötelemeyi hiç oynatmadığı için jetonların
değişmesi **beklenmiyor**, ama hareket saatine dokunan bir değişiklikten sonra
`make duman`'ın bir kez daha koşması ucuz bir teminat.

### Beklenen çıktı

- `make hepsi` — `denetim: temiz`, clippy uyarısız, beş crate yeşil.
- `make duman` — üç sayaç **oynamamalı** (`hucre=8 glif=6 kural=15`), `icerik`
  `IDLE_FRAME_LIMIT`'in altında, `sessiz` tabanın (870 ms) üstünde,
  `kapanis=clean`. Son koşu (phase-2, kullanıcının gerçek penceresi):

  ```
  kare=29 hucre=8 glif=6 kural=15 icerik=2 hareket=27 kayma=0 sessiz=1748.29ms kapanis=clean
  ```

  **`kayma=0` doğru davranıştır**, kusur değil: reçete doluluğu
  (`content_rows`) hiç oynatmıyor, yani kayma doğmuyor. Jeton kapıda değil,
  tanıda — kırmızı bir koşuda `hareket` ile birlikte okununca hangi animatörün
  yerleşmediğini ayırt ediyor. `icerik`'in phase-0'daki `2`'de kalması da
  sözleşmenin parçası: öteleme **çizim zamanı**, yani yeni içerik karesi
  doğurmuyor.

**Ölçüm bekleyen iddia var** (aşağıda B.1); `docs/OLCUMLER.md`'ye bu sette
sayı işlenmedi.

### Doğrulama Checklist

- [x] `make hepsi` yeşil (üç phase'de de, en son phase-2'nin commit'inde)
- [x] `make duman` yeşil ve üç sayaç oynamadı; jeton satırı yukarıda
- [x] Göz kontrolü, kullanıcının gerçek penceresinde (phase-1: dipte açılış,
      tam ekran uygulamalar, üst boşluktan sürükleme; phase-2: Enter, `clear`,
      vim giriş/çıkış, tekerlek, kayma ortasında tıklama, `"snap"`, Hareketi
      Azalt)
- [x] `make shader` yeşil (kapı commit'i `cell.metal`'in yorumunu düzeltti)
- [x] `/code-review` + `/audit` koştu; on bir bulgudan onu kapatıldı, biri
      gerekçeli waive (`Slide`/`State` ikizliği → `/simplify`). Denetimin dört
      merceğinden ikisi temiz, ikisi bulgu verdi; `make denetim: temiz`

## B. Yayın (doğrulamadan SONRA)

### B.1 Bekleyen ölçümler `[komut]`

İki kalem, ikisi de `/measure`'ın işi — sayı uydurulmadı:

1. **Kayma animasyonunun yerleşme süresi.** Duman koşusu onu **göremiyor**
   (reçete ötelemeyi oynatmıyor, `kayma=0`), yani tek kapatıcısı ölçüm.
   `sessiz` bandına etkisi bu koşuda yok: 1748,29 ms ölçüldü, taban 870 ms.
2. **`hareket` ve `sessiz` bantlarının yeniden gözlenmesi** (phase-0). Reçete
   değişti; `hucre/glif/kural` sabit kaldı ama iki bandın sahibi
   `docs/OLCUMLER.md` → `## Boşta kare` ve orayı yalnız `/measure` yazar.
   **Kapı bir sahiplik ihlali kapattı:** phase-0'ın iki gözlemi
   (`\033[4G` ile `hareket=32`/`sessiz=1706,21 ms`, `\033[2G` ile
   `hareket=27`/`sessiz=1752,07 ms`) `smoke_shell`'in doc'unda bir koşu tablosu
   olarak duruyordu; oradan kaldırıldı ve sayılar şimdilik yalnız
   `phase-0.md`'de (defter). `/measure` onları `docs/OLCUMLER.md`'ye taşımalı —
   dosyanın kendi gürültü kuralı gereği tek koşu değil, profil başına en az on.

```sh
/measure
```

### B.2 Yön kuralının göz kontrolü `[elle]`

Kayma yönü kullanıcının ikinci tur göz kontrolünden çıktı (vim'e **girerken**
süzülmesi beğenilmişti); kural yazıldıktan sonra üç senaryosuna gerçek
pencerede bakılmalı:

- `vim`/`less`/`htop` **gir** → ızgara süzülerek yukarı geliyor (istenen).
- Aynılarından **çık** → kabuk süzülmüyor, anında yerine oturuyor.
- Dolu ekranda `clear` → prompt **anında** dibe oturuyor, kayarak inmiyor.
  Bu kuralın tek yeni tanığı; kapıda borç yazılmıştı, yön onu kapattı.
- İsteğe bağlı: aynı oturumda `make duman`. Reçete ötelemeyi oynatmıyor, yani
  jetonların değişmesi beklenmiyor.

### Yayın Checklist

- [x] Yön kuralının göz kontrolü yapıldı (B.2) — vim girişi süzülüyor, çıkış
      ve dolu ekranda `clear` anında oturuyor; kullanıcı onayladı
- [ ] `/measure` koştu; kayma yerleşme süresi ve iki bandın yeni gözlemi
      `docs/OLCUMLER.md`'ye işlendi (B.1)

## Bilinen sınırlar

Dördü de tasarım kararı, kusur değil — ikisinin adı planda, ikisi yolun
şeklinden çıktı:

- **Yön kuralının ölçülmemiş yarısı.** Kayma tek yöne indirildi (büyüme
  süzülür, daralma anında oturur) ve bunun kabul edilmiş bir bedeli var: art
  arda satır yazıp silen bir program — spinner, çok satırlı ilerleme çubuğu —
  büyürken kayıp daralırken zıplıyor, yani simetrik bir salınım yerine testere.
  Gerçek bir örnekte rahatsız ederse çare yön değil **mesafe** eşiği olur ve o
  ölçülmemiş bir sayı. Kalem `docs/YOL-HARITASI.md` → Sete bağlanmamış
  borçlar'da.

- **Kayma yolunun gerçek pencerede koşan bekçisi yok.** Duman reçetesi onu
  tetiklemiyor; kanıtı birim sınamaları ve göz kontrolü. Reçeteyi kaymayı
  tetikleyecek biçimde değiştirmek `hucre/glif/kural` ve `sessiz`'in ölçülmüş
  sözleşmesine dokunurdu (R3.1'in "ayrı commit" kuralı), o yüzden bu sette
  yapılmadı.
- **Ekran dolduktan sonra besleme kaymıyor.** Grid satırlarının kayması başka
  bir mekanizma (yumuşak kaydırma borcu, `docs/YOL-HARITASI.md`); dikiş adıyla
  kondu — ekranın dolduğu anda kayma durur.
- **Kayma boyunca en alt satır alt kenardan yükseliyor.** Tek `setViewport`
  dört listeyi birden kaydırdığı için öteleme kayma boyunca hedefinden büyük
  kalıyor. "Yeni satır yerinde belirsin, ötekiler kaysın" bu mimaride temsil
  edilebilir değil; tespit `set_origin`'in doc'unda.

## Geri Alma

Setin üç kod commit'i var — `fa24586` (duman reçetesi), `0271989` (tabana
yapışma), `34c9d3c` (yumuşak kayma) — yanında kapı commit'i ve planlama
commit'i `bddccc0`. Sıra **tersten** olmalı, phase'ler üst üste kuruyor.

- **Yalnız kapıyı geri almak** (`git log --grep '011-tabana-yapisik-icerik kapı'`):
  alternatif ekran geçişi yeniden **kayar**
  (vim'e girmek ızgarayı pencerenin altından süzer) ve fare eşlemesi
  ötelemeyi encode edilemeyen kareden de okur. İkisi de kusur, yani bu geri
  alma tek başına anlamsız — kapı commit'i belge düzeltmelerini de taşıyor ve
  onlar koda bağlı.
- **Yalnız kaymayı geri almak** (`34c9d3c`): içerik tabana yapışık kalır, yeni
  satırda anında sıçrar. En ucuz geri dönüş; `kayma=` jetonu satırdan düşer ve
  jeton sözleşmesi "silinmez" kuralına **aykırı** olacağı için bu geri alma
  kalıcılaşırsa jeton `kayma=off` olarak bırakılmalı.
- **Tabana yapışmayı da geri almak** (`0271989`): içerik tavana döner. Üç
  tüketici birlikte dönmeli — `frame()`'in `content_rows`'u, `setViewport` ve
  `point_to_cell`'in origin okuması; ayrı ayrı alınırsa fare dikeyde kayar.
- **Duman reçetesini geri almak** (`fa24586`): **yalnız** üstteki ikisi de
  geri alındıysa anlamlı. Tabana yapışma yerindeyken `\033[H`'ye dönmek
  `hareket`'i sıfıra indirir ve kapı **kod doğruyken** kırmızı düşer.
- **Ayar göçü yok**: anahtar eklenmedi, silinmedi, adı değişmedi. `feed_lift`
  bilinçli olarak reddedildi (`discussion.md` → Muhakeme, 2. tur).
