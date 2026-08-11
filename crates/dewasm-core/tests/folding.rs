//! Build-time expression folding: assert the IR shapes the func builder produces from small wat inputs (single-use folding, the spill rules, the node-count cap, and what ends up in `Func.temps`).

use dewasm_core::build_module;
use dewasm_core::ir::{BinOp, Expr, Func, Stmt};

/// The node-count cap mirrored from `func.rs` (`MAX_FOLD_SIZE`).
const MAX_FOLD_SIZE: u32 = 32;

fn func(wat: &str, idx: usize) -> Func {
    let bytes = wat::parse_str(wat).expect("wat parses");
    let mut module = build_module(&bytes).expect("module builds");
    module.funcs.remove(idx)
}

/// Nodes in an expression tree.
fn expr_size(e: &Expr) -> u32 {
    match e {
        Expr::Un(_, a) => 1 + expr_size(a),
        Expr::Bin(_, a, b) => 1 + expr_size(a) + expr_size(b),
        Expr::Load { addr, .. } => 1 + expr_size(addr),
        Expr::Select { cond, then, els } => 1 + expr_size(cond) + expr_size(then) + expr_size(els),
        _ => 1,
    }
}

fn walk_exprs(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    fn expr(e: &Expr, f: &mut impl FnMut(&Expr)) {
        f(e);
        match e {
            Expr::Un(_, a) => expr(a, f),
            Expr::Bin(_, a, b) => {
                expr(a, f);
                expr(b, f);
            }
            Expr::Load { addr, .. } => expr(addr, f),
            Expr::Select { cond, then, els } => {
                expr(cond, f);
                expr(then, f);
                expr(els, f);
            }
            _ => {}
        }
    }
    for s in stmts {
        match s {
            Stmt::Assign { expr: e, .. }
            | Stmt::LocalSet { expr: e, .. }
            | Stmt::GlobalSet { expr: e, .. } => expr(e, f),
            Stmt::Store { addr, value, .. } => {
                expr(addr, f);
                expr(value, f);
            }
            Stmt::Return { values } => values.iter().for_each(|e| expr(e, f)),
            Stmt::Call { args, .. } => args.iter().for_each(|e| expr(e, f)),
            Stmt::CallIndirect { index, args, .. } => {
                expr(index, f);
                args.iter().for_each(|e| expr(e, f));
            }
            Stmt::If {
                cond, then, els, ..
            } => {
                expr(cond, f);
                walk_exprs(then, f);
                walk_exprs(els, f);
            }
            Stmt::Block { body, .. } | Stmt::Loop { body, .. } => walk_exprs(body, f),
            _ => {}
        }
    }
}

fn count_assigns(stmts: &[Stmt]) -> usize {
    stmts
        .iter()
        .filter(|s| matches!(s, Stmt::Assign { .. }))
        .count()
}

#[test]
fn single_use_values_fold_into_the_consumer() {
    // add(a, b) folds to a single `return a + b`: no temps, no assigns, the return value is the composed expression.
    let f = func(
        "(module (func (param i32 i32) (result i32)
            local.get 0 local.get 1 i32.add))",
        0,
    );
    assert!(f.temps.is_empty(), "no temps: {:?}", f.temps);
    assert_eq!(count_assigns(&f.body), 0);
    match &f.body[..] {
        [Stmt::Return { values }] => match &values[..] {
            [Expr::Bin(BinOp::I32Add, a, b)] => {
                assert!(matches!(**a, Expr::LocalGet(0)));
                assert!(matches!(**b, Expr::LocalGet(1)));
            }
            other => panic!("unexpected return values: {other:?}"),
        },
        other => panic!("expected a single folded return, got {other:?}"),
    }
}

#[test]
fn local_set_spills_a_pending_that_reads_the_local() {
    // The first `local.get 0` must be spilled before `local.set 0` so it keeps the old value.
    let f = func(
        "(module (func (param i32) (result i32)
            local.get 0
            i32.const 5 local.set 0
            local.get 0
            i32.add))",
        0,
    );
    assert_eq!(f.temps.len(), 1, "the stale read is materialized");
    assert!(
        matches!(
            &f.body[0],
            Stmt::Assign {
                expr: Expr::LocalGet(0),
                ..
            }
        ),
        "first stmt should spill the old local.get: {:?}",
        f.body[0]
    );
}

#[test]
fn a_call_spills_pending_memory_reads() {
    // A pending load cannot cross the call (which may write memory), so it is spilled before the call.
    let f = func(
        "(module
            (memory 1)
            (func $g (result i32) i32.const 7)
            (func (result i32)
                i32.const 0 i32.load
                call $g
                i32.add))",
        1,
    );
    let load_pos = f.body.iter().position(|s| {
        matches!(
            s,
            Stmt::Assign {
                expr: Expr::Load { .. },
                ..
            }
        )
    });
    let call_pos = f.body.iter().position(|s| matches!(s, Stmt::Call { .. }));
    assert!(load_pos.is_some(), "the load is spilled: {:?}", f.body);
    assert!(call_pos.is_some(), "the call is emitted: {:?}", f.body);
    assert!(load_pos < call_pos, "load spill precedes the call");
}

