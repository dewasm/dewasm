#!/usr/bin/env bash
# Fetch real-world example apps as wasm binaries from the Wasmer registry.
#
# Third-party artifacts are never committed to this repository (ADR-9):
# this script downloads version-pinned, checksum-verified packages into
# examples/apps/cache/ (gitignored). The apps e2e test self-skips when the
# cache is absent.
set -euo pipefail

cd "$(dirname "$0")"
mkdir -p cache

# name | tarball URL | tarball sha256 | wasm path inside the tarball
APPS=(
  "cowsay|https://cdn.wasmer.io/packages/syrusakbary/cowsay/cowsay-0.3.0-b185348b-2e15-480b-96ac-216064a85e0d.tar.gz|44c990f3ceec797d6e90f54e2ba72789b9544be61ee4011aa7ac6c05252ca605|target/wasm32-wasi/release/cowsay.wasm"
  "qjs|https://cdn.wasmer.io/packages/_/quickjs/quickjs-0.0.3.tar.gz|8f2614b6efcf1c47923f0ce030da11622c1d72b8d6329653bc2cacb5b78e8bfb|build/qjs.wasm"
)

for app in "${APPS[@]}"; do
  IFS="|" read -r name url sha256 wasm_path <<<"$app"
  out="cache/$name.wasm"
  if [ -f "$out" ]; then
    echo "$name: cached"
    continue
  fi
  echo "$name: fetching $url"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  curl -fsSL -o "$tmp/pkg.tar.gz" "$url"
  echo "$sha256  $tmp/pkg.tar.gz" | shasum -a 256 -c - >/dev/null
  tar xzf "$tmp/pkg.tar.gz" -C "$tmp"
  cp "$tmp/$wasm_path" "$out"
  rm -rf "$tmp"
  trap - EXIT
  echo "$name: -> $out"
done
