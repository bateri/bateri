# bateri's LOCAL fish wrapper — a `vendor_conf.d` file.
#
# fish reads `vendor_conf.d` from every `XDG_DATA_DIRS` entry, before the
# user's `config.fish`, and we touch none of the user's files: the terminal put
# our directory first in `XDG_DATA_DIRS`, and the first thing this file does is
# take it out again, so child processes see the user's own value. The user's
# files and universal variables are read, never written (the `make audit` gate).
#
# CONTRACT (the setting side is `bt-shell-macos`'s `launch::integration_env`):
#   BATERI_FISH_DIR   the `XDG_DATA_DIRS` entry that is ours; taken out below.
#   BATERI_XDG_UNSET  `XDG_DATA_DIRS` was unset: the terminal wrote fish's own
#                     defaults after ours (fish computes the `vendor_conf.d`,
#                     `functions` and `completions` search paths from the
#                     variable at startup, and ours alone would have dropped
#                     `/usr/local/share/fish` and `/usr/share/fish` for the
#                     whole session), so "taking ours out" means unsetting it.
#   BATERI_DOCK       if `off`, there is no dock in this session. Absent means
#                     there is one, and then the PROMPT IS THE TERMINAL'S: it
#                     draws the input line in the dock from the cells fish
#                     painted, and what fish prints as the prompt is only the
#                     contract below. `off` leaves the user's prompt in place
#                     and adds the marks around it (the `"blocks"` setting).
#   BATERI_BIN, BATERI_SSH_INSTANCE, BATERI_SSH_CWD
#                     the `ssh` function's inputs, as in the zsh wrapper.
#
# The three of them are ours, so they are taken out of the environment too
# (children have no use for them).

if set -q BATERI_FISH_DIR
    set -l __bateri_dirs (string split : -- "$XDG_DATA_DIRS")
    if set -l __bateri_at (contains -i -- $BATERI_FISH_DIR $__bateri_dirs)
        set -e __bateri_dirs[$__bateri_at]
    end
    if set -q BATERI_XDG_UNSET; or not set -q __bateri_dirs[1]
        set -e XDG_DATA_DIRS
    else
        set -gx XDG_DATA_DIRS (string join : -- $__bateri_dirs)
    end
    set -e BATERI_FISH_DIR
    set -e BATERI_XDG_UNSET
end

# Read before the interactive check, so a non-interactive fish does not pass
# them on.
set -g __bateri_bin ""
set -g __bateri_ssh_instance ""
set -g __bateri_ssh_cwd ""
set -q BATERI_BIN; and set __bateri_bin $BATERI_BIN
set -q BATERI_SSH_INSTANCE; and set __bateri_ssh_instance $BATERI_SSH_INSTANCE
set -q BATERI_SSH_CWD; and set __bateri_ssh_cwd $BATERI_SSH_CWD
set -e BATERI_BIN BATERI_SSH_INSTANCE BATERI_SSH_CWD
if test "$BATERI_DOCK" = off
    set -g __bateri_dock 0
else
    set -g __bateri_dock 1
end
set -e BATERI_DOCK

status is-interactive; or return

# "Has a command run since the last prompt": `D` is printed only after a
# command that really ran. An Enter on an empty line draws a new prompt but no
# command finished.
set -g __bateri_ran 0
# Block counter. Every prompt opens a block and its id enters both OSC 133
# (`bt_block=`) and the prompt's cells (OSC 8): the terminal reads which line a
# block started on from the grid on every frame, so it stays right after
# scrolling and reflow. The field is ours, not `aid`, which the spec reserves
# for a constant application id.
set -g __bateri_block 0
# The text of `fish_prompt` as we left it (see `__bateri_wrap`).
set -g __bateri_defn ""

# Before the prompt: the finished command's code (`D`) then the new block (`A`).
#
# THE BLOCK ADVANCES HERE AND NOWHERE ELSE. fish 4 erases a cancelled line in
# place and runs no prompt function; a syntax error leaves the line editable.
# Neither starts a new prompt, so neither starts a new block (a counter moved by
# `fish_cancel` would leave the book at N+1 while the grid's cached anchor
# still says N). The prompt function itself is no place either: fish runs it
# again without a command to redraw (a resize, `repaint`).
function __bateri_begin --on-event fish_prompt
    set -g __bateri_block (math $__bateri_block + 1)
    printf '\e]133;A;bt_block=%s\a' $__bateri_block
    if test $__bateri_dock = 1
        __bateri_take
    else
        __bateri_wrap
    end
    __bateri_cwd
    if test $__bateri_dock = 1
        __bateri_branch
    end
    __bateri_ssh_arm
    __bateri_command_arm
