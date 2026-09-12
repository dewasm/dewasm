#!/usr/bin/env bash

# Compile the C microbenchmarks into benchmarks/cache/c/<id>.wasm with wasi-sdk clang, so that every runner in the suite consumes byte-identical modules.
#
# The .c sources are checked in; the built .wasm is not, like everything else under cache/.
# Run this after editing one, or via `benchmarks/setup.sh`, which calls it together with the .wat family's benchmarks/wat/build.sh.
#
# Idempotent, and cheap enough that it just rebuilds unconditionally.

set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p cache/c

# Fail loudly with an actionable message rather than half-building.
if [ -z "${WASI_SDK_PATH-}" ] || [ ! -x "$WASI_SDK_PATH/bin/clang" ]; then
  echo "benchmarks/c/build.sh: WASI_SDK_PATH does not point at a wasi-sdk root: download wasi-sdk (local dev and CI use wasi-sdk-34, https://github.com/WebAssembly/wasi-sdk/releases) and set WASI_SDK_PATH to its root, as examples/apps/setup.sh also needs" >&2
  exit 1
fi

# --- C microbenchmark flags.
# Each one is load-bearing.
#
# -nostdlib      The microbenchmarks define their own _start and use no libc.
# Linking
# wasi-libc's stdio would import fd_seek and fd_close, which
# wardite does not implement, and imports are resolved at
# instantiation, so the module would not even load there.
# -mno-*         Keep the output inside the feature set every runner in the
# suite handles. bulk-memory and bulk-memory-opt are separate
# LLVM features and both must be off, or clang lowers array
# zeroing to memory.fill. nontrapping-fptoint matters because
# wardite mishandles NaN in i32/i64.trunc_sat; multivalue and
# reference-types it does not implement at all.
# -z stack-size  The default leaves a 16 MiB shadow stack, which forces a
# 16 MiB initial memory, a real cost for interpreters that back
# linear memory with a host byte array. 64 KiB is ample here.
# --no-wasm-opt: with a wasm-opt on PATH the driver would otherwise run it on the linked module, an uncontrolled pass that could reintroduce what the -mno-* flags disable.
CFLAGS=(
  --target=wasm32-wasip1
  --no-wasm-opt
  -O2
  -nostdlib
  -mno-bulk-memory
  -mno-bulk-memory-opt
  -mno-nontrapping-fptoint
  -mno-multivalue
  -mno-reference-types
  -Wall
  -Wextra
  -Werror
  # Quoted because the commas are part of the argument, not separators.
  '-Wl,--no-entry'
  '-Wl,--export=_start'
  '-Wl,--strip-all'
  '-Wl,-z,stack-size=65536'
)

for src in c/*.c; do
  id=$(basename "$src" .c)
  "$WASI_SDK_PATH/bin/clang" "${CFLAGS[@]}" -o "cache/c/$id.wasm" "$src"
  echo "$id: $src -> cache/c/$id.wasm"
done
