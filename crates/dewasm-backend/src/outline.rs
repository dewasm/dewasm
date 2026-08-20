//! Loop-body outlining: move a contiguous, branch-closed span of a loop body into its own function, called once per iteration.
//!
//! Ruby's JITs (and several others) compile a method only when it is entered through a call, never mid-execution, so a hot loop inside a function entered once is interpreted forever.
//! Splitting the body across a call turns it into a method invoked every iteration, which such a JIT does compile; the call costs nanoseconds per iteration while a compiled body of sufficient size saves far more.
//! The extracted span must be branch-closed (every branch inside it lands on a frame opened inside it), must not return, and may leave at most one value behind for the rest of the function, because a call returns at most one.
//! The span need not start at the body's first statement: a head-tested loop begins with its own exit branch, which stays behind while the work after it is extracted.
//! Thresholds are a per-backend judgement, passed in as [`Params`].

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use dewasm_core::ir::{BrTarget, Expr, Func, FuncType, Module, Stmt, Temp, ValType};

/// Extraction thresholds; each backend picks its own values.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Minimum region size in IR nodes (statements plus expression nodes).
    /// Below it the per-iteration call overhead outweighs what a compiled body can save.
    pub min_weight: u32,
    /// Maximum number of parameters an extracted function may take.
    pub max_params: usize,
    /// Maximum number of values the region may leave live for the rest of the function: 1 (returned from the call) or 0 (require none).
    pub max_results: usize,
    /// Minimum region size when the span consumes incoming temp values (see [`Extraction::arg_temps`]): such spans pay an extra argument and an entry copy per temp, so they must be larger to amortize it.
    pub min_weight_with_temps: u32,
}

/// The rewritten function list: the module's defined functions with outlined regions replaced by calls, followed by the extracted functions.
/// `types` extends the module's type list with any new signatures; indices below `module.types.len()` are unchanged, so only [`Func::type_idx`] lookups need the extended list.
pub struct Outline {
    pub funcs: Vec<Func>,
    pub types: Vec<FuncType>,
}

/// Boundaries per loop body are recorded only up to this many leading statements; a longer prefix is never a candidate.
const BOUNDARY_CAP: usize = 256;

pub fn outline(module: &Module, params: &Params) -> Outline {
    let mut types = module.types.clone();
    let mut funcs: Vec<Func> = module.funcs.clone();
    let mut synths: Vec<Func> = Vec::new();
    let base_idx = module.imported_funcs.len() as u32 + module.funcs.len() as u32;
    for func in &mut funcs {
        let boundaries = Liveness::run(func);
        let mut local_tys = types[func.type_idx as usize].params.clone();
        local_tys.extend(func.locals.iter().copied());
        let Func { body, temps, .. } = func;
        let mut cx = Cx {
            params,
            boundaries,
            local_tys,
            temps,
            types: &mut types,
            synths: &mut synths,
            base_idx,
        };
        walk_seq(body, &mut cx, false);
    }
    funcs.append(&mut synths);
    Outline { funcs, types }
}

/// A variable the analyses track: a wasm local or a spilled stack temp.
/// Globals and memory are instance state shared with the extracted function and need no tracking.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Var {
    Local(u32),
    Temp(Temp),
}

struct Cx<'a> {
    params: &'a Params,
    /// Per loop-body sequence (keyed by its buffer address): the live set at each leading top-level boundary, `[k]` = live just before the k-th statement.
    boundaries: BTreeMap<usize, Vec<BTreeSet<Var>>>,
    /// Types of the enclosing function's locals (params ++ locals).
    local_tys: Vec<ValType>,
    /// The enclosing function's temps, extended when a call needs a fresh result temp.
    temps: &'a mut Vec<Temp>,
    types: &'a mut Vec<FuncType>,
    synths: &'a mut Vec<Func>,
    /// Function index of the first synthesized function.
    base_idx: u32,
}

/// `in_try` is true inside any try_table of the same function: an exception thrown from an extracted region would then skip the write-back of values the catch handler could observe, so throw-capable regions are refused there.
fn walk_seq(seq: &mut [Stmt], cx: &mut Cx, in_try: bool) {
    for stmt in seq.iter_mut() {
        if let Stmt::Loop { label, body } = stmt {
            // A loop nothing branches back to runs its body once; a call there gains no compilation.
            if label.referenced {
                try_outline_loop(body, cx, in_try);
            }
        }
        let enters_try = matches!(stmt, Stmt::TryTable { .. });
        for child in stmt.child_seqs_mut() {
            walk_seq(child, cx, in_try || enters_try);
        }
    }
}

