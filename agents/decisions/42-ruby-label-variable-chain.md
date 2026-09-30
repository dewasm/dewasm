# Decision 42: Ruby Backend Label-Variable Chain for Multi-Level `br`

Status: **Accepted, 2026-07-28.**
Implemented in `crates/dewasm-backend-ruby/src/lib.rs`.
It replaces two decisions of [decision 4](4-ruby-backend-lowering.md): multi-level `br` and the `catch` value of a loop.
Decision 4's temps-hoisting and `call_indirect` decisions stand.
The cross-frame relay protocol below is in turn replaced by [decision 58](58-ruby-branch-by-value.md).
That protocol is `__br`, the land-or-relay epilogue, and the left-out arm of the top frame.
The lean frame shapes and the depth-1 fast path stand.
They remain the lowering for every function with no crossed frame.
[Decision 60](60-ruby-flatten-only-deep-crossings.md) then narrowed decision 58 to crossings of at least 16 frames.
So the relay protocol still runs for every shallower branch.
[Decision 72](72-ruby-dead-br-clear-elision.md) drops the epilogue's method-body-level clear where nothing later reads `__br`.

## Context

Decision 4 lowered a multi-level `br` to Ruby `catch`/`throw`.
A referenced frame became `catch(:lN) do ... end`, and the branch a `throw :lN`.
`catch`/`throw` is expensive in MRI.
Profiling the converted `sqlite3-shell` gave these figures, on a workload that ran 24.0s:

- `Kernel#catch` took 31.4% of CPU, and `Kernel#throw` 5.1%.
- Each `throw` allocates one `T_IMEMO`.
  Those accounted for 63-70% of the run's ~8M object allocations.
- The allocations drove GC to 16.5% of CPU.

A label-variable chain micro-benchmarked 2.8x faster at 4 levels deep and 4.3x at 16.

Decision 4 explicitly rejected flag variables.
Its grounds were that they obscure the code and need a state-machine dispatch.
That rejection came before decision 4's own separation of result values from control.
Branch result values already travel through slot-copy `assigns` and method-scope-hoisted temps.
They never travel through the branch itself.
So a control flag now carries *only* control.
One method-local variable plus structured `break`/`next` is enough.
The per-block dispatch loop decision 4 feared is unnecessary.

## Decision

- **A method-local `__br` holds the pending target label identifier** (`nil` = none).
  Label identifiers start at 0, so `nil` never names a label.
  It carries control only, hoisted with the temps when the function has any crossed frame.
- **Frames keep the lean shapes**: a `Block` or a referenced `If` is `begin ... end while false`.
  A `Loop` is `while true`.
- **Depth-1 fast path.**
  A `br` whose target is the nearest outer frame leaves it directly.
  It does a `break` for a block/if, and a `next` for an unwrapped loop's back-edge.
- **A multi-level `br` sets `__br` and `break`s** the nearest scope.
  Every frame the branch crosses carries a land-or-relay epilogue, emitted *after* its scope.
  So the `break` skips any code left in the crossed body.
  If `__br` names this frame, the epilogue clears it (the branch lands).
  Otherwise it does `break` again to relay the branch outward.
  The deciding rule for needing an epilogue concerns *crossed* frames.
  A frame is crossed if and only if some outward `br` has it on the stack path.
  The path runs from the target up to and including the frame that directly holds the branch.
  That frame counts because its plain `break` would otherwise land mid-body in its parent.
- **A loop targeted from a strictly nested frame is *wrapped***.
  Its body moves into an inner `begin ... end while false`.
  So the relayed `break` re-enters the loop head via `next` instead of exiting the `while`.
  Loops not targeted from a nested frame stay unwrapped with a plain `next` back-edge.
  A per-function pre-pass (`compute_frame_sets`) computes the crossed and wrapped sets.
- **The top frame leaves out its relay arm.**
  A plain `break` at method-body scope is a Ruby `SyntaxError`.
  A pending branch can never target something outside the top frame, so that arm is dead anyway.

## Rejected alternatives

- **Keep `catch`/`throw` (decision 4).**
  The `T_IMEMO` allocation and stack unwind dominate hot control flow.
  The 31%+5% CPU and 63-70% of allocations above are the direct cost.
- **Epilogue *inside* the scope, the frame that holds the branch excluded** (the first cut of this design).
  Wrong: a `break` out of a nested frame lands mid-body in its parent.
  So any intervening parent code runs before the epilogue is reached.
  Also, a plain inner `break` skips the epilogue that should have relayed `__br`.
  The epilogue must sit after the scope, and the frame that directly holds the branch needs one too.
  `sqlite3-shell` and hand-built mixed-depth `br_table` cases exercise this.
- **Relooper-style state machine / per-statement guards.**
  More code changes and a dispatch per block.
  The structured `break`/`next` chain maps directly onto wasm's already-structured labels.

## Consequences

- Positive: `catch`/`throw` is gone from generated Ruby.
  `sqlite3-shell` on the benchmark workload dropped 24.0s → 6.95s (3.45x), with byte-identical output.
  GC fell from 16.5% to 11%.
  A CPU profile's top frames are now the work function and memory loads/stores.
  It has no `Kernel#catch` or `Kernel#throw`.
- Negative: a *wrapped* loop's back-edge takes a `__br` assignment and a compare instead of a plain `next`.
  The common loop+block pattern stays unwrapped and keeps `next`.
  A multi-level `br` emits one small epilogue per crossed frame, an output-size cost.
  Epilogues sit at deep indentation, so they are emitted as single lines.
  Measured on `sqlite3-shell`, the multi-line first cut grew the output by 37%.
  The growth was mostly leading spaces.
- The specification harness (decision 3) binds correctness.
  It passes for the Ruby backend under this lowering, including `br_table`, `unwind`, and `labels`.
