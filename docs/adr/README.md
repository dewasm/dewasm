# Architecture Decision Records

This directory contains the Architecture Decision Records (ADRs) for
dewasm. Each document captures a significant design decision: its
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
| ADR-5 | [Bash Floats: Pure-Bash Softfloat](5-bash-softfloat.md) | Accepted |
| ADR-6 | [Runtime as Per-Method Units with Selectable Linkage](6-runtime-units.md) | Accepted |
| ADR-7 | [Import Providers and the Default WASI Fallback](7-import-providers.md) | Accepted |
| ADR-8 | [Track the Latest Testsuite; Attribute Skips to a Support Matrix](8-latest-testsuite-support-matrix.md) | Accepted |
| ADR-9 | [Example Apps Fetched from Upstream, Never Committed](9-example-apps-from-registry.md) | Accepted |
| ADR-10 | [Add C# to the Target Languages, Paired with Java](10-csharp-target.md) | Accepted |
| ADR-11 | [Bash Backend Lowering Conventions (Integer Subset)](11-bash-backend-lowering.md) | Accepted |
| ADR-12 | [Bash WASI Conventions](12-bash-wasi.md) | Accepted |
| ADR-13 | [Bash Softfloat Conventions](13-bash-softfloat-conventions.md) | Accepted |
| ADR-14 | [Ruby WASI Filesystem Support](14-ruby-wasi-filesystem.md) | Accepted |
| ADR-15 | [Tests Fail Loud on Missing Environment, Never Skip](15-tests-fail-not-skip.md) | Accepted |
| ADR-16 | [Completing Wasm 1.0 for Ruby: Non-Function Imports, Multiple Tables, Table Bulk Ops, Linking](16-ruby-wasm1-completion.md) | Accepted |
| ADR-17 | [Reference Types in the Ruby Backend: funcref = the Table Pair, externref = a Raw Host Value](17-ruby-reference-types.md) | Superseded (ADR-24) |
| ADR-18 | [Tail Calls in the Ruby Backend: Flat Trampoline with a Body/Entry Split](18-ruby-tail-calls.md) | Superseded (ADR-24) |
| ADR-19 | [Exception Handling in the Ruby Backend: Tags as Identity Objects, Exceptions as Native Exceptions](19-ruby-exception-handling.md) | Superseded (ADR-24) |
| ADR-20 | [Component Model: Canonical-ABI Adapters Synthesized as Core IR, Host Boundary as a Fixed Vocabulary](20-component-model-core-ir-adapters.md) | Superseded (ADR-24) |
| ADR-21 | [WASI Preview 2 Host for Ruby (CLI World)](21-ruby-wasi-preview2.md) | Superseded (ADR-24) |
| ADR-22 | [Build the sqlite3 Apps From Pinned Source With zig, Both Standalone and Library](22-sqlite3-built-from-source.md) | Accepted |
| ADR-23 | [Backend Support Tiers, Specialized to Wasm 1.0 + WASI Preview 1](23-backend-support-tiers.md) | Superseded (ADR-25) |
| ADR-24 | [0.1 Scope Reset: Wasm 1.0 + WASI Preview 1 Only, App-Driven Goals](24-01-scope-reset.md) | Accepted |
| ADR-25 | [Retire the Support-Tier Ladder for Plain Capability Declarations](25-retire-support-tiers.md) | Accepted |
| ADR-26 | [Rename the Project: dewasmify → dewasm](26-rename-dewasm.md) | Accepted |
| ADR-27 | [Shared Test-Helper Crate with Per-Feature Test Macros](27-test-helper-crate.md) | Accepted |
| ADR-28 | [Python Backend Lowering Conventions](28-python-backend-lowering.md) | Accepted |
| ADR-29 | [Go Backend Lowering Conventions](29-go-backend-lowering.md) | Accepted |
| ADR-30 | [Java Backend Lowering Conventions](30-java-backend-lowering.md) | Accepted |
| ADR-31 | [Standalone Runtime Interface (argv, --dir, env, exit)](31-standalone-runtime-interface.md) | Accepted |
| ADR-32 | [Build-time Expression Folding](32-expression-folding.md) | Accepted |
| ADR-33 | [`IO::Buffer`-Backed Linear Memory for Ruby](33-ruby-io-buffer-memory.md) | Accepted |
| ADR-34 | [Bash WASI Filesystem](34-bash-wasi-filesystem.md) | Accepted |
| ADR-35 | [Bash Cross-Module Linking](35-bash-cross-module-linking.md) | Accepted |
| ADR-36 | [Official WASI p1 Conformance Suite as a Harness Layer](36-wasi-testsuite-conformance.md) | Accepted |
| ADR-37 | [Opt-in Data-Segment Externalization (`--data-file`)](37-data-segment-externalization.md) | Accepted |
| ADR-38 | [Opt-in DWARF Line-Number Back-Mapping (`--dwarf-line`)](38-dwarf-line-back-mapping.md) | Accepted |
| ADR-39 | [wasm-opt Preprocessing of Locally-Built App Modules](39-wasm-opt-preprocessing.md) | Accepted |
| ADR-40 | [WASI p1 Completion: Symlink Family, Enforced Per-Fd Rights, and the Conformance-Runner Environment](40-wasi-p1-completion.md) | Accepted |
| ADR-41 | [Merge Adjacent Active Data Segments at Build Time](41-adjacent-data-segment-merging.md) | Accepted |

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
- **`docs/getting-started.md`** and **`docs/backends/`** — the user tutorial
  and per-target reference; they link to the lowering ADRs (4/11–13/28/29/30)
  for design rationale rather than restating it.
- **`docs/docs-policy.md`** — the doc taxonomy (which file each kind of
  content belongs in, and why `docs/support.md` is generated, never
  hand-edited).
