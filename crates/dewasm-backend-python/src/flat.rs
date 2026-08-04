//! Flat dispatch: give a branch an address instead of a lexical position.
//!
//! The ADR-28 branch register addresses a branch target by *label id*, but the
//! branch itself only sets `_br` — reaching the target is the job of the frames
//! in between, each of which tests the register once (a run guard `if _br == 0:`
//! it must fall out of, then the landing marker of the frame that owns it). So a
//! branch crossing N frames costs O(N) tests, the same shape as Ruby's ADR-42
//! cascade this is ported from.
//!
//! w2c2 emits `goto label_N` and WasmKit emits `pc += offset`; both name the
//! target as a *value*, so a branch costs the same at any depth. Python's
//! equivalent is a dispatch loop over an integer state:
//!
//! ```text
//! _state = 0
//! while True:
//!     if _state == 0:
//!         ...
//!         _state = 7; continue   # a br, one jump at any depth
//!     elif _state == 7:
//!         ...
//!     else:
//!         break
//! ```
//!
//! **Only crossed frames are dissolved.** A `continue` binds to the innermost
//! Python loop, so any frame a branch escapes that is a real `while True:` must
//! stop being one; blocks are already spliced inline (ADR-28) but dissolve with
//! the rest of the path, because the closure below is what guarantees the
//! `continue` reaches the dispatch. Frames no branch escapes are left exactly as
//! they were, and `if` is never a loop so it never needs splitting.
//!
//! Keeping uncrossed loops structured is not just economy: turning a tight
//! back-edge into a state transition replaces one `continue` with an assignment,
//! a jump and a dispatch probe. Flatten branches, not loops.
//!
//! **Only *deep* branches pay for a dispatch** — see [`DEEP_CROSSING`];
//! everything else keeps the ADR-28 register, in the same function, side by side
//! (the Ruby original is ADR-58/ADR-60).

use std::collections::{HashMap, HashSet};

use dewasm_core::ir::Stmt;

/// State numbering for one function's flattened control flow.
pub struct Plan {
    /// Frames that stop being emitted as frames, so a `continue` from inside
    /// them reaches the dispatch loop.
    pub dissolved: HashSet<u32>,
    /// Where a branch to each dissolved label lands. For a block or `if` that is
    /// the state after it; for a loop it is the state at its head.
    pub state_of: HashMap<u32, u32>,
    /// The state control continues in once a dissolved frame is finished.
    pub after: HashMap<u32, u32>,
    pub nstates: u32,
}

impl Plan {
    fn new_state(&mut self) -> u32 {
        let s = self.nstates;
        self.nstates += 1;
        s
    }
}

/// Crossing depth from which a branch is worth a dispatch.
///
/// Ruby's calibrated constant (ADR-60), kept after measuring it against the
/// binary-tree dispatch this backend emits (`emit_dispatch_tree`): a transition
/// costs O(log2 states) compares (~11 for the largest machine here, 1463
/// states), against ~16+ region checks for the relay it replaces. Measured on
/// converted apps (CPython 3.14, 2 runs each, ±0.2 s): the sqlite3 shell's
/// query-heavy workload runs 1.42x faster than the relay-only lowering
/// (23.4 s → 16.5 s) and the packed CRuby boot is at parity (17.0 s → 16.6 s).
/// A *linear* dispatch at this threshold measured 1.22x *slower* than the
/// relay on the same boot (largest machine ~700 compares per transition) —
/// which is why the tree shape, not the Ruby `case`'s O(1) probe or an
/// `elif`/`match` chain, carries this constant's calibration.
pub const DEEP_CROSSING: usize = 16;

/// Decide which frames to dissolve. `paths` is [`crate::compute_frame_paths`] —
/// one entry per outward branch, the inclusive frame path from its target down
/// to its own innermost frame, which is exactly the set of frames that must stop
/// existing if that branch is to become a state transition.
///
/// Returns `None` when no branch is deep enough, so the function keeps the
/// ADR-28 branch register untouched and pays nothing for the machinery.
pub fn plan(body: &[Stmt], paths: &[Vec<u32>]) -> Option<Plan> {
    let mut dissolved: HashSet<u32> = paths
        .iter()
        .filter(|p| p.len() >= DEEP_CROSSING)
        .flatten()
        .copied()
        .collect();
    if dissolved.is_empty() {
        return None;
    }
    // Two closures, to a joint fixpoint.
    //
    // *Paths.* A `_state = N; continue` must not be captured on its way to the
    // dispatch loop, so once any frame a branch crosses is dissolved, every
    // frame it crosses has to go — the branch can no longer relay through `_br`.
    //
    // *Ancestors.* Dissolution is transitive up the spine for the same reason:
    // a frame that still exists would have to run its landing marker after a
    // body that no longer falls out of it. What survives is the leaves: loops
    // and blocks with no escaping branch anywhere inside them — which is
    // precisely where keeping the structured form matters.
    loop {
        let before = dissolved.len();
        for path in paths {
            if path.iter().any(|f| dissolved.contains(f)) {
                dissolved.extend(path.iter().copied());
            }
        }
        mark_ancestors(body, &mut dissolved);
        if dissolved.len() == before {
            break;
        }
    }
    let mut plan = Plan {
        dissolved,
        state_of: HashMap::new(),
        after: HashMap::new(),
        nstates: 1, // state 0 is the entry
    };
    assign(body, &mut plan);
    Some(plan)
}

/// Add any frame that contains a dissolved frame. Returns whether `stmts`
/// contains one.
fn mark_ancestors(stmts: &[Stmt], dissolved: &mut HashSet<u32>) -> bool {
    let mut any = false;
    for stmt in stmts {
        let (label, inner) = match stmt {
            Stmt::Block { label, body } | Stmt::Loop { label, body } => {
                (label.id, mark_ancestors(body, dissolved))
            }
            Stmt::If {
                label, then, els, ..
            } => {
                let a = mark_ancestors(then, dissolved);
                let b = mark_ancestors(els, dissolved);
                (label.id, a || b)
            }
            _ => continue,
        };
        if inner {
            dissolved.insert(label);
        }
        if inner || dissolved.contains(&label) {
            any = true;
        }
    }
    any
}

/// Walk the body in the same order the emitter will, allocating each dissolved
/// frame's landing state. Emission re-walks identically, so the numbering agrees
/// without threading a counter through both.
fn assign(stmts: &[Stmt], plan: &mut Plan) {
    for stmt in stmts {
        match stmt {
            Stmt::Block { label, body } => {
                if plan.dissolved.contains(&label.id) {
                    assign(body, plan);
                    let after = plan.new_state();
                    plan.state_of.insert(label.id, after);
                    plan.after.insert(label.id, after);
                } else {
                    assign(body, plan);
                }
            }
            Stmt::Loop { label, body } => {
                if plan.dissolved.contains(&label.id) {
                    // The head is the branch target; the body follows it.
                    let head = plan.new_state();
                    plan.state_of.insert(label.id, head);
                    assign(body, plan);
                    let after = plan.new_state();
                    plan.after.insert(label.id, after);
                } else {
                    assign(body, plan);
                }
            }
            Stmt::If {
                label, then, els, ..
            } => {
                // `if` is not a Python loop, so a `continue` inside it already
                // reaches the dispatch loop; it only needs a landing state when
                // it is itself a branch target that something escapes.
                assign(then, plan);
                assign(els, plan);
                if plan.dissolved.contains(&label.id) {
                    let after = plan.new_state();
                    plan.state_of.insert(label.id, after);
                    plan.after.insert(label.id, after);
                }
            }
            _ => {}
        }
    }
}
