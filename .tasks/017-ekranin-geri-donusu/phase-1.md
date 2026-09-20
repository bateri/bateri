# Phase 1 — Kasten temizleme bayrağı

## Özet

PTY tarayıcısı `CSI 2 J`'yi tanır ve "ekran kasten temizlendi" bayrağını
kurar; bayrak ekran doğal yoldan dolunca düşer.

_Requirements: R1.1, R1.2, R1.3, R1.4_

## Neden önce

Doldurmadan **sonra** inseydi, arada kalan commit'te Ctrl-L geri alınmış
görünürdü — ölçülmüş bir regresyon (`context.md` → Kanıt 3, 4). Bu phase tek
başına hiçbir davranış değiştirmiyor: bayrağın tüketicisi phase-2'de doğuyor.

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `ScanState`'e CSI durumu. Bugün `ESC [`
  `Ground`'a düşüyor (`:1515`'in `_ =>` kolu) ve modül doc'u "CSI bizi
  ilgilendirmiyor" diyor; ikisi de değişiyor ve **gerekçesi yazılıyor**.
  - Tanınan tek dizi `CSI 2 J`. `3J` ve RIS **yok**: ikisi de
    `clear_history()` çağırıyor (alacritty `term/mod.rs:1805`,
    `grid/mod.rs:341`), yani `history_size() == 0` ve doldurma kendiliğinden
    kapanıyor. Üçüncü bir kol önerisini kapatan bu cümle doc'a girer.
  - **İptal kuralları vte'den birebir taşınır** (R1.3): `ESC` → `Escape`,
    `0x18`/`0x1A` → `Ground`, parametre uzunluğuna `MAX_OSC_NUMBER` emsali
    tavan. Taşınmazsa bozuk bir CSI durumu takar ve peşinden gelen
    `ESC ] 133;…` yutulur — bloklar, bastırma ve dock **sessizce** ölür.
  - `feed`'in "baytlara dokunmama" garantisi korunur (R1.4).
- **`crates/bt-core/src/shell.rs` ya da `session.rs`** — bayrağın **yaşadığı
  yer bu phase'in asıl tasarım kararı.** İki kol var ve biri seçilip gerekçesi
  yazılır:
  - *nesil sayacı + compare-and-set* — temizleme yalnız kurulduğu neslin
    üstüne yazar;
  - *bayrak `Term` kilidinin altında* — okuyucu thread baytları uygulamak için
    o kilidi zaten alıyor ve "temizleme bir terminal olayı" gerekçesi oraya
    işaret ediyor.
  Seçilmezse şu dizi sessizce bozar: tarayıcı kurar → `frame()` kurulu okur →
  `Term` kilidi (baytlar henüz uygulanmadı, `content_rows == rows`) → kilit
  bırakılır → bayrak **temizlenir** → `Term` `2J`'yi uygular.
- **Ömür:** `content_rows == rows` **ve** `!alt_screen`. İkinci koşul zorunlu:
  alternatif ekranda `content_rows` tanımı gereği `rows`
  (`session.rs:2020-2021`, bekçi `:6270`), yani onsuz `vim` bayrağı düşürür.

## Kabul

- `CSI 2 J` bayrağı kuruyor; `CSI J`, `CSI 1 J`, `CSI 3 J` **kurmuyor**.
- Bozuk/yarım CSI'dan sonra gelen `ESC ] 133;A` hâlâ görülüyor (bekçi).
- `ESC [ ? 1049 h` gibi özel CSI'lar bayrağı kurmuyor ve durumu takmıyor.
- Alternatif ekranda geçen kareler bayrağı düşürmüyor.
- Bayrağın hiç tüketicisi yok: davranış bugünküyle **birebir** aynı.
- `make hepsi` ve `make test-yaris` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** `bt-core` satırındaki "tarayıcının üç kolu var" cümlesi
  dördüncüye çıkıyor — ama yeni kol OSC değil CSI, ve yükü yok (bayrak).
