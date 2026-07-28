//! Adjacent active-data-segment merging (ADR-41).
//!
//! Toolchains such as LLVM can split a program's initialized data across
//! thousands of tiny active segments (ruby.wasm ships 7871). Every backend
//! emits one initializer per active segment, so collapsing runs of
//! consecutive, near-adjacent segments into a single zero-filled blob is a
//! semantics-preserving win that composes with every downstream backend for
//! free. This pass runs unconditionally at the end of module building.
//!
//! Three guards make the rewrite sound; failing any of them bails the whole
//! pass (leaving `module.datas` untouched):
//!
//! 1. **No bulk-memory segment references.** `memory.init` / `data.drop`
//!    address segments *by index*; merging renumbers and drops indices, so a
//!    single such op anywhere makes the pass unsafe.
//! 2. **Declaration order is never reordered among barriers.** Only runs of
//!    segments already consecutive in `module.datas` are merged; active
//!    segments initialize in order and later writes win, so preserving order
//!    preserves the final memory image.
//! 3. **Active `i32.const` segments are globally ascending and
//!    non-overlapping.** This proves no other constant-offset segment occupies
//!    a gap between two merged segments, so zero-filling that gap cannot erase
//!    a byte some other segment wrote.
//!
//! Passive segments (`offset: None`) carry no standalone memory effect once
//! guard 1 has ruled out `memory.init`, so they pass straight through without
//! closing an open run. A `global.get`-offset active segment writes to a
//! runtime-unknown address and so acts as an opaque barrier: it flushes the
//! current run, keeping its relative order against the constant-offset
//! segments intact.

use crate::ir::{DataSegment, Expr, Module, Stmt};

/// Largest byte gap between two consecutive active segments that is still worth
/// bridging with zero fill. The bytes are emitted inline by every backend
/// unconditionally (the `--data-file` externalization threshold is a separate,
/// backend-visible concern; this pass runs in the core, which cannot see
/// `GenOptions`), so the figure is tuned to the always-on inline cost rather
/// than to wasm2go's externalize-only 4096.
const MAX_MERGE_GAP: u64 = 64;

/// Merge runs of adjacent active data segments in `module`, in place.
pub(crate) fn merge_adjacent_data_segments(module: &mut Module) {
    // Guard 1: any body that references a segment by index (bulk memory) makes
    // renumbering unsafe.
    if module.funcs.iter().any(|f| body_refs_segment(&f.body)) {
        return;
    }

    // Guard 3: the active i32.const segments must be globally ascending and
    // non-overlapping.
    if !active_const_segments_ascending(&module.datas) {
        return;
    }

    let old = std::mem::take(&mut module.datas);
    let mut out: Vec<DataSegment> = Vec::with_capacity(old.len());
    // The open run being accumulated as (start offset, concatenated bytes).
    let mut run: Option<(u64, Vec<u8>)> = None;

    for DataSegment { offset, data } in old {
        match offset {
            Some(Expr::I32Const(off)) => {
                let start = u64::from(off);
                match run.take() {
                    None => run = Some((start, data)),
                    Some((run_start, mut acc)) => {
                        let run_end = run_start + acc.len() as u64;
                        // Guard 3 guarantees `start >= run_end`; the gap bound
                        // is the only real merge condition.
                        if start >= run_end && start - run_end < MAX_MERGE_GAP {
                            acc.resize(acc.len() + (start - run_end) as usize, 0);
                            acc.extend_from_slice(&data);
                            run = Some((run_start, acc));
                        } else {
                            out.push(active_segment(run_start, acc));
                            run = Some((start, data));
                        }
                    }
                }
            }
            // Passive: no standalone memory effect, keep the run open.
            None => out.push(DataSegment { offset: None, data }),
            // global.get (or any non-constant offset): opaque barrier.
            Some(other) => {
                if let Some((run_start, acc)) = run.take() {
                    out.push(active_segment(run_start, acc));
                }
                out.push(DataSegment {
                    offset: Some(other),
                    data,
                });
            }
        }
    }
    if let Some((run_start, acc)) = run.take() {
        out.push(active_segment(run_start, acc));
    }

    module.datas = out;
}

/// A constant-offset active segment. `start` originated as a `u32` offset.
fn active_segment(start: u64, data: Vec<u8>) -> DataSegment {
    DataSegment {
        offset: Some(Expr::I32Const(start as u32)),
        data,
    }
}

/// Whether any statement (recursing into nested control flow) references a data
/// segment by index via `memory.init` or `data.drop`.
fn body_refs_segment(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|s| match s {
        Stmt::MemoryInit { .. } | Stmt::DataDrop { .. } => true,
        Stmt::Block { body, .. } | Stmt::Loop { body, .. } => body_refs_segment(body),
        Stmt::If { then, els, .. } => body_refs_segment(then) || body_refs_segment(els),
        _ => false,
    })
}

/// Whether every active `i32.const` segment starts at or after the running
/// maximum end of all earlier such segments (globally ascending, non-overlapping
/// in declaration order). `global.get`-offset and passive segments are ignored.
fn active_const_segments_ascending(datas: &[DataSegment]) -> bool {
    let mut max_end: u64 = 0;
    for seg in datas {
        if let Some(Expr::I32Const(off)) = &seg.offset {
            let start = u64::from(*off);
            if start < max_end {
                return false;
            }
            max_end = start + seg.data.len() as u64;
        }
    }
    true
}
