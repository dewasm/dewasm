//! End-to-end coverage for `--data-file` data-segment externalization. For Ruby, Go, Python, Perl and Java: convert a module both embedded and with a sidecar, run each generated program, and assert byte-identical stdout/exit plus a smaller source file. Also pins the loud rejections (the bash target, `-o -`).
//!
//! The inline fixture carries an active segment, a passive segment initialized via `memory.init` + `data.drop`, and a bulky third segment so the sidecar form provably shrinks the source. The slow real-app cases (`qjs.wasm`) are `#[ignore]`d unless the `slow_test` feature is on, matching the project's speed-category convention for cases that pay a multi-second `go build` / interpreter startup (run with `--features slow_test` or `--include-ignored`).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dewasm_backend_go::find_go;
use dewasm_backend_java::{find_java, javac_command};
use dewasm_backend_perl::find_perl;
use dewasm_backend_python::find_python;
use dewasm_backend_ruby::find_ruby;

fn dewasm_bin() -> &'static str {
    env!("CARGO_BIN_EXE_dewasm")
}

/// A standalone module exercising every data-emission path: an active segment (index 0), a passive segment written by `memory.init` then `data.drop`ped (index 1), and a 2 KiB third segment (index 2) whose bytes only exist to make the embedded hex dwarf the externalized sidecar. `_start` prints `Active!\nPassive!\n`.
fn fixture_wat() -> String {
    let bulk = "x".repeat(2000);
    format!(
        r#"(module
  (import "wasi_snapshot_preview1" "fd_write"
    (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory (export "memory") 1)
  (data (i32.const 16) "Active!\n")
  (data "Passive!\n")
  (data "{bulk}")
  (func (export "_start")
    (i32.store (i32.const 0) (i32.const 16))
    (i32.store (i32.const 4) (i32.const 8))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 100)))
    (memory.init 1 (i32.const 32) (i32.const 0) (i32.const 9))
    (data.drop 1)
    (i32.store (i32.const 0) (i32.const 32))
    (i32.store (i32.const 4) (i32.const 9))
    (drop (call $fd_write (i32.const 1) (i32.const 0) (i32.const 1) (i32.const 100)))
    (call $proc_exit (i32.const 0)))
)
"#
    )
}

const FIXTURE_STDOUT: &str = "Active!\nPassive!\n";
/// Total data bytes: 8 (active) + 9 (passive) + 2000 (bulk).
const FIXTURE_DATA_LEN: u64 = 8 + 9 + 2000;

fn tempdir(tag: &str) -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dewasm-data-file-{}-{}-{tag}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn run_dewasm(args: &[&str]) -> Output {
    Command::new(dewasm_bin())
        .args(args)
        .output()
        .expect("spawn dewasm")
}

fn write(path: &Path, contents: &str) {
    std::fs::write(path, contents).unwrap();
}

/// Run a Ruby program from its own directory (so `__dir__`-relative sidecar loads resolve), returning (stdout, exit code).
fn run_ruby(prog: &Path, args: &[&str]) -> (Vec<u8>, i32) {
    let ruby = find_ruby().expect("ruby >= 3.4 not found on PATH — see docs/testing.md");
    let out = Command::new(ruby)
        .arg(prog)
        .args(args)
        .current_dir(prog.parent().unwrap())
        .output()
        .expect("spawn ruby");
    (out.stdout, out.status.code().unwrap_or(-1))
}

/// `go build` a program in its own directory (so `//go:embed` resolves) and run the resulting binary, returning (stdout, exit code).
fn run_go(prog: &Path, args: &[&str]) -> (Vec<u8>, i32) {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO) — see docs/testing.md");
    let dir = prog.parent().unwrap();
    let bin = dir.join("prog_bin");
    let build = Command::new(&go)
        .arg("build")
        .arg("-o")
        .arg(&bin)
        .arg(prog.file_name().unwrap())
        .current_dir(dir)
        .output()
        .expect("spawn go build");
    assert!(
        build.status.success(),
        "go build failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let out = Command::new(&bin)
        .args(args)
        .current_dir(dir)
        .output()
        .expect("spawn go program");
    (out.stdout, out.status.code().unwrap_or(-1))
}

