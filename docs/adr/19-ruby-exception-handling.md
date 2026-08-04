# ADR-19 — Exception Handling in the Ruby Backend: Tags as Identity Objects, Exceptions as Native Exceptions

Status: **Superseded by [ADR-24](24-01-scope-reset.md), 2026-07-26.** Kept as a design record for a future restoration of this support; git history plus this ADR make the work cheap to revive. The original acceptance note and implementation pointers below are retained as history.

Originally accepted 2026-07-24. Implemented: `crates/dewasm-core/src/{ir,module,func}.rs`, `crates/dewasm-backend/src/lib.rs`, `crates/dewasm-backend-ruby/src/lib.rs`, `runtime/ruby/units/rt/{tag,wasm_exception,throw_ref}.rb`, and the spec harness's `assert_exception` support (`crates/dewasm-test-helper/src/spec.rs` plus `crates/dewasm-backend-ruby/tests/spec.rs`).

## Context

Wasm 3.0 exception handling (`try_table`/`throw`/`throw_ref`, tags, `exnref`) is the third roadmap phase. The pinned testsuite contains only the final `try_table` design — no legacy `try`/`catch`/`rethrow`/`delegate` anywhere — so full `Supported` was reachable without a `Partial` carve-out. The design questions: what a tag is at runtime (imported tags must match their origin across `register`ed instances), what an `exnref` value is, and how catch-clause payloads — which arrive dynamically, not from stack slots — fit an IR whose branches move values between statically-known temps.

## Decision

- **A tag is an empty identity object, `Rt::Tag`;** catch clauses compare `.equal?`. **Criterion: wasm tag equality is instance identity, never structure** — two `(tag)` definitions are distinct, while one tag imported twice matches itself — which is exactly Ruby object identity, so sharing the object through the ADR-7 provider protocol (`TAG_EXPORTS`, `tag_export`, an `import(name)` arm, `check_import_kind :tag` via `wasm_kind`) is the entire cross-instance story. Tag *types* are not carried: the kind-not-type gap of ADR-16 extends to tags (see Consequences).
- **A thrown exception is `raise Rt::WasmException.new(@tagK, [values])`, and the exception object itself is the exnref value.** `throw_ref` re-raises it (`Rt.throw_ref`, trapping `"null exception reference"` on `nil`); `ValType::ExnRef` joins the flat ref variants with `nil` as null. `Rt::WasmException` is deliberately unrelated to `Rt::Trap`: `try_table`'s `rescue Rt::WasmException` structurally cannot catch traps, exhaustion, or `Rt::Exit` (`unreachable-not-caught`/`trap-in-callee` in `try_table.wast` pin this down).
- **`Stmt::TryTable` lowers to the body wrapped in `begin … rescue Rt::WasmException => __e`**, clauses checked in order (first match wins, per `duplicated-catches`), ending in a bare `raise` for the no-match case. A catchless `try_table` is folded to a plain block in the builder — it is one.
- **Catch payloads land directly in the target frame's slots.** `ir::CatchClause` carries `value_temps` (computed with `branch_target`'s arithmetic — target base + index — but sourced from `__e.values[i]`/`__e` instead of the stack) plus a `target: BrTarget` with empty assigns. Catch labels resolve against the context *enclosing* the `try_table` (the spec validates clauses under `C`, before the block's own label is pushed).
- **`return_call` interacts correctly by construction** — and forced an ADR-18 correction: a thunk returned out of the body leaves the `begin/rescue` before the callee runs, so an exception thrown by a tail-called function escapes the caller's `try_table` as required. The "plain call for non-tail-calling callees" shortcut ADR-18 originally allowed was observably wrong here and was removed (every `return_call` now thunks).

## Rejected alternatives

- **Tag = symbol/index keyed by module** — breaks imported-tag identity across instances (`catch-imported` in `try_table.wast`); an interned structural key (the `type_symbol` trick) is wrong by design here because tag equality is *not* structural.
- **`exnref` as a separate wrapper around (tag, values)** — the exception object already *is* that tuple; a second object would need converting at every catch_ref/throw_ref boundary.
- **Routing traps and exceptions through one class ladder** — a `rescue` of a common superclass would make it too easy for generated code to catch traps; two unrelated classes make the "traps are uncatchable" property structural rather than disciplined.
- **`Partial("try_table only")`** — unnecessary: the pinned suite has no legacy-EH constructs (verified by grep before implementation); legacy binaries fail validation (the `LEGACY_EXCEPTIONS` validator feature stays off) and surface as clean `unknown-proposal` refusals.

## Consequences

- Positive: `try_table` (44), `throw` (9), `throw_ref` (12), `tag` (1 + skips attributed to gc-era constructs) all pass; full Ruby run pass 29,598 → 29,679. `assert_exception` is now a real check (`SpecLang::emit_check_exception`; the default keeps it an attributed skip for Bash).
- **List change (ADR-8): `imports.wast` expected failures 28 → 59**, same `import-limits` tag. Its "test" fixture module exports tags, so it never converted before this ADR; now that it does, the downstream `assert_unlinkable` cases checking function signatures, global types, and tag parameter types run — all instances of the ADR-16 kind-not-type gap, no new mechanism. Every other list entry (imports2 2, linking 4, linking0 1, load1 5) is byte-identical, and Bash's run is unchanged.
- Negative / carry-over: tag parameter types join the `import-limits` debt. An uncaught wasm exception in `--mode standalone` surfaces as a raw Ruby backtrace (no dedicated exit path like `Rt::Trap`'s 134) — acceptable until a real p2/component consumer defines better.

See also: [ADR-16](16-ruby-wasm1-completion.md) (provider protocol, kind-not-type gap), [ADR-17](17-ruby-reference-types.md) (flat ref `ValType` variants), [ADR-18](18-ruby-tail-calls.md) (the trampoline this corrected).
