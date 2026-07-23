//! Spec test harness: parses .wast files from the official testsuite
//! (tests/spec), translates each module with the Ruby backend, generates a
//! Ruby script that runs all assertions, and executes it with the real
//! `ruby` interpreter.
//!
//! Directives that exercise unsupported features (reference types, SIMD,
//! multiple modules linking via `register`, ...) are counted as skipped:
//! a module that fails to convert marks itself broken and all directives
//! against it are skipped, so partial coverage per file is normal.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use std::collections::BTreeSet;

use dewasmify_backend::RuntimeLinkage;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::parser::{self, ParseBuffer};
use wast::{QuoteWat, Wast, WastArg, WastDirective, WastExecute, WastRet, Wat};

/// Files from the upstream testsuite that the Ruby backend is expected to
/// handle (possibly with skipped directives for unsupported features).
const FILES: &[&str] = &[
    "address", "align", "block", "br", "br_if", "br_table", "call",
    "call_indirect", "const", "conversions", "data", "elem", "endianness",
    "f32", "f32_bitwise", "f32_cmp", "f64", "f64_bitwise", "f64_cmp", "fac",
    "float_literals", "float_memory", "float_misc", "forward", "func",
    "global", "i32", "i64", "if", "int_exprs", "int_literals",
    "labels", "left-to-right", "load", "local_get", "local_set", "local_tee",
    "loop", "memory", "memory_copy", "memory_fill", "memory_grow",
    "memory_init", "memory_redundancy", "memory_size", "memory_trap", "nop",
    "return", "select", "stack", "store", "switch", "traps", "unreachable",
    "unwind",
];

/// Known failures with reasons; the file still runs so regressions in the
/// passing assertions are caught.
const EXPECTED_FAILURES: &[(&str, u32)] = &[
    // Cross-module table sharing via `register` is not supported; modules 2
    // and 3 fail to convert, so module 1's shared table never gets their
    // element segments.
    ("elem", 5),
];

#[test]
fn spec() {
    if Command::new("ruby").arg("--version").output().is_err() {
        eprintln!("ruby not found in PATH; skipping spec tests");
        return;
    }
    let spec_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/spec");
    if !spec_dir.exists() {
        eprintln!("tests/spec not found (clone WebAssembly/testsuite there); skipping");
        return;
    }

    let selected: Vec<String> = match std::env::var("DEWASMIFY_SPEC") {
        Ok(list) => list.split(',').map(|s| s.trim().to_string()).collect(),
        Err(_) => FILES.iter().map(|s| s.to_string()).collect(),
    };

    let mut failures = Vec::new();
    let mut total = Stats::default();
    for name in &selected {
        let path = spec_dir.join(format!("{name}.wast"));
        match run_file(name, &path) {
            Ok(stats) => {
                println!(
                    "{name}: pass={} fail={} skip={} (rust: invalid-ok={} invalid-bad={})",
                    stats.pass, stats.fail, stats.skip, stats.rust_pass, stats.rust_fail
                );
                total.pass += stats.pass;
                total.fail += stats.fail;
                total.skip += stats.skip;
                total.rust_pass += stats.rust_pass;
                total.rust_fail += stats.rust_fail;
                let expected = EXPECTED_FAILURES
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, count)| *count)
                    .unwrap_or(0);
                if stats.fail != expected {
                    failures.push(format!(
                        "{name}: {} assertion failures (expected {expected})",
                        stats.fail
                    ));
                }
            }
            Err(err) => failures.push(format!("{name}: {err:#}")),
        }
    }
    println!(
        "TOTAL: pass={} fail={} skip={} (rust: invalid-ok={} invalid-bad={})",
        total.pass, total.fail, total.skip, total.rust_pass, total.rust_fail
    );
    assert!(failures.is_empty(), "spec failures:\n{}", failures.join("\n"));
}

#[derive(Default)]
struct Stats {
    pass: u32,
    fail: u32,
    skip: u32,
    /// assert_invalid / assert_malformed handled on the Rust side
    rust_pass: u32,
    rust_fail: u32,
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
    /// Ruby global variable holding the most recent instance, or an error
    /// message when the most recent module failed to convert.
    current: Result<String, String>,
    /// Same, for named modules.
    named: std::collections::HashMap<String, Result<String, String>>,
    counter: u32,
    stats: Stats,
    /// Union of the runtime units needed by all converted modules; the
    /// script gets one shared `module Rt` bundle (minimal, so undeclared
    /// unit dependencies fail loudly).
    units: BTreeSet<String>,
}

