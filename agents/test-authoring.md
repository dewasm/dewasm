# Test authoring

How dewasm's test suites are structured, and the conventions a new case follows.
How to run them, and what each one needs installed, is in [`docs/testing.md`](../docs/testing.md).

## Test layout

Tests live with the one backend they exercise.
Only a test that needs *every* backend lives centrally.
The shared harness, case tables, and the per-feature test macros are in `crates/dewasm-test-helper`.
That crate depends only on `dewasm-core` + `dewasm-backend`, never on a concrete backend.
The `spec`, `convert`, and WASI-testsuite suites are [`libtest-mimic`](https://crates.io/crates/libtest-mimic) harnesses (`harness = false`).
They list their inputs at runtime as named trials.

- **`crates/dewasm-backend-<lang>/tests/spec.rs`**: that backend's `spec` harness, set up with `spec_suite!`.
  It holds its `SpecBackend` implementation and its `EXPECTED_FAILURES` list.
  For Bash, it also holds the selected file list.
- **`crates/dewasm-backend-<lang>/tests/convert.rs`**: that backend's whole-cache convert suite.
  It is a one-line `apps_convert_suite!(<Backend>)` call; the manifest and the harness are shared.
  It covers the (backend × app) pairs the execution e2e suites never run (decision 54).
- **`crates/dewasm-backend-<lang>/tests/wasi_testsuite.rs`**: that backend's WASI-testsuite harness.
  Its `main` comes from `wasi_testsuite_suite!`.
  It holds its `WASI_TESTSUITE_EXPECTED_FAILURES` list.
  It runs the `c` + `rust` + `assemblyscript` `wasm32-wasip1` trees.
  The Rust `wasm32-wasip3` tree is excluded, since the component model is out of scope.
  Each trial converts the prebuilt `.wasm` of the upstream testsuite in `--mode standalone`.
  It executes the output through the standalone interface.
  The manifest beside each trial gives its `args`/`env`/`root`.
  They become guest `argv`, child environment, and a `--dir` preopen.
  The preopen comes from a fresh temporary copy, so no trial affects another.
- **`crates/dewasm-backend-<lang>/tests/e2e.rs`**: that backend's suites.
  They are declared by calling the shared macros.
  The contract is below.
- The units lint lives in a `#[cfg(test)] mod units` at the bottom of each backend's `src/lib.rs`.
  Run it with `cargo test -p dewasm-backend-<lang> --lib`.
  Its unit tests are these:
  - `declared_requires_cover_references`;
  - `all_units_bundle`;
  - the Go/Java whole-bundle compile checks.

  **`softfloat.rs`** (Bash) is the one backend-local integration oracle.
- **`crates/dewasm/tests/`**: black-box tests of the `dewasm` binary's option handling.
  They are `module_name.rs`, `data_file.rs`, and `dwarf_line.rs`.
  Each starts the binary via `CARGO_BIN_EXE_dewasm`.
- The `docs/support.md` freshness check is the `support_docs_in_sync` unit test.
  It fails while the generated file is out of date, over all backends.
  It sits in `crates/xtask-support-docs/src/lib.rs`.
  That file also holds the `update-support-docs` command, which regenerates `docs/support.md`.
- **`crates/dewasm-test-helper/tests/apps_wasmtime.rs`**: Wasmtime as a `BackendUnderTest`.
  It runs the `apps`/`gzip`/`fs_apps` snapshot-freshness checks through the shared runners.
  It also runs `qjs_repl_interactive_snapshot`.
  That case re-captures the plain `qjs` REPL under a pseudo-terminal.
  It compares the result to the checked-in `.transcript` file.
  It also runs `doom_frame`/`nes_frame`, which are compare-only.
  Regenerate their snapshots with `cargo xtask update-snapshots`.
  Every case reaches Wasmtime through the `xtask` binary, so build it first (`cargo build -p xtask`).
  All behind the `wasmtime_test` feature, named for a future engine such as Wasmer/WasmEdge joining it.

## The `e2e.rs` contract

The file contains **only** these items:

- the `BackendUnderTest` implementation;
- named glue string constants:
  - the library glue;
  - the WASI-filesystem template;
  - the file system app and C-API driver glue;
  - the multi-module glue with its `compose_modules`/`run_in_dir` implementations;
- macro calls.

It holds no backend-specific `#[test]` function.
It holds no glue-returning function or `match` on a case name.

Which macros a backend calls is its capability declaration.
A case it cannot run is not called, with the reason as a comment at the missing callsite.

A glue constant cannot know some runtime paths statically.
Those paths are `{scratch}`/`{cache}`/`{guest}`/`{host}` placeholders, which the runner fills (`glue::fill`).

### Shared file system and C-API cases

The app cases that exercise the file system are shared by every backend with file system support.
They are `pub const` cases in `crates/dewasm-test-helper/src/apps_fs.rs`:

- the QuickJS file-I/O case;
- the sqlite3 DB-file case;
- `ripgrep`;
- the CPython/CRuby runtime examples.

Each is driven by its own per-case macro (`qjs_file_io_e2e!` … `cruby_hello_e2e!`).
The sqlite3 C-API / callback drives live in `apps_capi.rs`.
They run via `libsqlite3_c_api_e2e!` and the related macros.
The QuickJS REPL is covered separately, under a real pseudo-terminal (see below).
Each backend supplies only a named glue string constant per case.
The constant writes the class name, `argv`, environment, and preopen *guest* paths literally.
The runtime host paths in it are substituted from `{scratch}`/`{cache}` placeholders.
At present every backend calls every file system and C-API case.
Some of them are in the `ultra` category.

The committed driver fixtures (the `.js` scripts) live in `examples/apps/fixtures/`.
The file system app snapshots are still captured from `wasmtime`.
The C-API drives have no snapshot, since their results live in guest memory.
Each of those drives therefore checks against a fixed string.

### Category tokens

A slow case carries a trailing category token, `slow` by default or `ultra`.
The token decides which feature of the expanding crate un-ignores the generated `#[test]`.
`slow` maps to `slow_test`, and `ultra` to `ultra_slow_test`.

Every `ultra` case is marked at its callsite with a comment.
The comment gives the reason and, where there is one, the issue number.
No case is lost this way: each `ultra` case runs at `slow` on at least one other backend.
So CI still covers the case itself.
What the token leaves out is that one backend's run of it.

## The interactive REPL case under a pseudo-terminal (`qjs_repl_pty`)

`qjs_repl_pty_e2e!` drives the *plain* QuickJS REPL under a real pseudo-terminal.
With no script argument, `qjs` enters the interactive line editor.
The pseudo-terminal comes from `crates/dewasm-test-helper/src/pty.rs` and `portable-pty`.
The case requires its output, ANSI escapes and all, to be byte-identical to Wasmtime's.
Wasmtime's output is checked in as `examples/apps/snapshots/qjs_repl_interactive.transcript`.
`qjs` only enters that path when `fd_fdstat_get` on `stdin` reports a character device.
A pipe does not report one, so a pseudo-terminal is required.
The scripted session is *prompt-driven*: each line is sent only after the `qjs > ` prompt reappears.
So the captured output is stable however long a backend takes to start.
It is slow on every backend and `ultra` on Bash, where it timed out on CI (#22).

## Adding a new backend

To add a new backend to the e2e suites, implement `BackendUnderTest` in the new crate.
Implement `SpecBackend` too for the `spec` harness.
Then call the macros for the suites it participates in.

## Changing an app's version

After changing an app's fixed version in `examples/apps/setup.sh`:

1. Re-run `setup.sh`.
2. Regenerate with `cargo xtask update-snapshots [filter]`.
3. Re-run the `wasmtime_test` freshness suite.
4. If the exit status changed, update the app's `expect_code` in `crates/dewasm-test-helper/src/apps.rs`.

Every snapshot is captured through the `wasmtime` crate embedded in the `xtask` binary.
That crate is a dependency of `xtask-snapshot` only.
The DOOM and NES custom-import interfaces are not WASI commands.
Their frames therefore go through their own capture code rather than the WASI runner.
After a change to the DOOM version, re-run the per-backend `doom_frame` cases.

## The `EXPECTED_FAILURES` lists

The `spec` trial checks per-file failure counts against each backend's `EXPECTED_FAILURES` list.
A failing trial means a semantics bug: fix the cause.
Extending that list in the backend's `tests/spec.rs` is a last resort.
It requires an attribution tag plus a reason.

Each backend carries its own `WASI_TESTSUITE_EXPECTED_FAILURES` list too.
Every known failure there is attributed to one of three causes:

- a declared ENOSYS gap (`docs/support.md`);
- a semantics-precision gap on a supported system call;
- the standalone interface passing the whole environment through.

Both lists are checked both ways: a listed trial that unexpectedly *passes* is a hard failure.
So filling a gap forces the entry to be removed.
