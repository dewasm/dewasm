//! Hand-written `.wat` fixtures (under `examples/wat/`) run through the
//! real interpreters, checked against fixed expected output.
//!
//! Standalone-mode scenarios need no host-language glue at all (the
//! compiled program *is* the runnable artifact), so they're driven from
//! one data table (`STANDALONE_CASES`) exercised by both languages.
//! Library-mode scenarios do need per-language glue (Ruby method calls
//! vs. Bash function calls against globals) — that can't be shared, but
//! each glue is written to observe the same thing the same way (e.g.
//! both intercept fd_write and print the literal bytes written, rather
//! than a language-specific diagnostic), so one table (`LIBRARY_CASES`)
//! can still pin one `expect` per scenario instead of one per language
//! that could quietly drift apart. Adding a scenario is one row instead
//! of two hand-rolled `#[test]` functions.
//!
//! A few scenarios genuinely have no counterpart in the other language
//! (Bash has no object-provider model, no WASI filesystem support yet)
//! — those stay as individual `#[test]` functions below the tables,
//! each noting why.

use std::path::Path;
use std::process::Output;

use dewasmify_backend::{Backend, Mode};
use dewasmify_backend_bash::{find_bash5, BashBackend};
use dewasmify_backend_ruby::{find_ruby, RubyBackend};

use crate::support::{convert, examples_dir, run_bash, run_ruby, run_ruby_capture};

struct StandaloneCase {
    name: &'static str,
    wat: &'static str,
    args: &'static [&'static str],
    expect_stdout: &'static str,
    expect_code: i32,
}

const STANDALONE_CASES: &[StandaloneCase] = &[
    StandaloneCase {
        name: "hello",
        wat: "hello.wat",
        args: &[],
        expect_stdout: "Hello, WASI!\n",
        expect_code: 0,
    },
    // argc (program name + arguments) becomes the exit code via
    // args_sizes_get + proc_exit.
    StandaloneCase {
        name: "argc",
        wat: "args_proc_exit.wat",
        args: &["foo", "bar"],
        expect_stdout: "",
        expect_code: 3,
    },
];

fn check_standalone_case(
    backend: &dyn Backend,
    case: &StandaloneCase,
    run: impl Fn(&str, &[&str]) -> Output,
) {
    let code = convert(
        backend,
        &examples_dir().join(case.wat),
        Mode::Standalone,
        case.name,
    );
    let output = run(&code, case.args);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect_stdout,
        "{}: stdout",
        case.name
    );
    assert_eq!(
        output.status.code(),
        Some(case.expect_code),
        "{}: exit code",
        case.name
    );
}

#[test]
fn standalone_cases_ruby() {
    find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    for case in STANDALONE_CASES {
        check_standalone_case(&RubyBackend, case, run_ruby_capture);
    }
}

#[test]
fn standalone_cases_bash() {
    let bash = find_bash5().expect("bash >= 5 not found — see docs/testing.md");
    for case in STANDALONE_CASES {
        check_standalone_case(&BashBackend, case, |code, args| run_bash(&bash, code, args));
    }
}

struct LibraryCase {
    name: &'static str,
    wat: &'static str,
    module_name: &'static str,
    ruby_glue: &'static str,
    bash_glue: &'static str,
    /// Both sides are engineered to produce this same string — the glue
    /// captures and prints the actual bytes the wasm module wrote,
    /// rather than a language-specific diagnostic, so there is exactly
    /// one expectation per scenario instead of one per language that
    /// could quietly drift apart.
    expect: &'static str,
}

const LIBRARY_CASES: &[LibraryCase] = &[
    LibraryCase {
        name: "add",
        wat: "add.wat",
        module_name: "add",
        ruby_glue: "inst = Add.new\n\
                    print inst.invoke(\"add\", 2, 3), \"\\n\"\n\
                    print inst.invoke(\"add\", 0xffffffff, 1), \"\\n\"\n\
                    print inst.invoke(\"fib\", 10), \"\\n\"",
        // ADR-11: results come back through the global R0.
        bash_glue: "add_init || exit 1\n\
                    add_invoke add 2 3; echo $R0\n\
                    add_invoke add 4294967295 1; echo $R0\n\
                    add_invoke fib 10; echo $R0\n",
        expect: "5\n0\n55\n",
    },
    // The ADR-7 override/fallback semantics: an explicit import wins,
    // an unhandled one falls back to the bundled WASI. Both glues
    // intercept fd_write and print the actual bytes the module wrote
    // (rather than a fd/len diagnostic) — that's the one observable
    // both languages can produce identically, so there's a single
    // `expect` instead of a per-language one.
    LibraryCase {
        name: "wasi_import_override",
        wat: "wasi_imports.wat",
        module_name: "prog",
        ruby_glue: RUBY_OVERRIDE_GLUE,
        bash_glue: BASH_OVERRIDE_GLUE,
        expect: "ok\n",
    },
];

