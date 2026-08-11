#!/usr/bin/env bash
# Build (if needed) and run the NES frontend, forwarding any arguments (e.g.
# `./run.sh --smoke` for the headless self-check, or a ROM path).
set -euo pipefail

if (( BASH_VERSINFO[0] < 5 )); then
  echo "nes (bash): requires bash >= 5; found ${BASH_VERSION}. On macOS /bin/bash is 3.2. Install a newer one (e.g. \`brew install bash\`) and run this script with it." >&2
  exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")"

./build.sh
exec bash main.sh "$@"
