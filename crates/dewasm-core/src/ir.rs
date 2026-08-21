//! Intermediate representation of a wasm module.
//!
//! The IR keeps wasm's structured control flow (block/loop/if/br) as-is and flattens the value stack into "temps": one variable per (stack depth, type) pair, in the style of wasm2c.
//! A value folds into its consumer where it can and takes a temp only where it must; `func.rs` owns the spill discipline that keeps evaluation order and trap points correct.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    /// A nullable reference to a wasm function.
    /// Legal only as a table element type; reference types used as value types are rejected at conversion time.
    FuncRef,
    /// A nullable reference to a caught exception (exception handling).
    /// Unlike the other reference types this one *is* legal as a value type: `catch_ref` produces it in locals, temps, and block types.
    ExnRef,
}

/// A flattened stack slot.
/// `depth` is the value-stack depth the value lives at; the same (depth, ty) pair always maps to the same target variable.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub struct Temp {
    pub depth: u32,
    pub ty: ValType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuncType {
    pub params: Vec<ValType>,
    pub results: Vec<ValType>,
}

#[derive(Debug)]
pub struct Module {
    pub types: Vec<FuncType>,
    pub imported_funcs: Vec<ImportedFunc>,
    /// Defined functions.
    /// Function index space = imported_funcs ++ funcs.
    pub funcs: Vec<Func>,
    pub imported_tables: Vec<ImportedTable>,
    /// Defined tables.
    /// Table index space = imported_tables ++ tables.
    pub tables: Vec<Table>,
    pub imported_memory: Option<ImportedMemory>,
    pub memory: Option<MemoryDef>,
    pub imported_globals: Vec<ImportedGlobal>,
    /// Defined globals.
    /// Global index space = imported_globals ++ globals.
    pub globals: Vec<Global>,
    pub imported_tags: Vec<ImportedTag>,
    /// Defined tags.
    /// Tag index space = imported_tags ++ tags.
    pub tags: Vec<Tag>,
    pub exports: Vec<Export>,
    pub elems: Vec<ElemSegment>,
    pub datas: Vec<DataSegment>,
    pub start: Option<u32>,
    /// Interned source file paths referenced by [`Stmt::SourceLine`] markers, indexed by [`SourcePos::file`].
    /// Empty unless DWARF line back-mapping was requested (`BuildOptions::debug_line`).
    pub debug_files: Vec<String>,
}

impl Module {
    pub fn func_type(&self, func_idx: u32) -> &FuncType {
        let idx = func_idx as usize;
        let ty_idx = if idx < self.imported_funcs.len() {
            self.imported_funcs[idx].type_idx
        } else {
            self.funcs[idx - self.imported_funcs.len()].type_idx
        };
        &self.types[ty_idx as usize]
    }

    pub fn num_imported_funcs(&self) -> u32 {
        self.imported_funcs.len() as u32
    }

    pub fn num_imported_tables(&self) -> u32 {
        self.imported_tables.len() as u32
    }

    pub fn global_type(&self, global_idx: u32) -> ValType {
        let idx = global_idx as usize;
        if idx < self.imported_globals.len() {
            self.imported_globals[idx].ty
        } else {
            self.globals[idx - self.imported_globals.len()].ty
        }
    }

    pub fn table_type(&self, table_idx: u32) -> ValType {
        let idx = table_idx as usize;
        if idx < self.imported_tables.len() {
            self.imported_tables[idx].ty
        } else {
            self.tables[idx - self.imported_tables.len()].ty
        }
    }

    /// The parameter types a tag's exceptions carry; validation keeps a tag's results empty.
    pub fn tag_params(&self, tag_idx: u32) -> &[ValType] {
        let idx = tag_idx as usize;
        let type_idx = if idx < self.imported_tags.len() {
            self.imported_tags[idx].type_idx
        } else {
            self.tags[idx - self.imported_tags.len()].type_idx
        };
        &self.types[type_idx as usize].params
    }
}

#[derive(Debug)]
pub struct ImportedFunc {
    pub module: String,
    pub name: String,
    pub type_idx: u32,
}

#[derive(Debug)]
pub struct ImportedTable {
    pub module: String,
    pub name: String,
    pub ty: ValType,
    pub min: u32,
    pub max: Option<u32>,
}

#[derive(Debug)]
pub struct ImportedMemory {
    pub module: String,
    pub name: String,
    pub min_pages: u64,
    pub max_pages: Option<u64>,
}

#[derive(Debug)]
pub struct ImportedGlobal {
    pub module: String,
    pub name: String,
    pub ty: ValType,
    pub mutable: bool,
}

