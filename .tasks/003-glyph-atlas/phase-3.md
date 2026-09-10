# Phase 3 — Metrik geçişi: `CELL_PX` ölür

## Özet

`bt-gpu` `bt-atlas`'ı görür ve metriği yeniden yayınlar; `bt-shell`'in
`CELL_PX` yer tutucusu silinir ve grid ölçüsü gerçek font metriğinden türer.
Glyph hâlâ yok — değişen yalnız hücrelerin boyu.

_Requirements: R5, R7 (kısmi: `CELL_PX` yorumu)_

---

## 1. `bt-gpu` metriği yeniden yayınlar

`crates/bt-gpu/src/renderer.rs`

Karar 2(a): `bt-shell` `bt-atlas`'ı **görmez**, metrik `bt-gpu` üzerinden
geçer ve `CLAUDE.md`'nin katman tablosu değişmez.

```rust
/// Hücre ölçüsü; `bt-shell` grid boyutunu ve `TIOCSWINSZ`'i bundan türetir.
///
/// `scale` parametre çünkü ekran ölçeği değişebilir
/// (`windowDidChangeBackingProperties:`) ve atlas önbelleği ölçeği anahtarının
/// parçası olarak taşır: aynı `Renderer` iki ölçekte iki farklı metrik verir.
pub fn cell_metrics(&self, scale: f64) -> CellMetrics
```