- shell entegrasyonu: **yok** — betik ve tel değişmiyor, sinyal terminalin
  kendi gözlemi. Üç kabuk için de aynı.
- **Ölçüm bekliyor:** CSI kolunun yoğun akıştaki (vim, `less`) tarama
  maliyeti; hızlı yol artık CSI başına birkaç bayt fazladan adımlıyor.
- terminfo / `TERM` / ayar şeması / tema / app bundle / yeni bağımlılık: yok.

## Checklist

- [x] CSI durumu ve `2J` tanıma yazıldı, iptal kuralları taşındı
- [x] Bayrağın yaşadığı yer seçildi ve gerekçesi doc'a yazıldı
- [x] Ömre `!alt_screen` koşulu kondu
- [x] Test: `2J` kurar / `1J`,`3J`,`J` kurmaz
- [x] Test: bozuk CSI'dan sonra OSC 133 hâlâ görülüyor
- [x] Test: alternatif ekran bayrağı düşürmüyor
- [x] Test: yarış (`make test-yaris`, iki zamanlama profili)
- [x] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [x] Riskli phase: `/code-review` koştu (paylaşılan durum), bulgular giderildi
- [x] Yayın etkisi yazıldı

## Uygulama Notları

### 1. Bayrağın yeri — **nesil sayacı** (kol 1)

`Session.screen_clears: Arc<AtomicU32>` tarayıcının saydığı `CSI 2 J`;
`screen_seen` kare yolunun hesaba kattığı nesil, `screen_cleared` de bayrağın
kendisi. Kural tek yerde (`Session::observe_screen_clear`):

```
nesil ayrıştıysa  → nesli tüket, bayrağı KUR
eşitse ve ekran dolu ve alternatif ekranda değilsek → bayrağı DÜŞÜR
```

**Kol 2 (`Term` kilidinin altı) uygulanabilir değildi**, elenmedi: kilit
`FairMutex<Term>` ve `Term` bizim tipimiz değil, yani bayrak oraya *fiilen*
konulamıyor. Geriye "ikinci bir muteks, ama yalnız `Term` tutulurken alınır"
disiplini kalıyordu ve onu deponun kendi kuralı yasaklıyor — yaprak kilit
`Term`'ün altına girmez. Atomik ikisinin arasını buluyor: kilit değil, ama
`Term` kilidinin **altında** okunuyor.

Kol 2'nin **gerekçesi** ise aynen alındı ve yerleşimi o belirliyor: sayaç
`frame()` içinde, `Term` kilidi tutulurken ve `content_rows` ile **aynı
okumada** tüketiliyor. İki kazancı var ve ikisi de ölçüldü (aşağıda):

1. Okuyucu thread sayacı `advance`'ten **önce** artırıyor (`TappedPty::read`;
   `EventLoop::pty_read` önce okuyor sonra uyguluyor). Kilitten önce okunsaydı
   kare yolu eski sayacı alır, sonra ayrıştırma lease'inin arkasında bekler ve
   **temizlenmiş** ızgarayı bayrak kurulmamışken görürdü.
2. Okuma-değiştirme-yazma turunu serileştiren şey kilidin kendisi;
   `compare_exchange` gerekmiyor. Kilit bırakıldıktan sonra yazılsaydı arada
   gelen taze bir `CSI 2 J` ezilirdi.

**Taze nesil, doldurma kuralını aynı karede eziyor** ve phase dosyasındaki
adlı diziyi kapatan cümle bu: baytlar henüz uygulanmamışken ızgara dolu
görünür, ama nesil "bu temizlemeyi hesaba katmadım" der ve bayrak düşmez.
Düz bir `bool` bu ayrımı yapamaz — sorduğu soru "ekran temizlendi mi", oysa
gereken soru "bu temizlemeyi hesaba kattım mı".

**Bekçi çevrilebilir, ölçüldü** (2026-09-20, `race_screen_clear_and_frame`,
üçer koşu):

