# Decision 48: Two-Speed Slow-Test Classification (`slow_test` / `ultra_slow_test`)

Status: **Accepted, 2026-07-29.**
Implemented across four places:

- the backend crates' Cargo features;
- the `dewasm-test-helper` case macros (`test_speed!`);
- the specification harness's run conditioning;
- the CI workflow's main-branch legs.

## Context

The former single `heavy_test` feature marked every non-fast case.
CI's main-branch run ran them all via `cargo test -- --include-ignored`.
The first such run (issues #22, #23) showed the classification mixes two classes:

- cases that are slow but CI-affordable (the Java and Ruby app legs passed in minutes);
- cases that individually exceed about a minute on a 4-core runner.

The second class is the Bash QuickJS REPL PTY case (no prompt within 180 s).
It also covers Go's builds of giant generated programs.
The parallel `go build`s exhausted runner memory and got the job killed.
Verifying those on every push costs more than it returns.
But the fail-loud policy ([decision 15](15-tests-fail-not-skip.md)) rules out anything that silently skips.
"Heavy" was also the wrong word: the test has always been about wall time.

## Decision

Two explicit speed categories, named for what they test:

- `slow_test` (renamed from `heavy_test`): cases CI verifies on every push to main.
  The feature also un-ignores the specification harness's full testsuite run.
  So the CI switch from `--include-ignored` to `--features slow_test` drops nothing unintended.
- `ultra_slow_test = ["slow_test"]`: cases CI deliberately does not run.
  **Criterion: roughly one minute per test, measured locally.**
  It is observed, not estimated: a case is promoted on evidence from a real run.
  The speed category is chosen per callsite in each backend's `tests/e2e.rs`.
  So the same case can be `ultra` for Go (compiling a CPython-sized program) and `slow` for Java.

Local speeds:

| Command | Runs |
| --- | --- |
| `cargo test` | fast test |
| `--features slow_test` | what CI's main run runs |
| `--features ultra_slow_test` | everything |

`-- --include-ignored` remains the feature-independent way to run everything.
The `ultra` category is thereby *locally* verified, never silently skipped.
It is run before declaring support or tagging a release.
The cases stay compiled (Clippy runs `--all-features`) and visibly `ignored` in default output.

## Rejected alternatives

- **Keep `--include-ignored` and buy bigger runners.**
  It pays continuously for cases that change rarely.
  The Go memory exhaustion would need the largest runners for a few tests.
- **Runtime time-budget skipping** (skip when a case exceeds N seconds).
  Nondeterministic pass/skip is exactly what decision 15 rules out.
  A category flip in a reviewed diff is auditable; a runtime skip is not.
- **A single category with per-case CI excludes in the workflow.**
  It scatters the test-selection policy into YAML `--skip` lists that nothing type-checks.
  The feature keeps the policy next to the case, where `grep` finds it.
- **Keeping the `heavy` name.**
  The test has never measured memory or size, only time.
  Historical decisions keep the old name; current documents and code use `slow`.

## Consequences

- CI main legs run `--features <backend>/slow_test`.
  The core job runs `dewasm-cli/slow_test,dewasm-test-helper/wasmtime_test`.
  #22 and #23 are resolved by reclassification rather than tuning time limits and thread counts.
- The `ultra` category's coverage now depends on the local pre-release habit the criterion implies.
  The category list is small (Bash: 1 PTY case, Go: 8 builds).
  Each entry cites the evidence that promoted it.
- A future case moves to a slower or faster category by editing one callsite and citing a CI run.
  This mirrors the evidence-driven criterion of [decision 47](47-ruby-f64-sub-quiet-guard.md).
  The DOOM framebuffer snapshot under Bash later joined the `ultra` category this way.
  That is [decision 53](53-doom-frame-snapshot.md).
