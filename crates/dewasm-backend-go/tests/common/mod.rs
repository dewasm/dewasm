//! The `go build` step the Go crate's test binaries share: compile generated source to a content-addressed cache binary (so identical sources — e.g. cowsay's args and stdin cases — build once) and hand back the path. The cache is keyed on the source alone and shared by every suite in the crate (e2e, spec, wasi-testsuite, module-name), so a program two of them happen to agree on is built once.
//!
//! Two layouts, selected by the artifact's own `package` clause — the one fact that decides how Go can build it:
//!
//! - `package main` (standalone output, and the spec-style multi-module compositions the test crate assembles itself): one file, `go build` it directly.
//! - `package <name>` (library output): a Go *package*, which can only be built from a module. The source — the artifact plus whatever host glue the shared runner appended to it, both in that package — is written to `<pkg>/<pkg>.go` inside a temp module, next to a two-line `main.go` that imports it and calls the glue's `RunTest`. Glue that reaches into unexported internals (`inst.memory.data`, `*global[uint32]`) is why it is appended into the package rather than written beside `main.go`.

// Shared by several test binaries, each of which uses a subset: a test module is compiled into every binary that declares it, so an item only one of them calls would otherwise warn as dead code.
#![allow(dead_code)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Output};

use dewasm_backend_go::find_go;

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Compile `source` to a content-addressed cache binary and return its path. `Err(Output)` carries the `go build` failure so a piped run can report it via `status.success()` while a pty run panics on it. A missing `go` toolchain is a loud failure.
pub fn build_go(source: &str) -> Result<PathBuf, Output> {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO) — see docs/testing.md");

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let hash = hasher.finish();

    let cache = std::env::temp_dir().join("dewasm-go-cache");
    std::fs::create_dir_all(&cache).unwrap();
    let bin = cache.join(format!("prog-{hash:016x}"));
    if bin.exists() {
        return Ok(bin);
    }

    // Both the sources and the binary get per-attempt unique names: two threads with the same hash (cowsay's args and stdin cases) may build concurrently, and a shared source path would let one truncate the file mid-read of the other's `go build` (issue #19). Only the final rename onto the cache key is shared, and that is atomic.
    let unique = format!(
        "{hash:016x}.{}.{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let tmp_bin = cache.join(format!("prog-{unique}"));

    let package = package_clause(source);
    let build = if package == "main" {
        let src = cache.join(format!("src-{unique}.go"));
        std::fs::write(&src, source).unwrap();
        let build = run_build(&go, &tmp_bin, &src, None);
        if build.status.success() {
            let _ = std::fs::remove_file(&src);
        }
        build
    } else {
        let dir = cache.join(format!("mod-{unique}"));
        let pkg_dir = dir.join(package);
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(dir.join("go.mod"), "module dewasmtest\n\ngo 1.21\n").unwrap();
        std::fs::write(pkg_dir.join(format!("{package}.go")), source).unwrap();
        std::fs::write(
            dir.join("main.go"),
            format!(
                "package main\n\nimport \"dewasmtest/{package}\"\n\nfunc main() {{ {package}.RunTest() }}\n"
            ),
        )
        .unwrap();
        // Build the module's root package (`.`), which pulls in the artifact package through the import above.
        let build = run_build(&go, &tmp_bin, std::path::Path::new("."), Some(&dir));
        if build.status.success() {
            let _ = std::fs::remove_dir_all(&dir);
        }
        build
    };
    // A failed build leaves its sources behind: the compiler's messages name their paths.
    if !build.status.success() {
        return Err(build);
    }
    let _ = std::fs::rename(&tmp_bin, &bin);
    Ok(bin)
}

/// Build the Go module the caller has already laid out in `dir` — a `go.mod` at its root, `package main` files beside it, and whatever library packages it wrote into subdirectories — and return the resulting binary's path. The multi-module cases use this instead of [`build_go`]: their artifacts are several files by design (that is what the case is about), so there is no single source to key a cache on, and each case gets a fresh directory anyway.
pub fn build_go_dir(dir: &std::path::Path) -> Result<PathBuf, Output> {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO) — see docs/testing.md");
    let bin = dir.join("prog");
    let build = run_build(&go, &bin, std::path::Path::new("."), Some(dir));
    if !build.status.success() {
        return Err(build);
    }
    Ok(bin)
}

/// `go build -o out target`, optionally from `cwd` (the temp module root for the package layout).
fn run_build(
    go: &std::path::Path,
    out: &std::path::Path,
    target: &std::path::Path,
    cwd: Option<&std::path::Path>,
) -> Output {
    let mut cmd = Command::new(go);
    cmd.arg("build").arg("-o").arg(out).arg(target);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
        // A stray `go.work` above the temp dir would otherwise pull this throwaway module into a workspace that does not list it.
        cmd.env("GOWORK", "off");
    }
    cmd.output().expect("spawn go build")
}

/// The package `source` declares. The first line starting with `package ` is the clause itself: everything before it in generated output is `//` comments.
pub fn package_clause(source: &str) -> &str {
    source
        .lines()
        .find_map(|line| line.strip_prefix("package "))
        .map(str::trim)
        .expect("generated Go source declares a package")
}
