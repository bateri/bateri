# The image of `make linux`: the single recipe for building and testing
# bt-core, bt-atlas, bt-gpu and bt-shell-common on Linux.
#
# The base tag's major.minor MUST match the local rustc — `make linux`
# compares the two first and fails red on a mismatch: two compilers' clippy
# are two different gates (Homebrew's rustc is not pinned; after `brew
# upgrade` this line is updated too). The Makefile reads the tag from here;
# the version lives only on this line.
FROM rust:1.88-bookworm

# zsh: bt-core's mirror tests and bt-shell-common's `child`/`jobs` tests run a
# real zsh. LANG: the image defaults to the C locale, where zsh does not
# recognise multibyte characters — a non-ASCII letter comes out as `??` in the
# mirror and three mirror tests fail because of the environment (measured).
#
# The font stack of bt-atlas's Linux backend (user-approved packages):
# pkg-config + libfreetype-dev + libfontconfig-dev are what freetype-sys and
# yeslogic-fontconfig-sys link against dynamically; zlib1g-dev so libz-sys
# finds the system zlib instead of compiling its own. fonts-dejavu-core is the
# text family: `monospace` resolves to DejaVu Sans Mono and the atlas tests'
# Linux fixture (`bt-atlas/src/freetype.rs`) is measured against it.
# fonts-noto-color-emoji is the colour font (CBDT bitmaps): the colour plane,
# the two halves of an emoji and the grapheme-cluster tests draw from it.
# mesa-vulkan-drivers + libvulkan1 are lavapipe, Mesa's CPU Vulkan driver, and
# its loader: bt-gpu's device (Vulkan on Linux), `wgsl_pipelines_build` and the
# offscreen pixel tests run on it.
# The version check stops the build if FreeType is older than 2.12.1
# (libtool 24.3.18 — 2.12.0 is 24.1.18), the release the backend was
# written against.
RUN apt-get update \
    && apt-get install -y --no-install-recommends zsh \
        pkg-config libfreetype-dev libfontconfig-dev zlib1g-dev fonts-dejavu-core \
        fonts-noto-color-emoji mesa-vulkan-drivers libvulkan1 \
    && rm -rf /var/lib/apt/lists/* \
    && fc-cache -f \
    && dpkg --compare-versions "$(pkg-config --modversion freetype2)" ge 24.3.18
ENV LANG=C.UTF-8

# bt-shell-common's real-zsh tests need two pieces of macOS's
# environment that Debian lacks:
# - locales + en_US.UTF-8: the emoji wrap test pins `LANG=en_US.UTF-8` (the
#   name `child::decide_locale` writes); the image only has C.UTF-8, and zsh in
#   an unknown locale counts `👍🏽` as 8 bytes, so the mirror never matches.
# - History in the system zshrc: macOS's `/etc/zshrc` sets
#   `HISTFILE=${ZDOTDIR:-$HOME}/.zsh_history` (with `HISTSIZE`/`SAVEHIST`)
#   before the user's files, and the wrapper test's fifth claim is that the
#   wrapper moves it out of its own directory and history is written to the
#   user's. Debian sets none of the three (and zsh saves nothing with
#   `SAVEHIST=0`), so without macOS's three lines the claim has no trigger.
#   They reproduce the test's precondition, not a product setting.
RUN apt-get update \
    && apt-get install -y --no-install-recommends locales \
    && rm -rf /var/lib/apt/lists/* \
    && sed -i 's/^# *en_US.UTF-8 UTF-8/en_US.UTF-8 UTF-8/' /etc/locale.gen \
    && locale-gen \
    && printf '%s\n' 'HISTFILE=${ZDOTDIR:-$HOME}/.zsh_history' 'HISTSIZE=2000' \
        'SAVEHIST=1000' >> /etc/zsh/zshrc

# The remote shell integration's login shells: bt-shell-common's
# bootstrap tests run sshd's `"$SHELL" -c '<command>'` with each login shell
# the bootstrap handles — zsh (above), bash and dash (the base image), fish
# (integrated through `vendor_conf.d`) and BusyBox (the plain-shell arm, its
# applets standing in for a whole server's tools). Test packages only.
RUN apt-get update \
    && apt-get install -y --no-install-recommends fish busybox \
    && rm -rf /var/lib/apt/lists/*

# clippy from the compiler's own release; a no-op if the image already has it.
RUN rustup component add clippy

WORKDIR /w
