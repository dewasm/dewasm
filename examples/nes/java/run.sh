#!/usr/bin/env bash
# Build (if needed) and run the NES frontend, forwarding any arguments
# (e.g. `./run.sh --smoke` for the headless self-check, or a ROM path).
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
java -cp classes Main "$@"
