//! Go side of the shared spec harness: converts each module with the Go backend to package-level declarations, phrases every assertion as compiled Go (`check`/`check_trap`/`check_exhaust`/ `check_unlinkable`, bit-exact float comparison via `math.Float32bits`/ `math.Float64bits`), assembles one self-contained program per `.wast` file, and `go build`s + runs it. The generic harness lives in `dewasm-test-helper`.
//!
//! Three Go facts shape the phrasing:
//! - Go is statically typed and has no dynamic `invoke`, so each generated type carries a reflective `invoke(name, args...) []any` / `globalGet(name) any` dispatcher (built where the module, and hence every export's signature, is known); the harness asserts the boxed `any` results to the expected type.
//! - Type/method declarations cannot live inside `func main`, so per-module `Converted.source` is accumulated at package scope in the harness's file-scoped `decls` buffer (hoisted ahead of the body by `assemble`) while only instantiation/assertion statements go in the body.
//! - A runaway recursion overflows Go's goroutine stack *fatally* (uncatchable, killing the process), so the spec build instruments every generated function with a recursion guard that turns exhaustion into a catchable "call stack exhausted" trap the harness observes.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_go::{go_string, GoBackend};
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

mod common;

/// Known assertion-level failures with their attribution; the file still runs so regressions in the passing assertions are caught.
///
/// - `import-limits`: the Go type assertion that resolves an import checks its *kind* (func/global/table/memory) and, for functions and globals, the full value/signature type too, but not a global's mutability, nor a table/memory's min/max limits, against the import site's declared bounds. Every `assert_unlinkable` case testing one of those stays a known gap. The counts are *lower* than Ruby/Python's: the Go type assertion catches func-signature and global-value-type mismatches those backends' kind-only check misses, so only the mutability/limit cases remain (the two `linking` failures are both global-mutability mismatches).
/// - `linking` (`linking0`/`load1`): downstream of an *unrelated* declared-unsupported feature (multi-memory) inside a module that also uses `register`; that module never converts, so a later assertion against the module it would have written into observes stale state. Not a cross-module-linking gap itself.
///
/// `skip-stack-guard-page` is *not* here: its `function-with-many-locals` (1056 locals) is the one function in the suite whose frame cost trips the recursion guard even at shallow depth, so all 10 of its exhaustion cases pass.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 28, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 2, "import-limits"),
    ("linking0", 1, "linking"),
    ("load1", 5, "linking"),
];

pub struct GoSpec;

impl BackendUnderTest for GoSpec {
    fn name(&self) -> &'static str {
        "go"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &GoBackend
    }

    /// Compile `source` to the crate's shared cache binary (identical programs build once) and run it. A build failure is surfaced as the `go build` `Output` so the harness reports the compile error where it would report the program's own.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        match common::build_go(source) {
            Err(build) => build,
            Ok(bin) => dewasm_test_helper::run_command_bytes(Command::new(&bin).args(args), stdin),
        }
    }
}

