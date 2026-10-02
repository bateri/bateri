# bateri's remote bootstrap (048) — POSIX sh, run on the SERVER.
#
# HOW IT GETS THERE: the local `ssh` function asks `bateri ssh-argv`, which
# wraps the user's `ssh` as `ssh -t <their arguments> "exec sh -c '<one-liner>'
# bateri-boot"` (`bt-shell-common::ssh_wrap`). sshd runs that through the
# user's login shell (`$SHELL -c`, which may be fish or csh), `exec` replaces
# it with `sh`, and the one-liner decodes this file (base64, with the
# decoders' fallback chain) and `eval`s it. This file is therefore free of the
# one-liner's quoting limits; the one-liner is built in Rust, next to the rule
# that keeps it readable by every login shell.
#
# THE FIRST LINE IS A MAGIC LINE: the one-liner runs only a decoded text that
# starts with it — a `base64` that ignored `-d` and printed garbage is never
# `eval`ed. `ssh_wrap` prepends it, together with `bt_files`, the function that
# writes the integration's files (each one's content as a single-quoted
# literal, so no here-document — bash as `sh` would put one in `/tmp`).
#
# WHAT IT DOES, IN ORDER:
#   1. prints the motd — sshd does not when it runs a command (`do_login` is
#      the interactive login's path). A known limit both ways: a server that
#      turned the motd off for ssh (`PrintMotd no`, no pam_motd) shows it now,
#      and the one-liner's decode fallback prints none; `Last login:` is lost and that is
#      accepted (048 discussion → Karar). `~/.hushlogin` silences it, as it
#      silences sshd's.
#   2. checks the login shell: zsh, bash and fish get the integration; any
#      other (sh, csh, BusyBox ash) gets a plain login shell (R3.4).
#   3. writes the files under `${XDG_DATA_HOME:-~/.local/share}/bateri/shell/`
#      — temporary name + `mv`, owner-only, nothing in `/tmp`, no log (R3.2).
#      The same fixed path every time: a shell reads its files when it starts,
#      so rewriting them under a running shell is harmless and nothing piles
#      up.
#   4. `exec`s the login shell with the integration: zsh through the
#      `ZDOTDIR` swap, bash in POSIX mode with `ENV`, fish through
#      `XDG_DATA_DIRS` → `vendor_conf.d`. Each one reads the user's own login
#      files, unchanged; no rc file on the server is written (R3.1).
#
# EVERY FAILURE IS A PLAIN LOGIN SHELL, never a broken connection: the reason
# goes to bateri as `ESC ] 8133 ; f ; {code} BEL` (`write`, `shell`; the
# one-liner says `decode`) and the pane's label shows it. The code is a fixed
# word, never text from the server.
#
# THE LOGIN SHELL'S `$0` IS ITS PATH, not `-zsh`: `exec -a` is not POSIX, so
# the shell is started with `-l` (and bash with `--posix -l`). A login file
# that tests `$0` for a leading `-` sees a difference — a known and narrow one.

bt_fault() {
  printf '\033]8133;f;%s\007' "$1"
}

bt_login() {
  exec "${SHELL:-/bin/sh}" -l
}

# 1. The motd: Ubuntu's PAM writes the dynamic one at every login (also this
# one) and prints it before the static file; the two can be one file.
if [ ! -e "$HOME/.hushlogin" ]; then
  if [ -r /run/motd.dynamic ]; then
    cat /run/motd.dynamic
  fi
  if [ -r /etc/motd ] && ! [ /etc/motd -ef /run/motd.dynamic ]; then
    cat /etc/motd
  fi
fi

# 2. The login shell, by name.
bt_shell=${SHELL##*/}
case $bt_shell in
  zsh | bash | fish) ;;
  *)
    bt_fault shell
    bt_login
    ;;
esac

