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

- **Çekirdek dikdörtgen `CursorBlock`'tan değil, kendi çıplak `float4`'ünden
  geliyor** (plan `CursorBlock` diyordu). İki sebep: (a) phase-3'te boyanan
  dikdörtgen ile ters çevirmenin opak içi **ayrılıyor** (R5) ve fragment'in
  istediği boyanan olan — `CursorBlock.rect` o phase'de boşalacak; (b)
  `CursorBlock` `cell.metal`'de tanımlı ve `cell_bg.metal`'e kopyalamak aynı
  düzenin **üçüncü** yazarını doğururdu. Çıplak `float4` hizalama tuzağını da
  hiç doğurmuyor: `[f32; 4]` ↔ `float4` tek başına argüman olarak iki tarafta
  da 16 bayt, ofset 0.
- **`caret_rect` tuple çifti değil `CaretRects` döndürüyor.** İki dikdörtgenin
  hangisi olduğu çağrı yerinde adıyla okunuyor; `(( , ), ( , ))` iç içe
  tuple'ı sessizce yer değiştirebilirdi.
- **Dejenere kolun bekçisi bir `#[cfg(test)]` semi istedi** (`Frame::force_caret_shape`).
  R8 "yarıçap 0, hale 0 desteklenen ve sınanan bir yol" diyor ve bunu yalnız
  GPU söyleyebilir — dejenere kolda fragment `step`, açık kolda `smoothstep`
  kullanıyor. Üretimde çağıranı olsaydı ölü kod olurdu.
- **İddia "orta bant" olarak ayrıldı, tam içeri çekme olarak değil.** Yarıçap
  hücre **yüksekliğinden** türüyor ve dar bir hücrede genişliğin yarısına
  yaklaşıyor; sütunları da çekmek bandı büsbütün yutuyordu (ilk deneme tam
  buradan kırmızı düştü). Yuvarlaklık köşelerde, `radius` ile `ch - radius`
  arası her satırda şekil **tam genişlikte** — yalnız satır çekmek doğrusu.
- **Mevcut sınama ızgarasının payı sıfır**, yani eski piksel sınamalarında hale
  hiç doğmuyor (`glow_px` = `gutter_px`). Hale bekçileri bu yüzden kendi
  paylı ızgaralarını kuruyor (`grid_with_gutter`). Yan bilgi: eski sınamaların
  yarıçapı yine de gördü, çünkü o hücre yüksekliğinden geliyor.
- **`caret_pipeline_builds` kardeşine çok benziyor** ve bilerek: `system_default()`
  üç pipeline'ı da kuruyor, yani gövde aynı. Ayrı durmasının sebebi adlandırdığı
  **ayrı `MissingFunction` kolu** — `caret_fragment` metallib'de bulunamazsa
  belirti derleme hatası değil, açılışta düşen bir pipeline olur.
