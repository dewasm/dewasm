# Decision 81: Loop-Body Extraction into Per-Iteration Functions

Status: **Accepted, 2026-08-21.**
The shared pass lives in [`crates/dewasm-backend/src/extract.rs`](../../crates/dewasm-backend/src/extract.rs).
The Ruby backend is the only consumer.
Its thresholds are in `EXTRACT_PARAMS` (`crates/dewasm-backend-ruby/src/lib.rs`).
Regions containing a `return` or a branch out of the region are not yet extractable.
That leaves the NES example and sqlite3-shell's interpreter loop uncaptured; see Consequences.

## Context

YJIT and ZJIT compile a method only when it is entered through a call.
They never compile it mid-execution (no on-stack replacement).
A hot loop inside a function that is entered once is therefore interpreted forever.
The converted `c/mandelbrot` benchmark runs its whole workload inside one function.
It measures identically with and without `--yjit` (9.2 s / 9.1 s).
By contrast, its hand-split version with the loop body behind a call runs 3.1 s under `--yjit`.
Decision 58 measured the lack of on-stack replacement and rejected per-state extraction on it.
That rejection covers 5-to-10-line dispatch states.
Those states are too small to carry a call and leave the dispatch loop itself uncompiled.
The shape that wins is different.
It is a loop body with its inner loops, large enough that the call costs little beside the work.
The body is extracted whole so the JIT compiles the work.

Lowering the compile threshold instead of restructuring was measured and does not work.
`--yjit-call-threshold=1` changes nothing on `c/mandelbrot`.
There the running method is never recompiled.
It also makes `app/sqlite3_query` 1.52x slower (cold methods get compiled for nothing).

## Decision

- **An IR-to-IR pass extracts an unbroken, branch-closed span of a loop body into a new function.**
  **The new function is called once per iteration.**
  Branch-closed means every branch inside the span lands on a frame opened inside it.
  A `return`, a branch to the loop head, or a branch past the loop sets the span's boundary.
  The span need not start at the body's first statement.
  A head-tested loop opens with its own exit branch, which stays behind.
- **The span may leave at most one value live for the rest of the function, returned from the call.**
  Live-outs are found by a backward may-liveness over the structured body (loops to a fixed point).
  Parameters are the variables possibly read before the span assigns them.
  The span may read the incoming value of a spilled stack temp.
  That is possible once a span starts mid-body: earlier statements compute into temps.
  Such a temp is passed as a trailing parameter.
  It is copied into its temp at the extracted function's entry.
  So the span body needs no rewriting beyond local renumbering.
- **Inside a `try_table`, a throw-capable span is not extracted.**
  An exception would skip the write-back of values the catch handler could observe.
- **Thresholds are per backend.**
  [`extract::Params`] holds these thresholds:
  - minimum span weight in IR nodes;
  - maximum parameter count;
  - maximum live-out count;
  - a higher weight floor for spans consuming incoming temps.

  Ruby uses weight 40, 34 parameters, 1 live-out, and temp-consuming spans need weight 160.
  The parameter budget was raised from 12 after a measurement in 2026-08.
  It tried 12/24/34/48, with and without exit-carrying spans.
  34 is the smallest value admitting the NES frame loop (30 parameters, +3.4% ticks/sec under YJIT).
  YJIT showed no sudden cost at any parameter count: ~0.26 ns per extra parameter.
  It took zero side exits up to 64 parameters.
  Values above 34 changed only sqlite3-shell's span set, with no measured gain.
  They also had the largest interpreter-mode cost (+0.4% on sqlite3_query).
  The weight floor is load-bearing in both directions.
  At 80 the second `c/sha256` extraction disappears, and `--yjit` slows from 10.6 s to 13.2 s.
  Small tight-loop bodies (the `wat/` microbenchmarks) must stay unextracted.
  Otherwise the interpreter pays a call per iteration.
  At 24 the suite's outputs are byte-identical to 40.
  The separate temp floor is equally load-bearing.
  Without it, the temp-consuming spans unlocked in the DOOM module slow its smoke run.
  The run drops from 16.7 to 12.7 ticks/sec.
  With it, the DOOM extraction set keeps the gain.
  `c/sha256` also merges its two extractions into one larger span.
  That improves `--yjit` from 10.62 s to 10.20 s.
