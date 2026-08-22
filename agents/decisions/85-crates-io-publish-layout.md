# Decision 85: crates.io Publish Layout (Units Inside Their Crates, CLI Crate Named `dewasm`)

Status: **Accepted, 2026-08-22.**
Landed for the 0.1.0 release: the runtime units moved to `crates/dewasm-backend-<lang>/units/`, the CLI package renamed from `dewasm-cli` to `dewasm`, and the workspace path dependencies carry explicit versions.
The actual `cargo publish` of the nine crates is a release step, not part of this change.

## Context

0.1.0 publishes the workspace to crates.io so `cargo install dewasm` works without a checkout.
`cargo package` only includes files under a crate's own directory, but each backend's `build.rs` embedded its runtime units from the repository-level `runtime/<lang>/units/` (decision 6), so a packaged backend crate could not build.
Separately, the CLI package was named `dewasm-cli` while its binary is `dewasm`, and crates.io dependencies must name a version, which the path-only `[workspace.dependencies]` entries did not.

## Decision

- *Every file a published crate builds from lives under that crate's directory.*
  The units move to `crates/dewasm-backend-<lang>/units/`, and each `build.rs` reads `$CARGO_MANIFEST_DIR/units`.
  Decision 6's mechanism (per-method units, `# requires:` headers, on-demand bundling) is unchanged; only the location moved.
- *One public name per artifact: the package that installs the `dewasm` binary is the `dewasm` crate.*
  `cargo install dewasm` is the installation story; no separate `dewasm-cli` name exists on crates.io.
- The publishable `[workspace.dependencies]` entries carry `version = "<workspace version>"` beside `path`.
  `dewasm-test-helper` stays path-only: it is `publish = false`, and cargo drops path-only dev-dependencies from published manifests, which is exactly the intended shape.

## Rejected alternatives

- **Symlink `crates/dewasm-backend-<lang>/units` → `../../runtime/<lang>/units`.**
  Verified to work (`cargo package` follows the link and packages real files), but a symlinked tree breaks builds on Windows checkouts without symlink support and makes the package contents invisible in the checkout.
- **Copy the units into each crate at publish time.**
  Two copies of the truth, and the breakage (a stale or missing copy) only surfaces during a publish, the rarest operation.
- **Keep `dewasm-cli` and reserve `dewasm` with a stub crate.**
  Two crates.io names for one artifact, and the stub is a permanent redirection page users hit first.

## Consequences

- Published backend crates are self-contained; `cargo package --workspace` is the release dry-run.
- The units sit beside the `src/` that embeds them; the bundler (`crates/dewasm-backend/src/lib.rs`, `RuntimeBundler`) and the units lint are path-independent and did not change.
- Every reference to the old paths was rewritten in place (`AGENTS.md`, CI's shellcheck exclusion, unit-internal cross-language comments, `cargo run -p dewasm` in docs and example build scripts); older decisions keep their historical paths.
- Related: decision 6 (the unit mechanism), decision 26 (the previous rename, dewasmify → dewasm).
