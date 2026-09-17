# Phase 2 — Yumuşak kayma

## Özet

Yeni satır geldiğinde içerik anında sıçramasın, yumuşak kaysın — imleç dipteki
satırında dururken geçmiş arkasından yukarı aksın.

_Requirements: R2.1, R2.2, R2.3, R2.4, R2.5, R2.6, R2.7_

## Değişiklikler

- **`crates/bt-gpu/src/motion.rs`** — origin `Motion`'ın **içine** girer.
  Dışarıda kalamaz: hasarsız karede tek uyku kararı `motion.settled()`
  (`link.rs:492-505`), yani ayrı bir animatör kayma ortasında link'i uyutur ve
  **içerik donar**.
  - **İmlecin hedefi ekran uzayına taşınır** (`row + origin_rows`) ve imleç
    origin ötelemesinden **muaf** tutulur. Gerekçe: Enter'da grid satırı anında
    r→r+1 olurken origin animasyonla gidiyor; ikisi ayrı uzayda kalırsa imleç
    bir satır aşağı düşüp geri biner. Ekran uzayında hedef **hiç değişmiyor**.
  - **Hareketi Azalt'ta origin snap'ler**, `Mode::Fade` **değil**: fade "konum
    anında hedefte, değişen şey opaklık" demek ve her yeni satırda bütün
    ekranın belirmesi, indirgemeye çalıştığı hareketten beter olurdu.
    "İndirgemenin tek yeri `bt-gpu::motion`" kuralı yerinde kalıyor — **yer**
    aynı, **kip** iki.
  - **`display_offset` oynadıysa origin snap'ler** (`motion.rs:245`'in
    kuralının ikizi): tekerlek parmağı takip eder, 008 Karar 5 ayakta kalır.
    `clear`'dan sonra geçmişte gezinen kullanıcı bunu görüyor.
  - **Durma koşulu** yazılır ve girdisinin **monoton olmadığı** hesaba katılır:
    imleci yukarı taşıyıp alt satırı `\e[K` ile silen bir program
    `content_rows`'u daraltıp genişletebilir.
