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

use anyhow::Result;
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

/// Element-segment size above which the funcref table is built by the nested
/// `Elem` helper class instead of inline in the constructor. A big table's
/// thousands of funcref lambdas overflow both `<init>`'s 64KB limit and the
/// module class's 65535-entry constant pool; the nested class has its own pool
/// (ADR-30 third milestone). Tuned so qjs/sqlite (~550 entries) stay inline and
/// only rg-scale tables (~4900) split.
const ELEM_SPLIT: usize = 1024;

/// Funcref entries per `elem{i}_pK` part method (each ~20 bytes of bytecode per
/// entry stays well under the 64KB method limit).
const ELEM_PART: usize = 512;

/// Defined-function count above which the module's functions are split across
/// nested `P{k}` helper classes, each with its own 65535-entry constant pool
/// (ADR-30 third milestone). A single class holding thousands of functions
/// (their numeric literals, method refs, and names) overflows the pool: qjs
/// (~1500) and sqlite (~1970) fit, but rg (~7300) does not. Kept above sqlite's
/// proven single-class size so only rg-scale modules partition.
const FN_PARTITION_THRESHOLD: usize = 3000;

/// Defined functions per partition class. Kept under sqlite's proven
/// single-class function count so no partition's pool overflows.
const FN_PER_PARTITION: usize = 1500;

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
                    prefix: "global",
                    open: "final class Global {",
                    close: "}",
                    prelude: Some("global/_class"),
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
            // Wasm-1.0 completion (ADR-16, mirrored from Go's ADR-29): imported
            // globals/memories/tables through the provider map with an
            // `instanceof` kind check, multiple tables, and the table half of
            // bulk memory (passive/declared element segments, table.init/copy,
            // elem.drop).
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
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

/// Emit just the module class (constructor, functions, and the spec-harness
/// `invoke`/`globalGet` dispatch methods), for the shared spec harness (ADR-3):
/// the harness bundles one runtime for every module in a `.wast` file, so
/// per-module output carries no runtime classes / `Main`. Multi-value results,
/// wasm-1.0 imports, and trapping conversions are all exercised here. Returns
/// the class source and the runtime units it references.
pub fn generate_program_with_units(
    module: &Module,
    type_name: &str,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&JavaBackend, module)?;
    let gen = new_gen(module, type_name.to_string(), false);
    let mut body = CodeWriter::new("    ");
    gen.constructor(&mut body);
    for (i, func) in module.funcs.iter().enumerate() {
        body.line("");
        let idx = module.num_imported_funcs() as usize + i;
        gen.function(&mut body, idx as u32, func);
    }
    body.line("");
    gen.emit_invoke_method(&mut body);
    body.line("");
    gen.emit_global_get_method(&mut body);

    let mut out = format!("final class {type_name} {{\n");
    out.push_str(&reindent(&body.finish(), 1));
    out.push_str("}\n");
    Ok((out, gen.uses.into_inner()))
}

fn new_gen(module: &Module, type_name: String, default_wasi: bool) -> Gen<'_> {
    Gen {
        module,
        default_wasi,
        type_name,
        uses: RefCell::new(BTreeSet::new()),
        split: Cell::new(false),
        next_part: Cell::new(0),
        mv_counter: Cell::new(0),
        cur_base: RefCell::new(String::new()),
        cur_frame_ty: RefCell::new(String::new()),
        part_defs: RefCell::new(Vec::new()),
        elem_capture: Cell::new(false),
        partitioned: Cell::new(false),
        in_partition: Cell::new(false),
    }
}

