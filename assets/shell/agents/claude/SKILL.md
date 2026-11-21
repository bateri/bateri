---
name: bateri
description: You run inside a bateri terminal pane. Use the `bateri` command when the user asks to open a pane, split, tab or window, run something beside you, show them something too long for your reply (a whole diff, a log, a file to page through), look at what another pane shows, read a terminal's output, or arrange, rename, focus or close panes and tabs; and when you start a program that keeps running (a dev server, a watcher, a log tail) or one the user has to type into (a login, a password, a code).
---

# bateri

You run in a pane of bateri, a terminal app. The `bateri` command opens and arranges its panes and tabs and reads what they show. The panes are the user's: open what was asked for, and on your own only the three kinds below (When to open one yourself). Leave the focus where it is.

## The user's words

- "here", "beside you", "next to this", "on the side": a pane next to yours. `bateri split -- PROGRAM ARG...`, with `--left`, `--right`, `--above` or `--below` when a side was named.
- "in a new tab": `bateri tab -- ...`. "in a new window": `bateri window -- ...`. "floating", "over this": `bateri split --float -- ...`.
- "show me", "open the log" with no place named, for something too long for your reply: a floating card, see below.
- "what is open", "which tabs", "what runs in that pane": `bateri ls` (`--json` to read it).
- "what does that pane show", "what did it print": `bateri capture PANE` (`--history ROWS` for older lines).
- "type this there", "press enter there": `bateri send PANE TEXT`, `bateri send-key PANE KEY...`.
- "tell me when it is done": `bateri wait PANE`.
- "what does the page say", "click that", "go to that address", for a web page a program shows in a pane: `bateri browser text PANE` (also `snapshot`, which names elements as `[12]`, and `shot`, a picture), `bateri browser click PANE [12]`, `type PANE TEXT --into [12]`, `open PANE URL`, `back`, `reload`; `ls` lists the pane's browsers, and `--element ID` picks one of several. A long text or a picture is a file: the command prints where.
- close, rename, focus, zoom, swap, move, resize, minimize: `bateri close`, `rename`, `focus`, `zoom`, `swap`, `move`, `resize`, `minimize`.

"Here" and "beside you" mean your own pane. Never pick another pane for them. If your own pane floats, a plain `split` beside it is refused (exit 64): use `--float` or `bateri tab`.

## When to open one yourself

### A program that keeps running

A dev server, a watcher (`npm run watch`, `cargo watch`), a `tail -f`, a tunnel: anything that runs past this turn. Run it in a minimized floating pane, not in the background of your own shell, so the user can open it with a click and you can still read it:

1. Run `bateri ls --json` first. If a pane has the label you would give (the kind of program and the name of the project folder, short, since a label is at most 64 bytes: `dev server · api`) and its `directory` is the folder you would start it in, it is yours. If its `program` is the server, use it: a second dev server would fight the first for its port. If only a shell is left (the server stopped), close that pane and open a new one. A pane with the same kind of label from another project is not yours to use.
2. One command: `bateri split --float --minimized --label 'dev server · <folder name>' --json -- npm run dev`. The pane is born as a capsule, so no card shows over the user's work, and it stays open when the program ends. Do not run `minimize` after it: `minimize` opens a capsule again.
3. Tell the user in one line what runs in the capsule, by its label.
4. Read it with `bateri capture PANE` (`--history ROWS` for more). The pane keeps a shell after the program, so `bateri wait` does not return when the program ends and its exit status is not kept: read the end of the output instead.
5. When the work is done, close what you opened (`bateri close pane PANE`), or tell the user it is left running. The pane outlives your session. Never close a pane you did not open.

`--minimized` never goes with `--focus`: a capsule the keyboard is given to opens. A program that fails leaves its output in the pane for you to read.

### A program the user has to type into

A login (`gh auth login`, `npm login`), a password (`sudo`), a one-time code, any prompt your own shell cannot answer. Say first, in one line, that the user has to type in a floating card. Then open it, not minimized, and hold until it ends: `bateri split --float --focus --wait -- gh auth login`. `--focus` is right here, because the user has to type and you said so; `--wait` gives you its exit status, and you go on from there.

### Something the user asked to see

"show me the whole diff", "open the log", with no place named, and too long for your reply (a diff of more than a screen, a log, a file to page through): an open floating card that stays when the program ends, `bateri split --float --keep-open -- git diff`, `bateri split --float --keep-open -- less app.log`. What fits in a few lines stays in your reply.

### Not in a pane

A short command, and one whose result you need now (tests you will read, `git status`, a build you wait for): run it in your own shell, as you would without bateri. A pane for it costs three commands for one and fills the user's screen.

## What follows `--`

The program and its arguments, started directly: no shell runs them in the pane, so an alias or a function of yours is not found there. Your own shell has already expanded `~`, globs and `$VAR` before `bateri` sees the words, and a `|`, `&&` or `>` you type after them belongs to the `bateri` command, not to the program. For shell syntax in the pane, run a shell and quote its script: `bateri split -- sh -c 'cd app && npm test | tail'`.

Without a program the pane is the user's login shell. A program starts in your folder; `--cwd DIR` starts it elsewhere.

## When the program ends

The pane closes. `--keep-open` leaves a shell after the program, so its output stays on screen: use it when the user will read the result. `--wait` holds your command until the program ends and gives you its exit status: use it when you need the result yourself. Otherwise the command returns at once.

Opening a pane does not move the focus. `--focus` does; use it only when the user asked to be taken there, or for a program they have to type into.

## After you open one

The command prints the new pane's id (`p9`). Keep it for `capture`, `send`, `wait`, `minimize` and `close`; a pane of another machine is `p9@prod`. A command that timed out is not a failure: the pane may exist. Run `bateri ls` before you run it again, so a second pane does not open.

`-S SERVER` opens a pane on a server, where the user allows it.

## Not for you

`bateri message` is for hooks, and `bateri package` manages the programs the user keeps: leave both alone unless the user asked.

`bateri --help` lists every flag.
