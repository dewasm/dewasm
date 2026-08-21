//! Fuse the byte-scatter store idiom into one 32-bit store behind a runtime precondition.
//!
//! Portable C writes a 32-bit value byte by byte (`dest[i] = v >> (i * 8)`), and compiled to wasm that survives as a four-iteration loop of `store8(base + i, word >> shift)` with `shift` stepping by 8.
//! Proving the trip count statically needs path-sensitive bit reasoning (the exit condition folds a flag bit), so the pass proves nothing: it recognizes the loop's shape and emits a runtime-guarded fast path, `if flag is odd and the inductions start at zero and the base is safely in bounds, one 32-bit store; else the original loop`.
//! The bounds clause keeps the memory-edge case on the original loop, whose byte-at-a-time trap (with the partial writes it leaves visible) stays exact.

use dewasm_core::ir::{BinOp, BrTarget, Expr, Func, Label, Stmt, StoreOp};

/// Rewrite `funcs` in place.
pub fn fuse_byte_scatter(funcs: &mut [Func]) {
    for func in funcs {
        let mut next_label = max_label_id(&func.body) + 1;
        walk_seq(&mut func.body, &mut next_label);
    }
}

fn walk_seq(seq: &mut [Stmt], next_label: &mut u32) {
    for stmt in seq.iter_mut() {
        for child in stmt.child_seqs_mut() {
            walk_seq(child, next_label);
        }
        let Stmt::Loop { label, body } = stmt else {
            continue;
        };
        let Some(m) = match_scatter(label.id, body) else {
            continue;
        };
        let fast = vec![
            Stmt::Store {
                op: StoreOp::I32Store,
                addr: Expr::LocalGet(m.base),
                value: Expr::LocalGet(m.word),
                offset: 0,
            },
            Stmt::LocalSet {
                idx: m.shift,
                expr: Expr::I32Const(32),
            },
            Stmt::LocalSet {
                idx: m.done,
                expr: Expr::I32Const(1),
            },
            Stmt::LocalSet {
                idx: m.next_idx,
                expr: Expr::I32Const(4),
            },
            Stmt::LocalSet {
                idx: m.idx,
                expr: Expr::I32Const(4),
            },
        ];
        let cond = and(
            and(
                // The flag's low bit decides the exit; odd means the loop runs exactly four iterations.
                eq(
                    Expr::Bin(
                        BinOp::I32And,
                        Box::new(Expr::LocalGet(m.flag)),
                        Box::new(Expr::I32Const(1)),
                    ),
                    1,
                ),
                eq(Expr::LocalGet(m.idx), 0),
            ),
            and(
                eq(Expr::LocalGet(m.shift), 0),
                // The fast path needs the whole 32-bit store in bounds; comparing pages keeps the arithmetic inside 32 bits and sends a base in the memory's last page to the original loop, whose byte-at-a-time trap behavior (and the partial writes it leaves visible) must stay exact.
                Expr::Bin(
                    BinOp::I32LtU,
                    Box::new(Expr::Bin(
                        BinOp::I32ShrU,
                        Box::new(Expr::LocalGet(m.base)),
                        Box::new(Expr::I32Const(16)),
                    )),
                    Box::new(Expr::MemorySize),
                ),
            ),
        );
        let original = std::mem::replace(
            stmt,
            Stmt::If {
                label: Label {
                    id: *next_label,
                    referenced: false,
                },
                cond,
                then: fast,
                els: Vec::new(),
            },
        );
        *next_label += 1;
        let Stmt::If { els, .. } = stmt else {
            unreachable!();
        };
        els.push(original);
    }
}

fn and(a: Expr, b: Expr) -> Expr {
    Expr::Bin(BinOp::I32And, Box::new(a), Box::new(b))
}

fn eq(a: Expr, c: u32) -> Expr {
    Expr::Bin(BinOp::I32Eq, Box::new(a), Box::new(Expr::I32Const(c)))
}

