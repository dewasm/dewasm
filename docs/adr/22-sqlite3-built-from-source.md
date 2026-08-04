# ADR-22 — Build the sqlite3 Apps From Pinned Source With zig, Both Standalone and Library

Status: **Accepted, 2026-07-24.** Implemented: `examples/apps/setup.sh` (amalgamation download
+ two `zig cc` builds), `crates/dewasm-test-helper/src/apps.rs` (`sqlite3-shell` case,
`libsqlite3_c_api_ruby`), `examples/apps/snapshot/sqlite3_shell.stdout`. Extended (Phase 5a, 2026-07-26) with a third `zig cc` build, `sqlite3-binding.wasm`, compiled from the same pinned source plus our own `examples/apps/src/sqlite3_binding.c` (an exported `run_query` that calls `sqlite3_exec` with a C callback forwarding each row to an imported `env.host_row`), which exercises the guest→host `sqlite3_exec` function-pointer callback the two original artifacts left untested — driven from Ruby's `sqlite3_callback_binding_ruby`.

## Context

The apps e2e's SQLite was a wasmer-CDN binary of SQLite 3.26.0 (2018) — a CLI shell with no C API exported, which is why the sqlite3-gem-shim milestone (the Rails north star) was blocked on "obtain a wasm32-wasi libsqlite3 that exports the C API". No upstream distributes one. `zig cc -target wasm32-wasi` compiles the current amalgamation directly, and with `-mexec-model=reactor` plus `-Wl,--export=...` produces exactly that missing artifact.

## Decision

`setup.sh` downloads the version-pinned, checksum-verified amalgamation source zip (3.53.3) and builds **two artifacts from the one source**: `sqlite3-shell.wasm` (the CLI shell, `_start` + stdio; replaces the wasmer binary in the snapshot-diffed standalone cases) and `libsqlite3.wasm` (a reactor exporting the sqlite3 C API; driven from Ruby in `libsqlite3_c_api_ruby`, exercising `_initialize`, guest-memory pointer plumbing through `sqlite3_malloc`/`Rt::Memory`, and the prepare/step/column flow the future gem shim will use). **Criterion: what is pinned is the upstream *source*, not the build product** — the ADR-9 rule ("version-pinned, checksum-verified, never committed") is unchanged, with the stamp recording the source checksum; only the producing step moved from "extract" to "compile". The library test's expectation is a fixed string rather than a wasmtime snapshot: the wasmtime CLI cannot drive a C API whose results live in guest memory, and every expected value is determined by the pinned source version.

Cost accepted: `setup.sh` (and only `setup.sh` — never `cargo test` with a warm cache) now requires `zig` and `unzip`, failing loudly per ADR-15 when absent. Build output bytes vary across zig versions; that is fine because nothing pins the *artifact* — the goldens compare program behavior, which the pinned source fixes.

## Rejected alternatives

- **Keep the wasmer-CDN binary and add a library build beside it** — two SQLites of two vintages (3.26 vs 3.53) doubles the shell coverage without adding any, and keeps a CDN dependency the source zip on sqlite.org makes unnecessary.
- **Commit the built `.wasm` artifacts** — violates ADR-9, and at ~11 MB would bloat the repository for something reproducible in seconds.
- **wasi-sdk/clang instead of zig** — works, but wasi-sdk is a versioned SDK tarball to install and point at, while zig is a single brew-installable binary with the wasi sysroot built in; zig also matches how the artifact was first validated.

## Consequences

- Positive: the north star's last technical unknown is gone — the C API round-trip (open/exec/prepare/step/column_text/finalize/close, in-memory DB) runs in pure Ruby in the always-on test (~3 s). SQLite is current (3.53.3) and its build flags are ours to change (e.g. `SQLITE_OMIT_LOAD_EXTENSION`, future VFS experiments).
- Negative / carry-over: one more tool in `setup.sh`'s requirements; the snapshot for the shell changed shape (batch-mode output — the 3.26 wasmer build printed interactive prompts). `sqlite3_exec`-style function-pointer callbacks were unexercised by the two original artifacts (the prepare/step flow avoids them); the Phase 5a `sqlite3-binding.wasm` artifact now covers exactly that guest→host callback path.

See also: [ADR-9](9-example-apps-from-registry.md) (the fetch policy this extends), [ADR-15](15-tests-fail-not-skip.md) (fail-loud tooling), [ADR-16](16-ruby-wasm1-completion.md) (the `invoke`/`memory` surface the C API glue drives).
