//! Backend trait and code emission utilities shared by all language
//! backends.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{bail, Result};
use dewasmify_core::feature::{Feature, UnsupportedError};
use dewasmify_core::ir;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SupportStatus {
    Supported,
    Partial(&'static str),
    Unsupported,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Emit a module that is instantiated with an imports object and exposes
    /// its exports to the host language.
    Library,
    /// Emit a runnable program that wires up WASI and calls `_start`.
    Standalone,
}

/// How generated code gets its runtime. Generated code always refers to
/// the runtime by the relative name `Rt` (or the backend's equivalent);
/// linkage only decides where that name is defined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeLinkage {
    /// Nest the needed runtime units inside the generated module itself
    /// (single self-contained file; multiple generated artifacts never
    /// collide).
    Embedded,
    /// Emit only an alias to a runtime defined elsewhere (a shared bundle
    /// in the same program, or a future runtime package/gem).
    Alias(String),
}

#[derive(Clone, Debug)]
pub struct GenOptions {
    pub mode: Mode,
    /// Class/package/module name for the generated code.
    pub module_name: String,
    pub runtime: RuntimeLinkage,
    /// Bundle the built-in WASI implementation as a fallback for
    /// `wasi_snapshot_preview1` imports the embedder does not provide.
    /// Disable to keep generated libraries free of ambient authority.
    pub default_wasi: bool,
}

pub struct OutputFile {
    pub name: String,
    pub contents: String,
}

pub trait Backend {
    fn name(&self) -> &str;
    fn file_extension(&self) -> &str;
    fn generate(&self, module: &ir::Module, opts: &GenOptions) -> anyhow::Result<Vec<OutputFile>>;

    /// The input scope every backend targets before any `Feature` rows.
    fn baseline(&self) -> &'static str {
        "Wasm core 1.0 (minus the wasm 1.0 gaps listed below) + mutable globals, \
         sign-extension, saturating float-to-int, multi-value, and the memory half \
         of bulk memory (memory.copy/fill/init, passive data segments)"
    }

    /// Declared support level per feature (ADR-8). The spec harness only
    /// tolerates skips attributable to features that are not `Supported`;
    /// flipping a feature to `Supported` makes its skips hard failures.
    fn feature_status(&self, feature: Feature) -> SupportStatus {
        let _ = feature;
        SupportStatus::Unsupported
    }
}

/// Reject, with the same `UnsupportedError` attribution the core converter
/// uses (ADR-0), any construct the shared IR now represents but this
/// specific `backend` has not declared `Supported` (ADR-8). The core
/// builder is backend-agnostic and accepts every wasm-1.0-scoped
/// construct; a backend that hasn't implemented one of them yet must
/// refuse it itself, at conversion time, rather than mis-lower it.
pub fn check_module_support(backend: &dyn Backend, module: &ir::Module) -> Result<()> {
    // `used` is a closure so the usage scan (an IR walk for TableBulkOps)
    // only runs for features the backend has *not* declared Supported.
    let require = |feature: Feature, used: &dyn Fn() -> bool, detail: &str| -> Result<()> {
        if backend.feature_status(feature) != SupportStatus::Supported && used() {
            return Err(UnsupportedError::new(feature, detail.to_string()).into());
        }
        Ok(())
    };
    require(
        Feature::ImportedGlobals,
        &|| !module.imported_globals.is_empty(),
        "imported global",
    )?;
    require(
        Feature::ImportedMemories,
        &|| module.imported_memory.is_some(),
        "imported memory",
    )?;
    require(
        Feature::ImportedTables,
        &|| !module.imported_tables.is_empty(),
        "imported table",
    )?;
    require(
        Feature::MultipleTables,
        &|| module.imported_tables.len() + module.tables.len() > 1,
        "more than one table",
    )?;
    require(
        Feature::TableBulkOps,
        &|| {
            module.elems.iter().any(|e| {
                !matches!(e.kind, ir::ElemKind::Active { .. })
                    || e.items.iter().any(|i| !matches!(i, ir::ElemItem::Func(_)))
            }) || module
                .funcs
                .iter()
                .any(|f| stmts_use_table_bulk_ops(&f.body))
        },
        "passive/declared element segment, ref.null element item, or table.init/copy/elem.drop",
    )?;
    require(
        Feature::ReferenceTypes,
        &|| module_uses_reference_types(module),
        "reference-typed value, externref table, or reference/table instruction",
    )?;
    require(
        Feature::TailCall,
        &|| module.funcs.iter().any(|f| stmts_use_tail_calls(&f.body)),
        "return_call/return_call_indirect",
    )?;
    require(
        Feature::ExceptionHandling,
        &|| module_uses_exception_handling(module),
        "tag, exnref value, or try_table/throw instruction",
    )?;
    Ok(())
}

