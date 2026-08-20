//! Hoist constant-address loads out of loops, with a runtime store guard instead of an alias proof.
//!
//! Code compiled to wasm re-reads memory-resident globals every loop iteration because its own compiler could not prove the loop's stores leave them alone, and neither can this pass.
//! What it can check is the run-time address: hoist each constant-address load into a local before the loop, and after every store inside the loop compare the store's address against the hoisted address window, reloading the locals on overlap.
//! A few compares per store buy back a load per hoisted address per iteration, and the reload path keeps an aliasing program exact.
//!
//! Hoisting runs a load earlier, possibly on paths that would have skipped it, so only loads that can never trap are hoisted: the constant address plus access width must fit inside the memory's minimum size, which a memory never shrinks below.

use std::collections::BTreeMap;

use dewasm_core::ir::{
    BinOp, Expr, Func, FuncType, Label, LoadOp, Module, Stmt, StoreOp, Temp, ValType,
};

/// The memory's minimum size in bytes: the floor a memory never shrinks below, imported or not.
pub fn memory_min_bytes(module: &Module) -> Option<u64> {
    module
        .memory
        .as_ref()
        .map(|m| m.min_pages * 65536)
        .or_else(|| module.imported_memory.as_ref().map(|m| m.min_pages * 65536))
}

use crate::outline::{for_each_expr, for_each_expr_mut};

/// Hoisting thresholds; each backend picks its own values.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// Minimum number of distinct hoistable loads in a loop that contains stores: below it the per-store guard costs more than the hoisted loads save.
    /// A loop without stores needs no guards and hoists from one load up.
    pub min_hoisted_with_stores: usize,
}

/// Rewrite `funcs` in place, hoisting invariant constant-address loads out of eligible loops.
/// `types` resolves each function's parameter count; `mem_min_bytes` is the memory's minimum size (`None` when the module has no memory, which disables the pass).
pub fn hoist(funcs: &mut [Func], types: &[FuncType], mem_min_bytes: Option<u64>, params: &Params) {
    let Some(mem_min) = mem_min_bytes else {
        return;
    };
    for func in funcs {
        let mut cx = Cx {
            params,
            mem_min,
            next_local: types[func.type_idx as usize].params.len() as u32
                + func.locals.len() as u32,
            new_locals: Vec::new(),
            next_label: max_label_id(&func.body) + 1,
            spill_depth: func.temps.iter().map(|t| t.depth).max().unwrap_or(0) + 1,
            spill_used: false,
        };
        walk_seq(&mut func.body, &mut cx);
        func.locals.extend(cx.new_locals);
        if cx.spill_used {
            func.temps.push(Temp {
                depth: cx.spill_depth,
                ty: ValType::I32,
            });
            func.temps.sort();
            func.temps.dedup();
        }
    }
}

struct Cx<'a> {
    params: &'a Params,
    mem_min: u64,
    next_local: u32,
    new_locals: Vec<ValType>,
    next_label: u32,
    /// One reusable temp per function for spilling a guarded store's address.
    spill_depth: u32,
    spill_used: bool,
}

impl Cx<'_> {
    fn spill(&mut self) -> Temp {
        self.spill_used = true;
        Temp {
            depth: self.spill_depth,
            ty: ValType::I32,
        }
    }
}

/// One hoisted load: its shape and the local now holding its value.
struct Hoisted {
    op: LoadOp,
    addr: u32,
    offset: u64,
    local: u32,
}

fn walk_seq(seq: &mut Vec<Stmt>, cx: &mut Cx) {
    let mut i = 0;
    while i < seq.len() {
        let inserted = if matches!(seq[i], Stmt::Loop { .. }) {
            try_hoist_loop(seq, i, cx)
        } else {
            0
        };
        if inserted > 0 {
            // The loop moved to `i + inserted` and its interior is fully processed.
            i += inserted + 1;
            continue;
        }
        for child in seq[i].child_seqs_mut() {
            walk_seq(child, cx);
        }
        i += 1;
    }
}

