//! Intermediate representation of a wasm module.
//!
//! The IR keeps wasm's structured control flow (block/loop/if/br) as-is and
//! flattens the value stack into "temps": one variable per (stack depth, type)
//! pair, in the style of wasm2c. Every value-producing instruction is
//! materialized into a temp assignment, which keeps evaluation order and trap
//! points trivially correct. Expression folding for readability is a future
//! optimization pass.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, PartialOrd, Ord)]
pub enum ValType {
    I32,
    I64,
    F32,
    F64,
    /// A nullable reference to a wasm function (reference types). Kept as
    /// flat variants until typed function references/GC force a structured
    /// `Ref(RefType)`; keep matches funnelled through `is_ref` and the
    /// backends' `default_value` so that migration stays mechanical.
    FuncRef,
    /// A nullable reference to an arbitrary host value (reference types).
    ExternRef,
    /// A nullable reference to a caught exception (exception handling).
    ExnRef,
    /// An opaque host value crossing the canonical-ABI boundary (ADR-20):
    /// strings, lists, records, variants as the host language's native
    /// values. Only synthesized adapter modules use it; the host-boundary
    /// Exprs/Stmts below are its entire vocabulary.
    Host,
}

impl ValType {
    pub fn is_ref(self) -> bool {
        matches!(self, ValType::FuncRef | ValType::ExternRef)
    }
}

/// A flattened stack slot. `depth` is the value-stack depth the value lives
/// at; the same (depth, ty) pair always maps to the same target variable.
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
    /// Defined functions. Function index space = imported_funcs ++ funcs.
    pub funcs: Vec<Func>,
    pub imported_tables: Vec<ImportedTable>,
    /// Defined tables. Table index space = imported_tables ++ tables.
    pub tables: Vec<Table>,
    pub imported_memory: Option<ImportedMemory>,
    pub memory: Option<MemoryDef>,
    pub imported_globals: Vec<ImportedGlobal>,
    /// Defined globals. Global index space = imported_globals ++ globals.
    pub globals: Vec<Global>,
    pub imported_tags: Vec<ImportedTag>,
    /// Defined tags. Tag index space = imported_tags ++ tags.
    pub tags: Vec<Tag>,
    pub exports: Vec<Export>,
    pub elems: Vec<ElemSegment>,
    pub datas: Vec<DataSegment>,
    pub start: Option<u32>,
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

    /// The parameter types a tag's exceptions carry (results are empty by
    /// validation).
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
    /// Never copied into a table (only makes `ref.func` targets valid
    /// under reference-types validation); droppable, otherwise inert.
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

#[derive(Debug)]
pub struct Func {
    pub type_idx: u32,
    /// Declared locals (excluding params). Local index space = params ++ locals.
    pub locals: Vec<ValType>,
    /// All temps used by the body, sorted and deduplicated.
    pub temps: Vec<Temp>,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Copy, Debug)]
pub struct Label {
    pub id: u32,
    /// Whether any `br` targets this label. Unreferenced labels need no
    /// branch machinery in backends.
    pub referenced: bool,
}

