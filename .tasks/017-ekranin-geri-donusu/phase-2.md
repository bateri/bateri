# Phase 2 — Doldurma: sınır tarafı

## Özet

`frame()` üstte kalan boşluk kadar geçmiş satırını okur ve sınırdan **ayrı**
verir; `Cursor` kaç satır dolduğunu söyler.

_Requirements: R2.1, R2.2, R2.3, R2.4, R2.5_

## phase-1'den devralınan

Bayrağın ömrü **üç** koşul (`content_rows == rows` ∧ `!alt_screen` ∧
`display_offset == 0`); R1.2 ve `CLAUDE.md` phase-1 commit'inde düzeltildi.
İki sonucu bu phase'in kabul ölçütünü belirliyor (gerekçeler
`phase-1.md` → Uygulama Notları §5):

1. **Alternatif ekran bayrağı kurar** — `vim` açılışta `CSI 2 J` basıyor ve
   `!alt_screen` koşulu onu orada düşmekten koruyor. Yani vim'den çıkıldığında
   ekran dolu değilse doldurma **koşmaz**. Yönü güvenli (doldurma yapmamak
   bugünkü davranış).
2. **Dock'lu pencerede bayrak prompt'ta düşmüyor** — caret dock'a devrildiğinde
   doluluk giriş satırını saymıyor (`content_rows = drawn_rows.max(1)`), yani
   `content_rows == rows` ancak **çıktısı ekranı dolduran bir komutun**
   karesinde doğru oluyor. Doldurmanın tüketicisi de yalnız dock'lu pencere,
   yani kabul ölçütü "Ctrl-L'den sonra ekranı dolduran ilk komuta kadar
   doldurma yok" der. Bu, İşletme jürisinin R2c'de kabul ettiği bedelin
   gerçekleşmiş hâli; `teslim.md`'ye adıyla yazılır.

## phase-0'dan devralınan

**Doldurma ekranı Tab öncesine birebir değil, bir satır eksiğine döndürür** ve
bu bir aritmetik seçim değil zsh'in davranışı: Ctrl-C'nin `\r\r\n`'si yeni
prompt'u komut satırının bir altına indiriyor, yani listenin açtığı dört
satırın biri prompt tarafından tüketiliyor. `gap` olaydan **sonra** ölçüldüğü
için `fill = min(history_size, gap)` formülü **düzeltme istemiyor** (ölçülen
koşuda `min(24, 3) = 3`, doğru sayı). Sayılar `phase-0.md` → Uygulama
Notları §4. Yanlışın yönü güvenli: ekran dolu görünür, yalnız bir satır
yukarıdan başlar. Kabul ölçütü buna göre okunur — "Tab öncesinin **aynısı**"
değil, "delik yok ve içerik sürekli".

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `frame()`'e doldurma kolu.
  - `gap = rows - content_rows`, `fill = min(history_size(), gap)`. Koşul
    (R2.2): dock var **∧** `!alt_screen` **∧** bayrak temiz **∧**
    `display_offset == 0`. **Safha kapısı yok** — gerekçesi ölçülmüş
    (`discussion.md` → Karar 5): Enter kolunda `\e[J` `Running` safhasından
    geçiyor ve safha kapısı olsaydı o karede `fill == 0` kalır, dönüş
    animasyonsuz olurdu.
  - Geçmişten okuma **ayrı bir döngü**, stil değil zorunluluk: mevcut döngünün
    `debug_assert!((0..rows).contains(&row))` (`:1779`) negatif satırda patlar
    ve `drawn_rows` onları saymamalı. Giriş noktası `grid().iter_from(...)`
    ya da satır satır `grid()[Line(..)]`; her iki yolda da `grid_clamp` /
    `Boundary::Grid` ile sınırlanır — `bt-core`'da gerekçesiz panik yok (R2.5).
  - **`content_rows` ve `origin` aritmetiği değişmez** (R2.3). Doldurulan
    satırlar doluluğa **girmez**; girselerdi öteleme kapanır, içerik tabandan
    kopardı ve `session.rs:6283`'ün bekçisi kırılırdı — `27a0b98`'in
    maliyetini tekrarlamamanın tek yolu bu.
  - Doldurma hücreleri sink'e **fill-yerel** satırla (`0..fill`) gider; ekran
    satırına çeviren taraf çizen taraftır (phase-3).
- **`crates/bt-core/src/session.rs`** — `Cursor`'a `fill: u16`. Kare başına
  tek `Cursor` olduğu için bütçe kalemi yok; tüketicileri phase-3 ve phase-4.
- **Geri alma şeridi (R2.4):** doldurma tek boğaz noktasından geçer
  (`fill_rows()` gibi tek bir yüklem). Sıfır dönerken sınırdan geçen kare
  bugünküyle **bit bit** aynı olmalı ve bunu söyleyen bir bekçi yazılır —
  016'nın "yarıçap 0, hale 0" kolunun aynı örüntüsü.

## Kabul