- **Hale bir kez gözle küçültüldü.** İlk hâli sol payın **tamamı** kadar
  yayılıyordu (varsayılan puntoda ~8 px) ve tepe alfası 0.35'ti; altın bir
  bloğun çevresinde çıkan şey gölge değil neon oldu ("bu nasıl shadow pavyona
  döndü ortalık" — kullanıcı, ilk bakışta). Pay yarıya, alfa 0.14'e indi.
  Türetme bozulmadı: kaynak hâlâ tek (`gutter_px`), yalnız oranı var
  (`CARET_GLOW_RATIO`) ve sınamalar o oranı üretimle **paylaşıyor** — sabit
  yazsalardı hale küçülünce boş bir noktaya bakarlardı.
- **Hale caret'in alfasını bedavaya izliyor:** `in.rgba.a` hepsini çarpıyor,
  yani blink ve belirme haleye ikinci bir yol yazmadan iniyor (R6).

## `/code-review` (riskli phase: `make shader` gerekti)

**15 bulgu; 11 düzeltildi, 4 bilinen sınıra yazıldı.** Üçü gerçek bekçi
kusuruydu ve üçü de bendendi:

- **`the_caret_corner_is_rounded` totolojiydi.** Referansı caret'in **kendi**
  karşı köşesinden alıyordu; SDF simetrik olduğu için iki köşenin mesafesi
  her zaman eşit, yani iddia hiçbir yarıçap değerinde düşemezdi. Referans
  dışarı taşındı ve sınama artık yarıçapı **açıkça veriyor** — üretim oranı
  bir zevk sayısı ve 1x hücrede ~1.6 px'e denk geliyor, yani ona bağlı bir
  eşik hücre boyuna göre kızardı. (Kanıt: düzeltilmiş hâli üretim oranıyla
  koşunca köşe %69 boyalı çıktı ve kırmızı düştü.)
- **İçi boş caret kolu ölü sevk ediliyordu.** `caret_sdf()` `stroke`'u sabit 0
  veriyor, yani shader'ın `stroke > 0` dalı hiç koşmamıştı ve phase-3 onu
  "yazılmış ve geçmiş" sanarak açacaktı. Artık bir sınama o dalı sürüyor
  (`a_hollow_caret_paints_only_its_edge`) — `body -= inner` çıkarmasının
  gövdeyi sıfırlamadığı da orada çiviliydi.
- **`the_caret_glow_fades_with_the_caret` taşabiliyordu.** `pixel_at` x'i
  sınırlamıyor: geniş hücreli bir makinede örnekleme noktası satırı taşar ve
  sınama **bir alt satırın** pikselini okuyup sessizce yanlış iddia ederdi.

Kalan sekiz düzeltme yapı ve belge: `encode_caret` `encode_quads`'ın gövdesini
kopyalamıştı (ortak `draw_quads`'a indi, tampon kararı yine tek yerde),
caret'in tekilliği yalnız doc'ta yazılıydı (`debug_assert`'e bağlandı), adlar
çakışıyordu (`caret_shape` alanı `CaretShape`, metodu `[f32; 4]` — metot
`caret_sdf` oldu), yarıçap kırpmasının **üç** yazarı vardı (değer Rust'a
alındı, shader'ınki matematik ön koşulu olarak kaldı ve öyle yazıldı),
kapsama getirmeyen `caret_pipeline_builds` silindi (`system_default()` zaten
on küsur sınamada üç pipeline'ı da kuruyor), ve dört doc bayattı (`cell_bg`
artık imleci çizmiyor, `instance_buffer`'ın "iki pipeline"ı, `pipeline()`'ın
"iki pipeline"ı, `glow_px`'in "sol payın ta kendisi"si).

### Bilinen sınıra yazılan dört bulgu

1. **Ters çevirme dikdörtgeni keskin, boyanan alan yuvarlak.** Köşede caret
   boyamıyor ama `cell.metal` hâlâ o pikseli `Cursor::text` ile çiziyor.
   **Varsayılan zeminde görünmez** ve sebebi Karar 2'nin geçerlilik koşulunun
   ta kendisi: `Cursor::text == tema zemini`, yani köşedeki mürekkep zemin
   renginde çizilip zemine karışıyor — görünmesi gereken şey zaten o.
   **Görünür olduğu yer:** hücrenin zemini temanınkinden farklıysa (seçim,
   SGR arka planı) köşedeki mürekkep tema zemini renginde bir çentik bırakır.
   Yarıçap küçüldüğü için (%18 → %10) alan da küçüldü. Çaresi `cell.metal`'e
   SDF taşımak, yani Karar 2'yi yeniden açmak — bu setin konusundan büyük.
2. **Halenin üstü ilk içerik satırında kırpılıyor.** Izgara viewport'u
   `originY = origin_px` ile başlıyor ve üstünde kalan fragment'ler kırpılıyor.
   R9 bunu **zaten sayıyordu** ("`origin_y`'de kırpılma", "viewport'ta
   kırpılma"); hale sönükleştikten sonra belirti de sönük. Çaresi caret'i ayrı
   bir viewport'ta encode etmek ve o, NDC ölçek eşleşmesini bozma riski
   taşıyor.
3. **Dejenere hücre bekçisi `.painted`'e bakıyor**, GPU'ya giden şişmiş
   dörtlüye değil. Bugün güvenli çünkü sıfır hücre ancak sıfır payla oluyor ve
   hale de sıfır kalıyor; kural yazılı değildi, artık burada.
4. **Caret'in yuvarlak köşesinde glyph'e ne olduğunu hiçbir bekçi sormuyor.**
   `glyph_under_the_cursor_...` o bölgeyi iddiadan çıkardı (orta bant),
   `the_caret_corner_is_rounded`'ın hücresi ise boş. 1. maddenin bekçisi
   olacak sınama, o madde çözülürse yazılır.

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

- [x] `cell_bg.metal`: `caret_fragment` (yuvarlak dikdörtgen SDF, dolgu + kenar
      + hale), her alan okunuyor
- [x] `renderer.rs`: üçüncü pipeline + `encode_caret` + şekil uniform'u
- [x] `frame.rs`: `caret_rect` iki dikdörtgen; quad şişiyor, **yuva seçimi
      şişmemişe bakıyor**
- [x] Sayılar `CellMetrics`'ten türetildi (`rule_px`, `gutter_px`, hücre ölçüsü)
- [x] Test: dejenere kol düz dörtgenle bit bit aynı (`a_degenerate_caret_shape_...`)
- [x] Test: `caret_pipeline_builds`
- [x] Test: hale sınırı — dikdörtgenin **dışı** örnekleniyor
- [x] Test: hale blink alfasıyla sönüyor (yine **dışarıdan** örnekleyerek)
- [x] İddia **ikiye ayrıldı** (toleransa çevrilmedi); yalnız biri düşmüştü —
      `cursor_rect_stops_at_its_own_cell` paysız ızgarada hiç etkilenmedi
- [x] `CLAUDE.md`, `lib.rs` başlığı
- [x] Doğrulama geçti (`make hepsi` exit 0 + `make shader` exit 0)
- [x] Riskli phase: `/code-review` koştu — 15 bulgu, 11 giderildi, 4 bilinen sınıra
- [ ] `make duman` (kullanıcıda)
- [ ] **Gözle kontrol:** yarıçap ve halenin görüntüsü; seçili hücrenin üstündeki
      caret'in köşeleri (Karar 4'ün geçerlilik koşulunun tek görünür yeri);
      halenin komşu seçim vurgusunu soldurması
- [x] Yayın etkisi yazıldı
