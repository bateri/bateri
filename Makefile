CARGO ?= cargo

# Prerequisite order is only guaranteed under serial make; under -j the promises
# "cheapest gate first" and "version first" break.
.NOTPARALLEL:
.PHONY: check prune fmt audit clippy test shader smoke terminfo test-race scan bundle package install release publish ship release-gate sparkle dmgbuild linux

# Definition of done. Homebrew rustc is not pinned (there is deliberately no
# rust-toolchain.toml): the version is printed first so a clippy failure that
# arrives after a `brew upgrade` can be told apart from a code failure.
check:
	@rustc --version
	@$(MAKE) --no-print-directory prune fmt audit clippy test

# Debug builds leave each codegen unit's object file in target/debug/deps
# (macOS's unpacked debug info) and cargo never removes a stale one; they piled
# up by the hundred thousand, and every test binary that touches
# CoreFoundation lists its own directory when it starts (`CFBundleGetMainBundle`)
# — that listing became most of the test step (seen with `sample`).
# Objects older than two days belong to builds that have since been replaced;
# losing one only drops the debugger's line info for a crate not rebuilt since.
prune:
	@if [ -d target/debug/deps ]; then find target/debug/deps -name '*.o' -mtime +2 -delete; fi

fmt:
	$(CARGO) fmt --all -- --check

