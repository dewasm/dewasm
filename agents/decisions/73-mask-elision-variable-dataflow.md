# Decision 73: Mask Elision Across Statements via a Per-Function Variable Dataflow

Status: **Accepted, 2026-08-15.**
Landed as `Elision` in `crates/dewasm-backend/src/masking.rs` and its application to local and temp stores in the Ruby backend (`crates/dewasm-backend-ruby/src/lib.rs`).
Refines [decision 71](71-mask-elision-modular-consumers.md), whose within-tree elision this extends across statements; the other masked-unsigned backends can adopt it the same way they can adopt decision 71.
This is stage 2 of issue #164.

## Context

Decision 71 loosened the storage invariant to "masked at its observation points" but kept every store an observation point: a local or temp assignment always masks, because proving that every future read tolerates an unmasked value needs a whole-function analysis.
Store masks are the largest group of what remains (a folded expression tree carries one final mask at its store; a hot loop re-masks its counter every iteration), so the within-tree stage left them all in place: 31.8k of the original 36.6k mask sites in the converted `sqlite3-shell`.

## Decision

**A local or temp may store an unmasked value when a per-function dataflow proves every read of it modular and its value interval convergent within the backend's unboxed-integer limit.**
The analysis lives beside the consumption table in `dewasm_backend::masking` because it follows from the shared masked-unsigned convention (decision 2), not from any one language; only the limit and the emission are per backend.

**Qualification: every read must be modular.**
Reads are classified by the same consumption table emission threads through expression trees, from a `Masked` root at every observation statement (a comparison or condition, a division, a signed or unsigned view, an address, a call argument, a return, a memory store, a global set, a `Select` arm) and from a `Modular` root at a store whose destination itself qualifies.
One refinement goes beyond the table: `&` with a constant operand observes only the bits that constant keeps, all inside the width because constants are stored masked, so its other operand is a modular read in any position (`x & 1` in a condition does not disqualify `x`); only the read classification uses this, emission contexts are unchanged.
Any read the model does not cover disqualifies.
This preserves decision 2's ABI by construction: function boundaries, helper calls, and globals only ever see masked values, and a parameter is defined by its caller at full masked width.
The scan starts from every integer local and temp and only removes, so copies between variables (`BrTarget::Label`'s branch-result moves, a bare `local.get` on a store's right-hand side) resolve co-inductively: a copy is a modular read exactly while its destination still qualifies, and a copy cycle among survivors is sound because no survivor is ever observed exactly.

**Intervals: a Kleene fixpoint with widening enforces the Fixnum guard of decision 71 across statements.**
Each qualifying variable's interval is the join of its definitions' bounds: the decision 71 expression bound with qualifying variables read at their current intervals instead of full masked width, external definitions (call results, `memory.grow`, exception payloads) at full masked width, and the implicit definitions (a parameter at masked width, a declared local at its zero initializer).
Iteration runs until a pass changes nothing, which certifies that every recorded interval contains all of its definitions' bounds; after three passes a still-growing interval is widened, first to its masked width and then past the limit.
A variable whose interval ends outside `[-limit, limit)` is demoted to must-mask, and demotion reruns qualification, since the demoted variable's stores become masked observation roots again.
The widening ladder is what decides loop-carried definitions: `l = l + 1` compounds, gets widened past the limit, and keeps its masks, while `l = (l + 1) & 255` re-narrows each iteration, settles, and elides both the store mask and the add's own mask.
The masked-width rung exists so a definition whose bound is narrow on its own is widened to where a masked variable would live instead of being demoted outright.

**Emission: only stores change.**
A qualifying variable's assignments render their value in `Modular` context, so the root mask disappears under the decision 71 guard; every read is unchanged text, and the recorded intervals feed the same guard inside expression trees (a variable proven byte-narrow lets a product elide that masked-width operands would not).
Under the uniform limit an i64 variable qualifies only if every definition is provably narrow, because full masked i64 width already exceeds the limit: the same conservatism decision 71 chose for i64, applied per variable.

## Rejected alternatives

- **Keep stores as observation points (decision 71 status quo).**
  Store masks are the largest group of remaining sites (14.7k of the 31.8k on `sqlite3-shell` end a local or temp assignment) and include every loop-carried re-mask; the within-tree stage cannot reach them by design.
- **A per-backend dataflow.**
  Same reasoning as decision 71: qualification and intervals follow from the shared convention, only the limit and the emission differ, and duplication invites divergence.
- **Pessimistic copy handling (a copy always disqualifies its source).**
  Simpler than the optimistic removal fixpoint, but branch-result moves are exactly copies between temps, so block and loop results would almost never qualify.
- **Qualification without the interval fixpoint.**
  Congruence alone keeps it correct, but a loop-carried unmasked counter grows into guaranteed bignums: the same trade decision 71 rejected for expression trees, worse here because the value persists across iterations.
- **Widening straight past the limit.**
  Terminates just as fast but demotes every definition that converges slower than the pass budget, including the common `& mask` loop-carried shape the masked-width rung keeps.
- **Extending the model to globals.**
  A global is read by other functions and by the host boundary; proving all of those modular needs whole-module analysis for little gain, so globals stay observation points.

## Consequences

- On the converted `sqlite3-shell` (standalone Ruby), against the stage 1 tree: `& 0xffffffff` sites 31,800 to 31,603 (197 elided, 0.6%), `m64` calls 2,371 to 2,363, file size 7,868,630 to 7,866,029 bytes, ISeq instructions 1,360,259 to 1,359,849, ISeq memsize 47,212,640 to 47,196,232 bytes (same methodology as decision 71).
- Coverage is small by design: in C-derived code most integers are eventually compared, used as an address, or passed across a boundary, and one such read disqualifies the whole variable.
  What does clear are variables read purely as arithmetic and bitwise operands; wider coverage needs a finer-grained model (per definition-use region instead of per variable), left to a later stage with measurement.
- The spec harness (decision 3) binds as always and passes for the Ruby backend under this lowering; `mod dataflow` in `masking.rs` pins qualification, disqualification, the `&`-constant refinement, and both loop-carried outcomes, and `mod masks` in the Ruby backend pins the emitted shapes.
- The invariant of decision 71 tightens per variable: a qualifying variable is masked at none of its stores, and the soundness argument extends because all of its reads are modular consumers.
- The analysis reruns per function at conversion time; its passes are bounded (qualification removes a variable per changing pass, widening caps interval changes), and converting `sqlite3-shell` measures 1.12 s to 1.25 s (debug build).
