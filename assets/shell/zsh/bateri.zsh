# bateri's zsh wrapper — the shared body.
#
# Loaded by the four files in our ZDOTDIR (`.zshenv`, `.zprofile`, `.zshrc`,
# `.zlogin`). The body holds the ZDOTDIR swap and the hooks; `source` is
# **not here**, it is at the top level of each file.
#
# THE USER'S FILE IS NOT `source`D FROM INSIDE A FUNCTION and the
# cost of this rule was measured: in zsh a `typeset` inside a function is LOCAL,
# so `typeset -U path; path+=(…)` — the standard PATH idiom of Homebrew, asdf,
# pyenv and nvm — would be deleted on return. The symptom is silent: the user's
# tools vanish only in bateri and work in every other terminal. The same
# boundary also broke the positional parameters (the file saw `$#` as 1). That
# is why the body was split in two: [`__bateri_begin`] prepares, the file does
# a top-level `source`, [`__bateri_end`] collects.
#
# CONTRACT (the setting side is `bt-shell-macos`'s `app::shell_integration_env`):
#   ZDOTDIR         this directory
#   BATERI_ZDOTDIR  the user's original ZDOTDIR. If it is absent from the
#                   environment the user had none either; on restore ZDOTDIR is
#                   unset, not set equal to $HOME — an exported ZDOTDIR and a
#                   ZDOTDIR that does not exist at all are different things to
#                   children.
#   BATERI_DOCK     if `off`, there is NO DOCK in this session
#                   (`[shell] integration = "blocks"`): the prompt stays the
#                   user's and the arms that feed the dock are never set up. Its
#                   absence is the default, i.e. the dock EXISTS. The terminal
#                   makes the decision, it is not asked here.
#
# A KNOWN AND LIMITED DIFFERENCE: inside a top-level `source` zsh sets `$0` to
# the path of the file being loaded; at a real startup it would be the shell's
# name. The fix would be to turn `function_argzero` off temporarily — flipping
# an option around the user's code is exactly the invisible mutation we avoid.
# kitty's wrapper accepts the same difference.
#
# NOT FATAL IN ANY ARM: there is no `exit`, each of the user's files is read
# guarded. The reason is harsh — when the child dies the app closes
# (`bt-shell-macos`'s `child_exit` → `terminate:` path), so a wrapper that
# fell over would leave the user unable to even reach Settings….
#
# THE USER'S FILES ARE NOT WRITTEN TO, only read (the `make audit` gate).
#
# The SYSTEM's rc files (`/etc/zshrc`) are read BEFORE ours at every stage and
# ZDOTDIR points at us at that time. The only item that writes is `HISTFILE`
# and [`__bateri_begin`] fixes it. The remaining item is read-only and was left
# deliberately: `/etc/zshrc` looks for `${ZDOTDIR:-$HOME}/.zkbd/${TERM}-${VENDOR}`,
# so a user who generated key bindings with `~/.zkbd` cannot load them and falls
# back to the default from terminfo. The fix would be to copy the system's rc
# logic — a fragile duplicate that depends on the macOS version; there is no
# data loss.
#
# We have NO `.zlogout` and this is not an omission: ZDOTDIR is restored to the
# user at the latest in `.zlogin`, so on exit zsh already reads the user's own
# `.zlogout`. Had we put a fifth file it would have run in no arm — deferring
# the restore would mean leaking ZDOTDIR to children for the whole session
# (tmux, nested shell).

# THE ZDOTDIR SWAP is in `zdotdir.zsh`, shared with the remote wrapper:
# our directory, the user's original ZDOTDIR, `__bateri_begin`/`__bateri_end`
# and `__bateri_restore`. `source`d at the top level (the `typeset` rule above);
# if it cannot be read this body fails and the calling file's fallback arm
# hands ZDOTDIR back.
source ${ZDOTDIR:-${0:A:h}}/zdotdir.zsh || return 1

# THE DOCK'S ZLE SIDE is in `dock.zsh`, shared with the remote wrapper: the display mirror, the
# edit widget and its binding, the prompt the terminal draws a dock for, `LISTMAX`. Also
# `source`d at the top level, and for the same reason (its `typeset`s).
source ${ZDOTDIR:-${0:A:h}}/dock.zsh || return 1

