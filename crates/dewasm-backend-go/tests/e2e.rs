//! Go end-to-end suites.
//! They connect the shared library / WASI / apps case constants to the Go backend.
//! Those constants live in `dewasm-test-helper`.
//! This file holds ONLY three things:
//!
//! - the [`BackendUnderTest`] implementation;
//! - named glue string constants;
//! - per-case macro calls.
//!
//! Glue is a plain `&str` argument at the callsite.
//! Which macros this file calls is the capability declaration.
//! Each macro it leaves uncalled carries a REASON comment.
//!
//! Go is a *compiled* backend, so it overrides `BackendUnderTest::run` to compile-and-execute.
//! It does not interpret.
//! It `go build`s the generated source to a content-addressed cache binary, then runs the binary.
//! Identical sources (for example, `cowsay_args_e2e!` and `cowsay_stdin_e2e!`) then build once.
//! Running the binary (not `go run`) is required: `go run` does not pass on the guest exit code.
//! It prints "exit status N" and exits 1.
//! The WASI arguments and environment case asserts an exact exit code.
//! Go covers full WASI p1, including the file system.
//!
//! Library-mode output is `package <module name>`, not `package main`.
//! So a library case is not a runnable file on its own.
//! [`common::build_go`] wraps it in a temporary Go module.
//! That module's `main` imports the package and calls the glue's `RunTest`.
//! That is why every library glue below defines `func RunTest()`.
//! It used to define `func main()` there.
//! The multi-module glue keeps `func main`.
//! Its driver *is* the `package main` file of a module `compose_modules` lays out on disk.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Output};

use dewasm_backend::{Backend, Mode};
use dewasm_backend_go::GoBackend;
use dewasm_test_helper::BackendUnderTest;

mod common;

use common::{build_go, build_go_dir, package_clause};

pub struct Go;

impl BackendUnderTest for Go {
    fn name(&self) -> &'static str {
        "go"
    }

    fn backend(&self) -> &'static (dyn Backend + Sync) {
        &GoBackend
    }

    /// Compile `source` to a cache binary (keyed by content hash) and run it with `args`/`stdin`.
    /// A missing `go` toolchain is a loud failure.
    /// A build failure is surfaced as the build command's `Output`.
    /// The caller's `status.success()` assertion then reports the compile error.
    fn run_bytes(&self, source: &str, args: &[&str], stdin: &[u8]) -> Output {
        match build_go(source) {
            Err(build) => build,
            Ok(bin) => dewasm_test_helper::run_command_bytes(Command::new(&bin).args(args), stdin),
        }
    }

    /// Build `source` to the cache binary and run it under a pseudo-terminal.
    /// A build failure fails loud: the pseudo-terminal path gives the caller no `status` to check.
    /// So it panics with the compiler output.
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

    /// Lay out a multi-module case as a temporary Go module in `dir`.
    /// Return the driver file's `package main` clause.
    /// Everything else the driver needs, its imports included, comes from the case glue.
    /// That is because Go rejects an unused import, and only the glue knows what it uses.
    ///
    /// `shared_runtime` mirrors the specification harness.
    /// Each module is emitted as package-level declarations alone (`generate_program_with_units`).
    /// They are emitted against one flat top-level runtime.
    /// The referenced units are unioned and bundled once.
    /// All of it goes into a single `modules.go` of the *same* `package main` as the driver.
    /// A shared runtime cannot be split into a package per module.
    /// The runtime types would then be distinct types per package.
    /// A table value crossing modules would then no longer pass type checking.
    /// `shared_runtime=false` is the opposite layout and needs no trick at all.
    /// Each module is a library conversion, which since #155 declares `package <module name>`.
    /// So `compose_modules` writes them into `alpha/` and `beta/` beside the driver.
    /// That gives two artifacts with their own runtime and their own trap type.
    /// They share no identifier.
    fn compose_modules(
        &self,
        dir: &Path,
        modules: &[(&str, &str)],
        shared_runtime: bool,
    ) -> String {
        std::fs::write(dir.join("go.mod"), "module dewasmtest\n\ngo 1.21\n").unwrap();
        if shared_runtime {
            let mut units = BTreeSet::new();
            let mut decls = Vec::new();
            for (wat, name) in modules {
                let bytes = wat::parse_file(dewasm_test_helper::examples_dir().join(wat))
                    .expect("parse wat");
                let module = dewasm_core::build_module(&bytes).expect("build IR");
                let (src, u) = dewasm_backend_go::generate_program_with_units(&module, name)
                    .expect("generate");
                units.extend(u);
                decls.push(src);
            }
            let bundle = dewasm_backend_go::bundler()
                .bundle(&units, 0)
                .expect("bundle runtime");
            let decls = decls.join("\n");
            let imports = scan_imports(&format!("{bundle}\n{decls}"));
            // `generate_program_with_units` emits declarations for the specification harness.
            // Their recursion guard references a shared `rtStack` counter.
            // The specification harness's PREAMBLE declares it.
            // The multi-module composition must declare it too.
            std::fs::write(
                dir.join("modules.go"),
                format!(
                    "package main\n\n{}\nvar rtStack int\n\n{bundle}\n\n{decls}\n",
                    import_block(&imports)
                ),
            )
            .unwrap();
        } else {
            for (wat, name) in modules {
                let src = dewasm_test_helper::convert(
                    &GoBackend,
                    &dewasm_test_helper::examples_dir().join(wat),
                    Mode::Library,
                    name,
                );
                let package = package_clause(&src).to_string();
                let pkg_dir = dir.join(&package);
                std::fs::create_dir_all(&pkg_dir).unwrap();
                std::fs::write(pkg_dir.join(format!("{package}.go")), &src).unwrap();
            }
        }
        "package main".to_string()
    }

    /// Write the multi-module `driver` as the module's `main.go`.
    /// Then build the module rooted at `dir`, and run the binary.
    /// A build failure is surfaced as the `go build` `Output`, like [`Self::run_bytes`].
    /// The caller's `status.success()` assertion then reports the compile error.
    fn run_in_dir(&self, dir: &Path, driver: &str) -> Output {
        std::fs::write(dir.join("main.go"), driver).unwrap();
        match build_go_dir(dir) {
            Err(build) => build,
            Ok(bin) => dewasm_test_helper::run_command(&mut Command::new(&bin), ""),
        }
    }
}

