//! Translate a wasm function body (stack machine) into structured IR with
//! build-time expression folding.
//!
//! The operand stack is modeled as a stack of `Slot`s. Each pushed value is
//! kept as a *pending* expression (with its read/trap `Effects` and node
//! count) rather than being materialized into a temp immediately. A pending is
//! spilled to a temp (`sN = <expr>`) only when a later statement's effects
//! could be observed by it, when it crosses a control-flow boundary, or when a
//! consumer would grow it past `MAX_FOLD_SIZE` nodes. Single-use values thus
//! fold directly into their consumer, cutting the statement count for every
//! backend. Evaluation order and trap points are preserved by the spill
//! discipline. Dead code after an unconditional branch is skipped while
//! tracking block nesting only.

use std::collections::BTreeSet;

use anyhow::Result;
use wasmparser::{BlockType, FunctionBody, Operator};

use crate::feature::Feature;
use crate::ir::{self, BinOp, BrTarget, Expr, Label, LoadOp, Stmt, StoreOp, Temp, UnOp, ValType};
use crate::module::{unsupported, val_type};

/// Node-count cap for a folded expression tree. Once a consumer would build a
/// tree larger than this, its operands are spilled to temps first. The cap
/// keeps the generated expressions shallow enough not to blow the recursive
/// descent parser of any target language (Ruby/Python and friends parse
/// expressions recursively and have finite stack budgets), and bounds the
/// worst-case textual blow-up of backends whose inline lowerings duplicate an
/// operand.
const MAX_FOLD_SIZE: u32 = 32;

/// Whether values fold. Kept as a knob so the pending-stack machinery can be
/// introduced without behavioral change (every push materializes immediately),
/// then flipped on once verified. Always `true` in shipped builds.
const FOLD: bool = false;

/// Side effects a pending expression carries, used to decide when it must be
/// spilled before a later statement so it cannot observe that statement's
/// effect (or so its own trap fires at the right point).
#[derive(Clone, Copy, Default)]
struct Effects {
    /// Bitmask of read locals with index < 64.
    locals: u64,
    /// A local with index >= 64 is read (a conservative catch-all so the
    /// bitmask stays a single word).
    any_high_local: bool,
    /// Reads a global.
    globals: bool,
    /// Reads memory (a load) or `memory.size`.
    memory: bool,
    /// May trap (a load, a divide/remainder, or a non-saturating float->int
    /// truncation).
    trap: bool,
}

impl Effects {
    fn local(idx: u32) -> Self {
        let mut e = Effects::default();
        e.add_local(idx);
        e
    }

    fn add_local(&mut self, idx: u32) {
        if idx < 64 {
            self.locals |= 1 << idx;
        } else {
            self.any_high_local = true;
        }
    }

    fn merge(&mut self, o: Effects) {
        self.locals |= o.locals;
        self.any_high_local |= o.any_high_local;
        self.globals |= o.globals;
        self.memory |= o.memory;
        self.trap |= o.trap;
    }

    fn reads_local(&self, idx: u32) -> bool {
        if idx < 64 {
            self.locals & (1 << idx) != 0
        } else {
            self.any_high_local
        }
    }
}

/// A value that has been pushed but not yet materialized into a temp.
struct Pending {
    expr: Expr,
    fx: Effects,
    /// Node count of `expr` (see `MAX_FOLD_SIZE`).
    size: u32,
}

/// One operand-stack slot: its type, and a pending expression when the value
/// has not been spilled to a temp yet.
struct Slot {
    ty: ValType,
    pending: Option<Pending>,
}

pub struct FuncBuilder<'a> {
    module: &'a ir::Module,
    /// Type indices of all defined functions (the code section is being
    /// translated, so `module.funcs` is still incomplete).
    defined_func_types: &'a [u32],
    type_idx: u32,
    /// params ++ declared locals
    all_locals: Vec<ValType>,
    num_params: usize,
    stack: Vec<Slot>,
    frames: Vec<Frame>,
    temps: BTreeSet<Temp>,
    next_label: u32,
    result: Option<ir::Func>,
}

enum FrameKind {
    Func,
    Block,
    Loop,
    If {
        cond: Expr,
        /// Set when the `else` keyword is reached.
        then_body: Option<Vec<Stmt>>,
    },
}

struct Frame {
    kind: FrameKind,
    label_id: u32,
    /// Stack height at entry, with block params below it kept in place.
    base: usize,
    params: Vec<ValType>,
    results: Vec<ValType>,
    /// Code from here to the end of the frame cannot be reached.
    unreachable: bool,
    /// The whole frame is inside dead code; emits nothing.
    entered_dead: bool,
    /// Some br targets this frame's label.
    referenced: bool,
    stmts: Vec<Stmt>,
}

impl<'a> FuncBuilder<'a> {
    pub fn new(module: &'a ir::Module, defined_func_types: &'a [u32], type_idx: u32) -> Self {
        let ty = &module.types[type_idx as usize];
        FuncBuilder {
            module,
            defined_func_types,
            type_idx,
            all_locals: ty.params.clone(),
            num_params: ty.params.len(),
            stack: Vec::new(),
            frames: Vec::new(),
            temps: BTreeSet::new(),
            next_label: 0,
            result: None,
        }
    }

    pub fn translate(mut self, body: &FunctionBody<'_>) -> Result<ir::Func> {
        for local in body.get_locals_reader()? {
            let (count, ty) = local?;
            let ty = val_type(ty)?;
            for _ in 0..count {
                self.all_locals.push(ty);
            }
        }

        let func_results = self.module.types[self.type_idx as usize].results.clone();
        self.push_frame(FrameKind::Func, Vec::new(), func_results);

        let mut reader = body.get_operators_reader()?;
        while !self.frames.is_empty() {
            let op = reader.read()?;
            self.op(op)?;
        }

        Ok(self.result.take().expect("function body finished"))
    }

