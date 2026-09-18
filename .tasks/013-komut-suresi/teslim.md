# 013-komut-suresi — Teslim

## Ne indi

Bir saniyeyi geçen komutların süresi, komut satırının **sağ ucunda**, sönük:
koşarken saniyede bir ilerleyen canlı bir sayaç, bitince donan son değer.

- **Defter** (`bt-core::shell`): `Outcome::Finished` çıkış kodunun yanına geçen
  süreyi de taşıyor; başlangıç anı `ShellLog`'da **tek** alan.
- **Sayaç** (`bt-core::session`): yeni sınır tipi yok — `resolve_blocks` metni
  yığın tamponunda üretip mevcut sink'e hücre basıyor, `bt-gpu` onu sıradan bir
  harften ayırt etmiyor. Satırın son **dolu** hücresine değecekse
  **çizilmiyor** — komut metni de seçim vurgusu da örtülmüyor.
- **Saat** (`bt-gpu::link`): kare talebinin **üçüncü** sebebi. Süresi ve durma
  koşulu `bt-core`'dan (`Cursor::next_tick`).

## Bedeli, açıkça

Koşan `sleep 300` artık **300 içerik karesi** ediyor, sıfır değil. Doğru
bedel — canlı bir sayacın başka bir karşılığı yok — ama bu depo on bir phase
boyunca "boşta sıfır kare"yi savundu, o yüzden yazılı olsun. Komut koşmuyorken
hiçbir şey değişmedi: saat armed değilken tek bir saat okuması bile yok.

## Kapı

`/code-review` **13 bulgu** çıkardı ve 13'ü de düzeltildi; `make denetim`
temiz. Dördü gerçek davranış kusuruydu ve hiçbirini bir sayaç görmezdi:

- **Saatin durma koşulu bir kare geç işliyordu.** `arm_clock` `next_tick`
  `None` olunca erken dönüyor ve kuşağı **artırmıyordu**, yani bekleyen tik
  hâlâ geçerli sayılıyor ve komut bittikten sonra bir kare fazla istiyordu.
- **Saat kademesinde metin dakikada bir değişiyor**, saat ise saniyede bir
  uyandırıyordu: bir saatlik komutta 3540 **aynı** kare. Tik artık kademenin
  çözünürlüğünde.
- **Sayaç sığmıyorken saat yine kuruluyordu** — dar pencerede uzun bir komut
  sonsuza kadar saniyede bir uyanıp hiçbir pikseli değiştirmezdi.
- **Kaybolan bir `D` bayat saat bırakıyordu** ve onu **sonraki** bloğun `D`'si
  tüketiyordu: anlık bir komut dakikalarca sürmüş görünürdü. `A` ikinci
  sıfırlama noktası oldu.

Kalanlar: boşluğun gerçek glyph olarak basılması (atlasta yuva + şeffaf
dörtlü), çakışma ölçütünün seçim vurgusunu ve geniş glyph'in ikinci yarısını
saymaması, aynı satırda iki çıpa savunması, `const` assert'in `BlockLog`'un
doc'unu yutması, `bt-gpu`'da `last_cursor`'ın kopyası olan gereksiz alan, bir
sınama yarışı ve üç belge tutarsızlığı (`CLAUDE.md`'nin `dim` rolü,
yol haritasının "iki katına" dediği bütçe).

## A. Otomatik doğrulanan

- `make hepsi` (fmt + denetim + clippy + test) — her phase'de ve kapıda **0**.
  **Bir kez kırmızı görüldü ve tekrarlanamadı:** kapı belgeleri yazılırken bir
  koşu `2` döndü, ardından dört `make hepsi` ve altı `cargo test --workspace`
  koşusu (ve zamana bağlı sınamaların beş hedefli koşusu) **hepsi yeşil**.
  Çıktısı tutulmadığı için nedeni bilinmiyor. Şüphe bu setin doğurduğu üç
  zamana bağlı sınamanın üstünde (`a_slow_command_…`, `the_clock_runs_…`,
  `a_counter_that_does_not_fit_…`): üçü de gerçek PTY açıp gerçek saat
  bekliyor. Kapı ileride bu adlardan biriyle kızarırsa **önce buraya
  bakılsın** — yeni bir kusur değil, bu kaydın devamı olabilir.
- Defterin girdi başına bütçesi `const` assert ile bağlı
  (`size_of::<Outcome>() == 12`), yani 120 KB rakamı yazılı değil
  **doğrulanmış**.
- Sınamalar: biçimin dört kademesi, en uzun metnin tampona sığması, koşan ile
  bitmiş sayacın çözünürlük ayrımı, eşiğin altında sayaç olmaması, sağa
  yaslanma ve `dim` rengi, çakışmada sayacın düşmesi, saatin komutla kurulup
  komutla sönmesi.

