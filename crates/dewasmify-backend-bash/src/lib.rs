//! Bash backend: translates dewasmify IR into a sourceable Bash script
//! plus a bundled runtime of flat `rt_*`/`mem_*` functions.
//!
//! Lowering conventions (ADR-11; numeric conventions ADR-2):
//! - i32 is a masked-unsigned value in [0, 2^32); i64 is stored as its
//!   signed-64 two's-complement bit pattern (bash arithmetic is signed
//!   64-bit only). Signed/unsigned views are derived inline.
//! - f32/f64 are their bit patterns (u32 / signed-64), computed by the
//!   pure-Bash softfloat units (ADR-5/ADR-13); reinterprets and float
//!   loads/stores are the identity over the integer paths.
//! - Structured control flow maps to `while :; do ...; break; done`
//!   wrappers; `br` becomes `break N`/`continue N` (bash counts only
//!   loops, `if` adds no level). Unreferenced labels emit no wrapper.
//! - Functions return values through the globals `R0, R1, ...`; traps set
//!   `TRAP_MSG` and propagate status 134 through `|| return $?` chains.
//!   Every generated function ends with an explicit `return 0` because a
//!   trailing arithmetic statement would leak status 1.
//! - One module instance per generation-time prefix: functions `<p>f<i>`,
//!   state `<p>g<i>`/`<p>mem`/`<p>tab`, entry points `<p>init`,
//!   `<p>invoke`, `<p>global_get`. Imports resolve from the caller's
//!   `IMPORTS` associative array (`[module.name]=function`).
//!
//! Requires bash >= 5 (namerefs, associative arrays).

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::Result;
use dewasmify_backend::{
    check_module_support, Backend, CodeWriter, GenOptions, Mode, OutputFile, RuntimeBundler,
    RuntimeScope, SupportStatus,
};
use dewasmify_core::feature::Feature;
use dewasmify_core::ir::{
    BinOp, BrTarget, ElemKind, ExportKind, Expr, LoadOp, Module, Stmt, StoreOp, Temp, UnOp,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The runtime unit bundler for Bash (see runtime/bash/units/).
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| {
        RuntimeBundler::new(
            "#",
            "  ",
            vec![
                RuntimeScope {
                    prefix: "rt",
                    open: "",
                    close: "",
                    prelude: Some("rt/_prelude"),
                },
                RuntimeScope {
                    prefix: "mem",
                    open: "",
                    close: "",
                    prelude: None,
                },
                RuntimeScope {
                    prefix: "wasi",
                    open: "",
                    close: "",
                    prelude: None,
                },
            ],
            UNIT_SOURCES,
        )
        .expect("runtime units are well-formed")
    })
}

/// Emit a top-level shared runtime for the closure of `seeds`. Bash has no
/// namespaces, so the "runtime" is just the flat function definitions.
pub fn shared_runtime(seeds: &BTreeSet<String>) -> Result<String> {
    bundler().bundle(seeds, 0)
}

/// Locate a bash >= 5 interpreter able to run generated scripts: tries
/// `$DEWASMIFY_BASH`, then `bash` on PATH, then the common Homebrew/local
/// install paths (macOS system bash is 3.2 and does not qualify).
pub fn find_bash5() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("DEWASMIFY_BASH") {
        candidates.push(PathBuf::from(env));
    }
    candidates.push(PathBuf::from("bash"));
    candidates.push(PathBuf::from("/opt/homebrew/bin/bash"));
    candidates.push(PathBuf::from("/usr/local/bin/bash"));
    for candidate in candidates {
        let Ok(out) = std::process::Command::new(&candidate)
            .args(["-c", "echo ${BASH_VERSINFO[0]}"])
            .output()
        else {
            continue;
        };
        if String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u32>()
            .unwrap_or(0)
            >= 5
        {
            return Some(candidate);
        }
    }
    None
}

/// Generate one module for `module` with all names prefixed by `prefix`
/// (e.g. `"m1_"`). Returns the source and the set of runtime units it
/// needs; the caller decides whether to bundle them alongside.
pub fn generate_module_with_units(
    module: &Module,
    prefix: &str,
    default_wasi: bool,
) -> Result<(String, BTreeSet<String>)> {
    generate_module_inner(module, prefix, default_wasi, &BTreeSet::new())
}

fn generate_module_inner(
    module: &Module,
    prefix: &str,
    default_wasi: bool,
    extra_seeds: &BTreeSet<String>,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&BashBackend, module)?;
    let gen = Gen {
        module,
        default_wasi,
        prefix,
        uses: RefCell::new(extra_seeds.clone()),
        scratch: Cell::new(0),
        scratch_max: Cell::new(0),
        needs_ci: Cell::new(false),
    };
    let mut w = CodeWriter::new("  ");
    gen.body(&mut w);
    Ok((w.finish(), gen.uses.into_inner()))
}

pub struct BashBackend;

impl Backend for BashBackend {
    fn name(&self) -> &str {
        "bash"
    }

    fn file_extension(&self) -> &str {
        "sh"
    }

    fn baseline(&self) -> &'static str {
        "Wasm core 1.0 (minus the wasm 1.0 gaps listed below) + mutable globals, \
         sign-extension, saturating float-to-int, multi-value, and the memory half \
         of bulk memory; f32/f64 run on the pure-Bash softfloat (ADR-5/ADR-13); \
         requires bash >= 5"
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // The ADR-5 softfloat covers the full float surface; the
            // harness now treats any float skip as a hard failure (ADR-8).
            Feature::Floats => SupportStatus::Supported,
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        let prefix = func_prefix(&opts.module_name);

        // The status mappings in the standalone main need these even when
        // the module itself never references them.
        let mut extra_seeds = BTreeSet::new();
        if opts.mode == Mode::Standalone {
            extra_seeds.insert("rt/trap".to_string());
            extra_seeds.insert("rt/exit".to_string());
        }

