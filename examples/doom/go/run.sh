#!/usr/bin/env bash
# Build (if needed) and run the DOOM frontend, forwarding any arguments
# (e.g. `./run.sh -smoke` for the headless self-check).
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
exec ./bin/doom "$@"
