//! Ruby side of the spec harness: converts modules with the Ruby backend,
//! phrases assertions as Ruby (`check`/`check_trap`/`check_exhaust`
//! helpers, bit-exact float comparison via `Rt.f32_bits`/`Rt.f64_bits`),
//! and runs the script with the `ruby` on PATH.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use dewasmify_backend::{Backend, RuntimeLinkage};
use dewasmify_backend_ruby::RubyBackend;
use dewasmify_core::ir;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

use crate::{Converted, SpecLang};

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

pub struct RubyLang;

impl SpecLang for RubyLang {
    fn name(&self) -> &'static str {
        "ruby"
    }

    fn backend(&self) -> &dyn Backend {
        &RubyBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasmify_backend_ruby::find_ruby().expect("ruby not found on PATH — see docs/testing.md")
    }

    fn script_ext(&self) -> &'static str {
        "rb"
    }

    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    fn default_files(&self) -> Option<&'static [&'static str]> {
        None
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[
            "rt/trap",
            "rt/f32_bits",
            "rt/f32_from_bits",
            "rt/f64_bits",
            "rt/f64_from_bits",
        ]
    }

    fn generate(&self, module: &ir::Module, counter: u32) -> anyhow::Result<Converted> {
        let class_name = format!("WastMod{counter}");
        let (source, units) = dewasmify_backend_ruby::generate_class_with_units(
            module,
            &class_name,
            &RuntimeLinkage::Alias("::Rt".to_string()),
            false, // spec modules import spectest, never WASI
        )?;
        Ok(Converted {
            source,
            handle: class_name,
            units,
        })
    }

    fn emit_instantiate(&self, script: &mut String, conv: &Converted, var_id: u32) -> String {
        let var = format!("$i{var_id}");
        script.push_str(&conv.source);
        let _ = writeln!(script, "{var} = {}.new($spectest)", conv.handle);
        var
    }

    fn instantiate_call(&self, script: &mut String, conv: &Converted) -> String {
        script.push_str(&conv.source);
        format!("{}.new($spectest)", conv.handle)
    }

    fn invoke(&self, var: &str, name: &str, args: &[WastArg<'_>]) -> Result<String, String> {
        let mut parts = vec![ruby_str(name)];
        for arg in args {
            parts.push(arg_rb(arg)?);
        }
        Ok(format!("{var}.invoke({})", parts.join(", ")))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}.global_get({})", ruby_str(global))
    }

    fn emit_check(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
        results: &[WastRet<'_>],
    ) -> Result<(), String> {
        let cmp = match results.len() {
            0 => "true".to_string(),
            1 => ret_cmp("__r", &results[0])?,
            _ => {
                let mut parts = Vec::new();
                for (i, r) in results.iter().enumerate() {
                    parts.push(ret_cmp(&format!("__r[{i}]"), r)?);
                }
                parts.join(" && ")
            }
        };
        let _ = writeln!(
            script,
            "check({}) do\n  __r = {call}\n  [({cmp}), __r]\nend",
            ruby_str(desc)
        );
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let _ = writeln!(
            script,
            "check_trap({}, {}) do\n  {call}\nend",
            ruby_str(desc),
            ruby_str(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_exhaust({}) do\n  {call}\nend",
            ruby_str(desc)
        );
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check({}) do\n  {call}\n  [true, nil]\nend",
            ruby_str(desc)
        );
    }

    fn assemble(&self, units: &BTreeSet<String>, body: &str) -> anyhow::Result<String> {
        let mut script = dewasmify_backend_ruby::shared_runtime(units)
            .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?;
        script.push_str(PREAMBLE);
        script.push_str(body);
        script.push_str(POSTAMBLE);
        Ok(script)
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

fn arg_rb(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok((*v as u32).to_string()),
        WastArg::Core(WastArgCore::I64(v)) => Ok((*v as u64).to_string()),
        WastArg::Core(WastArgCore::F32(f)) => Ok(format!("Rt.f32_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::F64(f)) => Ok(format!("Rt.f64_from_bits(0x{:x})", f.bits)),
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
                format!("(Rt.f64_bits({value}) & 0x7fffffffffffffff) == 0x7ff8000000000000")
            }
            NanPattern::ArithmeticNan => {
                format!("(Rt.f64_bits({value}) & 0x7ff8000000000000) == 0x7ff8000000000000")
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
