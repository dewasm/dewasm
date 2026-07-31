#!/usr/bin/env bash
# Shared helpers for the per-app fetch/build scripts in this directory.
#
# Every scripts/<name>.sh sources this file, then either downloads a pinned,
# checksum-verified prebuilt wasm or builds one from a pinned source release
# (ADR-22). Third-party artifacts are never committed (ADR-9): everything
# lands in examples/apps/cache/ (gitignored), and the apps e2e test fails
# loudly when the cache is absent (ADR-15).
#
# Sourcing this file cd's to examples/apps (so cache/ and src/ resolve the
# same way whether a script is run directly or via fetch-and-build.sh) and
# ensures cache/ exists.
set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mkdir -p cache

# Every download goes through fetch_url: the pinned hosts (GitHub,
# sqlite.org, tcpdump.org, wasmer.io) return transient 5xx often enough to
# kill a cold CI run otherwise (issue #17); curl retries 408/429/5xx and
# refused connections.
fetch_url() {
  curl -fsSL --retry 5 --retry-delay 2 --retry-connrefused -o "$2" "$1"
}

# fetch_verified <url> <sha256> <out>: download and fail unless the bytes
# match the pin.
fetch_verified() {
  fetch_url "$1" "$3"
  echo "$2  $3" | shasum -a 256 -c - >/dev/null
}

# require_tool <app> <cmd> [install-hint]: fail loudly (ADR-15) when a build
# prerequisite is missing, naming the app and, when given, how to install it.
require_tool() {
  command -v "$2" >/dev/null && return
  if [ -n "${3:-}" ]; then
    echo "$1: $2 not found — $3" >&2
  else
    echo "$1: $2 not found" >&2
  fi
  exit 1
}

# new_tmpdir: create a scratch dir in $tmp, removed when the script exits.
# Each script is its own process, so the EXIT trap suffices — no manual
# cleanup/trap-reset needed.
new_tmpdir() {
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
}

# is_cached <stamp> <key> <output...>: true when every output exists and the
# stamp matches key. The stamp records which pinned inputs the cached copy
# came from; a re-pin refetches/rebuilds instead of silently keeping a stale
# copy (which would fail the golden comparison inscrutably).
is_cached() {
  local stamp="$1" key="$2"
  shift 2
  local f
  for f in "$@"; do
    [ -e "$f" ] || return 1
  done
  [ "$(cat "$stamp" 2>/dev/null || true)" = "$key" ]
}

# write_stamp <stamp> <key>: record the pin the fresh artifacts came from.
write_stamp() {
  printf '%s\n' "$2" >"$1"
}

# --- wasm-opt -O2 preprocessing (ADR-39) for the locally-built modules.
# Only modules we build here (the reactor C libs and ripgrep) are
# post-processed: it shrinks them and normalizes the overlong call_indirect
# immediates the LLVM toolchain emits (so the converter sees pure baseline
# wasm). Baseline features only — never SIMD/atomics/EH — and no
# wasm-ctor-eval. The stamp for each such module includes `wasm-opt --version`
# so a wasm-opt upgrade re-triggers the build.
WASM_OPT_FEATURES=(
  --enable-bulk-memory --enable-sign-ext --enable-nontrapping-float-to-int
  --enable-mutable-globals --enable-multivalue --enable-reference-types
)
wasm_opt_inplace() {
  wasm-opt "${WASM_OPT_FEATURES[@]}" -O2 "$1" -o "$1"
}
# The version string folded into a locally-built module's stamp (empty when
# wasm-opt is absent, so the cache misses and the loud prereq check fires).
wasm_opt_version() { wasm-opt --version 2>/dev/null || true; }

# wl_exports <sym...>: emit one -Wl,--export=<sym> per line, for mapfile into
# a zig cc argument array.
wl_exports() {
  local e
  for e in "$@"; do printf -- '-Wl,--export=%s\n' "$e"; done
}

# fetch_app <name> <url> <sha256> [wasm_path_in_tarball]: download a pinned
# prebuilt wasm — a standalone .wasm asset when wasm_path is empty, else the
# named entry inside a .tar.gz. Stamp = the source sha.
fetch_app() {
  local name="$1" url="$2" sha256="$3" wasm_path="${4:-}"
  local out="cache/$name.wasm" stamp="cache/$name.src-sha256"
  if is_cached "$stamp" "$sha256" "$out"; then
    echo "$name: cached"
    return
  fi
  [ -f "$out" ] && echo "$name: cached copy predates the current pin — refetching"
  echo "$name: fetching $url"
  new_tmpdir
  if [ -z "$wasm_path" ]; then
    fetch_verified "$url" "$sha256" "$tmp/app.wasm"
    cp "$tmp/app.wasm" "$out"
  else
    fetch_verified "$url" "$sha256" "$tmp/pkg.tar.gz"
    tar xzf "$tmp/pkg.tar.gz" -C "$tmp"
    cp "$tmp/$wasm_path" "$out"
  fi
  write_stamp "$stamp" "$sha256"
  echo "$name: -> $out"
}
