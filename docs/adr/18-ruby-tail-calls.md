# ADR-18 — Tail Calls in the Ruby Backend: Flat Trampoline with a Body/Entry Split

Status: **Superseded by [ADR-24](24-01-scope-reset.md), 2026-07-26.** Kept as a design record for a future restoration of this support; git history plus this ADR make the work cheap to revive. The original acceptance note and implementation pointers below are retained as history.

Originally accepted 2026-07-24. Implemented: `crates/dewasmify-core/src/{ir,func,module}.rs`,
`crates/dewasmify-backend/src/lib.rs` (`stmts_use_tail_calls`),
`crates/dewasmify-backend-ruby/src/lib.rs`, `runtime/ruby/units/rt/tail_call.rb`,
`runtime/ruby/units/table/tail_ref.rb`.

## Context

`return_call`/`return_call_indirect` require tail-call chains to run in constant stack space:
`return_call.wast` drives 1,000,000-deep *mutual* recursion (`even`/`odd`), far past MRI's
~10⁴-frame limit. Ruby has no dependable tail-call elimination, so the lowering itself must
keep chains flat.

## Decision

A trampoline, shaped so that no per-hop stack frame survives:

- Every defined function that **contains** a tail call (computed by the shared, exhaustive
  `stmts_use_tail_calls` walk) is split: `_fN_body` holds the real code, and the public `_fN`
  is `Rt.trampoline(_fN_body(...))`. `Rt::TailCall` is a `Struct(:target, :args)`;
  `Rt.trampoline` re-dispatches while the result is one
  (`runtime/ruby/units/rt/tail_call.rb`). All existing call sites (exports, `Stmt::Call`,
  tables, `start`) keep using `_fN`, so the split is invisible outside.
- **Every `return_call` produces a thunk — never a plain call.** To a tail-calling callee the
  thunk targets the *body* (`method(:_fM_body)`), so mutual chains bounce in the one outermost
  trampoline with zero intermediate frames; to anything else (imports, plain functions) it
  targets the ordinary callable, costing one completing frame per such hop. **Criterion: the
  callee must run after the caller's frame — including its `rescue` blocks — is gone.** A
  "plain call for non-tail-calling callees" shortcut passed every stack-depth test but was
  observably wrong under exception handling: `try_table.wast`'s `return-call-in-try-catch`
  requires an exception thrown by the tail-called function to *escape* the caller's
  `try_table`, and a direct call keeps that `rescue` wrapped around the callee (ADR-19). The
  thunk is unwrapped outside the body's `begin/rescue`, giving the frame-replacement semantics
  for free.
- `return_call_indirect` resolves through the table at the instruction's execution point:
  `Rt::TailCall.new(@tK.tail_ref(i, type_sym), [...])`. ADR-17's slot pair carries an optional
  **third element** — the body method — added by `Gen::func_pair` for tail-calling functions;
  `tail_ref` performs `call`'s exact trap sequence (undefined element, uninitialized, type
  mismatch — raised inside the caller's body, i.e. at the right point in execution order) and
  returns `slot[2] || slot[1]`. A pair from a non-tail-caller, or from another module
  instance, falls back to its public entry: that entry runs its *own* trampoline to
  completion, costing one frame per module switch, which the criterion permits.

## Rejected alternatives

- **Plain call + `return`** — correct results, wrong space: dies by stack overflow around 10⁴
  frames against the suite's 10⁶ chains.
- **Self-tail-call → loop rewrite in the IR** — only covers direct self-recursion; `even`/`odd`
  mutual recursion still overflows. Viable later as a readability/speed optimization layered
  on top (it would simply shrink the tail-caller set), never as the mechanism.
- **Thunks holding the public `_fM` entry (no body split)** — each hop enters a fresh
  trampoline one frame deeper; 10⁶ hops = 10⁶ frames. The body split is precisely what makes
  the chain flat.
- **`RubyVM::InstructionSequence.compile_option = {tailcall_optimization: true}`** — MRI-only,
  requires routing generated code through explicit iseq compilation, and silently absent on
  JRuby/TruffleRuby; generated code must not depend on an interpreter flag for correctness.

## Consequences

- Positive: `return_call.wast` (33) + `return_call_indirect.wast` (82 total with the former)
  pass in ~2 s including the 10⁶-deep chains; full Ruby sweep pass 29,516 → 29,598, fail
  stays 40; Bash unchanged (gated by `check_module_support`, `tail-call` skips re-appear in
  its histogram unchanged).
- Positive: `stmts_use_tail_calls` lives in `dewasmify-backend` because gating needs it anyway
  — a future backend implementing tail calls (C#'s real `tail.` prefix, Java trampolines)
  reuses the same tail-caller analysis.
- Negative / carry-over: tail-calling functions allocate one `Rt::TailCall` per hop, and every
  tail-caller pays the extra wrapper frame even when called normally. A host externref that is
  itself an `Rt::TailCall` instance would confuse a trampoline only if a wasm function could
  *return* it from a body — it cannot (bodies only produce thunks at `return_call` sites), so
  this is theoretical. `return_call_ref` stays rejected under `function-references`.

See also: [ADR-17](17-ruby-reference-types.md) (the table slot format the third element
extends), [ADR-4](4-ruby-backend-lowering.md) (lowering conventions),
[ADR-16](16-ruby-wasm1-completion.md) (`check_module_support`).
