# bateri's REMOTE fish wrapper — a `vendor_conf.d` file.
#
# The bootstrap put its directory first in `XDG_DATA_DIRS` and named it in
# `BATERI_FISH_DIR`; fish reads this file with its other configuration
# snippets, before the user's `config.fish`, which it does not touch. The
# first thing it does is take our directory out again, so child processes see
# the user's `XDG_DATA_DIRS` (unset if it was unset).
#
# fish's own OSC 7 depends on its version and on recognising the terminal
# (`TERM_PROGRAM`), which the bootstrap cannot tell; a second, identical
# report is harmless, so ours is always printed.

if set -q BATERI_FISH_DIR
    set -g __bateri_bin $BATERI_FISH_DIR/bin
    set -l __bateri_dirs (string split : -- "$XDG_DATA_DIRS")
    if set -l __bateri_at (contains -i -- $BATERI_FISH_DIR $__bateri_dirs)
        set -e __bateri_dirs[$__bateri_at]
    end
    # Unset stays unset: the bootstrap wrote fish's own defaults after ours so
    # that fish's search paths survived the startup, and says so.
    if set -q BATERI_XDG_UNSET; or not set -q __bateri_dirs[1]
        set -e XDG_DATA_DIRS
    else
        set -gx XDG_DATA_DIRS (string join : -- $__bateri_dirs)
    end
    set -e BATERI_FISH_DIR BATERI_XDG_UNSET
end

# The blocks' parent: the local `ssh` block the bootstrap
# exported; ours, so out of the environment (the zsh wrapper's reason). Read
# before the interactive check, so a non-interactive fish does not pass it on.
if set -q BATERI_RBLOCK
    set -g __bateri_rblock $BATERI_RBLOCK
    set -e BATERI_RBLOCK
end
# The dock's tier is the Mac's `[shell] integration`: `off` there is `"blocks"` (no dock), anything
# else is the dock. It is READ, not taken out: it stays in the environment so that `bateri`, the
# agent's command, hands it to the terminals it opens (`bin/bateri`).
set -g __bateri_dock 1
if test "$BATERI_DOCK" = off
    set -g __bateri_dock 0
end

status is-interactive; or exit

# `bateri`, the agent's command (`bin/bateri`), on `PATH` once. This file is
# read before the user's `config.fish`, so it is done at the first prompt, after
# it; and with `set -g`, for this session only (`fish_add_path` would write a
# universal variable to the user's configuration).
if set -q __bateri_bin
    function __bateri_path --on-event fish_prompt
        functions -e __bateri_path
        if test -x $__bateri_bin/bateri; and not contains -- $__bateri_bin $PATH
            set -gx PATH $__bateri_bin $PATH
        end
        set -e __bateri_bin
    end
end

# Reports the working directory with OSC 7, per prompt. The authority is the
# server's name (the zsh wrapper's reason).
function __bateri_cwd --on-event fish_prompt
    printf '\e]7;file://%s%s\a' $hostname (string escape --style=url -- $PWD)
end

# The command blocks on the server: the zsh wrapper's `D`/`A`/`C`
# with our remote field, `bt_remote=<P>.<S>.<n>` (`P` the local `ssh` block,
# `S` this shell's pid — two shells under one `ssh` command line are two
# trails — and `n` its counter; `fish_pid` is fish 3's, older fish gets none) — never `bt_block=`, which bateri reads as the local
# shell's. fish 4's own OSC 133 carries no identity and does not reach our
# trail. WITHOUT A DOCK (`BATERI_DOCK=off`, the Mac's `"blocks"`) the anchor is printed when the
# prompt is about to be drawn and closed before the command runs; the user's prompt is not
# touched; no `B`. WITH ONE the prompt is the terminal's, as on the Mac (`dock.fish`): the anchor,
# the `B` and the dock's events all carry the same identity. `D` only after a command we saw
# start: `fish_postexec` carries its `$status`.
if string match -qr '^[0-9]+$' -- "$__bateri_rblock"; and set -q fish_pid
    set -g __bateri_rshell $__bateri_rblock.$fish_pid
    set -g __bateri_block 0
    set -g __bateri_ran 0

    # The dock's prompt (`dock.fish`, one level above this directory, shared with the Mac's own
    # wrapper). Without the file the shell is as it is without a dock.
    if test $__bateri_dock = 1
        set -l __bateri_dock_file (status dirname)/../dock.fish
        if test -r $__bateri_dock_file
            source $__bateri_dock_file
        else
            set -g __bateri_dock 0
        end
    end

    function __bateri_end --on-event fish_postexec
        set -l code $status
        if test $__bateri_ran = 1
            set -g __bateri_ran 0
            # The screen a program that is over left on, before `D`, and not while a
            # job is stopped (the local wrapper's reasons).
            if not jobs 2>/dev/null | string match -qr '\tstopped\t'
                printf '\e[<4096u\e[?1049l'
            end
            printf '\e]133;D;%s;bt_remote=%s.%s\a' $code $__bateri_rshell $__bateri_block
            # The input modes the command left on go with it, and the title it set (the
            # local wrapper's reasons).
            printf '\e[?1000;1002;1003;1004;1005;1006;1016;2048l\e[<4096u\e[23t'
        end
    end

    function __bateri_prompt --on-event fish_prompt
        set -g __bateri_block (math $__bateri_block + 1)
        printf '\e]133;A;bt_remote=%s.%s\a' $__bateri_rshell $__bateri_block
        if test $__bateri_dock = 1
            # The anchor is in the prompt itself (`__bateri_take`), so that a prompt fish draws again
            # from its cache keeps it; the identity of this prompt is set before it is made.
            set -g __bateri_dock_anchor bateri://rblock/$__bateri_rshell.$__bateri_block
            set -g __bateri_dock_tag "bt_remote=$__bateri_rshell.$__bateri_block;"
            set -g __bateri_dock_mark ";bt_remote=$__bateri_rshell.$__bateri_block"
            __bateri_take
        else
            printf '\e]8;;bateri://rblock/%s.%s\a' $__bateri_rshell $__bateri_block
        end
    end

    # Right before a command runs: with a dock the line is finished first (`8133;e`: Enter has
    # already moved the cursor down a row, and until `e` arrives the dock would show the new, empty
    # row), then the anchor closes and output begins (`C`).
    function __bateri_start --on-event fish_preexec
        set -g __bateri_ran 1
        if test $__bateri_dock = 1
            printf '\e]8133;bt_remote=%s.%s;e\a' $__bateri_rshell $__bateri_block
        end
        printf '\e]8;;\a\e]133;C;bt_remote=%s.%s\a\e[22t' $__bateri_rshell $__bateri_block
    end
end
