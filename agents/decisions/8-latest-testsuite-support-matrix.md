# Decision 8: Track the Latest Testsuite; Attribute Skips to a Support Matrix

Status: **Accepted, 2026-07-23.**
Implemented:

- `Feature` + `UnsupportedError` (`crates/dewasm-core/src/feature.rs`);
- attribution in the harness (`crates/dewasm-test-helper/src/spec.rs`);
- the generated matrix (`docs/support.md`), controlled by the `support_docs` test.

The harness now runs every top-level `.wast`.
Revises decision 3's skip policy.

## Context

The upstream testsuite tracks the latest specification (Wasm 3.0).
But `dewasmify`'s first-release scope is Wasm 1.0 plus a few extensions.
Selected file lists plus plain skip counts made the numbers mean nothing:

- 894 skips mixed "declared out of scope" with "whatever else";
- a selected list silently excluded 200 files;
- any real failure could hide inside a skip.

Staying on an old testsuite was considered and rejected by the user.
Updates would pile up into one large step.
No historical snapshot matches our scope anyway.
`wg-1.0` uses directives the modern `wast` crate cannot parse, and it lacks the extensions.
Bulk memory merged into the specification together with reference types.
So a "bulk without reftypes" suite never existed.

## Decision

- **The testsuite submodule tracks upstream latest.**
  Updating it is a routine, intended commit, not a migration.
- **Every conversion refusal is attributed.**
  The converter's rejections carry an `UnsupportedError` naming `Feature`s.
  In IR building, these come from typed early returns.
  For validation failures, a probe re-validates with known proposals' feature bits.
  It keeps the minimal set that makes the module valid.
  Wasm 1.0 gaps are Features too.
  Those are imported globals/memories/tables, multiple tables, and bulk table operations.
  They are declared debt, not silence.
- **The harness only accepts attributable skips.**
  Criterion: *a skip must be a consequence of a declaration.*
  *An unattributable failure is a regression.*
  Unattributed conversion errors fail the suite.
  Assertion-level known failures live in an expected-failures list.
  Each entry is attributed (currently all `linking`).
  Validation failures beyond every proposal this toolchain knows are reported as `unknown-proposal`.
  But they are accepted, since the converter refused cleanly.
  Refusing cleanly is decision 0's contract.
- **Backends declare their support** (`Backend::feature_status`, `baseline`).
  Flipping a feature to `Supported` makes its remaining skips hard failures.
  They stay failures until the tests actually pass.
  So the declaration and the tests cannot drift apart.
- **`docs/support.md` is generated from those declarations**, controlled by a snapshot-file test.
  It holds features × backends, plus the WASI Preview 1 table derived from the runtime units.
  Regenerate it with `DEWASM_UPDATE_DOCS=1 cargo test -p dewasm-cli --test support_docs`.

## Rejected alternatives

- **Staying on an old testsuite commit**: see Context.
  It also loses upstream assertion fixes and turns every update into an event.
- **Selected file lists**: drift, and unknown failures hide in the excluded set.
  Running everything with attribution costs ~14 s.
- **Plain expected-failure/skip counts**: numbers without meaning go out of date.
  Attribution is what makes an update reviewable ("simd +120" vs. "??").

## Consequences

- Positive: the whole 257-file suite runs.
  The pass count went 19,446 → 24,338 just from previously-excluded files.
  `fail=23` are all linking-attributed.
  All 33k skips carry a feature identifier.
  The plan of future work is the `unsupported:` table sorted by count.
- Negative / limits: the `wast` crate's policy on confusing Unicode rejects `names.wast` whole.
  It is counted `unknown-proposal`.
  Attribution of *validation* failures depends on `wasmparser` knowing the proposal.
  So a brand-new proposal reports as `unknown-proposal` until the toolchain updates.
- Decision 3 remains the testing strategy.
  This decision replaces its "curated FILES + skip" mechanics.
