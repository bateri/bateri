# Ölçüm kancaları — Tartışma

## Karar 1: Hangi kancalar bu sete girer?

> **→ ✅ onaylandı (panel, oybirliği).** Sadelik jürisi bir adım öteye gitti: bu kararın ölçütü ("`/measure` istiyor ama bekleyen iddia yok → YAGNI") Karar 3'ü de mahkûm ediyor. Aynı sayfada aynı ölçütü bir kez uygulayıp bir kez çiğnemişim.


`/measure` dört kanca adı biliyor: `BT_FRAME_LOG`, `BT_SCROLL_TEST`,
`BT_STARTUP_TRACE`, `BT_INPUT_LATENCY_SAMPLES`.

**Öneri: ilk üçü girer, `BT_INPUT_LATENCY_SAMPLES` girmez.** İki bağımsız
gerekçe, ikisi de `context.md`'de kanıtlı:

1. Bekleyen on iki iddianın **hiçbiri** gecikme iddiası değil. Sekizi kare
   süresi, ikisi açılış/ölçek, ikisi atlas doluluğu.
2. Zincirin orta halkaları **bu depoda değil**: `Session::write` yalnız
   `Msg::Input` kanalına gönderiyor (`session.rs:684-689`), gerçek fd
   `write()` ve okuyucu thread `alacritty_terminal::EventLoop` içinde.
   *"PTY yazıldı → echo okundu"* halkalarını ölçmek kapsüllediğimiz bir
   bağımlılığın içine girmek demek.

Karşı argüman: zincir sekiz halkalı ve halkaları sonradan enstrümante etmek
pahalı. Ama bu "şimdi gerekiyor" değil "sonra zor olur" argümanı; bu depoda
karşılığı YAGNI. Gecikme kancası, gecikme iddiası doğuran ilk sette gelir —
en olası adayı klavye/pano seti.

## Karar 2: `BT_SCROLL_TEST` neyi sürecek? Viewport kaydırma **yok**

> **→ ✅ yön doğru, mekanizma yanlış.** `smoke_shell` ikinci müşteri **olamaz**: nullary, tek `printf` + `sleep 10` (`session.rs:221-232`) ve kendi doc'u onu `hucre=8 glif=6 kural=15`'in **tek sahibi** ilan ediyor — ikinci bir yük o sahipliği böler. Ayrıca `run_seconds.is_some()` bugün üç şey birden demek: sabit shell (`app.rs:327`), deadline (`app.rs:203`), bekçi (`app.rs:399`). Yerine: yanına ayrı bir `load_shell(secs)` ve bu üç anlamı ayıran bir `Workload` seçimi.


`/measure` bunu "dolu scrollback kaydırma" diye tarif ediyor. Ama uygulamada
fare tekerleği işleyicisi yok, viewport kaydırma diye bir şey yok — yol
haritasına göre o 006/007'nin işi.

**Öneri: `BT_SCROLL_TEST` viewport'u değil *içeriği* kaydırsın.** Sabit bir
shell komutu uzun süre çıktı akıtır; her satır kirli düşer, grid yukarı
kayar, kare akışı doğal olarak sürer. Ölçtüğümüz şey zaten bu: dolu bir
karede parse + `frame()` + encode + GPU maliyeti.

Deseni hazır: `smoke_shell()` (`session.rs:221`) tam bu işi yapıyor — sabit
komut, kullanıcının `$SHELL`'ine bağlı olmayan sonuç. İkinci müşterisi bu.

Bu, kancayı viewport kaydırma geldiğinde **değiştirmeden** bırakır mı? Hayır
— o zaman ikinci bir yük profili gerekir. Ama o gün gelince eklenir; bugün
olmayan bir yolu ölçmek sıfır döndürür ve `/measure` bunu açıkça yasaklıyor
("boşta duran pencerede kare süresi ölçmek … sessiz yanlış sayı üretir").

## Karar 3: Zamanı nereden okuyacağız?

> **→ ⚠️ revize: üç kaynak değil iki.** Sunum kaynağı (düşen kare) düştü — on iki iddianın **hiçbiri** düşen kareden söz etmiyor, yani Karar 1'in ölçütü bunu da eler. Maliyeti iki damga okumak da değil: "düşen kare" sayısına çevirmek kare-arası defter tutmak demek ve Karar 2'nin akıtma yükünde duraklamış link **normal** hâl, yani sayı anlamsızlaşır.
>
> Buna karşılık CPU tarafı **ikiye ayrılıyor**: `session.frame()` çevresi ve `draw` çevresi ayrı ölçülür. Tek aralık 002 #1'i (`FairMutex::lock()` beklemesi) ayıramıyordu; ayırınca o iddia da kapanıyor ve bedeli tek bir fazladan `Instant::now()`.


