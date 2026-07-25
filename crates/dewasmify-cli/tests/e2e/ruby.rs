//! Ruby-only e2e scenarios: the ADR-7 object-provider model, embedded
//! runtime coexistence, cross-module table sharing, and the WASI
//! filesystem (ADR-14) — none of which Bash has a counterpart for yet
//! (no duck-typed provider objects, no fs syscalls; ADR-11/ADR-12).
//! Each test notes why it is Ruby-only where the reason is specific.
//!
//! Two kinds of "Ruby-only" mix here (ADR-23). `shared_table_call_indirect_across_modules`
//! (imported tables) and the WASI-filesystem block are genuine Tier 2
//! gaps — Bash would pick these up automatically if it ever reached
//! Tier 2. `custom_wasi_provider`, `partial_override_falls_back_to_bundled_wasi`,
//! and `embedded_runtimes_coexist` are Ruby language-model capabilities
//! (duck-typed provider objects, nested runtime classes) with no wasm
//! feature behind them at all — they'd stay Ruby-only regardless of
//! Bash's tier.

use std::path::{Path, PathBuf};

use dewasmify_backend::Mode;
use dewasmify_backend_ruby::RubyBackend;

use crate::library::RUBY_OVERRIDE_GLUE;
use crate::support::{convert, examples_dir, run_ruby};

