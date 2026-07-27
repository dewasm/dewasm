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
- **The filesystem-exercising app cases are shared** across every fs-capable
  backend (ADR-27 revision): the QuickJS file-I/O and scripted-REPL cases, the
  sqlite3 DB-file case, ripgrep, and the CPython/CRuby runtime demos live in the
  shared `FS_APP_CASES` table (`crates/dewasm-test-helper/src/apps_fs.rs`, run
  via `fs_apps_e2e!`); the sqlite3 C-API / callback drives live in the shared
  `CAPI_CASES` table (`apps_capi.rs`, run via `capi_apps_e2e!`). Each backend
  supplies only per-language glue; per-case `exclude` rows carry the documented
  capability/practicality exclusions (e.g. CPython/CRuby on Java, CRuby on Go).
  The committed driver fixtures (the `.js` scripts) live in
  `examples/apps/fixtures/`; the `FS_APP_CASES` goldens are still captured from
  `wasmtime` (the C-API drives have no wasmtime golden — results live in guest
  memory — so each pins a fixed string).
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
$ cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime
```

This runs wasmtime as a `BackendUnderTest` (`crates/dewasm-test-helper/tests/apps_wasmtime.rs`)
through the *same* shared `apps`, `gzip`, and `fs_apps` runners the real
backends use: for each case it execs `wasmtime run` (with `--dir`/`--env` for
the filesystem cases) on the cached binary and compares the output against the
checked-in golden file (independent of whether dewasm's own generated output
also matches — the always-on per-backend `apps` tests already cover that half).
The `fs_apps` cases drive `wasmtime --dir <scratch>::<guest>` — QuickJS file I/O
(`--dir <scratch>::/work`, arg `/work/qjs_file_io.js`), the QuickJS scripted
REPL (same, `qjs_repl.js`, the scripted session on stdin), the sqlite3 shell
writing then reopening a DB file (`--dir <scratch>::/db`, two invocations), and
ripgrep over a staged tree. Run this whenever
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
- **`crates/dewasm-backend-<lang>/tests/e2e.rs`** — that backend's suites,
  declared by invoking the shared macros: `standalone_e2e!`, `library_e2e!`,
  `wasi_suite!`, `apps_e2e!`, `gzip_e2e!`, `qjs_repl_pty_e2e!`, and (for the
  fs-capable backends) `fs_apps_e2e!`, `capi_apps_e2e!`, `multi_module_e2e!`
  over the shared tables in `dewasm-test-helper`. Per the ADR-27 revision this
  file contains **only** the `BackendUnderTest` impl, glue strings /
  glue-producing functions (library glue, WASI-filesystem instantiation glue,
  the C-API driver glue, the `app_glue` and `compose_modules` impls), and macro
  invocations — no backend-specific `#[test]` function. Which macros a backend
  invokes, plus each shared table's per-case `exclude` `(lang, reason)` rows, is
  the capability declaration; a case a backend is wired to run but has no glue
  for fails loudly (ADR-15).
- **The interactive-REPL pty case (`qjs_repl_pty`).** `qjs_repl_pty_e2e!`
  drives the *bare* QuickJS REPL (no script arg → interactive line editor)
  under a real pty (`crates/dewasm-test-helper/src/pty.rs`, `portable-pty`) and
  requires its transcript — ANSI escapes and all — to be byte-identical to the
  one wasmtime produces. A pty is required because qjs only enters that path
  when `fd_fdstat_get` on stdin reports a character device; a pipe does not.
  The scripted session is *prompt-driven*: each line is sent only after the
  `qjs > ` prompt reappears, so the transcript is stable regardless of how long
  a backend takes to start (Ruby parses a ~200 MB source first). Gated behind
  `DEWASM_APPS_ALL` (perf opt-out, ADR-15); the golden lives at
  `examples/apps/golden/qjs_repl_interactive.transcript`.
- The units lint (`declared_requires_cover_references`, `all_units_bundle`, and
  the go/java whole-bundle compile checks) lives as `#[cfg(test)] mod units`
  unit tests at the bottom of each backend's `src/lib.rs`, run with
  `cargo test -p dewasm-backend-<lang> --lib`. **`softfloat.rs`** (bash) is the
  one remaining backend-local integration oracle, unchanged.
- **`crates/dewasm-cli/tests/`** — only `support_docs.rs` (the
  `docs/support.md` golden gate over all backends).
- **`crates/dewasm-test-helper/tests/apps_wasmtime.rs`** — wasmtime as a
  `BackendUnderTest`: the `apps`/`gzip`/`fs_apps` golden-freshness checks run
  through the shared runners, plus `qjs_repl_interactive_golden`, which
  re-captures the bare qjs REPL under a pty from a live wasmtime and compares it
  to the checked-in transcript (set `DEWASM_UPDATE_GOLDEN=1` to regenerate it).
  All behind the `wasmtime_test` feature, named for a future engine such as
  wasmer/wasmedge joining it.

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
