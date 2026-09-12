//! Codon backend: translates dewasm IR into a Codon module (a class plus a bundled lightweight runtime), compiled ahead of time by the Codon toolchain.
//!
//! Lowering conventions:
//! - i32/i64 are native `UInt[32]`/`UInt[64]` (wrapping arithmetic, unsigned compares and shifts are the stored representation's own); signed views are `Int[32]`/`Int[64]` casts, the Go backend's shape rather than the masked-unsigned one (Codon's `int` is 64-bit signed, so masked-unsigned i64 values would not fit).
//! - f32/f64 are native `float32`/`float`.
//!   Float division goes through an `@llvm` `fdiv` unit because Codon's `/` raises on a zero divisor.
//!   A float add/sub/mul/div with a constant operand is wrapped in `Rt.f32_q`/`Rt.f64_q`: LLVM folds `x * 1.0`, `x / 1.0` and `x + -0.0` to `x` under `-release`, which would skip the signaling-NaN quieting wasm requires (measured on Codon 0.20.1).
//! - Control flow uses the Python backend's branch-register model: block bodies are spliced inline, forward branches set a per-function `_br` register read by guarded regions, and only real loops become `while True:`.
//!   The flat state-machine lowering for deep crossings is not ported yet; every branch relays.
//! - The dynamic boundary is uniformly boxed, the Java backend's shape under Codon's static typing: a wasm function value is an `Rt.Fn` (`invoke(List[Val]) -> List[Val]`), an import/export value is an `Rt.Extern` whose typed field per kind is what makes import resolution check kinds and global value types, and direct calls to defined functions stay native.
//!   Import providers are plain `Dict[str, Dict[str, Rt.Extern]]`; the duck-typed provider objects of the dynamic backends have no Codon equivalent.
//! - Memory is a raw `Ptr[byte]` with `@llvm` `align 1` loads and stores, so unaligned access is defined and free.
//!
//! The runtime is composed from per-method units and referenced by a module-level class name.
//! Under `Embedded` linkage that name is per-artifact (`<Class>Rt`), so two generated artifacts in one namespace keep independent runtimes; `Alias` linkage keeps the shared `Rt`.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::{
    check_module_support, comparison, hex_string, is_boolean, is_ident, is_wasi_module,
    load_method, module_name_error, stmts_use_tail_calls, store_method, type_key, wasi_bundled,
    Backend, CodeWriter, CompareOperands, GenOptions, Mode, OutputFile, RuntimeBundler,
    RuntimeLinkage, RuntimeScope, SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, ElemItem, ElemKind, ExportKind, Expr, Func, FuncType, Module, Stmt, Temp,
    UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The runtime unit bundler for Codon (see crates/dewasm-backend-codon/units/).
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
                    prelude: None,
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
                    open: "class Global[T]:",
                    close: "",
                    prelude: Some("global/_class"),
                },
                RuntimeScope {
                    prefix: "wasi",
                    open: "class WASI:",
                    close: "",
                    prelude: Some("wasi/_class"),
                },
                // Last on purpose: Extern's field annotations name Fn, Global, Table, Memory and Tag, and Codon resolves nested-class field and signature annotations in definition order (no forward references).
                RuntimeScope {
                    prefix: "ext",
                    open: "",
                    close: "",
                    prelude: Some("ext/extern"),
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

/// Locate a `codon` toolchain able to compile generated programs: `$DEWASM_CODON` first, then `codon` on `PATH`.
/// A missing toolchain is a loud failure at the call site, not here.
pub fn find_codon() -> Option<std::path::PathBuf> {
    static CODON: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    CODON.get_or_init(find_codon_uncached).clone()
}

/// The probe behind [`find_codon`], memoized there: it spawns a process per call, and the toolchain cannot change under a running process.
fn find_codon_uncached() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("DEWASM_CODON") {
        candidates.push(PathBuf::from(env));
    }
    candidates.push(PathBuf::from("codon"));
    candidates.into_iter().find(|candidate| {
        std::process::Command::new(candidate)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// The directory holding Codon's runtime dylibs (`libcodonrt`, `libomp`), which a built binary needs on the loader path.
/// Resolved relative to the `codon` binary (`<prefix>/bin/codon` -> `<prefix>/lib/codon`), following symlinks.
pub fn codon_lib_dir(codon: &std::path::Path) -> Option<std::path::PathBuf> {
    let resolved = codon.canonicalize().ok()?;
    let prefix = resolved.parent()?.parent()?;
    let lib = prefix.join("lib").join("codon");
    lib.is_dir().then_some(lib)
}

pub struct CodonBackend;

impl Backend for CodonBackend {
    fn name(&self) -> &str {
        "codon"
    }

    fn file_extension(&self) -> &str {
        "codon"
    }

    fn has_wasi_p1(&self, name: &str) -> bool {
        bundler().has_unit(&format!("wasi/{name}"))
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // Native float32/float with @llvm bit paths; NaN payload handling mirrors the Go backend's units.
            Feature::Floats => SupportStatus::Supported,
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
            // Tags are identity objects, a thrown exception is a native exception that doubles as the exnref, and traps stay uncatchable: the Python backend's model under Codon's typing.
            Feature::ExceptionHandling => SupportStatus::Supported,
            // A trampoline with a body/entry split (the Go backend's shape under Codon's nominal typing): a parked call is typed per argument slot and per result signature, so a chain bounces in one frame with nothing boxed.
            Feature::TailCall => SupportStatus::Supported,
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
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
            false,
        )?;

        let mut w = CodeWriter::new("\t");
        w.line("# Generated by dewasm. Do not edit.");
        if opts.mode == Mode::Standalone {
            w.line("import os");
            w.line("import sys");
            w.line("");
            w.line("");
        }
        w.raw(&class_src);

        if opts.mode == Mode::Standalone {
            let wasi = wasi_bundled(module, opts.default_wasi, bundler());
            w.line("");
            w.line("");
            self::standalone_main(&mut w, module, &class_name, &rt_name, wasi);
        }

        Ok(vec![OutputFile {
            name: format!("{}.codon", opts.module_name),
            contents: w.finish().into_bytes(),
        }])
    }
}

/// The standalone entrypoint: parse the `--dir HOST::GUEST` runtime interface, instantiate, run `_start`, and map Exit/Trap to the process exit status.
fn standalone_main(
    w: &mut CodeWriter,
    module: &Module,
    class_name: &str,
    rt_name: &str,
    wasi: bool,
) {
    w.line("def _run_main():");
    w.indent();
    if wasi {
        w.line("_pre = Dict[str, str]()");
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
        w.line("_spec = \"\"");
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
        w.line("_j = _spec.find(\"::\")");
        w.line("if _j >= 0:");
        w.indent();
        w.line("_pre[_spec[_j + 2:]] = _spec[:_j]");
        w.dedent();
        w.line("else:");
        w.indent();
        w.line("_pre[_spec] = _spec");
        w.dedent();
        w.line("_i += 1");
        w.dedent();
        w.line("else:");
        w.indent();
        w.line("break");
        w.dedent();
        w.dedent();
        w.line("_name = sys.argv[0]");
        w.line("_k = _name.rfind(\"/\")");
        w.line("if _k >= 0:");
        w.indent();
        w.line("_name = _name[_k + 1:]");
        w.dedent();
        w.line("_args = [_name]");
        w.line("for _x in _argv[_i:]:");
        w.indent();
        w.line("_args.append(_x)");
        w.dedent();
        w.line(format!(
            "_inst = {class_name}(Dict[str, Dict[str, {rt_name}.Extern]](), _args, dict(os.environ), _pre)"
        ));
    } else {
        w.line(format!(
            "_inst = {class_name}(Dict[str, Dict[str, {rt_name}.Extern]](), List[str](), Dict[str, str](), Dict[str, str]())"
        ));
    }
    w.line("try:");
    w.indent();
    let start = module.exports.iter().find_map(|e| match e.kind {
        ExportKind::Func(idx) if e.name == "_start" => Some(idx),
        _ => None,
    });
    match start {
        Some(idx) if (idx as usize) < module.imported_funcs.len() => {
            w.line(format!("_inst.if{idx}.invoke(List[{rt_name}.Val]())"));
        }
        Some(idx) => {
            w.line(format!("_inst._f{idx}()"));
        }
        None => {
            w.line("sys.stderr.write(\"no _start export\\n\")");
            w.line("sys.exit(1)");
        }
    }
    w.dedent();
    w.line(format!("except {rt_name}.Exit as _e:"));
    w.indent();
    w.line("sys.exit(_e.code)");
    w.dedent();
    w.line(format!("except {rt_name}.Trap as _e:"));
    w.indent();
    w.line("sys.stderr.write(\"trap: \" + _e.message + \"\\n\")");
    w.line("sys.exit(134)");
    w.dedent();
    w.dedent();
    w.line("");
    w.line("");
    w.line("if __name__ == \"__main__\":");
    w.indent();
    w.line("_run_main()");
    w.dedent();
}

/// Returns the class source (runtime bundle included for `Embedded`, wrapper classes appended) and the set of runtime units it needs.
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
        false,
    )
}

/// The spec-harness variant: shared `Rt` linkage, plus the reflective `invoke`/`global_get` dispatchers and the recursion guard that turns a fatal native stack overflow into a catchable "call stack exhausted" trap.
pub fn generate_spec_class_with_units(
    module: &Module,
    class_name: &str,
) -> Result<(String, BTreeSet<String>)> {
    generate_class_inner(
        module,
        class_name,
        &RuntimeLinkage::Alias("Rt".to_string()),
        false,
        &BTreeSet::new(),
        true,
    )
}

