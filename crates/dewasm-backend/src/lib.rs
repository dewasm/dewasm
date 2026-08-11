//! Backend trait and code emission utilities shared by all language backends.

pub mod flat;

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{bail, Result};
use dewasm_core::feature::{Feature, UnsupportedError};
use dewasm_core::ir;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SupportStatus {
    Supported,
    Partial(&'static str),
    Unsupported,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Emit a module that is instantiated with an imports object and exposes its exports to the host language.
    Library,
    /// Emit a runnable program that wires up WASI and calls `_start`.
    Standalone,
}

/// How generated code gets its runtime. Generated code always refers to the runtime by the relative name `Rt` (or the backend's equivalent); linkage only decides where that name is defined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeLinkage {
    /// Nest the needed runtime units inside the generated module itself (single self-contained file; multiple generated artifacts never collide).
    Embedded,
    /// Emit only an alias to a runtime defined elsewhere (a shared bundle in the same program, or a future runtime package/gem).
    Alias(String),
}

#[derive(Clone, Debug)]
pub struct GenOptions {
    pub mode: Mode,
    /// Class/package/module name for the generated code, and the stem of the returned [`OutputFile`]'s name. In `Library` mode the backend validates it against its own grammar and uses it verbatim, with no sanitization. In `Standalone` mode the internal name is fixed per backend (`Program`/`program_`) and this only names the output file.
    pub module_name: String,
    pub runtime: RuntimeLinkage,
    /// Bundle the built-in WASI implementation as a fallback for `wasi_snapshot_preview1` imports the embedder does not provide. Disable to keep generated libraries free of ambient authority.
    pub default_wasi: bool,
    /// Externalize data-segment bytes into a binary sidecar instead of embedding them as hex literals. When `Some`, the backend emits load-from-sidecar code and returns a second `OutputFile` carrying the blob. Only backends that declare support honor it (the CLI rejects it for the rest); a backend that ignores it keeps embedding.
    pub data_file: Option<DataFileConfig>,
}

#[derive(Clone, Debug)]
pub struct DataFileConfig {
    /// The filename the generated program references relative to itself (e.g. via `__dir__`/`//go:embed`). The matching sidecar `OutputFile` carries this exact `name`; the CLI routes it to the requested path.
    pub sidecar_name: String,
}

pub struct OutputFile {
    pub name: String,
    pub contents: Vec<u8>,
}

pub trait Backend {
    fn name(&self) -> &str;
    fn file_extension(&self) -> &str;
    fn generate(&self, module: &ir::Module, opts: &GenOptions) -> anyhow::Result<Vec<OutputFile>>;

    /// Declared support per feature. The spec harness only tolerates skips attributable to features that are not `Supported`; flipping a feature to `Supported` makes its skips hard failures.
    fn feature_status(&self, feature: Feature) -> SupportStatus {
        let _ = feature;
        SupportStatus::Unsupported
    }

    /// Whether the backend bundles a WASI preview 1 runtime unit for `name` (e.g. `"fd_write"`). Feeds the generated support docs.
    fn has_wasi_p1(&self, name: &str) -> bool {
        let _ = name;
        false
    }
}

/// Reject, with the same `UnsupportedError` attribution the core converter uses, any construct the shared IR now represents but this specific `backend` has not declared `Supported`. The core builder is backend-agnostic and accepts every wasm-1.0-scoped construct; a backend that hasn't implemented one of them yet must refuse it itself, at conversion time, rather than mis-lower it.
pub fn check_module_support(backend: &dyn Backend, module: &ir::Module) -> Result<()> {
    // `used` is a closure so the usage scan (an IR walk for TableBulkOps) only runs for features the backend has *not* declared Supported.
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
    Ok(())
}

/// Reject a library-mode module name that does not fit `language`'s grammar. `grammar` is the prose form of the rule, quoted verbatim in the message: an invalid name is a conversion-time error, never a silent transformation, so the message must be enough to fix the invocation without reading the source.
pub fn module_name_error(language: &str, name: &str, grammar: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "invalid {language} module name {name:?}: it must be {grammar}. \
         Pass a valid one with --module-name (the default is the input file stem), \
         or drop --module-name for --mode standalone, whose internal name is fixed."
    )
}

/// Whether `seg` is a non-empty identifier whose first character satisfies `first` and whose remaining ones satisfy `rest`: the shape every backend's module-name grammar is built out of.
pub fn is_ident(seg: &str, first: fn(char) -> bool, rest: fn(char) -> bool) -> bool {
    let mut chars = seg.chars();
    match chars.next() {
        Some(c) if first(c) => chars.all(rest),
        _ => false,
    }
}

