# Phase 3 — `bt-gpu`: yüzler ve kurallar çizilir

## Özet

`bt-gpu` `(bold, italic)`'i `Face`'e çevirir, kural çizgilerini `GlyphInstance`
olarak glyph'lerden sonra çizer, duman sözleşmesine `kural=R` jetonunu ekler.
Ekranda görünen değişiklik burada.

_Requirements: R4, R4.1, R4.2, R4.3, R4.4, R5, R5.3, R5.4, R6_

**Geri alması en pahalı phase.** Uzarsa dikiş: önce yüz çevirisini al
(kalın/eğik görünür, kurallar hâlâ yok, `kural=` jetonu girmemiş — `make duman`
yeşil kalır), sonra kuralları.

---

## 1. `Face` çevirisi — **burası, çünkü tek yer**

`crates/bt-gpu/src/frame.rs`

`bt-atlas` `bt-core`'u görmüyor ve görmemeli: o kenar `alacritty_terminal`'i
saf-CoreText crate'ine çekerdi (`CLAUDE.md` → "bağımlılık mimari karardır").
`bt-gpu` ikisini birden gören **tek** katman.

```rust
// bt_core::{bold, italic} bir SGR bayrağı; bt_atlas::Face bir font yüzü.
// Dört varyantları aynı, SEBEPLERİ AYRI — ikisi bilerek iki tiptir ve
// "aynı görünüyorlar" diye birleştirilirse katman yönü ters döner:
// birleşik tip ya bt-core'a girer (bt-atlas onu göremez) ya bt-atlas'a
// (bt-core onu göremez). Çeviri burada kalmalı.
fn yuz(bold: bool, italic: bool) -> Face {
    match (bold, italic) {
        (false, false) => Face::Regular,
        (true, false) => Face::Bold,
        (false, true) => Face::Italic,
        (true, true) => Face::BoldItalic,
    }
}
```

`GlyphCell` yüzü taşır; `GlyphInstance` **taşımaz** (uv0 zaten yuvayı, yuva
da yüzü kodluyor):

```rust
pub(crate) struct GlyphCell {
    pub(crate) pos: [f32; 2],
    pub(crate) ch: char,
    pub(crate) face: Face,
    pub(crate) rgba: [f32; 4],
}
```

---

## 2. Kural listesi

`crates/bt-gpu/src/frame.rs`

**Yeni pipeline, yeni shader, yeni instance tipi YOK.** `cell` pipeline'ı
zaten genel bir kapsama maskesi çizicisi — kodun kendi yorumu
(`cell.metal:55-56`): *"Renk instance'tan gelir, dokudan değil — atlas glyph
başına bir maske tutuyor, bir görüntü değil."* Kural sprite'ı da hücre boyunda
bir maske; `GlyphInstance`'ın `size`'ı zaten yok, dörtlü tam bir hücre.

```rust
/// Kural çizgisi; `GlyphCell`'in kardeşi ve aynı gerekçeyle metriksiz —
/// yuva çözümü atlas ödüncünün yaşadığı yerde (`encode_glyphs`) yapılıyor.
pub(crate) struct RuleCell {
    pub(crate) pos: [f32; 2],
    pub(crate) kind: RuleKind,
    pub(crate) rgba: [f32; 4],
}

pub(crate) struct Frame {
    bg: Vec<Instance>,
    glyphs: Vec<GlyphCell>,
    rules: Vec<RuleCell>,      // yeni
    cell_px: (f32, f32),
    bg_count: usize,
}
```

`push()` bir hücre için **ikiye kadar** kural üretir (alt çizgi + üstü
çizili); rengi `underline_color` varsa o, yoksa `fg`:

```rust
let kural_rgba = cell.underline_color.unwrap_or(cell.fg).to_array();
if let Some(kind) = rule_kind(cell.underline) {
    self.rules.push(RuleCell { pos, kind, rgba: kural_rgba });
}
if cell.strikeout {
    // Üstü çizili SGR 58'i kullanmaz — SGR'de karşılığı yok.
    self.rules.push(RuleCell { pos, kind: RuleKind::Strike, rgba: cell.fg.to_array() });
}
```