#[derive(Debug)]
pub struct Table {
    pub ty: ValType,
    pub min: u32,
    pub max: Option<u32>,
}

#[derive(Debug)]
pub struct MemoryDef {
    pub min_pages: u64,
    pub max_pages: Option<u64>,
}

#[derive(Debug)]
pub struct Global {
    pub ty: ValType,
    pub mutable: bool,
    pub init: Expr,
}

#[derive(Debug)]
pub struct ImportedTag {
    pub module: String,
    pub name: String,
    pub type_idx: u32,
}

#[derive(Debug)]
pub struct Tag {
    pub type_idx: u32,
}

#[derive(Debug)]
pub enum ExportKind {
    Func(u32),
    Table(u32),
    Memory,
    Global(u32),
    Tag(u32),
}

#[derive(Debug)]
pub struct Export {
    pub name: String,
    pub kind: ExportKind,
}

#[derive(Debug)]
pub enum ElemKind {
    /// Eagerly copied into `table_index` at instantiation.
    Active { table_index: u32, offset: Expr },
    /// Retained for `table.init`; droppable.
    Passive,
    /// Never copied into a table (only makes `ref.func` targets valid under reference-types validation); droppable, otherwise inert.
    Declared,
}

/// One element-segment item, a constant reference expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElemItem {
    /// `ref.func $i` (function index space).
    Func(u32),
    /// `ref.null`: an intentionally-uninitialized slot.
    Null,
    /// `global.get $i` of a ref-typed immutable global (global index space).
    Global(u32),
}

#[derive(Debug)]
pub struct ElemSegment {
    pub kind: ElemKind,
    pub items: Vec<ElemItem>,
}

#[derive(Debug)]
pub struct DataSegment {
    /// `Some(offset)` for active segments, `None` for passive ones.
    pub offset: Option<Expr>,
    pub data: Vec<u8>,
}

/// Clonable so a backend-side rewriting pass (loop-body extraction) can produce an adjusted function list without mutating the shared module.
#[derive(Clone, Debug)]
pub struct Func {
    pub type_idx: u32,
    /// Declared locals (excluding params).
    /// Local index space = params ++ locals.
    pub locals: Vec<ValType>,
    /// All temps used by the body, sorted and deduplicated.
    pub temps: Vec<Temp>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Copy, Debug)]
pub struct Label {
    pub id: u32,
    /// Whether any `br` targets this label.
    /// Unreferenced labels need no branch machinery in backends.
    pub referenced: bool,
}

/// A resolved branch target.
#[derive(Clone, Debug)]
pub enum BrTarget {
    /// Branch to the function's outermost frame == return.
    Return { values: Vec<Expr> },
    /// Branch to a labelled frame.
    /// `assigns` moves the branch operands into the frame's result temps (or param temps for loops); self-assignments are already filtered out.
    Label {
        label: u32,
        /// true: continue the loop; false: exit the block/if.
        is_loop: bool,
        assigns: Vec<(Temp, Temp)>,
    },
}

/// One `try_table` catch clause.
/// The four catch kinds are fully encoded by two fields: `tag` (`Some` for `catch`/`catch_ref`, `None` for the catch-all kinds) and `exn_temp` (`Some` for the `_ref` kinds, which capture the exception as an exnref); no backend needs the kind spelled a second way.
/// The exception's payload lands directly in the *target frame's* slots (`value_temps`, whose last entry is `exn_temp` for the `_ref` kinds): the same arithmetic a branch's moves use, sourced from the exception instead of the stack, which is why `target` carries no assigns.
#[derive(Clone, Debug)]
pub struct CatchClause {
    /// Tag index for `catch`/`catch_ref`; `None` for the catch-all kinds.
    pub tag: Option<u32>,
    pub value_temps: Vec<Temp>,
    pub exn_temp: Option<Temp>,
    pub target: BrTarget,
}

/// A resolved source position, indexing [`Module::debug_files`].
/// Carried by [`Stmt::SourceLine`] markers for DWARF line back-mapping; a `col` of 0 means the column is unknown (DWARF's "left edge").
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SourcePos {
    pub file: u32,
    pub line: u32,
    pub col: u32,
}

