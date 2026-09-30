#!/usr/bin/env bash
# Build (if needed) and run the DOOM frontend, forwarding any arguments.
# For example, `./run.sh -smoke` runs the headless self-check.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
exec ./bin/doom "$@"
