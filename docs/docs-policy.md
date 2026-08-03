# Documentation policy

The taxonomy of dewasm's docs, so new content lands in one obvious place and nothing is duplicated. All docs are written in English.

| Doc | Role | Audience | Editable |
| --- | --- | --- | --- |
| [README.md](../README.md) | Front door: what dewasm is, one showpiece, install, usage, scope | Users evaluating the project | By hand |
| [docs/getting-started.md](getting-started.md) | Tutorial: a verified end-to-end walkthrough | New users | By hand (verify every command) |
| [docs/backends/](backends/) | Per-target reference: output shape, requirements, caveats, provider usage | Users of a specific target | By hand |
| [docs/standalone-interface.md](standalone-interface.md) | The standalone runtime interface (argv, `--dir`, env, exit/trap), uniform across backends | Users running standalone output | By hand |
| [docs/support.md](support.md) | The feature / WASI matrix per backend | Everyone | **Generated — never hand-edit** |
| [docs/apps-audit.md](apps-audit.md) | The real-world app gate record and feature verdicts | Contributors, evaluators | By hand |
| [docs/testing.md](testing.md) | What `cargo test` needs and why it fails loud | Contributors | By hand |
| [docs/related-work.md](related-work.md) | Comparison with prior art | Evaluators | By hand |
| [docs/benchmarks/](benchmarks/README.md) | How to run the benchmark suite and read its numbers | Contributors | By hand |
| [docs/benchmarks/results.md](benchmarks/results.md) | Measured performance, with figures under `figs/` | Evaluators | **Generated — never hand-edit** |
| [docs/adr/](adr/README.md) | Design decisions with rationale and rejected alternatives | Contributors, future maintainers | By hand (via the `adr-author` skill) |

## Rules

- **`docs/support.md` is generated** from the backend declarations. Never edit it by hand; regenerate with `cargo xtask update-support-docs` (`cargo test -p dewasm-cli --test support_docs` fails while the file is stale). Everywhere else, **link** to it rather than copying the matrix.
- **Decisions go in an ADR, not in prose docs.** Anything with real alternatives is recorded under `docs/adr/` (the `adr-author` skill carries the procedure). Other docs neither restate nor link into `docs/adr/`: an ADR cites docs and code, never the other way around. Where a pointer helps, name the ADR without a link.
- **Tutorial commands must be verified by running them.** getting-started and the backend docs claim exact output; keep them true.

## Where new content goes

- A user-facing capability or a new target → a line in the README table, a page under `docs/backends/`, and (if it needs a walkthrough) a section in getting-started.
- A contributor-facing setup requirement → `docs/testing.md`.
- A design decision → a new ADR (see [docs/adr/README.md](adr/README.md)).
- A new real-world app target → an audited row in `docs/apps-audit.md`.
- A performance number → a workload under `benchmarks/`, measured by `cargo xtask bench`. Never a hand-written figure in prose: numbers drift silently, and the ratio a benchmark reports depends on the workload ([ADR-57](adr/57-benchmark-harness.md)).
