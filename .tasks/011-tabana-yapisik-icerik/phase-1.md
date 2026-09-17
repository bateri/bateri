# Phase 1 — Tabana yapışık içerik

## Özet

İçerik pencerenin tabanına yaslansın: `frame()` doluluk sayısını üretsin,
çizim `setViewport` ile kaysın, fare aynı ofseti okusun — **animasyonsuz**.

_Requirements: R1.1, R1.2, R1.3, R1.4_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `frame()` döngüde `content_rows`
  toplar: atlama kapısından geçen en büyük `row` ile `cursor_row`'un
  maksimumu, artı bir. **İkisi birden gerekli** — imleç tek başına yetmez
  (imleci yukarı taşıyan ilerleme çubuğu içeriği aşağı iterdi), kapı tek
  başına yetmez (boş prompt satırı kapıdan geçmiyor). Alternatif ekranda
  değer ızgaranın tamamı olur (ofset 0); bayrak `alt_screen` olarak zaten
  kilidin altında okunuyor. Değer `Cursor`'la döner — `bt-core` **piksel
  değil sayı** veriyor.
- **`crates/bt-gpu/src/link.rs`** — `frame()` **döndükten sonra** origin'i
  kurar (`content_rows` → satır → piksel). Origin'in **tek sahibi** burası;
  `view` onu buradan okur. Hasarlı kolda yazılır; hasarsız kolda bu phase'de
  değişmiyor (phase-2 ikinci yazma noktasını açacak).
- **`crates/bt-gpu/src/renderer.rs`** — `encode_pass` `setViewport`'u
  `originY` ile kurar. **İki pipeline birden** kayar (`cell_bg` + `cell`),
  yani arka plan, glyph, kural, şerit ve imleç tek yerden. `.metal`
  dosyalarına **dokunulmuyor**.
- **`crates/bt-gpu/src/frame.rs`** — `CursorBlock.rect`'e origin **CPU'da**
  eklenir: `[[position]]` viewport dönüşümünden **sonraki** koordinat, yani
  rect kaydırılmazsa imlecin altındaki metnin rengi eski satırda kalır.
  `pos_at` ve `push_block` **değişmez** — payın tek sahibi ve `push_block`'un
  `pos_at`'i bilerek atlaması korunur.
- **`crates/bt-shell/src/view.rs`** — `point_to_cell` origin'i okur ve
  **`f64`'te** çıkarır. Boş alan **üstte**: `u16`'da çıkarma oraya yapılan
  tıklamada taşar. Resize zamanı önbelleği (`metrics`) origin **taşımaz** —
  origin kare başına değişiyor, o önbellek yalnız pencere olaylarında
  tazeleniyor.
  - **Tesisat kararı, adıyla konur:** `view`'ın bugün `link`'e erişimi **yok**
    (`app.rs:547` ikisini de sahipleniyor, `view`'a değerler `set_metrics` ile
    itiliyor). Bu phase **paylaşılan bir ana-thread hücresi** kurar
    (`Rc<Cell<…>>` ya da eşdeğeri): yazan kare yolu, okuyan fare yolu, ikisi de
    ana thread. **`Arc<AtomicU32>`'ye kaçılmaz** — atomik gerekmiyor, ve
    gerekiyormuş gibi yazmak `make test-yaris` tetiğini ve "riskli phase"
    etiketini geri getirir (`discussion.md` → Karar 4 eki).

## Kabul

