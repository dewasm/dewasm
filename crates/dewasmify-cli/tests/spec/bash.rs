//! Bash side of the spec harness: converts modules with the Bash backend,
//! phrases assertions as bash (`ck`/`ckt`/`cke` helpers over the R0..Rn
//! result globals and the status-134 trap protocol), and runs the script
//! with a discovered bash >= 5 (macOS system bash is 3.2).
//!
//! Bash executes wasm orders of magnitude slower than Ruby, so `cargo
//! test` runs a curated file list (ADR-3 pre-accepts this);
//! `DEWASMIFY_SPEC_ALL=1` sweeps everything.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;

use dewasmify_backend::Backend;
use dewasmify_backend_bash::BashBackend;
use dewasmify_core::ir;
use wast::core::{NanPattern, WastArgCore, WastRetCore};
use wast::{WastArg, WastRet};

use crate::{Converted, SpecLang};

/// Identical to the Ruby ledger: the same stale-state cross-module
/// linking failures (a `register`ed module was supposed to write into the
/// first module's shared table/memory, so assertions see stale state).
/// The linking milestone clears these.
const EXPECTED_FAILURES: &[(&str, u32, &str)] = &[
    ("elem", 5, "linking"),
    ("linking", 10, "linking"),
    ("linking0", 1, "linking"),
    ("linking3", 2, "linking"),
    ("load1", 5, "linking"),
];

/// Files `cargo test` runs by default: integer-only files the backend's
/// current milestones cover, plus files whose value is their Rust-side
/// assert_invalid checks and attributed `floats` skips.
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
    "int_exprs",
    "int_literals",
    "labels",
    "linking",
    "linking0",
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
    "traps",
    "traps0",
    "type",
    "unreachable",
    "unreached-valid",
    "unwind",
];

pub struct BashLang;

impl SpecLang for BashLang {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn backend(&self) -> &dyn Backend {
        &BashBackend
    }

    fn interpreter(&self) -> PathBuf {
        dewasmify_backend_bash::find_bash5().expect(
            "bash >= 5 not found (checked $DEWASMIFY_BASH, PATH, homebrew) — see docs/testing.md",
        )
    }

    fn script_ext(&self) -> &'static str {
        "sh"
    }

    fn expected_failures(&self) -> &'static [(&'static str, u32, &'static str)] {
        EXPECTED_FAILURES
    }

    fn default_files(&self) -> Option<&'static [&'static str]> {
        Some(CURATED_FILES)
    }

    fn seed_units(&self) -> &'static [&'static str] {
        &[]
    }

    fn generate(&self, module: &ir::Module, counter: u32) -> anyhow::Result<Converted> {
        let prefix = format!("m{counter}_");
        let (source, units) = dewasmify_backend_bash::generate_module_with_units(
            module, &prefix, false, // spec modules import spectest, never WASI
        )?;
        Ok(Converted {
            source,
            handle: prefix,
            units,
        })
    }

    fn emit_instantiate(
        &self,
        script: &mut String,
        conv: &Converted,
        _var_id: u32,
        _registered: &[(String, String)],
    ) -> String {
        script.push_str(&conv.source);
        // A trap while instantiating a plain module directive aborts the
        // file, mirroring an uncaught Ruby exception at toplevel.
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
        conv: &Converted,
        _registered: &[(String, String)],
    ) -> String {
        script.push_str(&conv.source);
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
        // FUNCNEST overflow kills the shell it happens in, so exhaustion
        // must run in a subshell; nothing else needs one.
        let _ = writeln!(
            script,
            "( FUNCNEST=1000; {call} ) 2>/dev/null\ncke $? {}",
            bash_str(desc)
        );
    }

    fn emit_bare_invoke(&self, script: &mut String, desc: &str, call: &str) {
        let _ = writeln!(script, "{call}\nck $? {} '1'", bash_str(desc));
    }

    fn assemble(&self, units: &BTreeSet<String>, body: &str) -> anyhow::Result<String> {
        let mut script = String::from("#!/usr/bin/env bash\n\n");
        script.push_str(&dewasmify_backend_bash::shared_runtime(units)?);
        script.push_str(PREAMBLE);
        script.push_str(body);
        script.push_str(POSTAMBLE);
        Ok(script)
    }
}

fn bash_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn arg_bash(arg: &WastArg<'_>) -> Result<String, String> {
    match arg {
        WastArg::Core(WastArgCore::I32(v)) => Ok((*v as u32).to_string()),
        // i64/f64 travel as the signed-64 bit pattern, f32 as its u32
        // pattern (ADR-11/ADR-13).
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
        // Floats are compared as bit patterns (they already are the
        // R registers' representation); the NaN masks mirror ruby.rs's
        // ret_cmp, as signed-64-safe integer constants.
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

spectest_print() { return 0; }
declare -A IMPORTS=(
  ['spectest.print']=spectest_print
  ['spectest.print_i32']=spectest_print
  ['spectest.print_i64']=spectest_print
  ['spectest.print_f32']=spectest_print
  ['spectest.print_f64']=spectest_print
  ['spectest.print_i32_f32']=spectest_print
  ['spectest.print_f64_f64']=spectest_print
)
"#;

const POSTAMBLE: &str = r#"
echo "RESULT pass=$PASS fail=$FAIL"
"#;
