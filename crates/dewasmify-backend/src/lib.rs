//! Backend trait and code emission utilities shared by all language
//! backends.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use anyhow::{bail, Result};
use dewasmify_core::feature::Feature;
use dewasmify_core::ir;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SupportStatus {
    Supported,
    Partial(&'static str),
    Unsupported,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Emit a module that is instantiated with an imports object and exposes
    /// its exports to the host language.
    Library,
    /// Emit a runnable program that wires up WASI and calls `_start`.
    Standalone,
}

/// How generated code gets its runtime. Generated code always refers to
/// the runtime by the relative name `Rt` (or the backend's equivalent);
/// linkage only decides where that name is defined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeLinkage {
    /// Nest the needed runtime units inside the generated module itself
    /// (single self-contained file; multiple generated artifacts never
    /// collide).
    Embedded,
    /// Emit only an alias to a runtime defined elsewhere (a shared bundle
    /// in the same program, or a future runtime package/gem).
    Alias(String),
}

#[derive(Clone, Debug)]
pub struct GenOptions {
    pub mode: Mode,
    /// Class/package/module name for the generated code.
    pub module_name: String,
    pub runtime: RuntimeLinkage,
    /// Bundle the built-in WASI implementation as a fallback for
    /// `wasi_snapshot_preview1` imports the embedder does not provide.
    /// Disable to keep generated libraries free of ambient authority.
    pub default_wasi: bool,
}

pub struct OutputFile {
    pub name: String,
    pub contents: String,
}

pub trait Backend {
    fn name(&self) -> &str;
    fn file_extension(&self) -> &str;
    fn generate(&self, module: &ir::Module, opts: &GenOptions) -> anyhow::Result<Vec<OutputFile>>;

    /// The input scope every backend targets before any `Feature` rows.
    fn baseline(&self) -> &'static str {
        "Wasm core 1.0 (minus the wasm 1.0 gaps listed below) + mutable globals, \
         sign-extension, saturating float-to-int, multi-value, and the memory half \
         of bulk memory (memory.copy/fill/init, passive data segments)"
    }

    /// Declared support level per feature (ADR-8). The spec harness only
    /// tolerates skips attributable to features that are not `Supported`;
    /// flipping a feature to `Supported` makes its skips hard failures.
    fn feature_status(&self, feature: Feature) -> SupportStatus {
        let _ = feature;
        SupportStatus::Unsupported
    }
}

/// The full WASI preview 1 surface, for the generated support docs; which
/// of these a backend implements is derived from its runtime units
/// (`bundler().has_unit("wasi/<name>")`).
pub const WASI_PREVIEW1_FUNCTIONS: &[&str] = &[
    "args_get", "args_sizes_get", "environ_get", "environ_sizes_get",
    "clock_res_get", "clock_time_get",
    "fd_advise", "fd_allocate", "fd_close", "fd_datasync", "fd_fdstat_get",
    "fd_fdstat_set_flags", "fd_fdstat_set_rights", "fd_filestat_get",
    "fd_filestat_set_size", "fd_filestat_set_times", "fd_pread",
    "fd_prestat_get", "fd_prestat_dir_name", "fd_pwrite", "fd_read",
    "fd_readdir", "fd_renumber", "fd_seek", "fd_sync", "fd_tell", "fd_write",
    "path_create_directory", "path_filestat_get", "path_filestat_set_times",
    "path_link", "path_open", "path_readlink", "path_remove_directory",
    "path_rename", "path_symlink", "path_unlink_file",
    "poll_oneoff", "proc_exit", "proc_raise", "random_get", "sched_yield",
    "sock_accept", "sock_recv", "sock_send", "sock_shutdown",
];

/// One runtime unit: a single method (or an inseparable scope prelude),
/// with its dependencies declared in `<comment> requires:` header lines.
pub struct RuntimeUnit {
    pub id: String,
    pub requires: Vec<String>,
    pub body: String,
}

/// A named scope units can live in (e.g. a class nested in the runtime
/// module). `prefix` is the unit-id path segment; `open`/`close` wrap the
/// scope's units; the root scope uses empty wrappers.
pub struct RuntimeScope {
    pub prefix: &'static str,
    pub open: &'static str,
    pub close: &'static str,
    /// Unit implicitly required by every unit of this scope (class
    /// skeleton, constants); also force-included for the root scope.
    pub prelude: Option<&'static str>,
}

/// Resolves `requires:` closures over runtime units and emits the bundle,
/// grouped by scope in declaration order, deterministically sorted within
/// a scope. Language-agnostic: syntax comes from the scopes and the
/// caller-provided wrapper around the whole bundle.
pub struct RuntimeBundler {
    scopes: Vec<RuntimeScope>,
    units: BTreeMap<String, RuntimeUnit>,
    indent_str: &'static str,
}