/// Run a Python program from its own directory (so the sidecar, resolved via `os.path.dirname(__file__)`, is found), returning (stdout, exit code).
fn run_python(prog: &Path, args: &[&str]) -> (Vec<u8>, i32) {
    let python = find_python().expect("python3 not found on PATH — see docs/testing.md");
    let out = Command::new(python)
        .arg(prog)
        .args(args)
        .current_dir(prog.parent().unwrap())
        .output()
        .expect("spawn python");
    (out.stdout, out.status.code().unwrap_or(-1))
}

/// Run a Perl program from its own directory (so the sidecar, resolved via `File::Basename::dirname(__FILE__)`, is found), returning (stdout, exit code).
fn run_perl(prog: &Path, args: &[&str]) -> (Vec<u8>, i32) {
    let perl = find_perl()
        .expect("perl >= 5.26 with 64-bit IVs/NVs not found on PATH — see docs/testing.md");
    let out = Command::new(perl)
        .arg(prog)
        .args(args)
        .current_dir(prog.parent().unwrap())
        .output()
        .expect("spawn perl");
    (out.stdout, out.status.code().unwrap_or(-1))
}

/// Compile `Main.java` into `classdir` (mirrors the java spec harness recipe).
fn compile_java(src: &Path, classdir: &Path) {
    std::fs::create_dir_all(classdir).unwrap();
    let build = javac_command()
        .arg("-d")
        .arg(classdir)
        .arg(src)
        .output()
        .expect("spawn javac");
    assert!(
        build.status.success(),
        "javac failed:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
}

/// Run `Main` from `classdir` on the classpath (so its `DATA_BLOB` loader resolves the sidecar sitting alongside `Main.class`), returning (stdout, exit code).
fn run_java(classdir: &Path, args: &[&str]) -> (Vec<u8>, i32) {
    let java = find_java().expect("java not found on PATH (or $DEWASM_JAVA) — see docs/testing.md");
    let out = Command::new(&java)
        .arg("-cp")
        .arg(classdir)
        .arg("Main")
        .args(args)
        .output()
        .expect("spawn java");
    (out.stdout, out.status.code().unwrap_or(-1))
}

#[test]
fn ruby_data_file_matches_embedded() {
    let dir = tempdir("ruby");
    let wat = dir.join("mod.wat");
    write(&wat, &fixture_wat());
    let watp = wat.to_str().unwrap();

    let embedded = dir.join("embedded.rb");
    let ext = dir.join("ext.rb");
    let sidecar = dir.join("ext.bin");

    let e = run_dewasm(&[
        watp,
        "-t",
        "ruby",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap(),
    ]);
    assert!(
        e.status.success(),
        "embedded convert: {}",
        String::from_utf8_lossy(&e.stderr)
    );
    let x = run_dewasm(&[
        watp,
        "-t",
        "ruby",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap(),
    ]);
    assert!(
        x.status.success(),
        "data-file convert: {}",
        String::from_utf8_lossy(&x.stderr)
    );

    assert_eq!(std::fs::metadata(&sidecar).unwrap().len(), FIXTURE_DATA_LEN);
    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized .rb ({src_ext} B) should be smaller than embedded ({src_embedded} B)"
    );

    let (out_e, code_e) = run_ruby(&embedded, &[]);
    let (out_x, code_x) = run_ruby(&ext, &[]);
    assert_eq!(out_e, FIXTURE_STDOUT.as_bytes());
    assert_eq!(
        out_e, out_x,
        "stdout differs between embedded and externalized"
    );
    assert_eq!(code_e, 0);
    assert_eq!(code_e, code_x);
}

