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

# archive_extract_file <archive> <kind> <inner> <dest>: extract the single
# member <inner> from a .zip / .tar.gz to <dest>. Goes through stdout
# (unzip -p / tar -O), so only that one member is unpacked — no scratch tree,
# and crucially none of the bulk (cruby's tarball carries a multi-hundred-MB
# static lib we never want on disk).
archive_extract_file() {
  case "$2" in
    zip) unzip -p "$1" "$3" >"$4" ;;
    tar.gz) tar xzOf "$1" "$3" >"$4" ;;
  esac
}

# archive_extract_tree <archive> <kind> <inner> <destdir> [strip]: extract the
# subtree <inner> from a .zip / .tar.gz into a fresh <destdir>, dropping <strip>
# leading path components (so it lands at <destdir>/<inner-minus-strip>). Only
# <inner> is unpacked, not the whole archive. zip has no strip flag, so the zip
# path only supports strip=0 (all it is asked for); a strip>0 zip would need
# extend-and-relocate, added when something needs it (ADR-0: fail, don't guess).
archive_extract_tree() {
  local ar="$1" kind="$2" inner="$3" dest="$4" strip="${5:-0}"
  rm -rf "$dest"
  mkdir -p "$dest"
  case "$kind" in
    tar.gz)
      tar xzf "$ar" -C "$dest" --strip-components="$strip" "$inner"
      ;;
    zip)
      [ "$strip" = 0 ] || {
        echo "archive_extract_tree: zip with strip=$strip is unimplemented" >&2
        exit 1
      }
      unzip -qo "$ar" "$inner/*" -d "$dest"
      ;;
  esac
}

# fetch_runtime_with_stdlib <name> <url> <sha256> <wasm_inner> <tree_inner> [strip]
# The prebuilt language-runtime pattern (cpython, cruby): a checksum-pinned
# archive from which we take the interpreter binary (<wasm_inner> -> cache/
# <name>.wasm) plus the stdlib tree it reads at startup (<tree_inner>, minus
# <strip> leading components -> cache/<name>-lib/). The archive kind is inferred
# from the URL, so callers stay format-agnostic. Stamp = the sha; skips on a
# cache hit (the wasm and the stdlib-tree marker present, stamp matching).
fetch_runtime_with_stdlib() {
  local name="$1" url="$2" sha256="$3" wasm_inner="$4" tree_inner="$5" strip="${6:-0}"
  local wasm="cache/$name.wasm" lib="cache/$name-lib"
  # Marker = where the stdlib tree lands: tree_inner with `strip` leading
  # components removed, under cache/<name>-lib/.
  local stripped="$tree_inner" n="$strip"
  while ((n-- > 0)); do stripped="${stripped#*/}"; done
  local marker="$lib/$stripped" stamp="cache/$name.src-sha256"
  local kind
  case "$url" in
    *.zip) kind=zip ;;
    *.tar.gz | *.tgz) kind=tar.gz ;;
    *)
      echo "$name: unrecognized archive type in $url" >&2
      exit 1
      ;;
  esac
  if is_cached "$stamp" "$sha256" "$wasm" "$marker"; then
    echo "$name: cached"
    return
  fi
  [ "$kind" = zip ] && require_tool "$name" unzip
  echo "$name: fetching $url"
  new_tmpdir
  local ar="$tmp/archive"
  fetch_verified "$url" "$sha256" "$ar"
  archive_extract_file "$ar" "$kind" "$wasm_inner" "$wasm"
  archive_extract_tree "$ar" "$kind" "$tree_inner" "$lib" "$strip"
  write_stamp "$stamp" "$sha256"
  echo "$name: -> $wasm, $marker"
}
