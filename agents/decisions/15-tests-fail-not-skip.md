# Decision 15: Tests Fail Loud on Missing Environment, Never Skip

Status: **Accepted, 2026-07-23.**
Implemented across:

- `crates/dewasm-cli/tests/{e2e,spec}/`;
- `crates/dewasm-backend-bash/tests/softfloat.rs`;
- `docs/testing.md`, the setup reference these failures point to.

The `apps` e2e cases additionally dropped their `wasmtime` dependency entirely.
They live in `crates/dewasm-test-helper/src/apps.rs`.
They use snapshot files instead, captured once and checked into `examples/apps/snapshots/`.
An opt-in wasmtime freshness test re-validates those files against a live `wasmtime` on demand.
It runs under the `wasmtime_test` Cargo feature and is `#[ignore]`d otherwise.

## Context

Every test needing `ruby`, `bash >= 5`, or `wasmtime` self-skipped when the tool was missing.
The check was `if find_ruby().is_none() { eprintln!("..."); return; }`.
It was repeated at nearly every call site.
`AGENTS.md` already flagged the consequence.
A passing run without those tools proves less than it looks.
The problem is a contributor (or CI runner) with a broken or incomplete environment.
They see `cargo test` pass and reasonably conclude the code works.
In fact, nothing ran.
A missing interpreter does not justify a wasm-to-source transpiler's tests reporting success.
The interpreter is exactly the environment those tests exist to exercise.

## Decision

**A missing required tool fails the test it's required by; it does not skip it.**
`find_ruby()`/`find_bash5()` are the backend-crate detection functions.
Decision 14's `find_ruby` mirrors the pre-existing `find_bash5`.
Both keep returning `Option<PathBuf>`, since that contract is still useful to non-test callers.
But every test call site now does this instead of checking `is_none()` and returning early:

```rust
find_ruby().expect("ruby not found on PATH (or $DEWASM_RUBY): see docs/testing.md")
```

The spec harness's `SpecLang::interpreter()` changed shape to match.
It returns `PathBuf` directly (not `Option<PathBuf>`).
It panics internally, since `run_suite` has no legitimate "interpreter absent" path left to handle.
The `tests/spec` submodule check follows the same rule (`assert!` instead of an `eprintln!` + return).
`docs/testing.md` is the single place that documents what a full `cargo test` run actually requires.
So every panic message has one canonical place to point to.
It need not restate setup instructions inline.

**The `apps` cases stop depending on `wasmtime` altogether.**
The alternative was adding it to the now-mandatory tool list.
Their historical role for `wasmtime` was purely as a comparison oracle.
They ran the same wasm binary both ways and diffed stdout and exit code.
For a pinned binary and fixed input, that comparison's *result* doesn't change from run to run.
So it can be captured once and checked in as `examples/apps/snapshots/<case>.stdout`.
An actual `wasmtime run` generates the file.
`docs/testing.md` documents that for whoever needs to regenerate one after re-pinning an app version.
The test compares it with `include_str!` at compile time.
This is strictly better than adding `wasmtime` to the required-tools list.
It means one fewer install requirement.
The tests still catch the exact regressions they did before.
Those are generated-language output silently diverging from the real runtime's.

Populating `examples/apps/cache/` (via `examples/apps/setup.sh`, decision 9) remains required.
It is a fail-loud precondition for the `apps` cases.
That one is a real, currently-necessary setup step.
The binaries are real, copyrighted, third-party artifacts.
Decision 9 deliberately keeps them out of git.
The step is not a convenience this decision is trying to remove.

**A snapshot file can itself go stale**, so there is one more test.
It is the wasmtime freshness suite (`crates/dewasm-test-helper/tests/apps_wasmtime.rs`).
It runs every case through a live `wasmtime run` and diffs it against the checked-in snapshot file.
It is independent of the snapshot files the always-on tests trust, and a check *on* them.
This one genuinely is optional.
It audits the fixtures; it doesn't test dewasmify's own correctness.
So it's the one place `#[ignore]` is the *right* tool rather than the rejected one above.
The attribute is `#[cfg_attr(not(feature = "wasmtime_test"), ignore)]`.
It keeps the suite out of a plain `cargo test`, so there is no new required tool.
The suite becomes a normal, non-ignored test the moment `--features wasmtime_test` is passed.
No `--include-ignored` is needed.
There is no separate skip-detection branch to keep in sync with the rest of this decision's policy.

## Rejected alternatives

- **Keep skipping, but print more loudly**: doesn't fix the actual problem.
  That problem is a passing `cargo test` that ran nothing.
  It only makes the log noisier.
- **`#[ignore = "reason"]`** for the *required*-tool tests.
  It is Rust's built-in "don't run this by default" mechanism, visible as `ignored` rather than `ok`.
  It is closer to honest than silent-pass-via-skip.
  But it still reports overall success without running the test.
  That is wrong for `ruby`/`bash`/the apps cache, which this decision treats as genuinely required.
  (It's the *right* tool for the wasmtime snapshot-file check below.
  That is precisely because that one is genuinely optional.)
- **Keep `wasmtime` but add it to the required-tools list.**
  It was considered as the straightforward way to make `apps` consistent with the new policy.
  But it adds a real install requirement for a role (comparison oracle).
  A checked-in snapshot file fills that role just as well.
  So removing the dependency outright is strictly better than requiring it.

## Consequences

- Positive: a passing `cargo test` now means what it says.
  Consider CI or a contributor missing `ruby`/`bash >= 5`/the spec submodule/the apps cache.
  They get an immediate, actionable failure instead of a quietly empty pass.
- Positive: the `apps` e2e cases no longer need `wasmtime` installed at all.
  Only maintainers regenerating a snapshot file need it.
- Negative / carry-over: snapshot files can go stale relative to a re-pinned app version.
  A new `setup.sh` URL bump needs a matching snapshot-file regeneration.
  `docs/testing.md` documents the regeneration.
  The old live-diff design always compared against whatever binary was currently cached.
  So it couldn't go stale this way.
  Mitigated, not eliminated.
  The wasmtime freshness suite (`--features wasmtime_test`) catches it on demand.
  But nothing runs that check automatically on every `setup.sh` pin bump.
- This decision is a policy every *future* test must also follow.
  A new test needing an external tool fails loud (`.expect(...)`).
  It does not add another silent-skip call site.