/// Try to hoist out of the loop at `seq[at]`; returns the number of preheader statements inserted before it.
fn try_hoist_loop(seq: &mut Vec<Stmt>, at: usize, cx: &mut Cx) -> usize {
    let Stmt::Loop { body, .. } = &seq[at] else {
        return 0;
    };
    if has_barrier(body) {
        return 0;
    }
    let mut loads: BTreeMap<(u32, u64, LoadOp), ()> = BTreeMap::new();
    collect_const_loads(body, cx.mem_min, &mut loads);
    let store_count = count_stores(body);
    let needed = if store_count == 0 {
        1
    } else {
        cx.params.min_hoisted_with_stores
    };
    if loads.is_empty() || loads.len() < needed {
        return 0;
    }

    let hoisted: Vec<Hoisted> = loads
        .keys()
        .map(|(addr, offset, op)| {
            let local = cx.next_local;
            cx.next_local += 1;
            cx.new_locals.push(load_ty(*op));
            Hoisted {
                op: *op,
                addr: *addr,
                offset: *offset,
                local,
            }
        })
        .collect();
    // The guarded window: a store landing inside it reloads every hoisted local.
    let lo = hoisted
        .iter()
        .map(|h| h.addr as u64 + h.offset)
        .min()
        .unwrap();
    let hi = hoisted
        .iter()
        .map(|h| h.addr as u64 + h.offset + load_width(h.op) as u64)
        .max()
        .unwrap();

    let Stmt::Loop { body, .. } = &mut seq[at] else {
        unreachable!();
    };
    replace_loads(body, &hoisted);
    if store_count > 0 {
        guard_stores(body, &hoisted, lo, hi, cx);
    }

    let preheader: Vec<Stmt> = hoisted.iter().map(reload_stmt).collect();
    let n = preheader.len();
    seq.splice(at..at, preheader);
    n
}

/// Statements that can write memory beyond a plain store (or run code that does): a loop containing one is not hoisted from.
fn has_barrier(stmts: &[Stmt]) -> bool {
    Stmt::any(stmts, &mut |s| {
        matches!(
            s,
            Stmt::Call { .. }
                | Stmt::CallIndirect { .. }
                | Stmt::MemoryGrow { .. }
                | Stmt::MemoryCopy { .. }
                | Stmt::MemoryFill { .. }
                | Stmt::MemoryInit { .. }
        )
    })
}

fn collect_const_loads(stmts: &[Stmt], mem_min: u64, out: &mut BTreeMap<(u32, u64, LoadOp), ()>) {
    fn expr(e: &Expr, mem_min: u64, out: &mut BTreeMap<(u32, u64, LoadOp), ()>) {
        if let Expr::Load { op, addr, offset } = e {
            if let Expr::I32Const(a) = **addr {
                let end = a as u64 + offset + load_width(*op) as u64;
                // In bounds forever (so it may run early), and comfortably below the 4 GiB edge so the guard arithmetic cannot wrap.
                if end <= mem_min && end <= (1u64 << 32) - 8 {
                    out.insert((a, *offset, *op), ());
                }
            }
        }
    }
    for stmt in stmts {
        for_each_expr(stmt, &mut |e| {
            walk_expr(e, &mut |e| expr(e, mem_min, out));
        });
        for seq in stmt.child_seqs() {
            collect_const_loads(seq, mem_min, out);
        }
    }
}

/// Visit `e` and every nested expression.
fn walk_expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Un(_, a) => walk_expr(a, f),
        Expr::Bin(_, a, b) => {
            walk_expr(a, f);
            walk_expr(b, f);
        }
        Expr::Load { addr, .. } => walk_expr(addr, f),
        Expr::Select { cond, then, els } => {
            walk_expr(cond, f);
            walk_expr(then, f);
            walk_expr(els, f);
        }
        Expr::I32Const(_)
        | Expr::I64Const(_)
        | Expr::F32Const(_)
        | Expr::F64Const(_)
        | Expr::Temp(_)
        | Expr::LocalGet(_)
        | Expr::GlobalGet(_)
        | Expr::MemorySize => {}
    }
}

fn count_stores(stmts: &[Stmt]) -> usize {
    let mut n = 0;
    Stmt::any(stmts, &mut |s| {
        if matches!(s, Stmt::Store { .. }) {
            n += 1;
        }
        false
    });
    n
}

fn replace_loads(stmts: &mut [Stmt], hoisted: &[Hoisted]) {
    fn expr(e: &mut Expr, hoisted: &[Hoisted]) {
        if let Expr::Load { op, addr, offset } = e {
            if let Expr::I32Const(a) = **addr {
                if let Some(h) = hoisted
                    .iter()
                    .find(|h| h.op == *op && h.addr == a && h.offset == *offset)
                {
                    *e = Expr::LocalGet(h.local);
                    return;
                }
            }
        }
        match e {
            Expr::Un(_, a) => expr(a, hoisted),
            Expr::Bin(_, a, b) => {
                expr(a, hoisted);
                expr(b, hoisted);
            }
            Expr::Load { addr, .. } => expr(addr, hoisted),
            Expr::Select { cond, then, els } => {
                expr(cond, hoisted);
                expr(then, hoisted);
                expr(els, hoisted);
            }
            Expr::I32Const(_)
            | Expr::I64Const(_)
            | Expr::F32Const(_)
            | Expr::F64Const(_)
            | Expr::Temp(_)
            | Expr::LocalGet(_)
            | Expr::GlobalGet(_)
            | Expr::MemorySize => {}
        }
    }
    for stmt in stmts {
        for_each_expr_mut(stmt, &mut |e| expr(e, hoisted));
        for seq in stmt.child_seqs_mut() {
            replace_loads(seq, hoisted);
        }
    }
}

