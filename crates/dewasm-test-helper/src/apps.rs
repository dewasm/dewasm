//! End-to-end cases over real-world apps (examples/apps/, ADR-9): convert each cached app with a backend and require byte-identical stdout and exit status against a snapshot output captured once from wasmtime and checked into `examples/apps/snapshots/` (ADR-15) — running these does not itself need `wasmtime` installed. (Older ADRs call these snapshot files "golden".)
//!
//! Per ADR-15, missing prerequisites (the interpreter, or the cache populated by `examples/apps/fetch-and-build.sh`) fail the test, they don't skip it. Each case is a `pub const` [`AppCase`] driven by its own per-case macro (`cowsay_args_e2e!`, `cowsay_stdin_e2e!`, `qjs_eval_e2e!`, `sqlite3_shell_e2e!`, ADR-27 revision). `qjs_eval_e2e!`/`sqlite3_shell_e2e!` are slow — softfloat makes QuickJS/SQLite take tens of seconds under Bash — so the macro expands their generated `#[test]` as `#[ignore]`d unless the expanding backend crate's `slow_test` feature is enabled; see [`run_slow_app_case`], which just runs the case unconditionally now that the gating lives at the macro/feature level.

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, apps_snapshot_dir};

pub struct AppCase {
    pub name: &'static str,
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    /// Captured once via `wasmtime run` (ADR-15); the snapshot reference this case's generated-language output must match exactly.
    pub expect_stdout: &'static str,
    pub expect_code: i32,
}

/// cowsay driven purely by argv.
pub const COWSAY_ARGS: AppCase = AppCase {
    name: "cowsay",
    args: &["Hello", "from", "dewasm!"],
    stdin: "",
    expect_stdout: include_str!("../../../examples/apps/snapshots/cowsay_args.stdout"),
    expect_code: 0,
};

/// cowsay reading its message from stdin.
pub const COWSAY_STDIN: AppCase = AppCase {
    name: "cowsay",
    args: &[],
    stdin: "moo via stdin\n",
    expect_stdout: include_str!("../../../examples/apps/snapshots/cowsay_stdin.stdout"),
    expect_code: 0,
};

/// QuickJS `-e` one-liner eval (slow tier: softfloat-bound interpreters skip by default, see [`run_slow_app_case`]).
pub const QJS_EVAL: AppCase = AppCase {
    name: "qjs",
    args: &[
        "-e",
        r#"console.log("2^16 =", Math.pow(2, 16)); console.log(JSON.stringify([3,1,2].sort()));"#,
    ],
    stdin: "",
    expect_stdout: include_str!("../../../examples/apps/snapshots/qjs.stdout"),
    expect_code: 0,
};

