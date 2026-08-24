//! Ruby backend: translates dewasm IR into a Ruby class plus a bundled lightweight runtime.
//!
//! Lowering conventions:
//! - i32/i64 are unsigned (masked) Ruby Integers; signed views via `s32`/`s64` only where an instruction needs them.
//! - f32/f64 are Ruby Floats; f32 results are re-rounded with `f32`.
//! - `br` lowers to a method-local `__br` label-variable cascade: blocks and referenced ifs are `begin...end while false`, loops are `while true`, and a multi-level branch sets `__br` to the target label id and `break`s, each crossed frame's epilogue relaying it until the target lands.
//! - A branch crossing 16 frames or more is addressed by value instead: the frames it crosses dissolve into a `case state` dispatch loop, so it costs one assignment at any depth (see [`flat`]).
//!   Shallower crossings keep the cascade and uncrossed frames stay structured, side by side in the same function.
//!
//! The runtime is composed from per-method units and referenced by the relative name `Rt`, so linkage (embedded per class or shared) is the caller's choice.

/// Flat dispatch, shared with the Python backend ([`dewasm_backend::flat`]); only the threshold below is Ruby's own.
mod flat {
    pub use dewasm_backend::flat::*;

    /// Crossing depth from which a branch is worth a dispatch.
    /// A relay costs one compare per crossed frame (measured at 0.82 ns/level under `--yjit`, flat from depth 2 to 32), while a dispatch is a `case`-over-integers chain whose cost grows with the number of *hot* states, measured at 0.9 ns for 3 hot states, 4.1 ns for 20 and 25 ns for 80.
    /// So the break-even sits somewhere between 5 and 30 crossed frames depending on how large the state machine ends up, and any threshold inside that band is a judgement call rather than a derived constant. 16 is the value picked, since it puts the two measured workloads on the side each was measured to prefer: `nes.wasm` crosses at most 12 frames anywhere in the module and is 1.18x faster fully cascaded, `sqlite3-shell` reaches 278 and is 2.08x faster flattened.
    pub const DEEP_CROSSING: usize = 16;
}

