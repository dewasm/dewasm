# Decision 52: Bash Emitter Inlines Linear-Memory Loads and Stores

Status: **Accepted, 2026-07-30.**
The Bash emitter inlines per-instruction loads/stores as arithmetic on the module's memory array.
It no longer emits `mem_*` unit calls for them; the units remain for WASI and bulk/rare operations.
This comes on top of decision 51's representation change.
DOOM's tick dropped 87s → 34s (2.5x) and `initGame` 198s → 103s.
Framebuffer checksums are identical, and the full test suite passes.
The specification testsuite passes 257 of 257.

## Context

After decision 51 made random access O(1), the next cost class is Bash function-call overhead.
A call-based i32 load measured ~41k operations/sec; the same composition inlined measured ~85k.
Every generated load also paid a second call (`mem_check`) and a step through `R0` into its destination.
Decision 1's ordering applies: this changes emitted shape, not semantics.
The specification harness tests it.

## Decision

Inline the 16 integer load/store variants and the memory-access half of f32/f64.
Bit-conversion stays in Rt.
An inlined access takes three steps:
1. Snapshot the effective address into one variable.
   It is base + folded static offset, computed in the unsigned-33-bit range that fits Bash's signed 64.
2. Emit the decision 11-conforming bounds check:
   `if (( ea + N > <p>pages * 65536 )); then rt_trap 'out of bounds memory access'; return $?; fi`.
3. Compose the value with arithmetic-expanded subscripts `M[$((ea+k))]`.
   This measured faster (85k vs. 73k operations/sec) than hoisting temporary per-byte keys.
   The pre-expansion keeps decision 51's canonical-decimal-key invariant.

Loads assign straight into their destination, eliminating the step through `R0`.
Stores are one short assignment per byte.
Comma-chaining associative array writes inside one `(( ))` is impossible.
Giant single `(( ))` statements measure slower anyway.
Criterion, extending decision 51's: *emitted-shape choices are settled by microbenchmark*.
*The microbenchmark runs at representative scale before the emitter changes*.
The expected result was wrong twice here.
Hoisting temporary keys lost, and a `declare -gn` memory alias lost to the direct array name.

Memory naming needs no new machinery: generated code references `<p>mem`/`<p>pages` literally.
For local memory, that is a direct array, with no step between.
For imported memory, it is the depth-1 nameref that `<p>_init` already establishes (decision 35).
Stores write through that nameref, and `memory.grow` on the owner is reflected through it.
This is verified, and exercised by the specification linking tests.

`mem_copy`/`fill`/`init`/`grow`/`size` stay unit calls (bulk or rare).
The `mem_*` load/store units themselves remain.
That is because WASI units and cross-module paths still call them.
The units lint also keeps binding them.

## Rejected alternatives

- **Per-module `declare -gn` memory alias** (the design's initial sketch).
  It adds a nameref step to the dominant local-memory case.
  It measured ~72k vs. 85k operations/sec, slower than just naming the array.
- **Pre-computed temporary keys per byte** (`a1=$((ea+1)); M[$a1]`).
  This is the usual form of the decision 51 units.
  At emit sites, the inline `$((ea+k))` form is both faster and shorter.
  Units keep the temporary-key style for readability, where the call overhead already dominates.
- **Inlining WASI/bulk paths too**: destroys the unit structure (decision 6) on those paths.
  There per-call overhead is spread over many bytes; no measured win justifies it.

## Consequences

- Positive: DOOM tick 2.5x, `initGame` 1.9x.
  Every converted module's hot loops drop two function calls plus an R0 copy per memory access.
  Remaining Bash cost is arithmetic/control-flow, not call overhead.
- Negative: generated files grow (DOOM: 16.7MB → 19.1MB, +14%; source time +7.6%).
  Load/store semantics now exist in two places (units and emitter helpers).
  The specification testsuite is the guard against drift.
- Carry-over: if ever needed, the next representation-level lever remains word-packed cells.
  Those are decision 51's rejected alternative.
  The function-return step through `R0` for value-returning bodies is untouched.
  It is the return mechanism, not load overhead.
