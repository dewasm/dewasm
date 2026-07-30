#!/usr/bin/env bash
# Build (if needed) and launch the interactive DOOM window.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
java -cp classes Main
