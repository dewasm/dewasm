# Decision 17: Ruby Reference Types (funcref = the Table Pair, externref = a Raw Host Value)

Status: **Superseded by [decision 24](24-01-scope-reset.md), 2026-07-26.**
Kept as a design record for a future restoration of this support.
Git history plus this decision make the work cheap to revive.
The original acceptance note and implementation pointers below are retained as history.

Originally accepted 2026-07-24.
Implemented in:

- `crates/dewasm-core/src/{ir,module,func}.rs`;
- `crates/dewasm-backend/src/lib.rs`;
- `crates/dewasm-backend-ruby/src/lib.rs`;
- `runtime/ruby/units/table/*.rb`;
- the spec harness's ref-valued arguments/results (`crates/dewasm-backend-ruby/tests/spec.rs`).

## Context

Reference types is the first post-1.0 proposal to land.
It is the opening move of the wasm 2.0+ / component-model roadmap.
It is also the first to put non-numeric values on the wasm stack.
`funcref` and `externref` flow through locals, block results, `select`, and globals.
They also flow through the new table instructions.
Those are `table.get/set/grow/size/fill` and `ref.null/ref.func/ref.is_null`.
The IR's `ValType` had exactly four numeric variants.
`Rt::Table` stored elements as parallel `@types`/`@funcs` arrays.
Nothing else could produce or consume them.
A representation had to be chosen for both reference kinds in Ruby.
The decision 16 test had to keep Bash rejecting every new construct at conversion time.

## Decision

- **A funcref value *is* the table slot: the `[type_symbol, callable]` pair.**
  Element segments already built that pair for `Rt::Table` (decision 16).
  `ref.func` emits the same pair (`Gen::func_pair`, `crates/dewasm-backend-ruby/src/lib.rs`).
  So `ref.func` → `table.set` → `call_indirect` round-trips with no conversion anywhere.
  **Criterion: one representation per wasm value type.**
  **It is chosen so the most semantics-critical consumer needs no adaptation.**
  That consumer is `call_indirect`'s structural type check (decision 4).
  The callable alone was rejected: the type symbol would have to be recomputed from a `Method` object.
  That is impossible across module boundaries.
  The symbol is interned from the *type shape*, not derivable from a Ruby callable.
- **An externref value is the raw host object; null is `nil` for both kinds.**
  No wrapper.
  Accepted degeneracy: a host may pass Ruby `nil` as a "non-null" externref.
  That is indistinguishable from `ref.null extern`.
  Wasm's null is whatever the host's null is, the same equation every JS embedding uses.
- **`Rt::Table` stores one `@slots` array** instead of parallel `@types`/`@funcs`.
  The class is in `runtime/ruby/units/table/_class.rb`.
  Since a funcref is the pair, slots are representation-agnostic.
  `get`/`set`/`grow`/`fill`/`copy`/`init` move values without inspecting them; only `call` destructures.
  Tables now carry their `max` (new `Table.new(min, max)`).
  That is because `table.grow` must refuse growth past it (`table/grow.rb`, returns `0xffffffff`).
- **`ValType` gains flat `FuncRef`/`ExternRef` variants**, not a structured `Ref(RefType)`.
  Typed function references/GC will force the structured form eventually.
  Until then the flat variants keep every backend match one arm per type.
  `ValType::is_ref()` + `default_value` funnel the spots that will need migrating.
- **Element items became a proper enum** (`ir::ElemItem::Func | Null | Global`).
  The testsuite's `elem.wast` initializes a table slot from an imported funcref global.
  That is a `global.get` item, which `Option<u32>` could not express.
  Ruby renders `Global(i)` as `@g{i}.value`.
- **Conditioning: `check_module_support` grew a `ReferenceTypes` require.**
  It is backed by the first exhaustive `Expr` walk, `module_uses_reference_types`.
  That walk is in `crates/dewasm-backend/src/lib.rs`.
  The walk finds:
  - ref-typed values anywhere (signatures, globals, locals, temps);
  - externref tables (funcref tables are MVP and must *not* trigger it);
  - the new instructions.

  Like `stmts_use_table_bulk_ops`, both walks are exhaustive on purpose.
  So a future `Stmt`/`Expr` variant is a compile error, not a silent mis-lowering.
  Bash's only change is `unreachable!` match arms.
- **The harness expresses ref-valued directives in the same representation.**
  That is in `crates/dewasm-backend-ruby/tests/spec.rs`.
  `(ref.extern n)` args/results are the Integer `n`, and nulls are `nil`.
  `(ref.func)` results check for the pair shape.
  This had to land in the same change as the `Supported` flip.
  The harness's anti-regression check (`crates/dewasm-test-helper/src/spec.rs`) is the reason.
  It turns any leftover `reference-types`-tagged skip into a suite failure.

## Rejected alternatives

- **funcref = bare callable, type symbol looked up at call time.**
  It needs a side table from callable to type symbol.
  It breaks for callables imported from another module instance where no such table exists.
  The pair costs one array per reference and buys structural identity that travels with the value.
- **A `Rt::FuncRef` wrapper class**: same information as the pair.
  It adds an extra class, allocation, and accessor per touch.
  The pair is already the established table wire format.
- **Wrapping externref so host `nil` ≠ wasm null**: a wrapper on every host↔wasm crossing.
  It preserves a distinction no realistic embedder relies on.
  Rejected for the same "raw host value" reasoning as decision 14's use of plain `IO` objects.
- **`ValType::Ref(RefType)` now**: structurally future-proof.
  It forces nested matching on all backends for two inhabitants, though.
  The flat variants plus `is_ref()` keep today simple and mark the migration points for GC.

## Consequences

- Positive: full Ruby run pass 29,233 → 29,516; `reference-types` disappeared from the skip histogram.
  Fail count stays 40 with the decision 16 `import-limits` list byte-identical.
  Bash's totals are unchanged: the test contained the scope of impact exactly as designed.
- Positive: funcref tables, `ref.func` globals, and cross-module table sharing all speak one format.
  So tail calls (a third pair element or sibling) build on this.
  They need no other representation decision.
  The component model's funcref shim tables build on this the same way.
- Negative / carry-over: the externref `nil` degeneracy is permanent by construction.
  The flat `ValType` variants are a known migration debt for typed function references/GC.
  `table_grow.rb` shares one init object across new slots.
  That is correct, since references are values.
  It is worth knowing when reading dumps.

See also:

- [decision 4](4-ruby-backend-lowering.md): the `type_symbol` mechanism this reuses;
- [decision 16](16-ruby-wasm1-completion.md): the table machinery and conditioning mechanism this extends;
- [decision 8](8-latest-testsuite-support-matrix.md): skip attribution that forced the harness work to be atomic with the flip.
