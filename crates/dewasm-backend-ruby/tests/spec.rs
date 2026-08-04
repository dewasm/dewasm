//! Ruby side of the shared spec harness (ADR-3, ADR-27): converts modules with the Ruby backend, phrases assertions as Ruby (`check`/`check_trap`/ `check_exhaust` helpers, bit-exact float comparison via `Rt.f32_bits`/ `Rt.f64_bits`), and runs the script with the `ruby` on PATH. The generic harness lives in `dewasm-test-helper`.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use dewasm_backend::{Backend, RuntimeLinkage};
use dewasm_backend_ruby::RubyBackend;
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{AbstractHeapType, HeapType, NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

/// Known assertion-level failures with their attribution; the file still runs so regressions in the passing assertions are caught.
///
/// - `import-limits`: `Rt.check_import_kind` validates that a resolved import is the right *kind* (func/global/table/memory/tag) but not the finer-grained wasm type — a function's param/result types, a global's mutability, a tag's parameter types, or a table/memory's min/max limits against the import site's declared bounds. Every `assert_unlinkable` case testing one of those (not a kind mismatch, which is caught) stays a known gap. The imports.wast count reverted 59 → 28 when exception handling was removed (ADR-24): its "test" fixture module exports tags, so it no longer converts — the tag export is now rejected at conversion time — and the downstream type-mismatch checks it exposed are no longer reached.
/// - `linking` (module `linking0`/`load1`): downstream of an *unrelated* declared-unsupported feature (multi-memory) inside a module that also happens to use `register`; that module never converts, so a later assertion against the module it would have written into observes stale state. Not a cross-module-linking gap itself.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 28, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 4, "import-limits"),
    ("linking0", 1, "linking"),
    ("load1", 5, "linking"),
];

pub struct RubySpec;

impl BackendUnderTest for RubySpec {
    fn name(&self) -> &'static str {
        "ruby"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &RubyBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasm_backend_ruby::find_ruby().expect("ruby not found on PATH — see docs/testing.md")
    }
}

impl dewasm_test_helper::SpecBackend for RubySpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    fn curated_files(&self) -> Option<&'static [&'static str]> {
        // Ruby is fast enough to run the whole testsuite by default; nothing is marked ignored.
        None
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[
            "rt/trap",
            // check_unlinkable's rescue clause references Rt::LinkError even when the converted modules themselves don't.
            "rt/link_error",
            "rt/f32_bits",
            "rt/f32_from_bits",
            "rt/f64_bits",
            "rt/f64_from_bits",
            // Referenced by the $spectest fixture (PREAMBLE below), not necessarily by the converted module itself.
            "global/_class",
            "table/_class",
            "memory/_class",
            "rt/f32",
        ]
    }

    fn generate(
        &self,
        module: &ir::Module,
        counter: u32,
    ) -> anyhow::Result<dewasm_test_helper::Converted> {
        let class_name = format!("WastMod{counter}");
        let (source, units) = dewasm_backend_ruby::generate_class_with_units(
            module,
            &class_name,
            &RuntimeLinkage::Alias("::Rt".to_string()),
            false, // spec modules import spectest, never WASI
        )?;
        Ok(dewasm_test_helper::Converted {
            source,
            handle: class_name,
            units,
        })
    }

    fn supports_registered_imports(&self) -> bool {
        true
    }

    fn emit_instantiate(
        &self,
        script: &mut String,
        _decls: &mut String,
        conv: &dewasm_test_helper::Converted,
        var_id: u32,
        registered: &[(String, String)],
    ) -> String {
        let var = format!("$i{var_id}");
        script.push_str(&conv.source);
        let _ = writeln!(
            script,
            "{var} = {}.new({})",
            conv.handle,
            imports_expr(registered)
        );
        var
    }

    fn instantiate_call(
        &self,
        script: &mut String,
        _decls: &mut String,
        conv: &dewasm_test_helper::Converted,
        registered: &[(String, String)],
    ) -> String {
        script.push_str(&conv.source);
        format!("{}.new({})", conv.handle, imports_expr(registered))
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

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_unlinkable({}) do\n  {call}\nend",
            ruby_str(desc)
        );
    }

    fn assemble(
        &self,
        units: &BTreeSet<String>,
        _decls: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let mut script = dewasm_backend_ruby::shared_runtime(units)
            .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?;
        script.push_str(PREAMBLE);
        script.push_str(body);
        script.push_str(POSTAMBLE);
        Ok(script)
    }
}

