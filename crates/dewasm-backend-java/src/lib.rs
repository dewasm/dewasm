//! Java backend: translates dewasm IR into a single self-contained `.java` source file (one package-private module class carrying the runtime as `static` nested classes), compiled with `javac` and run on the JVM.
//!
//! Lowering conventions:
//! - i32/i64 are native signed `int`/`long` treated as bit patterns; unsigned ops use `Integer.*`/`Long.*` (`divideUnsigned`, `compareUnsigned`, ...). f32/f64 are native `float`/`double` (Java is strict IEEE, no FMA contraction, so f32 re-rounding and trap-free division need no helper).
//!   NaN bit paths go through `Float.floatToRawIntBits`/`intBitsToFloat` etc.
//! - Control flow uses the per-function branch register `_br` (mirroring Python's model, depth-insensitive and, crucially, splittable across methods): block/if exits and the function return set `_br`; following siblings are guarded by `if (_br == 0)`; only real loops become `while (true)`.
//!   This avoids Java's "unreachable statement" error entirely (no bare mid-sequence `return`/`break`), and makes the split below mechanical.
//! - Exception handling is native: a tag is an identity object (`Rt.Tag`), a thrown exception is an `Rt.WasmException` that doubles as the exnref value, and `try_table` is a Java `try`/`catch` whose clauses bind the payload and then set `_br` like any other branch.
//!   Only `Rt.WasmException` is caught, never `Throwable`, so traps and the exit path structurally cannot be caught by `catch_all`.
//! - The JVM caps a method at 64KB of bytecode.
//!   A function whose estimated size crosses a threshold is emitted with its locals/temps/`br`/`ret` hoisted to a per-call **frame object** and its body split into numbered `part` methods sharing that frame; because control flow is data (`_br`), the parts are just called in order.
//!   Data segments that exceed the 64KB string-literal limit are emitted as chunked Base64 (`Rt.data_from_b64`).
//!
//! The runtime is composed from per-method units referenced as `Rt.<name>` / `Memory` / `Table` / `WASI`.
//! In self-contained output those classes are nested in the module class, so two artifacts coexist in one package; the shared-runtime path the spec harness drives keeps them top-level.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::{
    check_module_support, comparison, is_boolean, is_ident, is_wasi_module, load_method,
    local_runs, module_name_error, store_method, type_key, wasi_bundled, Backend, CodeWriter,
    CompareOperands, GenOptions, Mode, OutputFile, RuntimeBundler, RuntimeScope, SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, CatchClause, ElemItem, ElemKind, ExportKind, Expr, Func, Label, Module, Stmt,
    Temp, UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// Estimated body-cost above which a function is split into `part` methods to stay under the JVM's 64KB per-method bytecode limit.
/// Cost is an IR node count; the value is tuned so cowsay's largest functions compile.
const SPLIT_THRESHOLD: usize = 900;

/// Raw bytes per Base64 data chunk.
/// Base64 of this (~43.7KB) stays under Java's 64KB (65535-byte) string-literal limit.
const DATA_CHUNK: usize = 32768;

/// Element-segment size above which the funcref table is built by the nested `Elem` helper class instead of inline in the constructor.
/// A big table's thousands of funcref lambdas overflow both `<init>`'s 64KB limit and the module class's 65535-entry constant pool; the nested class has its own pool.
/// Tuned so qjs/sqlite (~550 entries) stay inline and only rg-scale tables (~4900) split.
const ELEM_SPLIT: usize = 1024;

/// Funcref entries per `elem{i}_pK` part method (each ~20 bytes of bytecode per entry stays well under the 64KB method limit).
const ELEM_PART: usize = 512;

/// Funcref entries per `ElemF{c}` filler class.
/// Every entry costs its class's constant pool a lambda (invokedynamic + method handle + synthetic method, ~7 entries) plus the `P{k}.f{idx}` method reference it calls (~3), so a single filler class saturates the 65535-entry pool at well under ten thousand entries: CRuby's 8737-entry table overflowed it (issue #142).
/// Kept low enough that a filler class stays around a third of the pool.
const ELEM_PER_CLASS: usize = 2048;

