# Decision 25: Retire the Support Maturity Levels for Plain Capability Declarations

Status: **Accepted, 2026-07-25.**
The removal has landed.
It removed the level type, its derivation functions, and the per-backend target declaration.
It also removed the level conditioning on the e2e case tables.
`docs/support.md` now renders a flat `## Features` table (the in-scope subset).
The `## WASI preview 1` table follows it.
Replaces [decision 23](23-backend-support-levels.md).
Since [decision 69](69-exception-handling-accepted-input.md), one features row differs per backend.
That row is exception handling.
The per-feature declarations this decision kept express it, and the maturity levels stay retired.

## Context

Decision 23 introduced a Zig-style four-level scale over wasm 1.0 + WASI p1.
One day later, decision 24 cut the input scope to exactly that surface.
It set the same bar for every 0.1 backend: a passing specification harness + full WASI p1.
With the 2.0+/CM labels gone and all backends aiming at one bar, the scale loses its use.
Levels 1-2 differ only by a list flag and twelve WASI functions.
Level 3 is just "filesystem not done yet".

## Decision

Retire the levels.
Backends declare capabilities directly.
Those are feature support (`Backend::feature_status`) and per-function WASI p1 coverage.
The latter is `Backend::has_wasi_p1`, derived from runtime units.
`docs/support.md` renders those declarations flat: a features table and a WASI p1 table.
It keeps the in-scope/out-of-scope distinction for the socket surface.
E2e coverage is expressed by which suites a backend crate calls (decision 27).
It is not expressed by comparing level numbers.
The level type, the achieved/target computations, and the level conditioning are removed.
That conditioning sat in the e2e case tables.

Criterion: **a ranking earns its keep only while it tells backends apart.**
When every backend targets the same bar, "which capabilities are done" is the whole truth.
A scalar summary of it is then noise.
Whether some summary scale is worth reintroducing is explicitly deferred.
It waits until the Python/Go/Java backends exist and show what actually varies.

## Rejected alternatives

- **Keep the levels**: collapses as described.
  It also forces every new test case to be level-classified.
  Decision 23's own experience showed that goes wrong when done by guessing instead of running.
- **A numeric coverage score** (e.g. "38/42 WASI functions"): false precision.
  The per-function table already says this without pretending the functions are of equal worth.

## Consequences

- Positive: one less set of categories to keep true.
  New-backend authors read a list of things to check, not a specification of levels.
- Negative: README/support.md lose a one-glance maturity summary.
  "production-ready?" now takes reading two tables.
- Carry-over: the decision 23 lesson survives in the support.md snapshot test and the harness.
  The lesson is that support claims must be verified by execution before being declared.
