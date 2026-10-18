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

### Added

- **The side column's rows can be cards that say what their tab is doing.**
  Turn on `tab_details` under `[appearance]` (or Tab Details in the settings
  window and the command palette) and, with the tabs at the side, each row
  keeps its icon, title, × and ⌘ key on its first line and says under them
  the last thing the tab told — a notification a program sent (`OSC 9` or
  `OSC 777`), such as what an agent last said or the permission it asks for,
  in up to three lines — or else how its newest command ended (`Finished ·
  cargo build · 12s`, `Exit 2 · make test`), and on its last line the branch
  and the folder (`main · ~/api`). A split tab shows each pane as a small
  card of its own — its title, and what it last said under it. A row
  is as tall as what it says: a tab
  with nothing to say keeps a plain row's height. A notification speaks for
  its tab until that pane's next shell command starts or ends — an agent
  ending its turn does not clear it — and a newer one replaces it; it is not
  posted to Notification Center. A command that still runs adds no line, the
  icon's ring says it. The title row's chips stay one line, and the setting
  is off by default.

### Changed

- **The command palette fades out as it closes**, the way it came in and like
  the find panel, instead of vanishing at once. The keys and the pointer are the
  tab's from the moment it starts to go, so what you type or click right after
  ⏎ or esc is not lost. With Reduce Motion it still goes at once.

## [0.19.0] - 2026-10-18

### Added

- **A command palette.** ⇧⌘P (View ▸ Command Palette…) opens one search field
  over the terminal and finds, by a few letters of each word — “spr” is Split
  Right, “side” is Tab Position — every command of the menus (with the
  shortcut, and greyed as the menu would be), every setting and theme, the tabs
  and groups of all windows, folders, servers, ports and the commands your open
  panes ran. ⏎ does it, → opens the row's menu or page, esc steps back and
  closes, and what you chose last comes first. A command from the history is
  never run for you: it is written on a prompt and waits for your own ⏎.
  - **Themes and settings try themselves out.** A theme, and the font family
    and size, the line spacing, the cursor shape, the split style, the veil over
    a window that lost focus, where the tabs sit and the tab icons and colours,
    show on every window as the highlight reaches them, without touching
    `settings.toml`; ⏎ writes the choice there (the rest of the file stays as it
    was) and esc puts everything back. The settings you type a number for stay
    in the settings window; the palette offers a list of values.
  - **⌘P goes to a tab.** The list is in the order you looked at the tabs, with
    the one you just left highlighted, so ⌘P ⏎ goes back and forth between two
    tabs and another ⌘P goes one further. Groups are listed with their ⌃⌘
    numbers (⏎ goes to the tab you were last on in the group), a tab of another
    window comes to the front, and → opens the menu its chip or its group header
    has.
  - **⌘O goes to a folder.** The folders your tabs have been in, newest first;
    type a path (`~/Pro`) and it lists what is inside, ⇥ steps in. ⏎ opens a
    new tab there. Servers come from the `Host` lines of `~/.ssh/config` and
    the ones you reached: ⏎ connects in a new tab, ⌥⏎ in a split beside the
    pane you are in. Ports listed by the panes open in the browser. Shell ▸
    Copy Folder Path and Show Folder in Finder are menu commands now.
  - **Command History** lists what the open panes ran, where and how each
    ended. ⏎ writes the command on the prompt of the tab in front when it is
    idle, and otherwise in a new tab in the command's folder; ⌥⏎ does it in a
    new split beside the tab; ⌘⏎ takes you to its output (Window ▸ Jump to
    Command… lists only those) and a row's menu copies the command or its
    output. A command that ran on a server is never opened in a new local tab
    (it would run on this Mac): it is copied. Edit ▸ Copy Last Command and Copy Last Output copy from the tab in
    front. When a program holds the whole screen (vim, a pager) the commands
    before it are still listed under a note that says so; their output can be
    copied from behind the program, and shows again when it closes. The
    palette reads only what your open panes ran — not the shell's own history
    file.
  - The menu has Go to Tab… (⌘P), Go to Folder… (⌘O), Jump to Command…, Copy
    Last Command, Copy Last Output, Copy Folder Path, Show Folder in Finder and
    Open settings.toml.

## [0.18.0] - 2026-10-17

### Added

