//! The `codon build` step the Codon crate's test binaries share: compile generated source to a content-addressed cache binary (identical programs build once) and hand back the path.
//! The cache is keyed on the source alone and shared by every suite in the crate.
//!
//! A built binary links Codon's runtime dylibs (`libcodonrt`, `libomp`) by `@rpath`/`@loader_path`, so [`run_codon_binary`] puts the toolchain's lib directory on the loader path of every spawned run.

// Shared by several test binaries, each of which uses a subset.
#![allow(dead_code)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::{Command, Output};

use dewasm_backend_codon::{codon_lib_dir, find_codon};

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The loader-path environment variable for this platform.
const LOADER_PATH_VAR: &str = if cfg!(target_os = "macos") {
    "DYLD_LIBRARY_PATH"
} else {
    "LD_LIBRARY_PATH"
};

/// Compile `source` to a content-addressed cache binary and return its path.
/// `Err(Output)` carries the `codon build` failure so a piped run can report it via `status.success()`.
/// A missing `codon` toolchain is a loud failure.
///
/// Every suite builds *debug* by default: `-release` costs ~8x the compile time (superlinearly worse on huge single generated functions), CI pays every codon build fresh, and the build is semantically identical (the one optimizer-sensitive path, identity-fold NaN quieting, is handled at emission via the quiet-if-NaN wrappers).
/// `DEWASM_CODON_RELEASE=1` switches every build to `-release`: the local pre-release verification runs the spec sweep once in the configuration the benchmarks and users run.
pub fn build_codon(source: &str) -> Result<PathBuf, Output> {
    build_codon_with(source, std::env::var_os("DEWASM_CODON_RELEASE").is_some())
}

/// An alias kept for the e2e suites' call sites; same policy as [`build_codon`].
pub fn build_codon_debug(source: &str) -> Result<PathBuf, Output> {
    build_codon(source)
}

fn build_codon_with(source: &str, release: bool) -> Result<PathBuf, Output> {
    let codon = find_codon()
        .expect("codon toolchain not found on PATH (or $DEWASM_CODON): see docs/testing.md");

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let hash = hasher.finish();
    let tag = if release { "" } else { "-dbg" };

    let cache = std::env::temp_dir().join("dewasm-codon-cache");
    std::fs::create_dir_all(&cache).unwrap();
    let bin = cache.join(format!("prog{tag}-{hash:016x}"));
    if bin.exists() {
        return Ok(bin);
    }

    // Both the sources and the binary get per-attempt unique names: two threads with the same hash may build concurrently, and a shared source path would let one truncate the file mid-read of the other's build.
    // Only the final rename onto the cache key is shared, and that is atomic.
    let unique = format!(
        "{hash:016x}.{}.{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let tmp_bin = cache.join(format!("prog{tag}-{unique}"));
    let src = cache.join(format!("src-{unique}.codon"));
    std::fs::write(&src, source).unwrap();
    let mut cmd = Command::new(&codon);
    cmd.arg("build");
    if release {
        cmd.arg("-release");
    }
    let build = cmd
        .arg("-o")
        .arg(&tmp_bin)
        .arg(&src)
        .output()
        .expect("spawn codon build");
    // A failed build leaves its source behind: the compiler's messages name its path.
    if !build.status.success() {
        return Err(build);
    }
    let _ = std::fs::remove_file(&src);
    ensure_runtime_dylibs(&cache);
    std::fs::rename(&tmp_bin, &bin).expect("move built binary into cache");
    Ok(bin)
}

/// Copy Codon's runtime shared libraries next to the cache binaries once: a built binary references them relative to itself (`@loader_path` on macOS), so a copy beside it runs without any loader-path environment variable.
/// That matters to the WASI-testsuite runs, whose child environment is exactly the manifest's: a loader-path variable added there would leak into the guest's environ.
fn ensure_runtime_dylibs(cache: &std::path::Path) {
    let Some(lib) = find_codon().and_then(|c| codon_lib_dir(&c)) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&lib) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !(name_str.ends_with(".dylib") || name_str.contains(".so")) {
            continue;
        }
        let dst = cache.join(&name);
        if !dst.exists() {
            let _ = std::fs::copy(entry.path(), &dst);
        }
    }
}

/// Run a built binary with the Codon runtime dylibs on the loader path.
pub fn run_codon_binary(bin: &std::path::Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut cmd = Command::new(bin);
    cmd.args(args);
    if let Some(lib) = find_codon().and_then(|c| codon_lib_dir(&c)) {
        cmd.env(LOADER_PATH_VAR, lib);
    }
    dewasm_test_helper::run_command_bytes(&mut cmd, stdin)
}
