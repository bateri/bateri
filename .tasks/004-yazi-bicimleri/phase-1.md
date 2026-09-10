# Phase 1 — `bt-atlas`: dört yüz ve kural sprite'ları

## Özet

Atlas dört font yüzü tanır ve kural çizgilerini yordamsal sprite olarak
rasterize eder; yuva anahtarı `(Sprite, Face)` olur ve `Metrics` kural
zarfını taşır. Ekranda hiçbir değişiklik yok — `bt-gpu` henüz `Face::Regular`
geçiyor.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4, R2, R2.1, R2.2, R2.3, R2.4_

**Neden bölünmedi:** yüzler ve sprite'lar aynı yuva anahtarını değiştiriyor.
Ayrı commit'ler `HashMap`'in anahtar tipini iki kez yazardı; ikincisi
birincinin sınamalarını da yeniden yazmak zorunda kalırdı.

---

## 1. Manifest: `CTFontTraits` feature'ı

`Cargo.toml` (kök, workspace)

`copy_with_symbolic_traits` ve `CTFontSymbolicTraits` bu feature'ın arkasında;
bugün açık değil.

```toml
objc2-core-text = { version = "0.3", default-features = false, features = [
    "std", "CTFont", "CTFontDescriptor", "CTFontTraits", "objc2-core-graphics",
] }
```

Yeni **crate** değil, yeni bir feature satırı. `Cargo.lock` oynarsa
`## Yayın Etkisi`'ne yazılır ve bilinçli karardır.

---

## 2. `Face` ve dört yüzün açılması

`crates/bt-atlas/src/font.rs`

```rust
/// Font yüzü — **tipografi kavramı**, SGR bayrağı değil.
///
/// `bt-core`'un `bold`/`italic` bayraklarıyla dört varyantı aynı, sebepleri
/// ayrı: oradaki terminal semantiği, buradaki CoreText trait'i. İkisini
/// birleştirmek `bt-atlas`'a `bt-core` kenarı eklemek demek olurdu ve o kenar
/// `alacritty_terminal`'i saf-CoreText crate'ine çeker. Çeviri `bt-gpu`'da,
/// ikisini birden gören tek katmanda.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum Face {
    #[default]
    Regular,
    Bold,
    Italic,
    BoldItalic,
}
```

Dört yüz düz yüzden türer. **Denetim iki kapılı ve aile adı karşılaştırması
YAPILMAZ** — API'nin sözleşmesi zaten "aynı ailede font, yoksa NULL", yani
`Menlo-Bold`'un ailesi `Menlo`'dur ve karşılaştırma totolojidir. Asıl risk
CoreText'in **düz yüzü geri vermesi**; onu ancak dönen fontun trait'lerine
bakarak görürsün.

```rust
/// `duz`den `face`in yüzünü türetir; edinemezse `None`.
fn yuz_turet(duz: &CTFont, face: Face) -> Option<CFRetained<CTFont>> {
    let istenen = match face {
        Face::Regular => return None, // çağıran düz yüzü zaten elinde tutuyor
        Face::Bold => CTFontSymbolicTraits::TraitBold,
        Face::Italic => CTFontSymbolicTraits::TraitItalic,
        Face::BoldItalic => CTFontSymbolicTraits::TraitBold
            | CTFontSymbolicTraits::TraitItalic,
    };
    // Birinci kapı: nil. `Option` sayesinde tipte.
    let font = unsafe { duz.copy_with_symbolic_traits(0.0, ptr::null(), istenen, istenen) }?;
    // İkinci kapı: gerçekten edindi mi? CoreText istenen yüzü bulamazsa düz
    // yüzü geri verebiliyor ve o sessiz ikame, `zincirden_ac`'ın
    // `CTFontCreateWithName` için yaşadığı hatanın ta kendisi.
    let donen = unsafe { font.symbolic_traits() };
    donen.contains(istenen).then_some(font)
}
```

Atlas kurulurken dört yüz bir kez açılır; bulunamayan yüz **düz yüze geri
düşer** ve uyarı **bir kez** basılır (kare başına değil — `slot` çizim
yolunda):

```rust
pub(crate) struct Yuzler([CFRetained<CTFont>; 4]);

impl Yuzler {
    pub(crate) fn ac(punto: CGFloat) -> Self { /* zincirden_ac + üç yuz_turet */ }
    pub(crate) fn get(&self, face: Face) -> &CTFont { &self.0[face as usize] }
}
```

