//! Go end-to-end suites (ADR-27): the shared library / WASI / apps case consts (`dewasm-test-helper`) wired up for the Go backend. Per the ADR-27 revision this file holds ONLY the [`BackendUnderTest`] impl, named glue string constants, and per-case macro invocations; glue is a plain `&str` argument at the callsite, and which macros this file invokes is the capability declaration (with a REASON comment at any non-invocation).
//!
//! Go is a *compiled* backend, so it overrides `BackendUnderTest::run` (ADR-27's hook) to compile-and-execute instead of interpreting: `go build` the generated file to a content-addressed cache binary (so identical sources — e.g. cowsay's args and stdin cases — build once), then run the binary directly. Running the binary (not `go run`) is required because `go run` does not propagate the guest exit code (it prints "exit status N" and exits 1); the WASI args/env case asserts an exact exit code. Go covers full WASI preview 1 incl. the filesystem (ADR-29).

use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Output};

use dewasm_backend::Backend;
use dewasm_backend_go::{find_go, GoBackend};
use dewasm_test_helper::BackendUnderTest;

pub struct Go;

impl BackendUnderTest for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &GoBackend
    }

    /// Compile `source` to a cache binary (keyed by content hash) and run it with `args`/`stdin`. A missing `go` toolchain is a loud failure (ADR-15); a build failure is surfaced as the build command's `Output` so the caller's `status.success()` assertion reports the compile error.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        match build_go(source) {
            // A build failure is surfaced as the build `Output` so the caller's `status.success()` assertion reports the compile error.
            Err(build) => build,
            Ok(bin) => dewasm_test_helper::run_command_bytes(Command::new(&bin).args(args), stdin),
        }
    }

    /// Build `source` to the cache binary and run it under a pty. A build failure fails loud (ADR-15): there is no `status` for the caller to inspect on the pty path, so panic with the compiler output.
    fn pty_command(&self, source: &str, args: &[&str]) -> dewasm_test_helper::PtyCommand {
        let bin = build_go(source).unwrap_or_else(|build| {
            panic!(
                "go build failed for the pty run:\n{}",
                String::from_utf8_lossy(&build.stderr)
            )
        });
        dewasm_test_helper::PtyCommand {
            program: bin,
            args: args.iter().map(|a| a.to_string()).collect(),
            cwd: None,
        }
    }

    /// Compose several `.wat` modules for the multi-module cases. `shared_runtime` mirrors the spec harness (ADR-29): each module is emitted as bare package-level declarations against one flat top-level runtime (`generate_program_with_units`), the referenced units are unioned and bundled once, and everything is assembled into a single `package main` file — an import block covering the runtime's needs plus `fmt` (the appended driver prints with it), the bundle, then the module decls. The driver (`func main`) is appended afterwards by the runner, so no main is emitted. `shared_runtime=false` (independent Embedded runtimes) is never exercised for Go: `embedded_coexist_e2e!` is not invoked because Go emits one flat top-level runtime shared by all modules (ADR-29).
    fn compose_modules(&self, modules: &[(&str, &str)], shared_runtime: bool) -> String {
        assert!(
            shared_runtime,
            "go multi-module: shared_runtime=false is excluded (one flat runtime, ADR-29)"
        );
        let mut units = BTreeSet::new();
        let mut decls = Vec::new();
        for (wat, name) in modules {
            let bytes =
                wat::parse_file(dewasm_test_helper::examples_dir().join(wat)).expect("parse wat");
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
        // Cover whatever the runtime bundle and module decls reference, then force `fmt` in — the appended driver prints with it and Go forbids a later `import` after other declarations.
        let mut imports = scan_imports(&format!("{bundle}\n{decls}"));
        if !imports.iter().any(|i| i == "fmt") {
            imports.push("fmt".to_string());
            imports.sort();
        }
        // `generate_program_with_units` emits spec-mode declarations whose recursion guard references a shared `rtStack` counter, declared in the spec harness's PREAMBLE; the multi-module composition must declare it too (ADR-29).
        format!(
            "package main\n\n{}\nvar rtStack int\n\n{bundle}\n\n{decls}\n",
            import_block(&imports)
        )
    }
}

