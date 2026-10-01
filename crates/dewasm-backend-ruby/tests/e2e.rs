//! Ruby end-to-end suites: the shared case constants (`dewasm-test-helper`) set up for Ruby.
//! This file holds ONLY:
//! - the [`BackendUnderTest`] implementation;
//! - named glue string constants;
//! - per-case macro calls.
//!
//! Every scenario's case content (fixtures, expectations, run logic) lives in a shared constant.
//! Glue is a plain `&str` argument at the callsite.
//! Which macros this file calls is the capability declaration.
//! A macro left uncalled carries a REASON comment.

use std::path::{Path, PathBuf};

use dewasm_backend::{Backend, Mode, RuntimeLinkage};
use dewasm_backend_ruby::{find_ruby, RubyBackend};
use dewasm_test_helper::BackendUnderTest;

pub struct Ruby;

impl BackendUnderTest for Ruby {
    fn name(&self) -> &'static str {
        "ruby"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &RubyBackend
    }

    fn interpreter(&self) -> PathBuf {
        find_ruby().expect("ruby not found on PATH (or $DEWASM_RUBY): see docs/testing.md")
    }

    /// Write each `.wat` module of a multi-module case into `dir` as its own `.rb` file.
    /// Return the `require_relative` preamble that loads them.
    /// `shared_runtime` emits each module against a single top-level `::Rt` (Alias linkage).
    /// That `::Rt` is written to `rt.rb`, so an imported table crosses modules.
    /// The specification harness's `register` path does the same.
    /// Each module file requires `rt.rb` first, since its class body resolves `::Rt` at load time.
    /// Otherwise each file is a self-contained Embedded conversion carrying its own nested `Rt`.
    fn compose_modules(
        &self,
        dir: &Path,
        modules: &[(&str, &str)],
        shared_runtime: bool,
    ) -> String {
        let mut requires = Vec::new();
        if shared_runtime {
            let mut units = std::collections::BTreeSet::new();
            let mut classes = Vec::new();
            for (wat, name) in modules {
                let bytes = wat::parse_file(dewasm_test_helper::examples_dir().join(wat))
                    .expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, u) = dewasm_backend_ruby::generate_class_with_units(
                    &module,
                    name,
                    &RuntimeLinkage::Alias("::Rt".to_string()),
                    false,
                )
                .expect("generate");
                units.extend(u);
                classes.push((name.to_lowercase(), src));
            }
            std::fs::write(
                dir.join("rt.rb"),
                dewasm_backend_ruby::shared_runtime(&units).expect("bundle runtime"),
            )
            .unwrap();
            requires.push("require_relative \"rt\"".to_string());
            for (stem, src) in classes {
                std::fs::write(
                    dir.join(format!("{stem}.rb")),
                    format!("require_relative \"rt\"\n\n{src}"),
                )
                .unwrap();
                requires.push(format!("require_relative \"{stem}\""));
            }
        } else {
            for (wat, name) in modules {
                let stem = name.to_lowercase();
                std::fs::write(
                    dir.join(format!("{stem}.rb")),
                    dewasm_test_helper::convert(
                        &RubyBackend,
                        &dewasm_test_helper::examples_dir().join(wat),
                        Mode::Library,
                        name,
                    ),
                )
                .unwrap();
                requires.push(format!("require_relative \"{stem}\""));
            }
        }
        requires.join("\n")
    }
}

/// `add.wat`: call the exported functions and print each result.
const RUBY_ADD_GLUE: &str = r#"inst = Add.new
print inst.invoke("add", 2, 3), "\n"
print inst.invoke("add", 0xffffffff, 1), "\n"
print inst.invoke("fib", 10), "\n"
"#;

/// The override/fallback glue.
/// An explicit `fd_write` import wins, and `random_get` falls back to the bundled WASI.
/// Intercepts `fd_write` and prints the actual bytes the module wrote.
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

/// The `custom_wasi_provider` glue: a provider *object* replaces the whole bundled WASI.
/// `import(name)` resolves functions, and `attach(instance)` binds the memory.
/// So the bundled WASI is never constructed (`@wasi` stays `nil`).
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

/// The `partial_override_falls_back_to_bundled_wasi` glue.
/// It reuses the override glue (`fd_write` intercepted, `random_get` falls back).
/// It adds one line probing that the bundled WASI *was* lazily constructed (`@wasi ||= ...`).
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

