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