- Ekran dolu → Tab → Ctrl-C reçetesinde `fill == gap` ve doldurulan satırlar
  **geçmişin en yenileri**, içeriğin hemen üstünde.
- Bayrak kurulu (Ctrl-L) → `fill == 0`.
- Alternatif ekran → `fill == 0`. Tekerlekle kaydırılmış pencere → `fill == 0`.
- Geçmiş boş (yeni oturum) → `fill == 0`, hiçbir ek okuma yok.
- `content_rows` değerleri bugünkü bekçilerle **aynı** kalıyor
  (`session.rs:6216`, `:6234`, `:6270`, `:6283` dokunulmuyor).
- `fill == 0` iken sınırdan geçen kare bugünküyle bit bit aynı (geri alma
  bekçisi).
- `make hepsi` ve `make test-yaris` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** `frame()` sınırının anlattığı yere bir cümle — boşluk artık
  boş değil, geçmişle doluyor ve doluluğa girmiyor.
- **Ölçüm bekliyor:** doldurmalı karede sınır hücresi sayısı ve `Term` kilidi
  altındaki ek satır okumasının maliyeti — `frame()` bugün geçmişe hiç
  inmiyor (tek `history_size()` çağrısı `scroll_locked`'ın kırpmasında,
  `:3153`), bu phase kilidin altına `rows` satıra kadar yeni okuma koyuyor.
- shader / terminfo / ayar şeması / tema / shell entegrasyonu / app bundle /
  yeni bağımlılık: yok.

## Checklist