/// The external packages an assembled multi-module program references.
/// They are found by the backend's own boundary-matching rule.
/// That rule is [`dewasm_backend_go::selector_used`].
/// This mirrors the specification harness's scanner.
/// Only controlled fragments (the runtime bundle and generated declarations) are scanned.
/// So no user string can inject a false import.
/// The candidate list stays local: it is what *this* program can reference.
/// Importing more than that is a Go compile error.
fn scan_imports(text: &str) -> Vec<String> {
    let candidates = [
        ("fmt.", "fmt"),
        ("math.", "math"),
        ("bits.", "math/bits"),
        ("binary.", "encoding/binary"),
        ("rand.", "crypto/rand"),
        ("strings.", "strings"),
        ("runtime.", "runtime"),
        ("unsafe.", "unsafe"),
    ];
    let mut set: BTreeSet<&'static str> = BTreeSet::new();
    for (sel, path) in candidates {
        if dewasm_backend_go::selector_used(text, sel) {
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
        let _ = writeln!(out, "\t{path:?}");
    }
    out.push_str(")\n");
    out
}

// --------------------------------------------------------------------- Library-case glue.
// Each defines `RunTest`, the entry point the generated `main.go` of the temp module calls.
// The layout is in `common::build_go`.
// Each carries no `import`: the generated file already imports `fmt`.
// Go rejects an `import` after other declarations anyway.

/// `add.wat`: call the exported functions and print each result.
const GO_ADD_GLUE: &str = r#"func RunTest() {
	inst := NewAdd(nil, nil, nil, nil)
	fmt.Println(inst.Exports["add"].(func(uint32, uint32) uint32)(2, 3))
	fmt.Println(inst.Exports["add"].(func(uint32, uint32) uint32)(0xffffffff, 1))
	fmt.Println(inst.Exports["fib"].(func(uint32) uint32)(10))
}
"#;