- Boş kabukta tek satırlık içerik pencerenin **dibinde**; üstünde boşluk.
- Ekran dolduktan sonra görünüm bugünküyle aynı (ofset 0'a iner).
- `vim`/`htop` açıkken ofset **0**: ızgaranın tamamı kullanılıyor.
- **Geçmişte kaydırırken içerik tabana yapışık kalır.** `content_rows`
  **görünür** satırlardan doğuyor, yani `display_offset > 0` iken de kural
  aynı: 99 satır geçmişi olan ve `clear`'lanmış bir pencerede tekerleğin ilk
  çentiği iki satırlık içerik gösterir ve ikisi **dipte** durur; kaydırma
  ilerledikçe pencere dolar ve ofset kendiliğinden 0'a iner. Alternatifi
  ("ofset `display_offset > 0` iken 0'a donar") **reddedildi**: tekerleğe
  dokunur dokunmaz içeriğin tavana sıçraması demekti.
- Tıklama ve sürükleme doğru hücreyi seçiyor; üstteki boş alana tıklamak
  taşmıyor.
- **Üç bekçi yeşil** (aşağıda).
- `make duman` yeşil; `icerik` jetonu **oynamıyor** — ofset çizim zamanı, yani
  yeni içerik karesi doğurmuyor.

### Bekçiler

Bekçiler **CPU-CPU eşitliği değil CPU→GPU dikişi** ölçer: ofset GPU'da
(`setViewport`), yani iki CPU listesini birbirine karşı ölçen bir sınama
inşa gereği doğru olan bir şeyi sınar (`discussion.md` → Muhakeme 2. tur,
kabul 4).

1. **`cell_bg` pipeline'ı** — offscreen render + origin `k` satır; boyanan
   satır okunur ve `k` kadar kaymış olmalı. Emsal
   `cell_bg_paints_pixels_on_the_gpu`.
2. **`cell` pipeline'ı** — aynı kurulum glyph/kural için; **iki pipeline'ın
   aynı miktarda kayması** bu setin asıl riski (birinin unutulması
   `make hepsi`'yi yeşil bırakırdı).
3. **İmleç** — `push_cursor`'ın ürettiği `rect.y` ile aynı satıra basılan
   hücrenin `pos.y`'si eşit. Karar 7'nin üç kör noktasından bugün bekçisi
   olmayan üçüncüsü bu.

## Uygulama Notları

- **`Cursor` `rows`'u da taşıyor, yalnız `content_rows`'u değil.** Plan
  ötelemeyi `link.rs`'te (`rows - content_rows`) hesaplatıyor ama çizen tarafın
  `rows`'u yoktu: `DisplayLink` yalnız `CellMetrics` tutuyor, satır sayısı
  `resize`'ın parametresi. Kendi kopyasını tutmak `Session::resize`'ın **ret
  kolunda** ayrışırdı — dejenere boyut oturum tarafından yoksayılıyor, oysa
  kopya yazılmış olurdu ve öteleme bir kare boyunca yanlış ızgara
  yüksekliğinden çıkardı. İki alan **aynı `renderable_content()` okumasından**
  geliyor, yani ayrışamıyorlar. Yerleşim kararı (`rows - content_rows`) plandaki
  yerinde, `link.rs`'te kaldı.
- **`setViewport` kanaryası tuttu** ve negatif kontrolle doğrulandı: `originY`
  sıfıra çivilendiğinde iki bekçi de kırmızı düşüyor, geri alınınca ikisi de
  yeşil. Metal taşan viewport'u kırpıyor (`originY 8 + boy 16 = 24 > 16`
  doğrulama hatası vermiyor, alt bölge sarmıyor). Uniform yoluna dönülmedi,
  `.metal` dosyalarına dokunulmadı, phase **riskli değil**.
- **Üretimde kırpma hiç devreye girmiyor** ve bunu ötelemenin tanımı veriyor:
  içerik `0..content_rows`, öteleme `rows - content_rows`, yani en alt dolu
  satır tam `rows`'ta bitiyor. Kırpma yalnız bekçinin kurduğu yapay sahnede
  konuşuyor; değişmez `set_origin`'in doc'una yazıldı.
- **İmlecin dikdörtgeni ötelemeyi CPU'da alıyor** (checklist'teki kalem) ve bu
  phase-2'de **düşmüyor**. 2. tur muhakemesi (kabul 6) "3c'nin `CursorBlock.rect`
  zorunlu ayrıntısı düşer" diyordu; düşen şey *gerekçe*, satır değil: imleç
  ekran uzayına taşındığında instance'ın konumu `ekran_satırı - origin` olur ve
  rect yine `instance + origin` = ekran satırı. Formül iki phase'de aynı,
  değişen tek şey `at`'in nereden geldiği.
- **Fare tesisatı `Rc<Cell<f32>>` oldu** ve plandaki gibi atomiksiz;
  `bt_gpu::Origin` adıyla ihraç edildi, `DisplayLink::origin()` `waker()`
  örüntüsünü izliyor. `view` onu `set_metrics`'ten **ayrı** bir çağrıyla
  (`attach_origin`) alıyor: kaynağı ayrı (pencere değil kare yolu) ve link
  `set_metrics`'ten sonra doğuyor. `point_to_cell` `origin_px`'i **parametre**
  olarak aldı, alan olarak değil — fonksiyon saf kaldı ve orijini konu etmeyen
  sınamalar `0.0` geçiyor.
- **İmleç kaydı döngüden sonra kuruluyor.** `content_rows` ancak sink döngüsü
  bitince biliniyor; alternatif onu sıfırla doğurup sonra düzeltmekti, yani bir
  kare boyunca yanlış olan bir alan. Girdilerin tamamı (şekil, nokta, satır,
  görünürlük) döngüden **önce** çözülüyor, yani "imleç döngüden önce çözülüyor"
  cümlesi ayakta.
