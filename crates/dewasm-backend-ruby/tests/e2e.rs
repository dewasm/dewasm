//! Ruby end-to-end suites (ADR-27): the shared standalone / library / WASI /
//! apps case tables (`dewasm-test-helper`) wired up for the Ruby backend,
//! plus the Ruby-only scenarios that have no counterpart in another backend
//! yet (the ADR-7 object-provider model, embedded runtime coexistence,
//! cross-module table sharing, WASI-filesystem internals, and the sqlite3 C
//! API drive).
//!
//! Two kinds of "Ruby-only" mix in the tail of this file. `custom_wasi_provider`,
//! `partial_override_falls_back_to_bundled_wasi`, `wasi_stdio_accepts_stringio`,
//! `wasi_fs_root_preopen_containment`, and `embedded_runtimes_coexist` are Ruby
//! language-model capabilities (duck-typed provider objects, StringIO stdio,
//! probing internal Ruby classes) with no wasm feature behind them.
//! `shared_table_call_indirect_across_modules` is a genuine capability gap Bash
//! would pick up if it implemented imported tables. The fixture-driven WASI
//! filesystem cases themselves live in the shared `WASI_CASES` table.

use std::path::{Path, PathBuf};

use dewasm_backend::{Backend, Mode, RuntimeLinkage};
use dewasm_backend_ruby::{find_ruby, RubyBackend};
use dewasm_test_helper::{
    apps_e2e, convert, examples_dir, library_e2e, run_script, standalone_e2e, wasi_suite,
    BackendUnderTest, LibraryCase, WasiCase,
};

pub struct Ruby;

impl BackendUnderTest for Ruby {
    fn name(&self) -> &'static str {
        "ruby"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &RubyBackend
    }

    fn interpreter(&self) -> PathBuf {
        find_ruby().expect("ruby not found on PATH — see docs/testing.md")
    }
}

/// Per-case Ruby glue. A case Ruby is wired to run but has no glue for panics
/// loudly (ADR-15).
fn ruby_glue(case: &LibraryCase) -> &'static str {
    match case.name {
        "add" => {
            "inst = Add.new\n\
                  print inst.invoke(\"add\", 2, 3), \"\\n\"\n\
                  print inst.invoke(\"add\", 0xffffffff, 1), \"\\n\"\n\
                  print inst.invoke(\"fib\", 10), \"\\n\""
        }
        "wasi_import_override" => RUBY_OVERRIDE_GLUE,
        other => panic!("{other}: no ruby glue"),
    }
}

/// Instantiate a fs fixture with the scratch dir preopened at guest `/`, run
/// `_start`, and surface a `proc_exit` code as a trailing decimal line. One
/// wrapper serves both stdout-reporting and proc_exit-reporting fixtures: the
/// former never raises `Rt::Exit`, so nothing extra is printed.
fn ruby_fs_glue(_case: &WasiCase, host: &Path) -> String {
    format!(
        "inst = Prog.new({{}}, preopens: {{ {:?} => {:?} }})\n\
         begin\n  inst.invoke(\"_start\")\nrescue Prog::Rt::Exit => e\n  print e.code, \"\\n\"\nend\n",
        "/",
        host.to_string_lossy()
    )
}

standalone_e2e!(Ruby);
library_e2e!(Ruby, ruby_glue);
wasi_suite!(Ruby, Stdio);
wasi_suite!(Ruby, ArgsEnv);
wasi_suite!(Ruby, Fs, ruby_fs_glue);
apps_e2e!(Ruby);

// ---------------------------------------------------------------------
// Ruby-only scenarios.

/// Run `script` under `ruby`, asserting success, and return stdout.
fn run_ruby(script: &str) -> String {
    let ruby = find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let output = run_script(&ruby, script, "rb", &[], "");
    assert!(
        output.status.success(),
        "ruby failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The ADR-7 override/fallback glue (an explicit `fd_write` import wins,
/// `random_get` falls back to the bundled WASI), shared by the
/// `wasi_import_override` library case and `partial_override_falls_back_to_bundled_wasi`.
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

/// A provider *object* replaces the bundled WASI wholesale: import(name)
/// resolves functions, attach(instance) binds the memory. Bash's import table
/// is a plain associative array (ADR-12); it has no duck-typed object model to
/// replace WASI with, so this scenario (unlike `wasi_import_override`) only
/// exists on the Ruby side.
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
    let out = run_ruby(&script);
    assert_eq!(out, "ok\nbundled wasi constructed: false\n");
}

/// The `true` counterpart to `custom_wasi_provider`'s `false`: a partial Hash
/// override still lets the bundled WASI get constructed on demand for the one
/// import it doesn't cover (ADR-7's `@wasi ||= ...`). Reuses
/// `RUBY_OVERRIDE_GLUE` plus one extra line probing `@wasi`.
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
    let out = run_ruby(&script);
    assert_eq!(out, "ok\nbundled wasi constructed: true\n");
}

/// Two Embedded-linkage artifacts must coexist in one process: each carries
/// its own nested `Rt`, so runtime classes (and even different runtime
/// versions, eventually) never collide.
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
    let out = run_ruby(&script);
    assert_eq!(out, "3\n4294967293\ntrue\ntrap: integer divide by zero\n");
}

