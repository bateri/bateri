# The image `make guest-linux` builds the demo program in: a Rust toolchain and the static (musl)
# target of the architecture the container is, so the binary opens on any Linux server. One image
# per platform (`docker build --platform`), nothing else installed.
FROM rust:1.88-bookworm

RUN rustup target add "$(uname -m)-unknown-linux-musl"

WORKDIR /w
