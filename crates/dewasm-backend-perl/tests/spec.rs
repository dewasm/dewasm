//! Perl side of the shared spec harness (ADR-3, ADR-27, ADR-55): converts modules with the Perl backend, phrases assertions as Perl (`check`/`check_trap`/`check_exhaust`/`check_unlinkable` subs, bit-exact float comparison via `Rt::f32_bits`/`Rt::f64_bits`), and runs the script with the `perl` on PATH. The generic harness lives in `dewasm-test-helper`.
//!
//! Two Perl facts shape the phrasing (ADR-55):
//! - Assertions are passed as zero-arg closures (`sub { ... }`); the value under test is captured in list context (`my @r = (<call>)`) so multi-value returns compare per slot.
//! - Deep guest recursion is heap-allocated in perl and only stops at the OOM killer, so exhaustion is the generated code's own `$Rt::DEPTH` cutoff — `check_exhaust` matches the resulting `call stack exhausted` trap rather than any interpreter error.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use dewasm_backend::{Backend, RuntimeLinkage};
use dewasm_backend_perl::PerlBackend;
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{AbstractHeapType, HeapType, NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

/// Known assertion-tier failures with their attribution; the file still runs so regressions in the passing assertions are caught. Identical in shape to the Ruby/Python ledgers (ADR-16): the only open gap is `import-limits` — `Rt::check_import_kind` validates the *kind* of a resolved import but not its finer wasm type (a global's mutability, a table/memory's min/max limits, a function's signature), so the `assert_unlinkable` cases that test those, plus the `linking`-tagged stale-state cases downstream of a declared-unsupported feature (multi-memory) that also happens to `register`, stay known gaps.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 28, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 4, "import-limits"),
    ("linking0", 1, "linking"),
    ("load1", 5, "linking"),
];

/// Files `cargo test` runs by default. Perl executes wasm in the same interpreter-speed class as Python (per-file `perl` startup plus a pure-perl numeric runtime), so — like Python and Bash (ADR-3 pre-accepts this) — the test runs a curated list covering every semantic area (integers, floats, control flow, memory/table, globals, linking, bulk ops) plus the whole list; `cargo test -- --include-ignored` runs everything.
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

pub struct PerlSpec;

impl BackendUnderTest for PerlSpec {
    fn name(&self) -> &'static str {
        "perl"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &PerlBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasm_backend_perl::find_perl()
            .expect("perl >= 5.26 with 64-bit IVs/NVs not found on PATH — see docs/testing.md")
    }
}

