//! Spec test harness (ADR-3, skip policy revised by ADR-8): parses every
//! .wast file of the testsuite submodule (tests/spec, tracking upstream
//! latest), translates each module with the Ruby backend, generates a Ruby
//! script that runs all assertions, and executes it with the real `ruby`
//! interpreter.
//!
//! Skips must be *attributable*: a module that fails to convert carries an
//! `UnsupportedError` naming the declared-unsupported features, and every
//! directive skipped because of it is counted under those feature ids. A
//! conversion failure without attribution is a dewasmify bug and fails the
//! suite. Validation failures beyond every proposal this toolchain knows
//! are reported as `unknown-proposal` but tolerated (the converter refused
//! cleanly, which is the ADR-0 contract).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use dewasmify_backend::{Backend, RuntimeLinkage, SupportStatus};
use dewasmify_backend_ruby::RubyBackend;
use dewasmify_core::feature::{Feature, UnsupportedError};
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastRet, Wat};

/// Known assertion-level failures with their attribution; the file still
/// runs so regressions in the passing assertions are caught.
///
/// All current entries are cross-module linking: a later `register`ed
/// module was supposed to write into the first module's shared
/// table/memory, so assertions against the first module see stale state.
/// The linking milestone clears this list.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("elem", 5, "linking"),
    ("linking", 10, "linking"),
    ("linking0", 1, "linking"),
    ("linking3", 2, "linking"),
    ("load1", 5, "linking"),
];

#[test]
fn spec() {
    if Command::new("ruby").arg("--version").output().is_err() {
        eprintln!("ruby not found in PATH; skipping spec tests");
        return;
    }
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
    if let Ok(list) = std::env::var("DEWASMIFY_SPEC") {
        let wanted: BTreeSet<&str> = list.split(',').map(|s| s.trim()).collect();
        names.retain(|n| wanted.contains(n.as_str()));
    }

    let mut failures = Vec::new();
    let mut total = Stats::default();
    for name in &names {
        let path = spec_dir.join(format!("{name}.wast"));
        match run_file(name, &path) {
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
                let expected = EXPECTED_FAILURES
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
                .map(|f| RubyBackend.feature_status(f) == SupportStatus::Supported)
                .unwrap_or(false)
        });
        if all_supported {
            failures.push(format!(
                "{count} directives skipped for {tag}, but the backend declares it supported"
            ));
        }
    }

    assert!(failures.is_empty(), "spec failures:\n{}", failures.join("\n"));
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