#[test]
fn go_data_file_matches_embedded() {
    let dir = tempdir("go");
    let wat = dir.join("mod.wat");
    write(&wat, &fixture_wat());
    let watp = wat.to_str().unwrap();

    // Distinct dirs: each go:embed program builds in isolation.
    let edir = dir.join("embedded");
    let xdir = dir.join("ext");
    std::fs::create_dir_all(&edir).unwrap();
    std::fs::create_dir_all(&xdir).unwrap();
    let embedded = edir.join("prog.go");
    let ext = xdir.join("prog.go");
    let sidecar = xdir.join("data.bin");

    let e = run_dewasm(&[
        watp,
        "-t",
        "go",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap(),
    ]);
    assert!(
        e.status.success(),
        "embedded convert: {}",
        String::from_utf8_lossy(&e.stderr)
    );
    let x = run_dewasm(&[
        watp,
        "-t",
        "go",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap(),
    ]);
    assert!(
        x.status.success(),
        "data-file convert: {}",
        String::from_utf8_lossy(&x.stderr)
    );

    assert_eq!(std::fs::metadata(&sidecar).unwrap().len(), FIXTURE_DATA_LEN);
    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized .go ({src_ext} B) should be smaller than embedded ({src_embedded} B)"
    );

    let (out_e, code_e) = run_go(&embedded, &[]);
    let (out_x, code_x) = run_go(&ext, &[]);
    assert_eq!(out_e, FIXTURE_STDOUT.as_bytes());
    assert_eq!(
        out_e, out_x,
        "stdout differs between embedded and externalized"
    );
    assert_eq!(code_e, 0);
    assert_eq!(code_e, code_x);
}

#[test]
fn python_data_file_matches_embedded() {
    let dir = tempdir("python");
    let wat = dir.join("mod.wat");
    write(&wat, &fixture_wat());
    let watp = wat.to_str().unwrap();

    let embedded = dir.join("embedded.py");
    let ext = dir.join("ext.py");
    let sidecar = dir.join("ext.bin");

    let e = run_dewasm(&[
        watp,
        "-t",
        "python",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap(),
    ]);
    assert!(
        e.status.success(),
        "embedded convert: {}",
        String::from_utf8_lossy(&e.stderr)
    );
    let x = run_dewasm(&[
        watp,
        "-t",
        "python",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap(),
    ]);
    assert!(
        x.status.success(),
        "data-file convert: {}",
        String::from_utf8_lossy(&x.stderr)
    );

    assert_eq!(std::fs::metadata(&sidecar).unwrap().len(), FIXTURE_DATA_LEN);
    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized .py ({src_ext} B) should be smaller than embedded ({src_embedded} B)"
    );

    let (out_e, code_e) = run_python(&embedded, &[]);
    let (out_x, code_x) = run_python(&ext, &[]);
    assert_eq!(out_e, FIXTURE_STDOUT.as_bytes());
    assert_eq!(
        out_e, out_x,
        "stdout differs between embedded and externalized"
    );
    assert_eq!(code_e, 0);
    assert_eq!(code_e, code_x);
}

#[test]
fn perl_data_file_matches_embedded() {
    let dir = tempdir("perl");
    let wat = dir.join("mod.wat");
    write(&wat, &fixture_wat());
    let watp = wat.to_str().unwrap();

    let embedded = dir.join("embedded.pl");
    let ext = dir.join("ext.pl");
    let sidecar = dir.join("ext.bin");

    let e = run_dewasm(&[
        watp,
        "-t",
        "perl",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap(),
    ]);
    assert!(
        e.status.success(),
        "embedded convert: {}",
        String::from_utf8_lossy(&e.stderr)
    );
    let x = run_dewasm(&[
        watp,
        "-t",
        "perl",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap(),
    ]);
    assert!(
        x.status.success(),
        "data-file convert: {}",
        String::from_utf8_lossy(&x.stderr)
    );

    assert_eq!(std::fs::metadata(&sidecar).unwrap().len(), FIXTURE_DATA_LEN);
    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized .pl ({src_ext} B) should be smaller than embedded ({src_embedded} B)"
    );

    let (out_e, code_e) = run_perl(&embedded, &[]);
    let (out_x, code_x) = run_perl(&ext, &[]);
    assert_eq!(out_e, FIXTURE_STDOUT.as_bytes());
    assert_eq!(
        out_e, out_x,
        "stdout differs between embedded and externalized"
    );
    assert_eq!(code_e, 0);
    assert_eq!(code_e, code_x);
}

