# Phase 1 — Geniş glyph iki hücreyi boyar

## Özet

İki sütun ilan edilmiş karakter iki yuvadan çizilir; doku `R8Unorm` kalıyor,
shader'a hiç dokunulmuyor.

_Requirements: R1, R1.1, R1.2, R1.3, R2, R2.1, R2.2, R2.3, R2.4, R2.5, R3,
R3.1, R3.2, R4, R4.1, R4.2, R4.3, R4.4, R7.1, R7.3_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Cell` `wide: bool` kazanır ve bayrak
  `Flags::WIDE_CHAR`'dan kurulur. Kurulum **iki** sink'te: ızgara sink'i ve
  doldurma bandı sink'i. Doldurma sink'inin yazılı şartı korunur — "atlama
  kapısı ızgaranınkiyle **aynı** olmak zorunda", yani bayrak da aynı cevabı
  vermek zorunda, yoksa bandın satırı ekrana çıktığındakinden farklı görünür.
  `LEADING_WIDE_CHAR_SPACER` bayrağı **almaz**: satır sonuna sığmayan geniş
  karakterin bıraktığı boşlukta sağ yarı kopmuş bir glyph çizerdi. Sarılan
  karakterin baş hücresi bir sonraki satırın 0. sütununda ve yelpazeleme orada
  sorunsuz.
- **`crates/bt-core/src/dock.rs`** — bayrak **hiç kurulmaz** ve bu bir
  varsayım değil adıyla yazılmış değişmez: sütun karakter indeksinden
  türüyor (`TEXT_COL + offset`), spacer yok, yani iki hücrelik bir glyph
  komşu karakterin üstüne boyardı. 021'in "kapı küçük sınıfta kapalı" emsali
  **yetmez** — giriş satırı `SizeClass::Normal`.
- **`crates/bt-core/src/lib.rs`** — `Cell`'in boyut yorumu 68 → 72 bayt.
  `const` assert'li **grid** hücresi değil, `frame()` sınırının kaydı; ölçüt
  kare başına maliyet ve tipin tek tamponlanan dizisi doldurma bandının
  `Vec<Cell>`'i (kapasitesi korunuyor), kalanı satır içine alınmış
  değer-geçişli sink.
- **`crates/bt-atlas/src/font.rs`** — `ink_fits_cell` ve `centre_shift` kutu
  genişliğini **argüman olarak** alır (`box_advance`); `fallback_font` da öyle.
  `unicode-width` buraya **girmez**: hem yeni bir bağımlılık kararı olurdu hem
  ikinci bir genişlik yetkilisi doğururdu ve ızgaranın cevabıyla ayrışırdı.
  "Tek formül, iki tüketici" kuralı aynı sayıyı iki yere taşımakla korunur.
- **`crates/bt-atlas/src/raster.rs`** — `draw` bir x-ofset argümanı alır. Ofset
  **tam sayı** piksel (`-cell_px.0`), yani AA fazı iki çağrıda birebir aynı ve
  iki yarı 2w'lik bir rasterin bölünmüşüyle bit bit aynı çıkıyor. Bağlam,
  satır adımı ve `assert_eq!(target.len(), m.slot_bytes())` ön koşulu
  **değişmez** — taşan yarıyı CG kırpıyor.
- **`crates/bt-atlas/src/lib.rs`** — yuva anahtarı
  `(Sprite, Face, SizeClass, Half)`. `Half` `SizeClass`'ın kardeşi: `Face`'e
  dik bir eksen, `Atlas::slot`'un anahtarında normalize (`Sprite::Rule` ve
  küçük sınıf `Whole`'a iner). `Sprite`'a **varyant eklenmez** — doc'unun
  reddettiği şey renderer'a terminal semantiği sızdırmaktı ve bu eksen
  çağıran tarafından taşınıyor. Kapı sırası burada: önce tek hücrelik mürekkep
  kapısı, geçerse bugünkü tek yuvalı yol, geçmezse iki hücrelik kapı, o da
  geçmezse tofu. İki yuva **atomik** ayrılır ya da birlikte reddedilir.
  **`slot` hangi yarıyı kullandığını söylemek zorunda** ve bu, setin en ince
  yeri: kapı sırası burada koşuyor, yani "bir yuva mı iki mi" ancak burada
  biliniyor — oysa yelpazelemeyi yapacak taraf `bt-gpu`. Menlo'nun `☕`'si tam
  bu köşe: `Cell.wide` **kurulu** (ızgara ona iki sütun ayırıyor) ama tek
  hücrelik mürekkep kapısı geçiyor, yani tek yuva ve tek instance. `slot`
  cevabında `Whole`/`Left+Right` ayrımını taşımazsa çağıran ikinci bir instance
  basar ve `☕`'nin sağına boş bir dörtlü düşer. Şeffaf bir "boş yuva"
  nöbetçisi **açılmıyor**: tofu'nun yanında ikinci bir rezident yuva ve her
  karede bir israf draw'ı olurdu.
- **`crates/bt-gpu/src/frame.rs`** — `GlyphCell` `wide: bool` kazanır, `half`
  **değil**: sınırın `Cell.wide`'ıyla birebir. `Frame::push` **yelpazelemiyor**
  ve kayıtlar arası durum tutmuyor — tutsaydı `bt-gpu` "önceki hücre geniş
  miydi" diye terminal semantiği hatırlardı.
- **`crates/bt-gpu/src/renderer.rs`** — yelpazeleme **burada**, `prepare`'in
  glyph döngüsünde. Gerekçe ödünç: `push` atlası ödünç alamıyor (`GlyphCell`'in
  uv'siz olmasının yazılı sebebi — sink'te çözüm ödüncü `draw` boyunca canlı
  tutar ve ilk glyph'li karede `BorrowMutError` verir), `prepare` ise atlası
  **zaten** ödünç almış ve `metrics`'i elinde. `slot`'un cevabı `Whole` ise tek
  instance, `Left`+`Right` ise ikinci instance `pos + cell_px.0`'da.
  `instances.reserve` ipucu artık bir alt sınır. **Üç yüzey bedavaya geliyor**:
  `prepare` kare başına dört kez koşuyor (şeritler, ızgara, doldurma, dock) ve
  dördü aynı gövdeden geçiyor — 017'nin "bant ızgaradan türeyen her şeyi
  ayrıca kazanmak zorunda" dersi burada tek yerde ödeniyor.

## Kabul

- `echo 漢字` ve `echo １２３` iki hücre genişliğinde, ortalanmış çiziliyor;
  spacer hücresinde kopma yok.
- Tek hücreye sığan geniş ilan edilmiş karakterlerin (`☕ ⚡ ♈ 丨 、 ！`)
  rasteri **bit bit aynı** ve yuva sayısı değişmemiş — bir sınama bunu
  ölçüyor, çünkü "65 çalışan çizim oynamıyor" sözü ancak sınanırsa söz.
- **Sınır sınaması:** atlasta tam **bir** boş yuva varken geniş bir karakter
  istenince iki yarı da tofu dönüyor, `next` kıpırdamıyor ve negatif önbellek
  çifti tutuyor. "Yarım glyph + yarım kutu" atomiklik yüzünden yapısal olarak
  doğmuyor, yani onu arayan bir sınama boşa yeşil kalırdı.
- Tek hücreye sığan geniş ilan edilmiş karakter (`☕`) **tek** instance
  üretiyor: sağına boş dörtlü düşmüyor.
- Dock sink'i CJK taşıyan bir `BUFFER` için **sıfır** `wide` kaydı üretiyor.
- `make hepsi` yeşil; `make shader` **gerekmiyor** (shader'a dokunulmadı) ve
  bu phase'in kendi kanıtı.
- `make duman` yeşil.

## Uygulama Notları

- **`Upload` iki yarıyı birden taşıyor ve ikinci bir tampon doğdu.** Plan
  "atomik" diyordu ama biçimini söylemiyordu. Tek tamponla iki yarı aynı
  dönüşte verilemiyor (ikisinin baytları aynı anda canlı olmak zorunda), yani
  `Atlas`'a `buffer_right` eklendi — boyu tam bir yuva. Alternatifi sağ yarıyı
  ikinci bir `slot()` turuna bırakmaktı ve o tur kapasite sınırını ikisinin
  arasına sokardı.
- **Kapasite kapısı `want`'tan türüyor, kapıdan değil.** "Bir yuva mı iki mi"
  ancak çizim sırasında biliniyor, tahsis kararı ise ondan önce veriliyor.
  `Half::Left` bu yüzden **koşulsuz** iki yuva istiyor; tek hücreye sığan bir
  geniş karakter sınırın bir yuva berisinde reddedilebiliyor. Yanlışın yönü
  güvenli — ters yönde sol yarı açılır, sağ tofu'ya düşerdi.
- **`slot_uv` clippy'nin argüman sınırını aştı** (8/7) ve lint susturulmadı:
  dört argüman (`sprite`, `face`, `size`, `want`) gerçekten tek bir şeyi
  adlandırıyor — atlas anahtarının istek hâli — ve `SlotAsk` tipine indi.
- **Sınır sınamasının havuzu CJK ile kurulamadı.** İlk yazım atlası CJK
  karakterleriyle doldurmaya çalıştı ve havuz tükendi: `Half::Whole` ile
  sorulan CJK **sıfır** yuva harcıyor (kapıdan dönüp negatif önbelleğe
  giriyor). Havuz `full_atlas_returns_tofu_without_caching`'inkiyle aynı oldu
  — yordamsal aile + ASCII × dört yüz. Aynı ders 021'in Braille kalemiyle
  birebir aynı ve bu setin kapasite bulgusunun da kökü.
- **`LEADING_WIDE_CHAR_SPACER` yapısal olarak muaf.** Bayrak ölçütü tam olarak
  `Flags::WIDE_CHAR` ve o, öncü spacer hücresinde kurulu değil; ayrı bir dal
  gerekmedi.
- **`Cell`'in boyut yorumu `session.rs`'te**, planın dediği `lib.rs`'te değil:
  tip orada tanımlı. Ölçüldü ve yazıldı — 72 bayt, hiza 4.

## Checklist

- [x] `bt_core::Cell` `wide` alanı, iki sink'te kurulum, `dock`'ta değişmez
- [x] `LEADING_WIDE_CHAR_SPACER` sağ yarı almıyor (yapısal; bkz. Uygulama Notları)
- [x] `box_advance` argümanı: `ink_fits_cell`, `centre_shift`, `fallback_font`
- [x] `raster::draw` x-ofset argümanı, `slot_bytes` ön koşulu korunuyor
- [x] Yuva anahtarına `Half`, `Sprite` varyantsız, iki yuva atomik
- [x] Kapı sırası: tek hücre → iki hücre → tofu
- [x] `GlyphCell::wide`; `Frame::push` yelpazelemiyor
- [x] `slot` cevabı `Whole`/`Left+Right` ayrımını taşıyor
- [x] Yelpazeleme `prepare`'in glyph döngüsünde (dört çağrı, tek gövde)
- [x] Test: `echo 漢字` iki hücre → `a_wide_cell_becomes_two_quads` (glyph konumu + spacer)
- [x] Test: tek hücreye sığan geniş karakterin rasteri bit bit aynı → `a_wide_char_that_fits_one_cell_keeps_the_single_slot_raster`
- [x] Test: tek boş yuvada geniş karakter → `a_wide_char_is_rejected_whole_when_only_one_slot_is_left` → iki yarı tofu, `next` sabit
- [x] Test: `☕` tek instance üretiyor → `a_wide_cell_that_fits_one_cell_stays_one_quad` (sağına boş dörtlü yok)
- [x] Test: dock sink'i `wide` kaydı üretmiyor → `the_dock_never_marks_a_cell_wide`
- [x] Doğrulama geçti (`make hepsi` yeşil; `make duman` `kare=28 hucre=8 glif=6 kural=15 hareket=26 icerik=2 sessiz=1753.58ms kapanis=clean`)
