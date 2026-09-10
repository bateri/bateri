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

**1. `ciz_kural` CoreGraphics kullanmıyor — doğrudan bayt yazıyor.** Phase
metni alfa-only `CGBitmapContext`'i öngörüyordu. Kod yazılırken `tofu_tamponu`
görüldü: o zaten fontu ve CG'yi hiç sormadan yuvaya elle kutu çiziyor. Kural
sprite'ları o mekanizmanın ikinci müşterisi oldu. Üç kazanç — çizim
deterministik (CG'nin antialias sürümüne bağlı değil, sınama tam yapı assert
edebiliyor), `BaglamYok` başarısızlık dalı **hiç doğmuyor** (dönüş `()`), ve
font hiç sorulmuyor. `slot()` bu yüzden kural kolunda `Cizim::Cizildi`'yi elle
üretiyor: doğmayan bir başarısızlığı tipte yaşatmamak için bilinçli.

**2. `crates/bt-gpu/src/renderer.rs` envanterde yoktu ama değişmek zorundaydı.**
`slot(ch)` → `slot(sprite, face)` imza değişimi tek çağıranı kırıyor
(`renderer.rs:657`). Tek satır, `Sprite::Char(glyph.ch), Face::Regular`.
Kapsam kayması değil derleme zorunluluğu; yüzün gerçek kullanımı phase-3'te.

**3. Kırpma gerçek fontla hiç ateşlenmiyor — bekçi bu yüzden saf fonksiyona
ayrıldı.** Ölçüldü (`font.rs`, bu makine): Menlo 13pt `u_pos` −0.825,
`u_kal` 0.571 → alt çizgi 14+1, hücre 17. 26pt'de 27+2, hücre 32. Yani
`kural_zarfi`'nin `.min()` kolu üretim yolunda ölü ve oradan yazılan bir
sınama mutasyonu yakalayamazdı. Kırpma ayrı bir saf fonksiyona alındı ve
`zarf_hucrenin_disina_tasmaz` **sentetik** girdiyle sınıyor. Mutasyonla
doğrulandı: `.min()` silinince kırmızı düşüyor.

**4. `curl_amp_px` `Metrics`'e girmedi.** Phase metni üç alan öngörüyordu.
Panelin (Simplification + Altitude, bağımsız) itirazı kabul edildi: genlik
fonttan gelmiyor, `kalinlik * 3` — bu çizicinin tasarım sabiti. `Metrics`
"fonttan türeyen hücre geometrisi" olarak kaldı; genlik `raster::KIVRIM_KAT`
olarak tek tüketicisinin yanında yaşıyor. `underline_px` ve `strikeout_px`
yerlerini hak ediyor: fonttan okunuyorlar ve `cell_px`'e kırpılmaları gerek.

**5. Sessiz yüz ikamesi yuva israfına yol açıyordu — `Yuzler` artık edinilen
yüzleri tutuyor.** Phase metninde yoktu; panelin en derin bulgusu. Tek yüzlü
bir ailede (`Monaco`) `unwrap_or_else(|| duz.clone())` kalın yüzü düz yüze
bağlıyordu **ama anahtar hâlâ `(Char, Bold)`'du**: bayt bayt aynı bitmap iki
ayrı yuvada, dört yüzle dört kat yuva. Üstelik `slot()`'un kendi yorumu bu
tehlikeyi tam adıyla anlatıp yalnız kurallar için önlüyordu. `Yuzler::etkin`
ikisini tek mekanizmaya indirdi: anahtar **istenen** yüzü değil **çizilen**
yüzü taşıyor. Geri düşüş uyarısı kurulumda bir kez basılıyor.

**6. `#[repr(u8)]` üç enum'a da eklendi.** `slot()` çizim yolunda ve türetilen
`Hash` enum discriminant'ını varsayılan olarak `isize` yazıyor: anahtar
`char`'ın 4 baytından 20 bayta çıkmıştı. `repr(u8)` onu 6 bayta ve tek
SipHash bloğuna indiriyor. İş sayımı, ölçüm değil — kare süresi iddiası
yok.

**7. `bulunamayan_yuz_duze_duser` Monaco ile yazıldı.** Zincirin tabanı Menlo
dört yüzü de taşıyor (ölçüldü: Bold/Italic/BoldItalic üçü de `true`), yani
geri düşüş dalı onunla ateşlenemiyor. Monaco bu makinede üçünü de vermiyor —
sınama tam olarak `Yuzler::turet`'in ayrı bir kurucu olma gerekçesini
kullanıyor. Mutasyonla doğrulandı: `etkin()` çökmeyi yapmayınca kırmızı.

