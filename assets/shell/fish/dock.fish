# bateri's fish dock — the part of the wrapper that is the same on this computer and on a server.
#
# Read (`source`) by the LOCAL wrapper (`vendor_conf.d/bateri.fish`) and by the REMOTE one
# (`remote/fish/vendor_conf.d/bateri.fish`, written to a server beside this file), and by nothing
# else. It sits one level above `vendor_conf.d` on purpose: fish reads every file in that
# directory by itself, and this one is read only by the wrappers that mean to. It defines a
# function and prints nothing. What differs between the two wrappers is one identity, and it is
# three variables the wrapper sets before each prompt:
#
#   __bateri_dock_anchor  the OSC 8 target of the prompt's block: local `bateri://block/<n>`,
#                         remote `bateri://rblock/<P>.<S>.<n>`;
#   __bateri_dock_tag     what follows `8133;` on the dock's events: empty locally, remote
#                         `bt_remote=<P>.<S>.<n>;`, so the terminal can tell its own server
#                         shell's prompt from any other program's 8133;
#   __bateri_dock_mark    what follows `133;B`: empty locally, remote `;bt_remote=<P>.<S>.<n>`.
#
# The prompt's contract with the terminal is written here once: two copies would part the day one
# of them changes, and nothing but a hand-typed click would show it.

set -g __bateri_dock_anchor ''
set -g __bateri_dock_tag ''
set -g __bateri_dock_mark ''

# The prompt the terminal draws a dock for: nothing of the user's.
#
# A dock session hands the prompt to the terminal, as zsh's does: the line fish
# draws is read back from the grid and shown in the dock, and the user's prompt
# is not drawn (`integration = "blocks"` gives it back). What fish prints here
# is a contract with the terminal (`grid_input.rs`):
#
#   anchor, two real spaces, `g`, `B`
#
# - THE TWO SPACES ARE REAL WIDTH, the same two columns zsh's dock prompt has: the
#   command line starts at the third column, and fish, which knows the prompt's
#   width, wraps and places its caret right. The anchor is the id-less OSC 8 link
#   of the block, laid over the cells. It stays open to `fish_preexec`, so every
#   cell fish draws after it carries the same link id; that id is what tells the
#   terminal where the input line begins (a job notice printed under the open
#   link is not part of it, as fish redraws a cached prompt with a fresh id).
# - THE SECOND SPACE IS DRAWN IN THE AUTOSUGGESTION'S STYLE. It is a style
#   sample: the terminal tells a suggestion from what was typed by the cells
#   after the caret that look like it. fish turns the variable into a color
#   with the same code `set_color` uses, so the sample is never the terminal's
#   own guess at fish's color syntax. A theme with no suggestion color leaves
#   the sample plain, and then everything is what was typed.
# - `g` says "this prompt's line is read from the grid". It is printed in the
#   prompt, not at the event: fish prints a bare `A` at every redraw (a cached
#   prompt, a resize) and every `A` forgets `g`, so only what comes with the
#   prompt's text survives. `B` is ours because fish's own came only in 4.0.
#
# The right prompt and the mode prompt are emptied (a vi mode indicator would
# sit in the middle of the line the terminal reads). They, and the prompt, are
# defined again at EVERY prompt, so a theme or a `fish_config prompt choose`
# that writes them later is taken back at the next one. A user's own handler on
# this event that does it runs after ours and wins (the wrapper's known limit);
# that prompt's line is then drawn in the grid as the user wrote it.
function __bateri_take
    function fish_prompt --description 'bateri: the prompt the terminal draws a dock for'
        printf '\e]8;;%s\a ' "$__bateri_dock_anchor"
        set -q fish_color_autosuggestion[1]; and set_color $fish_color_autosuggestion 2>/dev/null
        printf ' '
        set_color normal
        printf '\e]8133;%sg\a\e]133;B%s\a' "$__bateri_dock_tag" "$__bateri_dock_mark"
    end
    function fish_right_prompt --description 'bateri: the terminal draws the right side'
    end
    function fish_mode_prompt --description 'bateri: the terminal draws the mode'
    end
end
