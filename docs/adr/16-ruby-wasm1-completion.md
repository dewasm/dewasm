# ADR-16 — Completing Wasm 1.0 for Ruby: Non-Function Imports, Multiple Tables, Table Bulk Ops, Linking

Status: **Accepted, 2026-07-24.** Implemented: `crates/dewasm-core/src/{ir,module,func}.rs`,
`crates/dewasm-backend/src/lib.rs`, `crates/dewasm-backend-ruby/src/lib.rs`,
`runtime/ruby/units/{global,table,rt}/*.rb`, and the spec harness
(`crates/dewasm-cli/tests/spec/{main,ruby,bash}.rs`).

## Context

`docs/support.md` listed five wasm-1.0-scoped gaps for every backend (ADR-8's "declared debt"):
imported globals, imported memories, imported tables, multiple tables, and the table half of bulk
memory (passive/declared element segments, `table.init`/`table.copy`/`elem.drop`). The core IR
builder rejected all five universally, so no backend had ever needed to think about them. Closing
them for Ruby only, while leaving Bash exactly as unsupported as before, required decisions about
representation (globals, tables) and about a mechanism the core builder no longer provides for
free (per-backend gating).

## Decision

- **Every wasm global is a boxed `Rt::Global`** (`value` accessor,
  `runtime/ruby/units/global/_class.rb`), not a plain ivar holding the value. `Expr::GlobalGet`
  lowers to `@g{idx}.value`, `Stmt::GlobalSet` to `@g{idx}.value = ...`, uniformly for local and
  imported globals. **Criterion:** a global that crosses an instantiation boundary (imported, or
  exported and later imported by another instance) must be a shared mutable cell, not a copied
  value — `Memory`/`Table` are already always objects for the same reason, so making `Global`
  follow suit keeps one representation instead of two paths through every place a global is read,
  written, or exported.
- **Imports beyond functions reuse ADR-7's mechanism as-is.** `Rt.resolve_import(imports, mod,
  name)` already returns whatever object the embedder supplied; nothing about it was
  function-specific. Imported memory/table/global codegen calls it exactly like imported functions
  do (`crates/dewasm-backend-ruby/src/lib.rs`'s `resolve_import_string`), just assigning into
  `@memory`, `@t{N}`, or `@g{N}` instead of `@if{N}`.
- **A present-but-wrong-*kind* import is now a link error.** `Rt.check_import_kind(value, kind,
  mod, name)` (`runtime/ruby/units/rt/check_import_kind.rb`) checks a resolved import before
  accepting it: functions must be a `Method`/`Proc`; `Global`/`Table`/`Memory` self-report via a
  `wasm_kind` reader each class now implements. A `nil` (missing) import still falls through to
  the caller's `||` fallback (WASI/ENOSYS/raise); a present wrong-kind one raises immediately, never
  silently substitutes. **Accepted narrower gap:** only the *kind* is checked, not the full wasm
  type — function param/result types, global mutability, and table/memory min/max limits against
  the import site's declared bounds are not compared. This surfaces as the `import-limits`-tagged
  entries in `crates/dewasm-cli/tests/spec/ruby.rs`'s `EXPECTED_FAILURES`; implementing it fully
  would mean carrying `FuncType`/limit metadata into generated code purely for a check with no
  runtime-correctness payoff beyond `assert_unlinkable` conformance.
- **Table index space is `imported_tables ++ tables`**, matching how functions already worked
  (`ir::Module`). Ruby gives each table a fixed `@t{N}` ivar (`N` is always a compile-time
  constant — wasm 1.0 encodes `call_indirect`/element-segment table indices as immediates, never
  computed), the same shape as `@g{N}`/`@if{N}`.
- **Element segments are retained at instantiation**, mirroring how active data segments are
  already kept as `@data{i}` hex strings for `memory.init`/`data.drop`. `ir::ElemSegment` gained
  `kind: Active{table_index,offset} | Passive | Declared` and `items: Vec<Option<u32>>` (`None` =
  a `ref.null` item). Every segment becomes `@elem{i}` (an array of `[type_idx, func_ref]` pairs or
  `nil`); active ones eagerly populate their table via the new `Rt::Table#init` and then mark
  themselves already-dropped (`@elem{i} = []`), exactly like active data segments do. New IR
  `Stmt::TableInit`/`TableCopy`/`ElemDrop`; new units `table/init.rb`, `table/copy.rb`,
  `table/slice.rb`. Cross-table `table.copy` needs another `Rt::Table`'s raw arrays, exposed via a
  small `slice(offset, len)` — `Array#[]` always returns a fresh array, so self-copy overlap is
  safe automatically, the same trick `memory/copy.rb` already plays with `String#byteslice`.
  `table.get`/`set`/`grow`/`size`/`fill` stay rejected under `Feature::ReferenceTypes`: confirmed
  these were never part of wasm 1.0's MVP instruction set — they, table.get/set in particular,
  shipped later alongside reference types — so this is scope, not a partial implementation.