`Renderer` atlası ölçek başına önbellekler (`RefCell<HashMap<..>>` ya da son
ölçek için tek girdi — uygulamada karar, gerekçe `## Uygulama Notları`'na).
`CellMetrics` `bt-gpu`'nun `pub` tipidir ve `bt_atlas::Metrics`'i yeniden ihraç
etmez: `bt-shell`'in `bt-atlas` tipini görmesi katman tablosunu bulanıklaştırır.

---

## 2. `CELL_PX` silinir

`crates/bt-shell/src/app.rs`

```rust
// SİLİNİR:
// const CELL_PX: (f64, f64) = (9.0, 18.0);
```

`metrics()` bugün `CELL_PX.0 * scale` ile hesaplıyor; artık
`renderer.cell_metrics(scale)` çağırır. `scale` zaten elde
(`window.backingScaleFactor()`), yani çarpma **atlas tarafına** taşınır —
metriğin ölçekle ilişkisi tek yerde kalır.

Hücreler boy değiştirir; `hucre=8` durur (900×600 penceresine sekiz hücre her
makul metrikte sığar) ama `make duman` yine de koşturulur.

---

## Phase-2'den devir

Phase-2'nin kalite kapısı (reuse merceği) bu phase'e ait üç bulgu üretti;
uygulanmadılar çünkü hepsi `CELL_PX`'in ölmesiyle aynı anda çözülüyor.

**1. İki yuvarlama kuralı yan yana duruyor ve biri silinmezse kalıcı olur.**
`bt-atlas` hücre ölçüsünü `ceil()` ile yukarı yuvarlıyor (`font::yukari`),
`app.rs:38` ise `CELL_PX.0 * scale` sonucunu `.round()` ile yuvarlıyor.
`cell_metrics(scale)` bağlandığında `app.rs`'teki yuvarlama bloğu **silinmeli**
— kalırsa depoda iki kural olur, hangisinin kazandığı çağrı sırasına bağlanır
ve belirti bir piksellik hücre kayması, yani sessiz.

**2. İki `Metrics` tipi aynı dosyada buluşacak.** `app.rs`'in `Metrics`'i
aslında "grid ölçüsü + hücre" (`cols`, `rows`, `cell_px`); `bt_atlas::Metrics`
ise yalnız hücre. `cell_metrics` geldiğinde ikisi yan yana okunacak, biri
yeniden adlandırılmalı — `app.rs` tarafı için `GridMetrics`/`Geometry`.
`CellMetrics`'in `bt_atlas::Metrics`'i yeniden ihraç **etmemesi** kararı
(yukarıda, 1. bölüm) bu yüzden ayrıca değerli: ad çakışması katmanı da
bulanıklaştırırdı.

**3. `cell_px: (u16, u16)` adsız demeti dördüncü kez dolaşıyor**
(`bt_core::SessionOptions.cell_px`, `bt_gpu::DisplayLink::resize`,
`bt_gpu::Frame::clear`, `bt-shell::app::Metrics`). Katman sözleşmesi ortak bir
tipe izin vermiyor (`bt-atlas` `bt-core`'u göremez) ama `bt-gpu` ikisini birden
görüyor: `cell_metrics` bu demetin **tek geçiş noktası** olsun, beşincisi elle
kurulmasın.

**Ayrıca — yukarıdaki 1. bölümün "ölçek önbelleği" tasarımı değişti.**
Phase-2 `Atlas::yenile(punto, scale) -> bool` ile çıktı: anahtar değiştiyse
atlası yeniden kurar ve `true` döner, `true` aynı zamanda "dokuyu yeniden
ayır" demektir. Yani `RefCell<HashMap<ölçek, Atlas>>` **gerekmiyor**; tek bir
atlas + `yenile` çağrısı yeter ve ölçek başına ikinci bir atlas taşımaz.
Karar gerekçesiyle `## Uygulama Notları`'na yazılır.

---

## Uygulama Notları

**Ölçek önbelleği: `HashMap` değil, tek atlas + `ensure`.** Kılavuzun 1.
bölümü `RefCell<HashMap<ölçek, Atlas>>` ile "ya da son ölçek için tek girdi"
diyordu; phase-2 `Atlas::ensure(punto, scale) -> bool` ile çıkınca ikinci yol
kendiliğinden kazandı. `Renderer.atlas` tek bir `RefCell<Option<Atlas>>`;
`cell_metrics` önce yoksa kurar, sonra `ensure` çağırır. Harita taşımanın
bedeli ölçek başına ikinci bir CoreText zinciri ve ikinci bir doku olurdu ve
**hiçbir makinede iki ölçek aynı anda çizilmiyor** — pencere bir ekrandadır.

**Atlas tembel (`Option`), sabit `1.0` ile kurulmuyor.** Atlasın anahtarı
(punto + backing ölçeği) **pencereden** gelir, kurucu pencereyi görmez.
`Atlas::new(PUNTO, 1.0)` ile kurmak iki şeyi birden bozardı: retina makinede
font zinciri açılışta boşuna bir kez daha koşar (`ensure` hemen yeniden
kurar), ve metriği hiç sormadan atlası okuyan bir yol **sessizce @1x** çizerdi.
`None` o yolu sessiz olmaktan çıkarıyor: phase-4 glyph çizmeden önce metriği
sormak zorunda.

**`CellMetrics` alanı private, kurucusu `pub fn new(w, h) -> Option<Self>`.**
İlk hâlde alan `pub` idi ve `bt-shell` `CellMetrics { cell_px: (0, 0) }`
kurabiliyordu; `izgaraya_bol`'da `900.0 / 0.0 = inf`, `inf as u16 = 65535` ve
65535×65535'lik bir grid `TIOCSWINSZ`'e giderdi (`Session::resize` yalnız
sıfırı eliyor). Garanti "kimse kuramasın" ile değil **"kuran sıfırı
geçiremesin"** ile sağlandı: kurucu `pub` kaldığı için `bt-shell`'in grid
aritmetiği bir Metal device kurmadan sınanabilir kalıyor — alan gizlenip
kurucu da gizlenseydi `app.rs`'in saf testleri `Renderer` kurmak zorunda
kalırdı ve GPU'suz koşamazlardı.

**Devir maddesi 3 karşılandı: demet artık `cell_metrics`'te tek geçiş
noktası.** `CellMetrics` `izgaraya_bol` → `Grid` → `DisplayLink::new`/`resize`
→ `LinkIvars.cell` boyunca **tip olarak** taşınıyor; demet yalnız değerin
tipi bırakmak zorunda olduğu **üç** yerde iniyor: sayıya dönüp bölmeye
girerken (`izgaraya_bol`), `bt-core`'a geçerken (`SessionOptions.cell_px`,
`Session::resize` — `bt-core` `bt-gpu`'yu göremez, katman kuralının gerçek
bedeli bu) ve `#[repr(C)]` kare kurucusuna girerken (`Frame::clear`). Beşinci
bir elle kurulan demet yok, ki devir maddesinin istediği buydu.

**Devir maddesi 1 karşılandı:** `app.rs`'teki `.round()`/`.max(1.0)` bloğu
silindi, yerine yorumu kondu. Yuvarlama artık yalnız `bt-atlas`'ta ve
**fiziksel piksel uzayında**: `ceil(ascent)` ve `ceil(descent+leading)` ayrı
ayrı, `punto · ölçek` çarpımından **sonra**. Eski `(CELL_PX * scale).round()`
1x'te yuvarlayıp çarpıyordu; bu makinede ölçüm `@1x (8,17)`, `@2x (16,32)`,
`@3x (24,47)` veriyor — `(8,17) · 2 ≠ (16,32)`, tam olarak eski kuralın
yanlış olduğu yer.

**Devir maddesi 2 karşılandı ama adla:** `app.rs`'in `Metrics`'i `Geometry`
değil **`Grid`** oldu — `izgara` yerel değişkeninin birebir karşılığı ve
"metrik" sözcüğü artık yalnız hücre ölçüsü için kullanılıyor.

**`Arc<Renderer>` → `Rc<Renderer>` (planlanmamış, clippy zorladı).**
`RefCell<Atlas>` alanı `Renderer`'ı `!Sync` yapıyor (yapısal sebep:
`RefCell` koşulsuz `!Sync`; ayrıca `CFRetained<CTFont>` `!Send`).
`make hepsi` `arc_with_non_send_sync` ile kırmızı düştü — `Arc` artık
tutulamayan bir söz veriyordu. `Rc`'ye indirildi (`link.rs`, `app.rs`,
`lib.rs`); `retry` ve `session` `Arc` kalıyor, onlar gerçekten thread geçiyor.
Bu bir daralma değil, var olan gerçeğin tipe yazılması: `Renderer` zaten
yalnız ana thread'de kullanılıyordu.

