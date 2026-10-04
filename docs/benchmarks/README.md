# Running the benchmarks

How to run the cross-runtime benchmark suite and how to read its numbers.
The results are in [results.md](results.md) with its figures under `figs/`; the workloads live under [`benchmarks/`](../../benchmarks/README.md).

## Running

```console
$ export MISE_ENV=bench              # adds the tools of mise.bench.toml to those of mise.toml
$ mise install                       # installs wasmtime, wabt, and the engines with a release to fetch
$ examples/apps/setup.sh             # fetches and builds the pinned apps (sqlite3-shell, cowsay, ...)
$ benchmarks/setup.sh                # builds the microbenchmarks, pins pywasm and wardite
$ cargo xtask record-speed           # measures every workload on every runner
$ cargo xtask render-speed           # renders results.md and its charts from that record
```

Measuring and rendering are two commands.
A run writes a dated `<timestamp>Z-speed.json` to [`records/`](../../records/README.md) and nothing else.
Rendering turns a record into `docs/benchmarks/results.md` with its charts.
That way a wording fix in the document costs a second, not a re-measurement of the whole suite.

`record-speed` verifies the app cache against its fixed versions before it measures anything.
It refuses to start if a cached copy came from an earlier version.
An out-of-date copy is a different program.
Its numbers would be committed as a record of the current one.
Useful options:

| Command | Effect |
| --- | --- |
| `cargo xtask record-speed --list` | Show the matrix and each runner's availability without running anything. |
| `cargo xtask record-speed <filter>` | Only pairs whose workload or runner label contains the substring, for example `dewasm-ruby` or `app/`. A record from a filtered run covers only those pairs, so publish from a full run. |
| `--reps N`, `--target-ms MS`, `--timeout SECS` | Timed runs per measurement (default 3), calibration target per sample (default 300), per-process time limit (default 900). |
| `cargo xtask render-speed <record>` | Render an older record instead of the newest one; a `-size.json` path is refused. |
| `cargo xtask migrate-records` | Upgrade every stored record to its kind's current schema, in place; the render commands read only the current schema. |

`wasmtime` is required.
Any other missing runner is reported as skipped with the reason, and the run continues.
Every runner's binary is host-provided: the harness takes whatever `PATH` holds.
`mise.bench.toml` is one way to install some of them, and it is not required.

## How measurement works

- The fastest and slowest runners differ by factors in the tens of thousands.
  So no fixed iteration count fits everyone.
  Each microbenchmark takes an iteration count in `argv[1]`.
  The harness calibrates it per runner until one sample reaches the target compute time.
  Compare the per-iteration figures, never the raw wall times.
- Every microbenchmark is also run at zero iterations.
  That run is the cold start column: process start plus module load.
  Subtracting it from the timed run isolates compute.
  Application benchmarks are the opposite: one fixed input for everyone, whole wall time.
  They use whole wall time because that is what a user of the converted program experiences.
  Fast runners average several back-to-back executions per sample (the Runs/sample column).
- Each measurement is one untimed run plus the timed runs, reported as minimum and median.
  The charts plot the median.
- Every runner's `stdout` is compared with Wasmtime's at the same iteration count and must be identical.
  A mismatch fails the run; a wrong answer is never reported as a fast one.
