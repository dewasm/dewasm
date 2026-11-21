//! The `spinel` build step the Spinel crate's test binaries share.
//! It compiles generated source to a cached binary and hands back the path.
//! Identical programs build once.
//! The cache is keyed on the source, in a directory of the Spinel version that built it.

// Shared by several test binaries, each of which uses a subset.
#![allow(dead_code)]

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dewasm_backend_spinel::{find_spinel, SPINEL_FLAGS};

static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The build flags of every suite, beside the backend's own [`SPINEL_FLAGS`].
///
/// `-O 0`: a build takes a third of the time `-O 2` needs.
/// `--no-bigint` makes a value that only fits an arbitrary-precision integer a compile error.
/// So no such value is left in the runtime units or the generated code.
const TEST_FLAGS: &[&str] = &["-O", "0", "--no-bigint"];

/// Compile `source` to a cached binary and return its path.
/// `Err(Output)` carries the `spinel` failure, so a piped run reports it through its status.
/// A missing `spinel` is a loud failure.
/// The cache key holds the flags too, so a binary built with other flags is never served.
pub fn build_spinel(source: &str) -> Result<PathBuf, Output> {
    let spinel =
        find_spinel().expect("spinel not found on PATH (or $DEWASM_SPINEL): see docs/testing.md");

    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    TEST_FLAGS.hash(&mut hasher);
    SPINEL_FLAGS.hash(&mut hasher);
    let hash = hasher.finish();

    let cache = cache_dir(&spinel);
    let bin = cache.join(format!("prog-{hash:016x}"));
    if bin.exists() {
        return Ok(bin);
    }

    // The source and the binary get names no other attempt has.
    // Two threads with the same hash may build at the same time.
    // Only the final rename onto the cache key is shared, and that is atomic.
    let unique = format!(
        "{hash:016x}.{}.{}",
        std::process::id(),
        COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let tmp_bin = cache.join(format!("prog-{unique}"));
    let src = cache.join(format!("src-{unique}.rb"));
    std::fs::write(&src, source).unwrap();
    let build = Command::new(&spinel)
        .args(TEST_FLAGS)
        .args(SPINEL_FLAGS)
        .arg(&src)
        .arg("-o")
        .arg(&tmp_bin)
        .output()
        .expect("spawn spinel");
    // A failed build leaves its source behind: the compiler's messages name its path.
    if !build.status.success() {
        return Err(build);
    }
    let _ = std::fs::remove_file(&src);
    std::fs::rename(&tmp_bin, &bin).expect("move built binary into cache");
    Ok(bin)
}

/// The cache directory of the `spinel` in use, found once per test binary.
fn cache_dir(spinel: &Path) -> &'static Path {
    static DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        dewasm_test_helper::build_cache_dir(
            "dewasm-spinel-cache",
            Command::new(spinel).arg("--version"),
        )
    })
}

/// Build `source` and run it; a failed build is returned as the run's output.
pub fn build_and_run(source: &str, args: &[&str], stdin: &[u8]) -> Output {
    match build_spinel(source) {
        Err(build) => build,
        Ok(bin) => dewasm_test_helper::run_command_bytes(Command::new(bin).args(args), stdin),
    }
}