# The mechanical half of the project rules (moved out of `/audit`): agentless
# and done in seconds, on every `make check`. The lenses that need judgment stay
# in `/audit` and run once at the end of a set (`.claude/is-akisi/proje.md`).
# - Layering: `cargo tree` sees the contract in Cargo.toml, grep sees calls
#   leaking into the source (a use behind `cfg` may not show up in the tree).
#   The source grep drops comment lines: bt-core's header comment says "no
#   objc2". In bt-atlas `objc2-core-*` is allowed, only the `objc2` core is
#   forbidden.
# - bt-atlas's font system sits behind a trait (042): the CoreText/CoreGraphics
#   names (`objc2_core_*`, `CT…`/`CG…`/`CF…`) appear only in the macOS backend
#   (`coretext.rs`); the rules half stays platformless and the Linux backend
#   gets the same calls from the trait.
# - Panic: in bt-core's non-test code there is no unwrap/expect/panic!/
#   unreachable!; the deliberate one carries `// audit: {reason}` on its line.
#   The scan stops at the first `#[cfg(test)]` at line start, because the
#   test module is at the end of the file.
# - Shell: no line under `assets/shell/` writes to the user's rc files. The
#   list carries ALL FIVE of zsh's files: the files the wrapper redirects are
#   exactly those, and the gate never asks the question for a file whose name
#   is not in the list. `.zlogout` is in the list even though we have no such
#   file — the gate protects the user's files, not our directory.
# - bt-gpu sees no platform library (040): its direct dependencies (dev
#   included, `--depth 1`) and its source contain no objc2/dispatch2/block2/
#   metal. The GPU is reached through wgpu; what wgpu's Metal backend pulls in
#   **indirectly** is not this check's subject — the contract is the crate's
#   own code and manifest, the backend's internal dependencies are wgpu's
#   business. The layer and the vsync rhythm come from bt-shell-macos
#   (`Surface::from_layer`, `Pacer`). The `bt-shell` pattern of the "bt-gpu
#   does not link upward" line catches all three shell crates.
# - bt-shell-common sees no platform shell (043 Karar 2): its direct normal
#   dependencies contain no objc2 core, AppKit, Quartz, Foundation,
#   notification center, block2, or a platform shell (`bt-shell-macos`/
#   `-linux`) — the layering direction is `bt-shell-{macos,linux} ->
#   bt-shell-common`. Its only macOS-specific dependency is `dispatch2`, under
#   `cfg(macos)`; in the source `objc2`/`dispatch2`/`block2` appear only in
#   `watch`'s macOS body (`watch/dispatch.rs`).
# - It does NOT fail on a dependency change, it warns: a deliberate dependency
#   decision also changes Cargo.lock; `/audit` looks for the decision's record.
audit:
	@fail=0; \
	if $(CARGO) tree -p bt-core -e normal | grep -E "objc2|core-text|core-graphics|metal"; then echo "audit: bt-core links to a platform library"; fail=1; fi; \
	if $(CARGO) tree -p bt-atlas -e normal | grep -E "(^|[ ─])objc2 v"; then echo "audit: bt-atlas links to the objc2 core"; fail=1; fi; \
	if grep -rnE "objc2_core_|\b(CT|CG|CF)[A-Z][A-Za-z]+" crates/bt-atlas/src --include='*.rs' | grep -v "^crates/bt-atlas/src/coretext.rs:" | grep -v ":[[:space:]]*//"; then echo "audit: CoreText/CoreGraphics name in bt-atlas outside the macOS backend (coretext.rs)"; fail=1; fi; \
	if $(CARGO) tree -p bt-gpu -e normal | grep -E "bt-shell"; then echo "audit: bt-gpu links upward, to the shell layer (bt-shell-*)"; fail=1; fi; \
	if $(CARGO) tree -p bt-shell-common -e normal --depth 1 | grep -E "(^|[ ─])objc2 v|objc2-app-kit|objc2-quartz-core|objc2-foundation|objc2-user-notifications|block2|bt-shell-(macos|linux)"; then echo "audit: bt-shell-common links to a platform shell or the AppKit family"; fail=1; fi; \
	if grep -rnE "objc2|dispatch2|block2" crates/bt-shell-common/src | grep -v "^crates/bt-shell-common/src/watch/dispatch.rs:" | grep -v ":[[:space:]]*//"; then echo "audit: platform call in bt-shell-common outside watch's macOS body (watch/dispatch.rs)"; fail=1; fi; \
	if grep -rn "objc2\|core_text\|core_graphics" crates/bt-core/src | grep -v ":[[:space:]]*//"; then echo "audit: platform call in bt-core source"; fail=1; fi; \
	if $(CARGO) tree -p bt-gpu -e normal,dev --depth 1 | grep -E "objc2|dispatch2|block2|metal"; then echo "audit: bt-gpu links directly to a platform library"; fail=1; fi; \
	if grep -rnE "objc2|dispatch2|block2|metal" crates/bt-gpu/src | grep -v ":[[:space:]]*//"; then echo "audit: platform call in bt-gpu source"; fail=1; fi; \
	for f in crates/bt-core/src/*.rs; do \
		awk -v file="$$f" '/^[[:space:]]*#\[cfg\(test\)\]/{exit} /\.unwrap\(\)|\.expect\(|panic!|unreachable!/ && !/\/\/ audit: / && !/^[[:space:]]*\/\//{print file":"NR": "$$0; hit=1} END{exit hit}' "$$f" \
			|| { echo "audit: unjustified panic path in bt-core ($$f)"; fail=1; }; \
	done; \
	if [ -d assets/shell ] && grep -rnE "(>>?|sed -i|tee).*(\.zshenv|\.zprofile|\.zshrc|\.zlogin|\.zlogout|\.bashrc|\.bash_profile|\.profile|config\.fish)" assets/shell; then echo "audit: shell integration writes to the user's rc file"; fail=1; fi; \
	git diff --quiet HEAD -- Cargo.lock $$(git ls-files '*Cargo.toml') || echo "audit: warning — Cargo.toml/Cargo.lock differs from HEAD; is the dependency decision recorded?"; \
	if grep -rnEi "bateri|bt-(core|gpu|shell|atlas)|make [a-z]|cargo|crates/|assets/|metal|olcumler|yol-harita|arastirma|ayarlar\.md|zsh|dock|emoji|glyph|\bcrate|alacritty|terminfo|origin/main" .claude/skills .claude/is-akisi/duzen.md .claude/is-akisi/sablonlar; then echo "audit: project trace in a generic workflow file — it belongs in .claude/is-akisi/proje.md"; fail=1; fi; \
	test $$fail -eq 0 && echo "audit: clean"

clippy:
	$(CARGO) clippy --workspace --all-targets -- -D warnings

# `--all-targets`, not the bare form: the bare form also runs doc-tests, and the
# workspace has none — yet rustdoc still compiles every crate once more to find
# that out, and that pass was most of the test step's time. A doc-test added
# later must not stay silent: put `--doc` back here in the same commit.
test:
	$(CARGO) test --workspace --all-targets

# Opens the window, and when BT_RUN_SECONDS expires looks at the number of
# frames, cells, glyphs, rule lines and atlas slots:
# frames=N cells=K glyphs=G rules=R slots=U/T slots2=U/T load=smoke requests=I content=C \
#   motion=M slide=S quiet=Sms teardown=clean profile=debug samples=off pipeline=ok
# Red if ONE of the first four (frames, cells, glyphs, rules) or `motion` is 0;
# `slots`, `slots2`, `load`, `requests`, `slide` and `profile` are not gates,
# they are counters and labels. `slots` is the atlas's **mask** plane, `slots2`
# its **color** plane (023): both share the same slot grid, have separate
# counters, and their totals are the same. Since the recipe prints no emoji,
# `slots2=0/T` is expected; the reason it is on the line is diagnosis — a plane
# it could not see would be 021's Braille shape (spending zero slots, silent).
# `slide` is the witness of the content's offset (011) and is expected to be 0
# because the recipe does not trigger it; the reason it is on the line is
# diagnosis, read together with `motion` it tells which animator did not settle.
# `motion` moved from counter to requirement in 008: the smoke recipe has a
# cursor motion (bt-core smoke_shell), so 0 means "the animation path never
# ran". The hidden link is written there too — the requirement rests on the
# default cursor style being ANIMATED.
# THE SECOND ANIMATION GATE IS INVISIBLE IN THE TOKEN: if an animation has not
# settled at the deadline the run falls red (app.rs Verdict::MotionUnsettled)
# and the line is never printed — the decision is in `verdict`, because the
# token line is printed only on a green run. Its whole value is being
# independent of speed: the `content` limit only sees a leak that is fast
# enough.
# THE UPPER LIMIT is on `content`, NOT on `frames` (see app.rs
# IDLE_FRAME_LIMIT): the symptom of a change that breaks zero-frames-when-idle
# is not too few frames but TOO MANY, yet a legitimate animation also inflates
# `frames` — so the limit counts the content frames that were DECIDED to be
# drawn. `slots` is not a gate but a counter — its threshold was not measured,
# and an unmeasured number is not written into a gate.
# THE LOWER LIMIT is on `quiet` (the time between the last frame and the
# deadline; if there are no frames at all it is `none` and that is red too):
# MEASURED in 008 phase-6 and wired into the gate (app.rs QUIET_FLOOR; the
# number and its derivation are owned by docs/OLCUMLER.md). Its rule is the
# OPPOSITE of the others — large in a healthy run, small under a leak — and it
# is the gate's most sensitive layer: it sees EVERY leak whose period is
# shorter than the floor, while the `content` limit sees only one that is fast
# enough. A leak that requests frames rarely enough to stay under the limit
# passed GREEN without this arm (the measured evidence is in that file).
# The floor DEPENDS on the duration of `BT_RUN_SECONDS` and on smoke_shell's
# sleep: whoever shortens the duration must re-derive it too, or the gate
# fails while the code is right.
# `teardown` is PARTLY a gate: panic arms turn it red, the two arms that are a
# recorded debt do not — if they were wired in, the gate would fall red over a
# known debt.
# `samples=off` = the measurement gate (BT_FRAME_STATS) was off; measurement
# tokens are NEVER printed in that run.
# The full list of tokens and their contract: app.rs Report::token_line;
# the `teardown` values are in teardown_token, `insufficient` in push_span.
# In a headless environment the binary prints "SKIPPED" to stdout and exits
# 78; make returns that as 2 — the distinguishing signal is the stdout text,
# not the exit code. `cargo run` respects CARGO_TARGET_DIR and passes through
# the child's exit code.
# `env -u`: a BT_SCROLL_TEST or BT_FRAME_STATS exported in the shell would
# SILENTLY turn the gate into another run — `var_os` looks at PRESENCE, not
# the value, so even `BT_SCROLL_TEST=` selects the load. That run prints
# `load=load` and exits 0, while the `cells`/`rules` half is never tested: the
# gate stays green but tests something other than what it claims. The gate
# must be hermetic.
# In a timed run the window opens at a floating level (`float_for_timed_run`):
# wgpu gives no drawable to an occluded window, and the gate used to fall with
# `frames=0` when another application was in front (040 phase-7).
smoke:
	env -u BT_SCROLL_TEST -u BT_FRAME_STATS BT_RUN_SECONDS=3 $(CARGO) run -q -p bateri

# WGSL canary: the shaders are embedded with `include_str!` and have no build
# step, so the canary is the test that sets up the pipelines — naga validation
# + pipeline creation on a device requested with Vulkan's immediate floor (040
# Karar 9). Since there is a single canary, "1 passed" is searched for: if the
# test's name changes or it falls behind `cfg`, cargo returns 0 with zero tests
# and the gate would silently stay green.
shader:
	@out=$$($(CARGO) test -p bt-gpu --lib -- --exact renderer::wgpu_tests::wgsl_pipelines_build 2>&1); st=$$?; \
	echo "$$out" | tail -3; \
	test $$st -eq 0 && echo "$$out" | grep -q "test result: ok. 1 passed" || { echo "shader: the canary did not run or failed"; exit 1; }

# Runs on changes that touch shared state (PTY reader thread <-> frame
# producer). ThreadSanitizer needs nightly; the toolchain is not pinned (there
# is deliberately no `rust-toolchain.toml`, Homebrew rustc) and there is no
# nightly: instead two different timing profiles — first only the race_*
# stress, then the whole suite INCLUDING the ignored ones on a single thread.
# Without --include-ignored the second line would be a copy of `make test` and
# would never run the race tests. When TSan nightly arrives, the third line
# goes HERE.
test-race:
	$(CARGO) test --workspace -- --ignored race_
	$(CARGO) test --workspace -- --include-ignored --test-threads=1

# Inventory of the fallback-glyph gate (041): passes every character in the
# symbol and emoji blocks through the atlas's own gate and prints, per block
# and for 13/16pt × @1x/@2x, the groups (in the base / fit from the fallback /
# in no font / rejected by the gate), the ratio histogram of the rejected ones
# and a per-font breakdown. The definition of the ratios is
# `crates/bt-atlas/src/census.rs` -> `classify`. NOT in `make check`: its
# result depends on the fonts installed on the machine, so it cannot be a
# gate. `BT_SCAN_FONT=Family` changes the base family. Release, because nearly
# ten thousand characters are measured.
scan:
	$(CARGO) test -p bt-atlas --release -- --ignored census --nocapture

# Builds release and installs `bateri.app` under target/ (NOT to
# /Applications). It contains Sparkle (`Contents/Frameworks/`, below) and the
# signature; notarization is `package`'s job, because it goes to Apple and
# takes minutes.
# An unsigned package is also signed at least **ad-hoc** (`codesign -s -`)
# before the check: the signature the linker puts on the binary does not seal
# the package's resources, and Gatekeeper called the copy sent to another Mac
# as a zip "damaged" and trashed it (seen on the user's friend's machine). With
# a valid ad-hoc signature the same copy drops to the "unidentified developer"
# warning and opens via System Settings -> Privacy & Security -> "Open Anyway".
#
# The signing identity (`SIGN_ID`) is **chosen automatically**: a valid
# "Developer ID Application" in the keychain if there is one, else "Apple
# Development", else ad-hoc (`-`). So that there is no setting to remember;
# since the certificate's name carries personal data (an e-mail), it is not
# written into the repo and is read from the keychain on every run. It can be
# overridden by hand: `make bundle SIGN_ID=-` forces ad-hoc, `SIGN_ID="name"` a
# specific identity. The difference is the identity's **persistence**: the
# requirement an ad-hoc signature defines is the package's own fingerprint and
# changes on every build, so macOS treats every build as a different
# application and the granted permissions (accessibility, full disk access)
# are asked again after an update. A certificate you generated in the keychain
# binds that requirement to the certificate and keeps it stable across builds.
# Nothing changes for Gatekeeper on another Mac — nobody trusts the
# certificate, the warning is the same; only Developer ID plus notarization
# removes it. If there is no identity it falls, by name, before `codesign`.
#
# If the identity is not ad-hoc the signature is applied with the **hardened
# runtime** (`-o runtime`): notarization requires it and library validation
# loads the Sparkle in the package only with the same team's signature. With
# Developer ID there is also a **secure timestamp** (`--timestamp`, Apple's
# server — `bundle` needs the network with that identity); notarization rejects
# a signature without a timestamp. Apple Development and ad-hoc stay without a
# timestamp: they will not be notarized and `make install` must work offline
# too.
SIGN_OPTS = $(if $(filter -,$(SIGN_ID)),--timestamp=none,--options runtime $(if $(findstring Developer ID Application:,$(SIGN_ID)),--timestamp,--timestamp=none))
SIGN = codesign --force --sign '$(SIGN_ID)' $(SIGN_OPTS)
SIGN_ID ?= $(eval SIGN_ID := $$(shell ids=$$$$(security find-identity -v -p codesigning 2>/dev/null); \
	for kind in "Developer ID Application" "Apple Development"; do \
		n=$$$$(printf '%s\n' "$$$$ids" | sed -n "s/.*\"\($$$$kind: [^\"]*\)\".*/\1/p" | head -n 1); \
		[ -n "$$$$n" ] && { echo "$$$$n"; exit 0; }; \
	done; echo -))$(SIGN_ID)
