//! Perl backend: translates dewasm IR into a Perl package (plus a bundled lightweight runtime).
//!
//! Lowering conventions (ADR-55; numeric conventions ADR-2):
//! - i32/i64 are masked-unsigned perl integers (IV/UV); signed views via `Rt::s32`/`Rt::s64` only where an instruction needs them. Operations whose intermediates exceed the NV-exact range (i32 mul, all i64 add/sub/mul, div/rem, shr_s) live in runtime helpers built on tightly-scoped `use integer` blocks or the exact unsigned-division idiom.
//! - f32/f64 are native NVs; f32 results are re-rounded with `Rt::f32` (with the pack-'f' overflow boundary handled in software), NaN bit paths go through software bit conversion, and division goes through `Rt::fdiv` because perl dies on `x / 0.0`.
//! - Control flow lowers to perl's native labeled blocks: `Block` becomes `Ln: { ... }`, `Loop` becomes `Ln: while (1) { ... last Ln; }`, and every `br` is a direct `last Ln`/`next Ln` — `last`/`next` escape any enclosing labeled frame at arbitrary depth, so the flag-variable cascades Ruby (ADR-42) and Python (ADR-28) need do not exist here.
//! - Call-stack exhaustion is enforced by an explicit depth counter (`local $Rt::DEPTH`), because runaway perl recursion only stops at the OOM killer.
//!
//! The runtime is composed from per-method units (ADR-6) in `Rt`-rooted packages (`Rt`, `Rt::Memory`, `Rt::Table`, `Rt::Global`). Perl package names are absolute, so `Embedded` linkage namespaces the whole runtime under the generated package (`Foo::Rt`) by rewriting the `Rt::` prefix at bundle time; `Alias` linkage keeps the shared top-level `Rt` (the spec harness).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::OnceLock;

use anyhow::Result;
use dewasm_backend::{
    check_module_support, Backend, CodeWriter, GenOptions, Mode, OutputFile, RuntimeBundler,
    RuntimeLinkage, RuntimeScope, SupportStatus,
};
use dewasm_core::feature::Feature;
use dewasm_core::ir::{
    BinOp, BrTarget, ElemItem, ElemKind, ExportKind, Expr, Func, LoadOp, Module, Stmt, StoreOp,
    Temp, UnOp, ValType,
};

include!(concat!(env!("OUT_DIR"), "/units.rs"));

/// The runtime unit bundler for Perl (see runtime/perl/units/).
pub fn bundler() -> &'static RuntimeBundler {
    static BUNDLER: OnceLock<RuntimeBundler> = OnceLock::new();
    BUNDLER.get_or_init(|| {
        RuntimeBundler::new(
            "#",
            "    ",
            vec![
                RuntimeScope {
                    prefix: "rt",
                    open: "package Rt;",
                    close: "",
                    prelude: Some("rt/_package"),
                },
                RuntimeScope {
                    prefix: "memory",
                    open: "package Rt::Memory;",
                    close: "",
                    prelude: Some("memory/_package"),
                },
                RuntimeScope {
                    prefix: "table",
                    open: "package Rt::Table;",
                    close: "",
                    prelude: Some("table/_package"),
                },
                RuntimeScope {
                    prefix: "global",
                    open: "package Rt::Global;",
                    close: "",
                    prelude: Some("global/_package"),
                },
                // WASI preview 1 lands with the follow-up issue; the scope is declared so its units slot in without bundler changes.
                RuntimeScope {
                    prefix: "wasi",
                    open: "package Rt::WASI;",
                    close: "",
                    prelude: None,
                },
            ],
            UNIT_SOURCES,
        )
        .expect("runtime units are well-formed")
    })
}

/// Emit a shared runtime (`package Rt;` and friends) for the closure of `seeds`; generated packages then use `RuntimeLinkage::Alias("Rt")`. The bundle ends by switching back to `package main;`.
pub fn shared_runtime(seeds: &BTreeSet<String>) -> Result<String> {
    Ok(format!("{}package main;\n", bundler().bundle(seeds, 0)?))
}

/// Locate a perl interpreter (>= 5.26, 64-bit IVs and doubles) able to run generated scripts. Honors `$DEWASM_PERL`, then `perl` (ADR-15: a missing or unsuitable interpreter is a loud failure at the call site, not here — this only reports what qualifies).
pub fn find_perl() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("DEWASM_PERL") {
        candidates.push(PathBuf::from(env));
    }
    candidates.push(PathBuf::from("perl"));
    for candidate in candidates {
        let Ok(out) = std::process::Command::new(&candidate)
            .args([
                "-MConfig",
                "-e",
                "print(($] >= 5.026 && $Config{ivsize} == 8 && $Config{nvsize} == 8) ? 1 : 0)",
            ])
            .output()
        else {
            continue;
        };
        if out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "1" {
            return Some(candidate);
        }
    }
    None
}

/// Generate one package for `module`. Returns the package source and the set of runtime units it needs (already bundled inside for `Embedded`).
pub fn generate_package_with_units(
    module: &Module,
    package_name: &str,
    linkage: &RuntimeLinkage,
    default_wasi: bool,
) -> Result<(String, BTreeSet<String>)> {
    generate_package_inner(
        module,
        package_name,
        linkage,
        default_wasi,
        &BTreeSet::new(),
        None,
    )
}

