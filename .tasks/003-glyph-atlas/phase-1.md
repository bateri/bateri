# Phase 1 — sRGB geçişi

## Özet

Yüzey ve pipeline `BGRA8Unorm_sRGB`'ye geçer, palet lineerleştirilir, offscreen
sınaması bunu görebilecek hâle gelir. Glyph yok — bu phase alfa karıştırmanın
doğru uzayda koşması için zemini hazırlar.

_Requirements: R3, R3.1, R3.2, R3.3, R3.4_

**Bölünemez.** Yarısı (sRGB yüzey, lineerleşmemiş palet) tam olarak commit
edilmemesi gereken durum: pencere zemini `0x1a1c21` yerine `0x59` gri açılır ve
hiçbir sınama bunu görmez.

**Beklenen görünür etki: yok.** Kullanıcı bu commit'ten önce ve sonra aynı
renkleri görmeli. Değişen renk değil, karıştırmanın uzayı; glyph gelene kadar
karıştırma da yok. Renk gözle değiştiyse lineerleştirme eksik ya da fazla
uygulanmıştır.

---

## 1. Palet lineerleşir

`crates/bt-core/src/color.rs`

Bugün `rgba()` sRGB-kodlu float veriyor (`c / 255.0`). Hedef `_sRGB` olunca
fragment çıktısı **lineer** sayılır ve donanım yazarken sRGB'ye kodlar; aynı
float lineer sanılırsa palet açılır. Aynı floatlar `MTLClearColor`'a da gidiyor
(Metal sRGB hedefte clear rengini de lineer okur), yani düzeltmenin tek yeri bu
fonksiyon: pencere zemini ve hücreler oradan besleniyor.

`powf` stable'da `const` değil, `DEFAULT_BG`/`DEFAULT_CURSOR` ise `const`.
Çözüm 256 girdilik `const` tablo — kaynak baytı tam burada elde:

```rust
/// sRGB kodlu 8-bit kanal → lineer f32. Kaynağı IEC 61966-2-1 transfer
/// fonksiyonu; `srgb_tablosu_transfer_fonksiyonunu_izler` tabloyu formüle
/// bağlar, yani tablo elle bakımlı bir sabit değil türetilmiş bir veri.
#[rustfmt::skip]
const SRGB_LINEER: [f32; 256] = [ /* … */ ];

/// Rengi renderer'ın beklediği **lineer** RGBA'ya çevirir.
///
/// Ad "lineer" diyor çünkü bu depoda sessizce yanlış olabilecek tek şey
/// float'ın hangi uzayda olduğu: çizim hedefi `BGRA8Unorm_sRGB` ve donanım
/// yazarken kodluyor. `c / 255.0` yazan bir çağrı yeri paleti griye açar ve
/// hiçbir assert bunu görmez.
pub(crate) const fn lineer_rgba(color: Rgb) -> [f32; 4] {
    [
        SRGB_LINEER[color.r as usize],
        SRGB_LINEER[color.g as usize],
        SRGB_LINEER[color.b as usize],
        1.0,
    ]
}
```

`rgba` → `lineer_rgba` adlandırması bilinçli: çağrı yerleri (`DEFAULT_BG`,
`DEFAULT_CURSOR`, `session::frame`) uzayı adında okur.

Sınama, tabloyu formüle bağlar (f64 referans, f32 tablo → dar epsilon) ve iki
uç noktayı çakar:

```rust
#[test]
fn srgb_tablosu_transfer_fonksiyonunu_izler() {
    assert_eq!(SRGB_LINEER[0], 0.0);
    assert_eq!(SRGB_LINEER[255], 1.0);
    for (i, &lineer) in SRGB_LINEER.iter().enumerate() {
        let c = i as f64 / 255.0;
        let beklenen = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        assert!((f64::from(lineer) - beklenen).abs() < 1e-7, "{i}");
    }
}
```

---

## 2. Yüzey ve pipeline `BGRA8Unorm_sRGB`

`crates/bt-gpu/src/renderer.rs`

Tek satır: `system_default()` formatı değişir. `Surface` ve pipeline formatı
zaten `self.pixel_format`'tan okuyor, ayrışma yapısal olarak imkânsız.

