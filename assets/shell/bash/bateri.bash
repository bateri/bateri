# bateri's LOCAL bash wrapper — read as `$ENV` by `bash --posix`.
#
# WHY POSIX MODE: `--rcfile` is ignored by a login shell, and a login shell is what the
# terminal starts (argv[0] is `-bash`, or `-l` where there is no `login(1)`): `logout` works
# and `~/.bash_logout` is read on exit. In POSIX mode an interactive bash reads `$ENV` and no
# other startup file (kitty's method), so this file runs first, leaves POSIX mode and reads
# the login files itself, in bash's own order: `/etc/profile`, then the first readable of
# `~/.bash_profile`, `~/.bash_login`, `~/.profile`. The terminal only starts a bash it has
# asked the version of: 4.4 or newer (`PS0`); on an older one this file is never set up.
#
# THE USER'S FILES ARE READ AT THE TOP LEVEL, not from a function: `declare` and `local`
# inside a function are local, so `declare -x PATH=…` would be undone on return (the trap the
# zsh wrapper measured). They are read, never written (the `make audit` gate). Our hooks are
# installed AFTER they are read, so a `PROMPT_COMMAND` or `PS1` the files assign outright
# cannot take them away.
#
# CONTRACT (the setting side is `bt-shell-macos`'s `launch::integration_env`):
#   ENV              this file; the user's own value comes back below.
#   BATERI_ENV       the user's `ENV`, if there was one. It is put back and never read: a
#                    login bash does not read `ENV`.
#   BATERI_DOCK      `off`: there is no dock in this session (`integration = "blocks"`), so the
#                    prompt is the user's, with the marks around it. Absent: the terminal draws
#                    the prompt and shows the line in its dock (`__bateri_prompt`).
#   BATERI_BIN, BATERI_SSH_INSTANCE, BATERI_SSH_CWD
#                    the `ssh` function's inputs, as in the zsh wrapper.
# The last four are ours, so they are taken out of the environment too (children have no use
# for them).

# `ENV` is ours only for this start: the user's own value comes back, and without one it goes,
# so child shells do not read this file.
if [ -n "${BATERI_ENV+x}" ]; then
  ENV=$BATERI_ENV
  unset BATERI_ENV
else
  unset ENV
fi

# Read once, before the user's files can see them.
__bateri_bin=${BATERI_BIN-}
__bateri_ssh_instance=${BATERI_SSH_INSTANCE-}
__bateri_ssh_cwd=${BATERI_SSH_CWD-}
if [ "${BATERI_DOCK-}" = off ]; then
  __bateri_dock=0
else
  __bateri_dock=1
fi
unset BATERI_BIN BATERI_SSH_INSTANCE BATERI_SSH_CWD BATERI_DOCK

# POSIX mode's default history file is `~/.sh_history`, set before any startup file is read.
# A login bash keeps `~/.bash_history`, so the default is put back, unless the user's
# environment (an exported `HISTFILE`) named the file: that value is theirs, whatever it is.
# Their files may still set it, and win, as they do in a plain login shell.
if [ "$HISTFILE" = "$HOME/.sh_history" ] && [[ ${HISTFILE@a} != *x* ]]; then
  HISTFILE=$HOME/.bash_history
fi
set +o posix

if [ -r /etc/profile ]; then
  . /etc/profile
fi
for __bateri_login in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
  if [ -r "$__bateri_login" ]; then
    . "$__bateri_login"
    break
  fi
done
unset __bateri_login

