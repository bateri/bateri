# Phase 4 — Glyph: `frame()` sınırı, `cell` pipeline, `glif=` kapısı

## Özet

`frame()` sınırı karakteri ve ön plan rengini geçirir, `bt-gpu` ikinci bir
pipeline ile glyph'leri arka planların üstüne çizer, duman sözleşmesi `glif=`
jetonu kazanır ve belgeler aynı commit'te güncellenir. Yazılan görünür olur.

_Requirements: R2, R2.1, R2.2, R2.3, R2.4, R4, R4.1, R4.2, R6, R7_

Atomik ve geri alması en pahalı phase. **Uzarsa dikiş**: önce genişlemiş
sink'i renderer glyph verisini *yoksayarak* al (görünmez, yeşil, `hucre=`
sabit), sonra pipeline'ı ekle.

---

## 1. `frame()` sınırı genişler

`crates/bt-core/src/session.rs`

Karar 1(a): tek zengin tip, tek sink.

```rust
/// Çizilecek tek hücre.
pub struct Cell {
    pub col: u16,
    pub row: u16,
    pub ch: char,
    /// Ön plan, **lineer** RGBA (bkz. `color::lineer_rgba`).
    pub fg: [f32; 4],
    /// `None` = varsayılan arka plan, çizilmez. `Frame::push` yalnız `Some`
    /// gördüğünde `bg_count`'u artırır: `hucre=K` jetonunun anlamı bit bit
    /// korunur ve `sabit_shell_arka_plan_hucreleri_verir` oynamaz.
    pub bg: Option<[f32; 4]>,
}
```

- `Flags::WIDE_CHAR_SPACER` hücreleri **elenir** — elenmezse CJK satırlarında
  ikinci hücreye hayalet glyph çizilir.
- Biçim bayrakları geçmez (R2.3): `INVERSE`/`DIM` zaten renge çözülüyor,
  `BOLD`/`ITALIC`/`UNDERLINE`/`STRIKEOUT` 004'ün işi. Alacritty'nin `Flags`'i
  hiçbir hâlde yeniden ihraç edilmez.
- Atlama koşulu `bg.is_none() && ch == ' '`. Koşul biçim bayrakları gelince
  onları da elemeli — yorumla bağlanır, yoksa altı çizili boşluklar sessizce
  kaybolur.

`CellBg` adı ölür; `bt-gpu`'nun `Frame::push_bg`'si `push` olur.

**Phase-1'den devir — uzayı tipe yaz.** `/code-review` phase-1'de şunu buldu:
`bt-gpu` sRGB olmayan hedefi *temsil edilemez* hâle getirdi (`PIXEL_FORMAT`
`const`), ama simetrik hata `bt-core` tarafında hâlâ tamamen temsil edilebilir
— `CellBg.rgba` `pub` bir `[f32; 4]` ve uzayı yalnız bir yorum söylüyor.
Sınır zaten bu phase'de yeniden yazıldığı için newtype'ın yeri burası:

```rust
/// Lineer RGBA; tek kurucusu paletin dönüşümü.
pub struct LinearRgba([f32; 4]);
```

`fg` ve `bg` bunu taşır, `DEFAULT_BG`/`DEFAULT_CURSOR` tip değiştirir,
`bt-gpu` `.0` ile okur. Maliyeti bu phase'de birkaç satır; ertelenirse
`Cell`'in üçüncü yazımı olur.

---

## 2. `cell` pipeline

`crates/bt-gpu/shaders/cell.metal` + `crates/bt-gpu/src/frame.rs`

Ayrı `.metal`, ayrı `#[repr(C)]` instance, **iki taraflı** assert çifti
(`static_assert` / `offset_of!`) — `cell_bg`'deki örüntünün birebir eşi.

```
GlyphInstance { pos: float2, size: float2, uv0: float2, rgba: float4 }
```

`build.rs` değişmez: dizini tarayıp her `.metal`'i linkliyor. `make shader`
kanaryası bu phase'de **zorunlu**.

