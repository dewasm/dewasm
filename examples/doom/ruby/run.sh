#!/usr/bin/env bash
# Build (if needed) and run the DOOM frontend, forwarding any arguments
# (e.g. `./run.sh --smoke` for the headless self-check). --yjit is required:
# the Ruby backend is dewasm's slowest, and only YJIT keeps DOOM playable.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
export RUBY_YJIT_ENABLE=1
exec ruby --yjit main.rb "$@"