Geri düşen yüz için stderr, `zincirden_ac`'ınkiyle **aynı önekle**
(`bateri:` — ayrı bir önek, `bateri` diye süzen okuyucunun tam da bu satırı
kaçırması demek olurdu).

---

## 3. `Metrics` kural zarfını taşır ve **burada** kırpılır

`crates/bt-atlas/src/font.rs`

```rust
pub struct Metrics {
    pub cell_px: (u16, u16),
    pub baseline_px: u16,
    /// Alt çizgi: (hücrenin üstünden konum, kalınlık).
    pub underline_px: (u16, u16),
    /// Üstü çizili: (konum, kalınlık).
    pub strikeout_px: (u16, u16),
    /// Kıvrımın tepeden tepeye genliği, piksel.
    pub curl_amp_px: u16,
}
```

Kaynaklar: `CTFontGetUnderlinePosition` (**negatif** = taban çizgisinin
altında) ve `CTFontGetUnderlineThickness`. Üstü çizili için CoreText API'si
**yok** — `CTFontGetXHeight`/2 kadar taban çizgisinin üstü kullanılır; kıvrım
genliğinin de API'si yok, kalınlıktan türeyen bir tasarım sabitidir.

**Üçü de `metrics()` içinde hücreye kırpılır** ve bu tesadüf değil:
`slot_bytes`'ın doc'u (`font.rs:42-45`) yuva geometrisinin tek sahibinin
burası olduğunu söylüyor. Kırpılmazsa küçük descent'li bir fontta çizgi
`cell_px.1`'in dışına düşer, komşu satıra taşar ve **belirti sessizdir**:

```rust
// Konum + kalınlık (kıvrımda + genlik) hücrenin içinde kalmalı.
let en_alt = cell_px.1.saturating_sub(1);
let konum = (taban + ofset).min(en_alt.saturating_sub(kalinlik));
```

`descender_hucreye_sigar` sınamasının kardeşi gerekiyor:
`kural_hucreye_sigar` — üç zarfın da `cell_px.1`'i aşmadığını, metriği
fontun kendisinden okuyarak doğrular (sayı yazmaz, iddia eskimez).

---

## 4. Kural sprite'larının yordamsal rasteri

`crates/bt-atlas/src/raster.rs`

```rust
/// Kural çizgisi çeşidi — atlasta karakter gibi yuva tutar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuleKind { Single, Double, Curl, Dotted, Dashed, Strike }

/// `hedef`e kural çizgisinin kapsama baytlarını çizer.
///
/// `ciz`'in kardeşi ve **aynı bağlamı** kullanıyor: alfa-only
/// `CGBitmapContext`, renk uzayı yok, kapsama doğrudan alfa baytı. Font
/// görmüyor — `tofu_tamponu`'nun yaptığı işin genellemesi.
pub(crate) fn ciz_kural(kind: RuleKind, m: Metrics, hedef: &mut [u8]) -> Cizim
```

**R2.4 — periyot kısıtı, kurulurken sağlanır:** dalganın ve noktalı/kesikli
desenin periyodu `cell_px.0`'ı **tam bölmeli**. Bölmezse yan yana iki hücre
sınırında faz kırılır ve çok hücreli bir alt çizgi kesintili görünür. Sprite
tek hücre genişliğinde ve komşularıyla döşeniyor; süreklilik kendiliğinden
gelmez.

```rust
// Periyot hücre genişliğini tam bölen en büyük değer: bir hücrede tam
// `n` dalga. `n = 1` en yumuşak; hücre dar olduğunda desen sıkışır.
let periyot = f64::from(m.cell_px.0) / f64::from(DALGA_SAYISI);
```

Bekçisi: `kivrim_hucre_sinirinda_sureklidir` — sprite'ın sol ve sağ kenar
sütunlarının kapsama değerlerinin eşleştiğini assert eder. Faz kırılırsa
kırmızı düşer.

---

## 5. Yuva anahtarı `(Sprite, Face)`

`crates/bt-atlas/src/lib.rs`

```rust
/// Atlasta yuva tutan şey: bir karakter ya da bir kural çizgisi.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sprite { Char(char), Rule(RuleKind) }

// yuvalar: HashMap<(Sprite, Face), u16>
pub fn slot(&mut self, sprite: Sprite, face: Face) -> (u16, Option<Upload<'_>>)
```

Kurallar yüzden bağımsız; çağıran onları **her zaman** `Face::Regular` ile
sorar (kalın metnin altındaki çizgi kalın değildir).

`slot`'un gövdesi tek yerde ayrışır — raster çağrısı:

