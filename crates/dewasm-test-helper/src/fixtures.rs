//! Fixture paths and the one conversion policy the e2e suites share. Paths resolve from the consuming crate via `CARGO_MANIFEST_DIR`; every crate that uses this helper sits at `crates/<x>/`, so `../../` still reaches the repo root (ADR-27).

use std::path::{Path, PathBuf};

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};

/// `examples/wat/`, home of the hand-written `.wat` fixtures.
pub fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wat")
}

/// `examples/apps/cache/`, populated by `examples/apps/fetch-and-build.sh` (ADR-9).
pub fn apps_cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/cache")
}

/// Read the cached app binary `examples/apps/cache/<stem>.wasm`, failing loud (ADR-15) when the cache has not been populated by `examples/apps/fetch-and-build.sh`.
pub fn require_cached_app(stem: &str) -> Vec<u8> {
    let path = apps_cache_dir().join(format!("{stem}.wasm"));
    assert!(
        path.exists(),
        "{stem} not cached — run examples/apps/fetch-and-build.sh (see docs/testing.md)"
    );
    std::fs::read(&path).expect("read wasm")
}

/// The entire assertion of a convert-only smoke (ADR-54): the conversion returned at all (any backend error panics inside `convert_app`), and it returned something. Deliberately nothing more — no golden over generated size or shape, which would be pure noise.
pub fn assert_converted(source: &str, case: &str, lang: &str) {
    assert!(
        !source.is_empty(),
        "{case} under {lang}: conversion produced empty source"
    );
}

/// `examples/apps/fixtures/`, home of our own committed app-driver fixtures (the QuickJS `.js` scripts the Phase 5a filesystem app cases run).
pub fn apps_fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/fixtures")
}

/// `examples/apps/golden/`, the checked-in golden outputs captured from `wasmtime` (ADR-15).
pub fn apps_golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/golden")
}

/// A fresh, empty scratch directory under the temp dir keyed by `name`, so cases running in parallel never share host state.
pub fn fresh_scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dewasm-app-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Convert raw wasm bytes with `backend`.
pub fn convert_bytes(backend: &dyn Backend, bytes: &[u8], mode: Mode, name: &str) -> String {
    let module = dewasm_core::build_module(bytes).expect("build IR");
    let source = backend
        .generate(
            &module,
            &GenOptions {
                mode,
                module_name: name.to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
                data_file: None,
            },
        )
        .expect("generate")
        .remove(0)
        .contents;
    // The primary source is always UTF-8 (generated code); only the optional data sidecar is raw bytes, and this helper only ever asks for the primary file (`data_file: None`).
    String::from_utf8(source).expect("generated source is valid UTF-8")
}

/// Convert a `.wat` fixture with `backend`.
pub fn convert(backend: &dyn Backend, wat_path: &Path, mode: Mode, name: &str) -> String {
    let bytes = wat::parse_file(wat_path).expect("parse wat");
    convert_bytes(backend, &bytes, mode, name)
}

/// Codegen recurses with the IR's control-flow nesting; SQLite's deepest functions exceed the 2 MiB test-thread default stack, so app conversion runs on a roomier stack.
pub fn convert_on_big_stack(
    backend: &(dyn Backend + Sync),
    bytes: &[u8],
    mode: Mode,
    name: &str,
) -> String {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(64 << 20)
            .spawn_scoped(scope, || convert_bytes(backend, bytes, mode, name))
            .expect("spawn codegen thread")
            .join()
            .expect("codegen thread")
    })
}