/// sqlite3 shell against an in-memory database (slow tier: softfloat-bound interpreters skip by default, see [`run_slow_app_case`]).
pub const SQLITE3_SHELL: AppCase = AppCase {
    name: "sqlite3-shell",
    args: &[],
    stdin: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\n\
            INSERT INTO users (name) VALUES (\"alice\"), (\"bob\"), (\"carol\");\n\
            SELECT id, upper(name) FROM users WHERE id >= 2;\n\
            SELECT count(*), avg(id) FROM users;\n",
    expect_stdout: include_str!("../../../examples/apps/snapshots/sqlite3_shell.stdout"),
    expect_code: 0,
};

/// Convert the cached app `case` names with `lang`'s backend, run it, and diff against the case's snapshot output.
fn run_app_case_inner(lang: &dyn BackendUnderTest, case: &AppCase) {
    let wasm_path = apps_cache_dir().join(format!("{}.wasm", case.name));
    assert!(
        wasm_path.exists(),
        "{} not cached — run examples/apps/fetch-and-build.sh (see docs/testing.md)",
        case.name
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let src = lang.convert_app(&bytes, Mode::Standalone, case.name);
    let output = lang.run(&src, case.args, case.stdin);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect_stdout,
        "{} {:?} under {}: stdout differs from the snapshot output",
        case.name,
        case.args,
        lang.name()
    );
    assert_eq!(
        output.status.code(),
        Some(case.expect_code),
        "{} under {}: exit status differs",
        case.name,
        lang.name()
    );
    println!(
        "{} {:?} under {}: matches snapshot output",
        case.name,
        case.args,
        lang.name()
    );
}

/// Run a fast [`AppCase`] (`COWSAY_ARGS`/`COWSAY_STDIN`) for `lang` unconditionally.
pub fn run_app_case(lang: &dyn BackendUnderTest, case: &AppCase) {
    run_app_case_inner(lang, case);
}

/// Rerun an [`AppCase`] under `lang` (the wasmtime engine) and return its raw stdout — the bytes to write into the case's snapshot file (ADR-56). Used by `cargo xtask update-snapshots`; the compare-only `apps` suites never call it. Fails loud (ADR-15) on a missing cache or a capture whose exit status is not the pinned `expect_code`.
pub fn capture_app_stdout(lang: &dyn BackendUnderTest, case: &AppCase) -> Vec<u8> {
    let wasm_path = apps_cache_dir().join(format!("{}.wasm", case.name));
    assert!(
        wasm_path.exists(),
        "{} not cached — run examples/apps/fetch-and-build.sh (see docs/testing.md)",
        case.name
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let src = lang.convert_app(&bytes, Mode::Standalone, case.name);
    let output = lang.run(&src, case.args, case.stdin);
    assert_eq!(
        output.status.code(),
        Some(case.expect_code),
        "{} under {}: exit status differs while capturing the snapshot",
        case.name,
        lang.name()
    );
    output.stdout
}

/// Rerun the gzip *compress* case under `lang` and return its raw compressed stdout — the bytes for `examples/apps/snapshots/minigzip_compress.gz` (ADR-56). Fails loud on a missing cache or a nonzero exit.
pub fn capture_gzip_compress(lang: &dyn BackendUnderTest) -> Vec<u8> {
    let wasm_path = apps_cache_dir().join("minigzip.wasm");
    assert!(
        wasm_path.exists(),
        "minigzip not cached — run examples/apps/fetch-and-build.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let src = lang.convert_app(&bytes, Mode::Standalone, "minigzip");
    let input = std::fs::read(apps_fixtures_dir().join("gzip").join("input.txt"))
        .expect("read gzip input fixture");
    let compressed = lang.run_bytes(&src, &[], &input);
    assert!(
        compressed.status.success(),
        "minigzip compress under {}: nonzero exit {} while capturing the snapshot\n{}",
        lang.name(),
        compressed.status,
        String::from_utf8_lossy(&compressed.stderr)
    );
    compressed.stdout
}

/// Run a slow-tier [`AppCase`] (`QJS_EVAL`/`SQLITE3_SHELL`) for `lang` unconditionally. The perf opt-out now lives at the macro/feature level (`qjs_eval_e2e!`/`sqlite3_shell_e2e!` expand their `#[test]` as `#[ignore]`d unless the `slow_test` feature is on), so this runner — also used directly by the wasmtime suite — never needs to gate itself.
pub fn run_slow_app_case(lang: &dyn BackendUnderTest, case: &AppCase) {
    run_app_case_inner(lang, case);
}

/// The gzip byte-stdio stress cases (minigzip, the Phase 5b compression CLI): binary stdin/stdout the text-only app cases cannot carry (their `&str` stdin and `include_str!` snapshots require valid UTF-8; a gz stream is neither). Runs under *every* backend — Ruby and Bash both — since it is integer-only (no softfloat) and therefore fast even under Bash. Two cases:
///
/// * *compress* — feed a fixed text input on stdin, require the compressed stdout to be byte-identical to the snapshot captured from `wasmtime` (`examples/apps/snapshots/minigzip_compress.gz`). zlib's gz stream is deterministic here (mtime 0, OS byte 3), so this is a stable equality. * *round trip* — compress, then decompress that output with `-d`, and require the result to equal the original input (self-checking; proves both directions of the binary stdio path).
pub fn run_gzip_cases(lang: &dyn BackendUnderTest) {
    let wasm_path = apps_cache_dir().join("minigzip.wasm");
    assert!(
        wasm_path.exists(),
        "minigzip not cached — run examples/apps/fetch-and-build.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let src = lang.convert_app(&bytes, Mode::Standalone, "minigzip");

    let input = std::fs::read(apps_fixtures_dir().join("gzip").join("input.txt"))
        .expect("read gzip input fixture");
    let snapshot = std::fs::read(apps_snapshot_dir().join("minigzip_compress.gz"))
        .expect("read minigzip snapshot");

    // compress: stdout must be byte-identical to the wasmtime snapshot.
    let compressed = lang.run_bytes(&src, &[], &input);
    assert!(
        compressed.status.success(),
        "minigzip compress under {}: nonzero exit {}\n{}",
        lang.name(),
        compressed.status,
        String::from_utf8_lossy(&compressed.stderr)
    );
    assert_eq!(
        compressed.stdout,
        snapshot,
        "minigzip compress under {}: stdout differs from the wasmtime snapshot (byte count {} vs {})",
        lang.name(),
        compressed.stdout.len(),
        snapshot.len()
    );

    // round trip: decompress the just-produced stream back to the original.
    let restored = lang.run_bytes(&src, &["-d"], &compressed.stdout);
    assert!(
        restored.status.success(),
        "minigzip decompress under {}: nonzero exit {}\n{}",
        lang.name(),
        restored.status,
        String::from_utf8_lossy(&restored.stderr)
    );
    assert_eq!(
        restored.stdout,
        input,
        "minigzip round trip under {}: decompressed output differs from the original input",
        lang.name()
    );
    println!("minigzip compress + round trip under {}: ok", lang.name());
}
