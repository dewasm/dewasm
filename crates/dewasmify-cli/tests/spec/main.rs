//! Spec test harness (ADR-3, skip policy revised by ADR-8): parses every
//! .wast file of the testsuite submodule (tests/spec, tracking upstream
//! latest), translates each module with a target-language backend,
//! generates a script that runs all assertions, and executes it with the
//! real interpreter for that language. The language-specific pieces live
//! behind the `SpecLang` trait (one module per language); everything about
//! directive iteration, skip attribution, and result accounting is shared.
//!
//! Skips must be *attributable*: a module that fails to convert carries an
//! `UnsupportedError` naming the declared-unsupported features, and every
//! directive skipped because of it is counted under those feature ids. A
//! conversion failure without attribution is a dewasmify bug and fails the
//! suite. Validation failures beyond every proposal this toolchain knows
//! are reported as `unknown-proposal` but tolerated (the converter refused
//! cleanly, which is the ADR-0 contract).

mod bash;
mod ruby;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use dewasmify_backend::Backend;
use dewasmify_backend::SupportStatus;
use dewasmify_core::feature::{Feature, UnsupportedError};
use dewasmify_core::ir;
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastRet, Wat};

#[test]
fn spec_ruby() {
    run_suite(&ruby::RubyLang);
}

#[test]
fn spec_bash() {
    run_suite(&bash::BashLang);
}

/// A target language plugged into the shared harness: how to convert a
/// module, how to phrase each assertion in that language, and how to run
/// the resulting script. `emit_*` methods append to the script body;
/// returning `Err(tag)` skips the directive under that attribution tag.
trait SpecLang {
    fn name(&self) -> &'static str;
    fn backend(&self) -> &dyn Backend;
    /// Interpreter to run generated scripts with, or None to skip the
    /// whole suite (the implementation prints why).
    fn interpreter(&self) -> Option<PathBuf>;
    fn script_ext(&self) -> &'static str;
    /// Known assertion-level failures: (file, count, attribution tag).
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)];
    /// Curated default file list for slow interpreters; None runs every
    /// file. `DEWASMIFY_SPEC_ALL=1` overrides the curation.
    fn default_files(&self) -> Option<&'static [&'static str]>;
    /// Units the harness helpers themselves use.
    fn seed_units(&self) -> &'static [&'static str];
    /// Lower an IR module to source. Backend-level refusals (e.g. floats
    /// on an integer-only backend) carry `UnsupportedError` in the chain.
    fn generate(&self, module: &ir::Module, counter: u32) -> anyhow::Result<Converted>;
    /// Emit the module source plus its instantiation; returns the variable
    /// (or prefix) later invocations use to reach the instance.
    fn emit_instantiate(&self, script: &mut String, conv: &Converted, var_id: u32) -> String;
    /// Emit the module source only, returning the call that performs the
    /// (possibly trapping) instantiation, for assert_trap on a module.
    fn instantiate_call(&self, script: &mut String, conv: &Converted) -> String;
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
    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str);
    /// Wrap the accumulated body into a runnable script: shared runtime
    /// for `units`, harness helpers, body, result-line footer.
    fn assemble(&self, units: &BTreeSet<String>, body: &str) -> anyhow::Result<String>;
}

/// A converted module: its source text, the language-specific handle used
/// to instantiate it (class name, function prefix, ...), and the runtime
/// units it references.
struct Converted {
    source: String,
    handle: String,
    units: BTreeSet<String>,
}

