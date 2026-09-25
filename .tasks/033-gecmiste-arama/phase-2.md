# Phase 2 — Vurgunun çizimi ve tema rolleri

## Özet

`search_match` ve `search_current` temaya girer ve `bt-gpu` phase-1'in
koşularını 031'in `selection` pipeline'ıyla ızgarada ve doldurma bandında
çizer.

_Requirements: R2, R3_

## Değişiklikler

- **`crates/bt-core/src/theme.rs`** — iki rol, `roles` dizisi, iki gömülü
  temada değer (tasarım kararı, gözle kontrolle iner), lineer ve odaksız
  (`dim_toward`, `selection_unfocused_linear` emsali) karşılıkları;
  `SearchRuns` renkleri temanın aynı kopyasından hazır verir (031 Karar 9).
- **`crates/bt-gpu/src/frame.rs`** — ızgara ve bant için arama listeleri
  (`fill_rules` emsali); köşeler **eşleşme başına**: `selection_parts` her
  eşleşmenin kendi dilimiyle çağrılır, çünkü `selection_corners` satır başına
  tek koşu ve dizi komşuluğu varsayıyor. Yarıçap `SELECTION_RADIUS`.
- **`crates/bt-gpu/src/renderer.rs`** — `encode_selection`'ın ızgara ve bant
  viewport'unda ek çağrıları; sıra zemin → `search_match` → `search_current`
  → seçim → caret → glyph. Shader ve `#[repr(C)]` düzeni değişmez.
- **`crates/bt-gpu/src/link.rs`** — `push_*`'lar içerik karesinde, hareket
  karesi listeleri koruyor; renk odak bitinden.
- **`docs/AYARLAR.md`** → Temalar — iki rolün satırı ve örnek bloklar;
  belgedeki tema bloğunun sınaması yeni anahtarları görsün.
- **`CLAUDE.md`** — tema paragrafındaki tüketilen roller (`selection`
  emsali, modelin dışında) ve pipeline cümlesinde aramanın `selection`'ı
  paylaştığı.

## Kabul

- Offscreen sınamalar: eşleşme `search_match` renginde, geçerli
  `search_current` renginde; ardışık satırlardaki iki ayrı eşleşme iki şekil,
  sarılan tek eşleşme tek şekil; seçim aramanın üstünde; bant viewport'unda
  vurgu çiziliyor; arama kapalıyken kare bugünküyle aynı.
- Tema: eksik rol tabandan; belge bloğu sınaması yeşil.
- `make hepsi` yeşil.

## Checklist

- [x] Tema rolleri, gömülü değerler, odaksız karşılıklar
- [x] `Frame`: ızgara + bant listeleri, eşleşme başına köşe
- [x] `Renderer`: ek encode'lar ve sıra
- [x] `docs/AYARLAR.md`, `CLAUDE.md`
- [x] Test: yukarıdaki senaryolar
- [x] Doğrulama geçti (`make hepsi`; çizim yolu değişti, `make duman` de yeşil)
- [~] Riskli phase `/code-review`: tetiklenmedi — shader ve `#[repr(C)]` düzeni aynı, paylaşılan durum ya da kilit değişmedi

## Uygulama Notları

- **Test-first sırası:** sınamalar uygulamadan sonra yazıldı; ısırdıkları
  mutasyonla gösterildi (`continues` bölünmesini kaldırmak üç `Frame`
  sınamasını, arama encode'unu seçimin arkasına almak GPU sıra bekçisini,
  `search_current` satırını `roles`'tan silmek tema sınamasını, geçerli
  eşleşmeyi `#5a4718`'e açmak okunurluk bekçisini kırdı).
- **Renklerin sözleşmesi bir bekçi oldu**
  (`color::tests::search_highlights_keep_every_readable_text_readable`):
  031'in ölçütü (zeminde 3:1'i geçen her metin rengi vurguda da 3:1) ve
  "geçerli eşleşme zemine karşı ötekinden parlak". Seçimin değeri bu bekçiyi
  almadı — kapsam dışı.
- **Değerler** (WCAG, zemine karşı / en zayıf metin): koyu `search_match`
  `#302c1e` 1.50 / `red` 4.06, `search_current` `#503a0c` 1.95 / `red`
  3.13; açık `#f9f1d2` 1.05 / `bright_yellow` 3.37, `#fee29a` 1.17 /
  `bright_yellow` 3.01. Sıcak aile, seçimin soğuk arduvaz/buz mavisinden ton
  olarak ayrık. Açık temada ayrımı tavan (3:1) sıkıştırıyor; ilk aday
  (`#f8edc2`/`#fae3a0`) dökümde geçerli eşleşmeyi ötekinden zor ayırıyordu,
  eşleşme kreme soluklaştırılıp geçerli eşleşme bala doyuruldu.
- **Gözle kontrol offscreen dökümle** (512² kare, 18 satır; ön plan, `dim`
  ve 16 ANSI rengi, eşleşme + geçerli + üstünde seçim, iki tema): ⌘F
  phase-4'te geldiği için gerçek pencerede görülemedi; döküm kodu depoya
  girmedi. Gerçek pencere kontrolü phase-4'e devredildi.
- **Odak tek bit:** vurgu rengi bugün `link.rs`'in `focused`'ından (seçimle
  aynı); vurgu/seçim solmasının "yalnız key değilse" biti phase-4'ün "Odak
  iki bit" maddesi.
- `SearchRuns` artık renkleri taşıyor (`SelectionRuns` emsali), yani
  `Default`'u elle — `LinearRgba`'nın `Default`'u yok.
