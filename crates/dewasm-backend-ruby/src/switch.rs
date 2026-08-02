//! Switch recovery: rebuild a C `switch` from the block tower a compiler
//! lowers it into, so it comes back as a Ruby `case`/`when` instead of a
//! ladder of nested scopes.
//!
//! Clang lowers `switch (x) { case 0: …; case 1: …; }` to a tower of nested
//! `block`s whose innermost content is only a `br_table`, with each case body
//! sitting just past its own block's `end`:
//!
//! ```wat
//! (block $join
//!   (block $case1
//!     (block $case0
//!       (br_table $case0 $case1 $join (local.get $x)))
//!     …case 0 body…  (br $out))
//!   …case 1 body…    (br $out))
//! ```
//!
//! Lowered structurally, a `br` out of case *k* relays through *k* frames of
//! the ADR-42 cascade. That is where the generated Ruby's branch cost is
//! concentrated: on `sqlite3-shell` the deepest tower is **278 blocks with 349
//! targets**, 92.9% of every relay check executed lands in the one function
//! that carries it, and the module holds 2337 tower frames in total.
//!
//! Recognising the idiom removes the tower rather than making it cheaper to
//! walk. The arms become `when` branches, so there is no relay at all, no
//! method call, and no locals crossing a boundary — and Ruby compiles a `case`
//! over integer literals to `opt_case_dispatch`, a hash jump, in place of up to
//! 278 sequential compares.

use std::collections::BTreeMap;

use dewasm_core::ir::{BrTarget, Expr, Stmt};

