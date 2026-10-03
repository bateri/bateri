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

  # LET AN OVERFLOWING COMPLETION LIST NOT ERASE THE SCREEN. zsh's default
  # criterion is `LISTMAX=100` and it looks at the NUMBER OF OPTIONS, not the
  # SPACE it takes: a list under a hundred is printed without asking, even if
  # the line count exceeds the screen. The symptom was seen by the user
  # (2026-09-21, `ls -` completion): the list was printed without asking,
  # overflowed the grid and STAYED on screen when the line was erased. `0`
  # turns the criterion from a count into a space: "ask if it does not fit on
  # the screen". How many lines how many options take depends on the user's
  # `zstyle` and no number is written here — the criterion is not a number
  # anyway.
  #
  # The rationale was measured (2026-09-21, pure PTY, the same 37-line list at
  # two screen heights): when the list FITS (60 lines) zsh on Tab moves the
  # cursor above the list with `\e[37A` and sends `\e[J` when the line is
  # erased — the fill band fills the gap from the scrollback and the screen
  # returns to its pre-Tab state. When the list OVERFLOWS (26 lines) zsh sends
  # neither; on line erase only a backspace arrives, because in a normal
  # terminal it cannot bring back the lines that scrolled into the history and
  # half-clearing would corrupt the screen. No signal that tells the terminal
  # "the list is over" ever arrives: the scrollback is with us but there is no
  # trigger to bring it back. `LISTMAX=0` stops erasing the screen from being
  # an irreversible step — for an overflowing list zsh asks first and `n`
  # leaves the screen as it is.
  #
  # A KNOWN LIMIT: when `y` is answered the list is printed and is permanent
  # again. The criterion is "ask before it breaks", not "bring it back".
  #
  # THE VALUE IS SET BEFORE THE USER'S FILES and the place itself is a
  # decision: `LISTMAX` is SET by default in zsh (`typeset -i LISTMAX=100`), so
  # "did the user set it" cannot be tested. This block runs once from
  # `.zshenv`, before any of the user's startup files is read; a user who
  # writes their own `LISTMAX` runs LATER and wins. Had the value been put
  # inside `__bateri_hooks` the opposite would happen and the user's preference
  # would be overridden.
  #
  # CONDITIONAL ON THE DOCK, because what it protects is the dock's promise
  # (the return of the screen) and at the `integration = "blocks"` tier
  # that promise does not exist — there the input line is the user's and the
  # shell should behave classically. It is the third decision `__bateri_dock`
  # consumes; had a separate key been opened an inconsistent state like "no
  # dock but completion is ours" would have arisen.
  if (( __bateri_dock )); then
    LISTMAX=0
  fi
fi

