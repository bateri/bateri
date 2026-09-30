# bateri

A GPU-rendered terminal for macOS. bateri is written in Rust and draws every
frame on the GPU through wgpu (Metal on macOS), talking to AppKit directly —
there is no Swift or web layer in between. It aims to feel calm: commands are grouped into blocks,
the line you type lives in a dock at the bottom of the window, the cursor
glides instead of jumping, and an idle window draws nothing at all.

**[Download for macOS →](https://github.com/bateri/bateri/releases/latest/download/bateri.dmg)**
· macOS 14 or later · Apple silicon · [bateri.dev](https://bateri.dev) ·
[Changelog](CHANGELOG.md) · Free software (GPL-3.0)

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
- **Automatic updates** from GitHub Releases via Sparkle, signed and notarized.

## Install

Download [`bateri.dmg`](https://github.com/bateri/bateri/releases/latest/download/bateri.dmg)
from the [latest release](https://github.com/bateri/bateri/releases/latest),
open it and drag **bateri** into **Applications**. The app is signed with a
Developer ID and notarized by Apple, so it opens without a warning.

### Updates

bateri checks for a new version once a day and asks before installing it; you
can also check any time with **bateri ▸ Check for Updates…**. The check goes
straight to this repository's releases
(`releases/latest/download/appcast.xml`) — there is no update server of our
own, and nothing but the version check is sent. Every update is signed with
bateri's EdDSA key and verified before it is installed.

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

Requirements: macOS 14+ and a Rust toolchain (1.88 or later).

```sh
cargo run -p bateri   # run a debug build
make check            # format check, lints, clippy and the full test suite
make bundle              # build target/release/bateri.app (signed if a signing identity is available)
make install            # build and install to /Applications
```

The Makefile targets are named in Turkish, as are the project's internal
documents; code, identifiers, UI strings and settings are in English.

## Project layout

| crate | role |
|---|---|
| `bt-core` | Platform-independent terminal core: VT parsing (via `alacritty_terminal`), grid and scrollback, PTY, shell integration, command blocks, search, settings |
| `bt-atlas` | Glyph rasterization with Core Text and the glyph atlas |
| `bt-gpu` | wgpu renderer, WGSL shaders, frame pacing and motion |
| `bt-shell-common` | The platform-independent half of the app shell: settings reading, split tree, key encoding, upload rules, process table, the shell's command and environment, file watching |
| `bt-shell-macos` | The AppKit app: windows, tabs, splits, menus, input, settings window, updates |
| `bateri` | The binary and app bundle |

Dependencies only point downward; `bt-core`, `bt-atlas`, `bt-gpu` and
`bt-shell-common` have no AppKit dependencies and are also built and tested on
Linux (`make linux`).

## Releasing

Releases are cut from `main` on a Mac with the Developer ID certificate, a
`notarytool` keychain profile (`bateri-notary`) and Sparkle's signing key in
the keychain.

1. Put user-facing changes under **Unreleased** in
   [`CHANGELOG.md`](CHANGELOG.md) as they land.
2. To release, bump `version` in `Cargo.toml`, rename **Unreleased** to that
   version and today's date, and commit.
3. Run `make ship`. It builds and notarizes the app and the disk image,
   writes a one-item Sparkle feed signed with the EdDSA key, pushes `main`,
   tags the built commit `v<version>` and creates the GitHub release with
   `bateri.dmg`, `bateri-<version>.zip` and `appcast.xml`, using the changelog
   section as its notes.

`make ship` is `make release` (build everything locally, nothing public yet —
try the app first) followed by a push and `make publish` (tag and publish).
Publishing the release is publishing the update: installed copies read the
newest release's `appcast.xml`, and the website's download button points at
the newest `bateri.dmg`, so neither needs a change. A bad release is fixed by
releasing a newer version; Sparkle never downgrades.

## Contributing

Bug reports and pull requests are welcome on
[GitHub](https://github.com/bateri/bateri). By contributing you agree that your
contribution is licensed under the same terms as bateri (GPL-3.0-or-later).
Run `make check` before opening a pull request; it is the same gate the project
uses.

## License

bateri is free software: you can redistribute it and/or modify it under the
terms of the [GNU General Public License](LICENSE) as published by the Free
Software Foundation, either version 3 of the License, or (at your option) any
later version. It is distributed in the hope that it will be useful, but
WITHOUT ANY WARRANTY; see the license for details.

Copyright © 2026 Ömer Kala.

bateri includes third-party software under their own licenses (Apache-2.0,
MIT); see
[`assets/bundle/THIRD-PARTY-LICENSES.txt`](assets/bundle/THIRD-PARTY-LICENSES.txt).
