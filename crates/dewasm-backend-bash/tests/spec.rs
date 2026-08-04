//! Bash side of the shared spec harness (ADR-3, ADR-27): converts modules with the Bash backend, phrases assertions as bash (`ck`/`ckt`/`cke` helpers over the R0..Rn result globals and the status-134 trap protocol), and runs the script with a discovered bash >= 5 (macOS system bash is 3.2).
//!
//! Bash executes wasm orders of magnitude slower than Ruby, so `cargo test` runs a curated file list (ADR-3 pre-accepts this); the rest are `#[ignore]`d trials, so `cargo test -- --include-ignored` runs everything. The generic harness lives in `dewasm-test-helper`.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_bash::BashBackend;
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

/// Known assertion-level failures (ADR-35). Cross-module linking of function, global, memory, and now table imports (through PROVIDERS and the per-kind export maps) is fully wired; `assert_unlinkable` is checked for real. Two residual clusters, both pre-existing and out of this backend's scope to fix (matching the Ruby list, which has already covered every import kind for a while):
///
/// - `import-limits` (`imports`, `imports2`, 4 of `linking`'s 4): `rt_resolve_import` validates that a resolved import is the right *kind* (func/global/table/memory) but not the finer-grained wasm type — a function's param/result signature, a global's mutability, a table's min/max limits, or a memory's min/max limits. Every `assert_unlinkable` case testing one of those (not a kind mismatch, which is caught) links instead of failing. Same accepted gap as the Ruby list's `import-limits`, and now the same count (28/2/4) since Bash supports every import kind Ruby does.
/// - `multi-memory` (`linking0`, `load1`): a second `(memory ...)` declaration or import is rejected outright by the core builder (`Feature::MultiMemory`, a post-1.0 proposal, ADR-24) regardless of backend. Both files exercise this via a module with two memories (one often an import of another module's exported memory); that module fails to convert, so the data/assertions that depended on it running observe stale (zeroed) state in a memory another, unrelated module still owns. Not a linking gap — every import in play resolves fine — and not fixable without the multi-memory proposal, which ADR-24 rejects outright.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 28, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 4, "import-limits"),
    ("linking0", 1, "multi-memory"),
    ("load1", 5, "multi-memory"),
];

/// Files `cargo test` runs by default: integer-only files the backend's current milestones cover, plus files whose value is their Rust-side assert_invalid checks and attributed `floats` skips.
const CURATED_FILES: &[&str] = &[
    "address",
    "address0",
    "address1",
    "block",
    "br",
    "br_if",
    "br_table",
    "bulk",
    "call",
    "comments",
    "data",
    "data0",
    "data1",
    "elem",
    "endianness",
    "exports",
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
    "conversions",
    "forward",
    "func_ptrs",
    "global",
    "i32",
    "i64",
    "if",
    "imports",
    "imports0",
    "imports1",
    "imports2",
    "imports3",
    "imports4",
    "int_exprs",
    "int_literals",
    "labels",
    "linking",
    "linking0",
    "linking1",
    "linking2",
    "linking3",
    "load",
    "load0",
    "load1",
    "load2",
    "local_get",
    "local_set",
    "local_tee",
    "loop",
    "memory",
    "memory_copy0",
    "memory_fill0",
    "memory_grow",
    "memory_init0",
    "memory_redundancy",
    "memory_size",
    "memory_size0",
    "memory_size1",
    "memory_size2",
    "memory_trap0",
    "nop",
    "return",
    "select",
    "stack",
    "store",
    "store0",
    "store1",
    "store2",
    "switch",
    "table",
    "table_copy",
    "table_copy_mixed",
    "table_init",
    "traps",
    "traps0",
    "type",
    "unreachable",
    "unreached-valid",
    "unwind",
];

pub struct BashSpec;

impl BackendUnderTest for BashSpec {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &BashBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasm_backend_bash::find_bash5().expect(
            "bash >= 5 not found (checked $DEWASM_BASH, PATH, homebrew) — see docs/testing.md",
        )
    }
}

