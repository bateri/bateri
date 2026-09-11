# Yazı biçimleri

## Hedef

Terminal çıktısının biçimi ekrana ulaşır: kalın ve eğik yüzler gerçek font
yüzlerinden gelir, beş çeşit alt çizgi ve üstü çizili birer kural sprite'ı
olarak atlasa girer ve `cell` pipeline'ından çizilir. `git diff` başlıkları
kalın, `man` sayfaları altı çizili, nvim'in tanı çizgileri kıvrımlı ve
renkli görünür. 003'ün "yalnız düz metin" dönemi biter.

## Gereksinimler

- **R1** — `bt-atlas` dört font yüzü tanır: `Face { Regular, Bold, Italic,
  BoldItalic }`, düz yüzden `CTFontCreateCopyWithSymbolicTraits` ile türer.
  - **R1.1** — Denetim **iki kapılı**: dönüş `Option` (nil kapısı tipte) ve
    dönen fontun `symbolic_traits()`'i istenen maskeyi **gerçekten** taşımalı.
    Aile adı karşılaştırması **yapılmaz** — API'nin sözleşmesi zaten "aynı
    ailede font, yoksa NULL", yani karşılaştırma totolojidir. Asıl risk
    CoreText'in düz yüzü geri vermesi ve o sessiz ikame `font.rs`'in
    `CTFontCreateWithName` için yaşadığı hatanın aynısıdır.
  - **R1.2** — Bulunamayan yüz **düz yüze geri düşer**; uyarı atlas
    kurulurken **bir kez** basılır, kare başına değil.
  - **R1.3** — Metrik **yalnız düz yüzden**. Hücre genişliği düz yüzün boşluk
    advance'i olarak kalır; kalın glyph aynı yuvaya rasterize olur ve bir
    piksel kırpılabilir — her terminal böyle yapar, ızgara yüze göre oynayamaz.
    Bekçisi: kalın `M`'in yuvaya sığdığını doğrulayan sınama.
  - **R1.4** — Kök `Cargo.toml`'a `CTFontTraits` feature'ı eklenir
    (`copy_with_symbolic_traits` ve `CTFontSymbolicTraits` onun arkasında).
    Yeni **crate** değil; `Cargo.lock` oynarsa bilinçli karardır.
- **R2** — `bt-atlas` kural çizgilerini **yordamsal sprite** olarak rasterize
  eder: `RuleKind { Single, Double, Curl, Dotted, Dashed, Strike }`.
  - **R2.1** — Yuva anahtarı `HashMap<(Sprite, Face), u16>`,
    `Sprite { Char(char), Rule(RuleKind) }`. Kurallar yüzden bağımsız,
    `Face::Regular`'da yaşarlar.
  - **R2.2** — Raster `raster.rs`'in var olan alfa-only `CGBitmapContext`'ini
    kullanır; `tofu_tamponu` zaten fontsuz yordamsal çizim yapıyor, bu onun
    ikinci müşterisi. Yeni bağımlılık ve yeni mekanizma yok.
  - **R2.3** — `Metrics` kural zarfını taşır: `underline_px` (konum,
    kalınlık), `strikeout_px`, ve kıvrım genliği. **Üçü de `font.rs`'te,
    `cell_px`'i bilen tek yerde hücreye kırpılır** — `slot_bytes`'ın "yuva
    geometrisinin tek sahibi" gerekçesi buna birebir uyar. Kırpılmazsa küçük
    descent'li bir fontta çizgi komşu satıra taşar ve belirti sessizdir.
  - **R2.4** — Dalganın ve noktalı/kesikli desenin periyodu hücre genişliğini
    **tam bölmeli**; bölmezse çok hücreli bir alt çizgi hücre sınırında faz
    kırar ve kesintili görünür. Kendiliğinden gelmez, kurulurken sağlanır.
