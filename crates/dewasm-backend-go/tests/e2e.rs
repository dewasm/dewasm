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
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_go::{find_go, GoBackend};
use dewasm_test_helper::{
    apps_e2e, library_e2e, run_command_bytes, standalone_e2e, wasi_suite, BackendUnderTest,
    LibraryCase,
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

    /// The heavy apps (QuickJS, SQLite) need the WASI filesystem, which is a
    /// later milestone; keep them off (cowsay is the only app case that runs).
    fn run_heavy_apps(&self) -> bool {
        false
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
             \tinst := NewAdd(nil, nil, nil)\n\
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
	inst = NewProg(Imports{"wasi_snapshot_preview1": {"fd_write": fdWrite}}, nil, nil)
	inst.Exports["_start"].(func())() // random_get falls back to the bundled WASI
	fmt.Print(string(captured))
}
"#;

standalone_e2e!(Go);
library_e2e!(Go, go_glue);
wasi_suite!(Go, Stdio);
wasi_suite!(Go, ArgsEnv);
apps_e2e!(Go);