/// The view a wasm comparison imposes on its operands: what the stored representation must be read as before the target language's own operator answers the way wasm does. Integer `eq`/`ne` and every float comparison impose none: equal bit patterns compare equal under either view, and the languages' float comparison already matches wasm, NaN included.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompareOperands {
    Float,
    IntEq,
    Signed32,
    Signed64,
    Unsigned32,
    Unsigned64,
}

/// The comparison `op` performs, as the operator spelling every target language shares and the view its operands need; `None` for any other binary operation. Backends keep only the half that differs between them (how to spell a view their representation does not already provide) instead of each restating which `BinOp`s are comparisons.
pub fn comparison(op: ir::BinOp) -> Option<(&'static str, CompareOperands)> {
    use ir::BinOp::*;
    use CompareOperands::*;
    Some(match op {
        I32Eq | I64Eq => ("==", IntEq),
        I32Ne | I64Ne => ("!=", IntEq),
        F32Eq | F64Eq => ("==", Float),
        F32Ne | F64Ne => ("!=", Float),
        F32Lt | F64Lt => ("<", Float),
        F32Gt | F64Gt => (">", Float),
        F32Le | F64Le => ("<=", Float),
        F32Ge | F64Ge => (">=", Float),
        I32LtS => ("<", Signed32),
        I32GtS => (">", Signed32),
        I32LeS => ("<=", Signed32),
        I32GeS => (">=", Signed32),
        I64LtS => ("<", Signed64),
        I64GtS => (">", Signed64),
        I64LeS => ("<=", Signed64),
        I64GeS => (">=", Signed64),
        I32LtU => ("<", Unsigned32),
        I32GtU => (">", Unsigned32),
        I32LeU => ("<=", Unsigned32),
        I32GeU => (">=", Unsigned32),
        I64LtU => ("<", Unsigned64),
        I64GtU => (">", Unsigned64),
        I64LeU => ("<=", Unsigned64),
        I64GeU => (">=", Unsigned64),
        _ => return None,
    })
}

/// The operator and the signed view for a backend whose integers are stored masked-unsigned and whose native float comparison already answers wasm's way, NaN included: the operator from [`comparison`], paired with the runtime helper (`s32`/`s64`) the operands must go through, or `None` wherever the stored representation already compares correctly.
pub fn signed_view_rel_op(op: ir::BinOp) -> Option<(&'static str, Option<&'static str>)> {
    let (r, operands) = comparison(op)?;
    Some((
        r,
        match operands {
            CompareOperands::Signed32 => Some("s32"),
            CompareOperands::Signed64 => Some("s64"),
            _ => None,
        },
    ))
}

/// Whether `e` is a comparison or an `eqz`: the wasm expressions a backend's `cond` renders as a native boolean rather than as a `!= 0` test of a materialized 0/1, so a consumer can take that boolean directly.
pub fn is_boolean(e: &ir::Expr) -> bool {
    match e {
        ir::Expr::Un(ir::UnOp::I32Eqz | ir::UnOp::I64Eqz, _) => true,
        ir::Expr::Bin(op, ..) => comparison(*op).is_some(),
        _ => false,
    }
}

/// The maximal runs of consecutive locals that share `key`, as index ranges into `locals`: the grouping behind initializing a run in one statement. What a run is worth sharing differs per language (a rendered default value, a type name), so the caller supplies `key` and renders each range itself.
pub fn local_runs<K: PartialEq>(
    locals: &[ir::ValType],
    mut key: impl FnMut(ir::ValType) -> K,
) -> Vec<std::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut start = 0;
    while start < locals.len() {
        let k = key(locals[start]);
        let mut end = start + 1;
        while end < locals.len() && key(locals[end]) == k {
            end += 1;
        }
        runs.push(start..end);
        start = end;
    }
    runs
}

/// Whether `stmts` unconditionally leaves the current dispatch state, so a transition appended after it would be unreachable. A flat lowering appends a frame's exit transition on the way out, and for a body already ending in a `br`, `return` or `unreachable` that copy is dead. Deliberately conservative: answering `false` only re-emits the transition that used to be there unconditionally.
pub fn terminates(stmts: &[ir::Stmt]) -> bool {
    let Some(last) = stmts
        .iter()
        .rev()
        .find(|s| !matches!(s, ir::Stmt::SourceLine(_)))
    else {
        return false;
    };
    match last {
        ir::Stmt::Br(_) | ir::Stmt::Return { .. } | ir::Stmt::Unreachable => true,
        // Neither arm falling through means the `if` itself does not.
        ir::Stmt::If { then, els, .. } => !els.is_empty() && terminates(then) && terminates(els),
        _ => false,
    }
}

