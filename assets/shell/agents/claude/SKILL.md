---
name: bateri
description: You run inside a bateri terminal pane. Use the `bateri` command when the user asks to open a pane, split, tab or window, run something beside you, look at what another pane shows, read a terminal's output, or arrange, rename, focus or close panes and tabs.
---

# bateri

You run in a pane of bateri, a terminal app. The `bateri` command opens and arranges its panes and tabs and reads what they show. The panes are the user's: open what was asked for, and leave the focus where it is.

## The user's words

- "here", "beside you", "next to this", "on the side": a pane next to yours. `bateri split -- PROGRAM ARG...`, with `--left`, `--right`, `--above` or `--below` when a side was named.
- "in a new tab": `bateri tab -- ...`. "in a new window": `bateri window -- ...`. "floating", "over this": `bateri split --float -- ...`.
- "what is open", "which tabs", "what runs in that pane": `bateri ls` (`--json` to read it).
- "what does that pane show", "what did it print": `bateri capture PANE` (`--history ROWS` for older lines).
- "type this there", "press enter there": `bateri send PANE TEXT`, `bateri send-key PANE KEY...`.
- "tell me when it is done": `bateri wait PANE`.
- close, rename, focus, zoom, swap, move, resize: `bateri close`, `rename`, `focus`, `zoom`, `swap`, `move`, `resize`.

"Here" and "beside you" mean your own pane. Never pick another pane for them.

## What follows `--`

The program and its arguments, started directly: no shell runs them in the pane, so an alias or a function of yours is not found there. Your own shell has already expanded `~`, globs and `$VAR` before `bateri` sees the words, and a `|`, `&&` or `>` you type after them belongs to the `bateri` command, not to the program. For shell syntax in the pane, run a shell and quote its script: `bateri split -- sh -c 'cd app && npm test | tail'`.

Without a program the pane is the user's login shell. A program starts in your folder; `--cwd DIR` starts it elsewhere.

## When the program ends

The pane closes. `--keep-open` leaves a shell after the program, so its output stays on screen: use it when the user will read the result. `--wait` holds your command until the program ends and gives you its exit status: use it when you need the result yourself. Otherwise the command returns at once.

Opening a pane does not move the focus. `--focus` does; use it only when the user asked to be taken there.

## After you open one

The command prints the new pane's id (`p9`). Keep it for `capture`, `send`, `wait` and `close`; a pane of another machine is `p9@prod`. A command that timed out is not a failure: the pane may exist. Run `bateri ls` before you run it again, so a second pane does not open.

`-S SERVER` opens a pane on a server, where the user allows it.

## Not for you

`bateri message` is for hooks, and `bateri package` manages the programs the user keeps: leave both alone unless the user asked.

`bateri --help` lists every flag.
