# Hareket altyapısı ve imleç hareketi — Tartışma

Sekiz karar noktası; ilk ikisi devralınan borcun (boşta kare kapısı), üçüncü ve
dördüncü kare yolunun, kalanı ürün yüzeyinin. Çözülenler başlığa `→ ✅` ile
işlenir.

## Karar 1: Kapı yavaş bir animasyonu nasıl yakalar?

Bugünkü kapı `kare ≤ 8` ve üç saniyelik koşuda ancak **3 Hz**'lik bir sızıntıyı
görüyor (`context.md` → Kanıt). Aday çözümler:

**A) Sessiz kuyruk (süre).** Koşunun son `T` saniyesinde çizilen kare sayısı
ayrı bir sayaca yazılır (`kuyruk=`) ve duman yükünde **sıfır** olmak zorundadır.
Mekanizma: deadline'ın yanına ikinci bir `performSelector:afterDelay:` kurulur,
`seconds - T` anında `Renderer::frames()` okunur; deadline'da fark alınır. Kare
başına ek iş yok, saat okuması yok (`BT_FRAME_STATS` kapısına dokunmuyor).
Periyodu `T`'den kısa **her** sızıntıyı yakalar — mekanizması ne olursa olsun
(animasyon, yanlış kurulmuş timer, sızdıran `wake`).

**B) Durum jetonu (mekanizma).** Deadline anında hareket saati "yerleşti mi"
diye sorulur; yerleşmemişse kırmızı (`hareket=running`). Hızdan **bağımsız**:
periyot ne olursa olsun, durma koşulu olmayan animasyon yakalanır. Sınırı:
yalnız hareket altyapısından geçen animasyonları görür; `Waker`'ı doğrudan
çağıran bir timer'ı görmez.

**C) `istek=` oranı.** `IDLE_FRAME_LIMIT` doc'unun eski aday çözümü. Bugün
dayanaksız: ölçüm yükünde `istek` ile `kare` mertebelerce ayrışıyor ve
mekanizması ölçülmedi; eşik **ölçülmedi** ve ölçülmemiş sayı kapıya yazılmaz.

**D) Sınırı düşürmek.** Sağlıklı dağılım `1–2` olduğu için `kare ≤ 4` mümkün
görünüyor ama ilk animasyonla birlikte meşru kare sayısı zaten yükselecek;
düşürmek bu setin kendi işini kırmızıya çevirirdi.

**Öneri: A + B birlikte.** İkisi ortogonal — A hızı olan her sızıntıyı
mekanizmadan bağımsız, B mekanizması belli her sızıntıyı hızdan bağımsız
yakalar. A borcu **ilk animasyondan önce** kapatır (bugünkü kodla ölçülebilir),
B animasyon altyapısıyla birlikte gelir. C ve D reddedilir.

`T`'nin sayısı **ölçülecek**: sağlıklı koşuda son karenin ne zaman düştüğü
bilinmiyor (`acilis=` tabanı da yok). Ölçüm phase'i önce `BT_FRAME_STATS=1` ile
açılış dağılımını alır, `T` onun üstüne seçilir ve gerekçesi sabitin doc'una
yazılır. Sabit ve kapı, kod phase'lerinden **ayrı** commit'le iner
(`proje.md` → `IDLE_FRAME_LIMIT` kuralının aynısı).

## Karar 2: Animasyon kareleri hangi sayaca yazılır?

200 ms'lik bir imleç kayması 120 Hz'de ~24 kare eder; `kare ≤ 8` kapısı kod
doğruyken kırmızı düşer.

**A) Sınırı yükselt** (ör. 64). Algılama tabanını daha da yukarı iter — borcun
sebebi zaten buydu.

