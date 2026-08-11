//! Library-mode scenarios over hand-written `.wat` fixtures (`examples/wat/`). These need per-language glue (Ruby method calls vs. Bash function calls against globals): that can't be shared, so glue is *not* in the case data; each backend crate passes a named glue constant to the per-case macro. Each glue is engineered to observe the same thing the same way (e.g. both intercept fd_write and print the literal bytes written), so one `expect` per scenario is pinned instead of one per language that could quietly drift apart.
//!
//! Each case is a `pub const` [`LibraryCase`] a per-case macro (`library_add_e2e!`, `wasi_import_override_e2e!`, ...) drives; a backend declares participation by invoking the macro, and drops it (with a REASON comment) for a capability it lacks: the replacement for the old per-case `exclude` list.

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{convert, examples_dir};

pub struct LibraryCase {
    pub name: &'static str,
    pub wat: &'static str,
    /// The case's kebab-case name, converted per backend by [`BackendUnderTest::module_name`]: the glue consts spell out what that conversion yields (`prog` -> Ruby `Prog`, Bash `prog_`).
    pub module_name: &'static str,
    /// Both sides are engineered to produce this same string: the glue captures and prints the actual bytes the wasm module wrote, rather than a language-specific diagnostic, so there is exactly one expectation per scenario.
    pub expect: &'static str,
}

/// `add.wat`: plain exported arithmetic (masked-unsigned overflow and a fib recursion). Every backend runs it (`library_add_e2e!`).
pub const LIBRARY_ADD: LibraryCase = LibraryCase {
    name: "add",
    wat: "add.wat",
    module_name: "add",
    expect: "5\n0\n55\n",
};

/// The override/fallback semantics: an explicit import wins, an unhandled one falls back to the bundled WASI. Both glues intercept fd_write and print the actual bytes the module wrote (rather than a fd/len diagnostic): the one observable both languages can produce identically. Both sides only touch fd_write/random_get (WASI core), so every backend runs it (`wasi_import_override_e2e!`).
pub const WASI_IMPORT_OVERRIDE: LibraryCase = LibraryCase {
    name: "wasi_import_override",
    wat: "wasi_imports.wat",
    module_name: "prog",
    expect: "ok\n",
};

/// A provider *object* replaces the bundled WASI wholesale: `import`/`wasm_import`/`WasmImport`/`wasmImport` resolves every function, `attach` binds the memory, and, because every import is covered, the bundled WASI is never lazily constructed. The glue prints the intercepted bytes plus the "not constructed" observable. Every backend invokes `custom_wasi_provider_e2e!`; Bash's provider is a prefix whose export map covers both imports, and its bundled WASI is prefix-scoped shell variables built only when an import falls back, so `declare -p <p>wfds` is the same observable.
pub const CUSTOM_WASI_PROVIDER: LibraryCase = LibraryCase {
    name: "custom_wasi_provider",
    wat: "wasi_imports.wat",
    module_name: "prog",
    expect: "ok\nbundled wasi constructed: false\n",
};

/// The `true` counterpart: a *partial* override (fd_write only) still lets the bundled WASI be lazily constructed for the one import it doesn't cover (random_get): the `@wasi ||= ...` idiom. Same fallback the always-on `WASI_IMPORT_OVERRIDE` case proves, but here the glue additionally probes that the bundled WASI *was* built. Same participants as `CUSTOM_WASI_PROVIDER` (`partial_override_e2e!`).
pub const PARTIAL_OVERRIDE: LibraryCase = LibraryCase {
    name: "partial_override_falls_back_to_bundled_wasi",
    wat: "wasi_imports.wat",
    module_name: "prog",
    expect: "ok\nbundled wasi constructed: true\n",
};

/// Library-mode stdio captured by the embedder: the host wires the module's stdout to a sink of its own and reads the guest output back. Every backend invokes `stdio_capture_e2e!`, each with the sink its runtime's fd 1 accepts: an in-memory object where one fits (Ruby StringIO, Python BytesIO, Java ByteArrayOutputStream), an fd-level redirect where the fd must stay a real one (Perl's unlinked temp file, Go's `os.Pipe`, Bash's command substitution).
pub const STDIO_CAPTURE: LibraryCase = LibraryCase {
    name: "wasi_stdio_capture",
    wat: "hello.wat",
    module_name: "prog",
    expect: "Hello, WASI!\n",
};

/// Convert `case` in library mode, append `lang`'s `glue` for it, run it, and check stdout against the case's fixed expectation. A non-zero exit is a failure regardless of stdout.
pub fn run_library_case(lang: &dyn BackendUnderTest, case: &LibraryCase, glue: &str) {
    let code = convert(
        lang.backend(),
        &examples_dir().join(case.wat),
        Mode::Library,
        &lang.module_name(case.module_name),
    );
    let output = lang.run(&format!("{code}\n{glue}"), &[], "");
    assert!(
        output.status.success(),
        "{}: {} failed: {}\n{}",
        case.name,
        lang.name(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect,
        "{}: {} stdout",
        case.name,
        lang.name()
    );
}