**`bg_count` el değmiyor** → `hucre=K` bit bit korunuyor ve `push`'taki
`debug_assert_eq!(bg.len(), bg_count, "arka plan imleçten sonra eklendi")`
bekçisi olduğu gibi kalıyor. Kural sayacı ayrı: `rule_count()`.

---

## 3. Encode sırası — imleç bedavaya çözülüyor

`crates/bt-gpu/src/renderer.rs` → `AtlasDoku::hazirla` / `encode_glyphs`

Kurallar glyph'lerden **sonra**, **aynı** instance tamponuna ve **aynı** draw
call'a girer:

```rust
// Tek liste, tek draw call: önce glyph'ler, sonra kurallar. Sıra bilerek —
// üstü çizili harfin ÜSTÜNDEN geçmeli.
for g in glyphs { /* slot(Sprite::Char(g.ch), g.face) → instances.push(...) */ }
for r in rules  { /* slot(Sprite::Rule(r.kind), Face::Regular) → instances.push(...) */ }
```

**İmleç sırası bedava:** glyph geçişi zaten arka planlardan **ve imleçten
sonra** kodlanıyor (`encode_bg(...).and_then(|()| encode_glyphs(...))`,
`renderer.rs:409-412`), imleç ise `bg` listesinin sonunda. Yani kural imlecin
üstüne düşüyor — ve orada rengi `bt-core`'un imleç için zaten tersine
çevirdiği ön plan (SGR 58 yoksa), yani görünür kalması da bedava. 003'ün
glyph için kurduğu mekanizmanın aynısı.

Kurallar **her zaman `Face::Regular`** ile sorulur: kalın metnin altındaki
çizgi kalın değildir.

---

## 4. Duman jetonu ve kapı

`crates/bt-shell/src/app.rs`

```
kare=N hucre=K glif=G kural=R pipeline=ok
```

Kapı `n > 0 && k > 0 && g > 0 && r > 0`. Jeton **bu phase'de** girer —
phase-2'de girseydi `R=0` olurdu ve set ortasında `make duman` kırmızıya
düşerdi. (003 `glif=`'i tam bu yüzden phase-4'te ekledi.)

Reçete phase-2'de yerleşti, yani `R` bu commit'te ilk kez sıfırdan büyük
okunuyor: sayısı `smoke_shell`'in yedi kural hücresinden gelir.

**Jetonun sınırı yoruma yazılır:** `kural=` setin yalnız **kural yarısını**
kapatır ve stil ayrımını göremez — `Face` her zaman `Regular` dönen ya da
kıvrımı düz çizen bir yapı da aynı `R`'yi basar. Yüz yarısının kapısı
phase-1'in birim sınamaları, stil ayrımının kapısı phase-2'nin
`sabit_shell_bes_stili_ayirt_eder`'i, kıvrımın gerçekten dalga olduğunun
kapısı aşağıdaki offscreen sınaması.

---

## 5. Offscreen sınama — kural bandı tekdüze değil

`crates/bt-gpu/src/renderer.rs` (`#[cfg(test)]`)

`glif_hucrenin_icini_arka_planindan_ayirir`'ın kardeşi, **tam bayt assert
etmeden** (yoksa kapı sistem fontunun sürümüne rehin olur):

- Kıvrımlı alt çizgili bir hücre çizilir, kural bandındaki satır okunur.
- Assert: band **x boyunca tekdüze değil** — düz çizgi tekdüzedir, dalga
  değildir. Kıvrımı düz çizgiye düşüren bir kod burada kırmızı düşer.
- İkinci assert: SGR 58 rengi verilen bir kuralın pikselleri ön plan
  renginden **farklı**.

---

## 6. Belgeler

Hepsi bu commit'te (kodla çelişen cümle kuralı):

