# Decision 69: Exception Handling Joins the Accepted Input, Declared Per Backend

Status: **Accepted, 2026-08-14.**
The core IR accepts the final exception-handling proposal.
That is the tag section, `try_table` with all four catch clause kinds, `throw`, `throw_ref`, `exnref`.
The Ruby, Go, Python, Java, and Perl backends lower it, and Bash rejects it at conversion time.
mruby is the app in the app list that exercises it end to end.

## Context

mruby compiled for wasm32-wasi requires the exception-handling proposal twice over.
LLVM lowers `setjmp`/`longjmp` onto it (`-mllvm -wasm-enable-sjlj`).
mruby's own exception handling is built on `setjmp`/`longjmp`.
The same is true of the whole mruby family: PicoRuby's primary profile embeds the mruby VM.
It is also true of any C application using `setjmp`/`longjmp`.
That is a large class, which the previous input contract excluded as a whole.
A trial build confirmed the output is otherwise clean WASI p1.
It has one tag, a few `try_table`/`throw` sites, and no Emscripten glue.

[Decision 24](24-01-scope-reset.md) had removed the proposal's earlier implementation.
It went with everything else beyond wasm 1.0, under decision 24's criterion for keeping a feature.
Under it, a feature stays only if a target app in the app list needs it.
A feature every 0.1 backend is expected to implement also stays.
`AGENTS.md` stated the consequence for every proposal:
"wasm 2.0+ proposals and the component model are rejected outright, not per backend".
[Decision 19](19-ruby-exception-handling.md) was kept as the design record for a future restoration.

## Decision

mruby joins the app list as a target app.
That satisfies the first condition of decision 24's criterion for keeping a feature.
So the exception-handling proposal re-enters the accepted input.
The criterion itself is unchanged.
This decision adds one rule.
It covers a feature that an app in the app list needs but not every backend can express natively:

- The core IR accepts the feature unconditionally.
  `check_module_support` rejects it for every backend that lacks it, with the standard attributed error.
  Such a backend's `Backend::feature_status` does not declare it `Supported`.
- A backend declares `Supported` only when the shared specification harness passes for it.
  The harness runs with the feature's testsuite files enabled.
  At that moment the harness turns any remaining feature-attributed skip into a hard failure.
  The convert manifest asserts both directions per backend:
  - a declaring backend must convert the mruby module;
  - a non-declaring backend must reject it with the attributed error.
- Bash stays `Unsupported`: it has no exception mechanism.
  So the lowering would be a status code passed up through every call.
  That changes the calling convention of the whole backend.
  No app in the app list needs Bash specifically.

Only the final form of the proposal is accepted.
The legacy `try`/`catch`/`delegate` instructions stay rejected.
`LEGACY_EXCEPTIONS` stays off in `crates/dewasm-core/src/module.rs`.
The "rejected outright, not per backend" sentence, which covered every proposal, narrows.
It now covers only the proposals no app in the app list needs.
Those remain rejected for every backend.
This decision is the template for the next proposal an app in the app list drags in.

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
  It uses no `setjmp`, so exception handling could stay out.
  The mruby VM is where the family converges: PicoRuby's primary profile embeds it.
  The mruby/c variant is heading into low-maintenance mode.
  So this trades the actual target for a related variant that is losing maintenance.
- **`setjmp`/`longjmp` as Emscripten implements it (`invoke_*` trampolines).**
  It uses no exception-handling instructions.
  It produces imports outside WASI.
  It needs per-backend host trampoline glue with its own exception discipline.
  That is strictly more machinery than lowering the proposal, and nonstandard input besides.
- **Require every backend before accepting the feature.**
  That is the second condition of decision 24's criterion.
  It makes the whole mruby family wait on Bash.
  Its lowering would be a whole-backend calling-convention change nobody needs.
- **A Bash lowering that passes an exception status up through every call.**
  Rejected as its own item.
  It taxes every call site in every Bash artifact for a feature with no Bash-specific demand.
  Revisit only if an app in the app list must run under Bash specifically.

## Consequences

- Positive: mruby converts and runs on five backends.
  The door is open to the rest of the mruby family and to C applications that use `setjmp`.
  The design recorded in decision 19 is implemented again essentially unchanged.
- Negative: `docs/support.md` now shows a feature row that differs per backend.
  [Decision 25](25-retire-support-levels.md) had made that situation unrepresentable by uniformity.
  The per-feature declaration mechanism it kept is exactly what expresses it.
  So the maturity levels stay retired.
- Carry-over: functions containing a `try_table` keep the branch chain.
  They keep it even past the flattening threshold (`flat::plan` refuses them).
  No shipped app hits a measurable cost today.
- Carry-over: `wasm-opt` run before conversion ([decision 39](39-running-wasm-opt.md)) cannot parse the proposal.
  It cannot with its fixed baseline flag set.
  So the mruby build strips debug information at link time (`-Wl,--strip-debug`) and skips `wasm-opt`.
  That is the second exception after the DWARF fixture.
