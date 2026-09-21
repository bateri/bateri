# Phase 2 — Çizgiler

## Özet

U+2500–U+257F yordamsal çiziliyor: dört kol × {yok, ince, kalın, çift},
kesikli aile ve yuvarlak köşeler.

_Requirements: R1, R3.1, R5, R6, R7, R7.1, R8, R9, R10_

## Neden ayrı phase

Phase-1 kapıyı, normalizasyonu, üyelik yüklemini ve sınama iskeletini 32
karakterlik bir ailede kurdu. Bu phase setin **en çok karar taşıyan**
yarısı: 128 karakterlik gerçek bir tablo, üçüncü bir primitif (vuruş/yay)
ve kesikli periyot çatalının sonucu. İkisi tek incelemede boğulurdu ve
phase-1 tek başına `make hepsi`'yi yeşil bırakıyor.

## Değişiklikler

- **`crates/bt-atlas/src/raster.rs`**
  - `is_procedural` üçüncü aralığı kazanıyor: U+2500–U+257F, **köşegenler
    `╱╲╳` (U+2571–U+2573) hariç** (R6). Ret aralığın içinde bir delik, yani
    yüklem bunu adıyla söylemeli.
  - **Üçüncü primitif: vuruş ve yay.** Emsal `chevron`'un mesafe alanı
    (`distance_to_segment` + `(half_stroke + 0.5 - d).clamp(0,1)`); yayın
    karşılığı `|hypot(x - cx, y - cy) - r|`, aynı formül ve aynı
    `half_stroke`. **`curl` emsal değil** — sütun başına tek `y`
    örnekliyor ve çeyrek yayın dikey teğetinde bant kopar.
    Eksen hizalı vuruşta mesafe tabanlı AA ile `coverage` cebirsel olarak
    aynı sonucu veriyor (`rule_envelope` kalınlığı `>= 1`'e bağlıyor), yani
    düz kollar da bu primitiften çıkıyor ve yay ayrı bir teknik değil.
  - **Tablo:** kol stilleri `[Style; 4]` olarak, karakter başına bir giriş.
    Formül **çıkmaz** ve bu bir gözlem: T-bağlantı (8'li) ve artı (16'lı)
    grupları kalın-maskesinin permütasyonu, sayaç değil — `251C..2523`'ün
    (up, down, right) maskeleri sırayla 000, 001, 100, 010, 110, 101, 011,
    111.
  - **Tasarım sabiti (R7):** kalın çizginin çarpanı. İnce
    `underline_px.1`'den geliyor; kalın için ikinci bir sayı gerek ve o
    sayı ölçüm değil tasarım kararı (`CURL_FACTOR` emsali, doc'la).
  - **Kesikli aile (R7.1):** `dividing_period` **korunuyor**. Sonucu
    doc'ta adıyla yazılı olmalı: `w = 8`'de üçlü periyot 4'e çekiliyor ve
    `┄` ile `╌` **aynı sprite'a** çöküyor; asal genişlikte her yoğunluk
    hücre başına tek tireye iniyor. Döşeme bu setin varlık sebebi, kayıp
    kabul edildi.
- **`crates/bt-atlas/src/lib.rs`** — `face_fallback_is_cached_under_the_
  requested_face` bu phase'de **kırılır** (fikstürü `─`, normalizasyondan
  sonra `(Char('─'), Bold)` anahtarı hiç oluşmuyor). Phase-1 fikstürü
  ölçerek değiştirdiyse burada iş yok; değiştirmediyse **önce o** (R3.1,
  R9). Sınama `DrawResult::NoGlyph if face != Regular` kolunun tek bekçisi
  ve yeşil kalarak ölmesi en kötü hâl.

## Kabul

- Yan yana iki `─` arasında dikiş yok; alt alta iki `│` arasında da.
- `┌ ∪ ┘ == ┼` (piksel-max), ve aynı yasa ayrık kol kümeli her çift için.
- Kalın çizgi inceden **kalın**, çift çizgi iki ayrı banttan oluşuyor.
- Köşegenler (`╱╲╳`) hâlâ fonttan geliyor.
- `make hepsi` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** kutu çizim paragrafı bu phase'de **tamamlanıyor** —
  "Kutu ve blok çizim… çaresi yedek değil yordamsal çizim; üçü
  `docs/YOL-HARITASI.md`'de tek borç" cümlesi artık geçmiş. Köşegenlerin
  dışarıda kaldığı da yazılmalı: kapsamın içinde bilerek bırakılmış bir
  delik, sessiz bir eksik değil.
- **`docs/YOL-HARITASI.md`:** "blok, çizgi ve Braille fonttan geliyor ve
  döşemiyor" borcu **kapanıyor**.
- **`RuleKind` doc'u:** phase-1'de ikinci küme doğmuştu; bu phase o
  kümenin sınırını netleştiriyor.
- **Ayar şeması: yok.**
- **Ölçüm bekliyor:** phase-1'in kalemi genişliyor — aile 160'tan **413**
  yuvaya çıkıyor (2,6×; 416 değil, üç köşegen kapsam dışı). Kapasite puntoyla
  düşüyor ve 32pt@2x'te 338; büyük puntoda doyma eşiği `/measure`'ın işi.
  Kare süresi iddiası yok — çizim yuva başına ömürde bir kez koşuyor.
- **`make shader` / `test-yaris` / `kur` / `terminfo`: hayır.**

## Uygulama Notları

- **Raylar piksel ızgarasına oturtuluyor ve bu plandan sapma** (`raster::rail`).
  Ölçüldü (bu makine, Menlo 13pt@1x, `line_height = 1.0`): hücre 8×18, yani
  dikey çizginin ekseni x = 4.0 ve yuvarlanmamış bir ince bant `[3.5, 4.5)`
  iki sütuna %50'şer düşüyor; yatayın ekseni 9.0 ve bandı `[8.5, 9.5)` —
  aynı sorun. Kural yeni bir tasarım kararı değil, depoda zaten var olanın bu
  eksene taşınması: alt çizginin konumu da kalınlığı da tam sayı
  (`Metrics::underline_px`). Kolların uzantısı da oturtulmuş bantlardan
  türüyor, yani dikiş ve birleşim yasası **tam** kalıyor.
- **Düz kollar `max_rect`'ten, yalnız yay mesafe alanından.** Phase dosyası
  "düz kollar da bu primitiften çıkıyor" diyordu; ikisi cebirsel olarak aynı
  sonucu veriyor (eksen hizalı bantta `clamp(ht + 0.5 - |d|)` ile `overlap`
  eşit, `ht >= 0.5` iken — `rule_envelope` kalınlığı `>= 1`'e bağlıyor), o
  yüzden düz kollar mevcut dikdörtgen primitifinde kaldı: tam sayı kenarlarda
  kesir üretmiyor ve ikinci bir kod yolu doğmuyor. Yay (`corner`) gerçekten
  `chevron`'un ikizi.
- **Setin asıl içeriği tablo değil, çift çizginin kavşak kuralı çıktı.**
  Tablo 128 satır ve mekanik; kararı taşıyan şey rayın nereye kadar
  gideceğiydi (`arm` + `reach`). Kural tek cümleden türedi — çift çizgi bir
  çizgi değil iki duvarlı bir **kanal**, kavşakta açılan duvar kanalı
  kapatmaz — ve iki kola ayrıldı: çift rayın kendi tarafındaki dik kol da
  çiftse ray **döner**; tek ray ise yalnız dik kolların ikisi de çiftken
  **ve karşı kolu yokken** döner. İkinci koşul olmasaydı `╪`'nin dikey
  çizgisi ortadan ikiye bölünürdü (`╤`'ninki ise bölünmek zorunda).
- **Birleşim yasası çift çizgide geçmiyor ve geçmemeli.** `╔`'in üst rayı
  köşeyi kapatmak için kavşağı geçiyor, `╬`'te aynı ray dirsek yapıp
  duruyor; yani `╔ ∪ ╝ ≠ ╬`. Muafiyet adıyla yazıldı ve karşılığı ayrı bir
  bekçi: `double_junctions_keep_the_channel_open` sekiz adlandırılmış
  iddiayla (`╬ ╋ ╠ ╦ ╪ ╫ ╤ ╒`) dönme/geçme kararının tek tanığı — dikiş de
  birleşim de o farkı göremiyor, ikisi de kenarlara ve toplama bakıyor.
  İçindeki ölçüt de düzeldi: "boş satır var mı" değil **iç** boşluk, çünkü
  `╒`'nin üstündeki sekiz boş satır kanal değil karakterin dışı.
- **Oracle uygulamanın tablosunu hiç okumuyor** ve gözlemlenebilir olanı
  sınıyor: beklenti Unicode **adından** ayrıştırılıyor (adlar
  `unicodedata`'dan, UCD 16.0), iddia ise kenar profili — "her karakterin
  kenarı o stilin tek kollu referansına eşit". Mutasyonla doğrulandı: tabloda
  `├` ile `┤` yer değiştirdiğinde `the_arms_come_from_the_unicode_names` ile
  `arms_tile_across_the_cell_edge` düşüyor, **birleşim yasası yeşil kalıyor**
  (`├ ∪ ┤ == ┼` aynalamaya karşı kör). Karar 7'nin "doğru geometri, yanlış
  karakter" endişesinin ölçülmüş hâli. Ayrıştırıcının kendi bekçisi de var
  (`the_name_parser_reads_the_grammar`): dilbilgisinin dört tuzağı — miras
  alınan stil, `DOUBLE DASH`'in stil olmaması, `SINGLE`'ın ince demesi, `ARC`.
- **Kesikli çökmesi listeye yazılmadı, türetildi.** Sayı puntoya bağlı ve
  ölçüldü: `w = 8`'de `┄` ile `╌` aynı sprite (üçlü periyot 3 → 4), ama
  144pt@1x'in `w = 87`'sinde (bölenleri 1, 3, 29, 87) çöken çift `┄` ile `┈`
  oluyor ve `╌` hücre başına tek tireye iniyor. Bekçi bu yüzden eşitliği
  `dividing_period` eşitliğine bağlıyor; listeye yazılmış bir çift başka
  puntoda yanlış olurdu.
- **Yüz merdiveninin fikstürü yerinde** (R3.1/R9): phase-1 onu `╱`'ye
  taşımıştı ve köşegenler bu phase'de de kapsam dışı kaldığı için
  `face_fallback_is_cached_under_the_requested_face` dokunulmadan yeşil.
  Bağı görünür kılmak için `raster::family`'nin köşegen koluna ve
  `the_diagonals_stay_out_of_scope`'a yazıldı: delik kapanırsa o sınama
  sessizce ölür.
- **Bayat bir ölçü düzeltildi.** Üç doc ve iki sınama yorumu "13pt@1x hücresi
  8×17, yükseklik asal" diyordu; ölçüldü (`Atlas::new(None, 13.0, 1.0, 1.0)`)
  ve hücre **8×18**. Argümanın kendisi ayakta (dama deseni hücrenin iki
  ölçüsünü de bölmek zorunda) ama örneği yanlıştı; örnek ölçülmüş bir boyutla
  değişti: 13pt@2x hücresi 16×33 ve 33 tek. Aynı düzeltme `add_rect`'in
  "yarım 8.5'e düşüyor" örneğine de girdi (33'te 16.5).
- **Duman jetonları birebir aynı**: `hucre=8 glif=6 kural=15 yuva=13/1984`
  (phase-1'in satırı); `kare` ile `istek` koşudan koşuya oynuyor ve
  sözleşmenin sabit kısmı değil.

## Checklist

- [x] `is_procedural` üçüncü aralığı + köşegen deliği adıyla
- [x] Yay primitifi (`chevron`'un mesafe alanı; `curl` **değil**) — düz
      kollar `max_rect`'te kaldı, gerekçe Uygulama Notları
- [x] `[Stroke; 4]` tablosu — 128 giriş, permütasyon olduğu doc'ta
- [x] Kesikli aile + `dividing_period`'un çökme sonucu doc'ta adıyla
- [x] Yuvarlak köşeler (`╭╮╯╰`)
- [x] Kalın çizgi çarpanı (`HEAVY_FACTOR`), tasarım sabiti olarak gerekçeli
- [x] Test: **dikiş sürekliliği** — sağ kolu olan karakterde `sütun w-1`
      profili == `sütun 0`; alt kolu olanda `satır h-1` == `satır 0`
- [x] Test: **birleşim yasası** — `┌ ∪ ┘ == ┼` ve ayrık kol kümeli çiftler
- [x] Test oracle'ı **bağımsız**: kol kümesi tablosu sınamaya ikinci kez ve
      **Unicode adlarından** yazıldı (uygulamanın tablosunu okuyan sınama
      hiçbir şey kanıtlamaz)
- [x] Test: köşegenler kapsam dışı (üyelik sınaması), aralık sınırları
      (U+257F/U+2580)
- [x] Test: çöken kesikli çiftler **türetilerek muaf** (R7.1) — "hepsi farklı"
      diye bir değişmez yazılamaz
- [x] Değişmezler **en az üç (punto, ölçek) çiftinde** koşuyor
- [x] `face_fallback_is_cached_under_the_requested_face` fikstürü yerinde
      (phase-1'de değiştiyse doğrula, değişmediyse **önce** onu ölç)
- [x] Belgeler: `CLAUDE.md` paragrafı tamam, yol haritası borcu kapandı
- [x] Doğrulama geçti (`make hepsi`)
- [x] `make duman` — jetonlar birebir aynı (regresyon nöbetçisi)
- [x] **Gözle kontrol** (gerçek doğrulama; birleşim yasası ile dikiş
      "doğru geometri, yanlış karakter"i göremez): çizgi örnek sayfası —
      125 karakterin tamamı, altı çerçeve (ince · kalın · çift · yuvarlak ·
      ince-kalın · tek-çift karışık), kesikli aile, uzun satırda döşeme ve
      phase-1'in blok/Braille regresyonu — kullanıcı onayladı (2026-09-21)
- [x] Yayın etkisi yazıldı
