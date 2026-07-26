//! Java side of the shared spec harness (ADR-3, ADR-27, ADR-30): converts each
//! module with the Java backend to a package-private module class (carrying a
//! reflective `invoke`/`globalGet` dispatcher), phrases every assertion as
//! compiled Java (`check`/`check_trap`/`check_exhaust`/`check_unlinkable`,
//! bit-exact float comparison via `Float.floatToRawIntBits` /
//! `Double.doubleToRawLongBits`), assembles one self-contained `Main.java` per
//! `.wast` file, and `javac`s + runs it. The generic harness lives in
//! `dewasm-test-helper`.
//!
//! Two Java facts shape the phrasing (ADR-30):
//! - A JVM `StackOverflowError` is *catchable* (unlike Go's fatal goroutine
//!   overflow), so exhaustion maps directly: `check_exhaust` catches it, exactly
//!   as Ruby catches `SystemStackError`. No spec-build recursion guard is
//!   instrumented into the generated functions — each assertion is independent,
//!   so unwinding a mid-call overflow cannot corrupt a later, independent check.
//! - Type/class declarations cannot live inside a method, so per-module classes
//!   are accumulated at file scope (a `Mutex<String>`, drained in `assemble`)
//!   while only instantiation/assertion statements go in `main`.

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::hash::{Hash, Hasher};
use std::process::{Command, Output};
use std::sync::Mutex;

use dewasm_backend::Backend;
use dewasm_backend_java::{find_java, find_javac, JavaBackend};
use dewasm_core::ir;
use dewasm_test_helper::{run_command_bytes, spec_suite, BackendUnderTest, Converted, SpecBackend};
use wast::core::{AbstractHeapType, HeapType, NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

/// Known assertion-level failures with their attribution; the file still runs
/// so regressions in the passing assertions are caught.
///
/// - `import-limits`: an imported func resolves through the single `Rt.Fn`
///   boundary (no signature is checked at all), and an imported global/table/
///   memory is checked only for its *kind* via `instanceof` — not a func's
///   param/result types, a global's value type or mutability, nor a
///   table/memory's min/max limits. Every `assert_unlinkable` case testing one
///   of those (not a kind mismatch, which is caught) stays a known gap. Java's
///   counts match Ruby's (ADR-16), not Go's lower ones: Java's uniform `Rt.Fn`
///   and `Object`-valued `Global` box carry no static wasm type, so the
///   func-signature and global-value-type mismatches Go's typed assertion
///   rejects are not caught here (ADR-30).
/// - `linking` (`linking0`/`load1`): downstream of an *unrelated*
///   declared-unsupported feature (multi-memory) inside a module that also uses
///   `register`; that module never converts, so a later assertion against the
///   module it would have written into observes stale state. Not a
///   cross-module-linking gap itself.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 28, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 4, "import-limits"),
    ("linking0", 1, "linking"),
    ("load1", 5, "linking"),
];

/// Files `cargo test` runs by default. Java compiles each `.wast` file to one
/// `Main.java` (one `javac` per file), so the default gate runs a curated list
/// covering every semantic area (integers, floats, control flow, memory/table,
/// globals, linking, bulk ops) plus the whole ledger; `DEWASM_SPEC_ALL=1`
/// sweeps every file.
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

pub struct JavaSpec {
    /// File-scope module class declarations for the file currently being
    /// assembled. Java forbids type declarations inside a method, so they are
    /// accumulated here and emitted before `Main` by `assemble`. The harness
    /// runs files sequentially in one test, so the `Mutex` (needed for `Sync`)
    /// never contends; it is drained at the end of each file.
    decls: Mutex<String>,
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl BackendUnderTest for JavaSpec {
    fn name(&self) -> &'static str {
        "java"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &JavaBackend
    }

    /// Compile `source` (one `Main.java`) to a content-addressed class-dir cache
    /// (identical programs compile once) and run it. A missing `javac`/`java`
    /// fails loud (ADR-15); a `javac` failure is surfaced as its `Output` so the
    /// harness reports the compile error.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        let javac =
            find_javac().expect("javac not found on PATH (or $DEWASM_JAVAC) — see docs/testing.md");
        let java =
            find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");

        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let hash = hasher.finish();

        let cache = std::env::temp_dir().join("dewasm-java-spec-cache");
        std::fs::create_dir_all(&cache).unwrap();
        let classdir = cache.join(format!("spec-{hash:016x}"));