# Percent-encodes `$1` (RFC 3986's unreserved set + `/`) into `REPLY`, byte by byte: `LC_ALL=C`
# makes every element a byte and `'c` its value.
__bateri_percent() {
  local LC_ALL=C s=$1 out= c i n
  for (( i = 0; i < ${#s}; i++ )); do
    c=${s:i:1}
    case $c in
      [A-Za-z0-9/._~-]) out+=$c ;;
      *)
        printf -v n '%d' "'$c"
        printf -v c '%%%02X' $(( n & 255 ))
        out+=$c
        ;;
    esac
  done
  REPLY=$out
}

# Base64 of `$1`'s bytes into `REPLY`, without padding (the terminal reads both). No fork: the
# branch name goes through this at every prompt, after the one `git` already costs.
__bateri_b64() {
  local LC_ALL=C s=$1 out= i n a b c v
  local t=ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/
  for (( i = 0; i < ${#s}; i += 3 )); do
    n=$(( ${#s} - i ))
    printf -v a '%d' "'${s:i:1}"
    v=$(( (a & 255) << 16 ))
    if (( n > 1 )); then
      printf -v b '%d' "'${s:i+1:1}"
      v=$(( v | ((b & 255) << 8) ))
    fi
    if (( n > 2 )); then
      printf -v c '%d' "'${s:i+2:1}"
      v=$(( v | (c & 255) ))
    fi
    out+=${t:(v >> 18) & 63:1}${t:(v >> 12) & 63:1}
    (( n > 1 )) && out+=${t:(v >> 6) & 63:1}
    (( n > 2 )) && out+=${t:v & 63:1}
  done
  REPLY=$out
}

# Sends the git branch through the dock's channel; empty outside a repository, which deletes
# the previous directory's branch. One `git` in the usual case, per prompt. The environment
# report rides after it (`__bateri_env_fields`).
__bateri_branch() {
  local REPLY ref
  ref=$(command git rev-parse --abbrev-ref HEAD 2>/dev/null)
  # `HEAD` is the answer of a detached HEAD, not a branch name: the short SHA.
  if [[ $ref == HEAD ]]; then
    ref=$(command git rev-parse --short HEAD 2>/dev/null)
  fi
  __bateri_b64 "$ref"
  local branch=$REPLY
  __bateri_env_fields
  printf '\033]8133;b;%s%s\007' "$branch" "$__bateri_env_wire"
}

# The environment the dock's context row shows, as five more `8133;b` fields: `VIRTUAL_ENV`,
# `VIRTUAL_ENV_PROMPT`, `CONDA_PREFIX`, `CONDA_PROMPT_MODIFIER` and the `node` this shell would
# run, each base64, empty when unset — the zsh wrapper's report. The terminal reads the files
# they name and runs nothing. The dock's PS1 is ours and rebuilt at every prompt, so the name
# `activate` puts in front of it is lost there: the variables carry it.
#
# Encoded only when something changed (the encoder is a loop per three bytes). `node` is found
# by walking `PATH` here, with no fork — a `$(type -P node)` per prompt would be one; a relative
# entry gives a relative path, which the terminal does not read.
__bateri_env_raw=
__bateri_env_wire=
__bateri_env_fields() {
  local REPLY field out= node= dir rest=$PATH:
  while [[ -n $rest ]]; do
    dir=${rest%%:*}
    rest=${rest#*:}
    if [[ -f ${dir:-.}/node && -x ${dir:-.}/node ]]; then
      node=${dir:-.}/node
      break
    fi
  done
  local raw=${VIRTUAL_ENV-}$'\x1f'${VIRTUAL_ENV_PROMPT-}$'\x1f'${CONDA_PREFIX-}$'\x1f'${CONDA_PROMPT_MODIFIER-}$'\x1f'$node
  if [[ $raw == "$__bateri_env_raw" && -n $__bateri_env_wire ]]; then
    return 0
  fi
  for field in "${VIRTUAL_ENV-}" "${VIRTUAL_ENV_PROMPT-}" "${CONDA_PREFIX-}" \
    "${CONDA_PROMPT_MODIFIER-}" "$node"; do
    __bateri_b64 "$field"
    out+=";$REPLY"
  done
  __bateri_env_raw=$raw
  __bateri_env_wire=$out
}

# Reports the working directory with OSC 7, per prompt.
#
# THE AUTHORITY IS EMPTY (`file:///…`), not `file://$HOSTNAME…`: the terminal's gate counts a
# named host as foreign, and an empty one cannot go stale when the machine is renamed.
__bateri_cwd() {
  local REPLY
  __bateri_percent "$PWD"
  printf '\033]7;file://%s\007' "$REPLY"
}

# The command blocks: the zsh wrapper's `D`/`A`/`C`, with the block's id in `bt_block=` and,
# on the prompt's cells, in an OSC 8 anchor (`__bateri_prompt`) — the terminal reads which row
# a block started on from the grid, so it stays right after scrolling and reflow.
#
# THE CLOSING NEEDS `PS0` (bash 4.4): the only hook between reading a command and running it
# without a `DEBUG` trap (which is the user's). It closes the anchor and marks the output's
# start (`C`). `PS0` is expanded in this shell, and an arithmetic subscript is how it sets the
# "a command ran" flag without printing (`${…[…]-}`: empty even under `set -u`); an empty line
# reads no command and prints no `PS0`, so it closes no block — zsh's `preexec` rule.
#
# WITH A DOCK the line is handed back to the grid first (`8133;e`). Enter has already moved the
# caret down a row by the time `PS0` is printed — bash writes the newline, the bracketed-paste
# end and then `PS0` in separate writes (measured) — and until `e` arrives the dock would show
# the new, empty row.
#
# `PS0` is built at every prompt (`__bateri_ps0`), because that subscript is a parameter
# expansion and a user may turn `promptvars` off: the text would then be printed as it is
# before every command. Without it the flag cannot be set from `PS0`, so every prompt counts as
# one that ran a command: an empty line or a cancelled one closes a block it did not run.
__bateri_block=0
__bateri_ran=0
__bateri_ps0_user=${PS0-}
__bateri_ps0=
if (( __bateri_dock )); then
  __bateri_ps0_marks='\e]8133;e\a\e]8;;\a\e]133;C\a'
else
  __bateri_ps0_marks='\e]8;;\a\e]133;C\a'
fi
__bateri_ps0() {
  # A `PS0` that is not the one left last time is a new text of the user's.
  if [[ ${PS0-} != "$__bateri_ps0" ]]; then
    __bateri_ps0_user=${PS0-}
  fi
  if shopt -q promptvars; then
    PS0='${__bateri_none[__bateri_ran=1]-}'$__bateri_ps0_marks$__bateri_ps0_user
  else
    __bateri_ran=1
    PS0=$__bateri_ps0_marks$__bateri_ps0_user
  fi
  __bateri_ps0=$PS0
}

# Before the prompt: the finished command's `D`, the next prompt's `A`, the directory. The
# status of the command that ran is kept: a `PROMPT_COMMAND` after ours may read `$?`.
#
# `D` closes the FINISHED block, so its id is the counter's value before it moves. An empty
# line, or a line cancelled with Ctrl-C, ran nothing: it opens a new block with no `D` for the
# old one, as in zsh.
__bateri_precmd() {
  local status=$?
  if (( __bateri_ran )); then
    __bateri_ran=0
    printf '\033]133;D;%s;bt_block=%s\007' "$status" "$__bateri_block"
  fi
  __bateri_block=$(( __bateri_block + 1 ))
  printf '\033]133;A;bt_block=%s\007' "$__bateri_block"
  __bateri_cwd
  (( __bateri_dock )) && __bateri_branch
  return $status
}

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

# The prompt: with a dock, nothing of the user's; without, theirs with the anchor in front and
# the end mark behind.
#
# WITH A DOCK the prompt is the terminal's, and what bash draws is a contract (`grid_input.rs`):
#
#   anchor, two real spaces, `g`, [`w`], `B`
#
# - The anchor is the block's id-carrying OSC 8 link, laid over the cells and left open until
#   `PS0`, so every cell readline draws for the line carries the block's id: the terminal reads
#   the line from the cells with that id, shows it in the dock and keeps it off the grid.
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
# The prompt is rebuilt at every prompt, from nothing: a theme that rewrites `PS1` is taken
# back at the next one (`integration = "blocks"` gives the user's prompt back).
#
# WITHOUT A DOCK the anchor is a prefix: OSC 8 does not nest, so a prompt link of the user's
# replaces ours for the cells after it (a known limit, a stripe missing, not a wrong one). It
# lives IN `PS1` for the reason above.
#
# The id is written into the string each time, so the string is rebuilt at every prompt from
# the user's own text (`__bateri_user_ps1`). A `PS1` that is not the one left last time is a
# new text of the user's (a theme rewrites it at every prompt, `PROMPT_COMMAND` or not) and is
# the one wrapped; one that is, is wrapped again with the new id. `promptvars` is not relied
# on for the id: a user may turn it off.
#
# LAST in `PROMPT_COMMAND`: starship and powerline write `PS1` from their own entries, and
# this one must see what they wrote.
__bateri_user_ps1=
__bateri_ps1=
__bateri_prompt() {
  local status=$?
  __bateri_ps0
  if (( __bateri_dock )); then
    local ready=
    __bateri_dock_arm && ready='\[\e]8133;w\a\]'
    PS1='\[\e]8;;bateri://block/'$__bateri_block'\a\]  \[\e]8133;g\a\]'$ready'\[\e]133;B\a\]'
    PS2=
    return $status
  fi
  if [[ $PS1 != "$__bateri_ps1" ]]; then
    __bateri_user_ps1=$PS1
  fi
  PS1='\[\e]8;;bateri://block/'$__bateri_block'\a\]'$__bateri_user_ps1'\[\e]133;B\a\]'
  __bateri_ps1=$PS1
  return $status
}

# First in `PROMPT_COMMAND`, so `$?` is still the command's when it runs; the prompt last.
# A bash 5.1 array keeps its other elements; before 5.1 only the array's first element runs, so
# there the first element is taken as a string like any other value. A string uses newlines as
# the joiner: the user's value may end in a comment or a `;`. `-v` first: under `set -u` the
# attribute query of an unset variable is an error, and it would end this setup.
if [[ -v PROMPT_COMMAND && ${PROMPT_COMMAND@a} == *a* ]] \
  && (( BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1] >= 501 )); then
  PROMPT_COMMAND=(__bateri_precmd "${PROMPT_COMMAND[@]}" __bateri_prompt)
else
  __bateri_nl=$'\n'
  PROMPT_COMMAND="__bateri_precmd${PROMPT_COMMAND:+$__bateri_nl$PROMPT_COMMAND}$__bateri_nl"__bateri_prompt
  unset __bateri_nl
fi

# Runs the user's `ssh`, wrapped when bateri says so. The rules are the zsh wrapper's and the
# binary's (`ssh_wrap::decide`): which call is interactive, the settings, `ssh -G`.
# `bateri ssh-argv` prints the wrapped arguments, each followed by a NUL, or nothing — and
# nothing (also a missing binary or any failure) means plain `command ssh "$@"`. `--tty` only
# when stdin and stdout are terminals. Inside a local multiplexer nothing is wrapped: the pane
# sees tmux or screen, not the ssh, so the session could never prove it ran. After a wrapped
# session ends `bateri ssh-fell-back` says whether to rerun it plain (a router, a server that
# refused our command). `--block` is this command's block; `--cwd` is only for the first `ssh`
# of a tab opened from a remote one.
#
# An answer is read NUL by NUL; one that does not end with a NUL is not ours: plain `ssh`.
__bateri_ssh() {
  local arg rc
  local -a tty=() instance=() cwd=() wrapped=() plain=()
  if [[ -n $__bateri_ssh_cwd ]]; then
    cwd=(--cwd "$__bateri_ssh_cwd")
  fi
  __bateri_ssh_cwd=
  if [[ -n ${TMUX-} || -n ${STY-} ]]; then
    command ssh "$@"
    return
  fi
  [[ -t 0 && -t 1 ]] && tty=(--tty)
  [[ -n $__bateri_ssh_instance ]] && instance=(--instance "$__bateri_ssh_instance")
  if [[ -x $__bateri_bin ]]; then
    arg=
    while IFS= read -r -d '' arg; do
      wrapped+=("$arg")
    done < <(command "$__bateri_bin" ssh-argv "${tty[@]}" --block "$__bateri_block" \
      "${instance[@]}" "${cwd[@]}" -- "$@" 2>/dev/null)
    [[ -n $arg ]] && wrapped=()
  fi
  if (( ${#wrapped[@]} )); then
    command ssh "${wrapped[@]}"
    rc=$?
    arg=
    while IFS= read -r -d '' arg; do
      plain+=("$arg")
    done < <(command "$__bateri_bin" ssh-fell-back --rc "$rc" "${instance[@]}" \
      -- "${wrapped[@]}" 2>/dev/null)
    [[ -n $arg ]] && plain=()
    if (( ${#plain[@]} )); then
      command ssh "${plain[@]}"
      return
    fi
    return $rc
  fi
  command ssh "$@"
}

# The `ssh` function, defined AFTER the user's files, so a function or alias of theirs is seen
# and wins (a known limit: theirs bypasses the integration). Only with the binary's path.
if [[ -n $__bateri_bin ]] && ! declare -F ssh > /dev/null && ! alias ssh > /dev/null 2>&1; then
  ssh() { __bateri_ssh "$@"; }
fi

# `bateri` as a command: the binary that started this shell, which is not on PATH (it lives
# inside the `.app`) — `bateri config set …`, `bateri help`. The `ssh` function's rule: a
# `bateri` of the user's own (a function, an alias, a command on PATH) wins.
if [[ -n $__bateri_bin ]] && ! declare -F bateri > /dev/null && ! alias bateri > /dev/null 2>&1 \
  && ! type -P bateri > /dev/null; then
  bateri() { command "$__bateri_bin" "$@"; }
fi