impl dewasm_test_helper::SpecBackend for GoSpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    /// Go compiles each `.wast` file to one program (a few seconds each, dominated by compile latency), so a plain `cargo test` runs only the shared curated list, plus `skip-stack-guard-page`: its `function-with-many-locals` is the one function in the suite whose frame cost trips the recursion guard, so its exhaustion cases are worth running by default.
    fn curated_files(&self) -> Option<&'static [&'static str]> {
        Some(dewasm_test_helper::curated_with(&["skip-stack-guard-page"]))
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[
            // check_trap / check_exhaust / check_unlinkable match these types.
            "rt/trap",
            "rt/link_error",
            // float args are reconstructed bit-exactly; this also pulls in the `math` import the float result comparisons rely on.
            "rt/f32_from_bits",
            "rt/f64_from_bits",
            // Referenced by the _spectest fixture (PREAMBLE), not necessarily by the converted module itself.
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
        let type_name = format!("WastMod{counter}");
        let (source, units) = dewasm_backend_go::generate_program_with_units(module, &type_name)?;
        Ok(dewasm_test_helper::Converted {
            source,
            handle: type_name,
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
        let var = format!("_i{var_id}");
        let _ = writeln!(
            script,
            "{var} := New{}({}, nil, nil, nil)",
            conv.handle,
            imports_expr(registered)
        );
        // A module may be instantiated only for its side effects / to be registered; a never-read local is a Go compile error.
        let _ = writeln!(script, "_ = {var}");
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
        format!(
            "New{}({}, nil, nil, nil)",
            conv.handle,
            imports_expr(registered)
        )
    }

    fn invoke(&self, var: &str, name: &str, args: &[WastArg<'_>]) -> Result<String, String> {
        let mut parts = vec![go_string(name)];
        for arg in args {
            parts.push(arg_go(arg)?);
        }
        Ok(format!("{var}.invoke({})", parts.join(", ")))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}.globalGet({})", go_string(global))
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
            1 => ret_cmp("__r[0]", &results[0])?,
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
            "check({}, func() (bool, any) {{ __r := {call}; return ({cmp}), __r }})",
            go_string(desc)
        );
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let _ = writeln!(
            script,
            "check_trap({}, {}, func() {{ {call} }})",
            go_string(desc),
            go_string(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_exhaust({}, func() {{ {call} }})",
            go_string(desc)
        );
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check({}, func() (bool, any) {{ {call}; return true, nil }})",
            go_string(desc)
        );
    }

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_unlinkable({}, func() {{ {call} }})",
            go_string(desc)
        );
    }

    fn assemble(
        &self,
        units: &BTreeSet<String>,
        decls: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let bundle = dewasm_backend_go::bundler()
            .bundle(units, 0)
            .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?;

        let mut out = String::from("// Generated by the dewasm spec harness. Do not edit.\n");
        out.push_str("package main\n\n");
        out.push_str(&import_block(&scan_imports(&format!(
            "{bundle}\n{PREAMBLE}\n{decls}\n{body}"
        ))));
        out.push('\n');
        out.push_str(&bundle);
        out.push_str("\n\n");
        out.push_str(PREAMBLE);
        out.push('\n');
        out.push_str(decls);
        out.push_str("\nfunc main() {\n");
        for line in body.lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, "\t{line}");
            }
        }
        out.push_str("\tfmt.Printf(\"RESULT pass=%d fail=%d\\n\", _pass, _fail)\n");
        out.push_str("}\n");
        Ok(out)
    }
}

/// The external packages the assembled program references, by the backend's own boundary-matching rule ([`dewasm_backend_go::selector_used`]). Every fragment scanned is controlled (runtime bundle, harness preamble, generated declarations, and a body whose only free-form strings are `file.wast:line` descriptions and wasm trap messages) so no user data can inject a false import. The candidate list stays local: it is what *this* program can reference, and importing more than that is a Go compile error.
fn scan_imports(text: &str) -> Vec<String> {
    let candidates = [
        ("fmt.", "fmt"),
        ("math.", "math"),
        ("bits.", "math/bits"),
        ("binary.", "encoding/binary"),
        ("rand.", "crypto/rand"),
        ("strings.", "strings"),
    ];
    let mut set: BTreeSet<&'static str> = BTreeSet::new();
    for (sel, path) in candidates {
        if dewasm_backend_go::selector_used(text, sel) {
            set.insert(path);
        }
    }
    set.into_iter().map(|s| s.to_string()).collect()
}

fn import_block(imports: &[String]) -> String {
    if imports.is_empty() {
        return String::new();
    }
    let mut out = String::from("import (\n");
    for path in imports {
        let _ = writeln!(out, "\t{path:?}");
    }
    out.push_str(")\n");
    out
}

/// `_spectest`, plus any currently-`register`ed instances merged in under their registered name: each instance's `Exports` map doubles as an import provider.
fn imports_expr(registered: &[(String, String)]) -> String {
    let mut entries = vec!["\"spectest\": _spectest".to_string()];
    for (name, var) in registered {
        entries.push(format!("{}: {var}.Exports", go_string(name)));
    }
    format!("Imports{{{}}}", entries.join(", "))
}

