//! Specification test harness: parses every `.wast` file of the testsuite submodule.
//! The submodule is `tests/spec`, tracking upstream latest.
//! It translates each module with a target-language backend.
//! It generates a script that runs all assertions.
//! It executes that script with the real interpreter for that language.
//! The language-specific pieces live behind the `SpecBackend` trait.
//! Each backend crate implements that trait.
//! Directive iteration, skip attribution, and result accounting are shared here.
//!
//! Skips must be *attributable*.
//! A module that fails to convert carries an `UnsupportedError`.
//! The error names the declared-unsupported features.
//! Every directive skipped because of it is counted under those feature identifiers.
//! A conversion failure without attribution is a dewasm bug and fails the suite.
//! Validation failures beyond every proposal this toolchain knows are allowed.
//! They are reported as `unknown-proposal`, because the converter refused cleanly.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use dewasm_backend::SupportStatus;
use dewasm_core::feature::{Feature, UnsupportedError};
use dewasm_core::ir;
use libtest_mimic::{Failed, Trial};
use wast::core::{AbstractHeapType, HeapType};
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastRet, Wat};

use crate::backend::BackendUnderTest;

/// The script-phrasing layer of a backend under test.
/// It says how to convert a module and how to phrase each assertion in that language.
/// It also says how to bundle the runtime.
/// `name`/`backend`/`interpreter`/`run` come from the base [`BackendUnderTest`] trait.
/// `emit_*` methods append to the script body.
/// Returning `Err(tag)` skips the directive under that attribution tag.
pub trait SpecBackend: BackendUnderTest {
    /// Known assertion-level failures: (file, count, attribution tag).
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)];
    /// The non-ignored set: files run by a plain `cargo test`.
    /// Every other `.wast` file still becomes a trial, but marked `#[ignore]`d.
    /// `None` marks nothing ignored: the whole testsuite runs by default.
    /// The fast interpreters use that.
    fn curated_files(&self) -> Option<&'static [&'static str]>;
    /// Units the harness helpers themselves use.
    fn seed_units(&self) -> &'static [&'static str];
    /// Lower an IR module to source.
    /// Backend-level refusals carry `UnsupportedError`, which sits in the error chain.
    /// An example is floats on an integer-only backend.
    fn generate(&self, module: &ir::Module, counter: u32) -> anyhow::Result<Converted>;
    /// Whether `register`-directive linking is supported for this language.
    /// If so, a `(register "Name" $id)`'d instance becomes resolvable as an import source.
    /// Later modules can import from it (`registered` below).
    /// Also, `assert_unlinkable` is then checked for real instead of always skipped.
    fn supports_registered_imports(&self) -> bool {
        false
    }
    /// Emit the module source plus its instantiation.
    /// Return the variable (or prefix) later calls use to reach the instance.
    /// `registered` is the `(name, instance-expr)` pairs of currently `register`ed modules.
    /// Only those with a live instance count, for languages that support them.
    /// `decls` is the file-scoped declaration buffer.
    /// Go and Java must hoist a converted module's definitions to package/class scope.
    /// They push `conv.source` there rather than into `script`.
    /// It is owned by the per-file harness state, so parallel trials never share it.
    fn emit_instantiate(
        &self,
        script: &mut String,
        decls: &mut String,
        conv: &Converted,
        var_id: u32,
        registered: &[(String, String)],
    ) -> String;
    /// Emit the module source only.
    /// Return the call that performs the (possibly trapping) instantiation.
    /// That call serves `assert_trap` on a module.
    /// See [`Self::emit_instantiate`] for `decls`.
    fn instantiate_call(
        &self,
        script: &mut String,
        decls: &mut String,
        conv: &Converted,
        registered: &[(String, String)],
    ) -> String;
    fn invoke(&self, var: &str, name: &str, args: &[WastArg<'_>]) -> Result<String, String>;
    fn global_get(&self, var: &str, global: &str) -> String;
    fn emit_check(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
        results: &[WastRet<'_>],
    ) -> Result<(), String>;
    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str);
    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str);
    /// Emit an `assert_exception` check: `call` must raise an (uncaught) wasm exception.
    /// The default keeps the directive an attributed skip.
    /// That suits backends that don't declare exception handling supported.
    fn emit_check_exception(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
    ) -> Result<(), String> {
        let _ = (script, desc, call);
        Err("exception-handling".to_string())
    }
    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str);
    /// Emit an `assert_unlinkable` check: `call` (the instantiation) must raise/fail.
    /// Only called when `supports_registered_imports()` is true.
    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = (script, desc, call);
        unreachable!("only called when supports_registered_imports() is true")
    }
    /// Wrap the accumulated body into a runnable script.
    /// The script holds the shared runtime for `units`, harness helpers, and hoisted `decls`.
    /// It then holds the body and a result-line footer.
    /// See [`Self::emit_instantiate`] for `decls`.
    /// Backends that inline module definitions into `script` receive an empty `decls`.
    fn assemble(&self, units: &BTreeSet<String>, decls: &str, body: &str)
        -> anyhow::Result<String>;
}

