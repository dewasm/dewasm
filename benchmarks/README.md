# benchmarks

Everything the cross-runtime benchmark suite measures and compares against. How to run it and read the numbers is [docs/benchmarks/README.md](../docs/benchmarks/README.md).

- `wat/` hand-written microbenchmarks, each isolating one instruction axis.
- `c/` microbenchmarks compiled from C with `zig cc`, for workloads with realistic shape.
- `drivers/` scripts that run a module under the two pure-source wasm interpreters, [wardite](https://github.com/udzura/wardite) (Ruby) and [pywasm](https://github.com/mohanson/pywasm) (Python), with the same command line every other runner gets.
- `cache/` gitignored build output: the compiled modules and the pinned interpreter installs, produced by `setup.sh`.
- `results/` one dated JSON record per full benchmark run; the generated results page is rendered from such a record.
