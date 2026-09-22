# Phase 1 — Dock sütun biriktirir

## Özet

`dock::render` karakter indeksini sütun sanmayı bırakıyor: genişliği
biriktiriyor, geniş karakterin baş hücresine `wide` koyuyor ve spacer
sütununa zemin veriyor.

_Requirements: R1, R1.1, R1.2, R2, R2.1, R2.2, R2.3, R2.4, R3, R3.1, R3.2, R5_

## Değişiklikler

- **`Cargo.toml` (workspace) + `crates/bt-core/Cargo.toml`** —
  `unicode-width` bağımlılığı. Sürüm **kilitteki** olmalı (alacritty'nin
  kullandığı), yani `Cargo.lock` yalnız `bt-core`'un kenarını kazanıyor ve
  hiçbir sürüm oynamıyor. Oynuyorsa dur: kararın kaydı `discussion.md` →
  K1'de ve o "aynı crate, aynı sürüm" diyor.
- **`crates/bt-core/src/dock.rs`** — çizim döngüsünün birimi değişiyor.
  Bugün `col = TEXT_COL + (index - skip)`; artık sütun **birikiyor** ve
  karakter başına genişlik `unicode-width`'ten geliyor. Dört tüketici ayrı
  ayrı ele alınıyor:
  - **Hücrenin sütunu** biriken toplamdan.
  - **`style_at(state, index)` değişmiyor** ve bu şart: `region_highlight`'ın
    aralıkları ZLE'nin kendi birimi, yani karakter indeksi. Sütuna çevirmek
    aralıkları yanlışlardı.
  - **`skip`** sütun penceresine çevriliyor. Bugün `(cursor + 1) - available`
    ve karakter sayıyor; caret'in sütunu artık indeksinden büyük olabildiği
    için pencere de sütun cinsinden hesaplanmak zorunda.
  - **Caret'in sütunu** imleçten önceki karakterlerin genişlik toplamı.
  Kenar kuralı: pencerenin iki yakasında da sığmayan geniş karakter **hiç
  çizilmiyor** ve o sütun boş kalıyor — yarım glyph sessiz bir bozulma,
  boşluk görünür bir eksiklik (`discussion.md` → K2).
  Baş hücre `wide: true` taşıyor ve **spacer sütununa bir zemin hücresi**
  düşüyor: glyph'i yok, yalnız arka planı ve kuralları var. Izgaranın
  `WIDE_CHAR_SPACER` kolunun aynısı ve gerekçesi `frame()`'de yazılı
  ("hücreyi tümden elemek onun sağ yarısını renksiz bırakırdı"). Bu olmadan
  `region_highlight`'ın sarı zemini emojinin sağ yarısında biterdi.
  **Bağlam satırı (`DOCK_CONTEXT_ROW`) bu yolun dışında**: küçük sınıf,
  sütun adımı küçük yüzün ilerlemesi ve geniş yol orada kapalı kalıyor
  (021'in emsali, gerekçe ölçü ayrışması).

## Kabul

- Dock'un giriş satırında `🎉` **iki hücre** ve `Cell::wide` taşıyor; spacer
  sütununda glyph yok ama zemin var.
- Caret CJK'lı bir satırda doğru sütunda: `漢字` yazıp sola gitmek caret'i
  glyph'in başına koyuyor, bir sütun kaymıyor.
- `region_highlight` geniş karakteri **iki hücre** boyuyor.
- Pencere kenarında yarım glyph yok: `skip` bir geniş karakterin ortasına
  düşerse o karakter çizilmiyor.
- 023'ün `the_dock_never_marks_a_cell_wide` bekçisi **tersine çevrildi**:
  kutu silinmedi, iddiası değişti ve dock artık geniş hücreyi işaretliyor.
- `make hepsi` yeşil, `make duman` yeşil.
- `Cargo.lock` yalnız `bt-core`'un `unicode-width` kenarını kazandı.

## Uygulama Notları

- **Caret'in sütunu döngüden önce, ayrı bir önekle hesaplanıyor.** `skip`
  caret'in sütununa bağlı ve çizim döngüsü `skip`'e bağlı, yani ikisi aynı
  turda çözülemiyordu. Önek yalnız imlece kadar geziliyor — eski yorumun
  "tam dizgiyi ikinci kez gezmeyelim" gerekçesi korunuyor.
- **Sol kenar kuralı tek koşula indi ve ilk yazımı ölü bir disjunct
  taşıyordu.** `col_acc + width <= skip || col_acc < skip` yazmıştım ve bu
  notta "ikisini birden ifade ediyor" demiştim — **yanlıştı**: `width >= 1`
  olduğu için ilk şart ikinciyi zaten içeriyor, yani hiçbir zaman kararı
  veren o değil. Set kapısı (`/code-review`) hem kodu hem bu notu yakaladı;
  koşul `col_acc < skip`'e indi.
- **Sıfır genişlikli kod noktası çizim döngüsünden de düştü.** Plan bunu
  yalnız tazelik kapısı (phase-2) için yazmıştı, ama çizim tarafında da
  gerekiyordu: `width == 0` bir hücre alsaydı önceki karakterin sütununa
  ikinci bir hücre düşer ve glyph'ini örterdi. Bekçisi
  `a_zero_width_codepoint_gets_no_cell`.
- **`cell()` imzası `wide` aldı.** Alternatifi dönüşü çağrı yerinde
  değiştirmekti; imzaya almak spacer'ı `..lead` ile türetmeyi mümkün kıldı,
  yani zemin ve kurallar tek yerden geliyor.
- **Kenar sınamasının ilk yazımı yanlıştı** ve kod doğruydu: `TEXT_COL + 3`
  bütçesiyle `a` + `漢` **sığıyor** (1 + 2 = 3). Sınır tam ikide, bütçe ona
  indirildi.
- **Kapı beş bulgu verdi, hepsi giderildi.** En sertinin adı yok ama şekli
  var: **kontrol karakteri sütununu kaybediyordu.** `column_width`
  `unwrap_or(0)` yazıyordu ve 024 öncesinde her indeks bir sütun olduğu için
  kontrol karakteri sütununu **tutuyordu** — sıfıra indirmek `Ctrl-V` ile
  eklenmiş bir TAB'ın iki yanındaki kelimeleri birleştirir ve caret'i kontrol
  karakteri başına bir sütun sola kaydırırdı. Ayrım ölçüldü: `width()`
  kontrol karakterinde `None`, birleştiricide `Some(0)` — yani `unwrap_or(1)`
  ikisini doğru ayırıyor ve sıfır yalnız birleştiriciye kalıyor. Bekçisi
  `a_control_char_keeps_its_column`. **Yan kazanç bir kısıtın kalkması:**
  `cell()`'in doc'u `^C` yer tutucusu çizmemenin gerekçesini "sütun
  aritmetiğini karakter biriminden çıkarır" diye yazmıştı ve o kısıt bu setle
  konusuz kaldı — `column_width`'in doc'unda yazılı.
  Kalan dört bulgu iki bayat yorum (`frame.rs`'in "sınır her zaman `false`
  veriyor" değişmezi ve `render`'ın "pencereleme karakter biriminde" doc'u —
  ikisi de artık yanlış ve `CLAUDE.md` aynı commit'te düzeltilmesini
  istiyor), yukarıdaki ölü disjunct ve bağımlılık gerekçesinin phase sırasını
  söylemesi.
  **`frame.rs`'in yorumu kendi driftini önceden yazmıştı:** "alan hücreden
  okunuyor, sabit `false` yazılmıyor — sabit yazmak değişmezi iki yere
  kopyalar ve `bt-core` bir gün onu kaldırsa bu satır sessizce eski kalırdı."
  Kaldırdı; satır sessizce eski kalmadı çünkü sabit yazılmamıştı.
- **`make denetim` bağımlılık uyarısı verdi ve vermeli:** `Cargo.toml` ile
  `Cargo.lock` HEAD'den farklı. Kayıt iki yerde — `discussion.md` → Karar 1
  ve `Cargo.toml`'daki yorum. Lock farkı **tek satır** (`+ "unicode-width",`),
  yani hiçbir sürüm oynamadı.

## Checklist

- [x] `unicode-width` workspace + `bt-core` bağımlılığı; `Cargo.lock` farkı
      tek kenar (`+ "unicode-width",`)
- [x] Sütun biriktirme; `style_at` indeksle kalıyor
- [x] `skip` sütun penceresi
- [x] Caret'in sütunu genişlik toplamından
- [x] Kenarda yarılanma yok (iki yaka)
- [x] Baş hücre `wide`, spacer sütununa zemin hücresi
- [x] Bağlam satırı geniş yolun dışında (`render_context` bu yoldan geçmiyor)
- [x] Test: geniş karakter iki sütun + spacer glyph'siz → `a_wide_char_takes_two_columns_in_the_dock`
- [x] Test: CJK'lı satırda caret'in sütunu (aynı bekçinin son iddiası)
- [x] Test: vurgu iki hücreye yayılıyor → `a_highlight_covers_both_cells_of_a_wide_char`
- [x] Test: kenarda sığmayan geniş karakter çizilmiyor → `a_wide_char_is_never_split_at_the_window_edge`; ayrıca `a_zero_width_codepoint_gets_no_cell`
- [x] 023'ün dock bekçisi tersine çevrildi (`the_dock_never_marks_a_cell_wide` → `a_wide_char_takes_two_columns_in_the_dock`)
- [x] Test: kontrol karakteri sütununu tutuyor → `a_control_char_keeps_its_column`
- [x] Doğrulama geçti (`make hepsi` yeşil; `make duman` `hareket=27 icerik=3 sessiz=1755.01ms kapanis=clean`)
- [x] Riskli phase: `/code-review` koştu, beş bulgunun beşi giderildi (bkz. Uygulama Notları)
