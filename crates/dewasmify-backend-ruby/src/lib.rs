//! Ruby backend: translates dewasmify IR into a Ruby class plus a bundled
//! lightweight runtime.
//!
//! Lowering conventions (ADR-4; numeric conventions ADR-2):
//! - i32/i64 are unsigned (masked) Ruby Integers; signed views via
//!   `Rt.s32/s64` only where an instruction needs them.
//! - f32/f64 are Ruby Floats; f32 results are re-rounded with `Rt.f32`.
//! - Multi-level `br` uses catch/throw; loops become `while true` with a
//!   catch whose value distinguishes continue from fallthrough.
//!
//! The runtime is composed from per-method units (ADR-6) and referenced by
//! the relative name `Rt`, so linkage (embedded per class, shared, or a
//! future gem) is the caller's choice.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::Result;
use dewasmify_backend::{
    Backend, CodeWriter, GenOptions, Mode, OutputFile, RuntimeBundler, RuntimeLinkage,
    RuntimeScope, SupportStatus,
};
use dewasmify_core::feature::Feature;
use dewasmify_core::ir::{
    BinOp, BrTarget, ExportKind, Expr, LoadOp, Module, Stmt, StoreOp, Temp, UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The runtime unit bundler for Ruby (see runtime/ruby/units/).
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
                    prelude: Some("rt/_module"),
                },
                RuntimeScope {
                    prefix: "memory",
                    open: "class Memory",
                    close: "end",
                    prelude: Some("memory/_class"),
                },
                RuntimeScope {
                    prefix: "table",
                    open: "class Table",
                    close: "end",
                    prelude: Some("table/_class"),
                },
                RuntimeScope {
                    prefix: "wasi",
                    open: "class WASI",
                    close: "end",
                    prelude: Some("wasi/_class"),
                },
            ],
            UNIT_SOURCES,
        )
        .expect("runtime units are well-formed")
    })
}

/// Emit a top-level shared runtime (`module Rt ... end`) for the closure
/// of `seeds`; generated classes then use `RuntimeLinkage::Alias("::Rt")`.
pub fn shared_runtime(seeds: &BTreeSet<String>) -> Result<String> {
    Ok(format!("module Rt\n{}end\n", bundler().bundle(seeds, 1)?))
}

/// Locate a ruby interpreter able to run generated scripts. Unlike
/// `dewasmify_backend_bash::find_bash5`, there is no version floor or
/// alternate-path search to do: ruby has no documented minimum version
/// here, so this only confirms `ruby` on PATH actually runs.
pub fn find_ruby() -> Option<std::path::PathBuf> {
    std::process::Command::new("ruby")
        .arg("--version")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|_| std::path::PathBuf::from("ruby"))
}

/// Generate one class for `module`. Returns the class source and the set
/// of runtime units it needs (already bundled inside for `Embedded`).
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
    let gen = Gen {
        module,
        default_wasi,
        uses: RefCell::new(extra_seeds.clone()),
    };
    let mut wb = CodeWriter::new("  ");
    wb.indent();
    gen.body(&mut wb);
    let body = wb.finish();
    let uses = gen.uses.into_inner();

    let mut out = format!("class {class_name}\n");
    match linkage {
        RuntimeLinkage::Embedded => {
            if !uses.is_empty() {
                out.push_str("  module Rt\n");
                out.push_str(&bundler().bundle(&uses, 2)?);
                out.push_str("  end\n\n");
            }
        }
        RuntimeLinkage::Alias(path) => {
            out.push_str(&format!("  Rt = {path}\n\n"));
        }
    }
    out.push_str(&body);
    out.push_str("end\n");
    Ok((out, uses))
}

pub struct RubyBackend;

impl Backend for RubyBackend {
    fn name(&self) -> &str {
        "ruby"
    }

