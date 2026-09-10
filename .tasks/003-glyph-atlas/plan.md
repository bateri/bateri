# Glyph ve atlas

## Hedef

Pencerede gerçek harfler belirir: `bt-atlas` CoreText ile glyph'leri rasterize
edip bir atlas dokusuna dizer, `bt-core`'un `frame()` sınırı karakteri ve ön
plan rengini de geçirir, `bt-gpu` `cell` pipeline'ıyla glyph'leri hücre
arka planlarının üstüne çizer, `bt-shell`'in `CELL_PX` yer tutucusu ölür ve
grid ölçüsü gerçek font metriğinden türer. 002'nin körlemesine yazma dönemi
biter.

## Gereksinimler

- **R1** — `bt-atlas` CoreText'i kapsüller: `pub` API'de `CTFont` görünmez;
  crate `objc2-core-text` ve `objc2-core-graphics` dışında platform kütüphanesi
  görmez ve **Metal görmez**.
  - **R1.1** — Font zinciri: SF Mono → Menlo (Menlo garanti taban). CoreText
    font bulunamadığında **hata vermez, ikame eder**: istenen ad ile dönen ad
    karşılaştırılır ve ayrışma bir satır uyarı basar, yoksa yanlış font sessiz
    kalır.
  - **R1.2** — Metrik: hücre genişliği advance, yüksekliği ascent+descent+leading
    yuvarlanarak. **Önbellek anahtarı ilk commit'ten itibaren (font, punto,
    backing scale) taşır** — @1x'te rasterize edilmiş glyph @2x'te hatasız
    bulanıklaşır ve belirti yalnız iki ekranlı makinede görünür.
  - **R1.3** — `bt-atlas` **CPU bitmap + yerleşim dikdörtgeni** verir; dokunun
    sahibi `bt-gpu`'dur (`replaceRegion`). Bölüşüm crate'in kendi sözleşmesinden
    (`lib.rs`: "Metal görmez") türüyor.
  - **R1.4** — Atlas: tek `R8Unorm` doku, **sabit yuva ızgarası** (paketleyici
    yok, tahliye yok); `yuva_no → uv` aritmetiktir çünkü emoji ve kutu çizim
    kapsam dışı, yani tüm glyph'ler hücre boyutunda. Doluluk bir sayaca bağlanır
    ve **dolu atlasta rezident bir "tofu" kutusu çizilir** — sessiz kayıp
    görünür kayba döner.
  - **R1.5** — Sınama: `cargo test -p bt-atlas` GPU'suz koşar; metrik makul
    aralıkta, aynı karakter iki kez sorulunca aynı yuva, dolu atlas tofu verir.
