# Decision 88: Tail Calls Join the Accepted Input, Declared Per Backend

Status: **Accepted, 2026-08-31.**
The core IR accepts the tail-call proposal (`return_call`, `return_call_indirect`).
Every backend lowers it.
Bash was excluded for a day on the reasoning below, which was wrong.
The correction is recorded in place.

## Context

wasm3 is the app in the app list that drags the proposal in.
Its interpreter dispatches to the next opcode by calling that opcode's function.
The call is marked `M3_MUSTTAIL` (`source/m3_exec_defs.h`).
That is what keeps the C stack flat across a guest program of any length.
Clang lowers a `musttail` call to `return_call`.
So the stock build, and the official `wasm3-wasi.wasm` release asset, need the proposal.

`examples/apps/scripts/wasm3.sh` works around this.
It builds the source at that version with `-DM3_HAS_TAIL_CALL=0`.
That makes the macro expand to nothing: the dispatch becomes an ordinary call.
Whatever LLVM does not turn back into a tail call becomes real stack growth.
The growth is one frame per executed guest opcode.
That growth is what the glue works around on Ruby, Python, Java, and Bash.
The Java stack change reached main as a CI failure before it was noticed.
Accepting the proposal removes the cause of those changes.
It also lets the app's fixed version be the official asset rather than a source build.

This was measured on the v0.9.0 asset.
With the proposal accepted, the module validates and builds into the IR.
The only thing left rejecting it is the per-backend declaration.
Nothing else about it is out of scope.
So accepting the proposal also lets the fixed version be upstream's own asset, not a local build.

## Decision

[Decision 69](69-exception-handling-accepted-input.md) is the template, applied unchanged.
Decision 24 keeps a feature when one of two conditions holds, and the proposal satisfies the first.
That condition is that a target app in the app list needs it.
So it re-enters the accepted input under the same per-backend rule.

- The core IR accepts `return_call` and `return_call_indirect` unconditionally.
  They become `Stmt::ReturnCall` and `Stmt::ReturnCallIndirect`.
- `check_module_support` rejects them, with the standard attributed error, for some backends.
  Those are the backends whose `Backend::feature_status` does not declare `Feature::TailCall` supported.
- A backend declares `Supported` only when the shared specification harness passes for it.
  The harness must pass with `return_call.wast` and `return_call_indirect.wast` enabled.
  The harness turns any remaining tail-call-attributed skip into a hard failure at that moment.

One constraint shapes every lowering.
It is also the reason each lowering is its own change rather than a flag flip.
`return_call.wast` drives 1,000,000-deep *mutual* recursion.
So no backend passes it by lowering a tail call as a call followed by a return.
[Decision 18](18-ruby-tail-calls.md) holds the design that did pass: a flat trampoline with a body/entry split.
Ruby, Python, and Perl all lower it that way, since none has dependable tail-call elimination.

The two statically-typed backends need the thunk typed rather than boxed.
Each takes the shape its own lowering already has:

- Go gives each result signature its own thunk type, `type XTailI32 func() (int32, XTailI32)`.
  So nothing is boxed.
  A body returns its results *and* the thunk, and the entry loops while the thunk is not `nil`.
  A `try_table` body is a closure whose exits are numbered outcomes.
  So a tail call inside one becomes another outcome.
  It is re-emitted outside the closure, where the handler is already gone.
- Java needs no new type: its result register is already `Object` for multi-value returns.
  So a tail-calling body types it `Object`, and a tail call is a return of the thunk.
  The same `_br` register as any other return unwinds it.

Bash was first declared `Unsupported`.
The reasoning started from two facts.
Results come back through global variables, and the exit status is the trap channel.
So a thunk had nowhere to live that a call site did not already read.
That was wrong, and it is corrected here rather than quietly dropped.
The thunk lives exactly where results already live.
No call site reads it, because the entry consumes it before returning.
Bash parks the target and its arguments in `<p>tlfn`/`<p>tlargs`.
The entry's trampoline runs the chain.
So the entry keeps the name, arity, `R<i>` results, and exit status its callers already use.
Nothing outside a tail-calling function changes.

