#!/usr/bin/env bash
# Build the dewasm-converted SQLite library that backs the sqlite3 shim gem.
#
# Ensures the pinned libsqlite3.wasm exists in examples/apps/cache (building it if needed), converts it to Ruby with dewasm-cli in library mode, and drops the result where the shim gem loads it from (sqlite3/lib/sqlite3/sqlite3_wasm.rb, gitignored).
set -euo pipefail
cd "$(dirname "$0")"

(cd ../apps && ./setup.sh)

echo "rails example: converting libsqlite3.wasm -> sqlite3/lib/sqlite3/sqlite3_wasm.rb"
cargo run --release -q -p dewasm-cli -- \
  ../apps/cache/libsqlite3.wasm \
  --target ruby --mode library --module-name Sqlite3Wasm \
  -o sqlite3/lib/sqlite3/sqlite3_wasm.rb
echo "rails example: done"
