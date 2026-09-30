# Font sistemi soyutlaması ve Linux font yığını — Tartışma

**Ne** sorusunun cevabı kullanıcının (040 → "Kullanıcı kararları" 1–4: Linux,
winit, Wayland birincil; `freetype`, `fontconfig`, `harfrust`/`rustybuzz`
onaylı) ve yol haritası satırının: `FontSystem` trait'i, macOS'ta CoreText,
Linux'ta FreeType + fontconfig (`FcFontSort`) + HarfBuzz ailesi; kapı mantığı,
atlas ve yordamsal çizim dokunulmaz; kapı `bt-atlas` ve lavapipe üstünde
`bt-gpu` ile büyür. Bu dosya **nasıl** sorusunu karar-listesi olarak kuruyor.
Kullanıcının kısıtı: macOS'ta hiçbir fark görünmemeli.

## Karar 1: Soyutlamanın biçimi

Üç yol:

- **(a) İlişkili tipli trait + `cfg` tip takma adı (statik dağıtım).**
  `trait FontSystem { type Font; type Glyph: Copy; … }`, iki gerçekleme
  (`coretext::CoreText`, `freetype::FreeType`), `bt-atlas` içinde tek satır
  `type Backend = …` `cfg(target_os)` ile. `Atlas` generic **değil**: alanları
  `<Backend as FontSystem>::Font` tutar, dış API (`Atlas`, `Metrics`,
  `FontIssue`, `monospaced_families`, `family_issue`) harfi harfine aynı.
- **(b) Trait'siz `cfg` modül takası.** `mod coretext` / `mod freetype` aynı
  adlı serbest fonksiyonları ihraç eder, çağıranlar `backend::glyph_index(..)`
  der.
- **(c) `dyn FontSystem`.** Çalışma zamanında seçim.

**Öneri (a).** Sözleşme tek yerde yazılı: trait'in **imzası** platformsuz
kod ve macOS'ta da derleniyor, yani platformsuz çağıranların arka uçtan
istediği yüzey bir tipte adlı duruyor. (Gerçeklemenin trait'e uyup uymadığını
Linux kolunda yine yalnız `make linux` görür — bu (b)'de de aynı; (a)'nın
kazancı sözleşmenin **adı**, erken uyarı değil.) (b) sözleşmeyi çağrı
yerlerinin toplamına bırakır. (c)'nin kazancı yok: bir süreçte iki font
sistemi olmuyor. (a) monomorfize olur; macOS derlemesi bugünkü çağrıların
aynısını üretir. `bt-gpu` ve `bt-shell` değişmez (`Atlas` generic olsaydı
`bt-gpu`'nun `Renderer`'ı tip parametresi taşırdı, reddedildi).

## Karar 2: Platformsuz yarının sınırı ve tipleri

