# Decision 82: Hoist Invariant Constant-Address Loads with a Runtime Store Guard

Status: **Accepted, 2026-08-21.**
The shared pass lives in [`crates/dewasm-backend/src/licm.rs`](../../crates/dewasm-backend/src/licm.rs).
The Ruby backend runs it before loop-body extraction (decision 81).
Its threshold is in `LICM_PARAMS` (`crates/dewasm-backend-ruby/src/lib.rs`).
Loops containing calls or bulk-memory operations are not hoisted from.
That exclusion keeps the two profiled interpreter-style hot functions out of reach.
They are sqlite3-shell's and the NES frame function.
The exclusion is the main open end.

## Context

Code compiled to wasm re-reads memory-resident globals on every loop iteration.
DOOM's hottest function reads the same six framebuffer-format flags and one word per output pixel.
Those are seven of its thirteen memory accesses.
The loop's stores go through a run-time pointer.
The compiler that produced the wasm could not prove it distinct from the globals.
So that compiler could not hoist them.
A pass from wasm to Ruby faces the same wall.
For a store whose address is computed at run time, no static analysis here can prove non-aliasing.
Hand-hoisting those seven loads measured 13.5 to 23 ticks/sec on the DOOM smoke run.
The hand-hoisting assumed no aliasing.
So the prize was known before the design.

## Decision

- **Alias safety is checked at run time, not proven statically.**
  In each loop the pass accepts, constant-address loads are hoisted into fresh locals before it.
  A guard follows every store inside the loop.
  It compares the store's already-computed address against the hoisted address window.
  An overlap reloads every hoisted local.
  A few integer compares per store buy back a load per hoisted address per iteration.
  The reload path keeps an aliasing program exact, which an end-to-end test exercises.
  In that test, a loop stores through a dynamic pointer into the hoisted window.
  The loop observes the fresh value.
- **Only loads that can never trap are hoisted.**
  Hoisting runs a load earlier, possibly on an iteration-zero path that would have skipped it.
  So the constant address plus access width must fit inside the memory's minimum size.
  That is the floor a memory never goes below.
  That sum must also stay comfortably below the 4 GiB edge, so the guard arithmetic cannot wrap.
- **The guard sits after the store.**
  If the store traps, the loop is gone.
  Then no old hoisted value can be observed.
  If it succeeds, its effective address is below the memory size.
  So the wrapped compare arithmetic is exact.
  The store's address is spilled to a temp first.
  That also keeps its evaluation (and any trap inside it) in the original order.
- **A loop containing a call, an indirect call, or a bulk-memory operation is not hoisted from.**
  That is because those can write memory (or run code that does) with no address to guard.
- **The threshold is per backend** ([`licm::Params`]).
  A loop with stores needs at least `min_hoisted_with_stores` distinct hoistable loads (Ruby: 2).
  Below that count the per-store guards do not pay.
  A loop without stores hoists from one load with no guards at all.
- **The pass runs before loop-body extraction.**
  Extraction moves loop bodies into functions where the loop structure is gone.
  So hoisting must see the loops first.

## Rejected alternatives

- **A static alias proof.**
  The store addresses are run-time values with unknown intervals.
  The producing compiler already failed at exactly this.
  That is why the loads sit in the loop at all.
- **Restricting to loops without stores.**
  Sound and guard-free, and kept as the guard-free fast case.
  But alone it misses the motivating loop (DOOM's pixel copy loop stores four bytes per pixel).
- **Hoisting without guards.**
  The hand measurement's shape, and unsound.
  The point of the pass is that correctness does not depend on the data.
- **Reloading only the overlapped hoisted address instead of all of them.**
  Finer tracking costs compares proportional to the hoisted count on the hot (non-overlapping) path.
  The single window keeps the hot path at two compares.
  The reload path is for programs that actually alias, which are the rare case.

## Consequences

**Positive.**

- DOOM smoke run 13.5 to 16.6 ticks/sec (a 23% gain).
  The frame is byte-identical at the static 60-tick point.
- The aliasing end-to-end check returns the exact-reload value.
- The specification harness passes.
- sqlite3_query, `c/mandelbrot`, `c/sha256`, and the NES module are byte-identical or measured neutral.

**Negative.**
The gap to the unguarded hand upper bound (16.6 vs. 23 ticks/sec) is the guard cost.
DOOM's pixel copy loop stores four bytes per pixel.
So it pays four spill-and-compare sequences per pixel.
Fusing those four byte stores into one 32-bit store would cut the guards by the same factor.
It is the natural next pass.
The f406-hand-optimization entry in `agents/experiments.md` measured the combination.
It reached 33 to 37 ticks/sec.

**Carry-over.**
The call barrier keeps every interpreter-shaped hot loop out.
sqlite3-shell's dispatch calls helpers.
The NES frame function calls per-instruction helpers 10.5 M times per smoke run.
Admitting calls needs per-callee store summaries.
That is a whole-program analysis this pass deliberately avoided.
That extension was later measured to be not worth building for the two programs it would target.
The oracle experiment is `call-crossing-licm-ceiling` in `agents/experiments.md`.
It hoisted everything with no guards and moved neither NES nor SQLite.
Hoisting is limited to loads whose address is a literal constant.
Loads at loop-invariant but computed addresses are the next admission candidate.
An example is a base local never written in the loop.
Such loads need a check that the address does not change, not a constant match.