fn try_outline_loop(body: &mut Vec<Stmt>, cx: &mut Cx, in_try: bool) {
    let Some(bounds) = cx.boundaries.get(&(body.as_ptr() as usize)) else {
        return;
    };
    // Boundary live sets exist only up to the recording cap, so spans beyond it are not candidates.
    let limit = (bounds.len().saturating_sub(1)).min(body.len());
    let closed_flags: Vec<bool> = body[..limit].iter().map(closed).collect();
    let mut weights = Vec::with_capacity(limit + 1);
    weights.push(0u32);
    for stmt in &body[..limit] {
        weights.push(weights.last().unwrap().saturating_add(stmt_weight(stmt)));
    }

    // Maximal runs of consecutive closed statements, heaviest first: the heaviest run is where the loop's work sits.
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, ok) in closed_flags.iter().chain([&false]).enumerate() {
        match (ok, start) {
            (true, None) => start = Some(i),
            (false, Some(j)) => {
                runs.push((j, i));
                start = None;
            }
            _ => {}
        }
    }
    runs.sort_by_key(|(j, jend)| std::cmp::Reverse(weights[*jend] - weights[*j]));

    for (j, jend) in runs {
        if weights[jend] - weights[j] < cx.params.min_weight {
            return;
        }
        for k in ((j + 1)..=jend).rev() {
            if weights[k] - weights[j] < cx.params.min_weight {
                break;
            }
            if in_try && may_throw(&body[j..k]) {
                continue;
            }
            if let Some(extraction) =
                plan_extraction(&body[j..k], weights[k] - weights[j], &bounds[k], cx)
            {
                apply(body, j, k, extraction, cx);
                return;
            }
        }
    }
}

/// Whether executing `stmts` can raise a wasm exception (not a trap: traps unwind the whole program and expose no locals).
fn may_throw(stmts: &[Stmt]) -> bool {
    Stmt::any(stmts, &mut |s| {
        matches!(
            s,
            Stmt::Call { .. }
                | Stmt::CallIndirect { .. }
                | Stmt::Throw { .. }
                | Stmt::ThrowRef { .. }
        )
    })
}

/// Everything `apply` needs, computed on the immutable region.
struct Extraction {
    /// Caller locals passed as arguments, in ascending index order.
    arg_locals: Vec<u32>,
    /// Caller temps whose incoming value the region may read (a span starting mid-body can consume what earlier statements computed), passed as trailing arguments and copied into the matching temp at the extracted function's entry.
    arg_temps: Vec<Temp>,
    /// The one variable the region leaves live for the rest of the function, if any.
    live_out: Option<Var>,
    /// Caller locals that become the extracted function's own locals (referenced, not parameters), ascending.
    inner_locals: Vec<u32>,
    /// Temps the extracted function declares.
    temps: Vec<Temp>,
}

fn plan_extraction(
    region: &[Stmt],
    weight: u32,
    live_after: &BTreeSet<Var>,
    cx: &Cx,
) -> Option<Extraction> {
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    collect_seq(region, &mut reads, &mut writes);

    let live_outs: Vec<Var> = writes.intersection(live_after).copied().collect();
    if live_outs.len() > cx.params.max_results {
        return None;
    }
    let live_out = live_outs.first().copied();

    let (maybe_read_first, definite) = first_read_scan(region);
    let mut param_locals: BTreeSet<u32> = BTreeSet::new();
    let mut param_temps: BTreeSet<Temp> = BTreeSet::new();
    for v in &maybe_read_first {
        match v {
            Var::Local(i) => {
                param_locals.insert(*i);
            }
            Var::Temp(t) => {
                param_temps.insert(*t);
            }
        }
    }
    if let Some(v) = live_out {
        // A live-out the region does not assign on every path must flow through from the caller.
        if !definite.contains(&v) {
            match v {
                Var::Local(i) => {
                    param_locals.insert(i);
                }
                Var::Temp(t) => {
                    param_temps.insert(t);
                }
            }
        }
    }
    if param_locals.len() + param_temps.len() > cx.params.max_params {
        return None;
    }
    if !param_temps.is_empty() && weight < cx.params.min_weight_with_temps {
        return None;
    }

    let arg_locals: Vec<u32> = param_locals.iter().copied().collect();
    let arg_temps: Vec<Temp> = param_temps.iter().copied().collect();
    let inner_locals: Vec<u32> = reads
        .union(&writes)
        .filter_map(|v| match v {
            Var::Local(i) if !param_locals.contains(i) => Some(*i),
            _ => None,
        })
        .collect();
    let temps: Vec<Temp> = reads
        .union(&writes)
        .filter_map(|v| match v {
            Var::Temp(t) => Some(*t),
            _ => None,
        })
        .collect();
    Some(Extraction {
        arg_locals,
        arg_temps,
        live_out,
        inner_locals,
        temps,
    })
}