Üç kaynak var ve **üçü farklı şeyi ölçüyor**:

- **CPU tarafı** — `frame()` + `draw` arası, `Instant` ile. Bugün üretim
  kodunda `Instant` hiç yok (`context.md`).
- **GPU tarafı** — `GPUStartTime`/`GPUEndTime`, tamamlanma bloğunda. Blok
  **zaten var** (`renderer.rs:321-341`) ve bugün yalnız `status()`/`error()`
  okuyup `frames++` yapıyor. Kanca noktası hazır.
- **Sunum tarafı** — display link'in target/actual timestamp'i
  (`link.rs:289`, `update` bugün yalnız `drawable()` için kullanılıyor).
  Düşen kareyi **ancak bu** görür: GPU hatasız bitirmiş ama kare vaktine
  yetişmemiş olabilir.

**Öneri: üçü de.** `/measure` zaten "GPU/CPU ms ve düşen kare" istiyor ve
düşen kare üçüncüsü olmadan sayılamaz. Üçü ayrı sütun; tek bir "kare süresi"
sayısına indirilmez.

## Karar 4: Çıktı nereye, hangi biçimde?

> **→ ❌ düştü. Dosya yok.** Üç ayrı koldan çürüdü:
>
> 1. **Gerekçem yanlıştı.** "Makefile jeton satırını grep'liyor" yazmışım; grep'lemiyor (`Makefile:29-30` yalnız `cargo run`, kapı çıkış kodu — `app.rs:442`/`:452`). Jeton sözleşmesi zaten **eklemeli**, çakışma yalnız kare başına satırı stdout'a basarsan doğar ki kimse istemiyor.
> 2. **Kendi Karar 5'imle çelişiyordu.** Kare başına dosya yazımı, Metal'in thread'inde **başarı yolundaki ilk kilit** olurdu (bugün orada yalnız atomik var: `renderer.rs:325`, `:337`). Tamponlayıp sonda dökersem tampon zaten p95'i hesaplayabiliyor — yani dosya saf fazlalık.
> 3. **Tüketici dosyayı işleyemiyor.** `/measure`'ın `allowed-tools`'unda `awk`, `python`, `cut` yok; yalnız `sort` ve `wc`. Tek jeton satırı bir `Read`.
>
> Yerine: istatistik süreç içinde hesaplanır, mevcut jeton satırı genişler (`app.rs:441`). Jeton kuralı aynen geçerli — **silinmez, eklenir**.


`make duman`'ın jeton satırı `println!` ile stdout'a gidiyor
(`app.rs:441`) ve Makefile onu **grep'liyor**. Kare kaydı da stdout'a
basılırsa jeton satırını gürültüye gömer.

**Öneri: `BT_FRAME_LOG` bir dosya yolu alsın** (`BT_FRAME_LOG=/tmp/x.tsv`),
bayrak değil. Üç kazancı var: duman jetonuyla çakışmaz, `/measure`'ın
istediği dağılım hesabı (p95, en kötü kare) dosyadan yapılır, ve kaydın
nereye gittiği kullanıcının kararı olur.

Biçim: sekmeyle ayrılmış, başlık satırlı. Jeton sözleşmesinin aynısı geçerli
— **sütun silinmez, eklenir**.

## Karar 5: Enstrümantasyon her zaman açık mı, kapılı mı?

> **→ ✅ onaylandı, üç jüri de temiz buldu.** Yakalanmış bir `Option` üstünde dallanma bedava; `BT_RUN_SECONDS`'ın deseni birebir oturuyor. Tek ekleme: açılış izi için `Options` bool değil **`Instant`** taşımalı ve damga `main.rs`'te, `Renderer::system_default()`'tan (`lib.rs:43`) **önce** alınmalı — yoksa ölçülen şey açılışın kendisini kaçırır.


Bugünkü örüntü net (`context.md`): sayaçlar **koşulsuz** ve ucuz (tek
`Relaxed` atomik), env yalnız **dallanma** yaratıyor ve tek yerde okunuyor
(`main.rs:15-24`), tipli `Option` olarak taşınıyor.