## B. Elle / komutla

| # | Şerit | Ne | Neden |
|---|---|---|---|
| 1 | `[komut]` | `make kur` | **Doğruluk için gerekmiyor** (kabuk betiği değişmedi), ama `.app`'te görmek için gerekiyor: açık pencereler eski binary'yle koşuyor |
| 2 | `[elle]` | `sleep 5` koştur | Sayaç 1. saniyede belirmeli, `1s 2s 3s 4s` diye ilerlemeli, bitince `5.0s` gibi bir değere oturmalı |
| 3 | `[elle]` | **En değerli kontrol.** `sleep 5` bittikten sonra pencereye dokunma | Kare istenmemeli — saatin **durma koşulu**. Kapıda düzeltilen kusurun (bekleyen tikin iptal edilmemesi) **tek tanığı bu**: sınamalar `next_tick`'in `None`'a düştüğünü kanıtlıyor ama `arm_clock`'ın gerçekten sustuğunu kanıtlayamıyor — o kod `bt-gpu`'da, ana thread'de ve koşum altyapısı yok. Şüphe varsa `BT_RUN_SECONDS` + `BT_FRAME_STATS` ile `icerik=` bak |
| 4 | `[elle]` | `ls` gibi hızlı bir komut | Hiç sayaç çıkmamalı (eşik) |
| 5 | `[elle]` | Pencereyi daraltıp uzun bir komut yaz | Sayaç kaybolmalı, komut metni **bozulmamalı** |
| 6 | `[elle]` | `[shell] integration = "blocks"` ile bir prompt | Bilinen sınır (aşağıda): çok satırlı prompt'ta sayaç komut satırında değil çıpanın **ilk** satırında |
| 7 | `[komut]` | `make duman` (ajanın kabuğunda yanlış tanıyla kırmızı düşüyor) | Jeton satırı değişmedi; `icerik` ve `sessiz` bugünküyle aynı olmalı — reçete `/bin/sh` koştuğu için saat hiç armed olmuyor |

## Bilinen sınırlar

- **Ölçülen şey komutun kendisi değil, `C`–`D` arası.** Kancalarımız
  `add-zsh-hook` ile sona ekleniyor, yani kullanıcının kendi kancaları
  ikisinden de önce koşuyor: `C` geç basılıyor (süre kısalır), `D` kullanıcının
  `precmd`'lerinden sonra basılıyor (uzar). Kendi işimiz payın **dışında** —
  `D` `precmd`'in ilk işi, dalın `git` fork'undan önce. Pay gösterilen 0,1
  saniyelik kademenin altında kalıyor; kancayı ikiye bölmek betiği değiştirir
  ve bu sette yapılmadı.
- **Çok satırlı prompt'ta satır kayması.** `Block::row` çıpanın **ilk** satırı
  (010'un semantiği), yani p10k gibi iki satırlı bir prompt'ta sayaç komutun
  değil üstteki segment satırının sağ ucuna düşüyor. Bu setin doğurduğu bir
  kusur değil, görünür kıldığı bir semantik.
- **1–1,3 saniye arası biten komut "sıçrıyor" gibi görünür ve bu tasarım.**
  Koşan sayaç tam saniye gösteriyor (`1s`), bitince onda bire oturuyor
  (`1.2s`); o dar aralıkta ikisi bir iki kare arayla görülüyor. Kusur değil
  **kesinleşme** — gerekçesi `Precision`'ın doc'unda. Hata diye bildirilmesin
  diye burada yazılı.
- **Geçmişe dönük süre yok.** Set indiğinde açık olan pencerelerin daha önce
  koşmuş komutları süresiz kalır — defterde yok, uydurulmuyor.
- **`QUIET_FLOOR` ile tik periyodu ilkesel olarak uyumsuz.** Kapı "son içerik
  karesinden sonraki sessizlik" ölçüyor (868 ms) ve saniyede bir kare isteyen
  bir yük onu altına düşürebilir. Bugün çarpışmıyorlar: duman reçetesi
  entegrasyonsuz, yani saat hiç armed olmuyor. Entegrasyonlu bir ölçüm yükü
  doğarsa kapının `saat=` jetonuyla saatin meşru karelerini ayırt etmesi
  gerekecek. **Bu sette düzeltilmedi, adlandırıldı.**

## Ölçüm bekleyen iddia

- "Saat armed'ken kare maliyeti": koşan komut boyunca saniyede bir kare. Kanca
  var (`BT_FRAME_STATS`), **yükü yok** — entegrasyonlu bir ölçüm yükü bugün
  tanımlı değil. `/measure` bu yükü doğurana kadar açık.

## Sonraki iş

- **Eşiği ayara bağlamak** (`command_duration_threshold`; referansta var,
  `docs/ARASTIRMA.md:98`). Bu set ayar eklemedi.
