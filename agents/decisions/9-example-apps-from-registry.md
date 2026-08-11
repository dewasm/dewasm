# Decision 9: Example Apps Fetched from Upstream, Never Committed

Status: **Accepted, 2026-07-23.**
Implemented: `examples/apps/setup.sh` (version-pinned, sha256-verified downloads into the gitignored `examples/apps/cache/`) and the `apps` cases of the `e2e` test (`crates/dewasm-test-helper/src/apps.rs`) comparing converted output against a snapshot reference (originally a live `wasmtime` diff; decision 15 replaced that with snapshot files checked into `examples/apps/snapshot/`, dropping the `wasmtime` dependency; the fetch/pin/checksum decision below is unaffected).
Initial apps: cowsay and QuickJS, both from the Wasmer registry; QuickJS later moved to quickjs-ng's own GitHub release (a standalone WASI CLI asset) once that became available upstream.
The decision below was never registry-specific, only per-app source diversity was added.

## Context

Real applications (cowsay, a JavaScript engine) are the most convincing demos and the best end-to-end regression tests beyond the spec suite.
But committing third-party wasm binaries into this repository means *redistributing* them, a licensing question per app (the registry often carries no license metadata at all) plus permanent repository bloat.

## Decision

Third-party artifacts are **never committed**.
`examples/apps/setup.sh` downloads each app from **its own upstream** at **pinned versions with sha256 verification**, into a gitignored cache, and documents each app's upstream in `examples/apps/README.md`.
That upstream is a registry CDN (Wasmer) or a project's own release asset (quickjs-ng's GitHub releases), whichever actually publishes a standalone WASI binary for it.
Criterion: *distribution stays upstream's; the repository holds only references (URL + hash), which raise no license questions and keep the supply chain auditable*.
This criterion never named a specific registry, so per-app source diversity (the `fetch_app` helper in `examples/apps/scripts/common.sh` supports both a tarball-with-inner-path and a bare `.wasm` release asset) is a refinement, not a reversal.
Not every upstream qualifies: sqlite.org's own "WebAssembly" download is an Emscripten browser/Node.js bundle, not something `wasmtime run` can execute, so sqlite stays on the Wasmer registry build.
Per decision 15, the e2e test *fails* rather than skips when the cache is absent: `cargo test` itself stays hermetic (no network access during a normal run), but reaching that state requires the one-time, explicit `setup.sh` step first.

App selection is constrained by the declared WASI surface (docs/support.md): originally stdio/args/environ/clock/random only; WASI filesystem support (decision 14) later widened this for Ruby.
`wasi_unstable` (snapshot 0) is accepted as an alias of preview 1 for the implemented functions: the original Wasmer-registry QuickJS build (since replaced) needed it (known simplification: snapshot 0's fd_seek whence encoding differs; none of the current apps seek).

## Rejected alternatives

- **Committing the wasm binaries**: redistribution licensing per app, registry packages often lack license metadata, and megabytes of history bloat.
- **Building from crates.io sources at test time**: works for Rust apps but demands local toolchains (wasi-sdk for C apps like QuickJS) and long builds; the registry serves exactly the prebuilt artifact users would run (user's call).

## Consequences

- Positive: QuickJS (a complete C-built JS engine) converts to a ~25 MB Ruby file and produces wasmtime-identical output in about a second; the demo story ("a JS engine on plain Ruby") costs one script run.
- Negative: the `apps` cases fail (per decision 15) rather than passing vacuously in a fresh clone until the explicit `setup.sh` step runs; CI needs that step (or a cached fetch job) before `cargo test` passes.
  Upstream availability is a fetch-time dependency, mitigated by pinning and checksums.
- Adding an app = a script under `examples/apps/scripts/` + a case in `crates/dewasm-test-helper/src/apps.rs` + a row in the README table.
