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

use dewasm_core::ir::{BrTarget, Stmt};

/// Whether a native `break` out of a loop lands exactly where a branch to the block enclosing that loop lands.
/// The one place a backend's own scope semantics enter [`frames`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BreakToBlockEnd {
    /// Ruby: leaving the loop continues after the enclosing `begin ... end while false`, which is where such a branch was going, so the shape costs nothing and neither frame has to dissolve.
    Available,
    /// Python: a block exit is driven by the branch register instead, so a `break` carries no branch and every outward branch crosses its frames like any other.
    Unavailable,
}

/// How a function body's capturing frames relate to the branches inside it.
/// `paths` is [`plan`]'s input; the other three classify the frames a structured lowering keeps.
#[derive(Default)]
pub struct Frames {
    /// Capturing frames (`Block`, `Loop`, `TryTable`, or referenced-`If`) that a `br` from strictly inside crosses, and so must carry a land-or-relay epilogue.
    pub crossed: HashSet<u32>,
    /// Loops that a `br` targets from a *strictly nested* capturing frame, so the branch reaches the loop head by leaving an inner scope rather than by a direct back-edge.
    pub wrapped: HashSet<u32>,
    /// Loops that are the sole statement in an enclosing *block*'s direct body, so a `break` out of the loop lands exactly where a `br` to that block lands.
    /// A branch of that shape needs neither a relay nor a state transition, so neither frame has to dissolve.
    /// Empty unless the backend declares [`BreakToBlockEnd::Available`].
    pub break_ok: HashSet<u32>,
    /// One entry per outward `br`: the inclusive frame path it crosses, target first, its own innermost frame last.
    /// `crossed` is the union of these; the paths themselves are kept because [`plan`] weighs the individual branch, and because a branch is all-or-nothing: dissolving any frame it crosses forces the rest.
    pub paths: Vec<Vec<u32>>,
}

/// Classify `body`'s frames and the branches crossing them.
///
/// Walking with a stack of the open capturing frames (label id + is-loop, innermost last), a `br` to target `T` at stack position `pos` is either a self-branch (`pos` is the top: a plain exit from the innermost frame, crossing nothing) or an outward branch, `pos < top`, which must traverse `stack[pos..]`: the target frame, all pass-through frames, *and* the innermost frame whose own exit otherwise lands mid-body in its parent.
/// Every frame on that inclusive path is `crossed`; and if `T` is a loop reached this way, it is `wrapped`.
/// A `Block`, a `Loop` and a `TryTable` always capture, an `If` only when `referenced`, since an unreferenced one emits no landing marker and is not a frame anything can name; a plain `if`, `br_if`'s wrapper and `br_table`'s dispatch never capture, so they are not on the stack.
/// `br_if`/`br_table` feed every target through the same routine.
///
/// The one outward shape that marks nothing is `break_ok` under [`BreakToBlockEnd::Available`]: a branch crossing a single loop that is the **sole** statement of the block it targets.
pub fn frames(body: &[Stmt], break_to_block_end: BreakToBlockEnd) -> Frames {
    let mut walk = Walk {
        break_to_block_end,
        stack: Vec::new(),
        frames: Frames::default(),
    };
    walk.seq(body, true);
    walk.frames
}

struct Walk {
    break_to_block_end: BreakToBlockEnd,
    /// The open capturing frames, innermost last: (label id, is-loop).
    stack: Vec<(u32, bool)>,
    frames: Frames,
}

impl Walk {
    /// `direct` marks a sequence that is a frame's own body rather than an `if` arm.
    /// `break_ok` is not propagated through `if` arms: a `break` from inside an `if` lands after the `if`, and proving that is still the block's end is more than the rule needs to claim.
    fn seq(&mut self, stmts: &[Stmt], direct: bool) {
        // "Sole statement", not merely "last": if the block held anything besides the loop, that other statement could dissolve, which dissolves the block by the ancestor rule while the loop itself stays a native loop, and then a transition aimed at the dispatch loop is captured by that loop.
        // Requiring the loop to be the whole body makes the block dissolvable only through the loop, so the two always dissolve together.
        let sole = self.break_to_block_end == BreakToBlockEnd::Available
            && direct
            && stmts
                .iter()
                .filter(|s| !matches!(s, Stmt::SourceLine(_)))
                .count()
                == 1;
        for stmt in stmts {
            match stmt {
                Stmt::Block { label, body } => {
                    self.stack.push((label.id, false));
                    self.seq(body, true);
                    self.stack.pop();
                }
                Stmt::Loop { label, body } => {
                    if sole && matches!(self.stack.last(), Some((_, is_loop)) if !is_loop) {
                        self.frames.break_ok.insert(label.id);
                    }
                    self.stack.push((label.id, true));
                    self.seq(body, true);
                    self.stack.pop();
                }
                Stmt::If {
                    label, then, els, ..
                } => {
                    if label.referenced {
                        self.stack.push((label.id, false));
                    }
                    self.seq(then, false);
                    self.seq(els, false);
                    if label.referenced {
                        self.stack.pop();
                    }
                }
                Stmt::TryTable {
                    label,
                    catches,
                    body,
                } => {
                    self.stack.push((label.id, false));
                    self.seq(body, true);
                    // A catch clause's branch is emitted in the handler, inside the try_table's own scope, so it crosses that frame like a branch from the body would.
                    // [`plan`] never dissolves a function holding a try_table, so this is inert for the flat lowering; it is kept so the walk stays a faithful description of every frame a branch can cross, which the structured lowering does consult.
                    for clause in catches {
                        self.target(&clause.target);
                    }
                    self.stack.pop();
                }
                Stmt::Br(target) | Stmt::BrIf { target, .. } => self.target(target),
                Stmt::BrTable {
                    targets, default, ..
                } => {
                    for target in targets {
                        self.target(target);
                    }
                    self.target(default);
                }
                // Every other statement carries no frame and no branch, so `Stmt::child_seqs` yields nothing for it today.
                // A future variant that does carry a body is still walked, in the enclosing frame's context and with `direct` off, which can only over-report crossings: it never claims a branch stays structured when it does not.
                other => {
                    for seq in other.child_seqs() {
                        self.seq(seq, false);
                    }
                }
            }
        }
    }

    fn target(&mut self, target: &BrTarget) {
        let BrTarget::Label { label, .. } = target else {
            return;
        };
        let Some(pos) = self.stack.iter().position(|(id, _)| id == label) else {
            return;
        };
        if pos + 1 == self.stack.len() {
            return;
        }
        // Unless the branch is the `break_ok` shape: crossing exactly one loop that ends the block being targeted.
        // That already lands where the branch wants to go, at O(1), so nothing on the path dissolves.
        if pos + 2 == self.stack.len() && self.frames.break_ok.contains(&self.stack[pos + 1].0) {
            return;
        }
        let path: Vec<u32> = self.stack[pos..].iter().map(|(id, _)| *id).collect();
        self.frames.crossed.extend(path.iter().copied());
        self.frames.paths.push(path);
        if self.stack[pos].1 {
            self.frames.wrapped.insert(*label);
        }
    }
}

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
    // `mark_ancestors` and `assign` below never look inside a `try_table`, which is sound only because this bail comes first.
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

fn contains_try_table(stmts: &[Stmt]) -> bool {
    Stmt::any(stmts, &mut |stmt| matches!(stmt, Stmt::TryTable { .. }))
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