/// A converted module: its source text and the language-specific handle used to instantiate it.
/// The handle is a class name, a function prefix, and so on.
/// It also records the runtime units the module references.
pub struct Converted {
    pub source: String,
    pub handle: String,
    pub units: BTreeSet<String>,
}

/// The `.wast` files a plain `cargo test` runs for a backend with a high per-file cost.
/// For such a backend the whole 257-file testsuite is too slow to be the default.
/// There is one file per semantic area.
/// The areas: integers, floats, control flow, memory/table, globals, linking, and bulk operations.
/// Every other file stays a trial, `#[ignore]`d by default.
/// It runs when the backend crate's `slow_test` feature is on.
/// The selection is a property of the testsuite rather than of a target language.
/// So every backend with a selected list shares it.
/// One that needs more files adds them with [`curated_with`].
pub const CURATED_SPEC_FILES: &[&str] = &[
    "address",
    "align",
    "block",
    "br",
    "br_if",
    "br_table",
    "bulk",
    "call",
    "call_indirect",
    "comments",
    "const",
    "conversions",
    "custom",
    "data",
    "elem",
    "endianness",
    "f32",
    "f32_bitwise",
    "f32_cmp",
    "f64",
    "f64_bitwise",
    "f64_cmp",
    "fac",
    "float_exprs",
    "float_literals",
    "float_memory",
    "float_misc",
    "forward",
    "func",
    "func_ptrs",
    "global",
    "i32",
    "i64",
    "if",
    "imports",
    "imports2",
    "int_exprs",
    "int_literals",
    "labels",
    "left-to-right",
    "linking",
    "linking0",
    "load",
    "load1",
    "local_get",
    "local_set",
    "local_tee",
    "loop",
    "memory",
    "memory_copy",
    "memory_fill",
    "memory_grow",
    "memory_init",
    "memory_redundancy",
    "memory_size",
    "memory_trap",
    "names",
    "nop",
    "return",
    "select",
    "stack",
    "start",
    "store",
    "switch",
    "table",
    "table_copy",
    "table_init",
    "token",
    "traps",
    "type",
    "unreachable",
    "unreached-invalid",
    "unreached-valid",
    "unwind",
    "utf8-custom-section-id",
    "utf8-import-field",
    "utf8-import-module",
    "utf8-invalid-encoding",
];

/// The exception-handling testsuite files.
/// They are the shared `extra` for every backend that declares the feature and has a selected list.
/// One list rather than four copies so the set cannot drift per backend.
pub const EXCEPTION_HANDLING_SPEC_FILES: &[&str] = &["try_table", "throw", "throw_ref", "tag"];

/// The tail-call testsuite files.
/// They are the shared `extra` for every backend that declares the feature and has a selected list.
/// `return_call_ref` is not among them.
/// It belongs to the function-references proposal, which stays rejected.
pub const TAIL_CALL_SPEC_FILES: &[&str] = &["return_call", "return_call_indirect"];

