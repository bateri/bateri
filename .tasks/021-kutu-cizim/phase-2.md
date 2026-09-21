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
- **Ölçüm bekliyor:** phase-1'in kalemi genişliyor — aile 160'tan **416**
  yuvaya çıkıyor (2,6×). Kapasite puntoyla düşüyor ve 32pt@2x'te 338;
  büyük puntoda doyma eşiği `/measure`'ın işi. Kare süresi iddiası yok.
- **`make shader` / `test-yaris` / `kur` / `terminfo`: hayır.**

## Checklist

- [ ] `is_procedural` üçüncü aralığı + köşegen deliği adıyla
- [ ] Vuruş/yay primitifi (`chevron`'un mesafe alanı; `curl` **değil**)
- [ ] `[Style; 4]` tablosu — 128 giriş, permütasyon olduğu doc'ta
- [ ] Kesikli aile + `dividing_period`'un çökme sonucu doc'ta adıyla
- [ ] Yuvarlak köşeler (`╭╮╯╰`)
- [ ] Kalın çizgi çarpanı, tasarım sabiti olarak gerekçeli
- [ ] Test: **dikiş sürekliliği** — sağ kolu olan karakterde `sütun w-1`
      profili == `sütun 0`; alt kolu olanda `satır h-1` == `satır 0`
- [ ] Test: **birleşim yasası** — `┌ ∪ ┘ == ┼` ve ayrık kol kümeli çiftler
- [ ] Test oracle'ı **bağımsız**: kol kümesi tablosu sınamaya ikinci kez ve
      **Unicode adlarından** yazıldı (uygulamanın tablosunu okuyan sınama
      hiçbir şey kanıtlamaz)
- [ ] Test: köşegenler kapsam dışı (üyelik sınaması), aralık sınırları
      (U+257F/U+2580)
- [ ] Test: çöken kesikli çiftler **adıyla muaf** (R7.1) — "hepsi farklı"
      diye bir değişmez yazılamaz
- [ ] Değişmezler **en az üç (punto, ölçek) çiftinde** koşuyor
- [ ] `face_fallback_is_cached_under_the_requested_face` fikstürü yerinde
      (phase-1'de değiştiyse doğrula, değişmediyse **önce** onu ölç)
- [ ] Belgeler: `CLAUDE.md` paragrafı tamam, yol haritası borcu kapandı
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] `make duman` — phase-1'deki not aynen geçerli (regresyon nöbetçisi)
- [ ] **Gözle kontrol**: çizgi örnek sayfası (dört stil × kesişimler) ·
      `tree` · `htop` · `btop` ya da `lazygit` (çizgi ile bloğu aynı
      karede basan bir TUI)
- [ ] Yayın etkisi yazıldı
