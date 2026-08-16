//! Python backend: translates dewasm IR into a Python module (a class plus a bundled lightweight runtime).
//!
//! Lowering conventions:
//! - i32/i64 are unsigned (masked) Python ints; signed views via `Rt.s32/s64` only where an instruction needs them.
//! - f32/f64 are Python floats; f32 results are re-rounded with `Rt.f32`.
//!   Float division goes through `Rt.fdiv` because Python raises on `x/0.0`.
//! - Python has no goto and caps nested loops/`try` at ~20 ("too many statically nested blocks"), while `if` nests ~100 deep.
//!   So only wasm loops become real `while True`; every forward branch (block/if exit) is lowered with a per-function branch register `_br` and guarded statements, and block bodies are spliced inline so block nesting adds no Python nesting.
//! - A branch crossing many frames pays one test per crossed frame under that register, so a function holding a deep enough crossing has those frames dissolved into a state machine instead (see [`flat`]); shallower branches keep the register, in the same function.
//!
//! The runtime is composed from per-method units and referenced by a module-level class name (Python method scopes cannot see an enclosing class scope, so the runtime lives at module top level, not nested in the generated class as it is for Ruby).
//! Under `Embedded` linkage that name is per-artifact (`<Class>Rt`), so two generated artifacts in one namespace keep independent runtimes; `Alias` linkage keeps the shared `Rt`.

/// Flat dispatch, shared with the Ruby backend ([`dewasm_backend::flat`]); only the threshold below is this backend's own.
mod flat {
    pub use dewasm_backend::flat::*;