impl dewasm_test_helper::SpecBackend for PerlSpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    fn curated_files(&self) -> Option<&'static [&'static str]> {
        Some(CURATED_FILES)
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[
            "rt/trap",
            // check_unlinkable references Rt::LinkError even when the converted modules themselves don't.
            "rt/link_error",
            "rt/f32_bits",
            "rt/f32_from_bits",
            "rt/f64_bits",
            "rt/f64_from_bits",
            // Referenced by the $spectest fixture (PREAMBLE below), not necessarily by the converted module itself.
            "global/_package",
            "table/_package",
            "memory/_package",
            "rt/f32",
        ]
    }

    fn generate(
        &self,
        module: &ir::Module,
        counter: u32,
    ) -> anyhow::Result<dewasm_test_helper::Converted> {
        let package_name = format!("WastMod{counter}");
        let (source, units) = dewasm_backend_perl::generate_package_with_units(
            module,
            &package_name,
            &RuntimeLinkage::Alias("Rt".to_string()),
            false, // spec modules import spectest, never WASI
        )?;
        Ok(dewasm_test_helper::Converted {
            source,
            handle: package_name,
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
            "my {var} = {}->new({});",
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
        format!("{}->new({})", conv.handle, imports_expr(registered))
    }

    fn invoke(&self, var: &str, name: &str, args: &[WastArg<'_>]) -> Result<String, String> {
        let mut parts = vec![perl_str(name)];
        for arg in args {
            parts.push(arg_perl(arg)?);
        }
        Ok(format!("{var}->invoke({})", parts.join(", ")))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}->global_get({})", perl_str(global))
    }

    fn emit_check(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
        results: &[WastRet<'_>],
    ) -> Result<(), String> {
        let cmp = match results.len() {
            0 => "1".to_string(),
            1 => ret_cmp("$r[0]", &results[0])?,
            _ => {
                let mut parts = Vec::new();
                for (i, r) in results.iter().enumerate() {
                    parts.push(ret_cmp(&format!("$r[{i}]"), r)?);
                }
                parts.join(" && ")
            }
        };
        let _ = writeln!(
            script,
            "check({}, sub {{ my @r = ({call}); return ((({cmp}) ? 1 : 0), join(',', map {{ defined($_) ? $_ : 'undef' }} @r)); }});",
            perl_str(desc)
        );
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let _ = writeln!(
            script,
            "check_trap({}, {}, sub {{ {call}; }});",
            perl_str(desc),
            perl_str(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_exhaust({}, sub {{ {call}; }});",
            perl_str(desc)
        );
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check({}, sub {{ {call}; return (1, 'ok'); }});",
            perl_str(desc)
        );
    }

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(
            script,
            "check_unlinkable({}, sub {{ {call}; }});",
            perl_str(desc)
        );
    }

    fn assemble(
        &self,
        units: &BTreeSet<String>,
        _decls: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let mut script = dewasm_backend_perl::shared_runtime(units)
            .map_err(|e| anyhow::anyhow!("bundling runtime: {e:#}"))?;
        script.push('\n');
        script.push_str(PREAMBLE);
        script.push('\n');
        script.push_str(body);
        script.push_str("print \"RESULT pass=$pass fail=$fail\\n\";\n");
        Ok(script)
    }
}

/// `$spectest`, plus any currently-`register`ed instances merged in under their registered name — each instance doubles as an ADR-7 import provider (`wasm_import`).
fn imports_expr(registered: &[(String, String)]) -> String {
    if registered.is_empty() {
        return "$spectest".to_string();
    }
    let entries = registered
        .iter()
        .map(|(name, var)| format!("{} => {var}", perl_str(name)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ %$spectest, {entries} }}")
}

/// Perl single-quoted string literal (no interpolation, so `$`/`@` in wasm names are inert).
fn perl_str(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        match c {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Attribution for a null/ref heap type the harness cannot express as a Perl value; the reference-types hierarchies (and their bottoms, also just `undef`) are expressible.
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

fn arg_perl(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok((*v as u32).to_string()),
        WastArg::Core(WastArgCore::I64(v)) => Ok((*v as u64).to_string()),
        WastArg::Core(WastArgCore::F32(f)) => Ok(format!("Rt::f32_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::F64(f)) => Ok(format!("Rt::f64_from_bits(0x{:x})", f.bits)),
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(WastArgCore::RefNull(hty)) => {
            if nullable_heap_type(hty) {
                Ok("undef".to_string())
            } else {
                Err(heap_type_tag(hty))
            }
        }
        // An externref (or legacy hostref) with identity `n`: the host value is the integer itself (ADR-17: externref = raw host value).
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
                format!("(Rt::f32_bits({value}) & 0x7fffffff) == 0x7fc00000")
            }
            NanPattern::ArithmeticNan => {
                format!("(Rt::f32_bits({value}) & 0x7fc00000) == 0x7fc00000")
            }
            NanPattern::Value(f) => {
                format!("Rt::f32_bits({value}) == 0x{:x}", f.bits)
            }
        }),
        WastRet::Core(WastRetCore::F64(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => {
                format!("(Rt::f64_bits({value}) & 0x7fffffffffffffff) == 0x7ff8000000000000")
            }
            NanPattern::ArithmeticNan => {
                format!("(Rt::f64_bits({value}) & 0x7ff8000000000000) == 0x7ff8000000000000")
            }
            NanPattern::Value(f) => {
                format!("Rt::f64_bits({value}) == 0x{:x}", f.bits)
            }
        }),
        WastRet::Core(WastRetCore::V128(_)) => Err("simd".to_string()),
        WastRet::Core(WastRetCore::Either(_)) => Err("either-results".to_string()),
        WastRet::Core(WastRetCore::RefNull(hty)) => match hty {
            None => Ok(format!("!defined({value})")),
            Some(hty) if nullable_heap_type(hty) => Ok(format!("!defined({value})")),
            Some(hty) => Err(heap_type_tag(hty)),
        },
        WastRet::Core(WastRetCore::RefExtern(Some(n))) => Ok(format!("{value} == {n}")),
        // `(ref.extern)`: any non-null externref.
        WastRet::Core(WastRetCore::RefExtern(None)) => Ok(format!("defined({value})")),
        WastRet::Core(WastRetCore::RefHost(n)) => Ok(format!("{value} == {n}")),
        // `(ref.func)`: any non-null funcref — in ADR-17's representation, a `[type_string, coderef]` pair.
        WastRet::Core(WastRetCore::RefFunc(None)) => Ok(format!("ref({value}) eq 'ARRAY'")),
        // A specific function's identity: not expressible without an export map; no top-tier testsuite file uses it.
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

const PREAMBLE: &str = r#"my $pass = 0;
my $fail = 0;

sub check {
    my ($desc, $thunk) = @_;
    my ($ok, $actual) = eval { $thunk->() };
    if ($@) {
        my $e = ref($@) ? ($@->{message} // 'object') : $@;
        $e =~ s/\n+$//;
        $fail++;
        print "FAIL(error: $e): $desc\n";
        return;
    }
    if ($ok) { $pass++; }
    else {
        $fail++;
        $actual = 'undef' unless defined $actual;
        print "FAIL: $desc (got $actual)\n";
    }
}

sub check_trap {
    my ($desc, $msg, $thunk) = @_;
    my $done = eval { $thunk->(); 1 };
    if ($done) {
        $fail++;
        print "FAIL(no trap, want '$msg'): $desc\n";
        return;
    }
    my $e = $@;
    if (ref($e) && $e->isa('Rt::Trap')) {
        my $m = $e->{message};
        if (index($m, $msg) >= 0 || index($msg, $m) >= 0) { $pass++; }
        else {
            $fail++;
            print "FAIL(trap '$m', want '$msg'): $desc\n";
        }
    }
    else {
        $e =~ s/\n+$// unless ref($e);
        $fail++;
        print "FAIL(error '$e', want trap '$msg'): $desc\n";
    }
}

sub check_exhaust {
    my ($desc, $thunk) = @_;
    my $done = eval { $thunk->(); 1 };
    if (!$done && ref($@) && $@->isa('Rt::Trap') && $@->{message} eq 'call stack exhausted') {
        $pass++;
        return;
    }
    $fail++;
    print "FAIL(no exhaustion): $desc\n";
}

# Upstream's assert_unlinkable message text never matches ours; a raised
# Rt::LinkError confirms the import was correctly rejected as unlinkable. Any
# other error means the module linked and then crashed, which must not pass.
sub check_unlinkable {
    my ($desc, $thunk) = @_;
    my $done = eval { $thunk->(); 1 };
    if (!$done && ref($@) && $@->isa('Rt::LinkError')) {
        $pass++;
        return;
    }
    $fail++;
    print "FAIL(no link error, want unlinkable): $desc\n";
}

my $spectest = {
    'spectest' => {
        'print' => sub { },
        'print_i32' => sub { },
        'print_i64' => sub { },
        'print_f32' => sub { },
        'print_f64' => sub { },
        'print_i32_f32' => sub { },
        'print_f64_f64' => sub { },
        'global_i32' => Rt::Global->new(666),
        'global_i64' => Rt::Global->new(666),
        'global_f32' => Rt::Global->new(Rt::f32(666.6)),
        'global_f64' => Rt::Global->new(666.6),
        'table' => Rt::Table->new(10, 20),
        'memory' => Rt::Memory->new(1, 2),
    },
};
"#;

dewasm_test_helper::spec_suite!(PerlSpec);
