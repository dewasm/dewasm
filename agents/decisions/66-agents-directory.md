# Decision 66 — `agents/` for Agent-Facing Documents, `docs/` for Human-Facing Ones

Status: **Accepted, 2026-08-11.**
Landed: `docs/adr/` → `agents/decisions/`, `docs/docs-policy.md` → `agents/docs-policy.md`, the term "ADR" replaced by "decision N" throughout, the status vocabulary closed to `Accepted` / `Superseded (decision N)`, and the authoring skill renamed to `dewasm-decision-author` and slimmed to a pointer at `agents/decisions/README.md` § "Adding a new decision".

## Context

The repository's documents have two disjoint readers.
A user or an evaluator opens the README, the tutorial, the per-backend reference, the support matrix, the measurement records.
An agent about to change something reads `AGENTS.md`, the decision records, and the document taxonomy — material a user would never open, and which a user browsing `docs/` had to walk past.

Both lived under `docs/`, so the layout said nothing about who a file was for, and the rule that separates them (only agent-facing documents cite a decision) had to be carried in prose against a directory structure that contradicted it.

The records were named "Architecture Decision Records".
"Architecture" described almost none of them — demo shapes, test harnesses, benchmark design — and the acronym "ADR" only resolves for a reader who already knows the expansion, which is exactly what the vocabulary rule forbids.

The same boundary was blurred one level down: `.claude/skills/adr-author/SKILL.md` had accumulated the actual authoring procedure — numbering, skeleton, index row, verification.
That put content an agent must follow inside a directory specific to one harness, unreachable from `AGENTS.md`.

## Decision

Split the top level by audience.
`agents/` holds the documents an agent reads while doing the work; `docs/` holds the documents a human reads.

- `agents/decisions/` holds the decision records, `agents/docs-policy.md` the taxonomy for both directories.
- The records are "decisions": `agents/decisions/<N>-<slug>.md`, cited as "decision N".
  The term "ADR" is removed everywhere, identifiers included.
- The citation rule is stated as a directory rule: only files under `agents/` and `AGENTS.md` cite a decision.
  Everything else states its constraint in place, and the record links outward.
- A skill under `.claude/skills/` is a Claude Code router — a description that gets auto-invoked, plus a pointer.
  The test: if deleting `.claude/` would lose information rather than convenience, the substance belongs under `agents/`.
  A project-local skill is named with a `dewasm-` prefix, because the skill namespace is shared with the user's global skills.
- A status label is exactly `Accepted` or `Superseded (decision N)`, the parenthetical required.
  `Proposed` is retired: a decision is recorded when it is made, and a tentative idea lives in an issue until then.
  Scope and progress qualifiers ("Ruby only", "relay protocol superseded by decision 58") go in the Status paragraph, which can say why.

**Discriminating criterion:** *classify a document by its reader, not by its subject, and put the classification in the directory tree — a rule that the layout contradicts has to be re-argued every time someone adds a file.*

## Rejected alternatives

- **Keep the records under `docs/` and mark them agent-facing in prose.**
  That was the previous state: `docs-policy.md` already said `docs/adr/` was for agents and that user docs must not cite it.
  It did not stop the two audiences from interleaving, because the tree is the first thing a reader and a writer both consult.

- **Name the directory `agents/memory/`.**
  "Memory" is reserved as the umbrella if this ever broadens to non-decision knowledge entries.
  Decisions and knowledge have different lifecycles — a decision is fixed once made and retired only by being superseded by name, while a knowledge entry is edited in place as it goes stale — so they should not share a directory, and certainly not before the second kind exists.

- **Keep `ADR-N` as opaque historical identifiers.**
  Every reader must still be told what the letters once meant, so the vocabulary debt never amortizes; a one-time mechanical rename ends it.
  References written as "ADR-58" in old issues, pull requests, and commits still resolve, because the number is the identifying part.

- **Leave the authoring procedure in the skill and let `agents/decisions/README.md` hold only the quality bar.**
  The split was the source of the drift: the skill's copy of the numbering and index rules is exactly the part that goes stale, and only Claude Code can see it.

## Consequences

- Positive: the audience question is answered by the path, so a new document has one obvious home and the citation rule reads off the tree.
- Positive: the decision records are reachable by any agent from `AGENTS.md` alone; `.claude/` holds routing only, and can be deleted without loss of substance.
- Negative: links to `docs/adr/...` from outside the repository — issues, pull requests, external references — break, since a moved path is not redirected.
  Paid once, on the smallest tree this project will have again.
- Carry-over: `agents/` currently holds decisions and the policy.
  A later non-decision knowledge entry lands as a sibling under `agents/`, not inside `decisions/`.
