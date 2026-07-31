# Testing dewasm

What `cargo test` actually needs, and what happens when it's missing.

## The policy: fail loud, never skip

Per [ADR-15](adr/15-tests-fail-not-skip.md), a test whose required tool or setup step is missing **fails**, with a message pointing back to this file — it does not silently skip and report success. If you see a passing `cargo test`, every test in it actually ran. Every failure message below is something this document explains how to fix.

## Required environment

- **Rust toolchain**: pinned by `rust-toolchain.toml`; plain `cargo` commands pick it up automatically.
- **`git submodule update --init`**: fetches the two upstream testsuite submodules (never edit either) — the wasm spec testsuite into `tests/spec/` and the official WASI p1 conformance suite ([`WebAssembly/wasi-testsuite`](https://github.com/WebAssembly/wasi-testsuite), branch `prod/testsuite-base`) into `tests/wasi-testsuite/`. Without the former the spec harness (`cargo test -p dewasm-backend-ruby --test spec`, and likewise `-p dewasm-backend-bash` / `-p dewasm-backend-python`) fails immediately; without the latter the WASI-testsuite harness (`cargo test -p dewasm-backend-<lang> --test wasi_testsuite`, ADR-36) does. Initialize just one with `git submodule update --init tests/wasi-testsuite`.
- **`ruby` >= 3.4 on `PATH`**: needed by the spec harness and most of the `e2e` test (both the Ruby backend's own tests and, indirectly, anything comparing Ruby's output). The 3.4 floor is the generated runtime's `IO::Buffer`-backed memory (see docs/adr/33-ruby-io-buffer-memory.md).
- **`python3` >= 3.9 on `PATH` (or `$DEWASM_PYTHON`)**: needed by the Python backend's spec/e2e tests. Like Bash, the Python spec harness runs a curated `.wast` subset by default; add `-- --include-ignored` to run every file. Note: the slow qjs-scale generated modules (deep loop nesting) need **python >= 3.12.4** — 3.12.0–3.12.3 hit a static-block limit that breaks that generated code (issue #21), so CI pins 3.13.
- **`bash` >= 5 on `PATH`, `$DEWASM_BASH`, or a common Homebrew install path**: needed by the Bash backend's spec/e2e/softfloat tests. macOS's system `/bin/bash` is 3.2 and does not qualify (no associative arrays / namerefs); install a newer one (e.g. `brew install bash`) and either put it on `PATH` ahead of the system one or point `$DEWASM_BASH` at it directly.
- **`go` on `PATH` (or `$DEWASM_GO`)**: needed by the Go backend's spec, e2e, and units tests. Go is compiled, so those tests `go build` the generated code (to a content-addressed cache binary) and run the binary; a units test also `go build`s the whole runtime bundle. The spec harness compiles one program per `.wast` file, so it defaults to a curated list (like bash/python); `-- --include-ignored` sweeps every file. Any recent Go (generics, i.e. >= 1.18) qualifies.
- **`java` and `javac` on `PATH` (or `$DEWASM_JAVA`/`$DEWASM_JAVAC`)**: needed by the Java backend's spec, e2e, and units tests. Java is compiled, so those tests `javac` the generated `Main.java` (to a content-addressed class-dir cache) and run `java -cp <dir> Main`; a units test also `javac`s the whole runtime bundle. JDK 11+ qualifies (the backend uses only standard APIs). The spec harness compiles one `Main.java` per `.wast` file, so it defaults to a curated list (like bash/python/go); `-- --include-ignored` sweeps every file.

Nothing else is required for `cargo test` to pass in full, **except** for the `apps` cases specifically — see below.

## The `apps` end-to-end cases

The `apps` case table (`crates/dewasm-test-helper/src/apps.rs`, run per backend as `cargo test -p dewasm-backend-<lang> --test e2e apps`) converts real-world wasm binaries (cowsay, quickjs-ng, SQLite) and checks the output against a golden reference. Two things distinguish it from the rest of the suite:

- **The binaries themselves are fetched, not committed** ([ADR-9](adr/9-example-apps-from-registry.md)): run `examples/apps/fetch-and-build.sh` once (needs network access) to populate the gitignored `examples/apps/cache/`. Per ADR-15 this is a hard prerequisite, not an optional extra — the `apps` tests fail with a message naming the missing file until you run it. Several apps are built locally from pinned source, so `fetch-and-build.sh` needs a few extra tools on PATH (only for `fetch-and-build.sh` — `cargo test` itself never needs them once the cache exists):
    - the three sqlite3 shapes and `minigzip` (zlib) need **`zig`** and **`unzip`** ([ADR-22](adr/22-sqlite3-built-from-source.md); `minigzip` is `zig cc`'d from the pinned zlib 1.3.1 source, the byte-exact-stdio compression CLI that runs under both backends);
    - **ripgrep** is built with `cargo build --release --target wasm32-wasip1` from the pinned 14.1.1 source, so it needs the **`wasm32-wasip1`** rustup target (`rustup target add wasm32-wasip1`); `fetch-and-build.sh` fails loudly if the target is not installed;
    - **CPython** and **CRuby** are downloaded prebuilt (CRuby from the official ruby.wasm release; CPython from `brettcannon/cpython-wasi-build`, an unofficial WASI build — the PSF ships none); `fetch-and-build.sh` additionally extracts each one's stdlib tree (`cache/cpython-lib/lib/python3.14`, `cache/ruby-lib/usr/local/lib/ruby`) that the interpreters read at startup, which the heavy e2e cases preopen.
- **The filesystem-exercising app cases are shared** across every fs-capable backend (ADR-27 revision): the QuickJS file-I/O and scripted-REPL cases, the sqlite3 DB-file case, ripgrep, and the CPython/CRuby runtime demos are `pub const` cases in `crates/dewasm-test-helper/src/apps_fs.rs`, each driven by its own per-case macro (`qjs_file_io_e2e!` … `cruby_hello_e2e!`); the sqlite3 C-API / callback drives live in `apps_capi.rs`, run via `libsqlite3_c_api_e2e!` and friends. Each backend supplies only a named glue string constant per case (class name, argv, env, and preopen *guest* paths written literally; the runtime host paths substituted from `{scratch}`/`{cache}` placeholders). A backend that cannot run a case does not invoke that case's macro; the reason is a comment at the (absent) callsite, with the measured numbers in `docs/apps-audit.md` (e.g. CPython/CRuby on Java, CRuby on Go). The committed driver fixtures (the `.js` scripts) live in `examples/apps/fixtures/`; the filesystem-app goldens are still captured from `wasmtime` (the C-API drives have no wasmtime golden — results live in guest memory — so each pins a fixed string).
- **A tier-gated case still gets converted one tier down.** Its per-case macro also emits a convert-only `<case>_convert` smoke — conversion alone, no run, no golden ([ADR-54](adr/54-convert-only-app-smokes.md); see the tier list below). Consequence for setup: a plain `cargo test` now converts nearly every cached binary, so the whole cache — not just cowsay and minigzip — is a fast-gate prerequisite.
- **No `wasmtime` install is needed to run these tests.** They used to diff live against `wasmtime run`; that comparison's result is fixed for a pinned binary and fixed input, so it's captured once and checked into `examples/apps/golden/*.stdout`, compiled into the test binary via `include_str!`. `wasmtime` is only needed for the opt-in check below, or if you're re-pinning an app's version and must regenerate its golden file.

### Checking the golden files are still accurate

A golden file is a claim ("this is what `wasmtime run` produces") that can go stale — a re-pinned app version, or a hand-edit mistake. Since `wasmtime` isn't a required tool for the default suite (previous section), this check is opt-in: behind the `wasmtime_test` Cargo feature, and `#[ignore]`d when that feature is off, so a plain `cargo test` never needs `wasmtime` but a deliberate check can still run it:

```console
$ cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime
```

This runs wasmtime as a `BackendUnderTest` (`crates/dewasm-test-helper/tests/apps_wasmtime.rs`) through the *same* shared `apps`, `gzip`, and `fs_apps` runners the real backends use: for each case it execs `wasmtime run` (with `--dir`/`--env` for the filesystem cases) on the cached binary and compares the output against the checked-in golden file (independent of whether dewasm's own generated output also matches — the always-on per-backend `apps` tests already cover that half). The `fs_apps` cases drive `wasmtime --dir <scratch>::<guest>` — QuickJS file I/O (`--dir <scratch>::/work`, arg `/work/qjs_file_io.js`), the QuickJS scripted REPL (same, `qjs_repl.js`, the scripted session on stdin), the sqlite3 shell writing then reopening a DB file (`--dir <scratch>::/db`, two invocations), and ripgrep over a staged tree. Run this whenever you doubt a golden file, or as part of regenerating one after bumping a pin in `examples/apps/fetch-and-build.sh`:

```console
$ examples/apps/fetch-and-build.sh   # fetch the newly-pinned binary
$ wasmtime run examples/apps/cache/<name>.wasm <args...> < <stdin-fixture> \
    > examples/apps/golden/<case>.stdout
```

Then update the matching `AppCase` in `crates/dewasm-test-helper/src/apps.rs` (`expect_code` too, if the exit status changed), and confirm with the `wasmtime_test` command above before running the normal per-backend `cargo test -p dewasm-backend-<lang> --test e2e`.

## Test layout

Tests live with the one backend they exercise; only a test that needs *every* backend lives centrally (ADR-27). The shared harness, case tables, and the per-feature test macros are in `crates/dewasm-test-helper`, which depends only on `dewasm-core` + `dewasm-backend` (never on a concrete backend).

- **`crates/dewasm-backend-<lang>/tests/spec.rs`** — that backend's spec conformance suite: its `SpecBackend` impl, its `EXPECTED_FAILURES` ledger (and, for bash, the curated file list), wired up with `spec_suite!`. Run it with `cargo test -p dewasm-backend-<lang> --test spec`.
- **`crates/dewasm-backend-<lang>/tests/e2e.rs`** — that backend's suites, declared by invoking the shared macros. The zero-glue aggregate macros (`gzip_e2e!`, `qjs_repl_pty_e2e!`, `wasi_suite!(Stdio/ArgsEnv/ClockRandom/Poll)`) take just the backend; the per-case macros (the library cases, the apps `cowsay_args_e2e!`/`cowsay_stdin_e2e!`/`qjs_eval_e2e!`/`sqlite3_shell_e2e!`, the filesystem apps `qjs_file_io_e2e!` … `cruby_hello_e2e!`, the C-API `libsqlite3_c_api_e2e!` …, the multi-module `shared_table_e2e!`/`embedded_coexist_e2e!`, `wasi_root_containment_e2e!`) take no glue (the apps macros) or one named glue-string constant as their only glue argument; the WASI-filesystem template `wasi_suite!(Fs, …)` likewise takes one glue constant. `qjs_eval_e2e!`/`sqlite3_shell_e2e!` are slow — the macro expands their generated `#[test]` as `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (run it with `--features slow_test`, or run everything including the ultra tier with a single `cargo test -- --include-ignored`); a callsite may pass a trailing `ultra` tier token ([ADR-48](adr/48-slow-test-tiers.md)). Per the ADR-27 revision this file contains **only** the `BackendUnderTest` impl, named glue string constants (library glue, the WASI-filesystem template, the filesystem-app instantiation glue, the C-API driver glue, the multi-module driver glue, and the `compose_modules` impl), and macro invocations — no backend-specific `#[test]` function, no glue-returning function or `match` on a case name. Which macros a backend invokes is the capability declaration; a case a backend cannot run is simply not invoked, with the reason as a comment at the callsite. Runtime paths a glue const cannot know statically are `{scratch}`/`{cache}`/`{guest}`/`{host}` placeholders the runner fills (`glue::fill`).
- **The interactive-REPL pty case (`qjs_repl_pty`).** `qjs_repl_pty_e2e!` drives the *bare* QuickJS REPL (no script arg → interactive line editor) under a real pty (`crates/dewasm-test-helper/src/pty.rs`, `portable-pty`) and requires its transcript — ANSI escapes and all — to be byte-identical to the one wasmtime produces. A pty is required because qjs only enters that path when `fd_fdstat_get` on stdin reports a character device; a pipe does not. The scripted session is *prompt-driven*: each line is sent only after the `qjs > ` prompt reappears, so the transcript is stable regardless of how long a backend takes to start (Ruby parses a ~200 MB source first). Slow: like the other slow per-case macros, `qjs_repl_pty_e2e!`'s generated `#[test]` is `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled (perf opt-out, ADR-15). For bash it is promoted to the ultra tier (`ultra_slow_test`) because it timed out on CI ([ADR-48](adr/48-slow-test-tiers.md), #22); the golden lives at `examples/apps/golden/qjs_repl_interactive.transcript`.
- The units lint (`declared_requires_cover_references`, `all_units_bundle`, and the go/java whole-bundle compile checks) lives as `#[cfg(test)] mod units` unit tests at the bottom of each backend's `src/lib.rs`, run with `cargo test -p dewasm-backend-<lang> --lib`. **`softfloat.rs`** (bash) is the one remaining backend-local integration oracle, unchanged.
- **`crates/dewasm-cli/tests/`** — only `support_docs.rs` (the `docs/support.md` golden gate over all backends).
- **`crates/dewasm-test-helper/tests/apps_wasmtime.rs`** — wasmtime as a `BackendUnderTest`: the `apps`/`gzip`/`fs_apps` golden-freshness checks run through the shared runners, plus `qjs_repl_interactive_golden`, which re-captures the bare qjs REPL under a pty from a live wasmtime and compares it to the checked-in transcript (compare-only; regenerate with `cargo xtask update-repl-golden`). All behind the `wasmtime_test` feature, named for a future engine such as wasmer/wasmedge joining it.

Onboarding a new backend to the e2e suites is: implement `BackendUnderTest` (and `SpecBackend` for the spec harness) in the new crate, then invoke the macros for the suites it participates in.

## Regenerating golden files

Two golden files are code-derived rather than hand-written, and each has a compare-only test that fails with the exact command to regenerate it — no env-var modes:

| Golden | Regenerate with | Compare-only test |
| --- | --- | --- |
| `docs/support.md` | `cargo xtask update-support-docs` | `cargo test -p dewasm-cli --test support_docs` |
| `examples/apps/golden/qjs_repl_interactive.transcript` | `cargo xtask update-repl-golden` | `cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime` |
| `examples/doom/golden/frame.ppm` | `cargo xtask update-doom-golden` | `cargo test -p dewasm-backend-ruby --features slow_test --test e2e doom_frame` (Bash needs `--features ultra_slow_test`) |

`update-repl-golden` needs `wasmtime` on `PATH` and the qjs app cached (`examples/apps/fetch-and-build.sh`) — the same requirements as the freshness test it feeds. `update-doom-golden` instead embeds the `wasmtime` *crate* (an xtask-only dependency, never in the normal `cargo test` build) to drive `doom.wasm`'s custom-import interface, which the `wasmtime` CLI cannot; it needs only the doom app cached (`examples/apps/scripts/doom.sh`). Regenerate it after bumping the doom pin, then re-run the per-backend `doom_frame` cases (ADR-53). `cargo xtask` is aliased in `.cargo/config.toml` to `cargo run -p xtask --`; run `cargo xtask` with no arguments (or `--help`) for the command list.

## Useful environment variables

| Variable | Effect |
| --- | --- |
| `DEWASM_BASH` | Path to a bash >= 5 interpreter, checked before `PATH`/Homebrew fallbacks. |
| `DEWASM_PYTHON` | Path to a python3 >= 3.9 interpreter, checked before `python3`/`python` on `PATH`. |

## The spec harness (libtest-mimic)

Each backend's spec integration test (`crates/dewasm-backend-<lang>/tests/spec.rs`) is a [libtest-mimic](https://crates.io/crates/libtest-mimic) harness (`harness = false`): every upstream `.wast` file becomes one named trial (the file stem is the trial name), enumerated at runtime from the `tests/spec` submodule. This replaces the former `DEWASM_SPEC`/`DEWASM_SPEC_ALL` environment variables with cargo's own test UX:

- **Select files by name** with cargo's built-in filter: `cargo test -p dewasm-backend-ruby --test spec i32` runs every trial whose name contains `i32` (add `-- --exact i32` for that one file).
- **Curated vs. full sweep** is the ignore mechanism: files outside a backend's curated list are `#[ignore]`d trials, so a plain `cargo test` runs the curated set (Ruby is fast enough to curate nothing — it runs all files), and `-- --include-ignored` (or `-- --ignored` for only the non-curated ones) sweeps the whole testsuite. The `slow_test` feature also runs the full sweep (nothing is marked ignored), so CI's main leg — `--features slow_test` — covers every `.wast` file ([ADR-48](adr/48-slow-test-tiers.md)).
- **Trials run in parallel** on libtest-mimic's thread pool; each trial owns its per-file state, so the sweeps parallelize across cores (control the thread count with `-- --test-threads=N`).

A passing trial is quiet; a failing one carries that file's `pass/fail/skip` summary plus the failing assertion lines. The former aggregate cross-backend `TOTAL: pass=… fail=…` line is gone — the per-file trial results supersede it. Per-file failure counts are still gated against each backend's `EXPECTED_FAILURES` ledger (ADR-8) inside the trial.

The slow app cases (QuickJS, SQLite, the filesystem apps, the C-API cases, the interactive-REPL pty case) run in two tiers gated by cargo features rather than an environment variable ([ADR-48](adr/48-slow-test-tiers.md)):

- **`slow_test`** — CI's main sweep. Each backend crate declares it; the per-case macros expand their generated `#[test]` as `#[ignore]`d unless it is enabled, and it also runs the full spec-testsuite sweep. Run one backend's slow tier with `--features slow_test` (e.g. `cargo test -p dewasm-backend-bash --features slow_test --test e2e`).
- **`ultra_slow_test`** (implies `slow_test`) — the cases measured at roughly a minute or more on a CI runner: the interactive qjs REPL pty case (bash timed out >180s, #22), go's giant-generated-program `go build`s (which collectively exhausted a 4-core runner's memory, #23), and the DOOM framebuffer golden under Bash (`doom_frame` at `ultra`, minutes — the same case runs at `slow` on Ruby/Python/Go/Java, so DOOM *is* covered in CI; [ADR-53](adr/53-doom-frame-golden.md)). These are pinned per callsite and **deliberately kept out of CI** — run them only in local pre-release verification with `--features ultra_slow_test` or the everything-included `cargo test -- --include-ignored`.

This is a deliberate perf-based opt-out, not a missing-environment one — see ADR-15's scope; the ultra tier is not CI-verified.

Every slow/ultra case additionally gets a **convert-only smoke** — `#[test] fn <case>_convert()`, derived automatically by the same per-case macro and gated **one tier down**: a `slow` case's smoke is ungated (the fast gate runs it), an `ultra` case's smoke runs at `slow_test`. It performs exactly the conversion the execution case would (same mode, same module/class name) and asserts only that it completes and yields non-empty source; nothing is executed and there is no golden. So the codegen path of an app whose *run* costs minutes stays covered at a tier that costs a second or two ([ADR-54](adr/54-convert-only-app-smokes.md)). Backends declare nothing extra: the smokes come from the existing `e2e.rs` callsites, which stay byte-identical.

See `AGENTS.md`'s Common commands table for the exact invocations.

## The WASI-testsuite harness (libtest-mimic)

Alongside the spec harness, each backend has a second libtest-mimic suite (`crates/dewasm-backend-<lang>/tests/wasi_testsuite.rs`, `harness = false`, `main` from `wasi_testsuite_suite!`) that runs the official WASI p1 conformance modules from the `tests/wasi-testsuite` submodule (ADR-36). It converts each prebuilt `.wasm` in `--mode standalone` and executes it through the ADR-31 interface — the co-located `<name>.json` manifest's `args`/`env`/`root` become guest argv / child env / a `--dir <root>::/` preopen (from a fresh temp copy, so trials are hermetic), and the trial asserts the process exit code and, when pinned, stdout. Run one backend with:

```console
$ cargo test -p dewasm-backend-ruby --test wasi_testsuite   # or bash/python/go/java
```

The c + rust + assemblyscript `wasm32-wasip1` suites run (the Rust `wasm32-wasip3` tree is excluded — preview 3 is component-model territory, ADR-24). Each backend carries its own `WASI_TESTSUITE_EXPECTED_FAILURES` ledger (ADR-8): every known failure is attributed to a declared ENOSYS gap (docs/support.md), a semantics-precision gap on a supported syscall, or the ADR-31 whole-environment passthrough. As in the spec harness the ledger is checked both ways — a ledgered trial that unexpectedly *passes* is a hard failure, so filling a gap forces the entry to be removed.
