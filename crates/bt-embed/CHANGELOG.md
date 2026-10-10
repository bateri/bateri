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
