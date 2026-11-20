//! The one compile-and-cache recipe shared by the Java suites.
//! Those are `spec`, `e2e`, `wasi_testsuite`, and `memory_grow`.
//! Only the Java backend compiles to a class directory, so each of those suites needs this step.
//! Four copies of it also kept four cache namespaces.
//! Identical sources then compiled once per suite.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Output;

use dewasm_backend_java::javac_command;

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Compile `source` (a single `Main.java`) into a content-addressed cache of class directories.
/// Return the class directory's path.
/// `Err(Output)` carries the `javac` failure.
/// A caller can hand it straight back if it reports compile errors through `status.success()`.
/// A caller that treats a compile failure as a bug panics on it.
/// A missing `javac` is a loud failure (from `javac_command`).
///
/// The cache is keyed on the source, in a directory of the `javac` version that built it.
/// Every suite in the crate shares it.
/// These all land in the same namespace:
///
/// - the specification harness's `Main.java` for a `.wast` file;
/// - the e2e glue programs;
/// - the `wasi_testsuite` drivers.
///
/// So a source two suites happen to agree on is compiled once.
pub fn build_java(source: &str) -> Result<PathBuf, Output> {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let hash = hasher.finish();

    let cache = cache_dir();
    let classdir = cache.join(format!("cls-{hash:016x}"));

    if !classdir.join("Main.class").exists() {
        // Compile into a per-attempt unique directory, then rename onto the cache key.
        // Two threads with the same hash may build at the same time.
        // Only the final rename is shared.
        // So no reader ever sees a half-written class directory.
        let tmp = cache.join(format!(
            "cls-{hash:016x}.{}.{}",
            std::process::id(),
            COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("Main.java");
        std::fs::write(&src, source).unwrap();
        let build = javac_command()
            .arg("-d")
            .arg(&tmp)
            .arg(&src)
            .output()
            .expect("spawn javac");
        if !build.status.success() {
            return Err(build);
        }
        // A builder of the same source running at the same time may have claimed the key first.
        // Then the rename fails and this attempt's directory is not needed.
        // Drop it rather than leave it in `/tmp`.
        if std::fs::rename(&tmp, &classdir).is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
    }

    Ok(classdir)
}

/// The cache directory of the `javac` in use, found once per test binary.
fn cache_dir() -> &'static Path {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        dewasm_test_helper::build_cache_dir("dewasm-java-cache", javac_command().arg("-version"))
    })
}
