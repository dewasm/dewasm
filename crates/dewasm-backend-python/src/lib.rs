//! Python backend: translates dewasm IR into a Python module (a class plus a
//! bundled lightweight runtime).
//!
//! Lowering conventions (ADR-28; numeric conventions ADR-2):
//! - i32/i64 are unsigned (masked) Python ints; signed views via `Rt.s32/s64`
//!   only where an instruction needs them.
//! - f32/f64 are Python floats; f32 results are re-rounded with `Rt.f32`.
//!   Float division goes through `Rt.fdiv` because Python raises on `x/0.0`.
//! - Python has no goto and caps nested loops/`try` at ~20 ("too many
//!   statically nested blocks"), while `if` nests ~100 deep. So only wasm
//!   loops become real `while True`; every forward branch (block/if exit)
//!   is lowered with a per-function branch register `_br` and guarded
//!   statements, and block bodies are spliced inline so block nesting adds
//!   no Python nesting (ADR-28).
//!
//! The runtime is composed from per-method units (ADR-6) and referenced by
//! the module-level name `Rt` (Python method scopes cannot see an enclosing
//! class scope, so the runtime lives at module top level, not nested in the
//! generated class as it is for Ruby).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::{
    check_module_support, Backend, CodeWriter, GenOptions, Mode, OutputFile, RuntimeBundler,
    RuntimeLinkage, RuntimeScope, SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, ElemItem, ElemKind, ExportKind, Expr, Func, LoadOp, Module, Stmt, StoreOp,
    Temp, UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The runtime unit bundler for Python (see runtime/python/units/).
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| {
        RuntimeBundler::new(
            "#",
            "    ",
            vec![
                RuntimeScope {
                    prefix: "rt",
                    open: "",
                    close: "",
                    prelude: Some("rt/_module"),
                },
                RuntimeScope {
                    prefix: "memory",
                    open: "class Memory:",
                    close: "",
                    prelude: Some("memory/_class"),
                },
                RuntimeScope {
                    prefix: "table",
                    open: "class Table:",
                    close: "",
                    prelude: Some("table/_class"),
                },
                RuntimeScope {
                    prefix: "global",
                    open: "class Global:",
                    close: "",
                    prelude: Some("global/_class"),
                },
                RuntimeScope {
                    prefix: "wasi",
                    open: "class WASI:",
                    close: "",
                    prelude: Some("wasi/_class"),
                },
            ],
            UNIT_SOURCES,
        )
        .expect("runtime units are well-formed")
    })
}

/// Emit a top-level shared runtime (`class Rt: ...`) for the closure of
/// `seeds`; generated classes then use `RuntimeLinkage::Alias("Rt")`.
pub fn shared_runtime(seeds: &BTreeSet<String>) -> Result<String> {
    Ok(format!("class Rt:\n{}", bundler().bundle(seeds, 1)?))
}

/// Locate a python3 interpreter (>= 3.9) able to run generated scripts.
/// Honors `$DEWASM_PYTHON`, then `python3`, then `python` (ADR-15: a missing
/// or too-old interpreter is a loud failure at the call site, not here — this
/// only reports what qualifies).
pub fn find_python() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("DEWASM_PYTHON") {
        candidates.push(PathBuf::from(env));
    }
    candidates.push(PathBuf::from("python3"));
    candidates.push(PathBuf::from("python"));
    for candidate in candidates {
        let Ok(out) = std::process::Command::new(&candidate)
            .args([
                "-c",
                "import sys; print(1 if sys.version_info >= (3, 9) else 0)",
            ])
            .output()
        else {
            continue;
        };
        if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "1" {
            return Some(candidate);
        }
    }
    None
}

/// Generate one class for `module`. Returns the class source and the set of
/// runtime units it needs (already bundled inside for `Embedded`).
pub fn generate_class_with_units(
    module: &Module,
    class_name: &str,
    linkage: &RuntimeLinkage,
    default_wasi: bool,
) -> Result<(String, BTreeSet<String>)> {
    generate_class_inner(module, class_name, linkage, default_wasi, &BTreeSet::new())
}

fn generate_class_inner(
    module: &Module,
    class_name: &str,
    linkage: &RuntimeLinkage,
    default_wasi: bool,
    extra_seeds: &BTreeSet<String>,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&PythonBackend, module)?;
    let gen = Gen {
        module,
        default_wasi,
        uses: RefCell::new(extra_seeds.clone()),
    };
    let mut wb = CodeWriter::new("    ");
    gen.class(&mut wb, class_name);
    let body = wb.finish();
    let uses = gen.uses.into_inner();

    let mut out = String::new();
    match linkage {
        RuntimeLinkage::Embedded => {
            if !uses.is_empty() {
                out.push_str("class Rt:\n");
                out.push_str(&bundler().bundle(&uses, 1)?);
                out.push_str("\n\n");
            }
        }
        RuntimeLinkage::Alias(path) => {
            out.push_str(&format!("Rt = {path}\n\n\n"));
        }
    }
    out.push_str(&body);
    Ok((out, uses))
}

pub struct PythonBackend;

impl Backend for PythonBackend {
    fn name(&self) -> &str {
        "python"
    }

    fn file_extension(&self) -> &str {
        "py"
    }

