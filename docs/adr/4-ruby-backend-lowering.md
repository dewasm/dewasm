# ADR-4 — Ruby Backend Lowering Conventions

Status: **Accepted, 2026-07-23.** Backfilled; implemented in
`crates/dewasmify-backend-ruby/src/lib.rs` + `runtime/ruby/`. Numeric
conventions are ADR-2's; this ADR covers control flow and object shape.

## Context

Ruby has no labeled break/continue, creates a new variable scope inside
`do ... end` blocks, and compares `call_indirect` targets by nothing —
the backend must supply wasm's structural type check. These three
language facts drove the lowering shape.

## Decision

- **Multi-level `br` lowers to `catch`/`throw`.** A referenced block
  label becomes `catch(:lN) do ... end`; `br` becomes result-slot
  assignments followed by `throw :lN`. Unreferenced labels emit nothing
  (ADR-1's `referenced` flag).
- **Loops become `while true` wrapping a `catch` whose value picks
  continue vs. exit**: the body falls through to `true` (break the while)
  and a back-edge `throw`s `false` (next iteration). One shape covers
  branches to the loop head from any nesting depth.
- **All stack temps are hoisted to method scope** with a single
  `s0 = s1 = ... = nil` line at function entry. First assignment inside a
  Ruby block is block-local, so without hoisting, values assigned inside
  `catch` blocks vanish at `end` (found by spec-harness NameErrors, not
  foreseen).
- **`call_indirect` compares structural type symbols**: the backend
  renders each type index as a symbol interned from the type's shape
  (e.g. `:"i32,i64->i32"`), both when populating the table and at call
  sites. Wasm compares function types structurally, not by index — and
  any module-local id (even a canonicalized index) breaks once a table
  is shared across modules via an imported table, whose index spaces
  differ.
- **Module = one class**: imports resolved in `initialize` (`@ifN`
  ivars), globals as `@gN`, exports in an `@exports` hash keyed by the
  raw export name with `invoke(name, *args)` as the entry point (export
  names need not be valid Ruby method names), memory exposed via
  `attr_reader`. Data segments embed as hex strings decoded with
  `pack("H*")` (no `require` needed, unlike base64).

## Rejected alternatives

- **Flag variables / state-machine dispatch for multi-level br** — both
  obscure the code far more than catch/throw, and the state machine
  costs a dispatch loop per block; catch/throw benchmarked acceptably and
  maps 1:1 to label semantics.
- **`define_method` per export as the public API** — export names collide
  with `Object` methods and Ruby keywords; a name-keyed hash plus
  `invoke` is collision-free. Friendly named methods can be layered on
  later.

## Consequences

- Positive: the whole control-flow story is three emission shapes; the
  spec harness (ADR-3) is green including `br_table`, `unwind`, and
  `labels`.
- Negative: `catch` allocates and `throw` unwinds — hot loops pay for
  labels they rarely take. Candidate optimization: use `break`/`next`
  when the branch depth is 1, measured before adopted.
- Deep wasm recursion maps to Ruby stack frames; `SystemStackError` is
  the (accepted) analogue of "call stack exhausted".
