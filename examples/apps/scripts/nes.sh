#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# `nes`: an import-free NES emulator built with `wasi-sdk` from `agnes` at a fixed commit.
# A public-domain example ROM comes with it.
# Shared between the NES example frontends and the deterministic framebuffer-snapshot test.
# This mirrors the DOOM fixture.
#
# One reactor library, `cache/nes.wasm`, wraps `agnes` (`kgabis/agnes`) with our `src/nes_demo.c`.
# nes_demo.c provides `allocRom`/`initGame`/`setInput`/`tickGame` + the frame accessors.
# The host drives it and composes pixels from the palette-index screen buffer and the palette.
# `agnes` keeps that palette internally.
# The example ROM (Shiru's public-domain Alter Ego) lands separately at cache/alter_ego.nes.
# The host copies it into the module via `allocRom`.
#
# `agnes` has no upstream wasm32-wasi build, so it is compiled here.
# Its two files are fetched as raw blobs, each checked against its own fixed checksum.
# Those are more stable than a tarball `codeload` builds on the fly.
# agnes.c is not a separate translation unit: nes_demo.c #includes it.
# That is what lets the frame accessors address the internals of `agnes` (issue #117).

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

# `agnes` is fixed at its latest commit.
# agnes.h/agnes.c are fetched as raw blobs, each checked against a fixed SHA-256 checksum.
AGNES_COMMIT="0e4220b084c467e39c04805d955e78c463feadd0"
AGNES_H_URL="https://raw.githubusercontent.com/kgabis/agnes/$AGNES_COMMIT/agnes.h"
AGNES_C_URL="https://raw.githubusercontent.com/kgabis/agnes/$AGNES_COMMIT/agnes.c"
AGNES_H_SHA256="a595b12240134ab46861562303a62f19e0e2796ae2ff0b71b5169d98425d45a1"
AGNES_C_SHA256="2a8ff8770cc4fd1dacaa17b4841e7344fc1e458aba66351ee46e8423e7af618f"

# The example ROM: Alter Ego by Shiru, released into the public domain.
# The `.zip` carries the `.nes` alongside label art / a manual; only the ROM is extracted.
# shiru.untergrund.net stopped answering; the whole untergrund.net host refuses connections.
# So the fetch falls back to the Wayback snapshot below, whose bytes match ROM_SHA256.
# The author's page has no other live home.
# Keep the original first, so a returning host is used again.
ROM_URL="https://shiru.untergrund.net/files/nes/alter_ego.zip"
ROM_MIRROR_URL="https://web.archive.org/web/20241206185447id_/http://shiru.untergrund.net/files/nes/alter_ego.zip"
ROM_SHA256="c7dc651d06aa7aee830d7c1d4563c9347bd724ad9de44dde7f459090b466cdc8"
ROM_MEMBER="alter_ego/Alter_Ego.nes"

# The reactor export surface (`src/nes_demo.c`).
# Zero wasm imports is the goal, so `agnes`/`wasi-libc` must pull nothing in.
# `wasm-objdump` verifies that below.
NES_EXPORTS=(
  allocRom initGame setInput tickGame screenOffset paletteOffset
  frameWidth frameHeight
)

# The stamp covers the source and ROM checksums, the export list, and the `wasm-opt` version.
# It also covers the toolchain token, so editing any of them retriggers the build.
nes_key="agnes:$AGNES_COMMIT h:$AGNES_H_SHA256 c:$AGNES_C_SHA256 rom:$ROM_SHA256 exports:${NES_EXPORTS[*]} wasm-opt:$(wasm_opt_version) $(wasi_sdk_stamp)"
nes_stamp="cache/nes.src-sha256"
if is_cached "$nes_stamp" "$nes_key" cache/nes.wasm cache/alter_ego.nes; then
  echo "nes: cached"
  exit 0
fi

require_wasi_sdk nes
require_tool nes unzip
require_tool nes wasm-opt "install binaryen (e.g. brew install binaryen) to preprocess the nes app"
require_tool nes wasm-dis "install binaryen (e.g. brew install binaryen) to verify the nes import section"

echo "nes: fetching $ROM_URL"
new_tmpdir
fetch_verified "$ROM_URL" "$ROM_SHA256" "$tmp/alter_ego.zip" "$ROM_MIRROR_URL"
archive_extract_file "$tmp/alter_ego.zip" zip "$ROM_MEMBER" cache/alter_ego.nes

echo "nes: fetching agnes ($AGNES_COMMIT)"
fetch_verified "$AGNES_H_URL" "$AGNES_H_SHA256" "$tmp/agnes.h"
fetch_verified "$AGNES_C_URL" "$AGNES_C_SHA256" "$tmp/agnes.c"

echo "nes: building nes.wasm (wasi-sdk clang, reactor)"
mapfile -t exports < <(wl_exports "${NES_EXPORTS[@]}")
# --strip-debug drops the DWARF `wasm-opt` cannot parse.
# `-I $tmp` lets nes_demo.c find the fetched agnes.c/agnes.h.
# nes_demo.c #includes them rather than linking them as a separate TU.
wasi_sdk_clang -O2 -mexec-model=reactor -Wl,--strip-debug \
  -I "$tmp" \
  src/nes_demo.c \
  "${exports[@]}" \
  -o cache/nes.wasm

echo "nes: wasm-opt -O2"
wasm_opt_inplace cache/nes.wasm

# Import-free is a load-bearing property: the snapshot oracle provides no imports.
# So fail loud if `agnes`/`wasi-libc` pulled anything in.
# `wasm-dis` ships with Binaryen, which the build already requires for `wasm-opt`.
if wasm-dis cache/nes.wasm | grep -q '^ (import '; then
  echo "nes: nes.wasm has wasm imports (expected none):" >&2
  wasm-dis cache/nes.wasm | grep '^ (import ' >&2
  exit 1
fi

write_stamp "$nes_stamp" "$nes_key"
echo "nes: -> cache/nes.wasm, cache/alter_ego.nes"