    fn has_wasi_p1(&self, name: &str) -> bool {
        bundler().has_unit(&format!("wasi/{name}"))
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // Python floats are IEEE doubles; f32 re-rounding and the NaN
            // paths follow ADR-2 (mirroring Ruby).
            Feature::Floats => SupportStatus::Supported,
            // The wasm-1.0 completion (ADR-16 model): boxed globals, imported
            // globals/memories/tables, multiple tables, and the table half of
            // bulk memory.
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        let class_name = class_name(&opts.module_name);

        // The Exit/Trap handlers in the standalone main need these even when
        // the module itself never references them.
        let mut extra_seeds = BTreeSet::new();
        if opts.mode == Mode::Standalone {
            extra_seeds.insert("rt/trap".to_string());
            extra_seeds.insert("rt/exit".to_string());
        }

        let (class_src, _) = generate_class_inner(
            module,
            &class_name,
            &opts.runtime,
            opts.default_wasi,
            &extra_seeds,
        )?;

        let mut w = CodeWriter::new("    ");
        w.line("# Generated by dewasm. Do not edit.");
        w.line("import math");
        w.line("import os");
        w.line("import struct");
        w.line("import sys");
        w.line("");
        w.line("");
        w.raw(&class_src);

        if opts.mode == Mode::Standalone {
            let wasi_kwargs = wasi_bundled(module, opts.default_wasi);
            w.line("");
            w.line("");
            w.line("if __name__ == \"__main__\":");
            w.indent();
            if wasi_kwargs {
                // DEWASM_PREOPEN maps guest paths to host directories for
                // standalone runs, e.g. "/=./data,/tmp=/tmp"; kept out of argv
                // since that mirrors the guest's own argv.
                w.line("_pre = {}");
                w.line("for _kv in os.environ.get(\"DEWASM_PREOPEN\", \"\").split(\",\"):");
                w.indent();
                w.line("if \"=\" in _kv:");
                w.indent();
                w.line("_g, _h = _kv.split(\"=\", 1)");
                w.line("_pre[_g] = _h");
                w.dedent();
                w.dedent();
                w.line(format!(
                    "_inst = {class_name}({{}}, args=[os.path.basename(sys.argv[0])] + sys.argv[1:], env=dict(os.environ), preopens=_pre)"
                ));
            } else {
                w.line(format!("_inst = {class_name}()"));
            }
            w.line("try:");
            w.indent();
            w.line("_inst.invoke(\"_start\")");
            w.line("sys.exit(0)");
            w.dedent();
            w.line("except Rt.Exit as _e:");
            w.indent();
            w.line("sys.exit(_e.code)");
            w.dedent();
            w.line("except Rt.Trap as _e:");
            w.indent();
            w.line("sys.stderr.write(\"trap: %s\\n\" % _e)");
            w.line("sys.exit(134)");
            w.dedent();
            w.dedent();
        }

        Ok(vec![OutputFile {
            name: format!("{}.py", opts.module_name),
            contents: w.finish(),
        }])
    }
}

fn class_name(module_name: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in module_name.chars() {
        if c.is_ascii_alphanumeric() {
            if upper {
                out.extend(c.to_uppercase());
                upper = false;
            } else {
                out.push(c);
            }
        } else {
            upper = true;
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "Wasm");
    }
    out
}

/// Python double-quoted string literal.
fn py_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn hex_bytes(data: &[u8]) -> String {
    let mut hex = String::with_capacity(data.len() * 2);
    for b in data {
        hex.push_str(&format!("{b:02x}"));
    }
    format!("bytes.fromhex(\"{hex}\")")
}

/// WASI import module names the bundled runtime answers for. `wasi_unstable`
/// (snapshot 0) shares preview 1's ABI for everything implemented here.
const WASI_MODULES: &[&str] = &["wasi_snapshot_preview1", "wasi_unstable"];

fn is_wasi_module(name: &str) -> bool {
    WASI_MODULES.contains(&name)
}

pub use dewasm_backend::WASI_PREVIEW1_FUNCTIONS;

/// Whether the generated class bundles the built-in WASI as an import
/// fallback (and therefore takes `args`/`env`/`preopens` keyword arguments).
fn wasi_bundled(module: &Module, default_wasi: bool) -> bool {
    default_wasi
        && module
            .imported_funcs
            .iter()
            .any(|f| is_wasi_module(&f.module) && bundler().has_unit(&format!("wasi/{}", f.name)))
}

fn temp(t: Temp) -> String {
    format!("s{}", t.depth)
}

fn default_value(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 | ValType::I64 => "0",
        ValType::F32 | ValType::F64 => "0.0",
        ValType::FuncRef => "None",
    }
}

struct Gen<'a> {
    module: &'a Module,
    default_wasi: bool,
    /// Runtime units the generated code references.
    uses: RefCell<BTreeSet<String>>,
}

impl<'a> Gen<'a> {
    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    /// Reference a module-level runtime helper, recording its unit.
    fn rt(&self, name: &str) -> String {
        self.use_unit(&format!("rt/{name}"));
        format!("Rt.{name}")
    }

    fn resolve_import_string(&self, kind: &str, module: &str, name: &str) -> String {
        self.use_unit("rt/resolve_import");
        self.use_unit("rt/check_import_kind");
        format!(
            "Rt.check_import_kind(Rt.resolve_import(imports, {}, {}), {}, {}, {})",
            py_string(module),
            py_string(name),
            py_string(kind),
            py_string(module),
            py_string(name)
        )
    }

    fn class(&self, w: &mut CodeWriter, class_name: &str) {
        w.line(format!("class {class_name}:"));
        w.indent();
        self.constants(w);
        w.line("");
        self.initialize(w);
        w.line("");
        self.helpers(w);
        for (i, func) in self.module.funcs.iter().enumerate() {
            w.line("");
            let idx = self.module.num_imported_funcs() as usize + i;
            self.function(w, idx as u32, func);
        }
        w.dedent();
    }

