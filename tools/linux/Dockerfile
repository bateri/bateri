# `make linux`'un imajı: bt-core'u ve bt-atlas'ı Linux'ta derleyip sınamak
# için tek tarif.
#
# Taban etiketinin major.minor'ü yerel rustc'yle AYNI olmak zorunda — `make
# linux` önce bunu karşılaştırıyor ve uyuşmazlıkta kırmızı düşüyor: iki farklı
# derleyicinin clippy'si iki farklı kapı demek (Homebrew rustc pin'li değil,
# `brew upgrade` sonrası bu satır da güncellenir). Etiketi Makefile buradan
# okuyor; sürüm yalnız bu satırda yaşıyor.
FROM rust:1.88-bookworm

# zsh: bt-core'un ayna sınamaları gerçek bir zsh koşturuyor. LANG: imajın
# varsayılanı C yereli ve zsh orada çok baytlı karakteri tanımıyor — ASCII
# olmayan harf aynada `??` çıkıyor ve üç ayna sınaması ortam yüzünden düşüyor
# (ölçüm `.tasks/040-linux-kapisi-ve-wgpu/context.md` → Kanıt).
#
# The font stack of bt-atlas's Linux backend (042 Karar 5, 8; user-approved
# packages, record in `.tasks/042-font-sistemi-linux/phase-4.md`):
# pkg-config + libfreetype-dev + libfontconfig-dev are what freetype-sys and
# yeslogic-fontconfig-sys link against dynamically; zlib1g-dev so libz-sys
# finds the system zlib instead of compiling its own. fonts-dejavu-core is the
# only font family: `monospace` resolves to DejaVu Sans Mono and the atlas
# tests' Linux fixture (`bt-atlas/src/freetype.rs`) is measured against it.
# The version check stops the build if FreeType is older than 2.12.1
# (libtool 24.3.18 — 2.12.0 is 24.1.18), the release the backend was
# written against.
RUN apt-get update \
    && apt-get install -y --no-install-recommends zsh \
        pkg-config libfreetype-dev libfontconfig-dev zlib1g-dev fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/* \
    && fc-cache -f \
    && dpkg --compare-versions "$(pkg-config --modversion freetype2)" ge 24.3.18
ENV LANG=C.UTF-8

# clippy derleyicinin kendi sürümünden gelsin; imajda zaten varsa no-op.
RUN rustup component add clippy

WORKDIR /w
