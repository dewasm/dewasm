//! Go backend: translates dewasm IR into a single self-contained Go source file (a `package` clause plus a bundled runtime).
//!
//! The package clause follows the mode.
//! Standalone output is `package main` with the fixed type `Program` and a `func main`, so a standalone artifact is byte-stable whatever it was named.
//! Library output is `package <module name lowercased>` with the type `<module name, first letter uppercased>`: a Go artifact is a *package* an embedder imports, so the module name has to be a Go identifier and is rejected at conversion time when it is not (see [`validate_library_module_name`]).
//!
//! Lowering conventions:
//! - i32/i64 are native `uint32`/`uint64` (masking is free: arithmetic wraps); signed views are `int32(x)`/`int64(x)` casts. f32/f64 are native `float32`/`float64`, so f32 re-rounding and IEEE float division need no helper (Go floats trap-free, unlike Python/Ruby/Bash).
//! - NaN bit paths go through `math.Float32bits`/`Float64bits`, which are bit-preserving on native floats; only demote/promote reconstruct NaN payloads explicitly.
//! - Control flow maps onto Go's labeled loops: a referenced block/if becomes `L: for { ...; break L }`, a referenced loop `L: for { ...; break L }` with back-edges as `continue L`.
//!   Unreferenced structures are spliced inline.
//!   Unused labels/variables are Go compile errors, so labels are emitted only when referenced and locals/temps only when used (a pre-pass over the body computes the read/used sets, blanking the rest with `_ =`).
//! - `try_table` is the one structure that cannot stay a labeled loop: `recover` works only inside a deferred function, and a labeled `break`/`continue` may not cross a function-literal boundary.
//!   Its body becomes an immediately-invoked closure returning an outcome code, and the `switch` after it performs the branch that the body could not take from inside ([`TryFrame`]).
//! - An artifact carries its consumer's `go vet` run, which rejects unreachable code and self-assignment, so a statement the emitter can already see is dead is not emitted: [`prune`] drops it, together with the frame labels and locals that only it reached.
//!
//! The runtime is composed from per-method units referenced as `Rt.<name>` (methods on a zero-size `rt` receiver), plus package-level constructors (`newMemory`/`newTable`/`newWASI`) and a generic `rtSelect`.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::{
    check_module_support, hex_string, is_ident, is_wasi_module, load_method, module_name_error,
    stmts_use_tail_calls, store_method, type_key, wasi_bundled, Backend, CodeWriter, GenOptions,
    Mode, OutputFile, RuntimeBundler, RuntimeScope, SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, CatchClause, ElemItem, ElemKind, ExportKind, Expr, Func, Label, Module, Stmt,
    Temp, UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The runtime unit bundler for Go (see crates/dewasm-backend-go/units/).
/// Every scope has empty wrappers: Go methods and types are package-level regardless of the struct they belong to, so the bundle is a flat list of declarations (unlike Python's nested classes).
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| {
        RuntimeBundler::new(
            "//",
            "\t",
            // Emit unit bodies exactly as written (they are space-indented, unlike the tabbed base indent).
            0,
            vec![
                RuntimeScope {
                    prefix: "rt",
                    open: "",
                    close: "",
                    prelude: Some("rt/_prelude"),
                },
                RuntimeScope {
                    prefix: "memory",
                    open: "",
                    close: "",
                    prelude: Some("memory/_class"),
                },
                RuntimeScope {
                    prefix: "table",
                    open: "",
                    close: "",
                    prelude: Some("table/_class"),
                },
                RuntimeScope {
                    prefix: "global",
                    open: "",
                    close: "",
                    prelude: Some("global/_class"),
                },
                RuntimeScope {
                    prefix: "wasi",
                    open: "",
                    close: "",
                    prelude: Some("wasi/_class"),
                },
            ],
            UNIT_SOURCES,
        )
        .expect("runtime units are well-formed")
    })
}

/// Locate a `go` toolchain able to compile generated programs: `$DEWASM_GO` first, then `go` on `PATH`.
/// A missing toolchain is a loud failure at the call site, not here.
pub fn find_go() -> Option<std::path::PathBuf> {
    static GO: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    GO.get_or_init(find_go_uncached).clone()
}

