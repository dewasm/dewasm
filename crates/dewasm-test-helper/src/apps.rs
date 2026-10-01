//! End-to-end cases over real-world apps (examples/apps/).
//! Each case converts a cached app with a backend.
//! It requires byte-identical `stdout` and exit status against a snapshot output.
//! The snapshot was captured once from Wasmtime and checked into `examples/apps/snapshots/`.
//! Running these does not itself need `wasmtime` installed.
//!
//! Missing requirements fail the test, they don't skip it.
//! Those are the interpreter, or the cache populated by `examples/apps/setup.sh`.
//! Each case is a `pub const` [`AppCase`] driven by its own per-case macro.
//! The macros are `cowsay_args_e2e!`, `cowsay_stdin_e2e!`, `qjs_eval_e2e!`, `sqlite3_shell_e2e!`.
//! `qjs_eval_e2e!`/`sqlite3_shell_e2e!` are slow.
//! Softfloat makes QuickJS/SQLite take tens of seconds under Bash.
//! So the macro expands their generated `#[test]` as `#[ignore]`d by default.
//! It does so unless the expanding backend crate's `slow_test` feature is enabled.
//! [`run_slow_app_case`] just runs the case unconditionally.
//! The conditioning now lives at the macro/feature level.

use dewasm_backend::Mode;

use crate::backend::BackendUnderTest;
use crate::fixtures::{apps_cache_dir, apps_fixtures_dir, apps_snapshot_dir};

pub struct AppCase {
    pub name: &'static str,
    pub args: &'static [&'static str],
    pub stdin: &'static str,
    /// Captured once via `wasmtime run`.
    /// It is the snapshot reference this case's generated-language output must match exactly.
    pub expect_stdout: &'static str,
    pub expect_code: i32,
}

pub const COWSAY_ARGS: AppCase = AppCase {
    name: "cowsay",
    args: &["Hello", "from", "dewasm!"],
    stdin: "",
    expect_stdout: include_str!("../../../examples/apps/snapshots/cowsay_args.stdout"),
    expect_code: 0,
};

pub const COWSAY_STDIN: AppCase = AppCase {
    name: "cowsay",
    args: &[],
    stdin: "moo via stdin\n",
    expect_stdout: include_str!("../../../examples/apps/snapshots/cowsay_stdin.stdout"),
    expect_code: 0,
};

/// QuickJS `-e` one-liner evaluation.
/// Slow: softfloat-bound interpreters skip by default, see [`run_slow_app_case`].
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

/// sqlite3 shell against an in-memory database.
/// Slow: softfloat-bound interpreters skip by default, see [`run_slow_app_case`].
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

/// The same script on `cache/sqlite3-mod.wasm`.
/// That shell is built with the hot VDBE opcode bodies split into their own functions.
/// The patch is `examples/apps/src/sqlite3-vdbe-split.patch`.
/// The split is a code-shape change only.
/// So this case takes the stock shell's snapshot as its expectation.
/// The two artifacts must print the same bytes.
pub const SQLITE3_MOD_SHELL: AppCase = AppCase {
    name: "sqlite3-mod",
    args: SQLITE3_SHELL.args,
    stdin: SQLITE3_SHELL.stdin,
    expect_stdout: SQLITE3_SHELL.expect_stdout,
    expect_code: SQLITE3_SHELL.expect_code,
};

/// CRuby packed by `wasi-vfs`: `cache/ruby.wasm`, its standard library embedded at guest `/usr`.
/// `wasi-vfs pack` embeds it, and this is ruby.wasm's intended self-contained distribution shape.
/// It needs no preopens: a `require` from the standard library proves the embedded VFS serves it.
/// So it is a plain [`AppCase`].
/// The unpacked [`CRUBY_HELLO`](crate::CRUBY_HELLO) is an `FsAppCase` instead.
/// Expected `stdout` is inline like the other interpreter hellos (deterministic one-liner).
/// The Wasmtime suite revalidates it against a live engine.
/// Slow, same as the unpacked case.
/// mruby `-e` evaluation driving raise, rescue, ensure, a custom exception class, and retry.
/// The cached mruby is built with LLVM's `setjmp`/`longjmp` lowering onto exception handling.
/// That is the exception-handling proposal.
/// So `try_table`/`throw` run in a real interpreter here, not only in the specification harness.
/// Expected `stdout` is inline like the other interpreter hellos (deterministic).
/// The Wasmtime suite revalidates it against a live engine.
pub const MRUBY_EH: AppCase = AppCase {
    name: "mruby",
    args: &[
        "-e",
        "class RetryableError < StandardError; end\n\
         begin\n  raise \"boom\"\nrescue => e\n  puts \"rescued:#{e.message}\"\nend\n\
         begin\n  begin\n    raise \"boom2\"\n  ensure\n    puts \"ensured\"\n  end\nrescue => e\n  puts \"rescued2:#{e.message}\"\nend\n\
         attempts = 0\n\
         begin\n  attempts += 1\n  raise RetryableError, \"custom\" if attempts == 1\n  puts \"retried:#{attempts}\"\nrescue RetryableError => e\n  puts \"caught:#{e.class}:#{e.message}\"\n  retry\nend\n\
         puts \"mruby eh e2e: ok\"",
    ],
    stdin: "",
    expect_stdout: "rescued:boom\nensured\nrescued2:boom2\ncaught:RetryableError:custom\nretried:2\nmruby eh e2e: ok\n",
    expect_code: 0,
};

