# Glyph ve atlas — Bağlam

## Mevcut Durum

002 pencereye gerçek bir shell getirdi ama **harf çizmiyor**. Zincir uçtan uca
çalışıyor: `bt-core` PTY'yi okuyup `alacritty_terminal`'in grid'ini besliyor,
`Session::frame()` kirli kareyi veriyor, `bt-gpu` `cell_bg` pipeline'ıyla
hücre arka planlarını ve imleç bloğunu çiziyor, `CAMetalDisplayLink` ritmi
sürüyor, klavye `BateriView`'dan PTY'ye akıyor. Ekranda görünen tek şey
**renkli dikdörtgenler**.

Sınır tam olarak `Session::frame()`'in imzasında:

```rust
pub fn frame(&self, sink: impl FnMut(CellBg)) -> Option<Cursor>
pub struct CellBg { col: u16, row: u16, rgba: [f32; 4] }
```

Hücrenin karakteri (`cell.c`), ön plan rengi (`cell.fg`) ve biçim bayrakları
(`BOLD`, `ITALIC`, `UNDERLINE`, `STRIKEOUT`) grid'de **var** ve `bt-core`
onlara bakıyor bile — `INVERSE` ve `DIM` zaten okunuyor — ama sink'ten
dışarı yalnız arka plan çıkıyor.

`bt-atlas` crate'i **tamamen boş**: bir modül yorumu ve bağımlılıksız bir
`Cargo.toml`. Yorumu bu işin sözleşmesini şimdiden yazıyor: "CoreText ile
rasterizasyon, atlas paketleyici, kutu çizim karakterleri ve font seti burada
yaşar (003+). Yalnız `core-text` ve `core-graphics` görür; AppKit ve Metal
görmez."

Hücre ölçüsü bir **yer tutucu**: `bt-shell/src/app.rs`'te
`const CELL_PX: (f64, f64) = (9.0, 18.0)`, kendi yorumunda "gerçek font
metriği `bt-atlas` ile (003) gelene kadar" diyor. Grid boyutu ve PTY'ye giden
`TIOCSWINSZ` bu sayıdan türüyor.

Çizim yüzeyi `BGRA8Unorm` — **sRGB değil**. Bugün doğru görünüyor çünkü
çizilen her şey opak: blend yok, gamma sorusu hiç sorulmadı. 002 sRGB kararını
açıkça kapsam dışına attı ve 003'e bıraktı.

## Motivasyon

Terminal bugün **körlemesine yazılıyor**. Kullanıcı 002'nin göz kontrolünde
tam olarak bunu yaşadı: pencerede imleç bloğu var, `sh /tmp/r` yazınca renkli
bloklar beliriyor, ama yazdığı komut görünmüyor. `ls --color` bile hiçbir şey
göstermiyor — çünkü `ls` **ön plan** rengi basıyor (`ESC[35m`), arka plan
değil, ve bu renderer yalnız varsayılan olmayan arka planı çiziyor.

Glyph, "çalışan boru hattı"nı "kullanılabilir terminal"e çeviren tek eksik
parça. Ondan sonrası (tema, sekme, komut blokları, hareket) bir terminalin
üstüne bina; glyph binanın kendisi.

Referansın nasıl yaptığı `docs/ARASTIRMA.md` → "Nasıl yapılmış": `mt-atlas`
dört modül (`atlas`, `packer`, `raster`, `fontset`, `boxdraw`), `mt-gpu`'da
`cell` (glyph) ve `cell_rule` (alt çizgi) ayrı pipeline'lar, font zinciri
JetBrains Mono → SF Mono → Menlo → Apple Color Emoji. Kutu çizim
karakterlerinin ayrı bir modülü olması bir karar sinyali: font'tan alınmıyor.

## Kanıt

Kullanıcının 002 kapanışındaki göz kontrolü (10 Eylül 2026):

> "bir kare blok imleç var ama yazılarımı göremiyorum ki"

`ls`'in gerçekten arka plan basmadığı, bir tty'de doğrulandı:

```
$ script -q /dev/null /bin/ls -G -d /tmp /etc | cat -v
^[[1m^[[35m/etc^[[39;49m^[[0m ^[[1m^[[35m/tmp^[[39;49m^[[0m
```

`35` = magenta **ön plan**; arka plan kodu (`40`–`47`) yok. Yani bugünkü
renderer için `ls` çıktısı tanım gereği sıfır hücre üretiyor.

`make duman` bugün `kare=1 hucre=8 pipeline=ok` veriyor — sekiz hücre, sıfır
glyph. Duman sözleşmesinin bu sette bir jeton daha kazanması bekleniyor
(jetonlar silinmez, eklenir).

## Mevcut Mimari

```
bt-core                          bt-gpu                         bt-shell
───────                          ──────                         ────────
Term (alacritty)                 Frame                          BateriView
  │ cell.c      ✗ dışarı çıkmaz    │ Instance{pos,size,rgba}       │ keyDown:
  │ cell.fg     ✗ dışarı çıkmaz    │ 32 bayt, cell_bg.metal        ▼
  │ cell.flags  ✗ dışarı çıkmaz    ▼                             Session::write
  │ cell.bg     ✓                 cell_bg pipeline
  ▼                                │ instanced quad
frame(sink: FnMut(CellBg))  ─────► │ BGRA8Unorm, blend yok
  → Option<Cursor>                 ▼
                                 drawable → commit

bt-atlas: BOŞ (yalnız modül yorumu, bağımlılık yok)
CELL_PX = (9.0, 18.0) yer tutucu, bt-shell/app.rs
```

Katman düzeninin bu sette önemli olan yanı: **`bt-shell` `bt-atlas`'ı
görmez.** Bağımlılık `bateri → bt-shell → bt-gpu → {bt-atlas, bt-core}`.
Gerçek font metriği `bt-atlas`'ta doğacak ama grid ölçüsünü hesaplayan kod
`bt-shell`'de — metriğin `bt-gpu` üzerinden geçmesi gerekiyor.