#
# The package is first built in `$(STAGE)` and moved to replace `$(APP)` only
# if it passes the check: had it been built in place, a failed run would leave
# an openable package without a license or icon at the path the Dock shows.
#
# The CONTENT of the inputs (`assets/bundle/`, `assets/shell/`) is tested by
# `bundle_assets` inside `make check`; the check here tests the PRODUCT: if an
# input stayed in place and was not copied into the package, that test would
# stay green. The check's list is deliberately written separately from the copy
# lines — had it read from the same variable, a file deleted from the copy
# would be deleted from the check too.
#
# `LSMinimumSystemVersion` is filled from the binary's own `minos`, TOML is not
# parsed: `.cargo/config.toml`'s `[env]` reaches rustc from there, but a
# `MACOSX_DEPLOYMENT_TARGET` exported in the shell overrides it. When the plist
# is read from the binary the two can never diverge — if they did,
# LaunchServices would open the app on a system it cannot run on.
#
# The URL scheme (`CFBundleURLTypes` -> `bateri`, 038) is also looked for in
# the product: `bateri://tab/<id>` is registered with LaunchServices only from
# the package's plist, and a scheme dropped from the template would silently
# turn into "open opens nothing".
#
# `X = $(eval X := $$(shell …))$(X)`: lazy AND once. Plain `=` would rerun the
# command on every expansion (`$(APP)` is expanded more than twenty times in
# the recipe), `:=` on every make invocation — `check` included. The target
# directory comes from `cargo metadata`: `CARGO_TARGET_DIR` is not the only
# source (`build.target-dir` and `CARGO_BUILD_TARGET_DIR` exist too) and a
# wrong directory would silently package a stale binary. What it does not
# cover: the `build.target` triple (`target/<triple>/`).
TARGET_DIR = $(eval TARGET_DIR := $$(shell $(CARGO) metadata --format-version 1 --no-deps | sed -n 's/.*"target_directory":"\([^"]*\)".*/\1/p'))$(TARGET_DIR)
APP = $(TARGET_DIR)/release/bateri.app
STAGE = $(APP).partial
ICONSET = $(TARGET_DIR)/release/bateri.iconset
# Updating: Sparkle 2 (`bt-shell-macos::updater` loads it at run time). The
# framework does not enter the repo; the version and sha256 are fixed here, the
# tarball is downloaded into `$(SPARKLE_DIR)` on the first `bundle`, and a
# download whose digest does not match fails. Only on the `bundle` path:
# `make check`, `smoke` and `cargo run` are without Sparkle. It enters the
# package with `ditto --arch arm64` (the tarball is universal, the package is
# Apple silicon only) and the XPC services are dropped — Sparkle's
# documentation wants them only for a sandboxed app, and bateri is not
# sandboxed (it spawns a shell). The remaining two helpers (`Autoupdate`,
# `Updater.app`) and the framework are re-signed **inside out** with our
# identity: the outer package's signature does not sign the inner ones and
# Sparkle comes ad-hoc signed. No `--deep` (Sparkle's documentation).
SPARKLE_VERSION = 2.10.0
SPARKLE_SHA256 = c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c
SPARKLE_DIR = $(TARGET_DIR)/sparkle-$(SPARKLE_VERSION)
# The feed Sparkle reads; written into `Info.plist` at `bundle` time.
# GitHub's `releases/latest/download/` address always serves the same-named
# file of the newest release, so publishing a version is publishing the update
# — the site has nothing to do with versions, and even if the site went down
# installed copies would still update. Overridden in trials: `make package
# FEED_URL=https://…`.
REPO = bateri/bateri
FEED_URL ?= https://github.com/$(REPO)/releases/latest/download/appcast.xml

