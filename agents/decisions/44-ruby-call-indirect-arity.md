# ADR-44 — Ruby Backend: Fixed-Arity `call_indirect` Dispatch

Status: **Accepted, 2026-07-28.**
Implemented in `crates/dewasm-backend-ruby/src/lib.rs` + `runtime/ruby/units/table/call{0..8}.rb`.
Amends [ADR-4](4-ruby-backend-lowering.md)'s `call_indirect` bullet: the structural type-symbol comparison it fixed still binds; only the splat-array *dispatch* it used is superseded here.

## Context

ADR-4 renders `call_indirect` as `@tT.call(index, type_sym, *args)`, where `Table#call` re-splats the collected `*args` into the callee (`func.call(*args)`).
Both splats allocate a fresh `T_ARRAY` per indirect call.
SQLite's VDBE and virtual-table dispatch route almost everything through `call_indirect`: profiling the converted `sqlite3-shell` on the benchmark workload put `Rt::Table#call` at 3.7% of CPU, and its `*args` array at ~0.4M `T_ARRAY` allocations per run (5.2% of the run's objects).

The argument count is a static property of the call site's type signature, so the splat is avoidable — the arity is known at conversion time.
Measuring the two `call_indirect`-heavy real-world apps confirms a small fixed ceiling covers every site (arity = signature parameter count):

| arity | 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | >8 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| sqlite3-shell (2018 sites) | 31 | 1390 | 184 | 166 | 183 | 29 | 35 | 0 | 0 | 0 |
| qjs (1317 sites) | 0 | 522 | 524 | 239 | 13 | 8 | 7 | 1 | 3 | 0 |

Arity 1 alone is 68.9% of sqlite's sites; qjs tops out at 8.
A ceiling of 8 covers 100% of both.

## Decision

- **A per-arity `Table#callN` for `0 ≤ N ≤ MAX_FIXED_ARITY` (= 8)**, one runtime unit each (`runtime/ruby/units/table/call0.rb` … `call8.rb`).
  Each takes the index, the type symbol, and exactly `N` positional parameters — no `*args` on the caller side — and invokes the callee with a fixed argument list (`func.call(a0, a1)`), no splat on the callee side either.
  The dispatch and trap contract is **identical** to `call`: `undefined element` (out of bounds), `uninitialized element` (null slot), `indirect call type mismatch` (structural symbol `!=`), then the call.
- **The backend emits `@tT.callN(index, type_sym, a0, …)`** when the site's signature has `N ≤ MAX_FIXED_ARITY` args, and `use_unit`s only the `callN` actually referenced — so a module bundles just the arities it uses, matching the existing per-method unit granularity (ADR-6).
- **The splat `call` stays as the fallback** for signatures wider than `MAX_FIXED_ARITY`.
  No such site appears in the measured apps, but general wasm permits any arity, so the path is kept for correctness.
- **`MAX_FIXED_ARITY` is a single named constant** in the backend; the `call{0..N}.rb` units must exist for the value chosen.

Semantics are unchanged: the same interned structural type symbols (`:"i32,i64->i32"`, ADR-4), the same `Rt.trap` on mismatch / null / out-of-bounds, and the same cross-module structural typing (a symbol interned from the type's *shape*, never a module-local index).
Wasm 1.0 returns (zero or one value) flow through `func.call`'s return exactly as before.

## Rejected alternatives

- **One variadic `call` with `func.call(*args)` (ADR-4, status quo).**
  Allocates two `T_ARRAY`s per indirect call; the 0.4M-array measurement above is the direct cost on a `call_indirect`-saturated workload.
- **A single `call` taking a splat but forwarding fixed via a `case args.size` inside.**
  Removes the callee splat but not the caller one, and adds a per-call branch; the caller-side array is the larger share and is only removable by generating fixed positional arguments at the call site.
- **Unbounded `callN` generation (a unit per distinct arity seen).**
  The measured ceiling is 8; a fixed constant with a splat fallback is simpler than threading the module's arity set into the runtime bundler, and the fallback covers the (unobserved) tail with no correctness gap.

## Consequences

- Positive: no `T_ARRAY` is built for any `call_indirect` at arity ≤ 8.
  On the `sqlite3-shell` benchmark workload total object allocations fell 2,570,922 → 2,117,639 (−453,283, ~17.6%), byte-identical output.
  The wall-clock gain is modest (~1% on this bench, which is dominated by other work); the win is allocation/GC pressure, which the profile attributed to the splat.
- Negative: nine near-identical small units instead of one.
  They share the trap contract by copy, so a change to that contract touches all nine (and the fallback `call`); the units lint (ADR-6) keeps their `# requires:` headers honest but does not dedupe the bodies.
- The spec harness (ADR-3) binds correctness: it passes for the Ruby backend under this lowering, `call_indirect.wast` included, and the `qjs`/`sqlite3-shell` heavy e2e cases pass.
