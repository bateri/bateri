# Fare raporlama — Bağlam

## Mevcut Durum

Terminalin faresi bugün **yalnız bizim**: tıklama seçim başlatır, sürükleme
seçimi büyütür, tekerlek geçmişte kaydırır. Uygulamaya giden tek fare olayı
**tekerlek**.

Yol `bt-shell/src/view.rs`'te başlıyor:

- `mouseDown:` (satır 419) **koşulsuz** seçim başlatıyor — yalnız sol tuşu
  süzüyor, uygulamanın fare isteyip istemediğini hiç sormuyor.
- `mouseDragged:` seçimin ucunu taşıyor, `mouseUp:` sürüklemeyi bitiriyor.
- `scrollWheel:` **soruyor**: `Session::scroll_wheel` kipi `Term` kilidi
  altında okuyup dört yoldan birini seçiyor (`input::wheel_route`).

`bt-core/src/input.rs` raporun yarısını zaten taşıyor ve bu yarı **genel**:

- `wheel_report(encoding, button, col, row)` adı tekerlek ama gövdesi
  X10/UTF-8/SGR fare raporunun ta kendisi — `button` bir `u8` ve tekerlek
  yalnız 64/65'i kullanıyor.
- `MouseEncoding` üç kodlamayı ayırıyor (1006 SGR, 1005 UTF-8, X10), seçim
  `wheel_route`'ta ve sırası alacritty'nin `mouse_report`'uyla aynı.
- Koordinat dönüşümü hazır: `viewport_point` ile görünen pencereden
  uygulamanın satırına iniliyor ve satır uygulamanın ekranında değilse rapor
  **gitmiyor**.

Kip takibi de bedava: ayrıştırma `alacritty_terminal`'da ve `TermMode` üç fare
kipini (`MOUSE_REPORT_CLICK` 1000, `MOUSE_DRAG` 1002, `MOUSE_MOTION` 1003),
iki kodlamayı (`SGR_MOUSE`, `UTF8_MOUSE`) ve **odak raporunu**
(`FOCUS_IN_OUT`, 1004) zaten taşıyor. Üçü birbirini **dışlıyor**: `1003 h`
önce `MOUSE_MODE`'u siliyor sonra `MOUSE_MOTION`'ı kuruyor.

Eksik olan üç şey:

1. **Düğme raporu** — bas/bırak (0/1/2), SGR'ın bırakma biçimi (`m`; tekerlekte
   bırakma olmadığı için hiç yazılmamış) ve değiştirici bitleri.
2. **Hareket raporu** — 1002/1003 için `mouseMoved:`; view'da bugün hiç
   `NSTrackingArea` yok (`grep`: sıfır).
3. **Kipi soran bir kapı** — `mouseDown:`'ın arbitrajı. Rapor yazılsa bile bu
   kapı olmadan tıklama uygulamaya gitmez.

Odak ayrı bir eksik: `bt-shell` odağı biliyor (`AppDelegate::apply_focus` →
`DisplayLink::set_focused`) ama uygulamaya söylemiyor. `CLAUDE.md` bunu bir
mimari karar olarak yazıyor: "Odak `bt-core`'a **hiç girmiyor**".

## Motivasyon

Kullanıcı bildirdi (2026-09-21): "claude app input kısmında fare ile bastığım
yere imleç geliyor normalde ama bateri de olmuyor."

Belirti fare raporunun yokluğu. Claude Code giriş kutusunda tıklanan yere
imleci taşımak için terminalden tıklama raporu bekliyor; biz göndermediğimiz
için tıklama yalnız bizim seçimimizi başlatıyor ve uygulamanın imleci
kıpırdamıyor.

Bu bir **parite açığı**: Terminal.app, iTerm2, ghostty, kitty ve alacritty
fare raporu yapıyor. Etkisi Claude Code'la sınırlı değil — fare isteyen her
TUI (vim, htop, lazygit, tmux, less, ranger) bizde faresiz çalışıyor.

## Kanıt

Claude Code'un gerçekten ne istediği **ölçüldü** (2026-09-21, bu makine): CLI
bir pty'ye (`pty.fork`, `TERM=xterm-256color`) koşturulup ilk 6 saniyede
yazdığı baytlar yakalandı ve DECSET/DECRST dizileri ayıklandı.

| Kip | Ne | Bizde |
|---|---|---|
| `?1000` | tıklama raporu (bas/bırak) | ✗ |
| `?1002` | basılıyken hareket | ✗ |
| `?1003` | her hareket | ✗ |
| `?1006` | SGR koordinat | ✓ (yalnız tekerlekte) |
| `?1004` | odak raporu | ✗ |
| `?2031` | tema değişimi bildirimi | ✗ |
| `?2004` | bracketed paste | ✓ |
| `?1049` | alternate screen | ✓ |
| `?25` | imleç görünürlüğü | ✓ |

Üçü birbirini dışladığı için **etkin kip 1003**: Claude Code her fare
hareketini istiyor, kodlaması SGR.

Yan bulgu: `?2031` tema değişimi bildirimi (contour kökenli, kitty/ghostty/
WezTerm'de var). Sistemin açık/koyu geçişini zaten canlı izliyoruz
(`Session::set_theme`), yani sinyal elimizde — söyleyecek kanal yok. **Bu
setin kapsamı dışında**, konusu fare değil tema.

## Mevcut Mimari

```
AppKit olayı                bt-shell/view.rs              bt-core
───────────────────────────────────────────────────────────────────────────
mouseDown:      ────────→   session_cell()  ──────────→   set_selection()
                            (koşulsuz seçim)               ▲ kip sorulmuyor
mouseDragged:   ────────→   event_cell()    ──────────→   update_selection()
mouseUp:        ────────→   dragging = false
(mouseMoved:)   ── YOK: NSTrackingArea kurulmuyor

scrollWheel:    ────────→   window_point_cell(fill=0) ─→  scroll_wheel()
                                                            │  Term kilidi
                                                            ├─ wheel_route(mode, shift)
                                                            │    ├─ Report(enc) → wheel_report()
                                                            │    ├─ Arrows     → input::arrow()
                                                            │    ├─ Scroll     → scroll_locked()
                                                            │    └─ Ignore
                                                            └─ Wheel{Sent|Scrolled|Ignored}

windowDidBecomeKey: ────→   AppDelegate::apply_focus() ─→  DisplayLink::set_focused()
windowDidResignKey:                                         ▲ bt-core'a hiç girmiyor
```

Düğme yolunun `bt-core`'a hiç uğramaması ile tekerlek yolunun `Term` kilidinden
geçmesi arasındaki bu asimetri setin işidir: düğme de aynı kapıdan geçmeli.

## Boşta sıfır kare ile ilişkisi

1003 açıkken pencerenin üstündeki **her** fare hareketi PTY'ye bayt yazar ve
uygulama çizerse kare doğar. Bu `CLAUDE.md`'nin "boşta sıfır kare" kuralını
**delmiyor** — kare talebi bizden değil uygulamanın hasarından geliyor, yani
kirli satır gerçekten var. Ama rapor **hücre değişiminde** kısılmak zorunda:
piksel başına rapor gönderen bir terminal boşta duran bir uygulamayı sürekli
çizdirir. xterm ve alacritty ikisi de son raporlanan hücreyi tutuyor.
