# Decision 66: `agents/` for Agent-Facing Documents, `docs/` for Human-Facing Ones

Status: **Accepted, 2026-08-11.**
Landed:

- `docs/adr/` → `agents/decisions/`, `docs/docs-policy.md` → `agents/docs-policy.md`;
- the term "ADR" replaced by "decision N" throughout;
- the status vocabulary closed to `Accepted` / `Superseded (decision N)`;
- the authoring skill renamed to `dewasm-decision-author` and cut down to a pointer.
  It points at `agents/decisions/README.md` § "Adding a new decision".

## Context

The repository's documents have two separate groups of readers.
A user or someone judging the project opens the README, the guide, and the per-backend reference.
They also open the support matrix and the measurement records.
An agent about to change something reads `AGENTS.md`, the decisions, and the document classification.
That is material a user would never open, and a user looking through `docs/` had to walk past it.

Both lived under `docs/`, so the layout said nothing about who a file was for.
A rule separates them: nothing outside the agent-facing set references the agent-facing material.
That rule had to be carried in text against a directory structure that contradicted it.

The records were named "Architecture Decision Records".
"Architecture" described almost none of them (example shapes, test harnesses, benchmark design).
The short form "ADR" only resolves for a reader who already knows the expansion.
That is exactly what the vocabulary rule does not allow.

The same boundary was unclear one level down.
`.claude/skills/adr-author/SKILL.md` had accumulated the actual authoring procedure.
That was numbering, template, index row, and verification.
It put content an agent must follow inside a directory specific to one harness.
That content was unreachable from `AGENTS.md`.

## Decision

Split the top level by audience.
`agents/` holds the documents an agent reads while doing the work.
`docs/` holds the documents a human reads.

- `agents/decisions/` holds the decision records.
  `agents/docs-policy.md` holds the classification for both directories.
- The records are "decisions": `agents/decisions/<N>-<slug>.md`, cited as "decision N".
  The term "ADR" is removed everywhere, identifiers included.
- The reference rule is stated as a directory rule.
  Nothing outside `agents/` references anything under it.
  `AGENTS.md`, `CLAUDE.md`, and `.claude/` are excepted.
  Everything else states its constraint in place, and an `agents/` document links out.
- A skill under `.claude/skills/` is a Claude Code router.
  It is a description that makes it load automatically, plus a pointer.
  The test is whether removing `.claude/` would lose information rather than ease of use.
  If it would, the substance belongs under `agents/`.
  A project-local skill is named with a `dewasm-` prefix.
  The reason is that the skill namespace is shared with the user's global skills.
- A status label is exactly `Accepted` or `Superseded (decision N)`, the part in parentheses required.
  `Proposed` is retired: a decision is recorded when it is made.
  An idea not yet decided lives in an issue until then.
  Scope and progress qualifiers go in the Status paragraph, which can say why.
  Examples are "Ruby only" and "relay protocol superseded by decision 58".

**Deciding criterion:** *classify a document by its reader, not by its subject.*
*Put the classification in the directory tree.*
*A rule that the layout contradicts has to be re-argued every time someone adds a file.*

## Rejected alternatives

- **Keep the records under `docs/` and mark them agent-facing in text.**
  That was the previous state.
  `docs-policy.md` already said `docs/adr/` was for agents and that user documents must not cite it.
  It did not stop the documents for the two audiences from mixing.
  The reason is that the tree is the first thing a reader and a writer both consult.

- **Name the directory `agents/memory/`.**
  "Memory" is reserved as the name for a wider set.
  It is for the case that this ever broadens to non-decision knowledge entries.
  Decisions and knowledge have different life cycles.
  A decision is fixed once made, and it is retired only by a later decision that names it.
  A knowledge entry is edited in place as it goes out of date.
  So they should not share a directory, and certainly not before the second kind exists.

- **Keep `ADR-N` as historical identifiers with no meaning of their own.**
  Every reader must still be told what the letters once meant.
  So every reader pays that vocabulary cost again; a one-time mechanical rename ends it.
  References written as "ADR-58" in old issues, pull requests, and commits still resolve.
  The reason is that the number is the identifying part.

- **Leave the authoring procedure in the skill.**
  **Let `agents/decisions/README.md` hold only the quality bar.**
  The split was the source of the drift.
  The skill's copy of the numbering and index rules is exactly the part that goes out of date.
  Only Claude Code can see it.

## Consequences

- Positive: the audience question is answered by the path.
  So a new document has one obvious home, and the reference rule reads off the tree.
- Positive: the decision records are reachable by any agent from `AGENTS.md` alone.
  `.claude/` holds routing only, and can be removed without loss of substance.
- Negative: some links to `docs/adr/...` come from outside the repository.
  Those links are in issues, pull requests, and external references.
  They break, since a moved path is not redirected.
  Paid once, on the smallest tree this project will have again.
- Carry-over: `agents/` currently holds decisions and the policy.
  A later non-decision knowledge entry lands directly under `agents/`, not inside `decisions/`.