/// The external packages an assembled multi-module program references (mirrors the spec harness's scanner). Only controlled fragments — the runtime bundle and generated declarations — are scanned, so no user string can inject a false import (ADR-29).
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

/// Compile `source` to a content-addressed cache binary (so identical sources build once) and return its path. `Err(Output)` carries the `go build` failure so a piped run can report it via `status.success()` while a pty run panics on it. A missing `go` toolchain is a loud failure (ADR-15).
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
        // Both the source and the binary get per-attempt unique names: two threads with the same hash (cowsay's args and stdin cases) may build concurrently, and a shared source path would let one truncate the file mid-read of the other's `go build` (issue #19). Only the final rename onto the cache key is shared, and that is atomic.
        let unique = format!(
            "{hash:016x}.{}.{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let src = cache.join(format!("src-{unique}.go"));
        std::fs::write(&src, source).unwrap();
        let tmp_bin = cache.join(format!("prog-{unique}"));
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
        let _ = std::fs::remove_file(&src);
    }

    Ok(bin)
}

// --------------------------------------------------------------------- Library-case glue. The appended `func main` carries no `import` — the generated file already imports `fmt`.

/// `add.wat`: call the exported functions and print each result.
const GO_ADD_GLUE: &str = r#"func main() {
	inst := NewAdd(nil, nil, nil, nil)
	fmt.Println(inst.Exports["add"].(func(uint32, uint32) uint32)(2, 3))
	fmt.Println(inst.Exports["add"].(func(uint32, uint32) uint32)(0xffffffff, 1))
	fmt.Println(inst.Exports["fib"].(func(uint32) uint32)(10))
}
"#;

/// The ADR-7 override/fallback glue: an explicit `fd_write` import wins, `random_get` falls back to the bundled WASI. Mirrors the other backends' override glues — intercept fd_write and print the actual bytes written.
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

/// The `wasi_stdio_capture` glue: Go's bundled WASI holds `*os.File` at fd 1 (`fd_write` type-asserts exactly that), so the embedder's sink is an `os.Pipe` write end swapped into `inst.wasi.fds` after construction — a real fd rather than an in-memory buffer, the same shape Perl's glue uses. Run `_start` (swallowing its clean `proc_exit`), close the writer so the reader sees EOF, and copy the captured bytes to the real stdout. The read loop is hand-rolled: library-mode Go imports `fmt` plus whatever the runtime bundle needs (`os`, here), and appended glue cannot add an `io` import of its own.
const GO_STDIO_CAPTURE_GLUE: &str = r#"func main() {
	r, w, err := os.Pipe()
	if err != nil {
		panic(err)
	}
	inst := NewProg(nil, nil, nil, nil)
	inst.wasi.fds[1] = w
	func() {
		defer func() {
			if rec := recover(); rec != nil {
				if _, ok := rec.(*rtExit); !ok {
					panic(rec)
				}
			}
		}()
		inst.Exports["_start"].(func())()
	}()
	w.Close()
	buf := make([]byte, 4096)
	for {
		n, e := r.Read(buf)
		if n > 0 {
			os.Stdout.Write(buf[:n])
		}
		if e != nil {
			break
		}
	}
}
"#;

// --------------------------------------------------------------------- WASI filesystem glue.

/// The shared filesystem template: preopen the scratch dir (`{host}`) at guest `{guest}` (always `/`), run `_start`, and surface a `proc_exit` code (via rtExit) as a trailing decimal line. rt/exit is always seeded for library-mode WASI output, so `*rtExit` is defined even for fixtures that never import proc_exit.
const GO_FS_GLUE: &str = r#"func main() {
	inst := NewProg(nil, nil, nil, map[string]string{"{guest}": "{host}"})
	defer func() {
		if r := recover(); r != nil {
			if e, ok := r.(*rtExit); ok {
				fmt.Println(e.code)
				return
			}
			panic(r)
		}
	}()
	inst.Exports["_start"].(func())()
}
"#;

/// The root-preopen containment probe: probe the WASI resolver directly (no guest run). A `"/" => "/"` preopen must resolve a relative path rather than reject every one; fd 3 is the sole preopen, errno 0 (wasiOk) means contained.
const GO_CONTAINMENT_GLUE: &str = r#"func main() {
	w := newWASI(nil, nil, map[string]string{"/": "/"})
	_, err := w.resolve_path(3, "etc", true)
	if err == 0 {
		fmt.Println("contained")
	} else {
		fmt.Println("rejected")
	}
}
"#;

