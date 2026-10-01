#!/usr/bin/env bash
# Build (if needed) and run the windowed NES frontend, forwarding any arguments.
# An example is `./run.sh path/to/game.nes`, or `./run.sh --smoke` for the headless self-check.
# The generated library is shared with the terminal frontend.
# `../build.sh` builds `nes.wasm` and regenerates `../nes_gen.rb`, which `main.rb` requires.
# `gosu` is installed with Bundler into `vendor/bundle`, which is ignored by Git.
# So the gem and its version stay scoped to this directory.
# `--yjit` is required: the Ruby backend is dewasm's slowest.
# Only YJIT keeps the emulator playable.
set -euo pipefail
cd "$(dirname "$0")"

../build.sh

bundle config set --local path vendor/bundle
bundle install --quiet

ruby -c main.rb > /dev/null

exec bundle exec ruby --yjit main.rb "$@"
