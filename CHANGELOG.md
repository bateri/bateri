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

### Added

- **Letter spacing.** `[font] letter_spacing` in `settings.toml` (or Settings
  › Font) widens or narrows the space between letters, the horizontal twin of
  line spacing — iTerm2's “Horizontal spacing” divided by 100.
- **Tighter spacing.** Line and letter spacing now go down to `0.5`. Below `1`
  letters are no longer cut: accents and tails reach into the neighbouring
  line, as in iTerm2, while box-drawing lines stay joined.

### Changed

- `line_height = 1.0` is now exactly the font's own line spacing; lines were
  one pixel taller than the font asks for.

### Fixed

- The Settings window scrolls: the bottom rows of Remote Files were hidden
  under the button and could not be reached. The window can also be made
  taller or shorter.

## [0.3.0] - 2026-10-03

### Added

- **Folder tracking and command blocks over ssh, with nothing to install.**
  Type `ssh server` as usual: on the first prompt the status bar shows the
  remote folder (`⇄ server  ~/project`) and follows `cd`, remote commands get
  their success/failure stripe and duration, and ⌘-click opens relative
  paths. Works with zsh, bash and fish on the server; your rc files on the
  server are never touched (bateri keeps a small folder under
  `~/.local/share/bateri` there). Inside a local tmux, and for servers marked
  Production, it stays off.
- Servers without a shell (routers, Windows, `git@github.com`) are detected
  on the first try: the connection quietly reopens as a plain ssh session and
  bateri remembers that server.
- **Turn it off** in Settings › Remote Files, or per server with Shell ›
  Shell Integration on “server”.
- **Password servers work for remote files.** Dropping a file, ⌘-click
  preview, downloads and the server load now work on servers that ask for a
  password. While your ssh session is open, they ride on it and ask nothing;
  otherwise bateri asks once in its own sheet and can remember the password
  in your Keychain (Shell › Forget Password for “server” removes it). The
  status bar shows **Sign In…** when a password is needed.
- Remote programs can recognise bateri: `LC_TERMINAL=bateri`,
  `LC_TERMINAL_VERSION` and `LC_BATERI_TAB_URL` reach the server (servers
  that accept `LC_*`, and every server bateri sets up).
- A second `ssh` to the same server in another tab doesn't ask for the
  password again while the first one is open.
- **Server load in the ssh status bar.** While you're connected to a Linux
  server, the right side of the status bar shows its CPU and memory:
  `cpu ▂▃▅▇▅▃▂▁ 23%  mem 61%`, with the last eight CPU samples as a small
  graph. Values stay dim until they cross a threshold (CPU 70/90%, memory
  80/92%), then turn yellow or red, with a `▲` when critical. Disk usage
  appears only once `/` is more than 85% full. In a narrow window the graph
  goes first, then all but the worst value; the host name is never cut.
- **Click the load for details**: CPU cores, load average, memory and swap,
  disk, uptime and the three processes using the most CPU right now.
- **Settings › Remote Files › Server load**: graph, numbers only, alerts only
  (a green dot until something crosses a threshold) or off, and how often to
  sample (every 3 seconds by default).
- Sampling uses the connection bateri already keeps open for remote files, so
  no extra login. It pauses while the tab is in the background or after two
  minutes without input, and stops when you leave the server. Servers that
  aren't Linux show no load. The load appears only after you've logged in.

### Changed

- The ssh status bar now shows the remote folder from the window title too
  (`⇄ host  ~`), not only from servers that report it (OSC 7). The title can
  also be in the `user@host:folder` form, without a space.

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
