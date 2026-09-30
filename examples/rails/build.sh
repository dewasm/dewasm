#!/usr/bin/env bash
# Build the dewasm-converted SQLite library that backs the sqlite3 shim gem.
#
# Builds `libsqlite3.wasm` (at a fixed version) into `examples/apps/cache` unless it is there.
# Then converts it to Ruby with dewasm in library mode.
# The result goes where the shim gem loads it from.
# That is `sqlite3/lib/sqlite3/sqlite3_wasm.rb`, ignored by Git.
set -euo pipefail
cd "$(dirname "$0")"

(cd ../apps && ./setup.sh)

echo "rails example: converting libsqlite3.wasm -> sqlite3/lib/sqlite3/sqlite3_wasm.rb"
cargo run --release -q -p dewasm -- \
  ../apps/cache/libsqlite3.wasm \
  --target ruby --mode library --module-name Sqlite3Wasm \
  -o sqlite3/lib/sqlite3/sqlite3_wasm.rb
echo "rails example: done"
