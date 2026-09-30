---
name: "dewasm-bench"
description: |
  Run and publish the benchmark measurements (`cargo xtask record-speed` / `record-size`).
  Use when asked to run the benchmarks, take a speed or size record, or publish one.
  The commands live in `docs/benchmarks/README.md`.
  The steps to check and the traps live in `agents/measurement-records.md`.
---

# Run a benchmark record

Read [`agents/measurement-records.md`](../../../agents/measurement-records.md) and follow it.
It carries the steps to check before and after a run, and what makes a result suspect.
It also says what a run must satisfy before it is published.
The commands and the measurement methodology are in two files.
They are [`docs/benchmarks/README.md`](../../../docs/benchmarks/README.md) and [`docs/sizes/README.md`](../../../docs/sizes/README.md).
