<!--
Maintainer notes. Block-level HTML comments are stripped before this file enters an agent's
context:

- Claude Code reads CLAUDE.md, not AGENTS.md; CLAUDE.md pulls this file in with `@AGENTS.md`.
- Everything here loads into every session. Keep it short and keep it INSTRUCTIONS; explain a
  rule's why only when the why changes what you do.
- Material needed only inside one area belongs in .claude/skills/ or docs/, never here.
-->

# AGENTS.md

Agent contract for dewasmify. Project docs are written in English; `tests/spec` is an upstream
submodule — never edit it.

## Development environment

- Rust toolchain is pinned by `rust-toolchain.toml` (stable); plain `cargo` commands pick it up.
- First-time setup: `git submodule update --init` (fetches the spec testsuite into `tests/spec`).
- The spec harness and e2e tests need `ruby` on `PATH`; the bash suites need bash >= 5
  (`$DEWASMIFY_BASH`, `PATH`, or the homebrew path — macOS system bash 3.2 does not count).
  All of them self-skip when the interpreter is missing, so a green run without it proves less
  than it looks.

## Common commands

| Command | What it does |
| --- | --- |
| `cargo test` | **The gate**: unit + e2e + full spec harness (~5 s for the harness). |
| `DEWASMIFY_SPEC=i32,br cargo test -p dewasmify-cli --test spec -- --nocapture` | Spec harness on selected `.wast` files only; prints per-file pass/fail/skip. Add a test-name filter (`spec_ruby`/`spec_bash`) for one language. |
| `DEWASMIFY_SPEC_ALL=1 cargo test -p dewasmify-cli --test spec spec_bash -- --nocapture` | Full-testsuite sweep for bash (~40 s); `cargo test` alone runs bash on a curated file list. |
| `cargo run -p dewasmify-cli -- input.wasm --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |
| `examples/apps/fetch.sh` | Fetch pinned real-world apps (cowsay, QuickJS) into the gitignored cache; enables the `apps` e2e test. |

## Verification

After any non-trivial change, run `cargo test`. Spec-harness failures mean a semantics bug: fix
the cause. Adding to a per-language `EXPECTED_FAILURES` ledger in
`crates/dewasmify-cli/tests/spec/` is a last resort and requires an attribution tag plus a reason
([ADR-8](docs/adr/8-latest-testsuite-support-matrix.md)). When support declarations or WASI
units change, regenerate the matrix: `DEWASMIFY_UPDATE_DOCS=1 cargo test -p dewasmify-cli
--test support_docs` (the test fails while docs/support.md is stale).

## Implementation guidelines

- **The spec testsuite binds; an ADR says why** ([ADR-3](docs/adr/3-testing-strategy.md)).
  Correctness of generated code outranks its readability ([ADR-1](docs/adr/1-ir-design.md));
  readability improvements go into optional passes, never into semantics-relevant lowering.
- Numeric representation conventions (masked-unsigned integers, f32 re-rounding, NaN bit paths)
  are fixed in [ADR-2](docs/adr/2-numeric-semantics.md); new backends follow them. Per-backend
  lowering shapes live in [ADR-4](docs/adr/4-ruby-backend-lowering.md) (Ruby) and
  [ADR-11](docs/adr/11-bash-backend-lowering.md) (Bash — incl. the status-cascade trap
  protocol and the `return 0` discipline the units lint enforces).
- Runtime code lives as per-method units under `runtime/<lang>/units/` with `# requires:`
  headers, referenced as `Rt` ([ADR-6](docs/adr/6-runtime-units.md)); keep the headers in sync
  when editing a unit — the units lint test enforces most of it.
- A new backend is done when the shared spec harness passes for it — not before
  ([ADR-3](docs/adr/3-testing-strategy.md)).
- Unsupported wasm features must fail at conversion time with a clear error, never at runtime
  ([ADR-0](docs/adr/0-foundation.md)).

## ADRs

Significant decisions — anything with real alternatives — are recorded in
[`docs/adr/`](docs/adr/README.md). The `adr-author` skill carries the procedure (numbering, the
skeleton, the index update). Do not restate ADR content elsewhere; link to it.

## Commit etiquette

- Imperative subject in sentence case; **no** Conventional-Commits `type:` prefixes.
- Body explains the *why*, wrapped at ~72 columns; the diff already shows the what.
- Do not commit or push unless asked.
