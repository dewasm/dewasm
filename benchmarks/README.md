# Benchmark kernels

Micro-benchmarks that compare dewasm-generated code against real wasm runtimes on the same workloads: `wasmtime` as the compiled-native reference, and two pure-source interpreters — [wardite](https://github.com/udzura/wardite) (Ruby) and [pywasm](https://github.com/mohanson/pywasm) (Python) — as the "same language, but interpreting instead of transpiling" comparison.

This directory owns the *workloads* and the *drivers* for those two interpreters. The Rust harness that runs them and reports numbers lives in `crates/xtask`.

## Quick start

```console
$ benchmarks/setup.sh                                  # pinned interpreters + built kernels
$ wasmtime benchmarks/cache/sha256.wasm 1000
1388636310267144906
$ GEM_HOME=$PWD/benchmarks/cache/gems \
    ruby benchmarks/drivers/wardite.rb benchmarks/cache/sha256.wasm 1000
1388636310267144906
$ benchmarks/cache/venv/bin/python \
    benchmarks/drivers/pywasm.py benchmarks/cache/sha256.wasm 1000
1388636310267144906
```

## The kernel contract

A kernel is a WASI command module whose only imports are from `wasi_snapshot_preview1`.

- Invoked as `<module> <iterations>`, with `<iterations>` a non-negative decimal integer in `argv[1]`.
- It performs `<iterations>` units of work, writes **exactly one line** to stdout — the decimal result and `\n` — and exits 0.
- `<iterations> = 0` performs no work but still prints. The harness times that run to isolate process startup plus module load, and subtracts it.
- Bad or missing `argv[1]` prints `usage: <module> <iterations>` on stderr and exits 2. The harness always passes exactly one argument, so anything else is a caller bug, not an input to guess at.
- **Deterministic**: the same `<iterations>` produces byte-identical stdout on every runtime. `wasmtime` is the oracle and the harness fails any runner that disagrees — that is how silent wrongness gets caught rather than showing up as a suspiciously good number.
- **Every kernel prints an integer.** Never a float: Ruby, Python, Perl, Go and Java disagree on float formatting, and the harness compares stdout byte for byte. The float kernels print an integer-ized checksum instead.

## What the kernels may use, and why

The comparison is only worth having if every runner executes the same module, so the workloads sit inside the intersection of what all of them support. That intersection is set by wardite, which is the most limited:

| Not allowed | Because |
| --- | --- |
| f32 anywhere | wardite computes f32 in double precision and never re-rounds, so `f32.add(0.1, 0.2)` yields `0.30000000447034836`. Silently wrong, no error. |
| multi-value returns, typed `select` (0x1C), reference types, all `table.*` instructions, `data.drop` | wardite does not implement them. |
| WASI beyond `args_get`, `args_sizes_get`, `fd_write`, `proc_exit` | wardite implements 26 p1 functions but not `fd_seek`, which libc stdio needs. Imports are resolved at instantiation, so merely *linking* stdio makes a module unloadable there even if it never calls it. |
| wasm call depth ≥ 1000 | pywasm hard-caps depth at 1024 with an `assert` in `core.py`. |

i32, i64, f64, all loads and stores, `memory.grow`/`size`, `br_table`, globals, direct calls, `call_indirect` and a declared table with an active element segment are all fine. wardite has no validation pass at all, so a module outside this set will not be rejected — it will just misbehave.

## The kernels

Hand-written `.wat` micro-kernels, each isolating one axis. They share a preamble — WASI imports, an argv atoi, a decimal printer, and a fixed memory map — which is duplicated verbatim rather than included, so each `.wat` stays a standalone module `wat2wasm` and `dewasm` can consume directly.

| Kernel | One iteration is |
| --- | --- |
| `i32_alu` | a dependency chain of i32 mul / add / xor / shift / rotl |
| `i64_alu` | the same chain widened to i64; the ratio against `i32_alu` is the interesting number, since the Ruby/Python/Perl backends carry i64 in a bignum-capable host integer |
| `f64_alu` | f64 add, mul and sqrt; the accumulated sum is scaled by 1000 and truncated to an integer |
| `mem_rw` | one i32 load and one i32 store at a pseudo-random offset in a 256 KiB window |
| `call_direct` | four nested direct calls, four frames deep |
| `call_indirect` | four indirect calls through a 4-entry funcref table, index derived from the loop counter so nothing can devirtualize it |

`call_direct` is the one that most needs to exist. YJIT has no on-stack replacement, so a single long-running loop in generated Ruby is never JIT compiled, while the same arithmetic split across called methods is — a ~10× swing that `i32_alu` alone would hide entirely.

C kernels in `kernels/src/`, built with `zig cc`, for workloads with real shape:

| Kernel | One iteration is |
| --- | --- |
| `sha256` | one 64-byte SHA-256 compression: i32-heavy, with a 64-word message schedule in memory and a 64-round loop |
| `mandelbrot` | one escape-loop evaluation (≤ 100 steps) at a hashed sample point; prints the total escape count, an integer |
| `wordcount` | one byte of a scan over a generated 8 KiB text buffer: a load, unpredictable branches, a scattered histogram update |

`wordcount` generates its buffer before reading `argv`, so that fixed cost lands in the `<iterations> = 0` baseline and per-iteration numbers stay pure scanning.

## Build flags

`kernels/build.sh` assembles the `.wat` kernels with `wat2wasm` and compiles the C kernels with `zig cc`, writing every module to `benchmarks/cache/<id>.wasm` so every runner consumes the same bytes. The `.wat` and `.c` sources are checked in; the built `.wasm` is not, and neither is anything else under `cache/`.

The C flags are all load-bearing:

- `-nostdlib` — the kernels define their own `_start` and use no libc, which is what keeps the import list down to the four allowed WASI functions. Note that `-nostartfiles` does *not* work: `zig cc` ignores it for wasm, still links `crt1-command.o`, and the build dies on a duplicate `_start`. A consequence is that nothing calls `__wasm_call_ctors`, so `kernel.h` exposes an explicit `kernel_setup()` hook instead of supporting `__attribute__((constructor))`, and kernels must avoid anything clang lowers to a `memset`/`memcpy` call (use `static` storage rather than a zero-initialized local array).
- `-mno-bulk-memory -mno-bulk-memory-opt` — both, because they are separate LLVM features and with only the first one set clang still lowers array zeroing to `memory.fill`.
- `-mno-nontrapping-fptoint` — wardite mishandles NaN in `i32`/`i64.trunc_sat`.
- `-mno-multivalue -mno-reference-types` — wardite implements neither.
- `-Wl,-z,stack-size=65536` — the default leaves a 16 MiB shadow stack, which forces a 16 MiB initial memory. That is a real cost for interpreters that back linear memory with a host byte array; 64 KiB is ample here and brings each module down to 2 pages.

## The drivers

```console
$ ruby   benchmarks/drivers/wardite.rb <module.wasm> [guest-args...]
$ python benchmarks/drivers/pywasm.py  <module.wasm> [guest-args...]
```

Both set the guest's argv to `[basename(module), *guest-args]`, matching what `wasmtime` passes, run `_start`, and propagate the guest exit code. stdout carries guest output and nothing else; diagnostics go to stderr, including one line the harness records:

```
load_ms=<float>
```

which is the module load and instantiate time, measured just around that step.

Two things worth knowing:

- wardite implements `proc_exit` with Ruby's `Kernel#exit`, so a guest that calls it terminates the driver process directly with the guest's status. Convenient — the exit code propagates for free — but it means no driver code after `_start` runs on that path.
- pywasm implements wasm traps as bare `assert` statements, so running it under `python -O` turns a trap into silently wrong output. `pywasm.py` refuses to start when `__debug__` is false.

`pywasm.py` is pure Python and runs unmodified under PyPy; `setup.sh` deliberately does not set PyPy up, because the harness drives the host's own `pypy3`. pywasm just has to be importable there (`pypy3 -m pip install pywasm==2.2.3`).

## Verification

Every kernel produces identical stdout under `wasmtime`, wardite and pywasm, and under dewasm's own Ruby backend — the `.wat` kernels converting identically whether the converter is fed the `.wat` or the built `.wasm`. Spot-checked at `<iterations>` = 0, 1 and 1000:

| Kernel | `0` | `1` | `1000` |
| --- | --- | --- | --- |
| `i32_alu` | 2654435769 | 2301304102 | 4267955784 |
| `i64_alu` | 11400714819323198485 | 7492284122455132145 | 11176877086846500725 |
| `f64_alu` | 0 | 1500 | 31646183 |
| `mem_rw` | 0 | 0 | 2199 |
| `call_direct` | 0 | 1 | 1583081352 |
| `call_indirect` | 0 | 1 | 1036725302 |
| `sha256` | 7640891576010911365 | 1806834717474567160 | 1388636310267144906 |
| `mandelbrot` | 0 | 2 | 29262 |
| `wordcount` | 0 | 1000007 | 154013700 |

## Rough per-iteration cost

Measured on an M-series macOS host as `(t(N) - t(0)) / N`, so startup and module load are already out. Indicative only — the point is picking per-runner iteration counts that land in the same wall-clock ballpark, not the numbers themselves.

| Kernel | wasmtime | wardite (ruby 4.0) | pywasm (CPython 3.14) | pywasm (PyPy 3.11) |
| --- | --- | --- | --- | --- |
| `i32_alu` | 2.4 ns | 15.6 µs | 55.0 µs | 7.7 µs |
| `i64_alu` | 2.4 ns | 17.4 µs | 58.9 µs | 8.9 µs |
| `f64_alu` | 1.0 ns | 7.7 µs | 29.9 µs | 4.1 µs |
| `mem_rw` | 1.0 ns | 13.1 µs | 40.3 µs | 5.7 µs |
| `call_direct` | 3.6 ns | 18.2 µs | 50.3 µs | 7.7 µs |
| `call_indirect` | 10.7 ns | 28.3 µs | 83.6 µs | 12.9 µs |
| `sha256` | 299 ns | 4.19 ms | 14.9 ms | 2.15 ms |
| `mandelbrot` | 98.8 ns | 385 µs | 1.39 ms | 279 µs |
| `wordcount` | 2.1 ns | 21.2 µs | 61.4 µs | 8.2 µs |

For roughly one second of work per run that means something like:

| Runner | micro-kernels | `mandelbrot` | `sha256` |
| --- | --- | --- | --- |
| wasmtime | 10<sup>8</sup> | 10<sup>7</sup> | 3 × 10<sup>6</sup> |
| wardite / PyPy-pywasm | 10<sup>5</sup> | 2 × 10<sup>3</sup> | 200 |
| CPython-pywasm | 3 × 10<sup>4</sup> | 600 | 60 |

Process startup and module load, for sizing the fixed overhead: wasmtime ≈ 29 ms, wardite ≈ 85 ms (`load_ms` ≈ 5–6 ms of it), CPython pywasm ≈ 68 ms (`load_ms` ≈ 0.7 ms), PyPy pywasm ≈ 105 ms (`load_ms` ≈ 3 ms).