/// Whether the module contains any exception-handling construct: a tag
/// (defined or imported; exports imply one), an exnref-typed value
/// anywhere, or the proposal's instructions. Same exhaustiveness contract
/// as `stmts_use_table_bulk_ops`.
fn module_uses_exception_handling(module: &ir::Module) -> bool {
    let exn_ty = |ty: &ir::ValType| *ty == ir::ValType::ExnRef;
    !module.imported_tags.is_empty()
        || !module.tags.is_empty()
        || module
            .types
            .iter()
            .any(|t| t.params.iter().any(exn_ty) || t.results.iter().any(exn_ty))
        || module.imported_globals.iter().any(|g| exn_ty(&g.ty))
        || module.globals.iter().any(|g| exn_ty(&g.ty))
        || module.imported_tables.iter().any(|t| exn_ty(&t.ty))
        || module.tables.iter().any(|t| exn_ty(&t.ty))
        || module.funcs.iter().any(|f| {
            f.locals.iter().any(exn_ty)
                || f.temps.iter().any(|t| exn_ty(&t.ty))
                || stmts_use_exception_handling(&f.body)
        })
}

fn stmts_use_exception_handling(stmts: &[ir::Stmt]) -> bool {
    // Exhaustive on purpose (see stmts_use_table_bulk_ops).
    stmts.iter().any(|stmt| match stmt {
        ir::Stmt::TryTable { .. } | ir::Stmt::Throw { .. } | ir::Stmt::ThrowRef { .. } => true,
        ir::Stmt::Block { body, .. } | ir::Stmt::Loop { body, .. } => {
            stmts_use_exception_handling(body)
        }
        ir::Stmt::If { then, els, .. } => {
            stmts_use_exception_handling(then) || stmts_use_exception_handling(els)
        }
        ir::Stmt::Assign { .. }
        | ir::Stmt::LocalSet { .. }
        | ir::Stmt::GlobalSet { .. }
        | ir::Stmt::Store { .. }
        | ir::Stmt::Br(_)
        | ir::Stmt::BrIf { .. }
        | ir::Stmt::BrTable { .. }
        | ir::Stmt::Return { .. }
        | ir::Stmt::Call { .. }
        | ir::Stmt::CallIndirect { .. }
        | ir::Stmt::MemoryGrow { .. }
        | ir::Stmt::MemoryCopy { .. }
        | ir::Stmt::MemoryFill { .. }
        | ir::Stmt::MemoryInit { .. }
        | ir::Stmt::DataDrop { .. }
        | ir::Stmt::TableInit { .. }
        | ir::Stmt::TableCopy { .. }
        | ir::Stmt::ElemDrop { .. }
        | ir::Stmt::TableSet { .. }
        | ir::Stmt::TableGrow { .. }
        | ir::Stmt::TableFill { .. }
        | ir::Stmt::ReturnCall { .. }
        | ir::Stmt::ReturnCallIndirect { .. }
        | ir::Stmt::HostBytesStore { .. }
        | ir::Stmt::HostListPush { .. }
        | ir::Stmt::Unreachable => false,
    })
}

