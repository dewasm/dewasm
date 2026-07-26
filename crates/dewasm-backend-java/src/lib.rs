//! Java backend: translates dewasm IR into a single self-contained `.java`
//! source file (a set of package-private runtime classes plus the generated
//! module class), compiled with `javac` and run on the JVM.
//!
//! Lowering conventions (ADR-30; numeric conventions ADR-2):
//! - i32/i64 are native signed `int`/`long` treated as bit patterns; unsigned
//!   ops use `Integer.*`/`Long.*` (`divideUnsigned`, `compareUnsigned`, ...).
//!   f32/f64 are native `float`/`double` (Java is strict IEEE, no FMA
//!   contraction, so f32 re-rounding and trap-free division need no helper).
//!   NaN bit paths go through `Float.floatToRawIntBits`/`intBitsToFloat` etc.
//! - Control flow uses the per-function branch register `_br` (ADR-28's Python
//!   model, depth-insensitive and — crucially — splittable across methods):
//!   block/if exits and the function return set `_br`; following siblings are
//!   guarded by `if (_br == 0)`; only real loops become `while (true)`. This
//!   avoids Java's "unreachable statement" error entirely (no bare mid-sequence
//!   `return`/`break`), and makes the split below mechanical.
//! - The JVM caps a method at 64KB of bytecode. A function whose estimated size
//!   crosses a threshold is emitted with its locals/temps/`br`/`ret` hoisted to
//!   a per-call **frame object** and its body split into numbered `part`
//!   methods sharing that frame; because control flow is data (`_br`), the parts
//!   are just called in order. Data segments that exceed the 64KB string-literal
//!   limit are emitted as chunked Base64 (`Rt.data_from_b64`).
//!
//! The runtime is composed from per-method units (ADR-6) referenced as
//! `Rt.<name>` / `Memory` / `Table` / `WASI`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::{bail, Result};
use dewasm_backend::{
    check_module_support, Backend, CodeWriter, GenOptions, Mode, OutputFile, RuntimeBundler,
    RuntimeScope, SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, ElemItem, ElemKind, ExportKind, Expr, Func, Label, LoadOp, Module, Stmt,
    StoreOp, Temp, UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// Estimated body-cost above which a function is split into `part` methods to
/// stay under the JVM's 64KB per-method bytecode limit (ADR-30). Cost is an IR
/// node count; the value is tuned so cowsay's largest functions compile.
const SPLIT_THRESHOLD: usize = 900;

/// Raw bytes per Base64 data chunk. Base64 of this (~43.7KB) stays under Java's
/// 64KB (65535-byte) string-literal limit (ADR-30).
const DATA_CHUNK: usize = 32768;

/// A branch-register sentinel for "return from the function", distinct from any
/// real label id (which are small). Emitted as `-1`.
const RETURN_SENTINEL: u32 = u32::MAX;

/// The runtime unit bundler for Java (see runtime/java/units/). Each scope is a
/// top-level package-private class wrapping its unit bodies (methods / nested
/// types); generated code refers to them as `Rt.*` / `Memory` / `Table` /
/// `WASI` (ADR-30).
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| {
        RuntimeBundler::new(
            "//",
            "    ",
            vec![
                RuntimeScope {
                    prefix: "rt",
                    open: "final class Rt {",
                    close: "}",
                    prelude: Some("rt/_prelude"),
                },
                RuntimeScope {
                    prefix: "memory",
                    open: "final class Memory {",
                    close: "}",
                    prelude: Some("memory/_class"),
                },
                RuntimeScope {
                    prefix: "table",
                    open: "final class Table {",
                    close: "}",
                    prelude: Some("table/_class"),
                },
                RuntimeScope {
                    prefix: "wasi",
                    open: "final class WASI {",
                    close: "}",
                    prelude: Some("wasi/_class"),
                },
            ],
            UNIT_SOURCES,
        )
        .expect("runtime units are well-formed")
    })
}

/// Locate a `java` launcher (ADR-15: a missing toolchain is a loud failure at
/// the call site, not here). Honors `$DEWASM_JAVA`, then `java` on `PATH`.
pub fn find_java() -> Option<std::path::PathBuf> {
    find_tool("DEWASM_JAVA", "java")
}

/// Locate a `javac` compiler. Honors `$DEWASM_JAVAC`, then `javac` on `PATH`.
pub fn find_javac() -> Option<std::path::PathBuf> {
    find_tool("DEWASM_JAVAC", "javac")
}