fn apply(body: &mut Vec<Stmt>, j: usize, k: usize, ext: Extraction, cx: &mut Cx) {
    let mut region: Vec<Stmt> = body.drain(j..k).collect();

    // Renumber caller locals into the extracted function's local index space: local parameters, then temp parameters, then its own locals.
    let params = ext.arg_locals.len() + ext.arg_temps.len();
    let mut map: BTreeMap<u32, u32> = BTreeMap::new();
    for (new, old) in ext.arg_locals.iter().enumerate() {
        map.insert(*old, new as u32);
    }
    for (new, old) in ext.inner_locals.iter().enumerate() {
        map.insert(*old, (params + new) as u32);
    }
    for stmt in &mut region {
        renumber_stmt(stmt, &map);
    }
    // An incoming temp value arrives as a parameter and is copied into its temp at entry, so the region body reads it unchanged.
    for (i, t) in ext.arg_temps.iter().enumerate().rev() {
        region.insert(
            0,
            Stmt::Assign {
                dst: *t,
                expr: Expr::LocalGet((ext.arg_locals.len() + i) as u32),
            },
        );
    }

    let param_tys: Vec<ValType> = ext
        .arg_locals
        .iter()
        .map(|i| cx.local_tys[*i as usize])
        .chain(ext.arg_temps.iter().map(|t| t.ty))
        .collect();
    let result_ty = ext.live_out.map(|v| match v {
        Var::Local(i) => cx.local_tys[i as usize],
        Var::Temp(t) => t.ty,
    });
    match ext.live_out {
        Some(Var::Local(i)) => region.push(Stmt::Return {
            values: vec![Expr::LocalGet(map[&i])],
        }),
        Some(Var::Temp(t)) => region.push(Stmt::Return {
            values: vec![Expr::Temp(t)],
        }),
        None => {}
    }

    let ty = FuncType {
        params: param_tys,
        results: result_ty.into_iter().collect(),
    };
    let type_idx = cx.types.iter().position(|t| *t == ty).unwrap_or_else(|| {
        cx.types.push(ty);
        cx.types.len() - 1
    }) as u32;

    let func_idx = cx.base_idx + cx.synths.len() as u32;
    cx.synths.push(Func {
        type_idx,
        locals: ext
            .inner_locals
            .iter()
            .map(|i| cx.local_tys[*i as usize])
            .collect(),
        temps: ext.temps,
        body: region,
    });

    let args: Vec<Expr> = ext
        .arg_locals
        .iter()
        .map(|i| Expr::LocalGet(*i))
        .chain(ext.arg_temps.iter().map(|t| Expr::Temp(*t)))
        .collect();
    let replacement: Vec<Stmt> = match ext.live_out {
        None => vec![Stmt::Call {
            func: func_idx,
            args,
            results: vec![],
        }],
        Some(Var::Temp(t)) => vec![Stmt::Call {
            func: func_idx,
            args,
            results: vec![t],
        }],
        Some(Var::Local(i)) => {
            let depth = cx.temps.iter().map(|t| t.depth).max().unwrap_or(0) + 1;
            let result = Temp {
                depth,
                ty: cx.local_tys[i as usize],
            };
            cx.temps.push(result);
            cx.temps.sort();
            cx.temps.dedup();
            vec![
                Stmt::Call {
                    func: func_idx,
                    args,
                    results: vec![result],
                },
                Stmt::LocalSet {
                    idx: i,
                    expr: Expr::Temp(result),
                },
            ]
        }
    };
    body.splice(j..j, replacement);
}

/// Whether every branch inside `stmt` lands on a frame opened inside `stmt` itself.
/// A statement that returns, or branches to the enclosing loop or further out, pins the extraction boundary in front of it.
fn closed(stmt: &Stmt) -> bool {
    fn go(stmt: &Stmt, scope: &mut Vec<u32>) -> bool {
        let target_ok = |t: &BrTarget, scope: &Vec<u32>| match t {
            BrTarget::Return { .. } => false,
            BrTarget::Label { label, .. } => scope.contains(label),
        };
        let own_ok = match stmt {
            Stmt::Br(t) => target_ok(t, scope),
            Stmt::BrIf { target, .. } => target_ok(target, scope),
            Stmt::BrTable {
                targets, default, ..
            } => targets.iter().chain([default]).all(|t| target_ok(t, scope)),
            Stmt::Return { .. } => false,
            // Catch targets are resolved outside the try_table's own frame, so they are checked against the scope before it opens.
            Stmt::TryTable { catches, .. } => catches.iter().all(|c| target_ok(&c.target, scope)),
            _ => true,
        };
        if !own_ok {
            return false;
        }
        let label = match stmt {
            Stmt::Block { label, .. }
            | Stmt::Loop { label, .. }
            | Stmt::If { label, .. }
            | Stmt::TryTable { label, .. } => Some(label.id),
            _ => None,
        };
        if let Some(id) = label {
            scope.push(id);
        }
        let ok = stmt
            .child_seqs()
            .all(|seq| seq.iter().all(|s| go(s, scope)));
        if label.is_some() {
            scope.pop();
        }
        ok
    }
    go(stmt, &mut Vec::new())
}

fn expr_weight(expr: &Expr) -> u32 {
    1 + match expr {
        Expr::I32Const(_)
        | Expr::I64Const(_)
        | Expr::F32Const(_)
        | Expr::F64Const(_)
        | Expr::Temp(_)
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::MemorySize => 0,
        Expr::Un(_, a) => expr_weight(a),
        Expr::Bin(_, a, b) => expr_weight(a) + expr_weight(b),
        Expr::Load { addr, .. } => expr_weight(addr),
        Expr::Select { cond, then, els } => {
            expr_weight(cond) + expr_weight(then) + expr_weight(els)
        }
    }
}

