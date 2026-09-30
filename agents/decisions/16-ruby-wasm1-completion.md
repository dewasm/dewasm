# Decision 16: Wasm 1.0 in Ruby: Non-Function Imports, Multiple Tables, Bulk Table Operations, Linking

Status: **Accepted, 2026-07-24.**
Implemented:

- `crates/dewasm-core/src/{ir,module,func}.rs`;
- `crates/dewasm-backend/src/lib.rs`;
- `crates/dewasm-backend-ruby/src/lib.rs`;
- `runtime/ruby/units/{global,table,rt}/*.rb`;
- the specification harness in `crates/dewasm-test-helper/src/spec.rs`;
- the per-backend `crates/dewasm-backend-{ruby,bash}/tests/spec.rs`.

## Context

`docs/support.md` listed five wasm-1.0-scoped gaps for every backend (decision 8's "declared debt"):

- imported globals;
- imported memories;
- imported tables;
- multiple tables;
- the table half of bulk memory (passive/declared element segments, `table.init`/`table.copy`/`elem.drop`).

The core IR builder rejected all five universally.
So no backend had ever needed to think about them.
The plan closed them for Ruby only and left Bash exactly as unsupported as before.
That required a decision about representation (globals, tables).
It also required a decision about a mechanism the core builder no longer provides for free.
That mechanism is per-backend conditioning.

## Decision

- **Every wasm global is a boxed `Rt::Global`**, not a plain instance variable holding the value.
  The class has a `value` accessor and lives in `runtime/ruby/units/global/_class.rb`.
  `Expr::GlobalGet` lowers to `@g{idx}.value`, and `Stmt::GlobalSet` to `@g{idx}.value = ...`.
  This holds uniformly for local and imported globals.
  **Criterion:** a global that crosses an instantiation boundary must be a shared mutable cell.
  It must not be a copied value.
  Crossing means imported, or exported and later imported by another instance.
  `Memory`/`Table` are already always objects for the same reason.
  So making `Global` follow suit keeps one representation.
  Two representations would need two paths through every place a global is read, written, or exported.
  **Replaced for performance:** only globals that actually cross a boundary are boxed now.
  Those are imported globals and `ExportKind::Global` ones.
  The criterion above is unchanged.
  See the "Rejected alternatives" entry below, now adopted.
- **Imports beyond functions reuse decision 7's mechanism as-is.**
  `Rt.resolve_import(imports, mod, name)` already returns whatever object the embedder supplied.
  Nothing about it was function-specific.
  Code generation for imported memory/table/global calls it exactly like imported functions do.
  The call site is `crates/dewasm-backend-ruby/src/lib.rs`'s `resolve_import_string`.
  It just assigns into `@memory`, `@t{N}`, or `@g{N}` instead of `@if{N}`.
- **A present-but-wrong-*kind* import is now a link error.**
  `Rt.check_import_kind(value, kind, mod, name)` checks a resolved import before accepting it.
  It lives in `runtime/ruby/units/rt/check_import_kind.rb`.
  Functions must be a `Method`/`Proc`.
  `Global`/`Table`/`Memory` self-report via a `wasm_kind` reader each class now implements.
  A `nil` (missing) import still falls through to the caller's `||` fallback (WASI/ENOSYS/raise).
  A present wrong-kind one raises immediately and never silently substitutes.
  **Accepted narrower gap:** only the *kind* is checked, not the full wasm type.
  These are not compared:

  - function parameter/result types;
  - global mutability;
  - table/memory `min`/`max` limits against the import site's declared bounds.

  The gap surfaces as `import-limits`-tagged entries in `EXPECTED_FAILURES`.
  That list is in `crates/dewasm-backend-ruby/tests/spec.rs`.
  Implementing it would mean carrying `FuncType`/limit data into generated code.
  That data would exist purely for that check.
  The check has no runtime-correctness gain beyond `assert_unlinkable` conformance.
- **Table index space is `imported_tables ++ tables`**, matching how functions already worked (`ir::Module`).
  Ruby gives each table a fixed `@t{N}` instance variable, the same shape as `@g{N}`/`@if{N}`.
  `N` is always a compile-time constant.
  Wasm 1.0 encodes `call_indirect`/element-segment table indices as immediates, never computed.
- **Element segments are retained at instantiation.**
  This mirrors active data segments, kept as `@data{i}` hexadecimal strings for `memory.init`/`data.drop`.
  `ir::ElemSegment` gained `kind: Active{table_index,offset} | Passive | Declared`.
  It also gained `items: Vec<Option<u32>>`, where `None` is a `ref.null` item.
  Every segment becomes `@elem{i}`, an array of `[type_idx, func_ref]` pairs or `nil`.
  Active ones fill their table at once via the new `Rt::Table#init`.
  Then they mark themselves already-dropped (`@elem{i} = []`), exactly like active data segments do.
  New IR: `Stmt::TableInit`/`TableCopy`/`ElemDrop`.
  New units: `table/init.rb`, `table/copy.rb`, `table/slice.rb`.
  Cross-table `table.copy` needs another `Rt::Table`'s raw arrays, exposed via a small `slice(offset, len)`.
  `Array#[]` always returns a fresh array, so self-copy overlap is safe automatically.
  This is the same trick `memory/copy.rb` already plays with `String#byteslice`.
  `table.get`/`set`/`grow`/`size`/`fill` stay rejected under `Feature::ReferenceTypes`.
  These were confirmed never part of wasm 1.0's MVP instruction set.
  They (table.get/set in particular) shipped later alongside reference types.
  So this is scope, not a partial implementation.
- **A shared `check_module_support(backend, module)` replaces the core builder's old conditioning.**
  The core builder used to do that conditioning unconditionally.
  The function is in `crates/dewasm-backend/src/lib.rs`.
  The core IR is now backend-independent about all five constructs.
  So each backend must refuse what it hasn't implemented itself, at conversion time.
  That is decision 0's contract.
  The refusal carries the same `UnsupportedError` attribution the core used to produce.
  It is called first thing in `dewasm-backend-ruby::generate_class_inner`.
  It is also called first thing in `dewasm-backend-bash::generate_module_inner`.
  This is the entire reason Bash's declared support didn't have to move.
- **Generated classes are their own decision 7 import providers.**
  Every class gets a public `import(name)`.
  It checks `@exports`, then `GLOBAL_EXPORTS`, `TABLE_EXPORTS`, `MEMORY_EXPORTS`.
  So one instance is directly usable as another's import source (`imports["M"] = other_instance`).
  This applies decision 7 to generated classes too; it is not a new mechanism.
  The specification harness could then implement `wast`'s `(register "Name" $id)` directive for real.
  The changes are in `crates/dewasm-test-helper/src/spec.rs`:

  - `ScriptGen` tracks registered-name → live instance;
  - `convert()` takes the set of module names it may treat as import sources;
  - `assert_unlinkable` is checked for real instead of always skipped.
    Any raised error during instantiation counts; upstream's exact wording never matches ours.

  `SpecLang::supports_registered_imports()` tests all of this per language.
  It is true for Ruby and false for Bash.
  The reason for Bash is its ambient global `IMPORTS` associative array.
  That array has no per-instance import object to extend this way.
  **This is harness-only use of the pre-existing imports-Hash mechanism.
  A real embedder would also use that mechanism.
  `dewasmify` itself still converts exactly one wasm module into one class.
  Decision 0's "cross-module linking is out of scope for the tool" is unchanged.**
  What changed: the test harness can now exercise import resolution.
  A Ruby embedder linking two generated classes by hand already could do so the same way.

## Rejected alternatives

- **Box only imported globals and exported mutable globals.**
  Keep plain instance variables for the rest.
  It saves an allocation and a `.value` indirection on the common case.
  The cost is two code generation paths for every global read/write/export site.
  It also needs a runtime decision that code generation can't always know locally.
  That decision is whether this global is ever imported elsewhere.
  Rejected at the time, following the earlier `Memory`/`Table` choice.
  It was also rejected for keeping `GlobalGet`/`GlobalSet` lowering a single rule.
  **Adopted later**: the two-path cost was smaller than stated.
  The decision is knowable statically.
  `boxed_globals = imported ∪ ExportKind::Global` is computed once per module.
  That works since wasm 1.0's export/import sets are fixed at conversion time, not runtime.
  A `global_ref` helper keeps `GlobalGet`/`GlobalSet`/`ElemItem::Global` at one call site each.
  `boxed_globals` computes exactly the boundary criterion above.
  That criterion is: crosses an instantiation boundary ⇒ needs the box.
  Only the half with plain instance variables for everything else was actually implemented.
- **Full wasm-type import validation (signatures, mutability, limits).**
  Its real correctness value is only for `assert_unlinkable` conformance.
  No embedder-visible behavior changes for a correctly-typed import.
  That is the only case that matters outside the specification harness.
  Deferred as documented debt (`import-limits` list entries) rather than implemented on a guess.
- **Keep the core builder rejecting these constructs per-backend.**
  That means a `Backend` parameter threaded into `build_module`.
  It would bring backend concerns into the shared, backend-independent IR builder.
  Decision 0 says "adding a language must not require touching the core".
  A check after IR building, in the shared backend-trait crate, keeps `dewasm-core` untouched.
  It gives every future backend the same free conditioning Bash uses here.

## Consequences

- Positive: `docs/support.md` shows ✅ for Ruby on all five rows.
  The full specification-suite run pass count went 24,338 → 29,233.
  The `fail` count went 23 → 40.
  Every one of the 17 new failures is re-attributed to the two narrow, documented gaps above.
  This was verified file by file; none are regressions.
  Bash's run is identical to the baseline before this milestone (pass=24,338, fail=23).
  `check_module_support` contained the scope of impact entirely.
- Positive: the `import(name)` provider method is not just for the specification harness.
  It is a real capability for any Ruby embedder linking two dewasm-generated classes by hand.
- Negative / carry-over: `import-limits` stays open debt.
  The debt concerns function/global/table/memory type data.
  It stays open until (if ever) that data is worth carrying at runtime.
  Carrying it would serve purely stricter unlinkable detection.
  Bash gaining any of these five features later is a separate milestone of the same shape.
  Its own `check_module_support` call means it costs nothing until then.

See also:

- [decision 0](0-foundation.md) (scope, cross-module linking);
- [decision 1](1-ir-design.md) (IR design);
- [decision 4](4-ruby-backend-lowering.md) (Ruby lowering conventions this extends);
- [decision 6](6-runtime-units.md) (runtime units);
- [decision 7](7-import-providers.md) (the import provider protocol this generalizes);
- [decision 8](8-latest-testsuite-support-matrix.md) (the support matrix and skip-attribution policy this fulfills).
