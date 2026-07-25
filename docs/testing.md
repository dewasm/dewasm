# Testing dewasmify

What `cargo test` actually needs, and what happens when it's missing.

## The policy: fail loud, never skip

Per [ADR-15](adr/15-tests-fail-not-skip.md), a test whose required tool
or setup step is missing **fails**, with a message pointing back to this
file — it does not silently skip and report success. If you see a
passing `cargo test`, every test in it actually ran. Every failure
message below is something this document explains how to fix.

## Required environment

- **Rust toolchain**: pinned by `rust-toolchain.toml`; plain `cargo`
  commands pick it up automatically.
- **`git submodule update --init`**: fetches the wasm spec testsuite into
  `tests/spec/` (an upstream submodule — never edit it). Without this,
  the spec harness (`cargo test -p dewasmify-cli --test spec`) fails
  immediately.
- **`ruby` on `PATH`**: needed by the spec harness and most of the `e2e`
  test (both the Ruby backend's own tests and, indirectly, anything
  comparing Ruby's output). No specific version is required.
- **`bash` >= 5 on `PATH`, `$DEWASMIFY_BASH`, or a common Homebrew
  install path**: needed by the Bash backend's spec/e2e/softfloat tests.
  macOS's system `/bin/bash` is 3.2 and does not qualify (no associative
  arrays / namerefs); install a newer one (e.g. `brew install bash`) and
  either put it on `PATH` ahead of the system one or point
  `$DEWASMIFY_BASH` at it directly.

Nothing else is required for `cargo test` to pass in full, **except** for
the `apps` cases specifically — see below.

## The `apps` end-to-end cases

`crates/dewasmify-cli/tests/e2e/apps.rs` converts real-world wasm
binaries (cowsay, quickjs-ng, SQLite) and checks the output against a
golden reference. Two things distinguish it from the rest of the suite:

- **The binaries themselves are fetched, not committed** ([ADR-9](adr/9-example-apps-from-registry.md)):
  run `examples/apps/fetch.sh` once (needs network access) to populate
  the gitignored `examples/apps/cache/`. Per ADR-15 this is a hard
  prerequisite, not an optional extra — the `apps` tests fail with a
  message naming the missing file until you run it. The sqlite3 pair
  (`sqlite3-shell.wasm` and the C-API-exporting `libsqlite3.wasm`) is
  built locally from the pinned amalgamation source, so `fetch.sh`
  additionally needs `zig` and `unzip` on PATH
  ([ADR-22](adr/22-sqlite3-built-from-source.md)); `cargo test` itself
  never needs them once the cache exists.
- **No `wasmtime` install is needed to run these tests.** They used to
  diff live against `wasmtime run`; that comparison's result is fixed
  for a pinned binary and fixed input, so it's captured once and checked
  into `examples/apps/golden/*.stdout`, compiled into the test binary via
  `include_str!`. `wasmtime` is only needed for the opt-in check below,
  or if you're re-pinning an app's version and must regenerate its
  golden file.

### Checking the golden files are still accurate

A golden file is a claim ("this is what `wasmtime run` produces") that
can go stale — a re-pinned app version, or a hand-edit mistake. Since
`wasmtime` isn't a required tool for the default suite (previous
section), this check is opt-in: behind the `wasmtime_test` Cargo
feature, and `#[ignore]`d when that feature is off, so a plain
`cargo test` never needs `wasmtime` but a deliberate check can still
run it:

```console
$ cargo test -p dewasmify-cli --test e2e --features wasmtime_test apps_golden_matches_wasmtime
```

This re-runs every `apps` case through a live `wasmtime run` and
compares its output against the checked-in golden file (independent of
whether dewasmify's own generated output also matches — the always-on
`apps_ruby`/`apps_bash` tests already cover that half). Run it whenever
you doubt a golden file, or as part of regenerating one after bumping a
pin in `examples/apps/fetch.sh`:

```console
$ examples/apps/fetch.sh   # fetch the newly-pinned binary
$ wasmtime run examples/apps/cache/<name>.wasm <args...> < <stdin-fixture> \
    > examples/apps/golden/<case>.stdout
```

Then update the matching `AppCase` in `apps.rs` (`expect_code` too, if
the exit status changed), and confirm with the `wasmtime_test` command
above before running the normal `cargo test --test e2e`.

## e2e case tables and support tiers

`standalone.rs`/`library.rs`/`apps.rs` each drive one case table across
every backend instead of duplicating tests per language. Each case
declares the wasm-1.0-+-WASI-p1 support tier it needs
([ADR-23](adr/23-backend-support-tiers.md)); a backend whose
`achieved_tier` doesn't reach it gets a skip line for that case, not a
failure — this is a declared-tier gap, not a missing-tool problem, so it
doesn't fall under the ADR-15 policy above. Onboarding a new backend to
this suite is implementing the `E2eLang` trait (`e2e/support.rs`), adding
a `glues` entry to each `LibraryCase` it should run, and writing that
language's own `#[test]` functions; the tier field on each case then
decides what it's expected to pass without further per-case wiring.

## Useful environment variables

| Variable | Effect |
| --- | --- |
| `DEWASMIFY_BASH` | Path to a bash >= 5 interpreter, checked before `PATH`/Homebrew fallbacks. |
| `DEWASMIFY_SPEC=i32,br` | Restrict the spec harness to specific `.wast` files (comma-separated stems). |
| `DEWASMIFY_SPEC_ALL=1` | Run the spec harness against every upstream `.wast` file instead of the curated default list (bash defaults to a curated subset for speed). |
| `DEWASMIFY_APPS_ALL=1` | Run the `apps` cases marked `heavy` (QuickJS, SQLite) under Bash too; skipped there by default since bash's softfloat makes them slow (this is a deliberate perf-based opt-out, not a missing-environment one — see ADR-15's scope). |

See `AGENTS.md`'s Common commands table for the exact invocations.
