# Phase 1 — `[font] letter_spacing`

## Özet

Atlas genişliği `Spacing`'in çarpanıyla türetiyor. Ayar, pencere ve belge
`line_height`'ın yolundan bağlanıyor. Kararlar `discussion.md` → Karar'da.

## Değişiklikler

- **`crates/bt-atlas/src/lib.rs`**
  - Yeni ihraç: `Spacing { line, letter }`. `Copy`, `PartialEq`, `Default`
    `1.0/1.0`.
  - `Key` dört alanlı: aile, punto, ölçek, `spacing`. `is` türetilmiş eşitlik.
  - `Atlas::new` ve `ensure` `Spacing` alıyor.
  - Büyük sınıfta `cell_advance = space_advance(regular) × letter`.
  - Küçük sınıfta `context_advance = space_advance(small) × letter`.
  - İki sınıf da `metrics_at(.., advance, line)` ile kuruluyor, yani
    `cell_px.0`, ortalama, `Half`'ın kutusu ve yordamsal sprite'lar aynı
    sayıyı görüyor.
  - Doğal ilerlemeler (çarpılmamış) yalnız yedek kapıya gitmek için alanda
    tutuluyor.
  - `MAX_EDGE`: kapasite köşesi kırmızıysa 8192, doc'u yeni köşeyle.
  - Eskiyen yorumlar yeni sözleşmeye çevriliyor: `484–488`, `928–936`,
    `ensure`'un "four values"'u.
  - Kapasite sınamasının döngüsü: `line_height` ekseninin yerine
    `Spacing` köşeleri `{(1,1), (MAX, MAX)}`.
- **`crates/bt-atlas/src/rules.rs`**
  - `accept` (ve onu saran `fallback_font`, `accept_font`) doğal ilerlemeyi
    de alıyor.
  - **İlk kapının kol kararı** doğal ilerlemeye bakıyor: "bugün tek hücreye
    sığan" ölçütü korunuyor.
  - Çizim kutusu, `centre_shift`, ikinci kol ve küçültme aralıklı ilerlemeye
    bakıyor.
  - Tek argümanlı `rules::metrics` siliniyor. Tek kalan sınama çağıranı
    (`freetype.rs`) `metrics_at`'e geçiyor.
- **`crates/bt-atlas/src/raster.rs`** — `position` yorumundaki "kaydırma
  sıfır" cümlesi koşullu hâle geliyor (`letter = 1`'de sıfır, açılınca
  ortalı). `every_base_glyph_advance_is_the_cell_advance`'ın yorumu da aynı
  biçimde düzeltiliyor.
- **`crates/bt-atlas/src/census.rs`, `crates/bt-atlas/tests/raster_digest.rs`**
  — Çağrılar `Spacing` ile. `raster_digest`'in özet satırları
  `Spacing::default()`'la değişmemeli.
- **`crates/bt-core/src/settings.rs`**
  - `FontOptions::letter_spacing`, varsayılan `1.0`. Doc'u `line_height`'ın
    yatay ikizi; alt ucun gerekçesi kırpılan geniş harf.
  - Yeni sabitler: `MAX_LETTER_SPACING`, `LETTER_SPACING_RANGE`
    (`1.0..=2.0`).
  - Okuma `ranged_float`'tan, tanı anahtarı `font.letter_spacing`.
  - `SettingsEdit::LetterSpacing` (`place`/`value`/round-trip kolları).
  - Şablona `line_height`'ın altına yorumlu satır. Şablonun anahtar
    listesine `("font", "letter_spacing")`.
  - Sınamalardaki üç `FontOptions` literal'i yeni alanla.
  - Sabitler `lib.rs`'in yeniden ihracına ekleniyor.
- **`crates/bt-gpu/src/renderer.rs`** — Çağrılara
  `Spacing { line: font.line_height, letter: font.letter_spacing }`
  geçiyor.
- **`crates/bt-shell-common/src/zoom.rs`**
  - `apply` → `FontOptions { size: self.size(font), ..font.clone() }`.
  - Yorum tek cümleye iniyor: yalnız punto geçici.
  - Sınamadaki literal yeni alanla.
- **`crates/bt-shell-macos/src/settings_window.rs`**
  - Line height'ın altına `Letter spacing` `Number` denetimi:
    `LETTER_SPACING_RANGE`, `decimal_label`, `SettingsEdit::LetterSpacing`.
  - `Key::LetterSpacing` → `font.letter_spacing`. `Key::ALL` dizisine
    `LineHeight`'ın hemen ardına giriyor. Tag'ler kayıyor, tag'i sayıyla
    okuyan yer kalmamalı.
  - `every_row_receives_its_own_diagnostic`'in metnine ve `match`'ine yeni
    satır.
- **`docs/AYARLAR.md`** — `[font]` örneği, tablo satırı ve maddeler: çarpan,
  harf hücrenin ortasında, neden `1`'in altına inilmiyor, üst sınır atlas
  bütçesi, kayıt anında uygulanma, Cmd +/− çarpanı taşıyor.
- **`docs/OLCUMLER.md`** — `MAX_EDGE` değiştiyse köşe satırına (`~969`)
  işaretçi. Sayıyı sınama hesaplıyor, belgeye sayı yazılmıyor.
