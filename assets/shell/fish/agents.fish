# bateri's fish agents — the part of the wrapper that starts an agent the way bateri asks, the same
# on this computer and on a server.
#
# Read (`source`) by the LOCAL wrapper (`vendor_conf.d/bateri.fish`) and by the REMOTE one
# (`remote/fish/vendor_conf.d/bateri.fish`, written to a server beside this file), and by nothing
# else. It sits one level above `vendor_conf.d` on purpose, like `dock.fish`: fish reads every
# file in that directory by itself, and this one is read only by the wrappers that mean to. It
# defines one function and prints nothing. The bytes are the same in both places (`make bundle`
# and a test hold it): what differs between a Mac and a server is the data file below, not this
# code.
#
# `claude` starts Claude Code with `--settings '<hook>'`, so that the end of each turn reaches
# `bateri message` and the tab can show what the agent last said (Settings › Agents). The hook
# lives and ends with that one session: nothing is written to Claude Code's own files. The text of
# the hook is the file `~/.local/share/bateri/shell/agents/claude.json`, which the app writes when
# the setting is on and removes when it is off; the function reads it at every start, so a change
# in Settings reaches the next `claude` without a new shell. No file, no flag.
#
# The flag is NOT added, and `claude` runs as it was typed, when
#   - the user has a `claude` function of their own (an alias is a function in fish): this file
#     does not replace it. The wrappers are read before the user's `config.fish` and before the
#     functions folder is looked in, so the function is defined at the first prompt, after them,
#     and only if there is none by then;
#   - the user passed `--settings` themselves (the last one given wins, so theirs would have run,
#     but then the hook would be half there);
#   - `CLAUDECODE` is set: this shell was started by Claude Code itself, and a `claude -p` it
#     opens must not have its own settings overridden.
# The flag goes before the user's arguments, whatever they are: a subcommand does not care.
function __bateri_agents_define --on-event fish_prompt
    functions -e __bateri_agents_define
    functions -q claude; and return
    function claude --description 'Claude Code, started with the hook bateri asks for'
        set -l file $HOME/.local/share/bateri/shell/agents/claude.json
        set -l hook
        if test -z "$CLAUDECODE"; and test -r $file
            set hook (string join '' < $file)
            for arg in $argv
                test "$arg" = --; and break
                if test "$arg" = --settings; or string match -q -- '--settings=*' "$arg"
                    set hook
                    break
                end
            end
        end
        if test -n "$hook"
            command claude --settings $hook $argv
        else
            command claude $argv
        end
    end
end
