# Phase 1 — Kapı, bloklar ve Braille

## Özet

Yordamsal çizim kapısı `Atlas::slot`'ta açılıyor; blok elemanları ve
Braille fonttan değil terminalden geliyor.

_Requirements: R1, R2, R2.1, R3, R3.1, R4, R5, R7, R8, R9, R10_

Kullanıcının gördüğü iki şey burada: **maskot** blok elemanı, **spinner**
Braille. Çizgiler phase-2'de ve belirtiyi kapatmak için gerekmiyor —
üstelik altyapı 128 karakterlik tablo yerine 32 karakterlik ailede doğuyor.

## Değişiklikler

- **`crates/bt-atlas/src/raster.rs`**
  - `is_procedural(ch) -> bool` — üyelik yükleminin **tek sahibi** (R2.1).
    Bu phase'de yalnız iki aralık: U+2580–U+259F ve U+2800–U+28FF.
    Çizgiler phase-2'de aynı fonksiyona ekleniyor.
  - `draw_procedural(ch, metrics, target)` — `draw_rule`'un ikizi ve aynı
    iki açılış satırı **önce**: `slot_bytes` assert'i, `fill(0)`.
    Başarısız olamaz; `Drawn` bir varsayım değil tipin kendisi.
  - **İki primitif** bu phase'de doğuyor ve ikisi de mevcut yardımcılardan:
    dikdörtgen (`coverage`'ın iki eksende çarpımı) ve birleşim (piksel-max,
    `band`'in `Double` kolundaki `max` emsali). Vuruş/yay phase-2'de.
  - **Tasarım sabitleri** (R7, `CURL_FACTOR` emsali, doc'la): gölgelerin
    dama adımı ve Braille'in nokta geometrisi (2×4 ızgarada yarıçap + kenar
    payı). Kalın çizgi çarpanı phase-2'nin.
  - **Braille'in tablosu yok:** kod noktasının alt 8 biti doğrudan nokta
    maskesi. Bit → hücre eşlemesi Unicode'un tanımı ve doc'ta adıyla yazılı
    olmalı (bit 0–2 sol sütun, 3–5 sağ sütun, 6/7 dördüncü satır).
- **`crates/bt-atlas/src/lib.rs`**
  - Normalizasyon `match`'ine dördüncü kol (R3): `(Char(ch), Normal) if
    is_procedural(ch) => (Regular, Normal)`. Deseni `SizeClass::Normal`,
    `_` **değil** — `_` küçük isteği `Normal`'e zorlar ve kapıyı tam da
    kapatılmak istenen yerde açardı (R4). Üç maddelik gerekçe yorumu
    dördüncüyü kazanıyor.
  - Çizim `match`'ine `Sprite::Char` kolundan **önce** guard'lı kol (R2):
    `size == Normal && is_procedural(ch)`. Yedek kolu aşağıda kalıyor,
    yani `⠋` artık `fallback_font`'a hiç gitmiyor.
  - Başlık yorumu ve `Atlas` struct doc'u düzeliyor (R10).
- **Kırılan sınama (R9):** `the_gate_decides_by_width_alone`'un `⠋` probu
  (`lib.rs:628`) bu phase'de kırılır — sınama beklentiyi `fallback_font`'tan
  türetip `TOFU` bekliyor, oysa `slot()` artık gerçek bir yuva veriyor.
  Çare probu silmek **değil**: `continue` kapısı kapının kendi yüklemiyle
  genişletiliyor; `wide > 0` bekçisi `𝔸 漢 🎉` ile ayakta kalıyor.
  İkinci sınama (`face_fallback_is_cached_under_the_requested_face`,
  fikstürü `─`) phase-2'de kırılır ama fikstürü **burada** ölçmek phase-2'yi
  temiz indirir (R3.1).

## Kabul

- `█` (U+2588) sprite'ı **bit bit 255**; alt alta iki blok arasında şerit
  yok (bildirilen kusurun tam tersi, `> 0` değil eşitlik).
- Braille sprite'ları kod noktasının bitlerinden türüyor: `0x2800` boş,
  `sprite(0x2800|mask)` set bitlerin piksel-max'i, sekiz noktanın desteği
  ayrık.
- `⠋` artık tofu değil; genişlik kapısı onu hiç görmüyor.
- Kalın yüzdeki blok karakteri düz yüzle **aynı yuvayı** paylaşıyor.
- Dock'un bağlam satırındaki blok karakteri hâlâ fonttan geliyor (R4).
- `make hepsi` yeşil.

## Uygulama Notları

- **Fikstür ölçüldü ve çıkan sayı kapsamı bağladı.** Menlo Regular'da olup
  Bold'da olmayan kod noktaları BMP+SMP'nin tamamında tarandı (bu makine,
  macOS 26.4.1, Menlo 13pt): **tek** blok çıktı, U+2500–U+257F, tam 128
  karakter. Yani `face_fallback_is_cached_under_the_requested_face`'in
  fikstürü zorunlu olarak o bloğun içinden ve zorunlu olarak 021'in kapsamı
  **dışından** olmak zorunda — geriye Karar 3B'nin bilerek bıraktığı üç
  köşegen kalıyor. Fikstür `╱` (U+2571) oldu ve sınamanın doc'una şu yazıldı:
  köşegenlerin kapsam dışı kalması artık `DrawResult::NoGlyph if face !=
  Regular` kolunun bu makinedeki **tek** bekçisini ayakta tutan şey. Delik
  kapansaydı kol sessizce ölürdü.
- **Gölgeler dama deseniyle değil düz kapsamayla çiziliyor** — plandan sapma
  ve gerekçesi planın kendi ölçütü. Karar 4 dama adımını "komşu hücrelerde
  faz tutması için mutlak olmalı" diye istemişti; faz ancak adım hücrenin
  **iki** ölçüsünü de bölerse tutar ve bu makinede 13pt@1x hücresi 8×17, yani
  yükseklik asal — `░` ile dolu bir alanda her satır sınırında yatay şerit
  belirirdi. Atlas sekiz bitlik, dama ise tek bitlik ekranların yoğunluk
  hilesi. Tasarım sabiti yine tek ve adıyla: `SHADE_LEVELS` (çeyrek, yarım,
  üç çeyrek). Bekçisi `the_shades_are_flat_and_ordered` — "tek değerli"
  iddiasını doğrudan sınıyor. **Sapma kullanıcıya soruldu ve onaylandı**
  (2026-09-21): iki kol yan yana gösterildi, düz kapsama seçildi.
- **Birleşim tek değil iki primitif.** Plan "birleşim (piksel-max)" diyordu;
  ölçüldü ki `max` ayrık döşemede yanlış: h = 17'de yarım 8.5'e düşüyor, `▀`
  ile `▄` o satıra 128'er bırakıyor ve `max` hücrenin **ortasında** %50'lik
  bir şerit bırakırdı — bu setin kapatmaya geldiği kusurun hücre içine
  taşınmış hâli. Ayrım ölçüte bağlandı: `add_rect` (doygun toplam) birbirini
  **döşeyen** parçalar için, `max_rect` (piksel-max) üst üste binen mürekkep
  için. Braille ikincisini kullanıyor ve kazanç yapısal — "maskenin sprite'ı
  = set bitlerin piksel-max'i" değişmezi noktaların ayrıklığından değil
  birleştiricinin kendisinden geliyor.
- **`coverage` ikiye ayrıldı** (`overlap` oranı veriyor, `coverage` onu
  yuvarlıyor). Dikdörtgen iki eksende örtüşüyor ve iki `coverage` **baytını**
  çarpmak iki kez yuvarlıyor: `▀` + `▄` 255'te durmuyor, birkaç eksik kalıyor
  ve o eksik tam da şeridin sönük bir kopyası. Oranlar çarpılıp **bir kez**
  yuvarlanıyor.
- **Değişmezler `Atlas::slot`'tan değil `raster::draw_procedural`'dan
  koşuyor.** `LARGE_POINT_SIZE`'ta kapasite birkaç düzine yuva ve tek başına
  256 Braille deseni oraya sığmıyor — `slot()` üzerinden koşan bir bekçi
  tofu'ya düşer, `Upload` hiç gelmez ve geometri yerine kapasite sınanmış
  olurdu. Kapının `slot()` yolunda gerçekten koştuğunu gösteren iki ayrı
  bekçi 13pt'de: `procedural_chars_share_one_slot_across_faces` ve
  `the_small_class_still_asks_the_font`.
- **`the_gate_decides_by_width_alone`'un `continue` kapısı yüklemi
  birebir tekrarlıyor** (`size == Normal && is_procedural(ch)`),
  `is_procedural(ch)` tek başına değil: `⠋` küçük sınıfta hâlâ yedek
  yolundan geçiyor ve o sınamada **kapalı kapının tek tanığı** o. Tek başına
  yazılsaydı probun tamamı elenir ve R4 orada hiç sınanmazdı.
- **Sekizde bir merdiveninin iki yönü var** ve ikincisi ters: alttan `▁..█`
  kod noktası artarken büyüyor, soldan `▏..▉` kod noktası **azalırken**.
  Bekçi ikisini de yürüyor; bir işaret hatası yalnız ikinci yönde sessiz
  kalırdı (merdiven yine merdiven görünür, yalnız ters).
- **`make hepsi` bir kez `bt-shell`'de SIGSEGV ile düştü** (`bt_shell` lib
  sınamaları), ardından üç koşuda da yeşil geçti. Bu phase `bt-shell`'e
  dokunmuyor; kayda geçiriliyor, kovalanmıyor.
- **Duman jetonları birebir aynı**: `kare=30 hucre=8 glif=6 kural=15
  yuva=13/1984 … sessiz=1756.52ms kapanis=clean`.

## Yayın Etkisi

- **`CLAUDE.md`:** "Kutu ve blok çizim ayrı bir olgu… çaresi yordamsal
  çizim" paragrafı artık geçmiş zamanlı yazılmalı; Braille'in kapıdan
  elendiği cümle de. (Çizgiler phase-2'de bittiğinde paragraf tamamlanır —
  bu phase yalnız kendi yarısını düzeltir.)
- **`bt-atlas` başlık yorumu ve `Atlas` doc'u:** "kutu çizim karakterleri
  de öyle" / "emoji ve kutu çizim kapsam dışı" cümleleri kodla çelişiyor.
- **`RuleKind` doc'u:** "bu enum yordamsal çizilen sprite'ların kümesi"
  artık **tek küme değil** — `draw_procedural` ikinci kümeyi getiriyor.
- **Ayar şeması: yok.** Yeni anahtar yok, varsayılan değişmiyor.
- **Ölçüm bekliyor — KAPANDI (2026-09-21, `/measure`):** kutu ailesinin yuva ayak izi (`yuva=U/T`,
  `Atlas::occupancy`) ve **büyük puntoda doyma eşiği**. Braille bugün
  **sıfır** yuva harcıyor (kapıdan dönüp tofu'ya bağlanıyor) ve bu
  phase'den sonra 256 yuva garantili harcayacak; 32pt@2x'te kapasite 338,
  yani büyük puntoda aile kapasiteyi zorluyor ve tahliye yok. Atlas doyması
  bugün de mümkün — set onu daha erken erişilebilir kılıyor. Sayılar ve
  gerekçeleri `discussion.md` → Muhakeme'nin sonunda; `/measure`'ın işi.
- **Kare süresi iddiası yok** ve bu bilerek: çizim yuva başına ömürde bir
  kez koşuyor (`draw_rule` emsali), kare başına yeni iş doğmuyor.
- **`make shader` / `test-yaris` / `kur` / `terminfo`: hayır.** `.metal`
  yok, paylaşılan durum yok, `assets/` yok, `Cargo.lock` oynamıyor.

## Checklist

- [x] `raster::is_procedural` (iki aralık) + `draw_procedural`
      (`draw_rule`'un iki açılış satırı önce)
- [x] Dikdörtgen ve birleşim primitifleri, mevcut `coverage`/`max`
      üstünden
- [x] Blok elemanları: `█`, sekizde birlik merdivenler, çeyrekler, üç gölge
- [x] Braille: alt 8 bitten nokta maskesi, **tablo yok**
- [x] Tasarım sabitleri adıyla ve gerekçeli (`SHADE_LEVELS` — dama adımı
      **değil**, bkz. Uygulama Notları; `BRAILLE_DOT_FILL`)
- [x] `Atlas::slot`: normalizasyon kolu (deseni `Normal`, `_` değil) +
      guard'lı çizim kolu
- [x] Test: `█` bit bit 255
- [x] Test: Braille bit-max değişmezi (`0x2800` boş, maskelerin birleşimi,
      sekiz noktanın desteği ayrık)
- [x] Test: sekizde bir merdiveni monoton iç içe; `▀` + `▄` doygun toplamı
      255
- [x] Test: kalın yüz ile düz yüz **aynı yuvayı** paylaşıyor
- [x] Test: küçük sınıfta kapı kapalı (fonttan geliyor)
- [x] Değişmezler **en az üç (punto, ölçek) çiftinde** koşuyor
- [x] `the_gate_decides_by_width_alone`'un `continue` kapısı genişletildi
      (`⠋`), `wide > 0` bekçisi ayakta
- [x] `face_fallback_is_cached_under_the_requested_face`'e yeni fikstür
      **ölçülerek** bulundu
- [x] Belgeler (R10'un bu phase'e düşen yarısı)
- [x] Doğrulama geçti (`make hepsi`)
- [x] `make duman` — **regresyon nöbetçisi, kanıt değil**: duman betiği
      donmuş (`session.rs`: "ikinci bir yük buraya eklenmez") ve
      `hucre=8 glif=6 kural=15` jetonları bu setin karakterlerine hiç
      dokunmuyor; birebir aynı kalmalı
- [x] **Gözle kontrol** (gerçek doğrulama; birleşim yasası "doğru
      geometri, yanlış karakter"i göremez): blok/Braille örnek sayfası ·
      Claude Code'un maskotu · Claude Code'un spinner'ı — kullanıcı
      onayladı (2026-09-21)
- [x] Yayın etkisi yazıldı
