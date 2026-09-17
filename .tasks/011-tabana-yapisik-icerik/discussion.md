# Input Dock ve tabana yapışık içerik — Tartışma

## Karar 1: Bu set neyi kapsıyor? → ✅ 1a

Yol haritası 011'i **üç iş** olarak yazıyor: Input Dock + yazma animasyonları
+ tabana yapışık içerik. Keşif bunun bir setten büyük olduğunu ve parçaların
**bağımlılıklarının farklı** olduğunu gösterdi.

- **1a — üçünü birden.** Yol haritasının yazdığı gibi.
  - *Artısı:* dock tek hamlede iner.
  - *Eksisi:* içinde çözülmemiş bir ürün çatalı (Karar 2) ve cevapsız bir
    mekanizma sorusu (Karar 4) var; ikisi de kodun şeklini belirliyor. Set
    yürürken karar değişirse yazılan kod atılır. Ayrıca prompt'un devri 010'u
    düşürüyor (Karar 2), yani "dock geldi ama blok işaretleri gitti" gibi bir
    ara durum üretme riski taşıyor.
- **1b — yalnız tabana yapışık içerik.** Dock ve prompt sonraki sete.
  - *Artısı:* saf yerleşim işi; kabuk betiğine, OSC 133'e, 010'a **hiç
    dokunmuyor**. Tek başına doğrulanabilir ve tek başına değerli — kullanıcının
    istediği his bu sette geliyor. Dock'un önünü de açıyor: dock satır
    aldığında içerik zaten oraya yaslanmış olacak.
  - *Eksisi:* dock gecikiyor.

**Önerim: 1b.** Gerekçe iki ölçülebilir gerçek: (i) tabana yapışma hiçbir
şeye bağlı değil, dock ise iki açık karara bağlı; (ii) prompt'un devri 010'un
mekanizmasını yeniden kurmayı gerektiriyor ve bu, "bir seti bitirirken
öncekini bozmak" demek. İkisini ayırmak her iki işi de küçültüyor.

## Karar 2: Prompt'u terminal çizerse 010 ne olur? → ✅ 2a

**Bu setin kapsamında değil ama kaydı burada durmalı** — sonraki setin girdisi
ve 1b'nin gerekçesi bu.

Blok kimliği bugün prompt'un **hücrelerinde** taşınıyor (`bateri.zsh:203`,
`session.rs:1361`). Prompt ızgarada çizilmezse hiç çıpa hücresi doğmaz ve
komut işaretleri tümden düşer. Üç yol var:

| | zsh tarafı | 010'a etkisi | `psvar[9]` borcuna etkisi |
|---|---|---|---|
| **2a** PS1 boşaltılır | `PS1=''` | **Çıpa ölür**, 010'un mekanizması yok olur; `B` de PS1'in içinde olduğu için dock'un aktivasyon sinyali gider | Konu dışı kalır (kapanmaz) |
| **2b** Terminal satırı `A`'da kaydeder | Betik **sadeleşir**: çıpa ve `psvar` silinir | Çıpa gereksizleşir, kimlik akıştan gelir | **Gerçekten kapanır** |
| **2c** Prompt basılır, terminal gizleyip dock'ta çizer | ~değişmez | Her şey korunur | Kalır, hatta kötüleşir |

**2b'nin ölçülmüş engeli:** tarayıcı `TappedPty::read` içinde ve
`advance()`'ten **önce** koşuyor (`shell.rs:13-16`, `session.rs:778`). İşareti
gördüğü anda ızgara o satıra henüz yazmamış, yani imleç satırı okunamaz.
2b okuyucu yolunun yeniden kurulmasını ister.

**Bir ürün bedeli daha:** 2a ve 2b'de kullanıcının p10k/starship prompt'u
**çizilmez**. Referans ürünün `>` görünümünü garanti edebilmesinin sebebi bu.
Kullanıcı kararı gerektirir ve bu sette sorulmuyor.

## Karar 3: Ofset nereden taşınır ve **ne zaman** uygulanır? → ✅ 3c

İlk hâli "ofseti `pos_at`'e ekle" diyordu. **Panelin üçü de aynı duvarı
gösterdi ve doğrular:** `Session::frame`'in sink'i döngünün **içinde**
koşuyor (`session.rs:1436`) ve `Frame::push` pozisyonu basma anında
`Instance`'a pişiriyor (`frame.rs:283`). `content_rows` ise döngü bitince
doğuyor. Yani ofset elimize geldiğinde o karenin bütün hücreleri, glyph'leri
ve kuralları **çoktan yerleşmiş** oluyor. `pos_at` yolu temsil edilemez.

