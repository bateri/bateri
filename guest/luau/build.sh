#!/bin/sh
# Builds the interpreter module's core: Luau, the bridge and the interface's C bindings, as a
# WebAssembly module (not yet a component: `componentize` is the last step, on the host).
# Runs inside wasi-sdk's container, started by `make luau`:
#
#   build.sh <luau-source> <bindings> <work>
#
# `<luau-source>` is the unpacked release, `<bindings>` what `cbindgen` wrote of the interface,
# `<work>` where the objects and the result (`<work>/luau-core.wasm`) go. This directory
# (`/luau` in the container) is read only.
set -eu

SOURCE=$1
BINDINGS=$2
WORK=$3
HERE=$(dirname "$0")

# C++ exceptions, in the form the runtime speaks (`try_table`, the proposal's final one):
# Luau reports a script's errors, syntax errors included, with them.
EH="-fwasm-exceptions -mllvm -wasm-use-legacy-eh=false"
# Luau's own warnings are Luau's; `clock` is answered by the bridge (it reads zero).
QUIET="-Wno-deprecated-declarations -Wno-unused-template"
SDK=/opt/wasi-sdk

mkdir -p "$WORK/build" "$WORK/objects"

# --- Luau: the virtual machine and the compiler --------------------------------------------------
cmake -S "$SOURCE" -B "$WORK/build" -G Ninja \
	-DCMAKE_TOOLCHAIN_FILE=$SDK/share/cmake/wasi-sdk-p1.cmake -DWASI_SDK_PREFIX=$SDK \
	-DCMAKE_BUILD_TYPE=Release -DLUAU_BUILD_CLI=OFF -DLUAU_BUILD_TESTS=OFF \
	-DCMAKE_C_FLAGS="$EH" -DCMAKE_CXX_FLAGS="$EH $QUIET" \
	-DCMAKE_C_FLAGS_RELEASE="-Os -DNDEBUG" -DCMAKE_CXX_FLAGS_RELEASE="-Os -DNDEBUG" >"$WORK/configure.log" 2>&1 \
	|| { cat "$WORK/configure.log"; exit 1; }
cmake --build "$WORK/build" --target Luau.VM Luau.Compiler >"$WORK/build.log" 2>&1 \
	|| { cat "$WORK/build.log"; exit 1; }

# --- The preludes, as C ----------------------------------------------------------------------------
# Each file of `prelude/` in the order of its name, as bytes (a script's text is data, whatever
# it holds).
{
	echo "// Written by build.sh."
	echo "struct PreludeChunk { const char* name; const char* source; unsigned long length; };"
	index=0
	for file in "$HERE"/prelude/*.lua; do
		echo "static const unsigned char prelude_$index[] = {"
		od -An -v -tu1 "$file" | tr -s ' \n' '\n\n' | sed '/^$/d' | paste -sd, -
		echo "};"
		index=$((index + 1))
	done
	echo "static const PreludeChunk kPrelude[] = {"
	index=0
	for file in "$HERE"/prelude/*.lua; do
		echo "	{\"prelude/$(basename "$file")\", (const char*)prelude_$index, sizeof prelude_$index},"
		index=$((index + 1))
	done
	echo "};"
} >"$WORK/prelude.inc"

# --- The bridge and the bindings, linked with Luau -------------------------------------------------
INCLUDES="-I$SOURCE/VM/include -I$SOURCE/Compiler/include -I$BINDINGS -I$WORK -I$HERE/bridge"
$SDK/bin/clang --target=wasm32-wasip1 -Os -c "$BINDINGS/view.c" -I"$BINDINGS" -o "$WORK/objects/view.o"
for file in bridge convert; do
	$SDK/bin/clang++ --target=wasm32-wasip1 $EH -Os -std=c++17 -Wall -Wextra $INCLUDES \
		-c "$HERE/bridge/$file.cpp" -o "$WORK/objects/$file.o"
done
# `--stack-first` puts the stack below the data, so that a stack that overflows runs off the
# start of memory (a trap) and not over what the module keeps.
$SDK/bin/clang++ --target=wasm32-wasip1 $EH -Wno-unused-command-line-argument -Os -mexec-model=reactor \
	"$WORK/objects/bridge.o" "$WORK/objects/convert.o" "$WORK/objects/view.o" "$BINDINGS/view_component_type.o" \
	"$WORK/build/libLuau.Compiler.a" "$WORK/build/libLuau.Ast.a" "$WORK/build/libLuau.Bytecode.a" \
	"$WORK/build/libLuau.VM.a" "$WORK/build/libLuau.Common.a" \
	-lunwind -Wl,--stack-first -Wl,-z,stack-size=1048576 -Wl,--strip-debug \
	-o "$WORK/luau-core.wasm"