fn stmt_weight(stmt: &Stmt) -> u32 {
    let mut w = 1;
    for_each_expr(stmt, &mut |e| w += expr_weight(e));
    w + stmt.child_seqs().flatten().map(stmt_weight).sum::<u32>()
}

/// Visit every expression held directly by `stmt` (not those of nested statements).
fn for_each_expr(stmt: &Stmt, f: &mut impl FnMut(&Expr)) {
    let target_exprs = |t: &BrTarget, f: &mut dyn FnMut(&Expr)| {
        if let BrTarget::Return { values } = t {
            values.iter().for_each(f);
        }
    };
    match stmt {
        Stmt::SourceLine(_) | Stmt::DataDrop { .. } | Stmt::ElemDrop { .. } | Stmt::Unreachable => {
        }
        Stmt::Assign { expr, .. } | Stmt::LocalSet { expr, .. } | Stmt::GlobalSet { expr, .. } => {
            f(expr)
        }
        Stmt::Store { addr, value, .. } => {
            f(addr);
            f(value);
        }
        Stmt::Block { .. } | Stmt::Loop { .. } => {}
        Stmt::If { cond, .. } => f(cond),
        Stmt::Br(t) => target_exprs(t, f),
        Stmt::BrIf { cond, target } => {
            f(cond);
            target_exprs(target, f);
        }
        Stmt::BrTable {
            index,
            targets,
            default,
        } => {
            f(index);
            for t in targets.iter().chain([default]) {
                target_exprs(t, f);
            }
        }
        Stmt::Return { values } => values.iter().for_each(f),
        Stmt::Call { args, .. } | Stmt::Throw { args, .. } => args.iter().for_each(f),
        Stmt::CallIndirect { index, args, .. } => {
            f(index);
            args.iter().for_each(f);
        }
        Stmt::MemoryGrow { delta, .. } => f(delta),
        Stmt::MemoryCopy { dst, src, len }
        | Stmt::MemoryInit { dst, src, len, .. }
        | Stmt::TableInit { dst, src, len, .. }
        | Stmt::TableCopy { dst, src, len, .. } => {
            f(dst);
            f(src);
            f(len);
        }
        Stmt::MemoryFill { dst, val, len } => {
            f(dst);
            f(val);
            f(len);
        }
        Stmt::TryTable { .. } => {}
        Stmt::ThrowRef { exn } => f(exn),
    }
}

fn expr_reads(expr: &Expr, out: &mut BTreeSet<Var>) {
    match expr {
        Expr::I32Const(_)
        | Expr::I64Const(_)
        | Expr::F32Const(_)
        | Expr::F64Const(_)
        | Expr::GlobalGet(_)
        | Expr::MemorySize => {}
        Expr::Temp(t) => {
            out.insert(Var::Temp(*t));
        }
        Expr::LocalGet(i) => {
            out.insert(Var::Local(*i));
        }
        Expr::Un(_, a) => expr_reads(a, out),
        Expr::Bin(_, a, b) => {
            expr_reads(a, out);
            expr_reads(b, out);
        }
        Expr::Load { addr, .. } => expr_reads(addr, out),
        Expr::Select { cond, then, els } => {
            expr_reads(cond, out);
            expr_reads(then, out);
            expr_reads(els, out);
        }
    }
}

/// Variables written directly by `stmt` (not by nested statements).
fn stmt_writes(stmt: &Stmt, out: &mut BTreeSet<Var>) {
    match stmt {
        Stmt::Assign { dst, .. } | Stmt::MemoryGrow { dst, .. } => {
            out.insert(Var::Temp(*dst));
        }
        Stmt::LocalSet { idx, .. } => {
            out.insert(Var::Local(*idx));
        }
        Stmt::Call { results, .. } | Stmt::CallIndirect { results, .. } => {
            out.extend(results.iter().map(|t| Var::Temp(*t)));
        }
        Stmt::Br(t) | Stmt::BrIf { target: t, .. } => target_writes(t, out),
        Stmt::BrTable {
            targets, default, ..
        } => {
            for t in targets.iter().chain([default]) {
                target_writes(t, out);
            }
        }
        Stmt::TryTable { catches, .. } => {
            for c in catches {
                out.extend(c.value_temps.iter().map(|t| Var::Temp(*t)));
                target_writes(&c.target, out);
            }
        }
        _ => {}
    }
}

fn target_writes(t: &BrTarget, out: &mut BTreeSet<Var>) {
    if let BrTarget::Label { assigns, .. } = t {
        out.extend(assigns.iter().map(|(dst, _)| Var::Temp(*dst)));
    }
}

fn target_reads(t: &BrTarget, out: &mut BTreeSet<Var>) {
    match t {
        BrTarget::Return { values } => values.iter().for_each(|v| expr_reads(v, out)),
        BrTarget::Label { assigns, .. } => {
            out.extend(assigns.iter().map(|(_, src)| Var::Temp(*src)));
        }
    }
}

