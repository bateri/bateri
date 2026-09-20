# Ekranın geri dönüşü — Tartışma

Beş karar noktası birbirinden bağımsız; karar-listesi biçimi kullanılıyor.

Ürün kuralı kullanıcı tarafından **konmuş** ve tartışmaya açık değil
(2026-09-20): *"auto complete öncesine ekranın dönebilmesi mesele"* ve
*"aşağıdan yukarıya kayarak geliyor ya, bunun tam tersi şekilde gitsinler;
oradaki geçiş sürecini de güzel tutalım"*. Aşağıdaki kararlar bunun **nasıl**
yapılacağına dair.

## Karar 1: Boşluk neyle doldurulur, ölçütü ne?

Bugün üstte kalan `origin` satır hiç çizilmiyor. Ölçüm (`context.md` → Kanıt)
listenin ittiği satırların scrollback'te durduğunu gösterdi.

- **1a — `fill = min(history_size, gap)`, geçmişin en yeni satırları.**
  Boşluk kadar geçmiş satırı, en yeniden geriye doğru.
  - *Artısı:* **sayaç** yok, delta aritmetiği yok. Boşluk zaten listenin ayak
    izi ve geçmişin en yeni satırları zaten listenin ittiği satırlar — ikisi
    aynı şeyin iki ucu, aralarına sayaç koymak ikinci bir doğruluk kaynağı
    yaratırdı. *Durumsuz değil:* Karar 3'ün bayrağının bir ömrü var; defter
    sayaçtan **bite** indi, ortadan kalkmadı.
  - *Artısı (2):* doldurma **hiçbir zaman yanlış satır göstermez.** Geçmişin
    en yeni satırı, tanımı gereği ekranın ilk satırının hemen üstündeki
    satırdır; süreklilik aritmetikten değil grid'in kendisinden geliyor. Tek
    "yanlış" hâl kullanıcının ekranı kasten sildiği durumdur → Karar 3.
  - *Eksisi:* "en yeni satırlar bir itmenin ürünü" varsayımı kasten
    temizlemede bozuluyor → Karar 3.
- **1b — borç sayacı.** Prompt çizilirken `history_size()` damgalanır, fark
  kadar doldurulur.
  - *Elenir, gerekçesi ölçülmüş:* Ctrl-C iptali **yeni bir prompt basıyor**
    (`context.md` → Kanıt 2, `precmd` koşuyor), yani "her prompt'ta borcu
    sıfırla" borcu tam da deliğin doğduğu anda sıfırlar ve `fill` sıfır
    çıkardı. Damgayı prompt'a bağlamayan bir ömür ise ne zaman sıfırlanacağını
    söyleyemez.
- **1c — `content_rows`'u oynatmak.** *Elenir:* denendi (`2fdca50`) ve geri
  alındı (`27a0b98`) — boşluğun yerini değiştiriyor, kendisini değil.

**Önerim: 1a.**

## Karar 2: Doldurulan satırlar sınırdan nasıl geçer?

Bu setin **asıl tasarım sorusu** ve animasyonu doğrudan belirliyor. Bugünkü
sözleşme (`session.rs:1774-1779`): sınırdan geçen her satır **pencere
satırı**, `0..rows`; `display_iter` yalnız görünür pencereyi veriyor ve
`frame()` scrollback'e ayrı bir yoldan hiç inmiyor (tek `history_size()`
çağrısı `scroll_locked`'ın kırpmasında, `session.rs:3153`).

- **2a — pencereyi `fill` kadar yukarı okumak.** `frame()` penceresini
  kaydırır; doldurulan satırlar sıradan satır olur, `content_rows` doğal
  olarak `rows`'a çıkar, `origin` sıfıra iner.
  - *Artısı:* `Cell.row` sözleşmesi hiç değişmiyor, eşleme zinciri kendiliğinden
    tutarlı.
  - *Eksisi — ölümcül:* **animasyonu öldürüyor.** Hareketi yapan şey artık
    `origin` değil pencerenin kayması, ve pencere satır bazlı, yani kesirli
    olamıyor: liste silinince içerik bir karede zıplıyor ve `motion` bunu hiç
    görmüyor. Kullanıcının istediği "kayarak" tam da bu kolda imkânsız.
  - *Eksisi (2):* seçim eşlemesi (`anchor`/`viewport_point`, `session.rs:3171`)
    ikinci bir ofset öğrenmek zorunda ve son karenin `fill`'i `Origin` gibi
    saklanmalı — iki yerde iki ofset.
  - *Eksisi (3):* `content_rows_come_from_the_visible_window_while_scrolled`
    (`session.rs:6283`) kırılır; o bekçi tam olarak `27a0b98`'in geri
    getirdiği şey. **Sonradan (2026-09-20):** o bekçinin *kaydırma* yarısı
    zaten teslimde çürüdü ve `content_rows_fill_the_window_while_scrolled`
    olarak tersine yazıldı — yaslama geçmiş penceresinde kalkıyor. Buradaki
    eksi yine de duruyor, çünkü kırılan yarı **yaslamanın kendisiydi**.
