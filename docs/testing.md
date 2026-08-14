# Testing `dewasm`

What the tests need and how to run them.

## Required environment

> [!IMPORTANT]
> A test whose required tool or setup step is missing **fails**; it never silently skips.
> Run `git submodule update --init` and `examples/apps/setup.sh` before the first `cargo test`, or a fresh checkout reports failures that look like broken code.

The following tools and setup steps are required to run all tests correctly:

- **Rust toolchain**: `rustup` applies the pin in `rust-toolchain.toml` automatically; a non-rustup `cargo` ignores the pin.
- **Each backend's interpreter or toolchain**, at the version its page under [`docs/backends/`](backends/) states; a full `cargo test` needs all of them.
  * Each tool should be found under `PATH` (the `bash` lookup also tries the common Homebrew install paths).
  * These environment variables override the lookup: `$DEWASM_RUBY`, `$DEWASM_PYTHON`, `$DEWASM_PERL`, `$DEWASM_BASH`, `$DEWASM_GO`, `$DEWASM_JAVA`, `$DEWASM_JAVAC`.
- **Testsuite submodules**: initialize them once with `git submodule update --init`.
- **The `.wasm` apps cache**: initialize it once with `examples/apps/setup.sh`.
  * Cached `.wasm` files are located in `examples/apps/cache`.

## Commands

```console
$ git submodule update --init
$ examples/apps/setup.sh
$ cargo test
```

There are some features for testing:

| Feature | Description |
| --- | --- |
| `slow_test` | CI's main run: the slow app cases and the full spec testsuite. |
| `ultra_slow_test` | Implies `slow_test`; the cases CI cannot afford by wall time or memory, run in local pre-release verification. |
| `wasmtime_test` | The snapshot freshness checks, which need the `xtask` binary built; see below. |

## Snapshots

Checked-in snapshots are code-derived, never hand-written; a stale one fails a comparing test with the exact command to regenerate it.

| Snapshot | Regenerate with |
| --- | --- |
| `docs/support.md` | `cargo xtask update-support-docs` |
| every execution snapshot (`examples/apps/snapshots/*`) | `cargo xtask update-snapshots [filter]` |

A snapshot claims "this is what wasmtime produces", so it can go stale.
The opt-in freshness check reruns the cached binaries and compares against the checked-in files.
Both it and the capture embed the `wasmtime` crate pinned by `Cargo.lock` (no `wasmtime` install is involved) and reach it through the `xtask` binary, which the suite never builds for you:

```console
$ cargo build -p xtask
$ cargo test -p dewasm-test-helper --features wasmtime_test --test apps_wasmtime
```

`cargo xtask update-snapshots [filter]` captures the execution snapshots the same way, so it needs only the apps cache.
On a clean tree it must reproduce every file byte-for-byte: a resulting `git status` diff is a capture bug or genuine nondeterminism, not a routine update.
