# Decision 32: Build-time Expression Folding

Status: **Accepted, 2026-07-28.**
The `FuncBuilder` folds single-use stack values into their consumers at IR-build time.
It is always on and identical for every backend.
It replaces the previous one-temp-per-instruction scheme.
Landed in `crates/dewasm-core/src/func.rs` behind no flag.
The `Expr` tree and every backend's `expr()` recursion are unchanged.

## Context

The IR flattened the wasm value stack into "temps": one variable per (stack depth, type) pair.
*Every* value-producing instruction emitted a `temp = <expr>` assignment (wasm2c style).
That made evaluation order and trap points trivially correct.
But it is one statement per instruction.
Converting the 35 MB `ruby.wasm` produced a 335 MB, 6.5-million-line `.rb`.
~60 % of its statements were `sN = ...` assigns.
A third of *those* were trivial `sN = <const|local|temp>` copies.
The cost is paid by every backend and at every stage:

- file size;
- the target's parse time;
- its compile/load time;
- runtime (an extra variable write and read per instruction).

`Expr` was already a nested tree (`Un`/`Bin`/`Load`/`Select` hold `Box<Expr>`).
Every backend's `expr()` already walked the tree by recursion.
So the machinery to *emit* folded expressions existed on the backend side.
Only the builder stored every value in a temp at once.

The hard part is correctness: folding a value into a later consumer moves *when* it is evaluated.
wasm evaluates strictly, left to right.
The specification asserts specific trap messages and post-trap state.
A fold is only sound if three conditions hold:

- the moved expression cannot observe an intervening effect;
- its own trap still fires at the right point;
- it does not cross a control-flow boundary the temp model relies on.

## Decision

Rework `FuncBuilder` into a **pending-expression stack** (w2c2 style).
Each operand-stack slot may hold a *pending* `Expr` together with its `Effects` and its node count.
The `Effects` say which locals/globals/memory it reads, and whether it can trap.
A value producer pushes a pending instead of emitting an assign.
A consumer pops its operands as expressions and composes them.
A pending is **spilled** to a temp (`sN = <expr>`) only when keeping it folded is unsafe or unprofitable.
Besides call/branch results, a spill is the only thing that now creates a temp.
No IR types change.
`Func.temps` ends up listing exactly the temps that are written.
So every backend's temp declarations get smaller for free.

**Spill discipline.**
Before emitting each statement, spill the pendings that could observe its effect.
Also spill the pendings whose trap must fire first, per statement kind:

- `local.set{k}` / `local.tee{k}`: pendings that read local `k`.
- `global.set`: `globals || trap` (post-trap global state is observable).
- `store` / `memory.{grow,copy,fill,init}`: `memory || trap`.
- `call` / `call_indirect`: `globals || memory || trap`.
  Arguments that read only locals survive and fold into the call (the main win, `_f5(l0, l1)`).
  `call_indirect` spills operands with effects *before* popping.
  So the index/argument fragments left inline are pure and cannot reorder an observable effect.
- `unreachable`: `trap`.
  A pending OOB load must trap with its own "out of bounds" message, not be shadowed by "unreachable".
- `br`/`br_if`/`br_table` to a label: spill everything.
  Branch targets read temps at fixed depths.
  A not-taken `br_if` must leave the operands reusable.
  `return` / `br` to the function frame / fall-through instead *fold* the return values.
  They do so after spilling any deeper trapping pending.
- block/loop/if entry, `else`, `end`: spill everything.
  So values that cross a control boundary stay in temps.
  The `if` condition is folded into the frame first.
- `select`: `cond` folds freely, but a trapping `then`/`els` arm is spilled.
  wasm always evaluates both arms.
  Ruby/Java/Python/Bash lower `select` to a conditionally-evaluated ternary, though.
  So only trap-free arms may be inlined.
- `drop`: a trapping pending is spilled (its trap must fire); a pure one is dropped.

**Cap.**
`MAX_FOLD_SIZE = 32` nodes.
When composing would exceed it, the operands are spilled first and referenced as temps.
The cap keeps expressions shallow enough for a target language's parser stack.
It also bounds the worst-case growth of the text in some backends.
Those are backends whose inline lowerings repeat an operand.

Two backend adjustments were needed.
Folded expressions now reach code that assumed plain-variable operands:

- **Go** rejects a compile-time constant conversion outside the destination range.
  An example is `int32(uint32(4294967231))`.
  A folded i32/i64 constant can land directly inside a signed cast.
  So large constants pass through new `rt/i32c`/`rt/i64c` identity helpers.
  A call result is never a constant, so the conversion becomes a runtime one.
  It was a runtime one before folding too, when every value passed through a variable.
- **Bash**: `memory.grow` evaluated its delta fragment three times.
  A folded `memory.size` delta would read the already-grown `pages`.
  It now snapshots the candidate page count once.
  And `value()`'s `Bin` arm snapshots non-trivial operands.
  It does so before inline lowerings that repeat them in the text.
  `I64ShrU` names an operand four times.

## Rejected alternatives

- **Keep the old scheme (one temp per instruction).**
  Simplest and obviously correct.
  It leaves the measured 335 MB / 6.5 M-line output on the table for every backend, though.
  So does its parse/compile/runtime cost.
- **A post-build optimization pass over the IR.**
  Fold as a separate pass that rewrites `Assign`-heavy IR into nested expressions.
  It would need to re-derive the effect/aliasing information.
  The builder already has that information as it walks the operand stack.
  It would re-do the trap- and effect-ordering analysis.
  It would do so on a form that has already lost the stack structure.
  Building the folded form directly is less code and less repeated reasoning.
  Readability passes (decision 1) can still layer on top.
- **Per-backend folding.**
  Each backend folds during its own lowering.
  That multiplies the careful trap/effect-ordering logic by the number of backends.
  It also invites backends drifting apart.
  The shared builder keeps one audited implementation and one specification harness test.

## Consequences

- Output gets smaller and loads/runs faster for **all** backends at once; no backend had to enable it.
  On the 35 MB `ruby.wasm` (`--mode standalone`):

  | measure | baseline (unfolded) | folded | change |
  | --- | --- | --- | --- |
  | file size | 335 MB | 165 MiB (173,419,563 B) | ~halved |
  | lines | 6,535,230 | 2,814,761 | −57 % |
  | `ruby -c` (parse) | 3.8 s | 2.1 s | −44 % |
  | ISeq compile | 10.7 s | 6.4 s | −40 % |
  | run (`--dir …::/usr -- -e 'puts "hello #{6*7}"'`) | 63 s | 37.9 s | 1.66× |

- `Func.temps` now holds only temps that are written (spills, call results, branch-assign destinations).
  Backends that walk it to declare variables get smaller automatically.
  No backend changed for declarations.
- Correctness is bound by the specification harness, as always (decision 3).
  The full testsuite passes for every backend.
  Targeted IR-shape unit tests in `crates/dewasm-core/tests/folding.rs` add to it.
  Generated output with folding turned off was verified identical to the previous scheme.
  That made the rework safe before the fold was turned on.
- New invariants a backend may rely on (documented in `ir.rs`):
  - an `Expr` tree preserves wasm's left-to-right evaluation order and trap points;
  - a `Select`'s `then`/`els` expressions are pure and non-trapping;
  - `Func.temps` lists exactly the temps that are written.
- The `Effects` local set is a 64-bit mask plus an `any_high_local` catch-all for indices ≥ 64.
  It is conservative: a set of a high local spills all pendings reading any high local.
  That is rare and never wrong.