    /// Crossing depth from which a branch is worth a dispatch.
    ///
    /// Ruby's calibrated constant, kept after measuring it against the binary-tree dispatch this backend emits (`emit_dispatch_tree`): a transition costs O(log2 states) compares (~11 for the largest machine here, 1463 states), against ~16+ region checks for the relay it replaces.
    /// Measured on converted apps (CPython 3.14, 2 runs each, ±0.2 s): the sqlite3 shell's query-heavy workload runs 1.42x faster than the relay-only lowering (23.4 s → 16.5 s) and the packed CRuby boot is at parity (17.0 s → 16.6 s).
    /// A *linear* dispatch at this threshold measured 1.22x *slower* than the relay on the same boot (largest machine ~700 compares per transition), which is why the tree shape, not the Ruby `case`'s O(1) probe or an `elif`/`match` chain, carries this constant's calibration.
    pub const DEEP_CROSSING: usize = 16;
}

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::masking::{
    bin_operand_context, elides_mask, eq_const_rewrite, fold_and_chain, shift_count_mode,
    shift_width, un_operand_context, MaskContext, ShiftCountMode,
};
use dewasm_backend::{
    check_module_support, hex_string, is_boolean, is_ident, is_wasi_module, load_code, local_runs,
    module_name_error, signed_view_rel_op, store_code, terminates, type_key, wasi_bundled, Backend,
    CodeWriter, GenOptions, Mode, OutputFile, RuntimeBundler, RuntimeLinkage, RuntimeScope,
    SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, CatchClause, ElemItem, ElemKind, ExportKind, Expr, Func, Module, Stmt, Temp,
    UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The runtime unit bundler for Python (see runtime/python/units/).
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| {
        RuntimeBundler::new(
            "#",
            "\t",
            4,
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

/// Emit a top-level shared runtime (`class Rt: ...`) for the closure of `seeds`; generated classes then use `RuntimeLinkage::Alias("Rt")`.
pub fn shared_runtime(seeds: &BTreeSet<String>) -> Result<String> {
    Ok(format!("class Rt:\n{}", bundler().bundle(seeds, 1)?))
}

/// Locate a python3 interpreter (>= 3.9) able to run generated scripts.
/// Honors `$DEWASM_PYTHON`, then `python3`, then `python` (a missing or too-old interpreter is a loud failure at the call site, not here: this only reports what qualifies).
pub fn find_python() -> Option<std::path::PathBuf> {
    static PYTHON: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    PYTHON.get_or_init(find_python_uncached).clone()
}

/// The probe behind [`find_python`], memoized there: it spawns a process per call, and the interpreter cannot change under a running process.
fn find_python_uncached() -> Option<std::path::PathBuf> {
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

/// Returns the class source and the set of runtime units it needs (already bundled inside for `Embedded`).
pub fn generate_class_with_units(
    module: &Module,
    class_name: &str,
    linkage: &RuntimeLinkage,
    default_wasi: bool,
) -> Result<(String, BTreeSet<String>)> {
    generate_class_inner(
        module,
        class_name,
        linkage,
        default_wasi,
        &BTreeSet::new(),
        None,
    )
}

fn generate_class_inner(
    module: &Module,
    class_name: &str,
    linkage: &RuntimeLinkage,
    default_wasi: bool,
    extra_seeds: &BTreeSet<String>,
    data_file: Option<&str>,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&PythonBackend, module)?;
    // Prefix sums: `data_offsets[i]` is where segment `i` begins in the concatenated data-file blob.
    // Only consulted when externalizing.
    let mut data_offsets = Vec::with_capacity(module.datas.len());
    let mut acc = 0usize;
    for data in &module.datas {
        data_offsets.push(acc);
        acc += data.data.len();
    }
    let rt_name = runtime_name(class_name, linkage);
    let gen = Gen {
        module,
        default_wasi,
        uses: RefCell::new(extra_seeds.clone()),
        flat: RefCell::new(None),
        data_file: data_file.map(str::to_string),
        data_offsets,
        rt_name: rt_name.clone(),
    };
    let mut wb = CodeWriter::new("\t");
    gen.class(&mut wb, class_name);
    let body = wb.finish();
    let uses = gen.uses.into_inner();

    let mut out = String::new();
    match linkage {
        RuntimeLinkage::Embedded => {
            if !uses.is_empty() {
                // Namespace the runtime under the generated class: the units reference the runtime only as the module global `Rt.<name>`, never inside a string literal (the units lint enforces that), so one textual replace moves every reference onto the per-artifact name.
                out.push_str(&format!("class {rt_name}:\n"));
                out.push_str(
                    &bundler()
                        .bundle(&uses, 1)?
                        .replace("Rt.", &format!("{rt_name}.")),
                );
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
            // Python floats are IEEE doubles; f32 re-rounding and the NaN paths mirror Ruby's numeric conventions.
            Feature::Floats => SupportStatus::Supported,
            // Wasm-1.0 completion (imported globals/memories/tables, multiple tables, table bulk ops) mirrors Ruby's model.
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
            // Tags are identity objects (fresh instances, compared with `is`), a thrown exception is a native Python exception that doubles as the exnref, and traps stay uncatchable: the same model as Ruby's.
            Feature::ExceptionHandling => SupportStatus::Supported,
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        // Standalone output is a self-contained program: its class name is fixed, not derived.
        // Library output uses the requested name verbatim, after validating it.
        let class_name = if opts.mode == Mode::Standalone {
            STANDALONE_CLASS.to_string()
        } else {
            check_module_name(&opts.module_name)?;
            opts.module_name.clone()
        };
        let rt_name = runtime_name(&class_name, &opts.runtime);

        // The Exit/Trap handlers in the standalone main need these even when the module itself never references them.
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
            opts.data_file.as_ref().map(|c| c.data_file_name.as_str()),
        )?;

        let mut w = CodeWriter::new("\t");
        w.line("# Generated by dewasm. Do not edit.");
        w.line("import errno");
        w.line("import math");
        w.line("import os");
        w.line("import select");
        w.line("import struct");
        w.line("import sys");
        if opts.mode == Mode::Standalone {
            // For the guest thread in the standalone entrypoint.
            w.line("import threading");
        }
        w.line("import time");
        // Externalized data blob: read once at import time from the data file next to this module, then sliced by the generated `DATA_BLOB[o:o+len]` expressions.
        // Only emitted when there is data to externalize (otherwise the generated code never reads it).
        if let Some(cfg) = &opts.data_file {
            if !module.datas.is_empty() {
                w.line("");
                w.line(format!(
                    "DATA_BLOB = open(os.path.join(os.path.dirname(__file__), {}), \"rb\").read()",
                    py_string(&cfg.data_file_name)
                ));
            }
        }
        w.line("");
        w.line("");
        w.raw(&class_src);

        if opts.mode == Mode::Standalone {
            let wasi_kwargs = wasi_bundled(module, opts.default_wasi, bundler());
            w.line("");
            w.line("");
            w.line("if __name__ == \"__main__\":");
            w.indent();
            if wasi_kwargs {
                // Parse the standalone runtime interface: a leading run of `--dir HOST::GUEST` flags mounts host directories at guest paths (wasmtime-style), stopping at `--` or the first non-flag token; the rest is the guest's argv[1..].
                w.line("_pre = {}");
                w.line("_argv = sys.argv[1:]");
                w.line("_i = 0");
                w.line("while _i < len(_argv):");
                w.indent();
                w.line("_a = _argv[_i]");
                w.line("if _a == \"--\":");
                w.indent();
                w.line("_i += 1");
                w.line("break");
                w.dedent();
                w.line("elif _a == \"--dir\" or _a.startswith(\"--dir=\"):");
                w.indent();
                w.line("if _a == \"--dir\":");
                w.indent();
                w.line("_i += 1");
                w.line("if _i >= len(_argv):");
                w.indent();
                w.line("sys.stderr.write(\"--dir requires a HOST::GUEST argument\\n\")");
                w.line("sys.exit(1)");
                w.dedent();
                w.line("_spec = _argv[_i]");
                w.dedent();
                w.line("else:");
                w.indent();
                w.line("_spec = _a[6:]");
                w.dedent();
                w.line("_h, _sep, _g = _spec.partition(\"::\")");
                w.line("_pre[_g if _sep else _h] = _h");
                w.line("_i += 1");
                w.dedent();
                w.line("else:");
                w.indent();
                w.line("break");
                w.dedent();
                w.dedent();
                w.line(format!(
                    "_inst = {class_name}({{}}, args=[os.path.basename(sys.argv[0])] + _argv[_i:], env=dict(os.environ), preopens=_pre)"
                ));
            } else {
                w.line(format!("_inst = {class_name}()"));
            }
            // Run the guest on a big-stack thread with a raised recursion limit.
            // A standalone guest may recurse arbitrarily deep for real work, so the sizing is generous: 1e6 frames at the ~1 KiB of C stack CPython <= 3.10 spends per frame.
            // The thread relays exceptions back so proc_exit/traps still exit via the main thread; daemon so Ctrl-C during `join` still terminates.
            w.line("_err = []");
            w.line("");
            w.line("def _run():");
            w.indent();
            w.line("try:");
            w.indent();
            w.line("_inst.invoke(\"_start\")");
            w.dedent();
            w.line("except BaseException as _e:");
            w.indent();
            w.line("_err.append(_e)");
            w.dedent();
            w.dedent();
            w.line("");
            w.line("sys.setrecursionlimit(1000000)");
            w.line("try:");
            w.indent();
            w.line("threading.stack_size(512 * 1024 * 1024)");
            w.dedent();
            w.line("except (ValueError, OverflowError, RuntimeError):");
            w.indent();
            w.line("pass");
            w.dedent();
            w.line("_t = threading.Thread(target=_run, daemon=True)");
            w.line("_t.start()");
            w.line("_t.join()");
            w.line("try:");
            w.indent();
            w.line("if _err:");
            w.indent();
            w.line("raise _err[0]");
            w.dedent();
            w.line("sys.exit(0)");
            w.dedent();
            w.line(format!("except {rt_name}.Exit as _e:"));
            w.indent();
            w.line("sys.exit(_e.code)");
            w.dedent();
            w.line(format!("except {rt_name}.Trap as _e:"));
            w.indent();
            w.line("sys.stderr.write(\"trap: %s\\n\" % _e)");
            w.line("sys.exit(134)");
            w.dedent();
            w.dedent();
        }

        let mut files = vec![OutputFile {
            name: format!("{}.py", opts.module_name),
            contents: w.finish().into_bytes(),
        }];
        // The data file: every segment's bytes concatenated in segment order, matching the `data_offsets` prefix sums baked into the generated `DATA_BLOB[o:o+len]` slices.
        // Only emitted when there is data to externalize (otherwise the generated code never reads it).
        if let Some(cfg) = &opts.data_file {
            if !module.datas.is_empty() {
                let mut blob = Vec::new();
                for data in &module.datas {
                    blob.extend_from_slice(&data.data);
                }
                files.push(OutputFile {
                    name: cfg.data_file_name.clone(),
                    contents: blob,
                });
            }
        }
        Ok(files)
    }
}

/// The module-level name the generated class references its runtime by.
/// `Embedded` output is self-contained and must survive sharing a namespace with another artifact, so it gets a per-artifact `<Class>Rt`; `Alias` output points at a runtime someone else emitted, which is always the shared `Rt`.
fn runtime_name(class_name: &str, linkage: &RuntimeLinkage) -> String {
    match linkage {
        RuntimeLinkage::Embedded => format!("{class_name}Rt"),
        RuntimeLinkage::Alias(_) => "Rt".to_string(),
    }
}

/// The class a `--mode standalone` program defines: fixed, since nothing outside a self-contained program observes it.
pub const STANDALONE_CLASS: &str = "Program";

/// The library-mode module name must be a single Python identifier and is used verbatim.
/// Everything the backend emits lives in one module (no package split is ever produced, so a dotted name would have nothing to mean), hence no separator.
fn check_module_name(name: &str) -> Result<()> {
    if is_ident(
        name,
        |c| c.is_ascii_alphabetic() || c == '_',
        |c| c.is_ascii_alphanumeric() || c == '_',
    ) {
        Ok(())
    } else {
        Err(module_name_error(
            "python",
            name,
            "a single identifier matching [A-Za-z_][A-Za-z0-9_]* (e.g. Add, sqlite3)",
        ))
    }
}

/// Python double-quoted string literal.
pub fn py_string(s: &str) -> String {
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
    format!("bytes.fromhex(\"{}\")", hex_string(data))
}

pub use dewasm_backend::WASI_PREVIEW1_FUNCTIONS;

fn temp(t: Temp) -> String {
    format!("s{}", t.depth)
}

fn default_value(ty: ValType) -> &'static str {
    dewasm_backend::default_value(ty, "None")
}

/// How a value type is spelled inside a structural type key ([`type_key`]): the shared wasm spelling.
fn val_name(ty: ValType) -> &'static str {
    dewasm_backend::val_name(ty)
}

struct Gen<'a> {
    module: &'a Module,
    default_wasi: bool,
    /// Runtime units the generated code references.
    uses: RefCell<BTreeSet<String>>,
    /// Flat-dispatch plan for the function being emitted, when it holds a deep enough crossing (see [`flat`]).
    /// `branch()` consults it to emit `_state = N; continue` instead of setting the branch register.
    flat: RefCell<Option<flat::Plan>>,
    /// When `Some`, data segments are externalized into a binary data file of this filename (loaded once into the module-level `DATA_BLOB`) instead of embedded as `bytes.fromhex` literals; `data_offsets[i]` locates segment `i` in the blob.
    data_file: Option<String>,
    data_offsets: Vec<usize>,
    /// The module-level name of the runtime this artifact references (see [`runtime_name`]).
    rt_name: String,
}

impl<'a> Gen<'a> {
    /// The Python expression yielding a data segment's bytes: a slice of the externalized `DATA_BLOB` when `--data-file` is on, else an inline `bytes.fromhex` literal.
    /// Both yield a `bytes` object.
    fn data_expr(&self, seg: usize, data: &[u8]) -> String {
        if self.data_file.is_some() {
            let o = self.data_offsets[seg];
            format!("DATA_BLOB[{o}:{}]", o + data.len())
        } else {
            hex_bytes(data)
        }
    }

    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    /// Reference a module-level runtime helper, recording its unit.
    fn rt(&self, name: &str) -> String {
        self.use_unit(&format!("rt/{name}"));
        format!("{}.{name}", self.rt_name)
    }

    fn resolve_import_string(&self, kind: &str, module: &str, name: &str) -> String {
        self.use_unit("rt/resolve_import");
        self.use_unit("rt/check_import_kind");
        let rt = &self.rt_name;
        format!(
            "{rt}.check_import_kind({rt}.resolve_import(imports, {}, {}), {}, {}, {})",
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
        let mut tag_exports: Vec<(String, u32)> = Vec::new();
        let mut memory_export_names: Vec<String> = Vec::new();
        for export in &m.exports {
            match export.kind {
                ExportKind::Global(idx) => global_exports.push((export.name.clone(), idx)),
                ExportKind::Table(idx) => table_exports.push((export.name.clone(), idx)),
                ExportKind::Tag(idx) => tag_exports.push((export.name.clone(), idx)),
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
        w.line(format!(
            "TAG_EXPORTS = {{{}}}",
            entries(&tag_exports, "tag")
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
        let wasi_fallback = wasi_bundled(m, self.default_wasi, bundler());
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
                "self.m = {} or self._missing({}, {})",
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
                "self.m = {}.Memory({}, {})",
                self.rt_name, mem.min_pages, max
            ));
        } else {
            w.line("self.m = None");
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
            w.line(format!(
                "self.t{idx} = {}.Table({}, {max})",
                self.rt_name, table.min
            ));
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            // Fallback order: explicit import -> bundled WASI unit (constructed lazily) -> ENOSYS stub; non-WASI imports stay mandatory (a missing one is a link error).
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
                "self.g{idx} = {}.Global({})",
                self.rt_name,
                self.masked(&global.init)
            ));
        }

        for (i, import) in m.imported_tags.iter().enumerate() {
            w.line(format!(
                "self.tag{i} = {} or self._missing({}, {})",
                self.resolve_import_string("tag", &import.module, &import.name),
                py_string(&import.module),
                py_string(&import.name),
            ));
        }
        for i in 0..m.tags.len() {
            self.use_unit("rt/tag");
            w.line(format!(
                "self.tag{} = {}.Tag()",
                m.imported_tags.len() + i,
                self.rt_name
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
                    let offset = self.masked(offset);
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
                        "self.m.init({}, {}, 0, {})",
                        self.masked(offset),
                        self.data_expr(i, &data.data),
                        data.data.len()
                    ));
                    w.line(format!("self.data{i} = b\"\""));
                }
                None => {
                    w.line(format!("self.data{i} = {}", self.data_expr(i, &data.data)));
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

        // Let import providers bind to the fully-constructed instance.
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
        // The memory is named at every load and store, so the attribute is short; the property keeps the name the provider protocol and embedders use.
        w.line("@property");
        w.line("def memory(self):");
        w.indent();
        w.line("return self.m");
        w.dedent();
        w.line("");
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
        // The boxed Rt.Global itself (not its current value), for a host embedder or another dewasm instance to import as a shared mutable cell.
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
        w.line("def tag_export(self, name):");
        w.indent();
        w.line("return getattr(self, self.TAG_EXPORTS[name])");
        w.dedent();
        w.line("");
        // Provider protocol: an instance is itself a valid import value.
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
        w.line("if name in self.TAG_EXPORTS:");
        w.indent();
        w.line("return getattr(self, self.TAG_EXPORTS[name])");
        w.dedent();
        w.line("if name in self.MEMORY_EXPORTS:");
        w.indent();
        w.line("return self.m");
        w.dedent();
        w.line("return None");
        w.dedent();
        w.line("");
        w.line("def _missing(self, mod, name):");
        w.indent();
        w.line(format!(
            "raise {}.LinkError(\"missing import %s.%s\" % (mod, name))",
            self.rt_name
        ));
        w.dedent();
        if wasi_bundled(self.module, self.default_wasi, bundler()) {
            w.line("");
            w.line("def _wasi_import(self, name):");
            w.indent();
            w.line("if self._wasi is None:");
            w.indent();
            w.line(format!("self._wasi = {}.WASI(args=self._wasi_args, env=self._wasi_env, preopens=self._wasi_preopens)", self.rt_name));
            w.dedent();
            w.line("return getattr(self._wasi, \"wasi_\" + name)");
            w.dedent();
        }
    }

    fn func_type_symbol(&self, func_idx: u32) -> String {
        self.type_symbol_of(self.module.func_type(func_idx))
    }

    fn type_symbol(&self, type_idx: u32) -> String {
        self.type_symbol_of(&self.module.types[type_idx as usize])
    }

    /// A structural type key (see [`type_key`]) as a Python string literal, which is what the table stores and `call_indirect` compares.
    fn type_symbol_of(&self, ty: &dewasm_core::ir::FuncType) -> String {
        py_string(&type_key(ty, val_name))
    }

    fn func_ref(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("self.if{func_idx}")
        } else {
            format!("self._f{func_idx}")
        }
    }

    /// A funcref value: the `[type_key, callable]` pair tables store.
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
        // The defaults are immutable literals, so a chained assignment binds every name to its own value rather than to one shared object.
        for run in local_runs(&func.locals, default_value) {
            let default = default_value(func.locals[run.start]);
            let names = run
                .map(|k| format!("l{}", ty.params.len() + k))
                .collect::<Vec<_>>()
                .join(" = ");
            w.line(format!("{names} = {default}"));
        }
        // Temps default to 0; every temp is assigned before any reachable read (valid wasm), so the value only guards against Python NameError.
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
        *self.flat.borrow_mut() = flat::plan(
            &func.body,
            &flat::frames(&func.body, flat::BreakToBlockEnd::Unavailable).paths,
            flat::DEEP_CROSSING,
        );
        // The branch register is declared only when the cascade can actually use it: a branch to a frame that survives the plan still relays through `_br`, one addressed by state never does, and a function with no label branch at all never reads it either.
        if self.seq_has_relay_branch(&func.body) {
            w.line("_br = 0");
        }
        let nstates = self.flat.borrow().as_ref().map(|p| p.nstates as usize);
        match nstates {
            None => {
                let mut guarded = false;
                self.emit_seq(w, &func.body, &mut guarded, true);
            }
            Some(n) => {
                // Every state body ends in a transition, a `return` or a trap (see `flat_seq`), so no arm falls through to the next round with the same state.
                let mut st: Vec<CodeWriter> = (0..n).map(|_| CodeWriter::new("\t")).collect();
                let last = self.flat_seq(&mut st, 0, &func.body);
                // Falling off the body ends the function; leave the dispatch loop through its `else`.
                // A body that cannot fall off needs no such exit.
                if !terminates(&func.body) {
                    st[last].line(format!("_state = {n}; continue"));
                }
                let texts: Vec<String> = st.into_iter().map(|c| c.finish()).collect();
                w.line("_state = 0");
                w.line("while True:");
                w.indent();
                emit_dispatch_tree(w, &texts, 0, n);
                w.dedent();
            }
        }
        *self.flat.borrow_mut() = None;
        w.dedent();
    }

    /// Emit `stmts` into the state machine, splitting at each dissolved frame.
    /// Statements between the splits go through the ordinary [`Self::emit_seq`], so surviving frames keep their `_br` regions and landing markers.
    /// Returns the state control is in afterwards.
    ///
    /// A run starts unguarded: every frame enclosing a sequence `flat_seq` walks is dissolved (dissolution is transitive up the spine), so every branch escaping the run is addressed by state and leaves `_br` alone.
    fn flat_seq(&self, st: &mut [CodeWriter], mut cur: usize, stmts: &[Stmt]) -> usize {
        let mut pending_start = 0;
        for (i, stmt) in stmts.iter().enumerate() {
            let plan_hit = {
                let p = self.flat.borrow();
                let p = p.as_ref().unwrap();
                match stmt {
                    Stmt::Block { label, .. }
                    | Stmt::Loop { label, .. }
                    | Stmt::If { label, .. }
                        if p.dissolved.contains(&label.id) =>
                    {
                        Some((p.state_of[&label.id], p.after[&label.id]))
                    }
                    _ => None,
                }
            };
            let Some((target, after)) = plan_hit else {
                continue;
            };
            if pending_start < i {
                let mut guarded = false;
                self.emit_seq(&mut st[cur], &stmts[pending_start..i], &mut guarded, false);
            }
            pending_start = i + 1;
            match stmt {
                Stmt::Block { body, .. } => {
                    cur = self.flat_seq(st, cur, body);
                    if !terminates(body) {
                        st[cur].line(format!("_state = {after}; continue"));
                    }
                    cur = after as usize;
                }
                Stmt::Loop { body, .. } => {
                    // `target` is the head: entering the loop and taking its back-edge are the same transition.
                    st[cur].line(format!("_state = {target}; continue"));
                    cur = target as usize;
                    cur = self.flat_seq(st, cur, body);
                    if !terminates(body) {
                        st[cur].line(format!("_state = {after}; continue"));
                    }
                    cur = after as usize;
                }
                Stmt::If {
                    cond, then, els, ..
                } => {
                    // The arms stay inline: `if` is not a Python loop, so a `continue` inside one already reaches the dispatch loop.
                    // An arm that changes state carries on in the new state's writer, at its top level, which is reachable only through the transition emitted under this `if`.
                    st[cur].line(format!("if {}:", self.cond(cond)));
                    st[cur].indent();
                    let a = self.flat_seq(st, cur, then);
                    if !terminates(then) {
                        st[a].line(format!("_state = {after}; continue"));
                    }
                    st[cur].dedent();
                    if !els.is_empty() {
                        st[cur].line("else:");
                        st[cur].indent();
                        let b = self.flat_seq(st, cur, els);
                        if !terminates(els) {
                            st[b].line(format!("_state = {after}; continue"));
                        }
                        st[cur].dedent();
                    }
                    // Reachable only through the condition-false fallthrough.
                    // With an `else` present both arms route themselves (a transition or a terminator), so nothing falls out of the `if` and a trailing transition would be dead text.
                    if els.is_empty() {
                        st[cur].line(format!("_state = {after}; continue"));
                    }
                    cur = after as usize;
                }
                _ => unreachable!("only frames are dissolved"),
            }
        }
        if pending_start < stmts.len() {
            let mut guarded = false;
            self.emit_seq(&mut st[cur], &stmts[pending_start..], &mut guarded, false);
        }
        cur
    }

    /// Emit a statement sequence, threading the compile-time `guarded` flag (whether a preceding statement may have left a branch pending in `_br`).
    /// Block/Loop bodies are spliced inline so block nesting adds no Python nesting; only real loops become `while`.
    ///
    /// Guards are per *run*, not per statement: once `guarded`, a single `if _br == 0:` suite is opened lazily and every following statement (nested constructs included) is emitted inside it unguarded, because the region establishes `_br == 0` at entry and only a statement carrying a free branch can change that.
    /// Such a statement ends its run (the suite closes; the next statement opens a fresh guard), so each statement still executes exactly when the old per-statement guard would have run it.
    /// A construct entered inside a region starts its own body unguarded for the same reason.
    ///
    /// `tail` says nothing runs after this sequence before the function falls off: no following statement in any enclosing sequence, and no enclosing loop back-edge (the flat dispatch passes `false` throughout).
    /// In that position a landing marker (`if _br == N: _br = 0`) writes a register nothing will read again, so it is skipped; a stale nonzero `_br` at fall-off is unobservable.
    ///
    /// Returns the sequence's *free* branch targets: the label ids it branches to that are not bound within it.
    /// The caller unions that set upward (minus the label it binds itself), so the information is derived once bottom-up; re-deriving it top-down at every enclosing block made conversion quadratic in nesting depth.
    fn emit_seq(
        &self,
        w: &mut CodeWriter,
        stmts: &[Stmt],
        guarded: &mut bool,
        tail: bool,
    ) -> BTreeSet<u32> {
        let mut free = BTreeSet::new();
        // Whether an `if _br == 0:` region suite is currently open.
        let mut open = false;
        // The last element that emits code: the one whose marker `tail` can elide.
        let last_emit = stmts.iter().rposition(stmt_emits);
        let mut i = 0;
        while i < stmts.len() {
            let stmt = &stmts[i];
            // A comment (or a construct that emits no code at all) must not open a region: a suite holding only comments is empty to Python.
            // Emitting it wherever the writer stands is safe: an open suite already holds a real statement, and nothing here can touch `_br`.
            if !stmt_emits(stmt) {
                self.simple_stmt_or_skip(w, stmt);
                i += 1;
                continue;
            }
            let stmt_tail = tail && last_emit == Some(i);
            if *guarded && !open {
                w.line("if _br == 0:");
                w.indent();
                open = true;
            }
            // Inside a region (or unguarded) `_br == 0` holds here, so the statement and any construct body emit unguarded.
            let may_set = match stmt {
                Stmt::Block { label, body } => {
                    let mut inner_guarded = false;
                    let mut inner = self.emit_seq(w, body, &mut inner_guarded, stmt_tail);
                    if label.referenced && !stmt_tail {
                        w.line(format!("if _br == {}:", label.id));
                        w.indent();
                        w.line("_br = 0");
                        w.dedent();
                    }
                    inner.remove(&label.id);
                    let escapes = !inner.is_empty();
                    free.extend(inner);
                    escapes
                }
                Stmt::Loop { label, body } => {
                    if label.referenced {
                        w.line("while True:");
                        w.indent();
                        let mut inner_guarded = false;
                        // Never tail: the back-edge rereads `_br` next iteration.
                        let mut inner = self.emit_seq(w, body, &mut inner_guarded, false);
                        w.line(format!("if _br == {}:", label.id));
                        w.indent();
                        w.line("_br = 0");
                        w.line("continue");
                        w.dedent();
                        w.line("break");
                        w.dedent();
                        inner.remove(&label.id);
                        let escapes = !inner.is_empty();
                        free.extend(inner);
                        escapes
                    } else {
                        // No br targets this loop, so it never repeats: the body is spliced inline.
                        // It opens its own regions, so it starts unguarded like any construct body.
                        let mut inner_guarded = false;
                        let mut inner = self.emit_seq(w, body, &mut inner_guarded, stmt_tail);
                        inner.remove(&label.id);
                        let escapes = !inner.is_empty();
                        free.extend(inner);
                        escapes
                    }
                }
                Stmt::If {
                    label,
                    cond,
                    then,
                    els,
                } => {
                    let mut inner = self.emit_if(w, cond, then, els, stmt_tail);
                    if label.referenced && !stmt_tail {
                        w.line(format!("if _br == {}:", label.id));
                        w.indent();
                        w.line("_br = 0");
                        w.dedent();
                    }
                    inner.remove(&label.id);
                    let escapes = !inner.is_empty();
                    free.extend(inner);
                    escapes
                }
                Stmt::TryTable {
                    label,
                    catches,
                    body,
                } => {
                    // Unlike Block/If, the body needs a real Python scope: catching an exception raised anywhere inside it (including a callee) is not expressible through the `_br` register alone.
                    // The wrapping `while True:` gives every catch clause's own branch (see `catch_clause`) somewhere to `break` to, exactly as Ruby's `begin ... end while false` gives its `rescue` a scope to `break`/`next` out of; body itself keeps using `_br` for any branch it contains, same as a Block's.
                    self.use_unit("rt/wasm_exception");
                    w.line("while True:");
                    w.indent();
                    w.line("try:");
                    w.indent();
                    let mut inner = if body.is_empty() {
                        w.line("pass");
                        BTreeSet::new()
                    } else {
                        let mut inner_guarded = false;
                        self.emit_seq(w, body, &mut inner_guarded, false)
                    };
                    w.dedent();
                    w.line(format!("except {}.WasmException as e:", self.rt_name));
                    w.indent();
                    for clause in catches {
                        self.catch_clause(w, clause);
                        self.collect_target_free(&clause.target, &mut inner);
                    }
                    // No clause matched: the exception keeps unwinding.
                    w.line("raise");
                    w.dedent();
                    // Reached only when the body ran to completion without an exception; an exception either lands in a clause (which breaks the loop itself) or re-raises past this statement entirely.
                    w.line("break");
                    w.dedent();
                    if label.referenced && !stmt_tail {
                        w.line(format!("if _br == {}:", label.id));
                        w.indent();
                        w.line("_br = 0");
                        w.dedent();
                    }
                    inner.remove(&label.id);
                    let escapes = !inner.is_empty();
                    free.extend(inner);
                    escapes
                }
                Stmt::SourceLine(_) => unreachable!("filtered by stmt_emits"),
                // Every other statement is a leaf; `simple_stmt` matches all of them exhaustively, so a new variant is a compile error there rather than silent output.
                _ => {
                    if let Some(line) = self.fused_call_line(stmt, stmts.get(i + 1)) {
                        w.line(line);
                        i += 1; // The consumer is emitted with its producer.
                    } else {
                        self.simple_stmt(w, stmt);
                    }
                    self.collect_leaf_free_targets(stmt, &mut free)
                }
            };
            if may_set {
                *guarded = true;
                if open {
                    w.dedent();
                    open = false;
                }
            }
            i += 1;
        }
        if open {
            w.dedent();
        }
        free
    }

    /// Fuse a call-family producer with the adjacent statement that consumes its whole result: `s0 = self._f1(...)` + `l2 = s0` becomes `l2 = self._f1(...)`.
    /// Sound because temps are stack slots: a write at depth `d` immediately followed by a statement whose entire right-hand side is that slot is that value's one and only pop: a value with more readers (a block result, a branch operand) is never consumed adjacently in full.
    /// Only call-shaped producers are fused; pure-expression producers are already folded into their consumers by the IR builder.
    fn fused_call_line(&self, producer: &Stmt, consumer: Option<&Stmt>) -> Option<String> {
        let (produced, call) = match producer {
            Stmt::Call {
                func,
                args,
                results,
            } => {
                let &[t] = &results[..] else { return None };
                let args: Vec<String> = args.iter().map(|a| self.masked(a)).collect();
                (t, self.call_string(*func, &args))
            }
            Stmt::CallIndirect {
                type_idx,
                table_index,
                index,
                args,
                results,
            } => {
                let &[t] = &results[..] else { return None };
                self.use_unit("table/call");
                let mut call_args = vec![self.masked(index), self.type_symbol(*type_idx)];
                call_args.extend(args.iter().map(|a| self.masked(a)));
                (
                    t,
                    format!("self.t{table_index}.call({})", call_args.join(", ")),
                )
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                (*dst, format!("self.m.grow({})", self.masked(delta)))
            }
            _ => return None,
        };
        let target = match consumer? {
            Stmt::Assign {
                dst,
                expr: Expr::Temp(t),
            } if t.depth == produced.depth => temp(*dst),
            Stmt::LocalSet {
                idx,
                expr: Expr::Temp(t),
            } if t.depth == produced.depth => format!("l{idx}"),
            Stmt::GlobalSet {
                idx,
                expr: Expr::Temp(t),
            } if t.depth == produced.depth => format!("self.g{idx}.value"),
            _ => return None,
        };
        Some(format!("{target} = {call}"))
    }

    /// Emit the statements `stmt_emits` deems code-free: a comment renders, an empty construct vanishes (its all-comment body still renders, at the current level).
    fn simple_stmt_or_skip(&self, w: &mut CodeWriter, stmt: &Stmt) {
        match stmt {
            Stmt::SourceLine(_) => self.simple_stmt(w, stmt),
            Stmt::Block { body, .. } | Stmt::Loop { body, .. } => {
                for s in body {
                    self.simple_stmt_or_skip(w, s);
                }
            }
            _ => unreachable!("stmt_emits admits only comments and empty constructs"),
        }
    }

    /// Emit an `if`, returning the free branch targets of both arms (the `if`'s own label is removed by the caller).
    /// `tail` propagates into both arms: when the `if` is the function's last code and its own marker is elided, an arm's trailing marker is equally unread.
    fn emit_if(
        &self,
        w: &mut CodeWriter,
        cond: &Expr,
        then: &[Stmt],
        els: &[Stmt],
        tail: bool,
    ) -> BTreeSet<u32> {
        // A pending `_br` never reaches here: `emit_seq`'s region guard already established `_br == 0` for every statement it emits.
        w.line(format!("if {}:", self.cond(cond)));
        w.indent();
        let mut free = BTreeSet::new();
        if then.is_empty() {
            w.line("pass");
        } else {
            let mut it = false;
            free = self.emit_seq(w, then, &mut it, tail);
        }
        w.dedent();
        if !els.is_empty() {
            w.line("else:");
            w.indent();
            let mut ie = false;
            free.extend(self.emit_seq(w, els, &mut ie, tail));
            w.dedent();
        }
        free
    }

    fn simple_stmt(&self, w: &mut CodeWriter, stmt: &Stmt) {
        match stmt {
            Stmt::Assign { dst, expr } => {
                w.line(format!("{} = {}", temp(*dst), self.masked(expr)));
            }
            Stmt::LocalSet { idx, expr } => {
                w.line(format!("l{idx} = {}", self.masked(expr)));
            }
            Stmt::GlobalSet { idx, expr } => {
                w.line(format!("self.g{idx}.value = {}", self.masked(expr)));
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                let (method, addr_args) = self.mem_call(store_code(*op), addr, *offset);
                w.line(format!(
                    "self.m.{method}({addr_args}, {})",
                    self.modular(value)
                ));
            }
            Stmt::Br(target) => self.branch(w, target),
            Stmt::BrIf { cond, target } => {
                w.line(format!("if {}:", self.cond(cond)));
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
                w.line(format!("_i = {}", self.masked(index)));
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
                let args: Vec<String> = args.iter().map(|a| self.masked(a)).collect();
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
                let mut call_args = vec![self.masked(index), self.type_symbol(*type_idx)];
                call_args.extend(args.iter().map(|a| self.masked(a)));
                let call = format!("self.t{table_index}.call({})", call_args.join(", "));
                w.line(assign_results(results, call));
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                w.line(format!(
                    "{} = self.m.grow({})",
                    temp(*dst),
                    self.masked(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "self.m.copy({}, {}, {})",
                    self.masked(dst),
                    self.masked(src),
                    self.masked(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "self.m.fill({}, {}, {})",
                    self.masked(dst),
                    self.masked(val),
                    self.masked(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "self.m.init({}, self.data{seg}, {}, {})",
                    self.masked(dst),
                    self.masked(src),
                    self.masked(len)
                ));
            }
            Stmt::DataDrop { seg } => {
                w.line(format!("self.data{seg} = b\"\""));
            }
            Stmt::Unreachable => {
                w.line(format!("{}(\"unreachable\")", self.rt("trap")));
            }
            Stmt::Throw { tag, args } => {
                self.use_unit("rt/wasm_exception");
                let args: Vec<String> = args.iter().map(|a| self.masked(a)).collect();
                w.line(format!(
                    "raise {}.WasmException(self.tag{tag}, [{}])",
                    self.rt_name,
                    args.join(", ")
                ));
            }
            Stmt::ThrowRef { exn } => {
                w.line(format!("{}({})", self.rt("throw_ref"), self.masked(exn)));
            }
            Stmt::SourceLine(pos) => {
                let file = &self.module.debug_files[pos.file as usize];
                w.line(format!("# {file}:{}", pos.line));
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
                    self.masked(dst),
                    self.masked(src),
                    self.masked(len)
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
                    self.masked(dst),
                    self.masked(src),
                    self.masked(len)
                ));
            }
            Stmt::ElemDrop { seg } => {
                w.line(format!("self.elem{seg} = []"));
            }
            Stmt::Block { .. } | Stmt::Loop { .. } | Stmt::If { .. } | Stmt::TryTable { .. } => {
                unreachable!("structured statement routed to simple_stmt")
            }
        }
    }

    /// One `try_table` catch clause inside the handler: bind the payload into the target frame's slots, then take the branch.
    /// A tagged clause guards that with `is`, wasm tag equality being object identity; a catch-all runs unconditionally, so any clause after it is dead (still emitted, never reached).
    /// `branch()` alone never leaves the `except` suite: it is shared with ordinary branches, which rely on later code testing `_br` instead, so this appends the `break` that exits the `try_table`'s wrapping `while True:` itself (dead, but harmless, right after a `return`).
    fn catch_clause(&self, w: &mut CodeWriter, clause: &CatchClause) {
        let bind_and_branch = |w: &mut CodeWriter| {
            for (i, t) in clause.value_temps.iter().enumerate() {
                if Some(*t) == clause.exn_temp {
                    w.line(format!("{} = e", temp(*t)));
                } else {
                    w.line(format!("{} = e.values[{i}]", temp(*t)));
                }
            }
            self.branch(w, &clause.target);
            w.line("break");
        };
        match clause.tag {
            Some(tag) => {
                w.line(format!("if e.tag is self.tag{tag}:"));
                w.indent();
                bind_and_branch(w);
                w.dedent();
            }
            None => bind_and_branch(w),
        }
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        match values {
            [] => w.line("return"),
            [v] => w.line(format!("return {}", self.masked(v))),
            vs => {
                let vs = vs
                    .iter()
                    .map(|v| self.masked(v))
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
                // Addressed by value: one assignment and one jump, the same cost from any depth.
                // The `continue` reaches the dispatch loop because every frame this branch escapes was dissolved with it (the path closure in [`flat::plan`]), so no surviving `while True:` sits in between.
                if let Some(st) = self.state_of(*label) {
                    w.line(format!("_state = {st}; continue"));
                    return;
                }
                // is_loop is irrelevant here: the loop trailer turns `_br == <loop id>` into a `continue`; a block/if exit is handled by the guards skipping to the label's reset marker.
                w.line(format!("_br = {label}"));
            }
        }
    }

    /// The dispatch state a branch to `label` lands in, when the current function's plan addresses that label by value.
    fn state_of(&self, label: u32) -> Option<u32> {
        self.flat
            .borrow()
            .as_ref()
            .and_then(|p| p.state_of.get(&label).copied())
    }

    /// Whether `stmts` holds a branch that still travels through `_br`, i.e. whether the function needs the branch register at all.
    /// A label branch the plan addresses by state never touches it, and neither does a `return`.
    /// Only a statement that names a branch target needs an arm below; the traversal reaches the rest.
    fn seq_has_relay_branch(&self, stmts: &[Stmt]) -> bool {
        let relays = |t: &BrTarget| match t {
            BrTarget::Return { .. } => false,
            BrTarget::Label { label, .. } => self.state_of(*label).is_none(),
        };
        Stmt::any(stmts, &mut |stmt| match stmt {
            Stmt::Br(t) | Stmt::BrIf { target: t, .. } => relays(t),
            Stmt::BrTable {
                targets, default, ..
            } => relays(default) || targets.iter().any(relays),
            // A catch clause's own branch relays through `_br` too (`catch_clause` calls the same `branch()`), even when the body underneath never does.
            Stmt::TryTable { catches, .. } => catches.iter().any(|c| relays(&c.target)),
            _ => false,
        })
    }

    /// Add the label ids a non-structured statement branches to into `free`, returning whether it has any.
    /// Non-empty means the statement may leave `_br` set on fall-through, so following siblings must be guarded.
    /// Structured statements get their free set from `emit_seq`, which builds it bottom-up as it emits.
    fn collect_leaf_free_targets(&self, stmt: &Stmt, free: &mut BTreeSet<u32>) -> bool {
        match stmt {
            Stmt::Br(t) | Stmt::BrIf { target: t, .. } => self.collect_target_free(t, free),
            Stmt::BrTable {
                targets, default, ..
            } => {
                let mut any = self.collect_target_free(default, free);
                for t in targets {
                    any |= self.collect_target_free(t, free);
                }
                any
            }
            _ => false,
        }
    }

    fn collect_target_free(&self, t: &BrTarget, free: &mut BTreeSet<u32>) -> bool {
        match t {
            BrTarget::Return { .. } => false,
            // A state-addressed branch jumps to the dispatch loop without touching `_br`: it neither escapes this sequence as a pending branch nor leaves anything for a following statement to guard against.
            BrTarget::Label { label, .. } if self.state_of(*label).is_some() => false,
            BrTarget::Label { label, .. } => {
                free.insert(*label);
                true
            }
        }
    }

    /// Reference a Memory method, recording its unit.
    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
    }

    /// The method name and address arguments for a load/store site.
    /// A nonzero static offset rides as a second argument to the `o`-suffixed unit, so the per-site addition disappears from the caller; an offset-zero site keeps the one-argument unit, whose call must not pay for an argument it never passes.
    /// An offset-zero address that is itself a dynamic `i32.add` rides as two arguments to the `a`-suffixed unit, whose wrapped addition replaces the site's own; the offset addition must not wrap, so the two families stay separate.
    /// A constant add operand keeps the one-argument unit, whose reduction of the site's sum already implements the wrap.
    /// The unit reduces the base address modulo 2^32 itself, so the base renders in `Modular` context and needs no call-site mask.
    /// A constant base folds with the offset at conversion time while the sum stays below 2^32 (where the unit's reduction is the identity); a larger sum can never be in bounds and rides as base plus offset so the unit's exact addition reaches the bounds check.
    fn mem_call(&self, method: &str, addr: &Expr, offset: u64) -> (String, String) {
        if offset == 0 {
            if let Expr::Bin(BinOp::I32Add, x, y) = addr {
                if !matches!(**x, Expr::I32Const(_)) && !matches!(**y, Expr::I32Const(_)) {
                    let method = format!("{method}a");
                    self.use_unit(&format!("memory/{method}"));
                    let args = format!("{}, {}", self.modular(x), self.modular(y));
                    return (method, args);
                }
            }
            return (self.mem(method).to_string(), self.modular(addr));
        }
        if let Expr::I32Const(base) = addr {
            let sum = u64::from(*base) + offset;
            if sum <= u64::from(u32::MAX) {
                return (self.mem(method).to_string(), sum.to_string());
            }
        }
        let method = format!("{method}o");
        self.use_unit(&format!("memory/{method}"));
        let args = format!("{}, {offset}", self.modular(addr));
        (method, args)
    }

    /// An expression whose exact stored value is observed (an argument, a comparison operand): a result mask stays unless it is provably the identity on the exact value.
    fn masked(&self, expr: &Expr) -> String {
        self.expr(expr, MaskContext::Masked)
    }

    /// An expression a memory unit consumes: the unit reduces its address and stored-value arguments itself, so a congruent value suffices and the site's own mask may go.
    fn modular(&self, expr: &Expr) -> String {
        self.expr(expr, MaskContext::Modular)
    }

    /// `ctx` is the consumer's view of the value: under a `Modular` consumer a site's own result mask is skipped when the shared bound guard allows it (see [`ELISION_LIMIT`]).
    fn expr(&self, expr: &Expr, ctx: MaskContext) -> String {
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
            // `eqz` of something already emitted as a Python boolean: read the wasm 0/1 straight off that boolean instead of materializing the operand's own 0/1 first and testing it.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) if is_boolean(a) => {
                format!("(0 if {} else 1)", self.cond(a))
            }
            Expr::Un(op, a) => {
                let a = self.expr(a, un_operand_context(*op));
                self.un(*op, &a, elide(ctx, expr))
            }
            Expr::Bin(op, a, b) => {
                if matches!(op, BinOp::I32And | BinOp::I64And) {
                    if let Some((e, c)) = fold_and_chain(a, b) {
                        return format!("({} & {c})", self.expr(e, MaskContext::Reducing));
                    }
                }
                if let Some((ra, rb)) = self.eq_rewrite_operands(*op, a, b) {
                    return self.bin(*op, &ra, &rb, false);
                }
                let ra = self.expr(a, bin_operand_context(*op, 0, b, ctx));
                let rb = match shift_width(*op) {
                    Some(bits) => self.shift_count(b, bits),
                    None => self.expr(b, bin_operand_context(*op, 1, a, ctx)),
                };
                self.bin(*op, &ra, &rb, elide(ctx, expr))
            }
            Expr::Load { op, addr, offset } => {
                let (method, addr_args) = self.mem_call(load_code(*op), addr, *offset);
                format!("self.m.{method}({addr_args})")
            }
            Expr::Select { cond, then, els } => {
                format!(
                    "({} if {} else {})",
                    self.masked(then),
                    self.cond(cond),
                    self.masked(els)
                )
            }
            Expr::MemorySize => {
                self.use_unit("memory/size");
                "self.m.size()".to_string()
            }
        }
    }

    /// A shift count, reduced modulo the width as wasm requires ([`shift_count_mode`]): a constant folds at conversion time, a provably in-range count is emitted bare from its `Masked` rendering, anything else under `& (bits - 1)`.
    fn shift_count(&self, b: &Expr, bits: u32) -> String {
        match shift_count_mode(b, bits, ELISION_LIMIT) {
            ShiftCountMode::Constant(c) => c.to_string(),
            ShiftCountMode::InRange => self.expr(b, MaskContext::Masked),
            ShiftCountMode::Masked => {
                format!("({} & {})", self.expr(b, MaskContext::Modular), bits - 1)
            }
        }
    }

    /// An expression in boolean context (an `if`/`br_if` test).
    ///
    /// A wasm comparison yields the i32 0 or 1, and every conditional context then compares that against 0, so the lowering built a conditional expression only to undo it one operation later.
    /// Emitting the comparison as a Python boolean drops both the conditional and the test; the operands are untouched, so a signed view still goes through `Rt.s32`/`Rt.s64`.
    /// Anything else keeps the `!= 0` test. (Ported from the Ruby backend, #122.)
    fn cond(&self, e: &Expr) -> String {
        match e {
            // `eqz` in boolean context is the negation of its operand's own test.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => self.not_cond(a),
            Expr::Bin(op, a, b) => match signed_view_rel_op(*op) {
                Some(rel) => {
                    let (ra, rb) = match self.eq_rewrite_operands(*op, a, b) {
                        Some(pair) => pair,
                        None => (self.masked(a), self.masked(b)),
                    };
                    self.rel(rel, &ra, &rb)
                }
                None => self.nonzero_test(e),
            },
            _ => self.nonzero_test(e),
        }
    }

    /// `e != 0`: the fallback test for a value that is not already a Python boolean.
    /// A mask site whose raw interval pins a unique preimage of 0 compares unmasked against it (see [`eq_const_rewrite`]).
    fn nonzero_test(&self, e: &Expr) -> String {
        match eq_const_rewrite(e, 0, ELISION_LIMIT) {
            Some((x, ctx, t)) => format!("{} != {t}", self.expr(x, ctx)),
            None => format!("({}) != 0", self.masked(e)),
        }
    }

    /// The rendered operands of an integer equality whose one side is a constant, when the shared analysis pins the other side's mask to a unique raw preimage (see [`eq_const_rewrite`]); `None` renders both sides exact.
    fn eq_rewrite_operands(&self, op: BinOp, a: &Expr, b: &Expr) -> Option<(String, String)> {
        use BinOp::*;
        if !matches!(op, I32Eq | I32Ne | I64Eq | I64Ne) {
            return None;
        }
        let (e, c) = match (a, b) {
            (Expr::I32Const(c), e) | (e, Expr::I32Const(c)) => (e, u64::from(*c)),
            (Expr::I64Const(c), e) | (e, Expr::I64Const(c)) => (e, *c),
            _ => return None,
        };
        let (x, ctx, t) = eq_const_rewrite(e, c, ELISION_LIMIT)?;
        Some((self.expr(x, ctx), t.to_string()))
    }

    /// The negation of [`cond`]: `e` is zero.
    /// A comparison is negated as a whole rather than by flipping its operator, which would be wrong for floats (both `x < y` and `x >= y` are false when either is NaN).
    fn not_cond(&self, e: &Expr) -> String {
        match e {
            // Two negations cancel.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => self.cond(a),
            Expr::Bin(op, ..) if signed_view_rel_op(*op).is_some() => {
                format!("not ({})", self.cond(e))
            }
            _ => match eq_const_rewrite(e, 0, ELISION_LIMIT) {
                Some((x, ctx, t)) => format!("{} == {t}", self.expr(x, ctx)),
                None => format!("{} == 0", self.masked(e)),
            },
        }
    }

    /// A comparison as a Python boolean, from a `rel_op` mapping and the already rendered operands.
    fn rel(&self, (r, sign): (&str, Option<&str>), a: &str, b: &str) -> String {
        match sign {
            None => format!("{a} {r} {b}"),
            Some(sign) => {
                let f = self.rt(sign);
                format!("{f}({a}) {r} {f}({b})")
            }
        }
    }

    fn un(&self, op: UnOp, a: &str, elide_mask: bool) -> String {
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
            I32WrapI64 => {
                if elide_mask {
                    a.to_string()
                } else {
                    format!("({a} & 0xFFFFFFFF)")
                }
            }
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

    fn bin(&self, op: BinOp, a: &str, b: &str, elide_mask: bool) -> String {
        use BinOp::*;
        // A comparison is a Python boolean; outside condition position it needs the conditional back to the i32 0 or 1 wasm expects (see `cond`).
        if let Some(rel) = signed_view_rel_op(op) {
            return format!("(1 if {} else 0)", self.rel(rel, a, b));
        }
        match op {
            I32Add => mask32_unless(elide_mask, format!("{a} + {b}")),
            I32Sub => mask32_unless(elide_mask, format!("{a} - {b}")),
            I32Mul => mask32_unless(elide_mask, format!("{a} * {b}")),
            I64Add => mask64_unless(elide_mask, format!("{a} + {b}")),
            I64Sub => mask64_unless(elide_mask, format!("{a} - {b}")),
            I64Mul => mask64_unless(elide_mask, format!("{a} * {b}")),
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
            // A shift's `b` arrives from `shift_count`, already reduced modulo the width.
            I32Shl => mask32_unless(elide_mask, format!("{a} << {b}")),
            I32ShrU => format!("({a} >> {b})"),
            I32ShrS => mask32_unless(elide_mask, format!("{}({a}) >> {b}", self.rt("s32"))),
            I64Shl => mask64_unless(elide_mask, format!("{a} << {b}")),
            I64ShrU => format!("({a} >> {b})"),
            I64ShrS => mask64_unless(elide_mask, format!("{}({a}) >> {b}", self.rt("s64"))),
            I32Rotl => format!("{}({a}, {b})", self.rt("i32_rotl")),
            I32Rotr => format!("{}({a}, {b})", self.rt("i32_rotr")),
            I64Rotl => format!("{}({a}, {b})", self.rt("i64_rotl")),
            I64Rotr => format!("{}({a}, {b})", self.rt("i64_rotr")),
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
            _ => unreachable!("op {op:?} is a comparison, rendered by `rel`"),
        }
    }
}