/// Whether a function body contains a tail call. Public because backends
/// implementing tail calls also need it (e.g. Ruby's body/entry trampoline
/// split, ADR-18).
pub fn stmts_use_tail_calls(stmts: &[ir::Stmt]) -> bool {
    // Exhaustive on purpose (see stmts_use_table_bulk_ops).
    stmts.iter().any(|stmt| match stmt {
        ir::Stmt::ReturnCall { .. } | ir::Stmt::ReturnCallIndirect { .. } => true,
        ir::Stmt::Block { body, .. }
        | ir::Stmt::Loop { body, .. }
        | ir::Stmt::TryTable { body, .. } => stmts_use_tail_calls(body),
        ir::Stmt::If { then, els, .. } => stmts_use_tail_calls(then) || stmts_use_tail_calls(els),
        ir::Stmt::Assign { .. }
        | ir::Stmt::LocalSet { .. }
        | ir::Stmt::GlobalSet { .. }
        | ir::Stmt::Store { .. }
        | ir::Stmt::Br(_)
        | ir::Stmt::BrIf { .. }
        | ir::Stmt::BrTable { .. }
        | ir::Stmt::Return { .. }
        | ir::Stmt::Call { .. }
        | ir::Stmt::CallIndirect { .. }
        | ir::Stmt::MemoryGrow { .. }
        | ir::Stmt::MemoryCopy { .. }
        | ir::Stmt::MemoryFill { .. }
        | ir::Stmt::MemoryInit { .. }
        | ir::Stmt::DataDrop { .. }
        | ir::Stmt::TableInit { .. }
        | ir::Stmt::TableCopy { .. }
        | ir::Stmt::ElemDrop { .. }
        | ir::Stmt::TableSet { .. }
        | ir::Stmt::TableGrow { .. }
        | ir::Stmt::TableFill { .. }
        | ir::Stmt::Throw { .. }
        | ir::Stmt::ThrowRef { .. }
        | ir::Stmt::HostBytesStore { .. }
        | ir::Stmt::HostListPush { .. }
        | ir::Stmt::Unreachable => false,
    })
}

/// Whether the module contains any reference-types construct: a ref-typed
/// value anywhere (function signatures, globals, locals, temps), an
/// externref table (funcref tables are MVP), a reference constant in an
/// initializer, or one of the proposal's instructions. Same exhaustiveness
/// contract as `stmts_use_table_bulk_ops`.
fn module_uses_reference_types(module: &ir::Module) -> bool {
    let ref_ty = |ty: &ir::ValType| ty.is_ref();
    module
        .types
        .iter()
        .any(|t| t.params.iter().any(ref_ty) || t.results.iter().any(ref_ty))
        || module.imported_globals.iter().any(|g| g.ty.is_ref())
        || module.globals.iter().any(|g| g.ty.is_ref())
        || module
            .imported_tables
            .iter()
            .any(|t| t.ty == ir::ValType::ExternRef)
        || module.tables.iter().any(|t| t.ty == ir::ValType::ExternRef)
        || module
            .globals
            .iter()
            .any(|g| expr_uses_reference_types(&g.init))
        || module.elems.iter().any(|e| {
            e.items.iter().any(|i| matches!(i, ir::ElemItem::Global(_)))
                || match &e.kind {
                    ir::ElemKind::Active { offset, .. } => expr_uses_reference_types(offset),
                    ir::ElemKind::Passive | ir::ElemKind::Declared => false,
                }
        })
        || module.funcs.iter().any(|f| {
            f.locals.iter().any(ref_ty)
                || f.temps.iter().any(|t| t.ty.is_ref())
                || stmts_use_reference_types(&f.body)
        })
}

