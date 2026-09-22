# Emoji ve geniş glyph

## Hedef

İki hücre genişliğinde ilan edilmiş karakter iki hücreyi boyasın (CJK,
fullwidth formlar) ve renkli emoji renkli çizilsin — atlasın tek kanallı
maske sözleşmesi, `GlyphInstance`'ın 32 baytlık stride'ı ve shader'ın "her
glyph tam bir hücre boyunda" cümlesi **bozulmadan**.

## Gereksinimler

- **R1** — Geniş karakter iki yuvadan çizilir.
  - **R1.1** — Yuva anahtarı bir `Half` eksenini kazanır (`Whole`/`Left`/
    `Right`), `SizeClass`'ın kardeşi olarak: `Face`'e dik, `Atlas::slot`'un
    anahtarında normalize. `Sprite`'a **varyant eklenmez**.
  - **R1.2** — Sağ yarı, glyph'i `-cell_px.0` **tam sayı** piksel ofsetiyle
    tek yuvalık tampona çizdirip CG'ye kırptırmakla elde edilir. `slot_bytes`,
    `upload_slot` ve `raster::draw`'un `assert_eq!` ön koşulu değişmez.
  - **R1.3** — İki yuva **atomik** ayrılır ya da birlikte reddedilir: kapasite
    sınırı iki yarının arasına düşerse yarım glyph + yarım kutu doğar.
- **R2** — Sütun sayısının tek sahibi `bt-core`.
  - **R2.1** — `bt_core::Cell` `wide: bool` kazanır ve bayrak **yalnız baş
    hücrede** kurulur (`Flags::WIDE_CHAR`). 68 → 72 bayt.
  - **R2.2** — Bayrak **iki sink'te** birden kurulur: ızgara (`session.rs`
    atlama kapısı) ve doldurma bandı. İkisinin aynı cevabı vermesi yazılı bir
    şart.
  - **R2.3** — `LEADING_WIDE_CHAR_SPACER` sağ yarı **almaz**.
  - **R2.4** — `dock::render` bayrağı **hiç kurmaz**; adıyla yazılmış değişmez.
  - **R2.5** — `bt-atlas` kutu genişliğini **argüman olarak** alır
    (`box_advance`); `unicode-width` `bt-atlas`'a bağımlılık olarak **girmez**.
- **R3** — Kapı sırası: geniş hücrede **önce** tek hücrelik mürekkep kapısı;
  geçerse bugünkü tek yuvalı yol, geçmezse iki hücrelik kapı, o da geçmezse
  kutu. Ölçüt `min(sütun, mürekkep)`.
  - **R3.1** — `ink_fits_cell` ile `centre_shift` **aynı** kutu genişliğini
    görür ("tek formül, iki tüketici").
  - **R3.2** — Tek hücreye sığan geniş ilan edilmiş karakterlerin rasteri
    **bit bit aynı** kalır ve ikinci yuva harcamaz.
