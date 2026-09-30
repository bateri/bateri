# Phase 7 — Metal'in sökümü, denetim ve sözleşme

## Özet

Bu phase'te yapılanlar:

- Metal kâhinini, `.metal` shader'larını, `build.rs`'i ve `bt-gpu`'nun
  platform bağımlılıklarını silmek.
- `make denetim`'i `bt-gpu`'nun platformsuzluğuna bağlamak.
- `make shader`'ı son hâline getirmek.
- Sözleşme belgelerini güncellemek.

Set kapısı bu phase'in commit'inden önce koşar.

_Requirements: R6.1, R6.2, R6.3_

_Kısıt: yazılan/taşınan kodun yorumları, doc-comment'leri ve tanı metinleri İngilizce (plan.md → Yaklaşım, dil kısıtı)._

## Değişiklikler

- **`crates/bt-gpu/src/renderer.rs`**
  - Metal kâhin modülü ve kâhin sahne listesinin Metal yarısı siliniyor.
  - Sahne listesi tek arka uçta **kalıyor mu** kodlayanın kararı: kalırsa
    anlamı "karenin bütün pipeline'ları birlikte" olur.
  - `metallib_is_embedded_and_valid` gidiyor. Ardılı phase-2'den beri var.
- **Silinenler**
  - `crates/bt-gpu/shaders/*.metal` ve `crates/bt-gpu/build.rs`.
  - `crates/bt-gpu/Cargo.toml`'dan `objc2*`, `dispatch2`, `block2` (dev
    dahil).
  - Workspace'te artık kimsenin kullanmadığı satır kalmışsa `Cargo.toml`
    yorumlarıyla birlikte o da gidiyor.
  - `bt-shell` hâlâ kullanıyorsa kalıyor.
- **`Makefile`**
  - `shader` yalnız WGSL kanaryası: pipeline kurulum sınaması. `.metal` kolu
    ve `touch` gidiyor.
  - `denetim`'e yeni satır: `bt-gpu`'nun **doğrudan** normal bağımlılığında
    (`cargo tree -p bt-gpu -e normal --depth 1`) ve kaynağında (yorum hariç
    grep) `objc2`, `dispatch2`, `block2`, `metal` yok.
  - wgpu'nun dolaylı çektikleri bu kontrolün konusu değil; gerekçe yorumda.
  - phase-5b ((a)) açıldıysa istisnası adıyla: tek modül, `cfg(target_os =
    "macos")`.
- **`CLAUDE.md`**
  - Proje paragrafı: "AppKit ve Metal'e doğrudan" → Metal wgpu üzerinden.
  - Pipeline anlatısı (Metal adları: `MTLClearColor`, `BGRA8Unorm_sRGB`,
    `setViewport`, `setVertexBytes`) wgpu karşılıklarına. Sözleşmenin
    anlamı değişmiyor, adları değişiyor.
  - "Taban macOS 14" maddesi: `build.rs`'in shader payı gitti; minos kaynağı
    kalıyor.
  - Tek cümle: Xcode komut satırı araçlarının `metal` derleyicisi artık
    derleme şartı değil.
  - Komutlar bloğunda `make shader` satırı.
  - "Renk uzayı sınırı geçer" maddesinin bekçi adları güncel.
  - Katman tablosu: `bt-gpu` "platform kütüphanesi görmez, `wgpu`";
    denetimin yeni satırı.
- **`crates/bt-gpu/src/lib.rs`** başlığı — "Metal renderer" → wgpu. Altı
  pipeline anlatısı aynı anlamla.
- **`.claude/is-akisi/proje.md`**
  - Doğrulama: `.metal` / `build.rs` satırı WGSL satırına iniyor.
  - Mekanik denetim açıklamasına `bt-gpu` platformsuzluğu.
  - Riskli phase tetikleyicisi yalnız WGSL.
- **`docs/YOL-HARITASI.md`**
  - 040 satırı tek satıra iniyor.
  - "`bt-gpu` donuk" notu kalkıyor.
  - Font setinin satırına: kapının `bt-gpu`'yu Linux'ta derlemek için
    beklediği tek şey `bt-atlas`.

## Kabul

- `make hepsi` (yeni denetim satırı dahil), `make shader`, `make duman` ve
  `make kur` yeşil. `make kur` paketin shader'sız da tam olduğunu denetliyor.
- `git grep -n "MTL\|objc2_metal\|\.metal\b" crates/bt-gpu` boş. Bulunan
  varsa yalnız tarihçe anlatan yorumdur ve gerekçelidir.
- `make linux` yeşil. `bt-core` değişmediyse koşulmaz, `[~]` gerekçesiyle.

## Checklist

- [x] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [x] Metal kâhini, `.metal`'ler, `build.rs` silindi
- [x] `bt-gpu` platform bağımlılıkları (dev dahil) ve kullanılmayan workspace satırları gitti
- [x] `make shader` WGSL-only; `make denetim` `bt-gpu` platformsuzluk satırı (+ (a) istisnası gerekiyorsa)
- [x] `CLAUDE.md`, `bt-gpu` başlığı, `proje.md`, `docs/YOL-HARITASI.md`
- [x] Doğrulama geçti (`make hepsi` + `make shader` + `make duman`; `make kur` [~] — orkestratörün talimatı: kurma, paketi etkileyen girdi de değişmedi)

