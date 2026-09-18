# İmleç stilleri ve blink

## Hedef

İmleç, uygulamanın istediği **şekli** alsın (blok, alt çizgi, dikey çubuk) ve
istenirse **yanıp sönsün** — sönme, boşta sıfır kare sözleşmesini bozmadan:
saniyede iki kare, `bt-core`'a hiç uğramayan hareket kareleri ve adlandırılmış
bir durma koşuluyla.

## Gereksinimler

- **R1 — Şekil sınırı geçer.** DECSCUSR'ın üç şekli `Cursor` ile `bt-core`'dan
  çıkar.
  - **R1.1** — `bt-core` **kendi** enum'unu verir; alacritty tipi `pub` API'de
    görünmez (`CLAUDE.md` → bağımlılık kapsülleme).
  - **R1.2** — `Hidden` taşınmaz: `Cursor::visible` onu zaten tüketiyor
    (`session.rs:1401`); iki yerde temsil edilen bir gerçek ayrışabilir.
  - **R1.3** — `HollowBlock` **adlandırılmış kararla** bloğa düşer; odak
    sınırda olmadan içi boş imleç çizilemez.
- **R2 — Çizim tek gerçek kalır.** Boyanan dörtlü (`Caret::instance`) ile ters
  çevirme dikdörtgeni (`CursorBlock.rect`) **birlikte** daralır.
  - **R2.1** — Daraltma instance kurulurken yapılır (`grid_caret`/`dock_caret`),
    `push_caret` içinde **değil**: yuva seçimi (`frame.rs:674`) hücre ayak
    izine bakıyor ve daraltma ondan önce olursa underline caret'i dock'un opak
    zemininin altında kalır.
  - **R2.2** — Kalınlık `bt_atlas`'ın alt çizgi metriğinden gelir (chevron
    emsali); yeni bir sayı uydurulmaz.
  - **R2.3** — Shader'a şekil bayrağı eklenmez; `CursorBlock`'un "bayrak ile
    dikdörtgen ayrışabilen iki gerçek olurdu" reddi korunur.
- **R3 — Şekil hareket karesinde kaybolmaz.** `Frame` şekli kendi alanında
  tutar: `push_caret` yazar, `move_caret` korur. Kaybolması **temsil edilemez**
  olmalı — yoksa beam ilk sönüp yanışta bloğa dönerdi.