| düzenleme | sonuç |
|---|---|
| doğru hâl | 3/3 yeşil |
| nesil karşılaştırması yok (düz bayrak) | 3/3 kırmızı (0,03–0,07 s) |
| sayaç `Term` kilidinden önce, bayrak kilitten sonra | 3/3 kırmızı |
| doldurma kuralı tazeyi ezer ama nesli **tüketmez** | 3/3 yeşil — ve doğrusu bu: kayıp tek karelik ve o karede `content_rows == rows`, yani `gap` sıfır ve doldurma zaten çizmezdi |

Bekçinin iki ayarı **zorunlu** ve ikisi de ölçülerek bulundu: betikteki kısa
`sleep` (temizlemeden sonra ekranın boş kaldığı bir pencere olmazsa kayıp
bayrak bir sonraki `2J` ile anında yerine konar) ve kare döngüsünün ailenin
1 ms'lik uykusu **olmadan**, hasar sormadan koşması (yarışın penceresi kare
yolunun `Term`'ü tutma oranı; uykulu döngüde iki bozuk düzenleme de üçte bir
koşuda yeşil geçiyordu).

### 2. `3J` ve RIS için kol yok

İkisi de `clear_history()` çağırıyor — `3J` doğrudan
(`alacritty_terminal-0.26.0/src/term/mod.rs:1806`), RIS `reset_state` →
`grid.reset()` üzerinden (`grid/mod.rs:341`). `history_size()` sıfıra indiği
için `fill = min(history_size, gap)` kendiliğinden sıfır; üçüncü bir kol,
bayrakla zaten kapalı olan bir yolu ikinci kez kapatmak olurdu. `2J`'nin
ayrı durması da aynı kaynaktan: birincil ekranda `ClearMode::All`
`clear_viewport()` çağırıyor (`term/mod.rs:1794`), yani görünen satırları
**geçmişe itiyor** — ekran boşalırken defter büyüyor.

### 3. Tarayıcının CSI kolu

`vte`'nin dört CSI durumu bizde **tek** (`ScanState::Csi`): tanıdığımız tek
dizi var ve geri kalan her şeyin cevabı aynı — `simple` düşer, dizi yalnız
çerçevelenmek için izlenir. İptal kuralları `vte::anywhere`'den birebir
(`ESC` → `Escape`, `0x18`/`0x1A` → `Ground`), parametre tavanı
`MAX_OSC_NUMBER` emsali.

**OSC'nin sonlandırıcı kümesi burada geçerli değil ve bu tek başına bir kusur
kaynağıydı:** `is_terminator` `BEL`'i (0x07) dizi sonu sayıyor, `vte`'nin CSI
durumları ise onu yerinde `execute` edip durumu değiştirmiyor — `ESC [ 2 BEL J`
hâlâ bir ED 2. İki kümeyi paylaştırmak ızgaranın gördüğü dizi sınırıyla
bizimkini ayırırdı; sınaması `a_bel_inside_a_csi_is_not_a_terminator`.

Kol bir **olay** değil **sayaç** üretiyor (`Scanner::take_screen_clears`):
`ScanEvent` "olay başına tek kilit turu" için var, bu kolun tüketicisi ise
hiç kilit istemiyor.

### 4. Ömrün **üçüncü** koşulu — plandan sapma

R1.2 iki koşul yazıyordu (`content_rows == rows` ve `!alt_screen`);
uygulamada **üç** oldu. Üçüncüsü `display_offset == 0` ve bulgusu
`/code-review`'un: `Cursor::content_rows` **görünür pencereden** doğuyor,
canlı ekrandan değil — tanığı deponun kendi sınaması
`content_rows_come_from_the_visible_window_while_scrolled`, orası `2J`
sonrası 20 çentiğin `content_rows == rows` verdiğini zaten sabitliyor.

Onsuz dizi şu: Ctrl-L → bayrak kurulu → kullanıcı tekerlekle geçmişe bakar →
pencere geçmişle dolar → **bayrak düşer** → kullanıcı dibe döner → doldurma
koşar ve temizlenen ekran geri gelir. R2.2'nin `display_offset == 0` kapısı
kurtarmıyor: o kapı doldurmayı kaydırma *sırasında* durduruyor, bayrağın
kaybı ise kalıcı. Sınama `scrolling_into_history_never_drops_the_flag`, ve
üçüncü terim kaldırılınca kırmızıya düşüyor (ölçüldü). `plan.md`'nin R1.2
satırı ve `CLAUDE.md` aynı commit'te düzeltildi.