/// The i32 result mask, skipped when the consumer restores it; the grouping parentheses stay.
fn mask32_unless(elide: bool, e: String) -> String {
    if elide {
        format!("({e})")
    } else {
        format!("(({e}) & 0xFFFFFFFF)")
    }
}

/// The i64 counterpart of [`mask32_unless`].
fn mask64_unless(elide: bool, e: String) -> String {
    if elide {
        format!("({e})")
    } else {
        format!("(({e}) & 0xFFFFFFFFFFFFFFFF)")
    }
}

/// CPython integers are heap-allocated 30-bit-digit bignums at every size, so no unboxed range bounds an exposed intermediate the way Ruby's Fixnum does.
/// The Ruby limit is kept anyway: under it every exposed intermediate fits in three digits, at most one more than the masked value it replaces, and the guard makes the same claim on every backend that uses it.
const ELISION_LIMIT: i128 = 1 << 62;

/// Whether `e`'s own result mask may be skipped here, per the shared context and bound analysis.
fn elide(ctx: MaskContext, e: &Expr) -> bool {
    elides_mask(e, ctx, ELISION_LIMIT)
}

/// A Python float literal that round-trips to the same double.
/// `{:?}` on f64 gives the shortest round-tripping decimal, which Python's `float()` parses back exactly; only the spelling of infinities/`e` notation differs, and non-finite values never reach here.
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