- **`crates/bt-gpu/src/link.rs`** — hasarsız kolda origin'in **ikinci yazma
  noktası** açılır (`move_cursor`'ın yanı). O kolda `frame()` de `clear` de
  çağrılmıyor, yani origin **korunuyor** ama değişemiyor; animasyonun tanımı
  iki içerik karesi arasında değişmektir. Ayrıca `kayma_frames` sayacı:
  yalnız origin animatörü yerleşmemişken artar.
- **`crates/bt-shell/src/app.rs`** — jeton satırına **`kayma=`** eklenir.
  `hareket` yalnız imleç animatörünün tanığı kalır; iki kaynak ayırt
  edilebilir olur. **Jeton silinmez, eklenir** — okuyan taraf tanımadığını
  atlar. Kapı `hareket > 0` kalır.
- **`docs/AYARLAR.md`** — `[motion]` bölümündeki **"İmlecin kendi hareketi
  kayar; altındaki ızgaranın hareketi kaymaz"** cümlesi ayrılır: tekerlekle
  kaydırma, boyutlandırma ve punto değişimi **kaymıyor** (değişmedi), içerik
  büyümesinin kaldırması **kayıyor** (yeni). `"snap"`ın "hareketi tamamen
  kapatmanın yolu bu" cümlesi **ayakta** — kayma da `cursor_motion`'ı izliyor.
- **`crates/bt-core/src/session.rs`** — `Cursor::display_offset`'in doc'u
  (008 Karar 5) aynı ayrımı yansıtacak biçimde güncellenir.
- **`CLAUDE.md`** — "her animasyonu 90 ms'lik bir **belirmeye** indirir"
  cümlesi artık yalnız imleç için doğru; origin'in indirgemesi snap.

## Kabul

- Ekran dolmadan Enter'a basınca içerik yumuşak kayıyor; **imleç sıçramıyor**
  (ne aşağı düşüyor ne geri biniyor).
- `cursor_motion = "snap"` iken içerik de anında yerine gidiyor.
- Hareketi Azalt açıkken içerik **snap**'liyor, ekran belirmiyor.
- Tekerlekle geçmişte gezinirken içerik kaymıyor (snap).
- Kayma ortasında tıklama doğru hücreyi seçiyor (R2.7 bekçisi).
- `make duman` yeşil: `hareket > 0` (imleç, phase-0'ın reçetesinden),
  `kayma` jetonu basılıyor, `icerik ≤ IDLE_FRAME_LIMIT`,
  `sessiz ≥ QUIET_FLOOR`.
- Kayma yerleştikten sonra **kare istenmiyor** (boşta sıfır kare).

## Uygulama Notları

- **Öteleme `State`'in üçüncü ekseni olmadı, ayrı bir tip oldu** (`Slide`).
  Sebep kip: imleç Hareketi Azalt'ta **belirirken** öteleme **snap**'liyor
  (R2.3), yani `since_move` (belirmenin saati) ötelemenin alanı değil.
  Paylaşılan şey fizik: kübik yavaşlama, yayın taşma kırpması ve eşik yolu
  üç serbest fonksiyona çıkarıldı (`ease_axis`, `spring_axis`, `axis_settled`)
  ve `State` de onlardan geçiyor. Taşma kırpmasının **tek kopya** kalması
  önemliydi — ikinci bir kopya, hedefi aşan bir ızgaranın geri tepmesini
  sessizce geri getirirdi.
- **Piksel aygıt ızgarasına yuvarlanıyor** (`Frame::set_origin_rows`) ve bu
  plandaki bir kalem değildi, yolun şeklinden çıktı: link'in "hasar yok" dalı
  animasyonun **yerleştiği** kareyi hiç çizmeden uyuyor, yani ekranda kalan son
  kare yerleşmeden bir adım öncesi. İmleç için bu yarım pikselin altında bir
  fark (`POS_EPSILON`), **bütün metin** için her Enter'dan sonra yarım piksel
  kaymış — yani bulanık — bir ekran olurdu. Yuvarlama kaymayı da
  keskinleştiriyor: metin tam piksel adımlarıyla ilerliyor. Yerleşmiş hâlde
  kimlik, yani phase-1'in üç bekçisi dokunulmadan geçti.
  Bekçi `a_sliding_origin_lands_on_whole_device_pixels`.
- **İmlecin ekran uzayına taşınması `push_cursor`'un işaretini çevirdi.**
  phase-1'de instance ham `pos`, dikdörtgen `pos + origin` idi; bugün instance
  `pos − origin` (viewport onu geri veriyor), dikdörtgen ham `pos`. Aritmetik
  aynı pikseli veriyor, değişen tek şey `at`'in hangi uzayda geldiği. Bekçi
  yeniden adlandırıldı
  (`the_cursor_rect_keeps_the_screen_row_and_the_instance_gives_the_origin_back`)
  ve sayıları **aynı** kaldı — testin hedefi `[0,2]`'den `[0,4]`'e, yani grid
  satırı + ötelemeye çevrildi.
- **`sync`'in `!visible` erken dönüşü ötelemeyi atlıyordu.** Öteleme artık
  guard'dan **önce** ve koşulsuz kuruluyor: imleci gizleyip çıktı akıtan bir
  betikte (vim değil, `tput civis` ile çalışan bir betik) içerik yanlış yerde
  donardı. İmlecin görünürlüğü ızgaranın nerede durduğuna karar veremez.
  Bekçi `a_hidden_cursor_does_not_freeze_the_origin`.
- **Geometri de ötelemeyi snap'liyor**, yalnız tekerlek değil (R2.6). Plan
  yalnız `display_offset`'i sayıyordu ama `docs/AYARLAR.md`'nin bu phase'de
  ayrılan cümlesi ("boyutlandırma ve punto değişimi kaymıyor") tetiği zaten
  söylüyordu: pencereyi yeniden boyutlandırmak `rows`'u oynatıyor, yani hedef
  sıçrıyor ve animasyon onu içerik büyümesi sanardı.
- **`kayma=` kapıya girmedi ve girmemesi iyi oldu: ölçüldü, `0`.** Reçete
  (`bt_core::smoke_shell`) tek `printf` + `\033[2G`, yani iki içerik karesinin
  ilki geometri snap'i, ikincisi **yalnız sütun** değişimi — doluluk
  (`content_rows`) hiç oynamıyor, dolayısıyla kayma da hiç doğmuyor. Jetonu
  kapıya yazmış olsaydık kod doğruyken kırmızı düşerdi. Satırda olmasının
  sebebi tanı: kırmızı bir koşuda `hareket` ile birlikte okununca hangi
  animatörün yerleşmediği ayırt ediliyor.
  **Kapsam kalemi, adıyla:** kayma yolunun gerçek pencerede koşan bir bekçisi
  **yok** — duman reçetesi onu tetiklemiyor, kanıtı birim sınamaları ve göz
  kontrolü. Reçeteyi kaymayı tetikleyecek biçimde değiştirmek `hucre/glif/kural`
  ve `sessiz`'in ölçülmüş sözleşmesine dokunurdu (R3.1: ayrı commit), o yüzden
  bu phase'de yapılmadı.
- **Yeni satır alt kenardan yükseliyor.** Tek `setViewport` (R1.1) dört listeyi
  birden kaydırdığı için kayma boyunca öteleme hedefinden büyük kalıyor ve en
  alt satırın bir kısmı o karelerde pencerenin altında oluyor. "Yeni satır
  yerinde belirsin, ötekiler kaysın" bu mimaride temsil edilebilir bir şey
  değil; tespit `set_origin`'in doc'una yazıldı.
- **Bir sınama kendi varsayımımı çürüttü.** Hareketi Azalt bekçisinin ilk hâli
  Enter'ın imleci beliritmesini bekliyordu; oysa R2.1'in tamamı tam olarak
  bunun **olmaması**: Enter'da imlecin ekran satırı hiç değişmiyor, yani
  belirme de doğmuyor. Sınama imlece sütun da değiştirtecek biçimde
  düzeltildi — düşen sınama koddaki değil testteki hatayı gösterdi.

## Yayın Etkisi

- **Ölçüm bekliyor: kayma animasyonunun yerleşme süresi.** Duman koşusu onu
  **göremiyor** (reçete kayma üretmiyor, `kayma=0`), yani bu iddianın tek
  kapatıcısı `/measure`. `sessiz` bandına etkisi ise bu koşuda **yok**:
  1748,29 ms ölçüldü, taban 870 ms.
- **Jeton sözleşmesi büyüyor:** `kayma=` eklendi, hiçbir jeton silinmedi.
  Anahtar Türkçe ve donmuş, değer İngilizce.
- **Belge:** `docs/AYARLAR.md` (`[motion]`), `CLAUDE.md`'nin indirgeme cümlesi
  ve `Cursor::display_offset`'in doc'u **aynı commit'te** düzeldi — üçü de bu
  phase olmadan doğruydu. Yanlarına phase dosyasının saymadığı ama aynı
  kuralın yakaladığı dördü eklendi: `CLAUDE.md`'nin "Bugünkü hâl"
  paragrafındaki **animasyonsuz** cümlesi ve duman jeton satırı (`kayma=`),
  `motion.rs`'in modül başlığı ile `Motion::position`'ın "hücre birimi"
  cümlesi, `Frame::origin_px`'in "hareket karesinde korunuyor" cümlesi (artık
  ikinci yazma noktası onu tazeliyor) ve `push_cursor`'ın iki uzay paragrafı.
- **Ayar şeması:** **değişmiyor**. Yeni anahtar yok; kayma `cursor_motion`'ı
  izliyor. (`feed_lift` bilinçli olarak **reddedildi** — `discussion.md` →
  Muhakeme 2. tur, kabul 7.)
- shader: `.metal` değişmiyor. terminfo, tema, shell entegrasyonu, app bundle:
  **yok**. Yeni bağımlılık: **yok**.
- **Riskli phase tetiği yok** ve bu bilinçli: `Motion` ana thread'e bağlı bir
  `Cell` (`link.rs` callback'i ana run loop'ta), yani `make test-yaris`'in
  "paylaşılan durum" koşulu tetiklenmiyor; `.metal` ve `Cargo.lock`
  değişmiyor. Hareket saatine ve boşta sıfır kare sözleşmesine dokunan bu
  phase'i **set sonundaki kapı** (`/code-review` + `/audit`) karşılıyor —
  `proje.md`'nin "geri kalan her şey set sonunu bekler" kuralı.

## Checklist

- [x] Origin `Motion` içinde (`Slide`), `settled()` kapısında;
      `cursor_settled`/`origin_settled` ikisini ayrı sorabiliyor
- [x] İmlecin hedefi ekran uzayında (`row + origin_rows`); Enter'da sıçrama yok
      (`the_cursor_does_not_move_while_the_origin_slides`)
- [x] Hareketi Azalt'ta snap (`Motion::origin_mode`); `display_offset` **ve**
      geometri oynadıysa snap
- [x] Durma koşulu yazılı (`Slide::settled`), `content_rows`'un monoton
      olmadığı hesaba katıldı (`a_shrinking_content_settles_too`)
- [x] Hasarsız kolda origin'in ikinci yazma noktası (`set_origin`, tek
      fonksiyon iki çağrı yeri)
