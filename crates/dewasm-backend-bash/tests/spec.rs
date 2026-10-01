//! Bash side of the shared specification harness: converts modules with the Bash backend.
//! It phrases assertions as Bash through the `ck`/`ckt`/`cke` helpers.
//! Those helpers read the R0..Rn result globals and follow the status-134 trap protocol.
//! It runs the script with a discovered `bash` >= 5 (macOS system `bash` is 3.2).
//!
//! Bash executes wasm orders of magnitude slower than Ruby.
//! So `cargo test` runs a curated file list.
//! The rest are `#[ignore]`d trials, unless the `slow_test` feature is on.
//! The generic harness lives in `dewasm-test-helper`.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use dewasm_backend::Backend;
use dewasm_backend_bash::{bash_str, BashBackend};
use dewasm_core::ir;
use dewasm_test_helper::BackendUnderTest;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

/// Known assertion-level failures.
/// Cross-module linking of function, global, memory, and now table imports is implemented.
/// It goes through PROVIDERS and the per-kind export maps.
/// `assert_unlinkable` is checked for real.
/// Two residual clusters remain, both pre-existing and out of this backend's scope to fix.
/// They match the Ruby list, which has already covered every import kind for a while:
///
/// - `import-limits` (`imports`, `imports2`, 4 of `linking`'s 4).
///   `rt_resolve_import` validates that a resolved import is the right *kind*.
///   The kinds are `func`, `global`, `table`, and `memory`.
///   It does not validate the finer-grained wasm type:
///   - a function's parameter/result signature;
///   - a global's mutability;
///   - a table's minimum/maximum limits;
///   - a memory's minimum/maximum limits.
///
///   Every `assert_unlinkable` case testing one of those links instead of failing.
///   A kind mismatch is caught.
///   This is the same accepted gap as the Ruby list's `import-limits`.
///   It now has the same count (28/2/4), since Bash supports every import kind Ruby does.
/// - `multi-memory` (`linking0`, `load1`): out-of-date state left by a module that never converts.
///   The module that calls `register` is not the one that uses multi-memory.
///   `linking0` registers `$Mt`, which has one table and no memory.
///   `load1` registers `$M`, which exports its one memory.
///   A later module imports that table or memory and writes into it as it instantiates.
///   That later module declares several memories, and multi-memory is declared unsupported.
///   So it never converts, and its writes never happen.
///   A later assertion against the registered module then observes out-of-date state.
///   In `linking0`, slot 7 of `$Mt`'s table stays null (1 failure).
///   The write must persist there although the writing module's instantiation then traps.
///   In `load1`, bytes 20 to 24 of `$M`'s memory stay zero (5 failures).
///   This is not a gap in cross-module linking itself: every import in play resolves.
///   The core builder rejects a second `(memory ...)` outright.
///   That holds for a declaration or an import, regardless of backend.
///   The feature is `Feature::MultiMemory`, a post-1.0 proposal.
///   It is not fixable without the multi-memory proposal.
///   That proposal is rejected outright as a post-1.0 wasm feature.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("imports", 28, "import-limits"),
    ("imports2", 2, "import-limits"),
    ("linking", 4, "import-limits"),
    ("linking0", 1, "multi-memory"),
    ("load1", 5, "multi-memory"),
];

/// Files `cargo test` runs by default; every other file is an `#[ignore]`d trial.
/// `slow_test` runs everything.
/// The list is curated separately from the shared list.
/// The heavy float files stay out because every float operation runs on the softfloat.
/// The tail-call pair stays out for the same reason.
/// Its million-deep chains are millions of Bash function calls.
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
            "bash >= 5 not found (checked $DEWASM_BASH, PATH, homebrew): see docs/testing.md",
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
        // Rebuild PROVIDERS from the *current* registered set before every instantiation.
        // A plain reassignment replaces the whole associative array on Bash >= 5.
        // So a module registered after a failed one never leaves an out-of-date provider entry.
        let _ = writeln!(script, "{}", providers_line(registered));
        // A trap while instantiating a plain module directive stops the file.
        // This mirrors an uncaught Ruby exception at the top level.
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
        // FUNCNEST overflow kills the shell it happens in, so exhaustion must run in a subshell.
        // Nothing else needs one.
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

/// The PROVIDERS rebuild line for an instantiation.
/// It always holds the `spectest` host prefix.
/// It also maps each currently-`register`ed module to its generation prefix.
/// That prefix is the `conv.handle` returned by `emit_instantiate`.
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
        // i64/f64 travel as the signed-64 bit pattern, f32 as its u32 pattern.
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
        // Floats are compared as bit patterns, which already are the R registers' representation.
        // The NaN masks mirror the Ruby side's `ret_cmp`, as signed-64-safe integer constants.
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
# linking, anything else crashed, all failures (mirrors ruby check_unlinkable).
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

# The $spectest host module as a provider: a `spectest_`-prefixed
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
# pattern of 666.6 (4649074691427585229), the bash float representation.
# Globals flatten to their backing variable names (nameref depth 1).
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