impl RuntimeBundler {
    pub fn new(
        comment_prefix: &str,
        indent_str: &'static str,
        scopes: Vec<RuntimeScope>,
        sources: &[(&str, &str)],
    ) -> Result<Self> {
        let requires_marker = format!("{comment_prefix} requires:");
        let mut units = BTreeMap::new();
        for (id, source) in sources {
            let mut requires = Vec::new();
            let mut body_lines = Vec::new();
            let mut in_header = true;
            for line in source.lines() {
                if in_header {
                    if let Some(rest) = line.strip_prefix(&requires_marker) {
                        for dep in rest.split(',') {
                            let dep = dep.trim();
                            if !dep.is_empty() {
                                requires.push(dep.to_string());
                            }
                        }
                        continue;
                    }
                    if line.trim().is_empty() {
                        continue;
                    }
                    in_header = false;
                }
                body_lines.push(line);
            }
            while body_lines.last().is_some_and(|l| l.trim().is_empty()) {
                body_lines.pop();
            }
            let unit = RuntimeUnit {
                id: id.to_string(),
                requires,
                body: body_lines.join("\n"),
            };
            if units.insert(unit.id.clone(), unit).is_some() {
                bail!("duplicate runtime unit {id}");
            }
        }
        let bundler = RuntimeBundler { scopes, units, indent_str };
        for unit in bundler.units.values() {
            for dep in &unit.requires {
                if !bundler.units.contains_key(dep) {
                    bail!("unit {} requires unknown unit {dep}", unit.id);
                }
            }
            bundler.scope_of(&unit.id)?;
        }
        Ok(bundler)
    }

    fn scope_of(&self, id: &str) -> Result<&RuntimeScope> {
        let prefix = id.split('/').next().unwrap_or("");
        self.scopes
            .iter()
            .find(|s| s.prefix == prefix)
            .ok_or_else(|| anyhow::anyhow!("unit {id} has unknown scope {prefix}"))
    }

    pub fn has_unit(&self, id: &str) -> bool {
        self.units.contains_key(id)
    }

    pub fn units(&self) -> impl Iterator<Item = &RuntimeUnit> {
        self.units.values()
    }

    /// Compute the dependency closure of `seeds`, including scope preludes
    /// and the root scope's prelude.
    pub fn closure(&self, seeds: &BTreeSet<String>) -> Result<BTreeSet<String>> {
        let mut closure = BTreeSet::new();
        let mut queue: VecDeque<String> = seeds.iter().cloned().collect();
        if let Some(root_prelude) = self.scopes.first().and_then(|s| s.prelude) {
            queue.push_back(root_prelude.to_string());
        }
        while let Some(id) = queue.pop_front() {
            let unit = self
                .units
                .get(&id)
                .ok_or_else(|| anyhow::anyhow!("unknown runtime unit {id}"))?;
            if !closure.insert(id.clone()) {
                continue;
            }
            for dep in &unit.requires {
                queue.push_back(dep.clone());
            }
            if let Some(prelude) = self.scope_of(&id)?.prelude {
                queue.push_back(prelude.to_string());
            }
        }
        Ok(closure)
    }

    /// Emit the bundle for `seeds`' closure. `base_indent` is the indent
    /// level of the bundle's root-scope members (the caller wraps the
    /// result in the runtime module/namespace itself).
    pub fn bundle(&self, seeds: &BTreeSet<String>, base_indent: usize) -> Result<String> {
        let closure = self.closure(seeds)?;
        let mut out = String::new();
        for scope in &self.scopes {
            let mut ids: Vec<&String> = closure
                .iter()
                .filter(|id| id.split('/').next() == Some(scope.prefix))
                .collect();
            if ids.is_empty() {
                continue;
            }
            // Prelude first, the rest in sorted order.
            ids.sort_by_key(|id| (Some(id.as_str()) != scope.prelude.map(|p| p), id.as_str()));
            let (open, body_indent) = if scope.open.is_empty() {
                ("", base_indent)
            } else {
                (scope.open, base_indent + 1)
            };
            if !open.is_empty() {
                if !out.is_empty() {
                    out.push('\n');
                }
                self.push_line(&mut out, base_indent, open);
            }
            let mut first = open.is_empty() && out.is_empty();
            for id in ids {
                if !first {
                    out.push('\n');
                }
                first = false;
                for line in self.units[id.as_str()].body.lines() {
                    self.push_line(&mut out, body_indent, line);
                }
            }
            if !scope.close.is_empty() {
                self.push_line(&mut out, base_indent, scope.close);
            }
        }
        Ok(out)
    }

    pub fn bundle_all(&self, base_indent: usize) -> Result<String> {
        let seeds: BTreeSet<String> = self.units.keys().cloned().collect();
        self.bundle(&seeds, base_indent)
    }

    fn push_line(&self, out: &mut String, indent: usize, line: &str) {
        if line.trim().is_empty() {
            out.push('\n');
            return;
        }
        for _ in 0..indent {
            out.push_str(self.indent_str);
        }
        out.push_str(line);
        out.push('\n');
    }
}

/// Indentation-aware line writer.
pub struct CodeWriter {
    buf: String,
    indent: usize,
    indent_str: &'static str,
}

impl CodeWriter {
    pub fn new(indent_str: &'static str) -> Self {
        CodeWriter { buf: String::new(), indent: 0, indent_str }
    }

    pub fn line(&mut self, s: impl AsRef<str>) {
        let s = s.as_ref();
        if s.is_empty() {
            self.buf.push('\n');
            return;
        }
        for _ in 0..self.indent {
            self.buf.push_str(self.indent_str);
        }
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    pub fn raw(&mut self, s: impl AsRef<str>) {
        self.buf.push_str(s.as_ref());
    }

    pub fn indent(&mut self) {
        self.indent += 1;
    }

    pub fn dedent(&mut self) {
        self.indent -= 1;
    }

    /// line(open); indent(); f(); dedent(); line(close)
    pub fn block(
        &mut self,
        open: impl AsRef<str>,
        close: impl AsRef<str>,
        f: impl FnOnce(&mut Self),
    ) {
        self.line(open);
        self.indent();
        f(self);
        self.dedent();
        self.line(close);
    }

    pub fn finish(self) -> String {
        self.buf
    }
}
