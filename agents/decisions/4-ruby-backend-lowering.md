# Decision 4 — Ruby Backend Lowering Conventions

Status: **Accepted, 2026-07-23.**
Backfilled; implemented in `crates/dewasm-backend-ruby/src/lib.rs` + `runtime/ruby/`.
Numeric conventions are decision 2's; this decision covers control flow and object shape.
The multi-level-`br` and loop-`catch`-value decisions (and the flag-variable rejection) are superseded by [decision 42](42-ruby-label-variable-cascade.md); the temps-hoisting decision stands, as does `call_indirect`'s structural type-symbol comparison — but its splat-array dispatch is amended by [decision 44](44-ruby-call-indirect-arity.md) (fixed-arity `Table#callN`).

## Context

Ruby has no labeled break/continue, creates a new variable scope inside `do ... end` blocks, and compares `call_indirect` targets by nothing — the backend must supply wasm's structural type check.
These three language facts drove the lowering shape.

## Decision

- **Multi-level `br` lowers to `catch`/`throw`.**
  *(Superseded by [decision 42](42-ruby-label-variable-cascade.md): the `__br` label-variable cascade.)* A referenced block label becomes `catch(:lN) do ... end`; `br` becomes result-slot assignments followed by `throw :lN`.
  Unreferenced labels emit nothing (decision 1's `referenced` flag).
- **Loops become `while true` wrapping a `catch` whose value picks continue vs. exit**: *(Superseded by [decision 42](42-ruby-label-variable-cascade.md).)* the body falls through to `true` (break the while) and a back-edge `throw`s `false` (next iteration).
  One shape covers branches to the loop head from any nesting depth.
- **A `br`/`br_if`/`br_table` at the innermost capturing frame — depth 1 — lowers to a plain `break`/`next` instead of `throw`, and a frame every one of whose incoming branches is depth-1 drops `catch`/`throw` entirely** *(Superseded by [decision 42](42-ruby-label-variable-cascade.md), which drops `catch`/`throw` for every frame, not only depth-1-only ones; the depth-1 `break`/`next` fast path it keeps.)* (`Block` renders as `begin ... end while false`, `Loop` as a bare `while true ... end` with an appended trailing `break` for fallthrough).
  This was the Consequences section's candidate optimization below, now adopted: `break`/`next` inside a `catch(...) do ... end` block still terminates/short-circuits it exactly like a `throw` would (Ruby block semantics), so the branch-site simplification is correct regardless of whether the target frame kept its `catch` wrapper — only frames with *zero* deeper incoming throws can drop the wrapper itself.
  A per-function pre-pass (`compute_break_only` in `crates/dewasm-backend-ruby/src/lib.rs`) computes, per label, whether every branch to it is depth-1; `plain if`, `br_if`'s wrapper `if`, and `br_table`'s `case` never capture, so they don't count as a frame boundary for this analysis.
  Measured in isolation (20M-iteration loop back-edge, MRI 4.0.4): `catch`/`throw` 2.59s vs. `break`/`next` 0.45s, ~5.7x; (5M-call block-exit, same benchmark): `catch`/`throw` 0.69s vs. `begin...end while false` 0.17s, ~4.1x.
  `catch` and `throw` are genuinely expensive in MRI — this is the actual driver of hot-loop overhead in generated code, not incidental.
- **All stack temps are hoisted to method scope** with a single `s0 = s1 = ... = nil` line at function entry.
  First assignment inside a Ruby block is block-local, so without hoisting, values assigned inside `catch` blocks vanish at `end` (found by spec-harness NameErrors, not foreseen).
- **`call_indirect` compares structural type symbols**: the backend renders each type index as a symbol interned from the type's shape (e.g. `:"i32,i64->i32"`), both when populating the table and at call sites.
  Wasm compares function types structurally, not by index — and any module-local id (even a canonicalized index) breaks once a table is shared across modules via an imported table, whose index spaces differ.
  *(The dispatch used a splat: `@tT.call(index, type_sym, *args)` re-splatting into `func.call(*args)`.
  Amended by [decision 44](44-ruby-call-indirect-arity.md): a per-arity `Table#callN` drops both splats; the structural-symbol comparison here is unchanged.)*
- **Module = one class**: imports resolved in `initialize` (`@ifN` ivars), globals as `@gN`, exports in an `@exports` hash keyed by the raw export name with `invoke(name, *args)` as the entry point (export names need not be valid Ruby method names), memory exposed via `attr_reader`.
  Data segments embed as hex strings decoded with `pack("H*")` (no `require` needed, unlike base64).

## Rejected alternatives

- **Flag variables / state-machine dispatch for multi-level br** — both obscure the code far more than catch/throw, and the state machine costs a dispatch loop per block; catch/throw benchmarked acceptably and maps 1:1 to label semantics.
  *(Reversed by [decision 42](42-ruby-label-variable-cascade.md): once result values are decoupled from control — via the `assigns` slot-copies and hoisted temps above — a control-only flag needs no per-block dispatch, and `catch`/`throw`'s allocation cost turned out to dominate hot loops.)*
- **`define_method` per export as the public API** — export names collide with `Object` methods and Ruby keywords; a name-keyed hash plus `invoke` is collision-free.
  Friendly named methods can be layered on later.

## Consequences

- Positive: the whole control-flow story is three emission shapes; the spec harness (decision 3) passes, including `br_table`, `unwind`, and `labels`.
- Negative: `catch` allocates and `throw` unwinds — hot loops pay for labels they rarely take.
  Mitigated for the common depth-1 case (see the `break`/`next` decision above); a multi-level `br` (depth > 1) still pays the full `catch`/`throw` cost, unavoidably — Ruby has no labeled `break`.
- Deep wasm recursion maps to Ruby stack frames; `SystemStackError` is the (accepted) analogue of "call stack exhausted".