    // ---- stack / frame helpers -------------------------------------------

    fn cur(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("frame stack is not empty")
    }

    fn emit(&mut self, stmt: Stmt) {
        self.cur().stmts.push(stmt);
    }

    /// Push a materialized value: allocate a temp for the top slot and record
    /// it in `self.temps`. Used for values that are never folded (call
    /// results, `memory.grow`) and by `spill`.
    fn push_temp(&mut self, ty: ValType) -> Temp {
        let temp = Temp {
            depth: self.stack.len() as u32,
            ty,
        };
        self.temps.insert(temp);
        self.stack.push(Slot { ty, pending: None });
        temp
    }

    /// Push a folded (pending) value. Does not touch `self.temps`.
    fn push_pending(&mut self, ty: ValType, expr: Expr, fx: Effects, size: u32) {
        self.stack.push(Slot {
            ty,
            pending: Some(Pending { expr, fx, size }),
        });
        if !FOLD {
            let top = self.stack.len() - 1;
            self.spill(top);
        }
    }

    /// Pop the top slot as an expression: its pending expression if any, else
    /// a reference to the temp it was materialized into.
    fn pop_expr(&mut self) -> (Expr, Effects, u32) {
        let slot = self.stack.pop().expect("value stack is not empty");
        match slot.pending {
            Some(p) => (p.expr, p.fx, p.size),
            None => (
                Expr::Temp(Temp {
                    depth: self.stack.len() as u32,
                    ty: slot.ty,
                }),
                Effects::default(),
                1,
            ),
        }
    }

    fn slot_size(&self, idx: usize) -> u32 {
        self.stack[idx].pending.as_ref().map_or(1, |p| p.size)
    }

    fn slot_traps(&self, idx: usize) -> bool {
        self.stack[idx].pending.as_ref().is_some_and(|p| p.fx.trap)
    }

    /// Materialize the pending value at stack index `idx` into its temp,
    /// emitting the assignment. A no-op if already materialized.
    fn spill(&mut self, idx: usize) {
        let ty = self.stack[idx].ty;
        if let Some(p) = self.stack[idx].pending.take() {
            let temp = Temp {
                depth: idx as u32,
                ty,
            };
            self.temps.insert(temp);
            self.emit(Stmt::Assign {
                dst: temp,
                expr: p.expr,
            });
        }
    }

    /// Spill every pending whose effects match `pred`, in ascending depth
    /// order (= wasm evaluation order, so their statements and traps land in
    /// the right sequence).
    fn spill_if(&mut self, pred: impl Fn(&Effects) -> bool) {
        for idx in 0..self.stack.len() {
            let hit = match &self.stack[idx].pending {
                Some(p) => pred(&p.fx),
                None => false,
            };
            if hit {
                self.spill(idx);
            }
        }
    }

    /// Spill the whole stack (used at control-flow boundaries).
    fn spill_all(&mut self) {
        for idx in 0..self.stack.len() {
            self.spill(idx);
        }
    }

    fn top_ty(&self) -> ValType {
        self.stack.last().expect("value stack is not empty").ty
    }

    fn un(&mut self, op: UnOp, res: ValType) {
        let top = self.stack.len() - 1;
        if self.slot_size(top) + 1 > MAX_FOLD_SIZE {
            self.spill(top);
        }
        let (a, mut fx, size) = self.pop_expr();
        if un_traps(op) {
            fx.trap = true;
        }
        self.push_pending(res, Expr::Un(op, Box::new(a)), fx, size + 1);
    }

    fn bin(&mut self, op: BinOp, res: ValType) {
        let b_idx = self.stack.len() - 1;
        let a_idx = b_idx - 1;
        if self.slot_size(a_idx) + self.slot_size(b_idx) + 1 > MAX_FOLD_SIZE {
            self.spill(a_idx);
            self.spill(b_idx);
        }
        let (b, fxb, sb) = self.pop_expr();
        let (a, fxa, sa) = self.pop_expr();
        let mut fx = fxa;
        fx.merge(fxb);
        if bin_traps(op) {
            fx.trap = true;
        }
        self.push_pending(
            res,
            Expr::Bin(op, Box::new(a), Box::new(b)),
            fx,
            sa + sb + 1,
        );
    }

    fn load(&mut self, op: LoadOp, res: ValType, memarg: &wasmparser::MemArg) {
        let top = self.stack.len() - 1;
        if self.slot_size(top) + 1 > MAX_FOLD_SIZE {
            self.spill(top);
        }
        let (addr, mut fx, size) = self.pop_expr();
        fx.memory = true;
        fx.trap = true;
        self.push_pending(
            res,
            Expr::Load {
                op,
                addr: Box::new(addr),
                offset: memarg.offset,
            },
            fx,
            size + 1,
        );
    }

    fn store(&mut self, op: StoreOp, memarg: &wasmparser::MemArg) {
        let (value, _, _) = self.pop_expr();
        let (addr, _, _) = self.pop_expr();
        // The store writes memory; earlier memory reads and pending traps must
        // be resolved first.
        self.spill_if(|fx| fx.memory || fx.trap);
        self.emit(Stmt::Store {
            op,
            addr,
            value,
            offset: memarg.offset,
        });
    }

