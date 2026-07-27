# ADR-27 — Shared Test-Helper Crate with Per-Feature Test Macros

Status: **Accepted, 2026-07-25; landed 2026-07-26.** The
`dewasm-test-helper` crate and the two-layer `BackendUnderTest` /
`SpecBackend` traits, shared case tables, and per-feature macros
(`spec_suite!`, `standalone_e2e!`, `library_e2e!`, `wasi_suite!`,
`apps_e2e!`, `gzip_e2e!`, `fs_apps_e2e!`) are in place; each backend crate
owns its spec and e2e suites, `dewasm-cli` keeps only the `support_docs`
cross-backend gate, and wasmtime is itself wired as a `BackendUnderTest`
(`apps_wasmtime`) running the `apps`/`gzip`/`fs_apps` golden-freshness
checks through the same shared runners. Builds on
[ADR-3](3-testing-strategy.md) (the spec harness binds),
[ADR-8](8-latest-testsuite-support-matrix.md) (skip attribution and
per-file failure ledgers), and [ADR-15](15-tests-fail-not-skip.md).

## Context

Nearly all tests live in the CLI crate — spec harness, e2e suites, app
golden tests — because it is the one crate that depends on every
backend. That placement means a backend's conformance suite is not in
the backend's crate, adding a backend means editing the CLI's test
tree, and the two ad-hoc abstractions that grew there (`SpecLang` with
its script-phrasing `emit_*` surface; the thinner `E2eLang`) overlap
without composing. With three more backends planned, the shape must be
fixed first.

## Decision

Create **`dewasm-test-helper`**: a crate depending only on
`dewasm-core` + `dewasm-backend` (never on concrete backend crates),
used by each backend crate as a dev-dependency. It provides:

- **A two-layer backend-under-test abstraction.** The base trait
  (`BackendUnderTest`) is `name` / `backend` / `run(source, args,
  stdin)`; `run`'s default implementation writes a temp file and execs
  an interpreter, and compiled targets (Go, Java) override `run`
  itself. The spec layer (`SpecBackend: BackendUnderTest`) carries the
  script-phrasing surface, the per-file failure ledger, and the
  curated-file list. Criterion for the split: **a backend must be able
  to run app/e2e suites before it can phrase spec assertions** — the
  "cowsay first" bring-up path of ADR-24 implements the base layer
  only.
- **Shared case tables without glue** (standalone, library, WASI,
  apps). WASI cases are grouped per feature unit — stdio, args/env,
  clock/random, filesystem — forming the project's own WASI p1
  conformance suite, which matters because WASI has no official one.
  Language-specific glue stays in the backend crate and is passed to
  the runners.
- **Per-feature test macros** (`spec_suite!`, `standalone_e2e!`,
  `library_e2e!`, `wasi_suite!`, `apps_e2e!`) that expand to `#[test]`
  functions iterating the shared tables, so a backend crate's
  `tests/` declares exactly which suites it participates in — this
  wiring replaces the retired tier gating (ADR-25).

Backend-specific tests (units lint, softfloat oracle, Ruby-only
scenarios) live in their backend crate as today. Tests that inherently
need every backend — the `docs/support.md` golden gate and the
golden-vs-wasmtime app check — stay in the CLI crate, which already
depends on all backends. Rule: **a test lives with the one backend it
exercises; only a test that needs every backend may live centrally.**

## Rejected alternatives

- **Keep tests in the CLI crate** — a new backend's conformance would
  again be someone else's test tree; the CLI crate is a 100-line
  binary, not the project's test host.
- **One flat trait** — forces a pre-spec backend to stub a dozen
  `emit_*` methods before its first cowsay run.
- **test-helper depends on the backend crates** — inverts the
  dependency so backends could no longer dev-depend on the helper.
- **A dedicated conformance crate for the cross-backend tests** — an
  extra crate for two tests; noted as the escape hatch if a third
  appears.

## Consequences

- Positive: backend bring-up is "implement the base trait, invoke
  macros"; suites stay byte-identical across backends by construction.
- Negative: macro indirection makes individual test names less
  greppable; fixture paths must resolve from each consuming crate
  (same `crates/<x>/` depth keeps `../../` valid).
- The ADR-8 ledgers and attribution tags move with each backend's
  `SpecBackend` impl — their natural home — and keep their semantics.

## Revision (2026-07-27): backend e2e is impl + glue + macros only

The original decision let "Ruby-only scenarios" (provider objects, embedded
coexistence, cross-module table sharing, the sqlite3 C-API drive, WASI-model
internals, and the CPython/CRuby runtime demos) stay as hand-written `#[test]`
functions in the Ruby crate. That carve-out is **withdrawn**. The rule is now
uniform:

- **A backend crate's `tests/e2e.rs` contains only** the `BackendUnderTest`
  impl, glue strings / glue-producing functions, and macro invocations. No
  backend-specific `#[test]` function exists.
- **Case content is always shared.** Every scenario's fixtures, expectations,
  and run/assert logic live in a `dewasm-test-helper` table + runner. What a
  backend supplies is per-language glue (and, for multi-module cases, a
  `BackendUnderTest::compose_modules` implementation using its own crate's API —
  the helper crate still may not depend on a concrete backend, so composition
  is a trait hook, not shared code).
- **Capability is declared by which macros a backend invokes**, refined by
  **explicit data-level exclusions with reasons** where a macro is invoked but a
  particular case is not expressible or not practical. Each shared table carries
  an `exclude: &[(lang, reason)]` (or, for the fixed-shape multi-module/library
  cases, a per-case exclusion list) that the runner prints and honors, instead
  of a bespoke skipped test. Examples now in the tables: Go/Java/Bash excluded
  from the provider-object and lazy-`@wasi` cases (eager WASI, no provider
  object); Go/Bash excluded from in-memory stdio capture (no injectable stream);
  the CPython/CRuby runtime demos excluded on Java (a generated method overflows
  the JVM 64 KB bytecode limit — `code too large`) and CRuby also on Go (its
  ~242 MB Go source exceeds the ADR-24 ~5-minute `go build` bar). These are
  measured, not guessed (docs/apps-audit.md records the numbers).

New shared tables/macros this introduced: `capi_apps_e2e!` (`CAPI_CASES`, the
sqlite3 C-API drives — no wasmtime golden is possible, so each pins a fixed
string), `multi_module_e2e!` (`MULTI_MODULE_CASES`, the shared-table and
embedded-coexistence scenarios, via `compose_modules`), plus new rows in
`LIBRARY_CASES` (provider/override/stdio), `WASI_CASES` (root-preopen
containment), and `FS_APP_CASES` (the cache-preopened CPython/CRuby demos, a new
`cache_preopens` field mounting stdlib trees straight from the app cache instead
of copying them). The consequence is that a scenario added for one backend is by
construction offered to every backend, green where the capability holds and
documented where it does not.
