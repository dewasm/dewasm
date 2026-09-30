# Decision 27: Shared Test-Helper Crate with Per-Feature Test Macros

Status: **Accepted, 2026-07-25.**
`dewasm-test-helper` holds the two-layer `BackendUnderTest` / `SpecBackend` traits.
It also holds the shared case data and the per-case test macros.
Each backend crate owns its spec, WASI, and e2e suites.
`dewasm-cli` keeps only the tests that need every backend.
wasmtime is itself wired as a `BackendUnderTest` (`crates/dewasm-test-helper/tests/apps_wasmtime.rs`).
The snapshot-freshness checks then run through the same shared runners.
Builds on [decision 3](3-testing-strategy.md) (the spec harness binds).
It also builds on [decision 8](8-latest-testsuite-support-matrix.md).
That decision gives skip attribution and per-file expected-failure lists.
It builds on [decision 15](15-tests-fail-not-skip.md) too.

## Context

Nearly all tests lived in the CLI crate, the one crate that depends on every backend.
A backend's conformance suite was not in its own crate.
Adding a backend meant editing the CLI's test tree.
Two abstractions had grown there, and they overlapped without composing.
They were `SpecLang` with its script-phrasing `emit_*` surface, and the thinner `E2eLang`.
With three more backends planned, the shape had to be fixed first.

## Decision

`dewasm-test-helper` depends only on `dewasm-core` + `dewasm-backend`, never on a concrete backend crate.
Each backend crate takes it as a dev-dependency.

- **Placement.**
  A test lives with the one backend it exercises.
  Only a test that needs every backend lives centrally.
  Those are the `docs/support.md` rendering check and the CLI-flag suites.
- **Two layers.**
  `BackendUnderTest` is `name` / `backend` / `run(source, args, stdin)`.
  `run` defaults to "write a temp file, exec an interpreter", and compiled targets override it outright.
  `SpecBackend` adds the script-phrasing surface, the per-file failure list, and the curated-file list.
  Criterion: **a backend must be able to run app/e2e suites before it can phrase spec assertions**.
  That is the "cowsay first" bring-up path of [decision 24](24-01-scope-reset.md).
- **Case content is shared; glue is not.**
  Fixtures, expectations, and run/assert logic are `pub const` cases plus runners in the helper crate.
  WASI cases are grouped per feature unit (stdio, args/env, clock/random, filesystem).
  They form the project's own WASI p1 conformance suite.
  That matters because WASI has no official one.
  A backend supplies per-language glue plus the hooks the helper cannot write for it.
  The helper cannot write them since it may not depend on a backend crate.
  The hooks are `convert_app`, `run`/`run_bytes`, `run_app_fs`, `run_in_dir`, `compose_modules`, `pty_command`.
- **Glue is a named `&str` constant, and every case has its own macro.**
  Each per-case macro expands to one `#[test] fn <case>()` calling that case's shared runner.
  So a test name is a case name, and a callsite shows exactly one case with exactly one glue.
  Static values (class name, argv, env, guest preopen paths) are written literally inside the glue.
  A glue cannot know two values statically: a fresh scratch directory and the app-cache root.
  They arrive through `glue::fill`'s `{scratch}` / `{cache}` / `{guest}` / `{host}` placeholders.
  Aggregate macros survive only where there is no per-case glue to show.
  Those are `spec_suite!`, `wasi_suite!`, `gzip_e2e!`, `apps_convert_suite!`, `wasi_testsuite_suite!`.
- **A backend's `tests/e2e.rs` holds only the impl, the glue constants, and macro invocations.**
  No backend-specific `#[test]` exists.
  So a scenario written for one backend is by construction offered to every backend.
  **Capability is declared by which macros a backend invokes**.
  A case it cannot run is simply not invoked.
  The reason is a comment at the absent callsite.
  The measurements behind it are in `agents/apps-audit.md`.
  This replaces the retired support-level conditioning ([decision 25](25-retire-support-levels.md)).
- **The spec harness is one [libtest-mimic](https://crates.io/crates/libtest-mimic) trial per `.wast` file**.
  So selection is cargo's own UX: the file stem names the trial (`cargo test --test spec i32`).
  Files outside `SpecBackend::curated_files` are `#[ignore]`d.
  So `cargo test` runs the curated set and `--include-ignored` the whole testsuite.
  Trials run in parallel, so per-file state belongs to the trial, not to the shared backend object.
  Decision 8's expected-failure and skip-attribution checks run inside each trial.
  This matches the old global checks because the global set is the union of the per-file ones.
- **Speed conditioning is a cargo feature, not an environment variable.**
  A slow case's macro expands its `#[test]` with `#[cfg_attr(not(feature = "slow_test"), ignore = …)]`.
  The runner stays unconditional.
  So `cargo test` skips it visibly, and `--features slow_test` runs one crate's cases.
  `-- --include-ignored` runs everything.
  There is no variable to keep in sync with the code it conditions.
  The categories themselves are [decision 48](48-slow-test-speeds.md).
- **Snapshot tests are compare-only.**
  Regeneration is an explicit command: `cargo xtask update-support-docs`, `cargo xtask update-snapshots`.
  [Decision 56](56-unified-snapshot-regeneration.md) records these commands.
  So a wrong capture cannot overwrite the reference that was meant to catch it.

## Rejected alternatives

- **Keep tests in the CLI crate**: a new backend's conformance would again be someone else's test tree.
  That crate is a 100-line binary, not the project's test host.
- **One flat trait**: forces a pre-spec backend to stub a dozen `emit_*` methods.
  It must do so before its first cowsay run.
- **test-helper depends on the backend crates**: inverts the dependency.
  Backends could then no longer dev-depend on it.
- **A dedicated conformance crate for the cross-backend tests**: an extra crate for two tests.
  It is the escape hatch if a third appears.
- **Backend-specific `#[test]`s for "language-only" scenarios**.
  Those were provider objects, embedded coexistence, and shared tables.
  They were also the sqlite3 C-API drives and the CPython/CRuby demos.
  Allowed at first and withdrawn: each turned out to be a shared case plus glue.
  Sharing them is what turned single-backend coverage into cross-backend coverage.
- **Glue from a resolver function, or `(case name, glue)` pair lists iterated by one macro per table.**
  Tried and reverted: the case name appeared twice.
  A missing entry became a run-time `panic!("no glue")` instead of an absence.
  One macro fanning out over a table hid which cases a backend actually ran.
- **Per-case `exclude: &[(lang, reason)]` fields in the shared data**: same reason.
  Non-invocation states the fact where the reader is already looking.
- **Environment variables for selection and speed** (`DEWASM_SPEC`, `DEWASM_SPEC_ALL`, `DEWASM_APPS_ALL`).
  Cargo's own filter/ignore UX and cargo features need no separate documentation.
  They cannot drift from what they condition.

## Consequences

- Backend bring-up is "implement the base trait, write glue constants, invoke macros".
  Suites stay byte-identical across backends by construction.
- Macro indirection costs greppability of the runner body.
  Each generated `#[test]` is still named for its case.
  Fixture paths must resolve from each consuming crate.
  Every consumer sits at `crates/<x>/`, which keeps `../../` valid.
- Decision 8's expected-failure lists and attribution tags live with each backend's `SpecBackend` impl.
  That is their natural home.
- `docs/support.md` carries no "spec testsuite list" section.
  It came from the retired [decision 23](23-backend-support-levels.md).
  Every backend reported the same value, so it gave a reader nothing to act on.
