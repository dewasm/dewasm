# ADR-27 — Shared Test-Helper Crate with Per-Feature Test Macros

Status: **Accepted, 2026-07-25; landed 2026-07-26.** The
`dewasm-test-helper` crate and the two-layer `BackendUnderTest` /
`SpecBackend` traits, shared case tables, and per-feature macros
(`spec_suite!`, `standalone_e2e!`, `library_e2e!`, `wasi_suite!`,
`apps_e2e!`) are in place; each backend crate owns its spec and e2e
suites, and the cross-backend tests remain outside the backend crates:
`support_docs` and the `wasmtime --dir` filesystem golden check
(`apps_golden_fs_matches_wasmtime`) in `dewasm-cli`, with wasmtime itself
wired as a `BackendUnderTest` (`apps_wasmtime`) running the non-fs golden
checks through the shared runners. Builds on
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