/// The matched scatter loop's locals.
struct Scatter {
    base: u32,
    idx: u32,
    word: u32,
    shift: u32,
    /// Holds `idx == 3`, consumed by the exit condition.
    done: u32,
    /// Holds `idx + 1`, copied back into `idx` (and read after the loop by the code advancing the base).
    next_idx: u32,
    flag: u32,
}

/// Match the six-statement scatter body exactly; anything else (including any other branch, which wasm scoping confines to the body) misses.
fn match_scatter(loop_label: u32, body: &[Stmt]) -> Option<Scatter> {
    let [Stmt::Store {
        op: StoreOp::I32Store8,
        addr,
        value,
        offset: 0,
    }, Stmt::LocalSet {
        idx: s2,
        expr: e_shift,
    }, Stmt::LocalSet {
        idx: s3,
        expr: e_done,
    }, Stmt::LocalSet {
        idx: s4,
        expr: e_next,
    }, Stmt::LocalSet {
        idx: s5,
        expr: e_copy,
    }, Stmt::BrIf { cond, target }] = body
    else {
        return None;
    };
    let BrTarget::Label {
        label,
        is_loop: true,
        assigns,
    } = target
    else {
        return None;
    };
    if *label != loop_label || !assigns.is_empty() {
        return None;
    }

    // shift += 8
    let shift = *s2;
    let (a, b) = add_operands(e_shift)?;
    if !(local(a) == Some(shift) && konst(b) == Some(8)
        || local(b) == Some(shift) && konst(a) == Some(8))
    {
        return None;
    }
    // done = idx == 3
    let done = *s3;
    let Expr::Bin(BinOp::I32Eq, a, b) = e_done else {
        return None;
    };
    let idx = match (local(a), konst(b), local(b), konst(a)) {
        (Some(i), Some(3), _, _) | (_, _, Some(i), Some(3)) => i,
        _ => return None,
    };
    // next_idx = idx + 1; idx = next_idx
    let next_idx = *s4;
    let (a, b) = add_operands(e_next)?;
    if !(local(a) == Some(idx) && konst(b) == Some(1)
        || local(b) == Some(idx) && konst(a) == Some(1))
    {
        return None;
    }
    if *s5 != idx || local(e_copy) != Some(next_idx) {
        return None;
    }
    // store8(base + idx, word >> shift) with the count possibly wrapped in `& 31`
    let (a, b) = add_operands(addr)?;
    let base = match (local(a), local(b)) {
        (Some(x), Some(y)) if y == idx => x,
        (Some(x), Some(y)) if x == idx => y,
        _ => return None,
    };
    let Expr::Bin(BinOp::I32ShrU, w, count) = value else {
        return None;
    };
    let word = local(w)?;
    let count_local = match &**count {
        Expr::LocalGet(l) => *l,
        Expr::Bin(BinOp::I32And, a, b) => match (local(a), konst(b), local(b), konst(a)) {
            (Some(l), Some(31), _, _) | (_, _, Some(l), Some(31)) => l,
            _ => return None,
        },
        _ => return None,
    };
    if count_local != shift {
        return None;
    }
    // exit: (flag & done) != 1
    let Expr::Bin(BinOp::I32Ne, a, b) = cond else {
        return None;
    };
    let masked = match (konst(b), konst(a)) {
        (Some(1), _) => a,
        (_, Some(1)) => b,
        _ => return None,
    };
    let Expr::Bin(BinOp::I32And, x, y) = &**masked else {
        return None;
    };
    let flag = match (local(x), local(y)) {
        (Some(f), Some(t)) if t == done => f,
        (Some(t), Some(f)) if t == done => f,
        _ => return None,
    };

    // The written locals must be pairwise distinct and never the read-only ones, or the exit values would clobber an input.
    let written = [shift, done, next_idx, idx];
    for (n, w) in written.iter().enumerate() {
        if written[..n].contains(w) || [base, word, flag].contains(w) {
            return None;
        }
    }
    Some(Scatter {
        base,
        idx,
        word,
        shift,
        done,
        next_idx,
        flag,
    })
}

fn local(e: &Expr) -> Option<u32> {
    match e {
        Expr::LocalGet(i) => Some(*i),
        _ => None,
    }
}

