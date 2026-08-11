# Test authoring

How dewasm's test suites are structured, and the conventions a new case follows.
How to run them, and what each one needs installed, is in [`docs/testing.md`](../docs/testing.md).

## Test layout

Tests live with the one backend they exercise; only a test that needs *every* backend lives centrally.
The shared harness, case tables, and the per-feature test macros are in `crates/dewasm-test-helper`, which depends only on `dewasm-core` + `dewasm-backend` (never on a concrete backend).
The spec, convert, and WASI-testsuite suites are [libtest-mimic](https://crates.io/crates/libtest-mimic) harnesses (`harness = false`), enumerating their inputs at runtime into named trials.

- **`crates/dewasm-backend-<lang>/tests/spec.rs`**: that backend's spec harness, holding its `SpecBackend` impl and its `EXPECTED_FAILURES` list (and, for bash, the curated file list), wired up with `spec_suite!`.
- **`crates/dewasm-backend-<lang>/tests/convert.rs`**: that backend's whole-cache convert suite, a one-line `apps_convert_suite!(<Backend>)` invocation (the manifest and harness are shared); it covers the (backend × app) pairs the execution e2e suites never run (decision 54).
- **`crates/dewasm-backend-<lang>/tests/wasi_testsuite.rs`**: that backend's WASI-testsuite harness, `main` from `wasi_testsuite_suite!`, holding its `WASI_TESTSUITE_EXPECTED_FAILURES` list.
  It runs the c + rust + assemblyscript `wasm32-wasip1` trees; the Rust `wasm32-wasip3` tree is excluded as component-model territory, out of scope.
  Each trial converts the prebuilt `.wasm` in `--mode standalone` and executes it through the standalone interface: the co-located manifest's `args`/`env`/`root` become guest argv, child env, and a `--dir` preopen from a fresh temp copy, so trials are hermetic.
- **`crates/dewasm-backend-<lang>/tests/e2e.rs`**: that backend's suites, declared by invoking the shared macros; see the contract below.
- The units lint (`declared_requires_cover_references`, `all_units_bundle`, and the go/java whole-bundle compile checks) lives as `#[cfg(test)] mod units` unit tests at the bottom of each backend's `src/lib.rs`, run with `cargo test -p dewasm-backend-<lang> --lib`.
  **`softfloat.rs`** (bash) is the one backend-local integration oracle.
- **`crates/dewasm-cli/tests/`**: only `support_docs.rs` (the freshness check that fails while the generated `docs/support.md` is stale, over all backends).
- **`crates/dewasm-test-helper/tests/apps_wasmtime.rs`**: wasmtime as a `BackendUnderTest`, running the `apps`/`gzip`/`fs_apps` snapshot-freshness checks through the shared runners, plus `qjs_repl_interactive_snapshot`, which re-captures the bare qjs REPL under a pty from a live wasmtime and compares it to the checked-in transcript (compare-only; regenerate with `cargo xtask update-snapshots`).
  All behind the `wasmtime_test` feature, named for a future engine such as wasmer/wasmedge joining it.

## The `e2e.rs` contract

The file contains **only** the `BackendUnderTest` impl, named glue string constants (library glue, the WASI-filesystem template, the filesystem-app and C-API driver glue, the multi-module glue with its `compose_modules`/`run_in_dir` impls), and macro invocations: no backend-specific `#[test]` function, no glue-returning function or `match` on a case name.

Which macros a backend invokes is its capability declaration; a case it cannot run is simply not invoked, with the reason as a comment at the absent callsite.

Runtime paths a glue const cannot know statically are `{scratch}`/`{cache}`/`{guest}`/`{host}` placeholders the runner fills (`glue::fill`).

### Shared filesystem and C-API cases

The filesystem-exercising app cases are shared across every fs-capable backend: the QuickJS file-I/O case, the sqlite3 DB-file case, ripgrep, and the CPython/CRuby runtime demos are `pub const` cases in `crates/dewasm-test-helper/src/apps_fs.rs`, each driven by its own per-case macro (`qjs_file_io_e2e!` … `cruby_hello_e2e!`); the sqlite3 C-API / callback drives live in `apps_capi.rs`, run via `libsqlite3_c_api_e2e!` and friends.
(The QuickJS REPL is covered separately, under a real pty; see below.) Each backend supplies only a named glue string constant per case (class name, argv, env, and preopen *guest* paths written literally; the runtime host paths substituted from `{scratch}`/`{cache}` placeholders).
At present every backend invokes every filesystem and C-API case, some of them at the ultra speed.

The committed driver fixtures (the `.js` scripts) live in `examples/apps/fixtures/`; the filesystem-app snapshots are still captured from `wasmtime`, while the C-API drives have none (their results live in guest memory), so each of those pins a fixed string.

### Speed tokens

A slow case carries a trailing speed token, `slow` by default or `ultra`, which decides whether the expanding crate's `slow_test` or `ultra_slow_test` feature un-ignores the generated `#[test]`.

Every `ultra` case is pinned at its callsite, with a comment giving the reason and, where there is one, the issue number.
No case is lost this way: each `ultra` case runs at `slow` on at least one other backend, so CI still covers the case itself; what the token withholds is that one backend's run of it.

## The interactive-REPL pty case (`qjs_repl_pty`)

`qjs_repl_pty_e2e!` drives the *bare* QuickJS REPL (no script arg, so the interactive line editor) under a real pty (`crates/dewasm-test-helper/src/pty.rs`, `portable-pty`) and requires its transcript, ANSI escapes and all, to be byte-identical to the one wasmtime produces (`examples/apps/snapshots/qjs_repl_interactive.transcript`).
A pty is required because qjs only enters that path when `fd_fdstat_get` on stdin reports a character device; a pipe does not.
The scripted session is *prompt-driven*: each line is sent only after the `qjs > ` prompt reappears, so the transcript is stable however long a backend takes to start.
It is slow on every backend and `ultra` on bash, where it timed out on CI (#22).

## Onboarding a new backend

Onboarding a new backend to the e2e suites is: implement `BackendUnderTest` (and `SpecBackend` for the spec harness) in the new crate, then invoke the macros for the suites it participates in.

## Re-pinning an app

After bumping a pin in `examples/apps/setup.sh`: re-run `setup.sh`, regenerate with `cargo xtask update-snapshots [filter]`, re-run the `wasmtime_test` freshness suite, and update the app's `expect_code` in `crates/dewasm-test-helper/src/apps.rs` if the exit status changed.
The DOOM frame is the one snapshot not captured through the `wasmtime` CLI: `doom.wasm`'s custom-import interface does not run under `wasmtime run`, so `update-snapshots` drives it with the embedded `wasmtime` crate (an xtask-only dependency); after a doom pin bump, re-run the per-backend `doom_frame` cases.

## The `EXPECTED_FAILURES` lists

Per-file failure counts are checked against each backend's `EXPECTED_FAILURES` list inside the spec trial.
A failing trial means a semantics bug: fix the cause.
Extending that list (in the backend's `tests/spec.rs`) is a last resort and requires an attribution tag plus a reason.

Each backend carries its own `WASI_TESTSUITE_EXPECTED_FAILURES` list too: every known failure is attributed to a declared ENOSYS gap (`docs/support.md`), a semantics-precision gap on a supported syscall, or the standalone interface's whole-environment passthrough.
Both lists are checked both ways: a listed trial that unexpectedly *passes* is a hard failure, so filling a gap forces the entry to be removed.