/// The `wasi_stdio_capture` glue: redirect `$stdout` to a StringIO before instantiation.
/// This is the standard Ruby way to capture output, so the module's output flows into it.
/// Then print the captured string to the real `stdout`.
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

/// The shared file system template.
/// Preopen the scratch directory (`{host}`) at guest `{guest}` (always `/`).
/// Then run `_start`, and surface a `proc_exit` code as a trailing decimal line.
const RUBY_FS_GLUE: &str = r#"inst = Prog.new({}, preopens: { "{guest}" => "{host}" })
begin
  inst.invoke("_start")
rescue Prog::Rt::Exit => e
  print e.code, "\n"
end
"#;

/// The root-preopen containment probe: call the WASI resolver directly with a `"/" => "/"` preopen.
/// No guest runs, and the outcome is normalized to `contained`.
const RUBY_CONTAINMENT_GLUE: &str = r#"wasi = Prog::Rt::WASI.new(preopens: { "/" => "/" })
_path, err = wasi.send(:resolve_path, 3, "etc")
print(err.nil? ? "contained" : "rejected", "\n")
"#;

// File system app glue: the class, `argv`, environment, and preopen guest paths are literals.
// Only the host scratch/cache directories come through `{scratch}`/`{cache}`.

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

/// The whole app cache is preopened at `/apps`.
/// The guest module this converted interpreter loads (`cowsay.wasm`) is itself a cached app.
const RUBY_TOYWASM_GLUE: &str = r#"inst = Toywasm.new({}, args: ["toywasm", "--wasi", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"], env: {}, preopens: {"/apps" => "{cache}"})
begin
  inst.invoke("_start")
rescue Toywasm::Rt::Exit
end
"#;

/// Like the `toywasm` glue; wasm3's CLI takes the guest module directly.
/// Its MetaWASI build always forwards the guest's WASI.
/// Plain glue, unlike every other converted-interpreter case here.
/// The official asset's dispatch is a tail call.
/// So the trampoline runs the whole chain in one Ruby frame, and no stack is raised.
const RUBY_WASM3_GLUE: &str = r#"inst = Wasm3.new({}, args: ["wasm3", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"], env: {}, preopens: {"/apps" => "{cache}"})
begin
  inst.invoke("_start")
rescue Wasm3::Rt::Exit
end
"#;

// C-API drive glue (sqlite3): `malloc`/pointer plumbing via `Rt::Memory`.
// No Wasmtime snapshot exists, since the results live in guest memory.
// So each drive's expected output is written in the shared case constant.
// Only the file-backed case uses {scratch}.

/// The sqlite3 C API driven in memory.
/// The steps are `_initialize` and `sqlite3_malloc` + `Rt::Memory` pointer plumbing.
/// Then come `open`/`exec`/`prepare`/`step`/`column`/`finalize`/`close`.
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
db = mem.iwl(pp_db)

sql = "create table t(a,b); insert into t values (1,'x'),(2,'y');"
rc = db_mod.invoke("sqlite3_exec", db, cstr(db_mod, mem, sql), 0, 0, 0)
raise "exec rc=#{rc}: #{read_cstr(mem, db_mod.invoke('sqlite3_errmsg', db))}" unless rc.zero?

pp_stmt = db_mod.invoke("sqlite3_malloc", 4)
rc = db_mod.invoke("sqlite3_prepare_v2", db,
                   cstr(db_mod, mem, "select a*10, b from t order by a desc"),
                   0xffffffff, pp_stmt, 0) # -1 as masked-unsigned i32
raise "prepare rc=#{rc}" unless rc.zero?
stmt = mem.iwl(pp_stmt)

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

/// The sqlite3 C API against a file preopen: create+insert, close, reopen, select.
/// The whole life of the file through the C API, on the same file system stack as the shell.
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
  mem.iwl(pp)
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
stmt = mem.iwl(pp_stmt)
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

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports `run_query`.
/// It calls `sqlite3_exec` with a C callback.
/// The callback forwards each row to the *imported* `env.host_row`.
/// The glue provides `host_row` via the import-provider mechanism and collects the rows.
const RUBY_SQLITE3_CALLBACK: &str = r##"
ROWS = []
MEM_HOLDER = {}
host_row = lambda do |argc, argv_ptr|
  mem = MEM_HOLDER[:mem]
  row = (0...argc).map do |i|
    p = mem.iwl(argv_ptr + i * 4)
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
db = mem.iwl(pp_db)

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

