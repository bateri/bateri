#!/bin/bash
# Runs the checks a change needs, chosen by the files it touched, and stamps what passed them.
#
#   gate.sh commit      the fast tier (`make check`) every time, and the heavy tests of the
#                       areas the change touched: a module of bt-shell-common names its own
#                       tests (`kablo/…` → `kablo::`), the shell scripts name the shells', the
#                       agent's and the remote scripts' tests, the program view's files its
#                       test; a manifest, the Makefile or this script names every heavy test.
#   gate.sh push        everything once, on a clean tree: the fast tier, every heavy test and
#                       the program view (`make test-all`), `make linux` when a crate that
#                       builds on Linux changed, `make bundle` when what goes in the package
#                       changed. The pre-push hook (`tools/pre-push`) asks for its stamp.
#   gate.sh check-push  whether HEAD passed the push tier (the release gate's question).
#
# A stamp names the content that passed — the working tree's files as a git tree, the
# uncommitted and the untracked ones included, the ignored and excluded ones not — so
# committing what passed keeps its stamp, and the same content is never checked twice. The
# files a run looks at are those that differ from the last content that passed; with no
# stamp, from the upstream's. A tier passes with nothing run when the content differs from
# its last green one only in files no build or test reads (`UNREAD`: the README and the
# changelog; `docs/SETTINGS.md` is read by tests and is not one).
#
# Each step's output goes to a log under the git directory; the screen gets one line a step.
set -u
tier=${1:?usage: gate.sh commit|push|check-push}
cd "$(git rev-parse --show-toplevel)" || exit 1
git_dir=$(git rev-parse --git-dir)
stamp=$git_dir/bateri-gate
logs=$git_dir/bateri-gate-logs
UNREAD="README.md CHANGELOG.md"

# The working tree as a tree object, through a copy of the index: the real one is not touched.
index=$(mktemp)
cp "$git_dir/index" "$index" 2>/dev/null
GIT_INDEX_FILE=$index git add -A >/dev/null 2>&1
tree=$(GIT_INDEX_FILE=$index git write-tree)
rm -f "$index"

stamped() { [ -f "$stamp" ] && grep -qx "$1 $tree" "$stamp"; }
last_of() { [ -f "$stamp" ] && sed -n "s/^$1 \\([0-9a-f]*\\)\$/\\1/p" "$stamp" | tail -n 1; }
write_stamp() {
    local t
    for t in "$@"; do
        { [ -f "$stamp" ] && grep -v "^$t " "$stamp"; echo "$t $tree"; } >"$stamp.new" &&
            mv "$stamp.new" "$stamp"
    done
}
only_unread() {
    local f
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        case " $UNREAD " in *" $f "*) ;; *) return 1 ;; esac
    done <<<"$1"
}

if [ "$tier" = check-push ]; then
    [ -z "$(git status --porcelain)" ] && stamped push && exit 0
    echo "gate: $(git rev-parse --short HEAD) has not passed \`make gate-push\` on a clean tree"
    exit 1
fi
case $tier in commit | push) ;; *) echo "usage: gate.sh commit|push|check-push" >&2; exit 64 ;; esac

if stamped "$tier" || { [ "$tier" = commit ] && stamped push; }; then
    echo "gate ($tier): this content already passed; nothing runs again"
    exit 0
fi
if [ "$tier" = push ] && [ -n "$(git status --porcelain)" ]; then
    echo "gate (push): the tree is not clean; commit first — a push sends commits"
    exit 1
fi

# The last green content of this tier: every green run moves the commit tier's, a push run
# only its own.
if [ "$tier" = commit ]; then last=$(last_of base); else last=$(last_of push); fi
if [ -n "$last" ] && only_unread "$(git diff --name-only "$last" "$tree")"; then
    echo "gate ($tier): only files no build or test reads changed ($UNREAD); nothing runs"
    if [ "$tier" = push ]; then write_stamp push commit base; else write_stamp commit base; fi
    exit 0