/// Emit the dispatch over `_state` as a balanced binary-search tree of `if _state < M:` splits, leaves holding the state bodies, at O(log n) compares per transition.
/// Ruby's `case/when` dispatch is a single hash probe, but CPython compiles both an `elif` chain and `match/case` over int literals to sequential compares: on the packed-CRuby conversion the linear chain (largest machine 1,463 states) measured 1.22x *slower* than the relay cascade it replaced, and the tree is what makes the flat lowering pay for itself.
/// `lo..=hi` is the id range this subtree serves; id `texts.len()` is the exit state, reachable only as a transition target, whose leaf is `break`.
/// A leaf emits no test: transitions only ever assign ids in range, so the path proves the value.
fn emit_dispatch_tree(w: &mut CodeWriter, texts: &[String], lo: usize, hi: usize) {
    if lo == hi {
        if lo == texts.len() {
            w.line("break");
        } else if texts[lo].is_empty() {
            w.line("pass");
        } else {
            for line in texts[lo].lines() {
                w.line(line);
            }
        }
        return;
    }
    let mid = lo + (hi - lo).div_ceil(2);
    w.line(format!("if _state < {mid}:"));
    w.indent();
    emit_dispatch_tree(w, texts, lo, mid - 1);
    w.dedent();
    w.line("else:");
    w.indent();
    emit_dispatch_tree(w, texts, mid, hi);
    w.dedent();
}

