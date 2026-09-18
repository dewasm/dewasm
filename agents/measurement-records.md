# Taking a measurement record

What to do around a speed or size run, and what makes a result suspect.
The commands and the methodology are in [docs/benchmarks/README.md](../docs/benchmarks/README.md) and [docs/sizes/README.md](../docs/sizes/README.md); this file holds what a user of the numbers never needs and whoever takes them always does.

## Before the run

- Every runner the matrix declares is available: `cargo xtask record-speed --list` names each one, its availability, and what would fix a missing one.
  A publishable run starts from zero unavailable runners.
- Some runners resolve their binary through an environment variable (`$DEWASM_MONORUBY`, `$DEWASM_JRUBY`, and the other `$DEWASM_*` names the list output shows); set them when the binary is not on PATH.
- The caches match their pins: `examples/apps/setup.sh --check` reports every app matching, and `benchmarks/setup.sh` provisions the rest.
- Measure on mains power.
  On battery an Apple silicon host runs the whole suite roughly 25% slower, with extra variance early in a run.
- `wasmtime` keeps an on-disk compilation cache by default.
  Warm and cold runs differ by an order of ten; `-C cache=n` disables it.

## Run and render

- `cargo xtask record-speed` measures, then `cargo xtask render-speed` regenerates `docs/benchmarks/results.md`; the same pairing holds for `record-size` and `render-size`.
- Only a full run is published: a filtered run validates a cell, and its record is deleted afterwards by exact file name, never by glob.
- The run appends a `TODO: describe the occasion.` line to `records/README.md`; fill it when committing, ten words or fewer.

## Reading a result that looks wrong

- Ruby's YJIT has no on-stack replacement.
  A single long-running loop is never JIT-compiled, so results swing on whether work is split across method calls.
- A fast runner's app cell should batch several runs per sample, since the harness calibrates until a sample reaches the target compute time.
  A cell that reports `runs_per_sample` of 1 while its median is far below that target was calibrated against a cold artifact, and its figure carries a whole process start that its neighbours amortize.
  Re-measure that pair: `app/cowsay` on `dewasm-go` read 10.6 ms at one run per sample on 2026-09-17, and 3.0 ms at 64 once the artifact was warm.
- A workload whose program changed is a new baseline, not a regression or an improvement.
  Records are dated snapshots, so say which records a comparison spans.

## Before publishing

- Zero mismatches, panics and timeouts, and every skip is a declared exclusion.
- Each runner's version string identifies its own build, so the record's `runtimes` block is the provenance; nothing goes into the commit message by hand (decision 93 records why).
