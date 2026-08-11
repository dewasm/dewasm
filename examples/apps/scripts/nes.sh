#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# nes: an import-free NES emulator built from the pinned agnes source with zig, plus a public-domain demo ROM.
# Shared between the NES example frontends and the deterministic framebuffer-snapshot test, mirroring the
# DOOM fixture.
#
# One reactor library, cache/nes.wasm, wraps agnes (kgabis/agnes) with our own src/nes_demo.c (allocRom/initGame/setInput/tickGame + the frame accessors), driven from the host, which composes pixels from the palette-index screen buffer and the palette agnes keeps internally.
# The ROM (Shiru's public-domain
# Alter Ego) lands separately at cache/alter_ego.nes; the host copies it into the module via allocRom.
#
# agnes has no upstream wasm32-wasi build, so it is compiled here.
# Its two files are pinned as individually checksummed raw blobs (stabler than an on-the-fly codeload tarball). agnes.c is not a separate translation unit: nes_demo.c
# #includes it, which is what lets the frame accessors address agnes's internals (issue #117).

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# agnes pinned at its latest commit; agnes.h/agnes.c fetched as raw blobs, each sha256-pinned.
AGNES_COMMIT="0e4220b084c467e39c04805d955e78c463feadd0"
AGNES_H_URL="https://raw.githubusercontent.com/kgabis/agnes/$AGNES_COMMIT/agnes.h"
AGNES_C_URL="https://raw.githubusercontent.com/kgabis/agnes/$AGNES_COMMIT/agnes.c"
AGNES_H_SHA256="a595b12240134ab46861562303a62f19e0e2796ae2ff0b71b5169d98425d45a1"
AGNES_C_SHA256="2a8ff8770cc4fd1dacaa17b4841e7344fc1e458aba66351ee46e8423e7af618f"

# The demo ROM: Alter Ego by Shiru, released into the public domain.
# The zip carries the .nes alongside label art / a manual; only the ROM is extracted.
ROM_URL="https://shiru.untergrund.net/files/nes/alter_ego.zip"
ROM_SHA256="c7dc651d06aa7aee830d7c1d4563c9347bd724ad9de44dde7f459090b466cdc8"
ROM_MEMBER="alter_ego/Alter_Ego.nes"

# The reactor export surface (src/nes_demo.c).
# Zero wasm imports is the goal, so agnes/wasi-libc must pull nothing in, verified below with wasm-objdump.
NES_EXPORTS=(
  allocRom initGame setInput tickGame screenOffset paletteOffset
  frameWidth frameHeight
)

# The stamp covers both source pins, the ROM pin, the export list, and the wasm-opt version, so editing any of them retriggers the build.
nes_key="agnes:$AGNES_COMMIT h:$AGNES_H_SHA256 c:$AGNES_C_SHA256 rom:$ROM_SHA256 exports:${NES_EXPORTS[*]} wasm-opt:$(wasm_opt_version)"
nes_stamp="cache/nes.src-sha256"
if is_cached "$nes_stamp" "$nes_key" cache/nes.wasm cache/alter_ego.nes; then
  echo "nes: cached"
  exit 0
fi

require_tool nes zig "install zig (e.g. brew install zig) to build the nes app"
require_tool nes unzip
require_tool nes wasm-opt "install binaryen (e.g. brew install binaryen) to preprocess the nes app"
require_tool nes wasm-dis "install binaryen (e.g. brew install binaryen) to verify the nes import section"

echo "nes: fetching $ROM_URL"
new_tmpdir
fetch_verified "$ROM_URL" "$ROM_SHA256" "$tmp/alter_ego.zip"
archive_extract_file "$tmp/alter_ego.zip" zip "$ROM_MEMBER" cache/alter_ego.nes

echo "nes: fetching agnes ($AGNES_COMMIT)"
fetch_verified "$AGNES_H_URL" "$AGNES_H_SHA256" "$tmp/agnes.h"
fetch_verified "$AGNES_C_URL" "$AGNES_C_SHA256" "$tmp/agnes.c"

echo "nes: building nes.wasm (zig cc, reactor)"
mapfile -t exports < <(wl_exports "${NES_EXPORTS[@]}")
# --strip-debug drops the DWARF wasm-opt cannot parse; -I $tmp lets nes_demo.c find the fetched agnes.c/agnes.h, which it #includes rather than linking as a separate TU.
zig_cc_wasi -O2 -mexec-model=reactor -Wl,--strip-debug \
  -I "$tmp" \
  src/nes_demo.c \
  "${exports[@]}" \
  -o cache/nes.wasm

echo "nes: wasm-opt -O2"
wasm_opt_inplace cache/nes.wasm

# Import-free is a load-bearing property (the snapshot oracle wires no imports):
# fail loud if agnes/wasi-libc pulled anything in. wasm-dis ships with binaryen, which the build already requires for wasm-opt.
if wasm-dis cache/nes.wasm | grep -q '^ (import '; then
  echo "nes: nes.wasm has wasm imports (expected none):" >&2
  wasm-dis cache/nes.wasm | grep '^ (import ' >&2
  exit 1
fi

write_stamp "$nes_stamp" "$nes_key"
echo "nes: -> cache/nes.wasm, cache/alter_ego.nes"
