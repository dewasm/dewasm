//! Go end-to-end suites (ADR-27): the shared standalone / library / WASI /
//! apps case tables (`dewasm-test-helper`) wired up for the Go backend.
//!
//! Go is the first *compiled* backend, so it overrides `BackendUnderTest::run`
//! (ADR-27's hook) to compile-and-execute instead of interpreting: `go build`
//! the generated file to a content-addressed cache binary (so identical
//! sources — e.g. cowsay's args and stdin cases — build once), then run the
//! binary directly. Running the binary (not `go run`) is required because
//! `go run` does not propagate the guest exit code (it prints "exit status N"
//! and exits 1); the WASI args/env case asserts an exact exit code.
//!
//! First-milestone scope (ADR-29): "cowsay runs". WASI covers the eight core
//! syscalls (stdio + args/env), so the whole-program `Stdio`/`ArgsEnv` kinds
//! and the two library cases are wired, plus `apps_e2e!` (cowsay byte-identical;
//! the heavy qjs/sqlite cases stay off, and the `Fs` suite and `gzip_e2e!` wait
//! for the WASI filesystem — minigzip needs `path_open`/`fd_seek`).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::process::{Command, Output};

use dewasm_backend::{Backend, Mode};
use dewasm_backend_go::{find_go, GoBackend};
use dewasm_test_helper::{
    apps_cache_dir, apps_e2e, apps_fixtures_dir, convert_on_big_stack, fresh_scratch_dir, gzip_e2e,
    library_e2e, run_command_bytes, standalone_e2e, wasi_suite, BackendUnderTest, LibraryCase,
    WasiCase,
};

pub struct Go;

impl BackendUnderTest for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &GoBackend
    }

    /// Compile `source` to a cache binary (keyed by content hash) and run it
    /// with `args`/`stdin`. A missing `go` toolchain is a loud failure
    /// (ADR-15); a build failure is surfaced as the build command's `Output`
    /// so the caller's `status.success()` assertion reports the compile error.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        let go = find_go()
            .expect("go toolchain not found on PATH (or $DEWASM_GO) — see docs/testing.md");

        let mut hasher = DefaultHasher::new();
        source.hash(&mut hasher);
        let hash = hasher.finish();

        let cache = std::env::temp_dir().join("dewasm-go-cache");
        std::fs::create_dir_all(&cache).unwrap();
        let bin = cache.join(format!("prog-{hash:016x}"));

        if !bin.exists() {
            let src = cache.join(format!("src-{hash:016x}.go"));
            std::fs::write(&src, source).unwrap();
            // Build to a unique path, then rename onto the cache key so
            // concurrent test threads never hand out a half-written binary.
            let tmp_bin = cache.join(format!(
                "prog-{hash:016x}.{}.{}",
                std::process::id(),
                COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            let build = Command::new(&go)
                .arg("build")
                .arg("-o")
                .arg(&tmp_bin)
                .arg(&src)
                .output()
                .expect("spawn go build");
            if !build.status.success() {
                return build;
            }
            let _ = std::fs::rename(&tmp_bin, &bin);
        }

        run_command_bytes(Command::new(&bin).args(args), stdin)
    }

    /// QuickJS and SQLite now run to completion under Go's full WASI surface
    /// (ADR-29 third milestone) and pass, so — like the fast interpreters
    /// (Ruby/Python) — Go runs the heavy `apps` cases (qjs, sqlite3-shell,
    /// in-memory) by default. Go is compiled, so its cost is bimodal but
    /// competitive: measured locally cowsay+qjs+sqlite3-shell standalone is
    /// ~33 s cold (Go's own build cache empty) and ~3 s on every subsequent
    /// run (that cache warm) — cheaper across repeated runs than Ruby's ~15 s
    /// (interpreted, no caching), and far under the ADR-24 5-minute bar. The
    /// much heavier filesystem app cases (qjs/sqlite reconversion for the fs
    /// scenarios, rg's 22 MB wasm) stay behind `DEWASM_APPS_ALL` in the tail of
    /// this file, mirroring how the Ruby/Python crates gate their heaviest
    /// cases.
    fn run_heavy_apps(&self) -> bool {
        true
    }
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Per-case Go glue (a `func main` appended after the generated declarations;
/// it carries no `import` — the generated file already imports `fmt`). A case
/// wired to run but with no glue panics loudly (ADR-15).
fn go_glue(case: &LibraryCase) -> &'static str {
    match case.name {
        "add" => {
            "func main() {\n\
             \tinst := NewAdd(nil, nil, nil, nil)\n\
             \tfmt.Println(inst.Exports[\"add\"].(func(uint32, uint32) uint32)(2, 3))\n\
             \tfmt.Println(inst.Exports[\"add\"].(func(uint32, uint32) uint32)(0xffffffff, 1))\n\
             \tfmt.Println(inst.Exports[\"fib\"].(func(uint32) uint32)(10))\n\
             }\n"
        }
        "wasi_import_override" => GO_OVERRIDE_GLUE,
        other => panic!("{other}: no go glue"),
    }
}

/// The ADR-7 override/fallback glue: an explicit `fd_write` import wins,
/// `random_get` falls back to the bundled WASI. Mirrors the Ruby/Python/Bash
/// override glues — intercept fd_write and print the actual bytes written.
const GO_OVERRIDE_GLUE: &str = r#"func main() {
	var captured []byte
	var inst *Prog
	fdWrite := func(fd, iovs, iovsLen, outPtr uint32) uint32 {
		ptr := inst.memory.i32_load(uint64(iovs))
		length := inst.memory.i32_load(uint64(iovs) + 4)
		captured = append(captured, inst.memory.read_string(uint64(ptr), uint64(length))...)
		inst.memory.i32_store(uint64(outPtr), length)
		return 0
	}
	inst = NewProg(Imports{"wasi_snapshot_preview1": {"fd_write": fdWrite}}, nil, nil, nil)
	inst.Exports["_start"].(func())() // random_get falls back to the bundled WASI
	fmt.Print(string(captured))
}
"#;

