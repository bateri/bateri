# bateri — the zsh wrapper. The body is in `bateri.zsh`.
#
# Read BEFORE `.zshrc` in a login shell and handing it over is mandatory: in
# most setups the user's PATH (Homebrew, nvm, asdf) is born here. A wrapper
# that handed over only `.zshrc` would silently drop it.
#
# The first line of the skeleton makes this file SELF-SUFFICIENT: if the body
# is not loaded it loads it itself. The reachable case is our `.zshenv` having
# gone unread (a missing or unreadable file — a hand-corrupted package); NOT
# `no_rcs`, because once that option is on zsh reads no other startup file at
# all, `/etc/zprofile` included (`/code-review`, 009 phase-5: the rationale in
# the first draft was wrong). A bare `return` still will not do — `ZDOTDIR`
# stays hanging on us, `BATERI_ZDOTDIR` stays exported and the fallback the
# `elif` arm performs would never run.
if (( $+functions[__bateri_begin] )) || source ${ZDOTDIR}/bateri.zsh 2>/dev/null; then
  __bateri_begin .zprofile
  [[ -n $__bateri_file ]] && source $__bateri_file
  __bateri_end
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
