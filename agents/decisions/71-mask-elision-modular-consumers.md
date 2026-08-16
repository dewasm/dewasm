# Decision 71: Mask Elision Inside One Expression Tree Under a Modular Consumer

Status: **Accepted, 2026-08-15.**
Landed as the shared analysis in `crates/dewasm-backend/src/masking.rs` and its application in the Ruby backend's expression rendering (`crates/dewasm-backend-ruby/src/lib.rs`).
The other masked-unsigned backends (Perl, Bash) still mask every site; each can adopt the same analysis against its own unboxed-integer limit.
The Python backend adopted the analysis with the same 2^62 limit, 2026-08-15 (issue #221); the limit judgement and the measurements are in the consequences below.
This is stage 1 of issue #164: elision within one expression tree only, no cross-statement analysis.

## Context

[Decision 2](2-numeric-semantics.md) stores integers as masked unsigned values, and every wrapping operation re-masks its result (`& 0xffffffff` inline for i32, `Rt.m64` for i64 per [decision 43](43-ruby-i64-mask-fast-path.md)).
Since [decision 32](32-expression-folding.md) folded single-use values into their consumers, many masked results feed directly into an operation that immediately re-masks: in `(a + b & 0xffffffff) + c & 0xffffffff` the inner mask buys nothing, because the outer one reduces the whole sum anyway.
The converted `sqlite3-shell` carries 36.6k `& 0xffffffff` sites and 2.4k `m64` calls; each is parse work, ISeq instructions, and a runtime operation.

## Decision

**The invariant loosens from "every value is masked" to "every value is masked at its observation points."**
An observation point is storage (local, temp, global, memory), a function boundary, or a non-modular consumer: a comparison, a division or remainder, a signed or unsigned view, a right shift's shifted operand, an address computation, a helper call.
Inside one expression tree, a value whose consumer reads only its congruence class modulo 2^w (a wrapping add/sub/mul operand, a bitwise operand, `shl`'s shifted operand, any shift's count, the wrap's operand, the operand under a site's own kept mask) may stay unmasked: the consumer's own mask restores the invariant before the value is observed.

**Soundness.**
The targets this convention covers have arbitrary-precision two's-complement integers, so an unmasked intermediate is congruent to the masked value modulo 2^w, and every modular consumer preserves that congruence: `(x - y) & 0xffffffff` is the correct wrap of a negative difference, and `&`/`|`/`^` read a negative operand as its infinite two's-complement form, leaving the low w bits right.
The first non-modular consumer sits behind a kept mask, which reduces the value back to the stored representation.

**The consumption table and the bound analysis are shared**, in `crates/dewasm-backend/src/masking.rs`: which operand each `BinOp`/`UnOp` reads modularly (`bin_operand_context`/`un_operand_context`, with the maskless bitwise operators passing the consumer's context through), and a bottom-up interval bound over `ir::Expr` (`elides_mask`) that treats locals, temps, globals, and calls as their full masked width, constants and zero-extending loads exactly, and shift counts by constant where known.
Only the emission and the limit are per backend.

**The guard: a mask is skipped only when the exposed intermediate provably stays within the backend's unboxed-integer range.**
For Ruby that is Fixnum, `-2**62 .. 2**62 - 1` on 64-bit MRI.
Elision under this guard is strictly profitable: it removes a mask and can never introduce an integer allocation, because every exposed value is a provable Fixnum.
Consequences of the bound: i32 add and sub always elide under a modular consumer (raw magnitude at most 2^33), while a full-range i32 mul (up to 2^64), `shl` by an unknown count (up to 2^63), and the wrap of a full-range i64 keep their masks unless the interval analysis narrows the operands.

**i64 uses the same Fixnum limit, not a wider one.**
A wider allowance (say 2^64, "no wider than the masked value") looks harmless because masked i64 values already reach 2^64, but it cuts both ways: an elided raw sum can be a guaranteed bignum where the masked value would have wrapped back into Fixnum range, and each unmasked mul doubles the width, so a chain grows without bound.
Under the uniform limit an elided i64 site is provably allocation-free, the same claim as i32; the cost is that full-range i64 add/sub and `shr_s` keep `Rt.m64` even under a modular consumer, so i64 elision fires mainly on chains the analysis can narrow (zero-extending loads, constants, values shifted down).
`i64.shr_s` is the known near-miss: its raw signed value (within 2^63) would often be cheaper elided than masked, since a negative result masks into a guaranteed bignum; loosening that one site is left for a later stage, with measurement.

**Operand context is independent of the elision outcome.**
Operands of a site that keeps its own mask are still rendered in modular context: the kept mask restores the invariant, so the guard failing at a node never forces masks back into its subtree.

## Rejected alternatives

- **Keep every mask (status quo).**
  Simplest, but the inner masks a modular consumer restores are pure overhead at every stage: file size, parse, ISeq, runtime.
- **Elide without the bound guard.**
  Congruence still holds, so it is correct, but a mul chain's intermediates grow past Fixnum (two full i32 products already reach 2^128 multiplied together) and every operation on them becomes multi-word bignum arithmetic: a cheap mask traded for unpredictable slowdowns.
- **A per-backend analysis.**
  The consumption table and the interval logic follow from the shared masked-unsigned convention (decision 2), not from any one language; duplicating them per backend invites divergence for no flexibility gained, since only the limit and the emission differ.
- **Cross-statement elision (unmasked locals or temps).**
  Storage is where every consumer, including future ones, reads the value; proving all of them modular needs a dataflow analysis over the whole function.
  Out of scope for stage 1, which establishes the invariant and the mechanism; issue #164 tracks the rest.

## Consequences

- On the converted `sqlite3-shell` (standalone Ruby): file size 7,937,554 to 7,868,630 bytes (0.87% smaller), ISeq instructions 1,370,047 to 1,360,259 (0.71% fewer), ISeq memsize 47,605,992 to 47,212,640 bytes (0.83% smaller; `RubyVM::InstructionSequence.compile_file` on MRI 4.0.4, children included).
  4,822 of 36,622 `& 0xffffffff` sites (13.2%) and 72 of 2,443 `m64` calls (2.9%) are elided: above the rough 4% ceiling issue #219 estimated for this stage, and small in absolute terms as expected.
- The Python backend (issue #221) uses the same 2^62 limit.
  CPython integers are heap-allocated 30-bit-digit bignums at every size, so no unboxed range makes elision allocation-free as Fixnum does for Ruby; under the shared limit every exposed intermediate still fits in three digits, at most one more than the masked value it replaces, and the guard states the same claim on every backend.
  On the converted `sqlite3-shell` (standalone Python): file size 8,403,153 to 8,329,167 bytes (0.88% smaller), 4,822 of 36,624 i32 and 72 of 2,440 i64 inline mask sites elided (the same sites as Ruby, analysis and limit being shared); a sqlite workload timing measured no change, and the Python backend's `mod masks` tests pin the same three shape directions.
- The spec harness (decision 3) binds as always and passes for the Ruby backend under this lowering; codegen-shape tests (`mod masks` in the Ruby backend, plus unit tests in `masking.rs`) pin both directions: an elided site, a site kept by a non-modular consumer, and a site kept by the bound guard.
- Decision 2's storage representation is unchanged; only the point at which the mask is applied moved from every producer to every observation point.
- A module whose only `m64` references were elided arithmetic sites no longer bundles the `rt/m64` unit (other runtime units can still require it).