        let (src, units) = generate_module_inner(module, &prefix, opts.default_wasi, &extra_seeds)?;

        let mut out = String::new();
        if opts.mode == Mode::Standalone {
            out.push_str("#!/usr/bin/env bash\n");
        }
        out.push_str("# Generated by dewasmify. Do not edit.\n");
        out.push_str(&format!(
            "# Requires bash >= 5. Source this file, provide imports in the IMPORTS\n\
             # associative array ([module.name]=function), call {prefix}init, then\n\
             # {prefix}invoke <export> [args...]; results are returned in R0, R1, ...\n\n"
        ));
        match &opts.runtime {
            dewasmify_backend::RuntimeLinkage::Embedded => {
                out.push_str(&bundler().bundle(&units, 0)?);
                out.push('\n');
            }
            // Runtime functions are global names in bash; an alias has
            // nothing to emit as long as the shared bundle is sourced.
            dewasmify_backend::RuntimeLinkage::Alias(_) => {}
        }
        out.push_str(&src);

        if opts.mode == Mode::Standalone {
            let mut w = CodeWriter::new("  ");
            w.line("");
            w.line("if [[ ${BASH_SOURCE[0]} == \"$0\" ]]; then");
            w.indent();
            if wasi_bundled(module, opts.default_wasi) {
                w.line("WASI_ARGS=(\"${0##*/}\" \"$@\")");
                w.line("WASI_ENV=()");
                w.line("for __n in $(compgen -e); do WASI_ENV+=(\"$__n=${!__n}\"); done");
            }
            w.line(format!(
                "{prefix}init || {{ echo \"init failed (status $?): $TRAP_MSG\" >&2; exit 1; }}"
            ));
            w.line(format!("{prefix}invoke '_start'"));
            w.line("__st=$?");
            w.line("if (( __st == 133 )); then exit $(( EXIT_CODE & 0xff )); fi");
            w.line("if (( __st == 134 )); then echo \"trap: $TRAP_MSG\" >&2; exit 134; fi");
            w.line("exit $__st");
            w.dedent();
            w.line("fi");
            out.push_str(&w.finish());
        }
        Ok(vec![OutputFile {
            name: format!("{}.sh", opts.module_name),
            contents: out,
        }])
    }
}

/// Sanitized state/function prefix for a module name: `add.wasm` -> `add_`.
fn func_prefix(module_name: &str) -> String {
    let mut out = String::new();
    for c in module_name.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.is_empty() && !out.ends_with('_') {
            out.push('_');
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "wasm_");
    }
    if !out.ends_with('_') {
        out.push('_');
    }
    out
}

/// `wasi_unstable` (snapshot 0) shares the ABI of preview 1 for
/// everything implemented here except fd_seek's whence encoding, and
/// fd_seek is ESPIPE-only on this backend anyway.
const WASI_MODULES: &[&str] = &["wasi_snapshot_preview1", "wasi_unstable"];

fn is_wasi_module(name: &str) -> bool {
    WASI_MODULES.contains(&name)
}

/// Whether the generated module bundles the built-in WASI as an import
/// fallback (and therefore consumes the WASI_ARGS/WASI_ENV arrays).
fn wasi_bundled(module: &Module, default_wasi: bool) -> bool {
    default_wasi
        && module
            .imported_funcs
            .iter()
            .any(|f| is_wasi_module(&f.module) && bundler().has_unit(&format!("wasi/{}", f.name)))
}

/// Single-quoted bash string; `'` is the only character needing an escape.
fn bash_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn temp(t: Temp) -> String {
    format!("s{}", t.depth)
}

struct Gen<'a> {
    module: &'a Module,
    default_wasi: bool,
    prefix: &'a str,
    /// Runtime units the generated code references.
    uses: RefCell<BTreeSet<String>>,
    /// Per-statement scratch variable counter and its per-function peak
    /// (`__t<n>` locals hold helper results used as nested operands).
    scratch: Cell<u32>,
    scratch_max: Cell<u32>,
    /// Whether the current function needs the call_indirect scratch vars.
    needs_ci: Cell<bool>,
}

impl<'a> Gen<'a> {
    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    fn body(&self, w: &mut CodeWriter) {
        self.init(w);
        w.line("");
        self.invoke_fn(w);
        w.line("");
        self.global_get_fn(w);
        // WASI units are prefix-parameterized flat functions; imports are
        // bound to bare function names, so each bundled syscall gets a
        // per-module wrapper baking the prefix in.
        if self.default_wasi {
            let wrappers: BTreeSet<&str> = self
                .module
                .imported_funcs
                .iter()
                .filter(|f| {
                    is_wasi_module(&f.module) && bundler().has_unit(&format!("wasi/{}", f.name))
                })
                .map(|f| f.name.as_str())
                .collect();
            for name in wrappers {
                let p = self.prefix;
                w.line("");
                w.line(format!("{p}wasi_{name}() {{"));
                w.line(format!("  wasi_{name} {p} \"$@\""));
                w.line("}");
            }
        }
        for (i, func) in self.module.funcs.iter().enumerate() {
            w.line("");
            let idx = self.module.num_imported_funcs() as usize + i;
            self.function(w, idx as u32, func);
        }
    }