/// The override/fallback glue.
/// An explicit `fd_write` import wins; `random_get` falls back to the bundled WASI.
/// It mirrors the other backends' override glues: intercept `fd_write` and print the bytes written.
const GO_OVERRIDE_GLUE: &str = r#"func RunTest() {
	var captured []byte
	var inst *Prog
	fdWrite := func(fd, iovs, iovsLen, outPtr uint32) uint32 {
		ptr := inst.memory.i32_load(uint64(iovs))
		length := inst.memory.i32_load(uint64(iovs) + 4)
		captured = append(captured, inst.memory.read_string(uint64(ptr), uint64(length))...)
		inst.memory.i32_store(uint64(outPtr), length)
		return 0
	}
	inst = NewProg(Imports{"wasi_snapshot_preview1": map[string]any{"fd_write": fdWrite}}, nil, nil, nil)
	inst.Exports["_start"].(func())() // random_get falls back to the bundled WASI
	fmt.Print(string(captured))
}
"#;

/// The `custom_wasi_provider` glue: a provider *object* replaces the whole bundled WASI.
/// `WasmImport(name)` resolves every function, and `Attach(instance)` binds the memory.
/// That is the Go shape of Ruby's `import`/`attach`.
/// So no import falls back, and `inst.wasi` stays `nil`.
/// The provider type is a package-level declaration, which appended glue may carry.
/// Only `import` blocks may not appear in appended glue.
const GO_CUSTOM_PROVIDER_GLUE: &str = r#"type myWasi struct {
	inst *Prog
	out  []byte
}

func (w *myWasi) WasmImport(name string) any {
	switch name {
	case "fd_write":
		return w.fdWrite
	case "random_get":
		return func(buf, length uint32) uint32 { return 0 }
	}
	return nil
}

func (w *myWasi) Attach(instance any) { w.inst = instance.(*Prog) }

func (w *myWasi) fdWrite(fd, iovs, iovsLen, outPtr uint32) uint32 {
	mem := w.inst.memory
	ptr := mem.i32_load(uint64(iovs))
	length := mem.i32_load(uint64(iovs) + 4)
	w.out = append(w.out, mem.read_string(uint64(ptr), uint64(length))...)
	mem.i32_store(uint64(outPtr), length)
	return 0
}

func RunTest() {
	wasi := &myWasi{}
	inst := NewProg(Imports{"wasi_snapshot_preview1": wasi}, nil, nil, nil)
	inst.Exports["_start"].(func())()
	fmt.Print(string(wasi.out))
	fmt.Printf("bundled wasi constructed: %v\n", inst.wasi != nil)
}
"#;

/// The `partial_override_falls_back_to_bundled_wasi` glue.
/// It is the override glue (`fd_write` intercepted, `random_get` falling back) plus one probe.
/// The probe checks that the bundled WASI *was* built for that one fallback.
/// `wasiInstance()` constructs it as the constructor takes the method value.
const GO_PARTIAL_OVERRIDE_GLUE: &str = r#"func RunTest() {
	var captured []byte
	var inst *Prog
	fdWrite := func(fd, iovs, iovsLen, outPtr uint32) uint32 {
		ptr := inst.memory.i32_load(uint64(iovs))
		length := inst.memory.i32_load(uint64(iovs) + 4)
		captured = append(captured, inst.memory.read_string(uint64(ptr), uint64(length))...)
		inst.memory.i32_store(uint64(outPtr), length)
		return 0
	}
	inst = NewProg(Imports{"wasi_snapshot_preview1": map[string]any{"fd_write": fdWrite}}, nil, nil, nil)
	inst.Exports["_start"].(func())() // random_get falls back to the bundled WASI
	fmt.Print(string(captured))
	fmt.Printf("bundled wasi constructed: %v\n", inst.wasi != nil)
}
"#;

/// The `wasi_stdio_capture` glue.
/// Go's bundled WASI holds `*os.File` at `fd` 1, and `fd_write` type-asserts exactly that.
/// So the embedder's sink is an `os.Pipe` write end.
/// It is swapped into `inst.wasi.fds` after construction.
/// It is a real `fd` rather than an in-memory buffer, the same shape Perl's glue uses.
/// Run `_start`, catching and dropping its clean `proc_exit`.
/// Then close the writer so the reader sees EOF.
/// Then copy the captured bytes to the real standard output.
/// The read loop is hand-rolled.
/// Library-mode Go imports `fmt` plus whatever the runtime bundle needs (`os`, here).
/// Appended glue cannot add an `io` import of its own.
const GO_STDIO_CAPTURE_GLUE: &str = r#"func RunTest() {
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

// --------------------------------------------------------------------- WASI file system glue.

