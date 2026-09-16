# Komut blokları — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) ·
> [phase-3.md](phase-3.md) · [phase-4.md](phase-4.md)

Kabuğun bastığı OSC 133 işaretleri ilk kez ürüne dönüştü: her komut, sol
kenarda çıkış koduna göre renklenen bir şeritle kendi bloğu olarak görünüyor.
Dışarıya üç şey değişti — zsh sarmalayıcısı her prompt'a bir blok kimliği
basıyor (eski betikle açılmış oturumlarda çıpa yok, yani şerit de yok), tema
biçimi iki yeni rol kazandı (`success`, `error`) ve ızgara soldan sabit bir pay
kadar daralıyor, yani `cols` tipik punto/ölçekte **bir** sütun azalabiliyor.
Ayar anahtarı eklenmedi, `TERM` değişmedi, yeni bağımlılık yok.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make test-yaris
make kur
make duman
```

`make hepsi` her phase'in kapısı. Üçü koşullu ve dördü de bu sette tetiklendi:
`test-yaris` paylaşılan duruma dokunulduğu için (okuyucu thread ↔ kare yolu;
kapı commit'i `set_terminal_options`'a bir yaprak kilit daha ekledi), `kur`
`assets/shell/*` değiştiği için (betik pakete kopyalanıp `cmp` ile
denetleniyor), `duman` pencereyi açan davranış değiştiği için (pay + şerit).

`make shader` **gerekmiyor**: `.metal` dosyaları ve `#[repr(C)]` düzenleri bu
sette hiç değişmedi — şerit mevcut `cell_bg` pipeline'ının genel piksel
dörtgeni. `make terminfo` girdisi yok.

### Beklenen çıktı

- `make hepsi` — `denetim: temiz`, clippy uyarısız, beş crate yeşil.
- `make test-yaris` — iki zamanlama profilinde de `race_*` ailesinin altısı
  yeşil; `race_shell_state_and_frame` ve `race_set_terminal_options_and_frame`
  **asılmamalı** (kilit sırası bekçisi: `shell` tutulurken `Term` istenmez).
- `make duman` — jetonlar **oynamamalı**: `hucre=8 glif=6 kural=15`,
  `kapanis=clean`, `icerik` sınırın altında, `sessiz` tabanın üstünde. Şerit
  bu kapının **dışında**: `smoke_shell` OSC 133 basmıyor, yani blok yok. Son
  koşu: `kare=30 hucre=8 glif=6 kural=15 icerik=3 hareket=27 sessiz=1750.64ms
  kapanis=clean`.
- `make kur` — çıkış 0; `Info.plist`, ikon, lisans ve `Contents/Resources/shell`
  altındaki betik denetimden geçmeli.

**Ölçüm yok.** Bu set kare süresi, gecikme ya da bellek **iddiası taşımıyor**;
`docs/OLCUMLER.md`'ye işlenecek bir sayı ve bekleyen bir `/measure` kalemi yok.
Tek sayısal satır `BlockLog`'un doc'undaki türetme (kayıt başına 8 bayt →
varsayılan 10 000 satırda 80 KB), ki o bir `const` türetmesi, koşu sonucu değil.

### Doğrulama Checklist

- [x] `make hepsi` yeşil
- [x] `make test-yaris` yeşil (iki profil, altı `race_*`)
- [x] `make duman` yeşil ve jetonlar oynamadı
- [x] `make kur` yeşil (phase-1 ve phase-2'de koştu)
- [x] `/code-review` + `/audit` koştu; bulgular kapatıldı ya da gerekçelendi
      (commit `066b59a`)

## B. Yayın (doğrulamadan SONRA)

### B.1 Paketi kur `[komut]`

Yeni sarmalayıcı betiği ancak pakete girdikten sonra kullanılıyor (debug'da
depodan okunuyor). Kurulu bir `bateri.app` varsa tazelenmeli:

```sh
make kur
```

### B.2 Gerçek bir zsh oturumunda gözle kontrol `[elle]`

Phase-4'ün Kabul listesinin tek açık maddesi (`[~]`). `bateri.app`'i açıp:

- `false` ↵ → şerit **kırmızı** (`error`); `true` ↵ → **sakin yeşil**
  (`success`); komut koşarken **mavi** (`accent`).
- Şerit **prompt satırından** başlıyor mu — blok komutun kendisini de
  kapsamalı, yalnız çıktısını değil.
- Geçmişe kaydırınca şeritler satırlarıyla birlikte gidiyor mu; pencereyi
  **yatay** boyutlandırınca prompt satırlarında kalıyorlar mı.
- **Şeridin genişliği.** Planda bir sayı yoktu; payın ortasındaki yarısı
  seçildi (iki yanında dörtte birlik nefes payı) ve bu setin **tek
  uydurulmuş ürün kararı**. İnce ya da kalın geliyorsa oran `Frame::push_block`
  içinde tek satır.
- Tema değiştirince (View ▸ Theme ▸) şerit **aynı karede** yeni palete
  geçmeli.

**Sonuç (2026-09-17, açık temada ölçüldü).** Üçü de doğru:

- **Genişlik onaylandı.** Pay `x=112..127` (16 fiziksel piksel = 8pt @2x),
  şerit `x=116..123` — payın tam ortasındaki yarısı, iki yanında 4'er piksel.
  Ekran görüntüsünden piksel ölçümüyle doğrulandı.
- **Renkler doğru ve prompt'un kendi renginden bağımsız.** İyi tanık: `false`
  düştükten sonraki prompt'un `→` oku **kırmızı**, ama o satırın şeridi
  **yeşil**, çünkü orada koşan `sleep 3` başarılı oldu. Ölçülen değerler
  temanın `#3b7a3b`/`#b5423d`/`#3d6aa8`'i (ekran görüntüsü Display P3'e
  çevirdiği için sayılar birebir değil, sapma üçünde de aynı yönde).
- **Koşan blok mavi ve pencerenin dibine kadar uzuyor** — plan böyle diyor
  (`son bloğun sonu pencerenin altıdır`) ama çıktısız bir komutta boş ekrana
  uzun bir bar çiziyor. **Bu sette düzeltilmedi ve düzeltilmemeli:** kullanıcı
  kararıyla içerik 011'de tabana yapışacak (`docs/YOL-HARITASI.md` → 011), ve o
  düzende aralık kendiliğinden kısalıyor. Şeridi ayrıca yamamak aynı şeyi iki
  kez çözmek olurdu.

### B.3 Kullanıcı temaları `[elle]`

Tema biçimi büyüdü: `success` ve `error` kökte iki yeni rol. Eksik anahtar
yuvaya dokunmadan geçiyor ve gömülü `bateri`'den miras alınıyor, yani mevcut
kullanıcı temaları **okunmaya devam ediyor**. Kendi **açık** temasını yazmış
kullanıcı iki rolü koyu temadan miras alır — `dim` ile aynı, kabul edilmiş ve
`docs/AYARLAR.md`'de belgelenen kusur. Kendi teması olan biri varsa iki satır
eklemesi yeterli:

```toml
success = "#3b7a3b"
error   = "#b5423d"
```

### B.4 Açık oturumlar `[elle]`

Betik güncellendi: **açık** pencereler eski betikle koşmaya devam eder ve
onlarda çıpa yok, yani şerit yok. Yeni pencere yenisini alır. Hata değil, geri
düşüşün kendisi — kullanıcıya söylenecek bir şey varsa "yeni sekme aç".

### Yayın Checklist

- [x] `make kur` koştu, paket tazelendi (B.1)
- [x] Gerçek zsh oturumunda şeritler doğrulandı; şerit genişliği **onaylandı**
      (B.2) — koşan bloğun uzunluğu bilinçli olarak 011'e bırakıldı
- [ ] Kendi teması olan kullanıcı varsa iki rol eklendi (B.3) — kullanıcı
      temasınız yoksa yapılacak bir şey yok

## Bilinen sınırlar

Üçü de kayıtlı ve hiçbiri yanlış çizmiyor — **çizmiyor**:

- **Geçici prompt** (`TRANSIENT_PROMPT`, p10k): çıpa yalnız canlı prompt'ta
  kaldığı için pencerenin üstü yanlış renk alabilir. Kapatan iş 011 (prompt'u
  terminalin çizmesi). Gerekçe `resolve_blocks`'ta.
- **`exec zsh` sonrası pay kalıcı vurgu rengi**: yeniden doğan kabuk
  sarmalayıcıyı yüklemiyor, `D` hiç gelmiyor ve kabuk `Running`'de takılı
  kalıyor. Borç listesinde (`docs/YOL-HARITASI.md`), çaresi bash/fish setinde.
- **`psvar[9]`** geç kayıt olan bir `precmd` hook'uyla silinebilir (zsh-defer,
  p10k instant-prompt sonu): bütün çıpalar sessizce düşer, blok da şerit de
  olmaz. Aynı borç kaleminde.

## Geri Alma

Setin dört kod commit'i var (`06a83a9`, `f39aeb7`, `0c6f32f`, `2fd785f`) ve bir
kapı commit'i (`066b59a`). Hepsi `main`'de ve ayrı ayrı geri alınabilir, ama
sıra **tersten** olmalı — sonraki phase öncekinin üstüne kuruyor.

- **Yalnız şeridi geri almak** (`2fd785f`): pay ayrılmış kalır, blok aralıkları
  sınırdan geçmeye devam eder, hiçbir şey çizilmez. En ucuz geri dönüş.
- **Payı da geri almak** (`0c6f32f`): `cols` bir sütun geri kazanılır; üç
  tüketici (`cols` hesabı, çizim orijini, fare eşlemesi) birlikte dönmeli,
  ayrı ayrı alınırsa fare bir sütun kayar.
- **Tema rollerini geri almak** (`f39aeb7` içinde): `success`/`error` gömülü
  temalardan düşer. Kullanıcının o anahtarları yazmış olması **sorun değil** —
  tanınmayan anahtar sessizce yoksayılıyor ve tema okunmaya devam ediyor;
  `docs/AYARLAR.md` de aynı commit'te dönmeli.
- **Betiği geri almak** (`06a83a9` içinde): `make kur` yeniden koşmalı, yoksa
  pakette yeni betik kalır. Kullanıcının rc dosyasına hiçbir şey yazılmadığı
  için temizlenecek iz yok.
- **Ayar göçü yok**: anahtar eklenmedi, silinmedi, adı değişmedi.