- **R3** — `bt-core` sınırı biçimi taşır. `Cell` kazandığı alanlar:
  `bold`, `italic`, `underline: UnderlineStyle`,
  `underline_color: Option<LinearRgba>`, `strikeout`.
  - **R3.1** — Alacritty `Flags`'i **yeniden ihraç edilmez** (003 R2.3 ve
    `CLAUDE.md`). `UnderlineStyle` `bt-core`'un kendi tipidir.
  - **R3.2** — **Eşleme tuzağı:** `Attr::Undercurl` `ALL_UNDERLINES`'ı
    silip yalnız `UNDERCURL` ekliyor, yani kıvrımlı metinde `Flags::UNDERLINE`
    **kapalıdır**. Beş bayrak **ayrı ayrı** ve kıvrımlı önce sorulur;
    `contains(UNDERLINE)` refleksi kıvrımlı çizgiyi tümden düşürür.
  - **R3.3** — Atlama koşulu genişler: `bg.is_none() && ch.is_none() &&
    underline == None && !strikeout`. Boşluk `ch: None` **kalır** — altı
    çizili boşluk bir kural ister, rasterize edilmiş boş bir yuva değil.
    003'ün `session.rs`'teki "tek dokunulacak satır `then_some`" notu aynı
    phase'de düzeltilir.
  - **R3.4** — `HIDDEN` kuralları da düşürür: `\e[8m` "mürekkep yok" demek,
    altı çizili gizli metin çizgiyi gösterirse gizleme delinir.
  - **R3.5** — SGR 58 rengi `cell.underline_color()`'dan gelir, `color::resolve`
    + `lineer_rgba` ile çözülür; `None` → ön plan. `bg: Option<LinearRgba>`
    ile birebir aynı örüntü.
- **R4** — `bt-gpu` iki katmanı birbirine çevirir ve kuralları çizer.
  - **R4.1** — `Face` çevirisi **burada**: `bt-atlas` `bt-core`'u görmüyor ve
    görmemeli (o kenar `alacritty_terminal`'i saf-CoreText crate'ine çekerdi);
    `bt-gpu` ikisini birden gören tek katman. `(bold, italic)` → `Face` dört
    kollu bir `match`. **İki tip bilerek ayrıdır** — `bt_core`'unki SGR
    semantiği, `bt_atlas`'ınki font yüzü; sonradan "aynı görünüyorlar" diye
    birleştirilirse katman yönü ters döner.
  - **R4.2** — Kurallar `Frame`'de ayrı liste (`RuleCell { pos, kind, rgba }`),
    `GlyphInstance`'a **encode'da** çevrilir ve glyph'lerden **sonra**, aynı
    `cell` pipeline'ında, tek draw call'da çizilir. `GlyphCell`'in örüntüsünün
    aynısı: `Frame` mantıksal veriyi tutar, instance atlas ödüncünün yaşadığı
    yerde kurulur.
  - **R4.3** — `bg_count` **el değmez** → `hucre=K` bit bit korunur ve
    `push`'taki `debug_assert_eq!(bg.len(), bg_count)` bekçisi olduğu gibi
    kalır. Kural sayacı ayrıdır.
  - **R4.4** — `.metal` dosyalarına **dokunulmaz**: `cell` pipeline'ı zaten
    kapsama maskesi çiziyor ve renk instance'tan geliyor.
- **R5** — Duman sözleşmesi jeton **ekler**: `kare=N hucre=K glif=G kural=R
  pipeline=ok`, kapı `r > 0` de sorar.
  - **R5.1** — Reçete beş stili, üstü çiziliyi ve SGR 58'i sınar; eklenen
    hücrelerin hepsi `bg: None, ch: None`, yani `hucre=8` ve `glif=6` bit bit
    korunur ve hepsi R3.3'ün yeni yan tümcesinden geçer.
  - **R5.2** — `\033[4:3m` (iki nokta) undercurl, `\033[4;3m` (noktalı virgül)
    `Underline + Italic`. Reçete "sadeleştirilirse" sınama sessizce iner.
  - **R5.3** — Jeton, kuralları **çizen** phase ile aynı commit'te girer;
    erken girerse `R=0` olur ve set ortasında duman kırmızıya düşer.
  - **R5.4** — Kapı üçe ayrılır: duman sayacı (sınır kural üretti),
    `bt-core` sınaması (beş stil **ayırt ediliyor**), offscreen sınama
    (kural bandı x boyunca **tekdüze değil** — kıvrım gerçekten dalga).
- **R6** — Belgeler aynı commit'te: `CLAUDE.md`'nin *"kalanı 004'ün işi
  (…emoji, kutu çizim)"* cümlesi **ve** `crates/bt-atlas/src/lib.rs:10-11`
  modül doc'unun aynı iddiası; `CLAUDE.md` jeton listesi, `Makefile`'ın
  `duman` yorumu, `proje.md`'nin doğrulama satırı.

## Yaklaşım