sparkle:
	@test -f $(SPARKLE_DIR)/.verified && exit 0; \
	rm -rf $(SPARKLE_DIR) && mkdir -p $(SPARKLE_DIR) && \
	curl -fsSL -o $(SPARKLE_DIR).tar.xz \
		https://github.com/sparkle-project/Sparkle/releases/download/$(SPARKLE_VERSION)/Sparkle-$(SPARKLE_VERSION).tar.xz && \
	echo '$(SPARKLE_SHA256)  $(SPARKLE_DIR).tar.xz' | shasum -a 256 -c - >/dev/null || \
		{ echo "sparkle: Sparkle-$(SPARKLE_VERSION).tar.xz could not be downloaded or its digest does not match"; rm -f $(SPARKLE_DIR).tar.xz; exit 1; }; \
	tar -xJf $(SPARKLE_DIR).tar.xz -C $(SPARKLE_DIR) && rm -f $(SPARKLE_DIR).tar.xz && touch $(SPARKLE_DIR)/.verified

# The version comes from the manifest (`cargo metadata --no-deps`), not from
# `cargo pkgid`: pkgid reads Cargo.lock and the lock is updated only at build
# time — the first `make release` after a version bump was producing a package
# with the old number (measured, in the update trial).
VERSION = $(eval VERSION := $$(shell $(CARGO) metadata --format-version 1 --no-deps | sed -n 's/.*"name":"bateri","version":"\([^"]*\)".*/\1/p'))$(VERSION)
# The icon's name in one place: the template's `CFBundleIconFile`.
ICON = $(eval ICON := $$(shell plutil -extract CFBundleIconFile raw assets/bundle/Info.plist.in))$(ICON)

