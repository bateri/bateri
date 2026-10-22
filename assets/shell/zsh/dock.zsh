# bateri's zsh dock — the part of the wrapper that is the same on this computer and on a server.
#
# `source`d at the top level (the `typeset` rule in `bateri.zsh`'s header) by the LOCAL wrapper
# (`bateri.zsh`) and by the REMOTE one (`remote/zsh/bateri.zsh`, written to a server beside this
# file), and by nothing else; it defines functions and prints nothing. What differs between the
# two is one identity, and it is a parameter:
#
#   local   the block's anchor is `bateri://block/<n>`, the dock's events are `8133;<letter>`;
#   remote  the anchor is `bateri://rblock/<P>.<S>.<n>` and every event is
#           `8133;bt_remote=<P>.<S>.<n>;<letter>` (and the end mark `133;B;bt_remote=…`), so the
#           terminal can tell its own server shell's prompt from any other program's 8133.
#
# The identity is `__bateri_rshell` (`<P>.<S>`: the local `ssh` block and this shell's pid), which
# only the remote wrapper sets, and `__bateri_block`, the prompt counter both wrappers keep; with
# `__bateri_rshell` empty every byte printed here is the local wrapper's.
#
# Everything else — the mirror, the edit widget and its binding, the prompt's contract, the
# order of the hooks — is written once, here: two copies would part the day one of them changes,
# and nothing but a hand-typed click would show it.
#
# The caller sets `__bateri_anchor` (the OSC 8 opening of a prompt's block, `%9v` standing for
# the block's counter) and `__bateri_dock` before it calls [`__bateri_dock_setup`]; the remote
# wrapper loads this file only where it can use it (zsh 5.3 for `add-zle-hook-widget`).

# LET AN OVERFLOWING COMPLETION LIST NOT ERASE THE SCREEN. Called once per shell, by the wrapper
# that has a dock. zsh's default criterion is `LISTMAX=100` and it looks at the NUMBER OF
# OPTIONS, not the SPACE it takes: a list under a hundred is printed without asking, even if
# the line count exceeds the screen. The symptom was seen by the user (`ls -` completion): the
# list was printed without asking, overflowed the grid and STAYED on screen when the line was
# erased. `0` turns the criterion from a count into a space: "ask if it does not fit on the
# screen". How many lines how many options take depends on the user's `zstyle` and no number is
# written here — the criterion is not a number anyway.
#
# The rationale was measured (pure PTY, the same 37-line list at two screen heights): when the
# list FITS (60 lines) zsh on Tab moves the cursor above the list with `\e[37A` and sends `\e[J`
# when the line is erased — the fill band fills the gap from the scrollback and the screen
# returns to its pre-Tab state. When the list OVERFLOWS (26 lines) zsh sends neither; on line
# erase only a backspace arrives, because in a normal terminal it cannot bring back the lines
# that scrolled into the history and half-clearing would corrupt the screen. No signal that
# tells the terminal "the list is over" ever arrives: the scrollback is with us but there is no
# trigger to bring it back. `LISTMAX=0` stops erasing the screen from being an irreversible step
# — for an overflowing list zsh asks first and `n` leaves the screen as it is.
#
# A KNOWN LIMIT: when `y` is answered the list is printed and is permanent again. The criterion
# is "ask before it breaks", not "bring it back".
#
# THE VALUE IS SET BEFORE THE USER'S FILES and the place itself is a decision: `LISTMAX` is SET
# by default in zsh (`typeset -i LISTMAX=100`), so "did the user set it" cannot be tested. The
# wrappers call this once from `.zshenv`, before any of the user's startup files is read; a user
# who writes their own `LISTMAX` runs LATER and wins. Had the value been put inside the hooks
# function the opposite would happen and the user's preference would be overridden.
#
# CONDITIONAL ON THE DOCK, because what it protects is the dock's promise (the return of the
# screen) and at the `integration = "blocks"` tier that promise does not exist — there the input
# line is the user's and the shell should behave classically. It is the third decision
# `__bateri_dock` consumes; had a separate key been opened an inconsistent state like "no dock
# but completion is ours" would have arisen.
__bateri_dock_start() {
  LISTMAX=0
}

