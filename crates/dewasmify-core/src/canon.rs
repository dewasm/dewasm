//! Canonical-ABI adapter synthesis (ADR-20): turn a parsed [`Component`]'s
//! `canon lower`/`canon lift` definitions into two synthesized core-IR
//! modules — `lowers` (guest-callable wrappers around host functions) and
//! `lifts` (host-callable wrappers around guest exports) — whose bodies do
//! all memory-layout walking with ordinary IR loads/stores/calls plus the
//! small host-boundary vocabulary (`ValType::Host`, `Expr::Host*`).
//!
//! Backends therefore never see the canonical ABI: an adapter module is a
//! regular `ir::Module` with conventional import names —
//! `canon.memory`/`canon.realloc` (the shared adapter memory and
//! `cabi_realloc`), `host.<interface>#<func>` (host functions over host
//! values), and `core.<instance>:<export>` (guest core functions a lift
//! calls). Anything the synthesizer cannot express yet is refused at
//! conversion time with `UnsupportedError(ComponentModel)` (ADR-0).

use anyhow::Result;
use std::collections::BTreeSet;

use crate::component::unsupported;
use crate::component::{CanonOpts, Component, CoreInstance, CoreItem, WitFunc, WitType};
use crate::ir::{self, BinOp, Expr, FuncType, Label, LoadOp, Stmt, StoreOp, Temp, ValType};

/// Flattened-signature limits from the canonical ABI.
const MAX_FLAT_PARAMS: usize = 16;
const MAX_FLAT_RESULTS: usize = 1;

/// A synthesized adapter module plus where its `canon.memory` and
/// `canon.realloc` imports come from (a core instance index and export
/// name) — validated uniform across the module's adapters.
pub struct AdapterModule {
    pub module: ir::Module,
    pub memory: Option<(usize, String)>,
    pub realloc: Option<(usize, String)>,
}

/// The two synthesized adapter modules plus the conventions binding them:
/// walking `component.instances` in order, the `j`-th `CoreItem::Lower`
/// encountered is `lowers`' defined function `j`; `component.lifted[i]`
/// is `lifts`' defined function `i`.
pub struct Synthesis {
    pub lowers: AdapterModule,
    pub lifts: AdapterModule,
}

/// A (core instance, export name) reference, e.g. an adapter's memory.
type CoreRef = Option<(usize, String)>;

/// Fold one adapter's canon opts into the module-wide memory/realloc
/// binding, refusing components whose adapters disagree (never produced
/// by wit-component, which routes everything through one main memory).
fn merge_opts(slot: &mut (CoreRef, CoreRef), opts: &CanonOpts) -> Result<()> {
    for (have, want) in [(&mut slot.0, &opts.memory), (&mut slot.1, &opts.realloc)] {
        if let Some(w) = want {
            match have {
                None => *have = Some(w.clone()),
                Some(h) if h == w => {}
                Some(_) => {
                    return Err(unsupported("adapters bound to different memories/reallocs"))
                }
            }
        }
    }
    Ok(())
}

pub fn synthesize(component: &Component) -> Result<Synthesis> {
    let mut lower_items: Vec<(&crate::component::HostFuncRef, &CanonOpts)> = Vec::new();
    for inst in &component.instances {
        if let CoreInstance::Synthetic(items) = inst {
            for (_, item) in items {
                if let CoreItem::Lower { host, opts } = item {
                    lower_items.push((host, opts));
                }
            }
        }
    }

    let mut lowers = ModuleBuilder::new();
    let mut lower_binding = (None, None);
    for (host, opts) in &lower_items {
        merge_opts(&mut lower_binding, opts)?;
        let body = LowerAdapter {
            b: Builder::new(),
            m: &mut lowers,
        }
        .build(host, opts)?;
        lowers.funcs.push(body);
    }
    lowers.needs_memory = lower_binding.0.is_some();

    let mut lifts = ModuleBuilder::new();
    let mut lift_binding = (None, None);
    for lifted in &component.lifted {
        merge_opts(&mut lift_binding, &lifted.opts)?;
        let body = LiftAdapter {
            b: Builder::new(),
            m: &mut lifts,
        }
        .build(lifted)?;
        lifts.funcs.push(body);
    }
    lifts.needs_memory = lift_binding.0.is_some();

    Ok(Synthesis {
        lowers: AdapterModule {
            module: lowers.finish(),
            memory: lower_binding.0,
            realloc: lower_binding.1,
        },
        lifts: AdapterModule {
            module: lifts.finish(),
            memory: lift_binding.0,
            realloc: lift_binding.1,
        },
    })
}

// ---- module scaffolding -------------------------------------------------

struct ModuleBuilder {
    types: Vec<FuncType>,
    imported_funcs: Vec<ir::ImportedFunc>,
    funcs: Vec<ir::Func>,
    needs_memory: bool,
}

impl ModuleBuilder {
    fn new() -> Self {
        ModuleBuilder {
            types: Vec::new(),
            imported_funcs: Vec::new(),
            funcs: Vec::new(),
            needs_memory: false,
        }
    }

    fn type_idx(&mut self, ty: FuncType) -> u32 {
        if let Some(i) = self.types.iter().position(|t| *t == ty) {
            return i as u32;
        }
        self.types.push(ty);
        (self.types.len() - 1) as u32
    }

    /// Import (or reuse) a function; returns its function index (import
    /// space precedes defined funcs, and adapters are appended after all
    /// imports exist, so indices stay stable).
    fn import_func(&mut self, module: &str, name: &str, ty: FuncType) -> u32 {
        if let Some(i) = self
            .imported_funcs
            .iter()
            .position(|f| f.module == module && f.name == name)
        {
            return i as u32;
        }
        let type_idx = self.type_idx(ty);
        self.imported_funcs.push(ir::ImportedFunc {
            module: module.to_string(),
            name: name.to_string(),
            type_idx,
        });
        (self.imported_funcs.len() - 1) as u32
    }

