# Phase 2 — Kayıt modeli ve dosyası (`bt-shell-common`)

## Özet

Platformsuz, saf ve sınanmış bir `restore` modülü düzeni sürümlü satır
biçimine yazar/okur ve dosyanın ömrünü (kilit, atomik yazım, tüketme,
süpürme) yönetir.

_Requirements: R2.1, R2.2, R2.3, R2.4_

## Değişiklikler

- **`crates/bt-shell-common/src/restore.rs`** (yeni, `lib.rs`'e kayıt) —
  - Model: `Saved { windows }`, `SavedWindow { frame, tabs, selected, key }`,
    `SavedTab { shape, focused, zoomed }`, `Shape` (`split::Tree`'nin ikizi,
    yaprağı sekmenin pane dizisindeki **indeks** — süreç içi `u64` kimlik diske
    yazılmaz), `SavedPane { tab_id: TabId, dir, zoom_steps, remote_line,
    history: bool }`. `split::Tree` ↔ `Shape` dönüşümü burada (kimlik → indeks
    yazarken, indeks → yeni kimlik okurken).
  - Biçim (`remote-hosts` emsali, `ssh_wrap.rs`): ilk satır `bateri-session
    {VERSION}`; `W`/`T`/`P` satırları, ağaç ön-sıralı jetonlar (`S h|v
    {ratio} … L {i}`), metin alanları (dizin, uzak satır) boşluk/sekme/satır
    sonu/ters bölü kaçışlı. Tanınmayan sürüm, bozuk satır, aralık dışı indeks,
    yaprak sayısı ile pane sayısının tutmaması, oranın `(0,1)` dışı → `None`.
  - Dosya: kök argüman (çağıran verir; sınamalar `TempRoot`), dizin `0700`
    ile kurulur, `flock`'lu bir kilit dosyası (`libc`, grafta) — `Lock`
    tutamağı düştüğünde bırakır; alamayan `None`. `save(&Lock, &Saved,
    histories)`: önce `{TabId}.vt`'ler (`0600`, geçici ad + `rename`), en son
    düzen; kayıtta adı geçmeyen `.vt` ve artık geçici dosyalar silinir.
    `take(&Lock) -> Option<Saved>`: düzeni okuyup **siler** (oynatmadan önce);
    `history(&Lock, &TabId) -> Option<Vec<u8>>` okuyup siler, yoksa `None`.
    `clear(&Lock)`: her şeyi siler (`"off"` ve pencere yokken).
- **`crates/bt-shell-common/src/zoom.rs`** — adımı okuyan ve adımdan `Zoom`
  kuran erişimci; `bigger`/`smaller` sınırları kurucuda da uygulanır.
- **`crates/bt-core/src/identity.rs`** — gerekirse `TabId`'nin metin hâli
  (`as_str` / `parse` var; yalnız eksikse).

## Kabul

- Biçim round-trip'i: çok pencere, çok sekme, iç içe ağaç, zoom'lu sekme,
  boşluklu/Türkçe karakterli/ters bölülü dizin, uzak satır.
- Ret kolları: tanınmayan sürüm, kesik dosya, bozuk ağaç, tutarsız indeks —
  hepsi `None`, panik yok.
- Dosya: izinler (`0700`/`0600`), `take`'in düzeni silmesi, ikinci `Lock`'un
  alınamaması, yetim `.vt` süpürmesi, `save` yarıda kalırsa eski düzenin
  okunabilir kalması (düzen en son yazılıyor).

## Checklist

- [x] Model ve `split::Tree` dönüşümü
- [x] Satır biçimi (yaz/oku) ve ret kolları
- [x] Dosya: kilit, atomik yazım, `take`/`history`/`clear`, süpürme
- [x] `Zoom` erişimcisi
- [x] Test: biçim round-trip, ret kolları, dosya ömrü
- [x] Doğrulama geçti (`make check` + `make linux`)

## Uygulama Notları

- Pane listesi `SavedTab::panes`'te (phase metnindeki `SavedTab { shape,
  focused, zoomed }`'un tamamlanmış hâli); `Shape`'in yaprakları o listenin
  indeksi.
- `take` yetimleri de süpürüyor: düzenin adını vermediği geçmişler ve
  geçiciler gidiyor; ayrıştırılamayan düzen geçmişlerini de götürüyor.
  `save` penceresiz kayıtta `clear`'a dönüşüyor.
- UTF-8 olmayan dizin yolu "yok" diye yazılıyor (pane ev dizininde doğar).
- `Shape::from_tree` `(0,1)` dışındaki oranı `0.5` yazıyor — okuyan taraf
  reddederdi ve tek ayırıcı yüzünden bütün düzen kaybolurdu.
- `TabId`'ye ekleme gerekmedi (`parse` / `as_str` yetti).