        if !classdir.join("Main.class").exists() {
            let tmp = cache.join(format!(
                "spec-{hash:016x}.{}.{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&tmp).unwrap();
            let src = tmp.join("Main.java");
            std::fs::write(&src, source).unwrap();
            let build = Command::new(&javac)
                .arg("-d")
                .arg(&tmp)
                .arg(&src)
                .output()
                .expect("spawn javac");
            if !build.status.success() {
                return build;
            }
            let _ = std::fs::rename(&tmp, &classdir);
        }

        run_command_bytes(
            // A generous per-thread stack keeps genuinely deep but terminating
            // recursions (fac, deep br chains) under the limit while a runaway
            // still overflows into a catchable StackOverflowError (ADR-30).
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

impl SpecBackend for JavaSpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    fn default_files(&self) -> Option<&'static [&'static str]> {
        Some(CURATED_FILES)
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[
            // check_trap / check_unlinkable match these exception types.
            "rt/trap",
            "rt/link_error",
            // The _spectest fixture (PREAMBLE) constructs these directly.
            "global/_class",
            "table/_class",
            "memory/_class",
        ]
    }

    fn generate(&self, module: &ir::Module, counter: u32) -> anyhow::Result<Converted> {
        let type_name = format!("WastMod{counter}");
        let (source, units) = dewasm_backend_java::generate_program_with_units(module, &type_name)?;
        Ok(Converted {
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
        conv: &Converted,
        var_id: u32,
        registered: &[(String, String)],
    ) -> String {
        self.decls.lock().unwrap().push_str(&conv.source);
        let var = format!("_i{var_id}");
        let _ = writeln!(
            script,
            "{ty} {var} = new {ty}({}, null, null);",
            imports_expr(registered),
            ty = conv.handle
        );
        var
    }

    fn instantiate_call(
        &self,
        script: &mut String,
        conv: &Converted,
        registered: &[(String, String)],
    ) -> String {
        let _ = script;
        self.decls.lock().unwrap().push_str(&conv.source);
        format!(
            "new {}({}, null, null)",
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
            java_str(name),
            parts.join(", ")
        ))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}.globalGet({})", java_str(global))
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
            java_str(desc)
        );
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let _ = writeln!(
            script,
            "check_trap({}, {}, () -> {{ {call}; }});",
            java_str(desc),
            java_str(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_exhaust({}, () -> {{ {call}; }});",
            java_str(desc)
        );
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check({}, () -> {{ {call}; return new Object[]{{ true, null }}; }});",
            java_str(desc)
        );
    }

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_unlinkable({}, () -> {{ {call}; }});",
            java_str(desc)
        );
    }

    fn assemble(&self, units: &BTreeSet<String>, body: &str) -> anyhow::Result<String> {
        let bundle = dewasm_backend_java::bundler()
            .bundle(units, 0)
            .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?;
        let decls = std::mem::take(&mut *self.decls.lock().unwrap());

        let mut out = String::from("// Generated by the dewasm spec harness. Do not edit.\n");
        out.push_str(&bundle);
        out.push_str("\n\n");
        out.push_str(&decls);
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

/// The imports object: `spectest`, plus any currently-`register`ed instances
/// merged in under their registered name — each instance's `Exports` map
/// doubles as an ADR-7 import provider (ADR-16).
fn imports_expr(registered: &[(String, String)]) -> String {
    let mut entries = vec!["\"spectest\", _spectest".to_string()];
    for (name, var) in registered {
        entries.push(format!("{}, {var}.Exports", java_str(name)));
    }
    format!("imps({})", entries.join(", "))
}

/// Java double-quoted string literal.
fn java_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Attribution for a ref heap type the harness cannot express as a Java value.
/// Ref-typed args/results only occur in reference-types modules, which fail to
/// convert and are skipped upstream; this is defensive attribution.
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
        WastArg::Core(WastArgCore::RefNull(hty)) => Err(heap_type_tag(hty)),
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

/// Harness helpers + the `spectest` host fixture + the `Main` entry point. The
/// generated body is spliced in after this preamble by `assemble`.
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

    // A JVM StackOverflowError is catchable, so exhaustion maps to it directly
    // (like Ruby's SystemStackError). Each assertion is independent, so
    // unwinding a mid-call overflow cannot corrupt a later check (ADR-30).
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

    // Upstream's assert_unlinkable message text never matches ours; a raised
    // Rt.LinkError confirms the import was correctly rejected as unlinkable. Any
    // other throwable means the module linked and then crashed, which must fail.
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

spec_suite!(JavaSpec {
    decls: Mutex::new(String::new()),
});