Elenen kurtarma yolları (üçü de panelde fiyatlandı): ön tarama (maliyet tam
da ofsetin sıfırdan farklı olduğu boş bölgede), `frame()` öncesi ayrı sorgu
(`Term` kilidi iki kez; arada düşen bir satır ofseti bayatlatır), önceki
karenin değeri (4b'nin reddedilme gerekçesi, üstelik ekran dolmadan her
Enter'da).

- **3c — ofset çizim zamanı bir ötelemedir.** `frame()` `content_rows`'u
  döngüde toplar ve `Cursor` ile döndürür (kanal olarak 3a yaşıyor);
  `link.rs` `frame()` **döndükten sonra** `Frame::set_origin_rows(...)` çağırır;
  `Frame` bir `origin_px` alanı tutar ve **vertex uniform'u** olarak GPU'ya
  geçer. `pos_at` ve `push_block` **hiç değişmez**.
  - *Artısı:* sıra problemi yok — değer hücrelerden sonra gelebilir. Dört liste
    (arka plan, glyph, kural, işaret) ve imleç **gerçekten** tek yerden kayar;
    `push_block`'un `pos_at`'i atlaması (`frame.rs:350`, bilinçli) korunur.
    Instance başına maliyet sıfır. Yumuşak kaydırma borcu geldiğinde kesirli
    `origin_px` bedavaya hazır.
  - *Eksisi:* iki `.metal` dosyasında birer satır → phase **riskli** olur
    (`make shader` + `#[repr(C)]` ↔ `.metal` denetimi + phase sonu
    `/code-review`, `proje.md` → Kalite kapısı). Bu bütçelenmeli.
  - *Zorunlu ayrıntı:* `CursorBlock.rect` fragment'te `[[position]]` ile
    karşılaştırılıyor (`cell.metal`), yani **o da aynı ofsetle** kaydırılmalı;
    yoksa imlecin altındaki metnin rengi eski satırda kalır. Ve `origin_px`
    yalnız `Frame::clear`'da sıfırlanır: hareket karesi `clear` çağırmıyor
    (`link.rs:512`), orada **korunmalı**.

`bt-core` piksel değil **sayı** veriyor (`content_rows`), ofsete çevirmek
çizenin işi — `CLAUDE.md` → karar burada, boyama orada.

## Karar 4: Fare eşlemesi ofseti nereden okur? → ✅ 4d

İlk öneri 4a idi ("`view` her olayda `Session`'a sorar"). **Panel çürüttü:**
"son dolu satır" doğrudan sorulabilir değil (`Row::occ` `pub(crate)`), yani
sorgu `Term` kilidi altında bir **tarama** demek — ve `point_to_cell`
sürükleme boyunca her `mouseDragged`/`scrollWheel`'de koşuyor. Üstelik aynı
sayı `frame()` içinde zaten hesaplanıyor: iki hesap, tam da 4b'ye karşı öne
sürülen "kopya ayrışır" dersinin ihlali.

- **4d — `frame()` hesabını `Adapter`'a yazar, `view` kilitsiz okur.**
  Atomik, `dirty`'nin yanında (`session.rs:605`).
  - *Artısı:* **tek hesap, iki okuyucu.** Kopya üreticide duruyor ve çizilen
    değeri üreten hesabın kendisi yazıyor. Okunan değer **çizilen piksellere**
    ait; tıklamanın hedefi de o. Kilit yok, ikinci tarama yok.
  - *Eksisi:* değer "son çizilen kare"nin. Bu 4b'nin bayatlığı değil —
    bayatlığın **tek sahibi** var ve tıklama zaten ekrandaki piksele yapılıyor.
  - *Zorunlu ayrıntı:* `split_into_grid`'in yazılı tuzağı dikeyde birebir
    geçerli — çıkarma **`f64`'te** yapılmalı. Tabana yapışmada boş alan
    **üstte**; oraya yapılan tıklama `u16`'da taşar.

## Karar 5: Ofset animasyonlu mu? → ✅ 5b

- **5a — snap (animasyonsuz).** Yeni satır geldiğinde içerik anında bir satır
  yukarı.
- **5b — kayma animasyonu.** Yumuşak kaydırma (008'in altyapısı).

**Önerim: 5a.** Üç gerekçe: (i) boşta sıfır kare sözleşmesi — animasyonsuz
ofset **sıfır ek kare** getiriyor, çünkü içerik karesi zaten çiziliyor;
(ii) her animasyon bir durma koşulu ve `reduce_motion` indirgemesi ister
(`CLAUDE.md`), yani `motion`'a ikinci bir tüketici; (iii) 010 aynı kararı
şerit için verdi ve doğru çıktı. Yumuşak kaydırma zaten ayrı bir borç
(`docs/YOL-HARITASI.md`) ve geldiğinde bu ofset onun **tüketicisi** olur.

## Karar 6: `make duman`'ın `hareket` jetonu ne olacak? → ✅ 6a

**Panelin bulduğu ve doğruladığım blokaj.** `smoke_shell` (`session.rs:446`)
koşudaki tek imleç hareketini `\033[H` ile üretiyor: imleç (satır 1, sütun 0)
→ (satır 0, sütun 0), yani **saf dikey**. Tabana yapışmada ikisinin de ekran
satırı `rows-1`, sütunu 0 — imleç ekranda **hiç kıpırdamıyor**.

Sonuç iki yönlü ve ikisi de kötü:

- Ofset **doğru** uygulanırsa hedef hiç değişmiyor → `hareket=0` → `make duman`
  **kırmızı, kod doğruyken**.
- Ofset imleç yoluna uygulanmazsa `hareket > 0` **yeşil kalıyor** ama imleç
  son görünür satırın bir altında, yani **ekran dışında** doğup yukarı
  kayıyor: görünür kusur, yeşil kapı.

- **6a — reçetenin hedefine sütun bileşeni ver.** `\033[H` yerine sütunu da
  oynatan bir CUP (`\033[1;4H` gibi). Yatay hareket dikey ofsetten
  **etkilenmiyor**, yani jeton hem bugünkü hem yeni davranışta anlamlı kalıyor.
  - *Artısı:* `hucre/glif/kural` sayıları **oynamıyor** — onlar içerikten
    doğuyor, imlecin hedefinden değil. `smoke_shell`'in doc'undaki yasak
    ("süre parametresi, bir de şu kadar satır bas") yük eklemeye karşı; bu
    yük eklemiyor.
  - *Eksisi:* reçete bir sözleşme ve değişimi kayda geçmeli; doc'u da.
- **6b — jetonun anlamını değiştir** (ekran hareketi yerine grid hareketi say).
  - *Eksisi:* jeton "ekranda bir şey kıpırdadı" demek için var; grid'i saymak
    onu görünmeyen bir şeyin bekçisi yapar.

**Önerim: 6a.** Ve reçete değişimi **kod phase'inden ayrı commit'le** inmeli —
`IDLE_FRAME_LIMIT`'in kuralıyla aynı gerekçe (`proje.md`): ölçülmüş bir
sözleşme kod değişikliğiyle aynı commit'te oynarsa regresyonu maskeler.

## Karar 7: Kısmi uygulamayı kim görecek? → ✅ 7a

**Panelin en rahatsız edici bulgusu:** beş tüketiciden biri unutulursa
**her ara durum `make hepsi`'yi yeşil bırakıyor**. Bugün hiçbir sınama iki
yolun y'sini birbirine karşı ölçmüyor:

| unutulan | belirti | bugün gören |
|---|---|---|
| işaret (`push_block`) | şeritler tepede, metin aşağıda | **yok** |
| fare (`point_to_cell`) | tıklama N satır kayar | **yok** |
| imleç (`motion`) | imleç ekran dışından kayar | **yok** (`hareket>0` yeşil) |

- **7a — iki yeni bekçi, phase-1'de.** (i) `frame.rs`: aynı satıra basılan
  hücre ile işaretin y'si **eşit** olmalı — bugün `stripes_stay_out_of_the_cell_count…`
  ikisini aynı satıra basıyor ama y'lerini karşılaştırmıyor. (ii) `pos_at` ↔
  `point_to_cell` arasında **dikey turlama**: bir piksel satıra çevrilip geri
  piksele dönünce aynı hücreye düşmeli. Bugün bu tur x (pay) için bile yazılı
  değil, yalnız doc cümlesi var.

**Önerim: 7a.** Bekçisiz bir ofset, bu setin bütün riskinin toplandığı yer.

## Karar Noktaları

Panel sonrası **üç karar kapandı** (3c, 4d, 6a, 7a — gerekçeleri yukarıda) ve
kullanıcıya kalan ikisi şunlar:

1. **Kapsam:** yalnız tabana yapışık içerik mi (1b, önerilen), üçü birden mi (1a)?
2. **Animasyon:** snap (5a, önerilen) mı, kayma (5b) mi?

Kayıt (karar değil): alt ekranda ofset **0'a zorlanır** — vim/htop tam ızgarayı
sahipleniyor ve kısa dosyadaki `less` yoksa yukarı itilirdi. Bayrak `frame()`
içinde zaten elde (`session.rs:1176`).

Kapsam dışı ama kaydı burada: **prompt'un devri** (Karar 2) sonraki setin ilk
sorusu ve bir **ürün kararı** gerektiriyor — kullanıcının p10k/starship
prompt'unun çizilmemesi.

## Muhakeme (2026-09-17)

Üç jüri paralel koştu (`opus`), üçü de **SORUNLU** verdi ve **aynı duvarı**
bağımsız olarak buldu: ofsetin değeri, onu isteyen yerden sonra doğuyor.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

**Kabul edilen itirazlar** (üçü de plana işlendi):

1. **Sıralama boşluğu** (üç jüri birden). `pos_at` yolu temsil edilemez;
   ofset çizim zamanı bir uniform oldu → Karar 3c. Bu, `push_block`'un
   `pos_at` istisnasını da koruyor ve iki tüketiciyi (çizim orijini, işaret)
   tek noktada birleştiriyor.
2. **Fare yolunda ikinci hesap** (Sadelik + Codebase-fit). 4a bir tarama
   ya da bir kilit demekti; `frame()`'in yazdığı atomik hem ucuz hem tek
   hesap → Karar 4d.
3. **`hareket` jetonu kırılıyor** (İşletme). Reçetenin hedefine sütun
   bileşeni → Karar 6a; aritmetiğini bağımsız doğruladım.
4. **Kısmi uygulama görünmez** (İşletme + Codebase-fit). İki yeni bekçi →
   Karar 7a.
5. **İmleç hareketi** (Codebase-fit + Sadelik). Ofset `motion`'ın "dünya
   kaydı" kimliğine katılmalı; `Motion` zaten `display_offset` değişimini
   snap'e çeviriyor (`motion.rs:245`), ofset oraya katlanırsa yazarken
   animasyon korunur, içerik kayarken snap olur. Plan bunu phase-1'in kabul
   ölçütüne yazacak.

**Reddedilen yok.** Panelin "kapsamı genişletme" sınırı korundu: hiçbir jüri
dock ya da prompt önermedi, üçü de 1b'yi zaten minimum kesit saydı.

**Panelin düzelttiği iki belge hatası** (ikisi de `docs/YOL-HARITASI.md:53`,
bu oturumda benim yazdığım): (a) "kirli satır takibi devre dışı kalır" —
devre dışı kalacak bir satır takibi yok, hasar tek bir `AtomicBool` ve
`Session::frame` koşulsuz tam tarıyor; (b) "010'un açık kalemini bu kapatıyor"
— o kalem 010'un kendi son commit'inde (`fba3083`) kapandı. İkisi de plan
onaylanınca düzelecek.

## Karar (2026-09-17, kullanıcı)

- **Karar 1 → 1a: dock aynı sette.** Öneri ve üç jürinin örtük duruşu 1b idi
  (kapsamı tabana yapışmayla sınırlamak); kullanıcı dock'u aynı sete koydu.
  Reddedilen: 1b. **Sonucu:** Karar 2 (prompt'un devri) artık kapsam dışı bir
  kayıt değil, **bu setin bloke eden ilk sorusu** — dock'un girdi yolu ile
  prompt'un kimin çizdiği aynı soruya bağlı ve cevap 010'un blok işaretlerinin
  yaşayıp yaşamayacağını belirliyor.
- **Karar 5 → 5b: yumuşak kayma.** Öneri 5a (snap) idi; kullanıcı animasyon
  istedi. Reddedilen: 5a. **Sonuçları planın taşıması gereken üç kalem:**
  (i) `bt-gpu::motion` ikinci tüketicisini kazanıyor ve `Mode::Fade`'in
  "indirgemenin tek yeri" kuralı ikinci bir yer buluyor; (ii) her animasyon bir
  **durma koşulu** ister (`CLAUDE.md` → boşta sıfır kare) ve `reduce_motion`
  ile sistemin Hareketi Azalt ayarı bu kaymayı da indirmeli; (iii) animasyon
  `kare` jetonunu meşru olarak şişirir, yani duman kapısının operandı yine
  `icerik` kalır ama `hareket`'in ikinci bir kaynağı doğar.
- **Karar 6 yeniden açılıyor.** 6a (reçeteye sütun bileşeni) 5a varsayımıyla
  seçilmişti: snap'te `\033[H` hiç hareket üretmiyordu. 5b'de içerik kayması
  **kendisi** hareket karesi doğuruyor, yani `hareket=0` riski ortadan kalkmış
  olabilir. Phase yazılmadan önce yeniden türetilmeli; jetonun iki kaynağı
  (imleç hareketi + içerik kayması) ayırt edilebilir kalmalı.
- **Karar 3c, 4d, 7a değişmedi:** ofsetin çizim zamanı uniform olması, fare
  yolunun `frame()`'in yazdığı atomiği okuması ve iki yeni bekçi, animasyon
  kararından bağımsız. 3c kesirli `origin_px`'i zaten destekliyor — 5b onun
  "yumuşak kaydırma geldiğinde bedavaya hazır" cümlesinin ilk tüketicisi.
- **Karar 2 → 2a: prompt'u terminal devralır.** Referans ürünün yolu.
  Reddedilenler: 2c (prompt kalsın, en düşük risk) ve 2b (satırı `A`'da
  kaydet, `psvar[9]` borcunu kapatan tek yol). **Bedelleri kayda geçiyor:**
  - Kullanıcının p10k/starship prompt'u **çizilmez**. Bu bir geri dönüş
    noktasıdır: prompt kullanıcının kimliğidir ve onu devralmak, karşılığında
    en az onun kadar iyi bir prompt vermeyi borçlandırır.
  - **010'un çıpa mekanizması yeniden kurulur.** Blok kimliği bugün prompt'un
    hücrelerinden okunuyor (`session.rs:1361`); prompt ızgarada çizilmezse
    hiç çıpa hücresi doğmaz. Kimliğin yeni kaynağı akış (`A`'nın `bt_block=`)
    olmak zorunda — yani 2b'nin okuyucu-yolu işi, 2a'nın **içinde** yapılmak
    zorunda.
  - **Yeni bir ürün yüzeyi doğuyor:** prompt artık bizim, yani içeriği
    (dizin, git dalı, çıkış kodu), teması ve ayarlanabilirliği bu setin
    sorusu. `docs/ARASTIRMA.md` referansın prompt renklerini sekiz rolden
    türettiğini ve tema değişince canlı güncellediğini söylüyor.
  - `B` işareti bugün PS1'in içinde (`bateri.zsh:218`); PS1 boşalınca
    **dock'un aktivasyon sinyali de gider** ve başka bir yere taşınmalı.

---

# İkinci tur (2026-09-17) — genişlemiş kapsamın karar noktaları

Kullanıcı kararları (1a dock aynı sette, 2a prompt'u terminal devralır, 5b
yumuşak kayma) kapsamı panelin gördüğünden büyüttü. Aşağıdaki karar noktaları
o genişlemenin içinden çıktı; `context.md`'nin "cevapsız kalan mekanizma
soruları" listesi burada kapanıyor.

## Karar 6 (yeniden türetildi) → **6a ayakta kalıyor**

Kullanıcı notu "5b'de `hareket=0` riski ortadan kalkmış olabilir" diyordu.
Yarısı doğru, ve yanlış yarısı daha tehlikeli.

`hareket` jetonu `motion_frames`'tir (`link.rs:389`, artma noktası tek:
`link.rs:510`) ve sayma koşulu iki şey: kare **hasarsız** olacak ve
`motion.settled()` **false** olacak. Hasarlı kare `icerik`'e yazılıyor
(`link.rs:614`), `hareket`'e değil. Ölçülmüş taban: bugünkü `\033[H`
reçetesinde tek hücrelik imleç sıçraması **25–27** hareket karesi üretiyor
(`docs/OLCUMLER.md` → Boşta kare, n=20).

Kayma animatörü aynı `settled()` kapısına bağlanırsa — ki `link.rs:484-497`'de
uyku kararının tek gerçek kaynağı o — kuyruğu **`hareket`'e yazılır**. Sonuç:

| | snap (5a) | kayma (5b) |
|---|---|---|
| ofset doğru | `hareket=0` → kod doğruyken **kırmızı** | `hareket>0` ama tanığı **kayma**, imleç değil |
| ofset imleç yoluna uygulanmamış | `hareket>0` **yalancı yeşil** | `hareket>0` **yalancı yeşil** |

5b'nin getirdiği şey kırmızıyı yeşile çevirmek değil, **kırmızıyı yalancı
yeşile** çevirmek: jeton artık imleç yolu tamamen bozukken de yeşil kalıyor.
Bu, reddedilen 6b'nin ("jetonun anlamını grid hareketi say") kazara
gerçekleşmiş hâli. Ayırt etme mekanizması bugün yok: tek `Motion`, tek
`settled()`, tek `u64`, tek jeton.

**6a değişmiyor:** reçetenin hedefine sütun bileşeni verilir (`\033[H` →
`\033[1;4H` gibi). Yatay hareket dikey ofsetten etkilenmiyor, yani jeton her
iki davranışta da **imleç yolunun** tanığı kalıyor; `hucre/glif/kural`
oynamıyor (onlar içerikten doğuyor). Ayrı commit kuralı da korunuyor.

## Karar 8: Dock hangi model? → **panelin ilk sorusu**

`context.md`'nin "ayna mı asıl mı" sorusu bir dipnot değil, bu sette "dock"un
**ne demek olduğu**. Ayırt edici tek soru: **ZLE komutu ızgaraya çizmeye devam
ediyor mu?**

- **8a — imleç satırı kroması.** Ayrı bir dock alanı yok: içerik tabana
  yapıştığı için **imlecin satırı zaten pencerenin dibinde**. Terminal
  `>` işaretini o satırın payına çizer, safha `Input` iken. ZLE kancası yok,
  özel OSC yok, base64 yok, tarayıcıya ikinci kol yok, çift çizim yok,
  ayrılmış satır yok — **Karar 9 konusuz kalır**.
  - *Artısı:* bu setin kapsamındaki beş işin hepsini karşılıyor ve girdi
    yolu (`view.rs:411` → `session.write`) hiç değişmiyor. Tabana yapışma
    zaten yazılan yeri dibe sabitliyor, yani Input Dock'un **görünüşü**
    geometriyle geliyor, ayrı bir widget'la değil.
  - *Eksisi:* tuş vuruşu animasyonları (`keypress`, `delete_mode`) bu modelde
    yapılamaz — "bu harfi kullanıcı mı yazdı" sorusunun cevabı yok. Ama o iş
    `context.md`'nin beş işinde **zaten yok**.
- **8b — ayna.** Tuşlar yine PTY'ye gider; ZLE `BUFFER`/`CURSOR`'ı özel bir
  OSC ile geri bildirir, terminal dock'ta çizer. Mantık ZLE'de kalır, yani
  Tab/geçmiş/Ctrl-R yeniden yazılmaz.
  - *Ölçülen bedeli üç katmanda:* (i) `add-zle-hook-widget line-pre-redraw`
    (`zle -N` **değil**: zsh-syntax-highlighting ve autosuggestions aynı
    widget'ı istiyor) + `line-finish`; (ii) tarayıcıya ikinci OSC numarası
    kolu — `shell.rs:486` bugün `133` olmayanı tampona hiç uğratmadan eliyor,
    `PAYLOAD_LIMIT` 256 bayt (`shell.rs:372`) ve çıplak `ESC` diziyi bitiriyor
    (`shell.rs:536`), yani `BUFFER` için **base64 zorunlu**; (iii) ZLE aynı
    metni ızgaraya da çiziyor, yani bastırma tutamağı gerekiyor.
  - *Ödenmemiş bedel:* ayna yalnız `BUFFER`/`CURSOR` taşıyor. ZLE'nin
    `BUFFER` olmayan çıktısı — tamamlama listesi, `menu-select`,
    `bck-i-search:`, `zle -M`, `CORRECT`'in `[nyae]`'i — aynada **yok**,
    ızgaraya çizilir. "Tab/Ctrl-R yeniden yazılmaz" mantık için doğru,
    **görüntü için yanlış**: listeler dock'ta değil ızgarada belirir.
- **8c — asıl.** Tampon dock'ta, satır Enter'da gönderilir.
  - *Elenir, iki ölçülmüş gerekçe:* ZLE'yi susturmadan çift çizim
    kaçınılmaz, ZLE'yi susturmak Tab/geçmiş/Ctrl-R'yi öldürür; ve zsh'in
    tek-tuş prompt'ları (`CORRECT`'in `[nyae]?`'ı, `RM_STAR_WAIT`)
    **ölümcül** — dock Enter'a kadar tamponlar, zsh o tuşu hiç görmez.

**Önerim: 8a.** Gerekçe: setin taşıdığı beş işin hepsini karşılıyor, üç
katmanda yeni tesisat istemiyor ve 8b'ye yükseltme yolu açık kalıyor (tuş
vuruşu animasyonları geldiğinde ayna onun altyapısı olur). 1a "dock aynı
sette" kararını **iptal etmiyor** — dock'un görünüşünü geometriden üretiyor.

## Karar 9: Dock ızgaradan satır alır mı? → **9a**, ama 8a'da konusuz

Yalnız 8b/8c seçilirse sorulur.

- **9a — kalıcı ayır.** `rows = (height_px − dock_px) / cell_h`,
  görünürlükten bağımsız. Deponun yatayda verdiği cevabın dikey ikizi
  (010 Karar 3, `app.rs:388-393`: "pay her zaman ayrılıyor"). Bedeli dock
  gizliyken birkaç satır ölü alan.
- **9b — görünürlüğe bağlı.** *Elenir:* komut başına **iki** resize demek ve
  her birinin bedeli ölçülü — `Term` kilidi (okuyucunun ayrıştırma lease'inin
  arkasında), `shrink/grow_lines` ring rotasyonu, `TIOCSWINSZ`/SIGWINCH, tam
  yeniden çizim, ve `link.rs:1015` → `motion.rs:255` yoluyla **her komutta
  imleç snap'i** — yani 008'in ürün kararının komut başına iptali.
- **9c — overlay.** *Elenir:* tabana yapışık içerikte pencerenin altı **en
  taze çıktının** yeri; overlay tam oraya biner. 011 öncesi (tavana yapışık)
  orası boştu, 011 sonrası en pahalı yer orası.

## Karar 10: Prompt çizilmezse blok **satırı** nereden gelir? → **10a**

`discussion.md` → Karar 2a "010'un çıpa mekanizması yeniden kurulur" diyordu.
**Bedel abartılmış:** kimlik zaten akışta (`bt_block=`, `bateri.zsh:172,183`;
okuyan `shell.rs:591`) ve satır için okuyucu yolunu yeniden kurmaya da gerek
yok.

- **10a — `anchor_close`'u preexec'e taşı.** PS1 görünür genişliği sıfır olur
  ama `anchor_open` + `B` içinde kalır; kapanış `bateri.zsh:221`'den
  `__bateri_preexec`'e (`C`'den önce) iner. OSC 8 açık kaldığı için ZLE'nin
  çizdiği **komut metninin her hücresi** çıpayı taşır.
  - *Doğrulandı, alacritty kaynağında üç satır:* `set_hyperlink` imleç
    **şablonuna** yazıyor (`term/mod.rs:1876`); `write_at_cursor`
    `template.extra`'yı yazılan her hücreye kopyalıyor (`:989`); `Attr::Reset`
    (SGR 0) fg/bg/flags/underline_color'ı siliyor ama **hyperlink'i
    silmiyor** (`:1888-1893`) — yani zsh-syntax-highlighting'in her tuşta
    bastığı `\e[0m` çıpayı bozmuyor.
  - *Artısı:* `session.rs:1361`'deki kare başına ızgara taraması **hiç
    değişmiyor**, yani reflow'a, kaydırmaya ve geçmiş taşmasına dayanıklılık
    (`bateri.zsh:185-188`'in tezi) aynen korunuyor. Betik tarafı **iki
    satır**; bekçisi 010'un mevcut sınamaları.
  - *Sınırı:* boş Enter'ın bloğu hücresiz, yani çıpasız kalır — ama o blok
    zaten hiçbir zaman çizilmiyor (`shell.rs:353-361`).
- **10b — satırı akıştan kaydet.** *Elenir:* tarayıcı `advance()`'ten önce
  koşuyor **ve** `EventLoop` tamponu biriktirerek okuyor (`session.rs:772-774`),
  yani işaret anında ızgara "bir adım" değil **belirsiz miktarda** geride;
  "işaretin yerinden böl" çaresi de bu yüzden çalışmıyor. Üstelik kaydedilen
  satır reflow'da ve geçmiş taşmasında bayatlar — bugünkü çıpanın **asıl**
  değeri buydu.

**Sonuç:** 2a'nın "çıpa mekanizması yeniden kurulur" maddesi ve `psvar[9]`
borcunun kapanacağı beklentisi **düşüyor**; borç ne kapanıyor ne büyüyor.

## Karar 11: Kayma neye bağlı, 008'in snap kuralı ne oluyor?

5b bugün **iki yerde yazılı** bir sözleşmeyi açıyor:
`docs/AYARLAR.md:399` ("imlecin kendi hareketi kayar, **altındaki ızgaranın**
hareketi kaymaz") ve `session.rs:205-218` (`Cursor::display_offset` doc'u,
008 Karar 5).

Ayrım korunabilir ve korunmalı: **tekerlekle kaydırma snap kalır** (parmağı
takip ediyor), **içerik büyümesinin kaldırması animasyonlu olur**. İkisi
`row`'dan ayırt edilemediği için `display_offset` kimliği konmuştu; tabana
yapışma ofseti **üçüncü** bir sinyal ve ikisiyle karışmıyor. Zorunlu kural:
`display_offset` bir önceki kareden farklıysa origin animatörü de **snap**'ler
(`motion.rs:245`'in kuralının ikizi) — yoksa `clear`'dan sonra tekerlekle
gezinen kullanıcıda içerik kayardı.

**Anahtar:** `cursor_motion` imlecin anahtarı, kaymanınki olamaz; ayrı bir
anahtar gerekiyor (referansın adı `feed_lift`, `ARASTIRMA.md` → Hareket).
`reduce_motion` bedavaya geliyor: `Mode::Fade` "konum anında hedefte" demek
(`motion.rs:132`), yani indirgemenin tek yeri kuralı korunuyor.

**Sınırı adıyla konmalı:** 5b yalnız **ekran dolmadan** geçerli — origin
animasyonu. Ekran dolduktan sonraki besleme kayması (grid satırlarının
kayması) **başka bir mekanizma** ve `docs/YOL-HARITASI.md`'de ayrı borç
(yumuşak kaydırma). Yani ekranın dolduğu anda kayma durur: "iki ayrı his"
animasyon düzeyinde geri gelir. Kullanıcı bunu **keşfetmemeli**, okumalı.

## Karar 12: Prompt'un içeriği bu sette ne kadar? → kapsam koruması

2a "prompt artık bizim" dedi ve yanında bir ürün yüzeyi doğurdu. Bu setin
alacağı kesit **dar**:

- **Girer:** `>` işareti ve safhaya göre görünürlüğü; çıkış kodu rengi —
  elde, `ShellState.last_exit` (`session.rs:1792`).
- **Girmez:** **çalışma dizini** — OSC 7 bugün hiç bağlı değil
  (`Event::Title`/`ResetTitle` sessizce düşüyor, `session.rs:728`), yani
  dizin göstermek ayrıca döşenecek bir iş. Git dalı, prompt ayar şeması ve
  prompt'un tema alt-rolleri de bu set değil.
- **Zorunlu ayrıntı:** PS1 tek başına yetmez, **`RPS1`/`RPROMPT` de**
  boşaltılmalı; ve p10k/starship PS1'i precmd'den **sonra** kendi ZLE
  kancalarından yeniden kuruyor — sıfır genişlik aynı yerden dayatılmazsa
  tema kazanır.

## Muhakeme — 2. tur (2026-09-17)

Üç jüri paralel koştu (`opus`), genişlemiş kapsamı (1a + 2a + 5b) ve ikinci
turun altı karar noktasını gördü.

| Mercek | Verdict |
|---|---|
| Sadelik | KIRMIZI |
| Codebase-fit | SORUNLU |
| İşletme | KIRMIZI |

**Doğrulanan (üç jüriden biri bağımsız kontrol etti): Karar 10a TEMİZ.**
Alacritty'nin üç satırı da iddia edildiği gibi (`term/mod.rs:1876`, `:989`,
`:1888-1893`) ve mekanizma iddia edilenden **sağlam**:
`Cell::set_underline_color(None)` `extra`'yı ancak hyperlink de yokken
düşürüyor (`cell.rs:182-186`), `Cell::reset` `extra`'yı sıfırlıyor ama
yeniden çizilen hücre PS1'in hâlâ açık şablonundan çıpayı geri alıyor
(`cell.rs:252-254`) — `zle reset-prompt`, Ctrl-L, geçmişte gezinme ve
`TRANSIENT_PROMPT` kırmıyor. *Plana yazılacak bilinen sınır:* sıfır genişlikli
PS1'de "ilk çıpalı satır = komutun satırı" yapısal değil **sıra** garantisi;
`zle -I` ile basılan bir iş bildirimi şeridi bir satır yukarı kaydırabilir.

**Kabul edilen itirazlar → tasarım değişikliği:**

1. **`>` bugünkü `Frame`'de temsil edilemez** (üç jüri birden). Payda
   dikdörtgen çizilebiliyor (`push_block` → `cell_bg`), **glyph
   çizilemiyor**: `pos_at` payı ekleyen tek yer ve doc'u ikinci bir yeri
   adıyla yasaklıyor (`frame.rs:519-523`); `GlyphInstance`'ta `size` alanı
   bilerek yok, boy kare başına tek uniform, yani hücreden dar bir glyph de
   temsil edilemez (alan eklemek iki taraftaki `stride 32` assert'ini kırar);
   `GUTTER_PT = 8.0` "şerit artı nefes payı" diye tanımlı ve `private`
   (`renderer.rs:123-135`). Eksik olan atlas sprite'ı **değil** (`>` ASCII,
   dört yüzde de rasterize), **yerleştirme**.
   → **`>` bu sette çizilmez.** Prompt işareti 010'un **şeridi** olur: safha
   `Input` iken `frame()` `cursor_row`'a bir `Block` verir. Yeni konum
   formülü, pay genişlemesi, sprite yok. Çarpışma da yok — `ShellLog::stripe`
   `Pending` + koşmuyor için `None` veriyor (`shell.rs:358-361`), yani o
   satırda zaten şerit çizilmiyor. `>` şekli, çizilecek bir yer doğduğunda.

2. **6a'nın aritmetiği yanlıştı** (İşletme). `Motion::sync` snap'i eksen
   başına değil **konumun tamamına** uyguluyor (`motion.rs:283-296`:
   `pos: target, vel: [0.0; 2]`). Önerilen `\033[1;4H` imleci satır 0'a
   taşıyor → `content_rows` 2→1 → ofset kimliği değişiyor → snap → **sütun
   bileşeni de sıfır hareket karesi üretiyor.**
   → Reçete **satırı korur, yalnız sütunu oynatır**: `\033[4G` (CHA).
   `content_rows` 2'de kalır, ofset kimliği oynamaz, `sync` animasyonlu kolu
   alır. Yan fayda: t=1 s'de origin olayı doğmaz, yani `sessiz` bandı
   bugünkü yerinde kalır ve `QUIET_FLOOR`'un türetmesi yeniden açılmaz —
   payı ölçülmüş **2,29 ms** (en düşük sağlıklı 1742,29 ms, taban 1740).

3. **Jetonun iki kaynağı ayrılmalı** (İşletme + Codebase-fit). Origin
   animatörü `Motion::settled()`'ın **içine girmek zorunda**, yoksa hasarsız
   kolda link kayma ortasında uyur ve içerik donar (`link.rs:492-505`). Yani
   "yalancı yeşil" yapısal: 6a kaynağı ayırt etmiyor.
   → `hareket`'in yanına **`kayma=`** eklenir (jeton silinmez, eklenir).
   `hareket` yalnız imleç animatörü, `kayma` yalnız origin animatörü
   yerleşmemişken artar; kapı `hareket > 0` kalır ve gerçekten imleç yolunun
   tanığı olur. Bu, ilk turun kendi şartıydı ("iki kaynak ayırt edilebilir
   kalmalı").

4. **7a'nın iki bekçisi 3c'ye karşı boş** (İşletme, Codebase-fit teyit).
   Bekçi (i) inşa gereği doğru olan bir eşitliği sınıyor — ofset GPU'da, iki
   taraf da CPU'da ofsetsiz. Bekçi (ii) yazılamıyor — `pos_at`'te ofset yok,
   `point_to_cell`'de var; test kaydırmayı kendisi yeniden yazmak zorunda
   kalır, yani bekçi olduğu şeyi taklit eder.
   → Bekçiler **CPU-CPU eşitliği değil CPU→GPU dikişi** olur: offscreen
   render + `set_origin_rows(k)`, boyanan satır **iki pipeline için de**
   okunur (emsal `cell_bg_paints_pixels_on_the_gpu`). Üçüncü bekçi eklenir:
   `push_cursor`'ın `rect.y`'si aynı satıra basılan hücrenin `pos.y`'siyle
   eşit — bugün Karar 7'nin üç belirtisinden yalnız ikisinin bekçisi var ve
   bekçisiz kalan, tam da 5b'nin körleştirdiği tüketici.

5. **4d "animasyon kararından bağımsız" değil** (İşletme + Codebase-fit).
   Atomik `content_rows`'un tam sayı **hedefini** taşıyor; 5b'de pikseller
   kayma boyunca animasyonlu origin'de. 4d'nin seçilme gerekçesi ("okunan
   değer **çizilen piksellere** ait") tam o pencerede yanlış.
   → Atomik hedefi değil **animasyonlu origin'i** yazar.

6. **Salınım: imlecin hedefi ekran uzayına taşınır** (Sadelik). 5b'de imlecin
   grid satırı anında r→r+1 olurken origin animasyonla giderse imleç bir
   satır aşağı düşüp geri biner; ilk turun "ikisi birbirini götürür"
   varsayımı 5a'ya aitti.
   → İmlecin hedefi **ekran satırı** (`row + origin_rows`) olur ve imleç
   origin ötelemesinden **muaf** tutulur. Enter'da ekran hedefi değişmez →
   salınım yok, imleç dipteki giriş satırında kalır, içerik arkasından yukarı
   akar. 3c'nin "`CursorBlock.rect` de kaydırılmalı" zorunlu ayrıntısı da
   böylece düşer.

7. **`feed_lift` düşer** (Sadelik; Codebase-fit maliyetini sayıyor). Ayrı
   anahtar `docs/AYARLAR.md:397-398`'i yalanlıyor (`"snap"` — "Hareketi
   tamamen kapatmanın yolu bu") ve `CLAUDE.md`'nin `cursor_motion = "snap"`
   üstünlük cümlesini de. Maliyeti beş kalem (`settings` şeması,
   `Settings::changes`, `AYARLAR.md`, vnode kolu, `DisplayLink` setter'ı).
   → Kayma **`cursor_motion`'ın animasyonlu/snap ayrımını izler**; yeni
   anahtar yok, belgeli cümleler ayakta kalır.

8. **`Mode::Fade` origin'e uymuyor** (Codebase-fit). Fade "konum anında
   hedefte, değişen şey opaklık" (`motion.rs:132`); her yeni satırda bütün
   ekranın belirmesi indirgemeye çalıştığı hareketten beterdir.
   → Origin'in indirgemesi **snap**. `CLAUDE.md`'nin "her animasyonu 90
   ms'lik bir belirmeye indirir" cümlesi aynı commit'te düzelir; "indirgemenin
   tek yeri `bt-gpu::motion`" kuralı ayakta (yer aynı, kip iki).

9. **3c'nin shader bedeli gereksiz olabilir** (Codebase-fit). Bugün hiçbir
   encoder `setViewport` çağırmıyor; varsayılan viewport tüm drawable.
   `encode_pass`'te tek satır (`originY: origin_px`) **iki pipeline'ı birden**
   kaydırır: sıfır shader satırı, sıfır stride/assert riski, kesirli origin
   bedava.
   → Birincil yol `setViewport`, **kanaryası**: Metal'in drawable'ı aşan
   viewport'u scissor'la kırptığı doğrulanır; tutmazsa uniform yoluna dönülür.
   Her iki yolda da aynı kalan iki kalem: (a) `[[position]]` viewport
   dönüşümünden **sonraki** koordinat, yani `CursorBlock.rect`'e origin CPU'da
   eklenir; (b) hasarsız kolda `origin`'in **ikinci yazma noktası** gerekir —
   3c "korunur" diyordu, doğru, ama animasyon *değişmeyi* gerektiriyor ve o
   kolda `frame()` de `clear` de çağrılmıyor (`link.rs:492`, `:576`).

10. **2a'nın bağımsız kapatma yolu yok** (İşletme). Bugün tek çıkış
    `shell.integration = "off"` ve o blokları da öldürüyor, üstelik sonraki
    oturumda.
    → Geri dönüşün **hep-ya-hiç** olduğu `docs/AYARLAR.md`'ye yazılır; ayrı
    anahtar bu setin işi değil (Karar 12 kapsam koruması).

11. **`content_rows` monoton değil** (İşletme). İmleci yukarı taşıyıp alt
    satırı `\e[K` ile silen bir program onu daraltıp genişletir, yani origin
    animatörünün durma koşulunun **girdisi** salınabilir.
    → "Her animasyon bir durma koşulu taşır" kuralının bu setteki karşılığı
    phase'de yazılı olur.

12. **Ölçüm bekleyen iki iddia.** Kayma animasyonunun yerleşme süresi ve
    `sessiz`e etkisi ölçülmedi → **"ölçüm bekliyor: kayma animasyonunun
    yerleşme süresi ve `sessiz` bandına etkisi"**.

**Kullanıcıya taşınan (panel karar veremez):** Karar 8'in kapsamı. Üç jüri de
8a'nın Input Dock'un tanımını (`ARASTIRMA.md:54`: "pencere altında sabit
**ayrı satır editörü**", "dock alanı geri alınıyor") karşılamadığını söyledi;
Sadelik bunu "reddedilen 1b'nin yeni adı" diye adlandırdı. Dock'un gerçek
karşılığı 8b (ayna) ve üç katmanda yeni tesisat istiyor. Bu bir kapsam
kararıdır ve kullanıcınındır.

**Reddedilen yok.** Panelin kapsam sınırı korundu: hiçbir jüri 1a/2a/5b'yi
iptal etmeyi önermedi, üçü de "nasıl"ı sorguladı.

## Karar — 2. tur (2026-09-17, kullanıcı)

- **Karar 8 → dock ayrılıyor.** Bu set beş işten dördünü taşıyor: tabana
  yapışık içerik, yumuşak kayma, prompt'un devri, çıpanın preexec'e taşınması
  ve safha şeridi. **Input Dock'un kendisi — ayrı satır editörü, ZLE aynası,
  ayrılmış satırlar — sonraki sete (012).**
  Gerekçe üç jürinin ortak bulgusu: 8a Input Dock'un tanımını karşılamıyor
  (`ARASTIRMA.md:54`) ve onu "dock" diye adlandırmak, kullanıcının reddettiği
  1b'yi yeni bir adla geri almak olurdu. Dock'un gerçek karşılığı 8b (ayna) ve
  üç katmanda yeni tesisat istiyor; üstelik aynanın **tasarlanmamış bir görsel
  dikişi** var — ZLE'nin `BUFFER` olmayan çıktısı (tamamlama listesi,
  `menu-select`, `bck-i-search`, `zle -M`) aynada yok, ızgaraya düşüyor. O
  dikiş kendi setinin tasarım turunu hak ediyor.
  **Adı dürüst konur:** bu set dock değil. Prompt'un devri de 012'ye taşınınca
  (aşağıdaki Karar 2 eki) klasör `011-tabana-yapisik-icerik` olarak yeniden
  adlandırıldı: **numara** yeniden kullanılmıyor (`duzen.md` → Konum ve ad),
  ama dock'u açıkça dışarıda bırakan bir sette `input-dock` slug'ı indeksi
  yanıltırdı. `input-dock` adı 012'ye kalıyor.
  Reddedilen: 8b'yi bu sete koymak. Konusuz kalan: **Karar 9** (ayrılmış satır
  sorusu dock'la birlikte 012'ye gider; 9a'nın gerekçesi orada geçerli kalır).
- **Karar 12 daralıyor.** Prompt'un devri (2a) duruyor, ama prompt'un
  **çizilen içeriği** bu sette yalnız safha şeridi: `>` şekli panelin
  bulgusuyla temsil edilemez ve dock'la birlikte 012'ye gider. Çıkış kodu
  rengi şeritten geliyor (010'un yolu), dizin OSC 7'ye bağlı ve o da kapsam
  dışı.

- **Karar 2 → 2a duruyor ama 012'ye taşınıyor** (kullanıcı, aynı oturum).
  Panelin "`>` temsil edilemez" bulgusundan sonra 011'de prompt'u devralmak,
  kullanıcının prompt'unu alıp yerine yalnız bir şerit koymak anlamına
  geliyordu — 2a'nın getirisi ("terminal daha iyi bir prompt çizsin") tam da o
  ara durumda ödenmemiş kalıyor. Sadelik jürisinin itirazının özü buydu.
  Bağımlılık da aynı yeri gösteriyor: R3 (prompt'un devri), R4 (çıpanın
  preexec'e taşınması) ve R5 (safha şeridi) **yalnızca** prompt devralındığı
  için vardı; prompt kabukta kalınca çıpa zaten çalışıyor ve betiğe hiç
  dokunulmuyor.
  **Sonuç:** 011 saf yerleşim işi olur — tabana yapışık içerik, yumuşak kayma,
  duman reçetesi. Kabuk betiğine, OSC 133'e ve 010'a **hiç dokunulmuyor**.
  012 tek tutarlı iş olur: prompt'un devri + `>` + dock + çıpanın taşınması +
  tuş vuruşu animasyonları, hepsi "giriş satırı artık terminalin" başlığı
  altında. Reddedilen: 2a'yı 011'de tutmak. Konusuz kalan: Karar 10 ve
  Karar 12 (ikisi de 012'nin girdisi; **bulguları geçerli ve orada
  kullanılacak** — özellikle 10a'nın alacritty doğrulaması).

- **Karar 4 → 4d'nin atomiği düşüyor, gerekçesi ayakta** (phase yazılırken,
  aynı oturum). Panelin R2.7 itirazı ("atomik hedefi değil **animasyonlu**
  origin'i yazmalı, yoksa kayma boyunca tıklama bir satır kayar") origin'in
  sahibini sorguya açtı. Animasyonlu origin `bt-gpu`'nun `Motion`'ında; fare
  ise `bt-shell`'de — ve `bt-shell → bt-gpu` **meşru yön**.
  Belirleyici gerçek: display link callback'i **ana thread'de** koşuyor
  (`link.rs:792` `addToRunLoop_forMode(mainRunLoop, …)`, sınıf
  `#[thread_kind = MainThreadOnly]`, ivar'lar bu yüzden `Cell`) ve
  `point_to_cell` de ana thread'de (NSView fare olayı). Yani iki taraf **aynı
  thread'de**: atomik, kilit ve yarış yok.
  → Origin'in **tek sahibi** `DisplayLink`: `Cursor.content_rows`'tan kurulur
  (hesap yine `frame()`'de, yani 4d'nin "tek hesap" gerekçesi korunuyor),
  `view` onu doğrudan okur. Fare **çizilen** değeri okuyor — 4d'nin asıl
  gerekçesi buydu — ve phase-2'de o değer animasyonlu olunca **çağrı yeri hiç
  değişmiyor**.
  **Yan kazanç:** `bt-core`'a yeni paylaşılan durum girmiyor, yani phase-1'in
  `make test-yaris` tetiği ve "riskli phase" etiketi düşüyor.
  Reddedilen: `Adapter`'a atomik koymak (thread'ler ayrı olsaydı doğru olurdu;
  değiller).