fn find_tool(env: &str, default: &str) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(v) = std::env::var(env) {
        candidates.push(PathBuf::from(v));
    }
    candidates.push(PathBuf::from(default));
    candidates.into_iter().find(|candidate| {
        std::process::Command::new(candidate)
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// A complete, compilable `.java` file bundling *every* runtime unit (plus a
/// tiny `Main`), for the units lint's `javac` check that all units — not just
/// the subset cowsay uses — are valid Java.
pub fn full_bundle_java() -> Result<String> {
    let bundle = bundler().bundle_all(0)?;
    let mut out = String::from("// Generated by dewasm. Do not edit.\n");
    out.push_str(&bundle);
    out.push_str("\n\npublic class Main {\n    public static void main(String[] a) {}\n}\n");
    Ok(out)
}

pub struct JavaBackend;

impl Backend for JavaBackend {
    fn name(&self) -> &str {
        "java"
    }

    fn file_extension(&self) -> &str {
        "java"
    }

    fn has_wasi_p1(&self, name: &str) -> bool {
        bundler().has_unit(&format!("wasi/{name}"))
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // Java floats are native IEEE float/double; NaN paths follow ADR-2.
            Feature::Floats => SupportStatus::Supported,
            // Everything else is rejected at conversion time in the cowsay
            // milestone (ADR-30): non-function imports, multiple tables, and
            // table bulk ops are out of scope for milestone 1.
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        check_module_support(&JavaBackend, module)?;
        let contents = generate_source(module, opts)?;
        Ok(vec![OutputFile {
            name: "Main.java".to_string(),
            contents,
        }])
    }
}

const WASI_MODULES: &[&str] = &["wasi_snapshot_preview1", "wasi_unstable"];

fn is_wasi_module(name: &str) -> bool {
    WASI_MODULES.contains(&name)
}

/// Whether the generated code bundles the built-in WASI as an import fallback
/// (and therefore takes `args`/`env` and constructs a `WASI`).
fn wasi_bundled(module: &Module, default_wasi: bool) -> bool {
    default_wasi
        && module
            .imported_funcs
            .iter()
            .any(|f| is_wasi_module(&f.module) && bundler().has_unit(&format!("wasi/{}", f.name)))
}

fn generate_source(module: &Module, opts: &GenOptions) -> Result<String> {
    // Multi-value function results have no native Java mapping (a method returns
    // one value); reject at conversion time (ADR-0). cowsay has none.
    for ty in &module.types {
        if ty.results.len() > 1 {
            bail!("multi-value function results are not supported by the Java backend yet");
        }
    }

    let type_name = type_name(&opts.module_name);
    let gen = Gen {
        module,
        default_wasi: opts.default_wasi,
        type_name: type_name.clone(),
        uses: RefCell::new(BTreeSet::new()),
        split: Cell::new(false),
        next_part: Cell::new(0),
        cur_base: RefCell::new(String::new()),
        cur_frame_ty: RefCell::new(String::new()),
        part_defs: RefCell::new(Vec::new()),
    };

    // Emit the module class body into its own writer so `uses` is populated
    // before the runtime bundle is assembled.
    let mut body = CodeWriter::new("    ");
    gen.constructor(&mut body);
    for (i, func) in module.funcs.iter().enumerate() {
        body.line("");
        let idx = module.num_imported_funcs() as usize + i;
        gen.function(&mut body, idx as u32, func);
    }

    let standalone = opts.mode == Mode::Standalone;
    let wasi = wasi_bundled(module, opts.default_wasi);
    if standalone || wasi {
        // The public boundary (standalone main / library glue) catches these.
        gen.use_unit("rt/exit");
        gen.use_unit("rt/trap");
    }

    let uses = gen.uses.borrow().clone();
    let bundle = bundler().bundle(&uses, 0)?;

    let mut out = String::from("// Generated by dewasm. Do not edit.\n");
    out.push_str(&bundle);
    out.push_str("\n\n");
    out.push_str(&format!("final class {type_name} {{\n"));
    out.push_str(&reindent(&body.finish(), 1));
    out.push_str("}\n");
    if standalone {
        out.push('\n');
        out.push_str(&main_class(&type_name, wasi));
    }
    Ok(out)
}

/// Re-indent a block of source by `levels` (four spaces each), leaving blank
/// lines empty.
fn reindent(src: &str, levels: usize) -> String {
    let pad = "    ".repeat(levels);
    let mut out = String::new();
    for line in src.lines() {
        if line.trim().is_empty() {
            out.push('\n');
        } else {
            out.push_str(&pad);
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The standalone entry point: prepend a program name to argv (WASI argv[0]),
/// instantiate, run `_start`, and map `proc_exit`/trap to a process exit code
/// (a trap prints to stderr and exits 134, mirroring Ruby/Python/Go).
fn main_class(type_name: &str, wasi: bool) -> String {
    let args = if wasi { "wasiArgs" } else { "null" };
    let arg_setup = if wasi {
        format!(
            "        String[] wasiArgs = new String[argv.length + 1];\n\
             {ind}wasiArgs[0] = {name};\n\
             {ind}System.arraycopy(argv, 0, wasiArgs, 1, argv.length);\n",
            ind = "        ",
            name = java_string(type_name),
        )
    } else {
        String::new()
    };
    let mut out = String::new();
    out.push_str("public class Main {\n");
    out.push_str("    public static void main(String[] argv) {\n");
    out.push_str(&arg_setup);
    out.push_str(&format!(
        "        {type_name} p = new {type_name}(null, {args}, new String[0]);\n"
    ));
    out.push_str("        try {\n");
    out.push_str("            ((Rt.Fn) p.Exports.get(\"_start\")).invoke(new Object[]{});\n");
    out.push_str("        } catch (Rt.Exit e) {\n");
    out.push_str("            System.exit(e.code);\n");
    out.push_str("        } catch (Rt.Trap e) {\n");
    out.push_str("            System.err.print(\"trap: \" + e.getMessage() + \"\\n\");\n");
    out.push_str("            System.err.flush();\n");
    out.push_str("            System.exit(134);\n");
    out.push_str("        }\n");
    out.push_str("        System.exit(0);\n");
    out.push_str("    }\n");
    out.push_str("}\n");
    out
}

/// Derive a Java class name (PascalCase) from the module name.
fn type_name(module_name: &str) -> String {
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

fn jtype(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "int",
        ValType::I64 => "long",
        ValType::F32 => "float",
        ValType::F64 => "double",
        ValType::FuncRef => "Rt.Funcref",
    }
}

fn zero_value(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "0",
        ValType::I64 => "0L",
        ValType::F32 => "0.0f",
        ValType::F64 => "0.0",
        ValType::FuncRef => "null",
    }
}

fn ty_suffix(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
        ValType::FuncRef => "fr",
    }
}

fn temp_name(t: Temp) -> String {
    format!("s{}_{}", t.depth, ty_suffix(t.ty))
}

/// A Java string literal.
fn java_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct Gen<'a> {
    module: &'a Module,
    default_wasi: bool,
    type_name: String,
    uses: RefCell<BTreeSet<String>>,
    /// Whether the function currently being emitted is split into part methods.
    split: Cell<bool>,
    /// Part-method counter for the current function.
    next_part: Cell<usize>,
    /// Base name (`f47`) and frame type (`Frame47`) of the current function.
    cur_base: RefCell<String>,
    cur_frame_ty: RefCell<String>,
    /// Part-method definitions produced while emitting the current function,
    /// flushed after its entry method.
    part_defs: RefCell<Vec<String>>,
}

impl<'a> Gen<'a> {
    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    fn rt(&self, name: &str) -> String {
        self.use_unit(&format!("rt/{name}"));
        format!("Rt.{name}")
    }

    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
    }

    // --- slot references (frame fields when split, else locals) -------------

    fn local_ref(&self, idx: u32) -> String {
        if self.split.get() {
            format!("f.l{idx}")
        } else {
            format!("l{idx}")
        }
    }

    fn temp_ref(&self, t: Temp) -> String {
        let n = temp_name(t);
        if self.split.get() {
            format!("f.{n}")
        } else {
            n
        }
    }

    fn br(&self) -> &'static str {
        if self.split.get() {
            "f.br"
        } else {
            "_br"
        }
    }

    fn ret(&self) -> &'static str {
        if self.split.get() {
            "f.ret"
        } else {
            "_ret"
        }
    }

    fn new_part(&self) -> String {
        let n = self.next_part.get();
        self.next_part.set(n + 1);
        format!("{}_p{}", self.cur_base.borrow(), n)
    }

    fn push_part(&self, name: &str, body: String) {
        let frame = self.cur_frame_ty.borrow().clone();
        let mut out = format!("private void {name}({frame} f) {{\n");
        out.push_str(&reindent(&body, 1));
        out.push_str("}\n");
        self.part_defs.borrow_mut().push(out);
    }

    // --- the constructor ----------------------------------------------------

    fn constructor(&self, w: &mut CodeWriter) {
        let m = self.module;
        let name = &self.type_name;
        self.struct_fields(w);
        w.line("");
        w.line(format!(
            "{name}(java.util.Map<String, java.util.Map<String, Object>> imports, String[] args, String[] env) {{"
        ));
        w.indent();

        if let Some(mem) = &m.memory {
            self.use_unit("memory/_class");
            let max = mem.max_pages.map(|p| p as u32).unwrap_or(65536);
            w.line(format!(
                "this.memory = new Memory({}, {});",
                mem.min_pages as u32, max
            ));
        }
        for (i, table) in m.tables.iter().enumerate() {
            self.use_unit("table/_class");
            w.line(format!("this.t{i} = new Table({});", table.min));
        }

        let wasi = wasi_bundled(m, self.default_wasi);
        if wasi {
            self.use_unit("wasi/_class");
            w.line("this.wasi = new WASI(args, env);");
            if m.memory.is_some() {
                w.line("this.wasi.memory = this.memory;");
            }
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            self.emit_import(w, i, import);
        }

        for (i, global) in m.globals.iter().enumerate() {
            w.line(format!("this.g{i} = {};", self.expr(&global.init)));
        }

        for (i, elem) in m.elems.iter().enumerate() {
            if let ElemKind::Active {
                table_index,
                offset,
            } = &elem.kind
            {
                let items = elem
                    .items
                    .iter()
                    .map(|item| self.elem_item(item))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.use_unit("table/init");
                w.line(format!(
                    "Rt.Funcref[] elem{i} = new Rt.Funcref[]{{{items}}};"
                ));
                w.line(format!(
                    "this.t{table_index}.init({}, elem{i}, 0, {});",
                    self.expr(offset),
                    elem.items.len()
                ));
            }
            // Passive/declared element segments are rejected (TableBulkOps).
        }

        for (i, data) in m.datas.iter().enumerate() {
            self.use_unit("rt/data_from_b64");
            let blob = data_blob(&data.data);
            match &data.offset {
                Some(offset) => {
                    self.use_unit("memory/init");
                    w.line(format!(
                        "this.memory.init(Integer.toUnsignedLong({}), {blob}, 0, {});",
                        self.expr(offset),
                        data.data.len()
                    ));
                }
                None => {
                    w.line(format!("this.data{i} = {blob};"));
                }
            }
        }

        w.line("this.Exports = new java.util.HashMap<>();");
        for export in &m.exports {
            let val = match export.kind {
                ExportKind::Func(idx) => self.func_value(idx),
                ExportKind::Global(idx) => format!("g{idx}"),
                ExportKind::Table(idx) => format!("t{idx}"),
                ExportKind::Memory => "memory".to_string(),
            };
            w.line(format!(
                "this.Exports.put({}, {val});",
                java_string(&export.name)
            ));
        }

        if let Some(start) = m.start {
            w.line(format!("{};", self.call_string(start, &[])));
        }

        w.dedent();
        w.line("}");
    }

    fn struct_fields(&self, w: &mut CodeWriter) {
        let m = self.module;
        if m.memory.is_some() {
            w.line("Memory memory;");
        }
        for i in 0..m.tables.len() {
            w.line(format!("Table t{i};"));
        }
        for (i, g) in m.globals.iter().enumerate() {
            w.line(format!("{} g{i};", jtype(g.ty)));
        }
        for i in 0..m.imported_funcs.len() {
            w.line(format!("Rt.Fn if{i};"));
        }
        if wasi_bundled(m, self.default_wasi) {
            w.line("WASI wasi;");
        }
        for (i, data) in m.datas.iter().enumerate() {
            if data.offset.is_none() {
                w.line(format!("byte[] data{i};"));
            }
        }
        w.line("java.util.Map<String, Object> Exports;");
    }

    fn emit_import(&self, w: &mut CodeWriter, i: usize, import: &dewasm_core::ir::ImportedFunc) {
        let m = self.module;
        let ty = &m.types[import.type_idx as usize];

        let fallback = if is_wasi_module(&import.module) && self.default_wasi {
            let unit = format!("wasi/{}", import.name);
            if bundler().has_unit(&unit) {
                self.use_unit(&unit);
                let call_args = ty
                    .params
                    .iter()
                    .enumerate()
                    .map(|(k, t)| unbox(*t, &format!("__a[{k}]")))
                    .collect::<Vec<_>>()
                    .join(", ");
                Some(format!(
                    "__a -> this.wasi.wasi_{}({call_args})",
                    import.name
                ))
            } else {
                // ENOSYS: unimplemented WASI call.
                Some(enosys_stub(ty))
            }
        } else {
            None
        };

        w.line(format!(
            "{{ Object v = {}; if (v != null) {{",
            self.resolve_import_string(&import.module, &import.name)
        ));
        w.indent();
        w.line("if (!(v instanceof Rt.Fn)) {");
        w.indent();
        w.line(format!(
            "{}({});",
            self.rt("link_error"),
            java_string(&format!(
                "incompatible import type for {}.{}",
                import.module, import.name
            ))
        ));
        w.dedent();
        w.line("}");
        w.line(format!("this.if{i} = (Rt.Fn) v;"));
        w.dedent();
        w.line("} else {");
        w.indent();
        match fallback {
            Some(f) => w.line(format!("this.if{i} = {f};")),
            None => w.line(format!(
                "{}({});",
                self.rt("link_error"),
                java_string(&format!("missing import {}.{}", import.module, import.name))
            )),
        }
        w.dedent();
        w.line("} }");
    }

    fn resolve_import_string(&self, module: &str, name: &str) -> String {
        format!(
            "{}(imports, {}, {})",
            self.rt("resolve_import"),
            java_string(module),
            java_string(name)
        )
    }

    /// The `Rt.Fn` value for a function export / table element.
    fn func_value(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            return format!("(Rt.Fn) if{func_idx}");
        }
        let ty = self.func_type(func_idx);
        let call_args = ty
            .params
            .iter()
            .enumerate()
            .map(|(k, t)| unbox(*t, &format!("__a[{k}]")))
            .collect::<Vec<_>>()
            .join(", ");
        if ty.results.is_empty() {
            format!("(Rt.Fn)(__a -> {{ f{func_idx}({call_args}); return null; }})")
        } else {
            format!("(Rt.Fn)(__a -> f{func_idx}({call_args}))")
        }
    }

    fn elem_item(&self, item: &ElemItem) -> String {
        match item {
            ElemItem::Func(func_idx) => {
                format!(
                    "new Rt.Funcref({}, {})",
                    java_string(&self.func_type_symbol(*func_idx)),
                    self.func_value(*func_idx)
                )
            }
            ElemItem::Null => "null".to_string(),
            // ref-typed global element items imply reference types (rejected).
            ElemItem::Global(idx) => format!("g{idx}"),
        }
    }

    // --- function emission --------------------------------------------------

    fn func_type(&self, func_idx: u32) -> &dewasm_core::ir::FuncType {
        let idx = func_idx as usize;
        let imports = self.module.imported_funcs.len();
        let ty_idx = if idx < imports {
            self.module.imported_funcs[idx].type_idx
        } else {
            self.module.funcs[idx - imports].type_idx
        };
        &self.module.types[ty_idx as usize]
    }

    fn func_type_symbol(&self, func_idx: u32) -> String {
        let ty = self.func_type(func_idx);
        self.type_symbol_of(&ty.params, &ty.results)
    }

    fn type_symbol(&self, type_idx: u32) -> String {
        let ty = &self.module.types[type_idx as usize];
        self.type_symbol_of(&ty.params, &ty.results)
    }

    fn type_symbol_of(&self, params: &[ValType], results: &[ValType]) -> String {
        let names = |tys: &[ValType]| {
            tys.iter()
                .map(|t| ty_suffix(*t))
                .collect::<Vec<_>>()
                .join(",")
        };
        format!("{}->{}", names(params), names(results))
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &Func) {
        let ty = &self.module.types[func.type_idx as usize];
        let nparams = ty.params.len();
        let result = ty.results.first().copied();
        let ret_ty = result.map(jtype).unwrap_or("void");

        let mut local_types = ty.params.clone();
        local_types.extend(func.locals.iter().copied());

        let split = seq_cost(&func.body) > SPLIT_THRESHOLD;
        self.split.set(split);
        self.next_part.set(0);
        *self.cur_base.borrow_mut() = format!("f{idx}");
        *self.cur_frame_ty.borrow_mut() = format!("Frame{idx}");
        self.part_defs.borrow_mut().clear();

        if split {
            // Frame class: all locals + temps + the branch/return registers.
            w.line(format!("static final class Frame{idx} {{"));
            w.indent();
            for (i, t) in local_types.iter().enumerate() {
                w.line(format!("{} l{i};", jtype(*t)));
            }
            for t in &func.temps {
                w.line(format!("{} {};", jtype(t.ty), temp_name(*t)));
            }
            w.line("int br;");
            if let Some(rt) = result {
                w.line(format!("{} ret;", jtype(rt)));
            }
            w.dedent();
            w.line("}");

            let params_str = (0..nparams)
                .map(|i| format!("{} p{i}", jtype(local_types[i])))
                .collect::<Vec<_>>()
                .join(", ");
            w.line(format!("{ret_ty} f{idx}({params_str}) {{"));
            w.indent();
            w.line(format!("Frame{idx} f = new Frame{idx}();"));
            for i in 0..nparams {
                w.line(format!("f.l{i} = p{i};"));
            }
            self.emit_body(w, &func.body, false);
            if result.is_some() {
                w.line("return f.ret;");
            }
            w.dedent();
            w.line("}");
            for def in self.part_defs.borrow_mut().drain(..) {
                w.raw(&def);
            }
        } else {
            let params_str = (0..nparams)
                .map(|i| format!("{} l{i}", jtype(local_types[i])))
                .collect::<Vec<_>>()
                .join(", ");
            w.line(format!("{ret_ty} f{idx}({params_str}) {{"));
            w.indent();
            for (i, lt) in local_types.iter().enumerate().skip(nparams) {
                w.line(format!("{} l{i} = {};", jtype(*lt), zero_value(*lt)));
            }
            for t in &func.temps {
                w.line(format!(
                    "{} {} = {};",
                    jtype(t.ty),
                    temp_name(*t),
                    zero_value(t.ty)
                ));
            }
            w.line("int _br = 0;");
            if let Some(rt) = result {
                w.line(format!("{} _ret = {};", jtype(rt), zero_value(rt)));
            }
            let mut guarded = false;
            for stmt in &func.body {
                self.emit_stmt(w, stmt, &mut guarded);
            }
            if result.is_some() {
                w.line("return _ret;");
            }
            w.dedent();
            w.line("}");
        }
    }

    /// Emit a statement sequence as the body of a construct (loop/if/block body
    /// or the function entry): inline when small, or split into chained `part`
    /// methods when the function is split and the sub-body is large.
    fn emit_body(&self, w: &mut CodeWriter, stmts: &[Stmt], guarded_in: bool) {
        if self.split.get() && seq_cost(stmts) > SPLIT_THRESHOLD {
            for name in self.emit_parts(stmts, guarded_in) {
                w.line(format!("{name}(f);"));
            }
        } else {
            let mut guarded = guarded_in;
            for stmt in stmts {
                self.emit_stmt(w, stmt, &mut guarded);
            }
        }
    }

    /// Split `stmts` into consecutive `part` methods (each below the threshold),
    /// threading the `guarded` flag across the boundaries. Parts are called
    /// unconditionally in order: control flow is carried by the `f.br` register
    /// and each part self-guards, so an escaped branch simply no-ops the rest.
    fn emit_parts(&self, stmts: &[Stmt], guarded_in: bool) -> Vec<String> {
        let mut names = Vec::new();
        let mut w = CodeWriter::new("    ");
        let mut guarded = guarded_in;
        let mut cost = 0usize;
        for stmt in stmts {
            let c = stmt_cost(stmt);
            if cost > 0 && cost + c > SPLIT_THRESHOLD {
                let name = self.new_part();
                self.push_part(&name, w.finish());
                names.push(name);
                w = CodeWriter::new("    ");
                cost = 0;
            }
            self.emit_stmt(&mut w, stmt, &mut guarded);
            cost += c;
        }
        let name = self.new_part();
        self.push_part(&name, w.finish());
        names.push(name);
        names
    }

    fn emit_stmt(&self, w: &mut CodeWriter, stmt: &Stmt, guarded: &mut bool) {
        match stmt {
            Stmt::Block { label, body } => {
                self.emit_body(w, body, *guarded);
                self.reset_marker(w, label);
                *guarded = *guarded || !stmt_free_targets(stmt).is_empty();
            }
            Stmt::Loop { label, body } => {
                if label.referenced {
                    let before = *guarded;
                    w.line("while (true) {");
                    w.indent();
                    self.emit_body(w, body, before);
                    w.line(format!(
                        "if ({0} == {1}) {{ {0} = 0; continue; }}",
                        self.br(),
                        label.id
                    ));
                    w.line("break;");
                    w.dedent();
                    w.line("}");
                    *guarded = before || !stmt_free_targets(stmt).is_empty();
                } else {
                    self.emit_body(w, body, *guarded);
                    *guarded = *guarded || !stmt_free_targets(stmt).is_empty();
                }
            }
            Stmt::If {
                label,
                cond,
                then,
                els,
            } => {
                self.emit_if(w, *guarded, cond, then, els);
                self.reset_marker(w, label);
                *guarded = *guarded || !stmt_free_targets(stmt).is_empty();
            }
            _ => {
                if *guarded {
                    w.line(format!("if ({} == 0) {{", self.br()));
                    w.indent();
                    self.simple_stmt(w, stmt);
                    w.dedent();
                    w.line("}");
                } else {
                    self.simple_stmt(w, stmt);
                }
                if !stmt_free_targets(stmt).is_empty() {
                    *guarded = true;
                }
            }
        }
    }

    fn reset_marker(&self, w: &mut CodeWriter, label: &Label) {
        if label.referenced {
            w.line(format!(
                "if ({0} == {1}) {{ {0} = 0; }}",
                self.br(),
                label.id
            ));
        }
    }

    fn emit_if(&self, w: &mut CodeWriter, guarded: bool, cond: &Expr, then: &[Stmt], els: &[Stmt]) {
        let cond_s = self.expr(cond);
        if guarded {
            // `_br == 0 &&` short-circuits, so `cond` is not evaluated (and
            // cannot trap) while a branch is pending.
            w.line(format!("if ({} == 0 && ({cond_s}) != 0) {{", self.br()));
        } else {
            w.line(format!("if (({cond_s}) != 0) {{"));
        }
        w.indent();
        self.emit_body(w, then, guarded);
        w.dedent();
        if els.is_empty() {
            w.line("}");
        } else if guarded {
            w.line(format!("}} else if ({} == 0) {{", self.br()));
            w.indent();
            self.emit_body(w, els, guarded);
            w.dedent();
            w.line("}");
        } else {
            w.line("} else {");
            w.indent();
            self.emit_body(w, els, guarded);
            w.dedent();
            w.line("}");
        }
    }

    fn simple_stmt(&self, w: &mut CodeWriter, stmt: &Stmt) {
        match stmt {
            Stmt::Assign { dst, expr } => {
                w.line(format!("{} = {};", self.temp_ref(*dst), self.expr(expr)));
            }
            Stmt::LocalSet { idx, expr } => {
                w.line(format!("{} = {};", self.local_ref(*idx), self.expr(expr)));
            }
            Stmt::GlobalSet { idx, expr } => {
                w.line(format!("g{idx} = {};", self.expr(expr)));
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                w.line(format!(
                    "memory.{}({}, {});",
                    self.mem(store_method(*op)),
                    self.addr(addr, *offset),
                    self.expr(value)
                ));
            }
            Stmt::Br(target) => self.branch(w, target),
            Stmt::BrIf { cond, target } => {
                w.line(format!("if (({}) != 0) {{", self.expr(cond)));
                w.indent();
                self.branch(w, target);
                w.dedent();
                w.line("}");
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
                w.line(format!("switch ({}) {{", self.expr(index)));
                for (n, target) in targets.iter().enumerate() {
                    w.line(format!("case {n}: {{"));
                    w.indent();
                    self.branch(w, target);
                    w.line("break;");
                    w.dedent();
                    w.line("}");
                }
                w.line("default: {");
                w.indent();
                self.branch(w, default);
                w.dedent();
                w.line("}");
                w.line("}");
            }
            Stmt::Return { values } => self.return_stmt(w, values),
            Stmt::Call {
                func,
                args,
                results,
            } => {
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                w.line(self.assign_results(results, self.call_string(*func, &args)));
            }
            Stmt::CallIndirect {
                type_idx,
                table_index,
                index,
                args,
                results,
            } => {
                self.use_unit("table/call");
                let boxed = args
                    .iter()
                    .map(|a| self.expr(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                let fnv = format!(
                    "t{table_index}.call({}, {})",
                    self.expr(index),
                    java_string(&self.type_symbol(*type_idx))
                );
                let ty = &self.module.types[*type_idx as usize];
                let call = self.invoke_string(&fnv, &boxed, ty.results.first().copied());
                w.line(self.assign_results(results, call));
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                w.line(format!(
                    "{} = memory.grow({});",
                    self.temp_ref(*dst),
                    self.expr(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "memory.copy(Integer.toUnsignedLong({}), Integer.toUnsignedLong({}), Integer.toUnsignedLong({}));",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "memory.fill(Integer.toUnsignedLong({}), Integer.toUnsignedLong({}), Integer.toUnsignedLong({}));",
                    self.expr(dst),
                    self.expr(val),
                    self.expr(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "memory.init(Integer.toUnsignedLong({}), data{seg}, Integer.toUnsignedLong({}), Integer.toUnsignedLong({}));",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::DataDrop { seg } => {
                w.line(format!("data{seg} = new byte[0];"));
            }
            Stmt::Unreachable => {
                // Void method that throws: emitting it as a statement (not a
                // `throw`) avoids an "unreachable statement" error after it.
                w.line(format!("{}(\"unreachable\");", self.rt("trap")));
            }
            Stmt::TableInit { .. } | Stmt::TableCopy { .. } | Stmt::ElemDrop { .. } => {
                unreachable!("table bulk ops are rejected by check_module_support");
            }
            Stmt::Block { .. } | Stmt::Loop { .. } | Stmt::If { .. } => {
                unreachable!("structured statement routed to simple_stmt");
            }
        }
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        if let Some(v) = values.first() {
            w.line(format!("{} = {};", self.ret(), self.expr(v)));
        }
        w.line(format!("{} = -1;", self.br()));
    }

    fn branch(&self, w: &mut CodeWriter, target: &BrTarget) {
        match target {
            BrTarget::Return { values } => self.return_stmt(w, values),
            BrTarget::Label { label, assigns, .. } => {
                for (dst, src) in assigns {
                    w.line(format!(
                        "{} = {};",
                        self.temp_ref(*dst),
                        self.temp_ref(*src)
                    ));
                }
                // is_loop is irrelevant: the loop trailer turns `_br == <loop
                // id>` into a `continue`; a block/if exit is resolved by the
                // guards skipping to the label's reset marker (ADR-30).
                w.line(format!("{} = {};", self.br(), label));
            }
        }
    }

    fn assign_results(&self, results: &[Temp], call: String) -> String {
        match results.first() {
            None => format!("{call};"),
            Some(t) => format!("{} = {call};", self.temp_ref(*t)),
        }
    }

    /// A direct call to a function by index (imported → boxed `Fn` invoke;
    /// defined → primitive method call).
    fn call_string(&self, func_idx: u32, args: &[String]) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            let ty = self.func_type(func_idx);
            let boxed = args.join(", ");
            self.invoke_string(
                &format!("if{func_idx}"),
                &boxed,
                ty.results.first().copied(),
            )
        } else {
            format!("f{func_idx}({})", args.join(", "))
        }
    }

    /// Invoke an `Rt.Fn` value with boxed args, unboxing the single result (if
    /// any) to its Java primitive.
    fn invoke_string(&self, fnv: &str, boxed_args: &str, result: Option<ValType>) -> String {
        let call = format!("{fnv}.invoke(new Object[]{{{boxed_args}}})");
        match result {
            None => call,
            Some(ty) => unbox(ty, &call),
        }
    }

    fn addr(&self, addr: &Expr, offset: u64) -> String {
        if offset == 0 {
            format!("Integer.toUnsignedLong({})", self.expr(addr))
        } else {
            format!("Integer.toUnsignedLong({}) + {offset}L", self.expr(addr))
        }
    }

    fn expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::I32Const(v) => format!("0x{v:x}"),
            Expr::I64Const(v) => format!("0x{v:x}L"),
            Expr::F32Const(bits) => format!("Float.intBitsToFloat(0x{bits:x})"),
            Expr::F64Const(bits) => format!("Double.longBitsToDouble(0x{bits:x}L)"),
            Expr::Temp(t) => self.temp_ref(*t),
            Expr::LocalGet(idx) => self.local_ref(*idx),
            Expr::GlobalGet(idx) => format!("g{idx}"),
            Expr::Un(op, a) => self.un(*op, &self.expr(a)),
            Expr::Bin(op, a, b) => self.bin(*op, &self.expr(a), &self.expr(b)),
            Expr::Load { op, addr, offset } => {
                format!(
                    "memory.{}({})",
                    self.mem(load_method(*op)),
                    self.addr(addr, *offset)
                )
            }
            Expr::Select { cond, then, els } => {
                format!(
                    "(({}) != 0 ? ({}) : ({}))",
                    self.expr(cond),
                    self.expr(then),
                    self.expr(els)
                )
            }
            Expr::MemorySize => {
                self.use_unit("memory/size");
                "memory.size()".to_string()
            }
        }
    }

    fn un(&self, op: UnOp, a: &str) -> String {
        use UnOp::*;
        match op {
            I32Eqz | I64Eqz => format!("(({a}) == 0 ? 1 : 0)"),
            I32Clz => format!("Integer.numberOfLeadingZeros({a})"),
            I32Ctz => format!("Integer.numberOfTrailingZeros({a})"),
            I32Popcnt => format!("Integer.bitCount({a})"),
            I64Clz => format!("(long) Long.numberOfLeadingZeros({a})"),
            I64Ctz => format!("(long) Long.numberOfTrailingZeros({a})"),
            I64Popcnt => format!("(long) Long.bitCount({a})"),
            F32Abs => format!("Math.abs({a})"),
            F32Neg => format!("(-({a}))"),
            F64Abs => format!("Math.abs({a})"),
            F64Neg => format!("(-({a}))"),
            F32Ceil => format!("((float) Math.ceil({a}))"),
            F32Floor => format!("((float) Math.floor({a}))"),
            F32Trunc => format!("((float) (({a}) < 0 ? Math.ceil({a}) : Math.floor({a})))"),
            F32Nearest => format!("((float) Math.rint({a}))"),
            F32Sqrt => format!("((float) Math.sqrt({a}))"),
            F64Ceil => format!("Math.ceil({a})"),
            F64Floor => format!("Math.floor({a})"),
            F64Trunc => format!("(({a}) < 0 ? Math.ceil({a}) : Math.floor({a}))"),
            F64Nearest => format!("Math.rint({a})"),
            F64Sqrt => format!("Math.sqrt({a})"),
            I32WrapI64 => format!("((int) ({a}))"),
            // Trapping float->int conversions saturate in the milestone (a
            // documented gap; the trapping form is a spec-milestone task,
            // ADR-30). The saturating forms are exactly Java's cast semantics.
            I32TruncF32S | I32TruncF64S | I32TruncSatF32S | I32TruncSatF64S => {
                format!("((int) ({a}))")
            }
            I32TruncF32U | I32TruncF64U | I32TruncSatF32U | I32TruncSatF64U => {
                format!("((int) (long) ({a}))")
            }
            I64TruncF32S | I64TruncF64S | I64TruncSatF32S | I64TruncSatF64S => {
                format!("((long) ({a}))")
            }
            I64TruncF32U | I64TruncF64U | I64TruncSatF32U | I64TruncSatF64U => {
                format!("((long) ({a}))")
            }
            I64ExtendI32S => format!("((long) ({a}))"),
            I64ExtendI32U => format!("Integer.toUnsignedLong({a})"),
            F32ConvertI32S => format!("((float) ({a}))"),
            F32ConvertI32U => format!("((float) Integer.toUnsignedLong({a}))"),
            F32ConvertI64S => format!("((float) ({a}))"),
            F32ConvertI64U => format!("((float) ({a}))"),
            F64ConvertI32S => format!("((double) ({a}))"),
            F64ConvertI32U => format!("((double) Integer.toUnsignedLong({a}))"),
            F64ConvertI64S => format!("((double) ({a}))"),
            F64ConvertI64U => format!("((double) ({a}))"),
            F32DemoteF64 => format!("((float) ({a}))"),
            F64PromoteF32 => format!("((double) ({a}))"),
            I32ReinterpretF32 => format!("Float.floatToRawIntBits({a})"),
            I64ReinterpretF64 => format!("Double.doubleToRawLongBits({a})"),
            F32ReinterpretI32 => format!("Float.intBitsToFloat({a})"),
            F64ReinterpretI64 => format!("Double.longBitsToDouble({a})"),
            I32Extend8S => format!("((int) (byte) ({a}))"),
            I32Extend16S => format!("((int) (short) ({a}))"),
            I64Extend8S => format!("((long) (byte) ({a}))"),
            I64Extend16S => format!("((long) (short) ({a}))"),
            I64Extend32S => format!("((long) (int) ({a}))"),
        }
    }

    fn bin(&self, op: BinOp, a: &str, b: &str) -> String {
        use BinOp::*;
        match op {
            I32Add | I64Add => format!("(({a}) + ({b}))"),
            I32Sub | I64Sub => format!("(({a}) - ({b}))"),
            I32Mul | I64Mul => format!("(({a}) * ({b}))"),
            I32DivS => format!("{}({a}, {b})", self.rt("i32_div_s")),
            I32DivU => format!("{}({a}, {b})", self.rt("i32_div_u")),
            I32RemS => format!("{}({a}, {b})", self.rt("i32_rem_s")),
            I32RemU => format!("{}({a}, {b})", self.rt("i32_rem_u")),
            I64DivS => format!("{}({a}, {b})", self.rt("i64_div_s")),
            I64DivU => format!("{}({a}, {b})", self.rt("i64_div_u")),
            I64RemS => format!("{}({a}, {b})", self.rt("i64_rem_s")),
            I64RemU => format!("{}({a}, {b})", self.rt("i64_rem_u")),
            I32And | I64And => format!("(({a}) & ({b}))"),
            I32Or | I64Or => format!("(({a}) | ({b}))"),
            I32Xor | I64Xor => format!("(({a}) ^ ({b}))"),
            I32Shl => format!("(({a}) << (({b}) & 31))"),
            I32ShrU => format!("(({a}) >>> (({b}) & 31))"),
            I32ShrS => format!("(({a}) >> (({b}) & 31))"),
            I64Shl => format!("(({a}) << (int) (({b}) & 63L))"),
            I64ShrU => format!("(({a}) >>> (int) (({b}) & 63L))"),
            I64ShrS => format!("(({a}) >> (int) (({b}) & 63L))"),
            I32Rotl => format!("Integer.rotateLeft({a}, {b})"),
            I32Rotr => format!("Integer.rotateRight({a}, {b})"),
            I64Rotl => format!("Long.rotateLeft({a}, (int) ({b}))"),
            I64Rotr => format!("Long.rotateRight({a}, (int) ({b}))"),
            I32Eq | I64Eq | F32Eq | F64Eq => format!("(({a}) == ({b}) ? 1 : 0)"),
            I32Ne | I64Ne | F32Ne | F64Ne => format!("(({a}) != ({b}) ? 1 : 0)"),
            I32LtS | I64LtS | F32Lt | F64Lt => format!("(({a}) < ({b}) ? 1 : 0)"),
            I32GtS | I64GtS | F32Gt | F64Gt => format!("(({a}) > ({b}) ? 1 : 0)"),
            I32LeS | I64LeS | F32Le | F64Le => format!("(({a}) <= ({b}) ? 1 : 0)"),
            I32GeS | I64GeS | F32Ge | F64Ge => format!("(({a}) >= ({b}) ? 1 : 0)"),
            I32LtU => format!("(Integer.compareUnsigned({a}, {b}) < 0 ? 1 : 0)"),
            I32GtU => format!("(Integer.compareUnsigned({a}, {b}) > 0 ? 1 : 0)"),
            I32LeU => format!("(Integer.compareUnsigned({a}, {b}) <= 0 ? 1 : 0)"),
            I32GeU => format!("(Integer.compareUnsigned({a}, {b}) >= 0 ? 1 : 0)"),
            I64LtU => format!("(Long.compareUnsigned({a}, {b}) < 0 ? 1 : 0)"),
            I64GtU => format!("(Long.compareUnsigned({a}, {b}) > 0 ? 1 : 0)"),
            I64LeU => format!("(Long.compareUnsigned({a}, {b}) <= 0 ? 1 : 0)"),
            I64GeU => format!("(Long.compareUnsigned({a}, {b}) >= 0 ? 1 : 0)"),
            F32Add | F64Add => format!("(({a}) + ({b}))"),
            F32Sub | F64Sub => format!("(({a}) - ({b}))"),
            F32Mul | F64Mul => format!("(({a}) * ({b}))"),
            F32Div | F64Div => format!("(({a}) / ({b}))"),
            F32Min | F64Min => format!("Math.min({a}, {b})"),
            F32Max | F64Max => format!("Math.max({a}, {b})"),
            F32Copysign | F64Copysign => format!("Math.copySign({a}, {b})"),
        }
    }
}

/// Box an `Object` back to a Java primitive of the wasm value type.
fn unbox(ty: ValType, expr: &str) -> String {
    match ty {
        ValType::I32 => format!("(int)(Integer) {expr}"),
        ValType::I64 => format!("(long)(Long) {expr}"),
        ValType::F32 => format!("(float)(Float) {expr}"),
        ValType::F64 => format!("(double)(Double) {expr}"),
        ValType::FuncRef => format!("(Rt.Funcref) {expr}"),
    }
}

/// An ENOSYS stub `Rt.Fn` for an unimplemented WASI import: an i32-result
/// syscall returns errno 52, everything else returns zero values / null.
fn enosys_stub(ty: &dewasm_core::ir::FuncType) -> String {
    match ty.results.first() {
        Some(ValType::I32) => "__a -> 52".to_string(),
        Some(t) => format!("__a -> {}", boxed_zero(*t)),
        None => "__a -> null".to_string(),
    }
}

fn boxed_zero(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "0",
        ValType::I64 => "0L",
        ValType::F32 => "0.0f",
        ValType::F64 => "0.0",
        ValType::FuncRef => "null",
    }
}

/// Emit a data blob as a chunked-Base64 constant decoded at runtime, staying
/// under Java's 64KB string-literal limit (ADR-30).
fn data_blob(data: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut chunks = String::new();
    for (i, chunk) in data.chunks(DATA_CHUNK).enumerate() {
        if i > 0 {
            chunks.push_str(", ");
        }
        let b64 = base64_encode(chunk);
        let _ = write!(chunks, "\"{b64}\"");
    }
    format!("Rt.data_from_b64(new String[]{{{chunks}}})")
}

/// Standard Base64 (RFC 4648) encoder — matches `java.util.Base64.getDecoder`.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18 & 0x3f) as usize] as char);
        out.push(ALPHABET[(n >> 12 & 0x3f) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
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

// --- branch-target free sets (drive the `_br` guard placement) --------------

/// The set of label ids a statement branches to that are *not* bound within it
/// (a `Return` contributes the RETURN sentinel). Non-empty means the statement
/// may leave `_br` set on fall-through, so following siblings must be guarded
/// (ADR-28/ADR-30).
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
        Stmt::Return { .. } => {
            let mut s = BTreeSet::new();
            s.insert(RETURN_SENTINEL);
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
    let mut s = BTreeSet::new();
    match t {
        BrTarget::Return { .. } => {
            s.insert(RETURN_SENTINEL);
        }
        BrTarget::Label { label, .. } => {
            s.insert(*label);
        }
    }
    s
}

// --- cost estimate (drives the 64KB method split) ---------------------------

fn seq_cost(stmts: &[Stmt]) -> usize {
    stmts.iter().map(stmt_cost).sum()
}

fn stmt_cost(stmt: &Stmt) -> usize {
    1 + match stmt {
        Stmt::Assign { expr, .. } | Stmt::LocalSet { expr, .. } | Stmt::GlobalSet { expr, .. } => {
            expr_cost(expr)
        }
        Stmt::Store { addr, value, .. } => expr_cost(addr) + expr_cost(value),
        Stmt::Block { body, .. } | Stmt::Loop { body, .. } => seq_cost(body),
        Stmt::If {
            cond, then, els, ..
        } => expr_cost(cond) + seq_cost(then) + seq_cost(els),
        Stmt::Br(t) => target_cost(t),
        Stmt::BrIf { cond, target } => expr_cost(cond) + target_cost(target),
        Stmt::BrTable {
            index,
            targets,
            default,
        } => {
            expr_cost(index) + target_cost(default) + targets.iter().map(target_cost).sum::<usize>()
        }
        Stmt::Return { values } => values.iter().map(expr_cost).sum(),
        Stmt::Call { args, .. } => args.iter().map(expr_cost).sum(),
        Stmt::CallIndirect { index, args, .. } => {
            expr_cost(index) + args.iter().map(expr_cost).sum::<usize>()
        }
        Stmt::MemoryGrow { delta, .. } => expr_cost(delta),
        Stmt::MemoryCopy { dst, src, len }
        | Stmt::MemoryFill { dst, val: src, len }
        | Stmt::MemoryInit { dst, src, len, .. } => {
            expr_cost(dst) + expr_cost(src) + expr_cost(len)
        }
        Stmt::TableInit { dst, src, len, .. } | Stmt::TableCopy { dst, src, len, .. } => {
            expr_cost(dst) + expr_cost(src) + expr_cost(len)
        }
        Stmt::DataDrop { .. } | Stmt::ElemDrop { .. } | Stmt::Unreachable => 0,
    }
}

fn target_cost(t: &BrTarget) -> usize {
    match t {
        BrTarget::Return { values } => values.iter().map(expr_cost).sum(),
        BrTarget::Label { assigns, .. } => assigns.len(),
    }
}

fn expr_cost(expr: &Expr) -> usize {
    1 + match expr {
        Expr::Un(_, a) => expr_cost(a),
        Expr::Bin(_, a, b) => expr_cost(a) + expr_cost(b),
        Expr::Load { addr, .. } => expr_cost(addr),
        Expr::Select { cond, then, els } => expr_cost(cond) + expr_cost(then) + expr_cost(els),
        _ => 0,
    }
}