#[test]
fn local_tee_folds_its_value_and_leaves_a_local_read() {
    // tee lowers to a local.set with the value inlined; the value left on the stack folds on into the following add.
    let f = func(
        "(module (func (param i32) (result i32)
            i32.const 5 local.tee 0
            i32.const 1 i32.add))",
        0,
    );
    assert!(f.temps.is_empty(), "tee needs no temp: {:?}", f.temps);
    match &f.body[..] {
        [Stmt::LocalSet { idx: 0, expr }, Stmt::Return { values }] => {
            assert!(matches!(expr, Expr::I32Const(5)), "tee value inlined");
            match &values[..] {
                [Expr::Bin(BinOp::I32Add, a, b)] => {
                    assert!(matches!(**a, Expr::LocalGet(0)), "left is the local read");
                    assert!(matches!(**b, Expr::I32Const(1)));
                }
                other => panic!("unexpected return: {other:?}"),
            }
        }
        other => panic!("unexpected tee lowering: {other:?}"),
    }
}

#[test]
fn select_spills_a_trapping_arm() {
    // The backends lower select to a conditionally-evaluated ternary, so a trapping `then` arm (a load) must be spilled to keep its trap eager.
    let f = func(
        "(module (memory 1)
            (func (param i32) (result i32)
                i32.const 0 i32.load
                i32.const 9
                local.get 0
                select))",
        0,
    );
    assert_eq!(f.temps.len(), 1, "the trapping arm is materialized");
    let mut selects = 0;
    walk_exprs(&f.body, &mut |e| {
        if let Expr::Select { then, els, .. } = e {
            selects += 1;
            assert!(
                !matches!(**then, Expr::Load { .. }),
                "trapping then arm must be spilled"
            );
            assert!(matches!(**then, Expr::Temp(_)), "then is a temp");
            assert!(matches!(**els, Expr::I32Const(9)), "pure els folds");
        }
    });
    assert_eq!(selects, 1, "exactly one select");
}

#[test]
fn deep_expressions_are_capped() {
    // Chain enough adds to exceed the node cap; the builder must spill so no single expression tree grows past MAX_FOLD_SIZE, and temps appear.
    let mut body = String::from("local.get 0\n");
    for _ in 0..40 {
        body.push_str("local.get 0 i32.add\n");
    }
    let wat = format!("(module (func (param i32) (result i32)\n{body}))");
    let f = func(&wat, 0);

    assert!(!f.temps.is_empty(), "capping forces some temps");
    let mut max_size = 0;
    walk_exprs(&f.body, &mut |e| max_size = max_size.max(expr_size(e)));
    assert!(
        max_size <= MAX_FOLD_SIZE,
        "no expression exceeds the cap; largest was {max_size}"
    );
}

#[test]
fn return_value_is_inlined_and_temps_track_materialization() {
    // A value produced by a call is materialized (call results never fold), then that single temp is the inlined return value.
    let f = func(
        "(module
            (func $g (result i32) i32.const 7)
            (func (result i32) call $g))",
        1,
    );
    assert_eq!(f.temps.len(), 1);
    match &f.body[..] {
        [Stmt::Call { results, .. }, Stmt::Return { values }] => {
            assert_eq!(results.len(), 1);
            match &values[..] {
                [Expr::Temp(t)] => assert_eq!(*t, results[0]),
                other => panic!("return should inline the call result temp: {other:?}"),
            }
        }
        other => panic!("unexpected call/return shape: {other:?}"),
    }
}

#[test]
fn a_call_result_spills_a_pending_that_reads_its_slot() {
    // `(one() + two())` is pending at depth 0 and reads the temp at depth 1, which the third call's result then takes: the pending must be spilled before that call, or the addition would read 100 twice.
    let f = func(
        "(module
            (func $one (result i32) i32.const 1)
            (func $two (result i32) i32.const 2)
            (func $big (result i32) i32.const 100)
            (func (result i32)
                call $one call $two i32.add
                call $big i32.add))",
        3,
    );
    let add_spill = f
        .body
        .iter()
        .position(|s| {
            matches!(
                s,
                Stmt::Assign {
                    expr: Expr::Bin(BinOp::I32Add, ..),
                    ..
                }
            )
        })
        .expect("the pending addition is spilled");
    let calls_before = f.body[..add_spill]
        .iter()
        .filter(|s| matches!(s, Stmt::Call { .. }))
        .count();
    assert_eq!(
        calls_before, 2,
        "the spill sits between the second and third call: {:?}",
        f.body
    );
}

#[test]
fn a_spill_flushes_a_pending_that_reads_the_reused_slot() {
    // Same shape without a call: the store spills the pending load into the temp at depth 1, which the pending addition at depth 0 reads, so that addition has to be spilled first.
    let f = func(
        "(module
            (memory 1)
            (func $one (result i32) i32.const 1)
            (func $two (result i32) i32.const 2)
            (func (result i32)
                call $one call $two i32.add
                i32.const 0 i32.load
                i32.const 4 i32.const 7 i32.store
                i32.add))",
        2,
    );
    let add_spill = f
        .body
        .iter()
        .position(|s| {
            matches!(
                s,
                Stmt::Assign {
                    expr: Expr::Bin(BinOp::I32Add, ..),
                    ..
                }
            )
        })
        .expect("the pending addition is spilled");
    let load_spill = f
        .body
        .iter()
        .position(|s| {
            matches!(
                s,
                Stmt::Assign {
                    expr: Expr::Load { .. },
                    ..
                }
            )
        })
        .expect("the pending load is spilled by the store");
    assert!(
        add_spill < load_spill,
        "the addition is read out before the load overwrites its operand slot: {:?}",
        f.body
    );
}
