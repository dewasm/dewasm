# Decision 41: Merge Adjacent Active Data Segments at Build Time

Status: **Accepted, 2026-07-28.**
A core pass collapses runs of near-adjacent active data segments that follow one another.
Each run becomes a single zero-filled segment.
It is always on and identical for every backend.
Landed in `crates/dewasm-core/src/data_merge.rs`.
It runs unconditionally at the end of `build_module_with_options`.
No IR types change, and no backend is touched.

## Context

Toolchains split a program's initialized data across many active segments.
There is one segment per `.data`-like region.
Every backend emits one initializer per active segment, index-keyed off `module.datas`.
The extreme case in the app corpus is `ruby.wasm`, with **7871 active segments**.
Each generates its own call to initialize memory and its own offset/length constants.
Most of those segments sit a few bytes apart.
Joining a run of them into one segment (zero-filling the small holes) cuts the initializer count.
The cut is more than an order of magnitude.
The reduction is on the shared `module.datas`, so it composes with every backend for free.
This is the follow-up decision 37 flagged.

The rewrite is only sound if it preserves the final memory image.
Three wasm rules bear on that:

- wasm initializes active segments in declaration order, and later writes win on overlap.
- A passive segment writes nothing on its own; only `memory.init` does.
- Bulk-memory operations (`memory.init` / `data.drop`) name segments **by index**.
  So renumbering them silently breaks a program.

A merge must respect all three.

## Decision

Add a crate-private pass, `merge_adjacent_data_segments`, over `module.datas`.
It walks the segments in declaration order.
It merges a run of active `i32.const`-offset segments into one segment.
The new segment's offset is the run start.
Its bytes are the segments joined in order, with each hole between segments zero-filled.
Three conditions guard the merge.
Failing **any** stops the whole pass, leaving `module.datas` identical to what was built.

1. **No segment-by-index reference.**
   Stop if any function body contains `Stmt::MemoryInit` or `Stmt::DataDrop`.
   The walk is recursive through `Block`/`Loop`/`If`.
   Merging drops and renumbers indices.
   A bulk operation would then address the wrong (or a missing) segment.
   Active segments are valid `memory.init` sources too, so this is all-or-nothing, not per-segment.
2. **Never reorder across a barrier.**
   Only segments that already follow one another in declaration order merge.
   A `global.get`-offset active segment writes to a runtime-unknown address.
   So it is a barrier the pass cannot see through: it flushes the current run and passes unchanged.
   That keeps its order against the constant segments.
   A passive segment carries no standalone effect, since guard 1 has ruled out `memory.init`.
   So it passes through *without* closing the run: the actives on either side still merge around it.
3. **Zero-fill soundness.**
   Require the active `i32.const` segments to be *globally* monotonically increasing and non-overlapping.
   That is, each start is ≥ the largest end of all earlier ones; else stop.
   This proves that no other constant-offset segment occupies a hole we zero-fill.
   So filling it cannot erase a byte some other segment wrote.

**Merge threshold.**
An active segment merges into the run when `next.offset >= run_end && next.offset - run_end < 64`.
The arithmetic is u64.
The 64-byte bound is the deciding rule.
The fill bytes are emitted **inline by every backend unconditionally**.
So the break-even weighs the always-on cost of a few zero bytes.
It weighs them against a second initializer's per-segment overhead, a small figure.
wasm2go's similar constant is 4096, but that threshold applies only to data moved out.
It concerns bytes moved to a data file (decision 37), a different trade-off.
Also, this pass lives in the core, which cannot see `GenOptions`.
So the pass cannot know whether `--data-file` is even on.
Tuning to the always-on inline cost is the only choice available here.
64 captures the dense runs without inventing large stretches of zero.
`ruby.wasm`'s segments are packed far tighter than that.

## Rejected alternatives

- **Sort segments by offset, then merge.**
  It would merge more.
  But reordering active segments changes which write wins on overlap.
  It also moves them relative to `global.get` barriers whose targets are unknown.
  Declaration order is the only order whose memory image is guaranteed.
  Sorting trades a proven-correct pass for an unprovable one.
- **A backend-side merge during lowering.**
  Each backend already walks over `module.datas`.
  Merging there would multiply the hard ordering/soundness reasoning by the backend count.
  It would also invite differences between backends.
  Doing it once on the shared IR keeps one audited implementation under the specification harness.
- **Merge regardless of gap size (bridge any hole).**
  A single pair of segments could sit on both sides of a hole of several thousand bytes.
  It would then emit thousands of zero bytes inline in every backend.
  That is strictly worse than two initializers.
  The threshold caps that blow-up.

## Consequences

- The active-segment count drops sharply for split-data modules, with no backend change.
  That cuts the per-segment initializer calls and offset/length constants every backend emits.
  Measured (`module.datas.len()` before vs. after):

  | module | before | after |
  | --- | --- | --- |
  | ruby.wasm | 7871 | 352 |
  | cpython.wasm | 2 | 1 |
  | qjs.wasm | 2 | 1 |

  The largest app, `ruby.wasm`, collapses 7871 → 352 (a 22× reduction).
  The code-dominated `cpython`/`qjs` have only two segments and merge to one.
- Correctness is bound by the specification harness (decision 3).
  The pass is always on.
  So the full testsuite passing for every backend *is* the execution-equivalence proof.
  Targeted IR-shape unit tests check the merge and the zero-fill.
  They are in `crates/dewasm-core/tests/data_merge.rs`.
  They also check the gap threshold, the barrier, and each case where the pass stops.
- Modules that use bulk data operations (`rg.wasm`, `libpcap.wasm`, tree-sitter) hit guard 1.
  They are passed through untouched: the pass never makes them worse.
- Composes with decision 37: `--data-file` moves the *merged* segments to the data file.
  So the data file carries fewer, larger segments, and the source fewer prefix-sum constants.

Related decisions:

- decision 1 (semantics-preserving transforms belong in the core IR);
- decision 3 (the harness binds);
- decision 32 (the related always-on core pass, expression folding);
- decision 37 (moving data segments to a file, whose follow-up this is).