Aynı sınıfın daha seyrek hâli — pencereyi `content_rows`'un altına kısmak —
**kapsam dışı**: orada ekran gerçekten doluyor, yani bayrağın düşmesi doğru.

### 5. phase-2'ye geçen sonuç: alternatif ekran bayrağı **kurar**

> **phase-1b'de düzeltildi (2026-09-20):** alternatif ekranın `CSI 2 J`'si
> artık nesli tüketiyor ama bayrağı **kurmuyor**, ve düşme ölçütü defterin
> büyümesi oldu. Aşağıdaki kayıt ölçüldüğü hâliyle duruyor.

`vim` açılışta `CSI 2 J` basıyor, yani bayrak alternatif ekranda da kuruluyor
ve ömrün ikinci koşulu (`!alt_screen`) onu orada düşmekten koruyor. Sonucu
phase-2'nin bilmesi gereken cümle: **vim'den çıkıldığında ekran dolu değilse
doldurma koşmaz**, bayrak ancak ekran doğal yoldan dolunca düşer. Yanlışın
yönü güvenli (doldurma yapmamak bugünkü davranış) ama plan bunu yazmıyordu.

**İkinci sonuç, dock'lu pencereye ait:** caret dock'a devrildiğinde doluluk
giriş satırını saymıyor (`content_rows = drawn_rows.max(1)`), yani prompt'ta
beklerken `content_rows == rows` neredeyse hiç doğru olmuyor — bayrak ancak
çıktısı ekranı dolduran bir komutun karesinde düşüyor. Yönü yine güvenli
(doldurma yapmamak), ama phase-2'nin `gap` aritmetiği aynı sayıdan besleniyor
ve tüketicisi **yalnız** dock'lu pencere: kabul ölçütü buna göre okunmalı.
Bu setin hermetik sınamaları dock'suz oturumda koşuyor.

### 6. Bilinen sınırlar

- **Eşzamanlı güncelleme.** `\e[?2026h` baytları `vte::ansi::Processor`'da
  tamponluyor, yani o blokun içindeki bir `CSI 2 J` uygulanmadan **önce**
  birden çok kare geçebilir ve nesil eşitlendikten sonra dolu bir ızgara
  bayrağı düşürebilir. Birincil ekranda eşzamanlı güncelleme kullanan kabuk
  yok; kapatmanın bedeli "uygulanan bayt sayacı" olurdu ve alacritty'de o
  kanca yok. Kayıt `Session::observe_screen_clear`'ın doc'unda.
- **DCS/APC/PM/SOS gövdeleri** hâlâ `Ground`'da taranıyor (`ESC P`, `ESC _`,
  `ESC ^`, `ESC X` tarayıcıda `Ground`'a düşüyor), yani böyle bir dizinin
  **içindeki** `ESC [ 2 J` sayılır — ızgara onu uygulamadığı hâlde sahte bir
  bayrak (`/code-review`, BULGU 3). Delik OSC kolunda zaten vardı ve orada
  zararsızdı; CSI kolu ona yeni bir sonuç ekliyor. Kapatmanın bedeli dört
  durum daha ve pratikteki olasılığı onu hak etmiyor — ayrıca yönü yalnız
  "fazladan bayrak", yani doldurmanın koşmaması.

### 7. Ölçüm borcu

CSI kolunun yoğun akıştaki (vim, `less`) tarama maliyeti **ölçülmedi**: hızlı
yol (`Ground`'da bir sonraki `ESC`'e zıplama) değişmedi, ama artık her CSI
dizisi bayt bayt adımlanıyor. Sayı uydurulmadı; `docs/OLCUMLER.md`'nin
konusu ve `/measure` ile istenir.
