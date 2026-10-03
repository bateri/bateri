# Ayarlar

bateri'nin kullanıcı ayarları tek bir TOML dosyasında, renk temaları ayrı
dosyalarda durur. Bu belge anahtarların, tema biçiminin, varsayılanların ve
dosya bozukken ne olacağının **tek sahibidir**; kod tarafındaki karşılığı
`crates/bt-core/src/settings.rs` ve `theme.rs` (ayrıştırma),
`crates/bt-shell-common/src/settings.rs` (okuma ve tema adının çözümü),
`watch.rs` (dosyaların izlenmesi), `zoom.rs` (geçici punto),
`crates/bt-shell-macos/src/menu.rs` (View menüsü: tema seçimi ve geçici punto),
`clipboard.rs` ve `app.rs`'in `ShellWake`'i
(OSC 52'nin yuvası, ana kuyruğa geçişi ve panoya yazması),
`crates/bt-atlas/src/font.rs` (font ailesinin bulunması),
`crates/bt-gpu/src/motion.rs` ve `link.rs` (imleç hareketinin ve Hareketi
Azalt'ın uygulanması; üç değerli ayarın tek `bool`'a indiği yer
`crates/bt-shell-macos/src/app.rs`'in `resolve_reduce_motion`'ı),
`crates/bt-shell-common/src/child.rs` (hangi kabuk koşuyor, sarmalayıcı
betiği nerede), `crates/bt-shell-macos/src/app.rs`'in `shell_integration_env`'i
(shell entegrasyonu kurulacak mı ve hangi ortamla),
`crates/bt-shell-common/src/jobs.rs` ile `crates/bt-shell-macos/src/window.rs`
(kapatma onayı: ön planda ne koşuyor, ne zaman sorulur);
betiğin kendisi `assets/shell/zsh/`.

## Dosyanın yeri

```
~/.config/bateri/settings.toml
```

Dosya yoksa her şey varsayılanıyla çalışır ve hiçbir uyarı çıkmaz.
**bateri ▸ Settings…** (Cmd ,) ayar penceresini açar; penceredeki **Open
settings.toml** düğmesi dosyayı editörde açar, yoksa önce yaratır (bkz.
[Settings…](#settings)). Dizini ve dosyayı elle oluşturmak da yeterli.
Dosya başka bir yere sembolik bağ olabilir (dotfile deposu); bağın hedefi
okunur.

Uygulama bu dosyaya dört yerden yazar: **Open settings.toml** dosya yokken
şablonu yaratır, **View ▸ Theme ▸** ile tema seçince `[appearance] theme`
satırını yazar (bkz. [View ▸ Theme ▸](#view--theme-)), **Shell ▸ Mark “host”
as ▸** ve **Shell ▸ Shell Integration on “host”** `[remote] hosts`'a o
host'un girdisini yazar (bkz. [`[remote]`](#remote))
ve **ayar penceresi** değiştirdiğiniz ayarın satırını yazar. Var olan dosyanın başka hiçbir
satırına — yorumlara, sıraya, tanımadığı anahtarlara — dokunulmaz.

Değişiklik **kaydettiğiniz anda** geçerli olur — ayar dosyasında da,
kullanılan temanın dosyasında da; kabuk ve içindeki program yaşamaya devam
eder. Tek istisna [`[shell] integration`](#shell): kabuk çoktan doğduğu için
o anahtar sonraki oturumda geçerlidir. Editörün kaydı nasıl yaptığı fark etmez (yerinde yazma, boşaltma,
geçici dosya ve üstüne taşıma, sembolik bağın hedefine yazma). Sistemin açık/koyu
görünümü de anında izlenir: tema `"system"` iken (varsayılan) Sistem
Ayarları'nda görünüm değişince pencere de değişir.

İzlenen yer `~/.config/bateri/` dizinidir. Uygulama açıkken bu dizin **hiç
yoksa** kabuktan oluşturmak izlemeyi başlatmaz: değişiklikler uygulamayı
yeniden açınca ya da pencereden bir ayar değiştirilince (ya da Open
settings.toml'a basılınca) görülür, sonrası kayıt anında izlenir. Pencerenin
yarattığı dizin hemen izlenir.

### Settings…

Kısayol ABD düzenli klavyede Cmd `,`. macOS menü kısayolunu klavye düzenine
göre yerleştirir; Türkçe Q klavyede aynı tuş **Cmd `ö`**, menüde de öyle
görünür. View ▸ Bigger da bu düzende `⌘:` görünür.

bateri ▸ Settings… (Cmd ,) ayar penceresini açar: solda beş kategori
(General, Appearance, Cursor, Motion, Remote Files), sağda ayarlar. Remote
Files en üstte `[remote] integration`'ı ("Set up shell integration on
servers"; bkz. [Uzak kabuk entegrasyonu](#uzak-kabuk-entegrasyonu)), sonra
`[remote]`'un sekiz önizleme/indirme anahtarını ve yük göstergesinin iki
anahtarını (`stats`, `stats_interval`; on bir anahtar) gösterir; klasör
satırlarında Change… klasör seçicisini açar (seçilen ev dizininin altındaysa
`~/…` diye yazılır), önizleme klasöründe Show in Finder onu açar ve "In use"
satırı klasördeki kopyaların toplamını gösterir — Clear Now önizlemeleri
hemen siler (değiştirdiğiniz kopya silinmez, indirme klasörüne taşınır). Pencere yalnız
`settings.toml`'a yazar; ekrana uygulayan, dosyayı kaydettiğinizde de koşan
yol, yani pencereden yapılan değişiklik de anında geçerlidir.

- **Ne zaman yazar:** açılır menü ve anahtar seçildiği anda, kaydırıcı
  bırakıldığında, sayı alanı Enter'da ya da alandan çıkınca (stepper her
  tıkta). Kabul edilmeyen girdi (harf, aralık dışı) yazılmaz, alan dosyadaki
  değere döner. `scrollback`'i küçültmek geçmişi o anda kırpar — dosyada
  olduğu gibi (bkz. [`[terminal]`](#terminal)).
- **Yalnız değiştirdiğiniz satırı yazar**, yorumsuz; anahtar dosyada yoksa
  bölümüne eklenir. Dosya yoksa önce aşağıdaki şablonla yaratılır. Pencereyi
  açmak dosya yaratmaz.
- **Dosya dışarıdan değişince pencere de değişir** — editörde kaydettiğiniz
  değer, `themes/`'e koyduğunuz yeni tema.
- **Kabul edilmeyen değer** kendi satırının altında turuncu yazılır; kontrol o
  an geçerli olan değeri gösterir ve yeni bir değer seçmek satırı düzeltir.
  Bir satıra ait olmayan tanı (örneğin emekli `shell.prompt`) pencerenin
  üstündeki şeritte çıkar.
- **Ayrıştırılamayan ya da okunamayan dosyada pencere kilitlenir:** bütün
  kontroller kapanır, şerit sebebi başlık çubuğundaki metnin aynısıyla söyler
  ve Open settings.toml varsayılan düğme olur (Enter). Pencere bozuk dosyaya
  yazmaz — içindeki yarım iş sizin; düzeltip kaydettiğinizde kilit kalkar.
- **Yazılamayan dosya** (izin) şeritte ve başlık çubuğunda söylenir, kontrol
  dosyadaki değere döner.
- Pencerede görünmeyenler: geçici punto (Cmd +/−; Size dosyanın değeridir) ve
  henüz olmayan ayarlar.

**Open settings.toml** dosyayı açar: önce `.toml` dosyalarını açan
uygulamayla, o yoksa varsayılan metin editörüyle (çoğu makinede TextEdit).
Dosya yoksa dizini ve dosyayı aşağıdaki şablonla yaratır. Şablon hiçbir şeyi
değiştirmez: varsayılanı olan her anahtar varsayılan değeriyle yazılıdır,
değeri yerinde değiştirip kaydetmek yeter.

- Var olan dosyaya **dokunmaz** — bozuk olsa da, başka yere sembolik bağ olsa
  da, bağın hedefi olmasa da.
- Dosya yaratılamazsa (izin yok, `~/.config/bateri` bir dosya) ya da hiçbir
  uygulama açamazsa pencerenin şeridinde ve başlık çubuğunda söylenir; ikincisinde dosyanın yolu da
  yazılır.
- Şablondaki değerler yaratıldığı günün varsayılanlarıdır: sonraki bir
  sürümde bir varsayılan değişirse bu dosya eski değeri tutar. Satırı silmek
  anahtarı güncel varsayılana döndürür.

### Şablon

```toml
# bateri settings. Changes apply as soon as you save this file.
# A key you delete goes back to its default. Values are case-sensitive; one that
# is not understood leaves its key alone and says so under the title — except
# clipboard.osc52 and remote.integration, which turn off instead, and
# terminal.restore_windows, which falls to "layout".

[terminal]
# 0 to 100000. Lines of history kept above the screen.
scrollback = 10000
# "block" | "underline" | "beam". The cursor's default shape: block fills the
# cell, underline sits below it, beam stands at its left edge. Programs such as
# vim may ask for a different shape while they run; this is the shape when none
# is asked for.
cursor = "block"
# "auto" | "on" | "off". Whether the cursor blinks: auto blinks until a program
# asks it to stop (vim in normal mode does), on blinks whatever the program
# says, off never blinks. Blinking asks for two frames a second, so it is off
# unless you choose it; with it on, it stops on its own 15 seconds after the
# window last drew anything and comes back with the next output or keystroke.
cursor_blink = "off"
# 0.0 to 0.5. How round the cursor's corners are, as a fraction of the cell's
# height: 0 is a sharp rectangle, 0.5 rounds a block into a stadium. It scales
# with the font size, so a larger point size keeps the same look.
cursor_radius = 0.10
# 0.0 to 3.0. How strong the soft shadow around the cursor is: 0 turns it off,
# 1 is the designed amount. It scales both how far the shadow reaches and how
# dark it is, because those two are one feeling, not two.
cursor_glow = 1.0
# "hollow" | "solid". What the cursor does while the window is not focused:
# hollow empties it to an outline, solid leaves it as it is. Either way a
# blinking cursor stops blinking until the window is focused again.
cursor_unfocused = "hollow"
# 0.05 to 5.0. Half the blink period in seconds: the cursor stays lit this
# long, then dark this long. Shorter costs more frames — 0.25 asks for four a
# second — and 0.5 is a blink you notice without it tiring the eye.
cursor_blink_interval = 0.5
# "never" | "running" | "always". When closing a tab or window, or quitting,
# asks first: running asks only while a program other than the shell is in
# the foreground (vim, ssh, a build) and names it, always asks even at an idle
# prompt, never closes without asking. Typing exit never asks, and neither do
# programs left running in the background.
confirm_close = "running"
# "all" | "layout" | "off". What comes back when bateri opens again after
# quitting, an update or a restart: all brings back the windows, tabs and
# splits with each pane's scrollback, layout brings back the windows without
# the scrollback (nothing you saw is written to disk), off starts with a
# single window and deletes what was saved. Shells always start fresh.
restore_windows = "all"

[appearance]
# "system" or a theme name. "system" follows the macOS light/dark appearance;
# any other value is a theme used in both — a file themes/NAME.toml next to
# this one, or a built-in theme, "bateri" (dark) or "bateri-light" (light).
theme = "system"
# Theme names, used while theme = "system".
light_theme = "bateri-light"
dark_theme = "bateri"

[font]
# A family name as shown in Font Book. Without it bateri uses SF Mono, or
# Menlo when SF Mono is not installed — SF Mono ships with Xcode, so it is not
# on every machine. A character the family lacks is drawn from the system font
# chain when it fits one cell; emoji, CJK and other wide glyphs stay as boxes.
# family = "Menlo"
# Greater than 0. Size in points.
size = 13
# 0.5 to 2. Line spacing as a multiple of the font's own: 1 is the font's own
# spacing, 1.4 is airy. Below 1 the rows tighten; letters are not clipped, their
# tails and accents overflow onto the neighbouring row.
line_height = 1.0
# 0.5 to 2. Letter spacing as a multiple of the font's own: 1 is the font's own
# spacing, 1.2 opens the columns a little. Letters keep their size and sit in
# the middle of the cell; below 1 they overflow onto the neighbouring column.
letter_spacing = 1.0

[clipboard]
# "copy" | "off". Lets programs in the terminal, also over ssh, copy text to
# the clipboard (OSC 52): copy allows it, off does not. They can never read it.
osc52 = "copy"

[motion]
# "snap" | "ease" | "spring". How the cursor travels between cells: spring
# glides and eases into place, ease glides for a fixed time, snap jumps there
# at once.
cursor_motion = "spring"
# "system" | "on" | "off". Whether to tone animations down to a short fade:
# system follows the macOS Reduce Motion setting, on and off decide it here.
reduce_motion = "system"
# "on" | "off". How scrolling back through history moves: on follows your
# fingers on a trackpad pixel by pixel, lets a flick coast to a stop, glides a
# mouse wheel notch and settles on a whole line when you let go; off moves
# line by line. Reduce Motion and cursor_motion = "snap" also move line by
# line.
smooth_scroll = "on"
# "off" | "fade" | "rise" | "pop" | "extrude" | "heat" | "echo" | "drop" |
# "ink" | "squeeze". How a letter you type in the dock at the bottom of the
# window appears: fade brings it in from clear, rise slides it up into place,
# pop springs it out from small, extrude stretches it out from its left edge,
# heat starts it in the cursor color and cools it to its own, echo sends a
# faint copy of it rippling outward, drop lets it fall into place with a small
# bounce, ink fills it from the middle of its strokes outward, squeeze starts
# it narrow and tall and lets it spring into shape. off shows it at once.
keypress = "fade"
# "off" | "iris" | "undertow" | "echo" | "bleed" | "unravel" | "recede" |
# "sublime" | "shatter". How a letter you delete in the dock goes: iris closes
# a round shutter over it, undertow pulls it down toward the cursor, echo
# swells it outward like a ripple, bleed lets its ink spread thin, unravel
# slides it apart in strips, recede shrinks it away, sublime lets it drift up
# like vapor, shatter breaks it into falling pieces. off removes it at once.
# Pasting, history and deleting a whole word or line are instant. cursor_motion = "snap" turns both off; Reduce Motion
# keeps only a fade for typing.
erase = "recede"

[shell]
# "auto" | "blocks" | "off". Whether bateri sets up the shell so it can report
# where prompts and commands begin and end. auto does it for shells bateri
# knows, and on those shells it also moves the line you type into the dock at
# the bottom of the window and draws the prompt itself. blocks keeps command
# blocks and marks but leaves the line and the prompt to your shell, the way a
# terminal normally works. off never sets anything up.
# Unlike every other key here, this one only takes effect in shells started
# after the change; shells already open keep what they were started with.
integration = "auto"

[remote]
# Colors the dock of an ssh or mosh session by the host it is on, so a
# production machine is never mistaken for another. Each entry names a host
# pattern and a mark: "production" (red), "staging" (yellow), "development"
# (green), "none" (no mark), or a color like "#c678dd". In a pattern * stands
# for any run of characters and ? for one, ignoring case; a pattern without @
# matches the host after any user@. The first entry that matches wins, so put
# exact names before wide patterns; "none" stops the search. Shell > Mark
# "host" as writes the entry for the host of the ssh tab you are in.
# hosts = [
#   { host = "prod-*", mark = "production" },
#   { host = "*.staging.example.com", mark = "staging" },
# ]
hosts = []
# true | false. Lets a plain ssh set up shell integration on the server, so
# the folder (and later command blocks) follow you there too. bateri writes a
# few small files to ~/.local/share/bateri/shell on the server and never
# touches its rc files. A host is set up only after bateri has seen a shell
# there once. A host marked "production" stays plain unless its entry says
# integration = true; integration = false in an entry turns one host off.
integration = true
# Sizes are written like "100MB" or "2GB" (B, KB, MB, GB, TB); folders start
# with / or ~/.
# A file larger than this asks before its preview downloads (cmd-click on a
# remote file name).
preview_max_size = "100MB"
# true | false. Previews open read-only. It is a hint: an app can unlock one,
# and a preview you changed is moved to the download folder, never deleted.
preview_read_only = true
# Where previews are kept.
preview_dir = "~/Library/Caches/bateri/Previews"
# "launch" | "1d" | "7d" | "30d". How long a preview stays after you last
# opened it; checked when bateri starts and once a day. launch keeps previews
# until bateri starts again.
preview_keep = "7d"
# The preview folder's size limit, applied when bateri starts, oldest first.
preview_limit = "2GB"
# Where "Download to Downloads" puts a remote file or folder.
download_dir = "~/Downloads"
# "ask" | "keep_both" | "replace". What a download does when the name already
# exists: ask, keep both (the new one gets a number), or replace the old one.
download_conflict = "ask"
# true | false. Notify when a transfer ends while bateri is in the background.
download_notify = true
# "sparkline" | "numbers" | "alerts" | "off". The remote machine's load at
# the right of the ssh status bar (Linux servers): sparkline shows the last
# CPU samples and the numbers, numbers only the numbers, alerts a small dot
# until a value passes its threshold, off nothing. Disk joins past 85%.
stats = "sparkline"
# Seconds between two samples, 2 to 60.
stats_interval = 3
```

Blok bir sınamayla şablona bağlıdır (`documented_template_is_the_template`).

### View ▸ Theme ▸

Menü her açılışta yeniden kurulur:

- **Match System** — `theme = "system"`: tema macOS'un görünümünü izler
  (`light_theme` / `dark_theme`).
- Gömülü temalar: `bateri`, `bateri-light`.
- `~/.config/bateri/themes/` altındaki her `{ad}.toml`, adıyla. Dizine yeni
  dosya koymak menüyü bir sonraki açılışta günceller. Nokta ile başlayan
  dosyalar ve `system.toml` listelenmez; gömülü bir temayı gölgeleyen dosya
  (`themes/bateri.toml`) ayrıca listelenmez, gömülü adın öğesi onu seçer.

İşaretli öğe ayar dosyasındaki `theme` değeridir.

Bir öğe seçmek temayı **ayar dosyasına yazar** ve dosya kaydedilmiş gibi hemen
uygulanır; uygulama yeniden açılınca da aynı tema gelir. Yazılan tek şey
`[appearance] theme` satırının değeridir:

- Yorumlar, boş satırlar, anahtarların sırası, tanınmayan anahtarlar ve
  satırın yanındaki yorum yerinde kalır.
- `light_theme` ve `dark_theme` değişmez: sabit bir tema seçip sonra
  Match System'e dönmek açık/koyu çiftini geri getirir.
- `[appearance]` bölümü yoksa dosyanın sonuna eklenir; `theme` yoksa bölümün
  içine. Satır içi (`appearance = { … }`) ya da noktalı
  (`appearance.theme = …`) yazılış korunur.
- Dosya yoksa önce [şablonla](#şablon) yaratılır, sonra satır yazılır.
- Dosya sembolik bağsa **hedefi** yazılır, bağ bağ olarak kalır.
- Satır sonları korunur: ilk satırı Windows satır sonuyla (CRLF) biten dosya
  CRLF kalır. Son satırın sonunda satır sonu yoksa eklenir.

Dosyaya **yazılmayan** durumlar — dosyanın içeriği olduğu gibi kalır ve başlık
çubuğu sebebini söyler (`…; the theme was not saved`):

- dosya geçersiz TOML ya da okunamıyor (izin, hedefi olmayan sembolik bağ);
- `appearance` bir bölüm değil (`appearance = 1`, `[[appearance]]`) ya da
  `theme` bir bölüm (`[appearance.theme]`): üstüne yazmak içeriğini silerdi.

Uyarı bir sonraki başarılı seçimde ya da dosya okunabilir ve geçerli
kaydedilince kalkar.

## Hata olursa

Hata pencerenin başlık çubuğunda, başlığın yanında İngilizce görünür:

```
bateri – settings.toml: line 2: `terminal.scrollback` must be an integer, found a string; using 10000
```

Birden çok hata varsa ilki ve kalanların sayısı yazılır (`(+2 more)`);
hepsi ayrıca standart hata çıkışına `bateri:` önekiyle basılır. Terminal her
durumda açılır.

**Tam ekranda** başlık çubuğu gizlenir ve uyarı onunla birlikte görünmez
olabilir (denenmedi); dosyayı pencere modunda açıp bakmak ya da stderr'deki
kopya yolu kalır.

| açılışta | sonuç |
|---|---|
| dosya yok | varsayılanlar, uyarı yok |
| dosya okunamıyor (izin, UTF-8 olmayan içerik, düz dosya değil, hedefi olmayan sembolik bağ) | varsayılanlar, yalnız `osc52` ve `[remote] integration` **kapalı**, `restore_windows` **`"layout"`**; uyarı |
| geçersiz TOML | **bütün** ayarlar varsayılan, yalnız `osc52` ve `[remote] integration` **kapalı**, `restore_windows` **`"layout"`**; uyarı satırı gösterir |
| bir anahtarın değeri kabul edilmiyor | yalnız o anahtar varsayılan (ya da sınırı; `osc52` ve `[remote] integration` için kapalı, `restore_windows` için `"layout"`), uyarı |
| tanınmayan anahtar ya da bölüm | sessizce yoksayılır |
| seçilen tema bulunamıyor | görünüme uyan gömülü tema (koyuda `bateri`, açıkta `bateri-light`), uyarı |
| tema dosyası okunamıyor, boş ya da geçersiz TOML | görünüme uyan gömülü tema, uyarı (aynı adlı gömülü tema **kullanılmaz**) |
| tema dosyasında bir renk kabul edilmiyor | yalnız o renk `bateri`'ninki, uyarı |
| font ailesi bulunamıyor | varsayılan font (SF Mono, yoksa Menlo), uyarı |
| font ailesi eşaralıklı değil | aile yine kullanılır, uyarı |

Tema kuralı görünüm değişiminde de aynı: yeni görünümün teması
kullanılamıyorsa o görünüme uyan gömülü tema gelir — pencere öteki
görünümün temasında kalmaz.

**Kaydettiğiniz anda** ise kural düzenlemeyi korur: yarım kalmış bir kayıt
ekranı bozmaz, uyarı çıkar ve dosyayı düzeltip kaydedince uyarı kalkar.

| kayıt anında | sonuç |
|---|---|
| ayar dosyası geçersiz TOML ya da okunamıyor | **hiçbir ayar değişmez**, uyarı |
| ayar dosyası silindi ya da boşaltıldı | ayarlar değişmez, uyarı yok; varsayılanlar uygulamayı yeniden açınca gelir |
| bir anahtarın değeri kabul edilmiyor | o anahtar **değişmez**, uyarı; tavanı aşan `scrollback` tavana iner, kabul edilmeyen `osc52` ve `[remote] integration` **kapanır**, `restore_windows` **`"layout"`** olur |
| anahtar dosyadan silindi | o anahtar varsayılanına döner |
| seçilen tema bulunamıyor, dosyası okunamıyor, boş ya da geçersiz TOML | **ekrandaki tema kalır**, uyarı |
| font ailesi bulunamıyor | varsayılan font, uyarı; adı düzeltip kaydedince uyarı kalkar |

Menüden tema seçerken dosya geçersiz ya da okunamıyorsa dosyaya **yazılmaz**;
bkz. [View ▸ Theme ▸](#view--theme-).

Silinen dosyanın ayarları değiştirmemesi bilerek: çoğu editör kaydederken
eski dosyayı bir an kenara taşır ya da önce boşaltıp sonra yazar; varsayılanlara
dönmek her kayıtta pencereyi çakar, `scrollback` büyütülmüşse geçmişin fazlasını
silerdi. Kabul edilmeyen değerin anahtarı değiştirmemesi de: `scrollback`'i
yanlışlıkla metin olarak kaydetmek varsayılana düşseydi geçmişin fazlası o
anda silinirdi.

"Geçersiz TOML" sözdizimi hatasından geniştir: aynı anahtarı iki kez yazmak
ve TOML'un tam sayı sınırını (9 223 372 036 854 775 807) aşan bir sayı da
dosyanın tamamını geçersiz yapar.

Tanınmayan anahtarın sessiz kalması bilerek: sonraki sürümlerin anahtarını
bugünkü sürüm hata diye göstermemeli.

`osc52`'nin kuralın dışında kalması da bilerek: dosya okunamayınca ya da
değeri yanlış yazılınca (`"of"`) kullanıcının onu kapatıp kapatmadığı
bilinemez, ve yanlış tahmin öteki anahtarlarda ekranda görünürken burada
görünmez — uzaktaki bir program panoya sessizce yazabilirdi. Kapalıya düşmek
geri alınabilir: dosyayı düzeltip kaydetmek yeter. `restore_windows` aynı
kuralla `"layout"`'a düşer: pencereler yine gelir (görünen yarısı), geçmiş
diske yazılmaz (görünmeyen yarısı).

## Anahtarlar

### `[terminal]`

```toml
[terminal]
scrollback = 10000
cursor = "block"
cursor_blink = "off"
cursor_radius = 0.10
cursor_glow = 1.0
cursor_unfocused = "hollow"
cursor_blink_interval = 0.5
confirm_close = "running"
restore_windows = "all"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `scrollback` | tam sayı, `0`–`100000` | `10000` | geçmişte tutulan satır sayısı |
| `cursor` | `"block"` \| `"underline"` \| `"beam"` | `"block"` | imlecin **varsayılan** şekli |
| `cursor_blink` | `"auto"` \| `"on"` \| `"off"` | `"off"` | imleç yanıp söner mi |
| `cursor_radius` | ondalık, `0.0`–`0.5` | `0.10` | imleç köşesinin yuvarlaklığı, hücre **yüksekliğinin** oranı |
| `cursor_glow` | ondalık, `0.0`–`3.0` | `1.0` | imlecin çevresindeki gölgenin gücü; `0` kapatır |
| `cursor_unfocused` | `"hollow"` \| `"solid"` | `"hollow"` | pencere odakta değilken imleç: `hollow` içini boşaltır, `solid` dokunmaz |
| `cursor_blink_interval` | ondalık, `0.05`–`5.0` | `0.5` | blink'in **yarım** periyodu, saniye |
| `confirm_close` | `"never"` \| `"running"` \| `"always"` | `"running"` | sekme, pencere ya da uygulama kapanırken ne zaman sorulsun |
| `restore_windows` | `"all"` \| `"layout"` \| `"off"` | `"all"` | bateri yeniden açılınca ne geri gelsin |

- `100000`'den büyük değer **`100000`** olur ve uyarı verir. Sınır
  alacritty'nin kendi ayar sınırı (`MAX_SCROLLBACK_LINES`); ölçülmüş bir
  bellek bütçesi değil.
- Negatif ya da tam sayı olmayan değer (`"lots"`, `1.5`) varsayılana döner
  ve uyarı verir.
- `0` geçerli: geçmiş tutulmaz.
- Değer uygulama açıkken değişince **hemen** uygulanır: küçültmek fazla
  satırları o anda siler, sonra büyütmek silineni geri getirmez. Yazarken
  kendiliğinden kaydeden bir editörde ara değer de (`100000` → `1`) kayıttır.

`cursor` yalnız **varsayılanı** söyler: terminaldeki uygulama DECSCUSR
(`\e[5 q`) ile şekli değiştirebilir ve o söz dinlenir — vim insert modda
çubuk isterse çubuk olur, çıkarken bloğa döner. Buradaki değer, kimse bir şey
istemediğindeki hâl. Tanınmayan değer (`"bar"`, `"Block"`) anahtarı
**değiştirmez** ve uyarı verir; büyük/küçük harf duyarlı. İnce şekillerin
kalınlığı fontun kendi alt çizgi metriğinden gelir, yani punto ya da font
değişince caret de onunla değişir.

`cursor_blink` üç değerli, çünkü iki ayrı soru var: `"auto"` **söner ve
uygulama onu durdurabilir**, `"on"` her zaman söner, `"off"` hiç söndürmez —
son ikisi uygulamanın dediğini **ezer**.

`"auto"`da uygulamanın sözü **alternatif ekrandan çıkınca bitiyor**: imlecin
şekli de sönmesi de `[terminal] cursor` + `cursor_blink` tabanına döner.
Bu olmadan `"auto"` pratikte "ilk tam ekran uygulamasına kadar" demekti ve
sebebi DECSCUSR değil terminfo: `xterm-256color`'da
`cnorm = \e[?12l\e[?25h`, yani "imleci normal görünür yap" komutunun
**içinde** blink'i kapatan özel mod 12 var. vim, less, man, htop — `cnorm`
gönderen her program çıkarken blink'i öldürüyor ve geri açan kimse yok
(ölçüldü 2026-09-20: `vim -u NONE`'un bütün oturumu 160 bayt, içinde
`\e[?12h` ve `\e[?12l` var, DECSCUSR hiç yok).

`"auto"`nun tabanı bilerek **açık**: kapalı olsaydı hiçbir şey blink
istemediği için (ne zsh ne bizim sarmalayıcımız DECSCUSR gönderiyor) düz bir
promptta `"off"` ile birebir aynı olurdu ve üç değerden ikisi ayırt
edilemezdi.

Varsayılan `"off"` ve bu bir ürün kararı: yanıp sönen imleç pencereyi kalıcı
olarak meşgul tutar (saniyede iki kare) ve bu terminalin ana vaadi boşta hiç
kare çizmemek. Açtığında bedeli sınırlı kalıyor — pencere 15 saniyedir
hiçbir şey çizmediyse blink **duruyor** ve imleç görünür hâlde kalıyor; ilk
çıktıda ya da tuşta geri geliyor. Sayaç **çizime** bakıyor, klavyeye değil:
`tail -f` gibi akan bir çıktı blink'i ayakta tutar.
Hareketi Azalt açıkken blink hiç başlamaz: erişilebilirlik ayarı animasyon
*eklemez*.

`cursor_radius` ve `cursor_glow` imlecin **görünüşünü** ölçekler, yeni bir
ölçü tanımlamaz: yarıçap hücre **yüksekliğinin** oranı, `cursor_glow` ise
tasarımın kendi gölge ölçüsünün çarpanı — `1.0` varsayılan görüntü, `0`
gölgeyi kapatır. İkisi de punto ile büyür, yani Cmd +/− imleci orantılı
bırakır. Gölge **tek sayı**, yayılma ve koyuluk ayrı ayrı değil: ikisi tek bir
his ve ayrı verilseydi "hiçbir şeyin geniş halesi" gibi anlamsız hâller
yazılabilirdi.

`cursor_unfocused` pencere odakta değilken imlecin ne olacağını söyler:
`"hollow"` içini boşaltıp çerçeveye çevirir, `"solid"` hiç dokunmaz. Blink'e
**etkisi yok** — odakta olmayan pencerede blink her hâlde durur, o ayrı bir
sinyal.

`cursor_blink_interval` blink'in **yarım** periyodu: imleç bu kadar açık, bu
kadar kapalı kalır. Kısaltmanın bedeli doğrusal — `0.25` saniyede dört kare
ister — ve alt sınır (`0.05`) tavanı orada durdurur. Bu anahtarın yanlış
değeri `make smoke`'ın sessizlik katına **yakalanmaz**: süreli koşu ayar
dosyasını hiç okumaz ve blink varsayılanı kapalıdır, yani tek koruma kabul
aralığının kendisidir.

`confirm_close` kapatmadan önce sorulup sorulmayacağını söyler. Soru
⌘W'de ve sekme çubuğunun × düğmesinde o sekme için, kırmızı düğmede ve
⇧⌘W'de pencerenin bütün sekmeleri için, "Close Other Tabs"ta öteki sekmeler
için **tek** bir sayfa, ⌘Q'da (Dock ▸ Quit, oturum kapatma
ve yeniden başlatma dahil) bütün pencereler için **tek** bir uyarıdır ve
koşan programları adıyla sayar; Return kapatır, Esc vazgeçer.

- `"running"` (varsayılan) yalnız kabuğun **dışında** bir program ön
  plandayken sorar: vim, `ssh`, Claude Code, süren bir derleme. `ssh`
  sayılır, çünkü kapatmak uzaktaki oturumu da bitirir. Boş bir prompt'ta
  sekme sormadan kapanır.
- `"always"` boş prompt'ta da sorar; `"never"` hiç sormaz.
- Kabukta `exit` yazmak **hiçbir** değerde sormaz: kapanışı isteyen zaten
  kabuk.
- Sayılmayanlar: arka plan işleri (`sleep 100 &` — zsh `exit`'te onları
  kendisi uyarır), kabuğun kendi içinde koşan bir döngü ve kabuğun yerine
  geçen program (`exec vim`). Üçü de kabuk boştaymış gibi görünür.

Değer kapanış anında okunur, yani kaydettiğiniz anda geçerlidir.

`restore_windows` bateri kapanıp yeniden açıldığında — ⌘Q, güncelleme,
oturum kapatma, yeniden başlatma — neyin geri geleceğini söyler:

- `"all"` (varsayılan) pencereleri, sekmeleri (grubu, sırası, seçili
  sekme), bölmeleri (yön ve oran), odaktaki ve büyütülmüş pane'i, her
  pane'in dizinini ve punto farkını **ve geçmişini** renk ve biçimiyle geri
  getirir.
- `"layout"` aynı düzeni geçmişsiz getirir: ekranda gördüğünüz hiçbir şey
  diske yazılmaz. Daha önce `"all"` ile kaydedilmiş bir geçmiş de
  gösterilmez, okunmadan silinir.
- `"off"` tek bir boş pencereyle açar, hiçbir şey yazmaz ve kalanı siler.
- Kabuklar **her zaman yeni**: koşan programlar (vim, bir derleme) kapanışta
  biter, geri gelen geçmiş yalnız metin. Kayıt yalnız düzgün kapanışta
  yazılır; çökmeden sonra açılış tek pencereyle olur.
- **ssh'taki pane** yerel kabukla, eski yerel dizininde gelir ve bağlantının
  satırı (`ssh prod` gibi) giriş satırında **hazır ama çalıştırılmamış**
  bekler: ⏎ bağlanır. Kendiliğinden bağlanmaz — güncellemeden sonra her
  pane'in aynı anda parola sorması ya da bir production sunucusuna siz
  dokunmadan bağlanılması istenmez.
- Kayıt `~/Library/Application Support/bateri/session/dev.bateri.bateri/`
  altındadır (dizin yalnız sizin, dosyalar `0600`) ve açılışta okunur okunmaz
  silinir. Geçmiş orada **düz metin** olarak durur — ekrana basılmış bir
  token ya da parola dahil — ve dizin Time Machine'in yedeklediği yerdedir;
  bateri kapalıyken yedeklenen bir kopya orada kalır. İstemiyorsanız
  `"layout"`.
- İki bateri aynı anda açıksa yalnız ilki kaydeder ve geri yükler.

Bölüm satır içi de yazılabilir: `terminal = { scrollback = 5000 }`.
`[[terminal]]` (bölüm dizisi) bölüm sayılmaz ve uyarı verir.

### `[appearance]`

```toml
[appearance]
theme = "system"
light_theme = "bateri-light"
dark_theme = "bateri"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `theme` | `"system"` ya da tema adı | `"system"` | kullanılacak renk teması |
| `light_theme` | tema adı | `"bateri-light"` | `theme = "system"` iken açık görünümün teması |
| `dark_theme` | tema adı | `"bateri"` | `theme = "system"` iken koyu görünümün teması |

- `theme = "system"` temayı macOS'un görünümüne bırakır: açıkta
  `light_theme`, koyuda `dark_theme`. Görünüm değişince tema anında değişir.
- `theme = "{ad}"` görünümden bağımsız sabit bir temadır; `light_theme` ve
  `dark_theme` o sırada okunmaz ama yerinde kalır — `"system"`'e dönünce
  çift geri gelir.
- `"system"` bir tema adı değildir: `themes/system.toml` seçilemez, ve
  `light_theme`/`dark_theme` bu değeri kabul etmez (kendi varsayılanına
  döner, uyarı verir).
- Ad önce `~/.config/bateri/themes/{ad}.toml` olarak aranır, yoksa gömülü
  temalar arasında. Gömülü iki tema var: `bateri` (koyu) ve `bateri-light`
  (açık).
- Aynı adlı bir dosya gömülü temayı **gölgeler**: `themes/bateri.toml`
  yazan kullanıcı gömülü `bateri`'yi değil kendi dosyasını görür.
- Hiçbir yerde bulunamayan ad görünüme uyan gömülü temaya döner (koyuda
  `bateri`, açıkta `bateri-light`) ve uyarı verir; açılışta da görünüm
  değişince de.
- Boş ad ya da `/` içeren ad (`"../x"`) kabul edilmez, anahtarın
  varsayılanına döner ve uyarı verir: tema `themes/` dizininin dışından
  okunmaz.

### `[font]`

```toml
[font]
family = "Menlo"
size = 13
line_height = 0.9
letter_spacing = 1.0
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `family` | metin | yok (SF Mono, yoksa Menlo) | yazı ailesi |
| `size` | sayı, `0`'dan büyük | `13` | punto |
| `line_height` | sayı, `0.5` – `2` | `1.0` | satır aralığı çarpanı |
| `letter_spacing` | sayı, `0.5` – `2` | `1.0` | harf aralığı çarpanı |

- `family` bir **aile adıdır**, Font Kitabı'nda görünen ad (`"JetBrains
  Mono"`, `"Menlo"`); büyük/küçük harf fark etmez. Tek bir yüzün PostScript
  adı (`"Menlo-Regular"`) aile sayılmaz ve "bulunamadı" uyarısı verir.
- Aile makinede yoksa varsayılan font kullanılır ve başlık çubuğunda
  söylenir: `font "Fira Code" not found; using Menlo`. Adı düzeltip
  kaydedince font değişir, uyarı kalkar.
- **Varsayılan font iki halkalı bir zincirdir:** önce SF Mono, o yoksa
  Menlo. SF Mono Xcode ile geliyor ve **her makinede bulunmaz**; kurulu
  değilse uyarı da verilmez, çünkü bu bir kusur değil tasarlanmış geri
  düşüştür — ikisi de eşaralıklı. Tablodaki "SF Mono, yoksa Menlo" bunu
  söylüyor: ekranda hangisinin olduğunu merak ediyorsan `family`'yi hiç
  yazmadığında gördüğün yüz odur.
- Eşaralıklı olmayan bir aile (`"Helvetica"`) **reddedilmez**, uyarı verir:
  `font "Helvetica" is not monospaced; text may not line up`. Hücre
  genişliği boşluk karakterinden gelir; ondan geniş harfler hücreye
  kırpılır.
- **Seçili fontta olmayan karakter sistemden gelir.** Tek hücreye sığdığı
  sürece macOS'un kendi font zincirinden çizilir; fontunu değiştirmene ya da
  bir yedek liste tanımlamana gerek yok. Ölçüt karakterin **ilerlemesi**:
  hücreden genişse çizilmez, **kutu kalır**. Bu bilinçli — emoji, CJK (`漢`),
  powerline'ın ayraçları (`U+E0B0`) ve Braille (spinner'ların `⠋⠙⠹`'si)
  hücrenin genişliğini aşıyor, ve yarım çizilmiş bir glyph sessiz bir bozulma
  olurdu; kutu ise görünür bir eksiklik, yani neyin çizilemediğini sana
  söyler. Bu karakterleri gerçekten çizmek ayrı bir işin konusu
  (`docs/YOL-HARITASI.md`).
- Ölçünün **ilerleme** olduğunun bir bedeli var: dar ilerleyip geniş boyayan
  bir glyph kapıdan geçer ve taşan kısmı kırpılır. Bugünkü font zincirinde
  böyle bir karakterle karşılaşılmadı, ama söz "hiç kırpılmaz" değil
  "ilerlemesi sığmayan hiç çizilmez".
- **Kutu ve blok çizim karakterleri bunun dışında** (`─ │ ┌ █ ▀ ▄`): onlar
  fontta zaten var, yedeğe hiç uğramıyorlar. Alt alta gelen iki bloğun
  arasında ince bir şerit görüyorsan sebebi bu değil — fontun glyph'i hücreyi
  tam doldurmuyor ve çaresi ayrı bir iş (`docs/YOL-HARITASI.md`).
- `family = ""` ya da anahtarın olmaması varsayılan font demektir, uyarı
  vermez.
- `size` tam sayı da ondalıklı da olabilir (`13`, `13.5`). Sıfır, negatif,
  `nan`, `inf` ya da sayı olmayan değer varsayılana (açılışta `13`, kayıt
  anında o anki punto) döner ve uyarı verir.
- Punto ekranın ölçeğiyle çarpılıp **sessizce** 4–144 aralığına çekilir:
  Retina ekranda (2×) yazılan punto 2–72 arasında etkilidir, normal ekranda
  4–144; dışındaki değer en yakın sınır gibi çizilir. Uyarı yok, çünkü aynı
  değer pencere ekran değiştirdikçe sınırın bir içinde bir dışında
  kalabilirdi.
- `line_height` satır aralığını **fontun kendi aralığının katı** olarak
  verir: `1.0` tam olarak fontun istediği aralık, `1.4` ferah. Fazlalık
  satırın altına ve üstüne **eşit** dağılır, yani harfler hücrenin ortasında
  kalır; alt çizgi ve üstü çizili de birlikte iner.
- **`1`'in altında satırlar sıkışır, harf kesilmez**: harf yine fontun kendi
  boyunda çizilir ve sığmayan kısmı komşu satıra **taşar** (iTerm2'deki
  gibi). `İ Ö Ü`'nün aksanı üstteki, `g j y`'nin kuyruğu alttaki satıra
  değebilir. Alt sınır `0.5`; altında üst üste binme okunmaz hâle gelir.
  Üst sınır `2`, çünkü hücre büyüdükçe glyph atlasına sığan karakter sayısı
  düşer (aşağıdaki maddeyle aynı bütçe). Aralık dışındaki değer varsayılana
  döner ve uyarı verir.
- Taşma **aynı yüzeyin içinde** kalır: pencerenin altındaki giriş satırında
  harf giriş alanının tepesine kadar taşar, alanın dışına çıkmaz; pencerenin
  kenarında ise kesilir. Kutu ve blok çizgileri (`─ │ █`) sıkışan hücreyi
  yine boydan boya doldurur, yani `tree`'nin çizgileri bitişik kalır.
- Satır aralığı **kayıt anında** uygulanır, punto gibi; Cmd +/− puntoyu
  oynatır, çarpan olduğu yerde kalır ve yeni puntoya göre ölçeklenir.
- `letter_spacing` `line_height`'ın **yatay ikizidir**: hücre genişliğini
  fontun kendi ilerlemesinin katı olarak verir. `1.0` fontun aralığı, `1.2`
  sütunları biraz açar. Harfin boyu değişmez, **genişleyen hücrenin
  ortasında** durur; sütunlar açılır. Kutu ve blok çizgileri (`─ │ █`)
  hücreyi boydan boya doldurduğu için geniş hücrede de birbirine bitişik
  kalır, iki sütunlu karakter (`中`, emoji) iki sütunun ortasında durur ve
  pencerenin altındaki bağlam satırı da aynı oranda açılır.
- **`1`'in altında sütunlar sıkışır, harf kesilmez**: harf kendi boyunda
  kalır ve `M`, `W` gibi geniş harflerin kenarı komşu sütuna taşar. Alt sınır
  `0.5`, satır aralığıyla aynı. Üst sınır `2`, satır aralığıyla aynı atlas
  bütçesi; iki çarpan birlikte en üstteyken de atlas bütün çizgi ve blok
  karakterlerini taşır. Aralık dışındaki ya da sayı olmayan değer varsayılana
  (kayıt anında o anki değere) döner ve uyarı verir.
- **Bilinen sınır:** çok dar harf aralığında (yaklaşık `0.7`'nin altı) iki
  sütunlu karakter (`中`, emoji) taşmak yerine **küçültülerek** çizilir, çünkü
  sistemden gelen karakterin sığması gereken kutu iki sütunla birlikte
  daralır.
- Harf aralığı da **kayıt anında** uygulanır: sütun sayısı yeniden
  hesaplanır ve kabuk yeni boyutu alır. Cmd +/− çarpanı taşır, yani yeni
  puntoda harfler yine aynı oranda açık durur.
- Çok büyük puntoda glyph atlası çabuk dolar: dolduktan sonra ekranda ilk
  kez görünen karakterler kutu (□) olarak çizilir. Punto küçültülünce ya da
  uygulama yeniden açılınca geçer.
- Font kaydettiğiniz anda değişir: pencere boyutu aynı kalır, sütun ve satır
  sayısı yeni hücreye göre yeniden hesaplanır ve kabuk ile içindeki program
  (vim, less) yeni boyutu pencere boyutlandırılmış gibi alır; uzun satırlar
  yeniden sarılır.
- Kalın ve eğik yüzü olmayan ailede o metin düz yüzle çizilir; bu uyarı
  vermez (standart hata çıkışına bir satır düşer).

#### Geçici punto: Cmd +, Cmd −, Cmd 0

**View ▸ Bigger** (Cmd +), **Smaller** (Cmd −) ve **Actual Size** (Cmd 0)
puntoyu **geçici** olarak değiştirir: dosyaya yazılmaz, uygulama kapanınca
gider.

- Her basış bir punto; aralık 4–72. Aralığın ucundaki basış hiçbir şey
  yapmaz, yani tuşu basılı tutup geri dönmek hemen görünür. Dosyadaki `size`
  aralığın dışındaysa basış yalnız aralığa doğru çalışır.
- **Actual Size** dosyadaki `size`'a döner.
- Dosyada `size`'ı değiştirip kaydetmek geçici farkı bırakır: yazdığınız
  punto görünür. `family` ya da başka bir anahtarı değiştirmek farkı korur.

### `[clipboard]`

```toml
[clipboard]
osc52 = "copy"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `osc52` | `"copy"` ya da `"off"` | `"copy"` | terminaldeki programın panoya yazıp yazamayacağı |

- **OSC 52**, bir programın terminal üzerinden panoya metin yazma dizisidir.
  En bilinen kullanımı ssh'la bağlanılan makinedeki vim ya da tmux: orada
  kopyalanan metin bu Mac'in panosuna gelir, uzak makinenin panoya erişimi
  olmasa da. `"copy"` buna izin verir, `"off"` diziyi yoksayar.
- Yazılan pano, Cmd-C'nin yazdığı **genel panodur**. Program art arda çok
  sayıda kopya yollarsa yalnız sonuncusu panoda kalır.
- **Okuma yönü yok**, hiçbir değerle açılmaz: terminaldeki bir program
  panonuzdaki metni okuyamaz. Bu yüzden `"paste"` gibi bir değer yoktur.
- **Bedeli:** `"copy"` iken arka planda koşan bir program da (uzaktaki dahil)
  panoya yazabilir ve sizin kopyaladığınızı değiştirebilir. İstemiyorsanız
  `"off"`.
- Dizinin hedefi fark etmez: birincil seçime (`p`, `s`) yazan dizi de genel
  panoya yazar. macOS'ta tek pano var; vim'de `*` ile `+` burada aynı panodur
  ve Neovim `*`'ı `p` diye yollar, yani `clipboard=unnamed` ayarlı bir
  Neovim'in ssh'taki kopyası da gelir. Boş metin panoyu silmez, yoksayılır.
- Kopyanın boyut sınırı yok. Pano yazılırken pencere yeni kare çizmez; çok
  büyük bir kopyada bu fark edilebilir (hangi boyutta olduğu ölçülmedi).
- Tanınmayan değer (`"paste"`, `"Copy"`, `true`) ve `[clipboard]`'ın bölüm
  olmaması **kapalıya** düşer ve uyarı verir — öteki anahtarlar gibi
  varsayılana (açık) değil; açılışta okunamayan ya da geçersiz ayar dosyası
  da (bkz. [Hata olursa](#hata-olursa)).
- Kaydettiğiniz anda geçerli olur; açık programı yeniden başlatmak gerekmez.

### `[motion]`

```toml
[motion]
cursor_motion = "spring"
reduce_motion = "system"
smooth_scroll = "on"
keypress = "fade"
erase = "recede"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `cursor_motion` | `"snap"`, `"ease"` ya da `"spring"` | `"spring"` | imlecin hücreler arasında nasıl gittiği |
| `reduce_motion` | `"system"`, `"on"` ya da `"off"` | `"system"` | animasyonların kısılıp kısılmayacağı |
| `smooth_scroll` | `"on"` ya da `"off"` | `"on"` | geçmişte kaydırmanın pürüzsüz mü satır satır mı gittiği |
| `keypress` | `"off"`, `"fade"`, `"rise"`, `"pop"`, `"extrude"`, `"heat"`, `"echo"`, `"drop"`, `"ink"` ya da `"squeeze"` | `"fade"` | dock'ta yazılan harfin nasıl geldiği |
| `erase` | `"off"`, `"iris"`, `"undertow"`, `"echo"`, `"bleed"`, `"unravel"`, `"recede"`, `"sublime"` ya da `"shatter"` | `"recede"` | dock'ta silinen harfin nasıl gittiği |

- **`"spring"`** — imleç yeni yerine bir yayla kayar ve yavaşlayarak oturur;
  hedefi aşmaz. Uzak bir sıçrama yakın bir sıçramadan biraz uzun sürer.
- **`"ease"`** — kayma **sabit** sürer, mesafe ne olursa olsun; sonuna doğru
  yavaşlar, hedefi aşmaz.
- **`"snap"`** — kayma yok, imleç doğrudan yeni hücrede görünür ve içerik de
  anında yerine gider. Hareketi tamamen kapatmanın yolu bu: dock'taki yazma
  ve silme efektleri de (`keypress`, `erase`) kapanır.
- Aynı stil **içeriğin yükselmesini** de sürer: bateri içeriği pencerenin
  tabanına yaslar, yani yeni bir satır geldiğinde geçmiş yukarı kayar ve imleç
  dipteki satırında durur. Kayan şey bütün ızgaradır, imleç değil.
- **Pencere dolduktan sonra da kayar.** Ekran dolup eski satırlar geçmişe
  itilmeye başladığında yeni satırlar yine süzülerek gelir; tepeden çıkan
  satırlar kayma bitene kadar görünür kalır. Bir karede ekrandan fazla satır
  gelen çok hızlı çıktıda kayma yapılmaz, en yeni satırlar hemen görünür.
- **Boş kalacak bir alana doğru yalnız yukarı kayar.** İçerik büyüyünce (yeni satır,
  `vim`/`less` gibi tam ekran bir uygulamanın açılması) ızgara yukarı
  **süzülerek** gelir; içerik daralıp üstte **boş** bir alan bırakacaksa (o
  uygulamadan çıkmak, dolu bir ekranda `clear`, silinen satırlar) **anında**
  yerine oturur. Sebebi his: yukarı akış içeriğin gelmesi gibi okunuyor, aşağı
  iniş düşmesi gibi.
- **Aşağı dönüş de kayar, eğer boşluğu geçmiş dolduruyorsa.** Bir tamamlama
  listesi kapanıp yerini eski satırlarınız aldığında ekranda aşağı inen şey
  boşluk değil, yukarıdan **gelen geçmiştir** — orada ızgara süzülerek döner.
  Ctrl-L ya da `clear` ile kasten temizlediğiniz ekran bu kolun dışında:
  geçmiş geri gelmez, yani oraya kayacak bir şey de yoktur.
- **Izgaranın başka sebeple yer değiştirmesi de kaymaz:** pencereyi
  boyutlandırmak, fontu ya da puntoyu değiştirmek. Orada hareket eden şey
  içerik değil, pencerenin kendisidir. Geçmişte kaydırmanın kendi ayarı var:
  `smooth_scroll` (aşağıda).
- Kaydettiğiniz anda geçerli olur. O sırada kayan bir imleç varsa `"snap"`
  onu hedefinde bitirir, öteki iki stil kaymayı bulunduğu yerden devralır:
  imleç hiçbir stil değişiminde ışınlanmaz.
- Tanınmayan değer (`"sprong"`, `"Spring"`, `true`) yalnız bu anahtarı
  etkiler (açılışta `"spring"`, kayıt anında ekrandaki stil) ve uyarı görünür.

`reduce_motion` animasyonların kısılıp kısılmayacağını söyler:

- **`"system"`** — macOS'un Sistem Ayarları ▸ Erişilebilirlik ▸ Görüntü ▸
  Hareketi Azalt ayarını izler. Ayarı açıp kapatmak bateri'yi yeniden
  başlatmadan etkiler.
- **`"on"`** — sistem kapalıyken de kısar, **`"off"`** sistem açıkken de
  kısmaz. İkisi sistemi hiç okumaz.
- Kısıldığında imleç kaymaz: yeni hücresinde **kısa bir belirmeyle** (90 ms)
  görünür, eski hücresinde iz bırakmaz. Kısılan şey kaymanın kendisi, imlecin
  görünürlüğü değil.
- İçeriğin yükselmesi kısıldığında **belirmez, anında yerine gider**: her yeni
  satırda bütün ekranın belirmesi, kısmaya çalıştığı hareketten beter olurdu.
- Belirme **duraksamadan sonraki** harekete aittir: normal yazma hızında her
  harf imleci yeni yerinde kısa bir belirmeyle gösterir. Hareketler
  belirmenin süresinden daha sık geldiğinde — çıktı akarken ya da çok hızlı
  yazarken — imleç tam opak kalır ve yeni yerine sessizce geçer; yoksa
  saniyede on kereden hızlı bir titreme doğardı (eski davranışta imleç akan
  çıktıda büsbütün görünmez oluyordu).
- Dock'ta yazılan harf, `keypress` ne olursa olsun **yalnız belirir**
  (`"fade"`); silinen harf efektsiz, anında gider. İmleçle aynı kural: kısılan
  şey hareket, yazdığınızın onayı değil. `keypress = "off"` ise kapalı kalır.
- `cursor_motion = "snap"` bunun **üstündedir**: hareketi zaten kapatmış
  olan kullanıcıya Hareketi Azalt bir belirme *eklemez*.
- Kaydettiğiniz anda geçerli olur. O sırada kayan bir imleç varsa hedefinde
  bitirilir — açarken de kapatırken de imleç ışınlanmaz.
- Tanınmayan değer (`"yes"`, `"System"`, `true`) yalnız bu anahtarı etkiler.

`smooth_scroll` geçmişte kaydırmanın nasıl gittiğini söyler:

- **`"on"`** — trackpad'le kaydırırken ekran parmağınızı **piksel piksel**
  izler ve tepede yarım bir satır görünebilir; fırlattığınızda macOS'un
  momentumuyla yavaşlayarak durur. Parmağınızı kaldırdığınızda ya da momentum
  bittiğinde ekran kısa bir süzülmeyle **en yakın satıra oturur**, yani
  dinlenen pencerede yarım satır kalmaz. Klasik farenin tekerleği aynı
  mesafeyi gider ama çentik sıçramaz, `cursor_motion`'ın stiliyle kısa bir
  süzülmeyle gider.
- **`"off"`** — satır adımı: her olay tam satırlarla kaydırır, animasyon yok.
  Hareketi Azalt açıkken ve `cursor_motion = "snap"` iken de aynısı olur —
  hareketi kapatmış olana kaydırma animasyon *eklemez*.
- Yalnız bateri'nin **kendi geçmişini** kaydırırken geçerlidir. `vim`, `less`
  gibi tam ekran uygulamalarda tekerlek ok tuşuna, fareyi isteyen
  uygulamalarda (Claude Code, `htop`) tekerlek raporuna dönüşür ve ikisi de
  bugünkü gibi tam satırla gider.
- Kaydırmayı kendisi yumuşatan bir araç (Mos gibi) kullanıyorsanız ikisi
  üst üste binmesin diye `"off"` seçebilirsiniz; referans ürün de anahtarı bu
  gerekçeyle sunuyor (`docs/ARASTIRMA.md`). Bu araçlarla nasıl davrandığı
  ölçülmedi.
- Kaydettiğiniz anda geçerli olur.
- Tanınmayan değer (`"yes"`, `"On"`, `true`) yalnız bu anahtarı etkiler.

`keypress` ile `erase` pencerenin altındaki **dock**'ta — komutu yazdığınız
satırda — harflerin nasıl geldiğini ve gittiğini söyler:

- **`keypress`** — yazdığınız harf:
  - **`"fade"`** — yerinde saydamdan tam renge belirir.
  - **`"rise"`** — hücrenin biraz altından yukarı kayarak yerine oturur,
    kayarken belirir.
  - **`"pop"`** — küçük doğar, bir an yerinden biraz büyür ve oturur.
  - **`"extrude"`** — sol kenarından sağa doğru uzayarak çıkar.
  - **`"heat"`** — temanın imleç renginde (`cursor`) doğar ve kendi rengine
    soğur. Emoji boyanmaz, yalnız belirir.
  - **`"echo"`** — yerinde belirir; soluk bir kopyası büyüyerek dışa doğru
    dağılıp söner.
  - **`"drop"`** — hücrenin üstünden düşer, hafifçe sekip yerine oturur.
  - **`"ink"`** — önce çizgilerin çekirdeği görünür, mürekkep kenarlara
    yayılır gibi dolar. Emojide düz bir belirmedir.
  - **`"squeeze"`** — yatayda dar, dikeyde uzun doğar ve esneyerek kendi
    oranına açılır.
  - **`"off"`** — anında görünür.

  Hepsi aynı kısa sürede (çeyrek saniyeye yakın) biter ve harf en sonda
  statik hâliyle birebir aynı yerde durur; emoji ve geniş karakterler
  (`漢`) tek parça olarak hareket eder.
- **`erase`** — Backspace'le sildiğiniz harf:
  - **`"iris"`** — üstünde dairesel bir diyafram merkezine doğru kapanır.
  - **`"undertow"`** — akıntıya kapılmış gibi aşağı ve imlece doğru
    çekilerek söner.
  - **`"echo"`** — büyüyerek dışa doğru dağılır ve söner.
  - **`"bleed"`** — mürekkebi dağılır: kenarları yayılıp incelirken söner.
  - **`"unravel"`** — yatay şeritlere ayrılır, şeritler yukarıdan aşağıya
    sırayla yana kayıp çözülür.
  - **`"recede"`** — yerinde küçülerek söner.
  - **`"sublime"`** — buharlaşır gibi yukarı süzülür, açılıp dağılarak söner.
  - **`"shatter"`** — parçalara kırılır; parçalar hafif dönerek dağılır,
    düşer ve söner. Aynı harf her silinişte aynı parçalara kırılmaz.
  - **`"off"`** — anında kaybolur.

  Satırın ortasından sildiyseniz sağdaki metin bugünkü gibi anında kayar,
  hayalet onun altında söner.
- Efekt imlecin üstünde, harfin kendi renginde çizilir — Backspace'ten sonra
  imleç tam silinen harfin yerine gelir ve efekt onun içinde kaybolmaz.
  Yukarıdan gelen ya da yukarı giden efektler (`drop`, `sublime`) dock'un
  üst çizgisini kısa bir an aşabilir.
- Efekt **tek tek** yazılan ve silinen harfler içindir: yapıştırma, geçmişten
  (↑) gelen satır, tamamlama ve kelime/satır silme (⌥⌫, Ctrl-U) anında olur.
- Satır dock'ta değil ızgaradayken (komut koşarken, `[shell] integration =
  "blocks"`, çok satırlı giriş) efekt yoktur.
- `cursor_motion = "snap"` ikisini de kapatır; Hareketi Azalt yazmayı
  belirmeye indirir, silmeyi kapatır (yukarıda). Ayar penceresinde (Motion)
  ezilen satır devre dışı görünür ve nedenini söyler — `smooth_scroll` da.
  Hareketi Azalt'ta Keypress satırı açık kalır: orada da açıp kapatmak bir
  fark.
- Kaydettiğiniz anda geçerli olur; o sırada süren bir efekt hemen biter.
- Tanınmayan değer (`"bounce"`, `"Fade"`, `1`, `"dissolve"`) yalnız kendi anahtarını
  etkiler (açılışta varsayılan, kayıt anında ekrandaki efekt) ve uyarı
  görünür.

### `[shell]`

```toml
[shell]
integration = "auto"
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `integration` | `"auto"`, `"blocks"` ya da `"off"` | `"auto"` | kabuğa entegrasyon kurulsun mu, ve ne kadarı |

> **Emekli anahtar:** `[shell] prompt` artık okunmuyor; yerini
> `integration = "blocks"` aldı. Dosyanızda kalması zararsız (hiçbir anahtar
> silinmez) ama bir uyarı görürsünüz — gerekçesi
> [Prompt'unuzu geri almak](#promptunuzu-geri-almak).

Entegrasyon, kabuğun terminale "prompt burada başladı, komut burada koştu, şu
kodla bitti" demesini sağlar. Bugün yalnız **zsh** için var; başka bir kabukta
(bash, fish) `"auto"` da hiçbir şey yapmaz ve terminal olduğu gibi çalışır.

- **`"auto"` (varsayılan)** — kabuk zsh ise `ZDOTDIR` bateri'nin kendi dizinini
  gösterir. O dizindeki dosyalar **sizin** başlangıç dosyalarınızı yükler,
  `ZDOTDIR`'ı özgün değerine geri koyar (yoksa siler) ve kabuğun kendi
  kancalarına işaretleri ekler. Komut geçmişiniz (`HISTFILE`) de kendi
  dizininde kalır. Aynalayabilen kabukta (bugün yalnız zsh) **dock** da açılır:
  yazdığınız satır pencerenin altına iner ve prompt'u bateri çizer.
- **`"blocks"`** — aynı kurulum, ama **giriş satırı sizin kalır**: dock
  açılmaz, yazdığınız satır ızgarada durur ve prompt'unuz (p10k, starship,
  elle yazdığınız `PS1`) olduğu gibi görünür. Komut blokları, işaretler ve
  renkler çalışmaya devam eder. bash ve fish desteği geldiğinde o kabuklar
  zaten böyle çalışacak — dock satır düzenleyicinin aynasına bağlı.
- **`"off"`** — hiçbir şey kurulmaz.
- **Dosyalarınıza yazılmaz.** Ne `.zshrc`'ye ne başka bir rc dosyasına tek
  satır eklenir; entegrasyon yalnız bir ortam değişkenidir, yani kapatmak iz
  bırakmaz.
- Başka bir aracın kurduğu **gerçek** OSC 133 işaretleri `"off"` iken de
  okunur: anahtarın anlamı "sarmalayıcıyı kurma", "işaretleri görmezden gel"
  değil.
- SSH ile uzak bir makineye geçtiğinizde orada bizim betiğimiz yoktur ve
  işaretler gelmez. Bu bir arıza değil; terminal olağan hâlinde çalışır.

**Bu anahtar öteki anahtarlar gibi kayıt anında uygulanmaz** — tek istisna
budur. Entegrasyon kabuk **doğarken** kuruluyor, dosyayı kaydettiğinizde kabuk
çoktan doğmuş oluyor: değer **sonraki oturumda** geçerli olur, açık pencere
etkilenmez.

Uygulama açılmıyorsa (bozuk bir kabuk yapılandırması yüzünden pencere hemen
kapanıyorsa) anahtarı **elle** kapatabilirsiniz; bateri'ye hiç ihtiyaç yok.
Başka bir terminalden başlayın.

Entegrasyon varsayılan olarak açık olduğu için ayar dosyanız **hiç
olmayabilir** — ayar penceresinden hiçbir şey değiştirmediyseniz ve Open
settings.toml'a hiç basmadıysanız yoktur. Önce o hâli
geçin; dosya yoksa tek komut yeter ve gerisini okumanıza gerek kalmaz:

```sh
mkdir -p ~/.config/bateri
[ -e ~/.config/bateri/settings.toml ] || printf '[shell]\nintegration = "off"\n' \
  > ~/.config/bateri/settings.toml
```

Dosya zaten varsa onu açın:

```sh
open -e ~/.config/bateri/settings.toml
```

`[shell]` bölümü varsa `integration` satırını `"off"` yapın; yoksa dosyanın
sonuna iki satır ekleyin:

```toml
[shell]
integration = "off"
```

Bölümü **iki kez** yazmayın: TOML aynı bölümün tekrarını kabul etmez ve dosya
bütünüyle okunamaz hâle gelir (başlık çubuğu bunu söyler).

- Tanınmayan değer (`"on"`, `"Auto"`, `false`) yalnız bu anahtarı etkiler
  (açılışta `"auto"`) ve uyarı görünür.

##### Prompt'unuzu geri almak

Varsayılan `"auto"` olduğu için, kurulu bir prompt'u olan herkes güncellemeden
sonra onu **göremez**: `"auto"` prompt'u terminale devrediyor ve yazdığınız
satırı dock'a taşıyor. Geri almanın yolu tek satır:

```toml
[shell]
integration = "blocks"
```

`[shell]` bölümü dosyanızda zaten varsa satırı **onun içine** ekleyin; bölümü
ikinci kez yazmayın (TOML aynı bölümün tekrarını kabul etmez ve dosya bütünüyle
okunamaz hâle gelir). Sonra yeni bir pencere açın.

`"blocks"` komut bloklarını ve işaretleri **korur** — kaybettiğiniz tek şey
dock. `integration = "off"` de prompt'u geri verir ama orantısız: entegrasyonu
büsbütün kapatır, yani blokları ve işaretleri de öldürür.

**Prompt ve dock tek karardır ve bu kasıtlı.** Bir dönem ayrı bir `prompt`
anahtarı vardı; ekranda **iki prompt** çıkıyordu (sizinki ızgarada, dock'unki
altta) ve imleç ikisi arasında sıçrıyordu. "Prompt benim olsun" demek zaten
"satır ızgarada kalsın" demek olduğu için anahtar emekliye ayrıldı.

Devir yalnız **zsh**'te oluyor. bash, fish, SSH'ın öte tarafı ve
`integration = "off"` oturumu prompt'unuzu zaten olduğu gibi gösterir.

### `[remote]`

```toml
[remote]
hosts = [
  { host = "prod-*", mark = "production" },
  { host = "*.staging.example.com", mark = "staging" },
  { host = "vm", mark = "#c678dd" },
  { host = "router*", integration = false },
]
integration = true
```

| anahtar | tür | varsayılan | anlamı |
|---|---|---|---|
| `hosts` | `{ host, mark }` dizisi | `[]` | uzak host'ların işareti: ssh ya da mosh o host'tayken dock'un renkleri |
| `preview_max_size` | boyut | `"100MB"` | uzak dosyanın önizlemesi (⌘-tık) bundan büyükse inmeden önce sorar |
| `preview_read_only` | `true` \| `false` | `true` | önizleme kopyası salt okunur (`0444`) açılır — bir ipucu, uygulama kilidi açabilir |
| `preview_dir` | klasör | `"~/Library/Caches/bateri/Previews"` | önizleme kopyalarının klasörü |
| `preview_keep` | `"launch"` \| `"1d"` \| `"7d"` \| `"30d"` | `"7d"` | önizleme son açılışından sonra ne kadar kalır; `launch` bir sonraki açılışa kadar |
| `preview_limit` | boyut | `"2GB"` | önizleme klasörünün boyut sınırı; yalnız açılışta, en eskiden başlayarak |
| `download_dir` | klasör | `"~/Downloads"` | "Download to Downloads"'un hedefi |
| `download_conflict` | `"ask"` \| `"keep_both"` \| `"replace"` | `"ask"` | hedefte aynı ad varsa: sor, ikisini de tut (yenisi numara alır) ya da üstüne yaz |
| `download_notify` | `true` \| `false` | `true` | bateri arkadayken biten aktarım bildirim gönderir |
| `stats` | `"sparkline"` \| `"numbers"` \| `"alerts"` \| `"off"` | `"sparkline"` | ssh durum çubuğunun sağındaki uzak yük göstergesinin biçimi; `off` göstergeyi ve örneklemeyi kapatır |
| `stats_interval` | tam sayı, saniye, `2`–`60` | `3` | yük göstergesinin iki örneği arası |
| `integration` | `true` \| `false` | `true` | düz `ssh` sunucuda kabuk entegrasyonunu kurar (uzak kabuk entegrasyonu, aşağıda) |

ssh ya da mosh ile uzak bir makinedeyken dock'un bağlam satırı `⇄ host`
gösterir ve üst çizgisi renklenir. `hosts` o rengi host'a göre seçer, yani
prod'da olduğunuzu renkten bilirsiniz.

- **`mark`**: `"production"` (temanın `error`'u, kırmızı), `"staging"`
  (`warning`, sarı), `"development"` (`success`, yeşil), `"none"` (işaretsiz —
  temanın `info`'su, camgöbeği) ya da `"#rrggbb"` biçiminde bir renk. Adlı
  işaretler temanın rolünden geldiği için açık/koyu temada kendiliğinden
  okunur; doğrudan renk temayla değişmez ve okunurluğunu kimse denetlemez.
- **`host`** bir desen: `*` herhangi bir karakter dizisi (boş ve nokta dahil),
  `?` tek karakter; büyük/küçük harf fark etmez. `[a-z]` ve `{a,b}` yok.
- Desen `@` taşımıyorsa host'un **son `@`'ten sonrası** ile karşılaştırılır:
  `ssh deploy@prod` de `ssh prod` de `prod` desenine uyar. `root@*` gibi `@`'li
  bir desen kullanıcı adını da sorar.
- Host, `ssh`'a **yazdığınız** addır (`ssh prod` → `prod`, `ssh
  deploy@10.0.0.5` → `deploy@10.0.0.5`); `~/.ssh/config`'in `HostName`'i
  çözülmez. Takma adla bağlanıyorsanız deseni takma ada yazın.
- **İlk eşleşen kazanır**, dizideki sırayla: tam adları geniş desenlerden önce
  yazın. `"none"` aramayı orada bitirir — bir globun yakaladığı tek bir
  host'u işaretsiz bırakmanın yolu o.
- Satır içi dizi yerine `[[remote.hosts]]` bölüm dizisi de yazılabilir.
- **Menüden**: ssh sekmesindeyken **Shell ▸ Mark “host” as ▸** Production /
  Staging / Development / None; onay işareti host'un şu anki işaretinde (bir
  desenden gelse de), yerel sekmede öğe gri. Seçim dosyaya yazılır: tam o
  host'un girdisi varsa işareti yerinde değişir, yoksa dizinin **başına**
  eklenir (önünde bir glob varsa girdi başa taşınır). **None** o host'un
  girdisini siler; bir desen hâlâ yakalıyorsa başa `mark = "none"` yazar.
  Desen host'un `user@`'siz kısmıdır. Yorumlara ve dizinin yazılışına
  dokunulmaz; dosya ayrıştırılamıyorsa ya da liste bozuksa yazılmaz.
- İşaretli bir host'un sekmesi, sekme çubuğu görünürken başlığının yanında
  işaretin renginde küçük bir nokta taşır; işaretsiz uzak sekmede nokta
  yoktur.
- Kaydettiğiniz anda geçerli olur, ssh sürerken de.
- **`integration`** (`true` \| `false`, isteğe bağlı): o host'ta uzak kabuk
  entegrasyonu (aşağıda). Girdi yalnız `integration` da taşıyabilir
  (`{ host = "router*", integration = false }`); o zaman işareti yoktur ve
  işaretin aramasına katılmaz. İki anahtar ayrı ayrı çözülür, ikisinde de
  **ilk eşleşen** kazanır: `mark` için `mark` taşıyan, `integration` için
  `integration` taşıyan ilk girdi — yani bir host'ta entegrasyonu kapatmak
  rengini almaz. Menüden işaret verilen böyle bir girdi `mark`'ını yerinde
  alır; menü bir girdiyi silerken ya da başa taşırken `integration`'ı yeni
  girdiye taşır.
- **Bozuk bir girdi** (bilinmeyen `mark`, `host`'suz girdi, ne `mark` ne
  `integration` taşıyan girdi, `true`/`false` olmayan `integration`, tablo
  olmayan öğe) listenin **tamamını** reddeder: açılışta liste boş, kayıt anında
  ekrandaki liste kalır ve uyarı görünür; uzak kabuk entegrasyonu da
  **kapanır**, çünkü listeyle birlikte prod işaretleri ve
  `integration = false` girdileri de gitmiştir. Yalnız bozuk girdiyi atmak sırayı
  değiştirip bir host'un işaretini sessizce değiştirebilirdi.

#### Uzak kabuk entegrasyonu

`integration = true` iken yerel zsh'te yazılan düz bir `ssh` sunucuda da
kabuk entegrasyonunu kurar: dizin (OSC 7) ve sonra komut blokları uzakta da
çalışır. bateri sunucuda yalnız `~/.local/share/bateri/shell/` altına birkaç
küçük dosya yazar; sunucunun rc dosyalarına dokunmaz.

- **Hangi host'ta**: `hosts`'ta `integration` taşıyan ilk eşleşen girdinin
  değeri; böyle girdi yoksa ve host'un işareti `"production"` ise **kapalı**;
  kalan her durumda bu anahtar.
  Yani prod işaretli bir host'ta açmak için girdisine `integration = true`
  yazılır.
- **İlk bağlantıdan itibaren**: dizin ve komut blokları ilk `ssh`'ta gelir.
  Sunucuda komut çalıştırmayan bir giriş kabuğu varsa (router, Windows) ilk
  bağlantıda sunucunun kendi hata satırı görünür ve bağlantı kendiliğinden
  düz açılır; bateri o sunucuyu "kabuksuz" diye hatırlar ve sonraki
  bağlantılar baştan düz açılır. Kabuklu bir sunucudan `exit` ile çıkmak
  bağlantıyı yeniden açmaz. Yerel tmux ya da screen içinden açılan `ssh`
  sarılmaz. Hatırlanan sunucular ve bateri'nin dosya yazdığı sunucular
  bateri'nin kendi dosyasında durur
  (`~/Library/Application Support/bateri/remote-hosts`), `settings.toml`'da
  değil; sunucu `~/.ssh/config`'in çözdüğü `kullanıcı@host:port` ile tanınır.
  Yanlışlıkla "kabuksuz" hatırlanan bir sunucu Shell ▸ Shell Integration on
  “{host}” öğesiyle unutturulur (öğe her tıklamada bu kaydı siler).
- **Hiç sarılmayanlar**: uzak komutlu ya da etkileşimsiz ssh (`ssh host
  komut`, `-N`, `-T`, `-W`, pipe), `scp`/`rsync`/`git`, `~/.ssh/config`'te
  `RemoteCommand`, `RequestTTY no` ya da `SessionType` taşıyan host. Bunlar
  bugünkü gibi entegrasyonsuz kalır; uzak oturum yine algılanır.
- Kabul edilmeyen değer ve okunamayan ayar dosyası entegrasyonu **kapatır**
  (`osc52` gibi): yanlış tahmin sunucuya sessizce yazmak olurdu.
- Ayar her `ssh`'ta o an okunur; açık pencereler beklemez.
- **Ayar penceresinden**: Remote Files ▸ **Set up shell integration on
  servers** bu anahtarı (`[remote] integration`) yazar; dosyayı elle
  değiştirince anahtar da değişir.
- **Menüden**: ssh sekmesindeyken **Shell ▸ Shell Integration on “host”**
  o host'un entegrasyonunu açar ya da kapatır. Onay işareti host'un şu anki
  cevabında — `integration` taşıyan girdisi, yoksa işareti (`"production"`
  ise kapalı), yoksa bu anahtar; yerel sekmede öğe gri. Seçim dosyaya o
  host'un **kendi** kararı olarak yazılır: tam o host'un girdisi varsa
  `integration` satırı yerinde eklenir ya da değişir (işaretine dokunulmaz),
  yoksa ya da önünde `integration` taşıyan bir desen varsa dizinin **başına**
  `{ host = "…", integration = … }` eklenir ve o host'un geride kalan
  girdilerinin artık etkisiz `integration`'ı silinir (yalnız onu taşıyan
  girdi bütünüyle silinir, işaretli olan işaretini korur). Desen host'un
  `user@`'siz kısmıdır; yorumlara ve dizinin yazılışına dokunulmaz, dosya
  ayrıştırılamıyorsa ya da liste bozuksa yazılmaz. Değişiklik **bir sonraki**
  `ssh`'ta geçerlidir; açık bağlantı olduğu gibi kalır.

#### Uzak yük göstergesi

ssh ya da mosh ile bir Linux sunucudayken durum çubuğunun sağı o makinenin
yükünü gösterir; veri dosya adlarının kullandığı yardımcı ssh oturumundan
gelir, yeni bağlantı açılmaz.

- **`stats`**: `"sparkline"` (varsayılan) `cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%` — son
  sekiz CPU örneği ve sayılar; `"numbers"` `cpu 23%  mem 61%`; `"alerts"`
  eşik aşılmadıkça küçük yeşil bir `●`, aşılınca yalnız aşan değerler;
  `"off"` gösterge yok, örnekleme de yok. Disk (`/`) her biçime yalnız %85'i
  geçince eklenir.
- **Eşikler** sabittir: cpu %70/%90, bellek %80/%92, disk %85/%95. Etiketler,
  grafik ve eşiğin altındaki sayılar sönük; eşiği aşan sayı temanın
  `warning`'i, ikinci eşiği aşan `error`'u ve başında `▲`.
- **Dar pencerede** önce grafik düşer, sonra yalnız en kötü değer kalır, sonra
  gösterge düşer; eşiği aşmış bir değer varsa yoldan önce gelir ve yol
  soldan `…` ile kısalır. Host hiç kısalmaz. Aktarım sürerken gösterge
  gizlidir.
- **Göstergeye tıklayınca** ayrıntı açılır: host ve işletim sistemi, CPU
  (çekirdek sayısıyla), load 1/5/15, bellek, swap, disk `/`, açık kalma süresi
  ve en çok CPU kullanan üç süreç; açıkken her örnekte tazelenir. İkinci tık,
  dışarı tık ya da Esc kapatır (Esc sunucuya gitmez).
- **`stats_interval`** iki örnek arasındaki saniye, `2`–`60` arası tam sayı.
- Kaydettiğiniz anda geçerli olur. Kabul edilmeyen değer o anahtarı açılışta
  varsayılanda, kayıt anında ekrandaki değerde bırakır ve uyarı görünür.

#### Uzak dosyalar: önizleme ve indirme

Önizleme ve indirmenin sekiz anahtarı ssh ya da mosh oturumundaki dosya adlarının ayarı:
⌘-tık dosyayı geçici, salt okunur bir kopyayla **önizler**, indirme kalıcı
kopyayı `download_dir`'e koyar.

- **Boyut** `"100MB"` gibi yazılır: tam sayı ve `B`, `KB`, `MB`, `GB`, `TB`
  birimlerinden biri (ondalık, Finder'ın birimleri; arada bir boşluk olabilir).
  Büyük/küçük harf duyarlı: `"100mb"` reddedilir. Birimsiz sayı da reddedilir,
  çünkü birimi tahmine bırakırdı.
- **Klasör** `/` ya da `~/` ile başlar; göreli yol ve `~kullanıcı` reddedilir.
- **Temizlik** açılışta (saklama süresini aşanlar ve `preview_limit`'i aşan
  kısım, en eskiden) ve günde bir kez (yalnız saklama süresi) çalışır; çıkışta
  hiçbir şey silinmez ve bateri açıkken boyut yüzünden silme yoktur. Siz
  değiştirdiğiniz için bateri'nin yazdığı hâlden ayrılan bir önizleme hiçbir
  temizlikte silinmez: `download_dir`'e taşınır ve bildirilir.
- Kabul edilmeyen değer o anahtarı açılışta varsayılanda, kayıt anında
  ekrandaki değerde bırakır ve uyarı görünür.

**Göreli adlar sunucunun OSC 7'sini ister.** `ls` çıktısındaki `backups` adı
göreli; hangi dizinde olduğunu uzak kabuğun bastığı OSC 7 söyler. Basmıyorsa
bateri pencere başlığına bakar: Debian ve Ubuntu'nun hazır `.bashrc`'si her
prompt'ta başlığı `kullanıcı@host: dizin` yapar ve o biçimdeki dizin kullanılır.
İkisi de yoksa mutlak (`/var/log/x`) ve `~/…` yollar yine çalışır, göreli adın
üstünde ⌘ basılıyken etiket nedeni söyler.
bateri sunucudaki rc dosyasına yazmaz; açmak için sunucuda tek satır yeter:

```sh
# ~/.bashrc
PROMPT_COMMAND='printf "\033]7;file://%s%s\007" "$HOSTNAME" "$PWD"'"${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
# ~/.zshrc
_bt_osc7() { printf '\033]7;file://%s%s\007' "$HOST" "$PWD"; }; precmd_functions+=(_bt_osc7)
```

## Temalar

Kullanıcı temaları şu dizinde, tema başına bir dosya:

```
~/.config/bateri/themes/{ad}.toml
```

Dosyanın adı (`.toml` olmadan) temanın adıdır ve `[appearance] theme`'e
yazılan budur. Uyarılar dosyanın adıyla gelir:

```
bateri – themes/paper.toml: line 3: `ansi.red` must be a color like "#rrggbb", found "red"; using #d16d6a
```

### Biçim

On iki rol kökte, 16 ANSI rengi `[ansi]` bölümünde. Renk `"#rrggbb"` biçiminde
bir metindir (büyük harf de olur; `#rgb` ve alfa yok).

| anahtar | anlamı |
|---|---|
| `background` | varsayılan arka plan, pencerenin zemini |
| `foreground` | varsayılan ön plan |
| `dim` | sönük (SGR 2) yazılmış varsayılan ön plan |
| `accent` | vurgu; **koşan** komutun işareti |
| `cursor` | imleç bloğunun rengi |
| `selection` | fareyle seçimin vurgusu; seçili metin kendi renginde kalır, odakta olmayan pencerede vurgu zemine doğru soluklaşır |
| `search_match` | geçmişte aramanın (⌘F) bütün eşleşmelerinin vurgusu; metin kendi renginde kalır, odakta olmayan pencerede soluklaşır |
| `search_current` | geçerli eşleşmenin vurgusu — ⏎/⌘G'nin gösterdiği; `search_match`'ten belirgin, seçim ikisinin de üstünde çizilir |
| `success` | durum: başarı; sıfır çıkış koduyla biten komutun işareti |
| `error` | durum: hata; sıfırdan farklı çıkış koduyla biten komutun işareti |
| `info` | durum: bilgi; uzak oturum — bağlam satırında host ve dock'un üst çizgisi |
| `warning` | durum: uyarı; `staging` işaretli uzak host — bağlam satırında host ve dock'un üst çizgisi |
| `[ansi]` `black` `red` `green` `yellow` `blue` `magenta` `cyan` `white` | ANSI 0–7 |
| `[ansi]` `bright_black` … `bright_white` | ANSI 8–15, aynı sırada |

- **Her anahtar opsiyoneldir.** Eksik anahtar gömülü `bateri` temasından
  gelir; yalnız zemini değiştiren iki satırlık bir dosya geçerli bir temadır.
- Gömülü bir temayı **gölgeleyen** dosyada (`themes/bateri-light.toml`) eksik
  anahtar o gömülü temanın kendisinden gelir; gölgelemeyen bir adın tabanı
  `bateri` kalır.
- **Kuralın istisnası yok**, `cursor`'ın da: yalnız `accent` yazan bir dosyada
  imleç gömülü temanın imleci olarak kalır. İmlecini ayırmak isteyen `cursor`
  yazar — yirmiden fazla anahtar içinde tek bir anahtarın başka davranması,
  kazandırdığından çok şaşırtırdı.
- Kabul edilmeyen renk (`"red"`, `"#12345"`, sayı) tabanın (`bateri` ya da
  gölgelenen gömülü tema) değerini alır ve uyarı verir; öteki renkler yine
  okunur.
- **Boş dosya** (ya da yalnız boşluk) kullanılamaz sayılır: çoğu editör
  kaydederken dosyayı önce boşaltır ve kaydın ortasında pencere tabana
  çakmamalı. Kaydettiğiniz anda ekrandaki tema kalır, açılışta görünüme uyan
  gömülü tema gelir; ikisinde de uyarı çıkar.
- Tanınmayan anahtar sessizce yoksayılır.
- **Sönük metin** (SGR 2) iki yoldan gelir. Varsayılan ön plan sönükse
  temanın `dim` rengi kullanılır. Adlı ve 256 renkli metnin sönüğü ise bir
  kuraldır: renk temanın `background`'una doğru üçte bir yol alır — koyu
  temada koyulaşır, açık temada açılır. Siyah zeminde bu, alacritty'nin ve
  vte'nin "rengin üçte ikisi" kuralıyla aynıdır.
- `dim`, `success`, `error` ve `selection` de her anahtar gibi eksikse
  `bateri`'den gelir: açık bir temada `dim` yazılmazsa sönük varsayılan metin
  koyu temanın grisiyle (`#909093`), `success`/`error` yazılmazsa komut
  işaretleri koyu temanın yeşil ve kırmızısıyla, `selection` yazılmazsa seçim
  koyu temanın arduvazıyla (`#283042`) çizilir — açık bir temada koyu metnin
  altında zor okunur, yani açık bir tema `selection`'ını yazmalı. Aynısı
  `search_match` ile `search_current` için: yazılmazlarsa arama vurgusu koyu
  temanın koyu sıcak tonlarıyla (`#3a3212`, `#503a0c`) gelir. `info`
  yazılmazsa uzak oturumun host'u ve üst çizgisi koyu temanın camgöbeğiyle
  (`#79b3b3`), `warning` yazılmazsa `staging` işaretli host koyu temanın
  sarısıyla (`#d6b16a`) çizilir.

### Gömülü `bateri`

Bir temaya başlamanın en kısa yolu bunu kopyalayıp değiştirmek:

```toml
background = "#000000"
foreground = "#d8d9dd"
dim = "#909093"
accent = "#7a9cc6"
cursor = "#d9b063"
selection = "#283042"
search_match = "#3a3212"
search_current = "#503a0c"
success = "#8bb58b"
error = "#d16d6a"
info = "#79b3b3"
warning = "#d6b16a"

[ansi]
black = "#22252b"
red = "#d16d6a"
green = "#8bb58b"
yellow = "#d6b16a"
blue = "#7a9cc6"
magenta = "#b08ec0"
cyan = "#79b3b3"
white = "#c8c9cc"
bright_black = "#4a4e57"
bright_red = "#e58b88"
bright_green = "#a4cba4"
bright_yellow = "#e8c988"
bright_blue = "#9bb8dc"
bright_magenta = "#c9aad8"
bright_cyan = "#96caca"
bright_white = "#e6e7ea"
```

### Gömülü `bateri-light`

Açık görünümün varsayılanı. Açık zeminde okunur kalsın diye sarı ve
camgöbeği koyu, doygun tonlarda; beyaz (`white`, `bright_white`) adının
anlamını korur ve açık uçta durur.

```toml
background = "#f5f6f8"
foreground = "#24262c"
dim = "#696b70"
accent = "#3d6aa8"
cursor = "#8a6512"
selection = "#dde6f3"
search_match = "#f9f1d2"
search_current = "#fee29a"
success = "#3b7a3b"
error = "#b5423d"
info = "#23787f"
warning = "#8f6a00"

[ansi]
black = "#2b2e35"
red = "#b5423d"
green = "#3b7a3b"
yellow = "#8f6a00"
blue = "#3a66a6"
magenta = "#8a4c9c"
cyan = "#23787f"
white = "#b9bbc1"
bright_black = "#70737b"
bright_red = "#c9504a"
bright_green = "#4a8f4a"
bright_yellow = "#a67c00"
bright_blue = "#4a78ba"
bright_magenta = "#9d5db0"
bright_cyan = "#2f8a92"
bright_white = "#dcdee3"
```

İki blok da bir sınamayla gömülü temasına bağlıdır
(`documented_blocks_are_the_embedded_themes`): gömülü tema değişip blok
değişmezse sınama düşer.