- **R4** — Yelpazeleme `AtlasTexture::prepare`'de, `Frame::push`'ta **değil**.
  - **R4.1** — `GlyphCell` `wide: bool` taşır (`half` **değil**): sınırın
    `Cell.wide`'ıyla birebir ve sink hiçbir şey hatırlamıyor.
  - **R4.2** — "Bir yuva mı iki mi" kararının tek sahibi `Atlas::slot`, çünkü
    kapı sırası (R3) orada koşuyor; `push` atlası ödünç alamıyor
    (`GlyphCell`'in uv'siz olmasının gerekçesi: sink'te çözüm atlas ödüncünü
    `draw` boyunca canlı tutar ve ilk glyph'li karede `BorrowMutError` verir).
    `prepare` atlası **zaten** ödünç alıyor ve `metrics`'i var, yani ikinci
    instance'ı (`pos + cell_px.0`) üretecek tek yer orası.
  - **R4.3** — `slot` hangi yarıyı kullandığını **söyler**: tek yuvalık
    (`Whole`) bir cevapta `prepare` tek instance basar, iki yuvalık cevapta
    ikinci instance'ı da basar. Şeffaf bir "boş yuva" nöbetçisi
    **doğmuyor** — o hem bir draw israfı hem de tofu'nun yanında ikinci bir
    rezident yuva olurdu.
  - **R4.4** — Yelpazeleme **üç yüzeyde** birden iner: `prepare` kare başına
    dört kez koşuyor (şeritler, ızgara, doldurma, dock) ve dördü de aynı
    gövdeden geçiyor, yani tek yer üç yüzeyi birden kazanıyor.
- **R5** — Renkli emoji `Atlas`'ın **içinde** ikinci bir düzlemde yaşar.
  - **R5.1** — Format `RGBA8Unorm_sRGB`; düz `RGBA8Unorm` paleti sessizce açar.
  - **R5.2** — Yuvalar yine **hücre boyunda**, yani `Half` mekanizması geniş
    emojinin geometrisini de çözer.
  - **R5.3** — Düzlem kendi **monoton** `next`'ini tutar; kare ortasında
    anlamı değişen paylaşımlı atlas durumu yasak (uv `prepare` anında pişiyor).
  - **R5.4** — İkinci bir `Atlas` **açılmaz**: beş CoreText türetmesi ve ikinci
    bir `Metrics` doğar.
- **R6** — Emoji `cell_vertex`'i **aynen** paylaşan kardeş bir fragment'le
  çizilir; `GlyphInstance` stride 32 ve `cell_px`/`uv_size` uniform'ları
  değişmez.
  - **R6.1** — Blend'de değişen tek çarpan RGB kaynağı (`One`, ön çarpımlı
    emoji için); alfa tarafı zaten doğru.
  - **R6.2** — Çizim sırası: arka planlar → caret → **emoji** → glyph + kural.
    Ekleme **üç yüzeyde** ayrı ayrı kazanılır.
- **R7** — Atlas doluluğunun yeni şekli görünür.
  - **R7.1** — Bekçi **sınırı** sınar, doluluğu değil: atlasta tam **bir** boş
    yuva varken geniş bir karakter istenince **iki yarı da** tofu döner, `next`
    kıpırdamaz ve negatif önbellek çifti tutar (sonraki kare yeniden
    denemez). "Yarım glyph + yarım kutu" R1.3'ün atomikliği yüzünden
    **yapısal olarak** doğmuyor, yani onu arayan bir sınama boşa yeşil
    kalırdı; sınanacak olan tam bu köşe.
  - **R7.2** — Duman jetonuna `yuva2=U/T` eklenir; `yuva=`'yi aynalar.
  - **R7.3** — Kapasite sınırı adıyla ve **formülle** yazılır, sabitle değil:
    karakterlere açık yuva `capacity() - RULE_RESERVE - 1` ve mürekkebi iki
    hücre isteyen her karakter **ikisini** harcıyor, tek hücreye sığan geniş
    ilan edilmişler (R3) **birini**. Yani tavan hangi karakterlerin göründüğüne
    bağlı ve tek bir sayı olarak yazılamaz ("ölçülmemiş sayı yazılmaz"). Tahliye
    yok: tavanı aşan oturum `ensure()` atlası yeniden kurana kadar kutu çizer.
- **R8** — sRGB ve ön çarpımın tanığı **sentetik** ara tonlu bir RGBA yuvası
  olur, gerçek emoji değil: emoji bitmap'i CoreGraphics'ten geliyor ve macOS
  sürümleri arasında bit bit sabit değil.
- **R9** — Yazılı sözleşme kodla aynı sette gerçeğe uyar: `CLAUDE.md`'nin
  pipeline sayısı / doku formatı / "emoji henüz yok" cümleleri,
  `renderer.rs`'in blend yorumu, `docs/OLCUMLER.md`'nin envanter kaydı ve
  `docs/YOL-HARITASI.md`'nin dört borç kalemi.

## Yaklaşım

