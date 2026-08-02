//! Flat dispatch: give a branch an address instead of a lexical position.
//!
//! The ADR-42 cascade addresses a branch target by *scope* — `break` leaves the
//! innermost enclosing scope, so naming a target further out means relaying
//! through every frame in between. On `sqlite3-shell` that is 56 epilogue checks
//! per branch in the VDBE loop, and about half of all CPU.
//!
//! w2c2 emits `goto label_N` and WasmKit emits `pc += offset`; both name the
//! target as a *value*, so a branch costs the same at any depth. Ruby's
//! equivalent primitive is `opt_case_dispatch`: a `case` over integer literals
//! compiles to one hash probe with no compares. So a function with cross-frame
//! branches becomes
//!
//! ```text
//! state = 0
//! while true
//!   case state
//!   when 0 then …; state = 7; next     # a br, O(1) at any depth
//!   when 7 then …
//!   end
//! end
//! ```
//!
//! **Only crossed frames are dissolved.** A `begin … end while false` and a
//! `while true` are both Ruby loops, so a `next` inside one binds to *it*, not
//! to the dispatch loop — any frame a branch escapes must therefore stop being a
//! Ruby loop. Frames no branch escapes are left exactly as they were, and `if`
//! is never a loop so it never needs splitting.
//!
//! Keeping uncrossed loops structured is not just economy, it is required for
//! performance: measured against a tight inner loop, turning its back-edge into
//! a state transition costs more than the plain `next` it replaces and loses to
//! the cascade outright once the loop runs ~100 trips per entry. Flatten
//! branches, not loops.

use std::collections::{HashMap, HashSet};

use dewasm_core::ir::Stmt;

/// State numbering for one function's flattened control flow.
pub struct Plan {
    /// Frames that stop being Ruby scopes, so a `next` from inside them reaches
    /// the dispatch loop.
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

/// Decide which frames to dissolve. `crossed` is [`crate::FrameSets::crossed`] —
/// exactly the frames some branch escapes, which is exactly the set that must
/// stop being Ruby loops.
///
/// Returns `None` when nothing is crossed, so the function keeps today's
/// lowering untouched and pays nothing for the machinery.
pub fn plan(body: &[Stmt], crossed: &HashSet<u32>) -> Option<Plan> {
    // Escape hatch for A/B measurement: falls back to the ADR-42 cascade so the
    // two lowerings can be compared from one build.
    if crossed.is_empty() || std::env::var_os("DEWASM_FLAT_DISABLE").is_some() {
        return None;
    }
    // Dissolution is transitive up the spine. A frame that still exists as a
    // Ruby loop would capture a `next` aimed at the dispatch loop, so anything
    // *containing* a dissolved frame has to go too. What survives is the leaves:
    // loops and blocks with no escaping branch anywhere inside them — which is
    // precisely where keeping the structured form was measured to matter.
    let mut dissolved = crossed.clone();
    loop {
        let before = dissolved.len();
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
                // `if` is not a Ruby loop, so a `next` inside it already reaches
                // the dispatch loop; it only needs a landing state when it is
                // itself a branch target that something escapes.
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
