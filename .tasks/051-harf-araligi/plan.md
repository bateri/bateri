# Harf aralığı (`[font] letter_spacing`)

## Hedef

`[font] letter_spacing` çarpanıyla hücre genişliği açılabilsin. Harfler
genişleyen hücrenin ortasında dursun, sütunlar açılsın. Davranış
`line_height`'ın yatay ikizi olsun: aynı birim, aynı uygulanma anı, aynı tanı.

## Gereksinimler

- `letter_spacing` bir çarpan: `1.0` fontun kendi aralığı (varsayılan), aralık
  `1`–`2`. Dışındaki ya da sayı olmayan değer varsayılana (kayıt anında o anki
  değere) dönsün ve tanı bıraksın, `line_height`'ın kuralıyla.
- Hücre genişliği çarpılan ilerlemeden türesin. Harf hücrede ortalı dursun,
  yedek glyph kapısı ve ortalama aynı sayıyı görsün. `1.0`'da raster ve ızgara
  bugünküyle bit bit aynı kalsın.
- İki sütunlu karakter (CJK, emoji) açılan aralıkta da iki sütunun ortasında
  kalsın, tek hücreye düşüp sola kaymasın.
- Bağlam satırının küçük sınıfı da aynı oranda açılsın, yordamsal sprite'lar
  (kutu, blok, alt çizgi) geniş hücrede bitişik kalsın.
- Kayıt anında uygulansın: sütun sayısı yeniden hesaplansın, PTY yeni boyutu
  alsın. Cmd +/− çarpanı taşısın.
- Ayar penceresinde Line height'ın yanında bir Letter spacing alanı olsun ve
  yalnız o anahtarı yazsın.
- `docs/AYARLAR.md` ve ayar şablonu anahtarı anlatsın.
- En büyük çarpanlarda (`letter_spacing` ve `line_height` birlikte en üstte)
  atlas kapasitesi yordamsal aileyi hâlâ taşısın.

## Kapsam Dışı

- `1`'in altı (daraltma): geniş harflerin kenarı sessizce kırpılırdı, aynı
  koruma `line_height`'ta da var. İstenirse ayrı iş (küçülterek daraltma).
- Piksel birimli ekleme (Alacritty'nin `offset.x`'i): tek birim, çarpan.
- Tema dosyası ya da sekme başına harf aralığı.

## Durum

| Phase | Durum |
|-------|-------|
| phase-1 | ✅ |
| kapı | ✅ |