/// Whether `stmt` emits at least one real Python statement.
/// A comment and a construct whose body is only comments (or nothing) emit no code, so `emit_seq` must not open a region guard for them: the suite could end up holding no statement, which Python rejects.
fn stmt_emits(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::SourceLine(_) => false,
        // A referenced label emits its landing marker (block) or the `while True:` shape (loop) regardless of the body.
        Stmt::Block { label, body } | Stmt::Loop { label, body } => {
            label.referenced || body.iter().any(stmt_emits)
        }
        // Everything else emits unconditionally (an `if` emits its header, with an empty `then` arm becoming `pass`), so only a construct that can lower to nothing needs an arm above.
        _ => true,
    }
}

/// Codegen-shape checks for mask elision.
/// The spec harness proves the generated code computes the right values; these pin the shapes it cannot distinguish: a mask restored by a modular consumer is gone, and a mask a non-modular consumer or the bound guard requires is still there.
#[cfg(test)]
mod masks {
    use super::*;

    fn body(wat: &str) -> String {
        let bytes = wat::parse_str(wat).expect("parse wat");
        let module = dewasm_core::build_module(&bytes).expect("build module");
        let (src, _) =
            generate_class_with_units(&module, "M", &RuntimeLinkage::Embedded, false).unwrap();
        src
    }

