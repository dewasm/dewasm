//! Fixture paths and the one conversion policy the e2e suites share. Paths
//! resolve from the consuming crate via `CARGO_MANIFEST_DIR`; every crate
//! that uses this helper sits at `crates/<x>/`, so `../../` still reaches the
//! repo root (ADR-27).

use std::path::{Path, PathBuf};

use dewasm_backend::{Backend, GenOptions, Mode, RuntimeLinkage};

/// `examples/wat/`, home of the hand-written `.wat` fixtures.
pub fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/wat")
}

/// `examples/apps/cache/`, populated by `examples/apps/fetch.sh` (ADR-9).
pub fn apps_cache_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/apps/cache")
}

/// Convert raw wasm bytes with `backend`.
pub fn convert_bytes(backend: &dyn Backend, bytes: &[u8], mode: Mode, name: &str) -> String {
    let module = dewasm_core::build_module(bytes).expect("build IR");
    backend
        .generate(
            &module,
            &GenOptions {
                mode,
                module_name: name.to_string(),
                runtime: RuntimeLinkage::Embedded,
                default_wasi: true,
            },
        )
        .expect("generate")
        .remove(0)
        .contents
}

/// Convert a `.wat` fixture with `backend`.
pub fn convert(backend: &dyn Backend, wat_path: &Path, mode: Mode, name: &str) -> String {
    let bytes = wat::parse_file(wat_path).expect("parse wat");
    convert_bytes(backend, &bytes, mode, name)
}

/// Codegen recurses with the IR's control-flow nesting; SQLite's deepest
/// functions exceed the 2 MiB test-thread default stack, so app conversion
/// runs on a roomier stack.
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
