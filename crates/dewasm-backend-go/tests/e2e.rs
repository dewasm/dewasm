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
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_go::{find_go, GoBackend};
use dewasm_test_helper::{
    apps_e2e, capi_apps_e2e, examples_dir, fs_apps_e2e, gzip_e2e, library_e2e, multi_module_e2e,
    qjs_repl_pty_e2e, run_command_bytes, standalone_e2e, wasi_suite, BackendUnderTest, CApiCase,
    LibraryCase, MultiModuleCase, PtyCommand, WasiCase,
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
        match build_go(source) {
            // A build failure is surfaced as the build `Output` so the caller's
            // `status.success()` assertion reports the compile error.
            Err(build) => build,
            Ok(bin) => run_command_bytes(Command::new(&bin).args(args), stdin),
        }
    }

    /// Build `source` to the cache binary and run it under a pty. A build
    /// failure fails loud (ADR-15): there is no `status` for the caller to
    /// inspect on the pty path, so panic with the compiler output.
    fn pty_command(&self, source: &str, args: &[&str]) -> PtyCommand {
        let bin = build_go(source).unwrap_or_else(|build| {
            panic!(
                "go build failed for the pty run:\n{}",
                String::from_utf8_lossy(&build.stderr)
            )
        });
        PtyCommand {
            program: bin,
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: None,
        }
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
        // The ctor's `env` is a `[]string` of `KEY=VALUE` pairs (the shape
        // `os.Environ()` produces and `newWASI` consumes), not a map.
        let env_expr = if env.is_empty() {
            "nil".to_string()
        } else {
            let e = env
                .iter()
                .map(|(k, v)| format!("{:?}", format!("{k}={v}")))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[]string{{{e}}}")
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

    /// Compose several `.wat` modules for the multi-module cases. `shared_runtime`
    /// mirrors the spec harness (ADR-29): each module is emitted as bare
    /// package-level declarations against one flat top-level runtime
    /// (`generate_program_with_units`), the referenced units are unioned and
    /// bundled once, and everything is assembled into a single `package main`
    /// file — an import block covering the runtime's needs plus `fmt` (the
    /// appended driver prints with it), the bundle, then the module decls. The
    /// driver (`func main`) is appended afterwards by the runner, so no main is
    /// emitted. `shared_runtime=false` (independent Embedded runtimes) is never
    /// exercised for Go: `embedded_runtimes_coexist` is excluded because Go emits
    /// one flat top-level runtime shared by all modules (ADR-29).
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        assert!(
            shared_runtime,
            "go multi-module: shared_runtime=false is excluded (one flat runtime, ADR-29)"
        );
        let mut units = BTreeSet::new();
        let mut decls = Vec::new();
        for (wat, name) in modules {
            let bytes = wat::parse_file(examples_dir().join(wat)).expect("parse wat");
            let module = dewasm_core::build_module(&bytes).expect("build IR");
            let (src, u) =
                dewasm_backend_go::generate_program_with_units(&module, name).expect("generate");
            units.extend(u);
            decls.push(src);
        }
        let bundle = dewasm_backend_go::bundler()
            .bundle(&units, 0)
            .expect("bundle runtime");
        let decls = decls.join("\n");
        // Cover whatever the runtime bundle and module decls reference, then
        // force `fmt` in — the appended driver prints with it and Go forbids a
        // later `import` after other declarations.
        let mut imports = scan_imports(&format!("{bundle}\n{decls}"));
        if !imports.iter().any(|i| i == "fmt") {
            imports.push("fmt".to_string());
            imports.sort();
        }
        // `generate_program_with_units` emits spec-mode declarations whose
        // recursion guard references a shared `rtStack` counter, declared in the
        // spec harness's PREAMBLE; the multi-module composition must declare it
        // too (ADR-29).
        format!(
            "package main\n\n{}\nvar rtStack int\n\n{bundle}\n\n{decls}\n",
            import_block(&imports)
        )
    }
}

/// The external packages an assembled multi-module program references (mirrors
/// the spec harness's scanner). Only controlled fragments — the runtime bundle
/// and generated declarations — are scanned, so no user string can inject a
/// false import (ADR-29).
fn scan_imports(text: &str) -> Vec<String> {
    let candidates = [
        ("fmt.", "fmt"),
        ("math.", "math"),
        ("bits.", "math/bits"),
        ("binary.", "encoding/binary"),
        ("rand.", "crypto/rand"),
        ("strings.", "strings"),
    ];
    let mut set: BTreeSet<&'static str> = BTreeSet::new();
    for (sel, path) in candidates {
        if text.contains(sel) {
            set.insert(path);
        }
    }
    set.into_iter().map(|s| s.to_string()).collect()
}