- **R4 — Ayarlar.** `[terminal] cursor` (varsayılan `"block"`) ve
  `[terminal] cursor_blink` (`"auto" | "on" | "off"`, varsayılan `"off"`).
  Bölüm seçimi kodu aynalıyor (ikisi de `TerminalOptions`'a iniyor) ve
  anahtar adları referansınkiler; bölüm **zaten var** (`scrollback`), yani
  ayrıştırıcıya yeni bir bölüm kolu eklenmiyor.
  - **R4.1** — İkisi de `TerminalOptions`'a iner; `Changes`'e **yeni alan
    eklenmez** (`Changes::terminal` zaten kapıyı tutuyor).
  - **R4.2** — Üç değerli `cursor_blink` `default_cursor_style`'a **sığmaz** ve
    birleşme yeri yazılıdır: `"auto"` orada çözülür (`blinking: false`,
    uygulama açar), `"on"` ve `"off"` ise **ezmedir** — `\e[2 q` gelse de
    `"on"` yanar, `\e[5 q` gelse de `"off"` söner. Ezme `frame()`'de,
    `Term::cursor_style()` okunduktan sonra `Cursor.blink` yazılırken uygulanır.
    Yazılmazsa phase-2 `default_cursor_style.blinking = true` yazıp `"on"`'u
    sessizce `"auto"`'ya indirir.
  - **R4.3** — Kabul edilmeyen değer kendi anahtarını değiştirmez ve tanı
    bırakır (`cursor_motion` örüntüsü; `osc52` istisnası buraya **geçmez**).
  - **R4.4** — phase-1 `cursor_blink`'i şemaya da `docs/AYARLAR.md`'ye de
    **koymaz**: okunmayan ama belgelenmiş bir anahtar en kötü ara durumdur.
- **R5 — Blink bir hareket karesidir.** `Waker::resume()` hasar **dikmez**;
  blink `icerik=`'i artırmaz ve `Term` kilidine girmez.
- **R6 — Uyku testi üç soru sorar.** Faz değişimi tek atımlıktır ve
  `motion.settled()`'ın erken dönüşünden **önce** tüketilir; aksi hâlde
  `resume()` kare üretmeyen bir uyan/uyu fırdöndüsü yaratır
  (`link.rs:652-660`).
- **R7 — Saat son tarih tutar, süre değil.** Her uyku
  `min(içerik deadline, bir sonraki faz)` kurar.
  - **R7.1** — 013'ün canlı sayacı blink açıkken **doğru tikler**. Bugünkü
    süre temelli kurulum onu sonsuza iterdi (context.md → Kanıt).
  - **R7.2** — Geçmişte kalan son tarih sıfıra doyar; sıfır süreli bir
    callback döngüye sokmaz (`shell::next_tick`'in savunması emsal).
  - **R7.3** — **`next_tick == None` saklanan son tarihi temizler.** Bugün
    `arm_clock` değeri her seferinde `last_cursor`'dan taze okuduğu için `None`
    saati kendiliğinden söndürüyor; saklanan bir deadline'a geçince o kendilik
    kayboluyor ve biten komutun bayat son tarihi bir kare daha isterdi. 013'ün
    kapısında düzeltilen kusurun (`durma koşulu bir kare geç işliyordu`) yeni
    kılığı budur ve tek tanığı elle kontroldü.
- **R8 — Blink fazı mutlak son tarihtir**, `dt` biriktirmez. `Motion`'ın
  `advance` biçimi taklit edilseydi `DT_MAX = 0.1` kırpması 500 ms'lik uykuyu
  100 ms sayar ve arada dört **birebir aynı** kare çizilirdi.
- **R9 — Durma koşulu adlandırılmıştır ve fazı "açık"a bırakır.** Blink şu
  hâllerde durur: uygulama kapatır (`"auto"`), imleç gizlenir, pencere örtülür
  (`Gate`), son içerik karesinden N saniye geçer.
  - **R9.1** — Durma **fazı "açık"a zorlar ve son bir kare çizdirir.** Sönük
    fazda durursa imleç bir sonraki hasara kadar kaybolur ve belirtisi
    sessizdir.
  - **R9.2** — Hareketsizliğin saati `last_frame_at` **olamaz**. O damga
    hareket kolunun `Ok` dalında da yazılıyor (`link.rs:730`) ve blink kareleri
    tam o koldan geçiyor: her yarım periyotta "aktivite" görünür, zaman aşımı
    **hiç ateşlemez** ve blink hiç durmaz. Belirtisi sessiz. Faz tipinin kendi
    "son içerik karesi" damgası olur ve yalnız içerik kolunda yazılır.
- **R10 — Hareketi Azalt blink'i kapatır.** Erişilebilirlik ayarı animasyon
  *eklemez* (`CLAUDE.md`). Yan kazanç yapısal: `Mode::Fade` ile blink birbirini
  dışlar, yani `alpha()` kanalına **ikinci yazar doğmaz** ve bir birleştirme
  kuralı icat edilmez.
- **R11 — Kapı bozulmaz.**
  - **R11.1** — `make duman` yeşil kalır (hermetik koşu ayar okumaz, reçete
    `/bin/sh` koşup DECSCUSR göndermez).
  - **R11.2** — `term_config_keeps_every_other_field` fixture'ı **varsayılan
    olmayan** bir şekil taşır; varsayılanla doldurulursa guard'ın vaadi
    sessizce yalan olur.
- **R13 — İmlecin kendi tema rolü var.** Caret rengini `accent`'ten değil
  `cursor` rolünden alır; koşan komut bloğunun şeridi `accent`'te kalır.
  - **R13.1** — Tema dosyası **geriye dönük okunur**: anahtar opsiyonel ve
    eksikse `accent`'e düşer, yani bugün yazılmış bir kullanıcı teması
    değişmeden çalışır.
  - **R13.2** — ANSI 258 (imleç rengi yuvası) yeni rolü söyler; bugün
    `accent`'e takma addı.
  - **R13.3** — Gömülü iki tema (`bateri`, `bateri-light`) rolü **altın**
    tonuyla dolduruyor; değer bir zevk kararı, ölçüm değil.
- **R12 — Sözleşme kodla aynı commit'te güncellenir:** `link.rs` modül
  başlığı (saatin **iki tadı**), `Waker` ve `requests` doc'ları,
  `Counters::motion` doc'u, `Cursor::next_tick`'in "'Ne zaman' sorusunun cevabı
  burada" cümlesi, `CLAUDE.md` ve `docs/AYARLAR.md`.

## Yaklaşım

**Şekil sınırdan, faz boyamada, tetik saatte.**

1. **Şekil `Cursor`'a bir alan olarak girer** ve `term_config`
   `default_cursor_style`'ı ayardan kurar. `Term::cursor_style()` **tek
   çağrıda** hem şekli hem blink bitini veriyor, yani phase-2'nin girdisi
   phase-1'de bedavaya doğuyor.
2. **Çizim şekli iki yerde birden daraltır** ve daraltmayı yuva seçiminden
   önce yapar; `Frame` şekli saklar, böylece hareket karesi onu koruyabilir.
3. **Blink `Waker`'ın ikinci kolundan sürülür.** Saat tek mekanizma kalır ama
   **iki tadı** olur: içerik tadı (`wake()`, hasar diker, `icerik=` sayar) ve
   hareket tadı (`resume()`, hasar dikmez, mevcut hareket kolunu kullanır).
   Hangisinin çağrılacağını dolan deadline söyler.
4. **Faz `bt-gpu`'da saf bir tipte yaşar** (`Gate`/`Motion` emsali: ObjC'siz,
   kilitsiz, gerçek pencere olmadan sınanabilir). `bt-core` yalnız blink'in
   açık olup olmadığını söyler.

## Kararlar

1. **Blink içerik değil hareket karesidir** ve bu 013'te yazılmış bir kararın
   uygulanmasıdır: `link.rs:18-23` yeni animasyonları (blink adıyla anılıyor)
   `Waker`'dan men ediyor. Yasağı aşan şey `resume()`'un hasar dikmemesi.
2. **Saat tek mekanizma, iki tat.** Dördüncü bir kare sebebi doğmuyor; saatin
   ne yaptığı değişiyor. Sözleşmenin "üç sebep" cümlesi duruyor.
3. **Faz `bt-gpu`'da, zorunluluktan.** Hareket karesi `bt-core`'a hiç gitmiyor
   (`link.rs:652-745`: ne `session.frame()` var ne `Term` kilidi), yani orada
   üretilen bir faz o kola ulaşamaz. Emsali `CursorMotion`: ayar hangi stil
   olduğunu söyler, süreler ve katsayılar `bt_gpu::motion`'ındır.
4. **Durma koşulu S1 + S2** (uygulama + hareketsizlik). Gerekçe ve reddedilen
   S3 `discussion.md → Karar`'da.
5. **`cursor_blink` üç değerli, varsayılan `"off"`.** Pencerenin kalıcı olarak
   boşta-değil olması kullanıcının **seçtiği** bir şey olmalı.
5b. **Anahtarlar `[terminal]` altında** ve adları referansınkiler (`cursor`,
   `cursor_blink`). Bölüm kodu aynalıyor: ikisi de `TerminalOptions`'a iniyor
   ve `Changes::terminal` zaten o kapıyı tutuyor. Referansın kendi yerleşimi
   `[typography]` ama bizde o bölüm yok (`[font]` var) ve imleç şekli bir font
   özelliği değil; kendi `[cursor]` bölümümüzü açmak da imleç ayarlarını üç
   yere dağıtırdı (`cursor_motion` `[motion]`'da kalıyor, anahtar silinmez).
6. **Yeni jeton eklenmiyor.** Blink karesinin CPU tanığı yok ve bu **yazılıyor**:
   varsayılan kapalıyken kapının hiçbir katı bozuk bir blink'i görmez — koruma
   bir jeton değil **varsayılanın kendisi**. Emsal `app.rs:1185-1196`
   (`cpu_elenen=` ölçülmüş ihtiyaç beklemeden atılmadı) ve jeton satırı geri
   alınamaz (**jeton silinmez**). `docs/YOL-HARITASI.md:112-124`'teki borcun
   **kapsamı** büyüyor ("meşru periyodik kare"), **vadesi** gelmiyor.
7. **Duman muafiyeti 013'ünkinden zayıf ve bu kayda geçiyor.** 013'te bütün bir
   alt sistem yoktu (OSC 133 hiç basılmıyor); burada reçetede dört baytlık bir
   kaçış dizisi yok — bir `printf` uzaklıkta. Kabul edilebilir, çünkü kırılırsa
   **sesli** kırılıyor (`sessiz < QUIET_FLOOR` → kırmızı).
8. **Hareketi Azalt blink'i kapatır** (R10). Karar erişilebilirlik sözünden
   çıktı, sadeleşme yan kazanç.
10. **İmlecin kendi rolü var, `accent` paylaşılmıyor** (R13). Caret ile koşan
   komut şeridi aynı değerden beslenince "imleci altın yap" isteği şeridi de
   altın yapıyordu. Ayrım ayrıca ANSI 258'i gerçek bir role bağlıyor ve
   ilerideki OSC 12'yi (terminfo'da `Cs` ilan ediyoruz, uygulamıyoruz) doğal
   kılıyor. Bedeli yazılı: "tema = sekiz rol" cümlesi **dokuza** çıkıyor.
9. **Blink'in görüntüsü (sert / sınırlı geçiş) referans bakışından sonra
   kararlaşır** — phase-2'nin ön koşulu, kullanıcıda. Sürekli nefes mimari
   olarak elendi. *Bakışın sonucu phase-2'nin **şeklini** değiştirebilir:*
   sınırlı geçiş seçilirse her flip kısa bir ekran-hızı animasyon ister, yani
   `Motion`'a girer, `hareket=` sayar ve R10'un "ikinci alfa yazarı doğmaz"
   kazancıyla çarpışır. Bu yüzden `phase-2.md` **bakış bitmeden yazılmaz**.
10. **Faz zamana bağlıdır; içerik karesi ona dokunmaz, ama caret'in
   hareketi dokunur** *(ikinci yarısı canlı kullanımdan geldi, 2026-09-19)*.
   Her içerik karesinde açığa çekmek 013 ile çarpışıyordu — koşan komutun
   sayacı saniyede bir içerik karesi üretiyor ve blink'in ritmi komut
   koşarken bozulurdu. Ama hiç dokunmamak da yanlıştı: yazarken imleç
   sönüyordu ve bu her editörün tersi. Ölçüt **caret'in hedefinin
   kıpırdaması**: sayaç onu kıpırdatmıyor, yazmak kıpırdatıyor. Tuş vuruşunun
   kendisi tetik değil — o `bt-shell`→`bt-gpu` sinyali isterdi.

10b. *(Eski hâli, tarihli kayıt olarak)* **Faz yalnız zamana bağlıdır.** Alternatifi
   "her içerik karesinde fazı açığa çek" idi (tuşa basınca imleç görünsün) ve
   013 ile çarpışıyor: koşan komutun sayacı saniyede bir içerik karesi üretiyor,
   yani blink'in ritmi komut boyunca bozulurdu. Bedeli kabul ediliyor ve
   adlandırılıyor: karanlık fazda basılan tuş imleci en çok yarım periyot
   bekletir. Girdiye bağlı sıfırlama `bt-shell`→`bt-gpu` sinyali ister ve
   maliyeti S3 ile aynı sınıfta — o da sonraki sete.

## Kapsam Dışı

- **İçi boş imleç ve odak.** `HollowBlock` bloğa düşüyor; odağın sınırdan
  geçmesi ve odaksız pencerenin içi boş caret'i ayrı bir set (008'de de
  kapsam dışıydı).