    /// One function of two i32 params whose body is `expr`, stored into a local so folding cannot drop it.
    fn i32_expr(expr: &str) -> String {
        body(&format!(
            "(module (func (export \"f\") (param i32 i32) (result i32) (local.set 0 {expr}) (local.get 0)))"
        ))
    }

    /// The i64 counterpart of [`i32_expr`].
    fn i64_expr(expr: &str) -> String {
        body(&format!(
            "(module (func (export \"f\") (param i64 i64) (result i64) (local.set 0 {expr}) (local.get 0)))"
        ))
    }

    fn assert_line(src: &str, want: &str) {
        assert!(
            src.lines().any(|l| l.trim() == want),
            "expected line `{want}` in:\n{src}"
        );
    }

    #[test]
    fn modular_consumer_drops_the_operand_mask() {
        // The outer add's mask reduces the whole sum; the inner add needs none.
        assert_line(
            &i32_expr("(i32.add (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = (((l0 + l1) + l1) & 0xFFFFFFFF)",
        );
        // `shr_s` exposes at most the signed 32-bit range, so its own mask goes too.
        assert_line(
            &i32_expr("(i32.add (i32.shr_s (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = (((MRt.s32(l0) >> (l1 & 31)) + l1) & 0xFFFFFFFF)",
        );
        // A shift count is read through `& 31`, so the subtraction feeding it needs no mask.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (i32.sub (local.get 1) (i32.const 1)))"),
            "l0 = ((l0 << ((l1 - 1) & 31)) & 0xFFFFFFFF)",
        );
        // The wrap disappears when the exposed i64 is provably narrow (here at most 2^32).
        assert_line(
            &body(
                "(module (func (export \"f\") (param i64) (result i32) (local i32) \
                 (local.set 1 (i32.add (i32.wrap_i64 (i64.shr_u (local.get 0) (i64.const 32))) (i32.const 7))) (local.get 1)))",
            ),
            "l1 = (((l0 >> 32) + 7) & 0xFFFFFFFF)",
        );
    }

    #[test]
    fn non_modular_consumer_keeps_the_operand_mask() {
        // A division observes the exact value.
        assert_line(
            &i32_expr("(i32.div_u (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = MRt.i32_div_u(((l0 + l1) & 0xFFFFFFFF), l1)",
        );
        // So does a comparison.
        assert_line(
            &i32_expr("(i32.lt_u (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = (1 if ((l0 + l1) & 0xFFFFFFFF) < l1 else 0)",
        );
        // And storage: the statement position itself is an observation point, so the outer mask always stays.
        assert_line(
            &i32_expr("(i32.add (local.get 0) (local.get 1))"),
            "l0 = ((l0 + l1) & 0xFFFFFFFF)",
        );
    }

    #[test]
    fn bound_guard_keeps_the_mask_on_wide_intermediates() {
        // A full-range i32 product reaches 2^64, past the elision limit: the mul stays masked under a modular consumer.
        assert_line(
            &i32_expr("(i32.add (i32.mul (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = ((((l0 * l1) & 0xFFFFFFFF) + l1) & 0xFFFFFFFF)",
        );
        // Narrowed to bytes, the product is provably small and the mask goes.
        assert_line(
            &i32_expr(
                "(i32.add (i32.mul (i32.and (local.get 0) (i32.const 255)) (i32.and (local.get 1) (i32.const 255))) (local.get 1))",
            ),
            "l0 = ((((l0 & 255) * (l1 & 255)) + l1) & 0xFFFFFFFF)",
        );
        // A full-range i64 wraps past the limit too: the wrap keeps its mask.
        assert_line(
            &body(
                "(module (func (export \"f\") (param i64) (result i32) (local i32) \
                 (local.set 1 (i32.add (i32.wrap_i64 (local.get 0)) (i32.const 7))) (local.get 1)))",
            ),
            "l1 = (((l0 & 0xFFFFFFFF) + 7) & 0xFFFFFFFF)",
        );
    }

    #[test]
    fn shift_count_reductions_fold_and_elide() {
        // A constant count folds at conversion time.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (i32.const 2))"),
            "l0 = ((l0 << 2) & 0xFFFFFFFF)",
        );
        // The width reduces to 0; the shift stays, and the unmoved value makes the result mask the identity, so it drops too.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (i32.const 32))"),
            "l0 = (l0 << 0)",
        );
        // A variable count keeps the reduction.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (local.get 1))"),
            "l0 = ((l0 << (l1 & 31)) & 0xFFFFFFFF)",
        );
        // A count wasm code already reduced is provably in range: no second `& 63`.
        assert_line(
            &i64_expr("(i64.shl (local.get 0) (i64.and (local.get 1) (i64.const 63)))"),
            "l0 = ((l0 << (l1 & 63)) & 0xFFFFFFFFFFFFFFFF)",
        );
    }

    #[test]
    fn i64_elides_only_under_the_shared_bound() {
        // Two full-range i64 values sum past the limit: the inline mask stays even under the modular sub.
        assert_line(
            &i64_expr("(i64.sub (i64.add (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = ((((l0 + l1) & 0xFFFFFFFFFFFFFFFF) - l1) & 0xFFFFFFFFFFFFFFFF)",
        );
        // Provably narrow (the high half plus a constant), the inner mask goes.
        assert_line(
            &i64_expr(
                "(i64.sub (local.get 1) (i64.add (i64.shr_u (local.get 0) (i64.const 32)) (i64.const 5)))",
            ),
            "l0 = ((l1 - ((l0 >> 32) + 5)) & 0xFFFFFFFFFFFFFFFF)",
        );
    }

    #[test]
    fn and_chain_constants_fold_at_conversion_time() {
        assert_line(
            &i32_expr("(i32.and (i32.and (local.get 0) (i32.const 65280)) (i32.const 4080))"),
            "l0 = (l0 & 3840)",
        );
        // A single constant is the plain AND shape: nothing folds.
        assert_line(
            &i32_expr("(i32.and (i32.and (local.get 0) (local.get 1)) (i32.const 15))"),
            "l0 = ((l0 & l1) & 15)",
        );
    }

    #[test]
    fn a_constant_and_drops_the_operand_mask_unguarded() {
        // The raw product exceeds the bound guard, but `& 255` reduces at least as strongly as the mask it replaces.
        assert_line(
            &i32_expr("(i32.and (i32.mul (local.get 0) (local.get 1)) (i32.const 255))"),
            "l0 = ((l0 * l1) & 255)",
        );
        // The i64 mask disappears the same way.
        assert_line(
            &i64_expr("(i64.and (i64.add (local.get 0) (local.get 1)) (i64.const 255))"),
            "l0 = ((l0 + l1) & 255)",
        );
        // The reduction covers only the AND's own operand: under the XOR the bound guard still binds.
        assert_line(
            &i32_expr(
                "(i32.and (i32.xor (i32.mul (local.get 0) (local.get 1)) (local.get 1)) (i32.const 255))",
            ),
            "l0 = ((((l0 * l1) & 0xFFFFFFFF) ^ l1) & 255)",
        );
    }

    #[test]
    fn an_identity_mask_drops_at_an_observation_point() {
        // The high half extracted by `>> 32` sits in [0, 2^32): the wrap is the identity even in the stored position.
        assert_line(
            &body(
                "(module (func (export \"f\") (param i64) (result i32) (local i32) \
                 (local.set 1 (i32.wrap_i64 (i64.shr_u (local.get 0) (i64.const 32)))) (local.get 1)))",
            ),
            "l1 = (l0 >> 32)",
        );
    }

    #[test]
    fn a_pinned_constant_equality_drops_the_mask() {
        // `((l0 - 5) & 0xFFFFFFFF) == 7` admits exactly one raw preimage, and the constant migrates across the sub.
        assert_line(
            &i32_expr("(i32.eq (i32.sub (local.get 0) (i32.const 5)) (i32.const 7))"),
            "l0 = (1 if l0 == 12 else 0)",
        );
        // The same rewrite in boolean position.
        assert_line(
            &body(
                "(module (func (export \"f\") (param i32) (result i32) (local i32) \
                 (if (i32.eq (i32.sub (local.get 0) (i32.const 5)) (i32.const 7)) (then (local.set 1 (i32.const 1)))) (local.get 1)))",
            ),
            "if l0 == 12:",
        );
        // `eqz` is the equality against 0.
        assert_line(
            &body(
                "(module (func (export \"f\") (param i32) (result i32) (local i32) \
                 (if (i32.eqz (i32.sub (local.get 0) (i32.const 5))) (then (local.set 1 (i32.const 1)))) (local.get 1)))",
            ),
            "if l0 == 5:",
        );
    }

    #[test]
    fn an_unpinned_constant_equality_keeps_the_mask() {
        // A two-sided sub spans almost 2^33: two candidates fit, so the mask stays.
        assert_line(
            &i32_expr("(i32.eq (i32.sub (local.get 0) (local.get 1)) (i32.const 7))"),
            "l0 = (1 if ((l0 - l1) & 0xFFFFFFFF) == 7 else 0)",
        );
        // No candidate fits: statically false, but the comparison still runs (the operand could trap), only the identity mask goes.
        assert_line(
            &i32_expr(
                "(i32.eq (i32.add (i32.and (local.get 0) (i32.const 3)) (i32.const 1)) (i32.const 9))",
            ),
            "l0 = (1 if ((l0 & 3) + 1) == 9 else 0)",
        );
    }
}

/// Codegen-shape checks for the static load/store offset: a nonzero offset rides as a second argument to the `o`-suffixed unit, an offset-zero site keeps the one-argument unit, a constant base folds with the offset at conversion time, and an offset-zero dynamic `i32.add` address rides as two arguments to the `a`-suffixed unit.
#[cfg(test)]
mod memory_offsets {
    use super::*;

    fn body(wat: &str) -> String {
        let bytes = wat::parse_str(wat).expect("parse wat");
        let module = dewasm_core::build_module(&bytes).expect("build module");
        let (src, _) =
            generate_class_with_units(&module, "M", &RuntimeLinkage::Embedded, false).unwrap();
        src
    }

    /// One function of an i32 address and an i32 value whose body is `stmt`.
    fn mem_stmt(stmt: &str) -> String {
        body(&format!(
            "(module (memory 1) (func (export \"f\") (param i32 i32) {stmt}))"
        ))
    }

    fn assert_line(src: &str, want: &str) {
        assert!(
            src.lines().any(|l| l.trim() == want),
            "expected line `{want}` in:\n{src}"
        );
    }

    #[test]
    fn nonzero_offset_rides_as_a_second_argument() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load offset=12 (local.get 0)))"),
            "l1 = self.m.iwlo(l0, 12)",
        );
        assert_line(
            &mem_stmt("(i32.store offset=8 (local.get 0) (local.get 1))"),
            "self.m.iwso(l0, 8, l1)",
        );
    }

    #[test]
    fn offset_zero_keeps_the_one_argument_unit() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load (local.get 0)))"),
            "l1 = self.m.iwl(l0)",
        );
        assert_line(
            &mem_stmt("(i32.store (local.get 0) (local.get 1))"),
            "self.m.iws(l0, l1)",
        );
    }

    #[test]
    fn constant_base_folds_with_the_offset() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load offset=12 (i32.const 4)))"),
            "l1 = self.m.iwl(16)",
        );
        assert_line(
            &mem_stmt("(i32.store offset=8 (i32.const 4) (local.get 1))"),
            "self.m.iws(12, l1)",
        );
    }

    #[test]
    fn dynamic_add_address_rides_as_two_arguments() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load (i32.add (local.get 0) (local.get 1))))"),
            "l1 = self.m.iwla(l0, l1)",
        );
        assert_line(
            &mem_stmt("(i32.store (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "self.m.iwsa(l0, l1, l1)",
        );
    }

    #[test]
    fn constant_add_operand_keeps_the_one_argument_unit() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load (i32.add (i32.const 4) (local.get 0))))"),
            "l1 = self.m.iwl((4 + l0))",
        );
    }

    #[test]
    fn dynamic_add_under_a_nonzero_offset_keeps_the_offset_unit() {
        assert_line(
            &mem_stmt("(i32.store offset=8 (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "self.m.iwso((l0 + l1), 8, l1)",
        );
    }
}