bundle: sparkle
	@case '$(APP)' in *[[:space:]]*) echo "bundle: the target directory has a space ($(APP)); the recipe would split the paths"; exit 1;; esac
	@# CFBundleVersion wants at most a three-part dotted integer; `0.2.0-alpha.1` passes plutil but not LaunchServices.
	@echo '$(VERSION)' | grep -Eq '^[0-9]+(\.[0-9]+){0,2}$$' || { echo "bundle: version '$(VERSION)' is not in CFBundleVersion format"; exit 1; }
	$(CARGO) build --release -p bateri
	rm -rf $(STAGE) $(ICONSET)
	mkdir -p $(STAGE)/Contents/MacOS $(STAGE)/Contents/Resources $(ICONSET)
	cp $(TARGET_DIR)/release/bateri $(STAGE)/Contents/MacOS/
	minos=$$(vtool -show-build $(STAGE)/Contents/MacOS/bateri | awk '$$1=="minos"{print $$2; exit}'); \
	sed -e 's/@VERSION@/$(VERSION)/g' -e "s/@MACOS_MIN@/$$minos/g" -e 's|@FEED_URL@|$(FEED_URL)|g' \
		assets/bundle/Info.plist.in > $(STAGE)/Contents/Info.plist
	@# iconutil wants the standard ten sizes; `sips -s format icns` rejects 1024.
	@for s in 16 32 128 256 512; do \
		sips -z $$s $$s assets/bundle/$(ICON).png --out $(ICONSET)/icon_$${s}x$${s}.png >/dev/null && \
		sips -z $$((s*2)) $$((s*2)) assets/bundle/$(ICON).png --out $(ICONSET)/icon_$${s}x$${s}@2x.png >/dev/null || exit 1; \
	done
	iconutil -c icns $(ICONSET) -o $(STAGE)/Contents/Resources/$(ICON).icns
	rm -rf $(ICONSET)
	@# GPL-3.0 §4: the binary ships with a copy of the license; its source is LICENSE at the root.
	cp LICENSE assets/bundle/Credits.html assets/bundle/THIRD-PARTY-LICENSES.txt $(STAGE)/Contents/Resources/
	@# The wrapper is copied file by file, NOT with `cp -R assets/shell`:
	@# as long as ZDOTDIR points at our directory, an arm that writes there
	@# (in 009 phase-3 `/etc/zshrc` once spawned a `.zsh_history`) or a
	@# `.DS_Store` would silently enter the product through a recursive copy.
	@# That the directory's inventory is EXACTLY these five files is tested
	@# by `bundle_assets`.
	mkdir -p $(STAGE)/Contents/Resources/shell/zsh
	cp assets/shell/zsh/.zshenv assets/shell/zsh/.zprofile assets/shell/zsh/.zshrc \
		assets/shell/zsh/.zlogin assets/shell/zsh/bateri.zsh \
		$(STAGE)/Contents/Resources/shell/zsh/
	mkdir -p $(STAGE)/Contents/Frameworks
	ditto --arch arm64 $(SPARKLE_DIR)/Sparkle.framework $(STAGE)/Contents/Frameworks/Sparkle.framework
	rm -rf $(STAGE)/Contents/Frameworks/Sparkle.framework/XPCServices \
		$(STAGE)/Contents/Frameworks/Sparkle.framework/Versions/B/XPCServices
	@# Signing is last and inside out: any byte entering the package later
	@# would break the seal, and the outer signature also seals the inner ones.
	@test '$(SIGN_ID)' = - || security find-identity -p codesigning | grep -qF '"$(SIGN_ID)"' || \
		{ echo "bundle: there is no code-signing identity named '$(SIGN_ID)' in the keychain (security find-identity -p codesigning)"; exit 1; }
	$(SIGN) $(STAGE)/Contents/Frameworks/Sparkle.framework/Versions/B/Autoupdate
	$(SIGN) $(STAGE)/Contents/Frameworks/Sparkle.framework/Versions/B/Updater.app
	$(SIGN) $(STAGE)/Contents/Frameworks/Sparkle.framework
	$(SIGN) $(STAGE)
	codesign --verify --deep --strict $(STAGE)
	@c=$(STAGE)/Contents; fail() { echo "bundle: content check failed — $$1"; exit 1; }; \
	key() { plutil -extract "$$1" raw $$c/Info.plist 2>/dev/null; }; \
	plutil -lint -s $$c/Info.plist || fail "Info.plist is invalid"; \
	! grep -q '@[A-Z_]*@' $$c/Info.plist || fail "Info.plist has an unfilled placeholder"; \
	exe=$$c/MacOS/$$(key CFBundleExecutable); test -f $$exe && test -x $$exe || fail "CFBundleExecutable is not in the package"; \
	minos=$$(vtool -show-build $$exe | awk '$$1=="minos"{print $$2; exit}'); \
	test -n "$$minos" && test "$$(key LSMinimumSystemVersion)" = "$$minos" || fail "LSMinimumSystemVersion is not the binary's minos ('$$minos')"; \
	test -s "$$c/Resources/$$(key CFBundleIconFile).icns" || fail "the icon is not in the package"; \
	test "$$(key CFBundleURLTypes.0.CFBundleURLSchemes.0)" = bateri || fail "the URL scheme (bateri) is not in Info.plist"; \
	test "$$(key SUFeedURL)" = '$(FEED_URL)' || fail "SUFeedURL is not '$(FEED_URL)'"; \
	test -n "$$(key SUPublicEDKey)" || fail "SUPublicEDKey is not in Info.plist"; \
	fw=$$c/Frameworks/Sparkle.framework; \
	test -f $$fw/Versions/B/Sparkle && test -x $$fw/Versions/B/Autoupdate || fail "Sparkle.framework is not in the package"; \
	test ! -e $$fw/Versions/B/XPCServices || fail "Sparkle's XPC services were left in the package"; \
	test "$$(lipo -archs $$fw/Versions/B/Sparkle)" = arm64 || fail "Sparkle was not thinned to arm64"; \
	team() { codesign -dv "$$1" 2>&1 | sed -n 's/^TeamIdentifier=//p'; }; \
	for x in $$fw $$fw/Versions/B/Autoupdate $$fw/Versions/B/Updater.app; do \
		test "$$(team $$x)" = "$$(team $(STAGE))" || fail "$$x is not signed with the same identity as the package"; \
	done; \
	test '$(SIGN_ID)' = - || codesign -dv $(STAGE) 2>&1 | grep -q 'flags=.*runtime' || fail "no hardened runtime"; \
	cmp -s LICENSE $$c/Resources/LICENSE || fail "LICENSE is not in the package or differs from the input"; \
	for f in Credits.html THIRD-PARTY-LICENSES.txt; do \
		cmp -s assets/bundle/$$f $$c/Resources/$$f || fail "$$f is not in the package or differs from the input"; \
	done; \
	for f in zsh/.zshenv zsh/.zprofile zsh/.zshrc zsh/.zlogin zsh/bateri.zsh; do \
		cmp -s assets/shell/$$f $$c/Resources/shell/$$f || fail "shell/$$f is not in the package or differs from the input"; \
	done; \
	rm -rf $(APP) && mv $(STAGE) $(APP) && \
	echo "bundle: $(APP) (version $(VERSION), macOS floor $$minos, signature $(if $(filter -,$(SIGN_ID)),ad-hoc,'$(SIGN_ID)'))"

# Targets that have no input yet. They exist so that `proje.md`'s verification
# table does not carry a target name that does not exist; if run they say "not
# yet" and fall red, they do not say "passed". make returns a recipe failure as
# 2; the distinguishing signal is the "not yet" text on stdout. When the
# target becomes real, delete its line and remove it from the list at the top
# of proje.md too.
henuz_yok = @echo "not yet: $(1)"; exit 1