fn stmts_use_reference_types(stmts: &[ir::Stmt]) -> bool {
    // Exhaustive on purpose (see stmts_use_table_bulk_ops).
    let expr = expr_uses_reference_types;
    let target = |t: &ir::BrTarget| match t {
        ir::BrTarget::Return { values } => values.iter().any(expr),
        ir::BrTarget::Label { .. } => false,
    };
    stmts.iter().any(|stmt| match stmt {
        ir::Stmt::TableSet { .. } | ir::Stmt::TableGrow { .. } | ir::Stmt::TableFill { .. } => true,
        ir::Stmt::Block { body, .. }
        | ir::Stmt::Loop { body, .. }
        | ir::Stmt::TryTable { body, .. } => stmts_use_reference_types(body),
        ir::Stmt::If {
            cond, then, els, ..
        } => expr(cond) || stmts_use_reference_types(then) || stmts_use_reference_types(els),
        ir::Stmt::Assign { expr: e, .. }
        | ir::Stmt::LocalSet { expr: e, .. }
        | ir::Stmt::GlobalSet { expr: e, .. } => expr(e),
        ir::Stmt::Store { addr, value, .. } => expr(addr) || expr(value),
        ir::Stmt::Br(t) => target(t),
        ir::Stmt::BrIf { cond, target: t } => expr(cond) || target(t),
        ir::Stmt::BrTable {
            index,
            targets,
            default,
        } => expr(index) || targets.iter().any(&target) || target(default),
        ir::Stmt::Return { values } => values.iter().any(expr),
        ir::Stmt::Call { args, .. }
        | ir::Stmt::ReturnCall { args, .. }
        | ir::Stmt::Throw { args, .. } => args.iter().any(expr),
        ir::Stmt::ThrowRef { exn } => expr(exn),
        ir::Stmt::CallIndirect { index, args, .. }
        | ir::Stmt::ReturnCallIndirect { index, args, .. } => expr(index) || args.iter().any(expr),
        ir::Stmt::MemoryGrow { delta, .. } => expr(delta),
        ir::Stmt::MemoryCopy { dst, src, len }
        | ir::Stmt::MemoryInit { dst, src, len, .. }
        | ir::Stmt::TableInit { dst, src, len, .. }
        | ir::Stmt::TableCopy { dst, src, len, .. } => expr(dst) || expr(src) || expr(len),
        ir::Stmt::MemoryFill { dst, val, len } => expr(dst) || expr(val) || expr(len),
        ir::Stmt::HostBytesStore { addr, value } => expr(addr) || expr(value),
        ir::Stmt::HostListPush { list, value } => expr(list) || expr(value),
        ir::Stmt::DataDrop { .. } | ir::Stmt::ElemDrop { .. } | ir::Stmt::Unreachable => false,
    })
}

fn expr_uses_reference_types(expr: &ir::Expr) -> bool {
    // Exhaustive on purpose (see stmts_use_table_bulk_ops).
    let rec = expr_uses_reference_types;
    match expr {
        ir::Expr::RefNull(_)
        | ir::Expr::RefFunc(_)
        | ir::Expr::RefIsNull(_)
        | ir::Expr::TableGet { .. }
        | ir::Expr::TableSize(_) => true,
        ir::Expr::Un(_, a) => rec(a),
        ir::Expr::Bin(_, a, b) => rec(a) || rec(b),
        ir::Expr::Load { addr, .. } => rec(addr),
        ir::Expr::Select { cond, then, els } => rec(cond) || rec(then) || rec(els),
        ir::Expr::HostString { ptr, len } | ir::Expr::HostBytes { ptr, len } => {
            rec(ptr) || rec(len)
        }
        ir::Expr::HostByteLen(a)
        | ir::Expr::HostListLen(a)
        | ir::Expr::HostTupleGet { value: a, .. }
        | ir::Expr::HostField { value: a, .. }
        | ir::Expr::HostVariantCase { value: a, .. }
        | ir::Expr::HostVariantPayload(a)
        | ir::Expr::HostEnum { value: a, .. }
        | ir::Expr::HostEnumIndex { value: a, .. }
        | ir::Expr::HostBool(a)
        | ir::Expr::HostBoolToI32(a)
        | ir::Expr::HostChar(a)
        | ir::Expr::HostCharToI32(a)
        | ir::Expr::HostIsSome(a)
        | ir::Expr::HostSigned32(a)
        | ir::Expr::HostSigned64(a)
        | ir::Expr::HostMask32(a)
        | ir::Expr::HostMask64(a) => rec(a),
        ir::Expr::HostListGet { list, index } => rec(list) || rec(index),
        ir::Expr::HostTuple(es) => es.iter().any(rec),
        ir::Expr::HostRecord(fs) => fs.iter().any(|(_, e)| rec(e)),
        ir::Expr::HostVariant { payload, .. } => payload.as_deref().is_some_and(rec),
        ir::Expr::I32Const(_)
        | ir::Expr::I64Const(_)
        | ir::Expr::F32Const(_)
        | ir::Expr::F64Const(_)
        | ir::Expr::Temp(_)
        | ir::Expr::LocalGet(_)
        | ir::Expr::GlobalGet(_)
        | ir::Expr::HostListNew
        | ir::Expr::HostNone
        | ir::Expr::MemorySize => false,
    }
}

