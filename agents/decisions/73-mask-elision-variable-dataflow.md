# Decision 73: Mask Elision Across Statements via a Per-Function Variable Dataflow

Status: **Accepted, 2026-08-15.**
Landed as `Elision` in `crates/dewasm-backend/src/masking.rs`.
The Ruby backend applies it to local and temp stores (`crates/dewasm-backend-ruby/src/lib.rs`).
It refines [decision 71](71-mask-elision-modular-consumers.md), extending its within-tree elision across statements.
The other masked-unsigned backends can adopt it the same way they can adopt decision 71.
This is stage 2 of issue #164.

## Context

Decision 71 loosened the storage invariant to "masked at its observation points".
But it kept every store an observation point: a local or temp assignment always masks.
That is because proving every future read accepts an unmasked value needs a whole-function analysis.
Store masks are the largest group of what remains.
A folded expression tree carries one final mask at its store.
A hot loop re-masks its counter every iteration.
So the within-tree stage left them all in place.
14.6k of the 21.2k `& 0xffffffff` sites in the converted `sqlite3-shell` end a local or temp assignment.

## Decision

**A local or temp may store an unmasked value when a per-function dataflow proves two things.**
Every read of it is modular.
Its value interval converges within the backend's unboxed-integer limit.
The analysis lives beside the consumption table in `dewasm_backend::masking`.
That is because it follows from decision 2's shared masked-unsigned convention, not one language.
Only the limit and the emission are per backend.

**Qualification: every read must be modular.**
Reads are classified by the same consumption table emission threads through expression trees.
The classification starts from a `Masked` root at every observation statement.
Those are:

- a comparison or condition;
- a division;
- a signed or unsigned view;
- a call argument, a return, or a global set;
- a `Select` arm.

It also starts from a `Modular` root at a local or temp store whose destination itself qualifies.
A memory store's address and value are `Modular` roots too, and a load's address is a modular read.
The memory units reduce their address and stored-value operands themselves ([decision 76](76-memory-unit-operand-reduction.md)).
So those positions observe only the value's congruence class.
A read under [decision 77](77-mask-constant-folds.md)'s `Reducing` context observes only bits congruence preserves.
That context is the non-constant operand of an AND with a constant.
So like a `Modular` read it keeps qualification: `x & 1` in a condition leaves `x` qualified.
Any read the model does not cover removes qualification.
This preserves decision 2's ABI by construction.
Function boundaries, helper calls, and globals only ever see masked values.
A parameter is defined by its caller at full masked width.
The scan starts from every integer local and temp and only removes.
So copies between variables resolve co-inductively.
Such copies are `BrTarget::Label`'s branch-result moves.
A plain `local.get` on a store's right-hand side is one too.
A copy is a modular read exactly while its destination still qualifies.
A copy cycle among the remaining variables is sound because none of them is ever observed exactly.

**Intervals: a Kleene fixed point with widening applies decision 71's Fixnum guard across statements.**
Each qualifying variable's interval is the join of its definitions' bounds:

- the decision 71 expression bound.
  Qualifying variables are read in it at their current intervals instead of full masked width;
- external definitions (call results, `memory.grow`, exception payloads) at full masked width;
- the implicit definitions (a parameter at masked width, a declared local at its zero initializer).

Iteration runs until a pass changes nothing.
That proves that every recorded interval contains all of its definitions' bounds.
After three passes a still-growing interval is widened.
It is widened first to its masked width and then past the limit.
A variable whose interval ends outside `[-limit, limit)` goes back to must-mask.
The stores of that variable become masked observation roots again.
So going back reruns qualification.
The two widening steps are what decide loop-carried definitions.
`l = l + 1` compounds, gets widened past the limit, and keeps its masks.
`l = (l + 1) & 255` re-narrows each iteration and settles.
It elides both the store mask and the add's own mask.
The step to masked width exists for a definition whose bound is narrow on its own.
So that definition does not go straight back to must-mask.
Instead it is widened to where a masked variable would live.

**Emission: only stores change.**
A qualifying variable's assignments render their value in `Modular` context.
So the root mask disappears under the decision 71 guard; every read is unchanged text.
The recorded intervals feed the same guard inside expression trees.
A variable proven byte-narrow lets a product elide a mask that masked-width operands would not.
Decision 77's constant-equality rewrite also reasons about raw intervals.
So a backend holding a per-function `Elision` routes the rewrite through that same analysis.
It routes the rendering through it as well.
That is because the two must agree on which masks drop.
Otherwise a rewritten comparison would read a raw value the rewrite never accounted for.
Under the uniform limit an i64 variable qualifies only if every definition is provably narrow.
That is because full masked i64 width already exceeds the limit.
It is the same careful choice decision 71 made for i64, applied per variable.

## Rejected alternatives

- **Keep stores as observation points (decision 71 unchanged).**
  Store masks are the largest group of remaining sites.
  14.6k of the 21.2k on `sqlite3-shell` end a local or temp assignment.
  They include every loop-carried re-mask; the within-tree stage cannot reach them by design.
- **A per-backend dataflow.**
  Same reasoning as decision 71: qualification and intervals follow from the shared convention.
  Only the limit and the emission differ, and a copy per backend lets the copies come to differ.
- **Pessimistic copy handling (a copy always removes qualification from its source).**
  Simpler than the optimistic removal fixed point.
  But branch-result moves are exactly copies between temps.
  So block and loop results would almost never qualify.
- **Qualification without the interval fixed point.**
  Congruence alone keeps it correct.
  But a loop-carried unmasked counter grows into guaranteed bignums.
  It is the same trade decision 71 rejected for expression trees.
  It is worse here because the value carries over across iterations.
- **Widening straight past the limit.**
  Ends just as fast, but every definition converging slower than the pass budget goes to must-mask.
  That includes the common `& mask` loop-carried shape the step to masked width keeps.
- **Extending the model to globals.**
  A global is read by other functions and by the host boundary.
  Proving all of those modular needs whole-module analysis for little gain.
  So globals stay observation points.

## Consequences

- Measured on the converted `sqlite3-shell` (standalone Ruby).
  The base is the one with decisions 74 to 79 landed.
  The methodology is the same as decision 71's.

  | Measure | Before | After |
  | --- | --- | --- |
  | `& 0xffffffff` sites | 21,218 | 20,700 (518 elided, 2.4%; 319 of them need the decision 76 store positions) |
  | `m64` calls | 2,106 | 2,092 |
  | File size | 7,249,218 bytes | 7,242,414 bytes |
  | ISeq instructions | 1,274,551 | 1,273,487 |
  | ISeq `memsize` | 43,390,448 bytes | 43,347,776 bytes |

- Coverage is small by design.
  That is because in C-derived code most integers are eventually compared or passed across a boundary.
  One such read removes qualification from the whole variable.
  What does clear are variables read purely as arithmetic and bitwise operands.
  Wider coverage needs a finer-grained model (per definition-use region instead of per variable).
  That is left to a later stage with measurement.
- The specification harness (decision 3) binds and passes for the Ruby backend under this lowering.
  `mod dataflow` in `masking.rs` checks qualification, its removal, and the `&`-constant reads.
  It also checks the store-position reads and both loop-carried outcomes.
  `mod masks` in the Ruby backend checks the emitted shapes.
- The invariant of decision 71 tightens per variable.
  A qualifying variable is masked at none of its stores.
  The soundness argument extends because all of its reads are modular consumers.
- The analysis reruns per function at conversion time.
  Its passes are bounded.
  Qualification removes a variable per changing pass, and widening caps interval changes.
  Converting `sqlite3-shell` measures 1.13 s to 1.27 s (debug build).