```rust
// Hedef sRGB: fragment çıktısı lineer sayılır ve donanım yazarken kodlar.
// Karşılığı `bt_core::color::lineer_rgba`; ikisi birlikte değişir.
Self::new(device, MTLPixelFormat::BGRA8Unorm_sRGB)
```

---

## 3. `hedef_doku` formatı `Renderer`'dan okur

`crates/bt-gpu/src/renderer.rs` (sınama yardımcısı)

Bugün formatı elle yazıyor (`BGRA8Unorm`) ve sözleşmeyi bozan tek yer o.
Pipeline sRGB'ye geçince offscreen sınaması assert'le değil Metal doğrulama
istisnasıyla düşerdi — teşhisi zor bir çökme.

```rust
fn hedef_doku(r: &Renderer, kenar: usize) -> Retained<ProtocolObject<dyn MTLTexture>> {
    // Format `Renderer`'dan: pipeline hangi formata derlendiyse hedef de o.
    // Elle yazılsaydı sRGB geçişi burada doğrulama istisnası olurdu.
    … MTLPixelFormat = r.pixel_format …
}
```

---

## 4. Offscreen sınamasına ara ton

`crates/bt-gpu/src/renderer.rs` → `cell_bg_pikseli_gpu_tarafinda_boyar`

Sınamanın bugünkü girdileri yalnız `0.0` ve `1.0`; ikisi de sRGB transfer
fonksiyonunun **sabit noktaları**, yani geçişi göremez. Boş kalan sağ üst
çeyrek için kullanılan clear rengi de saf mavi.

Değişiklik: üçüncü bir instance olarak `DEFAULT_BG` (`0x1a1c21`) çizilir ve geri
okunan baytların `0x1a, 0x1c, 0x21` olması aranır. Bu, lineerleştirme ile
donanımın kodlamasının birbirini tam olarak tersine çevirdiğinin kanıtı:
lineerleştirme unutulursa okunan bayt `0x59` civarına açılır.

```rust
// sRGB round-trip: lineerleştirilmiş palet + donanımın kodlaması birbirini
// tersine çevirmeli. ±1 tolerans: 8-bit sRGB kodlaması yuvarlama taşır,
// donanımdan bit birebirlik istemek kapıyı sürücüye rehin ederdi.
let (r, g, b) = piksel(2, 12);
assert!(r.abs_diff(0x1a) <= 1 && g.abs_diff(0x1c) <= 1 && b.abs_diff(0x21) <= 1);
```

---

## Uygulama Notları

**`Renderer::new`'in `pixel_format` parametresi kalktı** (planda yoktu).
`/simplify` altitude merceği gösterdi: tek çağrıcısı `system_default` ve
parametre yalnız bir sabiti taşıyordu, yani "lineer palet + sRGB olmayan
hedef" **temsil edilebilir bir durum** olarak kalıyordu. Phase'in bölünemezlik
iddiası böylece yorumdan tipe indi: ikinci bir renderer'a (offscreen, ekran
görüntüsü) düz `BGRA8Unorm` geçen kişi artık öyle bir yol bulamıyor. `new` ve
`system_default` tek fonksiyona katlandı.

