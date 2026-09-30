# Decision 4: Ruby Backend Lowering Conventions

Status: **Accepted, 2026-07-23.**
Recorded afterwards; implemented in `crates/dewasm-backend-ruby/src/lib.rs` + `runtime/ruby/`.
Numeric conventions are decision 2's; this decision covers control flow and object shape.
[Decision 42](42-ruby-label-variable-chain.md) replaces the decisions on multi-level `br` and on the loop `catch` value.
It also replaces the flag-variable rejection.
The temps-hoisting decision stands.
So does `call_indirect`'s structural type-symbol comparison.
But its splat-array dispatch is changed by [decision 44](44-ruby-call-indirect-arity.md) (fixed-arity `Table#callN`).

## Context

Ruby:

- has no labeled break/continue;
- creates a new variable scope inside `do ... end` blocks;
- compares `call_indirect` targets by nothing: the backend must supply wasm's structural type check.

These three language facts drove the lowering shape.

## Decision

- **Multi-level `br` lowers to `catch`/`throw`.**
  *(Superseded by [decision 42](42-ruby-label-variable-chain.md): the `__br` label-variable chain.)*
  A referenced block label becomes `catch(:lN) do ... end`.
  `br` becomes result-slot assignments followed by `throw :lN`.
  Unreferenced labels emit nothing (decision 1's `referenced` flag).
- **Loops become `while true` wrapping a `catch` whose value picks continue vs. exit.**
  *(Superseded by [decision 42](42-ruby-label-variable-chain.md).)*
  The body falls through to `true` (break the while).
  A back-edge `throw`s `false` (next iteration).
  One shape covers branches to the loop head from any nesting depth.
- **A `br`/`br_if`/`br_table` at the nearest capturing frame (depth 1) skips `throw`.**
  **It lowers to a plain `break`/`next`.**
  **A frame every one of whose incoming branches is depth-1 drops `catch`/`throw` entirely.**
  *(Superseded by [decision 42](42-ruby-label-variable-chain.md), which drops `catch`/`throw` for every frame, not only depth-1-only ones.
  Decision 42 keeps the depth-1 `break`/`next` fast path.)*
  `Block` renders as `begin ... end while false`.
  `Loop` renders as a plain `while true ... end` with a trailing `break` added for the fall-through case.
  This was the candidate improvement in the Consequences section below, now adopted.
  `break`/`next` inside a `catch(...) do ... end` block still ends the block early.
  It does so exactly like a `throw` would (Ruby block semantics).
  The target frame may or may not keep its `catch` wrapper.
  So the branch-site simplification is correct either way.
  Only frames with *zero* deeper incoming throws can drop the wrapper itself.
  A per-function pre-pass computes, per label, whether every branch to it is depth-1.
  The pre-pass is `compute_break_only` in `crates/dewasm-backend-ruby/src/lib.rs`.
  `plain if`, `br_if`'s wrapper `if`, and `br_table`'s `case` never capture.
  So they don't count as a frame boundary for this analysis.
  Measured in isolation on MRI 4.0.4, in one benchmark:

  - 20M-iteration loop back-edge: `catch`/`throw` 2.59s vs. `break`/`next` 0.45s, ~5.7x;
  - 5M-call block-exit: `catch`/`throw` 0.69s vs. `begin...end while false` 0.17s, ~4.1x.

  `catch` and `throw` are genuinely expensive in MRI.
  This is the actual driver of hot-loop overhead in generated code, not incidental.
- **All stack temps are hoisted to method scope** by one `s0 = s1 = ... = nil` line at function entry.
  First assignment inside a Ruby block is block-local.
  So without hoisting, values assigned inside `catch` blocks are lost at `end`.
  This was found by `NameError`s in the specification harness, not predicted.
- **`call_indirect` compares structural type symbols.**
  The backend renders each type index as a symbol interned from the type's shape.
  An example is `:"i32,i64->i32"`.
  It does so both when filling the table and at call sites.
  Wasm compares function types structurally, not by index.
  A table can be shared across modules via an imported table, and their index spaces differ.
  Any module-local identifier (even a canonicalized index) breaks once that happens.
  *(The dispatch used a splat: `@tT.call(index, type_sym, *args)` re-splatting into `func.call(*args)`.
  Changed by [decision 44](44-ruby-call-indirect-arity.md): a per-arity `Table#callN` drops both splats.
  The structural-symbol comparison here is unchanged.)*
- **Module = one class**:

  - imports resolved in `initialize` (`@ifN` instance variables);
  - globals as `@gN`;
  - exports in an `@exports` hash keyed by the raw export name;
  - `invoke(name, *args)` as the entry point;
  - memory exposed via `attr_reader`.

  Export names need not be valid Ruby method names.
  Data segments embed as hexadecimal strings decoded with `pack("H*")` (no `require`, unlike base64).

## Rejected alternatives

- **Flag variables / state-machine dispatch for multi-level `br`.**
  Both obscure the code far more than catch/throw.
  The state machine also costs a dispatch loop per block.
  catch/throw benchmarked acceptably and maps 1:1 to label semantics.
  *(Reversed by [decision 42](42-ruby-label-variable-chain.md).
  Result values are separated from control via the `assigns` slot-copies and hoisted temps above.
  Once they are, a control-only flag needs no per-block dispatch.
  And `catch`/`throw`'s allocation cost turned out to dominate hot loops.)*
- **`define_method` per export as the public API.**
  Export names collide with `Object` method names and Ruby keywords.
  A name-keyed hash plus `invoke` is collision-free.
  Friendly named methods can be layered on later.

## Consequences

- Positive: the whole control-flow story is three emission shapes.
  The specification harness (decision 3) passes, including `br_table`, `unwind`, and `labels`.
- Negative: `catch` allocates and `throw` unwinds.
  So hot loops pay for labels they rarely take.
  Reduced for the common depth-1 case (see the `break`/`next` decision above).
  A multi-level `br` (depth > 1) still pays the full `catch`/`throw` cost, which it cannot avoid.
  Ruby has no labeled `break`.
- Deep wasm recursion maps to Ruby stack frames.
  `SystemStackError` is the (accepted) equivalent of "call stack exhausted".
