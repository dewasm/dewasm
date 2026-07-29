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
| [docs/adr/](adr/README.md) | Design decisions with rationale and rejected alternatives | Contributors, future maintainers | By hand (via the `adr-author` skill) |

## Rules

- **`docs/support.md` is generated** from the backend declarations. Never edit it by hand; regenerate with `cargo xtask update-support-docs` (`cargo test -p dewasm-cli --test support_docs` fails while the file is stale). Everywhere else, **link** to it rather than copying the matrix.
- **Decisions go in an ADR, not in prose docs.** Anything with real alternatives is recorded under `docs/adr/` (the `adr-author` skill carries the procedure). Other docs *link* to the ADR for the "why"; they do not restate it.
- **Tutorial commands must be verified by running them.** getting-started and the backend docs claim exact output; keep them true.

## Where new content goes

- A user-facing capability or a new target → a line in the README table, a page under `docs/backends/`, and (if it needs a walkthrough) a section in getting-started.
- A contributor-facing setup requirement → `docs/testing.md`.
- A design decision → a new ADR (see [docs/adr/README.md](adr/README.md)).
- A new real-world app target → an audited row in `docs/apps-audit.md`.
