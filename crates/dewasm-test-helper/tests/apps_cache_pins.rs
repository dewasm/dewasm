//! The app cache matches the pins the fetch scripts hold.
//!
//! Every suite that runs a real app reads `examples/apps/cache/<app>.wasm` through [`dewasm_test_helper::apps_cache_dir`], which says nothing about *which* build is there.
//! A copy left over from an earlier pin is a different program: it may fail in ways that look like a code regression, or pass and prove nothing about what is pinned now.
//!
//! Checking on every `apps_cache_dir()` call would cost a subprocess per case, so the check is one test instead: `cargo test` runs it once, and a stale cache is named here rather than diagnosed from whatever the app did afterwards.
//!
//! The pins live in the fetch scripts and `setup.sh --check` asks them, so nothing is duplicated here; it needs no network.

use std::process::Command;

#[test]
fn the_app_cache_matches_its_pins() {
    let script =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/setup.sh");
    let out = Command::new(&script)
        .arg("--check")
        .output()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", script.display()));
    assert!(
        out.status.success(),
        "the app cache does not match its pins, so the suites below would run the wrong programs:\n{}\nrun examples/apps/setup.sh",
        String::from_utf8_lossy(&out.stderr).trim_end()
    );
}
