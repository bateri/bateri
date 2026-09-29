# Phase 2 — Küçültme

## Özet

Kapının iki kolundan da dönen aday, sınırın içindeyse küçük puntolu
kopyasıyla kabul edilir ve çizilir. Sınır phase-1'in dağılımından seçilir.

_Requirements: R2.1, R3, R3.1, R3.2, R3.3, R3.4_

## Değişiklikler

- **`crates/bt-atlas/src/font.rs`**
  - Sınır için adlı bir sabit tanımlanır. Doc'u türetmeyi taşır: phase-1'in
    dağılımı, emojinin üst ucu, `.LastResort`'un oranı.
  - `accept`'e üçüncü kol eklenir. İki kapı da reddettiyse kutu hücre olur
    (tek sütunluda) ya da iki hücre (geniş karakterde). Oran sınırın
    içindeyse adayın küçük puntolu kopyası kurulur
    (`CTFontCreateCopyWithAttributes`) ve `ink_fits_box` ile **yeniden**
    sınanır. Yuvarlama payı için geçmezse kutu. Sıra doc'ta gerekçelidir:
    küçültme en son kol, yani bugün geçen aday bit bit aynı (R3.3).
  - `.LastResort` bu kolda kabul edilmez (R3.2). Ölçüt phase-1'in bulgusuna
    göre seçilir. Oran sınırın üstündeyse geometri zaten yetiyor ve ek
    ölçüt yazılmaz. Altındaysa ölçüt ve gerekçesi Uygulama Notları'na
    yazılır; `CLAUDE.md`'deki "sihirli dizge yok" cümlesi de aynı commit'te
    güncellenir.
  - Dikey yerleşim, ~0.6'lık emoji için gözle seçilir (discussion.md →
    Karar 5). Ortalamaya karar verilirse formül `centre_shift` gibi tek
    yerde durur ve kapı ile çizim onu paylaşır.
- **Renk düzlemi**: küçük kopya aynı fonttan geldiği için trait biti
  (`has_color_glyphs`) değişmez. `draw_color_glyph` yolu dokunulmadan
  kalmalı; kalmıyorsa sebebi yazılır.
- **Bekçi**: `EXPECTED_TOFU` boşalır. Sınırın dışında kalan karakter varsa
  listede kalır ve nedeni yanına yazılır.
- **Sınamalar**: küçültülen glyph'in rasteri hücrenin içinde kalır (sol ve
  sağ sütunda mürekkep yok ya da sınırda). Sınırın hemen üstündeki aday
  kutu kalır. Bugün kabul edilen bir adayın (ör. `⏺`) rasteri değişmez.
- **`CLAUDE.md`**: "kutu ya da tam glyph" paragrafı "kutu, tam glyph ya da
  sığacak kadar küçültülmüş glyph" olur. Tek sütunlu emoji için yazılmış
  "kapsam dışı" cümlesi kalkar. Sınırın adı ve işaretçi eklenir.
- **`docs/YOL-HARITASI.md`**: 041 satırı.

## Kabul

- `⧉` ızgarada, dock'ta ve doldurma bandında hücreye sığan, kırpılmamış bir
  glyph olarak çizilir.
- Tek sütunlu renkli emoji (ör. `☺`) küçük ve renkli çizilir.
- `make tarama`'da `Rejected` grubunda yalnız sınırın üstündekiler kalır.
- Bekçi yeşil, `EXPECTED_TOFU` boş ya da gerekçeli.

## Checklist

