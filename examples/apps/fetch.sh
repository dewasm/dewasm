#!/usr/bin/env bash
# Fetch real-world example apps as wasm binaries, each from its own
# upstream (a registry tarball, or a project's own release asset).
#
# Third-party artifacts are never committed to this repository (ADR-9):
# this script downloads version-pinned, checksum-verified files into
# examples/apps/cache/ (gitignored). The apps e2e test fails loudly when
# the cache is absent (ADR-15).
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p cache

# name | URL | sha256 | path inside the tarball (empty = URL is the .wasm itself)
APPS=(
  "cowsay|https://cdn.wasmer.io/packages/syrusakbary/cowsay/cowsay-0.3.0-b185348b-2e15-480b-96ac-216064a85e0d.tar.gz|44c990f3ceec797d6e90f54e2ba72789b9544be61ee4011aa7ac6c05252ca605|target/wasm32-wasi/release/cowsay.wasm"
  "qjs|https://github.com/quickjs-ng/quickjs/releases/download/v0.15.1/qjs-wasi.wasm|b4071ef2fbb2bb693c0bbcfc07cb9d28639fd9cea2fd986824a57aeac929817b|"
  "sqlite|https://cdn.wasmer.io/packages/_/sqlite/sqlite-0.2.2.tar.gz|93d4c1f1b3625c311b431076fe071fa1a111472520fbcffd934fafee5e7cc2ed|build/sqlite.wasm"
)

for app in "${APPS[@]}"; do
  IFS="|" read -r name url sha256 wasm_path <<<"$app"
  out="cache/$name.wasm"
  # The stamp records which pinned source checksum the cached copy came
  # from; a re-pinned app is refetched instead of silently kept stale
  # (which would fail the golden-output comparison inscrutably).
  stamp="cache/$name.src-sha256"
  if [ -f "$out" ] && [ "$(cat "$stamp" 2>/dev/null || true)" = "$sha256" ]; then
    echo "$name: cached"
    continue
  fi
  if [ -f "$out" ]; then
    echo "$name: cached copy predates the current pin — refetching"
  fi
  echo "$name: fetching $url"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  if [ -z "$wasm_path" ]; then
    # URL is a standalone .wasm asset (e.g. a GitHub release) — no archive.
    curl -fsSL -o "$tmp/app.wasm" "$url"
    echo "$sha256  $tmp/app.wasm" | shasum -a 256 -c - >/dev/null
    cp "$tmp/app.wasm" "$out"
  else
    curl -fsSL -o "$tmp/pkg.tar.gz" "$url"
    echo "$sha256  $tmp/pkg.tar.gz" | shasum -a 256 -c - >/dev/null
    tar xzf "$tmp/pkg.tar.gz" -C "$tmp"
    cp "$tmp/$wasm_path" "$out"
  fi
  rm -rf "$tmp"
  trap - EXIT
  printf '%s\n' "$sha256" >"$stamp"
  echo "$name: -> $out"
done
