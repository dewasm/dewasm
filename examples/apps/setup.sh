#!/usr/bin/env bash

# Populate examples/apps/cache/ with the real-world example apps: prebuilt
# wasm binaries fetched from upstream, or (sqlite3, libpcap, tree-sitter,
# minigzip, ripgrep) version-pinned source releases built locally with zig or
# cargo. Third-party artifacts are never committed; the apps e2e test fails
# loudly when the cache is absent.
#
# Each app is a standalone, directly-runnable script under scripts/ (e.g.
# `scripts/sqlite3.sh` to (re)build just sqlite3 after bumping its pin); this
# driver simply runs them all. Shared boilerplate lives in scripts/common.sh.

set -euo pipefail

cd "$(dirname "$0")/scripts"

apps=()
for s in *.sh; do
  [ "$s" = common.sh ] || apps+=("$s")
done
IFS=$'\n' read -r -d '' -a apps < <(printf '%s\n' "${apps[@]}" | sort && printf '\0')

for app in "${apps[@]}"; do
  ./"$app"
done