## Uygulama Notları

- **Dosyaların son adları:** ürün renderer'ı `renderer.rs` (eski `wgpu_renderer.rs`);
  `CellMetrics`/`FontNotice`/`family_notice` `metrics.rs`'e; piksel bekçileri
  `renderer/tests.rs` (yol `crate::renderer::tests` değişmedi), wgpu iç yapısına
  bakan bekçiler `renderer/wgpu_tests.rs`. `scissor_rect_below` tek tüketicisine
  (`scissor_below`) eridi.
- **Sahne listesi kaldı** ve anlamı "karenin bütün pipeline'ları birlikte":
  kâhin karşılaştırması yerine `every_scene_draws_all_its_pipelines_together`
  her sahneyi doğrulama kapsamında çiziyor ve clear renginden başka piksel
  istiyor — tek-pipeline bekçilerinin görmediği bileşik kareler (üç viewport,
  bant + caret, efektler) korunuyor.
- **Ölçüm kancası tek arka uç:** `offscreen_frame_loop` (eski
  `…_on_both_backends`); satır `arka_uc=wgpu` olarak kaldı (jeton silinmez).
  `olcum.md` ve `docs/OLCUMLER.md`'nin yöntem satırları güncellendi, Metal
  satırları tarihli kayıt.
- **Kâhinin gerekçeleri taşındı:** silinen `encode_*`/`AtlasTexture`/`pipeline`
  doc'larının arka uçtan bağımsız "neden"leri `renderer.rs`'e (`Renderer::plan`,
  `glyph_draws`, `fx_draw`, `Gpu`'nun alanları, `pipeline`), `.metal`'lerinki
  `.wgsl`'lere İngilizce aktarıldı; Metal API mekaniği (`endEncoding`,
  `replaceRegion`, tampon indeksleri) düştü.
- **`GpuError` iki varyanta indi** (`NoAtlas`, `Wgpu`): ötekiler yalnız Metal'in
  kurucularıydı; `pub enum` varyantı `dead_code` vermediği için elle arandı.
- **Çeviri kapsamı:** `renderer/tests.rs`, `link.rs`, `metrics.rs`, `lib.rs`,
  `stats.rs` (Metal tamamlanma bloğu cümleleri gerçeğe göre yeniden yazıldı),
  `error.rs`, `slots.rs` başlığı; kodun yorumsuz hâli çeviriden önceki hâliyle
  karşılaştırıldı (aynı). `frame.rs`/`glyph_fx.rs`'te yalnız `.metal` → `.wgsl`
  işaretçileri.
- **`Cargo.lock` değişti (R6.1'in beklenen sonucu):** `bt-gpu`'nun dev
  bağımlılıkları gitti; `objc2-metal`'in açtığı `dispatch2`/`objc2-core-foundation`
  feature'ları birleşmeden düştü. Grafa giren ya da sürüm değiştiren crate yok.
  Workspace'ten `objc2-metal` satırı silindi (kullanan üye kalmadı).
- **Duman kapısı (phase-6'dan devralınan):** süreli koşuda pencere
  `NSFloatingWindowLevel`'da açılıyor (`TerminalWindow::float_for_timed_run`,
  yalnız `BT_RUN_SECONDS` yolunda). Etkinleştirme seçilmedi: `activate()` macOS
  14'ten beri işbirlikçi, zorlamak kullanıcının klavyesini çalardı; seviye
  yalnız istif sırasını değiştiriyor ve örtülme bildirimi "görünür" kalıyor.
  `make duman` (cargo run yolu) yeşil: `kare=29 … icerik=2 … sessiz=1752ms`.
  Ekran uykudayken pencere yine örtülü sayılıyor — ortam, kod değil (`proje.md`).

- **Set kapısı `/code-review` (medium, 4 düşük bulgu):** (1) device wgpu'nun
  taşınabilir 8192 px doku tavanıyla isteniyordu — ekranlara yayılan pencerede
  yüzey `configure`'u düşerdi; tavan artık adaptörün (`max_texture_dimension_2d`),
  immediate bütçesi yine 128. (4) `make shader` tek kanarya: `--exact` + "1 passed"
  aranıyor, ad değişince sessiz yeşil yok. (3) tanı metinlerinin İngilizcesi
  bulgu değil: `CLAUDE.md` → Dil'in 040 istisnası. (2) **Waive:** `Fault` device
  başına tek nesil; bir pane'in yakalanmamış hatası öbür pane'lerin uçuştaki
  karesini de düşürür (`kare=` bir eksik, `Retry` bir fazladan kare ister).
  Paylaşılan device'ta yakalanmamış hatanın hangi pane'e ait olduğu bilinmiyor
  (wgpu'nun işleyicisi kaynağı söylemiyor); pane başına ayırmak hata kapsamlarını
  `configure`/`present`'e de yaymayı ister. Yol nadir, bedeli bir kare ve bir
  stderr satırı; kullanıcının göreceği bir fark yok.
- Kalıntı iki "metallib" cümlesi (`bt-shell` `pane.rs`, `app.rs`) düzeltildi.