# THE BINARY THAT DECIDES `ssh`'s WRAPPING: `BATERI_BIN`, the path of the
# running bateri (`app::with_bateri_bin`), is kept in a shell variable and
# taken out of the environment — it is ours, children have no use for it
# (`BATERI_DOCK` precedent). Once per shell, like the decisions above.
if (( ! ${+__bateri_bin} )); then
  __bateri_bin=${BATERI_BIN-}
  unset BATERI_BIN
  # The running bateri's ssh instance directory: a wrapped
  # session becomes a master there. Same rule: ours, out of the environment.
  __bateri_ssh_instance=${BATERI_SSH_INSTANCE-}
  unset BATERI_SSH_INSTANCE
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
  # PS1 when the prompt is the TERMINAL's: two zero-width marks and **two real
  # spaces**. The terminal itself draws `>` in the dock and in the grid, but in
  # the grid a place is needed to put it — that place is these two columns.
  #
  # THE SPACES ARE NOT A DRAWING TRICK, THEY ARE REAL WIDTH. The alternative was
  # to shift the command line two columns to the right while drawing it, and it
  # would have broken three things at once: the mouse mapping would shift on that
  # line, the last two characters of a full-width command would overflow the
  # screen and zsh would miscalculate line wrapping. If the prompt really is two
  # columns all three are right on their own — zsh already knows the prompt
  # width.
  #
  # THE NUMBER MUST BE THE SAME AS `bt-core`'s `dock::TEXT_COL`: the dock's text
  # also starts two columns after the mark and if the two diverge the grid and
  # the dock start from different columns. The constant cannot be shared (one is
  # zsh, one is Rust), so a test reads this line and ties the number.
  #
  # ORDER: anchor → spaces → `B`. `B` is the END of the prompt, i.e. where the
  # input starts; the spaces must come before it. The anchor is first, because
  # the spaces also carry the link it opens — and that is a side gain: even an
  # empty prompt has an anchored cell, whereas a zero-width PS1 had none.
  #
  # `%{…%}` means "zero width"; the spaces are deliberately OUTSIDE, they must be
  # counted.
  typeset -g __bateri_ps1=$__bateri_anchor'  '$'%{\e]133;B\a%}'
  add-zsh-hook precmd __bateri_precmd
  add-zsh-hook preexec __bateri_preexec
  # MIRROR: ZLE's display state goes to the terminal on every line draw.
  #
  # NOT `zle -N zle-line-pre-redraw`: that binding has a single owner and
  # zsh-syntax-highlighting and zsh-autosuggestions both want the same widget —
  # the last writer would drop the other (measured). Instead of
  # `add-zle-hook-widget` it sets up a dispatcher and calls them all in order.
  #
  # THE GUARD IS IN THE HOOK ITSELF: `add-zle-hook-widget` does not add the same
  # widget twice (it asks the `zstyle` list for containment), so the counterpart
  # of the guard we wrote by hand for PS1's additions is ready here. Verified:
  # registered twice, the `add-zle-hook-widget -L line-pre-redraw` list is a
  # single line.
  #
  # A PLUGIN THAT REGISTERS AFTER US: `add-zle-hook-widget` appends, so our hook
  # runs AFTER the user's plugins and sees their `region_highlight`/`POSTDISPLAY`
  # contribution. This has two known limits and both are symptomless: (1) a
  # plugin that defers its registration (zsh-defer) comes after us and its
  # contribution reaches the mirror one draw LATE; (2) a plugin that says
  # `zle -N zle-line-<hook>` overrides the dispatcher itself and everything tied
  # to that hook — ours included — dies silently. Whichever of the three hooks
  # is overridden, that arm goes quiet and the symptoms differ: if
  # `line-pre-redraw` goes the dock freezes, if `line-init` goes the prompt
  # stays empty for a moment, if `line-finish` goes a finished command hangs on
  # in the mirror. `line-init` is the most likely — common in users' rc files
  # for the cursor shape.
  #
  # `line-init` IS ALSO HOOKED and this is not decoration: `line-pre-redraw` runs
  # only when the line CHANGES, not on the prompt's first (empty) draw — observed
  # in a real session, the first mirror came only at the first keystroke.
  # Without it the dock would stay dead at prompt time and would appear suddenly
  # at the first letter.
  #
  # IF THERE IS NO DOCK IT IS NOT SET UP AT ALL. The mirror's only consumer is
  # the dock; had the hooks been set up at the `blocks` tier, five variables
  # would be base64-encoded and written to the stream on every keystroke with no
  # one reading them. A cost paid per key going unanswered is unacceptable even
  # if unmeasured — especially when the shape of the cost already sits in the
  # debt list.
  if (( __bateri_dock )); then
    autoload -Uz add-zle-hook-widget
    # THE EDIT WIDGET: the terminal's only command goes here. The definition
    # once, the binding on every `line-init` (`__bateri_dock_arm`) — and arm is
    # registered BEFORE the mirror, so the capability is on the wire before the
    # first mirror of the same prompt.
    zle -N __bateri_dock_edit
    add-zle-hook-widget line-init __bateri_dock_arm
    add-zle-hook-widget line-init __bateri_dock_redraw
    add-zle-hook-widget line-pre-redraw __bateri_dock_redraw
    add-zle-hook-widget line-finish __bateri_dock_finish
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

