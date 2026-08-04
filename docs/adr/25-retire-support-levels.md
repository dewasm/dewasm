# ADR-25 — Retire the Support-Tier Ladder for Plain Capability Declarations

Status: **Accepted, 2026-07-25.** The removal of `Tier` and its derivation functions (`crates/dewasm-backend/src/tier.rs`), `Backend::target_tier`, and tier conditioning on the e2e case tables has landed; `docs/support.md` now renders a flat `## Features` table (the in-scope subset) plus the `## WASI preview 1` table. Supersedes [ADR-23](23-backend-support-levels.md).

## Context

ADR-23 introduced a Zig-style Tier 1–4 ladder over wasm 1.0 + WASI p1, one day before ADR-24 cut the input scope to exactly that surface and set the same bar — spec-green + full WASI p1 — for every 0.1 backend. With the 2.0+/CM badges gone and all backends aiming at one bar, the ladder degenerates: Tiers 1–2 differ only by a list flag and twelve WASI functions, and Tier 3 is just "filesystem not done yet".

## Decision

Retire the ladder. Backends declare capabilities directly — feature support (`Backend::feature_status`) and per-function WASI p1 coverage (`Backend::has_wasi_p1`, derived from runtime units) — and `docs/support.md` renders those declarations flat (features table + WASI p1 table, keeping the in-scope/out-of-scope distinction for the socket surface). E2e coverage is expressed by which suites a backend crate wires up (ADR-27), not by comparing tier numbers. `Tier`, `target_tier`, `achieved_tier`, and the tier conditioning in the e2e case tables are deleted.

Criterion: **a ranking earns its keep only while it discriminates.** When every backend targets the same bar, "which capabilities are done" is the whole truth and a scalar summary of it is noise. Whether some summary scale is worth reintroducing is explicitly deferred until the Python/Go/Java backends exist and show what actually varies.

## Rejected alternatives

- **Keep the ladder** — collapses as described; also forces every new test case to be tier-classified, which ADR-23's own experience showed goes wrong when done by guesswork instead of execution.
- **A numeric coverage score** (e.g. "38/42 WASI functions") — false precision; the per-function table already says exactly this without pretending the functions are fungible.

## Consequences

- Positive: one less taxonomy to keep truthful; new-backend authors read a checklist, not a ladder spec.
- Negative: README/support.md lose a one-glance maturity summary; "production-ready?" now takes reading two tables.
- Carry-over: the ADR-23 lesson that support claims must be verified by execution before being declared survives in the support.md snapshot test and the harness.
