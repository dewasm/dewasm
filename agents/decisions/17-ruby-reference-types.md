# Decision 17: Reference Types in the Ruby Backend (funcref = the Table Pair, externref = a Raw Host Value)

Status: **Superseded by [decision 24](24-01-scope-reset.md), 2026-07-26.**
Kept as a design record for a future restoration of this support; git history plus this decision make the work cheap to revive.
The original acceptance note and implementation pointers below are retained as history.

Originally accepted 2026-07-24.
Implemented: `crates/dewasm-core/src/{ir,module,func}.rs`, `crates/dewasm-backend/src/lib.rs`, `crates/dewasm-backend-ruby/src/lib.rs`, `runtime/ruby/units/table/*.rb`, and the spec harness's ref-valued arguments/results (`crates/dewasm-backend-ruby/tests/spec.rs`).

## Context

Reference types is the first post-1.0 proposal to land (the opening move of the wasm 2.0+ / component-model roadmap), and the first to put non-numeric values on the wasm stack: `funcref` and `externref` flow through locals, block results, `select`, globals, and the new table instructions (`table.get/set/grow/size/fill`, `ref.null/ref.func/ref.is_null`).
The IR's `ValType` had exactly four numeric variants, and `Rt::Table` stored elements as parallel `@types`/`@funcs` arrays that nothing else could produce or consume.
A representation had to be chosen for both reference kinds in Ruby, and the decision 16 test had to keep Bash rejecting every new construct at conversion time.

## Decision

- **A funcref value *is* the table slot: the `[type_symbol, callable]` pair** that element segments already built for `Rt::Table` (decision 16).
  `ref.func` emits the same pair (`Gen::func_pair`, `crates/dewasm-backend-ruby/src/lib.rs`), so `ref.func` → `table.set` → `call_indirect` round-trips with no conversion anywhere.
  **Criterion: one representation per wasm value type, chosen so the most semantics-critical consumer (`call_indirect`'s structural type check, decision 4) needs no adaptation.**
  The callable alone was rejected because the type symbol would have to be recomputed from a `Method` object, which is impossible across module boundaries (the symbol is interned from the *type shape*, not derivable from a Ruby callable).
- **An externref value is the raw host object; null is `nil` for both kinds.**
  No wrapper.
  Accepted degeneracy: a host passing Ruby `nil` as a "non-null" externref is indistinguishable from `ref.null extern`: wasm's null is whatever the host's null is, the same equation every JS embedding uses.
- **`Rt::Table` stores one `@slots` array** (`runtime/ruby/units/table/_class.rb`) instead of parallel `@types`/`@funcs`: since a funcref is the pair, slots are representation-agnostic and `get`/`set`/`grow`/`fill`/`copy`/`init` move values without inspecting them; only `call` destructures.
  Tables now carry their `max` (new `Table.new(min, max)`) because `table.grow` must refuse growth past it (`table/grow.rb`, returns `0xffffffff`).
- **`ValType` gains flat `FuncRef`/`ExternRef` variants**, not a structured `Ref(RefType)`.
  Typed function references/GC will force the structured form eventually; until then the flat variants keep every backend match one arm per type, and `ValType::is_ref()` + `default_value` funnel the spots that will need migrating.
- **Element items became a proper enum** (`ir::ElemItem::Func | Null | Global`): the testsuite's `elem.wast` initializes a table slot from an imported funcref global (`global.get` item), which `Option<u32>` could not express.
  Ruby renders `Global(i)` as `@g{i}.value`.
- **Conditioning: `check_module_support` grew a `ReferenceTypes` require backed by the first exhaustive `Expr` walk** (`module_uses_reference_types`, `crates/dewasm-backend/src/lib.rs`): ref-typed values anywhere (signatures, globals, locals, temps), externref tables (funcref tables are MVP and must *not* trigger it), and the new instructions.
  Like `stmts_use_table_bulk_ops`, both walks are exhaustive on purpose so a future `Stmt`/`Expr` variant is a compile error, not a silent mis-lowering.
  Bash's only change is `unreachable!` match arms.
- **The harness expresses ref-valued directives in the same representation** (`crates/dewasm-backend-ruby/tests/spec.rs`): `(ref.extern n)` args/results are the Integer `n`, nulls are `nil`, `(ref.func)` results check for the pair shape.
  This had to land in the same change as the `Supported` flip: the harness's anti-regression check (`crates/dewasm-test-helper/src/spec.rs`) turns any leftover `reference-types`-tagged skip into a suite failure.

## Rejected alternatives

- **funcref = bare callable, type symbol looked up at call time**: needs a side table from callable to type symbol; breaks for callables imported from another module instance where no such table exists.
  The pair costs one array per reference and buys structural identity that travels with the value.
- **A `Rt::FuncRef` wrapper class**: same information as the pair with an extra class, allocation, and accessor per touch; the pair is already the established table wire format.
- **Wrapping externref so host `nil` ≠ wasm null**: a wrapper on every host↔wasm crossing to preserve a distinction no realistic embedder relies on; rejected for the same "raw host value" reasoning as decision 14's use of plain `IO` objects.
- **`ValType::Ref(RefType)` now**: structurally future-proof but forces nested matching on all backends for two inhabitants; the flat variants plus `is_ref()` keep today simple and mark the migration points for GC.

## Consequences

- Positive: full Ruby run pass 29,233 → 29,516; `reference-types` disappeared from the skip histogram; fail count stays 40 with the decision 16 `import-limits` list byte-identical.
  Bash's totals are unchanged: the test contained the scope of impact exactly as designed.
- Positive: funcref tables, `ref.func` globals, and cross-module table sharing all speak one format, so tail calls (a third pair element or sibling) and the component model's funcref shim tables build on this without another representation decision.
- Negative / carry-over: the externref `nil` degeneracy is permanent by construction.
  The flat `ValType` variants are a known migration debt for typed function references/GC.
  `table_grow.rb` shares one init object across new slots (correct, since references are values, but worth knowing when reading dumps).

See also: [decision 4](4-ruby-backend-lowering.md) (the `type_symbol` mechanism this reuses), [decision 16](16-ruby-wasm1-completion.md) (the table machinery and conditioning mechanism this extends), [decision 8](8-latest-testsuite-support-matrix.md) (skip attribution that forced the harness work to be atomic with the flip).
