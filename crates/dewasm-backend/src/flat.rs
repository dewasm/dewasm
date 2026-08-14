//! Flat dispatch: give a branch an address instead of a lexical position.
//!
//! A structured lowering addresses a branch target by *position*: Ruby's cascade by scope (`break` leaves the innermost one), Python's register by label id (the branch only sets `_br`).
//! Reaching the target is the job of every frame in between, each of which tests once.
//! A branch crossing N frames costs O(N) tests: on `sqlite3-shell` that is 56 epilogue checks per branch in the VDBE loop, and about half of all CPU.
//!
//! w2c2 emits `goto label_N` and WasmKit emits `pc += offset`; both name the target as a *value*, so a branch costs the same at any depth.
//! A dispatch loop over an integer state is the equivalent primitive here (Ruby's `case` over integer literals compiles to one `opt_case_dispatch` hash probe, Python's takes the binary-search tree its emitter builds):
//!
//! ```text state = 0 while true
//! case state
//! when 0 then …; state = 7; next     # a br, O(1) at any depth
//! when 7 then …
//! end end
//! ```
//!
//! **Only crossed frames are dissolved.** A `next`/`continue` binds to the innermost enclosing native loop, so any frame a branch escapes that is one must stop being one: in Ruby both `begin … end while false` and `while true` count, in Python blocks are already spliced inline but dissolve with the rest of the path anyway, because the path closure below is what guarantees the jump reaches the dispatch.
//! Frames no branch escapes are left exactly as they were, and `if` is never a loop so it never needs splitting.
//!
//! Keeping uncrossed loops structured is not just economy, it is required for performance: a back-edge turned into a state transition replaces one `next`/`continue` with an assignment, a jump and a dispatch probe, and measured against a tight Ruby inner loop it loses to the cascade outright once the loop runs ~100 trips per entry.
//! Flatten branches, not loops.
//!
//! **Only *deep* branches pay for a dispatch.** The relay this replaces is linear in the frames a branch crosses and each level is cheap; the dispatch is a constant that is not.
//! So a function is flattened only where some branch crosses at least [`plan`]'s `deep_crossing` frames (a threshold each backend calibrates for itself), and everything else keeps the structured lowering, in the same function, side by side.

use std::collections::{HashMap, HashSet};

use dewasm_core::ir::Stmt;

/// State numbering for one function's flattened control flow.
pub struct Plan {
    /// Frames that stop being emitted as native scopes, so a `next`/`continue` from inside them reaches the dispatch loop.
    pub dissolved: HashSet<u32>,
    /// Where a branch to each dissolved label lands.
    /// For a block or `if` that is the state after it; for a loop it is the state at its head.
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

/// Decide which frames to dissolve.
/// `paths` holds one entry per outward branch: the inclusive frame path from its target down to its own innermost frame, which is exactly the set of frames that must stop existing if that branch is to become a state transition.
/// `deep_crossing` is the crossing depth from which a branch is worth a dispatch: the backend's own calibration, since it weighs that backend's relay against that backend's dispatch shape.
///
/// Returns `None` when no branch is deep enough, so the function keeps its structured lowering untouched and pays nothing for the machinery.
/// Also `None` for a function containing a `try_table`: its body must stay lexically inside the handler that guards it, so it cannot be split across states.
pub fn plan(body: &[Stmt], paths: &[Vec<u32>], deep_crossing: usize) -> Option<Plan> {
    if contains_try_table(body) {
        return None;
    }
    let mut dissolved: HashSet<u32> = paths
        .iter()
        .filter(|p| p.len() >= deep_crossing)
        .flatten()
        .copied()
        .collect();
    if dissolved.is_empty() {
        return None;
    }
    // Two closures, to a joint fixpoint.
    //
    // *Paths.* A `state = N; next` must not be captured on its way to the dispatch loop, so once any frame a branch crosses is dissolved, every frame it crosses has to go: the branch can no longer be a relay.
    //
    // *Ancestors.* Dissolution is transitive up the spine for the same reason: a frame that still exists would capture a jump aimed at the dispatch loop, and would have to run its landing marker after a body that no longer falls out of it.
    // What survives is the leaves: loops and blocks with no escaping branch anywhere inside them, which is precisely where keeping the structured form was measured to matter.
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

/// Exhaustive on purpose: a future body-carrying `Stmt` variant must show up here as a compile error rather than silently stop the recursion, which would flatten a function whose `try_table` then captures a transition aimed at the dispatch loop.
fn contains_try_table(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::TryTable { .. } => true,
        Stmt::Block { body, .. } | Stmt::Loop { body, .. } => contains_try_table(body),
        Stmt::If { then, els, .. } => contains_try_table(then) || contains_try_table(els),
        Stmt::SourceLine(_)
        | Stmt::Assign { .. }
        | Stmt::LocalSet { .. }
        | Stmt::GlobalSet { .. }
        | Stmt::Store { .. }
        | Stmt::Br(_)
        | Stmt::BrIf { .. }
        | Stmt::BrTable { .. }
        | Stmt::Return { .. }
        | Stmt::Call { .. }
        | Stmt::CallIndirect { .. }
        | Stmt::MemoryGrow { .. }
        | Stmt::MemoryCopy { .. }
        | Stmt::MemoryFill { .. }
        | Stmt::MemoryInit { .. }
        | Stmt::DataDrop { .. }
        | Stmt::TableInit { .. }
        | Stmt::TableCopy { .. }
        | Stmt::ElemDrop { .. }
        | Stmt::Throw { .. }
        | Stmt::ThrowRef { .. }
        | Stmt::Unreachable => false,
    })
}

/// Add any frame that contains a dissolved frame.
/// Returns whether `stmts` contains one.
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

/// Walk the body in the same order the emitter will, allocating each dissolved frame's landing state.
/// Emission re-walks identically, so the numbering agrees without threading a counter through both.
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
                // `if` is not a loop, so a jump inside it already reaches the dispatch loop; it only needs a landing state when it is itself a branch target that something escapes.
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
