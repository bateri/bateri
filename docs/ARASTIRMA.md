# Araştırma — Metalterm envanteri (9 Eylül 2026)

Kurulu `Metalterm.app` 0.1.10 (build 1149) binary'sinin, metalterm.dev'in ve
`pioner92/metalterm-site` sürüm notlarının incelemesi. **Tarihli bir kayıttır**;
Metalterm güncellendikçe eskir, güncellenmez. `bateri`'nin tasarım kararları
`CLAUDE.md` ve `.tasks/` setlerindedir, burada değil.

## Kimlik

- Geliştirici: pioner92 (GitHub, x.com/pioner_dev). Uygulama kaynağı **özel**;
  site deposu yalnız `index.html`, `appcast.xml` (Sparkle), issue ve sürümler.
- macOS 14+, universal binary, 12 MB, notarize. Ücretsiz, hesapsız, telemetrisiz.
- Sürüm ritmi: 0.1.3 (20 Ağu) → 0.1.10 (4 Eyl 2026), yaklaşık haftada bir.

## Nasıl yapılmış

- **Rust.** Swift/Zig izi yok. `objc2` ailesi: `objc2-app-kit`, `objc2-metal`,
  `objc2-foundation`, `objc2-quartz-core`, `objc2-user-notifications`,
  `block2`, `dispatch2`; font için `core-text 22`, `core-graphics`.
  Yardımcılar: `unicode-width`, `hashbrown`.
- **Workspace** (`/Users/alex/Documents/metal-terminal/crates/`):

