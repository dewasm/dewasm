# Decision 87: Record Schema Evolution by In-Place Migration

Status: **Accepted, 2026-08-30.** A record schema bump ships with an upgrade in `cargo xtask migrate-records` ([`crates/xtask/src/migrate.rs`](../../crates/xtask/src/migrate.rs)) that rewrites every stored record under `records/` in place; every command that reads a record supports only the current schema and names the migrate command when it meets an older one.

## Context

Speed-record schema 2 moved a skipped cell's classification (cost, capability, setup) out of the reason string into its own field, and nine committed schema-1 records predated it.
Something has to read them: either every consumer keeps a fallback for old shapes, or the stored files are brought forward once.
More record-consuming commands are expected (comparing two records, for one), which multiplies whatever choice is made here.

## Decision

Cross-version knowledge lives in exactly one place, the migration, and stored records are always at the current schema.
A reader checks the version and stops; it never interprets an old shape.
The migration preserves everything it does not transform: measurements re-serialize byte-identically (`serde_json`'s `float_roundtrip` feature exists for this; the default parser drifts a stored value's last ulp).

## Rejected alternatives

- **Per-reader fallbacks (deriving the classification from the legacy reason prefix at render time).**
  Works for one reader, but every future command re-implements the same derivation, and the stored files stay ambiguous forever.
- **Keeping old records at their original schema as immutable measurement artifacts.**
  The measurements are what must not change, and the migration does not touch them; the encoding around them is the tool's, not the measurement's.

## Consequences

- Positive: a new record consumer is written against one schema; the schema constant and the version check live beside each record type (`bench::report`, `size::report`).
- Negative: a schema bump is not done until the migration is written and the committed records are rewritten, which makes small schema changes slightly more expensive.
- Carry-over: the v1-to-v2 upgrade classifies legacy reasons by their prefix, with one content-matched exception recorded in `classify_v1_reason`.