fn arg_go(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok(format!("uint32({})", *v as u32)),
        WastArg::Core(WastArgCore::I64(v)) => Ok(format!("uint64({})", *v as u64)),
        WastArg::Core(WastArgCore::F32(f)) => Ok(format!("Rt.f32_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::F64(f)) => Ok(format!("Rt.f64_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(WastArgCore::RefNull(hty)) => Err(dewasm_test_helper::heap_type_tag(hty)),
        WastArg::Core(WastArgCore::RefExtern(_)) => Err("reference-types".to_string()),
        WastArg::Core(WastArgCore::RefHost(_)) => Err("reference-types".to_string()),
        _ => Err("component-model".to_string()),
    }
}

fn ret_cmp(value: &str, ret: &WastRet<'_>) -> Result<String, String> {
    match ret {
        WastRet::Core(WastRetCore::I32(v)) => Ok(format!("{value}.(uint32) == {}", *v as u32)),
        WastRet::Core(WastRetCore::I64(v)) => Ok(format!("{value}.(uint64) == {}", *v as u64)),
        WastRet::Core(WastRetCore::F32(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => {
                format!("(math.Float32bits({value}.(float32)) & 0x7fffffff) == 0x7fc00000")
            }
            NanPattern::ArithmeticNan => {
                format!("(math.Float32bits({value}.(float32)) & 0x7fc00000) == 0x7fc00000")
            }
            NanPattern::Value(f) => {
                format!("math.Float32bits({value}.(float32)) == 0x{:x}", f.bits)
            }
        }),
        WastRet::Core(WastRetCore::F64(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => {
                format!(
                    "(math.Float64bits({value}.(float64)) & 0x7fffffffffffffff) == 0x7ff8000000000000"
                )
            }
            NanPattern::ArithmeticNan => {
                format!(
                    "(math.Float64bits({value}.(float64)) & 0x7ff8000000000000) == 0x7ff8000000000000"
                )
            }
            NanPattern::Value(f) => {
                format!("math.Float64bits({value}.(float64)) == 0x{:x}", f.bits)
            }
        }),
        WastRet::Core(WastRetCore::V128(_)) => Err("simd".to_string()),
        WastRet::Core(WastRetCore::Either(_)) => Err("either-results".to_string()),
        WastRet::Core(WastRetCore::RefNull(_)) => Err("reference-types".to_string()),
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

/// Harness helpers + the `spectest` host fixture. `rtStack` is the recursion guard's shared counter (referenced only by spec-build generated functions).
const PREAMBLE: &str = r#"var rtStack int

var _pass, _fail int

func check(desc string, thunk func() (bool, any)) {
	var r any
	var ok bool
	var actual any
	func() {
		defer func() { r = recover() }()
		ok, actual = thunk()
	}()
	if r != nil {
		_fail++
		fmt.Printf("FAIL(panic %v): %s\n", r, desc)
		return
	}
	if ok {
		_pass++
	} else {
		_fail++
		fmt.Printf("FAIL: %s (got %v)\n", desc, actual)
	}
}

func check_trap(desc, msg string, thunk func()) {
	var r any
	func() {
		defer func() { r = recover() }()
		thunk()
	}()
	if r == nil {
		_fail++
		fmt.Printf("FAIL(no trap, want %q): %s\n", msg, desc)
		return
	}
	if t, ok := r.(*rtTrap); ok {
		if strings.Contains(t.msg, msg) || strings.Contains(msg, t.msg) {
			_pass++
		} else {
			_fail++
			fmt.Printf("FAIL(trap %q, want %q): %s\n", t.msg, msg, desc)
		}
		return
	}
	_fail++
	fmt.Printf("FAIL(panic %v, want trap %q): %s\n", r, msg, desc)
}

func check_exhaust(desc string, thunk func()) {
	var r any
	func() {
		defer func() { r = recover() }()
		thunk()
	}()
	if t, ok := r.(*rtTrap); ok && t.msg == "call stack exhausted" {
		_pass++
		return
	}
	if r == nil {
		_fail++
		fmt.Printf("FAIL(no exhaustion): %s\n", desc)
		return
	}
	_fail++
	fmt.Printf("FAIL(want exhaustion, got %v): %s\n", r, desc)
}

// Upstream's assert_unlinkable message text never matches ours; a raised rtLinkError confirms the import was correctly rejected as unlinkable. Any other panic means the module linked and then crashed, which must not pass.
func check_unlinkable(desc string, thunk func()) {
	var r any
	func() {
		defer func() { r = recover() }()
		thunk()
	}()
	if r == nil {
		_fail++
		fmt.Printf("FAIL(no error, want unlinkable): %s\n", desc)
		return
	}
	if _, ok := r.(*rtLinkError); ok {
		_pass++
		return
	}
	_fail++
	fmt.Printf("FAIL(non-link error, want unlinkable): %s (%v)\n", desc, r)
}

var _spectest = map[string]any{
	"print":         func() {},
	"print_i32":     func(uint32) {},
	"print_i64":     func(uint64) {},
	"print_f32":     func(float32) {},
	"print_f64":     func(float64) {},
	"print_i32_f32": func(uint32, float32) {},
	"print_f64_f64": func(float64, float64) {},
	"global_i32":    newGlobal[uint32](666),
	"global_i64":    newGlobal[uint64](666),
	"global_f32":    newGlobal[float32](666.6),
	"global_f64":    newGlobal[float64](666.6),
	"table":         newTable(10),
	"memory":        newMemory(1, 2),
}
"#;

dewasm_test_helper::spec_suite!(GoSpec);
