# Settings

bateri's user settings live in a single TOML file, and color themes in
separate files. This document is the **single owner** of the keys, the theme
format, the defaults and what happens when a file is broken; its counterpart
in the code is `crates/bt-core/src/settings.rs` and `theme.rs` (parsing),
`crates/bt-shell-common/src/settings.rs` (reading and resolving the theme
name), `watch.rs` (watching the files), `zoom.rs` (temporary font size),
`crates/bt-shell-macos/src/menu.rs` (the View menu: theme choice and temporary
font size), `clipboard.rs` and the `ShellWake` of `app.rs`
(the OSC 52 slot, the hop to the main queue and the write to the clipboard),
`crates/bt-atlas/src/font.rs` (finding the font family),
`crates/bt-gpu/src/motion.rs` and `link.rs` (applying cursor motion and Reduce
Motion; the place where the three-valued setting comes down to a single `bool`
is `resolve_reduce_motion` in `crates/bt-shell-macos/src/app.rs`),
`crates/bt-shell-common/src/child.rs` (which shell runs, where the wrapper
script is), `shell_integration_env` in `crates/bt-shell-macos/src/app.rs`
(whether shell integration is set up, and with which environment),
`crates/bt-shell-common/src/jobs.rs` and `crates/bt-shell-macos/src/window.rs`
(close confirmation: what is running in the foreground, when to ask);
the script itself is in `assets/shell/zsh/`.

## Where the file lives

```
~/.config/bateri/settings.toml
```

