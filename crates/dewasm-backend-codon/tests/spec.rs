//! Codon side of the shared spec harness: converts each module with the Codon backend, phrases every assertion as a Codon thunk plus a `check*` helper call, assembles one self-contained program per `.wast` file, and `codon build`s + runs it.
//! The generic harness lives in `dewasm-test-helper`.
//!
//! Builds are debug (see tests/common).
//!
//! Codon facts that shape the phrasing:
//! - Codon is statically typed with no dynamic `invoke`, so each generated class carries a boxed `invoke(name, List[Val]) -> List[Val]` / `global_get(name) -> List[Val]` dispatcher, and the harness compares boxed results bit-exactly through `Rt.f32_bits`/`Rt.f64_bits`.
//! - Assertions live in named thunks (`def _tN(): ...`) rather than one flat run: a single module-level run of thousands of statements would form the megafunction shape whose `-release` compile time is superlinear.
//! - A runaway recursion overflows the native stack fatally (uncatchable), so the spec build instruments every generated function with a recursion guard that turns exhaustion into a catchable "call stack exhausted" trap.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

use dewasm_backend::Backend;
use dewasm_backend_codon::{
    codon_string, i32_const as u32_lit, i64_const as u64_lit, CodonBackend,
};
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{AbstractHeapType, HeapType, NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

mod common;

/// Known assertion-level failures with their attribution; the file still runs so regressions in the passing assertions are caught.
///
/// - `import-limits`: import resolution checks the kind, a function's structural signature (the `Extern.fn_ty` key) and a global's value type (the typed `Extern` field), but not a global's mutability, a table/memory's min/max limits, nor a tag's parameter types (a tag is an identity object carrying no type at all).
///   Every `assert_unlinkable` case testing one of those stays a known gap; the counts match the Go backend's, whose type assertion covers the same surface.
/// - `linking` (`linking0`/`load1`): downstream of an *unrelated* declared-unsupported feature (multi-memory) inside a module that also uses `register`; that module never converts, so a later assertion against the module it would have written into observes stale state.
///   Not a cross-module-linking gap itself.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 34, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 2, "import-limits"),
    ("linking0", 1, "linking"),
    ("load1", 5, "linking"),
];

/// The pull-request tier: a small cross-section of cheap files, a semantic area each.
const FAST_SPEC_FILES: &[&str] = &[
    "block",
    "br_if",
    "call",
    "endianness",
    "fac",
    "forward",
    "nop",
    "select",
    "switch",
    "traps",
];

/// What `slow_test` adds on top of [`FAST_SPEC_FILES`] (the union is built in `curated_files`, so the slow tier is a superset by construction): the files covering this backend's own risk areas, sized by measured fresh-build cost.
/// Native unsigned arithmetic (`i32`/`i64`), the float conversion and bit paths (`conversions`/`f32_bitwise`), funcref tables and the element literal (`elem`/`call_indirect`), bulk memory (`memory_fill`), globals, and the boxed import boundary with its failure-ledger rows (`imports`).
/// The float arithmetic files (`f32`/`f64`, ~17 s each) and `memory_copy` (~27 s, the memmove overlap coverage) measured as the three heaviest builds in the suite and run only in the ultra sweep; `memory_grow` is multi-memory-first upstream, so nearly all of it skips and it covers nothing here.
const SLOW_EXTRA_SPEC_FILES: &[&str] = &[
    "call_indirect",
    "conversions",
    "elem",
    "f32_bitwise",
    "global",
    "i32",
    "i64",
    "imports",
    "memory_fill",
];

/// Thunk names must be unique per assembled file; a process-wide counter is unique across every file, which is enough.
static THUNK: AtomicU64 = AtomicU64::new(0);

fn thunk_name() -> String {
    format!("_t{}", THUNK.fetch_add(1, Ordering::Relaxed))
}

pub struct CodonSpec;

impl BackendUnderTest for CodonSpec {
    fn name(&self) -> &'static str {
        "codon"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &CodonBackend
    }

    /// Compile `source` to the crate's shared cache binary (identical programs build once; debug by default, see tests/common) and run it with the Codon runtime dylibs on the loader path.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        match common::build_codon(source) {
            Err(build) => build,
            Ok(bin) => common::run_codon_binary(&bin, args, stdin),
        }
    }
}

