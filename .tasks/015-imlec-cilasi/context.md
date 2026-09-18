# İmleç cilası — Bağlam

## Mevcut Durum

014 imlecin **ne olduğunu** çözdü: şekli (DECSCUSR'ın üç biçimi), sönmesi ve
kendi tema rolü. Kalan üç şey imlecin **nasıl göründüğü ve ne zaman
kıpırdadığı**.

- **Caret düz bir dörtgen.** `cell_bg` pipeline'ında bir `Instance`
  (`{ pos, size, rgba }`, stride 32, iki taraflı `static_assert`); fragment
  düz renk basıyor, köşe yarıçapı ya da kenar yumuşaması yok
  (`shaders/cell_bg.metal`).
- **Ama zaten kendi çizimi.** `Frame::grid_caret`/`dock_caret`
  `Option<Instance>` veriyor, `encode_pass` onu **ayrı bir draw call** ve
  **ayrı bir `MTLBuffer`** ile çiziyor (`renderer.rs:641`, `714-716`). Yani
  caret'i başka bir pipeline'a taşımak yeniden yapılandırma istemiyor.
- **Geometrinin tek kaynağı var** (014 phase-1): `frame::caret_rect` hem
  boyanan dörtlüyü hem ters çevirme dikdörtgenini üretiyor.
- **Ters çevirme ayrı bir yol:** `cell` pipeline'ının `CursorBlock` uniform'u,
  fragment'te **eksen hizalı sert bir test** (`cell.metal:113-124`).
- **Odak sınırdan geçmiyor.** `NSWindowDidBecomeKey`/`ResignKey` dinleyen
  hiçbir yer yok; `caret_shape_of`'un `HollowBlock` kolu (`session.rs`) fiilen
  **ölü**, çünkü alacritty o şekli kendiliğinden üretmiyor.
- **Devir `Running` safhasına bağlı:** caret komut koşarken ızgaranın, öncesi
  ve sonrası dock'un (`shell::caret_home`). Hareket yay fiziğiyle süzülüyor
  (`bt-gpu::motion`, bir hücre ~230 ms).

## Motivasyon

Üçü de kullanıcıdan geldi (2026-09-18/19), biri 008'den beri kayıtlı:

1. **"box shadow gibi temiz bir hafif tasarım dokunuşu"** — caret'in çevresinde
   yumuşak bir ışıma.
2. **"block ise belki biraz radius verilebilir"** — köşe yarıçapı.
3. **"enter yapınca cursor bir süreliğine yukarı çıkıp sonra aşağıya
   inebiliyor… garip bi animasyon"** — hızlı komutta yarıda dönen hareket.
4. **İçi boş imleç** (odak kaybında) — 008'de adıyla ertelendi
   (`.tasks/008-hareket-ve-imlec/plan.md` → Kapsam Dışı), 014'te tekrar dışarıda
   kaldı çünkü odak sınırdan geçmiyor.

1, 2 ve 4 **tek yeteneğin** sonucu: caret'in kendi çizim yüzeyi. Referans da
öyle yapmış — metallib'inde ayrı bir `shape` pipeline'ı var ve instance'ı
`fill`, `stroke`, **`strokeW`**, **`radius`**, `squareCorners` taşıyor
(`docs/ARASTIRMA.md` → İmleç). `strokeW`'nin orada olma sebebi içi boş imleç.

3 ayrı bir konu (boyama değil **zamanlama**) ama aynı organın kusuru ve
kullanıcı onu bu setle birlikte istedi.

## Kanıt

**Sıçrama ölçüldü.** *Yöntem:* gerçek `/bin/zsh -i`, bir PTY çiftinin altında,
`ZDOTDIR` bizim sarmalayıcımıza kurulu ve kullanıcının kendi `$HOME`'u miras
alınmış; prompt oturduktan sonra `ls\r` yazılıp gelen baytlarda OSC 133
işaretleri `time.monotonic` damgasıyla kaydedildi. Terminal penceresi yok, yani
ölçülen şey kabuğun safha zinciri — çizim değil. Tek koşu, 2026-09-19, bu
makine. Kanca değil atılan bir enstrümantasyon; `docs/OLCUMLER.md`'nin konusu
değil (o dosya ölçülmüş **ürün** sayılarının sahibi).

```
   9.2 ms   133;C   CommandStart  → safha Running
  53.2 ms   133;D   CommandEnd    → safha Finished
  >>> Running safhası: 43.9 ms
```

İmlecin yay animasyonu bir hücre için ~230 ms'de yerleşiyor; dock ile ızgara
arası daha uzun. Yani caret yola çıkıyor, **yolun beşte birini almadan** safha
dönüyor ve geri sarıyor. Tamamlanmayan, ortasından dönen bir hareket.

**Sebep tek ve devir `CommandStart`'ta başlamıyor** — ikisi de panelde
düzeldi (`discussion.md` → Muhakeme M1, M2). Gerçek zincir:

```
Enter → line-finish (ZLE) → 8133;e → ayna Idle
                                   → caret_home(Input, Idle) = GRID  ← devir BURADA
     → preexec  → 133;C → safha Running   (hâlâ Grid)
     → precmd   → 133;D → safha Finished  → DOCK
```

Yani caret ızgaraya **Enter'da** geçiyor, komut başlarken değil; `running_since`
(013'ün saati) o anda hâlâ `None`.

İçeriğin kayması **ikinci bir sebep değil**: `CLAUDE.md` imlecin hedefinin
**ekran satırı** olduğunu (`row + origin`) yazıyor, yani öteleme caret'i tanım
gereği kıpırdatmıyor; üstelik kaymanın inişi zaten snap (tek yönlü kayma
kuralı). Görülen animasyonlu gidiş-dönüş caret'in **kendi yayı**.

Kaymanın kendisi de aynı yüklemden besleniyor ve bu bir kazanç: `caret_in_dock`
üç tüketiciyi birden sürüyor — imlecin görünürlüğü, **doluluk sayısı** ve
dock'un caret'i. Devir ile kayma iki şüpheli değil, **tek yüklemin iki yüzü**.

**Pipeline eklemenin bedeli ölçüldü ve küçük.** Bugün **iki** pipeline var
(`cell_bg`, `cell`), ortak kurucusu `Renderer::pipeline`. Üçüncüsü: bir alan,
bir çağrı, bir `encode_*`. **`build.rs` sıfır değişiklik** istiyor — `shaders/`
dizinini glob'luyor. `MTLVertexDescriptor` hiç yok (dörtlünün köşesi
`vertex_id`'den). Blend sabit ve ön çarpımsız alfa, yani gölgenin çıktısına
uygun.

**`Instance`'ı büyütme tartışması konusuz kaldı.** 014'ün Kapsam Dışı'sında
"tampon ızgara boyunda" yazmıştım; **yanlış**: `bg` listesi **seyrek**, yalnız
zemini varsayılandan farklı hücreler giriyor (`Frame::push`). Panel bunun
üstüne şunu ekledi: caret kare başına **tek quad** olduğu için yeni bir instance
tipine hiç gerek yok — şekil parametreleri uniform'dan geçebilir. Yani ne
`Instance` büyüyor ne yeni bir `#[repr(C)]` çifti doğuyor
(`discussion.md` → Karar 1).

**Hizalama tuzağı, shader'ın kendi yorumunda yazılı** (`cell.metal:8-10`):
Rust'ta `#[repr(C)]` `[f32; 4]` hizası **4**, MSL'de `float4` hizası **16**.
Var olan üç çiftin (`Instance`, `GlyphInstance`, `CursorBlock`) tutmasının
sebebi alanların doğal ofsetlere düşmesi ve toplamın tam 32 olması. Herhangi
birine çıplak bir `float radius` eklemek Rust'ta 36, MSL'de **48** eder ve
sessizce ayrışır. Kural: alanlar `float2`/`float4` boyunda, toplam 16'nın katı.

**Odak ucuz.** `AppDelegate` **zaten `NSWindowDelegate`**
(`bt-shell/src/app.rs`), yani `windowDidBecomeKey:` / `windowDidResignKey:` iki
yeni delegate metodu — `addObserver` gerekmiyor. Emsali
`windowDidChangeOcclusionState:` → `DisplayLink::set_visible`. Yön **shell →
gpu**, yani alternatif ekran habercisinin (gpu → shell) tersi.

**Bayat doc şüphesi.** `Frame::move_caret`'ın doc'u ve `Frame::push`'un
`debug_assert` mesajı hâlâ "caret `bg` listesinin sonunda" diye okunuyor; kod
012'den beri caret'i ayrı alanlarda tutuyor. Sette doğrulanıp düzeltilecek.

## Mevcut Mimari

```
encode_pass sırası (renderer.rs:617-652)
 ┌─────────────────────────────────────────────────────────────┐
 │ 1. şeritler        cell        (dejenere CursorBlock ile)    │
 │ 2. arka planlar    cell_bg     (seyrek Instance listesi)     │
 │ 3. IZGARA CARET'İ  cell_bg     ← tek quad, kendi tamponu     │
 │ 4. glyph + kural   cell        ← CursorBlock ters çevirmesi  │
 │ 5. dock: zemin → bg → DOCK CARET'İ → glyph + kural           │
 └─────────────────────────────────────────────────────────────┘

caret_rect (frame.rs)  ──┬──►  Instance   (3. ve 5. adım, boyanan dörtlü)
   tek kaynak, iki      └──►  CursorBlock (4. adım, ters çevirme)
   tüketici (014)
```

Gölge bu resmi iki yerden zorluyor: quad caret'ten **büyük** olmak zorunda
(pad + fragment'te SDF) ve yuvası sabit — glyph'lerden önce çizilirse komşu
harfler gölgenin üstüne basar, sonra taşınırsa komşu metni karartır.
