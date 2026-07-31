# ADR-54 — Convert-Only App Smokes, Gated One Tier Down

Status: **Accepted, 2026-08-01.** Implemented as `convert_smoke_test!` and the `run_*_convert` runners in `crates/dewasm-test-helper/` (every slow/ultra per-case app macro emits one), documented in `docs/testing.md`. Supersedes [ADR-53](53-doom-frame-golden.md)'s rejection of a conversion smoke for DOOM.

## Context

[ADR-48](48-slow-test-tiers.md) gates the app cases by wall time: a `slow` case is `#[ignore]`d unless the backend crate's `slow_test` feature is on, an `ultra` case unless `ultra_slow_test` is. The gate is honest about *why* it skips (perf, not environment — [ADR-15](15-tests-fail-not-skip.md)), but it skips the whole case, conversion included. So a plain `cargo test` never converts qjs, sqlite3, ripgrep, CPython, CRuby, or DOOM at all, and the ultra tier is never touched by CI at any point. A change that makes the Bash backend reject `doom.wasm`, or panic converting `cpython.wasm`, stays invisible until someone runs the ultra tier locally before a release.

The two halves of a case are not remotely equal in cost. Executing these apps is what costs minutes; converting them costs about a second each, and the conversion is where a backend regression usually lands first (a rejected module, a panic, an unsupported-feature error). Measured on an M-series laptop, dev profile: qjs 0.6–1.2 s, libsqlite3 0.6–1.1 s, ripgrep 1.3–2.3 s, DOOM 0.3–1.0 s, cpython 2.9–7.4 s, ruby.wasm 7.6–18 s per backend.

## Decision

Every slow/ultra app case also gets a **convert-only smoke**, `#[test] fn <case>_convert()`, gated **one tier below** the case it derives from: a `slow` case's smoke is ungated (the fast gate converts it), an `ultra` case's smoke runs at `slow_test` (CI's main sweep converts it).

The discriminating criterion, reusable for any future check: **a derived check belongs one tier below its parent when it costs an order of magnitude less and proves a strictly weaker property.** One tier down rather than always-fast, because the cost of converting a case tracks the cost of running it — an extreme case's conversion is the most expensive conversion too, so tying the smoke's tier to the parent's keeps it self-limiting instead of letting a future monster land in the fast gate.

The smoke performs exactly the conversion its execution case would — same `Mode`, same module/class name — and asserts only that it completes and returns non-empty source. Nothing is run, so there is no golden and no interpreter needed. It is emitted by the *same* per-case macro as the execution case, so the `e2e.rs` callsites stay byte-identical and a backend that declines a case (by not invoking the macro, [ADR-27](27-test-helper-crate.md)) declines its smoke with it.

## Rejected alternatives

- **Nothing — the status quo, which is ADR-53's position** ("a convert-only assertion would be an idiom no other suite uses"). That argument was against a one-off for DOOM; applied uniformly the objection dissolves, since it *becomes* the idiom. The blind spot it leaves is concrete: zero CI coverage of the ultra tier, and a fast gate that converts no real-world app.
- **Put every smoke at the fast tier**, ignoring the parent's tier. Simpler to state, but it drops the self-limiting property: an ultra case exists because it is extreme, and the next one may be extreme in conversion too.
- **A central table of (binary, backend) pairs to smoke-convert.** Deduplicates the binaries shared by several cases, but drifts from the callsites — a case a backend drops would keep being smoked, and a new case would need two edits. The callsite is the capability declaration (ADR-27).
- **Memoize conversions so cases sharing a binary convert once.** Measured saving is about a second (the shared binaries are the cheap ones — qjs, sqlite3); not worth a cache in the test helper.
- **Golden the generated size or line count.** Would catch size regressions, but every legitimate lowering change rewrites the golden; pure noise against a smoke whose job is "did it convert at all".

## Consequences

- The fast gate converts nearly every cached app on every backend: `cargo test` goes from ~59 s to ~80–93 s here (two runs each; the spread is scheduling noise), for 44 new tests. Per backend, the `--test e2e` target grows: ruby 1.1→10.4 s, python 2.1→8.2 s, java 2.6→4.9 s, go 0.3→1.1 s, bash 1.8→2.3 s. Ruby and Python pay most because they convert `ruby.wasm` (CRuby) at the fast tier.
- The ultra tier gets its first CI coverage — conversion only, but it is the half that catches most regressions.
- Consequence for setup: the *whole* apps cache is now a fast-gate prerequisite, not just cowsay and minigzip. Missing binaries still fail loud with the `fetch-and-build.sh` message (ADR-15); `require_cached_app` in `crates/dewasm-test-helper/src/fixtures.rs` is now the single place that says so.
- Conversion *performance* becomes a fast-gate-visible property. This is why [#59](https://github.com/dewasm/dewasm/issues/59) had to be fixed first: the Python and Java backends re-derived branch-target sets top-down, making conversion quadratic in nesting depth and `cpython.wasm` an ~80 s outlier that this rule would have parked in the fast gate. A future superlinear regression now shows up as a slow gate instead of not showing up at all.
- Duplicated work at the slow tier: a case that runs also converts twice (once for the smoke, once for itself). Accepted — a second or two against a case measured in minutes.