/// Syntactic reads and writes of a whole subtree.
fn collect_seq(stmts: &[Stmt], reads: &mut BTreeSet<Var>, writes: &mut BTreeSet<Var>) {
    for stmt in stmts {
        for_each_expr(stmt, &mut |e| expr_reads(e, reads));
        match stmt {
            Stmt::Br(t) | Stmt::BrIf { target: t, .. } => target_reads(t, reads),
            Stmt::BrTable {
                targets, default, ..
            } => {
                for t in targets.iter().chain([default]) {
                    target_reads(t, reads);
                }
            }
            Stmt::TryTable { catches, .. } => {
                for c in catches {
                    target_reads(&c.target, reads);
                }
            }
            _ => {}
        }
        stmt_writes(stmt, writes);
        for seq in stmt.child_seqs() {
            collect_seq(seq, reads, writes);
        }
    }
}

/// Walk the region's top-level statements in order, tracking what straight-line statements definitely assign.
/// Returns the variables possibly read before assignment (they must be parameters) and the definitely-assigned set (safe as callee locals, and a live-out among them needs no incoming value).
/// Anything read inside nested control flow counts as possibly read first, and nothing written there counts as definite.
fn first_read_scan(stmts: &[Stmt]) -> (BTreeSet<Var>, BTreeSet<Var>) {
    let mut maybe_read = BTreeSet::new();
    let mut definite: BTreeSet<Var> = BTreeSet::new();
    for stmt in stmts {
        let mut reads = BTreeSet::new();
        if stmt.child_seqs().next().is_some() {
            let mut nested_writes = BTreeSet::new();
            collect_seq(std::slice::from_ref(stmt), &mut reads, &mut nested_writes);
        } else {
            for_each_expr(stmt, &mut |e| expr_reads(e, &mut reads));
            match stmt {
                Stmt::Br(t) | Stmt::BrIf { target: t, .. } => target_reads(t, &mut reads),
                Stmt::BrTable {
                    targets, default, ..
                } => {
                    for t in targets.iter().chain([default]) {
                        target_reads(t, &mut reads);
                    }
                }
                _ => {}
            }
        }
        maybe_read.extend(reads.difference(&definite).copied());
        // Only unconditional straight-line writes count as definite; a branch's assigns or a catch clause's temps may not execute.
        // Every top-level statement boundary is reached in order because branch-closure keeps control inside each statement.
        match stmt {
            Stmt::Assign { dst, .. } | Stmt::MemoryGrow { dst, .. } => {
                definite.insert(Var::Temp(*dst));
            }
            Stmt::LocalSet { idx, .. } => {
                definite.insert(Var::Local(*idx));
            }
            Stmt::Call { results, .. } | Stmt::CallIndirect { results, .. } => {
                definite.extend(results.iter().map(|t| Var::Temp(*t)));
            }
            _ => {}
        }
    }
    (maybe_read, definite)
}

/// Rewrite local indices per `map`; the map covers every local the region references.
fn renumber_stmt(stmt: &mut Stmt, map: &BTreeMap<u32, u32>) {
    fn renumber_expr(expr: &mut Expr, map: &BTreeMap<u32, u32>) {
        match expr {
            Expr::LocalGet(i) => *i = map[i],
            Expr::I32Const(_)
            | Expr::I64Const(_)
            | Expr::F32Const(_)
            | Expr::F64Const(_)
            | Expr::Temp(_)
            | Expr::GlobalGet(_)
            | Expr::MemorySize => {}
            Expr::Un(_, a) => renumber_expr(a, map),
            Expr::Bin(_, a, b) => {
                renumber_expr(a, map);
                renumber_expr(b, map);
            }
            Expr::Load { addr, .. } => renumber_expr(addr, map),
            Expr::Select { cond, then, els } => {
                renumber_expr(cond, map);
                renumber_expr(then, map);
                renumber_expr(els, map);
            }
        }
    }
    for_each_expr_mut(stmt, &mut |e| renumber_expr(e, map));
    if let Stmt::LocalSet { idx, .. } = stmt {
        *idx = map[idx];
    }
    for seq in stmt.child_seqs_mut() {
        for s in seq {
            renumber_stmt(s, map);
        }
    }
}

/// Mutable counterpart of [`for_each_expr`].
fn for_each_expr_mut(stmt: &mut Stmt, f: &mut impl FnMut(&mut Expr)) {
    let target_exprs = |t: &mut BrTarget, f: &mut dyn FnMut(&mut Expr)| {
        if let BrTarget::Return { values } = t {
            values.iter_mut().for_each(f);
        }
    };
    match stmt {
        Stmt::SourceLine(_) | Stmt::DataDrop { .. } | Stmt::ElemDrop { .. } | Stmt::Unreachable => {
        }
        Stmt::Assign { expr, .. } | Stmt::LocalSet { expr, .. } | Stmt::GlobalSet { expr, .. } => {
            f(expr)
        }
        Stmt::Store { addr, value, .. } => {
            f(addr);
            f(value);
        }
        Stmt::Block { .. } | Stmt::Loop { .. } => {}
        Stmt::If { cond, .. } => f(cond),
        Stmt::Br(t) => target_exprs(t, f),
        Stmt::BrIf { cond, target } => {
            f(cond);
            target_exprs(target, f);
        }
        Stmt::BrTable {
            index,
            targets,
            default,
        } => {
            f(index);
            for t in targets.iter_mut().chain([default]) {
                target_exprs(t, f);
            }
        }
        Stmt::Return { values } => values.iter_mut().for_each(f),
        Stmt::Call { args, .. } | Stmt::Throw { args, .. } => args.iter_mut().for_each(f),
        Stmt::CallIndirect { index, args, .. } => {
            f(index);
            args.iter_mut().for_each(f);
        }
        Stmt::MemoryGrow { delta, .. } => f(delta),
        Stmt::MemoryCopy { dst, src, len }
        | Stmt::MemoryInit { dst, src, len, .. }
        | Stmt::TableInit { dst, src, len, .. }
        | Stmt::TableCopy { dst, src, len, .. } => {
            f(dst);
            f(src);
            f(len);
        }
        Stmt::MemoryFill { dst, val, len } => {
            f(dst);
            f(val);
            f(len);
        }
        Stmt::TryTable { .. } => {}
        Stmt::ThrowRef { exn } => f(exn),
    }
}

