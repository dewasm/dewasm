#!/usr/bin/env python3

"""Run a benchmark kernel under pywasm, the pure-Python wasm interpreter, with
the same command line every other runner in the suite gets:

    python benchmarks/drivers/pywasm.py <module.wasm> [guest-args...]

The guest sees argv = [basename(module), *guest-args], matching what wasmtime
passes, so a kernel's `<module> <iterations>` contract holds identically here.

stdout carries guest output and nothing else -- the harness compares it byte
for byte against the wasmtime oracle. Diagnostics, including the
`load_ms=<float>` line the harness records, go to stderr.

pywasm implements wasm traps as bare `assert` statements, so this driver must
never run under `python -O`: assertions off turns a trap into silently wrong
output. It refuses to start in that case. The file is plain Python with no
CPython-only dependency, so `pypy3 pywasm.py ...` works unchanged.
"""

import os
import sys
import time

# This file is called pywasm.py by contract, and a script's own directory leads
# sys.path -- so an unguarded `import pywasm` imports this file instead of the
# package. Drop that entry (only that one) before importing.
_HERE = os.path.dirname(os.path.abspath(__file__))
sys.path[:] = [p for p in sys.path
               if os.path.abspath(p or os.curdir) != _HERE]

import pywasm  # noqa: E402


def die(message: str) -> None:
    print(f"pywasm.py: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> int:
    if not __debug__:
        die("refusing to run with assertions disabled: pywasm implements wasm "
            "traps as `assert`, so -O turns a trap into silently wrong output")

    argv = sys.argv[1:]
    if not argv:
        die("usage: python pywasm.py <module.wasm> [guest-args...]")
    path = argv[0]
    if not os.path.isfile(path):
        die(f"{path}: no such file")

    guest_argv = [os.path.basename(path), *argv[1:]]

    started = time.perf_counter()
    runtime = pywasm.core.Runtime()
    wasi = pywasm.wasi.Preview1(guest_argv, {}, {})
    wasi.bind(runtime)
    inst = runtime.instance_from_file(path)
    print(f"load_ms={(time.perf_counter() - started) * 1000.0:.3f}",
          file=sys.stderr)

    code = wasi.main(runtime, inst)
    sys.stdout.flush()
    return code


if __name__ == "__main__":
    raise SystemExit(main())
