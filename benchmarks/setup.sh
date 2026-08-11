#!/usr/bin/env bash

# Populate benchmarks/cache/ with everything the benchmark suite needs that is
# not checked in: the two pure-source wasm interpreters we compare against, and
# the built microbenchmark modules.
#
#   cache/venv/    a Python venv with pywasm pinned to PYWASM_VERSION
#   cache/gems/    a GEM_HOME with wardite pinned to WARDITE_VERSION
#   cache/wat/*.wasm  the hand-written microbenchmarks, via wat/build.sh
#   cache/c/*.wasm    the C microbenchmarks, via c/build.sh
#
# Third-party artifacts are never committed, and the cache is gitignored.
# Idempotent: re-running is a no-op beyond re-checking the pins and rebuilding
# the microbenchmarks.
#
# PyPy is deliberately not set up here. The harness drives the host's own
# pypy3 install; benchmarks/drivers/pywasm.py runs under it unmodified, but
# pywasm has to be importable there, e.g.
#
#   pypy3 -m pip install pywasm==2.2.3
#

set -euo pipefail

cd "$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
mkdir -p cache

PYWASM_VERSION=2.2.3
WARDITE_VERSION=0.9.0

# Fail loudly with an actionable message rather than half-installing.
require_tool() {
  command -v "$1" >/dev/null && return
  echo "benchmarks/setup.sh: $1 not found — $2" >&2
  exit 1
}

require_tool python3 "install Python 3 (brew install python)"
require_tool ruby "install Ruby (mise install ruby, or brew install ruby)"
require_tool gem "install Ruby (mise install ruby, or brew install ruby)"

venv=cache/venv
gems=cache/gems

# --- pywasm: pure Python, so the same install works under CPython and PyPy.
[ -x "$venv/bin/python" ] || python3 -m venv "$venv"
"$venv/bin/pip" install --quiet --disable-pip-version-check \
  "pywasm==$PYWASM_VERSION"

# --- wardite: pure Ruby. A private GEM_HOME keeps it out of the user's gems;
# the drivers are run with GEM_HOME pointed here.
GEM_HOME="$PWD/$gems" gem install wardite \
  --version "$WARDITE_VERSION" --no-document --silent --conservative

# --- the microbenchmark modules themselves, one build script per family.
./wat/build.sh
./c/build.sh

echo
echo "benchmarks/setup.sh: ready"
echo "  python  $("$venv/bin/python" -V 2>&1)"
echo "  pywasm  $("$venv/bin/python" -c \
  'import importlib.metadata as m; print(m.version("pywasm"))')"
echo "  ruby    $(ruby -v | cut -d' ' -f1-2)"
echo "  wardite $(GEM_HOME="$PWD/$gems" gem list --exact wardite |
  sed -n 's/^wardite (\(.*\))$/\1/p')"
echo
echo "  ruby   benchmarks/drivers/wardite.rb benchmarks/cache/<family>/<id>.wasm <n>"
echo "    (with GEM_HOME=$PWD/$gems)"
echo "  python benchmarks/drivers/pywasm.py  benchmarks/cache/<family>/<id>.wasm <n>"
echo "    (with $venv/bin/python)"