/// Backward may-liveness over the structured body.
/// Records, for every loop body, the live set at each leading top-level statement boundary; those are the only program points the extraction queries.
struct Liveness {
    /// Live set at each in-scope label's continuation: after the frame for blocks, ifs and try_tables, at the head for loops.
    labels: BTreeMap<u32, BTreeSet<Var>>,
    /// Union of the live sets at the catch targets of enclosing try_tables: what an exception thrown here may expose.
    catch_live: BTreeSet<Var>,
    boundaries: BTreeMap<usize, Vec<BTreeSet<Var>>>,
}

impl Liveness {
    fn run(func: &Func) -> BTreeMap<usize, Vec<BTreeSet<Var>>> {
        let mut lv = Liveness {
            labels: BTreeMap::new(),
            catch_live: BTreeSet::new(),
            boundaries: BTreeMap::new(),
        };
        lv.seq(&func.body, BTreeSet::new(), false);
        lv.boundaries
    }

    /// Live set before `stmts`, given `after` live at the fall-through exit.
    /// With `record`, stores the boundary sets for this sequence; enclosing loops re-analyze their bodies to a fixpoint, so the last visit's record is the stable one.
    fn seq(&mut self, stmts: &[Stmt], after: BTreeSet<Var>, record: bool) -> BTreeSet<Var> {
        let mut bounds: Vec<BTreeSet<Var>> = Vec::new();
        let mut live = after;
        if record {
            bounds.push(live.clone());
        }
        for stmt in stmts.iter().rev() {
            live = self.stmt(stmt, live);
            if record {
                bounds.push(live.clone());
            }
        }
        if record {
            bounds.reverse();
            bounds.truncate(BOUNDARY_CAP + 1);
            self.boundaries.insert(stmts.as_ptr() as usize, bounds);
        }
        live
    }

