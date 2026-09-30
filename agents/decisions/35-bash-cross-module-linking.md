# Decision 35: Bash Cross-Module Linking

Status: **Accepted, 2026-07-27.**
Implemented in `crates/dewasm-backend-bash/src/lib.rs` (`Gen::init`).
The runtime side is `runtime/bash/units/rt/resolve_import.sh` and `rt/link_err.sh`.
Covers every wasm 1.0 import kind:

- imported functions (retained);
- imported globals (`Feature::ImportedGlobals` Supported);
- imported memories (`Feature::ImportedMemories` Supported);
- imported tables (`Feature::ImportedTables` Supported).

It also covers the `shared_table_call_indirect` multi-module e2e case.
That case is in `crates/dewasm-backend-bash/tests/e2e.rs`, via `BackendUnderTest::compose_modules`.
Extends [decision 11](11-bash-backend-lowering.md).

## Context

The Ruby backend links across modules with a single `import(name)` provider protocol.
Mutable imports share a boxed `Rt::Global` cell by reference ([decision 16](16-ruby-wasm1-completion.md)).
Bash has neither objects nor references.
State lives in prefix-scoped variables and arrays (`<p>g<i>`, `<p>t<i>`, `<p>mem`).
Until now the only import mechanism was the `IMPORTS[module.name]=command` associative array.
It covered functions only, with no shared mutable state and no kind checking.

Wasm imports need three things Bash did not have:

- a way to alias another module's global cell so reads *and* writes reach it;
- a provider protocol that spans every import kind, not just functions;
- a link-error signal distinct from a trap.

The specification harness's `register`/`assert_unlinkable` directives exercise all three.
So the harness could not be turned on for Bash without them.

## Decision

**Imported globals alias the provider's cell with a `declare -gn` nameref.**
For imported global `i`, `Gen::init` emits `declare -gn <p>g<i>=$RESOLVED`.
`$RESOLVED` is the *variable name* of the owning module's cell.
Three uses resolve through the nameref to the shared variable:

- reads (`(( x = <p>g<i> ))`);
- mutable writes (`(( <p>g<i> = v ))`);
- `global.get` in the initial expressions of offsets.

So mutation is visible in both modules with no boxing.
Defined globals keep their literal `<p>g<num_imported_globals + i>` slot in the unified index space.

**PROVIDERS + per-kind export maps are the Bash shape of the provider protocol.**
`PROVIDERS[module]` names a prefix `<q>` that owns the export maps.
They are `<q>EXPORTS` (functions), `<q>GLOBAL_EXPORTS`, `<q>TABLE_EXPORTS`, and `<q>MEMORY_EXPORTS`.
This is exactly the shape another generated module already emits.
`rt_resolve_import <mod> <name> <kind>` looks the name up in the kind's map.
It returns the value in `RESOLVED`.
Deciding rule: a name found under a *different* kind's map is a link error for the wrong kind.
A name found nowhere leaves `RESOLVED=''` so the caller chooses.
The caller falls back to WASI/ENOSYS for WASI modules, else raises a link error.
`IMPORTS` is retained as a function-only host override, checked ahead of `PROVIDERS`.

**Export values are flattened so nameref chains stay depth ≤ 1.**
A global or table export publishes its backing variable *name*.
A defined global/table publishes its own `<p>g<idx>`/`<p>t<idx>`.
A re-exported imported one publishes `${!<p>g<idx>}`/`${!<p>t<idx>}`.
That is the nameref's target, not the nameref itself.
A consumer's `declare -gn` then points in one step at the real cell/array.
So `${!name}` never has to chase a chain.
Memory has no single name to flatten to.
Its derived state (`mem`/`pages`/`max_pages`) is three variables, not one cell.
`MEMORY_EXPORTS` therefore publishes the owning module's *prefix* (`<p>memown`).
A re-export just forwards that string.
A consumer's three `declare -gn`s (`${RESOLVED}mem`/`pages`/`max_pages`) still each resolve in one step.

**Link errors return status 135** (`rt_link_err`).
It is the linking exit status beside `rt_trap`'s 134 and `rt_exit`'s 133.
It passes up through the same `|| return $?` chain.
This is an observable change: a missing import used to `echo ...; return 1`.

**Type identity across a shared table is the structural type key**, not a module-local number.
It had already landed for defined tables.
Two modules sharing a table must agree on `call_indirect` types, and their type sections do not.
So the tag is derived from the type's shape (`i32,i64->f32`).

## Rejected alternatives

- **Handle-prefix threading**: pass the owning module's prefix to every operation on imported state.
  It touches every lowering site.
  It cannot express an inline `(( <p>g<i> += 1 ))` on a shared global.
  The nameref makes the shared cell look local everywhere it is used.
- **Boxed indirection via `${!name}` / `printf -v` everywhere**: model a global as a name.
  Every use then reads/writes it indirectly.
  It works, but turns every global access into a two-step indirect read/write and is slower.
  The nameref confines the indirection to one `declare -gn` at instantiation.
- **One canonical numeric identifier per type for `call_indirect`**: compact.
  But a table shared across independently generated modules has no shared identifier space.
  So the tag would disagree across the boundary.

## Consequences

- Positive: mutable imported globals, memory, and tables all share state correctly with no boxing.
  They do so through the same `declare -gn` mechanism.
  `assert_unlinkable` is checked for real (status 135).
  So the specification harness's `register` support is on for Bash.
  Every import kind's `linking` list cluster clears (`elem`, `linking0`→table entries, `linking3`).
  A shared table crossing independently-generated modules now has an e2e case too.
  The case is `shared_table_call_indirect`.
  Its `compose_modules` generates each module against one bundled runtime.
- Negative: `rt_resolve_import` validates import *kind* but not the finer wasm type.
  The finer type is a function's signature or a global's mutability.
  It is also the minimum and maximum limits of a table or a memory.
  So a number of `assert_unlinkable` cases link instead of failing.
  They form the `import-limits` list cluster: `imports`/`imports2`/part of `linking`.
  It is the same accepted gap as Ruby.
  It is now at the same counts, since Bash covers every import kind Ruby does.
  135 is also `128 + SIGBUS` under the signal convention.
  No generated module starts a child process that can raise SIGBUS, so the shared value is accepted.
- Residual, unrelated to this decision: `linking0` and `load1` still fail one assertion each.
  The failures follow from a *different*, permanently out-of-scope gap.
  The core builder rejects a module declaring two memories.
  That is `Feature::MultiMemory`, a post-1.0 proposal (decision 24).
  So data meant for a shared memory through that module never runs.
  The failures carry the `multi-memory` list tag in `crates/dewasm-backend-bash/tests/spec.rs`.
