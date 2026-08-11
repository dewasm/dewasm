# ADR-35 — Bash Cross-Module Linking

Status: **Accepted, 2026-07-27.**
Implemented in `crates/dewasm-backend-bash/src/lib.rs` (`Gen::init`) + `runtime/bash/units/rt/resolve_import.sh` and `rt/link_err.sh`.
Covers every wasm 1.0 import kind: imported functions (retained), imported globals (`Feature::ImportedGlobals` Supported), imported memories (`Feature::ImportedMemories` Supported), and imported tables (`Feature::ImportedTables` Supported) — plus the `shared_table_call_indirect` multi-module e2e case (`crates/dewasm-backend-bash/tests/e2e.rs`, `BackendUnderTest::compose_modules`).
Extends [ADR-11](11-bash-backend-lowering.md).

## Context

The Ruby backend links across modules with a single `import(name)` provider protocol and a boxed `Rt::Global` cell that mutable imports share by reference ([ADR-16](16-ruby-wasm1-completion.md)).
Bash has neither objects nor references: state lives in prefix-scoped variables and arrays (`<p>g<i>`, `<p>t<i>`, `<p>mem`), and until now the only import mechanism was the `IMPORTS[module.name]=command` associative array — functions only, no shared mutable state, no kind checking.

Wasm imports need three things Bash did not have: a way to alias another module's global cell so reads *and* writes reach it; a provider protocol that spans every import kind, not just functions; and a link-error signal distinct from a trap.
The spec harness's `register`/`assert_unlinkable` directives exercise all three, so the harness could not be turned on for Bash without them.

## Decision

**Imported globals alias the provider's cell with a `declare -gn` nameref.**
For imported global `i`, `Gen::init` emits `declare -gn <p>g<i>=$RESOLVED`, where `$RESOLVED` is the *variable name* of the owning module's cell.
Reads (`(( x = <p>g<i> ))`), mutable writes (`(( <p>g<i> = v ))`), and init-expr `global.get` offsets all resolve through the nameref to the shared variable, so mutation is visible in both modules with no boxing.
Defined globals keep their literal `<p>g<num_imported_globals + i>` slot in the unified index space.

**PROVIDERS + per-kind export maps are the Bash shape of the provider protocol.**
`PROVIDERS[module]` names a prefix `<q>` that owns the export maps `<q>EXPORTS` (functions), `<q>GLOBAL_EXPORTS`, `<q>TABLE_EXPORTS`, `<q>MEMORY_EXPORTS` — exactly the shape another generated module already emits.
`rt_resolve_import <mod> <name> <kind>` looks the name up in the kind's map and returns the value in `RESOLVED`.
Discriminating rule: a name found under a *different* kind's map is an incompatible-type link error; a name found nowhere leaves `RESOLVED=''` so the caller chooses (WASI/ENOSYS fallback for WASI modules, else a link error).
`IMPORTS` is retained as a function-only host override, checked ahead of `PROVIDERS`.

**Export values are flattened so nameref chains stay depth ≤ 1.**
A global or table export publishes its backing variable *name*: a defined global/table publishes its own `<p>g<idx>`/`<p>t<idx>`, a re-exported imported one publishes `${!<p>g<idx>}`/`${!<p>t<idx>}` (the nameref's target, not the nameref itself).
A consumer's `declare -gn` then points one hop at the real cell/array, so `${!name}` never has to chase a chain.
Memory has no single name to flatten to — its derived state (`mem`/`pages`/`max_pages`) is three variables, not one cell — so `MEMORY_EXPORTS` instead publishes the owning module's *prefix* (`<p>memown`); a re-export just forwards that string, and a consumer's three `declare -gn`s (`${RESOLVED}mem`/`pages`/`max_pages`) still each resolve in one hop.

**Link errors return status 135** (`rt_link_err`), the linking sibling of `rt_trap`'s 134 and `rt_exit`'s 133, propagated through the same `|| return $?` cascade.
This is an observable change: a missing import used to `echo ...; return 1`.

**Type identity across a shared table is the structural type key**, not a module-local numeric id (already landed for defined tables): two modules sharing a table must agree on `call_indirect` types, and their type sections do not, so the tag is derived from the type's shape (`i32,i64->f32`).

## Rejected alternatives

- **Handle-prefix threading** — pass the owning module's prefix to every operation that touches imported state.
  Touches every lowering site and cannot express an inline `(( <p>g<i> += 1 ))` on a shared global; the nameref makes the shared cell look local everywhere it is used.
- **Boxed indirection via `${!name}` / `printf -v` everywhere** — model a global as a name and read/write it indirectly at every use.
  Works, but churns every global access into a two-step indirect read/write and is slower; the nameref confines the indirection to one `declare -gn` at instantiation.
- **Numeric canonical type ids for `call_indirect`** — compact, but a table shared across independently generated modules has no shared id space, so the tag would disagree across the boundary.

## Consequences

- Positive: mutable imported globals, memory, and tables all share state correctly with no boxing, through the same `declare -gn` mechanism; `assert_unlinkable` is checked for real (status 135), so the spec harness's `register` support is fully on for Bash and every import kind's `linking` list cluster clears (`elem`, `linking0`→table entries, `linking3`).
  A shared table crossing independently-generated modules now has an e2e case too (`shared_table_call_indirect`, `compose_modules` generating each module against one bundled runtime).
- Negative: `rt_resolve_import` validates import *kind* but not the finer wasm type — a function's signature, a global's mutability, a table's min/max limits, or a memory's min/max limits — so a number of `assert_unlinkable` cases link instead of failing (the `import-limits` list cluster: `imports`/`imports2`/part of `linking`, the same accepted gap as Ruby, now at the same counts since Bash covers every import kind Ruby does).
  135 collides with the `128 + SIGBUS` signal convention; no generated module spawns a subprocess that can raise SIGBUS, so the collision is accepted.
- Residual, unrelated to this ADR: `linking0` and `load1` still fail one assertion apiece downstream of a *different*, permanently out-of-scope gap — a module declaring two memories is rejected outright by the core builder (`Feature::MultiMemory`, a post-1.0 proposal, ADR-24), so data meant for a shared memory through that module never runs (the `multi-memory` list tag in `crates/dewasm-backend-bash/tests/spec.rs`).
