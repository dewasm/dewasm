#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# `ripgrep`: built from the source release at a fixed version with `cargo` for wasm32-wasip1.
# Default features, which already exclude pcre2.
# No changes are needed: `ripgrep` 15.2.0 builds clean for wasip1 as-is.
# A Ruby-only file system example (recursive directory search over a preopened tree).

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

RG_URL="https://github.com/BurntSushi/ripgrep/archive/refs/tags/15.2.0.tar.gz"
RG_SHA256="7605249d3eb0d5f170e3414498e3344e26b1e7a147aec518b57090b80036a562"
RG_DIR="ripgrep-15.2.0"

rg_stamp="cache/rg.src-sha256"
rg_want="$(printf '%s\n%s' "$RG_SHA256" "$(wasm_opt_version)")"
if is_cached "$rg_stamp" "$rg_want" cache/rg.wasm; then
  echo "rg: cached"
  exit 0
fi

require_tool rg cargo "install the Rust toolchain to build ripgrep"
require_tool rg wasm-opt "install binaryen (e.g. brew install binaryen) to preprocess ripgrep"
rustup target list --installed 2>/dev/null | grep -qx wasm32-wasip1 || {
  echo "rg: wasm32-wasip1 target not installed. Run: rustup target add wasm32-wasip1" >&2
  exit 1
}

echo "rg: fetching $RG_URL"
new_tmpdir
fetch_verified "$RG_URL" "$RG_SHA256" "$tmp/rg.tar.gz"
tar xzf "$tmp/rg.tar.gz" -C "$tmp"
echo "rg: building rg.wasm (cargo build --release --target wasm32-wasip1)"
( cd "$tmp/$RG_DIR" && cargo build --release --target wasm32-wasip1 )
cp "$tmp/$RG_DIR/target/wasm32-wasip1/release/rg.wasm" cache/rg.wasm
echo "rg: wasm-opt -O2"
wasm_opt_inplace cache/rg.wasm

write_stamp "$rg_stamp" "$rg_want"
echo "rg: -> cache/rg.wasm"
