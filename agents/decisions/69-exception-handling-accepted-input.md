# Decision 69: Exception Handling Joins the Accepted Input, Declared Per Backend

Status: **Accepted, 2026-08-14.**
The core IR accepts the final exception-handling proposal.
That is the tag section, `try_table` with all four catch clause kinds, `throw`, `throw_ref`, `exnref`.
The Ruby, Go, Python, Java, and Perl backends lower it, and Bash rejects it at conversion time.
mruby is the pinned app exercising it end to end.

## Context

mruby compiled for wasm32-wasi requires the exception-handling proposal twice over.
LLVM lowers setjmp/longjmp onto it (`-mllvm -wasm-enable-sjlj`).
mruby's own exception handling is built on setjmp/longjmp.
The same is true of the whole mruby family: picoruby's primary profile embeds the mruby VM.
It is also true of any C application using setjmp/longjmp.
That is a large class, which the previous input contract excluded wholesale.
A build spike confirmed the output is otherwise clean WASI p1.
It has one tag, a handful of `try_table`/`throw` sites, and no emscripten glue.

[Decision 24](24-01-scope-reset.md) had removed the proposal's earlier implementation.
It went together with everything else beyond wasm 1.0, under decision 24's retention criterion.
Under it, a feature stays only if a pinned target app needs it.
A feature every 0.1 backend is expected to implement also stays.
`AGENTS.md` stated the blanket consequence:
"wasm 2.0+ proposals and the component model are rejected outright, not per backend".
[Decision 19](19-ruby-exception-handling.md) was kept as the design record for a future restoration.

## Decision

mruby becomes a pinned target app.
That satisfies the first disjunct of decision 24's retention criterion.
So the exception-handling proposal re-enters the accepted input.
The criterion itself is unchanged.
This decision adds one rule.
It covers a feature that a pinned app needs but not every backend can express natively:

- The core IR accepts the feature unconditionally.
  `check_module_support` rejects it for every backend that lacks it, with the standard attributed error.
  Such a backend's `Backend::feature_status` does not declare it `Supported`.
- A backend declares `Supported` only when the shared spec harness passes for it.
  The harness runs with the feature's testsuite files enabled.
  At that moment the harness turns any remaining feature-attributed skip into a hard failure.
  The convert manifest asserts both directions per backend:
  - a declaring backend must convert the mruby module;
  - a non-declaring backend must reject it with the attributed error.
- Bash stays `Unsupported`: it has no exception mechanism.
  So the lowering would be a status-code propagation threaded through every call.
  That is a calling-convention change to the whole backend, and no pinned app needs Bash specifically.

Only the final form of the proposal is accepted.
The legacy `try`/`catch`/`delegate` instructions stay rejected.
`LEGACY_EXCEPTIONS` stays off in `crates/dewasm-core/src/module.rs`.
The blanket "rejected outright, not per backend" sentence narrows.
It now covers only the proposals no pinned app needs.
Those remain rejected for all backends alike.
This decision is the template for the next proposal a pinned app drags in.

Code this governs:

- `crates/dewasm-core/src/{ir,func,module}.rs` (IR and parsing);
- `crates/dewasm-backend/src/lib.rs` (`check_module_support`);
- `src/flat.rs` in the same crate: functions containing a `try_table` are never flattened.
  A handler must stay lexically inside its frame.
- the five backend lowerings with their `runtime/<lang>/units/rt/` exception units;
- `crates/dewasm-test-helper/src/apps_convert.rs` (the per-entry required feature);
- `crates/xtask/src/feature_audit.rs` (exception handling never defers an app by itself).

## Rejected alternatives

- **Route the mruby family through the mruby/c VM (FemtoRuby) instead.**
  It uses no setjmp, so exception handling could stay out.
  The mruby VM is where the family converges: picoruby's primary profile embeds it.
  The mruby/c variant is heading into low-maintenance mode.
  So this trades the actual target for its shrinking sibling.
- **Emscripten-style setjmp/longjmp emulation (`invoke_*` trampolines).**
  It uses no exception-handling instructions.
  It produces non-WASI imports.
  It needs per-backend host trampoline glue with its own exception discipline.
  That is strictly more machinery than lowering the proposal, and nonstandard input besides.
- **Require every backend before accepting the feature.**
  That is the second disjunct of decision 24's criterion.
  It holds the whole mruby family hostage to Bash.
  Its lowering would be a whole-backend calling-convention change nobody needs.
- **A Bash lowering by exception-status propagation.**
  Rejected as its own item.
  It taxes every call site in every Bash artifact for a feature with no Bash-specific demand.
  Revisit only if a pinned app must run under Bash specifically.

## Consequences

- Positive: mruby converts and runs on five backends.
  The door is open to the rest of the mruby family and to setjmp-using C applications generally.
  The design recorded in decision 19 is implemented again essentially unchanged.
- Negative: `docs/support.md` now shows a feature row that differs per backend.
  [Decision 25](25-retire-support-levels.md) had made that situation unrepresentable by uniformity.
  The per-feature declaration mechanism it kept is exactly what expresses it.
  So the maturity levels stay retired.
- Carry-over: functions containing a `try_table` keep the branch cascade.
  They keep it even past the flattening threshold (`flat::plan` refuses them).
  No shipped app hits a measurable cost today.
- Carry-over: `wasm-opt` preprocessing ([decision 39](39-wasm-opt-preprocessing.md)) cannot parse the proposal.
  It cannot with its pinned baseline flag set.
  So the mruby build strips debug info at link time (`-Wl,--strip-debug`) and skips `wasm-opt`.
  That is the second exception after the DWARF fixture.