/// Clonable so a backend can hand its emitter a rewritten body (the Go backend drops the statements Go would report as unreachable before emitting).
#[derive(Clone, Debug)]
pub enum Stmt {
    /// A source-position marker emitted just before the statement it annotates when DWARF line back-mapping is on.
    /// Semantically inert: a backend renders it as a position directive/comment or drops it, and its presence never changes the surrounding statements' meaning.
    SourceLine(SourcePos),
    Assign {
        dst: Temp,
        expr: Expr,
    },
    LocalSet {
        idx: u32,
        expr: Expr,
    },
    GlobalSet {
        idx: u32,
        expr: Expr,
    },
    Store {
        op: StoreOp,
        addr: Expr,
        value: Expr,
        offset: u64,
    },
    Block {
        label: Label,
        body: Vec<Stmt>,
    },
    Loop {
        label: Label,
        body: Vec<Stmt>,
    },
    If {
        label: Label,
        cond: Expr,
        then: Vec<Stmt>,
        els: Vec<Stmt>,
    },
    Br(BrTarget),
    BrIf {
        cond: Expr,
        target: BrTarget,
    },
    BrTable {
        index: Expr,
        targets: Vec<BrTarget>,
        default: BrTarget,
    },
    Return {
        values: Vec<Expr>,
    },
    Call {
        func: u32,
        args: Vec<Expr>,
        results: Vec<Temp>,
    },
    CallIndirect {
        type_idx: u32,
        table_index: u32,
        index: Expr,
        args: Vec<Expr>,
        results: Vec<Temp>,
    },
    MemoryGrow {
        dst: Temp,
        delta: Expr,
    },
    MemoryCopy {
        dst: Expr,
        src: Expr,
        len: Expr,
    },
    MemoryFill {
        dst: Expr,
        val: Expr,
        len: Expr,
    },
    MemoryInit {
        seg: u32,
        dst: Expr,
        src: Expr,
        len: Expr,
    },
    DataDrop {
        seg: u32,
    },
    TableInit {
        seg: u32,
        table_index: u32,
        dst: Expr,
        src: Expr,
        len: Expr,
    },
    TableCopy {
        dst_table: u32,
        src_table: u32,
        dst: Expr,
        src: Expr,
        len: Expr,
    },
    ElemDrop {
        seg: u32,
    },
    /// `try_table`: a block whose body's exceptions are dispatched to the catch clauses, first match wins; an unmatched exception (and every trap) keeps unwinding.
    /// A catchless `try_table` is a plain [`Stmt::Block`] and never reaches here.
    TryTable {
        label: Label,
        catches: Vec<CatchClause>,
        body: Vec<Stmt>,
    },
    Throw {
        tag: u32,
        args: Vec<Expr>,
    },
    ThrowRef {
        exn: Expr,
    },
    Unreachable,
}

impl Stmt {
    /// The statement sequences nested directly in this statement, in emission order; a statement holding none yields nothing.
    /// Exhaustive on purpose: a new variant that carries statements must declare them here or fail to compile, which is what keeps every traversal built on [`Stmt::any`] reaching the whole tree.
    pub fn child_seqs(&self) -> impl Iterator<Item = &[Stmt]> {
        let seqs: [&[Stmt]; 2] = match self {
            Stmt::Block { body, .. } | Stmt::Loop { body, .. } | Stmt::TryTable { body, .. } => {
                [body, &[]]
            }
            Stmt::If { then, els, .. } => [then, els],
            Stmt::SourceLine(_)
            | Stmt::Assign { .. }
            | Stmt::LocalSet { .. }
            | Stmt::GlobalSet { .. }
            | Stmt::Store { .. }
            | Stmt::Br(_)
            | Stmt::BrIf { .. }
            | Stmt::BrTable { .. }
            | Stmt::Return { .. }
            | Stmt::Call { .. }
            | Stmt::CallIndirect { .. }
            | Stmt::MemoryGrow { .. }
            | Stmt::MemoryCopy { .. }
            | Stmt::MemoryFill { .. }
            | Stmt::MemoryInit { .. }
            | Stmt::DataDrop { .. }
            | Stmt::TableInit { .. }
            | Stmt::TableCopy { .. }
            | Stmt::ElemDrop { .. }
            | Stmt::Throw { .. }
            | Stmt::ThrowRef { .. }
            | Stmt::Unreachable => [&[], &[]],
        };
        seqs.into_iter().filter(|seq| !seq.is_empty())
    }