# The dock's decisions, once per shell (`.zshenv` is read by every zsh).
if (( ! ${+__bateri_dock} )); then
  # WHETHER THERE IS A DOCK IN THIS SESSION. A single variable, because it is a
  # single decision: if there is a dock the input line is the TERMINAL's — the
  # prompt is reset, ZLE is mirrored, the context's branch is printed. If there
  # is no dock all three are meaningless and all three are TURNED OFF; had they
  # been asked separately an inconsistent state like "the prompt is the
  # terminal's but there is no dock" would have been possible — and once was.
  #
  # The environment variable comes only at the dock-less tier
  # (`shell_integration_env`), so its absence means "there is a dock". An
  # unrecognized value also falls there: there is no place to print a
  # diagnostic at this end and falling back to the default is a VISIBLE outcome
  # (the user sees the dock and realizes they mistyped).
  #
  # `unset`: the variable belongs only to US and there is no point in leaking it
  # into child processes (`BATERI_ZDOTDIR` precedent). The shell variable that
  # keeps the value is the hooks' session state, so `__bateri_restore` does NOT
  # delete it (like `__bateri_block`).
  if [[ $BATERI_DOCK == off ]]; then
    __bateri_dock=0
  else
    __bateri_dock=1
  fi
  unset BATERI_DOCK

  # LET AN OVERFLOWING COMPLETION LIST NOT ERASE THE SCREEN: `dock.zsh`'s
  # `__bateri_dock_start` says why and how it was measured. It is called HERE, before any
  # of the user's files is read, and the place itself is a decision: `LISTMAX` is SET by default
  # in zsh, so "did the user set it" cannot be tested; a user who writes their own `LISTMAX`
  # runs LATER and wins.
  #
  # CONDITIONAL ON THE DOCK (the decision above): what it protects is the dock's promise and
  # at the `integration = "blocks"` tier that promise does not exist. It is the third decision
  # `__bateri_dock` consumes; had a separate key been opened an inconsistent state like "no
  # dock but completion is ours" would have arisen.
  if (( __bateri_dock )); then
    __bateri_dock_start
  fi
fi

# THE BINARY THAT DECIDES `ssh`'s WRAPPING: `BATERI_BIN`, the path of the
# running bateri (`app::with_bateri_bin`), is kept in a shell variable and
# taken out of the environment — it is ours, children have no use for it
# (`BATERI_DOCK` precedent). Once per shell, like the decisions above.
if (( ! ${+__bateri_bin} )); then
  __bateri_bin=${BATERI_BIN-}
  unset BATERI_BIN
  # The running bateri's ssh instance directory: `bateri ssh-hold` finds the
  # running bateri's socket there. Same rule: ours, out of the environment.
  __bateri_ssh_instance=${BATERI_SSH_INSTANCE-}
  unset BATERI_SSH_INSTANCE
  # A bateri before this one handed the remote folder of a tab opened by ⌘T
  # from a remote tab down in this variable; nothing reads it any more
  # (the new terminal asks the running bateri for the folder). Out of the
  # environment all the same.
  unset BATERI_SSH_CWD
fi

