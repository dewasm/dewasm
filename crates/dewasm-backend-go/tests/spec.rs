//! Go side of the shared spec harness (ADR-3, ADR-27, ADR-29): converts each module with the Go backend to package-level declarations, phrases every assertion as compiled Go (`check`/`check_trap`/`check_exhaust`/ `check_unlinkable`, bit-exact float comparison via `math.Float32bits`/ `math.Float64bits`), assembles one self-contained program per `.wast` file, and `go build`s + runs it. The generic harness lives in `dewasm-test-helper`.
//!
//! Three Go facts shape the phrasing (ADR-29):
//! - Go is statically typed and has no dynamic `invoke`, so each generated type carries a reflective `invoke(name, args...) []any` / `globalGet(name) any` dispatcher (built where the module — hence every export's signature — is known); the harness asserts the boxed `any` results to the expected type.
//! - Type/method declarations cannot live inside `func main`, so per-module `Converted.source` is accumulated at package scope in the harness's file-scoped `decls` buffer (hoisted ahead of the body by `assemble`) while only instantiation/assertion statements go in the body.
//! - A runaway recursion overflows Go's goroutine stack *fatally* (uncatchable, killing the process), so the spec build instruments every generated function with a recursion guard (ADR-29) that turns exhaustion into a catchable "call stack exhausted" trap the harness observes.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_go::{find_go, GoBackend};
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{AbstractHeapType, HeapType, NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

/// Known assertion-level failures with their attribution; the file still runs so regressions in the passing assertions are caught.
///
/// - `import-limits`: the Go type assertion that resolves an import checks its *kind* (func/global/table/memory) and, for functions and globals, the full value/signature type too — but not a global's mutability, nor a table/memory's min/max limits, against the import site's declared bounds. Every `assert_unlinkable` case testing one of those stays a known gap. The counts are *lower* than Ruby/Python's (ADR-16): the Go type assertion catches func-signature and global-value-type mismatches those backends' kind-only check misses, so only the mutability/limit cases remain (the two `linking` failures are both global-mutability mismatches).
/// - `linking` (`linking0`/`load1`): downstream of an *unrelated* declared-unsupported feature (multi-memory) inside a module that also uses `register`; that module never converts, so a later assertion against the module it would have written into observes stale state. Not a cross-module-linking gap itself.
///
/// `skip-stack-guard-page` is *not* here: its `function-with-many-locals` (1056 locals) is the one function in the suite whose frame cost trips the ADR-29 recursion guard even at shallow depth, so all 10 of its exhaustion cases pass.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 28, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 2, "import-limits"),
    ("linking0", 1, "linking"),
    ("load1", 5, "linking"),
];

/// Files `cargo test` runs by default (the non-ignored trials). Go compiles each `.wast` file to one program, so the default gate runs a curated list covering every semantic area (integers, floats, control flow, memory/table, globals, linking, bulk ops) plus the whole ledger; `cargo test -- --include-ignored` sweeps every file (one `go build` per file — a few seconds each, dominated by compile latency).
const CURATED_FILES: &[&str] = &[
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
    "skip-stack-guard-page",
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

pub struct GoSpec;

impl BackendUnderTest for GoSpec {
    fn name(&self) -> &'static str {
        "go"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &GoBackend
    }

    /// Compile `source` to a content-addressed cache binary (identical programs build once) and run it. A missing `go` toolchain fails loud (ADR-15); a build failure is surfaced as the build command's `Output` so the harness reports the compile error.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        let go = find_go()
            .expect("go toolchain not found on PATH (or $DEWASM_GO) — see docs/testing.md");

        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let hash = hasher.finish();

        let cache = std::env::temp_dir().join("dewasm-go-cache");
        std::fs::create_dir_all(&cache).unwrap();
        let bin = cache.join(format!("spec-{hash:016x}"));

        if !bin.exists() {
            let src = cache.join(format!("spec-{hash:016x}.go"));
            std::fs::write(&src, source).unwrap();
            let tmp_bin = cache.join(format!(
                "spec-{hash:016x}.{}.{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let build = Command::new(&go)
                .arg("build")
                .arg("-o")
                .arg(&tmp_bin)
                .arg(&src)
                .output()
                .expect("spawn go build");
            if !build.status.success() {
                return build;
            }
            let _ = std::fs::rename(&tmp_bin, &bin);
        }

        dewasm_test_helper::run_command_bytes(Command::new(&bin).args(args), stdin)
    }
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl dewasm_test_helper::SpecBackend for GoSpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    fn curated_files(&self) -> Option<&'static [&'static str]> {
        Some(CURATED_FILES)
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
        let mut parts = vec![go_str(name)];
        for arg in args {
            parts.push(arg_go(arg)?);
        }
        Ok(format!("{var}.invoke({})", parts.join(", ")))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}.globalGet({})", go_str(global))
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
            go_str(desc)
        );
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let _ = writeln!(
            script,
            "check_trap({}, {}, func() {{ {call} }})",
            go_str(desc),
            go_str(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_exhaust({}, func() {{ {call} }})",
            go_str(desc)
        );
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check({}, func() (bool, any) {{ {call}; return true, nil }})",
            go_str(desc)
        );
    }

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_unlinkable({}, func() {{ {call} }})",
            go_str(desc)
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

/// The external packages the assembled program references. Every fragment scanned is controlled (runtime bundle, harness preamble, generated declarations, and a body whose only free-form strings are `file.wast:line` descriptions and wasm trap messages) so no user data can inject a false import (ADR-29).
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
        if text.contains(sel) {
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

/// `_spectest`, plus any currently-`register`ed instances merged in under their registered name — each instance's `Exports` map doubles as an ADR-7 import provider (ADR-16).
fn imports_expr(registered: &[(String, String)]) -> String {
    let mut entries = vec!["\"spectest\": _spectest".to_string()];
    for (name, var) in registered {
        entries.push(format!("{}: {var}.Exports", go_str(name)));
    }
    format!("Imports{{{}}}", entries.join(", "))
}

/// Go double-quoted string literal.
fn go_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Attribution for a ref heap type the harness cannot express as a Go value. Ref-typed args/results only occur in reference-types modules, which fail to convert (Feature::ReferenceTypes unsupported) and so are skipped upstream; this is defensive attribution.
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

fn arg_go(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok(format!("uint32({})", *v as u32)),
        WastArg::Core(WastArgCore::I64(v)) => Ok(format!("uint64({})", *v as u64)),
        WastArg::Core(WastArgCore::F32(f)) => Ok(format!("Rt.f32_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::F64(f)) => Ok(format!("Rt.f64_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(WastArgCore::RefNull(hty)) => Err(heap_type_tag(hty)),
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
