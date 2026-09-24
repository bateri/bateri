# Çok satırlı dock — Bağlam

## Mevcut Durum

Dock **tek** giriş satırı + bir bağlam satırı (`bt_gpu::DOCK_ROWS = 2`,
`frame.rs`). Sözleşme `CLAUDE.md`'de (dock, bastırma, devir, `Multiline`
paragrafları); burada yalnız bu işin dokunduğu dikişler:

- **Satır sonu taşıyan görüntü dock'a hiç girmiyor.** `decode_line`
  (`shell.rs`) `PREDISPLAY`/`BUFFER`/`POSTDISPLAY`'de `\n` görünce
  `DockStatus::Multiline` kuruyor; `suppressed_input` yalnız `Live`'da
  döndüğü için bastırma kapanıyor, `caret_home_raw` caret'i ızgaraya veriyor
  (tutmanın dışında), `render_with` yalnız işareti çiziyor. Yani satır da
  caret de ızgarada ve 031'in düzenlemesi (seçim, sil/yerine yaz, ⌘X,
  tıkla-caret) ile 030'un yazım efektleri orada **hiç çalışmıyor**.
- **Dock'un bütün aritmetiği tek boyutlu.** `Dock` (`dock.rs`) tek caret
  sütunu, tek `(ilk, son)` seçim çifti taşıyor; `columns()` yürüyüşü, `hit`,
  `selection_range` (üçlü tıklama = bütün `BUFFER`), `window_skip` (yatay
  pencereleme) ve `diff` (030) satır görmüyor; `cell()` ve `render_context`
  `row: 0` / `CONTEXT_ROW = 1` yazıyor. `column_width('\n')` 1 döndürüyor —
  satır sonu bugün sıradan bir sütun.
- **Bastırmanın sınırları sütun bölmesi.** `suppress_to` ve `suppress_floor`
  (`session.rs`, `frame()`) satır sayısını `sütun / cols` ile türetiyor ve
  yorumu satır sonunu sayamadığını açıkça yazıyor; `Multiline` bastırmayı
  kapattığı için o kol bugün erişilemez.
- **Bandın boyu ile PTY aynı düğme.** `dock_px(dock_rows, cell)` hem
  `split_into_grid`'e (`app.rs`, PTY satırı) hem `encode_dock`'un viewport'una
  gidiyor; `dock_rows` yalnız alternatif ekran geçişinde değişiyor
  (`alt_screen_did_change`, geçiş başına bir `TIOCSWINSZ`).
  `dock_row_divider_y`/`dock_ground` tam iki satır için yazılı,
  `DOCK_CONTEXT_ROW = 1` küçük boy sınıfını seçiyor, `dock_caret_at` satır 0'a
  sabit, `window_point_dock` (`view.rs`) `rows: 1` ile eşliyor.
- **Öteleme işaretsiz.** `origin_target(cursor) -> u16`
  (`rows - content_rows`, `link.rs`) ve `Motion::sync(origin_rows: u16, …)`;
  `sync_origin` yalnız **düşen** hedefi süzüyor (011'in yön kuralı, `filled`
  istisnasıyla). Negatif viewport orijini zaten destekleniyor ve sınanıyor
  (doldurma bandı, `a_negative_viewport_origin_draws_and_clips_from_the_top`).
- **Yapıştırma.** `can_be_typed` satır sonlu yükü her zaman bracketed yola
  gönderiyor (satır sonu çıplak giderse komut koşar). `bracketed-paste-magic`
  aynayı bir tuş boyunca bayat bırakıyor ve kabuk tarafında üç çare ölçülüp
  kapandı (`session.rs`, `suppress_to`'nun tazelik yorumu).
- **Kabuk betiği** `PREBUFFER`'ı göndermiyor; `PS1`'i dayatıyor ama `PS2`'ye
  dokunmuyor (`__bateri_prompt_guard`).

## Motivasyon

**Kullanıcı isteği (2026-09-24):** çok satırlı giriş — yapıştırma, `for`
döngüsü, heredoc, `\`-devam — dock'ta kalmalı; bugün satır ızgaraya gidiyor
ve 030/031'in bütün dock davranışı orada kayboluyor. Kullanıcıyla
kararlaştırılan yön (ürün kararı): **dock çok satırı kendisi gösterir,
yukarı doğru büyür**; `Multiline` durumu ve yapıştırmada satırın ızgaraya
fırlaması biter.

**Borcun eski gerekçesi** (`docs/YOL-HARITASI.md`'den buraya taşındı,
2026-09-21'de yazılmıştı): bant ızgaranın satırlarından düşüldüğü için her
yeni satır bir PTY resize'ı — kullanıcı yazarken nefes alan bir ekran ve
sarmalı geçmişin yeniden akışı; opak bandı ızgaranın üstüne büyütmek de
tersi, kullanıcının o an görmek istediği çıktıyı örter. 012 phase-4'ün "çok
satırlı `BUFFER`" bilinen sınırı (bastırma aritmetiği satır sonlarını
saymıyor) `Multiline`'la konusuz kalmıştı; bu set onu geri getiriyor ve
aritmetiği satır farkında yaparak kapatıyor. Set iki gerekçeyi de çizim
tarafında büyüyerek karşılıyor (`discussion.md` → Karar 1).

## Kanıt — zsh'in çok satırlı hâlleri (ölçüldü, 2026-09-24)

`zsh -f -i` bir `zpty`'de, `zle-line-init` + `zle-line-pre-redraw`
kancasıyla `PREBUFFER`/`BUFFER`/`CURSOR` kaydedildi, ham çıktı okundu:

| giriş | `PREBUFFER` | `BUFFER` | ızgaraya basılan |
|---|---|---|---|
| `for i in 1 2; do` ⏎ `echo $i` | `for i in 1 2; do\n` | `echo $i` (tek satır) | ikinci satır kullanıcının `PS2`'si (`for> `) ile |
| `cat <<E` ⏎ | `cat <<E\n` | boş | `heredoc> ` |
| `echo a \` ⏎ | `echo a \\\n` | boş | `> ` |
| yapıştırma `echo x\necho y\n` | boş | `echo x\necho y\n`, `CURSOR=14` | ikinci satır **0. sütundan**, `PS2`'siz (`\r\n` + metin) |
| çok satırlı `BUFFER`'da ↑ | — | — | ZLE `BUFFER` içinde satır yukarı (`CSI A`) |

Sonuç: kullanıcının dört örneğinin **üçü** (`for`, heredoc, `\`-devam)
`BUFFER`'da satır sonu **değil**, `PREBUFFER`'da — kabul edilmiş, ZLE'nin
artık düzenlemediği satırlar; bugünkü `Multiline` onları hiç görmüyor, dock
yalnız son satırı gösteriyor, önceki satırlar ızgarada kalıyor. Satır sonu
taşıyan `BUFFER` yalnız yapıştırma, geçmişten çok satırlı komut ve
`Esc-Enter`. Izgarada `BUFFER`'ın devam satırları **0. sütundan** başlıyor;
`PREBUFFER`'ın satırları kullanıcının `PS2`'sinin genişliğinden.
