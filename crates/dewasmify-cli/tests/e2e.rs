//! End-to-end tests: convert .wat examples to Ruby/Bash and run them with
//! the real interpreters.

use std::path::Path;
use std::process::Command;

use dewasmify_backend::{Backend, GenOptions, Mode, RuntimeLinkage};
use dewasmify_backend_bash::{find_bash5, BashBackend};
use dewasmify_backend_ruby::RubyBackend;

fn ruby_available() -> bool {
    Command::new("ruby").arg("--version").output().is_ok()
}

fn convert(wat_path: &Path, mode: Mode, name: &str) -> String {
    let bytes = wat::parse_file(wat_path).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let files = RubyBackend
        .generate(
            &module,
            &GenOptions {
                mode,
                module_name: name.to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate ruby");
    files.into_iter().next().unwrap().contents
}

fn examples_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wat")
}

#[test]
fn library_mode_add() {
    if !ruby_available() {
        eprintln!("ruby not found; skipping");
        return;
    }
    let code = convert(&examples_dir().join("add.wat"), Mode::Library, "add");
    let script = format!(
        "{code}\ninst = Add.new\nprint inst.invoke(\"add\", 2, 3), \"\\n\"\nprint inst.invoke(\"add\", 0xffffffff, 1), \"\\n\"\nprint inst.invoke(\"fib\", 10), \"\\n\""
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "5\n0\n55\n");
}

#[test]
fn standalone_mode_wasi_hello() {
    if !ruby_available() {
        eprintln!("ruby not found; skipping");
        return;
    }
    let code = convert(&examples_dir().join("hello.wat"), Mode::Standalone, "hello");
    let out = run_ruby(&code, &[]);
    assert_eq!(out, "Hello, WASI!\n");
}

/// The bash equivalent of `library_mode_add`: an Embedded-linkage script
/// is sourced, initialized, and invoked with results read from R0
/// (ADR-11), including a masked-unsigned wraparound and a trap.
#[test]
fn library_mode_add_bash() {
    let Some(bash) = find_bash5() else {
        eprintln!("bash >= 5 not found; skipping");
        return;
    };
    let bytes = wat::parse_file(examples_dir().join("add.wat")).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let code = BashBackend
        .generate(
            &module,
            &GenOptions {
                mode: Mode::Library,
                module_name: "add".to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate bash")
        .remove(0)
        .contents;
    let script = format!(
        "{code}\nadd_init || exit 1\n\
         add_invoke add 2 3; echo $R0\n\
         add_invoke add 4294967295 1; echo $R0\n\
         add_invoke fib 10; echo $R0\n"
    );
    let path = std::env::temp_dir().join("dewasmify-e2e-add.sh");
    std::fs::write(&path, &script).unwrap();
    let output = Command::new(&bash).arg(&path).output().expect("run bash");
    assert!(
        output.status.success(),
        "bash failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), "5\n0\n55\n");
}

/// The bash twin of `standalone_mode_wasi_hello`: the generated script is
/// executed directly; fd_write and the status-133 proc_exit protocol
/// (ADR-12) must produce the same stdout and exit code.
#[test]
fn standalone_mode_wasi_hello_bash() {
    let Some(bash) = find_bash5() else {
        eprintln!("bash >= 5 not found; skipping");
        return;
    };
    let bytes = wat::parse_file(examples_dir().join("hello.wat")).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let code = BashBackend
        .generate(
            &module,
            &GenOptions {
                mode: Mode::Standalone,
                module_name: "hello".to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate bash")
        .remove(0)
        .contents;
    let path = std::env::temp_dir().join("dewasmify-e2e-hello.sh");
    std::fs::write(&path, &code).unwrap();
    let output = Command::new(&bash).arg(&path).output().expect("run bash");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "Hello, WASI!\n");
    assert_eq!(output.status.code(), Some(0));
}

/// Standalone argv wiring: argc (program name + arguments) becomes the
/// exit code via args_sizes_get + proc_exit.
#[test]
fn standalone_args_bash() {
    let Some(bash) = find_bash5() else {
        eprintln!("bash >= 5 not found; skipping");
        return;
    };
    let wat = r#"
        (module
          (import "wasi_snapshot_preview1" "args_sizes_get"
            (func $asg (param i32 i32) (result i32)))
          (import "wasi_snapshot_preview1" "proc_exit" (func $pe (param i32)))
          (memory 1)
          (func (export "_start")
            (drop (call $asg (i32.const 0) (i32.const 4)))
            (call $pe (i32.load (i32.const 0)))))
    "#;
    let bytes = wat::parse_str(wat).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let code = BashBackend
        .generate(
            &module,
            &GenOptions {
                mode: Mode::Standalone,
                module_name: "argc".to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate bash")
        .remove(0)
        .contents;
    let path = std::env::temp_dir().join("dewasmify-e2e-argc.sh");
    std::fs::write(&path, &code).unwrap();
    let output = Command::new(&bash)
        .arg(&path)
        .args(["foo", "bar"])
        .output()
        .expect("run bash");
    assert_eq!(
        output.status.code(),
        Some(3),
        "argc = program name + 2 args"
    );
}

/// The bash analogue of `partial_override_falls_back_to_bundled_wasi`:
/// an IMPORTS entry overrides fd_write while random_get falls through to
/// the bundled units.
#[test]
fn bash_imports_override_falls_back_to_bundled_wasi() {
    let Some(bash) = find_bash5() else {
        eprintln!("bash >= 5 not found; skipping");
        return;
    };
    let bytes = wat::parse_str(WASI_IMPORTS_WAT).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let code = BashBackend
        .generate(
            &module,
            &GenOptions {
                mode: Mode::Library,
                module_name: "prog".to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate bash")
        .remove(0)
        .contents;
    let script = format!(
        "{code}\n{}",
        r#"
my_fd_write() {
  # (fd, iovs, iovs_len, nwritten_ptr): prove we intercepted the call by
  # reading the first iovec through the module's memory helpers.
  mem_i32_load prog_ "$2" || return $?
  local ptr=$R0
  mem_i32_load prog_ $(( $2 + 4 )) || return $?
  local len=$R0
  echo "custom fd_write: fd=$1 len=$len"
  mem_i32_store prog_ "$4" "$len" || return $?
  R0=0
  return 0
}
declare -A IMPORTS=(['wasi_snapshot_preview1.fd_write']=my_fd_write)
prog_init || { echo "init failed" >&2; exit 1; }
prog_invoke '_start'
echo "status=$?"
"#
    );
    let path = std::env::temp_dir().join("dewasmify-e2e-override.sh");
    std::fs::write(&path, &script).unwrap();
    let output = Command::new(&bash).arg(&path).output().expect("run bash");
    assert!(
        output.status.success(),
        "bash failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "custom fd_write: fd=1 len=3\nstatus=0\n"
    );
}

/// A module that imports WASI functions, for exercising the import
/// provider protocol (ADR-7).
const WASI_IMPORTS_WAT: &str = r#"
    (module
      (import "wasi_snapshot_preview1" "fd_write"
        (func $fdw (param i32 i32 i32 i32) (result i32)))
      (import "wasi_snapshot_preview1" "random_get"
        (func $rnd (param i32 i32) (result i32)))
      (memory (export "memory") 1)
      (data (i32.const 8) "ok\n")
      (func (export "_start")
        (drop (call $rnd (i32.const 100) (i32.const 4)))
        (i32.store (i32.const 0) (i32.const 8))
        (i32.store (i32.const 4) (i32.const 3))
        (drop (call $fdw (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 20)))))
"#;

fn convert_str(wat: &str, name: &str) -> String {
    let bytes = wat::parse_str(wat).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    RubyBackend
        .generate(
            &module,
            &GenOptions {
                mode: Mode::Library,
                module_name: name.to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate ruby")
        .remove(0)
        .contents
}

/// A provider object under the wasi key replaces the bundled WASI
/// wholesale: import(name) resolves functions, attach(instance) binds the
/// memory. The bundled Rt::WASI must then never be constructed.
#[test]
fn custom_wasi_provider() {
    if !ruby_available() {
        eprintln!("ruby not found; skipping");
        return;
    }
    let script = format!(
        "{}\n{}",
        convert_str(WASI_IMPORTS_WAT, "prog"),
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

/// A partial Hash override: provided names win, the rest falls back to
/// the bundled WASI (constructed on demand).
#[test]
fn partial_override_falls_back_to_bundled_wasi() {
    if !ruby_available() {
        eprintln!("ruby not found; skipping");
        return;
    }
    let script = format!(
        "{}\n{}",
        convert_str(WASI_IMPORTS_WAT, "prog"),
        r#"
captured = +""
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
print "bundled wasi constructed: ", !inst.instance_variable_get(:@wasi).nil?, "\n"
"#
    );
    let out = run_ruby(&script, &[]);
    assert_eq!(out, "ok\nbundled wasi constructed: true\n");
}

/// Two Embedded-linkage artifacts must coexist in one process: each
/// carries its own nested `Rt`, so runtime classes (and even different
/// runtime versions, eventually) never collide.
#[test]
fn embedded_runtimes_coexist() {
    if !ruby_available() {
        eprintln!("ruby not found; skipping");
        return;
    }
    let wat = r#"
        (module
          (memory 1)
          (func (export "div") (param i32 i32) (result i32)
            local.get 0
            local.get 1
            i32.div_s))
    "#;
    let bytes = wat::parse_str(wat).expect("parse wat");
    let module = dewasmify_core::build_module(&bytes).expect("build IR");
    let gen = |name: &str| {
        RubyBackend
            .generate(
                &module,
                &GenOptions {
                    mode: Mode::Library,
                    module_name: name.to_string(),
                    runtime: RuntimeLinkage::Embedded,
                    default_wasi: true,
                },
            )
            .expect("generate ruby")
            .remove(0)
            .contents
    };
    let script = format!(
        "{}\n{}\n{}",
        gen("alpha"),
        gen("beta"),
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

fn run_ruby(script: &str, args: &[&str]) -> String {
    let path = std::env::temp_dir().join(format!(
        "dewasmify-e2e-{}.rb",
        std::process::id() as u64 + script.len() as u64
    ));
    std::fs::write(&path, script).unwrap();
    let output = Command::new("ruby")
        .arg(&path)
        .args(args)
        .output()
        .expect("run ruby");
    assert!(
        output.status.success(),
        "ruby failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}
