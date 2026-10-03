# bateri's zsh wrapper — the ZDOTDIR swap.
#
# Shared by the local wrapper (`bateri.zsh`, loaded by the four files in our
# ZDOTDIR) and the remote one (the bootstrap writes the same four files and
# this file to the server, with its own small `bateri.zsh` beside them —
# `assets/shell/remote/zsh/bateri.zsh`). The swap is one file so the two
# wrappers cannot drift apart: the day they diverged one of them would break
# silently. The rationale for the shape (why the
# user's file is `source`d at the top level, why ZDOTDIR is re-read, the
# `HISTFILE` fix) is in `bateri.zsh`'s header.
#
# CONTRACT: the caller `source`s this file at its top level, once, from its
# own body; it defines `__bateri_begin`, `__bateri_end` and `__bateri_restore`,
# and the caller defines `__bateri_hooks` (`.zshrc` calls it, the restore
# removes it). It also holds `__bateri_percent`, OSC 7's encoder, which both
# wrappers' `__bateri_cwd` use — one encoder, not two copies that could drift.

# Our directory. zsh used ZDOTDIR to find this file, so the value is ours right
# now; `${0:A:h}` is only a fallback for the `unsetopt function_argzero` edge.
: ${__bateri_dir:=${ZDOTDIR:-${0:A:h}}}

# The user's directory and WHETHER THEIR ZDOTDIR EXISTS — two separate pieces of
# information. Determined once: `.zshenv` is read by every zsh and we take the
# environment variable from there and delete it.
if (( ! ${+__bateri_had} )); then
  # A SELF-POINTING VALUE IS REJECTED: if `BATERI_ZDOTDIR` points at our own
  # directory we would put ourselves back instead of "the user's original
  # value" — `__bateri_begin` would reload our own `.zshenv` and recurse up to
  # zsh's FUNCNEST limit (measured: 336 lines of errors, the session is left
  # without ZDOTDIR). The first layer of the gate is on the Rust side
  # (`shell_integration_env`); this second layer is for the cases where the
  # environment is set up by hand.
  # The right-hand side is QUOTED: inside `[[ ]]` an unquoted right operand is a
  # **glob pattern**, not plain text. Had the package sat at a path like
  # `/Applications/[dev] bateri.app/…` the pattern would not match its own plain
  # value, the gate would open and we would fall into exactly the recursion it
  # prevents.
  if [[ -n ${BATERI_ZDOTDIR} && ${BATERI_ZDOTDIR:A} != "${__bateri_dir:A}" ]]; then
    __bateri_had=1
    __bateri_user=$BATERI_ZDOTDIR
  else
    __bateri_had=0
    __bateri_user=$HOME
  fi
  unset BATERI_ZDOTDIR
fi

# PREPARES the loading of the user's same-named startup file; the calling file
# does the loading at its own top level.
#
# ZDOTDIR carries the USER's value during loading, for two reasons: so the file
# itself sees `$ZDOTDIR` correctly, and so that child processes born from that
# file (brew shellenv, nvm, direnv) do not inherit our directory.
__bateri_begin() {
  if (( __bateri_had )); then
    ZDOTDIR=$__bateri_user
  else
    unset ZDOTDIR
  fi
  # The SYSTEM's rc file was read BEFORE us and ZDOTDIR was pointing at us at
  # that time: macOS's `/etc/zshrc` sets `HISTFILE` to
  # `${ZDOTDIR:-$HOME}/.zsh_history`. If we did not fix it the user's command
  # history would be written into the application's package and their own file
  # would freeze — and the symptom would be silent. The fix comes BEFORE the
  # user's file is loaded, because their rc can read `HISTFILE` and build on it.
  # A user who wrote their own path is not touched: the condition only catches a
  # value that points at OUR directory.
  if [[ -n $HISTFILE && $HISTFILE == "$__bateri_dir"/* ]]; then
    HISTFILE=${ZDOTDIR:-$HOME}/${HISTFILE#"$__bateri_dir"/}
  fi
  # `typeset -g`: the one who will read the value is the top level of the CALLING
  # file, not this function. `-r` filters out an unreadable file (missing, no
  # permission) with an empty value; `source`'s own error is not fatal either —
  # a syntax error abandons that file, not the shell.
  local file=${ZDOTDIR:-$HOME}/$1
  if [[ -r $file ]]; then
    typeset -g __bateri_file=$file
  else
    typeset -g __bateri_file=
  fi
}

# Loading is done: re-reads the user's value and takes ZDOTDIR back to us.
#
# The value is RE-READ: the most common way for a user to have a ZDOTDIR is to
# assign it inside `~/.zshenv`. Had we not read it we would look for the
# remaining files in the old directory, i.e. miss exactly the configuration the
# user moved.
__bateri_end() {
  if (( ${+ZDOTDIR} )); then
    __bateri_had=1
    __bateri_user=$ZDOTDIR
  else
    __bateri_had=0
    __bateri_user=$HOME
  fi
  ZDOTDIR=$__bateri_dir
  unset __bateri_file
}

# Restores the user's ZDOTDIR PERMANENTLY and erases our traces.
#
# Its callers are `.zshrc` and `.zlogin`, whichever is read; also `.zshenv`, in
# shells where no file of ours other than itself will be read (`no_rcs`; or
# `zsh -c`, which is neither interactive nor login).
__bateri_restore() {
  if (( __bateri_had )); then
    export ZDOTDIR=$__bateri_user
  else
    unset ZDOTDIR
  fi
  unset __bateri_dir __bateri_user __bateri_had __bateri_file
  # The hooks stay, the loader goes: the first works throughout the session, the
  # second's job is done and there is no point in it staying in the user's namespace.
  unfunction __bateri_begin __bateri_end __bateri_hooks __bateri_restore
}

# Hexadecimal digits, for the two digits of percent encoding.
typeset -ga __bateri_hex
__bateri_hex=( 0 1 2 3 4 5 6 7 8 9 A B C D E F )

# Percent-encodes `$1` (RFC 3986's "unreserved" set + `/`); the result is in `REPLY`.
#
# NO FORK, the same reason as `__bateri_b64` and the same two subtleties:
# `nomultibyte` makes every element a BYTE (percent encoding is of bytes, not
# characters) and the byte value is first taken into a scalar (`x=…`, then
# `#x`) — the arithmetic `##` form interprets escape sequences and reads the
# backslash as 32 instead of 92.
#
# `/` IS NOT ENCODED: it is the path separator and an encoded `/` would make the
# path look like a single component. The decoding side reads both, so this is
# not a necessity but readability: the user's path stays human-readable to us
# too.
__bateri_percent() {
  emulate -L zsh
  setopt nomultibyte
  REPLY=
  [[ -n $1 ]] || return 0
  local -a bytes
  bytes=( ${(s::)1} )
  local out= x
  local -i v
  for x in $bytes; do
    if [[ $x == [A-Za-z0-9/._~-] ]]; then
      out+=$x
    else
      v=$(( #x ))
      out+='%'${__bateri_hex[$(( (v >> 4) + 1 ))]}${__bateri_hex[$(( (v & 15) + 1 ))]}
    fi
  done
  REPLY=$out
}