    fn select(&mut self) {
        // Stack: [then, els, cond] with cond on top. The backends lower Select
        // to a conditionally-evaluated ternary, so a trapping then/els arm
        // (which wasm evaluates eagerly) must be spilled to keep its trap. The
        // cond folds freely.
        let n = self.stack.len();
        let cond_i = n - 1;
        let els_i = n - 2;
        let then_i = n - 3;
        let ty = self.stack[then_i].ty;
        let total = self.slot_size(then_i) + self.slot_size(els_i) + self.slot_size(cond_i) + 1;
        if total > MAX_FOLD_SIZE {
            self.spill(then_i);
            self.spill(els_i);
            self.spill(cond_i);
        } else {
            if self.slot_traps(then_i) {
                self.spill(then_i);
            }
            if self.slot_traps(els_i) {
                self.spill(els_i);
            }
        }
        let (cond, fxc, sc) = self.pop_expr();
        let (els, fxe, se) = self.pop_expr();
        let (then, fxt, st) = self.pop_expr();
        let mut fx = fxt;
        fx.merge(fxe);
        fx.merge(fxc);
        self.push_pending(
            ty,
            Expr::Select {
                cond: Box::new(cond),
                then: Box::new(then),
                els: Box::new(els),
            },
            fx,
            st + se + sc + 1,
        );
    }

    fn push_frame(&mut self, kind: FrameKind, params: Vec<ValType>, results: Vec<ValType>) {
        let label_id = self.next_label;
        self.next_label += 1;
        self.frames.push(Frame {
            kind,
            label_id,
            base: self.stack.len() - params.len(),
            params,
            results,
            unreachable: false,
            entered_dead: false,
            referenced: false,
            stmts: Vec::new(),
        });
    }

    fn block_type(&self, bt: BlockType) -> Result<(Vec<ValType>, Vec<ValType>)> {
        Ok(match bt {
            BlockType::Empty => (Vec::new(), Vec::new()),
            BlockType::Type(ty) => (Vec::new(), vec![val_type(ty)?]),
            BlockType::FuncType(idx) => {
                let ty = &self.module.types[idx as usize];
                (ty.params.clone(), ty.results.clone())
            }
        })
    }

    fn func_type_of(&self, func_idx: u32) -> &ir::FuncType {
        let idx = func_idx as usize;
        let imports = self.module.imported_funcs.len();
        let ty_idx = if idx < imports {
            self.module.imported_funcs[idx].type_idx
        } else {
            self.defined_func_types[idx - imports]
        };
        &self.module.types[ty_idx as usize]
    }

    /// Resolve a branch depth into a target, computing the moves from the
    /// current stack top into the target frame's result (or loop param)
    /// slots. Marks the target label as referenced. The caller must have
    /// spilled the stack first, so the sources read from materialized temps.
    fn branch_target(&mut self, relative_depth: u32) -> BrTarget {
        let idx = self.frames.len() - 1 - relative_depth as usize;
        let (arity_tys, base, is_loop, is_func, label_id) = {
            let frame = &self.frames[idx];
            match frame.kind {
                FrameKind::Func => (frame.results.clone(), 0, false, true, 0),
                FrameKind::Loop => (
                    frame.params.clone(),
                    frame.base,
                    true,
                    false,
                    frame.label_id,
                ),
                _ => (
                    frame.results.clone(),
                    frame.base,
                    false,
                    false,
                    frame.label_id,
                ),
            }
        };
        let arity = arity_tys.len();
        if is_func {
            let values = (0..arity)
                .map(|i| {
                    Expr::Temp(Temp {
                        depth: (self.stack.len() - arity + i) as u32,
                        ty: arity_tys[i],
                    })
                })
                .collect();
            return BrTarget::Return { values };
        }
        let mut assigns = Vec::new();
        for (i, ty) in arity_tys.iter().enumerate() {
            let src = Temp {
                depth: (self.stack.len() - arity + i) as u32,
                ty: *ty,
            };
            let dst = Temp {
                depth: (base + i) as u32,
                ty: *ty,
            };
            if src != dst {
                self.temps.insert(dst);
                assigns.push((dst, src));
            }
        }
        self.frames[idx].referenced = true;
        BrTarget::Label {
            label: label_id,
            is_loop,
            assigns,
        }
    }

    fn handle_else(&mut self) {
        // Materialize the then-branch's fallthrough values (into the frame's
        // result slots) before capturing its body. Skipped when the then
        // branch ended unreachable (its stack shape is stale).
        if !self.cur().unreachable {
            self.spill_all();
        }
        let base = self.cur().base;
        let params = self.cur().params.clone();
        let then_body = std::mem::take(&mut self.cur().stmts);
        match &mut self.cur().kind {
            FrameKind::If {
                then_body: slot, ..
            } => *slot = Some(then_body),
            _ => unreachable!("else outside of if"),
        }
        self.stack.truncate(base);
        for ty in params {
            self.push_temp(ty);
        }
        self.cur().unreachable = false;
    }