- [x] Sınır sabiti, türetmesi doc'ta
- [x] `accept`'in üçüncü kolu + yeniden sınama
- [x] `.LastResort` ölçütü (gerekirse)
- [x] Dikey yerleşim kararı, gözle
- [x] `EXPECTED_TOFU` boşaldı — `⧉` çıktı; `.LastResort`'un iki PUA karakteri gerekçeyle kaldı (R3.2)
- [x] Test: küçültülen raster hücrede, sınır üstü kutu, bugünkü aday bit bit aynı
- [x] `CLAUDE.md` ve `docs/YOL-HARITASI.md` güncellendi
- [x] Küçültme katsayısı `fit`'ten (phase-1 → Uygulama Notları): `ratio` ile küçültülen `⧉` (1.11) sola yapışma yüzünden yeniden sınamada yine dönüyor, `fit` 1.22
- [x] `.LastResort` (`fit` 1.660) emojinin (1.661–1.681) altında: geometri ayıramıyor, R3.2'nin ölçütü gerekli
- [x] @1x'te Apple Color Emoji `fit` 2.124 ve iki hücreye de sığmıyor: sınırın @1x'i kapsayıp kapsamadığı kararı — kapsıyor (orkestratör kararı)
- [x] `CLAUDE.md` → Komutlar'a `make tarama` satırı
- [~] Doğrulama geçti (`make hepsi`) — fmt, denetim, clippy -D warnings, `bt-atlas` 86, `bt-core` 602, `bt-shell` 268, `bateri` 4 yeşil; düşen tek sınama 040'ın commit'lenmemiş `wgpu_renderer::tests::wgpu_matches_the_metal_oracle_on_every_scene`'i (`wgpu_renderer.rs:921`, phase-1'deki aynı satır). Kullanıcı onayı 2026-09-29. `make duman` yeşil.

## Uygulama Notları

- **Kol** (`font::accept` → `shrink`): iki kapıdan da dönen adayın `fit`'i
  (`font::fit_ratio`, census'tan `font.rs`'e taşındı, tarama ile kapı aynı
  fonksiyonu okuyor) `SHRINK_LIMIT`'in içindeyse ve aday `.LastResort`
  değilse `CTFontCreateCopyWithAttributes(size / fit)` ile kopya kuruluyor ve
  `ink_fits_box` ile yeniden sınanıyor. Kutu ızgaranın ayırdığı alan: tek
  sütunluda bir, iki sütunluda iki hücre (daha dar kutu daha çok küçültme
  ister, ikinci deneme yok). `Accepted.shrunk` R3.3'ün tanığı.
- **Doğrusal olmayan font: sapma.** `size / fit`'lik kopya Apple Color
  Emoji'de **hiç** geçmedi: emojinin ilerlemesi tam sayıya yuvarlı ve küçük
  puntoda orantısından geniş (16pt @2x'te 1.661 ile küçültülen kopya 23 px
  ilerliyor, hücre 19.27; mürekkep de orantısından büyük). İlk koşuda
  @2x'te 1239 emojinin tamamı yeniden sınamadan döndü. Pay uydurulmadı:
  ilk kopya geçmezse `size / fit` ile yarısı arasında kopyanın **kendi**
  ölçüsüyle ikiye bölme (`SHRINK_STEPS` = 10) sığan en büyük puntoyu
  buluyor. Emojinin gerçek küçültmesi her birleşimde ~2.12'ye yakınsıyor.
  Sonuç: dört birleşimde sınırın içinde olup yeniden sınamadan dönen **0**.
- **Sınır `SHRINK_LIMIT` = 2.2** (`make tarama`, bu makine, dört birleşim
  aynı): içeride kalması gereken en büyük tek sütunlu emojinin @1x `fit`'i
  2.124; dışarıdaki en küçük `🝇` (Apple Symbols) 2.250, sonra `⬷` 2.307,
  `⬳` 2.583 (STIX Two Math), `U+F8E5` 2.803 (Symbol), `🡐🡒` 4.22/4.26
  (Inter Display, kullanıcı fontu). Tarama sonrası: küçültülen 1810,
  `Rejected` = 7189 `.LastResort` + yalnız bu 6. İki sütunlu emoji @1x'te
  iki hücrelik kutuda `fit` 1.062 → iki hücreye küçülüyor; @2x'te zaten
  sığıyor.