fn generate_class_inner(
    module: &Module,
    class_name: &str,
    linkage: &RuntimeLinkage,
    default_wasi: bool,
    extra_seeds: &BTreeSet<String>,
    spec: bool,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&CodonBackend, module)?;
    let rt_name = runtime_name(class_name, linkage);
    let tail_callers: BTreeSet<u32> = module
        .funcs
        .iter()
        .enumerate()
        .filter(|(_, f)| stmts_use_tail_calls(&f.body))
        .map(|(i, _)| module.num_imported_funcs() + i as u32)
        .collect();
    let mut seeds = extra_seeds.clone();
    // Always bundled: embedder glue names `<Rt>.Trap`/`<Rt>.Exit` in `except` clauses, which Codon resolves statically even when the run never raises (CPython's lazy except-expression evaluation has no equivalent).
    seeds.insert("rt/trap".to_string());
    seeds.insert("rt/exit".to_string());
    let gen = Gen {
        module,
        default_wasi,
        tail_callers,
        uses: RefCell::new(seeds),
        rt_name: rt_name.clone(),
        class_name: class_name.to_string(),
        spec,
    };
    let mut wb = CodeWriter::new("\t");
    // Tail-entry base classes come first: the generated class's parked-target fields name them in annotations, which Codon resolves in definition order.
    gen.tail_bases(&mut wb);
    gen.class(&mut wb, class_name);
    let mut body = wb.finish();

    // Boxed-function wrapper classes are module-level (they subclass the runtime's Fn and name the generated class), appended after it.
    let mut ww = CodeWriter::new("\t");
    gen.wrappers(&mut ww);
    let wrappers = ww.finish();
    if !wrappers.is_empty() {
        body.push('\n');
        body.push_str(&wrappers);
    }

    let uses = gen.uses.into_inner();
    let mut out = String::new();
    match linkage {
        RuntimeLinkage::Embedded => {
            if !uses.is_empty() {
                // Namespace the runtime under the generated class: the units reference the runtime only as `Rt.<name>`, never inside a string literal (the units lint enforces that), so one textual replace moves every reference onto the per-artifact name.
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
            if path != "Rt" {
                out.push_str(&format!("Rt = {path}\n\n\n"));
            }
        }
    }
    out.push_str(&body);
    Ok((out, uses))
}

/// The module-level name the generated class references its runtime by.
/// `Embedded` output is self-contained and must survive sharing a namespace with another artifact, so it gets a per-artifact `<Class>Rt`; `Alias` output points at a runtime someone else emitted.
fn runtime_name(class_name: &str, linkage: &RuntimeLinkage) -> String {
    match linkage {
        RuntimeLinkage::Embedded => format!("{class_name}Rt"),
        RuntimeLinkage::Alias(_) => "Rt".to_string(),
    }
}

/// The class a `--mode standalone` program defines: fixed, since nothing outside a self-contained program observes it.
pub const STANDALONE_CLASS: &str = "Program";

/// The library-mode module name must be a single identifier and is used verbatim (it names the generated class and, suffixed `Rt`, its embedded runtime).
fn check_module_name(name: &str) -> Result<()> {
    if is_ident(
        name,
        |c| c.is_ascii_alphabetic() || c == '_',
        |c| c.is_ascii_alphanumeric() || c == '_',
    ) {
        Ok(())
    } else {
        Err(module_name_error(
            "codon",
            name,
            "a single identifier matching [A-Za-z_][A-Za-z0-9_]* (e.g. Add, sqlite3)",
        ))
    }
}

/// Codon double-quoted string literal (same escapes as Python's).
pub fn codon_string(s: &str) -> String {
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

pub use dewasm_backend::WASI_PREVIEW1_FUNCTIONS;

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

fn codon_type(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "UInt[32]",
        ValType::I64 => "UInt[64]",
        ValType::F32 => "float32",
        ValType::F64 => "float",
        // Spelled by Gen::ty_str, which knows the runtime name.
        ValType::ExnRef => unreachable!("exnref is spelled by ty_str"),
        // funcref as a value type needs reference types, rejected at conversion time.
        ValType::FuncRef => unreachable!("reference-typed value"),
    }
}

fn zero_value(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "UInt[32](0)",
        ValType::I64 => "UInt[64](0)",
        ValType::F32 => "float32(0.0)",
        ValType::F64 => "0.0",
        // A wasm exnref local/temp defaults to the null reference.
        ValType::ExnRef => "None",
        ValType::FuncRef => unreachable!("reference-typed value"),
    }
}

fn temp(t: Temp) -> String {
    format!("s{}_{}", t.depth, ty_suffix(t.ty))
}

/// A `UInt[32]` constant expression.
fn i32_const(v: u32) -> String {
    format!("UInt[32]({v})")
}

/// A `UInt[64]` constant expression.
/// Values above `i64::MAX` are spelled in hex: Codon rejects a decimal literal outside the signed 64-bit range but parses the hex form (wrapping), and `UInt[64]` reinterprets the bits.
fn i64_const(v: u64) -> String {
    if v > i64::MAX as u64 {
        format!("UInt[64](0x{v:X})")
    } else {
        format!("UInt[64]({v})")
    }
}

/// A float literal that round-trips to the same double.
/// `{:?}` on f64 gives the shortest round-tripping decimal, which Codon parses back exactly; non-finite values never reach here.
fn codon_float(v: f64) -> String {
    format!("{v:?}")
}

/// The boxed accessor reading a [`ValType`] out of an `Rt.Val`.
fn val_get(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "i32",
        ValType::I64 => "i64",
        ValType::F32 => "f32",
        ValType::F64 => "f64",
        ValType::ExnRef => "exn",
        ValType::FuncRef => unreachable!("reference-typed value"),
    }
}

/// The `Rt.Val` constructor for a [`ValType`].
fn val_of(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 => "of_i32",
        ValType::I64 => "of_i64",
        ValType::F32 => "of_f32",
        ValType::F64 => "of_f64",
        ValType::ExnRef => "of_exn",
        ValType::FuncRef => unreachable!("reference-typed value"),
    }
}

/// The `Rt.Extern` accessor pair for a global's value type: (constructor, import checker).
fn global_extern(ty: ValType) -> (&'static str, &'static str) {
    match ty {
        ValType::I32 => ("of_global_i32", "import_global_i32"),
        ValType::I64 => ("of_global_i64", "import_global_i64"),
        ValType::F32 => ("of_global_f32", "import_global_f32"),
        ValType::F64 => ("of_global_f64", "import_global_f64"),
        ValType::ExnRef => ("of_global_exn", "import_global_exn"),
        ValType::FuncRef => unreachable!("reference-typed global"),
    }
}

/// The identifier suffix naming a tail-call result signature: `v` for none, else the type suffixes joined by `_`.
fn sig_id(results: &[ValType]) -> String {
    if results.is_empty() {
        return "v".to_string();
    }
    results
        .iter()
        .map(|t| ty_suffix(*t).to_string())
        .collect::<Vec<_>>()
        .join("_")
}

/// The parked-argument field for position `i` of type `ty`.
fn tail_arg_slot(i: usize, ty: ValType) -> String {
    format!("_ta{i}_{}", ty_suffix(ty))
}

/// The recursion-guard budget (spec builds only), the Go backend's model: each generated function adds its frame's slot count to a shared counter on entry and traps once the running total exceeds this, turning an otherwise-fatal native stack overflow into a catchable "call stack exhausted" trap.
const SPEC_STACK_LIMIT: usize = 1024;

struct Gen<'a> {
    module: &'a Module,
    default_wasi: bool,
    /// Defined functions (function index space) containing a tail call: these are the ones split into `_f{idx}_body` plus a trampoline entry.
    tail_callers: BTreeSet<u32>,
    /// Runtime units the generated code references.
    uses: RefCell<BTreeSet<String>>,
    /// The module-level name of the runtime this artifact references (see [`runtime_name`]).
    rt_name: String,
    class_name: String,
    /// Spec-harness mode: emit the reflective `invoke`/`global_get` dispatchers and the recursion guard.
    spec: bool,
}

impl<'a> Gen<'a> {
    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    /// Reference a runtime helper, recording its unit.
    fn rt(&self, name: &str) -> String {
        self.use_unit(&format!("rt/{name}"));
        format!("{}.{name}", self.rt_name)
    }

    /// The Codon spelling of a value type; exnref is the runtime's own nullable exception class, so it carries the per-artifact runtime name.
    fn ty_str(&self, ty: ValType) -> String {
        if ty == ValType::ExnRef {
            self.use_unit("rt/boxed");
            return format!("Optional[{}.WasmException]", self.rt_name);
        }
        codon_type(ty).to_string()
    }

