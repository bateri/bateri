# Changelog

All notable changes to bateri are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

Write user-facing changes under **Unreleased** as they land: what someone
running bateri will notice, not how the code changed. At release time rename
the section to the new version and date. `make release` takes that section as
the release notes — they appear on the GitHub release and in the update window
of every installed copy. A version without a section is not released.

## [Unreleased]

## [0.7.0] - 2026-10-07

### Added

- **A scroll bar that shows up when you scroll and then gets out of the
  way.** A thin bar appears at the terminal's right edge while you scroll —
  the trackpad or the wheel, ⌘PgUp/⌘PgDn/⌘Home/⌘End, a jump to a search
  match — and fades out about a second after you stop. Output arriving below
  never brings it up. It runs from under the title bar to the top of the
  dock, and it is not drawn in full-screen programs such as vim or less, nor
  before anything has gone into the history.
- **Point at the right edge to take hold of it.** With the pointer over the
  bar it widens into a faint track: drag the thumb, or click the track to
  jump to that spot. The wide bar marks every command in your history in the
  colours of the marks beside the commands — success, failure, still
  running — and pointing at a mark tells you the command, its exit code, how
  long it ran and when it started; a click on it brings that command into
  view.
- **Search matches on the bar.** While a search (⌘F) is open, the bar shows
  where its matches are across the whole history, the current one brightest.
- **Jump to latest.** When output arrives below a window you have scrolled
  up, a **Jump to latest** button at the bottom right says how many new lines
  came; a click glides you back to the bottom.
- **`[terminal] scrollbar`** chooses when the bar shows: `"system"` (the
  default) follows **Show scroll bars** in System Settings ▸ Appearance,
  `"auto"` shows it while you scroll, `"always"` keeps a wider bar in a track
  of its own and the text makes room for it, `"never"` hides it. It is also
  in Settings ▸ Appearance ▸ Scroll bar.

### Changed

- While there is history to scroll, the narrow strip at the terminal's right
  edge where the bar lives belongs to the bar, even while it is hidden: a
  selection or a ⌘-click on a link does not start there.
- Updating straight from 0.4.0 does not carry running programs over to this
  version: they end, and windows, tabs and history come back as after a
  quit. Updating from 0.5.0 or 0.6.0 carries them over as before.

## [0.6.0] - 2026-10-05

### Added

- **A REPL gets a one-line guide in the dock's place.** In the Python, Node,
  Bun, Deno and Ruby (`irb`, `pry`) REPLs — `ipython` and `bpython` too — the
  dock shrinks to a single line, like the ssh status bar, that says which
  interpreter you are in and where it comes from:
  `Python 3.14.5 · venv  ~/proj/.venv/bin/python3` or
  `Node v22.13.0 · nvm  ~/.nvm/versions/node/v22.13.0/bin/node`, with how to
  leave (`⌃D exit`) on the right. The version manager is named when the
  interpreter is one's — a venv, conda, pyenv, uv, nvm, volta, fnm, asdf,
  rbenv or mise. On a narrow window the exit hint goes first, then the path
  is shortened from the left. Programs that merely run on an interpreter (a
  script, a coding agent like codex or gemini) get no guide; the dock just
  steps aside for them.
- **Database clients get the same guide, colored like the host they are
  on.** In `psql`, `mysql` and `mariadb`, `sqlite3`, `redis-cli` and
  `mongosh` the line names the server the client was started with —
  `postgres  app@db.prod:5432/main`, or the file for SQLite — with how to
  leave (`\q to leave`) on the right. Your `[remote] hosts` marks now apply
  to the database's server too, so a production database is as red as a
  production machine, and **Shell ▸ Mark “host” as** marks it from there. A
  password is never shown, whichever way you passed it (an option, a
  connection URI or string). A wide pattern you wrote for ssh, like
  `{ host = "*", mark = "development" }`, now colors database clients as
  well.
- **Containers and Kubernetes pods say where you are.** A shell opened with
  `docker run -it`, `docker exec -it`, `docker compose exec` or `run` (and
  their `podman` forms) gets the guide line with the image, container or
  service it runs in — `container redis:alpine`. `kubectl exec -it`,
  `kubectl run -it` and `kubectl debug -it` name the cluster's context, the
  namespace you gave and the pod — `k8s prod-eu · payments  pod/api-7f9c` —
  and your `[remote] hosts` marks color the guide by the context, so a
  production cluster is red too; **Shell ▸ Mark “host” as** marks the
  context from there. The context is the one `--context` names or your
  kubeconfig's current one (only that line of the file is read), and a
  context's name is matched whole: write `kubernetes-admin@kubernetes`, not
  `kubernetes`.
