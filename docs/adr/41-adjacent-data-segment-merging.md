# ADR-41 — Merge Adjacent Active Data Segments at Build Time

Status: **Accepted, 2026-07-28.** A core pass collapses runs of consecutive, near-adjacent active data segments into single zero-filled blobs, always on and identical for every backend. Landed in `crates/dewasm-core/src/data_merge.rs`, run unconditionally at the end of `build_module_with_options`; no IR types change and no backend is touched.

## Context

Toolchains split a program's initialized data across many active segments — one per `.data`-like region — and every backend emits one initializer per active segment, index-keyed off `module.datas`. The extreme case in the app corpus is `ruby.wasm`: **7871 active segments**, each generating its own memory-init call and its own offset/length constants. Most of those segments sit a handful of bytes apart. Concatenating a run of them into one blob (zero-filling the small holes) cuts the initializer count by more than an order of magnitude, and because the reduction is on the shared `module.datas`, it composes with every backend for free — this is the follow-up ADR-37 flagged.

The rewrite is only sound if it preserves the final memory image. wasm initializes active segments in declaration order, later writes winning on overlap; a passive segment writes nothing on its own (only `memory.init` does); and bulk-memory ops (`memory.init` / `data.drop`) name segments **by index**, so renumbering them silently corrupts a program. A merge must respect all three.

## Decision

Add a crate-private pass, `merge_adjacent_data_segments`, over `module.datas`. It walks the segments in declaration order and merges a run of active `i32.const`-offset segments into one blob (offset = run start, bytes = the segments concatenated with each inter-segment hole zero-filled), guarded by three conditions — failing **any** bails the whole pass, leaving `module.datas` byte-for-byte as built:

1. **No segment-by-index reference.** If any function body contains `Stmt::MemoryInit` or `Stmt::DataDrop` (walked recursively through `Block`/`Loop`/`If`), bail. Merging drops and renumbers indices; a bulk op would then address the wrong (or a nonexistent) segment. Active segments are valid `memory.init` sources too, so this is all-or-nothing, not per-segment.
2. **Never reorder across a barrier.** Only segments already consecutive in declaration order merge. A `global.get`-offset active segment writes to a runtime-unknown address, so it is an opaque barrier that flushes the current run and passes through unchanged, keeping its order against the constant segments intact. A passive segment carries no standalone effect (guard 1 has ruled out `memory.init`), so it passes through *without* closing the run — the actives on either side still merge around it.
3. **Zero-fill soundness.** Require the active `i32.const` segments to be *globally* monotonically ascending and non-overlapping (each start ≥ the running max end of all earlier ones); else bail. This proves that no other constant-offset segment occupies a hole we zero-fill, so filling it cannot erase a byte some other segment wrote.

**Merge threshold.** Two consecutive active segments merge when `next.offset >= run_end && next.offset - run_end < 64` (u64 arithmetic). The 64-byte bound is the discriminating rule: the fill bytes are emitted **inline by every backend unconditionally**, so the break-even is the always-on cost of a few zero bytes versus a second initializer's per-segment overhead — a small figure. wasm2go's analogous constant is 4096, but that is an *externalize-only* threshold (bytes moved to a sidecar, ADR-37), a different trade-off; and this pass lives in the core, which cannot see `GenOptions` and so cannot know whether externalization is even on. Tuning to the always-on inline cost is the only choice available here, and 64 captures the dense runs (`ruby.wasm`'s segments are packed far tighter than that) without inventing large stretches of zero.

## Rejected alternatives

- **Sort segments by offset, then merge.** Would merge more, but reordering active segments changes which write wins on overlap and moves them relative to `global.get` barriers whose targets are unknown. Declaration order is the only order whose memory image is guaranteed; sorting trades a proven-correct pass for an unprovable one.
- **A backend-side merge during lowering.** Each backend already iterates `module.datas`; merging there would multiply the delicate ordering/soundness reasoning by the backend count and invite divergence. Doing it once on the shared IR keeps a single audited implementation under one spec-harness gate.
- **Merge regardless of gap size (bridge any hole).** A single pair of segments straddling a multi-kilobyte hole would materialize kilobytes of zero bytes inline in every backend, which is strictly worse than two initializers. The threshold caps that blow-up.

## Consequences

- The active-segment count drops sharply for split-data modules, cutting the per-segment initializer calls and offset/length constants every backend emits, with no backend change. Measured (`module.datas.len()` before vs. after):

  | module | before | after |
  | --- | --- | --- |
  | ruby.wasm | 7871 | 352 |
  | cpython.wasm | 2 | 1 |
  | qjs.wasm | 2 | 1 |

The flagship `ruby.wasm` collapses 7871 → 352 (a 22× reduction); the code-dominated `cpython`/`qjs` have only two segments and merge to one.
- Correctness is bound by the spec harness (ADR-3): the pass is always on, so the full testsuite passing for every backend *is* the execution-equivalence proof. Targeted IR-shape unit tests in `crates/dewasm-core/tests/data_merge.rs` pin the merge, the zero-fill, the gap threshold, the barrier, and each bail.
- Modules that use bulk data ops (`rg.wasm`, `libpcap.wasm`, treesitter) hit guard 1 and are passed through untouched — the pass never regresses them.
- Composes with ADR-37: `--data-file` externalizes the *merged* blobs, so the sidecar carries fewer, larger segments and the source fewer prefix-sum constants.

Cross-refs: ADR-1 (semantics-preserving transforms belong in the core IR), ADR-3 (the harness binds), ADR-32 (the sibling always-on core pass, expression folding), ADR-37 (data-segment externalization, whose follow-up this is).
