# ADR-57: Benchmark by Calibrated Per-Runner Iteration Counts, Net of a Measured Baseline

**Status:** Accepted (2026-08-02). Landed: `cargo xtask bench`, the workloads under `benchmarks/`, and the generated `docs/benchmarks.md`. Not covered: any CI enforcement — timings are not reproducible byte-for-byte, so nothing here is a compared snapshot.

## Context

Every neighbouring project publishes numbers and dewasm published none, which made the central claim — that AOT source output beats an interpreter loop — unfalsifiable. The direct competitors are [pywasm](https://github.com/mohanson/pywasm) (pure Python) and [wardite](https://github.com/udzura/wardite) (pure Ruby); wasmtime is the ceiling.

Measuring this naively is wrong in five ways, each observed on this host rather than assumed:

1. **The spread is a factor of ~23000.** On the `i32_alu` kernel, wasmtime costs 2.4 ns per iteration and pywasm under CPython costs 55 µs. Any single fixed iteration count either measures nothing but process startup at one end or takes minutes per sample at the other.
2. **Fixed startup cost differs per runner by more than the compute under test.** An empty Ruby process costs ~50 ms; parsing the 17 MB of Ruby generated from `sqlite3-shell.wasm` costs a further ~730 ms before any work happens. wasmtime loads the same module in ~15 ms.
3. **wasmtime caches compiled code on disk by default.** The same SQL workload takes 0.02 s warm and 0.25 s with `-C cache=n` — a 12x difference in the baseline everything else is divided by.
4. **Microbenchmark shape decides whether a JIT appears at all.** Ruby's YJIT has no on-stack replacement, so a loop that never returns from its single invocation is never compiled. The same 30M iterations take 1.70 s in one call and 0.17 s split across 3000 calls — a 10x difference in the *host*, not in the generated code.
5. **The ratio is a property of the workload, not of the backend.** dewasm-generated Ruby is 148x wasmtime on a 10k-row insert and ~570x on a 300k-row index-and-join. Publishing either alone as "the" number is a claim the next workload will refute.

## Decision

**Calibrate the iteration count per (workload, runner) pair and report `ns/op`.** Each pair measures `t(0)` first, then ramps `N` until `t(N) - t(0)` reaches a target (default 300 ms), bounded by a per-kernel cap. The cap exists only to stop a kernel whose per-iteration cost is not constant from being driven somewhere absurd; it binds only the fastest runners.

**Subtract `t(0)`, and report it as its own column.** Every kernel accepts `<iterations>` from `argv[1]` and does no work at `0` while still printing its result line. That single run measures process startup plus module load, so `t(N) - t(0)` is compute and `t(0)` is cold start — the one axis where an interpreter legitimately beats an AOT compiler. Hazards 2 and 3 become two reported numbers instead of one contaminated one.

**wasmtime is both the baseline and the correctness oracle.** Each runner's stdout is compared byte-for-byte against wasmtime at that runner's own iteration count; a mismatch fails the run. wardite computes f32 in double precision and never re-rounds, so `f32.add(0.1, 0.2)` yields `0.30000000447034836` — a benchmark that only timed results would have rewarded it for being fast and wrong.

**Workloads stay inside wardite's implemented subset** — i32/i64/f64 and four WASI functions — so one binary is measured on every runner. Constraining the workload is what makes the comparison a comparison.

**Every pair appears in the output.** Unavailable runners and declared exclusions are reported with their reason in both the JSON and the doc. A gap is stated, never omitted, so a missing row cannot read as a covered one.

## Rejected alternatives

**A fixed iteration count per workload.** The honest, obvious shape, and unusable at a 23000x spread: sizing for Bash leaves wasmtime measuring its own process startup.

**A declared table of per-runner divisors.** Avoids calibration's runtime cost, but the divisors are guessed constants that silently rot the moment a backend gets faster — and making backends faster is the point.

**hyperfine.** The standard tool, and it does the statistics well. It has no notion of calibrating a workload parameter per command, and no notion of an output oracle, so the two things this harness exists to do would both have to be bolted on around it. It also adds a required external binary.

**Criterion.** In-process Rust microbenchmarking. Everything measured here is an external process in another language.

**Letting each runtime run whatever it runs well.** Maximally flattering to everyone and comparable to nothing.

## Consequences

- A full sweep takes tens of minutes and is deliberately outside `cargo test`. It is run when numbers are published, not on every change.
- The caps in `KERNEL_ITER_CAPS` must be retuned when a kernel body changes; they are set to roughly 3x what wasmtime needs for the default target.
- The suite has **no f32 coverage at all**, because wardite's f32 is broken. This is a real gap in what the numbers describe.
- `wordcount` generates its input buffer before reading `argv[1]`, so its `t(0)` is startup plus that setup, not startup alone. Its per-iteration figures are unaffected; its cold-start column is not comparable to the other kernels'.
- Three of the workloads are also drawn, as generated SVG lollipop charts under `docs/benchmarks/` (two files per chart, light and dark, since GitHub's sanitizer cannot be trusted with CSS inside an SVG). A 23000x span forces a log axis, which rules out both Mermaid's `xychart` and any bar form — a bar's length is measured from a zero the axis does not have.
- Published numbers are host-specific and dated. `docs/benchmarks.md` is generated and states the host, every runtime version, and the date; it is not hand-edited.
