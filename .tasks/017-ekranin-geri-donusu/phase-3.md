# Phase 3 — Doldurma: çizim tarafı

## Özet

Doldurulan satırlar ötelemenin üstündeki alana çizilir ve ızgarayla
**birlikte** kayar.

_Requirements: R3.1, R3.2_

## phase-2'den devralınan

Sınır **hazır ve bugün tüketilmiyor** — bu phase'in ilk işi o iki ucu bağlamak
(`phase-2.md` → Uygulama Notları §1):

1. **`frame()`'in ikinci sink'i.** İmza `frame(sink, fill_sink, blocks)`;
   doldurma hücreleri oradan geçiyor ve satır numaraları **fill-yerel**
   (`0..fill`, `0` en eski, `fill - 1` içeriğin hemen üstü). Ekran satırına
   çeviren taraf bu phase. `bt-gpu::link` bugün `|_| ()` veriyor, yeri
   yorumla işaretli.
2. **`Cursor::fill`.** Kaç satır geldiğini o söylüyor; `content_rows`'a
   **girmiyor**, yani öteleme aritmetiği phase-2'de dokunulmadan kaldı (R2.3).
3. **Geri alma şeridi kurulu.** `Session::fill_rows` sıfır dediğinde ikinci
   sink hiç çağrılmıyor; `fill == 0` iken kareyi bit bit aynı tutmanın
   `bt-core` yarısı bitti, `bt-gpu` yarısı bu phase'in Kabul'ünde.
4. **Doldurmada seçim yok** (phase-2 §5) — vurgusuz hücreler bekleniyor, bu
   bir eksik değil karar.

## Kol seçimi phase-0'dan gelir