end

# `D` closes the FINISHED block: the counter has not moved yet.
function __bateri_end --on-event fish_postexec
    set -l code $status
    if test $__bateri_ran = 1
        set -g __bateri_ran 0
        printf '\e]133;D;%s;bt_block=%s\a' $code $__bateri_block
    end
end

# Right before a command runs: the line is finished (`8133;e`, with a dock), the
# anchor closes, output begins (`C`).
#
# The link stays open through the whole input line: every cell fish draws
# carries the id. Closing it here puts the command's output outside it, so the
# mark sits on the command's own line, not on its output.
#
# `e` is what hands the line from the dock back to the grid in one step. Enter
# has already moved the cursor down a row by the time this runs, and without it
# the dock would show the new, empty row until `C` arrives. fish's own `C`
# comes first and ours right after (measured), so `e` cannot wait for ours.
function __bateri_start --on-event fish_preexec
    set -g __bateri_ran 1
    if test $__bateri_dock = 1
        printf '\e]8133;e\a'
    end
    printf '\e]8;;\a\e]133;C\a'
end

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
# this event that does it runs after ours and wins (the wrapper's known limit,
# below); that prompt's line is then drawn in the grid as the user wrote it.
function __bateri_take
    function fish_prompt --description 'bateri: the prompt the terminal draws a dock for'
        printf '\e]8;;bateri://block/%s\a ' $__bateri_block
        set -q fish_color_autosuggestion[1]; and set_color $fish_color_autosuggestion 2>/dev/null
        printf ' '
        set_color normal
        printf '\e]8133;g\a\e]133;B\a'
    end
    function fish_right_prompt --description 'bateri: the terminal draws the right side'
    end
    function fish_mode_prompt --description 'bateri: the terminal draws the mode'
    end
end

# The prompt the terminal reads blocks from when there is no dock (`BATERI_DOCK`
# is `off`): the user's own, with an anchor in front and the end mark behind.
#
# The user's function is COPIED and the copy is what the wrapper calls; it is
# never erased first, so a copy that fails leaves the user's prompt exactly as it
# was. The wrapper is redefined whenever the definition in force is not the one
# we left, which is also how a theme that rewrites `fish_prompt` after
# `config.fish` (or `fish_config prompt choose`) is taken back at the next
# prompt. A user who erased the function has no prompt to wrap and gets none.
#
# The anchor is an id-carrying OSC 8 link laid over the prompt's cells, in the
# prompt's own text so a prompt fish draws again from its cache keeps it. It
# is a prefix: OSC 8 does not nest, so a prompt link of the user's replaces ours
# for the cells after it (a known limit, a stripe missing, not a wrong one).
#
# A KNOWN LIMIT: a handler of the user's on the same event that rewrites
# `fish_prompt` runs after ours, as fish runs the handlers in the order they
# were defined, and wins every prompt; those blocks have no anchor, so no
# stripe, not a wrong one.
#
# The user's prompt runs FIRST, in a command substitution, so it sees the
# `$status` and `$pipestatus` fish handed to the prompt; any command before it
# would overwrite both. The lines are joined back the way fish joins the lines
# of the output of a prompt function, so a trailing newline is dropped exactly
# as fish drops it.
function __bateri_wrap
    functions -q fish_prompt; or return
    set -l current (functions fish_prompt | string collect)
    test "$current" = "$__bateri_defn"; and return
    functions -e __bateri_user_prompt
    functions -c fish_prompt __bateri_user_prompt; or return
    function fish_prompt --description 'bateri: the prompt with the block marks'
        set -l lines (__bateri_user_prompt)
        printf '\e]8;;bateri://block/%s\a' $__bateri_block
        set -q lines[1]; and printf '%s' $lines[1]
        set -q lines[2]; and printf '\n%s' $lines[2..-1]
        printf '\e]133;B\a'
    end
    set -g __bateri_defn (functions fish_prompt | string collect)
end

# Reports the working directory with OSC 7. The authority is empty
# (`file:///…`): the terminal's gate counts a named host as foreign, and an
# empty one cannot go stale when the machine is renamed. fish prints its own
# report too, with the host's name, where it recognises the terminal; ours is
# there for the fish versions that do not.
function __bateri_cwd
    printf '\e]7;file://%s\a' (string escape --style=url -- $PWD)
end

