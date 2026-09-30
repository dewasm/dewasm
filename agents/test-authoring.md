# Test authoring

How dewasm's test suites are structured, and the conventions a new case follows.
How to run them, and what each one needs installed, is in [`docs/testing.md`](../docs/testing.md).

## Test layout

Tests live with the one backend they exercise.
Only a test that needs *every* backend lives centrally.
The shared harness, case tables, and the per-feature test macros are in `crates/dewasm-test-helper`.
That crate depends only on `dewasm-core` + `dewasm-backend`, never on a concrete backend.
The spec, convert, and WASI-testsuite suites are [libtest-mimic](https://crates.io/crates/libtest-mimic) harnesses (`harness = false`).
They enumerate their inputs at runtime into named trials.

- **`crates/dewasm-backend-<lang>/tests/spec.rs`**: that backend's spec harness, set up with `spec_suite!`.
  It holds its `SpecBackend` impl and its `EXPECTED_FAILURES` list.
  For bash, it also holds the curated file list.
- **`crates/dewasm-backend-<lang>/tests/convert.rs`**: that backend's whole-cache convert suite.
  It is a one-line `apps_convert_suite!(<Backend>)` invocation; the manifest and harness are shared.
  It covers the (backend × app) pairs the execution e2e suites never run (decision 54).
- **`crates/dewasm-backend-<lang>/tests/wasi_testsuite.rs`**: that backend's WASI-testsuite harness.
  Its `main` comes from `wasi_testsuite_suite!`.
  It holds its `WASI_TESTSUITE_EXPECTED_FAILURES` list.
  It runs the c + rust + assemblyscript `wasm32-wasip1` trees.
  The Rust `wasm32-wasip3` tree is excluded, since the component model is out of scope.
  Each trial converts the prebuilt `.wasm` in `--mode standalone`.
  It executes the output through the standalone interface.
  The co-located manifest's `args`/`env`/`root` become guest argv, child env, and a `--dir` preopen.
  The preopen comes from a fresh temp copy, so trials are hermetic.
- **`crates/dewasm-backend-<lang>/tests/e2e.rs`**: that backend's suites.
  They are declared by invoking the shared macros.
  The contract is below.
- The units lint lives in a `#[cfg(test)] mod units` at the bottom of each backend's `src/lib.rs`.
  Run it with `cargo test -p dewasm-backend-<lang> --lib`.
  Its unit tests are these:
  - `declared_requires_cover_references`;
  - `all_units_bundle`;
  - the go/java whole-bundle compile checks.

  **`softfloat.rs`** (bash) is the one backend-local integration oracle.
- **`crates/dewasm/tests/`**: black-box tests of the `dewasm` binary's option handling.
  They are `module_name.rs`, `data_file.rs`, and `dwarf_line.rs`.
  Each spawns the binary via `CARGO_BIN_EXE_dewasm`.
- The `docs/support.md` freshness check is the `support_docs_in_sync` unit test.
  It fails while the generated file is stale, over all backends.
  It sits in `crates/xtask/src/support_docs.rs`.
  That file also holds the `update-support-docs` command, which regenerates `docs/support.md`.
- **`crates/dewasm-test-helper/tests/apps_wasmtime.rs`**: wasmtime as a `BackendUnderTest`.
  It runs the `apps`/`gzip`/`fs_apps` snapshot-freshness checks through the shared runners.
  It also runs `qjs_repl_interactive_snapshot`.
  That case re-captures the bare qjs REPL under a pty and compares it to the checked-in transcript.
  It also runs `doom_frame`/`nes_frame`, which are compare-only.
  Regenerate their snapshots with `cargo xtask update-snapshots`.
  Every case reaches wasmtime through the `xtask` binary, so build it first (`cargo build -p xtask`).
  All behind the `wasmtime_test` feature, named for a future engine such as wasmer/wasmedge joining it.

## The `e2e.rs` contract

The file contains **only** these items:

- the `BackendUnderTest` impl;
- named glue string constants:
  - the library glue;
  - the WASI-filesystem template;
  - the filesystem-app and C-API driver glue;
  - the multi-module glue with its `compose_modules`/`run_in_dir` impls;
- macro invocations.

It holds no backend-specific `#[test]` function.
It holds no glue-returning function or `match` on a case name.

Which macros a backend invokes is its capability declaration.
A case it cannot run is not invoked, with the reason as a comment at the absent callsite.

A glue const cannot know some runtime paths statically.
Those paths are `{scratch}`/`{cache}`/`{guest}`/`{host}` placeholders, which the runner fills (`glue::fill`).

### Shared filesystem and C-API cases

The filesystem-exercising app cases are shared across every fs-capable backend.
They are `pub const` cases in `crates/dewasm-test-helper/src/apps_fs.rs`:

- the QuickJS file-I/O case;
- the sqlite3 DB-file case;
- ripgrep;
- the CPython/CRuby runtime demos.

Each is driven by its own per-case macro (`qjs_file_io_e2e!` … `cruby_hello_e2e!`).
The sqlite3 C-API / callback drives live in `apps_capi.rs`.
They run via `libsqlite3_c_api_e2e!` and the related macros.
The QuickJS REPL is covered separately, under a real pty (see below).
Each backend supplies only a named glue string constant per case.
The constant writes the class name, argv, env, and preopen *guest* paths literally.
The runtime host paths in it are substituted from `{scratch}`/`{cache}` placeholders.
At present every backend invokes every filesystem and C-API case.
Some of them are in the `ultra` category.

The committed driver fixtures (the `.js` scripts) live in `examples/apps/fixtures/`.
The filesystem-app snapshots are still captured from `wasmtime`.
The C-API drives have no snapshot, since their results live in guest memory.
Each of those drives therefore pins a fixed string.

### Category tokens

A slow case carries a trailing category token, `slow` by default or `ultra`.
The token decides which feature of the expanding crate un-ignores the generated `#[test]`.
`slow` maps to `slow_test`, and `ultra` to `ultra_slow_test`.

Every `ultra` case is pinned at its callsite with a comment.
The comment gives the reason and, where there is one, the issue number.
No case is lost this way: each `ultra` case runs at `slow` on at least one other backend.
CI then still covers the case itself.
What the token withholds is that one backend's run of it.

## The interactive-REPL pty case (`qjs_repl_pty`)

`qjs_repl_pty_e2e!` drives the *bare* QuickJS REPL under a real pty.
With no script arg, qjs enters the interactive line editor.
The pty comes from `crates/dewasm-test-helper/src/pty.rs` and `portable-pty`.
The case requires its transcript, ANSI escapes and all, to be byte-identical to wasmtime's.
wasmtime's transcript is checked in as `examples/apps/snapshots/qjs_repl_interactive.transcript`.
qjs only enters that path when `fd_fdstat_get` on stdin reports a character device.
A pipe does not report one, so a pty is required.
The scripted session is *prompt-driven*: each line is sent only after the `qjs > ` prompt reappears.
The transcript is then stable however long a backend takes to start.
It is slow on every backend and `ultra` on bash, where it timed out on CI (#22).

## Onboarding a new backend

To onboard a new backend to the e2e suites, implement `BackendUnderTest` in the new crate.
Implement `SpecBackend` too for the spec harness.
Then invoke the macros for the suites it participates in.

## Re-pinning an app

After bumping a pin in `examples/apps/setup.sh`:

1. Re-run `setup.sh`.
2. Regenerate with `cargo xtask update-snapshots [filter]`.
3. Re-run the `wasmtime_test` freshness suite.
4. If the exit status changed, update the app's `expect_code` in `crates/dewasm-test-helper/src/apps.rs`.

Every snapshot is captured through the `wasmtime` crate embedded in xtask (an xtask-only dependency).
The DOOM and NES custom-import interfaces are not WASI commands.
Their frames therefore go through their own capture code rather than the WASI runner.
After a doom pin bump, re-run the per-backend `doom_frame` cases.

## The `EXPECTED_FAILURES` lists

The spec trial checks per-file failure counts against each backend's `EXPECTED_FAILURES` list.
A failing trial means a semantics bug: fix the cause.
Extending that list in the backend's `tests/spec.rs` is a last resort.
It requires an attribution tag plus a reason.

Each backend carries its own `WASI_TESTSUITE_EXPECTED_FAILURES` list too.
Every known failure there is attributed to one of three causes:

- a declared ENOSYS gap (`docs/support.md`);
- a semantics-precision gap on a supported syscall;
- the standalone interface's whole-environment passthrough.

Both lists are checked both ways: a listed trial that unexpectedly *passes* is a hard failure.
Filling a gap then forces the entry to be removed.