/// A resolved branch target.
#[derive(Clone, Debug)]
pub enum BrTarget {
    /// Branch to the function's outermost frame == return.
    Return { values: Vec<Expr> },
    /// Branch to a labelled frame. `assigns` moves the branch operands into
    /// the frame's result temps (or param temps for loops); self-assignments
    /// are already filtered out.
    Label {
        label: u32,
        /// true: continue the loop; false: exit the block/if.
        is_loop: bool,
        assigns: Vec<(Temp, Temp)>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CatchKind {
    Catch,
    CatchRef,
    CatchAll,
    CatchAllRef,
}

/// One `try_table` catch clause. The exception's payload lands directly in
/// the *target frame's* slots (`value_temps`, plus `exn_temp` for the
/// `_ref` kinds) — computed like a branch's moves, but sourced from the
/// exception instead of the stack — so `target`'s assigns are empty.
#[derive(Debug)]
pub struct CatchClause {
    pub kind: CatchKind,
    /// Tag index for `Catch`/`CatchRef`; `None` for the catch-all kinds.
    pub tag: Option<u32>,
    pub value_temps: Vec<Temp>,
    pub exn_temp: Option<Temp>,
    pub target: BrTarget,
}

#[derive(Debug)]
pub enum Stmt {
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
    /// `try_table`: a block whose body's exceptions are dispatched to the
    /// catch clauses, first match wins; an unmatched exception (and every
    /// trap) propagates.
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
    /// `return_call`: a terminator; the callee's results become the
    /// caller's results without growing the stack.
    ReturnCall {
        func: u32,
        args: Vec<Expr>,
    },
    ReturnCallIndirect {
        type_idx: u32,
        table_index: u32,
        index: Expr,
        args: Vec<Expr>,
    },
    TableSet {
        table: u32,
        index: Expr,
        value: Expr,
    },
    TableGrow {
        dst: Temp,
        table: u32,
        init: Expr,
        delta: Expr,
    },
    TableFill {
        table: u32,
        dst: Expr,
        val: Expr,
        len: Expr,
    },
    // -- host-boundary vocabulary (ADR-20; adapter modules only) --------
    /// Store a host string's / byte string's raw bytes at `addr`.
    HostBytesStore {
        addr: Expr,
        value: Expr,
    },
    /// Append a value to a host list.
    HostListPush {
        list: Expr,
        value: Expr,
    },
    Unreachable,
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
    /// Operands are pre-evaluated temps, so no short-circuit semantics needed.
    Select {
        cond: Box<Expr>,
        then: Box<Expr>,
        els: Box<Expr>,
    },
    MemorySize,
    /// A null reference of the given (FuncRef/ExternRef) type.
    RefNull(ValType),
    /// A reference to the function at this index (function index space).
    RefFunc(u32),
    RefIsNull(Box<Expr>),
    TableGet {
        table: u32,
        index: Box<Expr>,
    },
    TableSize(u32),
    // -- host-boundary vocabulary (ADR-20; adapter modules only) --------
    /// Lift a utf8 string from linear memory into a host string.
    HostString {
        ptr: Box<Expr>,
        len: Box<Expr>,
    },
    /// Lift `len` raw bytes (a canonical `list<u8>`) into a host byte
    /// string.
    HostBytes {
        ptr: Box<Expr>,
        len: Box<Expr>,
    },
    /// Byte length of a host string / byte string.
    HostByteLen(Box<Expr>),
    /// A fresh, empty host list.
    HostListNew,
    /// Element `index` (i32 expr) of a host list.
    HostListGet {
        list: Box<Expr>,
        index: Box<Expr>,
    },
    /// Length of a host list (i32).
    HostListLen(Box<Expr>),
    /// A host tuple from element values.
    HostTuple(Vec<Expr>),
    /// Element `index` (static) of a host tuple.
    HostTupleGet {
        value: Box<Expr>,
        index: u32,
    },
    /// A host record from named fields.
    HostRecord(Vec<(String, Expr)>),
    /// Field `name` of a host record.
    HostField {
        value: Box<Expr>,
        name: String,
    },
    /// A host variant value: `[case, payload]` in spirit; `payload` absent
    /// for unit cases. Options and results are variants with case names
    /// `"none"/"some"` and `"ok"/"err"`.
    HostVariant {
        case: String,
        payload: Option<Box<Expr>>,
    },
    /// The case index (i32) of a host variant value, given the case order.
    HostVariantCase {
        cases: Vec<String>,
        value: Box<Expr>,
    },
    /// The payload of a host variant value.
    HostVariantPayload(Box<Expr>),
    /// Lift an enum discriminant (i32) into a host enum value; the host
    /// representation is the case name.
    HostEnum {
        cases: Vec<String>,
        value: Box<Expr>,
    },
    /// Lower a host enum value back to its case index (i32).
    HostEnumIndex {
        cases: Vec<String>,
        value: Box<Expr>,
    },
    /// Lift an i32 into a host boolean.
    HostBool(Box<Expr>),
    /// Lower a host boolean (or nil) to 0/1.
    HostBoolToI32(Box<Expr>),
    /// Lift an i32 code point into a host one-character string.
    HostChar(Box<Expr>),
    /// Lower a host one-character string to its code point.
    HostCharToI32(Box<Expr>),
    /// Whether a host option value is present (i32 0/1); the host null is
    /// the absent option.
    HostIsSome(Box<Expr>),
    /// The host "absent" value (nil).
    HostNone,
    /// Lift a masked-unsigned core i32 into the host's natural signed
    /// integer (ADR-2's signed view crossing the boundary).
    HostSigned32(Box<Expr>),
    /// Same for i64.
    HostSigned64(Box<Expr>),
    /// Lower a host integer (possibly negative) into the masked-unsigned
    /// core i32 representation.
    HostMask32(Box<Expr>),
    /// Same for i64.
    HostMask64(Box<Expr>),
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