pub const CRUBY_PACKED_HELLO: AppCase = AppCase {
    name: "ruby-packed",
    args: &[
        "-e",
        r#"require "json"; puts JSON.generate({app: "ruby-packed", answer: 6*7})"#,
    ],
    stdin: "",
    expect_stdout: "{\"app\":\"ruby-packed\",\"answer\":42}\n",
    expect_code: 0,
};

fn run_app_case_inner(lang: &dyn BackendUnderTest, case: &AppCase) {
    let wasm_path = apps_cache_dir().join(format!("{}.wasm", case.name));
    assert!(
        wasm_path.exists(),
        "{} not cached: run examples/apps/setup.sh (see docs/testing.md)",
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

/// Rerun an [`AppCase`] under `lang` (the Wasmtime engine) and return its raw `stdout`.
/// Those are the bytes to write into the case's snapshot file.
/// Used by `cargo xtask update-snapshots`; the compare-only `apps` suites never call it.
/// Fails loud on a missing cache or a capture whose exit status is not the case's `expect_code`.
pub fn capture_app_stdout(lang: &dyn BackendUnderTest, case: &AppCase) -> Vec<u8> {
    let wasm_path = apps_cache_dir().join(format!("{}.wasm", case.name));
    assert!(
        wasm_path.exists(),
        "{} not cached: run examples/apps/setup.sh (see docs/testing.md)",
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

/// Rerun the `gzip` *compress* case under `lang` and return its raw compressed `stdout`.
/// Those are the bytes for `examples/apps/snapshots/minigzip_compress.gz`.
/// Fails loud on a missing cache or a nonzero exit.
pub fn capture_gzip_compress(lang: &dyn BackendUnderTest) -> Vec<u8> {
    let wasm_path = apps_cache_dir().join("minigzip.wasm");
    assert!(
        wasm_path.exists(),
        "minigzip not cached: run examples/apps/setup.sh (see docs/testing.md)"
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

/// Run a slow [`AppCase`] (`QJS_EVAL`/`SQLITE3_SHELL`) for `lang` unconditionally.
/// Identical to [`run_app_case`]: the skip for speed lives on the macro, not here.
/// So the Wasmtime suite can call either directly.
pub fn run_slow_app_case(lang: &dyn BackendUnderTest, case: &AppCase) {
    run_app_case_inner(lang, case);
}

/// The `gzip` byte-stdio stress cases (`minigzip`, the compression CLI).
/// They carry binary `stdin`/`stdout`, which the text-only app cases cannot.
/// Those cases' `&str` `stdin` and `include_str!` snapshots require valid UTF-8.
/// A `.gz` stream is neither.
/// Every backend runs it; integer-only, so fast even under Bash.
/// Two cases:
///
/// * *compress*: feed a fixed text input on `stdin`.
///   Require the compressed `stdout` to be byte-identical to the snapshot captured from `wasmtime`.
///   That snapshot is `examples/apps/snapshots/minigzip_compress.gz`.
///   `zlib`'s `.gz` stream is deterministic here (`mtime` 0, OS byte 3), so equality is stable.
/// * *round trip*: compress, then decompress that output with `-d`.
///   Require the result to equal the original input.
///   It is self-checking and proves both directions of the binary stdio path.
pub fn run_gzip_cases(lang: &dyn BackendUnderTest) {
    let wasm_path = apps_cache_dir().join("minigzip.wasm");
    assert!(
        wasm_path.exists(),
        "minigzip not cached: run examples/apps/setup.sh (see docs/testing.md)"
    );
    let bytes = std::fs::read(&wasm_path).expect("read wasm");
    let src = lang.convert_app(&bytes, Mode::Standalone, "minigzip");

    let input = std::fs::read(apps_fixtures_dir().join("gzip").join("input.txt"))
        .expect("read gzip input fixture");
    let snapshot = std::fs::read(apps_snapshot_dir().join("minigzip_compress.gz"))
        .expect("read minigzip snapshot");

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
