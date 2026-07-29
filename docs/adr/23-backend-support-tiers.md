# ADR-23 — Backend Support Tiers, Specialized to Wasm 1.0 + WASI Preview 1

Status: **Superseded by [ADR-25](25-retire-support-tiers.md), 2026-07-26.** Originally accepted 2026-07-24 and implemented as described below; kept here as history of the tier ladder's design and rationale. ADR-25 removes `Tier` and everything derived from it.

## Context

`Feature` (ADR-8) is a flat matrix: 20 rows, each `Supported`/`Partial`/ `Unsupported` per backend. It has no vocabulary for "how far along is this backend overall" — Ruby and Bash's actual standing had to be reconstructed by eyeballing the table. As more backends are added (Java+C# per ADR-10, then Go/Python/PHP), each needs a stated goal a reviewer can check against, in the way Zig's platform support tiers (https://ziglang.org/learn/platform-support/) let a target's status be read off a ladder instead of a feature-by-feature diff.

The natural single-number ladder only holds together if it covers a scope every wasm binary can be measured against. Wasm 2.0+ proposals (reference types, tail calls, GC, SIMD, threads, ...) and the component model / WASI preview 2 do not qualify: adoption is concentrated in the Bytecode Alliance ecosystem and still churning (WASI 0.3 is next), while the common case — what Zig, Go, and most existing wasm binaries actually emit — is wasm 1.0 plus WASI preview 1, a frozen ABI. A ladder built to include CM would either force every future backend toward a speculative target or make Tier 1 a moving one.

## Decision

**The tier ladder covers wasm 1.0 + WASI p1 only**, best-first (`Tier::ALL`, `crates/dewasm-backend/src/tier.rs`):

- **Tier 1 (Full)** — wasm 1.0 handled completely (imported globals/memories/tables + floats all `Supported`) and WASI p1 handled completely short of the out-of-scope surface: `sock_accept/recv/send/ shutdown` and `proc_raise` (wasmtime itself leaves these unimplemented; no toolchain output exercises them). Also requires the spec harness's `EXPECTED_FAILURES` ledger to hold no wasm-1.0- attributable entries (`Backend::wasm10_ledger_clean`; e.g. Ruby's `import-limits` tag, ADR-16, blocks Tier 1 today).
- **Tier 2 (Production)** — wasm 1.0 imports complete, plus the WASI p1 filesystem functions (`path_open` and friends, 14 total). The standard goal for new backends (`Backend::target_tier`'s default): the scope a major toolchain's typical CLI/library output needs.
- **Tier 3 (Core)** — a single wasm 1.0 module with function-only imports, plus the 16-function WASI p1 core (args/environ/clock/fd read-write-seek-close/proc_exit/random/sched_yield). Self-contained CLI tools live here.
- **Tier 4 (Experimental)** — wired into the spec harness (`SpecLang`/`achieved_tier` observes pass/fail/skip) but outside the default gate. The starting point for a new backend.

Requirements are cumulative (Tier 2 implies Tier 3, etc.). Each `Feature`/WASI function names the tier whose requirements include it (`feature_tier`, and the `Option<Tier>` second element of `WASI_PREVIEW1_FUNCTIONS`); this mapping, not a restated table, is the source of truth — `docs/support.md`'s `## Tiers` and per-row `Tier` columns are generated from it.

**Everything outside wasm 1.0 + WASI p1 is an orthogonal extension badge**: the existing `Feature` rows for post-1.0 proposals and the component model (`is_extension`) stay exactly as ADR-8 defined them — per-backend opt-in, never affecting a tier. `MultipleTables`/ `TableBulkOps` are wasm-2.0-adjacent bulk-memory-proposal features and sit on the badge side too, even though `ImportedGlobals/Memories/Tables` (true wasm 1.0) are tier-gated.

`tier_gaps`/`achieved_tier` only check what's statically knowable from a backend's own declarations (feature status, `has_wasi_p1`, the ledger flag); the dynamic half — the spec sweep actually passing, e2e cases actually passing — is enforced by the test suite itself, the same split ADR-8 already draws between declaration and enforcement. `target_tier` is a second, independent declaration (what the backend *aims* for, not what it has reached) so the generated docs can show both.

The shared e2e case tables (`StandaloneCase`/`LibraryCase`/`AppCase`) carry a `tier` field; a case only runs for a language whose `achieved_tier` meets it (`tier_ok`), printing a skip line rather than failing — a declared-tier gap, not an ADR-15 missing-tool failure. Every case in the suite today only needs Tier 3, confirmed by re-running each under Bash before assigning tiers (including `sqlite3-shell`, which looked filesystem-heavy but uses no db-file argument). Component-model e2e is gated on the `ComponentModel` badge directly (`require_component_model_badge` in `component.rs`), not a tier — the precedent for badge-gated cases once one exists.

## Rejected alternatives

- **Include wasm 2.0+/CM in the ladder** (e.g. Tier 1 = "every feature dewasmify tracks, including CM"). Rejected: ties Tier 1 to WASI 0.3 churn and to proposals with genuinely uncertain adoption outside the Bytecode Alliance ecosystem, and would implicitly commit every future backend to a CM port to reach the top tier.
- **A second, orthogonal WASI-support axis** instead of folding WASI p1 into the same ladder as wasm-core features. Rejected: a backend's wasm- core completeness and its WASI completeness move together in practice (Ruby's ADR-16 work touched both), and two axes would need a combination rule to answer "what tier is this backend" anyway — one ladder with WASI folded in is more direct and matches ADR-8's existing practice of listing WASI p1 alongside the `Feature` rows.
- **Ascending, cumulative-milestone numbering** (Tier N ⊇ Tier N-1, 1 = weakest) instead of Zig's descending best-first convention. Rejected per explicit user preference for the Zig framing users may already recognize.

## Consequences

- Positive: `achieved_tier(backend)` and `target_tier(backend)` give a one-line answer to "how far along is this backend" (currently ruby = Tier 2 targeting Tier 1; bash = Tier 3, its settled target — the north-star use case, running self-contained C/Rust CLI tools, is met there). New backends have a concrete, checkable Tier 2 checklist instead of copying Ruby's full feature set.
- Positive: e2e cases self-describe their requirement instead of being hand-gated per language; a future language automatically inherits every case its tier covers.
- Negative / caveats: `wasm10_ledger_clean` is a second place (besides the harness's `EXPECTED_FAILURES`) that must be updated when a ledger entry is cleared — it can drift stale (falsely `false`, which only under-states a tier, never over-states one, so the failure mode is safe but requires remembering to flip it). Tier 1 for Ruby remains unreached: 12 WASI p1 functions (`fd_advise`/`allocate`/`fdstat_set_*`/ `renumber`, `poll_oneoff`, `path_link`/`readlink`/`symlink`, `fd_filestat_set_times`/`path_filestat_set_times`) and the `import-limits` ledger are the remaining gap.

See also: [ADR-8](8-latest-testsuite-support-matrix.md) (the `Feature` matrix and attribution this builds on), [ADR-15](15-tests-fail-not-skip.md) (the fail-loud policy tier skips deliberately don't fall under), [ADR-16](16-ruby-wasm1-completion.md) (the `import-limits` ledger gap blocking Ruby's Tier 1).
