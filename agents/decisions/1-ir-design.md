# Decision 1 — IR Design: Structured Control Flow + Stack-Slot Temps

Status: **Accepted, 2026-07-23.**
Backfilled; implemented in `crates/dewasm-core/src/ir.rs` and `func.rs`.
The expression-folding readability pass remains future work.

## Context

Backends emit source code in languages without `goto`, so the IR shape decides how hard every backend's control-flow and evaluation-order story is.
Wasm 1.0 bodies are stack-machine code, but their control flow is already structured (`block` / `loop` / `if` / `br` / `br_table` — branches only jump to enclosing labels).

## Decision

- **Keep wasm's structured control flow as-is.**
  The IR statement tree mirrors block/loop/if with resolved branch targets; no CFG, no relooper.
  Every structured target language can express "exit block N" / "continue loop N" one way or another (decision 4 for Ruby's), so re-deriving structure from a CFG would be pure waste.
  Labels carry a `referenced` flag so backends emit no machinery for branch-free blocks.
- **Flatten the value stack into temps keyed by (depth, type)**, the wasm2c approach, using the type information tracked during translation.
  The same slot is one target-language variable.
- **Materialize every pushed value into a temp immediately.**
  The criterion: *evaluation order and trap points must be provably identical to wasm's without any effect analysis.*
  A `drop` of a loaded value still traps on out-of-bounds; call/store ordering needs no reasoning.
  The cost is verbose output; folding single-use pure temps back into expressions is a planned, separate pass that must not change semantics.
- **Dead code after an unconditional branch is skipped tracking only block nesting** — no type tracking in unreachable code.
  Runtime-safe because the skipped region is never entered.
- Branch moves (`br` operands → target result/param slots) are computed during translation and stored on the branch (`BrTarget::Label.assigns`), with self-assignments already filtered; backends just print them.

## Rejected alternatives

- **Expression-tree reconstruction from the start** — better-looking output, but correctness (side-effect ordering, trap points) becomes an analysis problem in every backend.
  Ordered as: correct first, pretty later.
- **CFG + relooper** — needed only for input that has arbitrary jumps; wasm 1.0 does not.

## Consequences

- Positive: backends are lowering tables, not compilers; the spec harness (decision 3) passed on the first backend with only lowering bugs, no IR redesign.
- Negative: generated code reads like register transfers (`s0 = l0`; `s0 = (s0 + s1) & 0xffffffff`) until the folding pass exists; output size is larger than hand-written style.
- Multi-value blocks/returns are representable for free (result slots are just consecutive temps), so the multi-value feature costs backends almost nothing.