If the file does not exist, everything runs with its default and no warning
appears. **bateri ▸ Settings…** (Cmd ,) opens the settings window; its **Open
settings.toml** button opens the file in an editor, creating it first if it
does not exist (see [Settings…](#settings-1)). Creating the directory and the
file by hand works too. The file may be a symbolic link to somewhere else (a
dotfiles repository); the link's target is read.

The app writes to this file from four places: **Open settings.toml** creates
the template when the file does not exist, choosing a theme from **View ▸
Theme ▸** writes the `[appearance] theme` line (see [View ▸ Theme
▸](#view--theme-)), **Shell ▸ Mark “host” as ▸** and **Shell ▸ Shell
Integration on “host”** write that host's entry in `[remote] hosts` (see
[`[remote]`](#remote)), and the **settings window** writes the line of the
setting you changed. No other line of an existing file — comments, order,
keys it does not recognize — is touched.

A change takes effect **the moment you save** — in the settings file and in
the file of the theme in use alike; the shell and the program inside it keep
running. The one exception is [`[shell] integration`](#shell): the shell has
already been started, so that key takes effect in the next session. It does
not matter how your editor saves (writing in place, truncating, a temporary
file moved over the original, writing to the target of a symbolic link). The
system's light/dark appearance is also followed live: while the theme is
`"system"` (the default), changing the appearance in System Settings changes
the window too.

What is watched is the `~/.config/bateri/` directory. If that directory **does
not exist at all** while the app is open, creating it from the shell does not
start the watch: changes are seen when the app is reopened or when a setting is
changed from the window (or Open settings.toml is pressed), and from then on
they are watched as you save. A directory the window creates is watched right
away.

### Settings…

The shortcut is Cmd `,` on a US-layout keyboard. macOS places menu shortcuts
according to the keyboard layout; on a Turkish Q keyboard the same key
carries a different letter, and the menu shows that letter instead. View ▸
Bigger also shows as `⌘:` on that layout.

bateri ▸ Settings… (Cmd ,) opens the settings window: five categories on the
left (General, Appearance, Cursor, Motion, Remote Files), the settings on the
right. Remote Files shows `[remote] integration` at the top ("Set up shell
integration on servers"; see [Remote shell
integration](#remote-shell-integration)), then the eight preview/download keys
of `[remote]` and the two keys of the load indicator (`stats`,
`stats_interval`; eleven keys in all); on folder rows Change… opens a folder
picker (a folder under your home directory is written as `~/…`), on the
preview folder Show in Finder opens it, and the "In use" row shows the total
size of the copies in the folder — Clear Now deletes the previews at once (a
copy you changed is not deleted, it is moved to the download folder). The
window only writes to `settings.toml`; what applies it to the screen is the
same path that runs when you save the file, so a change made from the window
also takes effect at once.

- **When it writes:** pop-up menus and switches the moment you pick, sliders
  when released, number fields on Enter or when you leave the field (the
  stepper on every click). Input that is not accepted (letters, out of range)
  is not written, and the field goes back to the value in the file. Lowering
  `scrollback` trims the history at that moment — just as in the file (see
  [`[terminal]`](#terminal)).
- **It writes only the line you changed**, without comments; if the key is not
  in the file, it is added to its section. If the file does not exist, it is
  first created from the template below. Opening the window does not create
  the file.
- **When the file changes from outside, the window changes too** — a value you
  saved in the editor, a new theme you put in `themes/`.
- **A value that is not accepted** is shown in orange under its own row; the
  control shows the value currently in effect, and picking a new value fixes
  the row. A diagnostic that does not belong to a row (for example the retired
  `shell.prompt`) appears in the strip at the top of the window.
- **When the file cannot be parsed or read, the window locks:** all controls
  are disabled, the strip gives the reason with the same text as the title
  bar, and Open settings.toml becomes the default button (Enter). The window
  does not write to a broken file — the half-finished work in it is yours; the
  lock lifts once you fix and save it.
- **A file that cannot be written** (permissions) is reported in the strip and
  in the title bar, and the control goes back to the value in the file.
- Not shown in the window: the temporary font size (Cmd +/−; Size is the
  file's value) and settings that do not exist yet.

**Open settings.toml** opens the file: first with the app that opens `.toml`
files, otherwise with the default text editor (TextEdit on most machines). If
the file does not exist, it creates the directory and the file from the
template below. The template changes nothing: every key that has a default is
written with its default value, so changing a value in place and saving is
enough.

- It **does not touch** an existing file — even if it is broken, a symbolic
  link to somewhere else, or a link without a target.
- If the file cannot be created (no permission, `~/.config/bateri` is a file)
  or no app can open it, this is reported in the window's strip and in the
  title bar; in the second case the file's path is shown too.
- The values in the template are the defaults of the day it was created: if a
  later version changes a default, this file keeps the old value. Deleting the
  line returns the key to the current default.

### Template

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
# "all" | "layout" | "off". What comes back when bateri opens again: all
# brings back the windows, tabs and splits, with each pane's scrollback after
# a quit or a restart; layout brings back the windows without the scrollback
# (nothing you saw is written to disk); off writes nothing and deletes what
# was saved. A pane whose program was kept running (keep_running) comes back
# with it under every value; the others start a new shell, or with off do not
# come back.
restore_windows = "all"
# "update" | "crash" | "quit". When the programs running in bateri (vim, a
# build, a session over ssh) outlive it: update keeps them only across an
# update, crash also when bateri crashes or is forced to quit, quit also when
# you quit it — then quitting asks nothing, and Quit and End Programs (⌥⌘Q)
# ends them. The next bateri takes them back with their screens and
# scrollback; a pane already open when you switched away from update comes
# back with its program redrawing the screen. Restarting the Mac or logging
# out ends them.
keep_running = "crash"

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
# production machine is never mistaken for another. A database client (psql,
# mysql, redis-cli, mongosh) is colored by its server's host the same way.
# Each entry names a host pattern and a mark: "production" (red), "staging"
# (yellow), "development" (green), "none" (no mark), or a color like
# "#c678dd". In a pattern * stands for any run of characters and ? for one,
# ignoring case; a pattern without @ matches the host after any user@. The
# first entry that matches wins, so put exact names before wide patterns;
# "none" stops the search. Shell > Mark "host" as writes the entry for the
# host of the ssh tab or the database client you are in.
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

A test ties this block to the template (`documented_template_is_the_template`).

### View ▸ Theme ▸

The menu is rebuilt every time it opens:

- **Match System** — `theme = "system"`: the theme follows the macOS
  appearance (`light_theme` / `dark_theme`).
- Built-in themes: `bateri`, `bateri-light`.
- Every `{name}.toml` under `~/.config/bateri/themes/`, by its name. Putting a
  new file in the directory updates the menu the next time it opens. Files
  starting with a dot and `system.toml` are not listed; a file that shadows a
  built-in theme (`themes/bateri.toml`) is not listed separately, the built-in
  name's item selects it.

The checked item is the `theme` value in the settings file.

Choosing an item **writes the theme to the settings file** and applies it at
once, as if the file had been saved; the same theme comes back when the app is
reopened. The only thing written is the value of the `[appearance] theme`
line:

- Comments, blank lines, the order of keys, unrecognized keys and the comment
  next to the line stay where they are.
- `light_theme` and `dark_theme` do not change: picking a fixed theme and then
  going back to Match System brings back the light/dark pair.
- If there is no `[appearance]` section, it is added at the end of the file;
  if there is no `theme`, inside the section. An inline
  (`appearance = { … }`) or dotted (`appearance.theme = …`) spelling is
  preserved.
- If the file does not exist, it is first created [from the
  template](#template), then the line is written.
- If the file is a symbolic link, its **target** is written, and the link
  stays a link.
- Line endings are preserved: a file whose first line ends with a Windows line
  ending (CRLF) stays CRLF. If the last line has no line ending, one is added.

Cases where the file is **not written** — its contents stay as they are and
the title bar gives the reason (`…; the theme was not saved`):

- the file is invalid TOML or cannot be read (permissions, a symbolic link
  without a target);
- `appearance` is not a section (`appearance = 1`, `[[appearance]]`) or
  `theme` is a section (`[appearance.theme]`): writing over it would delete
  its contents.

The warning goes away with the next successful choice, or once the file is
saved readable and valid.

## If something goes wrong

The error appears in English in the window's title bar, next to the title:

```
bateri – settings.toml: line 2: `terminal.scrollback` must be an integer, found a string; using 10000
```

If there are several errors, the first one and the number of the rest are
shown (`(+2 more)`); all of them are also printed to standard error with the
prefix `bateri:`. The terminal opens in every case.

**In full screen** the title bar is hidden and the warning may become
invisible with it (not tried); what remains is opening the file in windowed
mode or reading the copy on stderr.

| at launch | result |
|---|---|
| no file | defaults, no warning |
| file cannot be read (permissions, non-UTF-8 content, not a regular file, a symbolic link without a target) | defaults, except `osc52` and `[remote] integration` are **off** and `restore_windows` is **`"layout"`**; warning |
| invalid TOML | **all** settings default, except `osc52` and `[remote] integration` are **off** and `restore_windows` is **`"layout"`**; the warning shows the line |
| a key's value is not accepted | only that key falls back to its default (or its limit; off for `osc52` and `[remote] integration`, `"layout"` for `restore_windows`), warning |
| unrecognized key or section | silently ignored |
| the chosen theme cannot be found | the built-in theme matching the appearance (`bateri` in dark, `bateri-light` in light), warning |
| the theme file cannot be read, is empty or is invalid TOML | the built-in theme matching the appearance, warning (a built-in theme with the same name is **not** used) |
| a color in the theme file is not accepted | only that color comes from `bateri`, warning |
| the font family cannot be found | the default font (SF Mono, otherwise Menlo), warning |
| the font family is not monospaced | the family is still used, warning |

The theme rule is the same when the appearance changes: if the new
appearance's theme cannot be used, the built-in theme matching that appearance
comes in — the window does not stay in the other appearance's theme.

**At the moment you save**, on the other hand, the rule protects your editing:
a half-finished save does not break the screen, a warning appears, and the
warning goes away when you fix and save the file.

| when saving | result |
|---|---|
| the settings file is invalid TOML or cannot be read | **no setting changes**, warning |
| the settings file was deleted or emptied | settings do not change, no warning; the defaults come when the app is reopened |
| a key's value is not accepted | that key **does not change**, warning; a `scrollback` above the ceiling comes down to the ceiling, an unaccepted `osc52` or `[remote] integration` **turns off**, `restore_windows` becomes **`"layout"`** |
| a key was deleted from the file | that key returns to its default |
| the chosen theme cannot be found, its file cannot be read, is empty or is invalid TOML | **the theme on screen stays**, warning |
| the font family cannot be found | the default font, warning; fixing the name and saving clears the warning |

When choosing a theme from the menu, if the file is invalid or cannot be read,
the file is **not written**; see [View ▸ Theme ▸](#view--theme-).

A deleted file not changing the settings is deliberate: many editors move the
old file aside for a moment while saving, or empty it first and then write it;
falling back to the defaults would make the window flash on every save and,
if `scrollback` had been raised, delete the extra history. A value that is not
accepted leaving its key alone is deliberate too: if saving `scrollback` as
text by mistake fell back to the default, the extra history would be deleted
at that moment.

"Invalid TOML" is wider than a syntax error: writing the same key twice, or a
number above TOML's integer limit (9 223 372 036 854 775 807), also makes the
whole file invalid.

An unrecognized key staying silent is deliberate: the current version should
not report the keys of later versions as errors.

`osc52` being outside the rule is deliberate too: when the file cannot be
read, or the value is misspelled (`"of"`), there is no telling whether you
turned it off, and while a wrong guess on the other keys is visible on screen,
here it is not — a remote program could write to the clipboard silently.
Falling to off can be undone: fixing the file and saving is enough.
`restore_windows` falls to `"layout"` by the same rule: the windows still come
back (the visible half), and the history is not written to disk (the
invisible half).

`keep_running` follows the ordinary rule — a value that is not understood is
`"crash"` at launch and leaves the value in effect when you save — and no
fallback ever makes it `"quit"`: had a typo made ⌘Q keep the programs, you
would believe they had ended.

## Keys

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
keep_running = "crash"
```

| key | type | default | meaning |
|---|---|---|---|
| `scrollback` | integer, `0`–`100000` | `10000` | number of lines kept in history |
| `cursor` | `"block"` \| `"underline"` \| `"beam"` | `"block"` | the cursor's **default** shape |
| `cursor_blink` | `"auto"` \| `"on"` \| `"off"` | `"off"` | whether the cursor blinks |
| `cursor_radius` | decimal, `0.0`–`0.5` | `0.10` | roundness of the cursor's corners, as a fraction of the cell's **height** |
| `cursor_glow` | decimal, `0.0`–`3.0` | `1.0` | strength of the shadow around the cursor; `0` turns it off |
| `cursor_unfocused` | `"hollow"` \| `"solid"` | `"hollow"` | the cursor while the window is not focused: `hollow` empties it, `solid` leaves it alone |
| `cursor_blink_interval` | decimal, `0.05`–`5.0` | `0.5` | **half** the blink period, in seconds |
| `confirm_close` | `"never"` \| `"running"` \| `"always"` | `"running"` | when to ask while closing a tab, a window or the app |
| `restore_windows` | `"all"` \| `"layout"` \| `"off"` | `"all"` | what comes back when bateri opens again |
| `keep_running` | `"update"` \| `"crash"` \| `"quit"` | `"crash"` | when the running programs outlive bateri |

- A value above `100000` becomes **`100000`** and gives a warning. The limit
  is alacritty's own setting limit (`MAX_SCROLLBACK_LINES`); it is not a
  measured memory budget.
- A negative or non-integer value (`"lots"`, `1.5`) falls back to the default
  and gives a warning.
- `0` is valid: no history is kept.
- When the value changes while the app is open, it applies **at once**:
  lowering it deletes the extra lines at that moment, and raising it later
  does not bring back what was deleted. In an editor that saves by itself as
  you type, an intermediate value (`100000` → `1`) is a save too.

`cursor` only sets the **default**: the application in the terminal can
change the shape with DECSCUSR (`\e[5 q`) and that request is honored — if vim
asks for a beam in insert mode, you get a beam, and it goes back to a block on
the way out. The value here is the state when nobody asks for anything. An
unrecognized value (`"bar"`, `"Block"`) **does not change** the key and gives
a warning; it is case-sensitive. The thickness of the thin shapes comes from
the font's own underline metric, so when the size or the font changes, the
caret changes with it.

`cursor_blink` has three values because there are two separate questions:
`"auto"` **blinks and the application can stop it**, `"on"` always blinks,
`"off"` never blinks — the last two **override** what the application says.

In `"auto"`, the application's request **ends when it leaves the alternate
screen**: both the shape and the blinking of the cursor go back to the
`[terminal] cursor` + `cursor_blink` base. Without this, `"auto"` in practice
meant "until the first full-screen application", and the cause is not
DECSCUSR but terminfo: in `xterm-256color`,
`cnorm = \e[?12l\e[?25h`, that is, the "make the cursor normally visible"
command carries **inside it** private mode 12, which turns blinking off. vim,
less, man, htop — every program that sends `cnorm` kills blinking on exit and
nobody turns it back on (measured on 2026-09-20: a whole `vim -u NONE` session
is 160 bytes, contains `\e[?12h` and `\e[?12l`, and no DECSCUSR at all).

The base of `"auto"` is deliberately **on**: if it were off, since nothing asks
for blinking (neither zsh nor our wrapper sends DECSCUSR), on a plain prompt it
would be exactly the same as `"off"` and two of the three values could not be
told apart.

The default is `"off"`, and that is a product decision: a blinking cursor
keeps the window permanently busy (two frames a second), and this terminal's
main promise is drawing no frames at all while idle. When you turn it on, the
cost stays limited — if the window has drawn nothing for 15 seconds, blinking
**stops** and the cursor stays visible; it comes back with the first output or
keystroke. The timer looks at **drawing**, not at the keyboard: streaming
output such as `tail -f` keeps blinking alive.
With Reduce Motion on, blinking never starts: an accessibility setting does
not *add* animation.

`cursor_radius` and `cursor_glow` scale the cursor's **appearance**; they do
not define a new measure: the radius is a fraction of the cell's **height**,
and `cursor_glow` is a multiplier of the design's own shadow size — `1.0` is
the default look, `0` turns the shadow off. Both grow with the font size, so
Cmd +/− keeps the cursor in proportion. The shadow is **a single number**, not
spread and darkness separately: the two are one feeling, and given separately
they would allow meaningless states such as "a wide halo of nothing".

`cursor_unfocused` says what happens to the cursor while the window is not
focused: `"hollow"` empties it into an outline, `"solid"` leaves it alone. It
has **no effect** on blinking — in an unfocused window blinking always stops;
that is a separate signal.

`cursor_blink_interval` is **half** the blink period: the cursor stays lit
this long, and dark this long. The cost of shortening it is linear — `0.25`
asks for four frames a second — and the lower limit (`0.05`) caps it there. A
wrong value for this key is **not caught** by bateri's own smoke test: the
timed run never reads the settings file and blinking is off by default, so the
only protection is the accepted range itself.

`confirm_close` says whether to ask before closing. The question is **one**
sheet for that tab on ⌘W and the tab bar's × button, for all of the window's
tabs on the red button and ⇧⌘W, for the other tabs on "Close Other Tabs", and
**one** alert for all windows on ⌘Q (including Dock ▸ Quit, logging out and
restarting); it lists the running programs by name; Return closes, Esc
cancels.

- `"running"` (the default) asks only while a program **other than** the shell
  is in the foreground: vim, `ssh`, Claude Code, an ongoing build. `ssh`
  counts, because closing also ends the remote session. At an empty prompt the
  tab closes without asking.
- `"always"` asks at an empty prompt too; `"never"` never asks.
- Typing `exit` in the shell asks under **no** value: the one asking to close
  is the shell itself.
- Not counted: background jobs (`sleep 100 &` — zsh warns about those itself
  on `exit`), a loop running inside the shell itself, and a program that
  replaces the shell (`exec vim`). All three look as if the shell were idle.

The value is read at closing time, so it takes effect the moment you save.

`restore_windows` says what comes back when bateri is closed and reopened —
⌘Q, an update, logging out, restarting:

- `"all"` (the default) brings back the windows, the tabs (their group, order
  and selected tab), the splits (direction and ratio), the focused and the
  zoomed pane, each pane's directory and font size difference **and its
  history** with colors and formatting, with the success/error marks of
  finished commands (their durations do not come back).
- `"layout"` brings back the same layout without the history: nothing you saw
  on screen is written to disk. A history saved earlier with `"all"` is not
  shown either; it is deleted unread.
- `"off"` writes nothing and deletes the rest: bateri opens with a single
  empty window — or, when programs were kept running (`keep_running`), with
  just their panes.
- After ⌘Q, logging out and restarting, the shells are **new** — unless
  `keep_running` (below) kept their programs: running programs (vim, a build)
  end on closing, and the history that comes back is text only. The history
  is written only on a clean shutdown; the layout (windows, tabs, splits,
  directories) is also kept up to date on disk while bateri runs, without
  the history, so after a crash or a power cut the windows come back under
  every `keep_running` value: with their programs where they were kept, with
  new shells where nothing kept them. After a crash a kept program comes back
  with its screen and history as they were at the crash.
- **Updates are the exception**: on "Install and Relaunch" the shells and the
  running programs — vim, `npm run dev`, a session over ssh — **do not die**,
  they move to the new version with their screens; on this path the history
  is not written to disk either, it is carried in memory. This holds under
  every `restore_windows` value, `"off"` included: whether programs live on
  is `keep_running`'s to say, and every value of it covers updates. While a
  file transfer or a password prompt is in progress, the relaunch waits for
  them to finish ("Update waits for N transfers" on the status line; ⌘.
  cancels the transfer and lets the update proceed). A pane whose program
  cannot be carried over comes back with a new shell and a dim note saying
  why — under its history with `"all"`, alone with `"layout"`; with `"off"`
  it does not come back.
- **A pane in ssh** comes back with a local shell in its old local directory,
  and the connection's line (such as `ssh prod`) waits on the input line
  **ready but not run**: ⏎ connects. It does not connect by itself — after an
  update you do not want every pane asking for a password at the same time,
  or a production server being connected to without you touching it.
- The saved state is under
  `~/Library/Application Support/bateri/session/dev.bateri.bateri/` (the
  directory is yours only, the files are `0600`) and is deleted as soon as it
  is read at launch. The history sits there as **plain text** — including a
  token or a password printed on screen — and the directory is in a place Time
  Machine backs up; a copy backed up while bateri was closed stays there. If
  you do not want that, use `"layout"`.
- If two copies of bateri are open at the same time, only the first one saves
  and restores.
- Holding **⇧** while bateri opens skips all of this: one empty window opens,
  nothing saved is read, and from then on this session is the one that gets
  saved. Programs kept through a crash or a quit (`keep_running`) are left
  alone and come back at the next launch; programs crossing an update still
  come back, since they cannot wait.
- If bateri crashes while it brings programs back, the next launch brings
  them back without their screens, and if that crashes too, the one after
  gives them up and opens a single empty window.

`keep_running` says when the programs running in bateri — vim, `npm run
dev`, a session over ssh — outlive it. Each value includes the ones before
it:

- `"update"` keeps them only across "Install and Relaunch" (above); ⌘Q, a
  crash and a forced quit end them.
- `"crash"` (the default) also keeps them when bateri crashes or is forced to
  quit (Activity Monitor, `kill -9`): they keep running without a window and
  the next bateri takes them back into their windows, the same processes,
  with the screen and the history as they were at the crash and whatever the
  programs printed while bateri was closed below. ⌘Q still ends them, after
  the usual question. A pane that was already open when you moved away from
  `"update"`, or whose record could not be kept (below), comes back with its
  program redrawing the screen instead: the pane says so in a dim line, a
  full-screen program (vim, htop) is made to redraw at once, and in a plain
  shell what was on screen is gone.
- `"quit"` also keeps them when you quit with ⌘Q, which then asks nothing:
  the programs run on with no window until bateri opens again, and then they
  come back with their screens. So that they are not forgotten, a quit that
  leaves a program running (not an idle shell) shows a macOS notification a
  moment after bateri is gone, naming them ("“vim” and “npm” keep running in
  the background…"); clicking it opens bateri, which brings them back. The
  notification needs permission, asked when you choose `"quit"` (or at the
  first launch that finds it); without it the reminder is the line under the
  setting in Settings. There is no notification when the Mac logs out or
  restarts — the programs end with it.
- With `"quit"`, holding ⌥ turns bateri ▸ Quit bateri into **Quit and End
  Programs** (⌥⌘Q): the quit that ends the programs, after the usual
  question (`confirm_close`), as ⌘Q does under the other values.
- If bateri is not opened again, kept programs run until they finish, or
  until you log out or restart the Mac; the helper exits once none is left.
  While bateri is closed their output is still read, so a server that logs
  does not stall — a program that prints without end keeps using the CPU, and
  past a limit the oldest output is dropped (the pane marks the cut when it
  comes back).
- Under every value, closing a pane, a tab or a window (⌘W, the red button)
  and typing `exit` end the programs as before, and restarting the Mac or
  logging out ends them.
- With `"crash"` and `"quit"` a small helper process (`bateri hold`) runs
  beside bateri and keeps a copy of each terminal, so the programs have
  somewhere to stay when bateri goes. While bateri runs it reads nothing and
  wakes for nothing; when no program is left for it to keep, it exits.
- So that the screen survives a crash, bateri keeps a record of what each
  pane printed since it last summarized it, in memory shared with the helper
  — never on disk — and summarizes it from time to time on a thread of its
  own; the record's room grows with `scrollback`. After a crash the helper
  rebuilds each screen from the summary and the record with `bateri compact`. A program that prints without pause for a long time may
  be slowed down to the pace of the summaries; if a summary cannot keep up,
  that pane's record is dropped and a crash brings it back redrawn by its
  program.
- A change applies when you save, in every direction, without restarting
  bateri: moving away from `"update"` protects the programs that are already
  running from then on, moving back to it lets them end with bateri again,
  and between `"crash"` and `"quit"` only the next ⌘Q reads the difference.
- In Settings it is General ▸ "Keep programs running:" — Only during
  updates, Also after a crash, Also after quitting.

The section can also be written inline: `terminal = { scrollback = 5000 }`.
`[[terminal]]` (an array of sections) does not count as a section and gives a
warning.

### `[appearance]`

```toml
[appearance]
theme = "system"
light_theme = "bateri-light"
dark_theme = "bateri"
```

| key | type | default | meaning |
|---|---|---|---|
| `theme` | `"system"` or a theme name | `"system"` | the color theme to use |
| `light_theme` | theme name | `"bateri-light"` | the light appearance's theme while `theme = "system"` |
| `dark_theme` | theme name | `"bateri"` | the dark appearance's theme while `theme = "system"` |

- `theme = "system"` leaves the theme to the macOS appearance: `light_theme`
  in light, `dark_theme` in dark. When the appearance changes, the theme
  changes at once.
- `theme = "{name}"` is a fixed theme independent of the appearance;
  `light_theme` and `dark_theme` are not read meanwhile but stay in place —
  going back to `"system"` brings the pair back.
- `"system"` is not a theme name: `themes/system.toml` cannot be chosen, and
  `light_theme`/`dark_theme` do not accept this value (they fall back to their
  own default and give a warning).
- A name is first looked up as `~/.config/bateri/themes/{name}.toml`,
  otherwise among the built-in themes. There are two built-in themes: `bateri`
  (dark) and `bateri-light` (light).
- A file with the same name **shadows** the built-in theme: a user who writes
  `themes/bateri.toml` sees their own file, not the built-in `bateri`.
- A name found nowhere falls back to the built-in theme matching the
  appearance (`bateri` in dark, `bateri-light` in light) and gives a warning;
  both at launch and when the appearance changes.
- An empty name or a name containing `/` (`"../x"`) is not accepted; it falls
  back to the key's default and gives a warning: themes are not read from
  outside the `themes/` directory.

### `[font]`

```toml
[font]
family = "Menlo"
size = 13
line_height = 0.9
letter_spacing = 1.0
```

| key | type | default | meaning |
|---|---|---|---|
| `family` | text | none (SF Mono, otherwise Menlo) | font family |
| `size` | number, greater than `0` | `13` | size in points |
| `line_height` | number, `0.5` – `2` | `1.0` | line spacing multiplier |
| `letter_spacing` | number, `0.5` – `2` | `1.0` | letter spacing multiplier |

- `family` is a **family name**, the name shown in Font Book (`"JetBrains
  Mono"`, `"Menlo"`); case does not matter. The PostScript name of a single
  face (`"Menlo-Regular"`) does not count as a family and gives a "not found"
  warning.
- If the family is not on the machine, the default font is used and the title
  bar says so: `font "Fira Code" not found; using Menlo`. Fixing the name and
  saving changes the font and clears the warning.
- **The default font is a two-link chain:** first SF Mono, otherwise Menlo.
  SF Mono comes with Xcode and is **not on every machine**; if it is not
  installed, no warning is given either, because this is not a fault but a
  designed fallback — both are monospaced. "SF Mono, otherwise Menlo" in the
  table says exactly this: if you wonder which one is on screen, it is the
  face you see when you do not write `family` at all.
- A family that is not monospaced (`"Helvetica"`) is **not rejected**, it
  gives a warning: `font "Helvetica" is not monospaced; text may not line up`.
  The cell width comes from the space character; letters wider than that are
  clipped to the cell.
- **A character the chosen font lacks comes from the system.** As long as it
  fits in one cell, it is drawn from macOS's own font chain; you do not need
  to change your font or define a fallback list. The measure is the
  character's **advance**: if it is wider than the cell, it is not drawn and
  **stays a box**. This is deliberate — emoji, CJK (`漢`), powerline
  separators (`U+E0B0`) and Braille (the `⠋⠙⠹` of spinners) exceed the cell's
  width, and a half-drawn glyph would be a silent corruption; a box is a
  visible gap, that is, it tells you what could not be drawn. Drawing these
  characters properly is a separate piece of work.
- The measure being the **advance** has a cost: a glyph that advances narrow
  but paints wide passes the gate and its overflow is clipped. No such
  character has been met in today's font chain, but the promise is not "never
  clipped" but "whatever does not fit by its advance is never drawn".
- **Box and block drawing characters are outside this** (`─ │ ┌ █ ▀ ▄`): they
  are already in the font and never go to the fallback. If you see a thin
  strip between two blocks stacked on top of each other, this is not the
  cause — the font's glyph does not fill the cell completely, and fixing that
  is a separate piece of work.
- `family = ""`, or the key being absent, means the default font and gives no
  warning.
- `size` can be an integer or a decimal (`13`, `13.5`). Zero, a negative
  value, `nan`, `inf` or a non-number falls back to the default (`13` at
  launch, the current size when saving) and gives a warning.
- The size is multiplied by the screen's scale and **silently** clamped to the
  4–144 range: on a Retina screen (2×) the effective size you can write is
  2–72, on a normal screen 4–144; a value outside is drawn as the nearest
  limit. There is no warning, because the same value could be inside the limit
  on one screen and outside on another as the window moves between screens.
- `line_height` gives the line spacing **as a multiple of the font's own
  spacing**: `1.0` is exactly the spacing the font asks for, `1.4` is airy.
  The extra space is distributed **equally** above and below the line, so
  letters stay in the middle of the cell; the underline and strikethrough move
  down with them.
- **Below `1` the rows tighten, letters are not cut**: a letter is still drawn
  at the font's own size and the part that does not fit **overflows** onto the
  neighbouring row (as in iTerm2). The accents of `É Å Ñ` may touch the row
  above, the tails of `g j y` the row below. The lower limit is `0.5`; below
  that the overlap becomes unreadable. The upper limit is `2`, because as the
  cell grows, the number of characters that fit in the glyph atlas drops (the
  same budget as in the item below). A value outside the range falls back to
  the default and gives a warning.
- The overflow stays **within the same surface**: in the input line at the
  bottom of the window a letter overflows up to the top of the input area and
  does not leave it; at the edge of the window it is cut. Box and block lines
  (`─ │ █`) still fill the tightened cell from edge to edge, so the lines of
  `tree` stay joined.
- Line spacing applies **when you save**, like the font size; Cmd +/− changes
  the size, the multiplier stays where it is and scales with the new size.
- `letter_spacing` is the **horizontal twin** of `line_height`: it gives the
  cell width as a multiple of the font's own advance. `1.0` is the font's
  spacing, `1.2` opens the columns a little. The letter's size does not
  change; it sits **in the middle of the widened cell**; the columns open up.
  Since box and block lines (`─ │ █`) fill the cell from edge to edge, they
  stay joined in a wide cell too, a two-column character (`中`, emoji) sits in
  the middle of its two columns, and the context line at the bottom of the
  window opens up by the same ratio.
- **Below `1` the columns tighten, letters are not cut**: a letter keeps its
  size and the edges of wide letters such as `M` and `W` overflow onto the
  neighbouring column. The lower limit is `0.5`, the same as line spacing. The
  upper limit is `2`, the same atlas budget as line spacing; even with both
  multipliers at the top, the atlas holds all line and block characters. A
  value outside the range or a non-number falls back to the default (the
  current value when saving) and gives a warning.
- **Known limit:** at a very narrow letter spacing (below about `0.7`), a
  two-column character (`中`, emoji) is drawn **shrunk** instead of
  overflowing, because the box a character coming from the system has to fit
  in narrows along with the two columns.
- Letter spacing also applies **when you save**: the number of columns is
  recalculated and the shell receives the new size. Cmd +/− carries the
  multiplier, so at the new size the letters stay open by the same ratio.
- At a very large font size the glyph atlas fills up quickly: once it is full,
  characters appearing on screen for the first time are drawn as boxes (□).
  This goes away when the size is reduced or the app is reopened.
- The font changes the moment you save: the window size stays the same, the
  number of columns and rows is recalculated for the new cell, and the shell
  and the program inside it (vim, less) receive the new size as if the window
  had been resized; long lines are rewrapped.
- In a family without bold and italic faces, that text is drawn with the
  regular face; this gives no warning (a line goes to standard error).

#### Temporary font size: Cmd +, Cmd −, Cmd 0

**View ▸ Bigger** (Cmd +), **Smaller** (Cmd −) and **Actual Size** (Cmd 0)
change the font size **temporarily**: it is not written to the file and goes
away when the app closes.

- Each press is one point; the range is 4–72. A press at the end of the range
  does nothing, so holding the key down and then going back is visible at
  once. If the `size` in the file is outside the range, presses only work
  towards the range.
- **Actual Size** goes back to the `size` in the file.
- Changing `size` in the file and saving drops the temporary difference: the
  size you wrote is what you see. Changing `family` or any other key keeps the
  difference.

### `[clipboard]`

```toml
[clipboard]
osc52 = "copy"
```

| key | type | default | meaning |
|---|---|---|---|
| `osc52` | `"copy"` or `"off"` | `"copy"` | whether a program in the terminal can write to the clipboard |

- **OSC 52** is the sequence a program uses to write text to the clipboard
  through the terminal. Its best-known use is vim or tmux on a machine you
  reach over ssh: text copied there arrives in this Mac's clipboard, even
  though the remote machine has no access to the clipboard. `"copy"` allows
  this, `"off"` ignores the sequence.
- The clipboard written is the **general clipboard**, the one Cmd-C writes. If
  a program sends many copies in a row, only the last one stays in the
  clipboard.
- **There is no read direction**, and no value turns one on: a program in the
  terminal cannot read the text in your clipboard. That is why there is no
  value such as `"paste"`.
- **The cost:** while it is `"copy"`, a program running in the background
  (remote ones included) can also write to the clipboard and replace what you
  copied. If you do not want that, use `"off"`.
- The sequence's target does not matter: a sequence that writes to the
  primary selection (`p`, `s`) also writes to the general clipboard. macOS has
  a single clipboard; in vim `*` and `+` are the same clipboard here, and
  Neovim sends `*` as `p`, so the copy of a Neovim set to `clipboard=unnamed`
  over ssh arrives too. Empty text does not clear the clipboard; it is
  ignored.
- There is no size limit on a copy. While the clipboard is being written, the
  window draws no new frame; with a very large copy this may be noticeable (at
  what size has not been measured).
- An unrecognized value (`"paste"`, `"Copy"`, `true`) and `[clipboard]` not
  being a section fall to **off** and give a warning — not to the default (on)
  as the other keys do; so does a settings file that cannot be read or is
  invalid at launch (see [If something goes wrong](#if-something-goes-wrong)).
- It takes effect the moment you save; there is no need to restart the
  running program.

### `[motion]`

```toml
[motion]
cursor_motion = "spring"
reduce_motion = "system"
smooth_scroll = "on"
keypress = "fade"
erase = "recede"
```

| key | type | default | meaning |
|---|---|---|---|
| `cursor_motion` | `"snap"`, `"ease"` or `"spring"` | `"spring"` | how the cursor travels between cells |
| `reduce_motion` | `"system"`, `"on"` or `"off"` | `"system"` | whether animations are toned down |
| `smooth_scroll` | `"on"` or `"off"` | `"on"` | whether scrolling through history is smooth or line by line |
| `keypress` | `"off"`, `"fade"`, `"rise"`, `"pop"`, `"extrude"`, `"heat"`, `"echo"`, `"drop"`, `"ink"` or `"squeeze"` | `"fade"` | how a letter typed in the dock arrives |
| `erase` | `"off"`, `"iris"`, `"undertow"`, `"echo"`, `"bleed"`, `"unravel"`, `"recede"`, `"sublime"` or `"shatter"` | `"recede"` | how a letter deleted in the dock goes |

- **`"spring"`** — the cursor glides to its new place on a spring and settles
  as it slows down; it does not overshoot. A long jump takes a little longer
  than a short one.
- **`"ease"`** — the glide takes a **fixed** time, whatever the distance; it
  slows down towards the end and does not overshoot.
- **`"snap"`** — no glide, the cursor appears directly in the new cell and the
  content also moves into place at once. This is the way to turn motion off
  completely: the typing and deleting effects in the dock (`keypress`,
  `erase`) turn off too.
- The same style also drives **the content rising**: bateri anchors the
  content to the bottom of the window, so when a new line arrives, the history
  moves up and the cursor stays on its bottom row. What moves is the whole
  grid, not the cursor.
- **It keeps gliding after the window is full.** Once the screen is full and
  old lines start being pushed into history, new lines still glide in; lines
  leaving at the top stay visible until the glide ends. With very fast output
  that brings more than a screenful of lines in one frame, there is no glide;
  the newest lines appear at once.
- **Towards an area that would stay empty, it only glides upward.** When the
  content grows (a new line, a full-screen application such as `vim`/`less`
  opening), the grid **glides** up; when the content shrinks and would leave
  an **empty** area at the top (quitting that application, `clear` on a full
  screen, deleted lines), it settles into place **at once**. The reason is
  feel: flowing upward reads as content arriving, moving down reads as it
  falling.
- **Going back down glides too, if history fills the gap.** When a
  completion list closes and your older lines take its place, what moves down
  on screen is not empty space but history **arriving** from above — there
  the grid glides back. A screen you cleared on purpose with Ctrl-L or `clear`
  is outside this case: the history does not come back, so there is nothing to
  glide there.
- **The grid moving for other reasons does not glide either:** resizing the
  window, changing the font or the size. There, what moves is not the content
  but the window itself. Scrolling through history has its own setting:
  `smooth_scroll` (below).
- It takes effect the moment you save. If a cursor is gliding at that moment,
  `"snap"` finishes it at its target, and the other two styles take over the
  glide from where it is: the cursor never teleports on any style change.
- An unrecognized value (`"sprong"`, `"Spring"`, `true`) only affects this key
  (`"spring"` at launch, the style on screen when saving) and a warning
  appears.

`reduce_motion` says whether animations are toned down:

- **`"system"`** — follows macOS's System Settings ▸ Accessibility ▸ Display ▸
  Reduce Motion. Turning that setting on and off affects bateri without a
  restart.
- **`"on"`** — tones down even while the system setting is off, **`"off"`**
  does not tone down even while it is on. Neither reads the system at all.
- When toned down, the cursor does not glide: it appears in its new cell with
  **a short fade-in** (90 ms) and leaves no trace in its old cell. What is
  toned down is the glide itself, not the cursor's visibility.
- When toned down, the content rising **does not fade, it goes into place at
  once**: the whole screen fading in on every new line would be worse than the
  motion it is trying to tone down.
- The fade-in belongs to the motion **after a pause**: at normal typing speed,
  each letter shows the cursor in its new place with a short fade-in. When
  moves come more often than the length of the fade-in — while output streams
  or when typing very fast — the cursor stays fully opaque and moves to its
  new place quietly; otherwise a flicker faster than ten times a second would
  appear (with the old behavior the cursor disappeared entirely in streaming
  output).
- A letter typed in the dock **only fades in** (`"fade"`), whatever `keypress`
  is; a deleted letter goes at once, without an effect. The same rule as the
  cursor: what is toned down is the motion, not the confirmation of what you
  typed. `keypress = "off"` stays off.
- `cursor_motion = "snap"` sits **above** this: Reduce Motion does not *add* a
  fade-in for a user who has already turned motion off.
- It takes effect the moment you save. If a cursor is gliding at that moment,
  it is finished at its target — the cursor does not teleport when turning it
  on or off.
- An unrecognized value (`"yes"`, `"System"`, `true`) only affects this key.

`smooth_scroll` says how scrolling through history moves:

- **`"on"`** — when scrolling with a trackpad, the screen follows your finger
  **pixel by pixel** and half a line may show at the top; when you flick, it
  slows to a stop with macOS's momentum. When you lift your finger or the
  momentum ends, the screen **settles on the nearest line** with a short
  glide, so no half line is left in a resting window. A classic mouse wheel
  goes the same distance, but a notch does not jump; it moves with a short
  glide in the style of `cursor_motion`.
- **`"off"`** — line steps: every event scrolls by whole lines, with no
  animation. The same happens with Reduce Motion on and with
  `cursor_motion = "snap"` — scrolling does not *add* animation for someone
  who has turned motion off.
- It applies only while scrolling bateri's **own history**. In full-screen
  applications such as `vim` and `less` the wheel turns into arrow keys, and
  in applications that want the mouse (Claude Code, `htop`) into wheel
  reports, and both move by whole lines as before.
- If you use a tool that smooths scrolling itself (such as Mos), you can
  choose `"off"` so the two do not stack. How it behaves with such tools has
  not been measured.
- It takes effect the moment you save.
- An unrecognized value (`"yes"`, `"On"`, `true`) only affects this key.

`keypress` and `erase` say how letters arrive and go in the **dock** at the
bottom of the window — the line where you type your command:

- **`keypress`** — a letter you type:
  - **`"fade"`** — fades in place from clear to full color.
  - **`"rise"`** — slides up into place from a little below the cell, fading
    in as it slides.
  - **`"pop"`** — starts small, grows a little past its size for a moment and
    settles.
  - **`"extrude"`** — stretches out from its left edge to the right.
  - **`"heat"`** — starts in the theme's cursor color (`cursor`) and cools to
    its own color. Emoji are not tinted; they only fade in.
  - **`"echo"`** — appears in place; a faint copy of it grows outward,
    spreading and fading.
  - **`"drop"`** — falls from above the cell, bounces slightly and settles
    into place.
  - **`"ink"`** — the core of its strokes shows first, then it fills as if the
    ink spread to the edges. On emoji it is a plain fade-in.
  - **`"squeeze"`** — starts narrow horizontally and tall vertically, and
    springs open to its own proportions.
  - **`"off"`** — appears at once.

  All of them finish in the same short time (close to a quarter of a second)
  and at the end the letter sits exactly where its static form is; emoji and
  wide characters (`漢`) move as a single piece.
- **`erase`** — a letter you delete with Backspace:
  - **`"iris"`** — a round shutter closes over it towards its center.
  - **`"undertow"`** — fades as it is pulled down and towards the cursor, as
    if caught in a current.
  - **`"echo"`** — spreads outward as it grows, and fades.
  - **`"bleed"`** — its ink spreads: it fades as its edges spread and thin
    out.
  - **`"unravel"`** — splits into horizontal strips, which slide sideways one
    after another from top to bottom and come apart.
  - **`"recede"`** — fades as it shrinks in place.
  - **`"sublime"`** — drifts up as if evaporating, fading as it opens and
    disperses.
  - **`"shatter"`** — breaks into pieces; the pieces scatter, turning
    slightly, fall and fade. The same letter does not break into the same
    pieces every time it is deleted.
  - **`"off"`** — disappears at once.

  If you delete from the middle of the line, the text to the right shifts at
  once as before, and the ghost fades beneath it.
- The effect is drawn above the cursor, in the letter's own color — after
  Backspace the cursor lands exactly where the deleted letter was, and the
  effect is not lost inside it. Effects that come from above or go upward
  (`drop`, `sublime`) may cross the dock's top line for a brief moment.
- The effects are for letters typed and deleted **one at a time**: pasting, a
  line from history (↑), completion and deleting a word or a line (⌥⌫,
  Ctrl-U) happen at once.
- While the line is in the grid rather than in the dock (while a command runs,
  with `[shell] integration = "blocks"`, multi-line input), there are no
  effects.
- `cursor_motion = "snap"` turns both off; Reduce Motion reduces typing to a
  fade-in and turns deleting off (above). In the settings window (Motion) the
  overridden row appears disabled and says why — `smooth_scroll` too. Under
  Reduce Motion the Keypress row stays enabled: turning it on or off still
  makes a difference there.
- It takes effect the moment you save; an effect in progress at that moment
  ends at once.
- An unrecognized value (`"bounce"`, `"Fade"`, `1`, `"dissolve"`) only affects
  its own key (the default at launch, the effect on screen when saving) and a
  warning appears.

### `[shell]`

```toml
[shell]
integration = "auto"
```

| key | type | default | meaning |
|---|---|---|---|
| `integration` | `"auto"`, `"blocks"` or `"off"` | `"auto"` | whether to set up integration in the shell, and how much |

> **Retired key:** `[shell] prompt` is no longer read; `integration =
> "blocks"` replaced it. Leaving it in your file is harmless (no key is ever
> deleted) but you will see a warning — the reason is in [Getting your prompt
> back](#getting-your-prompt-back).

Integration lets the shell tell the terminal "the prompt started here, the
command ran here, it ended with this code". Today it exists only for **zsh**;
in another shell (bash, fish) `"auto"` does nothing either and the terminal
works as it is.

- **`"auto"` (the default)** — if the shell is zsh, `ZDOTDIR` points to
  bateri's own directory. The files in that directory load **your** startup
  files, put `ZDOTDIR` back to its original value (or unset it) and add the
  marks to the shell's own hooks. Your command history (`HISTFILE`) also stays
  in its own directory. In a shell that can mirror its line (today only zsh)
  the **dock** opens too: the line you type moves to the bottom of the window
  and bateri draws the prompt.
- **`"blocks"`** — the same setup, but **the input line stays yours**: the
  dock does not open, the line you type stays in the grid, and your prompt
  (p10k, starship, a `PS1` you wrote by hand) shows as it is. Command blocks,
  marks and colors keep working. When bash and fish support arrives, those
  shells will already work this way — the dock depends on mirroring the line
  editor.
- **`"off"`** — nothing is set up.
- **Your files are not written.** Not a single line is added to `.zshrc` or
  any other rc file; integration is just an environment variable, so turning
  it off leaves no trace.
- **Real** OSC 133 marks set up by another tool are read even while it is
  `"off"`: the key means "do not set up the wrapper", not "ignore the marks".
- When you move to a remote machine over SSH, our script is not there and the
  marks do not come. This is not a fault; the terminal works in its usual
  state.

**This key does not apply when you save, as the other keys do** — it is the
one exception. Integration is set up when the shell **starts**, and by the
time you save the file, the shell has already started: the value takes effect
**in the next session**, and the open window is not affected.

If the app does not open (if the window closes at once because of a broken
shell configuration), you can turn the key off **by hand**; you do not need
bateri at all. Start from another terminal.

Since integration is on by default, your settings file **may not exist at
all** — if you have never changed anything from the settings window and never
pressed Open settings.toml, it does not. Handle that case first; if the file
does not exist, a single command is enough and you do not need to read the
rest:

```sh
mkdir -p ~/.config/bateri
[ -e ~/.config/bateri/settings.toml ] || printf '[shell]\nintegration = "off"\n' \
  > ~/.config/bateri/settings.toml
```

If the file already exists, open it:

```sh
open -e ~/.config/bateri/settings.toml
```

If there is a `[shell]` section, set its `integration` line to `"off"`;
otherwise add two lines at the end of the file:

```toml
[shell]
integration = "off"
```

Do not write the section **twice**: TOML does not accept a repeated section
and the whole file becomes unreadable (the title bar says so).

- An unrecognized value (`"on"`, `"Auto"`, `false`) only affects this key
  (`"auto"` at launch) and a warning appears.

##### Getting your prompt back

Since the default is `"auto"`, anyone with a prompt set up **cannot see it**
after updating: `"auto"` hands the prompt over to the terminal and moves the
line you type into the dock. The way back is a single line:

```toml
[shell]
integration = "blocks"
```

If your file already has a `[shell]` section, add the line **inside it**; do
not write the section a second time (TOML does not accept a repeated section
and the whole file becomes unreadable). Then open a new window.

`"blocks"` **keeps** command blocks and marks — the only thing you lose is
the dock. `integration = "off"` also gives your prompt back, but out of
proportion: it turns integration off entirely, so it kills the blocks and the
marks too.

**The prompt and the dock are a single decision, and that is intentional.**
At one point there was a separate `prompt` key; it put **two prompts** on
screen (yours in the grid, the dock's at the bottom) and the cursor jumped
between them. Since saying "the prompt should be mine" already means "the line
should stay in the grid", the key was retired.

The handover happens only in **zsh**. bash, fish, the far side of SSH and an
`integration = "off"` session already show your prompt as it is.

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

| key | type | default | meaning |
|---|---|---|---|
| `hosts` | array of `{ host, mark }` | `[]` | marks for hosts: the dock's colors while ssh or mosh is on that host, or a database client is connected to it |
| `preview_max_size` | size | `"100MB"` | if a remote file's preview (⌘-click) is larger than this, asks before downloading |
| `preview_read_only` | `true` \| `false` | `true` | the preview copy opens read-only (`0444`) — a hint, an app can unlock it |
| `preview_dir` | folder | `"~/Library/Caches/bateri/Previews"` | the folder of preview copies |
| `preview_keep` | `"launch"` \| `"1d"` \| `"7d"` \| `"30d"` | `"7d"` | how long a preview stays after it was last opened; `launch` until the next launch |
| `preview_limit` | size | `"2GB"` | the preview folder's size limit; only at launch, oldest first |
| `download_dir` | folder | `"~/Downloads"` | the target of "Download to Downloads" |
| `download_conflict` | `"ask"` \| `"keep_both"` \| `"replace"` | `"ask"` | if the same name exists at the target: ask, keep both (the new one gets a number) or overwrite |
| `download_notify` | `true` \| `false` | `true` | a transfer that ends while bateri is in the background sends a notification |
| `stats` | `"sparkline"` \| `"numbers"` \| `"alerts"` \| `"off"` | `"sparkline"` | the style of the remote load indicator at the right of the ssh status bar; `off` turns the indicator and the sampling off |
| `stats_interval` | integer, seconds, `2`–`60` | `3` | time between two samples of the load indicator |
| `integration` | `true` \| `false` | `true` | a plain `ssh` sets up shell integration on the server (remote shell integration, below) |

While you are on a remote machine over ssh or mosh, the dock's context line
shows `⇄ host` and its top line is colored. `hosts` picks that color by host,
so you know from the color that you are on prod.

The same marks apply to **database clients**. While `psql`, `mysql` or
`mariadb`, `redis-cli` or `mongosh` waits at its prompt, a one-line guide in
the dock's place names the server it is connected to
(`postgres  app@db.prod:5432/main`, how to leave on the right); the first
entry matching the server's host colors that guide and the dock's top line,
so a production database is as red as a production machine. The host is the
server's name as you gave it — to an option (`-h db.prod`), in a connection
URI or string, or in `PGHOST` — without the user; libpq's `hostaddr` is
shown in its place, since that is where the client connects. A local socket
and `sqlite3`'s file have no host. The password, in whatever form you passed
it, is never shown. The guide names the server the client **was started
with**: connecting elsewhere from inside it (psql's `\c`, mysql's
`connect`, redis-cli's `CONNECT`) is not followed, so the color stays the
first server's until you quit.

Since one list serves both, **a wide pattern written for ssh colors every
database client too**: `{ host = "*", mark = "development" }` makes each
`psql` guide green as well. Put the exact names you mean before it, or mark a
host `"none"` to leave it out.

- **`mark`**: `"production"` (the theme's `error`, red), `"staging"`
  (`warning`, yellow), `"development"` (`success`, green), `"none"` (no mark —
  the theme's `info`, cyan) or a color in the form `"#rrggbb"`. Named marks
  come from the theme's roles, so they read well in light and dark themes by
  themselves; a direct color does not change with the theme and nobody checks
  its legibility.
- **`host`** is a pattern: `*` is any run of characters (empty and dots
  included), `?` a single character; case does not matter. There is no
  `[a-z]` and no `{a,b}`.
- If the pattern has no `@`, it is compared with the part of the host **after
  the last `@`**: both `ssh deploy@prod` and `ssh prod` match the pattern
  `prod`. A pattern with an `@`, such as `root@*`, checks the user name too.
- The host is the name **you type** to `ssh` (`ssh prod` → `prod`, `ssh
  deploy@10.0.0.5` → `deploy@10.0.0.5`); the `HostName` of `~/.ssh/config` is
  not resolved. If you connect with an alias, write the pattern for the alias.
  A database client's host is the server's name alone (`psql -h db.prod -U
  app` → `db.prod`), so a pattern with `@` such as `root@*` never matches
  one. A libpq service file and MySQL's option files (`~/.my.cnf`) are not
  read, so a server named only there is not known and the guide stays
  unmarked; for the same reason `MYSQL_HOST` is used only with
  `mysql --no-defaults`, since an option file would override it.
- **The first match wins**, in the order of the array: write exact names
  before wide patterns. `"none"` ends the search there — that is the way to
  leave a single host caught by a glob unmarked.
- Instead of an inline array, a `[[remote.hosts]]` array of sections can be
  written too.
- **From the menu**: while in an ssh tab, or while a database client shows
  its server in the guide, **Shell ▸ Mark “host” as ▸** Production / Staging
  / Development / None; the check mark is on the host's current mark (even if
  it comes from a pattern), and with neither the item is grayed out. The choice is written to the file: if there is an entry for
  exactly that host, its mark changes in place; otherwise it is added at the
  **beginning** of the array (if a glob comes before it, the entry is moved to
  the beginning). **None** deletes that host's entry; if a pattern still
  catches it, it writes `mark = "none"` at the beginning. The pattern is the
  host without its `user@`. Comments and the array's formatting are not
  touched; if the file cannot be parsed or the list is broken, nothing is
  written.
- A marked remote host's tab carries a small dot in the mark's color next
  to its title while the tab bar is visible; an unmarked remote tab has no
  dot. A database client's mark shows in its guide and the dock's top line
  only.
- It takes effect the moment you save, even while ssh or a database client
  is running.
- **`integration`** (`true` \| `false`, optional): remote shell integration on
  that host (below). An entry may carry only `integration`
  (`{ host = "router*", integration = false }`); it then has no mark and takes
  no part in the search for a mark. The two keys are resolved separately, and
  in both **the first match** wins: for `mark`, the first entry carrying
  `mark`, for `integration`, the first entry carrying `integration` — so
  turning integration off for a host does not take away its color. Such an
  entry given a mark from the menu gets its `mark` in place; when the menu
  deletes an entry or moves it to the beginning, it carries `integration` over
  to the new entry.
- **A broken entry** (an unknown `mark`, an entry without `host`, an entry
  carrying neither `mark` nor `integration`, an `integration` that is not
  `true`/`false`, an item that is not a table) rejects the **whole** list: at
  launch the list is empty, when saving the list on screen stays, and a
  warning appears; remote shell integration also **turns off**, because the
  prod marks and the `integration = false` entries have gone along with the
  list. Dropping only the broken entry could change the order and silently
  change a host's mark.

#### Remote shell integration

While `integration = true`, a plain `ssh` typed in the local zsh also sets up
shell integration on the server: the directory (OSC 7) and then command blocks
work remotely too. On the server, bateri only writes a few small files under
`~/.local/share/bateri/shell/`; it does not touch the server's rc files.

- **On which host**: the value of the first matching entry in `hosts` that
  carries `integration`; if there is no such entry and the host's mark is
  `"production"`, **off**; in every other case, this key.
  So to turn it on for a host marked prod, write `integration = true` in its
  entry.
- **From the first connection**: the directory and command blocks arrive with
  the first `ssh`. If the server has a login shell that does not run commands
  (a router, Windows), the server's own error line shows on the first
  connection and the connection opens plain by itself; bateri remembers that
  server as "shell-less" and later connections open plain from the start.
  Leaving a server that has a shell with `exit` does not reopen the
  connection. An `ssh` opened inside a local tmux or screen is not wrapped.
  The remembered servers and the servers bateri has written files to are kept
  in bateri's own file
  (`~/Library/Application Support/bateri/remote-hosts`), not in
  `settings.toml`; a server is identified by the `user@host:port` that
  `~/.ssh/config` resolves. A server remembered as "shell-less" by mistake is
  forgotten with Shell ▸ Shell Integration on “{host}” (the item deletes this
  record on every click).
- **Never wrapped**: ssh with a remote command or non-interactive ssh (`ssh
  host command`, `-N`, `-T`, `-W`, a pipe), `scp`/`rsync`/`git`, and a host
  whose `~/.ssh/config` entry has `RemoteCommand`, `RequestTTY no` or
  `SessionType`. These stay without integration as before; the remote session
  is still detected.
- A value that is not accepted and a settings file that cannot be read **turn
  integration off** (like `osc52`): a wrong guess would mean writing to the
  server silently.
- The setting is read on every `ssh` at that moment; open windows do not wait.
- **From the settings window**: Remote Files ▸ **Set up shell integration on
  servers** writes this key (`[remote] integration`); changing the file by
  hand changes the switch too.
- **From the menu**: while in an ssh tab, **Shell ▸ Shell Integration on
  “host”** turns integration on or off for that host. The check mark reflects
  the host's current answer — its entry carrying `integration`, otherwise its
  mark (off if `"production"`), otherwise this key; in a local tab the item is
  grayed out. The choice is written to the file as that host's **own**
  decision: if there is an entry for exactly that host, its `integration`
  line is added or changed in place (its mark is not touched); otherwise, or
  if a pattern carrying `integration` comes before it,
  `{ host = "…", integration = … }` is added at the **beginning** of the
  array and the now ineffective `integration` of that host's remaining entries
  is removed (an entry carrying only that is deleted entirely, a marked one
  keeps its mark). The pattern is the host without its `user@`; comments and
  the array's formatting are not touched, and if the file cannot be parsed or
  the list is broken, nothing is written. The change takes effect on the
  **next** `ssh`; an open connection stays as it is.

#### Remote load indicator

While you are on a Linux server over ssh or mosh, the right side of the status
bar shows that machine's load; the data comes from the helper ssh session that
file names use, and no new connection is opened.

- **`stats`**: `"sparkline"` (the default) `cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%` — the
  last eight CPU samples and the numbers; `"numbers"` `cpu 23%  mem 61%`;
  `"alerts"` a small green `●` as long as no threshold is passed, and once one
  is passed only the values over it; `"off"` no indicator and no sampling
  either. Disk (`/`) is added to every style only once it passes 85%.
- **Thresholds** are fixed: cpu 70%/90%, memory 80%/92%, disk 85%/95%. The
  labels, the graph and the numbers under the threshold are dim; a number over
  the threshold is the theme's `warning`, one over the second threshold its
  `error`, with a `▲` in front.
- **In a narrow window** the graph goes first, then only the worst value
  remains, then the indicator goes; if a value has passed its threshold, it
  takes priority over the path, and the path is shortened from the left with
  `…`. The host is never shortened. While a transfer is in progress, the
  indicator is hidden.
- **Clicking the indicator** opens the details: host and operating system, CPU
  (with the number of cores), load 1/5/15, memory, swap, disk `/`, uptime and
  the three processes using the most CPU; while open, it refreshes on every
  sample. A second click, a click outside or Esc closes it (Esc does not go to
  the server).
- **`stats_interval`** is the number of seconds between two samples, an
  integer between `2` and `60`.
- It takes effect the moment you save. A value that is not accepted leaves
  that key at its default at launch and at the value on screen when saving,
  and a warning appears.

#### Remote files: preview and download

The eight preview and download keys are the settings for file names in an
ssh or mosh session: ⌘-click **previews** a file with a temporary, read-only
copy, and downloading puts a permanent copy in `download_dir`.

- **A size** is written like `"100MB"`: an integer and one of the units `B`,
  `KB`, `MB`, `GB`, `TB` (decimal, Finder's units; there may be a space in
  between). It is case-sensitive: `"100mb"` is rejected. A number without a
  unit is rejected too, because it would leave the unit to guesswork.
- **A folder** starts with `/` or `~/`; a relative path and `~user` are
  rejected.
- **Cleanup** runs at launch (those past the keep time and the part over
  `preview_limit`, oldest first) and once a day (only the keep time); nothing
  is deleted on quit, and nothing is deleted for size while bateri is open. A
  preview that differs from what bateri wrote because you changed it is never
  deleted by any cleanup: it is moved to `download_dir` and you are notified.
- A value that is not accepted leaves that key at its default at launch and
  at the value on screen when saving, and a warning appears.

**Relative names need the server's OSC 7.** The name `backups` in `ls` output
is relative; which directory it is in is told by the OSC 7 the remote shell
prints. If it does not print one, bateri looks at the window title: the stock
`.bashrc` of Debian and Ubuntu sets the title to `user@host: directory` on
every prompt, and a directory in that form is used. If neither is there,
absolute (`/var/log/x`) and `~/…` paths still work, and with ⌘ held over a
relative name the label says why.
bateri does not write to the rc file on the server; a single line on the
server is enough to turn it on:

```sh
# ~/.bashrc
PROMPT_COMMAND='printf "\033]7;file://%s%s\007" "$HOSTNAME" "$PWD"'"${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
# ~/.zshrc
_bt_osc7() { printf '\033]7;file://%s%s\007' "$HOST" "$PWD"; }; precmd_functions+=(_bt_osc7)
```

## Themes

User themes live in this directory, one file per theme:

```
~/.config/bateri/themes/{name}.toml
```

The file's name (without `.toml`) is the theme's name, and that is what is
written in `[appearance] theme`. Warnings come with the file's name:

```
bateri – themes/paper.toml: line 3: `ansi.red` must be a color like "#rrggbb", found "red"; using #d16d6a
```

### Format

Twelve roles at the root, the 16 ANSI colors in the `[ansi]` section. A color
is text in the form `"#rrggbb"` (uppercase works too; no `#rgb` and no alpha).

| key | meaning |
|---|---|
| `background` | default background, the window's backdrop |
| `foreground` | default foreground |
| `dim` | default foreground written dim (SGR 2) |
| `accent` | accent; the mark of a **running** command |
| `cursor` | the color of the cursor block |
| `selection` | the highlight of a mouse selection; selected text keeps its own color, and in an unfocused window the highlight fades towards the background |
| `search_match` | the highlight of all matches of a search through history (⌘F); text keeps its own color, and it fades in an unfocused window |
| `search_current` | the highlight of the current match — the one ⏎/⌘G shows; more prominent than `search_match`, and the selection is drawn above both |
| `success` | status: success; the mark of a command that ended with exit code zero |
| `error` | status: error; the mark of a command that ended with a non-zero exit code |
| `info` | status: info; a remote session — the host on the context line and the dock's top line |
| `warning` | status: warning; a remote host marked `staging` — the host on the context line and the dock's top line |
| `[ansi]` `black` `red` `green` `yellow` `blue` `magenta` `cyan` `white` | ANSI 0–7 |
| `[ansi]` `bright_black` … `bright_white` | ANSI 8–15, in the same order |

- **Every key is optional.** A missing key comes from the built-in `bateri`
  theme; a two-line file that only changes the background is a valid theme.
- In a file that **shadows** a built-in theme (`themes/bateri-light.toml`), a
  missing key comes from that built-in theme itself; for a name that does not
  shadow one, the base stays `bateri`.
- **The rule has no exceptions**, not even `cursor`: in a file that only
  writes `accent`, the cursor stays the built-in theme's cursor. Whoever wants
  a separate cursor writes `cursor` — one key among more than twenty behaving
  differently would surprise more than it would help.
- A color that is not accepted (`"red"`, `"#12345"`, a number) takes the
  base's value (`bateri` or the shadowed built-in theme) and gives a warning;
  the other colors are still read.
- **An empty file** (or only whitespace) counts as unusable: many editors
  empty the file first while saving, and the window should not flash to the
  base in the middle of a save. When you save, the theme on screen stays; at
  launch the built-in theme matching the appearance comes in; both give a
  warning.
- An unrecognized key is silently ignored.
- **Dim text** (SGR 2) comes from two paths. If the default foreground is dim,
  the theme's `dim` color is used. Dimming named and 256-color text is a rule:
  the color moves a third of the way towards the theme's `background` — it
  darkens in a dark theme and lightens in a light theme. On a black
  background this is the same as the "two thirds of the color" rule of
  alacritty and vte.
- `dim`, `success`, `error` and `selection`, like every key, come from
  `bateri` when missing: in a light theme, if `dim` is not written, dim
  default text is drawn with the dark theme's gray (`#909093`); if
  `success`/`error` are not written, the command marks are drawn with the dark
  theme's green and red; if `selection` is not written, the selection is drawn
  with the dark theme's slate (`#283042`) — hard to read under dark text in a
  light theme, so a light theme should write its `selection`. The same goes
  for `search_match` and `search_current`: if they are not written, the search
  highlight comes in the dark theme's dark warm tones (`#3a3212`, `#503a0c`).
  If `info` is not written, a remote session's host and top line are drawn
  with the dark theme's cyan (`#79b3b3`), and if `warning` is not written, a
  host marked `staging` is drawn with the dark theme's yellow (`#d6b16a`).

### Embedded `bateri`

The shortest way to start a theme is to copy this and change it:

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

### Embedded `bateri-light`

The default for the light appearance. To stay legible on a light background,
yellow and cyan are in dark, saturated tones; white (`white`,
`bright_white`) keeps the meaning of its name and sits at the light end.

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

Both blocks are tied to their built-in theme by a test
(`documented_blocks_are_the_embedded_themes`): if the built-in theme changes
and the block does not, the test fails.
