# Decision 19: Ruby Exception Handling (Tags as Identity Objects, Exceptions as Native Exceptions)

Status: **Accepted, 2026-08-14.**
[Decision 69](69-exception-handling-accepted-input.md) put the design below back in force.
It was adapted to the current Ruby lowering (the `__br` cascade instead of `catch`/`throw`).
It had been superseded by [decision 24](24-01-scope-reset.md) between 2026-07-26 and 2026-08-14.
It was kept as the design record that made the restoration cheap.
The original acceptance note and implementation pointers below are retained as history.

Originally accepted 2026-07-24.
Implemented in:

- `crates/dewasm-core/src/{ir,module,func}.rs`;
- `crates/dewasm-backend/src/lib.rs`;
- `crates/dewasm-backend-ruby/src/lib.rs`;
- `runtime/ruby/units/rt/{tag,wasm_exception,throw_ref}.rb`;
- the spec harness's `assert_exception` support.
  That is `crates/dewasm-test-helper/src/spec.rs` plus `crates/dewasm-backend-ruby/tests/spec.rs`.

## Context

Wasm 3.0 exception handling (`try_table`/`throw`/`throw_ref`, tags, `exnref`) is the third roadmap phase.
The pinned testsuite contains only the final `try_table` design.
It has no legacy `try`/`catch`/`rethrow`/`delegate` anywhere.
So full `Supported` was reachable without a `Partial` carve-out.
The design questions:

- what a tag is at runtime (imported tags must match their origin across `register`ed instances);
- what an `exnref` value is;
- how catch-clause payloads fit an IR whose branches move values between statically-known temps.
  Those payloads arrive dynamically, not from stack slots.

## Decision

- **A tag is an empty identity object, `Rt::Tag`;** catch clauses compare `.equal?`.
  **Criterion: wasm tag equality is instance identity, never structure.**
  Two `(tag)` definitions are distinct, while one tag imported twice matches itself.
  That is exactly Ruby object identity.
  So sharing the object through the decision 7 provider protocol is the entire cross-instance story.
  The protocol parts are `TAG_EXPORTS`, `tag_export`, and an `import(name)` arm.
  The last part is `check_import_kind :tag` via `wasm_kind`.
  Tag *types* are not carried: the kind-not-type gap of decision 16 extends to tags (see Consequences).
- **A thrown exception is `raise Rt::WasmException.new(@tagK, [values])`.**
  **The exception object itself is the exnref value.**
  `throw_ref` re-raises it (`Rt.throw_ref`, trapping `"null exception reference"` on `nil`).
  `ValType::ExnRef` joins the flat ref variants with `nil` as null.
  `Rt::WasmException` is deliberately unrelated to `Rt::Trap`.
  `try_table`'s `rescue Rt::WasmException` structurally cannot catch traps, exhaustion, or `Rt::Exit`.
  `unreachable-not-caught`/`trap-in-callee` in `try_table.wast` pin this down.
- **`Stmt::TryTable` lowers to the body wrapped in `begin … rescue Rt::WasmException => __e`.**
  Clauses are checked in order (first match wins, per `duplicated-catches`).
  A bare `raise` ends it for the no-match case.
  A catchless `try_table` is folded to a plain block in the builder: it is one.
- **Catch payloads land directly in the target frame's slots.**
  `ir::CatchClause` carries `value_temps` plus a `target: BrTarget` with empty assigns.
  `value_temps` is computed with `branch_target`'s arithmetic (target base + index).
  It is sourced from `__e.values[i]`/`__e` instead of the stack, though.
  Catch labels resolve against the context *enclosing* the `try_table`.
  The spec validates clauses under `C`, before the block's own label is pushed.
- **`return_call` interacts correctly by construction**, and forced a decision 18 correction.
  A thunk returned out of the body leaves the `begin/rescue` before the callee runs.
  So an exception thrown by a tail-called function escapes the caller's `try_table` as required.
  Decision 18 originally allowed a "plain call for non-tail-calling callees" shortcut.
  It was observably wrong here and was removed (every `return_call` now thunks).

## Rejected alternatives

- **Tag = symbol/index keyed by module**: breaks imported-tag identity across instances.
  `catch-imported` in `try_table.wast` shows it.
  An interned structural key (the `type_symbol` trick) is wrong by design here.
  That is because tag equality is *not* structural.
- **`exnref` as a separate wrapper around (tag, values)**: the exception object already *is* that tuple.
  A second object would need converting at every catch_ref/throw_ref boundary.
- **Routing traps and exceptions through one class ladder**: generated code could too easily catch traps.
  A `rescue` of a common superclass would do it.
  Two unrelated classes make the "traps are uncatchable" property structural rather than disciplined.
- **`Partial("try_table only")`**: unnecessary.
  The pinned suite has no legacy-EH constructs (verified by grep before implementation).
  Legacy binaries fail validation, since the `LEGACY_EXCEPTIONS` validator feature stays off.
  They surface as clean `unknown-proposal` refusals.

## Consequences

- Positive: `try_table` (44), `throw` (9), `throw_ref` (12), `tag` all pass.
  `tag` is 1 + skips attributed to gc-era constructs.
  Full Ruby run pass 29,598 → 29,679.
  `assert_exception` is now a real check (`SpecLang::emit_check_exception`).
  The default keeps it an attributed skip for Bash.
- **List change (decision 8): `imports.wast` expected failures 28 → 59**, same `import-limits` tag.
  Its "test" fixture module exports tags, so it never converted before this decision.
  Now that it does, the downstream `assert_unlinkable` cases run.
  They check function signatures, global types, and tag parameter types.
  All are instances of the decision 16 kind-not-type gap, no new mechanism.
  Every other list entry (imports2 2, linking 4, linking0 1, load1 5) is byte-identical.
  Bash's run is unchanged.
- Negative / carry-over: tag parameter types join the `import-limits` debt.
  An uncaught wasm exception in `--mode standalone` surfaces as a raw Ruby backtrace.
  It has no dedicated exit path like `Rt::Trap`'s 134.
  That is acceptable until a real p2/component consumer defines better.

See also:

- [decision 16](16-ruby-wasm1-completion.md): provider protocol, kind-not-type gap;
- [decision 17](17-ruby-reference-types.md): flat ref `ValType` variants;
- [decision 18](18-ruby-tail-calls.md): the trampoline this corrected.
