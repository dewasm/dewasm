//! Spec test harness (ADR-3, skip policy revised by ADR-8): parses every
//! .wast file of the testsuite submodule (tests/spec, tracking upstream
//! latest), translates each module with a target-language backend, generates
//! a script that runs all assertions, and executes it with the real
//! interpreter for that language. The language-specific pieces live behind
//! the `SpecBackend` trait (implemented in each backend crate); everything
//! about directive iteration, skip attribution, and result accounting is
//! shared here.
//!
//! Skips must be *attributable*: a module that fails to convert carries an
//! `UnsupportedError` naming the declared-unsupported features, and every
//! directive skipped because of it is counted under those feature ids. A
//! conversion failure without attribution is a dewasm bug and fails the
//! suite. Validation failures beyond every proposal this toolchain knows are
//! reported as `unknown-proposal` but tolerated (the converter refused
//! cleanly, which is the ADR-0 contract).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use dewasm_backend::SupportStatus;
use dewasm_core::feature::{Feature, UnsupportedError};
use dewasm_core::ir;
use libtest_mimic::{Failed, Trial};
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastRet, Wat};

use crate::backend::BackendUnderTest;

/// The script-phrasing layer of a backend under test (ADR-27): how to convert
/// a module, how to phrase each assertion in that language, and how to bundle
/// the runtime. `name`/`backend`/`interpreter`/`run` come from the base
/// [`BackendUnderTest`] trait. `emit_*` methods append to the script body;
/// returning `Err(tag)` skips the directive under that attribution tag.
pub trait SpecBackend: BackendUnderTest {
    /// Known assertion-level failures: (file, count, attribution tag).
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)];
    /// The non-ignored set: files run by a plain `cargo test`. Every other
    /// `.wast` file still becomes a trial, but marked `#[ignore]`d, so
    /// `cargo test -- --ignored` / `--include-ignored` sweeps the whole
    /// testsuite. `None` marks nothing ignored — the whole testsuite runs by
    /// default (used by the fast interpreters).
    fn curated_files(&self) -> Option<&'static [&'static str]>;
    /// Units the harness helpers themselves use.
    fn seed_units(&self) -> &'static [&'static str];
    /// Lower an IR module to source. Backend-level refusals (e.g. floats on an
    /// integer-only backend) carry `UnsupportedError` in the chain.
    fn generate(&self, module: &ir::Module, counter: u32) -> anyhow::Result<Converted>;
    /// Whether `register`-directive linking is wired up for this language: a
    /// `(register "Name" $id)`'d instance becomes resolvable as an import
    /// source for later modules (`registered` below), and `assert_unlinkable`
    /// is checked for real instead of always skipped.
    fn supports_registered_imports(&self) -> bool {
        false
    }
    /// Emit the module source plus its instantiation; returns the variable (or
    /// prefix) later invocations use to reach the instance. `registered` is
    /// the (name, instance-expr) pairs of currently `register`ed modules with
    /// a live instance, for languages that support them. `decls` is the
    /// file-scoped declaration buffer: languages that must hoist a converted
    /// module's definitions to package/class scope (Go, Java) push
    /// `conv.source` there rather than into `script`. It is owned by the
    /// per-file harness state, so parallel trials never share it.
    fn emit_instantiate(
        &self,
        script: &mut String,
        decls: &mut String,
        conv: &Converted,
        var_id: u32,
        registered: &[(String, String)],
    ) -> String;
    /// Emit the module source only, returning the call that performs the
    /// (possibly trapping) instantiation, for assert_trap on a module. See
    /// [`Self::emit_instantiate`] for `decls`.
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
    /// Emit an `assert_exception` check: `call` must raise an (uncaught) wasm
    /// exception. The default keeps the directive an attributed skip for
    /// backends that don't declare exception handling supported.
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
    /// Emit an `assert_unlinkable` check: `call` (the instantiation) must
    /// raise/fail. Only invoked when `supports_registered_imports()` is true.
    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = (script, desc, call);
        unreachable!("only called when supports_registered_imports() is true")
    }
    /// Wrap the accumulated body into a runnable script: shared runtime for
    /// `units`, harness helpers, hoisted `decls` (see
    /// [`Self::emit_instantiate`]), body, result-line footer. Backends that
    /// inline module definitions into `script` receive an empty `decls`.
    fn assemble(&self, units: &BTreeSet<String>, decls: &str, body: &str)
        -> anyhow::Result<String>;
}

/// A converted module: its source text, the language-specific handle used to
/// instantiate it (class name, function prefix, ...), and the runtime units it
/// references.
pub struct Converted {
    pub source: String,
    pub handle: String,
    pub units: BTreeSet<String>,
}

/// The tests/spec submodule directory (upstream testsuite).
fn spec_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec")
}

