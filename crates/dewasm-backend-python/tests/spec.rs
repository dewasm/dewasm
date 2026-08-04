//! Python side of the shared spec harness (ADR-3, ADR-27, ADR-28): converts modules with the Python backend, phrases assertions as Python (`check`/ `check_trap`/`check_exhaust`/`check_unlinkable` helpers, bit-exact float comparison via `Rt.f32_bits`/`Rt.f64_bits`), and runs the script with the `python3` on PATH. The generic harness lives in `dewasm-test-helper`.
//!
//! Two Python facts shape the phrasing (ADR-28):
//! - Assertions are passed as zero-arg lambdas because Python has no statement blocks; the value under test is bound inside the lambda with an inner `(lambda __r: (<cmp>, __r))(<call>)`.
//! - Deep guest recursion (call/fac) and the `assert_exhaustion` cases both need more stack than the default; the whole assertion body runs in a thread with a large `threading.stack_size` and a raised `sys.setrecursionlimit`, so a runaway recursion surfaces as a catchable `RecursionError` (mapped to `call stack exhausted`) instead of a C-stack crash — the guest-side analogue of the harness's `convert_on_big_stack`. `check_exhaust` lowers the limit around itself, because there the limit is not headroom but the entire cost of the check; see the comment on `_EXHAUST_RECURSION_LIMIT`.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use dewasm_backend::{Backend, RuntimeLinkage};
use dewasm_backend_python::PythonBackend;
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{AbstractHeapType, HeapType, NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

/// Known assertion-level failures with their attribution; the file still runs so regressions in the passing assertions are caught. Identical in shape to the Ruby ledger (ADR-16): the only open gap is `import-limits` — `Rt.check_import_kind` validates the *kind* of a resolved import but not its finer wasm type (a global's mutability, a table/memory's min/max limits, a function's signature), so the `assert_unlinkable` cases that test those, plus the two `linking`-tagged stale-state cases downstream of a declared-unsupported feature (multi-memory) that also happens to `register`, stay known gaps.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 28, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 4, "import-limits"),
    ("linking0", 1, "linking"),
    ("load1", 5, "linking"),
];

/// Files `cargo test` runs by default. Python executes wasm several times slower than Ruby — the full 257-file sweep takes ~9 s versus Ruby's ~4 s, spread thinly over per-file `python3` startup and the pure-Python numeric runtime with no single dominant file — so, like Bash (ADR-3 pre-accepts this), the gate runs a curated list covering every semantic area (integers, floats, control flow, memory/table, globals, linking, bulk ops) plus the whole ledger; `cargo test -- --include-ignored` sweeps everything.
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

pub struct PythonSpec;

impl BackendUnderTest for PythonSpec {
    fn name(&self) -> &'static str {
        "python"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &PythonBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasm_backend_python::find_python()
            .expect("python3 >= 3.9 not found on PATH — see docs/testing.md")
    }
}