use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::masking::{
    bin_operand_context, fold_and_chain, shift_count_mode, shift_width, un_operand_context,
    Elision, MaskContext, ShiftCountMode,
};
use dewasm_backend::{
    check_module_support, hex_string, is_boolean, is_ident, is_wasi_module, load_code, local_runs,
    module_name_error, signed_view_rel_op, store_code, terminates, type_key, wasi_bundled, Backend,
    CodeWriter, GenOptions, Mode, OutputFile, RuntimeBundler, RuntimeLinkage, RuntimeScope,
    SupportStatus,
};
use dewasm_backend::{extract, fuse, licm};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, CatchClause, ElemItem, ElemKind, ExportKind, Expr, Module, Stmt, Temp, UnOp,
    ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The runtime unit bundler for Ruby (see crates/dewasm-backend-ruby/units/).
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| {
        RuntimeBundler::new(
            "#",
            "\t",
            2,
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

/// Locate a ruby interpreter able to run generated scripts: at least 3.4, because the generated runtime's memory is `IO::Buffer`-backed.
/// Honors `$DEWASM_RUBY`, then `ruby` on `PATH`.
/// A missing or too-old interpreter fails loud with a setup instruction rather than silently skipping.
pub fn find_ruby() -> Option<std::path::PathBuf> {
    static RUBY: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    RUBY.get_or_init(find_ruby_uncached).clone()
}

/// The probe behind [`find_ruby`], memoized there: it spawns a process per candidate per call, and the interpreter cannot change under a running process.
fn find_ruby_uncached() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("DEWASM_RUBY") {
        candidates.push(PathBuf::from(env));
    }
    candidates.push(PathBuf::from("ruby"));
    for candidate in candidates {
        let Ok(out) = std::process::Command::new(&candidate)
            .args(["-e", "print RUBY_VERSION"])
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        let version = String::from_utf8_lossy(&out.stdout);
        let mut parts = version.trim().split('.');
        let (Some(major), Some(minor)) = (
            parts.next().and_then(|p| p.parse::<u32>().ok()),
            parts.next().and_then(|p| p.parse::<u32>().ok()),
        ) else {
            continue;
        };
        if (major, minor) >= (3, 4) {
            return Some(candidate);
        }
    }
    None
}

/// Generate one class for `module`.
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
    check_module_support(&RubyBackend, module)?;
    // A global needs a shared mutable cell (`Rt::Global`) only if it can cross an instantiation boundary: imported (came from another instance) or exported (another instance may import it later).
    // Every other global is local to this class and never observed from outside it, so it can be a plain ivar holding the value directly.
    let boxed_globals: BTreeSet<u32> = (0..module.imported_globals.len() as u32)
        .chain(module.exports.iter().filter_map(|e| match e.kind {
            ExportKind::Global(idx) => Some(idx),
            _ => None,
        }))
        .collect();
    let mut data_offsets = Vec::with_capacity(module.datas.len());
    let mut acc = 0usize;
    for data in &module.datas {
        data_offsets.push(acc);
        acc += data.data.len();
    }
    let gen = Gen {
        module,
        extracted: transformed_funcs(module),
        default_wasi,
        uses: RefCell::new(extra_seeds.clone()),
        frames: RefCell::new(flat::Frames::default()),
        frame_stack: RefCell::new(Vec::new()),
        flat: RefCell::new(None),
        dead_clears: RefCell::new(HashSet::new()),
        elision: RefCell::new(Elision::none(FIXNUM_LIMIT)),
        boxed_globals,
        data_file: data_file.map(str::to_string),
        data_offsets,
    };
    let mut wb = CodeWriter::new("\t");
    wb.indent();
    gen.body(&mut wb);
    let body = wb.finish();
    let uses = gen.uses.into_inner();

    let mut out = ancestor_guards(class_name);
    out.push_str(&format!("class {class_name}\n"));
    // `include Rt` makes the runtime's `module_function` helpers private instance methods of the class, so generated code calls them by bare name (`m64(x)`) instead of naming the module at every site.
    // Constants stay `Rt::`-qualified.
    match linkage {
        RuntimeLinkage::Embedded => {
            if !uses.is_empty() {
                out.push_str("\tmodule Rt\n");
                out.push_str(&bundler().bundle(&uses, 2)?);
                out.push_str("\tend\n\n\tinclude Rt\n\n");
            }
        }
        RuntimeLinkage::Alias(path) => {
            out.push_str(&format!("\tRt = {path}\n\n\tinclude Rt\n\n"));
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

    fn has_wasi_p1(&self, name: &str) -> bool {
        bundler().has_unit(&format!("wasi/{name}"))
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // Part of the wasm 1.0 baseline for Ruby; the row exists for backends whose language lacks floats.
            Feature::Floats => SupportStatus::Supported,
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
            // Tags are identity objects, a thrown exception is a native Ruby exception that doubles as the exnref, and traps stay uncatchable.
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
            opts.data_file.as_ref().map(|c| c.data_file_name.as_str()),
        )?;

        let mut w = CodeWriter::new("\t");
        w.line("# Generated by dewasm. Do not edit.");
        w.line("# frozen_string_literal: false");
        w.line("");
        w.raw(&class_src);

        if opts.mode == Mode::Standalone {
            let wasi_kwargs = wasi_bundled(module, opts.default_wasi, bundler());
            w.line("");
            w.block("if __FILE__ == $PROGRAM_NAME", "end", |w| {
                if wasi_kwargs {
                    // Parse the standalone runtime interface: a leading run of `--dir HOST::GUEST` flags mounts host directories at guest paths (wasmtime-style), stopping at `--` or the first non-flag token; the rest is the guest's argv[1..].
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
        // The data file: every segment's bytes concatenated in segment order, matching the `data_offsets` prefix sums baked into the generated `DATA_BLOB.byteslice` calls.
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

/// The class a `--mode standalone` program defines.
/// A standalone artifact is a self-contained program whose internal name nothing outside it can observe, so it is fixed rather than derived from the input.
pub const STANDALONE_CLASS: &str = "Program";

/// The library-mode module name must be a Ruby constant path (`::`-separated segments, each `[A-Z][A-Za-z0-9_]*`) and is used verbatim.
/// Nothing is sanitized: a name that is not a legal constant path is a conversion-time error.
fn check_module_name(name: &str) -> Result<()> {
    let ok = name.split("::").all(|seg| {
        is_ident(
            seg,
            |c| c.is_ascii_uppercase(),
            |c| c.is_ascii_alphanumeric() || c == '_',
        )
    });
    if ok {
        Ok(())
    } else {
        Err(module_name_error(
            "ruby",
            name,
            "a constant path: `::`-separated segments each matching [A-Z][A-Za-z0-9_]* (e.g. Add, Dewasm::Sqlite3)",
        ))
    }
}

/// `class A::B::C` needs `A` and `A::B` to exist, and the file must load both on its own and next to something that already defined them: each ancestor gets an unless-defined? guard, outermost first.
/// An ancestor that already exists is left alone, whatever it is: bound to a non-module constant it fails loudly at load, which is the correct outcome.
fn ancestor_guards(class_name: &str) -> String {
    let segs: Vec<&str> = class_name.split("::").collect();
    let mut out = String::new();
    for i in 1..segs.len() {
        let path = segs[..i].join("::");
        out.push_str(&format!(
            "unless defined?({path})\n\tmodule {path}; end\nend\n"
        ));
    }
    out
}

/// Ruby double-quoted string literal (`#` escaped too, since it opens an interpolation).
pub fn ruby_string(s: &str) -> String {
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
    format!("[\"{}\"].pack(\"H*\")", hex_string(data))
}

/// Labels whose epilogue at method-body level (`__br = nil if __br == {id}`, see [`Gen::emit_land_or_relay`]) may be omitted because no later emission reads `__br`.
///
/// At an epilogue with no enclosing capturing frame, a pending `__br` can only name that frame: nothing outer exists for a relay to reach (a lexical ancestor a branch could target would be on that branch's path and, dissolution being all-or-nothing per path, could not have dissolved alone).
/// So the statement never redirects control; it only resets `__br` to nil for whatever reads it later, and where nothing does, it is dead.
///
/// The walk is over emission order, which the structured lowering executes front to back, so a clear is dead when no read follows it in that order.
/// A dissolved loop breaks that equation (its back-edge re-runs reads that sit before the clear), so nothing under one is ever marked.
fn dead_clears(body: &[Stmt], frames: &flat::Frames, plan: Option<&flat::Plan>) -> HashSet<u32> {
    let none = HashSet::new();
    let dissolved = plan.map_or(&none, |p| &p.dissolved);
    let mut dead = HashSet::new();
    collect_dead_clears(body, frames, dissolved, false, &mut dead);
    dead
}

/// Walk `stmts` backward, marking each frame that emits a method-body-level clear no later `__br` read can observe.
/// `reads_after` states whether some emission after this sequence completes reads `__br`.
fn collect_dead_clears(
    stmts: &[Stmt],
    frames: &flat::Frames,
    dissolved: &HashSet<u32>,
    mut reads_after: bool,
    dead: &mut HashSet<u32>,
) {
    for stmt in stmts.iter().rev() {
        match stmt {
            Stmt::Block { label, .. } | Stmt::TryTable { label, .. } => {
                if dissolved.contains(&label.id) {
                    // A dissolved block emits no scope, so its body sits at method-body level and runs on into whatever follows the block.
                    for seq in stmt.child_seqs() {
                        collect_dead_clears(seq, frames, dissolved, reads_after, dead);
                    }
                } else if frames.crossed.contains(&label.id) && !reads_after {
                    dead.insert(label.id);
                }
                // A surviving frame needs no recursion: everything inside it has this frame on the emission stack and emits the relaying spelling, never the clear.
            }
            Stmt::Loop { .. } => {
                // A loop emits no clear of its own (its `__br` reads are the wrapped head check and the post-loop relay).
                // Nothing under it is a candidate either: under a surviving loop the emission stack is non-empty, and under a dissolved one the back-edge re-runs earlier reads, so no recursion.
            }
            Stmt::If {
                label, then, els, ..
            } => {
                if label.referenced && !dissolved.contains(&label.id) {
                    if frames.crossed.contains(&label.id) && !reads_after {
                        dead.insert(label.id);
                    }
                } else {
                    // The arms emit inline (an unreferenced `if` is no frame, a dissolved one loses its scope), and only the taken arm runs, so each sees the reads after the `if` and not the other arm's.
                    collect_dead_clears(then, frames, dissolved, reads_after, dead);
                    collect_dead_clears(els, frames, dissolved, reads_after, dead);
                }
            }
            // Every other statement carries no frame, so `Stmt::child_seqs` yields nothing for it today.
            // A future variant that carries a body is skipped here, which can only keep a droppable clear, never drop a live one.
            _ => {}
        }
        reads_after = reads_after || emits_br_read(std::slice::from_ref(stmt), frames, dissolved);
    }
}

/// Whether emitting `stmts` produces any read of `__br`.
/// Every read sits at a surviving crossed frame (the land-or-relay epilogue, the wrapped-loop head check, the post-loop relay), so that is the whole test.
/// It over-approximates only for a crossed loop at method-body level, which emits no post-loop relay; branches write `__br` but never read it.
fn emits_br_read(stmts: &[Stmt], frames: &flat::Frames, dissolved: &HashSet<u32>) -> bool {
    Stmt::any(stmts, &mut |stmt| {
        let label = match stmt {
            Stmt::Block { label, .. }
            | Stmt::Loop { label, .. }
            | Stmt::If { label, .. }
            | Stmt::TryTable { label, .. } => label,
            _ => return false,
        };
        frames.crossed.contains(&label.id) && !dissolved.contains(&label.id)
    })
}

/// Widest `call_indirect` signature that gets a fixed-arity `Table#callN` dispatch method; wider signatures fall back to the splat `call`.
/// The `table/call0`..`table/call{MAX_FIXED_ARITY}` runtime units must exist.
const MAX_FIXED_ARITY: usize = 8;

pub use dewasm_backend::WASI_PREVIEW1_FUNCTIONS;

/// Loop-body extraction thresholds (see [`dewasm_backend::extract`]).
/// Ruby-specific values: YJIT/ZJIT compile a method only at a call, so a hot loop body large enough to amortize a ~12 ns call per iteration is worth extracting into one.
/// Tuned against the benchmark suite and the DOOM/NES examples; other backends would pick their own values.
/// `max_params` 34 is the smallest budget that admits the NES frame loop (30 parameters), whose extraction measures +3.4% under YJIT.
/// YJIT compiles high-arity methods without a cliff (~0.26 ns per extra parameter, no side exits up to 64), so the budget is bounded by the per-call marshalling cost, not by compilability; above 34 only sqlite3-shell's span set changes, with no measured gain.
const EXTRACT_PARAMS: extract::Params = extract::Params {
    min_weight: 40,
    max_params: 34,
    max_results: 1,
    min_weight_with_temps: 160,
};

/// Invariant constant-address load hoisting thresholds (see [`dewasm_backend::licm`]).
/// The guard is a few integer compares per store; two hoisted loads per iteration already outweigh it.
const LICM_PARAMS: licm::Params = licm::Params {
    min_hoisted_with_stores: 2,
};

/// The function list the emitter consumes: hoisting first (it needs the loops still in place), then loop-body extraction.
fn transformed_funcs(module: &Module) -> extract::Extracted {
    let mut funcs = module.funcs.clone();
    fuse::fuse_byte_scatter(&mut funcs);
    licm::hoist(
        &mut funcs,
        &module.types,
        licm::memory_min_bytes(module),
        &LICM_PARAMS,
    );
    extract::extract_funcs(
        funcs,
        module.types.clone(),
        module.imported_funcs.len() as u32,
        &EXTRACT_PARAMS,
    )
}

struct Gen<'a> {
    module: &'a Module,
    /// The function list actually emitted: the module's functions with extracted loop bodies replaced by calls, followed by the extracted functions, with `types` extended to match.
    extracted: extract::Extracted,
    default_wasi: bool,
    /// Runtime units the generated code references.
    uses: RefCell<BTreeSet<String>>,
    /// Per-function frame classification, set by `function()`: which frames carry a land-or-relay epilogue and which loops wrap their body.
    /// See [`flat::frames`].
    frames: RefCell<flat::Frames>,
    /// Emission-time stack of capturing frames currently open (label ids), pushed/popped around `Block`/`Loop`/referenced-`If` bodies.
    /// `branch()` compares its top against a `br`'s target; see the fast path there.
    frame_stack: RefCell<Vec<u32>>,
    /// Flat-dispatch plan for the function being emitted, when it has cross-frame branches (see [`flat`]).
    /// `branch()` consults it to emit `state = N; next` instead of the cascade.
    flat: RefCell<Option<flat::Plan>>,
    /// Per-function labels whose method-body-level clear is dead, set by `function()`; see [`dead_clears`].
    dead_clears: RefCell<HashSet<u32>>,
    /// Mask-elision dataflow for the function being emitted, set by `function()`: which locals and temps store unmasked, and the variable intervals the elision guard reads.
    elision: RefCell<Elision>,
    /// Global indices that need the `Rt::Global` box: imported globals (index space `0..imported_globals.len()`) and every `ExportKind:: Global` target.
    /// Computed once in `generate_class_inner`.
    /// See the boundary criterion in the comment there.
    boxed_globals: BTreeSet<u32>,
    /// When `Some`, data segments are externalized into a binary data file of this filename (referenced via `__dir__`) instead of embedded as hex literals; `data_offsets[i]` locates segment `i` in the blob.
    data_file: Option<String>,
    data_offsets: Vec<usize>,
}

impl<'a> Gen<'a> {
    /// The Ruby expression yielding a data segment's bytes: a slice of the externalized blob when `--data-file` is on, else an inline packed-hex literal.
    /// Both yield an ASCII-8BIT (binary) string.
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

    /// Whether `label_id`'s capturing frame is crossed by some `br` (see [`flat::frames`]), populated per-function by `function()`.
    fn is_crossed(&self, label_id: u32) -> bool {
        self.frames.borrow().crossed.contains(&label_id)
    }

    /// Whether `label_id`'s loop wraps its body in an inner scope because a `br` targets it from a strictly nested frame (see [`flat::frames`]).
    /// Such a loop wraps its body in an inner `begin ... end while false` (the `break` target) and takes its back-edge through `__br`; every other loop keeps the lean `while true` with a plain `next` back-edge.
    fn is_wrapped(&self, label_id: u32) -> bool {
        self.frames.borrow().wrapped.contains(&label_id)
    }

    /// Whether the frame currently on top of `frame_stack` has an enclosing capturing frame, i.e. a `break` emitted in its epilogue has a loop to bind to.
    /// A bare `break` at method-body scope is a Ruby SyntaxError, so the outermost frame omits the relay arm (a pending branch can never target something outside it, so that arm is also dead).
    fn has_enclosing_frame(&self) -> bool {
        self.frame_stack.borrow().len() > 1
    }

    /// Land-or-relay epilogue for a crossed `Block`/referenced-`If`, emitted *after* the frame's `end while false` (so a `break` out of the scope, from a nested relay or a direct exit, skips any intervening body code and reaches this decision): if the pending `__br` names this frame, clear it and fall through (the wasm branch lands past the block); otherwise a still-pending `__br` targets an ancestor, so `break` again to relay it outward.
    /// Emitted as a single line: epilogues sit at every crossed frame, often deeply indented, so each extra line costs its full indent in output bytes.
    ///
    /// With no enclosing frame the relay arm is dead (a pending `__br` can only name this frame; see [`dead_clears`]), so the epilogue degenerates to a clear, and where no later emission reads `__br` even that is omitted.
    fn emit_land_or_relay(&self, w: &mut CodeWriter, label_id: u32) {
        if self.has_enclosing_frame() {
            w.line(format!(
                "if __br == {label_id} then __br = nil elsif __br then break end"
            ));
        } else if !self.dead_clears.borrow().contains(&label_id) {
            w.line(format!("__br = nil if __br == {label_id}"));
        }
    }

    /// An access expression for global `idx`, whichever representation it has: `@g{idx}.value` if it's boxed (crosses an instantiation boundary), plain `@g{idx}` otherwise.
    /// Valid on both sides of `=`.
    fn global_ref(&self, idx: u32) -> String {
        if self.boxed_globals.contains(&idx) {
            format!("@g{idx}.value")
        } else {
            format!("@g{idx}")
        }
    }

    /// Reference a module-level runtime helper, recording its unit.
    /// The class includes `Rt`, so the helper is called by bare name.
    fn rt<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("rt/{name}"));
        name
    }

    /// Reference a Memory method, recording its unit.
    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
    }

    /// Resolve one import and validate its kind.
    /// One mechanism covers every import kind, not only functions: a present-but-wrong-kind value raises immediately (a link error), a missing one returns nil so the caller's `|| fallback` applies.
    fn resolve_import_string(&self, kind: &str, module: &str, name: &str) -> String {
        self.use_unit("rt/resolve_import");
        self.use_unit("rt/check_import_kind");
        format!(
            "check_import_kind(resolve_import(imports, {}, {}), :{kind}, {}, {})",
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
        // The memory is named at every load and store, so the ivar is short; the accessor keeps the name the provider protocol and embedders use.
        w.line("attr_reader :exports");
        w.line("");
        w.line("def memory = @m");
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
        w.block("def tag_export(name)", "end", |w| {
            w.line("instance_variable_get(TAG_EXPORTS.fetch(name))");
        });
        w.line("");
        // Provider protocol: an instance of a generated class is itself a valid value in another instance's `imports` table, exposing every export regardless of kind under its one (per-module) namespace, the mechanism the spec harness's `register` support (and any real cross-module linking) uses.
        w.block("def import(name)", "end", |w| {
            w.line("return @exports[name] if @exports.key?(name)");
            w.line("return global_export(name) if GLOBAL_EXPORTS.key?(name)");
            w.line("return table_export(name) if TABLE_EXPORTS.key?(name)");
            w.line("return tag_export(name) if TAG_EXPORTS.key?(name)");
            w.line("return @m if MEMORY_EXPORTS.include?(name)");
            w.line("nil");
        });
        w.line("");
        w.line("private");
        for (i, func) in self.extracted.funcs.iter().enumerate() {
            w.line("");
            let idx = self.module.num_imported_funcs() as usize + i;
            self.function(w, idx as u32, func);
        }
    }

    fn initialize(&self, w: &mut CodeWriter) {
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
        let tag_entries = tag_exports
            .iter()
            .map(|(name, idx)| format!("{} => :@tag{}", ruby_string(name), idx))
            .collect::<Vec<_>>()
            .join(", ");
        w.line(format!("TAG_EXPORTS = {{ {tag_entries} }}.freeze"));
        let memory_entries = memory_export_names
            .iter()
            .map(|name| ruby_string(name))
            .collect::<Vec<_>>()
            .join(", ");
        w.line(format!("MEMORY_EXPORTS = [{memory_entries}].freeze"));
        // Externalized data blob: read once at class-definition time, kept binary (ASCII-8BIT, as File.binread returns).
        if let Some(name) = &self.data_file {
            if !m.datas.is_empty() {
                w.line(format!(
                    "DATA_BLOB = File.binread(File.join(__dir__, {})).freeze",
                    ruby_string(name)
                ));
            }
        }
        w.line("");

        let wasi_fallback = wasi_bundled(m, self.default_wasi, bundler());
        let header = if wasi_fallback {
            "def initialize(imports = {}, args: [], env: {}, preopens: {})"
        } else {
            "def initialize(imports = {})"
        };
        w.block(header, "end", |w| {
            if let Some(import) = &m.imported_memory {
                w.line(format!(
                    "@m = {} || {}",
                    self.resolve_import_string("memory", &import.module, &import.name),
                    self.missing_import_string(&import.module, &import.name),
                ));
            } else if let Some(mem) = &m.memory {
                self.use_unit("memory/_class");
                let max = mem
                    .max_pages
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "nil".to_string());
                w.line(format!("@m = Rt::Memory.new({}, {})", mem.min_pages, max));
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
                        "->(*) { 52 }".to_string() // ENOSYS: not implemented yet
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
                let init = self.expr_text(&global.init);
                if self.boxed_globals.contains(&idx) {
                    self.use_unit("global/_class");
                    w.line(format!("@g{idx} = Rt::Global.new({init})"));
                } else {
                    w.line(format!("@g{idx} = {init}"));
                }
            }
            for (i, import) in m.imported_tags.iter().enumerate() {
                w.line(format!(
                    "@tag{i} = {} || {}",
                    self.resolve_import_string("tag", &import.module, &import.name),
                    self.missing_import_string(&import.module, &import.name),
                ));
            }
            for i in 0..m.tags.len() {
                self.use_unit("rt/tag");
                w.line(format!("@tag{} = Rt::Tag.new", m.imported_tags.len() + i));
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
                        let offset = self.expr_text(offset);
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
                            "@m.init({}, {}, 0, {})",
                            self.expr_text(offset),
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
        self.type_symbol_of(self.module.func_type(func_idx))
    }

    fn type_symbol(&self, type_idx: u32) -> String {
        self.type_symbol_of(&self.module.types[type_idx as usize])
    }

    /// A structural type key (see [`type_key`]) as an interned Ruby symbol, which is what the table stores and `call_indirect` compares.
    fn type_symbol_of(&self, ty: &dewasm_core::ir::FuncType) -> String {
        format!(":\"{}\"", type_key(ty, val_name))
    }

    fn func_ref(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("@if{func_idx}")
        } else {
            format!("method(:_f{func_idx})")
        }
    }

    /// A funcref value: the `[type_symbol, callable]` pair tables store.
    /// Element items and `call_indirect` agree on this shape.
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
        *self.frames.borrow_mut() = flat::frames(&func.body, flat::BreakToBlockEnd::Available);
        self.frame_stack.borrow_mut().clear();
        let ty = &self.extracted.types[func.type_idx as usize];
        *self.elision.borrow_mut() = Elision::analyze(&ty.params, func, FIXNUM_LIMIT);
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
            // The chained assignment is sound only because the defaults are immutable literals: every name ends up bound to its own value, not to one shared object.
            for run in local_runs(&func.locals, default_value) {
                let default = default_value(func.locals[run.start]);
                let names = run
                    .map(|k| format!("l{}", ty.params.len() + k))
                    .collect::<Vec<_>>()
                    .join(" = ");
                w.line(format!("{names} = {default}"));
            }
            let plan = flat::plan(&func.body, &self.frames.borrow().paths, flat::DEEP_CROSSING);
            *self.dead_clears.borrow_mut() =
                dead_clears(&func.body, &self.frames.borrow(), plan.as_ref());
            // Hoist all temps to method scope: assignments inside the `begin`/`while` frames would otherwise be block-local in Ruby.
            // The pending-branch variable `__br` is hoisted alongside them only when the cascade can actually use it: a crossed frame that survives the plan still relays through `__br`, one addressed by state never does, and with no crossed frame at all nothing references it either.
            let mut depths: Vec<u32> = func.temps.iter().map(|t| t.depth).collect();
            depths.dedup();
            let mut decl = String::new();
            let relays = self.frames.borrow().crossed.iter().any(|id| {
                plan.as_ref()
                    .is_none_or(|p: &flat::Plan| !p.dissolved.contains(id))
            });
            if relays {
                decl.push_str("__br = ");
            }
            for d in &depths {
                decl.push_str(&format!("s{d} = "));
            }
            if !decl.is_empty() {
                w.line(format!("{decl}nil"));
            }
            match plan {
                None => {
                    *self.flat.borrow_mut() = None;
                    self.stmts(w, &func.body);
                }
                Some(plan) => {
                    let n = plan.nstates as usize;
                    *self.flat.borrow_mut() = Some(plan);
                    let mut st: Vec<CodeWriter> = (0..n).map(|_| CodeWriter::new("\t")).collect();
                    let last = self.flat_seq(&mut st, 0, &func.body);
                    // Falling off the body ends the function; leave the dispatch loop.
                    // A body that cannot fall off needs no such exit.
                    if !terminates(&func.body) {
                        st[last].line(format!("state = {n}; next"));
                    }
                    let texts: Vec<String> = st.into_iter().map(|c| c.finish()).collect();
                    w.line("state = 0");
                    w.block("while true", "end", |w| {
                        w.line("case state");
                        for (i, body) in texts.iter().enumerate() {
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
        *self.elision.borrow_mut() = Elision::none(FIXNUM_LIMIT);
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
                    if !terminates(body) {
                        st[cur].line(format!("state = {after}; next"));
                    }
                    cur = after as usize;
                }
                Stmt::Loop { body, .. } => {
                    // `target` is the head: entering the loop and taking its back-edge are the same transition.
                    st[cur].line(format!("state = {target}; next"));
                    cur = target as usize;
                    cur = self.flat_seq(st, cur, body);
                    if !terminates(body) {
                        st[cur].line(format!("state = {after}; next"));
                    }
                    cur = after as usize;
                }
                Stmt::If {
                    cond, then, els, ..
                } => {
                    // The arms stay inline: `if` is not a Ruby loop, so a `next`
                    // inside one already reaches the dispatch loop.
                    st[cur].line(format!("if {}", self.cond(cond).free()));
                    st[cur].indent();
                    let a = self.flat_seq(st, cur, then);
                    if !terminates(then) {
                        st[a].line(format!("state = {after}; next"));
                    }
                    st[cur].dedent();
                    if !els.is_empty() {
                        st[cur].line("else");
                        st[cur].indent();
                        let b = self.flat_seq(st, cur, els);
                        if !terminates(els) {
                            st[b].line(format!("state = {after}; next"));
                        }
                        st[cur].dedent();
                    }
                    st[cur].line("end");
                    // Reachable only through the condition-false fallthrough.
                    // With an `else` present both arms route themselves (a transition or a terminator), so nothing falls out of the `if` and a trailing transition would be dead text.
                    if els.is_empty() {
                        st[cur].line(format!("state = {after}; next"));
                    }
                    cur = after as usize;
                }
                _ => unreachable!("only frames are dissolved"),
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
                let unmasked = self.elision.borrow().unmasked_temp(*dst);
                w.line(format!(
                    "{} = {}",
                    temp(*dst),
                    self.store_text(unmasked, expr)
                ));
            }
            Stmt::LocalSet { idx, expr } => {
                let unmasked = self.elision.borrow().unmasked_local(*idx);
                w.line(format!("l{idx} = {}", self.store_text(unmasked, expr)));
            }
            Stmt::GlobalSet { idx, expr } => {
                w.line(format!(
                    "{} = {}",
                    self.global_ref(*idx),
                    self.expr_text(expr)
                ));
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                let (method, addr_args) = self.mem_call(store_code(*op), addr, *offset);
                w.line(format!(
                    "@m.{method}({addr_args}, {})",
                    self.modular(value).free()
                ));
            }
            Stmt::Block { label, body } => {
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
                    // A `br` targets this loop from a nested frame, arriving with `__br` set by `break`ing out of the inner `begin` (skipping any code left in the body).
                    // The decision then re-enters via `next`, or `break`s the `while` so the post-loop relay can pass `__br` further out.
                    // A plain fallthrough leaves `__br` nil and exits the loop.
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
                    w.line(format!("if {}", gen.cond(cond).free()));
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
                w.block(format!("if {}", self.cond(cond).free()), "end", |w| {
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
                w.line(format!("case {}", self.expr_text(index)));
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
                let args: Vec<String> = args.iter().map(|a| self.expr_text(a)).collect();
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
                // Fixed-arity dispatch: a per-arity `callN` avoids building a `*args` array on either side; the splat `call` stays as the fallback for signatures wider than MAX_FIXED_ARITY (unobserved in the real-world apps, whose call_indirect arities top out at 8).
                let mut call_args = vec![self.expr_text(index), self.type_symbol(*type_idx)];
                call_args.extend(args.iter().map(|a| self.expr_text(a)));
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
                    "{} = @m.grow({})",
                    temp(*dst),
                    self.expr_text(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "@m.copy({}, {}, {})",
                    self.expr_text(dst),
                    self.expr_text(src),
                    self.expr_text(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "@m.fill({}, {}, {})",
                    self.expr_text(dst),
                    self.expr_text(val),
                    self.expr_text(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "@m.init({}, @data{seg}, {}, {})",
                    self.expr_text(dst),
                    self.expr_text(src),
                    self.expr_text(len)
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
                    self.expr_text(dst),
                    self.expr_text(src),
                    self.expr_text(len)
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
                    self.expr_text(dst),
                    self.expr_text(src),
                    self.expr_text(len)
                ));
            }
            Stmt::ElemDrop { seg } => {
                w.line(format!("@elem{seg} = []"));
            }
            Stmt::TryTable {
                label,
                catches,
                body,
            } => {
                self.use_unit("rt/wasm_exception");
                let crossed = self.is_crossed(label.id);
                self.frame_stack.borrow_mut().push(label.id);
                // The same `begin ... end while false` frame a `Stmt::Block` gets, so a branch out of it is the same `break`; only the handler is added.
                w.line("begin");
                w.indent();
                if body.is_empty() {
                    w.line("nil");
                } else {
                    self.stmts(w, body);
                }
                w.dedent();
                w.line("rescue Rt::WasmException => __e");
                w.indent();
                for clause in catches {
                    self.catch_clause(w, clause);
                }
                // No clause matched: the exception keeps unwinding.
                w.line("raise");
                w.dedent();
                w.line("end while false");
                if crossed {
                    self.emit_land_or_relay(w, label.id);
                }
                self.frame_stack.borrow_mut().pop();
            }
            Stmt::Throw { tag, args } => {
                self.use_unit("rt/wasm_exception");
                let args: Vec<String> = args.iter().map(|a| self.expr_text(a)).collect();
                w.line(format!(
                    "raise Rt::WasmException.new(@tag{tag}, [{}])",
                    args.join(", ")
                ));
            }
            Stmt::ThrowRef { exn } => {
                w.line(format!("{}({})", self.rt("throw_ref"), self.expr_text(exn)));
            }
            Stmt::Unreachable => {
                w.line(format!("{}(\"unreachable\")", self.rt("trap")));
            }
            Stmt::SourceLine(pos) => {
                let file = &self.module.debug_files[pos.file as usize];
                w.line(format!("# {file}:{}", pos.line));
            }
        }
    }

    /// One `try_table` catch clause inside the handler: bind the payload into the target frame's slots, then take the branch.
    /// A tagged clause guards that with `equal?`, wasm tag equality being object identity; a catch-all runs unconditionally, so any clause after it is dead.
    fn catch_clause(&self, w: &mut CodeWriter, clause: &CatchClause) {
        let bind_and_branch = |w: &mut CodeWriter| {
            for (i, t) in clause.value_temps.iter().enumerate() {
                if Some(*t) == clause.exn_temp {
                    w.line(format!("{} = __e", temp(*t)));
                } else {
                    w.line(format!("{} = __e.values[{i}]", temp(*t)));
                }
            }
            self.branch(w, &clause.target);
        };
        match clause.tag {
            Some(tag) => w.block(
                format!("if __e.tag.equal?(@tag{tag})"),
                "end",
                bind_and_branch,
            ),
            None => bind_and_branch(w),
        }
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        match values {
            [] => w.line("return"),
            [v] => w.line(format!("return {}", self.expr_text(v))),
            vs => {
                let vs = vs
                    .iter()
                    .map(|v| self.expr_text(v))
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
                // `break_ok` (see [`flat::Frames`]): leaving the innermost loop lands at the end of the block enclosing it, which is where this branch is going.
                // Neither frame dissolved, so the plain `break` is still available and still O(1).
                {
                    let fs = self.frame_stack.borrow();
                    if fs.len() >= 2
                        && fs[fs.len() - 2] == *label
                        && self.frames.borrow().break_ok.contains(&fs[fs.len() - 1])
                    {
                        w.line("break");
                        return;
                    }
                }
                // Fast path (a br whose target is the innermost enclosing frame leaves it directly): `break` out of a block/if's `begin...end while false`, or `next` to take an unwrapped loop's back-edge.
                // Everything else (a br to an outer frame, or a back-edge into a *wrapped* loop, whose body sits in an inner `begin`) sets the pending label variable and `break`s out of the innermost scope; each crossed frame's epilogue then relays `__br` outward until the target lands.
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

    /// The method name and address arguments for a load/store site.
    /// A nonzero static offset rides as a second argument to the `o`-suffixed unit, so the per-site addition and its call data disappear from the caller; an offset-zero site keeps the one-argument unit, whose call must not pay for an argument it never passes.
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
                    let args = format!("{}, {}", self.modular(x).free(), self.modular(y).free());
                    return (method, args);
                }
            }
            return (self.mem(method).to_string(), self.modular(addr).free());
        }
        if let Expr::I32Const(base) = addr {
            let sum = u64::from(*base) + offset;
            if sum <= u64::from(u32::MAX) {
                return (self.mem(method).to_string(), sum.to_string());
            }
        }
        let method = format!("{method}o");
        self.use_unit(&format!("memory/{method}"));
        let args = format!("{}, {offset}", self.modular(addr).free());
        (method, args)
    }

    /// An expression in a position that constrains nothing, ready to be pasted into a statement.
    fn expr_text(&self, expr: &Expr) -> String {
        self.masked(expr).free()
    }

    /// The right-hand side of a local or temp store: rendered modular when the dataflow proved every read of the destination modular, so the store needs no mask of its own.
    fn store_text(&self, unmasked: bool, expr: &Expr) -> String {
        let ctx = if unmasked {
            MaskContext::Modular
        } else {
            MaskContext::Masked
        };
        self.expr(expr, ctx).free()
    }

    /// An expression whose exact stored value is observed (a local or temp store the dataflow did not clear, an argument, a comparison operand): a result mask stays unless it is provably the identity on the exact value.
    fn masked(&self, expr: &Expr) -> Rendered {
        self.expr(expr, MaskContext::Masked)
    }

    /// An expression a memory unit consumes: the unit reduces its address and stored-value arguments itself, so a congruent value suffices and the site's own mask may go.
    fn modular(&self, expr: &Expr) -> Rendered {
        self.expr(expr, MaskContext::Modular)
    }

    /// Whether `e`'s own result mask may be skipped when its consumer reads it in `ctx`, per the shared guard with the current function's variable intervals supplied (see [`FIXNUM_LIMIT`]).
    fn elide(&self, ctx: MaskContext, e: &Expr) -> bool {
        self.elision.borrow().elides_mask(e, ctx)
    }

    /// `ctx` is the consumer's view of the value: under a `Modular` consumer a site's own result mask is skipped when the shared bound guard allows it (see [`FIXNUM_LIMIT`]).
    fn expr(&self, expr: &Expr, ctx: MaskContext) -> Rendered {
        match expr {
            Expr::I32Const(v) => number(v.to_string()),
            Expr::I64Const(v) => number(v.to_string()),
            Expr::F32Const(bits) => {
                let v = f32::from_bits(*bits);
                if v.is_finite() {
                    number(format!("{:?}", v as f64))
                } else {
                    self.call1("f32_from_bits", Rendered::atom(format!("0x{bits:x}")))
                }
            }
            Expr::F64Const(bits) => {
                let v = f64::from_bits(*bits);
                if v.is_finite() {
                    number(format!("{v:?}"))
                } else {
                    self.call1("f64_from_bits", Rendered::atom(format!("0x{bits:x}")))
                }
            }
            Expr::Temp(t) => Rendered::atom(temp(*t)),
            Expr::LocalGet(idx) => Rendered::atom(format!("l{idx}")),
            Expr::GlobalGet(idx) => Rendered::atom(self.global_ref(*idx)),
            // `eqz` of something already emitted as a Ruby boolean: read the wasm 0/1 straight off that boolean instead of materializing the operand's own 0/1 first and testing it.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) if is_boolean(a) => ternary(
                self.cond(a),
                Rendered::atom("0".to_string()),
                Rendered::atom("1".to_string()),
            ),
            Expr::Un(op, a) => {
                let a = self.expr(a, un_operand_context(*op));
                self.un(*op, a, self.elide(ctx, expr))
            }
            Expr::Bin(op, a, b) => {
                if matches!(op, BinOp::I32And | BinOp::I64And) {
                    if let Some((e, c)) = fold_and_chain(a, b) {
                        let re = self.expr(e, MaskContext::Reducing);
                        return infix(re, "&", number(c.to_string()), BIT_AND);
                    }
                }
                if let Some((ra, rb)) = self.eq_rewrite_operands(*op, a, b) {
                    return self.bin(*op, ra, rb, false);
                }
                let ra = self.expr(a, bin_operand_context(*op, 0, b, ctx));
                let rb = match shift_width(*op) {
                    Some(bits) => self.shift_count(b, bits),
                    None => self.expr(b, bin_operand_context(*op, 1, a, ctx)),
                };
                self.bin(*op, ra, rb, self.elide(ctx, expr))
            }
            Expr::Load { op, addr, offset } => {
                let (method, addr_args) = self.mem_call(load_code(*op), addr, *offset);
                Rendered::atom(format!("@m.{method}({addr_args})"))
            }
            Expr::Select { cond, then, els } => {
                ternary(self.cond(cond), self.masked(then), self.masked(els))
            }
            Expr::MemorySize => {
                self.use_unit("memory/size");
                Rendered::atom("@m.size".to_string())
            }
        }
    }

    /// A shift count, reduced modulo the width as wasm requires ([`shift_count_mode`]): a constant folds at conversion time, a provably in-range count is emitted bare from its `Masked` rendering, anything else under `& (bits - 1)`.
    fn shift_count(&self, b: &Expr, bits: u32) -> Rendered {
        match shift_count_mode(b, bits, FIXNUM_LIMIT) {
            ShiftCountMode::Constant(c) => Rendered::atom(c.to_string()),
            ShiftCountMode::InRange => self.expr(b, MaskContext::Masked),
            ShiftCountMode::Masked => infix(
                self.expr(b, MaskContext::Modular),
                "&",
                Rendered::atom((bits - 1).to_string()),
                BIT_AND,
            ),
        }
    }

    /// An expression in boolean context (an `if`/`br_if`/`select` test).
    ///
    /// A wasm comparison yields the i32 0 or 1, and every conditional context then compares that against 0, so the lowering built a ternary only to undo it one operation later.
    /// Emitting the comparison as a Ruby boolean drops both the ternary and the test; the operands are untouched, so a signed view still goes through `s32`/`s64`.
    /// Anything else keeps the `!= 0` test.
    fn cond(&self, e: &Expr) -> Rendered {
        match e {
            // `eqz` in boolean context is the negation of its operand's own test.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => self.not_cond(a),
            Expr::Bin(op, a, b) => match signed_view_rel_op(*op) {
                Some(rel) => {
                    let (ra, rb) = self
                        .eq_rewrite_operands(*op, a, b)
                        .unwrap_or_else(|| (self.masked(a), self.masked(b)));
                    self.rel(rel, ra, rb)
                }
                None => self.zero_test("!=", e),
            },
            _ => self.zero_test("!=", e),
        }
    }

    /// The negation of [`cond`]: `e` is zero.
    /// A comparison is negated as a whole rather than by flipping its operator, which would be wrong for floats (both `x < y` and `x >= y` are false when either is NaN).
    fn not_cond(&self, e: &Expr) -> Rendered {
        match e {
            // Two negations cancel.
            Expr::Un(UnOp::I32Eqz | UnOp::I64Eqz, a) => self.cond(a),
            Expr::Bin(op, ..) if signed_view_rel_op(*op).is_some() => not(self.cond(e)),
            _ => self.zero_test("==", e),
        }
    }

    /// `e == 0` / `e != 0`: the fallback test for a value that is not already a Ruby boolean.
    /// A mask site whose raw interval pins a unique preimage of 0 compares unmasked against it (see [`Elision::eq_const_rewrite`]; the per-function analysis both pins and renders, so the two agree on which masks drop).
    fn zero_test(&self, op: &'static str, e: &Expr) -> Rendered {
        let rewrite = self.elision.borrow().eq_const_rewrite(e, 0);
        let (lhs, rhs) = match rewrite {
            Some((x, ctx, t)) => (self.expr(x, ctx), number(t.to_string())),
            None => (self.masked(e), Rendered::atom("0".to_string())),
        };
        compare(lhs, op, rhs, EQ)
    }

    /// The rendered operands of an integer equality whose one side is a constant, when the shared analysis pins the other side's mask to a unique raw preimage (see [`Elision::eq_const_rewrite`]); `None` renders both sides exact.
    fn eq_rewrite_operands(&self, op: BinOp, a: &Expr, b: &Expr) -> Option<(Rendered, Rendered)> {
        use BinOp::*;
        if !matches!(op, I32Eq | I32Ne | I64Eq | I64Ne) {
            return None;
        }
        let (e, c) = match (a, b) {
            (Expr::I32Const(c), e) | (e, Expr::I32Const(c)) => (e, u64::from(*c)),
            (Expr::I64Const(c), e) | (e, Expr::I64Const(c)) => (e, *c),
            _ => return None,
        };
        let (x, ctx, t) = self.elision.borrow().eq_const_rewrite(e, c)?;
        Some((self.expr(x, ctx), number(t.to_string())))
    }

    /// A one-argument call to a runtime helper, recording its unit.
    fn call1(&self, name: &str, a: Rendered) -> Rendered {
        Rendered::atom(format!("{}({})", self.rt(name), a.free()))
    }

    fn call2(&self, name: &str, a: Rendered, b: Rendered) -> Rendered {
        Rendered::atom(format!("{}({}, {})", self.rt(name), a.free(), b.free()))
    }

    /// The i64 result mask (`Rt.m64`), skipped when the consumer restores it; an elided site also keeps the `rt/m64` unit out of the bundle.
    fn m64_unless(&self, elide: bool, a: Rendered) -> Rendered {
        if elide {
            a
        } else {
            self.call1("m64", a)
        }
    }

    fn un(&self, op: UnOp, a: Rendered, elide_mask: bool) -> Rendered {
        use UnOp::*;
        match op {
            I32Eqz | I64Eqz => ternary(
                compare(a, "==", Rendered::atom("0".to_string()), EQ),
                Rendered::atom("1".to_string()),
                Rendered::atom("0".to_string()),
            ),
            I32Clz => self.call1("i32_clz", a),
            I32Ctz => self.call1("i32_ctz", a),
            I64Clz => self.call1("i64_clz", a),
            I64Ctz => self.call1("i64_ctz", a),
            I32Popcnt | I64Popcnt => self.call1("popcnt", a),
            F32Abs => self.call1("f32_abs", a),
            F32Neg => self.call1("f32_neg", a),
            F64Abs => self.call1("f64_abs", a),
            F64Neg => self.call1("f64_neg", a),
            F32Ceil | F64Ceil => self.call1("fceil", a),
            F32Floor | F64Floor => self.call1("ffloor", a),
            F32Trunc | F64Trunc => self.call1("ftrunc", a),
            F32Nearest | F64Nearest => self.call1("fnearest", a),
            F32Sqrt => {
                let r = self.call1("fsqrt", a);
                self.call1("f32", r)
            }
            F64Sqrt => self.call1("fsqrt", a),
            I32WrapI64 => mask32_unless(elide_mask, a),
            I32TruncF32S | I32TruncF64S => self.call1("i32_trunc_s", a),
            I32TruncF32U | I32TruncF64U => self.call1("i32_trunc_u", a),
            I64TruncF32S | I64TruncF64S => self.call1("i64_trunc_s", a),
            I64TruncF32U | I64TruncF64U => self.call1("i64_trunc_u", a),
            I32TruncSatF32S | I32TruncSatF64S => self.call1("i32_trunc_sat_s", a),
            I32TruncSatF32U | I32TruncSatF64U => self.call1("i32_trunc_sat_u", a),
            I64TruncSatF32S | I64TruncSatF64S => self.call1("i64_trunc_sat_s", a),
            I64TruncSatF32U | I64TruncSatF64U => self.call1("i64_trunc_sat_u", a),
            I64ExtendI32S => self.call1("i64_extend_i32_s", a),
            I64ExtendI32U => a,
            F32ConvertI32S => {
                let s = self.call1("s32", a);
                self.call1("f32", to_f(s))
            }
            F32ConvertI32U => self.call1("f32", to_f(a)),
            F32ConvertI64S => {
                let s = self.call1("s64", a);
                self.call1("cvt_f32_i", s)
            }
            F32ConvertI64U => self.call1("cvt_f32_i", a),
            F64ConvertI32S => to_f(self.call1("s32", a)),
            F64ConvertI32U => to_f(a),
            F64ConvertI64S => {
                let s = self.call1("s64", a);
                self.call1("cvt_f64_i", s)
            }
            F64ConvertI64U => self.call1("cvt_f64_i", a),
            F32DemoteF64 => self.call1("f32_demote", a),
            F64PromoteF32 => self.call1("f64_promote", a),
            I32ReinterpretF32 => self.call1("i32_reinterpret_f32", a),
            I64ReinterpretF64 => self.call1("i64_reinterpret_f64", a),
            F32ReinterpretI32 => self.call1("f32_reinterpret_i32", a),
            F64ReinterpretI64 => self.call1("f64_reinterpret_i64", a),
            I32Extend8S => self.call1("i32_extend8_s", a),
            I32Extend16S => self.call1("i32_extend16_s", a),
            I64Extend8S => self.call1("i64_extend8_s", a),
            I64Extend16S => self.call1("i64_extend16_s", a),
            I64Extend32S => self.call1("i64_extend32_s", a),
        }
    }

    fn bin(&self, op: BinOp, a: Rendered, b: Rendered, elide_mask: bool) -> Rendered {
        use BinOp::*;
        // A comparison is a Ruby boolean; outside condition position it needs the ternary back to the i32 0 or 1 wasm expects (see `cond`).
        if let Some(rel) = signed_view_rel_op(op) {
            return ternary(
                self.rel(rel, a, b),
                Rendered::atom("1".to_string()),
                Rendered::atom("0".to_string()),
            );
        }
        match op {
            I32Add => mask32_unless(elide_mask, infix(a, "+", b, ADD)),
            I32Sub => mask32_unless(elide_mask, infix(a, "-", b, ADD)),
            I32Mul => mask32_unless(elide_mask, infix(a, "*", b, MUL)),
            I64Add => self.m64_unless(elide_mask, infix(a, "+", b, ADD)),
            I64Sub => self.m64_unless(elide_mask, infix(a, "-", b, ADD)),
            I64Mul => self.m64_unless(elide_mask, infix(a, "*", b, MUL)),
            I32DivS => self.call2("i32_div_s", a, b),
            I32DivU => self.call2("i32_div_u", a, b),
            I32RemS => self.call2("i32_rem_s", a, b),
            I32RemU => self.call2("i32_rem_u", a, b),
            I64DivS => self.call2("i64_div_s", a, b),
            I64DivU => self.call2("i64_div_u", a, b),
            I64RemS => self.call2("i64_rem_s", a, b),
            I64RemU => self.call2("i64_rem_u", a, b),
            I32And | I64And => infix(a, "&", b, BIT_AND),
            I32Or | I64Or => infix(a, "|", b, BIT_OR),
            I32Xor | I64Xor => infix(a, "^", b, BIT_OR),
            // A shift's `b` arrives from `shift_count`, already reduced modulo the width.
            I32Shl => mask32_unless(elide_mask, infix(a, "<<", b, SHIFT)),
            I32ShrU => infix(a, ">>", b, SHIFT),
            I32ShrS => mask32_unless(elide_mask, infix(self.call1("s32", a), ">>", b, SHIFT)),
            I64Shl => self.m64_unless(elide_mask, infix(a, "<<", b, SHIFT)),
            I64ShrU => infix(a, ">>", b, SHIFT),
            I64ShrS => {
                let s = self.call1("s64", a);
                self.m64_unless(elide_mask, infix(s, ">>", b, SHIFT))
            }
            I32Rotl => self.call2("i32_rotl", a, b),
            I32Rotr => self.call2("i32_rotr", a, b),
            I64Rotl => self.call2("i64_rotl", a, b),
            I64Rotr => self.call2("i64_rotr", a, b),
            F32Add => self.call1("f32", infix(a, "+", b, ADD)),
            F32Sub => self.call1("f32", infix(a, "-", b, ADD)),
            F32Mul => self.call1("f32", infix(a, "*", b, MUL)),
            F32Div => self.call1("f32", infix(a, "/", b, MUL)),
            F64Add => infix(a, "+", b, ADD),
            // GCC-built MRI (any arch; every version probed) leaves a signaling NaN unquieted when the RHS is +0.0: the flonum decode returns +0.0 as a literal, and GCC folds `a - 0.0` to `a`, skipping the FPU sub.
            // The host `-` therefore cannot be trusted to return an arithmetic NaN.
            // Quiet any NaN result explicitly; the `== r` self-compare is false only for NaN, keeping the common finite path allocation- and call-free.
            // See issue #11.
            F64Sub => {
                let r = Rendered::atom("r".to_string());
                // The assignment's parens are required: `=` binds looser than everything around it.
                let store = Rendered::atom(format!("(r = {})", infix(a, "-", b, ADD).free()));
                ternary(
                    compare(store, "==", r.clone(), EQ),
                    r.clone(),
                    self.call1("quiet_nan", r),
                )
            }
            F64Mul => infix(a, "*", b, MUL),
            F64Div => infix(a, "/", b, MUL),
            F32Min | F64Min => self.call2("fmin", a, b),
            F32Max | F64Max => self.call2("fmax", a, b),
            F32Copysign => self.call2("f32_copysign", a, b),
            F64Copysign => self.call2("f64_copysign", a, b),
            _ => unreachable!("op {op:?} is a comparison, rendered by `rel`"),
        }
    }

    /// A comparison as a Ruby boolean, from a [`signed_view_rel_op`] mapping and the already rendered operands.
    fn rel(&self, (r, sign): (&'static str, Option<&str>), a: Rendered, b: Rendered) -> Rendered {
        let prec = if r == "==" || r == "!=" { EQ } else { CMP };
        match sign {
            None => compare(a, r, b, prec),
            Some(sign) => {
                let f = self.rt(sign);
                compare(
                    Rendered::atom(format!("{f}({})", a.free())),
                    r,
                    Rendered::atom(format!("{f}({})", b.free())),
                    prec,
                )
            }
        }
    }
}

/// Ruby's operator precedence over the subset the generated code emits, tightest first.
/// Only the levels an emitted expression can sit at are named; `**`, `&&`, `||` and the rest are never built here.
///
/// Two of these differ from C and are the reason the table is written out rather than assumed: `&` binds *tighter* than `|` and `^`, and all three bind tighter than the comparisons.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    /// A literal, variable, ivar, or anything ending in a call's closing paren.
    Atom,
    /// `!x`, and a negative numeric literal.
    Unary,
    Mul,
    Add,
    Shift,
    BitAnd,
    BitOr,
    Cmp,
    Eq,
    /// `c ? t : e`, the loosest thing built here, and so also the limit of a position that constrains nothing.
    Ternary,
}

use Prec::{Add as ADD, BitAnd as BIT_AND, BitOr as BIT_OR, Cmp as CMP, Eq as EQ, Mul as MUL};
use Prec::{Shift as SHIFT, Ternary as FREE};

impl Prec {
    /// The next tighter level: the limit for an operand that must not sit at this one unparenthesized.
    fn tighter(self) -> Prec {
        match self {
            Prec::Atom | Prec::Unary => Prec::Atom,
            Prec::Mul => Prec::Unary,
            Prec::Add => Prec::Mul,
            Prec::Shift => Prec::Add,
            Prec::BitAnd => Prec::Shift,
            Prec::BitOr => Prec::BitAnd,
            Prec::Cmp => Prec::BitOr,
            Prec::Eq => Prec::Cmp,
            Prec::Ternary => Prec::Eq,
        }
    }
}

/// A rendered Ruby expression together with how tightly it binds, so that each operand is parenthesized only where its context would otherwise reparse it.
#[derive(Clone)]
struct Rendered {
    src: String,
    prec: Prec,
    /// The top-level operator, for the one place equal precedence is not enough to decide: an associative operator may take an equal-precedence right operand only if it is the same operator.
    /// Empty for everything else.
    op: &'static str,
}

impl Rendered {
    /// Something that binds as tightly as a name: a literal, a variable, or a call.
    fn atom(src: String) -> Rendered {
        Rendered {
            src,
            prec: Prec::Atom,
            op: "",
        }
    }

    /// The rendering as an operand of a context that accepts anything up to `limit`, parenthesized if it binds looser.
    fn at(self, limit: Prec) -> String {
        if self.prec <= limit {
            self.src
        } else {
            format!("({})", self.src)
        }
    }

    /// The rendering in a position that constrains nothing: a statement's right-hand side, an `if`/`case` subject, a call argument, an array element.
    fn free(self) -> String {
        self.at(FREE)
    }
}

/// A numeric literal.
/// A negative one carries a unary minus, so it binds like a unary expression and is parenthesized where an atom is required.
fn number(src: String) -> Rendered {
    let prec = if src.starts_with('-') {
        Prec::Unary
    } else {
        Prec::Atom
    };
    Rendered { src, prec, op: "" }
}

/// `a OP b` for a left-associative operator.
/// An equal-precedence *left* operand reparses the way it was built, so it needs no parens; an equal-precedence right operand does not, and is kept parenthesized unless the operator is the same bitwise one: those are associative over the integers that are their only operands.
fn infix(a: Rendered, op: &'static str, b: Rendered, prec: Prec) -> Rendered {
    let associative = matches!(op, "&" | "|" | "^") && b.op == op;
    let right_limit = if associative { prec } else { prec.tighter() };
    Rendered {
        src: format!("{} {op} {}", a.at(prec), b.at(right_limit)),
        prec,
        op,
    }
}

/// `a OP b` for the comparison and equality families, which Ruby parses as non-associative (`a == b == c` is a syntax error): neither operand may sit at the operator's own level.
fn compare(a: Rendered, op: &'static str, b: Rendered, prec: Prec) -> Rendered {
    Rendered {
        src: format!("{} {op} {}", a.at(prec.tighter()), b.at(prec.tighter())),
        prec,
        op: "",
    }
}

/// `c ? t : e`.
/// The ternary is right-associative, so only the else-branch may hold another one unparenthesized.
fn ternary(c: Rendered, t: Rendered, e: Rendered) -> Rendered {
    Rendered {
        src: format!(
            "{} ? {} : {}",
            c.at(FREE.tighter()),
            t.at(FREE.tighter()),
            e.at(FREE)
        ),
        prec: Prec::Ternary,
        op: "",
    }
}

fn not(a: Rendered) -> Rendered {
    Rendered {
        src: format!("!{}", a.at(Prec::Unary)),
        prec: Prec::Unary,
        op: "",
    }
}

fn mask32(a: Rendered) -> Rendered {
    infix(a, "&", Rendered::atom("0xffffffff".to_string()), BIT_AND)
}

/// The i32 result mask, skipped when the consumer restores it.
fn mask32_unless(elide: bool, a: Rendered) -> Rendered {
    if elide {
        a
    } else {
        mask32(a)
    }
}

/// 64-bit MRI keeps integers in `-2**62 .. 2**62 - 1` unboxed; a skipped mask must never expose an intermediate outside that range, or the elision would trade a cheap mask for bignum arithmetic.
const FIXNUM_LIMIT: i128 = 1 << 62;

/// A receiver binds as tightly as a call, so anything looser is parenthesized.
fn to_f(a: Rendered) -> Rendered {
    Rendered::atom(format!("{}.to_f", a.at(Prec::Atom)))
}

fn temp(t: Temp) -> String {
    format!("s{}", t.depth)
}

fn default_value(ty: ValType) -> &'static str {
    dewasm_backend::default_value(ty, "nil")
}

/// How a value type is spelled inside a structural type key ([`type_key`]): the shared wasm spelling.
fn val_name(ty: ValType) -> &'static str {
    dewasm_backend::val_name(ty)
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

/// Lint for the runtime units: every reference a unit body makes to another unit must be declared in its `# requires:` header.
/// This is the static half of the drift defence; the dynamic half is the spec harness running against minimal bundles.
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

/// Shape checks for precedence-aware parenthesization.
/// The spec harness proves the generated code *runs* right; these pin the two halves the harness cannot distinguish: that the parens a shape does not need are gone, and that the ones it does need are still there.
#[cfg(test)]
mod parens {
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

    fn assert_line(src: &str, want: &str) {
        assert!(
            src.lines().any(|l| l.trim() == want),
            "expected line `{want}` in:\n{src}"
        );
    }

    #[test]
    fn tighter_operands_lose_their_parens() {
        // `+` binds tighter than `&`, `>>` tighter than `&`: the wrap needs no group.
        assert_line(
            &i32_expr("(i32.add (local.get 0) (local.get 1))"),
            "l0 = l0 + l1 & 0xffffffff",
        );
        assert_line(
            &i32_expr("(i32.shr_u (local.get 0) (local.get 1))"),
            "l0 = l0 >> (l1 & 31)",
        );
        // A materialized comparison is a bare ternary in a statement position.
        assert_line(
            &i32_expr("(i32.lt_u (local.get 0) (local.get 1))"),
            "l0 = l0 < l1 ? 1 : 0",
        );
    }

    #[test]
    fn looser_operands_keep_their_parens() {
        // `|` and `^` share a level, so the right operand would reassociate.
        assert_line(
            &i32_expr("(i32.or (local.get 0) (i32.xor (local.get 0) (local.get 1)))"),
            "l0 = l0 | (l0 ^ l1)",
        );
        // A shift count is a `&`, looser than the shift itself.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (i32.and (local.get 0) (local.get 1)))"),
            "l0 = l0 << (l0 & l1 & 31) & 0xffffffff",
        );
        // Ruby's equality family is non-associative: `a == b == 0` is a syntax error.
        assert_line(
            &i32_expr("(i32.eqz (i32.and (local.get 0) (local.get 1)))"),
            "l0 = l0 & l1 == 0 ? 1 : 0",
        );
        // A comparison is negated whole, and `!` binds tighter than it (flipping the operator would be wrong for NaN).
        assert!(
            body(
                "(module (func (export \"f\") (param f64 f64) (result i32) (local i32) \
                 (if (i32.eqz (f64.lt (local.get 0) (local.get 1))) (then (local.set 2 (i32.const 1)))) (local.get 2)))"
            )
            .contains("if !(l0 < l1)"),
            "the negated comparison lost its group"
        );
    }
}

/// Codegen-shape checks for mask elision.
/// The spec harness proves the generated code computes the right values; these pin the shapes it cannot distinguish: a mask restored by a modular consumer is gone, and a mask a non-modular consumer or the Fixnum bound guard requires is still there.
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
            "l0 = l0 + l1 + l1 & 0xffffffff",
        );
        // `shr_s` exposes at most the signed 32-bit range, so its own mask goes too.
        assert_line(
            &i32_expr("(i32.add (i32.shr_s (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = (s32(l0) >> (l1 & 31)) + l1 & 0xffffffff",
        );
        // A shift count is read through `& 31`, so the subtraction feeding it needs no mask.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (i32.sub (local.get 1) (i32.const 1)))"),
            "l0 = l0 << (l1 - 1 & 31) & 0xffffffff",
        );
        // The wrap disappears when the exposed i64 is provably narrow (here at most 2^32).
        assert_line(
            &body(
                "(module (func (export \"f\") (param i64) (result i32) (local i32) \
                 (local.set 1 (i32.add (i32.wrap_i64 (i64.shr_u (local.get 0) (i64.const 32))) (i32.const 7))) (local.get 1)))",
            ),
            "l1 = (l0 >> 32) + 7 & 0xffffffff",
        );
    }

    #[test]
    fn non_modular_consumer_keeps_the_operand_mask() {
        // A division observes the exact value.
        assert_line(
            &i32_expr("(i32.div_u (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = i32_div_u(l0 + l1 & 0xffffffff, l1)",
        );
        // So does a comparison.
        assert_line(
            &i32_expr("(i32.lt_u (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = l0 + l1 & 0xffffffff < l1 ? 1 : 0",
        );
        // And a store to a local the dataflow cannot clear (the helper returns it directly): the outer mask stays.
        assert_line(
            &i32_expr("(i32.add (local.get 0) (local.get 1))"),
            "l0 = l0 + l1 & 0xffffffff",
        );
    }

    #[test]
    fn bound_guard_keeps_the_mask_on_wide_intermediates() {
        // A full-range i32 product reaches 2^64, past the Fixnum limit: the mul stays masked under a modular consumer.
        assert_line(
            &i32_expr("(i32.add (i32.mul (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = (l0 * l1 & 0xffffffff) + l1 & 0xffffffff",
        );
        // Narrowed to bytes, the product is provably small and the mask goes.
        assert_line(
            &i32_expr(
                "(i32.add (i32.mul (i32.and (local.get 0) (i32.const 255)) (i32.and (local.get 1) (i32.const 255))) (local.get 1))",
            ),
            "l0 = (l0 & 255) * (l1 & 255) + l1 & 0xffffffff",
        );
        // A full-range i64 wraps past the limit too: the wrap keeps its mask.
        assert_line(
            &body(
                "(module (func (export \"f\") (param i64) (result i32) (local i32) \
                 (local.set 1 (i32.add (i32.wrap_i64 (local.get 0)) (i32.const 7))) (local.get 1)))",
            ),
            "l1 = (l0 & 0xffffffff) + 7 & 0xffffffff",
        );
    }

    #[test]
    fn shift_count_reductions_fold_and_elide() {
        // A constant count folds at conversion time.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (i32.const 2))"),
            "l0 = l0 << 2 & 0xffffffff",
        );
        // The width reduces to 0; the shift stays, and the unmoved value makes the result mask the identity, so it drops too.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (i32.const 32))"),
            "l0 = l0 << 0",
        );
        // A variable count keeps the reduction.
        assert_line(
            &i32_expr("(i32.shl (local.get 0) (local.get 1))"),
            "l0 = l0 << (l1 & 31) & 0xffffffff",
        );
        // A count wasm code already reduced is provably in range: no second `& 63`.
        assert_line(
            &i64_expr("(i64.shl (local.get 0) (i64.and (local.get 1) (i64.const 63)))"),
            "l0 = m64(l0 << (l1 & 63))",
        );
    }

    #[test]
    fn dataflow_cleared_local_stores_unmasked() {
        // Every read of l2 is a modular operand, so its store renders in modular context and the store mask disappears; the read sites are unchanged text.
        let src = body(
            "(module (func (export \"f\") (param i32 i32) (result i32) (local i32) \
             (local.set 2 (i32.add (local.get 0) (i32.const 5))) \
             (i32.add (local.get 2) (local.get 1))))",
        );
        assert_line(&src, "l2 = l0 + 5");
        assert_line(&src, "return l2 + l1 & 0xffffffff");
    }

    #[test]
    fn observed_local_keeps_the_store_mask() {
        // l2 is returned directly, an exact observation, so its store keeps the mask.
        let src = body(
            "(module (func (export \"f\") (param i32) (result i32) (local i32) \
             (local.set 1 (i32.add (local.get 0) (i32.const 5))) \
             (local.get 1)))",
        );
        assert_line(&src, "l1 = l0 + 5 & 0xffffffff");
    }

    #[test]
    fn compounding_loop_carried_local_keeps_the_store_mask() {
        // l1 = l1 + 1 every iteration: unmasked, the interval would grow past the Fixnum limit, so the dataflow demotes it.
        let src = body(
            "(module (func (export \"f\") (param i32) (result i32) (local i32) \
             (loop $l \
               (local.set 1 (i32.add (local.get 1) (i32.const 1))) \
               (br_if $l (local.get 0))) \
             (i32.add (local.get 1) (local.get 0))))",
        );
        assert_line(&src, "l1 = l1 + 1 & 0xffffffff");
    }

    #[test]
    fn converging_loop_carried_local_stores_unmasked() {
        // The `& 255` re-narrows every iteration, so the interval settles and both the store mask and the add's own mask go.
        let src = body(
            "(module (func (export \"f\") (param i32) (result i32) (local i32) \
             (loop $l \
               (local.set 1 (i32.and (i32.add (local.get 1) (i32.const 1)) (i32.const 255))) \
               (br_if $l (local.get 0))) \
             (i32.add (local.get 1) (local.get 0))))",
        );
        assert_line(&src, "l1 = l1 + 1 & 255");
    }

    #[test]
    fn i64_elides_only_under_the_same_fixnum_bound() {
        // Two full-range i64 values sum past the Fixnum limit: `Rt.m64` stays even under the modular sub.
        assert_line(
            &i64_expr("(i64.sub (i64.add (local.get 0) (local.get 1)) (local.get 1))"),
            "l0 = m64(m64(l0 + l1) - l1)",
        );
        // Provably narrow (the high half plus a constant), the inner `Rt.m64` goes.
        assert_line(
            &i64_expr(
                "(i64.sub (local.get 1) (i64.add (i64.shr_u (local.get 0) (i64.const 32)) (i64.const 5)))",
            ),
            "l0 = m64(l1 - ((l0 >> 32) + 5))",
        );
    }

    #[test]
    fn and_chain_constants_fold_at_conversion_time() {
        assert_line(
            &i32_expr("(i32.and (i32.and (local.get 0) (i32.const 65280)) (i32.const 4080))"),
            "l0 = l0 & 3840",
        );
        // A single constant is the plain AND shape: nothing folds.
        assert_line(
            &i32_expr("(i32.and (i32.and (local.get 0) (local.get 1)) (i32.const 15))"),
            "l0 = l0 & l1 & 15",
        );
    }

    #[test]
    fn a_constant_and_drops_the_operand_mask_unguarded() {
        // The raw product exceeds the Fixnum guard, but `& 255` reduces at least as strongly as the mask it replaces.
        assert_line(
            &i32_expr("(i32.and (i32.mul (local.get 0) (local.get 1)) (i32.const 255))"),
            "l0 = l0 * l1 & 255",
        );
        // The i64 `Rt.m64` disappears the same way.
        assert_line(
            &i64_expr("(i64.and (i64.add (local.get 0) (local.get 1)) (i64.const 255))"),
            "l0 = l0 + l1 & 255",
        );
        // The reduction covers only the AND's own operand: under the XOR the bound guard still binds.
        assert_line(
            &i32_expr(
                "(i32.and (i32.xor (i32.mul (local.get 0) (local.get 1)) (local.get 1)) (i32.const 255))",
            ),
            "l0 = (l0 * l1 & 0xffffffff ^ l1) & 255",
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
            "l1 = l0 >> 32",
        );
    }

    #[test]
    fn a_pinned_constant_equality_drops_the_mask() {
        // `(l0 - 5) & 0xffffffff == 7` admits exactly one raw preimage, and the constant migrates across the sub.
        assert_line(
            &i32_expr("(i32.eq (i32.sub (local.get 0) (i32.const 5)) (i32.const 7))"),
            "l0 = l0 == 12 ? 1 : 0",
        );
        // The same rewrite in boolean position.
        assert_line(
            &body(
                "(module (func (export \"f\") (param i32) (result i32) (local i32) \
                 (if (i32.eq (i32.sub (local.get 0) (i32.const 5)) (i32.const 7)) (then (local.set 1 (i32.const 1)))) (local.get 1)))",
            ),
            "if l0 == 12",
        );
        // `eqz` is the equality against 0.
        assert_line(
            &body(
                "(module (func (export \"f\") (param i32) (result i32) (local i32) \
                 (if (i32.eqz (i32.sub (local.get 0) (i32.const 5))) (then (local.set 1 (i32.const 1)))) (local.get 1)))",
            ),
            "if l0 == 5",
        );
    }

    #[test]
    fn an_unpinned_constant_equality_keeps_the_mask() {
        // A two-sided sub spans almost 2^33: two candidates fit, so the mask stays.
        assert_line(
            &i32_expr("(i32.eq (i32.sub (local.get 0) (local.get 1)) (i32.const 7))"),
            "l0 = l0 - l1 & 0xffffffff == 7 ? 1 : 0",
        );
        // No candidate fits: statically false, but the comparison still runs (the operand could trap), only the identity mask goes.
        assert_line(
            &i32_expr(
                "(i32.eq (i32.add (i32.and (local.get 0) (i32.const 3)) (i32.const 1)) (i32.const 9))",
            ),
            "l0 = (l0 & 3) + 1 == 9 ? 1 : 0",
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
            "l1 = @m.iwlo(l0, 12)",
        );
        assert_line(
            &mem_stmt("(i32.store offset=8 (local.get 0) (local.get 1))"),
            "@m.iwso(l0, 8, l1)",
        );
    }

    #[test]
    fn offset_zero_keeps_the_one_argument_unit() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load (local.get 0)))"),
            "l1 = @m.iwl(l0)",
        );
        assert_line(
            &mem_stmt("(i32.store (local.get 0) (local.get 1))"),
            "@m.iws(l0, l1)",
        );
    }

    #[test]
    fn constant_base_folds_with_the_offset() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load offset=12 (i32.const 4)))"),
            "l1 = @m.iwl(16)",
        );
        assert_line(
            &mem_stmt("(i32.store offset=8 (i32.const 4) (local.get 1))"),
            "@m.iws(12, l1)",
        );
    }

    #[test]
    fn dynamic_add_address_rides_as_two_arguments() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load (i32.add (local.get 0) (local.get 1))))"),
            "l1 = @m.iwla(l0, l1)",
        );
        assert_line(
            &mem_stmt("(i32.store (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "@m.iwsa(l0, l1, l1)",
        );
    }

    #[test]
    fn constant_add_operand_keeps_the_one_argument_unit() {
        assert_line(
            &mem_stmt("(local.set 1 (i32.load (i32.add (i32.const 4) (local.get 0))))"),
            "l1 = @m.iwl(4 + l0)",
        );
    }

    #[test]
    fn dynamic_add_under_a_nonzero_offset_keeps_the_offset_unit() {
        assert_line(
            &mem_stmt("(i32.store offset=8 (i32.add (local.get 0) (local.get 1)) (local.get 1))"),
            "@m.iwso(l0 + l1, 8, l1)",
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
            "l1 = @m.iwl(l0 + 4)",
        );
        assert_line(
            &mem_stmt("(i32.store (i32.add (local.get 0) (i32.const 4)) (local.get 1))"),
            "@m.iws(l0 + 4, l1)",
        );
    }

    #[test]
    fn two_argument_base_renders_bare() {
        assert_line(
            &mem_stmt("(i32.store offset=8 (i32.add (local.get 0) (i32.const 4)) (local.get 1))"),
            "@m.iwso(l0 + 4, 8, l1)",
        );
    }

    #[test]
    fn store_value_renders_bare() {
        assert_line(
            &mem_stmt("(i32.store (local.get 0) (i32.add (local.get 1) (i32.const 1)))"),
            "@m.iws(l0, l1 + 1)",
        );
    }

    #[test]
    fn narrow_store_value_renders_bare() {
        assert_line(
            &mem_stmt("(i32.store8 (local.get 0) (i32.add (local.get 1) (i32.const 1)))"),
            "@m.iwsb(l0, l1 + 1)",
        );
    }

    #[test]
    fn constant_base_past_the_address_space_keeps_the_exact_addition() {
        // The folded sum would be reduced by the unit into a bounds success; base plus offset reaches the bounds check unreduced and traps.
        assert_line(
            &mem_stmt("(local.set 1 (i32.load offset=4294967295 (i32.const 1)))"),
            "l1 = @m.iwlo(1, 4294967295)",
        );
    }
}

