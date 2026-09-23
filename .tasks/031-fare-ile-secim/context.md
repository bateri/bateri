# Fareyle seçim: ızgara ve dock — Bağlam

## Mevcut Durum

**Izgara.** Seçim 006'dan beri tek biçimde: sol tuşla sürükleme
(`bt-shell` `view.rs` → `button_event` / `drag_event`,
`Session::set_selection` + `update_selection`). Seçim alacritty'nin
`Selection`'ı ve **yalnız `SelectionType::Simple`** kuruluyor; alacritty'nin
`Semantic` (kelime) ve `Lines` (satır, sarılmış satırı bütün alır) tipleri
grafta var ama hiç çağrılmıyor. `NSEvent::clickCount` okunmuyor — çift ve
üçlü tıklama tek tıklamayla aynı, yani **hiçbir şey seçmiyor**. Shift'in
fare kipindeki arbitrajı 020'de (`input::button_route`); fare kipi kapalıyken
Shift+tıklamanın bir anlamı yok.

Vurgu **ters video** ve hücre başına (`Session::frame` → `selected`,
`inverse = plain_inverse ^ selected`): köşeli, temadan bağımsız (hücrenin kendi
iki rengi takaslanıyor). "Seçim içeriği vurgular, içerik yaratmaz" kuralı
(CLAUDE.md) hücre başına uygulanıyor: varsayılan zeminli boşluk çizilmez sayılıyor,
yani `echo hello world` seçildiğinde **kelime aralarındaki boşluklar
vurgusuz** kalıyor — `selection_to_string` o boşlukları kopyalarken.

Pano: Edit ▸ Copy (`menu.rs`, `copy:` → `BateriView::copy_selection` →
`Session::selection_text`). Cut ve Select All menüde yok. `keyDown:`'ın Cmd
izin listesi kapalı ve üç tuş (CLAUDE.md, 018 Karar 3); menü öğeleri
`performKeyEquivalent:` ile ondan **önce** yakalanıyor (026'nın sekme
kısayollarının yolu).

**Dock.** Fareyle hiçbir etkileşim yok: `window_point_cell` dock bandını
ızgaranın kırpma kuralıyla ele alıyor, dock'un satırına tıklamak ne caret'i
taşıyor ne seçim başlatıyor. Dock ZLE'nin aynası (012, OSC 8133): satırın
gerçeği ZLE'de, terminal `BUFFER`/`CURSOR`/`KEYMAP`'i okuyor ve tuşlar PTY'ye
gidiyor. Aynanın **tazelik** (025, `DockState::answers`), **sahiplik**
(`ShellLog::suppressed_input`, `Cursor::caret_in_dock`) ve **ekleme keymap'i**
(`DockState::insert_keymap`, `INSERT_KEYMAPS`) kapıları hazır;
`Session::can_be_typed` üçünü birleştirip yapıştırmayı sarmadan akıtıyor —
bu setin düzenleme kapısının emsali. Dock'un sütun→karakter eşlemesi
`dock::render` ile `window_skip`'te (024).

**Borç.** Yol haritasının "Farenin jest durumu sınanamıyor" kalemi
(`docs/YOL-HARITASI.md`): jest defteri (`dragging`, `sent_buttons`, `Click`'in
kolları) `define_class!` gövdesinde ve sınanamıyor; kalem "çift/üçlü tıkla
seçim doğal ev" diyor. Doldurma bandının seçilemezliği ayrı bir kalem ve bu
setin konusu değil (aynı dosya → "Doldurma bandının satırları seçilemiyor").

## Motivasyon

**Kullanıcı isteği (2026-09-24):** "fare ile metin seçme kısımlarında
iyileştirme yapmamız lazım. çift tıklamada hiçbir aksiyon yok. özellikle dock
kısmında metin seçme yok. bunlar çok büyük eksiklikler. metin seçip
silebilmek lazım. ek olarak metin seçme rengi oluştur temalar için ve border
radius ile güzel gösterme."

Üç eksik, üçü de her macOS metin yüzeyinde bariz beklenti:

1. **Izgara:** çift tıklama kelime, üçlü tıklama satır seçer; çift/üçlü
   tıklayıp sürüklemek kelime/satır adımıyla büyür; Shift+tıklama seçimi
   uzatır (Terminal.app, iTerm2, Ghostty, alacritty'nin dördü de böyle).
2. **Dock:** giriş satırı bir metin alanı gibi görünüyor ama öyle davranmıyor.
   Beklenen bir `NSTextField`'ın davranışı: tıklamak caret'i taşır, sürükleme /
   çift / üçlü tıklama seçer, seçim varken Backspace siler, yazmak seçimin
   yerine yazar, Cmd-C/X/A. Referans üründe de karşılığı var:
   `docs/ARASTIRMA.md` → mimari tablosu, `mt-gpu` `renderer` modülleri
   arasında `dock_selection`.
3. **Görünüş:** vurgu temadan bağımsız ters video; tema dosyasında seçim
   rengi yok ve köşeler kare. İstenen: tema rolü, yuvarlak köşe, çok satırlı
   seçimde tek parça şekil.

**Ölçüm (2026-09-24, zsh 5.9, `zsh -f -i` bir Python pty'sinde):** dock
düzenlemesinin ZLE'ye taşınma yolu prototiplendi. Özel bir CSI dizisine
(`\e[8133~`) bağlanmış bir widget, yükünü `read -k` ile BEL'e kadar okuyup
`CURSOR`'ı kurabiliyor ve `BUFFER`'da korumalı bir aralık değişimi
yapabiliyor; `emacs`'te, `bindkey -v` sonrası **`vicmd`'de** ve çok baytlı
metinde (`çğü`, `🥰` — indeksler karakter) çalıştı. Widget'ın değişikliği
ZLE'nin **tek geri alma birimi**: ardından yazılan harf ve bir `^_` yalnız
harfi, ikinci `^_` değişikliği geri aldı. Prototip kullanıcının hiçbir
dosyasına dokunmadı (`ZDOTDIR` geçici dizinde).
