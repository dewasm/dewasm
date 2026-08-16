# Decision 77: Constant AND Operands, Identity Masks, and Pinned Constant Equalities Extend Mask Elision

Status: **Accepted, 2026-08-16.** Landed in the shared analysis (`crates/dewasm-backend/src/masking.rs`) and the Ruby and Python emitters. Perl and Bash still mask every site, as under [decision 71](71-mask-elision-modular-consumers.md), and can adopt the same machinery.

## Context

[Decision 71](71-mask-elision-modular-consumers.md) skips a site's result mask only under a modular consumer, guarded by the interval bound against the backend's unboxed-integer limit.
Three shapes it leaves masked are common in real conversions (counted on the converted merman, Ruby library mode): a masked value feeding a bitwise AND (1,985 `& 0xffffffff & ` renderings, most with a constant on the other side), AND chains carrying two constants, and a masked value compared for equality against a constant (572 `& 0xffffffff ==`/`!=` sites plus 188 through `m64`).
In all three the consumer either redoes the mask's reduction or pins the value so tightly that the mask decides nothing.

## Decision

Four rules, all in the shared analysis per decision 71's discipline (the table and the bound decide; a backend never reasons on its own):

1. **Constant AND chains fold at conversion time** (`fold_and_chain`): `x & c1 & c2` emits `x & (c1 & c2)`, the constant computed during conversion.
   Sound by associativity of `&`; fires only when at least two constants fold.
2. **An AND with a constant operand consumes its other operand in a new `Reducing` context, in any position, with no bound guard.**
   Every IR constant sits inside its type's width, so `v & c` reduces `v` at least as strongly as `v`'s own mask would; by congruence the results agree, even at an observation point.
   The bound guard of decision 71 is not needed for profitability: the raw value feeds nothing but that one reduction, which the kept mask would have performed anyway (and once more).
   Through the other maskless bitwise operators a `Reducing` consumer weakens to `Modular`: they preserve congruence, but the raw operands feed the operator itself, so the guard binds again.
   This is what folds a kept representation mask into a constant AND: `(x * y & 0xffffffff) & 255` becomes `x * y & 255`.
3. **A mask whose raw interval already sits inside `[0, 2^w)` is the identity on the exact value and drops in every context** (`may_skip_mask`), including observation points: storage, comparisons, call arguments.
   The exposed value equals the masked value, so decision 71's allocation claim holds trivially.
4. **A masked equality against a constant drops the mask when the interval pins a unique preimage** (`eq_const_rewrite`): `wrap(v) == c` holds exactly when raw `v` lands on `c + k * 2^w`, so when the raw interval of `v` contains exactly one such candidate, the comparison reads raw `v` against that candidate.
   The constant then migrates across the raw add/sub-by-constant layers the rendering exposes, stopping at the first kept mask: `x - c1 & 0xffffffff == c` becomes `x == c1 + c`.
   `eqz` is the same rewrite with `c = 0`.
   With several candidates the mask stays; with none, see below.

Evaluation order is untouched: every rule reshapes a pure integer expression tree in place, and the one operand-order change (a constant moving to the other side of `==`) commutes with anything because a constant has no effects.

**A zero-candidate equality keeps its mask.**
The interval proves the comparison statically false, but replacing it with its constant result would delete the operand, and the operand can hold a trapping load or division.
The site keeps its mask and the comparison runs; rule 3 may still drop the mask when it is the identity.
This is the conservative branch of the "emit the boolean or keep the mask" choice; the aggressive branch needs a trap-freedom analysis over the operand, disproportionate for a comparison real code rarely writes.

## Rejected alternatives

- **Rewrite the unsigned range-check idiom `wrap(x - c1) < c2` to `Range#===`.**
  Measured 3.6x slower interpreted and 3.7 to 3.9x slower under YJIT for roughly 70KB of source saved on merman: `Range#===` is a method call where the mask is one instruction.
- **Rewrite the same idiom to two comparisons (`x >= c1 && x < c1 + c2`).**
  More ISeq than the mask it removes.
- **Constant-fold the zero-candidate equality to its boolean.**
  Smallest output, but it erases a trap the operand may carry; rejected above.
- **Extend `Reducing` through OR and XOR operands.**
  `v | c` and `v ^ c` pass `v`'s high bits through, so they do not reduce; only AND qualifies.

## Consequences

- Measured on the converted sqlite3-shell (standalone Ruby) and merman (`--target ruby --mode library --no-default-wasi`), before (decision 76's state) to after; ISeq via `RubyVM::InstructionSequence.compile_file` on ruby 4.0.4 arm64-darwin, children included:

  | Metric | sqlite3-shell before | after | merman before | after |
  | --- | --- | --- | --- | --- |
  | `& 0xffffffff` sites | 22,055 | 21,218 | 193,525 | 189,380 |
  | `Rt.m64` calls | 2,367 | 2,105 | 5,483 | 4,673 |
  | `& 0xffffffff & ` renderings | 449 | 18 | 1,985 | 550 |
  | `& 0xffffffff ==`/`!=` sites | 60 | 53 | 572 | 343 |
  | Source bytes | 7,744,041 | 7,731,846 | 47,771,297 | 47,712,065 |
  | ISeq instructions | 1,290,623 | 1,288,425 | 6,860,215 | 6,850,285 |
  | ISeq memsize (bytes) | 44,013,944 | 43,927,584 | 240,008,120 | 239,615,184 |

  The remaining mask-into-AND renderings have a non-constant other operand, or feed the semantic shift-count mask, which the table still reads as `Modular`; merman's 188 `m64(...) ==`/`!=` sites all compare `m64(x * y)` whose interval genuinely admits several candidates, and none dropped.
- Rule 1's literal chain is rare in wasm-opt-processed input (a handful of sites on sqlite3-shell, none on merman): the fold is kept because it is a few lines, sound unconditionally, and completes rule 2 (without it the folded-away mask would simply reappear as a second constant AND).
- The spec harness (decision 3) binds and passes for Ruby and Python; `masking.rs` unit tests cover each rule's firing and non-firing intervals, and each backend's `mod masks` pins the emitted shapes both ways.
- The `MaskContext` table gained sibling-operand visibility (`bin_operand_context` takes the other operand) and the `Reducing` variant; the interval analysis is otherwise decision 71's, and the limit judgement there is unchanged.
- Rule 4's constant migration can emit a comparison constant outside `[0, 2^w)` (a negative preimage of a wrapped subtraction); that is the exact raw value, not a masked one, and correct by construction.
