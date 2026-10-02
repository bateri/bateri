# Changelog

All notable changes to bateri are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

Write user-facing changes under **Unreleased** as they land: what someone
running bateri will notice, not how the code changed. At release time rename
the section to the new version and date. `make gonder` takes that section as
the release notes — they appear on the GitHub release and in the update window
of every installed copy. A version without a section is not released.

## [Unreleased]

## [0.2.0] - 2026-10-02

### Added

- **Clickable links.** Hold ⌘ and URLs, existing files and folders, and OSC 8
  hyperlinks get an underline and a hand cursor; ⌘-click opens them. Works in
  the scrollback, in full-screen apps and in the input line. Paths with
  spaces and bare names from `ls` (`src`, `Makefile`) are found too.
  Right-click a link for Open, Reveal in Finder and Copy. OSC 8 links show a
  dashed underline without ⌘ and their target in the bottom-left corner.
- **Remote files over ssh.** In an ssh session, ⌘-hover a name in `ls` output
  and bateri asks the server whether it exists:
  - ⌘-click previews a file: a temporary, read-only copy downloads and opens
    (scripts open as plain text). Larger files (100 MB by default) ask first.
  - ⌘-drag a file or folder to Finder or the Desktop to download it there.
  - Right-click for Download to Downloads, Download To…, Copy Path and Copy as
    scp Path.
  - Downloads and uploads share one transfer list (“Show transfers”), with
    progress, cancel (⌘.) and a notification when bateri is in the background.
    Downloaded files carry the usual “downloaded from the internet” mark.
- **Settings › Remote Files**: preview size limit, preview and download
  folders, how long previews are kept, a size limit and Clear Now. Previews
  are cleaned up when bateri starts and once a day, never while you might be
  reading one, and a preview you edited is moved to Downloads instead of
  deleted.

### Changed

- The ssh status bar (`⇄ host`) stays at the bottom while vim, htop or less
  runs on the server.
- When a server doesn't report its folder (OSC 7) but sets the usual
  `user@host: folder` window title (Debian and Ubuntu do), bateri uses that
  folder for links and for uploads dropped from Finder, instead of the home
  folder.

### Fixed

- A wide character missing from the font could draw as two boxes instead of
  one.

## [0.1.1] - 2026-09-30

### Changed

- Rendering now goes through wgpu (still Metal underneath on macOS). Nothing
  should look different; this is groundwork for a Linux version.

### Fixed

- Fast, continuous output (large logs, `seq 1 50000`) no longer makes the
  screen jump back a screenful every other frame, and costs roughly half the
  GPU work it did.
- Short bursts of output that overflow a full screen (`seq 1 200`, `ls -la`)
  now slide in every time, not only when they happened to arrive in one piece.
- The About panel shows the copyright line correctly (`©` and `Ö` were
  garbled).

## [0.1.0] - 2026-09-30

First public release. bateri is free software under the GNU General Public
License v3.0 or later.

### Added

- A GPU-rendered terminal for macOS, drawn with Metal. An idle window draws
  nothing.
- Command blocks: each command is marked in the gutter by how it ended, and
  commands that run longer than a second show their duration.
- The input dock (zsh): the line you are typing sits at the bottom of the
  window with your directory and git branch, wraps and grows for multi-line
  input, and can be selected and edited with the mouse.
- Native macOS tabs, and splits you can move between, resize, equalize and
  zoom from the keyboard.
- Search in scrollback, with case-sensitive and regular-expression modes.
- SSH awareness: over ssh or mosh the dock shows the host, colored by how you
  marked it, and files dropped from Finder are uploaded to the remote
  directory.
- Emoji sequences, wide characters, and box-drawing, block and Braille
  characters drawn to fit the cell.
- Themes, fonts and every setting in `~/.config/bateri/settings.toml`, applied
  as soon as the file is saved, and a settings window that edits the same file.
- Automatic updates from GitHub Releases, signed and notarized.