**Öneri: aynı örüntü.** Zaman okuma ve dosyaya yazma env kapılı; kapı
kapalıyken hiçbir `Instant::now()` çağrılmaz. Kancalar **kenarda bir kez**
okunur ve `Options`'a tipli alan olarak girer; kodun derinine `env::var`
saçılmaz.

Bu bir zevk değil ölçümün doğruluğu meselesi: ölçüm kodunun kendisi ölçülen
yola girerse sayı kendi gözlemcisini ölçer.

## Karar 6: `boşta sıfır kare` nasıl korunacak?

> **→ ✅ niyet doğru, bekçi zaten var.** Yeni bir harness gerekmiyor ve kurulamaz da: "boşta kare sıfır kaldı" iddiası gerçek bir display link ister, `Gate`/`FailureStreak` ise tam da bunun dışında kalsın diye ayrı tipler (`link.rs:109`, `:191`). Ama `make duman` **zaten boşta bir pencere**: `smoke_shell` bir kez basıp `sleep 10` yapıyor, `BT_RUN_SECONDS=3` ise ~3 saniye boşta geçiyor ve çıktı `kare=1`. Bekçi bu — kapı bugün yalnız `n > 0` (`app.rs:440`); kancalar boşta sıfır kareyi bozarsa `kare` 1'den ~180'e fırlar. Plana gereksinim olarak **üst sınır** yazılır.


`context.md` bunu bozmanın üç yolunu `link.rs`'ten okuyup listeledi: timer'dan
`wake()`, tamamlanma bloğunun `Ok` kolundan yeniden kare istemek, ve
`Session::frame`'i ikinci bir tüketicinin çağırıp `dirty` bayrağını çalması.

**Öneri: üçü de plan.md'ye gereksinim olarak yazılsın ve bir bekçi sınamayla
bağlansın.** Kancalar **kapalıyken** boşta kare sayısının sıfır kaldığını
gösteren bir sınama; belirti sessiz olduğu için (uygulama çalışır, pil gider)
gözle yakalanmaz.

## Karar 7: `cargo bench` ve logger bu sete girer mi?

> **→ ✅ ikisi de dışarıda, ama kapsam cümlesi dürüstleşmeli.** İşletme jürisi setin kendi gerekçesini vurdu: bench dışarıda kalınca 003 #1 ve #2 **kapanmıyor**, yani `/measure 003` yine "ölçüm aracı yok" diyecek. Bu kozmetik değil — `CLAUDE.md`'nin "kancalar gelince `cargo bench` satırı geri gelir" sözü **silinmez, yeniden yazılır** ("bench seti bekliyor"). Set bir borç cümlesini başka bir borç cümlesiyle takas ediyor ve bunu söylemek zorunda.


**Bench — öneri: girmesin.** `CLAUDE.md` "kancalar gelince `cargo bench`
satırı geri gelir" diyor ama bench ayrı bir şey ölçüyor: `bt-core` parse ve
`bt-atlas` mikro-bench'leri gerçek pencere istemiyor, dolayısıyla bu setin
riskini (boşta kare, render yolu) hiç paylaşmıyorlar. Ayrıca `criterion` yeni
bir bağımlılık, yani mimari karar. Ayrı ve küçük bir sete daha uygun.

**Logger (`tracing`) — öneri: girmesin, ama gerekçesi zayıf.** Yol haritası
bunu 005'in doğal adayı olarak işaretledi çünkü aynı enstrümantasyon
damarından geçiyor. Karşı argüman: logger bir **bağımlılık kararı** (`tracing`
+ bir subscriber) ve bu set zaten üç kanca + yeni bir belge taşıyor. İkisini
birleştirmek seti "ölçüm + gözlemlenebilirlik" diye şişirir.

Panel bu ikisine özellikle baksın: ayırmak mı doğru, yoksa logger'sız ölçüm
kodu kendi hatalarını mı yutar?

## Karar 8: `docs/OLCUMLER.md` gürültü eşiğini nasıl tanımlar?

> **→ ❌ düştü. Dosyayı bu set kurmuyor.** `/measure` skill'i zaten "dosya yoksa ilk ölçüm onu kurar" diyor; yöntemi ölçüm koşmadan yazmak, eşiği ölçmeden yazmakla aynı hataya bir adım kalıyor.
>
> Ama işletme jürisinin yakaladığı tuzak gerçek: `make duman` **debug** koşuyor, `/measure` **release** şart koşuyor. Yöntem dosyası bugün yazılmayacaksa bu uyarı kaybolur. Çözüm ikisinden de iyi: jeton satırı `profil=` taşısın (`cfg!(debug_assertions)`, tek okuma). O zaman debug sayısını release sanmak **imkânsız** olur — belgeye güvenmek yerine sayının kendisi profilini söyler.


