# Phase 2 — Caret'in kendi fragment'i

## Özet

Caret `cell_bg`'nin düz dörtgeninden çıkıp **kardeş bir fragment**'e taşınsın;
köşesi yuvarlansın ve çevresinde hafif bir hale olsun.

_Requirements: R2, R2.1, R2.2, R2.3, R3, R4, R5, R6, R8, R9, R10_

## Değişiklikler

- **`crates/bt-gpu/shaders/cell_bg.metal`** — `caret_fragment`, `cell_bg_vertex`
  ile eşleşiyor. Yuvarlak dikdörtgen SDF'i: dolgu, kenar (içi boş imleç için
  hazır ama bu phase'de sıfır) ve hale. İç dikdörtgen `CursorBlock`'tan —
  o pencere uzayında ve `[[position]]` ile aynı uzayda, `cell.metal`'in ters
  çevirmesi de öyle okuyor.
  - **`cell.metal` açılmıyor** (R2.2): yuvarlak köşenin ters çevirmeyle
    çakışması **yok** — gerekçe `discussion.md` → Karar 2, geçerlilik koşulu
    `Cursor::text == tema zemini`.
  - **Her alan okunuyor** (R2.3): okunmayan bir uniform alanı hiçbir pikselin
    doğrulamadığı bir sözleşme olurdu.
- **`crates/bt-gpu/src/renderer.rs`** — üçüncü pipeline: bir alan, bir
  `pipeline()` çağrısı, `encode_quads`'ın kardeşi bir `encode_caret`. Şekil
  uniform'u fragment'e `setFragmentBytes` ile. `cell_bg_pipeline_builds`'in
  kardeşi bir sınama (üçüncü pipeline = açılışta üçüncü `MissingFunction` kolu).
  - **Yeni `#[repr(C)] ↔ .metal` çifti yok** (R2.1): caret kare başına tek quad,
    şekil uniform'dan geçiyor. Hizalama tuzağı hiç doğmuyor.
- **`crates/bt-gpu/src/frame.rs`** — üç iş:
  1. `caret_rect` **iki** dikdörtgen döndürüyor (R5): boyanan ve **opak iç**.
     Değişmez ("tek yer, iki tüketici") kırılmıyor, eksik tanımlıydı.
  2. Quad hale payı kadar **şişiyor**, ama **yuva seçimi şişmemiş dikdörtgene
     bakmaya devam ediyor** (R4). Aksi hâlde caret dock yuvasına kayar,
     ızgaranın glyph'lerinden sonra çizilir ve altındaki harfi boyar — 014
     phase-1'de aynı tuzağa düşülmüştü.
  3. *(014'ün kapısında düzeldi — `Frame::move_caret` ve `Frame::push`'un
     bayat `truncate(bg_count)` atıfları temizlendi.)*
- **Şekil sayıları türetiliyor** (R3): kenar kalınlığı `CellMetrics::rule_px`,
  hale payı `gutter_px`, yarıçap hücre ölçüsünün oranı. Türetilemeyen kalırsa
  `const` doc'unda "seçilmiş, ölçülmemiş" **ve hangi metriğin neden yetmediği**.
- **`CLAUDE.md` ve `crates/bt-gpu/src/lib.rs` başlığı** — pipeline sayısı ikiden
  üçe çıkıyor ve caret'in çizimi artık kendi fragment'inde.
- **`docs/YOL-HARITASI.md`** — dokunulmuyor: altıncı kayma notu ve 015'in kendi
  satırı **2026-09-19'da yazıldı** (klavye seti sıraya girerken aynı tablo
  düzeltildi). Burada tekrar edilirse iki yerden yazılan bir satır olur.

## Kabul

- **Dejenere kolda çıktı bit bit bugünküyle aynı:** yarıçap 0, pad 0, hale
  alfası 0 iken mevcut piksel sınamaları (`glyph_under_the_cursor_takes_the_
  cursor_text_color`, `cursor_rect_stops_at_its_own_cell`) **değişmeden**
  geçiyor. Şart: o kolda kenar **sert adım** kalmalı — `smoothstep`/`fwidth`
  daha girerse eşitlik yarıçap gelmeden kırılır.