/// Codegen-shape checks for the memory unit contract: the unit reduces its address and stored-value arguments modulo the width itself, so both render in `Modular` context and carry no call-site mask.
#[cfg(test)]
mod memory_operand_reduction {
    use super::*;

    fn body(wat: &str) -> String {
        let bytes = wat::parse_str(wat).expect("parse wat");
        let module = dewasm_core::build_module(&bytes).expect("build module");
        let (src, _) =
            generate_class_with_units(&module, "M", &RuntimeLinkage::Embedded, false).unwrap();
        src
    }

    /// One function of an i32 address and an i32 value whose body is `stmt`.
    fn mem_stmt(stmt: &str) -> String {
        body(&format!(
            "(module (memory 1) (func (export \"f\") (param i32 i32) {stmt}))"
        ))
    }

    fn assert_line(src: &str, want: &str) {
        assert!(
            src.lines().any(|l| l.trim() == want),
            "expected line `{want}` in:\n{src}"
        );
    }

    #[test]
    fn address_renders_bare() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load (i32.add (local.get 0) (i32.const 4))))"),
            "l1 = self.m.iwl((l0 + 4))",
        );
        assert_line(
            &mem_stmt("(i32.store (i32.add (local.get 0) (i32.const 4)) (local.get 1))"),
            "self.m.iws((l0 + 4), l1)",
        );
    }

    #[test]
    fn two_argument_base_renders_bare() {
        assert_line(
            &mem_stmt("(i32.store offset=8 (i32.add (local.get 0) (i32.const 4)) (local.get 1))"),
            "self.m.iwso((l0 + 4), 8, l1)",
        );
    }

    #[test]
    fn store_value_renders_bare() {
        assert_line(
            &mem_stmt("(i32.store (local.get 0) (i32.add (local.get 1) (i32.const 1)))"),
            "self.m.iws(l0, (l1 + 1))",
        );
    }

    #[test]
    fn narrow_store_value_renders_bare() {
        assert_line(
            &mem_stmt("(i32.store8 (local.get 0) (i32.add (local.get 1) (i32.const 1)))"),
            "self.m.iwsb(l0, (l1 + 1))",
        );
    }

    #[test]
    fn constant_base_past_the_address_space_keeps_the_exact_addition() {
        // The folded sum would be reduced by the unit into a bounds success; base plus offset reaches the bounds check unreduced and traps.
        assert_line(
            &mem_stmt("(local.set 1 (i32.load offset=4294967295 (i32.const 1)))"),
            "l1 = self.m.iwlo(1, 4294967295)",
        );
    }
}