/// The probe behind [`find_go`], memoized there: it spawns a process per call, and the toolchain cannot change under a running process.
fn find_go_uncached() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("DEWASM_GO") {
        candidates.push(PathBuf::from(env));
    }
    candidates.push(PathBuf::from("go"));
    candidates.into_iter().find(|candidate| {
        std::process::Command::new(candidate)
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// A complete, compilable Go file bundling *every* runtime unit (with a dummy `main`), for the units lint's `go build` check that all units (not just the subset any one module uses) are valid Go.
pub fn full_bundle_go() -> Result<String> {
    let bundle = bundler().bundle_all(0)?;
    let imports = scan_imports(&bundle, false);
    let mut out = String::from("package main\n\n");
    out.push_str(&import_block(&imports));
    out.push_str("\nfunc main() {}\n\n");
    out.push_str(&bundle);
    out.push('\n');
    Ok(out)
}

pub struct GoBackend;

impl Backend for GoBackend {
    fn name(&self) -> &str {
        "go"
    }

    fn file_extension(&self) -> &str {
        "go"
    }

    fn has_wasi_p1(&self, name: &str) -> bool {
        bundler().has_unit(&format!("wasi/{name}"))
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // Go floats are native IEEE float32/float64, and the NaN paths are bit-exact via `math.Float32bits`/`Float64bits`.
            Feature::Floats => SupportStatus::Supported,
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
            // Tags are identity objects, a thrown exception is a panic carrying the `*rtException` that doubles as the exnref, and traps stay uncatchable.
            Feature::ExceptionHandling => SupportStatus::Supported,
            // A trampoline with a body/entry split: the body returns a thunk alongside its results, typed per result signature since a tail call always agrees with its callee on results.
            Feature::TailCall => SupportStatus::Supported,
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        check_module_support(&GoBackend, module)?;
        let contents = generate_source(module, opts)?;
        let mut files = vec![OutputFile {
            name: format!("{}.go", opts.module_name),
            contents: contents.into_bytes(),
        }];
        // The data file: every segment's bytes concatenated in segment order, matching the `data_offsets` baked into the generated `dataBlob[o:o+len]` slices and `//go:embed`ed by the generated file.
        // Only emitted when there is data to externalize.
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

/// Emit just the package-level declarations for `module` (the struct, its constructor and methods, the spec-harness `invoke`/`globalGet` dispatch, and the recursion guard), for the spec harness: the harness bundles one shared runtime for every module in a `.wast` file, so per-module output carries no `package`/`import`/`main`.
/// Returns the declarations and the runtime units they reference.
pub fn generate_program_with_units(
    module: &Module,
    type_name: &str,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&GoBackend, module)?;
    let gen = Gen {
        module,
        default_wasi: false,
        type_name: type_name.to_string(),
        uses: RefCell::new(BTreeSet::new()),
        cur_locals: RefCell::new(Vec::new()),
        try_stack: RefCell::new(Vec::new()),
        tail_callers: tail_callers(module),
        cur_tail: RefCell::new(None),
        tail_arg_seq: RefCell::new(0),
        spec: true,
        data_file: None,
        data_offsets: data_offsets(module),
    };
    let mut body = CodeWriter::new("\t");
    gen.emit_program(&mut body);
    Ok((body.finish(), gen.uses.into_inner()))
}

fn generate_source(module: &Module, opts: &GenOptions) -> Result<String> {
    let standalone = opts.mode == Mode::Standalone;
    // Standalone artifacts are `package main` with a fixed internal name: nothing outside the file can refer to them, so the module name has no work to do and the output stays byte-identical whatever it is.
    // Library artifacts *are* named: the package an embedder imports and the type it instantiates both come from the module name, which therefore has to be a Go identifier.
    let (package, type_name) = if standalone {
        (STANDALONE_PACKAGE.to_string(), STANDALONE_TYPE.to_string())
    } else {
        validate_library_module_name(&opts.module_name)?;
        (
            package_name(&opts.module_name),
            type_name(&opts.module_name),
        )
    };
    let gen = Gen {
        module,
        default_wasi: opts.default_wasi,
        type_name: type_name.clone(),
        uses: RefCell::new(BTreeSet::new()),
        cur_locals: RefCell::new(Vec::new()),
        try_stack: RefCell::new(Vec::new()),
        tail_callers: tail_callers(module),
        cur_tail: RefCell::new(None),
        tail_arg_seq: RefCell::new(0),
        spec: false,
        data_file: opts.data_file.as_ref().map(|c| c.data_file_name.clone()),
        data_offsets: data_offsets(module),
    };

    // Into its own writer: the `uses` set must be complete before the runtime bundle is assembled.
    let mut body = CodeWriter::new("\t");
    gen.emit_program(&mut body);

    let wasi = wasi_bundled(module, opts.default_wasi, bundler());
    if standalone {
        // main's recover arm references these runtime types.
        gen.use_unit("rt/trap");
        gen.use_unit("rt/exit");
    } else if wasi {
        // Library-mode WASI output is driven by host glue that instantiates and calls `_start`; that glue needs to catch a `proc_exit` (rtExit) to read the exit code, exactly as the standalone main does.
        // Seed rt/exit so the type is always defined even for a module that never imports proc_exit itself.
        gen.use_unit("rt/exit");
    }
    let uses = gen.uses.borrow().clone();
    let bundle = bundler().bundle(&uses, 0)?;

    let mut imports = scan_imports(&bundle, standalone);
    // The standalone WASI main parses `--dir` flags with `strings`; that code lives in main_func (not the scanned bundle), so add the import here.
    if standalone && wasi && !imports.iter().any(|i| i == "strings") {
        imports.push("strings".to_string());
        imports.sort();
    }
    // Library mode admits host code written into the same package (a second file, or text appended to this one).
    // Go requires all imports before other declarations, so glue appended to *this* file cannot carry its own `import`; the generated file imports `fmt` up front (marked used below) so such glue can print without one.
    if !standalone && !imports.iter().any(|i| i == "fmt") {
        imports.push("fmt".to_string());
        imports.sort();
    }

    let mut out = String::from("// Generated by dewasm. Do not edit.\n");
    out.push_str(&format!("package {package}\n\n"));
    out.push_str(&import_block(&imports));
    out.push('\n');
    // Data externalization: pull the segment bytes from a `//go:embed`ed data file.
    // `embed` is a blank import (the package is used only through the directive, which, unlike a package-qualified selector, the import scanner cannot see), and the directive must sit immediately above its `var` with no intervening blank line.
    // A separate `import` declaration is legal Go and keeps `import_block` untouched.
    if let Some(cfg) = &opts.data_file {
        if !module.datas.is_empty() {
            out.push_str("import _ \"embed\"\n\n");
            out.push_str(&format!(
                "//go:embed {}\nvar dataBlob []byte\n\n",
                cfg.data_file_name
            ));
        }
    }
    out.push_str(&bundle);
    out.push_str("\n\n");
    out.push_str(&body.finish());

    if standalone {
        out.push('\n');
        out.push_str(&main_func(&type_name, wasi));
    } else {
        // Mark the always-present `fmt` import used, so appended glue can rely on it (see above).
        out.push_str("\nvar _ = fmt.Sprint\n");
    }
    Ok(out)
}

/// The external packages a bundle references, plus `os` for a standalone main.
/// Only the runtime bundle (controlled code) is scanned; generated program code emits no package-qualified selectors and data blobs are hex literals, so no user string can inject a false import.
/// Line comments are stripped first: a prose "at instantiation time." must not pull in the `time` package (`//go:` directives survive in the emitted bundle regardless: this stripping only computes the import set).
fn scan_imports(bundle: &str, standalone: bool) -> Vec<String> {
    let candidates = [
        ("binary.", "encoding/binary"),
        ("bits.", "math/bits"),
        ("errors.", "errors"),
        ("filepath.", "path/filepath"),
        ("math.", "math"),
        ("rand.", "crypto/rand"),
        ("reflect.", "reflect"),
        ("runtime.", "runtime"),
        ("os.", "os"),
        ("sort.", "sort"),
        ("strings.", "strings"),
        ("syscall.", "syscall"),
        ("time.", "time"),
        ("unsafe.", "unsafe"),
    ];
    let code: String = bundle
        .lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut set: BTreeSet<&'static str> = BTreeSet::new();
    for (sel, path) in candidates {
        if selector_used(&code, sel) {
            set.insert(path);
        }
    }
    if standalone {
        set.insert("os");
    }
    set.into_iter().map(|s| s.to_string()).collect()
}

/// Whether `text` uses the package selector `sel` (`"time."`, `"os."`, ...), that is, whether it occurs at an identifier boundary.
/// `runtime.GOOS` must not register a use of `time.`, and `p.os.x` must not register one of `os.`, so an occurrence preceded by an identifier character or a dot does not count.
/// Public because the test crate's own scanners (the spec harness and the multi-module e2e composer assemble programs from several fragments and must compute the same import set) would otherwise re-derive this rule and drift from it.
pub fn selector_used(text: &str, sel: &str) -> bool {
    let bytes = text.as_bytes();
    let mut start = 0;
    while let Some(i) = text[start..].find(sel) {
        let idx = start + i;
        if idx == 0 {
            return true;
        }
        let prev = bytes[idx - 1];
        if !(prev.is_ascii_alphanumeric() || prev == b'_' || prev == b'.') {
            return true;
        }
        start = idx + 1;
    }
    false
}

fn import_block(imports: &[String]) -> String {
    if imports.is_empty() {
        return String::new();
    }
    let mut out = String::from("import (\n");
    for path in imports {
        out.push_str(&format!("\t{:?}\n", path));
    }
    out.push_str(")\n");
    out
}

fn main_func(type_name: &str, wasi: bool) -> String {
    // Standalone WASI parses the runtime interface: a leading run of `--dir HOST::GUEST` flags mounts host directories at guest paths (wasmtime-style), stopping at `--` or the first non-flag token; the rest is the guest's argv[1..], with argv[0] the program basename.
    // Without WASI there is nothing to preopen and no argv to deliver.
    let (arg_setup, args_arg, env_arg, preopen_arg) = if wasi {
        (
            "\tpreopens := map[string]string{}\n\
             \trest := os.Args[1:]\n\
             \ti := 0\n\
             \tfor i < len(rest) {\n\
             \t\ta := rest[i]\n\
             \t\tvar spec string\n\
             \t\tif a == \"--\" {\n\
             \t\t\ti++\n\
             \t\t\tbreak\n\
             \t\t} else if a == \"--dir\" {\n\
             \t\t\tif i+1 >= len(rest) {\n\
             \t\t\t\tos.Stderr.WriteString(\"--dir requires a HOST::GUEST argument\\n\")\n\
             \t\t\t\tos.Exit(1)\n\
             \t\t\t}\n\
             \t\t\tspec = rest[i+1]\n\
             \t\t\ti += 2\n\
             \t\t} else if strings.HasPrefix(a, \"--dir=\") {\n\
             \t\t\tspec = a[6:]\n\
             \t\t\ti++\n\
             \t\t} else {\n\
             \t\t\tbreak\n\
             \t\t}\n\
             \t\tif j := strings.Index(spec, \"::\"); j >= 0 {\n\
             \t\t\tpreopens[spec[j+2:]] = spec[:j]\n\
             \t\t} else {\n\
             \t\t\tpreopens[spec] = spec\n\
             \t\t}\n\
             \t}\n\
             \tname := os.Args[0]\n\
             \tif j := strings.LastIndexByte(name, '/'); j >= 0 {\n\
             \t\tname = name[j+1:]\n\
             \t}\n\
             \targv := append([]string{name}, rest[i:]...)\n",
            "argv",
            "os.Environ()",
            "preopens",
        )
    } else {
        ("", "nil", "nil", "nil")
    };
    format!(
        "func main() {{\n\
         {arg_setup}\
         \tp := New{type_name}(nil, {args_arg}, {env_arg}, {preopen_arg})\n\
         \tdefer func() {{\n\
         \t\tif r := recover(); r != nil {{\n\
         \t\t\tswitch e := r.(type) {{\n\
         \t\t\tcase *rtExit:\n\
         \t\t\t\tos.Exit(e.code)\n\
         \t\t\tcase *rtTrap:\n\
         \t\t\t\tos.Stderr.WriteString(\"trap: \" + e.msg + \"\\n\")\n\
         \t\t\t\tos.Exit(134)\n\
         \t\t\tdefault:\n\
         \t\t\t\tpanic(r)\n\
         \t\t\t}}\n\
         \t\t}}\n\
         \t}}()\n\
         \tp.Exports[\"_start\"].(func())()\n\
         }}\n"
    )
}

/// The package and type a standalone artifact always uses: it is a program, not something anyone imports, so its internal names are fixed and its bytes do not depend on the module name.
const STANDALONE_PACKAGE: &str = "main";
const STANDALONE_TYPE: &str = "Program";

/// The grammar a library-mode module name must match: a Go identifier restricted to ASCII.
/// Names are taken as written (there is no sanitization), so a name that cannot be a Go package/type name is a conversion-time error (fail at conversion, never at runtime), not something quietly rewritten into a name the embedder did not ask for.
fn validate_library_module_name(name: &str) -> Result<()> {
    if is_ident(
        name,
        |c| c.is_ascii_alphabetic() || c == '_',
        |c| c.is_ascii_alphanumeric() || c == '_',
    ) {
        Ok(())
    } else {
        Err(module_name_error(
            "go",
            name,
            "a single identifier matching [A-Za-z_][A-Za-z0-9_]* (the artifact declares `package <name lowercased>` and the type `<name capitalized>`)",
        ))
    }
}

/// The package clause of a library artifact.
/// Total on a validated name.
fn package_name(module_name: &str) -> String {
    module_name.to_ascii_lowercase()
}

/// The exported Go type of a library artifact: the module name with its first letter uppercased, leaving the rest exactly as written (`ruby` → `Ruby`, `Rg` → `Rg`).
/// Total on a validated name; a leading `_` stays unexported, which is the embedder's own choice to make.
fn type_name(module_name: &str) -> String {
    let mut chars = module_name.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

/// Go double-quoted string literal.
pub fn go_string(s: &str) -> String {
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
    format!("Rt.unhex(\"{}\")", hex_string(data))
}

/// Prefix sums locating each data segment in the concatenated data-file blob.
/// Only consulted when `--data-file` externalizes the segments.
fn data_offsets(module: &Module) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(module.datas.len());
    let mut acc = 0usize;
    for data in &module.datas {
        offsets.push(acc);
        acc += data.data.len();
    }
    offsets
}

pub use dewasm_backend::WASI_PREVIEW1_FUNCTIONS;

fn go_type(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "uint32",
        ValType::I64 => "uint64",
        ValType::F32 => "float32",
        ValType::F64 => "float64",
        ValType::FuncRef => "*funcref",
        ValType::ExnRef => "*rtException",
    }
}

/// The boxed-global field/assertion type for a value type: `*global[uint32]`.
fn global_field_type(ty: ValType) -> String {
    format!("*global[{}]", go_type(ty))
}

fn ty_suffix(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
        ValType::FuncRef => "fr",
        ValType::ExnRef => "exnref",
    }
}

/// Whether `expr` renders as a Go integer constant, which makes any operation over two of them a compile-time constant expression.
/// Float constants render as calls (`f32_from_bits`), so they are never constant to Go.
fn int_const(expr: &Expr) -> bool {
    matches!(expr, Expr::I32Const(_) | Expr::I64Const(_))
}

/// The suffix naming a tail-call thunk type for a given result signature.
fn tail_suffix(results: &[ValType]) -> String {
    if results.is_empty() {
        return "Void".to_string();
    }
    results
        .iter()
        .map(|t| {
            let s = ty_suffix(*t);
            let mut c = s.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => s.to_string(),
            }
        })
        .collect()
}

/// Defined functions (function index space) containing a tail call: these are the ones split into `f{idx}Body` plus a trampoline entry.
fn tail_callers(module: &Module) -> BTreeSet<u32> {
    module
        .funcs
        .iter()
        .enumerate()
        .filter(|(_, f)| stmts_use_tail_calls(&f.body))
        .map(|(i, _)| module.num_imported_funcs() + i as u32)
        .collect()
}

fn zero_value(ty: ValType) -> &'static str {
    match ty {
        ValType::FuncRef | ValType::ExnRef => "nil",
        _ => "0",
    }
}

fn temp(t: Temp) -> String {
    format!("s{}_{}", t.depth, ty_suffix(t.ty))
}