/// Rewrite every store in the loop body: spill its address to a temp, store through the temp, then compare the address against the hoisted window and reload on overlap.
/// The guard sits after the store: if the store traps the loop is gone and staleness cannot be observed, and if it succeeds its effective address is below the memory size, so the compare arithmetic cannot wrap.
fn guard_stores(stmts: &mut Vec<Stmt>, hoisted: &[Hoisted], lo: u64, hi: u64, cx: &mut Cx) {
    let mut i = 0;
    while i < stmts.len() {
        for child in stmts[i].child_seqs_mut() {
            guard_stores(child, hoisted, lo, hi, cx);
        }
        if !matches!(stmts[i], Stmt::Store { .. }) {
            i += 1;
            continue;
        }
        let Stmt::Store {
            op,
            addr,
            value,
            offset,
        } = stmts.remove(i)
        else {
            unreachable!();
        };
        let spill = cx.spill();
        let eff = |cx_offset: u64| -> Expr {
            // Effective address of the store; the addr operand wraps to 32 bits, the static offset does not.
            let base = Expr::Bin(
                BinOp::I32And,
                Box::new(Expr::Temp(spill)),
                Box::new(Expr::I32Const(u32::MAX)),
            );
            if cx_offset == 0 {
                base
            } else {
                Expr::Bin(
                    BinOp::I32Add,
                    Box::new(base),
                    Box::new(Expr::I32Const(cx_offset as u32)),
                )
            }
        };
        let overlap = Expr::Bin(
            BinOp::I32And,
            Box::new(Expr::Bin(
                BinOp::I32GtU,
                Box::new(Expr::Bin(
                    BinOp::I32Add,
                    Box::new(eff(offset)),
                    Box::new(Expr::I32Const(store_width(op) as u32)),
                )),
                Box::new(Expr::I32Const(lo as u32)),
            )),
            Box::new(Expr::Bin(
                BinOp::I32LtU,
                Box::new(eff(offset)),
                Box::new(Expr::I32Const(hi as u32)),
            )),
        );
        let guard = Stmt::If {
            label: Label {
                id: cx.next_label,
                referenced: false,
            },
            cond: overlap,
            then: hoisted.iter().map(reload_stmt).collect(),
            els: Vec::new(),
        };
        cx.next_label += 1;
        stmts.splice(
            i..i,
            [
                Stmt::Assign {
                    dst: spill,
                    expr: addr,
                },
                Stmt::Store {
                    op,
                    addr: Expr::Temp(spill),
                    value,
                    offset,
                },
                guard,
            ],
        );
        i += 3;
    }
}

fn reload_stmt(h: &Hoisted) -> Stmt {
    Stmt::LocalSet {
        idx: h.local,
        expr: Expr::Load {
            op: h.op,
            addr: Box::new(Expr::I32Const(h.addr)),
            offset: h.offset,
        },
    }
}

fn load_ty(op: LoadOp) -> ValType {
    match op {
        LoadOp::I32Load
        | LoadOp::I32Load8S
        | LoadOp::I32Load8U
        | LoadOp::I32Load16S
        | LoadOp::I32Load16U => ValType::I32,
        LoadOp::I64Load
        | LoadOp::I64Load8S
        | LoadOp::I64Load8U
        | LoadOp::I64Load16S
        | LoadOp::I64Load16U
        | LoadOp::I64Load32S
        | LoadOp::I64Load32U => ValType::I64,
        LoadOp::F32Load => ValType::F32,
        LoadOp::F64Load => ValType::F64,
    }
}

fn load_width(op: LoadOp) -> u8 {
    match op {
        LoadOp::I32Load8S | LoadOp::I32Load8U | LoadOp::I64Load8S | LoadOp::I64Load8U => 1,
        LoadOp::I32Load16S | LoadOp::I32Load16U | LoadOp::I64Load16S | LoadOp::I64Load16U => 2,
        LoadOp::I32Load | LoadOp::F32Load | LoadOp::I64Load32S | LoadOp::I64Load32U => 4,
        LoadOp::I64Load | LoadOp::F64Load => 8,
    }
}