fn generate_package_inner(
    module: &Module,
    package_name: &str,
    linkage: &RuntimeLinkage,
    default_wasi: bool,
    extra_seeds: &BTreeSet<String>,
    data_file: Option<&str>,
) -> Result<(String, BTreeSet<String>)> {
    check_module_support(&PerlBackend, module)?;
    // Prefix sums: `data_offsets[i]` is where segment `i` begins in the concatenated sidecar blob (ADR-37). Only consulted when externalizing.
    let mut data_offsets = Vec::with_capacity(module.datas.len());
    let mut acc = 0usize;
    for data in &module.datas {
        data_offsets.push(acc);
        acc += data.data.len();
    }
    // Perl package names are absolute (no lexical nesting), so the Embedded runtime is namespaced under the generated package by rewriting the `Rt::` prefix below; generated code references the same prefixed name directly.
    let rt_name = match linkage {
        RuntimeLinkage::Embedded => format!("{package_name}::Rt"),
        RuntimeLinkage::Alias(_) => "Rt".to_string(),
    };
    let gen = Gen {
        module,
        default_wasi,
        uses: RefCell::new(extra_seeds.clone()),
        data_file: data_file.map(str::to_string),
        data_offsets,
        rt_name,
    };
    let mut wb = CodeWriter::new("    ");
    gen.package(&mut wb, package_name);
    let body = wb.finish();
    let uses = gen.uses.into_inner();

    let mut out = String::new();
    match linkage {
        RuntimeLinkage::Embedded => {
            if !uses.is_empty() {
                let bundle = bundler().bundle(&uses, 0)?;
                // Namespace the runtime under the generated package: `Rt::` references (calls, class-name strings) and the scope-opening `package Rt;` all move under `<pkg>::Rt`.
                let bundle = bundle
                    .replace("Rt::", &format!("{package_name}::Rt::"))
                    .replace("package Rt;", &format!("package {package_name}::Rt;"));
                out.push_str(&bundle);
                out.push_str("package main;\n\n");
            }
        }
        RuntimeLinkage::Alias(path) => {
            // Generated code references the runtime as `Rt` directly; a shared runtime under any other name is aliased in via a stash alias.
            if path != "Rt" {
                out.push_str(&format!("BEGIN {{ *Rt:: = \\%{path}::; }}\n\n"));
            }
        }
    }
    out.push_str(&body);
    Ok((out, uses))
}

pub struct PerlBackend;

impl Backend for PerlBackend {
    fn name(&self) -> &str {
        "perl"
    }

    fn file_extension(&self) -> &str {
        "pl"
    }

    fn has_wasi_p1(&self, name: &str) -> bool {
        bundler().has_unit(&format!("wasi/{name}"))
    }

    fn feature_status(&self, feature: Feature) -> SupportStatus {
        match feature {
            // Perl NVs are IEEE doubles; f32 re-rounding and the NaN paths follow ADR-2 (mirroring Ruby/Python).
            Feature::Floats => SupportStatus::Supported,
            // The wasm-1.0 completion (ADR-16 model): boxed globals, imported globals/memories/tables, multiple tables, and the table half of bulk memory.
            Feature::ImportedGlobals
            | Feature::ImportedMemories
            | Feature::ImportedTables
            | Feature::MultipleTables
            | Feature::TableBulkOps => SupportStatus::Supported,
            _ => SupportStatus::Unsupported,
        }
    }