    fn init(&self, w: &mut CodeWriter) {
        let m = self.module;
        let p = self.prefix;
        w.line(format!("{p}init() {{"));
        w.indent();
        if !m.elems.is_empty() || m.datas.iter().any(|d| d.offset.is_some()) {
            w.line("local __off");
        }
        if wasi_bundled(m, self.default_wasi) {
            // Callers set the WASI_ARGS/WASI_ENV arrays before init (the
            // bash analogue of Ruby's args:/env: keywords); stdio fds are
            // preopened and fd_read/fd_write track per-fd offsets.
            w.line(format!("{p}wargs=(\"${{WASI_ARGS[@]}}\")"));
            w.line(format!("{p}wenv=(\"${{WASI_ENV[@]}}\")"));
            w.line(format!("{p}wfds=([0]=1 [1]=1 [2]=1)"));
            w.line(format!("{p}wtell=([0]=0 [1]=0 [2]=0)"));
        }
        if let Some(mem) = &m.memory {
            w.line(format!("{p}mem=()"));
            w.line(format!("{p}pages={}", mem.min_pages));
            w.line(format!("{p}max_pages={}", mem.max_pages.unwrap_or(65536)));
        }
        if let Some(table) = m.tables.first() {
            w.line(format!("{p}tab=()"));
            w.line(format!("{p}tabty=()"));
            w.line(format!("{p}tab_size={}", table.min));
        }
        if !m.imported_funcs.is_empty() {
            // Without this an unset IMPORTS would be treated as an indexed
            // array and its string keys evaluated as arithmetic.
            w.line("declare -gA IMPORTS");
        }
        for (i, import) in m.imported_funcs.iter().enumerate() {
            let key = bash_str(&format!("{}.{}", import.module, import.name));
            w.line(format!("{p}if{i}=${{IMPORTS[{key}]-}}"));
            if is_wasi_module(&import.module) && self.default_wasi {
                // Fallback order (ADR-7's bash shape): explicit import ->
                // bundled WASI unit via a per-module prefix wrapper ->
                // ENOSYS stub for known-but-unimplemented syscalls.
                let unit = format!("wasi/{}", import.name);
                if bundler().has_unit(&unit) {
                    self.use_unit(&unit);
                    w.line(format!(
                        "[[ -n ${p}if{i} ]] || {p}if{i}={p}wasi_{}",
                        import.name
                    ));
                } else {
                    self.use_unit("rt/enosys");
                    w.line(format!("[[ -n ${p}if{i} ]] || {p}if{i}=rt_enosys"));
                }
            } else {
                let msg = bash_str(&format!("missing import {}.{}", import.module, import.name));
                w.line(format!(
                    "[[ -n ${p}if{i} ]] || {{ echo {msg} >&2; return 1; }}"
                ));
            }
        }
        for (i, global) in m.globals.iter().enumerate() {
            self.scratch.set(0);
            self.emit_assign(w, &format!("{p}g{i}"), &global.init);
        }
        for elem in &m.elems {
            // check_module_support guarantees every segment reaching this
            // backend is active with only `ref.func` items (no bulk ops).
            let ElemKind::Active { offset, .. } = &elem.kind else {
                unreachable!("passive/declared element segment reached the bash backend")
            };
            self.scratch.set(0);
            self.use_unit("rt/trap");
            let off = self.value(w, offset);
            w.line(format!("(( __off = {off} ))"));
            w.line(format!(
                "(( __off + {} <= {p}tab_size )) || {{ rt_trap 'out of bounds table access'; return $?; }}",
                elem.items.len()
            ));
            for (i, func_idx) in elem.items.iter().enumerate() {
                let func_idx = func_idx.expect("ref.null element item reached the bash backend");
                let ty = self.func_type_idx(func_idx);
                w.line(format!("{p}tab[__off + {i}]={}", self.func_ref(func_idx)));
                w.line(format!("{p}tabty[__off + {i}]={ty}"));
            }
        }
        for (i, data) in m.datas.iter().enumerate() {
            let bytes = data
                .data
                .iter()
                .map(|b| b.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            match &data.offset {
                Some(offset) => {
                    self.scratch.set(0);
                    w.line(format!("{p}data{i}=({bytes})"));
                    let off = self.value(w, offset);
                    self.use_unit("mem/init");
                    w.line(format!(
                        "mem_init {p} {p}data{i} $(( {off} )) 0 {} || return $?",
                        data.data.len()
                    ));
                    w.line(format!("{p}data{i}=()"));
                }
                None => w.line(format!("{p}data{i}=({bytes})")),
            }
        }

        let mut entries = Vec::new();
        for export in &m.exports {
            if let ExportKind::Func(idx) = export.kind {
                entries.push(format!(
                    "[{}]={}",
                    bash_str(&export.name),
                    self.func_ref(idx)
                ));
            }
        }
        w.line(format!("declare -gA {p}EXPORTS=({})", entries.join(" ")));
        let mut gentries = Vec::new();
        for export in &m.exports {
            if let ExportKind::Global(idx) = export.kind {
                gentries.push(format!("[{}]={p}g{idx}", bash_str(&export.name)));
            }
        }
        w.line(format!(
            "declare -gA {p}GLOBAL_EXPORTS=({})",
            gentries.join(" ")
        ));

        if let Some(start) = m.start {
            w.line(format!("{} || return $?", self.call_cmd(start, &[])));
        }
        w.line("return 0");
        w.dedent();
        w.line("}");
    }

    fn invoke_fn(&self, w: &mut CodeWriter) {
        let p = self.prefix;
        w.line(format!("{p}invoke() {{"));
        w.indent();
        w.line("local __name=$1");
        w.line("shift");
        w.line(format!("local __fn=${{{p}EXPORTS[$__name]-}}"));
        w.line("if [[ -z $__fn ]]; then");
        w.indent();
        w.line("echo \"no such export: $__name\" >&2");
        w.line("return 1");
        w.dedent();
        w.line("fi");
        w.line("\"$__fn\" \"$@\"");
        w.dedent();
        w.line("}");
    }

    fn global_get_fn(&self, w: &mut CodeWriter) {
        let p = self.prefix;
        w.line(format!("{p}global_get() {{"));
        w.indent();
        w.line(format!("local __var=${{{p}GLOBAL_EXPORTS[$1]-}}"));
        w.line("if [[ -z $__var ]]; then");
        w.indent();
        w.line("echo \"no such global export: $1\" >&2");
        w.line("return 1");
        w.dedent();
        w.line("fi");
        w.line("R0=${!__var}");
        w.line("return 0");
        w.dedent();
        w.line("}");
    }

    fn func_type_idx(&self, func_idx: u32) -> u32 {
        let idx = func_idx as usize;
        let imports = self.module.imported_funcs.len();
        let ty = if idx < imports {
            self.module.imported_funcs[idx].type_idx
        } else {
            self.module.funcs[idx - imports].type_idx
        };
        self.canonical_type(ty)
    }

    /// call_indirect compares types structurally, so identical function
    /// types declared at different indices must collapse to one id.
    fn canonical_type(&self, type_idx: u32) -> u32 {
        let ty = &self.module.types[type_idx as usize];
        self.module
            .types
            .iter()
            .position(|t| t == ty)
            .map(|i| i as u32)
            .unwrap_or(type_idx)
    }

    /// The command name for the function (used in tables and exports).
    fn func_ref(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("${}if{func_idx}", self.prefix)
        } else {
            format!("{}f{func_idx}", self.prefix)
        }
    }

