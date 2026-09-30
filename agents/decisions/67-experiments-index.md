# Decision 67: An Experiments Index over Issues and PRs

Status: **Accepted, 2026-08-12.**
Landed: `agents/experiments.md` (the index and its entry shape).
Entries arrive with the experiments they record.
The conclusion moved out of the heading into the entry's first sentence (#341).

## Context

An experiment that ends in "change nothing" leaves its record in a closed Issue or PR.
That follows the decisions quality bar.
A survey or measurement with no decision attached stays out of `agents/decisions/`.
Agents read the repository tree, not the issue tracker.
So those verdicts are not seen at exactly the moment they matter.
That moment is when the same idea is about to be proposed again.
The same knowledge also accumulates in one machine's local agent memory.
That memory is unreachable from other machines, subagents, and other harnesses.

## Decision

`agents/experiments.md` is an index of past experiments.

- The full record stays in the Issue or PR.
  The entry carries the conclusion itself.
  That is the verdict with its deciding number, and what would invalidate it.
  So the index is sufficient without network access.
- Each entry is a structured section.
  It has a heading (slug, date), then its conclusion in one sentence.
  It has the fixed keys Tried / Verdict / Invalidated when / Details.
- The quality bar: an entry earns its place only if it changes what a future agent would do.

**Deciding criterion:** *store the conclusion where agents look (the tree).*
*Leave the evidence where it already accumulated (the Issue or PR).*

## Rejected alternatives

- **A per-file `agents/experiments/` directory.**
  Full-text entries repeat what the Issue or PR already holds.
  The directory also grows without bound.
  The conclusion is the only part an agent needs at proposal time.

- **Leaving results only in Issues and PRs.**
  That was the current state: correct storage, no discoverability.

- **Recording experiments as decisions.**
  An experiment decides nothing; in the decision template, its sections would say nothing.
  The decisions quality bar rightly rejects it.

## Consequences

- Positive: a rejected idea stays rejected visibly.
  A re-proposal meets the verdict in the tree before any work starts.
- Positive: machine-local agent memory about this repository can migrate here entry by entry.
- Negative: an entry can go out of date silently.
  The required "Invalidated when" key reduces that risk.
  It names the re-test condition, so no one has to guess whether an entry still holds.
- This settles decision 66's carry-over.
  The non-decision knowledge layer is an index over Issues and PRs.
  It is not a second directory of full entries beside the decisions.