/// What one `br_table` target selects.
pub enum Arm<'a> {
    /// A case body: the arm's own statements, followed by each subsequent arm
    /// it falls through into.
    ///
    /// C's `switch` falls through, Ruby's `case` does not, and in this idiom
    /// fall-through is just textual adjacency — an arm that does not end in a
    /// branch runs on into the next one, and an *empty* arm is the purest form
    /// of it (`case 1: case 2: case 3: …` where 1 and 2 carry no code of their
    /// own). Rather than reject those towers, the chain is flattened here and
    /// re-emitted per arm, so each `when` is self-contained.
    Body(Vec<&'a [Stmt]>),
    /// The tower's own outermost label: control leaves the switch, which in
    /// Ruby is simply falling out of the `case`.
    Leave,
    /// A label outside the tower — the arm is that branch. Still a win: the
    /// branch is now emitted at the tower root's depth rather than relayed up
    /// from the bottom of the ladder.
    Branch(&'a BrTarget),
}

/// A recovered switch, ready to emit as `case … when …`.
pub struct Switch<'a> {
    /// Statements the innermost block runs before dispatching — typically the
    /// `local.tee` that computes the index. Emitted ahead of the `case`.
    pub prelude: &'a [Stmt],
    /// The value dispatched on.
    pub index: &'a Expr,
    /// `(match values, arm)` in `br_table` target order. Repeated targets
    /// collapse into one arm with several values (`when 5, 6, 7`).
    pub arms: Vec<(Vec<u32>, Arm<'a>)>,
    /// What the `br_table` default selects.
    pub default: Arm<'a>,
}

/// Ignore inert position markers when matching structure (ADR-38).
fn significant(stmts: &[Stmt]) -> Vec<&Stmt> {
    stmts
        .iter()
        .filter(|s| !matches!(s, Stmt::SourceLine(_)))
        .collect()
}

/// Whether `stmts` cannot fall off its end — the precondition that lets an arm
/// become a `when` branch, since Ruby's `case` has no fall-through.
fn terminated(stmts: &[Stmt]) -> bool {
    match significant(stmts).last() {
        Some(Stmt::Br(_)) | Some(Stmt::Return { .. }) | Some(Stmt::Unreachable) => true,
        // A tower nested in the arm terminates iff every one of its own arms does.
        Some(Stmt::Block { body, .. }) => terminated(body),
        _ => false,
    }
}

/// Count `br` references to each label anywhere in `stmts`, so a tower whose
/// labels are targeted by something other than its own `br_table` can be
/// rejected — there the label carries meaning the `case` would lose.
pub fn count_label_refs(stmts: &[Stmt], out: &mut BTreeMap<u32, usize>) {
    fn note(t: &BrTarget, out: &mut BTreeMap<u32, usize>) {
        if let BrTarget::Label { label, .. } = t {
            *out.entry(*label).or_default() += 1;
        }
    }
    for stmt in stmts {
        match stmt {
            Stmt::Br(t) | Stmt::BrIf { target: t, .. } => note(t, out),
            Stmt::BrTable {
                targets, default, ..
            } => {
                for t in targets.iter().chain([default]) {
                    note(t, out);
                }
            }
            Stmt::Block { body, .. } | Stmt::Loop { body, .. } => count_label_refs(body, out),
            Stmt::If { then, els, .. } => {
                count_label_refs(then, out);
                count_label_refs(els, out);
            }
            _ => {}
        }
    }
}

/// Why a tower was not recovered. Recorded so the pass can be measured against
/// a real module rather than assumed to fire.
#[derive(Debug, PartialEq, Eq)]
pub enum Reject {
    /// Not the idiom at all (no nested-block chain ending in a `br_table`).
    NotATower,
    /// Fewer levels than it is worth rebuilding.
    TooShallow(usize),
    /// A tower label is branched to by something other than its own `br_table`.
    ForeignBranch,
}

/// Smallest tower worth rebuilding. Below this the cascade is already short and
/// the structural rewrite buys nothing.
const MIN_DEPTH: usize = 3;

/// Try to read `Block { label, body }` as a switch tower.
///
/// `refs` must be the whole enclosing function's label-reference counts.
pub fn recognize<'a>(
    label: u32,
    body: &'a [Stmt],
    refs: &BTreeMap<u32, usize>,
) -> Result<Switch<'a>, Reject> {
    // Walk in: each level is [nested Block, …arm…] until one is just a br_table.
    let mut labels = vec![label];
    let mut rests: Vec<&'a [Stmt]> = Vec::new();
    let mut cur = body;
    let prelude: &'a [Stmt];
    let (index, targets, default) = loop {
        let sig = significant(cur);
        // A tower level starts with its nested block; check that first, since an
        // arm may itself end in an unrelated `br_table`.
        if !matches!(sig.first(), Some(Stmt::Block { .. })) {
            // Innermost: the dispatch, optionally preceded by the straight-line
            // statements that compute its index (clang emits `local.tee` here,
            // which lowers to a set plus a read).
            let is_simple = |s: &Stmt| {
                !matches!(
                    s,
                    Stmt::Block { .. }
                        | Stmt::Loop { .. }
                        | Stmt::If { .. }
                        | Stmt::Br(_)
                        | Stmt::BrIf { .. }
                        | Stmt::BrTable { .. }
                        | Stmt::Return { .. }
                )
            };
            if let Some(Stmt::BrTable {
                index,
                targets,
                default,
            }) = sig.last()
            {
                if sig.iter().rev().skip(1).all(|s| is_simple(s)) {
                    let at = cur
                        .iter()
                        .position(|s| matches!(s, Stmt::BrTable { .. }))
                        .unwrap_or(cur.len());
                    prelude = &cur[..at];
                    break (index, targets, default);
                }
            }
            return Err(Reject::NotATower);
        }
        match sig.first() {
            Some(Stmt::Block {
                label: inner,
                body: inner_body,
            }) => {
                labels.push(inner.id);
                // Everything after the nested block is this level's arm.
                let split = cur
                    .iter()
                    .position(|s| matches!(s, Stmt::Block { label: l, .. } if l.id == inner.id))
                    .map(|i| i + 1)
                    .unwrap_or(cur.len());
                rests.push(&cur[split..]);
                cur = inner_body;
            }
            _ => return Err(Reject::NotATower),
        }
    };
    if labels.len() < MIN_DEPTH {
        return Err(Reject::TooShallow(labels.len()));
    }

    // labels[0] is the outermost, labels[n-1] the innermost (holding the
    // br_table). Branching to the block labelled labels[i] lands at the start of
    // that block's trailing arm, which is rests[i-1] — the arm belonging to the
    // level one step further out.
    // Branching to labels[i] lands at the start of rests[i-1]; if that arm does
    // not terminate it runs into rests[i], and so on, until one terminates or
    // the chain falls out of the tower altogether.
    let arm_for = |target: u32| -> Option<Vec<&'a [Stmt]>> {
        let pos = labels.iter().position(|l| *l == target)?;
        if pos == 0 {
            // The outermost label: control leaves the tower entirely.
            return None;
        }
        // Outward: running off the end of rests[i-1] leaves the block labelled
        // labels[i-1], which lands at the start of rests[i-2], and so on.
        let mut chain = Vec::new();
        for rest in rests[..pos].iter().rev() {
            chain.push(*rest);
            if terminated(rest) {
                break;
            }
        }
        Some(chain)
    };

    // Only the *inner* labels must be private to this `br_table`. The outermost
    // is the switch's join: arms branch to it to mean "done", and after
    // lowering it stays a real frame, so those branches keep working unchanged.
    let table_refs = {
        let mut m = BTreeMap::new();
        for t in targets.iter().chain([default]) {
            if let BrTarget::Label { label, .. } = t {
                *m.entry(*label).or_default() += 1usize;
            }
        }
        m
    };
    for l in &labels[1..] {
        if refs.get(l).copied().unwrap_or(0) != table_refs.get(l).copied().unwrap_or(0) {
            return Err(Reject::ForeignBranch);
        }
    }

    let classify = |t: &'a BrTarget| -> Arm<'a> {
        match t {
            BrTarget::Label { label, .. } if labels.contains(label) => match arm_for(*label) {
                Some(body) => Arm::Body(body),
                None => Arm::Leave,
            },
            other => Arm::Branch(other),
        }
    };

    // Group targets by the label they name, preserving first-seen order.
    let mut order: Vec<&'a BrTarget> = Vec::new();
    let mut values: Vec<Vec<u32>> = Vec::new();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    for (i, t) in targets.iter().enumerate() {
        let key = match t {
            BrTarget::Label { label, .. } => format!("l{label}"),
            BrTarget::Return { .. } => format!("r{i}"),
        };
        match seen.get(&key) {
            Some(&slot) => values[slot].push(i as u32),
            None => {
                seen.insert(key, order.len());
                order.push(t);
                values.push(vec![i as u32]);
            }
        }
    }

    let arms: Vec<(Vec<u32>, Arm<'a>)> = order
        .into_iter()
        .zip(values)
        .map(|(t, v)| (v, classify(t)))
        .collect();
    let default_arm = classify(default);

    // Fall-through is flattened by `arm_for`, so nothing is rejected for it; a
    // chain that runs off the end of the tower simply exits the `case`, which
    // lands where leaving the outermost block did.

    Ok(Switch {
        prelude,
        index,
        arms,
        default: default_arm,
    })
}
