# Komut blokları — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) ·
> [phase-3.md](phase-3.md) · [phase-4.md](phase-4.md)

Kabuğun bastığı OSC 133 işaretleri ilk kez ürüne dönüştü: her komut, sol
kenarda çıkış koduna göre renklenen bir işaretle kendi satırında görünüyor.
Dışarıya üç şey değişti — zsh sarmalayıcısı her prompt'a bir blok kimliği
basıyor (eski betikle açılmış oturumlarda çıpa yok, yani işaret de yok), tema
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
denetleniyor), `duman` pencereyi açan davranış değiştiği için (pay + işaret).

`make shader` **gerekmiyor**: `.metal` dosyaları ve `#[repr(C)]` düzenleri bu
sette hiç değişmedi — işaret mevcut `cell_bg` pipeline'ının genel piksel
dörtgeni. `make terminfo` girdisi yok.

### Beklenen çıktı

- `make hepsi` — `denetim: temiz`, clippy uyarısız, beş crate yeşil.
- `make test-yaris` — iki zamanlama profilinde de `race_*` ailesinin altısı
  yeşil; `race_shell_state_and_frame` ve `race_set_terminal_options_and_frame`
  **asılmamalı** (kilit sırası bekçisi: `shell` tutulurken `Term` istenmez).
- `make duman` — jetonlar **oynamamalı**: `hucre=8 glif=6 kural=15`,
  `kapanis=clean`, `icerik` sınırın altında, `sessiz` tabanın üstünde. Şerit
  bu kapının **dışında**: `smoke_shell` OSC 133 basmıyor, yani blok yok. Son
  koşu (işaret tasarımından sonra): `kare=29 hucre=8 glif=6 kural=15 icerik=2
  hareket=27 sessiz=1751.67ms kapanis=clean`.
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

- `false` ↵ → işaret **kırmızı** (`error`); `true` ↵ → **sakin yeşil**
  (`success`); komut koşarken **mavi** (`accent`).
- İşaret komutun satırında mı ve **yalnız** orada mı — çıktının solu boş
  kalmalı.
- Geçmişe kaydırınca işaretler satırlarıyla birlikte gidiyor mu; pencereyi
  **yatay** boyutlandırınca komut satırlarında kalıyorlar mı.
- **Şeridin genişliği.** Planda bir sayı yoktu; payın ortasındaki yarısı
  seçildi (iki yanında dörtte birlik nefes payı) ve bu setin **tek
  uydurulmuş ürün kararı**. İnce ya da kalın geliyorsa oran `Frame::push_block`
  içinde tek satır.
- Tema değiştirince (View ▸ Theme ▸) işaret **aynı karede** yeni palete
  geçmeli.

**Sonuç (2026-09-17, açık temada ölçüldü).** Üçü de doğru:

- **Genişlik onaylandı.** Pay `x=112..127` (16 fiziksel piksel = 8pt @2x),
  işaret `x=116..123` — payın tam ortasındaki yarısı, iki yanında 4'er piksel.
  Ekran görüntüsünden piksel ölçümüyle doğrulandı.
- **Renkler doğru ve prompt'un kendi renginden bağımsız.** İyi tanık: `false`
  düştükten sonraki prompt'un `→` oku **kırmızı**, ama o satırın şeridi
  **yeşil**, çünkü orada koşan `sleep 3` başarılı oldu. Ölçülen değerler
  temanın `#3b7a3b`/`#b5423d`/`#3d6aa8`'i (ekran görüntüsü Display P3'e
  çevirdiği için sayılar birebir değil, sapma üçünde de aynı yönde).
- **Tasarım değişti: bölge değil işaret** (kullanıcı kararı, gözle kontrol
  sonrası). İşaret artık komutun **kendi satırında** duruyor, çıktısının solunu
  boyamıyor. Değişiklik estetik değil yapısal — "bu satır hangi bloğun"
  sorusunun cevabı ancak çıpası görünen satırlar için **biliniyor**, ve bölge
  boyamak onu tahmine çeviriyordu. Üç sonucu:
  - **Koşan komutun pencereyi dibe kadar boyaması bitti**: `sleep 3` artık tek
    bir mavi işaret, otuz satırlık bar değil.
  - **İki bilinen sınır temsil edilemez oldu.** `exec zsh` sonrası payın
    kalıcı boyanması ve geçici prompt'ta (`TRANSIENT_PROMPT`) üst bölgenin
    yanlış renklenmesi — ikisi de bölge kollarından doğuyordu, o kollar
    silindi. Borç kaleminde yalnız `psvar[9]` kaldı.
  - **`Session::anchor_above` silindi.** Kaydırınca işaretin kaybolması bu
    tasarımda kusur değil: komut satırı ekranda değilse işaret de yok ve aynı
    satır her kaydırma konumunda aynı görünüyor. Tutarlılık, telafiden ucuz.
- **`>` şekli 011'e bırakıldı** (kullanıcı kararı). İşaretin dikdörtgen yerine
  chevron olması `bt-atlas`'a yedinci bir sprite türü ister; ve 011'de prompt'u
  terminal çizdiğinde `>` zaten **prompt'un kendisi** olacak, yani karar oraya
  ait.

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
onlarda çıpa yok, yani işaret yok. Yeni pencere yenisini alır. Hata değil, geri
düşüşün kendisi — kullanıcıya söylenecek bir şey varsa "yeni sekme aç".

### Yayın Checklist

- [x] `make kur` koştu, paket tazelendi (B.1)
- [x] Gerçek zsh oturumunda işaretler doğrulandı; genişlik **onaylandı**
      (B.2) — ve gözle kontrol tasarımı değiştirdi: işaret bölge değil komut satırı
- [x] Kendi teması olan kullanıcı varsa iki rol eklendi (B.3) — **no-op**:
      `~/.config/bateri/themes/` yok, gömülü temalar kullanılıyor

## Bilinen sınırlar

Biri kaldı ve yanlış çizmiyor — **çizmiyor**:

- **`psvar[9]`** geç kayıt olan bir `precmd` hook'uyla silinebilir (zsh-defer,
  p10k instant-prompt sonu): bütün çıpalar sessizce düşer, blok da işaret de
  olmaz. Borç listesinde (`docs/YOL-HARITASI.md`), çaresi bash/fish setinde.

**İkisi kapandı** ve ikisini de aynı şey kapattı — işaretin bölge değil satır
olması. `exec zsh` sonrası payın kalıcı boyanması ve geçici prompt'ta
(`TRANSIENT_PROMPT`) üst bölgenin yanlış renklenmesi, ikisi de "bu bölge şu
bloğun" tahmininden doğuyordu; o kollar silindi. Geçici prompt'un kalan etkisi
artık yanlış renk değil **eksik işaret**: p10k biten komutun prompt satırını
kendi `PROMPT`'uyla yeniden basınca çıpa gidiyor, yani o komut işaretsiz
kalıyor. Kapatan iş yine 011 (prompt'u terminalin çizmesi).

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