#[test]
fn java_data_file_matches_embedded() {
    let dir = tempdir("java");
    let wat = dir.join("mod.wat");
    write(&wat, &fixture_wat());
    let watp = wat.to_str().unwrap();

    let embedded = dir.join("embedded").join("Main.java");
    let ecls = dir.join("embedded-cls");
    std::fs::create_dir_all(embedded.parent().unwrap()).unwrap();
    // Externalized: the sidecar lands directly in the run-time class dir so the `DATA_BLOB` code-source loader finds it next to `Main.class`.
    let ext = dir.join("ext").join("Main.java");
    let xcls = dir.join("ext-cls");
    std::fs::create_dir_all(ext.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&xcls).unwrap();
    let sidecar = xcls.join("data.bin");

    let e = run_dewasm(&[
        watp,
        "-t",
        "java",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap(),
    ]);
    assert!(
        e.status.success(),
        "embedded convert: {}",
        String::from_utf8_lossy(&e.stderr)
    );
    let x = run_dewasm(&[
        watp,
        "-t",
        "java",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap(),
    ]);
    assert!(
        x.status.success(),
        "data-file convert: {}",
        String::from_utf8_lossy(&x.stderr)
    );

    assert_eq!(std::fs::metadata(&sidecar).unwrap().len(), FIXTURE_DATA_LEN);
    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized Main.java ({src_ext} B) should be smaller than embedded ({src_embedded} B)"
    );

    compile_java(&embedded, &ecls);
    compile_java(&ext, &xcls);

    let (out_e, code_e) = run_java(&ecls, &[]);
    let (out_x, code_x) = run_java(&xcls, &[]);
    assert_eq!(out_e, FIXTURE_STDOUT.as_bytes());
    assert_eq!(
        out_e, out_x,
        "stdout differs between embedded and externalized"
    );
    assert_eq!(code_e, 0);
    assert_eq!(code_e, code_x);
}

#[test]
fn rejects_unsupported_targets_and_stdout() {
    let dir = tempdir("reject");
    let wat = dir.join("mod.wat");
    write(&wat, &fixture_wat());
    let watp = wat.to_str().unwrap();
    let sidecar = dir.join("d.bin");
    let sc = sidecar.to_str().unwrap();

    // Bash is the sole target that rejects `--data-file` (its data lives in the runtime); the error names the target.
    let out = dir.join("out.bash");
    let r = run_dewasm(&[
        watp,
        "-t",
        "bash",
        "-m",
        "library",
        "-o",
        out.to_str().unwrap(),
        "--data-file",
        sc,
    ]);
    assert!(!r.status.success(), "bash: expected rejection");
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("bash"),
        "bash: error should mention the target, got: {}",
        String::from_utf8_lossy(&r.stderr)
    );

    // --data-file with stdout output is rejected even for a supported target.
    let r = run_dewasm(&[
        watp,
        "-t",
        "ruby",
        "-m",
        "standalone",
        "-o",
        "-",
        "--data-file",
        sc,
    ]);
    assert!(!r.status.success(), "expected -o - rejection");
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("stdout"),
        "error should mention stdout"
    );
}

/// A `--data-file` resolving to the same file as `-o` is rejected before anything is written: the blob would otherwise clobber the freshly written source (#30). Covers both the identical spelling and a `..`-hop alias of the same path.
#[test]
fn rejects_data_file_colliding_with_output_path() {
    let dir = tempdir("collide-output");
    let wat = dir.join("mod.wat");
    write(&wat, &fixture_wat());
    let watp = wat.to_str().unwrap();

    let out = dir.join("out.py");
    // Pre-existing content must survive the rejected conversion untouched.
    write(&out, "sentinel");

    let r = run_dewasm(&[
        watp,
        "-t",
        "python",
        "-m",
        "standalone",
        "-o",
        out.to_str().unwrap(),
        "--data-file",
        out.to_str().unwrap(),
    ]);
    assert!(!r.status.success(), "expected same-path rejection");
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("same file"),
        "error should say the paths are the same file, got: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "sentinel");

    // A differently spelled alias of the same file ("dir/../dir/out.py") must be caught too: the check compares resolved paths, not strings.
    let alias = dir.join("..").join(dir.file_name().unwrap()).join("out.py");
    let r = run_dewasm(&[
        watp,
        "-t",
        "python",
        "-m",
        "standalone",
        "-o",
        out.to_str().unwrap(),
        "--data-file",
        alias.to_str().unwrap(),
    ]);
    assert!(!r.status.success(), "expected aliased-path rejection");
    assert_eq!(std::fs::read_to_string(&out).unwrap(), "sentinel");
}

