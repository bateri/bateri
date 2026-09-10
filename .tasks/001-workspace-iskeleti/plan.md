# Workspace iskeleti

## Hedef

Her sonraki işin üstüne yazılacağı iskelet: beş crate'lik cargo workspace,
`proje.md`'nin `make` sözleşmesini koşturan `Makefile` ve `.metal → build.rs →
metallib → MTLLibrary → pipeline → CAMetalLayer → ekranda bir kare` zincirinin
uçtan uca açılması. Özellik yok; üç sözleşmeyi koda bağlamak var.

## Gereksinimler

- **R1** — Depo `git` altındadır; `.gitignore` `target/`, `*.metallib`,
  `*.air`, `*.dSYM`, `*.dmg`, `*.app`, `.DS_Store`'u dışarıda tutar,
  `Cargo.lock` içeridedir. İlk commit yalnız belgeleri taşır.
- **R2** — `Cargo.toml` workspace'i beş üye taşır: `bt-core`, `bt-atlas`,
  `bt-gpu`, `bt-shell`, `bateri`; edition 2024, `rust-version = "1.88"`.
  - **R2.1** — Bağımlılık yönü `bateri → bt-shell → bt-gpu → {bt-atlas, bt-core}`;
    `cargo tree -p bt-core` ve `-p bt-atlas` `objc2` içermez; `bt-shell`
    `objc2-quartz-core`'u yalnız `CALayer` feature'ıyla görür.
  - **R2.2** — `bt-core` ve `bt-atlas` boştur: `lib.rs` başlığında sözleşme
    yorumu, sahte sınama yok.
- **R3** — `Makefile`: `hepsi` (`rustc --version` + fmt --check + clippy
  `-D warnings` + test), `test`, `shader`, `duman`; `terminfo`, `test-yaris`,
  `kur` hedefleri var olup "henüz yok — {set}" deyip `exit 78` ile çıkar.
- **R4** — `crates/bt-gpu/build.rs` `shaders/` altındaki her `.metal`'i
  `xcrun -sdk macosx metal -mmacos-version-min=14.0` ile derleyip
  `$OUT_DIR/default.metallib` üretir.
  - **R4.1** — `xcrun -f metal` başarısızsa Türkçe anlaşılır hata ile durur.
  - **R4.2** — `cargo:rerun-if-changed=shaders` dizin olarak verilir.
- **R5** — `bt-gpu`: metallib `include_bytes!` ile gömülür,
  `DispatchData::from_static_bytes` → `newLibraryWithData_error`;
  `Renderer::new(device) -> Result<_, GpuError>`; fonksiyon adı bulunamazsa
  adıyla hata; `Surface` `CAMetalLayer`'ı kurar ve `&CALayer` verir;
  `Renderer::draw(&Surface)` `nextDrawable` alıp tam ekran quad'ı shader ile
  çizer, present eder ve kare sayacını artırır.
  - **R5.1** — `cargo test -p bt-gpu` gerçek device üstünde library ve
    pipeline'ı kurar (device yoksa sınama `ignored` değil, **açık hata**).
- **R6** — `bt-shell`: tek `define_class!` (`NSApplicationDelegate` +
  `NSWindowDelegate`); `applicationDidFinishLaunching` pencereyi açar,
  `NSView`'a önce `setLayer`, sonra `setWantsLayer(true)`; ilk kareyi çizer;
  `windowDidResize` drawable boyutunu güncelleyip yeniden çizer; son pencere
  kapanınca uygulama biter.
- **R7** — `bateri` main: `BT_RUN_SECONDS` tavanı; süre dolunca kare sayısı
  > 0 ise `kare={n} pipeline=ok` basıp `exit 0`, değilse `exit 1`; Aqua
  oturumu yoksa `exit 78` + "ATLANDI: Aqua oturumu yok". Device/library
  hatası `stderr` + `exit 1`.
- **R8** — `CLAUDE.md` ve `.claude/is-akisi/proje.md` kodla aynı commit'te
  düzeltilir: `bt-shell` satırına `objc2-quartz-core (yalnız CALayer)`;
  "`Cell` 16 bayt, 001 ölçer" → 002; `make shader` gerekçesi "kanarya";
  ertelenen üç hedefin notu.

## Yaklaşım

1. **Phase-0** — `git init`, `.gitignore`, mevcut belgelerin commit'i. Kod yok.
2. **Phase-1** — workspace `Cargo.toml`, beş crate iskeleti, `Makefile`,
   `Cargo.lock`; belge düzeltmeleri (R8). Xcode olmayan makinede de yeşil.
3. **Phase-2** — `bt-gpu`: `shaders/quad.metal`, `build.rs`, `Surface`,
   `Renderer`, hata tipi, device üstünde sınama. `xcrun` ilk kez burada gerekir.
4. **Phase-3** — `bt-shell` pencere + delegate, `bateri` main, `BT_RUN_SECONDS`,
   `make duman`. WindowServer ilk kez burada gerekir.

Her phase tam bir dış bağımlılık ekler; biri kırılırsa öncekiler yeşil kalır.

## Kapsam Dışı

- VT motoru, PTY, grid, hücre yapısı ve `Cell` boyutu (002).
- `CAMetalDisplayLink`, kirli satır, boşta duraklama (002).
- Glyph atlası, CoreText (003+).
- `.app` bundle, `Info.plist`, imza, Sparkle (`make kur` ertelendi).
- terminfo, `TERM` adı, shell entegrasyonu.
- Ayar dosyası, tema.
- `rust-toolchain.toml`, `rustup`, nightly (TSan).

## Akış

```
cargo build -p bt-gpu
  └─ build.rs: xcrun -f metal ✓ → metal -c shaders/*.metal → metallib → $OUT_DIR/default.metallib
       └─ include_bytes! → DispatchData → MTLDevice.newLibraryWithData
            └─ quad_vertex / quad_fragment → MTLRenderPipelineState

bateri main
  └─ Aqua? (hayır → exit 78)
  └─ NSApplication + Delegate
       └─ applicationDidFinishLaunching
            ├─ MTLCreateSystemDefaultDevice → Renderer::new (Err → stderr, exit 1)
            ├─ Surface (CAMetalLayer) → NSView.setLayer → setWantsLayer(true) → NSWindow
            ├─ Renderer::draw(&surface)  →  kare=1
            └─ BT_RUN_SECONDS → NSTimer → kare>0 ? exit 0 : exit 1
       └─ windowDidResize → drawableSize → draw
```

## Durum

| Phase | Durum | Commit |
|-------|-------|--------|
| phase-0 | ✅ | d3581bf |
| phase-1 | | |
| phase-2 | | |
| phase-3 | | |