fn stmts_use_table_bulk_ops(stmts: &[ir::Stmt]) -> bool {
    // Exhaustive on purpose: a future body-carrying Stmt variant must
    // show up here as a compile error, not silently stop the recursion
    // (which would let an Unsupported backend mis-lower instead of
    // rejecting at conversion time, violating ADR-0).
    stmts.iter().any(|stmt| match stmt {
        ir::Stmt::TableInit { .. } | ir::Stmt::TableCopy { .. } | ir::Stmt::ElemDrop { .. } => true,
        ir::Stmt::Block { body, .. }
        | ir::Stmt::Loop { body, .. }
        | ir::Stmt::TryTable { body, .. } => stmts_use_table_bulk_ops(body),
        ir::Stmt::If { then, els, .. } => {
            stmts_use_table_bulk_ops(then) || stmts_use_table_bulk_ops(els)
        }
        ir::Stmt::Assign { .. }
        | ir::Stmt::LocalSet { .. }
        | ir::Stmt::GlobalSet { .. }
        | ir::Stmt::Store { .. }
        | ir::Stmt::Br(_)
        | ir::Stmt::BrIf { .. }
        | ir::Stmt::BrTable { .. }
        | ir::Stmt::Return { .. }
        | ir::Stmt::Call { .. }
        | ir::Stmt::CallIndirect { .. }
        | ir::Stmt::MemoryGrow { .. }
        | ir::Stmt::MemoryCopy { .. }
        | ir::Stmt::MemoryFill { .. }
        | ir::Stmt::MemoryInit { .. }
        | ir::Stmt::DataDrop { .. }
        | ir::Stmt::TableSet { .. }
        | ir::Stmt::TableGrow { .. }
        | ir::Stmt::TableFill { .. }
        | ir::Stmt::ReturnCall { .. }
        | ir::Stmt::ReturnCallIndirect { .. }
        | ir::Stmt::Throw { .. }
        | ir::Stmt::ThrowRef { .. }
        | ir::Stmt::HostBytesStore { .. }
        | ir::Stmt::HostListPush { .. }
        | ir::Stmt::Unreachable => false,
    })
}

/// The full WASI preview 1 surface, for the generated support docs; which
/// of these a backend implements is derived from its runtime units
/// (`bundler().has_unit("wasi/<name>")`).
pub const WASI_PREVIEW1_FUNCTIONS: &[&str] = &[
    "args_get",
    "args_sizes_get",
    "environ_get",
    "environ_sizes_get",
    "clock_res_get",
    "clock_time_get",
    "fd_advise",
    "fd_allocate",
    "fd_close",
    "fd_datasync",
    "fd_fdstat_get",
    "fd_fdstat_set_flags",
    "fd_fdstat_set_rights",
    "fd_filestat_get",
    "fd_filestat_set_size",
    "fd_filestat_set_times",
    "fd_pread",
    "fd_prestat_get",
    "fd_prestat_dir_name",
    "fd_pwrite",
    "fd_read",
    "fd_readdir",
    "fd_renumber",
    "fd_seek",
    "fd_sync",
    "fd_tell",
    "fd_write",
    "path_create_directory",
    "path_filestat_get",
    "path_filestat_set_times",
    "path_link",
    "path_open",
    "path_readlink",
    "path_remove_directory",
    "path_rename",
    "path_symlink",
    "path_unlink_file",
    "poll_oneoff",
    "proc_exit",
    "proc_raise",
    "random_get",
    "sched_yield",
    "sock_accept",
    "sock_recv",
    "sock_send",
    "sock_shutdown",
];