`font.rs`'in kural yarısı (`context.md` → Mevcut Durum) CoreText'ten
ayrılıp platformsuz modüllere taşınır — **mantığı değişmeden**:
`centre_shift`, `ink_fits_placed` (+ `ink_fits_box`'ın trait üstünden
gövdesi), `fit_ratio`, `accept`'in sırası, `shrink`'in iki turu ve
`SHRINK_STEPS`, `SHRINK_LIMIT`, `Accepted` + `rise`, `metrics()`'in hücre
formülü (fontun ham ölçülerini trait'ten alarak), `rule_envelope`,
`round_up`, `Faces` + `effective`, zincirin istenen-aile kolu (`open_chain`,
`same_family`; varsayılan zincirin kendisi arka ucun, Karar 3.1),
`unpremultiply`.

Tipler: `CGFloat` yerine `f64`, `CGRect` yerine crate'in kendi `InkRect`'i
(`x, y, width, height: f64`). **Bit eşitliğinin gerekçesi:** macOS'ta
`CGFloat` `f64`'ün takma adı, yani aynı işlemler aynı sırada aynı sonucu
verir; `CGRect`'ten `InkRect`'e kopya dört `f64`'ün taşınması, yuvarlama
yok. Kural değişmediği için taşınan kod `accept`'in sırasını ve
`centre_shift`'in iki tüketicisini (kapı + çizim) aynen koruyor.

Modül adları ve dosya düzeni phase'in işi; ölçüt "CoreText tipi yalnız
`coretext` modülünde görünür" ve `make denetim`'e bağlanabilecek kadar
mekanik (Karar 8).

## Karar 3: Trait'in yüzeyi

Arka ucun vermesi gereken ilkel işlemler — kural içermeyen, yalnız fontu
soran çağrılar:

1. `open(name, px) -> (Font, String)` — aile adıyla açar ve **dönen** aileyi
   verir (CoreText de fontconfig de "hata vermez", ikisi de en yakını verir;
   istenen ailenin `same_family` sınaması platformsuz kalır).
   `open_default(px) -> (Font, String)` — **arka ucun** varsayılan zinciri:
   CoreText'te `PREFERRED` → `FALLBACK` ve ikamenin tanısı bugünkü gövdeyle
   aynen; FreeType'ta `monospace` takma adı ve fontconfig'in çözdüğü aile
   (takma adın sınanacak bir adı yok — `open_default`'un `returned !=
   FALLBACK` kontrolü Linux'ta her açılışta yanlış bir satır basardı).
   `PREFERRED`/`FALLBACK` CoreText modülüne iniyor; platformsuz `open_chain`
   yalnız istenen aileyi sınıyor ve geri düşüşte arka uca soruyor.
2. `derive(regular, Face) -> Option<Font>` — yüz türetme, **istenen trait'i
   edindiyse** (`derive_face`'in iki kapısı).
3. `is_monospaced(&Font) -> bool` ve `families() -> Vec<String>` —
   `monospaced_families`'in aday listesi; süzgecin kendisi (`same_family` +
   `is_monospaced`) platformsuz.
4. `glyph(&Font, char) -> Option<u32>`, `advance(&Font, u32) -> f64`,
   `ink(&Font, u32) -> InkRect`. Glyph numarası **somut `u32`** (CoreText'in
   `CGGlyph`'i `u16`, kayıpsız genişliyor); trait'in tek ilişkili tipi
   `Font: Clone` (`Faces::derive` onu dört kez klonluyor).
5. `raw_metrics(&Font) -> RawMetrics` — ascent, descent, leading, alt çizgi
   konumu/kalınlığı, x-yüksekliği; hücre formülü platformsuz.
6. `cascade(&Font, &str) -> Option<Font>` — `Option`, çünkü fontconfig'in
   `.LastResort`'u yok ve kimsenin çizemediği karakterde "aday yok" cevabı
   burada doğabilir (CoreText kolu bugünkü gibi hep `Some`).
7. `at_size(&Font, px) -> Font`, `size(&Font) -> f64` — `shrink`'in kopyası.
8. `is_last_resort(&Font) -> bool` — sözleşmedeki anlamı "arka ucun
   küçültülmemesi gereken son çare fontu"; CoreText'te PostScript adıyla
   (gerçeklemenin iç ayrıntısı), FreeType'ta `false` (fontconfig'in son çare
   fontu yok, "aday yok" cevabı 6. maddede doğuyor).
9. `has_color_glyphs(&Font) -> bool` — düzlem kararı.
10. `shape(&Font, &str) -> Option<(Font, Glyph)>` — dizinin tek glyph'e
    şekillenmesi ve glyph'i **gerçekten üreten** font; kapı (`accept`)
    platformsuz tarafta.
11. `draw_mask(&Font, Glyph, Metrics, x, baseline, &mut [u8])` ve
    `draw_color(…)` — konum platformsuz taraftan **hesaplanmış** gelir
    (`centre_shift - x_offset`, `cell_h - baseline_px + rise`), arka uç yalnız
    boyar. Tek formül iki tüketici kuralı (kapı + çizim) böylece arka uca
    sızmıyor. `baseline` bugünkü gibi **yuvanın altından** ölçülür (CG'nin
    koordinatı; CoreText kolu bit bit aynı kalsın diye formül değişmiyor),
    FreeType kolu onu üstten ölçülen satıra kendisi çevirir.

`DrawResult::NoContext` CoreText'e özgü bir başarısızlık ama varyant
kalıyor: FreeType'ın yükleme/çizim hatası aynı teşhis kovasına düşer.

## Karar 4: Bit eşitliğinin tanığı

Kullanıcının kısıtı bir cümleyle değil bir karşılaştırmayla kapanır, ve
tanık tanıklık ettiği refactor'dan **bağımsız** olmak zorunda:

- **Yalnız public API'den okuyan ayrı bir sınama dosyası**
  (`crates/bt-atlas/tests/raster_digest.rs`, `cfg(target_os = "macos")`,
  `#[ignore]`): `Atlas::new` → `metrics()`, `font_issue()`, sabit bir
  envanterin her `slot()` cevabı (`Placed`: yuva, yarı, düzlem) ve
  `Upload`'ın baytları (sol + sağ). Envanter: ASCII × dört yüz, küçük sınıf,
  taban fontta olmayan yedek, küçültülen yedek (`⧉`), geniş karakter, emoji,
  küme, yordamsal aile, kurallar, tofu, olmayan bir aile ve orantılı bir aile;
  13/16 pt × @1x/@2x, iki satır aralığı. Satır başına bir anahtar + özet
  basar, yani fark hangi sprite'ta olduğunu söyler. `mod tests`'e değil ayrı
  dosyaya, çünkü Karar 7 `lib.rs`'in sınamalarını yeniden yazıyor.
- **Karşılaştırma ebeveyne karşı, aynı makinede, aynı anda:** `git worktree`
  ile ebeveyn commit'te ve çalışma ağacında koşup iki çıktının `diff`'i boş.
  Saklanan bir özet yok — set sürerken gelen bir macOS güncellemesi fontları
  değiştirirse saklanan özet yanlış alarm verirdi.
- **`make tarama` tanık değil:** `census.rs` iç yapılara uzanıyor ve bu
  refactor'la birlikte düzenleniyor, çıktısı da blok başına sayım (telafi
  eden iki değişiklik farkı boş bırakır). Yardımcı sinyal olarak koşabilir,
  kabul ölçütü değil.

Tanık kendi phase'inde (phase-0) ve refactor'dan önce giriyor; makinedeki
fontlara bağlı olduğu için kapıya girmiyor, her macOS phase'inin kabul
ölçütü.

## Karar 5: Linux yığınının crate'leri ve bağlanma biçimi — ⚠️ eskalasyon

Onaylı liste aile adlarını veriyor (`freetype`, `fontconfig`,
`harfrust`/`rustybuzz`); crate/sürüm seçimi, geçişli crate'ler ve sistem
kütüphanesine bağlanma biçimi listede yok. Öneri:

- **FreeType: `freetype-rs` 0.38** (servo'nun `freetype` 0.8'i yerine).
  Güvenli sarmalayıcı (`Face`, `GlyphSlot`, `Bitmap`), yani `unsafe` yalnız
  sarmalayıcının açmadığı yerde (`FT_Outline_Translate`, `FT_Select_Size`,
  OS/2 tablosu — `face.raw()` üstünden). servo'nunki ham bağlama; her çağrı
  `unsafe` olurdu.
- **fontconfig: `fontconfig` 0.11** (yüksek düzey; `sort_fonts`, `CharSet`
  API'de). Ham bağlamaya (`yeslogic-fontconfig-sys`) doğrudan kenar
  gerekirse bu `polling` emsali olur ve ayrıca kaydedilir.
- **Şekillendirme: `harfrust` 0.13** (`rustybuzz` 0.20 yerine). HarfBuzz
  ekibinin Rust portu ve etkin olan dal; `rustybuzz` bakım kipinde. MSRV 1.85
  ≤ 1.88.
- **Bağlanma: `pkg-config` ile dinamik**, `bundled` ve `dlopen` kapalı.
  `freetype-sys` bulamazsa derleme düşer (sessiz bundled yok, `context.md` →
  Kanıt); dağıtım paketleri (`.deb`) sistem kütüphanesine bağımlılık yazar,
  AppImage zaten gömer. Bundled C derlemesi `cc` ile FreeType + libpng'yi her
  temiz derlemede derlerdi.
- **Geçişli yeni crate'ler (listede adı yok):** `freetype-sys`, `libz-sys`
  (normal bağımlılık; zlib'i pkg-config'le bulamazsa **kendisi derleyebilir**
  — imaja `zlib1g-dev` bunu kapatmak için), `cc` ve `pkg-config` (build),
  `yeslogic-fontconfig-sys`, `dlib`, `once_cell`, `read-fonts`, `font-types`,
  `bytemuck`, `smallvec`. Hepsi MIT/Apache-2.0/Zlib — GPL-3.0-or-later ile
  uyumlu.
- **Hedefe koşullu:** üçü de `[target.'cfg(target_os = "linux")'.dependencies]`
  altında, yani macOS derlemesi ve ürün grafı onları görmüyor. `Cargo.lock`
  platformsuz olduğu için satırlar kilide **girer** (`make denetim` uyarır;
  kayıt bu karar). `tools/third_party_notices.py` `cargo tree`'yi ana makinenin
  hedefinde koşuyor, yani macOS paketinin `THIRD-PARTY-LICENSES.txt`'i
  değişmez; Linux'un bildirim dosyası paketleme setinin işi.
- **Sistem kütüphanelerinin lisansı:** FreeType FTL/GPLv2 çift lisanslı (FTL
  GPLv3 ile uyumlu), fontconfig MIT benzeri; dinamik bağlanıyor.

## Karar 6: Linux'ta çizim ve zincir kararları (yalnız Linux'ta görünür)

- **Varsayılan zincir:** istenen aile → fontconfig'in `monospace` takma adı
  (kullanıcının sistem yapılandırmasının seçtiği eşaralıklı font; imajda
  DejaVu Sans Mono). `PREFERRED` Linux'ta boş. `FamilyNotFound.using`
  fontconfig'in çözdüğü aile.
- **Eşaralıklılık ölçütü:** `FT_IS_FIXED_WIDTH` (fontun kendi bayrağı) —
  CoreText'in `TraitMonoSpace`'inin karşılığı; aday listesi `FcFontList`
  (`spacing = mono`) ön süzgeç, son söz yine platformsuz süzgeçte.
- **Yüz türetme:** aynı aile + `weight = bold` / `slant = italic` ile
  fontconfig eşleşmesi; dönen yüzün stili istenen trait'i taşımıyorsa `None`
  (sessiz ikamenin aynı kapısı).
- **Cascade:** `FcFontSort` taban desenle atlas başına **bir kez**, karakter
  başına sıralı listede charset'i karakteri içeren ilk font. Anahtar başına
  arama atlasın ömründe yine bir kez (negatif önbellek bugünkü).
- **Raster:** hinting yok (`FT_LOAD_NO_HINTING`), gri AA, LCD yok; kesirli
  yatay konum dış hattın `FT_Outline_Translate` ile 26.6'da ötelenmesiyle.
  Gerekçe macOS'un modeli: hücre ilerlemesi kesirli ve `centre_shift`
  kesirli konum veriyor; hinting ilerlemeleri tam sayıya çeker ve kapının
  ölçtüğü ile çizilen ayrışırdı. Metrikler tasarım biriminden ölçeklenir
  (FreeType'ın yuvarlanmış `size->metrics`'i değil), CoreText'in kesirli
  değerleri gibi. Mürekkep kutusu dış hattın **tam** sınırı (`FT_Outline_Get_BBox`).
  Punto kuralı aynı: piksel = punto × ölçek, 72 dpi.
- **Renkli glyph:** Noto Color Emoji bookworm'da `CBDT` bitmap; `FT_LOAD_COLOR`
  ile en yakın strike yüklenir ve hedef puntoya **kendi** yeniden
  örneklememizle (ön çarpımlı uzayda alan ortalaması, sonra
  platformsuz `unpremultiply`) indirilir. Bitmap fontta `at_size` bir ölçek
  katsayısı ve o katsayının **tek sahibi** fontun kendisi: kapının ölçtüğü
  mürekkep/ilerleme ile çizimin ölçeği aynı sayıdan (yoksa `centre_shift`'in
  iki tüketicisi Linux'ta ayrışırdı). Örnekleyicinin sınırı yazılı: yalnız
  küçültür (Noto'nun strike'ı 109 px, hücre hep küçük), ön çarpımlı alfa
  üstünde alan ortalaması, filtre seçeneği yok. **Bilinen sınır:** FreeType 2.12 `COLRv1` çizmiyor; yalnız
  `COLRv1` taşıyan emoji fontu maske düzlemine ya da kutuya düşer.
- **Tek bayt kaynağı:** fontconfig'in dosyası font başına **bir kez** belleğe
  okunur; FreeType onu `new_memory_face` ile açar, `harfrust` aynı tamponu
  okur. Şekillendirmenin glyph numarası böylece çizen fontla aynı dosyadan
  gelir (macOS'taki "run'ın kendi fontu" kuralının karşılığı).
- **Küme:** `harfrust` cascade'den gelen fontla şekillendirir; tek glyph'e
  inmeyen dizi bugünkü gibi taban karaktere düşer.
- **`SHRINK_LIMIT` Linux'ta ölçülmeden kullanılır.** Değeri Apple Color
  Emoji'nin ve `.LastResort`'un geometrisinden (041); Linux'un dağılımı
  (`make tarama`'nın Linux kolu) bu setin dışında, bilinen sınır.

## Karar 7: Sınamaların bölünmesi

- **Değişmez bekçileri platformsuz:** sınama gövdesi fontun adını ve taban
  fontun **hangi karakteri taşıdığını** bilmiyor;
  arka uç başına `cfg(test)` bir fikstür örnek karakterleri (taban fontta
  olmayan yedek, geniş karakter, emoji, küme) ve varsayılan ailenin adını
  veriyor. Örnek: `the_gate_decides_by_ink_alone`, `same_char_gets_same_slot`,
  yordamsal ailenin bütün değişmezleri, `a_wide_char_takes_two_slots_in_one_answer`.
- **Kalibrasyon sınamaları `cfg(target_os = "macos")`:** belirli bir fontun
  ölçüsünü ya da adını bekleyenler (Menlo'nun `☕`'si, Monaco'nun tek yüzü,
  Helvetica'nın orantılılığı, STIX'in `⏺`'u, 041'in araç karakterleri).
  Sınıflama phase'in çıktısı; kural "gövde bir aile adı, ölçülmüş bir sayı
  ya da taban fontun belirli bir karakteri taşıdığı varsayımını taşıyor mu".
- `census.rs` macOS'a özgü kalır (tarama CoreText'in cascade'ini döküyor):
  `lib.rs`'teki bildirimi `cfg(all(test, target_os = "macos"))`.
- `bt-gpu`'da aynı kural ve kapsam **bütün glyph'li sınamalar**: üç aile
  adlı assert (`renderer/tests.rs:129–138`), taban fontun `☕`'sine dayanan
  `a_wide_cell_that_fits_one_cell_stays_one_quad` (`:2532–2560`), renk
  düzlemi ve emoji yolu (`:2666–2814`), `wgpu_tests.rs`'in `scene_wide`'ı
  (`:613`) ve Apple Color Emoji'ye göre gerekçelenen `SCALE = 2.0`
  (`:40–43`). Örnek karakterler `bt-atlas`'ın fikstürüyle aynı kaynaktan.

## Karar 8: Kapının büyümesi — ⚠️ imaj paketleri eskalasyon

- **`make linux`:** `LINUX_CRATES = -p bt-core -p bt-atlas -p bt-gpu`; yorum
  ve `proje.md` → Doğrulama satırı ("bugün `bt-core`") güncellenir.
- **İmaja eklenecek Debian paketleri** ve her birini isteyen şey:
  `pkg-config`, `libfreetype-dev`, `libfontconfig-dev`, `zlib1g-dev`
  (derleme; Karar 5), `fonts-dejavu-core` (varsayılan zincir ve dört yüz),
  `fonts-noto-color-emoji` (renk düzlemi ve küme sınamaları),
  `mesa-vulkan-drivers` + `libvulkan1` (lavapipe: `bt-gpu`'nun piksel
  sınamaları ve `wgsl_pipelines_build`); `fc-cache` imaj kurulurken. Taban
  font seçimi Linux kolunun kalibrasyonu, macOS'u etkilemiyor.
- **`bt-gpu`'nun yalnız `cfg` düzenlemeleri:** `Backends::METAL` macOS'ta
  kalır, Linux'ta `Backends::VULKAN` (genelleştirilmiş `PRIMARY` değil —
  macOS'ta adaptör sayımını değiştirirdi); `Surface::from_layer`
  `cfg(target_os = "macos")` (wgpu'nun `CoreAnimationLayer`'ı `cfg(metal)`);
  test assert'leri Karar 7'ye göre.
- **Önce fikstür, sonra arka uç:** Linux'ta düşen bir `bt-gpu` sınaması
  önce fonta mı arka uca mı bağlı diye ayrılır; font farkı fikstürle
  (Karar 7) kapanır, beklenti gevşetilerek değil.
- **Lavapipe'ta piksel beklentisi:** tam bayt bekleyen bir sınama Vulkan'da
  düşerse karar **bilerek** verilir — beklenen değer arka uç başına mı,
  toleranslı karşılaştırma mı — ve Uygulama Notları'na yazılır; kırmızıyı
  susturmak için beklenti gevşetilmez. `cell_bg_paints_pixels_on_the_gpu`'nun
  ara ton tanığı (`CLAUDE.md` → Renk uzayı) iki arka uçta da aynı anlamı
  taşımalı.
- **`bt-atlas/Cargo.toml`:** `objc2-core-*` üçlüsü
  `[target.'cfg(target_os = "macos")'.dependencies]`'e iner (bugün koşulsuz;
  Linux'ta derlemenin ilk engeli).
- **Bilinen yükler:** `bt-gpu` `make linux`'a girince her `bt-gpu` phase'i
  Docker ister (proje.md → Doğrulama tetiği) ve wgpu'nun Linux derlemesinin
  süresi ölçülmedi; imajın apt sürümleri pin'li değil, yani imaj yeniden
  kurulunca Noto/mesa sürümü kayabilir — Linux değişmezleri bu imajın
  fontlarıyla anlamlı; Linux'un üçüncü taraf bildirim dosyası paketleme
  setinin borcu (`third_party_notices.py` `aarch64-apple-darwin`'de koşuyor,
  macOS'unki değişmiyor).
- **`make denetim`:** CoreText/CoreGraphics adları `bt-atlas`'ta yalnız
  macOS arka ucunun dosyasında (katman kontrolünün yanına tek grep).

## Karar 9: Phase bölümü

0. **Tanık** — Karar 4'ün sınama dosyası; kodla ilgisi yok, ayrı commit.
1. **Yerinde çeviri** — `font.rs`, `raster.rs`'in platforma bağlı yarısı
   (iki çizim fonksiyonu, `unpremultiply`, `DrawResult`) ve `census.rs`'in
   yorumları, doc'ları, tanı metinleri İngilizceye; kod aynı. Kanıt: yorum
   satırları soyulmuş kaynağın farkı yalnız dizgi değişimi. Dil kısıtının
   kapsamı taşınan kod; `lib.rs` ve yordamsal çizim yerinde kalıyor ve
   yalnız yeni yorum İngilizce.
2. **Taşıma + tip** — Karar 2; CoreText çağrıları yerinde, `--color-moved`
   okunur fark. Tanık eşit.
3. **Trait ve CoreText arka ucu** — Karar 1 ve 3, `raster.rs`'in iki çizim
   fonksiyonu arka uca, `objc2` bağımlılıkları macOS hedefine, sınamaların
   sınıflanması (Karar 7), `census` `cfg`'i. Tanık eşit. Hâlâ yalnız macOS,
   yeni bağımlılık yok.
4. **Linux maske yolu** — `freetype-rs` + `fontconfig`, zincir, yüzler,
   metrik, mürekkep, cascade, maske çizimi; imaj (derleme paketleri +
   DejaVu), `make linux` += `bt-atlas`. `draw_color` ve `shape` bu phase'te
   **adlı taslak** (renkli aday ve dizi tofu/taban karaktere düşer), renk ve
   küme sınamaları Linux'ta `cfg`'li bekliyor.
5. **Linux renk ve küme** — `FT_LOAD_COLOR` + örnekleyici, `harfrust`, tek
   bayt kaynağı; Noto Color Emoji imaja; Linux'ta atlasın bütün değişmezleri
   yeşil.
6. **`bt-gpu` lavapipe'ta** — Karar 8'in `cfg` düzenlemeleri ve fikstür,
   mesa paketleri, `make linux` += `bt-gpu`; `CLAUDE.md` katman tablosu ve
   bağımlılık paragrafı, `bt-atlas`'ın başlık yorumu, `proje.md` Doğrulama
   satırı, `Makefile`/Dockerfile yorumları.

Her phase `make hepsi` ile yeşil biter; 4–6 ayrıca `make linux` ile.

## Muhakeme (2026-09-30)

| Mercek | Verdict |
|---|---|
| Sadelik | TEMİZ — `Glyph` ilişkili tipi gereksiz; Linux'ta font dosyası iki yoldan yükleniyor; CBDT örnekleyicisinin sınırı yazılı değil |
| Codebase-fit | SORUNLU — `open_default`'un dönen-ad sınaması `monospace` takma adıyla çakışıyor; `objc2` bağımlılıkları ve `census` Linux derlemesini engelliyor; `bt-gpu`'nun taban fonta dayanan sınamaları sayılmamış |
| İşletme | SORUNLU — tanık iç API'ye bağlı ve dar (metrik/tanı/düzlem yok), `make tarama` tanık değil, saklanan özet yanlış alarm; phase-1 taşıma + tip + çeviriyi tek commit'te incelenemez kılıyor; `bt-gpu`'nun fonta bağlı sınamaları eksik |

**Kabul edilen itirazlar → plan değişikliği:**
- Sadelik → `Glyph` ilişkili tipi → somut `u32`, tek ilişkili tip `Font: Clone` (Karar 3.4).
- Sadelik → iki yükleme yolu → tek bayt tamponu, `new_memory_face` + `harfrust` (Karar 6).
- Sadelik → örnekleyici → yalnız küçülten alan ortalaması, katsayının tek sahibi font (Karar 6).
- Sadelik → `is_last_resort`'un sözleşme adı → "küçültülmemesi gereken son çare" (Karar 3.8).
- Codebase-fit → takma ad çakışması → `open_default` arka ucun, `PREFERRED`/`FALLBACK` CoreText modülünde (Karar 3.1).
- Codebase-fit → derleme engelleri → `objc2` üçlüsü macOS hedefine, `census` `cfg(all(test, macos))` (Karar 7, 8).
- Codebase-fit → `baseline`'ın koordinatı → alttan, FreeType kolu çevirir (Karar 3.11).
- Codebase-fit + İşletme → `bt-gpu`'nun fonta bağlı sınamaları → kural "taban fontun karakteri" kolunu da kapsıyor, liste satırlarıyla; önce fikstür sonra arka uç (Karar 7, 8).
- İşletme → tanık → ayrı dosya, yalnız public API, metrik + tanı + `Placed` + baytlar, ebeveyne karşı `git worktree`'yle aynı anda; `make tarama` yardımcı sinyal (Karar 4).
- İşletme → phase-1'in yükü → tanık (0), yerinde çeviri (1), taşıma + tip (2) ayrı phase'ler; çevirinin kapsamı taşınan kod (Karar 9).
- İşletme → kısmi uygulama → phase-4'ün taslakları adıyla (Karar 9).
- İşletme → yük envanteri → Docker süresi, imajın pin'siz apt'si, Linux bildirim dosyası "bilinen yükler" olarak (Karar 8).
- Codebase-fit + Sadelik → Karar 1'in gerekçesi abartılıydı (vtable, erken uyarı) → düzeltildi, seçim değişmedi.

**Reddedilenler:**
- İşletme → phase-3'ü (şimdi 4) "bağımlılık + kilit + imaj, kod yok" ve "FreeType maske" diye ikiye bölmek — tüketicisi olmayan bağımlılıklar doğrulanabilir bir durum değil yarım bir durum; phase zaten kilit dosyası yüzünden riskli phase (`/code-review` kutusu), inceleme yükü orada karşılanıyor.

## Karar (2026-09-30, otonom akış)

- **Seçilen:** Karar 1–9'daki öneriler, Muhakeme'nin kabul ettiği
  değişikliklerle: statik trait (`Font: Clone`, glyph `u32`) + `cfg` takma adı,
  kural yarısı platformsuz ve mantığı değişmeden, public API'den okuyan ve
  ebeveyne karşı koşan bit-eşitlik tanığı, Linux'ta `freetype-rs` +
  `fontconfig` + `harfrust` pkg-config'le dinamik, yedi phase (0–6).
  Panelden geçmiş otonom öneri; ne yapılacağı (Linux, onaylı bağımlılık
  aileleri, macOS'ta fark yok) kullanıcı kararı.
- **Eskalasyon (kullanıcı onayı bekleyen, phase-4'ten önce):** Karar 5'in
  crate/sürüm seçimleri (`freetype-rs` 0.38, `fontconfig` 0.11, `harfrust`
  0.13) ve geçişli crate listesi, pkg-config ile dinamik bağlanma, Karar 8'in
  imaj paketleri (`pkg-config`, `libfreetype-dev`, `libfontconfig-dev`,
  `zlib1g-dev`, `fonts-dejavu-core`, `fonts-noto-color-emoji`,
  `mesa-vulkan-drivers`, `libvulkan1`). Phase 0–3 yalnız macOS'ta ve yeni
  bağımlılık getirmiyor, onaydan bağımsız ilerleyebilir.
- **Reddedilen:** (b) trait'siz `cfg` modül takası — sözleşmenin adı yok;
  (c) `dyn` — kazancı yok; servo `freetype` — her çağrı `unsafe`;
  `rustybuzz` — bakım kipinde; bundled/dlopen bağlanma — her temiz derlemede
  C derlemesi ya da çalışma zamanında sessiz yokluk; saklanan özet ve
  `make tarama` tanık olarak — yukarıda.