- **R2** — `bt-core` `frame()` sınırı genişler: `CellBg` →
  `Cell { col, row, ch: char, fg: [f32; 4], bg: Option<[f32; 4]> }`, tek sink.
  - **R2.1** — `bg` **`Option`**: `None` = varsayılan arka plan. `Frame` yalnız
    `Some` gördüğünde `bg_count`'u artırır, yani `hucre=K` jetonunun anlamı
    **bit bit korunur** ve `sabit_shell_arka_plan_hucreleri_verir` oynamaz.
  - **R2.2** — `Flags::WIDE_CHAR_SPACER` hücreleri elenir; elenmezse CJK
    satırlarında ikinci hücreye hayalet glyph çizilir.
  - **R2.3** — Biçim bayrakları **geçmez**. `INVERSE`/`DIM` zaten `bt-core`
    içinde renge çözülüyor; `BOLD`/`ITALIC` (ikinci font yüzü) ve
    `UNDERLINE`/`STRIKEOUT` (kural çizgisi) 004'ün işi. Alacritty'nin `Flags`'i
    hiçbir hâlde yeniden ihraç edilmez.
  - **R2.4** — Atlama koşulu `bg == None && ch == ' '`; koşul biçim
    bayraklarını da elemeli, yoksa altı çizili boşluk hücreleri sessizce
    kaybolur (004'te bayraklar gelince).
- **R3** — sRGB geçişi, **glyph'ten önce ve tek başına**.
  - **R3.1** — Yüzey ve pipeline `BGRA8Unorm_sRGB`.
  - **R3.2** — Palet lineerleştirilir. `color::rgba()` bugün sRGB-kodlu float
    veriyor (`c / 255.0`) ve `const fn`; sRGB hedefte bu değer **lineer**
    sayılır ve palet açılır (`0x1a1c21` ≈ `0x59` gri). `powf` stable'da `const`
    değil → 256 girdilik `const` tablo. Aynı floatlar `MTLClearColor`'a da
    gittiği için pencere zemini ve hücreler tek kaynaktan düzelir.
  - **R3.3** — `cell_bg_pikseli_gpu_tarafinda_boyar` bu geçişi **göremez**:
    girdileri yalnız `0.0` ve `1.0` ve ikisi de sRGB transfer fonksiyonunun
    sabit noktaları. Aynı commit'te sınamaya **ara ton** bir renk girer
    (`0x1a1c21` gidip `0x1a` geri okunmalı).
  - **R3.4** — `hedef_doku` piksel formatını elle yazmayı bırakır,
    `Renderer`'dan okur; yoksa pipeline sRGB'ye geçtiğinde sınama assert'le
    değil Metal doğrulama istisnasıyla düşer.
- **R4** — `bt-gpu` `cell` pipeline'ı: ayrı `.metal`, ayrı `#[repr(C)]`
  instance ve **iki taraflı** `static_assert`/`offset_of!` çifti; blend
  `src_alpha`/`one_minus_src_alpha`, atlas `R8Unorm` örneklenir.
  - **R4.1** — Çizim sırası: `cell_bg` (arka planlar **+ imleç**) → `cell`
    (glyph'ler üstte). Bugün imleç `Frame`'in sonunda ve opak; glyph gelince
    imlecin altındaki harfi örterdi.
  - **R4.2** — İki pipeline tek render pass'te, `viewport_px` uniform'u ortak.
    `build.rs` değişmez: dizini tarayıp her `.metal`'i linkliyor.
- **R5** — `bt-shell`: `CELL_PX` sabiti **silinir**; grid ölçüsü
  `Renderer::cell_metrics(scale)`'ten gelir. `bt-shell` `bt-atlas`'ı görmez —
  metrik `bt-gpu` üzerinden geçer, katman tablosu değişmez.
- **R6** — Duman sözleşmesi jeton **ekler**: `kare=N hucre=K glif=G pipeline=ok`.
  Kapı `g > 0` de sorar. Bu süs değil kapının kendisi: boş bir atlas ve hiç
  çizmeyen bir glyph yoluyla `kare=1 hucre=8 pipeline=ok` yine basılır ve
  `make duman` yeşil geçerdi. `G` sabit shell betiğine bağlanır
  (`" bateri "` → 6 boşluksuz glyph) ve tek sahibi `bt_core::smoke_shell`'dir.
  Yanına GPU tarafı kanıtı gelir — **tam bayt assert etmeden**: hücre içinin
  arka planla tekdüze olmadığı doğrulanır, yoksa kapı sistem fontunun sürümüne
  rehin olur.
- **R7** — Belgeler aynı commit'te: `CLAUDE.md` (`bt-atlas` "hâlâ boş" cümlesi
  ölür, katman tablosunda `core-text`/`core-graphics` → `objc2-core-text`/
  `objc2-core-graphics`, jeton listesine `glif=`), `proje.md`'nin `duman`
  satırı, `app.rs`'teki `CELL_PX` yer tutucu yorumu.

## Yaklaşım

1. **Phase-1 sRGB** — yüzey ve pipeline `BGRA8Unorm_sRGB`, palet
   lineerleştirme tablosu, `hedef_doku` formatı `Renderer`'dan, offscreen
   sınamasına ara ton. Glyph yok. **Bölünemez**: yarısı (sRGB yüzey,
   lineerleşmemiş palet) tam olarak commit edilmemesi gereken durum.
2. **Phase-2 `bt-atlas`** — bağımlılıklar, CoreText raster, sabit yuva
   ızgarası, metrik, tofu. Saf `cargo test -p bt-atlas`; ekranda hiçbir
   değişiklik, `make duman`'a sıfır risk. Yarım kalırsa `bt-gpu` kullanmayan
   bir bağımlılık kenarıyla yaşar — bugün de öyle yaşıyor.
3. **Phase-3 metrik geçişi** — `Renderer::cell_metrics(scale)`, `CELL_PX`
   silinir, `bt-shell` ondan okur. Hücreler boy değiştirir; `hucre=8` durur
   (900×600 penceresine sekiz hücre her makul metrikte sığar).
4. **Phase-4 glyph** — `frame()` sınırı, `cell` pipeline, imleç sırası,
   `glif=` jetonu ve kapı, belgeler. Atomik ve geri alması en pahalı phase.
   Uzarsa dikiş: önce genişlemiş sink'i renderer glyph verisini **yoksayarak**
   al (görünmez, yeşil, `hucre=` sabit), sonra pipeline.

## Kapsam Dışı

Emoji ve renkli glyph (ayrı BGRA doku + ayrı örnekleme yolu), kutu çizim
karakterleri (`boxdraw` — bedeli TUI çerçevelerinde bir piksellik boşluk,
kayda geçti), `BOLD`/`ITALIC`/`UNDERLINE`/`STRIKEOUT`, ligatürler, RTL ve
karmaşık şekillendirme, paketli font (`.app` yok) ve `family` ayarı (ayar
dosyası yok), LRU tahliye (`/measure` sonrası yeniden açılır), imleç ve glyph
animasyonları (`cursor_motion`, `shatter`, `keypress` — bu setin iki pipeline
kararı onları **mümkün kılmak** için verildi, kendileri sonraki setin işi),
tema ve materyal yüzey, seçim/kopyalama.

## Göç

Ayar dosyası, tema ve terminfo değişmiyor. `make duman` çıktısı jeton
**ekler** (`glif=`), eskisini korur — okuyan taraf tanımadığı jetonu atlar.
Hücre boyutu yer tutucudan gerçek metriğe geçtiği için pencerede görünen
sütun/satır sayısı değişir; PTY'ye giden `TIOCSWINSZ` onu izler.

## Akış

```
bt-atlas (CoreText, Metal görmez)          bt-gpu
─────────────────────────────────          ──────
CTFont(SF Mono → Menlo)                    Renderer
  │ advance, ascent, descent                 │ cell_metrics(scale) ──► bt-shell
  ▼                                          │                         (CELL_PX ölür)
metrik (font, punto, scale) anahtarlı        │
  │                                          ▼
  ├─ slot(ch) → yuva_no ────────────────►  atlas dokusu (R8Unorm)
  └─ CPU bitmap + yerleşim ──────────────►  replaceRegion

bt-core                                    çizim (tek render pass)
───────                                    ───────────────────────
frame(sink: FnMut(Cell)) → Option<Cursor>  1. cell_bg: arka planlar + imleç
  Cell { col,row,ch,fg,bg: Option }        2. cell:    glyph'ler (blend, üstte)
  WIDE_CHAR_SPACER elenir                     yüzey BGRA8Unorm_sRGB
                                              palet lineerleştirilmiş
```

## Durum

| Phase | Durum | Commit |
|-------|-------|--------|
| phase-1 | ✅ | b5a0585 |
| phase-2 | ✅ | ae50826 |
| phase-3 | ✅ | 2f19277 |
| phase-4 | ✅ | 3c91814 |
