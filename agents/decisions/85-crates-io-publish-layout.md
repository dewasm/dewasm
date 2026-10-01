# Decision 85: crates.io Publish Layout (Units Inside Their Crates, CLI Crate Named `dewasm`)

Status: **Accepted, 2026-08-22.**
Landed for the 0.1.0 release:
- the runtime units moved to `crates/dewasm-backend-<lang>/units/`;
- the CLI package renamed from `dewasm-cli` to `dewasm`;
- the workspace path dependencies carry explicit versions.

The actual `cargo publish` of the nine crates is a release step, not part of this change.

## Context

0.1.0 publishes the workspace to crates.io so `cargo install dewasm` works without a clone.
`cargo package` only includes files under a crate's own directory.
But each backend's `build.rs` embedded its units from the repository-level `runtime/<lang>/units/`.
That layout is decision 6's, and so a packaged backend crate could not build with it.
Separately, the CLI package was named `dewasm-cli` while its binary is `dewasm`.
Also, crates.io dependencies must name a version.
The path-only `[workspace.dependencies]` entries did not.

## Decision

- *Every file a published crate builds from lives under that crate's directory.*
  The units move to `crates/dewasm-backend-<lang>/units/`.
  Each `build.rs` reads `$CARGO_MANIFEST_DIR/units`.
  Decision 6's mechanism (per-method units, `# requires:` headers, on-demand bundling) is unchanged.
  Only the location moved.
- *One public name per artifact: the package that installs the `dewasm` binary is the `dewasm` crate.*
  `cargo install dewasm` is the installation story; no separate `dewasm-cli` name exists on crates.io.
- The publishable `[workspace.dependencies]` entries carry `version = "<workspace version>"` beside `path`.
  `dewasm-test-helper` stays path-only: it is `publish = false`.
  Cargo drops path-only `dev-dependencies` from published manifests.
  That is exactly the intended shape.

## Rejected alternatives

- **A symbolic link from `crates/dewasm-backend-<lang>/units` → `../../runtime/<lang>/units`.**
  Verified to work: `cargo package` follows the link and packages real files.
  But a tree with symbolic links breaks builds on a Windows clone without support for them.
  It also hides the package contents in the repository.
- **Copy the units into each crate at publish time.**
  Two copies of the truth.
  The failure (an out-of-date or missing copy) only surfaces during a publish, the rarest operation.
- **Keep `dewasm-cli` and reserve `dewasm` with a stub crate.**
  Two crates.io names for one artifact.
  The stub is a permanent redirection page that users hit first.

## Consequences

- Published backend crates are self-contained; `cargo package --workspace` is the release dry-run.
- The units sit beside the `src/` that embeds them.
  The bundler and the units lint are path-independent, and did not change.
  The bundler is `RuntimeBundler` in `crates/dewasm-backend/src/lib.rs`.
- Every reference to the old paths was rewritten in place.
  Those are in `AGENTS.md`, CI's ShellCheck exclusion, and unit-internal cross-language comments.
  `cargo run -p dewasm` in documents and example build scripts was also rewritten.
  Older decisions keep their historical paths.
- Related: decision 6 (the unit mechanism), decision 26 (the previous rename, `dewasmify` → `dewasm`).