**8. `kivrim_hucre_sinirinda_sureklidir`'in iddiası düzeltildi.** Checklist
"sol/sağ kenar sütunları eşleşiyor" diyordu; **o sınama yanlış olurdu** — bir
tam periyotta ilk ve son sütun eşit değil, orta eksene göre **ayna**dır
(`sin(τ(x+0.5)/w)` ile `sin(τ(w−1−x+0.5)/w)` işaretçe zıt). Bekçi
`tepe[x] + tepe[w−1−x]` toplamının sabit olduğunu assert ediyor; bu R2.4'ün
(periyot hücreyi tam bölmeli) gerçek karşılığı. Mutasyonla doğrulandı:
`DALGA_SAYISI` 1.5 yapılınca kırmızı.

**9. `Cargo.lock` oynamadı.** `CTFontTraits` feature'ı yeni crate çekmedi —
`objc2-core-text` zaten grafta ve feature yalnız kendi modülünü açıyor.

### `/audit` kaydı

**İlgisiz mercekler (elendi, kayda geçiyor):** 4 (ayar/tema şeması — model
yok), 5 (shell üçlüsü — `assets/shell/` el değmedi), 8 (boşta sıfır kare —
yeni animasyon/zamanlayıcı yok, `frame()` gövdesi değişmedi), 9 (shader/Rust
düzeni ve hücre boyutu — `.metal`, `build.rs` ve `Cell` el değmedi).

**Koşan mercekler:**

- **1 katman yönü — temiz.** `cargo tree`: `bt-atlas` yalnız
  `objc2-core-{foundation,graphics,text}` görüyor; `bt-core` ağacında ve
  kaynağında platform kütüphanesi yok. `Face`/`Sprite`/`RuleKind` `bt-core`'a
  **sızmadı** (bu setin en kolay hatası olurdu).
- **2 bağımlılık — temiz, ama kayda değer.** `Cargo.lock` **hiç değişmedi**;
  `Cargo.toml`'a yalnız `CTFontTraits` feature satırı girdi ve `/code-review`
  gerekçesinin yazılmasını istedi, yazıldı. Yeni crate yok.