# The zip to be sent to another Mac: compresses `bundle`'s checked package with
# `ditto` (the same format as Finder's "Compress"; `zip -r` can corrupt macOS's
# extended attributes and the signature seal). It has the version in its name
# so an old zip is not sent by mistake.
#
# **Notarized with Developer ID**: the package is zipped and goes to Apple
# (`notarytool submit --wait`, the `$(NOTARY_PROFILE)` profile in the keychain —
# set up with `xcrun notarytool store-credentials`, the password does not enter
# the repo), and if accepted the ticket is **stapled** to the package and the
# zip is taken again AFTER that: the ticket is inside the package, and a Mac
# that opens an unstapled zip offline cannot ask Apple and would drop to the
# warning. The gate is `spctl` saying "Notarized Developer ID". With another
# identity there is no notarization and the recipient passes the Gatekeeper
# warning on first launch via System Settings -> Privacy & Security -> "Open
# Anyway".
#
# **Two outputs, two readers.** The zip is Sparkle's: on an update the user
# drags nothing and the zip is small; the feed's item names it with its
# version. The DMG is for first install and has **no version** in its name:
# GitHub's `releases/latest/download/bateri.dmg` link then never goes stale and
# the site shows it as is. On first install the Finder window shows a single
# job that makes the user drag the app to Applications (background, arrow,
# Applications shortcut; layout `assets/dmg/settings.py`, visual
# `tools/dmg_background.py`). An app that came with the zip stayed in
# Downloads and was opened from there; macOS runs an app opened that way from a
# read-only copy (App Translocation) and Sparkle cannot update it. The DMG is
# built from the stapled app inside it, and is itself also signed, notarized
# and stapled — a Mac that opens the downloaded DMG should not ask questions
# either.
#
# The DMG is built by `dmgbuild` (version and dependencies pinned, downloaded
# into a venv at `$(DMGBUILD_DIR)` on the first run, does not enter the repo):
# it writes Finder's layout file (`.DS_Store`) itself, i.e. it does not drive
# Finder with AppleScript — it runs headless and without asking permission.
ZIP = $(TARGET_DIR)/release/bateri-$(VERSION).zip
DMG = $(TARGET_DIR)/release/bateri.dmg
NOTARY_PROFILE ?= bateri-notary
DMGBUILD_VERSION = 1.6.7
DMGBUILD_DIR = $(TARGET_DIR)/dmgbuild-$(DMGBUILD_VERSION)

dmgbuild:
	@test -x $(DMGBUILD_DIR)/bin/dmgbuild && exit 0; \
	rm -rf $(DMGBUILD_DIR) && python3 -m venv $(DMGBUILD_DIR) && \
	$(DMGBUILD_DIR)/bin/pip install -q --disable-pip-version-check \
		dmgbuild==$(DMGBUILD_VERSION) ds_store==1.3.3 mac_alias==2.2.3 || \
		{ echo "dmgbuild: could not be installed (python3 -m venv + pip, needs the network)"; rm -rf $(DMGBUILD_DIR); exit 1; }

# Notarizes and staples: $(1) is the file that goes to Apple, $(2) the item to
# staple. Never called with an identity other than Developer ID.
define notarize
	out=$$(xcrun notarytool submit $(1) --keychain-profile '$(NOTARY_PROFILE)' --wait 2>&1) || true; \
	echo "$$out" | tail -n 2; \
	echo "$$out" | grep -q 'status: Accepted' || { \
		id=$$(echo "$$out" | sed -n 's/^ *id: //p' | head -n 1); \
		echo "package: notarization was not accepted$${id:+ — details: xcrun notarytool log $$id --keychain-profile $(NOTARY_PROFILE)}"; exit 1; }; \
	xcrun stapler staple -q $(2); \
	xcrun stapler validate -q $(2)
endef

package: bundle dmgbuild
	codesign --verify --deep --strict $(APP)
	rm -f $(ZIP) $(ZIP).notary $(DMG)
	@if printf '%s' '$(SIGN_ID)' | grep -q '^Developer ID Application:'; then \
		set -e; \
		ditto -c -k --sequesterRsrc --keepParent $(APP) $(ZIP).notary; \
		echo "package: sending the app to Apple for notarization (may take a few minutes)"; \
		$(call notarize,$(ZIP).notary,$(APP)); \
		rm -f $(ZIP).notary; \
		spctl -a -vv -t exec $(APP) 2>&1 | grep -q 'source=Notarized Developer ID' || \
			{ echo "package: spctl does not see the package as notarized"; spctl -a -vv -t exec $(APP); exit 1; }; \
	else \
		echo "package: NOT notarized — signature is '$(SIGN_ID)', not Developer ID"; \
	fi
	ditto -c -k --sequesterRsrc --keepParent $(APP) $(ZIP)
	$(DMGBUILD_DIR)/bin/dmgbuild -s assets/dmg/settings.py -D app=$(APP) \
		-D icon=$(APP)/Contents/Resources/$(ICON).icns -D here=$(CURDIR)/assets/dmg bateri $(DMG)
	@if printf '%s' '$(SIGN_ID)' | grep -q '^Developer ID Application:'; then \
		set -e; \
		codesign --force --sign '$(SIGN_ID)' --timestamp $(DMG); \
		echo "package: sending the DMG to Apple for notarization"; \
		$(call notarize,$(DMG),$(DMG)); \
		spctl -a -vv -t open --context context:primary-signature $(DMG) 2>&1 | grep -q 'source=Notarized Developer ID' || \
			{ echo "package: spctl does not see the DMG as notarized"; spctl -a -vv -t open --context context:primary-signature $(DMG); exit 1; }; \
	fi
	@echo "package: $(ZIP) + $(DMG) ($$(lipo -archs $(APP)/Contents/MacOS/bateri), macOS $$(plutil -extract LSMinimumSystemVersion raw $(APP)/Contents/Info.plist)+)"

