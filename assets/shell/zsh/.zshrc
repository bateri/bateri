# bateri — the zsh wrapper. The body is in `bateri.zsh`.
#
# The hooks are attached AFTER the user's file: `add-zsh-hook` appends, and we
# end up being the last party to touch the prompt.
#
# ZDOTDIR is restored here, because this is the last file of ours read in an
# interactive shell: the remaining `.zlogin` and `.zlogout` are now read from
# the user's directory, so no separate file is needed to hand them over.
#
# The reason for the first line is the same as in `.zprofile`: the file is
# self-sufficient (the reachable case is an unreadable `.zshenv`, not `no_rcs`).
if (( $+functions[__bateri_begin] )) || source ${ZDOTDIR}/bateri.zsh 2>/dev/null; then
  __bateri_begin .zshrc
  [[ -n $__bateri_file ]] && source $__bateri_file
  __bateri_end
  __bateri_hooks
  __bateri_restore
elif [[ -n ${BATERI_ZDOTDIR} ]]; then
  export ZDOTDIR=$BATERI_ZDOTDIR
  unset BATERI_ZDOTDIR
else
  unset ZDOTDIR BATERI_ZDOTDIR
fi