fn arg_codon(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok(format!("Rt.Val.of_i32({})", u32_lit(*v as u32))),
        WastArg::Core(WastArgCore::I64(v)) => Ok(format!("Rt.Val.of_i64({})", u64_lit(*v as u64))),
        WastArg::Core(WastArgCore::F32(f)) => Ok(format!(
            "Rt.Val.of_f32(Rt.f32_from_bits({}))",
            u32_lit(f.bits)
        )),
        WastArg::Core(WastArgCore::F64(f)) => Ok(format!(
            "Rt.Val.of_f64(Rt.f64_from_bits({}))",
            u64_lit(f.bits)
        )),
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(WastArgCore::RefNull(hty)) => match null_exn(hty) {
            Some(v) => Ok(v),
            None => Err(dewasm_test_helper::heap_type_tag(hty)),
        },
        WastArg::Core(WastArgCore::RefExtern(_)) => Err("reference-types".to_string()),
        WastArg::Core(WastArgCore::RefHost(_)) => Err("reference-types".to_string()),
        _ => Err("component-model".to_string()),
    }
}

fn ret_cmp(value: &str, ret: &WastRet<'_>) -> Result<String, String> {
    match ret {
        WastRet::Core(WastRetCore::I32(v)) => {
            Ok(format!("{value}.i32() == {}", u32_lit(*v as u32)))
        }
        WastRet::Core(WastRetCore::I64(v)) => {
            Ok(format!("{value}.i64() == {}", u64_lit(*v as u64)))
        }
        WastRet::Core(WastRetCore::F32(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => format!(
                "(Rt.f32_bits({value}.f32()) & UInt[32](0x7FFFFFFF)) == UInt[32](0x7FC00000)"
            ),
            NanPattern::ArithmeticNan => format!(
                "(Rt.f32_bits({value}.f32()) & UInt[32](0x7FC00000)) == UInt[32](0x7FC00000)"
            ),
            NanPattern::Value(f) => {
                format!("Rt.f32_bits({value}.f32()) == {}", u32_lit(f.bits))
            }
        }),
        WastRet::Core(WastRetCore::F64(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => format!(
                "(Rt.f64_bits({value}.f64()) & UInt[64](0x7FFFFFFFFFFFFFFF)) == UInt[64](0x7FF8000000000000)"
            ),
            NanPattern::ArithmeticNan => format!(
                "(Rt.f64_bits({value}.f64()) & UInt[64](0x7FF8000000000000)) == UInt[64](0x7FF8000000000000)"
            ),
            NanPattern::Value(f) => {
                format!("Rt.f64_bits({value}.f64()) == {}", u64_lit(f.bits))
            }
        }),
        WastRet::Core(WastRetCore::V128(_)) => Err("simd".to_string()),
        WastRet::Core(WastRetCore::Either(_)) => Err("either-results".to_string()),
        WastRet::Core(WastRetCore::RefNull(Some(hty))) if null_exn(hty).is_some() => {
            Ok(format!("{value}.exn() is None"))
        }
        WastRet::Core(WastRetCore::RefNull(Some(hty))) => {
            Err(dewasm_test_helper::heap_type_tag(hty))
        }
        WastRet::Core(WastRetCore::RefNull(None)) => Err("reference-types".to_string()),
        WastRet::Core(WastRetCore::RefExtern(_)) => Err("reference-types".to_string()),
        WastRet::Core(WastRetCore::RefHost(_)) => Err("reference-types".to_string()),
        WastRet::Core(WastRetCore::RefFunc(_)) => Err("reference-types".to_string()),
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

/// The boxed null exnref, the one `ref.null` host value this backend can express.
fn null_exn(hty: &HeapType<'_>) -> Option<String> {
    match hty {
        HeapType::Abstract {
            ty: AbstractHeapType::Exn | AbstractHeapType::NoExn,
            ..
        } if dewasm_test_helper::nullable_heap_type(hty) => Some("Rt.Val.of_exn(None)".to_string()),
        _ => None,
    }
}

/// `_spectest`, plus any currently-`register`ed instances merged in under their registered name: each instance's `exports` dict doubles as an import source.
fn imports_expr(registered: &[(String, String)]) -> String {
    let mut entries = vec!["\"spectest\": _spectest".to_string()];
    for (name, var) in registered {
        entries.push(format!("{}: {var}.exports", codon_string(name)));
    }
    format!("{{{}}}", entries.join(", "))
}

impl dewasm_test_helper::SpecBackend for CodonSpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    /// Codon compiles each `.wast` file to one program, so the tier sizes are set by compile latency, not run time: a plain `cargo test` (the pull-request tier, target: ~3 minutes) runs [`FAST_SPEC_FILES`], `--features slow_test` (CI's main-branch lane, target: ~5 minutes all in; the shared curated list measured 268 seconds there, over half the lane) adds [`SLOW_EXTRA_SPEC_FILES`] and the exception-handling and tail-call files, and the full testsuite runs only under `--features ultra_slow_test`.
    fn curated_files(&self) -> Option<&'static [&'static str]> {
        if cfg!(feature = "ultra_slow_test") {
            None
        } else if cfg!(feature = "slow_test") {
            static SLOW: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
            Some(SLOW.get_or_init(|| {
                FAST_SPEC_FILES
                    .iter()
                    .chain(SLOW_EXTRA_SPEC_FILES)
                    .chain(dewasm_test_helper::EXCEPTION_HANDLING_SPEC_FILES)
                    .chain(dewasm_test_helper::TAIL_CALL_SPEC_FILES)
                    .copied()
                    .collect()
            }))
        } else {
            Some(FAST_SPEC_FILES)
        }
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[
            // check_trap / check_exhaust / check_unlinkable match these classes.
            "rt/trap",
            "rt/link_error",
            "rt/exit",
            // Boxed values and bit-exact float comparisons.
            "rt/boxed",
            "rt/f32_bits",
            "rt/f32_from_bits",
            "rt/f64_bits",
            "rt/f64_from_bits",
            // Referenced by the _spectest fixture (PREAMBLE), not necessarily by the converted module itself.
            "ext/extern",
            "global/_class",
            "table/_class",
            "memory/_class",
        ]
    }

    fn generate(
        &self,
        module: &ir::Module,
        counter: u32,
    ) -> anyhow::Result<dewasm_test_helper::Converted> {
        let class_name = format!("WastMod{counter}");
        let (source, units) =
            dewasm_backend_codon::generate_spec_class_with_units(module, &class_name)?;
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
        decls: &mut String,
        conv: &dewasm_test_helper::Converted,
        var_id: u32,
        registered: &[(String, String)],
    ) -> String {
        decls.push_str(&conv.source);
        decls.push('\n');
        let var = format!("_i{var_id}");
        let _ = writeln!(
            script,
            "{var} = {}({}, List[str](), Dict[str, str](), Dict[str, str]())",
            conv.handle,
            imports_expr(registered)
        );
        var
    }

    fn instantiate_call(
        &self,
        script: &mut String,
        decls: &mut String,
        conv: &dewasm_test_helper::Converted,
        registered: &[(String, String)],
    ) -> String {
        let _ = script;
        decls.push_str(&conv.source);
        decls.push('\n');
        format!(
            "{}({}, List[str](), Dict[str, str](), Dict[str, str]())",
            conv.handle,
            imports_expr(registered)
        )
    }

    fn invoke(&self, var: &str, name: &str, args: &[WastArg<'_>]) -> Result<String, String> {
        let mut vals = Vec::new();
        for arg in args {
            vals.push(arg_codon(arg)?);
        }
        let list = if vals.is_empty() {
            "List[Rt.Val]()".to_string()
        } else {
            format!("[{}]", vals.join(", "))
        };
        Ok(format!("{var}.invoke({}, {list})", codon_string(name)))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}.global_get({})", codon_string(global))
    }

    fn emit_check(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
        results: &[WastRet<'_>],
    ) -> Result<(), String> {
        let name = thunk_name();
        let _ = writeln!(script, "def {name}() -> bool:");
        if results.is_empty() {
            let _ = writeln!(script, "\t{call}");
            let _ = writeln!(script, "\treturn True");
        } else {
            let _ = writeln!(script, "\t__r = {call}");
            let mut parts = Vec::new();
            for (i, r) in results.iter().enumerate() {
                parts.push(ret_cmp(&format!("__r[{i}]"), r)?);
            }
            let _ = writeln!(script, "\treturn {}", parts.join(" and "));
        }
        let _ = writeln!(script, "check({}, {name})", codon_string(desc));
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let name = thunk_name();
        let _ = writeln!(script, "def {name}():");
        let _ = writeln!(script, "\t{call}");
        let _ = writeln!(
            script,
            "check_trap({}, {}, {name})",
            codon_string(desc),
            codon_string(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        let name = thunk_name();
        let _ = writeln!(script, "def {name}():");
        let _ = writeln!(script, "\t{call}");
        let _ = writeln!(script, "check_exhaust({}, {name})", codon_string(desc));
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let name = thunk_name();
        let _ = writeln!(script, "def {name}():");
        let _ = writeln!(script, "\t{call}");
        let _ = writeln!(script, "check_ok({}, {name})", codon_string(desc));
    }

    fn emit_check_exception(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
    ) -> Result<(), String> {
        let name = thunk_name();
        let _ = writeln!(script, "def {name}():");
        let _ = writeln!(script, "\t{call}");
        let _ = writeln!(script, "check_exception({}, {name})", codon_string(desc));
        Ok(())
    }

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let name = thunk_name();
        let _ = writeln!(script, "def {name}():");
        let _ = writeln!(script, "\t{call}");
        let _ = writeln!(script, "check_unlinkable({}, {name})", codon_string(desc));
    }

    fn assemble(
        &self,
        units: &BTreeSet<String>,
        decls: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let bundle = dewasm_backend_codon::bundler()
            .bundle(units, 1)
            .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?;

        let mut out = String::from("# Generated by the dewasm spec harness. Do not edit.\n");
        out.push_str("class Rt:\n");
        out.push_str(&bundle);
        out.push_str("\n\n");
        out.push_str(PREAMBLE);
        out.push('\n');
        out.push_str(decls);
        out.push('\n');
        out.push_str(body);
        out.push_str("\nprint(\"RESULT pass=\" + str(_pass) + \" fail=\" + str(_fail))\n");
        Ok(out)
    }
}

/// Harness helpers + the `spectest` host fixture.
/// `_rt_stack` is the recursion guard's shared counter (referenced only by spec-build generated functions).
const PREAMBLE: &str = r#"_rt_stack = 0
_pass = 0
_fail = 0

def check(desc: str, f):
	global _pass, _fail
	try:
		ok = f()
	except BaseException as e:
		_fail += 1
		print("FAIL(panic " + str(e.message) + "): " + desc)
		return
	if ok:
		_pass += 1
	else:
		_fail += 1
		print("FAIL: " + desc)

def check_ok(desc: str, f):
	global _pass, _fail
	try:
		f()
	except BaseException as e:
		_fail += 1
		print("FAIL(panic " + str(e.message) + "): " + desc)
		return
	_pass += 1

def check_trap(desc: str, msg: str, f):
	global _pass, _fail
	try:
		f()
	except Rt.Trap as e:
		if msg in e.message or e.message in msg:
			_pass += 1
		else:
			_fail += 1
			print("FAIL(trap " + e.message + ", want " + msg + "): " + desc)
		return
	except BaseException as e:
		_fail += 1
		print("FAIL(panic " + str(e.message) + ", want trap " + msg + "): " + desc)
		return
	_fail += 1
	print("FAIL(no trap, want " + msg + "): " + desc)

def check_exhaust(desc: str, f):
	global _pass, _fail
	try:
		f()
	except Rt.Trap as e:
		if e.message == "call stack exhausted":
			_pass += 1
		else:
			_fail += 1
			print("FAIL(trap " + e.message + ", want exhaustion): " + desc)
		return
	except BaseException as e:
		_fail += 1
		print("FAIL(panic " + str(e.message) + ", want exhaustion): " + desc)
		return
	_fail += 1
	print("FAIL(no exhaustion): " + desc)

def check_exception(desc: str, f):
	global _pass, _fail
	try:
		f()
	except Rt.WasmException:
		_pass += 1
		return
	except BaseException as e:
		_fail += 1
		print("FAIL(panic " + str(e.message) + ", want a wasm exception): " + desc)
		return
	_fail += 1
	print("FAIL(no exception): " + desc)

def check_unlinkable(desc: str, f):
	global _pass, _fail
	try:
		f()
	except Rt.LinkError:
		_pass += 1
		return
	except BaseException as e:
		_fail += 1
		print("FAIL(non-link error " + str(e.message) + ", want unlinkable): " + desc)
		return
	_fail += 1
	print("FAIL(no error, want unlinkable): " + desc)

class _SpectestFn(Rt.Fn):
	def __init__(self):
		pass
	def invoke(self, a: List[Rt.Val]) -> List[Rt.Val]:
		return List[Rt.Val]()

def _mk_spectest() -> Dict[str, Rt.Extern]:
	d = Dict[str, Rt.Extern]()
	d["print"] = Rt.Extern.of_fn(_SpectestFn(), "->")
	d["print_i32"] = Rt.Extern.of_fn(_SpectestFn(), "i32->")
	d["print_i64"] = Rt.Extern.of_fn(_SpectestFn(), "i64->")
	d["print_f32"] = Rt.Extern.of_fn(_SpectestFn(), "f32->")
	d["print_f64"] = Rt.Extern.of_fn(_SpectestFn(), "f64->")
	d["print_i32_f32"] = Rt.Extern.of_fn(_SpectestFn(), "i32,f32->")
	d["print_f64_f64"] = Rt.Extern.of_fn(_SpectestFn(), "f64,f64->")
	d["global_i32"] = Rt.Extern.of_global_i32(Rt.Global[UInt[32]](UInt[32](666)))
	d["global_i64"] = Rt.Extern.of_global_i64(Rt.Global[UInt[64]](UInt[64](666)))
	d["global_f32"] = Rt.Extern.of_global_f32(Rt.Global[float32](float32(666.6)))
	d["global_f64"] = Rt.Extern.of_global_f64(Rt.Global[float](666.6))
	d["table"] = Rt.Extern.of_table(Rt.Table(10, 20))
	d["memory"] = Rt.Extern.of_memory(Rt.Memory(1, 2))
	return d

_spectest = _mk_spectest()
"#;

fn main() {
    dewasm_test_helper::spec_main(&CodonSpec, cfg!(feature = "ultra_slow_test"));
}