**`Atlas::ensure` `#[must_use]` oldu — ama koruma sanıldığı yere düşmüyor.**
`cell_metrics` dönüşü bilerek yutuyor (`let _ =`) çünkü dokunun sahibi
phase-4. İlk yazımda "`#[must_use]` bu satırı 004'e gösterir" denmişti;
`/code-review` bunun **yanlış** olduğunu gösterdi: `let _ = expr;` rustc'nin
kabul ettiği susturma biçimidir ve `unused_must_use` yalnız çıplak deyimde
atar, yani tek çağrı yerinde öznitelik hiçbir şey üretmiyor. Öznitelik yine
de duruyor çünkü değeri gerçek: 004'ün **yeni** `ensure` çağrılarını
yakalar. İddia hem `renderer.rs`'te hem `phase-4.md`'de düzeltildi ve
phase-4'ün ilk devir maddesi artık "bu satırı elle ele al" diyor.

**`bt-gpu → bt-atlas` kenarı zaten vardı** (001 iskeleti, `3e006ba`);
checklist'in ilk maddesi için `Cargo.toml` **değişmedi**. `Cargo.lock` ve
hiçbir `Cargo.toml` bu phase'de el değmedi.

**Kalite kapısında yakalanan gerileme.** `/simplify`'ın üç merceği
`CellMetrics`'i "tek hop sarmalayıcısı, sil" derken dördüncüsü (altitude)
yukarıdaki sıfır-ölçü senaryosunu adlandırdı; ikisini birden karşılayan hâl
private alan + denetli kurucu oldu. Silme yolu seçilseydi sıfır ölçü
`bt-shell`'de kurulabilir kalırdı.

**Waive edilen `/simplify` bulguları (kod değişmedi, gerekçe burada):**

- *`Renderer::system_default(mtm: MainThreadMarker)` ile ana thread'e çivile.*
  Mevcut dört renderer sınaması `cargo test`'in kendi thread'lerinde koşuyor;
  `MainThreadMarker::new()` orada `None` döner ve öneri dördünü birden kırar.
  Ayrıca `PIXEL_FORMAT`'ın belgesi offscreen renderer'ı açıkça bir gelecek
  olarak anıyor.
- *`last_bg_count: AtomicUsize` → `Cell<usize>`.* Tip yanlış değil, fazla
  iddialı; ama `Renderer`'ın thread aidiyeti tam da açık kalan soru ve bu
  phase'in ona bir taraf seçmek için sebebi yok.