    /// Mutable counterpart of [`Stmt::child_seqs`], for passes that rewrite statement trees in place.
    /// The same exhaustiveness contract applies: a new variant carrying statements must appear in both.
    pub fn child_seqs_mut(&mut self) -> impl Iterator<Item = &mut Vec<Stmt>> {
        let seqs: [Option<&mut Vec<Stmt>>; 2] = match self {
            Stmt::Block { body, .. } | Stmt::Loop { body, .. } | Stmt::TryTable { body, .. } => {
                [Some(body), None]
            }
            Stmt::If { then, els, .. } => [Some(then), Some(els)],
            Stmt::SourceLine(_)
            | Stmt::Assign { .. }
            | Stmt::LocalSet { .. }
            | Stmt::GlobalSet { .. }
            | Stmt::Store { .. }
            | Stmt::Br(_)
            | Stmt::BrIf { .. }
            | Stmt::BrTable { .. }
            | Stmt::Return { .. }
            | Stmt::Call { .. }
            | Stmt::CallIndirect { .. }
            | Stmt::MemoryGrow { .. }
            | Stmt::MemoryCopy { .. }
            | Stmt::MemoryFill { .. }
            | Stmt::MemoryInit { .. }
            | Stmt::DataDrop { .. }
            | Stmt::TableInit { .. }
            | Stmt::TableCopy { .. }
            | Stmt::ElemDrop { .. }
            | Stmt::Throw { .. }
            | Stmt::ThrowRef { .. }
            | Stmt::Unreachable => [None, None],
        };
        seqs.into_iter().flatten()
    }

    /// Whether `pred` holds for any statement in `stmts` or in the sequences nested below them.
    /// A caller classifies one statement and leaves reaching the rest to [`Stmt::child_seqs`].
    pub fn any(stmts: &[Stmt], pred: &mut impl FnMut(&Stmt) -> bool) -> bool {
        stmts
            .iter()
            .any(|stmt| pred(stmt) || stmt.child_seqs().any(|seq| Stmt::any(seq, pred)))
    }
}

#[derive(Clone, Debug)]
pub enum Expr {
    /// i32 constant, stored as the unsigned (masked) value.
    I32Const(u32),
    /// i64 constant, stored as the unsigned (masked) value.
    I64Const(u64),
    /// f32 constant as raw bits.
    F32Const(u32),
    /// f64 constant as raw bits.
    F64Const(u64),
    Temp(Temp),
    LocalGet(u32),
    GlobalGet(u32),
    Un(UnOp, Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Load {
        op: LoadOp,
        addr: Box<Expr>,
        offset: u64,
    },
    /// Neither arm can trap (the builder spills a trapping one to a temp), so backends may lower this as a lazy ternary even though wasm evaluates both arms.
    Select {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    MemorySize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoadOp {
    I32Load,
    I64Load,
    F32Load,
    F64Load,
    I32Load8S,
    I32Load8U,
    I32Load16S,
    I32Load16U,
    I64Load8S,
    I64Load8U,
    I64Load16S,
    I64Load16U,
    I64Load32S,
    I64Load32U,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreOp {
    I32Store,
    I64Store,
    F32Store,
    F64Store,
    I32Store8,
    I32Store16,
    I64Store8,
    I64Store16,
    I64Store32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum UnOp {
    // i32
    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Eqz,
    // i64
    I64Clz,
    I64Ctz,
    I64Popcnt,
    I64Eqz,
    // f32
    F32Abs,
    F32Neg,
    F32Ceil,
    F32Floor,
    F32Trunc,
    F32Nearest,
    F32Sqrt,
    // f64
    F64Abs,
    F64Neg,
    F64Ceil,
    F64Floor,
    F64Trunc,
    F64Nearest,
    F64Sqrt,
    // conversions
    I32WrapI64,
    I32TruncF32S,
    I32TruncF32U,
    I32TruncF64S,
    I32TruncF64U,
    I64ExtendI32S,
    I64ExtendI32U,
    I64TruncF32S,
    I64TruncF32U,
    I64TruncF64S,
    I64TruncF64U,
    F32ConvertI32S,
    F32ConvertI32U,
    F32ConvertI64S,
    F32ConvertI64U,
    F32DemoteF64,
    F64ConvertI32S,
    F64ConvertI32U,
    F64ConvertI64S,
    F64ConvertI64U,
    F64PromoteF32,
    I32ReinterpretF32,
    I64ReinterpretF64,
    F32ReinterpretI32,
    F64ReinterpretI64,
    // sign-extension operators
    I32Extend8S,
    I32Extend16S,
    I64Extend8S,
    I64Extend16S,
    I64Extend32S,
    // saturating truncations
    I32TruncSatF32S,
    I32TruncSatF32U,
    I32TruncSatF64S,
    I32TruncSatF64U,
    I64TruncSatF32S,
    I64TruncSatF32U,
    I64TruncSatF64S,
    I64TruncSatF64U,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BinOp {
    // i32
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    // i64
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64DivU,
    I64RemS,
    I64RemU,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64Rotl,
    I64Rotr,
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,
    // f32
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Min,
    F32Max,
    F32Copysign,
    F32Eq,
    F32Ne,
    F32Lt,
    F32Gt,
    F32Le,
    F32Ge,
    // f64
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Min,
    F64Max,
    F64Copysign,
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,
}