```rust
let cizim = match sprite {
    Sprite::Char(ch) => raster::ciz(self.yuzler.get(face), ch, self.metrics, &mut self.tampon),
    Sprite::Rule(kind) => raster::ciz_kural(kind, self.metrics, &mut self.tampon),
};
```

**Negatif önbellek olduğu gibi kalıyor.** `Cizim::GlifYok` kurallarda hiç
doğmaz (yordamsal çizim fontu sormuyor), `BaglamYok` doğabilir ve zaten
kalıcı. `retain(|_, &mut yuva| yuva != TOFU)` tahliyesi anahtar tipinden
bağımsız, dokunulmuyor.

---

## 6. Modül doc'unun kapsam cümlesi

`crates/bt-atlas/src/lib.rs:10-11`

Bugün *"Kutu çizim karakterleri, emoji ve font seti ayarı kapsam dışı …
ikinci font yüzü isteyen `BOLD`/`ITALIC` 004'ün"* diyor. İkinci yarısı bu
commit'te yanlışa düşüyor — kodla çelişen cümle aynı commit'te düzelir.
(`CLAUDE.md`'nin aynı iddiayı taşıyan cümlesi phase-3'te, belgelerle birlikte.)

---

## Uygulama Notları

<!-- /implement doldurur. -->

## Yayın Etkisi

- **shader** — yok, `.metal` el değmiyor.
- **terminfo / `TERM`** — yok.
- **ayar şeması** — yok (`settings.toml` modeli henüz yok).
- **tema / materyal** — yok.
- **shell entegrasyonu** — yok.
- **app bundle** — yok.
- **yeni bağımlılık** — yeni **crate** yok; kök `Cargo.toml`'a
  `CTFontTraits` feature satırı eklendi. `Cargo.lock` oynadıysa bu satırdan
  ötürüdür ve kararı `discussion.md → Karar 5`'te.
- **belge** — `crates/bt-atlas/src/lib.rs` modül doc'unun kapsam cümlesi
  (madde 6). `CLAUDE.md` phase-3'te.
- **ölçüm bekliyor:** dört yüzün atlas kurulumundaki ana thread bedeli —
  `Atlas::new` artık bir yerine dört `CTFont` açıyor ve `ensure()` her ölçek
  değişiminde `*self = Self::new(...)` yapıyor. Bu, 003 `teslim.md`
  B.1 **#4**'ün genişlemesidir, yeni bir liste değil. Kancası
  (`BT_STARTUP_TRACE`) hâlâ yok; `/measure` bugün yine "ölçüm aracı yok" der.
- **ölçüm bekliyor:** yüz başına ayrı yuva tutulmasının atlas doluluğuna
  etkisi — 003 B.1 **#3**'ün genişlemesi. Aritmetik (kapasite
  `(1024/w)×(1024/h)`, aynı karakter yüz başına ayrı yuva) plana yazıldı,
  **sayı yazılmadı**; tahliye yok, dolan atlas görünür `TOFU`'ya düşer.

---

## Checklist

- [ ] `Cargo.toml`: `CTFontTraits` feature'ı
- [ ] `Face` enum'ı + `Yuzler`: dört yüz, `symbolic_traits()` kesişimi, düz yüze geri düşüş, **tek seferlik** uyarı
- [ ] `Metrics`: `underline_px`, `strikeout_px`, `curl_amp_px` — üçü de `metrics()` içinde hücreye kırpılır
- [ ] `RuleKind` + `raster::ciz_kural`: altı çeşit, alfa-only bağlam, fontsuz
- [ ] `Sprite` enum'ı + `HashMap<(Sprite, Face), u16>` + `slot(sprite, face)`
- [ ] `lib.rs` modül doc'unun kapsam cümlesi düzeltildi
- [ ] Test: `kalin_yuz_ayri_yuva_alir` — `slot(Char('M'), Bold) != slot(Char('M'), Regular)`
- [ ] Test: `bulunamayan_yuz_duze_duser` — trait edinilemezse düz yüzün fontu, tofu değil
- [ ] Test: `kural_hucreye_sigar` — üç zarf da `cell_px.1`'i aşmıyor (metrik fonttan okunur, sayı yazılmaz)
- [ ] Test: `kivrim_hucre_sinirinda_sureklidir` — sprite'ın sol/sağ kenar sütunları eşleşiyor
- [ ] Test: `kural_sprite_bos_degil` — altı çeşit birbirinden ve boş yuvadan farklı
- [ ] Doğrulama geçti (`make hepsi`; `make shader` `[~]` — `.metal` ve `build.rs` el değmedi; `make duman` bu phase'de değişmiyor)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