    fn finish(self) -> ir::Module {
        // Defined funcs were built against pre-final import indices; the
        // builders below only ever call imports, so `Call { func }` uses
        // import-space indices that remain valid.
        ir::Module {
            types: self.types,
            imported_funcs: self.imported_funcs,
            funcs: self.funcs,
            imported_tables: Vec::new(),
            tables: Vec::new(),
            imported_memory: self.needs_memory.then(|| ir::ImportedMemory {
                module: "canon".to_string(),
                name: "memory".to_string(),
                min_pages: 0,
                max_pages: None,
            }),
            memory: None,
            imported_globals: Vec::new(),
            globals: Vec::new(),
            imported_tags: Vec::new(),
            tags: Vec::new(),
            exports: Vec::new(),
            elems: Vec::new(),
            datas: Vec::new(),
            start: None,
        }
    }
}

// ---- IR body builder ----------------------------------------------------

/// Builds a single adapter body. Temps are allocated with unique depths
/// (one Ruby variable per value); locals hold loop state.
struct Builder {
    stmts_stack: Vec<Vec<Stmt>>,
    temps: BTreeSet<Temp>,
    next_temp: u32,
    locals: Vec<ValType>,
    num_params: usize,
    next_label: u32,
}

impl Builder {
    fn new() -> Self {
        Builder {
            stmts_stack: vec![Vec::new()],
            temps: BTreeSet::new(),
            next_temp: 0,
            locals: Vec::new(),
            num_params: 0,
            next_label: 0,
        }
    }

    fn emit(&mut self, stmt: Stmt) {
        self.stmts_stack.last_mut().unwrap().push(stmt);
    }

    /// A fresh temp holding `expr`.
    fn let_(&mut self, ty: ValType, expr: Expr) -> Expr {
        let t = Temp {
            depth: self.next_temp,
            ty,
        };
        self.next_temp += 1;
        self.temps.insert(t);
        self.emit(Stmt::Assign { dst: t, expr });
        Expr::Temp(t)
    }

    fn temp_of(&self, e: &Expr) -> Temp {
        match e {
            Expr::Temp(t) => *t,
            _ => unreachable!("adapter values are temps"),
        }
    }

    /// A fresh local (for mutable loop state); returns its local index.
    fn local(&mut self, ty: ValType, init: Expr) -> u32 {
        let idx = (self.num_params + self.locals.len()) as u32;
        self.locals.push(ty);
        self.emit(Stmt::LocalSet { idx, expr: init });
        idx
    }

    fn fresh_label(&mut self) -> u32 {
        self.next_label += 1;
        self.next_label - 1
    }

    fn finish(mut self, type_idx: u32) -> ir::Func {
        let body = self.stmts_stack.pop().unwrap();
        assert!(self.stmts_stack.is_empty());
        let mut temps: Vec<Temp> = self.temps.iter().copied().collect();
        temps.sort();
        ir::Func {
            type_idx,
            locals: self.locals,
            temps,
            body,
        }
    }
}

// ---- flat-type computation ----------------------------------------------

fn despecialized_cases(t: &WitType) -> Option<Vec<(String, Option<WitType>)>> {
    match t {
        WitType::Variant(cases) => Some(cases.clone()),
        WitType::Enum(names) => Some(names.iter().map(|n| (n.clone(), None)).collect()),
        WitType::Option(inner) => Some(vec![
            ("none".to_string(), None),
            ("some".to_string(), Some((**inner).clone())),
        ]),
        WitType::Result { ok, err } => Some(vec![
            ("ok".to_string(), ok.as_deref().cloned()),
            ("err".to_string(), err.as_deref().cloned()),
        ]),
        _ => None,
    }
}

fn record_fields(t: &WitType) -> Option<Vec<(String, WitType)>> {
    match t {
        WitType::Record(fs) => Some(fs.clone()),
        WitType::Tuple(ts) => Some(
            ts.iter()
                .enumerate()
                .map(|(i, t)| (i.to_string(), t.clone()))
                .collect(),
        ),
        _ => None,
    }
}

fn flatten(t: &WitType, out: &mut Vec<ValType>) -> Result<()> {
    use WitType::*;
    match t {
        Bool | U8 | U16 | U32 | S8 | S16 | S32 | Char | Flags(_) | Own(_) | Borrow(_) => {
            out.push(ValType::I32)
        }
        U64 | S64 => out.push(ValType::I64),
        F32 => out.push(ValType::F32),
        F64 => out.push(ValType::F64),
        String | List(_) => {
            out.push(ValType::I32);
            out.push(ValType::I32);
        }
        Record(_) | Tuple(_) => {
            for (_, f) in record_fields(t).unwrap() {
                flatten(&f, out)?;
            }
        }
        Enum(_) => out.push(ValType::I32),
        Variant(_) | Option(_) | Result { .. } => {
            out.push(ValType::I32);
            let cases = despecialized_cases(t).unwrap();
            let mut joined: Vec<ValType> = Vec::new();
            for (_, payload) in &cases {
                if let Some(p) = payload {
                    let mut flat = Vec::new();
                    flatten(p, &mut flat)?;
                    for (i, ty) in flat.into_iter().enumerate() {
                        if i < joined.len() {
                            joined[i] = join(joined[i], ty);
                        } else {
                            joined.push(ty);
                        }
                    }
                }
            }
            out.extend(joined);
        }
    }
    Ok(())
}

fn join(a: ValType, b: ValType) -> ValType {
    use ValType::*;
    if a == b {
        return a;
    }
    match (a, b) {
        (I32, F32) | (F32, I32) => I32,
        _ => I64,
    }
}

/// (size, alignment) of a type in the canonical memory layout.
fn size_align(t: &WitType) -> (u32, u32) {
    use WitType::*;
    match t {
        Bool | U8 | S8 => (1, 1),
        U16 | S16 => (2, 2),
        U32 | S32 | F32 | Char | Flags(_) | Own(_) | Borrow(_) | Enum(_) => (4, 4),
        U64 | S64 | F64 => (8, 8),
        String | List(_) => (8, 4),
        Record(_) | Tuple(_) => {
            let mut size = 0u32;
            let mut align = 1u32;
            for (_, f) in record_fields(t).unwrap() {
                let (s, a) = size_align(&f);
                size = align_up(size, a) + s;
                align = align.max(a);
            }
            (align_up(size, align), align)
        }
        Variant(_) | Option(_) | Result { .. } => {
            let cases = despecialized_cases(t).unwrap();
            let disc = disc_size(cases.len());
            let mut size = 0u32;
            let mut align = disc;
            for (_, payload) in &cases {
                if let Some(p) = payload {
                    let (s, a) = size_align(p);
                    size = size.max(s);
                    align = align.max(a);
                }
            }
            (align_up(align_up(disc, align) + size, align), align)
        }
    }
}

