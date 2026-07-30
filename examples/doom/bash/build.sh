#!/usr/bin/env bash
# Regenerate the dewasm-generated DOOM library and syntax-check the
# frontend. doom_gen.sh is ~19MB of generated code and is gitignored, so
# this step has to run before main.sh can source it from a clean checkout.
# There's no compile step for bash; `bash -n` stands in for one.
set -euo pipefail

if (( BASH_VERSINFO[0] < 5 )); then
  echo "doom (bash): requires bash >= 5; found ${BASH_VERSION}. On macOS /bin/bash is 3.2 -- install a newer one (e.g. \`brew install bash\`) and run this script with it." >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")"

repo_root="$(cd ../../.. && pwd)"

../fetch.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/doom/cache/doom.wasm \
    --target bash --mode library --module-name doom \
    -o examples/doom/bash/doom_gen.sh
)

bash -n main.sh
bash -n doom_gen.sh

echo "build complete: doom_gen.sh regenerated"