fn store_width(op: StoreOp) -> u8 {
    match op {
        StoreOp::I32Store8 | StoreOp::I64Store8 => 1,
        StoreOp::I32Store16 | StoreOp::I64Store16 => 2,
        StoreOp::I32Store | StoreOp::F32Store | StoreOp::I64Store32 => 4,
        StoreOp::I64Store | StoreOp::F64Store => 8,
    }
}

fn max_label_id(stmts: &[Stmt]) -> u32 {
    let mut max = 0;
    Stmt::any(stmts, &mut |s| {
        match s {
            Stmt::Block { label, .. }
            | Stmt::Loop { label, .. }
            | Stmt::If { label, .. }
            | Stmt::TryTable { label, .. } => max = max.max(label.id),
            _ => {}
        }
        false
    });
    max
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module_types() -> Vec<FuncType> {
        vec![FuncType {
            params: vec![ValType::I32],
            results: vec![],
        }]
    }

    fn load(addr: u32) -> Expr {
        Expr::Load {
            op: LoadOp::I32Load,
            addr: Box::new(Expr::I32Const(addr)),
            offset: 0,
        }
    }

    fn loop_func(body: Vec<Stmt>) -> Func {
        Func {
            type_idx: 0,
            locals: vec![],
            temps: vec![],
            body: vec![Stmt::Loop {
                label: Label {
                    id: 0,
                    referenced: true,
                },
                body,
            }],
        }
    }

    const TEST_PARAMS: Params = Params {
        min_hoisted_with_stores: 2,
    };

    #[test]
    fn hoists_and_guards() {
        let mut funcs = vec![loop_func(vec![
            Stmt::Store {
                op: StoreOp::I32Store8,
                addr: Expr::LocalGet(0),
                value: Expr::Bin(BinOp::I32Add, Box::new(load(100)), Box::new(load(200))),
                offset: 0,
            },
            Stmt::Br(dewasm_core::ir::BrTarget::Label {
                label: 0,
                is_loop: true,
                assigns: vec![],
            }),
        ])];
        hoist(&mut funcs, &module_types(), Some(65536), &TEST_PARAMS);
        let f = &funcs[0];
        // Two hoisted locals, a preheader of two loads, and the store rewritten to spill + store + guard.
        assert_eq!(f.locals, vec![ValType::I32, ValType::I32]);
        assert_eq!(f.temps.len(), 1);
        assert!(matches!(f.body[0], Stmt::LocalSet { idx: 1, .. }));
        assert!(matches!(f.body[1], Stmt::LocalSet { idx: 2, .. }));
        let Stmt::Loop { body, .. } = &f.body[2] else {
            panic!("loop expected");
        };
        assert!(matches!(body[0], Stmt::Assign { .. }));
        assert!(matches!(
            body[1],
            Stmt::Store {
                addr: Expr::Temp(_),
                ..
            }
        ));
        let Stmt::If { then, .. } = &body[2] else {
            panic!("guard expected, got {:?}", body[2]);
        };
        assert_eq!(then.len(), 2);
    }

    #[test]
    fn skips_loops_with_calls() {
        let mut funcs = vec![loop_func(vec![
            Stmt::LocalSet {
                idx: 0,
                expr: load(100),
            },
            Stmt::Call {
                func: 0,
                args: vec![],
                results: vec![],
            },
        ])];
        hoist(&mut funcs, &module_types(), Some(65536), &TEST_PARAMS);
        assert!(funcs[0].locals.is_empty());
    }

    #[test]
    fn requires_min_size_coverage() {
        let mut funcs = vec![loop_func(vec![Stmt::LocalSet {
            idx: 0,
            expr: load(70000),
        }])];
        hoist(&mut funcs, &module_types(), Some(65536), &TEST_PARAMS);
        assert!(funcs[0].locals.is_empty());
    }

    #[test]
    fn hoists_without_guards_when_no_store() {
        let mut funcs = vec![loop_func(vec![
            Stmt::LocalSet {
                idx: 0,
                expr: load(100),
            },
            Stmt::Br(dewasm_core::ir::BrTarget::Label {
                label: 0,
                is_loop: true,
                assigns: vec![],
            }),
        ])];
        hoist(&mut funcs, &module_types(), Some(65536), &TEST_PARAMS);
        let f = &funcs[0];
        assert_eq!(f.locals, vec![ValType::I32]);
        assert!(matches!(f.body[0], Stmt::LocalSet { idx: 1, .. }));
        let Stmt::Loop { body, .. } = &f.body[1] else {
            panic!("loop expected");
        };
        assert!(matches!(
            body[0],
            Stmt::LocalSet {
                idx: 0,
                expr: Expr::LocalGet(1)
            }
        ));
        assert!(f.temps.is_empty());
    }
}