fn ruby_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '#' => out.push_str("\\#"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct ScriptGen<'a> {
    script: String,
    source: &'a str,
    file: &'a str,
    /// Ruby global variable holding the most recent instance, or the
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

    fn define_module(&mut self, mut qw: QuoteWat<'a>, desc: &str) {
        let id = match &qw {
            QuoteWat::Wat(Wat::Module(m)) => m.id.map(|i| i.name().to_string()),
            _ => None,
        };
        let converted = qw
            .encode()
            .map_err(|e| Attribution::Tag("unknown-proposal".to_string(), e.to_string()))
            .and_then(|bytes| convert(&bytes, self.counter));
        self.counter += 1;
        let result = match converted {
            Ok((class_src, class_name, units)) => {
                self.converted += 1;
                let var = format!("$i{}", self.counter);
                self.script.push_str(&class_src);
                let _ = writeln!(self.script, "{var} = {class_name}.new($spectest)");
                self.units.extend(units);
                Ok(var)
            }
            Err(Attribution::Tag(tag, _detail)) => Err(tag),
            Err(Attribution::Bug(detail)) => {
                self.stats
                    .hard_errors
                    .push(format!("unattributed conversion failure at {desc}: {detail}"));
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
        let mut args = vec![ruby_str(inv.name)];
        for arg in &inv.args {
            args.push(arg_rb(arg)?);
        }
        Ok(format!("{var}.invoke({})", args.join(", ")))
    }
}

enum Attribution {
    /// Refusal attributed to a declared-unsupported tag.
    Tag(String, String),
    /// Unattributed refusal: a dewasmify bug.
    Bug(String),
}

fn convert(
    bytes: &[u8],
    counter: u32,
) -> Result<(String, String, BTreeSet<String>), Attribution> {
    let module = dewasmify_core::build_module(bytes).map_err(|err| {
        match err.chain().find_map(|e| e.downcast_ref::<UnsupportedError>()) {
            Some(unsupported) if unsupported.features.is_empty() => {
                Attribution::Tag("unknown-proposal".to_string(), unsupported.detail.clone())
            }
            Some(unsupported) => {
                let ids: Vec<&str> = unsupported.features.iter().map(|f| f.id()).collect();
                Attribution::Tag(ids.join("+"), unsupported.detail.clone())
            }
            None => Attribution::Bug(format!("{err:#}")),
        }
    })?;
    // The harness only provides the spectest host module; imports from
    // registered modules need cross-module linking.
    if module.imported_funcs.iter().any(|f| f.module != "spectest") {
        return Err(Attribution::Tag(
            "linking".to_string(),
            "imports from a registered module".to_string(),
        ));
    }
    let class_name = format!("WastMod{counter}");
    let (src, units) = dewasmify_backend_ruby::generate_class_with_units(
        &module,
        &class_name,
        &RuntimeLinkage::Alias("::Rt".to_string()),
        false, // spec modules import spectest, never WASI
    )
    .map_err(|e| Attribution::Bug(format!("{e:#}")))?;
    Ok((src, class_name, units))
}

fn arg_rb(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok((*v as u32).to_string()),
        WastArg::Core(WastArgCore::I64(v)) => Ok((*v as u64).to_string()),
        WastArg::Core(WastArgCore::F32(f)) => {
            Ok(format!("Rt.f32_from_bits(0x{:x})", f.bits))
        }
        WastArg::Core(WastArgCore::F64(f)) => {
            Ok(format!("Rt.f64_from_bits(0x{:x})", f.bits))
        }
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(_) => Err("reference-types".to_string()),
        _ => Err("component-model".to_string()),
    }
}

fn ret_cmp(value: &str, ret: &WastRet<'_>) -> Result<String, String> {
    match ret {
        WastRet::Core(WastRetCore::I32(v)) => Ok(format!("{value} == {}", *v as u32)),
        WastRet::Core(WastRetCore::I64(v)) => Ok(format!("{value} == {}", *v as u64)),
        WastRet::Core(WastRetCore::F32(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => {
                format!("(Rt.f32_bits({value}) & 0x7fffffff) == 0x7fc00000")
            }
            NanPattern::ArithmeticNan => {
                format!("(Rt.f32_bits({value}) & 0x7fc00000) == 0x7fc00000")
            }
            NanPattern::Value(f) => {
                format!("Rt.f32_bits({value}) == 0x{:x}", f.bits)
            }
        }),
        WastRet::Core(WastRetCore::F64(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => {
                format!(
                    "(Rt.f64_bits({value}) & 0x7fffffffffffffff) == 0x7ff8000000000000"
                )
            }
            NanPattern::ArithmeticNan => {
                format!(
                    "(Rt.f64_bits({value}) & 0x7ff8000000000000) == 0x7ff8000000000000"
                )
            }
            NanPattern::Value(f) => {
                format!("Rt.f64_bits({value}) == 0x{:x}", f.bits)
            }
        }),
        WastRet::Core(WastRetCore::V128(_)) => Err("simd".to_string()),
        WastRet::Core(WastRetCore::Either(_)) => Err("either-results".to_string()),
        WastRet::Core(_) => Err("reference-types".to_string()),
        _ => Err("component-model".to_string()),
    }
}

fn run_file(name: &str, path: &Path) -> anyhow::Result<Stats> {
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
    run_directives(name, &source, wast)
}

fn run_directives(name: &str, source: &str, wast: Wast<'_>) -> anyhow::Result<Stats> {
    let mut gen = ScriptGen {
        script: String::new(),
        source,
        file: name,
        current: Err("linking".to_string()),
        named: Default::default(),
        counter: 0,
        converted: 0,
        stats: Stats::default(),
        // Units the harness helpers themselves use.
        units: ["rt/trap", "rt/f32_bits", "rt/f32_from_bits", "rt/f64_bits", "rt/f64_from_bits"]
            .into_iter()
            .map(String::from)
            .collect(),
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
                        .map(|var| format!("{var}.global_get({})", ruby_str(global))),
                    WastExecute::Wat(_) => Err("linking".to_string()),
                };
                let cmp = (|| -> Result<String, String> {
                    Ok(match results.len() {
                        0 => "true".to_string(),
                        1 => ret_cmp("__r", &results[0])?,
                        _ => {
                            let mut parts = Vec::new();
                            for (i, r) in results.iter().enumerate() {
                                parts.push(ret_cmp(&format!("__r[{i}]"), r)?);
                            }
                            parts.join(" && ")
                        }
                    })
                })();
                match (call, cmp) {
                    (Ok(call), Ok(cmp)) => {
                        let _ = writeln!(
                            gen.script,
                            "check({}) do\n  __r = {call}\n  [({cmp}), __r]\nend",
                            ruby_str(&desc)
                        );
                    }
                    (Err(tag), _) | (_, Err(tag)) => gen.skip(&tag),
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
                            .and_then(|bytes| convert(&bytes, counter));
                        match converted {
                            Ok((class_src, class_name, units)) => {
                                gen.converted += 1;
                                gen.script.push_str(&class_src);
                                gen.units.extend(units);
                                Ok(format!("{class_name}.new($spectest)"))
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
                    Ok(call) => {
                        let _ = writeln!(
                            gen.script,
                            "check_trap({}, {}) do\n  {call}\nend",
                            ruby_str(&desc),
                            ruby_str(message)
                        );
                    }
                    Err(tag) => gen.skip(&tag),
                }
            }
            WastDirective::AssertExhaustion { call, .. } => match gen.invoke_expr(&call) {
                Ok(call) => {
                    let _ = writeln!(
                        gen.script,
                        "check_exhaust({}) do\n  {call}\nend",
                        ruby_str(&desc)
                    );
                }
                Err(tag) => gen.skip(&tag),
            },
            WastDirective::Invoke(inv) => match gen.invoke_expr(&inv) {
                Ok(call) => {
                    let _ = writeln!(
                        gen.script,
                        "check({}) do\n  {call}\n  [true, nil]\nend",
                        ruby_str(&desc)
                    );
                }
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

    gen.script.push_str(POSTAMBLE);

    // One shared runtime bundle for the whole file, kept minimal so that
    // undeclared unit dependencies surface as NoMethodError.
    let mut script = dewasmify_backend_ruby::shared_runtime(&gen.units)
        .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?;
    script.push_str(PREAMBLE);
    script.push_str(&gen.script);

    let script_path = std::env::temp_dir().join(format!("dewasmify-spec-{name}.rb"));
    std::fs::write(&script_path, &script)?;
    let output = Command::new("ruby").arg(&script_path).output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut stats = gen.stats;
    let mut found = false;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("RESULT pass=") {
            let mut parts = rest.split(|c| c == ' ' || c == '=');
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
            "ruby did not report results (exit: {:?}):\n{}\n{}",
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

const PREAMBLE: &str = r#"
$pass = 0
$fail = 0

def check(desc)
  ok, actual = yield
  if ok
    $pass += 1
  else
    $fail += 1
    puts "FAIL: #{desc} (got #{actual.inspect})"
  end
rescue => e
  $fail += 1
  puts "FAIL(#{e.class}: #{e.message}): #{desc}"
end

def check_trap(desc, msg)
  yield
  $fail += 1
  puts "FAIL(no trap, want #{msg.inspect}): #{desc}"
rescue Rt::Trap => e
  if e.message.include?(msg) || msg.include?(e.message)
    $pass += 1
  else
    $fail += 1
    puts "FAIL(trap #{e.message.inspect}, want #{msg.inspect}): #{desc}"
  end
rescue => e
  $fail += 1
  puts "FAIL(#{e.class}: #{e.message}, want trap #{msg.inspect}): #{desc}"
end

def check_exhaust(desc)
  yield
  $fail += 1
  puts "FAIL(no exhaustion): #{desc}"
rescue SystemStackError
  $pass += 1
end

$spectest = {
  "spectest" => {
    "print" => ->(*) { nil },
    "print_i32" => ->(*) { nil },
    "print_i64" => ->(*) { nil },
    "print_f32" => ->(*) { nil },
    "print_f64" => ->(*) { nil },
    "print_i32_f32" => ->(*) { nil },
    "print_f64_f64" => ->(*) { nil },
  },
}
"#;

const POSTAMBLE: &str = r#"
puts "RESULT pass=#{$pass} fail=#{$fail}"
"#;