- blend: `src_alpha` / `one_minus_src_alpha`, sRGB hedefte lineer uzayda
  (phase-1'in sebebi tam olarak bu)
- **shader'a gamma düzeltmesi YAZILMAZ.** Kodlamayı ROP yapıyor; `cell.metal`
  içine bir `pow(c, 1/2.2)` eklemek paleti **iki kez** kodlar. `cell_bg.metal`
  ve `frame.rs::Instance` bu uyarıyı taşıyor, yenisi de taşımalı
- atlas `R8Unorm`, `sample(atlas, uv).r` ile alfa; renk `rgba`'dan
- iki pipeline **tek render pass'te**, `viewport_px` uniform'u ortak

**Çizim sırası (R4.1):** `cell_bg` (arka planlar **+ imleç**) → `cell`
(glyph'ler üstte). Bugün imleç `Frame`'in sonunda ve opak; glyph gelince
imlecin altındaki harfi örterdi — 003'ün çözdüğü şikâyetin küçük bir kopyası.

Atlas dokusunun sahibi `Renderer`: `bt-atlas`'ın verdiği bitmap `replaceRegion`
ile yüklenir.

---

## 3. `glif=` jetonu ve kapı

`crates/bt-gpu`, `crates/bt-shell/src/app.rs`, `Makefile`, `proje.md`

Duman sözleşmesi jeton **ekler**: `kare=N hucre=K glif=G pipeline=ok`.
Jetonlar silinmez, eklenir — okuyan taraf tanımadığını atlar.

Bu süs değil kapının kendisi: boş bir atlas ve hiç çizmeyen bir glyph yoluyla
`kare=1 hucre=8 pipeline=ok` yine basılır ve `make duman` yeşil geçerdi. Kapı
`G > 0` de sorar.

`G`'nin sahibi `bt_core::smoke_shell`: `" bateri "` → **6** boşluksuz glyph.
Sayı `hucre=8`in bağlandığı gibi bir `bt-core` sınamasına bağlanır.

**GPU tarafı kanıtı, tam bayt assert etmeden:** offscreen sınamasında bir
hücrenin içinin arka planla **tekdüze olmadığı** doğrulanır. Tam bayt aransaydı
kapı sistem fontunun sürümüne rehin olurdu.

---

## 4. Belgeler (aynı commit)

- `CLAUDE.md`: `bt-atlas` "hâlâ boş" cümlesi ölür; katman tablosunda
  `core-text`/`core-graphics` → `objc2-core-text`/`objc2-core-graphics`
  (+ `objc2-core-foundation`); jeton listesine `glif=`; "bugün ekranda yalnız
  renkli hücreler var" cümlesi ölür
- `.claude/is-akisi/proje.md`: `duman` satırı `glif=` ve `G > 0` kapısını anlatır
- `bt-atlas/src/lib.rs` başlık yorumu: "003+" gelecek zamanı ölür

---

## Phase-3'ten devir

Phase-3'ün kalite kapısı (irtifa merceği + `/code-review`) dört bulgu üretti;
hiçbiri phase-3'te uygulanamadı çünkü dördü de **atlas dokusunun doğduğu** ana
bağlı.

**1. `Atlas::ensure`'ün dönüşü bu phase'de yakalanmak zorunda.**
`Renderer::cell_metrics` içindeki `let _ = atlas.ensure(PUNTO, scale)` bugün
sinyali atıyor: "atlası yeniden kurdum, dokuyu da yeniden ayır". Bugün ayrılacak
doku yok. Bu phase'de doku doğuyor ve sinyal kaçarsa belirti **sessiz**: ızgara
geometrisi değişmiş bir atlastan bayat bir yuva okumak, yuva aralık içinde
kaldığı sürece `slot_origin`'in savunmasına takılmaz ve **başka bir glyph**
çizer. Belirti "harici ekranı çıkardım, harfler karıştı" ve yalnız iki ekranlı
makinede görünür.

Phase-3 `ensure`'ü `#[must_use]` işaretledi ama **bu satır o korumanın dışında
kalıyor** ve bunu bilerek bilmek gerek: `let _ = expr;` rustc'nin kabul ettiği
susturma biçimidir, yani `cell_metrics` derlenirken hiçbir uyarı çıkmaz.
`#[must_use]` bu phase'in **yeni** `ensure` çağrılarında durdurur; mevcut
satırı bu phase elle ele almak zorunda. İlk iş: `cell_metrics`'in `let _`'sini
dokuyu yeniden ayıran yola bağla ya da sinyali oradan taşıyacak bir alan
kur — "işaretliydi, görürüz" yanlış bir güvendi.

**2. Atlası ödünç alan tek yer `Renderer::draw` olsun.**
`Renderer.atlas` bir `RefCell`. `link.rs:260` şu örüntüyü öğretiyor:
`iv.frame.borrow_mut()` alınıyor ve `renderer.draw(..., &frame, ...)` çağrısı
**boyunca tutuluyor**. Aynı şekil atlas için kopyalanırsa — yuva çözümünü
`session.frame(...)` sink'inde hoist et (sink hücre başına koştuğu için doğal
refleks bu), sonra o guard canlıyken `draw`'u çağır, `draw` da `slot_origin`
için atlası ödünç alsın — ilk glyph'li karede `BorrowMutError`. Çizim yolunda
ve `Retry` `GpuError` için tasarlandı, unwind için değil.
Çözüm: `Frame` `char` taşısın, `char → yuva → uv` çözümü `draw` içinde **tek
bir** `borrow_mut` altında yapılsın. Üç şey bedavaya gelir: yeniden-ödünç
temsil edilemez olur; bütün kare tek atlas kuşağıyla çizilir (1. maddedeki
bayat-yuva tehlikesi kuşak sayacı gerektirmeden yok olur); `link.rs`'in sink'i
renderer durumundan uzak kalır.

**3. Atlas ölçeği koşulsuz, `cell_px` koşullu uygulanıyor.**
`geometriyi_esitle` her çağrıda `cell_metrics(scale)` çağırıyor ve o çağrı
atlası **koşulsuz** yeni ölçeğe geçiriyor; `DisplayLink::resize` ise
`iv.cell` alanını yalnız `Session::resize` kabul ederse yazıyor. Bugün ikisi
ayrışsa da zarar yok: reddin iki sebebi var, "dejenere boyut" (o durumda
çizilecek hücre de yok) ve "hiç değişmedi" (o durumda ölçü zaten aynı), ve
bir sonraki geometri olayı ikisini eşitliyor. Doku gelince ayrışma
**ucuz olmaktan çıkar**: @2x rasterize edilmiş bir glyph @1x yuvaya blit
edilir. Bu phase atlas kuşağını çizim tarafında kabul edilen ölçüyle
eşlemeli — 2. maddedeki "tek `borrow_mut`, tek kuşak" çözümü bunu da kapatır.

**4. "Önce metriği sor" sözleşmesi ve GPU kapısının kendi tuzağı.**
`Renderer.atlas` artık `Option` ve `None` doğuyor: metriği hiç sormadan atlası
okuyan bir yol sessizce @1x çizmek yerine "atlas yok" durumuyla karşılaşır.
Bu phase'in glyph kapısı (`hücre içi arka planla tekdüze değil`, §3) tam da
böyle bir yolda koşacak: `cell_bg_pikseli_gpu_tarafinda_boyar` bir `Renderer`
kurup `cell_metrics`'i **hiç çağırmıyor**. Yeni sınama da öyle kurulursa atlas
`None` kalır; kapı ya düşer ya da (eski tasarımda olacağı gibi) @1x atlasla
yeşil geçerdi. Sınama ölçeği açıkça söylemeli.
Ölçeğin `bt-gpu`'ya **iki kapısı** olduğu da kayda geçsin: `Surface::set_size`
ve `cell_metrics`, `app.rs`'te komşu iki satır. Bugün uyuşuyorlar çünkü öyle
yazıldı, kurgu gereği değil. Birleştirmek (`Renderer::resize(surface, w, h,
scale) -> CellMetrics`) R5'in harfiyle çelişir; bu sette yapılmadı, doku
geldiğinde yeniden bakılır.

---

## Uygulama Notları

## Yayın Etkisi

---

## Checklist

- [ ] `bt-core`: `CellBg` → `Cell`, `bg: Option`, `WIDE_CHAR_SPACER` elenir
- [ ] `cell.metal` + `GlyphInstance` + iki taraflı assert çifti
- [ ] `Frame`: glyph listesi, `push_bg` → `push`, imleç `cell_bg`'de kalır
- [ ] Renderer: atlas dokusu, `replaceRegion`, iki pipeline tek pass
- [ ] `glif=` jetonu, `make duman` kapısı `G > 0`
- [ ] Test: `smoke_shell` altı glyph verir (`bt-core`)
- [ ] Test: offscreen — hücre içi arka planla tekdüze değil (`bt-gpu`)
- [ ] Belgeler: `CLAUDE.md`, `proje.md`, `bt-atlas/lib.rs`
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make hepsi` + `make shader` + `make duman`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
