# Running the benchmarks

How to run the cross-runtime benchmark suite and how to read its numbers.
The results are in [results.md](results.md) with its figures under `figs/`; the workloads live under [`benchmarks/`](../../benchmarks/README.md).

## Running

```console
$ examples/apps/fetch-and-build.sh   # the cached apps (sqlite3-shell, cowsay, ...)
$ benchmarks/setup.sh                # builds the microbenchmarks, pins pywasm and wardite
$ cargo xtask bench                  # the full matrix, roughly an hour
```

A full run writes a dated record to `benchmarks/results/` and regenerates `docs/benchmarks/results.md` with its charts.
Useful options:

| Command | Effect |
| --- | --- |
| `cargo xtask bench --list` | Show the matrix and each runner's availability without running anything. |
| `cargo xtask bench <filter>` | Only pairs whose workload or runner label contains the substring, e.g. `dewasm-ruby` or `app/`. The generated doc then covers only those pairs, so publish from a full run. |
| `cargo xtask bench --render <results.json>` | Regenerate the doc and charts from a stored record without measuring. |
| `--reps N`, `--target-ms MS`, `--timeout SECS` | Timed runs per measurement (default 5), calibration target per sample (default 300), per-process ceiling (default 900). |

`wasmtime` is required; any other missing runner is reported as skipped with the reason, and the run continues.

## How measurement works

- The fastest and slowest runners differ by factors in the tens of thousands, so no fixed iteration count fits everyone.
  Each microbenchmark takes an iteration count in `argv[1]`, and the harness calibrates it per runner until one sample reaches the target compute time.
  Compare the per-iteration figures, never the raw wall times.
- Every microbenchmark is also run at zero iterations.
  That run is the cold start column (process startup plus module load), and subtracting it from the timed run isolates compute.
  Application benchmarks are the opposite: one fixed input for everyone, whole wall time, because that is what a user of the converted program experiences.
  Fast runners average several back-to-back executions per sample (the Runs/sample column).
- Each measurement is one warmup plus the timed repetitions, reported as minimum and median.
  The charts plot the median.
- Every runner's stdout is compared byte for byte against wasmtime at the same iteration count.
  A mismatch fails the run; a wrong answer is never reported as a fast one.

## Pitfalls when measuring by hand

- Measure on mains power.
  On battery an Apple silicon host runs the whole suite roughly 25% slower, with extra variance early in a run.
- `wasmtime` keeps an on-disk compilation cache by default.
  Warm and cold runs differ by an order of ten; `-C cache=n` disables it.
- Ruby's YJIT has no on-stack replacement.
  A single long-running loop is never JIT-compiled, so results swing on whether work is split across method calls.
