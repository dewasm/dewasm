#!/usr/bin/env bash
# Fetch the *candidate* apps of the 0.1 reset (ADR-24) — binaries that
# passed the feature audit (docs/apps-audit.md) but are not yet wired into
# the apps e2e gate. When one graduates (its cases + goldens land), its
# entry moves into fetch.sh.
#
# Same rules as fetch.sh: version-pinned, checksum-verified, cached into
# the gitignored examples/apps/cache/, never committed (ADR-9).
#
# There are currently no pending candidates: CPython and CRuby both
# graduated to fetch.sh in Phase 5b (with their stdlib trees extracted for
# the e2e cases). The scaffold below is kept for the next candidate — add a
# `fetch <name> <url> <sha256> <zip|tar.gz> <path-inside-archive>` call, run
# the feature audit on cache/<name>.wasm, and record the verdict in
# docs/apps-audit.md; move the entry here into fetch.sh once its e2e case and
# golden land.
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

echo "no pending candidates (CPython and CRuby graduated to fetch.sh in Phase 5b)"