fn disc_size(cases: usize) -> u32 {
    if cases <= 256 {
        1
    } else if cases <= 65536 {
        2
    } else {
        4
    }
}

fn align_up(n: u32, align: u32) -> u32 {
    n.div_ceil(align) * align
}

fn enum_case_names(cases: &[(String, Option<WitType>)]) -> Vec<String> {
    cases.iter().map(|(n, _)| n.clone()).collect()
}

// ---- shared lift/lower over an adapter's memory -------------------------

/// Common context: the module being built (for realloc/host imports) and
/// the body builder.
struct LowerAdapter<'m> {
    b: Builder,
    m: &'m mut ModuleBuilder,
}

struct LiftAdapter<'m> {
    b: Builder,
    m: &'m mut ModuleBuilder,
}

fn host_func_type(ty: &WitFunc) -> FuncType {
    FuncType {
        params: ty.params.iter().map(|_| ValType::Host).collect(),
        results: ty.result.iter().map(|_| ValType::Host).collect(),
    }
}

fn realloc_type() -> FuncType {
    FuncType {
        params: vec![ValType::I32; 4],
        results: vec![ValType::I32],
    }
}

/// The core signature of an adapter around `ty`, in the given direction.
/// Returns (params, results, spilled_args, retptr).
fn flat_signature(ty: &WitFunc) -> Result<(Vec<ValType>, Vec<ValType>, bool, bool)> {
    let mut params = Vec::new();
    for (_, t) in &ty.params {
        flatten(t, &mut params)?;
    }
    let mut spilled = false;
    if params.len() > MAX_FLAT_PARAMS {
        params = vec![ValType::I32];
        spilled = true;
    }
    let mut results = Vec::new();
    if let Some(t) = &ty.result {
        flatten(t, &mut results)?;
    }
    let mut retptr = false;
    if results.len() > MAX_FLAT_RESULTS {
        results.clear();
        params.push(ValType::I32);
        retptr = true;
    }
    Ok((params, results, spilled, retptr))
}

impl LowerAdapter<'_> {
    /// Build a guest-callable adapter: lift core args to host values, call
    /// the host function, lower the result back.
    fn build(mut self, host: &crate::component::HostFuncRef, opts: &CanonOpts) -> Result<ir::Func> {
        let (params, results, spilled, retptr) = flat_signature(&host.ty)?;
        self.b.num_params = params.len();
        let sig = FuncType {
            params: params.clone(),
            results: results.clone(),
        };

        let host_idx = self.m.import_func(
            "host",
            &format!("{}#{}", host.interface, host.func),
            host_func_type(&host.ty),
        );

        // Lift arguments.
        let mut ctx = Ctx {
            b: &mut self.b,
            m: self.m,
            realloc: opts.realloc.is_some(),
        };
        let mut host_args = Vec::new();
        if spilled {
            // Args are a pointer to a canonical record of all params.
            let base = Expr::LocalGet(0);
            let mut offset = 0u32;
            for (_, t) in &host.ty.params {
                let (s, a) = size_align(t);
                offset = align_up(offset, a);
                let v = ctx.lift_mem(t, &base, offset)?;
                host_args.push(v);
                offset += s;
            }
        } else {
            let mut slot = 0usize;
            for (_, t) in &host.ty.params {
                let v = ctx.lift_flat(t, &mut slot)?;
                host_args.push(v);
            }
        }

        // Call the host.
        let result_temp = if host.ty.result.is_some() {
            let t = Temp {
                depth: ctx.b.next_temp,
                ty: ValType::Host,
            };
            ctx.b.next_temp += 1;
            ctx.b.temps.insert(t);
            Some(t)
        } else {
            None
        };
        ctx.b.emit(Stmt::Call {
            func: host_idx,
            args: host_args,
            results: result_temp.iter().copied().collect(),
        });

        // Lower the result.
        if let Some(rt) = &host.ty.result {
            let value = Expr::Temp(result_temp.unwrap());
            if retptr {
                let ptr = Expr::LocalGet((params.len() - 1) as u32);
                ctx.lower_mem(rt, &value, &ptr, 0)?;
                ctx.b.emit(Stmt::Return { values: vec![] });
            } else {
                let mut out = Vec::new();
                ctx.lower_flat(rt, &value, &mut out)?;
                ctx.b.emit(Stmt::Return { values: out });
            }
        } else {
            self.b.emit(Stmt::Return { values: vec![] });
        }

        let type_idx = self.m.type_idx(sig);
        Ok(self.b.finish(type_idx))
    }
}

