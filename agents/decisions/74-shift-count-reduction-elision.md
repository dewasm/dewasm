# Decision 74: Shift-Count Reduction Folded for Constants, Dropped Only on an Exact-Value Proof

Status: **Accepted, 2026-08-15.**
Landed as `shift_count_mode` in `crates/dewasm-backend/src/masking.rs`.
The Ruby and Python backends use it (`fn shift_count` in each backend's `lib.rs`).
Perl and Bash still emit the reduction at every shift site.
Each can adopt `shift_count_mode` the same way.

## Context

wasm defines every shift to reduce its count modulo the width.
The backends implement that per site: `x << (c & 31)` for i32, `& 63` for i64 ([decision 2](2-numeric-semantics.md)).
On the converted `sqlite3-shell` (Ruby, standalone) that is 3,672 `& 31` and 942 `& 63` occurrences.
Those counts include the count reductions plus the module's own and-operations.
The count reductions are overwhelmingly constant.
wasm code that already reduced the count itself produces doubled forms like `x << (l6 & 63 & 63)`.
There are 13 such sites.
[Decision 71](71-mask-elision-modular-consumers.md) elides representation masks under modular consumers.
Its machinery renders a shift count in the `Modular` context.
That is because the emitted reduction is congruence-preserving.

## Decision

**A semantic mask is dropped only when it is provably the identity on the exact rendered value.**
Congruence is not enough.
The count reduction is not a representation mask restoring decision 2's invariant.
Its result feeds the target's shift operator, which observes the exact count.
A negative count shifts the other way, and an oversized one shifts too far.
So the decision 71 rule (elide under a modular consumer) does not apply.
`shift_count_mode` implements the exact-value rule with three outcomes:

- **Constant count**: fold at conversion time and emit the reduced value bare (`x << 2`).
  The width itself folds to 0 and still shifts, `x << 0`.
- **Provably in-range count**: the interval bound proves the count's rendering sits in `0..width`.
  Then emit it bare; this removes the doubled reduction above.
- **Anything else**: emit the reduction as before.

**A dropped reduction switches the count to the `Masked` rendering context.**
Under a kept reduction the count may render in the `Modular` context, exposing unmasked intermediates.
That is because the emitted `& (width - 1)` reduces them.
With no reduction emitted, the count must be the exact stored value.
So the count renders in the `Masked` context, and the in-range interval is judged on that rendering.
This is the soundness point.
Consider an interval judged on the `Modular` rendering.
It would accept a count expression whose unmasked value is negative.

`rotl`/`rotr` are unaffected: their runtime helpers reduce the count internally.
So no per-site reduction exists to fold.

## Rejected alternatives

- **Keep every count reduction (status quo).**
  Constant counts dominate, and their reduction is pure parse, size, and runtime overhead.
  Folding them is free and loses nothing.
- **Fold constants only.**
  Simpler, but leaves the doubled reduction on wasm code that already masked its count.
  The interval machinery of decision 71 detects that with no extra analysis.
- **Judge the interval on the `Modular` rendering.**
  Unsound: a count like `l1 - 1` renders unmasked under decision 71 and can be negative.
  The target's shift operator observes that.
  The `Masked`-context switch is what makes the elision exact.
- **Elide a count-0 shift entirely (`x` instead of `x << 0`).**
  A separate identity rewrite with its own operand-context questions.
  Its case is one wasm-opt already removes from optimized modules.
  It is not worth coupling to this change.

## Consequences

- Measured on the converted `sqlite3-shell` (standalone Ruby).
  The base is the decision 71 + Python-port state.
  ISeq memsize is from `RubyVM::InstructionSequence.compile_file` on MRI 4.0.4, children included.

  | Measure | Before | After |
  | --- | --- | --- |
  | File size | 7,868,630 bytes | 7,839,479 bytes (0.37% smaller) |
  | ISeq instructions | 1,360,259 | 1,351,909 (0.61% fewer) |
  | ISeq memsize | 47,212,640 bytes | 46,876,408 bytes (0.71% smaller) |

  `& 31` occurrences drop from 3,672 to 150 and `& 63` from 942 to 289.
  The survivors are variable counts the interval cannot bound, plus the module's own and-operations.
- The spec harness (decision 3) binds as always and passes for both backends.
  The i32/i64 shift trials drive counts through function parameters.
  The counts are 32, 33, 64, and wrapped negatives.
  They pin the kept reduction at its boundaries.
  Folded constant counts run for real in the app suites and the converted `sqlite3-shell`.
  The latter has 984 bare `<< 2` sites alone, and its output matches its snapshot.
- Codegen-shape tests pin all three outcomes and the boundary fold.
  They are `shift_count_reductions_fold_and_elide` in each backend's `mod masks`.
  They also include `shift_count_*` unit tests in `masking.rs`.
- The consumption table in `bin_operand_context` still calls a shift count modular.
  Backends that render counts through `shift_count_mode` consult it instead.
  A backend that adopts elision without the `Masked`-context switch reintroduces the unsoundness above.