- *`PUNTO` sabiti `Cell<f64>` + `set_point_size` olsun.* Ayar modeli yokken
  ayar kancası kurmak; `SCROLLBACK` (`bt-shell`) aynı örüntüyü izliyor —
  ayar modeli gelince sabit ölü doğar.

**`/code-review` bulguları — uygulananlar (5).**

1. *`cell_metrics` denetli kurucuyu atlıyordu.* Tek üretim kurucusu yapı
   gövdesiydi (`CellMetrics { cell_px: ... }`), yani "≥ 1 garantisi tipin
   içinde" cümlesi kendi kodunda geçerli değildi: `bt_atlas::Metrics.cell_px`
   çıplak bir `pub` alan ve kırpma bir crate ötede. Artık
   `CellMetrics::new(w, h).expect("bt-atlas hücre ölçüsünü 1'e kırpar")` —
   `// audit:` gerekçesiyle; burası pencere geometrisi yolu, PTY/ayrıştırma
   yolu değil.
2. *`#[must_use]` iddiası yanlıştı* — yukarıda.
3. *`cell_px()` belgesi kodla çelişiyordu* ("tek meşru tüketicisi
   `SessionOptions`", oysa dört çağrı yeri var). `CLAUDE.md` kuralı gereği
   aynı commit'te düzeltildi: belge artık demetin **üç** meşru iniş yerini
   sayıyor (bölme aritmetiği, `bt-core` sınırı, `#[repr(C)]` kare kurucusu).
4. *`LinkIvars` demeti sakladığı için tip sınırda açılıyordu.* Alan
   `Cell<(u16, u16)>` → `Cell<CellMetrics>`; `DisplayLink::new` ve `resize`
   artık ölçüyü tip olarak saklıyor, demete inen tek yer `Frame::clear`.
5. *`izgaraya_bol`'un belgesi kapsamını aşıyordu* ("`CELL_PX`'in geri
   gelemeyeceğinin kanıtı bu imza") ve *`Grid`'in üç derive'ı kullanılmıyordu*.
   Belge artık sınanmayan satırı (`geometriyi_esitle`'deki `cell_metrics`
   çağrısı) adıyla söylüyor; `Grid` `#[derive(Clone, Copy)]`.

**`/code-review` bulguları — waive (4, gerekçeleriyle).**

- *Atlas ölçeği koşulsuz, `cell_px` koşullu uygulanıyor.* Gerçek bir yapısal
  ayrışma ama bugün zararsız: `Session::resize`'ın `false`'u ya "dejenere
  boyut" (çizilecek hücre yok) ya da "hiç değişmedi" (ölçü zaten aynı)
  demek ve sonraki geometri olayı ikisini eşitliyor. Doku gelince ucuz
  olmaktan çıkar → phase-4 devir maddesi 3.
- *Türetilen grid'in üst sınırı yok.* Senaryo gerçek (boşluk glyph'i olmayan
  bir font `bosluk_advance`'ten `0.0` döndürür, `yukari` onu 1 piksele
  kırpar, 1800 piksellik drawable 1800 sütun eder) ama phase-3'ün getirdiği
  bir şey **değil**: silinen `.max(1.0)` sabit bir `CELL_PX`'e uygulanıyordu
  ve bu yolu hiçbir zaman korumuyordu. Delik `bt-atlas`'ta ve phase-2'den
  önce de oradaydı; makul-ölçü politikası uydurmak bu phase'in işi değil.
- *Yeni `bt-gpu` sınamaları `bt-atlas` kapsamını tekrarlıyor ve Metal device
  istiyor.* Dosyadaki dört mevcut renderer sınaması da device kuruyor, yani
  örüntü yeni değil; `iki_olcek_iki_metrik_verir` atlası değil **dikişi**
  sınıyor (mutasyon doğrulaması: `ensure` düşürülünce bu sınama düşüyor) ve
  `hucre_olcusu_hic_sifir_olmaz` yukarıdaki 1. maddenin `expect`'inin üç
  ölçekte atmadığının bekçisi oldu. Cihazsız kalması gereken sınamalar
  (`app.rs`, `CellMetrics::new`) zaten cihazsız.
