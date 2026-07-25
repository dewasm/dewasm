<!--
Maintainer notes. Block-level HTML comments are stripped before this file enters an agent's
context:

- Claude Code reads CLAUDE.md, not AGENTS.md; CLAUDE.md pulls this file in with `@AGENTS.md`.
- Everything here loads into every session. Keep it short and keep it INSTRUCTIONS; explain a
  rule's why only when the why changes what you do.
- Material needed only inside one area belongs in .claude/skills/ or docs/, never here.
-->

# AGENTS.md

Agent contract for dewasm. Project docs are written in English; `tests/spec` is an upstream
submodule — never edit it.

## Development environment

Rust toolchain is pinned by `rust-toolchain.toml` (stable); plain `cargo` commands pick it up.
Required tools/setup for the test suite (ruby, bash >= 5, the spec submodule, the `apps` cache)
and the fail-loud-not-skip policy behind it (ADR-15) are documented in
[`docs/testing.md`](docs/testing.md) — read it before wondering why a test panics with a setup
instruction instead of skipping.

## Common commands

| Command | What it does |
| --- | --- |
| `cargo test` | **The gate**: unit + e2e + full spec harness (~5 s for the harness). Each backend crate owns its own conformance suites (ADR-27); only cross-backend tests live in `dewasm-cli`. |
| `cargo fmt --check` | Verify Rust code formatting. |
| `cargo clippy --all-targets -- -D warnings` | Run linter on all targets, failing on any warnings. |
| `DEWASM_SPEC=i32,br cargo test -p dewasm-backend-ruby --test spec -- --nocapture` | Spec harness on selected `.wast` files only; prints per-file pass/fail/skip. Swap the crate (`-p dewasm-backend-bash`) to switch language. |
| `DEWASM_SPEC_ALL=1 cargo test -p dewasm-backend-bash --test spec -- --nocapture` | Full-testsuite sweep for bash (~60 s); `cargo test` alone runs bash on a curated file list. |
| `cargo run -p dewasm-cli -- input.wasm --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |
| `examples/apps/fetch.sh` | Fetch pinned real-world apps (cowsay, QuickJS) and build the sqlite3 pair from pinned source (needs `zig`, ADR-22) into the gitignored cache; enables the `apps` cases of the `e2e` test. |

## Verification

After any non-trivial change, run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
and `cargo test`. Spec-harness failures mean a semantics bug: fix the cause. Adding to a
per-backend `EXPECTED_FAILURES` ledger in `crates/dewasm-backend-<lang>/tests/spec.rs` is a last resort and
requires an attribution tag plus a reason ([ADR-8](docs/adr/8-latest-testsuite-support-matrix.md)).
When support declarations or WASI units change, regenerate the matrix:
`DEWASM_UPDATE_DOCS=1 cargo test -p dewasm-cli --test support_docs` (the test fails while
docs/support.md is stale).

## Implementation guidelines

- **The spec testsuite binds; an ADR says why** ([ADR-3](docs/adr/3-testing-strategy.md)).
  Correctness of generated code outranks its readability ([ADR-1](docs/adr/1-ir-design.md));
  readability improvements go into optional passes, never into semantics-relevant lowering.
- Numeric representation conventions (masked-unsigned integers, f32 re-rounding, NaN bit paths)
  are fixed in [ADR-2](docs/adr/2-numeric-semantics.md); new backends follow them. Per-backend
  lowering shapes live in [ADR-4](docs/adr/4-ruby-backend-lowering.md) (Ruby) and
  [ADR-11](docs/adr/11-bash-backend-lowering.md) (Bash — incl. the status-cascade trap
  protocol and the `return 0` discipline the units lint enforces); Bash WASI conventions
  (status-133 proc_exit, byte-wise binary stdio) in [ADR-12](docs/adr/12-bash-wasi.md);
  the Bash softfloat (bit-pattern floats, the round_pack contract, the Rust-oracle test
  in `crates/dewasm-backend-bash/tests/softfloat.rs`) in
  [ADR-13](docs/adr/13-bash-softfloat-conventions.md); Ruby WASI filesystem support (the
  `preopens:` provider kwarg, the fd-table model, and the accepted TOCTOU/symlink sandboxing
  caveat) in [ADR-14](docs/adr/14-ruby-wasi-filesystem.md).
- Runtime code lives as per-method units under `runtime/<lang>/units/` with `# requires:`
  headers, referenced as `Rt` ([ADR-6](docs/adr/6-runtime-units.md)); keep the headers in sync
  when editing a unit — the units lint test enforces most of it.
- A new backend is done when the shared spec harness passes for it — not before
  ([ADR-3](docs/adr/3-testing-strategy.md)).
- Backends declare their capabilities directly — feature support (`Backend::feature_status`) and
  per-function WASI p1 coverage (`Backend::has_wasi_p1`) — rendered flat into `docs/support.md`;
  the standard goal for a new backend is wasm 1.0 + full WASI p1. Wasm 2.0+ proposals and the
  component model are rejected outright, not a backend opt-in (ADR-24) — see
  [ADR-25](docs/adr/25-retire-support-tiers.md) for why the former per-backend tier ladder was
  retired.
- Unsupported wasm features must fail at conversion time with a clear error, never at runtime
  ([ADR-0](docs/adr/0-foundation.md)). Non-function imports, multiple tables, and table bulk ops
  are accepted by the core IR unconditionally; a backend that hasn't implemented one must reject
  it itself via `dewasm_backend::check_module_support` ([ADR-16](docs/adr/16-ruby-wasm1-completion.md)),
  which also covers the `Rt::Global` box, the import-kind check, and the spec harness's `register`
  support.

## ADRs

Significant decisions — anything with real alternatives — are recorded in
[`docs/adr/`](docs/adr/README.md). The `adr-author` skill carries the procedure (numbering, the
skeleton, the index update). Do not restate ADR content elsewhere; link to it.

## Commit etiquette

- Imperative subject in sentence case; **no** Conventional-Commits `type:` prefixes.
- Body explains the *why*, wrapped at ~72 columns; the diff already shows the what.
- Do not commit or push unless asked.
