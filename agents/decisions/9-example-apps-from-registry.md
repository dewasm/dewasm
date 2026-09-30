# Decision 9: Example Apps Fetched from Upstream, Never Committed

Status: **Accepted, 2026-07-23.**
Implemented in `examples/apps/setup.sh` and the `apps` cases of the `e2e` test.
The script's downloads are version-pinned and sha256-verified.
They land in the gitignored `examples/apps/cache/`.
The `apps` cases compare converted output against a snapshot reference.
They live in `crates/dewasm-test-helper/src/apps.rs`.
The reference was originally a live `wasmtime` diff.
Decision 15 replaced that with snapshot files checked into `examples/apps/snapshot/`.
That dropped the `wasmtime` dependency.
The fetch/pin/checksum decision below is unaffected.
Initial apps: cowsay and QuickJS, both from the Wasmer registry.
QuickJS later moved to quickjs-ng's own GitHub release once one became available upstream.
That release is a standalone WASI CLI asset.
The decision below was never registry-specific; only per-app source diversity was added.

## Context

Real applications (cowsay, a JavaScript engine) are the most convincing demos.
They are also the best end-to-end regression tests beyond the spec suite.
But committing third-party wasm binaries into this repository means *redistributing* them.
That raises a licensing question per app.
The registry often carries no license metadata at all.
It also means permanent repository bloat.

## Decision

Third-party artifacts are **never committed**.
`examples/apps/setup.sh` downloads each app from **its own upstream** into a gitignored cache.
Downloads are at **pinned versions with sha256 verification**.
The script documents each app's upstream in `examples/apps/README.md`.
That upstream is whichever source actually publishes a standalone WASI binary for the app.
It is a registry CDN (Wasmer) or a project's own release asset (quickjs-ng's GitHub releases).
Criterion: *distribution stays upstream's.*
*The repository holds only references (URL + hash).*
*Those raise no license questions and keep the supply chain auditable.*
This criterion never named a specific registry.
So per-app source diversity is a refinement, not a reversal.
The `fetch_app` helper in `examples/apps/scripts/common.sh` supports both source kinds.
Those kinds are a tarball-with-inner-path and a bare `.wasm` release asset.
Not every upstream qualifies.
sqlite.org's own "WebAssembly" download is an Emscripten browser/Node.js bundle.
`wasmtime run` cannot execute it, so sqlite stays on the Wasmer registry build.
Per decision 15, the e2e test *fails* rather than skips when the cache is absent.
`cargo test` itself stays hermetic (no network access during a normal run).
But reaching that state requires the one-time, explicit `setup.sh` step first.

App selection is constrained by the declared WASI surface (docs/support.md).
Originally that was stdio/args/environ/clock/random only.
WASI filesystem support (decision 14) later widened this for Ruby.
`wasi_unstable` (snapshot 0) is accepted as an alias of preview 1 for the implemented functions.
The original Wasmer-registry QuickJS build (since replaced) needed it.
This is a known simplification: snapshot 0's fd_seek whence encoding differs.
None of the current apps seek.

## Rejected alternatives

- **Committing the wasm binaries**: redistribution licensing per app and megabytes of history bloat.
  Registry packages often lack license metadata.
- **Building from crates.io sources at test time**: works for Rust apps.
  But it demands local toolchains (wasi-sdk for C apps like QuickJS) and long builds.
  The registry serves exactly the prebuilt artifact users would run (user's call).

## Consequences

- Positive: QuickJS (a complete C-built JS engine) converts to a ~25 MB Ruby file.
  It produces wasmtime-identical output in about a second.
  The demo story ("a JS engine on plain Ruby") costs one script run.
- Negative: the `apps` cases fail (per decision 15) rather than passing vacuously in a fresh clone.
  They fail until the explicit `setup.sh` step runs.
  CI needs that step (or a cached fetch job) before `cargo test` passes.
  Upstream availability is a fetch-time dependency, mitigated by pinning and checksums.
- Adding an app = three parts:
  - a script under `examples/apps/scripts/`;
  - a case in `crates/dewasm-test-helper/src/apps.rs`;
  - a row in the README table.