- **Caret'in köşe yarıçapı ve gölgesi.** Kullanıcı istedi (2026-09-18) ve
  **kendi setine** gidiyor. Gerekçe risk sınıfı: caret bugün `cell_bg`'nin
  düz dörtgen pipeline'ında bir `Instance` ve o tampon **ızgara boyunda**
  (`#[repr(C)]`, iki taraflı `static_assert`, stride 32) — tek bir caret için
  oraya alan eklemek bütün arka plan hücrelerine bindirmek olurdu. Doğru yol
  caret'e kendi küçük pipeline'ını vermek, yani yeni bir `.metal` fonksiyonu
  ve yeni bir düzen sözleşmesi: 014'ün hiçbir işi shader'a dokunmuyor, bu
  dokunuyor. Referansta emsali var (`ShapeInstance` + `fill`/`stroke`/
  `strokeW`/`radius`/`squareCorners`, `docs/ARASTIRMA.md` → İmleç) ve aynı
  yetenek **içi boş imleci** de getiriyor — üçü tek işin sonucu.
- **İmleç rengi (OSC 12).** İlan ettiğimiz terminfo `Cs`/`Cr` taşıyor ve
  `Event::ColorRequest` sorguyu yanıtlıyor, ama caret'i boyayan yol rengi
  `theme.accent_linear()`'dan alıyor — OSC 12 ile yazılan renk çizime hiç
  ulaşmıyor. Bu setin konusu değil; `context.md` → Kanıt'ta kayıtlı.