Push anında aritmetik (`origin - fill + row`) **elendi** ve gerekçesi repoda
yazılı: `renderer.rs:715-719` dock'un muafiyetinin aritmetikle kurulamadığını
söylüyor (`set_origin_rows` sink'ten sonra çağrılıyor), ve hareket karesi
listeleri koruyup yalnız `origin_px`'i yeniden yazdığı için (`frame.rs:1774`)
push anında pişmiş bir konum her kayma karesinde bayat olur — 200 ms boyunca
ızgara süzülürken doldurma yerinde donar. Ayrıca `[0, origin_px)` ızgara
viewport'unun **üstünde** kalıyor ve Metal orayı kırpıyor
(`renderer.rs:643-647`), yani negatif satır da çare değil.

- **2b-i — üçüncü `setViewport`**, `originY = origin_px - fill_px`. Dock
  emsalinin birebiri (`renderer.rs:734`).
- **2b-ii — okuma anında çeviri**, caret emsali (`frame.rs:1215`).

## Değişiklikler

- **`crates/bt-gpu/src/frame.rs`** — doldurma listeleri, dock örüntüsünde:
  kendi `bg`/`glyph`/`rule` listeleri, `clear` temizler, `move_caret` korur.
  **Sayaçlardan muaf** (R3.2): `hucre=`/`glif=`/`kural=` jetonlarına girmez.
  Kardeş bekçi `frame.rs:2191`'in (`the_dock_keeps_its_own_lists_and_stays_out_of_the_counters`)
  eşi yazılır — duman sözleşmesi `hucre=8 glif=6 kural=15` bit bit korunmalı.
- **`crates/bt-gpu/src/renderer.rs`** — doldurmanın encode'u, **ızgaradan
  sonra dock'tan önce**. Sıra keyfi değil: kayma boyunca ızgaranın üst satırı
  doldurma bandına taşıyor ve dock'un opak zemini en altta kalmalı.
- **`crates/bt-gpu/src/link.rs`** — `Cursor::fill` `Frame`'e geçer; hareket
  karesinin yolu (`frame.rs:1774` emsali) doldurmayı da tazeler ya da
  viewport'unu yeniden hesaplar — hangisi phase-0'ın koluna bağlı (R3.1).

## Kabul

- Ekran dolu → Tab → Ctrl-C: doldurulan satırlar **içeriğin hemen üstünde**,
  ekran tam dolu, delik yok.
- Kayma boyunca (200 ms) doldurma ızgarayla birlikte kayıyor; ikisinin
  arasında dikiş yok. Bekçi hareket karesini taklit eder (yalnız `origin_px`
  değişir, listeler korunur).
- `fill == 0` iken çizilen kare bugünküyle bit bit aynı; `hucre=8 glif=6
  kural=15` oynamıyor.
- Dock'u olmayan pencerede doldurma encode'u **hiç kurulmuyor** (dock
  emsali, `renderer.rs:743`).
- `make hepsi` ve `make shader` (dokunulduysa) yeşil; `make duman` jetonları
  değişmemiş.

## Yayın Etkisi

- **shader:** yeni `.metal` beklenmiyor — doldurma mevcut `cell_bg`/`cell`
  pipeline'larını kullanır. Dokunulursa `make shader` koşar ve `#[repr(C)]`
  ↔ `.metal` alan alan karşılaştırılır. `stride 32` assert'leri değişmiyor
  (yeni alan yok).
- **`CLAUDE.md`:** "`setViewport` dört listeyi birden kaydırıyor" ve
  `frame.rs:575`'in `origin_px` doc'u — liste sayısı ve uzay sayısı
  güncellenir.
- **Duman kapısı bu özelliğe yapısal olarak kör:** süreli koşu `/bin/sh`
  koşuyor, dock yok, doldurma hiç tetiklenmiyor. `icerik`/`sessiz`/`kayma`
  oynamıyor — iyi haber ve aynı zamanda uyarı: doldurmadan doğan bir kare
  sızıntısını duman göremez. Tek koruma birim bekçi + gözle kontrol.
  "Duman'a dock ekleyelim" önerisi kapıyı gevşetir, açılmaz.
- terminfo / ayar şeması / tema / shell entegrasyonu / app bundle / yeni
  bağımlılık: yok.

## Checklist

- [x] phase-0'ın seçtiği kol uygulandı (2b-i, `Renderer::encode_fill`)
- [x] Doldurma listeleri sayaçlardan muaf, kardeş bekçi yazıldı
      (`the_fill_keeps_its_own_lists_and_stays_out_of_the_counters`)
- [x] Encode sırası: ızgara → doldurma → dock
- [x] Test: hareket karesinde doldurma ızgarayla birlikte kayıyor
      (`the_fill_band_draws_above_the_content_and_rides_the_origin` +
      `the_fill_band_rides_the_origin`)
- [x] Test: `fill == 0` iken çizim bit bit aynı
      (`a_frame_without_fill_draws_todays_picture`)
- [x] Test: dock'u olmayan pencerede encode kurulmuyor — zincirle (§3):
      `bt-core`'un `a_window_without_a_dock_never_fills_the_gap`'ı +
      `fill_rows() == 0` kapısı + `clear`'ın bandı sıfırlaması
- [x] Doğrulama geçti (`make hepsi`)
- [x] Riskli phase **değil**: `.metal` ve `#[repr(C)]` düzeni dokunulmadı,
      `stride 32` assert'leri aynı; `make shader` ve `/code-review`
      tetiklenmedi
- [x] Yayın etkisi yazıldı

## Uygulama Notları

### 1. İkinci sink `Frame`'e **doğrudan** akamadı: tampon gerekti

`frame(sink, fill_sink, blocks)`'in iki kapatması da `frame`'i ödünç alsaydı
aynı çağrıda iki `&mut` doğardı; borç kuralı bunu derleme zamanında kapatıyor.
Çözüm `blocks`'un ta kendisi — `LinkIvars::fill`, `Frame`'in **içinde değil
yanında** bir `RefCell<Vec<Cell>>`; çağrı dönünce hücreler `Frame::push_fill`
ile banda geçiyor. Boşaltılıp yeniden doluyor, yani kare başına ayırma yok;
doldurması olmayan pencerede sınır sink'i hiç çağırmıyor ve tampon boş
kalıyor. Plan'ın "Değişiklikler"i bu ucu saymıyordu.

### 2. Encode sırasının gerekçesi planda yazıldığı gibi **değil**

Plan "kayma boyunca ızgaranın üst satırı doldurma bandına taşıyor" diyordu;
ızgaranın listeleri her iki kayma yönünde de `y ≥ origin_px` çiziliyor, yani
bandın içine **hiç** girmiyorlar. Giren tek şey ötelemeden muaf olan caret
(ekran satırı, `Frame::push_caret`). Sıra **değişmedi** — dock'un opak zemini
en altta kalmak zorunda ve bant ondan önce — ama `encode_fill`'in doc'u gerçek
gerekçeyi taşıyor: bu depo kodla çelişen cümleyi aynı commit'te düzeltiyor.

### 3. "Dock'suz pencerede encode kurulmuyor" GPU'dan **gözlenemez**

`setViewport` çağrıları CPU'dan okunmuyor, yani bu maddeyi doğrudan söyleyen
bir sınama uydurma olurdu. Zincir üç halka: kapı `bt-core`'da açılıyor
(`Session::fill_rows` dock'suz pencerede koşulsuz sıfır,
`a_window_without_a_dock_never_fills_the_gap`), `encode_fill` `fill_rows() ==
0`'da erken dönüyor, ve `Frame::clear` bandı sıfırlıyor
(`the_fill_keeps_its_own_lists_...`'in son bloğu) — üçüncüsü olmadan bir
önceki karenin bandı asılı kalırdı.

### 4. Orijin formülü `Frame`'de, `renderer.rs`'te değil

Dock kendi orijinini `renderer.rs`'te kuruyor (`viewport_px[1] - dock_px()`),
çünkü terimlerinden biri dokunun boyu. Doldurmanın iki terimi de `Frame`'in
kendi alanları (`origin_px`, bandın satırı × hücre boyu), yani
`Frame::fill_origin_px` hem formülün tek kopyası hem de R3.1'in CPU'dan
sınanabilir hâli: hareket karesi yalnız `origin_px`'i yazıyor ve bant onu
**okuma anında** görüyor.

### 5. Bandın glyph encode'una **dejenere** `CursorBlock` gidiyor

Emsal sol payın blok işaretleri. Bandın caret yuvası yok — caret'in ekran
satırı yerleşik karede her zaman içeriğin içinde — ve gerçek dikdörtgeni
geçirmek, kaymanın ortasında bandın üstünden geçen bir caret'in altındaki
harfi zemin rengine boyardı: çizilmemiş bir caret için okunmaz bir hücre.

### 6. "Bit bit aynı"nın sınanabilir hâli üç okuma

Tek kare iki kez çizilip karşılaştırılsaydı sınama determinizmi ölçerdi.
Bekçi üç okuma alıyor: doldurmasız kare → doldurmalı kare (**ayrışmak
zorunda**, yoksa eşitlik hiçbir şey söylemez) → yeniden doldurmasız kare, ve
birinciyle üçüncüyü **bütün tampon** üzerinden karşılaştırıyor. Sessiz
kalabilecek tek kusur — `clear`'ın bandın boyunu unutması ya da viewport'un
koşulsuz kurulması — tam orada düşüyor.