# Sets the dock up for this shell; its caller is `__bateri_hooks`, AFTER the user's file is
# loaded, so what is registered here comes behind the user's own hooks.
#
# PS1 when the prompt is the TERMINAL's: two zero-width marks and **two real spaces**. The
# terminal itself draws `>` in the dock and in the grid, but in the grid a place is needed to put
# it — that place is these two columns.
#
# THE SPACES ARE NOT A DRAWING TRICK, THEY ARE REAL WIDTH. The alternative was to shift the
# command line two columns to the right while drawing it, and it would have broken three things
# at once: the mouse mapping would shift on that line, the last two characters of a full-width
# command would overflow the screen and zsh would miscalculate line wrapping. If the prompt
# really is two columns all three are right on their own — zsh already knows the prompt width.
#
# THE NUMBER MUST BE THE SAME AS `bt-core`'s `dock::TEXT_COL`: the dock's text also starts two
# columns after the mark and if the two diverge the grid and the dock start from different
# columns. The constant cannot be shared (one is zsh, one is Rust), so a test reads this line
# and ties the number.
#
# ORDER: anchor → spaces → `B`. `B` is the END of the prompt, i.e. where the input starts; the
# spaces must come before it. The anchor is first, because the spaces also carry the link it
# opens — and that is a side gain: even an empty prompt has an anchored cell, whereas a
# zero-width PS1 had none.
#
# `%{…%}` means "zero width"; the spaces are deliberately OUTSIDE, they must be counted. On a
# server `B` carries the shell's identity like the anchor does, with the same `%9v`.
#
# MIRROR: ZLE's display state goes to the terminal on every line draw.
#
# NOT `zle -N zle-line-pre-redraw`: that binding has a single owner and zsh-syntax-highlighting
# and zsh-autosuggestions both want the same widget — the last writer would drop the other
# (measured). Instead of `add-zle-hook-widget` it sets up a dispatcher and calls them all in
# order.
#
# THE GUARD IS IN THE HOOK ITSELF: `add-zle-hook-widget` does not add the same widget twice (it
# asks the `zstyle` list for containment), so the counterpart of the guard we wrote by hand for
# PS1's additions is ready here. Verified: registered twice, the
# `add-zle-hook-widget -L line-pre-redraw` list is a single line.
#
# A PLUGIN THAT REGISTERS AFTER US: `add-zle-hook-widget` appends, so our hook runs AFTER the
# user's plugins and sees their `region_highlight`/`POSTDISPLAY` contribution. This has two known
# limits and both are symptomless: (1) a plugin that defers its registration (zsh-defer) comes
# after us and its contribution reaches the mirror one draw LATE; (2) a plugin that says
# `zle -N zle-line-<hook>` overrides the dispatcher itself and everything tied to that hook —
# ours included — dies silently. Whichever of the three hooks is overridden, that arm goes quiet
# and the symptoms differ: if `line-pre-redraw` goes the dock freezes, if `line-init` goes the
# prompt stays empty for a moment, if `line-finish` goes a finished command hangs on in the
# mirror. `line-init` is the most likely — common in users' rc files for the cursor shape.
#
# `line-init` IS ALSO HOOKED and this is not decoration: `line-pre-redraw` runs only when the
# line CHANGES, not on the prompt's first (empty) draw — observed in a real session, the first
# mirror came only at the first keystroke. Without it the dock would stay dead at prompt time and
# would appear suddenly at the first letter.
#
# THE EDIT WIDGET: the terminal's only command goes here. The definition once, the binding on
# every `line-init` (`__bateri_dock_arm`) — and arm is registered BEFORE the mirror, so the
# capability is on the wire before the first mirror of the same prompt.
__bateri_dock_setup() {
  autoload -Uz add-zle-hook-widget
  typeset -g __bateri_ps1=$__bateri_anchor'  '$'%{\e]133;B'${__bateri_rshell:+;bt_remote=$__bateri_rshell.%9v}$'\a%}'
  zle -N __bateri_dock_edit
  add-zle-hook-widget line-init __bateri_dock_arm
  add-zle-hook-widget line-init __bateri_dock_redraw
  add-zle-hook-widget line-pre-redraw __bateri_dock_redraw
  add-zle-hook-widget line-finish __bateri_dock_finish
}

