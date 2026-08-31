//! Rewrite a tail call to the function's own index into a loop.
//!
//! A `return_call` to self is a jump back to the top with new arguments, which is what a loop is.
//! Written that way it needs no trampoline at all, and a function whose tail calls are all self-calls leaves the tail-caller set entirely, so it also loses the body/entry split and the parked-call machinery.
//!
//! This is not the mechanism tail calls are lowered by: it covers direct self-recursion only, and the conformance suite's `even`/`odd` pair is mutual, so the trampoline stays for everything else.

use std::collections::BTreeSet;

use dewasm_core::ir::{BrTarget, Expr, Func, FuncType, Label, Stmt, Temp, ValType};

use crate::terminates;

/// Rewrite every self tail call in `funcs` into a branch back to a loop wrapping the function's body.
/// `num_imported` is the offset between a position in `funcs` and its function index.
pub fn rewrite(funcs: &mut [Func], types: &[FuncType], num_imported: u32) {
    for (i, func) in funcs.iter_mut().enumerate() {
        let idx = num_imported + i as u32;
        let ty = &types[func.type_idx as usize];
        if eligible(func, idx) {
            rewrite_func(func, ty, idx);
        }
    }
}

/// Whether wrapping `func`'s body in a loop preserves its meaning.
///
/// Two conditions beyond having a self tail call at all.
/// The body must terminate, because a body that can fall off its end would spin in the loop rather than return.
/// And every declared local must have a constant zero, because a fresh call zeroes the locals and the loop has to do the same; a reference-typed local has no such expression in the IR.
fn eligible(func: &Func, idx: u32) -> bool {
    Stmt::any(
        &func.body,
        &mut |s| matches!(s, Stmt::ReturnCall { func, .. } if *func == idx),
    ) && terminates(&func.body)
        && func.locals.iter().all(|t| zero(*t).is_some())
}

/// The constant a declared local starts a call at, or `None` for a type the IR has no constant for.
fn zero(ty: ValType) -> Option<Expr> {
    match ty {
        ValType::I32 => Some(Expr::I32Const(0)),
        ValType::I64 => Some(Expr::I64Const(0)),
        ValType::F32 => Some(Expr::F32Const(0)),
        ValType::F64 => Some(Expr::F64Const(0)),
        ValType::FuncRef | ValType::ExnRef => None,
    }
}

fn rewrite_func(func: &mut Func, ty: &FuncType, idx: u32) {
    let label = Label {
        id: next_label(&func.body),
        referenced: true,
    };
    let resets: Vec<Stmt> = func
        .locals
        .iter()
        .enumerate()
        .map(|(k, t)| Stmt::LocalSet {
            idx: (ty.params.len() + k) as u32,
            expr: zero(*t).expect("eligible() rejected the types without one"),
        })
        .collect();
    let mut cx = Cx {
        idx,
        params: &ty.params,
        label: label.id,
        next_depth: func
            .temps
            .iter()
            .map(|t| t.depth)
            .max()
            .map_or(0, |d| d + 1),
        added: Vec::new(),
        resets: &resets,
    };
    let mut body = std::mem::take(&mut func.body);
    replace(&mut body, &mut cx);
    func.temps.extend(cx.added);
    func.temps.sort();
    func.temps.dedup();
    func.body = vec![Stmt::Loop { label, body }];
}

struct Cx<'a> {
    idx: u32,
    params: &'a [ValType],
    label: u32,
    next_depth: u32,
    added: Vec<Temp>,
    resets: &'a [Stmt],
}

fn replace(stmts: &mut Vec<Stmt>, cx: &mut Cx) {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for mut stmt in stmts.drain(..) {
        for child in stmt.child_seqs_mut() {
            replace(child, cx);
        }
        match stmt {
            Stmt::ReturnCall { func, args } if func == cx.idx => {
                // The arguments land in fresh temps before any parameter is written: an argument may read a parameter an earlier assignment would already have overwritten.
                let mut slots = Vec::with_capacity(args.len());
                for (arg, param) in args.into_iter().zip(cx.params) {
                    let slot = Temp {
                        depth: cx.next_depth,
                        ty: *param,
                    };
                    cx.next_depth += 1;
                    cx.added.push(slot);
                    out.push(Stmt::Assign {
                        dst: slot,
                        expr: arg,
                    });
                    slots.push(slot);
                }
                for (i, slot) in slots.into_iter().enumerate() {
                    out.push(Stmt::LocalSet {
                        idx: i as u32,
                        expr: Expr::Temp(slot),
                    });
                }
                out.extend(cx.resets.iter().cloned());
                out.push(Stmt::Br(BrTarget::Label {
                    label: cx.label,
                    is_loop: true,
                    assigns: Vec::new(),
                }));
            }
            other => out.push(other),
        }
    }
    *stmts = out;
}

/// One past the largest label id the body uses, so the new frame cannot collide with an existing one.
fn next_label(stmts: &[Stmt]) -> u32 {
    let mut ids: BTreeSet<u32> = BTreeSet::new();
    Stmt::any(stmts, &mut |s| {
        match s {
            Stmt::Block { label, .. }
            | Stmt::Loop { label, .. }
            | Stmt::If { label, .. }
            | Stmt::TryTable { label, .. } => {
                ids.insert(label.id);
            }
            _ => {}
        }
        false
    });
    ids.iter().next_back().map_or(0, |m| m + 1)
}
