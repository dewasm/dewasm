# ADR-48 — Two-Tier Slow-Test Gating (slow_test / ultra_slow_test)

Status: **Accepted, 2026-07-29.** Implemented across the backend crates'
cargo features, the `dewasm-test-helper` case macros (`slow_tier_test!`), the
spec harness's sweep gating, and the CI workflow's main-branch legs.

## Context

The former single `heavy_test` feature marked every non-fast case, and CI's
main-branch sweep ran them all via `cargo test -- --include-ignored`. The
first such run (issues #22, #23) showed the tier conflates two classes: cases
that are slow but CI-affordable (the java/ruby app legs passed in minutes),
and cases that individually exceed about a minute on a 4-core runner — the
bash QuickJS REPL pty case (no prompt within 180 s) and go's
giant-generated-program builds, whose parallel `go build`s exhausted runner
memory and got the job killed. Verifying those on every push costs more than
it returns, but the fail-loud policy ([ADR-15](15-tests-fail-not-skip.md))
rules out anything that silently skips. "Heavy" was also the wrong word: the
gate has always been about wall time.

## Decision

Two explicit tiers, named for what they gate:

- `slow_test` (renamed from `heavy_test`): cases CI verifies on every push to
  main. The feature also un-ignores the spec harness's non-curated sweep, so
  the CI switch from `--include-ignored` to `--features slow_test` drops
  nothing unintended.
- `ultra_slow_test = ["slow_test"]`: cases CI deliberately does not run.
  **Criterion: roughly one minute per test on a CI runner** (observed, not
  estimated — a case is promoted on evidence from a real run). The tier is
  chosen per callsite in each backend's `tests/e2e.rs`, so the same case can
  be ultra for go (compiling a CPython-sized program) and slow for java.

Local tiers: `cargo test` (fast gate) → `--features slow_test` (what CI's
main sweep runs) → `--features ultra_slow_test` (everything);
`-- --include-ignored` remains the feature-independent sledgehammer. The
ultra tier is thereby *locally* verified — run it before declaring support or
tagging a release — never silently skipped: the cases stay compiled (clippy
runs `--all-features`) and visibly `ignored` in default output.

## Rejected alternatives

- **Keep `--include-ignored` and buy bigger runners.** Pays continuously for
  cases that change rarely; the go memory exhaustion would need the largest
  runners for a handful of tests.
- **Runtime time-budget skipping** (skip when a case exceeds N seconds).
  Nondeterministic pass/skip is exactly what ADR-15 forbids; a tier flip in
  a reviewed diff is auditable, a runtime skip is not.
- **A single tier with per-case CI excludes in the workflow.** Scatters the
  test-selection policy into YAML `--skip` lists that nothing type-checks;
  the feature keeps the policy next to the case and greppable.
- **Keeping the `heavy` name.** The gate has never measured memory or size —
  only time. Historical ADRs keep the old name; living docs and code use
  `slow`.

## Consequences

- CI main legs run `--features <backend>/slow_test` (the core job:
  `dewasm-cli/slow_test,dewasm-test-helper/wasmtime_test`); #22 and #23 are
  resolved by reclassification rather than tuning timeouts and thread counts.
- The ultra tier's coverage now depends on the local pre-release habit the
  criterion implies; the tier list is small (bash: 1 pty case, go: 8 builds)
  and each entry cites the evidence that promoted it.
- A future case is promoted (or demoted) by editing one callsite and citing a
  CI run, mirroring the evidence-driven criterion of
  [ADR-47](47-ruby-f64-sub-quiet-guard.md).
