# benchmarks

Everything the cross-runtime benchmark suite measures and compares against.
How to run it and read the numbers is [`docs/benchmarks/README.md`](../docs/benchmarks/README.md).

- `wat/` hand-written microbenchmarks, each isolating one instruction axis.
- `c/` microbenchmarks compiled from C with `wasi-sdk` `clang`, for workloads with realistic shape.
- `drivers/` scripts that run a module under the two pure-source wasm interpreters.
  These are [`wardite`](https://github.com/udzura/wardite) (Ruby) and [`pywasm`](https://github.com/mohanson/pywasm) (Python).
  A driver takes the same command line every other runner gets.
- `cache/` build output, ignored by Git, that `setup.sh` produces.
  It holds the compiled modules and the interpreter installs at fixed versions.

The dated JSON record a full run writes lives in [`records/`](../records/README.md), with every other measurement record.
