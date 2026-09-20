# Phase 2 — Belgeler ve borcun kapanması

## Özet

Kullanıcıya bakan belgeler yedeği anlatıyor; yol haritasının borcu kapanıyor.

_Requirements: R6_

## Neden ayrı phase

Phase-1 yalnız **kodla çelişen** cümleleri düzeltiyor (`CLAUDE.md`,
`lib.rs` başlığı) — `CLAUDE.md`'nin aynı-commit kuralı bunu zorunlu kılıyor.
Buradakiler **çelişmiyor, eksik kalıyor**: kullanıcı diline çevrilmiş
açıklamalar ve indeks bakımı. Tek başına doğrulanabilir ve `make hepsi`'yi
yeşil bırakıyor.

## Değişiklikler

- **`docs/AYARLAR.md`** — iki kalem:
  1. `[font] family` bölümünün "aile verilmezse SF Mono" ifadesi
     **netleşiyor**: zincir SF Mono'yu tercih ediyor ama o Xcode ile geliyor
     ve **her makinede yok**; yoksa Menlo. Ölçüldü (bu makinede SF Mono
     kurulu değil, `CTFontCreateWithName` Helvetica veriyor ve zincir
     Menlo'ya düşüyor).
  2. Kırpma cümlesinin yedek ikizi: seçili fontta olmayan **tek hücrelik**
     karakter sistemden geliyor; sığmayan (emoji, CJK, geniş ok) kutu kalıyor
     ve **bu bilinçli** — yarım çizilmiş bir glyph sessiz bozulma, kutu
     görünür eksiklik.
- **`crates/bt-core/src/settings.rs`** — şablon yorumunun `AYARLAR.md` ikizi;
  ikisi birlikte değişir.
- **`docs/YOL-HARITASI.md`** — "Font fallback yok" borcu kapanıyor; kalan
  kısım (emoji, geniş glyph) zaten 021'de.
- **`.tasks/README.md`** — indeks notu.

## Kabul

- `AYARLAR.md` ile `settings.rs` şablonu aynı şeyi söylüyor.
- Yol haritasında kapanmamış bir "font fallback" maddesi kalmıyor.
- `make hepsi` yeşil.

## Yayın Etkisi

- **`docs/AYARLAR.md`** ve **`crates/bt-core/src/settings.rs`** — yukarıda.
  **Ayar şeması değişmedi:** yeni anahtar yok, varsayılan yok, eski anahtarın
  akıbeti yok — yalnız açıklama metni. Şablonun iki kopyası
  `documented_template_is_the_template` ile bağlı ve yeşil.
- **`docs/YOL-HARITASI.md`** — "font fallback yok" borcu kapanıyor; ölçümün
  çıkardığı yeni borç (blok/çizgi/Braille fonttan geliyor ve döşemiyor)
  ekleniyor ve 021'in satırı ona bağlanıyor.

## Uygulama Notları

- **Şablonun iki kopyası zaten bağlı.** `settings.rs`'in `TEMPLATE`'i ile
  `AYARLAR.md`'nin `### Şablon` bloğu `documented_template_is_the_template`
  ile bit bit karşılaştırılıyor, yani "ikisi birlikte değişir" bir dilek
  değil kapı. İkisine de aynı iki cümle girdi.
- **Kullanıcı ölçümü kapsamı genişletti.** Phase yazılırken planlanan yedek
  cümlesi "sığmayan (emoji, CJK, geniş ok) kutu kalıyor" diyordu; kullanıcı
  019 kurulduktan sonra maskotun hâlâ bozuk olduğunu bildirdi ve ölçüm iki
  şey çıkardı:
  1. **Maskot yedeğin konusu değil.** Blok elemanları (U+2580–U+259F) ve
     çizgi çizim (U+2500–U+254B) **zaten Menlo'da**; yedek yoluna hiç
     girmiyorlar. Sorun Menlo'nun `█`'inin hücreyi doldurmaması — 13pt'de
     8×18 hücrenin yalnız 3–16 satırları boyanıyor, iki blok alt alta gelince
     ~5 piksel şerit kalıyor. Ekran görüntüsündeki boşluk 11 aygıt pikseli,
     @2x ile birebir tutuyor.
  2. **Braille kapıdan dönüyor.** Apple Braille'den geliyor ve oranı
     **1.1354×**; Claude Code'un spinner'ı braille, yani kutu kalıyor.
  Kullanıcı kararı: ikisi de yol haritasına yazılsın, 019 planlandığı gibi
  bitsin, çare yordamsal çizim olsun. `AYARLAR.md`'nin cümlesine Braille de
  eklendi (kullanıcının gerçekten göreceği kalem), yol haritasına yeni bir
  borç maddesi girdi ve 021'in satırı "kutu çizim yarısı ayrılabilir" notunu
  aldı — 019'un 020/021'den ayrıldığı gerekçenin aynısı.
- **`AYARLAR.md`'nin SF Mono kalemi kısmen zaten netti** (tablo ve şablon
  "SF Mono, yoksa Menlo" diyordu); eksik olan **neden** yoklukla
  karşılaşılabildiğiydi (Xcode ile geliyor) ve uyarı verilmemesinin gerekçesi.
  İkisi de eklendi.

## Checklist

- [x] `docs/AYARLAR.md`: SF Mono netleştirmesi + yedek cümlesi
- [x] `settings.rs` şablon yorumu (ikizi; `documented_template_is_the_template`
      ikisini bağlıyor ve yeşil)
- [x] `docs/YOL-HARITASI.md`: borç kapandı
- [x] `docs/YOL-HARITASI.md`: ölçümün çıkardığı yeni borç yazıldı (blok/çizgi/
      Braille yordamsal çizim) + 021'in satırı işaretlendi
- [x] `.tasks/README.md`: indeks notu
- [x] Doğrulama geçti (`make hepsi`)
- [x] Yayın etkisi yazıldı