# Publishing a version is a single command, on this Mac:
#
#   make ship        release + push main + publish — end to end
#
# Its two halves can also be called separately, because the split itself is the
# point: the zip is tried on this Mac before it is opened to everyone.
#
#   make release     gates, package (build, sign, notarize, staple),
#                    release note and feed -> $(RELEASE_DIR); apart from
#                    Apple's notarization nothing leaves the machine
#   make publish     tags the built commit as v<version>, pushes the tag and
#                    opens the GitHub release with the zip, DMG and feed
#                    (`gh`)
#
# **Gates first, then the package** (`release-gate`; the package takes minutes
# and the gate is cheap): the version comes from a known code — the tree is
# clean and `v$(VERSION)` exists neither here nor on origin — and comes with a
# release note. The branch is not asked: the real guarantee is `publish`'s
# "commit is on origin/main" gate, i.e. a commit going to main can also be
# packaged in another worktree. The note's only source is `CHANGELOG.md` at the
# root (Keep a Changelog, English): the gate cuts out the `## [$(VERSION)]`
# section, and if the section is missing or empty it stops before the package
# is built. The note shows both on the release page and in Sparkle's "new
# version available" window — a window without a note was pushing the user to
# "Install" without knowing what they were installing. The version number is
# `Cargo.toml`'s; the gate reads it, there is no need to give `VERSION=`.
#
# `release` writes the commit it built to `$(RELEASE_DIR)/commit` and `publish`
# tags **that** commit, not whatever HEAD is at that moment — the tag always
# names the code inside the zip. The tag is not pushed on its own: if the
# commit is not on origin/main `publish` stops, otherwise a commit no branch
# holds would be published.
#
# **The feed has a single item.** `releases/latest/download/appcast.xml` always
# serves the newest release's, so the only item read is the newest; the items
# of older versions would reach no installed copy. `generate_appcast` runs from
# a temporary folder that holds only this version's zip and note, signs the zip
# with the secret EdDSA key (the key is in the keychain, `generate_keys`; if it
# is lost no more updates can ever be sent to installed copies — back it up
# with `generate_keys -x`) and the download address is the release's own
# address (`releases/download/v<version>/`). There are no delta updates
# (`--maximum-deltas 0`): the package is small. A package that is not notarized
# does not enter the release: the copy Sparkle downloads must also pass
# Gatekeeper.
#
# Rolling back a faulty version is **a new version**: Sparkle does not go down
# to an older version, and deleting a release returns `latest` to the previous
# one but leaves the copies that installed that version where they are.
RELEASE_DIR = $(TARGET_DIR)/release/v$(VERSION)
NOTES = $(RELEASE_DIR)/notes.md
RELEASE_URL = https://github.com/$(REPO)/releases/download/v$(VERSION)/

release-gate:
	@echo '$(VERSION)' | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "release: version '$(VERSION)' is not x.y.z (Cargo.toml)"; exit 1; }
	@test -z "$$(git status --porcelain)" || { echo "release: the working tree is not clean, the version must come from a known code"; exit 1; }
	@! git rev-parse -q --verify 'refs/tags/v$(VERSION)' >/dev/null || { echo "release: tag v$(VERSION) already exists — bump the version in Cargo.toml"; exit 1; }
	@! git ls-remote --exit-code --tags origin 'refs/tags/v$(VERSION)' >/dev/null || { echo "release: tag v$(VERSION) already exists on origin"; exit 1; }
	@rm -rf $(RELEASE_DIR) && mkdir -p $(RELEASE_DIR); \
	awk -v v='$(VERSION)' '/^## \[/ { if (on) exit; if (index($$0, "## [" v "]") == 1) { on = 1; next } } on' CHANGELOG.md \
		| awk 'NF { p = 1 } p' | awk '{ l[NR] = $$0 } END { n = NR; while (n > 0 && l[n] ~ /^[[:space:]]*$$/) n--; for (i = 1; i <= n; i++) print l[i] }' > $(NOTES); \
	grep -q '[^[:space:]]' $(NOTES) || { echo "release: CHANGELOG.md has no '## [$(VERSION)]' section or it is empty (turn Unreleased into the version)"; exit 1; }

release: release-gate package
	@spctl -a -vv -t exec $(APP) 2>&1 | grep -q 'source=Notarized Developer ID' && xcrun stapler validate -q $(APP) || \
		{ echo "release: the package is not notarized, it does not enter the release"; exit 1; }
	@spctl -a -vv -t open --context context:primary-signature $(DMG) 2>&1 | grep -q 'source=Notarized Developer ID' || \
		{ echo "release: the DMG is not notarized, it does not enter the release"; exit 1; }
	@test "$$(plutil -extract SUFeedURL raw $(APP)/Contents/Info.plist)" = '$(FEED_URL)' || { echo "release: the package's feed is not $(FEED_URL)"; exit 1; }
	cp $(ZIP) $(DMG) $(RELEASE_DIR)/
	rm -rf $(RELEASE_DIR)/feed && mkdir -p $(RELEASE_DIR)/feed
	cp $(ZIP) $(RELEASE_DIR)/feed/
	cp $(NOTES) $(RELEASE_DIR)/feed/bateri-$(VERSION).md
	$(SPARKLE_DIR)/bin/generate_appcast --maximum-deltas 0 --embed-release-notes \
		--download-url-prefix '$(RELEASE_URL)' -o $(RELEASE_DIR)/appcast.xml $(RELEASE_DIR)/feed
	rm -rf $(RELEASE_DIR)/feed
	@test "$$(grep -c '<item>' $(RELEASE_DIR)/appcast.xml)" = 1 && grep -q 'url="$(RELEASE_URL)bateri-$(VERSION).zip"' $(RELEASE_DIR)/appcast.xml \
		&& grep -q 'sparkle:edSignature=' $(RELEASE_DIR)/appcast.xml || { echo "release: the feed is not a single, signed item pointing at the release"; exit 1; }
	git rev-parse HEAD > $(RELEASE_DIR)/commit
	@echo "release: $(RELEASE_DIR) ($$(cat $(RELEASE_DIR)/commit)) — try $(APP), then: make publish"

