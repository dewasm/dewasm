//! Java side of the shared spec harness: converts each module with the Java backend to a package-private module class (carrying a reflective `invoke`/`globalGet` dispatcher), phrases every assertion as compiled Java (`check`/`check_trap`/`check_exhaust`/`check_unlinkable`, bit-exact float comparison via `Float.floatToRawIntBits` / `Double.doubleToRawLongBits`), assembles one self-contained `Main.java` per `.wast` file, and `javac`s + runs it.
//! The generic harness lives in `dewasm-test-helper`.
//!
//! Two Java facts shape the phrasing:
//! - A JVM `StackOverflowError` is *catchable* (unlike Go's fatal goroutine overflow), so exhaustion maps directly: `check_exhaust` catches it, exactly as Ruby catches `SystemStackError`.
//!   No spec-build recursion guard is instrumented into the generated functions: each assertion is independent, so unwinding a mid-call overflow cannot corrupt a later, independent check.
//! - Type/class declarations cannot live inside a method, so per-module classes are accumulated in the harness's file-scoped `decls` buffer (hoisted ahead of `main`'s body by `assemble`) while only instantiation/assertion statements go in `main`.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, java_string, JavaBackend};
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

/// Known assertion-level failures with their attribution; the file still runs so regressions in the passing assertions are caught.
///
/// - `import-limits`: an imported func resolves through the single `Rt.Fn` boundary (no signature is checked at all), and an imported global/table/ memory/tag is checked only for its *kind* via `instanceof`, not a func's param/result types, a global's value type or mutability, a tag's parameter types, nor a table/memory's min/max limits.
///   Every `assert_unlinkable` case testing one of those (not a kind mismatch, which is caught) stays a known gap.
///   Java's counts match Ruby's, not Go's lower ones: Java's uniform `Rt.Fn` and `Object`-valued `Global` box carry no static wasm type, so the func-signature and global-value-type mismatches Go's typed assertion rejects are not caught here.
///   imports.wast contributes 59 of them: its fixture module exports tags, so it converts only now that tags are represented, and every type-mismatch check downstream of it became reachable at once (28 before).
/// - `linking` (`linking0`/`load1`): downstream of an *unrelated* declared-unsupported feature (multi-memory) inside a module that also uses `register`; that module never converts, so a later assertion against the module it would have written into observes stale state.
///   Not a cross-module-linking gap itself.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 59, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 4, "import-limits"),
    ("linking0", 1, "linking"),
    ("load1", 5, "linking"),
];

mod common;

pub struct JavaSpec;

impl BackendUnderTest for JavaSpec {
    fn name(&self) -> &'static str {
        "java"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &JavaBackend
    }

    /// Compile `source` (one `Main.java`) to a content-addressed class-dir cache (identical programs compile once) and run it.
    /// A missing `javac`/`java` fails loud; a `javac` failure is surfaced as its `Output` so the harness reports the compile error.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA): see docs/testing.md");
        let classdir = match common::build_java(source) {
            Err(build) => return build,
            Ok(classdir) => classdir,
        };

        dewasm_test_helper::run_command_bytes(
            // A generous per-thread stack keeps genuinely deep but terminating recursions (fac, deep br chains) under the limit while a runaway still overflows into a catchable StackOverflowError.
            Command::new(&java)
                .arg("-Xss16m")
                .arg("-cp")
                .arg(&classdir)
                .arg("Main")
                .args(args),
            stdin,
        )
    }
}

