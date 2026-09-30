#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# `zeroperl`: a WASI reactor build of Perl 5.42 (`6over3/zeroperl`).
# The organization `6over3` was renamed from `uswriting`.
# The source repository cuts no releases/tags.
# So the distribution that can be fetched at a fixed version is an `npm` wrapper.
# That wrapper is `@6over3/zeroperl-ts`, which ships the prebuilt zeroperl.wasm under `dist/esm/`.
# Licensing: the `zeroperl` source (`github.com/6over3/zeroperl`) is MIT.
# The `npm` package `zeroperl-ts` that redistributes the wasm is Apache-2.0.
#
# `fetch_app` verifies the tarball checksum and extracts the inner wasm.
# It does not check the extracted file's own checksum, so we add that check here.
# For a 25 MB module, a silent partial extract would cause an unclear convert failure downstream.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

fetch_app zeroperl \
  "https://registry.npmjs.org/@6over3/zeroperl-ts/-/zeroperl-ts-1.0.10.tgz" \
  c46c5ffff4f2c2216137a6b34b4eb03f5554e17a2b88979463c5f61dffb36fed \
  package/dist/esm/zeroperl.wasm

echo "a6ae97f184fabc444b0b70d77c3d16475bfb3de16ebfb58ab8b2fbf98a53835e  cache/zeroperl.wasm" \
  | shasum -a 256 -c - >/dev/null