/// Codegen-shape checks for control flow: a *deep* multi-level `br` must be addressed by value (a state assignment plus a `continue` into the dispatch loop); a shallow one must keep the `_br` branch register, measured cheaper than a dispatch at that depth.
#[cfg(test)]
mod cascade {
    use super::*;

    fn convert(wat: &str) -> String {
        let bytes = wat::parse_str(wat).expect("parse wat");
        let module = dewasm_core::build_module(&bytes).expect("build module");
        let (src, _) =
            generate_class_with_units(&module, "M", &RuntimeLinkage::Embedded, false).unwrap();
        src
    }

    /// A `br_table` tower `depth` blocks deep whose table names every level, so the outermost target is crossed by a branch of exactly that path length: the wasm compilation of a C `switch`, and the shape whose size decides between the two lowerings.
    fn tower(depth: usize) -> String {
        let opens = (0..depth)
            .map(|i| format!("(block $l{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let targets = (0..depth)
            .rev()
            .map(|i| format!("$l{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            "(module (func (export \"f\") (param i32) (result i32) (local i32) \
             {opens} (br_table {targets} (local.get 0)) {closes} \
             (local.set 1 (i32.const 7)) (local.get 1)))",
            closes = ")".repeat(depth)
        )
    }

    #[test]
    fn deep_multi_level_br_is_addressed_by_value() {
        let src = convert(&tower(flat::DEEP_CROSSING + 2));
        assert!(src.contains("_state = 0"), "no dispatch entry in:\n{src}");
        assert!(src.contains("if _state < "), "no dispatch tree in:\n{src}");
        assert!(src.contains("; continue"), "no transition in:\n{src}");
        // The register lowering it replaces: no relay, and no register at all in a function whose every crossing is addressed by state.
        assert!(!src.contains("_br"), "branch register survived in:\n{src}");
    }

    #[test]
    fn shallow_multi_level_br_keeps_the_relay() {
        // The same tower, one level below the threshold: a relay of that depth is cheaper than a dispatch, so nothing dissolves.
        let src = convert(&tower(flat::DEEP_CROSSING - 1));
        assert!(
            src.contains("_br = "),
            "no relay for a shallow branch in:\n{src}"
        );
        assert!(
            src.contains("if _br == 1:"),
            "no landing marker for a shallow branch in:\n{src}"
        );
        assert!(!src.contains("_state"), "state machine leaked in:\n{src}");
    }

    #[test]
    fn mixed_depths_stay_structured() {
        // block $A { loop $B { block $C { br_table $C $B $A } ... } }
        // A single `br_table` whose targets span all three nesting depths.
        // Every crossing is shallow, so the whole function keeps the register lowering and its structured loop.
        let src = convert(
            r#"
              (module
                (func (export "f") (param i32) (result i32)
                  (local i32)
                  (block $A
                    (loop $B
                      (block $C
                        (br_table $C $B $A (local.get 0)))
                      (local.set 1 (i32.const 7))))
                  (local.get 1)))
            "#,
        );
        assert!(src.contains("while True:"), "no structured loop in:\n{src}");
        assert!(src.contains("_br = "), "no relay in:\n{src}");
        assert!(!src.contains("_state"), "dispatch appeared in:\n{src}");
    }
}

/// Lint for the runtime units: every reference a unit body makes to another unit must be declared in its `# requires:` header.
/// Mirrors the Ruby backend's units lint, adjusted for Python syntax (`Rt.<name>` staticmethod/const references, `self.memory.<name>` memory calls, and `self.<name>(...)` sibling calls within a scope's nested class).
#[cfg(test)]
mod units {
    use super::*;
    use std::collections::BTreeSet;

    use regex::Regex;

    #[test]
    fn all_units_bundle() {
        bundler().bundle_all(0).expect("full bundle resolves");
    }

    /// `Embedded` linkage renames the runtime per artifact by replacing `Rt.` across the bundle text, which is sound only while every `Rt.` in a unit is code.
    /// A `Rt.` inside a string literal would be rewritten too, silently changing program-visible text (a trap message, an errno key); no unit has one today, and this keeps it that way.
    /// Triple-quoted strings are rejected outright, since the single-line scanner cannot see across them.
    #[test]
    fn no_rt_reference_inside_a_string_literal() {
        let mut problems = Vec::new();
        for unit in bundler().units() {
            assert!(
                !unit.body.contains("\"\"\"") && !unit.body.contains("'''"),
                "{}: triple-quoted string, which the rename lint cannot scan",
                unit.id
            );
            for (n, line) in unit.body.lines().enumerate() {
                for lit in string_literals(line) {
                    if lit.contains("Rt.") {
                        problems.push(format!(
                            "{}:{}: `Rt.` inside the string literal \"{lit}\"",
                            unit.id,
                            n + 1
                        ));
                    }
                }
            }
        }
        assert!(
            problems.is_empty(),
            "the Embedded runtime rename would rewrite these string literals:\n{}",
            problems.join("\n")
        );
    }

    /// The contents of every single-line string literal on `line`, stopping at an unquoted `#` comment.
    fn string_literals(line: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut quote: Option<char> = None;
        let mut buf = String::new();
        let mut chars = line.chars();
        while let Some(c) = chars.next() {
            match quote {
                Some(q) => {
                    if c == '\\' {
                        if let Some(escaped) = chars.next() {
                            buf.push(c);
                            buf.push(escaped);
                        }
                    } else if c == q {
                        out.push(std::mem::take(&mut buf));
                        quote = None;
                    } else {
                        buf.push(c);
                    }
                }
                None => match c {
                    '#' => break,
                    '"' | '\'' => quote = Some(c),
                    _ => {}
                },
            }
        }
        out
    }

    #[test]
    fn declared_requires_cover_references() {
        let b = bundler();
        let unit_ids: BTreeSet<&str> = b.units().map(|u| u.id.as_str()).collect();

        let rt_call = Regex::new(r"Rt\.([a-z_][a-z0-9_]*)").unwrap();
        let rt_const = Regex::new(r"Rt\.([A-Z]\w*)").unwrap();
        let memory_call = Regex::new(r"self\.memory\.([a-z_]\w*)").unwrap();
        // One precompiled sibling-call matcher per unit name (`self.<name>(`).
        let sibling_calls: Vec<(&str, Regex)> = unit_ids
            .iter()
            .map(|id| {
                let name = id.split('/').nth(1).unwrap();
                let re = Regex::new(&format!(r"self\.{}\(", regex::escape(name))).unwrap();
                (*id, re)
            })
            .collect();

        let mut problems = Vec::new();
        for unit in b.units() {
            let scope = unit.id.split('/').next().unwrap();
            let declared: BTreeSet<&str> = unit.requires.iter().map(|s| s.as_str()).collect();
            let mut demand = |dep: String, what: &str| {
                if dep == unit.id || declared.contains(dep.as_str()) {
                    return;
                }
                // Scope preludes and the root prelude are implicit.
                if dep.ends_with("/_class") || dep.ends_with("/_module") {
                    return;
                }
                problems.push(format!(
                    "{}: uses {what} but does not require {dep}",
                    unit.id
                ));
            };

            let code: String = unit
                .body
                .lines()
                .filter(|l| !l.trim_start().starts_with('#'))
                .collect::<Vec<_>>()
                .join("\n");

            for cap in rt_call.captures_iter(&code) {
                demand(format!("rt/{}", &cap[1]), &format!("Rt.{}", &cap[1]));
            }
            for cap in rt_const.captures_iter(&code) {
                let dep = match &cap[1] {
                    "Trap" => "rt/trap".to_string(),
                    "Exit" => "rt/exit".to_string(),
                    "LinkError" => "rt/link_error".to_string(),
                    "Memory" => "memory/_class".to_string(),
                    "Table" => "table/_class".to_string(),
                    "WASI" => "wasi/_class".to_string(),
                    // Root-prelude / self-unit constants, always available.
                    "M32" | "M64" | "F32_MAX" | "F32_OVERFLOW" => continue,
                    other => panic!("{}: unknown runtime constant Rt.{other}", unit.id),
                };
                demand(dep, &format!("Rt.{}", &cap[1]));
            }
            for cap in memory_call.captures_iter(&code) {
                demand(
                    format!("memory/{}", &cap[1]),
                    &format!("self.memory.{}", &cap[1]),
                );
            }

            for (sibling, re) in &sibling_calls {
                let Some(name) = sibling.strip_prefix(&format!("{scope}/")) else {
                    continue;
                };
                if name.starts_with('_') || *sibling == unit.id {
                    continue;
                }
                if re.is_match(&code) {
                    demand(sibling.to_string(), &format!("self.{name}(...)"));
                }
            }
        }
        assert!(
            problems.is_empty(),
            "unit dependency drift:\n{}",
            problems.join("\n")
        );
    }
}
