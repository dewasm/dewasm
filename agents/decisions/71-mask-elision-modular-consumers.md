# Decision 71: Mask Elision Inside One Expression Tree Under a Modular Consumer

Status: **Accepted, 2026-08-15.**
Landed as the shared analysis in `crates/dewasm-backend/src/masking.rs`.
It is applied in the Ruby backend's expression rendering (`crates/dewasm-backend-ruby/src/lib.rs`).
The other masked-unsigned backends (Perl, Bash) still mask every site.
Each can adopt the same analysis against its own unboxed-integer limit.
The Python backend adopted the analysis with the same 2^62 limit, 2026-08-15 (issue #221).
The limit judgement and the measurements are in the consequences below.
[Decision 77](77-mask-constant-folds.md) extends the same machinery with four additions:

- constant AND folding;
- the `Reducing` context;
- identity-mask elision;
- the constant-equality rewrite where the interval fixes one value.

This is stage 1 of issue #164: elision within one expression tree only.
[Decision 73](73-mask-elision-variable-dataflow.md) extends it across statements.
It lets a local or temp store unmasked when a per-function dataflow proves every read modular.

## Context

[Decision 2](2-numeric-semantics.md) stores integers as masked unsigned values.
Every wrapping operation re-masks its result.
That is `& 0xffffffff` inline for i32, and `Rt.m64` for i64 per [decision 43](43-ruby-i64-mask-fast-path.md).
[Decision 32](32-expression-folding.md) folded single-use values into their consumers.
Since then, many masked results feed directly into an operation that immediately re-masks.
In `(a + b & 0xffffffff) + c & 0xffffffff` the inner mask buys nothing.
The outer one reduces the whole sum anyway.
The converted `sqlite3-shell` carries 36.6k `& 0xffffffff` sites and 2.4k `m64` calls.
Each is parse work, ISeq instructions, and a runtime operation.

## Decision

**The invariant loosens to "every value is masked at its observation points."**
It was "every value is masked".
An observation point is one of:

- storage (local, temp, global, memory);
- a function boundary;
- a non-modular consumer.

The non-modular consumers are:

- a comparison;
- a division or remainder;
- a signed or unsigned view;
- a right shift's shifted operand;
- an address computation;
- a helper call.

Some consumers read only a value's congruence class modulo 2^w:

- a wrapping `add`/`sub`/`mul` operand;
- a bitwise operand;
- `shl`'s shifted operand;
- any shift's count;
- the wrap's operand;
- the operand under a site's own kept mask.

Inside one expression tree, a value whose consumer is one of these may stay unmasked.
The consumer's own mask restores the invariant before the value is observed.

**Soundness.**
The targets this convention covers have arbitrary-precision two's-complement integers.
So an unmasked intermediate is congruent to the masked value modulo 2^w.
Every modular consumer preserves that congruence.
`(x - y) & 0xffffffff` is the correct wrap of a negative difference.
`&`/`|`/`^` read a negative operand as its infinite two's-complement form, leaving the low w bits right.
The first non-modular consumer sits behind a kept mask.
That mask reduces the value back to the stored representation.

**The consumption table and the bound analysis are shared**, in `crates/dewasm-backend/src/masking.rs`.
The table says which operand each `BinOp`/`UnOp` reads modularly.
That is `bin_operand_context`/`un_operand_context`.
The bitwise operators that need no mask pass the consumer's context through.
The bound analysis is a bottom-up interval bound over `ir::Expr` (`elides_mask`).
It treats locals, temps, globals, and calls as their full masked width.
It treats constants and zero-extending loads exactly, and shift counts by constant where known.
Only the emission and the limit are per backend.

**The guard: a mask is skipped only when the exposed intermediate provably stays in range.**
The range is the backend's unboxed-integer range.
For Ruby that is Fixnum, `-2**62 .. 2**62 - 1` on 64-bit MRI.
Elision under this guard is strictly profitable: it removes a mask.
It can never introduce an integer allocation, because every exposed value is a provable Fixnum.
Consequences of the bound:

- i32 add and sub always elide under a modular consumer (raw magnitude at most 2^33).
- Three sites keep their masks unless the interval analysis narrows the operands:
  - a full-range i32 `mul` (up to 2^64);
  - `shl` by an unknown count (up to 2^63);
  - the wrap of a full-range i64.

**i64 uses the same Fixnum limit, not a wider one.**
A wider limit could be 2^64, "no wider than the masked value".
It looks safe because masked i64 values already reach 2^64.
But it cuts both ways:

- An elided raw sum can be a guaranteed bignum.
  The masked value would have wrapped back into Fixnum range.
- Each unmasked `mul` doubles the width, so a chain grows without bound.

Under the uniform limit an elided i64 site is provably allocation-free, the same claim as i32.
The cost: full-range i64 `add`/`sub` and `shr_s` keep `Rt.m64` even under a modular consumer.
So i64 elision fires mainly on chains the analysis can narrow.
Those are zero-extending loads, constants, and values shifted down.
`i64.shr_s` is the known near-miss.
A negative result masks into a guaranteed bignum.
So its raw signed value (within 2^63) would often be cheaper elided than masked.
Loosening that one site is left for a later stage, with measurement.

**Operand context is independent of the elision outcome.**
Operands of a site that keeps its own mask are still rendered in modular context.
The kept mask restores the invariant.
So the guard failing at a node never forces masks back into the tree below it.

## Rejected alternatives

- **Keep every mask (the state before this decision).**
  Simplest, but the inner masks a modular consumer restores are pure overhead at every stage.
  The stages are file size, parse, ISeq, runtime.
- **Elide without the bound guard.**
  Congruence still holds, so it is correct.
  But a `mul` chain's intermediates grow past Fixnum.
  Two full i32 products multiplied together already reach 2^128.
  Every operation on them becomes multi-word bignum arithmetic.
  That trades a cheap mask for unpredictable loss of speed.
- **A per-backend analysis.**
  The consumption table and the interval logic follow from the shared convention of decision 2.
  That is the masked-unsigned convention, not any one language.
  Only the limit and the emission differ.
  So a copy per backend lets the copies come to differ, for no flexibility gained.
- **Cross-statement elision (unmasked locals or temps).**
  Storage is where every consumer, including future ones, reads the value.
  Proving all of them modular needs a dataflow analysis over the whole function.
  It is out of scope for stage 1, which establishes the invariant and the mechanism.
  Issue #164 tracks the rest.

## Consequences

- The measurements on the converted `sqlite3-shell` (standalone Ruby):

  | Measure | Before | After | Change |
  | --- | --- | --- | --- |
  | File size | 7,937,554 bytes | 7,868,630 bytes | 0.87% smaller |
  | ISeq instructions | 1,370,047 | 1,360,259 | 0.71% fewer |
  | ISeq `memsize` | 47,605,992 bytes | 47,212,640 bytes | 0.83% smaller |

  ISeq figures are from `RubyVM::InstructionSequence.compile_file` on MRI 4.0.4, children included.
  4,822 of 36,622 `& 0xffffffff` sites (13.2%) and 72 of 2,443 `m64` calls (2.9%) are elided.
  That is above the rough 4% upper limit issue #219 estimated for this stage.
  It is small in absolute terms, as expected.
- The Python backend (issue #221) uses the same 2^62 limit.
  CPython integers are heap-allocated bignums of 30-bit digits at every size.
  So no unboxed range makes elision allocation-free as Fixnum does for Ruby.
  Under the shared limit every exposed intermediate still fits in three digits.
  That is at most one more than the masked value it replaces.
  The guard states the same claim on every backend.
  The converted `sqlite3-shell` (standalone Python) went from 8,403,153 to 8,329,167 bytes.
  That file size is 0.88% smaller.
  4,822 of 36,624 i32 and 72 of 2,440 i64 inline mask sites are elided.
  They are the same sites as Ruby, since the analysis and the limit are shared.
  An SQLite workload timing measured no change.
  The Python backend's `mod masks` tests check the same three shape directions.
- The specification harness (decision 3) binds and passes for the Ruby backend under this lowering.
  Tests of the generated code's shape check both directions.
  They are `mod masks` in the Ruby backend, plus unit tests in `masking.rs`.
  They check an elided site, a site a non-modular consumer keeps, and a site the bound guard keeps.
- Decision 2's storage representation is unchanged.
  Only the point at which the mask is applied moved, from every producer to every observation point.
- A module whose only `m64` references were elided arithmetic sites no longer bundles the `rt/m64` unit.
  Other runtime units can still require it.
