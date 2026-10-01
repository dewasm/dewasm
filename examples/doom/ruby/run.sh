#!/usr/bin/env bash
# Build (if needed) and run the DOOM frontend, forwarding any arguments.
# For example, `./run.sh --smoke` runs the headless self-check.
# `--yjit` is required: the Ruby backend is dewasm's slowest, and only YJIT keeps DOOM playable.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
exec ruby --yjit main.rb "$@"
