# Decision 51: Bash Linear Memory as an Associative Array

Status: **Accepted, 2026-07-30.**
Linear memory in generated Bash is `declare -gA` (one byte per key) instead of an indexed array.
All 23 `mem/` units and the 10 memory-touching WASI units use pre-expanded decimal keys.
The full Bash test passes.
DOOM's `initGame` never completed in 3+ CPU-hours on the indexed representation.
It now finishes in ~3.3 minutes.

## Context

Bash indexed arrays are linked lists with a last-reference cursor.
So element access costs O(distance from the previous access).
Measured with 4M elements set:
- ~100k operations/sec in order;
- ~767 operations/sec random;
- ~180/sec for the alternating far-apart pattern a real heap produces.

Specification testsuite modules never showed this: their memories are small and access is local.
A module of several MB (DOOM, SQLite) spends ~85% of its samples in two Bash functions.
They are `array_reference` and `array_insert`.
Associative arrays are hash tables: ~85k random operations/sec at the same scale.
That rate is flat in memory size.

## Decision

Represent linear memory as an **associative array, still one byte per element**.
The change buys the access-complexity class and nothing else.
Criterion: *judge Bash runtime representations on how access cost grows at real-module scale*.
*Do not judge them on small-module microbenchmarks*.
Per-element hash overhead makes tiny in-order benchmarks slightly slower, and that is acceptable.

Consequences of the representation, fixed here:
- Associative array subscripts are literal strings.
  So every subscript is a **pre-expanded canonical-decimal key**.
  The shape is `k=$(( a+1 )); __m[$k]`.
  Inside `(( ))`, `__m[$k]` with `$k` already expanded stays on the fast path.
- Keys are canonical automatically because they all come from `$(( ))`.
  The invariant is stated in `runtime/bash/units/mem/check.sh`.
- Sparse reads still default to 0 (no `set -u` anywhere in the backend or harness).
- Data segments and tables **stay indexed**.
  That is because their elements are contiguous and (for segments) immutable staging data.
  There the linked list with cursor is optimal.
- The emitter declares memory with `declare -gA <p>mem=()`.
  This both forces the associative kind and empties a re-instantiated prefix.
  A `<p>mem=()` without `declare -gA` would silently create an indexed array.

## Rejected alternatives

- **Keep indexed, split into one array per 64KB page.**
  Bounds the list walk to a page but stays O(distance) within it.
  It also adds a nameref dispatch per access; the hash is both faster and simpler.
- **Word-packed cells (8 bytes per element).**
  Up to another ~4-8x on aligned traffic.
  But every unaligned or sub-word access becomes a two-cell splice.
  Several paths operate on bytes: softfloat bit paths, byte-wise WASI stdio.
  Data-segment loading is a third.
  The byte-wise WASI stdio is [decision 12](12-bash-wasi.md)'s.
  Left as a possible later decision on top of this one.
  That is because the complexity is not needed to make real modules run.
- **Inline the bounds check / load-store bodies into generated code.**
  Attacks function-call overhead (~4.8x available).
  It does not attack the access-complexity class that actually blocked DOOM.
  It can still be done later, independently of this one.

## Consequences

- Positive: random access is O(1).
  DOOM `initGame`: 3+ CPU-hours (unfinished) → 198s, ticks ~87s.
  The phase that took minutes on message I/O now takes seconds.
  Every converted module with a non-trivial heap benefits.
- Negative: instantiation pays hash-insert cost on data segments (DOOM's 4.5MB: 19s → 25s).
  RSS grows with hash overhead per filled byte; in-order microbenchmarks lose a little.
- Carry-over: the pre-expanded-key form is now part of the Bash unit style.
  [Decision 11](11-bash-backend-lowering.md)'s lowering conventions are otherwise unchanged.
  The e2e `wasi_import_override` glue copies the byte loop of `fd_write` by hand.
  It had to change at the same time: a shared helper would remove that coupling.
