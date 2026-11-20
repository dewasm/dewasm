//! Where a compiled backend's test suites keep what they built.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The directory of the build cache `name` for the toolchain `version_command` reports.
/// A cache keyed on the source alone hands a later toolchain what an earlier one built.
/// The later toolchain then never compiles that source, so a break in it stays hidden.
/// A binary may also load a runtime library of the other version from beside it.
///
/// The directory is created; a toolchain that cannot report its version is a loud failure.
pub fn build_cache_dir(name: &str, version_command: &mut Command) -> PathBuf {
    let output = version_command
        .output()
        .unwrap_or_else(|e| panic!("spawn {version_command:?}: {e}"));
    assert!(
        output.status.success(),
        "{version_command:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut version = output.stdout;
    version.extend_from_slice(&output.stderr);
    let dir = versioned_dir(&std::env::temp_dir(), name, &version);
    std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("create {}: {e}", dir.display()));
    dir
}

fn versioned_dir(root: &Path, name: &str, version: &[u8]) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    version.hash(&mut hasher);
    root.join(name).join(format!("{:016x}", hasher.finish()))
}

#[cfg(test)]
mod tests {
    use super::versioned_dir;
    use std::path::Path;

    #[test]
    fn two_toolchain_versions_do_not_share_a_directory() {
        let root = Path::new("/tmp");
        let old = versioned_dir(root, "dewasm-codon-cache", b"0.20.1\n");
        let new = versioned_dir(root, "dewasm-codon-cache", b"0.20.7\n");
        assert_ne!(old, new);
        assert_eq!(old, versioned_dir(root, "dewasm-codon-cache", b"0.20.1\n"));
        assert_eq!(old.parent(), new.parent());
    }
}
