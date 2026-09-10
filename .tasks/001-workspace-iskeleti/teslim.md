# Workspace iskeleti — Doğrulama & Yayın

> İlgili: [plan.md](plan.md) · [phase-0.md](phase-0.md) · [phase-1.md](phase-1.md) · [phase-2.md](phase-2.md) · [phase-3.md](phase-3.md)

Depo kod taşımaya başladı: beş crate'lik workspace, `make` sözleşmesi,
`.metal → build.rs → metallib → pipeline → CAMetalLayer → ekranda bir kare`
zinciri ve `make duman`. Dışarıya etki yalnız depo içi belgeler ve
`.cargo/config.toml` üzerinden; kullanıcı makinesinde dosya, `TERM`, ayar ya da
shell entegrasyonu değişmedi.

## A. Doğrulama (ÖNCELİK)

```sh
make hepsi
make shader
make duman
```

### Beklenen çıktı

- `make hepsi`: `rustc 1.88.0` satırı, fmt/clippy sessiz, `bt-gpu` 2 sınama geçer.
- `make shader`: `Finished` (kanarya; hata olsaydı `build.rs` düşerdi).
- `make duman`: stdout `kare=1 pipeline=ok`, çıkış 0. Başsız ortamda
  `ATLANDI: Aqua oturumu yok` + `Error 78`.

### Doğrulama Checklist

- [x] `make hepsi` yeşil (phase-3 sonunda, kapı sonrası koşuldu)
- [x] `make shader` yeşil
- [x] `make duman` → `kare=1 pipeline=ok`

## B. Yayın (doğrulamadan SONRA)

- **`[oto]`** — `/ship` kapsıyor (kalite kapısı + commit + `main`'e push).
- **`[komut]`** — kullanıcının tetiklediği komut ya da skill.
- **`[elle]`** — insan kararı/gözü gereken iş.

### B.1 Görsel kontrol `[elle]`

`make duman` bir kare çizildiğini kanıtlar, pencerenin görünür ve doğru
olduğunu değil (bundle'sız süreç öne çıkma hakkı taşımaz, ekran görüntüsü
pencereyi yakalamadı). Kullanıcı bir kez bakmalı:

```sh
cargo run -q -p bateri
```

Beklenen: koyu gri (`[0.10, 0.11, 0.13]`) pencere, siyah değil; kenardan
boyutlandırınca bulanıklık yok; kırmızı düğmeyle kapatınca süreç 0 ile biter
(⌘Q çalışmaz — menü çubuğu yok, kapsam dışı).

### B.2 Uzak depo `[elle]`

Depo yerel; `origin` tanımlı değil. `/ship` push edecekse önce uzak depo
adresi eklenmeli (`git remote add origin …`).

### Yayın Checklist

- [ ] B.1 görsel kontrol (phase-3 checklist'indeki iki `[~]` kutusu buna bağlı)
- [x] B.2 uzak depo tanımı (`origin` = git@github.com:bateri/bateri.git)
- [x] `[oto]` `/ship`: doğrulama + push (8 commit, `main`)

## Geri Alma

Her phase tek commit; sıra bozulmadan `git revert` edilebilir:
`a4c5b79` (phase-3) → `905aa50` (phase-2) → `3e006ba` (phase-1). Durum
commit'leri (`bb3e6b1`, `4cfd712` ve bu dosyanın commit'i) yalnız `.tasks/`
taşır. Kök commit `d3581bf` revert edilmez. `.cargo/config.toml`'un kaldırılması
binary'nin minos'unu 11.0'a düşürür ve `build.rs`'i "tanımsız" ile durdurur —
ikisi birlikte gider.

## 002'ye devredilen notlar

Phase notlarından derlendi; 002'nin `context.md`'si buradan başlar:

- `draw` drawable alır; display link gelince `draw_surface`/resize yolu yalnız
  kirli işaretler. `waitUntilCompleted` kalkınca drawable geri basıncı
  `nextDrawable`'a taşınır; sayaç anlamı `addCompletedHandler`'a.
- Duman çıkışı `process::exit` yerine `stop:`/`terminate:` üzerinden
  `applicationWillTerminate:`'a uğramalı (PTY çocuğu gelince `Drop` şart).
- `Cell` 16 bayt hedefi ve `const` assert 002'de ölçülür.
- sRGB pixel format kararı glyph harmanlamasıyla birlikte verilir.
- İkinci `NSView` sınıfı (`BateriView`) klavye/IME/display link ile gelir.
- Aqua tespiti `bt-shell`'e `CGSessionCopyCurrentDictionary` ile taşınabilir
  (`objc2-core-graphics` bağımlılık kararı).
