# Decision 76: Memory Units Reduce Their Address and Stored-Value Operands

Status: **Accepted, 2026-08-16.** Landed for Ruby and Python: every memory load/store unit reduces its incoming address modulo 2^32 itself, a full-width store reduces its value, and the emitters render both operand positions in modular context. Bash inlines its memory operations (decision 52), Go and Java compile the operands natively, and Perl still masks at every site; each can adopt the same contract with its own measurement.

## Context

Decision 71 skips a site's own result mask under a modular consumer, but a load/store address and a store value stayed observation points: the call site masked them (`@m.i32_load(l0 + 4 & 0xffffffff)`).
Two unit-side facts forced that: the unit's bounds check compares the exact address, and Ruby's `IO::Buffer#set_value(:u32, ...)` raises `RangeError` on an out-of-range value.
After decisions 71 and 75, the converted sqlite3-shell still carries 31,800 `& 0xffffffff` sites, and memory operands are where most of them feed.

Wasm defines the effective address as the i32 base reduced modulo 2^32, plus the static offset without further reduction, checked against the memory size.
`memory_trap.wast` binds the reduction: a store at `memory.size * 0x10000 + (-4)` must succeed (the wrapped address lands back in bounds) while the same shape at `-3..-1` traps.
So the call-site mask in front of the bounds check cannot simply be dropped; the reduction has to move, not vanish.
A full-width store, by contrast, observes only the value's low bits, so a congruent value suffices; the narrow stores (`i32_store8`, `i32_store16`, `i64_store32`, and their `o` twins) already reduced the value inside the unit, and the full-width `i32_store`/`i64_store` were the exception.

## Decision

**The unit contract loosens: a memory load/store unit's address and stored-value arguments may arrive unreduced, and the unit reduces them.**
The discriminating criterion is the one from decision 75: an operation repeated at tens of thousands of call sites, resident in the artifact's ISeq, moves into the shared unit even when the unit then pays it once per call.

Concretely, in [`runtime/ruby/units/memory/`](../../runtime/ruby/units/memory/) and [`runtime/python/units/memory/`](../../runtime/python/units/memory/):

- Every unit reduces the address first: `a &= M32` in the one-argument form, `a = (a & M32) + off` in the `o` form (decision 75), which is exactly wasm's effective-address rule: wrap the base, add the offset without wrapping, then bounds-check.
- `i32_store`/`i32_storeo` reduce the value with `& M32`; `i64_store`/`i64_storeo` use the `Rt.m64` fast path (decision 43), so an already-reduced value stays allocation-free.
  Narrow stores were already reducing; float stores do not touch the value.
- A delegating unit forwards the base and offset separately (`f32_loado` calls `i32_loado(a, off)`, not `i32_load(a + off)`): the inner unit's wrap must see the base alone, or a base-plus-offset sum crossing 2^32 would be wrapped back into bounds instead of trapping.
- The emitters (`mem_call` and the `Stmt::Store` arm in [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs) and [`crates/dewasm-backend-python/src/lib.rs`](../../crates/dewasm-backend-python/src/lib.rs)) render the address and the stored value in `Modular` context; which masks then disappear is decision 71's shared guard, unchanged.
- Decision 75's constant fold narrows: base and offset fold only while the sum stays below 2^32, where the unit's reduction is the identity.
  A larger sum can never be in bounds, and rides as base plus offset through the `o` form so the unit's exact addition reaches the bounds check.

The bulk and host-facing units (`copy`, `fill`, `init`, `grow`, `read_string`) keep the strict contract: their call sites still render in masked context, so they never receive an unreduced operand, and they pay no reduction.
Comparisons, call arguments, returns, and every other observation point are unchanged.

Measured on the converted sqlite3-shell (standalone Ruby, ruby 4.0.4 arm64-darwin; the workload is a recursive CTE inserting 30,000 rows plus aggregates, user-CPU medians of 3 alternating runs):

| Metric | Before (decision 75) | After | Delta |
| --- | --- | --- | --- |
| `& 0xffffffff` sites | 31,800 | 22,055 | -30.6% |
| Source bytes | 7,870,302 | 7,744,041 | -1.6% |
| ISeq instructions | 1,309,998 | 1,290,623 | -1.5% |
| ISeq memsize (bytes) | 44,792,656 | 44,013,944 | -1.7% |
| Workload, plain (s user) | 5.49 | 5.86 | +6.7% |
| Workload, `--yjit` (s user) | 3.01 | 3.05 | +1.3% |

## Rejected alternatives

- **Keep the call-site masks (status quo).**
  9,745 resident mask sites on sqlite3-shell that a one-instruction reduction inside 46 shared units replaces.
- **Interval-chosen non-wrapping variants.**
  Emit a wrapping unit only where the address interval cannot prove reduction unnecessary, keeping a non-wrapping twin for proven-reduced sites.
  Doubles the unit family again (decision 75 already doubled it for the offset), and the proven-reduced case is precisely the cheap one: the fast path of `Rt.m64` and a fixnum `&` cost almost nothing when the operand is already reduced.
- **Drop the mask without moving the reduction into the unit.**
  Unsound: `memory_trap.wast`'s wrapped store must succeed, and an unreduced negative or overflowing address would instead reach the bounds check exact and trap (or, negative, read the wrong slice in Python).

## Consequences

- Positive: every memory operand position is now a modular consumer, so decision 71's elision applies there with no new analysis; the table above lands as source, ISeq, and mask-count reductions.
- Negative: each unit call pays one reduction even when the operand is already reduced; on the sqlite3-shell workload that is +6.7% plain-interpreter and +1.3% `--yjit` user time (accepted under the criterion above, the same trade as decision 75).
- Carry-over: the stage 2 dataflow (issue #220, decision 73 on its branch) currently disqualifies a variable read at a store's value or address position; under this contract those reads are modular, so adopting it there lets more variables qualify.
  Perl keeps call-site masks until it adopts the same contract with its own measurement.