    fn generate(&self, module: &Module, opts: &GenOptions) -> Result<Vec<OutputFile>> {
        let package_name = package_name(&opts.module_name);

        // The Exit/Trap handlers in the standalone main need these even when the module itself never references them.
        let mut extra_seeds = BTreeSet::new();
        if opts.mode == Mode::Standalone {
            extra_seeds.insert("rt/trap".to_string());
            extra_seeds.insert("rt/exit".to_string());
        }

        let (package_src, _) = generate_package_inner(
            module,
            &package_name,
            &opts.runtime,
            opts.default_wasi,
            &extra_seeds,
            opts.data_file.as_ref().map(|c| c.sidecar_name.as_str()),
        )?;

        let mut w = CodeWriter::new("    ");
        w.line("#!/usr/bin/env perl");
        w.line("# Generated by dewasm. Do not edit.");
        // Externalized data blob (ADR-37): read once at load time from the sidecar next to this file, then sliced by the generated `substr($DATA_BLOB, o, len)` expressions. Only emitted when there is data to externalize (otherwise the generated code never reads it).
        if let Some(cfg) = &opts.data_file {
            if !module.datas.is_empty() {
                w.line("use File::Basename ();");
                w.line("my $DATA_BLOB = do {");
                w.indent();
                w.line(format!(
                    "open(my $fh, '<:raw', File::Basename::dirname(__FILE__) . '/' . {}) or die \"dewasm: cannot open data sidecar: $!\\n\";",
                    perl_string(&cfg.sidecar_name)
                ));
                w.line("local $/;");
                w.line("scalar <$fh>;");
                w.dedent();
                w.line("};");
            }
        }
        w.line("");
        w.raw(&package_src);

        if opts.mode == Mode::Standalone {
            let rt = match &opts.runtime {
                RuntimeLinkage::Embedded => format!("{package_name}::Rt"),
                RuntimeLinkage::Alias(_) => "Rt".to_string(),
            };
            w.line("");
            w.line("package main;");
            w.line("");
            w.line(format!("my $_inst = {package_name}->new({{}});"));
            w.line("eval { $_inst->invoke('_start'); };");
            w.line("if (my $_e = $@) {");
            w.indent();
            w.line(format!(
                "exit($_e->{{code}}) if ref($_e) && $_e->isa('{rt}::Exit');"
            ));
            w.line(format!(
                "if (ref($_e) && $_e->isa('{rt}::Trap')) {{ print STDERR \"trap: $_e->{{message}}\\n\"; exit(134); }}"
            ));
            w.line("die $_e;");
            w.dedent();
            w.line("}");
            w.line("exit(0);");
        } else {
            // Library mode: the file must return true so embedders can `require` it.
            w.line("");
            w.line("1;");
        }

        let mut files = vec![OutputFile {
            name: format!("{}.pl", opts.module_name),
            contents: w.finish().into_bytes(),
        }];
        // The data sidecar (ADR-37): every segment's bytes concatenated in segment order, matching the `data_offsets` prefix sums baked into the generated `substr($DATA_BLOB, o, len)` slices.
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

fn package_name(module_name: &str) -> String {
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

/// Perl single-quoted string literal (no interpolation, so `$`/`@` in wasm names are inert; raw control bytes are legal inside single quotes).
fn perl_string(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

fn hex_bytes(data: &[u8]) -> String {
    let mut hex = String::with_capacity(data.len() * 2);
    for b in data {
        hex.push_str(&format!("{b:02x}"));
    }
    format!("pack('H*', '{hex}')")
}

/// WASI import module names the bundled runtime answers for. `wasi_unstable` (snapshot 0) shares preview 1's ABI for everything implemented here.
const WASI_MODULES: &[&str] = &["wasi_snapshot_preview1", "wasi_unstable"];

fn is_wasi_module(name: &str) -> bool {
    WASI_MODULES.contains(&name)
}

pub use dewasm_backend::WASI_PREVIEW1_FUNCTIONS;

fn temp(t: Temp) -> String {
    format!("$s{}", t.depth)
}

fn default_value(ty: ValType) -> &'static str {
    match ty {
        ValType::I32 | ValType::I64 => "0",
        ValType::F32 | ValType::F64 => "0.0",
        ValType::FuncRef => "undef",
    }
}

struct Gen<'a> {
    module: &'a Module,
    default_wasi: bool,
    /// Runtime units the generated code references.
    uses: RefCell<BTreeSet<String>>,
    /// When `Some`, data segments are externalized into a binary sidecar of this filename (loaded once into the file-scoped `$DATA_BLOB`) instead of embedded as `pack('H*', ...)` literals (ADR-37); `data_offsets[i]` locates segment `i` in the blob.
    data_file: Option<String>,
    data_offsets: Vec<usize>,
    /// The runtime's package name as generated code spells it: `Rt` under `Alias` linkage, `<Package>::Rt` under `Embedded` (perl package names are absolute).
    rt_name: String,
}

impl<'a> Gen<'a> {
    /// The Perl expression yielding a data segment's bytes: a slice of the externalized `$DATA_BLOB` when `--data-file` is on, else an inline `pack('H*', ...)` literal (ADR-37). Both yield a byte string.
    fn data_expr(&self, seg: usize, data: &[u8]) -> String {
        if self.data_file.is_some() {
            let o = self.data_offsets[seg];
            format!("substr($DATA_BLOB, {o}, {})", data.len())
        } else {
            hex_bytes(data)
        }
    }

    fn use_unit(&self, id: &str) {
        self.uses.borrow_mut().insert(id.to_string());
    }

    /// Reference a runtime helper, recording its unit.
    fn rt(&self, name: &str) -> String {
        self.use_unit(&format!("rt/{name}"));
        format!("{}::{name}", self.rt_name)
    }

    /// The runtime class name for `scope` (`Memory`, `Table`, `Global`), recording its prelude unit.
    fn rt_class(&self, scope: &str, class: &str) -> String {
        self.use_unit(&format!("{scope}/_package"));
        format!("{}::{class}", self.rt_name)
    }

    fn resolve_import_string(&self, kind: &str, module: &str, name: &str) -> String {
        format!(
            "{}({}($imports, {}, {}), {}, {}, {})",
            self.rt("check_import_kind"),
            self.rt("resolve_import"),
            perl_string(module),
            perl_string(name),
            perl_string(kind),
            perl_string(module),
            perl_string(name)
        )
    }

    fn package(&self, w: &mut CodeWriter, package_name: &str) {
        w.line(format!("package {package_name} {{"));
        w.indent();
        self.constants(w);
        w.line("");
        self.initialize(w);
        w.line("");
        self.helpers(w);
        for (i, func) in self.module.funcs.iter().enumerate() {
            w.line("");
            let idx = self.module.num_imported_funcs() as usize + i;
            self.function(w, idx as u32, func);
        }
        w.dedent();
        w.line("}");
    }

    fn constants(&self, w: &mut CodeWriter) {
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
        let entries = |pairs: &[(String, u32)], prefix: &str| {
            pairs
                .iter()
                .map(|(name, idx)| {
                    format!(
                        "{} => {}",
                        perl_string(name),
                        perl_string(&format!("{prefix}{idx}"))
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        };
        w.line(format!(
            "our %GLOBAL_EXPORTS = ({});",
            entries(&global_exports, "g")
        ));
        w.line(format!(
            "our %TABLE_EXPORTS = ({});",
            entries(&table_exports, "t")
        ));
        let mem_set = memory_export_names
            .iter()
            .map(|n| format!("{} => 1", perl_string(n)))
            .collect::<Vec<_>>()
            .join(", ");
        w.line(format!("our %MEMORY_EXPORTS = ({mem_set});"));
    }

    fn initialize(&self, w: &mut CodeWriter) {
        let m = self.module;
        w.line("sub new {");
        w.indent();
        w.line("my ($class, $imports) = @_;");
        w.line("$imports = {} unless defined $imports;");
        w.line("my $self = bless({}, $class);");

        if let Some(import) = &m.imported_memory {
            w.line(format!(
                "$self->{{memory}} = {} // $self->_missing({}, {});",
                self.resolve_import_string("memory", &import.module, &import.name),
                perl_string(&import.module),
                perl_string(&import.name),
            ));
        } else if let Some(mem) = &m.memory {
            let max = mem
                .max_pages
                .map(|p| p.to_string())
                .unwrap_or_else(|| "undef".to_string());
            w.line(format!(
                "$self->{{memory}} = {}->new({}, {});",
                self.rt_class("memory", "Memory"),
                mem.min_pages,
                max
            ));
        } else {
            w.line("$self->{memory} = undef;");
        }

        for (i, import) in m.imported_tables.iter().enumerate() {
            w.line(format!(
                "$self->{{t{i}}} = {} // $self->_missing({}, {});",
                self.resolve_import_string("table", &import.module, &import.name),
                perl_string(&import.module),
                perl_string(&import.name),
            ));
        }
        let num_imported_tables = m.num_imported_tables();
        for (i, table) in m.tables.iter().enumerate() {
            let idx = num_imported_tables as usize + i;
            let max = table
                .max
                .map(|p| p.to_string())
                .unwrap_or_else(|| "undef".to_string());
            w.line(format!(
                "$self->{{t{idx}}} = {}->new({}, {max});",
                self.rt_class("table", "Table"),
                table.min
            ));
        }

        for (i, import) in m.imported_funcs.iter().enumerate() {
            // Fallback order (ADR-7): explicit import -> bundled WASI unit -> ENOSYS stub; non-WASI imports stay mandatory (a missing one is a link error).
            let fallback = if is_wasi_module(&import.module) && self.default_wasi {
                let unit = format!("wasi/{}", import.name);
                if bundler().has_unit(&unit) {
                    self.use_unit(&unit);
                    format!("$self->_wasi_import({})", perl_string(&import.name))
                } else {
                    "sub { return 52 }".to_string() // ENOSYS: not implemented yet
                }
            } else {
                self.use_unit("rt/link_error");
                format!(
                    "$self->_missing({}, {})",
                    perl_string(&import.module),
                    perl_string(&import.name)
                )
            };
            w.line(format!(
                "$self->{{if{i}}} = {} // {fallback};",
                self.resolve_import_string("func", &import.module, &import.name)
            ));
        }

        for (i, import) in m.imported_globals.iter().enumerate() {
            w.line(format!(
                "$self->{{g{i}}} = {} // $self->_missing({}, {});",
                self.resolve_import_string("global", &import.module, &import.name),
                perl_string(&import.module),
                perl_string(&import.name),
            ));
        }
        let num_imported_globals = m.imported_globals.len();
        for (i, global) in m.globals.iter().enumerate() {
            let idx = num_imported_globals + i;
            w.line(format!(
                "$self->{{g{idx}}} = {}->new({});",
                self.rt_class("global", "Global"),
                self.expr(&global.init)
            ));
        }

        for (i, elem) in m.elems.iter().enumerate() {
            let items = || {
                elem.items
                    .iter()
                    .map(|item| match item {
                        ElemItem::Func(func_idx) => self.func_pair(*func_idx),
                        ElemItem::Null => "undef".to_string(),
                        ElemItem::Global(idx) => format!("$self->{{g{idx}}}{{value}}"),
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            match &elem.kind {
                ElemKind::Declared => w.line(format!("$self->{{elem{i}}} = [];")),
                ElemKind::Passive => w.line(format!("$self->{{elem{i}}} = [{}];", items())),
                ElemKind::Active {
                    table_index,
                    offset,
                } => {
                    self.use_unit("table/init");
                    w.line(format!("$self->{{elem{i}}} = [{}];", items()));
                    let offset = self.expr(offset);
                    w.line(format!(
                        "$self->{{t{table_index}}}->init({offset}, $self->{{elem{i}}}, 0, {});",
                        elem.items.len()
                    ));
                    w.line(format!("$self->{{elem{i}}} = [];"));
                }
            }
        }

        for (i, data) in m.datas.iter().enumerate() {
            match &data.offset {
                Some(offset) => {
                    self.use_unit("memory/init");
                    w.line(format!(
                        "$self->{{memory}}->init({}, {}, 0, {});",
                        self.expr(offset),
                        self.data_expr(i, &data.data),
                        data.data.len()
                    ));
                    w.line(format!("$self->{{data{i}}} = '';"));
                }
                None => {
                    w.line(format!(
                        "$self->{{data{i}}} = {};",
                        self.data_expr(i, &data.data)
                    ));
                }
            }
        }

        let mut export_entries = Vec::new();
        for export in &m.exports {
            if let ExportKind::Func(idx) = export.kind {
                export_entries.push(format!(
                    "{} => {}",
                    perl_string(&export.name),
                    self.func_closure(idx)
                ));
            }
        }
        w.line(format!(
            "$self->{{exports}} = {{{}}};",
            export_entries.join(", ")
        ));

        if let Some(start) = m.start {
            w.line(format!("{};", self.call_string(start, &[])));
        }
        w.line("return $self;");
        w.dedent();
        w.line("}");
    }

    fn helpers(&self, w: &mut CodeWriter) {
        w.line("sub invoke {");
        w.indent();
        w.line("my ($self, $name, @args) = @_;");
        w.line("return $self->{exports}{$name}->(@args);");
        w.dedent();
        w.line("}");
        w.line("");
        w.line("sub global_get {");
        w.indent();
        w.line("my ($self, $name) = @_;");
        w.line("return $self->{$GLOBAL_EXPORTS{$name}}{value};");
        w.dedent();
        w.line("}");
        w.line("");
        // The boxed Rt::Global itself (not its current value), for a host embedder or another dewasm instance to import as a shared mutable cell (ADR-16).
        w.line("sub global_export {");
        w.indent();
        w.line("my ($self, $name) = @_;");
        w.line("return $self->{$GLOBAL_EXPORTS{$name}};");
        w.dedent();
        w.line("}");
        w.line("");
        w.line("sub table_export {");
        w.indent();
        w.line("my ($self, $name) = @_;");
        w.line("return $self->{$TABLE_EXPORTS{$name}};");
        w.dedent();
        w.line("}");
        w.line("");
        // ADR-7 provider protocol: an instance is itself a valid import value.
        w.line("sub wasm_import {");
        w.indent();
        w.line("my ($self, $name) = @_;");
        w.line("return $self->{exports}{$name} if exists $self->{exports}{$name};");
        w.line("return $self->{$GLOBAL_EXPORTS{$name}} if exists $GLOBAL_EXPORTS{$name};");
        w.line("return $self->{$TABLE_EXPORTS{$name}} if exists $TABLE_EXPORTS{$name};");
        w.line("return $self->{memory} if exists $MEMORY_EXPORTS{$name};");
        w.line("return undef;");
        w.dedent();
        w.line("}");
        w.line("");
        w.line("sub _missing {");
        w.indent();
        w.line("my ($self, $mod, $name) = @_;");
        w.line(format!(
            "{}(\"missing import $mod.$name\");",
            self.rt("link_error")
        ));
        w.dedent();
        w.line("}");
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

    /// A structural key for a function type (not a module-local index), so a table shared across modules stays consistent.
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
        perl_string(&format!("{}->{}", names(&ty.params), names(&ty.results)))
    }

    /// A callable for the function: the imported coderef itself, or a closure over the instance method.
    fn func_closure(&self, func_idx: u32) -> String {
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("$self->{{if{func_idx}}}")
        } else {
            format!("sub {{ return $self->_f{func_idx}(@_); }}")
        }
    }

    /// A funcref value: the `[type_key, coderef]` pair tables store (ADR-16).
    fn func_pair(&self, func_idx: u32) -> String {
        format!(
            "[{}, {}]",
            self.func_type_symbol(func_idx),
            self.func_closure(func_idx)
        )
    }

    fn call_string(&self, func_idx: u32, args: &[String]) -> String {
        let args = args.join(", ");
        if (func_idx as usize) < self.module.imported_funcs.len() {
            format!("$self->{{if{func_idx}}}->({args})")
        } else {
            format!("$self->_f{func_idx}({args})")
        }
    }

    fn function(&self, w: &mut CodeWriter, idx: u32, func: &Func) {
        let ty = &self.module.types[func.type_idx as usize];
        let mut params = String::new();
        for i in 0..ty.params.len() {
            params.push_str(&format!(", $l{i}"));
        }
        w.line(format!("sub _f{idx} {{"));
        w.indent();
        w.line(format!("my ($self{params}) = @_;"));
        // The explicit call-depth cutoff (ADR-55): `local` restores the counter on every exit path, including a trap's die-unwind.
        self.use_unit("rt/exhausted");
        w.line(format!(
            "local ${0}::DEPTH = ${0}::DEPTH + 1;",
            self.rt_name
        ));
        w.line(format!(
            "{}() if ${1}::DEPTH > ${1}::LIMIT;",
            self.rt("exhausted"),
            self.rt_name
        ));
        for (i, local_ty) in func.locals.iter().enumerate() {
            let name = format!("$l{}", ty.params.len() + i);
            w.line(format!("my {name} = {};", default_value(*local_ty)));
        }
        // Temps default to 0; every temp is assigned before any reachable read (valid wasm), so the value only guards against reading undef.
        let mut depths: Vec<u32> = func.temps.iter().map(|t| t.depth).collect();
        depths.dedup();
        for d in depths {
            w.line(format!("my $s{d} = 0;"));
        }
        if seq_has_br_table(&func.body) {
            w.line("my $_bt = 0;");
        }
        self.emit_seq(w, &func.body);
        w.line("return;");
        w.dedent();
        w.line("}");
    }

    /// Emit a statement sequence. Structured frames map directly onto perl's labeled blocks/loops (ADR-55): a referenced `Block` is `Ln: { ... }`, a referenced `Loop` is `Ln: while (1) { ...; last Ln; }` (a wasm loop label is a continue-target, and falling off the body exits), a referenced `If` gets a labeled block wrapper. Unreferenced labels need no frame at all — bodies are spliced inline.
    fn emit_seq(&self, w: &mut CodeWriter, stmts: &[Stmt]) {
        for stmt in stmts {
            match stmt {
                Stmt::Block { label, body } => {
                    if label.referenced {
                        w.line(format!("L{}: {{", label.id));
                        w.indent();
                        self.emit_seq(w, body);
                        w.dedent();
                        w.line("}");
                    } else {
                        self.emit_seq(w, body);
                    }
                }
                Stmt::Loop { label, body } => {
                    if label.referenced {
                        w.line(format!("L{}: while (1) {{", label.id));
                        w.indent();
                        self.emit_seq(w, body);
                        w.line(format!("last L{};", label.id));
                        w.dedent();
                        w.line("}");
                    } else {
                        // No br targets this loop, so it never repeats.
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
                        w.line(format!("L{}: {{", label.id));
                        w.indent();
                    }
                    w.line(format!("if (({}) != 0) {{", self.expr(cond)));
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
                    if label.referenced {
                        w.dedent();
                        w.line("}");
                    }
                }
                _ => self.simple_stmt(w, stmt),
            }
        }
    }

    fn simple_stmt(&self, w: &mut CodeWriter, stmt: &Stmt) {
        match stmt {
            Stmt::Assign { dst, expr } => {
                w.line(format!("{} = {};", temp(*dst), self.expr(expr)));
            }
            Stmt::LocalSet { idx, expr } => {
                w.line(format!("$l{idx} = {};", self.expr(expr)));
            }
            Stmt::GlobalSet { idx, expr } => {
                w.line(format!("$self->{{g{idx}}}{{value}} = {};", self.expr(expr)));
            }
            Stmt::Store {
                op,
                addr,
                value,
                offset,
            } => {
                w.line(format!(
                    "$self->{{memory}}->{}({}, {});",
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
                w.line(format!("$_bt = {};", self.expr(index)));
                for (n, target) in targets.iter().enumerate() {
                    let kw = if n == 0 { "if" } else { "} elsif" };
                    w.line(format!("{kw} ($_bt == {n}) {{"));
                    w.indent();
                    self.branch(w, target);
                    w.dedent();
                }
                w.line("} else {");
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
                self.use_unit("table/call");
                let mut call_args = vec![self.expr(index), self.type_symbol(*type_idx)];
                call_args.extend(args.iter().map(|a| self.expr(a)));
                let call = format!("$self->{{t{table_index}}}->call({})", call_args.join(", "));
                w.line(assign_results(results, call));
            }
            Stmt::MemoryGrow { dst, delta } => {
                self.use_unit("memory/grow");
                w.line(format!(
                    "{} = $self->{{memory}}->grow({});",
                    temp(*dst),
                    self.expr(delta)
                ));
            }
            Stmt::MemoryCopy { dst, src, len } => {
                self.use_unit("memory/copy");
                w.line(format!(
                    "$self->{{memory}}->copy({}, {}, {});",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::MemoryFill { dst, val, len } => {
                self.use_unit("memory/fill");
                w.line(format!(
                    "$self->{{memory}}->fill({}, {}, {});",
                    self.expr(dst),
                    self.expr(val),
                    self.expr(len)
                ));
            }
            Stmt::MemoryInit { seg, dst, src, len } => {
                self.use_unit("memory/init");
                w.line(format!(
                    "$self->{{memory}}->init({}, $self->{{data{seg}}}, {}, {});",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::DataDrop { seg } => {
                w.line(format!("$self->{{data{seg}}} = '';"));
            }
            Stmt::Unreachable => {
                w.line(format!("{}('unreachable');", self.rt("trap")));
            }
            Stmt::SourceLine(pos) => {
                // A source-position back-mapping comment (ADR-38); inert.
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
                    "$self->{{t{table_index}}}->init({}, $self->{{elem{seg}}}, {}, {});",
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
                    "$self->{{t{dst_table}}}->copy({}, $self->{{t{src_table}}}, {}, {});",
                    self.expr(dst),
                    self.expr(src),
                    self.expr(len)
                ));
            }
            Stmt::ElemDrop { seg } => {
                w.line(format!("$self->{{elem{seg}}} = [];"));
            }
            // Handled in emit_seq, never routed here.
            Stmt::Block { .. } | Stmt::Loop { .. } | Stmt::If { .. } => {
                unreachable!("structured statement routed to simple_stmt")
            }
        }
    }

    fn return_stmt(&self, w: &mut CodeWriter, values: &[Expr]) {
        match values {
            [] => w.line("return;"),
            [v] => w.line(format!("return {};", self.expr(v))),
            vs => {
                let vs = vs
                    .iter()
                    .map(|v| self.expr(v))
                    .collect::<Vec<_>>()
                    .join(", ");
                w.line(format!("return ({vs});"));
            }
        }
    }

    /// A branch is a direct `last`/`next` on the target frame's label (ADR-55): `last Ln` exits a block/if frame, `next Ln` re-enters a loop head. Branch operands travel through the frame's result temps via `assigns` first (self-assignments already filtered by the IR builder).
    fn branch(&self, w: &mut CodeWriter, target: &BrTarget) {
        match target {
            BrTarget::Return { values } => self.return_stmt(w, values),
            BrTarget::Label {
                label,
                is_loop,
                assigns,
            } => {
                for (dst, src) in assigns {
                    w.line(format!("{} = {};", temp(*dst), temp(*src)));
                }
                let kw = if *is_loop { "next" } else { "last" };
                w.line(format!("{kw} L{label};"));
            }
        }
    }

    /// Reference a Memory method, recording its unit.
    fn mem<'n>(&self, name: &'n str) -> &'n str {
        self.use_unit(&format!("memory/{name}"));
        name
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
                    perl_float(v as f64)
                } else {
                    format!("{}(0x{bits:x})", self.rt("f32_from_bits"))
                }
            }
            Expr::F64Const(bits) => {
                let v = f64::from_bits(*bits);
                if v.is_finite() {
                    perl_float(v)
                } else {
                    format!("{}(0x{bits:x})", self.rt("f64_from_bits"))
                }
            }
            Expr::Temp(t) => temp(*t),
            Expr::LocalGet(idx) => format!("$l{idx}"),
            Expr::GlobalGet(idx) => format!("$self->{{g{idx}}}{{value}}"),
            Expr::Un(op, a) => self.un(*op, &self.expr(a)),
            Expr::Bin(op, a, b) => self.bin(*op, &self.expr(a), &self.expr(b)),
            Expr::Load { op, addr, offset } => {
                format!(
                    "$self->{{memory}}->{}({})",
                    self.mem(load_method(*op)),
                    self.addr(addr, *offset)
                )
            }
            Expr::Select { cond, then, els } => {
                format!(
                    "(({}) != 0 ? {} : {})",
                    self.expr(cond),
                    self.expr(then),
                    self.expr(els)
                )
            }
            Expr::MemorySize => {
                self.use_unit("memory/size");
                "$self->{memory}->size()".to_string()
            }
        }
    }

    fn un(&self, op: UnOp, a: &str) -> String {
        use UnOp::*;
        match op {
            I32Eqz | I64Eqz => format!("(({a}) == 0 ? 1 : 0)"),
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
            I32WrapI64 => format!("(({a}) & 0xFFFFFFFF)"),
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
            F32ConvertI32S => format!("{}(0.0 + {}({a}))", self.rt("f32"), self.rt("s32")),
            F32ConvertI32U => format!("{}(0.0 + {a})", self.rt("f32")),
            F32ConvertI64S => format!("{}({}({a}))", self.rt("cvt_f32_i"), self.rt("s64")),
            F32ConvertI64U => format!("{}({a})", self.rt("cvt_f32_i")),
            F64ConvertI32S => format!("(0.0 + {}({a}))", self.rt("s32")),
            F64ConvertI32U => format!("(0.0 + {a})"),
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
            // i32 sums stay well inside the NV-exact range, so plain arithmetic plus a mask is exact; products are not (ADR-55), hence the helper.
            I32Add => format!("((({a}) + ({b})) & 0xFFFFFFFF)"),
            I32Sub => format!("((({a}) - ({b})) & 0xFFFFFFFF)"),
            I32Mul => format!("{}({a}, {b})", self.rt("i32_mul")),
            I64Add => format!("{}({a}, {b})", self.rt("i64_add")),
            I64Sub => format!("{}({a}, {b})", self.rt("i64_sub")),
            I64Mul => format!("{}({a}, {b})", self.rt("i64_mul")),
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
            I32Shl => format!("((({a}) << (({b}) & 31)) & 0xFFFFFFFF)"),
            I32ShrU => format!("(({a}) >> (({b}) & 31))"),
            I32ShrS => format!("{}({a}, {b})", self.rt("i32_shr_s")),
            // UV << wraps mod 2^64 (measured, ADR-55), so the mask suffices.
            I64Shl => format!("((({a}) << (({b}) & 63)) & 0xFFFFFFFFFFFFFFFF)"),
            I64ShrU => format!("(({a}) >> (({b}) & 63))"),
            I64ShrS => format!("{}({a}, {b})", self.rt("i64_shr_s")),
            I32Rotl => format!("{}({a}, {b})", self.rt("i32_rotl")),
            I32Rotr => format!("{}({a}, {b})", self.rt("i32_rotr")),
            I64Rotl => format!("{}({a}, {b})", self.rt("i64_rotl")),
            I64Rotr => format!("{}({a}, {b})", self.rt("i64_rotr")),
            // Unsigned comparison: both operands are masked-unsigned IV/UVs, which perl's comparison operators order exactly (measured, ADR-55).
            I32Eq | I64Eq => format!("(({a}) == ({b}) ? 1 : 0)"),
            I32Ne | I64Ne => format!("(({a}) != ({b}) ? 1 : 0)"),
            I32LtU | I64LtU => format!("(({a}) < ({b}) ? 1 : 0)"),
            I32GtU | I64GtU => format!("(({a}) > ({b}) ? 1 : 0)"),
            I32LeU | I64LeU => format!("(({a}) <= ({b}) ? 1 : 0)"),
            I32GeU | I64GeU => format!("(({a}) >= ({b}) ? 1 : 0)"),
            I32LtS => format!("({0}({a}) < {0}({b}) ? 1 : 0)", self.rt("s32")),
            I32GtS => format!("({0}({a}) > {0}({b}) ? 1 : 0)", self.rt("s32")),
            I32LeS => format!("({0}({a}) <= {0}({b}) ? 1 : 0)", self.rt("s32")),
            I32GeS => format!("({0}({a}) >= {0}({b}) ? 1 : 0)", self.rt("s32")),
            I64LtS => format!("({0}({a}) < {0}({b}) ? 1 : 0)", self.rt("s64")),
            I64GtS => format!("({0}({a}) > {0}({b}) ? 1 : 0)", self.rt("s64")),
            I64LeS => format!("({0}({a}) <= {0}({b}) ? 1 : 0)", self.rt("s64")),
            I64GeS => format!("({0}({a}) >= {0}({b}) ? 1 : 0)", self.rt("s64")),
            // Float add/sub/mul go through helpers because perl's arithmetic operators take an integer fast path that keeps integer exactness beyond double precision and drops -0.0 (measured, ADR-55).
            F32Add => format!("{}({}({a}, {b}))", self.rt("f32"), self.rt("fadd")),
            F32Sub => format!("{}({}({a}, {b}))", self.rt("f32"), self.rt("fsub")),
            F32Mul => format!("{}({}({a}, {b}))", self.rt("f32"), self.rt("fmul")),
            F32Div => format!("{}({}({a}, {b}))", self.rt("f32"), self.rt("fdiv")),
            F64Add => format!("{}({a}, {b})", self.rt("fadd")),
            F64Sub => format!("{}({a}, {b})", self.rt("fsub")),
            F64Mul => format!("{}({a}, {b})", self.rt("fmul")),
            F64Div => format!("{}({a}, {b})", self.rt("fdiv")),
            F32Min | F64Min => format!("{}({a}, {b})", self.rt("fmin")),
            F32Max | F64Max => format!("{}({a}, {b})", self.rt("fmax")),
            F32Copysign => format!("{}({a}, {b})", self.rt("f32_copysign")),
            F64Copysign => format!("{}({a}, {b})", self.rt("f64_copysign")),
            F32Eq | F64Eq => format!("(({a}) == ({b}) ? 1 : 0)"),
            F32Ne | F64Ne => format!("(({a}) != ({b}) ? 1 : 0)"),
            F32Lt | F64Lt => format!("(({a}) < ({b}) ? 1 : 0)"),
            F32Gt | F64Gt => format!("(({a}) > ({b}) ? 1 : 0)"),
            F32Le | F64Le => format!("(({a}) <= ({b}) ? 1 : 0)"),
            F32Ge | F64Ge => format!("(({a}) >= ({b}) ? 1 : 0)"),
        }
    }
}

/// A Perl float literal that round-trips to the same double. `{:?}` on f64 gives the shortest round-tripping decimal, which perl's (correctly rounded) string->NV conversion parses back exactly; non-finite values never reach here.
fn perl_float(v: f64) -> String {
    format!("{v:?}")
}

fn assign_results(results: &[Temp], call: String) -> String {
    match results {
        [] => format!("{call};"),
        [r] => format!("{} = {call};", temp(*r)),
        rs => {
            let names = rs.iter().map(|r| temp(*r)).collect::<Vec<_>>().join(", ");
            format!("({names}) = {call};")
        }
    }
}

/// Whether `stmts` contains a `br_table` (which needs the per-function `$_bt` selector variable declared).
fn seq_has_br_table(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::BrTable { .. } => true,
        Stmt::Block { body, .. } | Stmt::Loop { body, .. } => seq_has_br_table(body),
        Stmt::If { then, els, .. } => seq_has_br_table(then) || seq_has_br_table(els),
        _ => false,
    })
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

/// Codegen-shape test for the label lowering (ADR-55): multi-level branches must come out as direct `last`/`next` on perl labels, with no flag-variable cascade (Ruby's `__br`, ADR-42) sneaking back in.
#[cfg(test)]
mod branch_shape {
    use super::*;

    fn generate(wat: &str) -> String {
        let bytes = wat::parse_str(wat).expect("valid wat");
        let module = dewasm_core::build_module(&bytes).expect("buildable module");
        let (source, _) = generate_package_with_units(
            &module,
            "Shape",
            &RuntimeLinkage::Alias("Rt".to_string()),
            false,
        )
        .expect("generates");
        source
    }

    #[test]
    fn multi_level_br_uses_labels() {
        let source = generate(
            r#"(module
              (func (export "f") (param i32) (result i32)
                (local i32)
                (block $outer
                  (block $inner
                    (loop $l
                      (br_if $outer (i32.eq (local.get 0) (i32.const 1)))
                      (br_if $inner (i32.eq (local.get 0) (i32.const 2)))
                      (local.set 0 (i32.sub (local.get 0) (i32.const 1)))
                      (br $l)))
                  (local.set 1 (i32.const 7)))
                (local.get 1)))"#,
        );
        // The loop is a labeled `while (1)`; both blocks are labeled bare blocks.
        assert!(source.contains(": while (1) {"), "loop frame:\n{source}");
        // Branches are direct label exits/continues: the back-edge is a `next`, the block exits are `last`s at two different depths.
        assert!(source.contains("next L"), "loop back-edge:\n{source}");
        let lasts = source.matches("last L").count();
        assert!(lasts >= 3, "multi-level exits (got {lasts}):\n{source}");
        // No flag-variable cascade (ADR-42/ADR-28 schemes must not reappear).
        assert!(!source.contains("__br"), "flag cascade leaked:\n{source}");
        assert!(!source.contains("$_br"), "flag register leaked:\n{source}");
    }

    #[test]
    fn br_table_dispatches_to_labels() {
        let source = generate(
            r#"(module
              (func (export "f") (param i32) (result i32)
                (block $a
                  (block $b
                    (block $c
                      (br_table $a $b $c (local.get 0)))
                    (return (i32.const 2)))
                  (return (i32.const 1)))
                (i32.const 0)))"#,
        );
        assert!(source.contains("$_bt == 0"), "dispatch:\n{source}");
        assert!(source.matches("last L").count() >= 3, "targets:\n{source}");
    }
}

/// Lint for the runtime units: every reference a unit body makes to another unit must be declared in its `# requires:` header. Mirrors the Ruby/Python units lint (ADR-6), adjusted for Perl syntax (`Rt::name(...)` calls, `'Rt::Class'` names, `$self->name(...)` sibling calls within a scope's package).
#[cfg(test)]
mod units {
    use super::*;
    use std::collections::BTreeSet;
    use std::io::Write as _;

    use regex::Regex;

    #[test]
    fn all_units_bundle() {
        bundler().bundle_all(0).expect("full bundle resolves");
    }

    /// The whole runtime bundle must be valid perl: `perl -c` the full bundle (the interpreted-language analog of the compiled backends' compile check). Fail-loud on a missing perl (ADR-15).
    #[test]
    fn full_bundle_is_valid_perl() {
        let perl = find_perl()
            .expect("perl >= 5.26 with 64-bit IVs/NVs not found on PATH — see docs/testing.md");
        let bundle = format!(
            "{}package main;\n1;\n",
            bundler().bundle_all(0).expect("full bundle resolves")
        );
        let dir = std::env::temp_dir();
        let path = dir.join(format!("dewasm-perl-bundle-{}.pl", std::process::id()));
        let mut f = std::fs::File::create(&path).expect("write temp bundle");
        f.write_all(bundle.as_bytes()).expect("write temp bundle");
        drop(f);
        let out = std::process::Command::new(&perl)
            .arg("-c")
            .arg(&path)
            .output()
            .expect("run perl -c");
        let _ = std::fs::remove_file(&path);
        assert!(
            out.status.success(),
            "perl -c rejected the runtime bundle:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn declared_requires_cover_references() {
        let b = bundler();
        let unit_ids: BTreeSet<&str> = b.units().map(|u| u.id.as_str()).collect();

        // `Rt::name(` function calls and `Rt::Name` class references (in strings or arrow calls).
        let rt_ref = Regex::new(r"Rt::([A-Za-z_]\w*)").unwrap();
        // One precompiled sibling-call matcher per unit name (`$self->name(`).
        let sibling_calls: Vec<(&str, Regex)> = unit_ids
            .iter()
            .map(|id| {
                let name = id.split('/').nth(1).unwrap();
                let re = Regex::new(&format!(r"\$self->{}\(", regex::escape(name))).unwrap();
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
                // Scope preludes are implicit.
                if dep.ends_with("/_package") {
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

            for cap in rt_ref.captures_iter(&code) {
                let name = &cap[1];
                let dep = if name.chars().next().unwrap().is_lowercase() {
                    format!("rt/{name}")
                } else {
                    match name {
                        "Trap" => "rt/trap".to_string(),
                        "Exit" => "rt/exit".to_string(),
                        "LinkError" => "rt/link_error".to_string(),
                        "Memory" => "memory/_package".to_string(),
                        "Table" => "table/_package".to_string(),
                        "Global" => "global/_package".to_string(),
                        "WASI" => continue,
                        // Root-prelude package vars, always available.
                        "DEPTH" | "LIMIT" => continue,
                        other => panic!("{}: unknown runtime reference Rt::{other}", unit.id),
                    }
                };
                demand(dep, &format!("Rt::{name}"));
            }

            // Sibling calls within the same scope's package.
            for (sibling, re) in &sibling_calls {
                let Some(name) = sibling.strip_prefix(&format!("{scope}/")) else {
                    continue;
                };
                if name.starts_with('_') || *sibling == unit.id {
                    continue;
                }
                if re.is_match(&code) {
                    demand(sibling.to_string(), &format!("$self->{name}(...)"));
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
