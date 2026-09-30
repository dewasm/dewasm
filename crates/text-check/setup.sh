#!/usr/bin/env bash

# Populate crates/text-check/cache/ with the word lists the vocabulary check reads.
# They are NGSL 1.2 and NAWL 1.2 by Browne, Culligan, and Phillips.
# The official source is `https://www.newgeneralservicelist.com`.
# Both lists are licensed under CC BY-SA 4.0, so they are fetched, never committed.
# The vocabulary check fails loudly when the cache is missing.

# `--check` verifies instead of fetching.
# It names every list whose cached copy does not match its sha256, and exits nonzero.
# It needs no network.

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

cd "$(dirname "$0")"
mkdir -p cache

# name sha256
lists=(
  "NGSL_12_lemmatized_for_research d814f2a0a3c61479a2c5ad037661719a0cc6e7dbcde31f181b54f12d0f1e11a4"
  "NAWL_12_lemmatized_for_research c28ef95623d79c08a4060d6d6d51d3331115e75a18ee247caa4cc3ae5506b92e"
)
origin=https://www.newgeneralservicelist.com/s
# The Wayback Machine copy is tried only when fetching from the official host fails at all.
# The sha256 is what makes it safe to accept.
mirror=https://web.archive.org/web/2025id_/https://www.newgeneralservicelist.com/s

matches() {
  [ -f "$2" ] && echo "$1  $2" | shasum -a 256 -c - >/dev/null 2>&1
}

stale=0
for entry in "${lists[@]}"; do
  read -r name sha256 <<<"$entry"
  out="cache/$name.csv"
  if matches "$sha256" "$out"; then
    continue
  fi
  if [ -n "$check" ]; then
    echo "text-check: $out does not match its sha256; run crates/text-check/setup.sh" >&2
    stale=1
    continue
  fi
  tmp="$out.tmp"
  for src in "$origin" "$mirror"; do
    [ "$src" = "$origin" ] || echo "fetch: $origin failed, trying $src"
    if curl -fsSL --retry 5 --retry-delay 2 --retry-connrefused -o "$tmp" "$src/$name.csv"; then
      break
    fi
  done
  if ! matches "$sha256" "$tmp"; then
    echo "fetch: $name.csv is missing or does not match its sha256" >&2
    rm -f "$tmp"
    exit 1
  fi
  mv "$tmp" "$out"
  echo "fetched $out"
done
[ "$stale" = 0 ] || exit 1
[ -n "$check" ] && echo "text-check: every cached list matches its sha256"
exit 0