# Attaches the OSC 133 marks to zsh's own hooks.
#
# Called from `.zshrc`, AFTER the user's file is loaded: `add-zsh-hook` appends,
# so our hook runs behind the user's hooks and sees what they did to PS1.
__bateri_hooks() {
  autoload -Uz add-zsh-hook
  # "Has a command run since the last prompt": `D` is printed only after a command
  # that really ran. An Enter on an empty line spawns a new prompt but there is
  # no command that finished.
  typeset -g __bateri_ran=0
  # Block counter. Every prompt opens a block and its id enters both OSC 133
  # (`bt_block=`) and the prompt's cells (OSC 8); the terminal does not LEARN
  # which line a block started on this way, it READS IT FROM THE GRID on every
  # frame. `__bateri_restore` does not delete it: the loader's traces go, not
  # the hooks' session state.
  #
  # THE FIELD NAME IS OURS, NOT `aid`: in the spec `aid` is the "application id"
  # and usually carries the pid, i.e. it is CONSTANT for the whole session. Had
  # we written our counter there, the constant value of another integration that
  # follows the spec would get mixed with our ledger.
  typeset -g __bateri_block=0
  # ANCHOR: an id-carrying OSC 8 link laid over the prompt's cells. The terminal
  # does not remember which line a block started on, it reads it from the grid
  # on every frame — so it stays correct after line scrolling, window reflow
  # and once the history has filled.
  #
  # THE VALUE IS NOT EMBEDDED, it expands at prompt time with `%9v`: when the
  # part added to PS1 stays CONSTANT the `shell` arm's "is it already there"
  # guard works. Had the id been written into the URI the addition would change
  # on every prompt, the guard could not hold and PS1 would be destructively
  # taken apart and rebuilt every time — exactly the race we avoid with themes
  # that set PS1 themselves. Printing the opening with `print` is NOT a solution
  # either: zsh redraws the prompt without running precmd (SIGWINCH, Ctrl-L,
  # `zle reset-prompt`) and those cells would be written without an anchor.
  #
  # The opening is a PREFIX: OSC 8 does not nest, a new URI replaces the
  # previous one — in the `shell` arm, if a theme uses a link in its own prompt
  # the cells BEFORE it carry our anchor. If the theme's link starts at the
  # first character the anchor is never born and the stripe is not drawn; a
  # known limit, not a wrong drawing.
  typeset -g __bateri_anchor=$'%{\e]8;;bateri://block/%9v\a%}'
  # PS1 when the prompt is the TERMINAL's (`__bateri_ps1`: the anchor, two real spaces, `B`) is
  # built by `dock.zsh`'s `__bateri_dock_setup` from the anchor above.
  add-zsh-hook precmd __bateri_precmd
  add-zsh-hook preexec __bateri_preexec
  # FIRST, not appended: the title a command set goes back before the user's
  # hooks run (`__bateri_title_back` says why).
  precmd_functions=(__bateri_title_back ${precmd_functions:#__bateri_title_back})
  # THE DOCK'S ZLE SIDE (the mirror, the edit widget, the prompt's PS1 and the hooks that
  # carry them) is `dock.zsh`'s, shared with the remote wrapper: `__bateri_dock_setup` sets it
  # up.
  #
  # IF THERE IS NO DOCK IT IS NOT SET UP AT ALL. The mirror's only consumer is
  # the dock; had the hooks been set up at the `blocks` tier, five variables
  # would be base64-encoded and written to the stream on every keystroke with no
  # one reading them. A cost paid per key going unanswered is unacceptable even
  # if unmeasured — especially when the shape of the cost already sits in the
  # debt list.
  if (( __bateri_dock )); then
    __bateri_dock_setup
  fi
  # THE REMOTE INTEGRATION'S `ssh`: the user types plain `ssh`
  # and bateri decides whether the connection gets the remote bootstrap
  # (`__bateri_ssh`). Defined here, AFTER the user's files, so a user's own
  # `ssh` alias or function is seen and wins — we define nothing then (a
  # known limit: theirs bypasses the integration and today's detection
  # stays). Not at the `off` tier (no wrapper at all) and only with the
  # binary's path; at the `blocks` tier it exists, since the remote
  # integration does not depend on the dock.
  if [[ -n $__bateri_bin ]] && (( ! ${+aliases[ssh]} && ! ${+functions[ssh]} )); then
    ssh() { __bateri_ssh "$@" }
  fi
}

# Runs the user's `ssh`: as a terminal kept on the server when bateri says so,
# else plain `command ssh "$@"`.
#
# `bateri ssh-hold -- <args…>` stands in the `ssh`'s place and is the pane's
# program for as long as the terminal is shown: it asks the running bateri for
# the terminal and then only carries bytes (what the terminal's program writes
# to the screen, what is typed to it, the pane's size). Everything it would
# have to decide — which call is interactive, the settings, `ssh -G`, whether the
# server can keep a terminal, the password and the server's key (asked on this
# terminal, by ssh itself) — is bateri's, in the binary; whatever it cannot do,
# before a terminal is shown, it replaces itself with the plain `ssh` with the
# arguments as they were typed, so the worst case is today's `ssh`. Its exit
# status is the terminal's program's (`ssh` reports the remote command's the
# same way). The function does not look at it.
#
# Plain without asking when it could not work: stdin AND stdout must be
# terminals (`ssh host | grep` is the user's own pipe), there is the binary and
# the directory of bateri's sockets (`__bateri_ssh_instance`, which names the
# running bateri the pane was born under: the binary finds the socket there
# and nowhere else), and no local multiplexer (`$TMUX`, `$STY`): the pane sees
# tmux or screen, not the ssh. `BATERI_SSH_INSTANCE` is exported for the one
# command only — the shell unset it at startup so that programs started from
# here do not carry it.
__bateri_ssh() {
  emulate -L zsh
  if [[ -n ${TMUX-} || -n ${STY-} || ! -t 0 || ! -t 1 \
        || ! -x $__bateri_bin || -z $__bateri_ssh_instance ]]; then
    command ssh "$@"
    return
  fi
  BATERI_SSH_INSTANCE=$__bateri_ssh_instance command $__bateri_bin ssh-hold -- "$@"
}

# The window title a command set goes with it: `__bateri_preexec` pushes the
# title onto the terminal's title stack (`CSI 22 t`) as the command starts, and
# this pops it (`CSI 23 t`) once the command is over. A program that names the
# window (vim, htop, an app drawn in the terminal) and never puts the old name
# back would otherwise leave its name on the tab until something wrote another.
# It rides the stream, so the parser sees it in order after the program's last
# bytes; a program that pushed its own and pops it on the way out leaves the
# stack as it found it.
#
# A HOOK OF ITS OWN, FIRST AMONG THE PRECMD HOOKS: ours is appended and runs
# after the user's, and a user hook that names the window at every prompt
# (oh-my-zsh's `user@host:dir`) would be undone by a pop behind it — the tab
# would keep the name from before the command, a `cd` behind. Not printed with
# `D` for the same reason.
#
# So does the alternate screen a program that is over left on — one killed, one
# that crashed, the vim behind an ssh that dropped: the screen it had goes, the
# one it found comes back, and the prompt is drawn there and not on a blank
# full-screen page (where bateri also keeps the dock hidden). In this hook, so a
# user hook that prints (a theme's line above the prompt) prints on that screen.
# The kitty key stack is popped first: each screen has its own, and the program's
# is the one showing. Not for a program that is only stopped (⌃Z, or a STOP,
# TTIN or TTOU): its status is 128 + the signal, and `fg` brings it back to its
# screen. `$signals` holds the names one place above their numbers.
#
# `$?` is handed back: the hooks after this one read the command's status.
__bateri_title_back() {
  local code=$?
  emulate -L zsh
  if (( __bateri_ran )); then
    print -nr -- $'\e[23t'
    if (( code <= 128 )) || [[ ${signals[code - 127]-} != (TSTP|STOP|TTIN|TTOU) ]]; then
      print -nr -- $'\e[<4096u\e[?1049l'
    fi
  fi
  return code
}

# Before the prompt is drawn: the finished command's code (`D`), then the prompt start (`A`).
__bateri_precmd() {
  # MUST be the FIRST line: every following command overwrites `$?` — `emulate`
  # included, so that too is BELOW this.
  local code=$?
  # The hook body runs with the user's options and the `psvar[9]` below is an
  # ARRAY INDEX: with `KSH_ARRAYS` on, the assignment lands in zsh's 10th slot
  # while `%9v` still reads the 9th, so the anchor carries an empty id and the
  # blocks vanish WITHOUT A DIAGNOSTIC (found in review; verified with
  # `zsh -f`). `-L` is function-local, undone on return.
  emulate -L zsh
  # `D` closes the FINISHED block, i.e. its id is the value BEFORE the counter increments.
  if (( __bateri_ran )); then
    __bateri_ran=0
    print -nr -- $'\e]133;D;'$code$';bt_block='$__bateri_block$'\a'
    # The input modes the command left on go with it: mouse reports (SGR and
    # pixel forms too), focus reports, size reports, kitty's key forms. A
    # program that dies without its exit path — a dropped ssh, a killed TUI —
    # never turns them off, and ZLE takes the reports for typed text
    # (`35;12;7M…` on the line at every mouse move, `48;…t` at every resize)
    # and the kitty forms for keys it does not bind.
    # Bracketed paste is the shell's own, so it is not touched; the alternate
    # screen went back already (`__bateri_title_back`).
    print -nr -- $'\e[?1000;1002;1003;1004;1005;1006;1016;2048l\e[<4096u'
  fi
  (( __bateri_block++ ))
  # The slot that carries the id to the prompt; `%9v` reads it in the anchor
  # below. The index appears in two places and they must change together.
  #
  # A HIGH INDEX ON PURPOSE: `psvar` is the user's namespace and customary use is
  # the first few slots. Since our hook is appended LAST with `add-zsh-hook` it
  # runs after the user's precmds — a theme that sets `psvar` wholesale cannot
  # override our slot.
  psvar[9]=$__bateri_block
  print -nr -- $'\e]133;A;bt_block='$__bateri_block$'\a'
  __bateri_prompt_set
  # THE DOCK'S CONTEXT LINE. Per prompt, NOT per key: both wait for a command to
  # change (`cd`, `git checkout`) and when that command finishes we are here.
  # OSC 7 STAYS, THE BRANCH'S FORK DOES NOT. Both feed only the dock's context
  # line today but their costs are not comparable: OSC 7 is a single `print` and
  # a STANDARD sequence (the path for jobs like opening a new tab in the same
  # directory, i.e. it has a future independent of the dock). The branch is a
  # `git` FORK per prompt and its only consumer is the dock — paying for it
  # while there is no dock is pure waste.
  __bateri_cwd
  (( __bateri_dock )) && __bateri_branch_print
}

# Reports the working directory with OSC 7.
#
# THE AUTHORITY PART IS EMPTY (`file:///…`), NOT `file://$HOST…`: the gate on the
# terminal side counts every named host as foreign (`LOCAL_AUTHORITIES`) and the
# reason is not a shortcoming but a dependency decision — comparing names would
# mean `gethostname` in `bt-core`. When printed with an empty authority the gate
# never depends on a name match: the directory stays visible even if the machine
# is renamed.
#
# PERCENT ENCODING IS MANDATORY: `$PWD` can contain spaces, `%`, `;` and
# multi-byte characters; `;` would break the OSC field, control bytes would
# break the sequence.
__bateri_cwd() {
  emulate -L zsh
  local REPLY
  __bateri_percent "$PWD"
  print -nr -- $'\e]7;file://'$REPLY$'\a'
}

# Sends the git branch through the mirror's channel; if it is not a repository
# the body is EMPTY. The environment report rides after it
# (`__bateri_env_fields`).
#
# A SINGLE FORK in the usual case: `--abbrev-ref` is one call even outside a
# repository, a second call on a detached HEAD for the short SHA. The cost is
# per PROMPT and is felt in a large repository — p10k's `gitstatusd` daemon
# exists for this reason; speeding it up is a separate job, out of scope here.
#
# `command`: so that the user's `git` alias or function does not interfere.
__bateri_branch_print() {
  emulate -L zsh
  local REPLY ref
  ref=$(command git rev-parse --abbrev-ref HEAD 2>/dev/null)
  # `HEAD` is not a branch name, it is the answer of a detached HEAD: the short SHA instead.
  if [[ $ref == HEAD ]]; then
    ref=$(command git rev-parse --short HEAD 2>/dev/null)
  fi
  __bateri_b64 "$ref"
  local branch=$REPLY
  __bateri_env_fields
  print -nr -- $'\e]8133;b;'$branch$__bateri_env_wire$'\a'
}

# The environment the dock's context row shows, as five more `8133;b` fields:
# `VIRTUAL_ENV`, `VIRTUAL_ENV_PROMPT`, `CONDA_PREFIX`, `CONDA_PROMPT_MODIFIER`
# and the `node` this shell would run, each base64, empty when unset. The
# terminal reads the files they name (the Python version, whether this is a
# Node project, the Node version) and RUNS NOTHING; this side only says what
# the variables hold.
#
# THE VARIABLES, NOT THE PROMPT: `activate` and conda put their name in front
# of PS1, and the dock's PS1 is ours — rebuilt at every prompt, so their
# prefix is lost there. Reading the variables is what makes the name survive.
#
# ENCODED ONLY WHEN SOMETHING CHANGED: the encoder is a loop per three bytes,
# and the five values change only at an `activate`, a `conda activate` or a
# `nvm use`. `$commands[node]` is zsh's own command table, no fork; a `PATH`
# assignment empties it, so a `nvm use` is seen at the next prompt.
typeset -g __bateri_env_raw= __bateri_env_wire=
__bateri_env_fields() {
  emulate -L zsh
  local REPLY field out= node=${commands[node]-}
  local raw=$VIRTUAL_ENV$'\0'$VIRTUAL_ENV_PROMPT$'\0'$CONDA_PREFIX$'\0'$CONDA_PROMPT_MODIFIER$'\0'$node
  [[ $raw == "$__bateri_env_raw" && -n $__bateri_env_wire ]] && return 0
  for field in "$VIRTUAL_ENV" "$VIRTUAL_ENV_PROMPT" "$CONDA_PREFIX" "$CONDA_PROMPT_MODIFIER" "$node"; do
    __bateri_b64 "$field"
    out+=";$REPLY"
  done
  __bateri_env_raw=$raw
  __bateri_env_wire=$out
}

# Sets up the prompt according to the owner of this session; its caller is `precmd`.
#
# TWO ARMS, the only difference is the OWNER of PS1:
#
# - `terminal` (default): PS1 is entirely OURS and its visible width is zero.
#   The user's prompt is not drawn; the dock's `>` mark takes its place.
#   `RPS1`/`RPROMPT` are emptied too and this is not a detail but MANDATORY: the
#   right prompt lives independently of PS1, resetting only PS1 would leave a
#   theme fragment hanging on the right of the screen.
# - if there is NO dock (`integration = "blocks"`): the user's prompt stays in
#   place, we only ADD the marks. At one time there was a third state, "the
#   prompt is the user's but the dock is open anyway", and it produced TWO
#   PROMPTS on screen; it was reduced to a single decision.
__bateri_prompt_set() {
  if (( __bateri_dock )); then
    __bateri_dock_prompt
    return 0
  fi
  # The guard asks for CONTAINMENT, not position (found in review):
  # the same form as the `B` addition's guard. A prefix test is not idempotent
  # when SOMEONE ELSE touches PS1 — a theme that decorates PS1 on every precmd
  # (virtualenv, git info) does not move our addition to the front, so we would
  # add a new one every round and PS1 would grow without bound over the session.
  #
  # The right-hand side is QUOTED: inside `[[ ]]` an unquoted right operand is a
  # glob pattern.
  [[ $PS1 == *"$__bateri_anchor"* ]] || PS1=$__bateri_anchor$PS1
  # `B` is the END of the prompt, i.e. not a hook but the prompt itself. The
  # reason it is retried on every prompt is themes: a theme that rebuilds PS1 on
  # every precmd deletes our addition. The condition is for that too — so the
  # same addition does not go in twice.
  [[ $PS1 == *$'\e]133;B\a'* ]] || PS1=$PS1$'%{\e]133;B\a%}'
}

# Right before a command runs: the anchor closes, output begins (`C`).
__bateri_preexec() {
  __bateri_ran=1
  # THE ANCHOR CLOSES HERE, NOT AT THE END OF THE PROMPT — and this is the
  # mandatory companion of the zero-width PS1: PS1 no longer writes any cell, so
  # had the closing stayed at the end of PS1 THE CELL CARRYING THE ANCHOR WOULD
  # NEVER BE BORN. Both the block stripe and the suppression of the input line
  # derive from that cell (`Session::frame`), so both would die silently.
  #
  # The link stays OPEN throughout `Input`: every cell ZLE writes carries the id.
  # The command's OUTPUT does not, because the closing is right before the output
  # — the mark is on the command's own line, not its output.
  #
  # UNCONDITIONAL, in both arms: in the `shell` arm too the input line is
  # suppressed (the dock does not close) and it looks at the same anchor.
  #
  # A KNOWN LIMIT: on paths where `preexec` does not run (Ctrl-C, Enter on an
  # empty line) the link stays open until the next prompt's PS1 expansion.
  # Measured: in that window there are only the cells printed by the user's own
  # `precmd` hooks and they carry the PREVIOUS block's id — since our hook is
  # appended with `add-zsh-hook` we run after them, so there is no way to close
  # earlier. The direction is safe: an extra stripe mark gets drawn, suppression
  # is unaffected (that block's id is no longer the block being written).
  print -nr -- $'\e]8;;\a'
  print -nr -- $'\e]133;C\a'
  # The title as the command finds it, for `__bateri_title_back` to put back.
  print -nr -- $'\e[22t'
}

# ZLE's display mirror, the edit widget and the prompt's guard are in `dock.zsh`, shared with the
# remote wrapper; `__bateri_dock_setup` registers them.