/// A `--data-file` whose filename collides with a generated output file's name is rejected: routing is by name, so the java backend's fixed `Main.java` source would be misrouted to the sidecar path and clobbered by the blob (#30). Covered with data segments (source and sidecar share the name) and without (the lone source itself matches the sidecar name).
#[test]
fn rejects_data_file_colliding_with_generated_name() {
    let dir = tempdir("collide-name");
    let wat = dir.join("mod.wat");
    write(&wat, &fixture_wat());

    let out = dir.join("out").join("Main.java");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    let sidecar = dir.join("sidecar").join("Main.java");
    std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();

    let r = run_dewasm(&[
        wat.to_str().unwrap(),
        "-t",
        "java",
        "-m",
        "standalone",
        "-o",
        out.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap(),
    ]);
    assert!(!r.status.success(), "expected name-collision rejection");
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("collides"),
        "error should name the collision, got: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    assert!(!out.exists(), "no source may be written on rejection");
    assert!(!sidecar.exists(), "no sidecar may be written on rejection");

    // Same collision with a data-less module: the backend emits no sidecar, so the lone `Main.java` source itself matches the sidecar name and would be misrouted; it must be rejected identically.
    let nodata = dir.join("nodata.wat");
    write(&nodata, "(module (func (export \"_start\")))\n");
    let r = run_dewasm(&[
        nodata.to_str().unwrap(),
        "-t",
        "java",
        "-m",
        "standalone",
        "-o",
        out.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap(),
    ]);
    assert!(
        !r.status.success(),
        "expected name-collision rejection (no data segments)"
    );
    assert!(!out.exists(), "no source may be written on rejection");
    assert!(!sidecar.exists(), "no sidecar may be written on rejection");
}

fn qjs_wasm() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/cache/qjs.wasm")
}

/// qjs `-e` one-liner; matches the shape the app suite uses (apps.rs QJS_EVAL).
const QJS_ARGS: &[&str] = &["-e", "console.log(6 * 7)"];
const QJS_STDOUT: &str = "42\n";

#[test]
#[cfg_attr(
    not(feature = "slow_test"),
    ignore = "slow: converts and runs the cached qjs.wasm (multi-second): --features slow_test or -- --include-ignored"
)]
fn ruby_qjs_data_file_matches_embedded() {
    let wasm = qjs_wasm();
    assert!(
        wasm.exists(),
        "qjs not cached — run examples/apps/setup.sh (see docs/testing.md)"
    );
    let wasm = wasm.to_str().unwrap();
    let dir = tempdir("qjs-ruby");
    let embedded = dir.join("embedded.rb");
    let ext = dir.join("qjs.rb");
    let sidecar = dir.join("qjs.bin");

    assert!(run_dewasm(&[
        wasm,
        "-t",
        "ruby",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap()
    ])
    .status
    .success());
    assert!(run_dewasm(&[
        wasm,
        "-t",
        "ruby",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap()
    ])
    .status
    .success());

    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized qjs.rb ({src_ext}) should shrink vs embedded ({src_embedded})"
    );

    let (out_e, code_e) = run_ruby(&embedded, QJS_ARGS);
    let (out_x, code_x) = run_ruby(&ext, QJS_ARGS);
    assert_eq!(String::from_utf8_lossy(&out_e), QJS_STDOUT);
    assert_eq!(out_e, out_x);
    assert_eq!(code_e, code_x);
}

