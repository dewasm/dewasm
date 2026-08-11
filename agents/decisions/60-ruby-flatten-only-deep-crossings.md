# Decision 60: Ruby Backend Flattens Only Deep Crossings

Status: **Accepted, 2026-08-04.**
`flat::plan` (the `flat` module of `crates/dewasm-backend-ruby/src/lib.rs`) dissolves a frame only when some branch crossing it spans at least `DEEP_CROSSING` (16) frames; every shallower branch keeps decision 42's `__br` relay, in the same function, side by side.
Refines [decision 58](58-ruby-branch-by-value.md), which flattened every crossed frame.

## Context

Profiling the NES example (issue #116) showed the flat lowering's dispatch probe as the hottest single line of the whole workload: the PPU dot loop runs 89,341 trips per frame, its back-edge had been dissolved into a state transition, and the `case state` probe alone cost 11.8% of wall time (~1.24M dispatches/frame).
Decision 58's own motivation was the opposite shape: sqlite's VDBE, where a `br_table` crosses hundreds of frames and the relay's per-level compares dwarf one dispatch.
Both are real; the difference is *how deep* the branch is.

## Decision

Weigh each branch, not each function.
A relay costs one compare per crossed frame (measured at ~0.8 ns/level under `--yjit`, flat from depth 2 to 32), while a dispatch is a `case`-over-integers whose cost grows with the number of hot states (~0.9 ns at 3, ~25 ns at 80).
The break-even sits between ~5 and ~30 crossed frames depending on machine size, so any threshold in that band is a judgement call, recorded as `flat::DEEP_CROSSING = 16`: it puts both measured workloads on the side each prefers: `nes.wasm` crosses at most 12 frames anywhere and is ~1.16-1.18× faster fully cascaded (11.2 → 13.0 t/s), `sqlite3-shell` reaches 278 and was 2.08× faster flattened (decision 58's original result; still 22 flattened functions after this change).

Mechanically, `FrameSets` now records one inclusive frame *path* per outward branch, and dissolution runs to a joint fixpoint over two closures: a branch is all-or-nothing (once any frame on its path dissolves, a relayed `break`/`next` could no longer thread past the dispatch loop, so the whole path goes), and dissolution stays transitive up the spine (a surviving Ruby loop would capture a `next` aimed at the dispatch).
Relay and dispatch coexist in one function; `__br` is hoisted whenever any crossed frame survives.

## Rejected alternatives

- **Keep decision 58's flatten-everything.**
  Loses ~15% on the NES workload for no correctness gain; contradicts the `flat` module's own documented "flatten branches, not loops" finding.
- **Structured loops nested inside the flat function** (emit a real `while` around a state sub-range no external transition enters).
  Strictly more general, but the depth threshold already separates every workload measured, without a second lowering form to verify; revisit only if a module shows a deep crossing *and* a hot interior loop in the same function.
- **A derived (non-judgement) threshold.**
  The dispatch's cost depends on the hot-state count, unknowable at conversion time; pretending to derive the constant would just hide the band (5-30) the measurements actually support.

## Consequences

- Positive: NES 11.2 → 13.0 t/s (+16%, Alter Ego; the gain shrinks on ROMs whose dots cost more memory traffic, since the dispatch share is smaller); the spec harness, DOOM and NES snapshots, and sqlite's flattening are all unchanged.
- Negative: `DEEP_CROSSING` is a pinned judgement inside a measured band, not a law; a workload whose hot loop sits under a ≥16-deep crossing would still dissolve it (see the rejected nested form for the escape hatch).
- Carry-over: the codegen-shape tests pin both sides of the threshold (`deep_multi_level_br_is_addressed_by_value`, `shallow_multi_level_br_keeps_the_relay`, `mixed_depths_stay_structured` in `crates/dewasm-backend-ruby/src/lib.rs`).
