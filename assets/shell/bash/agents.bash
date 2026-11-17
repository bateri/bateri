# bateri's bash agents — the part of the wrapper that starts an agent the way bateri asks, the same
# on this computer and on a server.
#
# Read (`.`) by the LOCAL wrapper (`bateri.bash`) and by the REMOTE one (`remote/bash/bateri.bash`,
# written to a server beside this file), after the user's login files, and by nothing else; it
# defines one function and prints nothing. The bytes are the same in both places (`make bundle`
# and a test hold it). It is written for bash 3.2 as well: the wrappers read it wherever they run.
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

if ! declare -F claude > /dev/null 2>&1; then
  function claude {
    local wrap=$HOME/.local/share/bateri/shell/wrap/claude
    if [ -r "$wrap" ]; then
      sh "$wrap" claude "$@"
    else
      command claude "$@"
    fi
  }
fi