**Clear rengi de ara ton oldu.** Plan yalnız bir hücreyi ara ton yapıyordu;
`MTLClearColor` yolu sınanmamış kalıyordu ve üretimde pencerenin görünen
zemininin **tamamı** o yoldan geliyor (`frame()` varsayılan arka planlı
hücreleri eliyor, `link.rs` clear'a `DEFAULT_BG` veriyor). Saf mavi
bırakılsaydı, clear rengini "hedefin uzayına çevireyim" diye bir kez daha
kodlayan biri pencere zeminini karartır, hücreleri doğru bırakır ve **bütün
sınamalar yeşil geçerdi**. Boş çeyreğin clear'ı `DEFAULT_CURSOR` (`0x7a9cc6`)
oldu; kırmızı/yeşil/`DEFAULT_BG`'den ayrık, "boş çeyrek clear'dan geliyor"
iddiası korunuyor.

**Derleme zamanı kanaryası eklendi, sonra silindi.** `const _: () =
assert!(SRGB_LINEER[0x1a] < 0.02)` yazılmıştı; `/simplify` haklı olarak
fazlalık buldu — `srgb_tablosu_transfer_fonksiyonunu_izler` 256 girdinin
**hepsini** formüle 1e-7 toleransla bağlıyor, yani kanaryanın iddiası onun
kesin altında. Dahası `0x1a` indeksi `BG` sabitine elle bağlıydı: tema modeli
paleti değiştirdiğinde kanarya düşmez, sessizce başka bir rengi sınamaya
başlardı.

**`#[inline]` bulgusu — planda yoktu, sıcak yol gerilemesi.** `lineer_rgba`
hücre başına, kare başına çağrılıyor (`Session::frame`) ve workspace'te
`[profile.release]` yok, yani **LTO kapalı**: gövde crate sınırını geçmiyordu.
`nm -u target/release/deps/libbt_gpu-*.rlib` bunu tanımsız sembol olarak
gösterdi. `lineer_rgba`, `resolve`, `default` ve `dim` birlikte işaretlendi —
yalnız biri alınırsa çağrı ötekine kayıyor. İşaretlemeden sonra dört sembolün
dördü de tanımsız listesinden düştü. **Bu diff'in getirdiği bir gerileme
değil** (eski `rgba` de aynı durumdaydı) ama düzeltmenin doğal yeri burası.
Kazancın büyüklüğü **ölçüm bekliyor**.

**`CLAUDE.md` maddesi phase-4'ten buraya çekildi.** Plan R7 belgeleri phase-4'e
bırakıyor, ama iki crate arasındaki değişmez (`bt-core` lineer verir, hedef
kodlar, ikisi birlikte değişir) **bu commit'te doğuyor** ve `CLAUDE.md`'nin
kendi kuralı "buradaki bir cümle kodla çelişirse ikisinden biri aynı commit'te
düzelir" diyor. Atlas ve jeton belgeleri phase-4'te kalıyor.

**Bekçilerin kendisi mutasyonla doğrulandı.** İki mutasyon koşuldu ve ikisi de
ayrı, teşhis edilebilir mesajla düştü: lineerleştirme kaldırılınca
`(5a, 5d, 65)` (planın öngördüğü ~`0x59` gri), hedef `BGRA8Unorm`'a çevrilince
`(03, 03, 04)`. Phase'in "bölünemez" iddiası böylece kanıtlı.

**Bilinen sınır — `CAMetalLayer.colorspace` `nil` kaldı.** Katman renk eşleme
yapmıyor, yani P3 ekranda sRGB baytları P3 olarak yorumlanıyor ve renkler
doygunlaşıyor. Bu **bu phase'in getirdiği bir şey değil**, geçişten önce de
böyleydi; düzeltmesi `CGColorSpaceCreateWithName` ile bir satır ama `bt-gpu`'ya
`objc2-core-graphics` kenarı ekliyor — yani ayrı bir bağımlılık kararı ve
kullanıcıya sorulur (`proje.md` → Yayın etkisi, "yeni bağımlılık"). Kayda
geçti, yapılmadı.

**Kalite kapısının atlanan bulguları (waive, gerekçeli):**

- **Offscreen sınaması paletin baytlarını elle yazıyor** (`0x1a1c21`,
  `0x7a9cc6`). `/code-review` bunu tema modeli geldiğinde kırılacak bir bağ
  olarak işaretledi ve sınama-yerel bir ara ton önerdi. **Reddedildi:** sınama
  çizdiği rengi `DEFAULT_BG`/`DEFAULT_CURSOR`'dan, yani **üretim yolundan**
  alıyor; girdisi de yerel olsaydı `lineer_rgba`'yı bozan mutasyon sınamayı
  düşürmezdi ve phase'in tek uçtan uca bekçisi ölürdü. Bağ bilinçli; sınamanın
  yorumunda yazılı.
- **`CAMetalLayer.colorspace` `nil`** — yukarıdaki bilinen sınır. Yeni olan
  şu: format artık sRGB etiketli, katman etiketsiz, yani ikisi **artık aynı
  şeyi söylemiyor**. Saklanan baytlar değişmediği için (kanıtlandı) beklenen
  sonuç yine "görünür etki yok", ama **hiçbir otomatik sınama kompozit edilmiş
  pikseli görmüyor**: offscreen sınaması `Shared` dokudan okuyor, `make duman`
  yalnız jeton sayıyor ve `proje.md` zaten "pencerenin görünür ve doğru
  olduğunu hiçbir hâlde kanıtlamaz" diyor. Ucuz doğrulama bir göz kontrolü.

**Uzayı tipe yazmak phase-4'e devredildi.** `/code-review` haklı bir simetri
kaçağı buldu: `bt-gpu` tarafında yanlış format artık **temsil edilemez**
(`PIXEL_FORMAT` `const`), ama `bt-core` tarafında `CellBg.rgba` hâlâ `pub` bir
`[f32; 4]` ve uzayı yalnız bir yorum söylüyor. Newtype (`LinearRgba`) doğru
düzeltme; yeri phase-4, çünkü o phase bu sınırı zaten baştan yazıyor
(`CellBg` → `Cell`). Şimdi yapılsaydı aynı tipin üçüncü yazımı olurdu.
Kılavuza işlendi (`phase-4.md` → "Phase-1'den devir").

**`context.md`'ye tarihli not düştü.** Setin başlangıç anlık görüntüsü "yüzey
`BGRA8Unorm` — sRGB değil" diyor ve phase-2..4 onu okuyor; düzeltmek yerine
(anlık görüntü öyle kalmalı) başına phase-1'i işaret eden bir not kondu.
Tuzağın kendisi phase-4'ün kılavuzunda da adıyla yasaklandı: shader'a gamma
düzeltmesi yazmak paleti iki kez kodlar.

**Görünür etki gerçekten yok.** Framebuffer'a giden baytlar geçiş öncesi ve
sonrası aynı (`0x1a1c21` → `0x1a1c21`, ±1 yuvarlama). `make duman` iki koşuda
`kare=2 hucre=8 pipeline=ok` ve `kare=1 hucre=8 pipeline=ok` verdi; `kare`
oynaması hasar zamanlamasıdır, jeton sözleşmesi değişmedi.

## Yayın Etkisi

- **shader** — `.metal` **değişmedi**; `cell_bg.metal` transfer fonksiyonu
  bilmiyor ve bilmemeli (kodlama ROP'un sabit fonksiyonu). `make shader`
  gerekmedi.
- **belge** — `CLAUDE.md` → "Bilinmesi gerekenler"e renk uzayı maddesi eklendi
  (yukarıdaki gerekçe). `docs/MIMARI.md` yok, crate `lib.rs` başlıkları
  etkilenmedi.
- **ölçüm bekliyor: `#[inline]` işaretlerinin hücre başına renk yolundaki
  etkisi** — `lineer_rgba`/`resolve`/`default`/`dim` artık satır içine alınıyor
  (sembol tablosuyla doğrulandı), kazancın büyüklüğü ölçülmedi. `/measure`.
- terminfo / `TERM`, ayar şeması, tema biçimi, shell entegrasyonu, app bundle,
  yeni bağımlılık: **yok**. `Cargo.lock` oynamadı.

---

## Checklist

- [x] `color.rs`: `SRGB_LINEER` tablosu + `rgba` → `lineer_rgba` (+ `#[inline]`)
- [x] `renderer.rs`: `system_default` → `BGRA8Unorm_sRGB` (`PIXEL_FORMAT` `const`)
- [x] `hedef_doku` formatı `Renderer`'dan okur
- [x] Test: `srgb_tablosu_transfer_fonksiyonunu_izler` (tablo ↔ formül + lineerlik)
- [x] Test: `cell_bg_pikseli_gpu_tarafinda_boyar` ara ton hücre **ve** ara ton clear geri okur (iki mutasyonla doğrulandı)
- [x] Doğrulama geçti: `make hepsi`, `make shader` (`.metal` yorumu değişti), `make duman` → `kare=2 hucre=8 pipeline=ok`
- [~] `make test-yaris` koşulmadı — thread, PTY okuyucu ya da paylaşılan duruma dokunulmadı (koşul sağlanmıyor)
- [~] `make terminfo` / `make kur` koşulmadı — girdisi yok (`proje.md` notu)
- [x] `/simplify` çalıştırıldı (4 mercek), bulgular uygulandı: `new` parametresi kalktı, clear ara ton oldu, kanarya silindi, `#[inline]`
- [x] `/code-review` çalıştırıldı (10 bulgu): 7 uygulandı, 1 phase-4'e devredildi, 2 gerekçeli waive (yukarıda)
- [x] `/audit` çalıştırıldı: M1/M6/M9 temiz, M3 + M10 giderildi, M2/M4/M5/M7/M8 ilgisiz
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: 4938faf
