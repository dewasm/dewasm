#!/usr/bin/env bash
# Build (if needed) and run the NES frontend, forwarding any arguments (e.g.
# a ROM path, or `./run.sh --smoke` for the headless self-check).
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
exec python3 main.py "$@"