# The prompt the terminal draws a dock for, set at every prompt (and by [`__bateri_prompt_guard`]):
# entirely OURS and its visible width is zero. The user's prompt is not drawn; the dock's `>` mark
# takes its place. `RPS1`/`RPROMPT` are emptied too and this is not a detail but MANDATORY: the
# right prompt lives independently of PS1, resetting only PS1 would leave a theme fragment
# hanging on the right of the screen.
__bateri_dock_prompt() {
  PS1=$__bateri_ps1
  RPS1=
  RPROMPT=
}

# Takes back the prompt the theme wrote back; its caller is the mirror's ZLE hook.
#
# WHY PRECMD IS NOT ENOUGH: p10k and starship rebuild PS1 AFTER `precmd`, from their own ZLE
# hooks, and `zle reset-prompt` it — the value we wrote in precmd is erased from the screen. If
# it is not imposed from the same place the theme wins.
#
# WHY IT IS NOT ENOUGH ALONE EITHER — MEASURED, not chosen: a PS1 assigned from a ZLE hook does
# NOTHING by itself. The prompt is printed before `line-init` runs and zsh keeps the expanded
# form; in a `zsh -i` PTY probe the value assigned from the hook never appeared on screen. The
# only way for it to take effect is `zle reset-prompt`. So the two are needed TOGETHER: precmd
# gets the first print right, this hook takes back what the theme wrote back.
#
# TWO COSTS OF DROPPING PRECMD WERE MEASURED and both justify precmd: (1) with a theme that sets
# it only from `precmd` (starship) the prompt is already right on first print and the guard never
# resets — without precmd EVERY prompt would pay a `reset-prompt` redraw; (2) if a plugin that
# says `zle -N zle-line-init` overrides the dispatcher (the limit this file counts as most
# likely) this hook goes completely silent and the prompt would come back. The reverse is also
# true: had only precmd remained, p10k would write the prompt back.
#
# THE GUARD CUTS THE PING-PONG: `reset-prompt` spawns a new draw and that draw calls
# `line-pre-redraw` again. An unconditional reset would be a self-feeding loop; with the guard a
# SINGLE reset per prompt was measured.
#
# IT GOES SILENT IN THE DOCK-LESS ARM: there is no point fighting the theme of a user who wants
# their prompt back.
__bateri_prompt_guard() {
  (( __bateri_dock )) || return 0
  [[ $PS1 == "$__bateri_ps1" && -z $RPS1 && -z $RPROMPT ]] && return 0
  __bateri_dock_prompt
  zle reset-prompt
}