    fn call_cmd(&self, func_idx: u32, args: &[String]) -> String {
        let target = if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("\"${{{}if{func_idx}}}\"", self.prefix)
        } else {
            format!("{}f{func_idx}", self.prefix)
        };
        if args.is_empty() {
            target
        } else {
            format!("{target} {}", args.join(" "))
        }
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &dewasmify_core::ir::Func) {
        let ty = &self.module.types[func.type_idx as usize];
        self.scratch_max.set(0);
        self.needs_ci.set(false);

        let mut bw = CodeWriter::new("  ");
        bw.indent();
        let mut frames = Frames { stack: Vec::new() };
        self.stmts(&mut bw, &func.body, &mut frames);
        bw.line("return 0");
        let body = bw.finish();

        w.line(format!("{}f{idx}() {{", self.prefix));
        w.indent();
        let mut decls: Vec<String> = (0..ty.params.len())
            .map(|i| format!("l{i}=\"${{{}}}\"", i + 1))
            .collect();
        for i in 0..func.locals.len() {
            decls.push(format!("l{}=0", ty.params.len() + i));
        }
        if !decls.is_empty() {
            w.line(format!("local {}", decls.join(" ")));
        }
        let mut depths: Vec<u32> = func.temps.iter().map(|t| t.depth).collect();
        depths.dedup();
        if !depths.is_empty() {
            let names = depths
                .iter()
                .map(|d| format!("s{d}"))
                .collect::<Vec<_>>()
                .join(" ");
            w.line(format!("local {names}"));
        }
        let mut extras = Vec::new();
        for t in 0..self.scratch_max.get() {
            extras.push(format!("__t{t}"));
        }
        if self.needs_ci.get() {
            extras.push("__x".to_string());
            extras.push("__fn".to_string());
        }
        if !extras.is_empty() {
            w.line(format!("local {}", extras.join(" ")));
        }
        w.dedent();
        w.raw(&body);
        w.line("}");
    }

    fn stmts(&self, w: &mut CodeWriter, stmts: &[Stmt], f: &mut Frames) {
        for stmt in stmts {
            self.stmt(w, stmt, f);
        }
    }

    fn stmt(&self, w: &mut CodeWriter, stmt: &Stmt, f: &mut Frames) {
        self.scratch.set(0);
        match stmt {
            Stmt::Assign { dst, expr } => self.emit_assign(w, &temp(*dst), expr),
            Stmt::LocalSet { idx, expr } => self.emit_assign(w, &format!("l{idx}"), expr),
            Stmt::GlobalSet { idx, expr } => {
                self.emit_assign(w, &format!("{}g{idx}", self.prefix), expr)
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                let a = self.addr(w, addr, *offset);
                let v = self.value(w, value);
                let method = store_method(*op);
                self.use_unit(&format!("mem/{method}"));
                w.line(format!(
                    "mem_{method} {} {} {} || return $?",
                    self.prefix,
                    cmd_arg(&a),
                    cmd_arg(&v)
                ));
            }
            Stmt::Block { label, body } => {
                if label.referenced {
                    f.stack.push(label.id);
                    w.line("while :; do");
                    w.indent();
                    self.stmts(w, body, f);
                    w.line("break");
                    w.dedent();
                    w.line("done");
                    f.stack.pop();
                } else {
                    self.stmts(w, body, f);
                }
            }
            Stmt::Loop { label, body } => {
                if label.referenced {
                    f.stack.push(label.id);
                    w.line("while :; do");
                    w.indent();
                    self.stmts(w, body, f);
                    w.line("break");
                    w.dedent();
                    w.line("done");
                    f.stack.pop();
                } else {
                    // No br targets this loop, so it never repeats.
                    self.stmts(w, body, f);
                }
            }
            Stmt::If {
                label,
                cond,
                then,
                els,
            } => {
                if label.referenced {
                    f.stack.push(label.id);
                    w.line("while :; do");
                    w.indent();
                    self.emit_if(w, cond, then, els, f);
                    w.line("break");
                    w.dedent();
                    w.line("done");
                    f.stack.pop();
                } else {
                    self.emit_if(w, cond, then, els, f);
                }
            }
            Stmt::Br(target) => self.branch(w, target, f),
            Stmt::BrIf { cond, target } => {
                let c = self.value(w, cond);
                w.line(format!("if (( ({c}) != 0 )); then"));
                w.indent();
                self.branch(w, target, f);
                w.dedent();
                w.line("fi");
            }
            Stmt::BrTable {
                index,
                targets,
                default,
            } => {
                if targets.is_empty() {
                    self.branch(w, default, f);
                    return;
                }
                let i = self.value(w, index);
                let v = self.as_var(w, &i);
                w.line(format!("case ${v} in"));
                for (n, target) in targets.iter().enumerate() {
                    w.line(format!("{n})"));
                    w.indent();
                    self.branch(w, target, f);
                    w.line(";;");
                    w.dedent();
                }
                w.line("*)");
                w.indent();
                self.branch(w, default, f);
                w.line(";;");
                w.dedent();
                w.line("esac");
            }
            Stmt::Return { values } => self.return_stmt(w, values),
            Stmt::Call {
                func,
                args,
                results,
            } => {
                let args: Vec<String> = args.iter().map(|a| cmd_arg(&self.value(w, a))).collect();
                w.line(format!("{} || return $?", self.call_cmd(*func, &args)));
                self.assign_results(w, results);
            }
            Stmt::CallIndirect {
                type_idx,
                table_index: _,
                index,
                args,
                results,
            } => {
                let p = self.prefix;
                self.needs_ci.set(true);
                self.use_unit("rt/trap");
                let i = self.value(w, index);
                let args: Vec<String> = args.iter().map(|a| cmd_arg(&self.value(w, a))).collect();
                w.line(format!("(( __x = {i} ))"));
                w.line(format!(
                    "(( __x < {p}tab_size )) || {{ rt_trap 'undefined element'; return $?; }}"
                ));
                w.line(format!("__fn=${{{p}tab[__x]-}}"));
                w.line("[[ -n $__fn ]] || { rt_trap 'uninitialized element'; return $?; }");
                w.line(format!(
                    "(( {p}tabty[__x] == {} )) || {{ rt_trap 'indirect call type mismatch'; return $?; }}",
                    self.canonical_type(*type_idx)
                ));
                let cmd = if args.is_empty() {
                    "\"$__fn\"".to_string()
                } else {
                    format!("\"$__fn\" {}", args.join(" "))
                };
                w.line(format!("{cmd} || return $?"));
                self.assign_results(w, results);
            }
            Stmt::MemoryGrow { dst, delta } => {
                let p = self.prefix;
                let d = self.value(w, delta);
                w.line(format!("if (( ({d}) <= {p}max_pages - {p}pages )); then"));
                w.indent();
                // The delta operand may alias the destination temp, so
                // grow first and derive the old size afterwards.
                w.line(format!(
                    "(( {p}pages += ({d}), {} = {p}pages - ({d}) ))",
                    temp(*dst)
                ));
                w.dedent();
                w.line("else");
                w.indent();
                w.line(format!("(( {} = 0xffffffff ))", temp(*dst)));
                w.dedent();
                w.line("fi");
            }
            Stmt::MemoryCopy { dst, src, len } => {
                let d = self.value(w, dst);
                let s = self.value(w, src);
                let n = self.value(w, len);
                self.use_unit("mem/copy");
                w.line(format!(
                    "mem_copy {} {} {} {} || return $?",
                    self.prefix,
                    cmd_arg(&d),
                    cmd_arg(&s),
                    cmd_arg(&n)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                let d = self.value(w, dst);
                let v = self.value(w, val);
                let n = self.value(w, len);
                self.use_unit("mem/fill");
                w.line(format!(
                    "mem_fill {} {} {} {} || return $?",
                    self.prefix,
                    cmd_arg(&d),
                    cmd_arg(&v),
                    cmd_arg(&n)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                let d = self.value(w, dst);
                let s = self.value(w, src);
                let n = self.value(w, len);
                self.use_unit("mem/init");
                w.line(format!(
                    "mem_init {} {}data{seg} {} {} {} || return $?",
                    self.prefix,
                    self.prefix,
                    cmd_arg(&d),
                    cmd_arg(&s),
                    cmd_arg(&n)
                ));
            }
            Stmt::DataDrop { seg } => {
                w.line(format!("{}data{seg}=()", self.prefix));
            }
            Stmt::TableInit { .. } | Stmt::TableCopy { .. } | Stmt::ElemDrop { .. } => {
                unreachable!("table bulk ops reached the bash backend")
            }
            Stmt::Unreachable => {
                self.use_unit("rt/trap");
                w.line("rt_trap 'unreachable' || return $?");
            }
        }
    }

    fn emit_if(
        &self,
        w: &mut CodeWriter,
        cond: &Expr,
        then: &[Stmt],
        els: &[Stmt],
        f: &mut Frames,
    ) {
        let c = self.value(w, cond);
        w.line(format!("if (( ({c}) != 0 )); then"));
        w.indent();
        if then.is_empty() {
            w.line(":");
        } else {
            self.stmts(w, then, f);
        }
        w.dedent();
        if !els.is_empty() {
            w.line("else");
            w.indent();
            self.stmts(w, els, f);
            w.dedent();
        }
        w.line("fi");
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        // Command-shaped values are copied into scratch first, so a later
        // helper call cannot clobber an R<i> we already assigned.
        let frags: Vec<String> = values.iter().map(|v| self.value(w, v)).collect();
        for (i, frag) in frags.iter().enumerate() {
            w.line(format!("R{i}=$(( {frag} ))"));
        }
        w.line("return 0");
    }

    fn branch(&self, w: &mut CodeWriter, target: &BrTarget, f: &Frames) {
        match target {
            BrTarget::Return { values } => self.return_stmt(w, values),
            BrTarget::Label {
                label,
                is_loop,
                assigns,
            } => {
                for (dst, src) in assigns {
                    w.line(format!("(( {} = {} ))", temp(*dst), temp(*src)));
                }
                let depth = f.depth_of(*label);
                if *is_loop {
                    w.line(format!("continue {depth}"));
                } else {
                    w.line(format!("break {depth}"));
                }
            }
        }
    }

    fn assign_results(&self, w: &mut CodeWriter, results: &[Temp]) {
        for (i, r) in results.iter().enumerate() {
            w.line(format!("(( {} = R{i} ))", temp(*r)));
        }
    }

    /// Memory address fragment with the static offset folded in.
    fn addr(&self, w: &mut CodeWriter, addr: &Expr, offset: u64) -> String {
        let a = self.value(w, addr);
        if offset == 0 {
            a
        } else {
            format!("(({a}) + {offset})")
        }
    }

    fn emit_assign(&self, w: &mut CodeWriter, dst: &str, expr: &Expr) {
        if let Some(frag) = self.arith(expr) {
            w.line(format!("(( {dst} = {frag} ))"));
        } else if self.emit_command(w, expr) {
            w.line(format!("(( {dst} = R0 ))"));
        } else {
            let frag = self.value(w, expr);
            w.line(format!("(( {dst} = {frag} ))"));
        }
    }

    /// Evaluate `expr` to a pure arithmetic fragment, emitting helper
    /// commands (with scratch copies) as needed.
    fn value(&self, w: &mut CodeWriter, expr: &Expr) -> String {
        if let Some(frag) = self.arith(expr) {
            return frag;
        }
        match expr {
            Expr::Un(op, a) => {
                let a = self.value(w, a);
                if let Some(frag) = un_inline(*op, &a) {
                    return frag;
                }
                self.rt_cmd(w, un_helper(*op), &[cmd_arg(&a)]);
                self.grab_r0(w)
            }
            Expr::Bin(op, a, b) => {
                let a = self.value(w, a);
                let b = self.value(w, b);
                if let Some(frag) = bin_inline(*op, &a, &b) {
                    return frag;
                }
                self.rt_cmd(w, bin_helper(*op), &[cmd_arg(&a), cmd_arg(&b)]);
                self.grab_r0(w)
            }
            Expr::Load { op, addr, offset } => {
                let a = self.addr(w, addr, *offset);
                let method = load_method(*op);
                self.use_unit(&format!("mem/{method}"));
                w.line(format!(
                    "mem_{method} {} {} || return $?",
                    self.prefix,
                    cmd_arg(&a)
                ));
                self.grab_r0(w)
            }
            Expr::Select { cond, then, els } => {
                let c = self.value(w, cond);
                let t = self.value(w, then);
                let e = self.value(w, els);
                format!("((({c}) != 0 ? ({t}) : ({e})))")
            }
            _ => unreachable!("expression should have lowered to arithmetic"),
        }
    }

    /// Emit `expr` as one helper command leaving its result in R0, when it
    /// is directly command-shaped with pure operands. Returns false to let
    /// the caller take the generic scratch path.
    fn emit_command(&self, w: &mut CodeWriter, expr: &Expr) -> bool {
        match expr {
            Expr::Un(op, a) => {
                if un_inline_probe(*op) {
                    return false;
                }
                let Some(a) = self.arith(a) else { return false };
                self.rt_cmd(w, un_helper(*op), &[cmd_arg(&a)]);
                true
            }
            Expr::Bin(op, a, b) => {
                if bin_inline_probe(*op) {
                    return false;
                }
                let (Some(a), Some(b)) = (self.arith(a), self.arith(b)) else {
                    return false;
                };
                self.rt_cmd(w, bin_helper(*op), &[cmd_arg(&a), cmd_arg(&b)]);
                true
            }
            Expr::Load { op, addr, offset } => {
                let Some(a) = self.arith(addr) else {
                    return false;
                };
                let a = if *offset == 0 {
                    a
                } else {
                    format!("(({a}) + {offset})")
                };
                let method = load_method(*op);
                self.use_unit(&format!("mem/{method}"));
                w.line(format!(
                    "mem_{method} {} {} || return $?",
                    self.prefix,
                    cmd_arg(&a)
                ));
                true
            }
            _ => false,
        }
    }

    fn rt_cmd(&self, w: &mut CodeWriter, name: &str, args: &[String]) {
        self.use_unit(&format!("rt/{name}"));
        w.line(format!("rt_{name} {} || return $?", args.join(" ")));
    }

    fn grab_r0(&self, w: &mut CodeWriter) -> String {
        let t = self.scratch.get();
        self.scratch.set(t + 1);
        if t + 1 > self.scratch_max.get() {
            self.scratch_max.set(t + 1);
        }
        let name = format!("__t{t}");
        w.line(format!("{name}=$R0"));
        name
    }

    /// Force a fragment into a plain variable (for `case $v in`).
    fn as_var(&self, w: &mut CodeWriter, frag: &str) -> String {
        if frag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && frag.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
        {
            return frag.to_string();
        }
        let t = self.scratch.get();
        self.scratch.set(t + 1);
        if t + 1 > self.scratch_max.get() {
            self.scratch_max.set(t + 1);
        }
        let name = format!("__t{t}");
        w.line(format!("(( {name} = {frag} ))"));
        name
    }

    /// Pure arithmetic fragment for `expr`, or None if it needs commands.
    fn arith(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::I32Const(v) => Some(v.to_string()),
            // i64 constants are emitted as their signed-64 bit pattern;
            // an unsigned literal above INT64_MAX would be
            // implementation-defined in bash.
            Expr::I64Const(v) => Some(format!("{}", *v as i64)),
            // Floats are their bit patterns (ADR-13): f32 as u32, f64 as
            // the signed-64 pattern (a u64 decimal >= 2^63 would wrap
            // implementation-defined in bash).
            Expr::F32Const(bits) => Some(bits.to_string()),
            Expr::F64Const(bits) => Some(format!("{}", *bits as i64)),
            Expr::Temp(t) => Some(temp(*t)),
            Expr::LocalGet(idx) => Some(format!("l{idx}")),
            Expr::GlobalGet(idx) => Some(format!("{}g{idx}", self.prefix)),
            Expr::Un(op, a) => un_inline(*op, &self.arith(a)?),
            Expr::Bin(op, a, b) => bin_inline(*op, &self.arith(a)?, &self.arith(b)?),
            Expr::Load { .. } => None,
            Expr::Select { cond, then, els } => {
                let c = self.arith(cond)?;
                let t = self.arith(then)?;
                let e = self.arith(els)?;
                Some(format!("((({c}) != 0 ? ({t}) : ({e})))"))
            }
            Expr::MemorySize => Some(format!("{}pages", self.prefix)),
        }
    }
}

struct Frames {
    /// Label ids that occupy a bash loop level, outermost first.
    stack: Vec<u32>,
}

impl Frames {
    fn depth_of(&self, label: u32) -> usize {
        let pos = self
            .stack
            .iter()
            .rposition(|&l| l == label)
            .expect("branch to a label outside the frame stack");
        self.stack.len() - pos
    }
}

/// Command argument for a pure fragment: decimal literals pass through,
/// anything else is evaluated with `$(( ))`.
fn cmd_arg(frag: &str) -> String {
    let digits = frag.strip_prefix('-').unwrap_or(frag);
    if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
        frag.to_string()
    } else {
        format!("\"$(( {frag} ))\"")
    }
}

fn s32_view(x: &str) -> String {
    format!("((({x}) ^ 0x80000000) - 0x80000000)")
}

fn u64_flip(x: &str) -> String {
    format!("(({x}) ^ (1 << 63))")
}

/// Inline arithmetic lowering for a unary op, if it has one.
fn un_inline(op: UnOp, a: &str) -> Option<String> {
    use UnOp::*;
    Some(match op {
        I32Eqz | I64Eqz => format!("(({a}) == 0)"),
        I32WrapI64 => format!("(({a}) & 0xffffffff)"),
        I64ExtendI32U => format!("({a})"),
        I64ExtendI32S => s32_view(a),
        I32Extend8S => format!("((((({a}) & 0xff) ^ 0x80) - 0x80) & 0xffffffff)"),
        I32Extend16S => format!("((((({a}) & 0xffff) ^ 0x8000) - 0x8000) & 0xffffffff)"),
        I64Extend8S => format!("(((({a}) & 0xff) ^ 0x80) - 0x80)"),
        I64Extend16S => format!("(((({a}) & 0xffff) ^ 0x8000) - 0x8000)"),
        I64Extend32S => format!("(((({a}) & 0xffffffff) ^ 0x80000000) - 0x80000000)"),
        // Pure bit ops on the float patterns (payload-preserving, as the
        // spec requires for the sign ops); reinterprets are the identity.
        F32Abs => format!("(({a}) & 0x7fffffff)"),
        F32Neg => format!("(({a}) ^ 0x80000000)"),
        F64Abs => format!("(({a}) & 0x7fffffffffffffff)"),
        F64Neg => format!("(({a}) ^ (1 << 63))"),
        I32ReinterpretF32 | I64ReinterpretF64 | F32ReinterpretI32 | F64ReinterpretI64 => {
            format!("({a})")
        }
        _ => return None,
    })
}

/// Whether `un_inline` handles this op (without needing the fragment).
fn un_inline_probe(op: UnOp) -> bool {
    un_inline(op, "0").is_some()
}

fn un_helper(op: UnOp) -> &'static str {
    use UnOp::*;
    match op {
        I32Clz => "i32_clz",
        I32Ctz => "i32_ctz",
        I64Clz => "i64_clz",
        I64Ctz => "i64_ctz",
        I32Popcnt | I64Popcnt => "popcnt",
        F32Ceil => "f32_ceil",
        F32Floor => "f32_floor",
        F32Trunc => "f32_trunc",
        F32Nearest => "f32_nearest",
        F32Sqrt => "f32_sqrt",
        F64Ceil => "f64_ceil",
        F64Floor => "f64_floor",
        F64Trunc => "f64_trunc",
        F64Nearest => "f64_nearest",
        F64Sqrt => "f64_sqrt",
        I32TruncF32S => "i32_trunc_f32_s",
        I32TruncF32U => "i32_trunc_f32_u",
        I32TruncF64S => "i32_trunc_f64_s",
        I32TruncF64U => "i32_trunc_f64_u",
        I64TruncF32S => "i64_trunc_f32_s",
        I64TruncF32U => "i64_trunc_f32_u",
        I64TruncF64S => "i64_trunc_f64_s",
        I64TruncF64U => "i64_trunc_f64_u",
        I32TruncSatF32S => "i32_trunc_sat_f32_s",
        I32TruncSatF32U => "i32_trunc_sat_f32_u",
        I32TruncSatF64S => "i32_trunc_sat_f64_s",
        I32TruncSatF64U => "i32_trunc_sat_f64_u",
        I64TruncSatF32S => "i64_trunc_sat_f32_s",
        I64TruncSatF32U => "i64_trunc_sat_f32_u",
        I64TruncSatF64S => "i64_trunc_sat_f64_s",
        I64TruncSatF64U => "i64_trunc_sat_f64_u",
        F32ConvertI32S => "f32_convert_i32_s",
        F32ConvertI32U => "f32_convert_i32_u",
        F32ConvertI64S => "f32_convert_i64_s",
        F32ConvertI64U => "f32_convert_i64_u",
        F64ConvertI32S => "f64_convert_i32_s",
        F64ConvertI32U => "f64_convert_i32_u",
        F64ConvertI64S => "f64_convert_i64_s",
        F64ConvertI64U => "f64_convert_i64_u",
        F32DemoteF64 => "f32_demote",
        F64PromoteF32 => "f64_promote",
        _ => unreachable!("op {op:?} has an inline lowering"),
    }
}