/// The runtime-unit name for `op`'s memory read (`i32_load8_u`, …), as the runtime units spell it. Shared because the unit names are: a backend that renames one renames the unit, not this table. Bash keeps its own copy, which deliberately routes float loads to the integer units.
pub fn load_method(op: ir::LoadOp) -> &'static str {
    use ir::LoadOp::*;
    match op {
        I32Load => "i32_load",
        I64Load => "i64_load",
        F32Load => "f32_load",
        F64Load => "f64_load",
        I32Load8S => "i32_load8_s",
        I32Load8U => "i32_load8_u",
        I32Load16S => "i32_load16_s",
        I32Load16U => "i32_load16_u",
        I64Load8S => "i64_load8_s",
        I64Load8U => "i64_load8_u",
        I64Load16S => "i64_load16_s",
        I64Load16U => "i64_load16_u",
        I64Load32S => "i64_load32_s",
        I64Load32U => "i64_load32_u",
    }
}

/// The runtime-unit name for `op`'s memory write, the [`load_method`] counterpart.
pub fn store_method(op: ir::StoreOp) -> &'static str {
    use ir::StoreOp::*;
    match op {
        I32Store => "i32_store",
        I64Store => "i64_store",
        F32Store => "f32_store",
        F64Store => "f64_store",
        I32Store8 => "i32_store8",
        I32Store16 => "i32_store16",
        I64Store8 => "i64_store8",
        I64Store16 => "i64_store16",
        I64Store32 => "i64_store32",
    }
}

/// A structural key for a function type: `params->results`, each value type spelled by `name_of`, e.g. `i32,i64->f32`. `call_indirect` compares types structurally and a table can be shared between separately generated artifacts, so the runtime type tag must not be a module-local index: those disagree across modules. A table is only ever shared between artifacts of *one* backend, so the spelling only has to be self-consistent per backend, which is why `name_of` stays the caller's.
pub fn type_key(ty: &ir::FuncType, name_of: fn(ir::ValType) -> &'static str) -> String {
    let names = |tys: &[ir::ValType]| {
        tys.iter()
            .map(|t| name_of(*t))
            .collect::<Vec<_>>()
            .join(",")
    };
    format!("{}->{}", names(&ty.params), names(&ty.results))
}

/// `data` as lowercase two-digit-per-byte hex, the payload every backend's embedded data-segment literal wraps (`pack("H*")`, `bytes.fromhex`, `Rt.unhex`). Written without a per-byte `format!`: a real app's segments run to megabytes, and CPython's alone would cost tens of millions of allocations.
pub fn hex_string(data: &[u8]) -> String {
    const DIGITS: [u8; 16] = *b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for b in data {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0xf) as usize] as char);
    }
    out
}

/// WASI import module names a bundled runtime answers for. `wasi_unstable` (snapshot 0) shares preview 1's ABI for everything implemented here except `fd_seek`'s whence encoding: a snapshot 0 module that actually seeks may misbehave, accepted until snapshot 0 gets its own units.
pub const WASI_MODULES: &[&str] = &["wasi_snapshot_preview1", "wasi_unstable"];

pub fn is_wasi_module(name: &str) -> bool {
    WASI_MODULES.contains(&name)
}

/// Whether the generated artifact carries the built-in WASI as an import fallback, and so takes the backend's args/env/preopens entry points, and its standalone main parses `--dir`. True when `default_wasi` is on and the module imports at least one WASI function `bundler` has a unit for.
pub fn wasi_bundled(module: &ir::Module, default_wasi: bool, bundler: &RuntimeBundler) -> bool {
    default_wasi
        && module
            .imported_funcs
            .iter()
            .any(|f| is_wasi_module(&f.module) && bundler.has_unit(&format!("wasi/{}", f.name)))
}

fn stmts_use_table_bulk_ops(stmts: &[ir::Stmt]) -> bool {
    // Exhaustive on purpose: a future body-carrying Stmt variant must show up here as a compile error, not silently stop the recursion (which would let an Unsupported backend mis-lower instead of rejecting at conversion time).
    stmts.iter().any(|stmt| match stmt {
        ir::Stmt::TableInit { .. } | ir::Stmt::TableCopy { .. } | ir::Stmt::ElemDrop { .. } => true,
        ir::Stmt::Block { body, .. } | ir::Stmt::Loop { body, .. } => {
            stmts_use_table_bulk_ops(body)
        }
        ir::Stmt::If { then, els, .. } => {
            stmts_use_table_bulk_ops(then) || stmts_use_table_bulk_ops(els)
        }
        ir::Stmt::SourceLine(_)
        | ir::Stmt::Assign { .. }
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
        | ir::Stmt::Unreachable => false,
    })
}