/// Instantiate an fs fixture with the scratch dir preopened at guest `/`, run
/// `_start`, and surface a `proc_exit` code as a trailing decimal line (via
/// rtExit). One glue serves both stdout-reporting and proc_exit fixtures: the
/// former return from `_start` normally, so nothing extra is printed. rt/exit
/// is always seeded for library-mode WASI output (see lib.rs), so `*rtExit` is
/// defined even for fixtures that never import proc_exit. Mirrors
/// `python_fs_glue`/`ruby_fs_glue`.
fn go_fs_glue(_case: &WasiCase, host: &Path) -> String {
    format!(
        "func main() {{\n\
         \tinst := NewProg(nil, nil, nil, map[string]string{{{:?}: {:?}}})\n\
         \tdefer func() {{\n\
         \t\tif r := recover(); r != nil {{\n\
         \t\t\tif e, ok := r.(*rtExit); ok {{\n\
         \t\t\t\tfmt.Println(e.code)\n\
         \t\t\t\treturn\n\
         \t\t\t}}\n\
         \t\t\tpanic(r)\n\
         \t\t}}\n\
         \t}}()\n\
         \tinst.Exports[\"_start\"].(func())()\n\
         }}\n",
        "/",
        host.to_string_lossy()
    )
}

standalone_e2e!(Go);
library_e2e!(Go, go_glue);
wasi_suite!(Go, Stdio);
wasi_suite!(Go, ArgsEnv);
wasi_suite!(Go, Fs, go_fs_glue);
apps_e2e!(Go);
gzip_e2e!(Go);

// ---------------------------------------------------------------------
// Filesystem-exercising app cases, mirrored from the Ruby/Python crates now
// that Go has WASI filesystem support (ADR-29 third milestone, adopting
// ADR-14's model). Their goldens are the same wasmtime-ground-truthed files
// the Ruby/Python cases use (crates/dewasm-cli/tests/apps_golden.rs), so the
// generated Go must produce byte-identical output.
//
// These are the heaviest cases in the suite (qjs/sqlite3 softfloat, rg's
// 22 MB wasm) and Go pays a cold `go build` on top, so they are gated behind
// DEWASM_APPS_ALL — the same deliberate perf opt-out `run_heavy_apps` uses.
// The task's gate runs them via
// `DEWASM_APPS_ALL=1 cargo test -p dewasm-backend-go --test e2e`.

