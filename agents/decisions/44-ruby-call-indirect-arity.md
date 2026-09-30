# Decision 44: Ruby Backend Fixed-Arity `call_indirect` Dispatch

Status: **Accepted, 2026-07-28.**
Implemented in `crates/dewasm-backend-ruby/src/lib.rs` + `runtime/ruby/units/table/call{0..8}.rb`.
Changes [decision 4](4-ruby-backend-lowering.md)'s `call_indirect` bullet.
The structural type-symbol comparison that bullet fixed still binds.
Only the splat-array *dispatch* it used is replaced here.

## Context

Decision 4 renders `call_indirect` as `@tT.call(index, type_sym, *args)`.
There `Table#call` re-splats the collected `*args` into the callee (`func.call(*args)`).
Both splats allocate a fresh `T_ARRAY` per indirect call.
SQLite's VDBE and virtual-table dispatch route almost everything through `call_indirect`.
Profiling the converted `sqlite3-shell` on the benchmark workload put `Rt::Table#call` at 3.7% of CPU.
Its `*args` array came to ~0.4M `T_ARRAY` allocations per run (5.2% of the run's objects).

The argument count is a static property of the call site's type signature.
So the splat is avoidable: the arity is known at conversion time.
The two `call_indirect`-heavy real-world apps were measured.
A small fixed limit covers every site (arity = signature parameter count):

| arity | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | >8 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `sqlite3-shell` (2018 sites) | 31 | 1390 | 184 | 166 | 183 | 29 | 35 | 0 | 0 | 0 |
| `qjs` (1317 sites) | 0 | 522 | 524 | 239 | 13 | 8 | 7 | 1 | 3 | 0 |

Arity 1 alone is 68.9% of the sites of `sqlite3-shell`; `qjs` tops out at 8.
A limit of 8 covers 100% of both.

## Decision

- **A per-arity `Table#callN` for `0 ≤ N ≤ MAX_FIXED_ARITY` (= 8)**, one runtime unit each.
  The units are `runtime/ruby/units/table/call0.rb` … `call8.rb`.
  Each takes the index, the type symbol, and exactly `N` positional parameters.
  So the caller side has no `*args`.
  Each calls the callee with a fixed argument list (`func.call(a0, a1)`).
  So the callee side has no splat either.
  The dispatch and trap contract is **identical** to `call`, in this order:
  1. `undefined element` (out of bounds);
  2. `uninitialized element` (null slot);
  3. `indirect call type mismatch` (structural symbol `!=`);
  4. the call.
- **The backend emits `@tT.callN(index, type_sym, a0, …)`** for a site whose signature has `N` arguments.
  It does so when `N ≤ MAX_FIXED_ARITY`.
  It `use_unit`s only the `callN` actually referenced.
  So a module bundles just the arities it uses.
  This matches the existing split into one unit per method (decision 6).
- **The splat `call` stays as the fallback** for signatures wider than `MAX_FIXED_ARITY`.
  No such site appears in the measured apps.
  But general wasm permits any arity, so the path is kept for correctness.
- **`MAX_FIXED_ARITY` is a single named constant** in the backend.
  The `call{0..N}.rb` units must exist for the value chosen.

Semantics are unchanged:

- the same interned structural type symbols (`:"i32,i64->i32"`, decision 4);
- the same `Rt.trap` on mismatch / null / out-of-bounds;
- the same cross-module structural typing.
  The symbol is interned from the type's *shape*, never a module-local index.

Wasm 1.0 returns (zero or one value) flow through `func.call`'s return exactly as before.

## Rejected alternatives

- **One `call` for any argument count, with `func.call(*args)` (decision 4, the current state).**
  It allocates two `T_ARRAY`s per indirect call.
  The 0.4M-array measurement above is the direct cost on a `call_indirect`-saturated workload.
- **A single `call` taking a splat but forwarding fixed via a `case args.size` inside.**
  It removes the callee splat but not the caller one, and adds a per-call branch.
  The caller-side array is the larger share.
  It is only removable by generating fixed positional arguments at the call site.
- **Unbounded `callN` generation (a unit per distinct arity seen).**
  The measured limit is 8.
  A fixed constant with a splat fallback is simpler.
  The alternative threads the module's arity set into the runtime bundler.
  The fallback covers the (unobserved) tail with no correctness gap.

## Consequences

- Positive: no `T_ARRAY` is built for any `call_indirect` at arity ≤ 8.
  On the `sqlite3-shell` benchmark workload, total object allocations fell 2,570,922 → 2,117,639.
  That is −453,283, ~17.6%, with byte-identical output.
  The wall-clock gain is small: ~1% on this benchmark, which is dominated by other work.
  The win is allocation/GC pressure, which the profile attributed to the splat.
- Negative: nine near-identical small units instead of one.
  They share the trap contract by copy.
  So a change to that contract touches all nine (and the fallback `call`).
  The units lint (decision 6) keeps their `# requires:` headers honest but leaves the repeated bodies.
- The specification harness (decision 3) binds correctness.
  It passes for the Ruby backend under this lowering, `call_indirect.wast` included.
  The `qjs`/`sqlite3-shell` heavy e2e cases pass.
