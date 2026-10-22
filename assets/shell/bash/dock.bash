# bateri's bash dock — the part of the wrapper that is the same on this computer and on a server.
#
# Read (`.`) by the LOCAL wrapper (`bateri.bash`) and by the REMOTE one (`remote/bash/bateri.bash`,
# written to a server beside this file), and by nothing else; it defines functions and prints
# nothing. What differs between the two is one identity, and it is a parameter:
#
#   local   the block's anchor is `bateri://block/<n>`, the dock's events are `8133;<letter>`;
#   remote  the anchor is `bateri://rblock/<P>.<S>.<n>` and every event is
#           `8133;bt_remote=<P>.<S>.<n>;<letter>` (and the end mark `133;B;bt_remote=…`), so the
#           terminal can tell its own server shell's prompt from any other program's 8133.
#
# Everything else — the edit widget, its binding, the prompt's contract, the order of the hooks —
# is written once, here: two copies would part the day one of them changes, and nothing but a
# hand-typed click would show it.
#
# It needs bash 4.4 (`READLINE_LINE`, `bind -x`); the wrappers load it only where they run.

# The terminal's edit command (`CSI 8133 ~ d;S;E;L BEL`): delete the characters `[S, E)` of the
# line and put the caret at `S`, if the line is `L` characters long. A click in the dock is
# `S == E`; typing or deleting over a selection is `S < E`. `r;…` (the refresh a paste sends a
# shell that mirrors its line) is not ours to answer.
#
# SILENT IN EVERY CASE: a corrupt payload, an `L` that does not match the line (the terminal
# read it from the grid a moment ago and the user has typed since) or numbers out of range
# return without touching the line. The wrong direction is "no edit happened".
#
# The payload follows the sequence in the same write, so it is already waiting to be read; the
# limit only bounds how long a corrupt wire (a payload whose BEL never comes) can freeze the
# line. Not a measured number, a feel threshold.
#
# CHARACTERS: the payload counts them and `${#READLINE_LINE}` does too, in a UTF-8 locale. So
# does `READLINE_POINT` from bash 5.0; bash 4.4 counts bytes there, so the character index is
# turned into a byte offset first (`LC_ALL=C` makes `${#…}` count bytes).
#
# `$?` is handed back: the widget runs between a prompt and the command the user has not typed
# yet, and the command's own status is what `$?` should still say.
#
# `$1` is `move` where the line is not being typed into (vi's command mode): a click is
# followed there, a deletion is not, because the key that comes after the terminal's deletion
# would be a command and run on the line the terminal had already changed. The terminal does not
# know the keymap, so the widget has to.
#
# A payload that does not end in its BEL (a corrupt wire, a stall) leaves the line alone, but
# the bytes already on the way are read by readline as keys; the same is true of the zsh widget.
__bateri_dock_edit() {
  local status=$? ch= payload= head
  local -i only_move=0
  [[ ${1-} == move ]] && only_move=1
  local re='^d;([0-9]{1,9});([0-9]{1,9});([0-9]{1,9})$'
  while IFS= read -r -n1 -t 0.5 ch; do
    [[ $ch == $'\a' ]] && break
    payload+=$ch
    # A real payload is four short numbers: reading without a limit would swallow the line.
    (( ${#payload} > 64 )) && break
  done
  if [[ $ch == $'\a' && $payload =~ $re ]]; then
    local -i start=10#${BASH_REMATCH[1]} end=10#${BASH_REMATCH[2]} len=10#${BASH_REMATCH[3]}
    if (( len == ${#READLINE_LINE} && start <= end && end <= len && (start == end || !only_move) )); then
      head=${READLINE_LINE:0:start}
      if (( start < end )); then
        READLINE_LINE=$head${READLINE_LINE:end}
      fi
      if (( BASH_VERSINFO[0] < 5 )); then
        local LC_ALL=C
        READLINE_POINT=${#head}
      else
        READLINE_POINT=$start
      fi
    fi
  fi
  return $status
}

# Binds the widget, in every keymap, and says whether it is bound.
#
# AT EVERY PROMPT, because a binding is not persistent: `bind -f ~/.inputrc`, `set -o vi` and a
# theme's setup can each replace the keymaps. The cost is three builtins, no fork. The sequence
# is one no keyboard produces, so the user's own bindings are not touched. Every keymap is bound
# because an unbound sequence is typed: in vi's command mode `8133~` would be a count and a
# case toggle. There the widget only follows clicks (`move`, see `__bateri_dock_edit`).
__bateri_dock_arm() {
  local map how
  for map in emacs vi-insert vi-command; do
    how=
    [[ $map == vi-command ]] && how=' move'
    bind -m "$map" -x '"\e[8133~": __bateri_dock_edit'"$how" 2> /dev/null || return 1
  done
}


# The prompt the terminal draws a dock for, set at every prompt (`PS1`, and `PS2` empty): nothing of
# the user's. `$1` is the anchor (the OSC 8 target of this prompt's block), `$2` the remote shell's
# identity `<P>.<S>.<n>` — empty for the local shell.
#
# What bash draws is a contract (`grid_input.rs`):
#
#   anchor, two real spaces, `g`, [`w`], `B`
#
# - The anchor is the block's id-carrying OSC 8 link, laid over the cells and left open until
#   the command starts, so every cell readline draws for the line carries the block's id: the
#   terminal reads the line from the cells with that id, shows it in the dock and keeps it off
#   the grid.
# - The two spaces are real width, so the line starts at the third column, readline knows the
#   prompt's width, wraps and places the caret right.
# - `g` says "this prompt's line is read from the grid", `w` that the edit widget is bound right
#   now (a click in the dock moves the caret, typing over a selection deletes it). Both are in
#   `PS1` rather than printed from a hook: readline draws the prompt again without running
#   `PROMPT_COMMAND` (Ctrl-L, a resize, the return of a widget), and a mark printed from a hook
#   would be missing from that copy. The terminal forgets both at every `A`, which is why
#   they come after it.
# - `PS2` is empty: the lines of an unfinished command stay under the same open link, so they
#   go to the dock with the first one, and an empty continuation prompt puts no `> ` between
#   them. The dock does not edit such a command: the earlier lines are not the line readline is
#   editing, so no length the terminal could send would match, and the terminal closes the
#   edit gate for a line with a line break in it (the keys go to readline as they are typed).
#
# The prompt is rebuilt at every prompt, from nothing: a theme that rewrites `PS1` is taken back
# at the next one.
__bateri_dock_ps1() {
  local ready= tag= mark=
  if [[ -n ${2-} ]]; then
    tag="bt_remote=$2;"
    mark=";bt_remote=$2"
  fi
  __bateri_dock_arm && ready='\[\e]8133;'$tag'w\a\]'
  PS1='\[\e]8;;'$1'\a\]  \[\e]8133;'$tag'g\a\]'$ready'\[\e]133;B'$mark'\a\]'
  PS2=
}

# Puts the wrapper's hooks into `PROMPT_COMMAND`: `__bateri_precmd` FIRST, so `$?` is still the
# command's when it runs, and `__bateri_prompt` LAST — starship and powerline write `PS1` from their
# own entries, and the prompt must see what they wrote. A bash 5.1 array keeps its other
# elements; before 5.1 only the array's first element runs, so there the first element is taken as
# a string like any other value. A string uses newlines as the joiner: the user's value may end
# in a comment or a `;`. `-v` first: under `set -u` the attribute query of an unset variable is
# an error, and it would end this setup.
__bateri_hook_prompt_command() {
  if [[ -v PROMPT_COMMAND && ${PROMPT_COMMAND@a} == *a* ]] \
    && (( BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1] >= 501 )); then
    PROMPT_COMMAND=(__bateri_precmd "${PROMPT_COMMAND[@]}" __bateri_prompt)
  else
    local nl=$'\n'
    PROMPT_COMMAND="__bateri_precmd${PROMPT_COMMAND:+$nl$PROMPT_COMMAND}$nl"__bateri_prompt
  fi
}
