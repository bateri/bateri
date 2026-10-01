# Tıklanabilir bağlantılar — Bağlam

## Mevcut Durum

Ekrandaki hiçbir metin tıklanamıyor. Satırın kaynağı `docs/YOL-HARITASI.md` →
"tıklanabilir bağlantılar" (034'ün envanterinden).

- **OSC 8 yalnız blok çıpası olarak okunuyor.** alacritty hücrenin
  `hyperlink()`'ini yan tablodan (`CellExtra`) veriyor; `bt-core` onu yalnız
  `bateri://block/N` önekiyle süzüyor (`session.rs` → `block_id`, `frame()`'in
  çıpa taraması, `block_row_continues`). `ls --hyperlink`'in `file://`'ı, bir
  `man` sayfasının `https://`'ı ya da Claude Code'un bağlantıları hücrede
  duruyor ama hiçbir yol onları okumuyor. `bateri://` şemasının dış yüzü
  (`tab/<id>`) 038'in; bu setin onunla ilişkisi 038 `discussion.md` → Karar 7:
  `bateri://` bağlantısı `NSWorkspace`'e verilmeden yutulmalı.
- **Fare rotası üç kollu.** `input::button_route` (`bt-core`) kip + Shift'ten
  `Report`/`Select` veriyor, `Session::mouse_button` `Click::{Sent, Select,
  Ignored}` döndürüyor, jest defteri (`bt-shell-common::gesture`) rotayı
  basışta kilitliyor ve bırakmayı ona göre yolluyor (`Release::Report` fare
  kipinde bırakma raporu gönderir). Cmd bir xterm biti değil, `MouseModifiers`
  onu taşımıyor. Kural ve gerekçesi `CLAUDE.md` → "Fare isteyen uygulama
  fareyi alıyor".
- **Görünüm için hazır parçalar.** Sınır hücresi `underline: UnderlineStyle`
  (`Single`/`Double`/`Curl`/`Dotted`/`Dashed`) ve `underline_color` taşıyor,
  renk yoksa `fg`; beş alt çizgi sprite'ı atlasta (`bt-atlas`). El imleci
  bugün yalnız yükleme düğmelerinde, AppKit'in cursor rect'iyle
  (`view.rs` → `upload_cursor_rects`; `set()`'in neden tutmadığı orada yazılı).
  `BateriView`'da `flagsChanged:` yok — ⌘'ye basıp bırakmak bugün hiçbir şeyi
  tetiklemiyor.
- **Metin yürüyüşleri.** Arama `alacritty`'nin `RegexSearch`/`RegexIter`'ını
  kullanıyor, sarılmış satırın mantıksal başına `search::WRAP_REACH` kadar
  uzanıyor; `regex-syntax`'ı doğrudan bağımlılık yapmamak 033'ün kaydı
  (`search.rs` → `META`'nın doc'u, 033 Karar 11). Dock'un metni `Term`'de değil
  aynada (`dock::selectable`: `PREBUFFER ++ BUFFER`), yani `RegexSearch` onu
  göremiyor.
- **Açma ve dizin.** `NSWorkspace::openURL` yolu `app.rs` → `open_in_editor`'da
  (ayar dosyası için). Çalışma dizini `Session::working_directory` (OSC 7,
  **bugünkü** dizin), uzak oturum `DockContext::remote` (036).
- **Üç yüzey.** Izgara, doldurma bandı ve dock (`proje.md` → Set kapısı
  ekleri). Bandın satırları seçilemez (`view.rs` → `point_to_cell`, gerekçesi
  `CLAUDE.md` → "Seçim içeriği vurgular"), ama hepsi defterdeki belli
  satırlar (`Session::fill_shown`).

## Motivasyon

Terminalde URL'yi ya da derleyicinin bastığı `src/main.rs:12:5`'i açmak
bugün seç-kopyala-yapıştır demek. iTerm2, Ghostty, Terminal.app, kitty,
WezTerm ve VS Code terminalinin ortak davranışı ⌘ basılıyken bağlantının
alt çizgiyle vurgulanması, imlecin el olması ve ⌘-tıkın açmasıdır; OSC 8
(`ls --hyperlink`, `gcc`'nin tanıları, `systemd`, Claude Code) metnin
arkasına ayrı bir hedef koyuyor ve onu açacak tek yol terminal. Referansta
ayrı bir modül var (`docs/ARASTIRMA.md` → `mt-gpu`'nun `path_link`'i); kayıt
yalnız modülün adını taşıyor, davranışı değil.

Güvenlik ekseni de var: OSC 8'de görünen metin ile hedef ayrı ve hedefi
yazan, ekrana bayt basan her program (`cat` edilen bir dosya, uzak makine).
