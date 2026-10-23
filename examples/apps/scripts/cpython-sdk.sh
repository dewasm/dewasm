#!/usr/bin/env bash
# shellcheck source-path=SCRIPTDIR
# shellcheck source=common.sh

# CPython 3.14.6 as a library for wasm32-wasip1, built from the source release with `wasi-sdk`.
# The prebuilt `cpython` app is a command; a program that embeds the interpreter needs the static
# library instead, which no release ships.
# `examples/rails/pygments` links its reactor against it.
#
# The result is `cache/cpython-sdk/`:
# - `lib/libpython3.14.a` and the static libraries of the built-in modules it needs;
# - `include/python3.14/`, the headers with the `pyconfig.h` of this build;
# - `lib/python3.14/`, the standard library tree, without its tests.
#
# The build is the one `Tools/wasm/wasi` does: a build Python for this host, then the cross build.
# It runs the cross-compiled interpreter once, through `wasmtime`.
# The audit record is agents/apps-audit.md.

source "$(dirname "${BASH_SOURCE[0]}")/common.sh"

PY_URL="https://www.python.org/ftp/python/3.14.6/Python-3.14.6.tar.xz"
PY_SHA256="143b1dddefaec3bd2e21e3b839b34a2b7fb9842272883c576420d605e9f30c63"
PY_DIR="Python-3.14.6"

sdk=cache/cpython-sdk
sdk_stamp="cache/cpython-sdk.src-sha256"
sdk_want="$(printf '%s\n%s' "$PY_SHA256" "$(wasi_sdk_stamp)")"
if is_cached "$sdk_stamp" "$sdk_want" "$sdk/lib/libpython3.14.a" "$sdk/include/python3.14/Python.h" \
  "$sdk/lib/python3.14/os.py"; then
  echo "cpython-sdk: cached"
  exit 0
fi

require_wasi_sdk cpython-sdk
require_tool cpython-sdk python3 "a host Python runs the CPython build script"
require_tool cpython-sdk wasmtime "the CPython build runs the cross-compiled interpreter once"

echo "cpython-sdk: fetching $PY_URL"
new_tmpdir
fetch_verified "$PY_URL" "$PY_SHA256" "$tmp/python.tar.xz"
tar xJf "$tmp/python.tar.xz" -C "$tmp"
echo "cpython-sdk: building CPython for wasm32-wasip1 (Tools/wasm/wasi build, several minutes)"
( cd "$tmp/$PY_DIR" && python3 Tools/wasm/wasi build --wasi-sdk "$WASI_SDK_PATH" )

build="$tmp/$PY_DIR/cross-build/wasm32-wasip1"
rm -rf "$sdk"
mkdir -p "$sdk/lib" "$sdk/include"
cp "$build/libpython3.14.a" "$build"/Modules/_decimal/libmpdec/libmpdec.a "$build"/Modules/expat/libexpat.a \
  "$build"/Modules/_hacl/*.a "$sdk/lib/"
cp -R "$tmp/$PY_DIR/Include" "$sdk/include/python3.14"
cp "$build/pyconfig.h" "$sdk/include/python3.14/"
cp -R "$tmp/$PY_DIR/Lib" "$sdk/lib/python3.14"
# `sysconfig` reads this module, which the cross build generates.
cp "$build"/build/lib.wasi-wasm32-3.14/_sysconfigdata__*.py "$sdk/lib/python3.14/"
find "$sdk/lib/python3.14" \( -name test -o -name tests -o -name __pycache__ -o -name idlelib \
  -o -name tkinter -o -name turtledemo -o -name ensurepip \) -type d -prune -exec rm -rf {} +

write_stamp "$sdk_stamp" "$sdk_want"
echo "cpython-sdk: -> $sdk"