// --------------------------------------------------------------------- Filesystem app glue: class/argv/env/preopen-guest-paths are literals; only the host scratch/cache dirs come through {scratch}/{cache}. One glue serves both stdout-reporting and proc_exit fixtures: the former return from `_start` normally, so nothing extra is printed.

const GO_QJS_FILE_IO_GLUE: &str = r#"func main() {
	inst := NewQjs(nil, []string{"qjs", "/work/qjs_file_io.js"}, nil, map[string]string{"/work": "{scratch}"})
	defer func() {
		if r := recover(); r != nil {
			if _, ok := r.(*rtExit); ok {
				return
			}
			panic(r)
		}
	}()
	inst.Exports["_start"].(func())()
}
"#;

const GO_SQLITE3_SHELL_GLUE: &str = r#"func main() {
	inst := NewSqlite3Shell(nil, []string{"sqlite3"}, nil, map[string]string{"/db": "{scratch}"})
	defer func() {
		if r := recover(); r != nil {
			if _, ok := r.(*rtExit); ok {
				return
			}
			panic(r)
		}
	}()
	inst.Exports["_start"].(func())()
}
"#;

const GO_RG_SEARCH_GLUE: &str = r#"func main() {
	inst := NewRg(nil, []string{"rg", "--sort", "path", "needle", "/work"}, nil, map[string]string{"/work": "{scratch}"})
	defer func() {
		if r := recover(); r != nil {
			if _, ok := r.(*rtExit); ok {
				return
			}
			panic(r)
		}
	}()
	inst.Exports["_start"].(func())()
}
"#;

const GO_CPYTHON_GLUE: &str = r#"func main() {
	inst := NewCpython(nil, []string{"python", "-c", "print('hello from cpython', 6 * 7)"}, []string{"PYTHONHOME=/", "PYTHONPATH=/lib/python3.14"}, map[string]string{"/lib": "{cache}/cpython-lib/lib"})
	defer func() {
		if r := recover(); r != nil {
			if _, ok := r.(*rtExit); ok {
				return
			}
			panic(r)
		}
	}()
	inst.Exports["_start"].(func())()
}
"#;

const GO_CRUBY_GLUE: &str = r#"func main() {
	inst := NewCruby(nil, []string{"ruby", "-e", "puts \"hello from cruby #{6*7}\""}, nil, map[string]string{"/usr": "{cache}/ruby-lib/usr"})
	defer func() {
		if r := recover(); r != nil {
			if _, ok := r.(*rtExit); ok {
				return
			}
			panic(r)
		}
	}()
	inst.Exports["_start"].(func())()
}
"#;

// --------------------------------------------------------------------- C-API drive glue (sqlite3): malloc / guest-memory pointer plumbing via the unexported `inst.memory` (`*Memory`). The appended `func main` carries no `import` (the library file already imports `fmt`). No wasmtime snapshot exists (the results live in guest memory), so each drive's output is pinned in the shared case const. Only the file-backed case uses {scratch}.

/// The sqlite3 C API driven in memory: `_initialize`, `sqlite3_malloc` + `*Memory` pointer plumbing, open/exec/prepare/step/column/finalize/close.
const GO_LIBSQLITE3_MEM: &str = r#"func main() {
	inst := NewLibsqlite3(nil, nil, nil, nil)
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

/// The sqlite3 C API against a file preopen: create+insert, close, reopen, select — the file lifecycle through the C API (same ADR-14 fs stack as the shell).
const GO_LIBSQLITE3_FILE: &str = r#"func main() {
	inst := NewLibsqlite3(nil, nil, nil, map[string]string{"/db": "{scratch}"})
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

/// Guest->host callback round trip: the committed `sqlite3-binding.wasm` exports `run_query`, which calls `sqlite3_exec` with a C callback forwarding each row to the *imported* `env.host_row`. The glue provides `host_row` via the ADR-7 import-provider map and collects the rows.
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

	inst := NewSqlite3Binding(Imports{"env": {"host_row": hostRow}}, nil, nil, nil)
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

