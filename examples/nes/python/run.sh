#!/usr/bin/env bash
# Build (if needed) and run the NES frontend, forwarding any arguments (e.g.
# a ROM path, or `./run.sh --smoke` for the headless self-check).
# PyPy runs the generated module several times faster than CPython, so it is used when installed; $PYTHON overrides the choice.
set -euo pipefail
cd "$(dirname "$0")"

if [[ -z "${PYTHON:-}" ]]; then
  if command -v pypy3 >/dev/null 2>&1; then PYTHON=pypy3; else PYTHON=python3; fi
fi
export PYTHON

./build.sh
exec "$PYTHON" main.py "$@"
