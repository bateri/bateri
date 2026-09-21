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
- **Ölçüm bekliyor:** kutu ailesinin yuva ayak izi (`yuva=U/T`,
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

- [ ] `raster::is_procedural` (iki aralık) + `draw_procedural`
      (`draw_rule`'un iki açılış satırı önce)
- [ ] Dikdörtgen ve birleşim primitifleri, mevcut `coverage`/`max`
      üstünden
- [ ] Blok elemanları: `█`, sekizde birlik merdivenler, çeyrekler, üç gölge
- [ ] Braille: alt 8 bitten nokta maskesi, **tablo yok**
- [ ] Tasarım sabitleri adıyla ve gerekçeli (dama adımı, Braille nokta
      geometrisi)
- [ ] `Atlas::slot`: normalizasyon kolu (deseni `Normal`, `_` değil) +
      guard'lı çizim kolu
- [ ] Test: `█` bit bit 255
- [ ] Test: Braille bit-max değişmezi (`0x2800` boş, maskelerin birleşimi,
      sekiz noktanın desteği ayrık)
- [ ] Test: sekizde bir merdiveni monoton iç içe; `▀` + `▄` doygun toplamı
      255
- [ ] Test: kalın yüz ile düz yüz **aynı yuvayı** paylaşıyor
- [ ] Test: küçük sınıfta kapı kapalı (fonttan geliyor)
- [ ] Değişmezler **en az üç (punto, ölçek) çiftinde** koşuyor
- [ ] `the_gate_decides_by_width_alone`'un `continue` kapısı genişletildi
      (`⠋`), `wide > 0` bekçisi ayakta
- [ ] `face_fallback_is_cached_under_the_requested_face`'e yeni fikstür
      **ölçülerek** bulundu
- [ ] Belgeler (R10'un bu phase'e düşen yarısı)
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] `make duman` — **regresyon nöbetçisi, kanıt değil**: duman betiği
      donmuş (`session.rs`: "ikinci bir yük buraya eklenmez") ve
      `hucre=8 glif=6 kural=15` jetonları bu setin karakterlerine hiç
      dokunmuyor; birebir aynı kalmalı
- [ ] **Gözle kontrol** (gerçek doğrulama; birleşim yasası "doğru
      geometri, yanlış karakter"i göremez): blok/Braille örnek sayfası ·
      Claude Code'un maskotu · Claude Code'un spinner'ı
- [ ] Yayın etkisi yazıldı
