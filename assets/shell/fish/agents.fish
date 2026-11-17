# bateri's fish agents — the part of the wrapper that starts an agent the way bateri asks, the same
# on this computer and on a server.
#
# Read (`source`) by the LOCAL wrapper (`vendor_conf.d/bateri.fish`) and by the REMOTE one
# (`remote/fish/vendor_conf.d/bateri.fish`, written to a server beside this file), and by nothing
# else. It sits one level above `vendor_conf.d` on purpose, like `dock.fish`: fish reads every
# file in that directory by itself, and this one is read only by the wrappers that mean to. It
# defines one function and prints nothing. The bytes are the same in both places (`make bundle`
# and a test hold it).
#
# `claude` is started through `wrap/claude` (`~/.local/share/bateri/shell/wrap/claude`), which
# decides what bateri adds to Claude Code's command line (the hook of Settings › Agents) and runs
# it: the same file starts a `claude` that the `bateri` command or the app opens, so a `claude`
# typed in a pane and one a program opens in it start alike. Without that file (the command is
# turned off, or an update has not written it yet) `claude` runs as it was typed. The user's own
# words go on whole.
#
# This file defines nothing when the user has a `claude` function of their own (an alias is a
# function in fish): it does not replace it. The wrappers are read before the user's `config.fish`
# and before the functions folder is looked in, so the function is defined at the first prompt,
# after them, and only if there is none by then.
function __bateri_agents_define --on-event fish_prompt
    functions -e __bateri_agents_define
    functions -q claude; and return
    function claude --description 'Claude Code, started the way bateri asks'
        set -l wrap $HOME/.local/share/bateri/shell/wrap/claude
        if test -r $wrap
            sh $wrap claude $argv
        else
            command claude $argv
        end
    end
end