/// `libpcap` BPF filter compilation.
/// Drive `compile_filter` on "tcp port 80" (`DLT_EN10MB`, `snaplen` 65535).
/// Then walk the serialized program in guest memory.
/// Its layout is `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]`.
/// Each instruction is printed as `code jt jf k`.
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
n = mem.iwl(prog)
n.times do |i|
  base = prog + 4 + i * 8
  code = mem.uwlh(base)
  jt = mem.uwlb(base + 2)
  jf = mem.uwlb(base + 3)
  k = mem.iwl(base + 4)
  puts "#{code} #{jt} #{jf} #{k}"
end
inst.invoke("free", prog)
puts "BPF-OK"
"##;

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}`.
/// Then print the parse tree's S-expression from guest memory.
/// The S-expression is a NUL-terminated C string from `malloc`.
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
fin += 1 while mem.uwlb(fin) != 0
puts mem.read_string(r, fin - r)
inst.invoke("free", r)
puts "TS-OK"
"##;

/// `zeroperl` Perl-5.42 `eval` (issue #67).
/// Instantiate the reactor with a zero-returning `env.call_host_function` import stub.
/// The stub is only called when the guest registers host callbacks; this program registers none.
/// The reactor also gets a `/dev/null` preopen (`zeroperl_init` returns 1 without it).
/// Then run `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest memory.
/// After that come `zeroperl_eval` → `zeroperl_flush`.
/// The program is a regular expression capture + `printf`, so its `stdout` is deterministic.
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

/// ExifTool on `zeroperl` (issue #70): the flattened `exiftool` CLI driver.
/// The driver is `{cache}/exiftool-lib/exiftool`, preopened at `/work`.
/// It runs on the same `cache/zeroperl.wasm` reactor.
/// The reactor's SFS blob embeds the `Image::ExifTool` module tree.
/// So `use Image::ExifTool` resolves in-guest with no module preopen.
/// Instantiated like [`RUBY_ZEROPERL_EVAL`].
/// That is the `call_host_function` stub + a `/dev/null` preopen.
/// It also preopens the staged image at `/img`.
/// The Perl driver snippet sets `@ARGV`/`$0` and `do`es the script.
/// It first overrides `CORE::GLOBAL::exit` to a `die`.
/// So ExifTool's terminal `exit` unwinds back into `eval_pv` instead of tripping `proc_exit`.
/// Then `zeroperl_flush` pushes ExifTool's buffered `stdout` out through file descriptor 1.
/// Only deterministic tags are requested (`-S -Make -Model -DateTimeOriginal`).
/// Their expected output is written in the case constant and cross-checked against host `exiftool`.
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

/// Driver for the shared-table case.
/// Instantiate the exporter and the importer linked against it.
/// Then print `call0` (`call_indirect` through the shared table -> 42).
const RUBY_SHARED_TABLE_GLUE: &str = r#"a = TableExp.new
b = TableImp.new({ "a" => a })
print b.invoke("call0"), "\n"
"#;

/// Two Embedded artifacts coexist, each with its own nested `Rt`.
/// Exercise both, prove their trap classes are distinct, and catch one's trap.
/// Output is normalized (`distinct-rt`/`trapped`) so it matches across languages.
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

/// DOOM: deterministic drive (synthetic clock, no input).
/// It dumps the framebuffer as a P6 PPM matching the Wasmtime snapshot.
/// `{ticks}`/`{clock_step}` filled by the runner.
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

/// NES (issue #114, mirrors the DOOM glue above).
/// Load the ROM into `allocRom`'s buffer, and tick `{frames}` times with no input.
/// The ROM is checked against a fixed checksum.
/// Compose the frame from the palette-index screen buffer of `agnes` and its palette (issue #117).
/// The `& 0x3f` mask in that step is required.
/// Then dump the frame as a P6 PPM matching the Wasmtime snapshot.
/// `{rom}` (the cached ROM's host path) and `{frames}` filled by the runner.
const RUBY_NES_FRAME_GLUE: &str = r#"nes = Nes.new
nes.invoke("_initialize")
mem = nes.memory
rom = File.binread("{rom}")
ptr = nes.invoke("allocRom", rom.bytesize)
mem.init(ptr, rom, 0, rom.bytesize)
ok = nes.invoke("initGame")
raise "initGame failed: #{ok}" unless ok == 1
{frames}.times { nes.invoke("tickGame") }
w = nes.invoke("frameWidth")
h = nes.invoke("frameHeight")
screen = mem.buffer.get_string(nes.invoke("screenOffset"), w * h).bytes
palette = mem.buffer.get_string(nes.invoke("paletteOffset"), 64 * 4).bytes
rgb = []
screen.each { |ix| rgb.concat(palette[(ix & 0x3f) * 4, 3]) }
$stdout.binmode
$stdout.write("P6\n#{w} #{h}\n255\n")
$stdout.write(rgb.pack("C*"))
"#;

