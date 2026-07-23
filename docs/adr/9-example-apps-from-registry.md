# ADR-9 — Example Apps Fetched from the Wasmer Registry, Never Committed

Status: **Accepted, 2026-07-23.** Implemented: `examples/apps/fetch.sh`
(version-pinned, sha256-verified downloads into the gitignored
`examples/apps/cache/`) and the `apps` e2e test
(`crates/dewasmify-cli/tests/apps.rs`) comparing converted output against
wasmtime. Initial apps: cowsay and QuickJS.

## Context

Real applications (cowsay, a JavaScript engine) are the most convincing
demos and the best end-to-end regression tests beyond the spec suite.
But committing third-party wasm binaries into this repository means
*redistributing* them — a licensing question per app (the registry often
carries no license metadata at all) plus permanent repository bloat.

## Decision

Third-party artifacts are **never committed**. `examples/apps/fetch.sh`
downloads packages from the Wasmer registry CDN at **pinned versions
with sha256 verification**, extracts the `.wasm` into a gitignored
cache, and documents each app's upstream in `examples/apps/README.md`.
Criterion: *distribution stays upstream's; the repository holds only
references (URL + hash), which raise no license questions and keep the
supply chain auditable.* The e2e test self-skips when the cache or the
reference runtime (wasmtime) is absent, so `cargo test` stays hermetic.

App selection is constrained by the declared WASI surface
(docs/support.md): stdio/args/environ/clock/random only, until
`path_open` lands. `wasi_unstable` (snapshot 0) is accepted as an alias
of preview 1 for the implemented functions — QuickJS's 2019 build needs
it (known simplification: snapshot 0's fd_seek whence encoding differs;
none of the current apps seek).

## Rejected alternatives

- **Committing the wasm binaries** — redistribution licensing per app,
  registry packages often lack license metadata, and megabytes of
  history bloat.
- **Building from crates.io sources at test time** — works for Rust
  apps but demands local toolchains (wasi-sdk for C apps like QuickJS)
  and long builds; the registry serves exactly the prebuilt artifact
  users would run (user's call).

## Consequences

- Positive: QuickJS — a complete C-built JS engine — converts to a
  21 MB Ruby file and produces wasmtime-identical output in ~0.8 s; the
  demo story ("a JS engine on plain Ruby") costs one script run.
- Negative: tests depending on the cache don't run by default in a fresh
  clone (explicit `fetch.sh` step; CI can add a cached fetch job later).
  Registry availability is a fetch-time dependency, mitigated by pinning
  and checksums.
- Adding an app = one line in `fetch.sh` + a case in `apps.rs` + a row
  in the README table.
