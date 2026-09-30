# Decision 76: Memory Units Reduce Their Address and Stored-Value Operands

Status: **Accepted, 2026-08-16.**
Landed for Ruby and Python.
Every memory load/store unit reduces its incoming address modulo 2^32 itself.
A full-width store reduces its value.
The emitters render both operand positions in modular context.
Bash inlines its memory operations (decision 52), and Go and Java compile the operands natively.
Perl still masks at every site.
Each can adopt the same contract with its own measurement.

## Context

Decision 71 skips a site's own result mask under a modular consumer.
But a load/store address and a store value stayed observation points.
The call site masked them (`@m.i32_load(l0 + 4 & 0xffffffff)`).
Two unit-side facts forced that.
The unit's bounds check compares the exact address.
Ruby's `IO::Buffer#set_value(:u32, ...)` raises `RangeError` on an out-of-range value.
After decisions 71 and 75, the converted sqlite3-shell still carries 31,800 `& 0xffffffff` sites.
Most of them feed memory operands.

Wasm defines the effective address as the i32 base reduced modulo 2^32.
The static offset is added without further reduction.
The sum is checked against the memory size.
`memory_trap.wast` binds the reduction.
A store at `memory.size * 0x10000 + (-4)` must succeed: the wrapped address lands back in bounds.
The same shape at `-3..-1` traps.
So the call-site mask in front of the bounds check cannot simply be dropped.
The reduction has to move, not vanish.
A full-width store, by contrast, observes only the value's low bits, so a congruent value suffices.
The narrow stores already reduced the value inside the unit.
Those are `i32_store8`, `i32_store16`, `i64_store32`, and their `o` twins.
The full-width `i32_store`/`i64_store` were the exception.

## Decision

**The unit contract loosens.**
A memory load/store unit's address and stored-value arguments may arrive unreduced.
The unit reduces them.
The discriminating criterion is the one from decision 75.
An operation repeated at tens of thousands of call sites is resident in the artifact's ISeq.
It moves into the shared unit, even when the unit then pays it once per call.

Concretely, in [`runtime/ruby/units/memory/`](../../runtime/ruby/units/memory/) and [`runtime/python/units/memory/`](../../runtime/python/units/memory/):

- Every unit reduces the address first.
  The one-argument form does `a &= M32`; the `o` form (decision 75) does `a = (a & M32) + off`.
  That is exactly wasm's effective-address rule.
  It wraps the base, adds the offset without wrapping, then bounds-checks.
- `i32_store`/`i32_storeo` reduce the value with `& M32`.
  `i64_store`/`i64_storeo` use the `Rt.m64` fast path (decision 43).
  So an already-reduced value stays allocation-free.
  Narrow stores were already reducing, and float stores do not touch the value.
- A delegating unit forwards the base and offset separately.
  `f32_loado` calls `i32_loado(a, off)`, not `i32_load(a + off)`.
  The inner unit's wrap must see the base alone.
  Otherwise a base-plus-offset sum crossing 2^32 would wrap back into bounds instead of trapping.
- The emitters render the address and the stored value in `Modular` context.
  The emitters are `mem_call` and the `Stmt::Store` arm.
  They are in [`crates/dewasm-backend-ruby/src/lib.rs`](../../crates/dewasm-backend-ruby/src/lib.rs) and [`crates/dewasm-backend-python/src/lib.rs`](../../crates/dewasm-backend-python/src/lib.rs).
  Which masks then disappear is decision 71's shared guard, unchanged.
- Decision 75's constant fold narrows.
  Base and offset fold only while the sum stays below 2^32.
  There the unit's reduction is the identity.
  A larger sum can never be in bounds.
  It rides as base plus offset through the `o` form.
  So the unit's exact addition reaches the bounds check.

The bulk and host-facing units (`copy`, `fill`, `init`, `grow`, `read_string`) keep the strict contract.
Their call sites still render in masked context, so they never receive an unreduced operand.
They pay no reduction.
Comparisons, call arguments, returns, and every other observation point are unchanged.

Measured on the converted sqlite3-shell (standalone Ruby, ruby 4.0.4 arm64-darwin).
The workload is a recursive CTE inserting 30,000 rows plus aggregates.
Times are user-CPU medians of 3 alternating runs.

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
  It keeps 9,745 resident mask sites on sqlite3-shell.
  A one-instruction reduction inside 46 shared units replaces them.
- **Interval-chosen non-wrapping variants.**
  Emit a wrapping unit only where the address interval cannot prove reduction unnecessary.
  Keep a non-wrapping twin for proven-reduced sites.
  Doubles the unit family again (decision 75 already doubled it for the offset).
  And the proven-reduced case is precisely the cheap one.
  The fast path of `Rt.m64` and a fixnum `&` cost almost nothing when the operand is already reduced.
- **Drop the mask without moving the reduction into the unit.**
  Unsound: `memory_trap.wast`'s wrapped store must succeed.
  An unreduced negative or overflowing address would instead reach the bounds check exact and trap.
  A negative one would instead read the wrong slice in Python.

## Consequences

- Positive: every memory operand position is now a modular consumer.
  So decision 71's elision applies there with no new analysis.
  The table above lands as source, ISeq, and mask-count reductions.
- Negative: each unit call pays one reduction even when the operand is already reduced.
  On the sqlite3-shell workload that is +6.7% plain-interpreter and +1.3% `--yjit` user time.
  It is accepted under the criterion above, the same trade as decision 75.
- Carry-over: the stage 2 dataflow is issue #220, decision 73 on its branch.
  It currently disqualifies a variable read at a store's value or address position.
  Under this contract those reads are modular, so adopting it there lets more variables qualify.
  Perl keeps call-site masks until it adopts the same contract with its own measurement.