impl dewasm_test_helper::SpecBackend for BashSpec {
    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    fn curated_files(&self) -> Option<&'static [&'static str]> {
        Some(CURATED_FILES)
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[]
    }

    fn generate(
        &self,
        module: &ir::Module,
        counter: u32,
    ) -> anyhow::Result<dewasm_test_helper::Converted> {
        let prefix = format!("m{counter}_");
        let (source, units) = dewasm_backend_bash::generate_module_with_units(
            module, &prefix, false, // spec modules import spectest, never WASI
        )?;
        Ok(dewasm_test_helper::Converted {
            source,
            handle: prefix,
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
        _var_id: u32,
        registered: &[(String, String)],
    ) -> String {
        script.push_str(&conv.source);
        // Rebuild PROVIDERS from the *current* registered set before every instantiation (ADR-35): a plain reassignment fully replaces the associative array on bash >= 5, so a module registered after a failed one never leaves a stale provider entry behind.
        let _ = writeln!(script, "{}", providers_line(registered));
        // A trap while instantiating a plain module directive aborts the file, mirroring an uncaught Ruby exception at toplevel.
        let _ = writeln!(
            script,
            "{}init || {{ echo \"toplevel init failed (status $?): $TRAP_MSG\" >&2; exit 1; }}",
            conv.handle
        );
        conv.handle.clone()
    }

    fn instantiate_call(
        &self,
        script: &mut String,
        _decls: &mut String,
        conv: &dewasm_test_helper::Converted,
        registered: &[(String, String)],
    ) -> String {
        script.push_str(&conv.source);
        let _ = writeln!(script, "{}", providers_line(registered));
        format!("{}init", conv.handle)
    }

    fn invoke(&self, var: &str, name: &str, args: &[WastArg<'_>]) -> Result<String, String> {
        let mut parts = vec![format!("{var}invoke"), bash_str(name)];
        for arg in args {
            parts.push(arg_bash(arg)?);
        }
        Ok(parts.join(" "))
    }

    fn global_get(&self, var: &str, global: &str) -> String {
        format!("{var}global_get {}", bash_str(global))
    }

    fn emit_check(
        &self,
        script: &mut String,
        desc: &str,
        call: &str,
        results: &[WastRet<'_>],
    ) -> Result<(), String> {
        let cond = match results.len() {
            0 => "1".to_string(),
            _ => {
                let mut parts = Vec::new();
                for (i, r) in results.iter().enumerate() {
                    parts.push(ret_cond(i, r)?);
                }
                parts.join(" && ")
            }
        };
        let _ = writeln!(
            script,
            "{call}\nck $? {} {}",
            bash_str(desc),
            bash_str(&cond)
        );
        Ok(())
    }

    fn emit_check_trap(&self, script: &mut String, desc: &str, call: &str, message: &str) {
        let _ = writeln!(
            script,
            "{call}\nckt $? {} {}",
            bash_str(desc),
            bash_str(message)
        );
    }

    fn emit_check_exhaust(&self, script: &mut String, desc: &str, call: &str) {
        // FUNCNEST overflow kills the shell it happens in, so exhaustion must run in a subshell; nothing else needs one.
        let _ = writeln!(
            script,
            "( FUNCNEST=1000; {call} ) 2>/dev/null\ncke $? {}",
            bash_str(desc)
        );
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(script, "{call}\nck $? {} '1'", bash_str(desc));
    }

    fn emit_check_unlinkable(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(script, "{call}\nckl $? {}", bash_str(desc));
    }

    fn assemble(
        &self,
        units: &BTreeSet<String>,
        _decls: &str,
        body: &str,
    ) -> anyhow::Result<String> {
        let mut script = String::from("#!/usr/bin/env bash\n\n");
        script.push_str(&dewasm_backend_bash::shared_runtime(units)?);
        script.push_str(PREAMBLE);
        script.push_str(body);
        script.push_str(POSTAMBLE);
        Ok(script)
    }
}

fn bash_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The PROVIDERS rebuild line for an instantiation: always the `spectest` host prefix plus each currently-`register`ed module mapped to its generation prefix (the `conv.handle` returned by `emit_instantiate`).
fn providers_line(registered: &[(String, String)]) -> String {
    let mut entries = vec!["[spectest]=spectest_".to_string()];
    for (name, prefix) in registered {
        entries.push(format!("[{}]={prefix}", bash_str(name)));
    }
    format!("PROVIDERS=({})", entries.join(" "))
}

fn arg_bash(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok((*v as u32).to_string()),
        // i64/f64 travel as the signed-64 bit pattern, f32 as its u32 pattern (ADR-11/ADR-13).
        WastArg::Core(WastArgCore::I64(v)) => Ok(v.to_string()),
        WastArg::Core(WastArgCore::F32(f)) => Ok(f.bits.to_string()),
        WastArg::Core(WastArgCore::F64(f)) => Ok((f.bits as i64).to_string()),
        WastArg::Core(WastArgCore::V128(_)) => Err("simd".to_string()),
        WastArg::Core(_) => Err("reference-types".to_string()),
        _ => Err("component-model".to_string()),
    }
}

fn ret_cond(i: usize, ret: &WastRet<'_>) -> Result<String, String> {
    match ret {
        WastRet::Core(WastRetCore::I32(v)) => Ok(format!("R{i} == {}", *v as u32)),
        WastRet::Core(WastRetCore::I64(v)) => Ok(format!("R{i} == {v}")),
        // Floats are compared as bit patterns (they already are the R registers' representation); the NaN masks mirror the ruby side's ret_cmp, as signed-64-safe integer constants.
        WastRet::Core(WastRetCore::F32(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => {
                format!("(R{i} & 0x7fffffff) == 0x7fc00000")
            }
            NanPattern::ArithmeticNan => {
                format!("(R{i} & 0x7fc00000) == 0x7fc00000")
            }
            NanPattern::Value(f) => format!("R{i} == {}", f.bits),
        }),
        WastRet::Core(WastRetCore::F64(pattern)) => Ok(match pattern {
            NanPattern::CanonicalNan => {
                format!("(R{i} & 0x7fffffffffffffff) == 0x7ff8000000000000")
            }
            NanPattern::ArithmeticNan => {
                format!("(R{i} & 0x7ff8000000000000) == 0x7ff8000000000000")
            }
            NanPattern::Value(f) => format!("R{i} == {}", f.bits as i64),
        }),
        WastRet::Core(WastRetCore::V128(_)) => Err("simd".to_string()),
        WastRet::Core(WastRetCore::Either(_)) => Err("either-results".to_string()),
        WastRet::Core(_) => Err("reference-types".to_string()),
        _ => Err("component-model".to_string()),
    }
}

const PREAMBLE: &str = r#"
PASS=0
FAIL=0

# ck <status> <desc> <cond>: cond is a bash arithmetic expression over the
# R0..Rn result globals (recursively evaluated by (( )) ).
ck() {
  local st=$1 desc=$2 cond=$3
  if (( st == 0 )); then
    if (( cond )); then
      (( PASS += 1 ))
    else
      (( FAIL += 1 ))
      echo "FAIL: $desc (got R0=${R0-} R1=${R1-}, want $cond)"
    fi
  else
    (( FAIL += 1 ))
    echo "FAIL(status $st: $TRAP_MSG): $desc"
  fi
  return 0
}

ckt() {
  local st=$1 desc=$2 want=$3
  if (( st == 134 )); then
    if [[ $TRAP_MSG == *"$want"* || $want == *"$TRAP_MSG"* ]]; then
      (( PASS += 1 ))
    else
      (( FAIL += 1 ))
      echo "FAIL(trap $TRAP_MSG, want $want): $desc"
    fi
  elif (( st == 0 )); then
    (( FAIL += 1 ))
    echo "FAIL(no trap, want $want): $desc"
  else
    (( FAIL += 1 ))
    echo "FAIL(status $st, want trap $want): $desc"
  fi
  return 0
}

cke() {
  local st=$1 desc=$2
  if (( st != 0 && st != 134 )); then
    (( PASS += 1 ))
  else
    (( FAIL += 1 ))
    echo "FAIL(no exhaustion, status $st): $desc"
  fi
  return 0
}

# ckl <status> <desc>: an assert_unlinkable check. Upstream's wording never
# matches ours, so only the status is inspected: exactly 135 (rt_link_err)
# is a pass; 0 means it linked when it shouldn't, 134 means it trapped after
# linking, anything else crashed — all failures (mirrors ruby check_unlinkable).
ckl() {
  local st=$1 desc=$2
  if (( st == 135 )); then
    (( PASS += 1 ))
  elif (( st == 0 )); then
    (( FAIL += 1 ))
    echo "FAIL(linked, want unlinkable): $desc"
  elif (( st == 134 )); then
    (( FAIL += 1 ))
    echo "FAIL(trapped '$TRAP_MSG', want unlinkable): $desc"
  else
    (( FAIL += 1 ))
    echo "FAIL(status $st, want unlinkable): $desc"
  fi
  return 0
}

# The $spectest host module as an ADR-35 provider: a `spectest_`-prefixed
# family of per-kind export maps that PROVIDERS[spectest] points at. IMPORTS
# stays declared (empty) as the function-only host override channel.
declare -A IMPORTS=()
spectest_print() { return 0; }
declare -A spectest_EXPORTS=(
  ['print']=spectest_print
  ['print_i32']=spectest_print
  ['print_i64']=spectest_print
  ['print_f32']=spectest_print
  ['print_f64']=spectest_print
  ['print_i32_f32']=spectest_print
  ['print_f64_f64']=spectest_print
)
# Global fixture values from the wast spectest host module: i32/i64 = 666;
# f32 = the u32 bit pattern of 666.6f (1143383654); f64 = the signed-64 bit
# pattern of 666.6 (4649074691427585229) — the bash float representation
# (ADR-13). Globals flatten to their backing variable names (nameref depth 1).
spectest_g_i32=666
spectest_g_i64=666
spectest_g_f32=1143383654
spectest_g_f64=4649074691427585229
declare -A spectest_GLOBAL_EXPORTS=(
  ['global_i32']=spectest_g_i32
  ['global_i64']=spectest_g_i64
  ['global_f32']=spectest_g_f32
  ['global_f64']=spectest_g_f64
)
# Table/memory fixtures are inert until the imported-table/-memory steps but
# complete the provider shape so a kind mismatch against them link-errors.
spectest_t0=()
spectest_t0ty=()
spectest_t0sz=10
declare -A spectest_TABLE_EXPORTS=(['table']=spectest_t0)
spectest_mem=()
spectest_pages=1
spectest_max_pages=2
declare -A spectest_MEMORY_EXPORTS=(['memory']=spectest_)
declare -A PROVIDERS=([spectest]=spectest_)
"#;

const POSTAMBLE: &str = r#"
echo "RESULT pass=$PASS fail=$FAIL"
"#;

dewasm_test_helper::spec_suite!(BashSpec);