/// The shared file system template.
/// It preopens the scratch directory (`{host}`) at guest `{guest}` (always `/`) and runs `_start`.
/// It surfaces a `proc_exit` code (via `rtExit`) as a trailing decimal line.
/// `rt/exit` is always seeded for library-mode WASI output.
/// So `*rtExit` is defined even for fixtures that never import `proc_exit`.
const GO_FS_GLUE: &str = r#"func RunTest() {
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

/// The root-preopen containment probe: probe the WASI resolver directly (no guest run).
/// A `"/" => "/"` preopen must resolve a relative path rather than reject every one.
/// `fd` 3 is the only preopen; `errno` 0 (`wasiOk`) means contained.
const GO_CONTAINMENT_GLUE: &str = r#"func RunTest() {
	w := newWASI(nil, nil, map[string]string{"/": "/"})
	_, err := w.resolve_path(3, "etc", true)
	if err == 0 {
		fmt.Println("contained")
	} else {
		fmt.Println("rejected")
	}
}
"#;

// --------------------------------------------------------------------- File system app glue.
// Class, `argv`, environment, and preopen guest paths are literals.
// Only the host scratch/cache directories come through {scratch}/{cache}.
// One glue serves both `stdout`-reporting and `proc_exit` fixtures.
// The former return from `_start` normally, so nothing extra is printed.

const GO_QJS_FILE_IO_GLUE: &str = r#"func RunTest() {
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

const GO_SQLITE3_SHELL_GLUE: &str = r#"func RunTest() {
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

const GO_RG_SEARCH_GLUE: &str = r#"func RunTest() {
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

