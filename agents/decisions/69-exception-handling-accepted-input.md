# Decision 69: Exception Handling Joins the Accepted Input, Declared Per Backend

Status: **Accepted, 2026-08-14.**
The core IR accepts the final exception-handling proposal (tag section, `try_table` with all four catch clause kinds, `throw`, `throw_ref`, `exnref`), the Ruby, Go, Python, Java, and Perl backends lower it, Bash rejects it at conversion time, and mruby is the pinned app exercising it end to end.

## Context

mruby compiled for wasm32-wasi requires the exception-handling proposal twice over: LLVM lowers setjmp/longjmp onto it (`-mllvm -wasm-enable-sjlj`), and mruby's own exception handling is built on setjmp/longjmp.
The same is true of the whole mruby family (picoruby's primary profile embeds the mruby VM) and of any C application using setjmp/longjmp, a large class that the previous input contract excluded wholesale.
A build spike confirmed the output is otherwise clean WASI p1: one tag, a handful of `try_table`/`throw` sites, no emscripten glue.

[Decision 24](24-01-scope-reset.md) had removed the proposal's earlier implementation together with everything else beyond wasm 1.0, under the retention criterion "a feature stays only if a pinned target app needs it or every 0.1 backend is expected to implement it", and `AGENTS.md` stated the blanket consequence "wasm 2.0+ proposals and the component model are rejected outright, not per backend".
[Decision 19](19-ruby-exception-handling.md) was kept as the design record for a future restoration.

## Decision

mruby becomes a pinned target app, which satisfies the first disjunct of decision 24's retention criterion, so the exception-handling proposal re-enters the accepted input.
The criterion itself is unchanged; what this decision adds is the rule for a feature a pinned app needs but not every backend can express natively:

- The core IR accepts the feature unconditionally; `check_module_support` rejects it, with the standard attributed error, for every backend whose `Backend::feature_status` does not declare it `Supported`.
- A backend declares `Supported` only when the shared spec harness passes for it with the feature's testsuite files enabled; the harness turns any remaining feature-attributed skip into a hard failure at that moment, and the convert manifest asserts both directions per backend (a declaring backend must convert the mruby module, a non-declaring backend must reject it with the attributed error).
- Bash stays `Unsupported`: it has no exception mechanism, so the lowering would be a status-code propagation threaded through every call, a calling-convention change to the whole backend, and no pinned app needs Bash specifically.

Only the final form of the proposal is accepted; the legacy `try`/`catch`/`delegate` instructions stay rejected (`LEGACY_EXCEPTIONS` stays off in `crates/dewasm-core/src/module.rs`).
The blanket "rejected outright, not per backend" sentence narrows to the proposals no pinned app needs: those remain rejected for all backends alike, and this decision is the template for the next proposal a pinned app drags in.

Code this governs: `crates/dewasm-core/src/{ir,func,module}.rs` (IR and parsing), `crates/dewasm-backend/src/lib.rs` (`check_module_support`) and `src/flat.rs` (functions containing a `try_table` are never flattened: a handler must stay lexically inside its frame), the five backend lowerings with their `runtime/<lang>/units/rt/` exception units, `crates/dewasm-test-helper/src/apps_convert.rs` (the per-entry required feature), and `crates/xtask/src/feature_audit.rs` (exception handling never defers an app by itself).

## Rejected alternatives

- **Keep exception handling out and route the mruby family through the mruby/c VM (FemtoRuby), which uses no setjmp.**
  The mruby VM is where the family converges (picoruby's primary profile embeds it; the mruby/c variant is heading into low-maintenance mode), so this trades the actual target for its shrinking sibling.
- **Emscripten-style setjmp/longjmp emulation without exception-handling instructions (`invoke_*` trampolines).**
  Produces non-WASI imports and needs per-backend host trampoline glue with its own exception discipline; strictly more machinery than lowering the proposal, and nonstandard input besides.
- **Require every backend before accepting the feature (the second disjunct of decision 24's criterion).**
  Holds the whole mruby family hostage to Bash, whose lowering would be a whole-backend calling-convention change nobody needs.
- **A Bash lowering by exception-status propagation.**
  Rejected as its own item: it taxes every call site in every Bash artifact for a feature with no Bash-specific demand; revisit only if a pinned app must run under Bash specifically.

## Consequences

- Positive: mruby converts and runs on five backends, and the door is open to the rest of the mruby family and to setjmp-using C applications generally; the design recorded in decision 19 is implemented again essentially unchanged.
- Negative: `docs/support.md` now shows a feature row that differs per backend, the situation [decision 25](25-retire-support-levels.md) had made unrepresentable by uniformity; the per-feature declaration mechanism it kept is exactly what expresses it, so the maturity levels stay retired.
- Carry-over: functions containing a `try_table` keep the branch cascade even past the flattening threshold (`flat::plan` refuses them); no shipped app hits a measurable cost today.
- Carry-over: `wasm-opt` preprocessing ([decision 39](39-wasm-opt-preprocessing.md)) cannot parse the proposal with its pinned baseline flag set, so the mruby build strips debug info at link time (`-Wl,--strip-debug`) and skips `wasm-opt`, the second exception after the DWARF fixture.