- **Tabs can be gathered into named groups.** Right-click
  a tab — a window's only tab too, where it is the window's title — and choose
  **Move to New Group** (or **Move to Group ▸**), or press
  ⇧⌘N for a new tab in a group of its own: in the side column the group gets a
  header — an arrow, its name and, with the pointer over it, a `+` that opens
  a tab in it — and its tabs stand under it. Tabs that are in no group stay at
  the top of the column with no header, and a window with no group looks as it
  always did. A new group is named for the folder of its first tab (`~` is
  `home`, a remote tab gives its host), with its name field already open.
  Click a header to close the group: it keeps its tab count, a dot when one of
  its tabs has a question, a failure or a finish you have not seen, and the
  tab on screen stays listed under it. Double-click a header, or right-click
  it, to rename the group, add a tab to it, ungroup it or close it. ⌘T opens a
  tab beside the one on screen, in its group; Shell ▸ New Ungrouped Tab opens
  one among the ungrouped tabs; ⌘1–9 count the tabs as the column lists them;
  ⌃⌘1–9 go to the nth group (⌃⌘9 the last), to the tab you were last on in it,
  and ⌃⌘0 to the ungrouped tabs; hold ⌃⌘ to see the keys on the headers. Moving
  a tab into or out of a group is undone with Undo Move, the groups come back
  with the windows when bateri restores them, and Merge All Windows brings
  every group along under its own name. The name you type in the field a new
  group opens with is part of the move — Undo Move still takes the move back —
  while renaming a group later is not. With the tabs in the title row the bar
  shows one group at a time: a button at its start names the group on screen
  (the ungrouped tabs have the stack alone), with a dot on its corner while
  another group has a question, a failure or a finish you have not seen. Click
  it to list the window's groups — choose one to go to it, or New Group —
  double-click it to rename the group, right-click it to ungroup or close it.
  Moving the tab on screen into another group leaves the window where it is,
  on the group's next tab. In the side column ⇧-click picks the rows from the
  tab on screen to the one you click and ⌘-click adds a row or takes it out,
  within one group (or among the ungrouped tabs) — the tab on screen stays
  where it is, and a plain click, a click on a row of another group or a click
  in the terminal lets the picks go. Right-click a picked row to move all of
  them into a group or a new one, out of their group, to a window of their
  own, or to close them; ⌘W closes the picked tabs and, unlike ⌘W on one tab,
  always asks first (`confirm_close = "never"` still does not). Tabs and groups can
  be dragged. In the side column a row slides as it always did, and the rows
  make room where it would stand: among the rows of a group, among the ungrouped
  tabs, or at a group's border — over the upper half of a header the gap opens
  above it, at the end of the group above, over the lower half under it, at the
  head of the group — and onto a closed group's header it joins that group; out
  of the window it becomes a window of its own (ungrouped), and over another
  window's column or title row it lands in the group it is let go on. The rows
  you picked are drawn as a stack with their count and carried to a line that
  marks the place the same way. A group's header carries the group: between
  two groups to put them in order (the ungrouped tabs stay first), out of the
  window for a window of its own under the group's name, or onto another window,
  where it comes up under that name (` 2` when that window has a group of it
  already); in the title row the group's button is pulled out of the bar for the
  same, and in the list under it a group's row is carried to the line between two
  groups. A click on a header still opens and closes it, and every drag is undone
  with Undo Move. A tab's name in the title row or the side column, and a group
  button's name, no longer ends in "…" when it does not fit: its last stretch
  fades out, and the whole name is in the tooltip. Group headers and the lists
  keep their "…".

## [0.17.0] - 2026-10-17

### Added

- **The side column can be made wider or narrower.** With the tabs at the
  side, drag the column's right edge — the pointer turns into a resize cursor
  over the space between the rows and the content — anywhere from 136 to
  400 pt; the terminal reflows as it moves, as it does while the window is
  resized, and with `resize_in_steps` on the column moves a cell at a time so
  a lone pane keeps whole cells. A double click on the edge brings it back to
  200 pt. In a narrow window the column gives way before the panes do, leaving
  them at least 300 pt, and widens again with the window. Each window keeps
  its own width: a new window, or one a tab is torn off into, starts as wide
  as the window it came from, and the width comes back when bateri restores
  its windows.