/// [`CURATED_SPEC_FILES`] plus every slice in `extras`.
/// So a backend adds its own files to the shared lists ([`EXCEPTION_HANDLING_SPEC_FILES`]).
/// It copies neither.
/// It goes through `Vec::leak`: [`SpecBackend::curated_files`] hands back a `'static` slice.
/// Each backend calls this once per suite run, when its trials are built.
pub fn curated_with(extras: &[&[&'static str]]) -> &'static [&'static str] {
    let mut files = CURATED_SPEC_FILES.to_vec();
    for extra in extras {
        files.extend_from_slice(extra);
    }
    Vec::leak(files)
}

/// The attribution tag for a heap type no backend can express as a host value.
/// The tag is the wasm proposal the type belongs to.
/// So the skipped directive is counted against the *feature* rather than against the backend.
/// Classifies the `wast` type only: no target language enters into it.
/// That is why every backend shares one copy.
pub fn heap_type_tag(hty: &HeapType<'_>) -> String {
    match hty {
        HeapType::Abstract {
            ty: AbstractHeapType::Exn | AbstractHeapType::NoExn,
            ..
        } => "exception-handling".to_string(),
        HeapType::Abstract { .. } => "gc".to_string(),
        HeapType::Concrete(_) | HeapType::Exact(_) => "function-references".to_string(),
    }
}

/// Whether `ref.null <hty>` maps to the host language's own null.
/// The two reference-types hierarchies and their bottoms are.
/// All of them are just the host's `nil`/`None`/`undef`.
/// Anything else is not, and is skipped under [`heap_type_tag`].
pub fn nullable_heap_type(hty: &HeapType<'_>) -> bool {
    matches!(
        hty,
        HeapType::Abstract {
            ty: AbstractHeapType::Func
                | AbstractHeapType::Extern
                | AbstractHeapType::Exn
                | AbstractHeapType::NoFunc
                | AbstractHeapType::NoExtern
                | AbstractHeapType::NoExn
                | AbstractHeapType::None,
            ..
        }
    )
}

fn spec_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec")
}

/// Build one `libtest-mimic` [`Trial`] per `.wast` file of the testsuite for `lang`.
/// This is the `spec_suite!` macro's entry point.
/// The trial name is the file stem, so Cargo's own name filter applies.
/// For example, `cargo test --test spec i32` runs the `i32`-named file(s).
/// Files outside the backend's [`SpecBackend::curated_files`] set become `#[ignore]`d trials.
/// So a plain `cargo test` runs the `curated_files` set.
/// Trials run on the `libtest-mimic` thread pool.
/// Each owns its per-file state, so the files run in parallel.
///
/// `slow_test` mirrors the backend crate's feature of the same name (CI's main run).
/// When it is on, nothing is marked ignored, and the whole testsuite runs.
/// That is the same set the old `--include-ignored` run covered.
pub fn spec_trials(lang: &'static dyn SpecBackend, slow_test: bool) -> Vec<Trial> {
    let dir = spec_dir();
    assert!(
        dir.exists(),
        "tests/spec not found: run `git submodule update --init` (see docs/testing.md)"
    );

    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("read tests/spec")
        .filter_map(|e| {
            let path = e.ok()?.path();
            if path.extension().and_then(|x| x.to_str()) == Some("wast") {
                Some(path.file_stem()?.to_str()?.to_string())
            } else {
                None
            }
        })
        .collect();
    names.sort();

    let curated: Option<BTreeSet<&'static str>> = if slow_test {
        None
    } else {
        lang.curated_files().map(|c| c.iter().copied().collect())
    };

    names
        .into_iter()
        .map(|name| {
            let ignored = curated
                .as_ref()
                .is_some_and(|set| !set.contains(name.as_str()));
            let path = dir.join(format!("{name}.wast"));
            Trial::test(name.clone(), move || run_trial(lang, &name, &path))
                .with_ignored_flag(ignored)
        })
        .collect()
}

/// `harness=false` entry point: parse Cargo's test arguments and run the trials.
/// The arguments are the name filter, `--ignored`/`--include-ignored`, the thread count, and so on.
pub fn spec_main(lang: &'static dyn SpecBackend, slow_test: bool) {
    let args = libtest_mimic::Arguments::from_args();
    libtest_mimic::run(&args, spec_trials(lang, slow_test)).exit();
}

/// Run one `.wast` file and apply the per-file checks:
///
/// * the assertion-failure count must equal the backend's `EXPECTED_FAILURES` entry (0 if missing);
/// * an unattributed conversion failure is a dewasm bug;
/// * a skip attributed to a feature the backend declares `Supported` is a declaration regression.
///
/// A passing trial stays quiet; failures carry the per-file summary and detail.
fn run_trial(lang: &dyn SpecBackend, name: &str, path: &Path) -> Result<(), Failed> {
    let stats = run_file(lang, name, path).map_err(|err| format!("{name}: {err:#}"))?;

    let mut failures: Vec<String> = stats.hard_errors.clone();

    let expected = lang
        .expected_failures()
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, count, _)| *count)
        .unwrap_or(0);
    if stats.fail != expected {
        failures.push(format!(
            "{} assertion failures (expected {expected})",
            stats.fail
        ));
        failures.extend(stats.fail_lines.iter().cloned());
    }

    // A skip is only legitimate while its feature is declared unsupported.
    // Once the backend flips a feature to Supported, remaining skips are declaration regressions.
    for (tag, count) in &stats.unsupported {
        let ids: Vec<&str> = tag.split('+').collect();
        let all_supported = ids.iter().all(|id| {
            Feature::from_id(id)
                .map(|f| lang.backend().feature_status(f) == SupportStatus::Supported)
                .unwrap_or(false)
        });
        if all_supported {
            failures.push(format!(
                "{count} directives skipped for {tag}, but the backend declares it supported"
            ));
        }
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(Failed::from(format!(
            "{name}: pass={} fail={} skip={} (rust: invalid-ok={} invalid-bad={})\n{}",
            stats.pass,
            stats.fail,
            stats.skipped(),
            stats.rust_pass,
            stats.rust_fail,
            failures.join("\n"),
        )))
    }
}