/// libpcap BPF filter compilation: drive `compile_filter` on "tcp port 80" (DLT_EN10MB, snaplen 65535), then walk the serialized program `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]` in guest memory, printing each instruction as `code jt jf k`.
const GO_PCAP_COMPILE: &str = r#"func main() {
	inst := NewLibpcap(nil, nil, nil, nil)
	inst.Exports["_initialize"].(func())()
	mem := inst.memory
	malloc := inst.Exports["malloc"].(func(uint32) uint32)
	cstr := func(s string) uint32 {
		b := append([]byte(s), 0)
		p := malloc(uint32(len(b)))
		mem.init(uint64(p), b, 0, uint64(len(b)))
		return p
	}

	compile := inst.Exports["compile_filter"].(func(uint32, uint32, uint32) uint32)
	prog := compile(cstr("tcp port 80"), 1, 65535)
	if prog == 0 {
		panic("compile failed")
	}
	n := mem.i32_load(uint64(prog))
	for i := uint32(0); i < n; i++ {
		base := prog + 4 + i*8
		code := uint32(mem.data[base]) | uint32(mem.data[base+1])<<8
		jt := uint32(mem.data[base+2])
		jf := uint32(mem.data[base+3])
		k := mem.i32_load(uint64(base + 4))
		fmt.Printf("%d %d %d %d\n", code, jt, jf, k)
	}
	inst.Exports["free"].(func(uint32))(prog)
	fmt.Println("BPF-OK")
}
"#;

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}` and print the parse tree's S-expression (a malloc'd NUL-terminated C string) from guest memory.
const GO_TREESITTER_PARSE: &str = r#"func main() {
	inst := NewTreesitter(nil, nil, nil, nil)
	inst.Exports["_initialize"].(func())()
	mem := inst.memory
	malloc := inst.Exports["malloc"].(func(uint32) uint32)
	cstr := func(s string) uint32 {
		b := append([]byte(s), 0)
		p := malloc(uint32(len(b)))
		mem.init(uint64(p), b, 0, uint64(len(b)))
		return p
	}

	src := "{\"key\": [1, true, null]}"
	parse := inst.Exports["parse_source"].(func(uint32, uint32) uint32)
	r := parse(cstr(src), uint32(len(src)))
	if r == 0 {
		panic("parse failed")
	}
	end := r
	for mem.data[end] != 0 {
		end++
	}
	fmt.Println(string(mem.read_string(uint64(r), uint64(end-r))))
	inst.Exports["free"].(func(uint32))(r)
	fmt.Println("TS-OK")
}
"#;

/// zeroperl Perl-5.42 eval (issue #67): instantiate the reactor with a
/// zero-returning `env.call_host_function` import stub (only invoked when the
/// guest registers host callbacks — this program registers none) and a
/// `/dev/null` preopen (`zeroperl_init` returns 1 without it), then
/// `_initialize` → `zeroperl_init` → `malloc` + copy a Perl program into guest
/// memory → `zeroperl_eval` → `zeroperl_flush`. The program is a regex
/// capture and a `printf`, so its stdout is deterministic. The Perl source is a Go raw
/// string literal: its backslash escapes belong to Perl, not to Go.
const GO_ZEROPERL_EVAL: &str = r#"func main() {
	inst := NewZeroperl(
		Imports{"env": {"call_host_function": func(a, b, c uint32) uint32 { return 0 }}},
		nil, nil, map[string]string{"/dev/null": "/dev/null"})
	inst.Exports["_initialize"].(func())()
	if rc := inst.Exports["zeroperl_init"].(func() uint32)(); rc != 0 {
		panic(fmt.Sprintf("zeroperl_init rc=%d", rc))
	}
	mem := inst.memory

	prog := append([]byte(`my $s = "hello world 42";
