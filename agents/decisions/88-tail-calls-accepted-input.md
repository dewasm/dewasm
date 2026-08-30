# Decision 88: Tail Calls Join the Accepted Input, Declared Per Backend

Status: **Accepted, 2026-08-31.**
The core IR accepts the tail-call proposal (`return_call`, `return_call_indirect`); Ruby, Python, and Perl lower it, Go, Java, and Bash reject it at conversion time, and their lowerings land as their own changes.

## Context

wasm3 is the pinned app that drags the proposal in.
Its interpreter dispatches to the next opcode by calling that opcode's function, with the call marked `M3_MUSTTAIL` (`source/m3_exec_defs.h`), which is what keeps the C stack flat across a guest program of any length.
Clang lowers a `musttail` call to `return_call`, so the stock build, and the official `wasm3-wasi.wasm` release asset, need the proposal.

`examples/apps/scripts/wasm3.sh` works around this by building the pinned source with `-DM3_HAS_TAIL_CALL=0`, which makes the macro expand to nothing: the dispatch becomes an ordinary call, and whatever LLVM does not turn back into a tail call becomes real stack growth, one frame per executed guest opcode.
That is what the glue stack workarounds on Ruby, Python, Java, and Bash exist for, and the Java one reached main as a CI failure before it was noticed.
Accepting the proposal removes the workarounds at their source and lets the pinned app be the official asset rather than a source build.

Measured on the v0.9.0 asset: with the proposal accepted, the module validates and builds into the IR, and the only thing left rejecting it is the per-backend declaration.
Nothing else about it is out of scope.

## Decision

[Decision 69](69-exception-handling-accepted-input.md) is the template, applied verbatim: the proposal satisfies decision 24's retention criterion through its first disjunct (a pinned target app needs it), so it re-enters the accepted input under the same per-backend rule.

- The core IR accepts `return_call` and `return_call_indirect` unconditionally, as `Stmt::ReturnCall` and `Stmt::ReturnCallIndirect`.
- `check_module_support` rejects them, with the standard attributed error, for every backend whose `Backend::feature_status` does not declare `Feature::TailCall` supported.
- A backend declares `Supported` only when the shared spec harness passes for it with `return_call.wast` and `return_call_indirect.wast` enabled.
  The harness turns any remaining tail-call-attributed skip into a hard failure at that moment.

The constraint that shapes every lowering, and the reason each is its own change rather than a flag flip: `return_call.wast` drives 1,000,000-deep *mutual* recursion, so no backend passes it by lowering a tail call as a call followed by a return.
[Decision 18](18-ruby-tail-calls.md) holds the design that did pass, a flat trampoline with a body/entry split; Ruby, Python, and Perl all lower it that way, since none of the three has dependable tail-call elimination.

Go, Java, and Bash stay `Unsupported` for now, for reasons that differ from Bash's standing exception-handling one and are not a judgement that they cannot:
Go and Java are statically typed, so a thunk needs a named type per result signature rather than one dynamic wrapper, and Go additionally lowers a `try_table` body as a closure whose escapes are numbered outcomes, which a thunk has to join.
Bash returns results through globals and uses the exit status as its trap channel, so the thunk has nowhere to live that a call site does not already read.

`return_call_ref` stays rejected: it belongs to the function-references proposal, which no pinned app needs.

Code this governs: `crates/dewasm-core/src/{ir,func,module}.rs` (the two statements, their operator translation, and the accepted feature set), `crates/dewasm-backend/src/lib.rs` (`stmts_use_tail_calls` and the `check_module_support` requirement), `src/extract.rs` (a tail call pins an extraction boundary exactly as a return does) and `src/licm.rs` (a tail call is a memory barrier), and `crates/xtask/src/{support_docs,feature_audit}.rs` (the per-backend row, and an app needing only accepted-per-backend proposals stays in scope).

## Rejected alternatives

- **Keep building wasm3 with `-DM3_HAS_TAIL_CALL=0`.**
  It is the status quo, and it is what makes the converted interpreter consume host stack proportional to the guest program's opcode count rather than its call depth.
  Every backend then needs its own workaround for stack that the guest never actually asked for, each tuned by hand and each a silent CI failure away from breaking.
- **Rewrite tail calls into ordinary calls in the core IR, so no backend has to change.**
  Correct results, wrong space: the mutual recursion in the conformance suite overflows every backend's host stack, and the whole point of the proposal is the space guarantee.
- **A self-tail-call to loop rewrite in the core IR as the mechanism.**
  It covers direct self-recursion only, and the suite's `even`/`odd` pair is mutual.
  Still viable later as an optimization layered on a real lowering, where it just shrinks the tail-caller set.
- **Hold the proposal until every backend can lower it.**
  This is decision 24's second disjunct, and decision 69 already rejected it for the same reason: it makes the pinned app hostage to whichever backend is hardest, with no benefit to the backends that are ready.

## Consequences

- Positive: the official `wasm3-wasi.wasm` asset converts and runs on the three declaring backends, with no stack workaround at all: converted to Ruby it runs the cowsay guest in under a second on a stock `ruby`, where the `-DM3_HAS_TAIL_CALL=0` build needs `RUBY_THREAD_VM_STACK_SIZE` raised to get that far.
- Positive: `docs/support.md` gains a tail-call row, the second feature row that differs per backend.
- Carry-over: the source build and the glue workarounds stay until Go, Java, and Bash lower the proposal, because the app case runs on all six; the pinned app cannot move to the official asset before then.
- Carry-over: a tail-calling function allocates one thunk per hop and pays a wrapper frame even when called normally, the cost decision 18 already recorded.