impl LiftAdapter<'_> {
    /// Build a host-callable adapter: lower host args into core values,
    /// call the guest core function, lift its result to a host value.
    fn build(mut self, lifted: &crate::component::LiftedFunc) -> Result<ir::Func> {
        let (core_params, core_results, spilled, retptr) = flat_signature(&lifted.ty)?;
        // The adapter itself takes host values and returns one.
        self.b.num_params = lifted.ty.params.len();
        let sig = host_func_type(&lifted.ty);

        let CoreItem::InstanceExport { instance, name } = &lifted.core_func else {
            return Err(unsupported("canon lift of a non-instance-export"));
        };
        let core_idx = self.m.import_func(
            "core",
            &format!("{instance}:{name}"),
            FuncType {
                params: core_params.clone(),
                results: core_results.clone(),
            },
        );
        let post_return = lifted.opts.post_return.as_ref().map(|(inst, name)| {
            self.m.import_func(
                "core",
                &format!("{inst}:{name}"),
                FuncType {
                    params: core_results.clone(),
                    results: vec![],
                },
            )
        });

        let mut ctx = Ctx {
            b: &mut self.b,
            m: self.m,
            realloc: lifted.opts.realloc.is_some(),
        };

        // Lower host params into core args.
        let mut core_args: Vec<Expr> = Vec::new();
        if spilled {
            let mut layout = Vec::new();
            for (_, t) in &lifted.ty.params {
                layout.push(size_align(t));
            }
            let (total, max_align) = {
                let mut size = 0u32;
                let mut align = 1u32;
                for (s, a) in &layout {
                    size = align_up(size, *a) + s;
                    align = align.max(*a);
                }
                (align_up(size, align), align)
            };
            let base = ctx.alloc(total, max_align)?;
            let mut offset = 0u32;
            for (i, (_, t)) in lifted.ty.params.iter().enumerate() {
                let (s, a) = layout[i];
                offset = align_up(offset, a);
                let v = Expr::LocalGet(i as u32);
                ctx.lower_mem(t, &v, &base, offset)?;
                offset += s;
            }
            core_args.push(base);
        } else {
            for (i, (_, t)) in lifted.ty.params.iter().enumerate() {
                ctx.lower_flat(t, &Expr::LocalGet(i as u32), &mut core_args)?;
            }
        }
        let retptr_expr = if retptr {
            let (s, a) = size_align(lifted.ty.result.as_ref().unwrap());
            let p = ctx.alloc(s, a)?;
            core_args.push(p.clone());
            Some(p)
        } else {
            None
        };

        // Call the guest.
        let result_temps: Vec<Temp> = core_results
            .iter()
            .map(|ty| {
                let t = Temp {
                    depth: ctx.b.next_temp,
                    ty: *ty,
                };
                ctx.b.next_temp += 1;
                ctx.b.temps.insert(t);
                t
            })
            .collect();
        ctx.b.emit(Stmt::Call {
            func: core_idx,
            args: core_args,
            results: result_temps.clone(),
        });

        // Lift the result.
        let ret = if let Some(rt) = &lifted.ty.result {
            let v = if let Some(ptr) = retptr_expr {
                ctx.lift_mem(rt, &ptr, 0)?
            } else {
                let mut slot = 0usize;
                let args: Vec<Expr> = result_temps.iter().map(|t| Expr::Temp(*t)).collect();
                ctx.lift_from(rt, &args, &mut slot)?
            };
            vec![v]
        } else {
            vec![]
        };
        if let Some(pr) = post_return {
            ctx.b.emit(Stmt::Call {
                func: pr,
                args: result_temps.iter().map(|t| Expr::Temp(*t)).collect(),
                results: vec![],
            });
        }
        ctx.b.emit(Stmt::Return { values: ret });

        let type_idx = self.m.type_idx(sig);
        Ok(self.b.finish(type_idx))
    }
}

// ---- the lift/lower engine ---------------------------------------------

struct Ctx<'a, 'm> {
    b: &'a mut Builder,
    m: &'m mut ModuleBuilder,
    realloc: bool,
}

impl Ctx<'_, '_> {
    fn realloc_idx(&mut self) -> Result<u32> {
        if !self.realloc {
            return Err(unsupported(
                "canonical lowering needs realloc but none was given",
            ));
        }
        Ok(self.m.import_func("canon", "realloc", realloc_type()))
    }

    /// `cabi_realloc(0, 0, align, size)` -> pointer temp.
    fn alloc(&mut self, size: u32, align: u32) -> Result<Expr> {
        let realloc = self.realloc_idx()?;
        let t = Temp {
            depth: self.b.next_temp,
            ty: ValType::I32,
        };
        self.b.next_temp += 1;
        self.b.temps.insert(t);
        self.b.emit(Stmt::Call {
            func: realloc,
            args: vec![
                Expr::I32Const(0),
                Expr::I32Const(0),
                Expr::I32Const(align),
                Expr::I32Const(size),
            ],
            results: vec![t],
        });
        Ok(Expr::Temp(t))
    }

    // -- lifting (core -> host) ------------------------------------------

    /// Lift from flattened core function params, starting at local `slot`.
    fn lift_flat(&mut self, t: &WitType, slot: &mut usize) -> Result<Expr> {
        let args: Vec<Expr> = (0..self.b.num_params)
            .map(|i| Expr::LocalGet(i as u32))
            .collect();
        self.lift_from(t, &args, slot)
    }

    /// Lift from a slice of core-value expressions (function params or a
    /// callee's results), consuming `slot`s.
    fn lift_from(&mut self, t: &WitType, args: &[Expr], slot: &mut usize) -> Result<Expr> {
        use WitType::*;
        let take = |slot: &mut usize| -> Expr {
            let e = args[*slot].clone();
            *slot += 1;
            e
        };
        Ok(match t {
            Bool => {
                let v = take(slot);
                self.b.let_(ValType::Host, Expr::HostBool(Box::new(v)))
            }
            U8 | U16 | U32 | S8 | S16 | S32 | U64 | S64 | F32 | F64 | Own(_) | Borrow(_)
            | Flags(_) => {
                // Core representation is the host representation
                // (Integers/Floats); signedness of the masked value is the
                // host unit's concern for s8..s64? No: hosts get natural
                // signed Integers, so re-sign here.
                let v = take(slot);
                let signed = self.resign_lift(t, v);
                self.b.let_(ValType::Host, signed)
            }
            Char => {
                let v = take(slot);
                self.b.let_(ValType::Host, Expr::HostChar(Box::new(v)))
            }
            Enum(names) => {
                let v = take(slot);
                self.b.let_(
                    ValType::Host,
                    Expr::HostEnum {
                        cases: names.clone(),
                        value: Box::new(v),
                    },
                )
            }
            String => {
                let ptr = take(slot);
                let len = take(slot);
                self.b.let_(
                    ValType::Host,
                    Expr::HostString {
                        ptr: Box::new(ptr),
                        len: Box::new(len),
                    },
                )
            }
            List(elem) => {
                let ptr = take(slot);
                let len = take(slot);
                self.lift_list(elem, ptr, len)?
            }
            Record(_) | Tuple(_) => {
                let fields = record_fields(t).unwrap();
                let mut vals = Vec::new();
                for (name, f) in &fields {
                    let v = self.lift_from(f, args, slot)?;
                    vals.push((name.clone(), v));
                }
                self.host_composite(t, vals)
            }
            Variant(_) | Option(_) | Result { .. } => {
                let cases = despecialized_cases(t).unwrap();
                if cases.iter().any(|(_, p)| p.is_some()) {
                    return Err(unsupported(
                        "flat-position variant with payloads (only memory-position \
                         variants are lowered)",
                    ));
                }
                let disc = take(slot);
                self.lift_unit_variant(t, &cases, disc)
            }
        })
    }

