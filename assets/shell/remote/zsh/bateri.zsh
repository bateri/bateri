# bateri's REMOTE zsh wrapper — the body the four `ZDOTDIR` files load on
# a server.
#
# The bootstrap (`boot.sh`) writes the local wrapper's four files verbatim
# (`.zshenv`, `.zprofile`, `.zshrc`, `.zlogin`), the shared swap
# (`zdotdir.zsh`) and this file in their place, so the user's own startup
# files are read exactly as the local wrapper reads them. Only the hooks
# differ: here the working directory (OSC 7) and the command blocks (OSC
# 133) — no dock mirror, no prompt of the terminal's, no `ssh` function
# (nested ssh is out of scope).

source ${ZDOTDIR:-${0:A:h}}/zdotdir.zsh || return 1

# THE BLOCKS' PARENT: the local block of the `ssh` command, which the
# bootstrap exported as `BATERI_RBLOCK` (`boot.sh`). Kept in a shell variable
# and taken out of the environment — the local wrapper's `BATERI_BIN`
# precedent: a child shell has no use for it. Once per shell; without it no
# block mark is printed.
if (( ! ${+__bateri_rblock} )); then
  typeset -g __bateri_rblock=${BATERI_RBLOCK-}
  unset BATERI_RBLOCK
fi

# `.zshrc` calls it after the user's file; `add-zsh-hook` appends, so ours runs
# after the user's hooks.
__bateri_hooks() {
  autoload -Uz add-zsh-hook
  # `bateri`, the agent's command (`bin/bateri`, beside the `zsh` directory
  # this runs from), on `PATH` once and after the user's files have set theirs.
  local bt_bin=${__bateri_dir:h}/bin
  if [[ -x $bt_bin/bateri ]] && (( ! ${path[(Ie)$bt_bin]} )); then
    path=($bt_bin $path)
  fi
  add-zsh-hook precmd __bateri_cwd
  if [[ $__bateri_rblock == <-> ]]; then
    # `<P>.<S>`: the parent and this shell's pid — two shells under one `ssh`
    # command line (`ssh a; ssh b`) are two trails (`bt-core`'s `RemoteShell`).
    typeset -g __bateri_rshell=$__bateri_rblock.$$ __bateri_block=0 __bateri_ran=0
    add-zsh-hook precmd __bateri_precmd
    add-zsh-hook preexec __bateri_preexec
    precmd_functions=(__bateri_title_back ${precmd_functions:#__bateri_title_back})
  fi
}

# The command blocks on the server: the local wrapper's
# `D`/`A`/`C`, but the identity is OUR REMOTE FIELD, `bt_remote=<P>.<S>.<n>`,
# on every mark — `P` the local `ssh` block, `S` this shell's pid, `n` its
# counter. bateri
# keeps them on a trail of their own (`bt-core`'s `ShellLog::remote`): a
# `bt_block=` from here would be read as the local shell's and end the remote
# session.
#
# THE ANCHOR IS PRINTED, NOT PUT IN PS1 (unlike the local wrapper): the
# prompt here is the user's and stays untouched. zsh prints `PROMPT_SP`'s
# partial-line mark BEFORE `precmd` (measured), so the link opened at the end
# of our `precmd` — the last hook — covers the prompt and the typed command
# and nothing above; a prompt redrawn without `precmd` (Ctrl-L, SIGWINCH) is
# still inside the open link. A theme's own OSC 8 link closes ours (a known
# limit, the local one's). No `B`: its consumers are the dock's suppression
# and caret, neither of which exists on the remote side.
__bateri_precmd() {
  # MUST be the FIRST line (the local wrapper's reason).
  local code=$?
  emulate -L zsh
  if (( __bateri_ran )); then
    __bateri_ran=0
    print -nr -- $'\e]133;D;'$code';bt_remote='$__bateri_rshell.$__bateri_block$'\a'
    # The input modes the command left on go with it (the local wrapper's reason).
    print -nr -- $'\e[?1000;1002;1003;1004;1005;1006;1016;2048l\e[<4096u'
  fi
  (( __bateri_block++ ))
  print -nr -- $'\e]133;A;bt_remote='$__bateri_rshell.$__bateri_block$'\a'
  print -nr -- $'\e]8;;bateri://rblock/'$__bateri_rshell.$__bateri_block$'\a'
}

# Right before a command runs: the anchor closes (the output does not carry
# it), the output begins.
__bateri_preexec() {
  __bateri_ran=1
  print -nr -- $'\e]8;;\a\e]133;C;bt_remote='$__bateri_rshell.$__bateri_block$'\a'
  # The title as the command finds it, for `__bateri_title_back`.
  print -nr -- $'\e[22t'
}

# The title the command set goes back, first among the precmd hooks so a
# user's hook that names the window at every prompt still has the last word
# (the local wrapper's reason); so does the alternate screen a program that is
# over left on, after its kitty key stack, and not for one that is only stopped
# (the local wrapper's reasons). The signal numbers are this server's.
__bateri_title_back() {
  local code=$?
  emulate -L zsh
  if (( __bateri_ran )); then
    print -nr -- $'\e[23t'
    if (( code <= 128 )) || [[ ${signals[code - 127]-} != (TSTP|STOP|TTIN|TTOU) ]]; then
      print -nr -- $'\e[<4096u\e[?1049l'
    fi
  fi
  return code
}

# Reports the working directory with OSC 7, per prompt (the encoder,
# `__bateri_percent`, is the shared one in `zdotdir.zsh`).
#
# THE AUTHORITY IS THE SERVER'S NAME, unlike the local wrapper's empty one: a
# foreign authority goes to bateri's remote slot even before the remote probe
# has landed (`ShellLog::apply_scan_answering`), an empty one would overwrite
# the local folder in that window.
__bateri_cwd() {
  emulate -L zsh
  local REPLY
  __bateri_percent "$PWD"
  print -nr -- $'\e]7;file://'$HOST$REPLY$'\a'
}