/// One runtime unit: a single method (or an inseparable scope prelude),
/// with its dependencies declared in `<comment> requires:` header lines.
pub struct RuntimeUnit {
    pub id: String,
    pub requires: Vec<String>,
    pub body: String,
}

/// A named scope units can live in (e.g. a class nested in the runtime
/// module). `prefix` is the unit-id path segment; `open`/`close` wrap the
/// scope's units; the root scope uses empty wrappers.
pub struct RuntimeScope {
    pub prefix: &'static str,
    pub open: &'static str,
    pub close: &'static str,
    /// Unit implicitly required by every unit of this scope (class
    /// skeleton, constants); also force-included for the root scope.
    pub prelude: Option<&'static str>,
}

/// Resolves `requires:` closures over runtime units and emits the bundle,
/// grouped by scope in declaration order, deterministically sorted within
/// a scope. Language-agnostic: syntax comes from the scopes and the
/// caller-provided wrapper around the whole bundle.
pub struct RuntimeBundler {
    scopes: Vec<RuntimeScope>,
    units: BTreeMap<String, RuntimeUnit>,
    indent_str: &'static str,
}

impl RuntimeBundler {
    pub fn new(
        comment_prefix: &str,
        indent_str: &'static str,
        scopes: Vec<RuntimeScope>,
        sources: &[(&str, &str)],
    ) -> Result<Self> {
        let requires_marker = format!("{comment_prefix} requires:");
        let mut units = BTreeMap::new();
        for (id, source) in sources {
            let mut requires = Vec::new();
            let mut body_lines = Vec::new();
            let mut in_header = true;
            for line in source.lines() {
                if in_header {
                    if let Some(rest) = line.strip_prefix(&requires_marker) {
                        for dep in rest.split(',') {
                            let dep = dep.trim();
                            if !dep.is_empty() {
                                requires.push(dep.to_string());
                            }
                        }
                        continue;
                    }
                    if line.trim().is_empty() {
                        continue;
                    }
                    in_header = false;
                }
                body_lines.push(line);
            }
            while body_lines.last().is_some_and(|l| l.trim().is_empty()) {
                body_lines.pop();
            }
            let unit = RuntimeUnit {
                id: id.to_string(),
                requires,
                body: body_lines.join("\n"),
            };
            if units.insert(unit.id.clone(), unit).is_some() {
                bail!("duplicate runtime unit {id}");
            }
        }
        let bundler = RuntimeBundler {
            scopes,
            units,
            indent_str,
        };
        for unit in bundler.units.values() {
            for dep in &unit.requires {
                if !bundler.units.contains_key(dep) {
                    bail!("unit {} requires unknown unit {dep}", unit.id);
                }
            }
            bundler.scope_of(&unit.id)?;
        }
        Ok(bundler)
    }

    fn scope_of(&self, id: &str) -> Result<&RuntimeScope> {
        let prefix = id.split('/').next().unwrap_or("");
        self.scopes
            .iter()
            .find(|s| s.prefix == prefix)
            .ok_or_else(|| anyhow::anyhow!("unit {id} has unknown scope {prefix}"))
    }

    pub fn has_unit(&self, id: &str) -> bool {
        self.units.contains_key(id)
    }

    pub fn units(&self) -> impl Iterator<Item = &RuntimeUnit> {
        self.units.values()
    }