fn import_block(imports: &[String]) -> String {
    if imports.is_empty() {
        return String::new();
    }
    let mut out = String::from("import (\n");
    for path in imports {
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\t{path:?}\n"));
    }
    out.push_str(")\n");
    out
}

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Compile `source` to a content-addressed cache binary (so identical sources
/// build once) and return its path. `Err(Output)` carries the `go build`
/// failure so a piped run can report it via `status.success()` while a pty run
/// panics on it. A missing `go` toolchain is a loud failure (ADR-15).
fn build_go(source: &str) -> Result<PathBuf, Output> {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO) — see docs/testing.md");

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let hash = hasher.finish();

    let cache = std::env::temp_dir().join("dewasm-go-cache");
    std::fs::create_dir_all(&cache).unwrap();
    let bin = cache.join(format!("prog-{hash:016x}"));

    if !bin.exists() {
        let src = cache.join(format!("src-{hash:016x}.go"));
        std::fs::write(&src, source).unwrap();
        // Build to a unique path, then rename onto the cache key so concurrent
        // test threads never hand out a half-written binary.
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
            return Err(build);
        }
        let _ = std::fs::rename(&tmp_bin, &bin);
    }

    Ok(bin)
}

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
fn go_fs_glue(case: &WasiCase, host: &Path) -> String {
    if case.name == "fs_root_preopen_containment" {
        // Probe the WASI resolver directly (no guest run): a `"/" => "/"`
        // preopen must resolve a relative path rather than reject every one
        // (the containment prefix must not degenerate to "//"). fd 3 is the
        // sole preopen; errno 0 (wasiOk) means the path stayed contained.
        return "func main() {\n\
                \tw := newWASI(nil, nil, map[string]string{\"/\": \"/\"})\n\
                \t_, err := w.resolve_path(3, \"etc\", true)\n\
                \tif err == 0 {\n\
                \t\tfmt.Println(\"contained\")\n\
                \t} else {\n\
                \t\tfmt.Println(\"rejected\")\n\
                \t}\n\
                }\n"
        .to_string();
    }
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

// ---------------------------------------------------------------------
// C-API drive glue (sqlite3): malloc / guest-memory pointer plumbing via the
// unexported `inst.memory` (`*Memory`). The appended `func main` carries no
// `import` (the library file already imports `fmt`), so the glue uses only
// `fmt` + builtins — C strings are scanned/written through `inst.memory.data`
// / `read_string` / `init`. No wasmtime golden exists (the results live in
// guest memory), so each drive's output is pinned in the shared table.

fn go_capi_glue(case: &CApiCase, scratch: &Path) -> String {
    match case.name {
        "libsqlite3_c_api" => GO_LIBSQLITE3_MEM.replace("__CLASS__", case.class),
        "sqlite3_file_c_api" => GO_LIBSQLITE3_FILE
            .replace("__CLASS__", case.class)
            .replace("__DB__", &format!("{:?}", scratch.to_string_lossy())),
        "sqlite3_callback_binding" => GO_SQLITE3_CALLBACK.replace("__CLASS__", case.class),
        other => panic!("{other}: no go capi glue"),
    }
}

/// The sqlite3 C API driven in memory: `_initialize`, `sqlite3_malloc` +
/// `*Memory` pointer plumbing, open/exec/prepare/step/column/finalize/close.
const GO_LIBSQLITE3_MEM: &str = r#"func main() {
	inst := New__CLASS__(nil, nil, nil, nil)
	inst.Exports["_initialize"].(func())()
	mem := inst.memory
	malloc := inst.Exports["sqlite3_malloc"].(func(uint32) uint32)

	readCstr := func(ptr uint32) string {
		if ptr == 0 {
			return ""
		}
		end := ptr
		for mem.data[end] != 0 {
			end++
		}
		return string(mem.read_string(uint64(ptr), uint64(end-ptr)))
	}
	cstr := func(s string) uint32 {
		b := append([]byte(s), 0)
		p := malloc(uint32(len(b)))
		mem.init(uint64(p), b, 0, uint64(len(b)))
		return p
	}

	fmt.Printf("version: %s\n", readCstr(inst.Exports["sqlite3_libversion"].(func() uint32)()))

	ppDb := malloc(4)
	rc := inst.Exports["sqlite3_open"].(func(uint32, uint32) uint32)(cstr(":memory:"), ppDb)
	if rc != 0 {
		panic(fmt.Sprintf("open rc=%d", rc))
	}
	db := mem.i32_load(uint64(ppDb))

	exec := inst.Exports["sqlite3_exec"].(func(uint32, uint32, uint32, uint32, uint32) uint32)
	errmsg := inst.Exports["sqlite3_errmsg"].(func(uint32) uint32)
	rc = exec(db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0)
	if rc != 0 {
		panic(fmt.Sprintf("exec rc=%d: %s", rc, readCstr(errmsg(db))))
	}

	ppStmt := malloc(4)
	rc = inst.Exports["sqlite3_prepare_v2"].(func(uint32, uint32, uint32, uint32, uint32) uint32)(
		db, cstr("select a*10, b from t order by a desc"), 0xffffffff, ppStmt, 0)
	if rc != 0 {
		panic(fmt.Sprintf("prepare rc=%d", rc))
	}
	stmt := mem.i32_load(uint64(ppStmt))

	step := inst.Exports["sqlite3_step"].(func(uint32) uint32)
	columnCount := inst.Exports["sqlite3_column_count"].(func(uint32) uint32)
	columnText := inst.Exports["sqlite3_column_text"].(func(uint32, uint32) uint32)
	for step(stmt) == 100 { // SQLITE_ROW
		n := columnCount(stmt)
		row := ""
		for i := uint32(0); i < n; i++ {
			if i > 0 {
				row += "|"
			}
			row += readCstr(columnText(stmt, i))
		}
		fmt.Println(row)
	}
	inst.Exports["sqlite3_finalize"].(func(uint32) uint32)(stmt)
	inst.Exports["sqlite3_close"].(func(uint32) uint32)(db)
	fmt.Println("C-API-OK")
}
"#;

