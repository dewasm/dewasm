//! Library-mode scenarios over hand-written `.wat` fixtures
//! (`examples/wat/`). These do need per-language glue (Ruby method calls
//! vs. Bash function calls against globals) — that can't be shared, but
//! each glue is written to observe the same thing the same way (e.g.
//! both intercept fd_write and print the literal bytes written, rather
//! than a language-specific diagnostic), so one table (`LIBRARY_CASES`)
//! can still pin one `expect` per scenario instead of one per language
//! that could quietly drift apart. Adding a scenario is one row instead
//! of two hand-rolled `#[test]` functions; a case's `glues` list names
//! which languages run it, so a language missing a glue entry for a case
//! its tier requires fails loudly instead of silently not being tested
//! (ADR-23).
//!
//! Scenarios with no counterpart in the other language live in `ruby`
//! instead (Bash has no object-provider model and no WASI filesystem
//! support yet).

use std::path::Path;

use dewasmify_backend::{Mode, Tier};
use dewasmify_backend_bash::{find_bash5, BashBackend};
use dewasmify_backend_ruby::RubyBackend;

use crate::support::{
    convert, examples_dir, print_tier_skip, run_bash, run_ruby, tier_ok, BashLang, RubyLang,
};

struct LibraryCase {
    name: &'static str,
    wat: &'static str,
    module_name: &'static str,
    tier: Tier,
    /// (language name, glue source) pairs; `glue_for` panics if a
    /// language whose tier covers this case has no entry here.
    glues: &'static [(&'static str, &'static str)],
    /// Both sides are engineered to produce this same string — the glue
    /// captures and prints the actual bytes the wasm module wrote,
    /// rather than a language-specific diagnostic, so there is exactly
    /// one expectation per scenario instead of one per language that
    /// could quietly drift apart.
    expect: &'static str,
}

fn glue_for(case: &LibraryCase, lang: &str) -> &'static str {
    case.glues
        .iter()
        .find(|(l, _)| *l == lang)
        .unwrap_or_else(|| panic!("{}: no glue for {lang}", case.name))
        .1
}

const LIBRARY_CASES: &[LibraryCase] = &[
    LibraryCase {
        name: "add",
        wat: "add.wat",
        module_name: "add",
        tier: Tier::Tier3,
        glues: &[
            (
                "ruby",
                "inst = Add.new\n\
                 print inst.invoke(\"add\", 2, 3), \"\\n\"\n\
                 print inst.invoke(\"add\", 0xffffffff, 1), \"\\n\"\n\
                 print inst.invoke(\"fib\", 10), \"\\n\"",
            ),
            // ADR-11: results come back through the global R0.
            (
                "bash",
                "add_init || exit 1\n\
                 add_invoke add 2 3; echo $R0\n\
                 add_invoke add 4294967295 1; echo $R0\n\
                 add_invoke fib 10; echo $R0\n",
            ),
        ],
        expect: "5\n0\n55\n",
    },
    // The ADR-7 override/fallback semantics: an explicit import wins,
    // an unhandled one falls back to the bundled WASI. Both glues
    // intercept fd_write and print the actual bytes the module wrote
    // (rather than a fd/len diagnostic) — that's the one observable
    // both languages can produce identically, so there's a single
    // `expect` instead of a per-language one. Both sides only touch
    // fd_write/random_get (Tier 3 WASI), so the override *mechanism*
    // itself doesn't need anything beyond Tier 3.
    LibraryCase {
        name: "wasi_import_override",
        wat: "wasi_imports.wat",
        module_name: "prog",
        tier: Tier::Tier3,
        glues: &[("ruby", RUBY_OVERRIDE_GLUE), ("bash", BASH_OVERRIDE_GLUE)],
        expect: "ok\n",
    },
];

/// Also reused by `ruby::partial_override_falls_back_to_bundled_wasi`.
pub const RUBY_OVERRIDE_GLUE: &str = r#"captured = +""
holder = {}
fd_write = lambda do |_fd, iovs, _iovs_len, out_ptr|
  mem = holder[:inst].memory
  ptr = mem.bytes.unpack1("L<", offset: iovs)
  len = mem.bytes.unpack1("L<", offset: iovs + 4)
  captured << mem.bytes.byteslice(ptr, len)
  mem.bytes[out_ptr, 4] = [len].pack("L<")
  0
end
inst = Prog.new({ "wasi_snapshot_preview1" => { "fd_write" => fd_write } })
holder[:inst] = inst
inst.invoke("_start") # random_get falls back to the bundled WASI
print captured
"#;

const BASH_OVERRIDE_GLUE: &str = r#"my_fd_write() {
  # (fd, iovs, iovs_len, nwritten_ptr): capture and print the actual
  # bytes the module wrote, the same observable proof of interception
  # `RUBY_OVERRIDE_GLUE` uses, via the same byte-reconstruction the
  # bundled fd_write unit itself uses (runtime/bash/units/wasi/fd_write.sh).
  mem_i32_load prog_ "$2" || return $?
  local ptr=$R0
  mem_i32_load prog_ $(( $2 + 4 )) || return $?
  local len=$R0
  local -n mem=prog_mem
  local out='' chunk bytes=() j
  for (( j = 0; j < len; j++ )); do
    bytes+=("$(( mem[ptr + j] ))")
  done
  printf -v chunk '\\x%02x' "${bytes[@]}"
  out+=$chunk
  printf "$out"
  mem_i32_store prog_ "$4" "$len" || return $?
  R0=0
  return 0
}
declare -A IMPORTS=(['wasi_snapshot_preview1.fd_write']=my_fd_write)
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
"#;

fn check_library_case_ruby(case: &LibraryCase) {
    if !tier_ok(&RubyLang, case.tier) {
        print_tier_skip(case.name, &RubyLang, case.tier);
        return;
    }
    let code = convert(
        &RubyBackend,
        &examples_dir().join(case.wat),
        Mode::Library,
        case.module_name,
    );
    let out = run_ruby(&format!("{code}\n{}", glue_for(case, "ruby")), &[]);
    assert_eq!(out, case.expect, "{}: ruby stdout", case.name);
}

fn check_library_case_bash(bash: &Path, case: &LibraryCase) {
    if !tier_ok(&BashLang, case.tier) {
        print_tier_skip(case.name, &BashLang, case.tier);
        return;
    }
    let code = convert(
        &BashBackend,
        &examples_dir().join(case.wat),
        Mode::Library,
        case.module_name,
    );
    let output = run_bash(bash, &format!("{code}\n{}", glue_for(case, "bash")), &[]);
    assert!(
        output.status.success(),
        "{}: bash failed: {}\n{}",
        case.name,
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect,
        "{}: bash stdout",
        case.name
    );
}

#[test]
fn library_cases_ruby() {
    for case in LIBRARY_CASES {
        check_library_case_ruby(case);
    }
}

#[test]
fn library_cases_bash() {
    let bash = find_bash5().expect("bash >= 5 not found — see docs/testing.md");
    for case in LIBRARY_CASES {
        check_library_case_bash(&bash, case);
    }
}
