# bateri's REMOTE zsh wrapper (048) — the body the four `ZDOTDIR` files load on
# a server.
#
# The bootstrap (`boot.sh`) writes the local wrapper's four files verbatim
# (`.zshenv`, `.zprofile`, `.zshrc`, `.zlogin`), the shared swap
# (`zdotdir.zsh`) and this file in their place, so the user's own startup
# files are read exactly as the local wrapper reads them. Only the hooks
# differ: here the working directory (OSC 7) and nothing else — no dock
# mirror, no prompt of the terminal's, no `ssh` function (nested ssh is out of
# scope, 048 plan → Kapsam Dışı).

source ${ZDOTDIR:-${0:A:h}}/zdotdir.zsh || return 1

# `.zshrc` calls it after the user's file; `add-zsh-hook` appends, so ours runs
# after the user's hooks.
__bateri_hooks() {
  autoload -Uz add-zsh-hook
  add-zsh-hook precmd __bateri_cwd
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