if ($s =~ /(\w+)\s+(\w+)\s+(\d+)/) {
  printf("m=%s|%s|%d sum=%d\n", $1, $2, $3, $3 + 8);
}
`), 0)
	ptr := inst.Exports["malloc"].(func(uint32) uint32)(uint32(len(prog)))
	mem.init(uint64(ptr), prog, 0, uint64(len(prog)))
	inst.Exports["zeroperl_eval"].(func(uint32, uint32, uint32, uint32) uint32)(ptr, 0, 0, 0)
	inst.Exports["zeroperl_flush"].(func() uint32)()
}
"#;

/// ExifTool on zeroperl (issue #70): the flattened `exiftool` CLI driver
/// (`{cache}/exiftool-lib/exiftool`, preopened at `/work`) run on the same
/// `cache/zeroperl.wasm` reactor, whose SFS blob embeds the `Image::ExifTool`
/// module tree — so `use Image::ExifTool` resolves in-guest with no module
/// preopen. Instantiated like [`GO_ZEROPERL_EVAL`] (the `call_host_function`
/// stub + a `/dev/null` preopen), plus the staged image at `/img`. The Perl
/// driver snippet sets `@ARGV`/`$0` and `do`es the script; it first overrides
/// `CORE::GLOBAL::exit` to a `die` so ExifTool's terminal `exit` unwinds back
/// into `eval_pv` instead of tripping `proc_exit` — then `zeroperl_flush`
/// pushes ExifTool's buffered stdout out through fd 1. Only deterministic tags
/// are requested (`-S -Make -Model -DateTimeOriginal`).
const GO_EXIFTOOL: &str = r#"func main() {
	inst := NewZeroperl(
		Imports{"env": {"call_host_function": func(a, b, c uint32) uint32 { return 0 }}},
		nil, nil, map[string]string{
			"/dev/null": "/dev/null",
			"/work":     "{cache}/exiftool-lib",
			"/img":      "{scratch}",
		})
	inst.Exports["_initialize"].(func())()
	if rc := inst.Exports["zeroperl_init"].(func() uint32)(); rc != 0 {
		panic(fmt.Sprintf("zeroperl_init rc=%d", rc))
	}
	mem := inst.memory

	driver := append([]byte(`BEGIN { *CORE::GLOBAL::exit = sub (;$) { die "zeroperl_exit\n" }; }