fn run_suite(lang: &dyn SpecLang) {
    let Some(interpreter) = lang.interpreter() else {
        return;
    };
    let spec_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec");
    if !spec_dir.exists() {
        eprintln!("tests/spec not found (git submodule update --init); skipping");
        return;
    }

    let mut names: Vec<String> = std::fs::read_dir(&spec_dir)
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
    if let Some(curated) = lang.default_files() {
        if std::env::var("DEWASMIFY_SPEC_ALL").is_err() {
            let wanted: BTreeSet<&str> = curated.iter().copied().collect();
            names.retain(|n| wanted.contains(n.as_str()));
        }
    }
    if let Ok(list) = std::env::var("DEWASMIFY_SPEC") {
        let wanted: BTreeSet<&str> = list.split(',').map(|s| s.trim()).collect();
        names.retain(|n| wanted.contains(n.as_str()));
    }

    let mut failures = Vec::new();
    let mut total = Stats::default();
    for name in &names {
        let path = spec_dir.join(format!("{name}.wast"));
        match run_file(lang, &interpreter, name, &path) {
            Ok(stats) => {
                if stats.pass + stats.fail > 0 || !stats.unsupported.is_empty() {
                    println!(
                        "{name}: pass={} fail={} skip={} (rust: invalid-ok={} invalid-bad={})",
                        stats.pass,
                        stats.fail,
                        stats.skipped(),
                        stats.rust_pass,
                        stats.rust_fail
                    );
                }
                for err in &stats.hard_errors {
                    failures.push(format!("{name}: {err}"));
                }
                let expected = lang
                    .expected_failures()
                    .iter()
                    .find(|(n, _, _)| n == name)
                    .map(|(_, count, _)| *count)
                    .unwrap_or(0);
                if stats.fail != expected {
                    failures.push(format!(
                        "{name}: {} assertion failures (expected {expected})",
                        stats.fail
                    ));
                }
                total.merge(stats);
            }
            Err(err) => failures.push(format!("{name}: {err:#}")),
        }
    }

    println!(
        "\nTOTAL: pass={} fail={} skip={} (rust: invalid-ok={} invalid-bad={})",
        total.pass,
        total.fail,
        total.skipped(),
        total.rust_pass,
        total.rust_fail
    );
    let mut by_count: Vec<(&String, &u32)> = total.unsupported.iter().collect();
    by_count.sort_by_key(|(tag, count)| (std::cmp::Reverse(**count), tag.as_str()));
    println!("unsupported (declared, ADR-8):");
    for (tag, count) in by_count {
        println!("  {tag}: {count}");
    }

    // A skip is only legitimate while its feature is declared unsupported;
    // once the backend flips a feature to Supported, remaining skips are
    // regressions of the declaration.
    for (tag, count) in &total.unsupported {
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

    assert!(
        failures.is_empty(),
        "spec failures:\n{}",
        failures.join("\n")
    );
}

#[derive(Default)]
struct Stats {
    pass: u32,
    fail: u32,
    /// Skipped directives, attributed to declared-unsupported feature ids
    /// (plus harness-level tags like "linking" and "unknown-proposal").
    unsupported: BTreeMap<String, u32>,
    /// Unattributed conversion errors: dewasmify bugs, fail the suite.
    hard_errors: Vec<String>,
    /// assert_invalid / assert_malformed handled on the Rust side
    rust_pass: u32,
    rust_fail: u32,
}

impl Stats {
    fn skipped(&self) -> u32 {
        self.unsupported.values().sum()
    }

    fn merge(&mut self, other: Stats) {
        self.pass += other.pass;
        self.fail += other.fail;
        self.rust_pass += other.rust_pass;
        self.rust_fail += other.rust_fail;
        for (tag, count) in other.unsupported {
            *self.unsupported.entry(tag).or_default() += count;
        }
        self.hard_errors.extend(other.hard_errors);
    }
}

struct ScriptGen<'a> {
    lang: &'a dyn SpecLang,
    script: String,
    source: &'a str,
    file: &'a str,
    /// Variable/prefix holding the most recent instance, or the
    /// attribution tag when the most recent module failed to convert.
    current: Result<String, String>,
    /// Same, for named modules.
    named: std::collections::HashMap<String, Result<String, String>>,
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

    fn define_module(&mut self, mut qw: QuoteWat<'_>, desc: &str) {
        let id = match &qw {
            QuoteWat::Wat(Wat::Module(m)) => m.id.map(|i| i.name().to_string()),
            _ => None,
        };
        let converted = qw
            .encode()
            .map_err(|e| Attribution::Tag("unknown-proposal".to_string(), e.to_string()))
            .and_then(|bytes| convert(self.lang, &bytes, self.counter));
        self.counter += 1;
        let result = match converted {
            Ok(conv) => {
                self.converted += 1;
                let var = self
                    .lang
                    .emit_instantiate(&mut self.script, &conv, self.counter);
                self.units.extend(conv.units);
                Ok(var)
            }
            Err(Attribution::Tag(tag, _detail)) => Err(tag),
            Err(Attribution::Bug(detail)) => {
                self.stats.hard_errors.push(format!(
                    "unattributed conversion failure at {desc}: {detail}"
                ));
                Err("conversion-bug".to_string())
            }
        };
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
    /// Unattributed refusal: a dewasmify bug.
    Bug(String),
}

/// Map a conversion error to its attribution: `UnsupportedError` anywhere
/// in the chain names the responsible features; anything else is a bug.
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

fn convert(lang: &dyn SpecLang, bytes: &[u8], counter: u32) -> Result<Converted, Attribution> {
    let module = dewasmify_core::build_module(bytes).map_err(|err| attribute(&err))?;
    // The harness only provides the spectest host module; imports from
    // registered modules need cross-module linking.
    if module.imported_funcs.iter().any(|f| f.module != "spectest") {
        return Err(Attribution::Tag(
            "linking".to_string(),
            "imports from a registered module".to_string(),
        ));
    }
    lang.generate(&module, counter)
        .map_err(|err| attribute(&err))
}

fn run_file(
    lang: &dyn SpecLang,
    interpreter: &Path,
    name: &str,
    path: &Path,
) -> anyhow::Result<Stats> {
    let source = std::fs::read_to_string(path)?;
    // A text-format construct newer than the wast crate is not our bug;
    // report it like an unknown proposal.
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
    run_directives(lang, interpreter, name, &source, wast)
}

fn run_directives(
    lang: &dyn SpecLang,
    interpreter: &Path,
    name: &str,
    source: &str,
    wast: Wast<'_>,
) -> anyhow::Result<Stats> {
    let mut gen = ScriptGen {
        lang,
        script: String::new(),
        source,
        file: name,
        current: Err("linking".to_string()),
        named: Default::default(),
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
                        let mut qw = QuoteWat::Wat(wat);
                        gen.counter += 1;
                        let counter = gen.counter;
                        let converted = qw
                            .encode()
                            .map_err(|e| {
                                Attribution::Tag("unknown-proposal".to_string(), e.to_string())
                            })
                            .and_then(|bytes| convert(lang, &bytes, counter));
                        match converted {
                            Ok(conv) => {
                                gen.converted += 1;
                                let call = lang.instantiate_call(&mut gen.script, &conv);
                                gen.units.extend(conv.units);
                                Ok(call)
                            }
                            Err(Attribution::Tag(tag, _)) => Err(tag),
                            Err(Attribution::Bug(detail)) => {
                                gen.stats.hard_errors.push(format!(
                                    "unattributed conversion failure at {desc}: {detail}"
                                ));
                                Err("conversion-bug".to_string())
                            }
                        }
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
                    Ok(bytes) => match dewasmify_core::build_module(&bytes) {
                        Err(_) => gen.stats.rust_pass += 1,
                        Ok(_) => {
                            gen.stats.rust_fail += 1;
                            eprintln!("expected invalid but converted fine: {desc}");
                        }
                    },
                    Err(_) => gen.stats.rust_pass += 1,
                }
            }
            WastDirective::Register { .. }
            | WastDirective::ModuleDefinition(_)
            | WastDirective::ModuleInstance { .. }
            | WastDirective::AssertUnlinkable { .. } => gen.skip("linking"),
            WastDirective::Thread(_) | WastDirective::Wait { .. } => gen.skip("threads"),
            WastDirective::AssertException { .. } => gen.skip("exception-handling"),
            WastDirective::AssertSuspension { .. } => gen.skip("stack-switching"),
        }
    }

    if gen.converted == 0 {
        return Ok(gen.stats);
    }

    // One shared runtime bundle for the whole file, kept minimal so that
    // undeclared unit dependencies surface as missing-method errors.
    let script = lang
        .assemble(&gen.units, &gen.script)
        .map_err(|e| anyhow::anyhow!("assembling script: {e:#}"))?;

    let script_path = std::env::temp_dir().join(format!(
        "dewasmify-spec-{}-{name}.{}",
        lang.name(),
        lang.script_ext()
    ));
    std::fs::write(&script_path, &script)?;
    let output = Command::new(interpreter).arg(&script_path).output()?;
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
            eprintln!("{line}");
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
