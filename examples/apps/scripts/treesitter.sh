#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# tree-sitter: the incremental-parsing runtime plus the tree-sitter-json grammar, built from the pinned upstream releases with zig as a reactor library.
# The runtime is a single-TU amalgamation (lib/src/lib.c);
# tree-sitter-json ships a pre-generated src/parser.c (no grammar codegen).
# Our own src/treesitter_binding.c exports parse_source(), which parses a source string and returns the parse tree's S-expression (ts_node_string).
# One combined stamp covers both source checksums.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

TS_URL="https://github.com/tree-sitter/tree-sitter/archive/refs/tags/v0.26.11.tar.gz"
TS_SHA256="1bab01ed21464f3272665b9c60e39ee79f68da1333e80b23f2c9356569d06971"
TS_DIR="tree-sitter-0.26.11"
TSJSON_URL="https://github.com/tree-sitter/tree-sitter-json/archive/refs/tags/v0.24.8.tar.gz"
TSJSON_SHA256="acf6e8362457e819ed8b613f2ad9a0e1b621a77556c296f3abea58f7880a9213"
TSJSON_DIR="tree-sitter-json-0.24.8"

# One stamp covering both pinned checksums (order-fixed) plus wasm-opt version.
ts_stamp="cache/treesitter.src-sha256"
ts_want="$(printf '%s %s\n%s' "$TS_SHA256" "$TSJSON_SHA256" "$(wasm_opt_version)")"
if is_cached "$ts_stamp" "$ts_want" cache/treesitter.wasm; then
  echo "treesitter: cached"
  exit 0
fi

require_tool treesitter zig "install zig (e.g. brew install zig) to build the tree-sitter app"
require_tool treesitter wasm-opt "install binaryen (e.g. brew install binaryen) to preprocess the tree-sitter app"

echo "treesitter: fetching $TS_URL"
new_tmpdir
fetch_verified "$TS_URL" "$TS_SHA256" "$tmp/ts.tar.gz"
echo "treesitter: fetching $TSJSON_URL"
fetch_verified "$TSJSON_URL" "$TSJSON_SHA256" "$tmp/tsjson.tar.gz"
tar xzf "$tmp/ts.tar.gz" -C "$tmp"
tar xzf "$tmp/tsjson.tar.gz" -C "$tmp"
echo "treesitter: building treesitter.wasm (zig cc, reactor)"
# --strip-debug drops the DWARF wasm-opt cannot process.
zig_cc_wasi -mexec-model=reactor -O2 -Wl,--strip-debug \
  -I "$tmp/$TS_DIR/lib/include" -I "$tmp/$TS_DIR/lib/src" \
  -I "$tmp/$TSJSON_DIR/src" \
  "$tmp/$TS_DIR/lib/src/lib.c" "$tmp/$TSJSON_DIR/src/parser.c" src/treesitter_binding.c \
  -Wl,--export=parse_source -Wl,--export=malloc -Wl,--export=free \
  -o cache/treesitter.wasm
echo "treesitter: wasm-opt -O2"
wasm_opt_inplace cache/treesitter.wasm

write_stamp "$ts_stamp" "$ts_want"
echo "treesitter: -> cache/treesitter.wasm"