- **The dock shows the Python environment you activated, and the Node
  version in a Node project.** The dock draws the prompt itself, so the
  `(.venv)` that `activate` puts in front of a prompt used to be lost; the
  dock's bottom line now starts with it, as `activate` prints it and in the
  line's brightest tone, with its Python version: `(.venv) 3.12  ~/api |
  main`. conda's environment shows the same way, `(base)` too when conda
  shows it in your prompt. In a folder with a `package.json`, `.nvmrc` or
  `.node-version` (here or above, up to the repository's root) the Node
  version that runs follows the branch, `node 22.11`, and turns yellow with
  what is asked beside it when the project's `.nvmrc` or `.node-version` asks
  for another (`node 18.20 (.nvmrc 22)`). bateri reads where Node is
  installed and never runs it; a shim that picks the version per folder
  (volta, asdf's) shows none. On a narrow line the path shortens first, then
  the Node version goes, then the Python version, and the environment's name
  goes last. Turn either off with `python_env` or `node_version` under
  `[shell]`, or in the settings window's General page.

### Changed

- **A tab with one pane has the same summary card as a split one.** Its pane
  is a tile like each pane of a split tab: its program's mark, its name and
  what it is doing over its picture, outlined as the focused pane. The card's
  lines are a split tab's too — the title, the folder and a marked host — so
  "Running · npm run dev · 0:12" is now the tile's "Running".

### Fixed

- **The line between the side column and a tab with one pane comes back.**
  In a window that opened on a split tab — reopened by bateri at launch — the
  thin line beside the column stayed away when a tab with one pane was shown.
- **Only the focused pane's cursor looks focused when a window opens with
  several panes.** A window restored at launch drew a filled, blinking cursor
  in every pane of its split tab, as if each had the keyboard, until you
  clicked into each one; the others now show the hollow cursor from the start.
- **Resizing a window with a split tab keeps up with the pointer.** Every step
  of a resize — the window's edge or the line between two panes — drew the
  shadows under the panes again, which took about 55 ms on a Retina display
  and held the resize to a handful of steps a second. The shadows are now
  drawn once and stretched to each pane, and look the same.

## [0.16.0] - 2026-10-16

### Added

- **Tabs can stand in a column down the window's left edge.** Choose
  View ▸ Tabs ▸ Side (or Tabs in the settings window, `tab_position = "side"`
  in the file) and the tabs move, in every window at once, into a 200 pt
  column as tall as the window: the title row goes and the traffic lights sit
  at the column's head. A row is a tab's chip stood upright, with its icon,
  title, indicator, ⌘1–9 number, × and summary card, which opens to the right
  of the row; New Tab is at the foot with the ⚠ of a settings problem beside
  it, and a long list scrolls and fades at its ends. Drag a row to reorder it,
  out of the column to tear it off into a window, or onto another window's
  column or bar; carry a pane with ⌥⌘ onto a row to join that tab, or between
  two rows to open a new one. A tab with one pane is set off from the column by
  a thin line; a split tab's cards and the column stand on the same ground, and
  the first card keeps its 8 pt from the column. The window moves by the
  column's empty space. Tabs along the top stay the default and look as they
  did; `content_edge = "line"` draws no line along the window's top with the
  tabs at the side.
- **A tab's summary card shows its panes.** Hovering over a tab shows a small
  picture of what its pane shows right now — the grid, the history above it,
  the dock, a program on the alternate screen like vim or htop as it looks in
  the pane — under the lines its card has always had. A tab that holds several
  panes shows each pane where it sits in the tab, with its program's mark, its
  name and what it is doing over its picture, the focused one outlined, in
  place of "N panes". The pictures are taken when the card opens and twice a
  second while it stays open, and only then; panes in tabs that are not shown
  give theirs too. A press on a pane selects the tab and focuses that pane, and
  the tab's icon follows to that pane's program; a press on the picture of a
  tab with one pane shows that tab. The card waits a moment as the pointer crosses from
  the tab to the card, so it does not close on the way.

### Changed

- **A transfer fills its tab.** The tab's own ground fills from the left as an
  upload goes, at an even pace between reports, instead of a line under the tab
  that stepped and vanished. When it ends the fill runs to the end and fades
  and a tick (or, if it failed, a dot) stays on the tab until you look at it,
  so a transfer that finishes in a background tab is no longer missed.
- **Badges and the running line move.** A question, tick, dot or arrow on a
  tab's icon grows onto the tile's corner and shrinks away, one badge turning
  into another keeps its circle, and the line round a running tile fades in and
  out. A tick or a dot rests for a moment after you look at the tab, then goes.
  With Reduce Motion on they only fade.
- **A tab's summary card eases in from the tab** and fades out, instead of
  appearing and vanishing at once.
- **An `ssh` session on another port shows its port.** When the port is given
  on the command line (`ssh -p 2222 host`, `-o Port=2222` or an
  `ssh://host:2222` address), the context line, the window and tab title, the
  reconnect prompt and the transfer line read `⇄ 10.0.0.5:2222` instead of
  `⇄ 10.0.0.5`, so two sessions on the same machine, `localhost:2222` and
  `localhost:2223`, are no longer the same label. Marks, saved passwords and
  Shell Integration still go by the host alone; a `Port` set only in
  `~/.ssh/config` is not shown, nor is port 22 or a mosh session's.

### Fixed

- **The input source balloon appears at the cursor.** Pressing fn (🌐) or
  switching keyboard layouts showed macOS's input source indicator in the
  pane's top-left corner, far from where you type; it now opens at the
  cursor, in the prompt and in programs that hide the cursor to draw their
  own.

