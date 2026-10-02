# bateri's REMOTE bash wrapper (048) — read as `$ENV` by `bash --posix -l`.
#
# WHY POSIX MODE: `--rcfile` is ignored by a login shell, and a login shell is
# what sshd would have started. In POSIX mode an interactive bash reads
# `$ENV` and no other startup file (kitty's method), so this file runs first,
# leaves POSIX mode and reads the login files itself — in bash's own order:
# `/etc/profile`, then the first readable of `~/.bash_profile`,
# `~/.bash_login`, `~/.profile`. The shell stays a real login shell: `logout`
# works and `~/.bash_logout` is read on exit. Before bash 4 the bootstrap
# starts it with `--rcfile` instead (`boot.sh`); this file is the same.
#
# THE USER'S FILES ARE READ AT THE TOP LEVEL, not from a function: `declare`
# and `local` inside a function are local, so `declare -x PATH=…` would be
# undone on return (the trap the zsh wrapper measured, 009 phase-5). They are
# read, never written (the `make audit` gate).

# `ENV` is ours only for this start: the user's own value comes back, and
# without one it goes, so child shells do not read this file. (Under
# `--rcfile`, before bash 4, the bootstrap left `ENV` as it was and saved it
# all the same, so the same lines put it back.) The history file the
# bootstrap named for POSIX mode is bash's default, not an export of the
# user's.
if [ -n "${BATERI_ENV+x}" ]; then
  ENV=$BATERI_ENV
  unset BATERI_ENV
else
  unset ENV
fi
if [ "${BATERI_HISTFILE-}" = unexport ]; then
  export -n HISTFILE
fi
unset BATERI_HISTFILE
set +o posix

if [ -r /etc/profile ]; then
  . /etc/profile
fi
for __bateri_login in "$HOME/.bash_profile" "$HOME/.bash_login" "$HOME/.profile"; do
  if [ -r "$__bateri_login" ]; then
    . "$__bateri_login"
    break
  fi
done
unset __bateri_login

# Reports the working directory with OSC 7, per prompt. The status of the
# command that ran is kept: a `PROMPT_COMMAND` after ours may read `$?`.
#
# THE AUTHORITY IS THE SERVER'S NAME (the zsh wrapper's reason): a foreign
# authority reaches bateri's remote slot even before the remote probe lands.
__bateri_cwd() {
  local status=$? REPLY
  __bateri_percent "$PWD"
  printf '\033]7;file://%s%s\007' "$HOSTNAME" "$REPLY"
  return $status
}

# Percent-encodes `$1` (RFC 3986's unreserved set + `/`) into `REPLY`, byte by
# byte: `LC_ALL=C` makes every element a byte and `'c` its value — masked,
# because bash 3.2 sign-extends a byte above 0x7F (`ğ` would read as
# `%FFFFFFFFFFFFFFC4`).
__bateri_percent() {
  local LC_ALL=C s=$1 out= c i n
  for (( i = 0; i < ${#s}; i++ )); do
    c=${s:i:1}
    case $c in
      [A-Za-z0-9/._~-]) out+=$c ;;
      *)
        printf -v n '%d' "'$c"
        printf -v c '%%%02X' $(( n & 255 ))
        out+=$c
        ;;
    esac
  done
  REPLY=$out
}

# First, so `$?` is still the command's when it runs; appended to whatever the
# user's files set (a bash 5.1 array keeps its other elements).
PROMPT_COMMAND="__bateri_cwd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