- **`saat=` / `blink=` jetonu** (Karar 6).
- **Smear/Squash/Phosphor/Arc** hareket stilleri, `intensity`/`duration`
  çarpanları — 008'in kapsam dışısı olarak duruyor.
- **`cursor_motion`'ın `[terminal]`'a taşınması.** Anahtar silinmez; taşıma
  ayrı bir iştir ve bu set yapmıyor.

## Göç

**Ayar tarafı:** `[terminal]`'a iki yeni anahtar; silinen anahtar yok,
bilinmeyen anahtar korunuyor. Bölüm zaten okunuyor (`scrollback`), yani
"bilinmeyen bölüm birden uygulanmaya başladı" durumu **yok** — göçün yükü
sıfır. Tanınmayan **değer** kendi anahtarını değiştirmez ve tanı bırakır.

**`make kur` gerekmiyor:** kabuk betiği, terminfo, jeton satırı ve app bundle
değişmiyor. Şekil ve blink tamamen terminalin kendi işi.

**Yol haritası kayar (beşinci kez).** Bugün 014 = materyal yüzey; bu set onu
**015**'e, emoji/geniş-glyph'i **016**'ya, sekme/bölme'yi **017**'ye itiyor.
`docs/YOL-HARITASI.md` kendi kuralına göre tarihli bir kayma notu ister ve
kaydedilecek bedel şu: materyal yüzeyin yazılı ön koşulu (kare süresi tabanı
`/measure` ile bu setten **önce** alınmış olmalı) bir kez daha erteleniyor.

