# ADR-42 — Ruby Backend: Label-Variable Cascade for Multi-Level `br`

Status: **Accepted, 2026-07-28.** Implemented in `crates/dewasm-backend-ruby/src/lib.rs`; supersedes the multi-level-`br` and loop-`catch`-value decisions of [ADR-4](4-ruby-backend-lowering.md) (its temps-hoisting and `call_indirect` decisions stand). The cross-frame relay protocol below — `__br`, the land-or-relay epilogue, the omitted outermost arm — is in turn superseded by [ADR-58](58-ruby-branch-by-value.md) and no longer runs on any path; the lean frame shapes and the depth-1 fast path stand, and remain the lowering for every function with no crossed frame.

## Context

ADR-4 lowered a multi-level `br` to Ruby `catch`/`throw`: a referenced frame became `catch(:lN) do ... end` and the branch a `throw :lN`. `catch`/`throw` is expensive in MRI. Profiling the converted `sqlite3-shell` (a workload that ran 24.0s) put `Kernel#catch` at 31.4% and `Kernel#throw` at 5.1% of CPU; each `throw` allocates one `T_IMEMO`, which accounted for 63–70% of the run's ~8M object allocations and drove GC to 16.5% of CPU. A label-variable cascade micro-benchmarked 2.8x faster at 4 levels deep and 4.3x at 16.

ADR-4 explicitly rejected flag variables as obscuring the code and needing a state-machine dispatch. That rejection predated ADR-4's own decoupling of result values from control: branch result values already travel through slot-copy `assigns` and method-scope-hoisted temps, never through the branch itself. So a control flag now carries *only* control — one method-local variable plus structured `break`/`next` is enough, and the per-block dispatch loop ADR-4 feared is unnecessary.

## Decision

- **A method-local `__br` holds the pending target label id** (`nil` = none; label ids start at 0, so `nil` is an unambiguous sentinel). It carries control only, hoisted with the temps when the function has any crossed frame.
- **Frames keep the lean shapes**: a `Block` or referenced-`If` is `begin ... end while false`; a `Loop` is `while true`.
- **Depth-1 fast path** — a `br` whose target is the innermost enclosing frame leaves it directly: `break` a block/if, `next` an unwrapped loop's back-edge.
- **A multi-level `br` sets `__br` and `break`s** the innermost scope. Every frame the branch crosses carries a land-or-relay epilogue, emitted *after* its scope, so the `break` skips any code left in the crossed body: if `__br` names this frame, clear it (the branch lands); otherwise `break` again to relay outward. The discriminating rule for needing an epilogue: a frame is *crossed* iff some outward `br` has it on the inclusive stack path from the target up to and including the branch's own innermost frame (whose bare `break` would otherwise land mid-body in its parent).
- **A loop targeted from a strictly nested frame is *wrapped***: its body moves into an inner `begin ... end while false` so the relayed `break` re-enters the loop head via `next` instead of exiting the `while`. Loops not targeted from a nested frame stay unwrapped with a plain `next` back-edge. A per-function pre-pass (`compute_frame_sets`) computes the crossed and wrapped sets.
- **The outermost frame omits its relay arm.** A bare `break` at method-body scope is a Ruby `SyntaxError`, and a pending branch can never target something outside the outermost frame, so that arm is dead anyway.

## Rejected alternatives

- **Keep `catch`/`throw` (ADR-4).** The `T_IMEMO` allocation and stack unwind dominate hot control flow — the 31%+5% CPU and 63–70% of allocations above are the direct cost.
- **Epilogue *inside* the scope, innermost frame excluded** (the first cut of this design). Incorrect: a `break` out of a nested frame lands mid-body in its parent, so any intervening parent code runs before the epilogue is reached, and a bare inner `break` skips the epilogue that should have relayed `__br`. The epilogue must sit after the scope, and the branch's own innermost frame needs one too. `sqlite3-shell` and hand-built mixed-depth `br_table` cases exercise this.
- **Relooper-style state machine / per-statement guards.** More code churn and a dispatch per block; the structured `break`/`next` cascade maps directly onto wasm's already-structured labels.

## Consequences

- Positive: `catch`/`throw` is gone from generated Ruby. `sqlite3-shell` on the benchmark workload dropped 24.0s → 6.95s (3.45x), byte-identical output; GC fell from 16.5% to 11%, and a CPU profile's top frames are now the work function and memory loads/stores, with no `Kernel#catch` or `Kernel#throw`.
- Negative: a *wrapped* loop's back-edge takes a `__br` assignment and a compare instead of a plain `next`; the common loop+block idiom stays unwrapped and keeps `next`. A multi-level `br` emits one small epilogue per crossed frame — an output-size cost (epilogues sit at deep indents, so they are emitted as single lines; measured on `sqlite3-shell`, the multi-line first cut grew the output by 37%, mostly leading whitespace).
- The spec harness (ADR-3) binds correctness: it passes for the Ruby backend under this lowering, including `br_table`, `unwind`, and `labels`.
