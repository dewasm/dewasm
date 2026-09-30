# Decision 83: Byte-Scatter Store Fusion Behind a Runtime Precondition

Status: **Accepted, 2026-08-21.**
The shared pass lives in [`crates/dewasm-backend/src/fuse.rs`](../../crates/dewasm-backend/src/fuse.rs).
The Ruby backend runs it first.
It runs before load hoisting (decision 82) and loop-body extraction (decision 81).
So the hoisting pass sees one store where the pattern matched, and emits one guard instead of four.
The recognizer covers the one pattern measured hot (the four-byte little-endian scatter).
Widening it is driven by profiles, not guesses.

## Context

Portable C writes a 32-bit pixel one byte at a time: `dest[i] = v >> (i * 8)`.
That keeps it independent of byte order.
Compiled to wasm, that survives as a four-iteration loop of `store8(base + idx, word >> shift)`.
In the DOOM module's hottest function this loop runs once per output pixel.
Each of the four stores costs a unit call and a bounds check.
After decision 82, each also costs an aliasing guard.
Proving the trip count statically needs path-sensitive bit reasoning.
The exit condition is `(flag & (idx == 3)) != 1`.
It only ends because a preceding branch established that `flag` is odd.
None of the existing analyses carry that fact.

## Decision

- **The pass proves nothing statically.**
  It recognizes the loop's shape and emits a runtime-guarded fast path.
  The six-statement body is matched exactly:
  - the scatter store;
  - the two inductions;
  - the exit-bit computation;
  - the copy-back;
  - the conditional back edge.

  The loop is replaced by an `if` whose condition has three clauses:
  - `flag` is odd;
  - both inductions start at zero;
  - the store's page is below the current memory size.

  The then branch is one 32-bit store plus the inductions' exit values.
  The else branch is the original loop, unchanged.
  Under the precondition the loop provably runs exactly four iterations.
  They store `word`'s bytes in little-endian order, which is the fused store.
- **The bounds clause compares pages, not byte addresses, against the live memory size.**
  `(base >> 16) < memory.size` keeps the arithmetic inside 32 bits.
  It sends a base in the memory's last page to the original loop.
  That loop's byte-at-a-time trap (and the partial writes it leaves visible) must stay exact.
  An earlier draft compared against the memory's *minimum* size.
  DOOM's framebuffer lives above that floor, so the fast path never fired.
  The measured gain was zero, which is what promoted the live-size comparison.
- **A miss is free.**
  Any deviation from the shape leaves the loop untouched.
  Examples are a different width, an extra statement, or another branch.

## Rejected alternatives

- **Proving the trip count via path-sensitive analysis and unrolling.**
  It needs guard facts (`flag & 1 == 1`) carried onto the path that falls through.
  It also needs bit-level value tracking through a Boolean, and a small-loop unroller.
  That is three new analyses for one pattern; a three-compare runtime precondition covers it exactly.
- **Merging stores next to each other in straight-line code only.**
  Sound and general, but the hot instance is a loop, not a straight line.
  The straight-line form can be added when a profile shows it.
- **Fusing without the memory-edge clause.**
  Differs from wasm's per-store trap semantics in the last three bytes of memory.
  There the original leaves partial writes visible, and the fused store leaves none.

## Consequences

**Positive.**
DOOM smoke run 16.6 to 28-31 ticks/sec on top of decision 82's hoisting.
Against the 13.5 baseline, that is a combined 2.1-2.3x.
The frame is byte-identical at the static 60-tick point.
This matches the hand-measured bound of the combined transforms (33-37 with unguarded hoisting).
Every other suite artifact is byte-identical: the pattern matches only where it was profiled.
Those artifacts are sqlite3-shell, `c/mandelbrot`, `c/sha256`, and the NES module.

**Negative.**
The recognizer deliberately accepts one exact shape.
A compiler update can reorder the six statements or change an operand shape.
The fusion is then lost silently; the DOOM example's speed would show it.
The precondition costs four compares and a memory-size read per entry.
The slow path pays it too.

**Carry-over.**
The 16-bit variant (two-byte scatter) and the straight-line unrolled form are the next shapes.
They are built if a profile surfaces them.
The general route around one recognizer per pattern is carrying guard facts, rejected above.
It becomes worth building when a second pattern needs the same facts.
