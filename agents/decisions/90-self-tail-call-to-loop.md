# Decision 90: Rewrite a Self Tail Call into a Loop

Status: **Accepted, 2026-08-31.**
A shared pass turns a `return_call` to the function's own index into a branch back to a loop.
The loop wraps the function's body.
Ruby adopts the pass.
The other backends adopt it the way they adopt the other shared passes.

## Context

[Decision 18](18-ruby-tail-calls.md) rejected this as the *mechanism*.
It covers direct self-recursion only, and the conformance suite's `even`/`odd` pair is mutual.
Decision 18 then recorded it as viable later, in these words:
"as a readability/speed optimization layered on top (it would simply shrink the tail-caller set)".

[Decision 89](89-park-the-pending-tail-call.md) took the per-step cost down to a parked write and a call through a table.
What remains is the call itself.
For a self tail call there is nothing to call: the frame it would replace is the frame it is in.

## Decision

A self tail call becomes an assignment of the arguments to the parameters.
It is followed by a branch to a loop that wraps the whole body.
A function whose tail calls are all self-calls then contains none, so it leaves the tail-caller set.
With that it loses the body/entry split, the parked slots and the trampoline.

Three details the rewrite has to get right, and one it declines:

- The arguments land in fresh temps before any parameter is written.
  That is because an argument may read a parameter that an earlier assignment would write over.
- The declared locals are reset to their zero, because a fresh call zeroes them.
  The loop has to do the same.
- The loop's label is one past the largest the body already uses.
  So the new frame cannot share a label with an existing one.
- A function whose body can fall off its end is left alone.
  That is because it would spin in the loop rather than return.
  A function declaring a reference-typed local is also left alone.
  Such a local has no constant zero in the IR to reset it to.

The pass runs before the others.
So its loop is a candidate for load hoisting and body extraction, like any loop the guest wrote.

Code this governs: `crates/dewasm-backend/src/selfcall.rs`, and each backend's decision to run it.

## Rejected alternatives

- **Do it in the core IR rather than as a backend pass.**
  The core builder does not depend on the backend, and this is an optimization.
  So it belongs with the other optimizing passes.
  A backend that has some better way to lower a self tail call is then free not to run it.
- **Extend it to a cycle of two or three functions by inlining them into one loop.**
  That merges several functions into one dispatch loop on a small scale, and it was measured.
  It is worth having only under a size threshold.
  It is worth its own decision if it is ever taken.
  Issue 295 holds the measurements.

## Consequences

- Positive: a self-recursive `count` at 3,000,000 deep was converted to Ruby and run under YJIT.
  It took 0.329 s through the trampoline against 0.130 s as a loop, 2.53x.
- Positive: the rewritten function needs no entry, no parked slots and no table entry.
  So a guest whose recursion is all self-recursion pays nothing at all for the proposal.
- Carry-over: it does nothing for the app the proposal was accepted for.
  Converted wasm3 has 523 indirect tail calls, 5 direct, and no self tail calls.
  The pass is for the guests that are shaped the other way.
