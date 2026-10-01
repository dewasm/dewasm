#!/usr/bin/env bash
# Build (if needed) and run the windowed DOOM frontend, forwarding any arguments.
# Examples are `./run.sh --smoke` for the headless self-check, and `./run.sh --scale 4`.
# The generated library is shared with the terminal frontend.
# `../build.sh` fetches doom.wasm and regenerates `../doom_gen.rb`, which main.rb requires.
# Bundler installs the `gosu` gem into `vendor/bundle`, which is ignored by Git.
# So the gem and its version stay scoped to this directory.
# `--yjit` is required: the Ruby backend is dewasm's slowest, and only YJIT keeps DOOM playable.
set -euo pipefail
cd "$(dirname "$0")"

../build.sh

bundle config set --local path vendor/bundle
bundle install --quiet

ruby -c main.rb > /dev/null

exec bundle exec ruby --yjit main.rb "$@"