    /// Compute the dependency closure of `seeds`, including scope preludes
    /// and the root scope's prelude.
    pub fn closure(&self, seeds: &BTreeSet<String>) -> Result<BTreeSet<String>> {
        let mut closure = BTreeSet::new();
        let mut queue: VecDeque<String> = seeds.iter().cloned().collect();
        if let Some(root_prelude) = self.scopes.first().and_then(|s| s.prelude) {
            queue.push_back(root_prelude.to_string());
        }
        while let Some(id) = queue.pop_front() {
            let unit = self
                .units
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("unknown runtime unit {id}"))?;
            if !closure.insert(id.clone()) {
                continue;
            }
            for dep in &unit.requires {
                queue.push_back(dep.clone());
            }
            if let Some(prelude) = self.scope_of(&id)?.prelude {
                queue.push_back(prelude.to_string());
            }
        }
        Ok(closure)
    }

    /// Emit the bundle for `seeds`' closure. `base_indent` is the indent
    /// level of the bundle's root-scope members (the caller wraps the
    /// result in the runtime module/namespace itself).
    pub fn bundle(&self, seeds: &BTreeSet<String>, base_indent: usize) -> Result<String> {
        let closure = self.closure(seeds)?;
        let mut out = String::new();
        for scope in &self.scopes {
            let mut ids: Vec<&String> = closure
                .iter()
                .filter(|id| id.split('/').next() == Some(scope.prefix))
                .collect();
            if ids.is_empty() {
                continue;
            }
            // Prelude first, the rest in sorted order.
            ids.sort_by_key(|id| (Some(id.as_str()) != scope.prelude, id.as_str()));
            let (open, body_indent) = if scope.open.is_empty() {
                ("", base_indent)
            } else {
                (scope.open, base_indent + 1)
            };
            if !open.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                self.push_line(&mut out, base_indent, open);
            }
            let mut first = open.is_empty() && out.is_empty();
            for id in ids {
                if !first {
                    out.push('\n');
                }
                first = false;
                for line in self.units[id.as_str()].body.lines() {
                    self.push_line(&mut out, body_indent, line);
                }
            }
            if !scope.close.is_empty() {
                self.push_line(&mut out, base_indent, scope.close);
            }
        }
        Ok(out)
    }

    pub fn bundle_all(&self, base_indent: usize) -> Result<String> {
        let seeds: BTreeSet<String> = self.units.keys().cloned().collect();
        self.bundle(&seeds, base_indent)
    }

    fn push_line(&self, out: &mut String, indent: usize, line: &str) {
        if line.trim().is_empty() {
            out.push('\n');
            return;
        }
        for _ in 0..indent {
            out.push_str(self.indent_str);
        }
        out.push_str(line);
        out.push('\n');
    }
}

/// Indentation-aware line writer.
pub struct CodeWriter {
    buf: String,
    indent: usize,
    indent_str: &'static str,
}

impl CodeWriter {
    pub fn new(indent_str: &'static str) -> Self {
        CodeWriter {
            buf: String::new(),
            indent: 0,
            indent_str,
        }
    }

    pub fn line(&mut self, s: impl AsRef<str>) {
        let s = s.as_ref();
        if s.is_empty() {
            self.buf.push('\n');
            return;
        }
        for _ in 0..self.indent {
            self.buf.push_str(self.indent_str);
        }
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    pub fn raw(&mut self, s: impl AsRef<str>) {
        self.buf.push_str(s.as_ref());
    }

    pub fn indent(&mut self) {
        self.indent += 1;
    }

    pub fn dedent(&mut self) {
        self.indent -= 1;
    }

    /// line(open); indent(); f(); dedent(); line(close)
    pub fn block(
        &mut self,
        open: impl AsRef<str>,
        close: impl AsRef<str>,
        f: impl FnOnce(&mut Self),
    ) {
        self.line(open);
        self.indent();
        f(self);
        self.dedent();
        self.line(close);
    }

    pub fn finish(self) -> String {
        self.buf
    }
}
