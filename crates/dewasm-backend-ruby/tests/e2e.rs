//! Ruby end-to-end suites (ADR-27): the shared case consts (`dewasm-test-helper`) wired up for the Ruby backend. Per the ADR-27 revision this file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations — every scenario's case content (fixtures, expectations, run logic) lives in a shared const, glue is a plain `&str` argument at the callsite, and which macros this file invokes is the capability declaration. The formerly Ruby-only scenarios (the ADR-7 provider model, embedded-runtime coexistence, cross-module table sharing, the sqlite3 C-API drive, WASI-filesystem internals, and the CPython/CRuby runtime demos) now run wherever a backend's declared capabilities cover them, with a REASON comment at any non-invocation.

use std::path::PathBuf;

use dewasm_backend::{Backend, Mode, RuntimeLinkage};
use dewasm_backend_ruby::{find_ruby, RubyBackend};
use dewasm_test_helper::{
    convert, cowsay_args_e2e, cowsay_stdin_e2e, cpython_hello_e2e, cruby_hello_e2e,
    custom_wasi_provider_e2e, doom_frame_e2e, embedded_coexist_e2e, examples_dir,
    exiftool_extract_e2e, gzip_e2e, library_add_e2e, libsqlite3_c_api_e2e, partial_override_e2e,
    pcap_compile_e2e, qjs_eval_e2e, qjs_file_io_e2e, qjs_repl_pty_e2e, rg_search_e2e,
    shared_table_e2e, sqlite3_callback_binding_e2e, sqlite3_file_c_api_e2e,
    sqlite3_shell_dbfile_e2e, sqlite3_shell_e2e, standalone_dir_e2e, stdio_capture_e2e,
    treesitter_parse_e2e, wasi_import_override_e2e, wasi_root_containment_e2e, wasi_suite,
    zeroperl_eval_e2e, BackendUnderTest,
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

    /// Compose several `.wat` modules for the multi-module cases. `shared_runtime` emits each module against a single top-level `::Rt` (Alias linkage) plus one bundled runtime, so an imported table crosses modules (as the spec harness's `register` path does); otherwise it emits independent Embedded classes, each with its own nested `Rt`.
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        if shared_runtime {
            let mut units = std::collections::BTreeSet::new();
            let mut classes = Vec::new();
            for (wat, name) in modules {
                let bytes = wat::parse_file(examples_dir().join(wat)).expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, u) = dewasm_backend_ruby::generate_class_with_units(
                    &module,
                    name,
                    &RuntimeLinkage::Alias("::Rt".to_string()),
                    false,
                )
                .expect("generate");
                units.extend(u);
                classes.push(src);
            }
            format!(
                "{}\n{}",
                dewasm_backend_ruby::shared_runtime(&units).expect("bundle runtime"),
                classes.join("\n")
            )
        } else {
            modules
                .iter()
                .map(|(wat, name)| {
                    convert(&RubyBackend, &examples_dir().join(wat), Mode::Library, name)
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

// --------------------------------------------------------------------- Library-case glue.

/// `add.wat`: call the exported functions and print each result.
const RUBY_ADD_GLUE: &str = r#"inst = Add.new
print inst.invoke("add", 2, 3), "\n"
print inst.invoke("add", 0xffffffff, 1), "\n"
print inst.invoke("fib", 10), "\n"
"#;

/// The ADR-7 override/fallback glue (an explicit `fd_write` import wins, `random_get` falls back to the bundled WASI). Intercepts fd_write and prints the actual bytes the module wrote.
const RUBY_OVERRIDE_GLUE: &str = r#"captured = +""
holder = {}
fd_write = lambda do |_fd, iovs, _iovs_len, out_ptr|
  mem = holder[:inst].memory
  ptr = mem.buffer.get_value(:u32, iovs)
  len = mem.buffer.get_value(:u32, iovs + 4)
  captured << mem.buffer.get_string(ptr, len)
  mem.buffer.set_value(:u32, out_ptr, len)
  0
end
inst = Prog.new({ "wasi_snapshot_preview1" => { "fd_write" => fd_write } })
holder[:inst] = inst
inst.invoke("_start") # random_get falls back to the bundled WASI
print captured
"#;

/// The `custom_wasi_provider` glue: a provider *object* replaces the bundled WASI wholesale — `import(name)` resolves functions, `attach(instance)` binds the memory — so the bundled WASI is never constructed (`@wasi` stays nil).
const RUBY_CUSTOM_PROVIDER_GLUE: &str = r#"
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
    ptr = @memory.buffer.get_value(:u32, iovs)
    len = @memory.buffer.get_value(:u32, iovs + 4)
    (@out ||= +"") << @memory.buffer.get_string(ptr, len)
    @memory.buffer.set_value(:u32, out_ptr, len)
    0
  end
end

wasi = MyWasi.new
inst = Prog.new({ "wasi_snapshot_preview1" => wasi })
inst.invoke("_start")
print wasi.out
print "bundled wasi constructed: ", !inst.instance_variable_get(:@wasi).nil?, "\n"
"#;

/// The `partial_override_falls_back_to_bundled_wasi` glue: reuses the override glue (fd_write intercepted, random_get falls back) plus one line probing that the bundled WASI *was* lazily constructed (ADR-7's `@wasi ||= ...`).
const RUBY_PARTIAL_OVERRIDE_GLUE: &str = r#"captured = +""
holder = {}
fd_write = lambda do |_fd, iovs, _iovs_len, out_ptr|
  mem = holder[:inst].memory
  ptr = mem.buffer.get_value(:u32, iovs)
  len = mem.buffer.get_value(:u32, iovs + 4)
  captured << mem.buffer.get_string(ptr, len)
  mem.buffer.set_value(:u32, out_ptr, len)
  0
end
inst = Prog.new({ "wasi_snapshot_preview1" => { "fd_write" => fd_write } })
holder[:inst] = inst
inst.invoke("_start") # random_get falls back to the bundled WASI
print captured
print "bundled wasi constructed: ", !inst.instance_variable_get(:@wasi).nil?, "\n"
"#;

/// The `wasi_stdio_capture` glue: redirect `$stdout` to a StringIO before instantiation (the standard Ruby capture idiom) so the module's output flows into it, then print the captured string to the real stdout.
const RUBY_STDIO_CAPTURE_GLUE: &str = r#"
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
"#;

// --------------------------------------------------------------------- WASI filesystem glue.

/// The shared filesystem template: preopen the scratch dir (`{host}`) at guest `{guest}` (always `/`), run `_start`, and surface a `proc_exit` code as a trailing decimal line.
const RUBY_FS_GLUE: &str = r#"inst = Prog.new({}, preopens: { "{guest}" => "{host}" })
begin
  inst.invoke("_start")
rescue Prog::Rt::Exit => e
  print e.code, "\n"
end
"#;

/// The root-preopen containment probe: call the WASI resolver directly with a `"/" => "/"` preopen (no guest run) and normalize the outcome to `contained`.
const RUBY_CONTAINMENT_GLUE: &str = r#"wasi = Prog::Rt::WASI.new(preopens: { "/" => "/" })
_path, err = wasi.send(:resolve_path, 3, "etc")
print(err.nil? ? "contained" : "rejected", "\n")
"#;

// --------------------------------------------------------------------- Filesystem app glue: class/argv/env/preopen-guest-paths are literals; only the host scratch/cache dirs come through {scratch}/{cache}.

const RUBY_QJS_FILE_IO_GLUE: &str = r#"inst = Qjs.new({}, args: ["qjs", "/work/qjs_file_io.js"], env: {}, preopens: {"/work" => "{scratch}"})
begin
  inst.invoke("_start")
rescue Qjs::Rt::Exit
end
"#;

const RUBY_SQLITE3_SHELL_GLUE: &str = r#"inst = Sqlite3Shell.new({}, args: ["sqlite3"], env: {}, preopens: {"/db" => "{scratch}"})
begin
  inst.invoke("_start")
rescue Sqlite3Shell::Rt::Exit
end
"#;

const RUBY_RG_SEARCH_GLUE: &str = r#"inst = Rg.new({}, args: ["rg", "--sort", "path", "needle", "/work"], env: {}, preopens: {"/work" => "{scratch}"})
begin
  inst.invoke("_start")
rescue Rg::Rt::Exit
end
"#;

const RUBY_CPYTHON_GLUE: &str = r#"inst = Cpython.new({}, args: ["python", "-c", "print('hello from cpython', 6 * 7)"], env: {"PYTHONHOME" => "/", "PYTHONPATH" => "/lib/python3.14"}, preopens: {"/lib" => "{cache}/cpython-lib/lib"})
begin
  inst.invoke("_start")
rescue Cpython::Rt::Exit
end
"#;

const RUBY_CRUBY_GLUE: &str = r#"inst = Cruby.new({}, args: ["ruby", "-e", "puts \"hello from cruby #{6*7}\""], env: {}, preopens: {"/usr" => "{cache}/ruby-lib/usr"})
begin
  inst.invoke("_start")
rescue Cruby::Rt::Exit
end
"#;

// --------------------------------------------------------------------- C-API drive glue (sqlite3): malloc/pointer plumbing via Rt::Memory. No wasmtime snapshot — the results live in guest memory — so each drive's output is pinned in the shared case const. Only the file-backed case uses {scratch}.

/// The sqlite3 C API driven in memory: `_initialize`, `sqlite3_malloc` + `Rt::Memory` pointer plumbing, open/exec/prepare/step/column/finalize/close.
const RUBY_LIBSQLITE3_MEM: &str = r##"
db_mod = Libsqlite3.new
db_mod.invoke("_initialize")
mem = db_mod.memory

def read_cstr(mem, ptr)
  return nil if ptr.zero?
  fin = ptr
  fin += 1 while mem.buffer.get_value(:U8, fin) != 0
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

/// The sqlite3 C API against a file preopen: create+insert, close, reopen, select — the file lifecycle through the C API (same ADR-14 fs stack as the shell).
const RUBY_LIBSQLITE3_FILE: &str = r##"
DB_MOD = Libsqlite3.new({}, preopens: { "/db" => "{scratch}" })
DB_MOD.invoke("_initialize")
mem = DB_MOD.memory

def read_cstr(mem, ptr)
  return nil if ptr.zero?
  fin = ptr
  fin += 1 while mem.buffer.get_value(:U8, fin) != 0
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

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to the *imported* `env.host_row`. The glue provides `host_row` via the ADR-7 import-provider mechanism and collects the rows.
const RUBY_SQLITE3_CALLBACK: &str = r##"
ROWS = []
MEM_HOLDER = {}
host_row = lambda do |argc, argv_ptr|
  mem = MEM_HOLDER[:mem]
  row = (0...argc).map do |i|
    p = mem.i32_load(argv_ptr + i * 4)
    next nil if p.zero?
    fin = p
    fin += 1 while mem.buffer.get_value(:U8, fin) != 0
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
  fin = ptr
  fin += 1 while mem.buffer.get_value(:U8, fin) != 0
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

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`.
const RUBY_PCAP_COMPILE: &str = r##"
inst = Libpcap.new
inst.invoke("_initialize")
mem = inst.memory

def cstr(inst, mem, s)
  p = inst.invoke("malloc", s.bytesize + 1)
  mem.init(p, "#{s}\0", 0, s.bytesize + 1)
  p
end

prog = inst.invoke("compile_filter", cstr(inst, mem, "tcp port 80"), 1, 65535)
raise "compile failed" if prog.zero?
n = mem.i32_load(prog)
n.times do |i|
  base = prog + 4 + i * 8
  code = mem.i32_load16_u(base)
  jt = mem.i32_load8_u(base + 2)
  jf = mem.i32_load8_u(base + 3)
  k = mem.i32_load(base + 4)
  puts "#{code} #{jt} #{jf} #{k}"
end
inst.invoke("free", prog)
puts "BPF-OK"
"##;

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}` and print the parse tree's S-expression (a malloc'd NUL-terminated C string) from guest memory.
const RUBY_TREESITTER_PARSE: &str = r##"
inst = Treesitter.new
inst.invoke("_initialize")
mem = inst.memory

def cstr(inst, mem, s)
  p = inst.invoke("malloc", s.bytesize + 1)
  mem.init(p, "#{s}\0", 0, s.bytesize + 1)
  p
end

src = %q({"key": [1, true, null]})
r = inst.invoke("parse_source", cstr(inst, mem, src), src.bytesize)
raise "parse failed" if r.zero?
fin = r
fin += 1 while mem.i32_load8_u(fin) != 0
puts mem.read_string(r, fin - r)
inst.invoke("free", r)
puts "TS-OK"
"##;

/// zeroperl Perl-5.42 eval (issue #67): instantiate the reactor with a
/// zero-returning `env.call_host_function` import stub (only invoked when the
/// guest registers host callbacks — this program registers none) and a
/// `/dev/null` preopen (`zeroperl_init` returns 1 without it), then
/// `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest
/// memory → `zeroperl_eval` → `zeroperl_flush`. The program is a regex capture
/// + `printf`, so its stdout is deterministic.
const RUBY_ZEROPERL_EVAL: &str = r##"
inst = Zeroperl.new(
  { "env" => { "call_host_function" => ->(_, _, _) { 0 } } },
  preopens: { "/dev/null" => "/dev/null" },
)
inst.invoke("_initialize")
rc = inst.invoke("zeroperl_init")
raise "zeroperl_init rc=#{rc}" unless rc.zero?
mem = inst.memory

prog = <<~'PERL'
my $s = "hello world 42";
if ($s =~ /(\w+)\s+(\w+)\s+(\d+)/) {
  printf("m=%s|%s|%d sum=%d\n", $1, $2, $3, $3 + 8);
}
PERL
bytes = "#{prog}\0"
ptr = inst.invoke("malloc", bytes.bytesize)
mem.init(ptr, bytes, 0, bytes.bytesize)
inst.invoke("zeroperl_eval", ptr, 0, 0, 0)
inst.invoke("zeroperl_flush")
"##;

/// ExifTool on zeroperl (issue #70): the flattened `exiftool` CLI driver
/// (`{cache}/exiftool-lib/exiftool`, preopened at `/work`) run on the same
/// `cache/zeroperl.wasm` reactor, whose SFS blob embeds the `Image::ExifTool`
/// module tree — so `use Image::ExifTool` resolves in-guest with no module
/// preopen. Instantiated like [`RUBY_ZEROPERL_EVAL`] (the `call_host_function`
/// stub + a `/dev/null` preopen), plus the staged image at `/img`. The Perl
/// driver snippet sets `@ARGV`/`$0` and `do`es the script; it first overrides
/// `CORE::GLOBAL::exit` to a `die` so ExifTool's terminal `exit` unwinds back
/// into `eval_pv` instead of tripping `proc_exit` — then `zeroperl_flush`
/// pushes ExifTool's buffered stdout out through fd 1. Only deterministic tags
/// are requested (`-S -Make -Model -DateTimeOriginal`), pinned in the case
/// const and cross-checked against host exiftool.
const RUBY_EXIFTOOL: &str = r##"
inst = Zeroperl.new(
  { "env" => { "call_host_function" => ->(_, _, _) { 0 } } },
  preopens: {
    "/dev/null" => "/dev/null",
    "/work" => "{cache}/exiftool-lib",
    "/img" => "{scratch}",
  },
)
inst.invoke("_initialize")
rc = inst.invoke("zeroperl_init")
raise "zeroperl_init rc=#{rc}" unless rc.zero?
mem = inst.memory

driver = <<~'PERL'
BEGIN { *CORE::GLOBAL::exit = sub (;$) { die "zeroperl_exit\n" }; }
@ARGV = ('-S', '-Make', '-Model', '-DateTimeOriginal', '/img/exif_fixture.jpg');
$0 = '/work/exiftool';
do '/work/exiftool';
PERL
bytes = "#{driver}\0"
ptr = inst.invoke("malloc", bytes.bytesize)
mem.init(ptr, bytes, 0, bytes.bytesize)
inst.invoke("zeroperl_eval", ptr, 0, 0, 0)
inst.invoke("zeroperl_flush")
"##;

// --------------------------------------------------------------------- Multi-module drive glue.

/// Driver for the shared-table case: instantiate the exporter and the importer linked against it, then print `call0` (call_indirect through the shared table -> 42).
const RUBY_SHARED_TABLE_GLUE: &str = r#"a = TableExp.new
b = TableImp.new({ "a" => a })
print b.invoke("call0"), "\n"
"#;

/// Two Embedded artifacts coexist, each with its own nested `Rt`: exercise both, prove their trap classes are distinct, and catch one's trap. Output is normalized (`distinct-rt`/`trapped`) so it matches across languages.
const RUBY_EMBEDDED_COEXIST_GLUE: &str = r#"
a = Alpha.new
b = Beta.new
print a.invoke("div", 7, 2), "\n"
print b.invoke("div", 0xfffffff9, 2), "\n"
print(Alpha::Rt::Trap != Beta::Rt::Trap ? "distinct-rt" : "same-rt", "\n")
begin
  a.invoke("div", 1, 0)
rescue Alpha::Rt::Trap
  print "trapped\n"
end
"#;

/// DOOM (ADR-53): deterministic drive (synthetic clock, no input) dumping the framebuffer as a P6 PPM matching the wasmtime snapshot. `{ticks}`/`{clock_step}` filled by the runner.
const RUBY_DOOM_FRAME_GLUE: &str = r#"frame = { off: nil, w: 0, h: 0 }
ms = [0]
imports = {
  "console" => { "onErrorMessage" => ->(o, n) {}, "onInfoMessage" => ->(o, n) {} },
  "gameSaving" => {
    "sizeOfSaveGame" => ->(i) { 0 },
    "readSaveGame" => ->(i, d) { 0 },
    "writeSaveGame" => ->(i, s, n) { n },
  },
  "runtimeControl" => { "timeInMilliseconds" => -> { ms[0] += {clock_step}; ms[0] } },
  "ui" => { "drawFrame" => ->(off) { frame[:off] = off } },
  "loading" => {
    "onGameInit" => ->(w, h) { frame[:w] = w; frame[:h] = h },
    "wadSizes" => ->(a, b) {},
    "readWads" => ->(a, b) {},
  },
}
doom = Doom.new(imports)
doom.invoke("initGame")
{ticks}.times { doom.invoke("tickGame") }
w = frame[:w]
h = frame[:h]
pixels = doom.memory.buffer.get_string(frame[:off], w * h * 4).bytes
rgb = []
pixels.each_slice(4) { |b, g, r, _a| rgb.push(r, g, b) }
$stdout.binmode
$stdout.write("P6\n#{w} #{h}\n255\n")
$stdout.write(rgb.pack("C*"))
"#;

// --------------------------------------------------------------------- Suite wiring (ADR-27): each per-case macro invocation declares participation.

library_add_e2e!(Ruby, RUBY_ADD_GLUE);
wasi_import_override_e2e!(Ruby, RUBY_OVERRIDE_GLUE);
custom_wasi_provider_e2e!(Ruby, RUBY_CUSTOM_PROVIDER_GLUE);
partial_override_e2e!(Ruby, RUBY_PARTIAL_OVERRIDE_GLUE);
stdio_capture_e2e!(Ruby, RUBY_STDIO_CAPTURE_GLUE);

wasi_suite!(Ruby, Stdio);
wasi_suite!(Ruby, ArgsEnv);
wasi_suite!(Ruby, Poll);
wasi_suite!(Ruby, Fs, RUBY_FS_GLUE);
wasi_root_containment_e2e!(Ruby, RUBY_CONTAINMENT_GLUE);
standalone_dir_e2e!(Ruby);

cowsay_args_e2e!(Ruby);
cowsay_stdin_e2e!(Ruby);
qjs_eval_e2e!(Ruby);
sqlite3_shell_e2e!(Ruby);
gzip_e2e!(Ruby);

qjs_file_io_e2e!(Ruby, RUBY_QJS_FILE_IO_GLUE);
sqlite3_shell_dbfile_e2e!(Ruby, RUBY_SQLITE3_SHELL_GLUE);
rg_search_e2e!(Ruby, RUBY_RG_SEARCH_GLUE);
cpython_hello_e2e!(Ruby, RUBY_CPYTHON_GLUE);
cruby_hello_e2e!(Ruby, RUBY_CRUBY_GLUE);
qjs_repl_pty_e2e!(Ruby);

libsqlite3_c_api_e2e!(Ruby, RUBY_LIBSQLITE3_MEM);
sqlite3_file_c_api_e2e!(Ruby, RUBY_LIBSQLITE3_FILE);
sqlite3_callback_binding_e2e!(Ruby, RUBY_SQLITE3_CALLBACK);
pcap_compile_e2e!(Ruby, RUBY_PCAP_COMPILE);
treesitter_parse_e2e!(Ruby, RUBY_TREESITTER_PARSE);
zeroperl_eval_e2e!(Ruby, RUBY_ZEROPERL_EVAL);
exiftool_extract_e2e!(Ruby, RUBY_EXIFTOOL);

doom_frame_e2e!(Ruby, RUBY_DOOM_FRAME_GLUE);

shared_table_e2e!(Ruby, RUBY_SHARED_TABLE_GLUE);
embedded_coexist_e2e!(Ruby, RUBY_EMBEDDED_COEXIST_GLUE);