const RUBY_OVERRIDE_GLUE: &str = r#"captured = +""
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
    let code = convert(
        &RubyBackend,
        &examples_dir().join(case.wat),
        Mode::Library,
        case.module_name,
    );
    let out = run_ruby(&format!("{code}\n{}", case.ruby_glue), &[]);
    assert_eq!(out, case.expect, "{}: ruby stdout", case.name);
}

fn check_library_case_bash(bash: &Path, case: &LibraryCase) {
    let code = convert(
        &BashBackend,
        &examples_dir().join(case.wat),
        Mode::Library,
        case.module_name,
    );
    let output = run_bash(bash, &format!("{code}\n{}", case.bash_glue), &[]);
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
    find_ruby().expect("ruby not found on PATH — see docs/testing.md");
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

/// A provider *object* replaces the bundled WASI wholesale: import(name)
/// resolves functions, attach(instance) binds the memory. No Bash
/// counterpart — Bash's import table is a plain associative array
/// (ADR-12), it has no duck-typed object model to replace WASI with, so
/// this scenario (unlike `wasi_import_override` above) only exists on
/// the Ruby side.
#[test]
fn custom_wasi_provider() {
    find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let script = format!(
        "{}\n{}",
        convert(
            &RubyBackend,
            &examples_dir().join("wasi_imports.wat"),
            Mode::Library,
            "prog",
        ),
        r#"
class MyWasi
  attr_reader :out
  def import(name)
    case name
    when "fd_write" then method(:fd_write)
    when "random_get" then ->(_buf, _len) { 0 }
    end
  end
  def attach(instance) = @memory = instance.memory
  def fd_write(_fd, iovs, _iovs_len, out_ptr)
    ptr = @memory.bytes.unpack1("L<", offset: iovs)
    len = @memory.bytes.unpack1("L<", offset: iovs + 4)
    (@out ||= +"") << @memory.bytes.byteslice(ptr, len)
    @memory.bytes[out_ptr, 4] = [len].pack("L<")
    0
  end
end

wasi = MyWasi.new
inst = Prog.new({ "wasi_snapshot_preview1" => wasi })
inst.invoke("_start")
print wasi.out
print "bundled wasi constructed: ", !inst.instance_variable_get(:@wasi).nil?, "\n"
"#
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "ok\nbundled wasi constructed: false\n");
}

/// The `true` counterpart to `custom_wasi_provider`'s `false`: a partial
/// Hash override still lets the bundled WASI get constructed on demand
/// for the one import it doesn't cover (ADR-7's `@wasi ||= ...`).
/// Reuses `RUBY_OVERRIDE_GLUE` (the same glue `wasi_import_override`
/// runs) plus one extra line probing `@wasi` — an internal, Ruby-only
/// detail the shared scenario doesn't need to assert on to prove the
/// override/fallback *behavior* itself, so it stays a separate test
/// rather than a second `expect` on the shared one. No Bash
/// counterpart, for the same reason `custom_wasi_provider` has none.
#[test]
fn partial_override_falls_back_to_bundled_wasi() {
    find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let code = convert(
        &RubyBackend,
        &examples_dir().join("wasi_imports.wat"),
        Mode::Library,
        "prog",
    );
    let script = format!(
        "{code}\n{RUBY_OVERRIDE_GLUE}print \"bundled wasi constructed: \", !inst.instance_variable_get(:@wasi).nil?, \"\\n\"\n"
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "ok\nbundled wasi constructed: true\n");
}

/// Two Embedded-linkage artifacts must coexist in one process: each
/// carries its own nested `Rt`, so runtime classes (and even different
/// runtime versions, eventually) never collide. No Bash counterpart yet
/// — Bash's Embedded linkage namespaces by prefix rather than by nested
/// class, and proving the analogous non-collision guarantee there is a
/// separate test to write, not a mechanical translation of this one.
#[test]
fn embedded_runtimes_coexist() {
    find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let wat_path = examples_dir().join("div_trap.wat");
    let script = format!(
        "{}\n{}\n{}",
        convert(&RubyBackend, &wat_path, Mode::Library, "alpha"),
        convert(&RubyBackend, &wat_path, Mode::Library, "beta"),
        r#"
a = Alpha.new
b = Beta.new
print a.invoke("div", 7, 2), "\n"
print b.invoke("div", 0xfffffff9, 2), "\n"
print (Alpha::Rt::Trap != Beta::Rt::Trap), "\n"
begin
  a.invoke("div", 1, 0)
rescue Alpha::Rt::Trap => e
  print "trap: ", e.message, "\n"
end
"#
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "3\n4294967293\ntrue\ntrap: integer divide by zero\n");
}

/// A fresh, empty directory for one WASI-fs test to preopen, so tests
/// running in parallel never share state.
fn wasi_fs_scratch_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dewasmify-e2e-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn preopen_kwarg(guest: &str, host: &std::path::Path) -> String {
    format!(
        "preopens: {{ {:?} => {:?} }}",
        guest,
        host.to_string_lossy()
    )
}

// The three wasi_fs_* tests below are Ruby-only: Bash's WASI surface
// has no filesystem syscalls yet (ADR-12), so there is no Bash
// scenario to pair these with.

/// path_open (create+write, then reopen+read) round-trips real file
/// content through a preopened host directory.
#[test]
fn wasi_fs_path_open_roundtrip() {
    find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let dir = wasi_fs_scratch_dir("wasi-fs-roundtrip");
    let script = format!(
        "{}\ninst = Prog.new({{}}, {})\ninst.invoke(\"_start\")\n",
        convert(
            &RubyBackend,
            &examples_dir().join("wasi_path_open_roundtrip.wat"),
            Mode::Library,
            "prog",
        ),
        preopen_kwarg("/", &dir)
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "hello, wasi fs!");
    assert_eq!(
        std::fs::read_to_string(dir.join("hello.txt")).unwrap(),
        "hello, wasi fs!"
    );
}

/// path_create_directory + fd_readdir + path_unlink_file/path_remove_directory
/// against a real host directory, verified from both sides: the dirent
/// listing seen by the guest, and the host filesystem after cleanup.
#[test]
fn wasi_fs_mkdir_readdir_unlink() {
    find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let dir = wasi_fs_scratch_dir("wasi-fs-mkdir-readdir-unlink");
    let script = format!(
        "{}\ninst = Prog.new({{}}, {})\ninst.invoke(\"_start\")\n",
        convert(
            &RubyBackend,
            &examples_dir().join("wasi_mkdir_readdir_unlink.wat"),
            Mode::Library,
            "prog",
        ),
        preopen_kwarg("/", &dir)
    );
    let out = run_ruby(&script, &[]);
    assert!(
        out.contains("sub"),
        "expected \"sub\" in dirent listing: {out:?}"
    );
    assert!(!dir.join("sub").exists(), "sub/ should have been removed");
}

/// A `..`-escaping guest path must be rejected (ERRNO_NOTCAPABLE, not a
/// host filesystem escape): a canary file sitting just outside the
/// preopened directory must stay unreadable and untouched.
#[test]
fn wasi_fs_escape_rejected() {
    find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let root = wasi_fs_scratch_dir("wasi-fs-escape");
    let sandbox = root.join("sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    let canary_dir = root.join("escape-canary");
    std::fs::create_dir_all(&canary_dir).unwrap();
    std::fs::write(canary_dir.join("canary.txt"), "secret").unwrap();

    let script = format!(
        "{}\ninst = Prog.new({{}}, {})\ninst.invoke(\"_start\")\n",
        convert(
            &RubyBackend,
            &examples_dir().join("wasi_escape_rejected.wat"),
            Mode::Library,
            "prog",
        ),
        preopen_kwarg("/", &sandbox)
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "BLOCKED\n");
    assert_eq!(
        std::fs::read_to_string(canary_dir.join("canary.txt")).unwrap(),
        "secret"
    );
}