- [x] `kayma=` jetonu; `hareket` saf imleç tanığı. Kapı `hareket > 0` kaldı,
      `kayma` sayaç (eşiği ölçülmedi)
- [x] Test: kayma ortasında `point_to_cell` tutarlı (R2.7) —
      `the_origin_shifts_the_grid_down_and_the_blank_area_clamps`'in yarım
      hücrelik kolu; tutarlılığın kendisi **yapısal** (tek `set_origin` hem
      viewport'u hem `Origin`'i yazıyor) ve sınama fonksiyonun tam satır
      varsayımı olmadığını çiviliyor
- [x] Test: kayma yerleştikten sonra kare istenmiyor
      (`the_origin_settles_and_then_lets_the_link_sleep`)
- [x] `docs/AYARLAR.md`, `CLAUDE.md` ve `Cursor::display_offset` doc'u
      düzeltildi (yanlarında dört doc borcu daha, bkz. Yayın Etkisi)
- [x] Doğrulama geçti: `make hepsi` yeşil (exit 0) ve `make duman`
      kullanıcının gerçek penceresinde yeşil — `kare=29 hucre=8 glif=6
      kural=15 icerik=2 hareket=27 kayma=0 sessiz=1748.29ms kapanis=clean`.
      Üç sayaç oynamadı, `icerik` phase-0/1'deki `2`'de kaldı ve `sessiz`
      tabanın (870 ms) iki katının üstünde. `kayma=0`'ın gerekçesi Uygulama
      Notları'nda — reçete ötelemeyi hiç oynatmıyor
- [x] Yayın etkisi yazıldı ("ölçüm bekliyor" satırı dahil)
- [x] Göz kontrolü (kullanıcı, gerçek pencere): Enter, `clear`, vim giriş/çıkış,
      tekerlek, kayma ortasında tıklama, `cursor_motion = "snap"`, Hareketi
      Azalt — yedisi de beklendiği gibi; tasarımı değiştiren bulgu yok
      (010'un aksine)