/// Defined-function count above which the module's functions are split across nested `P{k}` helper classes, each with its own 65535-entry constant pool.
/// A single class holding thousands of functions (their numeric literals, method refs, and names) overflows the pool: qjs (~1500) and sqlite (~1970) fit, but zeroperl (~2450) and rg (~7300) do not.
/// zeroperl's Perl core is constant-dense enough that it overflowed while under the former 3000 bound (`javac`: *too many constants*).
/// Kept just above sqlite's proven single-class size, so a module only partitions once it exceeds the largest size measured to fit.
const FN_PARTITION_THRESHOLD: usize = 2000;

/// Defined functions per partition class.
/// Kept under sqlite's proven single-class function count so no partition's pool overflows.
const FN_PER_PARTITION: usize = 1500;

/// A branch-register sentinel for "return from the function", distinct from any real label id (which are small).
/// Emitted as `-1`.
const RETURN_SENTINEL: u32 = u32::MAX;

/// The runtime unit bundler for Java (see crates/dewasm-backend-java/units/).
/// Each scope is a *top-level* package-private class wrapping its unit bodies (methods / nested types); generated code refers to them as `Rt.*` / `Memory` / `Table` / `WASI`.
/// This is the shared-runtime shape: the spec harness bundles one runtime for all the modules of a `.wast` file, and the multi-module shared-runtime composition does the same.
/// Self-contained `Embedded` output uses [`nested_bundler`] instead.
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| runtime_bundler(false))
}

/// The bundler behind `Backend::generate`'s `Embedded` output: the same units under the same simple names, wrapped as `static` **nested** classes so they belong to the generated module class.
/// Java resolves a simple name through enclosing class scopes, so every `Rt.trap(...)` / `new Memory(...)` inside the module class, units included, reads exactly as it did when the classes were top-level; only an *outside* reference has to spell the module class (`Program.Rt.Fn`).
/// Two independently generated artifacts can then sit in one package without their runtimes colliding.
fn nested_bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| runtime_bundler(true))
}

