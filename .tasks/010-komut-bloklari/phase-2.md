# Phase 2 — Bloklar `frame()` sınırından geçer

## Özet

`frame()` çıpayı ızgaradan okur, kimlikleri defterden renklendirir ve blok
aralıklarını **çözülmüş** olarak sınırdan verir; çizen taraf henüz yok.

_Requirements: R3, R3.1, R3.2, R3.3, R3.4, R6, R6.1_

## Değişiklikler

- **`crates/bt-core/src/theme.rs`** — **iki** yeni durum rolü: `success` ve
  `error`. Rol burada doğuyor çünkü "renk çözülmüş geçer" kararının ilk
  tüketicisi `frame()`; çizen taraf renk **üretmiyor**, alıyor. Koşan
  bloğun rengi mevcut `accent`'tan gelir, yani üçüncü bir rol eklenmiyor.
  `dim` **yeniden kullanılmıyor**: `CLAUDE.md`'nin "`dim` rolü yalnız
  varsayılan ön planın" cümlesi bağlayıcı. Kalan iki durum rolü (uyarı,
  bilgi) 013'e kalır — çizilmeyen rol eklenmiyor (Karar 4b). Eksik ve
  bilinmeyen anahtarın iki yönde de sessiz kalması korunur;
  `unknown_keys_are_silent` sınamasının örneği artık bilinen bir anahtar,
  sentinel değişir.
- **`crates/bt-core/src/session.rs`** — `frame()` iki fazlı olur.
  **Faz 1**, `Term` kilidi altında: hücre döngüsünde, hücrenin **atlama
  kapısından sonra**, hyperlink'ten kimlik çekilir ve `(aid, ilk_satır)`
  çiftleri çağıranın yeniden kullandığı bir tampona toplanır. Kapıdan sonra
  olması `fg`/`underline_color`'ın kapıdan sonra çözülmesiyle aynı disiplin:
  kapı "çizilecek bir şey var mı" diye sorar. Hyperlink okuması `extra`
  yokken anında dönüyor, yani çizilen hücre başına tek bir boş kontrol.
  **Faz 2**, kilit bırakıldıktan sonra: kimlikler defterden renklendirilir.
  Sıra zorunlu — `session.rs`'in yazılı kuralı yaprak kilidin `Term`
  kilidinin altına girmemesi.
  Blok aralığı: bir kimliğin başladığı satırdan bir sonraki kimliğin bir
  üstüne; son bloğun sonu pencerenin altı. Şerit **prompt satırından**
  başlar, yani blok komutun kendisini de kapsar.
- **`crates/bt-core/src/lib.rs`** — yeni `pub` tip (satır aralığı + renk)
  ihraç listesine ve modül başlığındaki sınır cümlesine girer. Alacritty
  tipi yine görünmez: kimlik `u32`, renk `LinearRgba`.

## Kabul

- Çıkış kodu **sınırı geçmez**: `bt-gpu`'ya yalnız satır aralığı ve renk
  gider (`CLAUDE.md` → karar burada, boyama orada).
- Kaydırma: geçmişe kaydırılmış pencerede blok aralıkları hücrelerle **aynı**
  `display_offset`'ten çıkar; şerit bir kare geride kalmaz.
- Pencere üstü: ilk görünür çıpa `N` ise üstündeki satırlar `N−1`'in.
  `N−1` defterde yoksa (halka dolaştı, sayaç sıfırlandı) o bölge
  **çizilmez**.
- Hiç çıpa görünmüyorken kabuk `Running`'se pencere son `A`'nın bloğuna
  aittir; `Input`'taysa **çizilmez**. Bilinmeyeni yanlış çizmemek bu
  tasarımın savunma tezi ve köşede de tutar.
- **Koşan bloğun rengi defterden değil safhadan gelir:** `D` henüz
  gelmediği için defterde kaydı yok. Bu, "kimlikler defterden
  renklendirilir" kuralının tek istisnası ve tipin şeklinde görünmeli.
  Rengi bugün `accent`; **teslimde kullanıcıya sorulacak tek kalem** — imleç
  rengiyle aynı şerit dikkat dağıtabilir.
- Alternatif ekranda blok verilmez (`TermMode::ALT_SCREEN`; erişim
  `session.rs`'te zaten var).
- Kullanıcının kendi teması okunmaya devam eder; iki yeni anahtarı yazmamış
  tema onları gömülü `bateri`'den miras alır.
- Reflow: pencereyi yatay boyutlandırdıktan sonra aralıklar hâlâ prompt
  satırlarında başlar — çünkü satır ızgaradan okunuyor, hatırlanmıyor.
- Kare başına ayırma yok: tampon çağıranda yaşar ve yeniden kullanılır.

## Yayın Etkisi

- **`CLAUDE.md`** — 009'un erdem diye yazdığı "`frame()` imzası değişmedi"
  cümlesi düşer; `bt-core`'un sorumluluk satırına blok aralığı girer.
- **`crates/bt-core/src/lib.rs`** başlık yorumu: `pub` API listesine yeni
  tip eklenir.
- **tema / materyal biçimi** — iki yeni rol. Gömülü `bateri` ve
  `bateri-light`'ın ikisine de değer girer, `docs/AYARLAR.md`'nin Temalar
  bölümü ve iki tema bloğu aynı commit'te güncellenir
  (`documented_blocks_are_the_embedded_themes` bunu mekanik olarak
  zorluyor). Kendi **açık** temasını yazmış kullanıcı iki rolü koyu
  `bateri`'den miras alır — `dim` ile aynı, kabul edilmiş kusur;
  `docs/AYARLAR.md`'deki o uyarı üç role genişler.
- **`CLAUDE.md`** — "durum rolleri 013 ile gelir" ve "Bugün dördü
  tüketiliyor" cümleleri düzelir; `theme.rs`'in "013'ün durum rolleri"
  notu da.
- `bt-gpu`'nun `Cursor` literalli kare sınamaları mekanik olarak düşebilir;
  aynı commit'te düzelir.
- Ayar şeması, tema biçimi, shell entegrasyonu, terminfo, shader, bundle:
  değişiklik yok. Yeni bağımlılık yok.
- Ölçüm iddiası yok: bu phase kare süresi, gecikme ya da bellek sayısı
  iddia etmiyor, dolayısıyla "ölçüm bekliyor" satırı da yazılmaz.

## Checklist

- [ ] `Theme`'e `success` ve `error`; gömülü iki temaya değerler;
      `docs/AYARLAR.md` ve `CLAUDE.md` güncellendi
- [ ] Test: tema ayrıştırma — iki yeni rol, eksik anahtar miras alıyor
- [ ] `frame()` faz 1: kimlik çekme, atlama kapısından sonra, yeniden
      kullanılan tampona
- [ ] `frame()` faz 2: kilit bırakıldıktan sonra defterden renk
- [ ] Yeni `pub` tip ve `lib.rs` ihracı
- [ ] Test: iki blok, aralıkların sınırı bir sonraki kimliğin bir üstü
- [ ] Test: pencere üstü `N−1` kuralı; `N−1` defterde yokken çizilmiyor
- [ ] Test: çıpasız pencere — `Running`'de son bloğa ait, `Input`'ta boş
- [ ] Test: alternatif ekranda blok yok
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] Yayın etkisi yazıldı