- **3 panik yolu — bt-core el değmedi, yani merceğin harfiyle ilgisiz;** yine
  de `bt-atlas`'a giren iki üretim paniği gerekçelendirildi ve depo
  konvansiyonuna göre `// audit:` işareti aldı: `Face::traits`'in
  `unreachable!()`'ı (modül sınırıyla korunuyor, çağrı disipliniyle değil) ve
  `ciz_kural`'ın tampon boyu assert'i (`ciz`'inkinin kardeşi). Geri kalan
  bütün assert'ler `#[cfg(test)]` içinde.
- **6 ölçüm sahipliği — temiz.** `docs/OLCUMLER.md` yok ve oluşturulmadı; kod
  yorumlarında ve phase notlarında **hız/bellek iddiası yok**. Sınır bilinçli
  çizildi: "anahtar 4 bayttan 20 bayta çıkmıştı" ve "8 % 6 = 2" birer **iş
  sayımı / aritmetik**, kodu okuyarak doğrulanabilir; "Menlo 13pt `u_pos`
  −0.825" **fonttan okunan bir değer**, kare süresi iddiası değil — ve zaten
  `metrics()`'in var olan doc'u aynı türden sayılar taşıyor (003'ün
  denetiminden geçmişti).
- **7 thread ve blokaj — temiz.** `bt-atlas`'ta kilit, `sleep`, dosya G/Ç
  yok. İki `eprintln!` de **kurulum yolunda** (`Yuzler::turet`,
  `zincirden_ac`), çizim yolunda değil. `Atlas::new`'in dört `CTFont` açması
  ana thread'de ama kare başına değil: açılışta ve ölçek değişiminde
  (`/simplify`'ın Efficiency merceği bunu tartıp "eager kalsın" dedi).
- **10 belge ve üslup borcu — temiz.** `lib.rs` modül doc'unun kapsam cümlesi
  güncellendi ve doğru söylüyor; `Metrics`'in doc'unda düşürülen alandan iz
  kalmadı; `kural_zarfi`'nin "13pt'de 14+1, hücre 17" cümlesi ölçülen
  değerlerle uyuşuyor; `Yuzler::turet`'in "sınama için" gerekçesi artık
  karşılıklı (`bulunamayan_yuz_duze_duser`). *Bu mercek 002'de 4, 003'te 6
  çelişki bulmuştu; bu phase'de sıfır — çünkü çelişkilerin hepsini
  `/code-review` önce yakaladı.*

### `/code-review` kaydı

**12 bulgu, 12'si de uygulandı, waive yok.** Üçü gerçek arızaydı:

- **Dolu atlasta kural sprite'ı da tofu'ya düşüyordu.** `slot()`'un kapasite
  kapısı sprite match'inden önce. Birkaç bin farklı glyph görüldükten sonra
  (CJK metin, simge-ağır TUI) ızgara dolar ve o andan itibaren altı çizili
  **her** hücrenin altında çizgi yerine tofu kutusu belirirdi; belirti ancak
  uzun bir oturumdan sonra çıkardı. `KURAL_PAYI` ile kapasitenin altısı
  karakterlere kapatıldı — kurallar tembel kalıyor ama yerleri garanti.
  *Bunu `/simplify`'da Altitude merceği "rezident yuva" olarak önermiş, ben
  "LRU 00X'in işi, erken" diye reddetmiştim. Yanılmışım: gelecek kaygısı
  değil, bugünkü bir arızaydı.* Mutasyon doğrulandı.
- **Nokta/kesik deseninin periyodu hücre genişliğini bölmüyordu.** Ölçüldü:
  13pt@1x `w=8`, `Dashed` periyodu 6 → `8 % 6 = 2`; @2x `w=16`, periyot 12
  → kalan 4. Yani çok hücreli bir kesik alt çizgi her hücre sınırında faz
  atlıyor, komşu hücrelerde tire uzunlukları farklı görünüyordu. Kod bu
  tehlikeyi `DALGA_SAYISI`'nın doc'unda açıkça yazıp **kıvrımda uygulamış,
  `bant`'ta uygulamamıştı** — gözden kaçma. `bolen_periyot` eklendi,
  `desen_periyodu_hucreyi_tam_boler` bekçilik ediyor, mutasyon doğrulandı.
- **`Double`'ın ikinci bandı ve kıvrım yukarı doğru büyüyüp glyph gövdesine
  giriyordu.** 13pt: hücre 17, taban 13, alt çizgi 14 → 15-16 satırları
  **boş**, ama ikinci bant 12'ye yani `a e o`'nun son gövde satırına
  düşüyordu; iki çizgi ayrı görünmek yerine harflerin dibine yapışık tek
  kalın çizgi gibi okunurdu. İkisi de artık **önce aşağıdaki boş satırları**
  kullanıyor, yer kalmazsa yukarı taşıyor.

Kalan dokuz bulgu ve düzeltmeleri: `etkin()` düz düşüş yerine **merdiven**
oldu (`BoldItalic → Bold → Italic → Regular`; doğrudan `Regular`'a inmek
kalınlığı da düşürürdü ve `Bold`'u olup `BoldItalic`'i olmayan aile yaygın);
glyph düzeyinde geri düşüş eklendi (kalın yüzde olmayan '→' düz yüzde varsa
oradan gelir, tofu'ya değil); `yuz_turet` modül-özel yapıldı, böylece
`Face::traits`'in `unreachable!()`'ı çağrı disiplinine değil **modül sınırına**
dayanıyor; `copy_with_symbolic_traits`'in SAFETY yorumu düzeltildi (copy
ailesinde `matrix = NULL` "birim matris" değil, **kaynağın matrisi korunur**
— `ac()`'tan kopyalanmış yanlış gerekçeydi); `Metrics` literalinde kalan ölü
genlik yorumu silindi; `cell_wh()` gerekçesini kendi diff'inde tutmuyordu,
`ciz()` ve `tofu_tamponu` da ona geçti; `Cargo.toml`'daki feature'a "neden"i
yazıldı; `kural_zarfi` `cell_h == 0` girdisinde kendi değişmezini deliyordu,
artık `(0, 0)` dönüyor; ve `eprintln!`'in "bir kez" iddiası düzeltildi —
`ensure()` atlası yeniden kurunca satır tekrar düşüyor, yorum artık bunu
söylüyor.

**Sınanmayan tek düzeltme:** glyph düzeyinde geri düşüş. Menlo'da düz yüzün
taşıyıp kalın yüzün taşımadığı bir karakter bulunamadı, yani dal bu makinede
ateşlenemiyor. Uydurma sınama yazmak yerine kayda geçiyor — kırpma dalıyla
(not 3) aynı dürüstlük.

### `/simplify` kaydı

Dört mercek (reuse, simplification, efficiency, altitude) paralel koştu.
**Uygulanan:** ortak `kaplama()` yardımcısı (formül `bant`/`kivrim`'de
kopyaydı ve harmanlama disiplininde **zaten ayrışmıştı**); ölü `w` + çizimden
**sonra** koşan `debug_assert!` silindi (değişmezin sahibi `font::yukari`);
desen closure'ı (`&dyn Fn`) sayısal `(periyot, dolu)` çiftine indi ve
`.max(2.0)`/`.max(6.0)` ölü savunmaları düştü; `curl_amp_px` `Metrics`'ten
çıktı (not 4); `Metrics::cell_wh()` eklendi (açım dört yerde kopyaydı);
`Yuzler` edinilen yüzleri tutar oldu (not 5); `#[repr(u8)]` (not 6);
`yuz_turet`'in `is_empty()` kapısı ve `Face::traits`'in `Regular` kolu ölü
sentineldi → `unreachable!()` ile sözleşme netleşti; `turet`'in doc'unun vaat
ettiği sınama yazıldı (not 7).

**Reddedilen:** kural sprite'larını `TOFU` gibi **rezident** yapmak (Altitude,
kendisi de "önerilen değil" dedi) — LRU 00X'in işi ve bugün var olmayan bir
tahliyeye bugünden istisna kurmak erken. Dört `CTFont`'u **tembelleştirmek**
(Efficiency kendi reddetti: kazanç yalnız açılış, bedeli descriptor
eşleşmesini çizim yoluna taşımak). Anahtarı elle paketlenmiş `u32` yapmak
(`repr(u8)` zaten eski maliyete döndürüyor; elle `Hash` aşırı). `&dyn Fn`'i
generic'e çevirmek (Efficiency: beş monomorfizasyon kopyası, karşılığı yok —
sayısal çift ikisini de gereksiz kıldı).

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

- [x] `Cargo.toml`: `CTFontTraits` feature'ı (`Cargo.lock` oynamadı)
- [x] `Face` enum'ı + `Yuzler`: dört yüz, `symbolic_traits()` kesişimi, düz yüze geri düşüş, **tek seferlik** uyarı
- [x] `Metrics`: `underline_px`, `strikeout_px` — ikisi de `metrics()` içinde hücreye kırpılır
- [~] `curl_amp_px` **eklenmedi** — fonttan gelmiyor, tasarım sabiti; `raster::KIVRIM_KAT` oldu (not 4)
- [x] `RuleKind` + `raster::ciz_kural`: altı çeşit, fontsuz — **CG değil, doğrudan bayt** (not 1)
- [x] `Sprite` enum'ı + `HashMap<(Sprite, Face), u16>` + `slot(sprite, face)` + `bt-gpu` çağrı yeri (not 2)
- [x] `lib.rs` modül doc'unun kapsam cümlesi düzeltildi
- [x] Test: `kalin_yuz_ayri_yuva_alir` — dört yüzün dördü de ayrı yuva
- [x] Test: `bulunamayan_yuz_duze_duser` — Monaco ile, **mutasyon doğrulandı** (not 7)
- [x] Test: `kural_zarfi_gercek_fontta_hucreye_sigar` + `zarf_hucrenin_disina_tasmaz` (sentetik, **mutasyon doğrulandı** — not 3)
- [x] Test: `kivrim_hucre_sinirinda_sureklidir` — **iddiası düzeltildi**, ayna simetrisi (not 8); `kivrim_gercekten_dalga` da eklendi, ikisi de mutasyon doğrulandı
- [x] Test: `kural_sprite_bos_degil_ve_cesitler_ayrisir` + `kural_yuzden_bagimsiz_tek_yuva_tutar` + `kalin_glif_duz_yuzun_yuvasina_sigar`
- [x] Doğrulama geçti — `make hepsi` 0, `make test-yaris` 0, `make duman` `kare=1 hucre=8 glif=6 pipeline=ok` (**değişmedi**); `make shader` 0 koştu ama koşulu doğmamıştı (`.metal` el değmedi); `make terminfo`/`make kur` girdisi yok
- [x] `/simplify` çalıştırıldı — 4 mercek, 9 bulgu uygulandı, 4 gerekçesiyle reddedildi (kayıt yukarıda)
- [x] `/code-review` çalıştırıldı — 12 bulgu, **12'si de giderildi**, waive yok (kayıt yukarıda)
- [x] `/audit` çalıştırıldı — 4 mercek ilgisiz, 6 mercek koştu, hepsi temiz; iki üretim paniğine `// audit:` işareti eklendi (kayıt yukarıda)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
