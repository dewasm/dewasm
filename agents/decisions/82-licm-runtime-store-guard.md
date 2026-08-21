# Decision 82: Hoist Invariant Constant-Address Loads with a Runtime Store Guard

Status: **Accepted, 2026-08-21.**
The shared pass lives in [`crates/dewasm-backend/src/licm.rs`](../../crates/dewasm-backend/src/licm.rs); the Ruby backend runs it before loop-body extraction (decision 81), with its threshold in `LICM_PARAMS` (`crates/dewasm-backend-ruby/src/lib.rs`).
Loops containing calls or bulk-memory operations are not hoisted from; that exclusion keeps the two profiled interpreter-style hot functions (sqlite3-shell's, the NES frame function) out of reach and is the main open end.

## Context

Code compiled to wasm re-reads memory-resident globals on every loop iteration: DOOM's hottest function reads the same six framebuffer-format flags and one word per output pixel, seven of its thirteen memory accesses.
The compiler that produced the wasm could not hoist them because the loop's stores go through a run-time pointer it could not prove distinct from the globals, and a wasm-to-Ruby pass faces the same wall: for a store whose address is computed at run time, no static analysis here can prove non-aliasing.
Hand-hoisting those seven loads (assuming no aliasing) measured 13.5 to 23 ticks/sec on the DOOM smoke run, so the prize was known before the design.

## Decision

- **Alias safety is checked at run time, not proven statically.**
  Each eligible loop's constant-address loads are hoisted into fresh locals before the loop; after every store inside the loop, the store's already-computed address is compared against the hoisted address window, and an overlap reloads every hoisted local.
  A few integer compares per store buy back a load per hoisted address per iteration, and the reload path keeps an aliasing program exact, which an end-to-end test exercises (a loop storing through a dynamic pointer into the hoisted window observes the fresh value).
- **Only loads that can never trap are hoisted.**
  Hoisting runs a load earlier, possibly on an iteration-zero path that would have skipped it, so the constant address plus access width must fit inside the memory's minimum size, the floor a memory never shrinks below (and comfortably below the 4 GiB edge, so the guard arithmetic cannot wrap).
- **The guard sits after the store.**
  If the store traps, the loop is gone and staleness cannot be observed; if it succeeds, its effective address is below the memory size, so the wrapped compare arithmetic is exact.
  The store's address is spilled to a temp first, which also keeps its evaluation (and any trap inside it) in the original order.
- **A loop containing a call, an indirect call, or a bulk-memory operation is not hoisted from**: those can write memory (or run code that does) with no address to guard.
- **The threshold is per backend** ([`licm::Params`]): a loop with stores needs at least `min_hoisted_with_stores` distinct hoistable loads (Ruby: 2) before the per-store guards pay; a loop without stores hoists from one load with no guards at all.
- **The pass runs before loop-body extraction**: extraction moves loop bodies into functions where the loop structure is gone, so hoisting must see the loops first.

## Rejected alternatives

- **A static alias proof.**
  The store addresses are run-time values with unknown intervals; the producing compiler already failed at exactly this, which is why the loads sit in the loop at all.
- **Restricting to loops without stores.**
  Sound and guard-free, and kept as the guard-free fast case, but alone it misses the motivating loop (DOOM's blit stores four bytes per pixel).
- **Hoisting without guards.**
  The hand measurement's shape; unsound, and the whole point of the pass is not to gamble on the data.
- **Invalidating per hoisted address instead of reloading all on overlap.**
  Finer tracking costs compares proportional to the hoisted count on the hot (non-overlapping) path; the coarse window keeps the hot path at two compares, and the reload path is for programs that actually alias, which are the rare case.

## Consequences

**Positive.**
DOOM smoke run 13.5 to 16.6 ticks/sec (a 23% gain), frame byte-identical at the static 60-tick point; the aliasing end-to-end check returns the exact-reload value; the spec harness passes; sqlite3_query, `c/mandelbrot`, `c/sha256`, and the NES module are byte-identical or measured neutral.

**Negative.**
The gap to the unguarded hand ceiling (16.6 vs 23 ticks/sec) is the guard cost: DOOM's blit stores four bytes per pixel, so it pays four spill-and-compare sequences per pixel.
Fusing that byte-decomposed store into one 32-bit store (the `agents/experiments.md` f406-hand-optimization entry measured the combination at 33 to 37 ticks/sec) would cut the guards by the same factor and is the natural next pass.

**Carry-over.**
The call barrier keeps every interpreter-shaped hot loop out (sqlite3-shell's dispatch calls helpers; the NES frame function calls per-instruction helpers 10.5 M times per smoke run); admitting calls needs per-callee store summaries, a whole-program analysis this pass deliberately avoided.
That extension was later measured to be not worth building for the two programs it would target: the oracle experiment in `agents/experiments.md` (call-crossing-licm-ceiling) hoisted everything with no guards and moved neither NES nor sqlite.
Hoisting is limited to loads whose address is a literal constant; loads at loop-invariant but computed addresses (a base local never written in the loop) are the next admission candidate and need an invariance check instead of a constant match.
