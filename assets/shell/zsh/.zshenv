# bateri — the first link of the zsh wrapper. The body is in `bateri.zsh`.
#
# This file is read by every zsh; the remaining three files are read only if
# ZDOTDIR still points at us, so the fallback here covers them too.
#
# `source` is at the TOP LEVEL of this file and that is a requirement: the
# `typeset`s of an rc file loaded from inside a function would stay local
# (`bateri.zsh`'s header).
if (( $+functions[__bateri_begin] )) || source ${ZDOTDIR}/bateri.zsh 2>/dev/null; then
  __bateri_begin .zshenv
  [[ -n $__bateri_file ]] && source $__bateri_file
  __bateri_end
  # THE TWO SHELLS IN WHICH NO OTHER FILE OF OURS WILL BE READ, both gathered
  # here — since `__bateri_restore` `unfunction`s itself, two separate
  # conditions cannot fire twice:
  #   - `no_rcs`: a user file said `setopt no_rcs`; from that point zsh reads NO
  #     other startup file (`/etc/*` included), so this is the last chance to
  #     restore.
  #   - neither interactive nor login (`zsh -c`): zsh reads only `.zshenv`. If
  #     we do not restore, ZDOTDIR stays hanging on us in that shell and every
  #     zsh born from it loses the user's configuration. The window is real:
  #     `/etc/zprofile` and `/etc/zshrc` run right there.
  if [[ ! -o rcs || ( ! -o interactive && ! -o login ) ]]; then
    __bateri_restore
  fi
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  # The body could not be read: no integration, but the shell still opens and the
  # user's configuration stays in place — zsh reads the remaining files from its directory.
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