- **2b — `origin` korunur, boşluk ayrı listeler hâlinde boyanır.**
  `content_rows` ve `origin = rows - content_rows` aritmetiği **aynen kalır**;
  değişen yalnız üstteki `origin` satırın **ne boyandığı**. `frame()`
  doldurulan hücreleri ayrı verir, `bt-gpu` onları ötelemenin üstündeki alana
  çizer.
  - *Artısı:* animasyon **bedava ve doğru** — `origin` hâlâ hedefine süzülüyor,
    kesirli origin sayesinde en üstteki geçmiş satırı yarım giriyor;
    kullanıcının tarif ettiği etki bu.
  - *Artısı (2):* eşleme zinciri hiç değişmiyor; `content_rows`'un anlamı
    ("çizilen içerik") ve bekçileri (`session.rs:6283`) yerinde kalıyor —
    `27a0b98`'in maliyeti tekrarlanmıyor.
  - *Eksisi:* `bt-gpu`'da **ikinci bir dikey konumlandırma kaynağı** doğuyor.
    Bedeli "dar bir istisna" değil, **dock ölçüsünde**: `Frame`'de yeni
    listeler, sayaçlardan muafiyet, `renderer`'da kendi encode bloğu ve sıra
    kararı, kendi bekçileri.
  - *Eksisi (2):* doldurulan satırlarda **seçim bugün yanlış çalışır** — bkz.
    R1, çaresi bu setin içinde.

  **Mekanizma iki alt kola ayrılıyor ve seçim ölçüme bağlı.** Push anında
  aritmetik (`origin - fill + row`) **çalışmaz**: reponun kendi kaydı bunu
  söylüyor (`renderer.rs:715-719`, dock'un muafiyeti aritmetikle kurulamadı,
  çünkü `set_origin_rows` sink'ten sonra çağrılıyor) ve hareket karesi
  listeleri koruyup yalnız `origin_px`'i yeniden yazdığı için
  (`frame.rs:1774`) push anında pişmiş bir liste her kayma karesinde bayat
  olur — 200 ms boyunca ızgara süzülürken doldurma yerinde donar, dikiş tam
  kullanıcının baktığı anda görünür. Ayrıca `[0, origin_px)` aralığı ızgara
  viewport'unun **üstünde** kalıyor ve Metal orayı kırpıyor
  (`renderer.rs:643-647`), yani negatif satır da çare değil.
  - **2b-i — üçüncü `setViewport`** (dock emsalinin birebiri),
    `originY = origin_px - fill_px`.
    *Açık kalem:* kayma sırasında `origin_px < fill_px` ve `originY`
    **negatife iniyor**. `encode_dock`'un yanındaki kayıt negatif `originY`
    için "Metal'in doğrulamasına düşer — süreci öldüren bir istisna" diyor
    (`renderer.rs:746-749`); bunun **ölçülmüş mü varsayım mı** olduğu
    belirsiz ve phase-0'ın kanaryası bu.
  - **2b-ii — okuma anında çeviri**, caret emsali (`frame.rs:1215`,
    `instance.pos[1] -= self.origin_px`, gerekçesi yanındaki yorumda:
    "`origin_px` `push_caret` ile encode arasında hâlâ değişebilir").
    Negatif `originY` sorunu doğmuyor; bedeli kare başına `fill × cols`
    instance'ın pozisyonunun tazelenmesi.

  **Karar sırası:** phase-0 negatif `originY`'yi ölçer; meşruysa 2b-i
  (dock'un birebir kopyası, daha az kod), değilse 2b-ii.
- **2c — `Term::scroll_display` ile gerçek ofseti oynatmak.** *Elenir:*
  `frame()`'i durum değiştiren bir yola çevirir, kullanıcının tekerlek
  durumuyla karışır ve `send_input`'un dibe dönüşü (`session.rs:2800`)
  doldurmayı ilk tuşta siler.

**Önerim: 2b.** Kullanıcının iki isteğinden ikincisi (kayarak dönüş) 2a'da
yapısal olarak karşılanamıyor; 2b'de bedava geliyor.

## Karar 3: Kasten temizleme nereden anlaşılır?

Ölçüm (`context.md` → Kanıt 3 ve 4) Ctrl-L'nin de `history_size`'ı
büyüttüğünü gösterdi: `\e[H\e[2J` → `ClearMode::All` → `clear_viewport()` →
ekran geçmişe kayıyor. Doldurma bu hâlde de çalışırsa Ctrl-L hiç çalışmamış
görünür. Ayrım **zorunlu**, opsiyonel değil.

- **3a — PTY tarayıcısına CSI kolu.** Tarayıcı (`shell.rs:1500-1516`) bugün
  yalnız `ESC ]` açıyor, `ESC [` görünce `Ground`'a düşüyor. `ESC [` … `J`
  tanınır ve parametresi `2` ise "ekran kasten temizlendi" bayrağı kurulur.
  - *Artısı:* temizleme bir **terminal olayı**; sinyalini terminal katmanında
    okumak katman yönüne uyuyor. Kabuktan bağımsız: bash/fish geldiğinde ve
    entegrasyonsuz oturumda da çalışır.
  - *Eksisi:* tarayıcının "CSI bizi ilgilendirmiyor" sözleşmesi
    (`ScanState::Escape` doc'u) değişiyor. "Baytlara dokunmama" garantisi
    korunuyor (tarayıcı hiçbir kolda yazmıyor), ama **arıza kipi sessiz**:
    yeni `Csi` durumu vte'nin iptal kurallarını birebir taşımazsa
    (`ESC` → `Escape`, `0x18`/`0x1A` → `Ground`, parametre uzunluğuna
    `MAX_OSC_NUMBER` emsali tavan) bozuk bir CSI'da durum takılır ve peşinden
    gelen `ESC ] 133;…` yutulur — bloklar, bastırma ve dock **sessizce** ölür.
  - *Eksisi (2):* `\e[K`/`\e[m` yoğun akışta (vim, `less`) hızlı yol CSI
    başına birkaç bayt fazladan adımlıyor. **Ölçüm bekliyor**, sayı
    uydurulmaz.

**`3J` kolu yok, gerekmiyor** (kodda doğrulandı): `ClearMode::Saved` →
`clear_history()` (alacritty `term/mod.rs:1805`), yani çıplak `3J` sonrası
`history_size() == 0` ve `fill = min(0, gap) = 0` — kural kendi kendini
kapatıyor. `clear(1)` zaten arkasından `2J` gönderiyor (`context.md` →
Kanıt 3). Aynı gerekçe **RIS** (`\ec`, `tput reset`) için de geçerli:
`reset_state` → `grid.reset()` → `clear_history()`. Üçüncü bir kol önerisi
baştan kapalı.
- **3b — betikte `clear-screen` sarmalama.** *Elenir:* yanlış katman — ekranı
  temizleyen tek şey kabuk değil; entegrasyonsuz oturumda sinyal hiç doğmaz ve
  kusur sessizce geri gelir.
- **3c — alacritty `Handler`'ını sarmalamak.** *Elenir:* 100+ metot
  delegasyonu, kazancı tek çağrı.
- **3d — grid sezgisi** (imleç 0. satıra mı düştü, çıpanın mutlak konumu
  atladı mı). *Elenir:* tahmin sınıfı; 012 phase-4'ün `$KEYMAP` dersi aynı
  kapıya çıktı.

**Bayrağın ömrü (3a seçilirse):** `2J`'de kurulur, ekran **doğal yoldan
yeniden dolunca** (`content_rows == rows` **ve** alternatif ekranda değilken)
düşer. Üç kontrol:
Ctrl-L → ekran boş, bayrak kurulu → boşluk boş kalır ✓ ·
20 satır bas → ekran dolar → bayrak düşer → Tab/iptal → doldurma çalışır ✓ ·
Ctrl-L sonra `echo hi` → boşluk var, bayrak kurulu → boş kalır ✓.

**`!alt_screen` koşulu zorunlu, süs değil:** alternatif ekranda `content_rows`
**tanımı gereği** `rows` (`session.rs:2020-2021`, bekçi `:6270`). Onsuz şu
yol açılıyor: dolu ekranda Ctrl-L → bayrak kurulu → `vim` → ilk alt-ekran
karesi bayrağı **düşürüyor** → `:q` → Tab + Ctrl-C → doldurma kullanıcının
kasten sildiği ekranı geri boyuyor.

**İki yarış var ve ikincisi güvensiz.** Kurma yönü OSC 133 ile aynı sınıf:
tarayıcı bayrağı `Term` baytları uygulamadan önce yazıyor, bedeli en çok bir
kare için "temizlendi" demek ve o karede zaten boşluk yok — çare aranmaz.
**Temizleme yönü** farklı: ölçüt `content_rows` ve o `Term` kilidinin
**içinde** doğuyor, oysa yaprak kilit `Term`'ün altına giremez
(`CLAUDE.md`). Araya giren bir `2J` şu diziyi üretiyor — tarayıcı bayrağı
kurar → `frame()` kurulu okur → `Term` kilidi (baytlar henüz uygulanmadı,
`content_rows == rows`) → kilit bırakılır → bayrak **temizlenir** → `Term`
`2J`'yi uygular; sonraki karede bayrak temiz ve Ctrl-L'den önceki ekranın
tamamı geri boyanıyor. Pencere dar, vakası yaygın (dolu ekranda Ctrl-L).
**Karar gerekiyor:** nesil sayacı + compare-and-set mi, yoksa bayrağı `Term`
kilidinin altına koymak mı (okuyucu thread baytları uygulamak için o kilidi
zaten alıyor, ve "temizleme bir terminal olayı" gerekçesi de oraya işaret
ediyor). Her hâlde `make test-yaris` gerektiren bir phase.

**Takılı bayrağın sessiz bedeli:** 50 satırlık bir pencerede Ctrl-L'den sonra
ekran bir daha hiç dolmayabilir; bayrak oturum boyunca kurulu kalır ve
doldurma bir daha çalışmaz. Belirti bugünkü davranışın **birebir aynısı**,
yani kimse bozulduğunu anlamaz. Kabul ediliyor, `teslim.md`'de adıyla:
*özelliğin varlığı pencere boyuna ve çıktı uzunluğuna bağlı.*

**Adıyla konan kör nokta:** imleç tepedeyken gönderilen `\e[H \e[J`
(`ClearMode::Below`) kasten temizleme **sayılmaz** ve bant ekranı geri getirir.
Gerekçe phase-1b'de (2026-09-20) düzeltildi: bir dönem burada "geçmişi
büyütmez, dolayısıyla doldurmayı zaten tetiklemez" yazıyordu ve bu, bayrak
modeli gelmeden önceki **büyüme** ölçütüne aitti — bayrakta ölçüt `2J`'nin
basılması, büyüme değil, yani bayraksız temizleme bandı açık bırakır. Kör
nokta pratikte dar: zsh'in `clear-screen`'i, `clear(1)` ve `tput clear` üçü de
`2J` basıyor (`context.md` → Kanıt 3). Ayırt edilemezlik ancak `2J` basmayan
bir program çıkarsa doğuyor.

**Önerim: 3a.**

## Karar 4: Dönüşün animasyonu

Liste silinince `content_rows` daralıyor, öteleme hedefi **yükseliyor** —
bugün tam da snap'lenen yön. Kural `bt-gpu/src/motion.rs:437`'de tek guard:
`target <= slide.target` düşen hedefi kaydırıyor, yükselen `slide =>` koluna
düşüp `Slide`'ı hedefinde yeniden kuruyor.

- **4a — işaret kuralına `fill > 0` istisnası.** Doldurma varken yükselen
  hedef de süzülür.
  - *Artısı:* yeni animatör yok, yeni durma koşulu yok; kesirli origin
    `Frame::set_origin_rows`'ta zaten aygıt ızgarasına yuvarlanıyor.
  - *Artısı (2):* 011'in kararıyla çelişmiyor, onu **daraltıyor**. Snap'in
    gerekçesi `4c291ee`'de yazılı: "aşağı iniş *düşmesi* gibi okunuyor ve
    tuhaf — kabuğun vim'den çıkarken aşağı süzülmesi". Doldurma varken aşağı
    inen şey boşluk değil; üstten geçmiş **geliyor**, yani okunuşu 011'in
    hoşuna giden yöne dönüyor.
  - *Eksisi:* kural artık tek koşula bakmıyor; `motion` bir bit daha öğreniyor
    (`Cursor`'da bir alan). Katman sorunu değil: `Motion::sync` zaten
    `display_offset` ve `geometry` alıyor, `fill > 0` aynı sınıfta ve `Cursor`
    kare başına bir tane, yani bütçe kalemi de yok.

  **İstisna `!snap`'in İÇİNE yazılır** (`motion.rs:437`'deki guard'ın üçüncü
  terimi): `animated && !snap && (target <= slide.target || filled)`. Dışına
  yazılırsa `scrolling_and_geometry_snap_the_origin` kırılır ve **tekerlek ile
  pencere boyutlandırma animasyona başlar** — 5c tekerleği `bt-core` tarafında
  kesiyor ama geometri kolunu kesmiyor.

  **Kayma yalnız `fill` hedefle *aynı karede* doğuyorsa oluşur.** Guard hedefin
  yükseldiği karede bakıyor; o karede `fill == 0` ise `Slide` hedefinde
  yeniden kuruluyor ve sonradan gelen doldurma **hazır duran** boşluğa oturuyor.
  Karar 5 bu yüzden animasyonun ön koşulu.
- **4b — ikinci bir animatör.** *Elenir:* aynı ekseni iki animatör sürerdi.
- **4c — animasyonsuz.** *Elenir:* kullanıcının açık isteği.

011'in karar kaydı ve `docs/AYARLAR.md:506-516`'nın kullanıcı dili **bu sette
tadil edilir**; sessizce çelişilmez. Bekçi
`a_growing_origin_slides_and_a_shrinking_one_snaps` (`motion.rs:1649`)
korunur, yanına doldurma kolu eklenir.

**Önerim: 4a.**

## Karar 5: Doldurmanın kapsamı → **animasyonun ön koşulu**

İlk taslak "yalnız `Input` safhası" diyordu. Panel bunun **ölçülmüş iki delik
yolundan birini animasyonsuz bıraktığını** gösterdi (`context.md` → Kanıt 2):

- **Ctrl-C:** `\e[J` ile yeni prompt'un OSC 133 işaretleri aynı 149 baytta,
  safha `Input`, delik ve `fill` aynı karede → kayar ✓
- **Enter + kısa çıktı:** `\e[J` `preexec`'ten, yani **`Running`**
  safhasından geçiyor. Safha kapısı varsa o karede `fill == 0`, öteleme
  snap'ler, delik **anında** açılır; prompt dönünce geçmiş hazır boşluğa
  **pat diye** oturur. Kullanıcının istediği dönüş o kolda hiç olmaz.

Seçenekler:

- **5a — safha kapısı yok.** Koşul: `dock var ∧ !alt_screen ∧ bayrak temiz ∧
  display_offset == 0`.
  - *Artısı:* iki delik yolu da kayarak dönüyor; kural tek cümle ve safhaya
    bağlı değil.
  - *Eksisi:* `\e[K` ile satır silip yazan bir program (spinner) `content_rows`'u
    salındırınca `fill` de salınır — üstten satır girip çıkar. `sync_origin`'in
    doc'undaki "testere" bedeli görünür hâle gelebilir. Gözle kontrol kalemi.
- **5b — safha kapısı var** (`Input` + dock).
  - *Artısı:* regresyon yüzeyi en dar; salınım riski yapısal olarak kapalı,
    çünkü `Input`'ta içerik durağan.
  - *Eksisi:* Enter kolu animasyonsuz — kullanıcının kuralının yarısı
    karşılanmıyor.
- **5c — tekerlek** (her iki kolda da geçerli): doldurma yalnız
  `display_offset == 0` iken. Kullanıcı geçmişe kaydırıyorken bugünkü davranış
  aynen korunur (011 Karar 11: tekerlek snap'ler).

**Önerim: 5a + 5c.** Kullanıcının kuralı iki kolu da kapsıyor ("auto complete
öncesine ekranın dönebilmesi"); salınım riski ölçülmemiş bir tahmin, Enter
kolundaki "pop" ise ölçülmüş bir kusur. Salınım gözle kontrolde görülürse
kapı safhaya daraltılır — tersi (dar başlayıp genişletmek) kullanıcıya yarım
bir ürün gösterir.

## Karar 6: Doldurulan alanda seçim → **karar değil, zorunluluk**

Panel bunu bir kusur olarak buldu ve kodda doğrulandı. `view.rs:94`
(`((y / cell_h) as u16).min(rows - 1)`) orijinin üstündeki her tıklamayı
ızgaranın **0. satırına doyuruyor**; bekçisi bunu kendi diliyle söylüyor:
"Boş alanın tamamı 0. satıra yapışır" (`view.rs:741`).

Bugün görünmez, çünkü orası boş. Doldurma gelince kullanıcı oraya **metin
görüyor** ve üstünden sürüklüyor: çapa 0. satıra düşüyor, vurgu gözün
gördüğü yerde değil içeriğin tepesinde beliriyor. İlk taslak bunu
"doldurulan satırlar seçilemez" diye yazmıştı; **seçilemez değil, yanlış
seçiliyor** — ve `CLAUDE.md`'nin seçim sözleşmesi bunu adıyla yasaklıyor
("gözün gördüğü ile panonun verdiği ayrışmıyor").

**Çare:** `fill > 0` iken orijinin üstü `None` döner (reddedilir, kırpılmaz).
Kırpma boş alanda **istenen** davranış olduğu için koşulsuz kaldırılamaz;
bekçi (`view.rs:741`) yeniden yazılır, öncülü değişiyor.

Doldurulan satırların **seçilebilir olması** bu setin kapsamı dışında: o,
`Cell.row` sözleşmesini negatife açmak demek.

## Karar Noktaları

1. **Ölçüt:** `min(history, gap)` (1a) mı, borç sayacı (1b) mı?
2. **Sınırdan geçiş:** pencereyi kaydır (2a) mı, `origin` korunsun + ayrı
   liste (2b) mi? — animasyonu bu belirliyor. 2b seçilirse alt kol (2b-i /
   2b-ii) phase-0'ın ölçümüne bağlı.
3. **Kasten temizleme sinyali:** CSI kolu (3a) mı, betik (3b) mi? Ve bayrak
   nerede yaşıyor — nesil sayacı mı, `Term` kilidinin altı mı?
4. **Animasyon:** işaret kuralına istisna (4a) mı, snap (4c) mi?
5. **Kapsam:** safha kapısı yok (5a) mı, yalnız `Input` + dock (5b) mi? —
   Enter kolunun kayıp kayması buna bağlı.

## Muhakeme (2026-09-20)

Üç jüri paralel koştu (`opus`); altı karar noktasını, `context.md`'nin
ölçümlerini ve repo erişimini gördü.

| Mercek | Verdict |
|---|---|
| Sadelik | SORUNLU |
| Codebase-fit | SORUNLU |
| İşletme | SORUNLU |

Üçü de **yaklaşımı** (1a + 2b + 3a + 4a) onayladı; hiçbiri kol değiştirmeyi
önermedi. İtirazların tamamı mekanizma, kapsam ve sessiz bozulma üzerine.

**Üçünün ortak onayı — zorunlu olduğu doğrulananlar:**
- **CSI sinyali gerçekten gerekli.** Ctrl-L'nin `clear_viewport()`'u geçmişi
  dolu satır sayısı kadar büyütüyor; dört satırlık bir ekranda Ctrl-L ile
  Tab'ın dört `\n`'i grid verisinde **birebir aynı** izi bırakıyor. Yani 1b,
  3d ve "büyüme < rows" türü her eşik ölçülemez; niyet ancak `CSI 2 J` olarak
  geliyor.
- **Animasyon istisnası gerekli.** Son hâl Tab öncesinin aynısı olacaksa
  ızgara aşağı inmek zorunda, yani `origin` yükseliyor ve bugünkü kural onu
  snap'liyor.
- **Katman yönü korunuyor**, yeniden icat edilen mekanizma yok, `stride 32`
  assert'leri etkilenmiyor (yeni alan yok).

**Referans doğrulaması (Codebase-fit):** kod referanslarının tamamı tuttu
(`session.rs`, `shell.rs`, `motion.rs:437` guard'ı birebir, `frame.rs`,
`renderer.rs`, `view.rs`, alacritty kaynağı). Üç sapma **yalnız**
`docs/YOL-HARITASI.md`'de ve hepsi ~22 satır kaymış (356→378, 367→389,
193→203) — sebebi dosyanın commit'siz düzenlenmiş olması; aynı turda
düzeltildi.

### Kabul edilen itirazlar → tasarım değişikliği

- **2b'nin mekanizma cümlesi uygulanamazdı** (üç jüri de) → push anı aritmetik
  eleniyor; Karar 2 iki alt kola ayrıldı (üçüncü viewport / okuma anı çeviri)
  ve seçim phase-0'ın negatif `originY` ölçümüne bağlandı. Bedelin "dar bir
  istisna" değil dock ölçüsünde olduğu yazıldı.
- **Enter kolu animasyonsuz kalıyordu** (Codebase-fit) → Karar 5 tersine
  döndü: safha kapısı kaldırılıyor (5a), çünkü `\e[J` o kolda `Running`
  safhasından geçiyor ve `fill` hedefle aynı karede doğmuyor.
- **Doldurulan alanda seçim yanlış çalışıyordu** (İşletme R1) → Karar 6 olarak
  eklendi; "seçilemez" ifadesi yanlıştı, `CLAUDE.md`'nin seçim sözleşmesinin
  ihlaliydi.
- **Bayrak alternatif ekranda erken düşüyordu** (İşletme R2a) → ömre
  `!alt_screen` eklendi; `content_rows == rows` orada tanımı gereği doğru.
- **Temizleme yönündeki yarış güvensizdi** (İşletme R2b) → adıyla yazıldı ve
  karar noktası oldu (nesil sayacı / `Term` kilidinin altı); `make test-yaris`
  gerektiren phase.
- **`3J` kolu ölü ağırlıktı** (Sadelik + İşletme) → silindi; RIS (`\ec`) için
  de kol gerekmediği adıyla yazıldı. İkisi de kodda doğrulandı.
- **"Borç defteri yok" iddiası yarımdı** (Sadelik) → dürüstleştirildi: defter
  sayaçtan bite indi, ortadan kalkmadı.
- **İstisna `!snap`'in dışına yazılırsa tekerlek/geometri animasyona
  başlıyordu** (İşletme) → guard'ın üçüncü terimi olarak konumu yazıldı.
- **Takılı bayrak özelliği sessizce kapatıyor** (İşletme R2c) → kabul edilen
  bedel, `teslim.md`'ye yazılacak.
- **CSI kolunun arıza kipi sessiz** (Codebase-fit) → vte'nin iptal kuralları
  (ESC/0x18/0x1A/uzunluk tavanı) phase şartı olarak yazıldı; yoksa bozuk bir
  CSI OSC 133'ü yutar ve bloklar, bastırma, dock sessizce ölür.

### Plana taşınan, tasarımı değiştirmeyenler

- **Duman kapısı bu özelliğe yapısal olarak kör** (İşletme): süreli koşu
  `/bin/sh` koşuyor, dock yok → doldurma hiç tetiklenmiyor, `hucre=8 glif=6
  kural=15` ve `icerik`/`sessiz`/`kayma` oynamıyor. Tek koruma `motion`'da
  birim bekçi + gözle kontrol; "duman'a dock ekleyelim" önerisi baştan kapalı.
- **Geri alma şeridi** (İşletme): `fill_rows()` sıfır dönünce sınırın
  bugünküyle **bit bit** aynı olduğunu söyleyen bekçi — caret'in "yarıçap 0,
  hale 0" kolunun (016) aynı örüntüsü.
- **Phase sırası zorunlu:** bayrak (3a) **önce**, doldurma (1a/2b) sonra.
  Ters sırada tek commit boyunca Ctrl-L geri alınmış görünür.
- **Bekçi envanteri:** `session.rs:6283` korunur (2b'nin asıl kazancı);
  `motion.rs:1649` adı yalan olacağı için yeniden adlandırılır;
  `view.rs:741` öncülü öldüğü için yeniden yazılır; `frame.rs:2191`'e
  doldurma kardeşi eklenir.
- **Ölçüm bekleyenler:** `\r\r\n`'in +1 prompt kayması · doldurmalı karede
  sınır hücresi sayısı ve `Term` kilidi altındaki ek satır okuması · CSI
  kolunun yoğun akıştaki (vim, `less`) tarama maliyeti.

### Reddedilenler

- **Ötelemeyi pass uniform'una taşımak** (Codebase-fit'in kendi "tek satırlık
  alternatif"i) — jürinin kendisi "bu sete büyük" dedi; shader işi artı caret
  dikdörtgeninin yeniden türetilmesi. Kapsam dışına yazıldı.

## Karar (2026-09-20, kullanıcı onayı)

- **Seçilen — Karar 1: 1a**, `fill = min(history_size, gap)`. Sayaç yok;
  boşluk listenin ayak izi, geçmişin en yeni satırları listenin ittiği
  satırlar. **Reddedilen:** 1b (borç sayacı) — Ctrl-C yeni prompt bastığı için
  borcu tam da deliğin doğduğu anda sıfırlardı, ölçüldü; 1c (`content_rows`)
  — `2fdca50`'de denendi, `27a0b98`'de geri alındı.
- **Seçilen — Karar 2: 2b**, `origin` korunur, doldurma ayrı listelerle
  boyanır. Alt kol (2b-i üçüncü viewport / 2b-ii okuma anı çeviri)
  **phase-0'ın ölçümüne** bağlı: negatif `originY` Metal'de meşru mu?
  **Reddedilen:** 2a (pencereyi kaydırmak) — animasyonu yapısal olarak
  öldürüyor, pencere satır bazlı olduğu için kesirli olamıyor ve kullanıcının
  ikinci isteği o kolda karşılanamıyor; 2c (`scroll_display`) — `frame()`'i
  durum değiştiren yola çevirir, ilk tuşta silinir.
- **Seçilen — Karar 3: 3a**, PTY tarayıcısına CSI kolu, **yalnız `2J`**.
  Bayrağın nerede yaşadığı (nesil sayacı / `Term` kilidinin altı) phase
  kararı; her hâlde `make test-yaris`. **Reddedilen:** 3b (betikte
  `clear-screen`) — yanlış katman, entegrasyonsuz oturumda sinyal doğmaz;
  3c (`Handler` sarmalama) — 100+ metot; 3d (grid sezgisi) — tahmin sınıfı.
  **Düşen kollar:** `3J` ve RIS, ikisi de geçmişi zaten siliyor (kodda
  doğrulandı).
- **Seçilen — Karar 4: 4a**, işaret kuralına `fill > 0` istisnası, guard'ın
  **`!snap` içinde** üçüncü terimi. **Reddedilen:** 4b (ikinci animatör) —
  aynı ekseni iki animatör sürerdi; 4c (snap) — kullanıcının açık isteğinin
  tersi.
- **Seçilen — Karar 5: 5a + 5c**, safha kapısı **yok**
  (`dock ∧ !alt_screen ∧ bayrak temiz ∧ display_offset == 0`).
  **Reddedilen:** 5b (`Input` kapısı) — Enter kolunda `\e[J` `Running`
  safhasından geçiyor ve o karede `fill == 0` kalıyor, yani ölçülmüş iki
  delik yolundan biri animasyonsuz kalırdı. Salınım riski (spinner) ölçülmemiş
  bir tahmin, gözle kontrolde görülürse kapı daraltılır.
- **Seçilen — Karar 6:** doldurulan alanda tıklama **reddedilir** (`None`),
  kırpılmaz. Zorunluluk, tercih değil: `CLAUDE.md`'nin seçim sözleşmesi.
- **Kapsam kararı (kullanıcı, 2026-09-20):** set 5-6 phase ve üç crate olarak
  **tam** yazılıyor; "önce delik kapansın, animasyon sonra" ikiye bölmesi
  reddedildi — aradaki sürede geçiş sert kalır ve animasyon phase'i sonradan
  `motion`'a geri dönerdi.

## Kapsam dışı (bu sette değil)

- **Listeyi ızgaraya hiç düşürmemek** (overlay, aynanın altıncı kanalı) —
  `docs/YOL-HARITASI.md:389`'un "gerçek çare"si. Bu set onu **kapatmıyor,
  küçültüyor**: liste ekrandayken üstteki çıktı yine görünmez (iTerm de
  göstermiyor), yalnız liste kalktıktan sonra ekran Tab öncesine döner.
  Kapanıp kapanmayacağına kullanıcı gözle kontrolde karar verir.
- **Doldurulan satırların seçilebilmesi** — Karar 6'nın çizdiği sınır.
- **Ötelemeyi `setViewport`'tan alıp pass uniform'una taşımak** — üç yolu
  (ızgara, doldurma, caret) tek uzayda birleştirir ve `pos_at` gerçekten tek
  kaynak olurdu; shader işi artı caret dikdörtgeninin yeniden türetilmesi
  demek, bu sete **büyük** (panelin kendi değerlendirmesi).
- **Bastırmanın tazelik kapısının prompt hücrelerini sayması**
  (`docs/YOL-HARITASI.md:203`) — ayrı kök, ayrı karar.