fn generate_source(module: &Module, opts: &GenOptions) -> Result<String> {
    let type_name = type_name(&opts.module_name);
    let gen = new_gen(module, type_name.clone(), opts.default_wasi);
    // A module whose function count crosses the threshold is split across nested
    // `P{k}` classes, each with its own constant pool (ADR-30). Set before the
    // constructor: its exports/start emit function calls through `defined_call`,
    // which qualifies them by partition.
    gen.partitioned
        .set(module.funcs.len() > FN_PARTITION_THRESHOLD);

    // Emit the module class body into its own writer so `uses` is populated
    // before the runtime bundle is assembled.
    let mut body = CodeWriter::new("    ");
    gen.constructor(&mut body);
    let num_imported = module.num_imported_funcs() as usize;
    if gen.partitioned.get() {
        gen.emit_partition_classes(&mut body, num_imported);
    } else {
        for (i, func) in module.funcs.iter().enumerate() {
            body.line("");
            gen.function(&mut body, (num_imported + i) as u32, func);
        }
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
    let preopens = if wasi { "preopens" } else { "null" };
    // Standalone WASI reads DEWASM_PREOPEN ("guest=host,...") into the preopens
    // argument, kept separate from argv since argv already mirrors the guest's
    // own argv (ADR-14). Without WASI there is nothing to preopen.
    let arg_setup = if wasi {
        format!(
            "        String[] wasiArgs = new String[argv.length + 1];\n\
             {ind}wasiArgs[0] = {name};\n\
             {ind}System.arraycopy(argv, 0, wasiArgs, 1, argv.length);\n\
             {ind}java.util.Map<String, String> preopens = new java.util.HashMap<>();\n\
             {ind}String preopenEnv = System.getenv(\"DEWASM_PREOPEN\");\n\
             {ind}if (preopenEnv != null) {{\n\
             {ind}    for (String kv : preopenEnv.split(\",\")) {{\n\
             {ind}        int eq = kv.indexOf('=');\n\
             {ind}        if (eq >= 0) {{\n\
             {ind}            preopens.put(kv.substring(0, eq), kv.substring(eq + 1));\n\
             {ind}        }}\n\
             {ind}    }}\n\
             {ind}}}\n",
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
        "        {type_name} p = new {type_name}(null, {args}, new String[0], {preopens});\n"
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
    /// Monotonic counter for multi-value result destructuring temps (`__mvN`).
    mv_counter: Cell<usize>,
    /// Base name (`f47`) and frame type (`Frame47`) of the current function.
    cur_base: RefCell<String>,
    cur_frame_ty: RefCell<String>,
    /// Part-method definitions produced while emitting the current function,
    /// flushed after its entry method.
    part_defs: RefCell<Vec<String>>,
    /// When true, a function value (`func_value`) captures the module instance
    /// as `inst.` rather than referencing `this` implicitly. Set while emitting
    /// the nested `Elem` helper class's static methods, whose funcref lambdas
    /// live in a separate constant pool (ADR-30 third milestone).
    elem_capture: Cell<bool>,
    /// Whether this module's functions are split across nested `P{k}` classes
    /// (its function count crosses `FN_PARTITION_THRESHOLD`). When set, defined
    /// functions are `static` methods taking the module instance, called
    /// class-qualified (ADR-30 third milestone).
    partitioned: Cell<bool>,
    /// True while emitting a function body inside a `P{k}` partition class, so
    /// instance references resolve through the passed `inst` parameter.
    in_partition: Cell<bool>,
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

    // --- instance-reference prefixing (function partitioning, ADR-30) --------

    /// Whether the code being emitted reaches the module's instance fields
    /// through a passed `inst` parameter (a `P{k}` function body or the `Elem`
    /// helper class) rather than an implicit `this`.
    fn via_inst(&self) -> bool {
        self.in_partition.get() || self.elem_capture.get()
    }

    /// Reference an instance field (`memory`, `g3`, `t0`, `if5`, `data2`,
    /// `elem0`): `inst.<name>` when reached via a passed instance, else the bare
    /// name (implicit `this`).
    fn iref(&self, name: &str) -> String {
        if self.via_inst() {
            format!("inst.{name}")
        } else {
            name.to_string()
        }
    }

    /// The instance to pass as the first argument of a partitioned static
    /// function call: `inst` inside a `P{k}`/`Elem` method, else `this`.
    fn self_arg(&self) -> &'static str {
        if self.via_inst() {
            "inst"
        } else {
            "this"
        }
    }

    /// The `P{k}` partition class holding defined function `func_idx`.
    fn partition_of(&self, func_idx: u32) -> usize {
        (func_idx as usize - self.module.imported_funcs.len()) / FN_PER_PARTITION
    }

    /// A function/part method head. In a partitioned module the method is
    /// `static` and takes the module instance as its first parameter, so it can
    /// live in a `P{k}` class with its own constant pool (ADR-30 third
    /// milestone); otherwise it is a plain instance method.
    fn method_head(&self, ret_ty: &str, name: &str, params: &str) -> String {
        if self.partitioned.get() {
            let inst = &self.type_name;
            if params.is_empty() {
                format!("static {ret_ty} {name}({inst} inst) {{")
            } else {
                format!("static {ret_ty} {name}({inst} inst, {params}) {{")
            }
        } else {
            format!("{ret_ty} {name}({params}) {{")
        }
    }

    /// A call to a *defined* function by index, honoring partitioning: a
    /// partitioned module calls `P{k}.f{idx}(<inst>, args)`; otherwise
    /// `[inst.]f{idx}(args)` (the `inst.` form serves the non-partitioned `Elem`
    /// class). `args_joined` is the already-formatted argument list.
    fn defined_call(&self, func_idx: u32, args_joined: &str) -> String {
        if self.partitioned.get() {
            let k = self.partition_of(func_idx);
            let s = self.self_arg();
            if args_joined.is_empty() {
                format!("P{k}.f{func_idx}({s})")
            } else {
                format!("P{k}.f{func_idx}({s}, {args_joined})")
            }
        } else if self.via_inst() {
            format!("inst.f{func_idx}({args_joined})")
        } else {
            format!("f{func_idx}({args_joined})")
        }
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
        // A partitioned module's parts are static and take the module instance
        // alongside the frame, so instance references resolve through `inst`.
        let head = if self.partitioned.get() {
            format!(
                "static void {name}({} inst, {frame} f) {{\n",
                self.type_name
            )
        } else {
            format!("private void {name}({frame} f) {{\n")
        };
        let mut out = head;
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
            "{name}(java.util.Map<String, java.util.Map<String, Object>> imports, String[] args, String[] env, java.util.Map<String, String> preopens) {{"
        ));
        w.indent();

        // Memory: imported or locally defined (index space has at most one).
        if let Some(import) = &m.imported_memory {
            self.emit_typed_import(w, "this.memory", "Memory", &import.module, &import.name);
        }
        if let Some(mem) = &m.memory {
            self.use_unit("memory/_class");
            let max = mem.max_pages.map(|p| p as u32).unwrap_or(65536);
            w.line(format!(
                "this.memory = new Memory({}, {});",
                mem.min_pages as u32, max
            ));
        }

        // Tables: imported first, then defined (index space is
        // imported_tables ++ tables, ADR-16).
        for (i, import) in m.imported_tables.iter().enumerate() {
            self.emit_typed_import(
                w,
                &format!("this.t{i}"),
                "Table",
                &import.module,
                &import.name,
            );
        }
        let num_imported_tables = m.imported_tables.len();
        for (i, table) in m.tables.iter().enumerate() {
            self.use_unit("table/_class");
            w.line(format!(
                "this.t{} = new Table({});",
                num_imported_tables + i,
                table.min
            ));
        }

        let wasi = wasi_bundled(m, self.default_wasi);
        if wasi {
            self.use_unit("wasi/_class");
            w.line("this.wasi = new WASI(args, env, preopens);");
            if m.memory.is_some() || m.imported_memory.is_some() {
                w.line("this.wasi.memory = this.memory;");
            }
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            self.emit_import(w, i, import);
        }

        // Globals: imported first, then defined; every global is a boxed
        // `Global` (ADR-16). Defined-global inits may read imported globals, so
        // they resolve after the imported ones.
        for (i, import) in m.imported_globals.iter().enumerate() {
            self.emit_typed_import(
                w,
                &format!("this.g{i}"),
                "Global",
                &import.module,
                &import.name,
            );
        }
        let num_imported_globals = m.imported_globals.len();
        for (i, global) in m.globals.iter().enumerate() {
            self.use_unit("global/_class");
            w.line(format!(
                "this.g{} = new Global({});",
                num_imported_globals + i,
                self.expr(&global.init)
            ));
        }

        // Element segments. A segment over ELEM_SPLIT is built by the nested
        // `Elem` helper class (its own constant pool, chunked part methods),
        // else inline (ADR-30 third milestone).
        let mut split_elems: Vec<usize> = Vec::new();
        for (i, elem) in m.elems.iter().enumerate() {
            let large = elem.items.len() > ELEM_SPLIT;
            if large {
                split_elems.push(i);
            }
            let build = || {
                if large {
                    format!("Elem.elem{i}(this)")
                } else {
                    let items = elem
                        .items
                        .iter()
                        .map(|item| self.elem_item(item))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("new Rt.Funcref[]{{{items}}}")
                }
            };
            match &elem.kind {
                ElemKind::Declared => w.line(format!("this.elem{i} = new Rt.Funcref[0];")),
                ElemKind::Passive => w.line(format!("this.elem{i} = {};", build())),
                ElemKind::Active {
                    table_index,
                    offset,
                } => {
                    self.use_unit("table/init");
                    w.line(format!("this.elem{i} = {};", build()));
                    w.line(format!(
                        "this.t{table_index}.init({}, this.elem{i}, 0, {});",
                        self.expr(offset),
                        elem.items.len()
                    ));
                    // Active segments are dropped after instantiation.
                    w.line(format!("this.elem{i} = new Rt.Funcref[0];"));
                }
            }
        }

        // Each data segment is materialized in its own `initData{i}()` method,
        // not inline in the constructor. A multi-MB segment lowers to a
        // chunked-Base64 array whose initializer (plus the `memory.init` copy)
        // would otherwise accumulate toward the 64KB `<init>` limit ADR-30
        // predicted; splitting one method per segment keeps the constructor
        // bounded regardless of how large or how many the segments are.
        for i in 0..m.datas.len() {
            w.line(format!("this.initData{i}();"));
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

        // Per-segment data initializers, called in order from the constructor
        // (see above). Each is a self-contained method so an oversized segment
        // never bloats `<init>` past the 64KB method limit (ADR-30).
        for (i, data) in m.datas.iter().enumerate() {
            self.use_unit("rt/data_from_b64");
            let blob = data_blob(&data.data);
            w.line("");
            w.line(format!("private void initData{i}() {{"));
            w.indent();
            match &data.offset {
                Some(offset) => {
                    self.use_unit("memory/init");
                    w.line(format!(
                        "this.memory.init(Integer.toUnsignedLong({}), {blob}, 0, {});",
                        self.expr(offset),
                        data.data.len()
                    ));
                    // Active segments are dropped after instantiation, but stay
                    // addressable (as empty) by memory.init/data.drop.
                    w.line(format!("this.data{i} = new byte[0];"));
                }
                None => {
                    w.line(format!("this.data{i} = {blob};"));
                }
            }
            w.dedent();
            w.line("}");
        }

        // The nested `Elem` helper class builds any oversized funcref table
        // (see the element-segment loop above). It is a separate class, so its
        // thousands of funcref lambdas land in their own 65535-entry constant
        // pool instead of the module class's; each table is filled by chunked
        // `elem{i}_pK` part methods to stay under the 64KB method limit (ADR-30
        // third milestone).
        if !split_elems.is_empty() {
            self.emit_elem_class(w, &split_elems);
        }
    }

    /// Emit the nested `Elem` helper class for the oversized element segments in
    /// `split_elems`. Each segment `i` gets an `elem{i}(inst)` factory that
    /// allocates the array and calls chunked `elem{i}_pK(inst, a)` fillers.
    fn emit_elem_class(&self, w: &mut CodeWriter, split_elems: &[usize]) {
        self.elem_capture.set(true);
        w.line("");
        w.line("static final class Elem {");
        w.indent();
        let ty = &self.type_name;
        for (n, &i) in split_elems.iter().enumerate() {
            if n > 0 {
                w.line("");
            }
            let elem = &self.module.elems[i];
            let len = elem.items.len();
            let parts = len.div_ceil(ELEM_PART);
            w.line(format!("static Rt.Funcref[] elem{i}({ty} inst) {{"));
            w.indent();
            w.line(format!("Rt.Funcref[] a = new Rt.Funcref[{len}];"));
            for p in 0..parts {
                w.line(format!("elem{i}_p{p}(inst, a);"));
            }
            w.line("return a;");
            w.dedent();
            w.line("}");
            for p in 0..parts {
                w.line("");
                w.line(format!(
                    "static void elem{i}_p{p}({ty} inst, Rt.Funcref[] a) {{"
                ));
                w.indent();
                let start = p * ELEM_PART;
                let end = (start + ELEM_PART).min(len);
                for (k, item) in elem.items[start..end].iter().enumerate() {
                    w.line(format!("a[{}] = {};", start + k, self.elem_item(item)));
                }
                w.dedent();
                w.line("}");
            }
        }
        w.dedent();
        w.line("}");
        self.elem_capture.set(false);
    }

    /// Emit the module's defined functions grouped into nested `P{k}` classes of
    /// up to `FN_PER_PARTITION` functions each, so no single class's constant
    /// pool overflows Java's 65535-entry limit (ADR-30 third milestone). The
    /// functions are `static` and take the module instance; call sites reach
    /// them class-qualified via `defined_call`.
    fn emit_partition_classes(&self, w: &mut CodeWriter, num_imported: usize) {
        self.in_partition.set(true);
        let n = self.module.funcs.len();
        let parts = n.div_ceil(FN_PER_PARTITION);
        for k in 0..parts {
            w.line("");
            w.line(format!("static final class P{k} {{"));
            w.indent();
            let start = k * FN_PER_PARTITION;
            let end = (start + FN_PER_PARTITION).min(n);
            for (offset, func) in self.module.funcs[start..end].iter().enumerate() {
                if offset > 0 {
                    w.line("");
                }
                self.function(w, (num_imported + start + offset) as u32, func);
            }
            w.dedent();
            w.line("}");
        }
        self.in_partition.set(false);
    }

    fn struct_fields(&self, w: &mut CodeWriter) {
        let m = self.module;
        if m.imported_memory.is_some() || m.memory.is_some() {
            w.line("Memory memory;");
        }
        // Table index space = imported_tables ++ tables (ADR-16).
        for i in 0..(m.imported_tables.len() + m.tables.len()) {
            w.line(format!("Table t{i};"));
        }
        // Global index space = imported_globals ++ globals; each is a boxed
        // `Global` (ADR-16).
        for i in 0..(m.imported_globals.len() + m.globals.len()) {
            w.line(format!("Global g{i};"));
        }
        for i in 0..m.imported_funcs.len() {
            w.line(format!("Rt.Fn if{i};"));
        }
        if wasi_bundled(m, self.default_wasi) {
            w.line("WASI wasi;");
        }
        // Element segments retained for table.init (active ones are emptied
        // after instantiation).
        for i in 0..m.elems.len() {
            w.line(format!("Rt.Funcref[] elem{i};"));
        }
        // Every data segment is addressable by memory.init/data.drop (active
        // ones become empty after instantiation), so all get a field.
        for i in 0..m.datas.len() {
            w.line(format!("byte[] data{i};"));
        }
        w.line("java.util.Map<String, Object> Exports;");
    }

    /// Resolve a non-function import (memory/table/global) into `target`,
    /// checking its *kind* via `instanceof` (ADR-16). A wrong-kind or missing
    /// value is a link error; the finer wasm type (a global's value type and
    /// mutability, a table/memory's limits) is not checked — the import-limits
    /// gap, wider for Java than Go since these carry no static type (ADR-30).
    fn emit_typed_import(
        &self,
        w: &mut CodeWriter,
        target: &str,
        java_ty: &str,
        module: &str,
        name: &str,
    ) {
        w.line(format!(
            "{{ Object v = {}; if (v != null) {{",
            self.resolve_import_string(module, name)
        ));
        w.indent();
        w.line(format!("if (!(v instanceof {java_ty})) {{"));
        w.indent();
        w.line(format!(
            "{}({});",
            self.rt("link_error"),
            java_string(&format!("incompatible import type for {module}.{name}"))
        ));
        w.dedent();
        w.line("}");
        w.line(format!("{target} = ({java_ty}) v;"));
        w.dedent();
        w.line("} else {");
        w.indent();
        w.line(format!(
            "{}({});",
            self.rt("link_error"),
            java_string(&format!("missing import {module}.{name}"))
        ));
        w.dedent();
        w.line("} }");
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

    /// The `Rt.Fn` value for a function export / table element. When emitting
    /// the nested `Elem` helper class, functions are reached through the passed
    /// module instance (`inst.`), so the lambdas can live in that class's own
    /// constant pool (ADR-30 third milestone).
    fn func_value(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            return format!("(Rt.Fn) {}", self.iref(&format!("if{func_idx}")));
        }
        let ty = self.func_type(func_idx);
        let call_args = ty
            .params
            .iter()
            .enumerate()
            .map(|(k, t)| unbox(*t, &format!("__a[{k}]")))
            .collect::<Vec<_>>()
            .join(", ");
        let call = self.defined_call(func_idx, &call_args);
        if ty.results.is_empty() {
            format!("(Rt.Fn)(__a -> {{ {call}; return null; }})")
        } else {
            format!("(Rt.Fn)(__a -> {call})")
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
            // ref-typed global element items imply reference types (rejected at
            // conversion); unreachable, but must type-check as a Funcref.
            ElemItem::Global(idx) => {
                format!("(Rt.Funcref) {}.value", self.iref(&format!("g{idx}")))
            }
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
        let results = &ty.results;
        let has_result = !results.is_empty();
        // A method returns one value; multi-value signatures return a boxed
        // `Object[]` (ADR-30). The result register (`_ret` / frame `ret`) takes
        // the same shape.
        let ret_ty = ret_slot_ty(results);
        let ret_init = ret_slot_init(results);

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
            if has_result {
                w.line(format!("{ret_ty} ret;"));
            }
            w.dedent();
            w.line("}");

            let params_str = (0..nparams)
                .map(|i| format!("{} p{i}", jtype(local_types[i])))
                .collect::<Vec<_>>()
                .join(", ");
            w.line(self.method_head(&ret_ty, &format!("f{idx}"), &params_str));
            w.indent();
            w.line(format!("Frame{idx} f = new Frame{idx}();"));
            for i in 0..nparams {
                w.line(format!("f.l{i} = p{i};"));
            }
            if has_result {
                w.line(format!("f.ret = {ret_init};"));
            }
            self.emit_body(w, &func.body, false);
            if has_result {
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
            w.line(self.method_head(&ret_ty, &format!("f{idx}"), &params_str));
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
            if has_result {
                w.line(format!("{ret_ty} _ret = {ret_init};"));
            }
            let mut guarded = false;
            for stmt in &func.body {
                self.emit_stmt(w, stmt, &mut guarded);
            }
            if has_result {
                w.line("return _ret;");
            }
            w.dedent();
            w.line("}");
        }
    }

    /// The spec-harness reflective dispatcher: `invoke(name, args)` boxes every
    /// result into an `Object[]` (empty for a void export, one element for a
    /// single result, the function's own `Object[]` for multi-value). Mirrors
    /// Go's reflective invoke under Java's static typing.
    fn emit_invoke_method(&self, w: &mut CodeWriter) {
        w.line("Object[] invoke(String name, Object[] a) {");
        w.indent();
        w.line("switch (name) {");
        w.indent();
        for export in &self.module.exports {
            let ExportKind::Func(idx) = export.kind else {
                continue;
            };
            let ty = self.func_type(idx);
            w.line(format!("case {}: {{", java_string(&export.name)));
            w.indent();
            let args: Vec<String> = ty
                .params
                .iter()
                .enumerate()
                .map(|(k, t)| unbox(*t, &format!("a[{k}]")))
                .collect();
            match ty.results.len() {
                0 => {
                    w.line(format!("{};", self.call_string(idx, &args)));
                    w.line("return new Object[0];");
                }
                1 => {
                    w.line(format!(
                        "return new Object[]{{ {} }};",
                        self.call_string(idx, &args)
                    ));
                }
                _ => {
                    w.line(format!("return {};", self.call_multi_array(idx, &args)));
                }
            }
            w.dedent();
            w.line("}");
        }
        w.line("default: throw new RuntimeException(\"no export \" + name);");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    }

    /// The spec-harness global reader: the exported boxed global's current
    /// value in a one-element `Object[]`, so the harness treats it exactly like
    /// a single-result `invoke`.
    fn emit_global_get_method(&self, w: &mut CodeWriter) {
        w.line("Object[] globalGet(String name) {");
        w.indent();
        w.line("switch (name) {");
        w.indent();
        for export in &self.module.exports {
            let ExportKind::Global(idx) = export.kind else {
                continue;
            };
            w.line(format!(
                "case {}: return new Object[]{{ g{idx}.value }};",
                java_string(&export.name)
            ));
        }
        w.line("default: throw new RuntimeException(\"no global \" + name);");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    }

    /// Emit a statement sequence as the body of a construct (loop/if/block body
    /// or the function entry): inline when small, or split into chained `part`
    /// methods when the function is split and the sub-body is large.
    fn emit_body(&self, w: &mut CodeWriter, stmts: &[Stmt], guarded_in: bool) {
        if self.split.get() && seq_cost(stmts) > SPLIT_THRESHOLD {
            let part_args = if self.partitioned.get() {
                "inst, f"
            } else {
                "f"
            };
            for name in self.emit_parts(stmts, guarded_in) {
                w.line(format!("{name}({part_args});"));
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
                w.line(format!(
                    "{}.value = {};",
                    self.iref(&format!("g{idx}")),
                    self.expr(expr)
                ));
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                w.line(format!(
                    "{}.{}({}, {});",
                    self.iref("memory"),
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
                if self.func_type(*func).results.len() > 1 {
                    let arr = self.call_multi_array(*func, &args);
                    self.emit_multi_results(w, results, &arr);
                } else {
                    w.line(self.assign_results(results, self.call_string(*func, &args)));
                }
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
                    "{}.call({}, {})",
                    self.iref(&format!("t{table_index}")),
                    self.expr(index),
                    java_string(&self.type_symbol(*type_idx))
                );
                let ty = &self.module.types[*type_idx as usize];
                if ty.results.len() > 1 {
                    let arr = format!("(Object[]) {}", self.invoke_string(&fnv, &boxed, None));
                    self.emit_multi_results(w, results, &arr);
                } else {
                    let call = self.invoke_string(&fnv, &boxed, ty.results.first().copied());
                    w.line(self.assign_results(results, call));
                }
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                w.line(format!(
                    "{} = {}.grow({});",
                    self.temp_ref(*dst),
                    self.iref("memory"),
                    self.expr(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "{}.copy(Integer.toUnsignedLong({}), Integer.toUnsignedLong({}), Integer.toUnsignedLong({}));",
                    self.iref("memory"),
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "{}.fill(Integer.toUnsignedLong({}), Integer.toUnsignedLong({}), Integer.toUnsignedLong({}));",
                    self.iref("memory"),
                    self.expr(dst),
                    self.expr(val),
                    self.expr(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "{}.init(Integer.toUnsignedLong({}), {}, Integer.toUnsignedLong({}), Integer.toUnsignedLong({}));",
                    self.iref("memory"),
                    self.expr(dst),
                    self.iref(&format!("data{seg}")),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::DataDrop { seg } => {
                w.line(format!(
                    "{} = new byte[0];",
                    self.iref(&format!("data{seg}"))
                ));
            }
            Stmt::Unreachable => {
                // Void method that throws: emitting it as a statement (not a
                // `throw`) avoids an "unreachable statement" error after it.
                w.line(format!("{}(\"unreachable\");", self.rt("trap")));
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
                    "{}.init({}, {}, {}, {});",
                    self.iref(&format!("t{table_index}")),
                    self.expr(dst),
                    self.iref(&format!("elem{seg}")),
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
                    "{}.copy({}, {}, {}, {});",
                    self.iref(&format!("t{dst_table}")),
                    self.expr(dst),
                    self.iref(&format!("t{src_table}")),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::ElemDrop { seg } => {
                w.line(format!(
                    "{} = new Rt.Funcref[0];",
                    self.iref(&format!("elem{seg}"))
                ));
            }
            Stmt::Block { .. } | Stmt::Loop { .. } | Stmt::If { .. } => {
                unreachable!("structured statement routed to simple_stmt");
            }
        }
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        match values.len() {
            0 => {}
            1 => w.line(format!("{} = {};", self.ret(), self.expr(&values[0]))),
            _ => {
                // Multi-value: box each into the function's `Object[]` register.
                let vs = values
                    .iter()
                    .map(|v| self.expr(v))
                    .collect::<Vec<_>>()
                    .join(", ");
                w.line(format!("{} = new Object[]{{{vs}}};", self.ret()));
            }
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

    /// The `Object[]` a multi-value call produces: a defined method returns one
    /// directly; an imported `Fn.invoke` returns `Object`, cast to `Object[]`.
    fn call_multi_array(&self, func_idx: u32, args: &[String]) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            let boxed = args.join(", ");
            format!(
                "(Object[]) {}",
                self.invoke_string(&self.iref(&format!("if{func_idx}")), &boxed, None)
            )
        } else {
            self.defined_call(func_idx, &args.join(", "))
        }
    }

    /// Destructure a multi-value call's `Object[]` into the result temps,
    /// unboxing each slot to its wasm type (ADR-30).
    fn emit_multi_results(&self, w: &mut CodeWriter, results: &[Temp], arr: &str) {
        let n = self.mv_counter.get();
        self.mv_counter.set(n + 1);
        let mv = format!("__mv{n}");
        w.line(format!("Object[] {mv} = {arr};"));
        for (i, t) in results.iter().enumerate() {
            w.line(format!(
                "{} = {};",
                self.temp_ref(*t),
                unbox(t.ty, &format!("{mv}[{i}]"))
            ));
        }
    }

    /// A direct call to a function by index (imported → boxed `Fn` invoke;
    /// defined → primitive method call).
    fn call_string(&self, func_idx: u32, args: &[String]) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            let ty = self.func_type(func_idx);
            let boxed = args.join(", ");
            self.invoke_string(
                &self.iref(&format!("if{func_idx}")),
                &boxed,
                ty.results.first().copied(),
            )
        } else {
            self.defined_call(func_idx, &args.join(", "))
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
            Expr::GlobalGet(idx) => unbox(
                self.module.global_type(*idx),
                &format!("{}.value", self.iref(&format!("g{idx}"))),
            ),
            Expr::Un(op, a) => self.un(*op, &self.expr(a)),
            Expr::Bin(op, a, b) => self.bin(*op, &self.expr(a), &self.expr(b)),
            Expr::Load { op, addr, offset } => {
                format!(
                    "{}.{}({})",
                    self.iref("memory"),
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
                format!("{}.size()", self.iref("memory"))
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
            // abs/neg are bit ops on the sign bit: they must NOT quiet a NaN
            // (Math.abs would leave a negative NaN's sign set) (ADR-2).
            F32Abs => format!("Float.intBitsToFloat(Float.floatToRawIntBits({a}) & 0x7fffffff)"),
            F32Neg => format!("Float.intBitsToFloat(Float.floatToRawIntBits({a}) ^ 0x80000000)"),
            F64Abs => format!(
                "Double.longBitsToDouble(Double.doubleToRawLongBits({a}) & 0x7fffffffffffffffL)"
            ),
            F64Neg => format!(
                "Double.longBitsToDouble(Double.doubleToRawLongBits({a}) ^ 0x8000000000000000L)"
            ),
            // ceil/floor/nearest/sqrt canonicalize a NaN result to wasm's
            // arithmetic NaN (Java's Math.* may pass a signaling operand through
            // unquieted); trunc is a helper (single operand eval) (ADR-2).
            F32Ceil => format!("{}((float) Math.ceil({a}))", self.rt("f32_canon")),
            F32Floor => format!("{}((float) Math.floor({a}))", self.rt("f32_canon")),
            F32Trunc => format!("{}({a})", self.rt("f32_trunc")),
            F32Nearest => format!("{}((float) Math.rint({a}))", self.rt("f32_canon")),
            F32Sqrt => format!("{}((float) Math.sqrt({a}))", self.rt("f32_canon")),
            F64Ceil => format!("{}(Math.ceil({a}))", self.rt("f64_canon")),
            F64Floor => format!("{}(Math.floor({a}))", self.rt("f64_canon")),
            F64Trunc => format!("{}({a})", self.rt("f64_trunc")),
            F64Nearest => format!("{}(Math.rint({a}))", self.rt("f64_canon")),
            F64Sqrt => format!("{}(Math.sqrt({a}))", self.rt("f64_canon")),
            I32WrapI64 => format!("((int) ({a}))"),
            // Trapping float->int conversions go through helpers that trap on
            // NaN/overflow; the source is widened to double first (exact for
            // f32). The saturating signed forms are exactly Java's cast; the
            // saturating unsigned forms need helpers (Java's cast wraps past the
            // unsigned range) (ADR-2).
            I32TruncF32S | I32TruncF64S => format!("{}((double)({a}))", self.rt("i32_trunc_s")),
            I32TruncF32U | I32TruncF64U => format!("{}((double)({a}))", self.rt("i32_trunc_u")),
            I64TruncF32S | I64TruncF64S => format!("{}((double)({a}))", self.rt("i64_trunc_s")),
            I64TruncF32U | I64TruncF64U => format!("{}((double)({a}))", self.rt("i64_trunc_u")),
            I32TruncSatF32S | I32TruncSatF64S => format!("((int) ({a}))"),
            I32TruncSatF32U | I32TruncSatF64U => {
                format!("{}((double)({a}))", self.rt("i32_trunc_sat_u"))
            }
            I64TruncSatF32S | I64TruncSatF64S => format!("((long) ({a}))"),
            I64TruncSatF32U | I64TruncSatF64U => {
                format!("{}((double)({a}))", self.rt("i64_trunc_sat_u"))
            }
            I64ExtendI32S => format!("((long) ({a}))"),
            I64ExtendI32U => format!("Integer.toUnsignedLong({a})"),
            F32ConvertI32S => format!("((float) ({a}))"),
            F32ConvertI32U => format!("((float) Integer.toUnsignedLong({a}))"),
            F32ConvertI64S => format!("((float) ({a}))"),
            F32ConvertI64U => format!("{}({a})", self.rt("f32_convert_i64_u")),
            F64ConvertI32S => format!("((double) ({a}))"),
            F64ConvertI32U => format!("((double) Integer.toUnsignedLong({a}))"),
            F64ConvertI64S => format!("((double) ({a}))"),
            F64ConvertI64U => format!("{}({a})", self.rt("f64_convert_i64_u")),
            F32DemoteF64 => format!("{}({a})", self.rt("f32_demote")),
            F64PromoteF32 => format!("{}({a})", self.rt("f64_promote")),
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
            // Java's Math.min/max pass a signaling NaN operand through
            // unquieted and Math.copySign may treat NaN's sign inconsistently;
            // wasm min/max return an arithmetic NaN and copysign is a pure sign
            // bit op, so both go through explicit code (ADR-2).
            F32Min => format!("{}({a}, {b})", self.rt("f32_min")),
            F32Max => format!("{}({a}, {b})", self.rt("f32_max")),
            F64Min => format!("{}({a}, {b})", self.rt("f64_min")),
            F64Max => format!("{}({a}, {b})", self.rt("f64_max")),
            F32Copysign => format!(
                "Float.intBitsToFloat((Float.floatToRawIntBits({a}) & 0x7fffffff) | (Float.floatToRawIntBits({b}) & 0x80000000))"
            ),
            F64Copysign => format!(
                "Double.longBitsToDouble((Double.doubleToRawLongBits({a}) & 0x7fffffffffffffffL) | (Double.doubleToRawLongBits({b}) & 0x8000000000000000L))"
            ),
        }
    }
}

/// The Java type of a function's result register (`_ret` / frame `ret` / the
/// method return): the single value type, `Object[]` for multi-value, or
/// `void` for no result (ADR-30).
fn ret_slot_ty(results: &[ValType]) -> String {
    match results {
        [] => "void".to_string(),
        [t] => jtype(*t).to_string(),
        _ => "Object[]".to_string(),
    }
}

/// The initial value of the result register: the value type's zero, or an
/// `Object[]` of boxed zeros for multi-value.
fn ret_slot_init(results: &[ValType]) -> String {
    match results {
        [] => String::new(),
        [t] => zero_value(*t).to_string(),
        ts => {
            let zeros = ts
                .iter()
                .map(|t| zero_value(*t))
                .collect::<Vec<_>>()
                .join(", ");
            format!("new Object[]{{{zeros}}}")
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