impl dewasm_test_helper::SpecBackend for PythonSpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    fn curated_files(&self) -> Option<&'static [&'static str]> {
        Some(CURATED_FILES)
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[
            "rt/trap",
            // check_unlinkable references Rt.LinkError even when the converted modules themselves don't.
            "rt/link_error",
            "rt/f32_bits",
            "rt/f32_from_bits",
            "rt/f64_bits",
            "rt/f64_from_bits",
            // Referenced by the _spectest fixture (PREAMBLE below), not necessarily by the converted module itself.
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
        let (source, units) = dewasm_backend_python::generate_class_with_units(
            module,
            &class_name,
            &RuntimeLinkage::Alias("Rt".to_string()),
            false, // spec modules import spectest, never WASI
        )?;
        // The whole assertion body runs inside `def _main()`; a class defined there is a local class whose methods still resolve the module-level `Rt` global (ADR-28). The `Rt = Rt` alias line, however, would make `Rt` a `_main`-local name — drop it and rely on the module global.
        let source = source
            .strip_prefix("Rt = Rt\n\n\n")
            .unwrap_or(&source)
            .to_string();
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
        let var = format!("_i{var_id}");
        script.push_str(&conv.source);
        let _ = writeln!(
            script,
            "{var} = {}({})",
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
        format!("{}({})", conv.handle, imports_expr(registered))
    }

    fn invoke(&self, var: &str, name: &str, args: &[WastArg<'_>]) -> Result<String, String> {
        let mut parts = vec![py_str(name)];
        for arg in args {
            parts.push(arg_py(arg)?);
        }
        Ok(format!("{var}.invoke({})", parts.join(", ")))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}.global_get({})", py_str(global))
    }

    fn emit_check(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
        results: &[WastRet<'_>],
    ) -> Result<(), String> {
        let cmp = match results.len() {
            0 => "True".to_string(),
            1 => ret_cmp("__r", &results[0])?,
            _ => {
                let mut parts = Vec::new();
                for (i, r) in results.iter().enumerate() {
                    parts.push(ret_cmp(&format!("__r[{i}]"), r)?);
                }
                parts.join(" and ")
            }
        };
        let _ = writeln!(
            script,
            "check({}, lambda: (lambda __r: (({cmp}), __r))({call}))",
            py_str(desc)
        );
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let _ = writeln!(
            script,
            "check_trap({}, {}, lambda: {call})",
            py_str(desc),
            py_str(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(script, "check_exhaust({}, lambda: {call})", py_str(desc));
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check({}, lambda: (({call}), (True, None))[1])",
            py_str(desc)
        );
    }

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(script, "check_unlinkable({}, lambda: {call})", py_str(desc));
    }

    fn assemble(
        &self,
        units: &BTreeSet<String>,
        _decls: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        // The runtime units use `math`/`struct`; the harness uses `sys` and `threading` (PREAMBLE) — shared_runtime emits only `class Rt`.
        let mut script = String::from("import math\nimport struct\nimport sys\n\n");
        script.push_str(
            &dewasm_backend_python::shared_runtime(units)
                .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?,
        );
        script.push('\n');
        script.push_str(PREAMBLE);
        // The whole body (module class defs, instantiations, and assertions) runs inside `_main` so it can execute on a large-stack thread.
        script.push_str("\ndef _main():\n");
        for line in body.lines() {
            if line.is_empty() {
                script.push('\n');
            } else {
                let _ = writeln!(script, "    {line}");
            }
        }
        script.push_str("    print(\"RESULT pass=%d fail=%d\" % (_pass, _fail))\n");
        script.push_str(POSTAMBLE);
        Ok(script)
    }
}

/// `_spectest`, plus any currently-`register`ed instances merged in under their registered name — each instance doubles as an ADR-7 import provider (`wasm_import`).
fn imports_expr(registered: &[(String, String)]) -> String {
    if registered.is_empty() {
        return "_spectest".to_string();
    }
    let entries = registered
        .iter()
        .map(|(name, var)| format!("{}: {var}", py_str(name)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{**_spectest, {entries}}}")
}

/// Python double-quoted string literal.
fn py_str(s: &str) -> String {
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

/// Attribution for a null/ref heap type the harness cannot express as a Python value; the reference-types hierarchies (and their bottoms, also just `None`) are expressible.
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

fn arg_py(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok((*v as u32).to_string()),
        WastArg::Core(WastArgCore::I64(v)) => Ok((*v as u64).to_string()),
        WastArg::Core(WastArgCore::F32(f)) => Ok(format!("Rt.f32_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::F64(f)) => Ok(format!("Rt.f64_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(WastArgCore::RefNull(hty)) => {
            if nullable_heap_type(hty) {
                Ok("None".to_string())
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
            None => Ok(format!("{value} is None")),
            Some(hty) if nullable_heap_type(hty) => Ok(format!("{value} is None")),
            Some(hty) => Err(heap_type_tag(hty)),
        },
        WastRet::Core(WastRetCore::RefExtern(Some(n))) => Ok(format!("{value} == {n}")),
        // `(ref.extern)`: any non-null externref.
        WastRet::Core(WastRetCore::RefExtern(None)) => Ok(format!("{value} is not None")),
        WastRet::Core(WastRetCore::RefHost(n)) => Ok(format!("{value} == {n}")),
        // `(ref.func)`: any non-null funcref — in ADR-17's representation, a `[type_string, callable]` pair.
        WastRet::Core(WastRetCore::RefFunc(None)) => Ok(format!(
            "(isinstance({value}, list) and isinstance({value}[0], str))"
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

const PREAMBLE: &str = r#"import threading

_pass = 0
_fail = 0


def check(desc, thunk):
    global _pass, _fail
    try:
        ok, actual = thunk()
    except Exception as e:
        _fail += 1
        print("FAIL(%s: %s): %s" % (type(e).__name__, e, desc))
        return
    if ok:
        _pass += 1
    else:
        _fail += 1
        print("FAIL: %s (got %r)" % (desc, actual))


def check_trap(desc, msg, thunk):
    global _pass, _fail
    try:
        thunk()
    except Rt.Trap as e:
        m = str(e)
        if msg in m or m in msg:
            _pass += 1
        else:
            _fail += 1
            print("FAIL(trap %r, want %r): %s" % (m, msg, desc))
        return
    except Exception as e:
        _fail += 1
        print("FAIL(%s: %s, want trap %r): %s" % (type(e).__name__, e, msg, desc))
        return
    _fail += 1
    print("FAIL(no trap, want %r): %s" % (msg, desc))


# Exhaustion is detected by descending until the recursion limit trips, so the
# limit is a direct multiplier on how long every assert_exhaustion case takes.
# No such case in the testsuite has a reachable base case: call/call_indirect's
# `runaway`/`mutual-runaway` and skip-stack-guard-page's
# `function-with-many-locals` recurse unconditionally, and fac's
# `fac-rec 0x40000000` would need 2^30 frames — so a lower limit changes only how
# fast the descent ends, never whether it ends. The deepest bounded
# prefix any of them walks before entering the runaway part is 908 Python frames
# (`test-guard-page-skip 900`: 901 guest frames plus the harness), so 20000
# leaves ~20x headroom, and 20x CPython's own default limit of 1000. The global
# limit set in the postamble stays high for the checks that legitimately recurse.
_EXHAUST_RECURSION_LIMIT = 20000


def check_exhaust(desc, thunk):
    global _pass, _fail
    _prev = sys.getrecursionlimit()
    sys.setrecursionlimit(_EXHAUST_RECURSION_LIMIT)
    try:
        thunk()
    except RecursionError:
        _pass += 1
        return
    except Exception as e:
        _fail += 1
        print("FAIL(%s: %s, want exhaustion): %s" % (type(e).__name__, e, desc))
        return
    finally:
        sys.setrecursionlimit(_prev)
    _fail += 1
    print("FAIL(no exhaustion): %s" % desc)


# Upstream's assert_unlinkable message text never matches ours; a raised
# Rt.LinkError confirms the import was correctly rejected as unlinkable. Any
# other error means the module linked and then crashed, which must not pass.
def check_unlinkable(desc, thunk):
    global _pass, _fail
    try:
        thunk()
    except Rt.LinkError:
        _pass += 1
        return
    except Exception as e:
        _fail += 1
        print("FAIL(non-link error, want unlinkable): %s: %s: %s" % (desc, type(e).__name__, e))
        return
    _fail += 1
    print("FAIL(no error, want unlinkable): %s" % desc)


_spectest = {
    "spectest": {
        "print": lambda *a: None,
        "print_i32": lambda *a: None,
        "print_i64": lambda *a: None,
        "print_f32": lambda *a: None,
        "print_f64": lambda *a: None,
        "print_i32_f32": lambda *a: None,
        "print_f64_f64": lambda *a: None,
        "global_i32": Rt.Global(666),
        "global_i64": Rt.Global(666),
        "global_f32": Rt.Global(Rt.f32(666.6)),
        "global_f64": Rt.Global(666.6),
        "table": Rt.Table(10, 20),
        "memory": Rt.Memory(1, 2),
    },
}
"#;

const POSTAMBLE: &str = r#"

# The limit is a ceiling for the checks that legitimately recurse, not a budget:
# with check_exhaust capping itself at _EXHAUST_RECURSION_LIMIT, the deepest
# descent the whole 257-file sweep performs is that cap, and no other check comes
# near it (the sweep still passes with this global limit lowered to the cap). The
# thread stack is sized for that descent: CPython <= 3.10 keeps a C frame per
# Python frame and needs ~1 KiB each (20000 frames measured to fault below 24 MiB
# and survive above it), 3.11+ moves them off the C stack entirely, so 64 MiB is
# ~3x the worst-case need. Oversizing is not free — every parallel spec trial
# reserves this much.
sys.setrecursionlimit(1000000)
try:
    threading.stack_size(64 * 1024 * 1024)
except (ValueError, OverflowError, RuntimeError):
    pass
_t = threading.Thread(target=_main)
_t.start()
_t.join()
"#;

dewasm_test_helper::spec_suite!(PythonSpec);
