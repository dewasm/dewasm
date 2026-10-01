# Decision 70: Shared Statement Walk for IR Analyses

Status: **Accepted, 2026-08-14.**
`Stmt::child_seqs` and `Stmt::any` exist in `crates/dewasm-core/src/ir.rs`.
Every recursive Boolean search over statement trees rides them.
The frame analysis feeding `flat::plan` is one walker in `crates/dewasm-backend/src/flat.rs`.

## Context

Thirty-one sites across the backend crates match on `ir::Stmt`.
Before this decision sixteen of them used recursion or classified through silent wildcard arms.
Adding `Stmt::TryTable` for exception handling shipped two bugs of the same shape.
A hand-written recursive helper was not taught about the new variant and silently skipped its body.
The helpers were Python's relay-branch probe and Perl's `br_table` probe.
The IR offered no shared way to list a statement's nested sequences.
So every analysis wrote its own recursion, and every new variant had to find them all.

## Decision

A statement's nested sequences are declared in exactly one place: `Stmt::child_seqs`.
Its match is exhaustive, so a new body-carrying variant is a compile error there and nowhere else.
The rule for a `Stmt` analysis follows from what it asks:

- A **search** asks "does any statement in these trees satisfy this predicate".
  It goes through the shared walk (`Stmt::any` or an explicit `child_seqs` walk).
  It never writes its own recursion.
- A **leaf classifier** may keep a silent wildcard.
  It is a function that only ever receives leaf statements by construction.
  Or it is a deliberately conservative classification.
  It must state in a comment the invariant that makes silence safe.

The same criterion applied one level up merged two copies of the frame analysis feeding `flat::plan`.
Ruby's and Python's copies differed in exactly one capability.
That capability is whether the language has a break to a block end.
So that difference became a parameter.
The walker therefore lives once, in `crates/dewasm-backend/src/flat.rs` next to its consumer.

## Rejected alternatives

- **Exhaustive matches in every walker.**
  Turns each of the sixteen silent sites into a twenty-five-arm match.
  The two shipped bugs were in helpers whose authors reasonably wanted to name three variants.
  The noise would invite `_ =>` back within a release.
- **A visitor trait with per-variant methods.**
  The analyses here are searches for one condition.
  A visitor's ceremony exceeds every current consumer.
  The emitters, which do need per-variant behavior, already have their own exhaustive matches.
- **Leaving the copies and relying on review.**
  That was the state that shipped both bugs in one feature.

## Consequences

- Positive: the "walker not taught about the new variant" class is gone for searches.
  A new variant compiles only after declaring its children once.
- Positive: the flat-plan analysis cannot drift between Ruby and Python again.
- Carry-over: emitters and leaf classifiers still match `Stmt` directly.
  Their safety rests on the documented invariants, which review must keep honest.
