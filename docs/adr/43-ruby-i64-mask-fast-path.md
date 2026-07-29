# ADR-43 — Ruby Backend: i64 Mask Fixnum Fast Path

Status: **Accepted, 2026-07-28.** Implemented in `crates/dewasm-backend-ruby/src/lib.rs` and `runtime/ruby/units/rt/m64.rb`. Refines the masked-unsigned i64 lowering; the numeric conventions of [ADR-2](2-numeric-semantics.md) are unchanged.

## Context

The Ruby backend stores i64 as a masked-unsigned integer in `0..2**64-1` (ADR-2). Wrapping arithmetic re-masks with `& 0xffff_ffff_ffff_ffff`. On a 64-bit MRI the mask literal `M64` is itself a `T_BIGNUM`: the `Integer#&` call therefore boxes a bignum on *every* i64 add/sub/mul/shift, even when the result fits a fixnum. Measured, `12345 & M64` allocates 1.00 objects per operation, whereas the 32-bit `12345 & M32` (a fixnum literal) allocates ≈0.

Profiling the converted `sqlite3-shell` on the benchmark workload put `T_BIGNUM` at 24.5% of ~8M object allocations (after the ADR-42 cascade removed the `T_IMEMO` churn). 91.9% of those bignums held fixnum-range values — allocated only because the `& M64` forces the bignum path.

Two measured facts open a fast path: a comparison against a bignum literal (`x <= M64`) does *not* allocate, and the overwhelming majority of masked i64 values are already in range.

## Decision

- **A runtime helper `Rt.m64(x)` replaces the inline `& M64`** on the i64 arithmetic sites that can produce an out-of-range value:

  ```ruby
  def m64(x) = (x >= 0 && x <= M64) ? x : (x & M64)
  ```

In-range values (the common case) return unboxed via two non-allocating comparisons; genuine out-of-range values take the same `& M64` as before, so the helper is allocation-neutral on real bignums and correct for every input (`x & M64` still yields the low 64 bits for a negative or oversized `x`).
- **Applied to exactly the sites whose value can leave `0..2**64-1`**: in `lib.rs`, `I64Add`/`I64Sub`/`I64Mul`/`I64Shl`/`I64ShrS`; in the runtime units, `rt/i64_rotl`, `rt/i64_rotr`, `rt/i64_extend_i32_s`, `rt/i64_div_s`, `rt/i64_rem_s`, `rt/i64_trunc_s`, `rt/i64_trunc_sat_s`, the signed i64 memory loads (`memory/i64_load{8,16,32}_s`), and the cold WASI offset stores (`wasi/fd_seek`, `fd_tell`, `clock_time_get`).
- **Mask-free ops stay mask-free.** `And`/`Or`/`Xor`, `I64ShrU`, `Const`, `ExtendI32U`, and the unsigned loads already keep their operand in range and are not touched. Sites where `M64` is a *bound argument* rather than a top-level mask — `rt/i64_trunc_u`, `rt/i64_trunc_sat_u` (upper bound to `trunc_checked`/`trunc_sat`) and `rt/i64_extend{8,16,32}_s` (mask arg into the i32/i64-shared `rt/sext`) — are left as-is; routing them through an i64-specific `m64` would mean restructuring a shared helper for no clear win.
- The helper is evaluated with its argument exactly once, so it composes with the ADR-32 expression-folding/spill machinery unchanged.

## Rejected alternatives

- **Inline shared-temp guard** (`(t = A + B) >= 0 && t <= M64 ? t : ...` emitted at each site). Microbenchmarked slightly faster (69ns vs the helper's 82ns) but expands every arithmetic site and needs a spill temp, growing the already-large generated file instead of shrinking it. The helper call `Rt.m64(A + B)` is *shorter* than `((A + B) & M64)`.
- **Keep the inline `& M64`.** This is the status quo whose 24.5%-of-allocs bignum churn the change targets; 1.00 obj/op on the hot i64 path.
- **Drop the masked-unsigned representation** (store i64 signed, ADR-2). Out of scope and a far larger change; ADR-2's conventions stand.

## Consequences

- Positive: on the `sqlite3-shell` benchmark, allocated objects dropped 2.40M → 0.62M (−74%) for byte-identical output; wall time stayed flat (~6.2s, within noise). The generated `sqlite3-shell` shrank ~42 KB (21,316,625 → 21,274,277 bytes) because `Rt.m64(...)` is shorter than the inline mask.
- Negative: rotates and other sites whose value is usually out of range pay two extra comparisons before the same `& M64`; these are rare, and the code-size and consistency win holds. One more runtime unit (`rt/m64`) enters the bundle whenever any masked i64 op is emitted.
- The spec harness (ADR-3) binds correctness and is green for the Ruby backend under this lowering (257 trials); the numeric conventions of ADR-2 are unchanged.
