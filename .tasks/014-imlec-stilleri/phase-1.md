# Phase 1 — İmleç şekilleri

## Özet

DECSCUSR'ın üç şekli (blok, alt çizgi, dikey çubuk) sınırdan geçsin ve
çizilsin; kare altyapısına **hiç dokunulmasın**.

_Requirements: R1, R1.1, R1.2, R1.3, R2, R2.1, R2.2, R2.3, R3, R4.1, R4.3,
R4.4, R11.2, R12 (şekil yarısı)_

## Değişiklikler

- **`crates/bt-core/src/session.rs`** — `Cursor`'a `shape` alanı. Tip
  **bizim** enum'umuz (üç değer: blok, alt çizgi, dikey çubuk);
  `alacritty_terminal::vte::ansi::CursorShape` `pub` API'de görünmez. Kaynağı
  `Term::cursor_style()` — **aynı çağrı phase-2'nin blink bitini de veriyor**,
  yani o çağrı buraya bilerek kuruluyor. `Hidden` **taşınmaz**: `visible`
  (`session.rs:1401`) onu zaten tüketiyor ve iki yerde temsil edilen bir gerçek
  ayrışır. `HollowBlock` bloğa düşer — adlandırılmış karar, gerekçesi odağın
  sınırda olmaması (plan → Kapsam Dışı).
- **`crates/bt-core/src/session.rs` → `term_config`** — `default_cursor_style`
  artık `TerminalOptions`'tan kuruluyor, `..Config::default()`'a bırakılmıyor.
  Phase-1 yalnız **şekli** yazar; `blinking` varsayılanında (`false`) kalır.
