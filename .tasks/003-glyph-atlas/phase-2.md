# Phase 2 — `bt-atlas`: CoreText raster ve yuva ızgarası

## Özet

`bt-atlas` boş olmaktan çıkar: font zincirini kurar, metriği hesaplar,
glyph'leri CPU bitmap'e rasterize eder ve sabit yuva ızgarasında adresler.
Metal görmez, doku üretmez; ekranda hiçbir değişiklik olmaz.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4, R1.5_

Bu phase saf `cargo test -p bt-atlas` ile doğrulanır ve `make duman`'a sıfır
risk taşır. Yarım kalırsa `bt-gpu`'nun kullanmadığı bir bağımlılık kenarıyla
yaşar — bugün de öyle yaşıyor.

---

## 1. Bağımlılıklar

`Cargo.toml` (workspace) + `crates/bt-atlas/Cargo.toml`

Karar kaydı `discussion.md → ## Karar`: `objc2-core-text` + `objc2-core-graphics`
(0.3 serisi), servo ailesi (`core-text` 22) reddedildi. Workspace zaten `objc2`
0.6 ailesini pinliyor; tek FFI yığını, tek sürüm politikası.

`objc2-core-foundation` üçüncü bir satır olarak gerekiyor: `CFString`
(font adı) ve `CFRetained` ikisinin de ortak tabanı ve zaten grafta —
`Cargo.lock`'a yeni bir crate girmiyor, `bt-atlas`'ın listesine kenar
ekleniyor. Feature'lar üye başına kırpılır (`default-features = false`),
`CLAUDE.md`'nin katman tablosundaki `core-text`/`core-graphics` adları bu
phase'de değil **phase-4'te** (belge commit'i) güncellenir — burada crate hâlâ
tek başına.

---

## 2. Font zinciri ve metrik

`crates/bt-atlas/src/font.rs`

SF Mono → Menlo. CoreText font bulunamadığında **hata vermez, ikame eder**:
istenen ad ile `CTFontCopyFamilyName`'in dönen adı karşılaştırılır, ayrışma tek
satır uyarı basar (stderr, Türkçe — süreç çıktısı, UI dizgisi değil).

```rust
/// Hücre ölçüsü. `scale` **anahtarın parçası**: @1x'te rasterize edilmiş
/// glyph @2x'te hatasız bulanıklaşır ve belirti yalnız iki ekranlı makinede
/// görünür (bkz. discussion.md → Muhakeme).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub cell_px: (u16, u16),
    /// Hücrenin üstünden taban çizgisine piksel; glyph buradan çizilir.
    pub baseline_px: u16,
}
```

- genişlik = `CTFontGetAdvancesForGlyphs` (tek boşluk glyph'i), yukarı yuvarlanır
- yükseklik = `ascent + descent + leading`, yukarı yuvarlanır
- `baseline_px` = `ascent` yuvarlanır

Punto ve ölçek `Atlas::new(punto, scale)`'e girer; punto bu sette sabit
(varsayılan 13.0), `family` ayarı yok — ayar dosyası 00X.

---

## 3. Rasterizasyon

`crates/bt-atlas/src/raster.rs`

Grayscale AA, tek kanal (karar 3a). Alfa-only `CGBitmapContext`
(`kCGImageAlphaOnly`, 8 bpp, satır adımı = `cell_w`) → çizilen beyaz glyph'in
kapsama değeri doğrudan alfa baytı olur; ayrı bir kanal ayıklama adımı yok.
`CGContextSetShouldSmoothFonts(false)`: subpixel AA kapalı, macOS 10.14'ten
beri sistemin kendisi de bırakmış durumda.

CG'nin başlangıcı sol **alt**: taban çizgisi `y = descent`.

Glyph araması `CTFontGetGlyphsForCharacters` (UTF-16). Glyph bulunamayan
karakter (`.notdef`) **tofu'ya düşer** — kendi kutusu zaten yuva 0'da.

Bitmap her zaman tam olarak `cell_w × cell_h` bayt: sabit yuva ızgarasının
karşılığı budur. Hücre kutusunu taşan glyph (uzun descender, italik) **kırpılır**
ve bu bilinçli — kutu çizim karakterleri ve emoji zaten kapsam dışı.

---

## 4. Atlas: sabit yuva ızgarası

`crates/bt-atlas/src/lib.rs`

Paketleyici yok, tahliye yok (karar 5a + Muhakeme). Tüm glyph'ler hücre
boyutunda olduğu için `yuva_no → uv` **aritmetiktir**.

```rust
pub struct Atlas { /* font, metrics, yuvalar: HashMap<char, u16>, sonraki: u16, bitmap tamponu */ }

/// Yuva 0 **rezident tofu**: dolu atlasta ve `.notdef`'te buraya düşülür.
/// Sessiz kayıp (glyph hiç çizilmez) yerine görünür kayıp (kutu çizilir).
pub const TOFU: u16 = 0;

impl Atlas {
    pub fn new(punto: f64, scale: f64) -> Self;
    pub fn metrics(&self) -> Metrics;
    /// Atlas dokusunun piksel boyutu; `bt-gpu` dokuyu buna göre ayırır.
    pub fn texture_px(&self) -> (u16, u16);
    /// Yuvanın doku içindeki sol üst köşesi (piksel). uv aritmetiği çağıranın.
    pub fn slot_origin(&self, slot: u16) -> (u16, u16);
    /// Karakterin yuvası. Yuva **yeni açıldıysa** ikinci değer o yuvaya
    /// yazılacak `cell_w × cell_h` baytlık R8 bitmap'idir; `bt-gpu`
    /// `replaceRegion` ile yükler. Doku `bt-atlas`'ın işi değil (lib.rs
    /// sözleşmesi: "Metal görmez").
    pub fn slot(&mut self, ch: char) -> (u16, Option<&[u8]>);
    /// (kullanılan, toplam) yuva — doluluk `/measure`'ın okuyacağı sayaç.
    pub fn doluluk(&self) -> (usize, usize);
}
```

Izgara boyutu: doku kenarı 1024 px hedeflenir, satır/sütun sayısı hücre
ölçüsünden türer (`1024 / cell_w`); toplam yuva sayısı ondan çıkar. Sayı
**ölçüm iddiası değil**, bir kapasite tercihi — doluluk `/measure` ile okunur.

---

## 5. Sınamalar

`cargo test -p bt-atlas`, GPU'suz:

- metrik makul aralıkta (genişlik > 0, yükseklik > genişlik, baseline < yükseklik)
- aynı karakter iki kez sorulunca **aynı yuva** ve ikincide `None` bitmap
- dolu atlas tofu verir (kapasiteyi aşana kadar farklı karakter sorulur)
- ikame uyarısı: var olmayan bir aile adı istendiğinde dönen ad ayrışır
- `scale` anahtarın parçası: 1.0 ve 2.0 ile kurulan iki atlas farklı `cell_px` verir

---

## Uygulama Notları

## Yayın Etkisi

---

## Checklist

- [ ] Bağımlılıklar workspace + `bt-atlas` `Cargo.toml`'a eklendi
- [ ] `font.rs`: zincir, ikame uyarısı, `Metrics`
- [ ] `raster.rs`: alfa-only `CGBitmapContext`, `CTFontDrawGlyphs`
- [ ] `lib.rs`: `Atlas`, sabit yuva ızgarası, rezident tofu, `doluluk()`
- [ ] Test: metrik / aynı yuva / dolu atlas tofu / ikame / ölçek anahtarı
- [ ] Doğrulama geçti (`proje.md` → Doğrulama; `make hepsi`)
- [ ] `/simplify` çalıştırıldı, bulgular uygulandı
- [ ] `/code-review` çalıştırıldı, bulgular giderildi
- [ ] `/audit` çalıştırıldı, bulgular giderildi
- [ ] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