- [x] `fill` hesabı ve dört koşullu kapı yazıldı
- [x] Geçmişten okuma ayrı döngüde, `grid_clamp` ile sınırlı
- [x] `Cursor::fill` eklendi
- [x] Tek boğaz noktası (`fill_rows()`) ve geri alma bekçisi
- [x] Test: Tab→Ctrl-C reçetesi (`fill == gap`, satırlar doğru)
- [x] Test: bayrak / alternatif ekran / tekerlek / boş geçmiş → `fill == 0`
- [x] Test: `content_rows` bekçileri dokunulmadan geçiyor
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [x] Riskli phase: `/code-review` koştu (paylaşılan durum); belge bulgusu
      giderildi, ömre ait iki bulgu **kayda geçti** (Uygulama Notları §7 —
      R1.2'ye ait, kullanıcı kararı bekliyor)
- [x] Yayın etkisi yazıldı

## Uygulama Notları

### 1. Sınırın şekli — **ikinci sink**, işaretli hücre değil

`frame(sink, fill_sink, blocks)`. Doldurma hücreleri ızgarayla aynı kanaldan
geçemezdi: satır numaraları **fill-yerel** (`0..fill`) ve ızgaranınkilerle
çakışıyor, yani tek kanalda ayırt edilemezlerdi. Hücreye bir bayrak eklemek de
yanlış yön — kare başına maliyeti olan bir alanı çizen tarafın sayaç
muafiyeti (R3.2) için ödemek olurdu. Emsal dock: kendi listeleri, kendi
sayaçları.

Bedeli ~35 sınama çağrısına bir `|_| ()`. Üretimde tek gerçek tüketici
`bt-gpu::link`; o da bugün boş sink veriyor (phase-3'e devredildi) ve
`bt-shell::child`'ın sınama yardımcısı da öyle.

### 2. `content_rows` kayıttan **önce** bir yerele çıktı

İki tüketicisi var ve ikisi de `Cursor` kurulmadan önce, `Term` kilidi altında
koşuyor: bayrağın ömrü (`observe_screen_clear`) ve doldurmanın boşluğu
(`fill_rows`). Alan üstünden okunsaydı kayıt yalnız sırayı taşıyan bir ara
durak olurdu. `debug_assert` de yerelle birlikte taşındı; değeri ve kolları
bit bit aynı — `content_rows` bekçilerinin hiçbirine dokunulmadı.

### 3. Sıra zorunlu: ömür **önce**, doldurma **sonra**

`fill_rows` bayrağı okuyor, yani `observe_screen_clear`'dan sonra çağrılmak
zorunda. Ters sırada, baytları henüz uygulanmamış bir Ctrl-L'in karesinde
doldurma bir kereliğine koşar ve temizlenen ekranı geri getirirdi. Sıranın
bekçisi `race_screen_clear_and_frame`: yarış boyunca "ekran dolu değil ∧
bayrak kurulu" olan her karede `fill == 0` ve ikinci sink sessiz. Sınamanın
oturumu bu phase'de **dock'lu** oldu — dock'suz koşan bir yarışta `fill` her
hâlde sıfır kalır ve iddia boşa düşerdi.

### 4. Ortak parça: `cell_style`

Kapıdan **sonra** çözülen dört alan (ön plan, alt çizgi çeşidi, üstü çizili,
SGR 58) iki döngüde de birebir aynı ve serbest bir fonksiyona çıktı. Gerekçe
stil değil tuzak: alt çizgi zincirinde `UNDERCURL` `UNDERLINE`'ı içermiyor ve
`underline_color`'ın kapısı `ruled` değil alt çizginin kendisi — ikisi de
ikinci bir kopyada sessizce ayrışırdı. Kapıdan **önceki** yarısı (zemin,
mürekkep, kural, çizilirlik) iki döngüde ayrı duruyor: ızgaranınki seçimi
sorması gerekiyor, doldurmanınki sormuyor.

### 5. Doldurmada **seçim yok** ve bu bir karar

Doldurulan satırlar seçilemiyor (`plan.md` → Kapsam Dışı; R5.1 orijinin
üstündeki tıklamayı reddediyor), yani vurgulanmaları "burada seçilebilir bir
şey var" derdi. Izgara da bugün aynı cevabı veriyor: tamamı geçmişte kalan bir
aralık `visible_range`'den geçmiyor. **Bilinen sınır:** geçmişten ekrana uzanan
bir seçim dibe dönüldüğünde doldurma bandında vurgusuz görünür —
`selection_text()` onu hâlâ kopyalar. Yönü güvenli (eksik vurgu, fazla değil)
ve kapatmanın yeri phase-5.

### 6. `alt_screen` kapısı bugün **ikinci bir kilit**

`content_rows` alternatif ekranda tanım gereği `rows`, yani `gap` zaten sıfır
ve `fill_rows`'un `alt_screen` kolu hiç fark yaratmıyor. R2.2 onu yazıyor ve
duruyor: `content_rows`'un alternatif ekran kolu değişirse tutan tek şey o
olur. Sınaması bunu iddia etmiyor, **gözlüyor** (`an_empty_history_and_the_
alternate_screen_leave_the_gap_empty`, ikinci kol).

### 7. `/code-review`'un iki bulgusu — **giderilmedi, kayda geçti**

İkisi de bu phase'in kodunda değil **bayrağın ömründe** (R1.2, phase-1) ve
ikisi de yukarıdaki iki devir bloğunun *ölçülmüş* hâli. Düzeltmeleri
hissedilir davranışı değiştirir ve R1.2'yi yeniden tasarlamak demektir, yani
phase-2'nin kapsamı değil; karar kullanıcının.

**(a) Alternatif ekranın `CSI 2 J`'si bayrağı kurup bırakıyor.** Kurma kolu
`alt_screen` sormuyor, düşürme kolu soruyor — yani `vim`/`less`/`htop`'tan
çıkıldığında bayrak **kurulu** kalıyor ve ana ekranda doldurma sessizce
kapanıyor. Devir bloğu #1 bunu "vim'den çıkıldığında ekran dolu değilse
doldurma koşmaz" diye yazıyordu; ölçüm onu doğruladı ve **sayıya bağladı**
(dock'lu oturum, `\e[4A\e[J` ile beş satırlık delik, sonra
`\e[?1049h\e[2J\e[?1049l`: `Cursor::fill` 5 → 0, `screen_cleared` `true`,
`content_rows = 5 / rows = 10`).

Düzeltme **tek satır değil**: sayaç baytlar uygulanmadan **önce** artıyor,
yani `?1049l` ile `2J`'yi aynı okumada taşıyan bir tur `Term` kilidi altında
`alt_screen` hâlâ `true` iken gözlenebiliyor — kurma kolunu naifçe
`alt_screen`'de atlamak nesli **tüketip** bayrağı kurmamak olurdu, ve o hâlde
gerçek bir Ctrl-L kaybolurdu.

**(b) Düşürme yüklemi dock'lu zsh'te neredeyse erişilemez.** `content_rows ==
rows` ancak imleç **dip satırdayken** doğru; komut çıktısı `\n` ile bittiği
için dolu ekranda imleç boş bir alt satırda duruyor ve `Finished`/`Input`
safhalarında `caret_in_dock` doğru, yani `content_rows = rows - 1`. Yüklem
yalnız `Running`'in içine düşen bir karede tutuyor ve `ls` sınıfı bir komutta
o pencere ~1 ms — vsync'e karşı. Sonuç devir bloğu #2'nin yazdığından
**daha ağır**: "ekranı dolduran ilk komuta kadar" değil, pratikte oturumun
sonuna kadar bayrak asılı kalabilir ve Ctrl-L'den sonra doldurma bir daha hiç
koşmayabilir.

Hermetik olarak sınanamıyor: `spawn_docked_session`'ın canlı ZLE aynası yok,
yani orada `caret_in_dock` yanlış ve yüklem **tutuyor** — deponun bugünkü
sınamalarının bulguyu görmemesinin sebebi bu.

### 8. Ölçüm borcu

`frame()` bugüne kadar geçmişe **hiç** inmiyordu; bu phase `Term` kilidinin
altına `fill × cols` hücrelik yeni bir okuma koydu (dock'lu, dibe yaslı,
deliği olan pencerede — yani üst sınır `rows × cols`, ızgara döngüsünün
bir katı). Sayı ölçülmedi: `docs/OLCUMLER.md`'nin konusu ve `/measure` ile
istenir.