# Sends the git branch through the dock's channel; empty if it is not a
# repository. A single `git` fork in the usual case, per prompt. The
# environment report rides after it (`__bateri_env_fields`).
function __bateri_branch
    set -l ref (command git rev-parse --abbrev-ref HEAD 2>/dev/null)
    # `HEAD` is the answer of a detached HEAD, not a branch name: the short SHA.
    if test "$ref" = HEAD
        set ref (command git rev-parse --short HEAD 2>/dev/null)
    end
    set -l branch (printf %s "$ref" | command base64 | string join '')
    __bateri_env_fields
    printf '\e]8133;b;%s%s\a' "$branch" "$__bateri_env_wire"
end

# The environment the dock's context row shows, as five more `8133;b` fields:
# `VIRTUAL_ENV`, `VIRTUAL_ENV_PROMPT`, `CONDA_PREFIX`, `CONDA_PROMPT_MODIFIER`
# and the `node` fish would run, each base64, empty when unset — the zsh
# wrapper's report. The terminal reads the files they name and runs nothing;
# the prompt fish draws is ours, so the name `activate.fish` puts in front of
# the user's is not on screen.
#
# ENCODED ONLY WHEN SOMETHING CHANGED: each field is a `base64` fork, and the
# values change only at an `activate`, a `conda activate` or a `nvm use`.
# `command -s` is fish's own lookup, no fork. An empty field is an empty
# element, not a missing one, so the fields keep their places.
function __bateri_env_fields
    set -l node (command -s node)
    set -l fields "$VIRTUAL_ENV" "$VIRTUAL_ENV_PROMPT" "$CONDA_PREFIX" "$CONDA_PROMPT_MODIFIER" "$node"
    set -l raw (string join \x1f -- $fields)
    if set -q __bateri_env_wire; and test "$raw" = "$__bateri_env_raw"
        return
    end
    set -l out
    for field in $fields
        if test -n "$field"
            set -a out (printf %s "$field" | command base64 | string join '')
        else
            set -a out ''
        end
    end
    set -g __bateri_env_raw "$raw"
    set -g __bateri_env_wire (string join '' -- ';'$out)
end

# The `ssh` function, defined at the first prompt — after `config.fish` — so a
# `ssh` function or alias of the user's is seen and wins (a known limit: theirs
# bypasses the integration). Only with the binary's path.
function __bateri_ssh_arm
    set -q __bateri_ssh_armed; and return
    set -g __bateri_ssh_armed 1
    test -n "$__bateri_bin"; or return
    functions -q ssh; and return
    function ssh --description 'bateri: ssh through the remote integration'
        __bateri_ssh $argv
    end
end

# `bateri` as a command: the binary that started this shell, which is not on
# PATH (it lives inside the `.app`) — `bateri config set …`, `bateri help`.
# Defined at the first prompt, as `ssh` is, so a `bateri` function or alias of
# the user's, or a `bateri` command on PATH, is seen and wins.
function __bateri_command_arm
    set -q __bateri_command_armed; and return
    set -g __bateri_command_armed 1
    test -n "$__bateri_bin"; or return
    functions -q bateri; and return
    command -q bateri; and return
    function bateri --description 'bateri: run the bateri that started this shell'
        command $__bateri_bin $argv
    end
end

# Runs the user's `ssh`, wrapped when bateri says so. The rules are the zsh
# wrapper's, and the binary's (`ssh_wrap::decide`): which call is interactive,
# the settings, `ssh -G`. `bateri ssh-argv` prints the wrapped arguments, each
# followed by a NUL, or nothing — and nothing (also a missing binary or any
# failure) means plain `command ssh $argv`. `--tty` only when stdin and stdout
# are terminals. Inside a local multiplexer nothing is wrapped: the pane sees
# tmux or screen, not the ssh, so the session could never prove it ran.
# After a wrapped session ends `bateri ssh-fell-back` says whether to rerun it
# plain (a router, a server that refused our command).
function __bateri_ssh
    set -l cwd
    test -n "$__bateri_ssh_cwd"; and set cwd --cwd $__bateri_ssh_cwd
    set -g __bateri_ssh_cwd ""
    if set -q TMUX; or set -q STY
        command ssh $argv
        return
    end
    set -l tty
    isatty stdin; and isatty stdout; and set tty --tty
    set -l instance
    test -n "$__bateri_ssh_instance"; and set instance --instance $__bateri_ssh_instance
    set -l wrapped
    if test -x "$__bateri_bin"
        set wrapped (command $__bateri_bin ssh-argv $tty --block $__bateri_block $instance $cwd -- $argv 2>/dev/null | string split0)
    end
    if not set -q wrapped[1]
        command ssh $argv
        return
    end
    command ssh $wrapped
    set -l code $status
    set -l plain (command $__bateri_bin ssh-fell-back --rc $code $instance -- $wrapped 2>/dev/null | string split0)
    if set -q plain[1]
        command ssh $plain
        return
    end
    return $code
end
