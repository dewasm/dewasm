//! Java backend: translates dewasm IR into one self-contained `.java` source file.
//! The file holds one package-private module class carrying the runtime as `static` nested classes.
//! It is compiled with `javac` and run on the JVM.
//!
//! Lowering conventions:
//! - i32/i64 are native signed `int`/`long` treated as bit patterns.
//!   Unsigned operations use `Integer.*`/`Long.*` (`divideUnsigned`, `compareUnsigned`, ...).
//!   f32/f64 are native `float`/`double`.
//!   Java is strict IEEE with no FMA contraction.
//!   So f32 re-rounding and trap-free division need no helper.
//!   NaN bit paths go through `Float.floatToRawIntBits`/`intBitsToFloat` etc.
//! - Control flow uses the per-function branch register `_br`, mirroring Python's model.
//!   The register does not depend on depth and is splittable across methods.
//!   Block/if exits and the function return set `_br`.
//!   Following statements in the same sequence are guarded by `if (_br == 0)`.
//!   Only real loops become `while (true)`.
//!   No unguarded `return`/`break` sits mid-sequence.
//!   So Java's "unreachable statement" error cannot arise.
//!   It also makes the split below mechanical.
//! - Exception handling is native.
//!   A tag is an identity object (`Rt.Tag`).
//!   A thrown exception is an `Rt.WasmException` that doubles as the `exnref` value.
//!   A `try_table` is a Java `try`/`catch`.
//!   Its clauses bind the payload and then set `_br` like any other branch.
//!   Only `Rt.WasmException` is caught, never `Throwable`.
//!   So `catch_all` structurally cannot catch a trap or the exit path.
//! - The JVM caps a method at 64KB of bytecode.
//!   A function whose estimated size crosses a threshold is emitted in split form.
//!   Its locals/temps/`br`/`ret` are hoisted to a per-call **frame object**.
//!   Its body is split into numbered `part` methods sharing that frame.
//!   Control flow is data (`_br`), so the parts are just called in order.
//!   Data segments over the 64KB string-literal limit become chunked Base64 (`Rt.data_from_b64`).
//!
//! The runtime is built from per-method units referenced as `Rt.<name>`/`Memory`/`Table`/`WASI`.
//! In self-contained output those classes are nested in the module class.
//! Two artifacts then coexist in one package.
//! The shared-runtime path the specification harness drives keeps them top-level.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::{
    check_module_support, comparison, is_boolean, is_ident, is_wasi_module, load_method,
    local_runs, module_name_error, stmts_use_tail_calls, store_method, type_key, wasi_bundled,
    Backend, CodeWriter, CompareOperands, GenOptions, Mode, OutputFile, RuntimeBundler,
    RuntimeScope, SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, CatchClause, ElemItem, ElemKind, ExportKind, Expr, Func, Label, Module, Stmt,
    Temp, UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// Estimated body cost above which a function is split into `part` methods.
/// The split keeps each method under the JVM's 64KB per-method bytecode limit.
/// Cost is an IR node count; the value is tuned so `cowsay`'s largest functions compile.
const SPLIT_THRESHOLD: usize = 900;

/// Raw bytes per Base64 data chunk.
/// Base64 of this (~43.7KB) stays under Java's 64KB (65535-byte) string-literal limit.
const DATA_CHUNK: usize = 32768;

/// Element-segment size above which the nested `Elem` helper class builds the `funcref` table.
/// Below it, the table is built inline in the constructor.
/// A big table has thousands of `funcref` lambdas.
/// They overflow both `<init>`'s 64KB limit and the module class's 65535-entry constant pool.
/// The nested class has its own pool.
/// Tuned so `qjs`/SQLite (~550 entries) stay inline and only `rg`-scale tables (~4900) split.
const ELEM_SPLIT: usize = 1024;

/// `funcref` entries per `elem{i}_pK` part method.
/// At ~20 bytes of bytecode per entry, a part stays well under the 64KB method limit.
const ELEM_PART: usize = 512;

/// `funcref` entries per `ElemF{c}` filler class.
/// Every entry costs its class's constant pool a lambda and a method reference.
/// The lambda is an `invokedynamic`, a method handle and a synthetic method (~7 entries).
/// The reference is the `P{k}.f{idx}` method the lambda calls (~3 entries).
/// So one filler class saturates the 65535-entry pool at well under ten thousand entries.
/// CRuby's 8737-entry table overflowed it (issue #142).
/// Kept low enough that a filler class stays around a third of the pool.
const ELEM_PER_CLASS: usize = 2048;

/// Defined-function count above which functions are split across nested `P{k}` helper classes.
/// Each such class has its own 65535-entry constant pool.
/// A single class holding thousands of functions overflows the pool.
/// Their numeric literals, method references, and names fill it.
/// `qjs` (~1500) and SQLite (~1970) fit, but `zeroperl` (~2450) and `rg` (~7300) do not.
/// `zeroperl`'s Perl core is constant-dense enough to overflow under the former 3000 bound.
/// `javac` reported *too many constants*.
/// The value is just above SQLite's proven single-class size.
/// A module then partitions only once it exceeds the largest size measured to fit.
const FN_PARTITION_THRESHOLD: usize = 2000;

/// Defined functions per partition class.
/// Kept under SQLite's proven single-class function count so no partition's pool overflows.
const FN_PER_PARTITION: usize = 1500;

/// A branch-register value reserved for "return from the function".
/// It is distinct from any real label id, since those are small.
/// Emitted as `-1`.
const RETURN_SENTINEL: u32 = u32::MAX;

/// The runtime unit bundler for Java (see `crates/dewasm-backend-java/units/`).
/// Each scope is a *top-level* package-private class that wraps its unit bodies.
/// The bodies are methods or nested types.
/// Generated code refers to the classes as `Rt.*` / `Memory` / `Table` / `WASI`.
/// This is the shared-runtime shape.
/// The specification harness bundles one runtime for all the modules of a `.wast` file.
/// The multi-module shared-runtime composition does the same.
/// Self-contained `Embedded` output uses [`nested_bundler`] instead.
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| runtime_bundler(false))
}

/// The bundler behind `Backend::generate`'s `Embedded` output.
/// It wraps the same units, under the same simple names, as `static` **nested** classes.
/// The classes then belong to the generated module class.
/// Java resolves a simple name through outer class scopes.
/// So every `Rt.trap(...)` / `new Memory(...)` in the module class keeps its top-level spelling.
/// This includes the units.
/// Only an *outside* reference has to spell the module class (`Program.Rt.Fn`).
/// Two independently generated artifacts then sit in one package without their runtimes colliding.
fn nested_bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| runtime_bundler(true))
}

/// Build the bundler for either placement.
/// The two differ only in each scope's `open` line, so the scope list is written once.
/// A nested class needs `static`, and a top-level one may not have it.
fn runtime_bundler(nested: bool) -> RuntimeBundler {
    // `open` is `&'static str`.
    // Both spellings of each scope's class header are therefore written out, not formatted.
    let scopes = [
        (
            "rt",
            "final class Rt {",
            "static final class Rt {",
            "rt/_prelude",
        ),
        (
            "memory",
            "final class Memory {",
            "static final class Memory {",
            "memory/_class",
        ),
        (
            "table",
            "final class Table {",
            "static final class Table {",
            "table/_class",
        ),
        (
            "global",
            "final class Global {",
            "static final class Global {",
            "global/_class",
        ),
        (
            "wasi",
            "final class WASI {",
            "static final class WASI {",
            "wasi/_class",
        ),
    ]
    .iter()
    .map(|(prefix, flat, nested_open, prelude)| RuntimeScope {
        prefix,
        open: if nested { nested_open } else { flat },
        close: "}",
        prelude: Some(prelude),
    })
    .collect();
    RuntimeBundler::new("//", "\t", 4, scopes, UNIT_SOURCES).expect("runtime units are well-formed")
}

/// Locate a `java` launcher (a missing toolchain is a loud failure at the call site, not here).
/// Honors `$DEWASM_JAVA`, then `java` on `PATH`.
pub fn find_java() -> Option<std::path::PathBuf> {
    static JAVA: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    JAVA.get_or_init(|| find_tool("DEWASM_JAVA", "java"))
        .clone()
}

/// Locate a `javac` compiler.
/// Honors `$DEWASM_JAVAC`, then `javac` on `PATH`.
pub fn find_javac() -> Option<std::path::PathBuf> {
    static JAVAC: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    JAVAC
        .get_or_init(|| find_tool("DEWASM_JAVAC", "javac"))
        .clone()
}

/// The one way dewasm's suites run `javac`: the located compiler.
/// A missing compiler fails loud.
/// It is capped at C1 with the `UseSerialGC` collector and one processor.
/// That is because C2 cannot repay its compilation cost in a ~1 s run.
/// N machine-sized JVMs also destroy parallel scaling on a small CI runner.
/// The heap stays at the default: the slow category's `qjs`/DOOM sources need that room.
/// The `.class` output is byte-identical with and without these flags.
/// This was verified on the `cowsay` and `qjs` standalone sources.
pub fn javac_command() -> std::process::Command {
    let javac =
        find_javac().expect("javac not found on PATH (or $DEWASM_JAVAC): see docs/testing.md");
    let mut cmd = std::process::Command::new(javac);
    cmd.args([
        "-J-XX:TieredStopAtLevel=1",
        "-J-XX:+UseSerialGC",
        "-J-XX:ActiveProcessorCount=1",
    ]);
    cmd
}

/// The probe behind [`find_java`]/[`find_javac`].
/// Each probe spawns a JVM (~0.35 s for `javac -version`).
/// Both callers therefore memoize the answer for the process.
/// A test binary asks once per trial, and the toolchain cannot change under a running process.
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