**B) Muhasebeyi ayır.** Hareket yüzünden çizilen kare (grid kirli **değil**,
hareket yerleşmemiş) ayrı sayılır: `hareket=N`. Kapı `kare − hareket ≤
IDLE_FRAME_LIMIT` olur; sınır **değişmez**, anlamı da değişmez ("boştaki içerik
karesi"). `kuyruk=` ikisini birden kapsar: kuyrukta ne içerik ne hareket karesi
olmalı.

**Öneri: B.** Bir uyarı `Measured::read`'in emsalinden geliyor: `kare` GPU'nun
tamamlanma bloğunda, `hareket` CPU'da sayılıyor — deadline'da **bir karelik**
kayma olabilir; kapı bunu payla kurar ve jetonun doc'u söyler.

## Karar 3: İmlecin altındaki metin nerede tersine döner?

Bugün `bt-core`: imlecin durduğu hücrenin ön planı temanın zeminine çevriliyor,
alt çizgi rengi düşürülüyor (003/004 R4.1: "kararın adı terminal
semantiğidir"). Alt hücre kayması bu kararı açıyor: blok iki hücre arasındayken
hedef hücrenin harfi zemin renginde, yani **görünmez**; kaynak hücrenin harfi
bloğun üstünde kendi renginde kalıyor.

**A) Bugünkü yerinde kalsın.** Kayma süresince (≈150–200 ms) yukarıdaki iki
kusur görünür. Harfin üstünden geçen imleç (ok tuşları, vim, satır editörü) tam
olarak bu duruma düşüyor.

**B) Piksel işi shader'a insin.** İmlecin **piksel** dikdörtgeni ve "blok
altındaki metin rengi" uniform olarak `cell` pipeline'ına geçer; fragment
dikdörtgenin içindeyse o rengi kullanır. Semantik karar `bt-core`'da kalır
(imleç altını tersine çevirir ve rengi temanın zemini) ve sınırdan `Cursor` ile
geçer; `bt-gpu` yalnız "bu dikdörtgenin içindeki fragment şu renk" der — kural
çizgileri de aynı pipeline'dan geçtiği için bedavaya doğru davranır. Yarım
örtülen hücre piksel piksel bölünür, yani blok metnin **üstünden geçer**.
Bedeli: `.metal` + `#[repr(C)]` uniform (riskli phase, `make shader`), ve
`frame()`'in "imleç hücresini ters çevir" dalı ile ona bağlı iki sınamanın
taşınması.

**Öneri: B, ve hareketten önceki ayrı bir phase'te** — o phase'in görsel
sonucu **birebir bugünküyle aynı** olmalı ve bunu offscreen sınama kanıtlar;
böylece hareket geldiğinde değişen tek şey dikdörtgenin nereye düştüğü olur.
Sonraki hareket stilleri (Smear, Squash) de dikdörtgeni şekle çevirince aynı
yoldan gider.

## Karar 4: Hareket karesinin içeriği nereden gelir?

Grid kirli değilken de kare çizilecek; drawable içeriği korunmuyor, yani o
karede bütün liste yeniden gerekiyor.

**A) Grid'i yeniden tara.** `Session::frame`'in hasar bayrağını yoksayan bir
kardeşi. Animasyon boyunca her karede `Term` kilidi alınır ve tam grid taranır.

**B) Son `Frame`'i yeniden kullan.** Liste zaten ivar'da yaşıyor. Hareket
karesinde `bg.truncate(bg_count)` ile imleç dikdörtgeni atılır, yenisi yeni
konumla eklenir; glyph ve kural listelerine dokunulmaz. `Term` kilidi hiç
alınmaz. Karar 3'ün B'siyle birlikte doğru: harflerin rengi imleç konumuna
bağlı olmaktan çıktığı için listeyi yeniden kurmaya gerek kalmıyor. Temanın
accent/zemin rengi `frame()` ile birlikte okunuyor, yanına saklanır.

**Öneri: B.** A'nın bedeli tam da "render yolu bloklanmaz" ile kavga eden yer:
saniyede 120 kez PTY okuyucusunun kilidine girmek, hem de hiçbir şey değişmemiş
olabileceğini bilerek.

