# `make linux`'un imajı: bt-core'u Linux'ta derleyip sınamak için tek tarif.
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
RUN apt-get update \
    && apt-get install -y --no-install-recommends zsh \
    && rm -rf /var/lib/apt/lists/*
ENV LANG=C.UTF-8

# clippy derleyicinin kendi sürümünden gelsin; imajda zaten varsa no-op.
RUN rustup component add clippy

WORKDIR /w
