#!/usr/bin/env bash

# Compile the C microbenchmarks into benchmarks/cache/c/<id>.wasm with zig cc, so that
# every runner in the suite consumes byte-identical modules.
#
# The .c sources are checked in; the built .wasm is not, like everything else
# under cache/. Run this after editing one, or via `benchmarks/setup.sh`,
# which calls it together with the .wat family's benchmarks/wat/build.sh.
#
# Idempotent, and cheap enough that it just rebuilds unconditionally.

set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p cache/c

# Fail loudly with an actionable message rather than half-building.
require_tool() {
  command -v "$1" >/dev/null && return
  echo "benchmarks/c/build.sh: $1 not found — $2" >&2
  exit 1
}

require_tool zig "install zig (brew install zig), as examples/apps/setup.sh also needs"

# --- C microbenchmark flags. Each one is load-bearing.
#
# -nostdlib      The microbenchmarks define their own _start and use no libc. Linking
#                wasi-libc's stdio would import fd_seek and fd_close, which
#                wardite does not implement — and imports are resolved at
#                instantiation, so the module would not even load there.
#                (`-nostartfiles` is silently ignored by zig cc for wasm: crt1
#                still gets linked and _start collides.)
# -mno-*         Keep the output inside the feature set every runner in the
#                suite handles. bulk-memory and bulk-memory-opt are separate
#                LLVM features and both must be off, or clang lowers array
#                zeroing to memory.fill. nontrapping-fptoint matters because
#                wardite mishandles NaN in i32/i64.trunc_sat; multivalue and
#                reference-types it does not implement at all.
# -z stack-size  The default leaves a 16 MiB shadow stack, which forces a
#                16 MiB initial memory — a real cost for interpreters that back
#                linear memory with a host byte array. 64 KiB is ample here.
CFLAGS=(
  -target wasm32-wasi
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
  zig cc "${CFLAGS[@]}" -o "cache/c/$id.wasm" "$src"
  echo "$id: $src -> cache/c/$id.wasm"
done
