# Decision 81: Loop-Body Extraction into Per-Iteration Functions

Status: **Accepted, 2026-08-21.**
The shared pass lives in [`crates/dewasm-backend/src/extract.rs`](../../crates/dewasm-backend/src/extract.rs); the Ruby backend is the only consumer, with its thresholds in `EXTRACT_PARAMS` (`crates/dewasm-backend-ruby/src/lib.rs`).
Regions containing a `return` or an outward branch are not yet extractable, which leaves the NES example and sqlite3-shell's interpreter loop uncaptured; see Consequences.

## Context

YJIT and ZJIT compile a method only when it is entered through a call, never mid-execution (no on-stack replacement).
A hot loop inside a function that is entered once is therefore interpreted forever: the converted `c/mandelbrot` benchmark runs its whole workload inside one function and measures identically with and without `--yjit` (9.2 s / 9.1 s), while its hand-split counterpart with the loop body behind a call runs 3.1 s under `--yjit`.
Decision 58 recorded the no-OSR measurement and rejected per-state extraction on it; that rejection covers 5-to-10-line dispatch states, which are too small to carry a call and leave the dispatch loop itself uncompiled.
The shape that wins is different: a loop body large enough to amortize the call, containing its inner loops, extracted whole so the JIT compiles the work.

Lowering the compile threshold instead of restructuring was measured and does not work: `--yjit-call-threshold=1` changes nothing on `c/mandelbrot` (the running method is never recompiled) and makes `app/sqlite3_query` 1.52x slower (cold methods get compiled for nothing).

## Decision

- **An IR-to-IR pass extracts a contiguous, branch-closed span of a loop body into a new function, called once per iteration.**
  Branch-closed means every branch inside the span lands on a frame opened inside it; a `return`, a branch to the loop head, or a branch past the loop pins the span's boundary.
  The span need not start at the body's first statement: a head-tested loop opens with its own exit branch, which stays behind.
- **The span may leave at most one value live for the rest of the function, returned from the call.**
  Live-outs are found by a backward may-liveness over the structured body (loops to a fixpoint); parameters are the variables possibly read before the span assigns them.
  A spilled stack temp whose incoming value the span may read (possible once a span starts mid-body: earlier statements compute into temps) is passed as a trailing parameter and copied into its temp at the extracted function's entry, so the span body needs no rewriting beyond local renumbering.
- **Inside a `try_table`, a throw-capable span is not extracted**: an exception would skip the write-back of values the catch handler could observe.
- **Thresholds are per backend** ([`extract::Params`]: minimum span weight in IR nodes, maximum parameter count, maximum live-out count, and a higher weight floor for spans consuming incoming temps).
  Ruby uses weight 40, 34 parameters, 1 live-out, and temp-consuming spans need weight 160.
  The parameter budget was raised from 12 after a 2026-08 sweep (12/24/34/48, with and without exit-carrying spans): 34 is the smallest value admitting the NES frame loop (30 parameters, +3.4% ticks/sec under YJIT), YJIT showed no arity cliff (~0.26 ns per extra parameter, zero side exits up to 64), and values above 34 changed only sqlite3-shell's span set with no measured gain and the largest interpreter-mode cost (+0.4% on sqlite3_query).
  The weight floor is load-bearing in both directions: at 80 the second `c/sha256` extraction disappears and `--yjit` regresses from 10.6 s to 13.2 s, and small tight-loop bodies (the `wat/` microbenchmarks) must stay unextracted or the interpreter pays a call per iteration; at 24 the suite's outputs are byte-identical to 40.
  The separate temp floor is equally load-bearing: without it, the temp-consuming spans unlocked in the DOOM module regress its smoke run from 16.7 to 12.7 ticks/sec, while with it the DOOM extraction set keeps the gain and `c/sha256` consolidates its two extractions into one larger span, improving `--yjit` from 10.62 s to 10.20 s.
- **The pass rewrites a copied function list; the shared module is not mutated.**
  `extract()` returns the defined functions with spans replaced by calls, the extracted functions appended, and the type list extended; a backend swaps that list in at emission time.

## Rejected alternatives

- **Per-state extraction of dispatch states.**
  Rejected with measurements in decision 58; the granularity is wrong, not the idea of a method boundary.
- **Multiple live-outs via an array return.**
  A Ruby method returning a pair allocates an Array per call, in the hottest place the program has.
  The `max_results` parameter exists so a backend whose language returns tuples without allocation can raise it.
- **Documenting a lower `--yjit-call-threshold` for converted programs instead.**
  Measured: no effect where it was hoped to help and a 1.52x regression on `app/sqlite3_query` (see Context).
- **Splitting the generated Ruby text.**
  The IR already has the structure; recovering scopes, liveness and types from emitted text repeats decision 58's rejected text pass with more ways to be wrong.
- **Extracting spans that return or branch outward, via a signal protocol.**
  The call would have to report which exit was taken as well as the value, which needs a second return slot (an allocation per iteration) or a sentinel encoding with its own masking questions.
  Deferred, not refuted; see Consequences.

## Consequences

**Positive.**
Measured on Ruby 4.0.4 (arm64), alternating runs:

- `c/mandelbrot` 2 M iterations: `--yjit` 9.09 s → 3.09 s (2.9x); interpreter unchanged (9.21 s → 9.12 s).
- `c/sha256` 300 k iterations: `--yjit` 14.27 s → 10.62 s (1.34x); interpreter 23.65 s → 24.45 s (a 3.4% cost).
- DOOM example smoke run: 16.2 → 16.7 ticks/sec under `--yjit`.
- `app/sqlite3_query`: neutral (9.56 s → 9.48 s `--yjit`, interpreter unchanged), with 242 functions extracted; the interpreter loop's own body branches outward everywhere and is not captured.
- The `wat/` microbenchmark outputs are byte-identical: the dangerous tight-loop case is structurally refused, not just discouraged.

**Negative.**
Conversion time grows: ruby.wasm 14.4 s → 21.9 s in a debug build (+52%), dominated by the liveness fixpoints.
The interpreter pays the call where a span is extracted (the sha256 3.4%); the weight floor keeps the payment small.
Method counts grow (sqlite3-shell 1553 → 1817, ruby.wasm 17711 → 18123 generated methods).

**Carry-over.**
A loop whose body returns from the middle (the NES example's frame-completion exit, `c/wordcount`, sqlite3-shell's interpreter loop) gets nothing today; the signal-protocol alternative above is the known route and needs its own measurement.
For sqlite3-shell's interpreter function specifically, compiling it by other means was tried and lost: the step-lambda experiment (`agents/experiments.md`, step-lambda-dispatch) measured closure-environment access and large-compiled-method costs exceeding the JIT gain, so a future attempt needs a different shape (small per-opcode functions), not a revival of that one.
The liveness cost is unoptimized (per-function boundary recording is capped at 256 leading statements per loop body, nothing else is pruned); skipping functions with no candidate loop is the obvious next cut.
Only Ruby consumes the pass; a backend whose runtime compiles hot loops in place (PyPy traces loops mid-execution) has no reason to.
