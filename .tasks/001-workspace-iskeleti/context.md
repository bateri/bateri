# Workspace iskeleti — Bağlam

## Mevcut Durum

Depoda kod yok. `CLAUDE.md` mimariyi (beş crate, tek yönlü katmanlar,
`bt-core` platformsuz), `.claude/is-akisi/proje.md` doğrulama sözleşmesini
(`make hepsi`, `make shader`, `make terminfo`, `make test-yaris`, `make duman`,
`make kur`) tanımlıyor; ikisi de "ilk set kurar" diyor. Git deposu da yok:
`/implement` phase başına commit attığı için bu setin **ön koşulu** `git init`'tir.

Araç zinciri hazır (bu oturumda ölçüldü):

| araç | durum |
|---|---|
| rustc / cargo | 1.88.0 stable (Homebrew); nightly yok |
| Xcode | 16.2, `xcrun -sdk macosx metal` bir kernel'i derleyip `metallib` üretti |
| `tic` | `/usr/bin/tic` |
| macOS | 26.4.1 (Metal 3 ve 4; hedef macOS 14+) |
| cargo önbelleği | `objc2 0.6.4`, `objc2-app-kit 0.3.2`, `objc2-foundation 0.3.2`, `block2 0.6.2` indirilmiş; `objc2-metal`, `objc2-quartz-core`, `dispatch2` henüz değil |

`objc2-app-kit 0.3.2` 334 feature bayrağı taşıyor (başlık dosyası başına bir
bayrak: `NSWindow`, `NSApplication`, `NSView`, `NSEvent`, `NSMenu`...);
`objc2-quartz-core 0.3.2`'de `CAMetalLayer` ve `CAMetalDisplayLink` (macOS 14+)
bağlamaları var, ikincisi kendi feature bayrağının arkasında.
`objc2-metal 0.3.2`'de derlenmiş metallib için `newLibraryWithData_error(&DispatchData)`
(`dispatch2` feature'ı) ve `newLibraryWithURL_error(&NSURL)` mevcut.

## Motivasyon

Her sonraki iş bu iskelete yazılacak. İskeletin işi üç sözleşmeyi **koda
bağlamak**tır, özellik eklemek değil:

1. **Katman yönü** `cargo tree` ile denetlenebilir olsun — `/audit`'in ilk
   merceği beş crate'in varlığını ve `bt-core`'un platform kütüphanesi
   görmemesini ilk günden sorabilsin.
2. **Doğrulama hedefleri** gerçekten koşsun — `proje.md`'deki `make` adları
   bugün havada; phase'ler "yeşil" diyemiyor.
3. **Metal yolu uçtan uca açılsın** — `.metal` → `build.rs` → `metallib` →
   `MTLLibrary` → pipeline → `CAMetalLayer` → ekranda bir kare. Bu zincirin
   her halkası ilk terminal hücresi çizilmeden önce tek başına doğrulanmalı;
   glyph atlası (002+) bunun üstüne gelecek.

Referans: Metalterm aynı yapıyı taşıyor (`docs/ARASTIRMA.md` → "Nasıl yapılmış"):
dört crate + binary, tek `default.metallib` (`build.rs` çıktısı, binary'deki
yol `target/.../build/mt-gpu-*/out/default.metallib`), AppKit'e `objc2` ile
doğrudan bağlanma ve `metalDisplayLink:needsUpdate:` seçicisi — yani
`CAMetalDisplayLink`.

## Kanıt

- `xcrun -sdk macosx metal -c t.metal -o t.air && xcrun -sdk macosx metallib t.air -o t.metallib`
  → 2 983 baytlık metallib (bu oturum, 9 Eylül 2026).
- `objc2` deposundaki `examples/app/delegate.rs` ve `examples/metal/circle/main.rs`:
  `define_class!` ile `NSApplicationDelegate`, `NSWindow::initWithContentRect_styleMask_backing_defer`,
  `MTLCreateSystemDefaultDevice`, `MTLRenderPipelineDescriptor`, `presentDrawable` —
  hepsi `objc2 0.6` / `objc2-app-kit 0.3` API'siyle. Örnek `MTKView` kullanıyor;
  bizim seçimimiz `discussion.md` K2'de.

## Mevcut Mimari

Hedef (`CLAUDE.md`):

```
bateri (bin)  →  bt-shell  →  bt-gpu  →  { bt-atlas , bt-core }
 main, bundle    AppKit        Metal       CoreText   platformsuz
```

Bu sette dolan kutular: `bateri` (main), `bt-shell` (pencere, delegate,
display link), `bt-gpu` (device, library, pipeline, bir kare). `bt-core` ve
`bt-atlas` boş kalır ama var olur — sebebi K5'te.
