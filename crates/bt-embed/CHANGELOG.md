# bt-embed changelog

Changes to bateri's embedding interface — the C header
`include/bt_embed.h` and the library behind it — for applications that host
bateri's terminal pane. bateri's own `CHANGELOG.md` is for people running
bateri and appears in its update window; this one is for those who link the
pane.

A published function never changes its signature or meaning. A new need is a
new function. A function on its way out stays at least one release and is
listed here under **Going**, with what replaces it.

## [Unreleased]

### Added

- The first version of the interface (`bt_embed_abi_version()` answers 1).
  A host builds a pane's configuration with setters: identity, settings and
  theme as text, the working directory, extra environment, a first command
  and an event handler. It opens the pane inside an `NSView` of its own,
  starts it, writes or pastes to it, reads its title, directory and identity,
  changes its theme and closes it.
- Events reach the handler on the main thread, tagged with the pane's id:
  title, shell exit, focus, activity, transfers, notifications, notices,
  files dragged over, Option-Command presses and questions. An event is read
  with getters, so later fields arrive as new functions.
- The pane follows its window by itself: key state, occlusion, screen scale,
  and a question waiting behind the window's own.
- The layout engine: the rules bateri's own windows follow for moving panes
  and tabs, for a host that keeps its own model of windows, tabs and splits.
  The host paints a picture of what it holds (`BtWorld`) and asks it as many
  questions as it likes:
  - a plan for a move (`bt_plan_new`) in flat, tagged steps;
  - a verdict for a pane carried under the pointer (`bt_verdict_new`);
  - a tab's frames (`bt_layout_new`);
  - a divider dragged, a resize, or a swap that keeps every pane at its
    smallest size.
  Undo Move is a move of its own, built from the record a plan leaves.
- How a host reads a plan. Steps come in two parts. A **main** step whose
  kind the host does not know must make it refuse the whole plan, since
  skipping one would leave its model wrong. An **after** step whose kind it
  does not know may be skipped. Later releases may add kinds under this
  rule.
- Lent values. A tree, name or strip a plan lends lives exactly as long as
  the plan; a tree a verdict lends, as long as the verdict.
- Identities stay the host's. A tab a move makes takes the picture's next
  identity, then the one after it, and the plan says how many it used.
- `bt_world_tree_fits`: whether a tree keeps every pane of a tab at its
  smallest size. bateri checks it once more before taking a new tree for a
  tab, including a pane carried within its own tab, and a host does the same.
- Split trees as versioned text (`bt_tree_encode` / `bt_tree_decode`, format
  version 1). Every later release reads every earlier version, so a layout
  kept on disk stays readable.
- A strip's rules (`BtStrip`): where a new tab goes, which tab comes up when
  one closes, Show Next/Previous Tab, Command-1 to 9.
- `bt_pane_min_size`: a terminal pane's smallest size, for the engine.
- Command events, which need the shell integration:
  `BT_EVENT_COMMAND_STARTED` and `BT_EVENT_COMMAND_FINISHED`, carrying the
  command's line, whether it ran in our remote shell, when it started, and
  at its end its exit code and duration (`bt_event_exit_code`,
  `bt_event_duration_ms`, `bt_event_started`). A start the host did not see
  is told before the end. Only a command that came and went between two
  activity edges (typeahead) is missed.
- `BT_EVENT_DIRECTORY`: the shell's directory, with the host in a remote
  session. `BT_EVENT_PORTS`: the listening ports changed.
- Queries: the shell's phase (`bt_pane_phase`), the foreground programs
  (`bt_pane_foreground`), the remote host and directory, and the listening
  ports.
- `BT_EVENT_PROGRAM_STATUS`: what a program that reports its status (`OSC
  7501`, or `OSC 9;4` for the root record) says it is doing. Each record
  that changed, came or went is told with its id, state (`BT_PROGRAM_*`) and
  progress (`bt_event_progress`). The records can also be queried
  (`bt_pane_program_count` and the getters beside it).
- `BT_EVENT_OPEN_LINK`: the user opened a link (Command-click, or the link
  menu's Open). The event carries a URL as written, or a file or directory
  that exists, local or on the remote session's server, with the line and
  column a `path:12:5` named (`bt_event_line`, `bt_event_column`). A handler
  that opens it itself calls `bt_event_set_handled`, and the pane does
  nothing more. Otherwise the pane opens it as bateri does.
- `BT_EVENT_SHELL_EXITED` carries an exit code. It is the code of the
  shell's login process, and on macOS `login` answers 0 whatever the shell's
  own code was: it tells a normal exit from a killed one, not the shell's
  status.
