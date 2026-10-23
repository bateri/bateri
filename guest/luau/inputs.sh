#!/bin/sh
# Prints the digest of everything the interpreter module (`crates/bt-view-wasm/luau/luau.wasm`) is
# built from: the interface it speaks, the bridge and the preludes, the build script and its pins,
# the generator of the C bindings and the tool that makes the component. `make luau` writes it
# beside the module (`inputs.sha256`) and `make audit` compares it, so that a module built from
# sources that have changed since is noticed by the gate and not by the first person to run a
# script.
set -eu
cd "$(dirname "$0")/../.."
export LC_ALL=C
{
	for file in crates/bt-view-wasm/wit/view.wit guest/luau/pins guest/luau/build.sh \
		guest/luau/bridge/* guest/luau/prelude/* \
		guest/cbindgen/src/main.rs guest/componentize/src/main.rs; do
		echo "== $file"
		cat "$file"
	done
	# The releases of the generator and the component tool are lines of the workspace manifest.
	echo "== guest/Cargo.toml"
	grep -E '^wit-' guest/Cargo.toml
} | { shasum -a 256 2>/dev/null || sha256sum; } | cut -d' ' -f1
