# Decision 87: Record Schema Evolution by In-Place Migration

Status: **Accepted, 2026-08-30.**
A new record schema version ships with a migration in `cargo xtask migrate-records`.
The command is in [`crates/xtask/src/migrate.rs`](../../crates/xtask/src/migrate.rs).
It rewrites every stored record under `records/` in place.
Every command that reads a record supports only the current schema.
When such a command meets an older schema, it names the migrate command.

## Context

Speed-record schema 2 moved a skipped cell's classification into its own field.
The classification is one of `cost`, `capability`, and `setup`.
Before, the classification was part of the reason string.
Nine committed schema-1 records were older than it.
Something has to read them.
Either every consumer keeps a fallback for old shapes, or the stored files are brought forward once.
More record-consuming commands are expected, for one a command comparing two records.
They multiply whatever choice is made here.

## Decision

Cross-version knowledge lives in exactly one place, the migration.
Stored records are always at the current schema.
A reader checks the version and stops; it never interprets an old shape.
The migration preserves everything it does not transform.
Measurements are written back byte-identically.
`serde_json`'s `float_roundtrip` feature exists for this.
That is because the default parser changes the last ulp of a stored value.

## Rejected alternatives

- **Per-reader fallbacks.**
  These derive the classification from the legacy reason prefix at render time.
  It works for one reader, but every future command re-implements the same derivation.
  The stored files also stay ambiguous forever.
- **Keeping old records at their original schema as immutable measurement artifacts.**
  The measurements are what must not change, and the migration does not touch them.
  The encoding around them is the tool's, not the measurement's.

## Consequences

- Positive: a new record consumer is written against one schema.
  The schema constant and the version check live beside each record type.
  Those types are in `bench::report` and `size::report`.
- Negative: a new schema version is not done until the migration is written.
  The committed records must also be rewritten.
  That makes small schema changes slightly more expensive.
- Carry-over: the v1-to-v2 migration classifies legacy reasons by their prefix.
  One content-matched exception is recorded in `classify_v1_reason`.
