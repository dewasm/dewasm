# Decision 27 — Shared Test-Helper Crate with Per-Feature Test Macros

Status: **Accepted, 2026-07-25.**
`dewasm-test-helper` holds the two-layer `BackendUnderTest` / `SpecBackend` traits, the shared case data, and the per-case test macros; each backend crate owns its spec, WASI, and e2e suites; `dewasm-cli` keeps only the tests that need every backend; and wasmtime is itself wired as a `BackendUnderTest` (`crates/dewasm-test-helper/tests/apps_wasmtime.rs`) so the snapshot-freshness checks run through the same shared runners.
Builds on [decision 3](3-testing-strategy.md) (the spec harness binds), [decision 8](8-latest-testsuite-support-matrix.md) (skip attribution, per-file expected-failure lists), and [decision 15](15-tests-fail-not-skip.md).

## Context

Nearly all tests lived in the CLI crate, the one crate that depends on every backend: a backend's conformance suite was not in its own crate, adding a backend meant editing the CLI's test tree, and the two abstractions that had grown there (`SpecLang` with its script-phrasing `emit_*` surface, the thinner `E2eLang`) overlapped without composing.
With three more backends planned, the shape had to be fixed first.

## Decision

`dewasm-test-helper` depends only on `dewasm-core` + `dewasm-backend`, never on a concrete backend crate, and is taken by each backend crate as a dev-dependency.

- **Placement.**
  A test lives with the one backend it exercises; only a test that needs every backend lives centrally (the `docs/support.md` rendering check, the CLI-flag suites).
- **Two layers.**
  `BackendUnderTest` is `name` / `backend` / `run(source, args, stdin)`, with `run` defaulting to "write a temp file, exec an interpreter" and overridden outright by compiled targets; `SpecBackend` adds the script-phrasing surface, the per-file failure list, and the curated-file list.
  Criterion: **a backend must be able to run app/e2e suites before it can phrase spec assertions**, which is the "cowsay first" bring-up path of [decision 24](24-01-scope-reset.md).
- **Case content is shared; glue is not.**
  Fixtures, expectations, and run/assert logic are `pub const` cases plus runners in the helper crate.
  WASI cases are grouped per feature unit (stdio, args/env, clock/random, filesystem), forming the project's own WASI p1 conformance suite, which matters because WASI has no official one.
  A backend supplies per-language glue plus the hooks the helper cannot write for it (`convert_app`, `run`/`run_bytes`, `run_app_fs`, `run_in_dir`, `compose_modules`, `pty_command`), since it may not depend on a backend crate.
- **Glue is a named `&str` constant, and every case has its own macro.**
  Each per-case macro expands to one `#[test] fn <case>()` calling that case's shared runner, so a test name is a case name and a callsite shows exactly one case with exactly one glue.
  Static values (class name, argv, env, guest preopen paths) are written literally inside the glue; the two values a glue cannot know statically, a fresh scratch directory and the app-cache root, arrive through `glue::fill`'s `{scratch}` / `{cache}` / `{guest}` / `{host}` placeholders.
  Aggregate macros survive only where there is no per-case glue to show (`spec_suite!`, `wasi_suite!`, `gzip_e2e!`, `apps_convert_suite!`, `wasi_testsuite_suite!`).
- **A backend's `tests/e2e.rs` holds only the impl, the glue constants, and macro invocations.**
  No backend-specific `#[test]` exists, so a scenario written for one backend is by construction offered to every backend.
  **Capability is declared by which macros a backend invokes**; a case it cannot run is simply not invoked, with the reason as a comment at the absent callsite and the measurements behind it in `docs/apps-audit.md`.
  This replaces the retired support-level conditioning ([decision 25](25-retire-support-levels.md)).
- **The spec harness is one [libtest-mimic](https://crates.io/crates/libtest-mimic) trial per `.wast` file**, so selection is cargo's own UX: the file stem names the trial (`cargo test --test spec i32`), and files outside `SpecBackend::curated_files` are `#[ignore]`d, so `cargo test` runs the curated set and `--include-ignored` the whole testsuite.
  Trials run in parallel, so per-file state belongs to the trial, not to the shared backend object.
  Decision 8's expected-failure and skip-attribution checks run inside each trial, equivalent to the old global checks because the global set is the union of the per-file ones.
- **Speed conditioning is a cargo feature, not an environment variable.**
  A slow case's macro expands its `#[test]` with `#[cfg_attr(not(feature = "slow_test"), ignore = …)]` while the runner stays unconditional, so `cargo test` skips it visibly, `--features slow_test` runs one crate's cases, and `-- --include-ignored` runs everything, with no variable to keep in sync with the code it conditions.
  The categories themselves are [decision 48](48-slow-test-speeds.md).
- **Snapshot tests are compare-only.**
  Regeneration is an explicit command (`cargo xtask update-support-docs`, `cargo xtask update-snapshots`, [decision 56](56-unified-snapshot-regeneration.md)), so a wrong capture cannot overwrite the reference that was meant to catch it.

## Rejected alternatives

- **Keep tests in the CLI crate**: a new backend's conformance would again be someone else's test tree; that crate is a 100-line binary, not the project's test host.
- **One flat trait**: forces a pre-spec backend to stub a dozen `emit_*` methods before its first cowsay run.
- **test-helper depends on the backend crates**: inverts the dependency, so backends could no longer dev-depend on it.
- **A dedicated conformance crate for the cross-backend tests**: an extra crate for two tests; the escape hatch if a third appears.
- **Backend-specific `#[test]`s for "language-only" scenarios** (provider objects, embedded coexistence, shared tables, the sqlite3 C-API drives, the CPython/CRuby demos).
  Allowed at first and withdrawn: each turned out to be a shared case plus glue, and sharing them is what turned single-backend coverage into cross-backend coverage.
- **Glue from a resolver function, or `(case name, glue)` pair lists iterated by one macro per table.**
  Tried and reverted: the case name appeared twice, a missing entry became a run-time `panic!("no glue")` instead of an absence, and one macro fanning out over a table hid which cases a backend actually ran.
- **Per-case `exclude: &[(lang, reason)]` fields in the shared data**: same reason; non-invocation states the fact where the reader is already looking.
- **Environment variables for selection and speed** (`DEWASM_SPEC`, `DEWASM_SPEC_ALL`, `DEWASM_APPS_ALL`): cargo's own filter/ignore UX and cargo features need no separate documentation and cannot drift from what they condition.

## Consequences

- Backend bring-up is "implement the base trait, write glue constants, invoke macros"; suites stay byte-identical across backends by construction.
- Macro indirection costs greppability of the runner body, though each generated `#[test]` is named for its case.
  Fixture paths must resolve from each consuming crate; every consumer sits at `crates/<x>/`, which keeps `../../` valid.
- Decision 8's expected-failure lists and attribution tags live with each backend's `SpecBackend` impl, their natural home.
- `docs/support.md` carries no "spec testsuite list" section (it came from the retired [decision 23](23-backend-support-levels.md)): every backend reported the same value, so it gave a reader nothing to act on.