/// A provider *object* replaces the bundled WASI wholesale: import(name)
/// resolves functions, attach(instance) binds the memory. Bash's import
/// table is a plain associative array (ADR-12); it has no duck-typed
/// object model to replace WASI with, so this scenario (unlike
/// `library::wasi_import_override`) only exists on the Ruby side.
#[test]
fn custom_wasi_provider() {
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
/// rather than a second `expect` on the shared one.
#[test]
fn partial_override_falls_back_to_bundled_wasi() {
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

/// A table shared across two modules whose type sections order the same
/// structural type differently: the call_indirect check must compare
/// types structurally (ADR-4), never via a module-local id. Ruby-only —
/// Bash rejects imported tables at conversion time. Cross-module linking
/// runs on one shared runtime (`Alias` linkage, as the spec harness's
/// `register` path does): two Embedded artifacts have distinct `Rt`
/// class trees and cannot exchange runtime objects.
#[test]
fn shared_table_call_indirect_across_modules() {
    let mut units = std::collections::BTreeSet::new();
    let mut modules = String::new();
    for (wat, name) in [
        ("shared_table_a.wat", "TableExp"),
        ("shared_table_b.wat", "TableImp"),
    ] {
        let bytes = wat::parse_file(examples_dir().join(wat)).expect("parse wat");
        let module = dewasmify_core::build_module(&bytes).expect("build IR");
        let (source, u) = dewasmify_backend_ruby::generate_class_with_units(
            &module,
            name,
            &dewasmify_backend::RuntimeLinkage::Alias("::Rt".to_string()),
            false,
        )
        .expect("generate");
        units.extend(u);
        modules.push_str(&source);
    }
    let script = format!(
        "{}\n{}\n{}",
        dewasmify_backend_ruby::shared_runtime(&units).expect("bundle runtime"),
        modules,
        r#"
a = TableExp.new
b = TableImp.new({ "a" => a })
print b.invoke("call0"), "\n"
"#
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "42\n");
}

// ---------------------------------------------------------------------
// WASI filesystem (ADR-14). Bash's WASI surface has no filesystem
// syscalls yet (ADR-12), so there is no Bash scenario to pair these
// with.

/// A fresh, empty directory for one WASI-fs test to preopen, so tests
/// running in parallel never share state.
fn wasi_fs_scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dewasmify-e2e-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn preopen_kwarg(guest: &str, host: &Path) -> String {
    format!(
        "preopens: {{ {:?} => {:?} }}",
        guest,
        host.to_string_lossy()
    )
}

/// Convert `wat` (library mode), instantiate it with `host` preopened at
/// guest `/`, run `_start`, and return stdout.
fn run_wasi_fs(wat: &str, host: &Path) -> String {
    let script = format!(
        "{}\ninst = Prog.new({{}}, {})\ninst.invoke(\"_start\")\n",
        convert(
            &RubyBackend,
            &examples_dir().join(wat),
            Mode::Library,
            "prog",
        ),
        preopen_kwarg("/", host)
    );
    run_ruby(&script, &[])
}

/// Like `run_wasi_fs`, but for fixtures that report their result via
/// `proc_exit`: returns the `Rt::Exit` code as a decimal line.
fn run_wasi_fs_exit_code(wat: &str, host: &Path) -> String {
    let script = format!(
        "{}\ninst = Prog.new({{}}, {})\nbegin\n  inst.invoke(\"_start\")\nrescue Prog::Rt::Exit => e\n  print e.code, \"\\n\"\nend\n",
        convert(
            &RubyBackend,
            &examples_dir().join(wat),
            Mode::Library,
            "prog",
        ),
        preopen_kwarg("/", host)
    );
    run_ruby(&script, &[])
}

/// path_open (create+write, then reopen+read) round-trips real file
/// content through a preopened host directory.
#[test]
fn wasi_fs_path_open_roundtrip() {
    let dir = wasi_fs_scratch_dir("wasi-fs-roundtrip");
    let out = run_wasi_fs("wasi_path_open_roundtrip.wat", &dir);
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
    let dir = wasi_fs_scratch_dir("wasi-fs-mkdir-readdir-unlink");
    let out = run_wasi_fs("wasi_mkdir_readdir_unlink.wat", &dir);
    assert!(
        out.contains("sub"),
        "expected \"sub\" in dirent listing: {out:?}"
    );
    assert!(!dir.join("sub").exists(), "sub/ should have been removed");
}

/// Stdio fds accept duck-typed IO objects: an embedder redirecting
/// `$stdout` to a StringIO (the standard Ruby capture idiom) before
/// instantiation must still receive the module's output, not ERRNO_BADF.
#[test]
fn wasi_stdio_accepts_stringio() {
    let script = format!(
        "{}\n{}",
        convert(
            &RubyBackend,
            &examples_dir().join("hello.wat"),
            Mode::Library,
            "prog",
        ),
        r#"
require "stringio"
captured = StringIO.new
orig = $stdout
$stdout = captured
begin
  inst = Prog.new({})
  inst.invoke("_start")
rescue Prog::Rt::Exit
ensure
  $stdout = orig
end
print captured.string
"#
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "Hello, WASI!\n");
}

/// path_open with oflags::DIRECTORY on a missing path is ENOENT (44),
/// not ENOTDIR (54) — guests (e.g. wasi-libc's opendir) branch on the
/// difference. The fixture exits with the errno.
#[test]
fn wasi_fs_dir_open_missing_is_noent() {
    let dir = wasi_fs_scratch_dir("wasi-fs-dir-open-missing");
    let out = run_wasi_fs_exit_code("wasi_dir_open_missing.wat", &dir);
    assert_eq!(out, "44\n");
}

/// path_filestat_get without SYMLINK_FOLLOW stats the symlink itself
/// (filetype 7), not its target: resolution must not follow the final
/// component. The fixture exits with the reported filetype.
#[cfg(unix)]
#[test]
fn wasi_fs_filestat_nofollow_symlink() {
    let dir = wasi_fs_scratch_dir("wasi-fs-filestat-nofollow");
    std::fs::write(dir.join("file"), "x").unwrap();
    std::os::unix::fs::symlink("file", dir.join("link")).unwrap();
    let out = run_wasi_fs_exit_code("wasi_filestat_nofollow.wat", &dir);
    assert_eq!(out, "7\n");
}

/// A preopen whose realpath is the filesystem root must not reject every
/// path: `within?` would otherwise build the prefix "//". Probed via
/// resolve_path directly — no host files are touched.
#[test]
fn wasi_fs_root_preopen_containment() {
    let script = format!(
        "{}\n{}",
        convert(
            &RubyBackend,
            &examples_dir().join("wasi_path_open_roundtrip.wat"),
            Mode::Library,
            "prog",
        ),
        r#"
wasi = Prog::Rt::WASI.new(preopens: { "/" => "/" })
_path, err = wasi.send(:resolve_path, 3, "etc")
print err.inspect, "\n"
"#
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "nil\n");
}

/// A `..`-escaping guest path must be rejected (ERRNO_NOTCAPABLE, not a
/// host filesystem escape): a canary file sitting just outside the
/// preopened directory must stay unreadable and untouched.
#[test]
fn wasi_fs_escape_rejected() {
    let root = wasi_fs_scratch_dir("wasi-fs-escape");
    let sandbox = root.join("sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    let canary_dir = root.join("escape-canary");
    std::fs::create_dir_all(&canary_dir).unwrap();
    std::fs::write(canary_dir.join("canary.txt"), "secret").unwrap();

    let out = run_wasi_fs("wasi_escape_rejected.wat", &sandbox);
    assert_eq!(out, "BLOCKED\n");
    assert_eq!(
        std::fs::read_to_string(canary_dir.join("canary.txt")).unwrap(),
        "secret"
    );
}
