# Decision 67: An Experiments Index over Issues and PRs

Status: **Accepted, 2026-08-12.**
Landed: `agents/experiments.md` (the index and its entry shape); entries arrive with the experiments they record.

## Context

An experiment that ends in "change nothing" leaves its record in a closed Issue or PR, per the decisions quality bar (a survey or measurement with no decision attached stays out of `agents/decisions/`).
Agents browse the repository tree, not the issue tracker, so those verdicts are invisible at exactly the moment they matter: when the same idea is about to be proposed again.
The same knowledge also accumulates in one machine's local agent memory, unreachable from other machines, subagents, and other harnesses.

## Decision

`agents/experiments.md` is an index of past experiments.

- The full record stays in the Issue or PR; the entry carries the conclusion itself (the verdict with its deciding number, and what would invalidate it), so the index is sufficient without network access.
- Each entry is a structured section: a heading (slug, one-line conclusion, date) and the fixed keys Tried / Verdict / Invalidated when / Details.
- The quality bar: an entry earns its place only if it changes what a future agent would do.

**Discriminating criterion:** *store the conclusion where agents look (the tree), and leave the evidence where it already accumulated (the Issue or PR).*

## Rejected alternatives

- **A per-file `agents/experiments/` directory.**
  Full-text entries duplicate what the Issue or PR already holds, and the directory grows without bound; the conclusion is the only part an agent needs at proposal time.

- **Leaving results only in Issues and PRs.**
  That was the status quo: correct storage, no discoverability.

- **Recording experiments as decisions.**
  An experiment decides nothing; forcing one into the decision skeleton pads it, and the decisions quality bar rightly rejects it.

## Consequences

- Positive: a rejected idea stays rejected visibly; a re-proposal meets the verdict in the tree before any work starts.
- Positive: machine-local agent memory about this repository can migrate here entry by entry.
- Negative: an entry can go stale silently; the required "Invalidated when" key is the mitigation, naming the re-test condition instead of leaving staleness to intuition.
- This settles decision 66's carry-over: the non-decision knowledge layer is an index over Issues and PRs, not a sibling directory of full entries.