@ARGV = ('-S', '-Make', '-Model', '-DateTimeOriginal', '/img/exif_fixture.jpg');
$0 = '/work/exiftool';
do '/work/exiftool';
`), 0)
	ptr := inst.Exports["malloc"].(func(uint32) uint32)(uint32(len(driver)))
	mem.init(uint64(ptr), driver, 0, uint64(len(driver)))
	inst.Exports["zeroperl_eval"].(func(uint32, uint32, uint32, uint32) uint32)(ptr, 0, 0, 0)
	inst.Exports["zeroperl_flush"].(func() uint32)()
}
"#;

// --------------------------------------------------------------------- Multi-module drive glue.

/// Driver for the shared-table case: instantiate `TableExp` (exports the table), then `TableImp` linked against it via the ADR-7 provider (`otherInst.Exports` as the module provider, as the spec harness's cross-module `register` path does), and print its `call0` result (`42`).
const GO_SHARED_TABLE_GLUE: &str = r#"func main() {
	a := NewTableExp(nil, nil, nil, nil)
	b := NewTableImp(Imports{"a": a.Exports}, nil, nil, nil)
	fmt.Println(b.Exports["call0"].(func() uint32)())
}
"#;

/// DOOM (ADR-53): deterministic drive (synthetic clock, no input) dumping the framebuffer as a P6 PPM matching the wasmtime snapshot. Library-mode Go imports `fmt` but not `os`, so the binary frame goes out via `fmt.Print(string(...))`. `{ticks}`/`{clock_step}` filled by the runner.
const GO_DOOM_FRAME_GLUE: &str = r#"func main() {
	var ms uint64
	var frameOff, frameW, frameH uint32
	imports := Imports{
		"console": {
			"onErrorMessage": func(o, l uint32) {},
			"onInfoMessage":  func(o, l uint32) {},
		},
		"gameSaving": {
			"sizeOfSaveGame": func(id uint32) uint32 { return 0 },
			"readSaveGame":   func(id, dst uint32) uint32 { return 0 },
			"writeSaveGame":  func(id, src, n uint32) uint32 { return n },
		},
		"runtimeControl": {
			"timeInMilliseconds": func() uint64 { ms += {clock_step}; return ms },
		},
		"ui": {
			"drawFrame": func(off uint32) { frameOff = off },
		},
		"loading": {
			"onGameInit": func(w, h uint32) { frameW, frameH = w, h },
			"wadSizes":   func(a, b uint32) {},
			"readWads":   func(a, b uint32) {},
		},
	}
	inst := NewDoom(imports, nil, nil, nil)
	inst.Exports["initGame"].(func())()
	tick := inst.Exports["tickGame"].(func())
	for i := 0; i < {ticks}; i++ {
		tick()
	}
	data := inst.memory.data
	header := fmt.Sprintf("P6\n%d %d\n255\n", frameW, frameH)
	out := make([]byte, 0, len(header)+int(frameW*frameH*3))
	out = append(out, header...)
	for i := uint32(0); i < frameW*frameH*4; i += 4 {
		out = append(out, data[frameOff+i+2], data[frameOff+i+1], data[frameOff+i])
	}
	fmt.Print(string(out))
}
"#;

/// NES (issue #114, mirrors the DOOM glue above): load the pinned ROM into
/// `allocRom`'s buffer, tick [`NES_FRAMES`] times with no input, compose the
/// frame from agnes's palette-index screen buffer and its palette (issue #117;
/// the `& 0x3f` mask is load-bearing) and dump it as a P6 PPM matching the
/// wasmtime snapshot.
///
/// Unlike every other backend's glue, this is a function rather than a static
/// `&str` const. Library-mode Go's generated file imports only `fmt` (plus
/// whatever the wasm module's own decompiled code happens to reference) —
/// Go requires every `import` to appear before all other top-level
/// declarations, so glue text *appended* after the generated class cannot
/// add its own `import "os"` to open `{rom}`'s host path (`go build` rejects
/// a trailing import with "imports must appear before other declarations").
/// With no WASI imports either (`nes.wasm` imports nothing at all) there is
/// no other route to `os` for a free host-file read. So the ROM's bytes are
/// read here, at test time, and embedded as a `\xHH`-escaped Go string
/// literal — a `[]byte(...)` conversion of an escaped string literal needs no
/// import, the same trick the Go backend's own codegen uses for a module's
/// data segments (`Rt.unhex`, `f0`'s `p.memory.init` call in a converted
/// `nes.wasm`).
fn go_nes_frame_glue() -> String {
    let rom = std::fs::read(dewasm_test_helper::alter_ego_rom_path())
        .expect("read alter_ego_rom_path — see examples/apps/scripts/nes.sh");
    let mut literal = String::with_capacity(rom.len() * 4 + 2);
    literal.push('"');
    for b in &rom {
        literal.push_str(&format!("\\x{b:02x}"));
    }
    literal.push('"');
    format!(
        r#"func main() {{
	rom := []byte({literal})
	inst := NewNes(nil, nil, nil, nil)
	inst.Exports["_initialize"].(func())()
	ptr := inst.Exports["allocRom"].(func(uint32) uint32)(uint32(len(rom)))
	inst.memory.init(uint64(ptr), rom, 0, uint64(len(rom)))
	ok := inst.Exports["initGame"].(func() uint32)()
	if ok != 1 {{
		panic(fmt.Sprintf("initGame failed: %d", ok))
	}}
	tick := inst.Exports["tickGame"].(func())
	for i := 0; i < {frames}; i++ {{
		tick()
	}}
	w := inst.Exports["frameWidth"].(func() uint32)()
	h := inst.Exports["frameHeight"].(func() uint32)()
	screenOff := inst.Exports["screenOffset"].(func() uint32)()
	paletteOff := inst.Exports["paletteOffset"].(func() uint32)()
	data := inst.memory.data
	header := fmt.Sprintf("P6\n%d %d\n255\n", w, h)
	out := make([]byte, 0, len(header)+int(w*h*3))
	out = append(out, header...)
	for i := uint32(0); i < w*h; i++ {{
		c := paletteOff + uint32(data[screenOff+i]&0x3f)*4
		out = append(out, data[c], data[c+1], data[c+2])
	}}
	fmt.Print(string(out))
}}
"#,
        literal = literal,
        frames = dewasm_test_helper::NES_FRAMES,
    )
}

// --------------------------------------------------------------------- Suite wiring (ADR-27): each per-case macro invocation declares participation.

dewasm_test_helper::library_add_e2e!(Go, GO_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Go, GO_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Go, GO_STDIO_CAPTURE_GLUE);
// custom_wasi_provider_e2e! / partial_override_e2e!: not invoked — Go's bundled WASI is eagerly constructed in the ctor and there is no provider-object import form (ADR-29), so the lazy-construction observable cannot hold.

dewasm_test_helper::wasi_suite!(Go, Stdio);
dewasm_test_helper::wasi_suite!(Go, ArgsEnv);
dewasm_test_helper::wasi_suite!(Go, Poll);
dewasm_test_helper::wasi_suite!(Go, Fs, GO_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Go, GO_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Go);
// Goroutine stacks start small and grow dynamically, so 5000 guest frames need no entrypoint mitigation.
dewasm_test_helper::deep_recursion_e2e!(Go);

dewasm_test_helper::cowsay_args_e2e!(Go);
dewasm_test_helper::cowsay_stdin_e2e!(Go);
// The `ultra` cases (ADR-48) are the giant-generated-program `go build`s that individually ran ~1 min+ and collectively exhausted a 4-core CI runner's memory (SIGTERM, #23): kept out of CI's `slow_test` run, run only under `--features ultra_slow_test` or `-- --include-ignored`. The other giant builds (`qjs_repl_pty`, `sqlite3_shell_dbfile`, `pcap_compile`, `treesitter_parse`) stayed under the ~1-min bar and remain at `slow`.
dewasm_test_helper::qjs_eval_e2e!(Go, ultra);
dewasm_test_helper::sqlite3_shell_e2e!(Go, ultra);
dewasm_test_helper::gzip_e2e!(Go);

dewasm_test_helper::qjs_file_io_e2e!(Go, GO_QJS_FILE_IO_GLUE, ultra);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Go, GO_SQLITE3_SHELL_GLUE);
dewasm_test_helper::rg_search_e2e!(Go, GO_RG_SEARCH_GLUE, ultra);
dewasm_test_helper::cpython_hello_e2e!(Go, GO_CPYTHON_GLUE, ultra);
// Ultra-slow category (ADR-48): the ~35 MB CRuby wasm's ~242 MB Go source takes `go build` well past the ~5-minute ADR-24 practicality bar and CI's own budget (convert + build + run measured ~14m54s locally) — wall time is the cost, not feasibility. The 49 MB wasi-vfs-packed variant (ADR-61) is the same interpreter plus embedded stdlib (~14m13s), so it inherits the category.
dewasm_test_helper::cruby_hello_e2e!(Go, GO_CRUBY_GLUE, ultra);
dewasm_test_helper::cruby_packed_hello_e2e!(Go, ultra);
dewasm_test_helper::qjs_repl_pty_e2e!(Go);

dewasm_test_helper::libsqlite3_c_api_e2e!(Go, GO_LIBSQLITE3_MEM, ultra);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Go, GO_LIBSQLITE3_FILE, ultra);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Go, GO_SQLITE3_CALLBACK, ultra);
dewasm_test_helper::pcap_compile_e2e!(Go, GO_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Go, GO_TREESITTER_PARSE);
// The zeroperl reactor cases (issue #139) join the `ultra` giants above: the 25 MB reactor becomes a ~90 MB Go program whose `go build` dominates the run — measured 71 s (zeroperl_eval) and 92 s (exiftool_extract), well past the ~1-min bar.
dewasm_test_helper::zeroperl_eval_e2e!(Go, GO_ZEROPERL_EVAL, ultra);
dewasm_test_helper::exiftool_extract_e2e!(Go, GO_EXIFTOOL, ultra);

dewasm_test_helper::doom_frame_e2e!(Go, GO_DOOM_FRAME_GLUE);
dewasm_test_helper::nes_frame_e2e!(Go, &go_nes_frame_glue());

dewasm_test_helper::shared_table_e2e!(Go, GO_SHARED_TABLE_GLUE);
// embedded_coexist_e2e!: not invoked — a single flat top-level runtime is shared by all modules (ADR-29); two independent runtimes cannot coexist.