    fn stmt(&mut self, stmt: &Stmt, mut live: BTreeSet<Var>) -> BTreeSet<Var> {
        match stmt {
            Stmt::SourceLine(_) | Stmt::DataDrop { .. } | Stmt::ElemDrop { .. } => live,
            Stmt::Assign { dst, expr } => {
                live.remove(&Var::Temp(*dst));
                expr_reads(expr, &mut live);
                live
            }
            Stmt::LocalSet { idx, expr } => {
                live.remove(&Var::Local(*idx));
                expr_reads(expr, &mut live);
                live
            }
            Stmt::GlobalSet { expr, .. } => {
                expr_reads(expr, &mut live);
                live
            }
            Stmt::Store { addr, value, .. } => {
                expr_reads(addr, &mut live);
                expr_reads(value, &mut live);
                live
            }
            Stmt::Call { args, results, .. } => {
                for r in results {
                    live.remove(&Var::Temp(*r));
                }
                // The callee may throw to an enclosing catch clause, so whatever those clauses need stays live across the call.
                live.extend(self.catch_live.iter().copied());
                for a in args {
                    expr_reads(a, &mut live);
                }
                live
            }
            Stmt::CallIndirect {
                index,
                args,
                results,
                ..
            } => {
                for r in results {
                    live.remove(&Var::Temp(*r));
                }
                live.extend(self.catch_live.iter().copied());
                expr_reads(index, &mut live);
                for a in args {
                    expr_reads(a, &mut live);
                }
                live
            }
            Stmt::MemoryGrow { dst, delta } => {
                live.remove(&Var::Temp(*dst));
                expr_reads(delta, &mut live);
                live
            }
            Stmt::MemoryCopy { dst, src, len }
            | Stmt::MemoryInit { dst, src, len, .. }
            | Stmt::TableInit { dst, src, len, .. }
            | Stmt::TableCopy { dst, src, len, .. } => {
                expr_reads(dst, &mut live);
                expr_reads(src, &mut live);
                expr_reads(len, &mut live);
                live
            }
            Stmt::MemoryFill { dst, val, len } => {
                expr_reads(dst, &mut live);
                expr_reads(val, &mut live);
                expr_reads(len, &mut live);
                live
            }
            Stmt::Block { label, body } => {
                self.labels.insert(label.id, live.clone());
                let l = self.seq(body, live, false);
                self.labels.remove(&label.id);
                l
            }
            Stmt::Loop { label, body } => {
                // Fixpoint on the live set at the loop head: a back edge re-enters the body, and falling off its end exits the loop.
                let mut head = BTreeSet::new();
                loop {
                    self.labels.insert(label.id, head.clone());
                    let new_head = self.seq(body, live.clone(), true);
                    if new_head == head {
                        break;
                    }
                    head = new_head;
                }
                self.labels.remove(&label.id);
                head
            }
            Stmt::If {
                label,
                cond,
                then,
                els,
            } => {
                self.labels.insert(label.id, live.clone());
                let mut l = self.seq(then, live.clone(), false);
                l.extend(self.seq(els, live, false));
                expr_reads(cond, &mut l);
                self.labels.remove(&label.id);
                l
            }
            Stmt::Br(target) => self.target_live(target),
            Stmt::BrIf { cond, target } => {
                live.extend(self.target_live(target));
                expr_reads(cond, &mut live);
                live
            }
            Stmt::BrTable {
                index,
                targets,
                default,
            } => {
                let mut l = BTreeSet::new();
                for t in targets.iter().chain([default]) {
                    l.extend(self.target_live(t));
                }
                expr_reads(index, &mut l);
                l
            }
            Stmt::Return { values } => {
                let mut l = BTreeSet::new();
                for v in values {
                    expr_reads(v, &mut l);
                }
                l
            }
            Stmt::TryTable {
                label,
                catches,
                body,
            } => {
                self.labels.insert(label.id, live.clone());
                let saved = self.catch_live.clone();
                for c in catches {
                    let mut cl = self.target_live(&c.target);
                    for t in &c.value_temps {
                        cl.remove(&Var::Temp(*t));
                    }
                    self.catch_live.extend(cl);
                }
                let l = self.seq(body, live, false);
                self.catch_live = saved;
                self.labels.remove(&label.id);
                l
            }
            Stmt::Throw { args, .. } => {
                let mut l = self.catch_live.clone();
                for a in args {
                    expr_reads(a, &mut l);
                }
                l
            }
            Stmt::ThrowRef { exn } => {
                let mut l = self.catch_live.clone();
                expr_reads(exn, &mut l);
                l
            }
            Stmt::Unreachable => BTreeSet::new(),
        }
    }