    fn handle_end(&mut self) {
        // Spill the frame's live values into its own body before it is popped
        // (so control-boundary-crossing temps are materialized). The function
        // frame folds its fallthrough return instead.
        let is_func = matches!(self.cur().kind, FrameKind::Func);
        if !self.cur().entered_dead && !self.cur().unreachable && !is_func {
            self.spill_all();
        }

        let frame = self.frames.pop().expect("frame stack is not empty");
        if frame.entered_dead {
            // The whole frame was dead code; the parent stays unreachable and
            // the stack was never touched.
            return;
        }

        match frame.kind {
            FrameKind::Func => {
                let mut body = frame.stmts;
                if !frame.unreachable {
                    let arity = frame.results.len();
                    let mut values = vec![Expr::I32Const(0); arity];
                    for i in (0..arity).rev() {
                        values[i] = self.pop_expr().0;
                    }
                    body.push(Stmt::Return { values });
                }
                let mut temps: Vec<Temp> = self.temps.iter().copied().collect();
                temps.sort();
                self.result = Some(ir::Func {
                    type_idx: self.type_idx,
                    locals: self.all_locals[self.num_params..].to_vec(),
                    temps,
                    body,
                });
            }
            kind => {
                // Fallthrough values already live at the result slots; formally
                // rebuild the stack shape for the parent frame.
                self.stack.truncate(frame.base);
                for ty in &frame.results {
                    self.push_temp(*ty);
                }
                match kind {
                    FrameKind::Block => {
                        let label = Label {
                            id: frame.label_id,
                            referenced: frame.referenced,
                        };
                        if frame.referenced {
                            self.emit(Stmt::Block {
                                label,
                                body: frame.stmts,
                            });
                        } else {
                            // Nobody branches here: splice the body into the parent.
                            self.cur().stmts.extend(frame.stmts);
                        }
                    }
                    FrameKind::Loop => {
                        let label = Label {
                            id: frame.label_id,
                            referenced: frame.referenced,
                        };
                        if frame.referenced {
                            self.emit(Stmt::Loop {
                                label,
                                body: frame.stmts,
                            });
                        } else {
                            // No back edge: the loop body runs exactly once.
                            self.cur().stmts.extend(frame.stmts);
                        }
                    }
                    FrameKind::If { cond, then_body } => {
                        let label = Label {
                            id: frame.label_id,
                            referenced: frame.referenced,
                        };
                        let (then, els) = match then_body {
                            Some(then) => (then, frame.stmts),
                            None => (frame.stmts, Vec::new()),
                        };
                        self.emit(Stmt::If {
                            label,
                            cond,
                            then,
                            els,
                        });
                    }
                    FrameKind::Func => unreachable!("func frame handled above"),
                }
            }
        }
    }

    // ---- operator dispatch -----------------------------------------------