#[derive(Default)]
struct Stats {
    pass: u32,
    fail: u32,
    /// Skipped directives, attributed to declared-unsupported feature identifiers.
    /// Harness-level tags like "linking" and "unknown-proposal" count too.
    unsupported: BTreeMap<String, u32>,
    /// Unattributed conversion errors: dewasm bugs, fail the suite.
    hard_errors: Vec<String>,
    /// `assert_invalid` / `assert_malformed` handled on the Rust side
    rust_pass: u32,
    rust_fail: u32,
    /// `FAIL...` lines from the generated script's `stdout`.
    /// They surface in the trial's failure message, only when the file's count breaks the list.
    fail_lines: Vec<String>,
}

impl Stats {
    fn skipped(&self) -> u32 {
        self.unsupported.values().sum()
    }
}

struct ScriptGen<'a> {
    lang: &'a dyn SpecBackend,
    script: String,
    /// File-scoped declaration buffer hoisted ahead of `script` by `assemble`.
    /// See [`SpecBackend::emit_instantiate`].
    /// Owned per file, so parallel trials never share it.
    decls: String,
    source: &'a str,
    file: &'a str,
    /// Variable/prefix holding the most recent instance.
    /// It holds the attribution tag instead when the most recent module failed to convert.
    current: Result<String, String>,
    /// Same, for named modules.
    named: std::collections::HashMap<String, Result<String, String>>,
    /// Same, keyed by the *registered* name from `(register "Name" $id)`.
    /// That is a different namespace from the `.wast` `$id`s of `named`.
    /// It is what a later module's `(import "Name" ...)` actually resolves against.
    registered: std::collections::HashMap<String, Result<String, String>>,
    counter: u32,
    converted: u32,
    stats: Stats,
    /// Union of the runtime units needed by all converted modules.
    units: BTreeSet<String>,
}

impl<'a> ScriptGen<'a> {
    fn desc(&self, span: wast::token::Span) -> String {
        let (line, _) = span.linecol_in(self.source);
        format!("{}.wast:{}", self.file, line + 1)
    }

    fn skip(&mut self, tag: &str) {
        *self.stats.unsupported.entry(tag.to_string()).or_default() += 1;
    }