/// The full WASI preview 1 surface, for the generated support docs; which of these a backend implements is derived from its runtime units (`bundler().has_unit("wasi/<name>")`). The bool marks whether the function is in scope: `false` for the out-of-scope surface (sockets, `proc_raise`) that no toolchain output exercises and even wasmtime leaves unimplemented.
pub const WASI_PREVIEW1_FUNCTIONS: &[(&str, bool)] = &[
    ("args_get", true),
    ("args_sizes_get", true),
    ("environ_get", true),
    ("environ_sizes_get", true),
    ("clock_res_get", true),
    ("clock_time_get", true),
    ("fd_advise", true),
    ("fd_allocate", true),
    ("fd_close", true),
    ("fd_datasync", true),
    ("fd_fdstat_get", true),
    ("fd_fdstat_set_flags", true),
    ("fd_fdstat_set_rights", true),
    ("fd_filestat_get", true),
    ("fd_filestat_set_size", true),
    ("fd_filestat_set_times", true),
    ("fd_pread", true),
    ("fd_prestat_get", true),
    ("fd_prestat_dir_name", true),
    ("fd_pwrite", true),
    ("fd_read", true),
    ("fd_readdir", true),
    ("fd_renumber", true),
    ("fd_seek", true),
    ("fd_sync", true),
    ("fd_tell", true),
    ("fd_write", true),
    ("path_create_directory", true),
    ("path_filestat_get", true),
    ("path_filestat_set_times", true),
    ("path_link", true),
    ("path_open", true),
    ("path_readlink", true),
    ("path_remove_directory", true),
    ("path_rename", true),
    ("path_symlink", true),
    ("path_unlink_file", true),
    ("poll_oneoff", true),
    ("proc_exit", true),
    ("proc_raise", false),
    ("random_get", true),
    ("sched_yield", true),
    ("sock_accept", false),
    ("sock_recv", false),
    ("sock_send", false),
    ("sock_shutdown", false),
];

/// One runtime unit: a single method (or an inseparable scope prelude), with its dependencies declared in `<comment> requires:` header lines.
pub struct RuntimeUnit {
    pub id: String,
    pub requires: Vec<String>,
    pub body: String,
}

/// A named scope units can live in (e.g. a class nested in the runtime module). `prefix` is the unit-id path segment; `open`/`close` wrap the scope's units; the root scope uses empty wrappers.
pub struct RuntimeScope {
    pub prefix: &'static str,
    pub open: &'static str,
    pub close: &'static str,
    /// Unit implicitly required by every unit of this scope (class skeleton, constants); also force-included for the root scope.
    pub prelude: Option<&'static str>,
}

/// Resolves `requires:` closures over runtime units and emits the bundle, grouped by scope in declaration order, deterministically sorted within a scope. Language-agnostic: syntax comes from the scopes and the caller-provided wrapper around the whole bundle.
pub struct RuntimeBundler {
    scopes: Vec<RuntimeScope>,
    units: BTreeMap<String, RuntimeUnit>,
    indent_str: &'static str,
    unit_indent: usize,
}

impl RuntimeBundler {
    /// `indent_str` is one indent level of the emitted bundle; `unit_indent` is the width of one indent level *in the unit sources*, so that their own indentation can be re-emitted in `indent_str` (0 leaves body lines exactly as written).
    pub fn new(
        comment_prefix: &str,
        indent_str: &'static str,
        unit_indent: usize,
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
            unit_indent,
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

    /// Compute the dependency closure of `seeds`, including scope preludes and the root scope's prelude.
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

    /// Emit the bundle for `seeds`' closure. `base_indent` is the indent level of the bundle's root-scope members (the caller wraps the result in the runtime module/namespace itself).
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

    /// Emit one line at `indent` levels. A unit source is written space-indented at `unit_indent` per level; each such leading run becomes one `indent_str` here, so the bundle carries the caller's indentation style throughout instead of mixing it with the sources'. Spaces beyond the last whole run (an indentation that is not a multiple of `unit_indent`, e.g. an aligned continuation) are kept as spaces.
    fn push_line(&self, out: &mut String, indent: usize, line: &str) {
        if line.trim().is_empty() {
            out.push('\n');
            return;
        }
        let (levels, spaces, rest) = match self.unit_indent {
            0 => (0, 0, line),
            width => {
                let rest = line.trim_start_matches(' ');
                let leading = line.len() - rest.len();
                (leading / width, leading % width, rest)
            }
        };
        for _ in 0..indent + levels {
            out.push_str(self.indent_str);
        }
        for _ in 0..spaces {
            out.push(' ');
        }
        out.push_str(rest);
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