/// Inline arithmetic lowering for a binary op, if it has one.
fn bin_inline(op: BinOp, a: &str, b: &str) -> Option<String> {
    use BinOp::*;
    Some(match op {
        I32Add => format!("((({a}) + ({b})) & 0xffffffff)"),
        I32Sub => format!("((({a}) - ({b})) & 0xffffffff)"),
        I32Mul => format!("((({a}) * ({b})) & 0xffffffff)"),
        I64Add => format!("(({a}) + ({b}))"),
        I64Sub => format!("(({a}) - ({b}))"),
        I64Mul => format!("(({a}) * ({b}))"),
        I32And | I64And => format!("(({a}) & ({b}))"),
        I32Or | I64Or => format!("(({a}) | ({b}))"),
        I32Xor | I64Xor => format!("(({a}) ^ ({b}))"),
        I32Shl => format!("((({a}) << (({b}) & 31)) & 0xffffffff)"),
        I32ShrU => format!("(({a}) >> (({b}) & 31))"),
        I32ShrS => format!("(({} >> (({b}) & 31)) & 0xffffffff)", s32_view(a)),
        I64Shl => format!("(({a}) << (({b}) & 63))"),
        I64ShrS => format!("(({a}) >> (({b}) & 63))"),
        // Logical right shift over the signed representation: mask off the
        // dragged-in sign bits; a shift count of 0 must bypass the mask
        // because bash takes shift counts mod 64.
        I64ShrU => format!(
            "(((({b}) & 63) == 0 ? ({a}) : ((({a}) >> (({b}) & 63)) & ~(-1 << (64 - (({b}) & 63))))))"
        ),
        I32Eq | I64Eq => format!("(({a}) == ({b}))"),
        I32Ne | I64Ne => format!("(({a}) != ({b}))"),
        I32LtU => format!("(({a}) < ({b}))"),
        I32GtU => format!("(({a}) > ({b}))"),
        I32LeU => format!("(({a}) <= ({b}))"),
        I32GeU => format!("(({a}) >= ({b}))"),
        I32LtS => format!("({} < {})", s32_view(a), s32_view(b)),
        I32GtS => format!("({} > {})", s32_view(a), s32_view(b)),
        I32LeS => format!("({} <= {})", s32_view(a), s32_view(b)),
        I32GeS => format!("({} >= {})", s32_view(a), s32_view(b)),
        I64LtS => format!("(({a}) < ({b}))"),
        I64GtS => format!("(({a}) > ({b}))"),
        I64LeS => format!("(({a}) <= ({b}))"),
        I64GeS => format!("(({a}) >= ({b}))"),
        I64LtU => format!("({} < {})", u64_flip(a), u64_flip(b)),
        I64GtU => format!("({} > {})", u64_flip(a), u64_flip(b)),
        I64LeU => format!("({} <= {})", u64_flip(a), u64_flip(b)),
        I64GeU => format!("({} >= {})", u64_flip(a), u64_flip(b)),
        F32Copysign => format!("((({a}) & 0x7fffffff) | (({b}) & 0x80000000))"),
        F64Copysign => {
            format!("((({a}) & 0x7fffffffffffffff) | (({b}) & (1 << 63)))")
        }
        _ => return None,
    })
}

