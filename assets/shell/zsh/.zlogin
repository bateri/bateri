# bateri — the zsh wrapper. The body is in `bateri.zsh`.
#
# This file runs only in a login shell in which `.zshrc` is not read
# (non-interactive): in an interactive one `.zshrc` has already restored
# ZDOTDIR and zsh looks for `.zlogin` in the user's directory. Our session is
# interactive, so this is not the usual path — the file exists so that ZDOTDIR
# is left hanging on us in no arm.
#
# No hook is attached: a non-interactive shell has no prompt, hence no mark.
#
# The reason for the first line is the same as in `.zprofile`: the file is
# self-sufficient (the reachable case is an unreadable `.zshenv`, not `no_rcs`).
if (( $+functions[__bateri_begin] )) || source ${ZDOTDIR}/bateri.zsh 2>/dev/null; then
  __bateri_begin .zlogin
  [[ -n $__bateri_file ]] && source $__bateri_file
  __bateri_end
  __bateri_restore
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
