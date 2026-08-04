# ADR-24 — 0.1 Scope Reset: Wasm 1.0 + WASI Preview 1 Only, App-Driven Goals

Status: **Accepted, 2026-07-25.** The feature-audit tool (`crates/dewasm-core/src/bin/feature-audit.rs`) and the excision have landed: reference types, tail calls, exception handling, the component model, and WASI preview 2 are removed from the IR, backends, runtime units, harness, and docs, and their inputs are rejected at conversion time with attributed errors. The support maturity levels' retirement and the rename land next ([ADR-25](25-retire-support-levels.md)). Supersedes [ADR-17](17-ruby-reference-types.md), [ADR-18](18-ruby-tail-calls.md), [ADR-19](19-ruby-exception-handling.md), [ADR-20](20-component-model-core-ir-adapters.md), [ADR-21](21-ruby-wasi-preview2.md); revises the target-language plan of [ADR-10](10-csharp-target.md).

## Context

Right after completing wasm 1.0 for Ruby (ADR-16), the project extended the Ruby backend to reference types, tail calls, exception handling (ADRs 17–19) and the component model + WASI preview 2 (ADRs 20–21) in a single sprint. A retrospective found the project harder to see whole: the input surface had grown faster than the number of backends able to carry it, goals were phrased as spec coverage rather than as programs users can convert, and every IR construct added is a construct all future backends must either lower or reject. Meanwhile the planned 0.1 apps and backends (below) need none of the post-1.0 features.

## Decision

For the 0.1 release, the accepted input is **wasm 1.0 + WASI preview 1**, where "wasm 1.0" means the MVP plus the universally-emitted baseline (sign extension, saturating float-to-int, multi-value, bulk memory, mutable globals) and the ADR-16 completion set (non-function imports, multiple tables, table bulk ops). Everything else — reference types, tail calls, exception handling, the component model, WASI preview 2, and all unimplemented proposals — is **removed from the code**, not frozen: IR variants, lowering, feature tests, runtime units, harness support, and docs rows all go. Unsupported input keeps failing at conversion time with a clear error (ADR-0).

One validator-level nuance, found by the app audit (`docs/apps-audit.md`): LLVM-based toolchains encode `call_indirect` immediates as overlong LEBs when the reference-types *target feature* is on (their default), so real wasip1 binaries — including the already shipping qjs and sqlite3 — only validate with the reference-types feature bit enabled. The bit therefore stays on in `dewasm-core::module::features()` as a pure **encoding relaxation**; every actual reference-types construct (externref, table instructions, `ref.*`, non-zero table indices) is rejected during IR building with the usual attributed error.

The discriminating criterion: **a feature stays only if a pinned target app needs it or every 0.1 backend is expected to implement it.** Code kept "just in case" is code paid for in every exhaustive match, every new backend, and every reader; git history plus ADRs 17–21 (kept as design records) make restoration cheap if the need returns.

Goals are stated app-first; the spec testsuite remains the correctness test (ADR-3), not the goal. 0.1 targets: cowsay (backend bring-up), quickjs-ng (one-shot, script with file I/O, REPL), sqlite3 (shell and C-API library, DB files on disk, callback binding), ripgrep, a CPython or CRuby runtime binary, and a compression CLI. A **feature audit** (conversion-time feature report over each pinned binary) runs before the excision; an app that needs a dropped feature is deferred with a written note in `docs/apps-audit.md` — it does not block the excision. pandoc.wasm is the expected first deferral (GHC's wasm backend is believed to emit tail calls; to be confirmed by the audit).

0.1 backends: Ruby and Bash (existing) plus **Python, Go, Java**. Each new backend's first milestone is "cowsay runs"; its 0.1 bar is spec-green plus full WASI p1 including the filesystem. If the schedule demands, the release may relax to "Python at the full bar; Go/Java at the cowsay milestone" — that call is made at release time, not now.

Future work recorded, deliberately out of 0.1 scope: restoring wasm 2.0+ support, wasix and partial emscripten-runtime import surfaces as possible additional input dialects, and Haskell/OCaml (and C#, per the ADR-10 revision) as later target languages.

## Rejected alternatives

- **Freeze instead of delete** (keep the code, restrict new work to 1.0) — dormant variants still appear in every exhaustive match, every harness tag, and the support matrix; the visible surface is what made the project hard to see. Deletion is the only option that shrinks it.
- **Keep Ruby's 2.0+ support as badges** — it is tested, working code, and removing it costs real churn. Lost to the criterion above: no pinned app needs it, no other 0.1 backend will implement it, and a five-backend project whose backends accept different inputs reintroduces exactly the asymmetry ADR-23 tried to manage.
- **Spec-coverage goals** — coverage numbers do not answer "what can I convert?"; apps do, and each app pins an exact WASI surface to implement. The spec suite stays as the test underneath.

## Consequences

- Positive: one input dialect shared by all five 0.1 backends; the IR a new backend must lower shrinks; goals become demos a user can run.
- Negative: working, spec-green Ruby code for ADRs 17–21 is deleted (~4k LOC + 37 runtime units); pandoc is likely deferred; users with 2.0+ binaries are turned away at conversion time.
- Carry-over: ADRs 17–21 stay in the tree as Superseded design records for a future restoration; the excision commit flips their statuses.
- The support maturity levels lose their reason to exist once every backend targets the same bar — retired separately in [ADR-25](25-retire-support-levels.md).