- Yarıçap açılınca iki piksel sınaması **gürültülü** düşüyor ve tamiri iddiayı
  **ikiye ayırıyor**, toleransa çevirmiyor: dolgunun ve ters çevirmenin hücreyle
  sınırlı olduğu (iç piksellerde, hâlâ kesin) + halenin taşabildiği ama sınırlı
  olduğu (yeni). Toleransa çevirmek `bt-core`'dan inmiş iki iddiayı sessizce
  zayıflatırdı.
- **Hale blink alfasıyla sönüyor** (R6) ve bekçisi dikdörtgenin **dışını**
  örnekliyor. Mevcut `cursor_alpha_is_blended_on_the_gpu` yalnız caret'in kendi
  hücresine bakıyor, yani belirtiyi göremez — onu çoğaltmak bekçi taklidi olurdu.
- Punto değişince (Cmd +/−) yarıçap, kalınlık ve hale payı **kendiliğinden**
  ölçekleniyor.
- `make duman` jetonları değişmiyor: `hucre=` imleci zaten saymıyor ve hermetik
  koşuda dock yok.

## Uygulama Notları

<!-- Kodlanırken doldurulacak. -->

## Yayın Etkisi

- **shader** — `.metal` değişti: `make shader` koştu mu; yeni uniform'un Rust
  karşılığıyla **alan alan** aynı olduğu iki taraflı assert'le bağlı mı.
- **belge** — `CLAUDE.md`, `bt-gpu/src/lib.rs` başlığı, iki bayat doc.
  `docs/YOL-HARITASI.md` **bu phase'in işi değil**, zaten yazıldı.
- **seçilmiş sayı:** türetilemeyen kalırsa `const` doc'unda gerekçesiyle;
  `docs/OLCUMLER.md`'nin konusu değil (estetik sabit, emsali `OMEGA` /
  `FADE_DURATION`).
- **bilinen sınır, beş madde** (R9): dock zemininin örtmesi, `origin_y`'de
  kırpılma, viewport'ta kırpılma, artık şeritte **kalma**, komşu arka planların
  solması. Üçü kırpma, ikisi görünür etki — beşi de `teslim.md`'ye.
- ayar şeması / tema / terminfo / app bundle / yeni bağımlılık: yok.
- **geri alma:** ayar anahtarı **yok** (R8); yol phase commit'ini revert etmek
  ve dejenere kol (yarıçap 0, hale 0) desteklenen ve sınanan bir hâl olarak
  duruyor.

## Checklist

- [ ] `cell_bg.metal`: `caret_fragment` (yuvarlak dikdörtgen SDF, dolgu + kenar
      + hale), her alan okunuyor
- [ ] `renderer.rs`: üçüncü pipeline + `encode_caret` + şekil uniform'u
- [ ] `frame.rs`: `caret_rect` iki dikdörtgen; quad şişiyor, **yuva seçimi
      şişmemişe bakıyor**
- [ ] Sayılar `CellMetrics`'ten türetildi (`rule_px`, `gutter_px`, hücre ölçüsü)
- [ ] Test: dejenere kolda mevcut piksel sınamaları değişmeden geçiyor
- [ ] Test: `caret_pipeline_builds`
- [ ] Test: hale sınırı — dikdörtgenin **dışı** örnekleniyor
- [ ] Test: hale blink alfasıyla sönüyor (yine **dışarıdan** örnekleyerek)
- [ ] İki piksel sınamasının iddiası **ikiye ayrıldı** (toleransa çevrilmedi)
- [ ] `CLAUDE.md`, `lib.rs` başlığı
- [ ] Doğrulama geçti (`make hepsi` + `make shader`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
- [ ] `make duman` (kullanıcıda)
- [ ] **Gözle kontrol:** yarıçap ve halenin görüntüsü; seçili hücrenin üstündeki
      caret'in köşeleri (Karar 4'ün geçerlilik koşulunun tek görünür yeri);
      halenin komşu seçim vurgusunu soldurması
- [ ] Yayın etkisi yazıldı