/// Build one libtest-mimic [`Trial`] per `.wast` file of the testsuite for
/// `lang` (the `spec_suite!` macro's entry point). File selection is now
/// cargo's own: the trial name is the file stem, so `cargo test --test spec
/// i32` runs the `i32`-named file(s) via the built-in name filter. Files
/// outside the backend's [`SpecBackend::curated_files`] set become `#[ignore]`d
/// trials, so a plain `cargo test` runs the curated set and `--include-ignored`
/// (or `--ignored`) sweeps the whole testsuite. Trials run on libtest-mimic's
/// thread pool; each owns its per-file state, so the sweep parallelizes.
///
/// `slow_tier` is the backend crate's `slow_test` feature (CI's main sweep tier,
/// ADR-48): when on, nothing is marked ignored — the whole testsuite runs, the
/// same set the old `--include-ignored` sweep covered.
pub fn spec_trials(lang: &'static dyn SpecBackend, slow_tier: bool) -> Vec<Trial> {
    let dir = spec_dir();
    assert!(
        dir.exists(),
        "tests/spec not found — run `git submodule update --init` (see docs/testing.md)"
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

    // The slow tier runs the whole testsuite (nothing curated => nothing
    // ignored below), matching the old `--include-ignored` main sweep.
    let curated: Option<BTreeSet<&'static str>> = if slow_tier {
        None
    } else {
        lang.curated_files().map(|c| c.iter().copied().collect())
    };

    names
        .into_iter()
        .map(|name| {
            // Nothing curated => nothing ignored (the whole suite is the
            // default); otherwise files outside the curated set are ignored.
            let ignored = curated
                .as_ref()
                .is_some_and(|set| !set.contains(name.as_str()));
            let path = dir.join(format!("{name}.wast"));
            Trial::test(name.clone(), move || run_trial(lang, &name, &path))
                .with_ignored_flag(ignored)
        })
        .collect()
}

/// harness=false entry point: parse cargo's test arguments (name filter,
/// `--ignored`/`--include-ignored`, thread count, ...) and run the trials.
pub fn spec_main(lang: &'static dyn SpecBackend, slow_tier: bool) {
    let args = libtest_mimic::Arguments::from_args();
    libtest_mimic::run(&args, spec_trials(lang, slow_tier)).exit();
}

/// Run one `.wast` file and apply the per-file gates that the old aggregate
/// suite applied globally: (a) the assertion-failure count must equal the
/// backend's `EXPECTED_FAILURES` ledger entry (0 if absent); (b) an
/// unattributed conversion failure is a dewasm bug; (c) a skip attributed to a
/// feature the backend declares `Supported` is a declaration regression.
/// Checks (a)/(c) are per-file here, equivalent to the old global check because
/// the global sets are the union of the per-file sets. A passing trial stays
/// quiet; failures carry the per-file summary and detail.
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

    // A skip is only legitimate while its feature is declared unsupported;
    // once the backend flips a feature to Supported, remaining skips are
    // regressions of the declaration.
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
    /// Skipped directives, attributed to declared-unsupported feature ids
    /// (plus harness-level tags like "linking" and "unknown-proposal").
    unsupported: BTreeMap<String, u32>,
    /// Unattributed conversion errors: dewasm bugs, fail the suite.
    hard_errors: Vec<String>,
    /// assert_invalid / assert_malformed handled on the Rust side
    rust_pass: u32,
    rust_fail: u32,
    /// `FAIL...` lines from the generated script's stdout, surfaced in the
    /// trial's failure message (only when the file's count breaks the ledger).
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
    /// File-scoped declaration buffer hoisted ahead of `script` by `assemble`
    /// (see [`SpecBackend::emit_instantiate`]). Owned per file, so parallel
    /// trials never share it.
    decls: String,
    source: &'a str,
    file: &'a str,
    /// Variable/prefix holding the most recent instance, or the attribution
    /// tag when the most recent module failed to convert.
    current: Result<String, String>,
    /// Same, for named modules.
    named: std::collections::HashMap<String, Result<String, String>>,
    /// Same, keyed by the *registered* name from `(register "Name" $id)` (a
    /// different namespace from `named`'s wast `$id`s) — what a later module's
    /// `(import "Name" ...)` actually resolves against.
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

    /// Module names a fresh `convert()` may treat as import sources: always
    /// `spectest`, plus every successfully-registered module when the language
    /// supports it.
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

    /// (name, instance-expr) pairs for `emit_instantiate`/`instantiate_call`.
    fn registered_pairs(&self) -> Vec<(String, String)> {
        if !self.lang.supports_registered_imports() {
            return Vec::new();
        }
        self.registered
            .iter()
            .filter_map(|(name, r)| r.as_ref().ok().map(|var| (name.clone(), var.clone())))
            .collect()
    }

    /// Encode `qw` and convert it, with the bookkeeping every conversion site
    /// shares: the module counter, the converted count, the unit union, and
    /// attribution (unknown-proposal mapping, hard errors). `Err` carries the
    /// skip tag; emitting the instantiation is the caller's job.
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

/// Map a conversion error to its attribution: `UnsupportedError` anywhere in
/// the chain names the responsible features; anything else is a bug.
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
    // The harness only provides the spectest host module plus whatever is
    // currently `register`ed and resolvable (ScriptGen::resolvable_modules);
    // anything else needs cross-module linking this harness doesn't track.
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
    // A text-format construct newer than the wast crate is not our bug; report
    // it like an unknown proposal.
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
                        // instantiation trap: convert the module inline
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
                // Handled on the Rust side: the module must fail to decode,
                // validate, or convert.
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
                            // Upstream wording never matches ours; only the
                            // Rt::LinkError class is checked.
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

    // One shared runtime bundle for the whole file, kept minimal so that
    // undeclared unit dependencies surface as missing-method errors.
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
