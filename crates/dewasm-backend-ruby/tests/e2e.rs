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
    apps_cache_dir, apps_e2e, convert, convert_on_big_stack, examples_dir, fresh_scratch_dir,
    fs_apps_e2e, gzip_e2e, library_e2e, run_script, standalone_e2e, wasi_suite, BackendUnderTest,
    LibraryCase, WasiCase,
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

    /// Instantiate `class` with kwargs (args/env/preopens), run `_start`, and
    /// swallow a clean guest `proc_exit` (`{class}::Rt::Exit`). Generalizes the
    /// hand-written glue the mirrored fs app tests used.
    fn app_glue(
        &self,
        class: &str,
        args: &[&str],
        env: &[(&str, &str)],
        preopens: &[(&str, &Path)],
    ) -> String {
        let argv = args
            .iter()
            .map(|a| format!("{a:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let envs = env
            .iter()
            .map(|(k, v)| format!("{k:?} => {v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let pres = preopens
            .iter()
            .map(|(guest, host)| format!("{guest:?} => {:?}", host.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "inst = {class}.new({{}}, args: [{argv}], env: {{{envs}}}, preopens: {{{pres}}})\n\
             begin\n  inst.invoke(\"_start\")\nrescue {class}::Rt::Exit\nend\n"
        )
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
gzip_e2e!(Ruby);
fs_apps_e2e!(Ruby);

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

// ---------------------------------------------------------------------
// Phase 5a app deepening: Ruby-only filesystem library scenarios. The
// guest-visible fs app cases (qjs file I/O + REPL, sqlite3 shell dbfile,
// ripgrep) now live in the shared `FS_APP_CASES` table (`fs_apps_e2e!` above),
// run by every fs-capable backend and ground-truthed against wasmtime. What
// remains here are the Ruby-only C-API drives whose results live in guest
// memory (no wasmtime golden possible) and the heavy language-runtime binaries.
// Every fs flow runs on the syscall set ADR-14 already ships — no new WASI unit.

/// Convert a cached app binary to a library-mode Ruby class (roomy stack for
/// SQLite's deep functions, `convert_on_big_stack`).
fn convert_app_library(wasm: &str, class: &str) -> String {
    let path = apps_cache_dir().join(format!("{wasm}.wasm"));
    assert!(
        path.exists(),
        "{wasm} not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&path).expect("read wasm");
    convert_on_big_stack(&RubyBackend, &bytes, Mode::Library, class)
}

/// Run `script` under `ruby` with `stdin`, returning the raw `Output`.
fn run_ruby_io(script: &str, stdin: &str) -> std::process::Output {
    let ruby = find_ruby().expect("ruby not found on PATH — see docs/testing.md");
    run_script(&ruby, script, "rb", &[], stdin)
}

/// The library-shape counterpart to the shared `sqlite3_shell_dbfile` fs app
/// case (Phase 5a #2b):
/// the sqlite3 C API opening a *file* under a preopen — open, insert, close,
/// reopen, select — proving the C-API path hits the same ADR-14 fs stack as the
/// shell. No wasmtime golden (the CLI cannot drive a C-API flow whose results
/// live in guest memory); the expectation is a fixed string pinned by the
/// amalgamation version in examples/apps/fetch.sh.
#[test]
fn sqlite3_file_c_api_ruby() {
    let db = fresh_scratch_dir("ruby-libsqlite3-file");
    let class = convert_app_library("libsqlite3", "Libsqlite3");
    let glue = format!(
        "{}\n{}",
        format_args!(
            "DB_MOD = Libsqlite3.new({{}}, preopens: {{ \"/db\" => {:?} }})",
            db.to_string_lossy()
        ),
        LIBSQLITE3_FILE_GLUE
    );
    let output = run_ruby_io(&format!("{class}\n{glue}"), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "10|x\n20|y\nFILE-OK\n",
        "libsqlite3 file C API drive under ruby: output differs\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0));
    assert!(
        db.join("data.db")
            .metadata()
            .map(|m| m.len() > 0)
            .unwrap_or(false),
        "libsqlite3 file: no nonzero DB file left on the host"
    );
}

/// Ruby glue driving the sqlite3 C API against a file preopen: create+insert,
/// close, then reopen the same file and select — the file lifecycle the shell
/// case exercises, but through the C API.
const LIBSQLITE3_FILE_GLUE: &str = r##"
DB_MOD.invoke("_initialize")
mem = DB_MOD.memory

def read_cstr(mem, ptr)
  return nil if ptr.zero?
  fin = mem.bytes.index("\0".b, ptr)
  mem.read_string(ptr, fin - ptr)
end

def cstr(mem, s)
  p = DB_MOD.invoke("sqlite3_malloc", s.bytesize + 1)
  mem.init(p, "#{s}\0", 0, s.bytesize + 1)
  p
end

def open_db(mem, path)
  pp = DB_MOD.invoke("sqlite3_malloc", 4)
  rc = DB_MOD.invoke("sqlite3_open", cstr(mem, path), pp)
  raise "open rc=#{rc}" unless rc.zero?
  mem.i32_load(pp)
end

# create + insert, then close so the file is fully flushed
db = open_db(mem, "/db/data.db")
rc = DB_MOD.invoke("sqlite3_exec", db, cstr(mem, "create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0)
raise "exec rc=#{rc}: #{read_cstr(mem, DB_MOD.invoke('sqlite3_errmsg', db))}" unless rc.zero?
DB_MOD.invoke("sqlite3_close", db)

# reopen the same file and read it back
db = open_db(mem, "/db/data.db")
pp_stmt = DB_MOD.invoke("sqlite3_malloc", 4)
rc = DB_MOD.invoke("sqlite3_prepare_v2", db, cstr(mem, "select a*10, b from t order by a"), 0xffffffff, pp_stmt, 0)
raise "prepare rc=#{rc}" unless rc.zero?
stmt = mem.i32_load(pp_stmt)
while DB_MOD.invoke("sqlite3_step", stmt) == 100 # SQLITE_ROW
  row = (0...DB_MOD.invoke("sqlite3_column_count", stmt)).map do |i|
    read_cstr(mem, DB_MOD.invoke("sqlite3_column_text", stmt, i))
  end
  puts row.join("|")
end
DB_MOD.invoke("sqlite3_finalize", stmt)
DB_MOD.invoke("sqlite3_close", db)
puts "FILE-OK"
"##;

/// Guest->host callback round trip in library mode (Phase 5a #2c): our own
/// committed C (examples/apps/src/sqlite3_binding.c, built into
/// sqlite3-binding.wasm by fetch.sh) exports `run_query`, which calls
/// `sqlite3_exec` with a C callback forwarding each row to the *imported*
/// `env.host_row`. The Ruby side provides `host_row` via the ADR-7
/// import-provider mechanism and collects the rows. No wasmtime golden (a pure
/// in-memory C-API flow); the expectation is a fixed string.
#[test]
fn sqlite3_callback_binding_ruby() {
    let class = convert_app_library("sqlite3-binding", "Sqlite3Binding");
    let output = run_ruby_io(&format!("{class}\n{SQLITE3_BINDING_GLUE}"), "");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "row: 2|y\nrow: 3|z\nCALLBACK-OK\n",
        "sqlite3 callback binding drive under ruby: output differs\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.status.code(), Some(0));
}

/// Ruby glue for the callback-binding artifact: provide `env.host_row` (ADR-7),
/// set up an in-memory table via the C API, then call the guest `run_query`,
/// which drives `host_row` once per result row. `host_row` reads each row's
/// column-text pointers straight out of guest memory and collects them.
const SQLITE3_BINDING_GLUE: &str = r##"
ROWS = []
MEM_HOLDER = {}
host_row = lambda do |argc, argv_ptr|
  mem = MEM_HOLDER[:mem]
  row = (0...argc).map do |i|
    p = mem.i32_load(argv_ptr + i * 4)
    next nil if p.zero?
    fin = mem.bytes.index("\0".b, p)
    mem.read_string(p, fin - p)
  end
  ROWS << row
end

db_mod = Sqlite3Binding.new({ "env" => { "host_row" => host_row } })
db_mod.invoke("_initialize")
mem = db_mod.memory
MEM_HOLDER[:mem] = mem

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

pp_db = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_open", cstr(db_mod, mem, ":memory:"), pp_db)
raise "open rc=#{rc}" unless rc.zero?
db = mem.i32_load(pp_db)

rc = db_mod.invoke("sqlite3_exec", db,
                   cstr(db_mod, mem, "create table t(a,b); insert into t values (1,'x'),(2,'y'),(3,'z');"),
                   0, 0, 0)
raise "exec rc=#{rc}: #{read_cstr(mem, db_mod.invoke('sqlite3_errmsg', db))}" unless rc.zero?

# guest -> host: run_query calls back into env.host_row once per row
rc = db_mod.invoke("run_query", db, cstr(db_mod, mem, "select a, b from t where a >= 2 order by a"))
raise "run_query rc=#{rc}" unless rc.zero?
db_mod.invoke("sqlite3_close", db)

ROWS.each { |r| puts "row: #{r.join('|')}" }
puts "CALLBACK-OK"
"##;

// ---------------------------------------------------------------------
// Phase 5b: language-runtime binaries — CPython and CRuby — converted and
// *executed* on the Ruby backend, each reading its stdlib tree from a
// preopened directory (Ruby-only, ADR-14). These are the heaviest cases in
// the suite: converting the 30–35 MB wasm yields a ~200–335 MB Ruby source
// file, and running it means Ruby parsing that file and then interpreting a
// whole language runtime. Both complete well under the ADR-24 5-minute bar
// (measured in docs/apps-audit.md: CPython ~1.4 s convert + ~12 s run; CRuby
// ~2.8 s convert + ~60 s run) and neither needs a WASI unit beyond the ADR-14
// set (their fd_renumber/poll_oneoff/path_readlink/sock_* imports are never
// called on the boot path), so both genuinely execute rather than being
// conversion-only smokes.
//
// They are gated behind DEWASM_APPS_ALL — the same deliberate perf opt-out the
// `apps` heavy cases use (ADR-15's documented scope: a perf gate, not a
// missing-environment skip) — because a ~335 MB temp file and ~1 GB of RSS on
// every `cargo test` is too costly for the default gate. The task's own gate
// list runs them via `DEWASM_APPS_ALL=1 cargo test -p dewasm-backend-ruby
// --test e2e`.

/// Whether the heavy language-runtime cases should actually run. Mirrors the
/// `run_app_cases` heavy-skip contract: skip-with-a-note, never fail, when the
/// opt-in env var is unset (ADR-15's perf-gate scope).
fn apps_all() -> bool {
    std::env::var("DEWASM_APPS_ALL").is_ok()
}

/// CPython 3.14.6 executing a one-liner on the Ruby backend (Phase 5b),
/// reading its stdlib from the preopened `cache/cpython-lib/lib` tree.
/// Ground truth (wasmtime):
///   wasmtime --dir cache/cpython-lib/lib::/lib --env PYTHONHOME=/ \
///     --env PYTHONPATH=/lib/python3.14 cache/cpython.wasm \
///     -c 'print("hello from cpython", 6 * 7)'
#[test]
fn cpython_hello_ruby() {
    if !apps_all() {
        println!("cpython_hello: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let lib = apps_cache_dir().join("cpython-lib").join("lib");
    assert!(
        lib.join("python3.14").is_dir(),
        "cpython stdlib not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let class = convert_app_library("cpython", "Cpython");
    let glue = format!(
        "inst = Cpython.new({{}}, args: [\"python\", \"-c\", \"print('hello from cpython', 6 * 7)\"], \
         env: {{ \"PYTHONHOME\" => \"/\", \"PYTHONPATH\" => \"/lib/python3.14\" }}, \
         preopens: {{ \"/lib\" => {:?} }})\n\
         begin\n  inst.invoke(\"_start\")\nrescue Cpython::Rt::Exit\nend\n",
        lib.to_string_lossy()
    );
    let output = run_ruby_io(&format!("{class}\n{glue}"), "");
    assert!(
        output.status.success(),
        "cpython_hello under ruby failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "hello from cpython 42\n",
        "cpython_hello: guest stdout differs from the wasmtime ground truth"
    );
}

/// CRuby 3.4 executing a one-liner on the Ruby backend (Phase 5b) — the
/// "Ruby on Ruby" north-star demo — reading its stdlib from the preopened
/// `cache/ruby-lib/usr` tree. Ground truth (wasmtime):
///   wasmtime --dir cache/ruby-lib/usr::/usr cache/ruby.wasm \
///     -e 'puts "hello from cruby #{6*7}"'
#[test]
fn cruby_hello_ruby() {
    if !apps_all() {
        println!("cruby_hello: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let usr = apps_cache_dir().join("ruby-lib").join("usr");
    assert!(
        usr.join("local").join("lib").join("ruby").is_dir(),
        "cruby stdlib not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let class = convert_app_library("ruby", "Cruby");
    let glue = format!(
        "inst = Cruby.new({{}}, args: [\"ruby\", \"-e\", \"puts \\\"hello from cruby #{{6*7}}\\\"\"], \
         env: {{}}, preopens: {{ \"/usr\" => {:?} }})\n\
         begin\n  inst.invoke(\"_start\")\nrescue Cruby::Rt::Exit\nend\n",
        usr.to_string_lossy()
    );
    let output = run_ruby_io(&format!("{class}\n{glue}"), "");
    assert!(
        output.status.success(),
        "cruby_hello under ruby failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "hello from cruby 42\n",
        "cruby_hello: guest stdout differs from the wasmtime ground truth"
    );
}
