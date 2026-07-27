# ADR-32 — Build-time Expression Folding

Status: **Accepted, 2026-07-28.** The func builder folds single-use stack
values into their consumers at IR-build time, always on and identical for
every backend, replacing the previous one-temp-per-instruction scheme. Landed
in `crates/dewasm-core/src/func.rs` behind no flag; the `Expr` tree and every
backend's `expr()` recursion are unchanged.

## Context

The IR flattened the wasm value stack into "temps": one variable per (stack
depth, type) pair, and *every* value-producing instruction emitted a
`temp = <expr>` assignment (wasm2c style). That made evaluation order and trap
points trivially correct, but it is one statement per instruction. Converting
the 35 MB `ruby.wasm` produced a 335 MB, 6.5-million-line `.rb`: ~60 % of its
statements were `sN = ...` assigns, and a third of *those* were trivial
`sN = <const|local|temp>` copies. The cost is paid by every backend and at
every stage — file size, the target's parse time, its compile/load time, and
runtime (an extra variable write and read per instruction).

`Expr` was already a nested tree (`Un`/`Bin`/`Load`/`Select` hold `Box<Expr>`)
and every backend's `expr()` already recursed. So the machinery to *emit* folded
expressions existed on the backend side; only the builder materialized eagerly.

The hard part is correctness: folding a value into a later consumer moves *when*
it is evaluated. wasm evaluates strictly, left to right, and the spec asserts
specific trap messages and post-trap state. A fold is only sound if the moved
expression cannot observe an intervening effect, its own trap still fires at the
right point, and it does not cross a control-flow boundary the temp model relies
on.

## Decision

Rework `FuncBuilder` into a **pending-expression stack** (w2c2 style). Each
operand-stack slot may hold a *pending* `Expr` together with its `Effects` (which
locals/globals/memory it reads, and whether it can trap) and its node count. A
value producer pushes a pending instead of emitting an assign; a consumer pops
its operands as expressions and composes them. A pending is **spilled** to a temp
(`sN = <expr>`, the only thing that now creates a temp besides call/branch
results) only when keeping it folded would be unsafe or unprofitable. No IR types
change; `Func.temps` ends up listing exactly the materialized temps, so every
backend's temp declarations shrink for free.

**Spill discipline** (before emitting each statement, spill the pendings that
could observe its effect or whose trap must fire first):

- `local.set{k}` / `local.tee{k}`: pendings that read local `k`.
- `global.set`: `globals || trap` (post-trap global state is observable).
- `store` / `memory.{grow,copy,fill,init}`: `memory || trap`.
- `call` / `call_indirect`: `globals || memory || trap` — pure-local args
  survive and fold into the call (the main win, `_f5(l0, l1)`). `call_indirect`
  spills effectful operands *before* popping so the index/arg fragments left
  inline are pure and cannot reorder an observable effect.
- `unreachable`: `trap` (a pending OOB load must trap with its own "out of
  bounds" message, not be shadowed by "unreachable").
- `br`/`br_if`/`br_table` to a label: spill everything (branch targets read
  temps at canonical depths, and a not-taken `br_if` must leave the operands
  reusable). `return` / `br` to the function frame / fall-through instead *fold*
  the return values, after spilling any deeper trapping pending.
- block/loop/if entry, `else`, `end`: spill everything, so
  control-boundary-crossing values stay materialized. The `if` condition is
  folded into the frame first.
- `select`: `cond` folds freely, but a trapping `then`/`els` arm is spilled —
  wasm evaluates both arms eagerly, whereas Ruby/Java/Python/Bash lower `select`
  to a conditionally-evaluated ternary, so only trap-free arms may be inlined.
- `drop`: a trapping pending is spilled (its trap must fire); a pure one is
  discarded.

**Cap.** `MAX_FOLD_SIZE = 32` nodes. When composing would exceed it, the
operands are spilled first and referenced as temps. The cap keeps expressions
shallow enough not to blow a target language's recursive-descent parser stack,
and bounds the worst-case textual blow-up of backends whose inline lowerings
duplicate an operand.

Two backend adjustments were needed because folded expressions now reach code
that assumed bare-variable operands:

- **Go** rejects a compile-time constant conversion outside the destination
  range (`int32(uint32(4294967231))`). A folded i32/i64 constant can land
  directly inside a signed cast, so large constants are laundered through new
  `rt/i32c`/`rt/i64c` identity helpers — a call result is never a constant, so
  the conversion becomes a runtime one (as it was before folding, when every
  value passed through a variable).
- **Bash**: `memory.grow` evaluated its delta fragment three times (a folded
  `memory.size` delta would read the already-grown `pages`); it now snapshots
  the candidate page count once. And `value()`'s `Bin` arm snapshots non-trivial
  operands before inline lowerings that textually repeat them (`I64ShrU` names an
  operand four times).

## Rejected alternatives

- **Status quo (one temp per instruction).** Simplest and obviously correct, but
  leaves the measured 335 MB / 6.5 M-line output and its parse/compile/runtime
  cost on the table for every backend.
- **A post-build optimization pass over the IR.** Fold as a separate pass that
  rewrites `Assign`-heavy IR into nested expressions. It would need to
  re-derive the effect/aliasing information the builder already has as it walks
  the operand stack, re-doing the trap- and effect-ordering analysis on a form
  that has already lost the stack structure. Building the folded form directly
  is less code and less duplicated reasoning; readability passes (ADR-1) can
  still layer on top.
- **Per-backend folding.** Each backend folds during its own lowering. That
  multiplies the delicate trap/effect-ordering logic by the number of backends
  and invites divergence; doing it once in the shared builder keeps a single
  audited implementation and one spec-harness gate.

## Consequences

- Output shrinks and loads/runs faster for **all** backends at once; no backend
  opted in. On the 35 MB `ruby.wasm` (`--mode standalone`):

  | metric | baseline (unfolded) | folded | change |
  | --- | --- | --- | --- |
  | file size | 335 MB | 165 MiB (173,419,563 B) | ~halved |
  | lines | 6,535,230 | 2,814,761 | −57 % |
  | `ruby -c` (parse) | 3.8 s | 2.1 s | −44 % |
  | ISeq compile | 10.7 s | 6.4 s | −40 % |
  | run (`--dir …::/usr -- -e 'puts "hello #{6*7}"'`) | 63 s | 37.9 s | 1.66× |

- `Func.temps` now holds only materialized temps (spills, call results,
  branch-assign destinations); backends that iterate it to declare variables
  shrink automatically, with no backend change for declarations.
- Correctness is bound by the spec harness, as always (ADR-3): the full
  testsuite passes for every backend, plus targeted IR-shape unit tests in
  `crates/dewasm-core/tests/folding.rs`. Generated output was verified
  byte-identical to the previous scheme with folding disabled, de-risking the
  refactor before the fold was turned on.
- New invariants a backend may rely on (documented in `ir.rs`): an `Expr` tree
  preserves wasm's left-to-right evaluation order and trap points; a `Select`'s
  `then`/`els` subexpressions are pure and non-trapping; `Func.temps` lists
  exactly the materialized temps.
- The `Effects` local set is a 64-bit mask plus an `any_high_local` catch-all for
  indices ≥ 64 — conservative (a set of a high local spills all pendings reading
  any high local), which is rare and never wrong.
