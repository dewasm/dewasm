#!/usr/bin/env bash
# Build (if needed) and run the NES frontend, forwarding any arguments.
# An example is a ROM path, or `./run.sh --smoke` for the headless self-check.
# `--yjit` is required: the Ruby backend is dewasm's slowest.
# Only YJIT keeps the emulator playable at anything close to 60Hz.
set -euo pipefail
cd "$(dirname "$0")"

./build.sh
exec ruby --yjit main.rb "$@"
