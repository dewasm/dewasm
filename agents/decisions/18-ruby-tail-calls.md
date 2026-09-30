# Decision 18: Tail Calls in the Ruby Backend (Flat Trampoline with a Body/Entry Split)

Status: **Accepted, 2026-08-31.**
[Decision 88](88-tail-calls-accepted-input.md) put the design below back in force.
Python and Perl adopt it unchanged.
All three lack dependable tail-call elimination.
So the shape that keeps chains flat is the same in each.
Go, Java, and Bash follow the same body/entry split, described in decision 88.
The thunk itself is gone everywhere.
What a body leaves behind is parked in per-instance slots rather than allocated.
That is the shape Bash had from the start, and [decision 89](89-park-the-pending-tail-call.md) brought the rest to it.
Perl differs only in the two places its language forces:

- a parked call leaves through the `try_table` outcome table rather than a bare `return`.
  That is because perl's `return` inside an `eval` exits only the `eval`.
- the trampoline is list-aware.
  That is because perl flattens a multi-value return into its caller's argument list.

It had been superseded by [decision 24](24-01-scope-reset.md) between 2026-07-26 and 2026-08-31.
It was kept as the design record that made the restoration cheap.
The original acceptance note and implementation pointers below are retained as history.
"ADR-18" in them is this decision.
The runtime units now live inside their backend crates ([decision 85](85-crates-io-publish-layout.md)).

Originally accepted 2026-07-24.
Implemented in:

- `crates/dewasm-core/src/{ir,func,module}.rs`;
- `crates/dewasm-backend/src/lib.rs` (`stmts_use_tail_calls`);
- every backend in `crates/dewasm-backend-*/src/lib.rs`;
- each of their `units/rt/tail_call.*` and `units/table/tail_ref.*`.

Bash needs neither unit.
Its thunk is two globals, and its table already stores names.

## Context

`return_call`/`return_call_indirect` require tail-call chains to run in constant stack space.
`return_call.wast` drives 1,000,000-deep *mutual* recursion (`even`/`odd`), far past MRI's ~10⁴-frame limit.
Ruby has no dependable tail-call elimination, so the lowering itself must keep chains flat.

## Decision

A trampoline, shaped so that no per-hop stack frame survives:

- Every defined function that **contains** a tail call is split.
  The shared, exhaustive `stmts_use_tail_calls` walk computes that set.
  `_fN_body` holds the real code, and the public `_fN` is `Rt.trampoline(_fN_body(...))`.
  `Rt::TailCall` is a `Struct(:target, :args)`; `Rt.trampoline` re-dispatches while the result is one.
  That is in `runtime/ruby/units/rt/tail_call.rb`.
  All existing call sites (exports, `Stmt::Call`, tables, `start`) keep using `_fN`.
  So the split is invisible outside.
- **Every `return_call` produces a thunk, never a plain call.**
  To a tail-calling callee the thunk targets the *body* (`method(:_fM_body)`).
  So mutual chains bounce in the one outermost trampoline with zero intermediate frames.
  To anything else (imports, plain functions) it targets the ordinary callable.
  That costs one completing frame per such hop.
  **Criterion: the callee must run after the caller's frame (including its `rescue` blocks) is gone.**
  A "plain call for non-tail-calling callees" shortcut passed every stack-depth test.
  It was observably wrong under exception handling, though.
  `try_table.wast`'s `return-call-in-try-catch` shows it.
  An exception thrown by the tail-called function must *escape* the caller's `try_table`.
  A direct call keeps that `rescue` wrapped around the callee (decision 19).
  The thunk is unwrapped outside the body's `begin/rescue`.
  That gives the frame-replacement semantics for free.
- `return_call_indirect` resolves through the table at the instruction's execution point.
  It emits `Rt::TailCall.new(@tK.tail_ref(i, type_sym), [...])`.
  Decision 17's slot pair carries an optional **third element**, the body method.
  `Gen::func_pair` adds it for tail-calling functions.
  `tail_ref` performs `call`'s exact trap sequence: undefined element, uninitialized, type mismatch.
  It raises inside the caller's body, i.e. at the right point in execution order.
  It returns `slot[2] || slot[1]`.
  A pair from a non-tail-caller, or from another module instance, falls back to its public entry.
  That entry runs its *own* trampoline to completion.
  It costs one frame per module switch, which the criterion permits.

## Rejected alternatives

- **Plain call + `return`**: correct results, wrong space.
  It dies by stack overflow around 10⁴ frames against the suite's 10⁶ chains.
- **Self-tail-call → loop rewrite in the IR**: only covers direct self-recursion.
  `even`/`odd` mutual recursion still overflows.
  It is viable later as a readability/speed optimization layered on top, never as the mechanism.
  It would simply shrink the tail-caller set.
- **Thunks holding the public `_fM` entry (no body split).**
  Each hop enters a fresh trampoline one frame deeper.
  10⁶ hops = 10⁶ frames.
  The body split is precisely what makes the chain flat.
- **`RubyVM::InstructionSequence.compile_option = {tailcall_optimization: true}`**: MRI-only.
  It requires routing generated code through explicit iseq compilation.
  It is silently absent on JRuby/TruffleRuby.
  Generated code must not depend on an interpreter flag for correctness.

## Consequences

- Positive: `return_call.wast` (33) + `return_call_indirect.wast` (82 total with the former) pass in ~2 s.
  That includes the 10⁶-deep chains.
  Full Ruby run pass 29,516 → 29,598, fail stays 40.
  Bash is unchanged, since `check_module_support` controls it.
  Its `tail-call` skips re-appear unchanged in its histogram.
- Positive: `stmts_use_tail_calls` lives in `dewasm-backend` because conditioning needs it anyway.
  A future backend implementing tail calls reuses the same tail-caller analysis.
  Examples are C#'s real `tail.` prefix and Java trampolines.
- Negative / carry-over: tail-calling functions allocate one `Rt::TailCall` per hop.
  Every tail-caller pays the extra wrapper frame even when called normally.
  A host externref that is itself an `Rt::TailCall` instance could confuse a trampoline.
  That happens only if a wasm function could *return* it from a body.
  It cannot (bodies only produce thunks at `return_call` sites), so this is theoretical.
  `return_call_ref` stays rejected under `function-references`.

See also:

- [decision 17](17-ruby-reference-types.md): the table slot format the third element extends;
- [decision 4](4-ruby-backend-lowering.md): lowering conventions;
- [decision 16](16-ruby-wasm1-completion.md): `check_module_support`.