    fn target_live(&self, t: &BrTarget) -> BTreeSet<Var> {
        match t {
            BrTarget::Return { values } => {
                let mut l = BTreeSet::new();
                for v in values {
                    expr_reads(v, &mut l);
                }
                l
            }
            BrTarget::Label { label, assigns, .. } => {
                let mut l = self.labels.get(label).cloned().unwrap_or_default();
                for (dst, src) in assigns.iter().rev() {
                    l.remove(&Var::Temp(*dst));
                    l.insert(Var::Temp(*src));
                }
                l
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dewasm_core::ir::{BinOp, Label};

    fn module_of(params: Vec<ValType>, locals: Vec<ValType>, body: Vec<Stmt>) -> Module {
        Module {
            types: vec![FuncType {
                params,
                results: vec![ValType::I32],
            }],
            imported_funcs: vec![],
            funcs: vec![Func {
                type_idx: 0,
                locals,
                temps: vec![],
                body,
            }],
            imported_tables: vec![],
            tables: vec![],
            imported_memory: None,
            memory: None,
            imported_globals: vec![],
            globals: vec![],
            imported_tags: vec![],
            tags: vec![],
            exports: vec![],
            elems: vec![],
            datas: vec![],
            start: None,
            debug_files: vec![],
        }
    }

    fn local_set(idx: u32, expr: Expr) -> Stmt {
        Stmt::LocalSet { idx, expr }
    }

    fn bin(op: BinOp, a: Expr, b: Expr) -> Expr {
        Expr::Bin(op, Box::new(a), Box::new(b))
    }

    fn back_edge(cond: Expr, label: u32) -> Stmt {
        Stmt::BrIf {
            cond,
            target: BrTarget::Label {
                label,
                is_loop: true,
                assigns: vec![],
            },
        }
    }

    /// `l1` counts to `l0`; `l2` accumulates `l3 = l1 * l1` per iteration.
    /// The extractable span is the two leading statements: the count update reads the back edge's own condition local, and the back edge itself targets the loop.
    fn accumulate_body() -> Vec<Stmt> {
        vec![
            local_set(1, Expr::I32Const(0)),
            local_set(2, Expr::I32Const(0)),
            Stmt::Loop {
                label: Label {
                    id: 0,
                    referenced: true,
                },
                body: vec![
                    local_set(3, bin(BinOp::I32Mul, Expr::LocalGet(1), Expr::LocalGet(1))),
                    local_set(2, bin(BinOp::I32Add, Expr::LocalGet(2), Expr::LocalGet(3))),
                    local_set(1, bin(BinOp::I32Add, Expr::LocalGet(1), Expr::I32Const(1))),
                    back_edge(bin(BinOp::I32Ne, Expr::LocalGet(1), Expr::LocalGet(0)), 0),
                ],
            },
            Stmt::Return {
                values: vec![Expr::LocalGet(2)],
            },
        ]
    }

    const TEST_PARAMS: Params = Params {
        min_weight: 1,
        max_params: 8,
        max_results: 1,
        min_weight_with_temps: 1,
    };

    #[test]
    fn extracts_accumulator_loop_body() {
        let module = module_of(vec![ValType::I32], vec![ValType::I32; 3], accumulate_body());
        let out = outline(&module, &TEST_PARAMS);
        assert_eq!(out.funcs.len(), 2);

        let synth = &out.funcs[1];
        let ty = &out.types[synth.type_idx as usize];
        // The count local and the accumulator flow in; the accumulator flows back out; the scratch local stays inside.
        assert_eq!(ty.params, vec![ValType::I32, ValType::I32]);
        assert_eq!(ty.results, vec![ValType::I32]);
        assert_eq!(synth.locals, vec![ValType::I32]);
        assert!(matches!(synth.body.last(), Some(Stmt::Return { .. })));

        let Stmt::Loop { body, .. } = &out.funcs[0].body[2] else {
            panic!("loop expected");
        };
        let Stmt::Call {
            func,
            args,
            results,
        } = &body[0]
        else {
            panic!("call expected, got {:?}", body[0]);
        };
        assert_eq!(*func, 1);
        assert_eq!(args.len(), 2);
        assert_eq!(results.len(), 1);
        assert!(matches!(body[1], Stmt::LocalSet { idx: 2, .. }));
        // The count update and the back edge stay behind.
        assert_eq!(body.len(), 4);
    }

    #[test]
    fn extracts_past_a_leading_exit_branch() {
        // Head-tested shape: the loop opens with its own exit branch, does the work, then takes the back edge; the work in between is the extractable run.
        let body = vec![
            Stmt::Block {
                label: Label {
                    id: 9,
                    referenced: true,
                },
                body: vec![Stmt::Loop {
                    label: Label {
                        id: 0,
                        referenced: true,
                    },
                    body: vec![
                        Stmt::BrIf {
                            cond: bin(BinOp::I32Eq, Expr::LocalGet(1), Expr::LocalGet(0)),
                            target: BrTarget::Label {
                                label: 9,
                                is_loop: false,
                                assigns: vec![],
                            },
                        },
                        local_set(3, bin(BinOp::I32Mul, Expr::LocalGet(1), Expr::LocalGet(1))),
                        local_set(2, bin(BinOp::I32Add, Expr::LocalGet(2), Expr::LocalGet(3))),
                        local_set(1, bin(BinOp::I32Add, Expr::LocalGet(1), Expr::I32Const(1))),
                        Stmt::Br(BrTarget::Label {
                            label: 0,
                            is_loop: true,
                            assigns: vec![],
                        }),
                    ],
                }],
            },
            Stmt::Return {
                values: vec![Expr::LocalGet(2)],
            },
        ];
        let module = module_of(vec![ValType::I32], vec![ValType::I32; 3], body);
        let out = outline(&module, &TEST_PARAMS);
        assert_eq!(out.funcs.len(), 2);

        let Stmt::Block { body, .. } = &out.funcs[0].body[0] else {
            panic!("block expected");
        };
        let Stmt::Loop { body, .. } = &body[0] else {
            panic!("loop expected");
        };
        assert!(matches!(body[0], Stmt::BrIf { .. }));
        assert!(matches!(body[1], Stmt::Call { .. }));
        assert!(matches!(body[2], Stmt::LocalSet { idx: 2, .. }));
        assert!(matches!(body[4], Stmt::Br(_)));
    }

    #[test]
    fn outward_branch_pins_the_boundary() {
        let body = vec![Stmt::Block {
            label: Label {
                id: 5,
                referenced: true,
            },
            body: vec![Stmt::Loop {
                label: Label {
                    id: 0,
                    referenced: true,
                },
                body: vec![
                    Stmt::Br(BrTarget::Label {
                        label: 5,
                        is_loop: false,
                        assigns: vec![],
                    }),
                    back_edge(Expr::LocalGet(0), 0),
                ],
            }],
        }];
        let module = module_of(vec![ValType::I32], vec![], body);
        let out = outline(&module, &TEST_PARAMS);
        assert_eq!(out.funcs.len(), 1);
    }

    #[test]
    fn respects_min_weight() {
        let module = module_of(vec![ValType::I32], vec![ValType::I32; 3], accumulate_body());
        let params = Params {
            min_weight: 1000,
            ..TEST_PARAMS
        };
        let out = outline(&module, &params);
        assert_eq!(out.funcs.len(), 1);
    }

    #[test]
    fn skips_unreferenced_loops() {
        let mut body = accumulate_body();
        let Stmt::Loop { label, body: inner } = &mut body[2] else {
            unreachable!();
        };
        label.referenced = false;
        inner.pop();
        let module = module_of(vec![ValType::I32], vec![ValType::I32; 3], body);
        let out = outline(&module, &TEST_PARAMS);
        assert_eq!(out.funcs.len(), 1);
    }
}