/// A complete, compilable `.java` file bundling *every* runtime unit, plus a tiny `Main`.
/// The units lint's `javac` check compiles it to check that all units are valid Java.
/// That covers every unit, not just the subset any one module uses.
pub fn full_bundle_java() -> Result<String> {
    let bundle = bundler().bundle_all(0)?;
    let mut out = String::from("// Generated by dewasm. Do not edit.\n");
    out.push_str(&bundle);
    out.push_str("\n\npublic class Main {\n\tpublic static void main(String[] a) {}\n}\n");
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
            // Floats are native IEEE float/double; NaN paths mirror Ruby's numeric conventions.
            Feature::Floats => SupportStatus::Supported,
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
            // Tags are identity objects.
            // A thrown exception is a native Java exception that doubles as the `exnref`.
            // Traps stay uncatchable.
            Feature::ExceptionHandling => SupportStatus::Supported,
            // A trampoline with a body/entry split.
            // The body's result register holds the result or the thunk, so it is typed `Object`.
            // Multi-value results already use that shape.
            Feature::TailCall => SupportStatus::Supported,
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        check_module_support(&JavaBackend, module)?;
        let contents = generate_source(module, opts)?;
        let mut files = vec![OutputFile {
            name: "Main.java".to_string(),
            contents: contents.into_bytes(),
        }];
        // The data file: every segment's bytes concatenated in segment order.
        // The generated `Arrays.copyOfRange(DATA_BLOB, …)` slices rely on this order.
        // Their offsets are the `data_offsets` prefix sums written into the code.
        // Only emitted when there is data for a separate file (otherwise nothing reads it).
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

/// Emit just the module class, for the shared specification harness.
/// It holds the constructor, functions, and the harness's `invoke`/`globalGet` dispatch methods.
/// The harness bundles one runtime for every module in a `.wast` file.
/// So per-module output carries no runtime classes / `Main`.
/// Multi-value results, wasm-1.0 imports, and trapping conversions are all exercised here.
/// Returns the class source and the runtime units it references.
pub fn generate_program_with_units(
    module: &Module,
    type_name: &str,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&JavaBackend, module)?;
    // The specification harness's generation path never writes a data file: pass None.
    let gen = new_gen(module, type_name.to_string(), false, None);
    let mut body = CodeWriter::new("\t");
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

fn new_gen(
    module: &Module,
    type_name: String,
    default_wasi: bool,
    data_file: Option<String>,
) -> Gen<'_> {
    // Prefix sums: segment `i` begins at `data_offsets[i]` in the concatenated data-file blob.
    // Only consulted when data goes to a separate file.
    let mut data_offsets = Vec::with_capacity(module.datas.len());
    let mut acc = 0usize;
    for data in &module.datas {
        data_offsets.push(acc);
        acc += data.data.len();
    }
    Gen {
        module,
        default_wasi,
        type_name,
        tail_callers: module
            .funcs
            .iter()
            .enumerate()
            .filter(|(_, f)| stmts_use_tail_calls(&f.body))
            .map(|(i, _)| module.num_imported_funcs() + i as u32)
            .collect(),
        uses: RefCell::new(BTreeSet::new()),
        split: Cell::new(false),
        next_part: Cell::new(0),
        mv_counter: Cell::new(0),
        cur_base: RefCell::new(String::new()),
        cur_frame_ty: RefCell::new(String::new()),
        part_defs: RefCell::new(Vec::new()),
        costs: CostMemo::default(),
        elem_capture: Cell::new(false),
        partitioned: Cell::new(false),
        in_partition: Cell::new(false),
        data_file,
        data_offsets,
    }
}

fn generate_source(module: &Module, opts: &GenOptions) -> Result<String> {
    // Standalone output is a self-contained program.
    // Its module class is fixed and unqualified, not derived.
    // Library output uses the requested name unchanged.
    // A dotted name splits off a package declaration.
    let (package, type_name) = if opts.mode == Mode::Standalone {
        (None, STANDALONE_CLASS.to_string())
    } else {
        split_module_name(&opts.module_name)?
    };
    let gen = new_gen(
        module,
        type_name.clone(),
        opts.default_wasi,
        opts.data_file.as_ref().map(|c| c.data_file_name.clone()),
    );
    // A module whose function count crosses the threshold is split across nested `P{k}` classes.
    // Each class has its own constant pool.
    // Set before the constructor: its exports/start emit function calls through `defined_call`.
    // `defined_call` qualifies each call by partition.
    gen.partitioned
        .set(module.funcs.len() > FN_PARTITION_THRESHOLD);

    // Into its own writer: `uses` must be complete before the runtime bundle is assembled.
    let mut body = CodeWriter::new("\t");
    gen.constructor(&mut body);
    // Separate data blob: a static field loaded once from the data file next to this program.
    // The generated `Arrays.copyOfRange(DATA_BLOB, …)` calls slice it.
    // Only emitted when there is data for the file (otherwise the generated code never reads it).
    if let Some(data_file_name) = &gen.data_file {
        if !module.datas.is_empty() {
            body.line("");
            gen.emit_data_blob(&mut body, data_file_name);
        }
    }
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
    let wasi = wasi_bundled(module, opts.default_wasi, bundler());
    if standalone || wasi {
        // The public boundary (standalone main / library glue) catches these.
        gen.use_unit("rt/exit");
        gen.use_unit("rt/trap");
    }

    let uses = gen.uses.borrow().clone();
    // The runtime lives *inside* the module class.
    // Two artifacts in one package then own their runtime classes (trap type above all).
    // Otherwise one would silently win.
    // Nothing inside the class changes.
    // That is because Java resolves `Rt`/`Memory`/`Table`/`WASI` through the outer scope.
    let bundle = nested_bundler().bundle(&uses, 1)?;

    let mut out = String::from("// Generated by dewasm. Do not edit.\n");
    // The package declaration must precede every type in the compilation unit.
    if let Some(package) = &package {
        out.push_str(&format!("package {package};\n\n"));
    }
    out.push_str(&format!("final class {type_name} {{\n"));
    out.push_str(&bundle);
    out.push('\n');
    out.push_str(&reindent(&body.finish(), 1));
    out.push_str("}\n");
    if standalone {
        out.push('\n');
        out.push_str(&main_class(&type_name, wasi));
    }
    Ok(out)
}

/// Add `levels` of indentation to a block of source, leaving blank lines empty.
fn reindent(src: &str, levels: usize) -> String {
    let pad = "\t".repeat(levels);
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

/// The standalone entry point, in four steps:
/// - Parse the runtime interface: `--dir HOST::GUEST` preopens, then the guest `argv`.
///   Its `argv[0]` is the program name.
/// - Instantiate.
/// - Run `_start` on a dedicated large-stack thread.
/// - Map `proc_exit`/trap to a process exit code.
///   A trap prints to `stderr` and exits 134, mirroring Ruby/Python/Go.
///
/// The dedicated thread mirrors how Python handles deep-but-valid guest recursion (issue #137).
/// Such recursion can exceed the JVM's default main-thread stack on some hosts.
/// So `_start` runs on a 64 MiB thread instead.
/// Exceptions thrown on that thread do not cross `Thread.join()`.
/// So the runnable catches everything and hands the result back through `failure`.
fn main_class(type_name: &str, wasi: bool) -> String {
    let args = if wasi { "wasiArgs" } else { "null" };
    let env = if wasi { "wasiEnv" } else { "new String[0]" };
    let preopens = if wasi { "preopens" } else { "null" };
    // Standalone WASI parses a leading run of `--dir HOST::GUEST` flags (as Wasmtime does).
    // It stops at `--` or the first non-flag token; the rest is the guest's `argv[1..]`.
    // The JVM does not pass the launched file name to `main`.
    // So `argv[0]` is the module class name.
    // The whole process environment passes through.
    // Without WASI there is nothing to preopen and no `argv` to deliver.
    let arg_setup = if wasi {
        format!(
            "\t\tjava.util.Map<String, String> preopens = new java.util.HashMap<>();\n\
             {ind}int i = 0;\n\
             {ind}while (i < argv.length) {{\n\
             {ind}\tString a = argv[i];\n\
             {ind}\tString spec;\n\
             {ind}\tif (a.equals(\"--\")) {{\n\
             {ind}\t\ti++;\n\
             {ind}\t\tbreak;\n\
             {ind}\t}} else if (a.equals(\"--dir\")) {{\n\
             {ind}\t\tif (i + 1 >= argv.length) {{\n\
             {ind}\t\t\tSystem.err.print(\"--dir requires a HOST::GUEST argument\\n\");\n\
             {ind}\t\t\tSystem.exit(1);\n\
             {ind}\t\t}}\n\
             {ind}\t\tspec = argv[i + 1];\n\
             {ind}\t\ti += 2;\n\
             {ind}\t}} else if (a.startsWith(\"--dir=\")) {{\n\
             {ind}\t\tspec = a.substring(6);\n\
             {ind}\t\ti++;\n\
             {ind}\t}} else {{\n\
             {ind}\t\tbreak;\n\
             {ind}\t}}\n\
             {ind}\tint sep = spec.indexOf(\"::\");\n\
             {ind}\tif (sep >= 0) {{\n\
             {ind}\t\tpreopens.put(spec.substring(sep + 2), spec.substring(0, sep));\n\
             {ind}\t}} else {{\n\
             {ind}\t\tpreopens.put(spec, spec);\n\
             {ind}\t}}\n\
             {ind}}}\n\
             {ind}String[] wasiArgs = new String[argv.length - i + 1];\n\
             {ind}wasiArgs[0] = {name};\n\
             {ind}System.arraycopy(argv, i, wasiArgs, 1, argv.length - i);\n\
             {ind}java.util.Map<String, String> envMap = System.getenv();\n\
             {ind}String[] wasiEnv = new String[envMap.size()];\n\
             {ind}int ei = 0;\n\
             {ind}for (java.util.Map.Entry<String, String> e : envMap.entrySet()) {{\n\
             {ind}\twasiEnv[ei++] = e.getKey() + \"=\" + e.getValue();\n\
             {ind}}}\n",
            ind = "\t\t",
            name = java_string(type_name),
        )
    } else {
        String::new()
    };
    let mut out = String::new();
    out.push_str("public class Main {\n");
    out.push_str("\tpublic static void main(String[] argv) {\n");
    out.push_str(&arg_setup);
    out.push_str(&format!(
        "\t\t{type_name} p = new {type_name}(null, {args}, {env}, {preopens});\n"
    ));
    out.push_str("\t\tThrowable[] failure = new Throwable[1];\n");
    out.push_str("\t\tRunnable guestRun = () -> {\n");
    out.push_str("\t\t\ttry {\n");
    out.push_str(&format!(
        "\t\t\t\t(({type_name}.Rt.Fn) p.Exports.get(\"_start\")).invoke(new Object[]{{}});\n"
    ));
    out.push_str("\t\t\t} catch (Throwable e) {\n");
    out.push_str("\t\t\t\tfailure[0] = e;\n");
    out.push_str("\t\t\t}\n");
    out.push_str("\t\t};\n");
    out.push_str("\t\tThread guest = new Thread(null, guestRun, \"guest\", 64L << 20);\n");
    out.push_str("\t\tguest.start();\n");
    out.push_str("\t\ttry {\n");
    out.push_str("\t\t\tguest.join();\n");
    out.push_str("\t\t} catch (InterruptedException e) {\n");
    out.push_str("\t\t\tThread.currentThread().interrupt();\n");
    out.push_str("\t\t}\n");
    out.push_str(&format!(
        "\t\tif (failure[0] instanceof {type_name}.Rt.Exit) {{\n"
    ));
    out.push_str(&format!(
        "\t\t\tSystem.exit((({type_name}.Rt.Exit) failure[0]).code);\n"
    ));
    out.push_str(&format!(
        "\t\t}} else if (failure[0] instanceof {type_name}.Rt.Trap) {{\n"
    ));
    out.push_str("\t\t\tSystem.err.print(\"trap: \" + failure[0].getMessage() + \"\\n\");\n");
    out.push_str("\t\t\tSystem.err.flush();\n");
    out.push_str("\t\t\tSystem.exit(134);\n");
    out.push_str("\t\t} else if (failure[0] instanceof RuntimeException) {\n");
    out.push_str("\t\t\tthrow (RuntimeException) failure[0];\n");
    out.push_str("\t\t} else if (failure[0] instanceof Error) {\n");
    out.push_str("\t\t\tthrow (Error) failure[0];\n");
    out.push_str("\t\t}\n");
    out.push_str("\t\tSystem.exit(0);\n");
    out.push_str("\t}\n");
    out.push_str("}\n");
    out
}

/// The module class a `--mode standalone` program defines.
/// It is fixed, since nothing outside a self-contained program observes it.
/// It is also the standalone `argv[0]` the JVM cannot supply (see `docs/standalone-interface.md`).
pub const STANDALONE_CLASS: &str = "Program";

/// Split a validated library-mode module name into `(package, class)`.
/// The last dot-separated segment is the class name, used unchanged.
/// The leading segments, if any, are the `package` declaration.
/// A name like `com.github.dewasm.Sqlite3` gives a conventional package and class name.
/// The grammar is character-level only.
/// A segment that is a Java keyword (`int`, `package`, ...) passes here.
/// It fails in `javac` with the compiler's own message.
/// A maintained keyword list is not worth its cost.
fn split_module_name(name: &str) -> Result<(Option<String>, String)> {
    let segs: Vec<&str> = name.split('.').collect();
    let ok = segs.iter().all(|seg| {
        is_ident(
            seg,
            |c| c.is_ascii_alphabetic() || c == '_' || c == '$',
            |c| c.is_ascii_alphanumeric() || c == '_' || c == '$',
        )
    });
    if !ok {
        return Err(module_name_error(
            "java",
            name,
            "a class name optionally qualified by a package: `.`-separated segments each matching \
             [A-Za-z_$][A-Za-z0-9_$]*, the last one being the class \
             (e.g. Add, com.github.dewasm.Sqlite3)",
        ));
    }
    let (class, package) = segs.split_last().expect("split never yields an empty vec");
    let package = (!package.is_empty()).then(|| package.join("."));
    Ok((package, (*class).to_string()))
}

/// The Java rendering of a wasm comparison.
/// It is the operator, and the unsigned-comparison helper its operands go through.
/// The helper is `None` where Java's operator on the stored representation already matches wasm.
/// Integers are stored signed, so the signed forms are direct.
/// Java's float comparison agrees with wasm, NaN included.
/// `bin` wraps the result back into an i32, `cond` takes it as it stands.
fn rel_op(op: BinOp) -> Option<(&'static str, Option<&'static str>)> {
    let (r, operands) = comparison(op)?;
    Some((
        r,
        match operands {
            CompareOperands::Unsigned32 => Some("Integer.compareUnsigned"),
            CompareOperands::Unsigned64 => Some("Long.compareUnsigned"),
            _ => None,
        },
    ))
}

/// A comparison as a Java `boolean`, from a `rel_op` mapping and the already rendered operands.
fn rel((r, cmp): (&str, Option<&str>), a: &str, b: &str) -> String {
    match cmp {
        None => format!("({a}) {r} ({b})"),
        Some(cmp) => format!("{cmp}({a}, {b}) {r} 0"),
    }
}

/// The name suffix of a tail-entry interface, field and table.
/// It distinguishes them by the result signature they carry.
fn tail_suffix(results: &[ValType]) -> String {
    match results {
        [] => "V".to_string(),
        [t] => jtype_suffix(*t).to_uppercase(),
        _ => "A".to_string(),
    }
}

/// The field-name suffix distinguishing a parked argument slot's type.
fn jtype_suffix(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "i",
        ValType::I64 => "l",
        ValType::F32 => "f",
        ValType::F64 => "d",
        ValType::FuncRef => "r",
        ValType::ExnRef => "e",
    }
}