/// `$spectest`, plus any currently-`register`ed instances merged in under their registered name — each instance doubles as an ADR-7 import provider (`Gen::body`'s generated `import(name)` method).
fn imports_expr(registered: &[(String, String)]) -> String {
    if registered.is_empty() {
        return "$spectest".to_string();
    }
    let entries = registered
        .iter()
        .map(|(name, var)| format!("{} => {var}", ruby_str(name)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("$spectest.merge({{ {entries} }})")
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

/// Attribution for a null/ref heap type the harness cannot express as a Ruby value; the two reference-types hierarchies (and their bottoms, which are also just `nil`) are expressible.
fn heap_type_tag(hty: &HeapType<'_>) -> String {
    match hty {
        HeapType::Abstract {
            ty: AbstractHeapType::Exn | AbstractHeapType::NoExn,
            ..
        } => "exception-handling".to_string(),
        HeapType::Abstract { .. } => "gc".to_string(),
        HeapType::Concrete(_) | HeapType::Exact(_) => "function-references".to_string(),
    }
}

fn nullable_heap_type(hty: &HeapType<'_>) -> bool {
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

fn arg_rb(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok((*v as u32).to_string()),
        WastArg::Core(WastArgCore::I64(v)) => Ok((*v as u64).to_string()),
        WastArg::Core(WastArgCore::F32(f)) => Ok(format!("Rt.f32_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::F64(f)) => Ok(format!("Rt.f64_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(WastArgCore::RefNull(hty)) => {
            if nullable_heap_type(hty) {
                Ok("nil".to_string())
            } else {
                Err(heap_type_tag(hty))
            }
        }
        // An externref (or legacy hostref) with identity `n`: the host value is the Integer itself (ADR-17: externref = raw host value).
        WastArg::Core(WastArgCore::RefExtern(n)) => Ok(n.to_string()),
        WastArg::Core(WastArgCore::RefHost(n)) => Ok(n.to_string()),
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
        WastRet::Core(WastRetCore::RefNull(hty)) => match hty {
            None => Ok(format!("{value}.nil?")),
            Some(hty) if nullable_heap_type(hty) => Ok(format!("{value}.nil?")),
            Some(hty) => Err(heap_type_tag(hty)),
        },
        WastRet::Core(WastRetCore::RefExtern(Some(n))) => Ok(format!("{value} == {n}")),
        // `(ref.extern)`: any non-null externref.
        WastRet::Core(WastRetCore::RefExtern(None)) => Ok(format!("!{value}.nil?")),
        WastRet::Core(WastRetCore::RefHost(n)) => Ok(format!("{value} == {n}")),
        // `(ref.func)`: any non-null funcref — in ADR-17's representation, a `[type_symbol, callable]` pair.
        WastRet::Core(WastRetCore::RefFunc(None)) => Ok(format!(
            "({value}.is_a?(Array) && {value}[0].is_a?(Symbol))"
        )),
        // A specific function's identity: not expressible without an export map; no top-level testsuite file uses it.
        WastRet::Core(WastRetCore::RefFunc(Some(_))) => Err("funcref-identity".to_string()),
        WastRet::Core(
            WastRetCore::RefAny
            | WastRetCore::RefEq
            | WastRetCore::RefArray
            | WastRetCore::RefStruct
            | WastRetCore::RefI31
            | WastRetCore::RefI31Shared,
        ) => Err("gc".to_string()),
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

# Upstream's assert_unlinkable message text never matches ours (we don't
# reproduce wasm engines' wording); a raised Rt::LinkError confirms the
# import was correctly rejected as unlinkable. Any other error means the
# module *linked* and then crashed — that must not count as a pass.
def check_unlinkable(desc)
  yield
  $fail += 1
  puts "FAIL(no error, want unlinkable): #{desc}"
rescue Rt::LinkError
  $pass += 1
rescue => e
  $fail += 1
  puts "FAIL(non-link error, want unlinkable): #{desc}: #{e.class}: #{e.message}"
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
    "global_i32" => Rt::Global.new(666),
    "global_i64" => Rt::Global.new(666),
    "global_f32" => Rt::Global.new(Rt.f32(666.6)),
    "global_f64" => Rt::Global.new(666.6),
    "table" => Rt::Table.new(10, 20),
    "memory" => Rt::Memory.new(1, 2),
  },
}
"#;

const POSTAMBLE: &str = r#"
puts "RESULT pass=#{$pass} fail=#{$fail}"
"#;

dewasm_test_helper::spec_suite!(RubySpec);
