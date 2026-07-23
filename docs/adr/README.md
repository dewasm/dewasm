# Architecture Decision Records

This directory contains the Architecture Decision Records (ADRs) for
dewasmify. Each document captures a significant design decision: its
context, the decision with its rationale, the rejected alternatives, and
the consequences.

## How to read

- **ADR-0** is the foundation document — start there for the project's
  goal, scope, and architecture.
- Higher-numbered ADRs build on it and can be read as needed.
- Each ADR opens with a **Status** paragraph: `Accepted`, `Proposed`, or
  `Superseded`, with a date and a one-paragraph "what landed / what
  remains" summary. Accepted ADRs whose implementation is still pending
  carry a parenthetical note (e.g. *not yet implemented*).
- ADR-0 through ADR-5 were backfilled on 2026-07-23 from decisions made
  during initial planning and implementation.

## Index

| # | Title | Status |
| --- | --- | --- |
| ADR-0 | [Foundation and Core Architecture](0-foundation.md) | Accepted |
| ADR-1 | [IR Design: Structured Control Flow + Stack-Slot Temps](1-ir-design.md) | Accepted |
| ADR-2 | [Numeric Semantics Strategy for Dynamically-Typed Targets](2-numeric-semantics.md) | Accepted |
| ADR-3 | [Testing Strategy: Spec Testsuite on Real Interpreters](3-testing-strategy.md) | Accepted |
| ADR-4 | [Ruby Backend Lowering Conventions](4-ruby-backend-lowering.md) | Accepted |
| ADR-5 | [Bash Floats: Pure-Bash Softfloat](5-bash-softfloat.md) | Accepted (not yet implemented) |
| ADR-6 | [Runtime as Per-Method Units with Selectable Linkage](6-runtime-units.md) | Accepted |
| ADR-7 | [Import Providers and the Default WASI Fallback](7-import-providers.md) | Accepted |

## Adding a new ADR

When a decision with real alternatives is made:

1. Take the next free number and create `docs/adr/<N>-<slug>.md`.
2. Follow the structural contract: an opening **Status** paragraph
   (state, date, what landed / what remains), then **Context**,
   **Decision**, **Rejected alternatives**, **Consequences**.
3. Add a row to the index table above, in ascending order.
4. Cross-reference: link related ADRs, and cite the ADR from `AGENTS.md`
   or code comments where the rule is enforced.

Quality bar:

- An ADR records a **decision with rationale and rejected alternatives**,
  or a standing policy. State the *criterion* that discriminated between
  the options as a reusable rule, not just "we picked B".
- A mechanical change with no live alternatives does not need an ADR —
  the commit message is enough.
- **Length tracks stakes.** The common failure mode is writing too much,
  not too little. Move research material (surveys, comparison tables)
  out of the ADR and cite it; keep the ADR the decision, not the
  research.
- Anchor claims to real code (`crates/.../file.rs`, `runtime/<lang>/`)
  where possible.
- The spec testsuite binds behaviour (ADR-3); an ADR records *why*, never
  a normative description that the harness already enforces.

## Relationship to other documents

- **`AGENTS.md`** — the development contract for agents (and humans)
  working in this repository; cites ADRs where a rule needs its why.
- **`README.md`** — user-facing overview; links here for design
  rationale.
