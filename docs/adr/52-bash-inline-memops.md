# ADR-52 — Bash Emitter Inlines Linear-Memory Loads and Stores

Status: **Accepted, 2026-07-30.** The Bash emitter generates per-instruction loads/stores as inline arithmetic on the module's memory array instead of `mem_*` unit calls; the units remain for WASI and bulk/rare ops. DOOM's tick dropped 87s → 34s (2.5x) and initGame 198s → 103s on top of ADR-51's representation change, with identical framebuffer checksums and the full test suite passing (spec 257/257).

## Context

After ADR-51 made random access O(1), the next cost class is bash function-call overhead: a call-based i32 load measured ~41k ops/sec while the same composition inlined measured ~85k. Every generated load also paid a second call (`mem_check`) and an R0-hop into its destination. ADR-1's ordering applies: this changes emitted shape, not semantics, and the spec harness tests it.

## Decision

Inline the 16 integer load/store variants and the memory-access half of f32/f64 (bit-conversion stays in Rt): snapshot the effective address (base + folded static offset, computed in the unsigned-33-bit range that fits bash's signed 64) into one var, emit the ADR-11-conforming bounds check `if (( ea + N > <p>pages * 65536 )); then rt_trap 'out of bounds memory access'; return $?; fi`, then compose the value with arithmetic-expanded subscripts `M[$((ea+k))]` — measured faster (85k vs 73k ops/sec) than hoisting per-byte key temps, and the pre-expansion keeps ADR-51's canonical-decimal-key invariant. Loads assign straight into their destination, eliminating the `R0` hop; stores are one short assignment per byte (comma-chaining assoc writes inside one `(( ))` is impossible, and giant single `(( ))` statements measure slower anyway). Criterion, extending ADR-51's: *emitted-shape choices are settled by microbenchmark at representative scale before the emitter changes* — intuition inverted twice here (temp-key hoisting lost; a `declare -gn` memory alias lost to the direct array name).

Memory naming needs no new machinery: generated code references `<p>mem`/`<p>pages` literally — a 0-hop direct array for local memory, and the depth-1 nameref that `<p>_init` already establishes for imported memory (ADR-35), through which stores write and `memory.grow` on the owner is reflected (verified, and exercised by the spec linking tests).

`mem_copy`/`fill`/`init`/`grow`/`size` stay unit calls (bulk or rare), and the `mem_*` load/store units themselves remain — WASI units and cross-module paths still call them, and the units lint keeps binding them.

## Rejected alternatives

- **Per-module `declare -gn` memory alias** (the design's initial sketch): adds a nameref hop to the dominant local-memory case, measured ~72k vs 85k ops/sec — slower than just naming the array.
- **Pre-computed temp keys per byte** (`a1=$((ea+1)); M[$a1]`): the ADR-51 unit idiom, but at emit sites the inline `$((ea+k))` form is both faster and shorter; units keep the temp-key style for readability where the call overhead already dominates.
- **Inlining WASI/bulk paths too**: destroys the unit structure (ADR-6) for paths where per-call overhead is amortized over many bytes; no measured win to justify it.

## Consequences

- Positive: DOOM tick 2.5x, initGame 1.9x; every converted module's hot loops shed two function calls plus an R0 copy per memory access. Remaining bash cost is arithmetic/control-flow, not call overhead.
- Negative: generated files grow (DOOM: 16.7MB → 19.1MB, +14%; source time +7.6%); load/store semantics now exist in two places (units and emitter helpers) — the spec suite is the guard against drift.
- Carry-over: word-packed cells (ADR-51's rejected alternative) remain the next representation-level lever if ever needed; the function-return `R0` hop for value-returning bodies is untouched (it is the return mechanism, not load overhead).