    /// Lift `t` from memory at `base + offset`.
    fn lift_mem(&mut self, t: &WitType, base: &Expr, offset: u32) -> Result<Expr> {
        use WitType::*;
        let load = |op: LoadOp, res: ValType, this: &mut Self| {
            this.b.let_(
                res,
                Expr::Load {
                    op,
                    addr: Box::new(base.clone()),
                    offset: offset as u64,
                },
            )
        };
        Ok(match t {
            Bool => {
                let v = load(LoadOp::I32Load8U, ValType::I32, self);
                self.b.let_(ValType::Host, Expr::HostBool(Box::new(v)))
            }
            U8 => load(LoadOp::I32Load8U, ValType::I32, self),
            S8 => load(LoadOp::I32Load8S, ValType::I32, self),
            U16 => load(LoadOp::I32Load16U, ValType::I32, self),
            S16 => load(LoadOp::I32Load16S, ValType::I32, self),
            U32 | Own(_) | Borrow(_) | Flags(_) => load(LoadOp::I32Load, ValType::I32, self),
            S32 => {
                let v = load(LoadOp::I32Load, ValType::I32, self);
                self.resign_host(t, v)
            }
            U64 => load(LoadOp::I64Load, ValType::I64, self),
            S64 => {
                let v = load(LoadOp::I64Load, ValType::I64, self);
                self.resign_host(t, v)
            }
            F32 => load(LoadOp::F32Load, ValType::F32, self),
            F64 => load(LoadOp::F64Load, ValType::F64, self),
            Char => {
                let v = load(LoadOp::I32Load, ValType::I32, self);
                self.b.let_(ValType::Host, Expr::HostChar(Box::new(v)))
            }
            Enum(names) => {
                let v = self.load_disc(base, offset, disc_size(names.len()));
                self.b.let_(
                    ValType::Host,
                    Expr::HostEnum {
                        cases: names.clone(),
                        value: Box::new(v),
                    },
                )
            }
            String => {
                let ptr = load(LoadOp::I32Load, ValType::I32, self);
                let len = self.b.let_(
                    ValType::I32,
                    Expr::Load {
                        op: LoadOp::I32Load,
                        addr: Box::new(base.clone()),
                        offset: offset as u64 + 4,
                    },
                );
                self.b.let_(
                    ValType::Host,
                    Expr::HostString {
                        ptr: Box::new(ptr),
                        len: Box::new(len),
                    },
                )
            }
            List(elem) => {
                let ptr = load(LoadOp::I32Load, ValType::I32, self);
                let len = self.b.let_(
                    ValType::I32,
                    Expr::Load {
                        op: LoadOp::I32Load,
                        addr: Box::new(base.clone()),
                        offset: offset as u64 + 4,
                    },
                );
                self.lift_list(elem, ptr, len)?
            }
            Record(_) | Tuple(_) => {
                let fields = record_fields(t).unwrap();
                let mut off = offset;
                let mut vals = Vec::new();
                for (name, f) in &fields {
                    let (s, a) = size_align(f);
                    off = align_up(off, a);
                    let v = self.lift_mem(f, base, off)?;
                    vals.push((name.clone(), v));
                    off += s;
                }
                self.host_composite(t, vals)
            }
            Variant(_) | Option(_) | Result { .. } => {
                let cases = despecialized_cases(t).unwrap();
                let disc_sz = disc_size(cases.len());
                let (_, align) = size_align(t);
                let payload_off = align_up(align_up(disc_sz, align), payload_align(&cases));
                let disc = self.load_disc(base, offset, disc_sz);
                // Build nested ifs over the discriminant.
                let out = self.b.let_(ValType::Host, Expr::HostNone);
                let out_t = self.b.temp_of(&out);
                self.case_dispatch(&cases, &disc, |this, i, payload| {
                    let v = match payload {
                        Some(p) => {
                            let pv = this.lift_mem(p, base, offset + payload_off)?;
                            Expr::HostVariant {
                                case: cases[i].0.clone(),
                                payload: Some(Box::new(pv)),
                            }
                        }
                        None => Expr::HostVariant {
                            case: cases[i].0.clone(),
                            payload: None,
                        },
                    };
                    this.b.emit(Stmt::Assign {
                        dst: out_t,
                        expr: v,
                    });
                    Ok(())
                })?;
                self.finish_variant_host(t, out)
            }
        })
    }

    /// Options and results have friendlier host shapes than the generic
    /// `[case, payload]` pair: an option is the payload or nil, and enum
    /// stays a symbol. Variants and results keep the pair.
    fn finish_variant_host(&mut self, t: &WitType, pair: Expr) -> Expr {
        match t {
            WitType::Option(_) => {
                // [:none, nil] -> nil ; [:some, v] -> v
                self.b
                    .let_(ValType::Host, Expr::HostVariantPayload(Box::new(pair)))
            }
            _ => pair,
        }
    }

    fn lift_unit_variant(
        &mut self,
        t: &WitType,
        cases: &[(String, Option<WitType>)],
        disc: Expr,
    ) -> Expr {
        let e = Expr::HostEnum {
            cases: enum_case_names(cases),
            value: Box::new(disc),
        };
        let sym = self.b.let_(ValType::Host, e);
        match t {
            WitType::Option(_) => unreachable!("option payload is Some"),
            WitType::Enum(_) => sym,
            _ => {
                // Rebuild the [case, nil] pair shape from the symbol: the
                // host sees results/variants uniformly.
                let cases = enum_case_names(cases);
                let idx = self.b.let_(
                    ValType::I32,
                    Expr::HostEnumIndex {
                        cases: cases.clone(),
                        value: Box::new(sym),
                    },
                );
                let out = self.b.let_(ValType::Host, Expr::HostNone);
                let out_t = self.b.temp_of(&out);
                for (i, name) in cases.iter().enumerate() {
                    let cond = Expr::Bin(
                        BinOp::I32Eq,
                        Box::new(idx.clone()),
                        Box::new(Expr::I32Const(i as u32)),
                    );
                    let assign = Stmt::Assign {
                        dst: out_t,
                        expr: Expr::HostVariant {
                            case: name.clone(),
                            payload: None,
                        },
                    };
                    let label = Label {
                        id: 2000 + i as u32,
                        referenced: false,
                    };
                    self.b.emit(Stmt::If {
                        label,
                        cond,
                        then: vec![assign],
                        els: vec![],
                    });
                }
                out
            }
        }
    }

