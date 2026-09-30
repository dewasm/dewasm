# Decision 22: Build the sqlite3 Apps From a Fixed Source Version With Zig, Standalone and Library

Status: **Accepted, 2026-07-24.**
Implemented:
- `examples/apps/setup.sh` (amalgamation fetch + two `zig cc` builds);
- `crates/dewasm-test-helper/src/apps.rs` (`sqlite3-shell` case, `libsqlite3_c_api_ruby`);
- `examples/apps/snapshot/sqlite3_shell.stdout`.

The build toolchain has since moved from Zig to `wasi-sdk` ([decision 92](92-wasi-sdk-c-toolchain.md)).
That reverses the "wasi-sdk/clang instead of zig" rejection below.
The criterion on the fixed source version, the artifact set, and the stamp policy are unchanged.

Extended (Phase 5a, 2026-07-26) with a third `zig cc` build, `sqlite3-binding.wasm`.
It is compiled from the same source version plus our own `examples/apps/src/sqlite3_binding.c`.
That file exports `run_query`, which calls `sqlite3_exec` with a C callback.
The callback forwards each row to an imported `env.host_row`.
The build exercises the guest→host `sqlite3_exec` function-pointer callback.
The two original artifacts left that callback untested.
Ruby's `sqlite3_callback_binding_ruby` drives it.

## Context

The apps e2e's SQLite was a binary from the Wasmer CDN of SQLite 3.26.0 (2018).
It was a CLI shell with no C API exported.
That is why the sqlite3-gem-shim milestone (the Rails goal) was blocked.
The blocker was "obtain a wasm32-wasi libsqlite3 that exports the C API".
No upstream distributes one.
`zig cc -target wasm32-wasi` compiles the current amalgamation directly.
With `-mexec-model=reactor` plus `-Wl,--export=...` it produces exactly that missing artifact.

## Decision

`setup.sh` fetches the amalgamation source ZIP at version 3.53.3 and checks its fixed checksum.
It builds **two artifacts from the one source**:

- `sqlite3-shell.wasm`: the CLI shell, `_start` + stdio.
  It replaces the Wasmer binary in the snapshot-diffed standalone cases.
- `libsqlite3.wasm`: a reactor exporting the sqlite3 C API, driven from Ruby in `libsqlite3_c_api_ruby`.
  It exercises `_initialize` and guest-memory pointer passing through `sqlite3_malloc`/`Rt::Memory`.
  It also exercises the prepare/step/column flow the future gem shim will use.

**Criterion: what is held at a fixed version is the upstream *source*, not the build product**.
The decision 9 rule ("version-pinned, checksum-verified, never committed") is unchanged.
The stamp records the source checksum; only the producing step moved from "extract" to "compile".
The library test's expectation is a fixed string rather than a Wasmtime snapshot.
The `wasmtime` CLI cannot drive a C API whose results live in guest memory.
Every expected value is determined by the source version.

Cost accepted: `setup.sh` now requires `zig` and `unzip`, failing loudly per decision 15 when missing.
Only `setup.sh` requires them, never `cargo test` with a warm cache.
Build output bytes vary across Zig versions.
That is fine because no checksum covers the *artifact*.
The snapshots compare program behavior, which the source version decides.

## Rejected alternatives

- **Keep the Wasmer CDN binary and add a library build beside it**: two SQLite releases years apart.
  3.26 vs. 3.53 doubles the shell coverage without adding any.
  It also keeps a CDN dependency the source ZIP on sqlite.org makes unnecessary.
- **Commit the built `.wasm` artifacts**: breaks decision 9.
  At ~11 MB it would grow the repository for something reproducible in seconds.
- **`wasi-sdk`/`clang` instead of Zig**: works.
  But `wasi-sdk` is a versioned SDK tarball to install and point at.
  Zig is a single binary that Homebrew can install, with the WASI system root built in.
  Zig also matches how the artifact was first validated.

## Consequences

- Positive: the stated goal's last technical unknown is gone.
  The C API round-trip runs in pure Ruby in the always-on test (~3 s).
  That round-trip is `open`/`exec`/`prepare`/`step`/`column_text`/`finalize`/`close`, on an in-memory DB.
  SQLite is current (3.53.3) and its build flags are ours to change.
  Examples are `SQLITE_OMIT_LOAD_EXTENSION` and future VFS experiments.
- Negative / carry-over: one more tool in `setup.sh`'s requirements.
  The snapshot for the shell changed shape (batch-mode output).
  The 3.26 Wasmer build printed interactive prompts.
  The two original artifacts left `sqlite3_exec`-style function-pointer callbacks unexercised.
  The prepare/step flow avoids them.
  The Phase 5a `sqlite3-binding.wasm` artifact now covers exactly that guest→host callback path.

See also:

- [decision 9](9-example-apps-from-registry.md) (the fetch policy this extends);
- [decision 15](15-tests-fail-not-skip.md) (fail-loud tooling);
- [decision 16](16-ruby-wasm1-completion.md) (the `invoke`/`memory` surface the C API glue drives).
