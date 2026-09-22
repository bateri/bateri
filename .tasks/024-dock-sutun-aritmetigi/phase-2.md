# Phase 2 — Tazelik kapısının iki tarafı aynı birimi okur

## Özet

`DockState::last_ink` sıfır genişlikli kod noktalarını atlıyor, böylece
birleştirici taşıyan bir satır (`❤️`, ZWJ) bastırmayı bırakmıyor.

_Requirements: R4, R4.1, R4.2_

## Değişiklikler

- **`crates/bt-core/src/shell.rs`** — `last_ink` bugün son boşluk olmayan
  `char`'ı alıyor. Ölçüt **genişliğe** genişliyor: sıfır genişlikli kod
  noktaları atlanıyor, çünkü ızgara hücresine hiç girmiyorlar (alacritty
  birleştiriciyi `CellExtra`'da tutuyor ve `cell.c` taban karakteri
  taşıyor). İki taraf aynı birimi okumazsa kapı **kalıcı olarak** "bayat"
  der: `❤️` yazan kullanıcının satırı her tuşta ızgaraya düşer.
  Ölçüt yine `unicode-width` (genişlik 0) — phase-1'in bağımlılığının ikinci
  tüketicisi ve aynı kaynaktan beslenmesi şart, yoksa kapı ile çizim
  ayrışır.
  Mevcut daraltma **korunuyor**: `is_whitespace()` değil `' '`/`'\t'`, çünkü
  kapının ızgara yarısı da öyle (`Session::last_ink_in_row`) ve NBSP taşıyan
  bir tampon iki tarafı ayrıştırırdı — gerekçesi o fonksiyonun doc'unda.

## Kabul

- `❤️` (U+2764 + U+FE0F) yazılan satır **bastırılıyor**: caret dock'ta,
  ızgara imleci gizli. Ölçülen kusurun tam tersi (`context.md` → Kanıt).
- Bastırmanın var olan bekçileri oynamıyor — özellikle bayat aynanın kendi
  sınaması (`a_stale_mirror_leaves_the_input_line_in_the_grid`): kapı
  **gevşemiyor**, yalnız birimi düzeliyor.
- `make hepsi` yeşil.

## Checklist

- [ ] `last_ink` sıfır genişliklileri atlıyor; `' '`/`'\t'` daraltması duruyor
- [ ] Test: VS16'lı satır bastırılıyor (caret dock'ta)
- [ ] Test: bayat ayna hâlâ bastırmayı bırakıyor (kapı gevşemedi)
- [ ] Doğrulama geçti (`make hepsi`)
