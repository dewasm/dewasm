# Documentation policy

The taxonomy of dewasm's documents, so new content lands in one obvious place and nothing is duplicated.
Everything is written in English.

Two top-level directories, split by audience:

- **`agents/`** holds the documents an agent reads while doing the work: the decision records and this policy.
- **`docs/`** holds the documents a human reads: users evaluating or running dewasm, and contributors setting up a machine.

The split is by reader, not by subject.
A document a user would never open, but an agent must consult before changing something, belongs under `agents/` even when it describes user-facing behavior.

| Document | Role | Audience | Editable |
| --- | --- | --- | --- |
| [README.md](../README.md) | Front door: what dewasm is, one showpiece, install, usage, scope | Users evaluating the project | By hand |
| [AGENTS.md](../AGENTS.md) | The agent contract: the rules that bind every change | Agents, contributors | By hand |
| [agents/decisions/](decisions/README.md) | Design decisions with rationale and rejected alternatives, numbered and cited as "decision N" | Agents, future maintainers | By hand (procedure and quality bar in its README) |
| [agents/docs-policy.md](docs-policy.md) | This file: which document each kind of content belongs in | Agents writing documents | By hand |
| [agents/test-authoring.md](test-authoring.md) | How the test suites are structured and what a new case must look like | Agents and contributors writing tests | By hand |
| [docs/getting-started.md](../docs/getting-started.md) | Tutorial: a verified end-to-end walkthrough | New users | By hand (verify every command) |
| [docs/backends/](../docs/backends/) | Per-target reference: output shape, requirements, caveats, provider usage | Users of a specific target | By hand |
| [docs/standalone-interface.md](../docs/standalone-interface.md) | The standalone runtime interface (argv, `--dir`, env, exit/trap), uniform across backends | Users running standalone output | By hand |
| [docs/support.md](../docs/support.md) | The feature / WASI matrix per backend | Everyone | **Generated: never hand-edit** |
| [docs/apps-audit.md](../docs/apps-audit.md) | The real-world app test record and feature verdicts | Contributors, evaluators | By hand |
| [docs/testing.md](../docs/testing.md) | How to run the test suites: what each one needs installed, and why it fails loud | Contributors | By hand |
| [docs/related-work.md](../docs/related-work.md) | Comparison with prior art | Evaluators | By hand |
| [docs/benchmarks/](../docs/benchmarks/README.md) | How to run the benchmark suite and read its numbers | Contributors | By hand |
| [docs/benchmarks/results.md](../docs/benchmarks/results.md) | Measured performance, with figures under `figs/` | Evaluators | **Generated: never hand-edit** |
| [docs/sizes/](../docs/sizes/README.md) | How to run the size record and read its numbers | Contributors | By hand |
| [docs/sizes/results.md](../docs/sizes/results.md) | Measured distribution sizes (wasm binary, converted source, runtimes) with figures under `figs/` | Evaluators | **Generated: never hand-edit** |
| [records/README.md](../records/README.md) | When, on what host, and on what occasion each stored measurement record was taken | Contributors, evaluators | By hand (`cargo xtask record-speed` and `cargo xtask record-size` append a placeholder line per record they write) |

## Rules

- **`docs/support.md` is generated** from the backend declarations.
  Never edit it by hand; regenerate with `cargo xtask update-support-docs` (`cargo test -p dewasm-cli --test support_docs` fails while the file is stale).
  Everywhere else, **link** to it rather than copying the matrix.
- **Decisions go in a decision record, not in prose docs.**
  Anything with real alternatives is recorded under `agents/decisions/`.
- **Nothing outside `agents/` references anything under it.**
  `AGENTS.md`, `CLAUDE.md`, and `.claude/` are the exceptions.
  Code and user-facing docs state their constraint in place; an `agents/` document links outward to the code and docs it concerns, never the reverse.
- **A skill under `.claude/skills/` is a router, not a store.**
  Claude Code auto-invokes a skill by its description, which is what a skill is for; the substance it routes to belongs under `agents/`.
  The test: if deleting `.claude/` would lose information rather than convenience, the file is holding content it should be pointing at.
  A project-local skill is named with a `dewasm-` prefix, because the skill namespace is shared with the user's global skills.
- **Tutorial commands must be verified by running them.**
  getting-started and the backend docs claim exact output; keep them true.

## Where new content goes

- A user-facing capability or a new target → a bullet in the README's capability list, a page under `docs/backends/`, and (if it needs a walkthrough) a section in getting-started.
- A contributor-facing setup requirement, or how to run a suite → `docs/testing.md`.
- A test-structure or test-authoring convention → `agents/test-authoring.md`.
- A design decision → a new record (see [agents/decisions/README.md](decisions/README.md)).
- A rule that binds every change → a line in `AGENTS.md`, citing the record that holds its rationale.
- A new real-world app target → an audited row in `docs/apps-audit.md`.
- A performance number → a workload under `benchmarks/`, measured by `cargo xtask record-speed`.
  Never a hand-written figure in prose: numbers drift silently, and the ratio a benchmark reports depends on the workload.
- A size number → the record `cargo xtask record-size` writes to `records/`, rendered into `docs/sizes/results.md`, for the same reason: a generated artifact's size changes with every codegen change.