    fn op(&mut self, op: Operator<'_>) -> Result<()> {
        use ValType::*;

        // Skip dead code, only tracking block structure.
        if self.cur().unreachable {
            match op {
                Operator::Block { .. } | Operator::Loop { .. } | Operator::If { .. } => {
                    self.push_frame(FrameKind::Block, Vec::new(), Vec::new());
                    let frame = self.cur();
                    frame.entered_dead = true;
                    frame.unreachable = true;
                    return Ok(());
                }
                Operator::Else => {
                    if self.cur().entered_dead {
                        return Ok(());
                    }
                    self.handle_else();
                    return Ok(());
                }
                Operator::End => {
                    self.handle_end();
                    return Ok(());
                }
                _ => return Ok(()),
            }
        }

        match op {
            // -- control flow
            Operator::Nop => {}
            Operator::Unreachable => {
                // A pending OOB load must trap with its own message before the
                // "unreachable" trap, so resolve trapping pendings first.
                self.spill_if(|fx| fx.trap);
                self.emit(Stmt::Unreachable);
                self.cur().unreachable = true;
            }
            Operator::Block { blockty } => {
                let (params, results) = self.block_type(blockty)?;
                self.spill_all();
                self.push_frame(FrameKind::Block, params, results);
            }
            Operator::Loop { blockty } => {
                let (params, results) = self.block_type(blockty)?;
                self.spill_all();
                self.push_frame(FrameKind::Loop, params, results);
            }
            Operator::If { blockty } => {
                let (cond, _, _) = self.pop_expr();
                let (params, results) = self.block_type(blockty)?;
                self.spill_all();
                self.push_frame(
                    FrameKind::If {
                        cond,
                        then_body: None,
                    },
                    params,
                    results,
                );
            }
            Operator::Else => self.handle_else(),
            Operator::End => self.handle_end(),
            Operator::Br { relative_depth } => {
                let idx = self.frames.len() - 1 - relative_depth as usize;
                if matches!(self.frames[idx].kind, FrameKind::Func) {
                    // A branch to the function frame is a return: fold the
                    // values, but first flush any deeper trapping pending that
                    // wasm would have evaluated before the branch.
                    let arity = self.frames[idx].results.len();
                    let mut values = vec![Expr::I32Const(0); arity];
                    for i in (0..arity).rev() {
                        values[i] = self.pop_expr().0;
                    }
                    self.spill_if(|fx| fx.trap);
                    self.emit(Stmt::Br(BrTarget::Return { values }));
                } else {
                    self.spill_all();
                    let target = self.branch_target(relative_depth);
                    self.emit(Stmt::Br(target));
                }
                self.cur().unreachable = true;
            }
            Operator::BrIf { relative_depth } => {
                let (cond, _, _) = self.pop_expr();
                // A not-taken br_if must leave the operands reusable, and
                // branch_target reads them at canonical depths, so materialize
                // the whole stack. Return values are not folded under br_if
                // (the not-taken path would double-consume them).
                self.spill_all();
                let target = self.branch_target(relative_depth);
                self.emit(Stmt::BrIf { cond, target });
            }
            Operator::BrTable { targets } => {
                let (index, _, _) = self.pop_expr();
                self.spill_all();
                let mut resolved = Vec::new();
                for depth in targets.targets() {
                    resolved.push(self.branch_target(depth?));
                }
                let default = self.branch_target(targets.default());
                self.emit(Stmt::BrTable {
                    index,
                    targets: resolved,
                    default,
                });
                self.cur().unreachable = true;
            }
            Operator::Return => {
                let arity = self.frames[0].results.len();
                let mut values = vec![Expr::I32Const(0); arity];
                for i in (0..arity).rev() {
                    values[i] = self.pop_expr().0;
                }
                self.spill_if(|fx| fx.trap);
                self.emit(Stmt::Return { values });
                self.cur().unreachable = true;
            }
            Operator::Call { function_index } => {
                let ty = self.func_type_of(function_index).clone();
                // A call can read/write globals and memory and can trap;
                // pending values that observe any of those must be spilled.
                // Pure-local args survive and fold into the call.
                self.spill_if(|fx| fx.globals || fx.memory || fx.trap);
                let mut args = vec![Expr::I32Const(0); ty.params.len()];
                for i in (0..ty.params.len()).rev() {
                    args[i] = self.pop_expr().0;
                }
                let results = ty.results.iter().map(|ty| self.push_temp(*ty)).collect();
                self.emit(Stmt::Call {
                    func: function_index,
                    args,
                    results,
                });
            }
            Operator::CallIndirect {
                type_index,
                table_index,
                ..
            } => {
                // Spill effectful operands (and deeper pendings) first: this
                // both preserves wasm order and makes the remaining index/arg
                // fragments pure, so a backend that evaluates the index before
                // the args cannot reorder an observable effect.
                self.spill_if(|fx| fx.globals || fx.memory || fx.trap);
                let ty = self.module.types[type_index as usize].clone();
                let (index, _, _) = self.pop_expr();
                let mut args = vec![Expr::I32Const(0); ty.params.len()];
                for i in (0..ty.params.len()).rev() {
                    args[i] = self.pop_expr().0;
                }
                let results = ty.results.iter().map(|ty| self.push_temp(*ty)).collect();
                self.emit(Stmt::CallIndirect {
                    type_idx: type_index,
                    table_index,
                    index,
                    args,
                    results,
                });
            }
            Operator::Drop => {
                // A dropped value with a pending trap must still evaluate so
                // the trap fires; a pure one is simply discarded.
                let top = self.stack.len() - 1;
                if self.slot_traps(top) {
                    self.spill(top);
                }
                self.pop_expr();
            }
            Operator::TypedSelect { ty } => {
                // A numeric typed select is just an annotated select; a
                // ref-typed one is a reference-types construct (ADR-24).
                val_type(ty)?;
                self.select();
            }
            Operator::Select => self.select(),

            // -- locals and globals
            Operator::LocalGet { local_index } => {
                let ty = self.all_locals[local_index as usize];
                self.push_pending(ty, Expr::LocalGet(local_index), Effects::local(local_index), 1);
            }
            Operator::LocalSet { local_index } => {
                let (value, _, _) = self.pop_expr();
                // Earlier pendings that read this local must observe its old
                // value, so spill them before the assignment.
                self.spill_if(|fx| fx.reads_local(local_index));
                self.emit(Stmt::LocalSet {
                    idx: local_index,
                    expr: value,
                });
            }
            Operator::LocalTee { local_index } => {
                let ty = self.top_ty();
                let (value, _, _) = self.pop_expr();
                self.spill_if(|fx| fx.reads_local(local_index));
                self.emit(Stmt::LocalSet {
                    idx: local_index,
                    expr: value,
                });
                // The value stays on the stack; a later `local.set` on the
                // same local spills this via the reads-local rule.
                self.push_pending(ty, Expr::LocalGet(local_index), Effects::local(local_index), 1);
            }
            Operator::GlobalGet { global_index } => {
                let ty = self.module.global_type(global_index);
                let fx = Effects {
                    globals: true,
                    ..Effects::default()
                };
                self.push_pending(ty, Expr::GlobalGet(global_index), fx, 1);
            }
            Operator::GlobalSet { global_index } => {
                let (value, _, _) = self.pop_expr();
                // Post-trap global state is observable (the spec asserts it),
                // so spill both global reads and trapping pendings first.
                self.spill_if(|fx| fx.globals || fx.trap);
                self.emit(Stmt::GlobalSet {
                    idx: global_index,
                    expr: value,
                });
            }

            // -- memory
            Operator::I32Load { memarg } => self.load(LoadOp::I32Load, I32, &memarg),
            Operator::I64Load { memarg } => self.load(LoadOp::I64Load, I64, &memarg),
            Operator::F32Load { memarg } => self.load(LoadOp::F32Load, F32, &memarg),
            Operator::F64Load { memarg } => self.load(LoadOp::F64Load, F64, &memarg),
            Operator::I32Load8S { memarg } => self.load(LoadOp::I32Load8S, I32, &memarg),
            Operator::I32Load8U { memarg } => self.load(LoadOp::I32Load8U, I32, &memarg),
            Operator::I32Load16S { memarg } => self.load(LoadOp::I32Load16S, I32, &memarg),
            Operator::I32Load16U { memarg } => self.load(LoadOp::I32Load16U, I32, &memarg),
            Operator::I64Load8S { memarg } => self.load(LoadOp::I64Load8S, I64, &memarg),
            Operator::I64Load8U { memarg } => self.load(LoadOp::I64Load8U, I64, &memarg),
            Operator::I64Load16S { memarg } => self.load(LoadOp::I64Load16S, I64, &memarg),
            Operator::I64Load16U { memarg } => self.load(LoadOp::I64Load16U, I64, &memarg),
            Operator::I64Load32S { memarg } => self.load(LoadOp::I64Load32S, I64, &memarg),
            Operator::I64Load32U { memarg } => self.load(LoadOp::I64Load32U, I64, &memarg),
            Operator::I32Store { memarg } => self.store(StoreOp::I32Store, &memarg),
            Operator::I64Store { memarg } => self.store(StoreOp::I64Store, &memarg),
            Operator::F32Store { memarg } => self.store(StoreOp::F32Store, &memarg),
            Operator::F64Store { memarg } => self.store(StoreOp::F64Store, &memarg),
            Operator::I32Store8 { memarg } => self.store(StoreOp::I32Store8, &memarg),
            Operator::I32Store16 { memarg } => self.store(StoreOp::I32Store16, &memarg),
            Operator::I64Store8 { memarg } => self.store(StoreOp::I64Store8, &memarg),
            Operator::I64Store16 { memarg } => self.store(StoreOp::I64Store16, &memarg),
            Operator::I64Store32 { memarg } => self.store(StoreOp::I64Store32, &memarg),
            Operator::MemorySize { .. } => {
                let fx = Effects {
                    memory: true,
                    ..Effects::default()
                };
                self.push_pending(I32, Expr::MemorySize, fx, 1);
            }
            Operator::MemoryGrow { .. } => {
                let (delta, _, _) = self.pop_expr();
                self.spill_if(|fx| fx.memory || fx.trap);
                let dst = self.push_temp(I32);
                self.emit(Stmt::MemoryGrow { dst, delta });
            }
            Operator::MemoryCopy { .. } => {
                let (len, _, _) = self.pop_expr();
                let (src, _, _) = self.pop_expr();
                let (dst, _, _) = self.pop_expr();
                self.spill_if(|fx| fx.memory || fx.trap);
                self.emit(Stmt::MemoryCopy { dst, src, len });
            }
            Operator::MemoryFill { .. } => {
                let (len, _, _) = self.pop_expr();
                let (val, _, _) = self.pop_expr();
                let (dst, _, _) = self.pop_expr();
                self.spill_if(|fx| fx.memory || fx.trap);
                self.emit(Stmt::MemoryFill { dst, val, len });
            }
            Operator::MemoryInit { data_index, .. } => {
                let (len, _, _) = self.pop_expr();
                let (src, _, _) = self.pop_expr();
                let (dst, _, _) = self.pop_expr();
                self.spill_if(|fx| fx.memory || fx.trap);
                self.emit(Stmt::MemoryInit {
                    seg: data_index,
                    dst,
                    src,
                    len,
                });
            }
            Operator::DataDrop { data_index } => {
                self.emit(Stmt::DataDrop { seg: data_index });
            }
            Operator::TableInit { elem_index, table } => {
                let (len, _, _) = self.pop_expr();
                let (src, _, _) = self.pop_expr();
                let (dst, _, _) = self.pop_expr();
                self.spill_if(|fx| fx.memory || fx.trap);
                self.emit(Stmt::TableInit {
                    seg: elem_index,
                    table_index: table,
                    dst,
                    src,
                    len,
                });
            }
            Operator::TableCopy {
                dst_table,
                src_table,
            } => {
                let (len, _, _) = self.pop_expr();
                let (src, _, _) = self.pop_expr();
                let (dst, _, _) = self.pop_expr();
                self.spill_if(|fx| fx.memory || fx.trap);
                self.emit(Stmt::TableCopy {
                    dst_table,
                    src_table,
                    dst,
                    src,
                    len,
                });
            }
            Operator::ElemDrop { elem_index } => {
                self.emit(Stmt::ElemDrop { seg: elem_index });
            }

            // -- reference types: the validator tolerates the encoding
            // (features() keeps the bit for overlong call_indirect
            // immediates), but every actual construct is rejected (ADR-24).
            Operator::RefNull { .. }
            | Operator::RefFunc { .. }
            | Operator::RefIsNull
            | Operator::TableGet { .. }
            | Operator::TableSet { .. }
            | Operator::TableSize { .. }
            | Operator::TableGrow { .. }
            | Operator::TableFill { .. } => {
                return Err(unsupported(
                    Feature::ReferenceTypes,
                    format!("reference/table instruction {op:?}"),
                ));
            }

            // -- constants
            Operator::I32Const { value } => {
                self.push_pending(I32, Expr::I32Const(value as u32), Effects::default(), 1)
            }
            Operator::I64Const { value } => {
                self.push_pending(I64, Expr::I64Const(value as u64), Effects::default(), 1)
            }
            Operator::F32Const { value } => {
                self.push_pending(F32, Expr::F32Const(value.bits()), Effects::default(), 1)
            }
            Operator::F64Const { value } => {
                self.push_pending(F64, Expr::F64Const(value.bits()), Effects::default(), 1)
            }

            // -- i32 unary/binary
            Operator::I32Eqz => self.un(UnOp::I32Eqz, I32),
            Operator::I32Clz => self.un(UnOp::I32Clz, I32),
            Operator::I32Ctz => self.un(UnOp::I32Ctz, I32),
            Operator::I32Popcnt => self.un(UnOp::I32Popcnt, I32),
            Operator::I32Add => self.bin(BinOp::I32Add, I32),
            Operator::I32Sub => self.bin(BinOp::I32Sub, I32),
            Operator::I32Mul => self.bin(BinOp::I32Mul, I32),
            Operator::I32DivS => self.bin(BinOp::I32DivS, I32),
            Operator::I32DivU => self.bin(BinOp::I32DivU, I32),
            Operator::I32RemS => self.bin(BinOp::I32RemS, I32),
            Operator::I32RemU => self.bin(BinOp::I32RemU, I32),
            Operator::I32And => self.bin(BinOp::I32And, I32),
            Operator::I32Or => self.bin(BinOp::I32Or, I32),
            Operator::I32Xor => self.bin(BinOp::I32Xor, I32),
            Operator::I32Shl => self.bin(BinOp::I32Shl, I32),
            Operator::I32ShrS => self.bin(BinOp::I32ShrS, I32),
            Operator::I32ShrU => self.bin(BinOp::I32ShrU, I32),
            Operator::I32Rotl => self.bin(BinOp::I32Rotl, I32),
            Operator::I32Rotr => self.bin(BinOp::I32Rotr, I32),
            Operator::I32Eq => self.bin(BinOp::I32Eq, I32),
            Operator::I32Ne => self.bin(BinOp::I32Ne, I32),
            Operator::I32LtS => self.bin(BinOp::I32LtS, I32),
            Operator::I32LtU => self.bin(BinOp::I32LtU, I32),
            Operator::I32GtS => self.bin(BinOp::I32GtS, I32),
            Operator::I32GtU => self.bin(BinOp::I32GtU, I32),
            Operator::I32LeS => self.bin(BinOp::I32LeS, I32),
            Operator::I32LeU => self.bin(BinOp::I32LeU, I32),
            Operator::I32GeS => self.bin(BinOp::I32GeS, I32),
            Operator::I32GeU => self.bin(BinOp::I32GeU, I32),

            // -- i64 unary/binary
            Operator::I64Eqz => self.un(UnOp::I64Eqz, I32),
            Operator::I64Clz => self.un(UnOp::I64Clz, I64),
            Operator::I64Ctz => self.un(UnOp::I64Ctz, I64),
            Operator::I64Popcnt => self.un(UnOp::I64Popcnt, I64),
            Operator::I64Add => self.bin(BinOp::I64Add, I64),
            Operator::I64Sub => self.bin(BinOp::I64Sub, I64),
            Operator::I64Mul => self.bin(BinOp::I64Mul, I64),
            Operator::I64DivS => self.bin(BinOp::I64DivS, I64),
            Operator::I64DivU => self.bin(BinOp::I64DivU, I64),
            Operator::I64RemS => self.bin(BinOp::I64RemS, I64),
            Operator::I64RemU => self.bin(BinOp::I64RemU, I64),
            Operator::I64And => self.bin(BinOp::I64And, I64),
            Operator::I64Or => self.bin(BinOp::I64Or, I64),
            Operator::I64Xor => self.bin(BinOp::I64Xor, I64),
            Operator::I64Shl => self.bin(BinOp::I64Shl, I64),
            Operator::I64ShrS => self.bin(BinOp::I64ShrS, I64),
            Operator::I64ShrU => self.bin(BinOp::I64ShrU, I64),
            Operator::I64Rotl => self.bin(BinOp::I64Rotl, I64),
            Operator::I64Rotr => self.bin(BinOp::I64Rotr, I64),
            Operator::I64Eq => self.bin(BinOp::I64Eq, I32),
            Operator::I64Ne => self.bin(BinOp::I64Ne, I32),
            Operator::I64LtS => self.bin(BinOp::I64LtS, I32),
            Operator::I64LtU => self.bin(BinOp::I64LtU, I32),
            Operator::I64GtS => self.bin(BinOp::I64GtS, I32),
            Operator::I64GtU => self.bin(BinOp::I64GtU, I32),
            Operator::I64LeS => self.bin(BinOp::I64LeS, I32),
            Operator::I64LeU => self.bin(BinOp::I64LeU, I32),
            Operator::I64GeS => self.bin(BinOp::I64GeS, I32),
            Operator::I64GeU => self.bin(BinOp::I64GeU, I32),

            // -- f32
            Operator::F32Abs => self.un(UnOp::F32Abs, F32),
            Operator::F32Neg => self.un(UnOp::F32Neg, F32),
            Operator::F32Ceil => self.un(UnOp::F32Ceil, F32),
            Operator::F32Floor => self.un(UnOp::F32Floor, F32),
            Operator::F32Trunc => self.un(UnOp::F32Trunc, F32),
            Operator::F32Nearest => self.un(UnOp::F32Nearest, F32),
            Operator::F32Sqrt => self.un(UnOp::F32Sqrt, F32),
            Operator::F32Add => self.bin(BinOp::F32Add, F32),
            Operator::F32Sub => self.bin(BinOp::F32Sub, F32),
            Operator::F32Mul => self.bin(BinOp::F32Mul, F32),
            Operator::F32Div => self.bin(BinOp::F32Div, F32),
            Operator::F32Min => self.bin(BinOp::F32Min, F32),
            Operator::F32Max => self.bin(BinOp::F32Max, F32),
            Operator::F32Copysign => self.bin(BinOp::F32Copysign, F32),
            Operator::F32Eq => self.bin(BinOp::F32Eq, I32),
            Operator::F32Ne => self.bin(BinOp::F32Ne, I32),
            Operator::F32Lt => self.bin(BinOp::F32Lt, I32),
            Operator::F32Gt => self.bin(BinOp::F32Gt, I32),
            Operator::F32Le => self.bin(BinOp::F32Le, I32),
            Operator::F32Ge => self.bin(BinOp::F32Ge, I32),

            // -- f64
            Operator::F64Abs => self.un(UnOp::F64Abs, F64),
            Operator::F64Neg => self.un(UnOp::F64Neg, F64),
            Operator::F64Ceil => self.un(UnOp::F64Ceil, F64),
            Operator::F64Floor => self.un(UnOp::F64Floor, F64),
            Operator::F64Trunc => self.un(UnOp::F64Trunc, F64),
            Operator::F64Nearest => self.un(UnOp::F64Nearest, F64),
            Operator::F64Sqrt => self.un(UnOp::F64Sqrt, F64),
            Operator::F64Add => self.bin(BinOp::F64Add, F64),
            Operator::F64Sub => self.bin(BinOp::F64Sub, F64),
            Operator::F64Mul => self.bin(BinOp::F64Mul, F64),
            Operator::F64Div => self.bin(BinOp::F64Div, F64),
            Operator::F64Min => self.bin(BinOp::F64Min, F64),
            Operator::F64Max => self.bin(BinOp::F64Max, F64),
            Operator::F64Copysign => self.bin(BinOp::F64Copysign, F64),
            Operator::F64Eq => self.bin(BinOp::F64Eq, I32),
            Operator::F64Ne => self.bin(BinOp::F64Ne, I32),
            Operator::F64Lt => self.bin(BinOp::F64Lt, I32),
            Operator::F64Gt => self.bin(BinOp::F64Gt, I32),
            Operator::F64Le => self.bin(BinOp::F64Le, I32),
            Operator::F64Ge => self.bin(BinOp::F64Ge, I32),

            // -- conversions
            Operator::I32WrapI64 => self.un(UnOp::I32WrapI64, I32),
            Operator::I32TruncF32S => self.un(UnOp::I32TruncF32S, I32),
            Operator::I32TruncF32U => self.un(UnOp::I32TruncF32U, I32),
            Operator::I32TruncF64S => self.un(UnOp::I32TruncF64S, I32),
            Operator::I32TruncF64U => self.un(UnOp::I32TruncF64U, I32),
            Operator::I64ExtendI32S => self.un(UnOp::I64ExtendI32S, I64),
            Operator::I64ExtendI32U => self.un(UnOp::I64ExtendI32U, I64),
            Operator::I64TruncF32S => self.un(UnOp::I64TruncF32S, I64),
            Operator::I64TruncF32U => self.un(UnOp::I64TruncF32U, I64),
            Operator::I64TruncF64S => self.un(UnOp::I64TruncF64S, I64),
            Operator::I64TruncF64U => self.un(UnOp::I64TruncF64U, I64),
            Operator::F32ConvertI32S => self.un(UnOp::F32ConvertI32S, F32),
            Operator::F32ConvertI32U => self.un(UnOp::F32ConvertI32U, F32),
            Operator::F32ConvertI64S => self.un(UnOp::F32ConvertI64S, F32),
            Operator::F32ConvertI64U => self.un(UnOp::F32ConvertI64U, F32),
            Operator::F32DemoteF64 => self.un(UnOp::F32DemoteF64, F32),
            Operator::F64ConvertI32S => self.un(UnOp::F64ConvertI32S, F64),
            Operator::F64ConvertI32U => self.un(UnOp::F64ConvertI32U, F64),
            Operator::F64ConvertI64S => self.un(UnOp::F64ConvertI64S, F64),
            Operator::F64ConvertI64U => self.un(UnOp::F64ConvertI64U, F64),
            Operator::F64PromoteF32 => self.un(UnOp::F64PromoteF32, F64),
            Operator::I32ReinterpretF32 => self.un(UnOp::I32ReinterpretF32, I32),
            Operator::I64ReinterpretF64 => self.un(UnOp::I64ReinterpretF64, I64),
            Operator::F32ReinterpretI32 => self.un(UnOp::F32ReinterpretI32, F32),
            Operator::F64ReinterpretI64 => self.un(UnOp::F64ReinterpretI64, F64),
            Operator::I32Extend8S => self.un(UnOp::I32Extend8S, I32),
            Operator::I32Extend16S => self.un(UnOp::I32Extend16S, I32),
            Operator::I64Extend8S => self.un(UnOp::I64Extend8S, I64),
            Operator::I64Extend16S => self.un(UnOp::I64Extend16S, I64),
            Operator::I64Extend32S => self.un(UnOp::I64Extend32S, I64),
            Operator::I32TruncSatF32S => self.un(UnOp::I32TruncSatF32S, I32),
            Operator::I32TruncSatF32U => self.un(UnOp::I32TruncSatF32U, I32),
            Operator::I32TruncSatF64S => self.un(UnOp::I32TruncSatF64S, I32),
            Operator::I32TruncSatF64U => self.un(UnOp::I32TruncSatF64U, I32),
            Operator::I64TruncSatF32S => self.un(UnOp::I64TruncSatF32S, I64),
            Operator::I64TruncSatF32U => self.un(UnOp::I64TruncSatF32U, I64),
            Operator::I64TruncSatF64S => self.un(UnOp::I64TruncSatF64S, I64),
            Operator::I64TruncSatF64U => self.un(UnOp::I64TruncSatF64U, I64),

            op => {
                let name = format!("{op:?}");
                let name = name
                    .split_whitespace()
                    .next()
                    .unwrap_or(&name)
                    .trim_end_matches('{');
                return Err(match classify_op(name) {
                    Some(feature) => unsupported(feature, format!("instruction {name}")),
                    None => anyhow::anyhow!("unsupported instruction: {op:?}"),
                });
            }
        }
        Ok(())
    }
}