dewasm_test_helper::library_add_e2e!(Ruby, RUBY_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Ruby, RUBY_OVERRIDE_GLUE);
dewasm_test_helper::custom_wasi_provider_e2e!(Ruby, RUBY_CUSTOM_PROVIDER_GLUE);
dewasm_test_helper::partial_override_e2e!(Ruby, RUBY_PARTIAL_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Ruby, RUBY_STDIO_CAPTURE_GLUE);

dewasm_test_helper::wasi_suite!(Ruby, Stdio);
dewasm_test_helper::wasi_suite!(Ruby, ArgsEnv);
dewasm_test_helper::wasi_suite!(Ruby, Poll);
dewasm_test_helper::wasi_suite!(Ruby, Fs, RUBY_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Ruby, RUBY_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Ruby);
// 5000 guest frames fit the host stack Ruby's entrypoint already runs on.
// Nothing more is needed (measured).
dewasm_test_helper::deep_recursion_e2e!(Ruby);
dewasm_test_helper::folded_temp_reuse_e2e!(Ruby);

dewasm_test_helper::cowsay_args_e2e!(Ruby);
dewasm_test_helper::cowsay_stdin_e2e!(Ruby);
// Fast: mruby converts and runs well under the budget of the `cowsay` cases.
dewasm_test_helper::mruby_eh_e2e!(Ruby);
dewasm_test_helper::qjs_eval_e2e!(Ruby);
dewasm_test_helper::sqlite3_shell_e2e!(Ruby);
// Ruby only: the opcode-split shell exists for the benchmark suite's YJIT numbers.
// Every other backend converts it through the whole-cache convert suite.
// Slow, the stock shell case's class: same program on a same-sized module, same cost.
dewasm_test_helper::sqlite3_mod_shell_e2e!(Ruby);
dewasm_test_helper::gzip_e2e!(Ruby);

dewasm_test_helper::qjs_file_io_e2e!(Ruby, RUBY_QJS_FILE_IO_GLUE);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Ruby, RUBY_SQLITE3_SHELL_GLUE);
dewasm_test_helper::rg_search_e2e!(Ruby, RUBY_RG_SEARCH_GLUE);
dewasm_test_helper::cpython_hello_e2e!(Ruby, RUBY_CPYTHON_GLUE);
dewasm_test_helper::cruby_hello_e2e!(Ruby, RUBY_CRUBY_GLUE);
dewasm_test_helper::cruby_packed_hello_e2e!(Ruby);
// Slow, like the other file system app cases.
// It converts the interpreter, then interprets the `cowsay` guest.
dewasm_test_helper::toywasm_cowsay_e2e!(Ruby, RUBY_TOYWASM_GLUE);
// Slow for the same reason as the `toywasm` case above.
dewasm_test_helper::wasm3_cowsay_e2e!(Ruby, RUBY_WASM3_GLUE);
dewasm_test_helper::qjs_repl_pty_e2e!(Ruby);

dewasm_test_helper::libsqlite3_c_api_e2e!(Ruby, RUBY_LIBSQLITE3_MEM);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Ruby, RUBY_LIBSQLITE3_FILE);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Ruby, RUBY_SQLITE3_CALLBACK);
dewasm_test_helper::pcap_compile_e2e!(Ruby, RUBY_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Ruby, RUBY_TREESITTER_PARSE);
dewasm_test_helper::zeroperl_eval_e2e!(Ruby, RUBY_ZEROPERL_EVAL);
dewasm_test_helper::exiftool_extract_e2e!(Ruby, RUBY_EXIFTOOL);

dewasm_test_helper::doom_frame_e2e!(Ruby, RUBY_DOOM_FRAME_GLUE);
dewasm_test_helper::nes_frame_e2e!(Ruby, RUBY_NES_FRAME_GLUE);

dewasm_test_helper::shared_table_e2e!(Ruby, RUBY_SHARED_TABLE_GLUE);
dewasm_test_helper::embedded_coexist_e2e!(Ruby, RUBY_EMBEDDED_COEXIST_GLUE);