1. **Phase-1 `bt-atlas`** — dört yüz, `symbolic_traits()` denetimi, kural
   sprite'ları, `Metrics` kural zarfı ve kırpma, `(Sprite, Face)` anahtarı,
   `CTFontTraits` feature'ı. Saf `cargo test -p bt-atlas`; ekranda hiçbir
   değişiklik, `make duman`'a sıfır risk. İki yarısı (yüzler ve sprite'lar)
   aynı anahtarı değiştirdiği için **bölünmez**: ayrı commit'ler anahtarı iki
   kez yazardı.
2. **Phase-2 `bt-core`** — `UnderlineStyle`, `Cell` alanları, R3.2'nin
   eşleme tuzağı, atlama koşulu, `HIDDEN`, SGR 58 çözümü, yeni duman reçetesi
   ve stil sınaması. **Ekranda hiçbir değişiklik olmaz** — `Frame::push` yeni
   alanları henüz okumuyor, eklenen hücreler `bg: None, ch: None` olduğu için
   `hucre=8 glif=6` bit bit aynı kalır. 003 phase-4'ün "sink'i renderer
   veriyi yoksayarak al" dikişinin aynısı.
   Bedeli: `Cell`'e alan eklemek `bt-gpu`'nun **altı** sınama literalini kırar
   → `Default` + `..Default::default()`. Yani bu phase `bt-gpu` test koduna
   dokunur; katman ihlali değil, envanter kalemi.
3. **Phase-3 `bt-gpu`** — dört kollu `Face` çevirisi, `GlyphCell` yüzü taşır,
   kural listesi ve encode sırası, `kural=` jetonu ve kapı, offscreen sınama,
   belgeler. Geri alması en pahalı phase; ekranda görünen değişiklik burada.

## Kapsam Dışı

Emoji ve renkli glyph, **geniş glyph** (003 `teslim.md` B.3'ün "geniş
karakter tek yuvaya kırpılıyor" sınırıyla aynı iş — ikisi tek sette),
kutu çizim karakterleri, sentetik kalın ve sentetik eğik, kalın→parlak renk
eşlemesi (`draw_bold_text_with_bright_colors`, varsayılanı kapalı),
üstü çizili için ayrı renk (SGR'de yok), ligatürler, RTL ve karmaşık
şekillendirme, LRU tahliye, `cell_rule` pipeline'ı (kesintisiz dalga
gerekirse geri gelir — bkz. `discussion.md` Karar 4).

**003'ten devredilen borç yeniden erteleniyor:** *"ölçeğin `bt-gpu`'ya iki
kapısı var (`Surface::set_size` ve `cell_metrics`)"*. Ölçek borusuyla ilgili,
yazı biçimiyle değil; bu seti genişletir. Sessizce düşmesin diye buraya
yazıldı.

## Göç

Ayar dosyası, tema ve terminfo değişmiyor — `docs/AYARLAR.md` ve ayar modeli
henüz yok, SGR 58 var olan `color::resolve` yolundan geçiyor, yeni kullanıcı
ayarı doğmuyor. `make duman` çıktısı jeton **ekler** (`kural=`), eskisini
korur. Hücre boyutu ve grid ölçüsü değişmiyor: metrik düz yüzden geliyor.

## Akış

```
alacritty Flags                bt-core              bt-gpu            bt-atlas
──────────────                 ───────              ──────            ────────
BOLD ─────────────►  bold: bool ──────────► match (bold,italic)
ITALIC ───────────►  italic: bool ────────►   └─► Face ──────────► slot(Sprite::Char(c), face)
UNDERCURL ────┐                                                         │
DOUBLE_ ──────┤                                                         ▼
DOTTED_ ──────┼───►  UnderlineStyle ───────► RuleCell ──────────► slot(Sprite::Rule(kind), Regular)
DASHED_ ──────┤      (kıvrımlı ÖNCE sorulur;                            │
UNDERLINE ────┘       UNDERCURL, UNDERLINE'ı içermez)                   ▼
STRIKEOUT ────────►  strikeout: bool                          atlas dokusu (R8Unorm)
underline_color() ►  Option<LinearRgba>                       kapsama maskesi
                                                                        │
                     çizim sırası (tek render pass)                     ▼
                     1. cell_bg: arka planlar + imleç          cell pipeline
                     2. cell:    glyph'ler                     (renk instance'tan)
                     3. cell:    kurallar (aynı pipeline)
```

## Durum

| Phase | Durum | Commit |
|-------|-------|--------|
| phase-1 | ✅ | 92607aa |
| phase-2 | ✅ | 643c8c6 |
| phase-3 | ✅ | c3d7359 |