    fn file_extension(&self) -> &str {
        "rb"
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // Part of the wasm 1.0 baseline for Ruby; the row exists for
            // backends whose language lacks floats (ADR-5).
            Feature::Floats => SupportStatus::Supported,
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        let class_name = class_name(&opts.module_name);

        // The Exit/Trap rescue clauses in the standalone main need these
        // even when the module itself never references them.
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

        let mut w = CodeWriter::new("  ");
        w.line("# Generated by dewasmify. Do not edit.");
        w.line("# frozen_string_literal: false");
        w.line("");
        w.raw(&class_src);

        if opts.mode == Mode::Standalone {
            let wasi_kwargs = wasi_bundled(module, opts.default_wasi);
            w.line("");
            w.block("if __FILE__ == $PROGRAM_NAME", "end", |w| {
                if wasi_kwargs {
                    // DEWASMIFY_PREOPEN maps guest paths to host directories
                    // for standalone runs, e.g. "/=./data,/tmp=/tmp"; kept
                    // out of ARGV since that mirrors the guest's own argv.
                    w.line(
                        "preopens = ENV.fetch(\"DEWASMIFY_PREOPEN\", \"\").split(\",\").filter_map { |kv| g, h = kv.split(\"=\", 2); [g, h] if h }.to_h",
                    );
                    w.line(format!(
                        "inst = {class_name}.new({{}}, args: [File.basename($PROGRAM_NAME), *ARGV], env: ENV.to_h, preopens: preopens)"
                    ));
                } else {
                    w.line(format!("inst = {class_name}.new"));
                }
                w.line("begin");
                w.indent();
                w.line("inst.invoke(\"_start\")");
                w.line("exit 0");
                w.dedent();
                w.line(format!("rescue {class_name}::Rt::Exit => e"));
                w.indent();
                w.line("exit e.code");
                w.dedent();
                w.line(format!("rescue {class_name}::Rt::Trap => e"));
                w.indent();
                w.line("warn \"trap: #{e.message}\"");
                w.line("exit 134");
                w.dedent();
                w.line("end");
            });
        }

        Ok(vec![OutputFile {
            name: format!("{}.rb", opts.module_name),
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

fn ruby_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '#' => out.push_str("\\#"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
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
    format!("[\"{hex}\"].pack(\"H*\")")
}

/// WASI import module names the bundled runtime answers for.
/// `wasi_unstable` (snapshot 0) shares the ABI of preview 1 for everything
/// we implement except fd_seek's whence encoding (snapshot 0 modules that
/// actually seek may misbehave; acceptable until snapshot 0 gets its own
/// units).
const WASI_MODULES: &[&str] = &["wasi_snapshot_preview1", "wasi_unstable"];

fn is_wasi_module(name: &str) -> bool {
    WASI_MODULES.contains(&name)
}

pub use dewasmify_backend::WASI_PREVIEW1_FUNCTIONS;

/// Whether the generated class bundles the built-in WASI as an import
/// fallback (and therefore takes `args:`/`env:` keyword arguments).
fn wasi_bundled(module: &Module, default_wasi: bool) -> bool {
    default_wasi
        && module
            .imported_funcs
            .iter()
            .any(|f| is_wasi_module(&f.module) && bundler().has_unit(&format!("wasi/{}", f.name)))
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

    /// Reference a Memory method, recording its unit.
    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
    }

    /// Class body members, written at indent level 1.
    fn body(&self, w: &mut CodeWriter) {
        self.initialize(w);
        w.line("");
        w.line("attr_reader :memory, :exports");
        w.line("");
        w.block("def invoke(name, *args)", "end", |w| {
            w.line("@exports.fetch(name).call(*args)");
        });
        w.line("");
        w.block("def global_get(name)", "end", |w| {
            w.line("instance_variable_get(GLOBAL_EXPORTS.fetch(name))");
        });
        w.line("");
        w.line("private");
        for (i, func) in self.module.funcs.iter().enumerate() {
            w.line("");
            let idx = self.module.num_imported_funcs() as usize + i;
            self.function(w, idx as u32, func);
        }
    }

    fn initialize(&self, w: &mut CodeWriter) {
        let m = self.module;

        let mut global_exports: Vec<(String, u32)> = Vec::new();
        for export in &m.exports {
            if let ExportKind::Global(idx) = export.kind {
                global_exports.push((export.name.clone(), idx));
            }
        }
        let entries = global_exports
            .iter()
            .map(|(name, idx)| format!("{} => :@g{}", ruby_string(name), idx))
            .collect::<Vec<_>>()
            .join(", ");
        w.line(format!("GLOBAL_EXPORTS = {{ {entries} }}.freeze"));
        w.line("");

        let wasi_fallback = wasi_bundled(m, self.default_wasi);
        let header = if wasi_fallback {
            "def initialize(imports = {}, args: [], env: {}, preopens: {})"
        } else {
            "def initialize(imports = {})"
        };
        w.block(header, "end", |w| {
            if let Some(mem) = &m.memory {
                self.use_unit("memory/_class");
                let max = mem
                    .max_pages
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "nil".to_string());
                w.line(format!("@memory = Rt::Memory.new({}, {})", mem.min_pages, max));
            }
            if let Some(table) = &m.table {
                self.use_unit("table/_class");
                w.line(format!("@table = Rt::Table.new({})", table.min));
            }
            if wasi_fallback {
                w.line("@wasi = nil");
            }
            for (i, import) in m.imported_funcs.iter().enumerate() {
                self.use_unit("rt/resolve_import");
                let resolve = format!(
                    "Rt.resolve_import(imports, {}, {})",
                    ruby_string(&import.module),
                    ruby_string(&import.name)
                );
                // Fallback order: explicit import -> bundled WASI
                // (constructed only when first needed) -> ENOSYS stub;
                // non-WASI imports stay mandatory.
                let fallback = if is_wasi_module(&import.module) && self.default_wasi {
                    let unit = format!("wasi/{}", import.name);
                    if bundler().has_unit(&unit) {
                        self.use_unit(&unit);
                        self.use_unit("wasi/_class");
                        format!(
                            "(@wasi ||= Rt::WASI.new(args: args, env: env, preopens: preopens)).method(:wasi_{})",
                            import.name
                        )
                    } else {
                        "->(*) { 52 } # ENOSYS: not implemented yet".to_string()
                    }
                } else {
                    format!(
                        "raise(ArgumentError, {})",
                        ruby_string(&format!(
                            "missing import {}.{}",
                            import.module, import.name
                        ))
                    )
                };
                w.line(format!("@if{i} = {resolve} || {fallback}"));
            }
            for (i, global) in m.globals.iter().enumerate() {
                w.line(format!("@g{} = {}", i, self.expr(&global.init)));
            }
            for elem in &m.elems {
                self.use_unit("table/check_range");
                let offset = self.expr(&elem.offset);
                w.line(format!(
                    "@table.check_range({offset}, {})",
                    elem.func_indices.len()
                ));
                for (i, func_idx) in elem.func_indices.iter().enumerate() {
                    self.use_unit("table/set");
                    let ty = self.func_type_idx(*func_idx);
                    w.line(format!(
                        "@table.set({offset} + {i}, {ty}, {})",
                        self.func_ref(*func_idx)
                    ));
                }
            }
            for (i, data) in m.datas.iter().enumerate() {
                match &data.offset {
                    Some(offset) => {
                        self.use_unit("memory/init");
                        w.line(format!(
                            "@memory.init({}, {}, 0, {})",
                            self.expr(offset),
                            hex_bytes(&data.data),
                            data.data.len()
                        ));
                        w.line(format!("@data{i} = \"\".b"));
                    }
                    None => {
                        w.line(format!("@data{i} = {}", hex_bytes(&data.data)));
                    }
                }
            }

            let mut export_entries = Vec::new();
            for export in &m.exports {
                if let ExportKind::Func(idx) = export.kind {
                    export_entries
                        .push(format!("{} => {}", ruby_string(&export.name), self.func_ref(idx)));
                }
            }
            w.line(format!("@exports = {{ {} }}", export_entries.join(", ")));

            // The instance is complete: let import providers bind to it
            // (memory access etc.) before any wasm code can run.
            if !m.imported_funcs.is_empty() {
                w.line("imports.each_value.to_a.uniq.each { |s| s.attach(self) if s.respond_to?(:attach) }");
            }
            if wasi_fallback {
                w.line("@wasi&.attach(self)");
            }

            if let Some(start) = m.start {
                w.line(self.call_string(start, &[]));
            }
        });
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

    /// A callable object for the function (used in tables and exports).
    fn func_ref(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("@if{func_idx}")
        } else {
            format!("method(:_f{func_idx})")
        }
    }

    fn call_string(&self, func_idx: u32, args: &[String]) -> String {
        let args = args.join(", ");
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("@if{func_idx}.call({args})")
        } else if args.is_empty() {
            format!("_f{func_idx}")
        } else {
            format!("_f{func_idx}({args})")
        }
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &dewasmify_core::ir::Func) {
        let ty = &self.module.types[func.type_idx as usize];
        let params = (0..ty.params.len())
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let header = if params.is_empty() {
            format!("def _f{idx}")
        } else {
            format!("def _f{idx}({params})")
        };
        w.block(header, "end", |w| {
            for (i, local_ty) in func.locals.iter().enumerate() {
                let name = format!("l{}", ty.params.len() + i);
                w.line(format!("{name} = {}", default_value(*local_ty)));
            }
            // Hoist all temps to method scope: assignments inside catch/do
            // blocks would otherwise be block-local in Ruby.
            let mut depths: Vec<u32> = func.temps.iter().map(|t| t.depth).collect();
            depths.dedup();
            if !depths.is_empty() {
                let decl = depths
                    .iter()
                    .map(|d| format!("s{d} = "))
                    .collect::<String>();
                w.line(format!("{decl}nil"));
            }
            self.stmts(w, &func.body);
        });
    }

    fn stmts(&self, w: &mut CodeWriter, stmts: &[Stmt]) {
        for stmt in stmts {
            self.stmt(w, stmt);
        }
    }

    fn stmt(&self, w: &mut CodeWriter, stmt: &Stmt) {
        match stmt {
            Stmt::Assign { dst, expr } => {
                w.line(format!("{} = {}", temp(*dst), self.expr(expr)));
            }
            Stmt::LocalSet { idx, expr } => {
                w.line(format!("l{idx} = {}", self.expr(expr)));
            }
            Stmt::GlobalSet { idx, expr } => {
                w.line(format!("@g{idx} = {}", self.expr(expr)));
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                w.line(format!(
                    "@memory.{}({}, {})",
                    self.mem(store_method(*op)),
                    self.addr(addr, *offset),
                    self.expr(value)
                ));
            }
            Stmt::Block { label, body } => {
                w.block(format!("catch(:l{}) do", label.id), "end", |w| {
                    self.stmts(w, body);
                });
            }
            Stmt::Loop { label, body } => {
                w.block("while true", "end", |w| {
                    w.block(format!("__b = catch(:l{}) do", label.id), "end", |w| {
                        self.stmts(w, body);
                        w.line("true");
                    });
                    w.line("break if __b");
                });
            }
            Stmt::If {
                label,
                cond,
                then,
                els,
            } => {
                let emit_if = |w: &mut CodeWriter, gen: &Self| {
                    w.line(format!("if {} != 0", gen.expr(cond)));
                    w.indent();
                    if then.is_empty() {
                        w.line("nil");
                    } else {
                        gen.stmts(w, then);
                    }
                    w.dedent();
                    if !els.is_empty() {
                        w.line("else");
                        w.indent();
                        gen.stmts(w, els);
                        w.dedent();
                    }
                    w.line("end");
                };
                if label.referenced {
                    w.block(format!("catch(:l{}) do", label.id), "end", |w| {
                        emit_if(w, self);
                    });
                } else {
                    emit_if(w, self);
                }
            }
            Stmt::Br(target) => self.branch(w, target),
            Stmt::BrIf { cond, target } => {
                w.block(format!("if {} != 0", self.expr(cond)), "end", |w| {
                    self.branch(w, target);
                });
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
                w.line(format!("case {}", self.expr(index)));
                for (i, target) in targets.iter().enumerate() {
                    w.line(format!("when {i}"));
                    w.indent();
                    self.branch(w, target);
                    w.dedent();
                }
                w.line("else");
                w.indent();
                self.branch(w, default);
                w.dedent();
                w.line("end");
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
                index,
                args,
                results,
            } => {
                self.use_unit("table/call");
                let mut call_args =
                    vec![self.expr(index), self.canonical_type(*type_idx).to_string()];
                call_args.extend(args.iter().map(|a| self.expr(a)));
                let call = format!("@table.call({})", call_args.join(", "));
                w.line(assign_results(results, call));
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                w.line(format!(
                    "{} = @memory.grow({})",
                    temp(*dst),
                    self.expr(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "@memory.copy({}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "@memory.fill({}, {}, {})",
                    self.expr(dst),
                    self.expr(val),
                    self.expr(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "@memory.init({}, @data{seg}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::DataDrop { seg } => {
                w.line(format!("@data{seg} = \"\".b"));
            }
            Stmt::Unreachable => {
                w.line(format!("{}(\"unreachable\")", self.rt("trap")));
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
                w.line(format!("return [{vs}]"));
            }
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
                for (dst, src) in assigns {
                    w.line(format!("{} = {}", temp(*dst), temp(*src)));
                }
                if *is_loop {
                    w.line(format!("throw :l{label}, false"));
                } else {
                    w.line(format!("throw :l{label}"));
                }
            }
        }
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
                    format!("{:?}", v as f64)
                } else {
                    format!("{}(0x{bits:x})", self.rt("f32_from_bits"))
                }
            }
            Expr::F64Const(bits) => {
                let v = f64::from_bits(*bits);
                if v.is_finite() {
                    format!("{v:?}")
                } else {
                    format!("{}(0x{bits:x})", self.rt("f64_from_bits"))
                }
            }
            Expr::Temp(t) => temp(*t),
            Expr::LocalGet(idx) => format!("l{idx}"),
            Expr::GlobalGet(idx) => format!("@g{idx}"),
            Expr::Un(op, a) => self.un(*op, &self.expr(a)),
            Expr::Bin(op, a, b) => self.bin(*op, &self.expr(a), &self.expr(b)),
            Expr::Load { op, addr, offset } => {
                format!(
                    "@memory.{}({})",
                    self.mem(load_method(*op)),
                    self.addr(addr, *offset)
                )
            }
            Expr::Select { cond, then, els } => {
                format!(
                    "({} != 0 ? {} : {})",
                    self.expr(cond),
                    self.expr(then),
                    self.expr(els)
                )
            }
            Expr::MemorySize => {
                self.use_unit("memory/size");
                "@memory.size".to_string()
            }
        }
    }

    fn un(&self, op: UnOp, a: &str) -> String {
        use UnOp::*;
        match op {
            I32Eqz | I64Eqz => format!("({a} == 0 ? 1 : 0)"),
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
            I32WrapI64 => format!("({a} & 0xffffffff)"),
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
            F32ConvertI32S => format!("{}({}({a}).to_f)", self.rt("f32"), self.rt("s32")),
            F32ConvertI32U => format!("{}({a}.to_f)", self.rt("f32")),
            F32ConvertI64S => format!("{}({}({a}))", self.rt("cvt_f32_i"), self.rt("s64")),
            F32ConvertI64U => format!("{}({a})", self.rt("cvt_f32_i")),
            F64ConvertI32S => format!("{}({a}).to_f", self.rt("s32")),
            F64ConvertI32U => format!("{a}.to_f"),
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
            I32Add => format!("(({a} + {b}) & 0xffffffff)"),
            I32Sub => format!("(({a} - {b}) & 0xffffffff)"),
            I32Mul => format!("(({a} * {b}) & 0xffffffff)"),
            I64Add => format!("(({a} + {b}) & 0xffffffffffffffff)"),
            I64Sub => format!("(({a} - {b}) & 0xffffffffffffffff)"),
            I64Mul => format!("(({a} * {b}) & 0xffffffffffffffff)"),
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
            I32Shl => format!("(({a} << ({b} & 31)) & 0xffffffff)"),
            I32ShrU => format!("({a} >> ({b} & 31))"),
            I32ShrS => {
                format!("(({}({a}) >> ({b} & 31)) & 0xffffffff)", self.rt("s32"))
            }
            I64Shl => format!("(({a} << ({b} & 63)) & 0xffffffffffffffff)"),
            I64ShrU => format!("({a} >> ({b} & 63))"),
            I64ShrS => {
                format!(
                    "(({}({a}) >> ({b} & 63)) & 0xffffffffffffffff)",
                    self.rt("s64")
                )
            }
            I32Rotl => format!("{}({a}, {b})", self.rt("i32_rotl")),
            I32Rotr => format!("{}({a}, {b})", self.rt("i32_rotr")),
            I64Rotl => format!("{}({a}, {b})", self.rt("i64_rotl")),
            I64Rotr => format!("{}({a}, {b})", self.rt("i64_rotr")),
            I32Eq | I64Eq => format!("({a} == {b} ? 1 : 0)"),
            I32Ne | I64Ne => format!("({a} != {b} ? 1 : 0)"),
            I32LtU | I64LtU => format!("({a} < {b} ? 1 : 0)"),
            I32GtU | I64GtU => format!("({a} > {b} ? 1 : 0)"),
            I32LeU | I64LeU => format!("({a} <= {b} ? 1 : 0)"),
            I32GeU | I64GeU => format!("({a} >= {b} ? 1 : 0)"),
            I32LtS => format!("({0}({a}) < {0}({b}) ? 1 : 0)", self.rt("s32")),
            I32GtS => format!("({0}({a}) > {0}({b}) ? 1 : 0)", self.rt("s32")),
            I32LeS => format!("({0}({a}) <= {0}({b}) ? 1 : 0)", self.rt("s32")),
            I32GeS => format!("({0}({a}) >= {0}({b}) ? 1 : 0)", self.rt("s32")),
            I64LtS => format!("({0}({a}) < {0}({b}) ? 1 : 0)", self.rt("s64")),
            I64GtS => format!("({0}({a}) > {0}({b}) ? 1 : 0)", self.rt("s64")),
            I64LeS => format!("({0}({a}) <= {0}({b}) ? 1 : 0)", self.rt("s64")),
            I64GeS => format!("({0}({a}) >= {0}({b}) ? 1 : 0)", self.rt("s64")),
            F32Add => format!("{}({a} + {b})", self.rt("f32")),
            F32Sub => format!("{}({a} - {b})", self.rt("f32")),
            F32Mul => format!("{}({a} * {b})", self.rt("f32")),
            F32Div => format!("{}({a} / {b})", self.rt("f32")),
            F64Add => format!("({a} + {b})"),
            F64Sub => format!("({a} - {b})"),
            F64Mul => format!("({a} * {b})"),
            F64Div => format!("({a} / {b})"),
            F32Min | F64Min => format!("{}({a}, {b})", self.rt("fmin")),
            F32Max | F64Max => format!("{}({a}, {b})", self.rt("fmax")),
            F32Copysign => format!("{}({a}, {b})", self.rt("f32_copysign")),
            F64Copysign => format!("{}({a}, {b})", self.rt("f64_copysign")),
            F32Eq | F64Eq => format!("({a} == {b} ? 1 : 0)"),
            F32Ne | F64Ne => format!("({a} != {b} ? 1 : 0)"),
            F32Lt | F64Lt => format!("({a} < {b} ? 1 : 0)"),
            F32Gt | F64Gt => format!("({a} > {b} ? 1 : 0)"),
            F32Le | F64Le => format!("({a} <= {b} ? 1 : 0)"),
            F32Ge | F64Ge => format!("({a} >= {b} ? 1 : 0)"),
        }
    }
}

fn temp(t: Temp) -> String {
    format!("s{}", t.depth)
}

fn default_value(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 | ValType::I64 => "0",
        ValType::F32 | ValType::F64 => "0.0",
    }
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