/// Go result clause for a function's result types: "" / " T" / " (T, U)".
fn go_results(tys: &[ValType]) -> String {
    match tys {
        [] => String::new(),
        [t] => format!(" {}", go_type(*t)),
        ts => format!(
            " ({})",
            ts.iter()
                .map(|t| go_type(*t))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn go_func_type(params: &[ValType], results: &[ValType]) -> String {
    let ps = params
        .iter()
        .map(|t| go_type(*t))
        .collect::<Vec<_>>()
        .join(", ");
    format!("func({}){}", ps, go_results(results))
}

/// The recursion-guard budget (spec-harness builds only): each generated function adds its frame's slot count (`1 + params + locals + temps`) to the global `rtStack` on entry and traps once the running total exceeds this.
/// It bounds otherwise-uncatchable Go stack overflow (a runaway recursion aborts the process fatally) into a catchable "call stack exhausted" trap the spec harness can observe.
/// Sized so every runaway/huge recursion and the one >50-slot function in the testsuite (`function-with-many-locals`, 1056 locals, `skip-stack-guard-page.wast`) trip it, while every legitimately terminating recursion in the suite stays under it.
const SPEC_STACK_LIMIT: usize = 1024;

/// The state of one `try_table` whose body is currently being emitted into a closure.
///
/// Go's `recover` works only inside a deferred function and Go forbids a labeled `break`/`continue` (or a `return` for the enclosing function) crossing a function-literal boundary, so the body becomes an immediately-invoked closure returning an outcome code that the following `switch` acts on.
/// Code 0 is normal completion, `1..=catches` are the catch clauses, and the rest are allocated here, one per branch that has to leave the closure.
struct TryFrame {
    /// The try_table's own label: a branch to it lands exactly where falling off the end of the closure does, so it is outcome 0 rather than an escape.
    label: u32,
    /// Labels opened *inside* the closure; a branch to one of those stays a direct labeled break/continue.
    inner_labels: Vec<u32>,
    catches: usize,
    /// What leaves the closure, in outcome-code order; the enclosing `switch` re-emits each one where the labels are in scope again.
    escapes: Vec<Escape>,
}

/// One way out of a `try_table` closure other than falling off its end.
/// Re-emitting the original statement is sound because an escape is the last thing its closure does: nothing runs between the escape point and the `switch` arm, so the operands still read the same locals and temps.
enum Escape {
    Br(BrTarget),
    /// A tail call, which must run its callee with the closure (and the handler it installed) already gone.
    Tail(Stmt),
}

struct Gen<'a> {
    module: &'a Module,
    default_wasi: bool,
    type_name: String,
    uses: RefCell<BTreeSet<String>>,
    /// Param+local types of the function currently being emitted.
    cur_locals: RefCell<Vec<ValType>>,
    /// The `try_table` closures enclosing the statement being emitted, innermost last.
    try_stack: RefCell<Vec<TryFrame>>,
    /// Defined functions (function index space) containing a tail call: these are the ones split into `f{idx}Body` plus a trampoline entry.
    tail_callers: BTreeSet<u32>,
    /// The result signature of the tail-calling function currently being emitted, set by `function()`: every `return` in its body carries a trailing `nil` thunk.
    cur_tail: RefCell<Option<Vec<ValType>>>,
    /// Names the per-argument bindings a tail call makes; only has to be unique within one function.
    tail_arg_seq: RefCell<u32>,
    /// Spec-harness mode: emit the reflective `invoke`/`global_get` dispatch methods and the recursion guard.
    /// Off for the shipped standalone/library output, whose deep-but-valid recursions must not falsely trap.
    spec: bool,
    /// When `Some`, data segments are externalized into a `//go:embed`ed binary data file of this filename instead of embedded as `Rt.unhex` literals; `data_offsets[i]` locates segment `i` in the blob.
    data_file: Option<String>,
    data_offsets: Vec<usize>,
}

impl<'a> Gen<'a> {
    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    /// The distinct result signatures of the module's tail-calling functions, each needing one thunk type.
    fn tail_signatures(&self) -> Vec<Vec<ValType>> {
        let mut seen: BTreeSet<Vec<ValType>> = BTreeSet::new();
        for idx in &self.tail_callers {
            seen.insert(self.module.func_type(*idx).results.clone());
        }
        seen.into_iter().collect()
    }

    /// Evaluate each argument into a fresh local, so the thunk that runs later closes over the values the tail call had, not over variables the trampoline may reuse.
    fn bind_args(&self, w: &mut CodeWriter, args: &[Expr]) -> Vec<String> {
        let mut names = Vec::with_capacity(args.len());
        for arg in args {
            let n = self.next_tail_arg();
            w.line(format!("{n} := {}", self.expr(arg)));
            names.push(n);
        }
        names
    }

    fn next_tail_arg(&self) -> String {
        let mut n = self.tail_arg_seq.borrow_mut();
        *n += 1;
        format!("__ta{n}")
    }

    /// Return the thunk that continues the chain, alongside zero values for this frame's own results, which the trampoline discards.
    /// `run` is the closure's whole body: it has the thunk signature, so it either forwards to another body or completes and ends the chain.
    fn emit_thunk(&self, w: &mut CodeWriter, results: &[ValType], run: String) {
        let mut parts: Vec<String> = results.iter().map(|t| zero_value(*t).to_string()).collect();
        parts.push(format!(
            "{}(func(){} {{ {run} }})",
            self.tail_type(results),
            self.body_results(results)
        ));
        w.line(format!("return {}", parts.join(", ")));
    }

    /// A thunk body forwarding to another function's split body: its signature already matches.
    fn forwarding_body(call: &str) -> String {
        format!("return {call}")
    }

    /// A thunk body calling a function that completes in one frame, so the chain ends with a `nil` thunk.
    fn completing_body(results: &[ValType], call: &str) -> String {
        match results.len() {
            0 => format!("{call}; return nil"),
            1 => format!("return {call}, nil"),
            n => {
                let rs: Vec<String> = (0..n).map(|i| format!("__r{i}")).collect();
                format!("{} := {call}; return {}, nil", rs.join(", "), rs.join(", "))
            }
        }
    }

    /// The named type of a tail-call thunk for a given result signature: `func() (results, itself)`.
    /// A type per signature rather than one dynamic wrapper, because a caller and the callee it tail-calls always agree on results (wasm requires it), so the thunk stays statically typed with nothing boxed.
    /// Named after the module type, because the spec harness compiles several converted modules into one Go package.
    fn tail_type(&self, results: &[ValType]) -> String {
        format!("{}Tail{}", self.type_name, tail_suffix(results))
    }

    /// The Go signature of `f{idx}Body`: the function's own results followed by the thunk that continues the chain.
    fn body_results(&self, results: &[ValType]) -> String {
        let mut rets: Vec<String> = results.iter().map(|t| go_type(*t).to_string()).collect();
        rets.push(self.tail_type(results));
        format!(" ({})", rets.join(", "))
    }

    /// The Go expression yielding a data segment's bytes: a sub-slice of the embedded blob when `--data-file` is on (no runtime helper), else an `Rt.unhex(...)` inline hex literal.
    fn data_expr(&self, seg: usize, data: &[u8]) -> String {
        if self.data_file.is_some() {
            let o = self.data_offsets[seg];
            format!("dataBlob[{o}:{}]", o + data.len())
        } else {
            self.use_unit("rt/unhex");
            hex_bytes(data)
        }
    }

    /// Record the runtime unit a value type's Go spelling needs: `exnref` is spelled as the runtime's own `*rtException`, which a module can name without ever throwing or catching (an imported signature, a local, a global).
    fn note_type(&self, ty: ValType) {
        if ty == ValType::ExnRef {
            self.use_unit("rt/exception");
        }
    }

    fn note_types<'t>(&self, tys: impl IntoIterator<Item = &'t ValType>) {
        for ty in tys {
            self.note_type(*ty);
        }
    }

    /// Reference a runtime helper method, recording its unit.
    fn rt(&self, name: &str) -> String {
        self.use_unit(&format!("rt/{name}"));
        format!("Rt.{name}")
    }

    /// Reference a Memory method, recording its unit.
    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
    }

    fn emit_program(&self, w: &mut CodeWriter) {
        self.struct_def(w);
        for sig in self.tail_signatures() {
            let name = self.tail_type(&sig);
            let mut rets: Vec<String> = sig.iter().map(|t| go_type(*t).to_string()).collect();
            rets.push(name.clone());
            w.line("");
            w.line(format!("type {name} func() ({})", rets.join(", ")));
        }
        w.line("");
        self.constructor(w);
        for (i, func) in self.module.funcs.iter().enumerate() {
            w.line("");
            let idx = self.module.num_imported_funcs() as usize + i;
            self.function(w, idx as u32, func);
        }
        if self.spec {
            w.line("");
            self.emit_invoke_method(w);
            w.line("");
            self.emit_global_get_method(w);
        }
    }

    /// The spec-harness reflective dispatcher: `invoke(name, args...) []any` asserts each arg to its wasm param type and boxes every result into `[]any`, mirroring Ruby/Python's dynamic `invoke` under Go's static typing.
    /// The harness compares the boxed results bit-exactly.
    fn emit_invoke_method(&self, w: &mut CodeWriter) {
        w.line(format!(
            "func (p *{}) invoke(name string, args ...any) []any {{",
            self.type_name
        ));
        w.indent();
        w.line("switch name {");
        for export in &self.module.exports {
            let ExportKind::Func(idx) = export.kind else {
                continue;
            };
            let ty = self.module.func_type(idx);
            w.line(format!("case {}:", go_string(&export.name)));
            w.indent();
            let call_args = ty
                .params
                .iter()
                .enumerate()
                .map(|(i, t)| format!("args[{i}].({})", go_type(*t)))
                .collect::<Vec<_>>()
                .join(", ");
            let call = format!("{}({call_args})", self.func_ref(idx));
            match ty.results.len() {
                0 => {
                    w.line(call);
                    w.line("return nil");
                }
                n => {
                    let names = (0..n).map(|i| format!("r{i}")).collect::<Vec<_>>();
                    w.line(format!("{} := {call}", names.join(", ")));
                    w.line(format!("return []any{{{}}}", names.join(", ")));
                }
            }
            w.dedent();
        }
        w.line("default:");
        w.indent();
        w.line("panic(\"no export \" + name)");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    }

    /// The spec-harness global reader: returns the boxed global's current value boxed in a one-element `[]any`, so the harness treats it exactly like a single-result `invoke` (`__r[0]`).
    fn emit_global_get_method(&self, w: &mut CodeWriter) {
        w.line(format!(
            "func (p *{}) globalGet(name string) []any {{",
            self.type_name
        ));
        w.indent();
        w.line("switch name {");
        for export in &self.module.exports {
            let ExportKind::Global(idx) = export.kind else {
                continue;
            };
            w.line(format!("case {}:", go_string(&export.name)));
            w.indent();
            w.line(format!("return []any{{p.g{idx}.value}}"));
            w.dedent();
        }
        w.line("default:");
        w.indent();
        w.line("panic(\"no global \" + name)");
        w.dedent();
        w.line("}");
        w.dedent();
        w.line("}");
    }

    fn struct_def(&self, w: &mut CodeWriter) {
        let m = self.module;
        w.line(format!("type {} struct {{", self.type_name));
        w.indent();
        if m.imported_memory.is_some() || m.memory.is_some() {
            w.line("memory *Memory");
        }
        // Table index space = imported_tables ++ tables.
        let num_tables = m.imported_tables.len() + m.tables.len();
        for i in 0..num_tables {
            w.line(format!("t{i} *Table"));
        }
        // Global index space = imported_globals ++ globals; every global is a boxed *global[T].
        for (i, imp) in m.imported_globals.iter().enumerate() {
            self.note_type(imp.ty);
            w.line(format!("g{i} {}", global_field_type(imp.ty)));
        }
        let num_imported_globals = m.imported_globals.len();
        for (i, g) in m.globals.iter().enumerate() {
            self.note_type(g.ty);
            w.line(format!(
                "g{} {}",
                num_imported_globals + i,
                global_field_type(g.ty)
            ));
        }
        // Tag index space = imported_tags ++ tags; a tag is an identity object, so the field holds the shared pointer whether the tag is defined here or imported.
        for i in 0..m.imported_tags.len() + m.tags.len() {
            self.use_unit("rt/tag");
            w.line(format!("tag{i} *rtTag"));
        }
        for (i, imp) in m.imported_funcs.iter().enumerate() {
            let ty = &m.types[imp.type_idx as usize];
            self.note_types(ty.params.iter().chain(&ty.results));
            w.line(format!("if{i} {}", go_func_type(&ty.params, &ty.results)));
        }
        if wasi_bundled(m, self.default_wasi, bundler()) {
            // The bundled WASI is built on first fallback, not in the ctor, so the ctor arguments are kept for `wasiInstance` to use.
            w.line("wasi *WASI");
            w.line("wasiArgs []string");
            w.line("wasiEnv []string");
            w.line("wasiPreopens map[string]string");
        }
        for i in 0..m.elems.len() {
            w.line(format!("elem{i} []*funcref"));
        }
        for i in 0..m.datas.len() {
            w.line(format!("data{i} []byte"));
        }
        w.line("Exports map[string]any");
        w.dedent();
        w.line("}");
    }

    fn constructor(&self, w: &mut CodeWriter) {
        let m = self.module;
        let name = &self.type_name;
        w.line(format!(
            "func New{name}(imports Imports, args []string, env []string, preopens map[string]string) *{name} {{"
        ));
        w.indent();
        w.line(format!("p := &{name}{{}}"));

        if let Some(import) = &m.imported_memory {
            self.emit_typed_import(w, "p.memory", "*Memory", &import.module, &import.name);
        } else if let Some(mem) = &m.memory {
            self.use_unit("memory/_class");
            let max = mem.max_pages.map(|p| p as u32).unwrap_or(65536);
            w.line(format!(
                "p.memory = newMemory({}, {})",
                mem.min_pages as u32, max
            ));
        }
        // Tables: imported first, then defined (index space is imported_tables ++ tables).
        for (i, import) in m.imported_tables.iter().enumerate() {
            self.emit_typed_import(
                w,
                &format!("p.t{i}"),
                "*Table",
                &import.module,
                &import.name,
            );
        }
        let num_imported_tables = m.imported_tables.len();
        for (i, table) in m.tables.iter().enumerate() {
            self.use_unit("table/_class");
            w.line(format!(
                "p.t{} = newTable({})",
                num_imported_tables + i,
                table.min
            ));
        }

        let wasi = wasi_bundled(m, self.default_wasi, bundler());
        let has_imports = !m.imported_funcs.is_empty()
            || !m.imported_globals.is_empty()
            || !m.imported_tables.is_empty()
            || !m.imported_tags.is_empty()
            || m.imported_memory.is_some();
        if wasi {
            self.use_unit("wasi/_class");
            // Kept for `wasiInstance`, which builds the bundled WASI the first time an import falls back to it: an embedder covering every WASI import never pays for one.
            w.line("p.wasiArgs = args");
            w.line("p.wasiEnv = env");
            w.line("p.wasiPreopens = preopens");
        } else {
            // args/env/preopens unused when no WASI is bundled.
            w.line("_ = args");
            w.line("_ = env");
            w.line("_ = preopens");
        }
        if !has_imports {
            w.line("_ = imports");
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            self.emit_import(w, i, import);
        }

        // Globals: imported first, then defined; every global is a boxed *global[T].
        // Defined globals' init exprs may read imported globals, so they must resolve after the imported ones.
        for (i, import) in m.imported_globals.iter().enumerate() {
            self.emit_typed_import(
                w,
                &format!("p.g{i}"),
                &global_field_type(import.ty),
                &import.module,
                &import.name,
            );
        }
        let num_imported_globals = m.imported_globals.len();
        for (i, global) in m.globals.iter().enumerate() {
            self.use_unit("global/_class");
            w.line(format!(
                "p.g{} = newGlobal({})",
                num_imported_globals + i,
                self.expr(&global.init)
            ));
        }

        // Tags: imported first, then defined (index space is imported_tags ++ tags).
        // A defined tag is a fresh identity object; nothing about it is derived from its type, since wasm tag equality is identity and never structure.
        for (i, import) in m.imported_tags.iter().enumerate() {
            self.use_unit("rt/tag");
            self.emit_typed_import(
                w,
                &format!("p.tag{i}"),
                "*rtTag",
                &import.module,
                &import.name,
            );
        }
        for i in 0..m.tags.len() {
            self.use_unit("rt/tag");
            w.line(format!("p.tag{} = &rtTag{{}}", m.imported_tags.len() + i));
        }

        for (i, elem) in m.elems.iter().enumerate() {
            let items = || {
                elem.items
                    .iter()
                    .map(|item| self.elem_item(item))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            match &elem.kind {
                ElemKind::Declared => w.line(format!("p.elem{i} = nil")),
                ElemKind::Passive => w.line(format!("p.elem{i} = []*funcref{{{}}}", items())),
                ElemKind::Active {
                    table_index,
                    offset,
                } => {
                    self.use_unit("table/init");
                    w.line(format!("_elem{i} := []*funcref{{{}}}", items()));
                    w.line(format!(
                        "p.t{table_index}.init({}, _elem{i}, 0, {})",
                        self.expr(offset),
                        elem.items.len()
                    ));
                    // Active segments are dropped after instantiation.
                    w.line(format!("p.elem{i} = nil"));
                }
            }
        }

        for (i, data) in m.datas.iter().enumerate() {
            match &data.offset {
                Some(offset) => {
                    self.use_unit("memory/init");
                    w.line(format!(
                        "p.memory.init(uint64({}), {}, 0, {})",
                        self.expr(offset),
                        self.data_expr(i, &data.data),
                        data.data.len()
                    ));
                }
                None => {
                    w.line(format!("p.data{i} = {}", self.data_expr(i, &data.data)));
                }
            }
        }

        // Exports map holds every export kind as `any` so a generated instance doubles as another module's import provider (`imports["M"] = inst.Exports`), the mechanism the spec harness's `register` support uses.
        // Globals export the shared box, not its value.
        let mut export_entries = Vec::new();
        for export in &m.exports {
            let val = match export.kind {
                ExportKind::Func(idx) => self.func_ref(idx),
                ExportKind::Global(idx) => format!("p.g{idx}"),
                ExportKind::Table(idx) => format!("p.t{idx}"),
                ExportKind::Memory => "p.memory".to_string(),
                ExportKind::Tag(idx) => format!("p.tag{idx}"),
            };
            export_entries.push(format!("{}: {}", go_string(&export.name), val));
        }
        if export_entries.is_empty() {
            w.line("p.Exports = map[string]any{}");
        } else {
            w.line(format!(
                "p.Exports = map[string]any{{{}}}",
                export_entries.join(", ")
            ));
        }

        // Let import providers bind to the fully-constructed instance (the optional `Attach` half of the provider protocol).
        if has_imports {
            w.line("for _, s := range imports {");
            w.indent();
            w.line("if a, ok := s.(ImportAttacher); ok {");
            w.indent();
            w.line("a.Attach(p)");
            w.dedent();
            w.line("}");
            w.dedent();
            w.line("}");
        }

        if let Some(start) = m.start {
            w.line(self.call_string(start, &[]));
        }

        w.line("return p");
        w.dedent();
        w.line("}");

        if wasi {
            w.line("");
            self.wasi_accessor(w);
        }
    }

    /// The bundled WASI, built on first use.
    /// Nothing constructs it in the ctor: an embedder whose provider covers every WASI import gets no WASI at all (which is exactly what `p.wasi == nil` says) while the first import that falls back builds it, with the memory already bound (the ctor resolves memory before any import).
    fn wasi_accessor(&self, w: &mut CodeWriter) {
        let m = self.module;
        w.line(format!(
            "func (p *{}) wasiInstance() *WASI {{",
            self.type_name
        ));
        w.indent();
        w.line("if p.wasi == nil {");
        w.indent();
        w.line("p.wasi = newWASI(p.wasiArgs, p.wasiEnv, p.wasiPreopens)");
        if m.memory.is_some() || m.imported_memory.is_some() {
            w.line("p.wasi.memory = p.memory");
        }
        w.dedent();
        w.line("}");
        w.line("return p.wasi");
        w.dedent();
        w.line("}");
    }

    fn emit_import(&self, w: &mut CodeWriter, i: usize, import: &dewasm_core::ir::ImportedFunc) {
        let m = self.module;
        let ty = &m.types[import.type_idx as usize];
        let go_ft = go_func_type(&ty.params, &ty.results);

        let fallback = if is_wasi_module(&import.module) && self.default_wasi {
            let unit = format!("wasi/{}", import.name);
            if bundler().has_unit(&unit) {
                self.use_unit(&unit);
                // A method value on the lazily built WASI: taking it here is what constructs the bundled WASI, so it exists exactly when some import fell back to it (mirroring Ruby's `@wasi ||=`).
                Some(format!("p.wasiInstance().wasi_{}", import.name))
            } else {
                Some(self.enosys_stub(ty))
            }
        } else {
            None
        };

        w.line(format!(
            "if v := {}; v != nil {{",
            self.resolve_import_string(&import.module, &import.name)
        ));
        w.indent();
        w.line(format!("f, ok := v.({go_ft})"));
        w.line("if !ok {");
        w.indent();
        w.line(format!(
            "{}({})",
            self.rt("link_error"),
            go_string(&format!(
                "incompatible import type for {}.{}",
                import.module, import.name
            ))
        ));
        w.dedent();
        w.line("}");
        w.line(format!("p.if{i} = f"));
        w.dedent();
        w.line("} else {");
        w.indent();
        match fallback {
            Some(f) => w.line(format!("p.if{i} = {f}")),
            // A missing non-WASI import is a link error at instantiation, not a deferred call-time failure (mirrors Ruby/Python).
            None => w.line(format!(
                "{}({})",
                self.rt("link_error"),
                go_string(&format!("missing import {}.{}", import.module, import.name))
            )),
        }
        w.dedent();
        w.line("}");
    }

    /// Resolve a non-function import (memory/table/global) into `target`, asserting it to `go_ty` (`*Memory`/`*Table`/`*global[T]`).
    /// A present wrong-kind (or wrong-type) value is a link error; a missing one is a link error too (there is no fallback for these, unlike WASI funcs).
    /// The Go type assertion performs the kind check (and, for globals, the value-type check) inherently; mutability and min/max limits stay unchecked (the import-limits gap).
    fn emit_typed_import(
        &self,
        w: &mut CodeWriter,
        target: &str,
        go_ty: &str,
        module: &str,
        name: &str,
    ) {
        w.line(format!(
            "if v := {}; v != nil {{",
            self.resolve_import_string(module, name)
        ));
        w.indent();
        w.line(format!("x, ok := v.({go_ty})"));
        w.line("if !ok {");
        w.indent();
        w.line(format!(
            "{}({})",
            self.rt("link_error"),
            go_string(&format!("incompatible import type for {module}.{name}"))
        ));
        w.dedent();
        w.line("}");
        w.line(format!("{target} = x"));
        w.dedent();
        w.line("} else {");
        w.indent();
        w.line(format!(
            "{}({})",
            self.rt("link_error"),
            go_string(&format!("missing import {module}.{name}"))
        ));
        w.dedent();
        w.line("}");
    }

    fn resolve_import_string(&self, module: &str, name: &str) -> String {
        self.use_unit("rt/resolve_import");
        format!(
            "Rt.resolve_import(imports, {}, {})",
            go_string(module),
            go_string(name)
        )
    }

    /// An ENOSYS stub for an unimplemented WASI import: single-i32-result syscalls return errno 52, everything else returns zero values.
    fn enosys_stub(&self, ty: &dewasm_core::ir::FuncType) -> String {
        let params = ty
            .params
            .iter()
            .map(|t| go_type(*t))
            .collect::<Vec<_>>()
            .join(", ");
        if ty.results == [ValType::I32] {
            format!("func({params}) uint32 {{ return 52 }}")
        } else {
            format!(
                "func({params}){} {{{} }}",
                go_results(&ty.results),
                self.return_zeros(&ty.results)
            )
        }
    }

    fn return_zeros(&self, results: &[ValType]) -> String {
        if results.is_empty() {
            String::new()
        } else {
            let zeros = results
                .iter()
                .map(|t| zero_value(*t))
                .collect::<Vec<_>>()
                .join(", ");
            format!(" return {zeros}")
        }
    }

    /// A funcref value for a table slot / element.
    fn elem_item(&self, item: &ElemItem) -> String {
        match item {
            ElemItem::Func(func_idx) => {
                let body = if self.tail_callers.contains(func_idx) {
                    format!(", body: p.f{func_idx}Body")
                } else {
                    String::new()
                };
                format!(
                    "&funcref{{ty: {}, fn: {}{body}}}",
                    go_string(&self.func_type_symbol(*func_idx)),
                    self.func_ref(*func_idx)
                )
            }
            ElemItem::Null => "nil".to_string(),
            // A `global.get` element item needs a ref-typed immutable global, i.e. reference types (rejected at conversion); unreachable here.
            ElemItem::Global(idx) => format!("p.g{idx}.value"),
        }
    }

    fn func_ref(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("p.if{func_idx}")
        } else {
            format!("p.f{func_idx}")
        }
    }

    fn call_string(&self, func_idx: u32, args: &[String]) -> String {
        format!("{}({})", self.func_ref(func_idx), args.join(", "))
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

        let mut local_types = ty.params.clone();
        local_types.extend(func.locals.iter().copied());
        *self.cur_locals.borrow_mut() = local_types.clone();
        self.note_types(local_types.iter().chain(&ty.results));
        self.note_types(func.temps.iter().map(|t| &t.ty));

        let params_str = ty
            .params
            .iter()
            .enumerate()
            .map(|(i, t)| format!("l{i} {}", go_type(*t)))
            .collect::<Vec<_>>()
            .join(", ");
        // A tail-calling function's real code lives in `f{idx}Body`, which returns a thunk alongside its results instead of growing the stack; the public `f{idx}` is the trampoline that runs the chain, so no call site changes.
        let is_tail_caller = self.tail_callers.contains(&idx);
        if is_tail_caller {
            let args = (0..nparams)
                .map(|i| format!("l{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            let rs: Vec<String> = (0..ty.results.len()).map(|i| format!("r{i}")).collect();
            let mut lhs = rs.clone();
            lhs.push("next".to_string());
            w.line(format!(
                "func (p *{}) f{idx}({params_str}){} {{",
                self.type_name,
                go_results(&ty.results)
            ));
            w.indent();
            w.line(format!("{} := p.f{idx}Body({args})", lhs.join(", ")));
            w.line("for next != nil {");
            w.indent();
            w.line(format!("{} = next()", lhs.join(", ")));
            w.dedent();
            w.line("}");
            if !rs.is_empty() {
                w.line(format!("return {}", rs.join(", ")));
            }
            w.dedent();
            w.line("}");
            w.line("");
        }
        *self.cur_tail.borrow_mut() = is_tail_caller.then(|| ty.results.clone());
        let name = if is_tail_caller {
            format!("f{idx}Body")
        } else {
            format!("f{idx}")
        };
        let results_str = if is_tail_caller {
            self.body_results(&ty.results)
        } else {
            go_results(&ty.results)
        };
        w.line(format!(
            "func (p *{}) {name}({params_str}){results_str} {{",
            self.type_name
        ));
        w.indent();

        if self.spec {
            // Recursion guard (spec-harness only): bound otherwise-fatal Go stack overflow into a catchable "call stack exhausted" trap.
            // The defer is registered before the check so the trapping frame's own cost is unwound too.
            let cost = 1 + local_types.len() + func.temps.len();
            w.line(format!("rtStack += {cost}"));
            w.line(format!("defer func() {{ rtStack -= {cost} }}()"));
            w.line(format!(
                "if rtStack > {SPEC_STACK_LIMIT} {{ {}(\"call stack exhausted\") }}",
                self.rt("trap")
            ));
        }

        let body = prune(&func.body);

        let mut read_locals = BTreeSet::new();
        let mut used_locals = BTreeSet::new();
        let mut read_temps = BTreeSet::new();
        collect_reads_seq(&body, &mut read_locals, &mut used_locals, &mut read_temps);

        for (i, ty) in local_types.iter().enumerate().skip(nparams) {
            let idx = i as u32;
            if used_locals.contains(&idx) {
                w.line(format!("var l{i} {}", go_type(*ty)));
                if !read_locals.contains(&idx) {
                    w.line(format!("_ = l{i}"));
                }
            }
        }
        for t in &func.temps {
            let name = temp(*t);
            w.line(format!("var {name} {}", go_type(t.ty)));
            if !read_temps.contains(&name) {
                w.line(format!("_ = {name}"));
            }
        }

        self.emit_seq(w, &body);

        if (!ty.results.is_empty() || is_tail_caller) && !ends_unreachable(&body) {
            // Go's missing-return rule is syntactic: a body whose last statement is a call that only panics inside the runtime (`Rt.trap`) still needs this.
            let mut zeros: Vec<&str> = ty.results.iter().map(|t| zero_value(*t)).collect();
            if is_tail_caller {
                zeros.push("nil");
            }
            w.line(format!("return {}", zeros.join(", ")));
        }
        w.dedent();
        w.line("}");
    }

    fn emit_seq(&self, w: &mut CodeWriter, stmts: &[Stmt]) {
        for stmt in stmts {
            self.emit_stmt(w, stmt);
        }
    }

    fn emit_stmt(&self, w: &mut CodeWriter, stmt: &Stmt) {
        match stmt {
            Stmt::Block { label, body } => {
                if label.referenced {
                    self.open_label(label.id);
                    w.line(format!("L{}:", label.id));
                    w.line("for {");
                    w.indent();
                    self.emit_seq(w, body);
                    if !ends_unreachable(body) {
                        w.line(format!("break L{}", label.id));
                    }
                    w.dedent();
                    w.line("}");
                    self.close_label();
                } else {
                    self.emit_seq(w, body);
                }
            }
            Stmt::Loop { label, body } => {
                if label.referenced {
                    self.open_label(label.id);
                    w.line(format!("L{}:", label.id));
                    w.line("for {");
                    w.indent();
                    self.emit_seq(w, body);
                    if !ends_unreachable(body) {
                        w.line(format!("break L{}", label.id));
                    }
                    w.dedent();
                    w.line("}");
                    self.close_label();
                } else {
                    // No back-edge, so the loop body runs exactly once.
                    self.emit_seq(w, body);
                }
            }
            Stmt::If {
                label,
                cond,
                then,
                els,
            } => {
                if label.referenced {
                    self.open_label(label.id);
                    w.line(format!("L{}:", label.id));
                    w.line("for {");
                    w.indent();
                    self.emit_if(w, cond, then, els);
                    if !if_ends_unreachable(then, els) {
                        w.line(format!("break L{}", label.id));
                    }
                    w.dedent();
                    w.line("}");
                    self.close_label();
                } else {
                    self.emit_if(w, cond, then, els);
                }
            }
            Stmt::TryTable {
                label,
                catches,
                body,
            } => self.emit_try_table(w, label.id, catches, body),
            Stmt::SourceLine(pos) => {
                // Go honors `//line` directives only at column 1, so bypass the writer's indentation with `raw`.
                // The directive sets the source position of the *following* line, which is exactly the statement this marker precedes.
                let file = &self.module.debug_files[pos.file as usize];
                if pos.col > 0 {
                    w.raw(format!("//line {file}:{}:{}\n", pos.line, pos.col));
                } else {
                    w.raw(format!("//line {file}:{}\n", pos.line));
                }
            }
            // Every other statement is a leaf; `simple_stmt` matches all of them exhaustively, so a new variant is a compile error there rather than silent output.
            _ => self.simple_stmt(w, stmt),
        }
    }

    /// Record that `label` is in scope for the statements about to be emitted, so a branch to it stays a direct labeled break/continue rather than an escape out of the enclosing `try_table` closure.
    fn open_label(&self, label: u32) {
        if let Some(frame) = self.try_stack.borrow_mut().last_mut() {
            frame.inner_labels.push(label);
        }
    }

    fn close_label(&self) {
        if let Some(frame) = self.try_stack.borrow_mut().last_mut() {
            frame.inner_labels.pop();
        }
    }

    /// `try_table`: the body as an immediately-invoked closure whose deferred handler dispatches the catch clauses, followed by a `switch` on the outcome code (see [`TryFrame`]).
    /// The try_table's own label needs no Go label of its own: a branch to it is outcome 0, which lands where falling off the end of the closure does.
    fn emit_try_table(
        &self,
        w: &mut CodeWriter,
        label: u32,
        catches: &[CatchClause],
        body: &[Stmt],
    ) {
        self.use_unit("rt/exception");
        self.try_stack.borrow_mut().push(TryFrame {
            label,
            inner_labels: Vec::new(),
            catches: catches.len(),
            escapes: Vec::new(),
        });

        w.line(format!("__o{label} := func() (__c{label} int) {{"));
        w.indent();
        self.emit_catch_handler(w, label, catches);
        self.emit_seq(w, body);
        if !ends_unreachable(body) {
            w.line("return 0");
        }
        w.dedent();
        w.line("}()");

        let frame = self
            .try_stack
            .borrow_mut()
            .pop()
            .expect("the frame pushed above");
        w.line(format!("switch __o{label} {{"));
        for (i, clause) in catches.iter().enumerate() {
            w.line(format!("case {}:", i + 1));
            w.indent();
            self.branch(w, &clause.target);
            w.dedent();
        }
        for (i, escape) in frame.escapes.iter().enumerate() {
            w.line(format!("case {}:", catches.len() + i + 1));
            w.indent();
            match escape {
                Escape::Br(target) => self.branch(w, target),
                Escape::Tail(stmt) => self.emit_stmt(w, stmt),
            }
            w.dedent();
        }
        w.line("}");
    }

    /// The deferred handler: clauses are checked in order (first match wins), a matched one writes the payload into the target frame's slots and sets the outcome code, and anything else keeps unwinding.
    /// Only `*rtException` is caught, so a trap, a `proc_exit`, and a Go runtime panic are structurally uncatchable.
    fn emit_catch_handler(&self, w: &mut CodeWriter, label: u32, catches: &[CatchClause]) {
        w.line("defer func() {");
        w.indent();
        w.line("__r := recover()");
        w.line("if __r == nil {");
        w.indent();
        w.line("return");
        w.dedent();
        w.line("}");
        w.line("__e, __ok := __r.(*rtException)");
        w.line("if !__ok {");
        w.indent();
        w.line("panic(__r)");
        w.dedent();
        w.line("}");
        if !catches
            .iter()
            .any(|c| c.tag.is_some() || !c.value_temps.is_empty())
        {
            w.line("_ = __e");
        }
        for (i, clause) in catches.iter().enumerate() {
            let bind = |w: &mut CodeWriter| {
                for (n, t) in clause.value_temps.iter().enumerate() {
                    if Some(*t) == clause.exn_temp {
                        w.line(format!("{} = __e", temp(*t)));
                    } else {
                        w.line(format!(
                            "{} = __e.values[{n}].({})",
                            temp(*t),
                            go_type(t.ty)
                        ));
                    }
                }
                w.line(format!("__c{label} = {}", i + 1));
                w.line("return");
            };
            match clause.tag {
                // wasm tag equality is object identity, never structure.
                Some(tag) => {
                    w.line(format!("if __e.tag == p.tag{tag} {{"));
                    w.indent();
                    bind(w);
                    w.dedent();
                    w.line("}");
                }
                None => bind(w),
            }
        }
        // A tag-less clause matched and returned, so nothing reaches the rethrow behind it.
        if !catches.last().is_some_and(|c| c.tag.is_none()) {
            w.line("panic(__r)");
        }
        w.dedent();
        w.line("}()");
    }

    fn emit_if(&self, w: &mut CodeWriter, cond: &Expr, then: &[Stmt], els: &[Stmt]) {
        w.line(format!("if {} != 0 {{", self.expr(cond)));
        w.indent();
        self.emit_seq(w, then);
        w.dedent();
        if els.is_empty() {
            w.line("}");
        } else {
            w.line("} else {");
            w.indent();
            self.emit_seq(w, els);
            w.dedent();
            w.line("}");
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
                w.line(format!("p.g{idx}.value = {}", self.expr(expr)));
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                w.line(format!(
                    "p.memory.{}({}, {})",
                    self.mem(store_method(*op)),
                    self.addr(addr, *offset),
                    self.expr(value)
                ));
            }
            Stmt::Br(target) => self.branch(w, target),
            Stmt::BrIf { cond, target } => {
                w.line(format!("if {} != 0 {{", self.expr(cond)));
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
                w.line(format!("switch {} {{", self.expr(index)));
                for (n, target) in targets.iter().enumerate() {
                    w.line(format!("case {n}:"));
                    w.indent();
                    self.branch(w, target);
                    w.dedent();
                }
                w.line("default:");
                w.indent();
                self.branch(w, default);
                w.dedent();
                w.line("}");
            }
            Stmt::Return { values } => self.return_stmt(w, values),
            Stmt::Call {
                func,
                args,
                results,
            } => {
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                w.line(assign_results(results, self.call_string(*func, &args)));
            }
            Stmt::CallIndirect {
                type_idx,
                table_index,
                index,
                args,
                results,
            } => {
                self.use_unit("table/call");
                let ft = &self.module.types[*type_idx as usize];
                self.note_types(ft.params.iter().chain(&ft.results));
                let go_ft = go_func_type(&ft.params, &ft.results);
                let call_args = args
                    .iter()
                    .map(|a| self.expr(a))
                    .collect::<Vec<_>>()
                    .join(", ");
                let call = format!(
                    "p.t{table_index}.call({}, {}).({go_ft})({call_args})",
                    self.expr(index),
                    go_string(&self.type_symbol(*type_idx)),
                );
                w.line(assign_results(results, call));
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                w.line(format!(
                    "{} = p.memory.grow({})",
                    temp(*dst),
                    self.expr(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "p.memory.copy(uint64({}), uint64({}), uint64({}))",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "p.memory.fill(uint64({}), uint64({}), uint64({}))",
                    self.expr(dst),
                    self.expr(val),
                    self.expr(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "p.memory.init(uint64({}), p.data{seg}, uint64({}), uint64({}))",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::DataDrop { seg } => {
                w.line(format!("p.data{seg} = nil"));
            }
            // A thunk, never a plain call: the callee must run once this frame, including any `try_table` closure that installed a handler, is gone, and returning the thunk is what unwinds them.
            // The thunk targets the callee's *body* where it has one, so a mutual chain runs in the one outermost trampoline with no frame per hop.
            Stmt::ReturnCall { func, args } => {
                if let Some(code) = self.escape_tail(stmt) {
                    w.line(format!("return {code}"));
                    return;
                }
                let callee = self.module.func_type(*func).results.clone();
                let bound = self.bind_args(w, args);
                let run = if self.tail_callers.contains(func) {
                    Self::forwarding_body(&format!("p.f{func}Body({})", bound.join(", ")))
                } else {
                    Self::completing_body(&callee, &self.call_string(*func, &bound))
                };
                self.emit_thunk(w, &callee, run);
            }
            Stmt::ReturnCallIndirect {
                type_idx,
                table_index,
                index,
                args,
            } => {
                if let Some(code) = self.escape_tail(stmt) {
                    w.line(format!("return {code}"));
                    return;
                }
                self.use_unit("table/tail_ref");
                let callee = self.module.types[*type_idx as usize].results.clone();
                let mut bound = self.bind_args(w, std::slice::from_ref(index));
                bound.extend(self.bind_args(w, args));
                let (i, rest) = bound.split_first().expect("the index binding");
                // The slot is resolved, and its traps raised, here rather than inside the thunk: an indirect tail call's checks happen at the instruction, not after the frame is gone.
                let params: Vec<String> = self.module.types[*type_idx as usize]
                    .params
                    .iter()
                    .map(|t| go_type(*t).to_string())
                    .collect();
                let body_sig = format!("func({}){}", params.join(", "), self.body_results(&callee));
                let plain_sig = format!("func({}){}", params.join(", "), go_results(&callee));
                w.line(format!(
                    "__tf := p.t{table_index}.tailRef({i}, {})",
                    go_string(&self.type_symbol(*type_idx))
                ));
                w.line(format!("if __tb, __ok := __tf.({body_sig}); __ok {{"));
                w.indent();
                self.emit_thunk(
                    w,
                    &callee,
                    Self::forwarding_body(&format!("__tb({})", rest.join(", "))),
                );
                w.dedent();
                w.line("}");
                self.emit_thunk(
                    w,
                    &callee,
                    Self::completing_body(
                        &callee,
                        &format!("__tf.({plain_sig})({})", rest.join(", ")),
                    ),
                );
            }
            Stmt::Unreachable => {
                w.line(format!("{}(\"unreachable\")", self.rt("trap")));
            }
            Stmt::Throw { tag, args } => {
                self.use_unit("rt/exception");
                let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                w.line(format!(
                    "panic(&rtException{{tag: p.tag{tag}, values: []any{{{}}}}})",
                    args.join(", ")
                ));
            }
            Stmt::ThrowRef { exn } => {
                w.line(format!("{}({})", self.rt("throw_ref"), self.expr(exn)));
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
                    "p.t{table_index}.init({}, p.elem{seg}, {}, {})",
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
                    "p.t{dst_table}.copy({}, p.t{src_table}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::ElemDrop { seg } => {
                w.line(format!("p.elem{seg} = nil"));
            }
            Stmt::Block { .. }
            | Stmt::Loop { .. }
            | Stmt::If { .. }
            | Stmt::TryTable { .. }
            | Stmt::SourceLine(_) => {
                unreachable!("structured statement routed to simple_stmt")
            }
        }
    }

    /// The outcome code a branch to `target` returns out of the innermost `try_table` closure, one per branch site, or `None` when the branch does not leave a closure.
    /// A branch to a label opened inside the closure, and one to the try_table's own frame, both stay where they are.
    fn escape_code(&self, target: &BrTarget) -> Option<usize> {
        let mut stack = self.try_stack.borrow_mut();
        let frame = stack.last_mut()?;
        if let BrTarget::Label { label, .. } = target {
            if *label == frame.label || frame.inner_labels.contains(label) {
                return None;
            }
        }
        frame.escapes.push(Escape::Br(target.clone()));
        Some(frame.catches + frame.escapes.len())
    }

    /// The same for a `return`, which always leaves the closure (the closure's own `return` is the outcome code).
    fn escape_return(&self, values: &[Expr]) -> Option<usize> {
        let mut stack = self.try_stack.borrow_mut();
        let frame = stack.last_mut()?;
        frame.escapes.push(Escape::Br(BrTarget::Return {
            values: values.to_vec(),
        }));
        Some(frame.catches + frame.escapes.len())
    }

    /// The same for a tail call, whose callee must run with every enclosing handler already gone.
    fn escape_tail(&self, stmt: &Stmt) -> Option<usize> {
        let mut stack = self.try_stack.borrow_mut();
        let frame = stack.last_mut()?;
        frame.escapes.push(Escape::Tail(stmt.clone()));
        Some(frame.catches + frame.escapes.len())
    }

    /// Whether `label` is the innermost enclosing `try_table`'s own frame, whose landing point is the closure's normal return.
    fn is_enclosing_try(&self, label: u32) -> bool {
        matches!(self.try_stack.borrow().last(), Some(f) if f.label == label)
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        if let Some(code) = self.escape_return(values) {
            w.line(format!("return {code}"));
            return;
        }
        // Inside a tail-calling function's body the thunk is part of the signature, and a plain return ends the chain.
        let tail = self.cur_tail.borrow().is_some();
        let mut vs: Vec<String> = values.iter().map(|v| self.expr(v)).collect();
        if tail {
            vs.push("nil".to_string());
        }
        match vs.as_slice() {
            [] => w.line("return"),
            vs => w.line(format!("return {}", vs.join(", "))),
        }
    }

    fn branch(&self, w: &mut CodeWriter, target: &BrTarget) {
        match target {
            BrTarget::Return { values } => self.return_stmt(w, values),
            BrTarget::Label {
                label,
                is_loop,
                assigns,
            } => {
                if let Some(code) = self.escape_code(target) {
                    w.line(format!("return {code}"));
                    return;
                }
                for (dst, src) in assigns {
                    w.line(format!("{} = {}", temp(*dst), temp(*src)));
                }
                if self.is_enclosing_try(*label) {
                    w.line("return 0");
                } else if *is_loop {
                    w.line(format!("continue L{label}"));
                } else {
                    w.line(format!("break L{label}"));
                }
            }
        }
    }

    fn addr(&self, addr: &Expr, offset: u64) -> String {
        if offset == 0 {
            format!("uint64({})", self.expr(addr))
        } else {
            format!("uint64({}) + {offset}", self.expr(addr))
        }
    }

    /// The same constant, always laundered through the identity call, so the expression holding it is not a Go constant expression.
    fn laundered_const(&self, expr: &Expr) -> String {
        match expr {
            Expr::I32Const(v) => format!("{}(0x{v:x})", self.rt("i32c")),
            Expr::I64Const(v) => format!("{}(0x{v:x})", self.rt("i64c")),
            other => self.expr(other),
        }
    }

    fn expr(&self, expr: &Expr) -> String {
        match expr {
            // A folded constant may land directly inside a signed cast (int32/int64); Go rejects that conversion for a compile-time constant beyond the signed range, so launder large values through a call to keep the conversion a runtime one.
            Expr::I32Const(v) => {
                if *v > i32::MAX as u32 {
                    format!("{}(0x{v:x})", self.rt("i32c"))
                } else {
                    format!("uint32({v})")
                }
            }
            // An i64 constant is cast both to int64 (signed views) and, via i32.wrap_i64, to uint32; either conversion rejects a compile-time constant beyond its range, so launder anything above u32::MAX (the wrap target, a superset of the int64 overflow threshold).
            Expr::I64Const(v) => {
                if *v > u32::MAX as u64 {
                    format!("{}(0x{v:x})", self.rt("i64c"))
                } else {
                    format!("uint64({v})")
                }
            }
            Expr::F32Const(bits) => format!("{}(0x{bits:x})", self.rt("f32_from_bits")),
            Expr::F64Const(bits) => format!("{}(0x{bits:x})", self.rt("f64_from_bits")),
            Expr::Temp(t) => temp(*t),
            Expr::LocalGet(idx) => format!("l{idx}"),
            Expr::GlobalGet(idx) => format!("p.g{idx}.value"),
            Expr::Un(op, a) => self.un(*op, &self.expr(a)),
            // Go computes an operation between two constants at arbitrary precision and rejects a result outside the type, where wasm wraps; laundering one operand makes the operation a runtime one, which wraps.
            // Applied to every operator rather than the ones known to overflow today (`+`, `-`, `*`, `<<`), because the rule is about the operands being constants at all, and `bin` is free to lower an operator into arithmetic that overflows where the operator itself would not.
            Expr::Bin(op, a, b) if int_const(a) && int_const(b) => {
                self.bin(*op, &self.laundered_const(a), &self.expr(b))
            }
            Expr::Bin(op, a, b) => self.bin(*op, &self.expr(a), &self.expr(b)),
            Expr::Load { op, addr, offset } => {
                format!(
                    "p.memory.{}({})",
                    self.mem(load_method(*op)),
                    self.addr(addr, *offset)
                )
            }
            Expr::Select { cond, then, els } => {
                self.use_unit("rt/select");
                format!(
                    "rtSelect({}, {}, {})",
                    self.expr(cond),
                    self.expr(then),
                    self.expr(els)
                )
            }
            Expr::MemorySize => {
                self.use_unit("memory/size");
                "p.memory.size()".to_string()
            }
        }
    }

    fn un(&self, op: UnOp, a: &str) -> String {
        use UnOp::*;
        match op {
            I32Eqz | I64Eqz => format!("{}({a} == 0)", self.rt("b2i")),
            I32Clz => format!("{}({a})", self.rt("i32_clz")),
            I32Ctz => format!("{}({a})", self.rt("i32_ctz")),
            I32Popcnt => format!("{}({a})", self.rt("i32_popcnt")),
            I64Clz => format!("{}({a})", self.rt("i64_clz")),
            I64Ctz => format!("{}({a})", self.rt("i64_ctz")),
            I64Popcnt => format!("{}({a})", self.rt("i64_popcnt")),
            F32Abs => format!("{}({a})", self.rt("f32_abs")),
            F32Neg => format!("{}({a})", self.rt("f32_neg")),
            F64Abs => format!("{}({a})", self.rt("f64_abs")),
            F64Neg => format!("{}({a})", self.rt("f64_neg")),
            F32Ceil => format!("{}({a})", self.rt("f32_ceil")),
            F32Floor => format!("{}({a})", self.rt("f32_floor")),
            F32Trunc => format!("{}({a})", self.rt("f32_trunc")),
            F32Nearest => format!("{}({a})", self.rt("f32_nearest")),
            F32Sqrt => format!("{}({a})", self.rt("f32_sqrt")),
            F64Ceil => format!("{}({a})", self.rt("f64_ceil")),
            F64Floor => format!("{}({a})", self.rt("f64_floor")),
            F64Trunc => format!("{}({a})", self.rt("f64_trunc")),
            F64Nearest => format!("{}({a})", self.rt("f64_nearest")),
            F64Sqrt => format!("{}({a})", self.rt("f64_sqrt")),
            I32WrapI64 => format!("uint32({a})"),
            I32TruncF32S => format!("{}(float64({a}))", self.rt("i32_trunc_s")),
            I32TruncF64S => format!("{}({a})", self.rt("i32_trunc_s")),
            I32TruncF32U => format!("{}(float64({a}))", self.rt("i32_trunc_u")),
            I32TruncF64U => format!("{}({a})", self.rt("i32_trunc_u")),
            I64TruncF32S => format!("{}(float64({a}))", self.rt("i64_trunc_s")),
            I64TruncF64S => format!("{}({a})", self.rt("i64_trunc_s")),
            I64TruncF32U => format!("{}(float64({a}))", self.rt("i64_trunc_u")),
            I64TruncF64U => format!("{}({a})", self.rt("i64_trunc_u")),
            I32TruncSatF32S => format!("{}(float64({a}))", self.rt("i32_trunc_sat_s")),
            I32TruncSatF64S => format!("{}({a})", self.rt("i32_trunc_sat_s")),
            I32TruncSatF32U => format!("{}(float64({a}))", self.rt("i32_trunc_sat_u")),
            I32TruncSatF64U => format!("{}({a})", self.rt("i32_trunc_sat_u")),
            I64TruncSatF32S => format!("{}(float64({a}))", self.rt("i64_trunc_sat_s")),
            I64TruncSatF64S => format!("{}({a})", self.rt("i64_trunc_sat_s")),
            I64TruncSatF32U => format!("{}(float64({a}))", self.rt("i64_trunc_sat_u")),
            I64TruncSatF64U => format!("{}({a})", self.rt("i64_trunc_sat_u")),
            I64ExtendI32S => format!("uint64(int32({a}))"),
            I64ExtendI32U => format!("uint64({a})"),
            F32ConvertI32S => format!("float32(int32({a}))"),
            F32ConvertI32U => format!("float32({a})"),
            F32ConvertI64S => format!("float32(int64({a}))"),
            F32ConvertI64U => format!("float32({a})"),
            F64ConvertI32S => format!("float64(int32({a}))"),
            F64ConvertI32U => format!("float64({a})"),
            F64ConvertI64S => format!("float64(int64({a}))"),
            F64ConvertI64U => format!("float64({a})"),
            F32DemoteF64 => format!("{}({a})", self.rt("f32_demote")),
            F64PromoteF32 => format!("{}({a})", self.rt("f64_promote")),
            I32ReinterpretF32 => format!("{}({a})", self.rt("f32_bits")),
            I64ReinterpretF64 => format!("{}({a})", self.rt("f64_bits")),
            F32ReinterpretI32 => format!("{}({a})", self.rt("f32_from_bits")),
            F64ReinterpretI64 => format!("{}({a})", self.rt("f64_from_bits")),
            I32Extend8S => format!("uint32(int32(int8({a})))"),
            I32Extend16S => format!("uint32(int32(int16({a})))"),
            I64Extend8S => format!("uint64(int64(int8({a})))"),
            I64Extend16S => format!("uint64(int64(int16({a})))"),
            I64Extend32S => format!("uint64(int64(int32({a})))"),
        }
    }

    fn bin(&self, op: BinOp, a: &str, b: &str) -> String {
        use BinOp::*;
        match op {
            I32Add | I64Add => format!("({a} + {b})"),
            I32Sub | I64Sub => format!("({a} - {b})"),
            I32Mul | I64Mul => format!("({a} * {b})"),
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
            I32Shl => format!("({a} << ({b} & 31))"),
            I32ShrU => format!("({a} >> ({b} & 31))"),
            I32ShrS => format!("uint32(int32({a}) >> ({b} & 31))"),
            I64Shl => format!("({a} << ({b} & 63))"),
            I64ShrU => format!("({a} >> ({b} & 63))"),
            I64ShrS => format!("uint64(int64({a}) >> ({b} & 63))"),
            I32Rotl => format!("{}({a}, {b})", self.rt("i32_rotl")),
            I32Rotr => format!("{}({a}, {b})", self.rt("i32_rotr")),
            I64Rotl => format!("{}({a}, {b})", self.rt("i64_rotl")),
            I64Rotr => format!("{}({a}, {b})", self.rt("i64_rotr")),
            I32Eq | I64Eq | F32Eq | F64Eq => format!("{}({a} == {b})", self.rt("b2i")),
            I32Ne | I64Ne | F32Ne | F64Ne => format!("{}({a} != {b})", self.rt("b2i")),
            I32LtU | I64LtU | F32Lt | F64Lt => format!("{}({a} < {b})", self.rt("b2i")),
            I32GtU | I64GtU | F32Gt | F64Gt => format!("{}({a} > {b})", self.rt("b2i")),
            I32LeU | I64LeU | F32Le | F64Le => format!("{}({a} <= {b})", self.rt("b2i")),
            I32GeU | I64GeU | F32Ge | F64Ge => format!("{}({a} >= {b})", self.rt("b2i")),
            I32LtS => format!("{}(int32({a}) < int32({b}))", self.rt("b2i")),
            I32GtS => format!("{}(int32({a}) > int32({b}))", self.rt("b2i")),
            I32LeS => format!("{}(int32({a}) <= int32({b}))", self.rt("b2i")),
            I32GeS => format!("{}(int32({a}) >= int32({b}))", self.rt("b2i")),
            I64LtS => format!("{}(int64({a}) < int64({b}))", self.rt("b2i")),
            I64GtS => format!("{}(int64({a}) > int64({b}))", self.rt("b2i")),
            I64LeS => format!("{}(int64({a}) <= int64({b}))", self.rt("b2i")),
            I64GeS => format!("{}(int64({a}) >= int64({b}))", self.rt("b2i")),
            F32Add | F64Add => format!("({a} + {b})"),
            F32Sub | F64Sub => format!("({a} - {b})"),
            // mul/div route through //go:noinline helpers so Go's compiler cannot fuse a following add/sub into an FMA, nor fold `x * 1.0` / `x / 1.0` to `x` (which would skip the sNaN quieting wasm mandates): see the units.
            F32Mul => format!("{}({a}, {b})", self.rt("f32_mul")),
            F64Mul => format!("{}({a}, {b})", self.rt("f64_mul")),
            F32Div => format!("{}({a}, {b})", self.rt("f32_div")),
            F64Div => format!("{}({a}, {b})", self.rt("f64_div")),
            F32Min => format!("{}({a}, {b})", self.rt("f32_min")),
            F64Min => format!("{}({a}, {b})", self.rt("f64_min")),
            F32Max => format!("{}({a}, {b})", self.rt("f32_max")),
            F64Max => format!("{}({a}, {b})", self.rt("f64_max")),
            F32Copysign => format!("{}({a}, {b})", self.rt("f32_copysign")),
            F64Copysign => format!("{}({a}, {b})", self.rt("f64_copysign")),
        }
    }
}

fn assign_results(results: &[Temp], call: String) -> String {
    match results {
        [] => call,
        rs => {
            let names = rs.iter().map(|r| temp(*r)).collect::<Vec<_>>().join(", ");
            format!("{names} = {call}")
        }
    }
}

/// `stmts` as the emitter is to see them: statements Go would report as unreachable code are dropped, catch clauses that can never match go with them, and a frame whose surviving body no longer branches to its label loses the label.
/// Runs before the read/use pre-pass and before emission so both see the same statements: a local whose only read was dropped must not be declared, and a Go label nothing jumps to does not compile.
fn prune(stmts: &[Stmt]) -> Vec<Stmt> {
    let mut out: Vec<Stmt> = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        // `local.get $x; local.set $x` renders as `lx = lx`, which `go vet` reports as a self-assignment; the wasm pair is a no-op, so dropping it is the whole fix.
        if matches!(stmt, Stmt::LocalSet { idx, expr: Expr::LocalGet(src) } if idx == src) {
            continue;
        }
        out.push(prune_stmt(stmt));
        if ends_unreachable(&out) {
            break;
        }
    }
    out
}

fn prune_stmt(stmt: &Stmt) -> Stmt {
    match stmt {
        Stmt::Block { label, body } => {
            let body = prune(body);
            let label = frame_label(*label, &[&body]);
            Stmt::Block { label, body }
        }
        Stmt::Loop { label, body } => {
            let body = prune(body);
            let label = frame_label(*label, &[&body]);
            Stmt::Loop { label, body }
        }
        Stmt::If {
            label,
            cond,
            then,
            els,
        } => {
            let then = prune(then);
            let els = prune(els);
            let label = frame_label(*label, &[&then, &els]);
            Stmt::If {
                label,
                cond: cond.clone(),
                then,
                els,
            }
        }
        Stmt::TryTable {
            label,
            catches,
            body,
        } => Stmt::TryTable {
            // A try_table's label names its outcome variables rather than a Go label, so there is nothing to demote.
            label: *label,
            catches: live_catches(catches).to_vec(),
            body: prune(body),
        },
        stmt => stmt.clone(),
    }
}

/// `label` with `referenced` recomputed over the pruned `bodies`: Go rejects a label nothing jumps to, so a frame all of whose branches were pruned away loses its label with them.
fn frame_label(label: Label, bodies: &[&[Stmt]]) -> Label {
    Label {
        id: label.id,
        referenced: label.referenced && bodies.iter().any(|b| branches_to(b, label.id)),
    }
}

/// The catch clauses that can still run: clauses are matched in order and a tag-less one matches every exception, so it is the last that can.
fn live_catches(catches: &[CatchClause]) -> &[CatchClause] {
    let live = catches
        .iter()
        .position(|c| c.tag.is_none())
        .map_or(catches.len(), |i| i + 1);
    &catches[..live]
}

/// Whether any statement in `stmts` branches to `label`, that is, whether the emitted frame still needs its Go label.
fn branches_to(stmts: &[Stmt], label: u32) -> bool {
    let hits = |t: &BrTarget| matches!(t, BrTarget::Label { label: l, .. } if *l == label);
    Stmt::any(stmts, &mut |stmt| match stmt {
        Stmt::Br(t) | Stmt::BrIf { target: t, .. } => hits(t),
        Stmt::BrTable {
            targets, default, ..
        } => targets.iter().chain([default]).any(hits),
        // A matched clause branches from the outcome switch, which sits inside the frame like any other branch.
        Stmt::TryTable { catches, .. } => catches.iter().any(|c| hits(&c.target)),
        // No other statement carries a branch target, and the sequences nested in them are reached by `Stmt::any`.
        _ => false,
    })
}

/// Whether a statement emitted after `stmts` would be unreachable code: the Go rendering of the last one transfers control away unconditionally (a `break`, a `continue`, a `return`, or a `panic`).
/// `Stmt::Unreachable` and `Stmt::ThrowRef` are not among them: they render as a call that panics inside the runtime, which Go's reachability rule does not look through.
/// At the top level of a function body or of a `try_table` closure the same statements are Go *terminating* statements too (a `break` there would need an enclosing labeled `for`, which the top level has none of), so the trailing `return` Go's missing-return rule would otherwise demand is dead as well.
fn ends_unreachable(stmts: &[Stmt]) -> bool {
    let Some(last) = stmts
        .iter()
        .rev()
        .find(|s| !matches!(s, Stmt::SourceLine(_)))
    else {
        return false;
    };
    match last {
        // The `switch` a `br_table` renders as branches out of itself in every case, its default included.
        Stmt::Br(_) | Stmt::BrTable { .. } | Stmt::Return { .. } | Stmt::Throw { .. } => true,
        // A referenced block or if frame keeps a `break L` inside it, so its `for` is one Go can leave.
        Stmt::Block { label, body } => !label.referenced && ends_unreachable(body),
        Stmt::If {
            label, then, els, ..
        } => !label.referenced && if_ends_unreachable(then, els),
        // A branch to a loop label is a `continue`, never a `break`, so a loop body that cannot fall out of itself leaves the `for` with no exit at all.
        Stmt::Loop { body, .. } => ends_unreachable(body),
        // Every other statement can fall through to the next one; answering `false` only keeps the exit that used to be emitted unconditionally.
        _ => false,
    }
}

/// Whether neither arm of an `if` can fall out of it.
fn if_ends_unreachable(then: &[Stmt], els: &[Stmt]) -> bool {
    !els.is_empty() && ends_unreachable(then) && ends_unreachable(els)
}

/// Read/use pre-pass: collect which locals and temps a statement sequence reads, so emission can satisfy Go's unused-variable discipline.
fn collect_reads_seq(
    stmts: &[Stmt],
    read_locals: &mut BTreeSet<u32>,
    used_locals: &mut BTreeSet<u32>,
    read_temps: &mut BTreeSet<String>,
) {
    for stmt in stmts {
        collect_reads_stmt(stmt, read_locals, used_locals, read_temps);
    }
}

fn collect_reads_stmt(
    stmt: &Stmt,
    read_locals: &mut BTreeSet<u32>,
    used_locals: &mut BTreeSet<u32>,
    read_temps: &mut BTreeSet<String>,
) {
    let e = |expr: &Expr,
             rl: &mut BTreeSet<u32>,
             ul: &mut BTreeSet<u32>,
             rt: &mut BTreeSet<String>| collect_reads_expr(expr, rl, ul, rt);
    match stmt {
        Stmt::Assign { expr, .. } => e(expr, read_locals, used_locals, read_temps),
        Stmt::LocalSet { idx, expr } => {
            used_locals.insert(*idx);
            e(expr, read_locals, used_locals, read_temps);
        }
        Stmt::GlobalSet { expr, .. } => e(expr, read_locals, used_locals, read_temps),
        Stmt::Store { addr, value, .. } => {
            e(addr, read_locals, used_locals, read_temps);
            e(value, read_locals, used_locals, read_temps);
        }
        Stmt::Block { body, .. } | Stmt::Loop { body, .. } => {
            collect_reads_seq(body, read_locals, used_locals, read_temps)
        }
        Stmt::If {
            cond, then, els, ..
        } => {
            e(cond, read_locals, used_locals, read_temps);
            collect_reads_seq(then, read_locals, used_locals, read_temps);
            collect_reads_seq(els, read_locals, used_locals, read_temps);
        }
        Stmt::Br(t) => collect_reads_target(t, read_locals, used_locals, read_temps),
        Stmt::BrIf { cond, target } => {
            e(cond, read_locals, used_locals, read_temps);
            collect_reads_target(target, read_locals, used_locals, read_temps);
        }
        Stmt::BrTable {
            index,
            targets,
            default,
        } => {
            // Matches the emitter: with no case targets it collapses to the default branch and never evaluates the (already side-effect-free, hoisted) index, so counting it read here would leave a declared temp Go rejects as unused.
            if !targets.is_empty() {
                e(index, read_locals, used_locals, read_temps);
            }
            for t in targets {
                collect_reads_target(t, read_locals, used_locals, read_temps);
            }
            collect_reads_target(default, read_locals, used_locals, read_temps);
        }
        Stmt::Return { values } => {
            for v in values {
                e(v, read_locals, used_locals, read_temps);
            }
        }
        Stmt::Call { args, .. } | Stmt::ReturnCall { args, .. } => {
            for a in args {
                e(a, read_locals, used_locals, read_temps);
            }
        }
        Stmt::CallIndirect { index, args, .. } | Stmt::ReturnCallIndirect { index, args, .. } => {
            e(index, read_locals, used_locals, read_temps);
            for a in args {
                e(a, read_locals, used_locals, read_temps);
            }
        }
        Stmt::MemoryGrow { delta, .. } => e(delta, read_locals, used_locals, read_temps),
        Stmt::MemoryCopy { dst, src, len } => {
            e(dst, read_locals, used_locals, read_temps);
            e(src, read_locals, used_locals, read_temps);
            e(len, read_locals, used_locals, read_temps);
        }
        Stmt::MemoryFill { dst, val, len } => {
            e(dst, read_locals, used_locals, read_temps);
            e(val, read_locals, used_locals, read_temps);
            e(len, read_locals, used_locals, read_temps);
        }
        Stmt::MemoryInit { dst, src, len, .. } => {
            e(dst, read_locals, used_locals, read_temps);
            e(src, read_locals, used_locals, read_temps);
            e(len, read_locals, used_locals, read_temps);
        }
        Stmt::TableInit { dst, src, len, .. } | Stmt::TableCopy { dst, src, len, .. } => {
            e(dst, read_locals, used_locals, read_temps);
            e(src, read_locals, used_locals, read_temps);
            e(len, read_locals, used_locals, read_temps);
        }
        Stmt::TryTable { catches, body, .. } => {
            collect_reads_seq(body, read_locals, used_locals, read_temps);
            // A catch clause's payload temps are written by the handler, never read by it; the clause's target may read them back.
            for clause in catches {
                collect_reads_target(&clause.target, read_locals, used_locals, read_temps);
            }
        }
        Stmt::Throw { args, .. } => {
            for a in args {
                e(a, read_locals, used_locals, read_temps);
            }
        }
        Stmt::ThrowRef { exn } => e(exn, read_locals, used_locals, read_temps),
        Stmt::DataDrop { .. } | Stmt::ElemDrop { .. } | Stmt::Unreachable | Stmt::SourceLine(_) => {
        }
    }
}

fn collect_reads_target(
    t: &BrTarget,
    read_locals: &mut BTreeSet<u32>,
    used_locals: &mut BTreeSet<u32>,
    read_temps: &mut BTreeSet<String>,
) {
    match t {
        BrTarget::Return { values } => {
            for v in values {
                collect_reads_expr(v, read_locals, used_locals, read_temps);
            }
        }
        BrTarget::Label { assigns, .. } => {
            for (_dst, src) in assigns {
                read_temps.insert(temp(*src));
            }
        }
    }
}

fn collect_reads_expr(
    expr: &Expr,
    read_locals: &mut BTreeSet<u32>,
    used_locals: &mut BTreeSet<u32>,
    read_temps: &mut BTreeSet<String>,
) {
    match expr {
        Expr::I32Const(_)
        | Expr::I64Const(_)
        | Expr::F32Const(_)
        | Expr::F64Const(_)
        | Expr::GlobalGet(_)
        | Expr::MemorySize => {}
        Expr::Temp(t) => {
            read_temps.insert(temp(*t));
        }
        Expr::LocalGet(idx) => {
            read_locals.insert(*idx);
            used_locals.insert(*idx);
        }
        Expr::Un(_, a) => collect_reads_expr(a, read_locals, used_locals, read_temps),
        Expr::Bin(_, a, b) => {
            collect_reads_expr(a, read_locals, used_locals, read_temps);
            collect_reads_expr(b, read_locals, used_locals, read_temps);
        }
        Expr::Load { addr, .. } => collect_reads_expr(addr, read_locals, used_locals, read_temps),
        Expr::Select { cond, then, els } => {
            collect_reads_expr(cond, read_locals, used_locals, read_temps);
            collect_reads_expr(then, read_locals, used_locals, read_temps);
            collect_reads_expr(els, read_locals, used_locals, read_temps);
        }
    }
}

/// Lint for the runtime units: every reference a unit body makes to another unit must be declared in its `// requires:` header.
/// Mirrors the Python backend's units lint, adjusted for Go syntax: `Rt.<name>` helper calls, `.memory.<name>` memory-method calls, and per-scope sibling calls through the receiver letter (`m`/`t`/`w`).
/// A second test compiles the full bundle with `go build`, so a syntax error in any unit (not just the subset any one module uses) is caught.
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
        let memory_call = Regex::new(r"\.memory\.([a-z_][a-z0-9_]*)").unwrap();
        let recv_of = |scope: &str| -> Option<&'static str> {
            match scope {
                "memory" => Some("m"),
                "table" => Some("t"),
                "wasi" => Some("w"),
                _ => None, // rt siblings are referenced via `Rt.`
            }
        };
        let sibling_calls: Vec<(&str, Regex)> = unit_ids
            .iter()
            .filter_map(|id| {
                let scope = id.split('/').next().unwrap();
                let name = id.split('/').nth(1).unwrap();
                let recv = recv_of(scope)?;
                let re = Regex::new(&format!(r"\b{}\.{}\(", recv, regex::escape(name))).unwrap();
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
                    &format!(".memory.{}", &cap[1]),
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
                    demand(sibling.to_string(), &format!("<recv>.{name}(...)"));
                }
            }
        }
        assert!(
            problems.is_empty(),
            "unit dependency drift:\n{}",
            problems.join("\n")
        );
    }

    /// The whole runtime (every unit, not just the subset a given module uses) must be valid Go.
    /// Compile the full bundle with `go build` (a missing toolchain fails loud, it does not skip).
    #[test]
    fn all_units_compile_as_go() {
        let go =
            find_go().expect("go toolchain not found on PATH (or $DEWASM_GO): see docs/testing.md");
        let source = full_bundle_go().expect("full bundle assembles");
        let dir = std::env::temp_dir().join(format!("dewasm-go-units-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("main.go");
        std::fs::write(&src, &source).unwrap();
        let out = std::process::Command::new(&go)
            .arg("build")
            .arg("-o")
            .arg(dir.join("bundle_bin"))
            .arg(&src)
            .output()
            .expect("spawn go build");
        assert!(
            out.status.success(),
            "full runtime bundle failed to compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
