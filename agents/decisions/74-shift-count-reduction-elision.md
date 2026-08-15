# Decision 74: Shift-Count Reduction Folded for Constants, Dropped Only on an Exact-Value Proof

Status: **Accepted, 2026-08-15.**
Landed as `shift_count_mode` in `crates/dewasm-backend/src/masking.rs` and its use in the Ruby and Python backends (`fn shift_count` in each backend's `lib.rs`).
Perl and Bash still emit the reduction at every shift site; each can adopt `shift_count_mode` the same way.

## Context

wasm defines every shift to reduce its count modulo the width, and the backends implement that per site: `x << (c & 31)` for i32, `& 63` for i64 ([decision 2](2-numeric-semantics.md)).
On the converted `sqlite3-shell` (Ruby, standalone) that is 3,672 `& 31` and 942 `& 63` occurrences (count reductions plus the module's own and-operations), the count reductions overwhelmingly constant, and wasm code that already reduced the count itself produces doubled forms like `x << (l6 & 63 & 63)` (13 such sites).
[Decision 71](71-mask-elision-modular-consumers.md) elides representation masks under modular consumers, and its machinery renders a shift count in the `Modular` context because the emitted reduction is congruence-preserving.

## Decision

**A semantic mask is dropped only when it is provably the identity on the exact rendered value; congruence is not enough.**
The count reduction is not a representation mask restoring decision 2's invariant: its result feeds the target's shift operator, which observes the exact count (a negative count shifts the other way, an oversized one shifts too far).
So the decision 71 rule (elide under a modular consumer) does not apply, and `shift_count_mode` implements the exact-value rule with three outcomes:

- **Constant count**: fold at conversion time and emit the reduced value bare (`x << 2`; the width itself folds to 0 and still shifts, `x << 0`).
- **Provably in-range count**: when the interval bound proves the count's rendering sits in `0..width`, emit it bare; this removes the doubled reduction above.
- **Anything else**: emit the reduction as before.

**A dropped reduction switches the count to the `Masked` rendering context.**
Under a kept reduction the count may render in the `Modular` context, exposing unmasked intermediates, because the emitted `& (width - 1)` reduces them.
With no reduction emitted, the count must be the exact stored value, so the count renders in the `Masked` context and the in-range interval is judged on that rendering.
This is the soundness point: an interval judged on the `Modular` rendering would accept a count expression whose unmasked value is negative.

`rotl`/`rotr` are unaffected: their runtime helpers reduce the count internally, so no per-site reduction exists to fold.

## Rejected alternatives

- **Keep every count reduction (status quo).**
  Constant counts dominate and their reduction is pure parse, size, and runtime overhead; folding them is free and loses nothing.
- **Fold constants only.**
  Simpler, but leaves the doubled reduction on wasm code that already masked its count, which the interval machinery of decision 71 detects with no extra analysis.
- **Judge the interval on the `Modular` rendering.**
  Unsound: a count like `l1 - 1` renders unmasked under decision 71 and can be negative, which the target's shift operator observes.
  The `Masked`-context switch is what makes the elision exact.
- **Elide a count-0 shift entirely (`x` instead of `x << 0`).**
  A separate identity rewrite with its own operand-context questions, for a case wasm-opt already removes from optimized modules; not worth coupling to this change.

## Consequences

- On the converted `sqlite3-shell` (standalone Ruby, base = the decision 71 + Python-port state): file size 7,868,630 to 7,839,479 bytes (0.37% smaller), ISeq instructions 1,360,259 to 1,351,909 (0.61% fewer), ISeq memsize 47,212,640 to 46,876,408 bytes (0.71% smaller; `RubyVM::InstructionSequence.compile_file` on MRI 4.0.4, children included).
  `& 31` occurrences drop from 3,672 to 150 and `& 63` from 942 to 289; the survivors are variable counts the interval cannot bound, plus the module's own and-operations.
- The spec harness (decision 3) binds as always and passes for both backends.
  The i32/i64 shift trials drive counts of 32, 33, 64, and wrapped negatives through function parameters, pinning the kept reduction at its boundaries; folded constant counts run for real in the app suites and the converted `sqlite3-shell` (984 bare `<< 2` sites alone), whose output matches its snapshot.
- Codegen-shape tests (`shift_count_reductions_fold_and_elide` in each backend's `mod masks`, plus `shift_count_*` unit tests in `masking.rs`) pin all three outcomes and the boundary fold.
- The consumption table in `bin_operand_context` still calls a shift count modular; backends that render counts through `shift_count_mode` consult it instead, and a backend that adopts elision without the `Masked`-context switch reintroduces the unsoundness above.
