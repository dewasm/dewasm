# ADR-28 — Python Backend Lowering Conventions

Status: **Accepted, 2026-07-26.** First milestone ("cowsay runs", ADR-24)
implemented in `crates/dewasm-backend-python/src/lib.rs` + `runtime/python/`.
Numeric conventions are ADR-2's; this ADR covers the two places Python forced
a different shape from Ruby (ADR-4): control flow and float division. The spec
harness and the remaining WASI surface are a later milestone.

## Context

Python is dynamically typed with arbitrary-precision ints and IEEE doubles, so
ADR-2's masked-unsigned integers and double-backed f32-with-re-rounding
transfer from Ruby almost verbatim (`Rt.s32`/`s64`, `Rt.f32`, the software NaN
bit paths). Three language facts did *not* transfer:

1. **Python has no non-local control transfer.** Ruby's whole control-flow
   story is `catch`/`throw` (ADR-4); Python has neither that nor `goto` nor
   labeled `break`/`continue`.
2. **Python caps statically-nested loops/`try` at 20** ("too many statically
   nested blocks"), while `if` nests ~100 deep. Real wasm binaries nest far
   deeper: cowsay's hottest function nests referenced blocks/loops/ifs 42 deep
   (measured), and blocks (forward branches) dominate that.
3. **Python raises on `x / 0.0`**, on `math.sqrt` of a negative, and
   `struct.pack("<f")` raises `OverflowError` past the f32 range — where Ruby
   returns `inf`/`nan`. Integer `//`/`%` also floor rather than truncate.

## Decision

- **Forward branches (block/if exits) use a per-function branch register
  `_br`, not a loop or `try`.** A `br` to a block/if sets `_br = <label id>`;
  each statement after a possible branch is guarded by `if _br == 0:`; a
  referenced label emits an `if _br == <id>: _br = 0` reset marker at its end.
  Because only `while`/`try` count toward the 20-block cap, this keeps blocks
  free of it.
- **Block/if bodies are spliced inline into the enclosing statement list**, so
  block *nesting* adds zero Python nesting; the guards are siblings, so
  sequence *length* adds none either. cowsay's 42-deep wasm nesting lowers to
  a max Python indent of 9. Guards are emitted only after a statement whose
  subtree can leave `_br` set (`stmt_free_targets`), so straight-line code is
  unguarded.
- **Only real loops become `while True:`** with a trailer
  `if _br == <id>: _br = 0; continue` / `break`; a back-edge `br` sets `_br`
  and the trailer turns it into `continue`. Loop nesting is small (5 in
  cowsay) and is the *only* contributor to the 20-block budget. A guarded `if`
  folds its guard into the condition (`if _br == 0 and (cond) != 0:`) so a
  trapping `cond` is not evaluated while a branch is pending.
- **`Rt.fdiv` wraps float division** (returns IEEE `inf`/`nan` instead of
  raising); `Rt.f32` catches `OverflowError`; `fsqrt`/`div_s`/`rem_s` guard the
  negative/zero/truncation cases exactly as the Ruby units do (integer
  `div_s`/`rem_s` use `abs`-based truncation, never `//`/`%`).
- **Runtime lives at module top level, not nested in the generated class.**
  Python method scopes cannot see an enclosing class scope, so a nested
  `class Rt` would make `Rt.trap` unresolvable inside a method; `Rt` is emitted
  as a top-level class (`class Memory`/`Table`/`WASI` nested inside it), one
  self-contained module per file. Module = one class; imports resolved in
  `__init__` (`self.ifN`), own globals as plain `self.gN` attributes, exports
  in a `self.exports` dict with `invoke(name, *args)` as the entry point.
- **`call_indirect` compares structural type strings** (`"i32,i64->i32"`),
  like Ruby's symbols and for the same reason (shared tables, ADR-4).

## Rejected alternatives

- **Exceptions for `br` (a `_Br` exception per label, or one per function).**
  A `try` per block hits the 20-block cap exactly as loops would; a single
  per-function `try` cannot express "resume after *this* block" without a
  dispatch loop. The branch register is flat and cap-free.
- **Mirror Ruby's `catch`/`throw` shape with single-iteration `while` loops
  for blocks.** Correct, but every block then costs a loop, so cowsay's
  38-deep block+loop nesting blows the 20-loop cap immediately.
- **Box own globals in an `Rt.Global` cell (as Ruby does).** Only needed for
  sharing a mutable global across modules via an *imported* global, which this
  backend rejects at conversion time (ADR-16); plain attributes suffice.

## Consequences

- Positive: cowsay is byte-identical to the wasmtime golden for both the
  args and stdin cases; qjs and sqlite3 convert and compile. The control-flow
  scheme is depth-insensitive, so no relooper/label-dispatch was needed.
- Negative: guards add an `if _br == 0:` and a comparison per branchy
  statement — more lines and a small per-statement cost versus Ruby's
  `catch`/`throw`. `_br` is a whole-function register, so it serializes
  control flow textually rather than structurally.
- Carry-over: this milestone bundles only the eight WASI syscalls cowsay
  needs (args/environ, fd_read/fd_write, proc_exit, random_get); the spec
  harness, the full WASI surface, and the filesystem are later work. gzip
  (minigzip) needs fd_fdstat_get/fd_prestat_*/fd_seek/path_open and so is not
  yet wired.
