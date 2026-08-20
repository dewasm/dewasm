#!/usr/bin/env bash
# End-to-end demo: Rails on SQLite converted from wasm to pure Ruby by dewasm.
#
# 1. build the converted SQLite library (build.sh)
# 2. shim unit smoke (sqlite3 gem API surface)
# 3.
# ActiveRecord standalone smoke
# 4. the Rails app: bundle, migrate, boot, drive it over HTTP
#
# Requires: ruby >= 3.4 with the rails gem installed, zig (for the wasm build), network access for the first bundle install.
set -euo pipefail
cd "$(dirname "$0")"

PORT="${PORT:-3100}"

./build.sh

echo "=== shim smoke ==="
(cd sqlite3 && ruby test_shim.rb | tail -1)

echo "=== ActiveRecord smoke ==="
(cd ar_smoke && bundle install --quiet && bundle exec ruby smoke.rb | tail -1)

echo "=== Rails app ==="
[ -d app ] || ./setup-app.sh
cd app
bundle install --quiet
bin/rails db:prepare

pidfile=$(mktemp)
bin/rails server -p "$PORT" --pid "$pidfile" &
server_job=$!
trap 'kill "$(cat "$pidfile" 2>/dev/null)" 2>/dev/null || kill $server_job 2>/dev/null || true' EXIT

for _ in $(seq 1 60); do
  curl -sf "http://127.0.0.1:$PORT/up" >/dev/null 2>&1 && break
  sleep 1
done

echo "--- POST /posts"
curl -sf -X POST "http://127.0.0.1:$PORT/posts" \
  -H 'Content-Type: application/json' \
  -d '{"post":{"title":"Hello from HTTP","body":"Rails on dewasm SQLite","views":9007199254740993,"rating":4.9}}'
echo
echo "--- GET /posts"
curl -sf "http://127.0.0.1:$PORT/posts"
echo
echo "--- GET /stats (sqlite_version() answered by the wasm-converted SQLite)"
stats=$(curl -sf "http://127.0.0.1:$PORT/stats")
echo "$stats"
# The -wasm suffix is patched into the wasm build (../apps/scripts/sqlite3.sh): its absence means the answer came from a native SQLite, not the converted engine.
case "$stats" in
  *3.53.3-wasm*) ;;
  *)
    echo "/stats did not report the converted engine's version 3.53.3-wasm" >&2
    exit 1
    ;;
esac
echo "RAILS-ON-DEWASM-OK"