- **`.LastResort` ölçütü: PostScript adı** (`font::is_last_resort`,
  `"LastResort"`). Yapısal sinyal bulunmadı: cascade her zaman bir font
  veriyor ve `.LastResort`'un cmap'i karakteri kapsıyor (taramada "hiçbir
  fontta yok" 0), `fit`'i (1.660) emojinin altında. Kapsamı yalnız küçültme
  kolu; iki kapı onu bugünkü gibi geometriyle eliyor. `CLAUDE.md`'nin "aile
  adı karşılaştırması, trait biti ve sihirli dizge yok" cümlesi "tek ad
  küçültme kolunun `.LastResort`'u" olarak güncellendi. Taramanın rapor
  etiketi de aynı ölçütten (`font::LAST_RESORT`), ikinci bir dizge yok.
- **Dikey yerleşim: hücrede ortalama** (`font::Accepted::rise`, iki çizim
  reçetesi `raster::draw_glyph`/`draw_color_glyph` yeni `rise` argümanıyla
  okuyor). Taban, hücre ortası ve x-yüksekliğinin ortası 16pt @2x ve
  13pt @1x'te `A g ⧉ x 🌡 y 🗺 | 🕊 H` satırında yan yana çizilip bakıldı:
  tabanda kalan emoji alçakta, alt ucu `y`'nin kuyruğu hizasında; hücre
  ortasında metinle birlikte okunuyor, `⧉` iki hâlde de doğal. Kural yalnız
  küçültülen adaya (küçültülmemişte `rise` 0, R3.3), tam piksele yuvarlı.
  Kapı yatay olduğu için `rise`'ı ölçmüyor.
- **Mevcut sınamalar**: `UNKNOWN_CHAR` `漢`'ti ve küçültme onu tek hücreye
  sığdırdı; `\u{10FFFC}` oldu (on altıncı düzlem PUA, `.LastResort`: tek
  hücrede kutu, iki hücrede çift). Negatif önbellek havuzu CJK'dan aynı
  düzleme taşındı; `𝔸` artık çiziliyor; `the_gate_decides_by_ink_alone`
  beklentisini küçültme kolundan da türetiyor ve renk düzleminin 0. yuvasını
  çizim sayıyor. Bağlam satırının (tek sütunlu sorulan) CJK'sı artık kutu
  değil küçük glyph.
- **Yeni sınamalar** (`census::tests`): `shrunk_glyph_stays_inside_the_cell`
  (`⧉` maske, `🌡` renk, iki ölçek; üç hücrelik tuvalde ortadaki hücrenin
  dışında kapsama yok, renk trait'i korunuyor),
  `wide_emoji_shrinks_into_two_cells_at_1x`, `just_above_the_limit_stays_tofu`
  (`🝇`), `last_resort_is_not_shrunk`, `gate_accepted_candidates_are_unchanged`
  (taranan blokların kapıdan geçen her adayı: aynı font nesnesi,
  `shrunk = false`, `rise` 0; `⏺` rasteri 041 öncesi yolla bayt bayt).
- **Renk düzlemi** dokunulmadı: kopya aynı fontun başka puntosu, trait biti
  korunuyor (sınamada) ve `draw_color_glyph` yalnız `rise` aldı.
- **Set kapısı `/code-review` bulgusu (düzeltildi):** `Left` isteğinin `Whole`
  kısayolu "tek hücrelik kabul tam boyuyla sığıyor" varsayımına dayanıyordu;
  küçültülmüş bir `Whole` (ör. `cluster_as_base`'in geniş isteğinden önce tek
  başına görülen `✌`, ya da tek sütunlu sorulmuş `漢`) iki hücrelik isteğe
  küçük kopyasıyla cevap veriyordu — sonuç isteklerin sırasına bağlıydı.
  `Atlas::shrunk` kümesi küçültülmüş `Whole` anahtarlarını tutuyor, kısayol
  onları atlıyor, yüz merdiveni ve `cluster_as_base` takma adları biti
  taşıyor. Bekçi `a_shrunk_single_cell_does_not_answer_the_wide_request`
  (düzeltme geri alınınca kırmızı düştüğü görüldü). Aynı turda
  `shrunk_glyph_stays_inside_the_cell` 3×3 hücrelik tuvale genişledi:
  küçültülen glyph dikeyde de hücrenin içinde, `ink_fits_box`'un ve
  `CLAUDE.md`'nin "dikeyde taşan tek küme emoji" cümlesi buna göre.