/// A table shared across two modules whose type sections order the same
/// structural type differently: the call_indirect check must compare types
/// structurally (ADR-4), never via a module-local id. Ruby-only — Bash rejects
/// imported tables at conversion time. Cross-module linking runs on one shared
/// runtime (`Alias` linkage, as the spec harness's `register` path does).
#[test]
fn shared_table_call_indirect_across_modules() {
    let mut units = std::collections::BTreeSet::new();
    let mut modules = String::new();
    for (wat, name) in [
        ("shared_table_a.wat", "TableExp"),
        ("shared_table_b.wat", "TableImp"),
    ] {
        let bytes = wat::parse_file(examples_dir().join(wat)).expect("parse wat");
        let module = dewasm_core::build_module(&bytes).expect("build IR");
        let (source, u) = dewasm_backend_ruby::generate_class_with_units(
            &module,
            name,
            &RuntimeLinkage::Alias("::Rt".to_string()),
            false,
        )
        .expect("generate");
        units.extend(u);
        modules.push_str(&source);
    }
    let script = format!(
        "{}\n{}\n{}",
        dewasm_backend_ruby::shared_runtime(&units).expect("bundle runtime"),
        modules,
        r#"
a = TableExp.new
b = TableImp.new({ "a" => a })
print b.invoke("call0"), "\n"
"#
    );
    let out = run_ruby(&script);
    assert_eq!(out, "42\n");
}

// ---------------------------------------------------------------------
// WASI filesystem (ADR-14) scenarios with no fixture-table counterpart: they
// exercise Ruby-model surfaces (StringIO stdio, internal `Rt::WASI` methods)
// rather than a fixture's guest-visible behavior, so they stay here rather
// than in the shared `WASI_CASES` table.

/// Stdio fds accept duck-typed IO objects: an embedder redirecting `$stdout`
/// to a StringIO (the standard Ruby capture idiom) before instantiation must
/// still receive the module's output, not ERRNO_BADF.
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
    let out = run_ruby(&script);
    assert_eq!(out, "Hello, WASI!\n");
}

/// A preopen whose realpath is the filesystem root must not reject every path:
/// `within?` would otherwise build the prefix "//". Probed via resolve_path
/// directly — no host files are touched — so it uses the internal `Rt::WASI`
/// class rather than a guest fixture.
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
    let out = run_ruby(&script);
    assert_eq!(out, "nil\n");
}

/// The library half of the sqlite3 build (ADR-22): the C API driven from Ruby.
/// No golden-vs-wasmtime here — the wasmtime CLI cannot drive this flow
/// (results live in guest memory) — so the expectation is a fixed string;
/// every value in it is pinned by the amalgamation version in
/// examples/apps/fetch.sh.
#[test]
fn libsqlite3_c_api_ruby() {
    let ruby = find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    let wasm_path = dewasm_test_helper::apps_cache_dir().join("libsqlite3.wasm");
    assert!(
        wasm_path.exists(),
        "libsqlite3 not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let class =
        dewasm_test_helper::convert_on_big_stack(&RubyBackend, &bytes, Mode::Library, "libsqlite3");
    let output = run_script(&ruby, &format!("{class}\n{LIBSQLITE3_GLUE}"), "rb", &[], "");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "version: 3.53.3\n20|y\n10|x\nC-API-OK\n",
        "libsqlite3 C API drive under ruby: output differs\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0));
}

/// Ruby glue driving the sqlite3 C API of the reactor library build:
/// `_initialize`, guest-memory pointer plumbing via `sqlite3_malloc` +
/// `Rt::Memory`, and the open/exec/prepare/step/column/finalize/close flow —
/// the shape a sqlite3-gem-compatible shim will use.
const LIBSQLITE3_GLUE: &str = r##"
db_mod = Libsqlite3.new
db_mod.invoke("_initialize")
mem = db_mod.memory

def read_cstr(mem, ptr)
  return nil if ptr.zero?
  fin = mem.bytes.index("\0".b, ptr)
  mem.read_string(ptr, fin - ptr)
end

def cstr(db_mod, mem, s)
  p = db_mod.invoke("sqlite3_malloc", s.bytesize + 1)
  mem.init(p, "#{s}\0", 0, s.bytesize + 1)
  p
end

puts "version: #{read_cstr(mem, db_mod.invoke('sqlite3_libversion'))}"

pp_db = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_open", cstr(db_mod, mem, ":memory:"), pp_db)
raise "open rc=#{rc}" unless rc.zero?
db = mem.i32_load(pp_db)

sql = "create table t(a,b); insert into t values (1,'x'),(2,'y');"
rc = db_mod.invoke("sqlite3_exec", db, cstr(db_mod, mem, sql), 0, 0, 0)
raise "exec rc=#{rc}: #{read_cstr(mem, db_mod.invoke('sqlite3_errmsg', db))}" unless rc.zero?

pp_stmt = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_prepare_v2", db,
                   cstr(db_mod, mem, "select a*10, b from t order by a desc"),
                   0xffffffff, pp_stmt, 0) # -1 as masked-unsigned i32
raise "prepare rc=#{rc}" unless rc.zero?
stmt = mem.i32_load(pp_stmt)

while db_mod.invoke("sqlite3_step", stmt) == 100 # SQLITE_ROW
  row = (0...db_mod.invoke("sqlite3_column_count", stmt)).map do |i|
    read_cstr(mem, db_mod.invoke("sqlite3_column_text", stmt, i))
  end
  puts row.join("|")
end
db_mod.invoke("sqlite3_finalize", stmt)
db_mod.invoke("sqlite3_close", db)
puts "C-API-OK"
"##;
