# bateri's REMOTE fish wrapper (048) — a `vendor_conf.d` file.
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
    set -l __bateri_dirs (string split : -- "$XDG_DATA_DIRS")
    if set -l __bateri_at (contains -i -- $BATERI_FISH_DIR $__bateri_dirs)
        set -e __bateri_dirs[$__bateri_at]
    end
    if set -q __bateri_dirs[1]
        set -gx XDG_DATA_DIRS (string join : -- $__bateri_dirs)
    else
        set -e XDG_DATA_DIRS
    end
    set -e BATERI_FISH_DIR
end

# The blocks' parent (048 phase-3): the local `ssh` block the bootstrap
# exported; ours, so out of the environment (the zsh wrapper's reason). Read
# before the interactive check, so a non-interactive fish does not pass it on.
if set -q BATERI_RBLOCK
    set -g __bateri_rblock $BATERI_RBLOCK
    set -e BATERI_RBLOCK
end

status is-interactive; or exit

# Reports the working directory with OSC 7, per prompt. The authority is the
# server's name (the zsh wrapper's reason).
function __bateri_cwd --on-event fish_prompt
    printf '\e]7;file://%s%s\a' $hostname (string escape --style=url -- $PWD)
end

# The command blocks on the server (048 phase-3): the zsh wrapper's `D`/`A`/`C`
# with our remote field, `bt_remote=<P>.<S>.<n>` (`P` the local `ssh` block,
# `S` this shell's pid — two shells under one `ssh` command line are two
# trails — and `n` its counter; `fish_pid` is fish 3's, older fish gets none) — never `bt_block=`, which bateri reads as the local
# shell's. fish 4's own OSC 133 carries no identity and does not reach our
# trail. The anchor is printed when the prompt is about to be drawn and closed
# before the command runs; the user's prompt is not touched; no `B` (its
# consumers are the local dock's). `D` only after a command we saw start:
# `fish_postexec` carries its `$status`.
if string match -qr '^[0-9]+$' -- "$__bateri_rblock"; and set -q fish_pid
    set -g __bateri_rshell $__bateri_rblock.$fish_pid
    set -g __bateri_block 0
    set -g __bateri_ran 0

    function __bateri_end --on-event fish_postexec
        set -l code $status
        if test $__bateri_ran = 1
            set -g __bateri_ran 0
            printf '\e]133;D;%s;bt_remote=%s.%s\a' $code $__bateri_rshell $__bateri_block
        end
    end

    function __bateri_prompt --on-event fish_prompt
        set -g __bateri_block (math $__bateri_block + 1)
        printf '\e]133;A;bt_remote=%s.%s\a\e]8;;bateri://rblock/%s.%s\a' \
            $__bateri_rshell $__bateri_block $__bateri_rshell $__bateri_block
    end

    function __bateri_start --on-event fish_preexec
        set -g __bateri_ran 1
        printf '\e]8;;\a\e]133;C;bt_remote=%s.%s\a' $__bateri_rshell $__bateri_block
    end
end
