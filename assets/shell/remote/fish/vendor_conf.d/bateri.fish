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

status is-interactive; or exit

# Reports the working directory with OSC 7, per prompt. The authority is the
# server's name (the zsh wrapper's reason).
function __bateri_cwd --on-event fish_prompt
    printf '\e]7;file://%s%s\a' $hostname (string escape --style=url -- $PWD)
end