## Karar 5: Hangi imleç hareketi animasyonlu, hangisi snap?

Animasyonun "durma koşulu" yalnız fizikte değil, **hangi olayın animasyon
saymadığında** da yaşıyor. Öneri tek cümlede: **imlecin kendi hareketi
animasyonlu, altındaki dünyanın kayması snap.**

- **Animasyonlu:** yazarken/silerken kolon değişimi, satır başına dönüş, TUI'nin
  imleci taşıması, büyük sıçramalar dahil (ayrı bir "mesafe eşiği" yok — eşik
  ölçülmemiş bir sayı olurdu).
- **Snap (anında):** ilk kare; `visible` kapalıyken açılan imleç (TUI'ler
  çizerken imleci gizliyor); geometri değişimi (pencere boyutu, font, zoom →
  hücre ölçüsü değişti); geçmişte kaydırma (`scroll_wheel`, viewport kaydı).
  Ortak gerekçe: bunlarda imleç hareket etmedi, **altındaki ızgara** hareket
  etti.
- **Durum hücre biriminde tutulur**, pikselde değil: font/zoom değişimi
  konumu piksel olarak oynatır ama hücre koordinatı aynı kalır.
- **Spring'in durma koşulu iki katlı:** konum+hız epsilon'u **ve** süre tavanı.
  Tavan tam da kapının yakalaması gereken sızıntı sınıfına karşı kemer; sayısı
  seçilmiş bir üst sınır, ölçülmüş bir değer değil ve doc'u bunu söyler.

## Karar 6: Ayar anahtarları, değerler, varsayılan

Referansın anahtar adları (`docs/ARASTIRMA.md`): `[motion] cursor_motion`,
`reduce_motion` (+ bu sette **olmayan** `intensity`, `duration`, `keypress`,
`delete_mode`, `feed_lift`, `scroll.smooth`).

**Öneri:**

```toml
[motion]
cursor_motion = "spring"   # "snap" | "ease" | "spring"
reduce_motion = "system"   # "system" | "on" | "off"
```

- Enum `bt-core`'un ayar modelinde (`FontOptions`/`Osc52` emsali); `bt-gpu`
  tüketir, `bt-shell` uygular. Kabul edilmeyen değer kendi anahtarını
  değiştirmez ve bir `Diagnostic` bırakır (`osc52`'nin "kapalıya düş"
  istisnası **buraya geçmez**: yanlış tahminin bedeli görünür bir animasyon,
  sessiz bir pano sızıntısı değil).
- `reduce_motion` üç değerli bir dizgi, `bool` değil: "sistemi izle" en olası
  seçim ve `bool`'da onu ifade etmenin tek yolu anahtarı **silmek** olurdu —
  oysa bu dosyada anahtar silinmez.
- Varsayılan `"spring"`: setin ürün gerekçesi "Metalterm'i ekranda tanıtan üç
  şeyden biri" ve varsayılanı `"snap"` yapmak özelliği kapalı sevk etmek
  olurdu. Aşırıya kaçmaması fizikle sınırlanır (kritik sönümlemeye yakın, taşma
  yok).
- Menü yüzeyi **yok**: View ▸ Theme ▸ emsali burada tekrarlanmıyor (kapsam).

## Karar 7: Reduce Motion ne demek, nereden okunur?

`CLAUDE.md` sözleşmeyi zaten yazıyor: "`reduce_motion` ve sistemin Reduce
Motion ayarı her animasyonu 90 ms'lik solmaya indirir."

**Öneri:**

- **Kaynak iki, sonuç tek:** `[motion] reduce_motion = "system"` iken
  `NSWorkspace::accessibilityDisplayShouldReduceMotion`; `"on"`/`"off"` sistemi
  ezer. Sistem ayarı canlı izlenir
  (`NSWorkspaceAccessibilityDisplayOptionsDidChangeNotification`), tıpkı
  açık/koyu görünüm gibi.
