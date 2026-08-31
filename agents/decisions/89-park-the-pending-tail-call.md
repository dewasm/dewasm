# Decision 89: Park a Pending Tail Call, Never Allocate One

Status: **Accepted, 2026-08-31.**
A tail call writes its target and arguments into per-instance slots and returns; the entry's trampoline reads them back. No backend allocates anything per hop.

## Context

[Decision 18](18-ruby-tail-calls.md)'s trampoline was first implemented with a thunk: an object holding the target and an array of arguments, returned by the body and unwrapped by the entry.
That is the obvious shape, and it costs an allocation, or several, on every hop.

`wat/tail_call` measured what that costs against `call_direct`, the same chain of the same four functions made of ordinary calls: 9.93x on Go, 8.52x on Java, 6.42x on Ruby with YJIT.
A tail call was an order of magnitude more expensive than the call it replaces, on the two backends where an ordinary call is nearly free.

Counted on the converted official wasm3 asset, the app the proposal was accepted for, the shape that matters is the indirect one: 523 indirect tail calls, 5 direct, and no self tail calls at all.
So the hop itself is the whole cost, and nothing about the surrounding code shape can be optimized instead.

## Decision

A tail call parks, and the entry's trampoline collects:

- The **target** comes out of a per-instance table of tail entries built once at instantiation, so no callable is constructed per hop.
  A slot in a function table carries the same entry, so an indirect tail call parks without allocating either.
- The **arguments** go into per-position, per-type slots on the instance, so no argument array is built.
  Ruby and Python dispatch the trampoline's call by parked arity, the way `table/call<n>` already avoids a splat ([decision 44](44-ruby-call-indirect-arity.md)); an arity past the fixed set parks an array, which no shipped app reaches.
- The target is cleared before dispatching, so a callee that does not tail-call ends the chain, and the arguments are read as the dispatch call's own operands, before the callee can overwrite them.

Bash was already this shape, because a global was the only place its pending call could live; what this decision does is bring the other five to it.

A tail entry is bound to the instance that built it and reads *that* instance's slots, so the two statically-typed backends check ownership: `Rt.Funcref` carries the owner alongside the entry, and a slot is only parked by the instance that owns it.
Anything else, a foreign instance's entry or a callee with no entry at all, is wrapped instead of parked: one allocation, on a path a chain does not take.
Go needs no wrapper, because its tail call has already left every enclosing `try_table` closure by the time it is emitted; Java does, and getting that wrong is observable rather than merely slow (see below).

The argument expressions are safe to write straight into the slots because the IR spills an effectful operand before the instruction, so nothing between the assignments can reach another trampoline.

Code this governs: the `Stmt::ReturnCall`/`Stmt::ReturnCallIndirect` lowering and the entry in every backend, each backend's `rt/tail_call` unit and its table's tail-entry column.

## Rejected alternatives

- **Keep the thunk and make the allocation cheaper** (a reused instance, a struct rather than a class).
  The argument array remains, and in Go the thunk is a closure whose whole cost *is* the capture.
- **Defunctionalize the mutually tail-calling set into one dispatch loop**, so a hop is a state assignment.
  Measured: 2.85x faster than parking at ten arms, even at two hundred, and nine times *slower* at five hundred, where the JIT's code size falls off a cliff; the app's group is 519.
  Worth revisiting only as a pass gated on a small group.
- **Extract the arms into methods and dispatch by `switch`**, keeping each arm JIT-compilable.
  Measured: ties parking in Go, and needs a binary decision tree rather than a `case` in Ruby just to reach the same point.
- **Rewrite a self tail call into a loop**, which needs no trampoline at all.
  Worth doing, and done separately ([decision 90](90-self-tail-call-to-loop.md)), but it is not this: the motivating app has no self tail calls.

## Consequences

- Positive, measured on `wat/tail_call` against the same conversion before the change: Go 40.9 to 8.4 ns per hop (4.85x), Ruby with YJIT 653 to 269 ns (2.43x), Java 27.1 to 18.4 ns, Perl 3655 to 2480 ns, Python 854 to 684 ns.
  Against an ordinary call, Go's tail call goes from 9.93x to 2.05x and Ruby's from 6.42x to 2.50x.
- Positive: converted wasm3 on Ruby runs 8.36 to 6.75 us per guest iteration, starts 14% faster, and its artifact is 28% smaller, because a parked call spells out less than a constructed one.
- Carry-over: Java's entry still boxes the chain's final result, since a tail entry returns `Object`; typing the entry table per result signature would remove that and is not done here.
- Carry-over: the slots are per instance, so two instances sharing a table take the wrapped path between them. That is a correctness requirement, not a tuning choice, and the ownership check is what enforces it.
