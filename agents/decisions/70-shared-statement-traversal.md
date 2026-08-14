# Decision 70: Shared Statement Traversal for IR Analyses

Status: **Accepted, 2026-08-14.**
`Stmt::child_seqs` and `Stmt::any` exist in `crates/dewasm-core/src/ir.rs`, every recursive boolean search over statement trees rides them, and the frame analysis feeding `flat::plan` is one walker in `crates/dewasm-backend/src/flat.rs`.

## Context

Thirty-one sites across the backend crates match on `ir::Stmt`, and before this decision sixteen recursed or classified through silent wildcard arms.
Adding `Stmt::TryTable` for exception handling shipped two bugs of the same shape: a hand-written recursive helper (Python's relay-branch probe, Perl's `br_table` probe) was not taught about the new variant and silently skipped its body.
The IR offered no shared way to enumerate a statement's nested sequences, so every analysis wrote its own recursion and every new variant had to find them all.

## Decision

A statement's nested sequences are declared in exactly one place: `Stmt::child_seqs`, whose match is exhaustive, so a new body-carrying variant is a compile error there and nowhere else.
The rule for a `Stmt` analysis follows from what it asks:

- A **search** ("does any statement in these trees satisfy this predicate") goes through the shared traversal (`Stmt::any` or an explicit `child_seqs` walk) and never writes its own recursion.
- A **leaf classifier** (a function that only ever receives leaf statements by construction, or a deliberately conservative classification) may keep a silent wildcard, and must state in a comment the invariant that makes silence safe.

The same criterion applied one level up deduplicated the frame analysis feeding `flat::plan`: Ruby's and Python's copies differed in exactly one capability (whether the language has a break to a block end), so that difference became a parameter and the walker lives once, in `crates/dewasm-backend/src/flat.rs` next to its consumer.

## Rejected alternatives

- **Exhaustive matches in every walker.**
  Turns each of the sixteen silent sites into a twenty-five-arm match; the two shipped bugs were in helpers whose authors reasonably wanted to name three variants, and the noise would invite `_ =>` back within a release.
- **A visitor trait with per-variant methods.**
  The analyses here are one-predicate searches; a visitor's ceremony exceeds every current consumer, and the emitters (which do need per-variant behavior) already have their own exhaustive matches.
- **Leaving the duplication and relying on review.**
  That was the state that shipped both bugs in one feature.

## Consequences

- Positive: the "walker not taught about the new variant" class is gone for searches; a new variant compiles only after declaring its children once.
- Positive: the flat-plan analysis cannot drift between Ruby and Python again.
- Carry-over: emitters and leaf classifiers still match `Stmt` directly; their safety rests on the documented invariants, which review must keep honest.