- **Solmanın biçimi:** imleç yeni konumda 90 ms içinde **belirir**; eski
  konumda iz bırakmaz. Çapraz solma (iki dikdörtgen birden) uniform'a ikinci
  bir dikdörtgen ve alfa karışımı ekler, kazancı görünmez.
- **Hermetiklik (007 Karar 1'in aynısı):** süreli koşu ne `[motion]`'ı ne
  sistemin erişilebilirlik ayarını okur; ikisi de `AppDelegate::inputs()`'tan
  geçer. Yoksa `hareket=` jetonu ölçen makinenin erişilebilirlik ayarına
  bağlanırdı.

## Karar 8: Duman reçetesi animasyonu sınasın mı?

Bugünkü reçete bir satır basıp uyuyor; imleç yalnız satır başına iniyor ve ilk
karenin çıktıdan önce mi sonra mı düştüğü koşudan koşuya değişiyor
(`kare=1↔2`). Yani kapı, animasyonun **koştuğunu** da **durduğunu** da
göremeyebilir.

**Öneri:** reçeteye kısa bir uykudan sonra imleci kıpırdatan tek bir dizi
eklenir (ör. `\033[H`). Sayaçlar değişmez (`hucre=8 glif=6 kural=15`: imleç
`bg_count`'a girmiyor, harf sayısı aynı), ama artık her koşuda bir imleç
hareketi var: `hareket > 0` kapının **gerekli sayacı** olur ve `kuyruk = 0` o
hareketin durduğunu kanıtlar.

Bedeli: sağlıklı dağılım değişir → `docs/OLCUMLER.md`'ye yeni ölçüm girişi ve
`IDLE_FRAME_LIMIT`'in yeniden ölçümü. Bu zaten borçtu (sabitin doc'u: "kare
yolunu değiştiren bir set geldiğinde"), ölçüm gerçek pencere istiyor ve ayrı
commit kuralına tabi → **ayrı ölçüm phase'i**.

## Kapsam dışı (kayıt için)

İmleç şekilleri (beam/underline, DECSCUSR), blink (ilk **süresiz** animasyon
olurdu; boşta sıfır kareyle barışması ayrı bir karar: kitty'nin
"N saniye sonra dur" kolu), Smear/Squash/Phosphor/Arc, `intensity`/`duration`
çarpanları, yumuşak kaydırma ve `feed_lift` (yol haritası: "sete sığmazsa hemen
ardından"), yazma/silme animasyonları (013–014), odak kaybında içi boş imleç.

## Muhakeme (2026-09-16)

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de yönü onayladı (katman kuralıyla kavga yok), üçü de **hacme ve
mekanizmaya** itiraz etti. Kabul edilenler aşağıda; hepsi plana işlendi.

**Kabul edilen itirazlar → plan değişikliği:**

- **Durum jetonu düşen koşuda basılamaz.** `token_line()` yalnız
  `Verdict::Pass` dalında basılıyor (`app.rs`), yani `hareket=running` hiçbir
  zaman görünmezdi. → Yerleşme kontrolü jeton değil **karar** olur:
  `verdict()`'e yeni bir kol (`MotionUnsettled`). `hareket=N` sayaç olarak
  kalır. (Sadelik 1)
- **Çıkarma yok, tek sayaç var.** `kare` GPU'nun tamamlanma bloğunda, hareket
  sayacı ana thread'de artıyor; `kare − hareket` hem pay ister hem `u64`
  sarmasına açık (deadline animasyonun ortasına düşerse `hareket > kare`). →
  Kapı `needs_update`'te aynı noktada artan **`icerik=`** sayacına bakar:
  `icerik ≤ IDLE_FRAME_LIMIT`. `kare=` GPU sayacı olarak değişmeden basılır.
  Yan kazanç: `icerik ≤ kare` olduğu için bu değişiklik yanlış pozitif
  üretemez, yani **ölçüm oturumu beklemez**. (Sadelik 2, Codebase-fit 2,
  İşletme 3)
- **Hareket karesi `Waker`'a dokunmaz.** `wake()` hasarı koşulsuz dikiyor;
  hareket oradan kare isteseydi kendi karesini "içerik" diye saydırırdı. →
  Hareket, `needs_update`'in `None` dalını genişletir ("hareket yerleşmediyse
  uyuma"); link zaten uyanıkken kendini sürdürür. `link.rs`'in iki sözleşme
  cümlesi (modül başlığı, `Waker` = "kare istemenin tek tanımı") aynı
  commit'te güncellenir; ölçülmüş `istek ≈ kare + 2` ilişkisi de öyle.
  (İşletme 1)
- **`Frame` hareket karesinde boş olurdu.** `frame.clear()` koşulsuz ve
  `session.frame()`'den önce çağrılıyor; hareket karesi tam da `None` dalı. →
  `clear` yalnız içerik karesine alınır; hasar sorusu `frame()`'in içinden
  çıkar (`Session`'a "hasarı tüket" ucu). `Frame::push_cursor` ara konum için
  `f32` alır. İki sözleşme cümlesi (`frame.rs`, `link.rs`) aynı commit'te.
  (Codebase-fit 1)
- **Süreli kapı ölçüm borcuyla geliyor, kapı olarak inmez.** `kuyruk=0`'ın üç
  kusuru kanıtlandı: rapor yolunun ikinci girişi (shell erken çıkarsa anlık
  görüntü hiç alınmaz → sessiz yeşil), örtülme/geometri karelerinin kuyruğa
  düşmesi (kayıtlı yanlış pozitif, `IDLE_FRAME_LIMIT` doc'u) ve `T`'nin
  koşudan **önce** seçilmek zorunda olması. → Süreli yarı `sessiz=` **sayacı**
  olarak iner (deadline ile son kare arasındaki süre, kare başına tek atomik
  damga); eşiği ölçülene kadar kapı değil. "Ölçülmemiş sayı kapıya yazılmaz"
  kuralının `yuva=`/`istek=` ile aynı uygulaması. Borcu kapatan asıl yarı
  yerleşme kararı: hızdan bağımsız ve ölçüm istemez. (İşletme 2, Sadelik 1)
- **Sıralama:** muhasebe (jeton + `icerik` kapısı, hareket sayacı kanıtlanabilir
  şekilde `0`) **hareket kodundan önce** iner; yoksa 200 ms'lik ilk kayma
  (~24 kare) kapıyı kod doğruyken kırmızı düşürür ve iki değişiklik tek şişkin
  commit'e girer. (İşletme, Kısmi uygulama)
- **İmlecin metin rengi `Cursor` ile geçer.** Shader yolu seçilirse renk
  `bt-gpu`'da `theme.background_linear()` kısayoluyla türetilmez; türetilirse
  R4.1'in kararı gerçekten katmanı geçmiş olur. (Codebase-fit 3)
- **`dt` kırpılır.** Örtülme kalkınca `targetPresentationTimestamp` farkı
  sınırsız; kırpılmazsa "süre tavanı" vahşi kareyi önlemek yerine ondan sonra
  ateşler. (Codebase-fit 3)
- **`hucre=` sayacı korunur:** imleç dikdörtgeni `bg_count`'a girmemeye devam
  eder, yoksa `hucre=8` sessizce `9` olur ve hiçbir kapı görmez. (İşletme 3)
- **Belge borcu listelendi** ve phase'lere dağıtıldı: `CLAUDE.md`'nin "kirli
  satır yoksa frame gönderilmez" cümlesi, `Makefile`'ın duman yorumu,
  `proje.md`'nin kapı satırı, `IDLE_FRAME_LIMIT` ve `Waker::requests`
  doc'ları, `smoke_shell`'in "ikinci bir printf yok" cümlesi,
  `settings.rs`'in bilinmeyen-anahtar örneği olarak `[motion]`'ı vermesi,
  `docs/OLCUMLER.md`'nin sabit jeton bloğu, `docs/AYARLAR.md`.

**Reddedilenler:**

- **"Shader'daki dikdörtgen testi pahalı" (Sadelik 3'ün maliyet yarısı)** —
  fragment başına birkaç ALU işlemi; `cell_fragment` zaten doku örneklemesi
  yapıyor. Ölçülmemiş bir maliyet iddiası tasarımı değiştirmez.
- **"`ease` gerekçesiz, ikiyle başla" (Sadelik, küçük not)** — yol haritası üç
  stili adıyla sayıyor ve `ease` taşmasız seçeneği temsil ediyor; yay
  parametreleriyle aynı "seçilmiş sayı" kuralına tabi, ek bir mekanizma değil.
- **"Durma koşulu kontrolü `/audit`'in işi, runtime kapısı gereksiz"
  (Sadelik 1'in son yarısı)** — kapı gözetimsiz koşuyor, `/audit` koşmuyor;
  borcun kendisi "kapı görmüyor" diye açılmıştı.

## Karar (2026-09-16, kullanıcı onayı)

- **Karar 1 → iki katlı kapı, ikisi de bu sette.** (a) **Durum kararı:**
  deadline'da yerleşmemiş animasyon varsa `verdict()` kırmızı düşürür
  (`Verdict::MotionUnsettled`) — hızdan bağımsız, ölçüm istemez. (b) **Süreli
  kapı:** koşunun son karesiyle deadline arasındaki süre `sessiz=` olarak
  basılır ve **ölçülmüş** bir alt sınırla kapıya bağlanır (`sessiz ≥ T`).
  Kullanıcı ölçüm oturumunun bedelini bilerek kabul etti; gerekçe, altyapıyı
  atlayan bir sızıntının da görülmesi. Zaman tabanı display link'in kendi
  damgası (`CACurrentMediaTime` / `targetTimestamp`, ikisi de doğrulandı) —
  kare başına saat okuması yok, deadline'da tek okuma.
- **Karar 1'de reddedilen:** `kuyruk=` anlık görüntüsü (ikinci bir
  `performSelector`) — `T`'yi koşudan **önce** seçmeyi zorluyor ve shell erken
  çıkarsa hiç alınmadan sessiz yeşil veriyor; `sessiz=` aynı işi tek damgayla
  ve `none` değeriyle yapıyor. `istek=` oranı (ölçülmedi, iki yükte
  mertebelerce ayrışıyor) ve sınırı düşürmek (ilk animasyon kırmızı düşerdi).
- **Karar 2 → `icerik=`**, çıkarma yok (muhakeme).
- **Karar 3 → B (shader).** Kullanıcı kararı: blok harflerin üstünden piksel
  piksel geçsin. Renk `Cursor` ile sınırdan gelir, `bt-gpu` temadan türetmez.
  Phase hareketten **önce** iner ve görsel sonucu birebir bugünküdür.
  Reddedilen: ters çevirmenin `bt-core`'da kalması (kayma boyunca hedef harfin
  görünmemesi kabul edilmedi) ve shader'ı Smear/Squash setine ertelemek.
- **Karar 4 → B**, `clear` içerik karesine alınarak (muhakeme).
- **Karar 5 → öneri aynen:** imlecin kendi hareketi animasyonlu; ilk kare,
  görünürlük dönüşü, geometri ve kaydırma snap. `dt` kırpılır.
- **Karar 6 → `[motion] cursor_motion` + `reduce_motion`**, üç stil
  (snap/ease/spring), **varsayılan `spring`** (kullanıcı kararı). Enum
  `bt-core`'un ayar modelinde; `bt-gpu`'ya çözülmüş değer gider.
- **Karar 7 → öneri aynen:** iki kaynak tek sonuç, yalnız yeni konumda 90 ms
  belirme, hermetiklik `Inputs`'tan.
- **Karar 8 → reçeteye imleç hareketi eklenir.** `hareket > 0` duman kapısının
  gerekli sayacı olur.
