//! The `go build` step the Go crate's test binaries share.
//! It compiles generated source to a content-addressed cache binary and hands back the path.
//! Identical sources (for example, `cowsay_args_e2e!` and `cowsay_stdin_e2e!`) then build once.
//! The cache is keyed on the source alone.
//! Every suite in the crate shares it (`e2e`, `spec`, `wasi_testsuite`, `module_name`).
//! So a program two of them happen to agree on is built once.
//!
//! The artifact's own `package` clause selects one of two layouts.
//! It is the one fact that decides how Go can build the artifact.
//!
//! - `package main`: one file, `go build` it directly.
//!   This covers standalone output and multi-module compositions in the specification's style.
//!   The test crate assembles those compositions itself.
//! - `package <name>` (library output): a Go *package*, which can only be built from a module.
//!   The source is the artifact plus whatever host glue the shared runner appended to it.
//!   Both are in that package.
//!   The source is written to `<pkg>/<pkg>.go` inside a temp module.
//!   Next to it sits a two-line `main.go` that imports it and calls the glue's `RunTest`.
//!   Some glue reaches into unexported internals (`inst.memory.data`, `*global[uint32]`).
//!   That is why the glue is appended into the package rather than written beside `main.go`.

// Shared by several test binaries, each of which uses a subset.
// A test module is compiled into every binary that declares it.
// So an item only one of them calls would otherwise warn as dead code.
#![allow(dead_code)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Output};

use dewasm_backend_go::find_go;

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Compile `source` to a content-addressed cache binary and return its path.
/// `Err(Output)` carries the `go build` failure.
/// A piped run can then report it via `status.success()`, while a pseudo-terminal run panics on it.
/// A missing `go` toolchain is a loud failure.
pub fn build_go(source: &str) -> Result<PathBuf, Output> {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO): see docs/testing.md");

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let hash = hasher.finish();

    let cache = std::env::temp_dir().join("dewasm-go-cache");
    std::fs::create_dir_all(&cache).unwrap();
    let bin = cache.join(format!("prog-{hash:016x}"));
    if bin.exists() {
        return Ok(bin);
    }

    // Both the sources and the binary get per-attempt unique names.
    // Two threads with the same hash (the two `cowsay_*_e2e!` cases) may build at the same time.
    // A shared source path would let one truncate the file mid-read of the other's `go build`.
    // That was issue #19.
    // Only the final rename onto the cache key is shared, and that is atomic.
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
        // Build the module's root package (`.`).
        // It pulls in the artifact package through the import above.
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

/// Build the Go module the caller has already laid out in `dir` and return the binary's path.
/// The layout is a `go.mod` at its root and `package main` files beside it.
/// Library packages the caller wrote go into directories below it.
/// The multi-module cases use this instead of [`build_go`].
/// Their artifacts are several files by design (that is what the case is about).
/// So there is no single source to key a cache on, and each case gets a fresh directory anyway.
pub fn build_go_dir(dir: &std::path::Path) -> Result<PathBuf, Output> {
    let go =
        find_go().expect("go toolchain not found on PATH (or $DEWASM_GO): see docs/testing.md");
    let bin = dir.join("prog");
    let build = run_build(&go, &bin, std::path::Path::new("."), Some(dir));
    if !build.status.success() {
        return Err(build);
    }
    Ok(bin)
}

/// `cwd`, when given, is the temp module root the package layout is built from.
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
        // An unrelated `go.work` may sit above the temporary directory.
        // It would otherwise pull this temporary module into a workspace that does not list it.
        cmd.env("GOWORK", "off");
    }
    cmd.output().expect("spawn go build")
}

/// The package `source` declares.
/// The first line starting with `package ` is the clause itself.
/// Everything before it in generated output is `//` comments.
pub fn package_clause(source: &str) -> &str {
    source
        .lines()
        .find_map(|line| line.strip_prefix("package "))
        .map(str::trim)
        .expect("generated Go source declares a package")
}
