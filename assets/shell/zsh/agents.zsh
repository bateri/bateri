# bateri's zsh agents — the part of the wrapper that starts an agent the way bateri asks, the same
# on this computer and on a server.
#
# `source`d by the LOCAL wrapper (`bateri.zsh`) and by the REMOTE one (`remote/zsh/bateri.zsh`,
# written to a server beside this file), from `__bateri_hooks`, which `.zshrc` calls after the
# user's files, and by nothing else; it defines one function and prints nothing. The bytes are the
# same in both places (`make bundle` and a test hold it).
#
# `claude` is started through `wrap/claude` (`~/.local/share/bateri/shell/wrap/claude`), which
# decides what bateri adds to Claude Code's command line (the hook of Settings › Agents) and runs
# it: the same file starts a `claude` that the `bateri` command or the app opens, so a `claude`
# typed in a pane and one a program opens in it start alike. Without that file (the command is
# turned off, or an update has not written it yet) `claude` runs as it was typed. The user's own
# words go on whole.
#
# This file defines nothing when the user has a `claude` function of their own: it does not
# replace it. An alias of the word `claude` is kept: its text runs and calls this function. An
# alias that names the program by its path (`alias claude=~/.claude/local/claude`) runs that path
# and never reaches this function. The keyword form `function claude` is on purpose: `claude() {`
# is read as the alias if there is one.

if (( ! ${+functions[claude]} )); then
  function claude {
    emulate -L zsh
    local wrap=$HOME/.local/share/bateri/shell/wrap/claude
    if [[ -r $wrap ]]; then
      sh $wrap claude "$@"
    else
      command claude "$@"
    fi
  }
fi
