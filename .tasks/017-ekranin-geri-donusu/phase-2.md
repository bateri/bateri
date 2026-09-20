# Phase 2 — Doldurma: sınır tarafı

## Özet

`frame()` üstte kalan boşluk kadar geçmiş satırını okur ve sınırdan **ayrı**
verir; `Cursor` kaç satır dolduğunu söyler.

_Requirements: R2.1, R2.2, R2.3, R2.4, R2.5_

## phase-0'dan devralınan

**Doldurma ekranı Tab öncesine birebir değil, bir satır eksiğine döndürür** ve
bu bir aritmetik seçim değil zsh'in davranışı: Ctrl-C'nin `\r\r\n`'si yeni
prompt'u komut satırının bir altına indiriyor, yani listenin açtığı dört
satırın biri prompt tarafından tüketiliyor. `gap` olaydan **sonra** ölçüldüğü
için `fill = min(history_size, gap)` formülü **düzeltme istemiyor** (ölçülen
koşuda `min(24, 3) = 3`, doğru sayı). Sayılar `phase-0.md` → Uygulama
Notları §4. Yanlışın yönü güvenli: ekran dolu görünür, yalnız bir satır
yukarıdan başlar. Kabul ölçütü buna göre okunur — "Tab öncesinin **aynısı**"
değil, "delik yok ve içerik sürekli".

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `frame()`'e doldurma kolu.
  - `gap = rows - content_rows`, `fill = min(history_size(), gap)`. Koşul
    (R2.2): dock var **∧** `!alt_screen` **∧** bayrak temiz **∧**
    `display_offset == 0`. **Safha kapısı yok** — gerekçesi ölçülmüş
    (`discussion.md` → Karar 5): Enter kolunda `\e[J` `Running` safhasından
    geçiyor ve safha kapısı olsaydı o karede `fill == 0` kalır, dönüş
    animasyonsuz olurdu.
  - Geçmişten okuma **ayrı bir döngü**, stil değil zorunluluk: mevcut döngünün
    `debug_assert!((0..rows).contains(&row))` (`:1779`) negatif satırda patlar
    ve `drawn_rows` onları saymamalı. Giriş noktası `grid().iter_from(...)`
    ya da satır satır `grid()[Line(..)]`; her iki yolda da `grid_clamp` /
    `Boundary::Grid` ile sınırlanır — `bt-core`'da gerekçesiz panik yok (R2.5).
  - **`content_rows` ve `origin` aritmetiği değişmez** (R2.3). Doldurulan
    satırlar doluluğa **girmez**; girselerdi öteleme kapanır, içerik tabandan
    kopardı ve `session.rs:6283`'ün bekçisi kırılırdı — `27a0b98`'in
    maliyetini tekrarlamamanın tek yolu bu.
  - Doldurma hücreleri sink'e **fill-yerel** satırla (`0..fill`) gider; ekran
    satırına çeviren taraf çizen taraftır (phase-3).
- **`crates/bt-core/src/session.rs`** — `Cursor`'a `fill: u16`. Kare başına
  tek `Cursor` olduğu için bütçe kalemi yok; tüketicileri phase-3 ve phase-4.
- **Geri alma şeridi (R2.4):** doldurma tek boğaz noktasından geçer
  (`fill_rows()` gibi tek bir yüklem). Sıfır dönerken sınırdan geçen kare
  bugünküyle **bit bit** aynı olmalı ve bunu söyleyen bir bekçi yazılır —
  016'nın "yarıçap 0, hale 0" kolunun aynı örüntüsü.

## Kabul

- Ekran dolu → Tab → Ctrl-C reçetesinde `fill == gap` ve doldurulan satırlar
  **geçmişin en yenileri**, içeriğin hemen üstünde.
- Bayrak kurulu (Ctrl-L) → `fill == 0`.
- Alternatif ekran → `fill == 0`. Tekerlekle kaydırılmış pencere → `fill == 0`.
- Geçmiş boş (yeni oturum) → `fill == 0`, hiçbir ek okuma yok.
- `content_rows` değerleri bugünkü bekçilerle **aynı** kalıyor
  (`session.rs:6216`, `:6234`, `:6270`, `:6283` dokunulmuyor).
- `fill == 0` iken sınırdan geçen kare bugünküyle bit bit aynı (geri alma
  bekçisi).
- `make hepsi` ve `make test-yaris` yeşil.

## Yayın Etkisi

- **`CLAUDE.md`:** `frame()` sınırının anlattığı yere bir cümle — boşluk artık
  boş değil, geçmişle doluyor ve doluluğa girmiyor.
- **Ölçüm bekliyor:** doldurmalı karede sınır hücresi sayısı ve `Term` kilidi
  altındaki ek satır okumasının maliyeti — `frame()` bugün geçmişe hiç
  inmiyor (tek `history_size()` çağrısı `scroll_locked`'ın kırpmasında,
  `:3153`), bu phase kilidin altına `rows` satıra kadar yeni okuma koyuyor.
- shader / terminfo / ayar şeması / tema / shell entegrasyonu / app bundle /
  yeni bağımlılık: yok.

## Checklist

- [ ] `fill` hesabı ve dört koşullu kapı yazıldı
- [ ] Geçmişten okuma ayrı döngüde, `grid_clamp` ile sınırlı
- [ ] `Cursor::fill` eklendi
- [ ] Tek boğaz noktası (`fill_rows()`) ve geri alma bekçisi
- [ ] Test: Tab→Ctrl-C reçetesi (`fill == gap`, satırlar doğru)
- [ ] Test: bayrak / alternatif ekran / tekerlek / boş geçmiş → `fill == 0`
- [ ] Test: `content_rows` bekçileri dokunulmadan geçiyor
- [ ] Doğrulama geçti (`make hepsi` + `make test-yaris`)
- [ ] Riskli phase: `/code-review` koştu (paylaşılan durum), bulgular giderildi
- [ ] Yayın etkisi yazıldı
