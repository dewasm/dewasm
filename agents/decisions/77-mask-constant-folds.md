# Decision 77: Mask Elision for Constant AND Operands, Identity Masks, and One-Candidate Equalities

Status: **Accepted, 2026-08-16.**
It landed in the shared analysis and the Ruby and Python emitters.
The shared analysis is `crates/dewasm-backend/src/masking.rs`.
Perl and Bash still mask every site, as under [decision 71](71-mask-elision-modular-consumers.md), and can adopt the same machinery.

## Context

[Decision 71](71-mask-elision-modular-consumers.md) skips a site's result mask only under a modular consumer.
The skip is guarded by the interval bound against the backend's unboxed-integer limit.
Three shapes it leaves masked are common in real conversions.
The counts are on the converted `merman`, Ruby library mode:

- A masked value feeding a bitwise AND (1,985 `& 0xffffffff & ` renderings).
  Most have a constant on the other side.
- AND chains carrying two constants.
- A masked value compared for equality against a constant.
  The count is 572 `& 0xffffffff ==`/`!=` sites, plus 188 through `m64`.

In all three the consumer either redoes the mask's reduction or limits the value.
It limits the value so tightly that the mask decides nothing.

## Decision

Decision 71's discipline holds.
The table and the bound decide; a backend never reasons on its own.
Four rules follow, all in the shared analysis:

1. **Constant AND chains fold at conversion time** (`fold_and_chain`).
   `x & c1 & c2` emits `x & (c1 & c2)`, the constant computed during conversion.
   Sound by associativity of `&`; fires only when at least two constants fold.
2. **An AND with a constant operand consumes its other operand in a new `Reducing` context.**
   It does so in any position, with no bound guard.
   Every IR constant sits inside its type's width.
   So `v & c` reduces `v` at least as strongly as `v`'s own mask would.
   By congruence the results agree, even at an observation point.
   The bound guard of decision 71 is not needed for profitability.
   The raw value feeds nothing but that one reduction.
   The kept mask would have performed that reduction anyway (and once more).
   Through the other bitwise operators that need no mask, a `Reducing` consumer weakens to `Modular`.
   Those operators preserve congruence.
   But the raw operands feed the operator itself, so the guard binds again.
   This rule folds a kept representation mask into a constant AND.
   For example, `(x * y & 0xffffffff) & 255` becomes `x * y & 255`.
3. **A mask whose raw interval already sits inside `[0, 2^w)` is the identity on the exact value.**
   **It drops in every context** (`may_skip_mask`), including observation points.
   The observation points are storage, comparisons, and call arguments.
   The exposed value equals the masked value, so decision 71's allocation claim holds trivially.
4. **A masked equality against a constant drops the mask when its interval holds exactly one candidate.**
   It is implemented in `eq_const_rewrite`.
   `wrap(v) == c` holds exactly when raw `v` lands on `c + k * 2^w`.
   So the rule fires when the raw interval of `v` contains exactly one such candidate.
   The comparison then reads raw `v` against that candidate.
   The constant then migrates across the raw add/sub-by-constant layers the rendering exposes.
   It stops at the first kept mask: `x - c1 & 0xffffffff == c` becomes `x == c1 + c`.
   `eqz` is the same rewrite with `c = 0`.
   With several candidates the mask stays; with none, see below.

Evaluation order is untouched: every rule reshapes a pure integer expression tree in place.
The one operand-order change moves a constant to the other side of `==`.
It can move past anything because a constant has no effects.

**A zero-candidate equality keeps its mask.**
The interval proves the comparison statically false.
But replacing it with its constant result would remove the operand.
The operand can hold a trapping load or division.
The site keeps its mask and the comparison runs.
Rule 3 may still drop the mask when it is the identity.
This is the conservative branch of the "emit the boolean or keep the mask" choice.
The aggressive branch needs a trap-freedom analysis over the operand.
That analysis costs too much for a comparison real code rarely writes.

## Rejected alternatives

- **Rewrite the unsigned range-check pattern `wrap(x - c1) < c2` to `Range#===`.**
  It saves roughly 70KB of source on `merman`.
  It measured 3.6x slower interpreted and 3.7 to 3.9x slower under YJIT.
  `Range#===` is a method call where the mask is one instruction.
- **Rewrite the same pattern to two comparisons (`x >= c1 && x < c1 + c2`).**
  More ISeq than the mask it removes.
- **Constant-fold the zero-candidate equality to its Boolean.**
  Smallest output, but it erases a trap the operand may carry.
  It is rejected above.
- **Extend `Reducing` through OR and XOR operands.**
  `v | c` and `v ^ c` pass `v`'s high bits through, so they do not reduce.
  Only AND qualifies.

## Consequences

- Measured on the converted `sqlite3-shell` (standalone Ruby) and `merman`, before to after.
  Before is decision 76's state.
  `merman` is converted with `--target ruby --mode library --no-default-wasi`.
  ISeq is via `RubyVM::InstructionSequence.compile_file` on Ruby 4.0.4 `arm64-darwin`, children included:

  | Measure | `sqlite3-shell` before | after | `merman` before | after |
  | --- | --- | --- | --- | --- |
  | `& 0xffffffff` sites | 22,055 | 21,218 | 193,525 | 189,380 |
  | `Rt.m64` calls | 2,367 | 2,105 | 5,483 | 4,673 |
  | `& 0xffffffff & ` renderings | 449 | 18 | 1,985 | 550 |
  | `& 0xffffffff ==`/`!=` sites | 60 | 53 | 572 | 343 |
  | Source bytes | 7,744,041 | 7,731,846 | 47,771,297 | 47,712,065 |
  | ISeq instructions | 1,290,623 | 1,288,425 | 6,860,215 | 6,850,285 |
  | ISeq `memsize` (bytes) | 44,013,944 | 43,927,584 | 240,008,120 | 239,615,184 |

  The remaining mask-into-AND renderings are of two kinds:
  - those with a non-constant other operand;
  - those feeding the semantic shift-count mask, which the table still reads as `Modular`.

  On `merman`, the 188 `m64(...) ==`/`!=` sites all compare `m64(x * y)`.
  That interval genuinely admits several candidates, and none dropped.
- Rule 1's literal chain is rare in input processed by `wasm-opt`.
  It has a few sites on `sqlite3-shell` and none on `merman`.
  The fold is kept because it is a few lines, sound unconditionally, and completes rule 2.
  Without it the folded-away mask would reappear as a second constant AND.
- The specification harness (decision 3) binds and passes for Ruby and Python.
  `masking.rs` unit tests cover each rule's firing and non-firing intervals.
  Each backend's `mod masks` checks the emitted shapes both ways.
- The `MaskContext` table gained visibility of the other operand and the `Reducing` variant.
  For the visibility, `bin_operand_context` takes the other operand.
  The interval analysis is otherwise decision 71's, and the limit judgement there is unchanged.
- Rule 4's constant migration can emit a comparison constant outside `[0, 2^w)`.
  An example is a negative raw value that wraps to the original constant after a subtraction.
  That is the exact raw value, not a masked one, and correct by construction.
