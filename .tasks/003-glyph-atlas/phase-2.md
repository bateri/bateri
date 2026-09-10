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

**1. `slot()`'un dönüşü bölünemedi — `Upload { origin, bytes }`.** Kılavuzun API
taslağı `slot(ch) -> (u16, Option<&[u8]>)` ile ayrı bir `slot_origin(slot)`
öngörüyordu. İkisi **aynı döngüde çağrılamıyor**: `slot` `&mut self`'ten türeyen
bir ödünç döndürüyor, `slot_origin` `&self` istiyor, `replaceRegion` ise ikisine
**birden** ihtiyaç duyuyor → `E0502`. Yani phase-4'ün yükleme döngüsü
derlenmeyecekti ve kaçış yolları da kötüydü (bitmap'i `Vec`'e kopyalamak —
`tampon` alanı tam olarak bunu önlemek için var — ya da ızgara aritmetiğini
`bt-gpu`'da yeniden yazmak). Köşe `Upload`'ın içine alındı. `slot_origin` `pub`
kaldı: uv aritmetiği için gerekli ve orada ödünç çoktan bitmiş oluyor.

**2. `Atlas::ensure(point_size, scale) -> bool` eklendi (taslakta yoktu).**
Kılavuz ölçeğin "anahtarın parçası" olmasını şart koşuyordu, ama anahtar
hiçbir yerde **saklanmıyordu**: `Atlas` ne kurulduğu ölçeği biliyordu ne de
soruyordu, yeniden kurma tamamen çağıranın hatırlamasına kalmıştı. `ensure`
anahtarı tipe yazıyor; `true` dönüşü aynı zamanda "dokuyu yeniden ayır"
demek, çünkü metrik değişince `texture_px()` de değişiyor ve eski boyutlu
dokuya yeni metrikle yazmak sessizce bozar. Phase-3'ün `RefCell<HashMap<ölçek,
Atlas>>` önerisi bununla gereksizleşti — phase-3.md'ye işlendi.

**3. Hücre yüksekliği iki parçanın AYRI yuvarlanmasıyla hesaplanıyor.**
Kılavuz "yükseklik = `ascent + descent + leading`, yukarı yuvarlanır" ve
"`baseline_px` = `ascent` yuvarlanır" diyordu; ikisi birlikte descender'ı
**kırpıyor**. Bu makinede Menlo 13pt ascent 12.067, descent 3.066 veriyor:
birleşik yuvarlama 16 ediyor, taban 13'e oturunca alta 3 piksel kalıyor, oysa
font 3.066 istiyor — `g j p q y ,` altındaki son kapsama satırı gidiyor ve üstte
0.93 piksel boşa duruyor. `yukari(ascent) + yukari(descent + leading)` ile
yükseklik 17 oluyor ve taban **inşa gereği** tam oturuyor. Bekçisi
`descender_hucreye_sigar`; 'W' ile sınamak yetmiyordu, descender'ı olmayan harf
iki yuvarlamada da aynı görünüyor.

**4. `.notdef` önbelleğe giriyor, dolu atlas girmiyor.** Kılavuz ikisini de
"tofu'ya düşer" diye tarif ediyordu ve ilk uygulama tek bir `||` ile
birleştirmişti. Görünür sonuçları aynı ama **ömürleri** farklı: fontun bir
karakteri tanımaması kalıcı bir gerçek (önbelleğe girer, aynı karakter bir daha
CoreText'e sorulmaz), atlasın dolu olması geçici bir hâl (girmez — tahliye
geldiğinde, 00X, yer açılmış olacak ve kayıt yalan söyleyecekti). `raster::ciz`
bu yüzden `bool` değil `Cizim` enum'u döndürüyor. `Cizim::BaglamYok` da kalıcı
sayılıyor: `CGBitmapContextCreate`'in karakterle ilgili tek argümanı yok, hepsi
atlas ömrü boyunca sabit — "bir sonraki karede düzelir" diye bir şey yok ve
önbelleğe girmeseydi her hücre her karede başarısız bir bağlam kurulumu öderdi.

**5. Negatif önbelleğin tavanı `kapasite()`.** Tofu'ya çözümlenen kayıt yuva
harcamıyor, yani `sonraki` onu sınırlamıyordu: bir ikili dosyayı `cat`'lemek
milyonlarca ayrı codepoint üretebilir ve harita sınırsız büyürdü. Crate'in
tavanı olmayan tek sayısı burasıydı.

**6. `point_size · scale` aralığa oturtuluyor (`etkin_punto`).** `yukari`'nin
`clamp(1.0, …)`'ı NaN'ı **geçiriyor** ve `NaN as u16` sıfır ediyor; sınır
konmasaydı `DOKU_KENARI / 0` panikleyecekti. Devasa punto ise yuva başına
gigabaytlık tampon ve Metal'in doku sınırını katbekat aşan bir `texture_px()`
isterdi. Bekçisi `bozuk_punto_atlasi_dusurmez`.

**7. `NonNull` işaretçileri dilimden türetiliyor, `&dizi[0]`'dan değil.** BMP
dışı bir karakterde `encode_utf16` iki birim üretiyor ve CoreText ikinci
elemana da dokunuyor (düşük vekili okur, karşılığına 0 yazar). Tek elemanlık
bir referanstan türeyen işaretçinin provenance'ı o erişimi kapsamıyor — bugün
çalışır, aliasing modeline göre tanımsızdır. Yol artık
`bmp_disi_karakter_yolu_calisir` ile koşuluyor.

**8. `objc2` çekirdeği hiç gerekmedi.** `bt-atlas` yalnız C API'si çağırıyor,
ObjC runtime'ı değil: `cargo tree -p bt-atlas` üç CF/CG crate'i gösteriyor,
`objc2`'yi göstermiyor. `CLAUDE.md`'nin katman tablosuna bu şekilde yazıldı.

**9. `CGBitmapContext` dördüncü bir feature olarak gerekti.** Kılavuz
`CGColorSpace` + `CGContext` + `CGImage` sayıyordu; `CGBitmapContextCreate`
kendi başlığının arkasında ve o feature olmadan crate kökünde görünmüyor.
Kırpma `objc2-io-surface`'i grafın dışında tuttu: `Cargo.lock`'a yalnız
`objc2-core-text` ve `objc2-core-graphics` girdi, `objc2-core-foundation`
iddia edildiği gibi zaten graftaydı.

**10. `CLAUDE.md` phase-4'te değil BU commit'te güncellendi.** Kılavuz belge
güncellemesini phase-4'e erteliyordu, ama `CLAUDE.md`'nin kendi kuralı
"buradaki bir cümle kodla çelişirse ikisinden biri **aynı commit'te** düzelir"
diyor ve üç cümle çelişiyordu: "`bt-atlas` hâlâ boş", katman tablosundaki
`core-text`/`core-graphics` adları, bağımlılık tabanındaki `core-text`. Depo
kuralı kılavuzu yendi. Phase-4'e kalan belge işi `glif=` jetonu ve `bt-atlas`
satırının "bağlandı" hâline gelmesi.

**11. SF Mono bu makinede yok.** `CTFontCreateWithName("SF Mono")` **Helvetica**
döndürüyor — zincir Menlo'ya düşüyor ve tam olarak bu yüzden var. "Font açıldı"
bir kanıt değil; ölçüt dönen aile adı.

### Waive edilen bulgular

- **`HashMap<char, u16>` varsayılan SipHash ile.** `slot()` phase-4'te kare
  başına hücre başına çağrılacak. ASCII için `[u16; 128]` yan tablosu
  bağımlılıksız bir alternatif (hasher crate'i eklemek `CLAUDE.md`'ye göre
  mimari karar), ama ikinci bir arama yolu ekliyor ve kazanç **ölçülmedi** —
  `/measure` sonrası açılır.
- **`slot_origin`'deki bölme ve mod.** Sütun sayısını ikinin kuvvetine aşağı
  yuvarlamak maske/kaydırmaya çevirirdi; bedeli kenarda kullanılmayan yuvalar
  ve kazanç yine ölçülmedi. uv aritmetiği phase-4'ün işi.
- **`raster::ciz` her cache miss'te bağlam kuruyor.** Bağlamı alanda tutmak
  self-referential bir yapı demek (tampon işaretçisi); CG'nin tamponu
  sahiplenmesi ise `bytesPerRow`'u `slot()`'un sözleşmesine sızdırırdı —
  "tam `cell_w × cell_h` bayt" iddiası düşerdi.
- **`font::ac` her çağrıda `String` ayırıyor.** Karşılaştırma `CFString`
  düzeyinde de yapılabilirdi, ama bu yol atlas kurulumunda en çok iki kez
  koşuyor: soğuk.
- **Reuse merceğinin üç bulgusu phase-3'e devredildi** (iki yuvarlama kuralı,
  iki `Metrics` tipinin ad çakışması, `cell_px` demetinin tek geçiş noktası) —
  hepsi `CELL_PX`'in ölmesiyle aynı anda çözülüyor, phase-3.md'ye işlendi.

## Yayın Etkisi

- **`CLAUDE.md` bu commit'te güncellendi** (üç cümle: `bt-atlas`'ın durumu,
  katman tablosundaki platform kütüphaneleri, bağımlılık tabanı). Gerekçe
  yukarıda, madde 10.
- **`Cargo.lock` iki crate büyüdü**: `objc2-core-text` 0.3.2 ve
  `objc2-core-graphics` 0.3.2. Kullanıcı onaylı bağımlılık kararı
  (`discussion.md → ## Karar`). Sürüm oynaması ve kaldırılan crate yok.
- **ölçüm bekliyor:** `Atlas::slot`'un kare başına hücre başına maliyeti
  (SipHash araması + `slot_origin`'deki bölme) ve atlas doluluğu —
  `occupancy()` sayacı bunun için var. İkisi de ancak phase-4 atlası çizim
  yoluna bağladıktan sonra anlamlı.
- shader, terminfo/`TERM`, ayar şeması, tema/materyal biçimi, shell
  entegrasyonu, app bundle: **yok** — hiçbirine dokunulmadı.

---

## Checklist

- [x] Bağımlılıklar workspace + `bt-atlas` `Cargo.toml`'a eklendi
- [x] `font.rs`: zincir, ikame uyarısı, `Metrics`
- [x] `raster.rs`: alfa-only `CGBitmapContext`, `CTFontDrawGlyphs`
- [x] `lib.rs`: `Atlas`, sabit yuva ızgarası, rezident tofu, `occupancy()`
- [x] Test: metrik / aynı yuva / dolu atlas tofu / ikame / ölçek anahtarı
- [x] Doğrulama geçti (`make hepsi` yeşil; `make duman` → `kare=1 hucre=8 pipeline=ok`)
- [~] `make shader` — `.metal` ve `build.rs` el değmedi, koşul sağlanmıyor
- [~] `make test-yaris` — thread, PTY okuyucu ve paylaşılan duruma dokunulmadı;
      `bt-atlas` hiçbir yerden çağrılmıyor, koşul sağlanmıyor
- [~] `make terminfo` / `make kur` — girdisi yok (`proje.md`'nin bilinen listesi)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (dört mercek; 12 uygulandı, 9 waive/devir)
- [x] `/code-review` çalıştırıldı, bulgular giderildi (12 bulgu; 11 uygulandı, 1 waive)
- [x] `/audit` çalıştırıldı, bulgular giderildi (2 bulgu; 4 mercek temiz, 5 mercek ilgisiz)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