- **A root shell is red, a nested shell says so.** After `sudo -i`,
  `sudo -s` or `su` the guide line reads `root  exit to leave` in red,
  whatever your marks say, so a root shell is never mistaken for your own;
  a shell started from the prompt reads `nested bash  exit to return`, and
  one a tool opened names the tool (`nested poetry shell`). While `sudo`
  asks for your password, and for the whole of a `sudo` command that is not
  a shell (`sudo make install`), the dock stays as it was.

### Changed

- **The input dock steps aside for programs that read the keyboard
  themselves.** When a command such as `codex`, Claude Code in its inline
  mode, `python3` or `node` takes the keyboard, the dock at the bottom goes
  away for as long as that program runs — the program sits at the bottom of
  the window and the recent history fills the top — and comes back when you
  return to the prompt, as it does after vim. The terminal's size does not
  change, so the program is never asked to redraw. Commands that only print
  (a build, `ls -R`, `npm run dev`) keep the dock as before.

## [0.5.0] - 2026-10-04

### Added

- **Your programs survive a crash.** If bateri crashes or is forced to quit
  (Activity Monitor, `kill -9`), vim, `npm run dev` and an ssh session keep
  running without a window, and the next bateri takes them back into their
  windows — the same processes, not restarted copies — with their screens
  and scrollback as they were at the crash, and whatever they printed while
  bateri was closed below. This is the new default; a small helper process
  (`bateri hold`) runs beside bateri to keep the programs somewhere while
  bateri is gone, and bateri keeps a record of each pane's recent output in
  memory it shares with that helper (nothing is written to disk). A pane
  that was already open when you switched away from `"update"` comes back
  with its program redrawing the screen — vim or htop at once, a plain
  shell's screen is gone — and the pane says so in a dim line.
- **Programs can outlive ⌘Q too.** `[terminal] keep_running` (or Settings ›
  General › Keep programs running) chooses `"update"` (only an update keeps
  them, as before), `"crash"` (the default) or `"quit"`: with `"quit"`, ⌘Q
  asks nothing and the programs run on until you open bateri again, which
  brings them back with their screens. A notification names what is still
  running a moment after you quit, so nothing is forgotten, and **Quit and
  End Programs** (⌥⌘Q, under the bateri menu with ⌥ held) is the quit that
  ends them. The setting applies the moment you save it, no restart needed.
  Restarting the Mac or logging out still ends the programs.
- **Windows come back after a crash.** The layout — windows, tabs, splits,
  folders — is kept on disk while bateri runs (without the scrollback), so
  after a crash or a power cut the windows reopen even where no program was
  kept, each pane with a new shell (not with `restore_windows = "off"`).
- **Hold ⇧ while opening bateri** to start with one empty window and skip
  everything saved; kept programs wait for the next launch. If bringing
  programs back ever crashes bateri, the next launch brings them back without
  their screens, and the one after gives them up and opens an empty window,
  so a bad restore cannot trap you in a crash loop.

### Changed

- `restore_windows = "off"` no longer ends running programs on an update:
  whether programs live on is now `keep_running`'s alone, and `restore_windows`
  only decides what is written to disk and how a pane whose program did not
  live comes back.

### Fixed

- A screen brought back after quitting or an update keeps its layout in
  three cases where it shifted: text after a tab stays in its column (it
  moved right by the tab's width), a line that wrapped onto a line erased
  since keeps its own row (the two came back as one), and a tab stop set in
  the last column is still there when the window grows wider.
- bateri no longer crashes when a program redraws a progress bar under a
  fixed top line (apt does) while you are scrolled up in the history; the
  view now stops at the top of the history.

## [0.4.0] - 2026-10-03

### Added

- **Updates keep your programs running.** “Install and Relaunch” no longer
  ends what runs in your panes: shells, vim, `npm run dev` and an ssh session
  — Claude Code on a server included — carry on in the new version with their
  screens as they were. A file transfer or a password prompt in progress makes
  the relaunch wait (the status line says “Update waits for N transfers”; ⌘.
  cancels the transfer and lets the update go on). This starts with the next
  update: the version being replaced must already carry it, so updating to
  0.4.0 itself still restarts your programs.
- **Windows come back after quitting.** After ⌘Q or a restart,
  bateri reopens your windows, tabs and splits — each pane in its folder, at
  its font size and with its scrollback in colour. Shells start fresh; a pane
  that was on ssh comes back with the `ssh …` line typed and waiting for ⏎.
  `[terminal] restore_windows` (or Settings › General) chooses `"all"`,
  `"layout"` (no scrollback is written to disk) or `"off"`.
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
