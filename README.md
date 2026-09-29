# bateri

A GPU-rendered terminal for macOS. bateri is written in Rust and draws every
frame with Metal, talking to AppKit and Metal directly — there is no Swift or
web layer in between. It aims to feel calm: commands are grouped into blocks,
the line you type lives in a dock at the bottom of the window, the cursor
glides instead of jumping, and an idle window draws nothing at all.

**[Download for macOS →](https://bateri.dev)** · macOS 14 or later · Apple silicon

## Features

- **Command blocks.** Each prompt and its output form a block, marked in the
  gutter with a chevron colored by how the command ended. Commands that run
  longer than a second show their duration.
- **Input dock.** In zsh, the line you are editing moves to a dock at the
  bottom of the window with your current directory and git branch beneath it.
  It wraps, grows for multi-line input, supports mouse selection and editing,
  and animates typing and deleting (configurable, or off).
- **Tabs and splits.** Native macOS tabs; split any tab right or down, move
  between panes, resize, equalize and zoom — all from the keyboard.
- **Search in scrollback** with case-sensitive and regular-expression modes and
  a live match count.
- **SSH aware.** Over ssh or mosh the dock turns into a status bar showing the
  host, colored by how you marked it (production, staging, development). Drop
  files from Finder onto the window to upload them to the remote directory,
  with progress, cancel and a summary.
- **Modern text rendering.** Emoji sequences (flags, ZWJ families, skin tones),
  wide characters, and box-drawing, block and Braille characters drawn to fit
  the cell exactly.
- **Motion that respects you.** A spring-animated cursor, smooth trackpad
  scrolling, and content that settles to the bottom of the window. Everything
  follows the system's Reduce Motion setting.
- **Zero idle frames.** Nothing is drawn unless something changed, so an idle
  window costs no GPU time.
- **Automatic updates** via Sparkle, signed and notarized.

## Install

Download the DMG from [bateri.dev](https://bateri.dev), open it and drag
**bateri** into **Applications**. Updates arrive automatically; you can also
check any time with **bateri ▸ Check for Updates…**.

## Shell integration

bateri sets up **zsh** automatically when it starts a shell. It never writes to
your dotfiles: the integration is loaded through `ZDOTDIR` and your own
startup files run as usual. This is what enables command blocks, the input
dock and the ssh status bar.

If you prefer to keep your own prompt and edit on the grid like a classic
terminal, set `integration = "blocks"` (blocks and marks stay, the dock goes)
or `"off"` in the `[shell]` section of your settings. bash and fish support is
planned.

## Configuration

Settings live in `~/.config/bateri/settings.toml` and apply as soon as you
save the file. **bateri ▸ Settings…** (⌘,) opens a settings window that edits
the same file. Every key is optional; a missing key uses its default.

```toml
[appearance]
theme = "system"          # follows light/dark; or a theme name

[font]
family = "Menlo"
size = 13
line_height = 1.0

[terminal]
cursor = "block"          # "block" | "underline" | "beam"
confirm_close = "running" # ask before closing while a program is running

[motion]
cursor_motion = "spring"  # "snap" | "ease" | "spring"
keypress = "fade"         # how typed letters appear in the dock

[remote]
hosts = [
  { host = "prod-*", mark = "production" },
]
```

Themes are TOML files in `~/.config/bateri/themes/`, selectable from
**View ▸ Theme**. The full reference for every key and the theme format is in
[`docs/AYARLAR.md`](docs/AYARLAR.md) (Turkish).

## Keyboard shortcuts

| Action | Shortcut |
|---|---|
| New tab / new window | ⌘T / ⌘N |
| New local tab (from an ssh tab) | ⌥⌘T |
| Split right / split down | ⌘D / ⇧⌘D |
| Previous / next split | ⌘[ / ⌘] |
| Select split in a direction | ⌥⌘ + arrow |
| Resize split | ⌃⌘ + arrow |
| Equalize splits / zoom split | ⌃⌘= / ⇧⌘↩ |
| Close pane or tab / close window | ⌘W / ⇧⌘W |
| Find / next / previous | ⌘F / ⌘G / ⇧⌘G |
| Clear screen and scrollback / scrollback only | ⌘K / ⌥⌘K |
| Paste as a single escaped argument | ⌃⌘V |
| Bigger / smaller / actual size | ⌘+ / ⌘− / ⌘0 |
| Cancel upload | ⌘. |

Hold **Shift** to select text with the mouse inside programs that capture the
mouse, such as vim.

## Building from source

Requirements: macOS 14+, Xcode (for the Metal shader compiler) and a Rust
toolchain (1.88 or later).

```sh
cargo run -p bateri   # run a debug build
make hepsi            # format check, lints, clippy and the full test suite
make kur              # build target/release/bateri.app (signed if a signing identity is available)
make yukle            # build and install to /Applications
```

The Makefile targets are named in Turkish, as are the project's internal
documents; code, identifiers, UI strings and settings are in English.

## Project layout

| crate | role |
|---|---|
| `bt-core` | Platform-independent terminal core: VT parsing (via `alacritty_terminal`), grid and scrollback, PTY, shell integration, command blocks, search, settings |
| `bt-atlas` | Glyph rasterization with Core Text and the glyph atlas |
| `bt-gpu` | Metal renderer, shaders, frame pacing and motion |
| `bt-shell` | The AppKit app: windows, tabs, splits, menus, input, settings window, updates |
| `bateri` | The binary and app bundle |

Dependencies only point downward; `bt-core` has no macOS dependencies and is
also built and tested on Linux (`make linux`).

## Releasing

User-facing changes go under **Unreleased** in [`CHANGELOG.md`](CHANGELOG.md).
To release, rename that section to the new version, bump the version in
`Cargo.toml`, tag `v<version>` on `main` and run `make yayin`: it builds,
notarizes the app and the DMG, and places the DMG, the update archive, the
release notes and the Sparkle feed in the website repository, which deploys on
push.

## License

MIT. bateri includes third-party software; see
[`assets/bundle/THIRD-PARTY-LICENSES.txt`](assets/bundle/THIRD-PARTY-LICENSES.txt).