/// The guest module this converted interpreter loads (`cowsay.wasm`) is itself a cached app.
/// So the whole app cache is preopened at `/apps`.
const GO_TOYWASM_GLUE: &str = r#"func RunTest() {
	inst := NewToywasm(nil, []string{"toywasm", "--wasi", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"}, nil, map[string]string{"/apps": "{cache}"})
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

/// Like the `toywasm` glue; wasm3's CLI takes the guest module directly.
/// Its MetaWASI build always forwards the guest's WASI.
const GO_WASM3_GLUE: &str = r#"func RunTest() {
	inst := NewWasm3(nil, []string{"wasm3", "/apps/cowsay.wasm", "Hello", "from", "dewasm!"}, nil, map[string]string{"/apps": "{cache}"})
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

const GO_CPYTHON_GLUE: &str = r#"func RunTest() {
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

const GO_CRUBY_GLUE: &str = r#"func RunTest() {
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

// --------------------------------------------------------------------- C-API drive glue (sqlite3).
// It does `malloc` / guest-memory pointer plumbing via the unexported `inst.memory` (`*Memory`).
// The appended `RunTest` carries no `import` (the library file already imports `fmt`).
// No Wasmtime snapshot exists (the results live in guest memory).
// So the shared case constant states each drive's output.
// Only the file-backed case uses {scratch}.

/// The sqlite3 C API driven in memory.
/// It runs `_initialize` and `sqlite3_malloc` + `*Memory` pointer plumbing.
/// Then it runs `open`/`exec`/`prepare`/`step`/`column`/`finalize`/`close`.
const GO_LIBSQLITE3_MEM: &str = r#"func RunTest() {
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

/// The sqlite3 C API against a file preopen: create+insert, close, reopen, select.
/// That is the file life cycle through the C API (the same file system stack as the shell).
const GO_LIBSQLITE3_FILE: &str = r#"func RunTest() {
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

	// create + insert, then close so every write reaches the file
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

/// Guest->host callback round trip.
/// The committed `sqlite3-binding.wasm` exports `run_query`, which calls `sqlite3_exec`.
/// It passes a C callback that forwards each row to the *imported* `env.host_row`.
/// The glue provides `host_row` via the import-provider map and collects the rows.
const GO_SQLITE3_CALLBACK: &str = r#"func RunTest() {
	var rows []string
	var mem *Memory
	// `host_row` is imported as `void host_row(int argc, char **argv)`: no result.
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

	inst := NewSqlite3Binding(Imports{"env": map[string]any{"host_row": hostRow}}, nil, nil, nil)
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

	// guest -> host: `run_query` calls back into `env.host_row` once per row
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

/// `libpcap` BPF filter compilation.
/// It drives `compile_filter` on "tcp port 80" (DLT_EN10MB, `snaplen` 65535).
/// Then it walks the serialized program in guest memory:
/// `[u32 bf_len][bf_len × {u16 code; u8 jt; u8 jf; u32 k}]`.
/// It prints each instruction as `code jt jf k`.
const GO_PCAP_COMPILE: &str = r#"func RunTest() {
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

/// tree-sitter JSON parse: drive `parse_source` on the fixed snippet `{"key": [1, true, null]}`.
/// Then print the parse tree's S-expression from guest memory.
/// It is a NUL-terminated C string from `malloc`.
const GO_TREESITTER_PARSE: &str = r#"func RunTest() {
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

/// `zeroperl` Perl-5.42 `eval` (issue #67).
/// It instantiates the reactor with a zero-returning `env.call_host_function` import stub.
/// The stub is called only when the guest registers host callbacks.
/// This program registers none.
/// The reactor also gets a `/dev/null` preopen (`zeroperl_init` returns 1 without it).
/// Then the sequence is `_initialize` → `zeroperl_init` → `malloc`.
/// It copies a Perl program into guest memory, then runs `zeroperl_eval` → `zeroperl_flush`.
/// The program is a regular expression capture and a `printf`.
/// So its standard output is deterministic.
/// The Perl source is a Go raw string literal: its backslash escapes belong to Perl, not to Go.
const GO_ZEROPERL_EVAL: &str = r#"func RunTest() {
	inst := NewZeroperl(
		Imports{"env": map[string]any{"call_host_function": func(a, b, c uint32) uint32 { return 0 }}},
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

/// ExifTool on `zeroperl` (issue #70).
/// The flattened `exiftool` CLI driver is `{cache}/exiftool-lib/exiftool`, preopened at `/work`.
/// It runs on the same `cache/zeroperl.wasm` reactor.
/// The reactor's SFS blob embeds the `Image::ExifTool` module tree.
/// So `use Image::ExifTool` resolves in-guest with no module preopen.
/// Instantiated like [`GO_ZEROPERL_EVAL`] (the `call_host_function` stub + a `/dev/null` preopen).
/// It adds the staged image at `/img`.
/// The Perl driver snippet sets `@ARGV`/`$0` and `do`es the script.
/// It first overrides `CORE::GLOBAL::exit` to a `die`.
/// ExifTool's terminal `exit` then unwinds back into `eval_pv` instead of tripping `proc_exit`.
/// Then `zeroperl_flush` pushes ExifTool's buffered `stdout` out through `fd` 1.
/// Only deterministic tags are requested (`-S -Make -Model -DateTimeOriginal`).
const GO_EXIFTOOL: &str = r#"func RunTest() {
	inst := NewZeroperl(
		Imports{"env": map[string]any{"call_host_function": func(a, b, c uint32) uint32 { return 0 }}},
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

/// Driver for the shared-table case.
/// It instantiates `TableExp` (exports the table).
/// Then it instantiates `TableImp` linked against it via the provider.
/// `otherInst.Exports` is the module provider.
/// The specification harness's cross-module `register` path does the same.
/// It prints `TableImp`'s `call0` result (`42`).
/// Both modules share the driver's `package main`, so they are named unqualified.
/// The driver file carries its own imports.
/// `compose_modules` writes only the package clause, since Go rejects an unused import.
const GO_SHARED_TABLE_GLUE: &str = r#"
import "fmt"

func main() {
	a := NewTableExp(nil, nil, nil, nil)
	b := NewTableImp(Imports{"a": a.Exports}, nil, nil, nil)
	fmt.Println(b.Exports["call0"].(func() uint32)())
}
"#;

/// Driver for the case where two Embedded artifacts coexist.
/// It imports two library conversions of the same module side by side.
/// They are the packages `alpha` and `beta` (#155).
/// Each package has its own runtime, so its trap type is its own unexported `rtTrap`.
/// That type is not visible to an importer by name.
/// But the panic value's *dynamic type* is observable through `%T`.
/// `%T` prints it package-qualified (`*alpha.rtTrap` vs. `*beta.rtTrap`).
/// That is the whole claim that the two artifacts coexist: they differ, and Alpha raises its own.
const GO_EMBEDDED_COEXIST_GLUE: &str = r#"
import (
	"fmt"
	"strings"

	"dewasmtest/alpha"
	"dewasmtest/beta"
)

// `trapType` runs `f` and returns the dynamic type of the value it panicked with.
// It returns "" if f returned normally.
func trapType(f func()) (ty string) {
	defer func() {
		if r := recover(); r != nil {
			ty = fmt.Sprintf("%T", r)
		}
	}()
	f()
	return
}

func main() {
	a := alpha.NewAlpha(nil, nil, nil, nil)
	b := beta.NewBeta(nil, nil, nil, nil)
	fmt.Println(a.Exports["div"].(func(uint32, uint32) uint32)(7, 2))
	fmt.Println(b.Exports["div"].(func(uint32, uint32) uint32)(0xfffffff9, 2))
	at := trapType(func() { a.Exports["div"].(func(uint32, uint32) uint32)(1, 0) })
	bt := trapType(func() { b.Exports["div"].(func(uint32, uint32) uint32)(1, 0) })
	if at != bt {
		fmt.Println("distinct-rt")
	} else {
		fmt.Println("same-rt")
	}
	if strings.HasPrefix(at, "*alpha.") {
		fmt.Println("trapped")
	}
}
"#;

/// DOOM: deterministic drive (synthetic clock, no input).
/// It dumps the framebuffer as a P6 PPM matching the Wasmtime snapshot.
/// Library-mode Go imports `fmt` but not `os`.
/// So the binary frame goes out via `fmt.Print(string(...))`.
/// `{ticks}`/`{clock_step}` filled by the runner.
const GO_DOOM_FRAME_GLUE: &str = r#"func RunTest() {
	var ms uint64
	var frameOff, frameW, frameH uint32
	imports := Imports{
		"console": map[string]any{
			"onErrorMessage": func(o, l uint32) {},
			"onInfoMessage":  func(o, l uint32) {},
		},
		"gameSaving": map[string]any{
			"sizeOfSaveGame": func(id uint32) uint32 { return 0 },
			"readSaveGame":   func(id, dst uint32) uint32 { return 0 },
			"writeSaveGame":  func(id, src, n uint32) uint32 { return n },
		},
		"runtimeControl": map[string]any{
			"timeInMilliseconds": func() uint64 { ms += {clock_step}; return ms },
		},
		"ui": map[string]any{
			"drawFrame": func(off uint32) { frameOff = off },
		},
		"loading": map[string]any{
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

/// NES (issue #114, mirrors the DOOM glue above).
/// Load the example ROM into `allocRom`'s buffer.
/// Then tick [`NES_FRAMES`] times with no input.
/// Compose the frame from `agnes`'s palette-index screen buffer and its palette (issue #117).
/// The `& 0x3f` mask is load-bearing.
/// Dump the frame as a P6 PPM matching the Wasmtime snapshot.
///
/// A function rather than a static `&str` constant, unlike every other backend's glue.
/// Go requires every `import` to appear before all other top-level declarations.
/// So glue *appended* after the generated code cannot add its own `import "os"` to read `{rom}`.
/// `nes.wasm` imports nothing, so there is no WASI route to the host either.
/// The ROM is therefore read here, at test time.
/// It is embedded as a `\xHH`-escaped string literal.
/// That is the same import-free escape route the backend uses for data segments.
fn go_nes_frame_glue() -> String {
    let rom = std::fs::read(dewasm_test_helper::alter_ego_rom_path())
        .expect("read alter_ego_rom_path: see examples/apps/scripts/nes.sh");
    let mut literal = String::with_capacity(rom.len() * 4 + 2);
    literal.push('"');
    for b in &rom {
        literal.push_str(&format!("\\x{b:02x}"));
    }
    literal.push('"');
    format!(
        r#"func RunTest() {{
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

dewasm_test_helper::library_add_e2e!(Go, GO_ADD_GLUE);
dewasm_test_helper::wasi_import_override_e2e!(Go, GO_OVERRIDE_GLUE);
dewasm_test_helper::stdio_capture_e2e!(Go, GO_STDIO_CAPTURE_GLUE);
dewasm_test_helper::custom_wasi_provider_e2e!(Go, GO_CUSTOM_PROVIDER_GLUE);
dewasm_test_helper::partial_override_e2e!(Go, GO_PARTIAL_OVERRIDE_GLUE);

dewasm_test_helper::wasi_suite!(Go, Stdio);
dewasm_test_helper::wasi_suite!(Go, ArgsEnv);
dewasm_test_helper::wasi_suite!(Go, Poll);
dewasm_test_helper::wasi_suite!(Go, Fs, GO_FS_GLUE);
dewasm_test_helper::wasi_root_containment_e2e!(Go, GO_CONTAINMENT_GLUE);
dewasm_test_helper::standalone_dir_e2e!(Go);
// Goroutine stacks start small and grow dynamically.
// So 5000 guest frames need no special handling at the entrypoint.
dewasm_test_helper::deep_recursion_e2e!(Go);
dewasm_test_helper::folded_temp_reuse_e2e!(Go);

dewasm_test_helper::cowsay_args_e2e!(Go);
dewasm_test_helper::cowsay_stdin_e2e!(Go);
// Fast: the category of `cowsay` (the `go build` cache dominates).
dewasm_test_helper::mruby_eh_e2e!(Go);
// The `ultra` cases are the giant-generated-program `go build`s.
// Together they exhausted a CI runner's memory (SIGTERM, #23).
// The other giant builds stayed under the ~1-min bar and remain at `slow`.
// Those are `qjs_repl_pty`, `sqlite3_shell_dbfile`, `pcap_compile`, and `treesitter_parse`.
dewasm_test_helper::qjs_eval_e2e!(Go, ultra);
dewasm_test_helper::sqlite3_shell_e2e!(Go, ultra);
dewasm_test_helper::gzip_e2e!(Go);

dewasm_test_helper::qjs_file_io_e2e!(Go, GO_QJS_FILE_IO_GLUE, ultra);
dewasm_test_helper::sqlite3_shell_dbfile_e2e!(Go, GO_SQLITE3_SHELL_GLUE);
dewasm_test_helper::rg_search_e2e!(Go, GO_RG_SEARCH_GLUE, ultra);
dewasm_test_helper::cpython_hello_e2e!(Go, GO_CPYTHON_GLUE, ultra);
// `ultra`: the CRuby wasm's `go build` is the longest in the suite.
// Wall time is the cost, not whether it can be done.
// The `wasi-vfs`-packed variant is the same interpreter plus an embedded standard library.
// It is in the same cost class.
dewasm_test_helper::cruby_hello_e2e!(Go, GO_CRUBY_GLUE, ultra);
dewasm_test_helper::cruby_packed_hello_e2e!(Go, ultra);
// Slow, like the other file system app cases.
// They convert the interpreter, then interpret the `cowsay` guest; the `go build` dominates.
dewasm_test_helper::toywasm_cowsay_e2e!(Go, GO_TOYWASM_GLUE);
// Slow for the same reason as the `toywasm` case above.
dewasm_test_helper::wasm3_cowsay_e2e!(Go, GO_WASM3_GLUE);
dewasm_test_helper::qjs_repl_pty_e2e!(Go);

dewasm_test_helper::libsqlite3_c_api_e2e!(Go, GO_LIBSQLITE3_MEM, ultra);
dewasm_test_helper::sqlite3_file_c_api_e2e!(Go, GO_LIBSQLITE3_FILE, ultra);
dewasm_test_helper::sqlite3_callback_binding_e2e!(Go, GO_SQLITE3_CALLBACK, ultra);
dewasm_test_helper::pcap_compile_e2e!(Go, GO_PCAP_COMPILE);
dewasm_test_helper::treesitter_parse_e2e!(Go, GO_TREESITTER_PARSE);
// The `zeroperl` reactor cases (issue #139) join the `ultra` giants above.
// The reactor's `go build` dominates the run.
dewasm_test_helper::zeroperl_eval_e2e!(Go, GO_ZEROPERL_EVAL, ultra);
dewasm_test_helper::exiftool_extract_e2e!(Go, GO_EXIFTOOL, ultra);

dewasm_test_helper::doom_frame_e2e!(Go, GO_DOOM_FRAME_GLUE);
dewasm_test_helper::nes_frame_e2e!(Go, &go_nes_frame_glue());

dewasm_test_helper::shared_table_e2e!(Go, GO_SHARED_TABLE_GLUE);
dewasm_test_helper::embedded_coexist_e2e!(Go, GO_EMBEDDED_COEXIST_GLUE);