impl dewasm_test_helper::SpecBackend for JavaSpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    /// Java compiles each `.wast` file to one `Main.java` (one `javac` per file), so a plain `cargo test` runs only the shared curated list, plus `skip-stack-guard-page` for its deep-frame exhaustion cases and the four exception-handling files.
    fn curated_files(&self) -> Option<&'static [&'static str]> {
        Some(dewasm_test_helper::curated_with(&[
            &["skip-stack-guard-page"],
            dewasm_test_helper::EXCEPTION_HANDLING_SPEC_FILES,
            dewasm_test_helper::TAIL_CALL_SPEC_FILES,
        ]))
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[
            // check_trap / check_unlinkable / check_exception match these exception types.
            "rt/trap",
            "rt/link_error",
            "rt/wasm_exception",
            // The _spectest fixture (PREAMBLE) constructs these directly.
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
        let (source, units) = dewasm_backend_java::generate_program_with_units(module, &type_name)?;
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
            "{ty} {var} = new {ty}({}, null, null, null);",
            imports_expr(registered),
            ty = conv.handle
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
        format!(
            "new {}({}, null, null, null)",
            conv.handle,
            imports_expr(registered)
        )
    }

    fn invoke(&self, var: &str, name: &str, args: &[WastArg<'_>]) -> Result<String, String> {
        let mut parts = Vec::new();
        for arg in args {
            parts.push(arg_java(arg)?);
        }
        Ok(format!(
            "{var}.invoke({}, new Object[]{{{}}})",
            java_string(name),
            parts.join(", ")
        ))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}.globalGet({})", java_string(global))
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
            "check({}, () -> {{ Object[] __r = {call}; return new Object[]{{ ({cmp}), __r }}; }});",
            java_string(desc)
        );
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let _ = writeln!(
            script,
            "check_trap({}, {}, () -> {{ {call}; }});",
            java_string(desc),
            java_string(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_exhaust({}, () -> {{ {call}; }});",
            java_string(desc)
        );
    }

    fn emit_check_exception(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
    ) -> Result<(), String> {
        let _ = writeln!(
            script,
            "check_exception({}, () -> {{ {call}; }});",
            java_string(desc)
        );
        Ok(())
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check({}, () -> {{ {call}; return new Object[]{{ true, null }}; }});",
            java_string(desc)
        );
    }

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_unlinkable({}, () -> {{ {call}; }});",
            java_string(desc)
        );
    }

    fn assemble(
        &self,
        units: &BTreeSet<String>,
        decls: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let bundle = dewasm_backend_java::bundler()
            .bundle(units, 0)
            .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?;

        let mut out = String::from("// Generated by the dewasm spec harness. Do not edit.\n");
        out.push_str(&bundle);
        out.push_str("\n\n");
        out.push_str(decls);
        out.push('\n');
        out.push_str(PREAMBLE);
        for line in body.lines() {
            if line.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(out, "        {line}");
            }
        }
        out.push_str(POSTAMBLE);
        Ok(out)
    }
}

/// The imports object: `spectest`, plus any currently-`register`ed instances merged in under their registered name: each instance's `Exports` map doubles as an import provider.
fn imports_expr(registered: &[(String, String)]) -> String {
    let mut entries = vec!["\"spectest\", _spectest".to_string()];
    for (name, var) in registered {
        entries.push(format!("{}, {var}.Exports", java_string(name)));
    }
    format!("imps({})", entries.join(", "))
}

/// A wast arg as a boxed Java value (autoboxed inside `new Object[]{...}`).
fn arg_java(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok(format!("0x{:x}", *v as u32)),
        WastArg::Core(WastArgCore::I64(v)) => Ok(format!("0x{:x}L", *v as u64)),
        WastArg::Core(WastArgCore::F32(f)) => Ok(format!("Float.intBitsToFloat(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::F64(f)) => {
            Ok(format!("Double.longBitsToDouble(0x{:x}L)", f.bits))
        }
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(WastArgCore::RefNull(hty)) => {
            if dewasm_test_helper::nullable_heap_type(hty) {
                Ok("null".to_string())
            } else {
                Err(dewasm_test_helper::heap_type_tag(hty))
            }
        }
        WastArg::Core(WastArgCore::RefExtern(_)) => Err("reference-types".to_string()),
        WastArg::Core(WastArgCore::RefHost(_)) => Err("reference-types".to_string()),
        _ => Err("component-model".to_string()),
    }
}

