# Decision 100: Each `xtask` Command Group Is Its Own `xtask-<name>` Crate

Status: **Accepted, 2026-10-01.**
The `xtask` binary only selects a command; each command lives in a library crate named `xtask-<name>`.
The crates are `xtask-snapshot`, `xtask-support-docs`, and `xtask-feature-audit`.
The other two are `xtask-records` and `xtask-text-check`.
The list of commands is in [`crates/xtask/src/main.rs`](../../crates/xtask/src/main.rs).

## Context

Every developer command lived in one `xtask` crate.
Its tests and lints built `wasmtime` and every backend, even for a change to the record renderer.
The text check was a separate crate named `text-check`, while its command sat in `xtask`.
New commands keep coming, and each one needs a place.

The commands differ in what they build on:

| Command group | Heavy dependencies |
| --- | --- |
| `update-snapshots`, `test-wasmtime-*` | `wasmtime`, `wasmtime-wasi`, `png` |
| `update-support-docs` | every backend |
| `feature-audit` | `wasmparser` |
| `record-*`, `render-*`, `migrate-records` | every backend, `serde` |
| `check-text` | `pulldown-cmark` |

## Decision

A command goes into the crate whose internal items it uses.
When it uses none, it gets a new crate `crates/xtask-<name>`.
So a crate holds the commands that share code, and no item is made public only to cross a crate.

Each crate is a library that exposes one function per command, taking the remaining arguments.
`xtask` keeps its package and binary name and only maps a command name to that function.
The test helper and CI start the snapshot runner as the `xtask` binary, so the name stays.
A new crate is added to the `cargo test -p` list in `.github/workflows/ci.yml`.
That list names packages, so a crate missing from it has its tests skipped in CI.

## Rejected alternatives

- **One `xtask` crate.**
  A test of any one command builds `wasmtime` and every backend.
- **One crate per command.**
  `record-size` uses about ten items of the speed record code: runners, charts, and paths.
  `migrate-records` reads both record kinds.
  Separate crates would make all of those public for no other reader.
- **`xtask-bench` and `xtask-size` as two crates.**
  The dependency would only go one way, but `migrate-records` still needs both.
  All three write or read `records/`, so `xtask-records` holds them.
- **Separate binaries per crate.**
  `cargo xtask` would need one alias per binary, and the test helper finds one binary by name.

## Consequences

- Positive: `cargo test -p xtask-records` and the other crates build without `wasmtime`.
- Positive: a new command has a stated place.
- Negative: `cargo xtask` still builds every crate, so running a command costs as before.
- Negative: the REPL snapshot capture starts the running binary as `test-wasmtime-wasi`.
  So `xtask-snapshot` depends on the binary that calls it to route that command back to it.
