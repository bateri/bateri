# bateri's REMOTE zsh wrapper — the body the four `ZDOTDIR` files load on
# a server.
#
# The files of the local wrapper that are the same on a server — its four
# `ZDOTDIR` files (`.zshenv`, `.zprofile`, `.zshrc`, `.zlogin`), the shared swap
# (`zdotdir.zsh`) and the dock (`dock.zsh`) — are written to the server verbatim,
# and this file in place of the local `bateri.zsh`, so the user's own startup
# files are read exactly as the local wrapper reads them. Only the hooks
# differ: here the working directory (OSC 7) and the command blocks (OSC 133),
# and with the dock's tier the Mac's dock too: the prompt is the terminal's and
# the line editor's display goes to it as it does on the Mac — through the
# server shell's identity, so the terminal can tell it from any other program's
# dock events (`dock.zsh` has the identity's rules). No `ssh` function (nested ssh
# is out of scope) and no context line fields (the Mac's dock shows the server's
# name and folder by itself).

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

# WHETHER THERE IS A DOCK IN THIS SESSION: the tier the Mac sends (`BATERI_DOCK=off` is its
# `[shell] integration = "blocks"`; anything else, or nothing, is the dock) and a shell that can
# have one — it needs the blocks (the dock's events carry the same identity), its functions (the
# file beside this one, `dock.zsh`) and `add-zle-hook-widget` (zsh 5.3); without any of them the
# shell is as it is without a dock: the prompt stays the user's. The variable is READ, not taken
# out: it stays in the environment so that `bateri`, the agent's command, hands it to the
# terminals it opens (`bin/bateri`). Once per shell, before the user's files, like the local
# wrapper's decision — and `source` at the top level, for its `typeset`s.
if (( ! ${+__bateri_dock} )); then
  __bateri_dock=0
  if [[ $BATERI_DOCK != off && $__bateri_rblock == <-> && -r $__bateri_dir/dock.zsh \
        && $ZSH_VERSION == (<6->|5.<3->)(|.*) ]] \
    && source $__bateri_dir/dock.zsh; then
    __bateri_dock=1
    # `LISTMAX`, before the user's files: `dock.zsh` says why there.
    __bateri_dock_start
  fi
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
    if (( __bateri_dock )); then
      # The anchor is in the prompt itself (`%9v` reads the block's counter from `psvar[9]`, set
      # by `__bateri_precmd`), so a prompt zsh draws again without `precmd` (Ctrl-L, a resize,
      # `zle reset-prompt`) keeps it; the identity is the one every mark of this shell carries.
      typeset -g __bateri_anchor=$'%{\e]8;;bateri://rblock/'$__bateri_rshell$'.%9v\a%}'
      __bateri_dock_setup
    fi
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
# limit, the local one's). No `B` here: its consumers are the dock's
# suppression and caret, and without a dock neither exists on the remote side.
# WITH A DOCK the prompt is the terminal's, as on the Mac (`dock.zsh`): the anchor
# and the `B` are in the prompt itself, and `precmd` sets it (and the slot `%9v`
# reads) instead of printing the anchor.
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
  if (( __bateri_dock )); then
    # The slot that carries the counter to the prompt (the local wrapper's reason for the
    # index: a high one, behind the user's own `psvar`).
    psvar[9]=$__bateri_block
    __bateri_dock_prompt
  else
    print -nr -- $'\e]8;;bateri://rblock/'$__bateri_rshell.$__bateri_block$'\a'
  fi
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
