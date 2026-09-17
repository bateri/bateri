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