    fn load_disc(&mut self, base: &Expr, offset: u32, size: u32) -> Expr {
        let op = match size {
            1 => LoadOp::I32Load8U,
            2 => LoadOp::I32Load16U,
            _ => LoadOp::I32Load,
        };
        self.b.let_(
            ValType::I32,
            Expr::Load {
                op,
                addr: Box::new(base.clone()),
                offset: offset as u64,
            },
        )
    }

    fn case_dispatch(
        &mut self,
        cases: &[(String, Option<WitType>)],
        disc: &Expr,
        mut f: impl FnMut(&mut Self, usize, Option<&WitType>) -> Result<()>,
    ) -> Result<()> {
        for (i, (_, payload)) in cases.iter().enumerate() {
            let cond = Expr::Bin(
                BinOp::I32Eq,
                Box::new(disc.clone()),
                Box::new(Expr::I32Const(i as u32)),
            );
            self.b.stmts_stack.push(Vec::new());
            f(self, i, payload.as_ref())?;
            let then = self.b.stmts_stack.pop().unwrap();
            let label = Label {
                id: 3000 + i as u32,
                referenced: false,
            };
            self.b.emit(Stmt::If {
                label,
                cond,
                then,
                els: vec![],
            });
        }
        Ok(())
    }

    /// Force an expression into a temp (loop bodies re-reference values).
    fn materialize(&mut self, ty: ValType, e: Expr) -> Expr {
        match e {
            Expr::Temp(_) => e,
            _ => self.b.let_(ty, e),
        }
    }

    fn lift_list(&mut self, elem: &WitType, ptr: Expr, len: Expr) -> Result<Expr> {
        let ptr = self.materialize(ValType::I32, ptr);
        let len = self.materialize(ValType::I32, len);
        if matches!(elem, WitType::U8) {
            return Ok(self.b.let_(
                ValType::Host,
                Expr::HostBytes {
                    ptr: Box::new(ptr),
                    len: Box::new(len),
                },
            ));
        }
        let (elem_size, _) = size_align(elem);
        let list = self.b.let_(ValType::Host, Expr::HostListNew);
        let i = self.b.local(ValType::I32, Expr::I32Const(0));
        let ptr_t = self.b.temp_of(&ptr);
        let len_t = self.b.temp_of(&len);
        let list_t = self.b.temp_of(&list);
        let elem = elem.clone();
        self.while_gen(i, len_t, move |this, i_local| {
            let addr = this.b.let_(
                ValType::I32,
                Expr::Bin(
                    BinOp::I32Add,
                    Box::new(Expr::Temp(ptr_t)),
                    Box::new(Expr::Bin(
                        BinOp::I32Mul,
                        Box::new(Expr::LocalGet(i_local)),
                        Box::new(Expr::I32Const(elem_size)),
                    )),
                ),
            );
            let v = this.lift_mem(&elem, &addr, 0)?;
            this.b.emit(Stmt::HostListPush {
                list: Expr::Temp(list_t),
                value: v,
            });
            Ok(())
        })?;
        Ok(list)
    }

    /// `while i < len { body(i); i += 1 }` with i a local.
    fn while_gen(
        &mut self,
        i: u32,
        len: Temp,
        mut body: impl FnMut(&mut Self, u32) -> Result<()>,
    ) -> Result<()> {
        let label = self.b.fresh_label();
        self.b.stmts_stack.push(Vec::new()); // loop body
        let cond = Expr::Bin(
            BinOp::I32LtU,
            Box::new(Expr::LocalGet(i)),
            Box::new(Expr::Temp(len)),
        );
        self.b.stmts_stack.push(Vec::new()); // then
        body(self, i)?;
        self.b.emit(Stmt::LocalSet {
            idx: i,
            expr: Expr::Bin(
                BinOp::I32Add,
                Box::new(Expr::LocalGet(i)),
                Box::new(Expr::I32Const(1)),
            ),
        });
        self.b.emit(Stmt::Br(ir::BrTarget::Label {
            label,
            is_loop: true,
            assigns: Vec::new(),
        }));
        let then = self.b.stmts_stack.pop().unwrap();
        self.b.emit(Stmt::If {
            label: Label {
                id: 4000 + label,
                referenced: false,
            },
            cond,
            then,
            els: Vec::new(),
        });
        let body_stmts = self.b.stmts_stack.pop().unwrap();
        self.b.emit(Stmt::Loop {
            label: Label {
                id: label,
                referenced: true,
            },
            body: body_stmts,
        });
        Ok(())
    }

    fn host_composite(&mut self, t: &WitType, vals: Vec<(String, Expr)>) -> Expr {
        match t {
            WitType::Tuple(_) => {
                let es = vals.into_iter().map(|(_, e)| e).collect();
                self.b.let_(ValType::Host, Expr::HostTuple(es))
            }
            _ => self.b.let_(ValType::Host, Expr::HostRecord(vals)),
        }
    }

    /// Hosts see natural signed Integers; the core side is
    /// masked-unsigned (ADR-2). Only s8..s64 need re-signing on lift.
    fn resign_lift(&mut self, t: &WitType, v: Expr) -> Expr {
        match t {
            WitType::S8 | WitType::S16 | WitType::S32 => {
                // The flat value is a masked u32; the host wants the
                // signed view. Reuse the runtime's s32 via a UnOp-free
                // trick: i32 sign-extension ops give the masked pattern,
                // not a negative Integer, so route through the host op.
                Expr::HostSigned32(Box::new(v))
            }
            WitType::S64 => Expr::HostSigned64(Box::new(v)),
            _ => v,
        }
    }