publish:
	@for f in bateri-$(VERSION).zip bateri.dmg appcast.xml notes.md commit; do \
		test -f $(RELEASE_DIR)/$$f || { echo "publish: $(RELEASE_DIR)/$$f is missing — run make release first"; exit 1; }; \
	done
	@! git ls-remote --exit-code --tags origin 'refs/tags/v$(VERSION)' >/dev/null || { echo "publish: tag v$(VERSION) already exists on origin"; exit 1; }
	git fetch -q origin main
	@git merge-base --is-ancestor "$$(cat $(RELEASE_DIR)/commit)" origin/main || { echo "publish: the built commit is not on origin/main — push main first"; exit 1; }
	git tag -a 'v$(VERSION)' -m 'bateri $(VERSION)' "$$(cat $(RELEASE_DIR)/commit)"
	git push origin 'v$(VERSION)'
	gh release create 'v$(VERSION)' $(RELEASE_DIR)/bateri.dmg $(RELEASE_DIR)/bateri-$(VERSION).zip $(RELEASE_DIR)/appcast.xml \
		--repo '$(REPO)' --verify-tag --latest --title 'bateri $(VERSION)' --notes-file $(NOTES)
	@echo "publish: v$(VERSION) — https://github.com/$(REPO)/releases/tag/v$(VERSION)"

# Stops on non-main BEFORE waiting on notarization, not after:
# `publish` wants the commit on origin/main and only main is pushed here.
ship:
	@test "$$(git branch --show-current)" = main || { echo "ship: branch is not main"; exit 1; }
	$(MAKE) release
	git push origin main
	$(MAKE) publish

# Installs onto this Mac: puts `bundle`'s checked package into `$(INSTALL_DIR)`.
# It is not written over the old package with `ditto`, because `ditto` merges
# and a file deleted in the new version would be left over from the old
# package. The copy first lands under a temporary name beside it and replaces
# the old one only when the copy is done: so that an install cut halfway does
# not destroy the working package. It stops if a bateri is open and does not
# close it — killing the shells in the user's session is not a build target's
# decision.
INSTALL_DIR ?= /Applications
INSTALLED = $(INSTALL_DIR)/bateri.app

install: bundle
	@if pgrep -f '$(INSTALLED)/Contents/MacOS/bateri' >/dev/null; then \
		echo "install: $(INSTALLED) is open — quit it first (⌘Q), then retry"; exit 1; fi
	rm -rf $(INSTALLED).new
	ditto $(APP) $(INSTALLED).new
	codesign --verify --deep --strict $(INSTALLED).new
	rm -rf $(INSTALLED)
	mv $(INSTALLED).new $(INSTALLED)
	@echo "install: $(INSTALLED)"

terminfo:
	$(call henuz_yok,assets/terminfo comes with a shell/TERM set)

# Linux gate for bt-core, bt-atlas, bt-gpu and bt-shell-common: `clippy -D warnings` and `test`, in Docker,
# in the image from `tools/linux/Dockerfile`, with `--locked` (a run that would
# change Cargo.lock fails red, it does not silently resolve new versions).
# CLAUDE.md's "bt-core is platformless, the gate is compiling for Linux" is
# this command.
# OUTSIDE `make check`, because it needs Docker and its first run builds the
# image and compiles the whole graph for Linux; when it runs is in
# `.claude/is-akisi/proje.md` -> Doğrulama (if a crate that compiles on Linux
# changed).
# The scope grows with the sets — today bt-core, bt-atlas's FreeType/
# fontconfig/harfrust backend, bt-gpu (Vulkan; `wgsl_pipelines_build` and the
# offscreen pixel tests run on lavapipe, 042) and bt-shell-common (`jobs`'s
# `/proc` body and the real-PTY test, `child`'s `$SHELL -l` arm and the real
# zsh tests, 043); the order is in `docs/YOL-HARITASI.md`. As it grows the `-p`
# list and the image recipe change together.
# Order:
# 1. Version: RED (exit 1) if the local `rustc`'s major.minor is not the same
#    as the image tag's — two compilers' clippy are two separate gates. This is
#    not a "could not run": the fix is updating the Dockerfile's `FROM` line.
# 2. If Docker is missing or the daemon does not answer, it prints "SKIPPED"
#    to stdout and exits 78; make returns that as 2 — as in `make smoke`, the
#    distinguishing signal is the stdout text, not the exit code. In
#    verification `[~]` is written only in this arm.
# 3. It builds the image (layer-cached) and runs in the container: the repo is
#    bound to `/w`, outputs go to `target/linux` (so they do not mix with the
#    macOS build; `/target/` is already in .gitignore), crate downloads in a
#    named volume.
LINUX_DOCKERFILE = tools/linux/Dockerfile
LINUX_RUST = $(shell sed -n 's/^FROM rust:\([0-9]*\.[0-9]*\)-.*/\1/p' $(LINUX_DOCKERFILE))
LINUX_IMAGE = bateri-linux:$(LINUX_RUST)
LINUX_CRATES = -p bt-core -p bt-atlas -p bt-gpu -p bt-shell-common

linux:
	@yerel=$$(rustc --version | sed -n 's/^rustc \([0-9]*\.[0-9]*\).*/\1/p'); \
	if [ -z "$(LINUX_RUST)" ]; then \
		echo "linux: could not read the rust version from the FROM line of $(LINUX_DOCKERFILE)"; exit 1; fi; \
	if [ "$$yerel" != "$(LINUX_RUST)" ]; then \
		echo "linux: local rustc $$yerel, image rust:$(LINUX_RUST) — the versions do not match; bring the FROM line of $(LINUX_DOCKERFILE) to the local version"; exit 1; fi
	@if ! command -v docker >/dev/null 2>&1 || ! docker info >/dev/null 2>&1; then \
		echo "SKIPPED: Docker is missing or the daemon does not answer — make linux could not run"; exit 78; fi
	docker build -q -t $(LINUX_IMAGE) -f $(LINUX_DOCKERFILE) tools/linux
	docker run --rm -v "$(CURDIR)":/w -v bateri-linux-cargo:/usr/local/cargo/registry \
		-e CARGO_TARGET_DIR=/w/target/linux $(LINUX_IMAGE) sh -c '\
		cargo clippy $(LINUX_CRATES) --all-targets --locked -- -D warnings && \
		cargo test $(LINUX_CRATES) --all-targets --locked'