    fn instance_for(&self, module: Option<&str>) -> Result<String, String> {
        match module {
            Some(name) => self
                .named
                .get(name)
                .cloned()
                .unwrap_or_else(|| Err("linking".to_string())),
            None => self.current.clone(),
        }
    }

    /// Module names a fresh `convert()` may treat as import sources.
    /// Those are always `spectest`, plus every successfully-registered module.
    /// The registered ones count only when the language supports them.
    fn resolvable_modules(&self) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        set.insert("spectest".to_string());
        if self.lang.supports_registered_imports() {
            set.extend(
                self.registered
                    .iter()
                    .filter(|(_, r)| r.is_ok())
                    .map(|(name, _)| name.clone()),
            );
        }
        set
    }

    /// `(name, instance-expr)` pairs for `emit_instantiate`/`instantiate_call`.
    fn registered_pairs(&self) -> Vec<(String, String)> {
        if !self.lang.supports_registered_imports() {
            return Vec::new();
        }
        self.registered
            .iter()
            .filter_map(|(name, r)| r.as_ref().ok().map(|var| (name.clone(), var.clone())))
            .collect()
    }

    /// Encode `qw` and convert it, with the record-keeping every conversion site shares.
    /// That is the module counter, the converted count, the unit union, and attribution.
    /// Attribution covers the unknown-proposal mapping and hard errors.
    /// `Err` carries the skip tag; emitting the instantiation is the caller's job.
    fn convert_quote_wat(&mut self, mut qw: QuoteWat<'_>, desc: &str) -> Result<Converted, String> {
        let resolvable = self.resolvable_modules();
        self.counter += 1;
        let converted = qw
            .encode()
            .map_err(|e| Attribution::Tag("unknown-proposal".to_string(), e.to_string()))
            .and_then(|bytes| convert(self.lang, &bytes, self.counter, &resolvable));
        match converted {
            Ok(conv) => {
                self.converted += 1;
                self.units.extend(conv.units.iter().cloned());
                Ok(conv)
            }
            Err(Attribution::Tag(tag, _detail)) => Err(tag),
            Err(Attribution::Bug(detail)) => {
                self.stats.hard_errors.push(format!(
                    "unattributed conversion failure at {desc}: {detail}"
                ));
                Err("conversion-bug".to_string())
            }
        }
    }

    fn define_module(&mut self, qw: QuoteWat<'_>, desc: &str) {
        let id = match &qw {
            QuoteWat::Wat(Wat::Module(m)) => m.id.map(|i| i.name().to_string()),
            _ => None,
        };
        let result = self.convert_quote_wat(qw, desc).map(|conv| {
            let registered = self.registered_pairs();
            self.lang.emit_instantiate(
                &mut self.script,
                &mut self.decls,
                &conv,
                self.counter,
                &registered,
            )
        });
        if let Some(id) = id {
            self.named.insert(id, result.clone());
        }
        self.current = result;
    }

    fn invoke_expr(&self, inv: &wast::WastInvoke<'_>) -> Result<String, String> {
        let var = self.instance_for(inv.module.map(|i| i.name()))?;
        self.lang.invoke(&var, inv.name, &inv.args)
    }
}

enum Attribution {
    /// Refusal attributed to a declared-unsupported tag.
    Tag(String, String),
    /// Unattributed refusal: a dewasm bug.
    Bug(String),
}

/// Map a conversion error to its attribution.
/// `UnsupportedError` anywhere in the chain names the responsible features.
/// Anything else is a bug.
fn attribute(err: &anyhow::Error) -> Attribution {
    match err
        .chain()
        .find_map(|e| e.downcast_ref::<UnsupportedError>())
    {
        Some(unsupported) if unsupported.features.is_empty() => {
            Attribution::Tag("unknown-proposal".to_string(), unsupported.detail.clone())
        }
        Some(unsupported) => {
            let ids: Vec<&str> = unsupported.features.iter().map(|f| f.id()).collect();
            Attribution::Tag(ids.join("+"), unsupported.detail.clone())
        }
        None => Attribution::Bug(format!("{err:#}")),
    }
}