#[test]
#[cfg_attr(
    not(feature = "slow_test"),
    ignore = "slow: go build of the cached qjs.wasm source (multi-second): --features slow_test or -- --include-ignored"
)]
fn go_qjs_data_file_matches_embedded() {
    let wasm = qjs_wasm();
    assert!(
        wasm.exists(),
        "qjs not cached — run examples/apps/setup.sh (see docs/testing.md)"
    );
    let wasm = wasm.to_str().unwrap();
    let dir = tempdir("qjs-go");
    let edir = dir.join("embedded");
    let xdir = dir.join("ext");
    std::fs::create_dir_all(&edir).unwrap();
    std::fs::create_dir_all(&xdir).unwrap();
    let embedded = edir.join("prog.go");
    let ext = xdir.join("prog.go");
    let sidecar = xdir.join("qjs.bin");

    assert!(run_dewasm(&[
        wasm,
        "-t",
        "go",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap()
    ])
    .status
    .success());
    assert!(run_dewasm(&[
        wasm,
        "-t",
        "go",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap()
    ])
    .status
    .success());

    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized qjs.go ({src_ext}) should shrink vs embedded ({src_embedded})"
    );

    let (out_e, code_e) = run_go(&embedded, QJS_ARGS);
    let (out_x, code_x) = run_go(&ext, QJS_ARGS);
    assert_eq!(String::from_utf8_lossy(&out_e), QJS_STDOUT);
    assert_eq!(out_e, out_x);
    assert_eq!(code_e, code_x);
}

#[test]
#[cfg_attr(
    not(feature = "slow_test"),
    ignore = "slow: converts and runs the cached qjs.wasm (multi-second): --features slow_test or -- --include-ignored"
)]
fn python_qjs_data_file_matches_embedded() {
    let wasm = qjs_wasm();
    assert!(
        wasm.exists(),
        "qjs not cached — run examples/apps/setup.sh (see docs/testing.md)"
    );
    let wasm = wasm.to_str().unwrap();
    let dir = tempdir("qjs-python");
    let embedded = dir.join("embedded.py");
    let ext = dir.join("qjs.py");
    let sidecar = dir.join("qjs.bin");

    assert!(run_dewasm(&[
        wasm,
        "-t",
        "python",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap()
    ])
    .status
    .success());
    assert!(run_dewasm(&[
        wasm,
        "-t",
        "python",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap()
    ])
    .status
    .success());

    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized qjs.py ({src_ext}) should shrink vs embedded ({src_embedded})"
    );

    let (out_e, code_e) = run_python(&embedded, QJS_ARGS);
    let (out_x, code_x) = run_python(&ext, QJS_ARGS);
    assert_eq!(String::from_utf8_lossy(&out_e), QJS_STDOUT);
    assert_eq!(out_e, out_x);
    assert_eq!(code_e, code_x);
}

#[test]
#[cfg_attr(
    not(feature = "slow_test"),
    ignore = "slow: javac of the cached qjs.wasm source (multi-second): --features slow_test or -- --include-ignored"
)]
fn java_qjs_data_file_matches_embedded() {
    let wasm = qjs_wasm();
    assert!(
        wasm.exists(),
        "qjs not cached — run examples/apps/setup.sh (see docs/testing.md)"
    );
    let wasm = wasm.to_str().unwrap();
    let dir = tempdir("qjs-java");
    let embedded = dir.join("embedded").join("Main.java");
    let ecls = dir.join("embedded-cls");
    std::fs::create_dir_all(embedded.parent().unwrap()).unwrap();
    let ext = dir.join("ext").join("Main.java");
    let xcls = dir.join("ext-cls");
    std::fs::create_dir_all(ext.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&xcls).unwrap();
    let sidecar = xcls.join("qjs.bin");

    assert!(run_dewasm(&[
        wasm,
        "-t",
        "java",
        "-m",
        "standalone",
        "-o",
        embedded.to_str().unwrap()
    ])
    .status
    .success());
    assert!(run_dewasm(&[
        wasm,
        "-t",
        "java",
        "-m",
        "standalone",
        "-o",
        ext.to_str().unwrap(),
        "--data-file",
        sidecar.to_str().unwrap()
    ])
    .status
    .success());

    let src_embedded = std::fs::metadata(&embedded).unwrap().len();
    let src_ext = std::fs::metadata(&ext).unwrap().len();
    assert!(
        src_ext < src_embedded,
        "externalized qjs Main.java ({src_ext}) should shrink vs embedded ({src_embedded})"
    );

    compile_java(&embedded, &ecls);
    compile_java(&ext, &xcls);

    let (out_e, code_e) = run_java(&ecls, QJS_ARGS);
    let (out_x, code_x) = run_java(&xcls, QJS_ARGS);
    assert_eq!(String::from_utf8_lossy(&out_e), QJS_STDOUT);
    assert_eq!(out_e, out_x);
    assert_eq!(code_e, code_x);
}
