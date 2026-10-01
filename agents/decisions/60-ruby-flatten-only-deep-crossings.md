# Decision 60: Ruby Backend Flattens Only Deep Crossings

Status: **Accepted, 2026-08-04.**
`flat::plan` is in the `flat` module of `crates/dewasm-backend-ruby/src/lib.rs`.
It dissolves a frame only when some branch crossing it spans at least `DEEP_CROSSING` (16) frames.
Every shallower branch keeps decision 42's `__br` relay, in the same function, side by side.
Refines [decision 58](58-ruby-branch-by-value.md), which flattened every crossed frame.

## Context

Profiling the NES example (issue #116) found the hottest single line of the whole workload.
It was the flat lowering's dispatch probe.
The PPU dot loop runs 89,341 trips per frame.
Its back-edge had been dissolved into a state transition.
The `case state` probe alone cost 11.8% of wall time (~1.24M dispatches/frame).
Decision 58's own motivation was the opposite shape: SQLite's VDBE.
There a `br_table` crosses hundreds of frames.
The relay's per-level compares then cost far more than one dispatch.
Both are real; the difference is *how deep* the branch is.

## Decision

Weigh each branch, not each function.
A relay costs one compare per crossed frame.
That is measured at ~0.8 ns per level under `--yjit`, flat from depth 2 to 32.
A dispatch is a `case`-over-integers whose cost grows with the number of hot states.
That is ~0.9 ns at 3 hot states and ~25 ns at 80.
The break-even sits between ~5 and ~30 crossed frames depending on machine size.
So any threshold in that band is a judgement call, recorded as `flat::DEEP_CROSSING = 16`.
It puts both measured workloads on the side each prefers:

- `nes.wasm` crosses at most 12 frames anywhere.
  It is ~1.16-1.18× faster with no frame flattened (11.2 → 13.0 t/s).
- `sqlite3-shell` reaches 278 and was 2.08× faster flattened (decision 58's original result).
  It still has 22 flattened functions after this change.

Mechanically, `FrameSets` now records one frame *path*, ends included, per outward branch.
Frames are dissolved until two closure rules together reach a fixed point:

- A branch is all-or-nothing.
  Once any frame on its path dissolves, the whole path goes.
  The reason: a relayed `break`/`next` could no longer thread past the dispatch loop.
- Dissolving a frame still dissolves every frame outside it.
  That is because a surviving Ruby loop would capture a `next` aimed at the dispatch.

Relay and dispatch can both appear in one function.
`__br` is hoisted whenever any crossed frame survives.

## Rejected alternatives

- **Keep decision 58's flatten-everything.**
  Loses ~15% on the NES workload for no correctness gain.
  It contradicts the `flat` module's own documented "flatten branches, not loops" finding.
- **Structured loops nested inside the flat function.**
  This would emit a real `while` around a state sub-range no external transition enters.
  Strictly more general, but the depth threshold already separates every workload measured.
  It does so without a second lowering form to verify.
  Revisit only if a module shows a deep crossing *and* a hot interior loop in the same function.
- **A derived (non-judgement) threshold.**
  The dispatch's cost depends on the hot-state count, unknowable at conversion time.
  Pretending to derive the constant would just hide the band (5-30) the measurements actually support.

## Consequences

- Positive: NES 11.2 → 13.0 t/s (+16%, Alter Ego).
  The gain drops on ROMs whose dots cost more memory traffic, since the dispatch share is smaller.
  The specification harness, DOOM and NES snapshots, and SQLite's flattening are all unchanged.
- Negative: `DEEP_CROSSING` is a constant chosen by judgement inside a measured band, not a law.
  A workload whose hot loop sits under a ≥16-deep crossing would still dissolve it.
  The rejected nested form is the way out.
- Carry-over: the tests on the shape of generated code check both sides of the threshold.
  They are in `crates/dewasm-backend-ruby/src/lib.rs`:

  - `deep_multi_level_br_is_addressed_by_value`;
  - `shallow_multi_level_br_keeps_the_relay`;
  - `mixed_depths_stay_structured`.
