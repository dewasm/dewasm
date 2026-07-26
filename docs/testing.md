# Testing dewasm

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
  the spec harness (`cargo test -p dewasm-backend-ruby --test spec`, and
  likewise `-p dewasm-backend-bash` / `-p dewasm-backend-python`) fails
  immediately.
- **`ruby` on `PATH`**: needed by the spec harness and most of the `e2e`
  test (both the Ruby backend's own tests and, indirectly, anything
  comparing Ruby's output). No specific version is required.
- **`python3` >= 3.9 on `PATH` (or `$DEWASM_PYTHON`)**: needed by the
  Python backend's spec/e2e tests. Like Bash, the Python spec harness runs
  a curated `.wast` subset by default (the full sweep is ~2 min); pass
  `DEWASM_SPEC_ALL=1` to run every file.
- **`bash` >= 5 on `PATH`, `$DEWASM_BASH`, or a common Homebrew
  install path**: needed by the Bash backend's spec/e2e/softfloat tests.
  macOS's system `/bin/bash` is 3.2 and does not qualify (no associative
  arrays / namerefs); install a newer one (e.g. `brew install bash`) and
  either put it on `PATH` ahead of the system one or point
  `$DEWASM_BASH` at it directly.
- **`go` on `PATH` (or `$DEWASM_GO`)**: needed by the Go backend's spec, e2e,
  and units tests. Go is compiled, so those tests `go build` the generated code
  (to a content-addressed cache binary) and run the binary; a units test also
  `go build`s the whole runtime bundle. The spec harness compiles one program
  per `.wast` file, so it defaults to a curated list (like bash/python);
  `DEWASM_SPEC_ALL=1` sweeps every file. Any recent Go (generics, i.e. >= 1.18)
  qualifies.
- **`java` and `javac` on `PATH` (or `$DEWASM_JAVA`/`$DEWASM_JAVAC`)**: needed by
  the Java backend's spec, e2e, and units tests. Java is compiled, so those tests
  `javac` the generated `Main.java` (to a content-addressed class-dir cache) and
  run `java -cp <dir> Main`; a units test also `javac`s the whole runtime bundle.
  JDK 11+ qualifies (the backend uses only standard APIs). The spec harness
  compiles one `Main.java` per `.wast` file, so it defaults to a curated list
  (like bash/python/go); `DEWASM_SPEC_ALL=1` sweeps every file (~40 s).

Nothing else is required for `cargo test` to pass in full, **except** for
the `apps` cases specifically — see below.

## The `apps` end-to-end cases

The `apps` case table (`crates/dewasm-test-helper/src/apps.rs`, run per
backend as `cargo test -p dewasm-backend-<lang> --test e2e apps`) converts
real-world wasm binaries (cowsay, quickjs-ng, SQLite) and checks the output
against a golden reference. Two things distinguish it from the rest of the
suite:

- **The binaries themselves are fetched, not committed** ([ADR-9](adr/9-example-apps-from-registry.md)):
  run `examples/apps/fetch.sh` once (needs network access) to populate
  the gitignored `examples/apps/cache/`. Per ADR-15 this is a hard
  prerequisite, not an optional extra — the `apps` tests fail with a
  message naming the missing file until you run it. Several apps are built
  locally from pinned source, so `fetch.sh` needs a few extra tools on PATH
  (only for `fetch.sh` — `cargo test` itself never needs them once the cache
  exists):
    - the three sqlite3 shapes and `minigzip` (zlib) need **`zig`** and
      **`unzip`** ([ADR-22](adr/22-sqlite3-built-from-source.md); `minigzip`
      is `zig cc`'d from the pinned zlib 1.3.1 source, the byte-exact-stdio
      compression CLI that runs under both backends);
    - **ripgrep** is built with `cargo build --release --target
      wasm32-wasip1` from the pinned 14.1.1 source, so it needs the
      **`wasm32-wasip1`** rustup target (`rustup target add wasm32-wasip1`);
      `fetch.sh` fails loudly if the target is not installed;
    - **CPython** and **CRuby** are downloaded prebuilt (official
      wasm32-wasip1 releases); `fetch.sh` additionally extracts each one's
      stdlib tree (`cache/cpython-lib/lib/python3.14`,
      `cache/ruby-lib/usr/local/lib/ruby`) that the interpreters read at
      startup, which the heavy e2e cases preopen.