`/measure` bu dosyanın `## Yöntem` bölümünü **okumakla başlıyor** ve dosya
yoksa ilk ölçümün onu kurmasını şart koşuyor: *"yöntemsiz sayı sonraki
ölçümle karşılaştırılamaz."*

**Öneri: bu set dosyayı sayısız kursun.** `## Yöntem` ve `## Nasıl yeniden
ölçülür` yazılır; sayı bölümleri **boş başlık olarak bile** açılmaz — ilk
gerçek `/measure` koşusu onları doldurur. Gürültü eşiği ölçülmeden
yazılamaz, yani eşiğin kendisi ilk koşunun çıktısıdır: yöntem "eşik şu
yordamla belirlenir" der, sayıyı vermez.

## Karar 9: Atlas doluluğu — **panelin bulduğu eksik karar**

İki jüri bağımsız olarak aynı boşluğu gösterdi: `context.md` atlas doluluğunu
(003 #3, 004 #2) *"zamanlama değil, **sayaç** — `occupancy()` zaten var"* diye
ayırmış, ama `discussion.md`'nin sekiz kararından **hiçbiri** ona araç
atamamış. Context ile discussion arasında düşmüş.

Ucuz da değil, **en ucuz** kapanış: `Atlas::occupancy()`
(`bt-atlas/src/lib.rs:316`) zaten var ve bugün yalnız `#[cfg(test)]`
altında okunuyor — dokuz kullanımın dokuzu da sınama.

Tek engel katman: **`bt-shell`'in `bt-atlas` kenarı yok**
(`crates/bt-shell/Cargo.toml` yalnız `bt-gpu` ve `bt-core`). Yani jeton
satırını basan taraf doluluğu doğrudan okuyamaz.

**Karar: `bt-gpu` üzerinden yeniden yayımlansın.** Deseni hazır ve
kanıtlanmış — `Renderer::cell_metrics` (`renderer.rs:253`) tam bunu yapıyor:
`bt-atlas`'tan gelen bir değeri `bt-shell`'e katman yönünü bozmadan taşıyor.
Jeton: `yuva=U/T`.

Bu iki iddiayı kapatıyor ve maliyeti bir erişimci.

## Muhakeme (2026-09-11)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de `SORUNLU`, hiçbiri `KIRMIZI`: çekirdek yaklaşım (env kapılı zamanlama,
var olan kanca noktaları, sabit-shell yükü, boşta-sıfır bekçisi) ayakta;
itirazlar parçalara.

**Kabul edilen itirazlar → plan değişikliği:**

- Karar 4'ün TSV dosyası, kendi Karar 5'imle çelişiyor ve tüketicisi
  (`/measure`, `allowed-tools`: `sort`+`wc`, awk/python yok) onu işleyemiyor
  → **dosya tümden düştü**; istatistik süreç içinde hesaplanıp mevcut jeton
  satırına eklenir.
- Karar 4'ün yazılı gerekçesi olgu olarak yanlıştı (`Makefile` grep'lemiyor;
  kapı çıkış kodu) → düzeltildi, karar zaten düştü.
- Karar 3'ün sunum kaynağı, Karar 1'in kendi ölçütüne takılıyor (hiçbir
  bekleyen iddia düşen kareden söz etmiyor) → **üç kaynak ikiye indi**.
- Tek CPU aralığı 002 #1'i (`FairMutex::lock()` beklemesi) ayıramıyor
  → CPU **ikiye ayrıldı** (`session.frame()` ve `draw` ayrı), bedeli tek bir
  fazladan damga, kazancı bir iddianın daha kapanması.
