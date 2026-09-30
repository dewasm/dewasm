#!/usr/bin/env bash
# Build (if needed) and run the NES frontend, forwarding any arguments.
# An example is a ROM path, or `./run.sh -smoke` for the headless self-check.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
exec ./bin/nes "$@"