fn convert(
    lang: &dyn SpecBackend,
    bytes: &[u8],
    counter: u32,
    resolvable: &std::collections::HashSet<String>,
) -> Result<Converted, Attribution> {
    let module = dewasm_core::build_module(bytes).map_err(|err| attribute(&err))?;
    // The harness only provides the `spectest` host module plus the resolvable `register`ed ones.
    // Those are the modules `ScriptGen::resolvable_modules` lists.
    // Anything else needs cross-module linking this harness doesn't track.
    let unresolved = |m: &str| !resolvable.contains(m);
    let needs_linking = module.imported_funcs.iter().any(|f| unresolved(&f.module))
        || module
            .imported_globals
            .iter()
            .any(|g| unresolved(&g.module))
        || module.imported_tables.iter().any(|t| unresolved(&t.module))
        || module
            .imported_memory
            .as_ref()
            .is_some_and(|m| unresolved(&m.module));
    if needs_linking {
        return Err(Attribution::Tag(
            "linking".to_string(),
            "imports from an unresolvable module".to_string(),
        ));
    }
    lang.generate(&module, counter)
        .map_err(|err| attribute(&err))
}

fn run_file(lang: &dyn SpecBackend, name: &str, path: &Path) -> anyhow::Result<Stats> {
    let source = std::fs::read_to_string(path)?;
    // A text-format construct newer than the `wast` crate is not our bug.
    // Report it like an unknown proposal.
    let unparsable = |err: &dyn std::fmt::Display| {
        eprintln!("{name}.wast does not parse ({err}); counted as unknown-proposal");
        let mut stats = Stats::default();
        stats.unsupported.insert("unknown-proposal".to_string(), 1);
        stats
    };
    let buf = match ParseBuffer::new(&source) {
        Ok(buf) => buf,
        Err(err) => return Ok(unparsable(&err)),
    };
    let wast: Wast = match parser::parse(&buf) {
        Ok(wast) => wast,
        Err(err) => return Ok(unparsable(&err)),
    };
    run_directives(lang, name, &source, wast)
}

