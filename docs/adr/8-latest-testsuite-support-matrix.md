# ADR-8 — Track the Latest Testsuite; Attribute Skips to a Support Matrix

Status: **Accepted, 2026-07-23.** Implemented: `Feature` +
`UnsupportedError` (`crates/dewasm-core/src/feature.rs`), attribution
in the harness (`crates/dewasm-test-helper/src/spec.rs`, now running every
top-level `.wast`), and the generated matrix (`docs/support.md`, gated by
the `support_docs` test). Revises ADR-3's skip policy.

## Context

The upstream testsuite tracks the latest spec (Wasm 3.0), while
dewasmify's first-release scope is Wasm 1.0 plus a few extensions.
Curated file lists plus bare skip counts made the numbers meaningless:
894 skips mixed "declared out of scope" with "whatever else", a curated
list silently excluded 200 files, and any real breakage could hide inside
a skip. Pinning an old testsuite was considered and rejected by the user
— upgrades would pile up into one big bang — and no historical snapshot
matches our scope anyway (`wg-1.0` uses directives the modern wast crate
cannot parse and lacks the extensions; bulk memory merged into the spec
together with reference types, so a "bulk without reftypes" suite never
existed).

## Decision

- **The testsuite submodule tracks upstream latest**; updating it is a
  routine, deliberate commit, not a migration.
- **Every conversion refusal is attributed.** The converter's rejections
  carry an `UnsupportedError` naming `Feature`s (typed bails in IR
  building; for validation failures, a probe re-validates with known
  proposals' feature bits and keeps the minimal set that makes the module
  valid). Wasm 1.0 gaps (imported globals/memories/tables, multiple
  tables, bulk table ops) are Features too — declared debt, not silence.
- **The harness only tolerates attributable skips.** Criterion: *a skip
  must be a consequence of a declaration; an unattributable failure is a
  regression.* Unattributed conversion errors fail the suite. Assertion-
  level known failures live in an expected-failures ledger, each entry
  attributed (currently all `linking`). Validation failures beyond every
  proposal this toolchain knows are reported as `unknown-proposal` but
  tolerated — the converter refused cleanly, which is ADR-0's contract.
- **Backends declare their support** (`Backend::feature_status`,
  `baseline`). Flipping a feature to `Supported` makes its remaining
  skips hard failures until the tests actually pass — the declaration and
  the tests cannot drift apart.
- **`docs/support.md` is generated from those declarations** (features ×
  backends, plus the WASI preview 1 table derived from the runtime
  units), gated by a golden-file test; regenerate with
  `DEWASM_UPDATE_DOCS=1 cargo test -p dewasm-cli --test support_docs`.

## Rejected alternatives

- **Pinning an old testsuite commit** — see Context; also loses upstream
  assertion fixes and turns every update into an event.
- **Curated file lists** — drift, and unknown breakage hides in the
  excluded set; running everything with attribution costs ~14 s.
- **Bare expected-failure/skip counts** — numbers without meaning rot;
  attribution is what makes an update reviewable ("simd +120" vs. "??").

## Consequences

- Positive: the whole 257-file suite runs (pass 19,446 → 24,338 just
  from previously-excluded files); `fail=23` are all linking-attributed;
  all 33k skips carry a feature id; the roadmap is literally the
  `unsupported:` table sorted by count.
- Negative / caveats: `names.wast` is rejected wholesale by the wast
  crate's confusing-unicode policy (counted `unknown-proposal`);
  attribution of *validation* failures depends on wasmparser knowing the
  proposal, so a brand-new proposal reports as `unknown-proposal` until
  the toolchain updates.
- ADR-3 remains the testing strategy; its "curated FILES + skip"
  mechanics are superseded by this ADR.