    fn resign_host(&mut self, t: &WitType, v: Expr) -> Expr {
        let e = self.resign_lift(t, v);
        self.b.let_(ValType::Host, e)
    }

    // -- lowering (host -> core) -----------------------------------------

    /// Lower a host value into flattened core values appended to `out`.
    fn lower_flat(&mut self, t: &WitType, value: &Expr, out: &mut Vec<Expr>) -> Result<()> {
        use WitType::*;
        match t {
            Bool => out.push(
                self.b
                    .let_(ValType::I32, Expr::HostBoolToI32(Box::new(value.clone()))),
            ),
            U8 | U16 | U32 | Own(_) | Borrow(_) | Flags(_) => out.push(
                self.b
                    .let_(ValType::I32, Expr::HostMask32(Box::new(value.clone()))),
            ),
            S8 | S16 | S32 => out.push(
                self.b
                    .let_(ValType::I32, Expr::HostMask32(Box::new(value.clone()))),
            ),
            U64 | S64 => out.push(
                self.b
                    .let_(ValType::I64, Expr::HostMask64(Box::new(value.clone()))),
            ),
            F32 => out.push(self.b.let_(ValType::F32, value.clone())),
            F64 => out.push(self.b.let_(ValType::F64, value.clone())),
            Char => out.push(
                self.b
                    .let_(ValType::I32, Expr::HostCharToI32(Box::new(value.clone()))),
            ),
            Enum(names) => out.push(self.b.let_(
                ValType::I32,
                Expr::HostEnumIndex {
                    cases: names.clone(),
                    value: Box::new(value.clone()),
                },
            )),
            String | List(_) => {
                let (ptr, len) = self.lower_list_like(t, value)?;
                out.push(ptr);
                out.push(len);
            }
            Record(_) | Tuple(_) => {
                let fields = record_fields(t).unwrap();
                for (i, (name, f)) in fields.iter().enumerate() {
                    let fv = self.field_of(t, value, i, name);
                    self.lower_flat(f, &fv, out)?;
                }
            }
            Variant(_) | Option(_) | Result { .. } => {
                let cases = despecialized_cases(t).unwrap();
                if cases.iter().any(|(_, p)| p.is_some()) {
                    return Err(unsupported(
                        "flat-position variant with payloads (only memory-position \
                         variants are lowered)",
                    ));
                }
                let disc = self.variant_disc(t, &cases, value);
                out.push(disc);
            }
        }
        Ok(())
    }

    /// Lower a host value into memory at `base + offset`.
    fn lower_mem(&mut self, t: &WitType, value: &Expr, base: &Expr, offset: u32) -> Result<()> {
        use WitType::*;
        let store = |op: StoreOp, v: Expr, this: &mut Self| {
            this.b.emit(Stmt::Store {
                op,
                addr: base.clone(),
                value: v,
                offset: offset as u64,
            });
        };
        match t {
            Bool => {
                let v = self
                    .b
                    .let_(ValType::I32, Expr::HostBoolToI32(Box::new(value.clone())));
                store(StoreOp::I32Store8, v, self);
            }
            U8 | S8 => {
                let v = self
                    .b
                    .let_(ValType::I32, Expr::HostMask32(Box::new(value.clone())));
                store(StoreOp::I32Store8, v, self);
            }
            U16 | S16 => {
                let v = self
                    .b
                    .let_(ValType::I32, Expr::HostMask32(Box::new(value.clone())));
                store(StoreOp::I32Store16, v, self);
            }
            U32 | S32 | Own(_) | Borrow(_) | Flags(_) => {
                let v = self
                    .b
                    .let_(ValType::I32, Expr::HostMask32(Box::new(value.clone())));
                store(StoreOp::I32Store, v, self);
            }
            U64 | S64 => {
                let v = self
                    .b
                    .let_(ValType::I64, Expr::HostMask64(Box::new(value.clone())));
                store(StoreOp::I64Store, v, self);
            }
            F32 => store(StoreOp::F32Store, value.clone(), self),
            F64 => store(StoreOp::F64Store, value.clone(), self),
            Char => {
                let v = self
                    .b
                    .let_(ValType::I32, Expr::HostCharToI32(Box::new(value.clone())));
                store(StoreOp::I32Store, v, self);
            }
            Enum(names) => {
                let v = self.b.let_(
                    ValType::I32,
                    Expr::HostEnumIndex {
                        cases: names.clone(),
                        value: Box::new(value.clone()),
                    },
                );
                self.store_disc(base, offset, disc_size(names.len()), v);
            }
            String | List(_) => {
                let (ptr, len) = self.lower_list_like(t, value)?;
                store(StoreOp::I32Store, ptr, self);
                self.b.emit(Stmt::Store {
                    op: StoreOp::I32Store,
                    addr: base.clone(),
                    value: len,
                    offset: offset as u64 + 4,
                });
            }
            Record(_) | Tuple(_) => {
                let fields = record_fields(t).unwrap();
                let mut off = offset;
                for (i, (name, f)) in fields.iter().enumerate() {
                    let (s, a) = size_align(f);
                    off = align_up(off, a);
                    let fv = self.field_of(t, value, i, name);
                    self.lower_mem(f, &fv, base, off)?;
                    off += s;
                }
            }
            Variant(_) | Option(_) | Result { .. } => {
                let cases = despecialized_cases(t).unwrap();
                let disc_sz = disc_size(cases.len());
                let (_, align) = size_align(t);
                let payload_off = align_up(align_up(disc_sz, align), payload_align(&cases));
                let pair = self.variant_pair(t, value);
                let disc = self.b.let_(
                    ValType::I32,
                    Expr::HostVariantCase {
                        cases: enum_case_names(&cases),
                        value: Box::new(pair.clone()),
                    },
                );
                self.store_disc(base, offset, disc_sz, disc.clone());
                let payload = self
                    .b
                    .let_(ValType::Host, Expr::HostVariantPayload(Box::new(pair)));
                self.case_dispatch(&cases, &disc, |this, _i, p| {
                    if let Some(p) = p {
                        this.lower_mem(p, &payload, base, offset + payload_off)?;
                    }
                    Ok(())
                })?;
            }
        }
        Ok(())
    }

