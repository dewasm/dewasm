# Taking a measurement record

What to do around a speed or size run, and what makes a result suspect.
The commands and the methodology are in [`docs/benchmarks/README.md`](../docs/benchmarks/README.md) and [`docs/sizes/README.md`](../docs/sizes/README.md).
This file holds what a user of the numbers never needs and whoever takes them always does.

## Before the run

- Every runner the matrix declares is available.
  `cargo xtask record-speed --list` names each one, its availability, and what would fix a missing one.
  A publishable run starts from zero unavailable runners.
- Some runners resolve their binary through an environment variable.
  Those are `$DEWASM_MONORUBY`, `$DEWASM_JRUBY`, and the other `$DEWASM_*` names the list output shows.
  Set them when the binary is not on PATH.
- `MISE_ENV=bench mise install` installs the tools of `mise.bench.toml` beside those of `mise.toml`.
  Those are JRuby, GraalPy, TinyGo, `wasmtime`, and `wabt`.
  Take the record under `MISE_ENV=bench` too, so that they are on PATH.
  A shell where `mise` is not active needs `mise exec --` in front of each command.
- The JIT-enabled CPython is the `python3` of `mise.toml`.
  That prebuilt CPython turns its JIT on under `PYTHON_JIT=1`, except on an x86-64 macOS host.
- `mise` installs no engine that is a build from source.
  `monoruby` and Spinel are built from their repositories.
  A `mise.local.toml`, which Git ignores, can name them in its `[env]` table.
  The variables are `$DEWASM_MONORUBY` and `$DEWASM_SPINEL`.
- The prebuilt Ruby that `mise` installs has no ZJIT, so `dewasm-ruby-zjit` reports unavailable.
  ZJIT needs a Ruby built from source with `rustc` on PATH.
  `MISE_LOCKFILE=false MISE_RUBY_COMPILE=true mise install --force ruby` builds it in about 3 minutes.
  Without `MISE_LOCKFILE=false`, that install rewrites the Ruby entries of `mise.lock`.
- The caches match their fixed versions: `examples/apps/setup.sh --check` reports every app matching.
  `benchmarks/setup.sh` provisions the rest.
- Measure on mains power.
  Without mains power, an Apple silicon host runs the whole suite roughly 25% slower.
  It also shows extra variance early in a run.
- `wasmtime` keeps an on-disk compilation cache by default.
  Warm and cold runs differ by an order of ten; `-C cache=n` turns it off.

## Run and render

- `cargo xtask record-speed` measures.
  Then `cargo xtask render-speed` regenerates `docs/benchmarks/results.md`.
  The same pairing holds for `record-size` and `render-size`.
- Only a full run is published: a filtered run validates a cell.
  A filtered run's record is removed afterwards by exact file name, never by a wildcard.
- The run adds a `TODO: describe the occasion.` line to `records/README.md`.
  Fill it when committing, ten words or fewer.

## Reading a result that looks wrong

- Ruby's YJIT has no on-stack replacement.
  A single long-running loop is never JIT-compiled.
  So results swing on whether work is split across method calls.
- The test of a suspect cell is whether it reproduces, not what it looks like.
  Re-measure the pair with a filtered run and compare.
  The figure that repeats is the figure.
  `app/cowsay` on `dewasm-jruby` read 6.15 s once and 3.7 s on each re-measurement.
  The first figure was therefore dropped.
- `runs_per_sample` of 1 on a fast cell is a hint to check, not a verdict.
  The harness batches runs until a sample reaches the target compute time.
  So a calibration against a cold artifact can settle on 1.
  That leaves a whole process start in the figure.
  `dewasm-go` read 10.6 ms that way on 2026-09-17, and 3.0 ms at 64 runs per sample once warm.
  A compiled backend also reports 1 with figures that reproduce exactly, so compare before concluding.
- A workload whose program changed is a new baseline, not a regression or an improvement.
  Records are dated snapshots, so say which records a comparison spans.

## Before publishing

- Zero mismatches, zero panics, no run over its time limit, and every skip is a declared exclusion.
- Each runner's version string identifies its own build.
  So the record's `runtimes` block states where each figure came from.
  Nothing goes into the commit message by hand (decision 93 records why).