## [0.15.0] - 2026-10-13

### Added

- **Programs can show images.** Anything that speaks the kitty graphics
  protocol draws in the window: `kitten icat`, `chafa --format=kitty`,
  `timg -p kitty`, yazi's previews and terminal-browser. An image sits in the
  cells where the program put it, scrolls with the text around it (and back
  into view when you scroll up), stays under or over the text as the program
  asked, and goes when the program deletes it or you clear the screen. A
  program that asks for it also gets the pointer's exact position, not just
  the cell, so a click on a small button in a page lands on it. A program
  can hand a picture over through a file or shared memory, so big images and
  pages that redraw often need not pass through the terminal's input; bateri
  reads only ordinary files (never `/dev`, `/proc` or `/sys`), deletes only a
  temporary file named for the purpose, removes the shared memory object the
  program hands over, and never writes a name a program gave back into its
  answers. In an interactive `ssh` or `mosh` session the names a program
  sends are not read (they would be the server's files), so a program there
  sends the picture itself. A program that asks what the terminal can draw and
  then for its attributes (terminal-browser does) hears the answers in the
  order it asked.
  Images do not survive an update or a crash of bateri; the text does, and a
  program that redraws when its window is signalled draws its images again.
- **Programs can change the mouse pointer.** A program that asks for a
  pointer shape (OSC 22) gets it while the pointer is over its text: the hand
  over a link in terminal-browser, a text beam over a field, a crosshair, a
  resize arrow. The shape belongs to the screen the program drew it on, so
  the arrow comes back when a full-screen program exits, and again when your
  shell's prompt returns after a program left a shape behind. The dock, the
  scroll bar and a link under ⌘ keep their own pointers. A shape a program
  asks for while the mouse is still shows as soon as the mouse moves.
- **bash gets the command blocks and the dock.** When your login shell is
  bash 4.4 or newer, the stripe beside each command, the duration of a long
  one and the directory you are in show, the `ssh` integration reaches
  servers from bash too, and the line you type moves to the dock at the
  bottom of the window while bateri draws the prompt, as it does for zsh and
  fish. Clicking in the dock moves the caret there, and selecting text and
  typing over it replaces it. To keep your own `PS1`, set `[shell]
  integration = "blocks"`: the blocks stay and the dock goes. bateri starts
  bash as a login shell, so `logout`, `~/.bash_logout` and your history work
  as before, and nothing is written to your bash files. macOS's own
  `/bin/bash` is version 3.2 and stays exactly as it was; install a newer one
  (for example `brew install bash`) to get this.
- **fish gets the dock.** When fish is your login shell, the line you type
  now moves to the dock at the bottom of the window, in fish's own colors with
  its gray suggestion, and bateri draws the prompt, as it does for zsh. The
  stripe beside each command, the duration of a long one and the directory you
  are in come with it, and the `ssh` integration reaches servers from fish
  too. Nothing is written to your fish files. To keep your own fish prompt, set
  `[shell] integration = "blocks"`: the command blocks stay and the dock
  goes. With fish 4.1 or newer, clicking in the dock moves the caret there,
  as it does in zsh; selecting text in the dock and typing over it does not
  work in fish yet.
- **Resize in steps.** A new setting, Resize in steps (`[appearance]
  resize_in_steps`, off by default), resizes a window a whole cell at a time
  while you drag its edge, as Terminal does: a tab with one pane then keeps
  exactly the same margin on both sides and the thinnest strip at its top,
  and the window snaps to that size as soon as you turn it on. Split tabs,
  full screen, windows macOS tiles or fills, and the moment after ⌘+ or ⌘−
  keep resizing freely until the next drag.

### Changed

- **The text keeps the same margins as the dock, on every side.** The right
  edge of the text used to run almost to the window's edge, a few points
  from it and a different amount at every width, while the dock's own text
  stopped well inside; now the text stops where the dock's text does, and
  whatever the width leaves under a column stays inside that margin. The
  bottom row also keeps the dock's breathing room above the dock instead of
  touching it, and where the dock steps aside — Claude Code and other
  programs that read the keyboard themselves, full-screen programs such as
  vim or htop, vim on a server over ssh — the last row now stands that same
  margin above the window's bottom or the status bar's line, so the window's
  rounded corner no longer clips the last column. Programs that paint their
  own background still reach the window's edges. The cost is a column or two
  and sometimes a row fewer in the same window.

### Fixed

- **A full-screen program's row no longer slips one column to the left.**
  When a program's output arrived split in the middle of a character such as
  `·`, `ğ` or `│`, the character or space right after it could be lost, so
  the rest of that row was drawn one column too far left — in htop, a game
  or a program's own frame, the right border of a single row stepped in, and
  it stayed that way until the program redrew the row. Text shown in the
  usual way could lose a letter the same way. It depended on how the output
  happened to be split, so it came and went with the window's size and the
  moment.
- **fish on a server keeps its own directories.** Over ssh, a server whose
  fish had no `XDG_DATA_DIRS` of its own lost `/usr/local/share` and
  `/usr/share` from its search paths for the session, so completions and
  functions installed there went missing; they stay now.

## [0.14.0] - 2026-10-12

### Changed

- **A new tab from an ssh tab opens in the same remote folder.** ⌘T (and
  the tab bar's `+`, and a split) on a tab connected over ssh already
  connected the new tab to the same host; it now also lands in the folder
  that tab was in — the one its dock shows — instead of your home folder
  there. This works on hosts where bateri's remote integration is on. If the
  folder is gone by then, the new tab starts at home as before, and a later
  `ssh` you type in that tab starts at home as usual.

### Fixed

- **Tabs keep their program icons when bateri reopens.** After bateri was
  quit, updated or crashed with programs left running, the tabs it took back
  showed the terminal icon (or the server's, over ssh) instead of the
  program's: the command you typed had long scrolled away, and bateri often
  recognized the program only from it. Now each tab carries the icon it showed
  over to the reopened bateri, which keeps it for as long as that program
  runs. Claude Code is also recognized again in a reopened tab on its own: its
  installer starts it from a file named after its version, and bateri now
  reads the name it was started by as well.

## [0.13.0] - 2026-10-12

### Changed

- **The dock stands a step above the text.** The line over the input row is
  gone: the dock is now a raised plate, a shade lighter than the terminal,
  with a soft shadow falling on the last lines of output and a lit edge. The
  shadow lies under the text, so nothing you read gets darker. On a pure
  black theme, where a shadow cannot show, the lit edge carries it. When the
  edge has something to say it still says it: over ssh and under a program's
  guide bar it is a line in the host's or the bar's color, with no shadow on
  the line you are typing just above it, and an upload's progress fills
  along it and glows. While a new tab's shell starts, the rippling line now
  has the input row's color under it.

## [0.12.0] - 2026-10-12

### Added

- **24 well-known themes, built in.** View ▸ Theme ▸ and the settings window
  now list Catppuccin (Frappé, Latte, Macchiato, Mocha), Dracula, Everforest,
  Flexoki, GitHub, Gruvbox, Kanagawa Wave, Night Owl, Nord, One Dark, Rosé
  Pine (with Dawn and Moon), Solarized and Tokyo Night (with Day), dark and
  light ones in their own groups. Each keeps its authors' colors exactly;
  what bateri needs beyond them — the selection, the search highlights, the
  dim text — is drawn from the same palette, so a theme looks like itself and
  everything on it stays readable. Pick one from the menu, or write its name
  in `[appearance] theme` (`theme = "catppuccin-mocha"`); `light_theme` and
  `dark_theme` take them too.
- **Ultraviolet, a new theme of bateri's own.** Vivid colors on a deep violet
  ground, with an amber cursor; every color reads clearly on it, comments
  included.
- **A tab tells which program runs in it.** Before its title a tab now
  draws a small icon: the mark of the coding agent in it (Claude Code, Codex,
  OpenCode, Antigravity, Gemini CLI, Pi, Herdr, Cursor CLI, Copilot CLI, Qwen
  Code, Kimi CLI), or a drawing for what else a pane spends its time in — the
  shell, an ssh session's server, an editor, a language prompt, a database, a
  container, a system monitor, git, a build or test run, a pager, a file
  manager, a root shell. The mark stands on a tile, like an application's
  icon; while a command runs a line runs round the tile's edge, and a
  question, a finished command, a failure or a transfer sits on its corner. A
  tab split into panes shows a deck: the focused pane's program on the front
  tile and a smaller tile behind it for each other pane, the front ones
  turned a little, like cards laid down by hand. A command that ends while you
  look at another tab leaves its icon with the tick or the dot until you come
  back, so you can read what finished.
- **Turn tab icons off, or give them brand colors.** Settings ▸ Appearance ▸
  Tab icons (`tab_icons`) turns the icons off and brings back the small
  indicator tabs had before. Icon colors (`tab_icon_colors`) lets Claude
  Code, Antigravity, Gemini CLI and Qwen Code keep their own brand colors on
  their tiles instead of the theme's. Show All Tabs lists each tab with its
  icon too. A change applies at once.

### Changed

- **Text stays clear of the window's rounded corner.** On macOS 26 the bottom
  corners of a window are generously round, and the folder line and the
  prompt sat right against the curve. The left margin now follows the corner,
  so the first letter keeps the same distance from the curve as from the
  straight edge — in a window, in a card of a split tab and beside a window
  corner in a lined split. Full screen, whose corners are square, keeps the
  margin it had.
- **The ports and buttons on the folder line stay put at the right edge.**
  The listening ports, the load indicator and the upload buttons used to end
  anywhere between the window's right edge and a few points short of it,
  depending on the window's width, and they hugged the rounded corner just as
  the path did on the left. They now end the same distance from the right edge
  as the text starts from the left one, at every width; the path keeps its
  place beside the prompt's ›, and a click or hover lands on what is drawn.
- **The folder row has a tone of its own.** The line between the input row and
  the row with the folder, branch, ports and buttons is gone; the folder row
  now stands on a slightly lighter (on a light theme, darker) ground than the
  input above it, running to the window's bottom edge, and the two rows read
  as two surfaces without a rule. On a remote session, where the dock is only
  that row, the whole dock takes the tone. A new tab brings the tone in from
  the left, the way the line used to be drawn.
- **The dock's line is a point thick.** The line above the input row was a
  single pixel, which looked faint on a Retina display; it now keeps the same
  weight at every scale.
- **The running ring in a tab now turns smoothly.** It used to jump a twelfth
  of a turn every second, which read as stuttering; it now goes round at an
  even pace (twelve seconds a turn, twelve minutes once a command has run for
  an hour), and picks up at the right angle when a minimized or hidden window
  comes back. It still stands still under Reduce Motion.

### Fixed

- **Text inside a search match stays readable.** The highlight of a match
  (⌘F) used to leave the text on it in its own color, so a theme whose
  highlight was close to a text color could hide the very word you searched
  for. Text inside a match is now held to the minimum contrast against the
  highlight, the way selected text already is against the selection.

- **An ssh prompt no longer sits on the dock's line.** On a remote session the
  dock is only the folder row, and the screen came down to fill the room the
  input row left — all the way, so the remote prompt and its cursor touched the
  line above the dock. The screen now stops the dock's own margin short of
  that line, the same space that lies below it, and gives up no row for it.

- **Letters no longer turn into boxes after a long session.** A tab that had
  shown many different characters — hours of Chinese, Japanese or Korean
  text, say — could start drawing every character it met for the first time as
  an empty box, and kept doing so until the font size changed or bateri was
  restarted. When the tab runs out of room for new characters it now makes
  room and draws the screen again, so every character keeps its shape.

## [0.11.0] - 2026-10-10

### Added

- **Move a split anywhere with Option-Command.** Hold Option-Command for a
  moment and the panes of the tab lift, each with a small bar on top. Grab the
  bar — or, with the keys held, any point of a pane — and drop it beside
  another pane, on any side of it, or in its middle to swap the two; along the
  window's edge it becomes a full-height column or a full-width row. Before you
  let go, the place it would take is painted with its name (Left, Right, Above,
  Below, Swap). If it does not fit, room is made by shrinking the panes around
  it, none below 20 columns by 5 rows; a place that is still too small says so
  and shows the edges where it would fit. Esc puts it back. The bar also splits
  the pane right or down, opens it in a tab of its own and closes it.
- **Across tabs and windows.** Drop a pane on another tab's chip to add it to
  that tab, or hold it over the chip for a moment to open the tab and place it
  yourself. Drop it between two chips to make it a tab of its own. Drag it out
  of the window and it opens in a new window where you let go; drop it on
  another bateri window's panes or tabs and it moves there. The program in the
  pane keeps running — a build, vim, an ssh session — and is resized once, when
  it lands.
- **Pull a tab into the one in front.** With Option-Command held, drag a tab's
  chip down onto the open panes: the whole tab comes in as splits and keeps its
  own layout. Without the keys, dragging a chip down still opens it in a new
  window. A tab's right-click menu does the same with Merge into Current Tab ▸
  Split Right / Split Down.
- **From the menu and the keyboard.** Window ▸ Swap Split
  (Shift-Option-Command-arrow), Move Split to New Tab, Move Split to Tab ▸,
  Move Split to New Window, and Move Split to Previous / Next Tab
  (Shift-Option-Command-[ and ]).
- **Undo Move.** Edit ▸ Undo Move (Command-Z) takes back the last move, swap,
  merge or split-off, and a tab name lost in a merge comes back with it.
- **A tab says what the program in it is doing.** Programs that report their
  status (the `OSC 7501` protocol, which Claude Code speaks) now drive the tab's
  chip: the ring turns only while the program works and stands still while it
  waits for you, a question mark shows on a tab you are not looking at when the
  program is blocked on you — a permission, a question — and a green tick or
  red dot waits for you when it finishes or fails. Before, the ring turned for
  as long as the program was open. Progress bars printed with `OSC 9;4` (cargo,
  zig) turn the ring the same way, until a program starts reporting its own
  status. Works over ssh too.
- **A choice of how split tabs look.** Settings ▸ Appearance ▸ Split style
  (`[appearance] split_style`) keeps the new cards by default; "Lines" lets the
  panes touch with a one-pixel line between them, square corners and the plain
  background — the most room for text, as before this release.

### Changed

- **Split tabs are cards.** Two or more panes in a tab now stand as cards on a
  ground of their own: a soft gradient drawn from your theme's accent and
  cursor colors, a gap round and between the cards, corners rounded to match
  the window's. No line draws them — each card's top edge catches a light, a
  dark theme's card has a faint sheen under its top, and its shadow is tucked
  under its foot. The focused card stands nearer, with a deeper shadow and, on
  a dark theme, a warm light. Panes without the focus are no longer dimmed, so
  every pane reads at once; Settings ▸ Appearance ▸ Dim unfocused splits
  (`[appearance] dim_unfocused_splits`) brings the veil back. Going from one
  pane to two and back slides them into place, at once with Reduce Motion. A
  tab with one pane looks as it always has.
- **Full-screen programs reach the pane's edges.** btop, vim and other
  programs that paint their own background now fill the margin round the text
  too, instead of standing in a frame of the theme's color that was a
  different width on every side. Your shell's own lines look as before.

## [0.10.0] - 2026-10-09

### Added

- **The ports your programs listen on, in the dock.** Start a dev server —
  `npm run dev`, `rails s`, `python -m http.server` — and its port shows at
  the right of the dock's bottom line: `↗ :3000`. Every program started in the
  tab counts, not only the one in front: a job sent to the background, a
  server's worker that does the listening. Click the ports to list them with
  the program behind each; choose one to open it in your browser, or hold
  Option to copy its address. Command-click a port to open it at once. Shell ▸
  Open Port lists the same ports, also while vim or an agent covers the dock.
  Turn it off in Settings ▸ General ▸ Show listening ports (`[shell] ports` in
  `settings.toml`).
- **The server's ports too, over ssh.** In an ssh session with the shell
  integration on, the ports that the programs you start on the server listen
  on show beside this Mac's. A port this Mac reaches as it is shows green and
  opens directly. One that listens only on the server itself — a dev server's
  usual `127.0.0.1` — shows dim, and opening it forwards it through your ssh
  connection to the same port here first. The forward ends with the ssh
  session.

### Fixed

- **The load indicator holds still.** In an ssh session the indicator moved
  sideways whenever a `cpu` or `mem` number gained or lost a digit (`9%` →
  `10%`). Each number now has a column of its own, so the line moves only when
  a value turns critical.
- **Text stays readable on a light theme.** A program set up for a dark
  terminal printed its text white, which vanished on `bateri-light` and
  `linen`; the light themes' own white and dim colors read poorly too. Text a
  program colors too close to its background is now drawn darker or lighter
  until it reaches a 3:1 contrast. Only the text changes: backgrounds a program
  paints keep their color, and box, block and Powerline characters are left as
  they are. Choose how strict it is, or turn it off, in Settings ▸ Appearance
  ▸ Minimum contrast (`[appearance] minimum_contrast` in `settings.toml`).

## [0.9.0] - 2026-10-08

### Added

- **A new tab's dock now arrives instead of popping in.** Until the shell gives
  its first prompt the dock holds back its folder, branch, prompt mark and
  cursor, and when the prompt comes it arrives in one of three ways — pick it
  in Settings ▸ Motion ▸ Dock arrival (`[motion] dock_arrival` in
  `settings.toml`). `ripple` (the default): the dock's top line ripples faintly
  while the shell starts, the prompt mark drops onto it and a ring spreads from
  where it lands. `dust`: specks of dust drift around the dock, then are
  drawn left to right onto the top line, which is woven behind them. `type`:
  the dock rises a little, its lines are drawn from the left and the folder and
  branch type themselves out. `off` keeps today's dock. Nothing moves for the
  first moment, so a quick shell never flickers; a shell that takes longer than
  three seconds gets the arrival anyway; pressing a key, or leaving the window
  or tab, ends it at once; Reduce Motion turns it into a short fade and the
  cursor motion setting `snap` turns it off.

### Changed

- **No spinning ring beside full-screen programs.** vim, less, htop or an
  agent like Claude Code run for as long as they are open, so the ring that
  turns on a tab while a command runs no longer turns for them — it made a
  tab look as if it were still loading. Hover over the tab and the card still
  says how long the program has been open; when it ends while you are in
  another tab, the tab still shows a green tick or a red dot.

### Fixed

- **Dragging a tab moves the tab, not the window.** Dragging a tab moved the
  whole window instead, so a tab could not be put in a new place, pulled out
  into a window of its own or dropped on another window's tabs. Now the tab
  follows the pointer; dragging the empty part of the title row still moves
  the window, and Window ▸ Move & Resize works as before.
- **The Dock icon's menu keeps a steady width.** A window is listed there, and
  in the Window menu, by the title of what runs in it, and a program can set a
  whole sentence as its title — the menu grew as wide as that sentence. A long
  title now ends in "…" at a fixed width; the tab still shows its full title.

## [0.8.0] - 2026-10-08

### Added

- **Choose what text does at the top of the window.** Settings ▸ Appearance ▸
  Content edge (`[appearance] content_edge` in `settings.toml`) keeps the new
  fade, cuts the text under a thin line in the dividers' color, or cuts it
  where the pane ends as before. The last two give back the line some window
  heights lose to the fade.
- **A warm light theme, `linen`.** Warm ink on a cream background with a
  burnt-orange cursor and input mark. Pick it from View ▸ Theme ▸, or set
  `theme = "linen"` (or `light_theme = "linen"` to use it whenever macOS is in
  light mode).
- **Name your tabs, and scroll through as many as you open.** Double-click a
  tab (or choose Window ▸ Rename Tab…) and type: Return keeps the name, Esc
  leaves it, and emptying the field gives the tab back the title its shell
  sets. The name is kept when you quit, relaunch or update. When the tabs no
  longer fit they stop shrinking and the row scrolls — with the wheel or the
  trackpad — fading out at the edges, and a button beside `+` (Window ▸ Show
  All Tabs, ⇧⌘\) lists every tab, with the one you are on ticked and a dot
  for what is going on in it. Selecting a tab brings it into view.
- **Drag a tab to put it where you want it, or to give it a window.** Drag a
  tab along the row and the others make room; let go and it stays there. Pull
  it down into the window, or out of it, and it becomes a window of its own
  where you let go; drop it on another bateri window's row and it joins that
  window at the place you point to, with a gap opening there as you come over.
  Nothing restarts: the tab keeps its shell, its running command and any
  question it was asking.
- **Move a tab to a window of its own, or gather your windows into one.**
  Right-click a tab for Close Tab, Close Other Tabs (one question for all of
  them), Move Tab to New Window and Rename Tab…. Window ▸ Move Tab to New
  Window does the same for the tab you are on, and Window ▸ Merge All Windows
  puts every window's tabs into the current one. Nothing restarts: the tab
  keeps its shell, its running command and any question it was asking.

### Changed

- **Tabs in the title bar, in your theme's colors.** A window's tabs are now
  bateri's own: they sit in the title bar beside the traffic lights, drawn in
  the theme instead of the system's grey, with `+` at the right end. A window
  with one tab looks as before — its title in the middle. Click a tab to
  switch to it; middle-click it or use the × that appears on hover to close
  it. ⌘1–⌘9, ⇧⌘[ / ⇧⌘] and ⌃⇥ switch tabs as before, and ⌘N follows the
  system's "Prefer tabs" setting. A question a background tab asks — a
  password, an upload's confirmation — waits until you switch to that tab
  instead of covering the one you are looking at, and a question that is open
  stays with its tab when you switch away: it is there as you left it, half
  typed, when you come back. A tab with a question waiting shows a ? in a
  ring. With several tabs a settings error shows as a ⚠ beside `+`; hover
  over it to read it.
- **Each tab shows what goes on in it.** A ring turns while a command runs —
  on the far end of ssh too, where bateri's shell integration runs. A command
  that finishes while you are in another tab leaves a green tick, one that
  fails a red dot, until you look at that tab. An upload or download shows an
  arrow and fills a line along the tab's bottom, and a host you marked as
  production, staging or development colors a line along its top. Rest the
  pointer on a tab for a summary card: its folder, what its last command did
  and for how long, a transfer, the host. Hold ⌘ to see the key that reaches
  each tab. Tabs slide into place as they open and close; with Reduce Motion
  on everything changes at once and the ring stands still.
- **Text no longer gets cut in half at the top of the window.** Where the
  terminal meets the tab bar — or the divider above a split pane — a line
  scrolling up now fades out over a few pixels instead of being sliced through
  its letters, and lines from your history fade into that strip too. The
  first line of a screen at rest always stands fully readable below it. Full
  screen programs such as vim keep the strip empty. The strip is never
  shorter than the left margin, so at some window heights the terminal shows
  one line fewer than before.

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