/// The sqlite3 C API against a file preopen: create+insert, close, reopen,
/// select — the file lifecycle through the C API (same ADR-14 fs stack as the
/// shell).
const GO_LIBSQLITE3_FILE: &str = r#"func main() {
	inst := New__CLASS__(nil, nil, nil, map[string]string{"/db": __DB__})
	inst.Exports["_initialize"].(func())()
	mem := inst.memory
	malloc := inst.Exports["sqlite3_malloc"].(func(uint32) uint32)

	readCstr := func(ptr uint32) string {
		if ptr == 0 {
			return ""
		}
		end := ptr
		for mem.data[end] != 0 {
			end++
		}
		return string(mem.read_string(uint64(ptr), uint64(end-ptr)))
	}
	cstr := func(s string) uint32 {
		b := append([]byte(s), 0)
		p := malloc(uint32(len(b)))
		mem.init(uint64(p), b, 0, uint64(len(b)))
		return p
	}
	open := inst.Exports["sqlite3_open"].(func(uint32, uint32) uint32)
	openDb := func(path string) uint32 {
		pp := malloc(4)
		rc := open(cstr(path), pp)
		if rc != 0 {
			panic(fmt.Sprintf("open rc=%d", rc))
		}
		return mem.i32_load(uint64(pp))
	}
	exec := inst.Exports["sqlite3_exec"].(func(uint32, uint32, uint32, uint32, uint32) uint32)
	errmsg := inst.Exports["sqlite3_errmsg"].(func(uint32) uint32)
	closeDb := inst.Exports["sqlite3_close"].(func(uint32) uint32)

	// create + insert, then close so the file is fully flushed
	db := openDb("/db/data.db")
	rc := exec(db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y');"), 0, 0, 0)
	if rc != 0 {
		panic(fmt.Sprintf("exec rc=%d: %s", rc, readCstr(errmsg(db))))
	}
	closeDb(db)

	// reopen the same file and read it back
	db = openDb("/db/data.db")
	ppStmt := malloc(4)
	rc = inst.Exports["sqlite3_prepare_v2"].(func(uint32, uint32, uint32, uint32, uint32) uint32)(
		db, cstr("select a*10, b from t order by a"), 0xffffffff, ppStmt, 0)
	if rc != 0 {
		panic(fmt.Sprintf("prepare rc=%d", rc))
	}
	stmt := mem.i32_load(uint64(ppStmt))
	step := inst.Exports["sqlite3_step"].(func(uint32) uint32)
	columnCount := inst.Exports["sqlite3_column_count"].(func(uint32) uint32)
	columnText := inst.Exports["sqlite3_column_text"].(func(uint32, uint32) uint32)
	for step(stmt) == 100 { // SQLITE_ROW
		n := columnCount(stmt)
		row := ""
		for i := uint32(0); i < n; i++ {
			if i > 0 {
				row += "|"
			}
			row += readCstr(columnText(stmt, i))
		}
		fmt.Println(row)
	}
	inst.Exports["sqlite3_finalize"].(func(uint32) uint32)(stmt)
	closeDb(db)
	fmt.Println("FILE-OK")
}
"#;

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports
/// `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row
/// to the *imported* `env.host_row`. The glue provides `host_row` via the ADR-7
/// import-provider map and collects the rows.
const GO_SQLITE3_CALLBACK: &str = r#"func main() {
	var rows []string
	var mem *Memory
	// host_row is imported as `void host_row(int argc, char **argv)` — no result.
	hostRow := func(argc, argvPtr uint32) {
		row := ""
		for i := uint32(0); i < argc; i++ {
			p := mem.i32_load(uint64(argvPtr + i*4))
			if i > 0 {
				row += "|"
			}
			if p != 0 {
				end := p
				for mem.data[end] != 0 {
					end++
				}
				row += string(mem.read_string(uint64(p), uint64(end-p)))
			}
		}
		rows = append(rows, row)
	}

	inst := New__CLASS__(Imports{"env": {"host_row": hostRow}}, nil, nil, nil)
	inst.Exports["_initialize"].(func())()
	mem = inst.memory
	malloc := inst.Exports["sqlite3_malloc"].(func(uint32) uint32)

	readCstr := func(ptr uint32) string {
		if ptr == 0 {
			return ""
		}
		end := ptr
		for mem.data[end] != 0 {
			end++
		}
		return string(mem.read_string(uint64(ptr), uint64(end-ptr)))
	}
	cstr := func(s string) uint32 {
		b := append([]byte(s), 0)
		p := malloc(uint32(len(b)))
		mem.init(uint64(p), b, 0, uint64(len(b)))
		return p
	}

	ppDb := malloc(4)
	rc := inst.Exports["sqlite3_open"].(func(uint32, uint32) uint32)(cstr(":memory:"), ppDb)
	if rc != 0 {
		panic(fmt.Sprintf("open rc=%d", rc))
	}
	db := mem.i32_load(uint64(ppDb))

	exec := inst.Exports["sqlite3_exec"].(func(uint32, uint32, uint32, uint32, uint32) uint32)
	errmsg := inst.Exports["sqlite3_errmsg"].(func(uint32) uint32)
	rc = exec(db, cstr("create table t(a,b); insert into t values (1,'x'),(2,'y'),(3,'z');"), 0, 0, 0)
	if rc != 0 {
		panic(fmt.Sprintf("exec rc=%d: %s", rc, readCstr(errmsg(db))))
	}

	// guest -> host: run_query calls back into env.host_row once per row
	rc = inst.Exports["run_query"].(func(uint32, uint32) uint32)(
		db, cstr("select a, b from t where a >= 2 order by a"))
	if rc != 0 {
		panic(fmt.Sprintf("run_query rc=%d", rc))
	}
	inst.Exports["sqlite3_close"].(func(uint32) uint32)(db)

	for _, r := range rows {
		fmt.Printf("row: %s\n", r)
	}
	fmt.Println("CALLBACK-OK")
}
"#;