/// Whether `bin_inline` handles this op (without needing the fragments).
fn bin_inline_probe(op: BinOp) -> bool {
    bin_inline(op, "0", "0").is_some()
}

fn bin_helper(op: BinOp) -> &'static str {
    use BinOp::*;
    match op {
        I32DivS => "i32_div_s",
        I32DivU => "i32_div_u",
        I32RemS => "i32_rem_s",
        I32RemU => "i32_rem_u",
        I64DivS => "i64_div_s",
        I64DivU => "i64_div_u",
        I64RemS => "i64_rem_s",
        I64RemU => "i64_rem_u",
        I32Rotl => "i32_rotl",
        I32Rotr => "i32_rotr",
        I64Rotl => "i64_rotl",
        I64Rotr => "i64_rotr",
        F32Add => "f32_add",
        F32Sub => "f32_sub",
        F32Mul => "f32_mul",
        F32Div => "f32_div",
        F32Min => "f32_min",
        F32Max => "f32_max",
        F64Add => "f64_add",
        F64Sub => "f64_sub",
        F64Mul => "f64_mul",
        F64Div => "f64_div",
        F64Min => "f64_min",
        F64Max => "f64_max",
        F32Eq => "f32_eq",
        F32Ne => "f32_ne",
        F32Lt => "f32_lt",
        F32Gt => "f32_gt",
        F32Le => "f32_le",
        F32Ge => "f32_ge",
        F64Eq => "f64_eq",
        F64Ne => "f64_ne",
        F64Lt => "f64_lt",
        F64Gt => "f64_gt",
        F64Le => "f64_le",
        F64Ge => "f64_ge",
        _ => unreachable!("op {op:?} has an inline lowering"),
    }
}

fn load_method(op: LoadOp) -> &'static str {
    use LoadOp::*;
    match op {
        I32Load => "i32_load",
        I64Load => "i64_load",
        // Floats travel as bit patterns; the integer memory units are
        // exact (ADR-13).
        F32Load => "i32_load",
        F64Load => "i64_load",
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
        F32Store => "i32_store",
        F64Store => "i64_store",
        I32Store8 => "i32_store8",
        I32Store16 => "i32_store16",
        I64Store8 => "i64_store8",
        I64Store16 => "i64_store16",
        I64Store32 => "i64_store32",
    }
}
