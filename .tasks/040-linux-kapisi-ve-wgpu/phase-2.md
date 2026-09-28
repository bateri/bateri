# Phase 2 — wgpu denemesi: `cell_bg` + caret, kâhinle karşılaştırma ve ölçüm

## Özet

`cell_bg` pipeline'ını ve caret SDF'ini WGSL'de, wgpu'nun Metal arka ucunda,
**ürün grafının dışında** (dev-dependency, `cfg(test)`) kurmak. Bu phase
dört iş yapıyor:

- bekçilerin wgpu ikizlerini yazıp Metal kâhiniyle çapraz karşılaştırmak;
- ölçüm kancasını kurmak;
- ölçümü alıp durak kuralını uygulamak;
- `proje.md`'nin WGSL satırlarını aynı commit'te güncellemek.

_Requirements: R2.1, R2.2, R2.3, R2.4_

## Değişiklikler

### Bağımlılık

- **`Cargo.toml` (workspace)** — `wgpu` 30.x `[workspace.dependencies]`'e
  giriyor.
  - `default-features = false` + `std`, `wgsl`, `metal`, `vulkan`
    (`discussion.md` → Karar 10).
  - Yorum workspace örüntüsünde: kullanıcı onaylı karar (işaretçi bu setin
    Karar'ı), neden bu özellikler, kapalı arka uçlar, objc2 neslinin
    workspace'inkiyle aynı olduğu (`context.md` → Kanıt).
- **`CLAUDE.md`** — "Bağımlılık mimari karardır" maddesine `wgpu` tek cümleyle ve kararına işaretçiyle giriyor: bugün yalnız dev-dependency (deneme), ürün yoluna phase-5'te geçiyor. Kilit dosyası değiştiği commit'te sözleşme de güncel olmalı.
- **`crates/bt-gpu/Cargo.toml`** — `wgpu` yalnız
  `[dev-dependencies]`'de. Ürün binary'sinin grafı değişmiyor.

### wgpu renderer iskeleti

- **`crates/bt-gpu/src/` altında yeni `cfg(test)` modül(ler)i.** Ad ve bölme
  kodlayanın. İçerik:
  - Paylaşımlı test device'ı (bir kez kurulur).
  - `Waker::noop` ile küçük bir `block_on`.
  - Offscreen `Bgra8UnormSrgb` hedefe çizim.
  - `copy_texture_to_buffer` + `map_async` + bekleyen `poll` ile geri okuma.
  - `cell_bg` + caret pipeline'ları. `Frame`'in listeleri ve uniform
    değerleri **aynı** kaynaktan geliyor: `frame.rs`'e dokunulmuyor, iki
    renderer aynı `Frame`'i okuyor.
- **`crates/bt-gpu/shaders/cell_bg.wgsl` (yeni).** `cell_bg.metal`'in vertex'i,
  `cell_bg` fragment'i ve `caret_fragment`'i (SDF: yuvarlak köşe, kenar, hale,
  hale × caret alfası).
  - Küçük değerler `var<immediate>`.
  - Her yapının boyutu Vulkan'ın garanti ettiği immediate sınırıyla
    karşılaştırılıyor. Aşan yapı uniform buffer'a gidiyor; seçim ve sayı
    Uygulama Notları'na (`discussion.md` → Karar 5).
  - `#[repr(C)]` ↔ WGSL düzeni: `vec3` hizası ve uniform adım kuralları.
    Pipeline kuran sınama bekçi.

### Sınamalar

- **Bekçi ikizleri**, `renderer.rs`'in sınama modülünde ya da yeni modülde:
  - `MIDTONE` (`cell_bg_paints_pixels_on_the_gpu`'nun ikizi);
  - dejenere caret düz dörtgenle **bit bit** aynı, wgpu'nun **kendi içinde**;
  - köşe, içi boş kenar, hale taşar ama durur, hale caret'le söner.
  - Kaynak yorumu: bu ikizler phase-4'te `render_offscreen` wgpu'ya
    dönünce asıl bekçilere katılacak ve ikizlik kalkacak.
- **Kâhin sahne listesi.** Tek bir sınama, `Frame` üreten küçük fonksiyonların
  listesini iki arka uçta çizip karşılaştırıyor.
  - Düz dolgu pikselleri tam eşit, AA/SDF kenarı kanal başına ≤ 1/255.
  - Hata mesajı en büyük farkı ve yerini söylüyor.
  - Bugünkü sahneler: `MIDTONE` zemini, blok şeridi, dock zemini ve saç
    çizgisi, üç caret şekli, odaksız içi boş blok, hale.
- **Pipeline kurulum sınaması.** Her `.wgsl` naga'dan geçiyor ve pipeline
  kuruluyor (`metallib_is_embedded_and_valid`'in wgpu ardılı).
- **Ölçüm kancası.** `#[ignore]`'lu bir sınama: aynı sahne(ler)i N kare
  iki arka uçta çiziyor ve `Stats`'ın CPU aralıklarını topluyor, bugünkü
  `Stats` API'siyle. Kanca **basmıyor**; ne bastığı `/measure`'ın
  (`olcum.md`'ye yeni tür satırı: nasıl koşulur, hangi aralıklar).

### Sözleşme satırları (aynı commit'te, Karar 9)

- **`.claude/is-akisi/proje.md`**
  - Doğrulama: "`.wgsl` değiştiyse → `make shader`".
  - Riskli phase tetikleyicisi: `#[repr(C)]` ↔ WGSL.
  - Denetim merceği 6: WGSL düzeni (`vec3` 16 bayt hiza, uniform/immediate
    adım kuralları).
- **`Makefile`** — `shader` hedefi `.wgsl`'i de kapsıyor: pipeline kurulum
  sınamasını koşuyor. `.metal` kolu Metal sökülene kadar kalıyor.

### Ölçüm ve durak (kod değil, bu phase'in son adımı)

- Kullanıcının konuda istediği `/measure` koşusu iki şeyi ölçüyor:
  - offscreen iki arka uç karşılaştırması;
  - bugünkü **pencere yolunun Metal tabanı** (`BT_FRAME_STATS` +
    `BT_SCROLL_TEST`, release ve debug).
- Profil başına en az on koşu. Sonuç `docs/OLCUMLER.md`'ye giriyor. Sayı
  phase dosyasına yazılmıyor.
- **Durak kuralı** `discussion.md` → Karar 3. CPU aralıklarında wgpu'nun
  dağılımı Metal'inkiyle örtüşmüyor ve daha kötüyse set **burada durur**: eskale
  edilir ve phase-3 başlamaz. GPU sütunu raporlanır ama karar vermez.
- **Bu adım `[~]` olamaz.** Ölçüm `docs/OLCUMLER.md`'de değilse phase kapanmaz; otonom şerit burada durup eskale eder (waive değil). Kullanıcının "kötüyse eskale" cümlesi ölçümü zaten istiyor; atlamak durağı sessizce kaldırırdı.

## Kabul

- `make hepsi` yeşil: bekçi ikizleri, kâhin sahne listesi ve pipeline kurulum
  sınaması geçiyor.
- `cargo tree -p bateri -e normal` wgpu göstermiyor (ürün grafı değişmedi).
  `cargo tree -p bt-gpu -e dev` gösteriyor.
- `make shader` WGSL kolunu koşuyor.
- `docs/OLCUMLER.md`'de iki arka ucun offscreen dağılımı ve Metal pencere
  yolu tabanı var. Durak kuralının sonucu Uygulama Notları'nda tek satır.

## Checklist

- [ ] `wgpu` workspace + `bt-gpu` dev-dependency, özellik seti ve yorum
- [ ] `cfg(test)` wgpu iskeleti: device, `block_on`, offscreen hedef, geri okuma
- [ ] `cell_bg.wgsl`: `cell_bg` + `caret_fragment`; immediate/uniform seçimi yapı başına
- [ ] Bekçi ikizleri (`MIDTONE`, dejenere caret bit bit, köşe/kenar/hale)
- [ ] Kâhin sahne listesi ve pipeline kurulum sınaması
- [ ] Ölçüm kancası (`#[ignore]`) + `olcum.md` tür satırı
- [ ] `proje.md` üç WGSL satırı + `make shader`'ın `.wgsl` kolu
- [ ] `/measure`: offscreen karşılaştırma + Metal pencere tabanı → `docs/OLCUMLER.md`; durak kuralı uygulandı
- [ ] Doğrulama geçti (`make hepsi` + `make shader`)
- [ ] Riskli phase: `/code-review` koştu, bulgular giderildi
