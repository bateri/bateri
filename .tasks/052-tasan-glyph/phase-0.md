# Phase 0 — `1.0`'daki fazladan pikseli kaldır

## Özet

`line_height = 1.0` tam olarak fontun yüksekliği oluyor. Varsayılan hücre
1 px kısalıyor. Yuva işinden ayrı bir phase, böylece phase-1'in "`≥ 1`'de
aynı" kanıtı boş bir fark olabiliyor ve kullanıcının göreceği varsayılan
değişikliği kendi commit'inde duruyor.

_Requirements: R1.2_

## Değişiklikler

- **`crates/bt-atlas/src/rules.rs`** — `cell_metrics`'te `extra`
  `round_up`'tan (her ölçüyü `≥ 1`'e kırpar) değil işaretsiz `ceil`'den
  geçiyor, yani `1.0`'da `extra == 0`. "At 1.0 the surplus is zero" yorumu
  artık doğru. `1.0`'dan büyük değerlerde hücre bugünküyle aynı.
- **Hücre ölçüsünü sabitleyen sınamalar** yeni ölçüye geçiyor. Beklenen
  değerler sınamanın kendisinden ya da fontun metriğinden türüyor, elle
  sayı yazılmıyor:
  - `crates/bt-gpu/src/renderer/tests.rs`: `(9, 18)` beklentileri,
    `context_cell_px`, `fitting_cell_px`;
  - `crates/bt-gpu/src/frame.rs`: zemin örneğinin `size` beklentileri;
  - `crates/bt-atlas/src/lib.rs`: `line_height_grows_the_cell_and_keeps_the_glyph_centred`'in
    "+1" borç yorumu ve aritmetiği, `metrics_are_in_a_sane_range`.
- **Eskiyen gerekçe metinleri**:
  - `raster.rs`'in "8×18 hücre" örnekleri.
  - `CLAUDE.md`'deki gölgeler gerekçesi ("13pt@2x hücresi 16×33, yüksekliği
    tek"). Karar aynı kalıyor: dama ancak adım hücrenin iki ölçüsünü de
    bölerse döşer. Örnek sayı ya kaldırılıyor ya da yeni ölçüden türetiliyor.
  - `docs/OLCUMLER.md`'nin tarihli satırları kayıt olarak kalıyor. Yeni ölçü
    iddiası yazılmıyor.
- **`docs/YOL-HARITASI.md`** — `line_height` piksel borcu satırı (019'dan)
  varsa kapanıyor.

## Kabul

- `line_height_one_is_the_natural_height`: `1.0`'da `cell_px.1 ==
  round_up(ascent) + round_up(descent + leading)`, taban çizgisi
  `round_up(ascent)`.
- `raster_digest` (üst commit ile ağaç): fark yalnız `line_height = 1.0`
  konfigürasyonlarının metrik ve yuva baytı satırları. `1.2` satırları aynı.
  Sayılar phase notuna yazılıyor.
- `make check`, `make linux`, `make smoke`.
- Gözle: varsayılan ayarla satırlar 1 px sık. Kutu çizgileri (`tree`) ve
  bloklar yine bitişik.

## Checklist

- [x] `extra` `ceil` ile
- [x] Sınamaların beklentileri metrikten türüyor
- [x] `raster.rs`, `CLAUDE.md`, `docs/YOL-HARITASI.md` metinleri
- [x] `raster_digest` farkı yalnız `1.0` satırları
- [x] Doğrulama geçti: `make check`, `make linux`, `make smoke`

## Uygulama Notları

- `bt-gpu`'nun `(9, 18)` beklentileri (`renderer/tests.rs`, `frame.rs`)
  atlastan değil elle kurulan sentetik `CellMetrics`/ızgaradan geliyor;
  ölçü değişikliği onlara değmedi, dokunulmadı. Kırılan tek sınama listede
  olmayan `the_default_size_keeps_todays_texture`'dı (13pt@2x kapasitesi
  1984 → 2048, 16×33 → 16×32): beklenen artık tabanın hücre ızgarasından
  türüyor; `docs/OLCUMLER.md`'nin 1984'ü tarihli kayıt olarak kaldı.
- `PROCEDURAL_SIZES`'ın "tek yükseklik" gerekçesi 13pt@2x'ten (artık
  16×32) 13pt@1x'e (8×17) geçti; küme aynı, tek yükseklik yine sınanıyor.
- `raster_digest` (HEAD ile ağaç, 38 596 satır): 15 989 satır farklı ve
  hepsi `lh1.0`; `lh1.2` satırlarında fark sıfır. `lh1.0`'ın 19 296
  satırının kalanı (yuva indeksleri, `Placed` cevapları) aynı. Hücreler:
  13pt@1x 8×18 → 8×17, 13pt@2x 16×33 → 16×32, 16pt@1x 10×20 → 10×19,
  16pt@2x 20×39 → 20×38; taban çizgileri yerinde.
- `make linux`'un ilk koşusunda `bt-shell-common`'ın
  `a_load_sample_answers_through_a_local_shell`'i (süreç örneklemesi,
  yüke bağlı) düştü; ikinci koşu yeşil, `bt-atlas`/`bt-gpu` ikisinde de yeşil.