- **`crates/bt-core/src/settings.rs`** — `[terminal] cursor`, varsayılan
  `"block"`. Örüntü `cursor_motion`'ın aynısı: kendi enum'u, private `name()`,
  `fallback` + `Diagnostic`. Bölüm **zaten var** (`scrollback`), yani iki kol
  da (bölümün `Some`'ı ve `None if root.contains_key("terminal")`) yazılı —
  yeni anahtar ikisine de eklenir, yeni bölüm kolu açılmaz. `TEMPLATE` satırı
  ve şablon anahtar listesi sınaması. **`Changes`'e alan eklenmez** —
  `Changes::terminal` zaten `TerminalOptions`'ı tamamıyla taşıyor.
  **`cursor_blink` bu phase'de yok**: ne şemada, ne `TEMPLATE`'te, ne belgede
  (R4.4).
- **`crates/bt-gpu/src/frame.rs`** — üç iş:
  1. `Caret` bir **ölçü** kazanır; `instance()` artık boyutu koşulsuz
     `cell_px`'e çivilemez.
  2. Daraltma **`grid_caret`/`dock_caret` instance'ı kurulurken** yapılır,
     `push_caret` içinde **değil** (R2.1): yuva seçimi (`frame.rs:674`) hücre
     ayak izine bakıyor ve daraltma ondan önce olursa underline caret'i banda
     değmeyip ızgara yuvasında kalır, dock'un opak zemini onu örter.
  3. `Frame` şekli **kendi alanında tutar**: `push_caret` yazar, `move_caret`
     korur (R3). İmzaya dördüncü bir parametre eklemek yerine alan tutulmasının
     sebebi phase-2: blink karesi `move_caret`'tan geçiyor ve şeklin oradan
     kaybolması **temsil edilemez** olmalı.
  4. `CursorBlock.rect` de aynı daraltılmış dikdörtgeni alır (R2). Shader'a
     bayrak **eklenmez** — `cell.metal`'in `mix`'i dikdörtgenin içini zaten
     tarıyor, dar bir dikdörtgen dar bir karışım demek.
- **`crates/bt-gpu/src/renderer.rs`** (ve `link.rs:1123` `dock_caret_at`) —
  caret'in ölçüsünü taşıyan çağrı yerleri. Şekil `bt-core`'dan geliyor;
  **kalınlık** `bt_atlas`'ın alt çizgi metriğinden (R2.2), chevron emsali —
  ikinci bir tasarım sabiti uydurulmaz.
- **`docs/AYARLAR.md`** — `[terminal] cursor`: değerler, varsayılan, tanınmayan
  değerin davranışı. Şablon bloğu `Settings::TEMPLATE` ile birebir kalmalı
  (`documented_template_is_the_template`).
- **`CLAUDE.md`** — "Bugünkü hâl"deki imleç cümlesi ("her koşulda dolu bir
  blok") ve "Ayarlar" maddesinin anahtar envanteri.
- **`docs/YOL-HARITASI.md`** — **beşinci kayma notu** (tarihli blockquote,
  dördüncünün kalıbında): 014 materyal yüzey → **015**, emoji/geniş/kutu →
  **016**, sekme/bölme → **017**. Kaydedilecek bedel: materyalin yazılı ön
  koşulu (kare süresi tabanı `/measure`) bir kez daha erteleniyor.

## Kabul

- `\e[5 q` gönderen bir uygulama (vim insert modu) **dikey çubuk** gösterir,
  `\e[3 q` alt çizgi, `\e[2 q` blok. `Se=\E[2 q` ile bloğa döner.
- Underline caret'i dock bandına denk geldiğinde **dock yuvasına** geçmeye
  devam eder ve zeminin altında kalmaz (R2.1'in tanığı; daraltma yanlış yerde
  yapılırsa bu sınama kızarır).
- Dar caret'in altındaki harf hâlâ ters çevrilir, ama yalnız şeridin altında.
- `[terminal] cursor = "beam"` kayıt anında uygulanır; tanınmayan değer anahtarı
  **değiştirmez** ve tanı bırakır.
- **`term_config_keeps_every_other_field` fixture'ı varsayılan olmayan bir
  şekil taşır** (R11.2). Varsayılanla doldurulursa son döngü onu reset
  listesine eklemeden geçer ve guard'ın vaadi ("iki alanın dışında hiçbir şey
  kurulmuyor") **sessizce yalan** olur.
- `make duman` jetonları bugünküyle aynı: kapının hiçbir katı caret
  dikdörtgeninin boyutuna bakmıyor ve `hareket > 0` imlecin **hedefine**
  bakıyor, şekil onu değiştirmiyor.

## Uygulama Notları

- **Şeklin kaynağı `Term::cursor_style()` değil `RenderableCursor.shape`
  oldu.** İkisi aynı değer — alacritty `renderable_content()`'i şekli zaten
  `cursor_style()`'dan çözüyor — ve o okuma döngüden önce **zaten vardı**
  (`cursor_shape`, `Hidden` kapısı için). İkinci bir çağrı aynı değeri ikinci
  kez okumak olurdu. **phase-2 için not:** `RenderableCursor` blink bitini
  taşımıyor, yani phase-2 `Term::cursor_style()`'ı gerçekten çağırmak zorunda.
- **Kalınlık `CellMetrics`'e yeni bir alan olarak girdi** (`rule_px`), kurucuya
  **açık parametre** olarak: sessiz bir varsayılan bu deponun yasakladığı sınıf
  ve 20 çağrı yeri mekanik olarak güncellendi. Değer `bt_atlas::Metrics`'in
  `underline_px.1`'i, yani chevron'un da aldığı metrik.
- **Alt çizgi caret'i hücrenin dibinde**, fontun alt çizgi *konumunda* değil.
  R2.2 yalnız **kalınlığı** metriğe bağlıyor; konum olarak fontun alt çizgisi
  taban çizgisinin hemen altı ve caret orada `g`'nin kuyruğunu keserdi. Hücre
  dibi uydurulmuş bir sayı değil, hücrenin kenarı.
- **Geometrinin tek sahibi `caret_rect`** oldu; boyanan dörtlü ile ters çevirme
  dikdörtgeni onu paylaşıyor. İlk taslakta ikisi ayrı hesaplanıyordu ve
  `caret_shapes_narrow_both_rectangles` tam o ayrışmayı tutuyor.

## Yayın Etkisi

- **ayar şeması** — `[terminal] cursor` eklendi; silinen anahtar yok, bilinmeyen
  anahtar korunuyor. `docs/AYARLAR.md` ve `Settings::TEMPLATE` birlikte.
  `cursor_blink` **bilerek yok** (phase-2).
- **`CLAUDE.md`** — imleç cümlesi ve anahtar envanteri.
- **belge** — `docs/YOL-HARITASI.md` beşinci kayma notu (014→015→016→017).
- shader: **yok** — `.metal` değişmiyor, `#[repr(C)]` düzeni değişmiyor
  (`Instance` zaten ölçü taşıyor, `CursorBlock`'un alanları aynı).
- terminfo / app bundle / shell entegrasyonu / yeni bağımlılık: **yok**.
  `make kur` gerekmiyor.
- ölçüm bekleyen iddia: **yok**.

## Checklist

- [x] `bt-core`: `Cursor.shape` + kendi enum'u; `Hidden` taşınmıyor,
      `HollowBlock` bloğa düşüyor
- [x] `bt-core`: `term_config` `default_cursor_style`'ı `TerminalOptions`'tan
      kuruyor
- [x] `bt-core`: `[terminal] cursor` ayrıştırma, iki kol, `TEMPLATE`
- [x] `bt-gpu`: `Caret` ölçüsü, daraltmanın yeri, `Frame`'in şekil alanı,
      `CursorBlock.rect`
- [x] `bt-gpu`: kalınlık `bt_atlas`'ın alt çizgi metriğinden
- [x] Test: üç şeklin dikdörtgeni (`frame`), **underline caret'in dock
      yuvasına geçmesi**, `move_caret`'ın şekli koruması
- [x] Test: `term_config_keeps_every_other_field` fixture'ı varsayılan
      **olmayan** şekil taşıyor ve reset listesi genişledi
- [x] Test: `[terminal] cursor` round-trip + tanınmayan değerin tanısı
- [x] `docs/AYARLAR.md`, `CLAUDE.md`, `docs/YOL-HARITASI.md` (kayma notu)
- [x] Doğrulama geçti (`make hepsi`)
- [ ] `make duman` (kullanıcıda — ajanın kabuğunda yanlış tanıyla kırmızı
      düşüyor)
- [x] Yayın etkisi yazıldı
