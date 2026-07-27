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

use dewasm_backend::Backend;
use dewasm_backend_go::{find_go, GoBackend};
use dewasm_test_helper::{
    apps_e2e, fs_apps_e2e, gzip_e2e, library_e2e, run_command_bytes, standalone_e2e, wasi_suite,
    BackendUnderTest, LibraryCase, WasiCase,
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
    /// scenarios, rg's 22 MB wasm) live in the shared `FS_APP_CASES` table,
    /// gated behind `DEWASM_APPS_ALL` (`fs_apps_e2e!`).
    fn run_heavy_apps(&self) -> bool {
        true
    }

    /// A `func main` instantiating `class` (positional ctor
    /// `New<class>(imports, args, env, preopens)`), running `_start`, and
    /// recovering a clean guest `proc_exit` (`*rtExit`). Generalizes the
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
        let env_expr = if env.is_empty() {
            "nil".to_string()
        } else {
            let e = env
                .iter()
                .map(|(k, v)| format!("{k:?}: {v:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("map[string]string{{{e}}}")
        };
        let pres = preopens
            .iter()
            .map(|(guest, host)| format!("{guest:?}: {:?}", host.to_string_lossy()))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "func main() {{\n\
             \tinst := New{class}(nil, []string{{{argv}}}, {env_expr}, map[string]string{{{pres}}})\n\
             \tdefer func() {{\n\
             \t\tif r := recover(); r != nil {{\n\
             \t\t\tif _, ok := r.(*rtExit); ok {{\n\
             \t\t\t\treturn\n\
             \t\t\t}}\n\
             \t\t\tpanic(r)\n\
             \t\t}}\n\
             \t}}()\n\
             \tinst.Exports[\"_start\"].(func())()\n\
             }}\n"
        )
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
fs_apps_e2e!(Go);
