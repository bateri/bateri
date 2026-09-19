# Phase 1 — Yüzey ikilisi: `cursor_radius` ve `cursor_glow`

## Özet

İmlecin köşe yarıçapı ve gölgesi ayar dosyasından gelsin; shader'a
dokunmadan, yalnız uniform'u besleyen kaynağı değiştirerek.

_Requirements: R1, R1.1, R1.2, R1.3, R1.4, R2, R2.1, R3, R3.1, R3.2, R3.3, R4, R6, R7, R8_

## Değişiklikler

- **`crates/bt-core/src/settings.rs`** — setin ağırlık merkezi.
  - Varsayılanlar **`pub const`** olarak burada doğuyor (R2) ve `Settings`'in
    `Default`'u onları okuyor. `bt-gpu` aynı const'ları **import ediyor**;
    iki literal olsaydı piksel bekçileri kendi tutarlılığını sınar, sevk
    edilen hale başka ölçüde olsa da yeşil geçerdi.
  - `CaretStyle { radius_ratio, glow }` — iki alanlı `Copy` kayıt.
    **Hale tek sayı** (R1.1): pay ve alfa aynı çarpandan türüyor ve bugünkü
    iki sabit `bt-gpu`'da **taban olarak kalıyor**.
  - `ranged_float(…)` **adıyla doğuyor** (R4): aralıklı ondalık ayrıştırıcı
    depoda iki kez elle yazılmış (`line_height`, `font_size`) ve
    adlandırılmazsa üçüncü kopya burada doğardı. `line_height`/`font_size`'ın
    ona taşınması **bu phase'in işi değil** — taşınmazsa depo aynı işin iki
    yolunu bilerek taşır ve bu Uygulama Notları'na yazılır.
  - `Changes`'e **yeni alan** (`caret`): `changes.terminal` bugün
    `self.terminal() != new.terminal()`'in ta kendisi ve bu anahtarlar
    `TerminalOptions`'a girmiyor.
  - `TEMPLATE`'e iki satır; **kullanıcının diskine yazılıyor**, yani yorumlar
    kabul edilen aralığı söylemeli (014'ün dersi).
  - `CaretShape`'in doc'undaki "dosya kodu aynalıyor" cümlesi **betimleyici**
    olarak işaretleniyor (R1.4); emsal `osc52`.
- **`crates/bt-gpu/src/frame.rs`** — üç sabit `CaretStyle`'dan besleniyor.
  - `CARET_RADIUS_RATIO` ve `CARET_GLOW_RATIO`/`CARET_GLOW_ALPHA` **taban**
    olarak kalıyor; oran onların üstüne çarpan.
  - `Frame`'e alan; `clear`'ın **ikinci argümanı** olarak giriyor (R3.2).
    Hareket karesi `clear` çağırmıyor, yani değeri koruyor.
  - **`CellMetrics`'e dokunulmuyor** (R3.1): 32 çağrı yeri var ve
    `GUTTER_PT`'nin doc'u zaten "ayar değil sabit" diyor.
- **`crates/bt-gpu/src/link.rs`** — `caret: Cell<CaretStyle>` ve
  `set_caret_style`. **Değişimde kare istiyor, aynı değerde no-op** (R3.3);
  emsal ve gerekçe `set_cursor_motion`/`set_reduce_motion`/`set_focused`.
- **`crates/bt-shell/src/app.rs`** — iki çağrı yeri: açılış tohumu
  (`observe_reduce_motion`'ın yanı) ve kayıt anı (`changes.caret`).
  **İkisi de gerekli** — biri eksikse ayar ya hiç ya da yalnız sonraki
  oturumda uygulanır.
- **`docs/AYARLAR.md`** — `[terminal]` tablosu, proza ve **Şablon bloğu**;
  blok `TEMPLATE` ile **bayt bayt aynı** olmak zorunda
  (`documented_template_is_the_template`).

## Kabul

- Ayar dosyasına `cursor_radius = 0.3` yazıp kaydetmek imleci **kayıt anında**
  değiştiriyor; pencere boştayken de (kare isteniyor).
- `cursor_glow = 0` gölgeyi kapatıyor, `1.0` bugünkü görüntüyü veriyor.
- Aralık dışı değer (`cursor_radius = 1.5`) **kendi anahtarını
  değiştirmiyor**, öteki anahtarlar okunuyor ve pencere alt başlığında tanı
  çıkıyor. Kırpma yok.
- Dosyası olmayan kullanıcı ile hermetik duman koşusu **aynı sayıyı** görüyor.
- `make duman` jetonları değişmiyor: süreli koşu ayar dosyasını okumuyor.

## Uygulama Notları

<!-- Kodlanırken doldurulacak. -->

## Yayın Etkisi

- **ayar şeması** — iki yeni anahtar, varsayılanları bugünkü görüntü; eski
  anahtar akıbeti **yok** (hiçbiri değişmedi). `TEMPLATE` büyüyor ve
  `create_if_missing` yalnız dosya yokken yazdığı için mevcut kullanıcı
  dosyası **değişmiyor** — yeni anahtarları görmek isteyen dosyasını silmeli
  ya da elle eklemeli. Bu `docs/AYARLAR.md`'ye yazılacak.
- **belge** — `docs/AYARLAR.md` (tablo + proza + Şablon bloğu), `CLAUDE.md`'nin
  ayar cümlesi, `CaretShape`'in doc'u.
- **seçilmiş sayı** — aralıkların uçları; `const` doc'unda "seçilmiş,
  ölçülmemiş" + gerekçe. `docs/OLCUMLER.md`'nin konusu **değil**.
- shader / terminfo / app bundle / yeni bağımlılık: yok. `make kur` gerekmiyor.
- **geri alma:** phase commit'ini revert; anahtarlar **silinmez**, emekli
  edilir (dosyada kalır, okunmaz, görülünce tanı) — emsal 009'un `prompt`'u.

## Checklist

- [ ] `settings.rs`: `pub const` varsayılanlar, `CaretStyle`, `Changes.caret`
- [ ] `settings.rs`: `ranged_float` **adıyla** doğdu (R4)
- [ ] `settings.rs`: `TEMPLATE`'e iki satır, yorumlar aralığı söylüyor
- [ ] `settings.rs`: `CaretShape`'in doc'u — ayna betimleyici (R1.4)
- [ ] `frame.rs`: sabitler **taban**, oran çarpan; `clear`'ın ikinci argümanı
- [ ] `frame.rs`: `CellMetrics`'e dokunulmadı
- [ ] `link.rs`: `Cell<CaretStyle>` + `set_caret_style`, değişimde kare
- [ ] `app.rs`: açılış tohumu **ve** kayıt anı
- [ ] `bt-gpu` varsayılanı `bt-core`'un const'undan **import ediyor** (R2)
- [ ] Test: aralık dışı değer kendi anahtarını değiştirmiyor + tanı
- [ ] Test: `Changes.caret` yalnız bu iki anahtar değişince doğru
- [ ] Test: `template_is_the_defaults` ve `documented_template_is_the_template`
- [ ] Test: yarıçap/hale **gerçek yoldan** sürülüyor, `caret_sdf_override`
      yalnız kenar için kaldı (R6)
- [ ] `docs/AYARLAR.md` (tablo + proza + Şablon), `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi`)
- [ ] `make duman` (kullanıcıda) — jetonlar değişmemeli
- [ ] **Gözle kontrol:** ayarı kaydedince **boştaki** pencerede de değişiyor
- [ ] Yayın etkisi yazıldı