/// Binary ops that may trap (divide/remainder by zero or overflow).
fn bin_traps(op: BinOp) -> bool {
    use BinOp::*;
    matches!(
        op,
        I32DivS | I32DivU | I32RemS | I32RemU | I64DivS | I64DivU | I64RemS | I64RemU
    )
}

/// Unary ops that may trap: the non-saturating float->int truncations (the
/// saturating `*_sat_*` forms do not trap).
fn un_traps(op: UnOp) -> bool {
    use UnOp::*;
    matches!(
        op,
        I32TruncF32S
            | I32TruncF32U
            | I32TruncF64S
            | I32TruncF64U
            | I64TruncF32S
            | I64TruncF32U
            | I64TruncF64S
            | I64TruncF64U
    )
}

/// Attribute an untranslated operator to a feature. Operators gated by
/// validator features never reach this point; what does reach it are the
/// families our base validation accepts — an unclassified operator here is
/// a dewasm bug and the spec harness treats it as such.
fn classify_op(name: &str) -> Option<Feature> {
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| name.starts_with(p));
    if starts(&[
        "CallRef",
        "ReturnCallRef",
        "BrOnNull",
        "BrOnNonNull",
        "RefAsNonNull",
    ]) {
        Some(Feature::FunctionReferences)
    } else if starts(&[
        "RefEq",
        "RefTest",
        "RefCast",
        "RefI31",
        "I31Get",
        "StructNew",
        "StructGet",
        "StructSet",
        "ArrayNew",
        "ArrayGet",
        "ArraySet",
        "ArrayLen",
        "ArrayFill",
        "ArrayCopy",
        "ArrayInit",
        "AnyConvert",
        "ExternConvert",
        "BrOnCast",
    ]) {
        Some(Feature::Gc)
    } else if name.contains("Atomic") {
        Some(Feature::Threads)
    } else if name.contains("128") || name.contains("MulWide") {
        Some(Feature::WideArithmetic)
    } else {
        None
    }
}