Two things make it easier there than in the backends that got the feature first:
- The table stores function *names*, so `return_call_indirect` needs no closure.
  It needs only the same checks the existing `call_indirect` inlines.
  It also needs one more parallel array for the tail command.
- Bash has no exception handling.
  So the `try_table` interaction that shaped the Perl and Go lowerings does not exist.

It was measured against a plain-call lowering of the same recursion.
That lowering makes the shell fail with a segmentation fault at ten thousand deep.
The trampoline runs the conformance suite's million-deep chains.

The cost is one extra Bash function call whenever a tail-calling function is entered normally.
In this backend, such a call is the dominant unit.
That is paid only by tail-callers, unlike the passing of exception status that decision 69 rejected.
That passing would have taxed every call site in every artifact.
The two are not the same shape of change.
Treating them as one is what produced the wrong call.

`return_call_ref` stays rejected.
It belongs to the function-references proposal, which no app in the app list needs.

Code this governs:
- `crates/dewasm-core/src/{ir,func,module}.rs`.
  It holds the two statements, their operator translation, and the accepted feature set.
- `crates/dewasm-backend/src/lib.rs`: `stmts_use_tail_calls` and the `check_module_support` requirement.
- `src/extract.rs`: a tail call sets an extraction boundary exactly as a return does.
- `src/licm.rs`: a tail call is a memory barrier.
- `crates/xtask/src/{support_docs,feature_audit}.rs`: the per-backend row.
  An app needing only accepted-per-backend proposals stays in scope.

## Rejected alternatives

- **Keep building wasm3 with `-DM3_HAS_TAIL_CALL=0`.**
  It is the state before this decision.
  It makes the converted interpreter's host stack proportional to the guest program's opcode count.
  The stack is then not proportional to the program's call depth.
  Every backend then needs its own way to work around stack that the guest never actually asked for.
  Each such stack change is tuned by hand, and each is a silent CI failure away from breaking.
- **Rewrite tail calls into ordinary calls in the core IR, so no backend has to change.**
  Correct results, wrong space.
  The mutual recursion in the conformance suite overflows every backend's host stack.
  The whole point of the proposal is the space guarantee.
- **A self-tail-call to loop rewrite in the core IR as the mechanism.**
  It covers direct self-recursion only, and the suite's `even`/`odd` pair is mutual.
  It is still viable later as an optimization layered on a real lowering.
  There it just reduces the tail-caller set.
- **Hold the proposal until every backend can lower it.**
  This is decision 24's second condition, and decision 69 already rejected it for the same reason.
  It makes the app wait on whichever backend is hardest.
  It brings no benefit to the backends that are ready.

## Consequences

- Positive: the official `wasm3-wasi.wasm` asset converts and runs the `cowsay` guest.
  It needs no stack change at all.
  It runs in under a second on a stock `ruby`.
  There the `-DM3_HAS_TAIL_CALL=0` build needs `RUBY_THREAD_VM_STACK_SIZE` raised.
  It also runs on a default JVM stack.
  There the same build needed the glue's own 64 MB thread.
- Positive: `docs/support.md` gains a tail-call row, the second feature row that differs per backend.
- Positive: the fixed version of wasm3 is upstream's own release asset.
  It is no longer a local build with the dispatch turned off.
  Every stack change this app needed is gone: the glue is plain on all five backends that run it.
- Positive: every backend lowers the proposal.
  So `docs/support.md`'s tail-call row reads supported throughout, and no app case is lost to it.
- Carry-over: a tail-calling function allocates one thunk per step.
  It also pays a wrapper frame even when called normally, the cost decision 18 already recorded.
  In converted wasm3 that step is the guest opcode dispatch.
  So the allocation sits in the hottest loop there is.
  Whether it costs more than the stack growth it replaces is a measurement for the benchmark suite.
  The suite should make it once the app moves.
- Carry-over: converting the official asset to Go exposed an unrelated emission bug (issue #289).
  A constant `i32.mul` renders as a Go constant expression.
  Go rejects that expression for overflow instead of wrapping.
  It was fixed separately.
  The asset is what found it, since the specification testsuite passes its operands in at each `invoke`.
  So the testsuite never produces the shape.
