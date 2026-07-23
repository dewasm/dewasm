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
- The spec harness and e2e tests need `ruby` on `PATH`; they self-skip when it is missing, so a
  green run without Ruby proves less than it looks.

## Common commands

| Command | What it does |
| --- | --- |
| `cargo test` | **The gate**: unit + e2e + full spec harness (~5 s for the harness). |
| `DEWASMIFY_SPEC=i32,br cargo test -p dewasmify-cli --test spec -- --nocapture` | Spec harness on selected `.wast` files only; prints per-file pass/fail/skip. |
| `cargo run -p dewasmify-cli -- input.wasm --mode standalone -o out.rb` | Convert; `.wat` input works too, `-o -` for stdout. |

## Verification

After any non-trivial change, run `cargo test`. Spec-harness failures mean a semantics bug: fix
the cause. Adding to `EXPECTED_FAILURES` in `crates/dewasmify-cli/tests/spec.rs` is a last
resort and requires a reason comment plus an ADR reference
([ADR-3](docs/adr/3-testing-strategy.md)).

## Implementation guidelines

- **The spec testsuite binds; an ADR says why** ([ADR-3](docs/adr/3-testing-strategy.md)).
  Correctness of generated code outranks its readability ([ADR-1](docs/adr/1-ir-design.md));
  readability improvements go into optional passes, never into semantics-relevant lowering.
- Numeric representation conventions (masked-unsigned integers, f32 re-rounding, NaN bit paths)
  are fixed in [ADR-2](docs/adr/2-numeric-semantics.md); new backends follow them.
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