fn jtype(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "int",
        ValType::I64 => "long",
        ValType::F32 => "float",
        ValType::F64 => "double",
        ValType::FuncRef => "Rt.Funcref",
        // A caught exception is its own `exnref` value.
        // So the reference type is the exception class itself.
        // A null `exnref` is Java's `null`.
        ValType::ExnRef => "Rt.WasmException",
    }
}

fn zero_value(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "0",
        ValType::I64 => "0L",
        ValType::F32 => "0.0f",
        ValType::F64 => "0.0",
        ValType::FuncRef | ValType::ExnRef => "null",
    }
}

fn ty_suffix(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
        ValType::FuncRef => "fr",
        ValType::ExnRef => "exn",
    }
}

fn temp_name(t: Temp) -> String {
    format!("s{}_{}", t.depth, ty_suffix(t.ty))
}

/// A Java string literal.
pub fn java_string(s: &str) -> String {
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
    /// Defined functions (function index space) containing a tail call.
    /// Each is split into `f{idx}Body` plus a trampoline entry.
    tail_callers: BTreeSet<u32>,
    module: &'a Module,
    default_wasi: bool,
    type_name: String,
    uses: RefCell<BTreeSet<String>>,
    /// Whether the function currently being emitted is split into part methods.
    split: Cell<bool>,
    /// Part-method counter for the current function.
    next_part: Cell<usize>,
    /// Monotonic counter for the temps (`__mvN`) that unpack a multi-value result.
    mv_counter: Cell<usize>,
    /// Base name (`f47`) and frame type (`Frame47`) of the current function.
    cur_base: RefCell<String>,
    cur_frame_ty: RefCell<String>,
    /// Part-method definitions produced while emitting the current function.
    /// They are flushed after its entry method.
    part_defs: RefCell<Vec<String>>,
    /// Memoized body costs for the current function, driving the split decision (see `CostMemo`).
    costs: CostMemo,
    /// When true, a function value (`func_value`) captures the module instance as `inst.`.
    /// Otherwise it references `this` implicitly.
    /// Set while emitting the nested `Elem` helper class's static methods.
    /// Their `funcref` lambdas live in a separate constant pool.
    elem_capture: Cell<bool>,
    /// Whether this module's functions are split across nested `P{k}` classes.
    /// They are when its function count crosses `FN_PARTITION_THRESHOLD`.
    /// When set, defined functions are `static` methods taking the module instance.
    /// Calls to them are class-qualified.
    partitioned: Cell<bool>,
    /// True while emitting a function body inside a `P{k}` partition class.
    /// Instance references then resolve through the passed `inst` parameter.
    in_partition: Cell<bool>,
    /// When `Some`, data segments go into a separate binary data file of this filename.
    /// They are then not embedded as chunked Base64.
    /// The file is loaded once into the static `DATA_BLOB`.
    /// `data_offsets[i]` locates segment `i` in the blob.
    data_file: Option<String>,
    data_offsets: Vec<usize>,
}

impl<'a> Gen<'a> {
    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    /// The Java expression yielding a data segment's bytes.
    /// With `--data-file` on, it copies a slice of `DATA_BLOB`, loaded from the data file.
    /// Otherwise it is the inline chunked-Base64 decode.
    /// Both yield a fresh `byte[]`, so the segment field stays independently mutable.
    /// `data.drop` sets that field empty.
    fn data_expr(&self, seg: usize, data: &[u8]) -> String {
        if self.data_file.is_some() {
            let o = self.data_offsets[seg];
            format!(
                "java.util.Arrays.copyOfRange(DATA_BLOB, {o}, {})",
                o + data.len()
            )
        } else {
            self.use_unit("rt/data_from_b64");
            data_blob(data)
        }
    }

    /// Emit the static `DATA_BLOB` field and its loader.
    /// The data file is resolved relative to this program's own code source.
    /// That is the JAR's directory (regular file) or the class directory itself.
    /// So `java -cp <dir> Main` finds the data file alongside the class.
    /// Only called when a data file is in use and the module has data.
    fn emit_data_blob(&self, w: &mut CodeWriter, data_file_name: &str) {
        w.line("static final byte[] DATA_BLOB = loadDataBlob();");
        w.line("");
        w.line("private static byte[] loadDataBlob() {");
        w.indent();
        w.line("try {");
        w.indent();
        w.line(format!(
            "java.net.URI __uri = {}.class.getProtectionDomain().getCodeSource().getLocation().toURI();",
            self.type_name
        ));
        w.line("java.nio.file.Path __p = java.nio.file.Paths.get(__uri);");
        w.line(
            "java.nio.file.Path __dir = java.nio.file.Files.isRegularFile(__p) ? __p.getParent() : __p;",
        );
        w.line(format!(
            "return java.nio.file.Files.readAllBytes(__dir.resolve({}));",
            java_string(data_file_name)
        ));
        w.dedent();
        w.line("} catch (Exception __e) {");
        w.indent();
        w.line("throw new RuntimeException(\"failed to load data file\", __e);");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    }