- **A shared `check_module_support(backend, module)`** (`crates/dewasm-backend/src/lib.rs`)
  replaces the gating the core builder used to do unconditionally. Because the core IR is now
  backend-agnostic about all five constructs, each backend must refuse what it hasn't implemented
  itself, at conversion time (ADR-0's contract), with the same `UnsupportedError` attribution the
  core used to produce. Called first thing in both `dewasm-backend-ruby::generate_class_inner`
  and `dewasm-backend-bash::generate_module_inner` — this is the entire reason Bash's declared
  support didn't have to move.
- **Generated classes are their own ADR-7 import providers.** Every class gets a public
  `import(name)` (checks `@exports`, then `GLOBAL_EXPORTS`, `TABLE_EXPORTS`, `MEMORY_EXPORTS`) so
  one instance is directly usable as another's import source (`imports["M"] = other_instance`) —
  applying ADR-7 symmetrically, not a new mechanism. This is what let the spec harness implement
  the wast testsuite's `(register "Name" $id)` directive for real:
  `crates/dewasm-cli/tests/spec/main.rs`'s `ScriptGen` tracks registered-name → live instance,
  `convert()` takes the set of module names it may treat as import sources, and `assert_unlinkable`
  is checked for real (any raised error during instantiation counts — upstream's exact wording
  never matches ours) instead of always skipped. `SpecLang::supports_registered_imports()` gates
  all of this per language (Ruby: true; Bash: false — its ambient global `IMPORTS` associative
  array has no per-instance import object to extend this way). **This is harness-only wiring using
  the pre-existing imports-Hash mechanism a real embedder would also use; dewasmify itself still
  converts exactly one wasm module into one class. ADR-0's "cross-module linking is out of scope
  for the tool" is unchanged** — what changed is that the test harness can now exercise import
  resolution the same way a Ruby embedder linking two generated classes by hand already could.

## Rejected alternatives

- **Box only imported/exported-mutable globals, keep plain ivars for purely-local ones** — saves
  an allocation and a `.value` indirection on the common case, at the cost of two codegen paths for
  every global read/write/export site and a runtime decision (is this global ever imported
  elsewhere?) that the generator can't always know locally. Rejected for the `Memory`/`Table`
  precedent and for keeping `GlobalGet`/`GlobalSet` lowering a single rule.
- **Full wasm-type import validation (signatures, mutability, limits)** — real correctness value
  only for `assert_unlinkable` conformance; no embedder-visible behavior changes for a
  correctly-typed import, which is the only case that matters outside the spec harness. Deferred as
  documented debt (`import-limits` ledger entries) rather than implemented speculatively.
- **Keep the core builder rejecting these constructs per-backend (a `Backend` parameter threaded
  into `build_module`)** — would leak backend concerns into the shared, backend-agnostic IR builder
  (ADR-0's "adding a language must not require touching the core"). A post-hoc gate in the shared
  backend-trait crate keeps `dewasm-core` untouched and gives every future backend the same
  free gating Bash uses here.

## Consequences

- Positive: `docs/support.md` shows ✅ for Ruby on all five rows; full spec-suite sweep pass count
  24,338 → 29,233, `fail` count 23 → 40 with every one of the 17 new failures re-attributed to the
  two narrow, documented gaps above (verified file by file, none are regressions). Bash's sweep is
  byte-identical to the pre-milestone baseline (pass=24,338, fail=23) — `check_module_support`
  contained the blast radius entirely.
- Positive: the `import(name)` provider method is a real capability for any Ruby embedder linking
  two dewasm-generated classes by hand, not just the spec harness.
- Negative / carry-over: `import-limits` stays open debt until (if ever) function/global/table/
  memory type metadata is worth carrying at runtime purely for stricter unlinkable detection.
  Bash gaining any of these five features later is a separate, symmetric milestone — its own
  `check_module_support` call means it costs nothing until then.

See also: [ADR-0](0-foundation.md) (scope, cross-module linking), [ADR-1](1-ir-design.md) (IR
design), [ADR-4](4-ruby-backend-lowering.md) (Ruby lowering conventions this extends),
[ADR-6](6-runtime-units.md) (runtime units), [ADR-7](7-import-providers.md) (the import provider
protocol this generalizes), [ADR-8](8-latest-testsuite-support-matrix.md) (the support matrix and
skip-attribution policy this fulfills).
