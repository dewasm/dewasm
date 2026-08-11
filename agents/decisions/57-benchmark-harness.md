# Decision 57: Benchmark by Calibrated Per-Runner Iteration Counts, Net of a Measured Baseline

**Status:** Accepted (2026-08-02).
Landed: `cargo xtask bench`, the workloads under `benchmarks/`, and the generated `docs/benchmarks/results.md`.
Not covered: any CI enforcement.
Timings are not reproducible byte-for-byte, so nothing here is a compared snapshot.

## Context

Every neighbouring project publishes numbers and dewasm published none, which made the central claim (that AOT source output beats an interpreter loop) unfalsifiable.
The direct competitors are [pywasm](https://github.com/mohanson/pywasm) (pure Python) and [wardite](https://github.com/udzura/wardite) (pure Ruby); wasmtime is the ceiling.

Measuring this naively is wrong in five ways, each observed on this host rather than assumed:

1. **The spread is a factor of ~23000.**
   On `wat/i32_alu`, wasmtime costs 2.4 ns per iteration and pywasm under CPython costs 55 µs.
   Any single fixed iteration count either measures nothing but process startup at one end or takes minutes per sample at the other.
2. **Fixed startup cost differs per runner by more than the compute under test.**
   An empty Ruby process costs ~50 ms; parsing the 17 MB of Ruby generated from `sqlite3-shell.wasm` costs a further ~730 ms before any work happens.
   wasmtime loads the same module in ~15 ms.
3. **wasmtime caches compiled code on disk by default.**
   The same SQL workload takes 0.02 s warm and 0.25 s with `-C cache=n`, a 12x difference in the baseline everything else is divided by.
4. **Microbenchmark shape decides whether a JIT appears at all.**
   Ruby's YJIT has no on-stack replacement, so a loop that never returns from its single invocation is never compiled.
   The same 30M iterations take 1.70 s in one call and 0.17 s split across 3000 calls, a 10x difference in the *host*, not in the generated code.
5. **The ratio is a property of the workload, not of the backend.**
   dewasm-generated Ruby is 148x wasmtime on a 10k-row insert and ~570x on a 300k-row index-and-join.
   Publishing either alone as "the" number is a claim the next workload will refute.

## Decision

**Calibrate the iteration count per (workload, runner) pair and report `ns/op`.**
Each pair measures `t(0)` first, then ramps `N` until `t(N) - t(0)` reaches a target (default 300 ms), bounded by a per-workload cap.
The cap exists only to stop a workload whose per-iteration cost is not constant from being driven somewhere absurd; it binds only the fastest runners.

**For microbenchmarks, subtract `t(0)` and report it as its own column.**
Every microbenchmark accepts `<iterations>` from `argv[1]` and does no work at `0` while still printing its result line.
That single run measures process startup plus module load, so `t(N) - t(0)` is compute and `t(0)` is cold start, the one axis where an interpreter legitimately beats an AOT compiler.
Hazards 2 and 3 become two reported numbers instead of one contaminated one.
The subtraction is structurally necessary here and only here: the iteration count is calibrated per runner, so per-iteration cost is undefined until the fixed startup is removed.

**Apps report whole wall time only, with no `t(0)`.**
An app has no iteration parameter to calibrate, and its published quantities (the chart and the `vs wasmtime` ratio) are the whole run, which is what a user of the converted program actually experiences.
The `t(0)` subtraction was originally carried over to apps by symmetry, not need; it produced either a near-zero number (cowsay) or a decomposition the microbenchmark cold-start columns and the cowsay case already tell, so the app tables omit it.

**wasmtime is both the baseline and the correctness oracle.**
Each runner's stdout is compared byte-for-byte against wasmtime at that runner's own iteration count; a mismatch fails the run.
wardite computes f32 in double precision and never re-rounds, so `f32.add(0.1, 0.2)` yields `0.30000000447034836`.
A benchmark that only timed results would have rewarded it for being fast and wrong.

**Workloads stay inside the intersection every runner supports**, so one binary is measured on all of them; constraining the workload is what makes the comparison a comparison.
wardite sets most of the bound: no f32 (it computes f32 in double precision without re-rounding, silently wrong, not an error), no multi-value, typed `select`, reference types, `table.*` instructions or `data.drop`, and no WASI beyond `args_get`/`args_sizes_get`/`fd_write`/`proc_exit`.
Imports resolve at instantiation, so merely linking wasi-libc stdio (which imports `fd_seek`) makes a module unloadable there.
Two runtime caps come from elsewhere: pywasm asserts at wasm call depth 1024, and wasm3 rejects WASI out-params at linear-memory address 0, so the `.wat` workloads lay their scratch from `0x1000`.
The `zig cc` flags that keep the C workloads inside this set are documented per-flag in `benchmarks/c/build.sh`.

**Every pair appears in the output.**
Unavailable runners and declared exclusions are reported with their reason in both the JSON and the doc.
A gap is stated, never omitted, so a missing row cannot read as a covered one.

## Rejected alternatives

**A fixed iteration count per workload.**
The honest, obvious shape, and unusable at a 23000x spread: sizing for Bash leaves wasmtime measuring its own process startup.

**A declared table of per-runner divisors.**
Avoids calibration's runtime cost, but the divisors are guessed constants that silently rot the moment a backend gets faster, and making backends faster is the point.

**hyperfine.**
The standard tool, and it does the statistics well.
It has no notion of calibrating a workload parameter per command, and no notion of an output oracle, so the two things this harness exists to do would both have to be bolted on around it.
It also adds a required external binary.

**Criterion.**
In-process Rust microbenchmarking.
Everything measured here is an external process in another language.

**Letting each runtime run whatever it runs well.**
Maximally flattering to everyone and comparable to nothing.

## Consequences

- A full benchmark run takes tens of minutes and is deliberately outside `cargo test`.
  It is run when numbers are published, not on every change.
- The caps in `MICRO_ITER_CAPS` must be retuned when a microbenchmark body changes; they are set to roughly 3x what wasmtime needs for the default target.
- The suite has **no f32 coverage at all**, because wardite's f32 is broken.
  This is a real gap in what the numbers describe.
- `c/wordcount` generates its input buffer before reading `argv[1]`, so its `t(0)` is startup plus that setup, not startup alone.
  Its per-iteration figures are unaffected; its cold-start column is not comparable to the others..
- Every workload is also drawn, as a generated SVG lollipop chart under `docs/benchmarks/figs/` (two files per chart, light and dark, since GitHub's sanitizer cannot be trusted with CSS inside an SVG), with its table folded into a `<details>` underneath.
  A 23000x span forces a log axis, which rules out both Mermaid's `xychart` and any bar form: a bar's length is measured from a zero the axis does not have.
  The axis is seconds on every chart and never a ratio (seconds per iteration for a microbenchmark, seconds per run for an app), so two charts can be read against each other; a ratio axis can only be read against its own baseline.
- Runtimes other than wasmtime that consume the `.wasm` directly (wasmer, wasmedge, wazero, wasm3) are measured as ordinary runners and cross-checked like everything else.
  They widen the range the numbers sit in without changing the decision above: wasmtime alone remains the baseline and the oracle.
- Published numbers are host-specific and dated.
  `docs/benchmarks/results.md` is generated and states the host, every runtime version, and the date; it is not hand-edited.