- **Bilinen ara durum: Enter'da imleç bir satır üstten kayıyor.** Öteleme bu
  phase'de **anında** iniyor (animasyon yok) ama `Motion` imleci hâlâ **grid**
  uzayında sürüyor: Enter'da grid satırı `r → r+1` olurken doluluk da bir
  büyüyor, yani ilk karede ekran y'si `(rows-2)·h` — dipteki satırın bir üstü —
  ve yay onu aşağı indiriyor. Yerleşmiş hâl iki uçta da doğru (`(rows-1)·h`),
  oynayan yalnız geçiş. **Kasıtlı olarak yamanmadı:** çaresi R2.1, yani imlecin
  hedefinin ekran uzayına taşınması; burada `content_rows` değişimini snap
  tetiğine bağlamak phase-2'nin sileceği bir churn olurdu. `cursor_motion =
  "snap"` ve Hareketi Azalt bu geçişi hiç göstermiyor. Göz kontrolünde
  görülecek tek gerileme bu ve phase-2'nin ilk kabul maddesi tam olarak onu
  kapatıyor.
- **İki sınama kendi tuzağını buldu.** Kaydırma bekçisi ilk hâlinde yarışa
  girdi (`seq` geçmişi oluşmadan "ekran temiz" ölçütü tuttu, `Scrolled(0)`):
  iki adım `read` ile sıralandı. Glyph dikişi `GpuError::NoAtlas` ile düştü:
  atlasın anahtarının ölçek yarısı pencereden geliyor ve sınamanın penceresi
  yok, `cell_metrics(1.0)` eklendi.

## Yayın Etkisi

- **shader:** `.metal` **değişmedi** (`setViewport` yolu), `make shader`
  gerekmedi ve `#[repr(C)]` ↔ MSL düzeni dokunulmadan kaldı.
  **Kanarya tuttu:** Metal taşan viewport'u kırpıyor, negatif kontrolle
  doğrulandı (bkz. Uygulama Notları). Uniform yoluna dönülmedi, yani phase
  **riskli değil** ve `/code-review` set sonunu bekliyor.
- **Belge:** `docs/YOL-HARITASI.md` **bu phase'in işi değil** — set açılırken
  (`/rfc`, 2. tur) yeniden yazıldı: 011 satırı yeni kapsamıyla, dock için 012
  satırı, üçüncü numara kayması ve iki bilinen hatanın düzeltmesi
  (a: "kirli satır takibi devre dışı kalır" — devre dışı kalacak bir satır
  takibi yok; b: "010'un açık kalemini bu kapatıyor" — o kalem 010'un kendi
  son commit'inde kapandı) orada indi. Phase yalnız `CLAUDE.md`'ye dokunur.
- **`CLAUDE.md`:** "Bugünkü hâl" paragrafı içeriğin tabana yaslandığını söyler.
- terminfo/`TERM`, ayar şeması, tema biçimi, shell entegrasyonu, app bundle:
  **yok**.
- Yeni bağımlılık: **yok**.
- Ölçüm bekleyen iddia: **yok** (bu phase animasyonsuz; kayma phase-2'de).

## Checklist

- [x] `content_rows` `frame()`'de toplanıyor, alt ekranda ızgaranın tamamı
      (öteleme 0); `rows` da aynı okumadan geçiyor (bkz. Uygulama Notları)
- [x] Origin `DisplayLink`'te, `setViewport` iki pipeline'ı kaydırıyor
- [x] `CursorBlock.rect` CPU'da kaydırılıyor; `pos_at`/`push_block` değişmedi
- [x] `point_to_cell` origin'i `DisplayLink`'ten (`bt_gpu::Origin`) `f64`'te
      okuyor
- [x] Test: üç bekçi — `content_sticks_to_the_bottom_for_cell_bg`,
      `…_for_glyphs`, `the_cursor_rect_carries_the_origin_but_the_instance_does_not`;
      yanlarında `bt-core`'un dört doluluk sınaması ve `view`'ın orijin sınaması
- [x] `setViewport` kanaryası doğrulandı (negatif kontrol: `originY` sıfıra
      çivilendiğinde iki bekçi de kırmızı); uniform yoluna dönülmedi, phase
      riskli değil
- [x] `CLAUDE.md`'nin "Bugünkü hâl" paragrafı içeriğin tabana yaslandığını
      söylüyor (yol haritası set açılırken güncellendi, burada iş yok)
- [x] Doğrulama geçti: `make hepsi` yeşil (exit 0) ve `make duman` kullanıcının
      gerçek penceresinde yeşil — `kare=29 hucre=8 glif=6 kural=15 icerik=2
      hareket=27 sessiz=1749.31ms kapanis=clean`. Üç sayaç oynamadı ve
      **`icerik` phase-0'daki `2`'de kaldı**: öteleme çizim zamanı, yani yeni
      içerik karesi doğurmuyor
- [x] Yayın etkisi yazıldı
- [x] Göz kontrolü (kullanıcı, gerçek pencere): prompt açılışta **dipte**;
      `vim` ve benzeri tam ekran uygulamalar normal kullanılıyor (ızgara
      tavana dönüyor); üstteki boş alandan başlayan sürükleme metnin **ilk
      satırından** seçiyor, dibe fırlamıyor — `u16` taşma tuzağının gözle
      karşılığı da kapandı
