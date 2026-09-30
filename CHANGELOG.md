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