- **`CLAUDE.md`**
  - `settings.toml` anahtar listesine `letter_spacing`.
  - "Her glyph'in ilerlemesi hücrenin ilerlemesi, kaydırma sıfır" cümlesi
    `letter_spacing = 1`'e bağlanıyor.
  - Yedek kapının kol kararı doğal ilerlemede (tek cümle + işaretçi).

## Kabul

- `bt-core`: `letter_spacing_is_read_and_bounded`.
  - Varsayılan `1.0`. `1.25` ve tam sayı `2` okunuyor.
  - `0.9`, `2.1`, `nan` ve metin reddediliyor, tanı anahtar adıyla.
  - Reddedilen değer öteki `[font]` anahtarlarını bozmuyor.
  - `SettingsEdit::LetterSpacing` round-trip'i yorumu ve bilinmeyen anahtarı
    koruyor.
- `bt-atlas`:
  - `letter_spacing_widens_the_cell_and_keeps_the_glyph_centred`.
    - `letter = 1.5`'te `cell_px.0 == round_up(space × 1.5)` ve yükseklik
      değişmiyor.
    - Taban fontun bir harfinin mürekkebi ortada: sol ve sağ boşluk ±1 px
      içinde eşit.
    - Küçük sınıfın `context_cell_w`'si aynı oranda açılıyor.
  - `wide_glyph_stays_split_when_letter_spacing_opens`. `letter = 2.0`'da
    `中` ve `😀` iki yuvaya bölünüyor (`Half::Left`/`Right`).
  - Yordamsal `─` geniş hücrede sol ve sağ kenar sütununu boyuyor (dikiş).
  - `capacity_clears_the_family_at_every_accepted_size` köşede yeşil.
  - `raster_digest`: üst commit ile ağacın çıktısı arasında fark yok.
- `bt-shell-macos`: `parse_decimal("0.9", LETTER_SPACING_RANGE) == None`.
- Gözle (`letter_spacing = 1.3`, kaydet):
  - Izgarada, dock'un giriş satırında, bağlam satırında ve doldurma bandında
    harfler açılıyor ve ortalı duruyor.
  - `tree`/`htop` çizgileri kopmuyor.
  - `echo 中文 😀` iki sütunun ortasında duruyor.
  - Caret ve seçim yeni hücreyi kaplıyor.
  - `1.0`'a dönünce görüntü eskisiyle aynı.

## Checklist

- [x] `bt-atlas`: `Spacing`, anahtar, iki sınıfın genişliği, yedek kapının
      doğal kolu, `rules::metrics`'in silinmesi, yorumlar
- [x] `bt-atlas`: kapasite köşesi, gerekirse `MAX_EDGE`
- [x] `bt-core`: ayar alanı, aralık, okuma, `SettingsEdit`, şablon, literal'ler
- [x] `bt-gpu`, `zoom`, ayar penceresi
- [x] `docs/AYARLAR.md`, `CLAUDE.md`, gerekirse `docs/OLCUMLER.md`
- [x] Test: yukarıdaki Kabul sınamaları ve `raster_digest` farkı
- [x] Doğrulama geçti: `make check`, `make linux` (`bt-core`, `bt-atlas`,
      `bt-gpu` ve `bt-shell-common` değişiyor), `make smoke`

## Uygulama Notları

- **İkinci kol tek sütunlu karakteri de kapsıyor.** `rules::accept`'te
  `cols >= 2` koşulu kalktı: tek sütunlu adayın kutusu tek aralıklı hücre.
  `letter_spacing = 1`'de bu kutu ilk kapının reddettiği doğal hücrenin ta
  kendisi (aynı cevap, raster bit bit aynı); açılınca geniş hücreye sığan
  glyph küçültülmeden tam boyuyla çiziliyor. Plan bu kolu küçültmeye
  bırakıyordu; orada `fit_ratio = 1` ile tam boyda ama `shrunk` işaretli
  (dikey ortalanmış) bir kopya doğardı.
- **Kapasite köşesi kırmızıydı → `MAX_EDGE = 8192`** (Karar'ın izinli
  sapması): 144pt@1x, `Spacing { 2, 2 }` 4096'da 276 yuva verdi (aile 429).
- **İki kapasite sınamasının havuzu genişledi.** `full_atlas_returns_tofu_*`
  ve `a_wide_char_is_rejected_whole_*` en küçük kapasiteli köşeye
  (`LARGEST_SPACING`) geçti ve 8192 tavanında havuz kapasiteyi aşmıyordu;
  ASCII'ye Latin-1, Latin Extended-A, Yunanca ve temel Kiril eklendi
  (`text_chars`). Linux imajında DejaVu'nun eğik yüzü yok, yani havuz iki
  yüzle de taşmalı.
- **Ortalama sınaması göreli.** Plandaki "sol ve sağ boşluk ±1 eşit"
  ölçütü fontun kendi yan boşluklarını ölçüyordu (`M` 13pt@1x'te 1/3);
  sınama glyph'in açılan payın yarısı kadar kaydığını ve boyunun
  değişmediğini sınıyor (`H n o`).
- **`wide_glyph_stays_split_*` yalnız macOS'ta**: Linux imajında CJK fontu
  yok; renk düzleminde yuva 0 gerçek bir yuva (tofu payı yok), sınama bunu
  ayırıyor.
- `make smoke` bu makinede üst commit'te de `frames=0` veriyor (ekran
  kapalı/kilitli); çevresel, koşulamadı.
