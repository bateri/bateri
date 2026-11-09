# The image of `make linux-server`: what bateri runs on a server, tested in a
# Linux shell with Linux's tools. The Makefile names the image after the tag
# below; the version lives only on this line.
FROM rust:1.99-bookworm

# The login shells a server may have, each one a test's `"$SHELL" -c '<command>'`:
# zsh, fish and BusyBox (its applets standing in for a whole server's tools),
# with bash and dash from the base image. LANG: the image defaults to the C
# locale, where zsh does not recognise multibyte characters.
#
# The tests' crate stands on the app's crates, so these have to build, and
# bt-atlas's Linux backend links FreeType and fontconfig: pkg-config +
# libfreetype-dev + libfontconfig-dev for freetype-sys and
# yeslogic-fontconfig-sys, zlib1g-dev so libz-sys finds the system zlib.
RUN apt-get update \
    && apt-get install -y --no-install-recommends zsh fish busybox \
        pkg-config libfreetype-dev libfontconfig-dev zlib1g-dev \
    && rm -rf /var/lib/apt/lists/*
ENV LANG=C.UTF-8

WORKDIR /w
