# bateri's bash agents — the part of the wrapper that starts an agent the way bateri asks, the same
# on this computer and on a server.
#
# Read (`.`) by the LOCAL wrapper (`bateri.bash`) and by the REMOTE one (`remote/bash/bateri.bash`,
# written to a server beside this file), after the user's login files, and by nothing else; it
# defines one function and prints nothing. The bytes are the same in both places (`make bundle`
# and a test hold it): what differs between a Mac and a server is the data file below, not this
# code. It is written for bash 3.2 as well: the wrappers read it wherever they run.
#
# `claude` starts Claude Code with `--settings '<hook>'`, so that the end of each turn reaches
# `bateri message` and the tab can show what the agent last said (Settings › Agents). The hook
# lives and ends with that one session: nothing is written to Claude Code's own files. The text of
# the hook is the file `~/.local/share/bateri/shell/agents/claude.json`, which the app writes when
# the setting is on and removes when it is off; the function reads it at every start, so a change
# in Settings reaches the next `claude` without a new shell. No file, no flag.
#
# The flag is NOT added, and `claude` runs as it was typed, when
#   - the user has a `claude` function of their own: this file does not replace it (it defines
#     nothing then). An alias of the word `claude` is kept: its text runs and calls this
#     function. An alias that names the program by its path (`alias claude=~/.claude/local/claude`)
#     runs that path and never reaches this function: it gets no hook;
#   - the user passed `--settings` themselves (the last one given wins, so theirs would have run,
#     but then the hook would be half there);
#   - `CLAUDECODE` is set: this shell was started by Claude Code itself, and a `claude -p` it
#     opens must not have its own settings overridden.
# The flag goes before the user's arguments, whatever they are: a subcommand does not care. The
# keyword form `function claude` is on purpose: `claude() {` is read as the alias if there is one.

if ! declare -F claude > /dev/null 2>&1; then
  function claude {
    local file=$HOME/.local/share/bateri/shell/agents/claude.json hook= arg
    if [ -z "${CLAUDECODE-}" ] && [ -r "$file" ]; then
      hook=$(<"$file")
      for arg in "$@"; do
        case $arg in
          --) break ;;
          --settings | --settings=*) hook=; break ;;
        esac
      done
    fi
    if [ -n "$hook" ]; then
      command claude --settings "$hook" "$@"
    else
      command claude "$@"
    fi
  }
fi