// ---------------------------------------------------------------------
// Multi-module drive glue.

/// Driver for the shared-table case: instantiate `TableExp` (exports the table),
/// then `TableImp` linked against it via the ADR-7 provider (`otherInst.Exports`
/// as the module provider, as the spec harness's cross-module `register` path
/// does), and print its `call0` result (`42`).
fn go_multi_module_glue(case: &MultiModuleCase) -> &'static str {
    match case.name {
        "shared_table_call_indirect" => {
            "func main() {\n\
             \ta := NewTableExp(nil, nil, nil, nil)\n\
             \tb := NewTableImp(Imports{\"a\": a.Exports}, nil, nil, nil)\n\
             \tfmt.Println(b.Exports[\"call0\"].(func() uint32)())\n\
             }\n"
        }
        other => panic!("{other}: no go multi-module glue"),
    }
}

standalone_e2e!(Go);
library_e2e!(Go, go_glue);
wasi_suite!(Go, Stdio);
wasi_suite!(Go, ArgsEnv);
wasi_suite!(Go, Poll);
wasi_suite!(Go, Fs, go_fs_glue);
apps_e2e!(Go);
gzip_e2e!(Go);
fs_apps_e2e!(Go);
qjs_repl_pty_e2e!(Go);
capi_apps_e2e!(Go, go_capi_glue);
multi_module_e2e!(Go, go_multi_module_glue);