- *Ölçeğin `bt-gpu`'ya iki kapısı var* (`Surface::set_size` ve
  `cell_metrics`, `app.rs`'te komşu iki satır). Birleştirmek
  (`Renderer::resize(surface, w, h, scale) -> CellMetrics`) R5'in harfiyle
  çelişir; kayda geçti, doku gelince yeniden bakılacak.

**`/audit` — hangi mercek koştu.** İlgisiz sayılan dördü: **2** (yeni
bağımlılık — `Cargo.toml` ve `Cargo.lock` el değmedi, `bt-gpu → bt-atlas`
kenarı 001'den beri var), **4** (ayar/tema şeması — model henüz yok),
**5** (shell üçlüsü — `assets/shell/` el değmedi), **9** (hücre boyutu ve
shader/Rust düzeni — `.metal`, `build.rs` ve `bt_core::Cell` el değmedi).
Koşup **temiz** çıkan beşi: **1** katman yönü (`bt-shell`'de `bt_atlas`
yalnız iki yorumda geçiyor, kodda yok; `bt-core` ağacı ve kaynağı platformsuz;
`bt-atlas` Metal görmüyor), **3** panik yolu (`bt-core` diff'i boş; tek yeni
üretim `expect`'i `bt-gpu`'da, pencere geometrisi yolunda ve `// audit:`
gerekçeli), **6** ölçüm sahipliği (`CLAUDE.md` diff'inde sayı yok, ölçülmemiş
iddia yok), **7** thread ve blokaj (`cell_metrics` kare üreten yola girmiyor —
tek zinciri `geometriyi_esitle`; iki `RefCell` ödüncü iç içe geçmiyor;
tamamlanma bloğu `Renderer`'ı değil yalnız `Arc<AtomicU64>` ve `Arc<Retry>`'ı
yakalıyor, yani `Rc` GPU thread'ine düşmüyor; kilit sırası `term → size`
korunuyor), **8** boşta sıfır kare (diff animasyon/zamanlayıcı eklemiyor; kare
isteyen üç yer ve `needs_update`'in iki durma kapısı `HEAD` ile birebir aynı;
kendini besleyen döngü yok).

**`/audit` mercek 10 (belge ve üslup) dört bulgu verdi, dördü de düzeltildi.**
Hepsi aynı türden: bu diff'in kendi yazdığı bir "tek yer / yalnız X" iddiası
kodla çelişiyordu — `CLAUDE.md`'nin "çelişirse ikisinden biri aynı commit'te
düzelir" kuralı gereği belge tarafı düzeltildi.

1. `renderer.rs` altı yerde **`004`** diyordu (glyph yuvası, doku, `ensure`
   sinyali). Bu depoda `004` sonraki **iş setinin** adı (`BOLD`/`ITALIC`/
   `UNDERLINE`, `plan.md` R2.3), oysa kastedilen 003'ün **phase-4**'ü. İşi
   bir set ileri atan sessiz bir kayma olurdu; altısı da `phase-4` oldu,
   `phase-4.md`'deki aynı kayma da.
2. `CellMetrics`'in belgesi "kurucusu **yalnız** `Renderer::cell_metrics`"
   diyordu; `pub fn new` 25 satır altta duruyor ve `bt-shell` onu çağırıyor.
   Cümle `new` doğmadan önceki hâlden kalmıştı → "kurucusu sıfırı eleyen
   `new`".
3. Bu dosyanın kendi notu ("demet **tek yerde** açılıyor: `SessionOptions`")
   `/code-review` bulgusu 3'le düzeltilen iddianın eski hâlini ayakta
   bırakmıştı; üç iniş yerini sayacak biçimde eşitlendi.
4. `Grid.cell` ve `LinkIvars.cell` belgeleri alan ölçeğinde doğru ama dosya
   ölçeğinde yanıltıcıydı ("yalnız `SessionOptions`'a girerken açılıyor",
   oysa `izgaraya_bol` ve `Session::resize` de demete iniyor). İkisi de
   "**saklanan** değer" ayrımını yapacak biçimde yeniden yazıldı.

**Phase-4'e devredilen dört bulgu** `phase-4.md` → `## Phase-3'ten devir`
bölümünde: (1) `ensure` dönüşünün dokuya bağlanması, (2) atlası ödünç alan
tek yerin `Renderer::draw` olması — `link.rs`'in `frame.borrow_mut()` süresince
`draw` çağıran örüntüsü atlas için kopyalanırsa ilk glyph'li karede
`BorrowMutError`, (3) atlas kuşağının kabul edilen ölçüyle eşlenmesi,
(4) glyph kapısının ölçeği açıkça söylemesi:
`cell_bg_pikseli_gpu_tarafinda_boyar` `cell_metrics`'i hiç çağırmıyor, yani
atlas o sınamada `None` kalıyor.

## Yayın Etkisi

- **Türetilmiş dosya:** yok (`.metal`, `build.rs`, terminfo, bundle el
  değmedi; `Cargo.lock` oynamadı).
- **Ayar/tema/terminfo şeması:** değişmedi.
- **Duman sözleşmesi:** jeton listesi aynı (`kare=N hucre=K pipeline=ok`);
  `glif=` phase-4'te eklenir. `hucre=8` beklendiği gibi durdu.
- **Görünür davranış:** pencerede sütun/satır sayısı değişti — hücre artık
  gerçek font metriğinde. `TIOCSWINSZ` onu izliyor. Kullanıcı tarafında
  ayrıca yapılacak bir şey yok.
- **Belge:** `CLAUDE.md`'nin "kimse onu çağırmıyor" cümlesi öldü; hücre
  ölçüsünün `bt-atlas` → `Renderer::cell_metrics` yolundan geldiği ve
  glyph'in hâlâ çizilmediği yazıldı. `R7`'nin geri kalanı (jeton listesine
  `glif=`, katman tablosu) phase-4'ün işi.
- **Ölçüm bekliyor:** yok. Yukarıdaki metrik sayıları (`@1x (8,17)`,
  `@2x (16,32)`, `@3x (24,47)`) bu makinenin Menlo 13pt font ölçüsüdür ve
  `docs/OLCUMLER.md`'nin konusu **değildir**: orası kare süresi, giriş
  gecikmesi, bellek ve açılış ölçümlerinin sahibi (`proje.md`). Font
  metriğinin phase notunda durması phase-2'nin örneğini izliyor
  (`font.rs`'in ascent/descent yorumu) ve iddia sayıdan bağımsız: bekçisi
  `descender_hucreye_sigar`, metriği fontun kendisinden okuyor.

---

## Checklist

- [x] `bt-gpu` → `bt-atlas` bağımlılık kenarı (`Cargo.toml`) — **zaten vardı**
      (001 iskeleti, `3e006ba`); bu phase'de `Cargo.toml` değişmedi
- [x] `Renderer::cell_metrics(scale)` + `CellMetrics`, ölçek önbelleği
      (`HashMap` değil: tek `RefCell<Option<Atlas>>` + `Atlas::ensure`)
- [x] `bt-shell`: `CELL_PX` silindi, `geometriyi_esitle` renderer'dan okuyor
- [x] Test: iki ölçek iki metrik verir (`iki_olcek_iki_metrik_verir`);
      `bt-shell` yer tutucuyu artık taşımıyor (`hucre_olcusu_disaridan_gelir`);
      ikisi de mutasyonla doğrulandı
- [x] Doğrulama geçti: `make hepsi` (0), `make duman`
      (`kare=1 hucre=8 pipeline=ok`), `make test-yaris` (0). Koşulu
      tetiklenmeyenler — `make shader` (`.metal`/`build.rs` el değmedi),
      `make terminfo` ve `make kur` (girdileri henüz yok, `proje.md`'nin
      bilinen listesi): bunlar atlanmış kapı değil, **koşulu doğmamış** kapı
- [x] `/simplify` çalıştırıldı, bulgular uygulandı (8 uygulandı, 3 waive,
      3 phase-4'e devir)
- [x] `/code-review` çalıştırıldı, bulgular giderildi (9 bulgu: 5 uygulandı,
      4 waive — gerekçeleri `## Uygulama Notları`'nda)
- [x] `/audit` çalıştırıldı, bulgular giderildi (5 mercek temiz, 4 ilgisiz,
      mercek 10'un 4 belge–kod çelişkisi düzeltildi)
- [x] Yayın etkisi "Yayın Etkisi" bölümüne yazıldı
- [ ] Commit: {hash}