fn konst(e: &Expr) -> Option<u32> {
    match e {
        Expr::I32Const(c) => Some(*c),
        _ => None,
    }
}

fn add_operands(e: &Expr) -> Option<(&Expr, &Expr)> {
    match e {
        Expr::Bin(BinOp::I32Add, a, b) => Some((a, b)),
        _ => None,
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
    use dewasm_core::ir::{FuncType, ValType};

    fn scatter_loop() -> Stmt {
        // Locals: 0 base, 1 idx, 2 word, 3 shift, 4 done, 5 next_idx, 6 flag.
        Stmt::Loop {
            label: Label {
                id: 0,
                referenced: true,
            },
            body: vec![
                Stmt::Store {
                    op: StoreOp::I32Store8,
                    addr: Expr::Bin(
                        BinOp::I32Add,
                        Box::new(Expr::LocalGet(0)),
                        Box::new(Expr::LocalGet(1)),
                    ),
                    value: Expr::Bin(
                        BinOp::I32ShrU,
                        Box::new(Expr::LocalGet(2)),
                        Box::new(Expr::Bin(
                            BinOp::I32And,
                            Box::new(Expr::LocalGet(3)),
                            Box::new(Expr::I32Const(31)),
                        )),
                    ),
                    offset: 0,
                },
                Stmt::LocalSet {
                    idx: 3,
                    expr: Expr::Bin(
                        BinOp::I32Add,
                        Box::new(Expr::LocalGet(3)),
                        Box::new(Expr::I32Const(8)),
                    ),
                },
                Stmt::LocalSet {
                    idx: 4,
                    expr: Expr::Bin(
                        BinOp::I32Eq,
                        Box::new(Expr::LocalGet(1)),
                        Box::new(Expr::I32Const(3)),
                    ),
                },
                Stmt::LocalSet {
                    idx: 5,
                    expr: Expr::Bin(
                        BinOp::I32Add,
                        Box::new(Expr::LocalGet(1)),
                        Box::new(Expr::I32Const(1)),
                    ),
                },
                Stmt::LocalSet {
                    idx: 1,
                    expr: Expr::LocalGet(5),
                },
                Stmt::BrIf {
                    cond: Expr::Bin(
                        BinOp::I32Ne,
                        Box::new(Expr::Bin(
                            BinOp::I32And,
                            Box::new(Expr::LocalGet(6)),
                            Box::new(Expr::LocalGet(4)),
                        )),
                        Box::new(Expr::I32Const(1)),
                    ),
                    target: BrTarget::Label {
                        label: 0,
                        is_loop: true,
                        assigns: vec![],
                    },
                },
            ],
        }
    }

    #[test]
    fn fuses_the_scatter_idiom() {
        let mut funcs = vec![Func {
            type_idx: 0,
            locals: vec![ValType::I32; 7],
            temps: vec![],
            body: vec![scatter_loop()],
        }];
        let _types = [FuncType {
            params: vec![],
            results: vec![],
        }];
        fuse_byte_scatter(&mut funcs);
        let Stmt::If { then, els, .. } = &funcs[0].body[0] else {
            panic!("guarded fast path expected, got {:?}", funcs[0].body[0]);
        };
        assert!(matches!(
            then[0],
            Stmt::Store {
                op: StoreOp::I32Store,
                ..
            }
        ));
        assert_eq!(then.len(), 5);
        assert!(matches!(els[0], Stmt::Loop { .. }));
    }

    #[test]
    fn leaves_other_loops_alone() {
        let mut funcs = vec![Func {
            type_idx: 0,
            locals: vec![ValType::I32; 7],
            temps: vec![],
            body: vec![Stmt::Loop {
                label: Label {
                    id: 0,
                    referenced: true,
                },
                body: vec![Stmt::LocalSet {
                    idx: 1,
                    expr: Expr::I32Const(0),
                }],
            }],
        }];
        fuse_byte_scatter(&mut funcs);
        assert!(matches!(funcs[0].body[0], Stmt::Loop { .. }));
    }
}