    /// The canonical `[case, payload]` pair for a variant-shaped host
    /// value: options arrive as nil-or-payload and are wrapped here.
    fn variant_pair(&mut self, t: &WitType, value: &Expr) -> Expr {
        match t {
            WitType::Option(_) => {
                let is_some = self
                    .b
                    .let_(ValType::I32, Expr::HostIsSome(Box::new(value.clone())));
                let out = self.b.let_(
                    ValType::Host,
                    Expr::HostVariant {
                        case: "none".to_string(),
                        payload: None,
                    },
                );
                let out_t = self.b.temp_of(&out);
                let some = Expr::HostVariant {
                    case: "some".to_string(),
                    payload: Some(Box::new(value.clone())),
                };
                self.b.emit(Stmt::If {
                    label: Label {
                        id: 5000,
                        referenced: false,
                    },
                    cond: is_some,
                    then: vec![Stmt::Assign {
                        dst: out_t,
                        expr: some,
                    }],
                    els: vec![],
                });
                out
            }
            WitType::Enum(_) => {
                // Host enum is a bare symbol; wrap into the pair.
                self.b.let_(
                    ValType::Host,
                    Expr::HostTuple(vec![value.clone(), Expr::HostNone]),
                )
            }
            _ => value.clone(),
        }
    }

    fn variant_disc(
        &mut self,
        t: &WitType,
        cases: &[(String, Option<WitType>)],
        value: &Expr,
    ) -> Expr {
        let pair = self.variant_pair(t, value);
        self.b.let_(
            ValType::I32,
            Expr::HostVariantCase {
                cases: enum_case_names(cases),
                value: Box::new(pair),
            },
        )
    }

    fn store_disc(&mut self, base: &Expr, offset: u32, size: u32, v: Expr) {
        let op = match size {
            1 => StoreOp::I32Store8,
            2 => StoreOp::I32Store16,
            _ => StoreOp::I32Store,
        };
        self.b.emit(Stmt::Store {
            op,
            addr: base.clone(),
            value: v,
            offset: offset as u64,
        });
    }

    fn field_of(&mut self, t: &WitType, value: &Expr, index: usize, name: &str) -> Expr {
        match t {
            WitType::Tuple(_) => self.b.let_(
                ValType::Host,
                Expr::HostTupleGet {
                    value: Box::new(value.clone()),
                    index: index as u32,
                },
            ),
            _ => self.b.let_(
                ValType::Host,
                Expr::HostField {
                    value: Box::new(value.clone()),
                    name: name.to_string(),
                },
            ),
        }
    }

    /// Lower a string or list into (ptr, len) with realloc.
    fn lower_list_like(&mut self, t: &WitType, value: &Expr) -> Result<(Expr, Expr)> {
        match t {
            WitType::String => {
                let len = self
                    .b
                    .let_(ValType::I32, Expr::HostByteLen(Box::new(value.clone())));
                let ptr = self.alloc_dyn(len.clone(), 1)?;
                self.b.emit(Stmt::HostBytesStore {
                    addr: ptr.clone(),
                    value: value.clone(),
                });
                Ok((ptr, len))
            }
            WitType::List(elem) if matches!(**elem, WitType::U8) => {
                let len = self
                    .b
                    .let_(ValType::I32, Expr::HostByteLen(Box::new(value.clone())));
                let ptr = self.alloc_dyn(len.clone(), 1)?;
                self.b.emit(Stmt::HostBytesStore {
                    addr: ptr.clone(),
                    value: value.clone(),
                });
                Ok((ptr, len))
            }
            WitType::List(elem) => {
                let (elem_size, elem_align) = size_align(elem);
                let len = self
                    .b
                    .let_(ValType::I32, Expr::HostListLen(Box::new(value.clone())));
                let byte_len = self.b.let_(
                    ValType::I32,
                    Expr::Bin(
                        BinOp::I32Mul,
                        Box::new(len.clone()),
                        Box::new(Expr::I32Const(elem_size)),
                    ),
                );
                let ptr = self.alloc_dyn(byte_len, elem_align)?;
                let i = self.b.local(ValType::I32, Expr::I32Const(0));
                let len_t = self.b.temp_of(&len);
                let ptr_t = self.b.temp_of(&ptr);
                let value = value.clone();
                let elem = (**elem).clone();
                self.while_gen(i, len_t, move |this, i_local| {
                    let ev = this.b.let_(
                        ValType::Host,
                        Expr::HostListGet {
                            list: Box::new(value.clone()),
                            index: Box::new(Expr::LocalGet(i_local)),
                        },
                    );
                    let addr = this.b.let_(
                        ValType::I32,
                        Expr::Bin(
                            BinOp::I32Add,
                            Box::new(Expr::Temp(ptr_t)),
                            Box::new(Expr::Bin(
                                BinOp::I32Mul,
                                Box::new(Expr::LocalGet(i_local)),
                                Box::new(Expr::I32Const(elem_size)),
                            )),
                        ),
                    );
                    this.lower_mem(&elem, &ev, &addr, 0)?;
                    Ok(())
                })?;
                Ok((ptr, len))
            }
            _ => unreachable!("lower_list_like on non-list"),
        }
    }

    /// `cabi_realloc(0, 0, align, size_expr)`.
    fn alloc_dyn(&mut self, size: Expr, align: u32) -> Result<Expr> {
        let realloc = self.realloc_idx()?;
        let t = Temp {
            depth: self.b.next_temp,
            ty: ValType::I32,
        };
        self.b.next_temp += 1;
        self.b.temps.insert(t);
        self.b.emit(Stmt::Call {
            func: realloc,
            args: vec![
                Expr::I32Const(0),
                Expr::I32Const(0),
                Expr::I32Const(align),
                size,
            ],
            results: vec![t],
        });
        Ok(Expr::Temp(t))
    }
}

fn payload_align(cases: &[(String, Option<WitType>)]) -> u32 {
    let mut align = 1u32;
    for (_, p) in cases {
        if let Some(p) = p {
            align = align.max(size_align(p).1);
        }
    }
    align
}
