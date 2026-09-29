# Phase 7 — Metal'in sökümü, denetim ve sözleşme

## Özet

Bu phase'te yapılanlar:

- Metal kâhinini, `.metal` shader'larını, `build.rs`'i ve `bt-gpu`'nun
  platform bağımlılıklarını silmek.
- `make denetim`'i `bt-gpu`'nun platformsuzluğuna bağlamak.
- `make shader`'ı son hâline getirmek.
- Sözleşme belgelerini güncellemek.

Set kapısı bu phase'in commit'inden önce koşar.

_Requirements: R6.1, R6.2, R6.3_

_Kısıt: yazılan/taşınan kodun yorumları, doc-comment'leri ve tanı metinleri İngilizce (plan.md → Yaklaşım, dil kısıtı)._

## Değişiklikler

- **`crates/bt-gpu/src/renderer.rs`**
  - Metal kâhin modülü ve kâhin sahne listesinin Metal yarısı siliniyor.
  - Sahne listesi tek arka uçta **kalıyor mu** kodlayanın kararı: kalırsa
    anlamı "karenin bütün pipeline'ları birlikte" olur.
  - `metallib_is_embedded_and_valid` gidiyor. Ardılı phase-2'den beri var.
- **Silinenler**
  - `crates/bt-gpu/shaders/*.metal` ve `crates/bt-gpu/build.rs`.
  - `crates/bt-gpu/Cargo.toml`'dan `objc2*`, `dispatch2`, `block2` (dev
    dahil).
  - Workspace'te artık kimsenin kullanmadığı satır kalmışsa `Cargo.toml`
    yorumlarıyla birlikte o da gidiyor.
  - `bt-shell` hâlâ kullanıyorsa kalıyor.
- **`Makefile`**
  - `shader` yalnız WGSL kanaryası: pipeline kurulum sınaması. `.metal` kolu
    ve `touch` gidiyor.
  - `denetim`'e yeni satır: `bt-gpu`'nun **doğrudan** normal bağımlılığında
    (`cargo tree -p bt-gpu -e normal --depth 1`) ve kaynağında (yorum hariç
    grep) `objc2`, `dispatch2`, `block2`, `metal` yok.
  - wgpu'nun dolaylı çektikleri bu kontrolün konusu değil; gerekçe yorumda.
  - phase-5b ((a)) açıldıysa istisnası adıyla: tek modül, `cfg(target_os =
    "macos")`.
- **`CLAUDE.md`**
  - Proje paragrafı: "AppKit ve Metal'e doğrudan" → Metal wgpu üzerinden.
  - Pipeline anlatısı (Metal adları: `MTLClearColor`, `BGRA8Unorm_sRGB`,
    `setViewport`, `setVertexBytes`) wgpu karşılıklarına. Sözleşmenin
    anlamı değişmiyor, adları değişiyor.
  - "Taban macOS 14" maddesi: `build.rs`'in shader payı gitti; minos kaynağı
    kalıyor.
  - Tek cümle: Xcode komut satırı araçlarının `metal` derleyicisi artık
    derleme şartı değil.
  - Komutlar bloğunda `make shader` satırı.
  - "Renk uzayı sınırı geçer" maddesinin bekçi adları güncel.
  - Katman tablosu: `bt-gpu` "platform kütüphanesi görmez, `wgpu`";
    denetimin yeni satırı.
- **`crates/bt-gpu/src/lib.rs`** başlığı — "Metal renderer" → wgpu. Altı
  pipeline anlatısı aynı anlamla.
- **`.claude/is-akisi/proje.md`**
  - Doğrulama: `.metal` / `build.rs` satırı WGSL satırına iniyor.
  - Mekanik denetim açıklamasına `bt-gpu` platformsuzluğu.
  - Riskli phase tetikleyicisi yalnız WGSL.
- **`docs/YOL-HARITASI.md`**
  - 040 satırı tek satıra iniyor.
  - "`bt-gpu` donuk" notu kalkıyor.
  - Font setinin satırına: kapının `bt-gpu`'yu Linux'ta derlemek için
    beklediği tek şey `bt-atlas`.

## Kabul

- `make hepsi` (yeni denetim satırı dahil), `make shader`, `make duman` ve
  `make kur` yeşil. `make kur` paketin shader'sız da tam olduğunu denetliyor.
- `git grep -n "MTL\|objc2_metal\|\.metal\b" crates/bt-gpu` boş. Bulunan
  varsa yalnız tarihçe anlatan yorumdur ve gerekçelidir.
- `make linux` yeşil. `bt-core` değişmediyse koşulmaz, `[~]` gerekçesiyle.

## Checklist

- [ ] Yazılan/taşınan kodun yorumları ve tanı metinleri İngilizce
- [ ] Metal kâhini, `.metal`'ler, `build.rs` silindi
- [ ] `bt-gpu` platform bağımlılıkları (dev dahil) ve kullanılmayan workspace satırları gitti
- [ ] `make shader` WGSL-only; `make denetim` `bt-gpu` platformsuzluk satırı (+ (a) istisnası gerekiyorsa)
- [ ] `CLAUDE.md`, `bt-gpu` başlığı, `proje.md`, `docs/YOL-HARITASI.md`
- [ ] Doğrulama geçti (`make hepsi` + `make shader` + `make duman` + `make kur`)