- **The pass rewrites a copied function list; the shared module is not changed.**
  `extract()` returns the defined functions with spans replaced by calls.
  The extracted functions are added at the end, and the type list is extended.
  A backend swaps that list in at emission time.

## Rejected alternatives

- **Per-state extraction of dispatch states.**
  Rejected with measurements in decision 58.
  The unit size is wrong, not the idea of a method boundary.
- **Multiple live-outs via an array return.**
  A Ruby method returning a pair allocates an Array per call, in the hottest place the program has.
  The `max_results` parameter exists for a backend whose language returns tuples without allocation.
  Such a backend can raise it.
- **Documenting a lower `--yjit-call-threshold` for converted programs instead.**
  Measured: no effect where it was hoped to help.
  It also showed a 1.52x regression on `app/sqlite3_query` (see Context).
- **Splitting the generated Ruby text.**
  The IR already has the structure.
  Recovering scopes, liveness and types from emitted text repeats decision 58's rejected text pass.
  It does so with more ways to be wrong.
- **Extracting spans that return or branch out of the span, via a signal protocol.**
  The call would have to report which exit was taken as well as the value.
  That needs a second return slot (an allocation per iteration) or a special-value encoding.
  A special-value encoding has its own masking questions.
  Deferred, not shown wrong; see Consequences.

## Consequences

**Positive.**
Measured on Ruby 4.0.4 (arm64), with before and after runs taken in turn:

- `c/mandelbrot` 2 M iterations: `--yjit` 9.09 s → 3.09 s (2.9x); interpreter unchanged (9.21 s → 9.12 s).
- `c/sha256` 300 k iterations: `--yjit` 14.27 s → 10.62 s (1.34x).
  Interpreter 23.65 s → 24.45 s (a 3.4% cost).
- DOOM example smoke run: 16.2 → 16.7 ticks/sec under `--yjit`.
- `app/sqlite3_query`: neutral (9.56 s → 9.48 s `--yjit`, interpreter unchanged).
  It has 242 functions extracted.
  The interpreter loop's own body branches out of the loop everywhere and is not captured.
- The `wat/` microbenchmark outputs are byte-identical.
  The dangerous tight-loop case is structurally refused, not only warned against.

**Negative.**
Conversion time grows: ruby.wasm 14.4 s → 21.9 s in a debug build (+52%).
The fixed-point iteration of the liveness analysis dominates the growth.
The interpreter pays the call where a span is extracted (the sha256 3.4%).
The weight floor keeps the payment small.
Method counts grow (sqlite3-shell 1553 → 1817, ruby.wasm 17711 → 18123 generated methods).

**Carry-over.**
A loop whose body returns from the middle gets nothing today, for example:

- the NES example's frame-completion exit;
- `c/wordcount`;
- sqlite3-shell's interpreter loop.

The signal-protocol alternative above is the known route and needs its own measurement.
Compiling sqlite3-shell's interpreter function by other means was tried and lost.
The attempt was the `step-lambda` experiment (`agents/experiments.md`, `step-lambda-dispatch`).
It measured closure-environment access and large-compiled-method costs exceeding the JIT gain.
So a future attempt needs small functions, one per instruction, not a retry of that one.
The liveness cost is unoptimized.
Per-function boundary recording is capped at 256 leading statements per loop body.
Nothing else is skipped.
Skipping functions with no candidate loop is the obvious next cut.
Only Ruby consumes the pass.
A backend whose runtime compiles hot loops in place has no reason to.
For example, PyPy traces loops mid-execution.
