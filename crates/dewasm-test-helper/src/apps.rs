//! End-to-end cases over real-world apps (examples/apps/, ADR-9): convert
//! each cached app with a backend and require byte-identical stdout and exit
//! status against a golden output captured once from wasmtime and checked
//! into `examples/apps/golden/` (ADR-15) — running these does not itself need
//! `wasmtime` installed.
//!
//! Per ADR-15, missing prerequisites (the interpreter, or the cache populated
//! by `examples/apps/fetch.sh`) fail the test, they don't skip it. Each case
//! is a `pub const` [`AppCase`] driven by its own per-case macro
//! (`cowsay_args_e2e!`, `cowsay_stdin_e2e!`, `qjs_eval_e2e!`,
//! `sqlite3_shell_e2e!`, ADR-27 revision). `qjs_eval_e2e!`/`sqlite3_shell_e2e!`
//! are heavy — skipped for slow backends by default (softfloat makes
//! QuickJS/SQLite take tens of seconds) via [`run_heavy_app_case`], which
//! gates on `lang.run_heavy_apps()` or `DEWASM_APPS_ALL=1`.

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, apps_golden_dir};

pub struct AppCase {
    pub name: &'static str,
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    /// Captured once via `wasmtime run` (ADR-15); the golden reference this
    /// case's generated-language output must match exactly.
    pub expect_stdout: &'static str,
    pub expect_code: i32,
}

/// cowsay driven purely by argv.
pub const COWSAY_ARGS: AppCase = AppCase {
    name: "cowsay",
    args: &["Hello", "from", "dewasm!"],
    stdin: "",
    expect_stdout: include_str!("../../../examples/apps/golden/cowsay_args.stdout"),
    expect_code: 0,
};

/// cowsay reading its message from stdin.
pub const COWSAY_STDIN: AppCase = AppCase {
    name: "cowsay",
    args: &[],
    stdin: "moo via stdin\n",
    expect_stdout: include_str!("../../../examples/apps/golden/cowsay_stdin.stdout"),
    expect_code: 0,
};

/// QuickJS `-e` one-liner eval (heavy: softfloat-bound interpreters skip by
/// default, see [`run_heavy_app_case`]).
pub const QJS_EVAL: AppCase = AppCase {
    name: "qjs",
    args: &[
        "-e",
        r#"console.log("2^16 =", Math.pow(2, 16)); console.log(JSON.stringify([3,1,2].sort()));"#,
    ],
    stdin: "",
    expect_stdout: include_str!("../../../examples/apps/golden/qjs.stdout"),
    expect_code: 0,
};

/// sqlite3 shell against an in-memory database (heavy: softfloat-bound
/// interpreters skip by default, see [`run_heavy_app_case`]).
pub const SQLITE3_SHELL: AppCase = AppCase {
    name: "sqlite3-shell",
    args: &[],
    stdin: "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\n\
            INSERT INTO users (name) VALUES (\"alice\"), (\"bob\"), (\"carol\");\n\
            SELECT id, upper(name) FROM users WHERE id >= 2;\n\
            SELECT count(*), avg(id) FROM users;\n",
    expect_stdout: include_str!("../../../examples/apps/golden/sqlite3_shell.stdout"),
    expect_code: 0,
};

/// Convert the cached app `case` names with `lang`'s backend, run it, and diff
/// against the case's golden output.
fn run_app_case_inner(lang: &dyn BackendUnderTest, case: &AppCase) {
    let wasm_path = apps_cache_dir().join(format!("{}.wasm", case.name));
    assert!(
        wasm_path.exists(),
        "{} not cached — run examples/apps/fetch.sh (see docs/testing.md)",
        case.name
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let src = lang.convert_app(&bytes, Mode::Standalone, case.name);
    let output = lang.run(&src, case.args, case.stdin);

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        case.expect_stdout,
        "{} {:?} under {}: stdout differs from the golden output",
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
        "{} {:?} under {}: matches golden output",
        case.name,
        case.args,
        lang.name()
    );
}

/// Run a non-heavy [`AppCase`] (`COWSAY_ARGS`/`COWSAY_STDIN`) for `lang`
/// unconditionally.
pub fn run_app_case(lang: &dyn BackendUnderTest, case: &AppCase) {
    run_app_case_inner(lang, case);
}

/// Run a heavy [`AppCase`] (`QJS_EVAL`/`SQLITE3_SHELL`) for `lang`, gated on
/// `lang.run_heavy_apps()` or `DEWASM_APPS_ALL=1`. See
/// [`run_heavy_app_case_forced`] for the ungated entry point.
pub fn run_heavy_app_case(lang: &dyn BackendUnderTest, case: &AppCase) {
    let run_heavy = lang.run_heavy_apps() || std::env::var("DEWASM_APPS_ALL").is_ok();
    if !run_heavy {
        println!(
            "{} {:?}: heavy case skipped for {} (DEWASM_APPS_ALL=1 to run)",
            case.name,
            case.args,
            lang.name()
        );
        return;
    }
    run_app_case_inner(lang, case);
}

/// Run a heavy [`AppCase`] for `lang` unconditionally (ignoring the
/// `DEWASM_APPS_ALL` gate). Used by the wasmtime suite, whose `wasmtime_test`
/// feature is already the opt-in — requiring both flags would be
/// unergonomic (mirrors [`crate::run_fs_app_case_forced`]).
pub fn run_heavy_app_case_forced(lang: &dyn BackendUnderTest, case: &AppCase) {
    run_app_case_inner(lang, case);
}

/// The gzip byte-stdio stress cases (minigzip, the Phase 5b compression CLI):
/// binary stdin/stdout the text-only app cases cannot carry (their `&str`
/// stdin and `include_str!` goldens require valid UTF-8; a gz stream is
/// neither). Runs under *every* backend — Ruby and Bash both — since it is
/// integer-only (no softfloat) and therefore fast even under Bash. Two cases:
///
///   * *compress* — feed a fixed text input on stdin, require the compressed
///     stdout to be byte-identical to the golden captured from `wasmtime`
///     (`examples/apps/golden/minigzip_compress.gz`). zlib's gz stream is
///     deterministic here (mtime 0, OS byte 3), so this is a stable equality.
///   * *round trip* — compress, then decompress that output with `-d`, and
///     require the result to equal the original input (self-checking; proves
///     both directions of the binary stdio path).
pub fn run_gzip_cases(lang: &dyn BackendUnderTest) {
    let wasm_path = apps_cache_dir().join("minigzip.wasm");
    assert!(
        wasm_path.exists(),
        "minigzip not cached — run examples/apps/fetch.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let src = lang.convert_app(&bytes, Mode::Standalone, "minigzip");

    let input = std::fs::read(apps_fixtures_dir().join("gzip").join("input.txt"))
        .expect("read gzip input fixture");
    let golden = std::fs::read(apps_golden_dir().join("minigzip_compress.gz"))
        .expect("read minigzip golden");

    // compress: stdout must be byte-identical to the wasmtime golden.
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
        golden,
        "minigzip compress under {}: stdout differs from the wasmtime golden (byte count {} vs {})",
        lang.name(),
        compressed.stdout.len(),
        golden.len()
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
