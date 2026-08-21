# Decision 83: Byte-Scatter Store Fusion Behind a Runtime Precondition

Status: **Accepted, 2026-08-21.**
The shared pass lives in [`crates/dewasm-backend/src/fuse.rs`](../../crates/dewasm-backend/src/fuse.rs); the Ruby backend runs it first, before load hoisting (decision 82) and loop-body extraction (decision 81), so the hoisting pass sees one store where the idiom fired and emits one guard instead of four.
The recognizer covers the one idiom shape measured hot (the four-byte little-endian scatter); widening it is driven by profiles, not speculation.

## Context

Portable C writes a 32-bit pixel byte by byte (`dest[i] = v >> (i * 8)`) to be endian-independent, and compiled to wasm that survives as a four-iteration loop of `store8(base + idx, word >> shift)`.
In the DOOM module's hottest function this loop runs once per output pixel, and each of its four stores costs a unit call, a bounds check, and (after decision 82) an aliasing guard.
Proving the trip count statically needs path-sensitive bit reasoning: the exit condition is `(flag & (idx == 3)) != 1`, which only terminates because a preceding branch established that `flag` is odd, a fact none of the existing analyses carry.

## Decision

- **The pass proves nothing statically; it recognizes the loop's shape and emits a runtime-guarded fast path.**
  The six-statement body (the scatter store, the two inductions, the exit-bit computation, the copy-back, the conditional back edge) is matched exactly; the loop is replaced by `if flag is odd, both inductions start at zero, and the store's page is below the current memory size: one 32-bit store plus the inductions' exit values; else: the original loop, unchanged`.
  Under the precondition the loop provably runs exactly four iterations storing `word`'s bytes in little-endian order, which is the fused store.
- **The bounds clause compares pages, not byte addresses, against the live memory size.**
  `(base >> 16) < memory.size` keeps the arithmetic inside 32 bits and sends a base in the memory's last page to the original loop, whose byte-at-a-time trap (and the partial writes it leaves visible) must stay exact.
  An earlier draft compared against the memory's *minimum* size; DOOM's framebuffer lives above that floor, the fast path never fired, and the measured gain was zero, which is what promoted the live-size comparison.
- **A miss is free.** Any deviation from the shape (a different width, an extra statement, another branch) leaves the loop untouched.

## Rejected alternatives

- **Proving the trip count via path-sensitive analysis and unrolling.**
  Needs guard-fact propagation (`flag & 1 == 1` on the fallthrough path), bit-level value tracking through a boolean, and a small-loop unroller: three new analyses for one idiom that a three-compare runtime precondition covers exactly.
- **Straight-line consecutive-store coalescing only.**
  Sound and general, but the hot instance is a loop, not a straight line; the straight-line form can be added when a profile shows it.
- **Fusing without the memory-edge clause.**
  Diverges from wasm's per-store trap semantics in the last three bytes of memory, where the original leaves partial writes visible and the fused store leaves none.

## Consequences

**Positive.**
DOOM smoke run 16.6 to 28-31 ticks/sec on top of decision 82's hoisting (baseline 13.5: a combined 2.1-2.3x), frame byte-identical at the static 60-tick point, matching the hand-measured ceiling of the combined transforms (33-37 with unguarded hoisting).
Every other suite artifact (sqlite3-shell, `c/mandelbrot`, `c/sha256`, the NES module) is byte-identical: the idiom fires only where it was profiled.

**Negative.**
The recognizer is deliberately rigid; a compiler update that reorders the six statements or changes an operand shape silently loses the fusion (the DOOM example's throughput would show it).
The precondition costs four compares and a memory-size read per entry, paid also when the slow path is taken.

**Carry-over.**
The 16-bit variant (two-byte scatter) and the straight-line unrolled form are the next shapes if a profile surfaces them.
The general route around per-idiom recognizers is the guard-fact propagation rejected above; it becomes worth building when a second idiom needs the same facts.