| crate | dosyalar (binary'deki yollar) |
|---|---|
| `mt-core` | `pty`, `screen`, `screen/mutate`, `screen/reflow`, `reflow`, `term/model`, `term/state`, `term/osc`, `semantic`, `session`, `settings`, `shell_context`, `gitstatus`, `editline`, `inline`, `latency`, `width` + `unicode_tables`, `material/preview` |
| `mt-atlas` | `atlas`, `packer`, `raster`, `fontset`, `boxdraw` |
| `mt-gpu` | `renderer` (+ `chrome`, `frame`, `grid`, `interaction`, `model`, `session`, `setup`, `dock_selection`), `motion`, `grid`, `tiles`, `text`, `hud`, `search`, `path_link`, `context`, `detect`, `overlay/find`, `overlay/palette`, `elements/dock`, `elements/statusbar`, `elements/kit` |
| `mt-shell` | `app`, `keys`, `menu`, `services`, `diag` |

- **Metal 3.1**, tek `default.metallib` (157 KB), 9 pipeline: `substrate`
  (materyal arka plan), `cell_bg`, `cell` (glyph), `cell_rule` (alt çizgi),
  `block_gutter` (komut bloğu şeridi), `decay` (silme efekti), `quad`,
  `shape`, `ui_text`. Site: "iki geçiş — metin ve ortam ışığı", 0.22 ms GPU /
  0.13 ms CPU kare, 118×34 hücre, M3 Max, 119.7 Hz, 0 düşen kare.
- Boşta **hiç** frame göndermiyor; `CAMetalDisplayLink`; ProMotion takip;
  `energy_saving`, `frame_cap` ayarları; pasif sekmelerin drawable'ları bırakılıyor.
- Font: JetBrains Mono (varsayılan), SF Mono, Menlo, Apple Color Emoji.
- Güncelleme: Sparkle. Hata izleme: yok (telemetrisiz).

## Terminal davranışı

- OSC 0, 7, 8, 9, 12, 52, 104, 133, 777; kitty klavye protokolü; SGR-pixel fare;
  bracketed paste; DECSET; G0–G3 karakter seti; reverse-wrap; alternate screen.
- `TERM=xterm-metalterm` denendi, ncurses+SSH kırdı (#23) → legacy alias korundu.
- Hücre 20 bayt; emoji, grapheme kümeleri, alt çizgi rengi yan tablolarda.
- 10 000 satır scrollback / sekme; her sekme ve bölme ayrı shell süreci.

## Shell entegrasyonu

- zsh: `ZDOTDIR` sarmalayıcısı (`METALTERM_ZDOTDIR`, kullanıcının `.zshrc`'si
  sonra yüklenir); bash: `metalterm-integration.bash`; fish: `vendor_conf.d`.
  Kullanıcı rc dosyasına dokunulmuyor.
- Prompt'u terminal çiziyor (OSC 133 `B`); shell prompt çizmiyor.
- **Input Dock**: prompt pencere altında sabit ayrı satır editörü; zsh ZLE
  kancalarıyla; `Claude`, `Codex`, REPL gibi yazmayı devralan uygulamalar
  davranışla tespit edilip dock alanı geri alınıyor.
- Tema değişince LS_COLORS ve prompt renkleri aynı 8 rolden türetilip canlı
  güncelleniyor (`METALTERM_THEME_FILE`, `_metalterm_reload_theme`).
- Komut tamamlama sözlüğü gömülü: git, brew, docker, npm, gh, cargo, kubectl,
  uv/pip, terraform alt komutları.
- Ölçüm kancaları (env): `MT_RUN_SECONDS`, `MT_SCROLL_TEST`, `MT_SELECT_TEST`,
  `MT_RESIZE_TEST`, `MT_SPLIT_TEST`, `MT_INPUT_LATENCY_*`; gecikme zinciri
  `NSEvent → handler → PTY yazıldı → echo okundu → parse dirty → frame
  scheduled → commit → GPU complete → presented`.

## Görünüm

- **Tema = 8 rol:** background, foreground, dim, accent + 4 durum.
- **21 materyal yüzey** (Material sekmesi, `grain` ve `sheen` canlı):
  Anodised Obsidian, Dark Ceramic, Carbon Surface, Smoked Glass, Obsidian Paper,
  Soft OLED, Volcanic Stone, Graphite Fabric, Liquid Graphite, Soft Touch
  Polymer, Meteorite Steel, Urushi Nightfall, Charred Oak, Nocturne Velvet,
  Deep Field, Slate & Vellum, Forged Carbon, Bone China, Cold-Pressed, Sunlit
  Alabaster, Brushed Aluminium.
- **15 düz palet** (Classic sekmesi): Metalterm, Graphite, Ember, Abyss,
  Orchid, Phosphor, Nocturne, Slate, Solstice, Dune, Grape, Repo, Palenight
  Dark, Palenight (VS Code birebir), Cappuccino.
- Kullanıcı temaları `~/.config/metalterm/themes/`; ayar `~/.config/metalterm/settings.toml`.

## Hareket (`[motion]`)

Enum değerleri binary'den: `spring, pop, extrude, ink, squeeze, smear, squash,
arc, bleed, unravel, recede, sublime`; silme için `smooth | shatter`; buffer
lift için `off | smooth | spring`. Site yedi imleç stili sayıyor: Snap, Ease,
Spring, Smear, Squash, Phosphor, Arc.

| anahtar | ne |
|---|---|
| `cursor_motion` | alt hücre interpolasyonlu imleç kayması |
| `delete_mode` | Backspace ile silinen glyph'in akıbeti (`shatter`) |
| `keypress` | yazarken glyph'in tek tek belirmesi |
| `feed_lift` | çıktı geldikçe tamponun yukarı kayması |
| `status_bar_animation` | odometre sayaç animasyonu |
| `scroll.smooth` | Mos gibi dış kaydırıcılar için kapatılabilir |
| `reduce_motion`, `intensity`, `duration` | genel çarpanlar; sistem ayarını izler |

Ayar anahtarlarının tamamı (binary'den): `appearance.theme`,
`appearance.material`, `grain`, `sheen`, `command_duration_threshold`,
`command_gutter`, `block_depth`, `font_size`, `line_height`, `family`,
`ui_scale`, `cursor`, `cursor_blink`, `shell.command`, `input_dock`,
`working_directory`, `startup_command`, `scrollback`, `window.close_on_exit`,
`confirm_close`, `opacity`, `background_blur`, `transparent_bars`,
`restore_windows`, `bell`, `clipboard.osc52`, `keyboard.left_option`,
`right_option`, `renderer.energy_saving`, `frame_cap`, `substitute_spinner`.

## Ürün özellikleri (site + sürüm notları)

Komut blokları (süre, kırmızı gutter), komut paleti (⌘⇧P; ayar, tema, sekme,
son komutlar), scrollback'te regex arama (GPU vurgulama), sekme + iki eksenli
bölme (⌘D / ⇧⌘D, klavyeden gezinme/boyutlama/maksimize), ⌘-tıkla dosya açma,
Finder'dan sürükleme, OSC 9/777 sistem bildirimi, pencere geri yükleme,
arka plan bulanıklığı, kapatma onayı (never/running/always), yerel sağ tık
menüsü, Python venv adı durum çubuğunda, Option tuşu modları (auto/macOS/Esc+).

## Açık sorunlar (issue listesinden ders)

RTL metin (#8), fish emoji glitch (#21), atuin geçmişi (#22), bağlantı sarma
(#33), Claude Code TUI'de motion çalışmıyor (#26), scroll smoothing dış
araçlarla çakışıyor (#30 → ayar eklendi), tab bar tema (#24), TUI'de flicker
(#7, #9 — düzeltildi). Terminal uyumluluğu uzun kuyruktur; `alacritty_terminal`
bu kuyruğun büyük kısmını hazır getirir.

## Rust ekosistemi (9 Eylül 2026 itibarıyla)

`alacritty_terminal 0.26`, `vte 0.15`, `objc2-metal 0.3.2`, `objc2-app-kit 0.3.2`,
`portable-pty 0.9`, `core-text 22`, `swash 0.2.10`, `cosmic-text 0.19`.