# Runs the user's `ssh`, wrapped when bateri says so.
#
# `bateri ssh-argv [--tty] --block N [--instance I] -- <args…>` prints the wrapped arguments, each
# followed by a NUL, or nothing — and nothing (also a missing binary or any
# failure) means plain `command ssh "$@"`, today's path. `--tty` only when stdin
# AND stdout are terminals: under `$(…)` the binary's own stdout is our pipe, so
# it cannot ask itself (`ssh host | grep` is not wrapped). The rules —
# which call is interactive, the settings, `ssh -G`, whether the server is
# known to have no shell — are all in the binary (`ssh_wrap::decide`), not
# here: one parser.
# `--block` is this command's block (`__bateri_block`): the server's blocks are
# marked as its children (`bt_remote=<P>.<S>.<n>`). `--instance`
# is bateri's socket directory (`__bateri_ssh_instance`): the session becomes a
# master there and bateri's file jobs ride it.
#
# THE SILENT FALLBACK: every wrapped `ssh` that ended asks `bateri
# ssh-fell-back --rc N [--instance I] -- <the wrapped arguments>` — 255 too:
# ssh's own error (a password, the network, a host key) is the binary's to
# tell from an endpoint that refused our command after the login (the pane's
# login proof). The binary takes the attempt's nonce out
# of them and answers nothing when the bootstrap said `up` or the user typed
# after the login — the user's session ran and its code is theirs, so an
# `exit` is never followed by a new connection —, or the plain rerun's
# arguments when it did not (a router, Windows: our command never ran); the
# rerun shares the wrapped call's `--instance`, so it
# rides the wrapped session's master while it lingers. The function returns
# the code of the last `ssh` it ran.
#
# INSIDE A LOCAL MULTIPLEXER NOTHING IS WRAPPED (`$TMUX`, `$STY`): the pane
# sees tmux or screen, not the ssh, and the bootstrap's `up` does not reach it
# — a wrapped session there could never prove it ran, and a fallback would
# reconnect the user after their `exit`.
#
# `$(…)` keeps NUL bytes in zsh and `"${(@0)…}"` splits on them keeping empty
# arguments; the last NUL leaves one empty element behind, which is dropped.
# An answer that does not end with a NUL is not ours: plain `ssh`.
__bateri_ssh() {
  emulate -L zsh
  local out tty= rc
  local -a instance
  if [[ -n ${TMUX-} || -n ${STY-} ]]; then
    command ssh "$@"
    return
  fi
  [[ -t 0 && -t 1 ]] && tty=--tty
  [[ -n $__bateri_ssh_instance ]] && instance=( --instance "$__bateri_ssh_instance" )
  if [[ -x $__bateri_bin ]]; then
    out=$(command $__bateri_bin ssh-argv $tty --block "$__bateri_block" "${instance[@]}" -- "$@" 2>/dev/null)
  fi
  if [[ -n $out && $out == *$'\0' ]]; then
    local -a wrapped
    wrapped=( "${(@0)out}" )
    wrapped[-1]=()
    command ssh "${wrapped[@]}"
    rc=$?
    out=$(command $__bateri_bin ssh-fell-back --rc $rc "${instance[@]}" -- "${wrapped[@]}" 2>/dev/null)
    if [[ -n $out && $out == *$'\0' ]]; then
      local -a plain
      plain=( "${(@0)out}" )
      plain[-1]=()
      command ssh "${plain[@]}"
      return
    fi
    return rc
  else
    command ssh "$@"
  fi
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
# the body is EMPTY.
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
  print -nr -- $'\e]8133;b;'$REPLY$'\a'
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
    PS1=$__bateri_ps1
    RPS1=
    RPROMPT=
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

# Takes back the prompt the theme wrote back; its caller is the mirror's ZLE hook.
#
# WHY PRECMD IS NOT ENOUGH: p10k and starship rebuild PS1 AFTER `precmd`, from
# their own ZLE hooks, and `zle reset-prompt` it — the value we wrote in precmd
# is erased from the screen. If it is not imposed from the same place the theme
# wins.
#
# WHY IT IS NOT ENOUGH ALONE EITHER — MEASURED, not chosen: a PS1 assigned from
# a ZLE hook does NOTHING by itself. The prompt is printed before `line-init`
# runs and zsh keeps the expanded form; in a `zsh -i` PTY probe the value
# assigned from the hook never appeared on screen. The only way for it to take
# effect is `zle reset-prompt`. So the two are needed TOGETHER: precmd gets the
# first print right, this hook takes back what the theme wrote back.
#
# TWO COSTS OF DROPPING PRECMD WERE MEASURED and both justify precmd:
# (1) with a theme that sets it only from `precmd` (starship) the prompt is
# already right on first print and the guard never resets — without precmd EVERY
# prompt would pay a `reset-prompt` redraw; (2) if a plugin that says
# `zle -N zle-line-init` overrides the dispatcher (the limit this file counts as
# most likely) this hook goes completely silent and the prompt would come back.
# The reverse is also true: had only precmd remained, p10k would write the
# prompt back.
#
# THE GUARD CUTS THE PING-PONG: `reset-prompt` spawns a new draw and that draw
# calls `line-pre-redraw` again. An unconditional reset would be a self-feeding
# loop; with the guard a SINGLE reset per prompt was measured.
#
# IT GOES SILENT IN THE DOCK-LESS ARM: there is no point fighting the theme of a
# user who wants their prompt back.
__bateri_prompt_guard() {
  (( __bateri_dock )) || return 0
  [[ $PS1 == "$__bateri_ps1" && -z $RPS1 && -z $RPROMPT ]] && return 0
  PS1=$__bateri_ps1
  RPS1=
  RPROMPT=
  zle reset-prompt
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
}

# ── ZLE's display mirror ─────────────────────────────────────────────────
#
# WIRE FORMAT (its decoder is `bt-core`'s `parse_dock`; the two change together):
#
#   ESC ] 8133 ; u ; CURSOR ; b64(PREDISPLAY) ; b64(BUFFER) ;
#                             b64(POSTDISPLAY) ; b64(region_highlight) ;
#                             b64(KEYMAP) ; b64(PREBUFFER) BEL
#   ESC ] 8133 ; e BEL   line finished (`line-finish`)
#   ESC ] 8133 ; o BEL   the display does not fit the mirror (the gate below)
#   ESC ] 8133 ; b ; b64(branch) BEL   the context line's branch (`precmd`)
#   ESC ] 8133 ; w BEL   the edit widget is bound in this prompt (`line-init`)
#
# REVERSE DIRECTION — FROM THE TERMINAL TO THE SHELL, the wire's two sequences:
#
#   ESC [ 8133 ~ d ; S ; E ; L BEL
#   ESC [ 8133 ~ r BEL   only mirror: behind a paste with line breaks;
#                        since `bracketed-paste-magic` pushes the payload onto the
#                        queue with `zle -U` and skips the redisplay, the mirror
#                        stayed stale for a key. The widget's behavior on every
#                        payload that is not `d` is already this: it prints the
#                        mirror without touching `BUFFER`.
#
# Delete the `[S, E)` character range of `BUFFER`, put the caret at `S`; `S == E`
# only moves the caret. `L` is the `${#BUFFER}` the terminal saw: if it does not
# match the terminal was looking at a stale mirror and the widget does NOTHING.
# The sequence comes from the same PTY as the user's input and ZLE reads it like
# a key; `CSI 8133 ~` is a key no keyboard produces, its number is the mirror's
# number. The text NEVER enters the wire: the letter typed in place of the
# selection comes after `d` by the usual path, i.e. there is no base64 decoder
# in the shell and the letter still passes through `self-insert`. The terminal
# sends the sequence only if it has seen `w` in this prompt — in a shell without
# the binding the trailing BEL would be `send-break` (measured).
#
# KEYMAP IS THE SIXTH BODY and what it carries is not a POLICY but ZLE's state:
# the side that decides which keymaps mean "a typed key turns into text" is the
# terminal (`bt-core`, `insert_keymap`). We send the name as is, because the
# user can create their own keymap with `bindkey -N` and there is no information
# at this end to classify it. base64, because that name may contain `;`.
#
# PREBUFFER IS THE SEVENTH BODY: the earlier lines of a multi-line command
# that ZLE no longer edits (`for`, heredoc, `\`-continuation). Appended at the
# end, because the wire only grows at the end — the decoder reads it as optional,
# so a window running with an old script is decoded too. It is part of the
# display, i.e. it enters the overflow gate's total.
#
# THE BODIES are base64: the text the user typed can contain `;`, `ESC` and C0
# bytes and all three break the sequence's framing. None of the three is in
# base64's alphabet.

# The base64 alphabet, in index order.
typeset -ga __bateri_b64_table
__bateri_b64_table=( {A..Z} {a..z} {0..9} + / )

# The longest display the mirror will carry, in CHARACTERS.
#
# The number was derived, not chosen — and it is the other end of the SAME
# budget as the terminal side's `DOCK_PAYLOAD_LIMIT` (64 KiB): that limit came
# out of the arithmetic "4096 characters × worst-case 4 bytes of UTF-8 × base64's
# 4/3 inflation", this is its form in characters.
#
# WHY THIS END ALSO HAS A GATE: encoding is pure zsh and its cost is linear in
# the input length — and it is paid on EVERY KEYSTROKE. Without the gate a pasted
# block would be spent on encoding a payload the terminal will reject anyway,
# i.e. we would pay the cost and get nothing back. On overflow the mirror says "I
# cannot show it" and the input line stays in the grid; the user still sees what
# they typed.
typeset -g __bateri_dock_limit=4096

# Converts `$1` to base64; the result is in `REPLY`.
#
# NO FORK: encoding runs per keystroke and spawning a `base64` process would be
# the most expensive item of this path. `nomultibyte` makes every element a
# BYTE — base64 is an encoding of bytes, not of characters.
#
# THE BYTE VALUE IS TAKEN INTO A SCALAR FIRST (`x=$bytes[i]`, then `#x`), NOT
# directly with `##${bytes[i]}`: the arithmetic `##` form interprets escape
# sequences and the backslash byte (`\`) was read as 32 instead of 92 — a silent
# corruption for a byte that is common on command lines.
#
# PADDING IS NOT PRINTED: the decoder reads padded and unpadded bodies alike
# (`decode_base64`'s doc) and not printing saves a few bytes per keystroke.
__bateri_b64() {
  emulate -L zsh
  setopt nomultibyte
  REPLY=
  [[ -n $1 ]] || return 0
  local -a bytes
  bytes=( ${(s::)1} )
  local -i n=$#bytes i v rest
  local out= x y z
  for (( i = 1; i <= n; i += 3 )); do
    rest=$(( n - i + 1 ))
    x=$bytes[i]
    v=$(( #x << 16 ))
    if (( rest > 1 )); then
      y=$bytes[i+1]
      v=$(( v | (#y << 8) ))
    fi
    if (( rest > 2 )); then
      z=$bytes[i+2]
      v=$(( v | #z ))
    fi
    out+=${__bateri_b64_table[$(( (v >> 18 & 63) + 1 ))]}
    out+=${__bateri_b64_table[$(( (v >> 12 & 63) + 1 ))]}
    (( rest > 1 )) && out+=${__bateri_b64_table[$(( (v >> 6 & 63) + 1 ))]}
    (( rest > 2 )) && out+=${__bateri_b64_table[$(( (v & 63) + 1 ))]}
  done
  REPLY=$out
}

# `__bateri_percent` (OSC 7's percent encoding) is in `zdotdir.zsh`, shared
# with the remote wrapper.

# Prints ZLE's display state to the mirror; its hooks are `line-init` and
# `line-pre-redraw` (the first is the prompt's first draw, the second every change).
#
# FIVE VARIABLES, if one were missing the mirror would show less than the user
# sees: `POSTDISPLAY` is autosuggestions' suggestion, `region_highlight` is
# syntax highlighting's color.
#
# `emulate -L zsh` IS MANDATORY: the body runs with the user's options and what
# follows depends both on array indexing (`KSH_ARRAYS`) and on the multibyte
# `${#...}` count. `-L` is function-local, undone on return.
#
# `REPLY` IS LOCAL: it is a variable that lives in the user's namespace and our
# hook runs in the middle of their line editing.
__bateri_dock_redraw() {
  emulate -L zsh
  # IMPOSING THE PROMPT, BEFORE the mirror and from the same hook: the rationale
  # is in `__bateri_prompt_guard`'s header. Above the mirror's own payload gate,
  # because the prompt's ownership does not depend on the payload's length — had
  # the theme's prompt come back while the mirror stays silent on an overflowing
  # line the symptom would be unexplainable too.
  __bateri_prompt_guard
  # Records are separated by line breaks; the decoder reads the body with `lines()`.
  # The join is BEFORE the gate, because the fourth body is also subject to the gate.
  local REPLY entries=${(F)region_highlight} pre buf post highlights keymap prebuf
  # The gate is BEFORE ENCODING, because its whole meaning is avoiding encoding —
  # and it measures all FIVE bodies at once (`PREBUFFER` is in the total: it
  # is part of the display and the earlier lines of a pasted loop can
  # exceed the limit by themselves). `region_highlight` is counted separately, it
  # does not enter the total: syntax highlighting leaves one record per token, so
  # on a long line it is of the same order as the text itself and can exceed the
  # limit **on its own** (`DOCK_PAYLOAD_LIMIT`'s derivation also counts it as a
  # separate term beside the text).
  if (( ${#PREBUFFER} + ${#PREDISPLAY} + ${#BUFFER} + ${#POSTDISPLAY} > __bateri_dock_limit
        || ${#entries} > __bateri_dock_limit )); then
    print -nr -- $'\e]8133;o\a'
    return 0
  fi
  __bateri_b64 "$PREDISPLAY"; pre=$REPLY
  __bateri_b64 "$BUFFER"; buf=$REPLY
  __bateri_b64 "$POSTDISPLAY"; post=$REPLY
  __bateri_b64 "$entries"; highlights=$REPLY
  __bateri_b64 "$PREBUFFER"; prebuf=$REPLY
  # KEYMAP is counted OUTSIDE the gate: the longest keymap name is a handful of
  # bytes and adding it to the payload budget would add a second reason for the
  # mirror to go silent on a line that overflows the limit.
  __bateri_b64 "$KEYMAP"; keymap=$REPLY
  # `$CURSOR` IS A CHARACTER offset and the wire wants characters too — the form
  # counted from the start of `BUFFER` goes as is, the shift by `PREDISPLAY` is
  # made the other side of the boundary's job (`DockState::cursor`'s doc).
  print -nr -- $'\e]8133;u;'$CURSOR';'$pre';'$buf';'$post';'$highlights';'$keymap';'$prebuf$'\a'
}

# `line-finish`: ZLE let go of the line, the mirror closes.
#
# WITHOUT IT the last `BUFFER` would hang on: after Enter the dock would keep
# showing the running command's line.
__bateri_dock_finish() {
  emulate -L zsh
  print -nr -- $'\e]8133;e\a'
}

# The upper limit of waiting for the edit command's payload, in SECONDS.
#
# The terminal sends the sequence in a single write, so by the time the widget
# runs the payload is already waiting to be read and no time is spent. The limit
# only determines, on a CORRUPT wire — a sequence whose BEL never arrives — how
# long ZLE will freeze: a moment the user will notice but that will not lock the
# shell. Not a measured number, a feel threshold (precedent `HANDOVER_HOLD`).
typeset -g __bateri_dock_edit_wait=0.5

# The terminal's edit command (`CSI 8133 ~ d;S;E;L BEL`, wire header above).
#
# SILENT IN EVERY CASE: a corrupt payload, an `L` that does not match or an
# out-of-range number returns without touching `BUFFER`. The wrong direction is
# "no edit happened" — let the user's key go to waste rather than corrupt the
# line.
#
# `S == E` DOES NOT WRITE TO BUFFER, only `CURSOR`: even an empty assignment
# would spawn an undo record. Deletion is ZLE's single undo unit (measured).
#
# THE MIRROR IS EXPLICITLY PRINTED AT THE END OF THE WIDGET: `line-pre-redraw`
# runs only when the display changes and a command that puts the caret where it
# already was (or does nothing because `L` did not match) would never spawn a
# mirror. The terminal expects an answer to every input (`DockState::answers`); a
# command left unanswered would keep the edit gate closed until the next key.
__bateri_dock_edit() {
  emulate -L zsh
  local payload= ch=
  while read -k 1 -t $__bateri_dock_edit_wait ch; do
    [[ $ch == $'\a' ]] && break
    payload+=$ch
    # A legitimate payload is four numbers: reading without limit would swallow the line on a corrupt wire.
    (( ${#payload} > 64 )) && break
  done
  if [[ $ch == $'\a' && $payload == d\;<->\;<->\;<-> ]]; then
    local -a field
    field=( ${(s:;:)payload} )
    local -i start=$field[2] end=$field[3] len=$field[4]
    if (( len == ${#BUFFER} && start <= end && end <= len )); then
      if (( start < end )); then
        BUFFER=${BUFFER[1,start]}${BUFFER[end+1,-1]}
      fi
      CURSOR=$start
    fi
  fi
  __bateri_dock_redraw
}

# `line-init`: bind the widget and report the capability.
#
# ON EVERY PROMPT AGAIN, because the binding is not persistent: `bindkey -v`/`-e`
# binds a new keymap to `main`, a deferred plugin can reset the keymap, and the
# keymap of a user who says `bindkey -A mymap main` is born after our setup.
# All three are repaired at the next prompt. The cost is three builtins — no
# fork.
#
# `main` INCLUDED: it is common for users to bind their own keymap to `main` and
# binding to `emacs`/`viins` would not cover it. The terminal sends the sequence
# only in the insert keymap (`INSERT_KEYMAPS`), so `vicmd` is deliberately out.
#
# `w` AFTER THE BINDING: the capability means "bound right now". The terminal
# forgets it at `line-finish` (`e`), so in a session whose script is old or
# whose hook was overridden the gate stays closed and the sequence is never sent.
__bateri_dock_arm() {
  emulate -L zsh
  local map
  for map in main emacs viins; do
    bindkey -M $map $'\e[8133~' __bateri_dock_edit
  done
  print -nr -- $'\e]8133;w\a'
}
