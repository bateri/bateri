# Kutu çizim

## Hedef

Kutu/blok çizim ve Braille karakterleri fonttan değil **terminalin
kendisinden** gelsin: hücreyi tam doldursunlar ve komşularıyla döşesinler.
Kullanıcının bildirdiği iki kusur — Claude Code'un maskotu (blok
elemanları) ve spinner'ı (Braille) — phase-1'de kapansın.

## Gereksinimler

- **R1** — Kapsamdaki karakter **yordamsal** çiziliyor; font hiç
  sorulmuyor ve yordamsal çizim fontu **koşulsuz** yeniyor.
- **R2** — Kapı `Atlas::slot`'un `Sprite::Char` kolunda, `Sprite::Rule`
  kolunun ikizi ve **yedekten önce**. `bt-gpu` ile `bt-core` değişmiyor.
  - **R2.1** — Üyelik yüklemi **tek yerde** yaşıyor: normalizasyon kolu ile
    çizim kolu aynı fonksiyonu çağırıyor. İki kopya sessizce kayar ve aynı
    bitmap dört yuva tutar.
- **R3** — Yüz `Face::Regular`'a normalize ediliyor; dört yüz tek yuvayı
  paylaşıyor.
  - **R3.1** — `face_fallback_is_cached_under_the_requested_face`'in
    fikstürü (`─`) **ölçülerek** değişiyor: Menlo Regular'da olan, Bold'da
    olmayan, kapsam dışı bir karakter.
- **R4** — Küçük sınıfta (`SizeClass::Small`) kapı **kapalı**: çizim
  kolunda `size == Normal` guard'ı, normalizasyon kolunun deseni de
  `SizeClass::Normal` (`_` değil).
- **R5** — Sprite hücreyi **tam** dolduruyor: `█` bit bit `255`, ve dikiş
  sürekliliği komşu hücrede kırılmıyor.
- **R6** — Kapsam: U+2580–U+259F (bloklar), U+2800–U+28FF (Braille),
  U+2500–U+257F (çizgiler, **köşegenler `╱╲╳` hariç**).
- **R7** — Tasarım sabitleri adıyla ve gerekçesiyle yazılıyor (kalın
  çarpanı, dama adımı, Braille nokta geometrisi); ölçüm iddiası taşımıyorlar.
  - **R7.1** — `dividing_period` **korunuyor**; kesikli yoğunlukların bazı
    puntolarda çökmesi kabul ve adıyla yazılı.
- **R8** — Doğrulama **değişmezle**, sprite başına el yazması sınamayla
  değil; oracle sınamaya **ikinci kez ve Unicode adlarından** yazılıyor;
  değişmezler en az üç (punto, ölçek) çiftinde koşuyor.
- **R9** — Kapının kırdığı iki mevcut sınama telafi ediliyor: R3.1'in
  fikstürü ve `the_gate_decides_by_width_alone`'un `⠋` probu.
- **R10** — Kodla çelişen cümleler aynı commit'te düzeliyor: `CLAUDE.md`'nin
  kutu çizim paragrafı, `bt-atlas` `lib.rs` başlık yorumu, `Atlas` struct
  doc'u, `RuleKind` doc'u ("yordamsal çizilen sprite'ların kümesi" artık
  tek küme değil) ve yol haritasının borç maddesi.

## Yaklaşım

1. **`raster::is_procedural(ch) -> bool`** — üyelik yüklemi, tek sahip.
   Kapsam aralıkları burada yazılı (R6).
2. **`raster::draw_procedural(ch, metrics, target)`** — `draw_rule`'un
   ikizi: `slot_bytes` assert'i, `fill(0)`, sonra çizim. Başarısız olamaz.
3. **Üç primitif**, üçü de mevcut yardımcılardan türüyor: dikdörtgen
   (`coverage`'ın iki eksende çarpımı), vuruş/yay (`chevron`'un mesafe
   alanı: `distance_to_segment` ya da `|hypot - r|`), birleşim (piksel-max,
   `band`'in `Double` kolundaki `max` gibi).
4. **`Atlas::slot`** — normalizasyon `match`'ine dördüncü kol (R3, R4),
   çizim `match`'ine `Sprite::Char` kolundan **önce** guard'lı kol (R2).
5. **Geometri tarifi karakterden türüyor:** Braille tablosuz (alt 8 bit =
   nokta maskesi), bloklar iki aritmetik koşu + bir avuç özel durum,
   çizgiler **gerçek tablo** (kol stillerinin permütasyonu, formül çıkmaz).
6. Belgeler (R10).

## Kapsam Dışı

- **Emoji ve geniş glyph** — setin ayrılma gerekçesi; renkli bitmap çatalı
  hiç açılmıyor.
- **Köşegenler** (`╱╲╳`, U+2571–U+2573).
- **Geometrik şekiller** (U+25A0–U+25FF), **Legacy Computing**
  (U+1FB00–U+1FBFF), **Powerline** (U+E0B0–).
- **Ayar anahtarı** — anahtar eklemek geri alınamaz; karşılığı R8'in
  yükselttiği doğrulama çıtası.
- **Yuva rezervi** — `RULE_RESERVE`'ün kutu ailesi için karşılığı yok;
  asimetri adıyla yazılıyor, çözülmüyor (tahliye 00X'in işi).

## Akış

```
bt-gpu: Sprite::Char(ch), face, size
   │
   ▼
Atlas::slot
   ├─ normalizasyon match
   │    (Char(ch), Normal) if is_procedural(ch) → (Regular, Normal)   R3
   │    (Char(_),  Small)                       → (Regular, Small)    R4: dokunulmuyor
   │    …bugünkü üç kol
   │
   ├─ önbellek / kapasite (değişmiyor)
   │
   └─ çizim match
        Char(ch) if size == Normal && is_procedural(ch)               R2
             → draw_procedural(ch, metrics, buf) → Drawn
        Char(ch) → bugünkü font yolu → NoGlyph ise fallback_font       değişmiyor
        Rule(kind) → draw_rule                                        değişmiyor

draw_procedural:
   ch → geometri tarifi
        Braille : alt 8 bit = nokta maskesi        (tablo yok)
        Bloklar : iki aritmetik koşu + özel durumlar
        Çizgiler: [Style; 4] tablosu               (permütasyon, formül yok)
   → primitifler: rect | stroke/arc | union(piksel-max)
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | |
| kapı | |
