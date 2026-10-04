# Testing `dewasm`

What the tests need and how to run them.

## Required environment

> [!IMPORTANT]
> A test whose required tool or set-up step is missing **fails**; it never silently skips.
> Before the first `cargo test`, run the four commands that precede it under [Commands](#commands).
> Otherwise a fresh clone of the repository reports failures that look like broken code.

The following tools and set-up steps are required to run all tests correctly:

- **Rust toolchain**: `rustup` applies the version fixed in `rust-toolchain.toml` automatically.
  A `cargo` not installed by `rustup` ignores that version.
- **Every other tool at a fixed version**: [`mise`](https://mise.jdx.dev) installs them with `mise install`.
  `mise.toml` states each version, and `mise.lock` holds the URL and checksum of each fetched file.
  CI installs from the same two files.
  * The tools are Ruby, Python, Go, Java, Codon, `wasi-sdk`, Binaryen, `wasi-vfs`, and `shellcheck`.
    The host provides Bash and Perl.
  * Run the commands in a shell where `mise` is active, or put `mise exec --` in front of each.
    Each tool is then on `PATH`, and `WASI_SDK_PATH` is set.
  * `mise` is not required.
    Without it, install each tool yourself at the version `mise.toml` states.
- **Each backend's interpreter or toolchain**, at the version its page under [`docs/backends/`](backends/) states.
  A full `cargo test` needs all of them.
  * Each tool should be found under `PATH` (the `bash` lookup also tries the common Homebrew install paths).
  * These environment variables override the lookup:
    - `ruby`: `$DEWASM_RUBY`
    - `python`: `$DEWASM_PYTHON`
    - `perl`: `$DEWASM_PERL`
    - `bash`: `$DEWASM_BASH`
    - `go`: `$DEWASM_GO`
    - `java`: `$DEWASM_JAVA`, `$DEWASM_JAVAC`
    - `codon`: `$DEWASM_CODON`
- **Testsuite submodules**: initialize them once with `git submodule update --init`.
- **The `.wasm` apps cache**: initialize it once with `examples/apps/setup.sh`.
  Re-run it after pulling a change that moves an app to a new version.
  That is because a cached copy of the previous version is a different program.
  `examples/apps/setup.sh --check` names any that are out of date without fetching.
  * Cached `.wasm` files are located in `examples/apps/cache`.
  * Building some apps from source needs more:
    - [`wasi-sdk`](https://github.com/WebAssembly/wasi-sdk/releases) with `WASI_SDK_PATH` set to its root, for sqlite3 and others.
    - Binaryen and `wasi-vfs`, for the modules built or packed here.
    - A host Ruby with `rake`, for `mruby`.
  * Each `scripts/*.sh` fails loudly naming what it is missing.
- **The word lists of the text check**: fetch them once with `crates/xtask-text-check/setup.sh`.
  `cargo test -p xtask-text-check` reads them from `crates/xtask-text-check/cache`.
  `crates/xtask-text-check/setup.sh --check` names any that do not match their sha256.

## Commands

```console
$ mise install
$ git submodule update --init
$ examples/apps/setup.sh
$ crates/xtask-text-check/setup.sh
$ cargo test
```

There are some features for testing:

| Feature | Description |
| --- | --- |
| `slow_test` | CI's main run: the slow app cases and the full specification testsuite (Codon, whose per-file cost is a compile, runs a selected specification list here and its full run only under `ultra_slow_test`). |
| `ultra_slow_test` | Implies `slow_test`; the cases CI cannot afford by wall time or memory, run in local pre-release verification. |
| `wasmtime_test` | The snapshot freshness checks, which need the `xtask` binary built; see below. |

## Snapshots

Checked-in snapshots are code-derived, never hand-written.
An out-of-date one fails a comparing test with the exact command to regenerate it.

| Snapshot | Regenerate with |
| --- | --- |
| `docs/support.md` | `cargo xtask update-support-docs` |
| every execution snapshot (`examples/apps/snapshots/*`) | `cargo xtask update-snapshots [filter]` |

A snapshot claims "this is what wasmtime produces", so it can go out of date.
The optional freshness check reruns the cached binaries and compares against the checked-in files.
Both it and the capture embed the `wasmtime` crate at the version `Cargo.lock` states.
No `wasmtime` install is involved.
They reach it through the `xtask` binary, which the suite never builds for you:

```console
$ cargo build -p xtask
$ cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime
```

`cargo xtask update-snapshots [filter]` captures the execution snapshots the same way.
So it needs only the apps cache.
On a clean tree it must reproduce every file with identical bytes.
A resulting `git status` diff is not a routine update.
It is a capture bug or output that varies from run to run.