/// Codegen-shape checks for control flow: a *deep* multi-level `br` must be addressed by value (a state assignment plus `next` into the dispatch loop), never by `catch`/`throw`; a shallow one must keep the `__br` relay, measured cheaper than a dispatch at that depth.
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

    // block $A { loop $B { block $C { br_table $C $B $A } ... } }
    // A single `br_table` whose targets span all three nesting depths (self-exit, loop-continue from a nested frame, and outer-block-exit), exercising the fast path, a wrapped loop, and a relayed landing at once.
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

    // block $done { loop $l { br_if $done ...; br $l } }
    // The standard compilation of a `while` with a conditional exit.
    // The `br_if` crosses exactly one loop that is its block's sole statement, so the sole-statement exemption keeps both structured: a Ruby loop with a `break`, no dispatch.
    // This is the shape most prone to silent regression: dropping the exemption keeps every spec trial passing and only costs tight-loop speed.
    const SOLE_LOOP_EXIT: &str = r#"
      (module
        (func (export "f") (param i32) (result i32)
          (local i32)
          (block $done
            (loop $l
              (br_if $done (i32.eqz (local.get 0)))
              (local.set 0 (i32.sub (local.get 0) (i32.const 1)))
              (local.set 1 (i32.add (local.get 1) (i32.const 3)))
              (br $l)))
          (local.get 1)))
    "#;

    #[test]
    fn sole_statement_loop_exit_stays_structured() {
        let src = convert(SOLE_LOOP_EXIT);
        assert!(src.contains("while true"), "no structured loop in:\n{src}");
        assert!(
            src.contains("break"),
            "no plain break for the exit in:\n{src}"
        );
        assert!(
            !src.contains("case state"),
            "sole-statement loop was dissolved into a dispatch in:\n{src}"
        );
        assert!(!src.contains("state ="), "state machine leaked in:\n{src}");
    }

    /// A `br_table` tower `depth` blocks deep whose table names every level, so the outermost target is crossed by a branch of exactly that path length:
    /// the wasm compilation of a C `switch`, and the shape whose size decides between the two lowerings.
    fn tower_body(depth: usize) -> String {
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
            "{opens} (br_table {targets} (local.get 0)) {closes}",
            closes = ")".repeat(depth)
        )
    }

    fn tower(depth: usize) -> String {
        format!(
            "(module (func (export \"f\") (param i32) (result i32) (local i32) \
             {} (local.set 1 (i32.const 7)) (local.get 1)))",
            tower_body(depth)
        )
    }

    #[test]
    fn deep_multi_level_br_is_addressed_by_value() {
        let src = convert(&tower(flat::DEEP_CROSSING + 2));
        assert!(src.contains("state = 0"), "no dispatch entry in:\n{src}");
        assert!(src.contains("case state"), "no dispatch in:\n{src}");
        assert!(src.contains("; next"), "no state transition in:\n{src}");
        // The retired shapes: scope-walking relay and catch/throw.
        assert!(!src.contains("elsif __br"), "relay arm survived in:\n{src}");
        assert!(
            !src.contains("end while false"),
            "cascade frame survived in:\n{src}"
        );
        assert!(!src.contains("catch("), "catch survived in:\n{src}");
        assert!(!src.contains("throw "), "throw survived in:\n{src}");
    }

    #[test]
    fn shallow_multi_level_br_keeps_the_relay() {
        // The same tower, one level below the threshold: a relay of that depth is cheaper than a dispatch, so nothing dissolves.
        let src = convert(&tower(flat::DEEP_CROSSING - 1));
        assert!(
            src.contains("elsif __br"),
            "no relay arm for a shallow branch in:\n{src}"
        );
        assert!(
            !src.contains("case state"),
            "a shallow branch was flattened in:\n{src}"
        );
        assert!(!src.contains("state ="), "state machine leaked in:\n{src}");
    }

    // block $A { block $B { br_table $B $A } ... } with nothing after the tower: the epilogue at $A's method-body level is only a clear, and nothing later reads `__br`.
    const DEAD_CLEAR: &str = r#"
      (module
        (func (export "f") (param i32) (result i32)
          (local i32)
          (block $A
            (block $B
              (br_table $B $A (local.get 0)))
            (local.set 1 (i32.const 7)))
          (local.get 1)))
    "#;

    #[test]
    fn dead_method_level_clear_is_dropped() {
        let src = convert(DEAD_CLEAR);
        assert!(
            src.contains("elsif __br"),
            "the inner relay disappeared in:\n{src}"
        );
        assert!(
            !src.contains("__br = nil if"),
            "a dead method-level clear survived in:\n{src}"
        );
    }

    #[test]
    fn method_level_clear_before_a_later_reader_is_kept() {
        // Two towers in sequence: the first tower's clear protects the second tower's epilogue reads, the second's protects nothing.
        let src = convert(
            r#"
          (module
            (func (export "f") (param i32) (result i32)
              (local i32)
              (block $A
                (block $B
                  (br_table $B $A (local.get 0)))
                (local.set 1 (i32.const 7)))
              (block $C
                (block $D
                  (br_table $D $C (local.get 0)))
                (local.set 1 (i32.const 9)))
              (local.get 1)))
        "#,
        );
        assert!(
            src.contains("__br = nil if __br == 1"),
            "the clear before a later reader was dropped in:\n{src}"
        );
        assert!(
            !src.contains("__br = nil if __br == 3"),
            "the dead final clear survived in:\n{src}"
        );
    }

    #[test]
    fn method_level_clear_under_a_dissolved_loop_is_kept() {
        // A deep tower dissolves the enclosing loop, whose back-edge re-runs the shallow tower behind it: dropping that tower's clear would let a stale `__br` reach its epilogues on the next trip, even though nothing follows it in emission order.
        let deep = tower_body(flat::DEEP_CROSSING + 2);
        let src = convert(&format!(
            "(module (func (export \"f\") (param i32) (result i32) (local i32) \
             (loop $L \
               {deep} \
               (block $A (block $B (br_table $B $A (local.get 0))) (local.set 1 (i32.const 7))) \
               (br_if $L (local.get 1))) \
             (local.get 1)))"
        ));
        assert!(src.contains("case state"), "no dispatch in:\n{src}");
        assert!(
            src.contains("__br = nil if"),
            "the clear under a dissolved loop was dropped in:\n{src}"
        );
    }

    #[test]
    fn mixed_depths_stay_structured() {
        // The three-deep mixed-target shape: every crossing is shallow, so the whole function keeps the cascade's structured frames and its wrapped-loop back-edge.
        let src = convert(MIXED_DEPTHS);
        assert!(src.contains("while true"), "no structured loop in:\n{src}");
        assert!(
            src.contains("end while false"),
            "no cascade frame in:\n{src}"
        );
        assert!(!src.contains("case state"), "dispatch appeared in:\n{src}");
        assert!(!src.contains("catch("), "catch survived in:\n{src}");
    }
}