    fn rt(&self, name: &str) -> String {
        self.use_unit(&format!("rt/{name}"));
        format!("Rt.{name}")
    }

    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
    }

    /// Whether emitted code reaches the module's instance fields through a passed `inst` parameter.
    /// That is so in a `P{k}` partition-class function body or the `Elem` helper class.
    /// Elsewhere the code uses an implicit `this`.
    fn via_inst(&self) -> bool {
        self.in_partition.get() || self.elem_capture.get()
    }

    /// Reference an instance field (`memory`, `g3`, `t0`, `if5`, `data2`, `elem0`).
    /// It is `inst.<name>` when reached via a passed instance.
    /// Otherwise it is the plain name (implicit `this`).
    fn iref(&self, name: &str) -> String {
        if self.via_inst() {
            format!("inst.{name}")
        } else {
            name.to_string()
        }
    }

    /// The instance to pass as the first argument of a partitioned static function call.
    /// It is `inst` inside a `P{k}`/`Elem` method, else `this`.
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

    /// A function/part method head.
    /// In a partitioned module, the method is `static`.
    /// It takes the module instance as its first parameter.
    /// It can then live in a `P{k}` class with its own constant pool.
    /// Otherwise it is a plain instance method.
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

    /// A call to a *defined* function by index, honoring partitioning.
    /// A partitioned module calls `P{k}.f{idx}(<inst>, args)`.
    /// Otherwise the call is `[inst.]f{idx}(args)`.
    /// The `inst.` form serves the non-partitioned `Elem` class.
    /// `args_joined` is the already-formatted argument list.
    fn defined_call(&self, func_idx: u32, args_joined: &str) -> String {
        self.defined_call_named(func_idx, &format!("f{func_idx}"), args_joined)
    }

    /// The same call, naming the method explicitly.
    /// A tail-calling function is reached through its entry (`fN`) or its split body (`fNBody`).
    /// Both live in the same partition class.
    fn defined_call_named(&self, func_idx: u32, method: &str, args_joined: &str) -> String {
        if self.partitioned.get() {
            let k = self.partition_of(func_idx);
            let s = self.self_arg();
            if args_joined.is_empty() {
                format!("P{k}.{method}({s})")
            } else {
                format!("P{k}.{method}({s}, {args_joined})")
            }
        } else if self.via_inst() {
            format!("inst.{method}({args_joined})")
        } else {
            format!("{method}({args_joined})")
        }
    }

    /// A slot reference.
    /// It is a frame field when the function is split across `part` methods, else a plain local.
    /// Same for [`Self::temp_ref`], [`Self::br`] and [`Self::ret`].
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
        self.push_part_with(name, "", body);
    }

    /// `push_part` with extra parameters appended to the head.
    /// The outlined `br_table` case ranges use it, since they take the table index.
    fn push_part_with(&self, name: &str, extra_params: &str, body: String) {
        let frame = self.cur_frame_ty.borrow().clone();
        // A partitioned module's parts are static and take the module instance with the frame.
        // Instance references then resolve through `inst`.
        let head = if self.partitioned.get() {
            format!(
                "static void {name}({} inst, {frame} f{extra_params}) {{\n",
                self.type_name
            )
        } else {
            format!("private void {name}({frame} f{extra_params}) {{\n")
        };
        let mut out = head;
        out.push_str(&reindent(&body, 1));
        out.push_str("}\n");
        self.part_defs.borrow_mut().push(out);
    }

    fn constructor(&self, w: &mut CodeWriter) {
        let m = self.module;
        let name = &self.type_name;
        self.struct_fields(w);
        w.line("");
        w.line(format!(
            "{name}(java.util.Map<String, ?> imports, String[] args, String[] env, java.util.Map<String, String> preopens) {{"
        ));
        w.indent();

        // Built once, before anything that parks a target or stores one in a table.
        // A tail call reads its target out of here rather than building a lambda per hop.
        // Each entry is bound to this instance and reads *its* parked slots.
        // That is what makes the owner check at an indirect tail call necessary.
        if !self.tail_callers.is_empty() {
            for sig in self.tail_signatures() {
                let entries: Vec<String> = self
                    .tail_callers
                    .iter()
                    .filter(|f| self.module.func_type(**f).results == sig)
                    .map(|f| {
                        let params = &self.module.func_type(*f).params;
                        let args: Vec<String> = params
                            .iter()
                            .enumerate()
                            .map(|(i, t)| Self::arg_slot(i, *t))
                            .collect();
                        format!("() -> f{f}Body({})", args.join(", "))
                    })
                    .collect();
                w.line(format!(
                    "this.{} = new {}[]{{{}}};",
                    self.tail_table(&sig),
                    self.tail_iface(&sig),
                    entries.join(", ")
                ));
            }
        }

        // Memory: imported or locally defined (index space has at most one).
        if let Some(import) = &m.imported_memory {
            self.emit_typed_import(w, "this.memory", "Memory", &import.module, &import.name);
        } else if let Some(mem) = &m.memory {
            self.use_unit("memory/_class");
            let max = mem.max_pages.map(|p| p as u32).unwrap_or(65536);
            w.line(format!(
                "this.memory = new Memory({}, {});",
                mem.min_pages as u32, max
            ));
        }

        // Tables: imported first, then defined (index space is `imported_tables` ++ `tables`).
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

        let wasi = wasi_bundled(m, self.default_wasi, bundler());
        if wasi {
            self.use_unit("wasi/_class");
            // Kept for `wasiInstance()`.
            // That method builds the bundled WASI the first time an import falls back to it.
            // An embedder covering every WASI import never pays for one.
            w.line("this.wasiArgs = args;");
            w.line("this.wasiEnv = env;");
            w.line("this.wasiPreopens = preopens;");
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            self.emit_import(w, i, import);
        }

        // Globals: imported first, then defined; every global is a boxed `Global`.
        // Defined-global initializers may read imported globals.
        // So they resolve after the imported ones.
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

        // Tags: imported first, then defined (index space is `imported_tags` ++ `tags`).
        // A defined tag is a fresh identity object.
        // An imported one must be the very object its origin defined.
        // That is what makes `catch` match across instances.
        for (i, import) in m.imported_tags.iter().enumerate() {
            self.emit_typed_import(
                w,
                &format!("this.tag{i}"),
                "Rt.Tag",
                &import.module,
                &import.name,
            );
        }
        for i in 0..m.tags.len() {
            w.line(format!(
                "this.tag{} = new Rt.Tag();",
                m.imported_tags.len() + i
            ));
        }

        // Element segments.
        // A segment over ELEM_SPLIT is built by the nested `Elem` helper class, else inline.
        // That class has its own constant pool and chunked part methods.
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

        // Each data segment is materialized in its own `initData{i}()` method.
        // It is not inline in the constructor.
        // A multi-MB segment lowers to a chunked-Base64 array.
        // Its initializer, plus the `memory.init` copy, would otherwise accumulate in `<init>`.
        // `<init>` is bound by the 64KB method limit.
        // One method per segment bounds the constructor, however large or many the segments are.
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
                ExportKind::Tag(idx) => format!("tag{idx}"),
            };
            w.line(format!(
                "this.Exports.put({}, {val});",
                java_string(&export.name)
            ));
        }

        // Let import providers bind to the constructed instance.
        let has_imports = !m.imported_funcs.is_empty()
            || !m.imported_globals.is_empty()
            || !m.imported_tables.is_empty()
            || !m.imported_tags.is_empty()
            || m.imported_memory.is_some();
        if has_imports {
            w.line("if (imports != null) {");
            w.indent();
            w.line("for (Object __p : imports.values()) {");
            w.indent();
            w.line("if (__p instanceof Rt.ImportProvider) {");
            w.indent();
            w.line("((Rt.ImportProvider) __p).attach(this);");
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
        }

        if let Some(start) = m.start {
            w.line(format!("{};", self.call_string(start, &[])));
        }

        w.dedent();
        w.line("}");

        if wasi {
            w.line("");
            self.wasi_accessor(w);
        }

        // Per-segment data initializers, called in order from the constructor (see above).
        // Each is a self-contained method.
        // So a segment of any size never pushes `<init>` past the 64KB method limit.
        for (i, data) in m.datas.iter().enumerate() {
            let blob = self.data_expr(i, &data.data);
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
                    // Active segments are dropped after instantiation.
                    // They stay addressable (as empty) by memory.init/data.drop.
                    w.line(format!("this.data{i} = new byte[0];"));
                }
                None => {
                    w.line(format!("this.data{i} = {blob};"));
                }
            }
            w.dedent();
            w.line("}");
        }

        // The nested `Elem` helper class builds any `funcref` table that is too large.
        // See the element-segment loop above.
        // A separate class has its own 65535-entry constant pool.
        // Its thousands of `funcref` lambdas land there instead of in the module class's pool.
        // Chunked `elem{i}_pK` part methods fill each table to stay under the 64KB method limit.
        if !split_elems.is_empty() {
            self.emit_elem_class(w, &split_elems);
        }
    }

    /// The bundled WASI, built on first use.
    /// Nothing constructs it in the constructor.
    /// An embedder whose provider covers every WASI import gets no WASI at all.
    /// That is what `wasi == null` says.
    /// The first import that falls back builds it, with the memory already bound.
    /// The constructor resolves memory before any import.
    fn wasi_accessor(&self, w: &mut CodeWriter) {
        let m = self.module;
        w.line("WASI wasiInstance() {");
        w.indent();
        w.line("if (this.wasi == null) {");
        w.indent();
        w.line("this.wasi = new WASI(this.wasiArgs, this.wasiEnv, this.wasiPreopens);");
        if m.memory.is_some() || m.imported_memory.is_some() {
            w.line("this.wasi.memory = this.memory;");
        }
        w.dedent();
        w.line("}");
        w.line("return this.wasi;");
        w.dedent();
        w.line("}");
    }

    /// Emit the nested `Elem` helper class for the too-large element segments in `split_elems`.
    /// Each segment `i` gets an `elem{i}(inst)` factory that allocates the array.
    /// The factory calls chunked `elem{i}_pK(inst, a)` fillers.
    /// The fillers live in `ElemF{c}` classes of at most `ELEM_PER_CLASS` entries each.
    /// So no single pool holds more `funcref` lambdas than it can address.
    fn emit_elem_class(&self, w: &mut CodeWriter, split_elems: &[usize]) {
        self.elem_capture.set(true);
        let ty = &self.type_name;
        // Assign each `elem{i}_p{p}` filler to a filler class, packing them in order.
        let mut filler_class: HashMap<(usize, usize), usize> = HashMap::new();
        let mut cls = 0usize;
        let mut in_cls = 0usize;
        for &i in split_elems {
            let len = self.module.elems[i].items.len();
            for p in 0..len.div_ceil(ELEM_PART) {
                if in_cls > 0 && in_cls + ELEM_PART > ELEM_PER_CLASS {
                    cls += 1;
                    in_cls = 0;
                }
                filler_class.insert((i, p), cls);
                in_cls += ELEM_PART;
            }
        }

        w.line("");
        w.line("static final class Elem {");
        w.indent();
        for (n, &i) in split_elems.iter().enumerate() {
            if n > 0 {
                w.line("");
            }
            let len = self.module.elems[i].items.len();
            w.line(format!("static Rt.Funcref[] elem{i}({ty} inst) {{"));
            w.indent();
            w.line(format!("Rt.Funcref[] a = new Rt.Funcref[{len}];"));
            for p in 0..len.div_ceil(ELEM_PART) {
                w.line(format!(
                    "ElemF{}.elem{i}_p{p}(inst, a);",
                    filler_class[&(i, p)]
                ));
            }
            w.line("return a;");
            w.dedent();
            w.line("}");
        }
        w.dedent();
        w.line("}");

        for c in 0..=cls {
            w.line("");
            w.line(format!("static final class ElemF{c} {{"));
            w.indent();
            let mut first = true;
            for &i in split_elems {
                let elem = &self.module.elems[i];
                let len = elem.items.len();
                for p in 0..len.div_ceil(ELEM_PART) {
                    if filler_class[&(i, p)] != c {
                        continue;
                    }
                    if !first {
                        w.line("");
                    }
                    first = false;
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
        }
        self.elem_capture.set(false);
    }

    /// Emit the module's defined functions grouped into nested `P{k}` classes.
    /// Each class holds up to `FN_PER_PARTITION` functions.
    /// So no single class's constant pool overflows Java's 65535-entry limit.
    /// The functions are `static` and take the module instance.
    /// Call sites reach them class-qualified via `defined_call`.
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
        // Parked tail calls need three members per result signature.
        // Those are one entry interface, one pending target and one entry table.
        // There is also one argument slot per position and type a tail call passes.
        // The interface carries the signature's own Java type.
        // So a chain ends without boxing its result.
        if !self.tail_callers.is_empty() {
            for sig in self.tail_signatures() {
                w.line(format!(
                    "interface {} {{ {} run(); }}",
                    self.tail_iface(&sig),
                    ret_slot_ty(&sig)
                ));
            }
            for (name, ty) in self.arg_slots() {
                w.line(format!("{} {name};", jtype(ty)));
            }
            for sig in self.tail_signatures() {
                let iface = self.tail_iface(&sig);
                w.line(format!("{iface} {};", self.tail_field(&sig)));
                w.line(format!("{iface}[] {};", self.tail_table(&sig)));
            }
        }
        let m = self.module;
        if m.imported_memory.is_some() || m.memory.is_some() {
            w.line("Memory memory;");
        }
        // Table index space = `imported_tables` ++ `tables`.
        for i in 0..(m.imported_tables.len() + m.tables.len()) {
            w.line(format!("Table t{i};"));
        }
        // Global index space = `imported_globals` ++ `globals`; each is a boxed `Global`.
        for i in 0..(m.imported_globals.len() + m.globals.len()) {
            w.line(format!("Global g{i};"));
        }
        for i in 0..m.imported_funcs.len() {
            w.line(format!("Rt.Fn if{i};"));
        }
        // Tag index space = `imported_tags` ++ `tags`.
        if !m.imported_tags.is_empty() || !m.tags.is_empty() {
            self.use_unit("rt/tag");
        }
        for i in 0..(m.imported_tags.len() + m.tags.len()) {
            w.line(format!("Rt.Tag tag{i};"));
        }
        if wasi_bundled(m, self.default_wasi, bundler()) {
            // The bundled WASI is built on first fallback, not in the constructor.
            // So the constructor arguments are kept for `wasiInstance()` to use.
            w.line("WASI wasi;");
            w.line("String[] wasiArgs;");
            w.line("String[] wasiEnv;");
            w.line("java.util.Map<String, String> wasiPreopens;");
        }
        // Element segments retained for table.init (active ones are emptied after instantiation).
        for i in 0..m.elems.len() {
            w.line(format!("Rt.Funcref[] elem{i};"));
        }
        // Every data segment is addressable by memory.init/data.drop, so all get a field.
        // Active ones become empty after instantiation.
        for i in 0..m.datas.len() {
            w.line(format!("byte[] data{i};"));
        }
        w.line("java.util.Map<String, Object> Exports;");
    }

    /// Resolve a non-function import (memory/table/global) into `target`.
    /// Its *kind* is checked via `instanceof`.
    /// A wrong-kind or missing value is a link error.
    /// The finer wasm type is not checked.
    /// That covers a global's value type and mutability, and a table/memory's limits.
    /// This is the import-limits gap.
    /// It is wider for Java than Go, since these values carry no static type.
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

        let mut needs_wasi = false;
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
                // The adapter closes over the WASI the fallback just built.
                // So the bundled WASI exists exactly when some import fell back to it.
                // This mirrors Ruby's `@wasi ||=`, rather than building on the first *call*.
                needs_wasi = true;
                Some(format!("__a -> __w.wasi_{}({call_args})", import.name))
            } else {
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
            Some(f) => {
                if needs_wasi {
                    w.line("WASI __w = this.wasiInstance();");
                }
                w.line(format!("this.if{i} = {f};"));
            }
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
    /// Inside the nested `Elem` class, functions are reached through the passed module instance.
    /// The reference then carries the `inst.` prefix.
    /// So the lambdas can live in that class's own constant pool.
    /// The instance a slot's tail entry is compared against.
    /// It is `inst` inside a partition class, `this` otherwise.
    fn self_ref(&self) -> &'static str {
        if self.partitioned.get() || self.via_inst() {
            "inst"
        } else {
            "this"
        }
    }

    fn next_tail_name(&self) -> String {
        let n = self.mv_counter.get();
        self.mv_counter.set(n + 1);
        format!("__ts{n}")
    }

    /// The slot a tail call parks argument `i` of type `ty` in.
    fn arg_slot(i: usize, ty: ValType) -> String {
        format!("ta{i}{}", jtype_suffix(ty))
    }

    /// Every argument slot the module's tail calls need.
    /// There is one per position and type a tail call passes.
    /// The slots cover a tail-calling function's own parameters.
    /// The reason is that its tail entry reads them back out.
    /// They also cover every signature reachable through a table.
    /// The reason is that an indirect tail call parks against the call site's type.
    fn arg_slots(&self) -> Vec<(String, ValType)> {
        let mut seen: BTreeSet<(usize, ValType)> = BTreeSet::new();
        let mut note = |params: &[ValType]| {
            for (i, ty) in params.iter().enumerate() {
                seen.insert((i, *ty));
            }
        };
        for idx in &self.tail_callers {
            note(&self.module.func_type(*idx).params);
        }
        for f in &self.module.funcs {
            Stmt::any(&f.body, &mut |st| {
                if let Stmt::ReturnCallIndirect { type_idx, .. } = st {
                    note(&self.module.types[*type_idx as usize].params);
                }
                false
            });
        }
        seen.into_iter()
            .map(|(i, ty)| (Self::arg_slot(i, ty), ty))
            .collect()
    }

    /// The distinct result signatures of the module's tail-calling functions.
    /// Each needs its own entry interface, field and table.
    fn tail_signatures(&self) -> Vec<Vec<ValType>> {
        let mut seen: BTreeSet<Vec<ValType>> = BTreeSet::new();
        for idx in &self.tail_callers {
            seen.insert(self.module.func_type(*idx).results.clone());
        }
        seen.into_iter().collect()
    }

    /// The entry interface for a result signature: a call returning that signature's own Java type.
    /// So a chain ends without boxing.
    fn tail_iface(&self, results: &[ValType]) -> String {
        format!("TailBody{}", tail_suffix(results))
    }

    fn tail_field(&self, results: &[ValType]) -> String {
        format!("tf{}", tail_suffix(results))
    }

    fn tail_table(&self, results: &[ValType]) -> String {
        format!("tb{}", tail_suffix(results))
    }

    /// A tail-calling function's dense position in its result signature's table.
    fn tail_slot(&self, func_idx: u32) -> Option<usize> {
        if !self.tail_callers.contains(&func_idx) {
            return None;
        }
        let results = self.module.func_type(func_idx).results.clone();
        self.tail_callers
            .iter()
            .filter(|f| self.module.func_type(**f).results == results)
            .position(|f| *f == func_idx)
    }

    /// Bind each argument to a final local, so a lambda over them compiles.
    /// Java captures variables, and only effectively-final ones.
    fn bind_finals(&self, w: &mut CodeWriter, params: &[ValType], args: &[String]) -> Vec<String> {
        let mut names = Vec::with_capacity(args.len());
        for (a, ty) in args.iter().zip(params) {
            let n = self.next_tail_name();
            w.line(format!("final {} {n} = {a};", jtype(*ty)));
            names.push(n);
        }
        names
    }

    /// Park a call that has no tail entry to reuse, as a lambda.
    /// It still runs after this frame is gone, which is what a tail call means.
    fn park_lambda(&self, w: &mut CodeWriter, results: &[ValType], call: &str) {
        let run = if results.is_empty() {
            format!("{{ {call}; }}")
        } else {
            call.to_string()
        };
        w.line(format!(
            "{} = () -> {run};",
            self.iref(&self.tail_field(results))
        ));
        w.line(format!("{} = -1;", self.br()));
    }

    /// Park a tail call and end this frame.
    /// The arguments go into their slots, then the target, then the branch register.
    /// The branch register unwinds the frame the way a return does.
    /// The arguments are already free of calls.
    /// The IR spills an operand with side effects before the instruction.
    /// So nothing between these assignments can reach another trampoline and overwrite a slot.
    fn park_tail(
        &self,
        w: &mut CodeWriter,
        params: &[ValType],
        results: &[ValType],
        args: &[String],
        target: &str,
    ) {
        for (i, (a, ty)) in args.iter().zip(params).enumerate() {
            w.line(format!("{} = {a};", self.iref(&Self::arg_slot(i, *ty))));
        }
        w.line(format!(
            "{} = {target};",
            self.iref(&self.tail_field(results))
        ));
        w.line(format!("{} = -1;", self.br()));
    }

    fn func_value(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            return format!("(Rt.Fn) {}", self.iref(&format!("if{func_idx}")));
        }
        let ty = self.module.func_type(func_idx);
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
                let body = match self.tail_slot(*func_idx) {
                    Some(k) => format!(
                        ", {}[{k}], this",
                        self.iref(&self.tail_table(&self.module.func_type(*func_idx).results))
                    ),
                    None => String::new(),
                };
                format!(
                    "new Rt.Funcref({}, {}{body})",
                    java_string(&self.func_type_symbol(*func_idx)),
                    self.func_value(*func_idx)
                )
            }
            ElemItem::Null => "null".to_string(),
            // Element items reading a global of reference type imply reference types.
            // Conversion rejects those.
            // This arm is unreachable, but must type-check as a Funcref.
            ElemItem::Global(idx) => {
                format!("(Rt.Funcref) {}.value", self.iref(&format!("g{idx}")))
            }
        }
    }

    fn func_type_symbol(&self, func_idx: u32) -> String {
        type_key(self.module.func_type(func_idx), ty_suffix)
    }

    /// A structural function-type key ([`type_key`]) spelled with this backend's own [`ty_suffix`].
    /// A table is only ever shared between artifacts of one backend.
    /// So the spelling only has to be self-consistent here.
    fn type_symbol(&self, type_idx: u32) -> String {
        type_key(&self.module.types[type_idx as usize], ty_suffix)
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &Func) {
        let ty = &self.module.types[func.type_idx as usize];
        let nparams = ty.params.len();
        let results = &ty.results;
        // A tail-calling function's real code lives in `f{idx}Body`.
        // The public `f{idx}` is the trampoline that runs the chain, so no call site changes.
        // The body's result register keeps the function's own type.
        // A parked call carries the continuation.
        // So the register never has to hold anything but a result.
        let is_tail_caller = self.tail_callers.contains(&idx);
        let has_result = !results.is_empty();
        // A method returns one value; multi-value signatures return a boxed `Object[]`.
        // The result register (`_ret` / frame `ret`) takes the same shape.
        let ret_ty = ret_slot_ty(results);
        let ret_init = ret_slot_init(results);
        if is_tail_caller {
            let params_str = (0..nparams)
                .map(|i| {
                    format!(
                        "{} l{i}",
                        jtype(if i < ty.params.len() {
                            ty.params[i]
                        } else {
                            func.locals[i - ty.params.len()]
                        })
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            let args = (0..nparams)
                .map(|i| format!("l{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            w.line(self.method_head(&ret_slot_ty(results), &format!("f{idx}"), &params_str));
            w.indent();
            let field = self.iref(&self.tail_field(results));
            let call = self.defined_call_named(idx, &format!("f{idx}Body"), &args);
            if results.is_empty() {
                w.line(format!("{call};"));
            } else {
                w.line(format!("{ret_ty} __r = {call};"));
            }
            w.line(format!("while ({field} != null) {{"));
            w.indent();
            w.line(format!("{} __t = {field};", self.tail_iface(results)));
            w.line(format!("{field} = null;"));
            if results.is_empty() {
                w.line("__t.run();");
            } else {
                w.line("__r = __t.run();");
            }
            w.dedent();
            w.line("}");
            if !results.is_empty() {
                w.line("return __r;");
            }
            w.dedent();
            w.line("}");
            w.line("");
        }
        let method_name = if is_tail_caller {
            format!("f{idx}Body")
        } else {
            format!("f{idx}")
        };

        let mut local_types = ty.params.clone();
        local_types.extend(func.locals.iter().copied());

        // The `CostMemo` is keyed by node address.
        // So it is only valid while the nodes it covers are live.
        // This is the single entry point for a function body.
        // So clearing here bounds the `CostMemo` to one function's statements.
        self.costs.clear();
        let split = self.costs.seq(&func.body) > SPLIT_THRESHOLD;
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
            w.line(self.method_head(&ret_ty, &method_name, &params_str));
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
            w.line(self.method_head(&ret_ty, &method_name, &params_str));
            w.indent();
            // Locals start at their type's zero.
            // A run of adjacent locals of one type becomes a single declaration.
            // Java has no chained assignment inside one declaration.
            // So each name keeps its own initializer and only the type name is shared.
            for run in local_runs(&local_types[nparams..], |t| t) {
                let lt = local_types[nparams + run.start];
                let names: Vec<String> = run
                    .map(|k| format!("l{} = {}", nparams + k, zero_value(lt)))
                    .collect();
                w.line(format!("{} {};", jtype(lt), names.join(", ")));
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
            let mut free = BTreeSet::new();
            for stmt in &func.body {
                self.emit_stmt(w, stmt, &mut guarded, &mut free);
            }
            if has_result {
                w.line("return _ret;");
            }
            w.dedent();
            w.line("}");
        }
    }

    /// The specification harness's name-based dispatcher.
    /// `invoke(name, args)` boxes every result into an `Object[]`.
    /// The array is empty for a `void` export and has one element for a single result.
    /// A multi-value export returns the function's own `Object[]`.
    /// Mirrors Go's name-based `invoke` under Java's static typing.
    fn emit_invoke_method(&self, w: &mut CodeWriter) {
        w.line("Object[] invoke(String name, Object[] a) {");
        w.indent();
        w.line("switch (name) {");
        w.indent();
        for export in &self.module.exports {
            let ExportKind::Func(idx) = export.kind else {
                continue;
            };
            let ty = self.module.func_type(idx);
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

    /// The specification harness's global reader.
    /// It returns the exported boxed global's current value in a one-element `Object[]`.
    /// So the harness treats it like a single-result `invoke`.
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

    /// Emit a statement sequence as the body of a construct.
    /// The construct is a loop/if/block body or the function entry.
    /// It is inline when small.
    /// It becomes chained `part` methods when the function is split and the sub-body is large.
    ///
    /// Returns the sequence's *free* branch targets.
    /// Those are the labels it branches to that are not bound within it.
    /// The caller unions that set upward, minus the label it binds itself.
    /// So the information is derived once bottom-up.
    /// Re-deriving it top-down at every outer block made conversion quadratic in nesting depth.
    fn emit_body(&self, w: &mut CodeWriter, stmts: &[Stmt], guarded_in: bool) -> BTreeSet<u32> {
        let mut free = BTreeSet::new();
        if self.split.get() && self.costs.seq(stmts) > SPLIT_THRESHOLD {
            let part_args = if self.partitioned.get() {
                "inst, f"
            } else {
                "f"
            };
            for name in self.emit_parts(stmts, guarded_in, &mut free) {
                w.line(format!("{name}({part_args});"));
            }
        } else {
            let mut guarded = guarded_in;
            for stmt in stmts {
                self.emit_stmt(w, stmt, &mut guarded, &mut free);
            }
        }
        free
    }

    /// Split `stmts` into `part` methods in order, each below the threshold.
    /// The `guarded` flag is threaded across the boundaries.
    /// Parts are called unconditionally in order.
    /// Control flow is carried by the `f.br` register, and each part self-guards.
    /// So an escaped branch no-ops the rest.
    fn emit_parts(
        &self,
        stmts: &[Stmt],
        guarded_in: bool,
        free: &mut BTreeSet<u32>,
    ) -> Vec<String> {
        let mut names = Vec::new();
        let mut w = CodeWriter::new("\t");
        let mut guarded = guarded_in;
        let mut cost = 0usize;
        for stmt in stmts {
            let c = self.costs.stmt(stmt);
            if cost > 0 && cost + c > SPLIT_THRESHOLD {
                let name = self.new_part();
                self.push_part(&name, w.finish());
                names.push(name);
                w = CodeWriter::new("\t");
                cost = 0;
            }
            self.emit_stmt(&mut w, stmt, &mut guarded, free);
            cost += c;
        }
        let name = self.new_part();
        self.push_part(&name, w.finish());
        names.push(name);
        names
    }

    /// Split a `br_table`'s targets into adjacent case ranges.
    /// Each range has at most `SPLIT_THRESHOLD` estimated cost.
    /// Returns every range's end index, one past its last.
    /// So a one-element result means the whole table fits in a single method.
    fn br_table_groups(&self, targets: &[BrTarget]) -> Vec<usize> {
        let mut ends = Vec::new();
        let mut cost = 0usize;
        for (n, t) in targets.iter().enumerate() {
            let c = 1 + target_cost(t);
            if cost > 0 && cost + c > SPLIT_THRESHOLD {
                ends.push(n);
                cost = 0;
            }
            cost += c;
        }
        ends.push(targets.len());
        ends
    }

    /// Emit a `br_table` too large for one method.
    /// Each case range of `ends` becomes a part method taking the table index.
    /// The call site dispatches to it by range.
    /// A statement sequence splits at its statement boundaries.
    /// A table with thousands of targets is a single statement, though.
    /// So the split recurses into the case list, the only place it can (issue #142).
    /// CPython's largest function holds a 3202-target table, and 44 tables in one sequence.
    fn emit_br_table_parts(
        &self,
        w: &mut CodeWriter,
        index: &Expr,
        targets: &[BrTarget],
        default: &BrTarget,
        ends: &[usize],
    ) {
        let mut names = Vec::with_capacity(ends.len());
        for (g, &end) in ends.iter().enumerate() {
            let start = if g == 0 { 0 } else { ends[g - 1] };
            let mut pw = CodeWriter::new("\t");
            pw.line("switch (_sw) {");
            for (k, target) in targets[start..end].iter().enumerate() {
                pw.line(format!("case {}: {{", start + k));
                pw.indent();
                self.branch(&mut pw, target);
                pw.line("break;");
                pw.dedent();
                pw.line("}");
            }
            pw.line("}");
            let name = self.new_part();
            self.push_part_with(&name, ", int _sw", pw.finish());
            names.push(name);
        }

        let part_args = if self.partitioned.get() {
            "inst, f"
        } else {
            "f"
        };
        // The index is read once into a scoped local.
        // An out-of-range index takes the default target.
        // The check is unsigned, so a negative `int` is out of range too.
        // The rest is a plain range chain over the parts.
        w.line("{");
        w.indent();
        w.line(format!("int _sw = {};", self.expr(index)));
        w.line(format!(
            "if (Integer.compareUnsigned(_sw, {}) >= 0) {{",
            targets.len()
        ));
        w.indent();
        self.branch(w, default);
        w.dedent();
        for (g, name) in names.iter().enumerate() {
            if g + 1 == names.len() {
                w.line("} else {");
            } else {
                w.line(format!("}} else if (_sw < {}) {{", ends[g]));
            }
            w.indent();
            w.line(format!("{name}({part_args}, _sw);"));
            w.dedent();
        }
        w.line("}");
        w.dedent();
        w.line("}");
    }

    /// Emit one statement, adding its free branch targets to `free` (see `emit_body`).
    fn emit_stmt(
        &self,
        w: &mut CodeWriter,
        stmt: &Stmt,
        guarded: &mut bool,
        free: &mut BTreeSet<u32>,
    ) {
        match stmt {
            Stmt::Block { label, body } => {
                let mut inner = self.emit_body(w, body, *guarded);
                self.reset_marker(w, label);
                inner.remove(&label.id);
                *guarded = *guarded || !inner.is_empty();
                free.extend(inner);
            }
            Stmt::Loop { label, body } => {
                if label.referenced {
                    let before = *guarded;
                    w.line("while (true) {");
                    w.indent();
                    let mut inner = self.emit_body(w, body, before);
                    w.line(format!(
                        "if ({0} == {1}) {{ {0} = 0; continue; }}",
                        self.br(),
                        label.id
                    ));
                    w.line("break;");
                    w.dedent();
                    w.line("}");
                    inner.remove(&label.id);
                    *guarded = before || !inner.is_empty();
                    free.extend(inner);
                } else {
                    let mut inner = self.emit_body(w, body, *guarded);
                    inner.remove(&label.id);
                    *guarded = *guarded || !inner.is_empty();
                    free.extend(inner);
                }
            }
            Stmt::If {
                label,
                cond,
                then,
                els,
            } => {
                let mut inner = self.emit_if(w, *guarded, cond, then, els);
                self.reset_marker(w, label);
                inner.remove(&label.id);
                *guarded = *guarded || !inner.is_empty();
                free.extend(inner);
            }
            Stmt::TryTable {
                label,
                catches,
                body,
            } => {
                let mut inner = self.emit_try_table(w, *guarded, catches, body);
                self.reset_marker(w, label);
                inner.remove(&label.id);
                *guarded = *guarded || !inner.is_empty();
                free.extend(inner);
            }
            // REASON: Java has no line-directive to render source-line markers into.
            // Drop them here, not via the `_br` guard.
            // So guard state and emitted output stay byte-identical to a non-`--dwarf-line` build.
            Stmt::SourceLine(_) => {}
            // Every other statement is a leaf; `simple_stmt` matches them all exhaustively.
            // So a new variant is a compile error there rather than silent output.
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
                if collect_leaf_free_targets(stmt, free) {
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

    /// Emit an `if`, returning the free branch targets of both arms.
    /// The caller removes the `if`'s own label.
    fn emit_if(
        &self,
        w: &mut CodeWriter,
        guarded: bool,
        cond: &Expr,
        then: &[Stmt],
        els: &[Stmt],
    ) -> BTreeSet<u32> {
        let cond_s = self.cond(cond);
        if guarded {
            // `_br == 0 &&` evaluates its right side only when the left holds.
            // So `cond` is not evaluated, and cannot trap, while a branch is pending.
            w.line(format!("if ({} == 0 && {cond_s}) {{", self.br()));
        } else {
            w.line(format!("if ({cond_s}) {{"));
        }
        w.indent();
        let mut free = self.emit_body(w, then, guarded);
        w.dedent();
        if els.is_empty() {
            w.line("}");
        } else if guarded {
            w.line(format!("}} else if ({} == 0) {{", self.br()));
            w.indent();
            free.extend(self.emit_body(w, els, guarded));
            w.dedent();
            w.line("}");
        } else {
            w.line("} else {");
            w.indent();
            free.extend(self.emit_body(w, els, guarded));
            w.dedent();
            w.line("}");
        }
        free
    }

    /// Emit a `try_table` as a Java `try`/`catch` around the body.
    /// Returns the free branch targets of the body and of the reachable catch clauses.
    /// The caller removes the frame's own label.
    /// A clause writes the payload into the target frame's slots.
    /// It then sets the branch register just as a branch out of the body would.
    /// So the handler needs no exit of its own.
    /// `_br` skips the rest of the outer sequence either way.
    /// Only `Rt.WasmException` is caught.
    /// So `catch_all` structurally cannot catch a trap, exhaustion, or the exit path.
    /// The body may still be split into `part` methods.
    /// Every slot lives in the frame object.
    /// An exception unwinding out of a part reaches this `catch` up the JVM stack.
    /// So the try region stays in one method without keeping the body in that method.
    fn emit_try_table(
        &self,
        w: &mut CodeWriter,
        guarded: bool,
        catches: &[CatchClause],
        body: &[Stmt],
    ) -> BTreeSet<u32> {
        self.use_unit("rt/wasm_exception");
        w.line("try {");
        w.indent();
        let mut free = self.emit_body(w, body, guarded);
        w.dedent();
        w.line("} catch (Rt.WasmException __e) {");
        w.indent();
        let mut chained = false;
        let mut exhaustive = false;
        for clause in catches {
            match clause.tag {
                // wasm tag equality is object identity, never structure.
                Some(tag) => {
                    let cond = format!("__e.tag == {}", self.iref(&format!("tag{tag}")));
                    if chained {
                        w.line(format!("}} else if ({cond}) {{"));
                    } else {
                        w.line(format!("if ({cond}) {{"));
                        chained = true;
                    }
                    w.indent();
                    self.catch_clause(w, clause, &mut free);
                    w.dedent();
                }
                // A catch-all matches unconditionally, so it closes the chain.
                // Every clause after it is dead.
                None => {
                    if chained {
                        w.line("} else {");
                        w.indent();
                        self.catch_clause(w, clause, &mut free);
                        w.dedent();
                    } else {
                        self.catch_clause(w, clause, &mut free);
                    }
                    exhaustive = true;
                    break;
                }
            }
        }
        if !exhaustive {
            // No clause matched: the exception keeps unwinding.
            if chained {
                w.line("} else {");
                w.indent();
                w.line("throw __e;");
                w.dedent();
                w.line("}");
            } else {
                w.line("throw __e;");
            }
        } else if chained {
            w.line("}");
        }
        w.dedent();
        w.line("}");
        free
    }

    /// One `try_table` catch clause inside the handler.
    /// It binds the payload into the target frame's slots, then takes the branch.
    /// The payload arrives boxed, the same convention every dynamic boundary uses.
    /// So each value is unboxed to its slot's type.
    /// The `_ref` kinds bind the exception object itself, which *is* the `exnref` value.
    fn catch_clause(&self, w: &mut CodeWriter, clause: &CatchClause, free: &mut BTreeSet<u32>) {
        for (i, t) in clause.value_temps.iter().enumerate() {
            let src = if Some(*t) == clause.exn_temp {
                "__e".to_string()
            } else {
                unbox(t.ty, &format!("__e.values[{i}]"))
            };
            w.line(format!("{} = {src};", self.temp_ref(*t)));
        }
        self.branch(w, &clause.target);
        collect_target_free(&clause.target, free);
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
                w.line(format!("if ({}) {{", self.cond(cond)));
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
                let groups = self.br_table_groups(targets);
                if groups.len() > 1 && self.split.get() {
                    self.emit_br_table_parts(w, index, targets, default, &groups);
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
                if self.module.func_type(*func).results.len() > 1 {
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
            // Parked, never called.
            // The outer frame's `_br = -1` unwinds it, including any `try_table` handler.
            // Only then does the entry's trampoline run the callee.
            // The target is the callee's tail entry, built once at instantiation.
            // So a hop allocates nothing.
            Stmt::ReturnCall { func, args } => {
                let fty = self.module.func_type(*func).clone();
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                match self.tail_slot(*func) {
                    Some(k) => {
                        let target = format!("{}[{k}]", self.iref(&self.tail_table(&fty.results)));
                        self.park_tail(w, &fty.params, &fty.results, &args, &target);
                    }
                    // A callee with no tail entry here still has to run once this frame is gone.
                    // Otherwise a `try_table` this frame opened would catch an exception it throws.
                    // So it is parked too, as a lambda over the arguments.
                    // That costs one allocation, on a path a chain does not take.
                    None => {
                        let bound = self.bind_finals(w, &fty.params, &args);
                        let call = self.call_string(*func, &bound);
                        self.park_lambda(w, &fty.results, &call);
                    }
                }
            }
            Stmt::ReturnCallIndirect {
                type_idx,
                table_index,
                index,
                args,
            } => {
                self.use_unit("table/tail_ref");
                let ty = self.module.types[*type_idx as usize].clone();
                // The slot is resolved and its traps raised here, not after the frame is gone.
                // An indirect tail call's checks happen at the instruction.
                // A tail entry reads its owner's parked slots.
                // So only this instance's own entries can be parked.
                // Anything else completes here instead.
                let slot = self.next_tail_name();
                w.line(format!(
                    "Rt.Funcref {slot} = {}.tailSlot({}, {});",
                    self.iref(&format!("t{table_index}")),
                    self.expr(index),
                    java_string(&self.type_symbol(*type_idx))
                ));
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                w.line(format!("if ({slot}.owner == {}) {{", self.self_ref()));
                w.indent();
                let cast = format!("({}) {slot}.body", self.tail_iface(&ty.results));
                self.park_tail(w, &ty.params, &ty.results, &args, &cast);
                w.dedent();
                w.line("} else {");
                w.indent();
                // Another instance's entry reads its own slots, so it cannot be parked.
                // It is wrapped instead, which keeps it running after this frame is gone.
                // The wrapper costs one allocation.
                let bound = self.bind_finals(w, &ty.params, &args);
                // The wrapper has to yield the entry interface's own type.
                // So a multi-value result is cast rather than unboxed.
                let call = match ty.results.as_slice() {
                    [_, _, ..] => format!(
                        "(Object[]) {}",
                        self.invoke_string(&format!("{slot}.fn"), &bound.join(", "), None)
                    ),
                    rs => self.invoke_string(
                        &format!("{slot}.fn"),
                        &bound.join(", "),
                        rs.first().copied(),
                    ),
                };
                self.park_lambda(w, &ty.results, &call);
                w.dedent();
                w.line("}");
            }
            Stmt::Unreachable => {
                // A `void` method that throws.
                // Unlike a `throw`, a statement causes no "unreachable statement" error after it.
                w.line(format!("{}(\"unreachable\");", self.rt("trap")));
            }
            // `void` helpers that throw.
            // As statements, not a Java `throw`, they avoid an "unreachable statement" error.
            // The error would hit the dead code wasm allows after them.
            Stmt::Throw { tag, args } => {
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                w.line(format!(
                    "{}({}, new Object[]{{{}}});",
                    self.rt("wasm_exception"),
                    self.iref(&format!("tag{tag}")),
                    args.join(", ")
                ));
            }
            Stmt::ThrowRef { exn } => {
                w.line(format!("{}({});", self.rt("throw_ref"), self.expr(exn)));
            }
            // REASON: Java has no line-directive to render source-line markers into.
            // `emit_stmt` drops them before routing here.
            // So this arm is unreachable, but kept for the exhaustive match.
            Stmt::SourceLine(_) => {}
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
            Stmt::Block { .. } | Stmt::Loop { .. } | Stmt::If { .. } | Stmt::TryTable { .. } => {
                unreachable!("structured statement routed to simple_stmt");
            }
        }
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        match values.len() {
            0 => {}
            1 => w.line(format!("{} = {};", self.ret(), self.expr(&values[0]))),
            _ => {
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
                // `is_loop` is irrelevant.
                // The loop trailer turns `_br == <loop id>` into a `continue`.
                // The guards resolve a block/if exit by skipping to the label's reset marker.
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

    /// The `Object[]` a multi-value call produces.
    /// A defined method returns one directly.
    /// An imported `Fn.invoke` returns `Object`, cast to `Object[]`.
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

    /// Unpack a multi-value call's `Object[]` into the result temps.
    /// Each slot is unboxed to its wasm type.
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

    /// A direct call to a function by index.
    /// An imported one is a boxed `Fn` `invoke`; a defined one is a primitive method call.
    fn call_string(&self, func_idx: u32, args: &[String]) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            let ty = self.module.func_type(func_idx);
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

    /// Call an `Rt.Fn` value with boxed arguments.
    /// The single result, if any, is unboxed to its Java primitive.
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
            // `eqz` of something already emitted as a Java `boolean`.
            // Read the wasm 0/1 straight off that `boolean`.
            // Do not materialize the operand's own 0/1 first and test it.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) if is_boolean(a) => {
                format!("({} ? 0 : 1)", self.cond(a))
            }
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
                    "({} ? ({}) : ({}))",
                    self.cond(cond),
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

    /// `expr` in a condition context, as a Java `boolean`.
    ///
    /// A wasm comparison yields the i32 0 or 1.
    /// Every conditional context then compares that against 0.
    /// So the lowering built a conditional expression only to reverse it one operation later.
    /// Emitting the comparison as a Java `boolean` drops both the conditional and the test.
    /// The operands are untouched.
    /// So an unsigned view still goes through `Integer.compareUnsigned`/`Long.compareUnsigned`.
    /// Anything else keeps the `!= 0` test.
    /// This is ported from the Ruby backend (#122).
    fn cond(&self, e: &Expr) -> String {
        match e {
            // `eqz` in boolean context is the negation of its operand's own test.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => self.not_cond(a),
            Expr::Bin(op, a, b) => match rel_op(*op) {
                Some(r) => rel(r, &self.expr(a), &self.expr(b)),
                None => format!("({}) != 0", self.expr(e)),
            },
            _ => format!("({}) != 0", self.expr(e)),
        }
    }

    /// The negation of [`Gen::cond`]: `e` is zero.
    /// A comparison is negated as a whole rather than by flipping its operator.
    /// Flipping would be wrong for floats: `x < y` and `x >= y` are both false when either is NaN.
    fn not_cond(&self, e: &Expr) -> String {
        match e {
            // Two negations cancel.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => self.cond(a),
            Expr::Bin(op, ..) if rel_op(*op).is_some() => format!("!({})", self.cond(e)),
            _ => format!("({}) == 0", self.expr(e)),
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
            // `abs`/`neg` are bit operations on the sign bit: they must NOT quiet a NaN.
            // Math.abs would leave a negative NaN's sign set.
            F32Abs => format!("Float.intBitsToFloat(Float.floatToRawIntBits({a}) & 0x7fffffff)"),
            F32Neg => format!("Float.intBitsToFloat(Float.floatToRawIntBits({a}) ^ 0x80000000)"),
            F64Abs => format!(
                "Double.longBitsToDouble(Double.doubleToRawLongBits({a}) & 0x7fffffffffffffffL)"
            ),
            F64Neg => format!(
                "Double.longBitsToDouble(Double.doubleToRawLongBits({a}) ^ 0x8000000000000000L)"
            ),
            // `ceil`/`floor`/`nearest`/`sqrt` canonicalize a NaN result to wasm's arithmetic NaN.
            // Java's Math.* may pass a signaling operand through unquieted.
            // `trunc` is a helper (single operand evaluation).
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
            // Trapping float-to-integer conversions go through helpers that trap on NaN/overflow.
            // The source is widened to double first, which is exact for f32.
            // The saturating signed forms are Java's cast.
            // The saturating unsigned forms need helpers.
            // Java's cast wraps past the unsigned range.
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
        // A comparison is a Java `boolean`.
        // Outside condition position it needs the conditional back to the i32 0 or 1 wasm expects.
        // See `cond`.
        if let Some(r) = rel_op(op) {
            return format!("({} ? 1 : 0)", rel(r, a, b));
        }
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
            F32Add | F64Add => format!("(({a}) + ({b}))"),
            F32Sub | F64Sub => format!("(({a}) - ({b}))"),
            F32Mul | F64Mul => format!("(({a}) * ({b}))"),
            F32Div | F64Div => format!("(({a}) / ({b}))"),
            // Java's `Math.min`/`Math.max` pass a signaling NaN operand through unquieted.
            // `Math.copySign` may read one NaN's sign as positive and another's as negative.
            // Wasm `min`/`max` return an arithmetic NaN.
            // Wasm `copysign` is a pure sign bit operation.
            // So both go through explicit code.
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
            _ => unreachable!("op {op:?} is a comparison, rendered by `rel`"),
        }
    }
}

/// The Java type of a function's result register (`_ret` / frame `ret` / the method return).
/// It is the single value type, `Object[]` for multi-value, or `void` for no result.
fn ret_slot_ty(results: &[ValType]) -> String {
    match results {
        [] => "void".to_string(),
        [t] => jtype(*t).to_string(),
        _ => "Object[]".to_string(),
    }
}

/// The initial value of the result register: the value type's zero.
/// Multi-value starts as an `Object[]` of boxed zeros.
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

fn unbox(ty: ValType, expr: &str) -> String {
    match ty {
        ValType::I32 => format!("(int)(Integer) {expr}"),
        ValType::I64 => format!("(long)(Long) {expr}"),
        ValType::F32 => format!("(float)(Float) {expr}"),
        ValType::F64 => format!("(double)(Double) {expr}"),
        ValType::FuncRef => format!("(Rt.Funcref) {expr}"),
        ValType::ExnRef => format!("(Rt.WasmException) {expr}"),
    }
}

/// An ENOSYS stub `Rt.Fn` for an unimplemented WASI import.
/// An `i32`-result system call returns `errno` 52; everything else returns zero values / `null`.
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
        ValType::FuncRef | ValType::ExnRef => "null",
    }
}

/// Emit a data blob as a chunked-Base64 constant decoded at runtime.
/// The chunks stay under Java's 64KB string-literal limit.
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

/// Standard Base64 (RFC 4648) encoder, matching `java.util.Base64.getDecoder`.
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

/// Add the labels a non-structured statement branches to into `free`.
/// A `Return` contributes `RETURN_SENTINEL`.
/// Returns whether the statement has any.
/// Non-empty means the statement may leave `_br` set on fall-through.
/// Later statements in the same sequence must then be guarded.
/// Structured statements get their free set from `emit_body`.
/// It builds the set bottom-up as it emits.
fn collect_leaf_free_targets(stmt: &Stmt, free: &mut BTreeSet<u32>) -> bool {
    match stmt {
        Stmt::Br(t) | Stmt::BrIf { target: t, .. } => {
            collect_target_free(t, free);
            true
        }
        Stmt::BrTable {
            targets, default, ..
        } => {
            collect_target_free(default, free);
            for t in targets {
                collect_target_free(t, free);
            }
            true
        }
        // A tail call ends the frame the way a return does: it writes the branch register.
        // So later statements in the same sequence must be guarded.
        Stmt::Return { .. } | Stmt::ReturnCall { .. } | Stmt::ReturnCallIndirect { .. } => {
            free.insert(RETURN_SENTINEL);
            true
        }
        _ => false,
    }
}

fn collect_target_free(t: &BrTarget, free: &mut BTreeSet<u32>) {
    match t {
        BrTarget::Return { .. } => {
            free.insert(RETURN_SENTINEL);
        }
        BrTarget::Label { label, .. } => {
            free.insert(*label);
        }
    }
}

/// Statement-cost queries for the function being emitted, memoized by node identity.
/// They are the input to the 64KB method-split decision ([`SPLIT_THRESHOLD`]).
///
/// The split decision needs a body's cost *before* that body is emitted.
/// `emit_body` derives the free-branch-target set bottom-up while emitting.
/// Unlike that set, the cost cannot be derived during emission.
/// Asked without a cache, it is re-derived top-down at every outer level.
/// It is derived again per statement of a sequence in `emit_parts`.
/// That is the same O(nodes x nesting depth) shape that made the target query quadratic.
/// See issue #62.
///
/// Entries are keyed by the *address* of the `Stmt` node.
/// That is sound because `Gen` borrows its `Module` immutably for the whole of `generate_source`.
/// Nothing changes the IR while emitting, and there is no threading.
/// So every statement reachable from a function body sits at a fixed, unique address.
/// It stays there for at least as long as this table.
/// `Gen::function` clears it per function anyway, to bound it.
///
/// Only `Block`/`Loop`/`If` are memoized.
/// That alone makes the whole query linear.
/// A leaf's cost is recomputed a bounded number of times.
/// A structured statement's cost would otherwise be recomputed once per outer level.
/// It also keeps hashing off the hot leaf path.
#[derive(Default)]
struct CostMemo(RefCell<HashMap<usize, usize>>);

impl CostMemo {
    fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    fn seq(&self, stmts: &[Stmt]) -> usize {
        stmts.iter().map(|s| self.stmt(s)).sum()
    }

    fn stmt(&self, stmt: &Stmt) -> usize {
        match stmt {
            Stmt::Block { .. } | Stmt::Loop { .. } | Stmt::If { .. } | Stmt::TryTable { .. } => {
                let key = stmt as *const Stmt as usize;
                // Copy the hit out and drop the borrow before recursing.
                // `compute` re-enters and takes the table mutably.
                let hit = self.0.borrow().get(&key).copied();
                if let Some(c) = hit {
                    return c;
                }
                let c = self.compute(stmt);
                self.0.borrow_mut().insert(key, c);
                c
            }
            // Only a statement holding a body is worth a `CostMemo` entry.
            // Every other one is computed directly, and `compute` matches them exhaustively.
            _ => self.compute(stmt),
        }
    }

    fn compute(&self, stmt: &Stmt) -> usize {
        1 + match stmt {
            Stmt::Assign { expr, .. }
            | Stmt::LocalSet { expr, .. }
            | Stmt::GlobalSet { expr, .. } => expr_cost(expr),
            Stmt::Store { addr, value, .. } => expr_cost(addr) + expr_cost(value),
            Stmt::Block { body, .. } | Stmt::Loop { body, .. } => self.seq(body),
            Stmt::If {
                cond, then, els, ..
            } => expr_cost(cond) + self.seq(then) + self.seq(els),
            Stmt::Br(t) => target_cost(t),
            Stmt::BrIf { cond, target } => expr_cost(cond) + target_cost(target),
            Stmt::BrTable {
                index,
                targets,
                default,
            } => {
                // Every target expands to a `case n: { ...; break; }` arm.
                // So a table costs at least one node per target.
                // That holds even when no target carries assignments.
                // Counting only the assignments made a thousands-of-targets table look free.
                // The function holding it was then left unsplit (issue #142).
                expr_cost(index)
                    + target_cost(default)
                    + targets.iter().map(|t| 1 + target_cost(t)).sum::<usize>()
            }
            Stmt::Return { values } => values.iter().map(expr_cost).sum(),
            Stmt::Call { args, .. } | Stmt::ReturnCall { args, .. } => {
                args.iter().map(expr_cost).sum()
            }
            Stmt::CallIndirect { index, args, .. }
            | Stmt::ReturnCallIndirect { index, args, .. } => {
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
            // Each catch clause expands to a guarded arm binding its payload slots.
            // The arms add to the body's cost.
            Stmt::TryTable { body, catches, .. } => {
                self.seq(body)
                    + catches
                        .iter()
                        .map(|c| 1 + c.value_temps.len() + target_cost(&c.target))
                        .sum::<usize>()
            }
            Stmt::Throw { args, .. } => args.iter().map(expr_cost).sum(),
            Stmt::ThrowRef { exn } => expr_cost(exn),
            Stmt::DataDrop { .. }
            | Stmt::ElemDrop { .. }
            | Stmt::Unreachable
            | Stmt::SourceLine(_) => 0,
        }
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

/// Lint for the runtime units.
/// Every reference a unit body makes to another unit must be declared in its `// requires:` header.
/// Mirrors the Go backend's units lint, adjusted for Java.
/// It checks three kinds of reference:
/// - `Rt.<name>` helper calls;
/// - `memory.<name>` memory-method calls;
/// - calls within one scope: an unqualified `name(` not preceded by a `.`.
///
/// A second test compiles the whole bundle with `javac`.
/// So a syntax error in any unit is caught, not just in the subset any one module uses.
/// A missing toolchain fails loud.
#[cfg(test)]
mod units {
    use super::*;
    use std::collections::BTreeSet;

    use regex::Regex;

    #[test]
    fn all_units_bundle() {
        bundler().bundle_all(0).expect("full bundle resolves");
    }

    #[test]
    fn declared_requires_cover_references() {
        let b = bundler();
        let unit_ids: BTreeSet<&str> = b.units().map(|u| u.id.as_str()).collect();

        let rt_call = Regex::new(r"Rt\.([a-z_][a-z0-9_]*)").unwrap();
        let memory_call = Regex::new(r"\bmemory\.([a-z_][a-z0-9_]*)").unwrap();
        // One `sibling_calls` matcher per scoped unit.
        // It matches an unqualified `name(` not preceded by a `.`.
        // So `memory.init(` is not read as a call to the unit `init` of the same scope.
        let sibling_calls: Vec<(&str, Regex)> = unit_ids
            .iter()
            .filter_map(|id| {
                let name = id.split('/').nth(1).unwrap();
                if name.starts_with('_') {
                    return None;
                }
                let re = Regex::new(&format!(r"(^|[^\w.]){}\s*\(", regex::escape(name))).unwrap();
                Some((*id, re))
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
                if dep.ends_with("/_class") || dep.ends_with("/_prelude") {
                    return;
                }
                problems.push(format!(
                    "{}: uses {what} but does not require {dep}",
                    unit.id
                ));
            };

            // Strip `//` comment lines so requires headers/comments don't count.
            let code: String = unit
                .body
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n");

            for cap in rt_call.captures_iter(&code) {
                demand(format!("rt/{}", &cap[1]), &format!("Rt.{}", &cap[1]));
            }
            for cap in memory_call.captures_iter(&code) {
                demand(
                    format!("memory/{}", &cap[1]),
                    &format!("memory.{}", &cap[1]),
                );
            }
            for (sibling, re) in &sibling_calls {
                // Calls in `sibling_calls` are only in-scope (no receiver prefix).
                let Some(name) = sibling.strip_prefix(&format!("{scope}/")) else {
                    continue;
                };
                if *sibling == unit.id {
                    continue;
                }
                if re.is_match(&code) {
                    demand(sibling.to_string(), &format!("{name}(...)"));
                }
            }
        }
        assert!(
            problems.is_empty(),
            "unit dependency drift:\n{}",
            problems.join("\n")
        );
    }

    /// The whole runtime (every unit, not just the subset any one module uses) must be valid Java.
    /// Compile the full bundle with `javac`.
    #[test]
    fn all_units_compile_as_java() {
        let source = full_bundle_java().expect("full bundle assembles");
        let dir = std::env::temp_dir().join(format!("dewasm-java-units-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("Main.java");
        std::fs::write(&src, &source).unwrap();
        let out = javac_command()
            .arg("-d")
            .arg(&dir)
            .arg(&src)
            .output()
            .expect("spawn javac");
        assert!(
            out.status.success(),
            "full runtime bundle failed to compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