/// Build the bundler for either placement.
/// The two differ only in each scope's `open` line (`static` is what a nested class needs and what a top-level one may not have), so the scope list is written once.
fn runtime_bundler(nested: bool) -> RuntimeBundler {
    // `open` is `&'static str`, so both spellings of each scope's class header are written out rather than formatted.
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

/// The one way dewasm's suites invoke `javac`: the located compiler (a missing one fails loud), capped at C1 with the serial collector and one processor, because C2 cannot repay its compilation cost in a ~1 s run and N machine-sized JVMs destroy parallel scaling on a small CI runner.
/// The heap stays at the default: the slow category's qjs/DOOM sources need the headroom.
/// The `.class` output is byte-identical with and without these flags (verified on the cowsay and qjs standalone sources).
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
/// Each spawns a JVM (~0.35 s for `javac -version`), so both callers memoize the answer for the process: a test binary asks once per trial, and the toolchain cannot change under a running process.
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

/// A complete, compilable `.java` file bundling *every* runtime unit (plus a tiny `Main`), for the units lint's `javac` check that all units (not just the subset any one module uses) are valid Java.
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
            // Java floats are native IEEE float/double; NaN paths mirror Ruby's numeric conventions.
            Feature::Floats => SupportStatus::Supported,
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
            // Tags are identity objects, a thrown exception is a native Java exception that doubles as the exnref, and traps stay uncatchable.
            Feature::ExceptionHandling => SupportStatus::Supported,
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
        // The data file: every segment's bytes concatenated in segment order, matching the `data_offsets` prefix sums baked into the generated `Arrays.copyOfRange(DATA_BLOB, …)` slices.
        // Only emitted when there is data to externalize (otherwise nothing reads it).
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

/// Emit just the module class (constructor, functions, and the spec-harness `invoke`/`globalGet` dispatch methods), for the shared spec harness: the harness bundles one runtime for every module in a `.wast` file, so per-module output carries no runtime classes / `Main`.
/// Multi-value results, wasm-1.0 imports, and trapping conversions are all exercised here.
/// Returns the class source and the runtime units it references.
pub fn generate_program_with_units(
    module: &Module,
    type_name: &str,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&JavaBackend, module)?;
    // The spec-harness generation path never externalizes: pass None.
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
    // Prefix sums: `data_offsets[i]` is where segment `i` begins in the concatenated data-file blob.
    // Only consulted when externalizing.
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
    // Standalone output is a self-contained program: its module class is fixed and unqualified, not derived.
    // Library output uses the requested name verbatim, splitting off a package declaration when it is dotted.
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
    // A module whose function count crosses the threshold is split across nested `P{k}` classes, each with its own constant pool.
    // Set before the constructor: its exports/start emit function calls through `defined_call`, which qualifies them by partition.
    gen.partitioned
        .set(module.funcs.len() > FN_PARTITION_THRESHOLD);

    // Into its own writer: `uses` must be complete before the runtime bundle is assembled.
    let mut body = CodeWriter::new("\t");
    gen.constructor(&mut body);
    // Externalized data blob: a static field loaded once from the data file next to this program, sliced by the generated `Arrays.copyOfRange(DATA_BLOB, …)` calls.
    // Only emitted when there is data to externalize (otherwise the generated code never reads it).
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
    // The runtime lives *inside* the module class: two artifacts dropped into one package then own their runtime classes (trap type above all) instead of one silently winning.
    // Nothing inside the class changes, since Java resolves `Rt`/`Memory`/`Table`/`WASI` through the enclosing scope.
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

/// Re-indent a block of source by `levels`, leaving blank lines empty.
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

/// The standalone entry point: parse the runtime interface (`--dir HOST::GUEST` preopens, then the guest argv with argv[0] the program name), instantiate, run `_start` on a dedicated large-stack thread, and map `proc_exit`/trap to a process exit code (a trap prints to stderr and exits 134, mirroring Ruby/Python/Go).
/// The dedicated thread mirrors Python's mitigation: deep-but-valid guest recursion (issue #137) can exceed the JVM's default main-thread stack on some hosts, so `_start` runs on a 64 MiB thread instead.
/// Exceptions thrown on that thread do not cross `Thread.join()`, so the runnable catches everything and hands the result back through `failure`.
fn main_class(type_name: &str, wasi: bool) -> String {
    let args = if wasi { "wasiArgs" } else { "null" };
    let env = if wasi { "wasiEnv" } else { "new String[0]" };
    let preopens = if wasi { "preopens" } else { "null" };
    // Standalone WASI parses a leading run of `--dir HOST::GUEST` flags (wasmtime-style), stopping at `--` or the first non-flag token; the rest is the guest's argv[1..].
    // The JVM does not pass the launched file name to `main`, so argv[0] is the module class name.
    // The whole process environment passes through.
    // Without WASI there is nothing to preopen and no argv to deliver.
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

/// The module class a `--mode standalone` program defines: fixed, since nothing outside a self-contained program observes it.
/// It is also the standalone `argv[0]` the JVM cannot supply (see docs/standalone-interface.md).
pub const STANDALONE_CLASS: &str = "Program";

/// Split a validated library-mode module name into `(package, class)`: the last dot-separated segment is the class name, used verbatim; the leading segments, if any, are the `package` declaration.
/// `com.github.dewasm.Sqlite3` is how a caller gets both a conventional package and a conventional class name.
/// The grammar is character-level only: a segment that is a Java keyword (`int`, `package`, ...) passes here and fails in `javac` with the compiler's own message; a maintained keyword list is not worth its upkeep.
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

/// The Java rendering of a wasm comparison: the operator, and the unsigned-comparison helper its operands go through.
/// The helper is `None` wherever Java's own operator on the stored representation already matches wasm (integers are stored signed, so the signed forms are direct, and Java's float comparison agrees with wasm, NaN included).
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

/// A comparison as a Java boolean, from a `rel_op` mapping and the already rendered operands.
fn rel((r, cmp): (&str, Option<&str>), a: &str, b: &str) -> String {
    match cmp {
        None => format!("({a}) {r} ({b})"),
        Some(cmp) => format!("{cmp}({a}, {b}) {r} 0"),
    }
}

fn jtype(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "int",
        ValType::I64 => "long",
        ValType::F32 => "float",
        ValType::F64 => "double",
        ValType::FuncRef => "Rt.Funcref",
        // A caught exception is its own exnref value, so the reference type is the exception class itself and a null exnref is Java's null.
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
    /// Part-method definitions produced while emitting the current function, flushed after its entry method.
    part_defs: RefCell<Vec<String>>,
    /// Memoized body costs for the current function, driving the split decision (see `CostMemo`).
    costs: CostMemo,
    /// When true, a function value (`func_value`) captures the module instance as `inst.` rather than referencing `this` implicitly.
    /// Set while emitting the nested `Elem` helper class's static methods, whose funcref lambdas live in a separate constant pool.
    elem_capture: Cell<bool>,
    /// Whether this module's functions are split across nested `P{k}` classes (its function count crosses `FN_PARTITION_THRESHOLD`).
    /// When set, defined functions are `static` methods taking the module instance, called class-qualified.
    partitioned: Cell<bool>,
    /// True while emitting a function body inside a `P{k}` partition class, so instance references resolve through the passed `inst` parameter.
    in_partition: Cell<bool>,
    /// When `Some`, data segments are externalized into a binary data file of this filename (loaded once into the static `DATA_BLOB`) instead of embedded as chunked Base64; `data_offsets[i]` locates segment `i` in the blob.
    data_file: Option<String>,
    data_offsets: Vec<usize>,
}

impl<'a> Gen<'a> {
    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    /// The Java expression yielding a data segment's bytes: a copy of a slice of the externalized `DATA_BLOB` when `--data-file` is on, else the inline chunked-Base64 decode.
    /// Both yield a fresh `byte[]`, so the segment field stays independently mutable (`data.drop` sets it empty).
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
    /// The data file is resolved relative to this program's own code source: the jar's directory (regular file) or the class directory itself, so `java -cp <dir> Main` finds the data file alongside the class.
    /// Only called when externalizing and the module has data.
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

    /// Whether the code being emitted reaches the module's instance fields through a passed `inst` parameter (a `P{k}` partition-class function body or the `Elem` helper class) rather than an implicit `this`.
    fn via_inst(&self) -> bool {
        self.in_partition.get() || self.elem_capture.get()
    }

    /// Reference an instance field (`memory`, `g3`, `t0`, `if5`, `data2`, `elem0`): `inst.<name>` when reached via a passed instance, else the bare name (implicit `this`).
    fn iref(&self, name: &str) -> String {
        if self.via_inst() {
            format!("inst.{name}")
        } else {
            name.to_string()
        }
    }

    /// The instance to pass as the first argument of a partitioned static function call: `inst` inside a `P{k}`/`Elem` method, else `this`.
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
    /// In a partitioned module the method is `static` and takes the module instance as its first parameter, so it can live in a `P{k}` class with its own constant pool; otherwise it is a plain instance method.
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

    /// A call to a *defined* function by index, honoring partitioning: a partitioned module calls `P{k}.f{idx}(<inst>, args)`; otherwise `[inst.]f{idx}(args)` (the `inst.` form serves the non-partitioned `Elem` class).
    /// `args_joined` is the already-formatted argument list.
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

    /// A slot reference: a frame field when the function is split across `part` methods, else a plain local.
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

    /// `push_part` with extra parameters appended to the head (used by the outlined `br_table` case ranges, which take the table index).
    fn push_part_with(&self, name: &str, extra_params: &str, body: String) {
        let frame = self.cur_frame_ty.borrow().clone();
        // A partitioned module's parts are static and take the module instance alongside the frame, so instance references resolve through `inst`.
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

        // Tables: imported first, then defined (index space is imported_tables ++ tables).
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
            // Kept for `wasiInstance()`, which builds the bundled WASI the first time an import falls back to it: an embedder covering every WASI import never pays for one.
            w.line("this.wasiArgs = args;");
            w.line("this.wasiEnv = env;");
            w.line("this.wasiPreopens = preopens;");
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            self.emit_import(w, i, import);
        }

        // Globals: imported first, then defined; every global is a boxed `Global`.
        // Defined-global inits may read imported globals, so they resolve after the imported ones.
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

        // Tags: imported first, then defined (index space is imported_tags ++ tags).
        // A defined tag is a fresh identity object; an imported one must be the very object its origin defined, which is what makes `catch` match across instances.
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
        // A segment over ELEM_SPLIT is built by the nested `Elem` helper class (its own constant pool, chunked part methods), else inline.
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

        // Each data segment is materialized in its own `initData{i}()` method, not inline in the constructor.
        // A multi-MB segment lowers to a chunked-Base64 array whose initializer (plus the `memory.init` copy) would otherwise accumulate toward the 64KB `<init>` limit; splitting one method per segment keeps the constructor bounded regardless of how large or how many the segments are.
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

        // Let import providers bind to the fully-constructed instance.
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
        // Each is a self-contained method so an oversized segment never bloats `<init>` past the 64KB method limit.
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
                    // Active segments are dropped after instantiation, but stay addressable (as empty) by memory.init/data.drop.
                    w.line(format!("this.data{i} = new byte[0];"));
                }
                None => {
                    w.line(format!("this.data{i} = {blob};"));
                }
            }
            w.dedent();
            w.line("}");
        }

        // The nested `Elem` helper class builds any oversized funcref table (see the element-segment loop above).
        // It is a separate class, so its thousands of funcref lambdas land in their own 65535-entry constant pool instead of the module class's; each table is filled by chunked `elem{i}_pK` part methods to stay under the 64KB method limit.
        if !split_elems.is_empty() {
            self.emit_elem_class(w, &split_elems);
        }
    }

    /// The bundled WASI, built on first use.
    /// Nothing constructs it in the constructor: an embedder whose provider covers every WASI import gets no WASI at all (which is exactly what `wasi == null` says) while the first import that falls back builds it, with the memory already bound (the constructor resolves memory before any import).
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

    /// Emit the nested `Elem` helper class for the oversized element segments in `split_elems`.
    /// Each segment `i` gets an `elem{i}(inst)` factory that allocates the array and calls chunked `elem{i}_pK(inst, a)` fillers; the fillers themselves live in `ElemF{c}` classes of at most `ELEM_PER_CLASS` entries each, so no single pool holds more funcref lambdas than it can address.
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

    /// Emit the module's defined functions grouped into nested `P{k}` classes of up to `FN_PER_PARTITION` functions each, so no single class's constant pool overflows Java's 65535-entry limit.
    /// The functions are `static` and take the module instance; call sites reach them class-qualified via `defined_call`.
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
        // Table index space = imported_tables ++ tables.
        for i in 0..(m.imported_tables.len() + m.tables.len()) {
            w.line(format!("Table t{i};"));
        }
        // Global index space = imported_globals ++ globals; each is a boxed `Global`.
        for i in 0..(m.imported_globals.len() + m.globals.len()) {
            w.line(format!("Global g{i};"));
        }
        for i in 0..m.imported_funcs.len() {
            w.line(format!("Rt.Fn if{i};"));
        }
        // Tag index space = imported_tags ++ tags.
        if !m.imported_tags.is_empty() || !m.tags.is_empty() {
            self.use_unit("rt/tag");
        }
        for i in 0..(m.imported_tags.len() + m.tags.len()) {
            w.line(format!("Rt.Tag tag{i};"));
        }
        if wasi_bundled(m, self.default_wasi, bundler()) {
            // The bundled WASI is built on first fallback, not in the ctor, so the ctor arguments are kept for `wasiInstance()` to use.
            w.line("WASI wasi;");
            w.line("String[] wasiArgs;");
            w.line("String[] wasiEnv;");
            w.line("java.util.Map<String, String> wasiPreopens;");
        }
        // Element segments retained for table.init (active ones are emptied after instantiation).
        for i in 0..m.elems.len() {
            w.line(format!("Rt.Funcref[] elem{i};"));
        }
        // Every data segment is addressable by memory.init/data.drop (active ones become empty after instantiation), so all get a field.
        for i in 0..m.datas.len() {
            w.line(format!("byte[] data{i};"));
        }
        w.line("java.util.Map<String, Object> Exports;");
    }

    /// Resolve a non-function import (memory/table/global) into `target`, checking its *kind* via `instanceof`.
    /// A wrong-kind or missing value is a link error; the finer wasm type (a global's value type and mutability, a table/memory's limits) is not checked: the import-limits gap, wider for Java than Go since these carry no static type.
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
                // The adapter closes over the WASI the fallback just built, so the bundled WASI exists exactly when some import fell back to it (mirroring Ruby's `@wasi ||=`) rather than on the first *call*.
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
    /// When emitting the nested `Elem` helper class, functions are reached through the passed module instance (`inst.`), so the lambdas can live in that class's own constant pool.
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
                format!(
                    "new Rt.Funcref({}, {})",
                    java_string(&self.func_type_symbol(*func_idx)),
                    self.func_value(*func_idx)
                )
            }
            ElemItem::Null => "null".to_string(),
            // ref-typed global element items imply reference types (rejected at conversion); unreachable, but must type-check as a Funcref.
            ElemItem::Global(idx) => {
                format!("(Rt.Funcref) {}.value", self.iref(&format!("g{idx}")))
            }
        }
    }

    fn func_type_symbol(&self, func_idx: u32) -> String {
        type_key(self.module.func_type(func_idx), ty_suffix)
    }

    /// A structural key for a function type ([`type_key`]), spelled with this backend's own [`ty_suffix`]: a table is only ever shared between artifacts of one backend, so the spelling only has to be self-consistent here.
    fn type_symbol(&self, type_idx: u32) -> String {
        type_key(&self.module.types[type_idx as usize], ty_suffix)
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &Func) {
        let ty = &self.module.types[func.type_idx as usize];
        let nparams = ty.params.len();
        let results = &ty.results;
        let has_result = !results.is_empty();
        // A method returns one value; multi-value signatures return a boxed `Object[]`.
        // The result register (`_ret` / frame `ret`) takes the same shape.
        let ret_ty = ret_slot_ty(results);
        let ret_init = ret_slot_init(results);

        let mut local_types = ty.params.clone();
        local_types.extend(func.locals.iter().copied());

        // The memo is keyed by node address, so it is only valid while the nodes it covers are live; this is the single entry point for a function body, so clearing here bounds it to one function's statements.
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
            // Locals start at their type's zero.
            // A run of consecutive locals of one type becomes a single declaration; Java has no chained assignment in a declarator list, so each name keeps its own initializer and only the type name is shared.
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

    /// The spec-harness reflective dispatcher: `invoke(name, args)` boxes every result into an `Object[]` (empty for a void export, one element for a single result, the function's own `Object[]` for multi-value).
    /// Mirrors Go's reflective invoke under Java's static typing.
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

    /// The spec-harness global reader: the exported boxed global's current value in a one-element `Object[]`, so the harness treats it exactly like a single-result `invoke`.
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

    /// Emit a statement sequence as the body of a construct (loop/if/block body or the function entry): inline when small, or split into chained `part` methods when the function is split and the sub-body is large.
    ///
    /// Returns the sequence's *free* branch targets: the label ids it branches to that are not bound within it.
    /// The caller unions that set upward (minus the label it binds itself), so the information is derived once bottom-up; re-deriving it top-down at every enclosing block made conversion quadratic in nesting depth.
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

    /// Split `stmts` into consecutive `part` methods (each below the threshold), threading the `guarded` flag across the boundaries.
    /// Parts are called unconditionally in order: control flow is carried by the `f.br` register and each part self-guards, so an escaped branch simply no-ops the rest.
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

    /// Split a `br_table`'s targets into consecutive case ranges, each of at most `SPLIT_THRESHOLD` estimated cost.
    /// Returns every range's exclusive end index, so a one-element result means the whole table fits in a single method.
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

    /// Emit a `br_table` too large for one method: each case range of `ends` becomes a part method taking the table index, and the call site dispatches to it by range.
    /// A statement sequence splits at its statement boundaries, but a table with thousands of targets is a single statement, so the split recurses into the case list, which is the only place it can (issue #142).
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
        // The index is read once into a scoped local; an out-of-range index (unsigned, so a negative `int` is out of range too) takes the default target, and the rest is a plain range cascade over the parts.
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
            // Drop them here (not via the `_br` guard) so the guard state and emitted output stay byte-identical to a non-`--dwarf-line` build.
            Stmt::SourceLine(_) => {}
            // Every other statement is a leaf; `simple_stmt` matches all of them exhaustively, so a new variant is a compile error there rather than silent output.
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

    /// Emit an `if`, returning the free branch targets of both arms (the `if`'s own label is removed by the caller).
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
            // `_br == 0 &&` short-circuits, so `cond` is not evaluated (and cannot trap) while a branch is pending.
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

    /// Emit a `try_table` as a Java `try`/`catch` around the body, returning the free branch targets of the body and of the reachable catch clauses (the frame's own label is removed by the caller).
    /// A clause writes the payload into the target frame's slots and then sets the branch register exactly as a branch out of the body would, so the handler needs no exit of its own: `_br` skips the rest of the enclosing sequence either way.
    /// Only `Rt.WasmException` is caught, so a trap, exhaustion, or the exit path structurally cannot be caught by `catch_all`.
    /// The body may still be split into `part` methods: every slot lives in the frame object and an exception unwinding out of a part reaches this `catch` up the JVM stack, so the try region stays in one method without pinning the body to it.
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
                // A catch-all matches unconditionally, so it closes the chain and every clause after it is dead.
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

    /// One `try_table` catch clause inside the handler: bind the payload into the target frame's slots, then take the branch.
    /// The payload arrives boxed (the same convention every dynamic boundary uses), so each value is unboxed to its slot's type; the `_ref` kinds bind the exception object itself, which *is* the exnref value.
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
            Stmt::Unreachable => {
                // Void method that throws: emitting it as a statement (not a `throw`) avoids an "unreachable statement" error after it.
                w.line(format!("{}(\"unreachable\");", self.rt("trap")));
            }
            // Void helpers that throw: emitting them as statements (not a Java `throw`) avoids an "unreachable statement" error on the dead code wasm allows after them.
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
            // REASON: Java has no line-directive to render source-line markers into; `emit_stmt` drops them before routing here, so this is unreachable but kept for the exhaustive match.
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
                // is_loop is irrelevant: the loop trailer turns `_br == <loop id>` into a `continue`; a block/if exit is resolved by the guards skipping to the label's reset marker.
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

    /// The `Object[]` a multi-value call produces: a defined method returns one directly; an imported `Fn.invoke` returns `Object`, cast to `Object[]`.
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

    /// Destructure a multi-value call's `Object[]` into the result temps, unboxing each slot to its wasm type.
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

    /// A direct call to a function by index (imported → boxed `Fn` invoke; defined → primitive method call).
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

    /// Invoke an `Rt.Fn` value with boxed args, unboxing the single result (if any) to its Java primitive.
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
            // `eqz` of something already emitted as a Java boolean: read the wasm 0/1 straight off that boolean instead of materializing the operand's own 0/1 first and testing it.
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

    /// `expr` in a condition context, as a Java boolean.
    ///
    /// A wasm comparison yields the i32 0 or 1, and every conditional context then compares that against 0, so the lowering built a conditional expression only to undo it one operation later.
    /// Emitting the comparison as a Java boolean drops both the conditional and the test; the operands are untouched, so an unsigned view still goes through `Integer.compareUnsigned`/`Long.compareUnsigned`.
    /// Anything else keeps the `!= 0` test. (Ported from the Ruby backend, #122.)
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
    /// A comparison is negated as a whole rather than by flipping its operator, which would be wrong for floats (both `x < y` and `x >= y` are false when either is NaN).
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
            // abs/neg are bit ops on the sign bit: they must NOT quiet a NaN (Math.abs would leave a negative NaN's sign set).
            F32Abs => format!("Float.intBitsToFloat(Float.floatToRawIntBits({a}) & 0x7fffffff)"),
            F32Neg => format!("Float.intBitsToFloat(Float.floatToRawIntBits({a}) ^ 0x80000000)"),
            F64Abs => format!(
                "Double.longBitsToDouble(Double.doubleToRawLongBits({a}) & 0x7fffffffffffffffL)"
            ),
            F64Neg => format!(
                "Double.longBitsToDouble(Double.doubleToRawLongBits({a}) ^ 0x8000000000000000L)"
            ),
            // ceil/floor/nearest/sqrt canonicalize a NaN result to wasm's arithmetic NaN (Java's Math.* may pass a signaling operand through unquieted); trunc is a helper (single operand eval).
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
            // Trapping float->int conversions go through helpers that trap on NaN/overflow; the source is widened to double first (exact for f32).
            // The saturating signed forms are exactly Java's cast; the saturating unsigned forms need helpers (Java's cast wraps past the unsigned range).
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
        // A comparison is a Java boolean; outside condition position it needs the conditional back to the i32 0 or 1 wasm expects (see `cond`).
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
            // Java's Math.min/max pass a signaling NaN operand through unquieted and Math.copySign may treat NaN's sign inconsistently; wasm min/max return an arithmetic NaN and copysign is a pure sign bit op, so both go through explicit code.
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

/// The Java type of a function's result register (`_ret` / frame `ret` / the method return): the single value type, `Object[]` for multi-value, or `void` for no result.
fn ret_slot_ty(results: &[ValType]) -> String {
    match results {
        [] => "void".to_string(),
        [t] => jtype(*t).to_string(),
        _ => "Object[]".to_string(),
    }
}

/// The initial value of the result register: the value type's zero, or an `Object[]` of boxed zeros for multi-value.
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

/// An ENOSYS stub `Rt.Fn` for an unimplemented WASI import: an i32-result syscall returns errno 52, everything else returns zero values / null.
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

/// Emit a data blob as a chunked-Base64 constant decoded at runtime, staying under Java's 64KB string-literal limit.
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

/// Add the label ids a non-structured statement branches to into `free` (a `Return` contributes the RETURN sentinel), returning whether it has any.
/// Non-empty means the statement may leave `_br` set on fall-through, so following siblings must be guarded.
/// Structured statements get their free set from `emit_body`, which builds it bottom-up as it emits.
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
        Stmt::Return { .. } => {
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

/// Statement-cost queries for the function being emitted, memoized by node identity, the input to the 64KB method-split decision ([`SPLIT_THRESHOLD`]).
///
/// The split decision needs a body's cost *before* that body is emitted, so unlike the free-branch-target set (which `emit_body` now derives bottom-up while emitting) the cost cannot ride along with emission.
/// Asked naively it is re-derived top-down at every enclosing level and again per sibling in `emit_parts`, which is the same O(nodes x nesting depth) shape that made the target query quadratic: see issue #62.
///
/// Entries are keyed by the *address* of the `Stmt` node.
/// That is sound because `Gen` borrows its `Module` immutably for the whole of `generate_source`, nothing mutates the IR while emitting, and there is no threading: every statement reachable from a function body sits at a fixed, unique address for at least as long as this table.
/// `Gen::function` clears it per function anyway, to bound it.
///
/// Only `Block`/`Loop`/`If` are memoized.
/// That alone makes the whole query linear (a leaf's cost is recomputed a bounded number of times, while a structured statement's would be recomputed once per enclosing level), and it keeps hashing off the hot leaf path.
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
                // Copy the hit out and drop the borrow before recursing: `compute` re-enters and takes the table mutably.
                let hit = self.0.borrow().get(&key).copied();
                if let Some(c) = hit {
                    return c;
                }
                let c = self.compute(stmt);
                self.0.borrow_mut().insert(key, c);
                c
            }
            // Only a statement holding a body is worth a memo entry; every other one is computed directly, and `compute` matches them exhaustively.
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
                // Every target expands to a `case n: { ...; break; }` arm, so a table costs at least one node per target even when no target carries assignments.
                // Counting only the assignments made a thousands-of-targets table look free, and the function holding it was left unsplit (issue #142).
                expr_cost(index)
                    + target_cost(default)
                    + targets.iter().map(|t| 1 + target_cost(t)).sum::<usize>()
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
            // Each catch clause expands to a guarded arm binding its payload slots, on top of the body.
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

/// Lint for the runtime units: every reference a unit body makes to another unit must be declared in its `// requires:` header.
/// Mirrors the Go backend's units lint, adjusted for Java: `Rt.<name>` helper calls, `memory.<name>` memory-method calls, and per-scope sibling calls (a bare `name(` not preceded by a `.`).
/// A second test compiles the whole bundle with `javac`, so a syntax error in any unit (not just the subset any one module uses) is caught (a missing toolchain fails loud).
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
        // One sibling-call matcher per scoped unit: a bare `name(` not preceded by a `.` (so `memory.init(` is not read as a call to the sibling `init`).
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
                // Sibling calls are only in-scope (no receiver prefix).
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
