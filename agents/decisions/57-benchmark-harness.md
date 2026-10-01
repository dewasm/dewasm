# Decision 57: Benchmark by Calibrated Per-Runner Iteration Counts, Net of a Measured Baseline

**Status:** Accepted (2026-08-02).
Landed: `cargo xtask bench` and the workloads under `benchmarks/`.
The generated `docs/benchmarks/results.md` landed with them.
Not covered: any CI enforcement.
Timings never repeat with identical bytes, so nothing here is a compared snapshot.

## Context

Every neighbouring project publishes numbers, and dewasm published none.
That made the central claim unfalsifiable: AOT source output beats an interpreter loop.
The direct competitors are [`pywasm`](https://github.com/mohanson/pywasm) (pure Python) and [`wardite`](https://github.com/udzura/wardite) (pure Ruby).
Wasmtime is the upper limit.

Measuring this the plain way is wrong in five ways, each observed on this host rather than assumed:

1. **The spread is a factor of ~23000.**
   On `wat/i32_alu`, Wasmtime costs 2.4 ns per iteration and `pywasm` under CPython costs 55 µs.
   At one end, any single fixed iteration count measures nothing but process start.
   At the other end, it takes minutes per sample.
2. **Fixed start cost differs per runner by more than the compute under test.**
   An empty Ruby process costs ~50 ms.
   Parsing the 17 MB of Ruby generated from `sqlite3-shell.wasm` costs a further ~730 ms.
   That cost comes before any work happens.
   Wasmtime loads the same module in ~15 ms.
3. **Wasmtime caches compiled code on disk by default.**
   The same SQL workload takes 0.02 s warm and 0.25 s with `-C cache=n`.
   That is a 12x difference in the baseline everything else is divided by.
4. **Microbenchmark shape decides whether a JIT appears at all.**
   Ruby's YJIT has no on-stack replacement.
   So a loop that never returns from its single call is never compiled.
   The same 30M iterations take 1.70 s in one call and 0.17 s split across 3000 calls.
   That 10x difference is in the *host*, not in the generated code.
5. **The ratio is a property of the workload, not of the backend.**
   dewasm-generated Ruby is 148x Wasmtime on a 10k-row insert and ~570x on a 300k-row index-and-join.
   Publishing either alone as "the" number is a claim the next workload will prove wrong.

## Decision

**Calibrate the iteration count per (workload, runner) pair and report `ns/op`.**
Each pair measures `t(0)` first.
Then it raises `N` until `t(N) - t(0)` reaches a target (default 300 ms).
A per-workload cap bounds how far `N` rises.
The cap exists only for a workload whose per-iteration cost is not constant.
It stops such a workload from being driven to a size that makes no sense.
It binds only the fastest runners.

**For microbenchmarks, subtract `t(0)` and report it as its own column.**
Every microbenchmark accepts `<iterations>` from `argv[1]` and does no work at `0`.
At `0` it still prints its result line.
That single run measures process start plus module load.
So `t(N) - t(0)` is compute, and `t(0)` is cold start.
Cold start is the one axis where an interpreter legitimately beats an AOT compiler.
Problems 2 and 3 become two reported numbers instead of one mixed one.
The subtraction is structurally necessary here and only here.
That is because the iteration count is calibrated per runner.
So per-iteration cost is undefined until the fixed start cost is removed.

**Apps report whole wall time only, with no `t(0)`.**
An app has no iteration parameter to calibrate.
Its published quantities (the chart and the `vs wasmtime` ratio) are the whole run.
The whole run is what a user of the converted program actually experiences.
The `t(0)` subtraction was originally carried over to apps to keep one shape, not by need.
For `cowsay` it produced a near-zero number.
For other apps it produced a split that other numbers already tell.
Those are the microbenchmark cold-start columns and the `cowsay` case.
So the app tables leave it out.

**Wasmtime is both the baseline and the correctness oracle.**
Each runner's `stdout` is compared with Wasmtime's, and the bytes must be identical.
The comparison runs at that runner's own iteration count, and a mismatch fails the run.
`wardite` computes f32 in double precision and never re-rounds.
So `f32.add(0.1, 0.2)` yields `0.30000000447034836`.
A benchmark that only timed results would have rewarded it for being fast and wrong.

**Workloads stay inside what every runner supports.**
So one binary is measured on all of them.
Constraining the workload is what makes the comparison a comparison.
`wardite` sets most of the bound:

- no f32: it computes f32 in double precision without re-rounding (silently wrong, not an error);
- no multi-value, typed `select`, reference types, `table.*` instructions or `data.drop`;
- no WASI beyond `args_get`/`args_sizes_get`/`fd_write`/`proc_exit`.

Imports resolve at instantiation.
So merely linking `wasi-libc` stdio (which imports `fd_seek`) makes a module unloadable there.
Two runtime caps come from elsewhere.
`pywasm` asserts at wasm call depth 1024.
wasm3 rejects WASI output parameters at linear-memory address 0.
So the `.wat` workloads place their working memory from `0x1000`.
The `zig cc` flags that keep the C workloads inside this set are in `benchmarks/c/build.sh`.
That file documents each flag.

**Every pair appears in the output.**
Unavailable runners and declared exclusions are reported with their reason.
Both the JSON and the document report them.
A gap is stated, never left out, so a missing row cannot read as a covered one.

## Rejected alternatives

**A fixed iteration count per workload.**
The honest, obvious shape, and unusable at a 23000x spread.
That is because sizing for Bash leaves Wasmtime measuring its own process start.

**A declared table of per-runner scale factors.**
Avoids calibration's runtime cost, but the factors are guessed constants.
They silently go out of date when a backend gets faster, and making backends faster is the point.

**`hyperfine`.**
The standard tool, and it does the statistics well.
It has no notion of calibrating a workload parameter per command, and no notion of an output oracle.
So the two things this harness exists to do would both have to be added on around it.
It also adds a required external binary.

**Criterion.**
In-process Rust microbenchmarking.
Everything measured here is an external process in another language.

**Letting each runtime run whatever it runs well.**
Each runtime would look its best, and nothing would be comparable.

## Consequences

- A full benchmark run takes tens of minutes and is deliberately outside `cargo test`.
  It is run when numbers are published, not on every change.
- The caps in `MICRO_ITER_CAPS` must be retuned when a microbenchmark body changes.
  They are set to roughly 3x what Wasmtime needs for the default target.
- The suite has **no f32 coverage at all**, because `wardite`'s f32 is broken.
  This is a real gap in what the numbers describe.
- `c/wordcount` generates its input buffer before reading `argv[1]`.
  So its `t(0)` is start plus that input generation, not start alone.
  Its per-iteration figures are unaffected; its cold-start column is not comparable to the others.
- Every workload is also drawn as a generated SVG lollipop chart under `docs/benchmarks/figs/`.
  Each chart is two files, light and dark.
  Two files are needed since GitHub's sanitizer cannot be trusted with CSS inside an SVG.
  The workload's table is folded into a `<details>` underneath the chart.
  A 23000x span forces a log axis.
  The log axis rules out both Mermaid's `xychart` and any bar form.
  That is because a bar's length is measured from a zero the axis does not have.
  The axis is seconds on every chart and never a ratio.
  It is seconds per iteration for a microbenchmark and seconds per run for an app.
  So two charts can be read against each other.
  A ratio axis can only be read against its own baseline.
- The other runtimes that consume the `.wasm` directly are Wasmer, WasmEdge, `wazero`, and wasm3.
  They are measured as ordinary runners and cross-checked like everything else.
  They widen the range the numbers sit in without changing the decision above.
  Wasmtime alone remains the baseline and the oracle.
- Published numbers are host-specific and dated.
  `docs/benchmarks/results.md` is generated and is not hand-edited.
  It states the host, every runtime version, and the date.