# 3. The files. A relative `XDG_DATA_HOME` is ignored, as the spec says.
bt_data=${XDG_DATA_HOME:-}
case $bt_data in
  /*) ;;
  *) bt_data=$HOME/.local/share ;;
esac
bt_dir=$bt_data/bateri/shell

# Writes `$2` to `$bt_dir/$1` under a temporary name, then renames it; a file
# that is already the same is left as it is (no rename on every connection).
bt_put() {
  bt_tmp=$bt_dir/$1.tmp.$$
  if printf '%s' "$2" > "$bt_tmp"; then
    if cmp -s "$bt_tmp" "$bt_dir/$1"; then
      rm -f "$bt_tmp"
      return 0
    fi
    if mv -f "$bt_tmp" "$bt_dir/$1"; then
      return 0
    fi
  fi
  rm -f "$bt_tmp"
  return 1
}

# In a subshell: the owner-only `umask` must not reach the user's shell.
if ! (umask 077 && mkdir -p "$bt_dir/zsh" "$bt_dir/bash" "$bt_dir/fish/vendor_conf.d" && bt_files) 2>/dev/null; then
  bt_fault write
  bt_login
fi

# 4. The login shell with the integration.
case $bt_shell in
  zsh)
    # The `ZDOTDIR` swap (`zsh/zdotdir.zsh`, shared with the local wrapper).
    # A `ZDOTDIR` in this environment is NOT the user's starting value: sshd
    # ran the first hop as `zsh -c`, which already read `~/.zshenv`, and the
    # common `export ZDOTDIR=~/.config/zsh` there would make the swap skip
    # `~/.zshenv` itself. A plain `ssh` login starts without one, so the swap
    # starts without one too and reads `~/.zshenv`, which sets it again.
    unset BATERI_ZDOTDIR
    ZDOTDIR=$bt_dir/zsh
    export ZDOTDIR
    bt_login
    ;;
  bash)
    # `--rcfile` is ignored by a login shell, so bash starts in POSIX mode,
    # where an interactive shell reads only `$ENV` (kitty's method); our file
    # leaves POSIX mode and reads the login files in bash's own order. The
    # user's `ENV`, if any, travels in `BATERI_ENV`. POSIX mode's history file
    # would be `~/.sh_history`: without a `HISTFILE` of the user's, bash's own
    # default is set here and our file unexports it again.
    #
    # BEFORE BASH 4, `--rcfile` (measured: Apple's `/bin/bash` 3.2 ignores
    # `$ENV` under `--posix`): an interactive non-login shell that reads our
    # file, which reads the login files all the same — only `logout` and
    # `~/.bash_logout` are lost there.
    if [ -n "${ENV+x}" ]; then
      BATERI_ENV=$ENV
      export BATERI_ENV
    fi
    bt_rc=$bt_dir/bash/bateri.bash
    bt_major=$("$SHELL" -c 'echo "${BASH_VERSINFO[0]}"' 2>/dev/null)
    case $bt_major in
      [4-9] | [1-9][0-9])
        if [ -z "${HISTFILE+x}" ]; then
          HISTFILE=$HOME/.bash_history
          BATERI_HISTFILE=unexport
          export HISTFILE BATERI_HISTFILE
        fi
        ENV=$bt_rc
        export ENV
        exec "$SHELL" --posix -l
        ;;
      *)
        exec "$SHELL" --rcfile "$bt_rc" -i
        ;;
    esac
    ;;
  fish)
    # fish reads `vendor_conf.d` from every `XDG_DATA_DIRS` entry; ours is put
    # first and our file takes it out again (`BATERI_FISH_DIR` names it).
    # Unset stays unset: fish then falls back to its own data directory.
    BATERI_FISH_DIR=$bt_dir
    export BATERI_FISH_DIR
    XDG_DATA_DIRS=$bt_dir${XDG_DATA_DIRS:+:$XDG_DATA_DIRS}
    export XDG_DATA_DIRS
    bt_login
    ;;
esac
bt_login