# ── ZLE's display mirror ─────────────────────────────────────────────────
#
# WIRE FORMAT (its decoder is `bt-core`'s `parse_dock`; the two change together). On a server
# every sequence below has `bt_remote=<P>.<S>.<n>;` in front of its letter (`8133;` + that +
# `u;…`), the local wrapper's has nothing:
#
#   ESC ] 8133 ; u ; CURSOR ; b64(PREDISPLAY) ; b64(BUFFER) ;
#                             b64(POSTDISPLAY) ; b64(region_highlight) ;
#                             b64(KEYMAP) ; b64(PREBUFFER) BEL
#   ESC ] 8133 ; e BEL   line finished (`line-finish`)
#   ESC ] 8133 ; o BEL   the display does not fit the mirror (the gate below)
#   ESC ] 8133 ; b ; b64(branch) BEL   the context line's branch (`precmd`; the local wrapper's)
#   ESC ] 8133 ; w BEL   the edit widget is bound in this prompt (`line-init`)
#
# REVERSE DIRECTION — FROM THE TERMINAL TO THE SHELL, the wire's two sequences:
#
#   ESC [ 8133 ~ d ; S ; E ; L BEL
#   ESC [ 8133 ~ r BEL   only mirror: behind a paste with line breaks;
#                        since `bracketed-paste-magic` pushes the payload onto the
#                        queue with `zle -U` and skips the redisplay, the mirror
#                        stayed stale for a key. The widget's behavior on every
#                        payload that is not `d` is already this: it prints the
#                        mirror without touching `BUFFER`.
#
# Delete the `[S, E)` character range of `BUFFER`, put the caret at `S`; `S == E` only moves the
# caret. `L` is the `${#BUFFER}` the terminal saw: if it does not match the terminal was looking
# at a stale mirror and the widget does NOTHING. The sequence comes from the same PTY as the
# user's input and ZLE reads it like a key; `CSI 8133 ~` is a key no keyboard produces, its
# number is the mirror's number. The text NEVER enters the wire: the letter typed in place of the
# selection comes after `d` by the usual path, i.e. there is no base64 decoder in the shell and
# the letter still passes through `self-insert`. The terminal sends the sequence only if it has
# seen `w` in this prompt — in a shell without the binding the trailing BEL would be
# `send-break` (measured).
#
# KEYMAP IS THE SIXTH BODY and what it carries is not a POLICY but ZLE's state: the side that
# decides which keymaps mean "a typed key turns into text" is the terminal (`bt-core`,
# `insert_keymap`). We send the name as is, because the user can create their own keymap with
# `bindkey -N` and there is no information at this end to classify it. base64, because that name
# may contain `;`.
#
# PREBUFFER IS THE SEVENTH BODY: the earlier lines of a multi-line command that ZLE no longer
# edits (`for`, heredoc, `\`-continuation). Appended at the end, because the wire only grows at
# the end — the decoder reads it as optional, so a window running with an old script is decoded
# too. It is part of the display, i.e. it enters the overflow gate's total.
#
# THE BODIES are base64: the text the user typed can contain `;`, `ESC` and C0 bytes and all
# three break the sequence's framing. None of the three is in base64's alphabet.

# The base64 alphabet, in index order.
typeset -ga __bateri_b64_table
__bateri_b64_table=( {A..Z} {a..z} {0..9} + / )

# The longest display the mirror will carry, in CHARACTERS.
#
# The number was derived, not chosen — and it is the other end of the SAME budget as the terminal
# side's `DOCK_PAYLOAD_LIMIT` (64 KiB): that limit came out of the arithmetic "4096 characters ×
# worst-case 4 bytes of UTF-8 × base64's 4/3 inflation", this is its form in characters.
#
# WHY THIS END ALSO HAS A GATE: encoding is pure zsh and its cost is linear in the input length —
# and it is paid on EVERY KEYSTROKE. Without the gate a pasted block would be spent on encoding a
# payload the terminal will reject anyway, i.e. we would pay the cost and get nothing back. On
# overflow the mirror says "I cannot show it" and the input line stays in the grid; the user
# still sees what they typed.
typeset -g __bateri_dock_limit=4096

