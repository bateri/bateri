# Phase 6 — `bt-gpu` lavapipe'ta ve sözleşme

## Özet

`bt-gpu` Linux'ta derlenir ve offscreen piksel sınamaları lavapipe üstünde
koşar; sözleşme belgeleri setin sonucuna göre güncellenir (`discussion.md` →
Karar 7, 8).

_Requirements: R8, R9, R1.2_

## Değişiklikler

- **`crates/bt-gpu/src/renderer.rs`** — `Backends::METAL` macOS'ta, Linux'ta
  `Backends::VULKAN` (`cfg`); doc'undaki "font setinde açılır" cümlesi.
- **`crates/bt-gpu/src/surface.rs`** — `Surface::from_layer`
  `cfg(target_os = "macos")`.
- **`crates/bt-gpu/src/renderer/tests.rs`, `renderer/wgpu_tests.rs`** —
  Karar 7'nin `bt-gpu` listesi: aile adlı üç assert, `☕` sınaması, renk/emoji
  sınamaları, `scene_wide`, `SCALE`'in gerekçesi; örnekler `bt-atlas`'ın
  fikstürüyle aynı kaynaktan. Linux'ta düşen sınama önce font/arka uç diye
  ayrılır; arka uç farkı için karar (arka uca özgü beklenti ya da tolerans)
  Uygulama Notları'na.
- **`tools/linux/Dockerfile`** — `mesa-vulkan-drivers`, `libvulkan1`.
- **`Makefile`** — `LINUX_CRATES += -p bt-gpu`, `linux` yorumu.
- **`CLAUDE.md`** — katman tablosunun `bt-atlas` satırı (iki arka uç,
  platform kütüphaneleri) ve bağımlılık paragrafı (`freetype-rs`,
  `fontconfig`, `harfrust`, pkg-config, hedefe koşullu, Linux bildirim
  borcu); `bt-gpu` satırının "Linux'ta derlendiği font setinde" ifadesi.
- **`.claude/is-akisi/proje.md`** — Doğrulama'nın `make linux` satırı
  (kapsam üç crate, tetik).

## Kabul

- `make linux` yeşil: `bt-core`, `bt-atlas`, `bt-gpu`; `wgsl_pipelines_build`
  ve piksel sınamaları lavapipe'ta.
- `make hepsi` ve `make shader` yeşil; macOS'ta `bt-gpu` sınamaları
  değişmeden geçiyor.
- Belgeler kodla çelişmiyor (katman tablosu, Doğrulama satırı, Dockerfile
  ve `Makefile` yorumları aynı kapsamı söylüyor).

## Checklist

- [x] `cfg` düzenlemeleri (backend, `from_layer`)
- [x] `bt-gpu` sınamaları fikstüre
- [x] İmaj + `LINUX_CRATES`
- [x] `CLAUDE.md`, `proje.md`, yorumlar
- [x] Test: lavapipe'ta piksel sınamaları; macOS'ta değişmeden
- [x] Doğrulama geçti (`make hepsi` + `make shader` + `make linux`)

## Uygulama Notları

- **Arka uç farkı yok, hepsi font farkı.** `bt-gpu` Linux'ta ilk koşuda 5
  sınama düştü ve beşi de fontsuz CJK'dan (`漢`, `一`) ya da aile adından;
  lavapipe'ta tam bayt bekleyen hiçbir sınama düşmedi, yani arka uca özgü
  beklenti ya da tolerans kararı gerekmedi. Mesa'nın `XDG_RUNTIME_DIR is
  invalid` satırları loader gürültüsü (stderr), düşüş değil.
- **Fikstür `bt-atlas`'tan, bir özellikle.** `bt-atlas`'ın arka uç
  fikstürleri `#[cfg(any(test, feature = "fixture"))] pub mod fixture` ve
  `bt_atlas::fixture` olarak dışarıda (`#[doc(hidden)]`); `bt-gpu` onu
  dev-dependency'de `features = ["fixture"]` ile açıyor. Yeni crate yok,
  `Cargo.lock` değişmedi. `family_name` özel tipe dokunduğu için yalnız
  `cfg(test)`. Yeni sabitler: `CHAIN_FAMILIES` (macOS `SF Mono`/`Menlo`),
  `PAIR_CHAR`, `STROKE_PAIR_CHAR`, `ONE_CELL_WIDE_CHAR`; macOS değerleri
  bugünkü `漢`/`一`/`☕`, yani macOS sınamalarının öznesi değişmedi.
  `SCALE` = `CLUSTER_SCALE`.
