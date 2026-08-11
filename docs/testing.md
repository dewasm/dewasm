# Testing dewasm

What `cargo test` actually needs, and what happens when it's missing.

## The policy: fail loud, never skip

A test whose required tool or setup step is missing **fails**, with a message pointing back to this file; it does not silently skip and report success.
If you see a passing `cargo test`, every test in it actually ran.
Every failure message below is something this document explains how to fix.

## Required environment

- **Rust toolchain**: pinned by `rust-toolchain.toml`; plain `cargo` commands pick it up automatically.
- **`git submodule update --init`**: fetches the two upstream testsuite submodules (never edit either): the wasm spec testsuite into `tests/spec/` and the official WASI p1 conformance suite ([`WebAssembly/wasi-testsuite`](https://github.com/WebAssembly/wasi-testsuite), branch `prod/testsuite-base`) into `tests/wasi-testsuite/`.
  Without the former the spec harness (`cargo test -p dewasm-backend-ruby --test spec`, and likewise `-p dewasm-backend-bash` / `-p dewasm-backend-python`) fails immediately; without the latter the WASI-testsuite harness (`cargo test -p dewasm-backend-<lang> --test wasi_testsuite`) does.
  Initialize just one with `git submodule update --init tests/wasi-testsuite`.
- **`ruby` >= 3.4 on `PATH`**: needed by the spec harness and most of the `e2e` test (both the Ruby backend's own tests and, indirectly, anything comparing Ruby's output).
  The 3.4 floor is the generated runtime's `IO::Buffer`-backed memory.
- **`python3` >= 3.9 on `PATH` (or `$DEWASM_PYTHON`)**: needed by the Python backend's spec/e2e tests.
  Like Bash, the Python spec harness runs a curated `.wast` subset by default; add `-- --include-ignored` to run every file.
  Note: the slow qjs-scale generated modules (deep loop nesting) need **python >= 3.12.4**; 3.12.0-3.12.3 hit a static-block limit that breaks that generated code (issue #21), so CI pins 3.13.
- **`perl` >= 5.26 on `PATH` (or `$DEWASM_PERL`)**, built with 64-bit integers and doubles (`ivsize=8`/`nvsize=8`, which any stock perl on a 64-bit OS has): needed by the Perl backend's spec and units tests.
  Like Bash/Python, the Perl spec harness runs a curated `.wast` subset by default; add `-- --include-ignored` to run every file.
  The floor is the POSIX C99 math functions (`trunc`/`nearbyint`, perl 5.22) plus margin.
- **`bash` >= 5 on `PATH`, `$DEWASM_BASH`, or a common Homebrew install path**: needed by the Bash backend's spec/e2e/softfloat tests.
  macOS's system `/bin/bash` is 3.2 and does not qualify (no associative arrays / namerefs); install a newer one (e.g. `brew install bash`) and either put it on `PATH` ahead of the system one or point `$DEWASM_BASH` at it directly.
- **`go` on `PATH` (or `$DEWASM_GO`)**: needed by the Go backend's spec, e2e, and units tests.
  Go is compiled, so those tests `go build` the generated code (to a content-addressed cache binary) and run the binary; a units test also `go build`s the whole runtime bundle.
  The spec harness compiles one program per `.wast` file, so it defaults to a curated list (like bash/python); `-- --include-ignored` runs every file.
  Any recent Go (generics, i.e. >= 1.18) qualifies.
- **`java` and `javac` on `PATH` (or `$DEWASM_JAVA`/`$DEWASM_JAVAC`)**: needed by the Java backend's spec, e2e, and units tests.
  Java is compiled, so those tests `javac` the generated `Main.java` (to a content-addressed class-dir cache) and run `java -cp <dir> Main`; a units test also `javac`s the whole runtime bundle.
  JDK 11+ qualifies (the backend uses only standard APIs).
  The spec harness compiles one `Main.java` per `.wast` file, so it defaults to a curated list (like bash/python/go); `-- --include-ignored` runs every file.

Nothing else is required for `cargo test` to pass in full, **except** for the `apps` cases specifically; see below.

## The `apps` end-to-end cases

The `apps` case table (`crates/dewasm-test-helper/src/apps.rs`, run per backend as `cargo test -p dewasm-backend-<lang> --test e2e apps`) converts real-world wasm binaries (cowsay, quickjs-ng, SQLite) and checks the output against a snapshot reference.
Two things distinguish it from the rest of the suite:

- **The binaries themselves are fetched, not committed**: run `examples/apps/setup.sh` once (needs network access) to populate the gitignored `examples/apps/cache/`.
  This is a hard prerequisite, not an optional extra: the `apps` tests fail with a message naming the missing file until you run it.
  Several apps are built locally from pinned source, so `setup.sh` needs a few extra tools on PATH (only for `setup.sh`; `cargo test` itself never needs them once the cache exists):
    - the locally-built C apps (the three sqlite3 shapes, `minigzip`, libpcap, tree-sitter, the NES module, the DWARF fixture) need **`zig`** and, for zip archives, **`unzip`**; every locally-built app is post-processed with binaryen's **`wasm-opt`** (the NES build also uses `wasm-dis`), and libpcap additionally needs **`bison`** and **`flex`**;
    - **ripgrep** is built from pinned source with `cargo build --release --target wasm32-wasip1`, so it needs the **`wasm32-wasip1`** rustup target (`rustup target add wasm32-wasip1`); `setup.sh` fails loudly if it is missing;
    - **CPython** and **CRuby** are downloaded prebuilt (CRuby from the official ruby.wasm release; CPython from `brettcannon/cpython-wasi-build`, an unofficial WASI build, since the PSF ships none); `setup.sh` additionally extracts each one's stdlib tree (`cache/cpython-lib/lib/python3.14`, `cache/ruby-lib/usr/local/lib/ruby`) that the interpreters read at startup, which the heavy e2e cases preopen;
    - **ruby-packed** (CRuby with its stdlib embedded by `wasi-vfs pack`, ruby.wasm's self-contained deployment shape) is derived in-cache from the two CRuby artifacts above, so `setup.sh` needs the **`wasi-vfs`** CLI on PATH (a prebuilt from the [wasi-vfs releases](https://github.com/kateinoigakukun/wasi-vfs/releases), or `cargo install wasi-vfs-cli`).
- **The filesystem-exercising app cases are shared** across every fs-capable backend: the QuickJS file-I/O case, the sqlite3 DB-file case, ripgrep, and the CPython/CRuby runtime demos are `pub const` cases in `crates/dewasm-test-helper/src/apps_fs.rs`, each driven by its own per-case macro (`qjs_file_io_e2e!` … `cruby_hello_e2e!`); the sqlite3 C-API / callback drives live in `apps_capi.rs`, run via `libsqlite3_c_api_e2e!` and friends.
  (The QuickJS REPL is covered separately, under a real pty; see `qjs_repl_pty` below.) Each backend supplies only a named glue string constant per case (class name, argv, env, and preopen *guest* paths written literally; the runtime host paths substituted from `{scratch}`/`{cache}` placeholders).
  At present every backend invokes every filesystem and C-API case, some of them at the ultra speed; a backend that could not run a case would simply not invoke that case's macro, with the reason as a comment at the absent callsite.
  The committed driver fixtures (the `.js` scripts) live in `examples/apps/fixtures/`; the filesystem-app snapshots are still captured from `wasmtime`, while the C-API drives have none (their results live in guest memory), so each of those pins a fixed string.
- **No `wasmtime` install is needed to run these tests.**
  They used to diff live against `wasmtime run`; that comparison's result is fixed for a pinned binary and fixed input, so it's captured once and checked into `examples/apps/snapshots/*.stdout`, compiled into the test binary via `include_str!`.
  `wasmtime` is only needed for the opt-in check below, or if you're re-pinning an app's version and must regenerate its snapshot file.

### Checking the snapshot files are still accurate

A snapshot file is a claim ("this is what `wasmtime run` produces") that can go stale: a re-pinned app version, or a hand-edit mistake.
Since `wasmtime` isn't a required tool for the default suite (previous section), this check is opt-in: behind the `wasmtime_test` Cargo feature, and `#[ignore]`d when that feature is off, so a plain `cargo test` never needs `wasmtime` but a deliberate check can still run it:

```console
$ cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime
```

This runs wasmtime as a `BackendUnderTest` (`crates/dewasm-test-helper/tests/apps_wasmtime.rs`) through the *same* shared `apps`, `gzip`, and `fs_apps` runners the real backends use: for each case it execs `wasmtime run` (with `--dir`/`--env` for the filesystem cases) on the cached binary and compares the output against the checked-in snapshot file.
Whether dewasm's own generated output also matches is the other half, covered by the always-on per-backend `apps` tests.
Run this whenever you doubt a snapshot file, or as part of regenerating one after bumping a pin in `examples/apps/setup.sh`:

```console
$ examples/apps/setup.sh   # fetch the newly-pinned binary
$ cargo xtask update-snapshots       # or `update-snapshots <name>` for one app
```

`update-snapshots` drives the same runners this freshness test does, so a regenerated file is exactly what the test then re-verifies (see "Regenerating snapshot files" below).
After regenerating, update the matching `AppCase` in `crates/dewasm-test-helper/src/apps.rs` (`expect_code` too, if the exit status changed), and confirm with the `wasmtime_test` command above before running the normal per-backend `cargo test -p dewasm-backend-<lang> --test e2e`.

## Test layout

Tests live with the one backend they exercise; only a test that needs *every* backend lives centrally.
The shared harness, case tables, and the per-feature test macros are in `crates/dewasm-test-helper`, which depends only on `dewasm-core` + `dewasm-backend` (never on a concrete backend).

- **`crates/dewasm-backend-<lang>/tests/spec.rs`**: that backend's spec harness, holding its `SpecBackend` impl and its `EXPECTED_FAILURES` list (and, for bash, the curated file list), wired up with `spec_suite!`.
  Run it with `cargo test -p dewasm-backend-<lang> --test spec`.
- **`crates/dewasm-backend-<lang>/tests/convert.rs`**: that backend's whole-cache convert suite, a one-line `apps_convert_suite!(<Backend>)` invocation (the manifest and harness are shared).
  Run it with `cargo test -p dewasm-backend-<lang> --test convert`.
- **`crates/dewasm-backend-<lang>/tests/e2e.rs`**: that backend's suites, declared by invoking the shared macros.
  The file contains **only** the `BackendUnderTest` impl, named glue string constants (library glue, the WASI-filesystem template, the filesystem-app and C-API driver glue, the multi-module glue with its `compose_modules`/`run_in_dir` impls), and macro invocations: no backend-specific `#[test]` function, no glue-returning function or `match` on a case name.
  Which macros a backend invokes is its capability declaration; a case it cannot run is simply not invoked, with the reason as a comment at the callsite.
  A slow case carries a trailing speed token, `slow` by default or `ultra`, which decides whether the expanding crate's `slow_test` or `ultra_slow_test` feature un-ignores the generated `#[test]`.
  Runtime paths a glue const cannot know statically are `{scratch}`/`{cache}`/`{guest}`/`{host}` placeholders the runner fills (`glue::fill`).
- **The interactive-REPL pty case (`qjs_repl_pty`).**
  `qjs_repl_pty_e2e!` drives the *bare* QuickJS REPL (no script arg, so the interactive line editor) under a real pty (`crates/dewasm-test-helper/src/pty.rs`, `portable-pty`) and requires its transcript, ANSI escapes and all, to be byte-identical to the one wasmtime produces (`examples/apps/snapshots/qjs_repl_interactive.transcript`).
  A pty is required because qjs only enters that path when `fd_fdstat_get` on stdin reports a character device; a pipe does not.
  The scripted session is *prompt-driven*: each line is sent only after the `qjs > ` prompt reappears, so the transcript is stable however long a backend takes to start.
  It is slow on every backend and `ultra` on bash, where it timed out on CI (#22).
- The units lint (`declared_requires_cover_references`, `all_units_bundle`, and the go/java whole-bundle compile checks) lives as `#[cfg(test)] mod units` unit tests at the bottom of each backend's `src/lib.rs`, run with `cargo test -p dewasm-backend-<lang> --lib`.
  **`softfloat.rs`** (bash) is the one backend-local integration oracle.
- **`crates/dewasm-cli/tests/`**: only `support_docs.rs` (the freshness check that fails while the generated `docs/support.md` is stale, over all backends).
- **`crates/dewasm-test-helper/tests/apps_wasmtime.rs`**: wasmtime as a `BackendUnderTest`, running the `apps`/`gzip`/`fs_apps` snapshot-freshness checks through the shared runners, plus `qjs_repl_interactive_snapshot`, which re-captures the bare qjs REPL under a pty from a live wasmtime and compares it to the checked-in transcript (compare-only; regenerate with `cargo xtask update-snapshots`).
  All behind the `wasmtime_test` feature, named for a future engine such as wasmer/wasmedge joining it.

Onboarding a new backend to the e2e suites is: implement `BackendUnderTest` (and `SpecBackend` for the spec harness) in the new crate, then invoke the macros for the suites it participates in.

## Regenerating snapshot files

The checked-in snapshots are code-derived, not hand-written, and each has a compare-only test that fails with the exact command to regenerate it; there is no env-var "update mode" on the tests:

| Snapshot | Regenerate with | Compare-only test |
| --- | --- | --- |
| `docs/support.md` | `cargo xtask update-support-docs` | `cargo test -p dewasm-cli --test support_docs` |
| every execution snapshot (`examples/apps/snapshots/*`, incl. `doom_frame.ppm`) | `cargo xtask update-snapshots [filter]` | `cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime` (the DOOM frame: `cargo test -p dewasm-backend-ruby --features slow_test --test e2e doom_frame`, Bash needs `--features ultra_slow_test`) |

`update-support-docs` is separate because `docs/support.md` is generated *documentation*, not an execution snapshot: it needs no wasm engine at all.

`update-snapshots` regenerates every execution snapshot from one command: the app-stdout files, the gzip stream, the filesystem-app stdout, and the interactive-REPL transcript are all captured by driving the pinned binary through a live `wasmtime` (on `PATH`), so it needs the apps cache populated (`examples/apps/setup.sh`).
An optional substring `filter` limits it to matching snapshots (e.g. `cargo xtask update-snapshots doom`); no filter regenerates all of them.
On a clean tree it must reproduce every file byte-for-byte, so a resulting `git status` diff is a capture bug or genuine nondeterminism, not a routine update.

The DOOM frame is the one target that can't go through the `wasmtime` CLI: `doom.wasm`'s custom-import interface does not run under `wasmtime run`, so `update-snapshots` drives it with the embedded `wasmtime` *crate* (an xtask-only dependency, never in the normal `cargo test` build).
It writes `doom_frame.ppm`, the compared oracle, plus a `doom_frame.png` rendering of the same frame for human inspection (compared by no test), and needs only the doom app cached (`examples/apps/scripts/doom.sh`).
After re-pinning an app in `setup.sh`, run `update-snapshots` (a targeted filter is enough), re-run the `wasmtime_test` freshness suite above, and update the app's `expect_code` in `crates/dewasm-test-helper/src/apps.rs` if its exit status changed; for a doom pin bump, re-run the per-backend `doom_frame` cases.
`cargo xtask` is aliased in `.cargo/config.toml` to `cargo run -p xtask --`; run it with no arguments (or `--help`) for the command list.

## The spec harness (libtest-mimic)

Each backend's spec harness (`crates/dewasm-backend-<lang>/tests/spec.rs`) is a [libtest-mimic](https://crates.io/crates/libtest-mimic) harness (`harness = false`): every upstream `.wast` file becomes one named trial (the file stem is the trial name), enumerated at runtime from the `tests/spec` submodule.
This replaces the former `DEWASM_SPEC`/`DEWASM_SPEC_ALL` environment variables with cargo's own test UX:

- **Select files by name** with cargo's built-in filter: `cargo test -p dewasm-backend-ruby --test spec i32` runs every trial whose name contains `i32` (add `-- --exact i32` for that one file).
- **Curated vs. full run** is the ignore mechanism: files outside a backend's curated list are `#[ignore]`d trials, so a plain `cargo test` runs the curated set (Ruby is fast enough to curate nothing, so it runs all files), and `-- --include-ignored` (or `-- --ignored` for only the non-curated ones) runs the whole testsuite.
  The `slow_test` feature also runs the full suite (nothing is marked ignored), so CI's main run, `--features slow_test`, covers every `.wast` file.
- **Trials run in parallel** on libtest-mimic's thread pool; each trial owns its per-file state, so the runs parallelize across cores (control the thread count with `-- --test-threads=N`).

A passing trial is quiet; a failing one carries that file's `pass/fail/skip` summary plus the failing assertion lines.
Per-file failure counts are checked against each backend's `EXPECTED_FAILURES` list inside the trial.

A failing trial means a semantics bug: fix the cause.
Extending a backend's `EXPECTED_FAILURES` list (in its `tests/spec.rs`) is a last resort and requires an attribution tag plus a reason.

The slow app cases (QuickJS, SQLite, the filesystem apps, the C-API cases, the interactive-REPL pty case) are divided into two speed categories controlled by cargo features rather than an environment variable:

- **`slow_test`**: CI's main run.
  Each backend crate declares it; the per-case macros expand their generated `#[test]` as `#[ignore]`d unless it is enabled, and it also runs the full spec-testsuite run.
  Run one backend's slow category with `--features slow_test` (e.g. `cargo test -p dewasm-backend-bash --features slow_test --test e2e`).
- **`ultra_slow_test`** (implies `slow_test`): the cases a CI runner cannot afford, either by wall time (roughly a minute or more) or by host memory.
  Every one of them is pinned at its callsite in the backend's `e2e.rs`, with a comment giving the reason and, where there is one, the issue number.
  No case is lost this way: each `ultra` case runs at `slow` on at least one other backend, so CI still covers the case itself; what the token withholds is that one backend's run of it.
  Run these in local pre-release verification with `--features ultra_slow_test`, or with the everything-included `cargo test -- --include-ignored`.
  The Bash giants are memory-hungry as well as slow, so start them one at a time (`-- --test-threads=1`, or one `--exact` name per invocation) rather than letting the harness run them in parallel.

This is a deliberate perf-based opt-out, not a missing-environment one (those still fail loud, per the policy at the top of this file); the ultra-slow category is not CI-verified.

See `AGENTS.md`'s Common commands table for the exact invocations.

## The convert suite (libtest-mimic)

Each backend also has a whole-cache **convert** suite (`crates/dewasm-backend-<lang>/tests/convert.rs`, `harness = false`, `main` from `apps_convert_suite!`; the shared harness is `crates/dewasm-test-helper/src/apps_convert.rs`).
It converts every cached real-world app with that backend and asserts the conversion completes with non-empty source; it never runs the generated program.
This is the coverage the execution e2e suites miss: they only feed the emitter the (backend × app) pairs a backend also *runs*.

- **Select one app by name** with cargo's filter: `cargo test -p dewasm-backend-ruby --test convert qjs` (the trial name is the cache-file stem).
- **Speed rule** (measured): the giant interpreter artifacts, `cpython`, `ruby`, `ruby-packed`, and `zeroperl`, take seconds to convert on every backend, so their trials are `#[ignore]`d unless the crate's `slow_test` feature is on; the rest run in the fast category.
  Un-ignore everything for one backend with `--features slow_test` (or `-- --include-ignored`).
- A missing cache file **fails** the trial, pointing at `examples/apps/setup.sh`; it does not skip.

## The WASI-testsuite harness (libtest-mimic)

Alongside the spec harness, each backend has a second libtest-mimic suite (`crates/dewasm-backend-<lang>/tests/wasi_testsuite.rs`, `harness = false`, `main` from `wasi_testsuite_suite!`) that runs the official WASI p1 conformance modules from the `tests/wasi-testsuite` submodule.
It converts each prebuilt `.wasm` in `--mode standalone` and executes it through the [standalone interface](standalone-interface.md): the co-located `<name>.json` manifest's `args`/`env`/`root` become guest argv / child env / a `--dir <root>::/` preopen (from a fresh temp copy, so trials are hermetic), and the trial asserts the process exit code and, when pinned, stdout.
Run one backend with:

```console
$ cargo test -p dewasm-backend-ruby --test wasi_testsuite   # or bash/python/go/java
```

The c + rust + assemblyscript `wasm32-wasip1` suites run (the Rust `wasm32-wasip3` tree is excluded, since preview 3 is component-model territory, which is out of scope).
Each backend carries its own `WASI_TESTSUITE_EXPECTED_FAILURES` list: every known failure is attributed to a declared ENOSYS gap (docs/support.md), a semantics-precision gap on a supported syscall, or the standalone interface's whole-environment passthrough.
As in the spec harness the list is checked both ways: a listed trial that unexpectedly *passes* is a hard failure, so filling a gap forces the entry to be removed.