    fn constants(&self, w: &mut CodeWriter) {
        let m = self.module;
        let mut global_exports: Vec<(String, u32)> = Vec::new();
        let mut table_exports: Vec<(String, u32)> = Vec::new();
        let mut memory_export_names: Vec<String> = Vec::new();
        for export in &m.exports {
            match export.kind {
                ExportKind::Global(idx) => global_exports.push((export.name.clone(), idx)),
                ExportKind::Table(idx) => table_exports.push((export.name.clone(), idx)),
                ExportKind::Memory => memory_export_names.push(export.name.clone()),
                ExportKind::Func(_) => {}
            }
        }
        let entries = |pairs: &[(String, u32)], prefix: &str| {
            pairs
                .iter()
                .map(|(name, idx)| {
                    format!(
                        "{}: {}",
                        py_string(name),
                        py_string(&format!("{prefix}{idx}"))
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        w.line(format!(
            "GLOBAL_EXPORTS = {{{}}}",
            entries(&global_exports, "g")
        ));
        w.line(format!(
            "TABLE_EXPORTS = {{{}}}",
            entries(&table_exports, "t")
        ));
        let mem_set = memory_export_names
            .iter()
            .map(|n| py_string(n))
            .collect::<Vec<_>>()
            .join(", ");
        // `{...}` is a set literal; an empty one must be `set()`.
        if mem_set.is_empty() {
            w.line("MEMORY_EXPORTS = set()");
        } else {
            w.line(format!("MEMORY_EXPORTS = {{{mem_set}}}"));
        }
    }

    fn initialize(&self, w: &mut CodeWriter) {
        let m = self.module;
        let wasi_fallback = wasi_bundled(m, self.default_wasi);
        let header = if wasi_fallback {
            "def __init__(self, imports=None, args=None, env=None, preopens=None):"
        } else {
            "def __init__(self, imports=None):"
        };
        w.line(header);
        w.indent();
        w.line("if imports is None:");
        w.indent();
        w.line("imports = {}");
        w.dedent();
        if wasi_fallback {
            w.line("self._wasi = None");
            w.line("self._wasi_args = args if args is not None else []");
            w.line("self._wasi_env = env if env is not None else {}");
            w.line("self._wasi_preopens = preopens if preopens is not None else {}");
        }

        if let Some(import) = &m.imported_memory {
            w.line(format!(
                "self.memory = {} or self._missing({}, {})",
                self.resolve_import_string("memory", &import.module, &import.name),
                py_string(&import.module),
                py_string(&import.name),
            ));
        } else if let Some(mem) = &m.memory {
            self.use_unit("memory/_class");
            let max = mem
                .max_pages
                .map(|p| p.to_string())
                .unwrap_or_else(|| "None".to_string());
            w.line(format!(
                "self.memory = Rt.Memory({}, {})",
                mem.min_pages, max
            ));
        } else {
            w.line("self.memory = None");
        }

        for (i, import) in m.imported_tables.iter().enumerate() {
            w.line(format!(
                "self.t{i} = {} or self._missing({}, {})",
                self.resolve_import_string("table", &import.module, &import.name),
                py_string(&import.module),
                py_string(&import.name),
            ));
        }
        let num_imported_tables = m.num_imported_tables();
        for (i, table) in m.tables.iter().enumerate() {
            self.use_unit("table/_class");
            let idx = num_imported_tables as usize + i;
            let max = table
                .max
                .map(|p| p.to_string())
                .unwrap_or_else(|| "None".to_string());
            w.line(format!("self.t{idx} = Rt.Table({}, {max})", table.min));
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            // Fallback order (ADR-7): explicit import -> bundled WASI unit
            // (constructed lazily) -> ENOSYS stub; non-WASI imports stay
            // mandatory (a missing one is a link error).
            let fallback = if is_wasi_module(&import.module) && self.default_wasi {
                let unit = format!("wasi/{}", import.name);
                if bundler().has_unit(&unit) {
                    self.use_unit(&unit);
                    self.use_unit("wasi/_class");
                    format!("self._wasi_import({})", py_string(&import.name))
                } else {
                    "(lambda *_a: 52)".to_string() // ENOSYS: not implemented yet
                }
            } else {
                self.use_unit("rt/link_error");
                format!(
                    "self._missing({}, {})",
                    py_string(&import.module),
                    py_string(&import.name)
                )
            };
            w.line(format!(
                "self.if{i} = {} or {fallback}",
                self.resolve_import_string("func", &import.module, &import.name)
            ));
        }

        for (i, import) in m.imported_globals.iter().enumerate() {
            w.line(format!(
                "self.g{i} = {} or self._missing({}, {})",
                self.resolve_import_string("global", &import.module, &import.name),
                py_string(&import.module),
                py_string(&import.name),
            ));
        }
        let num_imported_globals = m.imported_globals.len();
        for (i, global) in m.globals.iter().enumerate() {
            self.use_unit("global/_class");
            let idx = num_imported_globals + i;
            w.line(format!(
                "self.g{idx} = Rt.Global({})",
                self.expr(&global.init)
            ));
        }

        for (i, elem) in m.elems.iter().enumerate() {
            let items = || {
                elem.items
                    .iter()
                    .map(|item| match item {
                        ElemItem::Func(func_idx) => self.func_pair(*func_idx),
                        ElemItem::Null => "None".to_string(),
                        ElemItem::Global(idx) => format!("self.g{idx}.value"),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            match &elem.kind {
                ElemKind::Declared => w.line(format!("self.elem{i} = []")),
                ElemKind::Passive => w.line(format!("self.elem{i} = [{}]", items())),
                ElemKind::Active {
                    table_index,
                    offset,
                } => {
                    self.use_unit("table/init");
                    w.line(format!("self.elem{i} = [{}]", items()));
                    let offset = self.expr(offset);
                    w.line(format!(
                        "self.t{table_index}.init({offset}, self.elem{i}, 0, {})",
                        elem.items.len()
                    ));
                    w.line(format!("self.elem{i} = []"));
                }
            }
        }

        for (i, data) in m.datas.iter().enumerate() {
            match &data.offset {
                Some(offset) => {
                    self.use_unit("memory/init");
                    w.line(format!(
                        "self.memory.init({}, {}, 0, {})",
                        self.expr(offset),
                        hex_bytes(&data.data),
                        data.data.len()
                    ));
                    w.line(format!("self.data{i} = b\"\""));
                }
                None => {
                    w.line(format!("self.data{i} = {}", hex_bytes(&data.data)));
                }
            }
        }

        let mut export_entries = Vec::new();
        for export in &m.exports {
            if let ExportKind::Func(idx) = export.kind {
                export_entries.push(format!(
                    "{}: {}",
                    py_string(&export.name),
                    self.func_ref(idx)
                ));
            }
        }
        w.line(format!("self.exports = {{{}}}", export_entries.join(", ")));

        // Let import providers bind to the fully-constructed instance (ADR-7).
        if !m.imported_funcs.is_empty() {
            w.line("for _p in imports.values():");
            w.indent();
            w.line("if hasattr(_p, \"attach\"):");
            w.indent();
            w.line("_p.attach(self)");
            w.dedent();
            w.dedent();
        }
        if wasi_fallback {
            w.line("if self._wasi is not None:");
            w.indent();
            w.line("self._wasi.attach(self)");
            w.dedent();
        }

        if let Some(start) = m.start {
            w.line(self.call_string(start, &[]));
        }
        w.dedent();
    }

    fn helpers(&self, w: &mut CodeWriter) {
        w.line("def invoke(self, name, *args):");
        w.indent();
        w.line("return self.exports[name](*args)");
        w.dedent();
        w.line("");
        w.line("def global_get(self, name):");
        w.indent();
        w.line("return getattr(self, self.GLOBAL_EXPORTS[name]).value");
        w.dedent();
        w.line("");
        // The boxed Rt.Global itself (not its current value), for a host
        // embedder or another dewasm instance to import as a shared mutable
        // cell (ADR-16).
        w.line("def global_export(self, name):");
        w.indent();
        w.line("return getattr(self, self.GLOBAL_EXPORTS[name])");
        w.dedent();
        w.line("");
        w.line("def table_export(self, name):");
        w.indent();
        w.line("return getattr(self, self.TABLE_EXPORTS[name])");
        w.dedent();
        w.line("");
        // ADR-7 provider protocol: an instance is itself a valid import value.
        w.line("def wasm_import(self, name):");
        w.indent();
        w.line("if name in self.exports:");
        w.indent();
        w.line("return self.exports[name]");
        w.dedent();
        w.line("if name in self.GLOBAL_EXPORTS:");
        w.indent();
        w.line("return getattr(self, self.GLOBAL_EXPORTS[name])");
        w.dedent();
        w.line("if name in self.TABLE_EXPORTS:");
        w.indent();
        w.line("return getattr(self, self.TABLE_EXPORTS[name])");
        w.dedent();
        w.line("if name in self.MEMORY_EXPORTS:");
        w.indent();
        w.line("return self.memory");
        w.dedent();
        w.line("return None");
        w.dedent();
        w.line("");
        w.line("def _missing(self, mod, name):");
        w.indent();
        w.line("raise Rt.LinkError(\"missing import %s.%s\" % (mod, name))");
        w.dedent();
        if wasi_bundled(self.module, self.default_wasi) {
            w.line("");
            w.line("def _wasi_import(self, name):");
            w.indent();
            w.line("if self._wasi is None:");
            w.indent();
            w.line("self._wasi = Rt.WASI(args=self._wasi_args, env=self._wasi_env, preopens=self._wasi_preopens)");
            w.dedent();
            w.line("return getattr(self._wasi, \"wasi_\" + name)");
            w.dedent();
        }
    }

    fn func_type_symbol(&self, func_idx: u32) -> String {
        let idx = func_idx as usize;
        let imports = self.module.imported_funcs.len();
        let ty = if idx < imports {
            self.module.imported_funcs[idx].type_idx
        } else {
            self.module.funcs[idx - imports].type_idx
        };
        self.type_symbol(ty)
    }

    /// A structural key for a function type (not a module-local index), so a
    /// table shared across modules stays consistent.
    fn type_symbol(&self, type_idx: u32) -> String {
        let ty = &self.module.types[type_idx as usize];
        let names = |tys: &[ValType]| {
            tys.iter()
                .map(|t| match t {
                    ValType::I32 => "i32",
                    ValType::I64 => "i64",
                    ValType::F32 => "f32",
                    ValType::F64 => "f64",
                    ValType::FuncRef => "funcref",
                })
                .collect::<Vec<_>>()
                .join(",")
        };
        py_string(&format!("{}->{}", names(&ty.params), names(&ty.results)))
    }

    fn func_ref(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("self.if{func_idx}")
        } else {
            format!("self._f{func_idx}")
        }
    }

    /// A funcref value: the `[type_key, callable]` pair tables store (ADR-16).
    fn func_pair(&self, func_idx: u32) -> String {
        format!(
            "[{}, {}]",
            self.func_type_symbol(func_idx),
            self.func_ref(func_idx)
        )
    }

    fn call_string(&self, func_idx: u32, args: &[String]) -> String {
        let args = args.join(", ");
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("self.if{func_idx}({args})")
        } else {
            format!("self._f{func_idx}({args})")
        }
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &Func) {
        let ty = &self.module.types[func.type_idx as usize];
        let mut params = String::new();
        for i in 0..ty.params.len() {
            params.push_str(&format!(", l{i}"));
        }
        w.line(format!("def _f{idx}(self{params}):"));
        w.indent();
        for (i, local_ty) in func.locals.iter().enumerate() {
            let name = format!("l{}", ty.params.len() + i);
            w.line(format!("{name} = {}", default_value(*local_ty)));
        }
        // Temps default to 0; every temp is assigned before any reachable read
        // (valid wasm), so the value only guards against Python NameError.
        let mut depths: Vec<u32> = func.temps.iter().map(|t| t.depth).collect();
        depths.dedup();
        if !depths.is_empty() {
            let decl = depths
                .iter()
                .map(|d| format!("s{d}"))
                .collect::<Vec<_>>()
                .join(" = ");
            w.line(format!("{decl} = 0"));
        }
        let needs_br = seq_has_label_branch(&func.body);
        if needs_br {
            w.line("_br = 0");
        }
        let mut guarded = false;
        self.emit_seq(w, &func.body, &mut guarded);
        w.dedent();
    }

    /// Emit a statement sequence, threading the compile-time `guarded` flag
    /// (whether a preceding statement may have left a branch pending in `_br`).
    /// Block/Loop bodies are spliced inline so block nesting adds no Python
    /// nesting; only real loops become `while` (ADR-28).
    fn emit_seq(&self, w: &mut CodeWriter, stmts: &[Stmt], guarded: &mut bool) {
        for stmt in stmts {
            match stmt {
                Stmt::Block { label, body } => {
                    let before = *guarded;
                    self.emit_seq(w, body, guarded);
                    if label.referenced {
                        w.line(format!("if _br == {}:", label.id));
                        w.indent();
                        w.line("_br = 0");
                        w.dedent();
                    }
                    *guarded = before || !stmt_free_targets(stmt).is_empty();
                }
                Stmt::Loop { label, body } => {
                    if label.referenced {
                        let before = *guarded;
                        w.line("while True:");
                        w.indent();
                        let mut inner = before;
                        self.emit_seq(w, body, &mut inner);
                        w.line(format!("if _br == {}:", label.id));
                        w.indent();
                        w.line("_br = 0");
                        w.line("continue");
                        w.dedent();
                        w.line("break");
                        w.dedent();
                        *guarded = before || !stmt_free_targets(stmt).is_empty();
                    } else {
                        // No br targets this loop, so it never repeats.
                        self.emit_seq(w, body, guarded);
                    }
                }
                Stmt::If {
                    label,
                    cond,
                    then,
                    els,
                } => {
                    let before = *guarded;
                    self.emit_if(w, before, cond, then, els);
                    if label.referenced {
                        w.line(format!("if _br == {}:", label.id));
                        w.indent();
                        w.line("_br = 0");
                        w.dedent();
                    }
                    *guarded = before || !stmt_free_targets(stmt).is_empty();
                }
                _ => {
                    if *guarded {
                        w.line("if _br == 0:");
                        w.indent();
                        self.simple_stmt(w, stmt);
                        w.dedent();
                    } else {
                        self.simple_stmt(w, stmt);
                    }
                    if !stmt_free_targets(stmt).is_empty() {
                        *guarded = true;
                    }
                }
            }
        }
    }

    fn emit_if(&self, w: &mut CodeWriter, guarded: bool, cond: &Expr, then: &[Stmt], els: &[Stmt]) {
        let cond_s = self.expr(cond);
        // When guarded, `_br == 0 and ...` short-circuits so `cond` (which may
        // trap on a load) is not evaluated while a branch is pending.
        if guarded {
            w.line(format!("if _br == 0 and ({cond_s}) != 0:"));
        } else {
            w.line(format!("if ({cond_s}) != 0:"));
        }
        w.indent();
        if then.is_empty() {
            w.line("pass");
        } else {
            let mut it = false;
            self.emit_seq(w, then, &mut it);
        }
        w.dedent();
        if !els.is_empty() {
            if guarded {
                w.line("elif _br == 0:");
            } else {
                w.line("else:");
            }
            w.indent();
            let mut ie = false;
            self.emit_seq(w, els, &mut ie);
            w.dedent();
        }
    }

    fn simple_stmt(&self, w: &mut CodeWriter, stmt: &Stmt) {
        match stmt {
            Stmt::Assign { dst, expr } => {
                w.line(format!("{} = {}", temp(*dst), self.expr(expr)));
            }
            Stmt::LocalSet { idx, expr } => {
                w.line(format!("l{idx} = {}", self.expr(expr)));
            }
            Stmt::GlobalSet { idx, expr } => {
                w.line(format!("self.g{idx}.value = {}", self.expr(expr)));
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                w.line(format!(
                    "self.memory.{}({}, {})",
                    self.mem(store_method(*op)),
                    self.addr(addr, *offset),
                    self.expr(value)
                ));
            }
            Stmt::Br(target) => self.branch(w, target),
            Stmt::BrIf { cond, target } => {
                w.line(format!("if ({}) != 0:", self.expr(cond)));
                w.indent();
                self.branch(w, target);
                w.dedent();
            }
            Stmt::BrTable {
                index,
                targets,
                default,
            } => {
                if targets.is_empty() {
                    self.branch(w, default);
                    return;
                }
                w.line(format!("_i = {}", self.expr(index)));
                for (n, target) in targets.iter().enumerate() {
                    let kw = if n == 0 { "if" } else { "elif" };
                    w.line(format!("{kw} _i == {n}:"));
                    w.indent();
                    self.branch(w, target);
                    w.dedent();
                }
                w.line("else:");
                w.indent();
                self.branch(w, default);
                w.dedent();
            }
            Stmt::Return { values } => self.return_stmt(w, values),
            Stmt::Call {
                func,
                args,
                results,
            } => {
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                let call = self.call_string(*func, &args);
                w.line(assign_results(results, call));
            }
            Stmt::CallIndirect {
                type_idx,
                table_index,
                index,
                args,
                results,
            } => {
                self.use_unit("table/call");
                let mut call_args = vec![self.expr(index), self.type_symbol(*type_idx)];
                call_args.extend(args.iter().map(|a| self.expr(a)));
                let call = format!("self.t{table_index}.call({})", call_args.join(", "));
                w.line(assign_results(results, call));
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                w.line(format!(
                    "{} = self.memory.grow({})",
                    temp(*dst),
                    self.expr(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "self.memory.copy({}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "self.memory.fill({}, {}, {})",
                    self.expr(dst),
                    self.expr(val),
                    self.expr(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "self.memory.init({}, self.data{seg}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::DataDrop { seg } => {
                w.line(format!("self.data{seg} = b\"\""));
            }
            Stmt::Unreachable => {
                w.line(format!("{}(\"unreachable\")", self.rt("trap")));
            }
            Stmt::TableInit {
                seg,
                table_index,
                dst,
                src,
                len,
            } => {
                self.use_unit("table/init");
                w.line(format!(
                    "self.t{table_index}.init({}, self.elem{seg}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::TableCopy {
                dst_table,
                src_table,
                dst,
                src,
                len,
            } => {
                self.use_unit("table/copy");
                w.line(format!(
                    "self.t{dst_table}.copy({}, self.t{src_table}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::ElemDrop { seg } => {
                w.line(format!("self.elem{seg} = []"));
            }
            // Handled in emit_seq, never routed here.
            Stmt::Block { .. } | Stmt::Loop { .. } | Stmt::If { .. } => {
                unreachable!("structured statement routed to simple_stmt")
            }
        }
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        match values {
            [] => w.line("return"),
            [v] => w.line(format!("return {}", self.expr(v))),
            vs => {
                let vs = vs
                    .iter()
                    .map(|v| self.expr(v))
                    .collect::<Vec<_>>()
                    .join(", ");
                w.line(format!("return ({vs})"));
            }
        }
    }

    fn branch(&self, w: &mut CodeWriter, target: &BrTarget) {
        match target {
            BrTarget::Return { values } => self.return_stmt(w, values),
            BrTarget::Label { label, assigns, .. } => {
                for (dst, src) in assigns {
                    w.line(format!("{} = {}", temp(*dst), temp(*src)));
                }
                // is_loop is irrelevant here: the loop trailer turns `_br ==
                // <loop id>` into a `continue`; a block/if exit is handled by
                // the guards skipping to the label's reset marker (ADR-28).
                w.line(format!("_br = {label}"));
            }
        }
    }

    /// Reference a Memory method, recording its unit.
    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
    }

    fn addr(&self, addr: &Expr, offset: u64) -> String {
        if offset == 0 {
            self.expr(addr)
        } else {
            format!("{} + {offset}", self.expr(addr))
        }
    }

    fn expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::I32Const(v) => v.to_string(),
            Expr::I64Const(v) => v.to_string(),
            Expr::F32Const(bits) => {
                let v = f32::from_bits(*bits);
                if v.is_finite() {
                    py_float(v as f64)
                } else {
                    format!("{}(0x{bits:x})", self.rt("f32_from_bits"))
                }
            }
            Expr::F64Const(bits) => {
                let v = f64::from_bits(*bits);
                if v.is_finite() {
                    py_float(v)
                } else {
                    format!("{}(0x{bits:x})", self.rt("f64_from_bits"))
                }
            }
            Expr::Temp(t) => temp(*t),
            Expr::LocalGet(idx) => format!("l{idx}"),
            Expr::GlobalGet(idx) => format!("self.g{idx}.value"),
            Expr::Un(op, a) => self.un(*op, &self.expr(a)),
            Expr::Bin(op, a, b) => self.bin(*op, &self.expr(a), &self.expr(b)),
            Expr::Load { op, addr, offset } => {
                format!(
                    "self.memory.{}({})",
                    self.mem(load_method(*op)),
                    self.addr(addr, *offset)
                )
            }
            Expr::Select { cond, then, els } => {
                format!(
                    "({} if ({}) != 0 else {})",
                    self.expr(then),
                    self.expr(cond),
                    self.expr(els)
                )
            }
            Expr::MemorySize => {
                self.use_unit("memory/size");
                "self.memory.size()".to_string()
            }
        }
    }

    fn un(&self, op: UnOp, a: &str) -> String {
        use UnOp::*;
        match op {
            I32Eqz | I64Eqz => format!("(1 if {a} == 0 else 0)"),
            I32Clz => format!("{}({a})", self.rt("i32_clz")),
            I32Ctz => format!("{}({a})", self.rt("i32_ctz")),
            I64Clz => format!("{}({a})", self.rt("i64_clz")),
            I64Ctz => format!("{}({a})", self.rt("i64_ctz")),
            I32Popcnt | I64Popcnt => format!("{}({a})", self.rt("popcnt")),
            F32Abs => format!("{}({a})", self.rt("f32_abs")),
            F32Neg => format!("{}({a})", self.rt("f32_neg")),
            F64Abs => format!("{}({a})", self.rt("f64_abs")),
            F64Neg => format!("{}({a})", self.rt("f64_neg")),
            F32Ceil | F64Ceil => format!("{}({a})", self.rt("fceil")),
            F32Floor | F64Floor => format!("{}({a})", self.rt("ffloor")),
            F32Trunc | F64Trunc => format!("{}({a})", self.rt("ftrunc")),
            F32Nearest | F64Nearest => format!("{}({a})", self.rt("fnearest")),
            F32Sqrt => format!("{}({}({a}))", self.rt("f32"), self.rt("fsqrt")),
            F64Sqrt => format!("{}({a})", self.rt("fsqrt")),
            I32WrapI64 => format!("({a} & 0xFFFFFFFF)"),
            I32TruncF32S | I32TruncF64S => format!("{}({a})", self.rt("i32_trunc_s")),
            I32TruncF32U | I32TruncF64U => format!("{}({a})", self.rt("i32_trunc_u")),
            I64TruncF32S | I64TruncF64S => format!("{}({a})", self.rt("i64_trunc_s")),
            I64TruncF32U | I64TruncF64U => format!("{}({a})", self.rt("i64_trunc_u")),
            I32TruncSatF32S | I32TruncSatF64S => format!("{}({a})", self.rt("i32_trunc_sat_s")),
            I32TruncSatF32U | I32TruncSatF64U => format!("{}({a})", self.rt("i32_trunc_sat_u")),
            I64TruncSatF32S | I64TruncSatF64S => format!("{}({a})", self.rt("i64_trunc_sat_s")),
            I64TruncSatF32U | I64TruncSatF64U => format!("{}({a})", self.rt("i64_trunc_sat_u")),
            I64ExtendI32S => format!("{}({a})", self.rt("i64_extend_i32_s")),
            I64ExtendI32U => a.to_string(),
            F32ConvertI32S => format!("{}({}({a}).__float__())", self.rt("f32"), self.rt("s32")),
            F32ConvertI32U => format!("{}(float({a}))", self.rt("f32")),
            F32ConvertI64S => format!("{}({}({a}))", self.rt("cvt_f32_i"), self.rt("s64")),
            F32ConvertI64U => format!("{}({a})", self.rt("cvt_f32_i")),
            F64ConvertI32S => format!("float({}({a}))", self.rt("s32")),
            F64ConvertI32U => format!("float({a})"),
            F64ConvertI64S => format!("{}({}({a}))", self.rt("cvt_f64_i"), self.rt("s64")),
            F64ConvertI64U => format!("{}({a})", self.rt("cvt_f64_i")),
            F32DemoteF64 => format!("{}({a})", self.rt("f32_demote")),
            F64PromoteF32 => format!("{}({a})", self.rt("f64_promote")),
            I32ReinterpretF32 => format!("{}({a})", self.rt("i32_reinterpret_f32")),
            I64ReinterpretF64 => format!("{}({a})", self.rt("i64_reinterpret_f64")),
            F32ReinterpretI32 => format!("{}({a})", self.rt("f32_reinterpret_i32")),
            F64ReinterpretI64 => format!("{}({a})", self.rt("f64_reinterpret_i64")),
            I32Extend8S => format!("{}({a})", self.rt("i32_extend8_s")),
            I32Extend16S => format!("{}({a})", self.rt("i32_extend16_s")),
            I64Extend8S => format!("{}({a})", self.rt("i64_extend8_s")),
            I64Extend16S => format!("{}({a})", self.rt("i64_extend16_s")),
            I64Extend32S => format!("{}({a})", self.rt("i64_extend32_s")),
        }
    }

    fn bin(&self, op: BinOp, a: &str, b: &str) -> String {
        use BinOp::*;
        match op {
            I32Add => format!("(({a} + {b}) & 0xFFFFFFFF)"),
            I32Sub => format!("(({a} - {b}) & 0xFFFFFFFF)"),
            I32Mul => format!("(({a} * {b}) & 0xFFFFFFFF)"),
            I64Add => format!("(({a} + {b}) & 0xFFFFFFFFFFFFFFFF)"),
            I64Sub => format!("(({a} - {b}) & 0xFFFFFFFFFFFFFFFF)"),
            I64Mul => format!("(({a} * {b}) & 0xFFFFFFFFFFFFFFFF)"),
            I32DivS => format!("{}({a}, {b})", self.rt("i32_div_s")),
            I32DivU => format!("{}({a}, {b})", self.rt("i32_div_u")),
            I32RemS => format!("{}({a}, {b})", self.rt("i32_rem_s")),
            I32RemU => format!("{}({a}, {b})", self.rt("i32_rem_u")),
            I64DivS => format!("{}({a}, {b})", self.rt("i64_div_s")),
            I64DivU => format!("{}({a}, {b})", self.rt("i64_div_u")),
            I64RemS => format!("{}({a}, {b})", self.rt("i64_rem_s")),
            I64RemU => format!("{}({a}, {b})", self.rt("i64_rem_u")),
            I32And | I64And => format!("({a} & {b})"),
            I32Or | I64Or => format!("({a} | {b})"),
            I32Xor | I64Xor => format!("({a} ^ {b})"),
            I32Shl => format!("(({a} << ({b} & 31)) & 0xFFFFFFFF)"),
            I32ShrU => format!("({a} >> ({b} & 31))"),
            I32ShrS => format!("(({}({a}) >> ({b} & 31)) & 0xFFFFFFFF)", self.rt("s32")),
            I64Shl => format!("(({a} << ({b} & 63)) & 0xFFFFFFFFFFFFFFFF)"),
            I64ShrU => format!("({a} >> ({b} & 63))"),
            I64ShrS => format!(
                "(({}({a}) >> ({b} & 63)) & 0xFFFFFFFFFFFFFFFF)",
                self.rt("s64")
            ),
            I32Rotl => format!("{}({a}, {b})", self.rt("i32_rotl")),
            I32Rotr => format!("{}({a}, {b})", self.rt("i32_rotr")),
            I64Rotl => format!("{}({a}, {b})", self.rt("i64_rotl")),
            I64Rotr => format!("{}({a}, {b})", self.rt("i64_rotr")),
            I32Eq | I64Eq => format!("(1 if {a} == {b} else 0)"),
            I32Ne | I64Ne => format!("(1 if {a} != {b} else 0)"),
            I32LtU | I64LtU => format!("(1 if {a} < {b} else 0)"),
            I32GtU | I64GtU => format!("(1 if {a} > {b} else 0)"),
            I32LeU | I64LeU => format!("(1 if {a} <= {b} else 0)"),
            I32GeU | I64GeU => format!("(1 if {a} >= {b} else 0)"),
            I32LtS => format!("(1 if {0}({a}) < {0}({b}) else 0)", self.rt("s32")),
            I32GtS => format!("(1 if {0}({a}) > {0}({b}) else 0)", self.rt("s32")),
            I32LeS => format!("(1 if {0}({a}) <= {0}({b}) else 0)", self.rt("s32")),
            I32GeS => format!("(1 if {0}({a}) >= {0}({b}) else 0)", self.rt("s32")),
            I64LtS => format!("(1 if {0}({a}) < {0}({b}) else 0)", self.rt("s64")),
            I64GtS => format!("(1 if {0}({a}) > {0}({b}) else 0)", self.rt("s64")),
            I64LeS => format!("(1 if {0}({a}) <= {0}({b}) else 0)", self.rt("s64")),
            I64GeS => format!("(1 if {0}({a}) >= {0}({b}) else 0)", self.rt("s64")),
            F32Add => format!("{}({a} + {b})", self.rt("f32")),
            F32Sub => format!("{}({a} - {b})", self.rt("f32")),
            F32Mul => format!("{}({a} * {b})", self.rt("f32")),
            F32Div => format!("{}({}({a}, {b}))", self.rt("f32"), self.rt("fdiv")),
            F64Add => format!("({a} + {b})"),
            F64Sub => format!("({a} - {b})"),
            F64Mul => format!("({a} * {b})"),
            F64Div => format!("{}({a}, {b})", self.rt("fdiv")),
            F32Min | F64Min => format!("{}({a}, {b})", self.rt("fmin")),
            F32Max | F64Max => format!("{}({a}, {b})", self.rt("fmax")),
            F32Copysign => format!("{}({a}, {b})", self.rt("f32_copysign")),
            F64Copysign => format!("{}({a}, {b})", self.rt("f64_copysign")),
            F32Eq | F64Eq => format!("(1 if {a} == {b} else 0)"),
            F32Ne | F64Ne => format!("(1 if {a} != {b} else 0)"),
            F32Lt | F64Lt => format!("(1 if {a} < {b} else 0)"),
            F32Gt | F64Gt => format!("(1 if {a} > {b} else 0)"),
            F32Le | F64Le => format!("(1 if {a} <= {b} else 0)"),
            F32Ge | F64Ge => format!("(1 if {a} >= {b} else 0)"),
        }
    }
}

/// A Python float literal that round-trips to the same double. `{:?}` on f64
/// gives the shortest round-tripping decimal, which Python's `float()` parses
/// back exactly; only the spelling of infinities/`e` notation differs, and
/// non-finite values never reach here.
fn py_float(v: f64) -> String {
    format!("{v:?}")
}

fn assign_results(results: &[Temp], call: String) -> String {
    match results {
        [] => call,
        [r] => format!("{} = {}", temp(*r), call),
        rs => {
            let names = rs.iter().map(|r| temp(*r)).collect::<Vec<_>>().join(", ");
            format!("{names} = {call}")
        }
    }
}

/// Whether `stmts` contains any branch to a label (as opposed to a return),
/// i.e. whether the function needs the `_br` branch register at all.
fn seq_has_label_branch(stmts: &[Stmt]) -> bool {
    stmts.iter().any(stmt_has_label_branch)
}

fn stmt_has_label_branch(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Br(t) | Stmt::BrIf { target: t, .. } => matches!(t, BrTarget::Label { .. }),
        Stmt::BrTable {
            targets, default, ..
        } => {
            matches!(default, BrTarget::Label { .. })
                || targets.iter().any(|t| matches!(t, BrTarget::Label { .. }))
        }
        Stmt::Block { body, .. } | Stmt::Loop { body, .. } => seq_has_label_branch(body),
        Stmt::If { then, els, .. } => seq_has_label_branch(then) || seq_has_label_branch(els),
        _ => false,
    }
}

/// The set of label ids a statement branches to that are *not* bound within
/// it. Non-empty means the statement may leave `_br` set on fall-through,
/// so following siblings must be guarded (ADR-28).
fn stmt_free_targets(stmt: &Stmt) -> BTreeSet<u32> {
    match stmt {
        Stmt::Br(t) | Stmt::BrIf { target: t, .. } => target_free(t),
        Stmt::BrTable {
            targets, default, ..
        } => {
            let mut s = target_free(default);
            for t in targets {
                s.extend(target_free(t));
            }
            s
        }
        Stmt::Block { label, body } | Stmt::Loop { label, body } => {
            let mut s = seq_free_targets(body);
            s.remove(&label.id);
            s
        }
        Stmt::If {
            label, then, els, ..
        } => {
            let mut s = seq_free_targets(then);
            s.extend(seq_free_targets(els));
            s.remove(&label.id);
            s
        }
        _ => BTreeSet::new(),
    }
}

fn seq_free_targets(stmts: &[Stmt]) -> BTreeSet<u32> {
    let mut s = BTreeSet::new();
    for stmt in stmts {
        s.extend(stmt_free_targets(stmt));
    }
    s
}

fn target_free(t: &BrTarget) -> BTreeSet<u32> {
    match t {
        BrTarget::Return { .. } => BTreeSet::new(),
        BrTarget::Label { label, .. } => {
            let mut s = BTreeSet::new();
            s.insert(*label);
            s
        }
    }
}

fn load_method(op: LoadOp) -> &'static str {
    use LoadOp::*;
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

fn store_method(op: StoreOp) -> &'static str {
    use StoreOp::*;
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