fi
upstream=$(git rev-parse -q --verify '@{u}' || git rev-parse -q --verify origin/main)
base=$(git rev-parse "$(git merge-base HEAD "$upstream")^{tree}")
[ "$tier" = commit ] && [ -n "$last" ] && base=$last
files=$(git diff --name-only "$base" "$tree")

areas=""
all_heavy=0 view=0 linux=0 bundle=0 smoke=0 race=0
while IFS= read -r f; do
    [ -n "$f" ] || continue
    case $f in
    Cargo.toml | Cargo.lock | */Cargo.toml | Makefile | tools/gate.sh) all_heavy=1 ;;
    crates/bt-shell-common/src/lib.rs) all_heavy=1 ;;
    crates/bt-shell-common/src/*)
        m=${f#crates/bt-shell-common/src/}
        m=${m%%/*}
        areas="$areas ${m%.rs}::"
        ;;
    assets/shell/*) areas="$areas child:: ssh_wrap:: agent::" ;;
    esac
    case $f in
    crates/bt-shell-macos/src/pencere_view* | crates/bt-core/src/pencere* | \
        crates/bt-shell-common/src/view_* | crates/bt-shell-macos/tests/pencere.rs) view=1 ;;
    esac
    case $f in
    crates/bt-core/* | crates/bt-atlas/* | crates/bt-gpu/* | crates/bt-shell-common/* | \
        Cargo.toml | Cargo.lock | tools/linux/*) linux=1 ;;
    esac
    case $f in assets/bundle/* | assets/shell/* | crates/bateri/*) bundle=1 ;; esac
    case $f in crates/bt-shell-macos/src/* | crates/bt-gpu/src/*) smoke=1 ;; esac
    case $f in crates/bt-core/src/reader* | crates/bt-core/src/session* | crates/bt-gpu/src/link*) race=1 ;; esac
done <<<"$files"
areas=$(printf '%s\n' $areas | sort -u | tr '\n' ' ')
[ "$all_heavy" = 1 ] && view=1

count=$(printf '%s\n' "$files" | grep -c .)
echo "gate ($tier): $count files differ from the last content that passed"

mkdir -p "$logs"
step() {
    local name=$1
    shift
    local start=$SECONDS out=$logs/$name.log
    if "$@" >"$out" 2>&1; then
        echo "  $name: ok ($((SECONDS - start)) s)"
    elif grep -q SKIPPED "$out"; then
        echo "  $name: SKIPPED — $(grep -m1 SKIPPED "$out")"
    else
        echo "  $name: FAILED ($((SECONDS - start)) s) — $out"
        tail -n 20 "$out" | sed 's/^/    /'
        exit 1
    fi
}

if [ "$tier" = commit ]; then
    step check make check
    if [ "$all_heavy" = 1 ]; then
        step heavy make test-heavy
    elif [ -n "${areas// /}" ]; then
        echo "  heavy tests of:${areas% }"
        step heavy make test-heavy AREA="$areas"
    fi
    [ "$view" = 1 ] && step view make test-view
    [ "$linux" = 1 ] && echo "  linux: a crate that builds on Linux changed — \`make gate-push\` runs it"
    [ "$bundle" = 1 ] && echo "  bundle: what goes in the package changed — \`make gate-push\` builds it"
else
    if stamped commit; then echo "  check: this content passed it already"; else step check make check; fi
    step all make test-all
    [ "$linux" = 1 ] && step linux make linux
    [ "$bundle" = 1 ] && step bundle make bundle
fi
[ "$smoke" = 1 ] && echo "  smoke: window code changed — \`make smoke\` if what a window does changed"
[ "$race" = 1 ] && echo "  race: the reader or the frame path changed — \`make test-race\` if shared state did"

if [ "$tier" = push ]; then write_stamp push commit base; else write_stamp commit base; fi
echo "gate ($tier): green — stamped $(git rev-parse --short HEAD)'s content"
