#!/usr/bin/env bash

# Compile the C microbenchmarks into `benchmarks/cache/c/<id>.wasm` with `wasi-sdk` Clang.
# So every runner in the suite consumes byte-identical modules.
#
# The `.c` sources are checked in; the built `.wasm` is not, like everything else under `cache/`.
# Run this after editing one, or via `benchmarks/setup.sh`.
# That script calls it together with the `.wat` family's `benchmarks/wat/build.sh`.
#
# Idempotent, and cheap enough that it just rebuilds unconditionally.

set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p cache/c

# Fail loudly with an actionable message rather than half-building.
if [ -z "${WASI_SDK_PATH-}" ] || [ ! -x "$WASI_SDK_PATH/bin/clang" ]; then
  echo "benchmarks/c/build.sh: WASI_SDK_PATH does not point at a wasi-sdk root: run \`mise install\` in a shell where mise is active, or download the wasi-sdk version mise.toml states (https://github.com/WebAssembly/wasi-sdk/releases) and set WASI_SDK_PATH to its root, as examples/apps/setup.sh also needs" >&2
  exit 1
fi

# --- C microbenchmark flags.
# Each one is required.
#
# `-nostdlib`: the microbenchmarks define their own `_start` and use no `libc`.
#   Linking `wasi-libc`'s stdio would import `fd_seek` and `fd_close`.
#   `wardite` does not implement those two.
#   Imports are resolved at instantiation, so the module would not even load there.
# `-mno-*`: keep the output inside the feature set every runner in the suite handles.
#   `bulk-memory` and `bulk-memory-opt` are separate LLVM features, and both must be off.
#   Otherwise Clang lowers array zeroing to `memory.fill`.
#   `nontrapping-fptoint` matters because `wardite` mishandles NaN in i32/i64.trunc_sat.
#   `wardite` does not implement `multivalue` and `reference-types` at all.
# `-z stack-size`: the default leaves a 16 MiB shadow stack, which forces a 16 MiB initial memory.
#   That is a real cost for interpreters that back linear memory with a host byte array.
#   64 KiB is more than enough here.
# `--no-wasm-opt`: with `wasm-opt` on PATH, the driver would otherwise run it on the linked module.
#   That uncontrolled pass could reintroduce what the `-mno-*` flags disable.
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
