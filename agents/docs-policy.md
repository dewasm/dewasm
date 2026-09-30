# Documentation policy

This file states where each kind of dewasm document belongs.
New content then lands in one obvious place, and nothing is written twice.
Everything is written in English.

Two top-level directories, split by audience:

- **`agents/`** holds the documents an agent reads while working: the decision records and this policy.
- **`docs/`** holds the documents a human reads.
  Its readers are users evaluating or running dewasm, and developers setting up a machine.

The split is by reader, not by subject.
Take a document a user would never open, but an agent must consult before changing something.
It belongs under `agents/` even when it describes user-facing behavior.

| Document | Role | Audience | Editable |
| --- | --- | --- | --- |
| [`README.md`](../README.md) | Front door: what dewasm is, one example, install, usage, scope | Users evaluating the project | By hand |
| [`AGENTS.md`](../AGENTS.md) | The agent contract: the rules that bind every change | Agents, developers | By hand |
| [`agents/decisions/`](decisions/README.md) | Design decisions with reasons and rejected alternatives, numbered and cited as "decision N" | Agents, future maintainers | By hand (procedure and quality bar in its README) |
| [`agents/experiments.md`](experiments.md) | Past experiments: the conclusion and its re-test condition, over the Issue/PR that holds the record | Agents | By hand |
| [`agents/docs-policy.md`](docs-policy.md) | This file: which document each kind of content belongs in | Agents writing documents | By hand |
| [`agents/test-authoring.md`](test-authoring.md) | How the test suites are structured and what a new case must look like | Agents and developers writing tests | By hand |
| [`agents/apps-audit.md`](apps-audit.md) | The real-world app test record and feature verdicts | Agents and developers adding an app target | By hand |
| [`agents/alternative-ruby-runtimes.md`](alternative-ruby-runtimes.md) | How to run the Ruby suites on a Ruby other than CRuby, and how to read and narrow what fails | Agents measuring another Ruby implementation | By hand |
| [`agents/measurement-records.md`](measurement-records.md) | The steps to check before and after a speed or size run, and what makes a result suspect | Agents taking a record | By hand |
| [`agents/vocabulary.md`](vocabulary.md) | The sources of allowed words, the project's terms, and the excluded words with what to write in their place | Agents writing text | By hand |
| [`docs/getting-started.md`](../docs/getting-started.md) | Tutorial: verified end-to-end steps | New users | By hand (verify every command) |
| [`docs/backends/`](../docs/backends/) | Per-target reference: output shape, requirements, limits, provider usage | Users of a specific target | By hand |
| [`docs/standalone-interface.md`](../docs/standalone-interface.md) | The standalone runtime interface (`argv`, `--dir`, environment, exit/trap), uniform across backends | Users running standalone output | By hand |
| [`docs/support.md`](../docs/support.md) | The feature / WASI matrix per backend | Everyone | **Generated: never hand-edit** |
| [`docs/testing.md`](../docs/testing.md) | How to run the test suites: what each one needs installed, and why it fails loud | Developers | By hand |
| [`docs/related-work.md`](../docs/related-work.md) | Comparison with prior art | Users evaluating the project | By hand |
| [`docs/benchmarks/`](../docs/benchmarks/README.md) | How to run the benchmark suite and read its numbers | Developers | By hand |
| [`docs/benchmarks/results.md`](../docs/benchmarks/results.md) | Measured performance, with figures under `figs/` | Users evaluating the project | **Generated: never hand-edit** |
| [`docs/sizes/`](../docs/sizes/README.md) | How to run the size record and read its numbers | Developers | By hand |
| [`docs/sizes/results.md`](../docs/sizes/results.md) | Measured distribution sizes (wasm binary, converted source, runtimes) with figures under `figs/` | Users evaluating the project | **Generated: never hand-edit** |
| [`docs/users.md`](../docs/users.md) | Projects that ship dewasm-converted code | Users | By hand |
| [`records/README.md`](../records/README.md) | When, on what host, and on what occasion each stored measurement record was taken | Developers, users evaluating the project | By hand (`cargo xtask record-speed` and `cargo xtask record-size` add a placeholder line per record they write) |

## Rules

- **`docs/support.md` is generated** from the backend declarations.
  Never edit it by hand; regenerate it with `cargo xtask update-support-docs`.
  `cargo test -p xtask support_docs_in_sync` fails while the file is out of date.
  Everywhere else, **link** to it rather than copying the matrix.
- **Decisions go in a decision record, not in other documents.**
  Anything with real alternatives is recorded under `agents/decisions/`.
- **Nothing outside `agents/` references anything under it.**
  `AGENTS.md`, `CLAUDE.md`, and `.claude/` are the exceptions.
  Code and user-facing documents state their constraint in place.
  An `agents/` document links out to the code and documents it concerns, never the reverse.
  The app audit tooling is a further exception.
  That tooling is the `feature-audit` `xtask` command and the `examples/apps` fetch scripts.
  It cites `agents/apps-audit.md` because that record is where its verdicts land.
- **A skill under `.claude/skills/` is a router, not a store.**
  Claude Code picks a skill by its description without being asked, which is what a skill is for.
  The substance it routes to belongs under `agents/`.
  The test is whether removing `.claude/` would lose information rather than a shorter way to reach it.
  If it would, the file is holding content it should be pointing at.
  A project-local skill is named with a `dewasm-` prefix.
  It needs one because the skill namespace is shared with the user's global skills.
- **Tutorial commands must be verified by running them.**
  `docs/getting-started.md` and the backend documents claim exact output; keep them true.

## Where new content goes

- A user-facing capability or a new target → three places:
  - a bullet in the README's capability list;
  - a page under `docs/backends/`;
  - a section in `docs/getting-started.md`, if it needs steps to follow.
- A developer-facing set-up requirement, or how to run a suite → `docs/testing.md`.
- A test-structure or test-authoring convention → `agents/test-authoring.md`.
- A design decision → a new record (see [`agents/decisions/README.md`](decisions/README.md)).
- An experiment's outcome with no decision attached → its Issue/PR.
  It also gets an entry in `agents/experiments.md` when it changes what a future agent would do.
- A rule that binds every change → a line in `AGENTS.md`, citing the record that holds its reasons.
- A new real-world app target → an audited row in `agents/apps-audit.md`.
- A trap that misleads whoever takes a measurement → `agents/measurement-records.md`.
  The commands and the methodology stay in `docs/benchmarks/` and `docs/sizes/`.
- A term of the field a sentence needs, or a word the writing rules exclude → `agents/vocabulary.md`.
- A project shipping dewasm output → an entry in [`docs/users.md`](../docs/users.md).
- A performance number → a workload under `benchmarks/`, measured by `cargo xtask record-speed`.
  Never a hand-written figure in text: numbers drift silently.
  The ratio a benchmark reports also depends on the workload.
- A size number → the record `cargo xtask record-size` writes to `records/`.
  It is rendered into `docs/sizes/results.md`.
  The reason is the same: a generated artifact's size changes with every change to code generation.