- **Some app cases are Ruby-only and filesystem-exercising** (Phase 5a): the
  QuickJS file-I/O and scripted-REPL cases and the sqlite3 DB-file / C-API /
  callback cases live in the Ruby crate's e2e (`crates/dewasm-backend-ruby/tests/e2e.rs`,
  e.g. `qjs_file_io_ruby`, `sqlite3_callback_binding_ruby`) rather than the
  shared `apps.rs` table, because only Ruby has WASI filesystem support
  (ADR-14). Their own committed driver fixtures (the `.js` scripts) live in
  `examples/apps/fixtures/`; their goldens are still captured from `wasmtime`
  (see below).
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
$ cargo test -p dewasm-cli --test apps_golden --features wasmtime_test apps_golden_matches_wasmtime
```

This re-runs every `apps` case through a live `wasmtime run` and
compares its output against the checked-in golden file (independent of
whether dewasm's own generated output also matches — the always-on per-backend
`apps` tests already cover that half). The sibling
`apps_golden_fs_matches_wasmtime` (same feature gate) does the same for the
filesystem cases, driving them under `wasmtime --dir <scratch>::<guest>` —
QuickJS file I/O (`--dir <scratch>::/work`, arg `/work/qjs_file_io.js`), the
QuickJS scripted REPL (same, `qjs_repl.js`, the scripted session on stdin),
and the sqlite3 shell writing then reopening a DB file (`--dir <scratch>::/db`,
two invocations). Run either whenever
you doubt a golden file, or as part of regenerating one after bumping a
pin in `examples/apps/fetch.sh`:

```console
$ examples/apps/fetch.sh   # fetch the newly-pinned binary
$ wasmtime run examples/apps/cache/<name>.wasm <args...> < <stdin-fixture> \
    > examples/apps/golden/<case>.stdout
```

Then update the matching `AppCase` in `crates/dewasm-test-helper/src/apps.rs`
(`expect_code` too, if the exit status changed), and confirm with the
`wasmtime_test` command above before running the normal per-backend
`cargo test -p dewasm-backend-<lang> --test e2e`.

## Test layout

Tests live with the one backend they exercise; only a test that needs *every*
backend lives centrally (ADR-27). The shared harness, case tables, and the
per-feature test macros are in `crates/dewasm-test-helper`, which depends only
on `dewasm-core` + `dewasm-backend` (never on a concrete backend).

- **`crates/dewasm-backend-<lang>/tests/spec.rs`** — that backend's spec
  conformance suite: its `SpecBackend` impl, its `EXPECTED_FAILURES` ledger
  (and, for bash, the curated file list), wired up with `spec_suite!`. Run it
  with `cargo test -p dewasm-backend-<lang> --test spec`.
- **`crates/dewasm-backend-<lang>/tests/e2e.rs`** — that backend's standalone
  / library / WASI / apps suites, declared by invoking `standalone_e2e!`,
  `library_e2e!`, `wasi_suite!`, and `apps_e2e!` over the shared tables in
  `dewasm-test-helper`. Library glue and (for the WASI filesystem cases) the
  per-backend instantiation glue live here; a case a backend is wired to run
  but has no glue for fails loudly (ADR-15). Ruby's file also holds the
  Ruby-only scenarios (provider objects, embedded coexistence, the sqlite3 C
  API drive, WASI-model internals).
- **`crates/dewasm-backend-<lang>/tests/units.rs`** (both) and
  **`softfloat.rs`** (bash) — backend-local lints/oracles, unchanged.
- **`crates/dewasm-cli/tests/`** — only the cross-backend tests:
  `support_docs.rs` (the `docs/support.md` golden gate over all backends) and
  `apps_golden.rs` (`apps_golden_matches_wasmtime`, behind the `wasmtime_test`
  feature).

Onboarding a new backend to the e2e suites is: implement `BackendUnderTest`
(and `SpecBackend` for the spec harness) in the new crate, then invoke the
macros for the suites it participates in.

## Useful environment variables

| Variable | Effect |
| --- | --- |
| `DEWASM_BASH` | Path to a bash >= 5 interpreter, checked before `PATH`/Homebrew fallbacks. |
| `DEWASM_PYTHON` | Path to a python3 >= 3.9 interpreter, checked before `python3`/`python` on `PATH`. |
| `DEWASM_SPEC=i32,br` | Restrict the spec harness to specific `.wast` files (comma-separated stems). |
| `DEWASM_SPEC_ALL=1` | Run the spec harness against every upstream `.wast` file instead of the curated default list (bash and python both default to a curated subset for speed). |
| `DEWASM_APPS_ALL=1` | Run the `apps` cases marked `heavy` (QuickJS, SQLite) under Bash too; skipped there by default since bash's softfloat makes them slow (this is a deliberate perf-based opt-out, not a missing-environment one — see ADR-15's scope). |

See `AGENTS.md`'s Common commands table for the exact invocations.
