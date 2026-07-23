//! Translate a wasm function body (stack machine) into structured IR.
//!
//! Every pushed value is materialized into a temp keyed by (stack depth,
//! type), so evaluation order and trap points are preserved without any
//! effect analysis. Dead code after an unconditional branch is skipped while
//! tracking block nesting only.

use std::collections::BTreeSet;

use anyhow::Result;
use wasmparser::{BlockType, FunctionBody, Operator};

use crate::feature::Feature;
use crate::ir::{self, BinOp, BrTarget, Expr, Label, LoadOp, Stmt, StoreOp, Temp, UnOp, ValType};
use crate::module::{unsupported, val_type};

pub struct FuncBuilder<'a> {
    module: &'a ir::Module,
    /// Type indices of all defined functions (the code section is being
    /// translated, so `module.funcs` is still incomplete).
    defined_func_types: &'a [u32],
    type_idx: u32,
    /// params ++ declared locals
    all_locals: Vec<ValType>,
    num_params: usize,
    stack: Vec<ValType>,
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

    fn push(&mut self, ty: ValType) -> Temp {
        let temp = Temp {
            depth: self.stack.len() as u32,
            ty,
        };
        self.temps.insert(temp);
        self.stack.push(ty);
        temp
    }

    fn pop(&mut self) -> Temp {
        let ty = self.stack.pop().expect("value stack is not empty");
        Temp {
            depth: self.stack.len() as u32,
            ty,
        }
    }

    fn peek(&self) -> Temp {
        let ty = *self.stack.last().expect("value stack is not empty");
        Temp {
            depth: self.stack.len() as u32 - 1,
            ty,
        }
    }

    fn push_assign(&mut self, ty: ValType, expr: Expr) {
        let dst = self.push(ty);
        self.emit(Stmt::Assign { dst, expr });
    }

    fn un(&mut self, op: UnOp, res: ValType) {
        let a = self.pop();
        self.push_assign(res, Expr::Un(op, Box::new(Expr::Temp(a))));
    }

    fn bin(&mut self, op: BinOp, res: ValType) {
        let b = self.pop();
        let a = self.pop();
        self.push_assign(
            res,
            Expr::Bin(op, Box::new(Expr::Temp(a)), Box::new(Expr::Temp(b))),
        );
    }

    fn load(&mut self, op: LoadOp, res: ValType, memarg: &wasmparser::MemArg) {
        let addr = self.pop();
        self.push_assign(
            res,
            Expr::Load {
                op,
                addr: Box::new(Expr::Temp(addr)),
                offset: memarg.offset,
            },
        );
    }

    fn store(&mut self, op: StoreOp, memarg: &wasmparser::MemArg) {
        let value = self.pop();
        let addr = self.pop();
        self.emit(Stmt::Store {
            op,
            addr: Expr::Temp(addr),
            value: Expr::Temp(value),
            offset: memarg.offset,
        });
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
    /// slots. Marks the target label as referenced.
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
            self.push(ty);
        }
        self.cur().unreachable = false;
    }

    fn handle_end(&mut self) {
        let frame = self.frames.pop().expect("frame stack is not empty");
        if frame.entered_dead {
            // The whole frame was dead code; the parent stays unreachable
            // and the stack was never touched.
            return;
        }

        // Fallthrough values already live at the result slots; formally
        // rebuild the stack shape for the parent frame.
        self.stack.truncate(frame.base);
        for ty in &frame.results {
            self.push(*ty);
        }

        match frame.kind {
            FrameKind::Func => {
                let mut body = frame.stmts;
                if !frame.unreachable {
                    let arity = frame.results.len();
                    let values = (0..arity)
                        .map(|i| {
                            Expr::Temp(Temp {
                                depth: i as u32,
                                ty: frame.results[i],
                            })
                        })
                        .collect();
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
                self.emit(Stmt::Unreachable);
                self.cur().unreachable = true;
            }
            Operator::Block { blockty } => {
                let (params, results) = self.block_type(blockty)?;
                self.push_frame(FrameKind::Block, params, results);
            }
            Operator::Loop { blockty } => {
                let (params, results) = self.block_type(blockty)?;
                self.push_frame(FrameKind::Loop, params, results);
            }
            Operator::If { blockty } => {
                let cond = self.pop();
                let (params, results) = self.block_type(blockty)?;
                self.push_frame(
                    FrameKind::If {
                        cond: Expr::Temp(cond),
                        then_body: None,
                    },
                    params,
                    results,
                );
            }
            Operator::Else => self.handle_else(),
            Operator::End => self.handle_end(),
            Operator::Br { relative_depth } => {
                let target = self.branch_target(relative_depth);
                self.emit(Stmt::Br(target));
                self.cur().unreachable = true;
            }
            Operator::BrIf { relative_depth } => {
                let cond = self.pop();
                let target = self.branch_target(relative_depth);
                self.emit(Stmt::BrIf {
                    cond: Expr::Temp(cond),
                    target,
                });
            }
            Operator::BrTable { targets } => {
                let index = self.pop();
                let mut resolved = Vec::new();
                for depth in targets.targets() {
                    resolved.push(self.branch_target(depth?));
                }
                let default = self.branch_target(targets.default());
                self.emit(Stmt::BrTable {
                    index: Expr::Temp(index),
                    targets: resolved,
                    default,
                });
                self.cur().unreachable = true;
            }
            Operator::Return => {
                let arity = {
                    let frame = &self.frames[0];
                    frame.results.len()
                };
                let values = (0..arity)
                    .map(|i| {
                        Expr::Temp(Temp {
                            depth: (self.stack.len() - arity + i) as u32,
                            ty: self.frames[0].results[i],
                        })
                    })
                    .collect();
                self.emit(Stmt::Return { values });
                self.cur().unreachable = true;
            }
            Operator::Call { function_index } => {
                let ty = self.func_type_of(function_index).clone();
                let mut args = vec![Expr::I32Const(0); ty.params.len()];
                for i in (0..ty.params.len()).rev() {
                    args[i] = Expr::Temp(self.pop());
                }
                let results = ty.results.iter().map(|ty| self.push(*ty)).collect();
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
                let index = self.pop();
                let ty = self.module.types[type_index as usize].clone();
                let mut args = vec![Expr::I32Const(0); ty.params.len()];
                for i in (0..ty.params.len()).rev() {
                    args[i] = Expr::Temp(self.pop());
                }
                let results = ty.results.iter().map(|ty| self.push(*ty)).collect();
                self.emit(Stmt::CallIndirect {
                    type_idx: type_index,
                    table_index,
                    index: Expr::Temp(index),
                    args,
                    results,
                });
            }
            Operator::Drop => {
                self.pop();
            }
            Operator::Select | Operator::TypedSelect { .. } => {
                let cond = self.pop();
                let els = self.pop();
                let then = self.pop();
                let dst = self.push(then.ty);
                self.emit(Stmt::Assign {
                    dst,
                    expr: Expr::Select {
                        cond: Box::new(Expr::Temp(cond)),
                        then: Box::new(Expr::Temp(then)),
                        els: Box::new(Expr::Temp(els)),
                    },
                });
            }

            // -- locals and globals
            Operator::LocalGet { local_index } => {
                let ty = self.all_locals[local_index as usize];
                self.push_assign(ty, Expr::LocalGet(local_index));
            }
            Operator::LocalSet { local_index } => {
                let value = self.pop();
                self.emit(Stmt::LocalSet {
                    idx: local_index,
                    expr: Expr::Temp(value),
                });
            }
            Operator::LocalTee { local_index } => {
                let value = self.peek();
                self.emit(Stmt::LocalSet {
                    idx: local_index,
                    expr: Expr::Temp(value),
                });
            }
            Operator::GlobalGet { global_index } => {
                let ty = self.module.global_type(global_index);
                self.push_assign(ty, Expr::GlobalGet(global_index));
            }
            Operator::GlobalSet { global_index } => {
                let value = self.pop();
                self.emit(Stmt::GlobalSet {
                    idx: global_index,
                    expr: Expr::Temp(value),
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
            Operator::MemorySize { .. } => self.push_assign(I32, Expr::MemorySize),
            Operator::MemoryGrow { .. } => {
                let delta = self.pop();
                let dst = self.push(I32);
                self.emit(Stmt::MemoryGrow {
                    dst,
                    delta: Expr::Temp(delta),
                });
            }
            Operator::MemoryCopy { .. } => {
                let len = self.pop();
                let src = self.pop();
                let dst = self.pop();
                self.emit(Stmt::MemoryCopy {
                    dst: Expr::Temp(dst),
                    src: Expr::Temp(src),
                    len: Expr::Temp(len),
                });
            }
            Operator::MemoryFill { .. } => {
                let len = self.pop();
                let val = self.pop();
                let dst = self.pop();
                self.emit(Stmt::MemoryFill {
                    dst: Expr::Temp(dst),
                    val: Expr::Temp(val),
                    len: Expr::Temp(len),
                });
            }
            Operator::MemoryInit { data_index, .. } => {
                let len = self.pop();
                let src = self.pop();
                let dst = self.pop();
                self.emit(Stmt::MemoryInit {
                    seg: data_index,
                    dst: Expr::Temp(dst),
                    src: Expr::Temp(src),
                    len: Expr::Temp(len),
                });
            }
            Operator::DataDrop { data_index } => {
                self.emit(Stmt::DataDrop { seg: data_index });
            }
            Operator::TableInit { elem_index, table } => {
                let len = self.pop();
                let src = self.pop();
                let dst = self.pop();
                self.emit(Stmt::TableInit {
                    seg: elem_index,
                    table_index: table,
                    dst: Expr::Temp(dst),
                    src: Expr::Temp(src),
                    len: Expr::Temp(len),
                });
            }
            Operator::TableCopy {
                dst_table,
                src_table,
            } => {
                let len = self.pop();
                let src = self.pop();
                let dst = self.pop();
                self.emit(Stmt::TableCopy {
                    dst_table,
                    src_table,
                    dst: Expr::Temp(dst),
                    src: Expr::Temp(src),
                    len: Expr::Temp(len),
                });
            }
            Operator::ElemDrop { elem_index } => {
                self.emit(Stmt::ElemDrop { seg: elem_index });
            }

            // -- constants
            Operator::I32Const { value } => self.push_assign(I32, Expr::I32Const(value as u32)),
            Operator::I64Const { value } => self.push_assign(I64, Expr::I64Const(value as u64)),
            Operator::F32Const { value } => self.push_assign(F32, Expr::F32Const(value.bits())),
            Operator::F64Const { value } => self.push_assign(F64, Expr::F64Const(value.bits())),

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

/// Attribute an untranslated operator to a feature. Operators gated by
/// validator features never reach this point; what does reach it are the
/// families our base validation accepts (reference types encodings, bulk
/// table ops) — an unclassified operator here is a dewasmify bug and the
/// spec harness treats it as such.
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
    } else if starts(&["ReturnCall"]) {
        Some(Feature::TailCall)
    } else if starts(&["Table", "RefNull", "RefIsNull", "RefFunc"]) {
        Some(Feature::ReferenceTypes)
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
    } else if starts(&["Throw", "Rethrow", "Try", "Catch", "Delegate", "ThrowRef"]) {
        Some(Feature::ExceptionHandling)
    } else if name.contains("Atomic") {
        Some(Feature::Threads)
    } else if name.contains("128") || name.contains("MulWide") {
        Some(Feature::WideArithmetic)
    } else {
        None
    }
}
