#!/usr/bin/env bash
# Regenerate the dewasm-generated NES library and syntax-check the frontend.
# nes_gen.sh is generated code and is gitignored, so this step has to run before main.sh can source it from a clean checkout.
# There's no compile step for bash; `bash -n` stands in for one.
set -euo pipefail

if (( BASH_VERSINFO[0] < 5 )); then
  echo "nes (bash): requires bash >= 5; found ${BASH_VERSION}. On macOS /bin/bash is 3.2 -- install a newer one (e.g. \`brew install bash\`) and run this script with it." >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")"

repo_root="$(cd ../../.. && pwd)"

../../apps/scripts/nes.sh

(
  cd "$repo_root"
  cargo run -q -p dewasm-cli -- \
    examples/apps/cache/nes.wasm \
    --target bash --mode library --module-name nes \
    -o examples/nes/bash/nes_gen.sh
)

bash -n main.sh
bash -n nes_gen.sh

echo "build complete: nes_gen.sh regenerated"