impl<'a> ScriptGen<'a> {
    fn desc(&self, span: wast::token::Span) -> String {
        let (line, _) = span.linecol_in(self.source);
        format!("{}.wast:{}", self.file, line + 1)
    }

    fn skip(&mut self) {
        self.stats.skip += 1;
        // Counted at script generation time, so add it to the script totals
        // via a comment only; the Ruby-side skip counter is separate.
        self.script.push_str("$skip += 1\n");
    }

    fn instance_for(&self, module: Option<&str>) -> Result<String, String> {
        match module {
            Some(name) => self
                .named
                .get(name)
                .cloned()
                .unwrap_or_else(|| Err(format!("unknown module ${name}"))),
            None => self.current.clone(),
        }
    }

    fn define_module(&mut self, mut qw: QuoteWat<'a>) {
        let id = match &qw {
            QuoteWat::Wat(Wat::Module(m)) => m.id.map(|i| i.name().to_string()),
            _ => None,
        };
        let converted = qw
            .encode()
            .map_err(|e| e.to_string())
            .and_then(|bytes| convert(&bytes, self.counter));
        self.counter += 1;
        let result = match converted {
            Ok((class_src, class_name, units)) => {
                let var = format!("$i{}", self.counter);
                self.script.push_str(&class_src);
                let _ = writeln!(self.script, "{var} = {class_name}.new($spectest)");
                self.units.extend(units);
                Ok(var)
            }
            Err(err) => {
                eprintln!("module at {}.wast (#{}) failed to convert: {err}", self.file, self.counter);
                Err(err)
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

fn convert(bytes: &[u8], counter: u32) -> Result<(String, String, BTreeSet<String>), String> {
    let module = dewasmify_core::build_module(bytes).map_err(|e| format!("{e:#}"))?;
    let class_name = format!("WastMod{counter}");
    let (src, units) = dewasmify_backend_ruby::generate_class_with_units(
        &module,
        &class_name,
        &RuntimeLinkage::Alias("::Rt".to_string()),
    )
    .map_err(|e| format!("{e:#}"))?;
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
        other => Err(format!("unsupported argument {other:?}")),
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
        other => Err(format!("unsupported result {other:?}")),
    }
}

fn run_file(name: &str, path: &Path) -> anyhow::Result<Stats> {
    let source = std::fs::read_to_string(path)?;
    let buf = ParseBuffer::new(&source)?;
    let wast: Wast = parser::parse(&buf)?;

    let mut gen = ScriptGen {
        script: String::new(),
        source: &source,
        file: name,
        current: Err("no module defined yet".to_string()),
        named: Default::default(),
        counter: 0,
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
            WastDirective::Module(qw) => gen.define_module(qw),
            WastDirective::AssertReturn { exec, results, .. } => {
                let call = match &exec {
                    WastExecute::Invoke(inv) => gen.invoke_expr(inv),
                    WastExecute::Get { module, global, .. } => gen
                        .instance_for(module.map(|i| i.name()))
                        .map(|var| format!("{var}.global_get({})", ruby_str(global))),
                    WastExecute::Wat(_) => Err("assert_return with module".to_string()),
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
                    _ => gen.skip(),
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
                            .map_err(|e| e.to_string())
                            .and_then(|bytes| convert(&bytes, counter));
                        match converted {
                            Ok((class_src, class_name, units)) => {
                                gen.script.push_str(&class_src);
                                gen.units.extend(units);
                                Ok(format!("{class_name}.new($spectest)"))
                            }
                            Err(e) => Err(e),
                        }
                    }
                    _ => Err("unsupported assert_trap form".to_string()),
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
                    Err(_) => gen.skip(),
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
                Err(_) => gen.skip(),
            },
            WastDirective::Invoke(inv) => match gen.invoke_expr(&inv) {
                Ok(call) => {
                    let _ = writeln!(
                        gen.script,
                        "check({}) do\n  {call}\n  [true, nil]\nend",
                        ruby_str(&desc)
                    );
                }
                Err(_) => gen.skip(),
            },
            WastDirective::AssertInvalid { mut module, .. }
            | WastDirective::AssertMalformed { mut module, .. } => {
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
            _ => gen.skip(),
        }
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
            let _ = parts.next(); // "skip"
            stats.skip += parts.next().unwrap_or("0").parse().unwrap_or(0);
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
$skip = 0

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
puts "RESULT pass=#{$pass} fail=#{$fail} skip=#{$skip}"
"#;