1. **`bt-core`** — `Cell`'e `wide: bool`; iki sink'te `Flags::WIDE_CHAR`'dan
   kurulur, `LEADING_WIDE_CHAR_SPACER`'da kurulmaz, `dock::render`'da hiç
   kurulmaz.
2. **`bt-atlas`** — yuva anahtarına `Half`; `raster::draw` bir x-ofset
   argümanı alır; `ink_fits_cell` ile `centre_shift` `box_advance` görür; kapı
   sırası (tek hücre önce) `fallback_font`'un çağrı yerine iner; iki yuva
   atomik ayrılır.
3. **`bt-gpu`** — `GlyphCell` `wide: bool` kazanır; `prepare` yuvayı
   `(Sprite, Face, SizeClass, Half)` ile sorar ve `slot`'un cevabına göre bir
   ya da iki instance basar. `Frame::push` yelpazelemiyor.
4. **`bt-atlas` ikinci düzlem** — `RGBA8Unorm_sRGB`, hücre boyunda yuvalar,
   kendi monoton `next`'i, format-duyarlı `slot_bytes`, ikinci bir CG reçetesi
   (ön çarpımlı RGBA + renk uzayı).
5. **`bt-gpu` kardeş fragment** — `cell_vertex` paylaşılır, fragment ayrı,
   blend'de RGB kaynağı `One`; üç yüzeyde birer draw.
6. **Bekçiler ve jeton** — dolu atlas geniş karakterle sınanır, `yuva2=`
   eklenir, sentetik ara tonlu piksel tanığı kurulur.
7. **Belgeler** — `CLAUDE.md`'nin üç cümlesi, `renderer.rs`'in blend yorumu,
   `docs/OLCUMLER.md` → `## Atlas yuva ayak izi` envanter kaydı,
   `docs/YOL-HARITASI.md`'nin borç kalemleri.

## Kapsam Dışı

- **Grapheme dizileri** — ZWJ (`👨‍👩‍👧`), ten rengi değiştiricileri,
  VS15/VS16. Anahtar `char` değil `&str` olmak zorunda; ayrı set.
- **78 tek sütunlu emoji** (`🌡 🎙 🏋 🏔`) — kutu kalıyor. Çare küçültme ve o,
  yol haritasının 190 karakterlik kalemi.
- **Aynanın `CURSOR`'u** — karakter indeksi, sütun değil. Bu setten önce de
  var, bu set kötüleştirmiyor; dock'un kendi seti.
- **Legacy computing (U+1FB00–1FBFF)** ve `.LastResort`'un 825'i — fontun
  yokluğu, 021'in yordamsal ailesinin devamı.
- **Atlas tahliyesi / geri dönüşüm** — 022 onu adıyla erteledi; bu set sınırı
  **görünür** kılıyor, kaldırmıyor.
- **`replaceRegion`'ın uçuşta yazma sınırı** — `prepare`'in doc'unda yazılı
  bilinen sınır; bu setle büyüyor (kare başına iki yuva) ve çaresi (staging +
  blit encoder) ayrı bir iş.

## Akış

```
bt-core            bt-atlas                      bt-gpu
Cell.wide ────────────────────────────────────▶ Frame::push
(WIDE_CHAR,                                      │ (yelpazelemez)
 iki sink,                                       ▼
 dock'ta asla)                            GlyphCell{wide}
                                                 │
                   Atlas::slot ◀───── prepare ───┘
                   (Sprite, Face, SizeClass, Half)
                     │  cevap: Whole → 1 instance
                     │          Left+Right → 2 instance
                     │
                     ├─ maske düzlemi  (R8Unorm)      ─▶ cell_fragment
                     └─ renk düzlemi   (RGBA8_sRGB)    ─▶ kardeş fragment
                                                          (cell_vertex ortak)
kapı sırası: ink(cell) → tek yuva | ink(2·cell) → iki yuva | kutu
```

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| phase-2 | ✅ |
| phase-3 | ✅ |
| kapı | |
