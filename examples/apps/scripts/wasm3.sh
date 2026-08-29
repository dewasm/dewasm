#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# wasm3: a WebAssembly interpreter written in C, built from the pinned v0.9.0 source release with zig.
# The official wasm3-wasi.wasm release asset needs the tail-call proposal (the default dispatch is a musttail chain), which is outside the accepted input, so it is compiled locally with that dispatch off; the build then audits as baseline wasm.
# The meta-WASI configuration (-Dd_m3HasMetaWASI) forwards the guest's WASI calls to the outer host: the shape a converted interpreter needs.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

WASM3_URL="https://github.com/wasm3/wasm3/archive/refs/tags/v0.9.0.tar.gz"
WASM3_SHA256="cab79ce74bcac25bbf80b5ebe14af9795b9bac30b05ee8f620a3bc8002f3b8e6"
WASM3_DIR="wasm3-0.9.0"
# The interpreter core plus the libc/meta-WASI/tracer API layers and the CLI main.
# The other two WASI variants (m3_api_wasi.c, m3_api_uvwasi.c) stay out: their defines are off anyway, and excluding them keeps the build's WASI surface unambiguous.
WASM3_SRCS=(
  source/m3_api_libc.c source/m3_api_meta_wasi.c source/m3_api_tracer.c
  source/m3_bind.c source/m3_code.c source/m3_compile.c source/m3_core.c
  source/m3_env.c source/m3_exec.c source/m3_function.c
  source/m3_info.c source/m3_module.c source/m3_parse.c source/m3_validate.c
  platforms/app/main.c
)

wasm3_stamp="cache/wasm3.src-sha256"
wasm3_key="$(printf '%s wasm-opt:%s' "$WASM3_SHA256" "$(wasm_opt_version)")"
if is_cached "$wasm3_stamp" "$wasm3_key" cache/wasm3.wasm; then
  echo "wasm3: cached"
  exit 0
fi

require_tool wasm3 zig "install zig (e.g. brew install zig) to build the wasm3 app"
require_tool wasm3 wasm-opt "install binaryen (e.g. brew install binaryen) to preprocess the wasm3 app"

echo "wasm3: fetching $WASM3_URL"
new_tmpdir
fetch_verified "$WASM3_URL" "$WASM3_SHA256" "$tmp/wasm3.tar.gz"
tar xzf "$tmp/wasm3.tar.gz" -C "$tmp"
echo "wasm3: building wasm3.wasm (zig cc)"
srcs=()
for s in "${WASM3_SRCS[@]}"; do srcs+=("$tmp/$WASM3_DIR/$s"); done
# Flag notes:
# -DM3_HAS_TAIL_CALL=0
# the default dispatch is `M3_MUSTTAIL return nextOpImpl()`, which
# wasm32 can only compile with the tail-call proposal; with the knob
# off the chain compiles to plain calls whose frames unwind at loops
# and returns, so flat guest loops interpret in bounded stack
# (measured: one million wat/i32_alu iterations complete under
# wasmtime's default stack limits).
# -w              keep upstream's warnings out of the build output;
# they are third-party C's, not ours.
# -mno-*          keep the output inside the suite's baseline feature set (see
# benchmarks/c/build.sh for the per-flag reasons).
# -fomit-frame-pointer -fno-stack-protector
# upstream's own release flags for the WASI build (CMakeLists.txt).
# stack-size      upstream's WASI build links an 8 MiB C stack; guest calls
# recurse on it, so keep upstream's headroom.
# --strip-debug   drops the DWARF wasm-opt cannot parse.
zig_cc_wasi -O3 \
  -Dd_m3HasMetaWASI -DM3_HAS_TAIL_CALL=0 \
  -I "$tmp/$WASM3_DIR/source" -w \
  -mno-bulk-memory -mno-bulk-memory-opt -mno-nontrapping-fptoint -mno-multivalue -mno-reference-types \
  -fomit-frame-pointer -fno-stack-protector \
  -Wl,-z,stack-size=8388608 -Wl,--strip-debug \
  "${srcs[@]}" \
  -o cache/wasm3.wasm
echo "wasm3: wasm-opt -O2"
wasm_opt_inplace cache/wasm3.wasm

write_stamp "$wasm3_stamp" "$wasm3_key"
echo "wasm3: -> cache/wasm3.wasm"