- **Linux'un iki geniş örneği ayrı ölçüldü:** `PAIR_CHAR` = `⁂` (fikstürün
  `WIDE_CHAR`'ı; dikiş hizasında mürekkep var), `STROKE_PAIR_CHAR` = `⟺`
  (U+27FA, DejaVu Sans): dikiş sınaması iki komşu sütunun eşitliğini
  istiyor ve `⁂`'ın yıldızı dikişin üstünde sütundan sütuna değişiyor
  (fark 0x29). 13pt@2x'te on sekiz aday tarandı; `⟺`, `⟼`, `⤚` geçti.
  `☕` DejaVu Sans Mono'da var, yani tek hücreye sığan geniş sınaması
  Linux'ta da boş değil.
- **Bulunan platformsuz kusur — düzeltildi: phase-6'dan sonraki istisna
  commit'i ("Önbellekteki reddedilen geniş karakteri tek kutu cevapla";
  kullanıcı onayı 2026-09-30).**
  `Atlas::slot`'un önbellek kolu kaydı `half: want` ile cevaplıyor; ret kolu
  `TOFU`'yu `Left` ve `Right` anahtarlarına da yazıyor ve taze cevap `Whole`.
  Yani reddedilen geniş karakter ilk karede **bir**, sonraki her karede
  **iki** kutu çiziliyor (`slots::fan` önbellekten gelen `Left`'i ikiye
  yelpazeliyor). Linux'ta fontu olmayan her CJK karakterde (piksel dökümüyle
  görüldü: `dock_fx`'in statik karesi bir kutu, hayalet iki); macOS'ta
  tanık gösterdi — orantılı ailenin (Helvetica) kalın/eğik yüzünde geniş
  `☕`/`⚡`: 160 satır `Left`/`Right` çifti → 80 satır `Whole`, başka fark
  yok. Çare önbellek kolunda tek dal (`(TOFU, Plane::Mask)` → `Half::Whole`)
  ve platformsuz bekçi (`GATE_PROBES`'un her karakteri için `Left` iki kez
  sorulunca aynı `Placed`; çaresiz Linux'ta kırmızı olduğu doğrulandı).
  R1.1 ("tanık ebeveynle aynı") yüzünden phase-6'ya girmedi; ayrı commit'te
  tanığın farkı yine yalnız bu 160 → 80 satır.
- `PAIR_CHAR` olarak `⁂` kabul edildiği için hayalet sınaması tofu yoluna
  hiç girmiyor; yukarıdaki kusur bu phase'in kabulünü etkilemiyor.
- Tanık: ebeveyn (`427c1f4`) worktree'de, 38 676 satır, fark boş.
- **Set kapısı `/code-review` (medium, --fix)**, iki bulgu giderildi
  (`freetype.rs`): cascade `UnicodeCoverage::NoTrim` — `Trim` renkli fontu
  (metin fontu Noto'nun kapsamını örtünce) ve bir diziyi bütünüyle
  kapsayan fontu listeden atıyordu; yalnız bitmap (PCF/BDF) aile adıyla
  açılmıyor (`open` "bulunamadı" der, `open_named` varsayılan yol) — seçilince
  her glyph kutu, hücre bir piksel oluyordu.
- **Waive (low):** değişken fontta (`JetBrains Mono VF`) `derive`'ın stil
  kapısı dosyanın varsayılan örneğinin bitlerini görüyor olabilir, o zaman
  kalın yüz düz yüze çöker; doğrulanmadı ve fontconfig'in ağırlığına güvenmek
  sentetik kalın kuralını delerdi. Kabul: orkestratör, 2026-09-30.
- **Waive (low, `/audit` mercek 4 ile aynı):** bayt önbelleği `Weak`, yani
  kabul edilen yedeğin dosyası (Noto ~10 MB) her yeni yedek karakterde ana
  thread'de yeniden okunuyor — render yolunda dosya G/Ç. Bellek ↔ yeniden
  okuma dengesi phase-4'ün kayıtlı kararı; Linux'ta pencere yok, kullanıcı
  görmüyor. Çaresi kabul edilen yedek fontları atlasın ömrünce tutmak.
  Kabul: orkestratör, 2026-09-30.
