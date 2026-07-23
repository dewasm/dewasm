---
name: adr-author
description: |
  Author a new Architecture Decision Record under docs/adr/. Use when the user asks to "write an
  ADR", "record this decision", "add ADR-N for X", or when a design discussion reaches a decision
  worth recording. Covers the go/no-go call (does this need an ADR at all), the skeleton, the
  numbering, the docs/adr/README.md index row, and verification. The quality bar itself lives in
  docs/adr/README.md § "Adding a new ADR" — this skill is the procedure, not a substitute for it.
---

# Author an ADR

The quality bar is defined in [`docs/adr/README.md`](../../../docs/adr/README.md) § "Adding a
new ADR" — read it; do not re-derive it here. This skill is the mechanical procedure, which is
where ADR authoring actually slips (numbering, index drift, missing cross-refs).

The recurring authoring mistake is writing too much for the stakes, not too little. Bias toward
brevity; a small decision must read small. ADR-5 is a reasonable length target for a
policy-sized decision, ADR-0 for a foundation-sized one.

## Step 0 — Does this need an ADR?

An ADR records a **decision with rationale and rejected alternatives**, or a standing policy.
Not an ADR:

- A mechanical change with no live alternatives → commit message.
- Behaviour the spec harness already enforces → the harness binds
  ([ADR-3](../../../docs/adr/3-testing-strategy.md)); an ADR only ever records *why*.
- A survey or measurement with no decision attached → keep it out, or park it in the PR/issue
  and cite it.

If no alternatives were weighed, there is no decision to record.

## Step 1 — Number and file

```sh
ls docs/adr/          # highest N + 1 is yours; no zero padding
```

Create `docs/adr/<N>-<slug>.md` with the structural contract:

```markdown
# ADR-N — <title>

Status: **<Accepted|Proposed>, <YYYY-MM-DD>.** <one paragraph: what landed / what remains.>

## Context
## Decision            <- include the discriminating criterion as a reusable rule
## Rejected alternatives  <- each with the reason it lost
## Consequences        <- positive / negative / carry-over
```

Anchor claims to real files (`crates/.../file.rs`, `runtime/<lang>/...`).

## Step 2 — Index

Add the row to the table in [`docs/adr/README.md`](../../../docs/adr/README.md), in ascending
order, with the capped status (e.g. `Accepted (not yet implemented)`).

## Step 3 — Cross-references

Link related ADRs from the new one and back-link where it matters. If the decision changes how
contributors must work, add or adjust the one-line rule in `AGENTS.md` citing the ADR — the rule
there, the why in the ADR, never both places in full.

## Verification

- Index row count equals ADR file count: `ls docs/adr/*.md | grep -v README | wc -l` vs. the
  table.
- Every relative link in the new ADR resolves (`ls` the targets).
- If the ADR supersedes another, the old one's Status says so and links forward.