    /// Reference a Memory method, recording its unit.
    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
    }

    fn func_type_symbol(&self, func_idx: u32) -> String {
        self.type_symbol_of(self.module.func_type(func_idx))
    }

    fn type_symbol(&self, type_idx: u32) -> String {
        self.type_symbol_of(&self.module.types[type_idx as usize])
    }

    /// A structural type key (see [`type_key`]) as a string literal, which is what the table stores and `call_indirect` compares.
    fn type_symbol_of(&self, ty: &FuncType) -> String {
        codon_string(&type_key(ty, dewasm_backend::val_name))
    }

    /// The defined functions that get a typed tail entry: the tail callers themselves plus every defined function a direct `return_call` targets.
    /// A tail call always parks (running the callee inline would keep this frame's exception handlers alive, which the proposal forbids), so every parkable direct target needs an entry.
    fn tail_entries(&self) -> BTreeSet<u32> {
        let mut set = self.tail_callers.clone();
        for f in &self.module.funcs {
            Stmt::any(&f.body, &mut |st| {
                if let Stmt::ReturnCall { func, .. } = st {
                    if *func >= self.module.num_imported_funcs() {
                        set.insert(*func);
                    }
                }
                false
            });
        }
        set
    }

    /// The dense position of `func_idx` in its result signature's entry table, or None when it has no entry.
    fn tail_slot(&self, func_idx: u32) -> Option<usize> {
        let entries = self.tail_entries();
        if !entries.contains(&func_idx) {
            return None;
        }
        let results = self.module.func_type(func_idx).results.clone();
        entries
            .iter()
            .filter(|f| self.module.func_type(**f).results == results)
            .position(|f| *f == func_idx)
    }

    /// The distinct result signatures a parked tail call can carry: the entries' own, every indirect tail call site's, and every imported direct target's.
    /// Each needs one parked-target field, one entry table, one entry base class, and one boxed entry subclass.
    fn tail_signatures(&self) -> Vec<Vec<ValType>> {
        let mut seen: BTreeSet<Vec<ValType>> = BTreeSet::new();
        for idx in self.tail_entries() {
            seen.insert(self.module.func_type(idx).results.clone());
        }
        for f in &self.module.funcs {
            Stmt::any(&f.body, &mut |st| {
                match st {
                    Stmt::ReturnCallIndirect { type_idx, .. } => {
                        seen.insert(self.module.types[*type_idx as usize].results.clone());
                    }
                    Stmt::ReturnCall { func, .. } if *func < self.module.num_imported_funcs() => {
                        seen.insert(self.module.func_type(*func).results.clone());
                    }
                    _ => {}
                }
                false
            });
        }
        seen.into_iter().collect()
    }

    /// Every argument slot the module's tail calls park into: one per position and type a tail-calling function's own parameters need (its entry reads them back out), plus every signature reachable through an indirect tail call site.
    fn tail_arg_slots(&self) -> Vec<(usize, ValType)> {
        let mut seen: BTreeSet<(usize, ValType)> = BTreeSet::new();
        let note = |params: &[ValType], seen: &mut BTreeSet<(usize, ValType)>| {
            for (i, ty) in params.iter().enumerate() {
                seen.insert((i, *ty));
            }
        };
        for idx in self.tail_entries() {
            note(&self.module.func_type(idx).params, &mut seen);
        }
        for f in &self.module.funcs {
            Stmt::any(&f.body, &mut |st| {
                if let Stmt::ReturnCallIndirect { type_idx, .. } = st {
                    note(&self.module.types[*type_idx as usize].params, &mut seen);
                }
                false
            });
        }
        seen.into_iter().collect()
    }

    /// The return clause of a tail entry's `run` for a result signature.
    fn tail_ret(&self, results: &[ValType]) -> String {
        match results {
            [] => String::new(),
            [t] => format!(" -> {}", self.ty_str(*t)),
            ts => format!(
                " -> Tuple[{}]",
                ts.iter()
                    .map(|t| self.ty_str(*t))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    /// The wrapper classes the boxed boundary needs: one per defined function that is exported or referenced by an element segment, plus the bundled-WASI and ENOSYS fallbacks per imported function.
    fn boxed_funcs(&self) -> BTreeSet<u32> {
        let m = self.module;
        let mut set = BTreeSet::new();
        for export in &m.exports {
            if let ExportKind::Func(idx) = export.kind {
                if idx >= m.num_imported_funcs() {
                    set.insert(idx);
                }
            }
        }
        for elem in &m.elems {
            for item in &elem.items {
                if let ElemItem::Func(idx) = item {
                    if *idx >= m.num_imported_funcs() {
                        set.insert(*idx);
                    }
                }
            }
        }
        set
    }

    /// The kind of fallback each imported function takes when the embedder does not provide it.
    fn import_fallback(&self, i: usize) -> ImportFallback {
        let import = &self.module.imported_funcs[i];
        if is_wasi_module(&import.module) && self.default_wasi {
            let unit = format!("wasi/{}", import.name);
            if bundler().has_unit(&unit) {
                ImportFallback::Wasi
            } else {
                ImportFallback::Enosys
            }
        } else {
            ImportFallback::LinkError
        }
    }

    /// Emit the abstract tail-entry base classes (module level, before the generated class): one per result signature, whose `run` re-enters a parked body.
    fn tail_bases(&self, w: &mut CodeWriter) {
        let class = &self.class_name;
        for sig in self.tail_signatures() {
            let id = sig_id(&sig);
            w.line(format!("class {class}_TB_{id}:"));
            w.indent();
            w.line("def __init__(self):");
            w.indent();
            w.line("pass");
            w.dedent();
            w.line(format!("def run(self){}:", self.tail_ret(&sig)));
            w.indent();
            w.line(format!("{}(\"uncallable tail entry\")", self.rt("trap")));
            match sig.as_slice() {
                [] => {}
                [t] => w.line(format!("return {}", zero_value(*t))),
                ts => {
                    let zeros = ts.iter().map(|t| zero_value(*t)).collect::<Vec<_>>();
                    w.line(format!("return ({})", zeros.join(", ")));
                }
            }
            w.dedent();
            w.dedent();
            w.line("");
        }
    }

    /// Emit every wrapper class this artifact needs (module level, after the generated class).
    fn wrappers(&self, w: &mut CodeWriter) {
        let m = self.module;
        let class = &self.class_name;
        let rt = &self.rt_name;
        for idx in self.tail_entries().iter().copied().collect::<Vec<_>>() {
            let ty = m.func_type(idx).clone();
            let id = sig_id(&ty.results);
            w.line(format!("class {class}_TB_{id}_F{idx}({class}_TB_{id}):"));
            w.indent();
            w.line(format!("p: {class}"));
            w.line(format!("def __init__(self, p: {class}):"));
            w.indent();
            w.line("self.p = p");
            w.dedent();
            w.line(format!("def run(self){}:", self.tail_ret(&ty.results)));
            w.indent();
            let args = ty
                .params
                .iter()
                .enumerate()
                .map(|(i, t)| format!("self.p.{}", tail_arg_slot(i, *t)))
                .collect::<Vec<_>>()
                .join(", ");
            // A tail caller's entry re-enters its body; a plain target's entry completes in one frame, which the criterion permits.
            let call = if self.tail_callers.contains(&idx) {
                format!("self.p._f{idx}_body({args})")
            } else {
                format!("self.p._f{idx}({args})")
            };
            if ty.results.is_empty() {
                w.line(call);
            } else {
                w.line(format!("return {call}"));
            }
            w.dedent();
            w.dedent();
            w.line("");
        }
        // The boxed entry: an indirect tail call to another instance's function (or a direct one to an import) parks the boxed callee with its boxed arguments.
        for sig in self.tail_signatures() {
            self.use_unit("rt/boxed");
            let id = sig_id(&sig);
            w.line(format!("class {class}_TB_{id}_X({class}_TB_{id}):"));
            w.indent();
            w.line(format!("f: {rt}.Fn"));
            w.line(format!("a: List[{rt}.Val]"));
            w.line(format!(
                "def __init__(self, f: {rt}.Fn, a: List[{rt}.Val]):"
            ));
            w.indent();
            w.line("self.f = f");
            w.line("self.a = a");
            w.dedent();
            w.line(format!("def run(self){}:", self.tail_ret(&sig)));
            w.indent();
            match sig.as_slice() {
                [] => {
                    w.line("self.f.invoke(self.a)");
                }
                [t] => {
                    w.line(format!("return self.f.invoke(self.a)[0].{}()", val_get(*t)));
                }
                ts => {
                    w.line("_ir = self.f.invoke(self.a)");
                    let items = ts
                        .iter()
                        .enumerate()
                        .map(|(k, t)| format!("_ir[{k}].{}()", val_get(*t)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    w.line(format!("return ({items})"));
                }
            }
            w.dedent();
            w.dedent();
            w.line("");
        }
        for idx in self.boxed_funcs() {
            self.use_unit("rt/boxed");
            self.use_unit("rt/boxed");
            let ty = m.func_type(idx).clone();
            w.line(format!("class {class}_W{idx}({rt}.Fn):"));
            w.indent();
            w.line(format!("p: {class}"));
            w.line(format!("def __init__(self, p: {class}):"));
            w.indent();
            w.line("self.p = p");
            w.dedent();
            w.line(format!(
                "def invoke(self, a: List[{rt}.Val]) -> List[{rt}.Val]:"
            ));
            w.indent();
            let args = ty
                .params
                .iter()
                .enumerate()
                .map(|(k, t)| format!("a[{k}].{}()", val_get(*t)))
                .collect::<Vec<_>>()
                .join(", ");
            let call = format!("self.p._f{idx}({args})");
            self.emit_boxed_return(w, &call, &ty.results);
            w.dedent();
            w.dedent();
            w.line("");
        }
        for (i, import) in m.imported_funcs.iter().enumerate() {
            let ty = m.types[import.type_idx as usize].clone();
            match self.import_fallback(i) {
                ImportFallback::Wasi => {
                    self.use_unit("rt/boxed");
                    self.use_unit("rt/boxed");
                    self.use_unit("wasi/_class");
                    self.use_unit(&format!("wasi/{}", import.name));
                    w.line(format!("class {class}_WW{i}({rt}.Fn):"));
                    w.indent();
                    w.line(format!("w: {rt}.WASI"));
                    w.line(format!("def __init__(self, w: {rt}.WASI):"));
                    w.indent();
                    w.line("self.w = w");
                    w.dedent();
                    w.line(format!(
                        "def invoke(self, a: List[{rt}.Val]) -> List[{rt}.Val]:"
                    ));
                    w.indent();
                    let args = ty
                        .params
                        .iter()
                        .enumerate()
                        .map(|(k, t)| format!("a[{k}].{}()", val_get(*t)))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let call = format!("self.w.wasi_{}({args})", import.name);
                    self.emit_boxed_return(w, &call, &ty.results);
                    w.dedent();
                    w.dedent();
                    w.line("");
                }
                ImportFallback::Enosys => {
                    self.use_unit("rt/boxed");
                    self.use_unit("rt/boxed");
                    w.line(format!("class {class}_WS{i}({rt}.Fn):"));
                    w.indent();
                    w.line("def __init__(self):");
                    w.indent();
                    w.line("pass");
                    w.dedent();
                    w.line(format!(
                        "def invoke(self, a: List[{rt}.Val]) -> List[{rt}.Val]:"
                    ));
                    w.indent();
                    // ENOSYS for the single-i32-result syscall shape, zero values otherwise.
                    if ty.results == [ValType::I32] {
                        w.line(format!("return [{rt}.Val.of_i32(UInt[32](52))]"));
                    } else if ty.results.is_empty() {
                        w.line(format!("return List[{rt}.Val]()"));
                    } else {
                        let vals = ty
                            .results
                            .iter()
                            .map(|t| format!("{rt}.Val.{}({})", val_of(*t), zero_value(*t)))
                            .collect::<Vec<_>>()
                            .join(", ");
                        w.line(format!("return [{vals}]"));
                    }
                    w.dedent();
                    w.dedent();
                    w.line("");
                }
                ImportFallback::LinkError => {}
            }
        }
    }

    /// The `invoke` body of a wrapper: run `call` and box its results.
    fn emit_boxed_return(&self, w: &mut CodeWriter, call: &str, results: &[ValType]) {
        let rt = &self.rt_name;
        match results.len() {
            0 => {
                w.line(call);
                w.line(format!("return List[{rt}.Val]()"));
            }
            1 => {
                w.line(format!("_r = {call}"));
                w.line(format!("return [{rt}.Val.{}(_r)]", val_of(results[0])));
            }
            _ => {
                let names: Vec<String> = (0..results.len()).map(|k| format!("_r{k}")).collect();
                w.line(format!("{} = {call}", names.join(", ")));
                let vals = names
                    .iter()
                    .zip(results)
                    .map(|(n, t)| format!("{rt}.Val.{}({n})", val_of(*t)))
                    .collect::<Vec<_>>()
                    .join(", ");
                w.line(format!("return [{vals}]"));
            }
        }
    }

    fn class(&self, w: &mut CodeWriter, class_name: &str) {
        w.line(format!("class {class_name}:"));
        w.indent();
        self.fields(w);
        w.line("");
        self.initialize(w);
        if self.wasi_fallback_used() {
            w.line("");
            self.wasi_accessor(w);
        }
        if self.spec {
            w.line("");
            self.emit_invoke_method(w);
            w.line("");
            self.emit_global_get_method(w);
        }
        for (i, func) in self.module.funcs.iter().enumerate() {
            w.line("");
            let idx = self.module.num_imported_funcs() as usize + i;
            self.function(w, idx as u32, func);
        }
        w.dedent();
    }

    /// Whether any imported function falls back to the bundled WASI (which is what makes the lazy `_wasi` machinery worth emitting).
    fn wasi_fallback_used(&self) -> bool {
        wasi_bundled(self.module, self.default_wasi, bundler())
    }

    /// Codon requires instance attributes declared as class-level annotations; emit one per field the constructor assigns.
    fn fields(&self, w: &mut CodeWriter) {
        let m = self.module;
        let rt = &self.rt_name;
        if m.imported_memory.is_some() || m.memory.is_some() {
            self.use_unit("memory/_class");
            w.line(format!("m: {rt}.Memory"));
        }
        let num_tables = m.imported_tables.len() + m.tables.len();
        for i in 0..num_tables {
            self.use_unit("table/_class");
            w.line(format!("t{i}: {rt}.Table"));
        }
        for (i, imp) in m.imported_globals.iter().enumerate() {
            self.use_unit("global/_class");
            w.line(format!("g{i}: {rt}.Global[{}]", self.ty_str(imp.ty)));
        }
        let nig = m.imported_globals.len();
        for (i, g) in m.globals.iter().enumerate() {
            self.use_unit("global/_class");
            w.line(format!("g{}: {rt}.Global[{}]", nig + i, self.ty_str(g.ty)));
        }
        for i in 0..m.imported_funcs.len() {
            self.use_unit("rt/boxed");
            w.line(format!("if{i}: {rt}.Fn"));
        }
        for i in 0..m.imported_tags.len() + m.tags.len() {
            self.use_unit("rt/boxed");
            w.line(format!("tag{i}: {rt}.Tag"));
        }
        for i in 0..m.elems.len() {
            self.use_unit("rt/boxed");
            w.line(format!("elem{i}: List[Optional[{rt}.Funcref]]"));
        }
        for i in 0..m.datas.len() {
            self.use_unit("rt/data");
            w.line(format!("data{i}: {rt}.Data"));
        }
        for (i, ty) in self.tail_arg_slots() {
            w.line(format!("{}: {}", tail_arg_slot(i, ty), self.ty_str(ty)));
        }
        for sig in self.tail_signatures() {
            let id = sig_id(&sig);
            let class = &self.class_name;
            w.line(format!("_tf_{id}: Optional[{class}_TB_{id}]"));
            w.line(format!("_tb_{id}: List[{class}_TB_{id}]"));
        }
        if self.wasi_fallback_used() {
            self.use_unit("wasi/_class");
            w.line(format!("_wasi: Optional[{rt}.WASI]"));
            w.line("_wasi_args: List[str]");
            w.line("_wasi_env: Dict[str, str]");
            w.line("_wasi_preopens: Dict[str, str]");
        }
        self.use_unit("ext/extern");
        w.line(format!("exports: Dict[str, {rt}.Extern]"));
    }

    fn initialize(&self, w: &mut CodeWriter) {
        let m = self.module;
        let rt = &self.rt_name;
        let wasi_fallback = self.wasi_fallback_used();
        w.line(format!(
            "def __init__(self, imports: Dict[str, Dict[str, {rt}.Extern]], args: List[str], env: Dict[str, str], preopens: Dict[str, str]):"
        ));
        w.indent();
        for (i, ty) in self.tail_arg_slots() {
            w.line(format!(
                "self.{} = {}",
                tail_arg_slot(i, ty),
                zero_value(ty)
            ));
        }
        // Entry tables are built before anything that parks a target or stores a funcref in a table: a tail call reads its target out of here rather than building a closure per hop.
        for sig in self.tail_signatures() {
            let id = sig_id(&sig);
            let class = self.class_name.clone();
            w.line(format!("self._tf_{id} = None"));
            w.line(format!("self._tb_{id} = List[{class}_TB_{id}]()"));
            for f in self
                .tail_entries()
                .iter()
                .filter(|f| self.module.func_type(**f).results == sig)
            {
                w.line(format!("self._tb_{id}.append({class}_TB_{id}_F{f}(self))"));
            }
        }
        if wasi_fallback {
            w.line("self._wasi = None");
            w.line("self._wasi_args = args");
            w.line("self._wasi_env = env");
            w.line("self._wasi_preopens = preopens");
        }

        if let Some(import) = &m.imported_memory {
            self.use_unit("ext/import_memory");
            w.line(format!(
                "self.m = {}",
                self.checked_import("import_memory", &import.module, &import.name)
            ));
        } else if let Some(mem) = &m.memory {
            self.use_unit("memory/_class");
            let max = mem.max_pages.map(|p| p.min(65536)).unwrap_or(65536);
            w.line(format!("self.m = {rt}.Memory({}, {max})", mem.min_pages));
        }

        for (i, import) in m.imported_tables.iter().enumerate() {
            self.use_unit("ext/import_table");
            w.line(format!(
                "self.t{i} = {}",
                self.checked_import("import_table", &import.module, &import.name)
            ));
        }
        let nit = m.num_imported_tables() as usize;
        for (i, table) in m.tables.iter().enumerate() {
            self.use_unit("table/_class");
            let max = table.max.map(|p| p as u64).unwrap_or(u32::MAX as u64);
            w.line(format!(
                "self.t{} = {rt}.Table({}, {max})",
                nit + i,
                table.min
            ));
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            self.use_unit("ext/resolve_import");
            let resolve = format!(
                "{}.resolve_import(imports, {}, {})",
                rt,
                codon_string(&import.module),
                codon_string(&import.name)
            );
            w.line(format!("_e{i} = {resolve}"));
            w.line(format!("if _e{i} is None:"));
            w.indent();
            match self.import_fallback(i) {
                ImportFallback::Wasi => {
                    w.line(format!(
                        "self.if{i} = {}_WW{i}(self._wasi_get())",
                        self.class_name
                    ));
                }
                ImportFallback::Enosys => {
                    w.line(format!("self.if{i} = {}_WS{i}()", self.class_name));
                }
                ImportFallback::LinkError => {
                    self.use_unit("rt/link_error");
                    w.line(format!(
                        "raise {rt}.LinkError({})",
                        codon_string(&format!("missing import {}.{}", import.module, import.name))
                    ));
                }
            }
            w.dedent();
            w.line("else:");
            w.indent();
            self.use_unit("ext/import_fn");
            let ty_key = self.type_symbol(import.type_idx);
            w.line(format!(
                "self.if{i} = {rt}.import_fn(_e{i}, {ty_key}, {}, {})",
                codon_string(&import.module),
                codon_string(&import.name)
            ));
            w.dedent();
        }

        for (i, import) in m.imported_globals.iter().enumerate() {
            let (_, checker) = global_extern(import.ty);
            self.use_unit(&format!("ext/{checker}"));
            w.line(format!(
                "self.g{i} = {}",
                self.checked_import(checker, &import.module, &import.name)
            ));
        }
        let nig = m.imported_globals.len();
        for (i, global) in m.globals.iter().enumerate() {
            self.use_unit("global/_class");
            w.line(format!(
                "self.g{} = {rt}.Global[{}]({})",
                nig + i,
                self.ty_str(global.ty),
                self.expr(&global.init)
            ));
        }

        // Tags: imported first, then defined (index space is imported_tags ++ tags).
        // A defined tag is a fresh identity object; wasm tag equality is identity, never structure.
        for (i, import) in m.imported_tags.iter().enumerate() {
            self.use_unit("ext/import_tag");
            w.line(format!(
                "self.tag{i} = {}",
                self.checked_import("import_tag", &import.module, &import.name)
            ));
        }
        for i in 0..m.tags.len() {
            self.use_unit("rt/boxed");
            w.line(format!(
                "self.tag{} = {rt}.Tag()",
                m.imported_tags.len() + i
            ));
        }

        for (i, elem) in m.elems.iter().enumerate() {
            self.use_unit("rt/boxed");
            match &elem.kind {
                ElemKind::Declared => {
                    w.line(format!("self.elem{i} = List[Optional[{rt}.Funcref]]()"));
                }
                ElemKind::Passive | ElemKind::Active { .. } => {
                    w.line(format!("self.elem{i} = List[Optional[{rt}.Funcref]]()"));
                    for item in &elem.items {
                        w.line(format!("self.elem{i}.append({})", self.elem_item(item)));
                    }
                    if let ElemKind::Active {
                        table_index,
                        offset,
                    } = &elem.kind
                    {
                        self.use_unit("table/init");
                        w.line(format!(
                            "self.t{table_index}.init({}, self.elem{i}, UInt[32](0), UInt[32]({}))",
                            self.expr(offset),
                            elem.items.len()
                        ));
                        w.line(format!("self.elem{i} = List[Optional[{rt}.Funcref]]()"));
                    }
                }
            }
        }

        for (i, data) in m.datas.iter().enumerate() {
            self.use_unit("rt/data");
            self.use_unit("rt/unhex");
            match &data.offset {
                Some(offset) => {
                    self.use_unit("memory/init");
                    w.line(format!(
                        "self.m.init({}, {rt}.unhex({}), UInt[32](0), UInt[32]({}))",
                        self.expr(offset),
                        codon_string(&hex_string(&data.data)),
                        data.data.len()
                    ));
                    w.line(format!("self.data{i} = {rt}.Data(Ptr[byte](1), 0)"));
                }
                None => {
                    w.line(format!(
                        "self.data{i} = {rt}.unhex({})",
                        codon_string(&hex_string(&data.data))
                    ));
                }
            }
        }

        self.use_unit("ext/extern");
        w.line(format!("self.exports = Dict[str, {rt}.Extern]()"));
        for export in &m.exports {
            let value = match export.kind {
                ExportKind::Func(idx) => {
                    let ty_key = self.func_type_symbol(idx);
                    if (idx as usize) < m.imported_funcs.len() {
                        format!("{rt}.Extern.of_fn(self.if{idx}, {ty_key})")
                    } else {
                        format!(
                            "{rt}.Extern.of_fn({}_W{idx}(self), {ty_key})",
                            self.class_name
                        )
                    }
                }
                ExportKind::Global(idx) => {
                    let ty = m.global_type(idx);
                    let (ctor, _) = global_extern(ty);
                    format!("{rt}.Extern.{ctor}(self.g{idx})")
                }
                ExportKind::Table(idx) => format!("{rt}.Extern.of_table(self.t{idx})"),
                ExportKind::Memory => format!("{rt}.Extern.of_memory(self.m)"),
                ExportKind::Tag(idx) => format!("{rt}.Extern.of_tag(self.tag{idx})"),
            };
            w.line(format!(
                "self.exports[{}] = {value}",
                codon_string(&export.name)
            ));
        }

        if let Some(start) = m.start {
            w.line(self.call_only(start));
        }
        w.dedent();
    }

    /// The lazily built bundled WASI, memory bound at creation (the constructor resolves memory before any import).
    fn wasi_accessor(&self, w: &mut CodeWriter) {
        let rt = &self.rt_name;
        self.use_unit("wasi/_class");
        w.line(format!("def _wasi_get(self) -> {rt}.WASI:"));
        w.indent();
        w.line("_w = self._wasi");
        w.line("if _w is not None:");
        w.indent();
        w.line("return _w");
        w.dedent();
        w.line(format!(
            "_w2 = {rt}.WASI(self._wasi_args, self._wasi_env, self._wasi_preopens)"
        ));
        if self.module.memory.is_some() || self.module.imported_memory.is_some() {
            w.line("_w2.memory = self.m");
        }
        w.line("self._wasi = _w2");
        w.line("return _w2");
        w.dedent();
    }

    /// Resolve a non-function import through its typed checker, with a missing import raising a link error inline.
    fn checked_import(&self, checker: &str, module: &str, name: &str) -> String {
        self.use_unit("ext/resolve_import");
        self.use_unit("ext/require");
        self.use_unit("rt/link_error");
        self.use_unit(&format!("ext/{checker}"));
        let rt = &self.rt_name;
        // `resolve_or_missing` would need a per-kind return type; instead the None case is folded into the checker call via a small helper expression.
        format!(
            "{rt}.{checker}({rt}.require({rt}.resolve_import(imports, {m}, {n}), {m}, {n}), {m}, {n})",
            m = codon_string(module),
            n = codon_string(name)
        )
    }

    /// The spec-harness reflective dispatcher: boxed args in, boxed results out, so the harness needs no per-signature phrasing.
    fn emit_invoke_method(&self, w: &mut CodeWriter) {
        let m = self.module;
        let rt = &self.rt_name;
        self.use_unit("rt/boxed");
        self.use_unit("rt/trap");
        w.line(format!(
            "def invoke(self, name: str, a: List[{rt}.Val]) -> List[{rt}.Val]:"
        ));
        w.indent();
        for export in &m.exports {
            let ExportKind::Func(idx) = export.kind else {
                continue;
            };
            let ty = m.func_type(idx).clone();
            w.line(format!("if name == {}:", codon_string(&export.name)));
            w.indent();
            if (idx as usize) < m.imported_funcs.len() {
                w.line(format!("return self.if{idx}.invoke(a)"));
            } else {
                let args = ty
                    .params
                    .iter()
                    .enumerate()
                    .map(|(k, t)| format!("a[{k}].{}()", val_get(*t)))
                    .collect::<Vec<_>>()
                    .join(", ");
                let call = format!("self._f{idx}({args})");
                self.emit_boxed_return(w, &call, &ty.results);
            }
            w.dedent();
        }
        w.line(format!("{}(\"no export \" + name)", self.rt("trap")));
        w.line("return a");
        w.dedent();
    }

    /// The spec-harness global reader: the boxed current value in a one-element list, treated by the harness exactly like a single-result invoke.
    fn emit_global_get_method(&self, w: &mut CodeWriter) {
        let m = self.module;
        let rt = &self.rt_name;
        self.use_unit("rt/boxed");
        self.use_unit("rt/trap");
        w.line(format!(
            "def global_get(self, name: str) -> List[{rt}.Val]:"
        ));
        w.indent();
        let num_imported = m.imported_globals.len();
        for export in &m.exports {
            let ExportKind::Global(idx) = export.kind else {
                continue;
            };
            let ty = if (idx as usize) < num_imported {
                m.imported_globals[idx as usize].ty
            } else {
                m.globals[idx as usize - num_imported].ty
            };
            w.line(format!("if name == {}:", codon_string(&export.name)));
            w.indent();
            w.line(format!(
                "return [{rt}.Val.{}(self.g{idx}.value)]",
                val_of(ty)
            ));
            w.dedent();
        }
        w.line(format!("{}(\"no global \" + name)", self.rt("trap")));
        w.line(format!("return List[{rt}.Val]()"));
        w.dedent();
    }

    /// A funcref value for a table slot / element item.
    fn elem_item(&self, item: &ElemItem) -> String {
        let rt = &self.rt_name;
        match item {
            ElemItem::Func(idx) => {
                let fn_expr = if (*idx as usize) < self.module.imported_funcs.len() {
                    format!("self.if{idx}")
                } else {
                    format!("{}_W{idx}(self)", self.class_name)
                };
                // A tail-calling function's funcref carries its entry-table position, so a chain through the table stays flat within the owning instance.
                let slot = self.tail_slot(*idx).map(|k| k as i64).unwrap_or(-1);
                format!(
                    "{rt}.Funcref({}, {fn_expr}, id(self), {slot})",
                    self.func_type_symbol(*idx)
                )
            }
            ElemItem::Null => "None".to_string(),
            // A `global.get` element item needs a ref-typed immutable global, i.e. reference types (rejected at conversion); unreachable here.
            ElemItem::Global(_) => unreachable!("ref-typed global element item"),
        }
    }

    /// A statement calling function `func_idx` with no arguments and discarding results (the start function).
    fn call_only(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            let rt = &self.rt_name;
            self.use_unit("rt/boxed");
            format!("self.if{func_idx}.invoke(List[{rt}.Val]())")
        } else {
            format!("self._f{func_idx}()")
        }
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &Func) {
        let ty = &self.module.types[func.type_idx as usize];
        let mut params = String::new();
        for (i, t) in ty.params.iter().enumerate() {
            params.push_str(&format!(", l{i}: {}", self.ty_str(*t)));
        }
        let ret = match ty.results.as_slice() {
            [] => String::new(),
            [t] => format!(" -> {}", self.ty_str(*t)),
            ts => format!(
                " -> Tuple[{}]",
                ts.iter()
                    .map(|t| self.ty_str(*t))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        // A tail-calling function's real code lives in `_f{idx}_body`, which parks the next call in the instance's slots instead of growing the stack; the public `_f{idx}` is the trampoline that runs the chain, so no call site changes.
        let is_tail_caller = self.tail_callers.contains(&idx);
        if is_tail_caller {
            let id = sig_id(&ty.results);
            let args = (0..ty.params.len())
                .map(|i| format!("l{i}"))
                .collect::<Vec<_>>()
                .join(", ");
            w.line(format!("def _f{idx}(self{params}){ret}:"));
            w.indent();
            if ty.results.is_empty() {
                w.line(format!("self._f{idx}_body({args})"));
            } else {
                w.line(format!("_r = self._f{idx}_body({args})"));
            }
            w.line("while True:");
            w.indent();
            w.line(format!("_t = self._tf_{id}"));
            w.line("if _t is None:");
            w.indent();
            w.line("break");
            w.dedent();
            w.line(format!("self._tf_{id} = None"));
            if ty.results.is_empty() {
                w.line("_t.run()");
            } else {
                w.line("_r = _t.run()");
            }
            w.dedent();
            if !ty.results.is_empty() {
                w.line("return _r");
            }
            w.dedent();
            w.line("");
        }
        let fname = if is_tail_caller {
            format!("_f{idx}_body")
        } else {
            format!("_f{idx}")
        };
        w.line(format!("def {fname}(self{params}){ret}:"));
        w.indent();

        // Recursion guard (spec builds only): see SPEC_STACK_LIMIT.
        let guard_cost = 1 + ty.params.len() + func.locals.len() + func.temps.len();
        if self.spec {
            w.line("global _rt_stack");
            w.line(format!("_rt_stack += {guard_cost}"));
            w.line(format!("if _rt_stack > {SPEC_STACK_LIMIT}:"));
            w.indent();
            w.line(format!("_rt_stack -= {guard_cost}"));
            w.line(format!("{}(\"call stack exhausted\")", self.rt("trap")));
            w.dedent();
            w.line("try:");
            w.indent();
        }

        for (i, t) in func.locals.iter().enumerate() {
            w.line(format!("l{} = {}", ty.params.len() + i, zero_value(*t)));
        }
        for t in &func.temps {
            w.line(format!("{} = {}", temp(*t), zero_value(t.ty)));
        }
        if self.seq_has_relay_branch(&func.body) {
            w.line("_br = 0");
        }
        let mut guarded = false;
        self.emit_seq(w, &func.body, &mut guarded, true);
        // Codon is statically typed: a function with results must return on every path the compiler sees, and the branch-register rendering can always fall off the end.
        if !ty.results.is_empty() {
            self.emit_zero_return(w, &ty.results);
        }

        if self.spec {
            w.dedent();
            w.line("finally:");
            w.indent();
            w.line(format!("_rt_stack -= {guard_cost}"));
            w.dedent();
            if !ty.results.is_empty() {
                self.emit_zero_return(w, &ty.results);
            }
        }
        w.dedent();
    }

    fn emit_zero_return(&self, w: &mut CodeWriter, results: &[ValType]) {
        match results {
            [] => w.line("return"),
            [t] => w.line(format!("return {}", zero_value(*t))),
            ts => {
                let zeros = ts.iter().map(|t| zero_value(*t)).collect::<Vec<_>>();
                w.line(format!("return ({})", zeros.join(", ")));
            }
        }
    }

    /// Whether `stmts` holds a branch that travels through `_br` (i.e. whether the function needs the branch register at all).
    fn seq_has_relay_branch(&self, stmts: &[Stmt]) -> bool {
        let relays = |t: &BrTarget| matches!(t, BrTarget::Label { .. });
        Stmt::any(stmts, &mut |stmt| match stmt {
            Stmt::Br(t) | Stmt::BrIf { target: t, .. } => relays(t),
            Stmt::BrTable {
                targets, default, ..
            } => relays(default) || targets.iter().any(relays),
            // A catch clause's own branch relays through `_br` too, even when the body underneath never does.
            Stmt::TryTable { catches, .. } => catches.iter().any(|c| relays(&c.target)),
            _ => false,
        })
    }

    /// Emit a statement sequence, threading the compile-time `guarded` flag (whether a preceding statement may have left a branch pending in `_br`).
    /// Block/Loop bodies are spliced inline so block nesting adds no host-language nesting; only real loops become `while True:`.
    /// The region discipline is the Python backend's: once guarded, one `if _br == 0:` suite is opened lazily and every following statement is emitted inside it unguarded, until a statement carrying a free branch ends the run.
    ///
    /// `tail` says nothing runs after this sequence before the function falls off; a landing marker there writes a register nothing reads again, so it is skipped.
    /// Returns the sequence's free branch targets (label ids it branches to that are not bound within it).
    fn emit_seq(
        &self,
        w: &mut CodeWriter,
        stmts: &[Stmt],
        guarded: &mut bool,
        tail: bool,
    ) -> BTreeSet<u32> {
        let mut free = BTreeSet::new();
        let mut open = false;
        let last_emit = stmts.iter().rposition(stmt_emits);
        let mut i = 0;
        while i < stmts.len() {
            let stmt = &stmts[i];
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
                    // The body needs a real host scope: catching an exception raised anywhere inside it (a callee included) is not expressible through the `_br` register alone.
                    // The wrapping `while True:` gives every catch clause's own branch somewhere to `break` to; the body itself keeps using `_br` for any branch it contains, same as a Block's (the Python backend's shape).
                    self.use_unit("rt/boxed");
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

    /// Fuse a call-family producer with the adjacent statement that consumes its whole result (see the Python backend; the shapes and the soundness argument are the same).
    fn fused_call_line(&self, producer: &Stmt, consumer: Option<&Stmt>) -> Option<String> {
        let (produced, call) = match producer {
            Stmt::Call {
                func,
                args,
                results,
            } => {
                let &[t] = &results[..] else { return None };
                (t, self.call_expr_single(*func, args, t)?)
            }
            Stmt::CallIndirect {
                type_idx,
                table_index,
                index,
                args,
                results,
            } => {
                let &[t] = &results[..] else { return None };
                (
                    t,
                    self.call_indirect_expr_single(*type_idx, *table_index, index, args, t),
                )
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                (*dst, format!("self.m.grow({})", self.expr(delta)))
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

    /// A single-result direct call as one expression: native for a defined function, boxed-and-unboxed for an imported one.
    fn call_expr_single(&self, func: u32, args: &[Expr], result: Temp) -> Option<String> {
        if (func as usize) < self.module.imported_funcs.len() {
            Some(format!(
                "{}[0].{}()",
                self.boxed_call(func, args),
                val_get(result.ty)
            ))
        } else {
            let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
            Some(format!("self._f{func}({})", args.join(", ")))
        }
    }

    /// A single-result `call_indirect` as one expression (always boxed).
    fn call_indirect_expr_single(
        &self,
        type_idx: u32,
        table_index: u32,
        index: &Expr,
        args: &[Expr],
        result: Temp,
    ) -> String {
        format!(
            "{}[0].{}()",
            self.indirect_invoke(type_idx, table_index, index, args),
            val_get(result.ty)
        )
    }

    /// The boxed invoke of imported function `func`.
    fn boxed_call(&self, func: u32, args: &[Expr]) -> String {
        let rt = &self.rt_name;
        self.use_unit("rt/boxed");
        if args.is_empty() {
            format!("self.if{func}.invoke(List[{rt}.Val]())")
        } else {
            let ty = self.module.func_type(func).clone();
            let vals = args
                .iter()
                .zip(&ty.params)
                .map(|(a, t)| format!("{rt}.Val.{}({})", val_of(*t), self.expr(a)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("self.if{func}.invoke([{vals}])")
        }
    }

    /// The boxed invoke of a table slot.
    fn indirect_invoke(
        &self,
        type_idx: u32,
        table_index: u32,
        index: &Expr,
        args: &[Expr],
    ) -> String {
        self.use_unit("table/call");
        self.use_unit("rt/boxed");
        let rt = &self.rt_name;
        let ty = self.module.types[type_idx as usize].clone();
        let target = format!(
            "self.t{table_index}.call({}, {})",
            self.expr(index),
            self.type_symbol(type_idx)
        );
        if args.is_empty() {
            format!("{target}.invoke(List[{rt}.Val]())")
        } else {
            let vals = args
                .iter()
                .zip(&ty.params)
                .map(|(a, t)| format!("{rt}.Val.{}({})", val_of(*t), self.expr(a)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{target}.invoke([{vals}])")
        }
    }

    /// Emit the statements `stmt_emits` deems code-free: a comment renders, an empty construct vanishes.
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

    fn emit_if(
        &self,
        w: &mut CodeWriter,
        cond: &Expr,
        then: &[Stmt],
        els: &[Stmt],
        tail: bool,
    ) -> BTreeSet<u32> {
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
                let method = self.mem(store_method(*op));
                w.line(format!(
                    "self.m.{method}({}, {})",
                    self.addr(addr, *offset),
                    self.expr(value)
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
            } => self.emit_call_results(w, *func, args, results),
            Stmt::CallIndirect {
                type_idx,
                table_index,
                index,
                args,
                results,
            } => {
                let call = self.indirect_invoke(*type_idx, *table_index, index, args);
                self.emit_boxed_results(w, &call, results);
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                w.line(format!(
                    "{} = self.m.grow({})",
                    temp(*dst),
                    self.expr(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "self.m.copy({}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "self.m.fill({}, {}, {})",
                    self.expr(dst),
                    self.expr(val),
                    self.expr(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "self.m.init({}, self.data{seg}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::DataDrop { seg } => {
                let rt = &self.rt_name;
                self.use_unit("rt/data");
                w.line(format!("self.data{seg} = {rt}.Data(Ptr[byte](1), 0)"));
            }
            Stmt::Unreachable => {
                w.line(format!("{}(\"unreachable\")", self.rt("trap")));
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
                let rt = &self.rt_name;
                self.use_unit("rt/boxed");
                w.line(format!("self.elem{seg} = List[Optional[{rt}.Funcref]]()"));
            }
            Stmt::Throw { tag, args } => {
                self.use_unit("rt/boxed");
                let params = self.module.tag_params(*tag).to_vec();
                let mut ks = Vec::new();
                let mut bits = Vec::new();
                let mut fs = Vec::new();
                let mut ss = Vec::new();
                for (a, t) in args.iter().zip(&params) {
                    let r = self.expr(a);
                    match t {
                        ValType::I32 => {
                            ks.push("0".to_string());
                            bits.push(format!("UInt[64]({r})"));
                            fs.push("0.0".to_string());
                            ss.push("float32(0.0)".to_string());
                        }
                        ValType::I64 => {
                            ks.push("1".to_string());
                            bits.push(r);
                            fs.push("0.0".to_string());
                            ss.push("float32(0.0)".to_string());
                        }
                        ValType::F32 => {
                            ks.push("2".to_string());
                            bits.push("UInt[64](0)".to_string());
                            fs.push("0.0".to_string());
                            ss.push(r);
                        }
                        ValType::F64 => {
                            ks.push("3".to_string());
                            bits.push("UInt[64](0)".to_string());
                            fs.push(r);
                            ss.push("float32(0.0)".to_string());
                        }
                        // Tag parameters of reference type are not carried (see rt/boxed).
                        ValType::FuncRef | ValType::ExnRef => {
                            unreachable!("reference-typed tag parameter")
                        }
                    }
                }
                let list = |items: &[String], empty: &str| {
                    if items.is_empty() {
                        empty.to_string()
                    } else {
                        format!("[{}]", items.join(", "))
                    }
                };
                w.line(format!(
                    "raise {}.WasmException(self.tag{tag}, {}, {}, {}, {})",
                    self.rt_name,
                    list(&ks, "List[int]()"),
                    list(&bits, "List[UInt[64]]()"),
                    list(&fs, "List[float]()"),
                    list(&ss, "List[float32]()")
                ));
            }
            Stmt::ThrowRef { exn } => {
                w.line(format!("{}({})", self.rt("throw_ref"), self.expr(exn)));
            }
            // Parked, never called: the callee must run once this frame (any enclosing handler included) is gone, and returning is what unwinds them.
            // The target is the callee's tail entry, built once at instantiation, so a hop allocates nothing.
            Stmt::ReturnCall { func, args } => {
                let args_r: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                let fty = self.module.func_type(*func).clone();
                let id = sig_id(&fty.results);
                match self.tail_slot(*func) {
                    Some(k) => {
                        for (i, (a, t)) in args_r.iter().zip(&fty.params).enumerate() {
                            w.line(format!("self.{} = {a}", tail_arg_slot(i, *t)));
                        }
                        w.line(format!("self._tf_{id} = self._tb_{id}[{k}]"));
                    }
                    // An imported callee has no typed entry: park it boxed.
                    None => {
                        let vals = self.boxed_vals(&args_r, &fty.params);
                        // Through an annotated local: Codon upcasts a subclass into a base-typed binding, but not directly into an Optional[base] field.
                        w.line(format!(
                            "_tx_{id}: {cls}_TB_{id} = {cls}_TB_{id}_X(self.if{func}, {vals})",
                            cls = self.class_name
                        ));
                        w.line(format!("self._tf_{id} = _tx_{id}"));
                    }
                }
                self.emit_zero_return(w, &fty.results);
            }
            Stmt::ReturnCallIndirect {
                type_idx,
                table_index,
                index,
                args,
            } => {
                self.use_unit("table/slot");
                self.use_unit("rt/boxed");
                let ty = self.module.types[*type_idx as usize].clone();
                let id = sig_id(&ty.results);
                // The slot is resolved, and its traps raised, here rather than after the frame is gone: an indirect tail call's checks happen at the instruction.
                w.line(format!(
                    "_fr = self.t{table_index}.slot({}, {})",
                    self.expr(index),
                    self.type_symbol(*type_idx)
                ));
                let args_r: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                // A tail entry belongs to the instance that built it and reads *its* parked slots, so only this instance's own entries can be parked; anything else completes here instead.
                w.line("if _fr.owner == id(self) and _fr.tail_slot >= 0:");
                w.indent();
                for (i, (a, t)) in args_r.iter().zip(&ty.params).enumerate() {
                    w.line(format!("self.{} = {a}", tail_arg_slot(i, *t)));
                }
                w.line(format!("self._tf_{id} = self._tb_{id}[_fr.tail_slot]"));
                w.dedent();
                w.line("else:");
                w.indent();
                // Another instance's function (or an own funcref without an entry): park it boxed, so the frame and its handlers are still gone before the callee runs.
                let vals = self.boxed_vals(&args_r, &ty.params);
                // Through an annotated local: Codon upcasts a subclass into a base-typed binding, but not directly into an Optional[base] field.
                w.line(format!(
                    "_tx_{id}: {cls}_TB_{id} = {cls}_TB_{id}_X(_fr.fn, {vals})",
                    cls = self.class_name
                ));
                w.line(format!("self._tf_{id} = _tx_{id}"));
                w.dedent();
                self.emit_zero_return(w, &ty.results);
            }
            Stmt::TryTable { .. } | Stmt::Block { .. } | Stmt::Loop { .. } | Stmt::If { .. } => {
                unreachable!("structured statement routed to simple_stmt")
            }
        }
    }

    /// A direct call with results assigned: native for defined functions, boxed for imports.
    fn emit_call_results(&self, w: &mut CodeWriter, func: u32, args: &[Expr], results: &[Temp]) {
        if (func as usize) < self.module.imported_funcs.len() {
            let call = self.boxed_call(func, args);
            self.emit_boxed_results(w, &call, results);
        } else {
            let args: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
            let call = format!("self._f{func}({})", args.join(", "));
            match results {
                [] => w.line(call),
                [r] => w.line(format!("{} = {call}", temp(*r))),
                rs => {
                    let names = rs.iter().map(|r| temp(*r)).collect::<Vec<_>>().join(", ");
                    w.line(format!("{names} = {call}"));
                }
            }
        }
    }

    /// Unbox a boxed call's `List[Val]` into result temps.
    fn emit_boxed_results(&self, w: &mut CodeWriter, call: &str, results: &[Temp]) {
        match results {
            [] => w.line(call),
            [r] => w.line(format!("{} = {call}[0].{}()", temp(*r), val_get(r.ty))),
            rs => {
                w.line(format!("_ir = {call}"));
                for (k, r) in rs.iter().enumerate() {
                    w.line(format!("{} = _ir[{k}].{}()", temp(*r), val_get(r.ty)));
                }
            }
        }
    }

    /// A boxed argument list from already-rendered expressions.
    fn boxed_vals(&self, args: &[String], params: &[ValType]) -> String {
        self.use_unit("rt/boxed");
        let rt = &self.rt_name;
        if args.is_empty() {
            return format!("List[{rt}.Val]()");
        }
        let items = args
            .iter()
            .zip(params)
            .map(|(a, t)| format!("{rt}.Val.{}({a})", val_of(*t)))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{items}]")
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

    /// One `try_table` catch clause inside the handler: bind the payload into the target frame's slots, then take the branch.
    /// A tagged clause guards that with `is` (wasm tag equality is object identity); a catch-all runs unconditionally.
    /// `branch()` alone never leaves the `except` suite, so this appends the `break` that exits the `try_table`'s wrapping `while True:` itself (dead, but harmless, right after a `return`).
    fn catch_clause(&self, w: &mut CodeWriter, clause: &dewasm_core::ir::CatchClause) {
        let bind_and_branch = |gen: &Self, w: &mut CodeWriter| {
            for (i, t) in clause.value_temps.iter().enumerate() {
                if Some(*t) == clause.exn_temp {
                    w.line(format!("{} = e", temp(*t)));
                } else {
                    let read = match t.ty {
                        ValType::I32 => format!("UInt[32](e.bits[{i}])"),
                        ValType::I64 => format!("e.bits[{i}]"),
                        ValType::F32 => format!("e.ss[{i}]"),
                        ValType::F64 => format!("e.fs[{i}]"),
                        ValType::FuncRef | ValType::ExnRef => {
                            unreachable!("reference-typed tag parameter")
                        }
                    };
                    w.line(format!("{} = {read}", temp(*t)));
                }
            }
            gen.branch(w, &clause.target);
            w.line("break");
        };
        match clause.tag {
            Some(tag) => {
                w.line(format!("if e.tag is self.tag{tag}:"));
                w.indent();
                bind_and_branch(self, w);
                w.dedent();
            }
            None => bind_and_branch(self, w),
        }
    }

    fn branch(&self, w: &mut CodeWriter, target: &BrTarget) {
        match target {
            BrTarget::Return { values } => self.return_stmt(w, values),
            BrTarget::Label { label, assigns, .. } => {
                for (dst, src) in assigns {
                    w.line(format!("{} = {}", temp(*dst), temp(*src)));
                }
                w.line(format!("_br = {label}"));
            }
        }
    }

    /// Record a branch target's label id into `free`, returning whether it is a label branch (one that travels through `_br`).
    fn collect_target_free(&self, t: &BrTarget, free: &mut BTreeSet<u32>) -> bool {
        match t {
            BrTarget::Return { .. } => false,
            BrTarget::Label { label, .. } => {
                free.insert(*label);
                true
            }
        }
    }

    /// Add the label ids a non-structured statement branches to into `free`, returning whether it has any (i.e. whether it may leave `_br` set on fall-through).
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

    /// The address argument of a memory unit: an `int` (64-bit) effective address, exact however large the static offset is.
    fn addr(&self, addr: &Expr, offset: u64) -> String {
        if let Expr::I32Const(base) = addr {
            return (u64::from(*base) + offset).to_string();
        }
        if offset == 0 {
            format!("int({})", self.expr(addr))
        } else {
            format!("int({}) + {offset}", self.expr(addr))
        }
    }

    fn expr(&self, expr: &Expr) -> String {
        match expr {
            Expr::I32Const(v) => i32_const(*v),
            Expr::I64Const(v) => i64_const(*v),
            Expr::F32Const(bits) => {
                let v = f32::from_bits(*bits);
                if v.is_finite() {
                    format!("float32({})", codon_float(v as f64))
                } else {
                    format!("{}(UInt[32](0x{bits:X}))", self.rt("f32_from_bits"))
                }
            }
            Expr::F64Const(bits) => {
                let v = f64::from_bits(*bits);
                if v.is_finite() {
                    codon_float(v)
                } else {
                    format!("{}(UInt[64](0x{bits:X}))", self.rt("f64_from_bits"))
                }
            }
            Expr::Temp(t) => temp(*t),
            Expr::LocalGet(idx) => format!("l{idx}"),
            Expr::GlobalGet(idx) => format!("self.g{idx}.value"),
            // `eqz` of something already emitted as a boolean: read the wasm 0/1 straight off that boolean.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) if is_boolean(a) => {
                format!("(UInt[32](0) if {} else UInt[32](1))", self.cond(a))
            }
            Expr::Un(op, a) => self.un(*op, &self.expr(a)),
            Expr::Bin(op, a, b) => self.bin(*op, a, b),
            Expr::Load { op, addr, offset } => {
                let method = self.mem(load_method(*op));
                format!("self.m.{method}({})", self.addr(addr, *offset))
            }
            Expr::Select { cond, then, els } => {
                format!(
                    "({} if {} else {})",
                    self.expr(then),
                    self.cond(cond),
                    self.expr(els)
                )
            }
            Expr::MemorySize => {
                self.use_unit("memory/size");
                "self.m.size()".to_string()
            }
        }
    }

    /// An expression in boolean context (an `if`/`br_if` test): a comparison renders as its own boolean, anything else as `!= 0`.
    fn cond(&self, e: &Expr) -> String {
        match e {
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => self.not_cond(a),
            Expr::Bin(op, a, b) => match comparison(*op) {
                Some(rel) => self.rel(rel, a, b),
                None => format!("({}) != 0", self.expr(e)),
            },
            _ => format!("({}) != 0", self.expr(e)),
        }
    }

    /// The negation of [`cond`]: `e` is zero.
    /// A comparison is negated as a whole rather than by flipping its operator, which would be wrong for floats.
    fn not_cond(&self, e: &Expr) -> String {
        match e {
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => self.cond(a),
            Expr::Bin(op, ..) if comparison(*op).is_some() => {
                format!("not ({})", self.cond(e))
            }
            _ => format!("({}) == 0", self.expr(e)),
        }
    }

    /// A comparison as a host boolean: unsigned and float compares are the stored representation's own, signed ones go through `Int[N]` casts.
    fn rel(&self, (r, operands): (&'static str, CompareOperands), a: &Expr, b: &Expr) -> String {
        let (ra, rb) = (self.expr(a), self.expr(b));
        match operands {
            CompareOperands::Float
            | CompareOperands::IntEq
            | CompareOperands::Unsigned32
            | CompareOperands::Unsigned64 => format!("{ra} {r} {rb}"),
            CompareOperands::Signed32 => format!("Int[32]({ra}) {r} Int[32]({rb})"),
            CompareOperands::Signed64 => format!("Int[64]({ra}) {r} Int[64]({rb})"),
        }
    }

    fn un(&self, op: UnOp, a: &str) -> String {
        use UnOp::*;
        match op {
            I32Eqz => format!("(UInt[32](1) if ({a}) == 0 else UInt[32](0))"),
            I64Eqz => format!("(UInt[32](1) if ({a}) == 0 else UInt[32](0))"),
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
            I32WrapI64 => format!("UInt[32]({a})"),
            I32TruncF32S => format!("{}(float({a}))", self.rt("i32_trunc_s")),
            I32TruncF64S => format!("{}({a})", self.rt("i32_trunc_s")),
            I32TruncF32U => format!("{}(float({a}))", self.rt("i32_trunc_u")),
            I32TruncF64U => format!("{}({a})", self.rt("i32_trunc_u")),
            I64TruncF32S => format!("{}(float({a}))", self.rt("i64_trunc_s")),
            I64TruncF64S => format!("{}({a})", self.rt("i64_trunc_s")),
            I64TruncF32U => format!("{}(float({a}))", self.rt("i64_trunc_u")),
            I64TruncF64U => format!("{}({a})", self.rt("i64_trunc_u")),
            I32TruncSatF32S => format!("{}(float({a}))", self.rt("i32_trunc_sat_s")),
            I32TruncSatF64S => format!("{}({a})", self.rt("i32_trunc_sat_s")),
            I32TruncSatF32U => format!("{}(float({a}))", self.rt("i32_trunc_sat_u")),
            I32TruncSatF64U => format!("{}({a})", self.rt("i32_trunc_sat_u")),
            I64TruncSatF32S => format!("{}(float({a}))", self.rt("i64_trunc_sat_s")),
            I64TruncSatF64S => format!("{}({a})", self.rt("i64_trunc_sat_s")),
            I64TruncSatF32U => format!("{}(float({a}))", self.rt("i64_trunc_sat_u")),
            I64TruncSatF64U => format!("{}({a})", self.rt("i64_trunc_sat_u")),
            I64ExtendI32S => format!("UInt[64](Int[64](Int[32]({a})))"),
            I64ExtendI32U => format!("UInt[64]({a})"),
            F32ConvertI32S => format!("float32(Int[32]({a}))"),
            F32ConvertI32U => format!("float32({a})"),
            F32ConvertI64S => format!("{}({a})", self.rt("f32_convert_i64_s")),
            F32ConvertI64U => format!("{}({a})", self.rt("f32_convert_i64_u")),
            F64ConvertI32S => format!("float(Int[32]({a}))"),
            F64ConvertI32U => format!("float({a})"),
            F64ConvertI64S => format!("float(Int[64]({a}))"),
            F64ConvertI64U => format!("float({a})"),
            F32DemoteF64 => format!("{}({a})", self.rt("f32_demote")),
            F64PromoteF32 => format!("{}({a})", self.rt("f64_promote")),
            I32ReinterpretF32 => format!("{}({a})", self.rt("f32_bits")),
            I64ReinterpretF64 => format!("{}({a})", self.rt("f64_bits")),
            F32ReinterpretI32 => format!("{}({a})", self.rt("f32_from_bits")),
            F64ReinterpretI64 => format!("{}({a})", self.rt("f64_from_bits")),
            I32Extend8S => format!("UInt[32](Int[32](Int[8]({a})))"),
            I32Extend16S => format!("UInt[32](Int[32](Int[16]({a})))"),
            I64Extend8S => format!("UInt[64](Int[64](Int[8]({a})))"),
            I64Extend16S => format!("UInt[64](Int[64](Int[16]({a})))"),
            I64Extend32S => format!("UInt[64](Int[64](Int[32]({a})))"),
        }
    }

    fn bin(&self, op: BinOp, a: &Expr, b: &Expr) -> String {
        use BinOp::*;
        if let Some(rel) = comparison(op) {
            return format!("(UInt[32](1) if {} else UInt[32](0))", self.rel(rel, a, b));
        }
        // A float operation with a constant operand may be folded to its other operand by LLVM (x * 1.0, x + -0.0, ...), skipping the signaling-NaN quieting wasm requires; the quiet-if-NaN wrapper restores it.
        let quiet32 = matches!(op, F32Add | F32Sub | F32Mul | F32Div)
            && (matches!(a, Expr::F32Const(_)) || matches!(b, Expr::F32Const(_)));
        let quiet64 = matches!(op, F64Add | F64Sub | F64Mul | F64Div)
            && (matches!(a, Expr::F64Const(_)) || matches!(b, Expr::F64Const(_)));
        let (ra, rb) = (self.expr(a), self.expr(b));
        let raw = match op {
            I32Add | I64Add => format!("({ra} + {rb})"),
            I32Sub | I64Sub => format!("({ra} - {rb})"),
            I32Mul | I64Mul => format!("({ra} * {rb})"),
            I32DivS => format!("{}({ra}, {rb})", self.rt("i32_div_s")),
            I32DivU => format!("{}({ra}, {rb})", self.rt("i32_div_u")),
            I32RemS => format!("{}({ra}, {rb})", self.rt("i32_rem_s")),
            I32RemU => format!("{}({ra}, {rb})", self.rt("i32_rem_u")),
            I64DivS => format!("{}({ra}, {rb})", self.rt("i64_div_s")),
            I64DivU => format!("{}({ra}, {rb})", self.rt("i64_div_u")),
            I64RemS => format!("{}({ra}, {rb})", self.rt("i64_rem_s")),
            I64RemU => format!("{}({ra}, {rb})", self.rt("i64_rem_u")),
            I32And | I64And => format!("({ra} & {rb})"),
            I32Or | I64Or => format!("({ra} | {rb})"),
            I32Xor | I64Xor => format!("({ra} ^ {rb})"),
            I32Shl => format!("({ra} << ({rb} & UInt[32](31)))"),
            I32ShrU => format!("({ra} >> ({rb} & UInt[32](31)))"),
            I32ShrS => format!("UInt[32](Int[32]({ra}) >> Int[32]({rb} & UInt[32](31)))"),
            I64Shl => format!("({ra} << ({rb} & UInt[64](63)))"),
            I64ShrU => format!("({ra} >> ({rb} & UInt[64](63)))"),
            I64ShrS => format!("UInt[64](Int[64]({ra}) >> Int[64]({rb} & UInt[64](63)))"),
            I32Rotl => format!("{}({ra}, {rb})", self.rt("i32_rotl")),
            I32Rotr => format!("{}({ra}, {rb})", self.rt("i32_rotr")),
            I64Rotl => format!("{}({ra}, {rb})", self.rt("i64_rotl")),
            I64Rotr => format!("{}({ra}, {rb})", self.rt("i64_rotr")),
            F32Add => format!("({ra} + {rb})"),
            F32Sub => format!("({ra} - {rb})"),
            F32Mul => format!("({ra} * {rb})"),
            F32Div => format!("{}({ra}, {rb})", self.rt("f32_div")),
            F64Add => format!("({ra} + {rb})"),
            F64Sub => format!("({ra} - {rb})"),
            F64Mul => format!("({ra} * {rb})"),
            F64Div => format!("{}({ra}, {rb})", self.rt("f64_div")),
            F32Min => format!("{}({ra}, {rb})", self.rt("f32_min")),
            F64Min => format!("{}({ra}, {rb})", self.rt("f64_min")),
            F32Max => format!("{}({ra}, {rb})", self.rt("f32_max")),
            F64Max => format!("{}({ra}, {rb})", self.rt("f64_max")),
            F32Copysign => format!("{}({ra}, {rb})", self.rt("f32_copysign")),
            F64Copysign => format!("{}({ra}, {rb})", self.rt("f64_copysign")),
            _ => unreachable!("op {op:?} is a comparison, rendered by `rel`"),
        };
        if quiet32 {
            format!("{}({raw})", self.rt("f32_q"))
        } else if quiet64 {
            format!("{}({raw})", self.rt("f64_q"))
        } else {
            raw
        }
    }
}

/// The kind of fallback an imported function takes when the embedder does not provide it.
enum ImportFallback {
    /// The bundled WASI unit, wrapped per import.
    Wasi,
    /// An ENOSYS stub, wrapped per import.
    Enosys,
    /// A missing non-WASI import is a link error at instantiation.
    LinkError,
}

/// Whether `stmt` emits at least one real statement (see the Python backend: a comment or an all-comment construct must not open a region guard, whose suite would be empty).
fn stmt_emits(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::SourceLine(_) => false,
        Stmt::Block { label, body } | Stmt::Loop { label, body } => {
            label.referenced || body.iter().any(stmt_emits)
        }
        _ => true,
    }
}

/// Lint for the runtime units: every reference a unit body makes to another unit must be declared in its `# requires:` header.
/// Mirrors the Python backend's units lint, adjusted for this runtime's shapes (`Rt.<name>` staticmethod/class references, `m.<name>` memory-method calls in the WASI units, `self.<name>(...)` sibling calls within a scope).
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
        let rt_class = Regex::new(r"Rt\.([A-Z]\w*)").unwrap();
        // WASI units reach the memory through the `m = self.memory` local.
        let memory_call = Regex::new(r"\bm\.([a-z_]\w*)\(").unwrap();
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
                if dep.ends_with("/_class") {
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
                let n = &cap[1];
                let dep = if n == "resolve_import" || n == "require" || n.starts_with("import_") {
                    format!("ext/{n}")
                } else {
                    format!("rt/{n}")
                };
                demand(dep, &format!("Rt.{n}"));
            }
            for cap in rt_class.captures_iter(&code) {
                let dep = match &cap[1] {
                    "Trap" => "rt/trap".to_string(),
                    "Exit" => "rt/exit".to_string(),
                    "LinkError" => "rt/link_error".to_string(),
                    "Val" => "rt/boxed".to_string(),
                    "Fn" => "rt/boxed".to_string(),
                    "Funcref" => "rt/boxed".to_string(),
                    "Extern" => "ext/extern".to_string(),
                    "Tag" => "rt/boxed".to_string(),
                    "WasmException" => "rt/boxed".to_string(),
                    "Data" => "rt/data".to_string(),
                    "Stat" => "rt/stat".to_string(),
                    "WasiFd" => "rt/wasi_fd".to_string(),
                    "Memory" => "memory/_class".to_string(),
                    "Table" => "table/_class".to_string(),
                    "Global" => "global/_class".to_string(),
                    "WASI" => "wasi/_class".to_string(),
                    other => panic!("{}: unknown runtime class Rt.{other}", unit.id),
                };
                demand(dep, &format!("Rt.{}", &cap[1]));
            }
            if scope == "wasi" {
                for cap in memory_call.captures_iter(&code) {
                    demand(format!("memory/{}", &cap[1]), &format!("m.{}", &cap[1]));
                }
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

    /// The whole runtime (every unit, not just the subset a given module uses) must be valid Codon.
    /// Type-check the full bundle with `codon build` (a missing toolchain fails loud, it does not skip).
    #[test]
    fn all_units_compile_as_codon() {
        let codon = find_codon()
            .expect("codon toolchain not found on PATH (or $DEWASM_CODON): see docs/testing.md");
        let bundle = bundler().bundle_all(1).expect("full bundle assembles");
        let mut source = String::from("class Rt:\n");
        source.push_str(&bundle);
        source.push('\n');
        // Instantiate the generic pieces so the type checker realizes them.
        source.push_str("_g = Rt.Global[UInt[32]](UInt[32](0))\n");
        source.push_str("_m = Rt.Memory(1, 2)\n");
        source.push_str("_t = Rt.Table(1, 2)\n");
        source.push_str("_w = Rt.WASI(List[str](), Dict[str, str](), Dict[str, str]())\n");
        source.push_str("_w.memory = _m\n");
        source.push_str("_e = Rt.Extern.of_memory(_m)\n");
        let dir = std::env::temp_dir().join(format!("dewasm-codon-units-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("bundle.codon");
        std::fs::write(&src, &source).unwrap();
        let out = std::process::Command::new(&codon)
            .arg("build")
            .arg("-o")
            .arg(dir.join("bundle_bin"))
            .arg(&src)
            .output()
            .expect("spawn codon build");
        assert!(
            out.status.success(),
            "full runtime bundle failed to compile:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
