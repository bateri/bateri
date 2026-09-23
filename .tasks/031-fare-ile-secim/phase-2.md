# Phase 2 — `selection` rolü ve düz satır koşuları

## Özet

Vurgu ters videodan temanın `selection` rengine geçiyor: `frame()` satır
koşularını veriyor, `bt-gpu` onları zeminle glyph'ler arasına düz dörtgen
olarak çiziyor; odaksız pencerede soluk.

_Requirements: R2.1, R2.2, R2.3, R2.4, R2.6_

## Değişiklikler

- **`crates/bt-core/src/color.rs`** — `Theme::selection` alanı, `BATERI` ve
  `BATERI_LIGHT` değerleri (**zevk kararı, gözle kontrol**; ölçüt: varsayılan
  ön planla ve paletin renkli sekizlisiyle okunur, zeminden ayrışır),
  `selection_linear()` ve `selection_unfocused_linear()` (`dim_toward` ile
  zemine üçte bir; `quiet_linear` emsali). Rol listesinin doc'u.
- **`crates/bt-core/src/theme.rs`** — `parse`'ın `roles` dizisine `selection`;
  eksikte tabandan (istisnasız kural). Belgedeki tema bloğunun sınaması yeni
  anahtarı ayrık tabanla okur.
- **`crates/bt-core/src/session.rs` → `frame()`** — hücre başına `inverse ^
  selected` takası kalkar. Seçili hücre: zemin çizilmez (seçim şeklinin
  altında kalır), metin hücrenin kendi ön planıyla, ters video çözülmüş
  (Karar 3). Satır başına ilk/son çizilir seçili sütun biriktirilir ve çağıranın
  sahip olduğu bir `&mut` tampona (`SelectionRuns`; `Blocks` emsali, kare başına
  ayırma yok) yazılır. **Takip atlama kapısından önce**: varsayılan zeminli
  spacer ve boşluk artık zemin boyamadığı için kapıya takılıyor, sonra olsaydı
  seçili CJK'nın sağ yarısı koşudan düşerdi. `hidden` hücre kenar koşusunu
  belirlemez ama içeride köprülenir. Dolu satırlar sayısı (`content_rows`)
  seçimden etkilenmez — seçim içerik yaratmaz.
- **`crates/bt-core/src/lib.rs`** — `SelectionRuns` (ya da koşu tipi) `pub`;
  sınır tipi alacritty görmez.
- **`crates/bt-gpu/src/frame.rs`** — ızgara koşuları için ayrı bir liste;
  koşu başına bir `Instance` (renk: odağa göre iki renkten biri), zemin listesinden
  sonra, glyph listesinden önce encode edilir. Liste `hucre=`/`glif=`/`kural=`
  sayaçlarından muaf (`renderer.rs`'in dock/fill listelerinin emsali).
  **Sıra: zemin → seçim → caret → glyph** — imleç seçimin üstünde kalır
  (bugünkü "imleç kazanır" kuralı); aynı sıra dock'ta da.
- **`crates/bt-gpu/src/link.rs`** — tamponun sahibi ve `frame()`'e geçişi;
  odak zaten `set_focused`'la geliyor ve bir içerik karesi istiyor — seçim
  rengi de o karede değişir, yeni bir kare kaynağı yok.
- **`crates/bt-gpu/src/renderer.rs`** (sınamalar) — offscreen bekçi: seçili
  bir hücrenin pikseli seçim renginde, üstündeki glyph kendi renginde. Renk
  temadan değil **ara tonlu** bir sabitten (`renderer::tests::MIDTONE`
  emsali — sRGB'nin sabit noktaları körleştirir).
- **`crates/bt-core/src/session.rs`** (sınamalar) — ters videoyu çivileyen
  bekçiler (`selected_cells_are_inverted_through_existing_pipe` ve
  seçimle ters videonun etkileşimini sınayanlar) yeni kurala göre yeniden
  yazılır: koşu sınırları, köprülenen boşluk, boş satırın koşusuzluğu, seçili
  ters videolu hücrenin ön planı, CJK'nın iki yarısı.
- **`docs/AYARLAR.md` → Temalar** — Biçim tablosuna `selection`, "Altı rol"
  cümlesi, iki gömülü tema tablosu; eksik `selection`'ın gömülü tabandan
  geldiği ve açık bir temada yazılması gerektiği notu.
- **`CLAUDE.md`** — "Tema = dokuz rol" maddesi (roller ve tüketilenler),
  "Seçim içeriği vurgular…" paragrafının birimi (hücre → satır koşusu, köprü),
  seçim renginin sınırdan lineer geçtiği.

## Kabul

- Sınama: `echo hello world` seçilince tek koşu, kelime arası boşluk dahil;
  satır sonundaki boşluk dışarıda; boş ekranda sürükleme koşu üretmez.
- Offscreen bekçi yeşil (piksel seçim renginde, glyph kendi renginde).
- `make hepsi` yeşil; `make duman` jetonları değişmez (duman koşusunda seçim
  yok).

## Checklist

- [ ] Rol, iki gömülü değer, iki lineer erişimci
- [ ] `frame()`: takas kalkar, koşular kapıdan önce biriktirilir
- [ ] `bt-gpu`: koşu listesi, sıra, sayaçtan muafiyet, odak rengi
- [ ] Test: koşu sınamaları; offscreen bekçi; eski ters video bekçileri yeniden
- [ ] `docs/AYARLAR.md`, `CLAUDE.md`
- [ ] Doğrulama geçti (`make hepsi`, `make duman`)