# Converts `$1` to base64; the result is in `REPLY`. The local wrapper's context line uses it
# too (`__bateri_branch_print`).
#
# NO FORK: encoding runs per keystroke and spawning a `base64` process would be the most
# expensive item of this path. `nomultibyte` makes every element a BYTE — base64 is an encoding of
# bytes, not of characters.
#
# THE BYTE VALUE IS TAKEN INTO A SCALAR FIRST (`x=$bytes[i]`, then `#x`), NOT directly with
# `##${bytes[i]}`: the arithmetic `##` form interprets escape sequences and the backslash byte
# (`\`) was read as 32 instead of 92 — a silent corruption for a byte that is common on command
# lines.
#
# PADDING IS NOT PRINTED: the decoder reads padded and unpadded bodies alike (`decode_base64`'s
# doc) and not printing saves a few bytes per keystroke.
__bateri_b64() {
  emulate -L zsh
  setopt nomultibyte
  REPLY=
  [[ -n $1 ]] || return 0
  local -a bytes
  bytes=( ${(s::)1} )
  local -i n=$#bytes i v rest
  local out= x y z
  for (( i = 1; i <= n; i += 3 )); do
    rest=$(( n - i + 1 ))
    x=$bytes[i]
    v=$(( #x << 16 ))
    if (( rest > 1 )); then
      y=$bytes[i+1]
      v=$(( v | (#y << 8) ))
    fi
    if (( rest > 2 )); then
      z=$bytes[i+2]
      v=$(( v | #z ))
    fi
    out+=${__bateri_b64_table[$(( (v >> 18 & 63) + 1 ))]}
    out+=${__bateri_b64_table[$(( (v >> 12 & 63) + 1 ))]}
    (( rest > 1 )) && out+=${__bateri_b64_table[$(( (v >> 6 & 63) + 1 ))]}
    (( rest > 2 )) && out+=${__bateri_b64_table[$(( (v & 63) + 1 ))]}
  done
  REPLY=$out
}

# Prints ZLE's display state to the mirror; its hooks are `line-init` and
# `line-pre-redraw` (the first is the prompt's first draw, the second every change).
#
# FIVE VARIABLES, if one were missing the mirror would show less than the user
# sees: `POSTDISPLAY` is autosuggestions' suggestion, `region_highlight` is
# syntax highlighting's color.
#
# `emulate -L zsh` IS MANDATORY: the body runs with the user's options and what
# follows depends both on array indexing (`KSH_ARRAYS`) and on the multibyte
# `${#...}` count. `-L` is function-local, undone on return.
#
# `REPLY` IS LOCAL: it is a variable that lives in the user's namespace and our
# hook runs in the middle of their line editing.
__bateri_dock_redraw() {
  emulate -L zsh
  # IMPOSING THE PROMPT, BEFORE the mirror and from the same hook: the rationale
  # is in `__bateri_prompt_guard`'s header. Above the mirror's own payload gate,
  # because the prompt's ownership does not depend on the payload's length — had
  # the theme's prompt come back while the mirror stays silent on an overflowing
  # line the symptom would be unexplainable too.
  __bateri_prompt_guard
  # Records are separated by line breaks; the decoder reads the body with `lines()`.
  # The join is BEFORE the gate, because the fourth body is also subject to the gate.
  local REPLY entries=${(F)region_highlight} pre buf post highlights keymap prebuf
  # The server's identity, in front of the letter (the wire header above).
  local tag=${__bateri_rshell:+bt_remote=$__bateri_rshell.$__bateri_block;}
  # The gate is BEFORE ENCODING, because its whole meaning is avoiding encoding —
  # and it measures all FIVE bodies at once (`PREBUFFER` is in the total: it
  # is part of the display and the earlier lines of a pasted loop can
  # exceed the limit by themselves). `region_highlight` is counted separately, it
  # does not enter the total: syntax highlighting leaves one record per token, so
  # on a long line it is of the same order as the text itself and can exceed the
  # limit **on its own** (`DOCK_PAYLOAD_LIMIT`'s derivation also counts it as a
  # separate term beside the text).
  if (( ${#PREBUFFER} + ${#PREDISPLAY} + ${#BUFFER} + ${#POSTDISPLAY} > __bateri_dock_limit
        || ${#entries} > __bateri_dock_limit )); then
    print -nr -- $'\e]8133;'$tag$'o\a'
    return 0
  fi
  __bateri_b64 "$PREDISPLAY"; pre=$REPLY
  __bateri_b64 "$BUFFER"; buf=$REPLY
  __bateri_b64 "$POSTDISPLAY"; post=$REPLY
  __bateri_b64 "$entries"; highlights=$REPLY
  __bateri_b64 "$PREBUFFER"; prebuf=$REPLY
  # KEYMAP is counted OUTSIDE the gate: the longest keymap name is a handful of
  # bytes and adding it to the payload budget would add a second reason for the
  # mirror to go silent on a line that overflows the limit.
  __bateri_b64 "$KEYMAP"; keymap=$REPLY
  # `$CURSOR` IS A CHARACTER offset and the wire wants characters too — the form
  # counted from the start of `BUFFER` goes as is, the shift by `PREDISPLAY` is
  # made the other side of the boundary's job (`DockState::cursor`'s doc).
  print -nr -- $'\e]8133;'$tag$'u;'$CURSOR';'$pre';'$buf';'$post';'$highlights';'$keymap';'$prebuf$'\a'
}

# `line-finish`: ZLE let go of the line, the mirror closes.
#
# WITHOUT IT the last `BUFFER` would hang on: after Enter the dock would keep
# showing the running command's line.
__bateri_dock_finish() {
  emulate -L zsh
  print -nr -- $'\e]8133;'${__bateri_rshell:+bt_remote=$__bateri_rshell.$__bateri_block;}$'e\a'
}

# The upper limit of waiting for the edit command's payload, in SECONDS.
#
# The terminal sends the sequence in a single write, so by the time the widget
# runs the payload is already waiting to be read and no time is spent. The limit
# only determines, on a CORRUPT wire — a sequence whose BEL never arrives — how
# long ZLE will freeze: a moment the user will notice but that will not lock the
# shell. Not a measured number, a feel threshold (precedent `HANDOVER_HOLD`).
typeset -g __bateri_dock_edit_wait=0.5

# The terminal's edit command (`CSI 8133 ~ d;S;E;L BEL`, wire header above).
#
# SILENT IN EVERY CASE: a corrupt payload, an `L` that does not match or an
# out-of-range number returns without touching `BUFFER`. The wrong direction is
# "no edit happened" — let the user's key go to waste rather than corrupt the
# line.
#
# `S == E` DOES NOT WRITE TO BUFFER, only `CURSOR`: even an empty assignment
# would spawn an undo record. Deletion is ZLE's single undo unit (measured).
#
# THE MIRROR IS EXPLICITLY PRINTED AT THE END OF THE WIDGET: `line-pre-redraw`
# runs only when the display changes and a command that puts the caret where it
# already was (or does nothing because `L` did not match) would never spawn a
# mirror. The terminal expects an answer to every input (`DockState::answers`); a
# command left unanswered would keep the edit gate closed until the next key.
__bateri_dock_edit() {
  emulate -L zsh
  local payload= ch=
  while read -k 1 -t $__bateri_dock_edit_wait ch; do
    [[ $ch == $'\a' ]] && break
    payload+=$ch
    # A legitimate payload is four numbers: reading without limit would swallow the line on a corrupt wire.
    (( ${#payload} > 64 )) && break
  done
  if [[ $ch == $'\a' && $payload == d\;<->\;<->\;<-> ]]; then
    local -a field
    field=( ${(s:;:)payload} )
    local -i start=$field[2] end=$field[3] len=$field[4]
    if (( len == ${#BUFFER} && start <= end && end <= len )); then
      if (( start < end )); then
        BUFFER=${BUFFER[1,start]}${BUFFER[end+1,-1]}
      fi
      CURSOR=$start
    fi
  fi
  __bateri_dock_redraw
}

# `line-init`: bind the widget and report the capability.
#
# ON EVERY PROMPT AGAIN, because the binding is not persistent: `bindkey -v`/`-e`
# binds a new keymap to `main`, a deferred plugin can reset the keymap, and the
# keymap of a user who says `bindkey -A mymap main` is born after our setup.
# All three are repaired at the next prompt. The cost is three builtins — no
# fork.
#
# `main` INCLUDED: it is common for users to bind their own keymap to `main` and
# binding to `emacs`/`viins` would not cover it. The terminal sends the sequence
# only in the insert keymap (`INSERT_KEYMAPS`), so `vicmd` is deliberately out.
#
# `w` AFTER THE BINDING: the capability means "bound right now". The terminal
# forgets it at `line-finish` (`e`), so in a session whose script is old or
# whose hook was overridden the gate stays closed and the sequence is never sent.
__bateri_dock_arm() {
  emulate -L zsh
  local map
  for map in main emacs viins; do
    bindkey -M $map $'\e[8133~' __bateri_dock_edit
  done
  print -nr -- $'\e]8133;'${__bateri_rshell:+bt_remote=$__bateri_rshell.$__bateri_block;}$'w\a'
}
