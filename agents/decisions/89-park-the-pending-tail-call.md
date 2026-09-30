# Decision 89: Park a Pending Tail Call, Never Allocate One

Status: **Accepted, 2026-08-31.**
A tail call writes its target and arguments into per-instance slots and returns.
The entry's trampoline reads them back.
No backend allocates anything per step.

## Context

[Decision 18](18-ruby-tail-calls.md)'s trampoline was first implemented with a thunk.
The thunk is an object holding the target and an array of arguments.
The body returns it, and the entry unwraps it.
That is the obvious shape, and it costs an allocation, or several, on every step.

`wat/tail_call` measured that cost against `call_direct`.
`call_direct` is the same chain of the same four functions, made of ordinary calls.
The ratios were 9.93x on Go, 8.52x on Java, and 6.42x on Ruby with YJIT.
A tail call was an order of magnitude more expensive than the call it replaces.
That held on the two backends where an ordinary call is nearly free.

The count was taken on the converted official wasm3 asset, the app the proposal was accepted for.
There the shape that matters is the indirect one.
It has 523 indirect tail calls, 5 direct, and no self tail calls at all.
So the step itself is the whole cost.
Nothing about the surrounding code shape can be optimized instead.

## Decision

A tail call parks, and the entry's trampoline collects:

- The **target** comes out of a per-instance table of tail entries built once at instantiation.
  So no callable is constructed per step.
  A slot in a function table carries the same entry.
  So an indirect tail call parks without allocating either.
- The **arguments** go into per-position, per-type slots on the instance, so no argument array is built.
  Ruby and Python dispatch the trampoline's call by parked arity.
  That is the way `table/call<n>` already avoids a splat ([decision 44](44-ruby-call-indirect-arity.md)).
  An arity past the fixed set parks an array, which no shipped app reaches.
- The target is cleared before dispatching, so a callee that does not tail-call ends the chain.
  The arguments are read as the dispatch call's own operands, before the callee can write over them.

Bash was already this shape, because a global was the only place its pending call could live.
What this decision does is bring the other five to it.

A tail entry is bound to the instance that built it, and reads *that* instance's slots.
So the two statically-typed backends check ownership.
`Rt.Funcref` carries the owner alongside the entry.
A slot is only parked by the instance that owns it.
Anything else is wrapped instead of parked.
That covers a foreign instance's entry, and a callee with no entry at all.
That is one allocation, on a path a chain does not take.
Go needs no wrapper.
Its tail call has already left every outer `try_table` closure by the time it is emitted.
Java does need one, and getting that wrong is observable rather than merely slow (see below).

The argument expressions are safe to write straight into the slots.
That is because the IR spills an operand with side effects before the instruction.
So nothing between the assignments can reach another trampoline.

Code this governs:
- the `Stmt::ReturnCall`/`Stmt::ReturnCallIndirect` lowering and the entry in every backend;
- each backend's `rt/tail_call` unit and its table's tail-entry column.

## Rejected alternatives

- **Keep the thunk and make the allocation cheaper** (a reused instance, a `struct` rather than a class).
  The argument array remains.
  In Go the thunk is a closure whose whole cost *is* the capture.
- **Defunctionalize the mutually tail-calling set into one dispatch loop.**
  A step is then a state assignment.
  It was measured 2.85x faster than parking at ten arms, even at two hundred.
  At five hundred it was nine times *slower*, past the code size the JIT handles well.
  It could be had as a pass limited to a small group, and it is not being taken.
  The app the proposal was accepted for has a group of 519, squarely past that size.
  So the pass would refuse exactly the case that motivated the work.
  It would also buy a whole-module analysis with a closed-world requirement.
  The return would be guests nobody has added to the app list.
  The measurements are kept so the question does not have to be reopened from the start.
- **Extract the arms into methods and dispatch by `switch`**, keeping each arm JIT-compilable.
  It was measured to tie parking in Go.
  In Ruby it needs a binary decision tree rather than a `case` just to reach the same point.
- **Rewrite a self tail call into a loop**, which needs no trampoline at all.
  Worth doing, and done separately ([decision 90](90-self-tail-call-to-loop.md)).
  But it is not this: the motivating app has no self tail calls.

## Consequences

- Positive, measured on `wat/tail_call` against the same conversion before the change:

  | Backend | Before (nanoseconds per step) | After (nanoseconds per step) |
  | --- | --- | --- |
  | Go | 40.9 | 8.4 (4.85x) |
  | Ruby with YJIT | 653 | 269 (2.43x) |
  | Java | 27.1 | 18.4 |
  | Perl | 3655 | 2480 |
  | Python | 854 | 684 |

  Against an ordinary call, Go's tail call goes from 9.93x to 2.05x, and Ruby's from 6.42x to 2.50x.
- Positive: converted wasm3 on Ruby runs 8.36 to 6.75 us per guest iteration, and starts 14% faster.
  Its artifact is 28% smaller, because a parked call spells out less than a constructed one.
- Carry-over: Java's entry still boxes the chain's final result, since a tail entry returns `Object`.
  Typing the entry table per result signature would remove that, and is not done here.
- Carry-over: the slots are per instance.
  So two instances sharing a table take the wrapped path between them.
  That is a correctness requirement, not a tuning choice, and the ownership check makes sure of it.