## Akış

| Phase | İş | Neden bu sırada |
|-------|-----|-----------------|
| phase-1 | Şekiller: `Cursor` alanı, `term_config`, `[terminal] cursor`, çizim | **Kare altyapısına sıfır dokunuş** ve kendi başına ürün: vim insert modda beam görünür. Tek başına doğrulanabilir — kapının hiçbir katı caret dikdörtgeninin boyutuna bakmıyor. Riskin tamamı phase-2'ye erteleniyor |
| phase-3 | İmlecin tema rolü: `cursor` | Küçük, görünür ve shader'a dokunmuyor. Sona bırakıldı çünkü setin omurgası blink; renk ondan bağımsız ve tek başına doğrulanabilir |
| phase-2 | Blink: `Waker::resume`, uyku testinin üçüncü sorusu, son tarihli saat, faz tipi, `[terminal] cursor_blink`, Reduce Motion dışlaması, sözleşme | Tek mimari risk burada ve phase-1 yeşilken tek başına sınanır. **Ön koşulu `[elle]` referans bakışı** (Karar 9) |

**phase-1'den sonraki ara durum tutarlı:** `\e[5 q` gönderen vim'de kullanıcı
yanıp sönmeyen bir **beam** görür. Bugün yanıp sönmeyen bir **blok** görüyor —
yani phase-1 blink'i bugünkünden fazla yutmuyor, şekli kazandırıyor.

```
phase-1                             phase-2
────────                            ────────
Term::cursor_style() → shape        aynı çağrıdan → blinking biti
Cursor { shape, … }                 Cursor { blink: bool, … }
term_config.default_cursor_style    Waker::resume()  (hasar dikmez)
Caret ölçüsü + CursorBlock.rect     uyku testi: settled() && !flipped
Frame şekli saklar                  arm_clock: min(iki deadline)
[terminal] cursor                   [terminal] cursor_blink + Reduce Motion
                                    sözleşme: saatin iki tadı
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| kapı | |
