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
#                    the `ssh` function's inputs, as in the zsh wrapper (the last one is no
#                    input any more; it is only taken out of the environment).
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

# The dock's functions and the hooks' installer (`dock.bash`, beside this file, shared with the
# server's wrapper). Read at every start, `"blocks"` too: the installer is in it. Without the
# file the shell is the user's, unmarked.
__bateri_dock_file=${BASH_SOURCE[0]%/*}/dock.bash
if [[ -r $__bateri_dock_file ]]; then
  . "$__bateri_dock_file"
  unset __bateri_dock_file
else
  unset __bateri_dock_file
  return 0
fi

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
# The statuses of a command that is only stopped, not over (⌃Z, a STOP, TTIN or TTOU): 128 + the
# signal, whose numbers are the system's. `__bateri_precmd` leaves such a program its screen.
__bateri_stopped=
for __bateri_signal in $(kill -l TSTP STOP TTIN TTOU 2> /dev/null); do
  __bateri_stopped+=" $((128 + __bateri_signal))"
done
unset __bateri_signal
__bateri_ps0_user=${PS0-}
__bateri_ps0=
# After `C`, the title as the command finds it goes onto the terminal's title stack (`CSI 22 t`)
# for `__bateri_precmd` to put back (the zsh script says why).
if (( __bateri_dock )); then
  __bateri_ps0_marks='\e]8133;e\a\e]8;;\a\e]133;C\a\e[22t'
else
  __bateri_ps0_marks='\e]8;;\a\e]133;C\a\e[22t'
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
    # The alternate screen a program that is over left on goes with it, its kitty key stack
    # first, before `D`; not for one that is only stopped (the zsh script says why).
    [[ " $__bateri_stopped " == *" $status "* ]] || printf '\033[<4096u\033[?1049l'
    printf '\033]133;D;%s;bt_block=%s\007' "$status" "$__bateri_block"
    # The input modes the command left on go with it (the zsh script says why).
    printf '\033[?1000;1002;1003;1004;1005;1006;1016;2048l\033[<4096u'
    # So does the title it set: ours is the first `PROMPT_COMMAND`, so a user's entry or a
    # `PS1` that names the window still has the last word.
    printf '\033[23t'
  fi
  __bateri_block=$(( __bateri_block + 1 ))
  printf '\033]133;A;bt_block=%s\007' "$__bateri_block"
  __bateri_cwd
  (( __bateri_dock )) && __bateri_branch
  return $status
}

# The prompt: with a dock, nothing of the user's; without, theirs with the anchor in front and
# the end mark behind.
#
# WITH A DOCK the prompt is the terminal's: `__bateri_dock_ps1` (`dock.bash`, beside this file and
# shared with the server's wrapper) sets it, from nothing, at every prompt — what it draws is a
# contract with the terminal and is written there. `integration = "blocks"` gives the user's
# prompt back.
#
# WITHOUT A DOCK the anchor is a prefix: OSC 8 does not nest, so a prompt link of the user's
# replaces ours for the cells after it (a known limit, a stripe missing, not a wrong one). It
# lives IN `PS1`: readline draws the prompt again without running `PROMPT_COMMAND` (Ctrl-L, a
# resize), and an anchor printed from a hook would be missing from that copy.
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
    __bateri_dock_ps1 "bateri://block/$__bateri_block"
    return $status
  fi
  if [[ $PS1 != "$__bateri_ps1" ]]; then
    __bateri_user_ps1=$PS1
  fi
  PS1='\[\e]8;;bateri://block/'$__bateri_block'\a\]'$__bateri_user_ps1'\[\e]133;B\a\]'
  __bateri_ps1=$PS1
  return $status
}

# First in `PROMPT_COMMAND`, so `$?` is still the command's when it runs; the prompt last
# (`dock.bash`).
__bateri_hook_prompt_command

# Runs the user's `ssh`: as a terminal kept on the server when bateri says so, else plain
# `command ssh "$@"`. The zsh wrapper's, word for word in what it does: `bateri ssh-hold -- <args…>`
# stands in the `ssh`'s place, asks the running bateri for the terminal and only carries bytes;
# every decision (which call is interactive, the settings, `ssh -G`, whether the server can keep a
# terminal, the password and the server's key — asked on this terminal by ssh itself) is the
# binary's, and whatever it cannot do before a terminal is shown, it replaces itself with the plain
# `ssh` with the arguments as typed. Plain without asking when it could not work: stdin and stdout
# must be terminals, there is the binary and the directory of bateri's sockets, and no local
# multiplexer (`$TMUX`, `$STY`). `BATERI_SSH_INSTANCE` is exported for the one command only: the
# shell unset it at startup.
__bateri_ssh() {
  if [[ -n ${TMUX-} || -n ${STY-} || ! -t 0 || ! -t 1 \
        || ! -x $__bateri_bin || -z $__bateri_ssh_instance ]]; then
    command ssh "$@"
    return
  fi
  BATERI_SSH_INSTANCE=$__bateri_ssh_instance command "$__bateri_bin" ssh-hold -- "$@"
}

# The `ssh` function, defined AFTER the user's files, so a function or alias of theirs is seen
# and wins (a known limit: theirs bypasses the integration). Only with the binary's path.
if [[ -n $__bateri_bin ]] && ! declare -F ssh > /dev/null && ! alias ssh > /dev/null 2>&1; then
  ssh() { __bateri_ssh "$@"; }
fi