| dosya | ne |
|---|---|
| `CLAUDE.md` | jeton listesine `kural=`; `make duman` satırı; **"kalanı 004'ün işi (…emoji, kutu çizim)"** cümlesi — emoji ve kutu çizim 004'te değil, ayrı setlerde |
| `Makefile` | `duman` hedefinin yorumu |
| `.claude/is-akisi/proje.md` | doğrulama tablosunun `duman` satırı |
| `crates/bt-gpu/src/renderer.rs` | atlas ödüncünün "tek yer" cümlesi hâlâ doğru mu (kural çözümü aynı fonksiyonda kalıyor, ama cümle okunup doğrulanır) |

---

## Uygulama Notları

<!-- /implement doldurur. -->

## Yayın Etkisi

- **shader** — **yok.** `.metal` dosyalarına dokunulmadı: kural sprite'ları
  var olan `cell` pipeline'ından geçiyor. `make shader` koşulu doğmuyor →
  `[~]`. (`discussion.md → Karar 4`, sprite rotası.)
- **terminfo / `TERM`** — yok.
- **ayar şeması** — yok.
- **tema / materyal** — yok.
- **shell entegrasyonu** — yok.
- **app bundle** — yok.
- **yeni bağımlılık** — yok.
- **belge** — madde 6'daki dört dosya.
- **duman sözleşmesi** — jeton **eklendi, silinmedi**: `kural=R`. Okuyan
  taraf tanımadığı jetonu atlar; geri alınırsa `kural=` kaybolur ve bu
  `teslim.md → Geri Alma`'ya yazılır.
- **ölçüm bekliyor:** kare başına kural instance'larının ve `slot()`'un
  ikinci çağrı sınıfının kare süresine etkisi — 003 `teslim.md` B.1
  **#2** ve **#5**'in genişlemesi. Kancası yok; `/measure` yine "ölçüm aracı
  yok" der.

---

## Checklist

- [ ] `yuz(bold, italic) -> Face` çevirisi + "iki tip bilerek ayrı" yorumu
- [ ] `GlyphCell` yüzü taşır; `GlyphInstance` **taşımaz**
- [ ] `RuleCell` + `Frame.rules` + `rule_count()`; `bg_count` el değmedi
- [ ] `push()` hücre başına ikiye kadar kural üretir; renk `underline_color ?? fg`, üstü çizili hep `fg`
- [ ] `encode_glyphs`: kurallar glyph'lerden **sonra**, aynı tamponda, aynı draw call'da, `Face::Regular` ile
- [ ] `app.rs`: `kural={r}` jetonu, kapı `r > 0`, jetonun sınırı yoruma yazıldı
- [ ] Test: `kural_bandi_x_boyunca_tekduze_degil` — kıvrım gerçekten dalga (offscreen)
- [ ] Test: `sgr58_rengi_on_plandan_farkli` (offscreen)
- [ ] Test: `kalin_ve_duz_ayri_cizilir` — aynı karakter iki yüzle iki farklı piksel kümesi verir (offscreen, `assert_ne!`)
- [ ] Test: `imlecin_ustundeki_kural_gorunur` — imleç hücresindeki kural imleç bloğuyla örtülmüyor
- [ ] Belgeler: `CLAUDE.md` (jeton + duman satırı + "004'ün işi" cümlesi), `Makefile`, `proje.md`
- [ ] **phase-2'den devir** (`/audit` mercek 9): `CLAUDE.md`'nin hücre maddesine "24 bayt **grid** hücresidir; `frame()` sınırının `Cell`'i ayrı bir kare kaydıdır" yan tümcesi. Sınır hücresi 004'te 5 alandan 10'a çıktı ve maddeyi okuyan "hücreye alan eklendi, yan tablo neden yok" diye okuyor; gerekçe `bt-core/src/lib.rs` ve `session.rs`'te yazılı ama `CLAUDE.md`'de değil
- [ ] **phase-2'den devir** (`/audit` mercek 10, bulgu değil not): `CLAUDE.md`'nin 003 anlatısındaki "`frame()` sınırı karakteri ve ön plan rengini geçiriyor" cümlesi artık eksik — sınır beş alan daha geçiriyor
- [ ] Doğrulama geçti (`make hepsi`; `make shader` `[~]` — `.metal` el değmedi; `make duman` → `kare=N hucre=8 glif=6 kural=R pipeline=ok`; `make test-yaris`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
