# Phase 2 — bt-gpu: cell_bg pipeline ve Frame

## Özet

Hücre arka planlarını ve imleci instanced quad'larla çizen `cell_bg`
pipeline'ı; `Frame` sink'ten dolar. 001'in tam ekran quad'ı **silindi** (arka
planı render pass'in `Clear` yükü boyuyor, görüntü birebir aynı) ve `make
duman` sözleşmesi olduğu gibi kaldı. Pencere yok, display link yok.

_Requirements: R2_

---

## 1. Shader

`crates/bt-gpu/shaders/cell_bg.metal`

```metal
#include <metal_stdlib>
using namespace metal;

// Rust: bt_gpu::frame::Instance, #[repr(C)] { pos: [f32;2], size: [f32;2], rgba: [f32;4] }
// float2+float2+float4 = 32 bayt; float4 16 hizalı → struct hizası 16, sırası aynı.
struct Instance { float2 pos; float2 size; float4 rgba; };
struct Viewport { float2 px; };                       // drawable boyutu (piksel)

struct Out { float4 position [[position]]; float4 rgba; };

vertex Out cell_bg_vertex(uint vid [[vertex_id]], uint iid [[instance_id]],
                          constant Instance* inst [[buffer(0)]],
                          constant Viewport& vp [[buffer(1)]]) {
    float2 corner = float2(vid & 1, (vid >> 1) & 1);   // 0..3 → dört köşe, triangle strip
    float2 p = inst[iid].pos + corner * inst[iid].size;
    float2 ndc = p / vp.px * 2.0 - 1.0;
    Out o; o.position = float4(ndc.x, -ndc.y, 0.0, 1.0); o.rgba = inst[iid].rgba; return o;
}
fragment float4 cell_bg_fragment(Out in [[stage_in]]) { return in.rgba; }
```

`build.rs` dizini izlediği için yeni dosya kendiliğinden derlenir.

## 2. Frame

`crates/bt-gpu/src/frame.rs`

```rust
#[repr(C)] #[derive(Clone, Copy)]
pub struct Instance { pub pos: [f32; 2], pub size: [f32; 2], pub rgba: [f32; 4] }
const _: () = assert!(size_of::<Instance>() == 32);

/// Bir karenin çizim listesi; hasar varsa `Session::frame`'in sink'iyle dolar.
pub struct Frame { pub instances: Vec<Instance>, pub cell_px: (f32, f32), pub bg_count: usize }
impl Frame {
    pub fn clear(&mut self) { self.instances.clear(); self.bg_count = 0; }
    pub fn push_bg(&mut self, col: u16, row: u16, rgba: [f32; 4]) { …; self.bg_count += 1; }
    pub fn push_cursor(&mut self, col: u16, row: u16, rgba: [f32; 4]) { … }   // sayılmaz
}
```

`bg_count` duman'ın `hucre=K`'sıdır (imleç hariç) — phase-3 kullanır.

## 3. Renderer

`renderer.rs`: ikinci pipeline `cell_bg` (`quad` kalır), `Viewport` uniform.
`draw(&self, drawable, clear: [f32;4], frame: &Frame)`:

1. Pass: `loadAction = Clear` (arka plan `clear`); quad pipeline **kalkar**
   bu yoldan — tam ekran quad'ın işini `Clear` yapar. 001'in `draw` imzası
   (`colour`) → `Frame` boşken aynı görüntü.
2. `newBufferWithBytes_length_options(instances, StorageModeShared)` — kare
   başına; komut tamponu buffer'ı tutar. Boş `Frame`'de buffer kurulmaz.
3. `drawPrimitives_vertexStart_vertexCount_instanceCount(TriangleStrip, 0, 4, n)`.
4. Bu phase'de **senkron kalır** (`waitUntilCompleted` + `status`); asenkron
   phase-3'te.

`make duman` (001): `draw_surface(surface, ARKA_PLAN, &Frame::empty())` →
`kare=1 pipeline=ok` aynen.

## 4. Sınamalar

- `Instance` 32 bayt assert (derleme zamanı).
- `cell_bg_pipeline_kurulur`: device üstünde ikinci pipeline.
- `frame_bg_count_imleci_saymaz`.

---

## Uygulama Notları

**`quad` pipeline'ı silindi — plan "kalır" diyordu.** Arka planı `Clear` yükü
boyayınca `quad_vertex`/`quad_fragment`, `quad.metal` ve `Uniforms`'un tek
kullanıcısı kalmıyor. Kurulan ama hiç çizmeyen bir pipeline ölü koddur:
metallib'i büyütür, açılışta derlenir ve okuyana "birileri bunu kullanıyor"
der. Silindi; `build.rs`'in `assert!(!airs.is_empty())` kapısı `cell_bg.metal`
ile sağlanıyor, `make shader` kanaryası çalışmaya devam ediyor.

**`make duman`'ın kapsamı bu phase'de daraldı ve bu yazıya döküldü.** 001'de
tam ekran quad her karede bir çizim çağrısı yapıyordu, yani `pipeline=ok`
gerçekten "pipeline'dan bir çizim geçti" demekti. Artık boş `Frame`'de hiç
çizim çağrısı yok: kalan kapsam "shader derlendi, pipeline device üstünde
kuruldu, komut tamponu hatasız tamamlandı, drawable sunuldu". Jeton
korunuyor (makine sözleşmesi, silinemez) ama `proje.md`'nin `duman` satırı
kapsamı **tek tek sayacak** şekilde yeniden yazıldı ve boşluğun phase-3'ün
`hucre=K`'sıyla kapandığı yazıldı. Sessizce zayıflayan kapı, zayıflamayan
kapıdan daha tehlikelidir.

**Sapmalar:**

- `Instance` `pub` değil `pub(crate)`: plan taslağı `pub struct Instance`
  yazıyordu. GPU düzeni dışarıya sızmamalı — `bt-core`'un `CellBg`'si anlam
  taşır, `Instance` bayt düzeni taşır; ikisini aynı tip yapmak hücre modelini
  shader'a çivilemek olurdu. `Frame` `pub`, alanları değil.
- `Frame` alanları `pub` değil, erişimciler var (`bg_count()`, `is_empty()`,
  `instances()` crate-içi). Plan taslağı üç alanı da `pub` yazıyordu; `Vec`'i
  dışarıya açmanın bir tüketicisi yok.
- **`offset_of` assert'leri eklendi** (plan yalnız `size_of == 32` istiyordu).
  Boyutun 32 olması alan ofseslerinin 0/8/16 olduğunu garanti etmez; MSL'de
  `float4` 16 hizalı, Rust'ta 4 — çakışma tesadüf değil ama **denetlenmemiş**
  bir tesadüftü. Üç ofset de derleme zamanında bağlandı; `/audit` mercek 9'un
  aradığı tam bu.
- `Viewport` `renderer.rs`'te ve `struct` özel: tek alanlık uniform'un
  `frame.rs`'te işi yok, encode sınırında yaşıyor. `size_of == 8` assert'i var.
- Yeni hata varyantı `GpuError::NoBuffer`: `newBufferWithBytes` `None`
  dönebiliyor ve sessiz `None` bu depoda yasak.
- `Frame::empty()` eklendi (plan `&Frame::empty()` yazıyordu ama kurucuyu
  saymamıştı): hücresi olmayan kare, `cell_px` anlamsız olduğu için sıfır.
  `bt-shell` phase-3'e kadar bunu veriyor.

**`/simplify` bulguları (dört mercek, paralel).** Uygulananlar:

- **`proje.md`'ye yazdığım cümle yanlıştı.** "Boşluğu phase-3'ün `hucre=K`'sı
  kapatır" demişim; `hucre=K` `Frame::push_bg` içinde artan bir **CPU**
  sayacıdır ve `draw`'dan önce okunur — sink'in hücre ürettiğini kanıtlar,
  GPU'nun piksel boyadığını kanıtlamaz. Yani phase-2'nin açtığı boşluk
  phase-3'ten sonra da açık kalırdı ve belge kapandığını söylüyordu.
  **Boşluk bu phase'de gerçekten kapatıldı:** `cell_bg_pikseli_gpu_tarafinda_boyar`
  offscreen bir `MTLTexture`'a tek hücre çizip pikseli `getBytes` ile geri
  okuyor. `offset_of` assert'leri düzeni derleme zamanında bağlıyordu ama
  hiç **çalıştırmıyordu**; bu sınama buffer indekslerini, NDC dönüşümünü ve y
  ters çevirmeyi çalıştırıyor. Pencere gerekmediği için `make duman` gibi
  başsız ortamda atlanmıyor, koşulsuz `make hepsi` altında koşuyor.
  Doğrulandı: shader'da `-ndc.y` → `ndc.y` yapınca **düşüyor**,
  `[[buffer(1)]]` → `[[buffer(2)]]` yapınca **düşüyor**.
- **`cell_px` alan olmaktan çıktı, `clear`'ın parametresi oldu.** Phase-3
  `Frame`'i ivar'da tutuyor; kurucuda dondurulmuş bir hücre boyutu ekran
  ölçeği değişince (`windowDidChangeBackingProperties:`) sessizce bayatlardı.
  Yan kazanç: `new` + `empty` ikilisi tek `Default`'a indi ve `Frame::empty()`
  dejenere nesnesi (sıfır boyutlu hücreler) ortadan kalktı. `hucre()`'ye
  `debug_assert!(w > 0 && h > 0)` konuldu: `clear`sız push sessiz kalmasın.
- **`cell_px` tipi `bt-core` ile hizalandı** (`(f32, f32)` → `(u16, u16)`).
  Aynı büyüklüğün sahibi `SessionOptions::cell_px` ve `Session::resize`, ikisi
  de `(u16, u16)`; phase-3 tek tuple'ı ikisine verecek. Ayrışsalar PTY'nin
  `TIOCSWINSZ`'i ile ekranda çizilen hücre boyutu sessizce farklı olurdu.
- **`push_bg(col, row, rgba)` → `push_bg(CellBg)`, `push_cursor` → `Cursor` +
  renk.** İki komşu `u16`'yı konumsal geçirmek takas edilse temiz derlerdi ve
  grid devrik çizilirdi. `bt-gpu`'nun `bt-core` bağımlılığı zaten bildirilmiş
  ama hiç kullanılmıyordu. `push_cursor` görünmez imleci çizmiyor: "görünmeyen
  şey çizilmez" çizim kararıdır, phase-3'ün callback'inden bir dal siliyor.
- **`Viewport` struct'ı düştü**, yerine `[f32; 2]` ve MSL'de `constant float2&`:
  tek alanlık bir sarmalayıcı ikinci bir Rust ↔ MSL düzen sözleşmesi (ve onu
  bekleyen bir assert) açıyordu.
- **`[[flat]]`** `Out.rgba`'ya: renk instance boyunca sabit, rasterizer'ın
  fragment başına interpolasyonu boşa işti.
- **`device` alanı `Renderer`'a**: `queue.device()` kare başına bir ObjC
  mesajı + retain/release'ti, oysa device kurucuda zaten elde.
- `is_empty()` dışa açık bir metot olmaktan çıktı; boş kare kontrolü
  `encode_cells`'in başında erken dönüş oldu — Metal'in "sıfır uzunluklu
  tampon geçersiz" kuralı gerekçesinin yanında duruyor.
- Shader'da `(vid >> 1) & 1` maskesi gereksizdi (`vertexCount` 4),
  `inst[iid]` üç kez okunuyordu.
- `offset_of!(Instance, pos) == 0` ve `size_of::<Viewport>() == 8` assert'leri
  tautolojiydi; `repr(C)`'nin ilk alanı zaten 0'da.
- Derleyicinin garantisini tekrar eden iki sınama silindi
  (`instance_dilimi_bayt_olarak_ardisiktir`, `bos_kare_instance_tasimaz`);
  yerine yukarıdaki offscreen okuma geldi.
- `phase-2.md` Özet'i, `plan.md` Yaklaşım-2 maddesi ve checklist metni "iki
  pipeline" dünyasını anlatmaya devam ediyordu; üçü de tek pipeline gerçeğine
  çekildi.

Uygulanmayanlar ve nedenleri:

- **`setVertexBytes` ile ≤4 KiB'lik kareyi tampon ayırmadan geçirmek**
  (eşik 128 instance; tipik kare 9 instance = 288 bayt). Muhakeme tampon
  stratejisini açıkça `/measure`'a bağladı ("kare başına `newBufferWithBytes`;
  üçlü tamponlama yok, `/measure` sonrası karar"). İkinci bir bağlama yolunu
  şimdi açmak o kararı ölçmeden ezmek olurdu — **phase-3'ün ölçüm maddesine**
  eklendi.
- **`size` alanını instance'tan çıkarmak** (kare başına 12 000 × 8 = 93,75 KiB
  tekrar eden sabit). Tek başına kazanç vermiyor: `float4` MSL'de stride'ı 32'de
  tutuyor, kazanç `rgba`'nın da paketlenmesiyle (`uchar4` → 12 bayt stride)
  geliyor. Düzen değişikliği, `/measure` adayı olarak not.
- `Frame::with_capacity`: kapasite rampası oturum başına bir kez ödeniyor ve
  `clear()` kapasiteyi koruyor; ipucu için grid boyutu gerekiyor, o da phase-3'te.
- `texture.width()/height()` önbelleğe alınması: kare başına iki ObjC mesajı,
  ve doku boyutun tek yetkili kaynağı — `Surface`'ın bildiğine güvenmek `draw`'u
  drawable'dan bağımsız tutma sözleşmesini bozardı.
- `Frame` kurucularının `pub(crate)` olması: bugün onları dolduracak yer crate
  **dışı** (`bt-shell`), çünkü display link phase-3'te doğuyor. Daraltma
  phase-3 checklist'ine yazıldı.

**`/code-review` bulguları.** Süreç düşüren bir hata ve veri kaybettiren bir
hata dahil, uygulananlar:

- **`encode_pass` hata yolunda süreci öldürüyordu.** `encode_cells` `Err`
  dönünce `?` erken çıkıyor ve render encoder `endEncoding()` görmeden
  düşüyordu; Metal bunu istisnayla karşılar ve süreci sonlandırır — yani
  `GpuError::NoBuffer`'ı zarifçe döndürme amacının tam tersi olurdu. Sonuç
  artık `endEncoding()` sonrasında döndürülüyor.
- **Dejenere `resize` kaydırma geçmişini yok ediyordu.** Phase-1'de sıfır
  boyutu 1×1'e kırpmıştım; panik gitmişti ama daha sessiz bir kayıp gelmişti:
  var olan bir grid'i 1 sütuna çekmek alacritty'de her sarmalı satırı açar ve
  `reversed.truncate(max_scroll_limit + lines)` geçmişin neredeyse tamamını
  atar — 80 sütuna dönmek geri getirmez. Ayrıca PTY'ye 1×1 winsize gitmesi
  tam ekran uygulamaları bozardı. `resize` artık kırpmıyor, **yoksayıyor**;
  kırpma yalnız `spawn`'da (henüz kaybedilecek geçmiş yokken) kaldı.
  Sınama `sifir_boyut_panik_etmez` yalnız panik yokluğuna bakıyordu ve
  kırpmayı yeşil geçiyordu; `sifir_boyut_yoksayilir` oldu ve hasar
  işaretlenmemesini iddia ediyor (kırpma geri konunca **düşüyor**).
- **`constant Instance*` → `device const Instance*`.** Dizi `instance_id` ile
  ıraksak indeksleniyor ve grid'le büyüyor (200×60'ta 384 KB); `constant`
  küçük ve tekdüze okunan veri içindir ve boyut sınırı vardır. Sınır aşımı
  pipeline kurulumunda değil çizimde ortaya çıkardı, yani tek instance'lık
  offscreen sınamamız da yeşil kalırdı. `viewport_px` gerçekten tekdüze,
  `constant` kaldı.
- **`Session::mark_dirty()` eklendi.** `frame()` bayrağı çizimden **önce**
  tüketiyor; çizim sonradan başarısız olursa (drawable alınamadı) o içerik
  bir daha istenmiyordu ve pencere PTY'den yeni bayt gelene kadar bayat
  kalıyordu — "boşta sıfır kare" sessizce "boşta hiç kare" olurdu. Çizemeyen
  tarafın çağıracağı yol açıldı; phase-3 checklist'ine bağlandı.
- **`frame()`'in sink'i jenerik oldu** (`&mut dyn FnMut` → `impl FnMut`).
  Üç bağımsız mercek aynı şeyi söyledi: dolu ekranda hücre başına dinamik
  çağrı, üstelik `Term` kilidi altında. `Session` hiçbir yerde trait nesnesi
  olarak kullanılmıyor. `plan.md`'nin R1.2 metni aynı commit'te düzeltildi.
- **İmleç sırası artık `debug_assert` ile korunuyor.** "İmleç en sonda"
  yalnız bir doküman sözüydü; `push_bg` sonradan çağrılırsa imleci gömüyordu
  ve belirti sessizdi. `instances.len() == bg_count` tam olarak "imleç henüz
  eklenmedi" demek.
- **`cell_px` değişimini sınayan sınama eklendi.** Alan → `clear` parametresi
  değişikliğinin **tek** sebebi buydu ama üç sınama da `clear`'ı bir kez ve
  aynı değerle çağırıyordu; değişiklik geri alınsa hepsi yeşil kalırdı.
- `ColorRequest`'in gerekçesi yanlıştı ("kimse sormadan değiştirmiyor");
  "önce ata sonra sor" yaygın bir örüntü. Yanıt hâlâ varsayılan paletten
  geliyor (kilit yeniden girilebilir değil, gerçek çözüm palet sahipliğinin
  alacritty'den bize geçmesi = 00X tema seti) ama artık **bilinen sınır**
  olarak yazılı.
- `CLAUDE.md`'nin "yoksayılır, **loglanır**" kuralı kodla çelişiyordu; `tracing`
  taban listesinde ama hiçbir crate'e bağlı değil. Borç kuralın yanına yazıldı.
- `phase-2.md`'nin Yayın Etkisi bloğu, aynı diff'in sildiği `Viewport`'u
  canlı bir sözleşme gibi sayıyordu.

Uygulanmayanlar:

- `color::rgba`'nın hücre başına dört bölme yapması: paletin 269 girdisi
  önceden f32'ye çevrilebilir ama `Color::Spec` 24-bit doğrudan renk de
  geliyor, yani tablo her durumu kapsamıyor. Ölçülmedi — `/measure` adayı.
- `setVertexBytes` eşiği: `/simplify`'da da geçti, aynı gerekçeyle phase-3'ün
  ölçüm maddesinde.
- `Frame`'in doldurma API'sinin tüketicisinden bir phase önce gelmesi: bölme
  bilinçli (phase-3 display link'i pencereyle aynı commit'te istiyor).
  `bg_count`'un doküman cümlesi jetonun **phase-3'te doğacağını** söyleyecek
  şekilde düzeltildi.

**`/audit` sonucu.** Koşan mercekler: 1 katman/platformsuzluk, 3 panik yolu,
6 ölçüm sahipliği (mekanik, inline); 7 thread ve blokaj, 8 boşta sıfır kare,
9 hücre/shader düzeni, 10 belge ve üslup (yargı, paralel ajan). **İlgisiz:**
2 (manifest hiç değişmedi), 4 (ayar/tema şeması yok), 5 (`assets/shell/` el
değmedi).

Temiz çıkanlar: katman yönü (`bt-gpu`'nun `bt_core::{CellBg, Cursor}` alması
aşağı yönlü ve `CellBg`'nin `repr(C)`'si yok — hücre modeli shader düzenine
çivilenmemiş); `bt-core` platformsuz; üretim yolunda panik kaynağı yok
(beş `.expect` `#[cfg(test)]` içinde); belgelere ölçüm sayısı ya da
ölçülmemiş iddia girmedi; **kilit sırası bozulmadı** (`term` → `size`, ters
yön yok); `Renderer` hâlâ `Send + Sync` (`device` alanı bozmadı, derleyiciyle
doğrulandı); `endEncoding()` her yoldan çağrılıyor; buffer indeksleri
shader bildirimleriyle eşleşiyor; dörtlü köşe birim kareyi kapatıyor
(`MTLCullModeNone` varsayılan); bu diff animasyon/zamanlayıcı eklemiyor;
dil kuralı ihlali yok, `pub` adların tamamı İngilizce.

**En değerli bulgu — düzen sözleşmesi tek taraflıydı.** Yorum "buraya eklenen
bir alan Rust'ta derlemeyi kırar" diyordu; **kırmıyordu.** Mercek deneyle
gösterdi: MSL `Instance`'ının sonuna bir `float4` eklenince stride 32→48
oluyor, Rust'ın üç assert'i de tek instance'lık offscreen sınama da **yeşil
kalıyor**. İki düzeltme birden:

- `.metal`'e kendi `static_assert`'leri kondu (`sizeof` + iki
  `__builtin_offsetof`; `offsetof` MSL'de yok). Doğrulandı: alan eklenince
  **shader derlemesi** kırılıyor.
- Sınamaya **ikinci instance** eklendi. `inst[0]` stride'dan bağımsız
  okunur, yani stride hatası tek instance'la görünmez; ikinci hücre ancak
  doğru stride ile bulunur. Üç çeyrek sınanıyor: sol üst kırmızı, sağ alt
  yeşil, sağ üst clear.

Diğer giderilenler:

- **`resize`'ın ucuz kapısı pahalı kilidin arkasındaydı.** Aynı-boyut
  kontrolü `Term` kilidi alındıktan sonra yapılıyordu; canlı boyutlandırmada
  `windowDidResize:` çağrılarının çoğu hücre sınırını geçmez ve hiçbir şey
  yapmaz, ama her biri ana thread'i okuyucunun ayrıştırma lease'inin arkasına
  kuyruğa sokuyordu. Ön kapı küçük `size` kilidiyle, guard `term`'den önce
  düşüyor; iki eşzamanlı resize'ın ikincisi içerideki kontrolde yakalanıyor.
- **`mark_dirty()`'nin durma koşulu yoktu.** Kalıcı bir çizim hatası
  "başarısız → bayrağı dik → yeniden dene" döngüsünü ekran tazeleme hızında
  sonsuza çevirirdi — mercek 8'in "her animasyon bir durma koşulu taşır"
  kuralının aynısı. Doküman artık hata başına tek yeniden denemeyi ve
  "bayrak kimseyi uyandırmaz" cümlesini bağlıyor; phase-3'ün checklist
  maddesi üç çağrı yerini (senkron hata, asenkron tamamlanma handler'ı,
  piksel boyutu değişimi) ve durma koşulunu ayrı ayrı sayıyor.
- **Piksel boyutu grid boyutu değişmeden değişebiliyor.** `sync_size`
  `drawableSize`/`contentsScale`'i koşulsuz yazıyor; pencereyi birkaç piksel
  sürüklemek `floor(w/cw)`'yi değiştirmez → aynı boyut → erken dönüş → hasar
  yok. Bugün görünmüyor çünkü `app.rs` koşulsuz `draw()` çağırıyor, ama
  phase-3 o çizim çağrısını kaldırıyor: kare hiç gelmez ve layer eski
  drawable'ı gerdirir. Phase-3 checklist'ine (c) maddesi olarak düştü.
- `GridSize::yeni` → `tam` + doküman (adı ayırt edici özelliğini, "kırpmaz",
  söylemiyordu); `GpuError::NoBuffer` → `NoInstanceBuffer` (aile nesnesini
  adında taşıyor: `NoCommandQueue`, `NoRenderEncoder`); `draw_surface`
  dokümanındaki "002'de" → "phase-3'te" (set numarasıyla phase numarası aynı
  yerde çelişiyordu); `app.rs`'in "002'de burası yalnız kirli işaretler"
  yorumu aynı şekilde.
- Türkçe: "ofsetler **çakışıyor**" → "örtüşüyor" (teknik Türkçede çakışma
  çatışmadır, kastedilen denk düşmek); "sink'i buraya doldurur" → "sink
  burayı doldurur".
- İmleç sırası gerekçesi 50 satırda üç kez tekrarlanıyordu; `proje.md`'nin
  `duman` satırı sınamanın kendi yorumunu neredeyse birebir tekrarlıyordu
  (aynı açıklama üç yerde ayrı ayrı bayatlar); `CLAUDE.md`'nin log borcu
  paragrafı aynı şeyi iki kez söylüyordu. Üçü de teke indi.

Devredilen: `Surface` ne `Send` ne `Sync` (`Retained<CAMetalLayer>`) — phase-3
`draw_surface`'i silip drawable'ı callback'ten aldığı için şekil tutuyor,
kayda geçti. Görünürlük bildirimleri (`windowDidDeminiaturize:`,
`windowDidChangeOcclusionState:`) dinlenmiyor; kod okumakla kanıtlanamaz,
phase-3'ün göz kontrolü maddesine eklendi.

**Ölçülmedi, iddia edilmedi:** kare başına `newBufferWithBytes`'ın maliyeti.
Üçlü tamponlama Muhakeme'de reddedildi ve gerekçesi "`/measure` sonrası
karar"dı; bu phase o ölçümü yapmıyor.


## Yayın Etkisi

- **shader:** `cell_bg.metal` yeni, `quad.metal` **silindi**; `Instance`'ın
  Rust ↔ MSL eşlemesi `size_of` + iki `offset_of` ile bağlı, üstelik
  `cell_bg_pikseli_gpu_tarafinda_boyar` ile çalıştırılıyor. Viewport artık
  sarmalayıcı bir tip değil, düz `[f32; 2]` ↔ `constant float2&`.
  Instance dizisi `device const` adres uzayında. `make shader` koştu.
- **belge:** `proje.md`'nin `duman` satırı — jeton aynı, kapsamı yeniden
  yazıldı (bkz. Uygulama Notları). `CLAUDE.md`'ye dokunulmadı: oradaki
  `make duman` satırı phase-3'ün işi ve yeni sözleşmeyle birlikte değişecek.
- Yeni bağımlılık: yok.
- Ölçüm bekliyor: kare başına instance tamponu ayırmanın maliyeti (üçlü
  tamponlama kararı `/measure`'a bağlı).

---

## Checklist

- [x] `cell_bg.metal`, `Frame`, tek pipeline `cell_bg` (`quad` silindi), `draw(drawable, clear, &Frame)`
- [x] Test: `Instance` 32 bayt + `offset_of` üçlüsü; `cell_bg_pipeline_kurulur`; `frame_bg_count_imleci_saymaz`; `cell_bg_pikseli_gpu_tarafinda_boyar` (offscreen okuma)
- [x] Test: `make duman` hâlâ `kare=1 pipeline=ok`
- [x] Doğrulama geçti (`make hepsi`; koşullu: `make shader`, `make duman`)
- [x] `/simplify` çalıştırıldı, bulgular uygulandı
- [x] `/code-review` çalıştırıldı, bulgular giderildi
- [x] `/audit` çalıştırıldı, bulgular giderildi (mercek 9: `Instance` ↔ MSL, `[[buffer(0/1)]]` indeksleri)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [x] Commit: 3191677
