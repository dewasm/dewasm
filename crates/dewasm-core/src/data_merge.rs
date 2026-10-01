//! Adjacent active-data-segment merging.
//!
//! Toolchains split initialized data across thousands of tiny active segments.
//! For example, ruby.wasm ships 7871.
//! Merging runs of near-adjacent ones into one zero-filled blob reduces each backend's output size.
//! Runs at the end of module building.
//!
//! Four properties keep the rewrite sound.
//! Properties 1, 3 and 4 are checked up front.
//! Failing any makes the whole pass return early, merging nothing.
//! Property 2 holds by construction, since the pass rebuilds the list in declaration order.
//!
//! 1. **No `memory.init`/`data.drop`**: they address segments by index.
//! 2. **Declaration order is preserved**: segments apply in order, later writes win.
//! 3. **Active `i32.const` segments are ascending and non-overlapping**.
//!    So no constant-offset segment sits inside a zero-filled gap.
//! 4. **No `global.get` offsets**: such a segment's target is unknown until runtime.
//!    It could sit inside a gap and be clobbered by the blob's zeros (issue #28).
//!
//! Passive segments carry no memory effect once 1 holds and pass through without closing a run.

use crate::ir::{DataSegment, Expr, Module, Stmt};

/// Largest gap worth bridging with zero fill.
/// Tuned to the always-on inline cost, since the core cannot see `GenOptions`.
/// It is not wasm2go's 4096, which applies only when data is in a separate file.
const MAX_MERGE_GAP: u64 = 64;

pub(crate) fn merge_adjacent_data_segments(module: &mut Module) {
    // Property 1: a body that references a segment by index (bulk memory) makes renumbering unsafe.
    if module.funcs.iter().any(|f| body_refs_segment(&f.body)) {
        return;
    }

    // Property 3: the active i32.const segments must be globally ascending and non-overlapping.
    if !active_const_segments_ascending(&module.datas) {
        return;
    }

    // Property 4: no non-constant (global.get) offsets.
    if module
        .datas
        .iter()
        .any(|seg| !matches!(seg.offset, None | Some(Expr::I32Const(_))))
    {
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
                        // Property 3 guarantees `start >= run_end`.
                        // The gap bound is the only real merge condition.
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
            // Unreachable under property 4; kept as an order-preserving pass-through.
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

/// A constant-offset active segment.
/// `start` came from a `u32` offset.
fn active_segment(start: u64, data: Vec<u8>) -> DataSegment {
    DataSegment {
        offset: Some(Expr::I32Const(start as u32)),
        data,
    }
}

/// Whether any statement references a data segment by index via `memory.init` or `data.drop`.
/// The search recurses into nested control flow.
fn body_refs_segment(stmts: &[Stmt]) -> bool {
    Stmt::any(stmts, &mut |s| {
        matches!(s, Stmt::MemoryInit { .. } | Stmt::DataDrop { .. })
    })
}

/// Whether every active `i32.const` segment starts at or after the end of all earlier ones.
/// That is, the segments are globally ascending and non-overlapping in declaration order.
/// `global.get`-offset and passive segments are ignored.
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
