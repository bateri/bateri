# Phase 3 — `cursor_unfocused`

## Özet

Odak kaybında imlecin içinin boşalması **ayardan kapatılabilsin**; varsayılan
bugünkü davranış.

_Requirements: R9, R9.1, R9.2, R9.3, R1.3, R1.4, R2, R3, R7, R8_

> **phase-1'in tesisatını tüketiyor.** `CaretStyle`, `Changes::caret`,
> `set_caret_style` ve `clear`'ın ikinci argümanı orada kuruldu; bu phase yeni
> yol açmıyor, üçüncü bir alan ekliyor.

## Değişiklikler

- **`crates/bt-core/src/settings.rs`**
  - `UnfocusedCaret { Hollow, Solid }` ve `CaretStyle`'ın **üçüncü alanı**
    (R9.1). Varsayılan `Hollow` = bugünkü davranış, yani dosyası olmayan
    kullanıcı hiçbir fark görmüyor.
  - **`named_enum` yardımcısı** (R9.3): "şu adlardan biri, değilse tanı bırak
    ve anahtarı kendi değerinde bırak" kalıbı depoda **beş kez** elle yazılmış
    ve `docs/YOL-HARITASI.md`'nin borcu altıncı kopyayı adıyla öngörüyor.
    Beş kopyanın taşınması **bu phase'in işi değil** — `ranged_float`'ta
    olduğu gibi ayrı karar ve Uygulama Notları'na yazılır.
  - `TEMPLATE` + `template_is_the_defaults` listesi.
- **`crates/bt-gpu/src/frame.rs`** — `push_caret`'in `hollow` yüklemi üçüncü
  terimini alıyor: `!focused && Block && style.unfocused == Hollow`.
  **Blink'e dokunulmuyor** (R9.2): `"solid"` yalnız içi boşalmayı kapatıyor,
  odakta blink'in durması 015'in kararı ve ayrı bir sinyal.
- **`docs/AYARLAR.md`** — tablo + Şablon bloğu (kaynaktan senkron).
- **`CLAUDE.md`** — caret cümlesi: içi boşalma artık ayara bağlı.

## Kabul

- `cursor_unfocused = "solid"` yazıp kaydetmek odaksız imleci **dolu**
  bırakıyor; `"hollow"` bugünkü görüntüye dönüyor. İkisi de kayıt anında.
- `"solid"` iken odakta blink **yine duruyor** — iki sinyal ayrı.
- Kabul edilmeyen değer kendi anahtarını değiştirmiyor + tanı.
- Ayar `bt-core`'da yaşıyor ama `TerminalOptions`'a **girmiyor**: `Changes`
  farkı `caret` alanından geçiyor.

## Uygulama Notları

- **`named_enum` doğdu, beş kopya taşınmadı.** Kalıbı depoda beş kez elle
  yazılmış (`osc52`, `cursor_motion`, `reduce_motion`, `cursor_blink`,
  `caret_shape`) ve her birinin tanı cümlesi kendi sözcükleriyle; taşımak
  mesajları tek turda değiştirirdi. `ranged_float`'ın `font_size`'ı bırakması
  ile aynı ölçüt. Yol haritasındaki borç artık **yarı kapalı**: yardımcı var,
  taşıma ayrı iş.
- **Beklenen mesaj listeden üretiliyor** (`"a", "b" or "c"`), elle yazılmıyan
  tek yer burası — iki değerli anahtarda `"hollow" or "solid"` çıkıyor.
- **Bekçi iki yönlü:** aynı girdide `solid` doldurup `hollow` boşaltıyor, yani
  ayrım gerçekten anahtardan geliyor. Tek yönlü bir sınama "içi hiç
  boşalmıyor" hâlinde de yeşil kalırdı.
- **Kullanıcının `settings.toml`'u da tazelendi** (phase-1'de olduğu gibi,
  elle): şablondan yeniden üretilip değerleri korundu. Bu her anahtarda
  tekrarlanan bir el işi ve kalıcı çaresi ayrı bir iş —
  `docs/YOL-HARITASI.md`'ye borç olarak yazılacak.

## Yayın Etkisi

- **ayar şeması** — üçüncü anahtar; varsayılanı bugünkü davranış, eski anahtar
  akıbeti yok. Mevcut kullanıcı dosyası **değişmiyor** (`create_if_missing`
  yalnız dosya yokken yazıyor) — aynı göç notu.
- **belge** — `docs/AYARLAR.md`, `CLAUDE.md`.
- shader / terminfo / app bundle / ölçüm / yeni bağımlılık: yok.
- **geri alma:** phase commit'ini revert; anahtar emekli edilir ve davranış
  `Hollow`'a sabitlenir.

## Checklist

- [x] `settings.rs`: `UnfocusedCaret`, `CaretStyle`'ın üçüncü alanı
- [x] `settings.rs`: `named_enum` **adıyla** doğdu (R9.3); beş kopya taşınmadı
- [x] `settings.rs`: `TEMPLATE` + `template_is_the_defaults` listesi
- [x] `frame.rs`: `hollow` yükleminin üçüncü terimi; blink'e dokunulmadı
- [x] Test: `"solid"` odaksız imleci dolu bırakıyor, `hollow` aynı girdide boşaltıyor
- [~] Test: blink'e dokunulmadığı **koddan** açık (`hollow` yüklemi blink'e hiç değmiyor);
      ayrı bir sınama `DisplayLink` isterdi — 015'in aynı sınırı
- [x] Test: kabul edilmeyen değer + tanı
- [x] `docs/AYARLAR.md` (Şablon kaynaktan senkron), `CLAUDE.md`
- [x] Doğrulama geçti (`make hepsi` — exit 0)
- [ ] **Gözle kontrol:** `"solid"` yazıp kaydet, başka pencereye geç — imleç
      dolu kalmalı
- [x] Yayın etkisi yazıldı
