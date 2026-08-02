//! Ruby backend: translates dewasm IR into a Ruby class plus a bundled lightweight runtime.
//!
//! Lowering conventions (ADR-4; numeric conventions ADR-2):
//! - i32/i64 are unsigned (masked) Ruby Integers; signed views via `Rt.s32/s64` only where an instruction needs them.
//! - f32/f64 are Ruby Floats; f32 results are re-rounded with `Rt.f32`.
//! - `br` lowers to a method-local `__br` label-variable cascade: blocks and referenced ifs are `begin...end while false`, loops are `while true`, and a multi-level branch sets `__br` to the target label id and `break`s, each crossed frame's epilogue relaying it until the target lands.
//!
//! The runtime is composed from per-method units (ADR-6) and referenced by the relative name `Rt`, so linkage (embedded per class, shared, or a future gem) is the caller's choice.

mod flat;
mod switch;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::{
    check_module_support, Backend, CodeWriter, GenOptions, Mode, OutputFile, RuntimeBundler,
    RuntimeLinkage, RuntimeScope, SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, ElemItem, ElemKind, ExportKind, Expr, LoadOp, Module, Stmt, StoreOp, Temp,
    UnOp, ValType,
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
                    prefix: "global",
                    open: "class Global",
                    close: "end",
                    prelude: Some("global/_class"),
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

/// Emit a top-level shared runtime (`module Rt ... end`) for the closure of `seeds`; generated classes then use `RuntimeLinkage::Alias("::Rt")`.
pub fn shared_runtime(seeds: &BTreeSet<String>) -> Result<String> {
    Ok(format!("module Rt\n{}end\n", bundler().bundle(seeds, 1)?))
}

/// Locate a ruby interpreter able to run generated scripts. Unlike `dewasm_backend_bash::find_bash5`'s version floor: no alternate-path search is needed (ruby is expected on `PATH`), but the generated runtime's memory model is `IO::Buffer`-backed (see docs/adr/33-ruby-io-buffer-memory.md), which requires Ruby >= 3.4. Per ADR-15, fail loud with a setup instruction rather than silently skipping.
pub fn find_ruby() -> Option<std::path::PathBuf> {
    static RUBY: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    RUBY.get_or_init(find_ruby_uncached).clone()
}

/// The probe behind [`find_ruby`], memoized there: it spawns a process per call, and the interpreter cannot change under a running process.
fn find_ruby_uncached() -> Option<std::path::PathBuf> {
    let out = std::process::Command::new("ruby")
        .args(["-e", "print RUBY_VERSION"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&out.stdout);
    let mut parts = version.trim().split('.');
    let major: u32 = parts.next()?.parse().ok()?;
    let minor: u32 = parts.next()?.parse().ok()?;
    ((major, minor) >= (3, 4)).then(|| std::path::PathBuf::from("ruby"))
}

/// Generate one class for `module`. Returns the class source and the set of runtime units it needs (already bundled inside for `Embedded`).
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
    check_module_support(&RubyBackend, module)?;
    // A global needs a shared mutable cell (`Rt::Global`, ADR-16) only if it can cross an instantiation boundary: imported (came from another instance) or exported (another instance may import it later). Every other global is local to this class and never observed from outside it, so it can be a plain ivar holding the value directly.
    let boxed_globals: BTreeSet<u32> = (0..module.imported_globals.len() as u32)
        .chain(module.exports.iter().filter_map(|e| match e.kind {
            ExportKind::Global(idx) => Some(idx),
            _ => None,
        }))
        .collect();
    // Prefix sums: `data_offsets[i]` is where segment `i` begins in the concatenated sidecar blob (ADR-37). Only consulted when externalizing.
    let mut data_offsets = Vec::with_capacity(module.datas.len());
    let mut acc = 0usize;
    for data in &module.datas {
        data_offsets.push(acc);
        acc += data.data.len();
    }
    let gen = Gen {
        module,
        default_wasi,
        uses: RefCell::new(extra_seeds.clone()),
        frames: RefCell::new(FrameSets::default()),
        frame_stack: RefCell::new(Vec::new()),
        flat: RefCell::new(None),
        label_refs: RefCell::new(std::collections::BTreeMap::new()),
        boxed_globals,
        data_file: data_file.map(str::to_string),
        data_offsets,
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

    // The flagship backend's remaining wasm-1.0 + WASI p1 gaps: a dozen WASI p1 functions and the import-limits ledger (ADR-16).
    fn has_wasi_p1(&self, name: &str) -> bool {
        bundler().has_unit(&format!("wasi/{name}"))
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // Part of the wasm 1.0 baseline for Ruby; the row exists for backends whose language lacks floats (ADR-5).
            Feature::Floats => SupportStatus::Supported,
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

        // The Exit/Trap rescue clauses in the standalone main need these even when the module itself never references them.
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
            opts.data_file.as_ref().map(|c| c.sidecar_name.as_str()),
        )?;

        let mut w = CodeWriter::new("  ");
        w.line("# Generated by dewasm. Do not edit.");
        w.line("# frozen_string_literal: false");
        w.line("");
        w.raw(&class_src);

        if opts.mode == Mode::Standalone {
            let wasi_kwargs = wasi_bundled(module, opts.default_wasi);
            w.line("");
            w.block("if __FILE__ == $PROGRAM_NAME", "end", |w| {
                if wasi_kwargs {
                    // Parse the standalone runtime interface (ADR-31): a leading run of `--dir HOST::GUEST` flags mounts host directories at guest paths (wasmtime-style), stopping at `--` or the first non-flag token; the rest is the guest's argv[1..].
                    w.line("preopens = {}");
                    w.line("argv = ARGV.dup");
                    w.line("while (a = argv.first)");
                    w.indent();
                    w.line("if a == \"--\"");
                    w.indent();
                    w.line("argv.shift");
                    w.line("break");
                    w.dedent();
                    w.line("elsif a == \"--dir\"");
                    w.indent();
                    w.line("argv.shift");
                    w.line("spec = argv.shift or abort(\"--dir requires a HOST::GUEST argument\")");
                    w.line("host, guest = spec.split(\"::\", 2)");
                    w.line("preopens[guest || host] = host");
                    w.dedent();
                    w.line("elsif a.start_with?(\"--dir=\")");
                    w.indent();
                    w.line("host, guest = argv.shift.delete_prefix(\"--dir=\").split(\"::\", 2)");
                    w.line("preopens[guest || host] = host");
                    w.dedent();
                    w.line("else");
                    w.indent();
                    w.line("break");
                    w.dedent();
                    w.line("end");
                    w.dedent();
                    w.line("end");
                    w.line(format!(
                        "inst = {class_name}.new({{}}, args: [File.basename($PROGRAM_NAME), *argv], env: ENV.to_h, preopens: preopens)"
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

        let mut files = vec![OutputFile {
            name: format!("{}.rb", opts.module_name),
            contents: w.finish().into_bytes(),
        }];
        // The data sidecar (ADR-37): every segment's bytes concatenated in segment order, matching the `data_offsets` prefix sums baked into the generated `DATA_BLOB.byteslice` calls. Only emitted when there is data to externalize (otherwise the generated code never reads it).
        if let Some(cfg) = &opts.data_file {
            if !module.datas.is_empty() {
                let mut blob = Vec::new();
                for data in &module.datas {
                    blob.extend_from_slice(&data.data);
                }
                files.push(OutputFile {
                    name: cfg.sidecar_name.clone(),
                    contents: blob,
                });
            }
        }
        Ok(files)
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

/// WASI import module names the bundled runtime answers for. `wasi_unstable` (snapshot 0) shares the ABI of preview 1 for everything we implement except fd_seek's whence encoding (snapshot 0 modules that actually seek may misbehave; acceptable until snapshot 0 gets its own units).
const WASI_MODULES: &[&str] = &["wasi_snapshot_preview1", "wasi_unstable"];

/// Widest `call_indirect` signature that gets a fixed-arity `Table#callN` dispatch method (ADR-44); wider signatures fall back to the splat `call`. The `table/call0`..`table/call{MAX_FIXED_ARITY}` runtime units must exist.
const MAX_FIXED_ARITY: usize = 8;

fn is_wasi_module(name: &str) -> bool {
    WASI_MODULES.contains(&name)
}

pub use dewasm_backend::WASI_PREVIEW1_FUNCTIONS;

/// Whether the generated class bundles the built-in WASI as an import fallback (and therefore takes `args:`/`env:` keyword arguments).
fn wasi_bundled(module: &Module, default_wasi: bool) -> bool {
    default_wasi
        && module
            .imported_funcs
            .iter()
            .any(|f| is_wasi_module(&f.module) && bundler().has_unit(&format!("wasi/{}", f.name)))
}

/// Per-function frame classification for the label-variable cascade.
#[derive(Default)]
struct FrameSets {
    /// Capturing frames (`Block`, `Loop`, or referenced-`If`) that a `br` from strictly inside crosses, and so must carry a land-or-relay epilogue.
    crossed: HashSet<u32>,
    /// Loops that a `br` targets from a *strictly nested* capturing frame, so the branch reaches the loop head by a `break` out of an inner scope rather than a direct `next`. Such a loop wraps its body in an inner `begin ... end while false` (the break target) and takes its back-edge through `__br`; every other loop keeps the lean `while true` with a plain `next` back-edge.
    wrapped: HashSet<u32>,
}

/// Compute the [`FrameSets`] for a function body.
///
/// Walking with a stack of the open capturing frames (label id + is-loop), a `br` to target `T` at stack position `pos` (the innermost open frame is the top) is either a self-branch — `pos == top`, a plain `break`/`next` that leaves the innermost frame directly, marking nothing — or an outward branch, `pos < top`, which must traverse `stack[pos..=top]`: the target frame, all pass-through frames, *and* the innermost frame whose own `break` otherwise lands mid-body in its parent. Every frame on that inclusive path needs the epilogue, so all of `stack[pos..]` is `crossed`; and if `T` itself is a loop reached this way (from a nested frame), it is `wrapped`. A plain `if`, `br_if`'s wrapper `if`, and `br_table`'s `case` never capture, so they are not on the stack. `br_if`/`br_table` feed every target through the same routine.
fn compute_frame_sets(body: &[Stmt]) -> FrameSets {
    let mut sets = FrameSets::default();
    let mut stack: Vec<(u32, bool)> = Vec::new();
    walk_frame_sets(body, &mut stack, &mut sets);
    sets
}

fn walk_frame_sets(stmts: &[Stmt], stack: &mut Vec<(u32, bool)>, sets: &mut FrameSets) {
    for stmt in stmts {
        match stmt {
            Stmt::Block { label, body } => {
                stack.push((label.id, false));
                walk_frame_sets(body, stack, sets);
                stack.pop();
            }
            Stmt::Loop { label, body } => {
                stack.push((label.id, true));
                walk_frame_sets(body, stack, sets);
                stack.pop();
            }
            Stmt::If {
                label, then, els, ..
            } => {
                if label.referenced {
                    stack.push((label.id, false));
                }
                walk_frame_sets(then, stack, sets);
                walk_frame_sets(els, stack, sets);
                if label.referenced {
                    stack.pop();
                }
            }
            Stmt::Br(target) => record_target(target, stack, sets),
            Stmt::BrIf { target, .. } => record_target(target, stack, sets),
            Stmt::BrTable {
                targets, default, ..
            } => {
                for target in targets {
                    record_target(target, stack, sets);
                }
                record_target(default, stack, sets);
            }
            _ => {}
        }
    }
}

fn record_target(target: &BrTarget, stack: &[(u32, bool)], sets: &mut FrameSets) {
    if let BrTarget::Label { label, .. } = target {
        if let Some(pos) = stack.iter().position(|(id, _)| id == label) {
            // Outward branch (target is not the innermost frame): mark the whole inclusive path from the target to the innermost frame.
            if pos + 1 < stack.len() {
                sets.crossed.extend(stack[pos..].iter().map(|(id, _)| *id));
                // A loop targeted from a nested frame needs its body-scope wrapper so the relayed `break` re-enters via `next`.
                if stack[pos].1 {
                    sets.wrapped.insert(*label);
                }
            }
        }
    }
}

struct Gen<'a> {
    module: &'a Module,
    default_wasi: bool,
    /// Runtime units the generated code references.
    uses: RefCell<BTreeSet<String>>,
    /// Per-function frame classification, set by `function()`: which frames carry a land-or-relay epilogue and which loops wrap their body. See `compute_frame_sets`.
    frames: RefCell<FrameSets>,
    /// Emission-time stack of capturing frames currently open (label ids), pushed/popped around `Block`/`Loop`/referenced-`If` bodies. `branch()` compares the top of this stack against a `br`'s target to decide between the depth-1 fast path (a plain `break`/`next` that leaves the innermost frame directly) and the cascade (`__br = <id>; break`, relayed by each crossed frame's epilogue until the target lands).
    frame_stack: RefCell<Vec<u32>>,
    /// Flat-dispatch plan for the function being emitted, when it has cross-frame branches (see [`flat`]). `branch()` consults it to emit `state = N; next` instead of the cascade.
    flat: RefCell<Option<flat::Plan>>,
    /// `br` reference counts per label for the function being emitted, used by [`switch::recognize`] to tell a tower's private labels from ones that carry independent meaning.
    label_refs: RefCell<std::collections::BTreeMap<u32, usize>>,
    /// Global indices that need the `Rt::Global` box: imported globals (index space `0..imported_globals.len()`) and every `ExportKind:: Global` target. Computed once in `generate_class_inner`. See the boundary criterion in the comment there and ADR-16.
    boxed_globals: BTreeSet<u32>,
    /// When `Some`, data segments are externalized into a binary sidecar of this filename (referenced via `__dir__`) instead of embedded as hex literals (ADR-37); `data_offsets[i]` locates segment `i` in the blob.
    data_file: Option<String>,
    data_offsets: Vec<usize>,
}

impl<'a> Gen<'a> {
    /// The Ruby expression yielding a data segment's bytes: a slice of the externalized blob when `--data-file` is on, else an inline packed-hex literal (ADR-37). Both yield an ASCII-8BIT (binary) string.
    fn data_expr(&self, seg: usize, data: &[u8]) -> String {
        if self.data_file.is_some() {
            format!(
                "DATA_BLOB.byteslice({}, {})",
                self.data_offsets[seg],
                data.len()
            )
        } else {
            hex_bytes(data)
        }
    }

    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    /// Whether `label_id`'s capturing frame is crossed by some `br` (see `compute_frame_sets`), populated per-function by `function()`.
    fn is_crossed(&self, label_id: u32) -> bool {
        self.frames.borrow().crossed.contains(&label_id)
    }

    /// Whether `label_id`'s loop wraps its body in an inner scope because a `br` targets it from a strictly nested frame (see `compute_frame_sets`).
    fn is_wrapped(&self, label_id: u32) -> bool {
        self.frames.borrow().wrapped.contains(&label_id)
    }

    /// Whether the frame currently on top of `frame_stack` has an enclosing capturing frame — i.e. a `break` emitted in its epilogue has a loop to bind to. A bare `break` at method-body scope is a Ruby SyntaxError, so the outermost frame omits the relay arm (a pending branch can never target something outside it, so that arm is also dead).
    fn has_enclosing_frame(&self) -> bool {
        self.frame_stack.borrow().len() > 1
    }

    /// Land-or-relay epilogue for a crossed `Block`/referenced-`If`, emitted *after* the frame's `end while false` (so a `break` out of the scope — from a nested relay or a direct exit — skips any intervening body code and reaches this decision): if the pending `__br` names this frame, clear it and fall through (the wasm branch lands past the block); otherwise a still-pending `__br` targets an ancestor, so `break` again to relay it outward. Emitted as a single line: epilogues sit at every crossed frame, often deeply indented, so each extra line costs its full indent in output bytes.
    fn emit_land_or_relay(&self, w: &mut CodeWriter, label_id: u32) {
        if self.has_enclosing_frame() {
            w.line(format!(
                "if __br == {label_id} then __br = nil elsif __br then break end"
            ));
        } else {
            w.line(format!("__br = nil if __br == {label_id}"));
        }
    }

    /// An access expression for global `idx`, whichever representation it has: `@g{idx}.value` if it's boxed (crosses an instantiation boundary), plain `@g{idx}` otherwise. Valid on both sides of `=`.
    fn global_ref(&self, idx: u32) -> String {
        if self.boxed_globals.contains(&idx) {
            format!("@g{idx}.value")
        } else {
            format!("@g{idx}")
        }
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

    /// Resolve one import and validate its kind (ADR-7's mechanism, generalized to every import kind): a present-but-wrong-kind value raises immediately (a link error), a missing one returns nil so the caller's `|| fallback` applies.
    fn resolve_import_string(&self, kind: &str, module: &str, name: &str) -> String {
        self.use_unit("rt/resolve_import");
        self.use_unit("rt/check_import_kind");
        format!(
            "Rt.check_import_kind(Rt.resolve_import(imports, {}, {}), :{kind}, {}, {})",
            ruby_string(module),
            ruby_string(name),
            ruby_string(module),
            ruby_string(name)
        )
    }

    fn missing_import_string(&self, module: &str, name: &str) -> String {
        self.use_unit("rt/link_error");
        format!(
            "raise(Rt::LinkError, {})",
            ruby_string(&format!("missing import {module}.{name}"))
        )
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
            w.line("instance_variable_get(GLOBAL_EXPORTS.fetch(name)).value");
        });
        w.line("");
        // The boxed Rt::Global itself (not its current value), for a host embedder or another dewasm instance to import as a shared mutable cell.
        w.block("def global_export(name)", "end", |w| {
            w.line("instance_variable_get(GLOBAL_EXPORTS.fetch(name))");
        });
        w.line("");
        w.block("def table_export(name)", "end", |w| {
            w.line("instance_variable_get(TABLE_EXPORTS.fetch(name))");
        });
        w.line("");
        // ADR-7 provider protocol: an instance of a generated class is itself a valid value in another instance's `imports` table, exposing every export regardless of kind under its one (per-module) namespace — the mechanism the spec harness's `register` support (and any real cross-module linking) uses.
        w.block("def import(name)", "end", |w| {
            w.line("return @exports[name] if @exports.key?(name)");
            w.line("return global_export(name) if GLOBAL_EXPORTS.key?(name)");
            w.line("return table_export(name) if TABLE_EXPORTS.key?(name)");
            w.line("return @memory if MEMORY_EXPORTS.include?(name)");
            w.line("nil");
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
        let global_entries = global_exports
            .iter()
            .map(|(name, idx)| format!("{} => :@g{}", ruby_string(name), idx))
            .collect::<Vec<_>>()
            .join(", ");
        w.line(format!("GLOBAL_EXPORTS = {{ {global_entries} }}.freeze"));
        let table_entries = table_exports
            .iter()
            .map(|(name, idx)| format!("{} => :@t{}", ruby_string(name), idx))
            .collect::<Vec<_>>()
            .join(", ");
        w.line(format!("TABLE_EXPORTS = {{ {table_entries} }}.freeze"));
        let memory_entries = memory_export_names
            .iter()
            .map(|name| ruby_string(name))
            .collect::<Vec<_>>()
            .join(", ");
        w.line(format!("MEMORY_EXPORTS = [{memory_entries}].freeze"));
        // Externalized data blob (ADR-37): read once at class-definition time, kept binary (ASCII-8BIT, as File.binread returns). Only emitted when there is data to externalize.
        if let Some(name) = &self.data_file {
            if !m.datas.is_empty() {
                w.line(format!(
                    "DATA_BLOB = File.binread(File.join(__dir__, {})).freeze",
                    ruby_string(name)
                ));
            }
        }
        w.line("");

        let wasi_fallback = wasi_bundled(m, self.default_wasi);
        let header = if wasi_fallback {
            "def initialize(imports = {}, args: [], env: {}, preopens: {})"
        } else {
            "def initialize(imports = {})"
        };
        w.block(header, "end", |w| {
            if let Some(import) = &m.imported_memory {
                w.line(format!(
                    "@memory = {} || {}",
                    self.resolve_import_string("memory", &import.module, &import.name),
                    self.missing_import_string(&import.module, &import.name),
                ));
            }
            if let Some(mem) = &m.memory {
                self.use_unit("memory/_class");
                let max = mem
                    .max_pages
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "nil".to_string());
                w.line(format!("@memory = Rt::Memory.new({}, {})", mem.min_pages, max));
            }
            for (i, import) in m.imported_tables.iter().enumerate() {
                w.line(format!(
                    "@t{i} = {} || {}",
                    self.resolve_import_string("table", &import.module, &import.name),
                    self.missing_import_string(&import.module, &import.name),
                ));
            }
            let num_imported_tables = m.num_imported_tables();
            for (i, table) in m.tables.iter().enumerate() {
                self.use_unit("table/_class");
                let idx = num_imported_tables as usize + i;
                let max = table
                    .max
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "nil".to_string());
                w.line(format!("@t{idx} = Rt::Table.new({}, {max})", table.min));
            }
            if wasi_fallback {
                w.line("@wasi = nil");
            }
            for (i, import) in m.imported_funcs.iter().enumerate() {
                // Fallback order: explicit import -> bundled WASI (constructed only when first needed) -> ENOSYS stub; non-WASI imports stay mandatory.
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
                    self.missing_import_string(&import.module, &import.name)
                };
                w.line(format!(
                    "@if{i} = {} || {fallback}",
                    self.resolve_import_string("func", &import.module, &import.name)
                ));
            }
            for (i, import) in m.imported_globals.iter().enumerate() {
                w.line(format!(
                    "@g{i} = {} || {}",
                    self.resolve_import_string("global", &import.module, &import.name),
                    self.missing_import_string(&import.module, &import.name),
                ));
            }
            let num_imported_globals = m.imported_globals.len();
            for (i, global) in m.globals.iter().enumerate() {
                let idx = (num_imported_globals + i) as u32;
                let init = self.expr(&global.init);
                if self.boxed_globals.contains(&idx) {
                    self.use_unit("global/_class");
                    w.line(format!("@g{idx} = Rt::Global.new({init})"));
                } else {
                    w.line(format!("@g{idx} = {init}"));
                }
            }
            for (i, elem) in m.elems.iter().enumerate() {
                // Built lazily: Declared segments emit an empty array and never need the rendered items.
                let items = || {
                    elem.items
                        .iter()
                        .map(|item| match item {
                            ElemItem::Func(func_idx) => self.func_pair(*func_idx),
                            ElemItem::Null => "nil".to_string(),
                            ElemItem::Global(idx) => self.global_ref(*idx),
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                match &elem.kind {
                    ElemKind::Declared => w.line(format!("@elem{i} = []")),
                    ElemKind::Passive => w.line(format!("@elem{i} = [{}]", items())),
                    ElemKind::Active {
                        table_index,
                        offset,
                    } => {
                        self.use_unit("table/init");
                        w.line(format!("@elem{i} = [{}]", items()));
                        let offset = self.expr(offset);
                        w.line(format!(
                            "@t{table_index}.init({offset}, @elem{i}, 0, {})",
                            elem.items.len()
                        ));
                        w.line(format!("@elem{i} = []"));
                    }
                }
            }
            for (i, data) in m.datas.iter().enumerate() {
                match &data.offset {
                    Some(offset) => {
                        self.use_unit("memory/init");
                        w.line(format!(
                            "@memory.init({}, {}, 0, {})",
                            self.expr(offset),
                            self.data_expr(i, &data.data),
                            data.data.len()
                        ));
                        w.line(format!("@data{i} = \"\".b"));
                    }
                    None => {
                        w.line(format!("@data{i} = {}", self.data_expr(i, &data.data)));
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

            // The instance is complete: let import providers bind to it (memory access etc.) before any wasm code can run.
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

    /// call_indirect compares types structurally, and a table can be shared across modules (imported tables), so the runtime type id must not come from any module-local index space: derive an interned symbol from the type's shape instead.
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
        format!(":\"{}->{}\"", names(&ty.params), names(&ty.results))
    }

    /// A callable object for the function (used in tables and exports).
    fn func_ref(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("@if{func_idx}")
        } else {
            format!("method(:_f{func_idx})")
        }
    }

    /// A funcref value: the `[type_symbol, callable]` pair tables store (ADR-16). Element items and `call_indirect` agree on this shape.
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
            format!("@if{func_idx}.call({args})")
        } else if args.is_empty() {
            format!("_f{func_idx}")
        } else {
            format!("_f{func_idx}({args})")
        }
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &dewasm_core::ir::Func) {
        {
            let mut refs = self.label_refs.borrow_mut();
            refs.clear();
            switch::count_label_refs(&func.body, &mut refs);
        }
        *self.frames.borrow_mut() = compute_frame_sets(&func.body);
        self.frame_stack.borrow_mut().clear();
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
            // Hoist all temps to method scope: assignments inside the `begin`/`while` frames would otherwise be block-local in Ruby. The pending-branch variable `__br` is hoisted alongside them whenever the function has any crossed frame (a fallthrough epilogue reads `__br` before any branch assigns it); with no crossed frame nothing ever references it.
            let mut depths: Vec<u32> = func.temps.iter().map(|t| t.depth).collect();
            depths.dedup();
            let mut decl = String::new();
            if !self.frames.borrow().crossed.is_empty() {
                decl.push_str("__br = ");
            }
            for d in &depths {
                decl.push_str(&format!("s{d} = "));
            }
            if !decl.is_empty() {
                w.line(format!("{decl}nil"));
            }
            let plan = flat::plan(&func.body, &self.frames.borrow().crossed);
            match plan {
                None => {
                    *self.flat.borrow_mut() = None;
                    self.stmts(w, &func.body);
                }
                Some(plan) => {
                    let n = plan.nstates as usize;
                    *self.flat.borrow_mut() = Some(plan);
                    let mut st: Vec<CodeWriter> = (0..n).map(|_| CodeWriter::new("  ")).collect();
                    let last = self.flat_seq(&mut st, 0, &func.body);
                    // Falling off the body ends the function; leave the dispatch loop.
                    st[last].line(format!("state = {n}; next"));
                    let texts = clean_states(st.into_iter().map(|c| c.finish()).collect());
                    w.line("state = 0");
                    w.block("while true", "end", |w| {
                        w.line("case state");
                        for (i, body) in texts.iter().enumerate() {
                            let Some(body) = body else { continue };
                            w.line(format!("when {i}"));
                            w.indent();
                            for line in body.lines() {
                                w.line(line);
                            }
                            w.dedent();
                        }
                        w.line("else");
                        w.indent();
                        w.line("break");
                        w.dedent();
                        w.line("end");
                    });
                    *self.flat.borrow_mut() = None;
                }
            }
        });
    }

    /// Emit a recovered switch as `case … when …`. Integer-literal `when`s let Ruby compile the dispatch to `opt_case_dispatch` (a hash jump) rather than a chain of compares.
    fn switch_stmt(&self, w: &mut CodeWriter, sw: &switch::Switch) {
        self.stmts(w, sw.prelude);
        w.line(format!("case {}", self.expr(sw.index)));
        let arm = |w: &mut CodeWriter, a: &switch::Arm| {
            w.indent();
            match a {
                // Falling out of the `case` lands exactly where a branch to the tower's outermost label did.
                switch::Arm::Leave => w.line("nil"),
                switch::Arm::Body(chain) if chain.iter().all(|c| c.is_empty()) => w.line("nil"),
                switch::Arm::Body(chain) => {
                    for part in chain {
                        self.stmts(w, part);
                    }
                }
                switch::Arm::Branch(t) => self.branch(w, t),
            }
            w.dedent();
        };
        for (values, a) in &sw.arms {
            let vs = values
                .iter()
                .map(|v| v.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            w.line(format!("when {vs}"));
            arm(w, a);
        }
        w.line("else");
        arm(w, &sw.default);
        w.line("end");
    }

    /// Emit `stmts` into a state machine, splitting at each dissolved frame.
    /// Returns the state control is in afterwards.
    fn flat_seq(&self, st: &mut [CodeWriter], mut cur: usize, stmts: &[Stmt]) -> usize {
        for stmt in stmts {
            let plan_hit = {
                let p = self.flat.borrow();
                let p = p.as_ref().unwrap();
                match stmt {
                    Stmt::Block { label, .. }
                    | Stmt::Loop { label, .. }
                    | Stmt::If { label, .. }
                        if p.dissolved.contains(&label.id) =>
                    {
                        Some((label.id, p.state_of[&label.id], p.after[&label.id]))
                    }
                    _ => None,
                }
            };
            let Some((_id, target, after)) = plan_hit else {
                self.stmt(&mut st[cur], stmt);
                continue;
            };
            match stmt {
                Stmt::Block { body, .. } => {
                    cur = self.flat_seq(st, cur, body);
                    st[cur].line(format!("state = {after}; next"));
                    cur = after as usize;
                }
                Stmt::Loop { body, .. } => {
                    // `target` is the head: entering the loop and taking its
                    // back-edge are the same transition.
                    st[cur].line(format!("state = {target}; next"));
                    cur = target as usize;
                    cur = self.flat_seq(st, cur, body);
                    st[cur].line(format!("state = {after}; next"));
                    cur = after as usize;
                }
                Stmt::If {
                    cond, then, els, ..
                } => {
                    // The arms stay inline — `if` is not a Ruby loop, so a `next`
                    // inside one already reaches the dispatch loop.
                    st[cur].line(format!("if {}", self.cond(cond)));
                    st[cur].indent();
                    let a = self.flat_seq(st, cur, then);
                    st[a].line(format!("state = {after}; next"));
                    st[cur].dedent();
                    if !els.is_empty() {
                        st[cur].line("else");
                        st[cur].indent();
                        let b = self.flat_seq(st, cur, els);
                        st[b].line(format!("state = {after}; next"));
                        st[cur].dedent();
                    }
                    st[cur].line("end");
                    st[cur].line(format!("state = {after}; next"));
                    cur = after as usize;
                }
                _ => unreachable!(),
            }
        }
        cur
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
                w.line(format!("{} = {}", self.global_ref(*idx), self.expr(expr)));
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
                // A recovered `switch` keeps only its outermost frame — arms branch to it as the join — and replaces the rest of the tower with one `case`, so those branches stop relaying through the ladder entirely (see `switch`).
                if let Ok(sw) = switch::recognize(label.id, body, &self.label_refs.borrow()) {
                    let crossed = self.is_crossed(label.id);
                    self.frame_stack.borrow_mut().push(label.id);
                    w.block("begin", "end while false", |w| self.switch_stmt(w, &sw));
                    if crossed {
                        self.emit_land_or_relay(w, label.id);
                    }
                    self.frame_stack.borrow_mut().pop();
                    return;
                }
                let crossed = self.is_crossed(label.id);
                self.frame_stack.borrow_mut().push(label.id);
                w.block("begin", "end while false", |w| self.stmts(w, body));
                if crossed {
                    self.emit_land_or_relay(w, label.id);
                }
                self.frame_stack.borrow_mut().pop();
            }
            Stmt::Loop { label, body } => {
                let crossed = self.is_crossed(label.id);
                let wrapped = self.is_wrapped(label.id);
                self.frame_stack.borrow_mut().push(label.id);
                if wrapped {
                    // A `br` targets this loop from a nested frame, arriving with `__br` set by `break`ing out of the inner `begin` (skipping any code left in the body). The decision then re-enters via `next`, or `break`s the `while` so the post-loop relay can pass `__br` further out. A plain fallthrough leaves `__br` nil and exits the loop.
                    w.block("while true", "end", |w| {
                        w.block("begin", "end while false", |w| self.stmts(w, body));
                        w.line(format!(
                            "if __br == {} then __br = nil; next else break end",
                            label.id
                        ));
                    });
                } else {
                    // No `br` reaches this loop from a nested frame: a back-edge is a plain `next` (see `branch()`), a fallthrough exits via the trailing `break`, and an outward `br` sets `__br` and `break`s the `while` for the post-loop relay to pass outward.
                    w.block("while true", "end", |w| {
                        self.stmts(w, body);
                        w.line("break");
                    });
                }
                // A `br` that passes through this loop toward an ancestor left `__br` set and `break`ed the `while`; relay it outward.
                if crossed && self.has_enclosing_frame() {
                    w.line("break if __br");
                }
                self.frame_stack.borrow_mut().pop();
            }
            Stmt::If {
                label,
                cond,
                then,
                els,
            } => {
                let emit_if = |w: &mut CodeWriter, gen: &Self| {
                    w.line(format!("if {}", gen.cond(cond)));
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
                    let crossed = self.is_crossed(label.id);
                    self.frame_stack.borrow_mut().push(label.id);
                    w.block("begin", "end while false", |w| emit_if(w, self));
                    if crossed {
                        self.emit_land_or_relay(w, label.id);
                    }
                    self.frame_stack.borrow_mut().pop();
                } else {
                    emit_if(w, self);
                }
            }
            Stmt::Br(target) => self.branch(w, target),
            Stmt::BrIf { cond, target } => {
                w.block(format!("if {}", self.cond(cond)), "end", |w| {
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
                table_index,
                index,
                args,
                results,
            } => {
                // Fixed-arity dispatch (ADR-44): a per-arity `callN` avoids building a `*args` array on either side; the splat `call` stays as the fallback for signatures wider than MAX_FIXED_ARITY (unobserved in the real-world apps, whose call_indirect arities top out at 8).
                let mut call_args = vec![self.expr(index), self.type_symbol(*type_idx)];
                call_args.extend(args.iter().map(|a| self.expr(a)));
                let method = if args.len() <= MAX_FIXED_ARITY {
                    let n = args.len();
                    self.use_unit(&format!("table/call{n}"));
                    format!("call{n}")
                } else {
                    self.use_unit("table/call");
                    "call".to_string()
                };
                let call = format!("@t{table_index}.{method}({})", call_args.join(", "));
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
            Stmt::TableInit {
                seg,
                table_index,
                dst,
                src,
                len,
            } => {
                self.use_unit("table/init");
                w.line(format!(
                    "@t{table_index}.init({}, @elem{seg}, {}, {})",
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
                    "@t{dst_table}.copy({}, @t{src_table}, {}, {})",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::ElemDrop { seg } => {
                w.line(format!("@elem{seg} = []"));
            }
            Stmt::Unreachable => {
                w.line(format!("{}(\"unreachable\")", self.rt("trap")));
            }
            Stmt::SourceLine(pos) => {
                // A source-position back-mapping comment (ADR-38); inert.
                let file = &self.module.debug_files[pos.file as usize];
                w.line(format!("# {file}:{}", pos.line));
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
                // Addressed by value: one assignment plus a hash dispatch, the same cost from any depth.
                if let Some(plan) = self.flat.borrow().as_ref() {
                    if let Some(st) = plan.state_of.get(label) {
                        w.line(format!("state = {st}; next"));
                        return;
                    }
                }
                // Fast path — a br whose target is the innermost enclosing frame leaves it directly: `break` out of a block/if's `begin...end while false`, or `next` to take an unwrapped loop's back-edge. Everything else (a br to an outer frame, or a back-edge into a *wrapped* loop, whose body sits in an inner `begin`) sets the pending label variable and `break`s out of the innermost scope; each crossed frame's epilogue then relays `__br` outward until the target lands.
                let innermost = self.frame_stack.borrow().last() == Some(label);
                let via_var = !innermost || (*is_loop && self.is_wrapped(*label));
                if via_var {
                    w.line(format!("__br = {label}; break"));
                } else {
                    w.line(if *is_loop { "next" } else { "break" });
                }
            }
        }
    }

    /// An expression in boolean context (an `if`/`br_if` test).
    ///
    /// A wasm comparison yields the i32 0 or 1, and every conditional context
    /// then compares that against 0 — so the lowering built a ternary only to
    /// undo it one operation later. In `sqlite3-shell` 24,794 of the 25,716
    /// emitted ternaries are immediately tested this way. Emitting the
    /// comparison as a Ruby boolean drops both the ternary and the test; the
    /// operands are untouched, so signed views still go through `Rt.s32`/`s64`.
    fn cond(&self, e: &Expr) -> String {
        use BinOp::*;
        let rel = |op: &BinOp| -> Option<(&'static str, Option<&'static str>)> {
            Some(match op {
                I32Eq | I64Eq | F32Eq | F64Eq => ("==", None),
                I32Ne | I64Ne | F32Ne | F64Ne => ("!=", None),
                I32LtU | I64LtU | F32Lt | F64Lt => ("<", None),
                I32GtU | I64GtU | F32Gt | F64Gt => (">", None),
                I32LeU | I64LeU | F32Le | F64Le => ("<=", None),
                I32GeU | I64GeU | F32Ge | F64Ge => (">=", None),
                I32LtS => ("<", Some("s32")),
                I32GtS => (">", Some("s32")),
                I32LeS => ("<=", Some("s32")),
                I32GeS => (">=", Some("s32")),
                I64LtS => ("<", Some("s64")),
                I64GtS => (">", Some("s64")),
                I64LeS => ("<=", Some("s64")),
                I64GeS => (">=", Some("s64")),
                _ => return None,
            })
        };
        match e {
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => format!("{} == 0", self.expr(a)),
            Expr::Bin(op, a, b) => match rel(op) {
                Some((r, None)) => format!("{} {r} {}", self.expr(a), self.expr(b)),
                Some((r, Some(sign))) => {
                    let f = self.rt(sign);
                    format!("{f}({}) {r} {f}({})", self.expr(a), self.expr(b))
                }
                None => format!("{} != 0", self.expr(e)),
            },
            _ => format!("{} != 0", self.expr(e)),
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
            Expr::GlobalGet(idx) => self.global_ref(*idx),
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
            I64Add => format!("{}({a} + {b})", self.rt("m64")),
            I64Sub => format!("{}({a} - {b})", self.rt("m64")),
            I64Mul => format!("{}({a} * {b})", self.rt("m64")),
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
            I64Shl => format!("{}({a} << ({b} & 63))", self.rt("m64")),
            I64ShrU => format!("({a} >> ({b} & 63))"),
            I64ShrS => {
                format!("{}({}({a}) >> ({b} & 63))", self.rt("m64"), self.rt("s64"))
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
            // GCC-built MRI (any arch; every version probed) leaves a signaling NaN unquieted when the RHS is +0.0: the flonum decode returns +0.0 as a literal, and GCC folds `a - 0.0` to `a`, skipping the FPU sub. The host `-` therefore cannot be trusted to return an arithmetic NaN. Quiet any NaN result explicitly; the `== r` self-compare is false only for NaN, keeping the common finite path allocation- and call-free. ADR-47, issue #11.
            F64Sub => format!("((r = {a} - {b}) == r ? r : {}(r))", self.rt("quiet_nan")),
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

/// Clean up the emitted state bodies.
///
/// Three passes, in order:
///
/// 1. **Dead code.** `flat_seq` appends a frame's exit transition whether or not
///    the body already ended in one, so anything after an unconditional
///    transition at an arm's top level is unreachable. On `sqlite3-shell` that
///    was **52.7% of every line in `_f99`'s states** — code the JIT still has to
///    compile and the instruction cache still has to hold.
/// 2. **Merge.** With the dead copies gone, a state whose only remaining entry
///    is one predecessor's trailing transition is just that predecessor's
///    continuation, so it is spliced in and its arm disappears.
/// 3. **Trailing `next`.** A transition that ends an arm can fall out of the
///    `case` instead: the `while` loops anyway.
fn clean_states(texts: Vec<String>) -> Vec<Option<String>> {
    let is_goto = |l: &str| -> Option<usize> {
        let t = l.trim();
        if l.starts_with(char::is_whitespace) {
            return None; // nested in an `if`: does not end the arm
        }
        t.strip_prefix("state = ")
            .map(|r| r.strip_suffix("; next").unwrap_or(r))
            .and_then(|r| r.parse().ok())
    };

    // 1. drop unreachable tails
    let mut out: Vec<Option<String>> = texts
        .into_iter()
        .map(|body| {
            let mut keep = Vec::new();
            for l in body.lines() {
                keep.push(l.to_string());
                if is_goto(l).is_some() {
                    break;
                }
            }
            Some(
                keep.join(
                    "
",
                ) + "
",
            )
        })
        .collect();

    // 2. splice single-entry states into their sole predecessor
    let n = out.len();
    loop {
        let mut refs = vec![0usize; n + 1];
        for body in out.iter().flatten() {
            for l in body.lines() {
                if let Some(t) = l
                    .trim()
                    .strip_prefix("state = ")
                    .map(|r| r.strip_suffix("; next").unwrap_or(r))
                    .and_then(|r| r.parse::<usize>().ok())
                {
                    if t <= n {
                        refs[t] += 1;
                    }
                }
            }
        }
        let mut merged = false;
        for p in 0..n {
            let Some(body) = out[p].clone() else { continue };
            let Some(t) = body.lines().next_back().and_then(is_goto) else {
                continue;
            };
            if t == 0 || t >= n || t == p || refs[t] != 1 || out[t].is_none() {
                continue;
            }
            let head: String = body
                .lines()
                .take(body.lines().count() - 1)
                .map(|l| {
                    format!(
                        "{l}
"
                    )
                })
                .collect();
            let tail = out[t].take().unwrap();
            out[p] = Some(format!("{head}{tail}"));
            merged = true;
            break;
        }
        if !merged {
            break;
        }
    }

    // 3. an arm-final transition can fall out of the `case`
    out.into_iter()
        .map(|b| {
            b.map(|body| {
                let Some(tail) = body.lines().next_back() else {
                    return body;
                };
                let Some(rest) = tail.strip_suffix("; next") else {
                    return body;
                };
                if is_goto(tail).is_none() {
                    return body;
                }
                let head: String = body
                    .lines()
                    .take(body.lines().count() - 1)
                    .map(|l| {
                        format!(
                            "{l}
"
                        )
                    })
                    .collect();
                format!(
                    "{head}{rest}
"
                )
            })
        })
        .collect()
}

fn temp(t: Temp) -> String {
    format!("s{}", t.depth)
}

fn default_value(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 | ValType::I64 => "0",
        ValType::F32 | ValType::F64 => "0.0",
        ValType::FuncRef => "nil",
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

/// Lint for the runtime units: every reference a unit body makes to another unit must be declared in its `# requires:` header. This is the static half of the drift defence; the dynamic half is the spec harness running against minimal bundles (ADR-6).
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
        let rt_const = Regex::new(r"Rt::([A-Z]\w*)").unwrap();
        let memory_call = Regex::new(r"@memory\.([a-z_][a-z0-9_]*)").unwrap();
        // One precompiled bare-call matcher per unit name.
        let bare_calls: Vec<(&str, Regex)> = unit_ids
            .iter()
            .map(|id| {
                let name = id.split('/').nth(1).unwrap();
                let re = Regex::new(&format!(r#"(^|[^\w.@:"]){}\("#, regex::escape(name))).unwrap();
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
                    "M32" | "M64" => continue, // root prelude, always bundled
                    "Memory" => "memory/_class".to_string(),
                    "Table" => "table/_class".to_string(),
                    "WASI" => "wasi/_class".to_string(),
                    other => panic!("{}: unknown runtime constant Rt::{other}", unit.id),
                };
                demand(dep, &format!("Rt::{}", &cap[1]));
            }
            for cap in memory_call.captures_iter(&code) {
                demand(
                    format!("memory/{}", &cap[1]),
                    &format!("@memory.{}", &cap[1]),
                );
            }

            // Bare sibling calls within the same scope (with parentheses; a parenless bare call cannot be told apart from a local variable, so those cases must keep their requires by hand).
            for (sibling, bare) in &bare_calls {
                let Some(name) = sibling.strip_prefix(&format!("{scope}/")) else {
                    continue;
                };
                if name.starts_with('_') || *sibling == unit.id {
                    continue;
                }
                if bare.is_match(&code) {
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
}

/// Codegen-shape checks for control flow: a multi-level `br` must be addressed by *value* — a state assignment plus `next` into the dispatch loop — never by walking scopes (the ADR-42 `__br` cascade) and never by `catch`/`throw` (ADR-4).
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

    // block $A { loop $B { block $C { br_table $C $B $A } ... } } — a single `br_table` whose targets span all three nesting depths (self-exit, loop-continue from a nested frame, and outer-block-exit), exercising the fast path, a wrapped loop, and a relayed landing at once.
    const MIXED_DEPTHS: &str = r#"
      (module
        (func (export "f") (param i32) (result i32)
          (local i32)
          (block $A
            (loop $B
              (block $C
                (br_table $C $B $A (local.get 0)))
              (local.set 1 (i32.const 7))))
          (local.get 1)))
    "#;

    #[test]
    fn multi_level_br_is_addressed_by_value() {
        let src = convert(MIXED_DEPTHS);
        // The dispatch loop, and a branch that names its target as a number.
        assert!(src.contains("state = 0"), "no dispatch entry in:\n{src}");
        assert!(src.contains("case state"), "no dispatch in:\n{src}");
        assert!(src.contains("; next"), "no state transition in:\n{src}");
        // The retired shapes: scope-walking relay (ADR-42) and catch/throw (ADR-4).
        assert!(!src.contains("elsif __br"), "relay arm survived in:\n{src}");
        assert!(
            !src.contains("end while false"),
            "cascade frame survived in:\n{src}"
        );
        assert!(!src.contains("catch("), "catch survived in:\n{src}");
        assert!(!src.contains("throw "), "throw survived in:\n{src}");
    }
}