/// Whether the heavy filesystem app cases should actually run.
fn apps_all() -> bool {
    std::env::var("DEWASM_APPS_ALL").is_ok()
}

/// Convert a cached app binary to a library-mode Go file (roomy stack for
/// SQLite's deep functions, `convert_on_big_stack`). `module_name` derives the
/// exported type name (`New<Type>`).
fn convert_app_library(wasm: &str, module_name: &str) -> String {
    let path = apps_cache_dir().join(format!("{wasm}.wasm"));
    assert!(
        path.exists(),
        "{wasm} not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&path).expect("read wasm");
    convert_on_big_stack(&GoBackend, &bytes, Mode::Library, module_name)
}

/// Compile `source` (generated declarations + appended `func main` glue) and
/// run it with `stdin`, returning the raw `Output`. Reuses the content-hash
/// build cache in `Go::run_bytes`.
fn run_go_io(source: &str, stdin: &str) -> Output {
    Go.run_bytes(source, &[], stdin.as_bytes())
}

/// Silently swallow a guest `proc_exit` (rtExit) so a clean guest exit is not
/// reported as a failure; re-panic anything else.
const CATCH_EXIT: &str = "\tdefer func() {\n\
    \t\tif r := recover(); r != nil {\n\
    \t\t\tif _, ok := r.(*rtExit); ok {\n\
    \t\t\t\treturn\n\
    \t\t\t}\n\
    \t\t\tpanic(r)\n\
    \t\t}\n\
    \t}()\n";

/// QuickJS with file I/O (Ruby's `qjs_file_io_ruby`): the `qjs:std` module
/// writes a file into a preopened scratch dir, reads it back, and prints it.
/// Asserts both the guest stdout golden and the host-side file content.
#[test]
fn qjs_file_io_go() {
    if !apps_all() {
        println!("qjs_file_io: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("go-qjs-file-io");
    std::fs::copy(
        apps_fixtures_dir().join("qjs_file_io.js"),
        work.join("qjs_file_io.js"),
    )
    .unwrap();
    let class = convert_app_library("qjs", "qjs");
    let glue = format!(
        "func main() {{\n\
         \tinst := NewQjs(nil, []string{{\"qjs\", \"/work/qjs_file_io.js\"}}, nil, map[string]string{{\"/work\": {:?}}})\n\
         {CATCH_EXIT}\
         \tinst.Exports[\"_start\"].(func())()\n\
         }}\n",
        work.to_string_lossy()
    );
    let output = run_go_io(&format!("{class}\n{glue}"), "");
    assert!(
        output.status.success(),
        "qjs_file_io under go failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/qjs_file_io.stdout"),
        "qjs_file_io: guest stdout differs from the wasmtime golden"
    );
    assert_eq!(
        std::fs::read_to_string(work.join("io_out.txt")).unwrap(),
        "hello from qjs file io\n",
        "qjs_file_io: the host file the guest wrote is wrong"
    );
}

/// QuickJS scripted REPL over piped stdin (Ruby's `qjs_repl_ruby`): the pinned
/// read-eval-print loop fixture exercises the stdin-read + evalScript path
/// byte-identically to wasmtime.
#[test]
fn qjs_repl_go() {
    if !apps_all() {
        println!("qjs_repl: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("go-qjs-repl");
    std::fs::copy(
        apps_fixtures_dir().join("qjs_repl.js"),
        work.join("qjs_repl.js"),
    )
    .unwrap();
    let class = convert_app_library("qjs", "qjs");
    let glue = format!(
        "func main() {{\n\
         \tinst := NewQjs(nil, []string{{\"qjs\", \"/work/qjs_repl.js\"}}, nil, map[string]string{{\"/work\": {:?}}})\n\
         {CATCH_EXIT}\
         \tinst.Exports[\"_start\"].(func())()\n\
         }}\n",
        work.to_string_lossy()
    );
    let output = run_go_io(
        &format!("{class}\n{glue}"),
        "1+2\n[3,1,2].sort()\nMath.max(4,9)\n\\q\n",
    );
    assert!(
        output.status.success(),
        "qjs_repl under go failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/qjs_repl.stdout"),
        "qjs_repl: guest stdout differs from the wasmtime golden"
    );
}

/// sqlite3 shell reading/writing a DB *file* (Ruby's
/// `sqlite3_shell_dbfile_ruby`): one invocation creates and populates
/// `/db/test.db`, a second reopens it and SELECTs. Asserts the second run's
/// stdout (golden) and that the DB file exists on the host with nonzero size.
#[test]
fn sqlite3_shell_dbfile_go() {
    if !apps_all() {
        println!("sqlite3_shell_dbfile: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let db = fresh_scratch_dir("go-sqlite3-dbfile");
    let class = convert_app_library("sqlite3-shell", "sqlite3_shell");
    let glue = format!(
        "func main() {{\n\
         \tinst := NewSqlite3Shell(nil, []string{{\"sqlite3\"}}, nil, map[string]string{{\"/db\": {:?}}})\n\
         {CATCH_EXIT}\
         \tinst.Exports[\"_start\"].(func())()\n\
         }}\n",
        db.to_string_lossy()
    );
    let program = format!("{class}\n{glue}");

    let create = run_go_io(
        &program,
        ".open /db/test.db\n\
         CREATE TABLE t(id INTEGER PRIMARY KEY, v TEXT);\n\
         INSERT INTO t(v) VALUES ('alpha'),('beta');\n\
         .exit\n",
    );
    assert!(
        create.status.success(),
        "sqlite3 dbfile create under go failed: {}\n{}",
        create.status,
        String::from_utf8_lossy(&create.stderr)
    );
    let db_file = db.join("test.db");
    assert!(
        db_file.metadata().map(|m| m.len() > 0).unwrap_or(false),
        "sqlite3 dbfile: the first run left no nonzero DB file"
    );

    let select = run_go_io(
        &program,
        ".open /db/test.db\nSELECT id, v FROM t ORDER BY id;\n.exit\n",
    );
    assert!(
        select.status.success(),
        "sqlite3 dbfile select under go failed: {}\n{}",
        select.status,
        String::from_utf8_lossy(&select.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&select.stdout),
        include_str!("../../../examples/apps/golden/sqlite3_shell_dbfile.stdout"),
        "sqlite3_shell_dbfile: reopened SELECT stdout differs from the wasmtime golden"
    );
}

/// Recursively copy `src` into `dst` (both directories), used to stage the
/// committed ripgrep fixture tree into a fresh scratch dir before the run.
fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).unwrap();
        }
    }
}

/// ripgrep searching a small fixture directory tree (Ruby's `rg_search_ruby`):
/// recursive directory walking (fd_readdir, path_open, path_filestat_get) over
/// a preopened tree. `--sort path` forces a deterministic order so the
/// `wasmtime --dir` golden is stable.
#[test]
fn rg_search_go() {
    if !apps_all() {
        println!("rg_search: heavy case skipped (DEWASM_APPS_ALL=1 to run)");
        return;
    }
    let work = fresh_scratch_dir("go-rg-search");
    copy_tree(&apps_fixtures_dir().join("rg"), &work);
    let class = convert_app_library("rg", "rg");
    let glue = format!(
        "func main() {{\n\
         \tinst := NewRg(nil, []string{{\"rg\", \"--sort\", \"path\", \"needle\", \"/work\"}}, nil, map[string]string{{\"/work\": {:?}}})\n\
         {CATCH_EXIT}\
         \tinst.Exports[\"_start\"].(func())()\n\
         }}\n",
        work.to_string_lossy()
    );
    let output = run_go_io(&format!("{class}\n{glue}"), "");
    assert!(
        output.status.success(),
        "rg_search under go failed: {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("../../../examples/apps/golden/rg_search.stdout"),
        "rg_search: guest stdout differs from the wasmtime golden"
    );
}
