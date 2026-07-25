#!/usr/bin/env bash
# Fetch the *candidate* apps of the 0.1 reset (ADR-24) — binaries that
# passed the feature audit (docs/apps-audit.md) but are not yet wired into
# the apps e2e gate. When one graduates (its cases + goldens land), its
# entry moves into fetch.sh.
#
# Same rules as fetch.sh: version-pinned, checksum-verified, cached into
# the gitignored examples/apps/cache/, never committed (ADR-9).
#
# Note for the e2e wiring (Phase 5): both runtimes also need their stdlib
# trees from the same archives at run time (cpython: lib/python3.14,
# ruby: usr/local/lib/ruby) via a preopened directory — extract those
# alongside the .wasm when the cases land; the audit only needs the module.
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p cache

fetch() {
  local name="$1" url="$2" sha256="$3" archive="$4" wasm_path="$5"
  local out="cache/$name.wasm" stamp="cache/$name.src-sha256"
  if [ -f "$out" ] && [ "$(cat "$stamp" 2>/dev/null || true)" = "$sha256" ]; then
    echo "$name: cached"
    return
  fi
  [ -f "$out" ] && echo "$name: cached copy predates the current pin — refetching"
  echo "$name: fetching $url"
  local tmp
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' RETURN
  curl -fsSL -o "$tmp/pkg" "$url"
  echo "$sha256  $tmp/pkg" | shasum -a 256 -c - >/dev/null
  case "$archive" in
    zip)
      command -v unzip >/dev/null || { echo "$name: unzip not found" >&2; exit 1; }
      unzip -q "$tmp/pkg" "$wasm_path" -d "$tmp"
      cp "$tmp/$wasm_path" "$out"
      ;;
    tar.gz)
      tar xzf "$tmp/pkg" -C "$tmp" "$wasm_path"
      cp "$tmp/$wasm_path" "$out"
      ;;
  esac
  printf '%s\n' "$sha256" >"$stamp"
  echo "$name: -> $out"
}

# CPython 3.14.6, official wasm32-wasip1 build (audit: baseline only).
fetch cpython \
  "https://github.com/brettcannon/cpython-wasi-build/releases/download/v3.14.6/python-3.14.6-wasi_sdk-24.zip" \
  "73bf2e9774c4d8820d0877ec5db0b963df3a9611fc2a63838aeaee29dfd034e6" \
  zip python.wasm

# CRuby 3.4, official ruby.wasm wasm32-wasip1 full build (audit: baseline only).
fetch ruby \
  "https://github.com/ruby/ruby.wasm/releases/download/2.9.4/ruby-3.4-wasm32-unknown-wasip1-full.tar.gz" \
  "ccda86a375a4fe09849846d3b03a370172a4902a0c571087f48457388a2762c7" \
  tar.gz ruby-3.4-wasm32-unknown-wasip1-full/usr/local/bin/ruby
