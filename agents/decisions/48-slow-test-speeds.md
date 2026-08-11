# Decision 48 — Two-Speed Slow-Test Classification (slow_test / ultra_slow_test)

Status: **Accepted, 2026-07-29.**
Implemented across the backend crates' cargo features, the `dewasm-test-helper` case macros (`test_speed!`), the spec harness's run conditioning, and the CI workflow's main-branch legs.

## Context

The former single `heavy_test` feature marked every non-fast case, and CI's main-branch run ran them all via `cargo test -- --include-ignored`.
The first such run (issues #22, #23) showed the classification conflates two classes: cases that are slow but CI-affordable (the java/ruby app legs passed in minutes), and cases that individually exceed about a minute on a 4-core runner — the bash QuickJS REPL pty case (no prompt within 180 s) and go's giant-generated-program builds, whose parallel `go build`s exhausted runner memory and got the job killed.
Verifying those on every push costs more than it returns, but the fail-loud policy ([decision 15](15-tests-fail-not-skip.md)) rules out anything that silently skips.
"Heavy" was also the wrong word: the test has always been about wall time.

## Decision

Two explicit speed categories, named for what they test:

- `slow_test` (renamed from `heavy_test`): cases CI verifies on every push to main.
  The feature also un-ignores the spec harness's non-curated run, so the CI switch from `--include-ignored` to `--features slow_test` drops nothing unintended.
- `ultra_slow_test = ["slow_test"]`: cases CI deliberately does not run.
  **Criterion: roughly one minute per test on a CI runner** (observed, not estimated — a case is promoted on evidence from a real run).
  The speed category is chosen per callsite in each backend's `tests/e2e.rs`, so the same case can be ultra for go (compiling a CPython-sized program) and slow for java.

Local speeds: `cargo test` (fast test) → `--features slow_test` (what CI's main run runs) → `--features ultra_slow_test` (everything); `-- --include-ignored` remains the feature-independent way to run everything.
The ultra-slow category is thereby *locally* verified — run it before declaring support or tagging a release — never silently skipped: the cases stay compiled (clippy runs `--all-features`) and visibly `ignored` in default output.

## Rejected alternatives

- **Keep `--include-ignored` and buy bigger runners.**
  Pays continuously for cases that change rarely; the go memory exhaustion would need the largest runners for a handful of tests.
- **Runtime time-budget skipping** (skip when a case exceeds N seconds).
  Nondeterministic pass/skip is exactly what decision 15 forbids; a category flip in a reviewed diff is auditable, a runtime skip is not.
- **A single category with per-case CI excludes in the workflow.**
  Scatters the test-selection policy into YAML `--skip` lists that nothing type-checks; the feature keeps the policy next to the case and greppable.
- **Keeping the `heavy` name.**
  The test has never measured memory or size — only time.
  Historical decisions keep the old name; living docs and code use `slow`.

## Consequences

- CI main legs run `--features <backend>/slow_test` (the core job: `dewasm-cli/slow_test,dewasm-test-helper/wasmtime_test`); #22 and #23 are resolved by reclassification rather than tuning timeouts and thread counts.
- The ultra-slow category's coverage now depends on the local pre-release habit the criterion implies; the category list is small (bash: 1 pty case, go: 8 builds) and each entry cites the evidence that promoted it.
- A future case is promoted (or demoted) by editing one callsite and citing a CI run, mirroring the evidence-driven criterion of [decision 47](47-ruby-f64-sub-quiet-guard.md).
  The DOOM framebuffer snapshot under Bash later joined the ultra-slow category this way ([decision 53](53-doom-frame-snapshot.md)).
