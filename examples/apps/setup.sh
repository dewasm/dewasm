#!/usr/bin/env bash

# Populate examples/apps/cache/ with the real-world example apps.
# Most are prebuilt wasm binaries fetched from upstream.
# Some are version-pinned source releases built locally with wasi-sdk or cargo.
# Those are sqlite3, libpcap, tree-sitter, minigzip, and ripgrep.
# Third-party artifacts are never committed.
# The apps e2e test fails loudly when the cache is absent.
#
# Each app is a standalone, directly-runnable script under scripts/.
# For example, `scripts/sqlite3.sh` (re)builds just sqlite3 after bumping its pin.
# This driver runs them all.
# Shared boilerplate lives in scripts/common.sh.

# `--check` verifies instead of fetching.
# Every app whose cached copy does not match its pin is named, and the exit status is nonzero.
# It needs no network, so a consumer can call it before reading the cache.
# The consumer can then refuse to proceed on a stale cache.
# It does not measure or test the wrong artifact.

set -euo pipefail

check=
if [ "${1-}" = --check ]; then
  check=1
  shift
fi
if [ $# -gt 0 ]; then
  echo "usage: setup.sh [--check]" >&2
  exit 2
fi

cd "$(dirname "$0")/scripts"

apps=()
for s in *.sh; do
  [ "$s" = common.sh ] || apps+=("$s")
done
IFS=$'\n' read -r -d '' -a apps < <(printf '%s\n' "${apps[@]}" | sort && printf '\0')

if [ -n "$check" ]; then
  # Every app is reported, not just the first, so one run names everything to re-fetch.
  stale=0
  for app in "${apps[@]}"; do
    DEWASM_APPS_CHECK=1 APP_NAME="${app%.sh}" ./"$app" >/dev/null || stale=1
  done
  [ "$stale" = 0 ] || exit 1
  echo "apps: every cached copy matches its pin"
  exit 0
fi

for app in "${apps[@]}"; do
  ./"$app"
done
