# Ölçüm kancaları — Tartışma

## Karar 1: Hangi kancalar bu sete girer?

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

`context.md` bunu bozmanın üç yolunu `link.rs`'ten okuyup listeledi: timer'dan
`wake()`, tamamlanma bloğunun `Ok` kolundan yeniden kare istemek, ve
`Session::frame`'i ikinci bir tüketicinin çağırıp `dirty` bayrağını çalması.

**Öneri: üçü de plan.md'ye gereksinim olarak yazılsın ve bir bekçi sınamayla
bağlansın.** Kancalar **kapalıyken** boşta kare sayısının sıfır kaldığını
gösteren bir sınama; belirti sessiz olduğu için (uygulama çalışır, pil gider)
gözle yakalanmaz.

## Karar 7: `cargo bench` ve logger bu sete girer mi?

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

`/measure` bu dosyanın `## Yöntem` bölümünü **okumakla başlıyor** ve dosya
yoksa ilk ölçümün onu kurmasını şart koşuyor: *"yöntemsiz sayı sonraki
ölçümle karşılaştırılamaz."*

**Öneri: bu set dosyayı sayısız kursun.** `## Yöntem` ve `## Nasıl yeniden
ölçülür` yazılır; sayı bölümleri **boş başlık olarak bile** açılmaz — ilk
gerçek `/measure` koşusu onları doldurur. Gürültü eşiği ölçülmeden
yazılamaz, yani eşiğin kendisi ilk koşunun çıktısıdır: yöntem "eşik şu
yordamla belirlenir" der, sayıyı vermez.

## Karar Noktaları

Kullanıcıya sorulacak tek şey **Karar 1**: gecikme zinciri bu sete girsin mi?
Önerim hayır; gerekçesi iki bağımsız kanıt. Kalan yedi karar teknik ve
panelin işi.
