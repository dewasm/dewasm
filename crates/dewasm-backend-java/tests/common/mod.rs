//! The one compile-and-cache recipe shared by the Java suites.
//! Those are spec, e2e, wasi-testsuite, and memory_grow.
//! Java is the only compiled-to-a-class-dir backend, so every one of those suites needs this step.
//! Four copies of it also kept four cache namespaces.
//! Identical sources then compiled once per suite.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Output;

use dewasm_backend_java::javac_command;

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Compile `source` (a single `Main.java`) into a content-addressed class-dir cache.
/// Return the class dir's path.
/// `Err(Output)` carries the `javac` failure.
/// A caller can hand it straight back if it reports compile errors through `status.success()`.
/// A caller that treats a compile failure as a bug panics on it.
/// A missing `javac` is a loud failure (from `javac_command`).
///
/// The cache is keyed on the source alone and shared by every suite in the crate.
/// These all land in the same namespace:
///
/// - the spec harness's `Main.java` for a `.wast` file;
/// - the e2e glue programs;
/// - the wasi-testsuite drivers.
///
/// So a source two suites happen to agree on is compiled once.
pub fn build_java(source: &str) -> Result<PathBuf, Output> {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let hash = hasher.finish();

    let cache = std::env::temp_dir().join("dewasm-java-cache");
    std::fs::create_dir_all(&cache).unwrap();
    let classdir = cache.join(format!("cls-{hash:016x}"));

    if !classdir.join("Main.class").exists() {
        // Compile into a per-attempt unique dir, then rename onto the cache key.
        // Two threads with the same hash may build concurrently.
        // Only the final rename is shared.
        // So no reader ever sees a half-written class dir.
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
        // A concurrent builder of the same source may have claimed the key first.
        // Then the rename fails and this attempt's dir is redundant.
        // Drop it rather than leave it in /tmp.
        if std::fs::rename(&tmp, &classdir).is_err() {
            let _ = std::fs::remove_dir_all(&tmp);
        }
    }

    Ok(classdir)
}
