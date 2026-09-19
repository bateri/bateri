# 014-imlec-stilleri — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md)

İmleç artık uygulamanın istediği **şekli** alıyor (blok, alt çizgi, dikey
çubuk), istenirse **yanıp sönüyor** ve rengini `accent`'ten değil **kendi tema
rolünden** alıyor — koyu temada altın. Dışarıya görünen üç şey: DECSCUSR
dizileri (vim'in insert modu) artık çiziliyor, `[terminal]` iki yeni anahtar
kazandı (`cursor`, `cursor_blink`), ve tema dosyası opsiyonel bir `cursor` rolü
okuyor.

## Bedeli, açıkça

Blink açıkken pencere **boşta değil**: saniyede iki kare. 013 ilk istisnayı
getirmişti ("koşan komutu olan pencere boşta değildir") ve o istisna komut
bitince kapanıyordu; bu kapanmıyor. O yüzden **varsayılan kapalı** ve açıkken
bile klavye/çıktı sessizliğinden 15 saniye sonra duruyor.

**Kapının hiçbir katı bozuk bir blink'i görmüyor** ve bu yazılı olsun: blink
karesi `icerik=`'i artırmıyor (tasarım), `hareket=`'i artırmıyor (`Motion`'ın
dışında), `istek=`'i artırmıyor (`resume` sayaca dokunmuyor). Geriye `kare=` ve
`sessiz=` kalıyor ve ikisi de blink **gerçekten koşarsa** kımıldıyor. Koruma
bir jeton değil **varsayılanın kendisi**; `saat=` jetonu bilerek eklenmedi
(emsal `cpu_elenen=`, ölçülmüş ihtiyaç beklemeden atılmadı).

## Kapı

**`/code-review`** setin aralığında koştu (phase-2'nin commit'i hariç — o phase
sonunda zaten incelenmişti ve 11 bulgusu düzeltilmişti). **13 bulgu; 12
düzeltildi, 1 kullanıcı kararı bekliyor, 1 gerekçeli waive.** İkisi gerçek kod
kusuruydu ve ikisini de hiçbir sayaç görmezdi:

- **Seçim, geniş karakterin yarısını yutuyordu.** Vurguya eklenen "çizilir mi"
  kapısı spacer hücresini eliyordu; seçili bir CJK karakteri yarım
  vurgulanıyor, `selection_text()` ise onu bütün kopyalıyordu. Bugün geniş
  glyph tek yuvaya kırpıldığı için örtülü, 016 görünür kılardı. Bekçi de
  **fixture şansıyla** geçiyordu (komşu sınama zemini boyuyordu); yenisi
  varsayılan renklerle.
- **`caret_rect` sıfır hücrede panik ediyordu.** `f32::clamp` `min ≤ max`
  istiyor; debug'da başka bir assert önce düşüyor ama `clamp`'ınki **release'de
  de** var — sürüm derlemesinde display link callback'i içinde bir pencereyi
  öldürürdü.

Kalan on belge kusuru: `auto`'nun değişen anlamı şablona ve `AYARLAR.md`'ye
işlenmemişti (**üstelik şablon kullanıcının diskine yazılıyor**), şablonun
başlığı `clipboard.osc52`'nin yazılı istisnasını yok sayıyordu, blink'in durma
sayacı "son tuştan" diye anlatılıyordu (oysa **çizime** bakıyor — `tail -f` onu
ayakta tutar), gölge tema maddesi bir alttakiyle çelişiyordu, `accent_linear`'ın
doc'unu yeni `cursor_linear` yutmuştu, `Theme`'in kendi doc'u hâlâ "sekiz rol"
diyordu, 013'ün sayaç gerekçesi artık ulaşılamaz bir hâli anıyordu, seçim
kapısı taşınırken `hidden`/`ruled` değişmezlerinin gerekçesi silinmişti ve
`truncate(bg_count)` atıfları dört yerde 012'den beri bayattı.

**`/audit`**: `make denetim` **temiz**. Bağımlılık ve shader/düzen mercekleri
**ilgisiz** (Cargo dosyaları ve `.metal` hiç değişmedi). Ayar/tema şeması,
ölçüm sahipliği, thread/blokaj ve dil **temiz**. Boşta sıfır kare merceği
davranışta temiz ama **bir belge bulgusu** çıkardı ve düzeltildi: yasağın
öznesi altı yerde `Waker` **tipi** olarak yazılıydı, oysa yasak
`Waker::wake`'in hasar bayrağına ait — `Waker::resume` hasar dikmediği için
aynı yasağın altına girmiyor. Kod doğruydu, cümleler eskiydi.

## A. Doğrulama

```sh
make hepsi          # exit 0
make test-yaris     # exit 0
```

### Doğrulama Checklist

- [x] `make hepsi` yeşil — her phase'de ve kapıda **0**
- [x] `make test-yaris` yeşil (phase-2 paylaşılan duruma dokundu)
- [x] `make duman` — **kullanıcı koştu** (2026-09-19):
      `kare=29 hucre=8 glif=6 kural=15 icerik=2 hareket=27 sessiz=1755.67ms
      kapanis=clean`. `icerik` blink'ten önceki değerinde ve `sessiz` tabanın
      iki katı: hermetik koşuda saat hiç kurulmuyor
- [x] `/code-review` (set aralığı, phase-2 hariç) — 13 bulgu, 12 düzeltildi
- [x] `/audit` — `make denetim` temiz, 1 belge bulgusu düzeltildi
- [ ] **Gözle kontrol** (aşağıda B.1)

### Ölçüm bekleyen iddia

- **"Saatin hareket tadı içerik tadından ucuz."** Yönü koddan kanıtlı (o kol
  `Term` kilidine girmiyor, ızgarayı taramıyor, `bt-core`'a hiç gitmiyor);
  **büyüklüğü ölçülmedi**. Kancası yok ve bu set kanca doğurmadı.

## B. Yayın

### B.1 Gözle kontrol `[elle]`

`cargo run -q -p bateri` ile:

1. **Şekil.** `vim` aç, `i`'ye bas → dikey çubuk; `Esc` → blok. `printf '\e[3 q'`
   → alt çizgi. Ayar dosyasına `[terminal] cursor = "beam"` yaz, kaydet → kayıt
   anında uygulanmalı.
2. **Blink.** `cursor_blink = "auto"` → düz promptta sönmeli. **Yazarken sabit
   kalmalı**, elini çekince yarım saniye içinde sönmeye dönmeli. 15 saniye
   dokunmayınca **durmalı ve görünür kalmalı**; ilk tuşta geri gelmeli.
3. **Blink × sayaç.** `sleep 5` koştur: blink sürerken süre sayacı hâlâ
   saniyede bir ilerlemeli. *Bu setin en kırılgan yeriydi.*
4. **Renk.** İmleç altın, **koşan komutun şeridi mavi** — ikisinin ayrıldığının
   tek bakışta görülen kanıtı bu.
5. **Dock.** Dock'ta yazarken şekil ve blink aynı olmalı (caret tek).

### Yayın Checklist

- [ ] B.1 gözle kontrol
- [ ] `/ship`

`make kur` **gerekmiyor**: kabuk betiği, terminfo, jeton satırı ve app bundle
değişmedi.

## Kullanıcı kararı bekleyen

**Tema dosyasında `cursor` eksikse `accent`'e düşüyor** — gerekçesi "rolden
önce yazılmış tema dosyaları tek harf değişmeden aynı görünsün". Ama kural
**yeni** dosyaları da vuruyor: `themes/bateri.toml`'a tek satır
`background = "#101010"` yazan biri imleci altından maviye çevirir ve gömülü
altını geri almanın yolu hex'i elle yazmaktır.

İkisi aynı anda olamaz (eski dosyaların görüntüsünü korumak ↔ yeni dosyaların
gömülü rolü miras alması); bugünkü kural birincisini seçiyor. Kullanıcının
tema dosyası yok, yani pratikte etkisiz — ama kural kalıcı.

## Bilinen sınırlar

- **Ham kontrol karakteri tazelik kapısını hâlâ düşürür.** Ctrl-V ile basılan
  `\x01` ZLE'de `^A` diye çiziliyor, yani ayna ile ızgara farklı şey söylüyor
  ve caret dock'tan ızgaraya sıçrıyor. Sekme düzeltildi, bu ölçülmedi;
  yön güvenli (satır iki yerde görünür, sessizce kaybolmaz).
- **`QUIET_FLOOR` ile blink periyodu ilkesel olarak uyumsuz.** Kapı 868 ms
  sessizlik istiyor, blink yarım saniyede bir kare. Bugün çarpışmıyorlar
  (varsayılan kapalı + reçete `/bin/sh`), ama muafiyetin cinsi 013'ünkinden
  **zayıf**: orada bütün bir alt sistem yoktu, burada reçetede dört baytlık bir
  kaçış dizisi yok — bir `printf` uzaklıkta. Kırılırsa **sesli** kırılıyor.
- **`CellMetrics::new` dört çıplak `u16` alıyor** ve `gutter` ile `rule` yer
  değiştirse derleyici görmez (`/code-review`, **waive**). Gerçek bir tuzak;
  çaresi ya 20 çağrı yerini builder'a çevirmek ya sessiz bir varsayılan — ikisi
  de bu setin konusundan büyük. Kayda geçti.
- **Blink'in durma sayacı çizime bakıyor, klavyeye değil.** `tail -f` gibi akan
  bir çıktı blink'i süresiz ayakta tutar. Belgede yazılı; kusur değil, tetiğin
  `bt-shell`→`bt-gpu` bir sinyal istememesinin sonucu.
- **Odak sınırdan geçmiyor**, yani içi boş imleç yok ve odaksız pencerede blink
  sürüyor. 015'in konusu.
- **Süre sayacının tiki artık tam sınıra oturuyor.** Eski (rölatif) kurulum bir
  karelik pay bırakıyordu; yeni mutlak son tarih aritmetik olarak sınırda.
  Pay artık `wake` → ana kuyruk → `setPaused(false)` → vsync zincirinde ve o
  zaten ≥ bir kare. **Okundu, ölçülmedi** (`/audit`).

## Geri Alma

- **Kod:** phase commit'lerini revert. Üçü bağımsız — renk (phase-3) tek
  başına, blink (phase-2) tek başına, şekiller (phase-1) en altta.
- **Ayar şeması:** `cursor` ve `cursor_blink` **silinmez**; geri alınırsa
  emekli edilir (dosyada korunur, okunmaz, görülünce tanı bırakır) — 009'un
  `prompt` anahtarı emsali.
- **Tema biçimi:** `cursor` rolü opsiyonel ve eksikte `accent`'e düşüyor, yani
  geri alınsa da kullanıcı tema dosyaları **değişmeden** okunmaya devam eder.
- **Belge:** `CLAUDE.md`, `docs/AYARLAR.md`, `docs/ARASTIRMA.md` ve
  `docs/YOL-HARITASI.md` aynı commit'lerde; revert onları da geri alır.

## Sonraki iş

- **015-imlec-cilasi** açıldı ve planı hazır: caret'in kendi fragment'i
  (yarıçap + hale), odak kaybında içi boş imleç, ve Enter'da caret'in yukarı
  çıkıp inmesi kusuru (histerezisle).
- **OSC 12** (uygulamanın imleç rengini değiştirmesi): terminfo'da `Cs`/`Cr`
  ilan ediyoruz, uygulamıyoruz. 014 rolü ayırarak yolu açtı.