fn run_directives(
    lang: &dyn SpecBackend,
    name: &str,
    source: &str,
    wast: Wast<'_>,
) -> anyhow::Result<Stats> {
    let mut gen = ScriptGen {
        lang,
        script: String::new(),
        decls: String::new(),
        source,
        file: name,
        current: Err("linking".to_string()),
        named: Default::default(),
        registered: Default::default(),
        counter: 0,
        converted: 0,
        stats: Stats::default(),
        units: lang.seed_units().iter().map(|s| s.to_string()).collect(),
    };

    for directive in wast.directives {
        let span = directive.span();
        let desc = gen.desc(span);
        match directive {
            WastDirective::Module(qw) => gen.define_module(qw, &desc),
            WastDirective::AssertReturn { exec, results, .. } => {
                let call = match &exec {
                    WastExecute::Invoke(inv) => gen.invoke_expr(inv),
                    WastExecute::Get { module, global, .. } => gen
                        .instance_for(module.map(|i| i.name()))
                        .map(|var| lang.global_get(&var, global)),
                    WastExecute::Wat(_) => Err("linking".to_string()),
                };
                let emitted =
                    call.and_then(|call| lang.emit_check(&mut gen.script, &desc, &call, &results));
                if let Err(tag) = emitted {
                    gen.skip(&tag);
                }
            }
            WastDirective::AssertTrap { exec, message, .. } => {
                let call = match exec {
                    WastExecute::Invoke(inv) => gen.invoke_expr(&inv),
                    WastExecute::Wat(wat) => {
                        // `assert_trap` on a module: the instantiation itself is what must trap.
                        gen.convert_quote_wat(QuoteWat::Wat(wat), &desc)
                            .map(|conv| {
                                let registered = gen.registered_pairs();
                                lang.instantiate_call(
                                    &mut gen.script,
                                    &mut gen.decls,
                                    &conv,
                                    &registered,
                                )
                            })
                    }
                    _ => Err("linking".to_string()),
                };
                match call {
                    Ok(call) => lang.emit_check_trap(&mut gen.script, &desc, &call, message),
                    Err(tag) => gen.skip(&tag),
                }
            }
            WastDirective::AssertExhaustion { call, .. } => match gen.invoke_expr(&call) {
                Ok(call) => lang.emit_check_exhaust(&mut gen.script, &desc, &call),
                Err(tag) => gen.skip(&tag),
            },
            WastDirective::Invoke(inv) => match gen.invoke_expr(&inv) {
                Ok(call) => lang.emit_bare_invoke(&mut gen.script, &desc, &call),
                Err(tag) => gen.skip(&tag),
            },
            WastDirective::AssertInvalid { mut module, .. }
            | WastDirective::AssertMalformed { mut module, .. }
            | WastDirective::AssertInvalidCustom { mut module, .. }
            | WastDirective::AssertMalformedCustom { mut module, .. } => {
                // Handled on the Rust side: the module must fail to decode, validate, or convert.
                match module.encode() {
                    Ok(bytes) => match dewasm_core::build_module(&bytes) {
                        Err(_) => gen.stats.rust_pass += 1,
                        Ok(_) => {
                            gen.stats.rust_fail += 1;
                            eprintln!("expected invalid but converted fine: {desc}");
                        }
                    },
                    Err(_) => gen.stats.rust_pass += 1,
                }
            }
            WastDirective::Register { name, module, .. } => {
                let inst = gen.instance_for(module.map(|i| i.name()));
                gen.registered.insert(name.to_string(), inst);
            }
            WastDirective::AssertUnlinkable {
                module, message, ..
            } => {
                if !lang.supports_registered_imports() {
                    gen.skip("linking");
                } else {
                    match gen.convert_quote_wat(QuoteWat::Wat(module), &desc) {
                        Ok(conv) => {
                            let registered = gen.registered_pairs();
                            let call = lang.instantiate_call(
                                &mut gen.script,
                                &mut gen.decls,
                                &conv,
                                &registered,
                            );
                            // Upstream wording never matches ours.
                            // Only the Rt::LinkError class is checked.
                            let _ = message;
                            lang.emit_check_unlinkable(&mut gen.script, &desc, &call);
                        }
                        Err(tag) => gen.skip(&tag),
                    }
                }
            }
            WastDirective::ModuleDefinition(_) | WastDirective::ModuleInstance { .. } => {
                gen.skip("linking")
            }
            WastDirective::Thread(_) | WastDirective::Wait { .. } => gen.skip("threads"),
            WastDirective::AssertException { exec, .. } => {
                let call = match &exec {
                    WastExecute::Invoke(inv) => gen.invoke_expr(inv),
                    _ => Err("linking".to_string()),
                };
                let emitted =
                    call.and_then(|call| lang.emit_check_exception(&mut gen.script, &desc, &call));
                if let Err(tag) = emitted {
                    gen.skip(&tag);
                }
            }
            WastDirective::AssertSuspension { .. } => gen.skip("stack-switching"),
        }
    }

    if gen.converted == 0 {
        return Ok(gen.stats);
    }

    // One shared runtime bundle for the whole file, kept minimal.
    // So undeclared unit dependencies surface as missing-method errors.
    let script = lang
        .assemble(&gen.units, &gen.decls, &gen.script)
        .map_err(|e| anyhow::anyhow!("assembling script: {e:#}"))?;

    let output = lang.run(&script, &[], "");
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut stats = gen.stats;
    let mut found = false;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("RESULT pass=") {
            let mut parts = rest.split([' ', '=']);
            stats.pass += parts.next().unwrap_or("0").parse().unwrap_or(0);
            let _ = parts.next(); // "fail"
            stats.fail += parts.next().unwrap_or("0").parse().unwrap_or(0);
            found = true;
        } else if line.starts_with("FAIL") {
            stats.fail_lines.push(line.to_string());
        }
    }
    if !found {
        anyhow::bail!(
            "{} did not report results (exit: {:?}):\n{}\n{}",
            lang.name(),
            output.status,
            stdout.lines().take(20).collect::<Vec<_>>().join("\n"),
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .take(20)
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    Ok(stats)
}
