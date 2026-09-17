---
name: dewasm-bench
description: |
  Run and publish the benchmark measurements (cargo xtask record-speed / record-size). Use when
  asked to run the benchmarks, take a speed or size record, or publish one. Methodology and its
  pitfalls live in docs/benchmarks/README.md; this skill adds the run-and-publish checklist.
---

# Run a benchmark record

Read [`docs/benchmarks/README.md`](../../../docs/benchmarks/README.md) first: it holds the commands, the measurement methodology, and the hand-measurement pitfalls.
This skill carries only the checklist around a run.

## Before the run

- Verify every runtime the matrix wants is present: `cargo xtask record-speed --list` names each runner, its availability, and what would fix a missing one.
  A publishable run starts from zero unavailable runners.
- Some runners resolve their binary through an environment variable (`$DEWASM_MONORUBY`, `$DEWASM_JRUBY`, and the other `$DEWASM_*` names the list output shows); set them when the binary is not on PATH.
- Verify the caches: `examples/apps/setup.sh --check` must report every pin matching, and `benchmarks/setup.sh` provisions the rest.

## Run and render

- `cargo xtask record-speed` for the measurement, then `cargo xtask render-speed` to regenerate `docs/benchmarks/results.md`; the same pairing holds for `record-size` and `render-size`.
- Only a full run is published; a filtered run is for validation, and its record is deleted afterwards by exact file name, never by glob.
- A publishable run has zero mismatches, panics, and timeouts, and every skip is a declared exclusion.

## Update records/README.md

- The run appends a `TODO: describe the occasion.` line for its record; fill it when committing, ten words or fewer.
- Every runner reports its own build in its version string, so the record's `runtimes` block identifies them; nothing goes into the commit message by hand (decision 93 records why).
