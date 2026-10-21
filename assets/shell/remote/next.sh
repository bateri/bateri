# bateri's terminal counter — POSIX sh, run on the SERVER.
#
# Prints the number of the next terminal, digits only, and never prints one
# twice: the terminals an agent opens are `t1`, `t2`, … per server, and a
# number that was given is not given again, whatever became of its terminal.
# Two callers share it — `bin/bateri` runs this file, and the Mac runs the
# same bytes over its connection (`sh -c`) for the terminal it opens itself —
# so the file reads nothing but `HOME` and is the whole of the rule.
#
# The state is `~/.local/share/bateri/next` (the number to give next), under a
# lock that is a directory (`mkdir` is the one atomic step every `sh` has). It
# is not `XDG_DATA_HOME`: a cron job and the Mac's connection do not have the
# environment a login shell had, and a counter that two environments place in
# two directories would give the same number twice.
#
# The number is written back BEFORE it is printed, so a caller that dies in
# between loses a number, which is allowed, and never repeats one.
#
# A lock whose holder died is taken over after about five seconds: the holder
# only reads and renames a file, so it is never held that long for a living
# reason. The taking over is a rename (one contender wins it).
#
# Exits 1, printing nothing, when it cannot count: no home, a directory it
# cannot write, a counter that is not a number (it is never guessed at — a
# guess is how a number is given twice).

case ${HOME-} in
  /*) ;;
  *) exit 1 ;;
esac
bt_state=$HOME/.local/share/bateri
(umask 077 && mkdir -p "$bt_state") 2>/dev/null || exit 1
bt_lock=$bt_state/next.lock

bt_tries=0
until (umask 077 && mkdir "$bt_lock") 2>/dev/null; do
  bt_tries=$((bt_tries + 1))
  if [ "$bt_tries" -ge 50 ]; then
    if mv "$bt_lock" "$bt_lock.dead.$$" 2>/dev/null; then
      rm -rf "$bt_lock.dead.$$"
    fi
    bt_tries=0
  fi
  sleep 0.1 2>/dev/null || sleep 1
done
trap 'rmdir "$bt_lock" 2>/dev/null' EXIT
trap 'exit 1' HUP INT TERM

bt_n=1
if [ -e "$bt_state/next" ]; then
  bt_n=
  read -r bt_n < "$bt_state/next" 2>/dev/null
  case $bt_n in
    '' | *[!0-9]* | 0*) exit 1 ;;
  esac
fi

bt_next=$((bt_n + 1))
if ! (umask 077 && printf '%s\n' "$bt_next" > "$bt_state/next.$$") 2>/dev/null; then
  rm -f "$bt_state/next.$$"
  exit 1
fi
if ! mv -f "$bt_state/next.$$" "$bt_state/next"; then
  rm -f "$bt_state/next.$$"
  exit 1
fi
printf '%s\n' "$bt_n"
