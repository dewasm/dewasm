#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# wasm3: a WebAssembly interpreter written in C, built from the pinned v0.5.0 source release with zig.
# Upstream ships no wasm32-wasi artifact, so it is compiled locally in the meta-WASI configuration (-Dd_m3HasMetaWASI), which forwards the guest's WASI calls to the outer host: the shape a converted interpreter needs.
# v0.5.0 predates the musttail dispatch that makes current wasm3 master need the tail-call proposal, so this build audits as baseline wasm and interprets with a bounded C stack.
# src/wasm3-meta-wasi-compat.patch carries the two wasi-libc compatibility fixes; its header states what it changes and why.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

WASM3_URL="https://github.com/wasm3/wasm3/archive/refs/tags/v0.5.0.tar.gz"
WASM3_SHA256="b778dd72ee2251f4fe9e2666ee3fe1c26f06f517c3ffce572416db067546536c"
WASM3_DIR="wasm3-0.5.0"
WASM3_COMPAT_PATCH="src/wasm3-meta-wasi-compat.patch"
# The interpreter core plus the libc/meta-WASI/tracer API layers and the CLI main.
# The other two WASI variants (m3_api_wasi.c, m3_api_uvwasi.c) stay out: their defines are off anyway, and excluding them keeps the build's WASI surface unambiguous.
WASM3_SRCS=(
  source/m3_api_libc.c source/m3_api_meta_wasi.c source/m3_api_tracer.c
  source/m3_bind.c source/m3_code.c source/m3_compile.c source/m3_core.c
  source/m3_emit.c source/m3_env.c source/m3_exec.c source/m3_function.c
  source/m3_info.c source/m3_module.c source/m3_parse.c
  platforms/app/main.c
)

wasm3_stamp="cache/wasm3.src-sha256"
wasm3_key="$(printf '%s compat:%s wasm-opt:%s' "$WASM3_SHA256" "$(shasum -a 256 "$WASM3_COMPAT_PATCH" | cut -d' ' -f1)" "$(wasm_opt_version)")"
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
patch -s -p1 -F 0 -d "$tmp/$WASM3_DIR" -i "$PWD/$WASM3_COMPAT_PATCH" || {
  echo "wasm3: $WASM3_COMPAT_PATCH does not apply to the pinned source; regenerate it" >&2
  exit 1
}
echo "wasm3: building wasm3.wasm (zig cc)"
srcs=()
for s in "${WASM3_SRCS[@]}"; do srcs+=("$tmp/$WASM3_DIR/$s"); done
# Flag notes:
# -w              2021-era third-party C under a current clang; the pointer-sign
# and deprecation warnings it trips are upstream's, not ours.
# -mno-*          keep the output inside the suite's baseline feature set (see
# benchmarks/c/build.sh for the per-flag reasons).
# -fomit-frame-pointer -fno-stack-protector
# upstream's own release flags for the WASI build (CMakeLists.txt).
# stack-size      upstream's WASI build links an 8 MiB C stack; guest calls
# recurse on it, so keep upstream's headroom.
# --strip-debug   drops the DWARF wasm-opt cannot parse.
zig_cc_wasi -O3 \
  -Dd_m3HasMetaWASI \
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