/// A boolean comparison of a boxed result slot against an expected wast return.
fn ret_cmp(value: &str, ret: &WastRet<'_>) -> Result<String, String> {
    match ret {
        WastRet::Core(WastRetCore::I32(v)) => {
            Ok(format!("(int)(Integer) {value} == 0x{:x}", *v as u32))
        }
        WastRet::Core(WastRetCore::I64(v)) => {
            Ok(format!("(long)(Long) {value} == 0x{:x}L", *v as u64))
        }
        WastRet::Core(WastRetCore::F32(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => {
                format!("(Float.floatToRawIntBits((float)(Float) {value}) & 0x7fffffff) == 0x7fc00000")
            }
            NanPattern::ArithmeticNan => {
                format!("(Float.floatToRawIntBits((float)(Float) {value}) & 0x7fc00000) == 0x7fc00000")
            }
            NanPattern::Value(f) => {
                format!("Float.floatToRawIntBits((float)(Float) {value}) == 0x{:x}", f.bits)
            }
        }),
        WastRet::Core(WastRetCore::F64(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => format!(
                "(Double.doubleToRawLongBits((double)(Double) {value}) & 0x7fffffffffffffffL) == 0x7ff8000000000000L"
            ),
            NanPattern::ArithmeticNan => format!(
                "(Double.doubleToRawLongBits((double)(Double) {value}) & 0x7ff8000000000000L) == 0x7ff8000000000000L"
            ),
            NanPattern::Value(f) => format!(
                "Double.doubleToRawLongBits((double)(Double) {value}) == 0x{:x}L",
                f.bits
            ),
        }),
        WastRet::Core(WastRetCore::V128(_)) => Err("simd".to_string()),
        WastRet::Core(WastRetCore::Either(_)) => Err("either-results".to_string()),
        WastRet::Core(WastRetCore::RefNull(hty)) => match hty {
            None => Ok(format!("{value} == null")),
            Some(hty) if dewasm_test_helper::nullable_heap_type(hty) => {
                Ok(format!("{value} == null"))
            }
            Some(hty) => Err(dewasm_test_helper::heap_type_tag(hty)),
        },
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

/// Harness helpers + the `spectest` host fixture + the `Main` entry point.
/// The generated body is spliced in after this preamble by `assemble`.
const PREAMBLE: &str = r#"public class Main {
    static int _pass = 0;
    static int _fail = 0;

    static java.util.Map<String, java.util.Map<String, Object>> imps(Object... kv) {
        java.util.Map<String, java.util.Map<String, Object>> m = new java.util.HashMap<>();
        for (int i = 0; i < kv.length; i += 2) {
            @SuppressWarnings("unchecked")
            java.util.Map<String, Object> v = (java.util.Map<String, Object>) kv[i + 1];
            m.put((String) kv[i], v);
        }
        return m;
    }

    static void check(String desc, java.util.function.Supplier<Object[]> thunk) {
        try {
            Object[] r = thunk.get();
            if ((Boolean) r[0]) {
                _pass++;
            } else {
                _fail++;
                System.out.println("FAIL: " + desc + " (got " + java.util.Arrays.deepToString((Object[]) r[1]) + ")");
            }
        } catch (Throwable e) {
            _fail++;
            System.out.println("FAIL(" + e.getClass().getSimpleName() + ": " + e.getMessage() + "): " + desc);
        }
    }

    static void check_trap(String desc, String msg, Runnable thunk) {
        try {
            thunk.run();
            _fail++;
            System.out.println("FAIL(no trap, want " + msg + "): " + desc);
        } catch (Rt.Trap e) {
            String m = e.getMessage();
            if (m != null && (m.contains(msg) || msg.contains(m))) {
                _pass++;
            } else {
                _fail++;
                System.out.println("FAIL(trap " + m + ", want " + msg + "): " + desc);
            }
        } catch (Throwable e) {
            _fail++;
            System.out.println("FAIL(" + e.getClass().getSimpleName() + ", want trap " + msg + "): " + desc);
        }
    }

    // A JVM StackOverflowError is catchable, so exhaustion maps to it directly (like Ruby's SystemStackError).
    // Each assertion is independent, so unwinding a mid-call overflow cannot corrupt a later check.
    static void check_exhaust(String desc, Runnable thunk) {
        try {
            thunk.run();
            _fail++;
            System.out.println("FAIL(no exhaustion): " + desc);
        } catch (StackOverflowError e) {
            _pass++;
        } catch (Throwable e) {
            _fail++;
            System.out.println("FAIL(want exhaustion, got " + e + "): " + desc);
        }
    }

    // A wasm exception must reach the host uncaught. Rt.Trap is an unrelated class on purpose, so a trap here is a failure, not a pass.
    static void check_exception(String desc, Runnable thunk) {
        try {
            thunk.run();
            _fail++;
            System.out.println("FAIL(no exception): " + desc);
        } catch (Rt.WasmException e) {
            _pass++;
        } catch (Throwable e) {
            _fail++;
            System.out.println("FAIL(want a wasm exception, got " + e + "): " + desc);
        }
    }

    // Upstream's assert_unlinkable message text never matches ours; a raised Rt.LinkError confirms the import was correctly rejected as unlinkable.
    // Any other throwable means the module linked and then crashed, which must fail.
    static void check_unlinkable(String desc, Runnable thunk) {
        try {
            thunk.run();
            _fail++;
            System.out.println("FAIL(no error, want unlinkable): " + desc);
        } catch (Rt.LinkError e) {
            _pass++;
        } catch (Throwable e) {
            _fail++;
            System.out.println("FAIL(non-link error, want unlinkable): " + desc + " (" + e + ")");
        }
    }

    static java.util.Map<String, Object> _spectest = mkSpectest();

    static java.util.Map<String, Object> mkSpectest() {
        java.util.Map<String, Object> m = new java.util.HashMap<>();
        m.put("print", (Rt.Fn)(a -> null));
        m.put("print_i32", (Rt.Fn)(a -> null));
        m.put("print_i64", (Rt.Fn)(a -> null));
        m.put("print_f32", (Rt.Fn)(a -> null));
        m.put("print_f64", (Rt.Fn)(a -> null));
        m.put("print_i32_f32", (Rt.Fn)(a -> null));
        m.put("print_f64_f64", (Rt.Fn)(a -> null));
        m.put("global_i32", new Global(666));
        m.put("global_i64", new Global(666L));
        m.put("global_f32", new Global(666.6f));
        m.put("global_f64", new Global(666.6));
        m.put("table", new Table(10));
        m.put("memory", new Memory(1, 2));
        return m;
    }

    public static void main(String[] argv) {
"#;

const POSTAMBLE: &str = r#"        System.out.println("RESULT pass=" + _pass + " fail=" + _fail);
    }
}
"#;

dewasm_test_helper::spec_suite!(JavaSpec);