- `smoke_shell` ikinci müşteri olamaz (nullary, tek atış + `sleep 10`, ve
  `8/6/15`'in tek sahibi); `run_seconds.is_some()` bugün üç anlam taşıyor
  → ayrı `load_shell(secs)` + `Workload` seçimi.
- Tamamlanma bloğu kare başına **kurulmuyor** ve doc'u bunu yasaklıyor
  (`renderer.rs:318-323`: "her kurulum bir heap ayırması") → GPU damgası
  closure ile yakalanamaz, **paylaşılan slot** gerekir.
- `process::exit(0)` `Drop` koşturmaz, bekçinin `_exit(70)`'i atexit'i bile
  atlar (`lib.rs:79-87`) → çıktı `report_and_exit` içinde **açıkça** yazılır,
  `Drop`'a güvenen hiçbir yol kullanılmaz.
- Örtülen pencere kapıyı kapatır ve örnekleme **sessizce durur**
  (`link.rs:262`, `app.rs:271-281`); üç örnek üstünden p95 *iyi* görünür
  → jeton satırı **örnek sayısı** taşır (`ornek=`), `/measure` sayı
  hesaplamadan önce ona bakar.
- Açılış damgası `Renderer::system_default()`'tan (`lib.rs:43`) sonra
  alınırsa açılışın kendisini kaçırır → `Options` bool değil `Instant` taşır,
  damga `main.rs`'te en başta alınır.
- Boşta-sıfır bekçisi `cargo test` olarak kurulamaz (gerçek display link
  ister) ama **zaten var**: `make duman` boşta bir pencere ve `kare=1` basıyor
  → kapı `n > 0`'dan **üst sınıra** çevrilir.
- Atlas doluluğuna hiçbir karar araç atamamış → **Karar 9** eklendi.
- Bench dışarıda kalınca üç `/measure` kutusu da `[x]` olamıyor → kapsam
  cümlesi dürüstleşir ve `CLAUDE.md`'nin bench sözü **silinmez, yeniden
  yazılır**.

**Reddedilenler:**

- *"TSV'de kapı-kapalı işareti, commit taşıyan başlık, açılışta truncate"*
  (işletme) — dosya tümden düştüğü için konusuz kaldı. Arkasındaki gerçek
  kaygı (bayat sayı okunması) `ornek=` jetonuyla ve dosyasızlıkla çözülüyor:
  kalıcı dosya yoksa bayatlayacak dosya da yok.
- *"`docs/OLCUMLER.md`'nin yöntem bölümü `cargo run --release`'i çivilemeli"*
  (işletme) — dosya bu sette yazılmıyor (Karar 8 düştü), ama tuzak gerçek:
  `make duman` debug koşuyor. Belgeye güvenmek yerine **`profil=` jetonu**
  eklendi; sayının kendisi hangi profilden geldiğini söylüyor. Jürinin
  kaygısı kabul, çözümü red.
- *"Bench'i kapsama al"* — zaten hiçbir jüri önermedi; işletme jürisi açıkça
  *"bench'i kapsama almayı önermiyorum"* dedi. `criterion` yeni bir bağımlılık,
  yani mimari karar; ayrı ve küçük bir sete ait.
- Sadelik jürisinin *"tek env: `BT_FRAME_STATS=1`"* sadeleştirmesi **kısmen**
  kabul: kanca sayısı üçten ikiye iniyor (açılış izi ayrı bayrak değil, aynı
  bayrağın altında tek sayı), ama yük seçimi ayrı kalıyor — çünkü 006/007
  viewport kaydırmayı getirdiğinde ikinci bir profil gerekecek ve o gün
  bayrağı bölmek, bugün ayrı tutmaktan pahalı.

## Karar Noktaları

Panelden önce buraya *"kullanıcıya sorulacak tek şey Karar 1"* yazılmıştı.
Panel onu oybirliğiyle onayladı, dolayısıyla **açık soru kalmadı**; geriye
kullanıcının kapsamı onaylaması kaldı.

Onaylanacak kapsam, panelden sonraki hâliyle:

- **İki zaman kaynağı** (CPU ve GPU), CPU ikiye ayrık — düşen kare yok.
- **İki yük profili değil, bir tane**: `load_shell(secs)`, `smoke_shell`'in
  yanında ve ondan ayrı.
- **Dosya yok**: istatistik süreç içinde hesaplanır, `make duman`'ın jeton
  satırı genişler (`ornek=`, `profil=`, `yuva=U/T` dâhil).
- **`docs/OLCUMLER.md` bu sette yazılmaz** — ilk `/measure` kurar.
- **Bench ve logger dışarıda**, ve bunun bedeli açıkça yazılı: üç `/measure`
  kutusu da `[x]` olmayacak.
- **Boşta sıfır kare bekçisi yeni bir harness değil**: `make duman`'ın
  `kare=1`'ine üst sınır.
